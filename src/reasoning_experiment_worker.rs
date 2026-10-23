//! Isolated RND-097 provider and tool-worker boundaries.
//!
//! The feature-gated broker consumes exactly one framed provider-call spec from
//! standard input, opens the owner-only ledger rooted at its private current
//! directory, and performs at most one Responses POST. It exposes no tool
//! execution API. A separate exact-argv tool worker has no configuration or
//! provider surface. Request and response bodies exist only in bounded IPC
//! frames and transient process memory; the ledger receives their digests only.

use crate::agent::client::{
    AgentClient, OpenAiResponsesRequestOptions, OpenAiServiceTier, ProviderCallPolicy,
};
use crate::agent::stream::{consume_stream, AssembledAssistantTurn, UsageInfo};
use crate::agent::ProviderProtocol;
use crate::config::ConfigStore;
#[cfg(test)]
use crate::config::{ProviderConfig, ProviderTimeoutConfig};
use crate::reasoning_experiment_ledger::ExperimentLedger;
use crate::reasoning_experiment_protocol::{
    read_ipc_frame, write_ipc_frame, ExperimentUsageV1, ProviderCallOutcomeV1,
    ProviderCallReceiptV1, ProviderCallSpecV1, ToolCallOutcomeV1, ToolCallReceiptV1,
    ToolCallSpecV1, MAX_PROVIDER_CALL_RECEIPT_FRAME_BYTES, MAX_PROVIDER_CALL_SPEC_FRAME_BYTES,
    MAX_TOOL_CALL_RECEIPT_FRAME_BYTES, MAX_TOOL_CALL_SPEC_FRAME_BYTES, MAX_TOOL_OUTPUT_BYTES,
    PROVIDER_CALL_RECEIPT_SCHEMA_V1, TOOL_CALL_RECEIPT_SCHEMA_V1,
};
use crate::runtime::{ActiveProfile, RuntimePaths};
use crate::sandbox::{SandboxBackend, SandboxMode, SandboxPolicy};
use crate::usage::usage_details_valid;
use chrono::{DateTime, Utc};
use codex_team::self_improvement::{
    daily_learning_budget_root, reserve_daily_learning, settle_daily_learning, DailyLearningTicket,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const LEDGER_DIRECTORY_NAME: &str = "ledger";
const MAX_EXACT_TOOL_ARGUMENTS: usize = 128;
const MAX_EXACT_TOOL_ARGUMENT_BYTES: usize = 8 * 1024;
const MAX_EXACT_TOOL_ARGUMENT_TOTAL_BYTES: usize = 64 * 1024;
const MAX_TOOL_CAPTURE_PER_STREAM_BYTES: usize = MAX_TOOL_OUTPUT_BYTES / 3;
const MAX_TYPED_FILE_BYTES: usize = 256 * 1024;
const TOOLCHAIN_ROOTS: [&str; 3] = ["/opt/homebrew", "/usr/local", "/usr"];
const ENVIRONMENT_PROBE_EXECUTABLE: &str = "/usr/bin/env";
const CANONICAL_OPENAI_BASE_URL: &str = "https://api.openai.com/v1";

#[derive(Debug, thiserror::Error)]
pub enum BrokerError {
    #[error("provider-call frame is invalid")]
    Frame,
    #[error("experiment ledger is unavailable")]
    Ledger,
    #[error("fresh official experiment pricing is unavailable")]
    PriceEvidence,
    #[error("shared daily learning allowance is unavailable or exhausted")]
    DailyBudget,
    #[error("canonical OpenAI Responses route is unavailable")]
    Route,
    #[error("broker call could not be completed")]
    Execution,
}

/// Runs the feature-only internal broker. It intentionally accepts no command
/// arguments: the current directory is a controller-owned private experiment
/// directory and the one framed spec is the sole call authority.
pub async fn run_provider_broker_stdio() -> i32 {
    let mut stdin = std::io::stdin().lock();
    let call = match read_ipc_frame::<_, ProviderCallSpecV1>(
        &mut stdin,
        MAX_PROVIDER_CALL_SPEC_FRAME_BYTES,
    ) {
        Ok(call) => call,
        Err(_) => return 2,
    };
    let current_dir = match std::env::current_dir() {
        Ok(path) => path,
        Err(_) => return 3,
    };
    let ledger = match ExperimentLedger::open(&current_dir.join(LEDGER_DIRECTORY_NAME)) {
        Ok(ledger) => ledger,
        Err(_) => return 3,
    };
    let client = match canonical_openai_client(&call) {
        Ok(client) => client,
        Err(_) => return 3,
    };
    let paths = match RuntimePaths::discover() {
        Ok(paths) => paths,
        Err(_) => return 3,
    };
    let daily_root = daily_learning_budget_root(&paths.home);
    let receipt =
        match execute_provider_call_with_client(&call, &ledger, &client, &daily_root, Utc::now)
            .await
        {
            Ok(receipt) => receipt,
            Err(_) => return 4,
        };
    let mut stdout = std::io::stdout().lock();
    match write_ipc_frame(&mut stdout, &receipt, MAX_PROVIDER_CALL_RECEIPT_FRAME_BYTES) {
        Ok(()) => 0,
        Err(_) => 5,
    }
}

/// Credential-bearing but traffic-free route preflight. It emits no output,
/// constructs both frozen clients under the process-only configuration, and
/// exits before opening the experiment ledger or making a provider request.
pub async fn run_provider_broker_preflight() -> i32 {
    match preflight_canonical_openai_routes() {
        Ok(()) => 0,
        Err(_) => 3,
    }
}

fn preflight_canonical_openai_routes() -> Result<(), BrokerError> {
    let paths = RuntimePaths::discover().map_err(|_| BrokerError::Route)?;
    let profile = ActiveProfile::Default;
    let store =
        ConfigStore::for_process_only_load(paths.for_profile(&profile).config_file, profile);
    let config = store.load().map_err(|_| BrokerError::Route)?;
    let provider = config.providers.get("openai").ok_or(BrokerError::Route)?;
    if provider.protocol != ProviderProtocol::OpenAiResponses
        || provider.base_url.trim_end_matches('/') != CANONICAL_OPENAI_BASE_URL
    {
        return Err(BrokerError::Route);
    }
    for model in [
        crate::reasoning_experiment_protocol::STRONG_MODEL,
        crate::reasoning_experiment_protocol::CHEAP_MODEL,
    ] {
        if !provider.models.iter().any(|configured| configured == model) {
            return Err(BrokerError::Route);
        }
        AgentClient::with_call_policy(
            "openai",
            provider,
            model,
            ProviderCallPolicy::one_attempt(),
            crate::request::RequestStore::default(),
        )
        .validate_frozen_openai_responses_route()
        .map_err(|_| BrokerError::Route)?;
    }
    Ok(())
}

/// Runs the feature-only tool worker. It receives one exact-argv tool call,
/// never loads configuration, and always requires the configured platform sandbox.
pub async fn run_tool_worker_stdio() -> i32 {
    let mut stdin = std::io::stdin().lock();
    let call = match read_ipc_frame::<_, ToolCallSpecV1>(&mut stdin, MAX_TOOL_CALL_SPEC_FRAME_BYTES)
    {
        Ok(call) => call,
        Err(_) => return 2,
    };
    let receipt = match execute_tool_call(&call).await {
        Ok(receipt) => receipt,
        Err(_) => return 3,
    };
    let mut stdout = std::io::stdout().lock();
    match write_ipc_frame(&mut stdout, &receipt, MAX_TOOL_CALL_RECEIPT_FRAME_BYTES) {
        Ok(()) => 0,
        Err(_) => 4,
    }
}

/// Traffic-free health check for the exact sandbox boundary used by tool
/// calls. The parent launches this worker with a scrubbed environment; this
/// process creates only short-lived directories beneath the private
/// experiment root and executes `/usr/bin/true` with networking denied.
pub async fn run_tool_worker_preflight() -> i32 {
    match preflight_required_tool_sandbox().await {
        Ok(()) => 0,
        Err(_) => 3,
    }
}

async fn preflight_required_tool_sandbox() -> Result<(), BrokerError> {
    let experiment_dir = std::env::current_dir().map_err(|_| BrokerError::Execution)?;
    preflight_required_tool_sandbox_in(&experiment_dir).await
}

async fn preflight_required_tool_sandbox_in(experiment_dir: &Path) -> Result<(), BrokerError> {
    let experiment_dir =
        existing_canonical_directory(experiment_dir).ok_or(BrokerError::Execution)?;
    let private_root = tempfile::Builder::new()
        .prefix("ca-rnd-tool-preflight-")
        .tempdir_in(&experiment_dir)
        .map_err(|_| BrokerError::Execution)?;
    let temporary_directory = private_root.path().join("tmp");
    std::fs::create_dir(&temporary_directory).map_err(|_| BrokerError::Execution)?;
    let policy = SandboxPolicy {
        roots: vec![private_root.path().to_path_buf()],
        read_only_roots: TOOLCHAIN_ROOTS
            .into_iter()
            .map(PathBuf::from)
            .filter(|path| path.exists())
            .collect(),
        temporary_directory: temporary_directory.clone(),
        network_allowed: false,
        hide_other_processes: true,
    };
    let prepared = SandboxBackend::new(SandboxMode::Required)
        .prepare_process(Path::new("/usr/bin/true"), &[], &policy)
        .map_err(|_| BrokerError::Execution)?;
    let status = tokio::process::Command::new(prepared.program)
        .args(prepared.arguments)
        .current_dir(private_root.path())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .env("TMPDIR", &temporary_directory)
        .status()
        .await
        .map_err(|_| BrokerError::Execution)?;
    if !status.success() {
        return Err(BrokerError::Execution);
    }
    Ok(())
}

fn canonical_openai_client(call: &ProviderCallSpecV1) -> Result<AgentClient, BrokerError> {
    call.validate().map_err(|_| BrokerError::Frame)?;
    let paths = RuntimePaths::discover().map_err(|_| BrokerError::Route)?;
    let profile = ActiveProfile::Default;
    let store =
        ConfigStore::for_process_only_load(paths.for_profile(&profile).config_file, profile);
    let config = store.load().map_err(|_| BrokerError::Route)?;
    let provider = config.providers.get("openai").ok_or(BrokerError::Route)?;
    if provider.protocol != ProviderProtocol::OpenAiResponses
        || provider.base_url.trim_end_matches('/') != CANONICAL_OPENAI_BASE_URL
        || !provider
            .models
            .iter()
            .any(|model| model == &call.route.model)
    {
        return Err(BrokerError::Route);
    }
    AgentClient::with_call_policy(
        "openai",
        provider,
        &call.route.model,
        ProviderCallPolicy::one_attempt(),
        crate::request::RequestStore::default(),
    )
    .with_openai_responses_options(OpenAiResponsesRequestOptions::capped(Some(
        OpenAiServiceTier::Default,
    )))
    .and_then(|client| client.with_openai_client_request_id(call.x_client_request_id.clone()))
    .map_err(|_| BrokerError::Route)
}

/// Executes one constrained local tool request. The worker supports only
/// exact-argv `run_command` plus typed reads and atomic rewrites of predeclared
/// regular workspace files. It never accepts a shell, network grant, provider
/// route, configuration, or credential.
///
/// The allowlist deliberately contains only the Rust toolchain programs needed
/// by the pilot. It is not a general command facility: the controller must add
/// separate, reviewed command-policy entries before another executable can be
/// used. `/usr/bin/env` is accepted only with no arguments as a test-only
/// environment probe; it cannot be used to launch a child process.
pub(crate) async fn execute_tool_call(
    call: &ToolCallSpecV1,
) -> Result<ToolCallReceiptV1, BrokerError> {
    call.validate().map_err(|_| BrokerError::Frame)?;
    if call.tool_name == "read_file" {
        return execute_read_file(call);
    }
    if call.tool_name == "write_file" {
        return execute_write_file(call);
    }
    let command = match exact_command(call) {
        Ok(command) => command,
        Err(reason) => return failed_tool_receipt(call, reason, 0),
    };
    let workspace = match existing_canonical_directory(&call.workspace_root) {
        Some(path) if path == call.workspace_root => path,
        _ => return failed_tool_receipt(call, "workspace_identity_mismatch", 0),
    };
    let target = match existing_canonical_directory(&call.cargo_target_dir) {
        Some(path) if path == call.cargo_target_dir && !path.starts_with(&workspace) => path,
        _ => return failed_tool_receipt(call, "target_identity_mismatch", 0),
    };
    let private_root = tempfile::Builder::new()
        .prefix("ca-rnd-tool-")
        .tempdir_in(&target)
        .map_err(|_| BrokerError::Execution)?;
    let home = private_root.path().join("home");
    let cargo_home = private_root.path().join("cargo-home");
    let temporary_directory = private_root.path().join("tmp");
    for directory in [&home, &cargo_home, &temporary_directory] {
        std::fs::create_dir(directory).map_err(|_| BrokerError::Execution)?;
    }
    let read_only_roots = TOOLCHAIN_ROOTS
        .into_iter()
        .map(PathBuf::from)
        .filter(|path| path.exists())
        .collect::<Vec<_>>();
    let policy = SandboxPolicy {
        roots: vec![
            workspace.clone(),
            target.clone(),
            private_root.path().to_path_buf(),
        ],
        read_only_roots,
        temporary_directory: temporary_directory.clone(),
        network_allowed: false,
        hide_other_processes: true,
    };
    let prepared = match SandboxBackend::new(SandboxMode::Required).prepare_process(
        &command.executable,
        &command.arguments,
        &policy,
    ) {
        Ok(prepared) => prepared,
        Err(_) => return failed_tool_receipt(call, "sandbox_unavailable", 0),
    };
    let started = std::time::Instant::now();
    let mut process = tokio::process::Command::new(&prepared.program);
    process
        .args(&prepared.arguments)
        .current_dir(&workspace)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .env_clear()
        .env(
            "PATH",
            "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin",
        )
        .env("HOME", &home)
        .env("CARGO_HOME", &cargo_home)
        .env("CARGO_TARGET_DIR", &target)
        .env("CARGO_NET_OFFLINE", "true")
        .env("CARGO_BUILD_JOBS", "1")
        .env("RUST_TEST_THREADS", "1")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .env("TZ", "UTC")
        .env("TMPDIR", &temporary_directory);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        process.as_std_mut().process_group(0);
    }
    let mut child = match process.spawn() {
        Ok(child) => child,
        Err(_) => return failed_tool_receipt(call, "spawn_failed", elapsed_millis(started)),
    };
    let stdout = child.stdout.take().ok_or(BrokerError::Execution)?;
    let stderr = child.stderr.take().ok_or(BrokerError::Execution)?;
    let stdout = tokio::spawn(read_bounded_output(stdout));
    let stderr = tokio::spawn(read_bounded_output(stderr));
    let outcome = match tokio::time::timeout(
        std::time::Duration::from_millis(call.timeout_millis),
        child.wait(),
    )
    .await
    {
        Ok(Ok(status)) => ToolProcessOutcome::Exited(status.code()),
        Ok(Err(_)) => ToolProcessOutcome::Failed,
        Err(_) => {
            kill_process_group(&mut child).await;
            ToolProcessOutcome::TimedOut
        }
    };
    let stdout = stdout.await.map_err(|_| BrokerError::Execution)?;
    let stderr = stderr.await.map_err(|_| BrokerError::Execution)?;
    let elapsed_millis = elapsed_millis(started);
    match outcome {
        ToolProcessOutcome::Exited(Some(0)) => completed_tool_receipt(
            call,
            ExactToolOutputV1 {
                exit_code: 0,
                stdout: stdout.text,
                stderr: stderr.text,
                stdout_omitted_bytes: stdout.omitted_bytes,
                stderr_omitted_bytes: stderr.omitted_bytes,
            },
            elapsed_millis,
        ),
        ToolProcessOutcome::TimedOut => timed_out_tool_receipt(call, elapsed_millis),
        ToolProcessOutcome::Exited(_) | ToolProcessOutcome::Failed => {
            failed_tool_receipt(call, "command_failed", elapsed_millis)
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadFileArgsV1 {
    path: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WriteFileArgsV1 {
    path: String,
    content: String,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum TypedFileOutputV1 {
    Read {
        path: String,
        content: String,
        sha256: String,
    },
    Written {
        path: String,
        sha256: String,
        bytes: usize,
    },
}

fn execute_read_file(call: &ToolCallSpecV1) -> Result<ToolCallReceiptV1, BrokerError> {
    let started = std::time::Instant::now();
    let args: ReadFileArgsV1 = match serde_json::from_str(&call.arguments_json) {
        Ok(args) => args,
        Err(_) => return failed_tool_receipt(call, "invalid_file_arguments", 0),
    };
    let (relative, path, metadata) = match select_workspace_file(call, &args.path, false) {
        Ok(file) => file,
        Err(reason) => return failed_tool_receipt(call, reason, 0),
    };
    let mut file = match open_verified_regular_file(&path, &metadata, false) {
        Ok(file) => file,
        Err(_) => return failed_tool_receipt(call, "file_changed_or_unreadable", 0),
    };
    let mut content = Vec::with_capacity(metadata.len() as usize);
    if file.read_to_end(&mut content).is_err() || content.len() > MAX_TYPED_FILE_BYTES {
        return failed_tool_receipt(call, "file_read_failed_or_oversized", 0);
    }
    let content = match String::from_utf8(content) {
        Ok(content) => content,
        Err(_) => return failed_tool_receipt(call, "file_is_not_utf8", 0),
    };
    completed_tool_receipt(
        call,
        TypedFileOutputV1::Read {
            path: relative.to_string_lossy().into_owned(),
            sha256: sha256_hex(content.as_bytes()),
            content,
        },
        elapsed_millis(started),
    )
}

fn execute_write_file(call: &ToolCallSpecV1) -> Result<ToolCallReceiptV1, BrokerError> {
    let started = std::time::Instant::now();
    let args: WriteFileArgsV1 = match serde_json::from_str(&call.arguments_json) {
        Ok(args) => args,
        Err(_) => return failed_tool_receipt(call, "invalid_file_arguments", 0),
    };
    if args.content.len() > MAX_TYPED_FILE_BYTES {
        return failed_tool_receipt(call, "file_write_oversized", 0);
    }
    let (relative, path, metadata) = match select_workspace_file(call, &args.path, true) {
        Ok(file) => file,
        Err(reason) => return failed_tool_receipt(call, reason, 0),
    };
    let parent = match path.parent() {
        Some(parent) => parent,
        None => return failed_tool_receipt(call, "file_parent_invalid", 0),
    };
    let mut temporary = match tempfile::NamedTempFile::new_in(parent) {
        Ok(file) => file,
        Err(_) => return failed_tool_receipt(call, "file_write_failed", 0),
    };
    if temporary.write_all(args.content.as_bytes()).is_err()
        || temporary.flush().is_err()
        || temporary.as_file().sync_all().is_err()
    {
        return failed_tool_receipt(call, "file_write_failed", 0);
    }
    let permissions = match metadata.permissions().readonly() {
        true => return failed_tool_receipt(call, "file_not_writable", 0),
        false => metadata.permissions(),
    };
    if temporary.as_file().set_permissions(permissions).is_err()
        || !same_regular_file_identity(&path, &metadata)
        || temporary.persist(&path).is_err()
    {
        return failed_tool_receipt(call, "file_changed_or_unwritable", 0);
    }
    completed_tool_receipt(
        call,
        TypedFileOutputV1::Written {
            path: relative.to_string_lossy().into_owned(),
            sha256: sha256_hex(args.content.as_bytes()),
            bytes: args.content.len(),
        },
        elapsed_millis(started),
    )
}

fn select_workspace_file(
    call: &ToolCallSpecV1,
    raw_path: &str,
    require_write: bool,
) -> Result<(PathBuf, PathBuf, fs::Metadata), &'static str> {
    let workspace = existing_canonical_directory(&call.workspace_root)
        .filter(|workspace| workspace == &call.workspace_root)
        .ok_or("workspace_identity_mismatch")?;
    let relative = PathBuf::from(raw_path);
    if raw_path.is_empty()
        || raw_path.len() > 4_096
        || raw_path.chars().any(char::is_control)
        || relative.is_absolute()
        || relative
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Err("file_path_invalid");
    }
    let grants = if require_write {
        &call.allowed_write_paths
    } else {
        &call.allowed_read_paths
    };
    if !grants.iter().any(|grant| grant == &relative) {
        return Err("file_path_not_granted");
    }
    let path = workspace.join(&relative);
    let metadata = fs::symlink_metadata(&path).map_err(|_| "file_not_found")?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() > MAX_TYPED_FILE_BYTES as u64
    {
        return Err("file_not_regular_or_oversized");
    }
    let canonical = path.canonicalize().map_err(|_| "file_not_found")?;
    if canonical != path || !canonical.starts_with(&workspace) {
        return Err("file_path_escape");
    }
    Ok((relative, path, metadata))
}

fn open_verified_regular_file(
    path: &Path,
    expected: &fs::Metadata,
    write: bool,
) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(write);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let file = options.open(path)?;
    if !same_file_metadata(&file.metadata()?, expected) {
        return Err(std::io::Error::other("workspace file changed"));
    }
    Ok(file)
}

fn same_regular_file_identity(path: &Path, expected: &fs::Metadata) -> bool {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return false;
    };
    !metadata.file_type().is_symlink()
        && metadata.is_file()
        && same_file_metadata(&metadata, expected)
}

fn same_file_metadata(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        left.dev() == right.dev() && left.ino() == right.ino()
    }
    #[cfg(not(unix))]
    {
        left.len() == right.len() && left.is_file() == right.is_file()
    }
}

