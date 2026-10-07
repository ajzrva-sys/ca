use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub const MAX_UNDO_ENTRIES: usize = 20;
pub const MAX_UNDO_FILE_BYTES: usize = 1024 * 1024;
pub const MAX_MUTATION_BATCH_FILES: usize = 64;

#[derive(Clone, PartialEq, Eq)]
pub enum FileState {
    Missing,
    Utf8(String),
}

impl std::fmt::Debug for FileState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing => formatter.write_str("Missing"),
            Self::Utf8(content) => formatter
                .debug_tuple("Utf8")
                .field(&format_args!("{} bytes", content.len()))
                .finish(),
        }
    }
}

impl FileState {
    pub fn utf8(content: String) -> Result<Self, String> {
        if content.len() > MAX_UNDO_FILE_BYTES {
            return Err(format!(
                "file exceeds the {} byte undo limit",
                MAX_UNDO_FILE_BYTES
            ));
        }
        Ok(Self::Utf8(content))
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct FileMutationReceipt {
    id: uuid::Uuid,
    pub path: PathBuf,
    pub action: String,
    pub before: FileState,
    pub after: FileState,
}

impl std::fmt::Debug for FileMutationReceipt {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FileMutationReceipt")
            .field("action", &self.action)
            .field("before", &self.before)
            .field("after", &self.after)
            .finish_non_exhaustive()
    }
}

impl FileMutationReceipt {
    pub fn new(
        path: PathBuf,
        action: impl Into<String>,
        before: FileState,
        after: FileState,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4(),
            path,
            action: action.into(),
            before,
            after,
        }
    }

    pub fn safe_path(&self) -> String {
        self.path.to_string_lossy().chars().take(240).collect()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct FileMutationBatchReceipt {
    id: uuid::Uuid,
    receipts: Vec<FileMutationReceipt>,
    pub action: String,
    pub digest: String,
}

/// A computed typed edit, separated from publication so a recovery intent can
/// be made durable before any target changes. Apply on a blocking worker.
#[derive(Debug, Clone)]
pub struct PreparedFileMutation {
    batch: FileMutationBatchReceipt,
}

impl PreparedFileMutation {
    pub fn from_receipt(batch: FileMutationBatchReceipt) -> Result<Self, String> {
        if batch.receipts().len() > MAX_MUTATION_BATCH_FILES {
            return Err("prepared mutation exceeds its file bound".into());
        }
        for receipt in batch.receipts() {
            for state in [&receipt.before, &receipt.after] {
                if matches!(state, FileState::Utf8(text) if text.len() > MAX_UNDO_FILE_BYTES) {
                    return Err("prepared mutation exceeds its content bound".into());
                }
            }
        }
        Ok(Self { batch })
    }

    pub fn single(
        path: PathBuf,
        action: impl Into<String>,
        before: FileState,
        after: FileState,
    ) -> Result<Self, String> {
        Self::from_receipt(FileMutationBatchReceipt::single(FileMutationReceipt::new(
            path, action, before, after,
        )))
    }

    pub fn receipt(&self) -> &FileMutationBatchReceipt {
        &self.batch
    }

    pub fn apply(self) -> Result<FileMutationBatchReceipt, String> {
        perform(&UndoOperation {
            direction: UndoDirection::Redo,
            batch: self.batch.clone(),
        })?;
        Ok(self.batch)
    }
}

impl std::fmt::Debug for FileMutationBatchReceipt {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FileMutationBatchReceipt")
            .field("action", &self.action)
            .field("file_count", &self.receipts.len())
            .field("digest", &self.digest)
            .finish()
    }
}

impl FileMutationBatchReceipt {
    pub fn new(mut receipts: Vec<FileMutationReceipt>) -> Result<Self, String> {
        if receipts.is_empty() || receipts.len() > MAX_MUTATION_BATCH_FILES {
            return Err(format!(
                "mutation batches require 1-{MAX_MUTATION_BATCH_FILES} files"
            ));
        }
        receipts.sort_by(|left, right| left.path.cmp(&right.path));
        if receipts.windows(2).any(|pair| pair[0].path == pair[1].path) {
            return Err("mutation batch contains duplicate file paths".into());
        }
        let action = if receipts
            .iter()
            .all(|receipt| receipt.action == receipts[0].action)
        {
            receipts[0].action.clone()
        } else {
            "multi_file".to_string()
        };
        let mut hasher = sha2::Sha256::new();
        use sha2::Digest as _;
        hasher.update(b"ca-file-mutation-batch-v1\0");
        for receipt in &receipts {
            hasher.update(receipt.path.as_os_str().as_encoded_bytes());
            hasher.update(b"\0");
            hasher.update(receipt.action.as_bytes());
            hasher.update(b"\0");
            hash_file_state(&mut hasher, &receipt.before);
            hash_file_state(&mut hasher, &receipt.after);
        }
        Ok(Self {
            id: uuid::Uuid::new_v4(),
            receipts,
            action,
            digest: format!("{:x}", hasher.finalize()),
        })
    }

