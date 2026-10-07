use chrono::{DateTime, TimeDelta, Utc};
use clap::{Parser, Subcommand};
use cron::Schedule;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::str::FromStr;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use uuid::Uuid;

const STORE_VERSION: u32 = 1;
const DEFINITION_VERSION: u32 = 1;
const MAX_DEFINITION_BYTES: u64 = 256 * 1024;
const MAX_PROMPT_BYTES: usize = 64 * 1024;
const MAX_WORKER_SPEC_BYTES: usize = 512 * 1024;
const MAX_WORKER_OUTPUT_BYTES: usize = 1024 * 1024;
const MAX_RESULT_BYTES: usize = 256 * 1024;
const MAX_SCHEDULES: usize = 256;
const MAX_WEBHOOK_SECRET_BYTES: usize = 4096;
tokio::task_local! { static QUIET_ADMISSION: bool; }
fn admission_output() -> bool {
    !QUIET_ADMISSION.try_with(|quiet| *quiet).unwrap_or(false)
}

pub(crate) async fn run_unified_quiet(
    paths: &crate::runtime::RuntimePaths,
    profile: &crate::runtime::ActiveProfile,
) -> Result<usize, String> {
    let store = ScheduleStore::new(paths.schedules_dir(profile));
    let v2 = crate::schedule_v2::ScheduleStoreV2::new(paths.schedules_dir(profile));
    let journal = crate::schedule_v2::RunJournal::new(paths.schedule_runs_v2_dir(profile));
    let cli = ScheduleCliArgs {
        active_profile: profile.clone(),
        command: ScheduleCommand::Doctor,
        allow_network: true,
        allow_webhooks: true,
    };
    QUIET_ADMISSION
        .scope(
            true,
            tick_unified(paths, &store, &v2, &journal, &cli, false, Utc::now()),
        )
        .await
        .map(|()| 0)
        .map_err(|error| error.to_string())
}

#[derive(Debug, Parser)]
#[command(
    name = "ca schedule",
    about = "Manage explicit profile-scoped scheduled runs"
)]
pub struct RawScheduleArgs {
    #[arg(long, value_name = "NAME")]
    profile: Option<String>,

    /// Upper bound for schedule-declared provider tool network access
    #[arg(long)]
    allow_network: bool,

    /// Upper bound for signed webhook delivery
    #[arg(long)]
    allow_webhooks: bool,

    #[command(subcommand)]
    command: ScheduleCommand,
}

#[derive(Debug, Subcommand)]
enum ScheduleCommand {
    /// List safe schedule metadata
    List,
    /// Validate and add a local TOML or JSON schedule, disabled
    Add {
        path: PathBuf,
        /// Exact SHA-256 printed by the V2 preview
        #[arg(long)]
        confirm: Option<String>,
    },
    /// Replace a V2 schedule after exact digest confirmation; leaves it paused
    Update {
        id: String,
        path: PathBuf,
        #[arg(long)]
        confirm: String,
    },
    /// Enable a schedule and compute its next invocation
    Enable { id: String },
    /// Disable a schedule
    Disable { id: String },
    /// V2 spelling for disable
    Pause { id: String },
    /// V2 spelling for enable
    Resume { id: String },
    /// Permanently remove one schedule
    Remove {
        id: String,
        #[arg(long, value_name = "ID")]
        confirm: String,
    },
    /// Run one schedule immediately without enabling it
    Run { id: String },
    /// Claim at most one due invocation
    Tick {
        /// Drain every currently due schedule in deterministic order
        #[arg(long)]
        drain: bool,
    },
    /// Inspect immutable V2 run receipts without executing anything
    History { id: Option<String> },
    /// Validate V2 definitions, journals, and recovery state offline
    Doctor,
    /// Preview or explicitly manage the per-profile user LaunchAgent
    Service {
        #[command(subcommand)]
        command: ScheduleServiceCommand,
    },
    /// Poll for due invocations until interrupted; installs no system service
    Daemon {
        #[arg(long, default_value_t = 30, value_parser = clap::value_parser!(u64).range(1..=3600))]
        interval_secs: u64,
    },
    #[command(hide = true, name = "__worker")]
    Worker,
}

#[derive(Debug, Subcommand)]
enum ScheduleServiceCommand {
    Preview,
    Install {
        #[arg(long)]
        confirm: String,
        #[arg(long, default_value = "/bin/launchctl")]
        launchctl: PathBuf,
    },
    Uninstall {
        #[arg(long)]
        confirm: String,
        #[arg(long, default_value = "/bin/launchctl")]
        launchctl: PathBuf,
    },
    Status {
        #[arg(long, default_value = "/bin/launchctl")]
        launchctl: PathBuf,
    },
}

#[derive(Debug)]
pub struct ScheduleCliArgs {
    pub active_profile: crate::runtime::ActiveProfile,
    command: ScheduleCommand,
    allow_network: bool,
    allow_webhooks: bool,
}

#[derive(Debug, Clone)]
pub struct ScheduleWorkerCli {
    pub active_profile: crate::runtime::ActiveProfile,
}

pub enum ParsedScheduleArgs {
    Command(ScheduleCliArgs),
    Worker(ScheduleWorkerCli),
}

pub fn parse_args<I, T>(args: I) -> Result<ParsedScheduleArgs, clap::Error>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let raw = RawScheduleArgs::try_parse_from(args)?;
    let profile =
        crate::runtime::ActiveProfile::parse(raw.profile.as_deref()).map_err(|error| {
            <RawScheduleArgs as clap::CommandFactory>::command()
                .error(clap::error::ErrorKind::InvalidValue, error.to_string())
        })?;
    if matches!(profile, crate::runtime::ActiveProfile::Bare) {
        return Err(<RawScheduleArgs as clap::CommandFactory>::command().error(
            clap::error::ErrorKind::ArgumentConflict,
            "--profile bare is non-persistent; schedules are unavailable",
        ));
    }
    if matches!(raw.command, ScheduleCommand::Worker) {
        if raw.allow_network || raw.allow_webhooks {
            return Err(<RawScheduleArgs as clap::CommandFactory>::command().error(
                clap::error::ErrorKind::ArgumentConflict,
                "internal schedule worker does not accept authority flags",
            ));
        }
        return Ok(ParsedScheduleArgs::Worker(ScheduleWorkerCli {
            active_profile: profile,
        }));
    }
    Ok(ParsedScheduleArgs::Command(ScheduleCliArgs {
        active_profile: profile,
        command: raw.command,
        allow_network: raw.allow_network,
        allow_webhooks: raw.allow_webhooks,
    }))
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ScheduleDefinition {
    #[serde(default = "definition_version")]
    pub version: u32,
    pub id: String,
    pub provider: String,
    pub model: String,
    pub workspace: PathBuf,
    #[serde(default = "default_runtime_secs")]
    pub runtime_secs: u64,
    #[serde(default = "default_grace_secs")]
    pub grace_secs: u64,
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default)]
    pub grants: Vec<crate::permission::PermissionMatcher>,
    #[serde(default)]
    pub allow_network: bool,
    pub trigger: ScheduleTrigger,
    pub target: ScheduleTarget,
    pub delivery: ScheduleDelivery,
}

impl std::fmt::Debug for ScheduleDefinition {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ScheduleDefinition")
            .field("version", &self.version)
            .field("id", &self.id)
            .field("provider", &self.provider)
            .field("model", &self.model)
            .field("workspace", &self.workspace)
            .field("runtime_secs", &self.runtime_secs)
            .field("grace_secs", &self.grace_secs)
            .field("tool_count", &self.tools.len())
            .field("grant_count", &self.grants.len())
            .field("allow_network", &self.allow_network)
            .field("trigger", &self.trigger)
            .field("target", &"[SCHEDULE TARGET REDACTED]")
            .field("delivery", &self.delivery.safe_label())
            .finish()
    }
}

fn definition_version() -> u32 {
    DEFINITION_VERSION
}

fn default_runtime_secs() -> u64 {
    300
}

