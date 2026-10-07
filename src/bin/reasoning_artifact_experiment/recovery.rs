//! Complete, bounded recovery bundles for RND-097 disposable trial workspaces.
//!
//! This is intentionally a full-tree artifact rather than a text diff.  A
//! successful tool phase may add, remove, or change executable files, and a
//! recovery proof must preserve every one of those changes before WSP is
//! allowed to remove the original workspace.  The controller retains only the
//! bundle and a small receipt; model prompts, provider bodies, and the
//! disposable trees are never written here.

use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
#[cfg(unix)]
use std::os::unix::ffi::{OsStrExt, OsStringExt};
#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use base64::engine::general_purpose::STANDARD_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::workspace_materialization::{
    materialize_workspace, ClonePreference, DisposableWorkspace, MaterializationPolicy,
    VerifiedRecoveryArtifact, DEFAULT_MAX_ENTRIES, DEFAULT_MAX_LOGICAL_BYTES,
};

const BUNDLE_MAGIC_V1: &[u8; 16] = b"CA-RND-BUNDLE-V1";
const BUNDLE_MAGIC_V2: &[u8; 16] = b"CA-RND-BUNDLE-V2";
const BUNDLE_SCHEMA_V1: &str = "ca.rnd097.recovery_bundle.v1";
const BUNDLE_SCHEMA_V2: &str = "ca.rnd097.recovery_bundle.v2";
const CHECKPOINT_SCHEMA: &str = "ca.rnd097.recovery_checkpoint.v1";
const MAX_BUNDLE_HEADER_BYTES: usize = 8 * 1024;
const MAX_ENTRY_HEADER_BYTES: usize = 16 * 1024;
const COPY_BUFFER_BYTES: usize = 64 * 1024;
const MAX_CHECKPOINT_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct BundleHeaderV1 {
    schema_version: String,
    source_manifest_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct BundleEntryV1 {
    relative_path_b64: String,
    mode: u32,
    bytes: u64,
    sha256: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum BundleEntryKindV2 {
    Directory,
    Regular,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct BundleEntryV2 {
    kind: BundleEntryKindV2,
    relative_path_b64: String,
    mode: u32,
    bytes: u64,
    sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct BundleFooterV1 {
    entries: u64,
    logical_bytes: u64,
    tree_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RecoveryBundleReceiptV1 {
    pub schema_version: String,
    pub source_manifest_sha256: String,
    pub final_manifest_sha256: String,
    pub recovery_artifact_sha256: String,
    pub recovery_artifact_size: u64,
    pub entries: u64,
    pub logical_bytes: u64,
}

/// Owner-only durable controller state written after export and before the
/// original workspace may be cleaned.  It names no secret and serializes no
/// WSP private capability: restart always reopens the WSP receipt and remints
/// a fresh capability after rehashing the external artifact.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RecoveryCheckpointV1 {
    pub schema_version: String,
    pub trial_id: String,
    pub private_root: PathBuf,
    pub source: PathBuf,
    pub workspace_id: Uuid,
    #[serde(default)]
    pub reconstructed_workspace_id: Option<Uuid>,
    pub artifact: PathBuf,
    pub bundle: RecoveryBundleReceiptV1,
}

impl std::fmt::Debug for RecoveryCheckpointV1 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RecoveryCheckpointV1")
            .field("schema_version", &self.schema_version)
            .field("trial_id", &self.trial_id)
            .field("private_root", &"[PRIVATE]")
            .field("source", &"[PRIVATE]")
            .field("workspace_id", &self.workspace_id)
            .field("artifact", &"[PRIVATE]")
            .field("bundle", &self.bundle)
            .finish()
    }
}

pub struct RecoveryCheckpointStore {
    root: PathBuf,
}

impl std::fmt::Debug for RecoveryCheckpointStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RecoveryCheckpointStore")
            .field("root", &"[PRIVATE]")
            .finish()
    }
}

