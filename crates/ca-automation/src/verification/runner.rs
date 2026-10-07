//! Deterministic command runner.
//!
//! Owns opaque `ValidatedCommand` execution via `Command::new(program).args(args)`,
//! cwd canonical containment, process-group timeouts, output capture, and no-shell
//! semantics. Evidence persistence remains with the app conductor until
//! state/event ownership is extracted.
//!
//! Read when changing timeout kill behavior or spawn safety. Do not read for
//! recipe allowlist parsing (`recipe` / `legacy`) or queue Markdown.

use anyhow::{anyhow, bail, Context, Result};
use std::collections::VecDeque;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use crate::cancellation::{ActiveChildProcessGroup, CancellationToken};
use crate::machine_json;
use crate::paths::Paths;
use crate::verification::ca::{is_ca_verification_command, require_builtin_check, BuiltinCheck};
use crate::verification::isolation::IsolationSession;
use crate::verification::recipe::VerificationCommand;
use crate::verification::validate::{validate_command, ValidationError};

const MAX_VERIFICATION_STREAM_BYTES: u64 = 1024 * 1024;
const MAX_VERIFICATION_AGGREGATE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_VERIFICATION_LINE_BYTES: usize = 256 * 1024;
const VERIFICATION_TAIL_BYTES: usize = 64 * 1024;
const TERMINATION_GRACE: Duration = Duration::from_secs(2);
const PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(25);

#[derive(Debug)]
struct BoundedVerificationOutput {
    tail: Vec<u8>,
    overflowed: bool,
    pipe_open_at_shutdown: bool,
}

fn progress(args: std::fmt::Arguments<'_>) {
    if machine_json::enabled() {
        eprintln!("{args}");
    } else {
        println!("{args}");
    }
}

#[derive(Debug, Clone)]
pub(crate) struct DeterministicCommandResult {
    // Internal execution result before serialization. It carries enough output
    // tail for failure classification but avoids storing full logs in memory.
    pub(crate) command: VerificationCommand,
    pub(crate) exit_code: i32,
    pub(crate) duration_ms: u128,
    pub(crate) category: DeterministicResultCategory,
    pub(crate) evidence: String,
    pub(crate) stdout_tail: String,
    pub(crate) stderr_tail: String,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum DeterministicResultCategory {
    // Keep categories coarse and stable. They are used for routing, cost
    // accounting, and tests; detailed compiler messages belong in evidence tails.
    Passed,
    NoMatches,
    KnownEnvironmentBlocker,
    CommandFailed,
    TimedOut,
}

impl DeterministicResultCategory {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Passed => "passed",
            Self::NoMatches => "noMatches",
            Self::KnownEnvironmentBlocker => "knownEnvironmentBlocker",
            Self::CommandFailed => "commandFailed",
            Self::TimedOut => "timedOut",
        }
    }

    pub(crate) fn is_success_like(self) -> bool {
        matches!(self, Self::Passed | Self::NoMatches)
    }
}

/// Opaque marker that a command has passed app allowlist validation.
#[derive(Debug, Clone)]
pub(crate) struct ValidatedCommand {
    inner: VerificationCommand,
}

impl ValidatedCommand {
    /// Validate a command against an allowlist and return a `ValidatedCommand`.
    ///
    /// The command's program must match an allowed recipe, and its args must be
    /// a prefix of that recipe's args. Returns `Err` if validation fails.
    #[allow(dead_code)]
    pub(crate) fn new(
        command: VerificationCommand,
        allowed_recipes: &[VerificationCommand],
    ) -> Result<Self, ValidationError> {
        validate_command(&command, allowed_recipes)?;
        Ok(Self { inner: command })
    }

    /// Bypass validation. Restricted to crate-internal callers that have
    /// already validated via another path (e.g. recipe expansion).
    pub(crate) fn assume_validated(command: VerificationCommand) -> Self {
        Self { inner: command }
    }

    pub(crate) fn as_command(&self) -> &VerificationCommand {
        &self.inner
    }
}

pub(crate) fn run_deterministic_command(
    paths: &Paths,
    command: &VerificationCommand,
) -> Result<DeterministicCommandResult> {
    run_validated_deterministic_command(paths, &ValidatedCommand::assume_validated(command.clone()))
}