fn default_grace_secs() -> u64 {
    60
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ScheduleTrigger {
    Once { at: DateTime<Utc> },
    Cron { expression: String },
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ScheduleTarget {
    Prompt {
        prompt: String,
    },
    Workflow {
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        workflow_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        digest: Option<String>,
    },
}

impl std::fmt::Debug for ScheduleTarget {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Prompt { .. } => formatter.write_str("Prompt([REDACTED])"),
            Self::Workflow {
                path,
                workflow_id,
                digest,
            } => formatter
                .debug_struct("Workflow")
                .field("path", path)
                .field("workflow_id", workflow_id)
                .field("digest", digest)
                .finish(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ScheduleDelivery {
    LocalFile { path: String },
    Webhook { url: String, secret_env: String },
}

impl ScheduleDelivery {
    fn safe_label(&self) -> String {
        match self {
            Self::LocalFile { path } => format!("file:{path}"),
            Self::Webhook { url, secret_env } => {
                let host = reqwest::Url::parse(url)
                    .ok()
                    .and_then(|url| url.host_str().map(str::to_string))
                    .unwrap_or_else(|| "invalid".into());
                format!("webhook:{host} ({secret_env})")
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ScheduleOutcome {
    Succeeded,
    Failed,
    TimedOut,
    SkippedLate,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ScheduleRecord {
    definition: ScheduleDefinition,
    enabled: bool,
    next_run_at: Option<DateTime<Utc>>,
    last_run_at: Option<DateTime<Utc>>,
    last_outcome: Option<ScheduleOutcome>,
    last_exit_code: Option<i32>,
}

impl std::fmt::Debug for ScheduleRecord {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ScheduleRecord")
            .field("id", &self.definition.id)
            .field("enabled", &self.enabled)
            .field("next_run_at", &self.next_run_at)
            .field("last_run_at", &self.last_run_at)
            .field("last_outcome", &self.last_outcome)
            .field("last_exit_code", &self.last_exit_code)
            .finish()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ScheduleStoreData {
    version: u32,
    revision: u64,
    records: Vec<ScheduleRecord>,
}

impl Default for ScheduleStoreData {
    fn default() -> Self {
        Self {
            version: STORE_VERSION,
            revision: 0,
            records: Vec::new(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum ScheduleError {
    #[error("schedule definition must be a bounded regular non-symlink .toml or .json file")]
    InvalidDefinitionFile,
    #[error("schedule definition could not be parsed: {0}")]
    Parse(String),
    #[error("unsupported schedule definition version: {0}")]
    UnsupportedVersion(u32),
    #[error("schedule IDs must be 1-64 lowercase ASCII letters, digits, '-' or '_', starting with a letter")]
    InvalidId,
    #[error("schedule provider/model must be exact nonempty identifiers")]
    InvalidRoute,
    #[error("schedule provider/model is not configured: {0}/{1}")]
    UnknownRoute(String, String),
    #[error("schedule workspace must be an existing canonical directory")]
    InvalidWorkspace,
    #[error("schedule runtime must be 1-86400 seconds and grace must be 1-3600 seconds")]
    InvalidLimits,
    #[error("schedule target is invalid")]
    InvalidTarget,
    #[error("schedule trigger is invalid: {0}")]
    InvalidTrigger(String),
    #[error("schedule references an unavailable or unsafe tool: {0}")]
    InvalidTool(String),
    #[error("schedule grant is invalid: {0}")]
    InvalidGrant(String),
    #[error("schedule delivery is invalid: {0}")]
    InvalidDelivery(String),
    #[error("schedule already exists: {0}")]
    Duplicate(String),
    #[error("schedule was not found: {0}")]
    Missing(String),
    #[error("schedule removal requires --confirm with the exact schedule ID")]
    Confirmation,
    #[error("another schedule command is active for this profile")]
    Busy,
    #[error("schedule state is invalid or unsafe: {0}")]
    State(String),
    #[error("schedule worker failed: {0}")]
    Worker(String),
    #[error("schedule delivery failed: {0}")]
    Delivery(String),
    #[error("schedule I/O failed: {0}")]
    Io(String),
}

#[derive(Debug)]
struct ScheduleStore {
    dir: PathBuf,
    path: PathBuf,
}

impl ScheduleStore {
    fn new(dir: PathBuf) -> Self {
        let dir = normalize_missing_path(&dir).unwrap_or(dir);
        Self {
            path: dir.join("schedules.json"),
            dir,
        }
    }

    fn load(&self) -> Result<ScheduleStoreData, ScheduleError> {
        if !self.path.exists() {
            return Ok(ScheduleStoreData::default());
        }
        reject_symlink_components(&self.path)?;
        let metadata = std::fs::symlink_metadata(&self.path)
            .map_err(|error| ScheduleError::Io(error.to_string()))?;
        if !metadata.is_file() || metadata.len() > MAX_DEFINITION_BYTES * 4 {
            return Err(ScheduleError::State(
                "schedule store is not a bounded regular file".into(),
            ));
        }
        check_owner_only(&metadata)?;
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        File::open(&self.path)
            .and_then(|file| {
                file.take(MAX_DEFINITION_BYTES * 4 + 1)
                    .read_to_end(&mut bytes)
            })
            .map_err(|error| ScheduleError::Io(error.to_string()))?;
        if bytes.len() as u64 > MAX_DEFINITION_BYTES * 4 {
            return Err(ScheduleError::State("schedule store is too large".into()));
        }
        let data: ScheduleStoreData = serde_json::from_slice(&bytes)
            .map_err(|error| ScheduleError::State(error.to_string()))?;
        if data.version != STORE_VERSION || data.records.len() > MAX_SCHEDULES {
            return Err(ScheduleError::State(
                "unsupported or oversized schedule store".into(),
            ));
        }
        Ok(data)
    }

    fn save(&self, data: &mut ScheduleStoreData) -> Result<(), ScheduleError> {
        data.revision = data
            .revision
            .checked_add(1)
            .ok_or_else(|| ScheduleError::State("schedule revision overflow".into()))?;
        ensure_private_dir(&self.dir)?;
        let bytes = serde_json::to_vec_pretty(data)
            .map_err(|error| ScheduleError::State(error.to_string()))?;
        if bytes.len() as u64 > MAX_DEFINITION_BYTES * 4 {
            return Err(ScheduleError::State("schedule store is too large".into()));
        }
        let temp = self.dir.join(format!(".schedules.{}.tmp", Uuid::new_v4()));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let result = (|| {
            let mut file = options
                .open(&temp)
                .map_err(|error| ScheduleError::Io(error.to_string()))?;
            file.write_all(&bytes)
                .and_then(|_| file.sync_all())
                .map_err(|error| ScheduleError::Io(error.to_string()))?;
            std::fs::rename(&temp, &self.path)
                .map_err(|error| ScheduleError::Io(error.to_string()))?;
            Ok(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temp);
        }
        result
    }

    fn lock(&self) -> Result<ScheduleLock, ScheduleError> {
        ensure_private_dir(&self.dir)?;
        ScheduleLock::acquire(self.dir.join("schedule.lock"))
    }
}

/// Count classic schedule records without exposing or changing either persisted format.
///
/// AUT-231 uses this only to enforce the profile-wide 256-definition ceiling shared by
/// classic schedules and additive `ca.automation.v1` records.
pub(crate) fn configured_schedule_count(
    paths: &crate::runtime::RuntimePaths,
    profile: &crate::runtime::ActiveProfile,
) -> Result<usize, String> {
    let classic = ScheduleStore::new(paths.schedules_dir(profile))
        .load()
        .map_err(|error| error.to_string())?
        .records
        .len();
    let v2 = crate::schedule_v2::ScheduleStoreV2::new(paths.schedules_dir(profile))
        .records()
        .map_err(|error| error.to_string())?
        .len();
    classic
        .checked_add(v2)
        .ok_or_else(|| "schedule count overflow".to_string())
}

fn normalize_missing_path(path: &Path) -> Option<PathBuf> {
    let existing = path.ancestors().find(|ancestor| ancestor.exists())?;
    let canonical = existing.canonicalize().ok()?;
    let suffix = path.strip_prefix(existing).ok()?;
    Some(canonical.join(suffix))
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LockOwner {
    pid: u32,
    token: Uuid,
}

struct ScheduleLock {
    path: PathBuf,
    token: Uuid,
}

impl ScheduleLock {
    fn acquire(path: PathBuf) -> Result<Self, ScheduleError> {
        for attempt in 0..2 {
            let token = Uuid::new_v4();
            let owner = LockOwner {
                pid: std::process::id(),
                token,
            };
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            match options.open(&path) {
                Ok(mut file) => {
                    let bytes = serde_json::to_vec(&owner)
                        .map_err(|error| ScheduleError::State(error.to_string()))?;
                    file.write_all(&bytes)
                        .and_then(|_| file.sync_all())
                        .map_err(|error| ScheduleError::Io(error.to_string()))?;
                    return Ok(Self { path, token });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists && attempt == 0 => {
                    if stale_lock(&path)? {
                        std::fs::remove_file(&path)
                            .map_err(|error| ScheduleError::Io(error.to_string()))?;
                        continue;
                    }
                    return Err(ScheduleError::Busy);
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    return Err(ScheduleError::Busy)
                }
                Err(error) => return Err(ScheduleError::Io(error.to_string())),
            }
        }
        Err(ScheduleError::Busy)
    }
}

impl Drop for ScheduleLock {
    fn drop(&mut self) {
        let owner = std::fs::read(&self.path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<LockOwner>(&bytes).ok());
        if owner.is_some_and(|owner| owner.pid == std::process::id() && owner.token == self.token) {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

fn stale_lock(path: &Path) -> Result<bool, ScheduleError> {
    reject_symlink_components(path)?;
    let bytes = std::fs::read(path).map_err(|error| ScheduleError::Io(error.to_string()))?;
    if bytes.len() > 4096 {
        return Err(ScheduleError::Busy);
    }
    let owner: LockOwner = serde_json::from_slice(&bytes).map_err(|_| ScheduleError::Busy)?;
    #[cfg(unix)]
    {
        let pid = i32::try_from(owner.pid).map_err(|_| ScheduleError::Busy)?;
        match nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None) {
            Ok(()) | Err(nix::errno::Errno::EPERM) => Ok(false),
            Err(nix::errno::Errno::ESRCH) => Ok(true),
            Err(_) => Err(ScheduleError::Busy),
        }
    }
    #[cfg(not(unix))]
    {
        let _ = owner;
        Err(ScheduleError::Busy)
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub automation_budget: Option<crate::automation::AutomationBudgetV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_context: Option<ca_automation::runtime::ExecutionContext>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) native_sandbox_binding: Option<crate::automation_runtime::NativeSandboxBinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub automation_pricing: Option<crate::automation::PricingBindingV1>,
    pub provider: String,
    pub model: String,
    pub workspace: PathBuf,
    pub prompt: String,
    pub tools: Vec<String>,
    pub grants: Vec<crate::permission::PermissionMatcher>,
    pub allow_network: bool,
    pub runtime_secs: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<crate::config::ReasoningEffort>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub history: Vec<crate::app::Message>,
}

impl std::fmt::Debug for WorkerSpec {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WorkerSpec")
            .field(
                "native_run",
                &self.native_context.as_ref().map(|context| &context.run_id),
            )
            .field("provider", &self.provider)
            .field("model", &self.model)
            .field("workspace", &self.workspace)
            .field("prompt", &"[SCHEDULE PROMPT REDACTED]")
            .field("tool_count", &self.tools.len())
            .field("grant_count", &self.grants.len())
            .field("allow_network", &self.allow_network)
            .field("runtime_secs", &self.runtime_secs)
            .field("reasoning_effort", &self.reasoning_effort)
            .field("history_messages", &self.history.len())
            .finish()
    }
}

pub fn read_worker_spec() -> Result<WorkerSpec, String> {
    let mut bytes = Vec::new();
    std::io::stdin()
        .take(MAX_WORKER_SPEC_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "could not read schedule worker input".to_string())?;
    if bytes.len() > MAX_WORKER_SPEC_BYTES {
        return Err("schedule worker input is too large".into());
    }
    let spec: WorkerSpec = serde_json::from_slice(&bytes)
        .map_err(|_| "schedule worker input is invalid".to_string())?;
    validate_worker_spec(&spec).map_err(|error| error.to_string())?;
    Ok(spec)
}

fn validate_worker_spec(spec: &WorkerSpec) -> Result<(), ScheduleError> {
    validate_route_text(&spec.provider, &spec.model)?;
    if spec.prompt.trim().is_empty() || spec.prompt.len() > MAX_PROMPT_BYTES {
        return Err(ScheduleError::InvalidTarget);
    }
    let canonical = spec
        .workspace
        .canonicalize()
        .map_err(|_| ScheduleError::InvalidWorkspace)?;
    if canonical != spec.workspace
        || !canonical.is_dir()
        || !(1..=86_400).contains(&spec.runtime_secs)
    {
        return Err(ScheduleError::InvalidWorkspace);
    }
    validate_tools_and_grants(&spec.tools, &spec.grants, spec.allow_network)
}

pub async fn run_cli(cli: ScheduleCliArgs) -> i32 {
    match run_cli_inner(cli).await {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("ca schedule: {error}");
            match error {
                ScheduleError::Parse(_)
                | ScheduleError::InvalidDefinitionFile
                | ScheduleError::UnsupportedVersion(_)
                | ScheduleError::InvalidId
                | ScheduleError::InvalidRoute
                | ScheduleError::UnknownRoute(_, _)
                | ScheduleError::InvalidWorkspace
                | ScheduleError::InvalidLimits
                | ScheduleError::InvalidTarget
                | ScheduleError::InvalidTrigger(_)
                | ScheduleError::InvalidTool(_)
                | ScheduleError::InvalidGrant(_)
                | ScheduleError::InvalidDelivery(_)
                | ScheduleError::Duplicate(_)
                | ScheduleError::Missing(_)
                | ScheduleError::Confirmation => 2,
                ScheduleError::Busy => 4,
                ScheduleError::Worker(_) | ScheduleError::Delivery(_) => 6,
                ScheduleError::State(_) | ScheduleError::Io(_) => 8,
            }
        }
    }
}

async fn run_cli_inner(cli: ScheduleCliArgs) -> Result<(), ScheduleError> {
    let paths = crate::runtime::RuntimePaths::discover()
        .map_err(|error| ScheduleError::Io(error.to_string()))?;
    let store = ScheduleStore::new(paths.schedules_dir(&cli.active_profile));
    let v2_store =
        crate::schedule_v2::ScheduleStoreV2::new(paths.schedules_dir(&cli.active_profile));
    let v2_journal =
        crate::schedule_v2::RunJournal::new(paths.schedule_runs_v2_dir(&cli.active_profile));
    match cli.command {
        ScheduleCommand::List => {
            let data = store.load()?;
            let v2_records = v2_store.records().map_err(v2_error)?;
            if data.records.is_empty() && v2_records.is_empty() {
                println!("No schedules configured");
                return Ok(());
            }
            print_list(&data);
            for record in v2_records {
                let state = if record.enabled { "enabled" } else { "paused" };
                let next = record
                    .next_occurrence
                    .as_ref()
                    .map(|occurrence| occurrence.at.to_rfc3339())
                    .unwrap_or_else(|| "-".into());
                println!(
                    "{}\t{}\tv2\tnext={}\tdigest={}",
                    record.definition.id, state, next, record.definition_digest
                );
            }
            Ok(())
        }
        ScheduleCommand::Add { path, confirm }
            if crate::schedule_v2::is_v2_definition_file(&path) =>
        {
            let definition = crate::schedule_v2::load_definition_file(&path).map_err(v2_error)?;
            let now = Utc::now();
            let normalized =
                crate::schedule_v2::normalize_at_confirmation(definition, now).map_err(v2_error)?;
            let digest = crate::schedule_v2::definition_digest(&normalized).map_err(v2_error)?;
            let Some(confirm) = confirm else {
                let preview = serde_json::to_string_pretty(&normalized)
                    .map_err(|error| ScheduleError::State(error.to_string()))?;
                println!("{preview}");
                println!("Confirmation digest: {digest}");
                return Ok(());
            };
            let record = v2_store
                .add_confirmed(normalized, &confirm, now)
                .map_err(v2_error)?;
            println!("Added V2 schedule {} (paused)", record.definition.id);
            Ok(())
        }
        ScheduleCommand::Add { path, .. } => {
            let _lock = store.lock()?;
            let config_paths = paths.clone().for_profile(&cli.active_profile);
            let config = crate::config::ConfigStore::for_profile(
                config_paths.config_file,
                cli.active_profile.clone(),
            )
            .load()
            .map_err(|error| ScheduleError::State(error.to_string()))?;
            let definition = load_definition(&path, &config)?;
            let id = definition.id.clone();
            let mut data = store.load()?;
            if data.records.iter().any(|record| record.definition.id == id) {
                return Err(ScheduleError::Duplicate(id));
            }
            if data.records.len() >= MAX_SCHEDULES {
                return Err(ScheduleError::State("schedule limit reached".into()));
            }
            data.records.push(ScheduleRecord {
                definition,
                enabled: false,
                next_run_at: None,
                last_run_at: None,
                last_outcome: None,
                last_exit_code: None,
            });
            data.records
                .sort_by(|left, right| left.definition.id.cmp(&right.definition.id));
            store.save(&mut data)?;
            println!("Added schedule {id} (disabled)");
            Ok(())
        }
        ScheduleCommand::Update { id, path, confirm } => {
            let definition = crate::schedule_v2::load_definition_file(&path).map_err(v2_error)?;
            let record = v2_store
                .update_confirmed(&id, definition, &confirm, Utc::now())
                .map_err(v2_error)?;
            println!("Updated V2 schedule {} (paused)", record.definition.id);
            Ok(())
        }
        ScheduleCommand::Enable { id } => {
            let v1_exists = store
                .load()?
                .records
                .iter()
                .any(|record| record.definition.id == id);
            if v1_exists {
                mutate_record(&store, &id, |record| {
                    record.enabled = true;
                    record.next_run_at =
                        Some(next_invocation(&record.definition.trigger, Utc::now())?);
                    Ok(())
                })?;
                println!("Enabled schedule {id}");
            } else {
                v2_store
                    .set_enabled(&id, true, Utc::now())
                    .map_err(v2_error)?;
                println!("Resumed V2 schedule {id}");
            }
            Ok(())
        }
        ScheduleCommand::Disable { id } => {
            let v1_exists = store
                .load()?
                .records
                .iter()
                .any(|record| record.definition.id == id);
            if v1_exists {
                mutate_record(&store, &id, |record| {
                    record.enabled = false;
                    record.next_run_at = None;
                    Ok(())
                })?;
                println!("Disabled schedule {id}");
            } else {
                v2_store
                    .set_enabled(&id, false, Utc::now())
                    .map_err(v2_error)?;
                println!("Paused V2 schedule {id}");
            }
            Ok(())
        }
        ScheduleCommand::Pause { id } => {
            v2_store
                .set_enabled(&id, false, Utc::now())
                .map_err(v2_error)?;
            println!("Paused V2 schedule {id}");
            Ok(())
        }
        ScheduleCommand::Resume { id } => {
            v2_store
                .set_enabled(&id, true, Utc::now())
                .map_err(v2_error)?;
            println!("Resumed V2 schedule {id}");
            Ok(())
        }
        ScheduleCommand::Remove { id, confirm } => {
            if id != confirm {
                return Err(ScheduleError::Confirmation);
            }
            let _lock = store.lock()?;
            let mut data = store.load()?;
            let Some(index) = data
                .records
                .iter()
                .position(|record| record.definition.id == id)
            else {
                return Err(ScheduleError::Missing(id));
            };
            data.records.remove(index);
            store.save(&mut data)?;
            println!("Removed schedule {id}");
            Ok(())
        }
        ScheduleCommand::Run { ref id } => {
            let has_v1 = store
                .load()?
                .records
                .iter()
                .any(|record| &record.definition.id == id);
            if has_v1 {
                let (record, revision) = {
                    let _lock = store.lock()?;
                    let mut data = store.load()?;
                    let index = record_index(&data, id)?;
                    let record = data.records[index].clone();
                    // A checked revision is the durable immediate-run claim;
                    // V1 bytes and fields remain unchanged.
                    store.save(&mut data)?;
                    (record, data.revision)
                };
                execute_claimed(&store, record, revision, &cli, Utc::now()).await
            } else {
                let claim = v2_store
                    .claim_immediate(cli.active_profile.label(), &v2_journal, id, Utc::now())
                    .map_err(v2_error)?;
                execute_v2_claim(&paths, &v2_store, &v2_journal, &cli, claim).await
            }
        }
        ScheduleCommand::Tick { drain } => {
            tick_unified(
                &paths,
                &store,
                &v2_store,
                &v2_journal,
                &cli,
                drain,
                Utc::now(),
            )
            .await?;
            Ok(())
        }
        ScheduleCommand::History { id } => {
            for event in v2_journal.history(id.as_deref()).map_err(v2_error)? {
                println!(
                    "{}\t{}\t{}\t{:?}\t{}\t{}",
                    event.created_at.to_rfc3339(),
                    event.schedule_id,
                    event.run_id,
                    event.state,
                    event.phase,
                    event.event_hash
                );
            }
            Ok(())
        }
        ScheduleCommand::Doctor => {
            let records = v2_store.records().map_err(v2_error)?;
            let report = v2_journal.doctor().map_err(v2_error)?;
            println!(
                "V2 schedules={} runs={} active={} interrupted={} healthy={}",
                records.len(),
                report.runs,
                report.active,
                report.interrupted,
                report.healthy
            );
            if report.healthy {
                Ok(())
            } else {
                Err(ScheduleError::State(
                    "V2 journal has unresolved active runs".into(),
                ))
            }
        }
        ScheduleCommand::Service { command } => {
            let executable =
                std::env::current_exe().map_err(|error| ScheduleError::Io(error.to_string()))?;
            let preview =
                crate::schedule_v2::launch_agent_preview(&paths, &cli.active_profile, &executable)
                    .map_err(v2_error)?;
            match command {
                ScheduleServiceCommand::Preview => {
                    print!(
                        "{}",
                        crate::schedule_v2::service_mutation(
                            crate::schedule_v2::ServiceAction::Preview,
                            &preview,
                            None,
                            Path::new("/bin/launchctl"),
                        )
                        .map_err(v2_error)?
                    );
                    eprintln!("Confirmation digest: {}", preview.digest);
                }
                ScheduleServiceCommand::Install { confirm, launchctl } => println!(
                    "{}",
                    crate::schedule_v2::service_mutation(
                        crate::schedule_v2::ServiceAction::Install,
                        &preview,
                        Some(&confirm),
                        &launchctl,
                    )
                    .map_err(v2_error)?
                ),
                ScheduleServiceCommand::Uninstall { confirm, launchctl } => println!(
                    "{}",
                    crate::schedule_v2::service_mutation(
                        crate::schedule_v2::ServiceAction::Uninstall,
                        &preview,
                        Some(&confirm),
                        &launchctl,
                    )
                    .map_err(v2_error)?
                ),
                ScheduleServiceCommand::Status { launchctl } => println!(
                    "{}",
                    crate::schedule_v2::service_mutation(
                        crate::schedule_v2::ServiceAction::Status,
                        &preview,
                        None,
                        &launchctl,
                    )
                    .map_err(v2_error)?
                ),
            }
            Ok(())
        }
        ScheduleCommand::Daemon { interval_secs } => loop {
            if let Err(error) = tick_unified(
                &paths,
                &store,
                &v2_store,
                &v2_journal,
                &cli,
                true,
                Utc::now(),
            )
            .await
            {
                eprintln!("ca automation: {error}");
            }
            #[cfg(unix)]
            let terminate = async {
                let mut signal =
                    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                        .expect("SIGTERM handler");
                signal.recv().await;
            };
            #[cfg(not(unix))]
            let terminate = std::future::pending::<()>();
            tokio::select! {
                _ = tokio::signal::ctrl_c() => return Ok(()),
                _ = terminate => return Ok(()),
                _ = tokio::time::sleep(Duration::from_secs(interval_secs)) => {}
            }
        },
        ScheduleCommand::Worker => unreachable!("worker routed before command execution"),
    }
}

fn v2_error(error: crate::schedule_v2::ScheduleV2Error) -> ScheduleError {
    match error {
        crate::schedule_v2::ScheduleV2Error::Busy => ScheduleError::Busy,
        crate::schedule_v2::ScheduleV2Error::Missing(id) => ScheduleError::Missing(id),
        crate::schedule_v2::ScheduleV2Error::Duplicate(id) => ScheduleError::Duplicate(id),
        crate::schedule_v2::ScheduleV2Error::ConfirmationRequired(_) => ScheduleError::Confirmation,
        crate::schedule_v2::ScheduleV2Error::UnsupportedVersion(version) => {
            ScheduleError::UnsupportedVersion(version)
        }
        crate::schedule_v2::ScheduleV2Error::InvalidDefinition(message) => {
            ScheduleError::Parse(message)
        }
        crate::schedule_v2::ScheduleV2Error::Execution(message) => ScheduleError::Worker(message),
        crate::schedule_v2::ScheduleV2Error::State(message) => ScheduleError::State(message),
        crate::schedule_v2::ScheduleV2Error::Io(message) => ScheduleError::Io(message),
    }
}

fn automation_error(error: crate::automation::AutomationError) -> ScheduleError {
    match error {
        crate::automation::AutomationError::Missing(id) => ScheduleError::Missing(id),
        crate::automation::AutomationError::Busy => ScheduleError::Busy,
        crate::automation::AutomationError::Invalid(message)
        | crate::automation::AutomationError::Confirmation(message) => {
            ScheduleError::Parse(message)
        }
        crate::automation::AutomationError::Duplicate(id) => ScheduleError::Duplicate(id),
        crate::automation::AutomationError::State(message) => ScheduleError::State(message),
        crate::automation::AutomationError::Io(message) => ScheduleError::Io(message),
    }
}

async fn tick_unified(
    paths: &crate::runtime::RuntimePaths,
    store: &ScheduleStore,
    v2: &crate::schedule_v2::ScheduleStoreV2,
    journal: &crate::schedule_v2::RunJournal,
    cli: &ScheduleCliArgs,
    drain: bool,
    now: DateTime<Utc>,
) -> Result<(), ScheduleError> {
    let _admission = ca_automation::runtime::ResourceLease::acquire(
        &crate::automation_runtime::lease_root(paths),
        &format!("admission:{}", cli.active_profile.label()),
    )
    .map_err(|_| ScheduleError::Busy)?;
    let config = crate::config::ConfigStore::for_profile(
        paths.clone().for_profile(&cli.active_profile).config_file,
        cli.active_profile.clone(),
    )
    .load()
    .map_err(|e| ScheduleError::State(e.to_string()))?;
    let registration_failure =
        crate::harness_automation::register_enabled(paths, &cli.active_profile, &config)
            .err()
            .map(|error| ScheduleError::State(error.to_string()));
    crate::automation::recover_pending_deliveries(paths, &cli.active_profile)
        .map_err(automation_error)?;
    recover_native_v2(paths, v2, journal, cli).await?;
    v2.recover_dead_owners(journal, now).map_err(v2_error)?;
    let mut failure = registration_failure;
    for _ in 0..if drain { 256 } else { 1 } {
        let mut due = Vec::new();
        for r in store.load()?.records {
            if r.enabled {
                if let Some(at) = r.next_run_at.filter(|at| *at <= now) {
                    due.push((at, r.definition.id, 0u8));
                }
            }
        }
        for r in v2.records().map_err(v2_error)? {
            if r.enabled && r.active_run_id.is_none() {
                if let Some(o) = r.next_occurrence.filter(|o| o.at <= now) {
                    due.push((o.at, r.definition.id, 1u8));
                }
            }
        }
        for r in crate::automation::AutomationStore::for_paths(paths, &cli.active_profile)
            .records()
            .map_err(automation_error)?
        {
            if r.enabled && r.active_run_id.is_none() && r.blocked_reason.is_none() {
                if let Some(o) = r
                    .pending_heartbeat
                    .or(r.next_occurrence)
                    .filter(|o| o.at <= now)
                {
                    due.push((o.at, r.definition.id, 2u8));
                }
            }
        }
        due.sort();
        let Some((_, _, kind)) = due.first() else {
            break;
        };
        let result = match kind {
            0 => tick_once(store, cli, now).await,
            1 => tick_v2(paths, v2, journal, cli, false, now).await,
            _ => if admission_output() {
                crate::automation::run_due(paths, &cli.active_profile, false, now).await
            } else {
                crate::automation::run_due_quiet(paths, &cli.active_profile, false, now).await
            }
            .map(|_| ())
            .map_err(automation_error),
        };
        if let Err(error) = result {
            if matches!(error, ScheduleError::Busy) {
                break;
            }
            failure = Some(error);
        }
    }
    failure.map_or(Ok(()), Err)
}

async fn tick_v2(
    paths: &crate::runtime::RuntimePaths,
    store: &crate::schedule_v2::ScheduleStoreV2,
    journal: &crate::schedule_v2::RunJournal,
    cli: &ScheduleCliArgs,
    drain: bool,
    now: DateTime<Utc>,
) -> Result<(), ScheduleError> {
    recover_native_v2(paths, store, journal, cli).await?;
    let recovered = store.recover_dead_owners(journal, now).map_err(v2_error)?;
    let lease_manager = crate::schedule_v2::LeaseManager::new(
        paths.schedule_private_root(&cli.active_profile),
        2 * 1024 * 1024 * 1024,
    );
    let recovered_leases = lease_manager
        .recover_terminal_dead(journal)
        .map_err(v2_error)?;
    if admission_output() && (recovered > 0 || recovered_leases > 0) {
        eprintln!(
            "Recovered {recovered} dead-owner run(s) and {recovered_leases} terminal lease(s) without replay"
        );
    }
    let mut claimed = 0_usize;
    loop {
        let Some(claim) = store
            .claim_due(cli.active_profile.label(), journal, now)
            .map_err(v2_error)?
        else {
            if claimed == 0 && admission_output() {
                println!("No V2 schedule is due");
            }
            return Ok(());
        };
        execute_v2_claim(paths, store, journal, cli, claim).await?;
        claimed = claimed.saturating_add(1);
        if !drain {
            return Ok(());
        }
    }
}

async fn recover_native_v2(
    paths: &crate::runtime::RuntimePaths,
    store: &crate::schedule_v2::ScheduleStoreV2,
    journal: &crate::schedule_v2::RunJournal,
    cli: &ScheduleCliArgs,
) -> Result<(), ScheduleError> {
    use crate::schedule_v2::{ReceiptUpdate, ScheduleRunState};
    for record in store.records().map_err(v2_error)? {
        let Some(run_id) = record.active_run_id else {
            continue;
        };
        let Some(latest) = journal.latest(run_id).map_err(v2_error)? else {
            continue;
        };
        if latest.state.terminal()
            || crate::schedule_v2::owner_is_alive(latest.owner_pid).map_err(v2_error)?
            || store.has_live_native_executor(&latest).map_err(v2_error)?
        {
            continue;
        }
        let native_root = crate::automation_runtime::state_root(paths, &cli.active_profile);
        if !native_root
            .join("runs")
            .join(run_id.to_string())
            .join("context.json")
            .is_file()
        {
            continue;
        }
        let receipt = crate::automation_runtime::inspect_run(
            paths,
            &cli.active_profile,
            &run_id.to_string(),
            true,
            false,
        )
        .map_err(ScheduleError::State)?;
        if receipt.state == ca_automation::runtime::ExecutionState::Running {
            continue;
        }
        let claim = crate::schedule_v2::RunClaim {
            run_id,
            profile: latest.profile.clone(),
            schedule_id: record.definition.id.clone(),
            definition_digest: record.definition_digest.clone(),
            occurrence: crate::schedule_v2::Occurrence {
                id: latest.occurrence_id.clone(),
                at: latest.occurrence_at,
            },
            claim_revision: latest.claim_revision,
            definition: record.definition,
        };
        if receipt.state == ca_automation::runtime::ExecutionState::Succeeded {
            // A webhook/email intent can have reached its remote destination.
            // Local publication is idempotent; ambiguous remote intent is held.
            if latest.state == ScheduleRunState::Delivering
                && matches!(
                    claim.definition.delivery,
                    crate::schedule_v2::ScheduleDeliveryV2::Webhook { .. }
                        | crate::schedule_v2::ScheduleDeliveryV2::EmailDraft { .. }
                )
            {
                continue;
            }
            let result = crate::automation_runtime::result_from_receipt(receipt)
                .map_err(ScheduleError::State)?;
            if latest.state != ScheduleRunState::Delivering {
                journal
                    .transition(
                        run_id,
                        ScheduleRunState::Delivering,
                        ReceiptUpdate {
                            phase: "recorded_native_delivery".into(),
                            delivery_status: Some("intent".into()),
                            ..Default::default()
                        },
                        Utc::now(),
                    )
                    .map_err(v2_error)?;
            }
            deliver_v2(paths, cli, &claim, &result.content).await?;
            journal
                .transition(
                    run_id,
                    ScheduleRunState::Succeeded,
                    ReceiptUpdate {
                        phase: "recorded_native_completion_recovered".into(),
                        delivery_status: Some("acknowledged".into()),
                        outcome: Some("succeeded without repeated execution".into()),
                        ..Default::default()
                    },
                    Utc::now(),
                )
                .map_err(v2_error)?;
            store.settle(&claim).map_err(v2_error)?;
        } else {
            journal
                .transition(
                    run_id,
                    ScheduleRunState::Interrupted,
                    ReceiptUpdate {
                        phase: "native_needs_attention".into(),
                        outcome: Some(
                            "recorded effects require reconciliation; schedule paused".into(),
                        ),
                        ..Default::default()
                    },
                    Utc::now(),
                )
                .map_err(v2_error)?;
            store.settle(&claim).map_err(v2_error)?;
            store
                .set_enabled(&claim.schedule_id, false, Utc::now())
                .map_err(v2_error)?;
        }
    }
    Ok(())
}

async fn execute_v2_claim(
    paths: &crate::runtime::RuntimePaths,
    store: &crate::schedule_v2::ScheduleStoreV2,
    journal: &crate::schedule_v2::RunJournal,
    cli: &ScheduleCliArgs,
    claim: crate::schedule_v2::RunClaim,
) -> Result<(), ScheduleError> {
    if claim.definition.target.native() {
        return execute_v2_claim_inner(paths, store, journal, cli, claim).await;
    }
    let context = crate::schedule_compat_runtime::execution_context(
        paths,
        &cli.active_profile,
        claim.run_id,
        &claim.definition,
        &(cli.allow_network, cli.allow_webhooks),
        &claim.definition.workspace,
        claim.definition.runtime_secs,
        claim.definition.output_bytes,
    )
    .map_err(ScheduleError::State)?;
    let run =
        crate::schedule_compat_runtime::LegacyRun::open(context).map_err(ScheduleError::State)?;
    if run.cached_result().map_err(ScheduleError::State)?.is_some() {
        drop(run);
        return recover_native_v2(paths, store, journal, cli).await;
    }
    run.begin_action("target").map_err(ScheduleError::State)?;
    let schedule_id = claim.schedule_id.clone();
    let result = run
        .scope(Box::pin(execute_v2_claim_inner(
            paths, store, journal, cli, claim,
        )))
        .await;
    if result.is_err()
        && run
            .store
            .status()
            .map_err(|error| ScheduleError::State(error.to_string()))?
            .state
            == ca_automation::runtime::ExecutionState::Running
    {
        run.finish_failure(
            ca_automation::runtime::ExecutionState::NeedsAttention,
            "legacy V2 execution stopped with unconfirmed effects",
        )
        .map_err(ScheduleError::State)?;
        store
            .set_enabled(&schedule_id, false, Utc::now())
            .map_err(v2_error)?;
    }
    result
}

async fn execute_v2_claim_inner(
    paths: &crate::runtime::RuntimePaths,
    store: &crate::schedule_v2::ScheduleStoreV2,
    journal: &crate::schedule_v2::RunJournal,
    cli: &ScheduleCliArgs,
    claim: crate::schedule_v2::RunClaim,
) -> Result<(), ScheduleError> {
    use crate::schedule_v2::{ReceiptUpdate, ScheduleRunState, ScheduleTargetV2, WorkspacePolicy};

    journal
        .transition(
            claim.run_id,
            ScheduleRunState::Running,
            ReceiptUpdate {
                phase: "workspace_preparation".into(),
                ..Default::default()
            },
            Utc::now(),
        )
        .map_err(v2_error)?;

    let reservation = claim
        .definition
        .output_bytes
        .checked_add(64 * 1024 * 1024)
        .ok_or_else(|| ScheduleError::State("lease reservation overflow".into()))?;
    let lease_manager = crate::schedule_v2::LeaseManager::new(
        paths.schedule_private_root(&cli.active_profile),
        2 * 1024 * 1024 * 1024,
    );
    let deadline = Utc::now()
        .checked_add_signed(TimeDelta::seconds(
            i64::try_from(claim.definition.runtime_secs).unwrap_or(i64::MAX),
        ))
        .ok_or_else(|| ScheduleError::State("runtime deadline overflow".into()))?;
    let _lease = match lease_manager.acquire(
        claim.run_id,
        &claim.definition.workspace,
        reservation,
        deadline,
    ) {
        Ok(lease) => lease,
        Err(error) => {
            return fail_v2_claim(
                store,
                journal,
                &claim,
                ScheduleRunState::Interrupted,
                v2_error(error),
            )
        }
    };

    let prepared = if claim.definition.workspace_policy == WorkspacePolicy::IsolatedPinned {
        let definition = claim.definition.clone();
        let private_root = paths.schedule_private_root(&cli.active_profile);
        let result = tokio::task::spawn_blocking(move || {
            crate::schedule_v2::materialize_isolated_workspace(&definition, &private_root)
        })
        .await;
        match result {
            Ok(Ok(prepared)) => Some(prepared),
            Ok(Err(error)) => {
                return fail_v2_claim(
                    store,
                    journal,
                    &claim,
                    ScheduleRunState::Failed,
                    v2_error(error),
                )
            }
            Err(_) => {
                return fail_v2_claim(
                    store,
                    journal,
                    &claim,
                    ScheduleRunState::Interrupted,
                    ScheduleError::State("workspace preparation task failed".into()),
                )
            }
        }
    } else {
        None
    };
    let workspace = prepared
        .as_ref()
        .map_or(claim.definition.workspace.as_path(), |prepared| {
            prepared.workspace.path()
        });
    if claim.definition.workspace_policy == WorkspacePolicy::ExistingReadOnly
        && !claim.definition.target.native()
    {
        let config_paths = paths.clone().for_profile(&cli.active_profile);
        let config = match crate::config::ConfigStore::for_profile(
            config_paths.config_file,
            cli.active_profile.clone(),
        )
        .load()
        {
            Ok(config) => config,
            Err(error) => {
                return fail_v2_claim(
                    store,
                    journal,
                    &claim,
                    ScheduleRunState::Failed,
                    ScheduleError::State(error.to_string()),
                )
            }
        };
        let mode = crate::automation_runtime::admitted_sandbox_mode(
            crate::sandbox::SandboxMode::parse(&config.agent.sandbox_mode),
        );
        if mode != crate::sandbox::SandboxMode::Required
            || !crate::sandbox::SandboxBackend::new(mode).is_available()
        {
            return fail_v2_claim(
                store,
                journal,
                &claim,
                ScheduleRunState::Failed,
                ScheduleError::State(
                    "existing_read_only scheduling requires the available required sandbox".into(),
                ),
            );
        }
    }
    let skill_prompt = match scheduled_skill_prompt(
        paths,
        &cli.active_profile,
        workspace,
        &claim.definition.skills,
    ) {
        Ok(prompt) => prompt,
        Err(error) => {
            return fail_v2_claim(store, journal, &claim, ScheduleRunState::Failed, error)
        }
    };
    let execution_grants = match rebase_schedule_grants(
        &claim.definition.grants,
        &claim.definition.workspace,
        workspace,
    ) {
        Ok(grants) => grants,
        Err(error) => {
            return fail_v2_claim(store, journal, &claim, ScheduleRunState::Failed, error)
        }
    };
    if let Some(prepared) = &prepared {
        journal
            .transition(
                claim.run_id,
                ScheduleRunState::Running,
                ReceiptUpdate {
                    phase: "executing".into(),
                    source_repo: Some(prepared.source_repo_digest.clone()),
                    pinned_revision: Some(prepared.pinned_revision.clone()),
                    tracked_manifest_digest: Some(prepared.tracked_manifest_digest.clone()),
                    workspace_id: Some(prepared.workspace.workspace_id()),
                    ..Default::default()
                },
                Utc::now(),
            )
            .map_err(v2_error)?;
    }

    let execution: Result<String, WorkerFailure> = match &claim.definition.target {
        ScheduleTargetV2::Prompt { prompt } => {
            let provider = claim
                .definition
                .provider
                .clone()
                .ok_or_else(|| WorkerFailure::Failed {
                    code: Some(2),
                    message: "V2 prompt route is missing".into(),
                });
            let model = claim
                .definition
                .model
                .clone()
                .ok_or_else(|| WorkerFailure::Failed {
                    code: Some(2),
                    message: "V2 prompt route is missing".into(),
                });
            match (provider, model) {
                (Ok(provider), Ok(model)) => run_worker(
                    WorkerSpec {
                        automation_budget: None,
                        native_context: None,
                        native_sandbox_binding: None,
                        automation_pricing: None,
                        provider,
                        model,
                        workspace: workspace.to_path_buf(),
                        prompt: format!("{prompt}{skill_prompt}"),
                        tools: claim.definition.tools.clone(),
                        grants: execution_grants.clone(),
                        allow_network: claim.definition.allow_network && cli.allow_network,
                        runtime_secs: claim.definition.runtime_secs,
                        reasoning_effort: None,
                        history: Vec::new(),
                    },
                    &cli.active_profile,
                )
                .await
                .map(|result| result.content),
                (Err(error), _) | (_, Err(error)) => Err(error),
            }
        }
        ScheduleTargetV2::Workflow {
            path,
            workflow_id,
            digest,
        } => {
            let definition = ScheduleDefinition {
                version: DEFINITION_VERSION,
                id: claim.schedule_id.clone(),
                provider: claim.definition.provider.clone().unwrap_or_default(),
                model: claim.definition.model.clone().unwrap_or_default(),
                workspace: workspace.to_path_buf(),
                runtime_secs: claim.definition.runtime_secs,
                grace_secs: 60,
                tools: claim.definition.tools.clone(),
                grants: execution_grants,
                allow_network: claim.definition.allow_network,
                trigger: ScheduleTrigger::Once {
                    at: claim.occurrence.at,
                },
                target: ScheduleTarget::Workflow {
                    path: path.clone(),
                    workflow_id: Some(workflow_id.clone()),
                    digest: Some(digest.clone()),
                },
                delivery: ScheduleDelivery::LocalFile {
                    path: ".ca-v2-unused".into(),
                },
            };
            execute_workflow(
                &definition,
                cli,
                tokio::time::Instant::now() + Duration::from_secs(claim.definition.runtime_secs),
                &skill_prompt,
            )
            .await
            .map(|result| result.content)
        }
        ScheduleTargetV2::ScriptOnly { .. } => {
            execute_schedule_script(paths, &cli.active_profile, &claim.definition, workspace)
                .await
                .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
        }
        target if target.native() => {
            let mut native_schedule = claim.definition.clone();
            native_schedule.workspace = workspace.to_path_buf();
            native_schedule.allow_network &= cli.allow_network;
            let mut budget = crate::automation::AutomationBudgetV1::per_run_default();
            budget.runtime_secs = claim.definition.runtime_secs;
            budget.output_bytes = claim.definition.output_bytes;
            let proposal = crate::automation::AutomationDefinitionV1 {
                schema: crate::automation::AUTOMATION_SCHEMA.into(),
                id: claim.schedule_id.clone(),
                name: claim.schedule_id.clone(),
                automation_kind: crate::automation::AutomationKindV1::Standalone,
                schedule: native_schedule,
                reasoning_effort: None,
                pricing: crate::automation::PricingBindingV1::ModelFree,
                context_sha256: Vec::new(),
                per_run_budget: budget,
                rolling_30_day_budget: crate::automation::AutomationBudgetV1::rolling_default(),
                notification_policy:
                    crate::automation::AutomationNotificationPolicy::MeaningfulChanges,
            };
            match crate::automation::prepare_proposed_definition(
                paths,
                &cli.active_profile,
                proposal,
                Utc::now(),
            ) {
                Ok(definition) => crate::automation_runtime::execute(
                    paths,
                    &cli.active_profile,
                    &definition,
                    claim.run_id,
                    Vec::new(),
                )
                .await
                .map(|result| result.content)
                .map_err(|message| WorkerFailure::Failed {
                    code: Some(6),
                    message,
                }),
                Err(error) => Err(WorkerFailure::Failed {
                    code: Some(2),
                    message: error.to_string(),
                }),
            }
        }
        _ => Err(WorkerFailure::Failed {
            code: Some(2),
            message: "unsupported schedule target".into(),
        }),
    };

    let execution = match (execution, prepared) {
        (Ok(content), Some(prepared)) => {
            let artifact_root = paths
                .schedules_dir(&cli.active_profile)
                .join("schedule-artifacts-v2");
            match tokio::task::spawn_blocking(move || {
                crate::schedule_v2::export_verify_and_cleanup(
                    prepared,
                    &artifact_root,
                    claim.run_id,
                )
            })
            .await
            {
                Ok(Ok(proof)) => {
                    journal
                        .transition(
                            claim.run_id,
                            ScheduleRunState::Running,
                            ReceiptUpdate {
                                phase: "artifact_verified".into(),
                                artifact_digest: Some(proof.digest),
                                artifact_size: Some(proof.size),
                                final_manifest_digest: Some(proof.final_manifest_digest),
                                ..Default::default()
                            },
                            Utc::now(),
                        )
                        .map_err(v2_error)?;
                    Ok(content)
                }
                Ok(Err(error)) => Err(WorkerFailure::Failed {
                    code: Some(8),
                    message: format!("recovery artifact proof failed: {error}"),
                }),
                Err(_) => Err(WorkerFailure::Failed {
                    code: Some(8),
                    message: "recovery artifact proof task failed".into(),
                }),
            }
        }
        (result, _) => result,
    };

    let terminal = match execution {
        Ok(content) => {
            if let Some(native) = ca_automation::runtime::current_run_store() {
                let result = crate::automation_runtime::NativeResult {
                    content: content.clone(),
                    usage: Default::default(),
                };
                let value = serde_json::to_value(result)
                    .map_err(|error| ScheduleError::State(error.to_string()))?;
                native
                    .confirm_action("target", value.clone())
                    .map_err(|error| ScheduleError::State(error.to_string()))?;
                native
                    .finish(
                        ca_automation::runtime::ExecutionState::Succeeded,
                        Some(value),
                        None,
                    )
                    .map_err(|error| ScheduleError::State(error.to_string()))?;
            }
            if content.is_empty() {
                journal
                    .transition(
                        claim.run_id,
                        ScheduleRunState::Succeeded,
                        ReceiptUpdate {
                            phase: "complete".into(),
                            delivery_status: Some("suppressed_empty".into()),
                            outcome: Some("succeeded".into()),
                            ..Default::default()
                        },
                        Utc::now(),
                    )
                    .map_err(v2_error)?;
                Ok(())
            } else {
                journal
                    .transition(
                        claim.run_id,
                        ScheduleRunState::Delivering,
                        ReceiptUpdate {
                            phase: "delivery_intent".into(),
                            delivery_status: Some("intent".into()),
                            ..Default::default()
                        },
                        Utc::now(),
                    )
                    .map_err(v2_error)?;
                match deliver_v2(paths, cli, &claim, &content).await {
                    Ok(()) => {
                        journal
                            .transition(
                                claim.run_id,
                                ScheduleRunState::Succeeded,
                                ReceiptUpdate {
                                    phase: "complete".into(),
                                    delivery_status: Some("acknowledged".into()),
                                    outcome: Some("succeeded".into()),
                                    ..Default::default()
                                },
                                Utc::now(),
                            )
                            .map_err(v2_error)?;
                        Ok(())
                    }
                    Err(error) => {
                        let state = if matches!(
                            &claim.definition.delivery,
                            crate::schedule_v2::ScheduleDeliveryV2::Webhook { .. }
                        ) {
                            ScheduleRunState::Interrupted
                        } else {
                            ScheduleRunState::Failed
                        };
                        journal
                            .transition(
                                claim.run_id,
                                state,
                                ReceiptUpdate {
                                    phase: "delivery_terminal".into(),
                                    delivery_status: Some(
                                        if state == ScheduleRunState::Interrupted {
                                            "ambiguous"
                                        } else {
                                            "failed"
                                        }
                                        .into(),
                                    ),
                                    outcome: Some(error.to_string()),
                                    ..Default::default()
                                },
                                Utc::now(),
                            )
                            .map_err(v2_error)?;
                        Err(error)
                    }
                }
            }
        }
        Err(WorkerFailure::TimedOut) => {
            journal
                .transition(
                    claim.run_id,
                    ScheduleRunState::TimedOut,
                    ReceiptUpdate {
                        phase: "terminal".into(),
                        outcome: Some("runtime deadline exceeded".into()),
                        ..Default::default()
                    },
                    Utc::now(),
                )
                .map_err(v2_error)?;
            Err(ScheduleError::Worker("runtime deadline exceeded".into()))
        }
        Err(WorkerFailure::Failed { message, .. }) => {
            journal
                .transition(
                    claim.run_id,
                    ScheduleRunState::Failed,
                    ReceiptUpdate {
                        phase: "terminal".into(),
                        outcome: Some(message.clone()),
                        ..Default::default()
                    },
                    Utc::now(),
                )
                .map_err(v2_error)?;
            Err(ScheduleError::Worker(message))
        }
    };
    store.settle(&claim).map_err(v2_error)?;
    if terminal.is_err()
        && crate::automation_runtime::inspect_run(
            paths,
            &cli.active_profile,
            &claim.run_id.to_string(),
            true,
            false,
        )
        .is_ok_and(|r| r.state == ca_automation::runtime::ExecutionState::NeedsAttention)
    {
        store
            .set_enabled(&claim.schedule_id, false, Utc::now())
            .map_err(v2_error)?;
    }
    if terminal.is_ok() && admission_output() {
        println!("V2 schedule {} completed", claim.schedule_id);
    }
    terminal
}

pub(crate) struct AutomationExecution {
    pub content: String,
    pub usages: Vec<crate::agent::stream::UsageInfo>,
    pub tool_calls: u32,
    pub artifact_digest: Option<String>,
    pub dispatch_usage: Option<crate::automation::AutomationUsageCountersV1>,
}

fn bounded_automation_schedule(
    schedule: &crate::schedule_v2::ScheduleDefinitionV2,
    budget: &crate::automation::AutomationBudgetV1,
) -> Result<crate::schedule_v2::ScheduleDefinitionV2, String> {
    if budget.output_bytes == 0 || budget.runtime_secs == 0 {
        return Err(
            "Automation output or runtime capacity exhausted before target dispatch".into(),
        );
    }
    let mut schedule = schedule.clone();
    schedule.runtime_secs = schedule.runtime_secs.min(budget.runtime_secs);
    schedule.output_bytes = schedule
        .output_bytes
        .min(budget.output_bytes)
        .min(MAX_RESULT_BYTES as u64);
    if schedule.runtime_secs == 0 || schedule.output_bytes == 0 {
        return Err("Automation target has no admitted output or runtime capacity".into());
    }
    Ok(schedule)
}

fn bounded_target_json<T: serde::Serialize>(
    value: &T,
    limit: Option<usize>,
) -> Result<String, String> {
    let Some(limit) = limit else {
        return serde_json::to_string(value).map_err(|error| error.to_string());
    };
    struct BoundedJson {
        bytes: Vec<u8>,
        limit: usize,
    }
    impl std::io::Write for BoundedJson {
        fn write(&mut self, input: &[u8]) -> std::io::Result<usize> {
            if input.len() > self.limit.saturating_sub(self.bytes.len()) {
                return Err(std::io::Error::other(
                    "Workflow result exceeded its admitted automation output capacity",
                ));
            }
            self.bytes.extend_from_slice(input);
            Ok(input.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut output = BoundedJson {
        bytes: Vec::with_capacity(limit.min(8192)),
        limit,
    };
    serde_json::to_writer(&mut output, value).map_err(|error| error.to_string())?;
    String::from_utf8(output.bytes).map_err(|_| "Workflow result is not valid UTF-8".into())
}

pub(crate) async fn execute_automation_target(
    paths: &crate::runtime::RuntimePaths,
    profile: &crate::runtime::ActiveProfile,
    definition: &crate::automation::AutomationDefinitionV1,
    run_id: Uuid,
    history: Vec<crate::app::Message>,
) -> Result<AutomationExecution, String> {
    let started = std::time::Instant::now();
    let mut result =
        crate::automation_runtime::execute(paths, profile, definition, run_id, history).await?;
    result.usage.runtime_secs = crate::automation_dispatch::charged_runtime_secs(started.elapsed());
    result.usage.output_bytes = result.content.len() as u64;
    Ok(AutomationExecution {
        content: result.content,
        usages: Vec::new(),
        tool_calls: result.usage.tool_calls,
        artifact_digest: None,
        dispatch_usage: Some(result.usage),
    })
}

async fn execute_schedule_script(
    paths: &crate::runtime::RuntimePaths,
    profile: &crate::runtime::ActiveProfile,
    definition: &crate::schedule_v2::ScheduleDefinitionV2,
    workspace: &Path,
) -> Result<crate::schedule_v2::ScriptOutput, WorkerFailure> {
    let config = crate::config::ConfigStore::for_profile(
        paths.clone().for_profile(profile).config_file,
        profile.clone(),
    )
    .load()
    .map_err(|error| WorkerFailure::Failed {
        code: Some(3),
        message: error.to_string(),
    })?;
    let private_paths = ca_automation::runtime::current_run_store()
        .map(|store| crate::automation_runtime::private_command_paths(paths, store.context()))
        .unwrap_or_default();
    crate::jobs::AUTOMATION_PRIVATE_PATHS
        .scope(
            private_paths,
            crate::schedule_v2::execute_script_only(
                definition,
                workspace,
                crate::automation_runtime::admitted_sandbox_mode(
                    crate::sandbox::SandboxMode::parse(&config.agent.sandbox_mode),
                ),
            ),
        )
        .await
        .map_err(|error| WorkerFailure::Failed {
            code: Some(6),
            message: error.to_string(),
        })
}

pub(crate) async fn execute_automation_target_inner(
    paths: &crate::runtime::RuntimePaths,
    profile: &crate::runtime::ActiveProfile,
    definition: &crate::automation::AutomationDefinitionV1,
    run_id: Uuid,
    history: Vec<crate::app::Message>,
) -> Result<AutomationExecution, String> {
    let budget_started = std::time::Instant::now();
    let non_provider_tools = match &definition.schedule.target {
        crate::schedule_v2::ScheduleTargetV2::ScriptOnly { .. }
        | crate::schedule_v2::ScheduleTargetV2::AutomationWorkflow { .. } => 1,
        crate::schedule_v2::ScheduleTargetV2::Mechanistic { actions } => {
            u32::try_from(actions.len())
                .map_err(|_| "Automation action batch exceeds counter range")?
        }
        _ => 0,
    };
    if non_provider_tools > definition.per_run_budget.tool_calls {
        return Err("Automation tool-call budget exhausted before target dispatch".into());
    }
    use crate::schedule_v2::{ScheduleTargetV2, WorkspacePolicy};

    let schedule = bounded_automation_schedule(&definition.schedule, &definition.per_run_budget)?;
    let schedule = &schedule;
    let prepared = if schedule.workspace_policy == WorkspacePolicy::IsolatedPinned {
        let schedule = schedule.clone();
        let private_root = paths.schedule_private_root(profile);
        Some(
            tokio::task::spawn_blocking(move || {
                crate::schedule_v2::materialize_isolated_workspace(&schedule, &private_root)
            })
            .await
            .map_err(|_| "automation workspace preparation task failed".to_string())?
            .map_err(|error| error.to_string())?,
        )
    } else {
        None
    };
    let workspace = prepared
        .as_ref()
        .map_or(schedule.workspace.as_path(), |prepared| {
            prepared.workspace.path()
        });
    if schedule.workspace_policy == WorkspacePolicy::ExistingReadOnly
        && !matches!(
            schedule.target,
            ScheduleTargetV2::Mechanistic { .. } | ScheduleTargetV2::AutomationWorkflow { .. }
        )
    {
        let config_paths = paths.clone().for_profile(profile);
        let config =
            crate::config::ConfigStore::for_profile(config_paths.config_file, profile.clone())
                .load()
                .map_err(|error| error.to_string())?;
        let mode = crate::automation_runtime::admitted_sandbox_mode(
            crate::sandbox::SandboxMode::parse(&config.agent.sandbox_mode),
        );
        if mode != crate::sandbox::SandboxMode::Required
            || !crate::sandbox::SandboxBackend::new(mode).is_available()
        {
            return Err(
                "existing_read_only automation requires the available required sandbox".into(),
            );
        }
    }
    let skill_prompt = scheduled_skill_prompt(paths, profile, workspace, &schedule.skills)
        .map_err(|error| error.to_string())?;
    let grants = rebase_schedule_grants(&schedule.grants, &schedule.workspace, workspace)
        .map_err(|error| error.to_string())?;
    let cli = ScheduleCliArgs {
        active_profile: profile.clone(),
        command: ScheduleCommand::Doctor,
        allow_network: schedule.allow_network,
        allow_webhooks: false,
    };
    let execution = async {
        match &schedule.target {
            ScheduleTargetV2::Prompt { prompt } => {
                let provider = schedule
                    .provider
                    .clone()
                    .ok_or_else(|| WorkerFailure::Failed {
                        code: Some(2),
                        message: "automation prompt route is missing".into(),
                    })?;
                let model = schedule
                    .model
                    .clone()
                    .ok_or_else(|| WorkerFailure::Failed {
                        code: Some(2),
                        message: "automation prompt route is missing".into(),
                    })?;
                run_worker(
                    WorkerSpec {
                        automation_budget: Some(definition.per_run_budget.clone()),
                        native_context: None,
                        native_sandbox_binding: None,
                        automation_pricing: Some(definition.pricing.clone()),
                        provider,
                        model,
                        workspace: workspace.to_path_buf(),
                        prompt: format!(
                            "{prompt}{skill_prompt}{}",
                            crate::automation::heartbeat_report_instruction(
                                &definition.automation_kind
                            )
                        ),
                        tools: schedule.tools.clone(),
                        grants,
                        allow_network: schedule.allow_network,
                        runtime_secs: schedule.runtime_secs,
                        reasoning_effort: definition.reasoning_effort,
                        history,
                    },
                    profile,
                )
                .await
            }
            ScheduleTargetV2::Workflow {
                path,
                workflow_id,
                digest,
            } => {
                let legacy = ScheduleDefinition {
                    version: DEFINITION_VERSION,
                    id: schedule.id.clone(),
                    provider: schedule.provider.clone().unwrap_or_default(),
                    model: schedule.model.clone().unwrap_or_default(),
                    workspace: workspace.to_path_buf(),
                    runtime_secs: schedule.runtime_secs,
                    grace_secs: 60,
                    tools: schedule.tools.clone(),
                    grants,
                    allow_network: schedule.allow_network,
                    trigger: ScheduleTrigger::Once { at: Utc::now() },
                    target: ScheduleTarget::Workflow {
                        path: path.clone(),
                        workflow_id: Some(workflow_id.clone()),
                        digest: Some(digest.clone()),
                    },
                    delivery: ScheduleDelivery::LocalFile {
                        path: ".ca-automation-unused".into(),
                    },
                };
                execute_workflow_with_budget(
                    &legacy,
                    &cli,
                    tokio::time::Instant::now() + Duration::from_secs(schedule.runtime_secs),
                    &skill_prompt,
                    Some(WorkflowAutomationContext {
                        budget: &definition.per_run_budget,
                        pricing: &definition.pricing,
                        reasoning_effort: definition.reasoning_effort,
                    }),
                )
                .await
            }
            ScheduleTargetV2::ScriptOnly { .. } => {
                execute_schedule_script(paths, profile, schedule, workspace)
                    .await
                    .map(|output| WorkerResult {
                        dispatch_usage: None,
                        content: bounded_content(
                            String::from_utf8_lossy(&output.stdout).into_owned(),
                            MAX_RESULT_BYTES,
                        ),
                        successful_tools: HashSet::new(),
                        usages: Vec::new(),
                        tool_calls: 0,
                    })
            }
            ScheduleTargetV2::Mechanistic { actions } => run_scheduled_mechanistic(
                paths,
                profile,
                workspace,
                &run_id.to_string(),
                actions.clone(),
                Some(schedule.output_bytes as usize),
            )
            .await
            .map(|content| WorkerResult {
                dispatch_usage: None,
                content,
                successful_tools: HashSet::new(),
                usages: Vec::new(),
                tool_calls: 0,
            }),
            ScheduleTargetV2::AutomationWorkflow {
                workflow_id,
                workflow_digest,
                resource_binding_digest,
            } => run_scheduled_automation_workflow(
                paths,
                profile,
                workspace,
                workflow_id,
                workflow_digest,
                resource_binding_digest,
                schedule.allow_network,
                Some(schedule.output_bytes as usize),
            )
            .await
            .map(|content| WorkerResult {
                dispatch_usage: None,
                content,
                successful_tools: HashSet::new(),
                usages: Vec::new(),
                tool_calls: 0,
            }),
            ScheduleTargetV2::RepositoryTask { .. }
            | ScheduleTargetV2::HarnessDiscovery { .. }
            | ScheduleTargetV2::HarnessEvaluation { .. }
            | ScheduleTargetV2::HarnessCycle { .. }
            | ScheduleTargetV2::Monitor { .. } => Err(WorkerFailure::Failed {
                code: Some(2),
                message: "native target must use its owned worker".into(),
            }),
        }
    };
    let remaining = Duration::from_secs(
        schedule
            .runtime_secs
            .min(definition.per_run_budget.runtime_secs),
    )
    .saturating_sub(budget_started.elapsed());
    if remaining.is_zero() {
        return Err("Automation runtime budget exhausted before target dispatch".into());
    }
    let mut result = tokio::time::timeout(remaining, execution)
        .await
        .map_err(|_| "Automation runtime budget exhausted".to_string())?
        .map_err(|failure| match failure {
            WorkerFailure::TimedOut => "automation runtime deadline exceeded".into(),
            WorkerFailure::Failed { message, .. } => message,
        })?;
    if result.content.len() as u64 > definition.per_run_budget.output_bytes {
        return Err("Automation retained-output budget exhausted".into());
    }
    if result.dispatch_usage.is_none() {
        result.dispatch_usage = Some(crate::automation::AutomationUsageCountersV1 {
            tool_calls: non_provider_tools,
            output_bytes: result.content.len() as u64,
            ..Default::default()
        });
    }
    if let Some(usage) = result.dispatch_usage.as_mut() {
        usage.runtime_secs =
            crate::automation_dispatch::charged_runtime_secs(budget_started.elapsed())
                .min(definition.per_run_budget.runtime_secs);
    }

    let artifact_digest = if let Some(prepared) = prepared {
        let artifact_root = paths
            .profile_data_dir(profile)
            .join("automations/v1/recovery-artifacts");
        Some(
            tokio::task::spawn_blocking(move || {
                crate::schedule_v2::export_verify_and_cleanup(prepared, &artifact_root, run_id)
            })
            .await
            .map_err(|_| "automation recovery-artifact task failed".to_string())?
            .map_err(|error| error.to_string())?
            .digest,
        )
    } else {
        None
    };
    Ok(AutomationExecution {
        content: result.content,
        usages: result.usages,
        tool_calls: result.tool_calls,
        artifact_digest,
        dispatch_usage: result.dispatch_usage,
    })
}

async fn run_scheduled_mechanistic(
    paths: &crate::runtime::RuntimePaths,
    profile: &crate::runtime::ActiveProfile,
    workspace: &Path,
    run_id: &str,
    actions: Vec<codex_team_protocol::MechanisticActionV1>,
    retained_output_cap: Option<usize>,
) -> Result<String, WorkerFailure> {
    let action_digest = crate::mechanistic_delegation::mechanistic_event_digest(&actions);
    let _ = crate::mechanistic_delegation::record_process_event(
        crate::mechanistic_delegation::MechanisticProcessEvent {
            kind: crate::mechanistic_delegation::MechanisticEventKind::DueSchedule,
            origin: run_id,
            revision: None,
            action_digest: action_digest.as_deref(),
        },
    );
    let config_paths = paths.clone().for_profile(profile);
    let config = crate::config::ConfigStore::for_profile(config_paths.config_file, profile.clone())
        .load()
        .map_err(|error| WorkerFailure::Failed {
            code: Some(3),
            message: error.to_string(),
        })?;
    if !config.mcp.mechanistic.enabled {
        return Err(WorkerFailure::Failed {
            code: Some(3),
            message: "mechanistic scheduling requires an enabled [mcp.mechanistic] profile".into(),
        });
    }
    let workspace = crate::workspace::WorkspaceContext::discover(workspace).map_err(|error| {
        WorkerFailure::Failed {
            code: Some(3),
            message: error.to_string(),
        }
    })?;
    let manager = crate::mcp::McpManager::load_with_oauth_store(
        &config,
        workspace,
        crate::sandbox::SandboxMode::parse(&config.agent.sandbox_mode),
        false,
        profile.clone(),
        crate::mcp_oauth::secret_store_for_profile(profile),
    )
    .map_err(|error| WorkerFailure::Failed {
        code: Some(3),
        message: error.to_string(),
    })?;
    let result = crate::mechanistic_delegation::execute_with_options(
        std::sync::Arc::new(manager),
        &serde_json::json!({ "actions": actions }),
        config.mcp.mechanistic.allow_unisolated_evidence,
    )
    .await;
    if retained_output_cap.is_some_and(|limit| result.persistent_text().len() > limit) {
        return Err(WorkerFailure::Failed {
            code: Some(7),
            message: "Mechanistic result exceeded its admitted automation output capacity".into(),
        });
    }
    let encoded = result.persistent_text().to_string();
    let completed = serde_json::from_str::<serde_json::Value>(&encoded)
        .ok()
        .and_then(|value| {
            value
                .get("status")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .as_deref()
        == Some("completed");
    let _ = crate::mechanistic_delegation::record_process_event(
        crate::mechanistic_delegation::MechanisticProcessEvent {
            kind: crate::mechanistic_delegation::MechanisticEventKind::ScheduledEvidenceCompletion,
            origin: run_id,
            revision: None,
            action_digest: action_digest.as_deref(),
        },
    );
    if result.is_success() && completed {
        Ok(encoded)
    } else {
        Err(WorkerFailure::Failed {
            code: Some(10),
            message: encoded,
        })
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_scheduled_automation_workflow(
    paths: &crate::runtime::RuntimePaths,
    profile: &crate::runtime::ActiveProfile,
    workspace: &Path,
    workflow_id: &str,
    workflow_digest: &str,
    resource_binding_digest: &str,
    cli_allows_network: bool,
    retained_output_cap: Option<usize>,
) -> Result<String, WorkerFailure> {
    if !cli_allows_network {
        return Err(WorkerFailure::Failed {
            code: Some(3),
            message: "scheduled automation workflow requires explicit scheduler network authority"
                .into(),
        });
    }
    let config_paths = paths.clone().for_profile(profile);
    let config = crate::config::ConfigStore::for_profile(config_paths.config_file, profile.clone())
        .load()
        .map_err(|error| WorkerFailure::Failed {
            code: Some(3),
            message: error.to_string(),
        })?;
    if !config.mcp.mechanistic.enabled || !config.mcp.mechanistic.remote.enabled {
        return Err(WorkerFailure::Failed {
            code: Some(3),
            message: "scheduled automation workflow requires enabled mechanistic remote automation"
                .into(),
        });
    }
    let workspace = crate::workspace::WorkspaceContext::discover(workspace).map_err(|error| {
        WorkerFailure::Failed {
            code: Some(3),
            message: error.to_string(),
        }
    })?;
    let manager = crate::mcp::McpManager::load_with_oauth_store(
        &config,
        workspace,
        crate::sandbox::SandboxMode::parse(&config.agent.sandbox_mode),
        false,
        profile.clone(),
        crate::mcp_oauth::secret_store_for_profile(profile),
    )
    .map_err(|error| WorkerFailure::Failed {
        code: Some(3),
        message: error.to_string(),
    })?;
    let request = codex_team_protocol::WorkflowRequestV1 {
        schema: codex_team_protocol::WORKFLOW_REQUEST_SCHEMA.into(),
        operation: codex_team_protocol::WorkflowOperationV1::Run {
            workflow_id: workflow_id.into(),
            draft: None,
            accepted_plan_digest: None,
            expected_workflow_digest: Some(workflow_digest.into()),
            expected_resource_binding_digest: Some(resource_binding_digest.into()),
        },
    };
    let response = manager
        .execute_automation_workflow(&request, true)
        .await
        .map_err(|error| WorkerFailure::Failed {
            code: Some(10),
            message: error.to_string(),
        })?;
    let encoded = bounded_target_json(&response, retained_output_cap).map_err(|error| {
        WorkerFailure::Failed {
            code: Some(10),
            message: error.to_string(),
        }
    })?;
    let completed = matches!(
        response,
        codex_team_protocol::WorkflowResponseV1::Execution {
            receipt: codex_team_protocol::WorkflowExecutionReceiptV1 {
                status: codex_team_protocol::WorkflowExecutionStatusV1::Completed,
                ..
            }
        }
    );
    if completed {
        Ok(encoded)
    } else {
        Err(WorkerFailure::Failed {
            code: Some(10),
            message: encoded,
        })
    }
}

pub(crate) fn rebase_schedule_grants(
    grants: &[crate::permission::PermissionMatcher],
    source: &Path,
    destination: &Path,
) -> Result<Vec<crate::permission::PermissionMatcher>, ScheduleError> {
    let source = source
        .to_str()
        .ok_or_else(|| ScheduleError::State("non-UTF8 schedule roots are unsupported".into()))?;
    let destination = destination
        .to_str()
        .ok_or_else(|| ScheduleError::State("non-UTF8 schedule roots are unsupported".into()))?;
    grants
        .iter()
        .map(|grant| match grant {
            crate::permission::PermissionMatcher::Path { tool, path_pattern } => {
                let suffix = path_pattern.strip_prefix(source).ok_or_else(|| {
                    ScheduleError::InvalidGrant(
                        "isolated path grant is outside the confirmed source root".into(),
                    )
                })?;
                if !suffix.is_empty() && !suffix.starts_with('/') {
                    return Err(ScheduleError::InvalidGrant(
                        "isolated path grant does not start at a path boundary".into(),
                    ));
                }
                let rebased = crate::permission::PermissionMatcher::Path {
                    tool: tool.clone(),
                    path_pattern: format!("{destination}{suffix}"),
                };
                rebased
                    .validate()
                    .map_err(|error| ScheduleError::InvalidGrant(error.to_string()))?;
                Ok(rebased)
            }
            crate::permission::PermissionMatcher::Invocation { tool, arguments }
                if tool == "run_command" && arguments.get("cwd").is_some() =>
            {
                let cwd = arguments
                    .get("cwd")
                    .and_then(serde_json::Value::as_str)
                    .map(Path::new)
                    .ok_or_else(|| {
                        ScheduleError::InvalidGrant(
                            "isolated command grant has an invalid working directory".into(),
                        )
                    })?;
                if !cwd.is_absolute()
                    || cwd
                        .components()
                        .any(|component| matches!(component, std::path::Component::ParentDir))
                {
                    return Err(ScheduleError::InvalidGrant(
                        "isolated command grant working directory must be an absolute bound path"
                            .into(),
                    ));
                }
                let source_root =
                    std::fs::canonicalize(source).map_err(|_| ScheduleError::InvalidWorkspace)?;
                let cwd = std::fs::canonicalize(cwd).map_err(|_| {
                    ScheduleError::InvalidGrant(
                        "isolated command grant working directory is unavailable".into(),
                    )
                })?;
                let suffix = cwd.strip_prefix(&source_root).map_err(|_| {
                    ScheduleError::InvalidGrant(
                        "isolated command grant working directory is outside its source root"
                            .into(),
                    )
                })?;
                let cwd = if suffix.as_os_str().is_empty() {
                    PathBuf::from(destination)
                } else {
                    Path::new(destination).join(suffix)
                };
                let mut arguments = arguments.clone();
                arguments["cwd"] = cwd
                    .to_str()
                    .ok_or_else(|| {
                        ScheduleError::InvalidGrant(
                            "isolated command grant working directory is not UTF-8".into(),
                        )
                    })?
                    .into();
                let rebased = crate::permission::PermissionMatcher::Invocation {
                    tool: tool.clone(),
                    arguments,
                };
                rebased
                    .validate()
                    .map_err(|error| ScheduleError::InvalidGrant(error.to_string()))?;
                Ok(rebased)
            }
            grant => Ok(grant.clone()),
        })
        .collect()
}

fn scheduled_skill_prompt(
    paths: &crate::runtime::RuntimePaths,
    profile: &crate::runtime::ActiveProfile,
    workspace_path: &Path,
    pins: &[crate::schedule_v2::ScheduleSkillPin],
) -> Result<String, ScheduleError> {
    if pins.is_empty() {
        return Ok(String::new());
    }
    let config_paths = paths.clone().for_profile(profile);
    let config = crate::config::ConfigStore::for_profile(config_paths.config_file, profile.clone())
        .load()
        .map_err(|error| ScheduleError::State(error.to_string()))?;
    let workspace = crate::workspace::WorkspaceContext::discover(workspace_path)
        .map_err(|_| ScheduleError::InvalidWorkspace)?;
    let mut roots = workspace
        .skill_ancestor_roots()
        .into_iter()
        .enumerate()
        .map(|(index, path)| {
            crate::skills::SkillDiscoveryRoot::new(
                crate::skills::SkillSourceScope::Workspace,
                path,
                format!("ancestor-{index}"),
            )
        })
        .collect::<Vec<_>>();
    roots.push(crate::skills::SkillDiscoveryRoot::new(
        crate::skills::SkillSourceScope::User,
        paths.agent_user_skills_dir(),
        "user",
    ));
    roots.push(crate::skills::SkillDiscoveryRoot::new(
        crate::skills::SkillSourceScope::Admin,
        "/etc/codex/skills",
        "admin",
    ));
    roots.push(crate::skills::SkillDiscoveryRoot::new(
        crate::skills::SkillSourceScope::Managed,
        paths.managed_skills_dir(profile),
        "managed",
    ));
    roots.push(crate::skills::SkillDiscoveryRoot::new(
        crate::skills::SkillSourceScope::AutoDistill,
        paths.auto_distilled_skills_dir(profile),
        "auto-distill",
    ));
    if let Some(primary) = workspace.primary() {
        roots.push(crate::skills::SkillDiscoveryRoot::new(
            crate::skills::SkillSourceScope::Legacy,
            primary.path.join(".ca/skills"),
            "workspace-ca",
        ));
    }
    roots.push(crate::skills::SkillDiscoveryRoot::new(
        crate::skills::SkillSourceScope::Legacy,
        paths.user_skills_dir(),
        "user-ca",
    ));
    roots.extend(
        config
            .skills
            .compatibility_roots
            .iter()
            .enumerate()
            .map(|(index, path)| {
                let path = if path.is_absolute() {
                    path.clone()
                } else {
                    paths.config_root.join(path)
                };
                crate::skills::SkillDiscoveryRoot::new(
                    crate::skills::SkillSourceScope::Legacy,
                    path,
                    format!("compatibility-{index}"),
                )
            }),
    );
    let mut registry =
        crate::skills::SkillRegistry::discover_portable(roots, &config.skills.disabled);
    registry.apply_exact_state_file(&paths.skill_state_file(profile));
    let mut loads = crate::skills::SkillLoadSet::default();
    for pin in pins {
        let instance = crate::skills::resolve_pinned_instance(
            registry.instances(),
            &pin.instance_id,
            &pin.full_digest,
        )
        .map_err(|error| ScheduleError::State(error.to_string()))?;
        if !instance.enabled {
            return Err(ScheduleError::State(format!(
                "pinned skill {} is disabled",
                pin.instance_id
            )));
        }
        loads.insert(crate::skills::SkillLoad {
            instance_id: pin.instance_id.clone(),
            content_digest: pin.full_digest.clone(),
            requested_resources: Vec::new(),
        });
    }
    registry
        .task_prompt(&loads)
        .map_err(|error| ScheduleError::State(error.to_string()))
}

fn fail_v2_claim(
    store: &crate::schedule_v2::ScheduleStoreV2,
    journal: &crate::schedule_v2::RunJournal,
    claim: &crate::schedule_v2::RunClaim,
    state: crate::schedule_v2::ScheduleRunState,
    error: ScheduleError,
) -> Result<(), ScheduleError> {
    journal
        .transition(
            claim.run_id,
            state,
            crate::schedule_v2::ReceiptUpdate {
                phase: "terminal".into(),
                outcome: Some(error.to_string()),
                ..Default::default()
            },
            Utc::now(),
        )
        .map_err(v2_error)?;
    store.settle(claim).map_err(v2_error)?;
    Err(error)
}

async fn deliver_v2(
    paths: &crate::runtime::RuntimePaths,
    cli: &ScheduleCliArgs,
    claim: &crate::schedule_v2::RunClaim,
    content: &str,
) -> Result<(), ScheduleError> {
    use crate::schedule_v2::ScheduleDeliveryV2;
    match &claim.definition.delivery {
        ScheduleDeliveryV2::None => Ok(()),
        ScheduleDeliveryV2::LocalFile { path } => {
            let output_root = paths
                .schedules_dir(&cli.active_profile)
                .join("schedule-outputs-v2")
                .join(claim.run_id.to_string());
            atomic_local_delivery(&output_root, path, content.as_bytes())
        }
        ScheduleDeliveryV2::Webhook { url, secret_env } => {
            if !cli.allow_webhooks {
                return Err(ScheduleError::Delivery(
                    "webhook delivery requires --allow-webhooks".into(),
                ));
            }
            let secret = std::env::var(secret_env).map_err(|_| {
                ScheduleError::Delivery("webhook secret environment variable is missing".into())
            })?;
            let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
                .map_err(|_| ScheduleError::Delivery("webhook secret is invalid".into()))?;
            mac.update(content.as_bytes());
            let signature = mac
                .finalize()
                .into_bytes()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            let response = reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(20))
                .build()
                .map_err(|_| ScheduleError::Delivery("could not create webhook client".into()))?
                .post(url)
                .header("content-type", "text/plain; charset=utf-8")
                .header("x-ca-run-id", claim.run_id.to_string())
                .header("idempotency-key", claim.run_id.to_string())
                .header("x-ca-signature", format!("sha256={signature}"))
                .body(content.to_owned())
                .send()
                .await
                .map_err(|_| ScheduleError::Delivery("webhook delivery is ambiguous".into()))?;
            if response.status().is_success() {
                Ok(())
            } else {
                Err(ScheduleError::Delivery(format!(
                    "webhook returned HTTP {}",
                    response.status().as_u16()
                )))
            }
        }
        ScheduleDeliveryV2::EmailDraft {
            account,
            recipients,
        } => {
            let config_path = paths.clone().for_profile(&cli.active_profile).config_file;
            let text = std::fs::read_to_string(config_path).map_err(|_| {
                ScheduleError::Delivery("email gateway configuration is unavailable".into())
            })?;
            let config: crate::config::CaConfig = toml::from_str(&text).map_err(|_| {
                ScheduleError::Delivery("email gateway configuration is invalid".into())
            })?;
            config
                .email
                .gateway
                .validate(&config.email.accounts)
                .map_err(|_| {
                    ScheduleError::Delivery("email gateway configuration is invalid".into())
                })?;
            crate::email::gateway::append_scheduled_result(
                &cli.active_profile,
                &config.email.gateway,
                account,
                recipients,
                content,
                &claim.run_id.to_string(),
            )
            .await
            .map_err(|_| {
                ScheduleError::Delivery("email draft append failed or is ambiguous".into())
            })
        }
    }
}

fn mutate_record(
    store: &ScheduleStore,
    id: &str,
    mutate: impl FnOnce(&mut ScheduleRecord) -> Result<(), ScheduleError>,
) -> Result<(), ScheduleError> {
    validate_id(id)?;
    let _lock = store.lock()?;
    let mut data = store.load()?;
    let index = record_index(&data, id)?;
    mutate(&mut data.records[index])?;
    store.save(&mut data)
}

fn record_index(data: &ScheduleStoreData, id: &str) -> Result<usize, ScheduleError> {
    validate_id(id)?;
    data.records
        .iter()
        .position(|record| record.definition.id == id)
        .ok_or_else(|| ScheduleError::Missing(id.to_string()))
}

async fn tick_once(
    store: &ScheduleStore,
    cli: &ScheduleCliArgs,
    now: DateTime<Utc>,
) -> Result<(), ScheduleError> {
    let lock = store.lock()?;
    let mut data = store.load()?;
    let due = data
        .records
        .iter()
        .enumerate()
        .filter(|(_, record)| record.enabled && record.next_run_at.is_some_and(|at| at <= now))
        .min_by_key(|(_, record)| (record.next_run_at, &record.definition.id))
        .map(|(index, _)| index);
    let Some(index) = due else {
        if admission_output() {
            println!("No schedule is due");
        }
        return Ok(());
    };
    let next = data.records[index]
        .next_run_at
        .expect("due record has time");
    let grace = TimeDelta::seconds(data.records[index].definition.grace_secs as i64);
    if now > next + grace {
        let record = &mut data.records[index];
        record.last_run_at = Some(now);
        record.last_outcome = Some(ScheduleOutcome::SkippedLate);
        record.last_exit_code = None;
        advance_after_claim(record, now)?;
        let id = record.definition.id.clone();
        store.save(&mut data)?;
        if admission_output() {
            println!("Skipped late schedule {id}");
        }
        return Ok(());
    }
    advance_after_claim(&mut data.records[index], now)?;
    store.save(&mut data)?;
    let record = data.records[index].clone();
    let revision = data.revision;
    drop(lock);
    execute_claimed(store, record, revision, cli, now).await
}

fn advance_after_claim(
    record: &mut ScheduleRecord,
    now: DateTime<Utc>,
) -> Result<(), ScheduleError> {
    match &record.definition.trigger {
        ScheduleTrigger::Once { .. } => {
            record.enabled = false;
            record.next_run_at = None;
        }
        ScheduleTrigger::Cron { .. } => {
            record.next_run_at = Some(next_invocation(&record.definition.trigger, now)?);
        }
    }
    Ok(())
}

async fn execute_claimed(
    store: &ScheduleStore,
    record: ScheduleRecord,
    claim_revision: u64,
    cli: &ScheduleCliArgs,
    started_at: DateTime<Utc>,
) -> Result<(), ScheduleError> {
    let paths = crate::runtime::RuntimePaths::discover()
        .map_err(|error| ScheduleError::State(error.to_string()))?;
    let mut identity = Sha256::new();
    identity.update(b"ca.legacy-schedule-occurrence.v1");
    identity.update(record.definition.id.as_bytes());
    identity.update(claim_revision.to_be_bytes());
    identity.update(
        serde_json::to_vec(&record.definition)
            .map_err(|error| ScheduleError::State(error.to_string()))?,
    );
    let bytes = identity.finalize();
    let run_id = Uuid::from_bytes(bytes[..16].try_into().expect("SHA256 prefix"));
    let context = crate::schedule_compat_runtime::execution_context(
        &paths,
        &cli.active_profile,
        run_id,
        &record.definition,
        &(cli.allow_network, cli.allow_webhooks),
        &record.definition.workspace,
        record.definition.runtime_secs,
        MAX_RESULT_BYTES as u64 + 8192,
    )
    .map_err(ScheduleError::State)?;
    let run =
        crate::schedule_compat_runtime::LegacyRun::open(context).map_err(ScheduleError::State)?;
    let execution = if let Some(result) = run.cached_result().map_err(ScheduleError::State)? {
        Ok(WorkerResult {
            content: result.content,
            dispatch_usage: None,
            successful_tools: HashSet::new(),
            usages: Vec::new(),
            tool_calls: 0,
        })
    } else {
        run.begin_action("target").map_err(ScheduleError::State)?;
        let result = run
            .scope(Box::pin(execute_record(
                &record.definition,
                cli,
                started_at,
            )))
            .await;
        match &result {
            Ok(result) => {
                run.finish_success(&crate::automation_runtime::NativeResult {
                    content: result.content.clone(),
                    usage: Default::default(),
                })
                .map_err(ScheduleError::State)?;
            }
            Err(_) => {
                run.finish_failure(
                    ca_automation::runtime::ExecutionState::NeedsAttention,
                    "legacy V1 execution stopped with unconfirmed effects",
                )
                .map_err(ScheduleError::State)?;
            }
        }
        result
    };
    let (outcome, exit_code, result) = match execution {
        Ok(result) => (ScheduleOutcome::Succeeded, Some(0), Ok(result)),
        Err(WorkerFailure::TimedOut) => (
            ScheduleOutcome::TimedOut,
            Some(124),
            Err(ScheduleError::Worker("runtime deadline exceeded".into())),
        ),
        Err(WorkerFailure::Failed { code, message }) => (
            ScheduleOutcome::Failed,
            code,
            Err(ScheduleError::Worker(message)),
        ),
    };
    let _lock = store.lock()?;
    let mut data = store.load()?;
    if data.revision != claim_revision {
        return Err(ScheduleError::State(
            "V1 schedule changed while its worker was running".into(),
        ));
    }
    let current = record_index(&data, &record.definition.id)?;
    if data.records[current].definition != record.definition {
        return Err(ScheduleError::State(
            "V1 schedule definition changed while its worker was running".into(),
        ));
    }
    data.records[current].last_run_at = Some(started_at);
    if outcome != ScheduleOutcome::Succeeded {
        data.records[current].enabled = false;
        data.records[current].next_run_at = None;
    }
    data.records[current].last_outcome = Some(outcome);
    data.records[current].last_exit_code = exit_code;
    store.save(&mut data)?;
    match result {
        Ok(_) => {
            if admission_output() {
                println!("Schedule {} completed", record.definition.id);
            }
            Ok(())
        }
        Err(error) => Err(error),
    }
}

#[derive(Debug)]
struct WorkerResult {
    dispatch_usage: Option<crate::automation::AutomationUsageCountersV1>,
    content: String,
    successful_tools: HashSet<String>,
    usages: Vec<crate::agent::stream::UsageInfo>,
    tool_calls: u32,
}

#[derive(Debug)]
enum WorkerFailure {
    TimedOut,
    Failed { code: Option<i32>, message: String },
}

async fn execute_record(
    definition: &ScheduleDefinition,
    cli: &ScheduleCliArgs,
    started_at: DateTime<Utc>,
) -> Result<WorkerResult, WorkerFailure> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(definition.runtime_secs);
    let result = match &definition.target {
        ScheduleTarget::Prompt { prompt } => {
            run_worker(
                WorkerSpec {
                    automation_budget: None,
                    native_context: None,
                    native_sandbox_binding: None,
                    automation_pricing: None,
                    provider: definition.provider.clone(),
                    model: definition.model.clone(),
                    workspace: definition.workspace.clone(),
                    prompt: prompt.clone(),
                    tools: definition.tools.clone(),
                    grants: definition.grants.clone(),
                    allow_network: definition.allow_network && cli.allow_network,
                    runtime_secs: definition.runtime_secs,
                    reasoning_effort: None,
                    history: Vec::new(),
                },
                &cli.active_profile,
            )
            .await?
        }
        ScheduleTarget::Workflow { .. } => execute_workflow(definition, cli, deadline, "").await?,
    };
    let content = bounded_content(result.content, MAX_RESULT_BYTES);
    let envelope = ResultEnvelope {
        schema: "ca.schedule.result.v1",
        schedule_id: definition.id.clone(),
        run_id: ca_automation::runtime::current_run_store()
            .and_then(|store| Uuid::parse_str(&store.context().run_id).ok())
            .unwrap_or_else(Uuid::new_v4),
        provider: definition.provider.clone(),
        model: definition.model.clone(),
        started_at,
        completed_at: Utc::now(),
        outcome: "succeeded",
        content: content.clone(),
    };
    deliver(definition, cli.allow_webhooks, &envelope)
        .await
        .map_err(|error| WorkerFailure::Failed {
            code: Some(7),
            message: error.to_string(),
        })?;
    Ok(WorkerResult {
        content,
        successful_tools: result.successful_tools,
        usages: result.usages,
        tool_calls: result.tool_calls,
        dispatch_usage: result.dispatch_usage,
    })
}

async fn execute_workflow(
    definition: &ScheduleDefinition,
    cli: &ScheduleCliArgs,
    deadline: tokio::time::Instant,
    skill_prompt: &str,
) -> Result<WorkerResult, WorkerFailure> {
    execute_workflow_with_budget(definition, cli, deadline, skill_prompt, None).await
}

#[derive(Clone, Copy)]
struct WorkflowAutomationContext<'a> {
    budget: &'a crate::automation::AutomationBudgetV1,
    pricing: &'a crate::automation::PricingBindingV1,
    reasoning_effort: Option<crate::config::ReasoningEffort>,
}

fn prepare_workflow_worker_spec(
    mut spec: WorkerSpec,
    automation: Option<WorkflowAutomationContext<'_>>,
    usage: Option<&crate::automation::AutomationUsageCountersV1>,
) -> Result<WorkerSpec, WorkerFailure> {
    if let Some(automation) = automation {
        spec.automation_budget = Some(
            crate::automation_dispatch::remaining_budget(
                automation.budget,
                usage.expect("automation accumulator"),
            )
            .map_err(|message| WorkerFailure::Failed {
                code: Some(7),
                message,
            })?,
        );
        spec.automation_pricing = Some(automation.pricing.clone());
        spec.reasoning_effort = automation.reasoning_effort;
    }
    Ok(spec)
}

fn record_workflow_worker_failure(
    run: &mut crate::workflow::WorkflowRun,
    definition: &crate::workflow::WorkflowDefinition,
    automation: bool,
) -> Result<crate::workflow::WorkflowStepCompletion, WorkerFailure> {
    run.fail_provider_attempt(
        definition,
        "scheduled provider step failed",
        false,
        // Failed workers do not return authoritative dispatch counters or
        // proof that no tools ran. An automation must retain its reservation
        // and stop instead of granting the next attempt a fresh budget.
        automation,
        Utc::now(),
    )
    .map_err(|_| WorkerFailure::Failed {
        code: Some(6),
        message: "scheduled workflow failure could not be recorded".into(),
    })
}

async fn execute_workflow_with_budget(
    definition: &ScheduleDefinition,
    cli: &ScheduleCliArgs,
    deadline: tokio::time::Instant,
    skill_prompt: &str,
    automation: Option<WorkflowAutomationContext<'_>>,
) -> Result<WorkerResult, WorkerFailure> {
    let ScheduleTarget::Workflow {
        path,
        workflow_id,
        digest,
    } = &definition.target
    else {
        unreachable!("workflow executor requires workflow target")
    };
    let workspace =
        crate::workspace::WorkspaceContext::discover(&definition.workspace).map_err(|_| {
            WorkerFailure::Failed {
                code: Some(2),
                message: "scheduled workflow workspace is unavailable".into(),
            }
        })?;
    let loaded =
        crate::workflow::load_workflow(&workspace, path).map_err(|_| WorkerFailure::Failed {
            code: Some(2),
            message: "scheduled workflow is unavailable or invalid".into(),
        })?;
    if workflow_id.as_deref() != Some(&loaded.definition.id)
        || digest.as_deref() != Some(&loaded.digest)
    {
        return Err(WorkerFailure::Failed {
            code: Some(2),
            message: "scheduled workflow changed after it was added".into(),
        });
    }
    let mut run = crate::workflow::WorkflowRun::start(
        &loaded,
        definition.provider.clone(),
        definition.model.clone(),
        Utc::now(),
    );
    let mut outputs = Vec::new();
    let mut all_successful_tools = HashSet::new();
    let mut dispatch_usage =
        automation.map(|_| crate::automation::AutomationUsageCountersV1::default());
    while let Some(step) =
        run.next_ready(&loaded.definition)
            .map_err(|_| WorkerFailure::Failed {
                code: Some(6),
                message: "scheduled workflow has no eligible next step".into(),
            })?
    {
        if step
            .model
            .as_deref()
            .is_some_and(|route| route != format!("{}/{}", definition.provider, definition.model))
        {
            return Err(WorkerFailure::Failed {
                code: Some(2),
                message: "scheduled workflow step widens its pinned provider/model".into(),
            });
        }
        let tools = step
            .tools
            .clone()
            .unwrap_or_else(|| definition.tools.clone());
        if tools.iter().any(|tool| !definition.tools.contains(tool)) {
            return Err(WorkerFailure::Failed {
                code: Some(2),
                message: "scheduled workflow step widens its tool set".into(),
            });
        }
        let grants = crate::workflow::resolve_grants(&step.grants, &workspace).map_err(|_| {
            WorkerFailure::Failed {
                code: Some(2),
                message: "scheduled workflow step has invalid grants".into(),
            }
        })?;
        if grants
            .iter()
            .any(|grant| !definition.grants.contains(grant))
        {
            return Err(WorkerFailure::Failed {
                code: Some(2),
                message: "scheduled workflow step widens its grants".into(),
            });
        }
        let prompt =
            crate::workflow::render_step_prompt(&run, &loaded.definition, step).map_err(|_| {
                WorkerFailure::Failed {
                    code: Some(6),
                    message: "scheduled workflow prompt could not be prepared".into(),
                }
            })? + skill_prompt;
        let step_id = step.id.clone();
        run.begin_step(
            &loaded.definition,
            &step_id,
            definition.provider.clone(),
            definition.model.clone(),
            Utc::now(),
        )
        .map_err(|_| WorkerFailure::Failed {
            code: Some(6),
            message: "scheduled workflow step could not start".into(),
        })?;
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Err(WorkerFailure::TimedOut);
        }
        let spec = prepare_workflow_worker_spec(
            WorkerSpec {
                automation_budget: None,
                native_context: None,
                native_sandbox_binding: None,
                automation_pricing: None,
                provider: definition.provider.clone(),
                model: definition.model.clone(),
                workspace: definition.workspace.clone(),
                prompt,
                tools,
                grants,
                allow_network: definition.allow_network && cli.allow_network,
                runtime_secs: remaining.as_secs().max(1),
                reasoning_effort: None,
                history: Vec::new(),
            },
            automation,
            dispatch_usage.as_ref(),
        )?;
        let step_result = run_worker(spec, &cli.active_profile).await;
        let step_result = match step_result {
            Ok(result) => result,
            Err(failure) => {
                let completion = record_workflow_worker_failure(
                    &mut run,
                    &loaded.definition,
                    automation.is_some(),
                )?;
                if completion == crate::workflow::WorkflowStepCompletion::Retry {
                    continue;
                }
                return Err(failure);
            }
        };
        if let Some(total) = dispatch_usage.as_mut() {
            let usage =
                step_result
                    .dispatch_usage
                    .as_ref()
                    .ok_or_else(|| WorkerFailure::Failed {
                        code: Some(7),
                        message: "Automation workflow step omitted host dispatch counters".into(),
                    })?;
            crate::automation_dispatch::add_usage(total, usage).map_err(|message| {
                WorkerFailure::Failed {
                    code: Some(7),
                    message,
                }
            })?;
        }
        for tool in &step_result.successful_tools {
            let mutating = crate::agent::registry::ToolRegistry::builtins()
                .get(tool)
                .is_some_and(|spec| {
                    spec.mutation == crate::agent::registry::ToolMutation::Mutating
                });
            run.observe_tool(tool, true, mutating);
        }
        let completion = run
            .complete_step(&loaded.definition, &step_result.content, Utc::now())
            .map_err(|_| WorkerFailure::Failed {
                code: Some(6),
                message: "scheduled workflow evidence check failed".into(),
            })?;
        if completion == crate::workflow::WorkflowStepCompletion::Retry {
            continue;
        }
        all_successful_tools.extend(step_result.successful_tools);
        outputs.push((step_id, step_result.content));
    }
    let content = outputs
        .into_iter()
        .map(|(id, output)| format!("Step {id}:\n{output}"))
        .collect::<Vec<_>>()
        .join("\n\n");
    let tool_calls = u32::try_from(all_successful_tools.len()).unwrap_or(u32::MAX);
    Ok(WorkerResult {
        dispatch_usage,
        content,
        successful_tools: all_successful_tools,
        usages: Vec::new(),
        tool_calls,
    })
}

const MAX_WORKER_CLEANUPS: usize = 64;
const WORKER_CLEANUP_GRACE: Duration = Duration::from_secs(5);

fn worker_cleanup_slots() -> &'static std::sync::Arc<tokio::sync::Semaphore> {
    static SLOTS: std::sync::OnceLock<std::sync::Arc<tokio::sync::Semaphore>> =
        std::sync::OnceLock::new();
    SLOTS.get_or_init(|| std::sync::Arc::new(tokio::sync::Semaphore::new(MAX_WORKER_CLEANUPS)))
}

/// Called after this terminal has stopped admission and dropped its owned tasks.
/// Cleanup guards retain slots until their process groups have fully drained.
pub(crate) async fn drain_owned_worker_cleanup() {
    let _ = worker_cleanup_slots()
        .clone()
        .acquire_many_owned(MAX_WORKER_CLEANUPS as u32)
        .await;
}

/// Dropping a timed-out execution transfers cleanup ownership to a task. The
/// workspace lease remains live until the worker and its native group drain.
pub(crate) struct WorkerProcessGuard {
    child: Option<tokio::process::Child>,
    process_group: Option<i32>,
    workspace: Option<crate::recovery::WorkspaceActivityLease>,
    slot: Option<tokio::sync::OwnedSemaphorePermit>,
}

pub(crate) struct WorkerProcessAdmission {
    workspace: crate::recovery::WorkspaceActivityLease,
    slot: tokio::sync::OwnedSemaphorePermit,
}

impl WorkerProcessAdmission {
    pub(crate) fn attach(self, child: tokio::process::Child) -> WorkerProcessGuard {
        WorkerProcessGuard::new(child, self.workspace, self.slot)
    }

    pub(crate) fn attach_contained(self, child: tokio::process::Child) -> WorkerProcessGuard {
        let mut guard = WorkerProcessGuard::new(child, self.workspace, self.slot);
        // The outer ca worker owns this group. An inner worker may reap its
        // child, but cannot signal or wait for the group containing itself.
        guard.process_group = None;
        guard
    }
}

pub(crate) async fn admit_native_worker(
    workspace: &Path,
) -> Result<WorkerProcessAdmission, String> {
    let slot = worker_cleanup_slots()
        .clone()
        .try_acquire_owned()
        .map_err(|_| "schedule worker cleanup capacity is exhausted".to_string())?;
    let workspace = workspace.to_path_buf();
    let workspace = tokio::task::spawn_blocking(move || {
        crate::recovery::RecoveryStore::new(std::env::temp_dir())
            .try_workspace_activity(&[workspace])
    })
    .await
    .map_err(|_| "schedule workspace ownership worker stopped".to_string())?
    .map_err(|error| format!("schedule workspace ownership unavailable: {error}"))?;
    Ok(WorkerProcessAdmission { workspace, slot })
}

impl WorkerProcessGuard {
    fn new(
        child: tokio::process::Child,
        workspace: crate::recovery::WorkspaceActivityLease,
        slot: tokio::sync::OwnedSemaphorePermit,
    ) -> Self {
        let process_group = child.id().and_then(|pid| i32::try_from(pid).ok());
        Self {
            child: Some(child),
            process_group,
            workspace: Some(workspace),
            slot: Some(slot),
        }
    }

    pub(crate) fn child_mut(&mut self) -> &mut tokio::process::Child {
        self.child.as_mut().expect("live schedule worker owner")
    }

    pub(crate) fn retire(mut self, graceful: bool) -> tokio::task::JoinHandle<bool> {
        self.spawn_retirement(graceful)
            .expect("live schedule worker owner")
    }

    fn spawn_retirement(&mut self, graceful: bool) -> Option<tokio::task::JoinHandle<bool>> {
        let mut child = self.child.take()?;
        let group = self.process_group;
        let workspace = self.workspace.take();
        let slot = self.slot.take();
        if graceful {
            signal_worker_group(group, nix_signal_interrupt());
        }
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            signal_worker_group(group, nix_signal_kill());
            let _ = child.start_kill();
            return None;
        };
        Some(runtime.spawn(async move {
            if graceful
                && tokio::time::timeout(WORKER_CLEANUP_GRACE, child.wait())
                    .await
                    .is_err()
            {
                signal_worker_group(group, nix_signal_kill());
                let _ = child.start_kill();
            }
            let _ = child.wait().await;
            // A worker can exit after cancelling its shell while grandchildren
            // still retain pipe handles. They share this verified containment
            // group, so drain the remaining group before releasing authority.
            let remaining_native_work = worker_group_active(group);
            if remaining_native_work {
                signal_worker_group(group, nix_signal_kill());
            }
            while worker_group_active(group) {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            drop(workspace);
            drop(slot);
            remaining_native_work
        }))
    }
}

impl Drop for WorkerProcessGuard {
    fn drop(&mut self) {
        self.spawn_retirement(true);
    }
}

#[cfg(unix)]
fn nix_signal_interrupt() -> i32 {
    libc::SIGINT
}
#[cfg(not(unix))]
fn nix_signal_interrupt() -> i32 {
    0
}
#[cfg(unix)]
fn nix_signal_kill() -> i32 {
    libc::SIGKILL
}
#[cfg(not(unix))]
fn nix_signal_kill() -> i32 {
    0
}

fn signal_worker_group(group: Option<i32>, signal: i32) {
    #[cfg(unix)]
    if let Some(group) = group {
        // The parent created this group directly; native worker commands are
        // initialized to inherit it before physical dispatch.
        let _ = unsafe { libc::kill(-group, signal) };
    }
    #[cfg(not(unix))]
    let _ = (group, signal);
}

fn worker_group_active(group: Option<i32>) -> bool {
    #[cfg(unix)]
    if let Some(group) = group {
        return unsafe { libc::kill(-group, 0) } == 0
            || std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH);
    }
    false
}

async fn run_worker(
    mut spec: WorkerSpec,
    profile: &crate::runtime::ActiveProfile,
) -> Result<WorkerResult, WorkerFailure> {
    let native_store = ca_automation::runtime::current_run_store();
    spec.native_context = native_store.as_ref().map(|store| store.context().clone());
    spec.native_sandbox_binding = native_store
        .as_ref()
        .and_then(|_| crate::automation_runtime::current_sandbox_binding());
    let inherited_group = native_store.is_some() && crate::jobs::worker_contained();
    let encoded = serde_json::to_vec(&spec).map_err(|_| WorkerFailure::Failed {
        code: Some(8),
        message: "could not encode schedule worker input".into(),
    })?;
    if encoded.len() > MAX_WORKER_SPEC_BYTES {
        return Err(WorkerFailure::Failed {
            code: Some(2),
            message: "schedule worker input is too large".into(),
        });
    }
    let admission = admit_native_worker(&spec.workspace)
        .await
        .map_err(|error| WorkerFailure::Failed {
            code: Some(8),
            message: error,
        })?;
    let executable = std::env::current_exe().map_err(|_| WorkerFailure::Failed {
        code: Some(8),
        message: "could not locate the ca executable".into(),
    })?;
    let mut command = tokio::process::Command::new(executable);
    command
        .arg("schedule")
        .arg("--profile")
        .arg(profile.label())
        .arg("__worker")
        .current_dir(&spec.workspace)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    {
        if inherited_group {
            command.env(
                "CA_AUTOMATION_PARENT_GROUP",
                unsafe { libc::getpgrp() }.to_string(),
            );
        } else {
            command.process_group(0);
            command.env_remove("CA_AUTOMATION_PARENT_GROUP");
        }
        let descriptors = if let Some(descriptors) =
            crate::schedule_compat_runtime::current_resource_fds()
        {
            Some(descriptors)
        } else if inherited_group {
            let raw =
                std::env::var("CA_AUTOMATION_RESOURCE_FDS").map_err(|_| WorkerFailure::Failed {
                    code: Some(8),
                    message: "native resource ownership is missing for its nested worker".into(),
                })?;
            Some(if raw.is_empty() {
                Vec::new()
            } else {
                raw.split(',')
                    .map(|value| value.parse::<i32>())
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|_| WorkerFailure::Failed {
                        code: Some(8),
                        message: "native resource ownership is invalid for its nested worker"
                            .into(),
                    })?
            })
        } else {
            None
        };
        if let Some(descriptors) = descriptors {
            crate::automation_runtime::inherit_resource_descriptors(&mut command, descriptors);
        }
        if let Some(descriptor) = crate::schedule_compat_runtime::current_executor_fd() {
            command.env("CA_AUTOMATION_EXECUTOR_FD", descriptor.to_string());
            unsafe {
                command.pre_exec(move || {
                    let flags = libc::fcntl(descriptor, libc::F_GETFD);
                    if flags < 0
                        || libc::fcntl(descriptor, libc::F_SETFD, flags & !libc::FD_CLOEXEC) < 0
                    {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
        }
    }
    let child = command.spawn().map_err(|_| WorkerFailure::Failed {
        code: Some(8),
        message: "could not start schedule worker".into(),
    })?;
    let mut worker = if inherited_group {
        admission.attach_contained(child)
    } else {
        admission.attach(child)
    };
    let mut stdin = worker.child_mut().stdin.take().expect("piped stdin");
    let stdout = worker.child_mut().stdout.take().expect("piped stdout");
    let stderr = worker.child_mut().stderr.take().expect("piped stderr");
    let write = tokio::spawn(async move {
        stdin.write_all(&encoded).await?;
        stdin.shutdown().await
    });
    let stdout_read = tokio::spawn(read_bounded(stdout, MAX_WORKER_OUTPUT_BYTES));
    let stderr_read = tokio::spawn(read_bounded(stderr, 64 * 1024));
    let deadline = Duration::from_secs(spec.runtime_secs);
    let cancellation = async {
        let Some(store) = native_store else {
            std::future::pending::<()>().await;
            return;
        };
        loop {
            if store.cancel_requested().unwrap_or(true) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    };
    let status = tokio::select! {
        status = worker.child_mut().wait() => match status {
        Ok(status) => status,
        Err(_) => {
            return Err(WorkerFailure::Failed {
                code: Some(8),
                message: "schedule worker could not be reaped".into(),
            })
        }
        },
        _ = tokio::time::sleep(deadline) => {
            let _ = worker.retire(true).await;
            return Err(WorkerFailure::TimedOut);
        },
        _ = cancellation => {
            let _ = worker.retire(true).await;
            return Err(WorkerFailure::Failed { code: None, message: "schedule worker cancellation requested; recorded effects retained".into() });
        },
    };
    if worker
        .retire(false)
        .await
        .map_err(|_| WorkerFailure::Failed {
            code: Some(8),
            message: "schedule worker cleanup stopped".into(),
        })?
    {
        return Err(WorkerFailure::Failed {
            code: Some(8),
            message:
                "schedule worker left native commands active; containment cleanup stopped them"
                    .into(),
        });
    }
    let _ = write.await;
    let stdout = stdout_read
        .await
        .map_err(|_| WorkerFailure::Failed {
            code: Some(8),
            message: "schedule worker output task failed".into(),
        })?
        .map_err(|_| WorkerFailure::Failed {
            code: Some(8),
            message: "schedule worker output could not be read".into(),
        })?;
    let _stderr = stderr_read.await;
    if stdout.1 {
        return Err(WorkerFailure::Failed {
            code: status.code(),
            message: "schedule worker output exceeded its bound".into(),
        });
    }
    let value: serde_json::Value =
        serde_json::from_slice(&stdout.0).map_err(|_| WorkerFailure::Failed {
            code: status.code(),
            message: "schedule worker returned invalid output".into(),
        })?;
    if !status.success() || value.get("success").and_then(serde_json::Value::as_bool) != Some(true)
    {
        return Err(WorkerFailure::Failed {
            code: status.code(),
            message: "schedule worker reported failure".into(),
        });
    }
    let content = value
        .get("final_message")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| WorkerFailure::Failed {
            code: status.code(),
            message: "schedule worker returned no final message".into(),
        })?;
    let successful_tools = value
        .get("events")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|envelope| envelope.get("event"))
        .filter(|event| {
            event.get("type").and_then(serde_json::Value::as_str) == Some("tool_finish")
        })
        .filter(|event| event.get("status").and_then(serde_json::Value::as_str) == Some("success"))
        .filter_map(|event| event.get("name").and_then(serde_json::Value::as_str))
        .map(str::to_string)
        .collect();
    let usages = value
        .get("events")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|envelope| envelope.get("event"))
        .filter(|event| event.get("type").and_then(serde_json::Value::as_str) == Some("usage"))
        .filter_map(|event| event.get("usage"))
        .filter_map(|usage| serde_json::from_value(usage.clone()).ok())
        .collect();
    let tool_calls = value
        .get("events")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|envelope| envelope.get("event"))
        .filter(|event| {
            event.get("type").and_then(serde_json::Value::as_str) == Some("tool_finish")
        })
        .count()
        .try_into()
        .unwrap_or(u32::MAX);
    Ok(WorkerResult {
        content: bounded_content(content.to_string(), MAX_RESULT_BYTES),
        successful_tools,
        usages,
        tool_calls,
        dispatch_usage: value
            .get("automation_usage")
            .cloned()
            .map(serde_json::from_value)
            .transpose()
            .map_err(|_| WorkerFailure::Failed {
                code: Some(8),
                message: "schedule worker returned invalid automation counters".into(),
            })?,
    })
}

async fn read_bounded<R: AsyncRead + Unpin>(
    mut reader: R,
    limit: usize,
) -> std::io::Result<(Vec<u8>, bool)> {
    let mut kept = Vec::with_capacity(limit.min(8192));
    let mut buffer = [0_u8; 8192];
    let mut omitted = false;
    loop {
        let count = reader.read(&mut buffer).await?;
        if count == 0 {
            break;
        }
        let remaining = limit.saturating_sub(kept.len());
        let take = count.min(remaining);
        kept.extend_from_slice(&buffer[..take]);
        omitted |= take < count;
    }
    Ok((kept, omitted))
}

#[derive(Serialize)]
struct ResultEnvelope {
    schema: &'static str,
    schedule_id: String,
    run_id: Uuid,
    provider: String,
    model: String,
    started_at: DateTime<Utc>,
    completed_at: DateTime<Utc>,
    outcome: &'static str,
    content: String,
}

async fn deliver(
    definition: &ScheduleDefinition,
    allow_webhooks: bool,
    envelope: &ResultEnvelope,
) -> Result<(), ScheduleError> {
    let bytes = serde_json::to_vec_pretty(envelope)
        .map_err(|error| ScheduleError::Delivery(error.to_string()))?;
    if bytes.len() > MAX_RESULT_BYTES + 4096 {
        return Err(ScheduleError::Delivery("result is too large".into()));
    }
    match &definition.delivery {
        ScheduleDelivery::LocalFile { path } => {
            atomic_local_delivery(&definition.workspace, path, &bytes)
        }
        ScheduleDelivery::Webhook { url, secret_env } => {
            if !allow_webhooks {
                return Err(ScheduleError::Delivery(
                    "webhook delivery requires --allow-webhooks".into(),
                ));
            }
            let mut secret = std::env::var(secret_env)
                .map(String::into_bytes)
                .map_err(|_| {
                    ScheduleError::Delivery(format!(
                        "webhook secret environment variable {secret_env} is missing"
                    ))
                })?;
            if secret.is_empty() || secret.len() > MAX_WEBHOOK_SECRET_BYTES {
                secret.fill(0);
                return Err(ScheduleError::Delivery("webhook secret is invalid".into()));
            }
            let mut mac = Hmac::<Sha256>::new_from_slice(&secret)
                .map_err(|_| ScheduleError::Delivery("webhook secret is invalid".into()))?;
            mac.update(&bytes);
            secret.fill(0);
            let signature = mac
                .finalize()
                .into_bytes()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            let client = reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(20))
                .build()
                .map_err(|_| ScheduleError::Delivery("could not create webhook client".into()))?;
            let response = client
                .post(url)
                .header("content-type", "application/json")
                .header("x-ca-signature", format!("sha256={signature}"))
                .body(bytes)
                .send()
                .await
                .map_err(|_| ScheduleError::Delivery("webhook request failed".into()))?;
            if !response.status().is_success() {
                return Err(ScheduleError::Delivery(format!(
                    "webhook returned HTTP {}",
                    response.status().as_u16()
                )));
            }
            Ok(())
        }
    }
}

fn atomic_local_delivery(
    workspace: &Path,
    raw_path: &str,
    bytes: &[u8],
) -> Result<(), ScheduleError> {
    let relative = validate_relative_path(raw_path)?;
    let destination = workspace.join(relative);
    reject_symlink_components(&destination)?;
    let parent = destination
        .parent()
        .ok_or_else(|| ScheduleError::Delivery("local delivery path has no parent".into()))?;
    std::fs::create_dir_all(parent).map_err(|error| ScheduleError::Delivery(error.to_string()))?;
    reject_symlink_components(&destination)?;
    let temp = parent.join(format!(".ca-schedule.{}.tmp", Uuid::new_v4()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| {
        let mut file = options
            .open(&temp)
            .map_err(|error| ScheduleError::Delivery(error.to_string()))?;
        file.write_all(bytes)
            .and_then(|_| file.sync_all())
            .map_err(|error| ScheduleError::Delivery(error.to_string()))?;
        if destination.exists() {
            let metadata = std::fs::symlink_metadata(&destination)
                .map_err(|error| ScheduleError::Delivery(error.to_string()))?;
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                return Err(ScheduleError::Delivery(
                    "local delivery target must be a regular file".into(),
                ));
            }
        }
        std::fs::rename(&temp, &destination)
            .map_err(|error| ScheduleError::Delivery(error.to_string()))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

fn load_definition(
    path: &Path,
    config: &crate::config::CaConfig,
) -> Result<ScheduleDefinition, ScheduleError> {
    let exact_metadata =
        std::fs::symlink_metadata(path).map_err(|_| ScheduleError::InvalidDefinitionFile)?;
    if exact_metadata.file_type().is_symlink() {
        return Err(ScheduleError::InvalidDefinitionFile);
    }
    let path = path
        .canonicalize()
        .map_err(|_| ScheduleError::InvalidDefinitionFile)?;
    reject_symlink_components(&path)?;
    let metadata =
        std::fs::symlink_metadata(&path).map_err(|_| ScheduleError::InvalidDefinitionFile)?;
    if !metadata.is_file() || metadata.len() > MAX_DEFINITION_BYTES {
        return Err(ScheduleError::InvalidDefinitionFile);
    }
    let extension = path.extension().and_then(|value| value.to_str());
    if !matches!(extension, Some("toml" | "json")) {
        return Err(ScheduleError::InvalidDefinitionFile);
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    File::open(&path)
        .and_then(|file| file.take(MAX_DEFINITION_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|_| ScheduleError::InvalidDefinitionFile)?;
    if bytes.len() as u64 > MAX_DEFINITION_BYTES {
        return Err(ScheduleError::InvalidDefinitionFile);
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| ScheduleError::InvalidDefinitionFile)?;
    let definition: ScheduleDefinition = if extension == Some("toml") {
        toml::from_str(text).map_err(|error| ScheduleError::Parse(error.to_string()))?
    } else {
        serde_json::from_str(text).map_err(|error| ScheduleError::Parse(error.to_string()))?
    };
    validate_definition(definition, config)
}

fn validate_definition(
    mut definition: ScheduleDefinition,
    config: &crate::config::CaConfig,
) -> Result<ScheduleDefinition, ScheduleError> {
    if definition.version != DEFINITION_VERSION {
        return Err(ScheduleError::UnsupportedVersion(definition.version));
    }
    validate_id(&definition.id)?;
    validate_route_text(&definition.provider, &definition.model)?;
    let provider = config.providers.get(&definition.provider).ok_or_else(|| {
        ScheduleError::UnknownRoute(definition.provider.clone(), definition.model.clone())
    })?;
    if !provider
        .models
        .iter()
        .any(|model| model == &definition.model)
    {
        return Err(ScheduleError::UnknownRoute(
            definition.provider.clone(),
            definition.model.clone(),
        ));
    }
    let workspace = definition
        .workspace
        .canonicalize()
        .map_err(|_| ScheduleError::InvalidWorkspace)?;
    if !workspace.is_dir() {
        return Err(ScheduleError::InvalidWorkspace);
    }
    definition.workspace = workspace;
    if !(1..=86_400).contains(&definition.runtime_secs)
        || !(1..=3600).contains(&definition.grace_secs)
    {
        return Err(ScheduleError::InvalidLimits);
    }
    let _ = next_invocation(&definition.trigger, Utc::now())?;
    validate_tools_and_grants(
        &definition.tools,
        &definition.grants,
        definition.allow_network,
    )?;
    match &mut definition.target {
        ScheduleTarget::Prompt { prompt } => {
            *prompt = prompt.trim().to_string();
            if prompt.is_empty() || prompt.len() > MAX_PROMPT_BYTES {
                return Err(ScheduleError::InvalidTarget);
            }
        }
        ScheduleTarget::Workflow {
            path,
            workflow_id,
            digest,
        } => {
            let workspace = crate::workspace::WorkspaceContext::discover(&definition.workspace)
                .map_err(|_| ScheduleError::InvalidWorkspace)?;
            let loaded = crate::workflow::load_workflow(&workspace, path)
                .map_err(|_| ScheduleError::InvalidTarget)?;
            *path = loaded.relative_path;
            *workflow_id = Some(loaded.definition.id);
            *digest = Some(loaded.digest);
        }
    }
    validate_delivery(&definition.delivery)?;
    Ok(definition)
}

fn validate_tools_and_grants(
    tools: &[String],
    grants: &[crate::permission::PermissionMatcher],
    allow_network: bool,
) -> Result<(), ScheduleError> {
    if tools.len() > 64 || grants.len() > 64 {
        return Err(ScheduleError::InvalidTool(
            "too many tools or grants".into(),
        ));
    }
    let registry = crate::agent::registry::ToolRegistry::builtins();
    let mut seen = HashSet::new();
    for tool in tools {
        if !seen.insert(tool) || registry.get(tool).is_none() {
            return Err(ScheduleError::InvalidTool(tool.clone()));
        }
        let spec = registry.get(tool).expect("checked");
        if spec.capabilities.iter().any(|capability| {
            matches!(
                capability,
                crate::agent::registry::ToolCapability::Interaction
                    | crate::agent::registry::ToolCapability::Subagent
                    | crate::agent::registry::ToolCapability::Privilege
            )
        }) || matches!(
            spec.handler,
            crate::agent::registry::ToolHandler::WatchProcess
                | crate::agent::registry::ToolHandler::StopWatcher
        ) {
            return Err(ScheduleError::InvalidTool(tool.clone()));
        }
        if spec
            .capabilities
            .contains(&crate::agent::registry::ToolCapability::Network)
            && !allow_network
        {
            return Err(ScheduleError::InvalidTool(format!(
                "{tool} requires allow_network"
            )));
        }
    }
    for grant in grants {
        grant
            .validate()
            .map_err(|error| ScheduleError::InvalidGrant(error.to_string()))?;
        let tool = match grant {
            crate::permission::PermissionMatcher::Tool { tool }
            | crate::permission::PermissionMatcher::Path { tool, .. }
            | crate::permission::PermissionMatcher::Invocation { tool, .. } => tool.as_str(),
            crate::permission::PermissionMatcher::CommandPrefix { .. } => "run_command",
        };
        if !tools.iter().any(|candidate| candidate == tool) {
            return Err(ScheduleError::InvalidGrant(format!(
                "grant for {tool} is outside the schedule tool set"
            )));
        }
    }
    Ok(())
}

fn validate_delivery(delivery: &ScheduleDelivery) -> Result<(), ScheduleError> {
    match delivery {
        ScheduleDelivery::LocalFile { path } => {
            validate_relative_path(path)?;
            Ok(())
        }
        ScheduleDelivery::Webhook { url, secret_env } => {
            if secret_env.is_empty()
                || secret_env.len() > 128
                || !secret_env
                    .bytes()
                    .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
                || !secret_env.as_bytes()[0].is_ascii_uppercase()
            {
                return Err(ScheduleError::InvalidDelivery(
                    "secret_env must be an uppercase environment variable name".into(),
                ));
            }
            let parsed = reqwest::Url::parse(url)
                .map_err(|_| ScheduleError::InvalidDelivery("invalid webhook URL".into()))?;
            if !parsed.username().is_empty()
                || parsed.password().is_some()
                || parsed.query().is_some()
                || parsed.fragment().is_some()
            {
                return Err(ScheduleError::InvalidDelivery(
                    "webhook URL cannot contain credentials, query, or fragment".into(),
                ));
            }
            let literal_loopback = parsed
                .host_str()
                .and_then(|host| host.parse::<std::net::IpAddr>().ok())
                .is_some_and(|ip| ip.is_loopback());
            if parsed.scheme() != "https" && !(parsed.scheme() == "http" && literal_loopback) {
                return Err(ScheduleError::InvalidDelivery(
                    "webhook must use HTTPS or literal-loopback HTTP".into(),
                ));
            }
            Ok(())
        }
    }
}

fn validate_relative_path(value: &str) -> Result<PathBuf, ScheduleError> {
    if value.is_empty() || value.len() > 4096 {
        return Err(ScheduleError::InvalidDelivery("invalid local path".into()));
    }
    let path = Path::new(value);
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(ScheduleError::InvalidDelivery(
            "local delivery path must be relative without traversal".into(),
        ));
    }
    Ok(path.to_path_buf())
}

fn validate_id(value: &str) -> Result<(), ScheduleError> {
    let valid = !value.is_empty()
        && value.len() <= 64
        && value.as_bytes()[0].is_ascii_lowercase()
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
        });
    if valid {
        Ok(())
    } else {
        Err(ScheduleError::InvalidId)
    }
}

fn validate_route_text(provider: &str, model: &str) -> Result<(), ScheduleError> {
    let valid = |value: &str| {
        !value.is_empty()
            && value.len() <= 256
            && !value.chars().any(char::is_control)
            && !value.chars().any(char::is_whitespace)
    };
    if valid(provider) && valid(model) {
        Ok(())
    } else {
        Err(ScheduleError::InvalidRoute)
    }
}

fn next_invocation(
    trigger: &ScheduleTrigger,
    after: DateTime<Utc>,
) -> Result<DateTime<Utc>, ScheduleError> {
    match trigger {
        ScheduleTrigger::Once { at } => Ok(*at),
        ScheduleTrigger::Cron { expression } => {
            let fields = expression.split_whitespace().count();
            let normalized = match fields {
                5 => format!("0 {expression}"),
                6 | 7 => expression.clone(),
                _ => {
                    return Err(ScheduleError::InvalidTrigger(
                        "cron expressions require 5, 6, or 7 fields".into(),
                    ))
                }
            };
            Schedule::from_str(&normalized)
                .map_err(|_| ScheduleError::InvalidTrigger("invalid cron expression".into()))?
                .after(&after)
                .next()
                .ok_or_else(|| {
                    ScheduleError::InvalidTrigger("cron has no future occurrence".into())
                })
        }
    }
}

fn print_list(data: &ScheduleStoreData) {
    for record in &data.records {
        let state = if record.enabled {
            "enabled"
        } else {
            "disabled"
        };
        let next = record
            .next_run_at
            .map(|time| time.to_rfc3339())
            .unwrap_or_else(|| "-".into());
        let last = record
            .last_outcome
            .as_ref()
            .map(|outcome| format!("{outcome:?}").to_ascii_lowercase())
            .unwrap_or_else(|| "never".into());
        let target = match &record.definition.target {
            ScheduleTarget::Prompt { .. } => "prompt",
            ScheduleTarget::Workflow { .. } => "workflow",
        };
        println!(
            "{}\t{}\t{}/{}\t{}\tnext={}\tlast={}\t{}",
            record.definition.id,
            state,
            record.definition.provider,
            record.definition.model,
            target,
            next,
            last,
            record.definition.delivery.safe_label()
        );
    }
}

fn ensure_private_dir(path: &Path) -> Result<(), ScheduleError> {
    reject_symlink_components(path)?;
    std::fs::create_dir_all(path).map_err(|error| ScheduleError::Io(error.to_string()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .map_err(|error| ScheduleError::Io(error.to_string()))?;
    }
    Ok(())
}

fn reject_symlink_components(path: &Path) -> Result<(), ScheduleError> {
    for ancestor in path.ancestors() {
        if ancestor.as_os_str().is_empty() {
            continue;
        }
        if std::fs::symlink_metadata(ancestor)
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
        {
            return Err(ScheduleError::State("path contains a symbolic link".into()));
        }
    }
    Ok(())
}

fn check_owner_only(metadata: &std::fs::Metadata) -> Result<(), ScheduleError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 {
            return Err(ScheduleError::State(
                "schedule store must be owned by the current user and mode 0600".into(),
            ));
        }
    }
    let _ = metadata;
    Ok(())
}

fn bounded_content(mut value: String, limit: usize) -> String {
    if value.len() <= limit {
        return value;
    }
    let marker = format!("\n[schedule result: {} bytes omitted]", value.len() - limit);
    let mut end = limit.saturating_sub(marker.len());
    while !value.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    value.truncate(end);
    value.push_str(&marker);
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workflow_contract_worker_spec() -> WorkerSpec {
        WorkerSpec {
            automation_budget: None,
            native_context: None,
            native_sandbox_binding: None,
            automation_pricing: None,
            provider: "fixture-provider".into(),
            model: "fixture-model".into(),
            workspace: PathBuf::from("/fixture"),
            prompt: "Inspect the fixture".into(),
            tools: Vec::new(),
            grants: Vec::new(),
            allow_network: false,
            runtime_secs: 10,
            reasoning_effort: None,
            history: Vec::new(),
        }
    }

    #[test]
    fn automation_workflow_preserves_frozen_reasoning_in_worker_payload_and_legacy_omits_it() {
        let budget = crate::automation::AutomationBudgetV1::per_run_default();
        let pricing = crate::automation::PricingBindingV1::LocalZero;
        let usage = crate::automation::AutomationUsageCountersV1::default();
        for saved_effort in ["high", "xhigh", "none"] {
            let effort = serde_json::from_value(serde_json::json!(saved_effort)).unwrap();
            let spec = prepare_workflow_worker_spec(
                workflow_contract_worker_spec(),
                Some(WorkflowAutomationContext {
                    budget: &budget,
                    pricing: &pricing,
                    reasoning_effort: Some(effort),
                }),
                Some(&usage),
            )
            .unwrap();
            let wire = serde_json::to_value(&spec).unwrap();
            assert_eq!(wire["reasoning_effort"], saved_effort);
            assert_eq!(wire["provider"], "fixture-provider");
            assert_eq!(wire["model"], "fixture-model");
            assert!(wire.get("automation_budget").is_some());
        }
        for automation in [
            None,
            Some(WorkflowAutomationContext {
                budget: &budget,
                pricing: &pricing,
                reasoning_effort: None,
            }),
        ] {
            let spec = prepare_workflow_worker_spec(
                workflow_contract_worker_spec(),
                automation,
                Some(&usage),
            )
            .unwrap();
            assert!(serde_json::to_value(spec)
                .unwrap()
                .get("reasoning_effort")
                .is_none());
        }
    }

    #[test]
    fn automation_workflow_unknown_failed_usage_stops_before_retry_and_legacy_can_retry() {
        let definition: crate::workflow::WorkflowDefinition =
            serde_json::from_value(serde_json::json!({
                "version": 1, "id": "budget-fixture", "steps": [{
                    "id": "inspect", "prompt": "Inspect the fixture",
                    "retry": {"max_retries": 1, "causes": ["provider_error"]}
                }]
            }))
            .unwrap();
        let loaded = crate::workflow::LoadedWorkflow {
            definition: definition.validate().unwrap(),
            relative_path: "fixture.json".into(),
            digest: "a".repeat(64),
        };
        let mut budget = crate::automation::AutomationBudgetV1::per_run_default();
        budget.provider_requests = 1;
        let pricing = crate::automation::PricingBindingV1::LocalZero;
        let usage = crate::automation::AutomationUsageCountersV1::default();
        let mut attempts = Vec::new();
        for automation_enabled in [false, true] {
            let mut run = crate::workflow::WorkflowRun::start(
                &loaded,
                "fixture-provider".into(),
                "fixture-model".into(),
                Utc::now(),
            );
            let mut dispatches = 0;
            while let Some(step) = run.next_ready(&loaded.definition).unwrap() {
                let step_id = step.id.clone();
                let spec = prepare_workflow_worker_spec(
                    workflow_contract_worker_spec(),
                    automation_enabled.then_some(WorkflowAutomationContext {
                        budget: &budget,
                        pricing: &pricing,
                        reasoning_effort: None,
                    }),
                    Some(&usage),
                )
                .unwrap();
                if automation_enabled {
                    assert_eq!(spec.automation_budget.unwrap().provider_requests, 1);
                }
                run.begin_step(
                    &loaded.definition,
                    &step_id,
                    spec.provider,
                    spec.model,
                    Utc::now(),
                )
                .unwrap();
                dispatches += 1;
                // The worker failed after admission without returning trustworthy
                // counters or evidence that no tools ran. Its reservation remains.
                let completion = record_workflow_worker_failure(
                    &mut run,
                    &loaded.definition,
                    automation_enabled,
                )
                .unwrap();
                if completion != crate::workflow::WorkflowStepCompletion::Retry {
                    break;
                }
            }
            attempts.push(dispatches);
        }
        assert_eq!(attempts[0], 2, "legacy retry behavior changed");
        assert_eq!(
            attempts[1], 1,
            "unknown usage refilled a one-request budget"
        );
    }

    #[test]
    fn automation_target_limits_freeze_before_dispatch_and_bound_encoded_results() {
        let schedule: crate::schedule_v2::ScheduleDefinitionV2 =
            serde_json::from_value(serde_json::json!({
                "version": 2, "id": "bounded", "workspace": "/fixture", "runtime_secs": 10,
                "output_bytes": 4096, "workspace_policy": "existing_read_only",
                "trigger": {"kind": "absolute", "at": Utc::now()},
                "target": {"kind": "mechanistic", "actions": []}, "delivery": {"kind": "none"}
            }))
            .unwrap();
        let mut budget = crate::automation::AutomationBudgetV1::per_run_default();
        budget.runtime_secs = 1;
        budget.output_bytes = 512;
        let admitted = bounded_automation_schedule(&schedule, &budget).unwrap();
        assert_eq!(admitted.runtime_secs, 1);
        assert_eq!(admitted.output_bytes, 512);
        assert_eq!(schedule.runtime_secs, 10);
        assert_eq!(schedule.output_bytes, 4096);
        let escaped = serde_json::json!({"text": "\n".repeat(400)});
        assert!(bounded_target_json(&escaped, Some(admitted.output_bytes as usize)).is_err());
        let encoded = bounded_target_json(&escaped, Some(4096)).unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&encoded).unwrap(),
            escaped
        );
        budget.output_bytes = 0;
        assert!(bounded_automation_schedule(&schedule, &budget).is_err());
    }

    #[cfg(unix)]
    async fn worker_containment_fixture(root: &Path, ignore_interrupt: bool) {
        crate::jobs::enable_worker_containment().unwrap();
        let mut interrupt =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt()).unwrap();
        let jobs = crate::jobs::JobManager::new();
        jobs.start_sandboxed(
            "printf '%s' \"$$\" > native-pid; trap '' INT; sleep 30; printf unreachable",
            root.to_string_lossy(),
            true,
            Duration::from_secs(30),
            vec![root.to_path_buf()],
            false,
            crate::sandbox::SandboxMode::PolicyOnly,
        )
        .await
        .unwrap();
        let harness_root = root.to_path_buf();
        let harness = tokio::spawn(async move {
            let mut command = tokio::process::Command::new("/bin/sh");
            command
                .args([
                    "-c",
                    "printf '%s' \"$$\" > harness-pid; trap '' INT; sleep 30",
                ])
                .current_dir(harness_root);
            crate::harness_engineering::execute_capture(command, Duration::from_secs(30)).await
        });
        tokio::time::timeout(Duration::from_secs(1), async {
            while !root.join("native-pid").exists() || !root.join("harness-pid").exists() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        std::fs::write(root.join("worker-ready"), "ready").unwrap();
        let _ = interrupt.recv().await;
        if ignore_interrupt {
            std::future::pending::<()>().await;
        }
        harness.abort();
        let _ = harness.await;
        jobs.shutdown().await;
        std::fs::write(root.join("cleanup-receipt"), "cleanup").unwrap();
    }

    #[cfg(unix)]
    async fn contained_worker(root: &Path, ignore_interrupt: bool) -> (WorkerProcessGuard, i32) {
        use std::os::unix::process::CommandExt;
        let lease = crate::recovery::RecoveryStore::new(root.join("unused-profile"))
            .try_workspace_activity(&[root.to_path_buf()])
            .unwrap();
        let slot = worker_cleanup_slots().clone().try_acquire_owned().unwrap();
        let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "schedule::tests::worker_future_drop_drains_contained_native_groups",
                "--exact",
                "--nocapture",
            ])
            .env("CA_SCHEDULE_CONTAINMENT_FIXTURE", root)
            .env(
                "CA_SCHEDULE_IGNORE_INTERRUPT",
                if ignore_interrupt { "1" } else { "0" },
            )
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .kill_on_drop(true);
        command.as_std_mut().process_group(0);
        let child = command.spawn().unwrap();
        let group = child.id().unwrap() as i32;
        let mut guard = WorkerProcessGuard::new(child, lease, slot);
        tokio::time::timeout(Duration::from_secs(2), async {
            while !root.join("worker-ready").exists() {
                assert!(
                    guard.child_mut().try_wait().unwrap().is_none(),
                    "contained worker exited before readiness"
                );
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        let native_pid: i32 = std::fs::read_to_string(root.join("native-pid"))
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(
            unsafe { libc::getpgid(native_pid) },
            group,
            "native job escaped worker containment"
        );
        let harness_pid: i32 = std::fs::read_to_string(root.join("harness-pid"))
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(
            unsafe { libc::getpgid(harness_pid) },
            group,
            "harness child escaped worker containment"
        );
        (guard, group)
    }

    #[cfg(unix)]
    async fn assert_worker_group_drained(root: &Path, group: i32, limit: Duration) {
        let store = crate::recovery::RecoveryStore::new(root.join("unused-profile"));
        tokio::time::timeout(limit, async {
            loop {
                if !worker_group_active(Some(group))
                    && store.try_workspace_restore(&[root.to_path_buf()]).is_ok()
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("worker/native group or cleanup ownership remained active");
        assert!(!root.join("unused-profile").exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn worker_future_drop_drains_contained_native_groups() {
        if let Some(root) = std::env::var_os("CA_SCHEDULE_CONTAINMENT_FIXTURE") {
            worker_containment_fixture(
                Path::new(&root),
                std::env::var("CA_SCHEDULE_IGNORE_INTERRUPT").as_deref() == Ok("1"),
            )
            .await;
            return;
        }
        let root = tempfile::tempdir().unwrap();
        let (guard, group) = contained_worker(root.path(), false).await;
        assert!(crate::recovery::RecoveryStore::new(root.path())
            .try_workspace_restore(&[root.path().to_path_buf()])
            .is_err());
        drop(guard);
        assert_worker_group_drained(root.path(), group, Duration::from_secs(4)).await;
        assert!(root.path().join("cleanup-receipt").exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn worker_emergency_kill_drains_native_group_before_releasing_workspace() {
        let root = tempfile::tempdir().unwrap();
        let (guard, group) = contained_worker(root.path(), true).await;
        drop(guard);
        assert!(crate::recovery::RecoveryStore::new(root.path())
            .try_workspace_restore(&[root.path().to_path_buf()])
            .is_err());
        assert_worker_group_drained(
            root.path(),
            group,
            WORKER_CLEANUP_GRACE + Duration::from_secs(3),
        )
        .await;
        assert!(!root.path().join("cleanup-receipt").exists());
    }

    fn config() -> crate::config::CaConfig {
        crate::config::CaConfig::default_config()
    }

    #[test]
    fn interactive_only_tools_are_rejected_by_schedules() {
        for tool in [
            crate::privilege::TOOL_NAME,
            crate::process_watch::WATCH_PROCESS_TOOL_NAME,
            crate::process_watch::STOP_WATCHER_TOOL_NAME,
        ] {
            assert!(validate_tools_and_grants(&[tool.into()], &[], false).is_err());
        }
    }

    #[test]
    fn id_and_delivery_validation_reject_traversal_and_unsafe_webhooks() {
        assert!(validate_id("nightly-1").is_ok());
        assert!(validate_id("../nightly").is_err());
        assert!(validate_relative_path("results/nightly.json").is_ok());
        assert!(validate_relative_path("../escape").is_err());
        assert!(validate_delivery(&ScheduleDelivery::Webhook {
            url: "http://127.0.0.1:9000/hook".into(),
            secret_env: "HOOK_SECRET".into(),
        })
        .is_ok());
        assert!(validate_delivery(&ScheduleDelivery::Webhook {
            url: "http://example.com/hook".into(),
            secret_env: "HOOK_SECRET".into(),
        })
        .is_err());
    }

    #[test]
    fn five_field_cron_has_one_future_occurrence_without_replay() {
        let after = "2026-08-12T12:34:56Z".parse().unwrap();
        let next = next_invocation(
            &ScheduleTrigger::Cron {
                expression: "0 * * * *".into(),
            },
            after,
        )
        .unwrap();
        assert_eq!(next.to_rfc3339(), "2026-08-12T13:00:00+00:00");
    }

    #[test]
    fn store_is_owner_only_atomic_and_duplicate_free() {
        let root = tempfile::tempdir().unwrap();
        let store = ScheduleStore::new(root.path().join("schedules"));
        let mut data = store.load().unwrap();
        assert_eq!(data.revision, 0);
        store.save(&mut data).unwrap();
        assert_eq!(store.load().unwrap().revision, 1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            assert_eq!(
                std::fs::metadata(&store.path).unwrap().mode() & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn definition_debug_redacts_prompt_and_worker_debug_redacts_prompt() {
        let root = tempfile::tempdir().unwrap();
        let mut definition = ScheduleDefinition {
            version: 1,
            id: "nightly".into(),
            provider: "ollama".into(),
            model: "deepseek-coder-v2:16b".into(),
            workspace: root.path().canonicalize().unwrap(),
            runtime_secs: 10,
            grace_secs: 10,
            tools: vec![],
            grants: vec![],
            allow_network: false,
            trigger: ScheduleTrigger::Once { at: Utc::now() },
            target: ScheduleTarget::Prompt {
                prompt: "TOP SECRET PROMPT".into(),
            },
            delivery: ScheduleDelivery::LocalFile {
                path: "out.json".into(),
            },
        };
        let shown = format!("{definition:?}");
        assert!(!shown.contains("TOP SECRET"));
        let worker = WorkerSpec {
            automation_budget: None,
            native_context: None,
            native_sandbox_binding: None,
            automation_pricing: None,
            provider: definition.provider.clone(),
            model: definition.model.clone(),
            workspace: definition.workspace.clone(),
            prompt: "TOP SECRET PROMPT".into(),
            tools: vec![],
            grants: vec![],
            allow_network: false,
            runtime_secs: 10,
            reasoning_effort: None,
            history: Vec::new(),
        };
        assert!(!format!("{worker:?}").contains("TOP SECRET"));
        definition.target = ScheduleTarget::Prompt {
            prompt: "ok".into(),
        };
        let _ = config();
    }

    #[test]
    fn worker_spec_rejects_noncanonical_workspace_and_unscoped_grant() {
        let root = tempfile::tempdir().unwrap();
        let spec = WorkerSpec {
            automation_budget: None,
            native_context: None,
            native_sandbox_binding: None,
            automation_pricing: None,
            provider: "p".into(),
            model: "m".into(),
            workspace: root.path().join("."),
            prompt: "do it".into(),
            tools: vec![],
            grants: vec![crate::permission::PermissionMatcher::Tool {
                tool: "read_file".into(),
            }],
            allow_network: false,
            runtime_secs: 10,
            reasoning_effort: None,
            history: Vec::new(),
        };
        assert!(validate_worker_spec(&spec).is_err());
    }

    #[test]
    fn late_claim_skips_once_and_cron_jumps_to_the_first_future_occurrence() {
        let now: DateTime<Utc> = "2026-08-12T12:34:56Z".parse().unwrap();
        let mut once = ScheduleRecord {
            definition: test_definition(
                ScheduleTrigger::Once {
                    at: now - TimeDelta::minutes(10),
                },
                ScheduleDelivery::LocalFile {
                    path: "out.json".into(),
                },
            ),
            enabled: true,
            next_run_at: Some(now - TimeDelta::minutes(10)),
            last_run_at: None,
            last_outcome: None,
            last_exit_code: None,
        };
        advance_after_claim(&mut once, now).unwrap();
        assert!(!once.enabled);
        assert!(once.next_run_at.is_none());

        let mut cron = ScheduleRecord {
            definition: test_definition(
                ScheduleTrigger::Cron {
                    expression: "* * * * *".into(),
                },
                ScheduleDelivery::LocalFile {
                    path: "out.json".into(),
                },
            ),
            enabled: true,
            next_run_at: Some(now - TimeDelta::days(2)),
            last_run_at: None,
            last_outcome: None,
            last_exit_code: None,
        };
        advance_after_claim(&mut cron, now).unwrap();
        assert_eq!(
            cron.next_run_at.unwrap().to_rfc3339(),
            "2026-08-12T12:35:00+00:00"
        );
    }

    #[test]
    fn webhook_validation_requires_external_secret_reference_and_no_url_authority() {
        for (url, env_name) in [
            ("https://user@example.com/hook", "HOOK_SECRET"),
            ("https://example.com/hook?token=x", "HOOK_SECRET"),
            ("https://example.com/hook#x", "HOOK_SECRET"),
            ("https://example.com/hook", "lowercase"),
        ] {
            assert!(validate_delivery(&ScheduleDelivery::Webhook {
                url: url.into(),
                secret_env: env_name.into(),
            })
            .is_err());
        }
    }

    #[test]
    fn schedule_runtime_path_is_profile_scoped_and_bare_is_rejected() {
        let paths = crate::runtime::RuntimePaths::from_home("/tmp/ca-schedule-home");
        assert_eq!(
            paths.schedules_dir(&crate::runtime::ActiveProfile::Default),
            PathBuf::from("/tmp/ca-schedule-home/.local/share/ca/schedules")
        );
        assert_eq!(
            paths.schedules_dir(&crate::runtime::ActiveProfile::Named("work".into())),
            PathBuf::from("/tmp/ca-schedule-home/.local/share/ca/profiles/work/schedules")
        );
        assert!(parse_args(["ca", "--profile", "bare", "list"]).is_err());
    }

    #[test]
    fn v1_public_and_worker_json_bytes_remain_frozen() {
        let definition = ScheduleDefinition {
            version: 1,
            id: "nightly".into(),
            provider: "fake".into(),
            model: "model".into(),
            workspace: PathBuf::from("/tmp"),
            runtime_secs: 10,
            grace_secs: 60,
            tools: Vec::new(),
            grants: Vec::new(),
            allow_network: false,
            trigger: ScheduleTrigger::Once {
                at: "2026-08-13T12:00:00Z".parse().unwrap(),
            },
            target: ScheduleTarget::Prompt {
                prompt: "safe".into(),
            },
            delivery: ScheduleDelivery::LocalFile {
                path: "out.json".into(),
            },
        };
        assert_eq!(
            serde_json::to_string(&definition).unwrap(),
            r#"{"version":1,"id":"nightly","provider":"fake","model":"model","workspace":"/tmp","runtime_secs":10,"grace_secs":60,"tools":[],"grants":[],"allow_network":false,"trigger":{"kind":"once","at":"2026-08-13T12:00:00Z"},"target":{"kind":"prompt","prompt":"safe"},"delivery":{"kind":"local_file","path":"out.json"}}"#
        );
        let worker = WorkerSpec {
            automation_budget: None,
            native_context: None,
            native_sandbox_binding: None,
            automation_pricing: None,
            provider: "fake".into(),
            model: "model".into(),
            workspace: PathBuf::from("/tmp"),
            prompt: "safe".into(),
            tools: Vec::new(),
            grants: Vec::new(),
            allow_network: false,
            runtime_secs: 10,
            reasoning_effort: None,
            history: Vec::new(),
        };
        assert_eq!(
            serde_json::to_string(&worker).unwrap(),
            r#"{"provider":"fake","model":"model","workspace":"/tmp","prompt":"safe","tools":[],"grants":[],"allow_network":false,"runtime_secs":10}"#
        );
        assert_eq!(
            serde_json::to_string(&ScheduleStoreData::default()).unwrap(),
            r#"{"version":1,"revision":0,"records":[]}"#
        );
    }

    #[test]
    fn isolated_path_grants_rebase_only_at_the_exact_source_boundary() {
        let grants = vec![crate::permission::PermissionMatcher::Path {
            tool: "write_file".into(),
            path_pattern: "/source/project/src/**".into(),
        }];
        assert_eq!(
            rebase_schedule_grants(
                &grants,
                Path::new("/source/project"),
                Path::new("/private/workspace")
            )
            .unwrap(),
            vec![crate::permission::PermissionMatcher::Path {
                tool: "write_file".into(),
                path_pattern: "/private/workspace/src/**".into(),
            }]
        );
        assert!(rebase_schedule_grants(
            &[crate::permission::PermissionMatcher::Path {
                tool: "write_file".into(),
                path_pattern: "/source/project-other/**".into(),
            }],
            Path::new("/source/project"),
            Path::new("/private/workspace")
        )
        .is_err());
    }

    #[test]
    fn isolated_exact_commands_rebind_only_declared_local_cwd() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().canonicalize().unwrap();
        let source = root.join("source");
        let destination = root.join("candidate");
        let outside = root.join("outside");
        for path in [source.join("src"), destination.clone(), outside.clone()] {
            std::fs::create_dir_all(path).unwrap();
        }
        let invocation = |cwd: &Path| crate::permission::PermissionMatcher::Invocation {
            tool: "run_command".into(),
            arguments: serde_json::json!({
                "command":"printf 'fixed command' > result.txt",
                "cwd":cwd,
                "background":false,
                "network":false
            }),
        };
        assert_eq!(
            rebase_schedule_grants(&[invocation(&source.join("src"))], &source, &destination)
                .unwrap(),
            vec![invocation(&destination.join("src"))]
        );
        for cwd in [&outside, &source.join("../outside")] {
            assert!(rebase_schedule_grants(&[invocation(cwd)], &source, &destination).is_err());
        }
        let unbound = crate::permission::PermissionMatcher::Invocation {
            tool: "run_command".into(),
            arguments: serde_json::json!({"command":"true","background":false,"network":false}),
        };
        assert_eq!(
            rebase_schedule_grants(std::slice::from_ref(&unbound), &source, &destination).unwrap(),
            vec![unbound],
            "an omitted working directory must not acquire a new authority binding"
        );
        #[cfg(unix)]
        {
            let escape = source.join("escape");
            std::os::unix::fs::symlink(&outside, &escape).unwrap();
            assert!(rebase_schedule_grants(&[invocation(&escape)], &source, &destination).is_err());
        }
    }

    fn test_definition(trigger: ScheduleTrigger, delivery: ScheduleDelivery) -> ScheduleDefinition {
        ScheduleDefinition {
            version: 1,
            id: "nightly".into(),
            provider: "fake".into(),
            model: "fake-model".into(),
            workspace: PathBuf::from("/tmp"),
            runtime_secs: 10,
            grace_secs: 60,
            tools: Vec::new(),
            grants: Vec::new(),
            allow_network: false,
            trigger,
            target: ScheduleTarget::Prompt {
                prompt: "safe".into(),
            },
            delivery,
        }
    }
}
