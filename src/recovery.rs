//! Private, best-effort recovery for typed CA edits. Blocking filesystem work
//! belongs on a worker; callers retain their session/workspace writer authority.
use crate::session_access::{SessionAccessCoordinator, SessionWriterLease};
use crate::undo::{FileMutationBatchReceipt, FileMutationReceipt, FileState, PreparedFileMutation};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use uuid::Uuid;

const JOURNAL_VERSION: u32 = 1;
const MAX_JOURNAL_BYTES: u64 = 2 * 1024 * 1024;
pub const DEFAULT_RECOVERY_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct RecoveryStore {
    scope: PathBuf,
    max_bytes: u64,
}

#[derive(Debug)]
struct WorkspaceLeaseInner {
    _files: Vec<File>,
    scope: PathBuf,
    roots: Vec<PathBuf>,
    exclusive: bool,
}

#[derive(Clone)]
pub struct WorkspaceActivityLease {
    inner: Arc<WorkspaceLeaseInner>,
}
pub struct WorkspaceRestoreLease {
    _inner: Arc<WorkspaceLeaseInner>,
}

impl std::fmt::Debug for WorkspaceActivityLease {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkspaceActivityLease")
            .field("root_count", &self.inner.roots.len())
            .finish()
    }
}

fn workspace_owners() -> &'static Mutex<Vec<Weak<WorkspaceLeaseInner>>> {
    static OWNERS: OnceLock<Mutex<Vec<Weak<WorkspaceLeaseInner>>>> = OnceLock::new();
    OWNERS.get_or_init(|| Mutex::new(Vec::new()))
}

fn roots_overlap(left: &[PathBuf], right: &[PathBuf]) -> bool {
    left.iter()
        .any(|a| right.iter().any(|b| a.starts_with(b) || b.starts_with(a)))
}

fn workspace_lease(
    _profile: &Path,
    requested: &[PathBuf],
    exclusive: bool,
) -> io::Result<Arc<WorkspaceLeaseInner>> {
    let scope = workspace_access_directory()?;
    workspace_lease_in_scope(scope, requested, exclusive)
}

fn workspace_lease_in_scope(
    scope: PathBuf,
    requested: &[PathBuf],
    exclusive: bool,
) -> io::Result<Arc<WorkspaceLeaseInner>> {
    let mut roots = requested
        .iter()
        .map(std::fs::canonicalize)
        .collect::<io::Result<Vec<_>>>()?;
    roots.sort();
    roots.dedup();
    if roots.is_empty() || roots.len() > 64 || roots.iter().any(|root| !root.is_dir()) {
        return Err(invalid("workspace activity roots exceed their bounds"));
    }
    let mut owners = workspace_owners()
        .lock()
        .map_err(|_| io::Error::other("workspace ownership coordinator is unavailable"))?;
    let registry = SessionAccessCoordinator::new(scope.join(".registry"));
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);
    let _registry = loop {
        match registry.try_writer(Uuid::nil()) {
            Ok(lease) => break lease,
            Err(error)
                if error.kind() == io::ErrorKind::WouldBlock
                    && std::time::Instant::now() < deadline =>
            {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            Err(error) => return Err(error),
        }
    };
    owners.retain(|owner| owner.strong_count() > 0);
    if owners.iter().filter_map(Weak::upgrade).any(|owner| {
        owner.scope == scope
            && (exclusive || owner.exclusive)
            && roots_overlap(&roots, &owner.roots)
    }) {
        return Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            "workspace has active local CA work",
        ));
    }
    for root in &roots {
        let digest = format!("{:x}", Sha256::digest(root.as_os_str().as_encoded_bytes()));
        let descriptor = scope.join(format!("{digest}.json"));
        if !descriptor.exists() {
            atomic_write(
                &descriptor,
                &serde_json::to_vec(root).map_err(io::Error::other)?,
            )?;
        }
    }
    let mut registered = Vec::new();
    let mut descriptors_seen = 0_usize;
    for entry in std::fs::read_dir(&scope)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
            continue;
        }
        descriptors_seen += 1;
        let metadata = std::fs::symlink_metadata(&path)?;
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || metadata.len() > 8192
            || descriptors_seen > 16384
        {
            return Err(invalid("workspace registry exceeds its private bounds"));
        }
        let root: PathBuf =
            serde_json::from_reader(File::open(&path)?).map_err(io::Error::other)?;
        let digest = format!("{:x}", Sha256::digest(root.as_os_str().as_encoded_bytes()));
        if !root.is_absolute() || path.file_stem().and_then(|stem| stem.to_str()) != Some(&digest) {
            return Err(invalid("workspace registry identity changed"));
        }
        if !roots.contains(&root)
            && prune_idle_workspace_registration(&scope, &path, &metadata, &digest)?
        {
            continue;
        }
        registered.push((root, digest));
    }
    registered.sort();
    let mut files = Vec::new();
    for (root, digest) in &registered {
        if exclusive && roots_overlap(&roots, std::slice::from_ref(root))
            || !exclusive && roots.contains(root)
        {
            files.push(workspace_lock(
                &scope.join(format!("{digest}.active.lock")),
                exclusive,
            )?);
        }
    }
    // Guards prevent a newly registered overlapping root from entering while
    // a restoration owns its exact scope. Activity locks remain exact, so an
    // unrelated sibling can continue working.
    for (root, digest) in &registered {
        if exclusive && roots.contains(root)
            || !exclusive && roots_overlap(&roots, std::slice::from_ref(root))
        {
            files.push(workspace_lock(
                &scope.join(format!("{digest}.restore.lock")),
                exclusive,
            )?);
        }
    }
    let inner = Arc::new(WorkspaceLeaseInner {
        _files: files,
        scope,
        roots,
        exclusive,
    });
    owners.push(Arc::downgrade(&inner));
    Ok(inner)
}

/// The registry claim serializes removal with every admission. Stable lock
/// files are never unlinked: an inherited live claim must keep its binding,
/// and a future admission must reopen the same inode.
fn prune_idle_workspace_registration(
    scope: &Path,
    descriptor: &Path,
    descriptor_metadata: &std::fs::Metadata,
    digest: &str,
) -> io::Result<bool> {
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
        let uid = unsafe { libc::geteuid() };
        let private_regular = |metadata: &std::fs::Metadata| {
            metadata.is_file()
                && !metadata.file_type().is_symlink()
                && metadata.uid() == uid
                && metadata.nlink() == 1
                && metadata.mode() & 0o077 == 0
        };
        if !private_regular(descriptor_metadata) {
            return Ok(false);
        }
        let mut claims = Vec::with_capacity(2);
        for suffix in ["active", "restore"] {
            let path = scope.join(format!("{digest}.{suffix}.lock"));
            let metadata = match std::fs::symlink_metadata(&path) {
                Ok(metadata) if private_regular(&metadata) => metadata,
                Ok(_) => return Ok(false),
                Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
                Err(error) => return Err(error),
            };
            let file = match OpenOptions::new()
                .read(true)
                .write(true)
                .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
                .open(&path)
            {
                Ok(file) => file,
                Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
                Err(error) => return Err(error),
            };
            let opened = file.metadata()?;
            if !private_regular(&opened)
                || opened.dev() != metadata.dev()
                || opened.ino() != metadata.ino()
            {
                return Ok(false);
            }
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::WouldBlock {
                    return Ok(false);
                }
                return Err(error);
            }
            claims.push((path, file));
        }
        // Recheck every name after taking both claims. Unknown/replaced names
        // remain registered rather than allowing removal on a stale proof.
        for (path, file) in &claims {
            let current = match std::fs::symlink_metadata(path) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
                Err(error) => return Err(error),
            };
            let opened = file.metadata()?;
            if !private_regular(&current)
                || current.dev() != opened.dev()
                || current.ino() != opened.ino()
            {
                return Ok(false);
            }
        }
        let current = match std::fs::symlink_metadata(descriptor) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error),
        };
        if !private_regular(&current)
            || current.dev() != descriptor_metadata.dev()
            || current.ino() != descriptor_metadata.ino()
            || current.len() != descriptor_metadata.len()
            || current.mtime() != descriptor_metadata.mtime()
            || current.mtime_nsec() != descriptor_metadata.mtime_nsec()
        {
            return Ok(false);
        }
        std::fs::remove_file(descriptor)?;
        Ok(true)
    }
    #[cfg(not(unix))]
    {
        let _ = (scope, descriptor, descriptor_metadata, digest);
        Ok(false)
    }
}

/// Workspace activity is shared across every profile, HOME override and Bare
/// runner. Only lock metadata lives in this owner-bound transient namespace.
fn workspace_access_directory() -> io::Result<PathBuf> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
        let uid = unsafe { libc::geteuid() };
        let scope = Path::new("/tmp")
            .canonicalize()?
            .join(format!("ca-workspace-access-v1-{uid}"));
        let mut builder = std::fs::DirBuilder::new();
        builder.mode(0o700);
        match builder.create(&scope) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
        let metadata = std::fs::symlink_metadata(&scope)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() || metadata.uid() != uid {
            return Err(invalid(
                "workspace access directory is not owned by this user",
            ));
        }
        std::fs::set_permissions(&scope, std::fs::Permissions::from_mode(0o700))?;
        Ok(scope)
    }
    #[cfg(not(unix))]
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "workspace advisory access requires Unix",
    ))
}

fn workspace_lock(path: &Path, exclusive: bool) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    let file = options.open(path)?;
    if !file.metadata()?.is_file() {
        return Err(invalid("workspace lock is not a regular file"));
    }
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        let kind = if exclusive {
            libc::LOCK_EX
        } else {
            libc::LOCK_SH
        };
        if unsafe { libc::flock(file.as_raw_fd(), kind | libc::LOCK_NB) } != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    #[cfg(not(unix))]
    {
        let _ = exclusive;
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "workspace advisory ownership is unavailable",
        ));
    }
    Ok(file)
}

#[derive(Clone)]
pub struct RecoveryContext {
    pub store: Arc<RecoveryStore>,
    pub session_id: Uuid,
    pub turn_id: Uuid,
    pub workspace_roots: Vec<PathBuf>,
    pub origin: String,
    pub writer: SessionWriterLease,
    pub activity: Option<WorkspaceActivityLease>,
    mutations: Arc<MutationTracker>,
}

#[derive(Default)]
struct MutationState {
    closed: bool,
    active: usize,
}

#[derive(Default)]
struct MutationTracker {
    state: Mutex<MutationState>,
    drained: tokio::sync::Notify,
}

/// Acquire before scheduling blocking mutation work. Moving this non-cloneable
/// guard into the worker keeps parent-turn retirement waiting through capture,
/// edit, completion and durable native-history retention.
pub struct RecoveryMutationAdmission {
    tracker: Arc<MutationTracker>,
    session_id: Uuid,
    turn_id: Uuid,
}

impl Drop for RecoveryMutationAdmission {
    fn drop(&mut self) {
        let mut state = self
            .tracker
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        state.active -= 1;
        if state.active == 0 {
            self.tracker.drained.notify_waiters();
        }
    }
}

impl std::fmt::Debug for RecoveryContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RecoveryContext")
            .field("session_id", &self.session_id)
            .field("turn_id", &self.turn_id)
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
pub struct RecordedMutationOutcome {
    pub result: Result<FileMutationBatchReceipt, String>,
    pub warning: Option<String>,
}

