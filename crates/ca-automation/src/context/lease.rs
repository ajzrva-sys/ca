//! Bounded file leases for Worker context packs.
//!
//! Owns `FileLeaseSet`, `RepoHintCache`, path-safe lease expansion, and capped
//! file-content rendering for context packs.
//!
//! Read when changing lease caps, path escape denial, or leased content clipping.
//! Do not read for orchestration request routing (that lives in `app`).

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_LEASE_BYTES: usize = 4_000;
const DEFAULT_MAX_LEASED_FILES: usize = 6;
const HINT_CACHE_TTL_SECS: u64 = 300;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct FileLeaseSet {
    pub(crate) task_id: String,
    pub(crate) paths: Vec<String>,
    pub(crate) max_bytes_per_file: usize,
}

impl FileLeaseSet {
    pub(crate) fn for_task(task_id: impl Into<String>, paths: Vec<String>) -> Self {
        Self {
            task_id: task_id.into(),
            paths: paths.into_iter().take(DEFAULT_MAX_LEASED_FILES).collect(),
            max_bytes_per_file: DEFAULT_LEASE_BYTES,
        }
    }

    pub(crate) fn expand(&mut self, requested: &[String]) -> Vec<String> {
        let mut granted = Vec::new();
        for path in requested {
            if self.paths.len() >= DEFAULT_MAX_LEASED_FILES {
                break;
            }
            if path.contains("..") || Path::new(path).is_absolute() {
                continue;
            }
            if !self.paths.iter().any(|existing| existing == path) {
                self.paths.push(path.clone());
                granted.push(path.clone());
            }
        }
        granted
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct RepoHintCache {
    entries: BTreeMap<String, RepoHintEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RepoHintEntry {
    summary: String,
    recorded_at_unix: u64,
}

impl RepoHintCache {
    pub(crate) fn get(&self, key: &str) -> Option<&str> {
        let now = unix_now();
        self.entries.get(key).and_then(|entry| {
            if now.saturating_sub(entry.recorded_at_unix) > HINT_CACHE_TTL_SECS {
                None
            } else {
                Some(entry.summary.as_str())
            }
        })
    }

    pub(crate) fn put(&mut self, key: impl Into<String>, summary: impl Into<String>) {
        self.entries.insert(
            key.into(),
            RepoHintEntry {
                summary: summary.into(),
                recorded_at_unix: unix_now(),
            },
        );
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct LeaseExpansionRequest {
    pub(crate) paths: Vec<String>,
    pub(crate) reason: String,
}

pub(crate) fn render_leased_file_contents(
    root: &Path,
    lease: &FileLeaseSet,
) -> Result<String, String> {
    if lease.paths.is_empty() {
        return Ok("No file contents embedded in the initial Worker prompt.".to_string());
    }
    let mut sections = Vec::new();
    for relative in &lease.paths {
        let absolute = root.join(relative);
        match std::fs::read_to_string(&absolute) {
            Ok(contents) => {
                let clipped = clip_bytes(&contents, lease.max_bytes_per_file);
                sections.push(format!("### {relative}\n\n{clipped}"));
            }
            Err(err) => sections.push(format!("### {relative}\n\n(unavailable: {err})")),
        }
    }
    Ok(sections.join("\n\n"))
}

fn clip_bytes(contents: &str, max_bytes: usize) -> String {
    if contents.len() <= max_bytes {
        return contents.to_string();
    }
    let mut out = String::new();
    let mut used = 0usize;
    for ch in contents.chars() {
        let len = ch.len_utf8();
        if used + len > max_bytes {
            break;
        }
        out.push(ch);
        used += len;
    }
    out.push_str("\n...[lease truncated]...");
    out
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lease_expand_respects_caps_and_path_safety() {
        let mut lease = FileLeaseSet::for_task("T1", vec!["src/a.rs".to_string()]);
        let granted = lease.expand(&[
            "../etc/passwd".to_string(),
            "/tmp/x".to_string(),
            "src/b.rs".to_string(),
            "src/c.rs".to_string(),
            "src/d.rs".to_string(),
            "src/e.rs".to_string(),
            "src/f.rs".to_string(),
            "src/g.rs".to_string(),
        ]);
        assert_eq!(
            granted,
            vec!["src/b.rs", "src/c.rs", "src/d.rs", "src/e.rs", "src/f.rs"]
        );
        assert_eq!(lease.paths.len(), DEFAULT_MAX_LEASED_FILES);
    }

    #[test]
    fn repo_hint_cache_expires() {
        let mut cache = RepoHintCache::default();
        cache.put("foo", "bar");
        assert_eq!(cache.get("foo"), Some("bar"));
        if let Some(entry) = cache.entries.get_mut("foo") {
            entry.recorded_at_unix = 0;
        }
        assert_eq!(cache.get("foo"), None);
    }
}