pub(crate) fn run_deterministic_command_with_cancellation(
    paths: &Paths,
    command: &VerificationCommand,
    cancellation: &CancellationToken,
) -> Result<DeterministicCommandResult> {
    anyhow::ensure!(
        !cancellation.is_cancelled(),
        "deterministic command was cancelled before launch"
    );
    anyhow::ensure!(
        crate::verification::registry::is_deterministic_command_structured_allowed(command),
        "command is outside the typed verification registry"
    );
    run_validated_deterministic_command_with_isolation(
        paths,
        &ValidatedCommand::assume_validated(command.clone()),
        None,
        cancellation,
        false,
    )
}

pub(crate) fn run_isolated_structured_command_in_session_with_cancellation(
    paths: &Paths,
    command: &VerificationCommand,
    isolation: &IsolationSession,
    cancellation: &CancellationToken,
) -> Result<DeterministicCommandResult> {
    anyhow::ensure!(
        !cancellation.is_cancelled(),
        "deterministic command was cancelled before launch"
    );
    anyhow::ensure!(
        crate::verification::registry::is_deterministic_command_structured_allowed(command),
        "command is outside the typed verification registry"
    );
    run_validated_deterministic_command_with_isolation(
        paths,
        &ValidatedCommand::assume_validated(command.clone()),
        Some(isolation),
        cancellation,
        true,
    )
}

pub(crate) fn run_isolated_deterministic_command(
    paths: &Paths,
    command: &VerificationCommand,
) -> Result<DeterministicCommandResult> {
    let isolation = IsolationSession::new(paths)?;
    run_isolated_deterministic_command_in_session(paths, command, &isolation)
}

pub(crate) fn run_isolated_deterministic_command_in_session(
    paths: &Paths,
    command: &VerificationCommand,
    isolation: &IsolationSession,
) -> Result<DeterministicCommandResult> {
    run_validated_deterministic_command_with_isolation(
        paths,
        &ValidatedCommand::assume_validated(command.clone()),
        Some(isolation),
        &CancellationToken::default(),
        false,
    )
}

pub(crate) fn run_validated_deterministic_command(
    paths: &Paths,
    command: &ValidatedCommand,
) -> Result<DeterministicCommandResult> {
    run_validated_deterministic_command_with_isolation(
        paths,
        command,
        None,
        &CancellationToken::default(),
        false,
    )
}

