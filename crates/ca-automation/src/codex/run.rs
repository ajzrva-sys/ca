//! Codex process runner.
//!
//! Owns spawning `codex exec`, writing prompt/run artifacts, stdin prompt
//! delivery, timeout handling, and model-call telemetry. The app conductor still
//! owns role routing and typed response parsing.

use anyhow::{bail, Context, Result};
use chrono::Utc;
use serde::Serialize;
use serde_json::{json, Value};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStderr, ChildStdout, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::cancellation::{ActiveChildProcessGroup, CancellationToken};
use crate::domain::{
    classify_tool_failure, OrchestratorEvent, OrchestratorEventKind, ToolFailureKind,
    ToolHealthRecord, ToolHealthStatus,
};
use crate::paths::Paths;

const TERMINATION_GRACE: Duration = Duration::from_secs(2);
const PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Debug, Clone)]
pub(crate) struct CodexRoleConfig {
    pub(crate) model: String,
    pub(crate) effort: String,
    pub(crate) sandbox: String,
    pub(crate) approval_policy: String,
    pub(crate) web_search: String,
    pub(crate) timeout_seconds: u64,
}

pub(crate) struct CodexRunRequest<'a> {
    pub(crate) role_name: &'a str,
    pub(crate) role: CodexRoleConfig,
    pub(crate) task_id: Option<&'a str>,
    pub(crate) prompt: &'a str,
    pub(crate) schema_path: &'a Path,
    pub(crate) live_output: bool,
    pub(crate) cancellation: CancellationToken,
    pub(crate) hooks: CodexRunHooks<'a>,
}

#[derive(Default)]
#[allow(clippy::type_complexity)]
pub(crate) struct CodexRunHooks<'a> {
    pub(crate) on_start: Option<Box<dyn FnOnce(&CodexRunStart<'_>) + 'a>>,
    pub(crate) stdout: Option<Box<dyn FnOnce(ChildStdout, PathBuf) -> thread::JoinHandle<()> + 'a>>,
    pub(crate) stderr: Option<Box<dyn FnOnce(ChildStderr, PathBuf) -> thread::JoinHandle<()> + 'a>>,
    pub(crate) on_stream_end: Option<Box<dyn FnOnce() + 'a>>,
    pub(crate) on_success: Option<Box<dyn FnOnce(Duration) + 'a>>,
}

pub(crate) struct CodexRunStart<'a> {
    pub(crate) prompt_tokens: usize,
    pub(crate) stdout_path: &'a Path,
    pub(crate) timeout_seconds: u64,
}

pub(crate) trait CodexRunner {
    fn run_codex(
        &self,
        paths: &Paths,
        codex_bin: &str,
        request: CodexRunRequest<'_>,
    ) -> Result<String>;
}

pub(crate) struct RealCodexRunner;

impl CodexRunner for RealCodexRunner {
    fn run_codex(
        &self,
        paths: &Paths,
        codex_bin: &str,
        request: CodexRunRequest<'_>,
    ) -> Result<String> {
        run_codex_process(paths, codex_bin, request)
    }
}