impl RecoveryCheckpointStore {
    pub fn create(root: &Path) -> Result<Self> {
        require_unix()?;
        match fs::symlink_metadata(root) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                bail!("checkpoint root must not be a symbolic link")
            }
            Ok(metadata) if !metadata.is_dir() => bail!("checkpoint root is not a directory"),
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let parent = root
                    .parent()
                    .ok_or_else(|| anyhow!("checkpoint root has no parent"))?;
                require_private_directory(&parent.canonicalize()?)?;
                fs::create_dir(root)
                    .with_context(|| format!("cannot create checkpoint root {}", root.display()))?;
                #[cfg(unix)]
                fs::set_permissions(root, fs::Permissions::from_mode(0o700))?;
            }
            Err(error) => return Err(error.into()),
        }
        let root = root
            .canonicalize()
            .context("cannot canonicalize checkpoint root")?;
        require_private_directory(&root)?;
        Ok(Self { root })
    }

    pub fn persist_exported(&self, checkpoint: &RecoveryCheckpointV1) -> Result<()> {
        validate_checkpoint(checkpoint)?;
        if checkpoint.reconstructed_workspace_id.is_some() {
            bail!("an exported checkpoint cannot already name a reconstruction");
        }
        let path = self.exported_path(&checkpoint.trial_id)?;
        if path.exists() {
            if self.load_exported(&checkpoint.trial_id)? == *checkpoint {
                return Ok(());
            }
            bail!("recovery checkpoint already exists for this trial");
        }
        let bytes = serde_json::to_vec(checkpoint)?;
        if bytes.len() > MAX_CHECKPOINT_BYTES {
            bail!("recovery checkpoint exceeds the bound");
        }
        let temporary = self
            .root
            .join(format!(".{}.{}.tmp", checkpoint.trial_id, Uuid::new_v4()));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options
            .mode(0o600)
            .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
        let mut file = options.open(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        if let Err(error) = fs::hard_link(&temporary, &path) {
            let _ = fs::remove_file(&temporary);
            return Err(anyhow!("cannot publish recovery checkpoint: {error}"));
        }
        fs::remove_file(&temporary)?;
        sync_parent(&path)?;
        Ok(())
    }

    /// Durably records the reconstruction identity before the journal may
    /// advertise `recovery_proven`. Repeating the exact publication is safe;
    /// changing either workspace identity is refused.
    pub fn persist_proven(
        &self,
        checkpoint: &RecoveryCheckpointV1,
        reconstructed_workspace_id: Uuid,
    ) -> Result<RecoveryCheckpointV1> {
        let current = self.load_exported(&checkpoint.trial_id)?;
        if current != *checkpoint {
            bail!("recovery checkpoint changed before proof publication");
        }
        if let Some(existing) = current.reconstructed_workspace_id {
            if existing == reconstructed_workspace_id {
                return Ok(current);
            }
            bail!("recovery checkpoint already names another reconstruction");
        }
        let mut proven = current;
        proven.reconstructed_workspace_id = Some(reconstructed_workspace_id);
        validate_checkpoint(&proven)?;
        self.replace(&proven)?;
        Ok(proven)
    }

    pub fn load_exported(&self, trial_id: &str) -> Result<RecoveryCheckpointV1> {
        validate_trial_id(trial_id)?;
        let path = self.exported_path(trial_id)?;
        let mut file = open_regular_readonly(&path)?;
        let metadata = file.metadata()?;
        #[cfg(unix)]
        if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 {
            bail!("recovery checkpoint is not owner-only");
        }
        if metadata.len() > MAX_CHECKPOINT_BYTES as u64 {
            bail!("recovery checkpoint exceeds the bound");
        }
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        file.read_to_end(&mut bytes)?;
        let checkpoint: RecoveryCheckpointV1 = serde_json::from_slice(&bytes)
            .context("recovery checkpoint is not valid strict JSON")?;
        validate_checkpoint(&checkpoint)?;
        if checkpoint.trial_id != trial_id {
            bail!("recovery checkpoint identity does not match its file name");
        }
        Ok(checkpoint)
    }

    /// Proves that the durable checkpoint still names the exact workspace and
    /// artifact.  The returned workspace remains published; callers may run a
    /// new reconstruction proof or explicitly clean it only afterwards.
    pub fn reopen_exported(
        &self,
        checkpoint: &RecoveryCheckpointV1,
    ) -> Result<DisposableWorkspace> {
        validate_checkpoint(checkpoint)?;
        let workspace =
            DisposableWorkspace::reopen(&checkpoint.private_root, checkpoint.workspace_id)
                .map_err(|error| anyhow!("cannot reopen recovery workspace: {error}"))?;
        if workspace.receipt().source_manifest_sha256 != checkpoint.bundle.source_manifest_sha256 {
            bail!("reopened workspace baseline does not match recovery checkpoint");
        }
        let recovery = workspace
            .verify_recovery_artifact(&checkpoint.artifact)
            .map_err(|error| anyhow!("cannot reverify recovery artifact: {error}"))?;
        if recovery.artifact_sha256() != checkpoint.bundle.recovery_artifact_sha256
            || recovery.artifact_size() != checkpoint.bundle.recovery_artifact_size
            || recovery.final_manifest_sha256() != checkpoint.bundle.final_manifest_sha256
        {
            bail!("reopened recovery artifact does not match checkpoint");
        }
        drop(recovery);
        Ok(workspace)
    }

    /// Reopens both exact WSP workspaces named by a proven checkpoint and
    /// revalidates the immutable external artifact against each one.
    pub fn reopen_proven(&self, checkpoint: &RecoveryCheckpointV1) -> Result<ProvenRecoveryBundle> {
        let reconstructed_id = checkpoint
            .reconstructed_workspace_id
            .ok_or_else(|| anyhow!("recovery checkpoint has no proven reconstruction"))?;
        let original = self.reopen_exported(checkpoint)?;
        let reconstructed = DisposableWorkspace::reopen(&checkpoint.private_root, reconstructed_id)
            .map_err(|error| anyhow!("cannot reopen reconstructed workspace: {error}"))?;
        let original_recovery = verified_for_checkpoint(&original, checkpoint)?;
        let reconstructed_recovery = verified_for_checkpoint(&reconstructed, checkpoint)?;
        Ok(ProvenRecoveryBundle {
            receipt: checkpoint.bundle.clone(),
            original,
            original_recovery,
            reconstructed,
            reconstructed_recovery,
        })
    }

    /// Retries cleanup using only durable identities. Each WSP cleanup is
    /// idempotent across a crash after tree removal but before controller
    /// settlement; an absent receipt is accepted only when no workspace name
    /// ending in that exact UUID remains in the private root.
    pub fn cleanup_proven(&self, checkpoint: &RecoveryCheckpointV1) -> Result<()> {
        let reconstructed = checkpoint
            .reconstructed_workspace_id
            .ok_or_else(|| anyhow!("recovery checkpoint has no proven reconstruction"))?;
        self.cleanup_one(checkpoint, reconstructed)?;
        self.cleanup_one(checkpoint, checkpoint.workspace_id)?;
        Ok(())
    }

    fn cleanup_one(&self, checkpoint: &RecoveryCheckpointV1, workspace_id: Uuid) -> Result<()> {
        match DisposableWorkspace::reopen(&checkpoint.private_root, workspace_id) {
            Ok(workspace) => {
                let recovery = verified_for_checkpoint(&workspace, checkpoint)?;
                workspace
                    .cleanup_after_export(recovery)
                    .map_err(|error| anyhow!("cannot clean checkpoint workspace: {error}"))
            }
            Err(error) if cleanup_already_complete(&checkpoint.private_root, workspace_id)? => {
                let _ = error;
                Ok(())
            }
            Err(error) => Err(anyhow!("cannot reopen checkpoint workspace: {error}")),
        }
    }

    fn replace(&self, checkpoint: &RecoveryCheckpointV1) -> Result<()> {
        let path = self.exported_path(&checkpoint.trial_id)?;
        let bytes = serde_json::to_vec(checkpoint)?;
        if bytes.len() > MAX_CHECKPOINT_BYTES {
            bail!("recovery checkpoint exceeds the bound");
        }
        let temporary = self
            .root
            .join(format!(".{}.{}.tmp", checkpoint.trial_id, Uuid::new_v4()));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options
            .mode(0o600)
            .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
        let mut file = options.open(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, &path)?;
        sync_parent(&path)?;
        Ok(())
    }

    fn exported_path(&self, trial_id: &str) -> Result<PathBuf> {
        validate_trial_id(trial_id)?;
        Ok(self.root.join(format!("{trial_id}.exported.json")))
    }
}

#[derive(Debug, Clone)]
struct SourceEntry {
    kind: BundleEntryKindV2,
    relative: OsString,
    absolute: PathBuf,
    mode: u32,
    bytes: u64,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
}

#[derive(Debug, Clone)]
struct ParsedEntry {
    kind: BundleEntryKindV2,
    relative: OsString,
    mode: u32,
    bytes: u64,
    sha256: [u8; 32],
    data_offset: u64,
}

#[derive(Debug)]
struct ParsedBundle {
    schema_version: String,
    records_directories: bool,
    source_manifest_sha256: String,
    entries: Vec<ParsedEntry>,
    logical_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BundleVersion {
    V1,
    V2,
}

impl BundleVersion {
    fn schema(self) -> &'static str {
        match self {
            Self::V1 => BUNDLE_SCHEMA_V1,
            Self::V2 => BUNDLE_SCHEMA_V2,
        }
    }

    fn records_directories(self) -> bool {
        self == Self::V2
    }
}

/// A successful reconstruction proof retains the private WSP cleanup
/// capabilities.  It has no public path accessors: callers can either clean
/// the two workspaces after persistence or let Drop preserve them for a later
/// reopen/remint retry.
pub struct ProvenRecoveryBundle {
    receipt: RecoveryBundleReceiptV1,
    original: DisposableWorkspace,
    original_recovery: VerifiedRecoveryArtifact,
    reconstructed: DisposableWorkspace,
    reconstructed_recovery: VerifiedRecoveryArtifact,
}

impl std::fmt::Debug for ProvenRecoveryBundle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProvenRecoveryBundle")
            .field("receipt", &self.receipt)
            .finish_non_exhaustive()
    }
}

impl ProvenRecoveryBundle {
    pub fn receipt(&self) -> &RecoveryBundleReceiptV1 {
        &self.receipt
    }

    pub fn reconstructed_workspace_id(&self) -> Uuid {
        self.reconstructed.workspace_id()
    }

    /// Cleans both workspaces only after the caller has durably recorded the
    /// receipt.  A WSP cleanup failure leaves that workspace published and
    /// remintable from its durable receipt plus the external bundle.
    pub fn cleanup_after_persisting(self) -> Result<()> {
        self.reconstructed
            .cleanup_after_export(self.reconstructed_recovery)
            .map_err(|error| anyhow!("cannot clean reconstructed workspace: {error}"))?;
        self.original
            .cleanup_after_export(self.original_recovery)
            .map_err(|error| anyhow!("cannot clean original workspace: {error}"))?;
        Ok(())
    }
}