/// Executes one provider call with an injected client. Tests use this seam with
/// a literal loopback endpoint and a synthetic credential; normal broker mode
/// is the only code path that loads configured OpenAI authority.
pub(crate) async fn execute_provider_call_with_client(
    call: &ProviderCallSpecV1,
    ledger: &ExperimentLedger,
    client: &AgentClient,
    daily_root: &Path,
    clock: impl Fn() -> DateTime<Utc>,
) -> Result<ProviderCallReceiptV1, BrokerError> {
    call.validate().map_err(|_| BrokerError::Frame)?;
    ledger
        .require_fresh_official_price_evidence(clock())
        .map_err(|_| BrokerError::PriceEvidence)?;
    let grant = ledger
        .reserve_provider_call(call)
        .map_err(|_| BrokerError::Ledger)?;
    let daily_id = format!(
        "harness:artifact:{}:{}",
        grant.reservation_id, grant.ledger_event_sha256
    );
    let maximum = grant
        .reserved_atto_usd
        .ceil_microusd()
        .map_err(|_| BrokerError::DailyBudget)?;
    let ticket = match reserve_daily_learning(daily_root, &daily_id, maximum, clock()) {
        Ok(ticket) => ticket,
        Err(_) => {
            // This new grant has not crossed the send boundary. Never release
            // a previous attempt's reservation, even when retry admission fails.
            ledger
                .release_provider_call_pre_send(&grant, call, "shared_daily_admission_denied")
                .map_err(|_| BrokerError::Ledger)?;
            return Err(BrokerError::DailyBudget);
        }
    };
    // Admission may wait for a ledger lock. Recheck freshness immediately
    // before the physical call rather than rely on the earlier controller gate.
    if ledger
        .require_fresh_official_price_evidence(clock())
        .is_err()
    {
        ledger
            .release_provider_call_pre_send(
                &grant,
                call,
                "official_price_evidence_expired_pre_send",
            )
            .map_err(|_| BrokerError::Ledger)?;
        settle_daily_learning(&ticket, None, true).map_err(|_| BrokerError::DailyBudget)?;
        return Err(BrokerError::PriceEvidence);
    }
    let started = std::time::Instant::now();
    let stream = match client
        .stream_frozen_openai_responses_body(call.request_json.as_bytes().to_vec())
        .await
    {
        Ok(stream) => stream,
        Err(_) => return unknown_receipt(call, ledger, &grant, "stream_start_failed", started),
    };
    let mut stream = stream;
    let turn = match consume_stream(&mut stream, |_| {}).await {
        Ok(turn) => turn,
        Err(_) => return unknown_receipt(call, ledger, &grant, "stream_incomplete", started),
    };
    completed_receipt(call, ledger, &grant, &ticket, turn, started)
}