fn run_codex_process(
    paths: &Paths,
    codex_bin: &str,
    mut request: CodexRunRequest<'_>,
) -> Result<String> {
    let call_reason = ai_call_reason_for_role(request.role_name);
    let _ = append_event(
        paths,
        OrchestratorEventKind::AiEscalated,
        request.task_id,
        json!({
            "role": request.role_name,
            "reason": call_reason,
            "model": request.role.model.clone(),
            "effort": request.role.effort.clone(),
        }),
    );

    fs::create_dir_all(&paths.runs)?;
    let run_id = format!(
        "{}-{}",
        Utc::now().format("%Y%m%dT%H%M%SZ"),
        request.role_name
    );
    let last_message_path = paths.runs.join(format!("{run_id}.last.json"));
    let stdout_path = paths.runs.join(format!("{run_id}.stdout.jsonl"));
    let stderr_path = paths.runs.join(format!("{run_id}.stderr.log"));
    let prompt_path = paths.runs.join(format!("{run_id}.prompt.md"));
    write_text(&prompt_path, request.prompt)?;

    let codex_started = Instant::now();
    let clang_module_cache = paths
        .root
        .join(".build")
        .join("codex-team-caches")
        .join("clang-module-cache");
    let swift_module_cache = paths
        .root
        .join(".build")
        .join("codex-team-caches")
        .join("swift-module-cache");
    fs::create_dir_all(&clang_module_cache)?;
    fs::create_dir_all(&swift_module_cache)?;

    let prompt_tokens = estimate_tokens(request.prompt);
    if let Some(on_start) = request.hooks.on_start.take() {
        on_start(&CodexRunStart {
            prompt_tokens,
            stdout_path: &stdout_path,
            timeout_seconds: request.role.timeout_seconds,
        });
    }

    let mut cmd = Command::new(codex_bin);
    cmd.args(build_codex_args(
        &request.role,
        &paths.root,
        request.schema_path,
        &last_message_path,
    ))
    .current_dir(&paths.root)
    .env("CLANG_MODULE_CACHE_PATH", &clang_module_cache)
    .env("SWIFT_MODULE_CACHE_PATH", &swift_module_cache)
    .stdin(Stdio::piped());

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            cmd.pre_exec(|| {
                extern "C" {
                    fn setpgid(pid: i32, pgid: i32) -> i32;
                }
                if setpgid(0, 0) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }

    if request.live_output {
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    } else {
        let stdout_file = File::create(&stdout_path)?;
        let stderr_file = File::create(&stderr_path)?;
        cmd.stdout(Stdio::from(stdout_file))
            .stderr(Stdio::from(stderr_file));
    }

    let mut child = cmd
        .spawn()
        .with_context(|| format!("failed to launch {codex_bin}"))?;
    let child_id = child.id();
    let mut active_child = match ActiveChildProcessGroup::register(paths, child_id) {
        Ok(registration) => registration,
        Err(error) => {
            terminate_process_group(&mut child, child_id, TERMINATION_GRACE);
            return Err(error).context("publish Codex child process group for MCP supervision");
        }
    };

    let stdout_join = if request.live_output {
        child.stdout.take().and_then(|stdout| {
            request
                .hooks
                .stdout
                .take()
                .map(|hook| hook(stdout, stdout_path.clone()))
        })
    } else {
        None
    };
    let stderr_join = if request.live_output {
        child.stderr.take().and_then(|stderr| {
            request
                .hooks
                .stderr
                .take()
                .map(|hook| hook(stderr, stderr_path.clone()))
        })
    } else {
        None
    };

    let prompt = request.prompt.to_string();
    let stdin_join = child
        .stdin
        .take()
        .map(|mut stdin| thread::spawn(move || stdin.write_all(prompt.as_bytes())));

    let timeout = Duration::from_secs(request.role.timeout_seconds);
    let status = loop {
        if request.cancellation.is_cancelled() {
            terminate_process_group(&mut child, child_id, TERMINATION_GRACE);
            let _ = join_stdin_writer(stdin_join);
            join_stream(stdout_join);
            join_stream(stderr_join);
            if let Some(on_stream_end) = request.hooks.on_stream_end.take() {
                on_stream_end();
            }
            let _ = append_model_call_record(
                paths,
                &ModelCallRecord {
                    timestamp: Utc::now().to_rfc3339(),
                    role: request.role_name.to_string(),
                    task_id: request.task_id.map(str::to_string),
                    model: request.role.model.clone(),
                    effort: request.role.effort.clone(),
                    prompt_token_estimate: prompt_tokens,
                    output_char_count: 0,
                    skipped_by_rust: false,
                    skip_or_call_reason: "codex role cancelled".to_string(),
                    tool_failure_kind: Some(ToolFailureKind::Cancelled),
                    tool_failure_excerpt: Some("codex role cancelled".to_string()),
                    exit_code: None,
                    timed_out: false,
                },
            );
            bail!("codex {} was cancelled", request.role_name);
        }
        if codex_started.elapsed() >= timeout {
            terminate_process_group(&mut child, child_id, TERMINATION_GRACE);
            let _ = join_stdin_writer(stdin_join);
            join_stream(stdout_join);
            join_stream(stderr_join);
            if let Some(on_stream_end) = request.hooks.on_stream_end.take() {
                on_stream_end();
            }
            let _ = append_model_call_record(
                paths,
                &ModelCallRecord {
                    timestamp: Utc::now().to_rfc3339(),
                    role: request.role_name.to_string(),
                    task_id: request.task_id.map(str::to_string),
                    model: request.role.model.clone(),
                    effort: request.role.effort.clone(),
                    prompt_token_estimate: prompt_tokens,
                    output_char_count: 0,
                    skipped_by_rust: false,
                    skip_or_call_reason: format!(
                        "codex timed out after {} seconds",
                        request.role.timeout_seconds
                    ),
                    tool_failure_kind: Some(ToolFailureKind::Timeout),
                    tool_failure_excerpt: Some(format!(
                        "codex timed out after {} seconds",
                        request.role.timeout_seconds
                    )),
                    exit_code: None,
                    timed_out: true,
                },
            );
            let _ = append_tool_health_record(
                paths,
                &ToolHealthRecord {
                    observed_at: Utc::now().to_rfc3339(),
                    role: request.role_name.to_string(),
                    task_id: request.task_id.map(str::to_string),
                    model: request.role.model.clone(),
                    execution_engine: Some("codex".to_string()),
                    route_provider: None,
                    status: ToolHealthStatus::Failure,
                    failure_kind: Some(ToolFailureKind::Timeout),
                    exit_code: None,
                    timed_out: true,
                    excerpt: format!(
                        "codex timed out after {} seconds",
                        request.role.timeout_seconds
                    ),
                },
            );
            bail!(
                "codex {} timed out after {} seconds",
                request.role_name,
                request.role.timeout_seconds
            );
        }
        if let Some(status) = child.try_wait()? {
            break status;
        }
        thread::sleep(PROCESS_POLL_INTERVAL);
    };

    // A successful direct `codex` exit does not prove that provider/tool
    // descendants are gone. They can retain stdout/stderr/stdin pipe ends and
    // make the joins below block forever. Preserve the captured direct-child
    // status, but always drain the entire dedicated process group before
    // joining any I/O thread or clearing MCP's active-child evidence.
    terminate_process_group(&mut child, child_id, TERMINATION_GRACE);
    let stdin_result = join_stdin_writer(stdin_join);
    if let Some(join) = stdout_join {
        let _ = join.join();
    }
    if let Some(join) = stderr_join {
        let _ = join.join();
    }
    if let Some(on_stream_end) = request.hooks.on_stream_end.take() {
        on_stream_end();
    }
    active_child.clear()?;

    if !status.success() {
        let stderr = read_text_or_empty(&stderr_path);
        let failure_kind = classify_tool_failure(status.code(), false, &stderr);
        let excerpt = sanitized_process_failure_summary(
            request.role_name,
            failure_kind,
            status.code(),
            stderr.len(),
        );
        let _ = append_model_call_record(
            paths,
            &ModelCallRecord {
                timestamp: Utc::now().to_rfc3339(),
                role: request.role_name.to_string(),
                task_id: request.task_id.map(str::to_string),
                model: request.role.model.clone(),
                effort: request.role.effort.clone(),
                prompt_token_estimate: prompt_tokens,
                output_char_count: read_text_or_empty(&last_message_path).chars().count(),
                skipped_by_rust: false,
                skip_or_call_reason: format!("codex exited with {status}"),
                tool_failure_kind: Some(failure_kind),
                tool_failure_excerpt: Some(excerpt.clone()),
                exit_code: status.code(),
                timed_out: false,
            },
        );
        let _ = append_tool_health_record(
            paths,
            &ToolHealthRecord {
                observed_at: Utc::now().to_rfc3339(),
                role: request.role_name.to_string(),
                task_id: request.task_id.map(str::to_string),
                model: request.role.model.clone(),
                execution_engine: Some("codex".to_string()),
                route_provider: None,
                status: ToolHealthStatus::Failure,
                failure_kind: Some(failure_kind),
                exit_code: status.code(),
                timed_out: false,
                excerpt: excerpt.clone(),
            },
        );
        bail!(
            "{}. stderr artifact: {}; stdout artifact: {}",
            excerpt,
            stderr_path.display(),
            stdout_path.display(),
        );
    }

    stdin_result.context("failed to write Codex prompt to stdin")?;

    if let Some(on_success) = request.hooks.on_success.take() {
        on_success(codex_started.elapsed());
    }

    let raw = read_text(&last_message_path).with_context(|| {
        format!(
            "missing output-last-message from codex at {}",
            last_message_path.display()
        )
    })?;
    append_model_call_record(
        paths,
        &ModelCallRecord {
            timestamp: Utc::now().to_rfc3339(),
            role: request.role_name.to_string(),
            task_id: request.task_id.map(str::to_string),
            model: request.role.model.clone(),
            effort: request.role.effort.clone(),
            prompt_token_estimate: prompt_tokens,
            output_char_count: raw.chars().count(),
            skipped_by_rust: false,
            skip_or_call_reason: call_reason.to_string(),
            tool_failure_kind: None,
            tool_failure_excerpt: None,
            exit_code: Some(0),
            timed_out: false,
        },
    )?;
    let _ = append_tool_health_record(
        paths,
        &ToolHealthRecord {
            observed_at: Utc::now().to_rfc3339(),
            role: request.role_name.to_string(),
            task_id: request.task_id.map(str::to_string),
            model: request.role.model.clone(),
            execution_engine: Some("codex".to_string()),
            route_provider: None,
            status: ToolHealthStatus::Success,
            failure_kind: None,
            exit_code: Some(0),
            timed_out: false,
            excerpt: String::new(),
        },
    );
    Ok(raw)
}

fn sanitized_process_failure_summary(
    role_name: &str,
    failure_kind: ToolFailureKind,
    exit_code: Option<i32>,
    stderr_bytes: usize,
) -> String {
    format!(
        "codex {role_name} failed (failure_kind={}, exit_code={}, stderr_bytes={stderr_bytes}); provider payload omitted",
        failure_kind.as_str(),
        exit_code
            .map(|code| code.to_string())
            .unwrap_or_else(|| "signal".to_string())
    )
}

pub(crate) fn build_codex_args(
    role: &CodexRoleConfig,
    cd_path: &Path,
    schema_path: &Path,
    last_message_path: &Path,
) -> Vec<String> {
    vec![
        "exec".to_string(),
        "--cd".to_string(),
        cd_path.display().to_string(),
        "--model".to_string(),
        role.model.clone(),
        "--sandbox".to_string(),
        role.sandbox.clone(),
        "--json".to_string(),
        "--output-schema".to_string(),
        schema_path.display().to_string(),
        "--output-last-message".to_string(),
        last_message_path.display().to_string(),
        "--config".to_string(),
        format!("model_reasoning_effort=\"{}\"", role.effort),
        "--config".to_string(),
        format!("approval_policy=\"{}\"", role.approval_policy),
        "--config".to_string(),
        format!("web_search=\"{}\"", role.web_search),
        "-".to_string(),
    ]
}

fn ai_call_reason_for_role(role_name: &str) -> &'static str {
    match role_name {
        "master" => "humanJudgmentNeeded",
        "planner" => "plannerNeeded",
        "worker" => "patchGenerationNeeded",
        "foreman" | "foreman-review" => "humanJudgmentNeeded",
        _ => "humanJudgmentNeeded",
    }
}