/// Streams a complete regular-file/directory workspace into an owner-only
/// artifact outside the workspace and WSP private root. It records additions,
/// deletions, file and directory modes, empty directories, and binary bytes
/// because it serializes the complete final tree, not a textual diff.
pub fn write_recovery_bundle(
    workspace: &DisposableWorkspace,
    artifact: &Path,
) -> Result<RecoveryBundleReceiptV1> {
    require_unix()?;
    ensure_artifact_is_external(workspace.path(), artifact)?;
    let mut sources = Vec::new();
    collect_sources(workspace.path(), workspace.path(), 0, &mut sources)?;
    if sources.is_empty() {
        bail!("recovery bundle refuses an empty workspace");
    }
    sources.sort_by(|left, right| os_bytes(&left.relative).cmp(os_bytes(&right.relative)));
    let logical_bytes = sources.iter().try_fold(0_u64, |total, entry| {
        total
            .checked_add(entry.bytes)
            .ok_or_else(|| anyhow!("recovery bundle logical byte count overflow"))
    })?;
    if sources.len() > DEFAULT_MAX_ENTRIES || logical_bytes > DEFAULT_MAX_LOGICAL_BYTES {
        bail!("recovery bundle exceeds the materialized-workspace bounds");
    }

    let source_manifest_sha256 = workspace.receipt().source_manifest_sha256.clone();
    let header = BundleHeaderV1 {
        schema_version: BUNDLE_SCHEMA_V2.into(),
        source_manifest_sha256: source_manifest_sha256.clone(),
    };
    let (mut output, temporary) = create_artifact_temporary(artifact)?;
    let result = (|| -> Result<()> {
        output.write_all(BUNDLE_MAGIC_V2)?;
        write_json_record(&mut output, &header, MAX_BUNDLE_HEADER_BYTES)?;
        let mut tree_hash = tree_hash_prefix(BundleVersion::V2);
        for source in &sources {
            let expected_digest = match source.kind {
                BundleEntryKindV2::Directory => {
                    verify_directory_source_unchanged(source)?;
                    empty_sha256()
                }
                BundleEntryKindV2::Regular => decode_sha256(&hash_source_without_copy(source)?)?,
            };
            let metadata = BundleEntryV2 {
                kind: source.kind,
                relative_path_b64: STANDARD_NO_PAD.encode(os_bytes(&source.relative)),
                mode: source.mode,
                bytes: source.bytes,
                sha256: encode_hex(&expected_digest),
            };
            let metadata_bytes = serde_json::to_vec(&metadata)?;
            if metadata_bytes.len() > MAX_ENTRY_HEADER_BYTES {
                bail!("recovery entry metadata exceeds the bound");
            }
            write_length(&mut output, metadata_bytes.len() as u32)?;
            output.write_all(&metadata_bytes)?;
            let copied = match source.kind {
                BundleEntryKindV2::Directory => {
                    verify_directory_source_unchanged(source)?;
                    empty_sha256()
                }
                BundleEntryKindV2::Regular => copy_source_and_hash(source, &mut output)?,
            };
            if expected_digest != copied {
                bail!("workspace entry changed while recovery bundle was written");
            }
            update_tree_hash(&mut tree_hash, source, &copied, BundleVersion::V2);
        }
        write_length(&mut output, 0)?;
        let footer = BundleFooterV1 {
            entries: sources.len() as u64,
            logical_bytes,
            tree_sha256: encode_hex(&tree_hash.finalize()),
        };
        write_json_record(&mut output, &footer, MAX_BUNDLE_HEADER_BYTES)?;
        output.sync_all()?;
        Ok(())
    })();
    if let Err(error) = result {
        drop(output);
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    drop(output);
    fs::rename(&temporary, artifact).with_context(|| {
        format!(
            "cannot atomically publish recovery bundle {}",
            artifact.display()
        )
    })?;
    sync_parent(artifact)?;
    let original_recovery = workspace
        .verify_recovery_artifact(artifact)
        .map_err(|error| anyhow!("cannot bind recovery bundle to workspace: {error}"))?;
    Ok(RecoveryBundleReceiptV1 {
        schema_version: BUNDLE_SCHEMA_V2.into(),
        source_manifest_sha256,
        final_manifest_sha256: original_recovery.final_manifest_sha256().to_string(),
        recovery_artifact_sha256: original_recovery.artifact_sha256(),
        recovery_artifact_size: original_recovery.artifact_size(),
        entries: sources.len() as u64,
        logical_bytes,
    })
}

/// Reconstructs a bundle into a fresh WSP baseline, requires WSP's final
/// manifest to exactly match the original final manifest, and returns both
/// private cleanup capabilities.  This is the semantic proof WSP deliberately
/// leaves to its caller.
pub fn prove_recovery_bundle(
    original: DisposableWorkspace,
    source: &Path,
    private_root: &Path,
    reconstruction_label: &str,
    artifact: &Path,
    receipt: RecoveryBundleReceiptV1,
) -> Result<ProvenRecoveryBundle> {
    require_unix()?;
    if !supported_bundle_schema(&receipt.schema_version)
        || receipt.source_manifest_sha256 != original.receipt().source_manifest_sha256
    {
        bail!("recovery receipt is not bound to the original workspace baseline");
    }
    let parsed = inspect_bundle(artifact)?;
    if parsed.schema_version != receipt.schema_version
        || parsed.source_manifest_sha256 != receipt.source_manifest_sha256
    {
        bail!("recovery artifact is not bound to the recorded source baseline");
    }
    let original_recovery = original
        .verify_recovery_artifact(artifact)
        .map_err(|error| anyhow!("cannot rebind original recovery artifact: {error}"))?;
    if original_recovery.artifact_sha256() != receipt.recovery_artifact_sha256
        || original_recovery.artifact_size() != receipt.recovery_artifact_size
        || original_recovery.final_manifest_sha256() != receipt.final_manifest_sha256
    {
        bail!("recovery artifact or original final manifest changed after export");
    }
    let reconstructed = materialize_workspace(
        source,
        private_root,
        reconstruction_label,
        &MaterializationPolicy::benchmark(),
        ClonePreference::PreferClone,
    )
    .map_err(|error| anyhow!("cannot materialize fresh reconstruction baseline: {error}"))?;
    if reconstructed.receipt().source_manifest_sha256 != receipt.source_manifest_sha256 {
        bail!("fresh reconstruction baseline does not match the exported source");
    }
    apply_recovery_bundle(artifact, reconstructed.path())?;
    let reconstructed_recovery = reconstructed
        .verify_recovery_artifact(artifact)
        .map_err(|error| anyhow!("cannot bind reconstructed recovery artifact: {error}"))?;
    if reconstructed_recovery.artifact_sha256() != receipt.recovery_artifact_sha256
        || reconstructed_recovery.artifact_size() != receipt.recovery_artifact_size
        || reconstructed_recovery.final_manifest_sha256() != receipt.final_manifest_sha256
    {
        bail!("recovery bundle does not reconstruct the exact final workspace manifest");
    }
    Ok(ProvenRecoveryBundle {
        receipt,
        original,
        original_recovery,
        reconstructed,
        reconstructed_recovery,
    })
}

/// Applies a fully validated recovery bundle to a fresh benchmark workspace.
/// The input is parsed and all payload hashes are checked before the target is
/// changed.  It refuses links and special files on both sides.
pub fn apply_recovery_bundle(artifact: &Path, destination: &Path) -> Result<()> {
    require_unix()?;
    let bundle = inspect_bundle(artifact)?;
    clear_regular_tree(destination)?;
    for entry in bundle
        .entries
        .iter()
        .filter(|entry| entry.kind == BundleEntryKindV2::Directory)
    {
        let destination_path = destination.join(&entry.relative);
        let parent = destination_path
            .parent()
            .ok_or_else(|| anyhow!("recovery entry has no parent"))?;
        if !parent.is_dir() {
            bail!("recovery directory parent is missing");
        }
        create_private_directory(&destination_path)?;
    }
    for entry in bundle
        .entries
        .iter()
        .filter(|entry| entry.kind == BundleEntryKindV2::Regular)
    {
        let destination_path = destination.join(&entry.relative);
        let parent = destination_path
            .parent()
            .ok_or_else(|| anyhow!("recovery entry has no parent"))?;
        if bundle.records_directories {
            if !parent.is_dir() {
                bail!("recovery file parent is missing");
            }
        } else {
            fs::create_dir_all(parent).with_context(|| {
                format!("cannot create recovery bundle parent {}", parent.display())
            })?;
        }
        let mut artifact_file = open_regular_readonly(artifact)?;
        artifact_file.seek(SeekFrom::Start(entry.data_offset))?;
        let mut output = create_new_regular_file(&destination_path, entry.mode)?;
        let digest = copy_exact_and_hash(&mut artifact_file, &mut output, entry.bytes)?;
        if digest != entry.sha256 {
            bail!("recovery bundle payload changed during application");
        }
        output.sync_all()?;
        #[cfg(unix)]
        fs::set_permissions(
            &destination_path,
            fs::Permissions::from_mode(entry.mode & 0o777),
        )?;
    }
    let mut directories = bundle
        .entries
        .iter()
        .filter(|entry| entry.kind == BundleEntryKindV2::Directory)
        .collect::<Vec<_>>();
    directories
        .sort_by_key(|entry| std::cmp::Reverse(Path::new(&entry.relative).components().count()));
    for entry in directories {
        fs::set_permissions(
            destination.join(&entry.relative),
            fs::Permissions::from_mode(entry.mode & 0o777),
        )?;
    }
    let after = inspect_regular_tree(destination)?;
    let expected_entries = if bundle.records_directories {
        after.entries
    } else {
        after.regular_files
    };
    if expected_entries != bundle.entries.len() || after.logical_bytes != bundle.logical_bytes {
        bail!("recovery bundle application produced an unexpected workspace tree");
    }
    Ok(())
}

fn collect_sources(
    root: &Path,
    current: &Path,
    depth: usize,
    output: &mut Vec<SourceEntry>,
) -> Result<()> {
    if depth > 128 {
        bail!("recovery bundle exceeds maximum directory depth");
    }
    let mut children = fs::read_dir(current)
        .with_context(|| format!("cannot read recovery source {}", current.display()))?
        .collect::<io::Result<Vec<_>>>()?;
    children.sort_by(|left, right| os_bytes(&left.file_name()).cmp(os_bytes(&right.file_name())));
    for child in children {
        let absolute = child.path();
        let metadata = fs::symlink_metadata(&absolute)
            .with_context(|| format!("cannot inspect recovery source {}", absolute.display()))?;
        if metadata.file_type().is_symlink() {
            bail!("recovery bundle rejects symbolic links");
        }
        let kind = if metadata.is_dir() {
            BundleEntryKindV2::Directory
        } else if metadata.is_file() {
            BundleEntryKindV2::Regular
        } else {
            bail!("recovery bundle rejects special files");
        };
        if output.len() >= DEFAULT_MAX_ENTRIES {
            bail!("recovery bundle exceeds entry limit");
        }
        let relative = absolute
            .strip_prefix(root)
            .map_err(|_| anyhow!("recovery source escaped workspace root"))?
            .as_os_str()
            .to_os_string();
        validate_relative_os_path(&relative)?;
        #[cfg(unix)]
        let source = SourceEntry {
            kind,
            relative,
            absolute: absolute.clone(),
            mode: metadata.mode() & 0o777,
            bytes: if kind == BundleEntryKindV2::Regular {
                metadata.len()
            } else {
                0
            },
            device: metadata.dev(),
            inode: metadata.ino(),
        };
        #[cfg(not(unix))]
        let source = SourceEntry {
            kind,
            relative,
            absolute: absolute.clone(),
            mode: 0,
            bytes: if kind == BundleEntryKindV2::Regular {
                metadata.len()
            } else {
                0
            },
        };
        let recurse = source.kind == BundleEntryKindV2::Directory;
        output.push(source);
        if recurse {
            collect_sources(root, &absolute, depth + 1, output)?;
        }
    }
    Ok(())
}

fn hash_source_without_copy(source: &SourceEntry) -> Result<String> {
    let mut input = open_verified_source(source)?;
    let digest = hash_exact(&mut input, source.bytes)?;
    verify_source_unchanged(source, &input)?;
    Ok(encode_hex(&digest))
}

fn copy_source_and_hash(source: &SourceEntry, output: &mut File) -> Result<[u8; 32]> {
    let mut input = open_verified_source(source)?;
    let digest = copy_exact_and_hash(&mut input, output, source.bytes)?;
    verify_source_unchanged(source, &input)?;
    Ok(digest)
}

fn open_verified_source(source: &SourceEntry) -> Result<File> {
    if source.kind != BundleEntryKindV2::Regular {
        bail!("recovery file entry is invalid");
    }
    let file = open_regular_readonly(&source.absolute)?;
    #[cfg(unix)]
    {
        let metadata = file.metadata()?;
        if !metadata.is_file()
            || metadata.dev() != source.device
            || metadata.ino() != source.inode
            || metadata.len() != source.bytes
            || metadata.mode() & 0o777 != source.mode
        {
            bail!("workspace source changed before recovery bundle read");
        }
    }
    Ok(file)
}

fn verify_source_unchanged(source: &SourceEntry, input: &File) -> Result<()> {
    #[cfg(unix)]
    {
        let metadata = input.metadata()?;
        if !metadata.is_file()
            || metadata.dev() != source.device
            || metadata.ino() != source.inode
            || metadata.len() != source.bytes
            || metadata.mode() & 0o777 != source.mode
        {
            bail!("workspace source changed while recovery bundle was read");
        }
    }
    Ok(())
}

fn verify_directory_source_unchanged(source: &SourceEntry) -> Result<()> {
    if source.kind != BundleEntryKindV2::Directory || source.bytes != 0 {
        bail!("recovery directory entry is invalid");
    }
    #[cfg(unix)]
    {
        let mut options = OpenOptions::new();
        options
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC);
        let directory = options.open(&source.absolute)?;
        let metadata = directory.metadata()?;
        if !metadata.is_dir()
            || metadata.dev() != source.device
            || metadata.ino() != source.inode
            || metadata.mode() & 0o777 != source.mode
        {
            bail!("workspace directory changed while recovery bundle was written");
        }
    }
    #[cfg(not(unix))]
    {
        let metadata = fs::symlink_metadata(&source.absolute)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            bail!("workspace directory changed while recovery bundle was written");
        }
    }
    Ok(())
}

