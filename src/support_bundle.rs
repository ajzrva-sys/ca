use crate::config::ConfigStore;
use crate::config_provenance::ConfigProvenanceReport;
use crate::doctor::{CheckLevel, DoctorReport};
use crate::runtime::{ActiveProfile, RuntimePaths};
use crate::security_audit::{AuditReport, AuditSeverity};
use chrono::{DateTime, Utc};
use clap::{CommandFactory, Parser, Subcommand};
use flate2::{Compression, GzBuilder};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub const SCHEMA: &str = "ca.support-bundle.v1";
pub const MAX_UNCOMPRESSED_BYTES: usize = 8 * 1024 * 1024;
const MAX_SINCE_SECONDS: u64 = 365 * 24 * 60 * 60;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupportBundleCliArgs {
    pub active_profile: ActiveProfile,
    pub output: Option<PathBuf>,
    pub since: Option<String>,
    pub json: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigExplainCliArgs {
    pub active_profile: ActiveProfile,
    pub selector: Option<String>,
    pub json: bool,
}

#[derive(Debug, Parser)]
#[command(
    name = "ca support-bundle",
    bin_name = "ca support-bundle",
    about = "Create a private, metadata-only local support archive"
)]
struct RawSupportBundleArgs {
    /// Write the archive to this new path instead of the private profile default
    #[arg(long, value_name = "FILE")]
    output: Option<PathBuf>,
    /// Retain runtime metadata no older than this duration (for example 2h or 7d)
    #[arg(long, value_name = "DURATION")]
    since: Option<String>,
    /// Emit the versioned result as JSON
    #[arg(long)]
    json: bool,
    /// Select the whole-file profile to diagnose
    #[arg(long, value_name = "NAME")]
    profile: Option<String>,
}

#[derive(Debug, Parser)]
#[command(
    name = "ca config",
    bin_name = "ca config",
    about = "Inspect effective local configuration metadata"
)]
struct RawConfigArgs {
    #[command(subcommand)]
    command: RawConfigCommand,
}

#[derive(Debug, Subcommand)]
enum RawConfigCommand {
    /// Explain where effective settings came from without showing values
    Explain {
        #[arg(value_name = "SELECTOR")]
        selector: Option<String>,
        #[arg(long)]
        json: bool,
        #[arg(long, value_name = "NAME")]
        profile: Option<String>,
    },
}

pub fn parse_support_bundle_args<I, T>(args: I) -> Result<SupportBundleCliArgs, clap::Error>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let raw = RawSupportBundleArgs::try_parse_from(args)?;
    let active_profile = ActiveProfile::parse(raw.profile.as_deref()).map_err(|error| {
        RawSupportBundleArgs::command()
            .error(clap::error::ErrorKind::InvalidValue, error.to_string())
    })?;
    if matches!(active_profile, ActiveProfile::Bare) {
        return Err(RawSupportBundleArgs::command().error(
            clap::error::ErrorKind::ArgumentConflict,
            "support bundles are unavailable with --profile bare",
        ));
    }
    Ok(SupportBundleCliArgs {
        active_profile,
        output: raw.output,
        since: raw.since,
        json: raw.json,
    })
}

pub fn parse_config_args<I, T>(args: I) -> Result<ConfigExplainCliArgs, clap::Error>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let raw = RawConfigArgs::try_parse_from(args)?;
    match raw.command {
        RawConfigCommand::Explain {
            selector,
            json,
            profile,
        } => {
            let active_profile = ActiveProfile::parse(profile.as_deref()).map_err(|error| {
                RawConfigArgs::command()
                    .error(clap::error::ErrorKind::InvalidValue, error.to_string())
            })?;
            Ok(ConfigExplainCliArgs {
                active_profile,
                selector,
                json,
            })
        }
    }
}

