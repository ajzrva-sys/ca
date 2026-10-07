use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

const MAX_ENTRIES: usize = 256;
const MAX_TEXT_BYTES: usize = 4 * 1024;
const MAX_STORE_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScratchCategory {
    Note,
    Todo,
    Plan,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScratchEntry {
    pub id: uuid::Uuid,
    pub category: ScratchCategory,
    pub text: Option<String>,
    #[serde(default)]
    pub done: bool,
    pub plan_id: Option<String>,
    pub plan_revision: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    version: u32,
    workspace: String,
    entries: Vec<ScratchEntry>,
}

#[derive(Debug, Clone)]
pub struct ScratchStore {
    path: PathBuf,
    workspace: String,
    writable: bool,
}

static ACTIVE: std::sync::OnceLock<std::sync::RwLock<Option<ScratchStore>>> =
    std::sync::OnceLock::new();

pub fn set_active(store: ScratchStore) {
    *ACTIVE
        .get_or_init(|| std::sync::RwLock::new(None))
        .write()
        .unwrap_or_else(|e| e.into_inner()) = Some(store);
}

pub fn read_active(category: Option<ScratchCategory>) -> Result<String, String> {
    let guard = ACTIVE
        .get_or_init(|| std::sync::RwLock::new(None))
        .read()
        .unwrap_or_else(|e| e.into_inner());
    guard
        .as_ref()
        .ok_or_else(|| "project scratch is unavailable".to_string())?
        .render(category)
}

pub fn active_store() -> Option<ScratchStore> {
    ACTIVE
        .get_or_init(|| std::sync::RwLock::new(None))
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

impl ScratchStore {
    pub fn new(profile_data: &Path, workspace: &Path, writable: bool) -> Result<Self, String> {
        let workspace = workspace
            .canonicalize()
            .map_err(|e| format!("scratch workspace is unavailable: {e}"))?;
        let identity = workspace.to_string_lossy().into_owned();
        let key = Sha256::digest(identity.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        Ok(Self {
            path: profile_data.join("scratch").join(format!("{key}.json")),
            workspace: identity,
            writable,
        })
    }

    pub fn unavailable(workspace: &Path) -> Self {
        Self {
            path: PathBuf::new(),
            workspace: workspace.to_string_lossy().into_owned(),
            writable: false,
        }
    }

    pub fn writable(&self) -> bool {
        self.writable
    }

    pub fn load(&self) -> Result<Vec<ScratchEntry>, String> {
        if self.path.as_os_str().is_empty() || !self.path.exists() {
            return Ok(Vec::new());
        }
        let meta = fs::symlink_metadata(&self.path)
            .map_err(|e| format!("could not inspect scratch data: {e}"))?;
        if !meta.file_type().is_file() {
            return Err("scratch data is not a regular file".into());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if meta.mode() & 0o077 != 0 {
                return Err("scratch data permissions are not owner-only".into());
            }
        }
        if meta.len() as usize > MAX_STORE_BYTES {
            return Err("scratch data exceeds 256 KiB".into());
        }
        let data = fs::read(&self.path).map_err(|e| format!("could not read scratch data: {e}"))?;
        let doc: Document =
            serde_json::from_slice(&data).map_err(|e| format!("scratch data is invalid: {e}"))?;
        if doc.version != 1 || doc.workspace != self.workspace {
            return Err("scratch data does not match this workspace/version".into());
        }
        validate(&doc.entries)?;
        Ok(doc.entries)
    }

    pub fn add_text(&self, category: ScratchCategory, text: &str) -> Result<ScratchEntry, String> {
        if category == ScratchCategory::Plan {
            return Err("plans require an ID and revision".into());
        }
        valid_text(text)?;
        let mut entries = self.load()?;
        let entry = ScratchEntry {
            id: uuid::Uuid::new_v4(),
            category,
            text: Some(text.into()),
            done: false,
            plan_id: None,
            plan_revision: None,
        };
        entries.push(entry.clone());
        self.save(&entries)?;
        Ok(entry)
    }

    pub fn save_plan(&self, id: &str, revision: u64) -> Result<ScratchEntry, String> {
        valid_text(id)?;
        let mut entries = self.load()?;
        let entry = ScratchEntry {
            id: uuid::Uuid::new_v4(),
            category: ScratchCategory::Plan,
            text: None,
            done: false,
            plan_id: Some(id.into()),
            plan_revision: Some(revision),
        };
        entries.push(entry.clone());
        self.save(&entries)?;
        Ok(entry)
    }

    pub fn delete(&self, id: uuid::Uuid) -> Result<(), String> {
        let mut entries = self.load()?;
        let before = entries.len();
        entries.retain(|e| e.id != id);
        if entries.len() == before {
            return Err("scratch entry was not found".into());
        }
        self.save(&entries)
    }

    pub fn toggle(&self, id: uuid::Uuid) -> Result<(), String> {
        let mut entries = self.load()?;
        let entry = entries
            .iter_mut()
            .find(|e| e.id == id && e.category == ScratchCategory::Todo)
            .ok_or("scratch todo was not found")?;
        entry.done = !entry.done;
        self.save(&entries)
    }

    pub fn edit(&self, id: uuid::Uuid, text: &str) -> Result<(), String> {
        valid_text(text)?;
        let mut entries = self.load()?;
        let entry = entries
            .iter_mut()
            .find(|e| e.id == id && e.category != ScratchCategory::Plan)
            .ok_or("scratch note/todo was not found")?;
        entry.text = Some(text.into());
        self.save(&entries)
    }

    pub fn render(&self, category: Option<ScratchCategory>) -> Result<String, String> {
        let entries = self.load()?;
        let mut out = String::from("Project scratch\n");
        let mut count = 0;
        for entry in entries
            .iter()
            .filter(|e| category.is_none_or(|c| c == e.category))
        {
            count += 1;
            let line = match entry.category {
                ScratchCategory::Note => {
                    format!("note {}: {}", entry.id, entry.text.as_deref().unwrap_or(""))
                }
                ScratchCategory::Todo => format!(
                    "todo {} [{}]: {}",
                    entry.id,
                    if entry.done { "done" } else { "pending" },
                    entry.text.as_deref().unwrap_or("")
                ),
                ScratchCategory::Plan => format!(
                    "plan {}: {} @ revision {}",
                    entry.id,
                    entry.plan_id.as_deref().unwrap_or(""),
                    entry.plan_revision.unwrap_or(0)
                ),
            };
            out.push_str(&crate::terminal_text::sanitize_terminal_text(&line));
            out.push('\n');
        }
        if count == 0 {
            out.push_str("(empty)\n");
        }
        Ok(bound(out, 16 * 1024))
    }

    fn save(&self, entries: &[ScratchEntry]) -> Result<(), String> {
        if !self.writable {
            return Err("scratch persistence is unavailable in bare mode".into());
        }
        validate(entries)?;
        let data = serde_json::to_vec_pretty(&Document {
            version: 1,
            workspace: self.workspace.clone(),
            entries: entries.to_vec(),
        })
        .map_err(|e| e.to_string())?;
        if data.len() > MAX_STORE_BYTES {
            return Err("scratch data exceeds 256 KiB".into());
        }
        let parent = self.path.parent().ok_or("scratch path has no parent")?;
        if parent.exists()
            && fs::symlink_metadata(parent)
                .map_err(|e| e.to_string())?
                .file_type()
                .is_symlink()
        {
            return Err("scratch directory cannot be a symlink".into());
        }
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(parent, fs::Permissions::from_mode(0o700))
                .map_err(|e| e.to_string())?;
        }
        if self.path.exists()
            && fs::symlink_metadata(&self.path)
                .map_err(|e| e.to_string())?
                .file_type()
                .is_symlink()
        {
            return Err("scratch data cannot be a symlink".into());
        }
        let tmp = parent.join(format!(".scratch-{}.tmp", uuid::Uuid::new_v4()));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&tmp).map_err(|e| e.to_string())?;
        let result = (|| {
            file.write_all(&data).map_err(|e| e.to_string())?;
            file.sync_all().map_err(|e| e.to_string())?;
            fs::rename(&tmp, &self.path).map_err(|e| e.to_string())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        result
    }
}

fn valid_text(text: &str) -> Result<(), String> {
    if text.trim().is_empty() || text.len() > MAX_TEXT_BYTES || text.chars().any(char::is_control) {
        Err("scratch text must be nonblank, control-free, and at most 4 KiB".into())
    } else {
        Ok(())
    }
}
fn validate(entries: &[ScratchEntry]) -> Result<(), String> {
    if entries.len() > MAX_ENTRIES {
        return Err("scratch data exceeds 256 entries".into());
    }
    for e in entries {
        match e.category {
            ScratchCategory::Note | ScratchCategory::Todo => {
                valid_text(e.text.as_deref().unwrap_or(""))?
            }
            ScratchCategory::Plan => valid_text(e.plan_id.as_deref().unwrap_or(""))?,
        }
    }
    Ok(())
}
fn bound(mut value: String, max: usize) -> String {
    if value.len() <= max {
        return value;
    }
    let marker = "\n[truncated]\n";
    let mut end = max - marker.len();
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value.truncate(end);
    value.push_str(marker);
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scratch_store_is_private_bounded_and_workspace_keyed() {
        let root = tempfile::tempdir().unwrap();
        let ws = tempfile::tempdir().unwrap();
        let store = ScratchStore::new(root.path(), ws.path(), true).unwrap();
        let note = store
            .add_text(ScratchCategory::Note, "remember 🙂")
            .unwrap();
        let todo = store.add_text(ScratchCategory::Todo, "run tests").unwrap();
        store.toggle(todo.id).unwrap();
        store.save_plan("plan-1", 7).unwrap();
        let text = store.render(None).unwrap();
        assert!(text.contains("remember 🙂"));
        assert!(text.contains("[done]"));
        assert!(text.contains("plan-1 @ revision 7"));
        store.delete(note.id).unwrap();
        assert!(!store.render(None).unwrap().contains("remember"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(store.path.parent().unwrap())
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
            assert_eq!(
                fs::metadata(&store.path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        assert!(store
            .add_text(ScratchCategory::Note, &"x".repeat(MAX_TEXT_BYTES + 1))
            .is_err());
    }
    #[test]
    fn bare_scratch_refuses_persistence() {
        let ws = tempfile::tempdir().unwrap();
        let store = ScratchStore::unavailable(ws.path());
        assert!(!store.writable());
        assert!(store.add_text(ScratchCategory::Note, "no").is_err());
        assert!(store.render(None).unwrap().contains("empty"));
    }

    #[tokio::test]
    async fn read_scratch_tool_is_read_only_filtered_and_bounded() {
        let root = tempfile::tempdir().unwrap();
        let ws = tempfile::tempdir().unwrap();
        let store = ScratchStore::new(root.path(), ws.path(), true).unwrap();
        store.add_text(ScratchCategory::Note, "human note").unwrap();
        store.add_text(ScratchCategory::Todo, "human todo").unwrap();
        set_active(store);
        let context = crate::agent::registry::ToolContext {
            recovery: crate::owned_tools::current_recovery(),
            cwd: ws.path().to_path_buf(),
            timeout_secs: 1,
            profile: crate::agent::registry::ToolProfile::Main,
            workspace_roots: vec![ws.path().to_path_buf()],
            sandbox_mode: crate::sandbox::SandboxMode::PolicyOnly,
            network_allowed: false,
            job_owner: crate::jobs::JobOwner::new(None),
            job_execution_policy: crate::jobs::JobExecutionPolicy::default(),
        };
        let result = crate::agent::tools::execute_tool_with_context(
            "read_scratch",
            &serde_json::json!({"category":"notes"}),
            &context,
        )
        .await;
        assert!(result.is_success());
        assert!(result.model_text().contains("human note"));
        assert!(!result.model_text().contains("human todo"));
        assert_eq!(
            crate::agent::registry::ToolRegistry::builtins()
                .get("read_scratch")
                .unwrap()
                .mutation,
            crate::agent::registry::ToolMutation::ReadOnly
        );
    }
}
