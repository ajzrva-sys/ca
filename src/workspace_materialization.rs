//! Verified disposable workspaces for hermetic experiments and isolated jobs.
//!
//! Copy-on-write cloning is only a materialization optimization. Correctness
//! comes from comparing a bounded source manifest before and after the copy and
//! an independently generated destination manifest before publication.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::{Condvar, Mutex, OnceLock};
use uuid::Uuid;

#[cfg(unix)]
use std::os::fd::{AsRawFd, FromRawFd, RawFd};
#[cfg(unix)]
use std::os::unix::ffi::{OsStrExt, OsStringExt};

pub const DEFAULT_MAX_ENTRIES: usize = 100_000;
pub const DEFAULT_MAX_LOGICAL_BYTES: u64 = 2 * 1024 * 1024 * 1024;
pub const DEFAULT_MIN_FREE_BYTES: u64 = 5 * 1024 * 1024 * 1024;
pub const DEFAULT_MAX_DEPTH: usize = 128;
pub const MAX_CONCURRENT_MATERIALIZATIONS: usize = 2;

const COPY_BUFFER_BYTES: usize = 64 * 1024;
const MAX_LABEL_BYTES: usize = 48;
const MAX_RECOVERY_ARTIFACT_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_DURABLE_RECEIPT_BYTES: u64 = 64 * 1024;
const RECEIPT_SCHEMA: u32 = 1;
const RECEIPT_DIRECTORY: &str = ".workspace-receipts";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SymlinkPolicy {
    RejectAll,
    SafeRelative,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ClonePreference {
    PreferClone,
    CopyOnly,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduledTrackedAllowlist {
    entries: BTreeSet<PathBuf>,
}

impl ScheduledTrackedAllowlist {
    pub fn new(entries: impl IntoIterator<Item = PathBuf>) -> Result<Self, MaterializationError> {
        let mut normalized = BTreeSet::new();
        for entry in entries {
            normalized.insert(normalize_relative_entry(&entry)?);
        }
        if normalized.is_empty() {
            return Err(MaterializationError::InvalidAllowlistEntry);
        }
        Ok(Self {
            entries: normalized,
        })
    }

    fn contains_or_contains_descendant(&self, relative: &Path) -> bool {
        self.entries
            .iter()
            .any(|entry| entry == relative || entry.starts_with(relative))
    }

    fn contains_exact(&self, relative: &Path) -> bool {
        self.entries.contains(relative)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaterializationPolicy {
    pub symlinks: SymlinkPolicy,
    pub max_entries: usize,
    pub max_logical_bytes: u64,
    pub min_free_bytes: u64,
    pub max_depth: usize,
}

impl MaterializationPolicy {
    pub fn benchmark() -> Self {
        Self {
            symlinks: SymlinkPolicy::RejectAll,
            ..Self::default()
        }
    }

    pub fn scheduled() -> Self {
        Self {
            symlinks: SymlinkPolicy::SafeRelative,
            ..Self::default()
        }
    }

    fn validate(&self) -> Result<(), MaterializationError> {
        if self.max_entries == 0
            || self.max_entries > DEFAULT_MAX_ENTRIES
            || self.max_logical_bytes == 0
            || self.max_logical_bytes > DEFAULT_MAX_LOGICAL_BYTES
            || self.max_depth == 0
            || self.max_depth > DEFAULT_MAX_DEPTH
        {
            return Err(MaterializationError::InvalidPolicy);
        }
        Ok(())
    }
}

impl Default for MaterializationPolicy {
    fn default() -> Self {
        Self {
            symlinks: SymlinkPolicy::RejectAll,
            max_entries: DEFAULT_MAX_ENTRIES,
            max_logical_bytes: DEFAULT_MAX_LOGICAL_BYTES,
            min_free_bytes: DEFAULT_MIN_FREE_BYTES,
            max_depth: DEFAULT_MAX_DEPTH,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MaterializationMode {
    AllCloned,
    Mixed,
    Copied,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceMaterialization {
    pub source_manifest_sha256: String,
    pub destination_manifest_sha256: String,
    pub entries: usize,
    pub regular_files: usize,
    pub logical_bytes: u64,
    pub mode: MaterializationMode,
}

/// Backward-compatible name used by the initial WSP-096 callers.
pub type MaterializationReceipt = WorkspaceMaterialization;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct DurablePolicy {
    symlinks: SymlinkPolicy,
    max_entries: u64,
    max_logical_bytes: u64,
    max_depth: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct DurableReceiptBody {
    schema: u32,
    workspace_id: Uuid,
    workspace_name: String,
    root_device: u64,
    root_inode: u64,
    workspace_device: u64,
    workspace_inode: u64,
    materialization: WorkspaceMaterialization,
    policy: DurablePolicy,
    #[serde(default)]
    cleanup_authorization: Option<DurableCleanupAuthorization>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct DurableCleanupAuthorization {
    artifact_sha256: String,
    artifact_size: u64,
    final_manifest_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct DurableWorkspaceReceipt {
    body: DurableReceiptBody,
    body_sha256: String,
}

/// A published, deliberately non-Git workspace.
///
/// Dropping this value does not delete the workspace. The caller must first
/// verify a concrete recovery artifact and then supply the returned private
/// capability to [`DisposableWorkspace::cleanup_after_export`].
pub struct DisposableWorkspace {
    path: PathBuf,
    private_root: PathBuf,
    workspace_id: Uuid,
    #[cfg(unix)]
    workspace_device: u64,
    #[cfg(unix)]
    workspace_inode: u64,
    #[cfg(unix)]
    private_root_dir: File,
    durable_receipt: DurableReceiptBody,
    location: WorkspaceLocation,
    receipt: MaterializationReceipt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkspaceLocation {
    Published,
    Deleting,
}

impl std::fmt::Debug for DisposableWorkspace {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DisposableWorkspace")
            .field("path", &"[PRIVATE]")
            .field("receipt", &self.receipt)
            .finish()
    }
}

impl DisposableWorkspace {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn receipt(&self) -> &MaterializationReceipt {
        &self.receipt
    }

    pub fn workspace_id(&self) -> Uuid {
        self.workspace_id
    }

    /// Opens and hashes one concrete, owner-only recovery artifact and binds it
    /// to both this workspace's published baseline and its current final tree.
    /// The caller remains responsible for proving that the artifact's format is
    /// semantically complete (for example, that a patch reconstructs the final
    /// manifest); this boundary proves identity, stability, and exact bytes.
    pub fn verify_recovery_artifact(
        &self,
        recovery_artifact: &Path,
    ) -> Result<VerifiedRecoveryArtifact, MaterializationError> {
        #[cfg(not(unix))]
        {
            let _ = recovery_artifact;
            Err(MaterializationError::UnsupportedPlatform)
        }
        #[cfg(unix)]
        {
            let workspace = self.open_current_workspace()?;
            let mut artifact_file = open_recovery_artifact_outside(
                recovery_artifact,
                directory_identity(&self.private_root_dir)?,
                directory_identity(&workspace)?,
            )?;
            let (artifact_sha256, artifact_size) =
                hash_bounded_open_file(&mut artifact_file, MAX_RECOVERY_ARTIFACT_BYTES)?;
            if artifact_size == 0 {
                return Err(MaterializationError::InvalidRecoveryArtifact);
            }
            if let Some(authorization) = &self.durable_receipt.cleanup_authorization {
                if authorization.artifact_sha256 != hex_digest(&artifact_sha256)
                    || authorization.artifact_size != artifact_size
                {
                    return Err(MaterializationError::InvalidRecoveryArtifact);
                }
                return Ok(VerifiedRecoveryArtifact {
                    workspace_id: self.workspace_id,
                    baseline_manifest_sha256: self.receipt.destination_manifest_sha256.clone(),
                    final_manifest: None,
                    final_manifest_sha256: authorization.final_manifest_sha256.clone(),
                    artifact_sha256,
                    artifact_size,
                    artifact_file,
                });
            }
            let final_manifest = self.scan_current_workspace()?.manifest;
            let final_manifest_sha256 = final_manifest.sha256.clone();
            Ok(VerifiedRecoveryArtifact {
                workspace_id: self.workspace_id,
                baseline_manifest_sha256: self.receipt.destination_manifest_sha256.clone(),
                final_manifest: Some(final_manifest),
                final_manifest_sha256,
                artifact_sha256,
                artifact_size,
                artifact_file,
            })
        }
    }

    pub fn reopen(private_root: &Path, workspace_id: Uuid) -> Result<Self, MaterializationError> {
        reopen_workspace(private_root, workspace_id)
    }

    pub fn cleanup_after_export(
        mut self,
        mut recovery: VerifiedRecoveryArtifact,
    ) -> Result<(), Box<CleanupFailure>> {
        match self.cleanup_inner(&mut recovery) {
            Ok(()) => Ok(()),
            Err(error) => Err(Box::new(CleanupFailure {
                error,
                workspace: self,
                recovery,
            })),
        }
    }

    fn policy_for_rescan(&self) -> Result<MaterializationPolicy, MaterializationError> {
        let policy = &self.durable_receipt.policy;
        let policy = MaterializationPolicy {
            symlinks: policy.symlinks,
            max_entries: policy
                .max_entries
                .try_into()
                .map_err(|_| MaterializationError::InvalidDurableReceipt)?,
            max_logical_bytes: policy.max_logical_bytes,
            min_free_bytes: 0,
            max_depth: policy
                .max_depth
                .try_into()
                .map_err(|_| MaterializationError::InvalidDurableReceipt)?,
        };
        policy
            .validate()
            .map_err(|_| MaterializationError::InvalidDurableReceipt)?;
        Ok(policy)
    }

    #[cfg(unix)]
    fn workspace_name(&self) -> OsString {
        match self.location {
            WorkspaceLocation::Published => OsString::from(&self.durable_receipt.workspace_name),
            WorkspaceLocation::Deleting => {
                OsString::from(format!(".deleting-{}", self.workspace_id))
            }
        }
    }

    #[cfg(unix)]
    fn open_current_workspace(&self) -> Result<File, MaterializationError> {
        let workspace = open_directory_at(&self.private_root_dir, &self.workspace_name())
            .map_err(|_| MaterializationError::UnsafeCleanup)?;
        if !identity_matches(
            &workspace,
            DirectoryIdentity {
                device: self.workspace_device,
                inode: self.workspace_inode,
            },
        )? {
            return Err(MaterializationError::UnsafeCleanup);
        }
        Ok(workspace)
    }

    fn scan_current_workspace(&self) -> Result<ScannedTree, MaterializationError> {
        let policy = self.policy_for_rescan()?;
        #[cfg(unix)]
        {
            scan_tree_from_open_dir(
                self.open_current_workspace()?,
                &policy,
                ScanPurpose::Destination,
            )
        }
        #[cfg(not(unix))]
        scan_tree(&self.path, &policy, ScanPurpose::Destination)
    }

    fn cleanup_inner(
        &mut self,
        recovery: &mut VerifiedRecoveryArtifact,
    ) -> Result<(), MaterializationError> {
        if recovery.workspace_id != self.workspace_id
            || recovery.baseline_manifest_sha256 != self.receipt.destination_manifest_sha256
        {
            return Err(MaterializationError::InvalidRecoveryArtifact);
        }
        let (artifact_sha256, artifact_size) =
            hash_bounded_open_file(&mut recovery.artifact_file, MAX_RECOVERY_ARTIFACT_BYTES)?;
        if artifact_sha256 != recovery.artifact_sha256 || artifact_size != recovery.artifact_size {
            return Err(MaterializationError::InvalidRecoveryArtifact);
        }

        #[cfg(unix)]
        {
            if self.location == WorkspaceLocation::Deleting
                && !entry_exists_at(&self.private_root_dir, &self.workspace_name())?
            {
                return remove_durable_receipt(&self.private_root_dir, self.workspace_id);
            }

            let artifact_sha256_hex = hex_digest(&recovery.artifact_sha256);
            if let Some(authorization) = &self.durable_receipt.cleanup_authorization {
                if authorization.artifact_sha256 != artifact_sha256_hex
                    || authorization.artifact_size != recovery.artifact_size
                    || authorization.final_manifest_sha256 != recovery.final_manifest_sha256
                {
                    return Err(MaterializationError::InvalidRecoveryArtifact);
                }
            } else {
                let final_manifest = recovery
                    .final_manifest
                    .as_ref()
                    .ok_or(MaterializationError::InvalidRecoveryArtifact)?;
                if final_manifest.sha256 != recovery.final_manifest_sha256 {
                    return Err(MaterializationError::InvalidRecoveryArtifact);
                }
                let workspace = self.open_current_workspace()?;
                let current = scan_tree_from_open_dir(
                    workspace,
                    &self.policy_for_rescan()?,
                    ScanPurpose::Destination,
                )?;
                if current.manifest != *final_manifest {
                    return Err(MaterializationError::DestinationMismatch);
                }
                self.durable_receipt.cleanup_authorization = Some(DurableCleanupAuthorization {
                    artifact_sha256: artifact_sha256_hex,
                    artifact_size: recovery.artifact_size,
                    final_manifest_sha256: recovery.final_manifest_sha256.clone(),
                });
                replace_durable_receipt(&self.private_root_dir, &self.durable_receipt)?;
            }

            let deleting_name = OsString::from(format!(".deleting-{}", self.workspace_id));
            if self.location == WorkspaceLocation::Published {
                if entry_exists_at(&self.private_root_dir, &deleting_name)? {
                    return Err(MaterializationError::UnsafeCleanup);
                }
                let published_name = OsString::from(&self.durable_receipt.workspace_name);
                rename_entry_at(&self.private_root_dir, &published_name, &deleting_name)?;
                self.location = WorkspaceLocation::Deleting;
                self.path = self.private_root.join(&deleting_name);
                fsync_directory(&self.private_root_dir)?;
            }

            remove_owned_tree_at(
                &self.private_root_dir,
                &deleting_name,
                DirectoryIdentity {
                    device: self.workspace_device,
                    inode: self.workspace_inode,
                },
            )?;
            remove_durable_receipt(&self.private_root_dir, self.workspace_id)?;
            fsync_directory(&self.private_root_dir)?;
            Ok(())
        }

        #[cfg(not(unix))]
        {
            let current = self.scan_current_workspace()?;
            if current.manifest != recovery.final_manifest {
                return Err(MaterializationError::DestinationMismatch);
            }
            make_owned_tree_writable(&self.path)?;
            fs::remove_dir_all(&self.path).map_err(MaterializationError::Io)
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MaterializationError {
    #[error("workspace materialization policy is invalid")]
    InvalidPolicy,
    #[error("workspace label must be 1-48 lowercase letters, digits, '-' or '_'")]
    InvalidLabel,
    #[error("source must be a real directory and may not be a symbolic link")]
    InvalidSource,
    #[error("private workspace root must be owner-only and may not be a symbolic link")]
    InvalidPrivateRoot,
    #[error("source and private workspace root may not overlap")]
    OverlappingRoots,
    #[error("source tree contains too many entries")]
    TooManyEntries,
    #[error("source tree exceeds the logical-byte limit")]
    TooManyBytes,
    #[error("source tree exceeds the depth limit")]
    TooDeep,
    #[error("source tree has no regular files")]
    NoRegularFiles,
    #[error("source tree contains an unsupported special file")]
    SpecialFile,
    #[error("source tree contains a hard-linked regular file")]
    HardLinkRejected,
    #[error("symbolic links are not allowed for this workspace")]
    SymlinkRejected,
    #[error("symbolic link is absolute or escapes the workspace")]
    UnsafeSymlink,
    #[error("symbolic link target is absent, excluded, or cyclic")]
    InvalidSymlinkTarget,
    #[error("scheduled tracked allowlist contains a non-normal relative path")]
    InvalidAllowlistEntry,
    #[error("scheduled tracked allowlist names an absent or unlisted path")]
    AllowlistMismatch,
    #[error("workspace materialization is currently unsupported on this platform")]
    UnsupportedPlatform,
    #[error("source changed while the workspace was being materialized")]
    SourceChanged,
    #[error("destination did not match the verified source manifest")]
    DestinationMismatch,
    #[error("destination unexpectedly contains excluded runtime or Git state")]
    UnexpectedExcludedEntry,
    #[error("insufficient free space for a safe copy fallback")]
    InsufficientSpace,
    #[error("workspace cleanup requires a complete owner-only regular recovery artifact")]
    InvalidRecoveryArtifact,
    #[error(
        "durable workspace receipt is missing, corrupt, or does not match the owned directory"
    )]
    InvalidDurableReceipt,
    #[error("workspace cleanup target is not the exact owned disposable directory")]
    UnsafeCleanup,
    #[error("copy-on-write clone failed: {0}")]
    Clone(io::Error),
    #[error("workspace I/O failed: {0}")]
    Io(io::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EntryKind {
    Directory,
    Regular,
    Symlink,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ManifestEntry {
    relative: PathBuf,
    kind: EntryKind,
    mode: u32,
    size: u64,
    content_sha256: [u8; 32],
    symlink_target: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TreeManifest {
    entries: Vec<ManifestEntry>,
    logical_bytes: u64,
    regular_files: usize,
    sha256: String,
}

/// Non-serializable authority proving that a concrete recovery artifact was
/// opened, hashed, and bound to one workspace's baseline and final manifests.
/// Its fields and constructor deliberately remain private.
pub struct VerifiedRecoveryArtifact {
    workspace_id: Uuid,
    baseline_manifest_sha256: String,
    final_manifest: Option<TreeManifest>,
    final_manifest_sha256: String,
    artifact_sha256: [u8; 32],
    artifact_size: u64,
    artifact_file: File,
}

impl std::fmt::Debug for VerifiedRecoveryArtifact {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VerifiedRecoveryArtifact")
            .field("workspace_id", &self.workspace_id)
            .field("baseline_manifest_sha256", &self.baseline_manifest_sha256)
            .field("final_manifest_sha256", &self.final_manifest_sha256)
            .field("artifact_size", &self.artifact_size)
            .finish_non_exhaustive()
    }
}

impl VerifiedRecoveryArtifact {
    pub fn final_manifest_sha256(&self) -> &str {
        &self.final_manifest_sha256
    }

    pub fn artifact_sha256(&self) -> String {
        hex_digest(&self.artifact_sha256)
    }

    pub fn artifact_size(&self) -> u64 {
        self.artifact_size
    }
}

/// A cleanup failure retains both authorities required for an exact retry.
pub struct CleanupFailure {
    error: MaterializationError,
    workspace: DisposableWorkspace,
    recovery: VerifiedRecoveryArtifact,
}

impl std::fmt::Debug for CleanupFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CleanupFailure")
            .field("error", &self.error)
            .field("workspace", &self.workspace)
            .field("recovery", &self.recovery)
            .finish()
    }
}

impl std::fmt::Display for CleanupFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.error.fmt(formatter)
    }
}

impl std::error::Error for CleanupFailure {}

impl CleanupFailure {
    pub fn error(&self) -> &MaterializationError {
        &self.error
    }

    pub fn retry(self) -> Result<(), Box<Self>> {
        self.workspace.cleanup_after_export(self.recovery)
    }

    pub fn into_parts(
        self,
    ) -> (
        DisposableWorkspace,
        VerifiedRecoveryArtifact,
        MaterializationError,
    ) {
        (self.workspace, self.recovery, self.error)
    }
}

#[derive(Debug, Clone, Copy)]
struct FileIdentity {
    size: u64,
    mode: u32,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
    #[cfg(unix)]
    links: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DirectoryIdentity {
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
}

struct ScannedTree {
    manifest: TreeManifest,
    identities: BTreeMap<PathBuf, FileIdentity>,
    #[cfg(unix)]
    root_dir: File,
}

struct ObservedEntryBudget {
    observed: usize,
    maximum: usize,
}

impl ObservedEntryBudget {
    fn new(maximum: usize) -> Self {
        Self {
            observed: 0,
            maximum,
        }
    }

    fn observe(&mut self) -> Result<(), MaterializationError> {
        self.observed = self
            .observed
            .checked_add(1)
            .ok_or(MaterializationError::TooManyEntries)?;
        if self.observed > self.maximum {
            Err(MaterializationError::TooManyEntries)
        } else {
            Ok(())
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScanPurpose {
    Source,
    Destination,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileMaterialization {
    Cloned,
    Copied,
}

#[cfg(unix)]
struct StagingGuard {
    root_dir: File,
    name: Option<OsString>,
    identity: DirectoryIdentity,
}

#[cfg(unix)]
impl StagingGuard {
    fn new(
        root_dir: &File,
        name: OsString,
        identity: DirectoryIdentity,
    ) -> Result<Self, MaterializationError> {
        Ok(Self {
            root_dir: root_dir.try_clone().map_err(MaterializationError::Io)?,
            name: Some(name),
            identity,
        })
    }

    fn disarm(&mut self) {
        self.name = None;
    }
}

#[cfg(unix)]
impl Drop for StagingGuard {
    fn drop(&mut self) {
        if let Some(name) = self.name.take() {
            let _ = remove_owned_tree_at(&self.root_dir, &name, self.identity);
        }
    }
}

#[cfg(not(unix))]
struct StagingGuard {
    path: Option<PathBuf>,
}

#[cfg(not(unix))]
impl StagingGuard {
    fn new(path: PathBuf) -> Self {
        Self { path: Some(path) }
    }

    fn disarm(&mut self) {
        self.path = None;
    }
}

#[cfg(not(unix))]
impl Drop for StagingGuard {
    fn drop(&mut self) {
        if let Some(path) = self.path.take() {
            let _ = make_owned_tree_writable(&path);
            let _ = fs::remove_dir_all(path);
        }
    }
}

struct ConcurrencyGate {
    active: Mutex<usize>,
    changed: Condvar,
}

struct ConcurrencyPermit(&'static ConcurrencyGate);

impl Drop for ConcurrencyPermit {
    fn drop(&mut self) {
        if let Ok(mut active) = self.0.active.lock() {
            *active = active.saturating_sub(1);
            self.0.changed.notify_one();
        }
    }
}

#[cfg(unix)]
struct ByteReservation {
    filesystem: u64,
    bytes: u64,
}

#[cfg(unix)]
impl Drop for ByteReservation {
    fn drop(&mut self) {
        if let Ok(mut reservations) = byte_reservations().lock() {
            let remove = if let Some(reserved) = reservations.get_mut(&self.filesystem) {
                *reserved = reserved.saturating_sub(self.bytes);
                *reserved == 0
            } else {
                false
            };
            if remove {
                reservations.remove(&self.filesystem);
            }
        }
    }
}

#[cfg(unix)]
fn byte_reservations() -> &'static Mutex<BTreeMap<u64, u64>> {
    static RESERVATIONS: OnceLock<Mutex<BTreeMap<u64, u64>>> = OnceLock::new();
    RESERVATIONS.get_or_init(|| Mutex::new(BTreeMap::new()))
}

#[cfg(unix)]
fn acquire_byte_reservation(
    private_root: &File,
    required: u64,
    available: u64,
) -> Result<ByteReservation, MaterializationError> {
    let filesystem = directory_identity(private_root)?.device;
    let mut reservations = byte_reservations().lock().map_err(|_| {
        MaterializationError::Io(io::Error::other("byte reservation gate poisoned"))
    })?;
    reserve_bytes(&mut reservations, filesystem, required, available)?;
    Ok(ByteReservation {
        filesystem,
        bytes: required,
    })
}

#[cfg(unix)]
fn reserve_bytes(
    reservations: &mut BTreeMap<u64, u64>,
    filesystem: u64,
    required: u64,
    available: u64,
) -> Result<(), MaterializationError> {
    let current = reservations.get(&filesystem).copied().unwrap_or(0);
    let total = current
        .checked_add(required)
        .ok_or(MaterializationError::InsufficientSpace)?;
    if total > available {
        return Err(MaterializationError::InsufficientSpace);
    }
    reservations.insert(filesystem, total);
    Ok(())
}

fn acquire_permit() -> Result<ConcurrencyPermit, MaterializationError> {
    static GATE: OnceLock<ConcurrencyGate> = OnceLock::new();
    let gate = GATE.get_or_init(|| ConcurrencyGate {
        active: Mutex::new(0),
        changed: Condvar::new(),
    });
    let mut active = gate
        .active
        .lock()
        .map_err(|_| MaterializationError::Io(io::Error::other("materialization gate poisoned")))?;
    while *active >= MAX_CONCURRENT_MATERIALIZATIONS {
        active = gate.changed.wait(active).map_err(|_| {
            MaterializationError::Io(io::Error::other("materialization gate poisoned"))
        })?;
    }
    *active += 1;
    Ok(ConcurrencyPermit(gate))
}

pub fn materialize_workspace(
    source: &Path,
    private_root: &Path,
    label: &str,
    policy: &MaterializationPolicy,
    preference: ClonePreference,
) -> Result<DisposableWorkspace, MaterializationError> {
    materialize_workspace_with_selection(
        source,
        private_root,
        label,
        policy,
        preference,
        SourceSelection::SanitizedTree,
        || Ok(()),
    )
}

/// Computes the same bounded, no-follow manifest used by WSP publication.
///
/// This read-only seam lets an external recovery format prove that an
/// independently reconstructed tree exactly matches a retained disposable
/// workspace before cleanup authority is exercised.
pub fn inspect_workspace_manifest(
    root: &Path,
    policy: &MaterializationPolicy,
) -> Result<String, MaterializationError> {
    policy.validate()?;
    #[cfg(unix)]
    {
        let root_dir = open_directory_path(root)?;
        Ok(
            scan_tree_from_open_dir(root_dir, policy, ScanPurpose::Destination)?
                .manifest
                .sha256,
        )
    }
    #[cfg(not(unix))]
    {
        Ok(scan_tree(root, policy, ScanPurpose::Destination)?
            .manifest
            .sha256)
    }
}

/// Read one explicitly selected source file without following links in its path.
#[cfg(unix)]
pub(crate) fn read_bounded_regular_file(
    root: &Path,
    relative: &Path,
    maximum: u64,
) -> Result<Vec<u8>, MaterializationError> {
    let relative = normalize_relative_entry(relative)?;
    use std::os::unix::fs::OpenOptionsExt;
    let mut directory = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(root)
        .map_err(MaterializationError::Io)?;
    let parts: Vec<_> = relative.components().collect();
    for part in &parts[..parts.len() - 1] {
        directory = open_directory_at(&directory, part.as_os_str())?;
    }
    let name = parts
        .last()
        .ok_or(MaterializationError::InvalidAllowlistEntry)?
        .as_os_str();
    let metadata = metadata_at(&directory, name)?;
    if !matches!(metadata.kind, AtKind::Regular) {
        return Err(MaterializationError::SpecialFile);
    }
    if metadata.identity.links != 1 {
        return Err(MaterializationError::HardLinkRejected);
    }
    if metadata.identity.size > maximum {
        return Err(MaterializationError::TooManyBytes);
    }
    let file = open_regular_at(&directory, name, libc::O_RDONLY | libc::O_NONBLOCK)?;
    if !metadata_matches(
        &file.metadata().map_err(MaterializationError::Io)?,
        metadata.identity,
    ) {
        return Err(MaterializationError::SourceChanged);
    }
    let mut bytes = Vec::new();
    (&file)
        .take(maximum.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(MaterializationError::Io)?;
    if bytes.len() as u64 > maximum
        || !metadata_matches(
            &file.metadata().map_err(MaterializationError::Io)?,
            metadata.identity,
        )
    {
        return Err(MaterializationError::SourceChanged);
    }
    Ok(bytes)
}

/// Replace a frozen preimage inside a host-owned disposable checkout. The open
/// descriptor pins the target; neither directory links nor hard links are allowed.
#[cfg(unix)]
pub(crate) fn replace_bounded_regular_file(
    root: &Path,
    relative: &Path,
    expected: &[u8],
    replacement: &[u8],
) -> Result<(), MaterializationError> {
    if expected.len() > 256 * 1024 || replacement.len() > 256 * 1024 {
        return Err(MaterializationError::TooManyBytes);
    }
    let relative = normalize_relative_entry(relative)?;
    use std::os::unix::fs::OpenOptionsExt;
    let mut directory = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(root)
        .map_err(MaterializationError::Io)?;
    let parts: Vec<_> = relative.components().collect();
    for part in &parts[..parts.len() - 1] {
        directory = open_directory_at(&directory, part.as_os_str())?;
    }
    let name = parts
        .last()
        .ok_or(MaterializationError::InvalidAllowlistEntry)?
        .as_os_str();
    let metadata = metadata_at(&directory, name)?;
    if !matches!(metadata.kind, AtKind::Regular) {
        return Err(MaterializationError::SpecialFile);
    }
    if metadata.identity.links != 1 {
        return Err(MaterializationError::HardLinkRejected);
    }
    if metadata.identity.size != expected.len() as u64 {
        return Err(MaterializationError::SourceChanged);
    }
    let mut file = open_regular_at(&directory, name, libc::O_RDWR | libc::O_NONBLOCK)?;
    if !metadata_matches(
        &file.metadata().map_err(MaterializationError::Io)?,
        metadata.identity,
    ) {
        return Err(MaterializationError::SourceChanged);
    }
    let mut bytes = Vec::new();
    (&file)
        .take(expected.len() as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(MaterializationError::Io)?;
    if bytes != expected
        || !metadata_matches(
            &file.metadata().map_err(MaterializationError::Io)?,
            metadata.identity,
        )
    {
        return Err(MaterializationError::SourceChanged);
    }
    file.rewind().map_err(MaterializationError::Io)?;
    file.set_len(0).map_err(MaterializationError::Io)?;
    file.write_all(replacement)
        .map_err(MaterializationError::Io)?;
    file.sync_all().map_err(MaterializationError::Io)?;
    Ok(())
}

/// Read a small host-verification input using the same descriptor-relative
/// no-follow traversal as materialization. Unlike manifests, this includes
/// file contents and therefore requires an explicit small byte budget.
#[cfg(unix)]
pub(crate) fn read_bounded_regular_snapshot(
    root: &Path,
    policy: &MaterializationPolicy,
) -> Result<BTreeMap<PathBuf, Vec<u8>>, MaterializationError> {
    fn walk(
        directory: &File,
        relative: &Path,
        policy: &MaterializationPolicy,
        observed: &mut ObservedEntryBudget,
        remaining: &mut u64,
        files: &mut BTreeMap<PathBuf, Vec<u8>>,
        depth: usize,
    ) -> Result<(), MaterializationError> {
        if depth > policy.max_depth {
            return Err(MaterializationError::TooDeep);
        }
        for name in read_directory_names(directory, observed)? {
            let metadata = metadata_at(directory, &name)?;
            let path = relative.join(&name);
            match metadata.kind {
                AtKind::Directory => {
                    let child = open_directory_at(directory, &name)?;
                    if !metadata_matches(
                        &child.metadata().map_err(MaterializationError::Io)?,
                        metadata.identity,
                    ) {
                        return Err(MaterializationError::SourceChanged);
                    }
                    walk(&child, &path, policy, observed, remaining, files, depth + 1)?;
                }
                AtKind::Regular => {
                    if metadata.identity.links != 1 {
                        return Err(MaterializationError::HardLinkRejected);
                    }
                    if metadata.identity.size > *remaining {
                        return Err(MaterializationError::TooManyBytes);
                    }
                    let file =
                        open_regular_at(directory, &name, libc::O_RDONLY | libc::O_NONBLOCK)?;
                    if !metadata_matches(
                        &file.metadata().map_err(MaterializationError::Io)?,
                        metadata.identity,
                    ) {
                        return Err(MaterializationError::SourceChanged);
                    }
                    let mut bytes = Vec::new();
                    (&file)
                        .take(remaining.saturating_add(1))
                        .read_to_end(&mut bytes)
                        .map_err(MaterializationError::Io)?;
                    if bytes.len() as u64 > *remaining
                        || !metadata_matches(
                            &file.metadata().map_err(MaterializationError::Io)?,
                            metadata.identity,
                        )
                    {
                        return Err(MaterializationError::SourceChanged);
                    }
                    *remaining -= bytes.len() as u64;
                    files.insert(path, bytes);
                }
                _ => return Err(MaterializationError::SpecialFile),
            }
        }
        Ok(())
    }
    // The host owns the root's parent (repository or private trial directory).
    // Pin the granted root directly: opening every ancestor for enumeration
    // would require unrelated read grants in the closed process sandbox.
    use std::os::unix::fs::OpenOptionsExt;
    let root = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(root)
        .map_err(MaterializationError::Io)?;
    let mut files = BTreeMap::new();
    let mut remaining = policy.max_logical_bytes;
    walk(
        &root,
        Path::new(""),
        policy,
        &mut ObservedEntryBudget::new(policy.max_entries),
        &mut remaining,
        &mut files,
        0,
    )?;
    Ok(files)
}

pub fn materialize_scheduled_workspace(
    source: &Path,
    private_root: &Path,
    label: &str,
    policy: &MaterializationPolicy,
    preference: ClonePreference,
    tracked: &ScheduledTrackedAllowlist,
) -> Result<DisposableWorkspace, MaterializationError> {
    materialize_workspace_with_selection(
        source,
        private_root,
        label,
        policy,
        preference,
        SourceSelection::Tracked(tracked),
        || Ok(()),
    )
}

#[cfg(test)]
fn materialize_workspace_with_hook(
    source: &Path,
    private_root: &Path,
    label: &str,
    policy: &MaterializationPolicy,
    preference: ClonePreference,
    after_initial_scan: impl FnOnce() -> Result<(), MaterializationError>,
) -> Result<DisposableWorkspace, MaterializationError> {
    materialize_workspace_with_selection(
        source,
        private_root,
        label,
        policy,
        preference,
        SourceSelection::SanitizedTree,
        after_initial_scan,
    )
}

#[derive(Clone, Copy)]
enum SourceSelection<'a> {
    SanitizedTree,
    Tracked(&'a ScheduledTrackedAllowlist),
}

#[cfg(unix)]
struct OpenMaterializationRoots {
    source: File,
    private_path: PathBuf,
    private_root: File,
}

fn materialize_workspace_with_selection(
    source: &Path,
    private_root: &Path,
    label: &str,
    policy: &MaterializationPolicy,
    preference: ClonePreference,
    selection: SourceSelection<'_>,
    after_initial_scan: impl FnOnce() -> Result<(), MaterializationError>,
) -> Result<DisposableWorkspace, MaterializationError> {
    #[cfg(not(unix))]
    {
        let _ = (
            source,
            private_root,
            label,
            policy,
            preference,
            selection,
            after_initial_scan,
        );
        return Err(MaterializationError::UnsupportedPlatform);
    }
    let _permit = acquire_permit()?;
    policy.validate()?;
    validate_label(label)?;
    let source_path =
        absolute_normal_path(source).map_err(|_| MaterializationError::InvalidSource)?;
    let private_path =
        absolute_normal_path(private_root).map_err(|_| MaterializationError::InvalidPrivateRoot)?;
    if source_path.starts_with(&private_path) || private_path.starts_with(&source_path) {
        return Err(MaterializationError::OverlappingRoots);
    }
    let source_dir =
        open_directory_path(&source_path).map_err(|_| MaterializationError::InvalidSource)?;
    let private_root_dir = prepare_private_root(&private_path)?;
    if directories_overlap(&source_dir, &private_root_dir)? {
        return Err(MaterializationError::OverlappingRoots);
    }

    #[cfg(unix)]
    {
        materialize_workspace_unix(
            OpenMaterializationRoots {
                source: source_dir,
                private_path,
                private_root: private_root_dir,
            },
            label,
            policy,
            preference,
            selection,
            after_initial_scan,
        )
    }
}

#[cfg(unix)]
fn materialize_workspace_unix(
    roots: OpenMaterializationRoots,
    label: &str,
    policy: &MaterializationPolicy,
    preference: ClonePreference,
    selection: SourceSelection<'_>,
    after_initial_scan: impl FnOnce() -> Result<(), MaterializationError>,
) -> Result<DisposableWorkspace, MaterializationError> {
    let OpenMaterializationRoots {
        source: source_dir,
        private_path: private_root,
        private_root: private_root_dir,
    } = roots;
    let root_identity = directory_identity(&private_root_dir)?;
    let before =
        scan_tree_from_open_dir_selected(source_dir, policy, ScanPurpose::Source, selection)?;
    if before.manifest.regular_files == 0 {
        return Err(MaterializationError::NoRegularFiles);
    }
    let required_space = policy
        .min_free_bytes
        .checked_add(before.manifest.logical_bytes)
        .ok_or(MaterializationError::TooManyBytes)?;
    let available =
        available_space_open(&private_root_dir)?.ok_or(MaterializationError::InsufficientSpace)?;
    let _space = acquire_byte_reservation(&private_root_dir, required_space, available)?;
    after_initial_scan()?;

    let workspace_id = Uuid::new_v4();
    let staging_name = OsString::from(format!(".staging-{workspace_id}"));
    let published_name = OsString::from(format!("workspace-{label}-{workspace_id}"));
    if entry_exists_at(&private_root_dir, &staging_name)?
        || entry_exists_at(&private_root_dir, &published_name)?
    {
        return Err(MaterializationError::InvalidPrivateRoot);
    }
    let staging_dir = create_directory_at(&private_root_dir, &staging_name)?;
    let workspace_identity = directory_identity(&staging_dir)?;
    let mut guard = StagingGuard::new(&private_root_dir, staging_name.clone(), workspace_identity)?;

    let (cloned, copied) = populate_destination_at(&staging_dir, &before, policy, preference)?;
    let after_root = before
        .root_dir
        .try_clone()
        .map_err(MaterializationError::Io)?;
    let after =
        scan_tree_from_open_dir_selected(after_root, policy, ScanPurpose::Source, selection)?;
    if before.manifest != after.manifest {
        return Err(MaterializationError::SourceChanged);
    }
    let destination = scan_tree_from_open_dir(
        staging_dir.try_clone().map_err(MaterializationError::Io)?,
        policy,
        ScanPurpose::Destination,
    )?;
    if before.manifest != destination.manifest {
        return Err(MaterializationError::DestinationMismatch);
    }
    let mode = materialization_mode(cloned, copied)?;
    let materialization = MaterializationReceipt {
        source_manifest_sha256: before.manifest.sha256.clone(),
        destination_manifest_sha256: destination.manifest.sha256.clone(),
        entries: before.manifest.entries.len(),
        regular_files: before.manifest.regular_files,
        logical_bytes: before.manifest.logical_bytes,
        mode,
    };
    let durable_receipt = DurableReceiptBody {
        schema: RECEIPT_SCHEMA,
        workspace_id,
        workspace_name: published_name
            .to_str()
            .ok_or(MaterializationError::InvalidPrivateRoot)?
            .to_owned(),
        root_device: root_identity.device,
        root_inode: root_identity.inode,
        workspace_device: workspace_identity.device,
        workspace_inode: workspace_identity.inode,
        materialization: materialization.clone(),
        policy: durable_policy(policy)?,
        cleanup_authorization: None,
    };
    persist_durable_receipt(&private_root_dir, &durable_receipt)?;
    let receipt_guard = DurableReceiptGuard::new(&private_root_dir, workspace_id)?;
    rename_entry_at(&private_root_dir, &staging_name, &published_name)?;
    guard.disarm();
    receipt_guard.disarm();
    fsync_directory(&private_root_dir)?;

    Ok(DisposableWorkspace {
        path: private_root.join(&published_name),
        private_root,
        workspace_id,
        workspace_device: workspace_identity.device,
        workspace_inode: workspace_identity.inode,
        private_root_dir,
        durable_receipt,
        location: WorkspaceLocation::Published,
        receipt: materialization,
    })
}

fn materialization_mode(
    cloned: usize,
    copied: usize,
) -> Result<MaterializationMode, MaterializationError> {
    match (cloned, copied) {
        (value, 0) if value > 0 => Ok(MaterializationMode::AllCloned),
        (0, value) if value > 0 => Ok(MaterializationMode::Copied),
        (left, right) if left > 0 && right > 0 => Ok(MaterializationMode::Mixed),
        _ => Err(MaterializationError::NoRegularFiles),
    }
}

fn durable_policy(policy: &MaterializationPolicy) -> Result<DurablePolicy, MaterializationError> {
    Ok(DurablePolicy {
        symlinks: policy.symlinks,
        max_entries: policy
            .max_entries
            .try_into()
            .map_err(|_| MaterializationError::InvalidPolicy)?,
        max_logical_bytes: policy.max_logical_bytes,
        max_depth: policy
            .max_depth
            .try_into()
            .map_err(|_| MaterializationError::InvalidPolicy)?,
    })
}

fn validate_label(label: &str) -> Result<(), MaterializationError> {
    if label.is_empty()
        || label.len() > MAX_LABEL_BYTES
        || !label.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
        })
    {
        return Err(MaterializationError::InvalidLabel);
    }
    Ok(())
}

fn normalize_relative_entry(path: &Path) -> Result<PathBuf, MaterializationError> {
    if path.as_os_str().is_empty() || path.is_absolute() {
        return Err(MaterializationError::InvalidAllowlistEntry);
    }
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(name) if name != OsStr::new("") => normalized.push(name),
            _ => return Err(MaterializationError::InvalidAllowlistEntry),
        }
    }
    if normalized.as_os_str().is_empty() {
        Err(MaterializationError::InvalidAllowlistEntry)
    } else {
        Ok(normalized)
    }
}

fn absolute_normal_path(path: &Path) -> Result<PathBuf, MaterializationError> {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(MaterializationError::Io)?
            .join(path)
    };
    let mut normalized = PathBuf::from("/");
    for component in path.components() {
        match component {
            Component::RootDir | Component::CurDir => {}
            Component::Normal(name) => normalized.push(name),
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Prefix(_) => return Err(MaterializationError::InvalidSource),
        }
    }
    Ok(normalized)
}

#[cfg(unix)]
fn prepare_private_root(root: &Path) -> Result<File, MaterializationError> {
    let mut current = open_filesystem_root()?;
    for component in root.components() {
        let Component::Normal(name) = component else {
            continue;
        };
        current = match open_directory_at(&current, name) {
            Ok(directory) => directory,
            Err(MaterializationError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
                create_directory_at(&current, name)?
            }
            Err(_) => return Err(MaterializationError::InvalidPrivateRoot),
        };
    }
    use std::os::unix::fs::MetadataExt;
    let metadata = current
        .metadata()
        .map_err(|_| MaterializationError::InvalidPrivateRoot)?;
    if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 {
        return Err(MaterializationError::InvalidPrivateRoot);
    }
    Ok(current)
}

#[cfg(not(unix))]
fn create_private_directory(path: &Path) -> Result<(), MaterializationError> {
    fs::create_dir(path).map_err(MaterializationError::Io)?;
    #[cfg(unix)]
    fs::set_permissions(path, unix_permissions(0o700)).map_err(MaterializationError::Io)?;
    Ok(())
}

#[cfg(unix)]
fn c_name(value: &OsStr) -> Result<std::ffi::CString, MaterializationError> {
    std::ffi::CString::new(value.as_bytes()).map_err(|_| MaterializationError::UnsafeCleanup)
}

#[cfg(unix)]
fn open_directory_path(path: &Path) -> Result<File, MaterializationError> {
    let normalized = absolute_normal_path(path)?;
    if !normalized.is_absolute() {
        return Err(MaterializationError::InvalidSource);
    }
    let mut current = open_filesystem_root()?;
    for component in normalized.components() {
        match component {
            Component::RootDir | Component::CurDir => {}
            Component::Normal(name) => current = open_directory_at(&current, name)?,
            Component::ParentDir | Component::Prefix(_) => {
                return Err(MaterializationError::InvalidSource)
            }
        }
    }
    Ok(current)
}

#[cfg(unix)]
fn open_filesystem_root() -> Result<File, MaterializationError> {
    let mut options = OpenOptions::new();
    options.read(true);
    use std::os::unix::fs::OpenOptionsExt;
    options.custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC);
    options
        .open(Path::new("/"))
        .map_err(MaterializationError::Io)
}

#[cfg(unix)]
fn open_directory_at(parent: &File, name: &OsStr) -> Result<File, MaterializationError> {
    let name = c_name(name)?;
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(MaterializationError::Io(io::Error::last_os_error()));
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}

#[cfg(unix)]
fn create_directory_at(parent: &File, name: &OsStr) -> Result<File, MaterializationError> {
    let name_c = c_name(name)?;
    if unsafe { libc::mkdirat(parent.as_raw_fd(), name_c.as_ptr(), 0o700) } != 0 {
        return Err(MaterializationError::Io(io::Error::last_os_error()));
    }
    open_directory_at(parent, name)
}

#[cfg(unix)]
fn open_regular_at(parent: &File, name: &OsStr, flags: i32) -> Result<File, MaterializationError> {
    let name = c_name(name)?;
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            flags | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0,
        )
    };
    if fd < 0 {
        return Err(MaterializationError::Io(io::Error::last_os_error()));
    }
    let file = unsafe { File::from_raw_fd(fd) };
    if !file.metadata().map_err(MaterializationError::Io)?.is_file() {
        return Err(MaterializationError::SpecialFile);
    }
    Ok(file)
}

#[cfg(all(unix, target_os = "macos"))]
fn open_symlink_at(parent: &File, name: &OsStr) -> Result<File, MaterializationError> {
    let name = c_name(name)?;
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_SYMLINK | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(MaterializationError::Io(io::Error::last_os_error()));
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}

#[cfg(all(unix, any(target_os = "freebsd", target_os = "linux")))]
fn open_symlink_at(parent: &File, name: &OsStr) -> Result<File, MaterializationError> {
    let name = c_name(name)?;
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_PATH | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(MaterializationError::Io(io::Error::last_os_error()));
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}

#[cfg(unix)]
fn create_regular_at(parent: &File, name: &OsStr, mode: u32) -> Result<File, MaterializationError> {
    let name = c_name(name)?;
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            mode as libc::c_uint,
        )
    };
    if fd < 0 {
        return Err(MaterializationError::Io(io::Error::last_os_error()));
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}

#[cfg(unix)]
fn directory_identity(file: &File) -> Result<DirectoryIdentity, MaterializationError> {
    use std::os::unix::fs::MetadataExt;
    let metadata = file.metadata().map_err(MaterializationError::Io)?;
    if !metadata.is_dir() {
        return Err(MaterializationError::UnsafeCleanup);
    }
    Ok(DirectoryIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

#[cfg(unix)]
fn identity_matches(
    file: &File,
    expected: DirectoryIdentity,
) -> Result<bool, MaterializationError> {
    Ok(directory_identity(file)? == expected)
}

#[cfg(unix)]
fn directory_has_ancestor(
    directory: &File,
    expected: DirectoryIdentity,
) -> Result<bool, MaterializationError> {
    let mut current = directory.try_clone().map_err(MaterializationError::Io)?;
    loop {
        let current_identity = directory_identity(&current)?;
        if current_identity == expected {
            return Ok(true);
        }
        let parent = open_directory_at(&current, OsStr::new(".."))?;
        if directory_identity(&parent)? == current_identity {
            return Ok(false);
        }
        current = parent;
    }
}

#[cfg(unix)]
fn directories_overlap(left: &File, right: &File) -> Result<bool, MaterializationError> {
    let left_identity = directory_identity(left)?;
    let right_identity = directory_identity(right)?;
    Ok(directory_has_ancestor(left, right_identity)?
        || directory_has_ancestor(right, left_identity)?)
}

#[cfg(unix)]
fn duplicate_fd(fd: RawFd) -> Result<RawFd, MaterializationError> {
    let duplicated = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 0) };
    if duplicated < 0 {
        Err(MaterializationError::Io(io::Error::last_os_error()))
    } else {
        Ok(duplicated)
    }
}

#[cfg(unix)]
struct DirectoryStream(*mut libc::DIR);

#[cfg(unix)]
impl Drop for DirectoryStream {
    fn drop(&mut self) {
        unsafe {
            libc::closedir(self.0);
        }
    }
}

#[cfg(unix)]
fn read_directory_names(
    directory: &File,
    budget: &mut ObservedEntryBudget,
) -> Result<Vec<OsString>, MaterializationError> {
    let duplicated = duplicate_fd(directory.as_raw_fd())?;
    let stream = unsafe { libc::fdopendir(duplicated) };
    if stream.is_null() {
        unsafe {
            libc::close(duplicated);
        }
        return Err(MaterializationError::Io(io::Error::last_os_error()));
    }
    let stream = DirectoryStream(stream);
    unsafe {
        libc::rewinddir(stream.0);
    }
    let mut names = Vec::new();
    loop {
        nix::errno::Errno::clear();
        let entry = unsafe { libc::readdir(stream.0) };
        if entry.is_null() {
            let errno = nix::errno::Errno::last_raw();
            if errno == 0 {
                break;
            }
            return Err(MaterializationError::Io(io::Error::from_raw_os_error(
                errno,
            )));
        }
        let bytes = unsafe { std::ffi::CStr::from_ptr((*entry).d_name.as_ptr()) }.to_bytes();
        if bytes != b"." && bytes != b".." {
            budget.observe()?;
            names.push(OsString::from_vec(bytes.to_vec()));
        }
    }
    names.sort();
    Ok(names)
}

#[cfg(unix)]
fn read_link_at(parent: &File, name: &OsStr) -> Result<PathBuf, MaterializationError> {
    let name = c_name(name)?;
    let mut capacity = 256_usize;
    loop {
        let mut bytes = vec![0_u8; capacity];
        let read = unsafe {
            libc::readlinkat(
                parent.as_raw_fd(),
                name.as_ptr(),
                bytes.as_mut_ptr().cast(),
                bytes.len(),
            )
        };
        if read < 0 {
            return Err(MaterializationError::Io(io::Error::last_os_error()));
        }
        let read = read as usize;
        if read < bytes.len() {
            bytes.truncate(read);
            return Ok(PathBuf::from(OsString::from_vec(bytes)));
        }
        capacity = capacity
            .checked_mul(2)
            .ok_or(MaterializationError::UnsafeSymlink)?;
        if capacity > 64 * 1024 {
            return Err(MaterializationError::UnsafeSymlink);
        }
    }
}

#[cfg(unix)]
fn resolve_directory_at(root: &File, relative: &Path) -> Result<File, MaterializationError> {
    let mut current = root.try_clone().map_err(MaterializationError::Io)?;
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err(MaterializationError::UnsafeCleanup);
        };
        current = open_directory_at(&current, name)?;
    }
    Ok(current)
}

#[cfg(unix)]
fn resolve_parent_at(
    root: &File,
    relative: &Path,
) -> Result<(File, OsString), MaterializationError> {
    let name = relative
        .file_name()
        .ok_or(MaterializationError::UnsafeCleanup)?
        .to_os_string();
    let parent = resolve_directory_at(root, relative.parent().unwrap_or_else(|| Path::new("")))?;
    Ok((parent, name))
}

#[cfg(unix)]
fn rename_entry_at(root: &File, from: &OsStr, to: &OsStr) -> Result<(), MaterializationError> {
    let from = c_name(from)?;
    let to = c_name(to)?;
    if unsafe {
        libc::renameat(
            root.as_raw_fd(),
            from.as_ptr(),
            root.as_raw_fd(),
            to.as_ptr(),
        )
    } != 0
    {
        return Err(MaterializationError::Io(io::Error::last_os_error()));
    }
    Ok(())
}

#[cfg(unix)]
fn unlink_entry_at(
    parent: &File,
    name: &OsStr,
    directory: bool,
) -> Result<(), MaterializationError> {
    let name = c_name(name)?;
    let flags = if directory { libc::AT_REMOVEDIR } else { 0 };
    if unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), flags) } != 0 {
        return Err(MaterializationError::Io(io::Error::last_os_error()));
    }
    Ok(())
}

#[cfg(unix)]
fn fsync_directory(directory: &File) -> Result<(), MaterializationError> {
    directory.sync_all().map_err(MaterializationError::Io)
}

#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AtKind {
    Directory,
    Regular,
    Symlink,
    Special,
}

#[cfg(unix)]
#[derive(Debug, Clone, Copy)]
struct AtMetadata {
    kind: AtKind,
    identity: FileIdentity,
}

#[cfg(unix)]
fn metadata_at(parent: &File, name: &OsStr) -> Result<AtMetadata, MaterializationError> {
    let name = c_name(name)?;
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    if unsafe {
        libc::fstatat(
            parent.as_raw_fd(),
            name.as_ptr(),
            stat.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } != 0
    {
        return Err(MaterializationError::Io(io::Error::last_os_error()));
    }
    let stat = unsafe { stat.assume_init() };
    let file_type = stat.st_mode & libc::S_IFMT;
    let kind = if file_type == libc::S_IFDIR {
        AtKind::Directory
    } else if file_type == libc::S_IFREG {
        AtKind::Regular
    } else if file_type == libc::S_IFLNK {
        AtKind::Symlink
    } else {
        AtKind::Special
    };
    Ok(AtMetadata {
        kind,
        identity: FileIdentity {
            size: stat
                .st_size
                .try_into()
                .map_err(|_| MaterializationError::TooManyBytes)?,
            mode: u32::from(stat.st_mode & 0o777),
            device: nonnegative_u64(stat.st_dev)?,
            inode: stat.st_ino,
            links: nonnegative_u64(stat.st_nlink)?,
        },
    })
}

#[cfg(unix)]
fn nonnegative_u64<T>(value: T) -> Result<u64, MaterializationError>
where
    T: TryInto<u64>,
{
    value
        .try_into()
        .map_err(|_| MaterializationError::TooManyBytes)
}

#[cfg(unix)]
fn entry_exists_at(parent: &File, name: &OsStr) -> Result<bool, MaterializationError> {
    let name = c_name(name)?;
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    if unsafe {
        libc::fstatat(
            parent.as_raw_fd(),
            name.as_ptr(),
            stat.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } == 0
    {
        return Ok(true);
    }
    let error = io::Error::last_os_error();
    if error.kind() == io::ErrorKind::NotFound {
        Ok(false)
    } else {
        Err(MaterializationError::Io(error))
    }
}

#[cfg(unix)]
fn ensure_receipt_directory(root: &File) -> Result<File, MaterializationError> {
    let name = OsStr::new(RECEIPT_DIRECTORY);
    let directory = if entry_exists_at(root, name)? {
        open_directory_at(root, name)?
    } else {
        create_directory_at(root, name)?
    };
    use std::os::unix::fs::MetadataExt;
    let metadata = directory.metadata().map_err(MaterializationError::Io)?;
    if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 {
        return Err(MaterializationError::InvalidDurableReceipt);
    }
    Ok(directory)
}

#[cfg(unix)]
fn open_receipt_directory(root: &File) -> Result<File, MaterializationError> {
    let directory = open_directory_at(root, OsStr::new(RECEIPT_DIRECTORY))
        .map_err(|_| MaterializationError::InvalidDurableReceipt)?;
    use std::os::unix::fs::MetadataExt;
    let metadata = directory
        .metadata()
        .map_err(|_| MaterializationError::InvalidDurableReceipt)?;
    if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 {
        return Err(MaterializationError::InvalidDurableReceipt);
    }
    Ok(directory)
}

fn receipt_file_name(workspace_id: Uuid) -> OsString {
    OsString::from(format!("{workspace_id}.json"))
}

#[cfg(unix)]
fn persist_durable_receipt(
    root: &File,
    body: &DurableReceiptBody,
) -> Result<(), MaterializationError> {
    write_durable_receipt(root, body, false)
}

#[cfg(unix)]
fn replace_durable_receipt(
    root: &File,
    body: &DurableReceiptBody,
) -> Result<(), MaterializationError> {
    write_durable_receipt(root, body, true)
}

#[cfg(unix)]
fn write_durable_receipt(
    root: &File,
    body: &DurableReceiptBody,
    replace_existing: bool,
) -> Result<(), MaterializationError> {
    let receipt_directory = ensure_receipt_directory(root)?;
    let body_bytes = serde_json::to_vec(body).map_err(|error| {
        MaterializationError::Io(io::Error::new(io::ErrorKind::InvalidData, error))
    })?;
    let receipt = DurableWorkspaceReceipt {
        body: body.clone(),
        body_sha256: format!("{:x}", Sha256::digest(&body_bytes)),
    };
    let bytes = serde_json::to_vec(&receipt).map_err(|error| {
        MaterializationError::Io(io::Error::new(io::ErrorKind::InvalidData, error))
    })?;
    if bytes.len() as u64 > MAX_DURABLE_RECEIPT_BYTES {
        return Err(MaterializationError::InvalidDurableReceipt);
    }
    let final_name = receipt_file_name(body.workspace_id);
    if entry_exists_at(&receipt_directory, &final_name)? != replace_existing {
        return Err(MaterializationError::InvalidDurableReceipt);
    }
    let temporary_name = OsString::from(format!(".{}.{}.tmp", body.workspace_id, Uuid::new_v4()));
    let mut file = create_regular_at(&receipt_directory, &temporary_name, 0o600)?;
    let write_result = (|| {
        file.write_all(&bytes).map_err(MaterializationError::Io)?;
        file.sync_all().map_err(MaterializationError::Io)?;
        rename_entry_at(&receipt_directory, &temporary_name, &final_name)?;
        fsync_directory(&receipt_directory)
    })();
    if write_result.is_err() {
        let _ = unlink_entry_at(&receipt_directory, &temporary_name, false);
    }
    write_result
}

#[cfg(unix)]
fn load_durable_receipt(
    root: &File,
    workspace_id: Uuid,
) -> Result<DurableReceiptBody, MaterializationError> {
    let receipt_directory = open_receipt_directory(root)?;
    let name = receipt_file_name(workspace_id);
    let mut file = open_regular_at(&receipt_directory, &name, libc::O_RDONLY | libc::O_NONBLOCK)
        .map_err(|_| MaterializationError::InvalidDurableReceipt)?;
    use std::os::unix::fs::MetadataExt;
    let before = file
        .metadata()
        .map_err(|_| MaterializationError::InvalidDurableReceipt)?;
    if before.uid() != unsafe { libc::geteuid() }
        || before.mode() & 0o077 != 0
        || before.nlink() != 1
        || before.len() > MAX_DURABLE_RECEIPT_BYTES
    {
        return Err(MaterializationError::InvalidDurableReceipt);
    }
    let mut bytes = Vec::with_capacity(before.len() as usize);
    file.read_to_end(&mut bytes)
        .map_err(|_| MaterializationError::InvalidDurableReceipt)?;
    let after = file
        .metadata()
        .map_err(|_| MaterializationError::InvalidDurableReceipt)?;
    if before.dev() != after.dev()
        || before.ino() != after.ino()
        || before.len() != after.len()
        || bytes.len() as u64 != before.len()
    {
        return Err(MaterializationError::InvalidDurableReceipt);
    }
    let receipt: DurableWorkspaceReceipt =
        serde_json::from_slice(&bytes).map_err(|_| MaterializationError::InvalidDurableReceipt)?;
    let body_bytes = serde_json::to_vec(&receipt.body)
        .map_err(|_| MaterializationError::InvalidDurableReceipt)?;
    if receipt.body.schema != RECEIPT_SCHEMA
        || receipt.body.workspace_id != workspace_id
        || receipt.body_sha256 != format!("{:x}", Sha256::digest(&body_bytes))
    {
        return Err(MaterializationError::InvalidDurableReceipt);
    }
    Ok(receipt.body)
}

#[cfg(unix)]
fn remove_durable_receipt(root: &File, workspace_id: Uuid) -> Result<(), MaterializationError> {
    let directory = open_receipt_directory(root)?;
    unlink_entry_at(&directory, &receipt_file_name(workspace_id), false)?;
    fsync_directory(&directory)
}

#[cfg(unix)]
struct DurableReceiptGuard {
    root: File,
    workspace_id: Uuid,
    armed: bool,
}

#[cfg(unix)]
impl DurableReceiptGuard {
    fn new(root: &File, workspace_id: Uuid) -> Result<Self, MaterializationError> {
        Ok(Self {
            root: root.try_clone().map_err(MaterializationError::Io)?,
            workspace_id,
            armed: true,
        })
    }

    fn disarm(mut self) {
        self.armed = false;
    }
}

#[cfg(unix)]
impl Drop for DurableReceiptGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = remove_durable_receipt(&self.root, self.workspace_id);
        }
    }
}

#[cfg(unix)]
fn open_recovery_artifact_outside(
    path: &Path,
    private_root: DirectoryIdentity,
    workspace: DirectoryIdentity,
) -> Result<File, MaterializationError> {
    let absolute =
        absolute_normal_path(path).map_err(|_| MaterializationError::InvalidRecoveryArtifact)?;
    let name = absolute
        .file_name()
        .ok_or(MaterializationError::InvalidRecoveryArtifact)?;
    let parent_path = absolute
        .parent()
        .ok_or(MaterializationError::InvalidRecoveryArtifact)?;
    let parent = open_directory_path(parent_path)
        .map_err(|_| MaterializationError::InvalidRecoveryArtifact)?;
    if directory_has_ancestor(&parent, private_root)? || directory_has_ancestor(&parent, workspace)?
    {
        return Err(MaterializationError::InvalidRecoveryArtifact);
    }
    let file = open_regular_at(&parent, name, libc::O_RDONLY | libc::O_NONBLOCK)
        .map_err(|_| MaterializationError::InvalidRecoveryArtifact)?;
    let metadata = file
        .metadata()
        .map_err(|_| MaterializationError::InvalidRecoveryArtifact)?;
    if !metadata.is_file() || metadata.len() > MAX_RECOVERY_ARTIFACT_BYTES {
        return Err(MaterializationError::InvalidRecoveryArtifact);
    }
    use std::os::unix::fs::MetadataExt;
    if metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o077 != 0
        || metadata.nlink() != 1
    {
        return Err(MaterializationError::InvalidRecoveryArtifact);
    }
    Ok(file)
}

fn hex_digest(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn hash_bounded_open_file(
    file: &mut File,
    max_bytes: u64,
) -> Result<([u8; 32], u64), MaterializationError> {
    let before = file
        .metadata()
        .map_err(|_| MaterializationError::InvalidRecoveryArtifact)?;
    if !before.is_file() || before.len() > max_bytes {
        return Err(MaterializationError::InvalidRecoveryArtifact);
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|_| MaterializationError::InvalidRecoveryArtifact)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; COPY_BUFFER_BYTES];
    let mut total = 0_u64;
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|_| MaterializationError::InvalidRecoveryArtifact)?;
        if read == 0 {
            break;
        }
        total = total
            .checked_add(read as u64)
            .ok_or(MaterializationError::InvalidRecoveryArtifact)?;
        if total > max_bytes || total > before.len() {
            return Err(MaterializationError::InvalidRecoveryArtifact);
        }
        hasher.update(&buffer[..read]);
    }
    let after = file
        .metadata()
        .map_err(|_| MaterializationError::InvalidRecoveryArtifact)?;
    if total != before.len() || before.len() != after.len() {
        return Err(MaterializationError::InvalidRecoveryArtifact);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if before.dev() != after.dev() || before.ino() != after.ino() {
            return Err(MaterializationError::InvalidRecoveryArtifact);
        }
    }
    Ok((hasher.finalize().into(), total))
}