fn inspect_bundle(artifact: &Path) -> Result<ParsedBundle> {
    let mut input = open_regular_readonly(artifact)?;
    let mut magic = [0_u8; BUNDLE_MAGIC_V1.len()];
    input.read_exact(&mut magic)?;
    let version = if &magic == BUNDLE_MAGIC_V1 {
        BundleVersion::V1
    } else if &magic == BUNDLE_MAGIC_V2 {
        BundleVersion::V2
    } else {
        bail!("recovery artifact has an invalid magic header");
    };
    let header: BundleHeaderV1 = read_json_record(&mut input, MAX_BUNDLE_HEADER_BYTES)?;
    if header.schema_version != version.schema() || !valid_sha256(&header.source_manifest_sha256) {
        bail!("recovery artifact has an invalid header");
    }
    let mut entries = Vec::new();
    let mut directories = BTreeSet::new();
    let mut logical_bytes = 0_u64;
    let mut tree_hash = tree_hash_prefix(version);
    let mut previous: Option<Vec<u8>> = None;
    loop {
        let metadata_length = read_length(&mut input)?;
        if metadata_length == 0 {
            break;
        }
        if metadata_length as usize > MAX_ENTRY_HEADER_BYTES {
            bail!("recovery artifact entry metadata exceeds the bound");
        }
        let mut metadata_bytes = vec![0_u8; metadata_length as usize];
        input.read_exact(&mut metadata_bytes)?;
        let (kind, relative_path_b64, mode, bytes, encoded_sha256) = match version {
            BundleVersion::V1 => {
                let entry: BundleEntryV1 = serde_json::from_slice(&metadata_bytes)
                    .context("recovery artifact has invalid V1 entry metadata")?;
                (
                    BundleEntryKindV2::Regular,
                    entry.relative_path_b64,
                    entry.mode,
                    entry.bytes,
                    entry.sha256,
                )
            }
            BundleVersion::V2 => {
                let entry: BundleEntryV2 = serde_json::from_slice(&metadata_bytes)
                    .context("recovery artifact has invalid V2 entry metadata")?;
                (
                    entry.kind,
                    entry.relative_path_b64,
                    entry.mode,
                    entry.bytes,
                    entry.sha256,
                )
            }
        };
        let relative_bytes = STANDARD_NO_PAD
            .decode(relative_path_b64)
            .context("recovery artifact path is not base64")?;
        let relative = os_string_from_bytes(relative_bytes.clone())?;
        validate_relative_os_path(&relative)?;
        if previous
            .as_ref()
            .is_some_and(|previous| previous.cmp(&relative_bytes) != Ordering::Less)
        {
            bail!("recovery artifact entries are not in strict path order");
        }
        previous = Some(relative_bytes);
        if mode & !0o777 != 0 || bytes > DEFAULT_MAX_LOGICAL_BYTES {
            bail!("recovery artifact entry mode or size is invalid");
        }
        let sha256 = decode_sha256(&encoded_sha256)?;
        if kind == BundleEntryKindV2::Directory
            && (version != BundleVersion::V2 || bytes != 0 || sha256 != empty_sha256())
        {
            bail!("recovery artifact directory entry is invalid");
        }
        if version == BundleVersion::V2 {
            let relative_path = PathBuf::from(&relative);
            let parent = relative_path
                .parent()
                .ok_or_else(|| anyhow!("recovery artifact entry has no parent"))?;
            if !parent.as_os_str().is_empty() && !directories.contains(parent) {
                bail!("recovery artifact omits a parent directory");
            }
            if kind == BundleEntryKindV2::Directory {
                directories.insert(relative_path);
            }
        }
        logical_bytes = logical_bytes
            .checked_add(bytes)
            .ok_or_else(|| anyhow!("recovery artifact logical byte overflow"))?;
        if logical_bytes > DEFAULT_MAX_LOGICAL_BYTES || entries.len() >= DEFAULT_MAX_ENTRIES {
            bail!("recovery artifact exceeds workspace bounds");
        }
        let data_offset = input.stream_position()?;
        let actual = hash_stream_bytes(&mut input, bytes)?;
        if actual != sha256 {
            bail!("recovery artifact payload digest mismatch");
        }
        let source = SourceEntry {
            kind,
            relative: relative.clone(),
            absolute: PathBuf::new(),
            mode,
            bytes,
            #[cfg(unix)]
            device: 0,
            #[cfg(unix)]
            inode: 0,
        };
        update_tree_hash(&mut tree_hash, &source, &sha256, version);
        entries.push(ParsedEntry {
            kind,
            relative,
            mode,
            bytes,
            sha256,
            data_offset,
        });
    }
    let footer: BundleFooterV1 = read_json_record(&mut input, MAX_BUNDLE_HEADER_BYTES)?;
    if footer.entries != entries.len() as u64
        || footer.logical_bytes != logical_bytes
        || footer.tree_sha256 != encode_hex(&tree_hash.finalize())
    {
        bail!("recovery artifact footer does not match payload");
    }
    if input.read(&mut [0_u8; 1])? != 0 {
        bail!("recovery artifact has trailing bytes");
    }
    Ok(ParsedBundle {
        schema_version: header.schema_version,
        records_directories: version.records_directories(),
        source_manifest_sha256: header.source_manifest_sha256,
        entries,
        logical_bytes,
    })
}

