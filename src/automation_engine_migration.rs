//! Recoverable, byte-preserving imports. Migration never edits the source or
//! activates an automation; legacy identities remain authoritative.
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};

const MAX_FILES: usize = 20_000;
const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 512 * 1024 * 1024;
const MAX_CONFIG_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct MigrationFile {
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
    pub mode: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct MigrationPreview {
    pub schema: String,
    pub source: PathBuf,
    pub destination: PathBuf,
    pub source_is_file: bool,
    pub files: Vec<MigrationFile>,
    pub total_bytes: u64,
    pub absolute_bindings: Vec<String>,
    pub route: String,
    pub activation_changed: bool,
    pub digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct MigrationDoctor {
    pub schema: String,
    pub repository: Option<PathBuf>,
    pub git_available: bool,
    pub delegate_profile: Option<PathBuf>,
    pub issues: Vec<String>,
}

pub(crate) fn preview(source: &Path, destination: &Path) -> Result<MigrationPreview> {
    let original = fs::symlink_metadata(source).context("inspect legacy automation source")?;
    if original.file_type().is_symlink() {
        bail!("migration source cannot be a symlink");
    }
    let source = source
        .canonicalize()
        .context("resolve legacy automation source")?;
    let destination = resolve_destination(destination)?;
    if destination.starts_with(&source) || source.starts_with(&destination) {
        bail!("migration source and destination must not overlap");
    }
    let source_is_file = original.is_file();
    if !source_is_file && !original.is_dir() {
        bail!("migration source must be a regular file or directory");
    }
    let mut files = Vec::new();
    let mut total_bytes = 0;
    let mut absolute_bindings = Vec::new();
    let base = if source_is_file {
        source.parent().context("migration file has no parent")?
    } else {
        &source
    };
    inventory(
        &source,
        base,
        &mut files,
        &mut total_bytes,
        &mut absolute_bindings,
    )?;
    if files.is_empty() {
        bail!("legacy migration source contains no records");
    }
    files.sort_by(|left, right| left.path.cmp(&right.path));
    absolute_bindings.sort();
    absolute_bindings.dedup();
    let mut receipt = MigrationPreview {
        schema: "ca.automation-config-migration.v1".into(),
        source,
        destination,
        source_is_file,
        files,
        total_bytes,
        route: if absolute_bindings.is_empty() {
            "compatibility_copy_ready"
        } else {
            "compatibility_original_bindings_retained"
        }
        .into(),
        absolute_bindings,
        activation_changed: false,
        digest: String::new(),
    };
    receipt.digest = digest(&receipt)?;
    Ok(receipt)
}

/// Copy an exact approved snapshot into an immutable digest-named import. The
/// unchanged source is the rollback path. Absolute bindings keep legacy routing.
pub(crate) fn apply(
    source: &Path,
    destination: &Path,
    expected_digest: &str,
) -> Result<MigrationPreview> {
    let receipt = preview(source, destination)?;
    if expected_digest != receipt.digest {
        bail!("migration preview changed; review a fresh digest before copying");
    }
    ca_automation::runtime::private_dir(&receipt.destination)?;
    let _lease = ca_automation::runtime::ResourceLease::acquire(&receipt.destination, "migration")?;
    let target = receipt.destination.join(&receipt.digest);
    if target.exists() {
        validate_copy(&target, &receipt)?;
        return Ok(receipt);
    }
    let staging =
        receipt
            .destination
            .join(format!(".copy-{}-{}", std::process::id(), receipt.digest));
    if staging.exists() {
        bail!("interrupted migration copy exists; preserve it and inspect before retrying");
    }
    fs::create_dir(&staging)?;
    #[cfg(unix)]
    fs::set_permissions(&staging, fs::Permissions::from_mode(0o700))?;
    let source_base = if receipt.source_is_file {
        receipt
            .source
            .parent()
            .context("migration file has no parent")?
    } else {
        &receipt.source
    };
    let result = (|| {
        for record in &receipt.files {
            let input = source_base.join(&record.path);
            let output = staging.join(&record.path);
            let parent = output.parent().context("migration entry has no parent")?;
            ca_automation::runtime::private_dir(parent)?;
            let bytes = read_regular(&input, MAX_FILE_BYTES)?;
            if bytes.len() as u64 != record.bytes || hex_digest(&bytes) != record.sha256 {
                bail!("legacy source changed while copying {}", record.path);
            }
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            options
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
            let mut file = options.open(&output)?;
            file.write_all(&bytes)?;
            #[cfg(unix)]
            file.set_permissions(fs::Permissions::from_mode(record.mode))?;
            file.sync_all()?;
        }
        validate_copy(&staging, &receipt)?;
        if preview(&receipt.source, &receipt.destination)?.digest != receipt.digest {
            bail!("legacy source changed during migration; source and staged copy are preserved");
        }
        fs::rename(&staging, &target)?;
        File::open(&receipt.destination)?.sync_all()?;
        ca_automation::runtime::atomic_write(
            &receipt
                .destination
                .join(format!("{}.receipt.json", receipt.digest)),
            &receipt,
        )?;
        Ok(receipt.clone())
    })();
    // A failed partial copy is retained deliberately for inspection and recovery.
    result
}

pub(crate) fn doctor(
    repository: Option<&Path>,
    delegate_profile: Option<&Path>,
) -> Result<MigrationDoctor> {
    let mut receipt = MigrationDoctor {
        schema: "ca.automation-engine-doctor.v1".into(),
        repository: repository.map(Path::to_path_buf),
        git_available: false,
        delegate_profile: delegate_profile.map(Path::to_path_buf),
        issues: Vec::new(),
    };
    if let Some(repository) = repository {
        if !repository.is_dir() {
            receipt.issues.push(format!(
                "repository binding is unavailable: {}",
                repository.display()
            ));
        } else {
            let output = std::process::Command::new("git")
                .args(["rev-parse", "--absolute-git-dir"])
                .current_dir(repository)
                .stdin(std::process::Stdio::null())
                .output();
            receipt.git_available = output.is_ok_and(|output| output.status.success());
            if !receipt.git_available {
                receipt.issues.push(format!(
                    "Git binding is unavailable: {}",
                    repository.display()
                ));
            }
        }
    }
    if let Some(profile) = delegate_profile {
        match read_regular(profile, MAX_CONFIG_BYTES) {
            Ok(bytes) => {
                let document: toml::Value = toml::from_str(std::str::from_utf8(&bytes)?)
                    .context("parse delegate profile")?;
                let mut bindings = Vec::new();
                repository_bindings(&document, &mut bindings);
                for binding in bindings {
                    if !Path::new(&binding).is_dir() {
                        receipt.issues.push(format!(
                            "delegate repository binding is unavailable: {binding}"
                        ));
                    }
                }
            }
            Err(_) => receipt.issues.push(format!(
                "delegate profile is unavailable: {}",
                profile.display()
            )),
        }
    }
    receipt.issues.sort();
    receipt.issues.dedup();
    Ok(receipt)
}

/// Install only the existing workflow V1 namespaces. Run journals, current ca
/// permissions and enabled flags are never synthesized or overwritten.
pub(crate) fn install_workflow_records(
    import: &MigrationPreview,
    native_state_root: &Path,
    lease_root: &Path,
) -> Result<usize> {
    let imported_root = import.destination.join(&import.digest);
    validate_copy(&imported_root, import)?;
    let namespaces = [
        "workflows/v1/",
        "workflow-policies/v1/",
        "workflow-previews/v1/",
        "workflow-feedback/v1/",
        "workflow-checkpoints/v1/",
    ];
    let records = import
        .files
        .iter()
        .filter(|record| {
            namespaces
                .iter()
                .any(|namespace| record.path.starts_with(namespace))
        })
        .collect::<Vec<_>>();
    if records.is_empty() {
        return Ok(0);
    }
    ca_automation::runtime::private_dir(native_state_root)?;
    let _resource =
        ca_automation::runtime::ResourceLease::acquire(lease_root, "typed-workflow-mutation")?;
    let _installation =
        ca_automation::runtime::ResourceLease::acquire(native_state_root, "workflow-migration")?;
    // Check the complete target before copying any approval or pending intent.
    for record in &records {
        let destination = native_state_root.join(&record.path);
        match fs::symlink_metadata(&destination) {
            Ok(metadata) => {
                let bytes = read_regular(&destination, MAX_FILE_BYTES)?;
                #[cfg(unix)]
                let mode = metadata.mode() & 0o777;
                #[cfg(not(unix))]
                let mode = 0;
                if hex_digest(&bytes) != record.sha256 || mode != record.mode {
                    bail!(
                        "native workflow record conflicts with imported authority: {}",
                        record.path
                    );
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    let journal_path = native_state_root.join("workflow-migration.json");
    ca_automation::runtime::atomic_write(
        &journal_path,
        &serde_json::json!({
            "schema":"ca.workflow-migration-install.v1", "phase":"prepared", "import":import.digest,
            "source":import.source,"records":records,
        }),
    )?;
    for record in &records {
        let destination = native_state_root.join(&record.path);
        if destination.exists() {
            continue;
        }
        ca_automation::runtime::private_dir(
            destination
                .parent()
                .context("workflow record has no parent")?,
        )?;
        let bytes = read_reviewed_record(&imported_root, record)?;
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
        let parent = destination
            .parent()
            .context("workflow record has no parent")?;
        let temporary = parent.join(format!(".workflow-import-{}", uuid::Uuid::new_v4()));
        let result = (|| {
            let mut file = options.open(&temporary)?;
            file.write_all(&bytes)?;
            #[cfg(unix)]
            file.set_permissions(fs::Permissions::from_mode(record.mode))?;
            file.sync_all()?;
            // Publish sealed bytes atomically without an overwrite or a
            // crash window that leaves an authority record with two links.
            publish_workflow_record(&temporary, &destination)?;
            File::open(parent)?.sync_all()?;
            Ok::<_, anyhow::Error>(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result?;
    }
    validate_copy(&imported_root, import)?;
    ca_automation::runtime::atomic_write(
        &journal_path,
        &serde_json::json!({
            "schema":"ca.workflow-migration-install.v1", "phase":"confirmed", "import":import.digest,
            "source":import.source,"records":records,
        }),
    )?;
    Ok(records.len())
}

fn publish_workflow_record(temporary: &Path, destination: &Path) -> Result<()> {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        let temporary = CString::new(temporary.as_os_str().as_bytes())?;
        let destination = CString::new(destination.as_os_str().as_bytes())?;
        #[cfg(target_os = "macos")]
        let status = unsafe {
            libc::renamex_np(temporary.as_ptr(), destination.as_ptr(), libc::RENAME_EXCL)
        };
        #[cfg(target_os = "linux")]
        let status = unsafe {
            libc::renameat2(
                libc::AT_FDCWD,
                temporary.as_ptr(),
                libc::AT_FDCWD,
                destination.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        };
        if status != 0 {
            return Err(std::io::Error::last_os_error())
                .context("publish workflow authority without replacement");
        }
        Ok(())
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = (temporary, destination);
        bail!("atomic workflow authority publication is unavailable on this platform")
    }
}

fn inventory(
    path: &Path,
    base: &Path,
    files: &mut Vec<MigrationFile>,
    total: &mut u64,
    bindings: &mut Vec<String>,
) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        bail!("legacy migration contains a symlink: {}", path.display());
    }
    if metadata.is_dir() {
        let mut children = fs::read_dir(path)?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<std::io::Result<Vec<_>>>()?;
        children.sort();
        for child in children {
            inventory(&child, base, files, total, bindings)?;
        }
        return Ok(());
    }
    if files.len() >= MAX_FILES {
        bail!("legacy migration exceeds its file bound");
    }
    let relative = path
        .strip_prefix(base)?
        .to_str()
        .context("migration path is not UTF-8")?
        .to_string();
    if relative.len() > 4096
        || Path::new(&relative)
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        bail!("legacy migration entry has an invalid path");
    }
    let bytes = read_regular(path, MAX_FILE_BYTES)?;
    *total = total
        .checked_add(bytes.len() as u64)
        .context("migration size overflow")?;
    if *total > MAX_TOTAL_BYTES {
        bail!("legacy migration exceeds its storage bound");
    }
    if bytes.len() as u64 <= MAX_CONFIG_BYTES {
        collect_absolute_bindings(&relative, &bytes, bindings);
    }
    #[cfg(unix)]
    let mode = metadata.mode() & 0o777;
    #[cfg(not(unix))]
    let mode = 0;
    files.push(MigrationFile {
        path: relative,
        bytes: bytes.len() as u64,
        sha256: hex_digest(&bytes),
        mode,
    });
    Ok(())
}

fn validate_copy(root: &Path, receipt: &MigrationPreview) -> Result<()> {
    let mut files = Vec::new();
    inventory(root, root, &mut files, &mut 0, &mut Vec::new())?;
    files.sort_by(|left, right| left.path.cmp(&right.path));
    if files != receipt.files {
        bail!("migration copy does not match the reviewed legacy snapshot");
    }
    Ok(())
}

fn read_reviewed_record(root: &Path, record: &MigrationFile) -> Result<Vec<u8>> {
    let bytes = read_regular(&root.join(&record.path), MAX_FILE_BYTES)?;
    if bytes.len() as u64 != record.bytes || hex_digest(&bytes) != record.sha256 {
        bail!(
            "reviewed migration archive changed while installing {}",
            record.path
        );
    }
    Ok(bytes)
}

fn read_regular(path: &Path, maximum: u64) -> Result<Vec<u8>> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK);
    let file = options.open(path)?;
    let before = file.metadata()?;
    if !before.is_file() || before.len() > maximum {
        bail!("migration record is not a bounded regular file");
    }
    #[cfg(unix)]
    if before.nlink() != 1 {
        bail!("migration record has multiple links");
    }
    let mut bytes = Vec::new();
    file.take(maximum + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > maximum {
        bail!("migration record exceeded its size bound");
    }
    Ok(bytes)
}

fn resolve_destination(path: &Path) -> Result<PathBuf> {
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir))
    {
        bail!("migration destination must be an absolute path without traversal");
    }
    let mut existing = path.to_path_buf();
    let mut suffix = Vec::new();
    while !existing.exists() {
        suffix.push(
            existing
                .file_name()
                .context("migration destination has no existing parent")?
                .to_os_string(),
        );
        existing.pop();
    }
    let mut resolved = existing.canonicalize()?;
    for part in suffix.into_iter().rev() {
        resolved.push(part);
    }
    Ok(resolved)
}

fn collect_absolute_bindings(record: &str, bytes: &[u8], bindings: &mut Vec<String>) {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return;
    };
    let values: Option<serde_json::Value> = match Path::new(record)
        .extension()
        .and_then(|value| value.to_str())
    {
        Some("json") => serde_json::from_str(text).ok(),
        Some("toml") => toml::from_str::<toml::Value>(text)
            .ok()
            .and_then(|value| serde_json::to_value(value).ok()),
        _ => None,
    };
    fn walk(value: &serde_json::Value, record: &str, bindings: &mut Vec<String>) {
        match value {
            serde_json::Value::String(value) if Path::new(value).is_absolute() => {
                if bindings.len() < MAX_FILES {
                    bindings.push(format!("{record}: {value}"));
                }
            }
            serde_json::Value::Array(values) => {
                for value in values {
                    walk(value, record, bindings);
                }
            }
            serde_json::Value::Object(values) => {
                for value in values.values() {
                    walk(value, record, bindings);
                }
            }
            _ => {}
        }
    }
    if let Some(value) = values {
        walk(&value, record, bindings);
    }
}

fn repository_bindings(value: &toml::Value, bindings: &mut Vec<String>) {
    match value {
        toml::Value::Table(table) => {
            for (key, value) in table {
                if matches!(key.as_str(), "repo" | "repository" | "repository_path") {
                    if let Some(value) = value.as_str() {
                        bindings.push(value.into());
                    }
                }
                if key == "args" {
                    if let Some(args) = value.as_array() {
                        for pair in args.windows(2) {
                            if pair[0].as_str() == Some("--repo") {
                                if let Some(path) = pair[1].as_str() {
                                    bindings.push(path.into());
                                }
                            }
                        }
                    }
                }
                repository_bindings(value, bindings);
            }
        }
        toml::Value::Array(values) => {
            for value in values {
                repository_bindings(value, bindings);
            }
        }
        _ => {}
    }
}

fn digest(value: &impl Serialize) -> Result<String> {
    Ok(hex_digest(&serde_json::to_vec(value)?))
}
fn hex_digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn workflow_record_publication_is_atomic_single_link_and_never_overwrites() -> Result<()> {
        let root = tempfile::tempdir()?;
        let staging = root.path().join("staging");
        let destination = root.path().join("authority");
        fs::write(&staging, b"approved original bytes")?;
        publish_workflow_record(&staging, &destination)?;
        assert!(!staging.exists());
        assert_eq!(fs::metadata(&destination)?.nlink(), 1);
        fs::write(&staging, b"widened permissions")?;
        assert!(publish_workflow_record(&staging, &destination).is_err());
        assert_eq!(fs::read(&destination)?, b"approved original bytes");
        assert_eq!(fs::read(&staging)?, b"widened permissions");
        Ok(())
    }
    #[test]
    fn copies_legacy_authority_exactly_and_retains_unresolved_source_routing() -> Result<()> {
        let temporary = tempfile::tempdir()?;
        let root = temporary.path().canonicalize()?;
        let source = root.join("legacy");
        fs::create_dir(&source)?;
        fs::write(
            source.join("workflow.toml"),
            "enabled = false\nrepo = '/old/repository'\n",
        )?;
        fs::write(
            source.join("approval.json"),
            br#"{"schema":"codex-team.workflow_approved.v1","digest":"unchanged","enabled":false}"#,
        )?;
        fs::write(source.join("vault.bin"), [0, 1, 255, 128])?;
        let destination = root.join("imports");
        let reviewed = preview(&source, &destination)?;
        assert_eq!(reviewed.route, "compatibility_original_bindings_retained");
        assert!(!reviewed.activation_changed);
        let applied = apply(&source, &destination, &reviewed.digest)?;
        for entry in &reviewed.files {
            assert_eq!(
                fs::read(source.join(&entry.path))?,
                fs::read(destination.join(&applied.digest).join(&entry.path))?
            );
        }
        assert_eq!(apply(&source, &destination, &reviewed.digest)?, applied);
        Ok(())
    }
    #[test]
    fn stale_preview_is_rejected_without_creating_destination() -> Result<()> {
        let temporary = tempfile::tempdir()?;
        let root = temporary.path().canonicalize()?;
        let source = root.join("legacy.json");
        fs::write(&source, "{\"enabled\":false}")?;
        let destination = root.join("imports");
        let receipt = preview(&source, &destination)?;
        fs::write(&source, "{\"enabled\":true}")?;
        assert!(apply(&source, &destination, &receipt.digest).is_err());
        assert!(!destination.exists());
        Ok(())
    }
    #[test]
    fn reviewed_archive_record_is_rechecked_when_read_for_installation() -> Result<()> {
        let temporary = tempfile::tempdir()?;
        let root = temporary.path().canonicalize()?;
        let source = root.join("legacy");
        fs::create_dir_all(source.join("workflows/v1"))?;
        fs::write(
            source.join("workflows/v1/approved.json"),
            b"reviewed authority",
        )?;
        let destination = root.join("imports");
        let reviewed = preview(&source, &destination)?;
        let import = apply(&source, &destination, &reviewed.digest)?;
        let archive = destination.join(&import.digest);
        validate_copy(&archive, &import)?;
        let record = &import.files[0];
        assert_eq!(
            read_reviewed_record(&archive, record)?,
            b"reviewed authority"
        );
        // Model a real archive mutation after complete preflight and before
        // the record is reread for publication. Matching length is insufficient.
        fs::write(archive.join(&record.path), b"modified authority")?;
        assert!(read_reviewed_record(&archive, record).is_err());
        assert!(validate_copy(&archive, &import).is_err());
        assert_eq!(fs::read(source.join(&record.path))?, b"reviewed authority");
        Ok(())
    }
    #[tokio::test]
    async fn installs_v1_namespaces_exactly_and_rejects_conflicting_approvals() -> Result<()> {
        let temporary = tempfile::tempdir()?;
        let root = temporary.path().canonicalize()?;
        let source = root.join("legacy");
        fs::create_dir_all(source.join("workflows/v1"))?;
        let bytes = serde_json::to_vec(&serde_json::json!({
            "schema":"codex-team.approved_workflow.v1", "workflow_id":"disabled",
            "workflow_digest":"a".repeat(64), "policy_digest":"b".repeat(64),
            "resource_binding_digest":"c".repeat(64), "first_run_plan_digest":"d".repeat(64),
            "enabled":false,"approved_at":"2026-10-06T00:00:00Z",
            "draft":{"schema":"codex-team.workflow_draft.v1","workflow_id":"disabled",
                "title":"Disabled legacy fixture","actions":[{"id":"read","action":{"kind":"probe_ssh","host":"fixture"}}],
                "limits":{"max_runtime_seconds":30,"min_free_space_bytes":0,"deletion_allowed":false,
                    "conflict_policy":"keep_both","verification":[]},
                "rollback":{"kind":"unavailable","limitation":"Read-only fixture"}},
        }))?;
        fs::write(source.join("workflows/v1/disabled.json"), &bytes)?;
        #[cfg(unix)]
        fs::set_permissions(
            source.join("workflows/v1/disabled.json"),
            fs::Permissions::from_mode(0o600),
        )?;
        let destination = root.join("imports");
        let receipt = preview(&source, &destination)?;
        let import = apply(&source, &destination, &receipt.digest)?;
        let state = root.join("native");
        let leases = root.join("leases");
        assert_eq!(install_workflow_records(&import, &state, &leases)?, 1);
        assert_eq!(fs::read(state.join("workflows/v1/disabled.json"))?, bytes);
        assert_eq!(install_workflow_records(&import, &state, &leases)?, 1);
        let response = ca_automation::automation_workflow::execute(
            &root.join("original-config.toml"),
            &state,
            ca_automation_protocol::WorkflowRequestV1 {
                schema: ca_automation_protocol::WORKFLOW_REQUEST_SCHEMA.into(),
                operation: ca_automation_protocol::WorkflowOperationV1::List { workflow_id: None },
            },
        )
        .await?;
        let ca_automation_protocol::WorkflowResponseV1::List { workflows } = response else {
            bail!("expected native workflow list");
        };
        assert_eq!(workflows.len(), 1);
        assert_eq!(workflows[0].workflow_id, "disabled");
        assert!(!workflows[0].enabled);
        assert_eq!(workflows[0].workflow_digest, "a".repeat(64));
        fs::write(
            state.join("workflows/v1/disabled.json"),
            b"changed authority",
        )?;
        assert!(install_workflow_records(&import, &state, &leases).is_err());
        assert_eq!(
            fs::read(state.join("workflows/v1/disabled.json"))?,
            b"changed authority"
        );
        Ok(())
    }
    #[cfg(unix)]
    #[test]
    fn migration_refuses_links_and_source_destination_overlap() -> Result<()> {
        let temporary = tempfile::tempdir()?;
        let root = temporary.path().canonicalize()?;
        let source = root.join("legacy");
        fs::create_dir(&source)?;
        fs::write(source.join("record"), "state")?;
        assert!(preview(&source, &source.join("imports")).is_err());
        std::os::unix::fs::symlink(source.join("record"), source.join("linked"))?;
        assert!(preview(&source, &root.join("imports")).is_err());
        Ok(())
    }
    #[test]
    fn doctor_surfaces_unavailable_git_and_stale_delegate_repository() -> Result<()> {
        let temporary = tempfile::tempdir()?;
        let root = temporary.path().canonicalize()?;
        let repository = root.join("repository");
        fs::create_dir(&repository)?;
        let profile = root.join("delegate.toml");
        fs::write(
            &profile,
            "[mcp.servers.delegate]\nargs = ['mcp','--repo','/missing/legacy-ca']\n",
        )?;
        let receipt = doctor(Some(&repository), Some(&profile))?;
        assert!(!receipt.git_available);
        assert_eq!(receipt.issues.len(), 2);
        Ok(())
    }
}
