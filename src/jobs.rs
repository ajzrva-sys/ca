use std::collections::HashMap;
use std::fmt;
use std::process::Stdio;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{broadcast, Mutex, Notify};
use uuid::Uuid;

const DEFAULT_BUFFER_BYTES: usize = 64 * 1024;
tokio::task_local! {
    /// Supplied by ca's validated automation coordinator, outside tool grants.
    pub(crate) static AUTOMATION_PRIVATE_PATHS: Vec<std::path::PathBuf>;
}
pub(crate) fn automation_private_paths() -> Vec<std::path::PathBuf> {
    let mut paths = AUTOMATION_PRIVATE_PATHS
        .try_with(Clone::clone)
        .unwrap_or_default();
    if let Some(store) = ca_automation::runtime::current_run_store() {
        paths.push(store.context().state_root.clone());
        paths.push(store.context().lease_root.clone());
    }
    paths.sort();
    paths.dedup();
    paths
}
#[cfg(test)]
const EXPERIMENT_PATH: &str = "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin";

#[derive(Debug, Clone, Default)]
enum JobEnvironmentPolicy {
    #[default]
    Inherit,
    #[cfg(test)]
    ExperimentClean {
        home: std::path::PathBuf,
        cargo_home: std::path::PathBuf,
        cargo_target_dir: std::path::PathBuf,
    },
}

/// Process policy for one spawned tool command. Ordinary jobs inherit their
/// existing behavior; experiment jobs use a closed, non-secret environment
/// and a sandbox profile that denies process inspection.
#[derive(Clone, Default)]
pub struct JobExecutionPolicy {
    environment: JobEnvironmentPolicy,
    hide_other_processes: bool,
    authorized_command: Option<String>,
    effective_network_allowed: bool,
    output_bytes: Option<usize>,
}

impl fmt::Debug for JobExecutionPolicy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("JobExecutionPolicy")
            .field(
                "environment",
                &match &self.environment {
                    JobEnvironmentPolicy::Inherit => "inherit",
                    #[cfg(test)]
                    JobEnvironmentPolicy::ExperimentClean { .. } => "experiment_clean",
                },
            )
            .field("hide_other_processes", &self.hide_other_processes)
            .field(
                "authorized_command",
                &self.authorized_command.as_ref().map(|_| "[REDACTED]"),
            )
            .field("effective_network_allowed", &self.effective_network_allowed)
            .finish()
    }
}

impl JobExecutionPolicy {
    #[cfg(test)]
    pub fn experiment_clean(
        home: std::path::PathBuf,
        cargo_home: std::path::PathBuf,
        cargo_target_dir: std::path::PathBuf,
    ) -> Result<Self, String> {
        for path in [&home, &cargo_home, &cargo_target_dir] {
            if !path.is_absolute() || path.as_os_str().is_empty() {
                return Err("experiment command directories must be absolute".into());
            }
        }
        Ok(Self {
            environment: JobEnvironmentPolicy::ExperimentClean {
                home,
                cargo_home,
                cargo_target_dir,
            },
            hide_other_processes: true,
            authorized_command: None,
            effective_network_allowed: false,
            output_bytes: None,
        })
    }

    pub fn for_authorized_command(&self, command: &str, network_allowed: bool) -> Self {
        let mut policy = self.clone();
        if matches!(policy.environment, JobEnvironmentPolicy::Inherit) {
            policy.authorized_command = Some(command.to_string());
            policy.effective_network_allowed = network_allowed;
        }
        policy
    }

    pub fn with_output_limit(mut self, bytes: usize) -> Self {
        self.output_bytes = Some(bytes);
        self
    }

    pub fn retained_output_limit(&self) -> Option<usize> {
        self.output_bytes
    }

    fn apply(&self, process: &mut Command, temporary_directory: &std::path::Path) {
        match &self.environment {
            JobEnvironmentPolicy::Inherit => {
                process
                    .env("TMPDIR", temporary_directory)
                    .env("TMP", temporary_directory)
                    .env("TEMP", temporary_directory);
                if self
                    .authorized_command
                    .as_deref()
                    .is_some_and(|command| command_has_literal_program(command, "cargo"))
                    && !self.effective_network_allowed
                {
                    process.env("CARGO_NET_OFFLINE", "true");
                }
            }
            #[cfg(test)]
            JobEnvironmentPolicy::ExperimentClean {
                home,
                cargo_home,
                cargo_target_dir,
            } => {
                process
                    .env_clear()
                    .env("PATH", EXPERIMENT_PATH)
                    .env("HOME", home)
                    .env("CARGO_HOME", cargo_home)
                    .env("CARGO_TARGET_DIR", cargo_target_dir)
                    .env("CARGO_NET_OFFLINE", "true")
                    // Cargo otherwise queries host parallelism before it can
                    // honor the experiment's sandboxed build policy. A fixed
                    // single-worker build is deterministic and avoids
                    // granting that host-inspection surface.
                    .env("CARGO_BUILD_JOBS", "1")
                    .env("RUST_TEST_THREADS", "1")
                    .env("GIT_CONFIG_NOSYSTEM", "1")
                    .env("GIT_CONFIG_GLOBAL", "/dev/null")
                    .env("LC_ALL", "C")
                    .env("LANG", "C")
                    .env("TZ", "UTC")
                    .env("TMPDIR", temporary_directory);
            }
        }
    }

    fn extend_sandbox_roots(
        &self,
        _writable: &mut Vec<std::path::PathBuf>,
        _read_only: &mut Vec<std::path::PathBuf>,
    ) {
        if matches!(self.environment, JobEnvironmentPolicy::Inherit) {
            _writable.push(std::path::PathBuf::from("/tmp"));
            _read_only.push(std::path::PathBuf::from("/"));
            for path in trusted_developer_roots() {
                if !_read_only.contains(&path) {
                    _read_only.push(path);
                }
            }
            if let Some(command) = self.authorized_command.as_deref() {
                for executable in literal_absolute_programs(command) {
                    if !_read_only.contains(&executable) {
                        _read_only.push(executable);
                    }
                }
                if command_has_literal_program(command, "cargo") {
                    if let Some(registry) = cargo_registry_root() {
                        if !_read_only.contains(&registry) {
                            _read_only.push(registry);
                        }
                    }
                }
            }
        }
        #[cfg(test)]
        if let JobEnvironmentPolicy::ExperimentClean {
            home,
            cargo_home,
            cargo_target_dir,
        } = &self.environment
        {
            for path in [home, cargo_home, cargo_target_dir] {
                if !_writable.contains(path) {
                    _writable.push(path.clone());
                }
            }
            // The compiler may be Homebrew-installed while the Darwin linker
            // resolves its SDK through the system-selected Xcode tree. These
            // are explicit read-only toolchain roots, never user data roots.
            for path in ["/opt/homebrew", "/usr/local", "/Applications/Xcode.app"] {
                let path = std::path::PathBuf::from(path);
                if path.exists() && !_read_only.contains(&path) {
                    _read_only.push(path);
                }
            }
        }
    }
}

#[cfg(test)]
pub fn trusted_executable(path: &std::path::Path) -> bool {
    let Ok(canonical) = path.canonicalize() else {
        return false;
    };
    let Ok(metadata) = std::fs::metadata(&canonical) else {
        return false;
    };
    let fixed_app_bundle = canonical.starts_with("/Applications/ChatGPT.app/Contents/Resources");
    if !metadata.is_file()
        || (!trusted_executable_metadata(&metadata)
            && !(fixed_app_bundle && immutable_foreign_executable_metadata(&metadata)))
    {
        return false;
    }
    trusted_system_roots()
        .into_iter()
        .chain(trusted_developer_roots())
        .any(|root| canonical.starts_with(root))
}

#[cfg(test)]
fn trusted_system_roots() -> Vec<std::path::PathBuf> {
    ["/System", "/usr", "/bin", "/sbin", "/Library"]
        .into_iter()
        .map(std::path::PathBuf::from)
        .collect()
}