#[cfg(unix)]
fn remove_open_directory_contents(directory: &File) -> Result<(), MaterializationError> {
    if unsafe { libc::fchmod(directory.as_raw_fd(), 0o700) } != 0 {
        return Err(MaterializationError::Io(io::Error::last_os_error()));
    }
    let mut cleanup_budget = ObservedEntryBudget::new(DEFAULT_MAX_ENTRIES);
    let owned_device = directory_identity(directory)?.device;
    remove_open_directory_contents_inner(directory, owned_device, &mut cleanup_budget)
}

#[cfg(unix)]
fn remove_open_directory_contents_inner(
    directory: &File,
    owned_device: u64,
    cleanup_budget: &mut ObservedEntryBudget,
) -> Result<(), MaterializationError> {
    for name in read_directory_names(directory, cleanup_budget)? {
        let before = metadata_at(directory, &name)?;
        run_cleanup_entry_hook(directory, &name, before)?;
        if before.identity.device != owned_device {
            return Err(MaterializationError::UnsafeCleanup);
        }
        match before.kind {
            AtKind::Directory => {
                let child = open_directory_at(directory, &name)
                    .map_err(|_| MaterializationError::UnsafeCleanup)?;
                if !opened_entry_matches(&child, before)? {
                    return Err(MaterializationError::UnsafeCleanup);
                }
                if unsafe { libc::fchmod(child.as_raw_fd(), 0o700) } != 0 {
                    return Err(MaterializationError::Io(io::Error::last_os_error()));
                }
                remove_open_directory_contents_inner(&child, owned_device, cleanup_budget)?;
                ensure_name_still_refers_to(directory, &name, before)?;
                unlink_entry_at(directory, &name, true)?;
            }
            AtKind::Regular => {
                if before.identity.links != 1 {
                    return Err(MaterializationError::UnsafeCleanup);
                }
                let file = open_regular_at(directory, &name, libc::O_RDONLY | libc::O_NONBLOCK)
                    .map_err(|_| MaterializationError::UnsafeCleanup)?;
                if !opened_entry_matches(&file, before)? {
                    return Err(MaterializationError::UnsafeCleanup);
                }
                if unsafe { libc::fchmod(file.as_raw_fd(), 0o600) } != 0 {
                    return Err(MaterializationError::Io(io::Error::last_os_error()));
                }
                ensure_name_still_refers_to(directory, &name, before)?;
                unlink_entry_at(directory, &name, false)?;
            }
            AtKind::Symlink => {
                if before.identity.links != 1 {
                    return Err(MaterializationError::UnsafeCleanup);
                }
                let link = open_symlink_at(directory, &name)
                    .map_err(|_| MaterializationError::UnsafeCleanup)?;
                if !opened_entry_matches(&link, before)? {
                    return Err(MaterializationError::UnsafeCleanup);
                }
                ensure_name_still_refers_to(directory, &name, before)?;
                unlink_entry_at(directory, &name, false)?;
            }
            AtKind::Special => {
                return Err(MaterializationError::UnsafeCleanup);
            }
        }
    }
    Ok(())
}