fn completed_receipt(
    call: &ProviderCallSpecV1,
    ledger: &ExperimentLedger,
    grant: &crate::reasoning_experiment_ledger::ReservationGrantV1,
    ticket: &DailyLearningTicket,
    turn: AssembledAssistantTurn,
    started: std::time::Instant,
) -> Result<ProviderCallReceiptV1, BrokerError> {
    let elapsed_millis = elapsed_millis(started);
    let Some(usage_info) = turn.usage.as_ref() else {
        return unknown_receipt(call, ledger, grant, "missing_usage", started);
    };
    let Some(usage) = experiment_usage(usage_info) else {
        return unknown_receipt(call, ledger, grant, "invalid_usage", started);
    };
    let Some(metadata) = usage_info.response_metadata.as_ref() else {
        return unknown_receipt(call, ledger, grant, "missing_metadata", started);
    };
    let Some(service_tier) = metadata.service_tier() else {
        return unknown_receipt(call, ledger, grant, "missing_service_tier", started);
    };
    let request_id_sha256 = metadata.request_id_sha256().to_string();
    let returned_model = metadata.model().to_string();
    let returned_service_tier = service_tier.to_string();
    let controller_response_json =
        serde_json::to_string(&ControllerTurnV1::from(turn)).map_err(|_| BrokerError::Execution)?;
    let receipt = ProviderCallReceiptV1 {
        schema_version: PROVIDER_CALL_RECEIPT_SCHEMA_V1.into(),
        call_id: call.call_id.clone(),
        channel_id: call.channel_id.clone(),
        nonce: call.nonce.clone(),
        worker_spec_sha256: call.worker_spec_sha256.clone(),
        request_sha256: call.request_sha256.clone(),
        reservation_id: grant.reservation_id.clone(),
        x_client_request_id_sha256: sha256_hex(call.x_client_request_id.as_bytes()),
        x_request_id_sha256: Some(request_id_sha256),
        returned_model: Some(returned_model),
        returned_service_tier: Some(returned_service_tier),
        usage: Some(usage),
        elapsed_millis,
        outcome: ProviderCallOutcomeV1::Completed {
            controller_response_sha256: sha256_hex(controller_response_json.as_bytes()),
            controller_response_json,
        },
    };
    receipt
        .validate_against(call)
        .map_err(|_| BrokerError::Execution)?;
    let receipt_sha256 = receipt_sha256(&receipt)?;
    let actual = ledger
        .price_card()
        .exact_cost_for_usage(&call.route, usage)
        .and_then(|cost| cost.ceil_microusd())
        .map_err(|_| BrokerError::Ledger)?;
    ledger
        .settle_provider_call(grant, call, usage, &receipt_sha256)
        .map_err(|_| BrokerError::Ledger)?;
    settle_daily_learning(ticket, Some(actual), false).map_err(|_| BrokerError::DailyBudget)?;
    Ok(receipt)
}