    pub fn single(receipt: FileMutationReceipt) -> Self {
        Self::new(vec![receipt]).expect("one file is a valid mutation batch")
    }

    pub fn receipts(&self) -> &[FileMutationReceipt] {
        &self.receipts
    }

    pub fn persisted_id(&self) -> uuid::Uuid {
        self.id
    }

    pub fn from_persisted(
        receipts: Vec<FileMutationReceipt>,
        id: Option<uuid::Uuid>,
    ) -> Result<Self, String> {
        let mut batch = Self::new(receipts)?;
        if let Some(id) = id {
            if id.is_nil() {
                return Err("persisted mutation identity is empty".into());
            }
            batch.id = id;
        }
        Ok(batch)
    }

    pub fn primary(&self) -> &FileMutationReceipt {
        &self.receipts[0]
    }

    pub fn safe_path(&self) -> String {
        if self.receipts.len() == 1 {
            self.primary().safe_path()
        } else {
            format!("{} files", self.receipts.len())
        }
    }
}

impl From<FileMutationReceipt> for FileMutationBatchReceipt {
    fn from(receipt: FileMutationReceipt) -> Self {
        Self::single(receipt)
    }
}

fn hash_file_state(hasher: &mut sha2::Sha256, state: &FileState) {
    use sha2::Digest as _;
    match state {
        FileState::Missing => hasher.update(b"missing\0"),
        FileState::Utf8(content) => {
            hasher.update(b"utf8\0");
            hasher.update(sha2::Sha256::digest(content.as_bytes()));
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UndoDirection {
    Undo,
    Redo,
}

impl UndoDirection {
    pub fn past_tense(self) -> &'static str {
        match self {
            Self::Undo => "Undid",
            Self::Redo => "Redid",
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct UndoOperation {
    pub direction: UndoDirection,
    pub batch: FileMutationBatchReceipt,
}

impl UndoOperation {
    /// Persisted receipt members are rebuilt with fresh process identities;
    /// selection is bound by the batch identity and exact mutation digest.
    pub fn matches(&self, other: &Self) -> bool {
        self.direction == other.direction
            && self.batch.persisted_id() == other.batch.persisted_id()
            && self.batch.digest == other.batch.digest
    }

    pub fn prepared_mutation(&self) -> Result<PreparedFileMutation, String> {
        let receipts = self
            .batch
            .receipts()
            .iter()
            .map(|receipt| {
                let (before, after) = match self.direction {
                    UndoDirection::Undo => (receipt.after.clone(), receipt.before.clone()),
                    UndoDirection::Redo => (receipt.before.clone(), receipt.after.clone()),
                };
                FileMutationReceipt::new(
                    receipt.path.clone(),
                    match self.direction {
                        UndoDirection::Undo => "undo",
                        UndoDirection::Redo => "redo",
                    },
                    before,
                    after,
                )
            })
            .collect();
        PreparedFileMutation::from_receipt(FileMutationBatchReceipt::new(receipts)?)
    }
}

impl std::fmt::Debug for UndoOperation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("UndoOperation")
            .field("direction", &self.direction)
            .field("batch", &self.batch)
            .finish()
    }
}

#[derive(Debug, Clone)]
pub struct UndoStack {
    undo: Vec<FileMutationBatchReceipt>,
    redo: Vec<FileMutationBatchReceipt>,
    max_size: usize,
    persistence_revision: Option<u64>,
    recovery_seen: Vec<uuid::Uuid>,
}

impl Default for UndoStack {
    fn default() -> Self {
        Self::new()
    }
}

impl UndoStack {
    pub fn new() -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            max_size: MAX_UNDO_ENTRIES,
            persistence_revision: None,
            recovery_seen: Vec::new(),
        }
    }

    pub fn persisted_batches(
        &self,
    ) -> (Vec<FileMutationBatchReceipt>, Vec<FileMutationBatchReceipt>) {
        (self.undo.clone(), self.redo.clone())
    }

    pub fn from_persisted(
        undo: Vec<FileMutationBatchReceipt>,
        redo: Vec<FileMutationBatchReceipt>,
    ) -> Result<Self, String> {
        if undo.len().saturating_add(redo.len()) > MAX_UNDO_ENTRIES {
            return Err("persisted undo history exceeds its entry bound".into());
        }
        for batch in undo.iter().chain(&redo) {
            PreparedFileMutation::from_receipt(batch.clone())?;
        }
        Ok(Self {
            undo,
            redo,
            max_size: MAX_UNDO_ENTRIES,
            persistence_revision: None,
            recovery_seen: Vec::new(),
        })
    }

    pub(crate) fn persistence_revision(&self) -> Option<u64> {
        self.persistence_revision
    }
    pub(crate) fn set_persistence_revision(&mut self, revision: Option<u64>) {
        self.persistence_revision = revision;
    }
    pub(crate) fn recovery_seen(&self) -> &[uuid::Uuid] {
        &self.recovery_seen
    }
    pub(crate) fn set_recovery_seen(&mut self, seen: Vec<uuid::Uuid>) {
        self.recovery_seen = seen;
    }

    pub fn record(&mut self, receipt: FileMutationReceipt) {
        self.record_batch(receipt.into());
    }

    pub fn record_batch(&mut self, batch: FileMutationBatchReceipt) {
        self.redo.clear();
        self.undo.push(batch);
        trim_oldest(&mut self.undo, self.max_size);
    }

    pub fn clear_redo(&mut self) {
        self.redo.clear();
    }

    pub fn prepare(&self, direction: UndoDirection) -> Option<UndoOperation> {
        let batch = match direction {
            UndoDirection::Undo => self.undo.last(),
            UndoDirection::Redo => self.redo.last(),
        }?;
        Some(UndoOperation {
            direction,
            batch: batch.clone(),
        })
    }

    pub fn complete(&mut self, operation: &UndoOperation) -> bool {
        let (source, destination) = match operation.direction {
            UndoDirection::Undo => (&mut self.undo, &mut self.redo),
            UndoDirection::Redo => (&mut self.redo, &mut self.undo),
        };
        if source.last().map(|entry| entry.id) != Some(operation.batch.id) {
            return false;
        }
        let receipt = source.pop().expect("checked non-empty history");
        destination.push(receipt);
        trim_oldest(destination, self.max_size);
        true
    }

    pub fn is_empty(&self) -> bool {
        self.undo.is_empty()
    }

    pub fn redo_is_empty(&self) -> bool {
        self.redo.is_empty()
    }

    #[cfg(test)]
    fn lengths(&self) -> (usize, usize) {
        (self.undo.len(), self.redo.len())
    }
}

fn trim_oldest(entries: &mut Vec<FileMutationBatchReceipt>, max_size: usize) {
    if entries.len() > max_size {
        entries.drain(..entries.len() - max_size);
    }
}

pub fn read_file_state(path: &Path) -> Result<FileState, String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(FileState::Missing);
        }
        Err(error) => return Err(format!("could not inspect target: {error}")),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("target is not a regular file".into());
    }
    if metadata.len() > MAX_UNDO_FILE_BYTES as u64 {
        return Err(format!(
            "target exceeds the {} byte undo limit",
            MAX_UNDO_FILE_BYTES
        ));
    }
    let file = fs::File::open(path).map_err(|error| format!("could not open target: {error}"))?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take((MAX_UNDO_FILE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("could not read target: {error}"))?;
    if bytes.len() > MAX_UNDO_FILE_BYTES {
        return Err(format!(
            "target exceeds the {} byte undo limit",
            MAX_UNDO_FILE_BYTES
        ));
    }
    let content = String::from_utf8(bytes).map_err(|_| "target is not UTF-8 text".to_string())?;
    Ok(FileState::Utf8(content))
}