fn clear_regular_tree(root: &Path) -> Result<()> {
    let root = root
        .canonicalize()
        .context("cannot canonicalize reconstruction root")?;
    let metadata = fs::symlink_metadata(&root)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        bail!("reconstruction root is not a regular directory");
    }
    let children = fs::read_dir(&root)?.collect::<io::Result<Vec<_>>>()?;
    for child in children {
        let path = child.path();
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() || (!metadata.is_file() && !metadata.is_dir()) {
            bail!("reconstruction root contains an unsafe entry");
        }
        if metadata.is_dir() {
            clear_regular_tree(&path)?;
            fs::remove_dir(&path)?;
        } else {
            fs::remove_file(&path)?;
        }
    }
    Ok(())
}

#[derive(Debug)]
struct TreeSummary {
    entries: usize,
    regular_files: usize,
    logical_bytes: u64,
}

fn inspect_regular_tree(root: &Path) -> Result<TreeSummary> {
    let mut entries = 0_usize;
    let mut regular_files = 0_usize;
    let mut logical_bytes = 0_u64;
    inspect_regular_tree_inner(root, &mut entries, &mut regular_files, &mut logical_bytes)?;
    Ok(TreeSummary {
        entries,
        regular_files,
        logical_bytes,
    })
}

fn inspect_regular_tree_inner(
    current: &Path,
    entries: &mut usize,
    regular_files: &mut usize,
    logical_bytes: &mut u64,
) -> Result<()> {
    for child in fs::read_dir(current)?.collect::<io::Result<Vec<_>>>()? {
        let path = child.path();
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() || (!metadata.is_file() && !metadata.is_dir()) {
            bail!("reconstructed workspace contains an unsafe entry");
        }
        *entries = entries
            .checked_add(1)
            .ok_or_else(|| anyhow!("reconstructed workspace entry count overflow"))?;
        if metadata.is_dir() {
            inspect_regular_tree_inner(&path, entries, regular_files, logical_bytes)?;
        } else {
            *regular_files = regular_files
                .checked_add(1)
                .ok_or_else(|| anyhow!("reconstructed workspace file count overflow"))?;
            *logical_bytes = logical_bytes
                .checked_add(metadata.len())
                .ok_or_else(|| anyhow!("reconstructed workspace byte overflow"))?;
        }
    }
    Ok(())
}

fn create_artifact_temporary(artifact: &Path) -> Result<(File, PathBuf)> {
    let parent = artifact
        .parent()
        .ok_or_else(|| anyhow!("recovery artifact has no parent"))?;
    let parent = parent
        .canonicalize()
        .context("cannot canonicalize recovery artifact parent")?;
    require_private_directory(&parent)?;
    let name = artifact
        .file_name()
        .filter(|name| !name.is_empty())
        .ok_or_else(|| anyhow!("recovery artifact has no safe file name"))?;
    let final_path = parent.join(name);
    if final_path.exists() {
        bail!("recovery artifact already exists");
    }
    let temporary = parent.join(format!(
        ".{}.{}.tmp",
        name.to_string_lossy(),
        Uuid::new_v4()
    ));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options
        .mode(0o600)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
    let file = options.open(&temporary).with_context(|| {
        format!(
            "cannot create recovery artifact temporary {}",
            temporary.display()
        )
    })?;
    Ok((file, temporary))
}

fn ensure_artifact_is_external(workspace: &Path, artifact: &Path) -> Result<()> {
    let workspace = workspace
        .canonicalize()
        .context("cannot canonicalize workspace for recovery artifact")?;
    let parent = artifact
        .parent()
        .ok_or_else(|| anyhow!("recovery artifact has no parent"))?
        .canonicalize()
        .context("cannot canonicalize recovery artifact parent")?;
    if parent.starts_with(&workspace) || workspace.starts_with(&parent) {
        bail!("recovery artifact must be outside the disposable workspace");
    }
    Ok(())
}