fn append_event(
    paths: &Paths,
    kind: OrchestratorEventKind,
    task_id: Option<&str>,
    payload: Value,
) -> Result<()> {
    fs::create_dir_all(&paths.orch)?;
    let event = OrchestratorEvent {
        timestamp: Utc::now().to_rfc3339(),
        kind,
        task_id: task_id.map(str::to_string),
        payload,
    };
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&paths.events)
        .with_context(|| format!("open event stream {}", paths.events.display()))?;
    writeln!(file, "{}", serde_json::to_string(&event)?)?;
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
struct ModelCallRecord {
    timestamp: String,
    role: String,
    task_id: Option<String>,
    model: String,
    effort: String,
    prompt_token_estimate: usize,
    output_char_count: usize,
    skipped_by_rust: bool,
    skip_or_call_reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_failure_kind: Option<ToolFailureKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_failure_excerpt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    exit_code: Option<i32>,
    #[serde(skip_serializing_if = "is_false")]
    timed_out: bool,
}

fn append_model_call_record(paths: &Paths, record: &ModelCallRecord) -> Result<()> {
    fs::create_dir_all(&paths.orch)?;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&paths.cost)
        .with_context(|| format!("open cost ledger {}", paths.cost.display()))?;
    writeln!(file, "{}", serde_json::to_string(record)?)?;
    Ok(())
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn append_tool_health_record(paths: &Paths, record: &ToolHealthRecord) -> Result<()> {
    fs::create_dir_all(&paths.observations)?;
    let path = paths.observations.join("tool_health.jsonl");
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("open tool health ledger {}", path.display()))?;
    writeln!(file, "{}", serde_json::to_string(record)?)?;
    Ok(())
}

