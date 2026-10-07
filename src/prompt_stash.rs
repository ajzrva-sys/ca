use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub const MAX_ENTRIES: usize = 50;
pub const MAX_ENTRY_BYTES: usize = 64 * 1024;
pub const MAX_AGGREGATE_BYTES: usize = 1024 * 1024;
const MAX_STORE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PromptDraft {
    pub id: uuid::Uuid,
    pub text: String,
    pub created_at: DateTime<Utc>,
}

impl PromptDraft {
    pub fn line_count(&self) -> usize {
        self.text.lines().count().max(1)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    version: u8,
    entries: Vec<PromptDraft>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveOutcome {
    pub entry: PromptDraft,
    pub recovered_corrupt_store: bool,
}

#[derive(Debug, Clone)]
pub struct PromptStashStore {
    path: PathBuf,
    writable: bool,
}

#[derive(Debug)]
enum LoadFailure {
    Corrupt(String),
    Unsafe(String),
}

impl LoadFailure {
    fn message(self) -> String {
        match self {
            Self::Corrupt(message) | Self::Unsafe(message) => message,
        }
    }
}

impl PromptStashStore {
    pub fn for_profile(home: &Path, profile_data: &Path, writable: bool) -> Result<Self, String> {
        let relative = profile_data
            .strip_prefix(home)
            .map_err(|_| "prompt stash profile data is outside HOME".to_string())?;
        if relative
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
        {
            return Err("prompt stash profile path is invalid".into());
        }
        let canonical_home = home
            .canonicalize()
            .map_err(|error| format!("could not resolve HOME for prompt stash: {error}"))?;
        let canonical_profile_data = canonical_home.join(relative);
        validate_profile_components(&canonical_home, &canonical_profile_data)?;
        Ok(Self {
            path: canonical_profile_data.join("input/prompt-stash.json"),
            writable,
        })
    }

    pub fn unavailable() -> Self {
        Self {
            path: PathBuf::new(),
            writable: false,
        }
    }

    pub fn writable(&self) -> bool {
        self.writable
    }

    pub fn load(&self) -> Result<Vec<PromptDraft>, String> {
        self.load_checked().map_err(LoadFailure::message)
    }

    pub fn save_draft(&self, text: &str, now: DateTime<Utc>) -> Result<SaveOutcome, String> {
        self.require_writable()?;
        validate_text(text)?;
        let (mut entries, recovered_corrupt_store) = match self.load_checked() {
            Ok(entries) => (entries, false),
            Err(LoadFailure::Corrupt(_)) => {
                self.quarantine_corrupt()?;
                (Vec::new(), true)
            }
            Err(error) => return Err(error.message()),
        };
        let entry = PromptDraft {
            id: uuid::Uuid::new_v4(),
            text: text.to_string(),
            created_at: now,
        };
        entries.push(entry.clone());
        self.save(&entries)?;
        Ok(SaveOutcome {
            entry,
            recovered_corrupt_store,
        })
    }

    pub fn restore_swap(
        &self,
        id: uuid::Uuid,
        replacement: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<PromptDraft, String> {
        self.require_writable()?;
        if let Some(text) = replacement {
            validate_text(text)?;
        }
        let mut entries = self.load()?;
        let index = entries
            .iter()
            .position(|entry| entry.id == id)
            .ok_or_else(|| "prompt stash entry was not found".to_string())?;
        let restored = entries.remove(index);
        if let Some(text) = replacement {
            entries.push(PromptDraft {
                id: uuid::Uuid::new_v4(),
                text: text.to_string(),
                created_at: now,
            });
        }
        self.save(&entries)?;
        Ok(restored)
    }

    pub fn delete(&self, id: uuid::Uuid) -> Result<(), String> {
        self.require_writable()?;
        let mut entries = self.load()?;
        let before = entries.len();
        entries.retain(|entry| entry.id != id);
        if entries.len() == before {
            return Err("prompt stash entry was not found".into());
        }
        self.save(&entries)
    }

    fn require_writable(&self) -> Result<(), String> {
        if self.writable {
            Ok(())
        } else {
            Err("prompt stash persistence is unavailable in bare mode".into())
        }
    }

    fn load_checked(&self) -> Result<Vec<PromptDraft>, LoadFailure> {
        if self.path.as_os_str().is_empty() || !self.path.exists() {
            return Ok(Vec::new());
        }
        validate_no_symlink_ancestors(&self.path).map_err(LoadFailure::Unsafe)?;
        let metadata = fs::symlink_metadata(&self.path).map_err(|error| {
            LoadFailure::Unsafe(format!("could not inspect prompt stash: {error}"))
        })?;
        if !metadata.file_type().is_file() {
            return Err(LoadFailure::Unsafe(
                "prompt stash is not a regular file".into(),
            ));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 {
                return Err(LoadFailure::Unsafe("prompt stash is not owner-only".into()));
            }
        }
        if metadata.len() as usize > MAX_STORE_BYTES {
            return Err(LoadFailure::Unsafe(
                "prompt stash exceeds its bounded storage limit".into(),
            ));
        }
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        File::open(&self.path)
            .map_err(|error| LoadFailure::Unsafe(format!("could not open prompt stash: {error}")))?
            .take((MAX_STORE_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|error| {
                LoadFailure::Unsafe(format!("could not read prompt stash: {error}"))
            })?;
        if bytes.len() > MAX_STORE_BYTES {
            return Err(LoadFailure::Unsafe(
                "prompt stash exceeds its bounded storage limit".into(),
            ));
        }
        let document: Document = serde_json::from_slice(&bytes)
            .map_err(|_| LoadFailure::Corrupt("prompt stash data is invalid".into()))?;
        if document.version != 1 {
            return Err(LoadFailure::Corrupt(
                "prompt stash version is unsupported".into(),
            ));
        }
        validate_entries(&document.entries).map_err(LoadFailure::Corrupt)?;
        Ok(document.entries)
    }

    fn save(&self, entries: &[PromptDraft]) -> Result<(), String> {
        self.require_writable()?;
        validate_entries(entries)?;
        let bytes = serde_json::to_vec_pretty(&Document {
            version: 1,
            entries: entries.to_vec(),
        })
        .map_err(|error| error.to_string())?;
        if bytes.len() > MAX_STORE_BYTES {
            return Err("prompt stash exceeds its bounded storage limit".into());
        }
        let parent = self
            .path
            .parent()
            .ok_or("prompt stash path has no parent")?;
        validate_no_symlink_ancestors(parent)?;
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        validate_no_symlink_ancestors(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(parent, fs::Permissions::from_mode(0o700))
                .map_err(|error| error.to_string())?;
        }
        if self.path.exists() {
            let metadata = fs::symlink_metadata(&self.path).map_err(|error| error.to_string())?;
            if !metadata.file_type().is_file() {
                return Err("prompt stash is not a regular file".into());
            }
        }
        let temp = parent.join(format!(".prompt-stash-{}.tmp", uuid::Uuid::new_v4()));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temp).map_err(|error| error.to_string())?;
        let result = (|| {
            file.write_all(&bytes).map_err(|error| error.to_string())?;
            file.sync_all().map_err(|error| error.to_string())?;
            fs::rename(&temp, &self.path).map_err(|error| error.to_string())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        result
    }

    fn quarantine_corrupt(&self) -> Result<(), String> {
        validate_no_symlink_ancestors(&self.path)?;
        let metadata = fs::symlink_metadata(&self.path).map_err(|error| error.to_string())?;
        if !metadata.file_type().is_file() || metadata.len() as usize > MAX_STORE_BYTES {
            return Err("unsafe prompt stash cannot be recovered automatically".into());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 {
                return Err("unsafe prompt stash cannot be recovered automatically".into());
            }
        }
        let parent = self
            .path
            .parent()
            .ok_or("prompt stash path has no parent")?;
        let quarantine = parent.join(format!(
            ".prompt-stash-corrupt-{}.json",
            uuid::Uuid::new_v4()
        ));
        fs::rename(&self.path, quarantine).map_err(|error| error.to_string())
    }
}

fn validate_text(text: &str) -> Result<(), String> {
    if text.trim().is_empty() {
        return Err("prompt stash entry cannot be blank".into());
    }
    if text.len() > MAX_ENTRY_BYTES {
        return Err("prompt stash entry exceeds 64 KiB".into());
    }
    if text.contains('\0') {
        return Err("prompt stash entry contains an unsupported control character".into());
    }
    Ok(())
}

fn validate_entries(entries: &[PromptDraft]) -> Result<(), String> {
    if entries.len() > MAX_ENTRIES {
        return Err("prompt stash exceeds 50 entries".into());
    }
    let mut ids = HashSet::new();
    let mut aggregate = 0usize;
    for entry in entries {
        validate_text(&entry.text)?;
        if !ids.insert(entry.id) {
            return Err("prompt stash contains duplicate entry IDs".into());
        }
        aggregate = aggregate.saturating_add(entry.text.len());
    }
    if aggregate > MAX_AGGREGATE_BYTES {
        return Err("prompt stash exceeds 1 MiB total".into());
    }
    Ok(())
}

fn validate_no_symlink_ancestors(path: &Path) -> Result<(), String> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err("prompt stash path cannot contain symlinks".into());
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("could not inspect prompt stash path: {error}")),
        }
    }
    Ok(())
}

fn validate_profile_components(home: &Path, profile_data: &Path) -> Result<(), String> {
    let relative = profile_data
        .strip_prefix(home)
        .map_err(|_| "prompt stash profile data is outside canonical HOME".to_string())?;
    let mut current = home.to_path_buf();
    for component in relative.components() {
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err("prompt stash profile path cannot contain symlinks".into());
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("could not inspect prompt stash profile: {error}")),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn now() -> DateTime<Utc> {
        Utc.timestamp_opt(1_700_000_000, 0).unwrap()
    }

    fn make_store(root: &tempfile::TempDir) -> PromptStashStore {
        PromptStashStore::for_profile(root.path(), &root.path().join("profile"), true).unwrap()
    }

    #[test]
    fn saves_loads_and_swaps_without_persisting_more_than_text() {
        let root = tempfile::tempdir().unwrap();
        let store = make_store(&root);
        let first = store.save_draft("first @src/main.rs#12-40", now()).unwrap();
        let second = store
            .save_draft("second\nline", now() + chrono::Duration::seconds(1))
            .unwrap();
        assert_eq!(store.load().unwrap().len(), 2);
        assert_eq!(second.entry.line_count(), 2);

        let restored = store
            .restore_swap(first.entry.id, Some("current @other.rs"), now())
            .unwrap();
        assert_eq!(restored.text, "first @src/main.rs#12-40");
        let entries = store.load().unwrap();
        assert_eq!(entries.len(), 2);
        assert!(entries
            .iter()
            .any(|entry| entry.text == "current @other.rs"));
        assert!(!entries.iter().any(|entry| entry.id == first.entry.id));
    }

    #[test]
    fn enforces_entry_count_and_aggregate_bounds_atomically() {
        let root = tempfile::tempdir().unwrap();
        let store = make_store(&root);
        assert!(store
            .save_draft(&"x".repeat(MAX_ENTRY_BYTES + 1), now())
            .is_err());
        for index in 0..MAX_ENTRIES {
            store.save_draft(&format!("entry {index}"), now()).unwrap();
        }
        assert!(store.save_draft("overflow", now()).is_err());
        assert_eq!(store.load().unwrap().len(), MAX_ENTRIES);

        let root = tempfile::tempdir().unwrap();
        let store = make_store(&root);
        for index in 0..16 {
            store
                .save_draft(
                    &format!("{index}{}", "x".repeat(MAX_ENTRY_BYTES - 2)),
                    now(),
                )
                .unwrap();
        }
        assert!(store.save_draft("aggregate overflow", now()).is_err());
        assert_eq!(store.load().unwrap().len(), 16);
    }

    #[test]
    fn corrupt_store_is_read_only_until_an_explicit_save_recovers_it() {
        let root = tempfile::tempdir().unwrap();
        let store = make_store(&root);
        fs::create_dir_all(store.path.parent().unwrap()).unwrap();
        fs::write(&store.path, b"not json").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&store.path, fs::Permissions::from_mode(0o600)).unwrap();
        }
        assert!(store.load().is_err());
        assert_eq!(fs::read(&store.path).unwrap(), b"not json");
        let saved = store.save_draft("recovered", now()).unwrap();
        assert!(saved.recovered_corrupt_store);
        assert_eq!(store.load().unwrap()[0].text, "recovered");
        assert!(fs::read_dir(store.path.parent().unwrap())
            .unwrap()
            .flatten()
            .any(|entry| entry
                .file_name()
                .to_string_lossy()
                .starts_with(".prompt-stash-corrupt-")));
    }

    #[test]
    fn bare_and_symlinked_stores_refuse_mutation() {
        let store = PromptStashStore::unavailable();
        assert!(!store.writable());
        assert!(store.save_draft("no", now()).is_err());

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let root = tempfile::tempdir().unwrap();
            let target = tempfile::tempdir().unwrap();
            symlink(target.path(), root.path().join("linked")).unwrap();
            assert!(
                PromptStashStore::for_profile(root.path(), &root.path().join("linked"), true,)
                    .is_err()
            );
        }
    }

    #[test]
    fn store_is_owner_only() {
        let root = tempfile::tempdir().unwrap();
        let store = make_store(&root);
        store.save_draft("private", now()).unwrap();
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
    }
}