fn validate_checkpoint(checkpoint: &RecoveryCheckpointV1) -> Result<()> {
    if checkpoint.schema_version != CHECKPOINT_SCHEMA {
        bail!("recovery checkpoint schema is invalid");
    }
    validate_trial_id(&checkpoint.trial_id)?;
    for (label, path) in [
        ("private_root", &checkpoint.private_root),
        ("source", &checkpoint.source),
        ("artifact", &checkpoint.artifact),
    ] {
        if !path.is_absolute() {
            bail!("recovery checkpoint {label} must be absolute");
        }
    }
    if !supported_bundle_schema(&checkpoint.bundle.schema_version)
        || !valid_sha256(&checkpoint.bundle.source_manifest_sha256)
        || !valid_sha256(&checkpoint.bundle.final_manifest_sha256)
        || !valid_sha256(&checkpoint.bundle.recovery_artifact_sha256)
        || checkpoint.bundle.recovery_artifact_size == 0
        || checkpoint.bundle.entries == 0
        || checkpoint.bundle.entries as usize > DEFAULT_MAX_ENTRIES
        || checkpoint.bundle.logical_bytes > DEFAULT_MAX_LOGICAL_BYTES
    {
        bail!("recovery checkpoint bundle receipt is invalid");
    }
    Ok(())
}

fn verified_for_checkpoint(
    workspace: &DisposableWorkspace,
    checkpoint: &RecoveryCheckpointV1,
) -> Result<VerifiedRecoveryArtifact> {
    if workspace.receipt().source_manifest_sha256 != checkpoint.bundle.source_manifest_sha256 {
        bail!("checkpoint workspace baseline does not match recovery bundle");
    }
    let recovery = workspace
        .verify_recovery_artifact(&checkpoint.artifact)
        .map_err(|error| anyhow!("cannot verify checkpoint recovery artifact: {error}"))?;
    if recovery.artifact_sha256() != checkpoint.bundle.recovery_artifact_sha256
        || recovery.artifact_size() != checkpoint.bundle.recovery_artifact_size
        || recovery.final_manifest_sha256() != checkpoint.bundle.final_manifest_sha256
    {
        bail!("checkpoint workspace does not match recovery bundle");
    }
    Ok(recovery)
}

fn cleanup_already_complete(private_root: &Path, workspace_id: Uuid) -> Result<bool> {
    let suffix = workspace_id.to_string();
    let receipt = private_root
        .join(".workspace-receipts")
        .join(format!("{workspace_id}.json"));
    if fs::symlink_metadata(&receipt).is_ok() {
        return Ok(false);
    }
    for entry in fs::read_dir(private_root)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.ends_with(&suffix) {
            return Ok(false);
        }
    }
    Ok(true)
}

fn validate_trial_id(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        bail!("recovery checkpoint trial ID is invalid");
    }
    Ok(())
}

fn require_private_directory(path: &Path) -> Result<()> {
    let metadata = fs::metadata(path)?;
    if !metadata.is_dir() {
        bail!("recovery artifact parent is not a directory");
    }
    #[cfg(unix)]
    if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 {
        bail!("recovery artifact parent is not owner-only");
    }
    Ok(())
}

fn open_regular_readonly(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
    let file = options
        .open(path)
        .with_context(|| format!("cannot open regular file {}", path.display()))?;
    if !file.metadata()?.is_file() {
        bail!("path is not a regular file");
    }
    Ok(file)
}

fn create_new_regular_file(path: &Path, mode: u32) -> Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options
        .mode(mode & 0o777)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
    options
        .open(path)
        .with_context(|| format!("cannot create recovery output {}", path.display()))
}

fn create_private_directory(path: &Path) -> Result<()> {
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    builder.mode(0o700);
    builder
        .create(path)
        .with_context(|| format!("cannot create recovery directory {}", path.display()))
}

fn hash_exact(input: &mut File, bytes: u64) -> Result<[u8; 32]> {
    let mut hasher = Sha256::new();
    let mut remaining = bytes;
    let mut buffer = [0_u8; COPY_BUFFER_BYTES];
    while remaining > 0 {
        let expected = remaining.min(buffer.len() as u64) as usize;
        input.read_exact(&mut buffer[..expected])?;
        hasher.update(&buffer[..expected]);
        remaining -= expected as u64;
    }
    if input.read(&mut buffer[..1])? != 0 {
        bail!("regular file grew while recovery bundle was read");
    }
    Ok(hasher.finalize().into())
}

fn copy_exact_and_hash(input: &mut File, output: &mut File, bytes: u64) -> Result<[u8; 32]> {
    let mut hasher = Sha256::new();
    let mut remaining = bytes;
    let mut buffer = [0_u8; COPY_BUFFER_BYTES];
    while remaining > 0 {
        let expected = remaining.min(buffer.len() as u64) as usize;
        input.read_exact(&mut buffer[..expected])?;
        output.write_all(&buffer[..expected])?;
        hasher.update(&buffer[..expected]);
        remaining -= expected as u64;
    }
    Ok(hasher.finalize().into())
}

fn hash_stream_bytes(input: &mut File, bytes: u64) -> Result<[u8; 32]> {
    let mut hasher = Sha256::new();
    let mut remaining = bytes;
    let mut buffer = [0_u8; COPY_BUFFER_BYTES];
    while remaining > 0 {
        let expected = remaining.min(buffer.len() as u64) as usize;
        input.read_exact(&mut buffer[..expected])?;
        hasher.update(&buffer[..expected]);
        remaining -= expected as u64;
    }
    Ok(hasher.finalize().into())
}

fn tree_hash_prefix(version: BundleVersion) -> Sha256 {
    let mut hasher = Sha256::new();
    match version {
        BundleVersion::V1 => hasher.update(b"ca-rnd097-recovery-tree-v1\0"),
        BundleVersion::V2 => hasher.update(b"ca-rnd097-recovery-tree-v2\0"),
    };
    hasher
}

fn empty_sha256() -> [u8; 32] {
    Sha256::digest([]).into()
}

fn update_tree_hash(
    hasher: &mut Sha256,
    entry: &SourceEntry,
    sha256: &[u8; 32],
    version: BundleVersion,
) {
    let bytes = os_bytes(&entry.relative);
    hasher.update((bytes.len() as u64).to_be_bytes());
    hasher.update(bytes);
    if version == BundleVersion::V2 {
        hasher.update([match entry.kind {
            BundleEntryKindV2::Directory => 0,
            BundleEntryKindV2::Regular => 1,
        }]);
    }
    hasher.update(entry.mode.to_be_bytes());
    hasher.update(entry.bytes.to_be_bytes());
    hasher.update(sha256);
}

fn write_json_record<T: Serialize>(output: &mut File, value: &T, max_bytes: usize) -> Result<()> {
    let bytes = serde_json::to_vec(value)?;
    if bytes.len() > max_bytes {
        bail!("recovery bundle JSON record exceeds the bound");
    }
    write_length(output, bytes.len() as u32)?;
    output.write_all(&bytes)?;
    Ok(())
}

fn read_json_record<T: for<'de> Deserialize<'de>>(input: &mut File, max_bytes: usize) -> Result<T> {
    let length = read_length(input)? as usize;
    if length == 0 || length > max_bytes {
        bail!("recovery bundle JSON record has an invalid length");
    }
    let mut bytes = vec![0_u8; length];
    input.read_exact(&mut bytes)?;
    serde_json::from_slice(&bytes).context("recovery bundle JSON record is invalid")
}

fn write_length(output: &mut File, length: u32) -> Result<()> {
    output.write_all(&length.to_be_bytes())?;
    Ok(())
}