fn estimate_tokens(text: &str) -> usize {
    (text.chars().count() / 4).max(1)
}

fn read_text(path: &Path) -> Result<String> {
    fs::read_to_string(path).with_context(|| format!("read {}", path.display()))
}

fn read_text_or_empty(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_default()
}

fn write_text(path: &Path, text: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, text).with_context(|| format!("write {}", path.display()))
}

fn join_stdin_writer(
    handle: Option<thread::JoinHandle<std::io::Result<()>>>,
) -> std::io::Result<()> {
    match handle {
        Some(handle) => handle
            .join()
            .unwrap_or_else(|_| Err(std::io::Error::other("Codex stdin writer thread panicked"))),
        None => Ok(()),
    }
}

fn join_stream(handle: Option<thread::JoinHandle<()>>) {
    if let Some(handle) = handle {
        let _ = handle.join();
    }
}

fn terminate_process_group(child: &mut Child, child_id: u32, grace: Duration) {
    #[cfg(unix)]
    signal_process_group(child_id, libc::SIGTERM);
    #[cfg(not(unix))]
    {
        let _ = child.kill();
    }
    let mut child_reaped = false;
    let started = Instant::now();
    while started.elapsed() < grace {
        if !child_reaped && matches!(child.try_wait(), Ok(Some(_))) {
            child_reaped = true;
        }
        #[cfg(unix)]
        if child_reaped && !process_group_exists(child_id) {
            return;
        }
        #[cfg(not(unix))]
        if child_reaped {
            return;
        }
        thread::sleep(PROCESS_POLL_INTERVAL);
    }
    #[cfg(unix)]
    signal_process_group(child_id, libc::SIGKILL);
    #[cfg(not(unix))]
    {
        let _ = child.kill();
    }
    if !child_reaped {
        let _ = child.wait();
    }
    #[cfg(unix)]
    {
        let cleanup_started = Instant::now();
        while cleanup_started.elapsed() < grace && process_group_exists(child_id) {
            thread::sleep(PROCESS_POLL_INTERVAL);
        }
    }
}