#[cfg(unix)]
fn opened_entry_matches(file: &File, expected: AtMetadata) -> Result<bool, MaterializationError> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    let metadata = file.metadata().map_err(MaterializationError::Io)?;
    let kind_matches = match expected.kind {
        AtKind::Directory => metadata.is_dir(),
        AtKind::Regular => metadata.is_file(),
        AtKind::Symlink => metadata.file_type().is_symlink(),
        AtKind::Special => {
            metadata.file_type().is_block_device()
                || metadata.file_type().is_char_device()
                || metadata.file_type().is_fifo()
                || metadata.file_type().is_socket()
        }
    };
    Ok(kind_matches
        && metadata.dev() == expected.identity.device
        && metadata.ino() == expected.identity.inode
        && metadata.len() == expected.identity.size
        && metadata.mode() & 0o777 == expected.identity.mode
        && metadata.nlink() == expected.identity.links)
}

#[cfg(unix)]
fn ensure_name_still_refers_to(
    parent: &File,
    name: &OsStr,
    expected: AtMetadata,
) -> Result<(), MaterializationError> {
    let actual = metadata_at(parent, name)?;
    if actual.kind != expected.kind
        || actual.identity.device != expected.identity.device
        || actual.identity.inode != expected.identity.inode
    {
        return Err(MaterializationError::UnsafeCleanup);
    }
    Ok(())
}

