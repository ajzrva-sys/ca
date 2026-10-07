//! Explicit, recoverable vault copying. Frozen absolute bindings are preserved;
//! pending legacy work continues at its original authority root until settled.
use anyhow::{bail, Context, Result};
use codex_team::self_improvement::{canonical_digest, list_harness_cycles, LearningVault};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::Read;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

const LINK: &str = ".ca-harness-migration.json";
const MAX_BYTES: u64 = 16 * 1024 * 1024 * 1024;
const MAX_FILE_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct VaultFile {
    pub bytes: u64,
    pub sha256: String,
    pub mode: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct VaultMigrationPreview {
    pub schema: String,
    pub source: PathBuf,
    pub destination: PathBuf,
    pub files: BTreeMap<PathBuf, VaultFile>,
    pub pending_cycles: Vec<String>,
    pub digest: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MigrationJournal {
    preview: VaultMigrationPreview,
    staging: PathBuf,
    published: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MigrationLink {
    preview: VaultMigrationPreview,
    source_active: bool,
    copied_files: BTreeMap<PathBuf, VaultFile>,
}

fn normalize_missing(path: &Path) -> Result<PathBuf> {
    if !path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        bail!("vault paths must be absolute without parent traversal");
    }
    let mut ancestor = path;
    let mut tail = Vec::new();
    while !ancestor.exists() {
        tail.push(
            ancestor
                .file_name()
                .context("vault path has no existing parent")?
                .to_owned(),
        );
        ancestor = ancestor
            .parent()
            .context("vault path has no existing parent")?;
    }
    let mut result = ancestor.canonicalize()?;
    // A symlinked caller path is not silently accepted as another authority root.
    if std::fs::symlink_metadata(ancestor)?
        .file_type()
        .is_symlink()
    {
        bail!("vault root or nearest existing parent is a symbolic link");
    }
    for component in tail.into_iter().rev() {
        result.push(component);
    }
    Ok(result)
}
fn file_digest(path: &Path) -> Result<VaultFile> {
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > MAX_FILE_BYTES {
        bail!("vault migration requires bounded regular files");
    }
    let mut hash = Sha256::new();
    let mut bytes = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        bytes = bytes
            .checked_add(n as u64)
            .context("vault file size overflow")?;
        if bytes > MAX_FILE_BYTES {
            bail!("vault file changed beyond migration bound");
        }
        hash.update(&buffer[..n]);
    }
    if bytes != metadata.len() {
        bail!("vault file changed during migration preview");
    }
    Ok(VaultFile {
        bytes,
        sha256: format!("{:x}", hash.finalize()),
        mode: metadata.permissions().mode() & 0o777,
    })
}
fn inventory(root: &Path) -> Result<BTreeMap<PathBuf, VaultFile>> {
    fn walk(
        root: &Path,
        relative: &Path,
        entries: &mut BTreeMap<PathBuf, VaultFile>,
        total: &mut u64,
        depth: usize,
    ) -> Result<()> {
        if depth > 64 {
            bail!("vault migration directory depth exceeded");
        }
        for entry in std::fs::read_dir(root.join(relative))? {
            let entry = entry?;
            let path = relative.join(entry.file_name());
            if path == Path::new("locks") || path == Path::new(LINK) {
                continue;
            }
            let metadata = std::fs::symlink_metadata(entry.path())?;
            if metadata.file_type().is_symlink() {
                bail!(
                    "vault migration does not follow symbolic links: {}",
                    path.display()
                );
            }
            if metadata.is_dir() {
                walk(root, &path, entries, total, depth + 1)?;
            } else {
                let file = file_digest(&entry.path())?;
                *total = total
                    .checked_add(file.bytes)
                    .context("vault size overflow")?;
                if *total > MAX_BYTES || entries.len() >= 200_000 {
                    bail!("vault migration inventory bound exceeded");
                }
                entries.insert(path, file);
            }
        }
        Ok(())
    }
    let mut files = BTreeMap::new();
    walk(root, Path::new(""), &mut files, &mut 0, 0)?;
    Ok(files)
}
fn validate_preview(preview: &VaultMigrationPreview) -> Result<()> {
    let mut unsigned = preview.clone();
    unsigned.digest.clear();
    if preview.schema != "ca.harness-vault-migration.v1"
        || canonical_digest(&unsigned)? != preview.digest
        || normalize_missing(&preview.source)? != preview.source
        || normalize_missing(&preview.destination)? != preview.destination
        || preview.source.starts_with(&preview.destination)
        || preview.destination.starts_with(&preview.source)
        || preview.files.keys().any(|path| {
            path.is_absolute()
                || path
                    .components()
                    .any(|c| !matches!(c, std::path::Component::Normal(_)))
        })
    {
        bail!("vault migration authority changed");
    }
    Ok(())
}
pub fn preview_vault_migration(source: &Path, destination: &Path) -> Result<VaultMigrationPreview> {
    let source = normalize_missing(source)?;
    let destination = normalize_missing(destination)?;
    if destination.exists() {
        bail!(
            "migration destination already exists; use migration recovery for an interrupted copy"
        );
    }
    let vault = LearningVault::existing_at(source.clone())?
        .context("source harness vault is unavailable")?;
    let mut preview = VaultMigrationPreview {
        schema: "ca.harness-vault-migration.v1".into(),
        source,
        destination,
        files: inventory(vault.root())?,
        pending_cycles: list_harness_cycles(&vault)?
            .into_iter()
            .filter(crate::harness_control::blocks_new_cycle)
            .map(|cycle| cycle.id)
            .collect(),
        digest: String::new(),
    };
    preview.pending_cycles.sort();
    preview.digest = canonical_digest(&preview)?;
    validate_preview(&preview)?;
    Ok(preview)
}
fn read_link(destination: &Path) -> Result<Option<MigrationLink>> {
    let path = destination.join(LINK);
    if !path.exists() {
        return Ok(None);
    }
    let link: MigrationLink = ca_automation::runtime::read(&path, 64 * 1024 * 1024)?;
    validate_preview(&link.preview)?;
    if normalize_missing(destination)? != link.preview.destination {
        bail!("migration profile destination changed");
    }
    Ok(Some(link))
}
pub fn migration_source(destination: &Path) -> Result<Option<PathBuf>> {
    Ok(read_link(destination)?
        .filter(|link| link.source_active)
        .map(|link| link.preview.source))
}
fn copy_files(
    source: &Path,
    destination: &Path,
    files: &BTreeMap<PathBuf, VaultFile>,
) -> Result<()> {
    for (relative, expected) in files {
        let target = destination.join(relative);
        if target.exists() && file_digest(&target)? == *expected {
            continue;
        }
        let parent = target.parent().context("migration target parent missing")?;
        ca_automation::runtime::private_dir(parent)?;
        let temporary = parent.join(format!(".migration-{}", uuid::Uuid::new_v4()));
        let mut input = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(source.join(relative))?;
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&temporary)?;
        let copied = std::io::copy(
            &mut Read::by_ref(&mut input).take(MAX_FILE_BYTES + 1),
            &mut output,
        )?;
        output.sync_all()?;
        if copied != expected.bytes || file_digest(&temporary)?.sha256 != expected.sha256 {
            bail!("source vault changed while copying");
        }
        std::fs::set_permissions(&temporary, std::fs::Permissions::from_mode(expected.mode))?;
        std::fs::rename(&temporary, &target)?;
        File::open(parent)?.sync_all()?;
    }
    Ok(())
}
fn validate_copy(destination: &Path, files: &BTreeMap<PathBuf, VaultFile>) -> Result<()> {
    for (relative, expected) in files {
        if file_digest(&destination.join(relative))? != *expected {
            bail!("copied vault did not validate");
        }
    }
    let vault =
        LearningVault::existing_at(destination.into())?.context("copied vault unavailable")?;
    list_harness_cycles(&vault)?;
    Ok(())
}
pub fn apply_vault_migration(
    source: &Path,
    destination: &Path,
    expected_digest: &str,
) -> Result<VaultMigrationPreview> {
    let source = normalize_missing(source)?;
    let destination = normalize_missing(destination)?;
    let parent = destination.parent().context("destination parent missing")?;
    let suffix = format!("harness-vault-migration-{expected_digest}");
    if expected_digest.len() != 64 || !expected_digest.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        bail!("exact vault migration digest required");
    }
    let journal_path = parent.join(format!(".{suffix}.json"));
    if let Some(link) = read_link(&destination)? {
        if link.preview.digest != expected_digest || link.preview.source != source {
            bail!("published migration identity changed");
        }
        validate_copy(&destination, &link.copied_files)?;
        return Ok(link.preview);
    }
    let vault = LearningVault::existing_at(source.clone())?.context("source vault unavailable")?;
    let _controller = vault.acquire_harness_controller_lock()?;
    let _worker = vault.acquire_worker_lock()?;
    let _records = vault.acquire_lock()?;
    let mut journal = if journal_path.exists() {
        ca_automation::runtime::read::<MigrationJournal>(&journal_path, 64 * 1024 * 1024)?
    } else {
        let preview = preview_vault_migration(&source, &destination)?;
        if preview.digest != expected_digest {
            bail!(
                "vault changed since migration preview; exact digest required: {}",
                preview.digest
            );
        }
        ca_automation::runtime::private_dir(parent)
            .context("migration destination parent must be private")?;
        let journal = MigrationJournal {
            preview,
            staging: parent.join(format!(".{suffix}.staging")),
            published: false,
        };
        ca_automation::runtime::atomic_write(&journal_path, &journal)?;
        journal
    };
    validate_preview(&journal.preview)?;
    if journal.preview.source != source
        || journal.preview.destination != destination
        || journal.preview.digest != expected_digest
        || journal.staging != parent.join(format!(".{suffix}.staging"))
        || inventory(&source)? != journal.preview.files
    {
        bail!(
            "interrupted migration authority or source changed; original and partial copy retained"
        );
    }
    ca_automation::runtime::private_dir(&journal.staging)?;
    copy_files(&source, &journal.staging, &journal.preview.files)?;
    LearningVault::open_at(journal.staging.clone())?;
    validate_copy(&journal.staging, &journal.preview.files)?;
    ca_automation::runtime::atomic_write(
        &journal.staging.join(LINK),
        &MigrationLink {
            preview: journal.preview.clone(),
            source_active: !journal.preview.pending_cycles.is_empty(),
            copied_files: journal.preview.files.clone(),
        },
    )?;
    if inventory(&source)? != journal.preview.files {
        bail!("source changed before migration publication; copies retained");
    }
    std::fs::rename(&journal.staging, &destination)?;
    File::open(parent)?.sync_all()?;
    journal.published = true;
    ca_automation::runtime::atomic_write(&journal_path, &journal)?;
    Ok(journal.preview)
}