fn trusted_developer_roots() -> Vec<std::path::PathBuf> {
    let mut roots = [
        "/opt/homebrew",
        "/usr/local",
        "/Applications/Xcode.app",
        "/Applications/ChatGPT.app/Contents/Resources",
    ]
    .into_iter()
    .map(std::path::PathBuf::from)
    .filter(|path| path.exists())
    .collect::<Vec<_>>();
    if let Some(cargo_home) = cargo_home() {
        let bin = cargo_home.join("bin");
        if bin.is_dir() {
            roots.push(bin);
        }
    }
    if let Some(rustup_home) = crate::run_environment::var_os("RUSTUP_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| user_home().map(|home| home.join(".rustup")))
        .filter(|path| path.is_dir())
    {
        let toolchains = rustup_home.join("toolchains");
        if toolchains.is_dir() {
            roots.push(toolchains);
        }
    }
    roots
}

#[cfg(all(test, unix))]
fn trusted_executable_metadata(metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    let owner = metadata.uid();
    (owner == 0 || owner == unsafe { libc::geteuid() }) && metadata.mode() & 0o002 == 0
}

#[cfg(all(test, unix))]
fn immutable_foreign_executable_metadata(metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    metadata.mode() & 0o022 == 0
}

#[cfg(all(test, not(unix)))]
fn trusted_executable_metadata(_metadata: &std::fs::Metadata) -> bool {
    false
}

#[cfg(all(test, not(unix)))]
fn immutable_foreign_executable_metadata(_metadata: &std::fs::Metadata) -> bool {
    false
}

fn cargo_home() -> Option<std::path::PathBuf> {
    crate::run_environment::var_os("CARGO_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| user_home().map(|home| home.join(".cargo")))
}

fn cargo_registry_root() -> Option<std::path::PathBuf> {
    cargo_home()
        .map(|home| home.join("registry"))
        .filter(|path| path.is_dir())
}

fn user_home() -> Option<std::path::PathBuf> {
    crate::run_environment::var_os("HOME").map(std::path::PathBuf::from)
}

fn command_has_literal_program(command: &str, wanted: &str) -> bool {
    literal_command_programs(command).iter().any(|program| {
        std::path::Path::new(program)
            .file_name()
            .and_then(|name| name.to_str())
            == Some(wanted)
    })
}

fn literal_absolute_programs(command: &str) -> Vec<std::path::PathBuf> {
    literal_command_programs(command)
        .into_iter()
        .map(std::path::PathBuf::from)
        .filter(|path| path.is_absolute() && path.is_file())
        .filter_map(|path| path.canonicalize().ok())
        .collect()
}

fn literal_command_programs(command: &str) -> Vec<String> {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Quote {
        None,
        Single,
        Double,
    }
    let mut programs = Vec::new();
    let mut word = String::new();
    let mut quote = Quote::None;
    let mut escaped = false;
    let mut command_position = true;
    let finish_word =
        |word: &mut String, command_position: &mut bool, programs: &mut Vec<String>| {
            if word.is_empty() {
                return;
            }
            if *command_position {
                if !word.contains(['$', '`']) && !word.contains('=') {
                    programs.push(std::mem::take(word));
                    *command_position = false;
                    return;
                }
                if word.contains('=') {
                    word.clear();
                    return;
                }
            }
            word.clear();
        };
    for character in command.chars() {
        if escaped {
            word.push(character);
            escaped = false;
            continue;
        }
        match quote {
            Quote::Single if character == '\'' => quote = Quote::None,
            Quote::Single => word.push(character),
            Quote::Double if character == '"' => quote = Quote::None,
            Quote::Double if character == '\\' => escaped = true,
            Quote::Double => word.push(character),
            Quote::None if character == '\'' => quote = Quote::Single,
            Quote::None if character == '"' => quote = Quote::Double,
            Quote::None if character == '\\' => escaped = true,
            Quote::None if character.is_whitespace() => {
                finish_word(&mut word, &mut command_position, &mut programs);
            }
            Quote::None if matches!(character, ';' | '|' | '&') => {
                finish_word(&mut word, &mut command_position, &mut programs);
                command_position = true;
            }
            Quote::None => word.push(character),
        }
    }
    if quote == Quote::None && !escaped {
        finish_word(&mut word, &mut command_position, &mut programs);
    }
    programs
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct JobId(Uuid);

impl JobId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct JobOwner {
    pub run_id: Uuid,
    pub session_id: Option<Uuid>,
}

impl JobOwner {
    pub fn new(session_id: Option<Uuid>) -> Self {
        Self {
            run_id: Uuid::new_v4(),
            session_id,
        }
    }

    pub fn process() -> Self {
        Self {
            run_id: Uuid::nil(),
            session_id: None,
        }
    }
}

impl fmt::Display for JobId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl FromStr for JobId {
    type Err = uuid::Error;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Uuid::parse_str(value).map(Self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobStatus {
    Running,
    Cancelling,
    Completed { exit_code: i32 },
    Failed { exit_code: Option<i32> },
    Cancelled,
    TimedOut,
}

/// Identifies who launched a job so lifecycle delivery cannot confuse model
/// tools with direct shell commands or host-internal work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobOrigin {
    ModelTool,
    DirectShell,
    Internal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompletionDelivery {
    Pending,
    AwaitReserved,
    AwaitDelivered,
    AutomaticDelivered,
}

#[derive(Debug, Clone)]
pub enum ForegroundJobOutcome {
    Finished(JobInfo),
    Promoted(JobInfo),
}

impl JobStatus {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed { .. } | Self::Failed { .. } | Self::Cancelled | Self::TimedOut
        )
    }

    pub fn label(self) -> String {
        match self {
            Self::Running => "running".into(),
            Self::Cancelling => "cancelling".into(),
            Self::Completed { exit_code } => format!("completed ({exit_code})"),
            Self::Failed { exit_code } => {
                exit_code.map_or_else(|| "failed".into(), |code| format!("failed ({code})"))
            }
            Self::Cancelled => "cancelled".into(),
            Self::TimedOut => "timed out".into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobStream {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobEvent {
    Started(JobId),
    Output {
        id: JobId,
        stream: JobStream,
        text: String,
    },
    Status {
        id: JobId,
        status: JobStatus,
    },
}

#[derive(Debug, Clone)]
pub struct JobInfo {
    pub id: JobId,
    pub command: String,
    pub cwd: String,
    pub status: JobStatus,
    pub elapsed: Duration,
    pub stdout: String,
    pub stderr: String,
    pub stdout_omitted_bytes: usize,
    pub stderr_omitted_bytes: usize,
    pub background: bool,
}

impl JobInfo {
    pub fn formatted_output(&self) -> String {
        let mut result = match self.status {
            JobStatus::Completed { exit_code }
            | JobStatus::Failed {
                exit_code: Some(exit_code),
            } => format!("exit code: {exit_code}"),
            _ => format!("status: {}", self.status.label()),
        };
        let stdout_present = !self.stdout.is_empty() || self.stdout_omitted_bytes > 0;
        let stderr_present = !self.stderr.is_empty() || self.stderr_omitted_bytes > 0;
        let labels = usize::from(stdout_present) * "\nstdout:\n".len()
            + usize::from(stderr_present) * "\nstderr:\n".len();
        let available = (4_usize * 1024).saturating_sub(result.len() + labels);
        let stdout_budget = if stdout_present && stderr_present {
            available.div_ceil(2)
        } else if stdout_present {
            available
        } else {
            0
        };
        let stderr_budget = if stdout_present && stderr_present {
            available / 2
        } else if stderr_present {
            available
        } else {
            0
        };
        if stdout_present {
            result.push_str("\nstdout:\n");
            result.push_str(&model_stream_output(
                &self.stdout,
                self.stdout_omitted_bytes,
                "stdout",
                stdout_budget,
            ));
        }
        if stderr_present {
            result.push_str("\nstderr:\n");
            result.push_str(&model_stream_output(
                &self.stderr,
                self.stderr_omitted_bytes,
                "stderr",
                stderr_budget,
            ));
        }
        result
    }
}

fn model_stream_output(value: &str, capture_omitted: usize, label: &str, budget: usize) -> String {
    if value.len() <= budget {
        return value.to_string();
    }
    let original_marker = format!("\n[… {label}: {capture_omitted} bytes omitted …]\n");
    if capture_omitted > 0 {
        if let Some((head, tail)) = value.split_once(&original_marker) {
            let retained = budget.saturating_sub(original_marker.len());
            let head = utf8_prefix(head, retained.div_ceil(2));
            let tail = utf8_suffix(tail, retained / 2);
            return format!("{head}{original_marker}{tail}");
        }
    }
    bounded_utf8(value, budget, label)
}

fn bounded_utf8(value: &str, budget: usize, label: &str) -> String {
    if value.len() <= budget {
        return value.into();
    }
    let mut retained = budget.saturating_sub(40 + label.len());
    loop {
        let head = utf8_prefix(value, retained.div_ceil(2));
        let tail = utf8_suffix(value, retained / 2);
        let omitted = value.len().saturating_sub(head.len() + tail.len());
        let marker = format!("\n[… {label}: {omitted} bytes omitted …]\n");
        let len = head.len() + marker.len() + tail.len();
        if len <= budget || retained == 0 {
            return format!("{head}{marker}{tail}");
        }
        retained = retained.saturating_sub(len - budget);
    }
}

fn utf8_prefix(value: &str, bytes: usize) -> &str {
    let mut end = bytes.min(value.len());
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

fn utf8_suffix(value: &str, bytes: usize) -> &str {
    let mut start = value.len().saturating_sub(bytes);
    while start < value.len() && !value.is_char_boundary(start) {
        start += 1;
    }
    &value[start..]
}

#[derive(Debug)]
struct JobState {
    command: String,
    cwd: String,
    status: JobStatus,
    started_at: Instant,
    finished_at: Option<Instant>,
    stdout: BoundedText,
    stderr: BoundedText,
    background: bool,
    owner: JobOwner,
    origin: JobOrigin,
    completion_delivery: CompletionDelivery,
    timeout_forced: bool,
    keep_awake_lease: Option<crate::keep_awake::KeepAwakeLease>,
    workspace_activity_lease: Option<crate::recovery::WorkspaceActivityLease>,
}

#[derive(Debug)]
struct JobRecord {
    state: Mutex<JobState>,
    child: Mutex<Option<Child>>,
    stdin: Mutex<Option<ChildStdin>>,
    notify: Notify,
    readers_remaining: AtomicUsize,
    readers_notify: Notify,
    temporary_directory: std::path::PathBuf,
    independent_process_group: bool,
}

#[derive(Debug)]
struct Inner {
    jobs: Mutex<HashMap<JobId, Arc<JobRecord>>>,
    events: broadcast::Sender<JobEvent>,
    buffer_bytes: usize,
}

#[derive(Debug, Clone)]
pub struct JobManager {
    inner: Arc<Inner>,
}

/// A foreground command belongs to the task awaiting it, unless explicitly
/// promoted. Aborting that task must stop the command, not just its waiter.
struct ForegroundWaitGuard {
    manager: JobManager,
    id: JobId,
    owner: JobOwner,
}

impl Drop for ForegroundWaitGuard {
    fn drop(&mut self) {
        let manager = self.manager.clone();
        let id = self.id;
        let owner = self.owner;
        tokio::spawn(async move {
            let _ = manager.cancel_owned(id, owner, true).await;
        });
    }
}

static PROCESS_JOB_MANAGER: LazyLock<JobManager> = LazyLock::new(JobManager::new);
static WORKER_CONTAINMENT: AtomicBool = AtomicBool::new(false);

/// Host-only initialization for the private schedule worker. Native commands
/// inherit this owned group so the supervising parent can drain them together.
pub(crate) fn enable_worker_containment() -> Result<(), String> {
    #[cfg(unix)]
    {
        if nix::unistd::getpgrp() != nix::unistd::getpid() {
            return Err("Schedule worker must own its containment process group".into());
        }
        WORKER_CONTAINMENT.store(true, Ordering::Release);
    }
    Ok(())
}

/// A nested schedule worker stays in the native worker's already owned group.
/// Its caller must first validate the execution context and inherited leases.
/// Group membership remains sufficient if the dispatching parent has exited;
/// the leases retain authority until the last contained worker exits.
pub(crate) fn enable_inherited_worker_containment(expected_group: i32) -> Result<(), String> {
    validate_inherited_worker_group(expected_group)?;
    WORKER_CONTAINMENT.store(true, Ordering::Release);
    Ok(())
}

fn validate_inherited_worker_group(expected_group: i32) -> Result<(), String> {
    #[cfg(unix)]
    {
        if expected_group <= 1 || nix::unistd::getpgrp().as_raw() != expected_group {
            return Err("Schedule worker does not belong to its admitted process group".into());
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = expected_group;
        Err("Inherited schedule worker containment is unavailable on this platform".into())
    }
}

pub(crate) fn worker_contained() -> bool {
    WORKER_CONTAINMENT.load(Ordering::Acquire)
}

/// Internal harness transports carry no resource authority into the trial.
/// This marker only retains the parent's already selected process group.
pub(crate) fn inherit_harness_worker_containment() -> Result<(), String> {
    if let Some(value) = std::env::var_os("CA_HARNESS_PARENT_GROUP") {
        let group = value
            .to_str()
            .and_then(|value| value.parse::<i32>().ok())
            .ok_or("Invalid harness containment group")?;
        enable_inherited_worker_containment(group)?;
    }
    Ok(())
}

#[cfg(unix)]
pub(crate) fn owned_child_process_group() -> i32 {
    if worker_contained() {
        nix::unistd::getpgrp().as_raw()
    } else {
        0
    }
}

impl JobManager {
    pub fn new() -> Self {
        Self::with_buffer_bytes(DEFAULT_BUFFER_BYTES)
    }
    pub fn process() -> Self {
        PROCESS_JOB_MANAGER.clone()
    }
    pub fn with_buffer_bytes(buffer_bytes: usize) -> Self {
        let (events, _) = broadcast::channel(256);
        Self {
            inner: Arc::new(Inner {
                jobs: Mutex::new(HashMap::new()),
                events,
                buffer_bytes: buffer_bytes.max(1),
            }),
        }
    }
    pub fn subscribe(&self) -> broadcast::Receiver<JobEvent> {
        self.inner.events.subscribe()
    }

    #[allow(dead_code)] // compatibility entry point for existing embedders/tests
    pub async fn start(
        &self,
        command: impl Into<String>,
        cwd: impl Into<String>,
        background: bool,
        timeout: Duration,
    ) -> Result<JobId, String> {
        self.start_sandboxed(
            command,
            cwd,
            background,
            timeout,
            Vec::new(),
            false,
            crate::sandbox::SandboxMode::PolicyOnly,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    #[allow(dead_code)] // ownerless compatibility wrapper
    pub async fn start_sandboxed(
        &self,
        command: impl Into<String>,
        cwd: impl Into<String>,
        background: bool,
        timeout: Duration,
        roots: Vec<std::path::PathBuf>,
        network_allowed: bool,
        sandbox_mode: crate::sandbox::SandboxMode,
    ) -> Result<JobId, String> {
        self.start_sandboxed_owned(
            command,
            cwd,
            background,
            timeout,
            roots,
            network_allowed,
            sandbox_mode,
            JobOwner::process(),
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn start_sandboxed_owned(
        &self,
        command: impl Into<String>,
        cwd: impl Into<String>,
        background: bool,
        timeout: Duration,
        roots: Vec<std::path::PathBuf>,
        network_allowed: bool,
        sandbox_mode: crate::sandbox::SandboxMode,
        owner: JobOwner,
    ) -> Result<JobId, String> {
        self.start_sandboxed_owned_with_policy(
            command,
            cwd,
            background,
            timeout,
            roots,
            network_allowed,
            sandbox_mode,
            owner,
            JobExecutionPolicy::default(),
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn start_sandboxed_owned_with_origin(
        &self,
        command: impl Into<String>,
        cwd: impl Into<String>,
        background: bool,
        timeout: Duration,
        roots: Vec<std::path::PathBuf>,
        network_allowed: bool,
        sandbox_mode: crate::sandbox::SandboxMode,
        owner: JobOwner,
        origin: JobOrigin,
    ) -> Result<JobId, String> {
        self.start_sandboxed_owned_with_policy_and_origin(
            command,
            cwd,
            background,
            timeout,
            roots,
            network_allowed,
            sandbox_mode,
            owner,
            JobExecutionPolicy::default(),
            origin,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn start_sandboxed_owned_with_policy(
        &self,
        command: impl Into<String>,
        cwd: impl Into<String>,
        background: bool,
        timeout: Duration,
        roots: Vec<std::path::PathBuf>,
        network_allowed: bool,
        sandbox_mode: crate::sandbox::SandboxMode,
        owner: JobOwner,
        execution_policy: JobExecutionPolicy,
    ) -> Result<JobId, String> {
        self.start_sandboxed_owned_with_policy_and_origin(
            command,
            cwd,
            background,
            timeout,
            roots,
            network_allowed,
            sandbox_mode,
            owner,
            execution_policy,
            JobOrigin::Internal,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn start_sandboxed_owned_with_policy_and_origin(
        &self,
        command: impl Into<String>,
        cwd: impl Into<String>,
        background: bool,
        timeout: Duration,
        roots: Vec<std::path::PathBuf>,
        network_allowed: bool,
        sandbox_mode: crate::sandbox::SandboxMode,
        owner: JobOwner,
        execution_policy: JobExecutionPolicy,
        origin: JobOrigin,
    ) -> Result<JobId, String> {
        let command = command.into();
        let cwd = cwd.into();
        // The job owns its workspace independently of the provider/UI waiter,
        // including manual shell commands and promoted background commands.
        // The transient registry never creates or changes a profile store.
        let mut activity_roots = roots.clone();
        activity_roots.push(std::path::PathBuf::from(&cwd));
        let workspace_activity_lease = tokio::task::spawn_blocking(move || {
            crate::recovery::RecoveryStore::new(std::env::temp_dir())
                .try_workspace_activity(&activity_roots)
        })
        .await
        .map_err(|_| "Workspace ownership worker stopped before command dispatch".to_string())?
        .map_err(|error| format!("Workspace ownership refused command dispatch: {error}"))?;
        let temporary_directory = std::env::temp_dir().join(format!("ca-job-{}", Uuid::new_v4()));
        std::fs::create_dir(&temporary_directory)
            .map_err(|error| format!("Error creating job temporary directory: {error}"))?;
        let mut roots = roots;
        let mut read_only_roots = Vec::new();
        execution_policy.extend_sandbox_roots(&mut roots, &mut read_only_roots);
        let cwd_path = std::path::PathBuf::from(&cwd);
        if !roots.iter().any(|root| cwd_path.starts_with(root))
            && !read_only_roots
                .iter()
                .any(|root| cwd_path.starts_with(root))
        {
            read_only_roots.push(cwd_path);
        }
        let backend = crate::sandbox::SandboxBackend::new(
            crate::automation_runtime::admitted_sandbox_mode(sandbox_mode),
        );
        let native_command = ca_automation::runtime::current_run_store().is_some()
            || crate::automation_runtime::current_sandbox_binding().is_some();
        let private_paths = automation_private_paths();
        let prepared = match backend.prepare_process_with_denied_paths(
            std::path::Path::new("/bin/sh"),
            &["-c".into(), command.clone()],
            &crate::sandbox::SandboxPolicy {
                roots,
                read_only_roots,
                temporary_directory: temporary_directory.clone(),
                network_allowed,
                hide_other_processes: execution_policy.hide_other_processes || native_command,
            },
            &private_paths,
        ) {
            Ok(prepared) => prepared,
            Err(error) => {
                let _ = std::fs::remove_dir(&temporary_directory);
                return Err(error.to_string());
            }
        };
        let mut process = Command::new(&prepared.program);
        process
            .args(&prepared.arguments)
            .current_dir(&cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if native_command {
            // Provider credentials belong to the trusted ca runner. A physical
            // command receives only ordinary runtime/toolchain variables.
            process.env_clear();
            for name in [
                "HOME",
                "PATH",
                "USER",
                "LOGNAME",
                "LANG",
                "LC_ALL",
                "CARGO_HOME",
                "RUSTUP_HOME",
            ] {
                if let Some(value) = std::env::var_os(name) {
                    process.env(name, value);
                }
            }
        }
        execution_policy.apply(&mut process, &temporary_directory);
        if !native_command {
            crate::run_environment::apply_tokio(&mut process);
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            if !WORKER_CONTAINMENT.load(Ordering::Acquire) {
                process.as_std_mut().process_group(0);
            }
        }
        let mut child = match process.spawn() {
            Ok(child) => child,
            Err(error) => {
                let _ = std::fs::remove_dir(&temporary_directory);
                return Err(format!("Error running command: {error}"));
            }
        };
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "Command stdout was unavailable".to_string())?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| "Command stderr was unavailable".to_string())?;
        let stdin = child.stdin.take();
        let buffer_bytes = execution_policy
            .output_bytes
            .map_or(self.inner.buffer_bytes, |bytes| {
                self.inner
                    .buffer_bytes
                    // Invalid UTF-8 can expand to three bytes per captured
                    // byte when rendered. Split that worst case across both
                    // streams and reserve space for omission labels/framing.
                    .min(bytes.saturating_sub(512).saturating_div(6).max(1))
            });
        let id = JobId::new();
        let record = Arc::new(JobRecord {
            state: Mutex::new(JobState {
                command,
                cwd,
                status: JobStatus::Running,
                started_at: Instant::now(),
                finished_at: None,
                stdout: BoundedText::new(buffer_bytes),
                stderr: BoundedText::new(buffer_bytes),
                background,
                owner,
                origin,
                completion_delivery: CompletionDelivery::Pending,
                timeout_forced: false,
                keep_awake_lease: (origin == JobOrigin::ModelTool)
                    .then(crate::keep_awake::acquire_process)
                    .flatten(),
                workspace_activity_lease: Some(workspace_activity_lease),
            }),
            child: Mutex::new(Some(child)),
            stdin: Mutex::new(stdin),
            notify: Notify::new(),
            readers_remaining: AtomicUsize::new(2),
            readers_notify: Notify::new(),
            temporary_directory,
            independent_process_group: !WORKER_CONTAINMENT.load(Ordering::Acquire),
        });
        self.inner.jobs.lock().await.insert(id, record.clone());
        let _ = self.inner.events.send(JobEvent::Started(id));
        self.spawn_reader(id, record.clone(), stdout, JobStream::Stdout);
        self.spawn_reader(id, record.clone(), stderr, JobStream::Stderr);
        self.spawn_watcher(id, record, timeout);
        Ok(id)
    }

    fn spawn_reader<R>(&self, id: JobId, record: Arc<JobRecord>, mut reader: R, stream: JobStream)
    where
        R: AsyncRead + Unpin + Send + 'static,
    {
        let inner = self.inner.clone();
        tokio::spawn(async move {
            let mut chunk = [0u8; 4096];
            loop {
                let count = match reader.read(&mut chunk).await {
                    Ok(0) | Err(_) => break,
                    Ok(count) => count,
                };
                let text = String::from_utf8_lossy(&chunk[..count]).into_owned();
                let mut state = record.state.lock().await;
                match stream {
                    JobStream::Stdout => state.stdout.push(&chunk[..count]),
                    JobStream::Stderr => state.stderr.push(&chunk[..count]),
                }
                drop(state);
                let _ = inner.events.send(JobEvent::Output { id, stream, text });
            }
            record.readers_remaining.fetch_sub(1, Ordering::AcqRel);
            record.readers_notify.notify_waiters();
        });
    }

    fn spawn_watcher(&self, id: JobId, record: Arc<JobRecord>, timeout: Duration) {
        let inner = self.inner.clone();
        tokio::spawn(async move {
            let started_at = Instant::now();
            let mut forced_status = None;
            loop {
                let exit = {
                    let mut child = record.child.lock().await;
                    match child.as_mut() {
                        Some(child) => child.try_wait(),
                        None => break,
                    }
                };
                match exit {
                    Ok(Some(exit)) => {
                        *record.child.lock().await = None;
                        *record.stdin.lock().await = None;
                        while record.readers_remaining.load(Ordering::Acquire) != 0 {
                            let notified = record.readers_notify.notified();
                            if record.readers_remaining.load(Ordering::Acquire) == 0 {
                                break;
                            }
                            notified.await;
                        }
                        let status = if let Some(status) = forced_status {
                            status
                        } else {
                            let current = record.state.lock().await.status;
                            if current == JobStatus::Cancelling {
                                JobStatus::Cancelled
                            } else if exit.success() {
                                JobStatus::Completed {
                                    exit_code: exit.code().unwrap_or(0),
                                }
                            } else {
                                JobStatus::Failed {
                                    exit_code: exit.code(),
                                }
                            }
                        };
                        set_terminal_status(&inner, id, &record, status).await;
                        cleanup_job_temp(&record.temporary_directory);
                        break;
                    }
                    Ok(None) => {}
                    Err(_) => {
                        set_terminal_status(
                            &inner,
                            id,
                            &record,
                            forced_status.unwrap_or(JobStatus::Failed { exit_code: None }),
                        )
                        .await;
                        cleanup_job_temp(&record.temporary_directory);
                        break;
                    }
                }
                if forced_status.is_none() && started_at.elapsed() >= timeout {
                    let should_timeout = {
                        let mut state = record.state.lock().await;
                        if state.background || state.timeout_forced || state.status.is_terminal() {
                            false
                        } else {
                            state.timeout_forced = true;
                            true
                        }
                    };
                    if should_timeout {
                        forced_status = Some(JobStatus::TimedOut);
                        terminate_child(&record).await;
                    }
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        });
    }

    #[allow(dead_code)] // ownerless compatibility wrapper
    pub async fn list(&self) -> Vec<JobInfo> {
        let records = self
            .inner
            .jobs
            .lock()
            .await
            .iter()
            .map(|(id, record)| (*id, record.clone()))
            .collect::<Vec<_>>();
        let mut jobs = Vec::with_capacity(records.len());
        for (id, record) in records {
            jobs.push(snapshot_record(id, &record).await);
        }
        jobs.sort_by_key(|job| std::cmp::Reverse(job.elapsed));
        jobs
    }

    pub async fn list_owned(&self, owner: JobOwner) -> Vec<JobInfo> {
        let records = self
            .inner
            .jobs
            .lock()
            .await
            .iter()
            .map(|(id, record)| (*id, record.clone()))
            .collect::<Vec<_>>();
        let mut jobs = Vec::new();
        for (id, record) in records {
            if ensure_owner(&record, owner).await.is_ok() {
                jobs.push(snapshot_record(id, &record).await);
            }
        }
        jobs.sort_by_key(|job| std::cmp::Reverse(job.elapsed));
        jobs
    }

    pub async fn get(&self, id: JobId) -> Option<JobInfo> {
        let record = self.inner.jobs.lock().await.get(&id).cloned()?;
        Some(snapshot_record(id, &record).await)
    }

    #[allow(dead_code)] // compatibility entry point for existing embedders/tests
    pub async fn get_owned(&self, id: JobId, owner: JobOwner) -> Option<JobInfo> {
        let record = self.inner.jobs.lock().await.get(&id).cloned()?;
        ensure_owner(&record, owner).await.ok()?;
        Some(snapshot_record(id, &record).await)
    }

    #[allow(dead_code)] // ownerless compatibility wrapper
    pub async fn await_job(&self, id: JobId) -> Result<JobInfo, String> {
        self.await_job_owned(id, JobOwner::process()).await
    }

    pub async fn await_job_owned(&self, id: JobId, owner: JobOwner) -> Result<JobInfo, String> {
        let record = self
            .inner
            .jobs
            .lock()
            .await
            .get(&id)
            .cloned()
            .ok_or_else(|| format!("Unknown job: {id}"))?;
        ensure_owner(&record, owner).await?;
        loop {
            let notified = record.notify.notified();
            if record.state.lock().await.status.is_terminal() {
                return Ok(snapshot_record(id, &record).await);
            }
            notified.await;
        }
    }

    /// Wait for an initially foreground job. Promotion wakes the waiter and
    /// transfers lifecycle responsibility without restarting the process.
    pub async fn await_foreground_job_owned(
        &self,
        id: JobId,
        owner: JobOwner,
    ) -> Result<ForegroundJobOutcome, String> {
        let _guard = ForegroundWaitGuard {
            manager: self.clone(),
            id,
            owner,
        };
        let record = self
            .inner
            .jobs
            .lock()
            .await
            .get(&id)
            .cloned()
            .ok_or_else(|| format!("Unknown job: {id}"))?;
        ensure_owner(&record, owner).await?;
        loop {
            let notified = record.notify.notified();
            let state = record.state.lock().await;
            if state.status.is_terminal() {
                drop(state);
                return Ok(ForegroundJobOutcome::Finished(
                    snapshot_record(id, &record).await,
                ));
            }
            if state.background {
                drop(state);
                return Ok(ForegroundJobOutcome::Promoted(
                    snapshot_record(id, &record).await,
                ));
            }
            drop(state);
            notified.await;
        }
    }

    pub async fn promote_owned(&self, id: JobId, owner: JobOwner) -> Result<JobInfo, String> {
        let record = self
            .inner
            .jobs
            .lock()
            .await
            .get(&id)
            .cloned()
            .ok_or_else(|| format!("Unknown job: {id}"))?;
        ensure_owner(&record, owner).await?;
        {
            let mut state = record.state.lock().await;
            if state.status.is_terminal() {
                return Err(format!("Job {id} has already finished"));
            }
            if state.status == JobStatus::Cancelling {
                return Err(format!("Job {id} is already stopping"));
            }
            if state.timeout_forced {
                return Err(format!("Job {id} is already timing out"));
            }
            if state.background {
                return Err(format!("Job {id} is already in the background"));
            }
            if !matches!(state.origin, JobOrigin::ModelTool | JobOrigin::DirectShell) {
                return Err(format!("Job {id} cannot be promoted"));
            }
            state.background = true;
        }
        record.notify.notify_waiters();
        Ok(snapshot_record(id, &record).await)
    }

    pub async fn promotable_foreground_owned(&self, owner: JobOwner) -> Option<JobId> {
        let records = self
            .inner
            .jobs
            .lock()
            .await
            .iter()
            .map(|(id, record)| (*id, record.clone()))
            .collect::<Vec<_>>();
        let mut selected = None;
        for (id, record) in records {
            let state = record.state.lock().await;
            if state.owner == owner
                && state.status == JobStatus::Running
                && !state.background
                && !state.timeout_forced
                && matches!(state.origin, JobOrigin::ModelTool | JobOrigin::DirectShell)
                && selected
                    .as_ref()
                    .is_none_or(|(_, started_at)| state.started_at > *started_at)
            {
                selected = Some((id, state.started_at));
            }
        }
        selected.map(|(id, _)| id)
    }

    /// Reserve explicit completion observation before waiting. Automatic
    /// delivery skips reserved jobs, closing the terminal-event race.
    pub async fn reserve_completion_await_owned(
        &self,
        id: JobId,
        owner: JobOwner,
    ) -> Result<(), String> {
        let record = self
            .inner
            .jobs
            .lock()
            .await
            .get(&id)
            .cloned()
            .ok_or_else(|| format!("Unknown job: {id}"))?;
        ensure_owner(&record, owner).await?;
        let mut state = record.state.lock().await;
        if state.origin == JobOrigin::ModelTool
            && state.completion_delivery == CompletionDelivery::Pending
        {
            state.completion_delivery = CompletionDelivery::AwaitReserved;
        }
        Ok(())
    }

    pub async fn finish_completion_await_owned(
        &self,
        id: JobId,
        owner: JobOwner,
        timed_out: bool,
    ) -> Result<JobInfo, String> {
        let record = self
            .inner
            .jobs
            .lock()
            .await
            .get(&id)
            .cloned()
            .ok_or_else(|| format!("Unknown job: {id}"))?;
        ensure_owner(&record, owner).await?;
        {
            let mut state = record.state.lock().await;
            if state.completion_delivery == CompletionDelivery::AwaitReserved {
                if state.status.is_terminal() || !timed_out {
                    state.completion_delivery = CompletionDelivery::AwaitDelivered;
                } else {
                    state.completion_delivery = CompletionDelivery::Pending;
                }
            }
        }
        Ok(snapshot_record(id, &record).await)
    }

    /// Atomically claims all completed, background, model-originated jobs for
    /// one host-authored continuation. Claimed jobs can never be claimed by an
    /// explicit await or another automatic continuation.
    pub async fn claim_completed_model_jobs_owned(&self, owner: JobOwner) -> Vec<JobInfo> {
        let records = self
            .inner
            .jobs
            .lock()
            .await
            .iter()
            .map(|(id, record)| (*id, record.clone()))
            .collect::<Vec<_>>();
        let mut claimed = Vec::new();
        for (id, record) in records {
            let claim = {
                let mut state = record.state.lock().await;
                if state.owner == owner
                    && state.origin == JobOrigin::ModelTool
                    && state.background
                    && state.status.is_terminal()
                    && state.completion_delivery == CompletionDelivery::Pending
                {
                    state.completion_delivery = CompletionDelivery::AutomaticDelivered;
                    true
                } else {
                    false
                }
            };
            if claim {
                claimed.push(snapshot_record(id, &record).await);
            }
        }
        claimed.sort_by_key(|job| job.elapsed);
        claimed
    }

    pub async fn active_model_background_jobs_owned(&self, owner: JobOwner) -> Vec<JobInfo> {
        let records = self
            .inner
            .jobs
            .lock()
            .await
            .iter()
            .map(|(id, record)| (*id, record.clone()))
            .collect::<Vec<_>>();
        let mut active = Vec::new();
        for (id, record) in records {
            let include = {
                let state = record.state.lock().await;
                state.owner == owner
                    && state.origin == JobOrigin::ModelTool
                    && state.background
                    && !state.status.is_terminal()
            };
            if include {
                active.push(snapshot_record(id, &record).await);
            }
        }
        active
    }

    pub async fn shutdown_model_background_jobs_owned(&self, owner: JobOwner) -> Vec<JobInfo> {
        let active = self.active_model_background_jobs_owned(owner).await;
        for job in &active {
            let _ = self.kill_owned(job.id, owner).await;
        }
        let mut outcomes = Vec::with_capacity(active.len());
        for job in active {
            if let Ok(Ok(outcome)) =
                tokio::time::timeout(Duration::from_secs(2), self.await_job_owned(job.id, owner))
                    .await
            {
                outcomes.push(outcome);
            }
        }
        outcomes
    }

    #[allow(dead_code)] // ownerless compatibility wrapper
    pub async fn write_stdin(&self, id: JobId, input: &str) -> Result<(), String> {
        self.write_stdin_owned(id, input, JobOwner::process()).await
    }

    pub async fn write_stdin_owned(
        &self,
        id: JobId,
        input: &str,
        owner: JobOwner,
    ) -> Result<(), String> {
        let record = self
            .inner
            .jobs
            .lock()
            .await
            .get(&id)
            .cloned()
            .ok_or_else(|| format!("Unknown job: {id}"))?;
        ensure_owner(&record, owner).await?;
        if record.state.lock().await.status != JobStatus::Running {
            return Err(format!("Job {id} is not running"));
        }
        let mut stdin = record.stdin.lock().await;
        let stdin = stdin
            .as_mut()
            .ok_or_else(|| format!("Job {id} stdin is closed"))?;
        stdin
            .write_all(input.as_bytes())
            .await
            .map_err(|error| format!("Unable to write job stdin: {error}"))?;
        stdin
            .flush()
            .await
            .map_err(|error| format!("Unable to flush job stdin: {error}"))
    }

    #[allow(dead_code)] // ownerless compatibility wrapper
    pub async fn kill(&self, id: JobId) -> Result<(), String> {
        self.kill_owned(id, JobOwner::process()).await
    }

    pub async fn kill_owned(&self, id: JobId, owner: JobOwner) -> Result<(), String> {
        self.cancel_owned(id, owner, false).await
    }

    async fn cancel_owned(
        &self,
        id: JobId,
        owner: JobOwner,
        foreground_only: bool,
    ) -> Result<(), String> {
        let record = self
            .inner
            .jobs
            .lock()
            .await
            .get(&id)
            .cloned()
            .ok_or_else(|| format!("Unknown job: {id}"))?;
        ensure_owner(&record, owner).await?;
        {
            let mut state = record.state.lock().await;
            if state.status.is_terminal() || (foreground_only && state.background) {
                return Ok(());
            }
            state.status = JobStatus::Cancelling;
        }
        let _ = self.inner.events.send(JobEvent::Status {
            id,
            status: JobStatus::Cancelling,
        });
        terminate_child(&record).await;
        Ok(())
    }

    pub async fn stop_and_await_owned(
        &self,
        id: JobId,
        owner: JobOwner,
        grace: Duration,
    ) -> Result<JobInfo, String> {
        if grace > Duration::from_secs(300) {
            return Err("grace_secs must be between 0 and 300".into());
        }
        let record = self
            .inner
            .jobs
            .lock()
            .await
            .get(&id)
            .cloned()
            .ok_or_else(|| format!("Unknown job: {id}"))?;
        ensure_owner(&record, owner).await?;
        {
            let mut state = record.state.lock().await;
            if state.status.is_terminal() {
                drop(state);
                return Ok(snapshot_record(id, &record).await);
            }
            state.status = JobStatus::Cancelling;
        }
        let _ = self.inner.events.send(JobEvent::Status {
            id,
            status: JobStatus::Cancelling,
        });
        terminate_child_gracefully(&record).await;
        if !grace.is_zero()
            && tokio::time::timeout(grace, wait_for_terminal(&record))
                .await
                .is_ok()
        {
            return Ok(snapshot_record(id, &record).await);
        }
        terminate_child(&record).await;
        tokio::time::timeout(Duration::from_secs(5), wait_for_terminal(&record))
            .await
            .map_err(|_| format!("Job {id} did not terminate after process-group escalation"))?;
        Ok(snapshot_record(id, &record).await)
    }

    pub async fn shutdown(&self) {
        let records = self
            .inner
            .jobs
            .lock()
            .await
            .iter()
            .map(|(id, record)| (*id, record.clone()))
            .collect::<Vec<_>>();
        let mut active = Vec::new();
        for (id, record) in records {
            let state = record.state.lock().await;
            if !state.status.is_terminal() {
                active.push((id, state.owner));
            }
        }
        for (id, owner) in &active {
            let _ = self.kill_owned(*id, *owner).await;
        }
        for (id, owner) in active {
            let _ =
                tokio::time::timeout(Duration::from_secs(2), self.await_job_owned(id, owner)).await;
        }
    }
}

fn cleanup_job_temp(path: &std::path::Path) {
    if path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with("ca-job-"))
    {
        let _ = std::fs::remove_dir_all(path);
    }
}

impl Default for JobManager {
    fn default() -> Self {
        Self::new()
    }
}

async fn snapshot_record(id: JobId, record: &JobRecord) -> JobInfo {
    let state = record.state.lock().await;
    JobInfo {
        id,
        command: state.command.clone(),
        cwd: state.cwd.clone(),
        status: state.status,
        elapsed: state
            .finished_at
            .unwrap_or_else(Instant::now)
            .duration_since(state.started_at),
        stdout: state.stdout.render("stdout"),
        stderr: state.stderr.render("stderr"),
        stdout_omitted_bytes: state.stdout.omitted_bytes(),
        stderr_omitted_bytes: state.stderr.omitted_bytes(),
        background: state.background,
    }
}

async fn ensure_owner(record: &JobRecord, owner: JobOwner) -> Result<(), String> {
    if record.state.lock().await.owner == owner {
        Ok(())
    } else {
        Err("Job belongs to a different run or session".into())
    }
}

async fn set_terminal_status(inner: &Inner, id: JobId, record: &JobRecord, status: JobStatus) {
    let mut state = record.state.lock().await;
    state.status = status;
    state.finished_at = Some(Instant::now());
    state.keep_awake_lease.take();
    state.workspace_activity_lease.take();
    drop(state);
    record.notify.notify_waiters();
    let _ = inner.events.send(JobEvent::Status { id, status });
}

async fn wait_for_terminal(record: &JobRecord) {
    loop {
        let notified = record.notify.notified();
        if record.state.lock().await.status.is_terminal() {
            return;
        }
        notified.await;
    }
}

async fn terminate_child_gracefully(record: &JobRecord) {
    let mut child = record.child.lock().await;
    if let Some(child) = child.as_mut() {
        #[cfg(unix)]
        if let Some(pid) = child.id() {
            if record.independent_process_group {
                signal_process_group(pid, nix::sys::signal::Signal::SIGTERM);
            } else if let Ok(pid) = i32::try_from(pid) {
                let _ = nix::sys::signal::kill(
                    nix::unistd::Pid::from_raw(pid),
                    nix::sys::signal::Signal::SIGTERM,
                );
            }
        }
        #[cfg(not(unix))]
        let _ = child.start_kill();
    }
}

async fn terminate_child(record: &JobRecord) {
    let mut child = record.child.lock().await;
    if let Some(child) = child.as_mut() {
        #[cfg(unix)]
        if let Some(pid) = child.id().filter(|_| record.independent_process_group) {
            signal_process_group(pid, nix::sys::signal::Signal::SIGKILL);
        }
        let _ = child.start_kill();
    }
}

#[cfg(unix)]
fn signal_process_group(pid: u32, signal: nix::sys::signal::Signal) {
    if let Ok(pid) = i32::try_from(pid) {
        let _ = nix::sys::signal::killpg(nix::unistd::Pid::from_raw(pid), signal);
    }
}

#[derive(Debug)]
struct BoundedText {
    head: Vec<u8>,
    tail: Vec<u8>,
    total_bytes: usize,
    capacity: usize,
}
impl BoundedText {
    fn new(capacity: usize) -> Self {
        Self {
            head: Vec::new(),
            tail: Vec::new(),
            total_bytes: 0,
            capacity,
        }
    }
    fn push(&mut self, bytes: &[u8]) {
        self.total_bytes = self.total_bytes.saturating_add(bytes.len());
        let head_capacity = self.capacity.div_ceil(2);
        let tail_capacity = self.capacity / 2;
        let head_remaining = head_capacity.saturating_sub(self.head.len());
        self.head
            .extend_from_slice(&bytes[..bytes.len().min(head_remaining)]);
        self.tail.extend_from_slice(bytes);
        if self.tail.len() > tail_capacity {
            let remove = self.tail.len() - tail_capacity;
            self.tail.drain(..remove);
        }
    }

    fn omitted_bytes(&self) -> usize {
        self.total_bytes.saturating_sub(self.capacity)
    }

    fn render(&self, label: &str) -> String {
        if self.total_bytes <= self.capacity {
            let suffix_len = self.total_bytes.saturating_sub(self.head.len());
            let mut full = self.head.clone();
            if suffix_len > 0 {
                full.extend_from_slice(&self.tail[self.tail.len() - suffix_len..]);
            }
            return String::from_utf8_lossy(&full).into_owned();
        }
        let omitted = self.omitted_bytes();
        format!(
            "{}\n[… {label}: {omitted} bytes omitted …]\n{}",
            String::from_utf8_lossy(&self.head),
            String::from_utf8_lossy(&self.tail)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn inherited_worker_containment_requires_the_admitted_group() {
        let group = nix::unistd::getpgrp().as_raw();
        assert!(validate_inherited_worker_group(group).is_ok());
        assert!(validate_inherited_worker_group(0).is_err());
        assert!(validate_inherited_worker_group(1).is_err());
        assert!(validate_inherited_worker_group(group.saturating_add(1)).is_err());
    }

    #[tokio::test]
    async fn workspace_restore_blocks_native_command_before_process_spawn() {
        let root = tempfile::tempdir().unwrap();
        let roots = vec![root.path().to_path_buf()];
        let store = crate::recovery::RecoveryStore::new(root.path().join("unused-profile"));
        let restore = store.try_workspace_restore(&roots).unwrap();
        let manager = JobManager::new();
        let result = manager
            .start_sandboxed(
                "printf started > process-started",
                root.path().to_string_lossy(),
                false,
                Duration::from_secs(2),
                roots.clone(),
                false,
                crate::sandbox::SandboxMode::PolicyOnly,
            )
            .await;
        if let Ok(id) = result.as_ref() {
            manager.await_job(*id).await.unwrap();
        }
        assert!(
            result.is_err(),
            "native command escaped active workspace restore"
        );
        assert!(!root.path().join("process-started").exists());
        drop(restore);
        assert!(!root.path().join("unused-profile").exists());
    }

    #[tokio::test]
    async fn native_command_retains_workspace_activity_until_terminal_status() {
        let root = tempfile::tempdir().unwrap();
        let roots = vec![root.path().to_path_buf()];
        let store = crate::recovery::RecoveryStore::new(root.path().join("unused-profile"));
        let manager = JobManager::new();
        let id = manager
            .start_sandboxed(
                "while [ ! -e release-command ]; do sleep 0.02; done",
                root.path().to_string_lossy(),
                true,
                Duration::from_secs(2),
                roots.clone(),
                false,
                crate::sandbox::SandboxMode::PolicyOnly,
            )
            .await
            .unwrap();
        let restore = store.try_workspace_restore(&roots);
        std::fs::write(root.path().join("release-command"), "release").unwrap();
        let result = tokio::time::timeout(Duration::from_secs(2), manager.await_job(id))
            .await
            .unwrap()
            .unwrap();
        assert!(result.status.is_terminal());
        assert!(
            restore.is_err(),
            "live native command released workspace ownership"
        );
        assert!(store.try_workspace_restore(&roots).is_ok());
        assert!(!root.path().join("unused-profile").exists());
    }

    fn experiment_policy(root: &std::path::Path) -> JobExecutionPolicy {
        let home = root.join("home");
        let cargo_home = root.join("cargo-home");
        let cargo_target = root.join("cargo-target");
        for path in [&home, &cargo_home, &cargo_target] {
            std::fs::create_dir_all(path).unwrap();
        }
        JobExecutionPolicy::experiment_clean(home, cargo_home, cargo_target).unwrap()
    }

    #[tokio::test]
    async fn experiment_environment_is_closed_and_redacted() {
        let root = tempfile::tempdir().unwrap();
        let policy = experiment_policy(root.path());
        assert_eq!(
            format!("{policy:?}"),
            "JobExecutionPolicy { environment: \"experiment_clean\", hide_other_processes: true, authorized_command: None, effective_network_allowed: false }"
        );

        let mut command = Command::new("/usr/bin/env");
        command.env("RND_FAKE_SECRET", "must-not-survive");
        policy.apply(&mut command, root.path());
        let output = command.output().await.unwrap();
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(output.status.success(), "{stdout}");
        assert!(!stdout.contains("RND_FAKE_SECRET"));
        assert!(!stdout.contains("must-not-survive"));
        for expected in [
            "CARGO_NET_OFFLINE=true",
            "CARGO_BUILD_JOBS=1",
            "GIT_CONFIG_NOSYSTEM=1",
            "GIT_CONFIG_GLOBAL=/dev/null",
            "LC_ALL=C",
            "LANG=C",
            "TZ=UTC",
        ] {
            assert!(stdout.lines().any(|line| line == expected), "{stdout}");
        }
    }

    #[test]
    fn authorized_command_analysis_finds_only_literal_command_positions() {
        assert!(command_has_literal_program(
            "cargo fmt --all --check && cargo build",
            "cargo"
        ));
        assert!(command_has_literal_program(
            "FOO=bar /opt/homebrew/bin/cargo build",
            "cargo"
        ));
        assert!(!command_has_literal_program("printf cargo", "cargo"));
        assert!(!command_has_literal_program("$TOOL build", "cargo"));
        assert_eq!(
            literal_absolute_programs("/definitely/missing/tool arg"),
            Vec::<std::path::PathBuf>::new()
        );
    }

    #[tokio::test]
    async fn ordinary_cargo_policy_is_offline_and_uses_read_only_registry_only() {
        let policy = JobExecutionPolicy::default()
            .for_authorized_command("cargo fmt --all --check && cargo build", false);
        let mut writable = Vec::new();
        let mut read_only = Vec::new();
        policy.extend_sandbox_roots(&mut writable, &mut read_only);
        assert_eq!(writable, [std::path::PathBuf::from("/tmp")]);
        assert!(read_only.contains(&std::path::PathBuf::from("/")));
        if let Some(registry) = cargo_registry_root() {
            assert!(read_only.contains(&registry));
            assert!(!read_only.iter().any(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("credentials") || name == "config.toml")
            }));
        }

        let root = tempfile::tempdir().unwrap();
        let mut command = Command::new("/usr/bin/env");
        policy.apply(&mut command, root.path());
        let output = command.output().await.unwrap();
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(stdout.lines().any(|line| line == "CARGO_NET_OFFLINE=true"));
        for variable in ["TMPDIR", "TMP", "TEMP"] {
            assert!(
                stdout
                    .lines()
                    .any(|line| line == format!("{variable}={}", root.path().display())),
                "{variable} did not use the private job directory"
            );
        }
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn ordinary_sandbox_reads_host_files_and_allows_only_workspace_or_temp_writes() {
        let workspace = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let conventional_temp = tempfile::Builder::new()
            .prefix("ca-sandbox-test-")
            .tempdir_in("/tmp")
            .unwrap();
        let source = outside.path().join("source.txt");
        let blocked = outside.path().join("blocked.txt");
        let copied = workspace.path().join("copied.txt");
        let temporary_output = conventional_temp.path().join("temporary.txt");
        let protected =
            std::path::PathBuf::from(format!("/tmp/ca-privilege-test-{}", Uuid::new_v4()));
        std::fs::write(&source, "readable outside the workspace").unwrap();
        for path in [&source, &blocked, &copied, &temporary_output, &protected] {
            assert!(!path.to_string_lossy().contains('\''));
        }
        let command = format!(
            "/bin/cat '{}' > '{}'; /usr/bin/touch '{}' 2>/dev/null || true; /usr/bin/touch '{}'; /bin/mkdir '{}' 2>/dev/null || true",
            source.display(),
            copied.display(),
            blocked.display(),
            temporary_output.display(),
            protected.display()
        );
        let manager = JobManager::new();
        let owner = JobOwner::new(None);
        let id = manager
            .start_sandboxed_owned_with_policy(
                command,
                workspace.path().to_string_lossy(),
                false,
                Duration::from_secs(3),
                vec![std::fs::canonicalize(workspace.path()).unwrap()],
                false,
                crate::sandbox::SandboxMode::Required,
                owner,
                JobExecutionPolicy::default(),
            )
            .await
            .unwrap();
        let job = manager.await_job_owned(id, owner).await.unwrap();
        assert!(matches!(job.status, JobStatus::Completed { exit_code: 0 }));
        assert_eq!(
            std::fs::read_to_string(copied).unwrap(),
            "readable outside the workspace"
        );
        assert!(!blocked.exists());
        assert!(temporary_output.exists());
        assert!(!protected.exists());
        manager.shutdown().await;
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn experiment_sandbox_runs_offline_stdlib_cargo_test() {
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("workspace");
        std::fs::create_dir_all(workspace.join("src")).unwrap();
        std::fs::write(
            workspace.join("Cargo.toml"),
            "[package]\nname = \"sandbox_fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        std::fs::write(
            workspace.join("src/lib.rs"),
            "pub fn answer() -> u8 { 42 }\n#[test]\nfn works() { assert_eq!(answer(), 42); }\n",
        )
        .unwrap();
        let policy = experiment_policy(root.path());
        let manager = JobManager::new();
        let owner = JobOwner::new(None);
        let cargo = crate::run_environment::var_os("CARGO")
            .map(std::path::PathBuf::from)
            .filter(|path| path.is_absolute())
            .unwrap_or_else(|| std::path::PathBuf::from("/opt/homebrew/bin/cargo"));
        assert!(!cargo.to_string_lossy().contains('\''));
        let command = format!(
            "'{}' test --offline --quiet; code=$?; echo CA_EXPERIMENT_EXIT:$code >&2; exit $code",
            cargo.to_string_lossy()
        );
        let id = manager
            .start_sandboxed_owned_with_policy(
                command,
                workspace.to_string_lossy(),
                false,
                Duration::from_secs(60),
                vec![workspace.clone()],
                false,
                crate::sandbox::SandboxMode::Required,
                owner,
                policy,
            )
            .await
            .unwrap();
        let job = manager.await_job_owned(id, owner).await.unwrap();
        assert_eq!(
            job.status,
            JobStatus::Completed { exit_code: 0 },
            "stdout:\n{}\nstderr:\n{}",
            job.stdout,
            job.stderr
        );
        manager.shutdown().await;
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn ordinary_required_sandbox_resolves_the_invoking_user() {
        let root = tempfile::tempdir().unwrap();
        let manager = JobManager::new();
        let owner = JobOwner::new(None);
        let id = manager
            .start_sandboxed_owned(
                "/usr/bin/id -un",
                root.path().to_string_lossy(),
                false,
                Duration::from_secs(5),
                vec![root.path().to_path_buf()],
                false,
                crate::sandbox::SandboxMode::Required,
                owner,
            )
            .await
            .unwrap();
        let job = manager.await_job_owned(id, owner).await.unwrap();
        assert_eq!(
            job.status,
            JobStatus::Completed { exit_code: 0 },
            "stdout:\n{}\nstderr:\n{}",
            job.stdout,
            job.stderr
        );
        let user = job.stdout.trim();
        assert!(!user.is_empty());
        assert!(
            !user.bytes().all(|byte| byte.is_ascii_digit()),
            "sandbox returned numeric uid instead of account name: {user}"
        );
        manager.shutdown().await;
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn ordinary_required_sandbox_supports_git_and_approved_cached_cargo() {
        if std::process::Command::new("cargo")
            .arg("--version")
            .output()
            .is_err()
        {
            return;
        }
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("workspace");
        std::fs::create_dir_all(workspace.join("src")).unwrap();
        std::fs::write(
            workspace.join("Cargo.toml"),
            "[package]\nname = \"ordinary_sandbox_fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nitoa = \"=1.0.18\"\n",
        )
        .unwrap();
        std::fs::write(
            workspace.join("src/lib.rs"),
            "pub fn answer() -> String {\n    itoa::Buffer::new().format(42).to_owned()\n}\n",
        )
        .unwrap();
        let initialized = std::process::Command::new("/usr/bin/git")
            .args(["init", "--quiet"])
            .current_dir(&workspace)
            .status()
            .unwrap();
        assert!(initialized.success());

        let manager = JobManager::new();
        let owner = JobOwner::new(None);
        for command in [
            "git status --short && git diff -- src/lib.rs",
            "cargo fmt --all --check && cargo build",
        ] {
            let policy = JobExecutionPolicy::default().for_authorized_command(command, false);
            let id = manager
                .start_sandboxed_owned_with_policy(
                    command,
                    workspace.to_string_lossy(),
                    false,
                    Duration::from_secs(60),
                    vec![workspace.clone()],
                    false,
                    crate::sandbox::SandboxMode::Required,
                    owner,
                    policy,
                )
                .await
                .unwrap();
            let job = manager.await_job_owned(id, owner).await.unwrap();
            assert_eq!(
                job.status,
                JobStatus::Completed { exit_code: 0 },
                "command: {command}\nstdout:\n{}\nstderr:\n{}",
                job.stdout,
                job.stderr
            );
            assert!(!job.stderr.contains("/etc/gitconfig"));
            assert!(!job.stderr.contains("command not found"));
        }
        manager.shutdown().await;
    }

    #[tokio::test]
    async fn streams_stdout_and_stderr_and_completes() {
        let manager = JobManager::new();
        let mut events = manager.subscribe();
        let id = manager
            .start(
                "printf out; printf err >&2",
                "/tmp",
                false,
                Duration::from_secs(2),
            )
            .await
            .unwrap();
        let job = manager.await_job(id).await.unwrap();
        assert_eq!(job.stdout, "out");
        assert_eq!(job.stderr, "err");
        assert_eq!(job.status, JobStatus::Completed { exit_code: 0 });
        let mut saw_output = false;
        while let Ok(event) = events.try_recv() {
            saw_output |= matches!(event, JobEvent::Output { id: event_id, .. } if event_id == id);
        }
        assert!(saw_output);
    }

    #[tokio::test]
    async fn admitted_command_output_limit_bounds_physical_capture() {
        let temporary = tempfile::tempdir().unwrap();
        let workspace = std::fs::canonicalize(temporary.path()).unwrap();
        let manager = JobManager::new();
        let owner = JobOwner::new(None);
        for (command, produced_bytes) in [
            ("printf '%04096d' 0; printf '%04096d' 0 >&2", 8192),
            ("i=0; while [ \"$i\" -lt 1024 ]; do printf '\\377'; printf '\\377' >&2; i=$((i+1)); done", 2048),
        ] {
        let id = manager
            .start_sandboxed_owned_with_policy_and_origin(
                command,
                workspace.to_string_lossy(),
                false,
                Duration::from_secs(2),
                vec![workspace.clone()],
                false,
                crate::sandbox::SandboxMode::PolicyOnly,
                owner,
                JobExecutionPolicy::default().with_output_limit(1024),
                JobOrigin::ModelTool,
            )
            .await
            .unwrap();
        let job = manager.await_job_owned(id, owner).await.unwrap();
        assert_eq!(job.status, JobStatus::Completed { exit_code: 0 });
        // Rendering adds omission labels; the raw capture occupies at most
        // half the admitted ceiling, leaving room for those labels and framing.
        assert!(job.stdout_omitted_bytes + job.stderr_omitted_bytes >= produced_bytes - 512);
        assert!(job.stdout.len() + job.stderr.len() <= 1024);
        assert!(job.stdout_omitted_bytes > 0 && job.stderr_omitted_bytes > 0);
        assert!(job.formatted_output().len() <= 1024);
        }
        manager.shutdown().await;
    }

    #[tokio::test]
    async fn stdin_reaches_a_running_job() {
        let manager = JobManager::new();
        let id = manager
            .start(
                "read line; printf '%s' \"$line\"",
                "/tmp",
                false,
                Duration::from_secs(2),
            )
            .await
            .unwrap();
        manager.write_stdin(id, "hello\n").await.unwrap();
        assert_eq!(manager.await_job(id).await.unwrap().stdout, "hello");
    }

    #[tokio::test]
    async fn cancellation_reaps_the_job() {
        let manager = JobManager::new();
        let id = manager
            .start("sleep 10", "/tmp", true, Duration::from_secs(20))
            .await
            .unwrap();
        manager.kill(id).await.unwrap();
        let job = tokio::time::timeout(Duration::from_secs(2), manager.await_job(id))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(job.status, JobStatus::Cancelled);
    }

    #[tokio::test]
    async fn interrupting_foreground_waiter_stops_its_command() {
        let manager = JobManager::new();
        let id = manager
            .start("sleep 30 & wait", "/tmp", false, Duration::from_secs(40))
            .await
            .unwrap();
        let waiting_manager = manager.clone();
        let waiter = tokio::spawn(async move {
            waiting_manager
                .await_foreground_job_owned(id, JobOwner::process())
                .await
        });
        tokio::task::yield_now().await;
        waiter.abort();
        assert!(waiter.await.unwrap_err().is_cancelled());
        let stopped = tokio::time::timeout(Duration::from_secs(2), manager.await_job(id)).await;
        // Clean up even on the expected pre-fix failure.
        if stopped.is_err() {
            manager.kill(id).await.unwrap();
            manager.await_job(id).await.unwrap();
        }
        assert_eq!(stopped.unwrap().unwrap().status, JobStatus::Cancelled);
    }

    #[tokio::test]
    async fn interrupting_waiter_leaves_promoted_and_other_owner_jobs_running() {
        let manager = JobManager::new();
        let owner = JobOwner::process();
        let id = manager
            .start_sandboxed_owned_with_origin(
                "sleep 30",
                "/tmp",
                false,
                Duration::from_secs(40),
                Vec::new(),
                false,
                crate::sandbox::SandboxMode::PolicyOnly,
                owner,
                JobOrigin::ModelTool,
            )
            .await
            .unwrap();
        let waiting_manager = manager.clone();
        let waiter =
            tokio::spawn(
                async move { waiting_manager.await_foreground_job_owned(id, owner).await },
            );
        tokio::task::yield_now().await;
        manager.promote_owned(id, owner).await.unwrap();
        waiter.abort();
        let _ = waiter.await;
        tokio::task::yield_now().await;
        assert_eq!(
            manager.get_owned(id, owner).await.unwrap().status,
            JobStatus::Running
        );
        assert!(manager
            .await_foreground_job_owned(id, JobOwner::new(None))
            .await
            .is_err());
        tokio::task::yield_now().await;
        assert_eq!(
            manager.get_owned(id, owner).await.unwrap().status,
            JobStatus::Running
        );
        manager.kill_owned(id, owner).await.unwrap();
        manager.await_job(id).await.unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn graceful_stop_waits_for_term_handler_and_preserves_final_output_once() {
        let manager = JobManager::new();
        let owner = JobOwner::new(None);
        let id = manager
            .start_sandboxed_owned_with_origin(
                "trap 'printf graceful; exit 0' TERM; while :; do sleep 1; done",
                "/tmp",
                true,
                Duration::from_secs(20),
                vec![std::fs::canonicalize("/tmp").unwrap()],
                false,
                crate::sandbox::SandboxMode::PolicyOnly,
                owner,
                JobOrigin::ModelTool,
            )
            .await
            .unwrap();
        manager
            .reserve_completion_await_owned(id, owner)
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(40)).await;
        let stopped = manager
            .stop_and_await_owned(id, owner, Duration::from_secs(2))
            .await
            .unwrap();
        assert_eq!(stopped.status, JobStatus::Cancelled);
        assert!(stopped.stdout.contains("graceful"), "{stopped:?}");
        let delivered = manager
            .finish_completion_await_owned(id, owner, false)
            .await
            .unwrap();
        assert_eq!(delivered.stdout, stopped.stdout);
        assert!(manager
            .claim_completed_model_jobs_owned(owner)
            .await
            .is_empty());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn graceful_stop_escalates_after_the_exact_grace_and_enforces_ownership() {
        let manager = JobManager::new();
        let owner = JobOwner::new(None);
        let other = JobOwner::new(None);
        let id = manager
            .start_sandboxed_owned_with_origin(
                "trap '' TERM; while :; do sleep 1; done",
                "/tmp",
                true,
                Duration::from_secs(20),
                vec![std::fs::canonicalize("/tmp").unwrap()],
                false,
                crate::sandbox::SandboxMode::PolicyOnly,
                owner,
                JobOrigin::ModelTool,
            )
            .await
            .unwrap();
        assert!(manager
            .stop_and_await_owned(id, other, Duration::ZERO)
            .await
            .unwrap_err()
            .contains("different run"));
        let started = Instant::now();
        let stopped = manager
            .stop_and_await_owned(id, owner, Duration::from_millis(30))
            .await
            .unwrap();
        assert_eq!(stopped.status, JobStatus::Cancelled);
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(manager
            .stop_and_await_owned(id, owner, Duration::from_secs(301))
            .await
            .unwrap_err()
            .contains("0 and 300"));
    }

    #[tokio::test]
    async fn timeout_terminates_the_job() {
        let manager = JobManager::new();
        let id = manager
            .start("sleep 10", "/tmp", false, Duration::from_millis(30))
            .await
            .unwrap();
        let job = tokio::time::timeout(Duration::from_secs(2), manager.await_job(id))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(job.status, JobStatus::TimedOut);
    }

    #[tokio::test]
    async fn explicit_background_jobs_do_not_inherit_the_foreground_timeout() {
        let manager = JobManager::new();
        let id = manager
            .start(
                "sleep 0.08; printf done",
                "/tmp",
                true,
                Duration::from_millis(20),
            )
            .await
            .unwrap();
        let job = tokio::time::timeout(Duration::from_secs(2), manager.await_job(id))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(job.status, JobStatus::Completed { exit_code: 0 });
        assert_eq!(job.stdout, "done");
    }

    #[tokio::test]
    async fn promotion_wakes_foreground_waiter_and_removes_timeout_without_restart() {
        let manager = JobManager::new();
        let owner = JobOwner::new(None);
        let id = manager
            .start_sandboxed_owned_with_origin(
                "printf before; sleep 0.08; printf after",
                "/tmp",
                false,
                Duration::from_millis(35),
                vec![std::fs::canonicalize("/tmp").unwrap()],
                false,
                crate::sandbox::SandboxMode::PolicyOnly,
                owner,
                JobOrigin::ModelTool,
            )
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(15)).await;
        let promoted = manager.promote_owned(id, owner).await.unwrap();
        assert!(promoted.background);
        assert!(matches!(
            manager.await_foreground_job_owned(id, owner).await.unwrap(),
            ForegroundJobOutcome::Promoted(_)
        ));
        let finished =
            tokio::time::timeout(Duration::from_secs(2), manager.await_job_owned(id, owner))
                .await
                .unwrap()
                .unwrap();
        assert_eq!(finished.status, JobStatus::Completed { exit_code: 0 });
        assert_eq!(finished.stdout, "beforeafter");
    }

    #[tokio::test]
    async fn internal_jobs_cannot_be_promoted() {
        let manager = JobManager::new();
        let owner = JobOwner::new(None);
        let id = manager
            .start_sandboxed_owned(
                "sleep 1",
                "/tmp",
                false,
                Duration::from_secs(2),
                vec![std::fs::canonicalize("/tmp").unwrap()],
                false,
                crate::sandbox::SandboxMode::PolicyOnly,
                owner,
            )
            .await
            .unwrap();
        assert!(manager.promote_owned(id, owner).await.is_err());
        manager.kill_owned(id, owner).await.unwrap();
    }

    #[tokio::test]
    async fn promotion_loses_safely_after_timeout_has_started() {
        let manager = JobManager::new();
        let owner = JobOwner::new(None);
        let id = manager
            .start_sandboxed_owned_with_origin(
                "sleep 10",
                "/tmp",
                false,
                Duration::from_millis(5),
                vec![std::fs::canonicalize("/tmp").unwrap()],
                false,
                crate::sandbox::SandboxMode::PolicyOnly,
                owner,
                JobOrigin::DirectShell,
            )
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert!(manager.promote_owned(id, owner).await.is_err());
        let finished = manager.await_job_owned(id, owner).await.unwrap();
        assert_eq!(finished.status, JobStatus::TimedOut);
    }

    #[tokio::test]
    async fn automatic_completion_claims_only_model_background_jobs_once() {
        let manager = JobManager::new();
        let owner = JobOwner::new(None);
        let mut ids = Vec::new();
        for origin in [JobOrigin::ModelTool, JobOrigin::DirectShell] {
            ids.push(
                manager
                    .start_sandboxed_owned_with_origin(
                        "printf done",
                        "/tmp",
                        true,
                        Duration::from_secs(2),
                        vec![std::fs::canonicalize("/tmp").unwrap()],
                        false,
                        crate::sandbox::SandboxMode::PolicyOnly,
                        owner,
                        origin,
                    )
                    .await
                    .unwrap(),
            );
        }
        for id in &ids {
            manager.await_job_owned(*id, owner).await.unwrap();
        }
        let claimed = manager.claim_completed_model_jobs_owned(owner).await;
        assert_eq!(claimed.len(), 1);
        assert_eq!(claimed[0].id, ids[0]);
        assert!(manager
            .claim_completed_model_jobs_owned(owner)
            .await
            .is_empty());
    }

    #[tokio::test]
    async fn explicit_await_reservation_prevents_automatic_completion_claim() {
        let manager = JobManager::new();
        let owner = JobOwner::new(None);
        let id = manager
            .start_sandboxed_owned_with_origin(
                "sleep 0.03; printf done",
                "/tmp",
                true,
                Duration::from_secs(2),
                vec![std::fs::canonicalize("/tmp").unwrap()],
                false,
                crate::sandbox::SandboxMode::PolicyOnly,
                owner,
                JobOrigin::ModelTool,
            )
            .await
            .unwrap();
        manager
            .reserve_completion_await_owned(id, owner)
            .await
            .unwrap();
        manager.await_job_owned(id, owner).await.unwrap();
        assert!(manager
            .claim_completed_model_jobs_owned(owner)
            .await
            .is_empty());
        let delivered = manager
            .finish_completion_await_owned(id, owner, false)
            .await
            .unwrap();
        assert_eq!(delivered.stdout, "done");
        assert!(manager
            .claim_completed_model_jobs_owned(owner)
            .await
            .is_empty());
    }

    #[tokio::test]
    async fn output_buffers_keep_bounded_head_and_tail_with_exact_omission() {
        let manager = JobManager::with_buffer_bytes(8);
        let id = manager
            .start("printf 1234567890", "/tmp", false, Duration::from_secs(2))
            .await
            .unwrap();
        let job = manager.await_job(id).await.unwrap();
        assert_eq!(job.stdout, "1234\n[… stdout: 2 bytes omitted …]\n7890");
        assert_eq!(job.stdout_omitted_bytes, 2);
        assert_eq!(job.stderr_omitted_bytes, 0);
    }

    #[tokio::test]
    async fn stdout_and_stderr_have_independent_head_tail_bounds() {
        let manager = JobManager::with_buffer_bytes(8);
        let id = manager
            .start(
                "printf 1234567890; printf abcdefghij >&2",
                "/tmp",
                false,
                Duration::from_secs(2),
            )
            .await
            .unwrap();
        let job = manager.await_job(id).await.unwrap();
        assert!(job
            .stdout
            .starts_with("1234\n[… stdout: 2 bytes omitted …]"));
        assert!(job.stdout.ends_with("7890"));
        assert!(job
            .stderr
            .starts_with("abcd\n[… stderr: 2 bytes omitted …]"));
        assert!(job.stderr.ends_with("ghij"));
        assert_eq!(job.stdout_omitted_bytes, 2);
        assert_eq!(job.stderr_omitted_bytes, 2);
    }

    #[test]
    fn model_job_output_shares_budget_across_both_streams() {
        let info = JobInfo {
            id: JobId::new(),
            command: "large".into(),
            cwd: "/tmp".into(),
            status: JobStatus::Failed { exit_code: Some(1) },
            elapsed: Duration::ZERO,
            stdout: format!("OUT-HEAD{}OUT-TAIL", "o".repeat(8_000)),
            stderr: format!("ERR-HEAD{}ERR-TAIL", "e".repeat(8_000)),
            stdout_omitted_bytes: 0,
            stderr_omitted_bytes: 0,
            background: false,
        };
        let output = info.formatted_output();
        assert!(output.len() <= 4 * 1024);
        assert!(output.contains("stdout:"));
        assert!(output.contains("stderr:"));
        assert!(output.contains("OUT-HEAD"));
        assert!(output.contains("OUT-TAIL"));
        assert!(output.contains("ERR-HEAD"));
        assert!(output.contains("ERR-TAIL"));
        assert!(output.contains("stdout:") && output.contains("bytes omitted"));
    }

    #[tokio::test]
    async fn shutdown_cancels_all_active_jobs() {
        let manager = JobManager::new();
        let id = manager
            .start("sleep 10", "/tmp", true, Duration::from_secs(20))
            .await
            .unwrap();
        manager.shutdown().await;
        assert_eq!(manager.get(id).await.unwrap().status, JobStatus::Cancelled);
    }

    #[tokio::test]
    async fn starting_a_foreground_job_does_not_block_event_progress() {
        let workspace = tempfile::tempdir().unwrap();
        let manager = JobManager::new();
        let mut events = manager.subscribe();
        // The child cannot finish until this test releases its stdin gate.
        // Dispatch includes blocking-pool admission and workspace ownership,
        // so wall-clock startup latency does not establish event-loop progress.
        let (id, heartbeat) = tokio::time::timeout(Duration::from_secs(2), async {
            tokio::join!(
                manager.start(
                    "printf READY; IFS= read -r gate",
                    workspace.path().to_string_lossy(),
                    false,
                    Duration::from_secs(2),
                ),
                async {
                    tokio::task::yield_now().await;
                    true
                },
            )
        })
        .await
        .expect("event work and foreground dispatch must progress before releasing the child");
        assert!(heartbeat);
        let id = id.unwrap();
        assert_eq!(manager.get(id).await.unwrap().status, JobStatus::Running);
        tokio::time::timeout(Duration::from_secs(2), async {
            let mut ready = String::new();
            loop {
                if let JobEvent::Output { stream, text, .. } = events.recv().await.unwrap() {
                    if stream == JobStream::Stdout {
                        ready.push_str(&text);
                        if ready.contains("READY") {
                            break;
                        }
                    }
                }
            }
        })
        .await
        .expect("foreground output should arrive while the stdin gate is closed");
        assert_eq!(manager.get(id).await.unwrap().status, JobStatus::Running);
        manager.write_stdin(id, "release\n").await.unwrap();
        let completed = manager.await_job(id).await.unwrap();
        assert_eq!(completed.status, JobStatus::Completed { exit_code: 0 });
    }

    #[tokio::test]
    async fn job_control_is_scoped_to_run_and_session_owner() {
        let manager = JobManager::new();
        let owner = JobOwner::new(Some(Uuid::new_v4()));
        let other = JobOwner::new(owner.session_id);
        let id = manager
            .start_sandboxed_owned(
                "sleep 10",
                "/tmp",
                true,
                Duration::from_secs(20),
                vec![std::fs::canonicalize("/tmp").unwrap()],
                false,
                crate::sandbox::SandboxMode::PolicyOnly,
                owner,
            )
            .await
            .unwrap();
        assert!(manager.await_job_owned(id, other).await.is_err());
        assert!(manager.kill_owned(id, other).await.is_err());
        manager.kill_owned(id, owner).await.unwrap();
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn macos_sandbox_allows_workspace_write_and_denies_outside_write() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let root_path = std::fs::canonicalize(root.path()).unwrap();
        let outside_target = outside.path().join("blocked.txt");
        let command = format!(
            "printf allowed > '{}'; printf blocked > '{}'",
            root.path().join("allowed.txt").display(),
            outside_target.display()
        );
        let manager = JobManager::new();
        let id = manager
            .start_sandboxed(
                command,
                root.path().to_string_lossy(),
                false,
                Duration::from_secs(3),
                vec![root_path],
                false,
                crate::sandbox::SandboxMode::Required,
            )
            .await
            .unwrap();
        let job = manager.await_job(id).await.unwrap();
        assert!(root.path().join("allowed.txt").is_file(), "{job:?}");
        assert!(!outside_target.exists(), "{job:?}");
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn macos_sandbox_denies_loopback_unless_network_is_granted() {
        let root = tempfile::tempdir().unwrap();
        let root_path = std::fs::canonicalize(root.path()).unwrap();
        let manager = JobManager::new();

        let denied_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let denied_port = denied_listener.local_addr().unwrap().port();
        let denied_id = manager
            .start_sandboxed(
                format!("printf ping | /usr/bin/nc -w 1 127.0.0.1 {denied_port}"),
                root.path().to_string_lossy(),
                false,
                Duration::from_secs(3),
                vec![root_path.clone()],
                false,
                crate::sandbox::SandboxMode::Required,
            )
            .await
            .unwrap();
        let denied = manager.await_job(denied_id).await.unwrap();
        assert!(!matches!(
            denied.status,
            JobStatus::Completed { exit_code: 0 }
        ));
        assert!(
            tokio::time::timeout(Duration::from_millis(100), denied_listener.accept())
                .await
                .is_err()
        );

        let allowed_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let allowed_port = allowed_listener.local_addr().unwrap().port();
        let allowed_id = manager
            .start_sandboxed(
                format!("printf ping | /usr/bin/nc -w 1 127.0.0.1 {allowed_port}"),
                root.path().to_string_lossy(),
                false,
                Duration::from_secs(3),
                vec![root_path],
                true,
                crate::sandbox::SandboxMode::Required,
            )
            .await
            .unwrap();
        let accepted = tokio::time::timeout(Duration::from_secs(2), allowed_listener.accept())
            .await
            .expect("network grant should reach loopback")
            .unwrap();
        drop(accepted);
        let allowed = manager.await_job(allowed_id).await.unwrap();
        assert!(matches!(
            allowed.status,
            JobStatus::Completed { exit_code: 0 }
        ));
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn experiment_sandbox_denies_parent_process_environment_inspection() {
        let root = tempfile::tempdir().unwrap();
        let root_path = std::fs::canonicalize(root.path()).unwrap();
        let mut secret_parent = tokio::process::Command::new("/bin/sleep")
            .arg("10")
            .env_clear()
            .env("RND_FAKE_SECRET", "parent-only-sentinel")
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let secret_pid = secret_parent.id().unwrap();
        let manager = JobManager::new();
        let owner = JobOwner::new(None);
        let id = manager
            .start_sandboxed_owned_with_policy(
                format!("/bin/ps e -p {secret_pid}"),
                root.path().to_string_lossy(),
                false,
                Duration::from_secs(3),
                vec![root_path],
                false,
                crate::sandbox::SandboxMode::Required,
                owner,
                experiment_policy(root.path()),
            )
            .await
            .unwrap();
        let job = manager.await_job_owned(id, owner).await.unwrap();
        assert!(!job.stdout.contains("parent-only-sentinel"), "{job:?}");
        assert!(!job.stderr.contains("parent-only-sentinel"), "{job:?}");
        assert!(!matches!(job.status, JobStatus::Completed { exit_code: 0 }));
        let _ = secret_parent.start_kill();
        let _ = secret_parent.wait().await;
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn experiment_sandbox_denies_host_home_read() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let sentinel = outside.path().join("host-home-sentinel.txt");
        std::fs::write(&sentinel, "host-home-only-sentinel").unwrap();
        let root_path = std::fs::canonicalize(root.path()).unwrap();
        let manager = JobManager::new();
        let owner = JobOwner::new(None);
        let id = manager
            .start_sandboxed_owned_with_policy(
                format!("/bin/cat '{}'", sentinel.display()),
                root.path().to_string_lossy(),
                false,
                Duration::from_secs(3),
                vec![root_path],
                false,
                crate::sandbox::SandboxMode::Required,
                owner,
                experiment_policy(root.path()),
            )
            .await
            .unwrap();
        let job = manager.await_job_owned(id, owner).await.unwrap();
        assert!(!job.stdout.contains("host-home-only-sentinel"), "{job:?}");
        assert!(!job.stderr.contains("host-home-only-sentinel"), "{job:?}");
        assert!(!matches!(job.status, JobStatus::Completed { exit_code: 0 }));
        manager.shutdown().await;
    }
}