pub fn run_support_bundle_cli(cli: SupportBundleCliArgs) -> i32 {
    let now = Utc::now();
    let since = match cli.since.as_deref() {
        Some(value) => match parse_since(value, now) {
            Ok(value) => Some(value),
            Err(error) => {
                eprintln!("ca support-bundle: {error}");
                return 2;
            }
        },
        None => None,
    };
    let runtime_paths = match RuntimePaths::discover() {
        Ok(paths) => paths,
        Err(_) => {
            eprintln!("ca support-bundle: runtime paths are unavailable");
            return 2;
        }
    };
    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        Err(_) => {
            eprintln!("ca support-bundle: current directory is unavailable");
            return 2;
        }
    };
    let workspace = match crate::workspace::WorkspaceContext::discover(&cwd) {
        Ok(workspace) => workspace,
        Err(_) => {
            eprintln!("ca support-bundle: primary workspace is unavailable");
            return 2;
        }
    };
    let Some(primary) = workspace.primary() else {
        eprintln!("ca support-bundle: primary workspace is unavailable");
        return 2;
    };
    let plan = match plan(SupportBundleRequest {
        runtime_paths,
        profile: cli.active_profile,
        workspace_root: primary.path.clone(),
        cwd,
        output: cli.output,
        since,
        process_overrides: Vec::new(),
    }) {
        Ok(plan) => plan,
        Err(error) => {
            eprintln!("ca support-bundle: {error}");
            return 2;
        }
    };
    let result = match plan.write() {
        Ok(result) => result,
        Err(error) => {
            eprintln!("ca support-bundle: {error}");
            return 2;
        }
    };
    let output = if cli.json {
        serde_json::to_string_pretty(&result).map(|value| format!("{value}\n"))
    } else {
        Ok(format!(
            "Support bundle created\nOutput: {}\nEntries: {}\nUncompressed: {} bytes\nSHA-256: {}\n",
            result.output, result.entries, result.total_uncompressed_bytes, result.sha256
        ))
    };
    write_stdout(output.unwrap_or_else(|_| String::new()))
}

pub fn run_config_explain_cli(cli: ConfigExplainCliArgs) -> i32 {
    let paths = match RuntimePaths::discover() {
        Ok(paths) => paths,
        Err(_) => {
            eprintln!("ca config explain: runtime paths are unavailable");
            return 2;
        }
    };
    let report = match provenance_for_cli(&paths, &cli.active_profile, cli.selector.as_deref()) {
        Ok(report) => report,
        Err(error) => {
            eprintln!("ca config explain: {error}");
            return 2;
        }
    };
    let output = if cli.json {
        serde_json::to_string_pretty(&report).map(|value| format!("{value}\n"))
    } else {
        Ok(format!("{}\n", report.render_text()))
    };
    write_stdout(output.unwrap_or_else(|_| String::new()))
}

fn write_stdout(output: String) -> i32 {
    let mut stdout = std::io::stdout().lock();
    match stdout.write_all(output.as_bytes()) {
        Ok(()) => 0,
        Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => 0,
        Err(_) => 2,
    }
}

#[derive(Debug, Clone)]
pub struct SupportBundleRequest {
    pub runtime_paths: RuntimePaths,
    pub profile: ActiveProfile,
    pub workspace_root: PathBuf,
    pub cwd: PathBuf,
    pub output: Option<PathBuf>,
    pub since: Option<DateTime<Utc>>,
    pub process_overrides: Vec<String>,
}

#[derive(Clone)]
pub struct SupportBundlePlan {
    output: PathBuf,
    parent_identity: Option<FileIdentity>,
    allow_parent_creation: bool,
    entries: Vec<BundleEntry>,
    preview: SupportBundlePreview,
}