#[cfg(all(unix, not(test)))]
fn run_cleanup_entry_hook(
    _parent: &File,
    _name: &OsStr,
    _observed: AtMetadata,
) -> Result<(), MaterializationError> {
    Ok(())
}

#[cfg(all(unix, test))]
type CleanupEntryHook =
    Box<dyn FnMut(&File, &OsStr, AtMetadata) -> Result<(), MaterializationError> + Send + 'static>;

#[cfg(all(unix, test))]
fn cleanup_entry_hook() -> &'static Mutex<Option<CleanupEntryHook>> {
    static HOOK: OnceLock<Mutex<Option<CleanupEntryHook>>> = OnceLock::new();
    HOOK.get_or_init(|| Mutex::new(None))
}

#[cfg(all(unix, test))]
fn run_cleanup_entry_hook(
    parent: &File,
    name: &OsStr,
    observed: AtMetadata,
) -> Result<(), MaterializationError> {
    let mut hook = cleanup_entry_hook()
        .lock()
        .map_err(|_| MaterializationError::UnsafeCleanup)?;
    match hook.as_mut() {
        Some(hook) => hook(parent, name, observed),
        None => Ok(()),
    }
}

#[cfg(unix)]
fn remove_owned_tree_at(
    root: &File,
    name: &OsStr,
    expected: DirectoryIdentity,
) -> Result<(), MaterializationError> {
    let directory =
        open_directory_at(root, name).map_err(|_| MaterializationError::UnsafeCleanup)?;
    if !identity_matches(&directory, expected)? {
        return Err(MaterializationError::UnsafeCleanup);
    }
    remove_open_directory_contents(&directory)?;
    let after = metadata_at(root, name)?;
    if after.kind != AtKind::Directory
        || after.identity.device != expected.device
        || after.identity.inode != expected.inode
    {
        return Err(MaterializationError::UnsafeCleanup);
    }
    unlink_entry_at(root, name, true)
}

