use crate::sandbox::{SandboxBackend, SandboxMode, SandboxPolicy};
use crate::workspace::WorkspaceContext;
use crate::worktree::WorktreeRecord;
use clap::{Parser, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncReadExt;

const NATIVE_RELATIVE_PATH: &str = ".ca/worktrees.toml";
const CURSOR_RELATIVE_PATH: &str = ".cursor/worktrees.json";
const MAX_SOURCE_BYTES: u64 = 256 * 1024;
const MAX_STEPS: usize = 32;
const MAX_ARGS: usize = 64;
const MAX_ARGUMENT_BYTES: usize = 4 * 1024;
const MAX_STEP_OUTPUT_BYTES: usize = 1024 * 1024;
const STEP_TIMEOUT: Duration = Duration::from_secs(600);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupSourceKind {
    Native,
    CursorCompatibility,
}

impl SetupSourceKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::CursorCompatibility => "cursor_compatibility",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupStep {
    pub name: String,
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub sandbox: SandboxMode,
    pub network: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupPreview {
    pub source_kind: SetupSourceKind,
    pub source_path: PathBuf,
    pub source_sha256: String,
    pub repository_root: PathBuf,
    pub label: String,
    pub base_ref: String,
    pub base_commit: String,
    pub steps: Vec<SetupStep>,
    pub digest: String,
}

impl SetupPreview {
    pub fn requires_process_trust(&self) -> bool {
        self.steps
            .iter()
            .any(|step| step.sandbox != SandboxMode::Required)
    }

    pub fn render_text(&self) -> String {
        let mut lines = vec![
            "Worktree setup preview".to_string(),
            String::new(),
            format!("Source: {}", self.source_kind.label()),
            format!("Configuration: {}", self.source_path.display()),
            format!("Configuration SHA-256: {}", self.source_sha256),
            format!("Repository: {}", self.repository_root.display()),
            format!("Worktree name: {}", self.label),
            format!("Base: {} ({})", self.base_ref, short_oid(&self.base_commit)),
            format!("Digest: {}", self.digest),
            String::new(),
        ];
        for (index, step) in self.steps.iter().enumerate() {
            lines.push(format!(
                "{}. {} · cwd={} · sandbox={} · network={}",
                index + 1,
                step.name,
                step.cwd.display(),
                step.sandbox,
                if step.network { "allowed" } else { "denied" }
            ));
            lines.push(format!(
                "   {}{}",
                step.program,
                if step.args.is_empty() {
                    String::new()
                } else {
                    format!(" {}", step.args.join(" "))
                }
            ));
        }
        lines.join("\n")
    }

    fn json_value(&self) -> serde_json::Value {
        serde_json::json!({
            "schema": "ca.worktree-setup-preview.v1",
            "configured": true,
            "source": self.source_kind.label(),
            "source_path": self.source_path,
            "source_sha256": self.source_sha256,
            "repository_root": self.repository_root,
            "worktree_name": self.label,
            "base_ref": self.base_ref,
            "base_commit": self.base_commit,
            "digest": self.digest,
            "steps": self.steps.iter().map(|step| serde_json::json!({
                "name": step.name,
                "program": step.program,
                "args": step.args,
                "cwd": step.cwd,
                "sandbox": step.sandbox.as_str(),
                "network": step.network,
            })).collect::<Vec<_>>(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupReceipt {
    pub digest: String,
    pub completed_steps: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupConfirmation {
    pub record: WorktreeRecord,
    pub preview: SetupPreview,
    pub run_selected: bool,
    pub busy: bool,
    pub error: Option<String>,
}

impl SetupConfirmation {
    pub fn new(record: WorktreeRecord, preview: SetupPreview) -> Self {
        Self {
            record,
            preview,
            run_selected: false,
            busy: false,
            error: None,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SetupError {
    #[error("workspace has no primary root")]
    NoPrimaryRoot,
    #[error("worktree setup configuration must be a regular file and not a symlink")]
    UnsafeSource,
    #[error("worktree setup configuration exceeds the 256 KiB limit")]
    SourceTooLarge,
    #[error("worktree setup configuration could not be read")]
    SourceRead,
    #[error("worktree setup configuration is malformed: {0}")]
    Malformed(String),
    #[error("worktree setup contains too many steps")]
    TooManySteps,
    #[error("worktree setup step is invalid: {0}")]
    InvalidStep(String),
    #[error("worktree setup source changed after preview")]
    Drift,
    #[error("worktree setup identity does not match the created worktree")]
    IdentityMismatch,
    #[error("worktree setup executable is unavailable: {0}")]
    ExecutableUnavailable(String),
    #[error("worktree setup sandbox could not be prepared")]
    SandboxUnavailable,
    #[error("worktree setup step could not be started: {0}")]
    StartFailed(String),
    #[error("worktree setup step timed out: {0}")]
    Timeout(String),
    #[error("worktree setup step failed: {0}")]
    StepFailed(String),
    #[error("worktree setup step output exceeded its safe limit: {0}")]
    OutputTooLarge(String),
    #[error("Git command failed while preparing worktree setup")]
    Git,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeConfig {
    version: u32,
    steps: Vec<NativeStep>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeStep {
    name: String,
    program: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default = "default_cwd")]
    cwd: PathBuf,
    #[serde(default = "default_sandbox")]
    sandbox: String,
    #[serde(default)]
    network: bool,
}

fn default_cwd() -> PathBuf {
    PathBuf::from(".")
}

fn default_sandbox() -> String {
    "required".into()
}

pub async fn inspect(
    workspace: &WorkspaceContext,
    label: &str,
    base: Option<&str>,
) -> Result<Option<SetupPreview>, SetupError> {
    crate::worktree::validate_label(label)
        .map_err(|error| SetupError::InvalidStep(error.to_string()))?;
    let base_ref = base.unwrap_or("HEAD");
    crate::worktree::validate_base(base_ref)
        .map_err(|error| SetupError::InvalidStep(error.to_string()))?;
    let primary = workspace.primary().ok_or(SetupError::NoPrimaryRoot)?;
    let repository_root = repository_root(&primary.path).await?;
    let base_commit = git_text(
        &repository_root,
        &[
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{base_ref}^{{commit}}"),
        ],
    )
    .await?
    .trim()
    .to_string();
    inspect_identity(repository_root, label, base_ref, base_commit).await
}

async fn inspect_identity(
    repository_root: PathBuf,
    label: &str,
    base_ref: &str,
    base_commit: String,
) -> Result<Option<SetupPreview>, SetupError> {
    let native = repository_root.join(NATIVE_RELATIVE_PATH);
    let cursor = repository_root.join(CURSOR_RELATIVE_PATH);
    let selected = if std::fs::symlink_metadata(&native).is_ok() {
        Some((SetupSourceKind::Native, native))
    } else if std::fs::symlink_metadata(&cursor).is_ok() {
        Some((SetupSourceKind::CursorCompatibility, cursor))
    } else {
        None
    };
    let Some((source_kind, source_path)) = selected else {
        return Ok(None);
    };
    let bytes = read_source(&source_path)?;
    let steps = match source_kind {
        SetupSourceKind::Native => parse_native(&bytes)?,
        SetupSourceKind::CursorCompatibility => parse_cursor(&bytes)?,
    };
    let source_sha256 = format!("{:x}", Sha256::digest(&bytes));
    let digest_material = serde_json::json!({
        "schema": "ca.worktree-setup.v1",
        "source_kind": source_kind.label(),
        "source_sha256": source_sha256,
        "repository_root": repository_root,
        "worktree_name": label,
        "base_ref": base_ref,
        "base_commit": base_commit,
        "steps": steps.iter().map(|step| serde_json::json!({
            "name": step.name,
            "program": step.program,
            "args": step.args,
            "cwd": step.cwd,
            "sandbox": step.sandbox.as_str(),
            "network": step.network,
        })).collect::<Vec<_>>(),
    });
    let digest = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&digest_material).expect("JSON value is serializable"))
    );
    Ok(Some(SetupPreview {
        source_kind,
        source_path,
        source_sha256,
        repository_root,
        label: label.to_string(),
        base_ref: base_ref.to_string(),
        base_commit,
        steps,
        digest,
    }))
}

pub async fn execute(
    record: &WorktreeRecord,
    approved: &SetupPreview,
) -> Result<SetupReceipt, SetupError> {
    let current = inspect_identity(
        approved.repository_root.clone(),
        &approved.label,
        &approved.base_ref,
        approved.base_commit.clone(),
    )
    .await?
    .ok_or(SetupError::Drift)?;
    if current != *approved {
        return Err(SetupError::Drift);
    }
    if record.repository_root != approved.repository_root
        || record.label != approved.label
        || record.base_commit != approved.base_commit
    {
        return Err(SetupError::IdentityMismatch);
    }
    for step in &approved.steps {
        execute_step(record, step).await?;
    }
    Ok(SetupReceipt {
        digest: approved.digest.clone(),
        completed_steps: approved.steps.len(),
    })
}

async fn execute_step(record: &WorktreeRecord, step: &SetupStep) -> Result<(), SetupError> {
    let cwd = std::fs::canonicalize(record.path.join(&step.cwd))
        .map_err(|_| SetupError::InvalidStep(step.name.clone()))?;
    if !cwd.starts_with(&record.path) || !cwd.is_dir() {
        return Err(SetupError::InvalidStep(step.name.clone()));
    }
    let executable = resolve_executable(&step.program, &cwd)
        .ok_or_else(|| SetupError::ExecutableUnavailable(step.name.clone()))?;
    let temporary = tempfile::tempdir().map_err(|_| SetupError::StartFailed(step.name.clone()))?;
    let prepared = SandboxBackend::new(step.sandbox)
        .prepare_process(
            &executable,
            &step.args,
            &SandboxPolicy {
                roots: vec![record.path.clone()],
                read_only_roots: vec![record.git_common_dir.clone()],
                temporary_directory: temporary.path().to_path_buf(),
                network_allowed: step.network,
                hide_other_processes: true,
            },
        )
        .map_err(|_| SetupError::SandboxUnavailable)?;
    let mut command = tokio::process::Command::new(prepared.program);
    command
        .args(prepared.arguments)
        .current_dir(&cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    crate::run_environment::apply_tokio(&mut command);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.as_std_mut().process_group(0);
    }
    let mut child = command
        .spawn()
        .map_err(|_| SetupError::StartFailed(step.name.clone()))?;
    let _guard = ProcessGroupGuard::new(child.id());
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| SetupError::StartFailed(step.name.clone()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| SetupError::StartFailed(step.name.clone()))?;
    let outcome = tokio::time::timeout(STEP_TIMEOUT, async move {
        let stdout = read_bounded(stdout);
        let stderr = read_bounded(stderr);
        let (stdout, stderr, status) = tokio::join!(stdout, stderr, child.wait());
        (stdout, stderr, status)
    })
    .await
    .map_err(|_| SetupError::Timeout(step.name.clone()))?;
    let stdout = outcome
        .0
        .map_err(|_| SetupError::StepFailed(step.name.clone()))?;
    let stderr = outcome
        .1
        .map_err(|_| SetupError::StepFailed(step.name.clone()))?;
    if stdout || stderr {
        return Err(SetupError::OutputTooLarge(step.name.clone()));
    }
    let status = outcome
        .2
        .map_err(|_| SetupError::StepFailed(step.name.clone()))?;
    if !status.success() {
        return Err(SetupError::StepFailed(step.name.clone()));
    }
    Ok(())
}

fn read_source(path: &Path) -> Result<Vec<u8>, SetupError> {
    let metadata = std::fs::symlink_metadata(path).map_err(|_| SetupError::SourceRead)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(SetupError::UnsafeSource);
    }
    if metadata.len() > MAX_SOURCE_BYTES {
        return Err(SetupError::SourceTooLarge);
    }
    std::fs::read(path).map_err(|_| SetupError::SourceRead)
}

fn parse_native(bytes: &[u8]) -> Result<Vec<SetupStep>, SetupError> {
    let text =
        std::str::from_utf8(bytes).map_err(|_| SetupError::Malformed("non-UTF-8 TOML".into()))?;
    let config: NativeConfig =
        toml::from_str(text).map_err(|error| SetupError::Malformed(error.to_string()))?;
    if config.version != 1 {
        return Err(SetupError::Malformed("native version must be 1".into()));
    }
    let steps = config
        .steps
        .into_iter()
        .map(|step| {
            let sandbox = SandboxMode::from_saved_value(&step.sandbox)
                .map_err(|_| SetupError::InvalidStep(step.name.clone()))?;
            validate_step(SetupStep {
                name: step.name,
                program: step.program,
                args: step.args,
                cwd: step.cwd,
                sandbox,
                network: step.network,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    validate_steps(steps)
}

fn parse_cursor(bytes: &[u8]) -> Result<Vec<SetupStep>, SetupError> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|error| SetupError::Malformed(error.to_string()))?;
    let object = value
        .as_object()
        .ok_or_else(|| SetupError::Malformed("Cursor setup must be an object".into()))?;
    let accepted = ["version", "setup", "setupCommands", "commands"];
    if object.keys().any(|key| !accepted.contains(&key.as_str())) {
        return Err(SetupError::Malformed(
            "Cursor setup contains unsupported fields".into(),
        ));
    }
    let candidates = ["setup", "setupCommands", "commands"]
        .into_iter()
        .filter_map(|key| object.get(key))
        .collect::<Vec<_>>();
    if candidates.len() != 1 {
        return Err(SetupError::Malformed(
            "Cursor setup must contain exactly one command list".into(),
        ));
    }
    let commands = match candidates[0] {
        serde_json::Value::String(command) => {
            vec![("setup".to_string(), command.clone(), PathBuf::from("."))]
        }
        serde_json::Value::Array(values) => values
            .iter()
            .enumerate()
            .map(|(index, value)| match value {
                serde_json::Value::String(command) => Ok((
                    format!("setup-{}", index + 1),
                    command.clone(),
                    PathBuf::from("."),
                )),
                serde_json::Value::Object(object) => {
                    if object
                        .keys()
                        .any(|key| !matches!(key.as_str(), "name" | "command" | "cwd"))
                    {
                        return Err(SetupError::Malformed(
                            "Cursor command contains unsupported fields".into(),
                        ));
                    }
                    let command = object
                        .get("command")
                        .and_then(serde_json::Value::as_str)
                        .ok_or_else(|| {
                            SetupError::Malformed("Cursor command is missing command".into())
                        })?;
                    let name = object
                        .get("name")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_string)
                        .unwrap_or_else(|| format!("setup-{}", index + 1));
                    let cwd = object
                        .get("cwd")
                        .and_then(serde_json::Value::as_str)
                        .map(PathBuf::from)
                        .unwrap_or_else(|| PathBuf::from("."));
                    Ok((name, command.to_string(), cwd))
                }
                _ => Err(SetupError::Malformed(
                    "Cursor command must be a string or object".into(),
                )),
            })
            .collect::<Result<Vec<_>, _>>()?,
        _ => {
            return Err(SetupError::Malformed(
                "Cursor command list has an unsupported type".into(),
            ))
        }
    };
    let steps = commands
        .into_iter()
        .map(|(name, command, cwd)| {
            validate_step(SetupStep {
                name,
                program: "/bin/sh".into(),
                args: vec!["-c".into(), command],
                cwd,
                sandbox: SandboxMode::Required,
                network: false,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    validate_steps(steps)
}

fn validate_steps(steps: Vec<SetupStep>) -> Result<Vec<SetupStep>, SetupError> {
    if steps.is_empty() {
        return Err(SetupError::Malformed(
            "setup must contain at least one step".into(),
        ));
    }
    if steps.len() > MAX_STEPS {
        return Err(SetupError::TooManySteps);
    }
    Ok(steps)
}

fn validate_step(step: SetupStep) -> Result<SetupStep, SetupError> {
    let text_valid = |value: &str, max: usize| {
        !value.is_empty()
            && value.len() <= max
            && !value
                .as_bytes()
                .iter()
                .any(|byte| matches!(byte, 0 | b'\n' | b'\r'))
    };
    if !text_valid(&step.name, 64)
        || !text_valid(&step.program, 256)
        || step.args.len() > MAX_ARGS
        || step
            .args
            .iter()
            .any(|arg| !text_valid(arg, MAX_ARGUMENT_BYTES))
        || step.cwd.as_os_str().is_empty()
        || step.cwd.is_absolute()
        || step.cwd.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(SetupError::InvalidStep(step.name));
    }
    Ok(step)
}

async fn repository_root(path: &Path) -> Result<PathBuf, SetupError> {
    let root = git_text(path, &["rev-parse", "--show-toplevel"]).await?;
    std::fs::canonicalize(root.trim()).map_err(|_| SetupError::Git)
}

async fn git_text(cwd: &Path, args: &[&str]) -> Result<String, SetupError> {
    let mut command = tokio::process::Command::new("git");
    command
        .args(args)
        .current_dir(cwd)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_PAGER", "cat")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    crate::run_environment::apply_tokio(&mut command);
    let output = tokio::time::timeout(Duration::from_secs(15), command.output())
        .await
        .map_err(|_| SetupError::Git)?
        .map_err(|_| SetupError::Git)?;
    if !output.status.success() || output.stdout.len() > 64 * 1024 {
        return Err(SetupError::Git);
    }
    String::from_utf8(output.stdout).map_err(|_| SetupError::Git)
}

fn resolve_executable(program: &str, cwd: &Path) -> Option<PathBuf> {
    let path = Path::new(program);
    if path.is_absolute() {
        return path.is_file().then(|| path.canonicalize().ok()).flatten();
    }
    if path.components().count() > 1 {
        let candidate = cwd.join(path);
        return candidate
            .is_file()
            .then(|| candidate.canonicalize().ok())
            .flatten();
    }
    let search = crate::run_environment::var_os("PATH")?;
    std::env::split_paths(&search).find_map(|directory| {
        let candidate = directory.join(path);
        candidate
            .is_file()
            .then(|| candidate.canonicalize().ok())
            .flatten()
    })
}

async fn read_bounded(reader: impl tokio::io::AsyncRead + Unpin) -> std::io::Result<bool> {
    let mut bytes = Vec::new();
    reader
        .take((MAX_STEP_OUTPUT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .await?;
    Ok(bytes.len() > MAX_STEP_OUTPUT_BYTES)
}

fn short_oid(value: &str) -> &str {
    value.get(..12).unwrap_or(value)
}

struct ProcessGroupGuard(Option<u32>);

impl ProcessGroupGuard {
    fn new(process_id: Option<u32>) -> Self {
        Self(process_id)
    }
}

impl Drop for ProcessGroupGuard {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(process_id) = self.0.and_then(|id| i32::try_from(id).ok()) {
            let _ = nix::sys::signal::killpg(
                nix::unistd::Pid::from_raw(process_id),
                nix::sys::signal::Signal::SIGKILL,
            );
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum RepoOutputFormat {
    Text,
    Json,
}

#[derive(Debug, Parser)]
#[command(
    name = "ca repo",
    about = "Inspect local repository state without network access"
)]
struct RawRepoArgs {
    #[command(subcommand)]
    command: RawRepoCommand,
}

#[derive(Debug, Subcommand)]
enum RawRepoCommand {
    /// Print stable local repository identity and subsystem readiness
    Info {
        #[arg(long, value_enum, default_value_t = RepoOutputFormat::Text)]
        output_format: RepoOutputFormat,
    },
    /// Print stable local Git change counts and worktree state
    Status {
        #[arg(long, value_enum, default_value_t = RepoOutputFormat::Text)]
        output_format: RepoOutputFormat,
    },
    /// Preview the repository-defined worktree setup and exact digest
    WorktreeSetup {
        #[arg(long, default_value = "launch")]
        name: String,
        #[arg(long, default_value = "HEAD")]
        base: String,
        #[arg(long, value_enum, default_value_t = RepoOutputFormat::Text)]
        output_format: RepoOutputFormat,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepoCli {
    Info {
        json: bool,
    },
    Status {
        json: bool,
    },
    WorktreeSetup {
        name: String,
        base: String,
        json: bool,
    },
}

pub fn parse_args<I, T>(args: I) -> Result<RepoCli, clap::Error>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let raw = RawRepoArgs::try_parse_from(args)?;
    Ok(match raw.command {
        RawRepoCommand::Info { output_format } => RepoCli::Info {
            json: output_format == RepoOutputFormat::Json,
        },
        RawRepoCommand::Status { output_format } => RepoCli::Status {
            json: output_format == RepoOutputFormat::Json,
        },
        RawRepoCommand::WorktreeSetup {
            name,
            base,
            output_format,
        } => RepoCli::WorktreeSetup {
            name,
            base,
            json: output_format == RepoOutputFormat::Json,
        },
    })
}

pub async fn run_cli(cli: RepoCli) -> i32 {
    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        Err(_) => {
            eprintln!("ca repo: current directory is unavailable");
            return 2;
        }
    };
    let workspace = match WorkspaceContext::discover(&cwd) {
        Ok(workspace) => workspace,
        Err(error) => {
            eprintln!("ca repo: {error}");
            return 2;
        }
    };
    match cli {
        RepoCli::Info { json } => match collect_repo_info(&workspace).await {
            Ok(info) if json => print_json(&info),
            Ok(info) => {
                println!("{}", info.render_text());
                0
            }
            Err(error) => {
                eprintln!("ca repo info: {error}");
                2
            }
        },
        RepoCli::Status { json } => match collect_repo_status(&workspace).await {
            Ok(status) if json => print_json(&status),
            Ok(status) => {
                println!("{}", status.render_text());
                0
            }
            Err(error) => {
                eprintln!("ca repo status: {error}");
                2
            }
        },
        RepoCli::WorktreeSetup { name, base, json } => {
            run_worktree_setup_preview(&workspace, &name, &base, json).await
        }
    }
}

async fn run_worktree_setup_preview(
    workspace: &WorkspaceContext,
    name: &str,
    base: &str,
    json: bool,
) -> i32 {
    match inspect(workspace, name, Some(base)).await {
        Ok(Some(preview)) if json => match serde_json::to_string_pretty(&preview.json_value()) {
            Ok(output) => println!("{output}"),
            Err(_) => return 1,
        },
        Ok(Some(preview)) => println!("{}", preview.render_text()),
        Ok(None) if json => println!(
            "{}",
            serde_json::json!({
                "schema": "ca.worktree-setup-preview.v1",
                "configured": false,
                "worktree_name": name,
                "base_ref": base,
            })
        ),
        Ok(None) => println!("No .ca/worktrees.toml or .cursor/worktrees.json setup is configured"),
        Err(error) => {
            eprintln!("ca repo worktree-setup: {error}");
            return 2;
        }
    }
    0
}

fn print_json(value: &impl Serialize) -> i32 {
    match serde_json::to_string_pretty(value) {
        Ok(output) => {
            println!("{output}");
            0
        }
        Err(_) => 1,
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct RepoInfoV1 {
    schema: &'static str,
    repository_root: PathBuf,
    workspace_roots: Vec<PathBuf>,
    branch: Option<String>,
    head: String,
    linked_worktree: bool,
    readiness: RepoReadinessV1,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct RepoReadinessV1 {
    git: bool,
    native_worktree_setup: bool,
    cursor_worktree_setup: bool,
    envrc_detected: bool,
    rules_detected: bool,
    commands_detected: bool,
}

impl RepoInfoV1 {
    fn render_text(&self) -> String {
        format!(
            "Repository: {}\nBranch: {}\nHEAD: {}\nLinked worktree: {}\nWorkspace roots: {}\nReadiness: git=yes native_setup={} cursor_setup={} envrc={} rules={} commands={}",
            self.repository_root.display(),
            self.branch.as_deref().unwrap_or("(detached)"),
            self.head,
            if self.linked_worktree { "yes" } else { "no" },
            self.workspace_roots.iter().map(|path| path.display().to_string()).collect::<Vec<_>>().join(", "),
            yes_no(self.readiness.native_worktree_setup),
            yes_no(self.readiness.cursor_worktree_setup),
            yes_no(self.readiness.envrc_detected),
            yes_no(self.readiness.rules_detected),
            yes_no(self.readiness.commands_detected),
        )
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct RepoStatusV1 {
    schema: &'static str,
    repository_root: PathBuf,
    branch: Option<String>,
    head: String,
    staged: usize,
    unstaged: usize,
    untracked: usize,
    conflicted: usize,
    linked_worktree: bool,
    clean: bool,
}

impl RepoStatusV1 {
    fn render_text(&self) -> String {
        format!(
            "Repository: {}\nBranch: {}\nHEAD: {}\nStaged: {}\nUnstaged: {}\nUntracked: {}\nConflicted: {}\nLinked worktree: {}\nClean: {}",
            self.repository_root.display(),
            self.branch.as_deref().unwrap_or("(detached)"),
            self.head,
            self.staged,
            self.unstaged,
            self.untracked,
            self.conflicted,
            yes_no(self.linked_worktree),
            yes_no(self.clean),
        )
    }
}

async fn collect_repo_info(workspace: &WorkspaceContext) -> Result<RepoInfoV1, SetupError> {
    let root = repository_root(workspace.cwd()).await?;
    let branch = git_text_optional(&root, &["symbolic-ref", "--short", "-q", "HEAD"]).await?;
    let head = git_text(&root, &["rev-parse", "HEAD"])
        .await?
        .trim()
        .to_string();
    let git_dir = git_text(&root, &["rev-parse", "--absolute-git-dir"]).await?;
    let readiness = RepoReadinessV1 {
        git: true,
        native_worktree_setup: root.join(NATIVE_RELATIVE_PATH).is_file(),
        cursor_worktree_setup: root.join(CURSOR_RELATIVE_PATH).is_file(),
        envrc_detected: root.join(".envrc").is_file(),
        rules_detected: root.join(".ca/rules").is_dir(),
        commands_detected: root.join(".ca/commands").is_dir(),
    };
    Ok(RepoInfoV1 {
        schema: "ca.repo-info.v1",
        repository_root: root,
        workspace_roots: workspace
            .roots()
            .iter()
            .map(|item| item.path.clone())
            .collect(),
        branch: branch
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
        head,
        linked_worktree: git_dir.replace('\\', "/").contains("/worktrees/"),
        readiness,
    })
}

async fn collect_repo_status(workspace: &WorkspaceContext) -> Result<RepoStatusV1, SetupError> {
    let info = collect_repo_info(workspace).await?;
    let status = git_text(
        &info.repository_root,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
    )
    .await?;
    let mut staged = 0usize;
    let mut unstaged = 0usize;
    let mut untracked = 0usize;
    let mut conflicted = 0usize;
    let mut records = status.split('\0').filter(|record| !record.is_empty());
    while let Some(record) = records.next() {
        let bytes = record.as_bytes();
        if bytes.len() < 3 || bytes[2] != b' ' {
            return Err(SetupError::Git);
        }
        let x = bytes[0];
        let y = bytes[1];
        if x == b'?' && y == b'?' {
            untracked = untracked.saturating_add(1);
        } else if x == b'U' || y == b'U' || (x == b'A' && y == b'A') || (x == b'D' && y == b'D') {
            conflicted = conflicted.saturating_add(1);
        } else {
            staged = staged.saturating_add(usize::from(x != b' '));
            unstaged = unstaged.saturating_add(usize::from(y != b' '));
        }
        if matches!(x, b'R' | b'C') && records.next().is_none() {
            return Err(SetupError::Git);
        }
    }
    Ok(RepoStatusV1 {
        schema: "ca.repo-status.v1",
        repository_root: info.repository_root,
        branch: info.branch,
        head: info.head,
        staged,
        unstaged,
        untracked,
        conflicted,
        linked_worktree: info.linked_worktree,
        clean: staged == 0 && unstaged == 0 && untracked == 0 && conflicted == 0,
    })
}

async fn git_text_optional(cwd: &Path, args: &[&str]) -> Result<Option<String>, SetupError> {
    let mut command = tokio::process::Command::new("git");
    command
        .args(args)
        .current_dir(cwd)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_PAGER", "cat")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    crate::run_environment::apply_tokio(&mut command);
    let output = tokio::time::timeout(Duration::from_secs(15), command.output())
        .await
        .map_err(|_| SetupError::Git)?
        .map_err(|_| SetupError::Git)?;
    if output.stdout.len() > 64 * 1024 {
        return Err(SetupError::Git);
    }
    if !output.status.success() {
        return Ok(None);
    }
    String::from_utf8(output.stdout)
        .map(Some)
        .map_err(|_| SetupError::Git)
}

fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command as StdCommand;

    fn git(cwd: &Path, args: &[&str]) {
        let output = StdCommand::new("git")
            .args(args)
            .current_dir(cwd)
            .env("GIT_AUTHOR_NAME", "CA Test")
            .env("GIT_AUTHOR_EMAIL", "ca@example.invalid")
            .env("GIT_COMMITTER_NAME", "CA Test")
            .env("GIT_COMMITTER_EMAIL", "ca@example.invalid")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn repository() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        git(root.path(), &["init", "-q"]);
        std::fs::write(root.path().join("tracked.txt"), "base\n").unwrap();
        git(root.path(), &["add", "tracked.txt"]);
        git(root.path(), &["commit", "-q", "-m", "base"]);
        root
    }

    fn native(root: &Path, body: &str) {
        std::fs::create_dir_all(root.join(".ca")).unwrap();
        std::fs::write(root.join(NATIVE_RELATIVE_PATH), body).unwrap();
    }

    #[tokio::test]
    async fn native_configuration_wins_and_digest_binds_source_and_identity() {
        let repository = repository();
        native(
            repository.path(),
            "version = 1\n[[steps]]\nname = \"native\"\nprogram = \"true\"\n",
        );
        std::fs::create_dir_all(repository.path().join(".cursor")).unwrap();
        std::fs::write(
            repository.path().join(CURSOR_RELATIVE_PATH),
            r#"{"setup":["false"]}"#,
        )
        .unwrap();
        let workspace = WorkspaceContext::discover(repository.path()).unwrap();
        let first = inspect(&workspace, "launch", Some("HEAD"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(first.source_kind, SetupSourceKind::Native);
        assert_eq!(first.steps[0].program, "true");
        let other_name = inspect(&workspace, "other", Some("HEAD"))
            .await
            .unwrap()
            .unwrap();
        assert_ne!(first.digest, other_name.digest);
        native(
            repository.path(),
            "version = 1\n[[steps]]\nname = \"changed\"\nprogram = \"true\"\n",
        );
        let changed = inspect(&workspace, "launch", Some("HEAD"))
            .await
            .unwrap()
            .unwrap();
        assert_ne!(first.digest, changed.digest);
    }

    #[tokio::test]
    async fn compatibility_is_required_sandboxed_and_network_denied() {
        let repository = repository();
        std::fs::create_dir_all(repository.path().join(".cursor")).unwrap();
        std::fs::write(
            repository.path().join(CURSOR_RELATIVE_PATH),
            r#"{"setup":[{"name":"compat","command":"printf ok","cwd":"."}]}"#,
        )
        .unwrap();
        let preview = inspect(
            &WorkspaceContext::discover(repository.path()).unwrap(),
            "launch",
            None,
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(preview.source_kind, SetupSourceKind::CursorCompatibility);
        assert_eq!(preview.steps[0].sandbox, SandboxMode::Required);
        assert!(!preview.steps[0].network);
        assert!(!preview.requires_process_trust());
    }

    #[tokio::test]
    async fn malformed_native_never_falls_back_and_no_setup_is_none() {
        let repository = repository();
        let workspace = WorkspaceContext::discover(repository.path()).unwrap();
        assert!(inspect(&workspace, "launch", None).await.unwrap().is_none());
        native(repository.path(), "version = 2\nsteps = []\n");
        std::fs::create_dir_all(repository.path().join(".cursor")).unwrap();
        std::fs::write(
            repository.path().join(CURSOR_RELATIVE_PATH),
            r#"{"setup":["true"]}"#,
        )
        .unwrap();
        assert!(inspect(&workspace, "launch", None).await.is_err());
    }

    #[tokio::test]
    async fn failed_setup_preserves_registered_worktree() {
        let repository = repository();
        native(
            repository.path(),
            "version = 1\n[[steps]]\nname = \"fail\"\nprogram = \"false\"\nsandbox = \"policy-only\"\n",
        );
        let workspace = WorkspaceContext::discover(repository.path()).unwrap();
        let preview = inspect(&workspace, "launch", None).await.unwrap().unwrap();
        let data = tempfile::tempdir().unwrap();
        let store = crate::worktree::WorktreeStore::new(data.path().join("worktrees"), true);
        let record = store
            .create(repository.path(), "launch", None)
            .await
            .unwrap();
        assert!(execute(&record, &preview).await.is_err());
        assert!(record.path.is_dir());
        assert_eq!(store.record("launch").unwrap().id, record.id);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn symlinked_and_oversized_sources_fail_closed() {
        use std::os::unix::fs::symlink;

        let repository = repository();
        std::fs::create_dir_all(repository.path().join(".ca")).unwrap();
        let outside = repository.path().join("outside.toml");
        std::fs::write(
            &outside,
            "version = 1\n[[steps]]\nname = \"ok\"\nprogram = \"true\"\n",
        )
        .unwrap();
        symlink(&outside, repository.path().join(NATIVE_RELATIVE_PATH)).unwrap();
        let workspace = WorkspaceContext::discover(repository.path()).unwrap();
        assert!(matches!(
            inspect(&workspace, "launch", None).await,
            Err(SetupError::UnsafeSource)
        ));

        std::fs::remove_file(repository.path().join(NATIVE_RELATIVE_PATH)).unwrap();
        std::fs::write(
            repository.path().join(NATIVE_RELATIVE_PATH),
            vec![b'x'; MAX_SOURCE_BYTES as usize + 1],
        )
        .unwrap();
        assert!(matches!(
            inspect(&workspace, "launch", None).await,
            Err(SetupError::SourceTooLarge)
        ));
    }

    #[tokio::test]
    async fn traversal_and_step_count_are_rejected() {
        let repository = repository();
        let workspace = WorkspaceContext::discover(repository.path()).unwrap();
        native(
            repository.path(),
            "version = 1\n[[steps]]\nname = \"escape\"\nprogram = \"true\"\ncwd = \"../outside\"\n",
        );
        assert!(matches!(
            inspect(&workspace, "launch", None).await,
            Err(SetupError::InvalidStep(_))
        ));

        let mut body = "version = 1\n".to_string();
        for index in 0..=MAX_STEPS {
            body.push_str(&format!(
                "[[steps]]\nname = \"step-{index}\"\nprogram = \"true\"\n"
            ));
        }
        native(repository.path(), &body);
        assert!(matches!(
            inspect(&workspace, "launch", None).await,
            Err(SetupError::TooManySteps)
        ));
    }

    #[tokio::test]
    async fn execution_rejects_source_drift_and_preserves_the_worktree() {
        let repository = repository();
        native(
            repository.path(),
            "version = 1\n[[steps]]\nname = \"first\"\nprogram = \"true\"\nsandbox = \"policy-only\"\n",
        );
        let workspace = WorkspaceContext::discover(repository.path()).unwrap();
        let preview = inspect(&workspace, "launch", None).await.unwrap().unwrap();
        let data = tempfile::tempdir().unwrap();
        let store = crate::worktree::WorktreeStore::new(data.path().join("worktrees"), true);
        let record = store
            .create(repository.path(), "launch", None)
            .await
            .unwrap();
        native(
            repository.path(),
            "version = 1\n[[steps]]\nname = \"changed\"\nprogram = \"true\"\nsandbox = \"policy-only\"\n",
        );
        assert!(matches!(
            execute(&record, &preview).await,
            Err(SetupError::Drift)
        ));
        assert!(record.path.is_dir());
        assert_eq!(store.record("launch").unwrap().id, record.id);
    }

    #[tokio::test]
    async fn successful_setup_runs_in_the_created_worktree() {
        let repository = repository();
        native(
            repository.path(),
            "version = 1\n[[steps]]\nname = \"marker\"\nprogram = \"/usr/bin/touch\"\nargs = [\"setup-ok\"]\nsandbox = \"policy-only\"\n",
        );
        let workspace = WorkspaceContext::discover(repository.path()).unwrap();
        let preview = inspect(&workspace, "launch", None).await.unwrap().unwrap();
        let data = tempfile::tempdir().unwrap();
        let store = crate::worktree::WorktreeStore::new(data.path().join("worktrees"), true);
        let record = store
            .create(repository.path(), "launch", None)
            .await
            .unwrap();
        let receipt = execute(&record, &preview).await.unwrap();
        assert_eq!(receipt.digest, preview.digest);
        assert_eq!(receipt.completed_steps, 1);
        assert!(record.path.join("setup-ok").is_file());
    }

    #[tokio::test]
    async fn repo_info_and_status_have_stable_local_schemas_and_counts() {
        let repository = repository();
        std::fs::create_dir_all(repository.path().join(".ca/rules")).unwrap();
        std::fs::write(repository.path().join("tracked.txt"), "changed\n").unwrap();
        std::fs::write(repository.path().join("untracked.txt"), "new\n").unwrap();
        let workspace = WorkspaceContext::discover(repository.path()).unwrap();
        let info = collect_repo_info(&workspace).await.unwrap();
        assert_eq!(info.schema, "ca.repo-info.v1");
        assert_eq!(info.head.len(), 40);
        assert!(info.readiness.git);
        assert!(info.readiness.rules_detected);
        let status = collect_repo_status(&workspace).await.unwrap();
        assert_eq!(status.schema, "ca.repo-status.v1");
        assert_eq!(status.unstaged, 1);
        assert_eq!(status.untracked, 1);
        assert!(!status.clean);
        assert_eq!(
            serde_json::to_value(&status).unwrap()["schema"],
            "ca.repo-status.v1"
        );
    }
}