fn run_validated_deterministic_command_with_isolation(
    paths: &Paths,
    command: &ValidatedCommand,
    isolation: Option<&IsolationSession>,
    cancellation: &CancellationToken,
    structured_output: bool,
) -> Result<DeterministicCommandResult> {
    // This is the execution safety line: no zsh, no `-lc`, no command string.
    // Rust supplies program/args/cwd directly and captures output itself.
    let command = command.as_command();
    let command_display = command.display();
    progress(format_args!(
        "[Rust] $ {}",
        truncate_inline(&command_display, 140)
    ));
    if let Some(check) = builtin_check_from_command(command) {
        return run_builtin_verification(command, paths, check);
    }
    let isolated = isolation.is_some();
    let isolated_command = isolation
        .map(|isolation| isolation.prepare(command, cancellation))
        .transpose()?;
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
    if !isolated {
        fs::create_dir_all(&clang_module_cache)?;
        fs::create_dir_all(&swift_module_cache)?;
    }
    let cwd = isolated_command
        .as_ref()
        .map(|prepared| prepared.cwd.clone())
        .unwrap_or(resolve_contained_cwd(paths, command.cwd.as_deref())?);
    let started = Instant::now();
    let launch_program = isolated_command
        .as_ref()
        .map(|prepared| prepared.program.as_path())
        .unwrap_or_else(|| Path::new(&command.program));
    let mut cmd = Command::new(launch_program);
    if let Some(prepared) = isolated_command.as_ref() {
        cmd.args(&prepared.args)
            .env_clear()
            .envs(prepared.env.clone());
    } else {
        cmd.args(&command.args)
            .env("CLANG_MODULE_CACHE_PATH", &clang_module_cache)
            .env("SWIFT_MODULE_CACHE_PATH", &swift_module_cache);
    }
    cmd.current_dir(&cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        if !crate::runtime::worker_contained() {
            unsafe {
                cmd.pre_exec(|| {
                    extern "C" {
                        fn setpgid(pid: i32, pgid: i32) -> i32;
                    }
                    if setpgid(0, 0) != 0 {
                        return Err(io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
        }
    }
    let mut child = cmd
        .spawn()
        .with_context(|| format!("run deterministic command `{command_display}`"))?;
    let child_id = child.id();
    let mut active_child = match ActiveChildProcessGroup::register(paths, child_id) {
        Ok(registration) => registration,
        Err(error) => {
            kill_process_tree(child_id);
            let _ = child.wait();
            return Err(error).with_context(|| {
                format!("publish verification process group `{command_display}`")
            });
        }
    };
    let mut stdout_pipe = child.stdout.take();
    let mut stderr_pipe = child.stderr.take();
    #[cfg(unix)]
    if let Err(error) = stdout_pipe
        .as_ref()
        .map_or(Ok(()), set_pipe_nonblocking)
        .and_then(|()| stderr_pipe.as_ref().map_or(Ok(()), set_pipe_nonblocking))
    {
        kill_process_tree(child_id);
        let _ = child.wait();
        active_child.clear()?;
        return Err(error).context("configure bounded verification output pipes");
    }
    let stop_readers = Arc::new(AtomicBool::new(false));
    let aggregate_bytes = Arc::new(AtomicU64::new(0));
    let stdout_aggregate = aggregate_bytes.clone();
    let stdout_stop = stop_readers.clone();
    let stdout_reader = thread::spawn(move || -> io::Result<BoundedVerificationOutput> {
        if let Some(mut stdout) = stdout_pipe.take() {
            return read_bounded_verification_output_with_limit(
                &mut stdout,
                &stdout_aggregate,
                &stdout_stop,
                if structured_output {
                    MAX_VERIFICATION_STREAM_BYTES as usize
                } else {
                    VERIFICATION_TAIL_BYTES
                },
            );
        }
        Ok(BoundedVerificationOutput {
            tail: Vec::new(),
            overflowed: false,
            pipe_open_at_shutdown: false,
        })
    });
    let stderr_aggregate = aggregate_bytes;
    let stderr_stop = stop_readers.clone();
    let stderr_reader = thread::spawn(move || -> io::Result<BoundedVerificationOutput> {
        if let Some(mut stderr) = stderr_pipe.take() {
            return read_bounded_verification_output(&mut stderr, &stderr_aggregate, &stderr_stop);
        }
        Ok(BoundedVerificationOutput {
            tail: Vec::new(),
            overflowed: false,
            pipe_open_at_shutdown: false,
        })
    });
    let timeout = Duration::from_secs(command.timeout_seconds.unwrap_or(900).clamp(1, 3_600));
    let mut timed_out = false;
    let mut cancelled = false;
    let status = loop {
        if cancellation.is_cancelled() {
            cancelled = true;
            kill_process_tree(child_id);
            break child
                .wait()
                .with_context(|| format!("wait after cancelling `{command_display}`"))?;
        }
        if started.elapsed() >= timeout {
            timed_out = true;
            kill_process_tree(child_id);
            break child
                .wait()
                .with_context(|| format!("wait after killing `{command_display}`"))?;
        }
        if let Some(status) = child.try_wait()? {
            terminate_remaining_process_group(child_id, TERMINATION_GRACE);
            break status;
        }
        thread::sleep(Duration::from_millis(50));
    };
    stop_readers.store(true, Ordering::Release);
    let stdout_output = stdout_reader
        .join()
        .map_err(|_| anyhow!("stdout reader panicked for `{command_display}`"))?
        .with_context(|| format!("read deterministic stdout `{command_display}`"))?;
    let stderr_output = stderr_reader
        .join()
        .map_err(|_| anyhow!("stderr reader panicked for `{command_display}`"))?
        .with_context(|| format!("read deterministic stderr `{command_display}`"))?;
    active_child.clear()?;
    if cancelled {
        bail!("deterministic command `{command_display}` was cancelled");
    }
    let duration_ms = started.elapsed().as_millis();
    let exit_code = status.code().unwrap_or(1);
    let stdout = String::from_utf8_lossy(&stdout_output.tail);
    let stderr = String::from_utf8_lossy(&stderr_output.tail);
    let combined = format!("{stdout}{stderr}");
    let output_overflowed = stdout_output.overflowed || stderr_output.overflowed;
    let pipe_open_at_shutdown =
        stdout_output.pipe_open_at_shutdown || stderr_output.pipe_open_at_shutdown;
    let category = if timed_out {
        DeterministicResultCategory::TimedOut
    } else if pipe_open_at_shutdown || output_overflowed {
        DeterministicResultCategory::CommandFailed
    } else if exit_code == 0
        && (command.expected_exit == Some(0) || command.expected_exit == Some(exit_code))
    {
        DeterministicResultCategory::Passed
    } else {
        classify_deterministic_result(&command_display, exit_code, &combined)
    };
    let evidence = if isolated {
        format!(
            "isolated verification category={} exit={} output withheld",
            category.as_str(),
            exit_code
        )
    } else if timed_out {
        format!("command timed out after {}s", timeout.as_secs())
    } else if pipe_open_at_shutdown {
        "command left an unterminated output pipe after process cleanup".to_string()
    } else if output_overflowed {
        "command output exceeded deterministic verification safety limits".to_string()
    } else {
        deterministic_evidence(&command_display, exit_code, category, &combined)
    };
    let elapsed = format_duration(Duration::from_millis(duration_ms as u64));
    if category == DeterministicResultCategory::Passed {
        progress(format_args!("[Rust] ok exit 0 in {elapsed}"));
    } else if category == DeterministicResultCategory::NoMatches {
        progress(format_args!("[Rust] no matches exit 1 in {elapsed}"));
    } else if category == DeterministicResultCategory::KnownEnvironmentBlocker {
        progress(format_args!(
            "[Rust] known environment blocker exit {exit_code} in {elapsed}"
        ));
    } else if category == DeterministicResultCategory::TimedOut {
        progress(format_args!(
            "[Rust] timed out after {}s",
            timeout.as_secs()
        ));
    } else {
        progress(format_args!("[Rust] fail exit {exit_code} in {elapsed}"));
        if !isolated {
            let preview = preview_text(&combined, 4, 600);
            for line in preview.lines {
                progress(format_args!("  {line}"));
            }
        }
    }
    Ok(DeterministicCommandResult {
        command: command.clone(),
        exit_code,
        duration_ms,
        category,
        evidence,
        stdout_tail: if structured_output {
            stdout.into_owned()
        } else if isolated {
            String::new()
        } else {
            output_tail(&stdout, 1_200)
        },
        stderr_tail: if isolated {
            String::new()
        } else {
            output_tail(&stderr, 1_200)
        },
    })
}

fn builtin_check_from_command(command: &VerificationCommand) -> Option<BuiltinCheck> {
    if !is_ca_verification_command(command) {
        return None;
    }
    match command.args.as_slice() {
        [verify, name] if command.program == "codex-team" && verify == "verify" => {
            BuiltinCheck::parse(name)
        }
        _ => None,
    }
}

fn run_builtin_verification(
    command: &VerificationCommand,
    paths: &Paths,
    check: BuiltinCheck,
) -> Result<DeterministicCommandResult> {
    let started = Instant::now();
    match require_builtin_check(paths, check) {
        Ok(receipt) => {
            let stdout = serde_json::to_string(&receipt)?;
            let duration_ms = started.elapsed().as_millis();
            progress(format_args!(
                "[Rust] ok {} in {}",
                receipt.summary,
                format_duration(Duration::from_millis(duration_ms as u64))
            ));
            Ok(DeterministicCommandResult {
                command: command.clone(),
                exit_code: 0,
                duration_ms,
                category: DeterministicResultCategory::Passed,
                evidence: receipt.summary,
                stdout_tail: output_tail(&stdout, 1_200),
                stderr_tail: String::new(),
            })
        }
        Err(err) => {
            let summary = truncate_inline(&err.to_string(), 1_200);
            let duration_ms = started.elapsed().as_millis();
            progress(format_args!(
                "[Rust] fail {} in {}",
                summary,
                format_duration(Duration::from_millis(duration_ms as u64))
            ));
            Ok(DeterministicCommandResult {
                command: command.clone(),
                exit_code: 1,
                duration_ms,
                category: DeterministicResultCategory::CommandFailed,
                evidence: summary.clone(),
                stdout_tail: String::new(),
                stderr_tail: summary,
            })
        }
    }
}

fn resolve_contained_cwd(paths: &Paths, cwd: Option<&str>) -> Result<PathBuf> {
    let candidate = cwd
        .map(|cwd| paths.root.join(cwd))
        .unwrap_or_else(|| paths.root.clone());
    let canonical = match fs::canonicalize(&candidate) {
        Ok(path) => path,
        Err(_) if cwd.is_none() => paths.root.clone(),
        Err(err) => {
            return Err(err)
                .with_context(|| format!("canonicalize verification cwd {}", candidate.display()));
        }
    };
    let root = fs::canonicalize(&paths.root).unwrap_or_else(|_| paths.root.clone());
    if canonical == root || canonical.starts_with(&root) {
        Ok(canonical)
    } else {
        bail!(
            "verification cwd {} escapes repository root {}",
            canonical.display(),
            root.display()
        )
    }
}

fn read_bounded_verification_output<R: Read>(
    reader: &mut R,
    aggregate_bytes: &AtomicU64,
    stop: &AtomicBool,
) -> io::Result<BoundedVerificationOutput> {
    read_bounded_verification_output_with_limit(
        reader,
        aggregate_bytes,
        stop,
        VERIFICATION_TAIL_BYTES,
    )
}

fn read_bounded_verification_output_with_limit<R: Read>(
    reader: &mut R,
    aggregate_bytes: &AtomicU64,
    stop: &AtomicBool,
    tail_limit: usize,
) -> io::Result<BoundedVerificationOutput> {
    let mut tail = VecDeque::with_capacity(tail_limit);
    let mut buffer = [0_u8; 8 * 1024];
    let mut stream_bytes = 0_u64;
    let mut current_line_bytes = 0_usize;
    let mut overflowed = false;
    loop {
        let read = match reader.read(&mut buffer) {
            Ok(read) => read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                if stop.load(Ordering::Acquire) {
                    return Ok(BoundedVerificationOutput {
                        tail: tail.into_iter().collect(),
                        overflowed,
                        pipe_open_at_shutdown: true,
                    });
                }
                thread::sleep(PROCESS_POLL_INTERVAL);
                continue;
            }
            Err(error) => return Err(error),
        };
        if read == 0 {
            break;
        }
        stream_bytes = stream_bytes.saturating_add(read as u64);
        if stream_bytes > MAX_VERIFICATION_STREAM_BYTES {
            overflowed = true;
        }
        let prior_aggregate = aggregate_bytes.fetch_add(read as u64, Ordering::Relaxed);
        if prior_aggregate.saturating_add(read as u64) > MAX_VERIFICATION_AGGREGATE_BYTES {
            overflowed = true;
        }
        for byte in &buffer[..read] {
            if *byte == b'\n' {
                current_line_bytes = 0;
            } else {
                current_line_bytes = current_line_bytes.saturating_add(1);
                if current_line_bytes > MAX_VERIFICATION_LINE_BYTES {
                    overflowed = true;
                }
            }
            tail.push_back(*byte);
            if tail.len() > tail_limit {
                tail.pop_front();
            }
        }
    }
    Ok(BoundedVerificationOutput {
        tail: tail.into_iter().collect(),
        overflowed,
        pipe_open_at_shutdown: false,
    })
}

#[cfg(unix)]
fn set_pipe_nonblocking<T: std::os::fd::AsRawFd>(pipe: &T) -> io::Result<()> {
    let fd = pipe.as_raw_fd();
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 {
        return Err(io::Error::last_os_error());
    }
    if unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn terminate_remaining_process_group(child_id: u32, grace: Duration) {
    // The ca parent drains the encompassing worker group after this worker
    // exits. A contained child cannot signal that group without killing itself.
    if crate::runtime::worker_contained() {
        return;
    }
    #[cfg(unix)]
    {
        if !process_group_exists(child_id) {
            return;
        }
        signal_process_group(child_id, libc::SIGTERM);
        let started = Instant::now();
        while started.elapsed() < grace {
            if !process_group_exists(child_id) {
                return;
            }
            thread::sleep(PROCESS_POLL_INTERVAL);
        }
        signal_process_group(child_id, libc::SIGKILL);
        let cleanup_started = Instant::now();
        while cleanup_started.elapsed() < grace && process_group_exists(child_id) {
            thread::sleep(PROCESS_POLL_INTERVAL);
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (child_id, grace);
    }
}

fn kill_process_tree(pid: u32) {
    #[cfg(unix)]
    {
        if crate::runtime::worker_contained() {
            unsafe {
                libc::kill(pid as i32, libc::SIGTERM);
                libc::kill(pid as i32, libc::SIGKILL);
            }
            return;
        }
        let _ = Command::new("kill")
            .args(["-TERM", &format!("-{pid}")])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let _ = Command::new("kill")
            .args(["-KILL", &format!("-{pid}")])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
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
    io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
}

pub(crate) fn classify_deterministic_result(
    command: &str,
    exit_code: i32,
    output: &str,
) -> DeterministicResultCategory {
    if exit_code == 0 {
        return DeterministicResultCategory::Passed;
    }
    if is_rg_no_matches(command, Some(exit_code as i64)) {
        return DeterministicResultCategory::NoMatches;
    }
    if is_known_environment_blocker(command, exit_code, output) {
        return DeterministicResultCategory::KnownEnvironmentBlocker;
    }
    DeterministicResultCategory::CommandFailed
}

fn is_known_environment_blocker(command: &str, exit_code: i32, output: &str) -> bool {
    if is_swiftpm_sandbox_failure(command, Some(exit_code as i64), output) {
        return true;
    }
    let lower = output.to_ascii_lowercase();
    lower.contains("operation not permitted")
        && (lower.contains("bind") || lower.contains("udp") || lower.contains("loopback"))
}

fn deterministic_evidence(
    command: &str,
    exit_code: i32,
    category: DeterministicResultCategory,
    output: &str,
) -> String {
    match category {
        DeterministicResultCategory::Passed => {
            if exit_code == 0 {
                "exit 0".to_string()
            } else {
                format!("expected exit {exit_code}")
            }
        }
        DeterministicResultCategory::NoMatches => "rg exited 1 with no matches".to_string(),
        DeterministicResultCategory::KnownEnvironmentBlocker => {
            if is_swiftpm_sandbox_failure(command, Some(exit_code as i64), output) {
                "known SwiftPM sandbox/module-cache blocker".to_string()
            } else {
                "known local environment permission blocker".to_string()
            }
        }
        DeterministicResultCategory::TimedOut => "command timed out".to_string(),
        DeterministicResultCategory::CommandFailed => {
            let preview = preview_text(output, 2, 240);
            let detail = preview.lines.join(" ");
            if detail.is_empty() {
                format!("command exited {exit_code}")
            } else {
                format!(
                    "command exited {exit_code}: {}",
                    truncate_inline(&detail, 240)
                )
            }
        }
    }
}

fn output_tail(text: &str, max_chars: usize) -> String {
    let clean = strip_ansi(text.trim());
    let len = clean.chars().count();
    if len <= max_chars {
        return clean;
    }
    let mut tail = clean
        .chars()
        .skip(len.saturating_sub(max_chars))
        .collect::<String>();
    tail.insert_str(0, "[truncated] ");
    tail
}

struct Preview {
    lines: Vec<String>,
}

fn preview_text(text: &str, max_lines: usize, max_chars: usize) -> Preview {
    let trimmed = text.trim();
    if trimmed.is_empty() || max_lines == 0 || max_chars == 0 {
        return Preview { lines: Vec::new() };
    }

    let mut lines = Vec::new();
    let mut used_chars = 0;
    for line in trimmed.lines() {
        if lines.len() >= max_lines || used_chars >= max_chars {
            break;
        }
        let remaining = max_chars.saturating_sub(used_chars);
        let clean = strip_ansi(line.trim_end());
        let visible = truncate_inline(&clean, remaining.min(240));
        used_chars += visible.chars().count() + 1;
        lines.push(visible);
    }

    Preview { lines }
}

fn is_rg_no_matches(command: &str, exit_code: Option<i64>) -> bool {
    exit_code == Some(1) && command.trim().starts_with("rg ")
}

fn is_swiftpm_sandbox_failure(command: &str, exit_code: Option<i64>, output: &str) -> bool {
    if exit_code == Some(0) || !is_swiftpm_command(command) {
        return false;
    }
    output.contains("sandbox-exec: sandbox_apply: Operation not permitted")
        || output.contains("/.cache/clang/ModuleCache")
            && output.contains("Operation not permitted")
        || output.contains("Invalid manifest")
            && output.contains("unable to load standard library")
            && output.contains("Operation not permitted")
}

fn is_swiftpm_command(command: &str) -> bool {
    command.split("&&").map(str::trim).any(|part| {
        part.starts_with("swift build")
            || part.starts_with("swift test")
            || part.starts_with("swift run")
            || part.contains(" swift build")
            || part.contains(" swift test")
            || part.contains(" swift run")
    })
}

fn format_duration(duration: Duration) -> String {
    let secs = duration.as_secs();
    if secs >= 60 {
        format!("{}m{}s", secs / 60, secs % 60)
    } else if secs > 0 {
        format!("{secs}s")
    } else {
        format!("{}ms", duration.as_millis())
    }
}

fn truncate_inline(s: &str, max_chars: usize) -> String {
    let mut out = String::new();
    let mut chars = s.chars();
    for _ in 0..max_chars {
        let Some(ch) = chars.next() else {
            return out;
        };
        out.push(if ch == '\n' { ' ' } else { ch });
    }
    if chars.next().is_some() {
        out.push_str("...");
    }
    out
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' && chars.peek() == Some(&'[') {
            let _ = chars.next();
            for next in chars.by_ref() {
                if next.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(ch);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::verification::ca::parse_ca_verification_command;
    use std::io::Cursor;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_paths(name: &str) -> Result<(PathBuf, Paths)> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("codex-team-runner-{name}-{nonce}"));
        fs::create_dir_all(&root)?;
        let paths = Paths::new(&root)?;
        Ok((root, paths))
    }

    #[cfg(unix)]
    #[test]
    fn escaped_pipe_holder_helper() -> Result<()> {
        let cwd = std::env::current_dir()?;
        if !cwd.join(".enable-escaped-pipe-helper").exists() {
            return Ok(());
        }
        let mut ready = [0_i32; 2];
        if unsafe { libc::pipe(ready.as_mut_ptr()) } != 0 {
            return Err(io::Error::last_os_error().into());
        }
        let pid = unsafe { libc::fork() };
        if pid < 0 {
            unsafe {
                libc::close(ready[0]);
                libc::close(ready[1]);
            }
            return Err(io::Error::last_os_error().into());
        }
        if pid == 0 {
            unsafe {
                libc::close(ready[0]);
                if libc::setsid() < 0 {
                    libc::_exit(2);
                }
                libc::signal(libc::SIGTERM, libc::SIG_IGN);
                libc::signal(libc::SIGHUP, libc::SIG_IGN);
                let _ = libc::write(ready[1], b"1".as_ptr().cast(), 1);
                libc::close(ready[1]);
                loop {
                    libc::sleep(30);
                }
            }
        }
        unsafe {
            libc::close(ready[1]);
            let mut byte = [0_u8; 1];
            let read = libc::read(ready[0], byte.as_mut_ptr().cast(), 1);
            libc::close(ready[0]);
            if read != 1 {
                let _ = libc::kill(pid, libc::SIGKILL);
                return Err(io::Error::last_os_error().into());
            }
        }
        fs::write(cwd.join("escaped-pipe-holder.pid"), pid.to_string())?;
        Ok(())
    }

    #[test]
    fn snapshot_hygiene_runs_in_process_and_preserves_result_contract() -> Result<()> {
        let (root, paths) = temp_paths("snapshot")?;
        let command =
            parse_ca_verification_command("codex-team verify snapshot-hygiene", "runner.test")
                .unwrap();

        // Success proves the runner did not try to resolve a `codex-team`
        // executable from PATH; this test only has the in-process library.
        let passed = run_deterministic_command(&paths, &command)?;
        assert_eq!(passed.exit_code, 0);
        assert_eq!(passed.category, DeterministicResultCategory::Passed);
        assert!(passed.stdout_tail.contains("codex-team.verify.v1"));
        assert!(passed.stderr_tail.is_empty());

        fs::create_dir_all(root.join("tests/snapshots"))?;
        fs::write(root.join("tests/snapshots/pending.snap.new"), "pending")?;
        let failed = run_deterministic_command(&paths, &command)?;
        assert_eq!(failed.exit_code, 1);
        assert_eq!(failed.category, DeterministicResultCategory::CommandFailed);
        assert!(failed.evidence.contains("pending .snap.new"));
        assert_eq!(failed.stderr_tail, failed.evidence);

        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn bounded_output_drains_but_rejects_stream_and_line_overflow() -> Result<()> {
        let bytes = vec![b'x'; MAX_VERIFICATION_STREAM_BYTES as usize + 1];
        let aggregate = AtomicU64::new(0);
        let stop = AtomicBool::new(false);
        let output = read_bounded_verification_output(&mut Cursor::new(bytes), &aggregate, &stop)?;

        assert!(output.overflowed);
        assert_eq!(output.tail.len(), VERIFICATION_TAIL_BYTES);
        assert_eq!(
            aggregate.load(Ordering::Relaxed),
            MAX_VERIFICATION_STREAM_BYTES + 1
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn high_volume_pipe_holding_grandchild_is_bounded_and_reaped() -> Result<()> {
        use std::os::unix::fs::PermissionsExt;

        let (root, paths) = temp_paths("bounded-grandchild")?;
        let pid_file = root.join("grandchild.pid");
        let fake = root.join("verification-output-flood");
        fs::write(
            &fake,
            format!(
                "#!/bin/sh\n(\n  trap '' TERM HUP\n  while :; do printf '%01024d\\n' 0; done\n) &\necho $! > '{}'\nexit 0\n",
                pid_file.display()
            ),
        )?;
        let mut permissions = fs::metadata(&fake)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&fake, permissions)?;
        let mut command = VerificationCommand::new(
            fake.to_string_lossy().into_owned(),
            Vec::new(),
            "runner.test",
        );
        command.timeout_seconds = Some(10);
        command.expected_exit = Some(0);

        let started = Instant::now();
        let result = run_deterministic_command(&paths, &command)?;
        assert!(started.elapsed() < Duration::from_secs(8));
        assert_eq!(result.category, DeterministicResultCategory::CommandFailed);
        assert!(result.evidence.contains("output exceeded"));

        let grandchild = fs::read_to_string(&pid_file)?.trim().parse::<i32>()?;
        let deadline = Instant::now() + Duration::from_secs(2);
        while process_exists(grandchild) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(25));
        }
        assert!(
            !process_exists(grandchild),
            "verification grandchild survived"
        );
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn session_escaping_pipe_holder_cannot_hang_or_pass_verification() -> Result<()> {
        let (root, paths) = temp_paths("escaped-pipe-holder")?;
        fs::write(root.join(".enable-escaped-pipe-helper"), b"enabled\n")?;
        let mut command = VerificationCommand::new(
            std::env::current_exe()?.to_string_lossy().into_owned(),
            vec![
                "--exact".to_string(),
                "verification::runner::tests::escaped_pipe_holder_helper".to_string(),
                "--nocapture".to_string(),
            ],
            "runner.test",
        );
        command.timeout_seconds = Some(10);
        command.expected_exit = Some(0);

        let started = Instant::now();
        let result = run_deterministic_command(&paths, &command)?;
        assert!(started.elapsed() < Duration::from_secs(8));
        assert_eq!(result.category, DeterministicResultCategory::CommandFailed);
        assert!(result.evidence.contains("unterminated output pipe"));

        let escaped = fs::read_to_string(root.join("escaped-pipe-holder.pid"))?
            .trim()
            .parse::<i32>()?;
        unsafe {
            libc::kill(escaped, libc::SIGKILL);
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        while process_exists(escaped) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(25));
        }
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[cfg(unix)]
    fn process_exists(pid: i32) -> bool {
        let result = unsafe { libc::kill(pid, 0) };
        if result == 0 {
            return true;
        }
        io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
    }
}