pub fn perform(operation: &UndoOperation) -> Result<(), String> {
    let ordered = match operation.direction {
        UndoDirection::Undo => operation.batch.receipts().iter().rev().collect::<Vec<_>>(),
        UndoDirection::Redo => operation.batch.receipts().iter().collect::<Vec<_>>(),
    };
    for receipt in &ordered {
        let expected = match operation.direction {
            UndoDirection::Undo => &receipt.after,
            UndoDirection::Redo => &receipt.before,
        };
        if read_file_state(&receipt.path)? != *expected {
            return Err(
                "a mutation-batch target changed since the recorded edit; history was preserved"
                    .into(),
            );
        }
    }
    let mut applied: Vec<&FileMutationReceipt> = Vec::new();
    for receipt in ordered {
        let desired = match operation.direction {
            UndoDirection::Undo => &receipt.before,
            UndoDirection::Redo => &receipt.after,
        };
        if let Err(error) = restore_file_state(&receipt.path, desired) {
            for completed in applied.into_iter().rev() {
                let original = match operation.direction {
                    UndoDirection::Undo => &completed.after,
                    UndoDirection::Redo => &completed.before,
                };
                let _ = restore_file_state(&completed.path, original);
            }
            return Err(error);
        }
        applied.push(receipt);
    }
    Ok(())
}