fn read_length(input: &mut File) -> Result<u32> {
    let mut bytes = [0_u8; 4];
    input.read_exact(&mut bytes)?;
    Ok(u32::from_be_bytes(bytes))
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn supported_bundle_schema(value: &str) -> bool {
    matches!(value, BUNDLE_SCHEMA_V1 | BUNDLE_SCHEMA_V2)
}

fn decode_sha256(value: &str) -> Result<[u8; 32]> {
    if !valid_sha256(value) {
        bail!("recovery artifact digest is invalid");
    }
    let mut output = [0_u8; 32];
    for (index, slot) in output.iter_mut().enumerate() {
        let high = from_hex(value.as_bytes()[index * 2])?;
        let low = from_hex(value.as_bytes()[index * 2 + 1])?;
        *slot = (high << 4) | low;
    }
    Ok(output)
}

fn encode_hex(bytes: &[u8]) -> String {
    const TABLE: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(TABLE[(byte >> 4) as usize] as char);
        output.push(TABLE[(byte & 0x0f) as usize] as char);
    }
    output
}

fn from_hex(byte: u8) -> Result<u8> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => bail!("recovery artifact digest is invalid"),
    }
}

fn validate_relative_os_path(value: &OsStr) -> Result<()> {
    let path = Path::new(value);
    if path.as_os_str().is_empty() || path.is_absolute() {
        bail!("recovery artifact path must be nonempty and relative");
    }
    for component in path.components() {
        if !matches!(component, Component::Normal(_)) {
            bail!("recovery artifact path is not normalized");
        }
    }
    Ok(())
}

#[cfg(unix)]
fn os_bytes(value: &OsStr) -> &[u8] {
    value.as_bytes()
}

#[cfg(not(unix))]
fn os_bytes(value: &OsStr) -> &[u8] {
    value.to_str().unwrap_or_default().as_bytes()
}

#[cfg(unix)]
fn os_string_from_bytes(value: Vec<u8>) -> Result<OsString> {
    if value.contains(&0) {
        bail!("recovery artifact path contains NUL");
    }
    Ok(OsString::from_vec(value))
}

#[cfg(not(unix))]
fn os_string_from_bytes(value: Vec<u8>) -> Result<OsString> {
    Ok(OsString::from(
        String::from_utf8(value).context("recovery path is not UTF-8")?,
    ))
}

fn sync_parent(path: &Path) -> Result<()> {
    let parent = path.parent().ok_or_else(|| anyhow!("path has no parent"))?;
    #[cfg(unix)]
    File::open(parent)?.sync_all()?;
    Ok(())
}