fn unknown_receipt(
    call: &ProviderCallSpecV1,
    ledger: &ExperimentLedger,
    grant: &crate::reasoning_experiment_ledger::ReservationGrantV1,
    reason: &str,
    started: std::time::Instant,
) -> Result<ProviderCallReceiptV1, BrokerError> {
    ledger
        .mark_provider_call_unknown(grant, call, reason)
        .map_err(|_| BrokerError::Ledger)?;
    Ok(ProviderCallReceiptV1 {
        schema_version: PROVIDER_CALL_RECEIPT_SCHEMA_V1.into(),
        call_id: call.call_id.clone(),
        channel_id: call.channel_id.clone(),
        nonce: call.nonce.clone(),
        worker_spec_sha256: call.worker_spec_sha256.clone(),
        request_sha256: call.request_sha256.clone(),
        reservation_id: grant.reservation_id.clone(),
        x_client_request_id_sha256: sha256_hex(call.x_client_request_id.as_bytes()),
        x_request_id_sha256: None,
        returned_model: None,
        returned_service_tier: None,
        usage: None,
        elapsed_millis: elapsed_millis(started),
        outcome: ProviderCallOutcomeV1::AmbiguousDelivery {
            reason: reason.into(),
        },
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExactCommandV1 {
    executable: String,
    arguments: Vec<String>,
}

struct ExactCommand {
    executable: PathBuf,
    arguments: Vec<String>,
}

fn exact_command(call: &ToolCallSpecV1) -> Result<ExactCommand, &'static str> {
    if call.tool_name != "run_command" {
        return Err("unsupported_tool");
    }
    let raw: ExactCommandV1 =
        serde_json::from_str(&call.arguments_json).map_err(|_| "invalid_exact_command")?;
    if raw.arguments.len() > MAX_EXACT_TOOL_ARGUMENTS
        || raw.executable.is_empty()
        || raw.executable.len() > MAX_EXACT_TOOL_ARGUMENT_BYTES
        || raw.arguments.iter().any(|argument| {
            argument.is_empty()
                || argument.len() > MAX_EXACT_TOOL_ARGUMENT_BYTES
                || argument
                    .bytes()
                    .any(|byte| byte == 0 || byte.is_ascii_control())
        })
        || raw
            .executable
            .bytes()
            .any(|byte| byte == 0 || byte.is_ascii_control())
    {
        return Err("invalid_exact_command");
    }
    let total = raw
        .arguments
        .iter()
        .try_fold(raw.executable.len(), |total, argument| {
            total.checked_add(argument.len())
        })
        .ok_or("invalid_exact_command")?;
    if total > MAX_EXACT_TOOL_ARGUMENT_TOTAL_BYTES {
        return Err("invalid_exact_command");
    }
    let executable = PathBuf::from(raw.executable);
    if !executable.is_absolute() || !executable.is_file() {
        return Err("invalid_exact_command");
    }
    let executable = executable
        .canonicalize()
        .map_err(|_| "invalid_exact_command")?;
    if !allowed_experiment_executable(&executable, &raw.arguments) {
        return Err("executable_not_allowed");
    }
    Ok(ExactCommand {
        executable,
        arguments: raw.arguments,
    })
}

fn allowed_experiment_executable(executable: &Path, arguments: &[String]) -> bool {
    if executable == Path::new(ENVIRONMENT_PROBE_EXECUTABLE) {
        return arguments.is_empty();
    }
    let Some(name) = executable.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    if !matches!(name, "cargo" | "rustc") {
        return false;
    }
    TOOLCHAIN_ROOTS
        .iter()
        .map(Path::new)
        .any(|root| executable.starts_with(root))
}

fn existing_canonical_directory(path: &Path) -> Option<PathBuf> {
    let metadata = std::fs::symlink_metadata(path).ok()?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return None;
    }
    let canonical = path.canonicalize().ok()?;
    (canonical.is_dir() && canonical == path).then_some(canonical)
}

#[derive(Debug)]
enum ToolProcessOutcome {
    Exited(Option<i32>),
    TimedOut,
    Failed,
}

struct BoundedOutput {
    text: String,
    omitted_bytes: usize,
}

async fn read_bounded_output<R>(mut reader: R) -> BoundedOutput
where
    R: tokio::io::AsyncRead + Unpin,
{
    use tokio::io::AsyncReadExt;

    let mut visible = Vec::new();
    let mut omitted_bytes = 0usize;
    let mut buffer = [0u8; 4096];
    loop {
        let read = match reader.read(&mut buffer).await {
            Ok(0) | Err(_) => break,
            Ok(read) => read,
        };
        let available = MAX_TOOL_CAPTURE_PER_STREAM_BYTES.saturating_sub(visible.len());
        let kept = read.min(available);
        visible.extend_from_slice(&buffer[..kept]);
        omitted_bytes = omitted_bytes.saturating_add(read.saturating_sub(kept));
    }
    BoundedOutput {
        text: String::from_utf8_lossy(&visible).into_owned(),
        omitted_bytes,
    }
}

async fn kill_process_group(child: &mut tokio::process::Child) {
    #[cfg(unix)]
    if let Some(pid) = child.id() {
        unsafe {
            libc::kill(-(pid as libc::pid_t), libc::SIGKILL);
        }
    }
    let _ = child.kill().await;
}

fn completed_tool_receipt<T: Serialize>(
    call: &ToolCallSpecV1,
    output: T,
    elapsed_millis: u64,
) -> Result<ToolCallReceiptV1, BrokerError> {
    let result_json = serde_json::to_string(&output).map_err(|_| BrokerError::Execution)?;
    let receipt = ToolCallReceiptV1 {
        schema_version: TOOL_CALL_RECEIPT_SCHEMA_V1.into(),
        call_id: call.call_id.clone(),
        channel_id: call.channel_id.clone(),
        nonce: call.nonce.clone(),
        worker_spec_sha256: call.worker_spec_sha256.clone(),
        arguments_sha256: call.arguments_sha256.clone(),
        elapsed_millis,
        outcome: ToolCallOutcomeV1::Completed,
        result_sha256: Some(sha256_hex(result_json.as_bytes())),
        result_json: Some(result_json),
    };
    receipt
        .validate_against(call)
        .map_err(|_| BrokerError::Execution)?;
    Ok(receipt)
}

fn timed_out_tool_receipt(
    call: &ToolCallSpecV1,
    elapsed_millis: u64,
) -> Result<ToolCallReceiptV1, BrokerError> {
    tool_terminal_receipt(call, ToolCallOutcomeV1::TimedOut, elapsed_millis)
}

fn failed_tool_receipt(
    call: &ToolCallSpecV1,
    reason: &str,
    elapsed_millis: u64,
) -> Result<ToolCallReceiptV1, BrokerError> {
    tool_terminal_receipt(
        call,
        ToolCallOutcomeV1::Failed {
            reason: reason.into(),
        },
        elapsed_millis,
    )
}

fn tool_terminal_receipt(
    call: &ToolCallSpecV1,
    outcome: ToolCallOutcomeV1,
    elapsed_millis: u64,
) -> Result<ToolCallReceiptV1, BrokerError> {
    let receipt = ToolCallReceiptV1 {
        schema_version: TOOL_CALL_RECEIPT_SCHEMA_V1.into(),
        call_id: call.call_id.clone(),
        channel_id: call.channel_id.clone(),
        nonce: call.nonce.clone(),
        worker_spec_sha256: call.worker_spec_sha256.clone(),
        arguments_sha256: call.arguments_sha256.clone(),
        elapsed_millis,
        outcome,
        result_json: None,
        result_sha256: None,
    };
    receipt
        .validate_against(call)
        .map_err(|_| BrokerError::Execution)?;
    Ok(receipt)
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct ExactToolOutputV1 {
    exit_code: i32,
    stdout: String,
    stderr: String,
    stdout_omitted_bytes: usize,
    stderr_omitted_bytes: usize,
}

fn experiment_usage(usage: &UsageInfo) -> Option<ExperimentUsageV1> {
    if !usage_details_valid(usage) {
        return None;
    }
    let cached_input_tokens = usage.cached_prompt_tokens.unwrap_or(0);
    let cache_write_tokens = usage.cache_write_tokens.unwrap_or(0);
    let reasoning_output_tokens = usage.reasoning_tokens.unwrap_or(0);
    let usage = ExperimentUsageV1 {
        regular_input_tokens: u64::from(
            usage
                .prompt_tokens
                .checked_sub(cached_input_tokens)?
                .checked_sub(cache_write_tokens)?,
        ),
        cached_input_tokens: u64::from(cached_input_tokens),
        cache_write_tokens: u64::from(cache_write_tokens),
        regular_output_tokens: u64::from(
            usage
                .completion_tokens
                .checked_sub(reasoning_output_tokens)?,
        ),
        reasoning_output_tokens: u64::from(reasoning_output_tokens),
    };
    usage.validate().ok().map(|()| usage)
}

fn receipt_sha256(receipt: &ProviderCallReceiptV1) -> Result<String, BrokerError> {
    let bytes = serde_json::to_vec(receipt).map_err(|_| BrokerError::Execution)?;
    Ok(sha256_hex(&bytes))
}

fn elapsed_millis(started: std::time::Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut digest = Sha256::new();
    digest.update(bytes);
    format!("{:x}", digest.finalize())
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct ControllerTurnV1 {
    content: String,
    reasoning: Option<String>,
    /// Raw validated OpenAI output items are intentionally frame-only. The
    /// controller may replay them only to the same broker route during this
    /// phase; they never enter a ledger, trial record, recovery artifact, or
    /// cross-model handoff.
    response_items: Vec<ControllerResponseItemV1>,
    tool_calls: Vec<ControllerToolCallV1>,
    finish_reason: String,
}

impl From<AssembledAssistantTurn> for ControllerTurnV1 {
    fn from(turn: AssembledAssistantTurn) -> Self {
        Self {
            content: turn.content,
            reasoning: turn.reasoning,
            response_items: turn
                .response_items
                .into_iter()
                .filter_map(|item| match item {
                    crate::agent::stream::ProviderResponseItem::ResponsesOutput {
                        output_index,
                        item,
                        ..
                    } => Some(ControllerResponseItemV1 { output_index, item }),
                    crate::agent::stream::ProviderResponseItem::NativeContent { .. }
                    | crate::agent::stream::ProviderResponseItem::ChatReasoning { .. } => None,
                })
                .collect(),
            tool_calls: turn
                .tool_calls
                .into_iter()
                .map(|call| ControllerToolCallV1 {
                    index: call.index,
                    id: call.id,
                    name: call.name,
                    arguments: call.arguments,
                })
                .collect(),
            finish_reason: turn.finish_reason.to_string(),
        }
    }
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct ControllerResponseItemV1 {
    output_index: usize,
    item: Value,
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct ControllerToolCallV1 {
    index: usize,
    id: String,
    name: String,
    arguments: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reasoning_experiment_ledger::{
        AttoUsd, FrozenModelPriceV1, FrozenPriceCardV1, FROZEN_PRICE_CARD_SCHEMA_V1,
    };
    use crate::reasoning_experiment_protocol::{
        ExperimentModelRouteV1, ExperimentPhase, CHEAP_MODEL, MAX_INPUT_TOKENS_PER_REQUEST,
        MAX_OUTPUT_TOKENS_PER_REQUEST, PROVIDER_CALL_SPEC_SCHEMA_V1,
    };
    use codex_team::self_improvement::daily_learning_budget_status;
    use tokio::io::AsyncWriteExt;

    fn digest(byte: char) -> String {
        std::iter::repeat_n(byte, 64).collect()
    }

    fn price(value: &str) -> AttoUsd {
        AttoUsd::parse_usd(value).unwrap()
    }

    fn price_card() -> FrozenPriceCardV1 {
        FrozenPriceCardV1 {
            schema_version: FROZEN_PRICE_CARD_SCHEMA_V1.into(),
            price_card_id: "openai-gpt56-default-2026-10-10".into(),
            effective_at_rfc3339: "2026-10-10T03:59:00Z".into(),
            service_tier: "default".into(),
            sol: FrozenModelPriceV1 {
                regular_input_per_million_atto_usd: price("5"),
                cached_input_per_million_atto_usd: price("0.5"),
                cache_write_per_million_atto_usd: price("6.25"),
                regular_output_per_million_atto_usd: price("30"),
                reasoning_output_per_million_atto_usd: price("30"),
            },
            luna: FrozenModelPriceV1 {
                regular_input_per_million_atto_usd: price("0.2"),
                cached_input_per_million_atto_usd: price("0.02"),
                cache_write_per_million_atto_usd: price("0.25"),
                regular_output_per_million_atto_usd: price("1.2"),
                reasoning_output_per_million_atto_usd: price("1.2"),
            },
        }
    }

    fn call() -> ProviderCallSpecV1 {
        let request_json = format!(
            r#"{{"model":"{CHEAP_MODEL}","input":[{{"role":"user","content":"fixture"}}],"stream":true,"reasoning":{{"effort":"high"}},"include":["reasoning.encrypted_content"],"max_output_tokens":8192,"store":false,"service_tier":"default"}}"#
        );
        ProviderCallSpecV1 {
            schema_version: PROVIDER_CALL_SPEC_SCHEMA_V1.into(),
            call_id: "provider-call-1".into(),
            channel_id: "channel-1".into(),
            nonce: "nonce-1".into(),
            worker_spec_sha256: digest('a'),
            controller_sha256: digest('b'),
            worker_sha256: digest('c'),
            experiment_id: "pilot-1".into(),
            preregistration_sha256: sha256_hex(b"fixture preregistration"),
            trial_id: "trial-1".into(),
            slot_index: 0,
            phase: ExperimentPhase::Executor,
            semantic_request_index: 0,
            physical_attempt: 1,
            route: ExperimentModelRouteV1::openai(CHEAP_MODEL),
            max_input_tokens: MAX_INPUT_TOKENS_PER_REQUEST,
            max_output_tokens: MAX_OUTPUT_TOKENS_PER_REQUEST,
            store: false,
            request_sha256: sha256_hex(request_json.as_bytes()),
            request_json,
            x_client_request_id: "rnd-097-client-1".into(),
        }
    }

    fn fixture_now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-10-10T03:59:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    fn ledger(root: &Path) -> ExperimentLedger {
        use std::os::unix::fs::PermissionsExt;
        let experiment = root.parent().unwrap();
        fs::set_permissions(experiment, fs::Permissions::from_mode(0o700)).unwrap();
        let preregistration = experiment.join("preregistration.v1.json");
        fs::write(&preregistration, b"fixture preregistration").unwrap();
        fs::set_permissions(&preregistration, fs::Permissions::from_mode(0o600)).unwrap();
        let card = price_card();
        let price_receipt = serde_json::json!({
            "schema_version": "ca.rnd097.official_price_card.v1",
            "preregistration_sha256": sha256_hex(b"fixture preregistration"),
            "frozen_at": card.effective_at_rfc3339,
            "sources": [
                {"url":"https://developers.openai.com/api/docs/models/compare", "sha256":digest('a'), "bytes":100},
                {"url":"https://developers.openai.com/api/docs/models/gpt-5.6-sol", "sha256":digest('b'), "bytes":100},
                {"url":"https://developers.openai.com/api/docs/models/gpt-5.6-luna", "sha256":digest('c'), "bytes":100}
            ],
            "price_card_sha256": card.sha256().unwrap(),
            "price_card": card,
        });
        let receipt_path = experiment.join("official-price-card.v1.json");
        fs::write(&receipt_path, serde_json::to_vec(&price_receipt).unwrap()).unwrap();
        fs::set_permissions(&receipt_path, fs::Permissions::from_mode(0o600)).unwrap();
        ExperimentLedger::create(
            root,
            "pilot-1",
            &sha256_hex(b"fixture preregistration"),
            AttoUsd::DEFAULT_CAP,
            price_card(),
        )
        .unwrap()
    }

    fn client(url: String) -> AgentClient {
        AgentClient::with_call_policy(
            "openai",
            &ProviderConfig {
                google: None,
                tool_schema_policy: Default::default(),
                base_url: url,
                api_key: "fake-test-key".into(),
                protocol: ProviderProtocol::OpenAiResponses,
                prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
                models: vec![CHEAP_MODEL.into()],
                timeout: ProviderTimeoutConfig::default(),
            },
            CHEAP_MODEL,
            ProviderCallPolicy::one_attempt(),
            crate::request::RequestStore::default(),
        )
        .with_openai_responses_options(OpenAiResponsesRequestOptions::capped(Some(
            OpenAiServiceTier::Default,
        )))
        .unwrap()
        .with_openai_client_request_id("rnd-097-client-1")
        .unwrap()
    }

    async fn read_http_request(stream: &mut tokio::net::TcpStream) -> Vec<u8> {
        use tokio::io::AsyncReadExt;

        let mut request = Vec::new();
        let mut chunk = [0_u8; 4096];
        loop {
            let read = stream.read(&mut chunk).await.unwrap();
            if read == 0 {
                break;
            }
            request.extend_from_slice(&chunk[..read]);
            let Some(header_end) = request
                .windows(4)
                .position(|window| window == b"\r\n\r\n")
                .map(|index| index + 4)
            else {
                continue;
            };
            let headers = String::from_utf8_lossy(&request[..header_end]);
            let content_length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())
                        .flatten()
                })
                .unwrap_or(0);
            if request.len() >= header_end + content_length {
                break;
            }
        }
        request
    }

    fn request_body(request: &[u8]) -> &[u8] {
        let header_end = request
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .unwrap()
            + 4;
        &request[header_end..]
    }

    #[tokio::test]
    async fn shared_learning_and_harness_spend_denies_a_fifty_dollar_pilot_before_send() {
        use codex_team::self_improvement::{reserve_daily_learning, settle_daily_learning};
        let root = tempfile::tempdir().unwrap();
        let daily_root = root.path().join("daily");
        let now = chrono::DateTime::parse_from_rfc3339("2026-10-10T03:59:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let knowledge =
            reserve_daily_learning(&daily_root, "knowledge:prior", 1_900_000, now).unwrap();
        settle_daily_learning(&knowledge, Some(1_900_000), false).unwrap();
        reserve_daily_learning(&daily_root, "harness:uncertain", 80_000, now).unwrap();
        let ledger = ledger(&root.path().join("ledger"));
        assert_eq!(ledger.header().cap_atto_usd, AttoUsd::DEFAULT_CAP);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let accepted =
                tokio::time::timeout(std::time::Duration::from_millis(300), listener.accept())
                    .await;
            if let Ok(Ok((mut stream, _))) = accepted {
                read_http_request(&mut stream).await;
                stream
                    .write_all(b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\n\r\n")
                    .await
                    .unwrap();
                1
            } else {
                0
            }
        });
        let result =
            execute_provider_call_with_client(&call(), &ledger, &client(url), &daily_root, || now)
                .await;
        let posts = server.await.unwrap();
        assert!(
            matches!(result, Err(BrokerError::DailyBudget)),
            "the shared $2 allowance must override the pilot's $50 grant"
        );
        assert_eq!(
            posts, 0,
            "a denied physical call must never reach the provider"
        );
        assert_eq!(
            ledger.snapshot().unwrap().outstanding_reserved_atto_usd,
            AttoUsd::ZERO
        );
        let remaining = daily_learning_budget_status(&daily_root, now).unwrap();
        assert_eq!(remaining.spent_microusd, 1_900_000);
        assert_eq!(remaining.reserved_microusd, 80_000);
        assert_eq!(remaining.remaining_microusd, 20_000);
    }

    #[tokio::test]
    async fn one_provider_call_reserves_once_posts_once_and_settles_exact_usage() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let request = read_http_request(&mut stream).await;
            let body = concat!(
                "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp-1\",\"model\":\"gpt-5.6-luna\",\"service_tier\":\"default\"}}\n\n",
                "data: {\"type\":\"response.output_text.delta\",\"output_index\":0,\"content_index\":0,\"delta\":\"OK\"}\n\n",
                "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-1\",\"model\":\"gpt-5.6-luna\",\"service_tier\":\"default\",\"status\":\"completed\",\"usage\":{\"input_tokens\":10,\"output_tokens\":2,\"total_tokens\":12,\"input_tokens_details\":{\"cached_tokens\":3,\"cache_write_tokens\":4},\"output_tokens_details\":{\"reasoning_tokens\":1}}}}\n\n"
            );
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nx-request-id: provider-request-id\r\nContent-Length: {}\r\n\r\n",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
            stream.write_all(body.as_bytes()).await.unwrap();
            request
        });
        let root = tempfile::tempdir().unwrap();
        let ledger = ledger(&root.path().join("ledger"));
        let call = call();
        let receipt = execute_provider_call_with_client(
            &call,
            &ledger,
            &client(url),
            &root.path().join("daily"),
            fixture_now,
        )
        .await
        .unwrap();
        receipt.validate_against(&call).unwrap();
        assert!(matches!(
            receipt.outcome,
            ProviderCallOutcomeV1::Completed { .. }
        ));
        assert_eq!(
            receipt.usage,
            Some(ExperimentUsageV1 {
                regular_input_tokens: 3,
                cached_input_tokens: 3,
                cache_write_tokens: 4,
                regular_output_tokens: 1,
                reasoning_output_tokens: 1,
            })
        );
        let request = server.await.unwrap();
        assert!(String::from_utf8_lossy(&request)
            .to_ascii_lowercase()
            .contains("x-client-request-id: rnd-097-client-1\r\n"));
        assert_eq!(request_body(&request), call.request_json.as_bytes());
        let snapshot = ledger.snapshot().unwrap();
        assert!(snapshot.settled_actual_atto_usd > AttoUsd::ZERO);
        assert_eq!(snapshot.outstanding_reserved_atto_usd, AttoUsd::ZERO);
        assert_eq!(snapshot.settled_actual_atto_usd, price("0.00000406"));
        let daily =
            daily_learning_budget_status(&root.path().join("daily"), fixture_now()).unwrap();
        assert_eq!(daily.spent_microusd, 5);
        assert_eq!(daily.reserved_microusd, 0);
        let persisted = std::fs::read_dir(root.path().join("ledger/events"))
            .unwrap()
            .map(|entry| std::fs::read_to_string(entry.unwrap().path()).unwrap())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(!persisted.contains("fixture"));
        assert!(!persisted.contains("OK"));
        assert!(!persisted.contains("rnd-097-client-1"));
    }

    #[tokio::test]
    async fn failed_provider_start_keeps_the_full_reservation_unknown() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let mut requests = 0;
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().await.unwrap();
                let _ = read_http_request(&mut stream).await;
                stream
                    .write_all(b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\n\r\n")
                    .await
                    .unwrap();
                requests += 1;
            }
            assert!(
                tokio::time::timeout(std::time::Duration::from_millis(100), listener.accept())
                    .await
                    .is_err()
            );
            requests
        });
        let root = tempfile::tempdir().unwrap();
        let ledger = ledger(&root.path().join("ledger"));
        let call = call();
        let receipt = execute_provider_call_with_client(
            &call,
            &ledger,
            &client(url.clone()),
            &root.path().join("daily"),
            fixture_now,
        )
        .await
        .unwrap();
        assert!(matches!(
            receipt.outcome,
            ProviderCallOutcomeV1::AmbiguousDelivery { .. }
        ));
        let snapshot = ledger.snapshot().unwrap();
        assert_eq!(
            snapshot.unknown_reserved_atto_usd,
            ledger
                .price_card()
                .worst_case_for_route(&call.route)
                .unwrap()
        );
        let daily_root = root.path().join("daily");
        assert_eq!(
            daily_learning_budget_status(&daily_root, fixture_now())
                .unwrap()
                .reserved_microusd,
            25_831
        );
        let reopened = ExperimentLedger::open(&root.path().join("ledger")).unwrap();
        assert_eq!(
            reopened.snapshot().unwrap().unknown_reserved_atto_usd,
            snapshot.unknown_reserved_atto_usd
        );
        let mut retry = call.clone();
        retry.physical_attempt = 2;
        retry.call_id = "provider-call-2".into();
        retry.nonce = "nonce-2".into();
        let receipt = execute_provider_call_with_client(
            &retry,
            &reopened,
            &client(url),
            &daily_root,
            fixture_now,
        )
        .await
        .unwrap();
        assert!(matches!(
            receipt.outcome,
            ProviderCallOutcomeV1::AmbiguousDelivery { .. }
        ));
        assert_eq!(server.await.unwrap(), 2);
        assert_eq!(
            daily_learning_budget_status(&daily_root, fixture_now())
                .unwrap()
                .reserved_microusd,
            51_662
        );
        let new_day = fixture_now() + chrono::Duration::minutes(2);
        let fresh = daily_learning_budget_status(&daily_root, new_day).unwrap();
        assert_eq!(fresh.date, "2026-10-10");
        assert_eq!(fresh.remaining_microusd, 2_000_000);
        assert_eq!(
            daily_learning_budget_status(&daily_root, fixture_now())
                .unwrap()
                .reserved_microusd,
            51_662
        );
        assert_eq!(
            ExperimentLedger::open(&root.path().join("ledger"))
                .unwrap()
                .snapshot()
                .unwrap()
                .unknown_reserved_atto_usd,
            snapshot
                .unknown_reserved_atto_usd
                .checked_add(snapshot.unknown_reserved_atto_usd)
                .unwrap()
        );
    }

    #[test]
    fn physical_pricing_gate_rejects_stale_future_drift_and_underpriced_evidence() {
        let root = tempfile::tempdir().unwrap();
        let ledger = ledger(&root.path().join("ledger"));
        ledger
            .require_fresh_official_price_evidence(fixture_now())
            .unwrap();
        assert!(ledger
            .require_fresh_official_price_evidence(fixture_now() - chrono::Duration::seconds(1))
            .is_err());
        assert!(ledger
            .require_fresh_official_price_evidence(
                fixture_now() + chrono::Duration::hours(24) + chrono::Duration::seconds(1)
            )
            .is_err());
        let receipt_path = root.path().join("official-price-card.v1.json");
        let original = fs::read(&receipt_path).unwrap();
        for (field, value) in [
            ("preregistration_sha256", serde_json::json!(digest('e'))),
            ("frozen_at", serde_json::json!("2026-10-10T03:58:00Z")),
            ("price_card_sha256", serde_json::json!(digest('f'))),
        ] {
            let mut receipt: Value = serde_json::from_slice(&original).unwrap();
            receipt[field] = value;
            fs::write(&receipt_path, serde_json::to_vec(&receipt).unwrap()).unwrap();
            assert!(ledger
                .require_fresh_official_price_evidence(fixture_now())
                .is_err());
        }
        let mut receipt: Value = serde_json::from_slice(&original).unwrap();
        receipt["sources"][0]["url"] = serde_json::json!("https://example.invalid/prices");
        fs::write(&receipt_path, serde_json::to_vec(&receipt).unwrap()).unwrap();
        assert!(ledger
            .require_fresh_official_price_evidence(fixture_now())
            .is_err());
        fs::write(&receipt_path, &original).unwrap();
        let mut cheap_card = price_card();
        cheap_card.luna.regular_output_per_million_atto_usd = price("0.000001");
        let underpriced = ExperimentLedger::create(
            &root.path().join("underpriced"),
            "pilot-1",
            &sha256_hex(b"fixture preregistration"),
            AttoUsd::DEFAULT_CAP,
            cheap_card.clone(),
        )
        .unwrap();
        let mut receipt: Value = serde_json::from_slice(&original).unwrap();
        receipt["price_card_sha256"] = serde_json::json!(cheap_card.sha256().unwrap());
        receipt["price_card"] = serde_json::to_value(cheap_card).unwrap();
        fs::write(&receipt_path, serde_json::to_vec(&receipt).unwrap()).unwrap();
        assert!(
            underpriced
                .require_fresh_official_price_evidence(fixture_now())
                .is_err(),
            "a matching fabricated cheaper card cannot authorize physical traffic"
        );
    }

    #[tokio::test]
    async fn pricing_expiry_during_admission_denies_send_and_releases_only_absent_tickets() {
        let root = tempfile::tempdir().unwrap();
        let ledger = ledger(&root.path().join("ledger"));
        let daily_root = root.path().join("daily");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        let calls = std::cell::Cell::new(0);
        let clock = || {
            let call = calls.get();
            calls.set(call + 1);
            fixture_now() + chrono::Duration::hours(if call >= 2 { 25 } else { 0 })
        };
        let result =
            execute_provider_call_with_client(&call(), &ledger, &client(url), &daily_root, clock)
                .await;
        assert!(matches!(result, Err(BrokerError::PriceEvidence)));
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(100), listener.accept())
                .await
                .is_err()
        );
        assert_eq!(
            ledger.snapshot().unwrap().outstanding_reserved_atto_usd,
            AttoUsd::ZERO
        );
        let daily = daily_learning_budget_status(&daily_root, fixture_now()).unwrap();
        assert_eq!(daily.reserved_microusd, 0);
        assert_eq!(daily.spent_microusd, 0);
    }

    #[tokio::test]
    async fn missing_or_invalid_provider_usage_retains_the_shared_reservation() {
        for usage in [
            None,
            Some(
                serde_json::json!({"input_tokens":10,"output_tokens":2,"total_tokens":12,"input_tokens_details":{"cached_tokens":20}}),
            ),
        ] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}/v1", listener.local_addr().unwrap());
            let server = tokio::spawn(async move {
                let (mut stream, _) = listener.accept().await.unwrap();
                read_http_request(&mut stream).await;
                let mut response = serde_json::json!({"id":"resp-unknown","model":CHEAP_MODEL,"service_tier":"default","status":"completed"});
                if let Some(usage) = usage {
                    response["usage"] = usage;
                }
                let body = format!(
                    "data: {}\n\n",
                    serde_json::json!({"type":"response.completed","response":response})
                );
                stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nx-request-id: provider-request-id\r\nContent-Length: {}\r\n\r\n{}", body.len(), body).as_bytes()).await.unwrap();
            });
            let root = tempfile::tempdir().unwrap();
            let ledger = ledger(&root.path().join("ledger"));
            let daily_root = root.path().join("daily");
            let receipt = execute_provider_call_with_client(
                &call(),
                &ledger,
                &client(url),
                &daily_root,
                fixture_now,
            )
            .await
            .unwrap();
            server.await.unwrap();
            assert!(matches!(
                receipt.outcome,
                ProviderCallOutcomeV1::AmbiguousDelivery { .. }
            ));
            let daily = daily_learning_budget_status(&daily_root, fixture_now()).unwrap();
            assert_eq!(daily.reserved_microusd, 25_831);
            assert_eq!(daily.spent_microusd, 0);
            assert_eq!(
                ExperimentLedger::open(&root.path().join("ledger"))
                    .unwrap()
                    .snapshot()
                    .unwrap()
                    .unknown_reserved_atto_usd
                    .ceil_microusd()
                    .unwrap(),
                25_831
            );
        }
    }

    #[tokio::test]
    async fn cancelled_physical_call_retains_both_reservations_after_reopen() {
        let root = tempfile::tempdir().unwrap();
        let ledger = std::sync::Arc::new(ledger(&root.path().join("ledger")));
        let daily_root = root.path().join("daily");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        let (sent, received) = tokio::sync::oneshot::channel();
        let (release, released) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            read_http_request(&mut stream).await;
            sent.send(()).unwrap();
            released.await.unwrap();
        });
        let mut running = tokio::spawn({
            let ledger = ledger.clone();
            let daily_root = daily_root.clone();
            async move {
                execute_provider_call_with_client(
                    &call(),
                    &ledger,
                    &client(url),
                    &daily_root,
                    fixture_now,
                )
                .await
            }
        });
        tokio::select! {
            delivered = received => delivered.unwrap(),
            result = &mut running => panic!("provider call ended before the held response: {result:?}"),
            _ = tokio::time::sleep(std::time::Duration::from_secs(3)) => panic!("provider request did not reach the loopback server"),
        }
        running.abort();
        assert!(running.await.unwrap_err().is_cancelled());
        release.send(()).unwrap();
        server.await.unwrap();
        let daily = daily_learning_budget_status(&daily_root, fixture_now()).unwrap();
        assert_eq!(daily.reserved_microusd, 25_831);
        assert_eq!(daily.spent_microusd, 0);
        let reopened = ExperimentLedger::open(&root.path().join("ledger")).unwrap();
        assert_eq!(
            reopened
                .snapshot()
                .unwrap()
                .outstanding_reserved_atto_usd
                .ceil_microusd()
                .unwrap(),
            25_831
        );
    }

    #[test]
    fn controller_turn_keeps_openai_output_items_only_in_the_ipc_payload() {
        let turn = AssembledAssistantTurn {
            content: "done".into(),
            reasoning: None,
            response_items: vec![
                crate::agent::stream::ProviderResponseItem::ResponsesOutput {
                    source: crate::agent::stream::ProviderStateSourceV1::for_test(
                        "openai",
                        ProviderProtocol::OpenAiResponses,
                    ),
                    output_index: 3,
                    item: serde_json::json!({"type": "reasoning", "encrypted_content": "opaque"}),
                },
            ],
            tool_calls: vec![],
            usage: None,
            finish_reason: crate::agent::stream::FinishReason::Stop,
        };
        let payload = serde_json::to_string(&ControllerTurnV1::from(turn)).unwrap();
        assert!(payload.contains("opaque"));
        assert!(payload.contains("output_index"));
        // `ControllerTurnV1` is a private frame-only type, while every durable
        // receipt is constructed from a hash/projection of this payload.
        assert!(!format!(
            "{:?}",
            ProviderCallOutcomeV1::Completed {
                controller_response_json: payload,
                controller_response_sha256: digest('a'),
            }
        )
        .contains("opaque"));
    }

    fn tool_call(
        workspace: &Path,
        target: &Path,
        executable: &str,
        arguments: &[&str],
    ) -> ToolCallSpecV1 {
        let arguments_json = serde_json::json!({
            "executable": executable,
            "arguments": arguments,
        })
        .to_string();
        ToolCallSpecV1 {
            schema_version: crate::reasoning_experiment_protocol::TOOL_CALL_SPEC_SCHEMA_V1.into(),
            call_id: "tool-call-1".into(),
            channel_id: "tool-channel-1".into(),
            nonce: "tool-nonce-1".into(),
            worker_spec_sha256: digest('a'),
            workspace_id: "workspace-1".into(),
            workspace_root: workspace.to_path_buf(),
            source_manifest_sha256: digest('b'),
            cargo_target_dir: target.to_path_buf(),
            tool_name: "run_command".into(),
            tool_call_id: "call-1".into(),
            arguments_sha256: sha256_hex(arguments_json.as_bytes()),
            arguments_json,
            allowed_read_paths: vec![PathBuf::from("src")],
            allowed_write_paths: vec![],
            timeout_millis: 5_000,
        }
    }

    fn required_test_sandbox_is_configured() -> bool {
        (cfg!(target_os = "macos") && Path::new("/usr/bin/sandbox-exec").is_file())
            || (cfg!(target_os = "freebsd")
                && Path::new("/usr/local/libexec/codex-freebsd-sandbox-client").is_file()
                && Path::new("/var/run/codex-freebsd-sandbox/control.sock").exists())
    }

    #[tokio::test]
    async fn tool_worker_preflight_exercises_the_required_platform_sandbox() {
        let experiment = tempfile::tempdir().unwrap();
        let canonical = experiment.path().canonicalize().unwrap();
        let result = preflight_required_tool_sandbox_in(&canonical).await;
        if required_test_sandbox_is_configured() {
            result.expect("configured platform sandbox preflight must execute successfully");
        } else {
            assert!(result.is_err(), "unsupported platforms must fail closed");
        }
    }

    #[tokio::test]
    async fn tool_worker_uses_exact_argv_scrubbed_environment_and_required_sandbox() {
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("workspace");
        let target = root.path().join("target");
        std::fs::create_dir(&workspace).unwrap();
        std::fs::create_dir(&target).unwrap();
        let workspace = workspace.canonicalize().unwrap();
        let target = target.canonicalize().unwrap();
        let call = tool_call(&workspace, &target, "/usr/bin/env", &[]);
        unsafe { std::env::set_var("RND_TOOL_PARENT_SECRET", "must-not-leak") };
        let receipt = execute_tool_call(&call).await.unwrap();
        unsafe { std::env::remove_var("RND_TOOL_PARENT_SECRET") };
        receipt.validate_against(&call).unwrap();
        if required_test_sandbox_is_configured() {
            assert!(matches!(receipt.outcome, ToolCallOutcomeV1::Completed));
            let output = receipt.result_json.unwrap();
            assert!(output.contains("CARGO_NET_OFFLINE"));
            assert!(!output.contains("RND_TOOL_PARENT_SECRET"));
            assert!(!output.contains("must-not-leak"));
        } else {
            assert!(matches!(
                receipt.outcome,
                ToolCallOutcomeV1::Failed { ref reason } if reason == "sandbox_unavailable"
            ));
            assert!(receipt.result_json.is_none());
            assert!(receipt.result_sha256.is_none());
        }
    }

    #[tokio::test]
    async fn tool_worker_rejects_shell_payloads_before_spawning() {
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("workspace");
        let target = root.path().join("target");
        std::fs::create_dir(&workspace).unwrap();
        std::fs::create_dir(&target).unwrap();
        let mut call = tool_call(
            &workspace.canonicalize().unwrap(),
            &target.canonicalize().unwrap(),
            "/bin/sh",
            &["-c", "echo unsafe"],
        );
        call.tool_name = "run_command".into();
        let receipt = execute_tool_call(&call).await.unwrap();
        assert!(matches!(
            receipt.outcome,
            ToolCallOutcomeV1::Failed { ref reason } if reason == "executable_not_allowed"
        ));
    }

    #[tokio::test]
    async fn tool_worker_rejects_environment_wrappers_with_arguments() {
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("workspace");
        let target = root.path().join("target");
        std::fs::create_dir(&workspace).unwrap();
        std::fs::create_dir(&target).unwrap();
        let call = tool_call(
            &workspace.canonicalize().unwrap(),
            &target.canonicalize().unwrap(),
            ENVIRONMENT_PROBE_EXECUTABLE,
            &["/bin/sh", "-c", "echo unsafe"],
        );
        let receipt = execute_tool_call(&call).await.unwrap();
        assert!(matches!(
            receipt.outcome,
            ToolCallOutcomeV1::Failed { ref reason } if reason == "executable_not_allowed"
        ));
    }

    #[tokio::test]
    async fn typed_file_tools_are_exact_grant_bound_and_never_follow_links() {
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("workspace");
        let target = root.path().join("target");
        let source = workspace.join("src");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::create_dir(&target).unwrap();
        let file = source.join("lib.rs");
        std::fs::write(&file, "pub const BEFORE: &str = \"before\";\n").unwrap();
        let workspace = workspace.canonicalize().unwrap();
        let target = target.canonicalize().unwrap();

        let mut read = tool_call(&workspace, &target, "/usr/bin/env", &[]);
        read.tool_name = "read_file".into();
        read.arguments_json = serde_json::json!({"path": "src/lib.rs"}).to_string();
        read.arguments_sha256 = sha256_hex(read.arguments_json.as_bytes());
        read.allowed_read_paths = vec![PathBuf::from("src/lib.rs")];
        let receipt = execute_tool_call(&read).await.unwrap();
        receipt.validate_against(&read).unwrap();
        let output: serde_json::Value =
            serde_json::from_str(receipt.result_json.as_deref().unwrap()).unwrap();
        assert_eq!(output["kind"], "read");
        assert_eq!(output["content"], "pub const BEFORE: &str = \"before\";\n");

        let mut write = read.clone();
        write.call_id = "tool-call-2".into();
        write.tool_call_id = "call-2".into();
        write.tool_name = "write_file".into();
        write.arguments_json =
            serde_json::json!({"path": "src/lib.rs", "content": "pub const AFTER: &str = \"after\";\n"})
                .to_string();
        write.arguments_sha256 = sha256_hex(write.arguments_json.as_bytes());
        write.allowed_write_paths = vec![PathBuf::from("src/lib.rs")];
        let receipt = execute_tool_call(&write).await.unwrap();
        receipt.validate_against(&write).unwrap();
        assert!(matches!(receipt.outcome, ToolCallOutcomeV1::Completed));
        assert_eq!(
            std::fs::read_to_string(workspace.join("src/lib.rs")).unwrap(),
            "pub const AFTER: &str = \"after\";\n"
        );

        let mut ungranted = read.clone();
        ungranted.call_id = "tool-call-3".into();
        ungranted.tool_call_id = "call-3".into();
        ungranted.arguments_json = serde_json::json!({"path": "Cargo.toml"}).to_string();
        ungranted.arguments_sha256 = sha256_hex(ungranted.arguments_json.as_bytes());
        let receipt = execute_tool_call(&ungranted).await.unwrap();
        assert!(matches!(
            receipt.outcome,
            ToolCallOutcomeV1::Failed { ref reason } if reason == "file_path_not_granted"
        ));

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;

            let outside = root.path().join("outside.rs");
            std::fs::write(&outside, "private").unwrap();
            symlink(&outside, workspace.join("src/escape.rs")).unwrap();
            let mut link = read.clone();
            link.call_id = "tool-call-4".into();
            link.tool_call_id = "call-4".into();
            link.arguments_json = serde_json::json!({"path": "src/escape.rs"}).to_string();
            link.arguments_sha256 = sha256_hex(link.arguments_json.as_bytes());
            link.allowed_read_paths = vec![PathBuf::from("src/escape.rs")];
            let receipt = execute_tool_call(&link).await.unwrap();
            assert!(matches!(
                receipt.outcome,
                ToolCallOutcomeV1::Failed { ref reason } if reason == "file_not_regular_or_oversized"
            ));
            assert_eq!(std::fs::read_to_string(outside).unwrap(), "private");
        }
    }
}