#[cfg(unix)]
fn reopen_workspace(
    private_root: &Path,
    workspace_id: Uuid,
) -> Result<DisposableWorkspace, MaterializationError> {
    let private_root =
        absolute_normal_path(private_root).map_err(|_| MaterializationError::InvalidPrivateRoot)?;
    let private_root_dir = open_directory_path(&private_root)?;
    let root_identity = directory_identity(&private_root_dir)?;
    let durable_receipt = load_durable_receipt(&private_root_dir, workspace_id)?;
    if durable_receipt.root_device != root_identity.device
        || durable_receipt.root_inode != root_identity.inode
        || !validate_sha256_value(&durable_receipt.materialization.source_manifest_sha256)
        || !validate_sha256_value(&durable_receipt.materialization.destination_manifest_sha256)
    {
        return Err(MaterializationError::InvalidDurableReceipt);
    }
    let expected_suffix = format!("-{workspace_id}");
    if !durable_receipt.workspace_name.starts_with("workspace-")
        || !durable_receipt.workspace_name.ends_with(&expected_suffix)
        || !matches!(
            Path::new(&durable_receipt.workspace_name)
                .components()
                .collect::<Vec<_>>()
                .as_slice(),
            [Component::Normal(_)]
        )
    {
        return Err(MaterializationError::InvalidDurableReceipt);
    }
    let published_name = OsString::from(&durable_receipt.workspace_name);
    let deleting_name = OsString::from(format!(".deleting-{workspace_id}"));
    let published = entry_exists_at(&private_root_dir, &published_name)?;
    let deleting = entry_exists_at(&private_root_dir, &deleting_name)?;
    let (location, name) = match (published, deleting) {
        (true, false) => (WorkspaceLocation::Published, published_name),
        (false, true) => (WorkspaceLocation::Deleting, deleting_name),
        _ => return Err(MaterializationError::InvalidDurableReceipt),
    };
    let workspace = open_directory_at(&private_root_dir, &name)
        .map_err(|_| MaterializationError::InvalidDurableReceipt)?;
    if !identity_matches(
        &workspace,
        DirectoryIdentity {
            device: durable_receipt.workspace_device,
            inode: durable_receipt.workspace_inode,
        },
    )? {
        return Err(MaterializationError::InvalidDurableReceipt);
    }
    let receipt = durable_receipt.materialization.clone();
    Ok(DisposableWorkspace {
        path: private_root.join(name),
        private_root,
        workspace_id,
        workspace_device: durable_receipt.workspace_device,
        workspace_inode: durable_receipt.workspace_inode,
        private_root_dir,
        durable_receipt,
        location,
        receipt,
    })
}

#[cfg(not(unix))]
fn reopen_workspace(
    _private_root: &Path,
    _workspace_id: Uuid,
) -> Result<DisposableWorkspace, MaterializationError> {
    Err(MaterializationError::InvalidDurableReceipt)
}

#[cfg(any(test, not(unix)))]
fn scan_tree(
    root: &Path,
    policy: &MaterializationPolicy,
    purpose: ScanPurpose,
) -> Result<ScannedTree, MaterializationError> {
    #[cfg(unix)]
    {
        let root_dir = open_directory_path(root)?;
        scan_tree_from_open_dir(root_dir, policy, purpose)
    }
    #[cfg(not(unix))]
    {
        let mut entries = Vec::new();
        let mut identities = BTreeMap::new();
        let mut logical_bytes = 0_u64;
        let mut regular_files = 0_usize;
        scan_directory(
            root,
            root,
            Path::new(""),
            0,
            policy,
            purpose,
            &mut entries,
            &mut identities,
            &mut logical_bytes,
            &mut regular_files,
        )?;
        entries.sort_by(|left, right| left.relative.cmp(&right.relative));
        let sha256 = hash_manifest(&entries, logical_bytes, regular_files);
        Ok(ScannedTree {
            manifest: TreeManifest {
                entries,
                logical_bytes,
                regular_files,
                sha256,
            },
            identities,
        })
    }
}

#[cfg(unix)]
fn scan_tree_from_open_dir(
    root_dir: File,
    policy: &MaterializationPolicy,
    purpose: ScanPurpose,
) -> Result<ScannedTree, MaterializationError> {
    scan_tree_from_open_dir_selected(root_dir, policy, purpose, SourceSelection::SanitizedTree)
}

#[cfg(unix)]
fn scan_tree_from_open_dir_selected(
    root_dir: File,
    policy: &MaterializationPolicy,
    purpose: ScanPurpose,
    selection: SourceSelection<'_>,
) -> Result<ScannedTree, MaterializationError> {
    let mut entries = Vec::new();
    let mut identities = BTreeMap::new();
    let mut logical_bytes = 0_u64;
    let mut regular_files = 0_usize;
    let mut observed = ObservedEntryBudget::new(policy.max_entries);
    scan_directory_at(
        &root_dir,
        Path::new(""),
        0,
        policy,
        purpose,
        selection,
        &mut observed,
        &mut entries,
        &mut identities,
        &mut logical_bytes,
        &mut regular_files,
    )?;
    validate_selected_manifest(&entries, selection)?;
    validate_symlink_graph(&entries)?;
    entries.sort_by(|left, right| left.relative.cmp(&right.relative));
    let sha256 = hash_manifest(&entries, logical_bytes, regular_files);
    Ok(ScannedTree {
        manifest: TreeManifest {
            entries,
            logical_bytes,
            regular_files,
            sha256,
        },
        identities,
        root_dir,
    })
}

#[cfg(unix)]
#[allow(clippy::too_many_arguments)]
fn scan_directory_at(
    directory: &File,
    relative_directory: &Path,
    depth: usize,
    policy: &MaterializationPolicy,
    purpose: ScanPurpose,
    selection: SourceSelection<'_>,
    observed: &mut ObservedEntryBudget,
    entries: &mut Vec<ManifestEntry>,
    identities: &mut BTreeMap<PathBuf, FileIdentity>,
    logical_bytes: &mut u64,
    regular_files: &mut usize,
) -> Result<(), MaterializationError> {
    if depth > policy.max_depth {
        return Err(MaterializationError::TooDeep);
    }
    for name in read_directory_names(directory, observed)? {
        let relative = relative_directory.join(&name);
        if is_excluded_name(&name) {
            if purpose == ScanPurpose::Destination {
                return Err(MaterializationError::UnexpectedExcludedEntry);
            }
            continue;
        }
        if let SourceSelection::Tracked(tracked) = selection {
            let metadata = metadata_at(directory, &name)?;
            if metadata.kind == AtKind::Directory {
                if !tracked.contains_or_contains_descendant(&relative) {
                    return Err(MaterializationError::AllowlistMismatch);
                }
            } else if !tracked.contains_exact(&relative) {
                return Err(MaterializationError::AllowlistMismatch);
            }
        }
        let metadata = metadata_at(directory, &name)?;
        match metadata.kind {
            AtKind::Directory => {
                let child = open_directory_at(directory, &name)?;
                if purpose == ScanPurpose::Source
                    && directory_contains_git_marker_at(&child, observed)?
                {
                    continue;
                }
                push_entry_bound(entries, policy)?;
                entries.push(ManifestEntry {
                    relative: relative.clone(),
                    kind: EntryKind::Directory,
                    mode: metadata.identity.mode,
                    size: 0,
                    content_sha256: Sha256::digest([]).into(),
                    symlink_target: None,
                });
                scan_directory_at(
                    &child,
                    &relative,
                    depth + 1,
                    policy,
                    purpose,
                    selection,
                    observed,
                    entries,
                    identities,
                    logical_bytes,
                    regular_files,
                )?;
            }
            AtKind::Regular => {
                if metadata.identity.links != 1 {
                    return Err(MaterializationError::HardLinkRejected);
                }
                push_entry_bound(entries, policy)?;
                *logical_bytes = logical_bytes
                    .checked_add(metadata.identity.size)
                    .ok_or(MaterializationError::TooManyBytes)?;
                if *logical_bytes > policy.max_logical_bytes {
                    return Err(MaterializationError::TooManyBytes);
                }
                let mut file =
                    open_regular_at(directory, &name, libc::O_RDONLY | libc::O_NONBLOCK)?;
                if !metadata_matches(
                    &file.metadata().map_err(MaterializationError::Io)?,
                    metadata.identity,
                ) {
                    return Err(MaterializationError::SourceChanged);
                }
                let digest = hash_regular_open_file(&mut file, metadata.identity)?;
                entries.push(ManifestEntry {
                    relative: relative.clone(),
                    kind: EntryKind::Regular,
                    mode: metadata.identity.mode,
                    size: metadata.identity.size,
                    content_sha256: digest,
                    symlink_target: None,
                });
                identities.insert(relative, metadata.identity);
                *regular_files += 1;
            }
            AtKind::Symlink => {
                if policy.symlinks == SymlinkPolicy::RejectAll {
                    return Err(MaterializationError::SymlinkRejected);
                }
                let target = read_link_at(directory, &name)?;
                validate_relative_symlink(&relative, &target)?;
                push_entry_bound(entries, policy)?;
                entries.push(ManifestEntry {
                    relative,
                    kind: EntryKind::Symlink,
                    mode: 0,
                    size: os_str_bytes(target.as_os_str()).len() as u64,
                    content_sha256: Sha256::digest(os_str_bytes(target.as_os_str())).into(),
                    symlink_target: Some(target),
                });
            }
            AtKind::Special => return Err(MaterializationError::SpecialFile),
        }
    }
    Ok(())
}

#[cfg(not(unix))]
#[allow(clippy::too_many_arguments)]
fn scan_directory(
    root: &Path,
    directory: &Path,
    relative_directory: &Path,
    depth: usize,
    policy: &MaterializationPolicy,
    purpose: ScanPurpose,
    entries: &mut Vec<ManifestEntry>,
    identities: &mut BTreeMap<PathBuf, FileIdentity>,
    logical_bytes: &mut u64,
    regular_files: &mut usize,
) -> Result<(), MaterializationError> {
    if depth > policy.max_depth {
        return Err(MaterializationError::TooDeep);
    }
    let canonical = fs::canonicalize(directory).map_err(MaterializationError::Io)?;
    if !canonical.starts_with(root) {
        return Err(MaterializationError::SourceChanged);
    }
    let mut children = fs::read_dir(directory)
        .map_err(MaterializationError::Io)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(MaterializationError::Io)?;
    children.sort_by_key(|entry| entry.file_name());

    for child in children {
        let name = child.file_name();
        if is_excluded_name(&name) {
            if purpose == ScanPurpose::Destination {
                return Err(MaterializationError::UnexpectedExcludedEntry);
            }
            continue;
        }
        let relative = relative_directory.join(&name);
        let path = directory.join(&name);
        let metadata = fs::symlink_metadata(&path).map_err(MaterializationError::Io)?;
        if metadata.file_type().is_dir() {
            if purpose == ScanPurpose::Source && directory_contains_git_marker(&path)? {
                continue;
            }
            push_entry_bound(entries, policy)?;
            entries.push(ManifestEntry {
                relative: relative.clone(),
                kind: EntryKind::Directory,
                mode: normalized_mode(&metadata),
                size: 0,
                content_sha256: Sha256::digest([]).into(),
                symlink_target: None,
            });
            scan_directory(
                root,
                &path,
                &relative,
                depth + 1,
                policy,
                purpose,
                entries,
                identities,
                logical_bytes,
                regular_files,
            )?;
        } else if metadata.file_type().is_file() {
            push_entry_bound(entries, policy)?;
            *logical_bytes = logical_bytes
                .checked_add(metadata.len())
                .ok_or(MaterializationError::TooManyBytes)?;
            if *logical_bytes > policy.max_logical_bytes {
                return Err(MaterializationError::TooManyBytes);
            }
            let identity = file_identity(&metadata);
            let digest = hash_regular_file(&path, identity)?;
            entries.push(ManifestEntry {
                relative: relative.clone(),
                kind: EntryKind::Regular,
                mode: identity.mode,
                size: identity.size,
                content_sha256: digest,
                symlink_target: None,
            });
            identities.insert(relative, identity);
            *regular_files += 1;
        } else if metadata.file_type().is_symlink() {
            if policy.symlinks == SymlinkPolicy::RejectAll {
                return Err(MaterializationError::SymlinkRejected);
            }
            let target = fs::read_link(&path).map_err(MaterializationError::Io)?;
            validate_relative_symlink(&relative, &target)?;
            push_entry_bound(entries, policy)?;
            entries.push(ManifestEntry {
                relative,
                kind: EntryKind::Symlink,
                mode: 0,
                size: os_str_bytes(target.as_os_str()).len() as u64,
                content_sha256: Sha256::digest(os_str_bytes(target.as_os_str())).into(),
                symlink_target: Some(target),
            });
        } else {
            return Err(MaterializationError::SpecialFile);
        }
    }
    Ok(())
}

fn push_entry_bound(
    entries: &[ManifestEntry],
    policy: &MaterializationPolicy,
) -> Result<(), MaterializationError> {
    if entries.len() >= policy.max_entries {
        return Err(MaterializationError::TooManyEntries);
    }
    Ok(())
}

#[cfg(unix)]
fn directory_contains_git_marker_at(
    directory: &File,
    observed: &mut ObservedEntryBudget,
) -> Result<bool, MaterializationError> {
    Ok(read_directory_names(directory, observed)?
        .into_iter()
        .any(|name| is_git_name(&name)))
}

#[cfg(not(unix))]
fn directory_contains_git_marker(directory: &Path) -> Result<bool, MaterializationError> {
    let children = fs::read_dir(directory).map_err(MaterializationError::Io)?;
    for child in children {
        let child = child.map_err(MaterializationError::Io)?;
        if is_git_name(&child.file_name()) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn is_git_name(name: &OsStr) -> bool {
    name.to_string_lossy().eq_ignore_ascii_case(".git")
}

fn is_excluded_name(name: &OsStr) -> bool {
    let value = name.to_string_lossy();
    let lower = value.to_ascii_lowercase();
    is_git_name(name)
        || matches!(
            lower.as_str(),
            ".ca"
                | ".codex"
                | ".hermes"
                | ".mimocode"
                | ".config"
                | ".cache"
                | "target"
                | "node_modules"
                | "__pycache__"
                | ".venv"
                | "venv"
                | "sessions"
                | "credentials"
                | "credentials.json"
                | "auth.json"
                | ".ds_store"
        )
        || lower == ".env"
        || lower.starts_with(".env.")
}

fn validate_relative_symlink(relative: &Path, target: &Path) -> Result<(), MaterializationError> {
    if target.is_absolute() {
        return Err(MaterializationError::UnsafeSymlink);
    }
    let mut depth = relative.parent().map_or(0_usize, |parent| {
        parent
            .components()
            .filter(|component| matches!(component, Component::Normal(_)))
            .count()
    });
    for component in target.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(_) => depth = depth.saturating_add(1),
            Component::ParentDir if depth > 0 => depth -= 1,
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(MaterializationError::UnsafeSymlink)
            }
        }
    }
    Ok(())
}

fn validate_selected_manifest(
    entries: &[ManifestEntry],
    selection: SourceSelection<'_>,
) -> Result<(), MaterializationError> {
    let SourceSelection::Tracked(tracked) = selection else {
        return Ok(());
    };
    let present = entries
        .iter()
        .map(|entry| entry.relative.as_path())
        .collect::<BTreeSet<_>>();
    if tracked
        .entries
        .iter()
        .all(|entry| present.contains(entry.as_path()))
    {
        Ok(())
    } else {
        Err(MaterializationError::AllowlistMismatch)
    }
}

fn normalize_joined_relative(
    parent: &Path,
    target: &Path,
) -> Result<PathBuf, MaterializationError> {
    let mut components = parent
        .components()
        .filter_map(|component| match component {
            Component::Normal(name) => Some(name.to_os_string()),
            _ => None,
        })
        .collect::<Vec<_>>();
    for component in target.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(name) => components.push(name.to_os_string()),
            Component::ParentDir if !components.is_empty() => {
                components.pop();
            }
            _ => return Err(MaterializationError::InvalidSymlinkTarget),
        }
    }
    Ok(components.into_iter().collect())
}

fn validate_symlink_graph(entries: &[ManifestEntry]) -> Result<(), MaterializationError> {
    let by_path = entries
        .iter()
        .map(|entry| (entry.relative.as_path(), entry))
        .collect::<BTreeMap<_, _>>();
    for entry in entries
        .iter()
        .filter(|entry| entry.kind == EntryKind::Symlink)
    {
        let mut current = entry;
        let mut visited = BTreeSet::new();
        loop {
            if !visited.insert(current.relative.as_path()) {
                return Err(MaterializationError::InvalidSymlinkTarget);
            }
            let target = current
                .symlink_target
                .as_deref()
                .ok_or(MaterializationError::InvalidSymlinkTarget)?;
            let resolved = normalize_joined_relative(
                current.relative.parent().unwrap_or_else(|| Path::new("")),
                target,
            )?;
            let target_entry = by_path
                .get(resolved.as_path())
                .ok_or(MaterializationError::InvalidSymlinkTarget)?;
            if target_entry.kind != EntryKind::Symlink {
                break;
            }
            current = target_entry;
        }
    }
    Ok(())
}

#[cfg(not(unix))]
fn hash_regular_file(
    path: &Path,
    identity: FileIdentity,
) -> Result<[u8; 32], MaterializationError> {
    let mut file = open_regular_checked(path, identity)?;
    hash_regular_open_file(&mut file, identity)
}

fn hash_regular_open_file(
    file: &mut File,
    identity: FileIdentity,
) -> Result<[u8; 32], MaterializationError> {
    file.seek(SeekFrom::Start(0))
        .map_err(MaterializationError::Io)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; COPY_BUFFER_BYTES];
    let mut read_total = 0_u64;
    loop {
        let read = file.read(&mut buffer).map_err(MaterializationError::Io)?;
        if read == 0 {
            break;
        }
        read_total = read_total
            .checked_add(read as u64)
            .ok_or(MaterializationError::TooManyBytes)?;
        if read_total > identity.size {
            return Err(MaterializationError::SourceChanged);
        }
        hasher.update(&buffer[..read]);
    }
    if read_total != identity.size
        || !metadata_matches(
            &file.metadata().map_err(MaterializationError::Io)?,
            identity,
        )
    {
        return Err(MaterializationError::SourceChanged);
    }
    Ok(hasher.finalize().into())
}

#[cfg(not(unix))]
fn open_regular_checked(path: &Path, identity: FileIdentity) -> Result<File, MaterializationError> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC);
    }
    let file = options.open(path).map_err(MaterializationError::Io)?;
    let metadata = file.metadata().map_err(MaterializationError::Io)?;
    if !metadata.is_file() || !metadata_matches(&metadata, identity) {
        return Err(MaterializationError::SourceChanged);
    }
    Ok(file)
}