/// Pending work uses its original absolute bindings. When it becomes terminal,
/// sync its latest receipts before admitting any new profile-owned experiment.
pub(super) fn reconcile_vault_migration(destination: &Path) -> Result<PathBuf> {
    let destination = normalize_missing(destination)?;
    reconcile_canonical_vault_migration(&destination)
}
fn reconcile_canonical_vault_migration(destination: &Path) -> Result<PathBuf> {
    let Some(mut link) = read_link(destination)? else {
        return Ok(destination.into());
    };
    if !link.source_active {
        return Ok(destination.into());
    }
    let source = LearningVault::existing_at(link.preview.source.clone())?
        .context("pending legacy migration source is unavailable")?;
    let cycles = list_harness_cycles(&source)?;
    if link.preview.pending_cycles.iter().any(|id| {
        cycles
            .iter()
            .any(|cycle| &cycle.id == id && crate::harness_control::blocks_new_cycle(cycle))
    }) {
        return Ok(link.preview.source);
    }
    let _controller = source.acquire_harness_controller_lock()?;
    let _worker = source.acquire_worker_lock()?;
    let _records = source.acquire_lock()?;
    // Destination has never admitted independent work while source_active=true.
    let current = inventory(source.root())?;
    let existing = inventory(destination)?;
    for (path, actual) in &existing {
        if link.copied_files.get(path) != Some(actual) && current.get(path) != Some(actual) {
            bail!(
                "migration copy changed independently; source and destination retained for review"
            );
        }
    }
    copy_files(source.root(), destination, &current)?;
    validate_copy(destination, &current)?;
    link.source_active = false;
    link.copied_files = current;
    ca_automation::runtime::atomic_write(&destination.join(LINK), &link)?;
    Ok(destination.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn migration_preserves_pending_identity_and_retries_without_rewriting_grants() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let vault = LearningVault::open_at(temp.path().join("legacy")).unwrap();
        let (plan, _, _) = crate::harness_generation::tests::inputs();
        codex_team::self_improvement::start_harness_cycle(&vault, &plan, 0).unwrap();
        let destination = temp.path().canonicalize().unwrap().join("native");
        let preview = preview_vault_migration(vault.root(), &destination).unwrap();
        assert!(!destination.exists());
        assert!(apply_vault_migration(vault.root(), &destination, &"0".repeat(64)).is_err());
        let applied = apply_vault_migration(vault.root(), &destination, &preview.digest).unwrap();
        assert_eq!(applied, preview);
        assert_eq!(
            migration_source(&destination).unwrap().as_deref(),
            Some(vault.root())
        );
        assert_eq!(
            reconcile_vault_migration(&destination).unwrap(),
            vault.root()
        );
        let copied = LearningVault::existing_at(destination.clone())
            .unwrap()
            .unwrap();
        assert_eq!(
            codex_team::self_improvement::load_harness_cycle(&copied, &plan.id)
                .unwrap()
                .0
                .digest,
            plan.digest
        );
        assert_eq!(
            apply_vault_migration(vault.root(), &destination, &preview.digest).unwrap(),
            preview
        );
        codex_team::self_improvement::update_harness_cycle(&vault, &plan.id, |cycle| {
            cycle.stage = codex_team::self_improvement::HarnessStage::Rejected;
            Ok(())
        })
        .unwrap();
        assert_eq!(
            reconcile_vault_migration(&destination).unwrap(),
            destination
        );
        assert!(migration_source(&destination).unwrap().is_none());
        assert_eq!(
            codex_team::self_improvement::load_harness_cycle(&copied, &plan.id)
                .unwrap()
                .1
                .stage,
            codex_team::self_improvement::HarnessStage::Rejected
        );
        assert!(vault.root().exists());
    }
}