#[cfg(unix)]
fn signal_process_group(child_id: u32, signal: i32) {
    unsafe {
        libc::kill(-(child_id as i32), signal);
    }
}

#[cfg(unix)]
fn process_group_exists(child_id: u32) -> bool {
    let result = unsafe { libc::kill(-(child_id as i32), 0) };
    if result == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("codex-team-codex-runner-{name}-{nonce}"))
    }

    fn sample_role() -> CodexRoleConfig {
        CodexRoleConfig {
            model: "gpt-test".to_string(),
            effort: "medium".to_string(),
            sandbox: "workspace-write".to_string(),
            approval_policy: "never".to_string(),
            web_search: "disabled".to_string(),
            timeout_seconds: 30,
        }
    }

    #[cfg(unix)]
    fn make_hanging_fake_codex(root: &Path) -> Result<PathBuf> {
        use std::os::unix::fs::PermissionsExt;

        let fake = root.join("fake-codex-hanging");
        let child_pid = root.join("codex-child.pid");
        let grandchild_pid = root.join("codex-grandchild.pid");
        fs::write(
            &fake,
            format!(
                "#!/bin/sh\n\
                 printf '%s\\n' \"$$\" > '{}'\n\
                 sh -c 'trap \"\" TERM; while :; do sleep 1; done' &\n\
                 grandchild=$!\n\
                 printf '%s\\n' \"$grandchild\" > '{}'\n\
                 trap 'exit 0' TERM\n\
                 while :; do sleep 1; done\n",
                child_pid.display(),
                grandchild_pid.display(),
            ),
        )?;
        let mut permissions = fs::metadata(&fake)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&fake, permissions)?;
        Ok(fake)
    }

    #[cfg(unix)]
    fn wait_for_pid(path: &Path) -> Result<u32> {
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(5) {
            if let Ok(text) = fs::read_to_string(path) {
                if let Ok(pid) = text.trim().parse::<u32>() {
                    return Ok(pid);
                }
            }
            thread::sleep(Duration::from_millis(20));
        }
        bail!(
            "timed out waiting for fake process pid at {}",
            path.display()
        )
    }

    #[cfg(unix)]
    fn process_exists(pid: u32) -> bool {
        let result = unsafe { libc::kill(pid as i32, 0) };
        result == 0 || std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
    }

    #[cfg(unix)]
    fn assert_process_exits(pid: u32) {
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(5) {
            if !process_exists(pid) {
                return;
            }
            thread::sleep(Duration::from_millis(20));
        }
        unsafe {
            libc::kill(pid as i32, libc::SIGKILL);
        }
        panic!("fake Codex descendant {pid} survived runner cleanup");
    }

    #[cfg(unix)]
    fn run_request<'a>(
        role: CodexRoleConfig,
        prompt: &'a str,
        schema: &'a Path,
        cancellation: CancellationToken,
    ) -> CodexRunRequest<'a> {
        CodexRunRequest {
            role_name: "worker",
            role,
            task_id: Some("INT-148"),
            prompt,
            schema_path: schema,
            live_output: false,
            cancellation,
            hooks: CodexRunHooks::default(),
        }
    }

    #[test]
    fn build_codex_args_preserves_role_contract_and_stdin_is_external() {
        let args = build_codex_args(
            &sample_role(),
            Path::new("/tmp/repo"),
            Path::new(".orchestrator/schemas/worker.schema.json"),
            Path::new(".orchestrator/runs/run.last.json"),
        );

        assert_eq!(args[0], "exec");
        assert!(args.windows(2).any(|pair| pair == ["--cd", "/tmp/repo"]));
        assert!(args.windows(2).any(|pair| pair == ["--model", "gpt-test"]));
        assert!(args
            .windows(2)
            .any(|pair| pair == ["--sandbox", "workspace-write"]));
        assert!(args.contains(&"--json".to_string()));
        assert!(args.windows(2).any(|pair| pair
            == [
                "--output-schema",
                ".orchestrator/schemas/worker.schema.json"
            ]));
        assert!(args
            .windows(2)
            .any(|pair| { pair == ["--output-last-message", ".orchestrator/runs/run.last.json"] }));
        assert!(args.contains(&"model_reasoning_effort=\"medium\"".to_string()));
        assert!(args.contains(&"approval_policy=\"never\"".to_string()));
        assert!(args.contains(&"web_search=\"disabled\"".to_string()));
        assert_eq!(args.last().map(String::as_str), Some("-"));
    }

    #[cfg(unix)]
    #[test]
    fn codex_nonzero_failure_omits_stderr_and_model_payload_from_error_and_telemetry() -> Result<()>
    {
        use std::os::unix::fs::PermissionsExt;

        const CANARY: &str = "CODEX_PROVIDER_SECRET_CANARY_d41d8c";
        let root = temp_root("sanitized-nonzero");
        fs::create_dir_all(root.join(".orchestrator/schemas"))?;
        let schema = root.join(".orchestrator/schemas/worker.schema.json");
        fs::write(&schema, "{}")?;
        let fake = root.join("fake-codex-failure");
        fs::write(
            &fake,
            format!(
                "#!/bin/sh\n\
                 last=''\n\
                 while [ \"$#\" -gt 0 ]; do\n\
                   if [ \"$1\" = '--output-last-message' ]; then shift; last=\"$1\"; fi\n\
                   shift\n\
                 done\n\
                 printf '%s' '{{\"model_payload\":\"{CANARY}\"}}' > \"$last\"\n\
                 printf '%s\\n' '{CANARY}' >&2\n\
                 exit 17\n"
            ),
        )?;
        let mut permissions = fs::metadata(&fake)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&fake, permissions)?;
        let paths = Paths::new(&root)?;

        let error = run_codex_process(
            &paths,
            fake.to_str().context("fake Codex path was not UTF-8")?,
            run_request(
                sample_role(),
                "safe prompt",
                &schema,
                CancellationToken::default(),
            ),
        )
        .expect_err("fake Codex process must fail");

        let public_error = format!("{error:#}");
        let cost = read_text_or_empty(&paths.cost);
        let tool_health = read_text_or_empty(&paths.observations.join("tool_health.jsonl"));
        let events = read_text_or_empty(&paths.events);
        for persisted in [&public_error, &cost, &tool_health, &events] {
            assert!(
                !persisted.contains(CANARY),
                "provider payload leaked: {persisted}"
            );
        }
        assert!(public_error.contains("failure_kind=process_failed"));
        assert!(public_error.contains("exit_code=17"));
        assert!(cost.contains("\"tool_failure_kind\":\"process_failed\""));
        assert!(tool_health.contains("\"failure_kind\":\"process_failed\""));

        let artifacts = fs::read_dir(&paths.runs)?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .collect::<Vec<_>>();
        assert!(artifacts.iter().any(|path| {
            path.extension().and_then(|ext| ext.to_str()) == Some("log")
                && read_text_or_empty(path).contains(CANARY)
        }));
        assert!(artifacts.iter().any(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(".last.json"))
                && read_text_or_empty(path).contains(CANARY)
        }));

        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn codex_runner_reaps_pipe_holding_descendant_after_successful_child_exit() -> Result<()> {
        use std::io::Read as _;
        use std::os::unix::fs::PermissionsExt as _;

        let root = temp_root("successful-exit-pipe-holder");
        fs::create_dir_all(root.join(".orchestrator/schemas"))?;
        let schema = root.join(".orchestrator/schemas/worker.schema.json");
        fs::write(&schema, "{}")?;
        let descendant_pid_path = root.join("codex-pipe-holder.pid");
        let fake = root.join("fake-codex-success-with-pipe-holder");
        fs::write(
            &fake,
            format!(
                "#!/bin/sh\n\
                 last=''\n\
                 while [ \"$#\" -gt 0 ]; do\n\
                   if [ \"$1\" = '--output-last-message' ]; then shift; last=\"$1\"; fi\n\
                   shift\n\
                 done\n\
                 printf '%s' '{{\"status\":\"done\"}}' > \"$last\"\n\
                 (trap '' TERM HUP; while :; do sleep 30; done) &\n\
                 printf '%s\\n' \"$!\" > '{}'\n\
                 exit 0\n",
                descendant_pid_path.display(),
            ),
        )?;
        let mut permissions = fs::metadata(&fake)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&fake, permissions)?;
        let paths = Paths::new(&root)?;
        let stream_reader = || {
            Box::new(|mut stream: ChildStdout, _path: PathBuf| {
                thread::spawn(move || {
                    let mut sink = Vec::new();
                    let _ = stream.read_to_end(&mut sink);
                })
            }) as Box<dyn FnOnce(ChildStdout, PathBuf) -> thread::JoinHandle<()>>
        };
        let stderr_reader = Box::new(|mut stream: ChildStderr, _path: PathBuf| {
            thread::spawn(move || {
                let mut sink = Vec::new();
                let _ = stream.read_to_end(&mut sink);
            })
        });
        let started = Instant::now();
        let raw = run_codex_process(
            &paths,
            fake.to_str().context("fake Codex path was not UTF-8")?,
            CodexRunRequest {
                role_name: "worker",
                role: sample_role(),
                task_id: Some("INT-148"),
                prompt: "return successfully",
                schema_path: &schema,
                live_output: true,
                cancellation: CancellationToken::default(),
                hooks: CodexRunHooks {
                    stdout: Some(stream_reader()),
                    stderr: Some(stderr_reader),
                    ..CodexRunHooks::default()
                },
            },
        )?;

        assert_eq!(raw, "{\"status\":\"done\"}");
        assert!(
            started.elapsed() < Duration::from_secs(8),
            "runner hung after the direct child exited"
        );
        let descendant_pid = wait_for_pid(&descendant_pid_path)?;
        assert_process_exits(descendant_pid);
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn codex_runner_cancellation_terminates_child_and_grandchild() -> Result<()> {
        let root = temp_root("cancel-process-tree");
        fs::create_dir_all(root.join(".orchestrator/schemas"))?;
        let schema = root.join(".orchestrator/schemas/worker.schema.json");
        fs::write(&schema, "{}")?;
        let fake = make_hanging_fake_codex(&root)?;
        let cancellation = CancellationToken::default();
        let runner_cancellation = cancellation.clone();
        let runner_root = root.clone();
        let runner_fake = fake.clone();
        let runner_schema = schema.clone();
        let handle = thread::spawn(move || {
            let paths = Paths::new(&runner_root)?;
            let mut role = sample_role();
            role.timeout_seconds = 30;
            run_codex_process(
                &paths,
                runner_fake
                    .to_str()
                    .context("fake Codex path was not UTF-8")?,
                run_request(
                    role,
                    "wait for cancellation",
                    &runner_schema,
                    runner_cancellation,
                ),
            )
        });

        let child_pid = wait_for_pid(&root.join("codex-child.pid"));
        let grandchild_pid = wait_for_pid(&root.join("codex-grandchild.pid"));
        cancellation.cancel();
        let result = handle
            .join()
            .map_err(|_| anyhow::anyhow!("Codex cancellation runner thread panicked"))?;
        let child_pid = child_pid?;
        let grandchild_pid = grandchild_pid?;
        assert!(result.unwrap_err().to_string().contains("was cancelled"));
        assert_process_exits(child_pid);
        assert_process_exits(grandchild_pid);
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn codex_runner_timeout_terminates_child_and_grandchild() -> Result<()> {
        let root = temp_root("timeout-process-tree");
        fs::create_dir_all(root.join(".orchestrator/schemas"))?;
        let schema = root.join(".orchestrator/schemas/worker.schema.json");
        fs::write(&schema, "{}")?;
        let fake = make_hanging_fake_codex(&root)?;
        let paths = Paths::new(&root)?;
        let mut role = sample_role();
        // Leave enough startup headroom when the full process-heavy suite is
        // running in parallel; the behavior under test is cleanup after the
        // deadline, not sub-second shell scheduling.
        role.timeout_seconds = 3;
        let large_prompt = "x".repeat(128 * 1024);

        let result = run_codex_process(
            &paths,
            fake.to_str().context("fake Codex path was not UTF-8")?,
            run_request(role, &large_prompt, &schema, CancellationToken::default()),
        );
        let child_pid = wait_for_pid(&root.join("codex-child.pid"))?;
        let grandchild_pid = wait_for_pid(&root.join("codex-grandchild.pid"))?;
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("timed out after 3 seconds"));
        assert_process_exits(child_pid);
        assert_process_exits(grandchild_pid);
        fs::remove_dir_all(root)?;
        Ok(())
    }
}