fn file_identity(metadata: &fs::Metadata) -> FileIdentity {
    FileIdentity {
        size: metadata.len(),
        mode: normalized_mode(metadata),
        #[cfg(unix)]
        device: {
            use std::os::unix::fs::MetadataExt;
            metadata.dev()
        },
        #[cfg(unix)]
        inode: {
            use std::os::unix::fs::MetadataExt;
            metadata.ino()
        },
        #[cfg(unix)]
        links: {
            use std::os::unix::fs::MetadataExt;
            metadata.nlink()
        },
    }
}

fn metadata_matches(metadata: &fs::Metadata, identity: FileIdentity) -> bool {
    if metadata.len() != identity.size || normalized_mode(metadata) != identity.mode {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.dev() != identity.device || metadata.ino() != identity.inode {
            return false;
        }
    }
    true
}

fn normalized_mode(metadata: &fs::Metadata) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        metadata.mode() & 0o777
    }
    #[cfg(not(unix))]
    {
        u32::from(!metadata.permissions().readonly()) * 0o200
    }
}

#[cfg(all(unix, test))]
fn unix_permissions(mode: u32) -> fs::Permissions {
    use std::os::unix::fs::PermissionsExt;
    fs::Permissions::from_mode(mode)
}

fn hash_manifest(entries: &[ManifestEntry], logical_bytes: u64, regular_files: usize) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"ca-workspace-manifest-v1\0");
    hasher.update((entries.len() as u64).to_be_bytes());
    hasher.update(logical_bytes.to_be_bytes());
    hasher.update((regular_files as u64).to_be_bytes());
    for entry in entries {
        let path = path_bytes(&entry.relative);
        hasher.update((path.len() as u64).to_be_bytes());
        hasher.update(path);
        hasher.update([match entry.kind {
            EntryKind::Directory => 0,
            EntryKind::Regular => 1,
            EntryKind::Symlink => 2,
        }]);
        hasher.update(entry.mode.to_be_bytes());
        hasher.update(entry.size.to_be_bytes());
        hasher.update(entry.content_sha256);
    }
    format!("{:x}", hasher.finalize())
}

#[cfg(unix)]
fn populate_destination_at(
    destination: &File,
    scanned: &ScannedTree,
    policy: &MaterializationPolicy,
    preference: ClonePreference,
) -> Result<(usize, usize), MaterializationError> {
    for entry in scanned
        .manifest
        .entries
        .iter()
        .filter(|entry| entry.kind == EntryKind::Directory)
    {
        let (parent, name) = resolve_parent_at(destination, &entry.relative)?;
        create_directory_at(&parent, &name)?;
    }

    let mut cloned = 0_usize;
    let mut copied = 0_usize;
    for entry in &scanned.manifest.entries {
        match entry.kind {
            EntryKind::Directory => {}
            EntryKind::Regular => {
                let identity = *scanned
                    .identities
                    .get(&entry.relative)
                    .ok_or(MaterializationError::SourceChanged)?;
                let (source_parent, source_name) =
                    resolve_parent_at(&scanned.root_dir, &entry.relative)?;
                let mut source_file = open_regular_at(
                    &source_parent,
                    &source_name,
                    libc::O_RDONLY | libc::O_NONBLOCK,
                )?;
                if !metadata_matches(
                    &source_file.metadata().map_err(MaterializationError::Io)?,
                    identity,
                ) {
                    return Err(MaterializationError::SourceChanged);
                }
                let (destination_parent, destination_name) =
                    resolve_parent_at(destination, &entry.relative)?;
                match materialize_regular_file_at(
                    &mut source_file,
                    &destination_parent,
                    &destination_name,
                    identity,
                    entry,
                    preference,
                )? {
                    FileMaterialization::Cloned => cloned += 1,
                    FileMaterialization::Copied => copied += 1,
                }
            }
            EntryKind::Symlink => {
                if policy.symlinks != SymlinkPolicy::SafeRelative {
                    return Err(MaterializationError::SymlinkRejected);
                }
                let target = entry
                    .symlink_target
                    .as_ref()
                    .ok_or(MaterializationError::UnsafeSymlink)?;
                let (parent, name) = resolve_parent_at(destination, &entry.relative)?;
                create_symlink_at(target, &parent, &name)?;
            }
        }
    }

    let mut directories = scanned
        .manifest
        .entries
        .iter()
        .filter(|entry| entry.kind == EntryKind::Directory)
        .collect::<Vec<_>>();
    directories.sort_by_key(|entry| std::cmp::Reverse(entry.relative.components().count()));
    for entry in directories {
        let directory = resolve_directory_at(destination, &entry.relative)?;
        if unsafe { libc::fchmod(directory.as_raw_fd(), entry.mode as libc::mode_t) } != 0 {
            return Err(MaterializationError::Io(io::Error::last_os_error()));
        }
    }
    Ok((cloned, copied))
}

#[cfg(not(unix))]
fn populate_destination(
    source: &Path,
    destination: &Path,
    scanned: &ScannedTree,
    policy: &MaterializationPolicy,
    preference: ClonePreference,
) -> Result<(usize, usize), MaterializationError> {
    for entry in scanned
        .manifest
        .entries
        .iter()
        .filter(|entry| entry.kind == EntryKind::Directory)
    {
        let path = destination.join(&entry.relative);
        fs::create_dir(&path).map_err(MaterializationError::Io)?;
        #[cfg(unix)]
        fs::set_permissions(&path, unix_permissions(0o700)).map_err(MaterializationError::Io)?;
    }

    let mut cloned = 0_usize;
    let mut copied = 0_usize;
    for entry in &scanned.manifest.entries {
        match entry.kind {
            EntryKind::Directory => {}
            EntryKind::Regular => {
                let identity = *scanned
                    .identities
                    .get(&entry.relative)
                    .ok_or(MaterializationError::SourceChanged)?;
                let source_path = source.join(&entry.relative);
                let destination_path = destination.join(&entry.relative);
                match materialize_regular_file(
                    &source_path,
                    &destination_path,
                    identity,
                    entry,
                    preference,
                )? {
                    FileMaterialization::Cloned => cloned += 1,
                    FileMaterialization::Copied => copied += 1,
                }
            }
            EntryKind::Symlink => {
                if policy.symlinks != SymlinkPolicy::SafeRelative {
                    return Err(MaterializationError::SymlinkRejected);
                }
                let target = entry
                    .symlink_target
                    .as_ref()
                    .ok_or(MaterializationError::UnsafeSymlink)?;
                create_symlink(target, &destination.join(&entry.relative))?;
            }
        }
    }

    let mut directories = scanned
        .manifest
        .entries
        .iter()
        .filter(|entry| entry.kind == EntryKind::Directory)
        .collect::<Vec<_>>();
    directories.sort_by_key(|entry| std::cmp::Reverse(entry.relative.components().count()));
    #[cfg(unix)]
    for entry in directories {
        fs::set_permissions(
            destination.join(&entry.relative),
            unix_permissions(entry.mode),
        )
        .map_err(MaterializationError::Io)?;
    }
    Ok((cloned, copied))
}

#[cfg(unix)]
fn materialize_regular_file_at(
    source: &mut File,
    destination_parent: &File,
    destination_name: &OsStr,
    identity: FileIdentity,
    entry: &ManifestEntry,
    preference: ClonePreference,
) -> Result<FileMaterialization, MaterializationError> {
    if preference == ClonePreference::PreferClone {
        match try_clone_open_file(source, destination_parent, destination_name)? {
            Some(()) => {
                let mut destination = open_regular_at(
                    destination_parent,
                    destination_name,
                    libc::O_RDONLY | libc::O_NONBLOCK,
                )?;
                normalize_destination_open_file(&destination, entry.mode)?;
                let destination_metadata =
                    destination.metadata().map_err(MaterializationError::Io)?;
                let destination_identity = file_identity(&destination_metadata);
                if hash_regular_open_file(&mut destination, destination_identity)?
                    != entry.content_sha256
                {
                    return Err(MaterializationError::SourceChanged);
                }
                return Ok(FileMaterialization::Cloned);
            }
            None => source
                .seek(SeekFrom::Start(0))
                .map_err(MaterializationError::Io)?,
        };
    }

    let mut destination = create_regular_at(destination_parent, destination_name, 0o600)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; COPY_BUFFER_BYTES];
    let mut copied = 0_u64;
    loop {
        let read = source.read(&mut buffer).map_err(MaterializationError::Io)?;
        if read == 0 {
            break;
        }
        copied = copied
            .checked_add(read as u64)
            .ok_or(MaterializationError::TooManyBytes)?;
        if copied > entry.size {
            return Err(MaterializationError::SourceChanged);
        }
        destination
            .write_all(&buffer[..read])
            .map_err(MaterializationError::Io)?;
        hasher.update(&buffer[..read]);
    }
    destination.sync_all().map_err(MaterializationError::Io)?;
    let copied_sha256: [u8; 32] = hasher.finalize().into();
    if copied != entry.size
        || copied_sha256 != entry.content_sha256
        || !metadata_matches(
            &source.metadata().map_err(MaterializationError::Io)?,
            identity,
        )
    {
        return Err(MaterializationError::SourceChanged);
    }
    normalize_destination_open_file(&destination, entry.mode)?;
    Ok(FileMaterialization::Copied)
}

#[cfg(not(unix))]
fn materialize_regular_file(
    source: &Path,
    destination: &Path,
    identity: FileIdentity,
    entry: &ManifestEntry,
    preference: ClonePreference,
) -> Result<FileMaterialization, MaterializationError> {
    let mut source_file = open_regular_checked(source, identity)?;
    if preference == ClonePreference::PreferClone {
        match try_clone_open_file(&source_file, destination)? {
            Some(()) => {
                normalize_destination_file(destination, entry.mode)?;
                if hash_destination_file(destination, entry.size)? != entry.content_sha256 {
                    return Err(MaterializationError::SourceChanged);
                }
                return Ok(FileMaterialization::Cloned);
            }
            None => source_file
                .seek(SeekFrom::Start(0))
                .map_err(MaterializationError::Io)?,
        };
    }

    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    let mut destination_file = options
        .open(destination)
        .map_err(MaterializationError::Io)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; COPY_BUFFER_BYTES];
    let mut copied = 0_u64;
    loop {
        let read = source_file
            .read(&mut buffer)
            .map_err(MaterializationError::Io)?;
        if read == 0 {
            break;
        }
        copied = copied
            .checked_add(read as u64)
            .ok_or(MaterializationError::TooManyBytes)?;
        if copied > entry.size {
            return Err(MaterializationError::SourceChanged);
        }
        destination_file
            .write_all(&buffer[..read])
            .map_err(MaterializationError::Io)?;
        hasher.update(&buffer[..read]);
    }
    destination_file
        .sync_all()
        .map_err(MaterializationError::Io)?;
    let copied_sha256: [u8; 32] = hasher.finalize().into();
    if copied != entry.size
        || copied_sha256 != entry.content_sha256
        || !metadata_matches(
            &source_file.metadata().map_err(MaterializationError::Io)?,
            identity,
        )
    {
        return Err(MaterializationError::SourceChanged);
    }
    drop(destination_file);
    normalize_destination_file(destination, entry.mode)?;
    Ok(FileMaterialization::Copied)
}

