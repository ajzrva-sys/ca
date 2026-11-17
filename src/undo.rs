#![allow(dead_code)]
use std::collections::HashMap;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HunkAuthor {
    Agent,
    External,
}

#[derive(Clone, PartialEq, Eq)]
pub struct FileMutationReceipt {
    id: uuid::Uuid,
    pub path: PathBuf,
    pub action: String,
    pub before: FileState,
    pub after: FileState,
    pub author: HunkAuthor,
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
            author: HunkAuthor::Agent,
        }
    }

    pub fn attribute_disk(&mut self, disk_now: &str) {
        let agent_after = match &self.after {
            FileState::Utf8(text) => text.as_str(),
            FileState::Missing => "",
        };
        self.author = if agent_after == disk_now {
            HunkAuthor::Agent
        } else {
            HunkAuthor::External
        };
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

    pub fn receipts_mut(&mut self) -> &mut [FileMutationReceipt] {
        &mut self.receipts
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
    hunks: HunkAttribution,
    seen_mtime: HashMap<PathBuf, std::time::SystemTime>,
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
            hunks: HunkAttribution::new(),
            seen_mtime: HashMap::new(),
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
            hunks: HunkAttribution::new(),
            seen_mtime: HashMap::new(),
        })
    }

    /// Re-read recent agent receipts and mark one external when the bytes differ.
    pub fn refresh_external_hunks(&mut self) {
        let mut paths = Vec::new();
        for batch in self.undo.iter().rev().take(32) {
            for receipt in batch.receipts() {
                if receipt.author == HunkAuthor::Agent {
                    paths.push(receipt.path.clone());
                }
            }
        }
        paths.sort();
        paths.dedup();
        for path in paths {
            let modified = fs::metadata(&path).and_then(|meta| meta.modified()).ok();
            if let Some(modified) = modified {
                if self.seen_mtime.get(&path) == Some(&modified) {
                    continue;
                }
                self.seen_mtime.insert(path.clone(), modified);
            }
            let disk = match fs::metadata(&path) {
                Ok(meta) if meta.len() > 256 * 1024 => continue,
                Ok(_) => fs::read_to_string(&path).unwrap_or_default(),
                Err(_) => String::new(),
            };
            for batch in self.undo.iter_mut().rev() {
                if let Some(receipt) = batch
                    .receipts_mut()
                    .iter_mut()
                    .find(|receipt| receipt.path == path)
                {
                    receipt.attribute_disk(&disk);
                    break;
                }
            }
        }
    }

    pub fn note_agent_edit(&mut self, path: &Path, before: &str, after: &str) {
        if let Some(previous) = self.hunks.get(path).and_then(|file| file.agent.clone()) {
            if previous != before {
                for batch in self.undo.iter_mut().rev() {
                    if let Some(receipt) = batch
                        .receipts_mut()
                        .iter_mut()
                        .find(|receipt| receipt.path == path)
                    {
                        receipt.attribute_disk(before);
                        break;
                    }
                }
            }
        }
        crate::parity_backend::invalidate_code_index(path);
        self.hunks.snapshot(path, before);
        let _ = self.hunks.record_agent(path, after);
    }

    pub fn restore_agent_lines(&mut self, path: &Path, current: &str) -> HunkUndoResult {
        self.hunks.undo(path, current)
    }

    pub fn hunk_attribution(&self) -> &HunkAttribution {
        &self.hunks
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

pub fn perform_with_agent_hunks(
    operation: &UndoOperation,
    hunks: &mut HunkAttribution,
) -> Result<(), String> {
    if operation.direction != UndoDirection::Undo {
        return perform(operation);
    }
    let ordered = operation.batch.receipts().iter().rev().collect::<Vec<_>>();
    for receipt in &ordered {
        let current = read_file_state(&receipt.path)?;
        if current == receipt.after {
            continue;
        }
        let FileState::Utf8(current_text) = &current else {
            return Err(
                "a mutation-batch target changed since the recorded edit; history was preserved"
                    .into(),
            );
        };
        let restored = hunks.undo(&receipt.path, current_text);
        if restored.message.contains("no turn snapshot") {
            return Err(
                "a mutation-batch target changed since the recorded edit; history was preserved"
                    .into(),
            );
        }
        restore_file_state(&receipt.path, &FileState::Utf8(restored.content))?;
    }
    for receipt in ordered {
        if read_file_state(&receipt.path)? == receipt.after {
            restore_file_state(&receipt.path, &receipt.before)?;
        }
    }
    Ok(())
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

/// Restores agent-owned lines from the turn baseline and leaves other lines.
/// A file over the snapshot cap stays on the whole-file rule.
#[allow(dead_code)]
pub fn revert_agent_lines(
    current: &str,
    baseline: &str,
    agent_lines: &[usize],
) -> Result<String, String> {
    if current.len() > MAX_UNDO_FILE_BYTES || baseline.len() > MAX_UNDO_FILE_BYTES {
        return Err(format!(
            "undo keeps the whole file snapshot; file exceeds {MAX_UNDO_FILE_BYTES} bytes"
        ));
    }
    let mut lines: Vec<&str> = current.lines().collect();
    let base: Vec<&str> = baseline.lines().collect();
    for line in agent_lines {
        let Some(index) = line.checked_sub(1) else {
            continue;
        };
        if let (Some(slot), Some(original)) = (lines.get_mut(index), base.get(index)) {
            *slot = original;
        }
    }
    Ok(lines.join("\n"))
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

const MAX_LINE_DIFF_CELLS: usize = 128 * 128;

/// One line replacement. `before` is the previous text; `after_lines` are the new lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineHunk {
    pub before_start: usize,
    pub before_len: usize,
    pub after_start: usize,
    pub after_lines: Vec<String>,
}

/// Agent and user hunks for one file during a turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileTurnHunks {
    pub path: PathBuf,
    pub baseline: String,
    pub agent_hunks: Vec<LineHunk>,
    pub user_hunks: Vec<LineHunk>,
    pub whole_file: bool,
    agent: Option<String>,
}

/// Restored text, or the whole-file snapshot when the byte cap applies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HunkUndoResult {
    pub content: String,
    pub message: String,
    pub whole_file: bool,
    pub agent_hunks: Vec<LineHunk>,
    pub user_hunks: Vec<LineHunk>,
}