fn restore_file_state(path: &Path, desired: &FileState) -> Result<(), String> {
    match desired {
        FileState::Missing => {
            let parent = path
                .parent()
                .ok_or_else(|| "target has no parent directory".to_string())?;
            let parent_metadata = fs::symlink_metadata(parent)
                .map_err(|error| format!("could not inspect target parent: {error}"))?;
            if parent_metadata.file_type().is_symlink() || !parent_metadata.is_dir() {
                return Err("target parent is not a regular directory".into());
            }
            match fs::remove_file(path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(format!("could not remove the exact target: {error}")),
            }
            sync_directory(parent)
        }
        FileState::Utf8(content) => {
            if content.len() > MAX_UNDO_FILE_BYTES {
                return Err("recorded content exceeds the undo limit".into());
            }
            let parent = path
                .parent()
                .ok_or_else(|| "target has no parent directory".to_string())?;
            let parent_metadata = fs::symlink_metadata(parent)
                .map_err(|error| format!("could not inspect target parent: {error}"))?;
            if parent_metadata.file_type().is_symlink() || !parent_metadata.is_dir() {
                return Err("target parent is not a regular directory".into());
            }
            let existing_permissions = match fs::symlink_metadata(path) {
                Ok(metadata) => {
                    if metadata.file_type().is_symlink() || !metadata.is_file() {
                        return Err("target is not a regular file".into());
                    }
                    Some(metadata.permissions())
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => return Err(format!("could not inspect target: {error}")),
            };
            let temporary = parent.join(format!(".ca-undo-{}.tmp", uuid::Uuid::new_v4()));
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
                .map_err(|error| format!("could not create restoration file: {error}"))?;
            let prepared = (|| {
                if let Some(permissions) = existing_permissions {
                    file.set_permissions(permissions)?;
                }
                file.write_all(content.as_bytes())?;
                file.sync_all()
            })();
            if let Err(error) = prepared {
                let _ = fs::remove_file(&temporary);
                return Err(format!("could not prepare restoration: {error}"));
            }
            drop(file);
            let published = if path.exists() {
                fs::rename(&temporary, path)
            } else {
                fs::hard_link(&temporary, path).and_then(|_| fs::remove_file(&temporary))
            };
            if let Err(error) = published {
                let _ = fs::remove_file(&temporary);
                return Err(format!("could not publish restoration: {error}"));
            }
            sync_directory(parent)
        }
    }
}

fn sync_directory(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        fs::File::open(path)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| format!("could not sync target directory: {error}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepared_edit_is_inert_until_exact_before_state_is_revalidated() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("sample.txt");
        fs::write(&path, "before").unwrap();
        let prepared = PreparedFileMutation::single(
            path.clone(),
            "write_file",
            FileState::Utf8("before".into()),
            FileState::Utf8("after".into()),
        )
        .unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "before");
        fs::write(&path, "external").unwrap();
        assert!(prepared.clone().apply().is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "external");
        fs::write(&path, "before").unwrap();
        prepared.apply().unwrap();
        assert_eq!(fs::read_to_string(path).unwrap(), "after");
        assert!(PreparedFileMutation::single(
            root.path().join("large"),
            "write_file",
            FileState::Missing,
            FileState::Utf8("x".repeat(MAX_UNDO_FILE_BYTES + 1))
        )
        .is_err());
    }

    fn receipt(path: &Path, before: FileState, after: FileState) -> FileMutationReceipt {
        FileMutationReceipt::new(path.to_path_buf(), "write_file", before, after)
    }

    #[test]
    fn undo_and_redo_exact_create_modify_delete_transitions() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("sample.txt");
        fs::write(&path, "two").unwrap();
        let mut history = UndoStack::new();
        history.record(receipt(
            &path,
            FileState::Utf8("one".into()),
            FileState::Utf8("two".into()),
        ));

        let undo = history.prepare(UndoDirection::Undo).unwrap();
        perform(&undo).unwrap();
        assert!(history.complete(&undo));
        assert_eq!(fs::read_to_string(&path).unwrap(), "one");

        let redo = history.prepare(UndoDirection::Redo).unwrap();
        perform(&redo).unwrap();
        assert!(history.complete(&redo));
        assert_eq!(fs::read_to_string(&path).unwrap(), "two");

        fs::remove_file(&path).unwrap();
        history.record(receipt(
            &path,
            FileState::Utf8("two".into()),
            FileState::Missing,
        ));
        let undo_delete = history.prepare(UndoDirection::Undo).unwrap();
        perform(&undo_delete).unwrap();
        assert!(history.complete(&undo_delete));
        assert_eq!(fs::read_to_string(&path).unwrap(), "two");

        let created = root.path().join("created.txt");
        fs::write(&created, "new").unwrap();
        history.record(receipt(
            &created,
            FileState::Missing,
            FileState::Utf8("new".into()),
        ));
        let undo_create = history.prepare(UndoDirection::Undo).unwrap();
        perform(&undo_create).unwrap();
        assert!(history.complete(&undo_create));
        assert!(!created.exists());
    }

    #[test]
    fn drift_and_failures_preserve_both_stacks() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("sample.txt");
        fs::write(&path, "external").unwrap();
        let mut history = UndoStack::new();
        history.record(receipt(
            &path,
            FileState::Utf8("one".into()),
            FileState::Utf8("two".into()),
        ));
        let operation = history.prepare(UndoDirection::Undo).unwrap();
        assert!(perform(&operation).unwrap_err().contains("changed"));
        assert_eq!(history.lengths(), (1, 0));
        assert_eq!(fs::read_to_string(&path).unwrap(), "external");
    }

    #[test]
    fn multi_file_batch_is_atomic_and_round_trips_in_order() {
        let root = tempfile::tempdir().unwrap();
        let first = root.path().join("first.txt");
        let second = root.path().join("second.txt");
        fs::write(&first, "first-after").unwrap();
        fs::write(&second, "second-after").unwrap();
        let batch = FileMutationBatchReceipt::new(vec![
            receipt(
                &second,
                FileState::Utf8("second-before".into()),
                FileState::Utf8("second-after".into()),
            ),
            receipt(
                &first,
                FileState::Utf8("first-before".into()),
                FileState::Utf8("first-after".into()),
            ),
        ])
        .unwrap();
        assert_eq!(batch.receipts()[0].path, first);

        let mut history = UndoStack::new();
        history.record_batch(batch);
        fs::write(&second, "external").unwrap();
        let undo = history.prepare(UndoDirection::Undo).unwrap();
        assert!(perform(&undo).unwrap_err().contains("changed"));
        assert_eq!(fs::read_to_string(&first).unwrap(), "first-after");
        assert_eq!(fs::read_to_string(&second).unwrap(), "external");
        assert_eq!(history.lengths(), (1, 0));

        fs::write(&second, "second-after").unwrap();
        perform(&undo).unwrap();
        assert!(history.complete(&undo));
        assert_eq!(fs::read_to_string(&first).unwrap(), "first-before");
        assert_eq!(fs::read_to_string(&second).unwrap(), "second-before");

        let redo = history.prepare(UndoDirection::Redo).unwrap();
        perform(&redo).unwrap();
        assert!(history.complete(&redo));
        assert_eq!(fs::read_to_string(&first).unwrap(), "first-after");
        assert_eq!(fs::read_to_string(&second).unwrap(), "second-after");
    }

    #[test]
    fn recording_after_undo_clears_redo_and_capacity_is_bounded() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("sample.txt");
        let mut history = UndoStack::new();
        for index in 0..=MAX_UNDO_ENTRIES {
            history.record(receipt(
                &path,
                FileState::Utf8(index.to_string()),
                FileState::Utf8((index + 1).to_string()),
            ));
        }
        assert_eq!(history.lengths(), (MAX_UNDO_ENTRIES, 0));
        let operation = history.prepare(UndoDirection::Undo).unwrap();
        assert!(history.complete(&operation));
        assert_eq!(history.lengths(), (MAX_UNDO_ENTRIES - 1, 1));
        history.record(receipt(
            &path,
            FileState::Missing,
            FileState::Utf8("fresh".into()),
        ));
        assert_eq!(history.lengths(), (MAX_UNDO_ENTRIES, 0));
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_rejected_without_reading_the_target() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("target.txt");
        let link = root.path().join("link.txt");
        fs::write(&target, "secret").unwrap();
        symlink(&target, &link).unwrap();
        assert_eq!(
            read_file_state(&link).unwrap_err(),
            "target is not a regular file"
        );
    }

    #[test]
    fn debug_omits_paths_and_contents() {
        let receipt = FileMutationReceipt::new(
            PathBuf::from("/private/secret/path"),
            "write_file",
            FileState::Utf8("old secret".into()),
            FileState::Utf8("new secret".into()),
        );
        let debug = format!("{receipt:?}");
        assert!(!debug.contains("private"));
        assert!(!debug.contains("old secret"));
        assert!(!debug.contains("new secret"));
        assert!(debug.contains("10 bytes"));
    }
}