impl std::fmt::Debug for SupportBundlePlan {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SupportBundlePlan")
            .field("output", &"[PRIVATE PATH]")
            .field("entry_count", &self.entries.len())
            .field(
                "total_uncompressed_bytes",
                &self.preview.total_uncompressed_bytes,
            )
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupportBundleEntryPreview {
    pub name: String,
    pub bytes: usize,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupportBundlePreview {
    pub schema: String,
    pub profile: String,
    pub entries: Vec<SupportBundleEntryPreview>,
    pub total_uncompressed_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupportBundleResult {
    pub schema: String,
    pub output: String,
    pub entries: usize,
    pub total_uncompressed_bytes: usize,
    pub sha256: String,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SupportBundleError {
    #[error("support bundles are unavailable in a bare profile")]
    BareProfile,
    #[error("support bundle sources are unavailable or unsafe")]
    Source,
    #[error("support bundle output path is invalid")]
    InvalidOutput,
    #[error("support bundle output storage is unsafe")]
    UnsafeOutput,
    #[error("support bundle output already exists")]
    Collision,
    #[error("support bundle output changed after preview")]
    OutputDrift,
    #[error("support bundle exceeds the 8 MiB uncompressed limit")]
    TooLarge,
    #[error("support bundle could not be encoded")]
    Encode,
    #[error("support bundle could not be written atomically")]
    Write,
    #[error("invalid duration; use an integer followed by s, m, h, or d")]
    InvalidDuration,
}

#[derive(Clone)]
struct BundleEntry {
    name: String,
    bytes: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileIdentity {
    dev: u64,
    ino: u64,
}

#[derive(Serialize)]
struct Manifest<'a> {
    schema: &'static str,
    created_at: DateTime<Utc>,
    profile: &'a str,
    version: &'static str,
    platform: Platform,
    entries: &'a [SupportBundleEntryPreview],
    total_uncompressed_bytes: usize,
}

#[derive(Serialize)]
struct Platform {
    os: &'static str,
    arch: &'static str,
}

#[derive(Serialize)]
struct SafeDoctor {
    schema: &'static str,
    ready: bool,
    checks: BTreeMap<&'static str, usize>,
    provider_count: usize,
}

#[derive(Serialize)]
struct SafeAudit {
    schema: &'static str,
    profile: String,
    scanned_files: usize,
    scanned_bytes: usize,
    partial: bool,
    findings: Vec<SafeAuditFinding>,
}

#[derive(Serialize)]
struct SafeAuditFinding {
    severity: &'static str,
    rule_id: &'static str,
    path_hash: String,
}

pub fn plan(request: SupportBundleRequest) -> Result<SupportBundlePlan, SupportBundleError> {
    if matches!(request.profile, ActiveProfile::Bare) {
        return Err(SupportBundleError::BareProfile);
    }
    let profile_paths = request.runtime_paths.clone().for_profile(&request.profile);
    let store =
        ConfigStore::for_read_only_load(profile_paths.config_file.clone(), request.profile.clone());
    let (doctor, _) = crate::doctor::diagnose_offline(&profile_paths, &store);
    let audit = crate::security_audit::run(crate::security_audit::AuditRequest {
        workspace_root: request.workspace_root,
        cwd: request.cwd,
        runtime_paths: request.runtime_paths.clone(),
        profile: request.profile.clone(),
        loaded_config: None,
    })
    .map_err(|_| SupportBundleError::Source)?;
    let provenance = crate::config_provenance::inspect(
        &profile_paths.config_file,
        &request.profile,
        request.process_overrides,
        [
            "runtime.profile".to_string(),
            "runtime.workspace".to_string(),
        ],
    )
    .map_err(|_| SupportBundleError::Source)?;
    let runtime = crate::runtime_diagnostics::support_snapshot(
        &request.runtime_paths.runtime_log_dir(&request.profile),
        request.since,
    )
    .map_err(|_| SupportBundleError::Source)?;

    let (output, allow_parent_creation) =
        resolve_output(&request.runtime_paths, &request.profile, request.output)?;
    if std::fs::symlink_metadata(&output).is_ok() {
        return Err(SupportBundleError::Collision);
    }
    let parent_identity = output
        .parent()
        .and_then(|parent| safe_parent_identity(parent).transpose())
        .transpose()?;

    let mut entries = vec![
        json_entry("doctor.json", &safe_doctor(&doctor))?,
        json_entry("audit.json", &safe_audit(&audit))?,
        json_entry("config-provenance.json", &provenance)?,
        json_entry("runtime-health.json", &runtime.health)?,
    ];
    let mut log_bytes = Vec::new();
    for entry in &runtime.entries {
        serde_json::to_writer(&mut log_bytes, entry).map_err(|_| SupportBundleError::Encode)?;
        log_bytes.push(b'\n');
    }
    entries.push(BundleEntry {
        name: "runtime-logs.jsonl".into(),
        bytes: log_bytes,
    });
    entries.push(json_entry(
        "version.json",
        &serde_json::json!({
            "schema": SCHEMA,
            "version": env!("CARGO_PKG_VERSION"),
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "profile": request.profile.label(),
        }),
    )?);
    entries.sort_by(|left, right| left.name.cmp(&right.name));

    let payload_preview = preview_entries(&entries);
    let payload_bytes = entries.iter().try_fold(0usize, |total, entry| {
        total
            .checked_add(entry.bytes.len())
            .ok_or(SupportBundleError::TooLarge)
    })?;
    let manifest_entry = manifest_entry(
        request.profile.label(),
        Utc::now(),
        &payload_preview,
        payload_bytes,
    )?;
    let total_uncompressed_bytes = payload_bytes
        .checked_add(manifest_entry.bytes.len())
        .ok_or(SupportBundleError::TooLarge)?;
    if total_uncompressed_bytes > MAX_UNCOMPRESSED_BYTES {
        return Err(SupportBundleError::TooLarge);
    }
    entries.push(manifest_entry);
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    let preview = SupportBundlePreview {
        schema: SCHEMA.into(),
        profile: request.profile.label().into(),
        entries: preview_entries(&entries),
        total_uncompressed_bytes,
    };
    Ok(SupportBundlePlan {
        output,
        parent_identity,
        allow_parent_creation,
        entries,
        preview,
    })
}

impl SupportBundlePlan {
    pub fn preview(&self) -> &SupportBundlePreview {
        &self.preview
    }

    pub fn write(self) -> Result<SupportBundleResult, SupportBundleError> {
        let checked_total = self.entries.iter().try_fold(0usize, |total, entry| {
            total
                .checked_add(entry.bytes.len())
                .ok_or(SupportBundleError::TooLarge)
        })?;
        if checked_total != self.preview.total_uncompressed_bytes
            || checked_total > MAX_UNCOMPRESSED_BYTES
        {
            return Err(SupportBundleError::TooLarge);
        }
        let parent = self
            .output
            .parent()
            .ok_or(SupportBundleError::InvalidOutput)?;
        prepare_parent(parent, self.allow_parent_creation)?;
        let current_parent =
            safe_parent_identity(parent)?.ok_or(SupportBundleError::UnsafeOutput)?;
        if self
            .parent_identity
            .is_some_and(|expected| expected != current_parent)
        {
            return Err(SupportBundleError::OutputDrift);
        }
        if std::fs::symlink_metadata(&self.output).is_ok() {
            return Err(SupportBundleError::Collision);
        }
        let staging = parent.join(format!(".ca-support-{}.tmp", Uuid::new_v4()));
        let mut published = false;
        let result = (|| {
            let mut options = OpenOptions::new();
            options
                .write(true)
                .create_new(true)
                .mode(0o600)
                .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
            let file = options
                .open(&staging)
                .map_err(|_| SupportBundleError::Write)?;
            let mut gzip = GzBuilder::new()
                .mtime(0)
                .write(file, Compression::default());
            {
                let mut archive = tar::Builder::new(&mut gzip);
                for entry in &self.entries {
                    append_entry(&mut archive, entry)?;
                }
                archive.finish().map_err(|_| SupportBundleError::Write)?;
            }
            let mut file = gzip.finish().map_err(|_| SupportBundleError::Write)?;
            file.flush()
                .and_then(|_| file.sync_all())
                .map_err(|_| SupportBundleError::Write)?;
            let staged = secure_file_identity(&staging)?;
            if safe_parent_identity(parent)? != Some(current_parent) {
                return Err(SupportBundleError::OutputDrift);
            }
            if std::fs::symlink_metadata(&self.output).is_ok() {
                return Err(SupportBundleError::Collision);
            }
            std::fs::hard_link(&staging, &self.output).map_err(|error| {
                if error.kind() == std::io::ErrorKind::AlreadyExists {
                    SupportBundleError::Collision
                } else {
                    SupportBundleError::Write
                }
            })?;
            published = true;
            let published = secure_file_identity_allow_two_links(&self.output)?;
            if staged != published {
                let _ = std::fs::remove_file(&self.output);
                return Err(SupportBundleError::OutputDrift);
            }
            if std::fs::remove_file(&staging).is_err() {
                let _ = std::fs::remove_file(&self.output);
                return Err(SupportBundleError::Write);
            }
            secure_file_identity(&self.output)?;
            File::open(parent)
                .and_then(|directory| directory.sync_all())
                .map_err(|_| SupportBundleError::Write)?;
            let mut bytes = Vec::new();
            OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
                .open(&self.output)
                .and_then(|mut file| file.read_to_end(&mut bytes))
                .map_err(|_| SupportBundleError::Write)?;
            Ok(SupportBundleResult {
                schema: SCHEMA.into(),
                output: self.output.to_string_lossy().into_owned(),
                entries: self.entries.len(),
                total_uncompressed_bytes: self.preview.total_uncompressed_bytes,
                sha256: hex_sha256(&bytes),
            })
        })();
        if result.is_err() {
            if published {
                let _ = std::fs::remove_file(&self.output);
            }
            let _ = std::fs::remove_file(&staging);
        }
        result
    }
}

pub fn parse_since(value: &str, now: DateTime<Utc>) -> Result<DateTime<Utc>, SupportBundleError> {
    let (digits, suffix) = value.split_at(value.len().saturating_sub(1));
    let amount = digits
        .parse::<u64>()
        .ok()
        .filter(|amount| *amount > 0)
        .ok_or(SupportBundleError::InvalidDuration)?;
    let multiplier = match suffix {
        "s" => 1,
        "m" => 60,
        "h" => 60 * 60,
        "d" => 24 * 60 * 60,
        _ => return Err(SupportBundleError::InvalidDuration),
    };
    let seconds = amount
        .checked_mul(multiplier)
        .filter(|seconds| *seconds <= MAX_SINCE_SECONDS)
        .ok_or(SupportBundleError::InvalidDuration)?;
    Ok(now - chrono::Duration::seconds(seconds as i64))
}

fn safe_doctor(report: &DoctorReport) -> SafeDoctor {
    let mut checks = BTreeMap::from([("pass", 0usize), ("warning", 0), ("fail", 0)]);
    for check in &report.checks {
        let key = match check.level {
            CheckLevel::Pass => "pass",
            CheckLevel::Warning => "warning",
            CheckLevel::Fail => "fail",
        };
        *checks.get_mut(key).expect("fixed doctor key") += 1;
    }
    SafeDoctor {
        schema: crate::doctor::SCHEMA,
        ready: report.ready,
        checks,
        provider_count: report.providers.len(),
    }
}

fn safe_audit(report: &AuditReport) -> SafeAudit {
    SafeAudit {
        schema: "ca.audit-summary.v1",
        profile: report.profile.clone(),
        scanned_files: report.scanned_files,
        scanned_bytes: report.scanned_bytes,
        partial: report.partial,
        findings: report
            .findings
            .iter()
            .map(|finding| SafeAuditFinding {
                severity: match finding.severity {
                    AuditSeverity::Info => "info",
                    AuditSeverity::Low => "low",
                    AuditSeverity::Medium => "medium",
                    AuditSeverity::High => "high",
                    AuditSeverity::Critical => "critical",
                },
                rule_id: finding.rule_id,
                path_hash: hex_sha256(finding.path.as_bytes())[..16].into(),
            })
            .collect(),
    }
}

fn json_entry(name: &str, value: &impl Serialize) -> Result<BundleEntry, SupportBundleError> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|_| SupportBundleError::Encode)?;
    bytes.push(b'\n');
    Ok(BundleEntry {
        name: name.into(),
        bytes,
    })
}

fn manifest_entry(
    profile: &str,
    created_at: DateTime<Utc>,
    entries: &[SupportBundleEntryPreview],
    payload_bytes: usize,
) -> Result<BundleEntry, SupportBundleError> {
    let mut total = payload_bytes;
    for _ in 0..16 {
        let manifest = Manifest {
            schema: SCHEMA,
            created_at,
            profile,
            version: env!("CARGO_PKG_VERSION"),
            platform: Platform {
                os: std::env::consts::OS,
                arch: std::env::consts::ARCH,
            },
            entries,
            total_uncompressed_bytes: total,
        };
        let encoded = json_entry("manifest.json", &manifest)?;
        let next = payload_bytes
            .checked_add(encoded.bytes.len())
            .ok_or(SupportBundleError::TooLarge)?;
        if next == total {
            return Ok(encoded);
        }
        total = next;
    }
    Err(SupportBundleError::Encode)
}

fn preview_entries(entries: &[BundleEntry]) -> Vec<SupportBundleEntryPreview> {
    entries
        .iter()
        .map(|entry| SupportBundleEntryPreview {
            name: entry.name.clone(),
            bytes: entry.bytes.len(),
            sha256: hex_sha256(&entry.bytes),
        })
        .collect()
}

fn append_entry(
    archive: &mut tar::Builder<&mut flate2::write::GzEncoder<File>>,
    entry: &BundleEntry,
) -> Result<(), SupportBundleError> {
    let mut header = tar::Header::new_gnu();
    header.set_size(entry.bytes.len() as u64);
    header.set_mode(0o600);
    header.set_uid(0);
    header.set_gid(0);
    header.set_mtime(0);
    header.set_cksum();
    archive
        .append_data(&mut header, &entry.name, entry.bytes.as_slice())
        .map_err(|_| SupportBundleError::Write)
}

fn resolve_output(
    paths: &RuntimePaths,
    profile: &ActiveProfile,
    output: Option<PathBuf>,
) -> Result<(PathBuf, bool), SupportBundleError> {
    let allow_parent_creation = output.is_none();
    let output = output.unwrap_or_else(|| {
        paths
            .profile_data_dir(profile)
            .join("support-bundles")
            .join(format!(
                "ca-support-{}-{}.tar.gz",
                Utc::now().format("%Y%m%dT%H%M%SZ"),
                &Uuid::new_v4().to_string()[..8]
            ))
    });
    let output = if output.is_absolute() {
        output
    } else {
        std::env::current_dir()
            .map_err(|_| SupportBundleError::InvalidOutput)?
            .join(output)
    };
    let file_name = output
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(SupportBundleError::InvalidOutput)?;
    if file_name.is_empty()
        || file_name.len() > 255
        || file_name.chars().any(char::is_control)
        || output
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return Err(SupportBundleError::InvalidOutput);
    }
    Ok((output, allow_parent_creation))
}

fn prepare_parent(parent: &Path, allow_creation: bool) -> Result<(), SupportBundleError> {
    if !parent.exists() {
        if !allow_creation {
            return Err(SupportBundleError::InvalidOutput);
        }
        std::fs::create_dir_all(parent).map_err(|_| SupportBundleError::Write)?;
        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))
            .map_err(|_| SupportBundleError::Write)?;
    }
    safe_parent_identity(parent)?.ok_or(SupportBundleError::UnsafeOutput)?;
    Ok(())
}

