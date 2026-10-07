//! Filesystem utility helpers.
//!
//! Owns focused file IO helpers, atomic text writes, directory creation, and
//! safe text/JSONL reads used across the crate.
//!
//! Read when changing generic file read/write behavior. Do not read for queue
//! semantics, verification recipes, or AI context construction.

use anyhow::{Context, Result};
use serde_json::Value;
#[cfg(unix)]
use std::fs::File;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_JSONL_STRING_CHARS: usize = 8 * 1024;

pub(crate) fn read_text(path: &Path) -> Result<String> {
    fs::read_to_string(path).with_context(|| format!("read {}", path.display()))
}

pub(crate) fn write_text_atomic(path: &Path, text: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create dir {}", parent.display()))?;
    }

    let existing_permissions = fs::metadata(path)
        .ok()
        .map(|metadata| metadata.permissions());
    let temp_path = temp_path_for(path);
    let write_result = (|| -> Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
            .with_context(|| format!("create temp file {}", temp_path.display()))?;
        if let Some(permissions) = existing_permissions {
            file.set_permissions(permissions)
                .with_context(|| format!("set permissions {}", temp_path.display()))?;
        }
        file.write_all(text.as_bytes())
            .with_context(|| format!("write temp file {}", temp_path.display()))?;
        file.sync_all()
            .with_context(|| format!("sync temp file {}", temp_path.display()))?;
        drop(file);
        fs::rename(&temp_path, path).with_context(|| {
            format!(
                "rename temp file {} to {}",
                temp_path.display(),
                path.display()
            )
        })?;
        sync_parent_directory(path)?;
        Ok(())
    })();

    if write_result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }

    write_result.with_context(|| format!("write {}", path.display()))
}

#[cfg(unix)]
fn sync_parent_directory(path: &Path) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("atomic text path has no parent"))?;
    File::open(parent)
        .with_context(|| format!("open parent directory {}", parent.display()))?
        .sync_all()
        .with_context(|| format!("sync parent directory {}", parent.display()))
}

#[cfg(not(unix))]
fn sync_parent_directory(_path: &Path) -> Result<()> {
    Ok(())
}

pub(crate) fn append_jsonl_bounded(path: &Path, value: &Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create dir {}", parent.display()))?;
    }

    let mut bounded = value.clone();
    truncate_json_strings(&mut bounded, MAX_JSONL_STRING_CHARS);
    let line = serde_json::to_string(&bounded)?;

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("open jsonl {}", path.display()))?;
    file.write_all(line.as_bytes())
        .with_context(|| format!("append jsonl {}", path.display()))?;
    file.write_all(b"\n")
        .with_context(|| format!("append jsonl newline {}", path.display()))?;
    Ok(())
}

fn truncate_json_strings(value: &mut Value, max_chars: usize) {
    match value {
        Value::String(text) => {
            if text.chars().count() > max_chars {
                *text = truncate_string(text, max_chars);
            }
        }
        Value::Array(items) => {
            for item in items {
                truncate_json_strings(item, max_chars);
            }
        }
        Value::Object(map) => {
            for value in map.values_mut() {
                truncate_json_strings(value, max_chars);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn truncate_string(text: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for ch in text.chars().take(max_chars) {
        out.push(ch);
    }
    out.push_str("\n...[truncated]...");
    out
}

fn temp_path_for(path: &Path) -> PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("tmp");
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    parent.join(format!(".{name}.tmp-{}-{nanos}", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;

    fn temp_root(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("codex-team-fs-util-{name}-{nonce}"))
    }

    #[test]
    fn write_text_atomic_replaces_existing_file() -> Result<()> {
        let root = temp_root("replace");
        let path = root.join("nested").join("state.json");

        write_text_atomic(&path, "old")?;
        write_text_atomic(&path, "new")?;

        assert_eq!(read_text(&path)?, "new");
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn write_text_atomic_creates_parent_directories() -> Result<()> {
        let root = temp_root("parent");
        let path = root.join("a").join("b").join("file.txt");

        write_text_atomic(&path, "hello")?;

        assert_eq!(read_text(&path)?, "hello");
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn write_text_atomic_preserves_existing_permissions() -> Result<()> {
        use std::os::unix::fs::PermissionsExt;

        let root = temp_root("perms");
        fs::create_dir_all(&root)?;
        let path = root.join("script.sh");
        fs::write(&path, "old")?;
        let mut permissions = fs::metadata(&path)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions)?;

        write_text_atomic(&path, "new")?;

        assert_eq!(fs::metadata(&path)?.permissions().mode() & 0o777, 0o755);
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn append_jsonl_bounded_writes_valid_newline_terminated_json() -> Result<()> {
        let root = temp_root("jsonl");
        let path = root.join("events.jsonl");

        append_jsonl_bounded(&path, &serde_json::json!({"kind": "one"}))?;
        append_jsonl_bounded(&path, &serde_json::json!({"kind": "two"}))?;

        let text = read_text(&path)?;
        assert!(text.ends_with('\n'));
        let lines = text.lines().collect::<Vec<_>>();
        assert_eq!(lines.len(), 2);
        assert_eq!(
            serde_json::from_str::<Value>(lines[0])?["kind"],
            Value::String("one".to_string())
        );
        assert_eq!(
            serde_json::from_str::<Value>(lines[1])?["kind"],
            Value::String("two".to_string())
        );
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn append_jsonl_bounded_truncates_large_strings() -> Result<()> {
        let root = temp_root("truncate");
        let path = root.join("events.jsonl");
        let large = "x".repeat(MAX_JSONL_STRING_CHARS + 100);

        append_jsonl_bounded(&path, &serde_json::json!({"message": large}))?;

        let text = read_text(&path)?;
        let parsed = serde_json::from_str::<Value>(text.trim())?;
        let message = parsed["message"].as_str().expect("message string");
        assert!(message.len() < MAX_JSONL_STRING_CHARS + 100);
        assert!(message.contains("[truncated]"));
        fs::remove_dir_all(root)?;
        Ok(())
    }
}
