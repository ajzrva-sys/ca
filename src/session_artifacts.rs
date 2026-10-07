use crate::undo::{FileMutationBatchReceipt, FileState};
use crate::workspace::WorkspaceContext;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

pub const ARTIFACT_LEDGER_SCHEMA: &str = "ca.session-artifacts.v1";
pub const MAX_ARTIFACT_ENTRIES: usize = 512;
pub const MAX_ARTIFACT_VIEW_BYTES: u64 = 1024 * 1024;
const MAX_RELATIVE_PATH_BYTES: usize = 1_024;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactAccessKind {
    Created,
    Modified,
    Deleted,
    Read,
}

impl ArtifactAccessKind {
    fn label(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Modified => "modified",
            Self::Deleted => "deleted",
            Self::Read => "read",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactDigestKind {
    Content,
    Receipt,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SessionArtifactEntry {
    pub root_id: String,
    pub relative_path: String,
    pub access: ArtifactAccessKind,
    pub access_count: u32,
    pub first_message_boundary: usize,
    pub last_message_boundary: usize,
    pub first_at: DateTime<Utc>,
    pub last_at: DateTime<Utc>,
    pub size_bytes: Option<u64>,
    pub digest_kind: ArtifactDigestKind,
    pub digest: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SessionArtifactLedger {
    schema: String,
    entries: Vec<SessionArtifactEntry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionArtifactLedgerWire {
    schema: String,
    entries: Vec<SessionArtifactEntry>,
}

impl<'de> Deserialize<'de> for SessionArtifactLedger {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = SessionArtifactLedgerWire::deserialize(deserializer)?;
        let ledger = Self {
            schema: wire.schema,
            entries: wire.entries,
        };
        ledger.validate().map_err(serde::de::Error::custom)?;
        Ok(ledger)
    }
}

impl Default for SessionArtifactLedger {
    fn default() -> Self {
        Self {
            schema: ARTIFACT_LEDGER_SCHEMA.into(),
            entries: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactState {
    Available,
    Deleted,
    Missing,
    Drifted,
    Unsafe,
}

impl ArtifactState {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::Deleted => "deleted",
            Self::Missing => "missing",
            Self::Drifted => "drifted",
            Self::Unsafe => "unsafe",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactInspection {
    pub state: ArtifactState,
    pub current_size: Option<u64>,
}

#[derive(Debug, thiserror::Error)]
pub enum ArtifactError {
    #[error("artifact selector did not match a tracked path")]
    NotFound,
    #[error("artifact workspace root is no longer active")]
    RootUnavailable,
    #[error("artifact path is missing")]
    Missing,
    #[error("artifact path is no longer a regular contained file")]
    Unsafe,
    #[error("artifact has changed since it was tracked")]
    Drifted,
    #[error("artifact exceeds the {MAX_ARTIFACT_VIEW_BYTES} byte viewing limit")]
    Oversized,
    #[error("artifact is not UTF-8 text")]
    NotText,
    #[error(transparent)]
    Io(#[from] io::Error),
}

impl SessionArtifactLedger {
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn entries(&self) -> &[SessionArtifactEntry] {
        &self.entries
    }

    pub fn record_mutation_batch(
        &mut self,
        workspace: &WorkspaceContext,
        mutation: &FileMutationBatchReceipt,
        message_boundary: usize,
        now: DateTime<Utc>,
    ) {
        for receipt in mutation.receipts() {
            let Some((root_id, relative_path)) = contained_identity(workspace, &receipt.path)
            else {
                continue;
            };
            let (access, size_bytes, digest_kind, digest) = match (&receipt.before, &receipt.after)
            {
                (FileState::Missing, FileState::Utf8(content)) => (
                    ArtifactAccessKind::Created,
                    Some(content.len() as u64),
                    ArtifactDigestKind::Content,
                    content_digest(content.as_bytes()),
                ),
                (FileState::Utf8(_), FileState::Utf8(content)) => (
                    ArtifactAccessKind::Modified,
                    Some(content.len() as u64),
                    ArtifactDigestKind::Content,
                    content_digest(content.as_bytes()),
                ),
                (FileState::Utf8(content), FileState::Missing) => (
                    ArtifactAccessKind::Deleted,
                    Some(content.len() as u64),
                    ArtifactDigestKind::Receipt,
                    mutation.digest.clone(),
                ),
                _ => continue,
            };
            self.record(RecordArtifact {
                root_id,
                relative_path,
                access,
                size_bytes,
                digest_kind,
                digest,
                message_boundary,
                now,
            });
        }
    }

    pub fn record_undo_operation(
        &mut self,
        workspace: &WorkspaceContext,
        operation: &crate::undo::UndoOperation,
        message_boundary: usize,
        now: DateTime<Utc>,
    ) {
        let receipts = operation
            .batch
            .receipts()
            .iter()
            .map(|receipt| {
                let (before, after) = match operation.direction {
                    crate::undo::UndoDirection::Undo => {
                        (receipt.after.clone(), receipt.before.clone())
                    }
                    crate::undo::UndoDirection::Redo => {
                        (receipt.before.clone(), receipt.after.clone())
                    }
                };
                crate::undo::FileMutationReceipt::new(
                    receipt.path.clone(),
                    operation.direction.past_tense(),
                    before,
                    after,
                )
            })
            .collect::<Vec<_>>();
        if let Ok(batch) = FileMutationBatchReceipt::new(receipts) {
            self.record_mutation_batch(workspace, &batch, message_boundary, now);
        }
    }

    pub fn record_read(
        &mut self,
        workspace: &WorkspaceContext,
        raw_path: &str,
        message_boundary: usize,
        now: DateTime<Utc>,
    ) -> Result<(), ArtifactError> {
        let path = workspace
            .resolve(raw_path, crate::workspace::PathOperation::Read)
            .map_err(|_| ArtifactError::Unsafe)?;
        let (root_id, relative_path) =
            contained_identity(workspace, &path).ok_or(ArtifactError::Unsafe)?;
        let (size_bytes, digest) =
            digest_regular_contained_file(workspace, &root_id, &relative_path)?;
        self.record(RecordArtifact {
            root_id,
            relative_path,
            access: ArtifactAccessKind::Read,
            size_bytes: Some(size_bytes),
            digest_kind: ArtifactDigestKind::Content,
            digest,
            message_boundary,
            now,
        });
        Ok(())
    }

    pub fn retain_through(&mut self, message_boundary: usize) {
        self.entries
            .retain(|entry| entry.last_message_boundary <= message_boundary);
    }

    pub fn find(&self, selector: &str) -> Result<(usize, &SessionArtifactEntry), ArtifactError> {
        if let Ok(index) = selector.parse::<usize>() {
            return index
                .checked_sub(1)
                .and_then(|index| self.entries.get(index).map(|entry| (index, entry)))
                .ok_or(ArtifactError::NotFound);
        }
        let normalized = selector.replace('\\', "/");
        let mut matches = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.relative_path == normalized);
        let Some(found) = matches.next() else {
            return Err(ArtifactError::NotFound);
        };
        if matches.next().is_some() {
            return Err(ArtifactError::NotFound);
        }
        Ok(found)
    }

    pub fn untrack(&mut self, selector: &str) -> Result<SessionArtifactEntry, ArtifactError> {
        let index = self.find(selector)?.0;
        Ok(self.entries.remove(index))
    }

    pub fn inspect(
        &self,
        workspace: &WorkspaceContext,
        entry: &SessionArtifactEntry,
    ) -> ArtifactInspection {
        if entry.access == ArtifactAccessKind::Deleted {
            let state =
                match resolve_contained_file(workspace, &entry.root_id, &entry.relative_path) {
                    Err(ArtifactError::Missing) => ArtifactState::Deleted,
                    Err(ArtifactError::Unsafe | ArtifactError::RootUnavailable) => {
                        ArtifactState::Unsafe
                    }
                    Err(_) | Ok(_) => ArtifactState::Drifted,
                };
            return ArtifactInspection {
                state,
                current_size: None,
            };
        }
        match digest_regular_contained_file(workspace, &entry.root_id, &entry.relative_path) {
            Ok((size, digest)) => ArtifactInspection {
                state: if entry.digest_kind == ArtifactDigestKind::Content
                    && digest == entry.digest
                    && Some(size) == entry.size_bytes
                {
                    ArtifactState::Available
                } else {
                    ArtifactState::Drifted
                },
                current_size: Some(size),
            },
            Err(ArtifactError::Missing) => ArtifactInspection {
                state: ArtifactState::Missing,
                current_size: None,
            },
            Err(_) => ArtifactInspection {
                state: ArtifactState::Unsafe,
                current_size: None,
            },
        }
    }

    pub fn view_text(
        &self,
        workspace: &WorkspaceContext,
        entry: &SessionArtifactEntry,
    ) -> Result<String, ArtifactError> {
        let state = self.inspect(workspace, entry).state;
        if state != ArtifactState::Available {
            return Err(match state {
                ArtifactState::Missing | ArtifactState::Deleted => ArtifactError::Missing,
                ArtifactState::Drifted => ArtifactError::Drifted,
                ArtifactState::Unsafe => ArtifactError::Unsafe,
                ArtifactState::Available => unreachable!(),
            });
        }
        let path = resolve_contained_file(workspace, &entry.root_id, &entry.relative_path)?;
        let bytes = fs::read(path)?;
        if bytes.len() as u64 > MAX_ARTIFACT_VIEW_BYTES {
            return Err(ArtifactError::Oversized);
        }
        String::from_utf8(bytes).map_err(|_| ArtifactError::NotText)
    }

    pub fn render(&self, _workspace: &WorkspaceContext, filter: Option<&str>) -> String {
        let filter = filter.map(str::to_ascii_lowercase);
        let mut output = String::from("Session artifacts\n\n");
        let mut shown = 0usize;
        for (index, entry) in self.entries.iter().enumerate() {
            if filter.as_ref().is_some_and(|filter| {
                !entry.relative_path.to_ascii_lowercase().contains(filter)
                    && !entry.access.label().contains(filter)
            }) {
                continue;
            }
            let recorded_state = if entry.access == ArtifactAccessKind::Deleted {
                "deleted"
            } else {
                "tracked"
            };
            output.push_str(&format!(
                "{}. {} · {} · {} · {} access{}\n",
                index + 1,
                entry.relative_path,
                entry.access.label(),
                recorded_state,
                entry.access_count,
                if entry.access_count == 1 { "" } else { "es" },
            ));
            shown += 1;
        }
        if shown == 0 {
            output.push_str("No matching tracked artifacts.\n");
        } else {
            output.push_str("\nUse /artifacts inspect|view|copy|untrack <number-or-path>.\n");
        }
        output
    }

    pub fn render_inspection(
        &self,
        workspace: &WorkspaceContext,
        entry: &SessionArtifactEntry,
    ) -> String {
        let inspection = self.inspect(workspace, entry);
        format!(
            "Session artifact\n\nPath: {}\nRoot: {}\nAccess: {}\nCount: {}\nState: {}\nRecorded size: {}\nCurrent size: {}\nDigest: {}:{}\nMessage boundary: {}\nLast observed: {}\n",
            entry.relative_path,
            entry.root_id,
            entry.access.label(),
            entry.access_count,
            inspection.state.label(),
            entry
                .size_bytes
                .map_or_else(|| "unknown".into(), |value| value.to_string()),
            inspection
                .current_size
                .map_or_else(|| "unknown".into(), |value| value.to_string()),
            match entry.digest_kind {
                ArtifactDigestKind::Content => "content",
                ArtifactDigestKind::Receipt => "receipt",
            },
            entry.digest,
            entry.last_message_boundary,
            entry.last_at.to_rfc3339(),
        )
    }

    fn record(&mut self, record: RecordArtifact) {
        if let Some(existing) = self.entries.iter_mut().find(|entry| {
            entry.root_id == record.root_id
                && entry.relative_path == record.relative_path
                && entry.access == record.access
        }) {
            existing.access_count = existing.access_count.saturating_add(1);
            existing.last_message_boundary = record.message_boundary;
            existing.last_at = record.now;
            existing.size_bytes = record.size_bytes;
            existing.digest_kind = record.digest_kind;
            existing.digest = record.digest;
            return;
        }
        if self.entries.len() == MAX_ARTIFACT_ENTRIES {
            self.entries.remove(0);
        }
        self.entries.push(SessionArtifactEntry {
            root_id: record.root_id,
            relative_path: record.relative_path,
            access: record.access,
            access_count: 1,
            first_message_boundary: record.message_boundary,
            last_message_boundary: record.message_boundary,
            first_at: record.now,
            last_at: record.now,
            size_bytes: record.size_bytes,
            digest_kind: record.digest_kind,
            digest: record.digest,
        });
    }

    fn validate(&self) -> Result<(), String> {
        if self.schema != ARTIFACT_LEDGER_SCHEMA {
            return Err("unsupported session artifact schema".into());
        }
        if self.entries.len() > MAX_ARTIFACT_ENTRIES {
            return Err("session artifact ledger exceeds its entry cap".into());
        }
        for entry in &self.entries {
            if uuid::Uuid::parse_str(&entry.root_id).is_err()
                || !valid_relative_path(&entry.relative_path)
                || entry.access_count == 0
                || entry.first_message_boundary > entry.last_message_boundary
                || entry.first_at > entry.last_at
                || !valid_digest(&entry.digest)
            {
                return Err("session artifact entry is malformed".into());
            }
        }
        Ok(())
    }
}

struct RecordArtifact {
    root_id: String,
    relative_path: String,
    access: ArtifactAccessKind,
    size_bytes: Option<u64>,
    digest_kind: ArtifactDigestKind,
    digest: String,
    message_boundary: usize,
    now: DateTime<Utc>,
}

fn contained_identity(workspace: &WorkspaceContext, path: &Path) -> Option<(String, String)> {
    workspace
        .roots()
        .iter()
        .filter_map(|root| {
            let relative = path.strip_prefix(&root.path).ok()?;
            let rendered = relative.to_string_lossy().replace('\\', "/");
            valid_relative_path(&rendered).then_some((root, rendered))
        })
        .max_by_key(|(root, _)| root.path.components().count())
        .map(|(root, relative)| (root.id.to_string(), relative))
}

fn active_root<'a>(workspace: &'a WorkspaceContext, root_id: &str) -> Option<&'a Path> {
    workspace
        .roots()
        .iter()
        .find(|root| root.id.to_string() == root_id)
        .map(|root| root.path.as_path())
}

fn resolve_contained_file(
    workspace: &WorkspaceContext,
    root_id: &str,
    relative_path: &str,
) -> Result<PathBuf, ArtifactError> {
    if !valid_relative_path(relative_path) {
        return Err(ArtifactError::Unsafe);
    }
    let root = active_root(workspace, root_id).ok_or(ArtifactError::RootUnavailable)?;
    let root_metadata = fs::symlink_metadata(root)?;
    if root_metadata.file_type().is_symlink() || !root_metadata.file_type().is_dir() {
        return Err(ArtifactError::Unsafe);
    }
    let mut current = root.to_path_buf();
    for component in Path::new(relative_path).components() {
        let Component::Normal(component) = component else {
            return Err(ArtifactError::Unsafe);
        };
        current.push(component);
        let metadata = match fs::symlink_metadata(&current) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(ArtifactError::Missing);
            }
            Err(error) => return Err(ArtifactError::Io(error)),
        };
        if metadata.file_type().is_symlink() {
            return Err(ArtifactError::Unsafe);
        }
    }
    if !current.is_file() {
        return Err(ArtifactError::Unsafe);
    }
    Ok(current)
}

fn digest_regular_contained_file(
    workspace: &WorkspaceContext,
    root_id: &str,
    relative_path: &str,
) -> Result<(u64, String), ArtifactError> {
    let path = resolve_contained_file(workspace, root_id, relative_path)?;
    let metadata = fs::metadata(&path)?;
    if metadata.len() > MAX_ARTIFACT_VIEW_BYTES {
        return Err(ArtifactError::Oversized);
    }
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 32 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok((metadata.len(), format!("{:x}", hasher.finalize())))
}

fn content_digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_relative_path(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_RELATIVE_PATH_BYTES
        && !value.contains('\0')
        && !Path::new(value).is_absolute()
        && Path::new(value)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::undo::FileMutationReceipt;
    use tempfile::tempdir;

    fn workspace() -> (tempfile::TempDir, WorkspaceContext) {
        let dir = tempdir().unwrap();
        let workspace = WorkspaceContext::from_roots(dir.path(), &[]).unwrap();
        (dir, workspace)
    }

    #[test]
    fn mutation_entries_coalesce_and_cap() {
        let (_dir, workspace) = workspace();
        let root = workspace.primary().unwrap().path.clone();
        let path = root.join("artifact.txt");
        let receipt = FileMutationBatchReceipt::single(FileMutationReceipt::new(
            path,
            "write_file",
            FileState::Missing,
            FileState::Utf8("hello".into()),
        ));
        let now = Utc::now();
        let mut ledger = SessionArtifactLedger::default();
        ledger.record_mutation_batch(&workspace, &receipt, 3, now);
        ledger.record_mutation_batch(&workspace, &receipt, 4, now);
        assert_eq!(ledger.entries.len(), 1);
        assert_eq!(ledger.entries[0].access_count, 2);
        assert_eq!(ledger.entries[0].last_message_boundary, 4);

        for index in 0..=MAX_ARTIFACT_ENTRIES {
            let receipt = FileMutationBatchReceipt::single(FileMutationReceipt::new(
                root.join(format!("{index}.txt")),
                "write_file",
                FileState::Missing,
                FileState::Utf8(index.to_string()),
            ));
            ledger.record_mutation_batch(&workspace, &receipt, index, now);
        }
        assert_eq!(ledger.entries.len(), MAX_ARTIFACT_ENTRIES);
        assert!(!ledger
            .entries
            .iter()
            .any(|entry| entry.relative_path == "artifact.txt"));
    }

    #[test]
    fn reads_are_viewable_only_while_contained_and_unchanged() {
        let (dir, workspace) = workspace();
        let path = dir.path().join("artifact.txt");
        fs::write(&path, "hello").unwrap();
        let mut ledger = SessionArtifactLedger::default();
        ledger
            .record_read(&workspace, "artifact.txt", 2, Utc::now())
            .unwrap();
        let entry = &ledger.entries[0];
        assert_eq!(ledger.view_text(&workspace, entry).unwrap(), "hello");
        fs::write(&path, "changed").unwrap();
        assert!(matches!(
            ledger.view_text(&workspace, entry),
            Err(ArtifactError::Drifted)
        ));
        fs::remove_file(&path).unwrap();
        assert_eq!(
            ledger.inspect(&workspace, entry).state,
            ArtifactState::Missing
        );
    }

    #[cfg(unix)]
    #[test]
    fn viewing_rejects_symlinks() {
        use std::os::unix::fs::symlink;
        let (dir, workspace) = workspace();
        let outside = tempdir().unwrap();
        fs::write(outside.path().join("secret.txt"), "secret").unwrap();
        fs::create_dir(dir.path().join("safe")).unwrap();
        fs::write(dir.path().join("safe/file.txt"), "safe").unwrap();
        let mut ledger = SessionArtifactLedger::default();
        ledger
            .record_read(&workspace, "safe/file.txt", 1, Utc::now())
            .unwrap();
        fs::remove_dir_all(dir.path().join("safe")).unwrap();
        symlink(outside.path(), dir.path().join("safe")).unwrap();
        assert_eq!(
            ledger.inspect(&workspace, &ledger.entries[0]).state,
            ArtifactState::Unsafe
        );
    }

    #[test]
    fn historical_retention_discards_later_entries() {
        let (_dir, workspace) = workspace();
        let root = workspace.primary().unwrap().path.clone();
        let mut ledger = SessionArtifactLedger::default();
        for (name, boundary) in [("a", 2), ("b", 8)] {
            let receipt = FileMutationBatchReceipt::single(FileMutationReceipt::new(
                root.join(name),
                "write_file",
                FileState::Missing,
                FileState::Utf8(name.into()),
            ));
            ledger.record_mutation_batch(&workspace, &receipt, boundary, Utc::now());
        }
        ledger.retain_through(4);
        assert_eq!(ledger.entries.len(), 1);
        assert_eq!(ledger.entries[0].relative_path, "a");
    }

    #[test]
    fn malformed_persisted_ledgers_fail_closed() {
        let invalid = serde_json::json!({
            "schema": ARTIFACT_LEDGER_SCHEMA,
            "entries": [{
                "root_id": uuid::Uuid::new_v4().to_string(),
                "relative_path": "../escape",
                "access": "read",
                "access_count": 1,
                "first_message_boundary": 0,
                "last_message_boundary": 0,
                "first_at": Utc::now(),
                "last_at": Utc::now(),
                "size_bytes": 1,
                "digest_kind": "content",
                "digest": "0".repeat(64),
            }],
        });
        assert!(serde_json::from_value::<SessionArtifactLedger>(invalid).is_err());
    }
}