fn safe_parent_identity(parent: &Path) -> Result<Option<FileIdentity>, SupportBundleError> {
    let metadata = match std::fs::symlink_metadata(parent) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(SupportBundleError::UnsafeOutput),
    };
    if !metadata.file_type().is_dir()
        || metadata.file_type().is_symlink()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o022 != 0
    {
        return Err(SupportBundleError::UnsafeOutput);
    }
    Ok(Some(FileIdentity {
        dev: metadata.dev(),
        ino: metadata.ino(),
    }))
}

fn secure_file_identity(path: &Path) -> Result<FileIdentity, SupportBundleError> {
    secure_file_identity_with_links(path, 1)
}

fn secure_file_identity_allow_two_links(path: &Path) -> Result<FileIdentity, SupportBundleError> {
    secure_file_identity_with_links(path, 2)
}

fn secure_file_identity_with_links(
    path: &Path,
    links: u64,
) -> Result<FileIdentity, SupportBundleError> {
    let metadata = std::fs::symlink_metadata(path).map_err(|_| SupportBundleError::Write)?;
    if !metadata.file_type().is_file()
        || metadata.file_type().is_symlink()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o077 != 0
        || metadata.nlink() != links
    {
        return Err(SupportBundleError::UnsafeOutput);
    }
    Ok(FileIdentity {
        dev: metadata.dev(),
        ino: metadata.ino(),
    })
}