/// Side log of agent hunks versus user hunks. The undo stack still stores
/// whole-file snapshots and still keeps at most [`MAX_UNDO_ENTRIES`].
#[derive(Debug, Clone, Default)]
pub struct HunkAttribution {
    files: Vec<FileTurnHunks>,
}

impl HunkAttribution {
    pub fn new() -> Self {
        Self { files: Vec::new() }
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    pub fn get(&self, path: &Path) -> Option<&FileTurnHunks> {
        self.files.iter().find(|file| file.path.as_path() == path)
    }

    /// Snapshot the file at turn start. This is the agent baseline.
    pub fn snapshot(&mut self, path: impl Into<PathBuf>, baseline: &str) {
        let path = path.into();
        let entry = FileTurnHunks::from_snapshot(path, baseline);
        if let Some(existing) = self.files.iter_mut().find(|file| file.path == entry.path) {
            *existing = entry;
            return;
        }
        self.files.push(entry);
        if self.files.len() > MAX_UNDO_ENTRIES {
            self.files.drain(..self.files.len() - MAX_UNDO_ENTRIES);
        }
    }

    /// Record the agent's edit as line replacement ranges against the baseline.
    pub fn record_agent(&mut self, path: &Path, agent: &str) -> Result<(), String> {
        let file = self
            .files
            .iter_mut()
            .find(|file| file.path.as_path() == path)
            .ok_or_else(|| "no turn snapshot for this file".to_string())?;
        if file.whole_file || agent.len() > MAX_UNDO_FILE_BYTES {
            file.whole_file = true;
            file.agent = None;
            file.agent_hunks.clear();
            return Ok(());
        }
        let baseline_lines = split_lines(&file.baseline).lines;
        let agent_lines = split_lines(agent).lines;
        file.agent_hunks = line_hunks(&baseline_lines, &agent_lines);
        file.agent = Some(agent.to_string());
        Ok(())
    }

    /// Undo agent hunks and leave lines the user changed during the turn.
    pub fn undo(&mut self, path: &Path, current: &str) -> HunkUndoResult {
        let Some(index) = self
            .files
            .iter()
            .position(|file| file.path.as_path() == path)
        else {
            return HunkUndoResult {
                content: current.to_string(),
                message: "no turn snapshot for this file".into(),
                whole_file: false,
                agent_hunks: Vec::new(),
                user_hunks: Vec::new(),
            };
        };
        if self.files[index].whole_file || current.len() > MAX_UNDO_FILE_BYTES {
            return whole_file_result(&self.files[index].baseline);
        }
        let baseline = self.files[index].baseline.clone();
        let agent = self.files[index]
            .agent
            .clone()
            .unwrap_or_else(|| baseline.clone());
        let result = undo_agent_hunks(&baseline, &agent, current);
        self.files[index].agent_hunks = result.agent_hunks.clone();
        self.files[index].user_hunks = result.user_hunks.clone();
        self.files[index].whole_file = result.whole_file;
        result
    }
}

impl FileTurnHunks {
    fn from_snapshot(path: PathBuf, baseline: &str) -> Self {
        Self {
            path,
            baseline: baseline.to_string(),
            agent_hunks: Vec::new(),
            user_hunks: Vec::new(),
            whole_file: baseline.len() > MAX_UNDO_FILE_BYTES,
            agent: None,
        }
    }
}

/// Diff the turn baseline, the agent edit, and the user's current text.
///
/// Files over [`MAX_UNDO_FILE_BYTES`] are not split into hunks. The result
/// content is the baseline and the message says the undo restores the whole file.
pub fn undo_agent_hunks(baseline: &str, agent: &str, current: &str) -> HunkUndoResult {
    if baseline.len() > MAX_UNDO_FILE_BYTES
        || agent.len() > MAX_UNDO_FILE_BYTES
        || current.len() > MAX_UNDO_FILE_BYTES
    {
        return whole_file_result(baseline);
    }
    let baseline_lines = split_lines(baseline);
    let agent_lines = split_lines(agent);
    let current_lines = split_lines(current);
    let agent_hunks = line_hunks(&baseline_lines.lines, &agent_lines.lines);
    let user_hunks = line_hunks(&agent_lines.lines, &current_lines.lines);
    let restored = revert_agent_hunks(
        &baseline_lines.lines,
        &current_lines.lines,
        &agent_hunks,
        &user_hunks,
    );
    HunkUndoResult {
        content: join_lines(&restored, current_lines.trailing_newline),
        message: "reverted agent hunks and left user keystrokes in place".into(),
        whole_file: false,
        agent_hunks,
        user_hunks,
    }
}

fn whole_file_result(baseline: &str) -> HunkUndoResult {
    HunkUndoResult {
        content: baseline.to_string(),
        message: format!(
            "file exceeds the {MAX_UNDO_FILE_BYTES} byte undo limit; undo restores the whole file"
        ),
        whole_file: true,
        agent_hunks: Vec::new(),
        user_hunks: Vec::new(),
    }
}

struct SplitText {
    lines: Vec<String>,
    trailing_newline: bool,
}

fn split_lines(text: &str) -> SplitText {
    if text.is_empty() {
        return SplitText {
            lines: Vec::new(),
            trailing_newline: false,
        };
    }
    let trailing_newline = text.ends_with('\n');
    let mut lines: Vec<String> = text.split('\n').map(|line| line.to_owned()).collect();
    if trailing_newline {
        lines.pop();
    }
    SplitText {
        lines,
        trailing_newline,
    }
}

fn join_lines(lines: &[String], trailing_newline: bool) -> String {
    let mut text = lines.join("\n");
    if trailing_newline {
        text.push('\n');
    }
    text
}

#[derive(Clone, Copy)]
struct LineRange {
    before_start: usize,
    before_end: usize,
    after_start: usize,
    after_end: usize,
}

impl LineRange {
    fn span(self) -> usize {
        self.before_end
            .saturating_sub(self.before_start)
            .saturating_add(self.after_end.saturating_sub(self.after_start))
    }
}

fn line_hunks(before: &[String], after: &[String]) -> Vec<LineHunk> {
    let mut hunks = Vec::new();
    let mut stack = vec![LineRange {
        before_start: 0,
        before_end: before.len(),
        after_start: 0,
        after_end: after.len(),
    }];
    while let Some(mut range) = stack.pop() {
        while range.before_start < range.before_end
            && range.after_start < range.after_end
            && before[range.before_start] == after[range.after_start]
        {
            range.before_start += 1;
            range.after_start += 1;
        }
        while range.before_end > range.before_start
            && range.after_end > range.after_start
            && before[range.before_end - 1] == after[range.after_end - 1]
        {
            range.before_end -= 1;
            range.after_end -= 1;
        }
        if range.before_start == range.before_end && range.after_start == range.after_end {
            continue;
        }
        if range.before_start == range.before_end {
            hunks.push(LineHunk {
                before_start: range.before_start,
                before_len: 0,
                after_start: range.after_start,
                after_lines: after[range.after_start..range.after_end].to_vec(),
            });
            continue;
        }
        if range.after_start == range.after_end {
            hunks.push(LineHunk {
                before_start: range.before_start,
                before_len: range.before_end - range.before_start,
                after_start: range.after_start,
                after_lines: Vec::new(),
            });
            continue;
        }
        if push_anchor_splits(before, after, range, &mut stack) {
            continue;
        }
        let width = range.before_end - range.before_start;
        let height = range.after_end - range.after_start;
        if width.saturating_mul(height) <= MAX_LINE_DIFF_CELLS {
            hunks.extend(diff_small(
                &before[range.before_start..range.before_end],
                &after[range.after_start..range.after_end],
                range.before_start,
                range.after_start,
            ));
        } else {
            hunks.push(LineHunk {
                before_start: range.before_start,
                before_len: width,
                after_start: range.after_start,
                after_lines: after[range.after_start..range.after_end].to_vec(),
            });
        }
    }
    hunks.sort_by(|left, right| {
        left.before_start
            .cmp(&right.before_start)
            .then(left.after_start.cmp(&right.after_start))
    });
    coalesce_hunks(&mut hunks);
    hunks
}

fn push_anchor_splits(
    before: &[String],
    after: &[String],
    range: LineRange,
    stack: &mut Vec<LineRange>,
) -> bool {
    let anchors = unique_anchors(before, after, range);
    if anchors.is_empty() {
        return false;
    }
    let parent = range.span();
    let mut next_before = range.before_end;
    let mut next_after = range.after_end;
    let mut children = Vec::with_capacity(anchors.len() + 1);
    for &(anchor_before, anchor_after) in anchors.iter().rev() {
        children.push(LineRange {
            before_start: anchor_before + 1,
            before_end: next_before,
            after_start: anchor_after + 1,
            after_end: next_after,
        });
        next_before = anchor_before;
        next_after = anchor_after;
    }
    children.push(LineRange {
        before_start: range.before_start,
        before_end: next_before,
        after_start: range.after_start,
        after_end: next_after,
    });
    if children.iter().any(|child| child.span() >= parent) {
        return false;
    }
    stack.extend(children);
    true
}

fn unique_anchors(before: &[String], after: &[String], range: LineRange) -> Vec<(usize, usize)> {
    let mut before_count: HashMap<&str, usize> = HashMap::new();
    let mut after_count: HashMap<&str, usize> = HashMap::new();
    for line in &before[range.before_start..range.before_end] {
        *before_count.entry(line.as_str()).or_insert(0) += 1;
    }
    for line in &after[range.after_start..range.after_end] {
        *after_count.entry(line.as_str()).or_insert(0) += 1;
    }
    let mut after_index: HashMap<&str, usize> = HashMap::new();
    for (offset, line) in after[range.after_start..range.after_end].iter().enumerate() {
        if after_count.get(line.as_str()) == Some(&1) && before_count.get(line.as_str()) == Some(&1)
        {
            after_index.insert(line.as_str(), range.after_start + offset);
        }
    }
    let mut pairs = Vec::new();
    for (offset, line) in before[range.before_start..range.before_end]
        .iter()
        .enumerate()
    {
        if let Some(&after_at) = after_index.get(line.as_str()) {
            pairs.push((range.before_start + offset, after_at));
        }
    }
    longest_increasing_pairs(&pairs)
}

fn longest_increasing_pairs(pairs: &[(usize, usize)]) -> Vec<(usize, usize)> {
    let mut tails: Vec<usize> = Vec::new();
    let mut previous: Vec<Option<usize>> = vec![None; pairs.len()];
    for (index, &(_, after_at)) in pairs.iter().enumerate() {
        let pos = tails.partition_point(|&tail| pairs[tail].1 < after_at);
        if pos < tails.len() && pairs[tails[pos]].1 == after_at {
            continue;
        }
        if pos > 0 {
            previous[index] = Some(tails[pos - 1]);
        }
        if pos == tails.len() {
            tails.push(index);
        } else {
            tails[pos] = index;
        }
    }
    let mut chain = Vec::new();
    let mut cursor = tails.last().copied();
    while let Some(index) = cursor {
        chain.push(pairs[index]);
        cursor = previous[index];
    }
    chain.reverse();
    chain
}

fn diff_small(
    before: &[String],
    after: &[String],
    before_offset: usize,
    after_offset: usize,
) -> Vec<LineHunk> {
    let before_len = before.len();
    let after_len = after.len();
    let mut lcs = vec![vec![0usize; after_len + 1]; before_len + 1];
    for i in (0..before_len).rev() {
        for j in (0..after_len).rev() {
            lcs[i][j] = if before[i] == after[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }

    let mut hunks = Vec::new();
    let mut open: Option<LineHunk> = None;
    let mut i = 0;
    let mut j = 0;
    while i < before_len || j < after_len {
        if i < before_len && j < after_len && before[i] == after[j] {
            if let Some(hunk) = open.take() {
                hunks.push(hunk);
            }
            i += 1;
            j += 1;
            continue;
        }
        let delete = i < before_len && (j == after_len || lcs[i + 1][j] >= lcs[i][j + 1]);
        let hunk = open.get_or_insert_with(|| LineHunk {
            before_start: before_offset + i,
            before_len: 0,
            after_start: after_offset + j,
            after_lines: Vec::new(),
        });
        if delete {
            hunk.before_len += 1;
            i += 1;
        } else {
            hunk.after_lines.push(after[j].clone());
            j += 1;
        }
    }
    if let Some(hunk) = open {
        hunks.push(hunk);
    }
    hunks
}

fn coalesce_hunks(hunks: &mut Vec<LineHunk>) {
    if hunks.len() < 2 {
        return;
    }
    let mut merged: Vec<LineHunk> = Vec::with_capacity(hunks.len());
    for hunk in hunks.drain(..) {
        if let Some(previous) = merged.last_mut() {
            let previous_before_end = previous.before_start + previous.before_len;
            let previous_after_end = previous.after_start + previous.after_lines.len();
            if previous_before_end == hunk.before_start && previous_after_end == hunk.after_start {
                previous.before_len += hunk.before_len;
                previous.after_lines.extend(hunk.after_lines);
                continue;
            }
        }
        merged.push(hunk);
    }
    *hunks = merged;
}

fn user_overlaps(user_hunks: &[LineHunk], start: usize, len: usize) -> bool {
    if len == 0 {
        return false;
    }
    let end = start + len;
    user_hunks.iter().any(|hunk| {
        if hunk.before_len == 0 {
            return false;
        }
        let hunk_end = hunk.before_start + hunk.before_len;
        hunk.before_start < end && start < hunk_end
    })
}

fn shift_before(user_hunks: &[LineHunk], agent_line: usize) -> isize {
    let mut shift = 0isize;
    for hunk in user_hunks {
        if hunk.before_start >= agent_line {
            break;
        }
        let end = hunk.before_start + hunk.before_len;
        if end <= agent_line {
            shift += hunk.after_lines.len() as isize - hunk.before_len as isize;
        }
    }
    shift
}

fn revert_agent_hunks(
    baseline: &[String],
    current: &[String],
    agent_hunks: &[LineHunk],
    user_hunks: &[LineHunk],
) -> Vec<String> {
    let mut edits = Vec::new();
    for hunk in agent_hunks {
        let span = hunk.after_lines.len();
        if user_overlaps(user_hunks, hunk.after_start, span) {
            continue;
        }
        let start = hunk.after_start as isize + shift_before(user_hunks, hunk.after_start);
        if start < 0 {
            continue;
        }
        let replacement = baseline
            .iter()
            .skip(hunk.before_start)
            .take(hunk.before_len)
            .cloned()
            .collect::<Vec<_>>();
        edits.push((start as usize, span, replacement));
    }
    edits.sort_by_key(|edit| std::cmp::Reverse(edit.0));
    let mut lines = current.to_vec();
    for (start, len, replacement) in edits {
        if start > lines.len() {
            continue;
        }
        let end = start.saturating_add(len).min(lines.len());
        lines.splice(start..end, replacement);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undo_restores_agent_lines_and_keeps_user_keystrokes() {
        let baseline = "a\nb\nc\nd\ne";
        let current = "a\nB\nc\nd\nE";
        let restored = revert_agent_lines(current, baseline, &[2]).unwrap();
        assert_eq!(restored, "a\nb\nc\nd\nE");
        let huge = "x".repeat(MAX_UNDO_FILE_BYTES + 1);
        let error = revert_agent_lines(&huge, "a", &[1]).unwrap_err();
        assert!(error.contains("whole file"));
    }

    #[test]
    fn an_external_edit_is_marked_when_the_turn_rereads_the_receipt() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("file.txt");
        fs::write(&path, "agent\n").unwrap();
        let mut stack = UndoStack::new();
        stack.record(FileMutationReceipt::new(
            path.clone(),
            "write",
            FileState::Missing,
            FileState::Utf8("agent\n".into()),
        ));
        fs::write(&path, "external\n").unwrap();
        stack.refresh_external_hunks();
        assert_eq!(
            stack.persisted_batches().0[0].receipts()[0].author,
            HunkAuthor::External
        );
    }

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

    #[test]
    fn undo_perform_restores_agent_lines_and_keeps_the_user_line() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("file.txt");
        std::fs::write(&path, "a\nB\nc\nd\nE\n").unwrap();
        let mut stack = UndoStack::new();
        stack.note_agent_edit(&path, "a\nb\nc\nd\ne\n", "a\nB\nc\nd\ne\n");
        let receipt = FileMutationReceipt::new(
            path.clone(),
            "edit",
            FileState::Utf8("a\nb\nc\nd\ne\n".into()),
            FileState::Utf8("a\nB\nc\nd\ne\n".into()),
        );
        let batch = FileMutationBatchReceipt::new(vec![receipt]).unwrap();
        let operation = UndoOperation {
            direction: UndoDirection::Undo,
            batch,
        };
        let mut hunks = stack.hunk_attribution().clone();
        perform_with_agent_hunks(&operation, &mut hunks).unwrap();
        let restored = std::fs::read_to_string(&path).unwrap();
        assert_eq!(restored, "a\nb\nc\nd\nE\n");
        assert!(restored.contains('E'));
        assert!(!restored.lines().nth(1).unwrap().contains('B'));
    }

    #[test]
    fn undo_restores_agent_line_two_and_keeps_user_line_five() {
        let baseline = "line1\nline2\nline3\nline4\nline5\nline6\n";
        let agent = "line1\nAGENT\nline3\nline4\nline5\nline6\n";
        let current = "line1\nAGENT\nline3\nline4\nUSER\nline6\n";
        let direct = undo_agent_hunks(baseline, agent, current);
        assert!(!direct.whole_file);
        assert_eq!(direct.content, "line1\nline2\nline3\nline4\nUSER\nline6\n");
        assert_eq!(direct.agent_hunks.len(), 1);
        assert_eq!(direct.agent_hunks[0].before_start, 1);
        assert_eq!(direct.agent_hunks[0].before_len, 1);
        assert_eq!(direct.user_hunks.len(), 1);
        assert_eq!(direct.user_hunks[0].before_start, 4);

        let mut attribution = HunkAttribution::new();
        let path = PathBuf::from("sample.txt");
        attribution.snapshot(path.clone(), baseline);
        attribution.record_agent(&path, agent).unwrap();
        let restored = attribution.undo(&path, current);
        assert_eq!(restored.content, direct.content);
        let recorded = attribution.get(&path).unwrap();
        assert_eq!(recorded.agent_hunks.len(), 1);
        assert_eq!(recorded.user_hunks.len(), 1);
        assert!(!recorded.whole_file);
    }

    #[test]
    fn undo_keeps_a_user_line_between_agent_hunks() {
        let baseline = "one\ntwo\nthree\nfour\nfive\n";
        let agent = "one\nAGENT\nthree\nAGENT4\nfive\n";
        let current = "one\nAGENT\nUSER\nAGENT4\nfive\n";
        let restored = undo_agent_hunks(baseline, agent, current);
        assert_eq!(restored.content, "one\ntwo\nUSER\nfour\nfive\n");
        assert_eq!(restored.agent_hunks.len(), 2);
        assert_eq!(restored.user_hunks.len(), 1);
    }

    #[test]
    fn oversized_file_uses_the_whole_file_rule() {
        let baseline = "b".repeat(MAX_UNDO_FILE_BYTES + 1);
        let agent = "a".repeat(MAX_UNDO_FILE_BYTES + 1);
        let current = "u".repeat(MAX_UNDO_FILE_BYTES + 1);
        let result = undo_agent_hunks(&baseline, &agent, &current);
        assert!(result.whole_file);
        assert!(result.message.contains("whole file"));
        assert_eq!(result.content, baseline);
        assert_ne!(result.content, current);
        assert!(result.agent_hunks.is_empty());

        let mut attribution = HunkAttribution::new();
        attribution.snapshot("big.txt", &baseline);
        let through_log = attribution.undo(Path::new("big.txt"), &current);
        assert!(through_log.whole_file);
        assert!(through_log.message.contains("whole file"));
        assert_eq!(through_log.content, baseline);
    }

    #[test]
    fn hunk_log_and_undo_stack_keep_the_twenty_entry_cap() {
        let mut attribution = HunkAttribution::new();
        for index in 0..=MAX_UNDO_ENTRIES {
            attribution.snapshot(format!("f{index}.txt"), "line1\nline2\n");
        }
        assert_eq!(attribution.len(), MAX_UNDO_ENTRIES);
        assert!(attribution.get(Path::new("f0.txt")).is_none());
        let newest = format!("f{MAX_UNDO_ENTRIES}.txt");
        assert!(attribution.get(Path::new(&newest)).is_some());

        let mut history = UndoStack::new();
        let path = PathBuf::from("sample.txt");
        for index in 0..=MAX_UNDO_ENTRIES {
            history.record(receipt(
                &path,
                FileState::Utf8(index.to_string()),
                FileState::Utf8((index + 1).to_string()),
            ));
        }
        assert_eq!(history.lengths(), (MAX_UNDO_ENTRIES, 0));
    }
}