#[cfg(target_os = "macos")]
fn try_clone_open_file(
    source: &File,
    destination_parent: &File,
    destination_name: &OsStr,
) -> Result<Option<()>, MaterializationError> {
    const CLONE_NOOWNERCOPY: u32 = 0x2;
    const CLONE_NOFOLLOW_ANY: u32 = 0x8;
    const CLONE_RESOLVE_BENEATH: u32 = 0x10;
    let name = std::ffi::CString::new(destination_name.as_bytes())
        .map_err(|_| MaterializationError::InvalidPrivateRoot)?;
    let result = unsafe {
        libc::fclonefileat(
            source.as_raw_fd(),
            destination_parent.as_raw_fd(),
            name.as_ptr(),
            CLONE_NOOWNERCOPY | CLONE_NOFOLLOW_ANY | CLONE_RESOLVE_BENEATH,
        )
    };
    if result == 0 {
        return Ok(Some(()));
    }
    let error = io::Error::last_os_error();
    match error.raw_os_error() {
        Some(libc::ENOTSUP) | Some(libc::EXDEV) | Some(libc::EINVAL) => Ok(None),
        _ => Err(MaterializationError::Clone(error)),
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn try_clone_open_file(
    _source: &File,
    _destination_parent: &File,
    _destination_name: &OsStr,
) -> Result<Option<()>, MaterializationError> {
    Ok(None)
}

#[cfg(not(unix))]
fn try_clone_open_file(
    _source: &File,
    _destination: &Path,
) -> Result<Option<()>, MaterializationError> {
    Ok(None)
}

#[cfg(unix)]
fn normalize_destination_open_file(file: &File, mode: u32) -> Result<(), MaterializationError> {
    #[cfg(target_os = "macos")]
    clear_macos_extended_attributes_open_file(file)?;
    if unsafe { libc::fchmod(file.as_raw_fd(), (mode & 0o777) as libc::mode_t) } != 0 {
        return Err(MaterializationError::Io(io::Error::last_os_error()));
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn clear_macos_extended_attributes_open_file(file: &File) -> Result<(), MaterializationError> {
    let size = unsafe { libc::flistxattr(file.as_raw_fd(), std::ptr::null_mut(), 0, 0) };
    if size < 0 {
        return Err(MaterializationError::Io(io::Error::last_os_error()));
    }
    if size == 0 {
        return Ok(());
    }
    let mut names = vec![0_u8; size as usize];
    let read =
        unsafe { libc::flistxattr(file.as_raw_fd(), names.as_mut_ptr().cast(), names.len(), 0) };
    if read < 0 {
        return Err(MaterializationError::Io(io::Error::last_os_error()));
    }
    for name in names[..read as usize]
        .split(|byte| *byte == 0)
        .filter(|name| !name.is_empty())
    {
        let name =
            std::ffi::CString::new(name).map_err(|_| MaterializationError::InvalidPrivateRoot)?;
        if unsafe { libc::fremovexattr(file.as_raw_fd(), name.as_ptr(), 0) } != 0 {
            return Err(MaterializationError::Io(io::Error::last_os_error()));
        }
    }
    Ok(())
}

#[cfg(not(unix))]
fn normalize_destination_file(path: &Path, mode: u32) -> Result<(), MaterializationError> {
    #[cfg(unix)]
    fs::set_permissions(path, unix_permissions(mode & 0o777)).map_err(MaterializationError::Io)?;
    Ok(())
}

#[cfg(not(unix))]
fn hash_destination_file(
    path: &Path,
    expected_size: u64,
) -> Result<[u8; 32], MaterializationError> {
    let metadata = fs::symlink_metadata(path).map_err(MaterializationError::Io)?;
    if !metadata.is_file() || metadata.len() != expected_size {
        return Err(MaterializationError::DestinationMismatch);
    }
    let identity = file_identity(&metadata);
    hash_regular_file(path, identity)
}

#[cfg(unix)]
fn create_symlink_at(
    target: &Path,
    destination_parent: &File,
    destination_name: &OsStr,
) -> Result<(), MaterializationError> {
    let target = std::ffi::CString::new(target.as_os_str().as_bytes())
        .map_err(|_| MaterializationError::UnsafeSymlink)?;
    let destination_name = c_name(destination_name)?;
    if unsafe {
        libc::symlinkat(
            target.as_ptr(),
            destination_parent.as_raw_fd(),
            destination_name.as_ptr(),
        )
    } != 0
    {
        return Err(MaterializationError::Io(io::Error::last_os_error()));
    }
    Ok(())
}

#[cfg(windows)]
fn create_symlink(_target: &Path, _destination: &Path) -> Result<(), MaterializationError> {
    Err(MaterializationError::UnsafeSymlink)
}

#[cfg(unix)]
fn available_space_open(directory: &File) -> Result<Option<u64>, MaterializationError> {
    let mut stats = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    if unsafe { libc::fstatvfs(directory.as_raw_fd(), stats.as_mut_ptr()) } != 0 {
        return Err(MaterializationError::Io(io::Error::last_os_error()));
    }
    let stats = unsafe { stats.assume_init() };
    let available_blocks = nonnegative_u64(stats.f_bavail)?;
    let fragment_size: u64 = stats.f_frsize;
    Ok(available_blocks.checked_mul(fragment_size))
}

fn validate_sha256_value(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

#[cfg(not(unix))]
fn make_owned_tree_writable(root: &Path) -> Result<(), MaterializationError> {
    let metadata = match fs::symlink_metadata(root) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(MaterializationError::Io(error)),
    };
    if metadata.file_type().is_symlink() {
        return Ok(());
    }
    if metadata.is_dir() {
        #[cfg(unix)]
        fs::set_permissions(root, unix_permissions(0o700)).map_err(MaterializationError::Io)?;
        for child in fs::read_dir(root).map_err(MaterializationError::Io)? {
            make_owned_tree_writable(&child.map_err(MaterializationError::Io)?.path())?;
        }
    } else if metadata.is_file() {
        #[cfg(unix)]
        fs::set_permissions(root, unix_permissions(0o600)).map_err(MaterializationError::Io)?;
    }
    Ok(())
}

#[cfg(unix)]
fn path_bytes(path: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    path.as_os_str().as_bytes().to_vec()
}

#[cfg(windows)]
fn path_bytes(path: &Path) -> Vec<u8> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str()
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect()
}

#[cfg(unix)]
fn os_str_bytes(value: &OsStr) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    value.as_bytes().to_vec()
}

#[cfg(windows)]
fn os_str_bytes(value: &OsStr) -> Vec<u8> {
    use std::os::windows::ffi::OsStrExt;
    value.encode_wide().flat_map(u16::to_le_bytes).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    use std::time::Duration;

    fn test_policy(symlinks: SymlinkPolicy) -> MaterializationPolicy {
        MaterializationPolicy {
            symlinks,
            max_entries: 128,
            max_logical_bytes: 1024 * 1024,
            min_free_bytes: 0,
            max_depth: 16,
        }
    }

    fn roots() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let temp = tempfile::tempdir().unwrap();
        let base = fs::canonicalize(temp.path()).unwrap();
        let source = base.join("source");
        let private = base.join("private");
        fs::create_dir(&source).unwrap();
        (temp, source, private)
    }

    fn recovery_artifact(
        temp: &tempfile::TempDir,
        workspace: &DisposableWorkspace,
    ) -> VerifiedRecoveryArtifact {
        let path = fs::canonicalize(temp.path())
            .unwrap()
            .join(format!("recovery-{}.artifact", workspace.workspace_id()));
        fs::write(&path, b"complete recovery artifact for the test workspace").unwrap();
        #[cfg(unix)]
        fs::set_permissions(&path, unix_permissions(0o600)).unwrap();
        workspace.verify_recovery_artifact(&path).unwrap()
    }

    #[cfg(unix)]
    fn cleanup_hook_test_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn workspace_materialization_copy_is_manifest_exact_and_cleanup_is_explicit() {
        let (temp, source, private) = roots();
        fs::create_dir(source.join("src")).unwrap();
        fs::write(source.join("src/main.rs"), "fn main() {}\n").unwrap();
        fs::write(source.join("README.md"), "hello\n").unwrap();
        #[cfg(unix)]
        fs::set_permissions(source.join("src/main.rs"), unix_permissions(0o755)).unwrap();

        let workspace = materialize_workspace(
            &source,
            &private,
            "copy",
            &test_policy(SymlinkPolicy::RejectAll),
            ClonePreference::CopyOnly,
        )
        .unwrap();
        assert_eq!(workspace.receipt.mode, MaterializationMode::Copied);
        assert_eq!(
            workspace.receipt.source_manifest_sha256,
            workspace.receipt.destination_manifest_sha256
        );
        assert_eq!(
            fs::read_to_string(workspace.path().join("README.md")).unwrap(),
            "hello\n"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            assert_eq!(
                fs::metadata(workspace.path().join("src/main.rs"))
                    .unwrap()
                    .mode()
                    & 0o777,
                0o755
            );
        }
        let path = workspace.path().to_path_buf();
        drop(workspace);
        assert!(path.exists(), "published workspaces survive handle drop");

        let cleanup_workspace = materialize_workspace(
            &source,
            &private,
            "cleanup",
            &test_policy(SymlinkPolicy::RejectAll),
            ClonePreference::CopyOnly,
        )
        .unwrap();
        let cleanup_path = cleanup_workspace.path().to_path_buf();
        let recovery = recovery_artifact(&temp, &cleanup_workspace);
        cleanup_workspace.cleanup_after_export(recovery).unwrap();
        assert!(!cleanup_path.exists());
    }

    #[test]
    fn workspace_materialization_excludes_git_runtime_secrets_and_nested_repositories() {
        let (_temp, source, private) = roots();
        fs::write(source.join("kept.txt"), "kept").unwrap();
        fs::write(source.join(".gitignore"), "target\n").unwrap();
        fs::write(source.join(".gitmodules"), "[submodule]\n").unwrap();
        fs::create_dir(source.join(".git")).unwrap();
        fs::write(source.join(".git/canary"), "secret").unwrap();
        fs::create_dir(source.join("upper")).unwrap();
        fs::write(source.join("upper/.GIT"), "pointer").unwrap();
        fs::write(source.join("upper/canary"), "upper secret").unwrap();
        fs::create_dir(source.join("nested")).unwrap();
        fs::write(source.join("nested/.git"), "gitdir: elsewhere").unwrap();
        fs::write(source.join("nested/canary"), "nested secret").unwrap();
        fs::write(source.join(".env.local"), "TOKEN=secret").unwrap();
        fs::create_dir(source.join("target")).unwrap();
        fs::write(source.join("target/build"), "build").unwrap();

        let workspace = materialize_workspace(
            &source,
            &private,
            "excluded",
            &test_policy(SymlinkPolicy::RejectAll),
            ClonePreference::CopyOnly,
        )
        .unwrap();
        assert!(workspace.path().join("kept.txt").is_file());
        assert!(workspace.path().join(".gitignore").is_file());
        assert!(workspace.path().join(".gitmodules").is_file());
        assert!(!workspace.path().join(".git").exists());
        assert!(!workspace.path().join("upper").exists());
        assert!(!workspace.path().join("nested").exists());
        assert!(!workspace.path().join(".env.local").exists());
        assert!(!workspace.path().join("target").exists());
    }

    #[cfg(unix)]
    #[test]
    fn workspace_materialization_symlinks_are_never_dereferenced_and_policy_is_closed() {
        let (_temp, source, private) = roots();
        fs::write(source.join("file"), "value").unwrap();
        std::os::unix::fs::symlink("file", source.join("link")).unwrap();
        assert!(matches!(
            materialize_workspace(
                &source,
                &private,
                "reject",
                &test_policy(SymlinkPolicy::RejectAll),
                ClonePreference::CopyOnly
            ),
            Err(MaterializationError::SymlinkRejected)
        ));
        assert!(fs::read_dir(&private).unwrap().next().is_none());

        let workspace = materialize_workspace(
            &source,
            &private,
            "safe",
            &test_policy(SymlinkPolicy::SafeRelative),
            ClonePreference::CopyOnly,
        )
        .unwrap();
        assert_eq!(
            fs::read_link(workspace.path().join("link")).unwrap(),
            Path::new("file")
        );

        let (_temp, source, private) = roots();
        fs::write(source.join("file"), "value").unwrap();
        std::os::unix::fs::symlink("../outside", source.join("escape")).unwrap();
        assert!(matches!(
            materialize_workspace(
                &source,
                &private,
                "unsafe",
                &test_policy(SymlinkPolicy::SafeRelative),
                ClonePreference::CopyOnly
            ),
            Err(MaterializationError::UnsafeSymlink)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn workspace_materialization_rejects_fifo_promptly_and_cleans_staging() {
        use std::os::unix::ffi::OsStrExt;
        let (_temp, source, private) = roots();
        fs::write(source.join("file"), "value").unwrap();
        let fifo = source.join("pipe");
        let fifo_c = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(fifo_c.as_ptr(), 0o600) }, 0);
        let started = std::time::Instant::now();
        assert!(matches!(
            materialize_workspace(
                &source,
                &private,
                "fifo",
                &test_policy(SymlinkPolicy::RejectAll),
                ClonePreference::CopyOnly
            ),
            Err(MaterializationError::SpecialFile)
        ));
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(fs::read_dir(&private).unwrap().next().is_none());
    }

    #[test]
    fn workspace_materialization_detects_source_change_before_publication() {
        let (_temp, source, private) = roots();
        fs::write(source.join("file"), "before").unwrap();
        let result = materialize_workspace_with_hook(
            &source,
            &private,
            "race",
            &test_policy(SymlinkPolicy::RejectAll),
            ClonePreference::CopyOnly,
            || fs::write(source.join("file"), "after!").map_err(MaterializationError::Io),
        );
        assert!(matches!(result, Err(MaterializationError::SourceChanged)));
        assert!(fs::read_dir(&private).unwrap().next().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn workspace_materialization_holds_source_descriptor_across_root_swap() {
        let (temp, source, private) = roots();
        fs::write(source.join("original"), "owned source").unwrap();
        let external = temp.path().join("external");
        fs::create_dir(&external).unwrap();
        fs::write(external.join("external-secret"), "do not copy").unwrap();
        let displaced = temp.path().join("displaced-source");

        let workspace = materialize_workspace_with_hook(
            &source,
            &private,
            "source-swap",
            &test_policy(SymlinkPolicy::RejectAll),
            ClonePreference::CopyOnly,
            || {
                fs::rename(&source, &displaced).map_err(MaterializationError::Io)?;
                std::os::unix::fs::symlink(&external, &source).map_err(MaterializationError::Io)
            },
        )
        .unwrap();

        assert_eq!(
            fs::read_to_string(workspace.path().join("original")).unwrap(),
            "owned source"
        );
        assert!(!workspace.path().join("external-secret").exists());
        assert_eq!(
            fs::read_to_string(external.join("external-secret")).unwrap(),
            "do not copy"
        );
    }

    #[cfg(unix)]
    #[test]
    fn descriptor_destination_and_staging_cleanup_ignore_swapped_path() {
        use std::os::unix::fs::MetadataExt;

        let (temp, source, private) = roots();
        fs::write(source.join("file"), "source bytes").unwrap();
        fs::create_dir(&private).unwrap();
        fs::set_permissions(&private, unix_permissions(0o700)).unwrap();
        let scanned = scan_tree(
            &source,
            &test_policy(SymlinkPolicy::RejectAll),
            ScanPurpose::Source,
        )
        .unwrap();
        let root = open_directory_path(&private).unwrap();
        let staging_name = OsString::from(".staging-test");
        let staging = create_directory_at(&root, &staging_name).unwrap();
        let staging_identity = directory_identity(&staging).unwrap();
        let guard = StagingGuard::new(&root, staging_name.clone(), staging_identity).unwrap();
        let displaced = private.join("held-staging");
        fs::rename(private.join(&staging_name), &displaced).unwrap();

        let external = temp.path().join("external-destination");
        fs::create_dir(&external).unwrap();
        fs::write(external.join("sentinel"), "unchanged").unwrap();
        fs::set_permissions(&external, unix_permissions(0o500)).unwrap();
        let external_mode = fs::metadata(&external).unwrap().mode() & 0o777;
        std::os::unix::fs::symlink(&external, private.join(&staging_name)).unwrap();

        populate_destination_at(
            &staging,
            &scanned,
            &test_policy(SymlinkPolicy::RejectAll),
            ClonePreference::CopyOnly,
        )
        .unwrap();
        drop(guard);

        assert_eq!(
            fs::read_to_string(displaced.join("file")).unwrap(),
            "source bytes"
        );
        assert_eq!(
            fs::read_to_string(external.join("sentinel")).unwrap(),
            "unchanged"
        );
        assert!(!external.join("file").exists());
        assert_eq!(
            fs::metadata(&external).unwrap().mode() & 0o777,
            external_mode
        );
    }

    #[test]
    fn workspace_materialization_enforces_entry_byte_depth_and_space_limits() {
        let (_temp, source, private) = roots();
        fs::write(source.join("one"), "1234").unwrap();
        fs::write(source.join("two"), "5678").unwrap();
        let mut policy = test_policy(SymlinkPolicy::RejectAll);
        policy.max_entries = 1;
        assert!(matches!(
            materialize_workspace(
                &source,
                &private,
                "entries",
                &policy,
                ClonePreference::CopyOnly
            ),
            Err(MaterializationError::TooManyEntries)
        ));
        policy = test_policy(SymlinkPolicy::RejectAll);
        policy.max_logical_bytes = 7;
        assert!(matches!(
            materialize_workspace(
                &source,
                &private,
                "bytes",
                &policy,
                ClonePreference::CopyOnly
            ),
            Err(MaterializationError::TooManyBytes)
        ));
        policy = test_policy(SymlinkPolicy::RejectAll);
        policy.min_free_bytes = u64::MAX;
        assert!(matches!(
            materialize_workspace(
                &source,
                &private,
                "space",
                &policy,
                ClonePreference::CopyOnly
            ),
            Err(MaterializationError::TooManyBytes | MaterializationError::InsufficientSpace)
        ));

        let (_temp, source, private) = roots();
        fs::create_dir(source.join("a")).unwrap();
        fs::create_dir(source.join("a/b")).unwrap();
        fs::write(source.join("a/b/file"), "x").unwrap();
        policy = test_policy(SymlinkPolicy::RejectAll);
        policy.max_depth = 1;
        assert!(matches!(
            materialize_workspace(
                &source,
                &private,
                "depth",
                &policy,
                ClonePreference::CopyOnly
            ),
            Err(MaterializationError::TooDeep)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn workspace_materialization_bounds_excluded_and_nested_marker_observations() {
        let (_temp, source, private) = roots();
        fs::write(source.join("kept"), "x").unwrap();
        for index in 0..4 {
            fs::write(source.join(format!(".env.{index}")), "excluded").unwrap();
        }
        let mut policy = test_policy(SymlinkPolicy::RejectAll);
        policy.max_entries = 3;
        assert!(matches!(
            materialize_workspace(
                &source,
                &private,
                "excluded-bound",
                &policy,
                ClonePreference::CopyOnly
            ),
            Err(MaterializationError::TooManyEntries)
        ));

        let (_temp, source, private) = roots();
        fs::write(source.join("kept"), "x").unwrap();
        fs::create_dir(source.join("nested")).unwrap();
        fs::write(source.join("nested/.git"), "gitdir: elsewhere").unwrap();
        fs::write(source.join("nested/one"), "1").unwrap();
        policy.max_entries = 2;
        assert!(matches!(
            materialize_workspace(
                &source,
                &private,
                "marker-bound",
                &policy,
                ClonePreference::CopyOnly
            ),
            Err(MaterializationError::TooManyEntries)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn byte_reservations_are_per_filesystem_checked_and_released() {
        let mut reservations = BTreeMap::new();
        reserve_bytes(&mut reservations, 1, 60, 100).unwrap();
        assert!(matches!(
            reserve_bytes(&mut reservations, 1, 50, 100),
            Err(MaterializationError::InsufficientSpace)
        ));
        assert!(matches!(
            reserve_bytes(&mut reservations, 1, u64::MAX, u64::MAX),
            Err(MaterializationError::InsufficientSpace)
        ));
        reservations.remove(&1);
        reserve_bytes(&mut reservations, 1, 100, 100).unwrap();
        reserve_bytes(&mut reservations, 2, 100, 100).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn scheduled_allowlist_is_normalized_exact_and_rejects_unlisted_content() {
        let (_temp, source, private) = roots();
        fs::create_dir(source.join("src")).unwrap();
        fs::write(source.join("src/main.rs"), "fn main() {}\n").unwrap();
        fs::write(source.join("README.md"), "readme").unwrap();
        let tracked =
            ScheduledTrackedAllowlist::new([PathBuf::from("src"), PathBuf::from("src/main.rs")])
                .unwrap();
        assert!(matches!(
            materialize_scheduled_workspace(
                &source,
                &private,
                "scheduled",
                &test_policy(SymlinkPolicy::SafeRelative),
                ClonePreference::CopyOnly,
                &tracked,
            ),
            Err(MaterializationError::AllowlistMismatch)
        ));
        fs::remove_file(source.join("README.md")).unwrap();
        let workspace = materialize_scheduled_workspace(
            &source,
            &private,
            "scheduled",
            &test_policy(SymlinkPolicy::SafeRelative),
            ClonePreference::CopyOnly,
            &tracked,
        )
        .unwrap();
        assert!(workspace.path().join("src/main.rs").is_file());
        assert!(matches!(
            ScheduledTrackedAllowlist::new([PathBuf::from("src/../secret")]),
            Err(MaterializationError::InvalidAllowlistEntry)
        ));
        let absent = ScheduledTrackedAllowlist::new([PathBuf::from("missing")]).unwrap();
        assert!(matches!(
            materialize_scheduled_workspace(
                &source,
                &private,
                "absent",
                &test_policy(SymlinkPolicy::SafeRelative),
                ClonePreference::CopyOnly,
                &absent,
            ),
            Err(MaterializationError::AllowlistMismatch)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn regular_hardlinks_and_invalid_safe_symlink_graphs_are_rejected() {
        let (_temp, source, private) = roots();
        fs::write(source.join("file"), "value").unwrap();
        fs::hard_link(source.join("file"), source.join("alias")).unwrap();
        assert!(matches!(
            materialize_workspace(
                &source,
                &private,
                "hardlink",
                &test_policy(SymlinkPolicy::RejectAll),
                ClonePreference::CopyOnly
            ),
            Err(MaterializationError::HardLinkRejected)
        ));

        for (label, left, right) in [
            ("missing", "missing", None),
            ("excluded", ".env", None),
            ("cycle", "b", Some(("b", "a"))),
        ] {
            let (_temp, source, private) = roots();
            fs::write(source.join("kept"), "value").unwrap();
            if label == "excluded" {
                fs::write(source.join(".env"), "secret").unwrap();
            }
            std::os::unix::fs::symlink(left, source.join("a")).unwrap();
            if let Some((name, target)) = right {
                std::os::unix::fs::symlink(target, source.join(name)).unwrap();
            }
            assert!(matches!(
                materialize_workspace(
                    &source,
                    &private,
                    label,
                    &test_policy(SymlinkPolicy::SafeRelative),
                    ClonePreference::CopyOnly
                ),
                Err(MaterializationError::InvalidSymlinkTarget)
            ));
        }
    }

    #[test]
    fn workspace_materialization_rejects_overlapping_roots_and_empty_sources() {
        let (_temp, source, _private) = roots();
        let private = source.join("private");
        assert!(matches!(
            materialize_workspace(
                &source,
                &private,
                "overlap",
                &test_policy(SymlinkPolicy::RejectAll),
                ClonePreference::CopyOnly
            ),
            Err(MaterializationError::OverlappingRoots)
        ));
        let (_temp, source, private) = roots();
        assert!(matches!(
            materialize_workspace(
                &source,
                &private,
                "empty",
                &test_policy(SymlinkPolicy::RejectAll),
                ClonePreference::CopyOnly
            ),
            Err(MaterializationError::NoRegularFiles)
        ));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn workspace_materialization_prefers_real_apfs_clones_when_available() {
        use std::os::unix::ffi::OsStrExt;

        let (_temp, source, private) = roots();
        fs::write(source.join("file"), vec![7_u8; 4096]).unwrap();
        let workspace = materialize_workspace(
            &source,
            &private,
            "clone",
            &test_policy(SymlinkPolicy::RejectAll),
            ClonePreference::PreferClone,
        )
        .unwrap();
        let source_c = std::ffi::CString::new(source.as_os_str().as_bytes()).unwrap();
        let mut stats = std::mem::MaybeUninit::<libc::statfs>::uninit();
        assert_eq!(
            unsafe { libc::statfs(source_c.as_ptr(), stats.as_mut_ptr()) },
            0
        );
        let stats = unsafe { stats.assume_init() };
        let filesystem = unsafe { std::ffi::CStr::from_ptr(stats.f_fstypename.as_ptr()) };
        if filesystem.to_bytes() == b"apfs" {
            assert_eq!(workspace.receipt.mode, MaterializationMode::AllCloned);
        } else {
            assert_eq!(workspace.receipt.mode, MaterializationMode::Copied);
        }
        fs::write(workspace.path().join("file"), "changed").unwrap();
        assert_eq!(fs::read(source.join("file")).unwrap(), vec![7_u8; 4096]);
    }

    #[test]
    fn workspace_materialization_debug_and_cleanup_do_not_expose_or_accept_broad_paths() {
        let (temp, source, private) = roots();
        fs::write(source.join("secret-name"), "value").unwrap();
        let workspace = materialize_workspace(
            &source,
            &private,
            "debug",
            &test_policy(SymlinkPolicy::RejectAll),
            ClonePreference::CopyOnly,
        )
        .unwrap();
        let debug = format!("{workspace:?}");
        assert!(!debug.contains("secret-name"));
        assert!(!debug.contains(private.to_string_lossy().as_ref()));
        let empty_artifact = temp.path().join("empty-artifact");
        fs::write(&empty_artifact, []).unwrap();
        #[cfg(unix)]
        fs::set_permissions(&empty_artifact, unix_permissions(0o600)).unwrap();
        assert!(matches!(
            workspace.verify_recovery_artifact(&empty_artifact),
            Err(MaterializationError::InvalidRecoveryArtifact)
        ));
        drop(workspace);

        let workspace = materialize_workspace(
            &source,
            &private,
            "substitution",
            &test_policy(SymlinkPolicy::RejectAll),
            ClonePreference::CopyOnly,
        )
        .unwrap();
        let recovery = recovery_artifact(&temp, &workspace);
        let published = workspace.path().to_path_buf();
        let displaced = private.join("displaced-workspace");
        fs::rename(&published, &displaced).unwrap();
        fs::create_dir(&published).unwrap();
        let failure = workspace.cleanup_after_export(recovery).unwrap_err();
        assert!(matches!(
            failure.error(),
            MaterializationError::UnsafeCleanup
        ));
        assert!(
            published.is_dir(),
            "a substituted directory is never deleted"
        );
    }

    #[cfg(unix)]
    #[test]
    fn recovery_capability_is_workspace_bound_and_durable_receipt_reopens() {
        let (temp, source, private) = roots();
        fs::write(source.join("file"), "baseline").unwrap();
        let first = materialize_workspace(
            &source,
            &private,
            "first",
            &test_policy(SymlinkPolicy::RejectAll),
            ClonePreference::CopyOnly,
        )
        .unwrap();
        let second = materialize_workspace(
            &source,
            &private,
            "second",
            &test_policy(SymlinkPolicy::RejectAll),
            ClonePreference::CopyOnly,
        )
        .unwrap();
        let first_id = first.workspace_id();
        let first_path = first.path().to_path_buf();
        let first_recovery = recovery_artifact(&temp, &first);
        assert_eq!(
            first_recovery.final_manifest_sha256(),
            first.receipt().destination_manifest_sha256
        );
        assert_eq!(first_recovery.artifact_sha256().len(), 64);
        assert!(first_recovery.artifact_size() > 0);

        let replay_failure = second.cleanup_after_export(first_recovery).unwrap_err();
        assert!(matches!(
            replay_failure.error(),
            MaterializationError::InvalidRecoveryArtifact
        ));
        let (_second, first_recovery, _) = replay_failure.into_parts();
        drop(first_recovery);
        drop(first);

        let reopened = DisposableWorkspace::reopen(&private, first_id).unwrap();
        assert_eq!(reopened.path(), first_path);
        let recovery = recovery_artifact(&temp, &reopened);
        reopened.cleanup_after_export(recovery).unwrap();
        assert!(!first_path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn recovery_artifact_is_descriptor_verified_outside_workspace_and_private_root() {
        let (temp, source, private) = roots();
        fs::write(source.join("file"), "baseline").unwrap();
        let workspace = materialize_workspace(
            &source,
            &private,
            "artifact-boundary",
            &test_policy(SymlinkPolicy::RejectAll),
            ClonePreference::CopyOnly,
        )
        .unwrap();

        let inside_workspace = workspace.path().join("recovery.artifact");
        fs::write(&inside_workspace, "inside workspace").unwrap();
        fs::set_permissions(&inside_workspace, unix_permissions(0o600)).unwrap();
        assert!(matches!(
            workspace.verify_recovery_artifact(&inside_workspace),
            Err(MaterializationError::InvalidRecoveryArtifact)
        ));
        fs::remove_file(&inside_workspace).unwrap();

        let inside_private = private.join("recovery.artifact");
        fs::write(&inside_private, "inside private root").unwrap();
        fs::set_permissions(&inside_private, unix_permissions(0o600)).unwrap();
        assert!(matches!(
            workspace.verify_recovery_artifact(&inside_private),
            Err(MaterializationError::InvalidRecoveryArtifact)
        ));
        fs::remove_file(&inside_private).unwrap();

        let base = fs::canonicalize(temp.path()).unwrap();
        let moved_private = base.join("moved-private");
        fs::rename(&private, &moved_private).unwrap();
        let moved_inside = moved_private.join("moved-recovery.artifact");
        fs::write(&moved_inside, "inside moved private root").unwrap();
        fs::set_permissions(&moved_inside, unix_permissions(0o600)).unwrap();
        assert!(matches!(
            workspace.verify_recovery_artifact(&moved_inside),
            Err(MaterializationError::InvalidRecoveryArtifact)
        ));

        let external = base.join("external-recovery.artifact");
        fs::write(&external, "complete external recovery artifact").unwrap();
        fs::set_permissions(&external, unix_permissions(0o600)).unwrap();
        assert!(workspace.verify_recovery_artifact(&external).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn cleanup_child_swap_is_rejected_before_permission_or_unlink_action() {
        use std::os::unix::fs::MetadataExt;

        let _test_lock = cleanup_hook_test_lock().lock().unwrap();
        let (temp, source, private) = roots();
        fs::create_dir(source.join("victim")).unwrap();
        fs::write(source.join("victim/owned"), "owned").unwrap();
        let workspace = materialize_workspace(
            &source,
            &private,
            "child-swap",
            &test_policy(SymlinkPolicy::RejectAll),
            ClonePreference::CopyOnly,
        )
        .unwrap();
        let workspace_id = workspace.workspace_id();
        let workspace_identity = DirectoryIdentity {
            device: workspace.workspace_device,
            inode: workspace.workspace_inode,
        };
        let recovery = recovery_artifact(&temp, &workspace);
        let deleting = private.join(format!(".deleting-{workspace_id}"));
        let swapped = Arc::new(AtomicBool::new(false));
        let swapped_for_hook = Arc::clone(&swapped);
        *cleanup_entry_hook().lock().unwrap() = Some(Box::new(move |parent, name, _| {
            if name == OsStr::new("victim")
                && directory_identity(parent)? == workspace_identity
                && !swapped_for_hook.load(Ordering::SeqCst)
            {
                let replacement = create_directory_at(parent, OsStr::new("replacement-victim"))?;
                let mut sentinel = create_regular_at(&replacement, OsStr::new("sentinel"), 0o600)?;
                sentinel
                    .write_all(b"preserve")
                    .map_err(MaterializationError::Io)?;
                if unsafe { libc::fchmod(replacement.as_raw_fd(), 0o500) } != 0 {
                    return Err(MaterializationError::Io(io::Error::last_os_error()));
                }
                rename_entry_at(parent, OsStr::new("victim"), OsStr::new("held-victim"))?;
                rename_entry_at(
                    parent,
                    OsStr::new("replacement-victim"),
                    OsStr::new("victim"),
                )?;
                swapped_for_hook.store(true, Ordering::SeqCst);
            }
            Ok(())
        }));

        let failure = workspace.cleanup_after_export(recovery).unwrap_err();
        *cleanup_entry_hook().lock().unwrap() = None;
        assert!(swapped.load(Ordering::SeqCst));
        assert!(
            matches!(failure.error(), MaterializationError::UnsafeCleanup),
            "unexpected cleanup failure: {:?}",
            failure.error()
        );
        assert_eq!(
            fs::metadata(deleting.join("victim")).unwrap().mode() & 0o777,
            0o500
        );
        assert_eq!(
            fs::read_to_string(deleting.join("victim/sentinel")).unwrap(),
            "preserve"
        );
    }

    #[cfg(unix)]
    #[test]
    fn authorized_partial_cleanup_reopens_and_resumes_without_manifest_replay() {
        let _test_lock = cleanup_hook_test_lock().lock().unwrap();
        let (temp, source, private) = roots();
        fs::write(source.join("a"), "first").unwrap();
        fs::write(source.join("b"), "second").unwrap();
        let workspace = materialize_workspace(
            &source,
            &private,
            "partial-resume",
            &test_policy(SymlinkPolicy::RejectAll),
            ClonePreference::CopyOnly,
        )
        .unwrap();
        let workspace_id = workspace.workspace_id();
        let workspace_identity = DirectoryIdentity {
            device: workspace.workspace_device,
            inode: workspace.workspace_inode,
        };
        let artifact_path = fs::canonicalize(temp.path())
            .unwrap()
            .join("partial-recovery.artifact");
        fs::write(&artifact_path, "complete partial recovery artifact").unwrap();
        fs::set_permissions(&artifact_path, unix_permissions(0o600)).unwrap();
        let recovery = workspace.verify_recovery_artifact(&artifact_path).unwrap();
        let deleting = private.join(format!(".deleting-{workspace_id}"));
        let swapped = Arc::new(AtomicBool::new(false));
        let swapped_for_hook = Arc::clone(&swapped);
        *cleanup_entry_hook().lock().unwrap() = Some(Box::new(move |parent, name, _| {
            if name == OsStr::new("b")
                && directory_identity(parent)? == workspace_identity
                && !swapped_for_hook.load(Ordering::SeqCst)
            {
                let mut replacement =
                    create_regular_at(parent, OsStr::new("replacement-b"), 0o600)?;
                replacement
                    .write_all(b"replacement")
                    .map_err(MaterializationError::Io)?;
                replacement.sync_all().map_err(MaterializationError::Io)?;
                rename_entry_at(parent, OsStr::new("b"), OsStr::new("held-b"))?;
                rename_entry_at(parent, OsStr::new("replacement-b"), OsStr::new("b"))?;
                swapped_for_hook.store(true, Ordering::SeqCst);
            }
            Ok(())
        }));

        let failure = workspace.cleanup_after_export(recovery).unwrap_err();
        *cleanup_entry_hook().lock().unwrap() = None;
        assert!(swapped.load(Ordering::SeqCst));
        assert!(!deleting.join("a").exists());
        assert!(deleting.join("b").exists());
        let root = open_directory_path(&private).unwrap();
        assert!(load_durable_receipt(&root, workspace_id)
            .unwrap()
            .cleanup_authorization
            .is_some());
        let (failed_workspace, failed_recovery, _) = failure.into_parts();
        drop(failed_workspace);
        drop(failed_recovery);

        fs::remove_file(deleting.join("b")).unwrap();
        fs::rename(deleting.join("held-b"), deleting.join("b")).unwrap();
        let reopened = DisposableWorkspace::reopen(&private, workspace_id).unwrap();
        let recovery = reopened.verify_recovery_artifact(&artifact_path).unwrap();
        reopened.cleanup_after_export(recovery).unwrap();
        assert!(!deleting.exists());
        assert!(!private
            .join(RECEIPT_DIRECTORY)
            .join(receipt_file_name(workspace_id))
            .exists());
    }

    #[cfg(unix)]
    #[test]
    fn root_descriptors_remain_stable_and_symlinked_ancestors_are_rejected() {
        let (temp, source, private) = roots();
        fs::write(source.join("file"), "held source").unwrap();
        let base = fs::canonicalize(temp.path()).unwrap();
        let displaced_private = base.join("displaced-private");
        let private_for_hook = private.clone();
        let displaced_for_hook = displaced_private.clone();
        let workspace = materialize_workspace_with_hook(
            &source,
            &private,
            "private-swap",
            &test_policy(SymlinkPolicy::RejectAll),
            ClonePreference::CopyOnly,
            move || {
                fs::rename(&private_for_hook, &displaced_for_hook)
                    .map_err(MaterializationError::Io)?;
                fs::create_dir(&private_for_hook).map_err(MaterializationError::Io)?;
                fs::set_permissions(&private_for_hook, unix_permissions(0o700))
                    .map_err(MaterializationError::Io)?;
                fs::write(private_for_hook.join("sentinel"), "external replacement")
                    .map_err(MaterializationError::Io)
            },
        )
        .unwrap();
        let published_name = workspace.path().file_name().unwrap();
        assert!(displaced_private.join(published_name).is_dir());
        assert_eq!(
            fs::read_to_string(private.join("sentinel")).unwrap(),
            "external replacement"
        );
        assert_eq!(fs::read_dir(&private).unwrap().count(), 1);

        let real_parent = base.join("real-parent");
        fs::create_dir(&real_parent).unwrap();
        let real_source = real_parent.join("source");
        fs::create_dir(&real_source).unwrap();
        fs::write(real_source.join("file"), "value").unwrap();
        let source_alias = base.join("source-alias");
        std::os::unix::fs::symlink(&real_parent, &source_alias).unwrap();
        assert!(matches!(
            materialize_workspace(
                &source_alias.join("source"),
                &base.join("other-private"),
                "source-alias",
                &test_policy(SymlinkPolicy::RejectAll),
                ClonePreference::CopyOnly,
            ),
            Err(MaterializationError::InvalidSource)
        ));

        let private_parent = base.join("private-parent");
        fs::create_dir(&private_parent).unwrap();
        let private_alias = base.join("private-alias");
        std::os::unix::fs::symlink(&private_parent, &private_alias).unwrap();
        assert!(matches!(
            materialize_workspace(
                &real_source,
                &private_alias.join("root"),
                "private-alias",
                &test_policy(SymlinkPolicy::RejectAll),
                ClonePreference::CopyOnly,
            ),
            Err(MaterializationError::InvalidPrivateRoot)
        ));
        assert!(!private_parent.join("root").exists());
    }

    #[cfg(unix)]
    #[test]
    fn cleanup_failure_returns_retryable_ownership_and_preserves_receipt() {
        let (temp, source, private) = roots();
        fs::write(source.join("file"), "final").unwrap();
        let workspace = materialize_workspace(
            &source,
            &private,
            "retry",
            &test_policy(SymlinkPolicy::RejectAll),
            ClonePreference::CopyOnly,
        )
        .unwrap();
        let workspace_id = workspace.workspace_id();
        let recovery = recovery_artifact(&temp, &workspace);
        let receipt_directory = private.join(RECEIPT_DIRECTORY);
        fs::set_permissions(&receipt_directory, unix_permissions(0o500)).unwrap();

        let failure = workspace.cleanup_after_export(recovery).unwrap_err();
        assert!(receipt_directory
            .join(receipt_file_name(workspace_id))
            .is_file());
        fs::set_permissions(&receipt_directory, unix_permissions(0o700)).unwrap();
        failure.retry().unwrap();
        assert!(!receipt_directory
            .join(receipt_file_name(workspace_id))
            .exists());
    }

    #[cfg(unix)]
    #[test]
    fn cleanup_never_follows_swapped_workspace_symlink_or_changes_external_permissions() {
        use std::os::unix::fs::MetadataExt;

        let (temp, source, private) = roots();
        fs::write(source.join("file"), "workspace").unwrap();
        let workspace = materialize_workspace(
            &source,
            &private,
            "cleanup-swap",
            &test_policy(SymlinkPolicy::RejectAll),
            ClonePreference::CopyOnly,
        )
        .unwrap();
        let recovery = recovery_artifact(&temp, &workspace);
        let published = workspace.path().to_path_buf();
        let displaced = private.join("displaced-cleanup-workspace");
        fs::rename(&published, &displaced).unwrap();
        let external = temp.path().join("external-cleanup");
        fs::create_dir(&external).unwrap();
        fs::write(external.join("sentinel"), "preserve").unwrap();
        fs::set_permissions(&external, unix_permissions(0o500)).unwrap();
        let external_mode = fs::metadata(&external).unwrap().mode() & 0o777;
        std::os::unix::fs::symlink(&external, &published).unwrap();

        let failure = workspace.cleanup_after_export(recovery).unwrap_err();
        assert!(matches!(
            failure.error(),
            MaterializationError::UnsafeCleanup
        ));
        assert_eq!(
            fs::read_to_string(external.join("sentinel")).unwrap(),
            "preserve"
        );
        assert_eq!(
            fs::metadata(&external).unwrap().mode() & 0o777,
            external_mode
        );
    }
}