#[derive(Debug)]
pub struct HistoryMutationOutcome {
    pub history: crate::undo::UndoStack,
    pub rescue_turn: Uuid,
    pub warning: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RecoveryRestoreOutcome {
    pub rescue_turn: Uuid,
    pub warning: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CaptureTicket {
    session_id: Uuid,
    turn_id: Uuid,
    record_id: Uuid,
}

#[derive(Debug, Clone)]
pub struct RecoveryTurnSummary {
    pub turn_id: Uuid,
    pub file_count: usize,
    pub completed: bool,
    pub incomplete: bool,
    pub conflicts: bool,
}

#[derive(Debug, Clone)]
pub struct RecoveryPreview {
    pub session_id: Uuid,
    pub turn_id: Uuid,
    pub files: Vec<String>,
    pub incomplete: bool,
    /// Current and proposed restored text, bounded independently of the saved
    /// blobs. The confirmation token still binds every byte when truncated.
    pub changes_text: String,
    pub changes_truncated: bool,
    pub token: String,
    journal_revision: u64,
    mutation: FileMutationBatchReceipt,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    version: u32,
    session_id: Uuid,
    revision: u64,
    groups: Vec<TurnGroup>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    undo: Option<StoredUndoStack>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredUndoStack {
    undo: Vec<StoredUndoBatch>,
    redo: Vec<StoredUndoBatch>,
    #[serde(default)]
    revision: u64,
    #[serde(default)]
    seen_records: Vec<Uuid>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredUndoBatch {
    files: Vec<RecordedFile>,
    #[serde(default)]
    id: Option<Uuid>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TurnGroup {
    turn_id: Uuid,
    created_at: i64,
    completed: bool,
    incomplete: bool,
    records: Vec<Intent>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Intent {
    id: Uuid,
    origin: String,
    status: IntentStatus,
    files: Vec<RecordedFile>,
    #[serde(default)]
    batch_id: Option<Uuid>,
    #[serde(default)]
    history_operation: Option<HistoryOperationPointer>,
    #[serde(default)]
    history_reset: bool,
    #[serde(default)]
    capture_revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    applied_revision: Option<u64>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoryOperationPointer {
    direction: crate::undo::UndoDirection,
    batch_id: Uuid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum IntentStatus {
    Pending,
    Applied,
    Aborted,
    Conflict,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RecordedFile {
    root: RootIdentity,
    relative_path: PathBuf,
    action: String,
    before: StoredState,
    after: StoredState,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RootIdentity {
    path: PathBuf,
    device: u64,
    inode: u64,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum StoredState {
    Missing,
    Utf8 { sha256: String, bytes: usize },
}

impl RecoveryStore {
    pub fn new(scope: impl Into<PathBuf>) -> Self {
        Self::with_quota(scope, DEFAULT_RECOVERY_BYTES)
    }
    pub fn with_quota(scope: impl Into<PathBuf>, max_bytes: u64) -> Self {
        Self {
            scope: scope.into(),
            max_bytes,
        }
    }

    pub fn try_workspace_activity(&self, roots: &[PathBuf]) -> io::Result<WorkspaceActivityLease> {
        Ok(WorkspaceActivityLease {
            inner: workspace_lease(&self.scope, roots, false)?,
        })
    }

    pub fn try_workspace_restore(&self, roots: &[PathBuf]) -> io::Result<WorkspaceRestoreLease> {
        Ok(WorkspaceRestoreLease {
            _inner: workspace_lease(&self.scope, roots, true)?,
        })
    }

    /// Persists bounded native undo/redo receipts using the same private blobs
    /// and journal commit. Failure leaves the prior durable stack untouched.
    pub fn save_undo_stack(
        &self,
        context: &RecoveryContext,
        history: &crate::undo::UndoStack,
    ) -> io::Result<()> {
        self.validate_context(context)?;
        let _maintenance = self.maintenance()?;
        self.save_undo_stack_locked(context, history, None)
    }

    fn save_undo_stack_locked(
        &self,
        context: &RecoveryContext,
        history: &crate::undo::UndoStack,
        processed: Option<Uuid>,
    ) -> io::Result<()> {
        let mut journal = self.read_journal(context.session_id)?;
        if history.persistence_revision() != journal.undo.as_ref().map(|stack| stack.revision) {
            return Err(invalid(
                "durable undo history changed; reload before saving",
            ));
        }
        self.prune_blobs(&journal)?;
        let (undo, redo) = history.persisted_batches();
        if undo.len().saturating_add(redo.len()) > crate::undo::MAX_UNDO_ENTRIES {
            return Err(invalid("native undo history exceeds its entry bound"));
        }
        let mut blobs = BTreeMap::new();
        let mut store_batch = |batch: &FileMutationBatchReceipt| -> io::Result<StoredUndoBatch> {
            let mut files = Vec::new();
            for receipt in batch.receipts() {
                let (root, relative_path) = record_path(&context.workspace_roots, &receipt.path)?;
                files.push(RecordedFile {
                    root,
                    relative_path,
                    action: receipt.action.clone(),
                    before: store_state(&receipt.before, &mut blobs)?,
                    after: store_state(&receipt.after, &mut blobs)?,
                });
            }
            Ok(StoredUndoBatch {
                files,
                id: Some(batch.persisted_id()),
            })
        };
        let batch_ids: HashSet<_> = undo
            .iter()
            .chain(&redo)
            .map(FileMutationBatchReceipt::persisted_id)
            .collect();
        let all_records: HashSet<_> = journal
            .groups
            .iter()
            .flat_map(|group| &group.records)
            .map(|record| record.id)
            .collect();
        let mut seen: HashSet<_> = history
            .recovery_seen()
            .iter()
            .filter(|id| all_records.contains(id))
            .copied()
            .collect();
        seen.extend(
            journal
                .groups
                .iter()
                .flat_map(|group| &group.records)
                .filter(|record| {
                    record.status == IntentStatus::Applied
                        && (record.batch_id.is_some_and(|id| batch_ids.contains(&id))
                            || history_origin(&record.origin))
                })
                .map(|record| record.id),
        );
        seen.extend(processed);
        journal.undo = Some(StoredUndoStack {
            undo: undo
                .iter()
                .map(&mut store_batch)
                .collect::<io::Result<_>>()?,
            redo: redo
                .iter()
                .map(&mut store_batch)
                .collect::<io::Result<_>>()?,
            revision: next_revision(history.persistence_revision().unwrap_or(0))?,
            seen_records: seen.into_iter().collect(),
        });
        let directory = self.directory(context.session_id);
        let old_bytes = std::fs::metadata(directory.join("journal.json"))
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        let blob_bytes = blobs
            .iter()
            .filter(|(digest, _)| !directory.join("blobs").join(digest).exists())
            .map(|(_, bytes)| bytes.len() as u64)
            .sum::<u64>();
        self.ensure_capacity(
            context,
            blob_bytes + (encode_journal(&journal)?.len() as u64).saturating_sub(old_bytes),
        )?;
        let latest = self.read_journal(context.session_id)?;
        let retained: HashSet<_> = latest.groups.iter().map(|group| group.turn_id).collect();
        journal
            .groups
            .retain(|group| retained.contains(&group.turn_id));
        let retained_records: HashSet<_> = journal
            .groups
            .iter()
            .flat_map(|group| &group.records)
            .map(|record| record.id)
            .collect();
        if let Some(stack) = &mut journal.undo {
            stack
                .seen_records
                .retain(|id| retained_records.contains(id));
            stack.seen_records.sort();
        }
        journal.revision = latest.revision;
        private_dir(&self.root())?;
        private_dir(&directory)?;
        private_dir(&directory.join("blobs"))?;
        for (digest, bytes) in blobs {
            let path = directory.join("blobs").join(&digest);
            if path.exists() {
                read_blob(&path, &digest, bytes.len())?;
            } else {
                atomic_write(&path, &bytes)?;
            }
        }
        self.write_journal(&mut journal)?;
        self.prune_blobs(&journal)
    }

    pub fn load_undo_stack(&self, context: &RecoveryContext) -> io::Result<crate::undo::UndoStack> {
        self.validate_context(context)?;
        let _maintenance = self.maintenance()?;
        self.load_undo_stack_locked(context, None)
    }

    /// Human undo/redo refuses to write without a durable rescue intent. It
    /// owns the workspace exclusively and commits the refreshed stack itself.
    pub fn apply_history(
        &self,
        context: &RecoveryContext,
        operation: &crate::undo::UndoOperation,
    ) -> io::Result<HistoryMutationOutcome> {
        self.validate_context(context)?;
        let _workspace = self.try_workspace_restore(&context.workspace_roots)?;
        let _maintenance = self.maintenance()?;
        let mut history = self.load_undo_stack_locked(context, None)?;
        let expected = history
            .prepare(operation.direction)
            .ok_or_else(|| invalid("durable undo history is empty"))?;
        if !expected.matches(operation) {
            return Err(invalid(
                "durable undo selection changed; reload before restoring",
            ));
        }
        let mut rescue = context.clone();
        rescue.turn_id = Uuid::new_v4();
        rescue.origin = "human_history".into();
        let prepared = operation.prepared_mutation().map_err(invalid)?;
        let ticket = self.prepare_locked_with_history(
            &rescue,
            prepared.receipt(),
            Some(HistoryOperationPointer {
                direction: operation.direction,
                batch_id: operation.batch.persisted_id(),
            }),
        )?;
        let result = prepared.apply();
        let mut warning = self.complete_locked(&rescue, &ticket).err().map(|_| {
            "Restoration completed; its recovery completion needs reconciliation.".to_owned()
        });
        match result {
            Ok(_) => {}
            Err(error) => {
                let _ = self.finish_turn_locked(&rescue);
                return Err(invalid(error));
            }
        }
        if !history.complete(operation) {
            return Err(invalid(
                "durable undo transaction lost its validated selection",
            ));
        }
        let mut seen = history.recovery_seen().to_vec();
        seen.push(ticket.record_id);
        history.set_recovery_seen(seen);
        if self
            .save_undo_stack_locked(&rescue, &history, Some(ticket.record_id))
            .is_err()
        {
            warning = Some("Restoration completed; durable undo state needs reconciliation before another restoration.".into());
        } else {
            match self.load_undo_stack_locked(&rescue, None) {
                Ok(refreshed) => history = refreshed,
                Err(_) => warning = Some("Restoration completed; its durable history needs reconciliation before another restoration.".into()),
            }
        }
        if self.finish_turn_locked(&rescue).is_err() {
            warning = Some("Restoration completed; its recovery turn needs reconciliation.".into());
        }
        Ok(HistoryMutationOutcome {
            history,
            rescue_turn: rescue.turn_id,
            warning,
        })
    }

    fn retain_receipt(
        &self,
        context: &RecoveryContext,
        receipt: &FileMutationBatchReceipt,
        ticket: Option<&CaptureTicket>,
    ) -> io::Result<()> {
        self.validate_context(context)?;
        let _maintenance = self.maintenance()?;
        let mut history =
            self.load_undo_stack_locked(context, ticket.map(|ticket| ticket.record_id))?;
        history.record_batch(receipt.clone());
        self.save_undo_stack_locked(context, &history, ticket.map(|ticket| ticket.record_id))
    }

    fn load_undo_stack_locked(
        &self,
        context: &RecoveryContext,
        excluded: Option<Uuid>,
    ) -> io::Result<crate::undo::UndoStack> {
        let journal = self.read_journal(context.session_id)?;
        let revision = journal.undo.as_ref().map(|stack| stack.revision);
        let stack = journal.undo.clone().unwrap_or(StoredUndoStack {
            undo: Vec::new(),
            redo: Vec::new(),
            revision: 0,
            seen_records: Vec::new(),
        });
        let mut seen: HashSet<_> = stack.seen_records.iter().copied().collect();
        let load_batch =
            |batch: StoredUndoBatch, fallback: Uuid| -> io::Result<FileMutationBatchReceipt> {
                let mut receipts = Vec::new();
                for file in batch.files {
                    let path = resolve_recorded_path(&file)?;
                    record_path(&context.workspace_roots, &path)?;
                    receipts.push(FileMutationReceipt::new(
                        path,
                        file.action,
                        self.load_state(context.session_id, &file.before)?,
                        self.load_state(context.session_id, &file.after)?,
                    ));
                }
                FileMutationBatchReceipt::from_persisted(receipts, batch.id.or(Some(fallback)))
                    .map_err(invalid)
            };
        let mut history = crate::undo::UndoStack::from_persisted(
            stack
                .undo
                .into_iter()
                .enumerate()
                .map(|(index, batch)| {
                    load_batch(
                        batch,
                        Uuid::new_v5(
                            &context.session_id,
                            format!("ca-legacy-undo-v1-{index}").as_bytes(),
                        ),
                    )
                })
                .collect::<io::Result<_>>()?,
            stack
                .redo
                .into_iter()
                .enumerate()
                .map(|(index, batch)| {
                    load_batch(
                        batch,
                        Uuid::new_v5(
                            &context.session_id,
                            format!("ca-legacy-redo-v1-{index}").as_bytes(),
                        ),
                    )
                })
                .collect::<io::Result<_>>()?,
        )
        .map_err(invalid)?;
        history.set_persistence_revision(revision);
        let mut applied: Vec<_> = journal
            .groups
            .iter()
            .flat_map(|group| &group.records)
            .filter(|record| record.status == IntentStatus::Applied && Some(record.id) != excluded)
            .collect();
        // A background child's older group can receive records after a newer
        // parent group. Preserve durable completion order across those groups;
        // legacy and uncompleted captures retain deterministic capture order.
        applied.sort_by_key(|record| record.applied_revision.unwrap_or(record.capture_revision));
        for record in applied {
            if seen.contains(&record.id) {
                continue;
            }
            if record.history_reset || record.origin == "human_recovery" {
                // A restored turn resets native undo. This transition is
                // reconstructed even if the process died before stack commit.
                history = crate::undo::UndoStack::new();
                history.set_persistence_revision(revision);
            } else if let Some(pointer) = &record.history_operation {
                let operation = history.prepare(pointer.direction).ok_or_else(|| {
                    invalid("interrupted history transaction has no matching stack entry")
                })?;
                if operation.batch.persisted_id() != pointer.batch_id
                    || !history.complete(&operation)
                {
                    return Err(invalid(
                        "interrupted history transaction no longer matches the stack",
                    ));
                }
            } else if !history_origin(&record.origin) {
                history.record_batch(load_batch(
                    StoredUndoBatch {
                        files: record.files.clone(),
                        id: record.batch_id,
                    },
                    record.id,
                )?);
            }
            seen.insert(record.id);
        }
        let mut seen: Vec<_> = seen.into_iter().collect();
        seen.sort();
        history.set_recovery_seen(seen);
        Ok(history)
    }
    fn root(&self) -> PathBuf {
        self.scope.join("recovery")
    }
    fn directory(&self, id: Uuid) -> PathBuf {
        self.root().join(id.to_string())
    }
    fn access(&self) -> SessionAccessCoordinator {
        SessionAccessCoordinator::new(&self.scope)
    }
    fn maintenance(&self) -> io::Result<SessionWriterLease> {
        SessionAccessCoordinator::new(self.scope.join(".recovery-maintenance"))
            .try_writer(Uuid::nil())
    }
    fn validate_context(&self, context: &RecoveryContext) -> io::Result<()> {
        self.access()
            .validate(&context.writer, context.session_id)?;
        if std::fs::canonicalize(&context.store.scope)? != std::fs::canonicalize(&self.scope)? {
            return Err(invalid("recovery context belongs to another profile"));
        }
        Ok(())
    }

    pub fn prepare(
        &self,
        context: &RecoveryContext,
        batch: &FileMutationBatchReceipt,
    ) -> io::Result<CaptureTicket> {
        self.validate_context(context)?;
        let _maintenance = self.maintenance()?;
        self.prepare_locked(context, batch)
    }

    fn prepare_locked(
        &self,
        context: &RecoveryContext,
        batch: &FileMutationBatchReceipt,
    ) -> io::Result<CaptureTicket> {
        self.prepare_locked_with_history(context, batch, None)
    }

    fn prepare_locked_with_history(
        &self,
        context: &RecoveryContext,
        batch: &FileMutationBatchReceipt,
        history_operation: Option<HistoryOperationPointer>,
    ) -> io::Result<CaptureTicket> {
        let mut journal = self.read_journal(context.session_id)?;
        self.prune_blobs(&journal)?;
        let mut files = Vec::new();
        let mut blobs = BTreeMap::<String, Vec<u8>>::new();
        for receipt in batch.receipts() {
            let (root, relative_path) = record_path(&context.workspace_roots, &receipt.path)?;
            files.push(RecordedFile {
                root,
                relative_path,
                action: receipt.action.clone(),
                before: store_state(&receipt.before, &mut blobs)?,
                after: store_state(&receipt.after, &mut blobs)?,
            });
        }
        let record_id = Uuid::new_v4();
        let capture_revision = next_revision(journal.revision)?;
        let group = group_mut(&mut journal, context.turn_id)?;
        if group.completed {
            return Err(invalid("recovery turn is already completed"));
        }
        if group.records.len() >= 1024 {
            return Err(invalid("recovery turn exceeds its mutation bound"));
        }
        let mut distinct: HashSet<(PathBuf, PathBuf)> = group
            .records
            .iter()
            .flat_map(|r| &r.files)
            .map(|f| (f.root.path.clone(), f.relative_path.clone()))
            .collect();
        distinct.extend(
            files
                .iter()
                .map(|f| (f.root.path.clone(), f.relative_path.clone())),
        );
        if distinct.len() > crate::undo::MAX_MUTATION_BATCH_FILES {
            return Err(invalid("recovery turn exceeds its file bound"));
        }
        group.records.push(Intent {
            id: record_id,
            origin: context.origin.chars().take(128).collect(),
            status: IntentStatus::Pending,
            files,
            batch_id: Some(batch.persisted_id()),
            history_operation,
            history_reset: context.origin == "human_recovery",
            capture_revision,
            applied_revision: None,
        });
        journal.revision = next_revision(journal.revision)?;
        let mut bytes = encode_journal(&journal)?;
        let dir = self.directory(context.session_id);
        let old_metadata = std::fs::metadata(dir.join("journal.json"))
            .map(|m| m.len())
            .unwrap_or(0);
        let blob_bytes: u64 = blobs
            .iter()
            .filter(|(digest, _)| !dir.join("blobs").join(digest).exists())
            .map(|(_, bytes)| bytes.len() as u64)
            .sum();
        self.ensure_capacity(
            context,
            blob_bytes + (bytes.len() as u64).saturating_sub(old_metadata),
        )?;
        // Quota pruning can remove completed groups from this session. Merge
        // that removal before publishing the new intent, without resurrecting
        // groups whose unreferenced blobs have already been pruned.
        let latest = self.read_journal(context.session_id)?;
        let retained: HashSet<Uuid> = latest.groups.iter().map(|g| g.turn_id).collect();
        journal
            .groups
            .retain(|g| g.turn_id == context.turn_id || retained.contains(&g.turn_id));
        journal.revision = next_revision(latest.revision)?;
        group_mut(&mut journal, context.turn_id)?
            .records
            .iter_mut()
            .find(|record| record.id == record_id)
            .ok_or_else(|| invalid("prepared intent was lost during quota maintenance"))?
            .capture_revision = journal.revision;
        bytes = encode_journal(&journal)?;
        private_dir(&self.root())?;
        private_dir(&dir)?;
        private_dir(&dir.join("blobs"))?;
        for (digest, blob) in blobs {
            let path = dir.join("blobs").join(&digest);
            if path.exists() {
                read_blob(&path, &digest, blob.len())?;
            } else {
                atomic_write(&path, &blob)?;
            }
        }
        atomic_write(&dir.join("journal.json"), &bytes)?;
        Ok(CaptureTicket {
            session_id: context.session_id,
            turn_id: context.turn_id,
            record_id,
        })
    }

    pub fn complete(&self, context: &RecoveryContext, ticket: &CaptureTicket) -> io::Result<()> {
        self.validate_context(context)?;
        if ticket.session_id != context.session_id || ticket.turn_id != context.turn_id {
            return Err(invalid("recovery ticket belongs to another turn"));
        }
        let _maintenance = self.maintenance()?;
        self.complete_locked(context, ticket)
    }

    fn complete_locked(&self, context: &RecoveryContext, ticket: &CaptureTicket) -> io::Result<()> {
        let mut journal = self.read_journal(context.session_id)?;
        let applied_revision = next_revision(journal.revision)?;
        let intent = journal
            .groups
            .iter_mut()
            .find(|g| g.turn_id == ticket.turn_id)
            .and_then(|g| g.records.iter_mut().find(|r| r.id == ticket.record_id))
            .ok_or_else(|| invalid("recovery intent is missing"))?;
        intent.status = self.classify_intent(context.session_id, intent)?;
        if intent.status == IntentStatus::Applied {
            intent.applied_revision.get_or_insert(applied_revision);
        } else {
            intent.applied_revision = None;
        }
        self.write_journal(&mut journal)
    }

    /// Reconcile interrupted captures by observation only; never publish edits.
    pub fn reconcile(&self, context: &RecoveryContext) -> io::Result<Vec<RecoveryTurnSummary>> {
        self.validate_context(context)?;
        let _maintenance = self.maintenance()?;
        let mut journal = self.read_journal(context.session_id)?;
        for group in &mut journal.groups {
            let mut interrupted = false;
            for intent in &mut group.records {
                if intent.status == IntentStatus::Pending {
                    intent.status = self.classify_intent(context.session_id, intent)?;
                    if intent.status == IntentStatus::Applied && intent.capture_revision != 0 {
                        intent.applied_revision = Some(intent.capture_revision);
                    }
                    interrupted = true;
                }
            }
            if interrupted || !group.completed {
                group.incomplete = true;
                group.completed = !group
                    .records
                    .iter()
                    .any(|r| matches!(r.status, IntentStatus::Pending | IntentStatus::Conflict));
            }
        }
        self.write_journal(&mut journal)?;
        Ok(summaries(&journal))
    }

    pub fn finish_turn(&self, context: &RecoveryContext) -> io::Result<()> {
        self.validate_context(context)?;
        if context
            .mutations
            .state
            .lock()
            .map_err(|_| invalid("mutation tracker failed"))?
            .active
            != 0
        {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "admitted mutations have not drained",
            ));
        }
        let _maintenance = self.maintenance()?;
        self.finish_turn_locked(context)
    }

    fn finish_turn_locked(&self, context: &RecoveryContext) -> io::Result<()> {
        let mut journal = self.read_journal(context.session_id)?;
        if let Some(group) = journal
            .groups
            .iter_mut()
            .find(|g| g.turn_id == context.turn_id)
        {
            group.completed = !group
                .records
                .iter()
                .any(|r| matches!(r.status, IntentStatus::Pending | IntentStatus::Conflict));
        }
        self.prune_count(&mut journal);
        self.write_journal(&mut journal)?;
        self.prune_blobs(&journal)
    }

    pub fn mark_incomplete(&self, context: &RecoveryContext) -> io::Result<()> {
        self.validate_context(context)?;
        let _maintenance = self.maintenance()?;
        let mut journal = self.read_journal(context.session_id)?;
        group_mut(&mut journal, context.turn_id)?.incomplete = true;
        self.write_journal(&mut journal)
    }

    pub fn list(&self, session_id: Uuid) -> io::Result<Vec<RecoveryTurnSummary>> {
        Ok(summaries(&self.read_journal(session_id)?))
    }

    pub fn preview(&self, context: &RecoveryContext, turn_id: Uuid) -> io::Result<RecoveryPreview> {
        self.validate_context(context)?;
        let journal = self.read_journal(context.session_id)?;
        let group = journal
            .groups
            .iter()
            .find(|g| g.turn_id == turn_id)
            .ok_or_else(|| invalid("recovery turn is missing"))?;
        if !group.completed
            || group
                .records
                .iter()
                .any(|r| matches!(r.status, IntentStatus::Pending | IntentStatus::Conflict))
        {
            return Err(invalid(
                "recovery turn needs reconciliation before restoration",
            ));
        }
        let mut collapsed = BTreeMap::<PathBuf, FileMutationReceipt>::new();
        for intent in group
            .records
            .iter()
            .filter(|r| r.status == IntentStatus::Applied)
        {
            for file in &intent.files {
                let path = resolve_recorded_path(file)?;
                record_path(&context.workspace_roots, &path)?;
                let before = self.load_state(context.session_id, &file.before)?;
                let after = self.load_state(context.session_id, &file.after)?;
                match collapsed.get_mut(&path) {
                    Some(existing) => {
                        if existing.after != before {
                            return Err(invalid(
                                "unrecorded edits interrupt this turn's recovery chain",
                            ));
                        }
                        existing.after = after;
                    }
                    None => {
                        collapsed.insert(
                            path.clone(),
                            FileMutationReceipt::new(path, "recover_turn", before, after),
                        );
                    }
                }
            }
        }
        let mutation =
            FileMutationBatchReceipt::new(collapsed.into_values().collect()).map_err(invalid)?;
        for file in mutation.receipts() {
            if crate::undo::read_file_state(&file.path).map_err(invalid)? != file.after {
                return Err(invalid("a recorded target changed after this turn"));
            }
        }
        let token = preview_token(&journal, turn_id, &mutation.digest)?;
        let (changes_text, changes_truncated) = recovery_changes_text(&mutation);
        Ok(RecoveryPreview {
            session_id: context.session_id,
            turn_id,
            files: mutation.receipts().iter().map(|r| r.safe_path()).collect(),
            incomplete: group.incomplete,
            changes_text,
            changes_truncated,
            token,
            journal_revision: journal.revision,
            mutation,
        })
    }

    /// A restoration is itself a durable, reversible typed edit. No transcript,
    /// Git index, untracked scope, or unrelated file is changed here.
    #[cfg(test)]
    pub fn restore(
        &self,
        context: &RecoveryContext,
        preview: &RecoveryPreview,
    ) -> io::Result<Uuid> {
        self.restore_with_outcome(context, preview)
            .map(|outcome| outcome.rescue_turn)
    }

    pub fn restore_with_outcome(
        &self,
        context: &RecoveryContext,
        preview: &RecoveryPreview,
    ) -> io::Result<RecoveryRestoreOutcome> {
        self.validate_context(context)?;
        let _maintenance = self.maintenance()?;
        let _workspace = self.try_workspace_restore(&context.workspace_roots)?;
        let journal = self.read_journal(context.session_id)?;
        if preview.session_id != context.session_id
            || journal.revision != preview.journal_revision
            || preview_token(&journal, preview.turn_id, &preview.mutation.digest)? != preview.token
        {
            return Err(invalid("recovery preview is stale"));
        }
        let rescue_turn = Uuid::new_v4();
        let mut rescue_context = context.clone();
        rescue_context.turn_id = rescue_turn;
        rescue_context.origin = "human_recovery".into();
        let inverse = FileMutationBatchReceipt::new(
            preview
                .mutation
                .receipts()
                .iter()
                .map(|r| {
                    FileMutationReceipt::new(
                        r.path.clone(),
                        "recover_turn",
                        r.after.clone(),
                        r.before.clone(),
                    )
                })
                .collect(),
        )
        .map_err(invalid)?;
        // Restoration differs from normal edit capture: without a durable rescue
        // transaction, it cannot safely proceed.
        let ticket = self.prepare_locked(&rescue_context, &inverse)?;
        let result = PreparedFileMutation::from_receipt(inverse)
            .map_err(invalid)?
            .apply();
        let mut warning = self
            .complete_locked(&rescue_context, &ticket)
            .err()
            .map(|_| {
                "Recorded edits restored; recovery completion needs reconciliation.".to_owned()
            });
        if self.finish_turn_locked(&rescue_context).is_err() {
            warning = Some("Recorded edits restored; the rescue turn needs reconciliation.".into());
        }
        result.map_err(invalid)?;
        // A turn restoration resets native undo; its rescue turn remains the
        // durable redo route. This prevents old undo entries resurfacing after
        // reopening a session whose recorded turn was explicitly restored.
        if self.clear_undo_stack_locked(&rescue_context).is_err() {
            warning = Some("Recorded edits restored; durable undo history needs reconciliation before another restoration.".into());
            if let Ok(mut journal) = self.read_journal(context.session_id) {
                if let Ok(group) = group_mut(&mut journal, rescue_turn) {
                    group.incomplete = true;
                    let _ = self.write_journal(&mut journal);
                }
            }
        }
        Ok(RecoveryRestoreOutcome {
            rescue_turn,
            warning,
        })
    }

    fn clear_undo_stack_locked(&self, context: &RecoveryContext) -> io::Result<()> {
        let journal = self.read_journal(context.session_id)?;
        let mut history = crate::undo::UndoStack::new();
        history.set_persistence_revision(journal.undo.as_ref().map(|stack| stack.revision));
        history.set_recovery_seen(
            journal
                .groups
                .iter()
                .flat_map(|group| &group.records)
                .filter(|record| record.status == IntentStatus::Applied)
                .map(|record| record.id)
                .collect(),
        );
        self.save_undo_stack_locked(context, &history, None)
    }

    fn classify_intent(&self, id: Uuid, intent: &Intent) -> io::Result<IntentStatus> {
        let mut before = true;
        let mut after = true;
        for file in &intent.files {
            let Ok(path) = resolve_recorded_path(file) else {
                return Ok(IntentStatus::Conflict);
            };
            let Ok(current) = crate::undo::read_file_state(&path) else {
                return Ok(IntentStatus::Conflict);
            };
            before &= current == self.load_state(id, &file.before)?;
            after &= current == self.load_state(id, &file.after)?;
        }
        Ok(if after {
            IntentStatus::Applied
        } else if before {
            IntentStatus::Aborted
        } else {
            IntentStatus::Conflict
        })
    }

    fn read_journal(&self, id: Uuid) -> io::Result<Journal> {
        check_directory_if_present(&self.root())?;
        check_directory_if_present(&self.directory(id))?;
        let path = self.directory(id).join("journal.json");
        if !path.exists() {
            return Ok(Journal {
                version: JOURNAL_VERSION,
                session_id: id,
                revision: 0,
                groups: Vec::new(),
                undo: None,
            });
        }
        let metadata = std::fs::symlink_metadata(&path)?;
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || metadata.len() > MAX_JOURNAL_BYTES
        {
            return Err(invalid("recovery journal exceeds its private file bounds"));
        }
        let journal: Journal =
            serde_json::from_reader(File::open(path)?).map_err(io::Error::other)?;
        if journal.version != JOURNAL_VERSION
            || journal.session_id != id
            || journal.groups.len() > 128
        {
            return Err(invalid("recovery journal identity or version is invalid"));
        }
        let mut groups = HashSet::new();
        let mut ids = HashSet::new();
        if journal.undo.as_ref().is_some_and(|stack| {
            stack.undo.len().saturating_add(stack.redo.len()) > crate::undo::MAX_UNDO_ENTRIES
                || stack.seen_records.len() > 16384
                || stack.undo.iter().chain(&stack.redo).any(|batch| {
                    batch.files.is_empty()
                        || batch.files.len() > crate::undo::MAX_MUTATION_BATCH_FILES
                })
        }) {
            return Err(invalid("persisted undo pointers exceed their bounds"));
        }
        for group in &journal.groups {
            if !groups.insert(group.turn_id) || group.records.len() > 1024 {
                return Err(invalid("recovery journal groups are invalid"));
            }
            for intent in &group.records {
                if !ids.insert(intent.id)
                    || intent.files.is_empty()
                    || intent.files.len() > crate::undo::MAX_MUTATION_BATCH_FILES
                    || intent.capture_revision > journal.revision
                    || intent.applied_revision.is_some_and(|revision| {
                        revision > journal.revision
                            || revision < intent.capture_revision
                            || intent.status != IntentStatus::Applied
                    })
                {
                    return Err(invalid("recovery intent exceeds its bounds"));
                }
            }
        }
        Ok(journal)
    }

    fn write_journal(&self, journal: &mut Journal) -> io::Result<()> {
        journal.revision = next_revision(journal.revision)?;
        private_dir(&self.root())?;
        private_dir(&self.directory(journal.session_id))?;
        atomic_write(
            &self.directory(journal.session_id).join("journal.json"),
            &encode_journal(journal)?,
        )
    }

    fn load_state(&self, id: Uuid, state: &StoredState) -> io::Result<FileState> {
        match state {
            StoredState::Missing => Ok(FileState::Missing),
            StoredState::Utf8 { sha256, bytes } => {
                let data = read_blob(
                    &self.directory(id).join("blobs").join(sha256),
                    sha256,
                    *bytes,
                )?;
                Ok(FileState::Utf8(
                    String::from_utf8(data).map_err(io::Error::other)?,
                ))
            }
        }
    }

    fn prune_count(&self, journal: &mut Journal) {
        let prunable = |g: &TurnGroup| {
            g.completed
                && !g
                    .records
                    .iter()
                    .any(|r| matches!(r.status, IntentStatus::Pending | IntentStatus::Conflict))
        };
        let mut completed = journal.groups.iter().filter(|g| prunable(g)).count();
        journal.groups.retain(|g| {
            if prunable(g) && completed > crate::undo::MAX_UNDO_ENTRIES {
                completed -= 1;
                false
            } else {
                true
            }
        });
    }

    fn prune_blobs(&self, journal: &Journal) -> io::Result<()> {
        let referenced: HashSet<&str> = recorded_files(journal)
            .flat_map(|f| [&f.before, &f.after])
            .filter_map(|s| match s {
                StoredState::Utf8 { sha256, .. } => Some(sha256.as_str()),
                StoredState::Missing => None,
            })
            .collect();
        let path = self.directory(journal.session_id).join("blobs");
        if !path.exists() {
            return Ok(());
        }
        check_directory_if_present(&path)?;
        for entry in std::fs::read_dir(&path)? {
            let entry = entry?;
            if entry.file_type()?.is_file()
                && !referenced.contains(entry.file_name().to_string_lossy().as_ref())
            {
                std::fs::remove_file(entry.path())?;
            }
        }
        Ok(())
    }

    fn ensure_capacity(&self, context: &RecoveryContext, extra: u64) -> io::Result<()> {
        if extra > self.max_bytes {
            return Err(invalid("recovery capture exceeds its storage quota"));
        }
        if tree_bytes(&self.root())?.saturating_add(extra) <= self.max_bytes {
            return Ok(());
        }
        let mut candidates = Vec::new();
        if self.root().exists() {
            for entry in std::fs::read_dir(self.root())? {
                let entry = entry?;
                if !entry.file_type()?.is_dir() {
                    continue;
                }
                let Ok(id) = Uuid::parse_str(&entry.file_name().to_string_lossy()) else {
                    continue;
                };
                let journal = self.read_journal(id)?;
                for group in journal.groups.iter().filter(|g| {
                    g.completed
                        && !g.records.iter().any(|r| {
                            matches!(r.status, IntentStatus::Pending | IntentStatus::Conflict)
                        })
                }) {
                    candidates.push((group.created_at, id, group.turn_id));
                }
            }
        }
        candidates.sort();
        for (_, id, turn_id) in candidates {
            if id == context.session_id && turn_id == context.turn_id {
                continue;
            }
            let other = if id != context.session_id {
                match self.access().try_writer(id) {
                    Ok(lease) => Some(lease),
                    Err(_) => continue,
                }
            } else {
                None
            };
            let mut journal = self.read_journal(id)?;
            journal.groups.retain(|g| g.turn_id != turn_id);
            self.write_journal(&mut journal)?;
            self.prune_blobs(&journal)?;
            drop(other);
            if tree_bytes(&self.root())?.saturating_add(extra) <= self.max_bytes {
                return Ok(());
            }
        }
        Err(invalid(
            "recovery storage quota is full; active and conflicted captures were preserved",
        ))
    }
}

impl RecoveryContext {
    pub fn new(
        store: Arc<RecoveryStore>,
        session_id: Uuid,
        turn_id: Uuid,
        workspace_roots: Vec<PathBuf>,
        origin: impl Into<String>,
        writer: SessionWriterLease,
    ) -> Self {
        Self {
            store,
            session_id,
            turn_id,
            workspace_roots,
            origin: origin.into(),
            writer,
            activity: None,
            mutations: Arc::new(MutationTracker::default()),
        }
    }

    pub fn with_agent(&self, agent: impl AsRef<str>, workspace_roots: Vec<PathBuf>) -> Self {
        let mut child = self.clone();
        child.origin = format!(
            "agent:{}",
            agent.as_ref().chars().take(120).collect::<String>()
        );
        child.workspace_roots = workspace_roots;
        child
    }

    pub fn with_activity(&self, activity: WorkspaceActivityLease) -> Self {
        let mut context = self.clone();
        context.activity = Some(activity);
        context
    }

    pub fn admit_mutation(&self) -> io::Result<RecoveryMutationAdmission> {
        let mut state = self
            .mutations
            .state
            .lock()
            .map_err(|_| invalid("mutation tracker failed"))?;
        if state.closed {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "this turn has retired; mutation was not started",
            ));
        }
        state.active = state
            .active
            .checked_add(1)
            .ok_or_else(|| invalid("mutation admission overflow"))?;
        Ok(RecoveryMutationAdmission {
            tracker: self.mutations.clone(),
            session_id: self.session_id,
            turn_id: self.turn_id,
        })
    }

    pub fn close_mutations(&self) {
        self.mutations
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .closed = true;
    }

    pub async fn close_and_drain_mutations(&self) {
        self.close_mutations();
        self.wait_mutations_idle().await;
    }

    pub fn has_active_mutations(&self) -> bool {
        self.mutations
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .active
            != 0
    }

    /// Wait for currently admitted native work without retiring live children.
    /// Final turn sealing must use close_and_drain_mutations instead.
    pub async fn wait_mutations_idle(&self) {
        loop {
            let notified = self.mutations.drained.notified();
            tokio::pin!(notified);
            // Register before observing the count so the final worker cannot
            // retire between the check and subscription.
            notified.as_mut().enable();
            if self
                .mutations
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .active
                == 0
            {
                return;
            }
            notified.await;
        }
    }

    /// Capture errors never veto an otherwise authorized normal edit.
    #[cfg(test)]
    pub fn apply(&self, prepared: PreparedFileMutation) -> RecordedMutationOutcome {
        match self.admit_mutation() {
            Ok(admission) => self.apply_admitted(prepared, admission),
            Err(error) => RecordedMutationOutcome {
                result: Err(error.to_string()),
                warning: None,
            },
        }
    }

    pub fn apply_admitted(
        &self,
        prepared: PreparedFileMutation,
        admission: RecoveryMutationAdmission,
    ) -> RecordedMutationOutcome {
        let _admission = admission;
        if !Arc::ptr_eq(&self.mutations, &_admission.tracker)
            || self.session_id != _admission.session_id
            || self.turn_id != _admission.turn_id
        {
            return RecordedMutationOutcome {
                result: Err("mutation admission belongs to another turn".into()),
                warning: None,
            };
        }
        let mut warning = None;
        let ticket = match self.store.prepare(self, prepared.receipt()) {
            Ok(ticket) => Some(ticket),
            Err(_) => {
                warning = Some("Recovery capture failed; this edit continues without complete recovery coverage.".into());
                let _ = self.store.mark_incomplete(self);
                None
            }
        };
        let result = prepared.apply();
        if let Some(ticket) = &ticket {
            if self.store.complete(self, ticket).is_err() {
                warning = Some(
                    "Recovery completion could not be saved; inspect recovery before restoring."
                        .into(),
                );
            }
        }
        if let Ok(receipt) = &result {
            if self
                .store
                .retain_receipt(self, receipt, ticket.as_ref())
                .is_err()
            {
                warning.get_or_insert_with(|| "Edit completed; durable undo retention failed and recovery coverage may be incomplete.".into());
                let _ = self.store.mark_incomplete(self);
            }
        }
        RecordedMutationOutcome { result, warning }
    }
}

fn group_mut(journal: &mut Journal, turn_id: Uuid) -> io::Result<&mut TurnGroup> {
    if let Some(index) = journal.groups.iter().position(|g| g.turn_id == turn_id) {
        return Ok(&mut journal.groups[index]);
    }
    if journal.groups.len() >= 128 {
        return Err(invalid("recovery journal exceeds its turn bound"));
    }
    journal.groups.push(TurnGroup {
        turn_id,
        created_at: chrono::Utc::now().timestamp_millis(),
        completed: false,
        incomplete: false,
        records: Vec::new(),
    });
    Ok(journal.groups.last_mut().expect("new recovery group"))
}

fn recorded_files(journal: &Journal) -> impl Iterator<Item = &RecordedFile> {
    journal
        .groups
        .iter()
        .flat_map(|group| &group.records)
        .flat_map(|record| &record.files)
        .chain(
            journal
                .undo
                .iter()
                .flat_map(|stack| stack.undo.iter().chain(&stack.redo))
                .flat_map(|batch| &batch.files),
        )
}

fn history_origin(origin: &str) -> bool {
    matches!(
        origin,
        "human_recovery" | "human_history" | "human_undo" | "human_redo"
    )
}

fn summaries(journal: &Journal) -> Vec<RecoveryTurnSummary> {
    journal
        .groups
        .iter()
        .rev()
        .map(|g| RecoveryTurnSummary {
            turn_id: g.turn_id,
            file_count: g
                .records
                .iter()
                .flat_map(|r| &r.files)
                .map(|f| (&f.root.path, &f.relative_path))
                .collect::<HashSet<_>>()
                .len(),
            completed: g.completed,
            incomplete: g.incomplete,
            conflicts: g.records.iter().any(|r| r.status == IntentStatus::Conflict),
        })
        .collect()
}

fn store_state(
    state: &FileState,
    blobs: &mut BTreeMap<String, Vec<u8>>,
) -> io::Result<StoredState> {
    match state {
        FileState::Missing => Ok(StoredState::Missing),
        FileState::Utf8(text) => {
            if text.len() > crate::undo::MAX_UNDO_FILE_BYTES {
                return Err(invalid("recovery file exceeds its content bound"));
            }
            let digest = format!("{:x}", Sha256::digest(text.as_bytes()));
            blobs
                .entry(digest.clone())
                .or_insert_with(|| text.as_bytes().to_vec());
            Ok(StoredState::Utf8 {
                sha256: digest,
                bytes: text.len(),
            })
        }
    }
}

fn record_path(roots: &[PathBuf], path: &Path) -> io::Result<(RootIdentity, PathBuf)> {
    for root in roots {
        let root = std::fs::canonicalize(root)?;
        if let Ok(relative) = path.strip_prefix(&root) {
            let identity = root_identity(&root)?;
            let file = RecordedFile {
                root: identity.clone(),
                relative_path: relative.to_owned(),
                action: String::new(),
                before: StoredState::Missing,
                after: StoredState::Missing,
            };
            resolve_recorded_path(&file)?;
            return Ok((identity, relative.to_owned()));
        }
    }
    Err(invalid(
        "recovery target is outside its recorded workspace roots",
    ))
}

fn root_identity(path: &Path) -> io::Result<RootIdentity> {
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(invalid("recovery workspace root is not a directory"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Ok(RootIdentity {
            path: path.to_owned(),
            device: metadata.dev(),
            inode: metadata.ino(),
        })
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "recovery workspace identity is unavailable",
        ))
    }
}

fn resolve_recorded_path(file: &RecordedFile) -> io::Result<PathBuf> {
    if file.relative_path.as_os_str().is_empty()
        || !file
            .relative_path
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
    {
        return Err(invalid("recovery target is not a contained relative path"));
    }
    let current = root_identity(&file.root.path)?;
    if current.device != file.root.device || current.inode != file.root.inode {
        return Err(invalid("recovery workspace root identity changed"));
    }
    let path = current.path.join(&file.relative_path);
    let parent = path
        .parent()
        .ok_or_else(|| invalid("recovery target has no parent"))?;
    if std::fs::canonicalize(parent)? != parent || !parent.starts_with(&current.path) {
        return Err(invalid(
            "recovery target parent changed or traverses a symlink",
        ));
    }
    Ok(path)
}

fn read_blob(path: &Path, digest: &str, expected_bytes: usize) -> io::Result<Vec<u8>> {
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        || expected_bytes > crate::undo::MAX_UNDO_FILE_BYTES
    {
        return Err(invalid("recovery blob identity exceeds its bounds"));
    }
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() != expected_bytes as u64
    {
        return Err(invalid("recovery blob is not the recorded private file"));
    }
    let mut bytes = Vec::new();
    File::open(path)?
        .take((crate::undo::MAX_UNDO_FILE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() != expected_bytes || format!("{:x}", Sha256::digest(&bytes)) != digest {
        return Err(invalid("recovery blob digest does not match"));
    }
    Ok(bytes)
}

fn recovery_changes_text(mutation: &FileMutationBatchReceipt) -> (String, bool) {
    const LIMIT: usize = 64 * 1024;
    let mut output = String::new();
    let mut truncated = false;
    let mut append = |text: &str| {
        let remaining = LIMIT.saturating_sub(output.len());
        if text.len() <= remaining {
            output.push_str(text);
        } else {
            let mut end = remaining;
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            output.push_str(&text[..end]);
            truncated = true;
        }
    };
    for file in mutation.receipts() {
        append(&format!("\nFile: {}\n", file.safe_path()));
        for (label, prefix, state) in [
            ("current", '-', &file.after),
            ("restored", '+', &file.before),
        ] {
            match state {
                FileState::Missing => append(&format!("{label}: file absent\n")),
                FileState::Utf8(text) => {
                    append(&format!(
                        "{label}: {} bytes · sha256 {:x}\n",
                        text.len(),
                        Sha256::digest(text.as_bytes())
                    ));
                    for line in text.split_inclusive('\n') {
                        append(&format!(
                            "{prefix}{}",
                            crate::terminal_text::sanitize_terminal_text(line)
                        ));
                        if !line.ends_with('\n') {
                            append("\n\\ No newline at end of file\n");
                        }
                    }
                }
            }
        }
    }
    if truncated {
        output.push_str(
            "\nPreview truncated at 64 KiB; confirmation still binds every saved byte.\n",
        );
    }
    (output, truncated)
}

fn preview_token(journal: &Journal, turn: Uuid, mutation: &str) -> io::Result<String> {
    let mut digest = Sha256::new();
    digest.update(b"ca-recovery-preview-v1\0");
    digest.update(encode_journal(journal)?);
    digest.update(turn.as_bytes());
    digest.update(mutation.as_bytes());
    Ok(format!("{:x}", digest.finalize()))
}
fn encode_journal(journal: &Journal) -> io::Result<Vec<u8>> {
    let bytes = serde_json::to_vec(journal).map_err(io::Error::other)?;
    if bytes.len() as u64 > MAX_JOURNAL_BYTES {
        return Err(invalid("recovery journal exceeds its metadata bound"));
    }
    Ok(bytes)
}
fn next_revision(revision: u64) -> io::Result<u64> {
    revision
        .checked_add(1)
        .ok_or_else(|| invalid("recovery journal revision exhausted"))
}
fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn private_dir(path: &Path) -> io::Result<()> {
    std::fs::create_dir_all(path)?;
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(invalid("recovery storage is not a private directory"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn check_directory_if_present(path: &Path) -> io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => Ok(()),
        Ok(_) => Err(invalid("recovery storage is not a private directory")),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| invalid("recovery file has no parent"))?;
    private_dir(parent)?;
    let temp = parent.join(format!(".recovery-{}.tmp", Uuid::new_v4()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    let result = (|| {
        let mut file = options.open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        std::fs::rename(&temp, path)?;
        File::open(parent)?.sync_all()
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

fn tree_bytes(path: &Path) -> io::Result<u64> {
    tree_bytes_at(path, 0)
}

fn tree_bytes_at(path: &Path, depth: usize) -> io::Result<u64> {
    if !path.exists() {
        return Ok(0);
    }
    if depth > 3 {
        return Err(invalid("recovery storage exceeds its directory bound"));
    }
    let mut bytes = 0_u64;
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            return Err(invalid("recovery storage contains a symlink"));
        }
        bytes = bytes.saturating_add(if kind.is_dir() {
            tree_bytes_at(&entry.path(), depth + 1)?
        } else if kind.is_file() {
            entry.metadata()?.len()
        } else {
            return Err(invalid("recovery storage contains a non-file"));
        });
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        _profile: tempfile::TempDir,
        _workspace: tempfile::TempDir,
        context: RecoveryContext,
        root: PathBuf,
    }

    fn fixture(quota: u64) -> Fixture {
        let profile = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let scope = std::fs::canonicalize(profile.path()).unwrap();
        let root = std::fs::canonicalize(workspace.path()).unwrap();
        let session_id = Uuid::new_v4();
        let writer = SessionAccessCoordinator::new(&scope)
            .try_writer(session_id)
            .unwrap();
        let context = RecoveryContext::new(
            Arc::new(RecoveryStore::with_quota(scope, quota)),
            session_id,
            Uuid::new_v4(),
            vec![root.clone()],
            "main",
            writer,
        );
        Fixture {
            _profile: profile,
            _workspace: workspace,
            context,
            root,
        }
    }

    fn edit(context: &RecoveryContext, path: &Path, before: FileState, after: FileState) {
        let outcome = context.apply(
            PreparedFileMutation::single(path.to_owned(), "write_file", before, after).unwrap(),
        );
        assert!(outcome.warning.is_none(), "{:?}", outcome.warning);
        outcome.result.unwrap();
    }

    fn capture_without_undo(context: &RecoveryContext, path: &Path, text: String) {
        let prepared = PreparedFileMutation::single(
            path.to_owned(),
            "write_file",
            FileState::Missing,
            FileState::Utf8(text),
        )
        .unwrap();
        let ticket = context.store.prepare(context, prepared.receipt()).unwrap();
        prepared.apply().unwrap();
        context.store.complete(context, &ticket).unwrap();
    }

    #[test]
    fn restarted_turn_restores_create_modify_delete_and_durable_redo() {
        let f = fixture(DEFAULT_RECOVERY_BYTES);
        let modify = f.root.join("modify.txt");
        let create = f.root.join("create.txt");
        let delete = f.root.join("delete.txt");
        std::fs::write(&modify, "original").unwrap();
        std::fs::write(&delete, "deleted").unwrap();
        edit(
            &f.context,
            &modify,
            FileState::Utf8("original".into()),
            FileState::Utf8("intermediate".into()),
        );
        edit(
            &f.context,
            &modify,
            FileState::Utf8("intermediate".into()),
            FileState::Utf8("final".into()),
        );
        edit(
            &f.context,
            &create,
            FileState::Missing,
            FileState::Utf8("created".into()),
        );
        edit(
            &f.context,
            &delete,
            FileState::Utf8("deleted".into()),
            FileState::Missing,
        );
        f.context.store.finish_turn(&f.context).unwrap();
        let restarted = RecoveryStore::new(&f.context.store.scope);
        let preview = restarted.preview(&f.context, f.context.turn_id).unwrap();
        assert_eq!(preview.files.len(), 3);
        let rescue = restarted.restore(&f.context, &preview).unwrap();
        assert_eq!(std::fs::read_to_string(&modify).unwrap(), "original");
        assert!(!create.exists());
        assert_eq!(std::fs::read_to_string(&delete).unwrap(), "deleted");
        let redo = restarted.preview(&f.context, rescue).unwrap();
        restarted.restore(&f.context, &redo).unwrap();
        assert_eq!(std::fs::read_to_string(modify).unwrap(), "final");
        assert_eq!(std::fs::read_to_string(create).unwrap(), "created");
        assert!(!delete.exists());
    }

    #[test]
    fn capture_storage_failure_warns_and_continues_exact_edit() {
        let f = fixture(0);
        let path = f.root.join("sample.txt");
        let result = f.context.apply(
            PreparedFileMutation::single(
                path.clone(),
                "write_file",
                FileState::Missing,
                FileState::Utf8("authorized".into()),
            )
            .unwrap(),
        );
        result.result.unwrap();
        assert!(result.warning.unwrap().contains("continues"));
        assert_eq!(std::fs::read_to_string(path).unwrap(), "authorized");
        assert!(f.context.store.list(f.context.session_id).unwrap()[0].incomplete);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn aborted_tool_waiter_still_drains_admitted_child_blocking_mutation_before_sealing() {
        let f = fixture(DEFAULT_RECOVERY_BYTES);
        let path = f.root.join("late-child.txt");
        let child = f
            .context
            .with_agent("child", f.context.workspace_roots.clone());
        let admission = child.admit_mutation().unwrap();
        let prepared = PreparedFileMutation::single(
            path.clone(),
            "write_file",
            FileState::Missing,
            FileState::Utf8("late body".into()),
        )
        .unwrap();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
        let (done_tx, done_rx) = tokio::sync::oneshot::channel();
        let waiter = tokio::spawn(async move {
            tokio::task::spawn_blocking(move || {
                let _ = ready_tx.send(());
                release_rx.recv().unwrap();
                let result = child.apply_admitted(prepared, admission);
                let _ = done_tx.send(result);
            })
            .await
        });
        ready_rx.await.unwrap();
        waiter.abort();
        f.context.close_mutations();
        assert!(f.context.has_active_mutations());
        assert!(f.context.admit_mutation().is_err());
        assert_eq!(
            f.context.store.finish_turn(&f.context).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        assert!(tokio::time::timeout(
            std::time::Duration::from_millis(20),
            f.context.close_and_drain_mutations()
        )
        .await
        .is_err());
        release_tx.send(()).unwrap();
        let outcome = tokio::time::timeout(std::time::Duration::from_secs(2), done_rx)
            .await
            .unwrap()
            .unwrap();
        outcome.result.unwrap();
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            f.context.close_and_drain_mutations(),
        )
        .await
        .unwrap();
        assert!(!f.context.has_active_mutations());
        f.context.store.finish_turn(&f.context).unwrap();
        assert_eq!(std::fs::read_to_string(path).unwrap(), "late body");
        assert!(f
            .context
            .store
            .load_undo_stack(&f.context)
            .unwrap()
            .prepare(crate::undo::UndoDirection::Undo)
            .is_some());
    }

    #[tokio::test]
    async fn mutation_idle_wait_preserves_child_admission_and_rejects_foreign_turn_guard() {
        let f = fixture(DEFAULT_RECOVERY_BYTES);
        let other = fixture(DEFAULT_RECOVERY_BYTES);
        f.context.wait_mutations_idle().await;
        let child = f
            .context
            .with_agent("child", f.context.workspace_roots.clone());
        let admission = child.admit_mutation().unwrap();
        drop(admission);
        assert!(!f.context.has_active_mutations());
        let path = f.root.join("wrong-owner.txt");
        let prepared = PreparedFileMutation::single(
            path.clone(),
            "write_file",
            FileState::Missing,
            FileState::Utf8("body".into()),
        )
        .unwrap();
        assert!(f
            .context
            .apply_admitted(prepared, other.context.admit_mutation().unwrap())
            .result
            .is_err());
        assert!(!path.exists());
        f.context.close_and_drain_mutations().await;
        assert!(child.admit_mutation().is_err());
    }

    #[test]
    fn recovery_preview_requires_current_workspace_scope() {
        let f = fixture(DEFAULT_RECOVERY_BYTES);
        edit(
            &f.context,
            &f.root.join("sample.txt"),
            FileState::Missing,
            FileState::Utf8("body".into()),
        );
        f.context.store.finish_turn(&f.context).unwrap();
        let other = tempfile::tempdir().unwrap();
        let mut narrowed = f.context.clone();
        narrowed.workspace_roots = vec![other.path().canonicalize().unwrap()];
        assert!(narrowed.store.preview(&narrowed, narrowed.turn_id).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn private_storage_symlinks_do_not_redirect_cleanup() {
        use std::os::unix::fs::symlink;
        let f = fixture(DEFAULT_RECOVERY_BYTES);
        let outside = tempfile::tempdir().unwrap();
        let kept = outside.path().join("keep.txt");
        std::fs::write(&kept, "keep").unwrap();
        std::fs::create_dir_all(f.context.store.directory(f.context.session_id)).unwrap();
        symlink(
            outside.path(),
            f.context
                .store
                .directory(f.context.session_id)
                .join("blobs"),
        )
        .unwrap();
        let outcome = f.context.apply(
            PreparedFileMutation::single(
                f.root.join("sample.txt"),
                "write_file",
                FileState::Missing,
                FileState::Utf8("body".into()),
            )
            .unwrap(),
        );
        outcome.result.unwrap();
        assert!(outcome.warning.is_some());
        assert_eq!(std::fs::read_to_string(kept).unwrap(), "keep");
    }

    #[test]
    fn interrupted_intents_reconcile_by_observation_without_writes() {
        for (apply, drift, status) in [
            (false, false, IntentStatus::Aborted),
            (true, false, IntentStatus::Applied),
            (false, true, IntentStatus::Conflict),
        ] {
            let f = fixture(DEFAULT_RECOVERY_BYTES);
            let path = f.root.join("sample.txt");
            std::fs::write(&path, "before").unwrap();
            let prepared = PreparedFileMutation::single(
                path.clone(),
                "write_file",
                FileState::Utf8("before".into()),
                FileState::Utf8("after".into()),
            )
            .unwrap();
            f.context
                .store
                .prepare(&f.context, prepared.receipt())
                .unwrap();
            if apply {
                prepared.apply().unwrap();
            }
            if drift {
                std::fs::write(&path, "external").unwrap();
            }
            let expected = std::fs::read_to_string(&path).unwrap();
            let summaries = f.context.store.reconcile(&f.context).unwrap();
            assert!(summaries[0].incomplete);
            assert_eq!(summaries[0].conflicts, drift);
            assert_eq!(
                f.context
                    .store
                    .read_journal(f.context.session_id)
                    .unwrap()
                    .groups[0]
                    .records[0]
                    .status,
                status
            );
            assert_eq!(std::fs::read_to_string(path).unwrap(), expected);
        }
    }

    #[test]
    fn interrupted_partial_batch_remains_conflicted_without_restoring_or_deriving_undo() {
        let f = fixture(DEFAULT_RECOVERY_BYTES);
        let first = f.root.join("first.txt");
        let second = f.root.join("second.txt");
        std::fs::write(&first, "before first").unwrap();
        std::fs::write(&second, "before second").unwrap();
        let receipt = FileMutationBatchReceipt::new(vec![
            FileMutationReceipt::new(
                first.clone(),
                "write_file",
                FileState::Utf8("before first".into()),
                FileState::Utf8("after first".into()),
            ),
            FileMutationReceipt::new(
                second.clone(),
                "write_file",
                FileState::Utf8("before second".into()),
                FileState::Utf8("after second".into()),
            ),
        ])
        .unwrap();
        f.context.store.prepare(&f.context, &receipt).unwrap();
        // Death between file commits leaves mixed observed states. Startup
        // cannot assume either the forward batch or its rollback completed.
        std::fs::write(&first, "after first").unwrap();
        let summaries = f.context.store.reconcile(&f.context).unwrap();
        assert!(summaries[0].conflicts);
        assert!(summaries[0].incomplete);
        assert!(f
            .context
            .store
            .preview(&f.context, f.context.turn_id)
            .is_err());
        assert!(f
            .context
            .store
            .load_undo_stack(&f.context)
            .unwrap()
            .prepare(crate::undo::UndoDirection::Undo)
            .is_none());
        assert_eq!(std::fs::read_to_string(first).unwrap(), "after first");
        assert_eq!(std::fs::read_to_string(second).unwrap(), "before second");
    }

    #[test]
    fn external_drift_and_stale_preview_never_overwrite_files() {
        let f = fixture(DEFAULT_RECOVERY_BYTES);
        let path = f.root.join("sample.txt");
        edit(
            &f.context,
            &path,
            FileState::Missing,
            FileState::Utf8("first".into()),
        );
        f.context.store.finish_turn(&f.context).unwrap();
        let preview = f
            .context
            .store
            .preview(&f.context, f.context.turn_id)
            .unwrap();
        std::fs::write(&path, "external").unwrap();
        assert!(f.context.store.restore(&f.context, &preview).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "external");
        assert!(f
            .context
            .store
            .preview(&f.context, f.context.turn_id)
            .is_err());
        std::fs::write(&path, "first").unwrap();
        assert!(f
            .context
            .store
            .restore(&f.context, &preview)
            .unwrap_err()
            .to_string()
            .contains("stale"));
    }

    #[test]
    fn completed_only_quota_pruning_does_not_resurrect_groups() {
        let f = fixture(4096);
        let first = f.root.join("first.txt");
        capture_without_undo(&f.context, &first, "a".repeat(2048));
        f.context.store.finish_turn(&f.context).unwrap();
        let mut next = f.context.clone();
        next.turn_id = Uuid::new_v4();
        capture_without_undo(&next, &f.root.join("second.txt"), "b".repeat(2048));
        next.store.finish_turn(&next).unwrap();
        let journal = next.store.read_journal(next.session_id).unwrap();
        assert_eq!(journal.groups.len(), 1);
        assert_eq!(journal.groups[0].turn_id, next.turn_id);
        assert!(tree_bytes(&next.store.root()).unwrap() <= 4096);
        assert!(next.store.preview(&next, next.turn_id).is_ok());
        assert_eq!(std::fs::read_to_string(first).unwrap().len(), 2048);
    }

    #[test]
    fn active_capture_is_preserved_when_quota_is_full() {
        let f = fixture(4096);
        edit(
            &f.context,
            &f.root.join("first.txt"),
            FileState::Missing,
            FileState::Utf8("a".repeat(2048)),
        );
        let mut next = f.context.clone();
        next.turn_id = Uuid::new_v4();
        let result = next.apply(
            PreparedFileMutation::single(
                f.root.join("second.txt"),
                "write_file",
                FileState::Missing,
                FileState::Utf8("b".repeat(2048)),
            )
            .unwrap(),
        );
        result.result.unwrap();
        assert!(result.warning.is_some());
        let journal = next.store.read_journal(next.session_id).unwrap();
        assert!(journal.groups.iter().any(
            |g| g.turn_id == f.context.turn_id && g.records[0].status == IntentStatus::Applied
        ));
    }

    #[test]
    fn retention_keeps_twenty_completed_turns_and_pending_turn() {
        let f = fixture(DEFAULT_RECOVERY_BYTES);
        let pending = f.context.turn_id;
        let prepared = PreparedFileMutation::single(
            f.root.join("pending.txt"),
            "write_file",
            FileState::Missing,
            FileState::Utf8("pending".into()),
        )
        .unwrap();
        f.context
            .store
            .prepare(&f.context, prepared.receipt())
            .unwrap();
        for index in 0..22 {
            let mut next = f.context.clone();
            next.turn_id = Uuid::new_v4();
            edit(
                &next,
                &f.root.join(format!("{index}.txt")),
                FileState::Missing,
                FileState::Utf8(format!("{index}")),
            );
            next.store.finish_turn(&next).unwrap();
        }
        let groups = f
            .context
            .store
            .read_journal(f.context.session_id)
            .unwrap()
            .groups;
        assert_eq!(groups.iter().filter(|g| g.completed).count(), 20);
        assert!(groups.iter().any(|g| g.turn_id == pending));
    }

    #[test]
    fn blobs_are_private_validated_and_not_embedded_in_journal() {
        let f = fixture(DEFAULT_RECOVERY_BYTES);
        let path = f.root.join("sample.txt");
        edit(
            &f.context,
            &path,
            FileState::Missing,
            FileState::Utf8("private body".into()),
        );
        f.context.store.finish_turn(&f.context).unwrap();
        let directory = f.context.store.directory(f.context.session_id);
        let journal_path = directory.join("journal.json");
        assert!(!std::fs::read_to_string(&journal_path)
            .unwrap()
            .contains("private body"));
        let digest = format!("{:x}", Sha256::digest(b"private body"));
        let blob = directory.join("blobs").join(digest);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(journal_path)
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
            assert_eq!(
                std::fs::metadata(&blob).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        std::fs::write(blob, "tampered body").unwrap();
        assert!(f
            .context
            .store
            .preview(&f.context, f.context.turn_id)
            .is_err());
    }

    #[cfg(unix)]
    #[test]
    fn root_replacement_and_symlink_parent_reject_capture_or_restore() {
        use std::os::unix::fs::symlink;
        let f = fixture(DEFAULT_RECOVERY_BYTES);
        let outside = tempfile::tempdir().unwrap();
        symlink(outside.path(), f.root.join("link")).unwrap();
        let prepared = PreparedFileMutation::single(
            f.root.join("link/file.txt"),
            "write_file",
            FileState::Missing,
            FileState::Utf8("body".into()),
        )
        .unwrap();
        assert!(f
            .context
            .store
            .prepare(&f.context, prepared.receipt())
            .is_err());
        edit(
            &f.context,
            &f.root.join("sample.txt"),
            FileState::Missing,
            FileState::Utf8("body".into()),
        );
        f.context.store.finish_turn(&f.context).unwrap();
        let moved = f.root.with_extension("moved");
        std::fs::rename(&f.root, &moved).unwrap();
        std::fs::create_dir(&f.root).unwrap();
        std::fs::write(f.root.join("sample.txt"), "body").unwrap();
        assert!(f
            .context
            .store
            .preview(&f.context, f.context.turn_id)
            .is_err());
        std::fs::remove_dir_all(moved).unwrap();
    }

    #[test]
    fn native_undo_and_redo_survive_reopening_with_stable_batch_identity() {
        let f = fixture(DEFAULT_RECOVERY_BYTES);
        let path = f.root.join("sample.txt");
        let outcome = f.context.apply(
            PreparedFileMutation::single(
                path.clone(),
                "write_file",
                FileState::Missing,
                FileState::Utf8("body".into()),
            )
            .unwrap(),
        );
        let receipt = outcome.result.unwrap();
        assert!(outcome.warning.is_none());
        f.context.store.finish_turn(&f.context).unwrap();
        let history = f.context.store.load_undo_stack(&f.context).unwrap();
        let operation = history.prepare(crate::undo::UndoDirection::Undo).unwrap();
        assert_eq!(operation.batch.persisted_id(), receipt.persisted_id());
        let undone = f
            .context
            .store
            .apply_history(&f.context, &operation)
            .unwrap();
        assert!(undone.warning.is_none());
        assert!(!path.exists());
        assert!(undone.history.is_empty());
        assert!(!undone.history.redo_is_empty());
        let reopened = f.context.store.load_undo_stack(&f.context).unwrap();
        let redo = reopened.prepare(crate::undo::UndoDirection::Redo).unwrap();
        assert_eq!(redo.batch.persisted_id(), receipt.persisted_id());
        f.context.store.apply_history(&f.context, &redo).unwrap();
        assert_eq!(std::fs::read_to_string(path).unwrap(), "body");
    }

    #[test]
    fn preview_shows_current_and_restored_text_and_bounds_large_unicode_output() {
        let path = PathBuf::from("/workspace/sample.txt");
        let mutation = FileMutationBatchReceipt::single(FileMutationReceipt::new(
            path.clone(),
            "edit_file",
            FileState::Utf8("before\n".into()),
            FileState::Utf8("after\n".into()),
        ));
        let (text, truncated) = recovery_changes_text(&mutation);
        assert!(text.contains("-after\n") && text.contains("+before\n"));
        assert!(text.contains("sha256") && !truncated);
        let mutation = FileMutationBatchReceipt::single(FileMutationReceipt::new(
            path,
            "edit_file",
            FileState::Missing,
            FileState::Utf8("é\n".repeat(40000)),
        ));
        let (text, truncated) = recovery_changes_text(&mutation);
        assert!(truncated && text.len() < 64 * 1024 + 128);
        assert!(text.contains("Preview truncated"));
    }

    #[test]
    fn create_then_delete_turn_restores_the_exact_net_zero_missing_state() {
        let f = fixture(DEFAULT_RECOVERY_BYTES);
        let path = f.root.join("net-zero.txt");
        edit(
            &f.context,
            &path,
            FileState::Missing,
            FileState::Utf8("temporary".into()),
        );
        edit(
            &f.context,
            &path,
            FileState::Utf8("temporary".into()),
            FileState::Missing,
        );
        f.context.store.finish_turn(&f.context).unwrap();
        let preview = f
            .context
            .store
            .preview(&f.context, f.context.turn_id)
            .unwrap();
        assert_eq!(preview.files.len(), 1);
        let restored = f
            .context
            .store
            .restore_with_outcome(&f.context, &preview)
            .unwrap();
        assert!(restored.warning.is_none());
        assert!(!path.exists());
        let redo = f
            .context
            .store
            .preview(&f.context, restored.rescue_turn)
            .unwrap();
        f.context
            .store
            .restore_with_outcome(&f.context, &redo)
            .unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn startup_seals_applied_turn_and_derives_receipt_after_crash_before_retention() {
        let f = fixture(DEFAULT_RECOVERY_BYTES);
        let path = f.root.join("sample.txt");
        let prepared = PreparedFileMutation::single(
            path.clone(),
            "write_file",
            FileState::Missing,
            FileState::Utf8("body".into()),
        )
        .unwrap();
        let id = prepared.receipt().persisted_id();
        let ticket = f
            .context
            .store
            .prepare(&f.context, prepared.receipt())
            .unwrap();
        prepared.apply().unwrap();
        f.context.store.complete(&f.context, &ticket).unwrap();
        let summary = f.context.store.reconcile(&f.context).unwrap().remove(0);
        assert!(summary.completed && summary.incomplete);
        assert!(
            f.context
                .store
                .preview(&f.context, f.context.turn_id)
                .unwrap()
                .incomplete
        );
        let history = f.context.store.load_undo_stack(&f.context).unwrap();
        let undo = history.prepare(crate::undo::UndoDirection::Undo).unwrap();
        assert_eq!(undo.batch.persisted_id(), id);
        let result = f.context.store.apply_history(&f.context, &undo).unwrap();
        assert!(result.history.is_empty());
        assert!(!path.exists());
    }

    #[test]
    fn interleaved_parent_and_child_turns_reconstruct_undo_by_monotonic_completion() {
        let f = fixture(DEFAULT_RECOVERY_BYTES);
        let first = f.root.join("old-parent.txt");
        let newer = f.root.join("new-parent.txt");
        let late_child = f.root.join("late-child.txt");
        capture_without_undo(&f.context, &first, "old".into());
        let mut next = f.context.clone();
        next.turn_id = Uuid::new_v4();
        capture_without_undo(&next, &newer, "new".into());
        let child = f
            .context
            .with_agent("late-child", f.context.workspace_roots.clone());
        capture_without_undo(&child, &late_child, "child".into());
        // All three completions reached the journal but automatic history
        // retention did not. The child's old group was inserted before `next`.
        let history = f.context.store.load_undo_stack(&next).unwrap();
        let undo = history.prepare(crate::undo::UndoDirection::Undo).unwrap();
        assert_eq!(undo.batch.receipts()[0].path, late_child);
        let prepared = undo.prepared_mutation().unwrap();
        let ticket = next
            .store
            .prepare_locked_with_history(
                &next,
                prepared.receipt(),
                Some(HistoryOperationPointer {
                    direction: undo.direction,
                    batch_id: undo.batch.persisted_id(),
                }),
            )
            .unwrap();
        prepared.apply().unwrap();
        next.store.complete(&next, &ticket).unwrap();
        // Crash again after inverse completion but before saving the stack.
        next.store.reconcile(&next).unwrap();
        for _ in 0..2 {
            let reconstructed = next.store.load_undo_stack(&next).unwrap();
            assert_eq!(
                reconstructed
                    .prepare(crate::undo::UndoDirection::Undo)
                    .unwrap()
                    .batch
                    .receipts()[0]
                    .path,
                newer
            );
            assert_eq!(
                reconstructed
                    .prepare(crate::undo::UndoDirection::Redo)
                    .unwrap()
                    .batch
                    .receipts()[0]
                    .path,
                late_child
            );
        }
        assert!(!late_child.exists());
        assert_eq!(std::fs::read_to_string(newer).unwrap(), "new");
    }

    #[test]
    fn legacy_undo_receipts_without_batch_ids_have_stable_reopen_identity() {
        let f = fixture(DEFAULT_RECOVERY_BYTES);
        let path = f.root.join("sample.txt");
        edit(
            &f.context,
            &path,
            FileState::Missing,
            FileState::Utf8("body".into()),
        );
        let mut journal = f.context.store.read_journal(f.context.session_id).unwrap();
        journal.undo.as_mut().unwrap().undo[0].id = None;
        for group in &mut journal.groups {
            for record in &mut group.records {
                record.batch_id = None;
            }
        }
        f.context.store.write_journal(&mut journal).unwrap();
        let mut legacy = serde_json::to_value(&journal).unwrap();
        for group in legacy["groups"].as_array_mut().unwrap() {
            for record in group["records"].as_array_mut().unwrap() {
                let object = record.as_object_mut().unwrap();
                object.remove("capture_revision");
                object.remove("applied_revision");
                object.remove("history_reset");
            }
        }
        atomic_write(
            &f.context
                .store
                .directory(f.context.session_id)
                .join("journal.json"),
            &serde_json::to_vec(&legacy).unwrap(),
        )
        .unwrap();
        let first = f
            .context
            .store
            .load_undo_stack(&f.context)
            .unwrap()
            .prepare(crate::undo::UndoDirection::Undo)
            .unwrap();
        let second = f
            .context
            .store
            .load_undo_stack(&f.context)
            .unwrap()
            .prepare(crate::undo::UndoDirection::Undo)
            .unwrap();
        assert!(first.matches(&second));
        f.context.store.apply_history(&f.context, &first).unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn interrupted_native_undo_reconstructs_transition_once_before_stack_commit() {
        let f = fixture(DEFAULT_RECOVERY_BYTES);
        let path = f.root.join("sample.txt");
        edit(
            &f.context,
            &path,
            FileState::Missing,
            FileState::Utf8("body".into()),
        );
        f.context.store.finish_turn(&f.context).unwrap();
        let operation = f
            .context
            .store
            .load_undo_stack(&f.context)
            .unwrap()
            .prepare(crate::undo::UndoDirection::Undo)
            .unwrap();
        let mut rescue = f.context.clone();
        rescue.turn_id = Uuid::new_v4();
        rescue.origin = "human_history".into();
        let prepared = operation.prepared_mutation().unwrap();
        let ticket = f
            .context
            .store
            .prepare_locked_with_history(
                &rescue,
                prepared.receipt(),
                Some(HistoryOperationPointer {
                    direction: operation.direction,
                    batch_id: operation.batch.persisted_id(),
                }),
            )
            .unwrap();
        prepared.apply().unwrap();
        f.context.store.complete(&rescue, &ticket).unwrap();
        f.context.store.reconcile(&f.context).unwrap();
        for _ in 0..2 {
            let history = f.context.store.load_undo_stack(&f.context).unwrap();
            assert!(history.is_empty());
            assert!(!history.redo_is_empty());
        }
        let redo = f
            .context
            .store
            .load_undo_stack(&f.context)
            .unwrap()
            .prepare(crate::undo::UndoDirection::Redo)
            .unwrap();
        f.context.store.apply_history(&f.context, &redo).unwrap();
        assert_eq!(std::fs::read_to_string(path).unwrap(), "body");
    }

    #[test]
    fn interrupted_turn_restore_resets_native_history_before_stack_reset_commit() {
        let f = fixture(DEFAULT_RECOVERY_BYTES);
        let path = f.root.join("sample.txt");
        edit(
            &f.context,
            &path,
            FileState::Missing,
            FileState::Utf8("body".into()),
        );
        f.context.store.finish_turn(&f.context).unwrap();
        let preview = f
            .context
            .store
            .preview(&f.context, f.context.turn_id)
            .unwrap();
        let mut rescue = f.context.clone();
        rescue.turn_id = Uuid::new_v4();
        rescue.origin = "human_recovery".into();
        let inverse = FileMutationBatchReceipt::new(
            preview
                .mutation
                .receipts()
                .iter()
                .map(|receipt| {
                    FileMutationReceipt::new(
                        receipt.path.clone(),
                        "recover_turn",
                        receipt.after.clone(),
                        receipt.before.clone(),
                    )
                })
                .collect(),
        )
        .unwrap();
        let ticket = f.context.store.prepare(&rescue, &inverse).unwrap();
        PreparedFileMutation::from_receipt(inverse)
            .unwrap()
            .apply()
            .unwrap();
        f.context.store.complete(&rescue, &ticket).unwrap();
        // Simulate death after bytes and completion, before reset or finish.
        f.context.store.reconcile(&rescue).unwrap();
        let first = f.context.store.load_undo_stack(&rescue).unwrap();
        assert!(first.is_empty() && first.redo_is_empty());
        f.context.store.save_undo_stack(&rescue, &first).unwrap();
        let reopened = f.context.store.load_undo_stack(&rescue).unwrap();
        assert!(reopened.is_empty() && reopened.redo_is_empty());
        let redo = f.context.store.preview(&rescue, rescue.turn_id).unwrap();
        f.context
            .store
            .restore_with_outcome(&rescue, &redo)
            .unwrap();
        assert_eq!(std::fs::read_to_string(path).unwrap(), "body");
    }

    #[test]
    fn stale_ui_stack_cannot_overwrite_automatic_durable_receipts() {
        let f = fixture(DEFAULT_RECOVERY_BYTES);
        edit(
            &f.context,
            &f.root.join("first.txt"),
            FileState::Missing,
            FileState::Utf8("first".into()),
        );
        let stale = f.context.store.load_undo_stack(&f.context).unwrap();
        edit(
            &f.context,
            &f.root.join("second.txt"),
            FileState::Missing,
            FileState::Utf8("second".into()),
        );
        assert!(f
            .context
            .store
            .save_undo_stack(&f.context, &stale)
            .unwrap_err()
            .to_string()
            .contains("changed"));
        assert_eq!(
            f.context
                .store
                .load_undo_stack(&f.context)
                .unwrap()
                .persisted_batches()
                .0
                .len(),
            2
        );
    }

    #[test]
    fn restoration_refuses_storage_failure_and_active_workspace_ownership() {
        let f = fixture(DEFAULT_RECOVERY_BYTES);
        let path = f.root.join("sample.txt");
        edit(
            &f.context,
            &path,
            FileState::Missing,
            FileState::Utf8("body".into()),
        );
        let operation = f
            .context
            .store
            .load_undo_stack(&f.context)
            .unwrap()
            .prepare(crate::undo::UndoDirection::Undo)
            .unwrap();
        let activity = f
            .context
            .store
            .try_workspace_activity(&f.context.workspace_roots)
            .unwrap();
        assert_eq!(
            f.context
                .store
                .apply_history(&f.context, &operation)
                .unwrap_err()
                .kind(),
            io::ErrorKind::WouldBlock
        );
        drop(activity);
        let store = Arc::new(RecoveryStore::with_quota(&f.context.store.scope, 0));
        let mut limited = f.context.clone();
        limited.store = store;
        assert!(limited.store.apply_history(&limited, &operation).is_err());
        assert_eq!(std::fs::read_to_string(path).unwrap(), "body");
    }

    #[test]
    fn workspace_activity_shares_profiles_and_handles_overlapping_roots_without_blocking_siblings()
    {
        let f = fixture(DEFAULT_RECOVERY_BYTES);
        let child_a = f.root.join("a");
        let child_b = f.root.join("b");
        std::fs::create_dir(&child_a).unwrap();
        std::fs::create_dir(&child_b).unwrap();
        drop(
            f.context
                .store
                .try_workspace_activity(std::slice::from_ref(&f.root))
                .unwrap(),
        );
        let named = RecoveryStore::new(f.context.store.scope.join("profiles/work"));
        let active = named
            .try_workspace_activity(std::slice::from_ref(&child_b))
            .unwrap();
        assert!(f
            .context
            .store
            .try_workspace_restore(std::slice::from_ref(&f.root))
            .is_err());
        let restore_a = f
            .context
            .store
            .try_workspace_restore(std::slice::from_ref(&child_a))
            .unwrap();
        assert!(named
            .try_workspace_activity(std::slice::from_ref(&child_a))
            .is_err());
        let sibling = named
            .try_workspace_activity(std::slice::from_ref(&child_b))
            .unwrap();
        drop(sibling);
        drop(restore_a);
        drop(active);
        assert!(f
            .context
            .store
            .try_workspace_restore(std::slice::from_ref(&f.root))
            .is_ok());
    }

    #[test]
    #[cfg(unix)]
    fn workspace_registry_compacts_idle_metadata_and_preserves_stable_claims() {
        use std::os::unix::fs::{symlink, MetadataExt};

        let temporary = tempfile::tempdir().unwrap();
        let base = temporary.path().canonicalize().unwrap();
        let scope = base.join("registry");
        private_dir(&scope).unwrap();
        let root = |name: &str| {
            let path = base.join(name);
            std::fs::create_dir(&path).unwrap();
            path
        };
        let descriptor = |path: &Path| {
            let digest = format!("{:x}", Sha256::digest(path.as_os_str().as_encoded_bytes()));
            scope.join(format!("{digest}.json"))
        };
        let lock_paths = |path: &Path| {
            let digest = format!("{:x}", Sha256::digest(path.as_os_str().as_encoded_bytes()));
            [
                scope.join(format!("{digest}.active.lock")),
                scope.join(format!("{digest}.restore.lock")),
            ]
        };
        let active = root("active");
        let idle = root("idle");
        let missing_claim = root("unknown");
        let corrupt_claim = root("corrupt");
        let unrelated = root("unrelated");
        let active_claim =
            workspace_lease_in_scope(scope.clone(), std::slice::from_ref(&active), false).unwrap();
        drop(workspace_lease_in_scope(scope.clone(), std::slice::from_ref(&idle), false).unwrap());
        let stable = lock_paths(&idle).map(|path| {
            let metadata = std::fs::symlink_metadata(&path).unwrap();
            (path, metadata.dev(), metadata.ino())
        });
        for path in [&missing_claim, &corrupt_claim] {
            atomic_write(&descriptor(path), &serde_json::to_vec(path).unwrap()).unwrap();
        }
        drop(workspace_lock(&lock_paths(&missing_claim)[0], false).unwrap());
        drop(workspace_lock(&lock_paths(&corrupt_claim)[0], false).unwrap());
        let outside = base.join("unrelated-private-file");
        std::fs::write(&outside, b"preserved").unwrap();
        symlink(&outside, &lock_paths(&corrupt_claim)[1]).unwrap();

        let unrelated_claim = workspace_lease_in_scope(scope.clone(), &[unrelated], false).unwrap();
        assert!(!descriptor(&idle).exists());
        assert!(descriptor(&active).exists());
        assert!(descriptor(&missing_claim).exists());
        assert!(descriptor(&corrupt_claim).exists());
        for (path, device, inode) in &stable {
            let metadata = std::fs::symlink_metadata(path).unwrap();
            assert_eq!((metadata.dev(), metadata.ino()), (*device, *inode));
        }
        assert_eq!(std::fs::read(outside).unwrap(), b"preserved");
        assert_eq!(
            workspace_lease_in_scope(scope.clone(), &[active], true)
                .unwrap_err()
                .kind(),
            io::ErrorKind::WouldBlock
        );
        let reopened =
            workspace_lease_in_scope(scope.clone(), std::slice::from_ref(&idle), false).unwrap();
        assert!(descriptor(&idle).exists());
        for (path, device, inode) in &stable {
            let metadata = std::fs::symlink_metadata(path).unwrap();
            assert_eq!((metadata.dev(), metadata.ino()), (*device, *inode));
        }
        drop((reopened, unrelated_claim, active_claim));
    }

    #[test]
    #[cfg(unix)]
    fn child_workspace_registry_probe() {
        use std::os::fd::FromRawFd;
        let Some(scope) = std::env::var_os("CA_WORKSPACE_REGISTRY_PROBE_SCOPE") else {
            return;
        };
        let scope = PathBuf::from(scope);
        let root = PathBuf::from(std::env::var_os("CA_WORKSPACE_REGISTRY_PROBE_ROOT").unwrap());
        let ready = PathBuf::from(std::env::var_os("CA_WORKSPACE_REGISTRY_PROBE_READY").unwrap());
        let claims = std::env::var("CA_WORKSPACE_REGISTRY_PROBE_FDS")
            .ok()
            .map(|fds| {
                fds.split(',')
                    .map(|fd| unsafe { File::from_raw_fd(fd.parse::<i32>().unwrap()) })
                    .collect::<Vec<_>>()
            });
        std::fs::write(ready, b"ready").unwrap();
        let mut input = [0_u8; 1];
        std::io::stdin().read_exact(&mut input).unwrap();
        if claims.is_none() {
            for _ in 0..8 {
                drop(
                    workspace_lease_in_scope(scope.clone(), std::slice::from_ref(&root), false)
                        .unwrap(),
                );
            }
        }
        drop(claims);
    }

    #[test]
    #[cfg(unix)]
    fn workspace_registry_retains_inherited_claims_after_parent_release() {
        use std::os::fd::AsRawFd;
        use std::os::unix::process::CommandExt;
        use std::process::Stdio;

        let temporary = tempfile::tempdir().unwrap();
        let base = temporary.path().canonicalize().unwrap();
        let scope = base.join("registry");
        private_dir(&scope).unwrap();
        let root = base.join("workspace");
        let other = base.join("other");
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(&other).unwrap();
        let ready = base.join("child-ready");
        let claim =
            workspace_lease_in_scope(scope.clone(), std::slice::from_ref(&root), false).unwrap();
        let fds = claim
            ._files
            .iter()
            .map(AsRawFd::as_raw_fd)
            .collect::<Vec<_>>();
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "recovery::tests::child_workspace_registry_probe",
                "--nocapture",
            ])
            .env("CA_WORKSPACE_REGISTRY_PROBE_SCOPE", &scope)
            .env("CA_WORKSPACE_REGISTRY_PROBE_ROOT", &root)
            .env("CA_WORKSPACE_REGISTRY_PROBE_READY", &ready)
            .env(
                "CA_WORKSPACE_REGISTRY_PROBE_FDS",
                fds.iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(","),
            )
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        unsafe {
            command.pre_exec(move || {
                for fd in &fds {
                    let flags = libc::fcntl(*fd, libc::F_GETFD);
                    if flags < 0 || libc::fcntl(*fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) != 0
                    {
                        return Err(io::Error::last_os_error());
                    }
                }
                Ok(())
            });
        }
        let mut child = command.spawn().unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !ready.exists() {
            assert!(
                std::time::Instant::now() < deadline,
                "inherited claim worker did not start"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        drop(claim);
        drop(workspace_lease_in_scope(scope.clone(), std::slice::from_ref(&other), false).unwrap());
        let digest = format!("{:x}", Sha256::digest(root.as_os_str().as_encoded_bytes()));
        assert!(scope.join(format!("{digest}.json")).exists());
        assert_eq!(
            workspace_lease_in_scope(scope.clone(), std::slice::from_ref(&root), true)
                .unwrap_err()
                .kind(),
            io::ErrorKind::WouldBlock
        );
        child.stdin.take().unwrap().write_all(b"x").unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        drop(workspace_lease_in_scope(scope.clone(), &[other], false).unwrap());
        assert!(!scope.join(format!("{digest}.json")).exists());
        drop(workspace_lease_in_scope(scope, &[root], true).unwrap());
    }

    #[test]
    #[cfg(unix)]
    fn workspace_registry_repeated_concurrent_admissions_compact_history() {
        use std::os::unix::fs::MetadataExt;
        use std::process::Stdio;

        let temporary = tempfile::tempdir().unwrap();
        let base = temporary.path().canonicalize().unwrap();
        let scope = base.join("registry");
        private_dir(&scope).unwrap();
        let roots = (0..64)
            .map(|index| {
                let path = base.join(format!("old-{index}"));
                std::fs::create_dir(&path).unwrap();
                path
            })
            .collect::<Vec<_>>();
        drop(workspace_lease_in_scope(scope.clone(), &roots, false).unwrap());
        let stable = std::fs::read_dir(&scope)
            .unwrap()
            .filter_map(|entry| {
                let path = entry.unwrap().path();
                if path.extension().and_then(|extension| extension.to_str()) != Some("lock") {
                    return None;
                }
                let metadata = std::fs::symlink_metadata(&path).unwrap();
                Some((path, metadata.dev(), metadata.ino()))
            })
            .collect::<Vec<_>>();
        for root in roots.iter().take(32) {
            std::fs::remove_dir(root).unwrap();
        }
        let mut workers = Vec::new();
        for index in 0..6 {
            let root = base.join(format!("new-{index}"));
            std::fs::create_dir(&root).unwrap();
            let ready = base.join(format!("ready-{index}"));
            let child = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "recovery::tests::child_workspace_registry_probe",
                    "--nocapture",
                ])
                .env("CA_WORKSPACE_REGISTRY_PROBE_SCOPE", &scope)
                .env("CA_WORKSPACE_REGISTRY_PROBE_ROOT", &root)
                .env("CA_WORKSPACE_REGISTRY_PROBE_READY", &ready)
                .env_remove("CA_WORKSPACE_REGISTRY_PROBE_FDS")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            workers.push((child, ready));
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while workers.iter().any(|(_, ready)| !ready.exists()) {
            assert!(
                std::time::Instant::now() < deadline,
                "registry admission workers did not start"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        for (child, _) in &mut workers {
            child.stdin.take().unwrap().write_all(b"x").unwrap();
        }
        for (child, _) in workers {
            let output = child.wait_with_output().unwrap();
            assert!(
                output.status.success(),
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        for (path, device, inode) in stable {
            let metadata = std::fs::symlink_metadata(path).unwrap();
            assert_eq!((metadata.dev(), metadata.ino()), (device, inode));
        }
        let descriptors = std::fs::read_dir(&scope)
            .unwrap()
            .filter(|entry| {
                entry
                    .as_ref()
                    .unwrap()
                    .path()
                    .extension()
                    .and_then(|extension| extension.to_str())
                    == Some("json")
            })
            .count();
        assert!(
            descriptors <= 6,
            "idle history remained registered: {descriptors}"
        );
    }

    #[test]
    fn child_workspace_probe() {
        let Some(profile) = std::env::var_os("CA_WORKSPACE_PROBE_PROFILE") else {
            return;
        };
        let root = PathBuf::from(std::env::var_os("CA_WORKSPACE_PROBE_ROOT").unwrap());
        let store = RecoveryStore::new(PathBuf::from(profile));
        let error = if std::env::var("CA_WORKSPACE_PROBE_MODE").unwrap() == "activity" {
            store.try_workspace_activity(&[root]).err().unwrap()
        } else {
            store.try_workspace_restore(&[root]).err().unwrap()
        };
        assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
    }

    #[test]
    fn workspace_shared_and_exclusive_ownership_is_cross_process() {
        let f = fixture(DEFAULT_RECOVERY_BYTES);
        let probe = |mode: &str| {
            let result = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "recovery::tests::child_workspace_probe",
                    "--nocapture",
                ])
                .env("CA_WORKSPACE_PROBE_PROFILE", &f.context.store.scope)
                .env("CA_WORKSPACE_PROBE_ROOT", &f.root)
                .env("CA_WORKSPACE_PROBE_MODE", mode)
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stdout)
            );
        };
        let activity = f
            .context
            .store
            .try_workspace_activity(&f.context.workspace_roots)
            .unwrap();
        probe("restore");
        drop(activity);
        let restore = f
            .context
            .store
            .try_workspace_restore(&f.context.workspace_roots)
            .unwrap();
        probe("activity");
        drop(restore);
    }
}