fn require_unix() -> Result<()> {
    #[cfg(not(unix))]
    bail!("RND-097 recovery bundles require Unix descriptor semantics");
    #[cfg(unix)]
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn fixture_source() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/reasoning_pilot/unicode_parser_boundary")
    }

    fn private_layout() -> (TempDir, PathBuf, PathBuf) {
        let outer = tempfile::tempdir().unwrap();
        let canonical_outer = outer.path().canonicalize().unwrap();
        let private = canonical_outer.join("private");
        let artifacts = canonical_outer.join("artifacts");
        fs::create_dir(&private).unwrap();
        fs::create_dir(&artifacts).unwrap();
        #[cfg(unix)]
        {
            fs::set_permissions(&private, fs::Permissions::from_mode(0o700)).unwrap();
            fs::set_permissions(&artifacts, fs::Permissions::from_mode(0o700)).unwrap();
        }
        (outer, private, artifacts)
    }

    fn write_v1_bundle_for_compatibility(
        workspace: &DisposableWorkspace,
        artifact: &Path,
    ) -> RecoveryBundleReceiptV1 {
        let mut sources = Vec::new();
        collect_sources(workspace.path(), workspace.path(), 0, &mut sources).unwrap();
        sources.retain(|entry| entry.kind == BundleEntryKindV2::Regular);
        sources.sort_by(|left, right| os_bytes(&left.relative).cmp(os_bytes(&right.relative)));
        let logical_bytes = sources.iter().map(|entry| entry.bytes).sum();
        let source_manifest_sha256 = workspace.receipt().source_manifest_sha256.clone();
        let header = BundleHeaderV1 {
            schema_version: BUNDLE_SCHEMA_V1.into(),
            source_manifest_sha256: source_manifest_sha256.clone(),
        };
        let (mut output, temporary) = create_artifact_temporary(artifact).unwrap();
        output.write_all(BUNDLE_MAGIC_V1).unwrap();
        write_json_record(&mut output, &header, MAX_BUNDLE_HEADER_BYTES).unwrap();
        let mut tree_hash = tree_hash_prefix(BundleVersion::V1);
        for source in &sources {
            let digest = decode_sha256(&hash_source_without_copy(source).unwrap()).unwrap();
            let metadata = BundleEntryV1 {
                relative_path_b64: STANDARD_NO_PAD.encode(os_bytes(&source.relative)),
                mode: source.mode,
                bytes: source.bytes,
                sha256: encode_hex(&digest),
            };
            write_json_record(&mut output, &metadata, MAX_ENTRY_HEADER_BYTES).unwrap();
            let copied = copy_source_and_hash(source, &mut output).unwrap();
            assert_eq!(copied, digest);
            update_tree_hash(&mut tree_hash, source, &copied, BundleVersion::V1);
        }
        write_length(&mut output, 0).unwrap();
        write_json_record(
            &mut output,
            &BundleFooterV1 {
                entries: sources.len() as u64,
                logical_bytes,
                tree_sha256: encode_hex(&tree_hash.finalize()),
            },
            MAX_BUNDLE_HEADER_BYTES,
        )
        .unwrap();
        output.sync_all().unwrap();
        drop(output);
        fs::rename(temporary, artifact).unwrap();
        let recovery = workspace.verify_recovery_artifact(artifact).unwrap();
        RecoveryBundleReceiptV1 {
            schema_version: BUNDLE_SCHEMA_V1.into(),
            source_manifest_sha256,
            final_manifest_sha256: recovery.final_manifest_sha256().into(),
            recovery_artifact_sha256: recovery.artifact_sha256(),
            recovery_artifact_size: recovery.artifact_size(),
            entries: sources.len() as u64,
            logical_bytes,
        }
    }

    #[test]
    fn recovery_bundle_round_trips_added_deleted_binary_and_mode_changes_before_cleanup() {
        let (_outer, private, artifacts) = private_layout();
        let source = fixture_source();
        let workspace = materialize_workspace(
            &source,
            &private,
            "original",
            &MaterializationPolicy::benchmark(),
            ClonePreference::CopyOnly,
        )
        .unwrap();
        fs::remove_file(workspace.path().join("src/main.rs")).unwrap();
        let added = workspace.path().join("src/added.bin");
        fs::write(&added, [0_u8, 255, 17, 0, 99]).unwrap();
        #[cfg(unix)]
        fs::set_permissions(&added, fs::Permissions::from_mode(0o751)).unwrap();
        fs::write(
            workspace.path().join("src/lib.rs"),
            b"pub fn recovered_value() -> &'static str { \"ok\" }\n",
        )
        .unwrap();
        let artifact = artifacts.join("trial.bundle");
        let receipt = write_recovery_bundle(&workspace, &artifact).unwrap();
        assert!(artifact.exists());
        assert!(receipt.entries >= 3);
        let proof = prove_recovery_bundle(
            workspace,
            &source,
            &private,
            "reconstructed",
            &artifact,
            receipt.clone(),
        )
        .unwrap();
        assert_eq!(proof.receipt(), &receipt);
        proof.cleanup_after_persisting().unwrap();
        assert!(!private.join("original").exists());
        assert!(!private.join("reconstructed").exists());
    }

    #[test]
    fn recovery_bundle_round_trips_directory_modes_and_empty_directories() {
        let (_outer, private, artifacts) = private_layout();
        let source = fixture_source();
        let workspace = materialize_workspace(
            &source,
            &private,
            "directory-original",
            &MaterializationPolicy::benchmark(),
            ClonePreference::CopyOnly,
        )
        .unwrap();
        let source_directory = workspace.path().join("src");
        let empty_directory = workspace.path().join("empty-retained");
        fs::create_dir(&empty_directory).unwrap();
        #[cfg(unix)]
        {
            fs::set_permissions(&source_directory, fs::Permissions::from_mode(0o751)).unwrap();
            fs::set_permissions(&empty_directory, fs::Permissions::from_mode(0o711)).unwrap();
        }
        let artifact = artifacts.join("directory.bundle");
        let receipt = write_recovery_bundle(&workspace, &artifact).unwrap();
        let proof = prove_recovery_bundle(
            workspace,
            &source,
            &private,
            "directory-reconstructed",
            &artifact,
            receipt,
        )
        .unwrap();
        proof.cleanup_after_persisting().unwrap();
    }

    #[test]
    fn legacy_v1_regular_file_bundle_remains_reconstructable() {
        let (outer, private, artifacts) = private_layout();
        let source = outer.path().join("legacy-source");
        fs::create_dir(&source).unwrap();
        fs::create_dir(source.join("src")).unwrap();
        fs::write(source.join("src/lib.rs"), b"pub fn legacy() {}\n").unwrap();
        #[cfg(unix)]
        {
            fs::set_permissions(source.join("src"), fs::Permissions::from_mode(0o755)).unwrap();
            fs::set_permissions(source.join("src/lib.rs"), fs::Permissions::from_mode(0o644))
                .unwrap();
        }
        let source = source.canonicalize().unwrap();
        let workspace = materialize_workspace(
            &source,
            &private,
            "legacy-original",
            &MaterializationPolicy::benchmark(),
            ClonePreference::CopyOnly,
        )
        .unwrap();
        let artifact = artifacts.join("legacy-v1.bundle");
        let receipt = write_v1_bundle_for_compatibility(&workspace, &artifact);
        let proof = prove_recovery_bundle(
            workspace,
            &source,
            &private,
            "legacy-reconstructed",
            &artifact,
            receipt,
        )
        .unwrap();
        proof.cleanup_after_persisting().unwrap();
    }

    #[test]
    fn corrupt_bundle_is_rejected_before_reconstruction_workspace_changes() {
        let (_outer, private, artifacts) = private_layout();
        let source = fixture_source();
        let original = materialize_workspace(
            &source,
            &private,
            "original-corrupt",
            &MaterializationPolicy::benchmark(),
            ClonePreference::CopyOnly,
        )
        .unwrap();
        let artifact = artifacts.join("corrupt.bundle");
        let receipt = write_recovery_bundle(&original, &artifact).unwrap();
        let mut bytes = fs::read(&artifact).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        fs::write(&artifact, bytes).unwrap();
        let fresh = materialize_workspace(
            &source,
            &private,
            "fresh-corrupt",
            &MaterializationPolicy::benchmark(),
            ClonePreference::CopyOnly,
        )
        .unwrap();
        let before = fs::read(fresh.path().join("src/lib.rs")).unwrap();
        assert!(apply_recovery_bundle(&artifact, fresh.path()).is_err());
        assert_eq!(fs::read(fresh.path().join("src/lib.rs")).unwrap(), before);
        // A corrupt artifact cannot authorize cleanup. Dropping preserves both
        // workspaces for a controller/operator to inspect or recover later.
        drop(fresh);
        drop(original);
        assert!(!receipt.recovery_artifact_sha256.is_empty());
    }

    #[test]
    fn exported_checkpoint_reopens_and_revalidates_without_serializing_wsp_capability() {
        let (_outer, private, artifacts) = private_layout();
        let checkpoints = artifacts.join("checkpoints");
        let source = fixture_source().canonicalize().unwrap();
        let workspace = materialize_workspace(
            &source,
            &private,
            "checkpoint-original",
            &MaterializationPolicy::benchmark(),
            ClonePreference::CopyOnly,
        )
        .unwrap();
        fs::write(
            workspace.path().join("src/lib.rs"),
            b"pub fn after_restart() {}\n",
        )
        .unwrap();
        let artifact = artifacts.join("checkpoint.bundle");
        let bundle = write_recovery_bundle(&workspace, &artifact).unwrap();
        let checkpoint = RecoveryCheckpointV1 {
            schema_version: CHECKPOINT_SCHEMA.into(),
            trial_id: "fixture-000-SC-1".into(),
            private_root: private.clone(),
            source: source.clone(),
            workspace_id: workspace.workspace_id(),
            reconstructed_workspace_id: None,
            artifact: artifact.clone(),
            bundle: bundle.clone(),
        };
        let store = RecoveryCheckpointStore::create(&checkpoints).unwrap();
        store.persist_exported(&checkpoint).unwrap();
        let saved_bytes = fs::read(checkpoints.join("fixture-000-SC-1.exported.json")).unwrap();
        assert!(!String::from_utf8_lossy(&saved_bytes).contains("VerifiedRecoveryArtifact"));
        drop(workspace);

        let loaded = store.load_exported("fixture-000-SC-1").unwrap();
        assert_eq!(loaded, checkpoint);
        let reopened = store.reopen_exported(&loaded).unwrap();
        let proof = prove_recovery_bundle(
            reopened,
            &source,
            &private,
            "checkpoint-reconstructed",
            &artifact,
            bundle,
        )
        .unwrap();
        proof.cleanup_after_persisting().unwrap();
    }

    #[test]
    fn checkpoint_store_rejects_tampering_and_duplicate_publication() {
        let (_outer, private, artifacts) = private_layout();
        let checkpoints = artifacts.join("checkpoints");
        let store = RecoveryCheckpointStore::create(&checkpoints).unwrap();
        let checkpoint = RecoveryCheckpointV1 {
            schema_version: CHECKPOINT_SCHEMA.into(),
            trial_id: "fixture-001-S0-1".into(),
            private_root: private,
            source: fixture_source().canonicalize().unwrap(),
            workspace_id: Uuid::new_v4(),
            reconstructed_workspace_id: None,
            artifact: artifacts.join("not-created.bundle"),
            bundle: RecoveryBundleReceiptV1 {
                schema_version: BUNDLE_SCHEMA_V2.into(),
                source_manifest_sha256: "a".repeat(64),
                final_manifest_sha256: "b".repeat(64),
                recovery_artifact_sha256: "c".repeat(64),
                recovery_artifact_size: 1,
                entries: 1,
                logical_bytes: 1,
            },
        };
        store.persist_exported(&checkpoint).unwrap();
        store.persist_exported(&checkpoint).unwrap();
        let mut conflicting = checkpoint.clone();
        conflicting.bundle.recovery_artifact_sha256 = "d".repeat(64);
        assert!(store.persist_exported(&conflicting).is_err());
        let path = checkpoints.join("fixture-001-S0-1.exported.json");
        fs::write(&path, b"{\"schema_version\":\"wrong\"}").unwrap();
        #[cfg(unix)]
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(store.load_exported("fixture-001-S0-1").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn checkpoint_store_refuses_a_symlink_root() {
        use std::os::unix::fs::symlink;

        let (_outer, _private, artifacts) = private_layout();
        let real = artifacts.join("real-checkpoints");
        fs::create_dir(&real).unwrap();
        fs::set_permissions(&real, fs::Permissions::from_mode(0o700)).unwrap();
        let linked = artifacts.join("linked-checkpoints");
        symlink(&real, &linked).unwrap();
        assert!(RecoveryCheckpointStore::create(&linked).is_err());
    }
}