fn hex_sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub fn provenance_for_cli(
    paths: &RuntimePaths,
    profile: &ActiveProfile,
    selector: Option<&str>,
) -> Result<ConfigProvenanceReport, SupportBundleError> {
    crate::config_provenance::inspect(
        &paths.clone().for_profile(profile).config_file,
        profile,
        [],
        ["runtime.profile".to_string()],
    )
    .and_then(|report| report.filtered(selector))
    .map_err(|_| SupportBundleError::Source)
}

#[cfg(test)]
pub fn test_plan() -> SupportBundlePlan {
    let entries = vec![BundleEntry {
        name: "manifest.json".into(),
        bytes: b"{}\n".to_vec(),
    }];
    SupportBundlePlan {
        output: PathBuf::from("/tmp/ca-support-test.tar.gz"),
        parent_identity: None,
        allow_parent_creation: false,
        preview: SupportBundlePreview {
            schema: SCHEMA.into(),
            profile: "default".into(),
            entries: preview_entries(&entries),
            total_uncompressed_bytes: entries[0].bytes.len(),
        },
        entries,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::read::GzDecoder;
    use std::os::unix::fs::PermissionsExt;

    fn fixture() -> (tempfile::TempDir, SupportBundleRequest) {
        let root = tempfile::tempdir().unwrap();
        let paths = RuntimePaths::from_home(root.path());
        std::fs::create_dir_all(&paths.config_root).unwrap();
        std::fs::set_permissions(&paths.config_root, std::fs::Permissions::from_mode(0o700))
            .unwrap();
        std::fs::write(
            &paths.config_file,
            "[defaults]\nprovider = 'safe'\nmodel = 'local'\n[providers.safe]\nbase_url = 'http://127.0.0.1:1/v1'\napi_key = '${SECRET_SENTINEL_ENV}'\nmodels = ['local']\n",
        )
        .unwrap();
        std::fs::set_permissions(&paths.config_file, std::fs::Permissions::from_mode(0o600))
            .unwrap();
        let workspace = root.path().join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        let output = root.path().join("bundle.tar.gz");
        let request = SupportBundleRequest {
            runtime_paths: paths,
            profile: ActiveProfile::Default,
            workspace_root: workspace.clone(),
            cwd: workspace,
            output: Some(output),
            since: None,
            process_overrides: Vec::new(),
        };
        (root, request)
    }

    #[test]
    fn archive_is_private_hashed_and_secret_free() {
        let (root, request) = fixture();
        let output = request.output.clone().unwrap();
        let plan = plan(request).unwrap();
        let preview = plan.preview().clone();
        let result = plan.write().unwrap();
        assert_eq!(result.entries, preview.entries.len());
        let metadata = std::fs::metadata(&output).unwrap();
        assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
        assert_eq!(metadata.nlink(), 1);
        let file = File::open(&output).unwrap();
        let mut archive = tar::Archive::new(GzDecoder::new(file));
        let mut contents = BTreeMap::new();
        for entry in archive.entries().unwrap() {
            let mut entry = entry.unwrap();
            let name = entry.path().unwrap().to_string_lossy().into_owned();
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).unwrap();
            contents.insert(name, bytes);
        }
        assert!(contents.contains_key("manifest.json"));
        let joined = contents
            .values()
            .flat_map(|bytes| bytes.iter().copied())
            .collect::<Vec<_>>();
        let text = String::from_utf8_lossy(&joined);
        assert!(!text.contains("SECRET_SENTINEL_ENV"));
        assert!(!text.contains(root.path().to_string_lossy().as_ref()));
        let manifest: serde_json::Value =
            serde_json::from_slice(&contents["manifest.json"]).unwrap();
        assert_eq!(
            manifest["total_uncompressed_bytes"].as_u64().unwrap() as usize,
            contents.values().map(Vec::len).sum::<usize>()
        );
        for entry in manifest["entries"].as_array().unwrap() {
            let name = entry["name"].as_str().unwrap();
            assert_eq!(
                entry["sha256"].as_str().unwrap(),
                hex_sha256(&contents[name])
            );
        }
    }

    #[test]
    fn collision_preserves_existing_output() {
        let (_root, request) = fixture();
        let output = request.output.clone().unwrap();
        let plan = plan(request).unwrap();
        std::fs::write(&output, b"existing").unwrap();
        assert_eq!(plan.write(), Err(SupportBundleError::Collision));
        assert_eq!(std::fs::read(output).unwrap(), b"existing");
        assert!(!std::fs::read_dir(_root.path())
            .unwrap()
            .filter_map(Result::ok)
            .any(|entry| entry
                .file_name()
                .to_string_lossy()
                .starts_with(".ca-support-")));
    }

    #[test]
    fn output_parent_drift_fails_closed() {
        let (root, mut request) = fixture();
        let parent = root.path().join("private");
        std::fs::create_dir(&parent).unwrap();
        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o700)).unwrap();
        request.output = Some(parent.join("bundle.tar.gz"));
        let plan = plan(request).unwrap();
        let moved = root.path().join("moved");
        std::fs::rename(&parent, moved).unwrap();
        std::fs::create_dir(&parent).unwrap();
        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(plan.write(), Err(SupportBundleError::OutputDrift));
    }

    #[test]
    fn duration_is_bounded_and_exact() {
        let now = Utc::now();
        assert_eq!(
            parse_since("2h", now).unwrap(),
            now - chrono::Duration::hours(2)
        );
        assert_eq!(
            parse_since("0h", now),
            Err(SupportBundleError::InvalidDuration)
        );
        assert_eq!(
            parse_since("2weeks", now),
            Err(SupportBundleError::InvalidDuration)
        );
    }

    #[test]
    fn oversized_plan_fails_before_creating_output() {
        let (_root, request) = fixture();
        let output = request.output.clone().unwrap();
        let mut plan = plan(request).unwrap();
        plan.entries.push(BundleEntry {
            name: "oversized.bin".into(),
            bytes: vec![0; MAX_UNCOMPRESSED_BYTES],
        });
        plan.preview.total_uncompressed_bytes =
            plan.entries.iter().map(|entry| entry.bytes.len()).sum();
        assert_eq!(plan.write(), Err(SupportBundleError::TooLarge));
        assert!(!output.exists());
    }

    #[test]
    fn explicit_missing_parent_and_symlink_destination_fail_closed() {
        let (root, mut request) = fixture();
        request.output = Some(root.path().join("missing/bundle.tar.gz"));
        let missing_plan = plan(request).unwrap();
        assert_eq!(missing_plan.write(), Err(SupportBundleError::InvalidOutput));
        assert!(!root.path().join("missing").exists());

        let (_root, request) = fixture();
        let output = request.output.clone().unwrap();
        let target = output.with_extension("target");
        std::fs::write(&target, b"preserve").unwrap();
        std::os::unix::fs::symlink(&target, &output).unwrap();
        assert!(matches!(plan(request), Err(SupportBundleError::Collision)));
        assert_eq!(std::fs::read(target).unwrap(), b"preserve");
    }
}
