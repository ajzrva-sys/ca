use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const FIXTURE_IDS: [&str; 6] = [
    "unicode_parser_boundary",
    "permission_scope_matching",
    "protocol_tool_interleave",
    "atomic_journal_recovery",
    "process_timeout_cleanup",
    "config_migration",
];

const FIXTURE_SCHEMA: &str = "reasoning_pilot_fixture.v1";
const MAX_FIXTURE_ENTRIES: usize = 64;
const MAX_FIXTURE_BYTES: u64 = 1_048_576;
const MAX_OUTPUT_BYTES: usize = 64 * 1024;
const COMMAND_TIMEOUT: Duration = Duration::from_secs(60);
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixtureCheckReceipt {
    pub passed: bool,
    pub network_isolation: String,
    pub candidates: Vec<CandidateReceipt>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateReceipt {
    pub fixture_id: String,
    pub kind: String,
    pub public_passed: bool,
    pub hidden_passed: bool,
    pub hidden_failure_code: Option<String>,
    pub build_diagnostic: Option<String>,
}

#[allow(dead_code)]
pub fn fixture_check(repository_root: &Path) -> Result<FixtureCheckReceipt, String> {
    let repository_root = repository_root
        .canonicalize()
        .map_err(|error| format!("invalid repository root: {error}"))?;
    require_network_sandbox()?;
    let work = PrivateWorkRoot::create(&repository_root)?;
    let mut candidates = Vec::with_capacity(FIXTURE_IDS.len() * 2);

    for (kind, relative_root) in [
        ("baseline", "tests/fixtures/reasoning_pilot"),
        ("golden", "tests/reasoning_pilot_golden"),
    ] {
        for fixture_id in FIXTURE_IDS {
            let source = repository_root.join(relative_root).join(fixture_id);
            let materialized = work.path.join("candidates").join(kind).join(fixture_id);
            validate_and_copy_fixture(&source, &materialized, fixture_id)?;
            candidates.push(verify_materialized_fixture(
                &repository_root,
                fixture_id,
                kind,
                &materialized,
            )?);
        }
    }

    let passed = candidates.iter().all(|candidate| {
        candidate.public_passed
            && if candidate.kind == "baseline" {
                !candidate.hidden_passed
                    && candidate.hidden_failure_code.as_deref()
                        == Some(expected_failure_code(&candidate.fixture_id))
            } else {
                candidate.hidden_passed && candidate.hidden_failure_code.is_none()
            }
    });
    Ok(FixtureCheckReceipt {
        passed,
        network_isolation: "sandbox-exec-deny-network".to_string(),
        candidates,
    })
}

/// Validates the immutable source fixture before a controller materializes it.
/// The public WSP-096 adapter uses this before every source snapshot; it does
/// not copy or execute the candidate.
pub fn validate_fixture_source(source: &Path, expected_id: &str) -> Result<(), String> {
    let source = source
        .canonicalize()
        .map_err(|error| format!("missing fixture {expected_id}: {error}"))?;
    if !source.is_dir() {
        return Err(format!("fixture {expected_id} is not a directory"));
    }
    let mut files = Vec::new();
    collect_fixture_files(&source, &source, &mut files, 0)?;
    files.sort();
    if files.len() > MAX_FIXTURE_ENTRIES {
        return Err(format!("fixture {expected_id} exceeds entry limit"));
    }
    let logical_bytes = files.iter().try_fold(0_u64, |total, relative| {
        let length = fs::metadata(source.join(relative))
            .map_err(|error| error.to_string())?
            .len();
        total
            .checked_add(length)
            .ok_or_else(|| "fixture byte count overflow".to_string())
    })?;
    if logical_bytes > MAX_FIXTURE_BYTES {
        return Err(format!("fixture {expected_id} exceeds byte limit"));
    }
    let manifest = fs::read_to_string(source.join("fixture.json"))
        .map_err(|error| format!("missing fixture manifest: {error}"))?;
    if !manifest.contains(&format!("\"schema_version\": \"{FIXTURE_SCHEMA}\""))
        || !manifest.contains(&format!("\"id\": \"{expected_id}\""))
        || !manifest.contains("\"stdlib_only\": true")
        || !manifest.contains("\"network_allowed\": false")
    {
        return Err(format!("fixture {expected_id} has an invalid manifest"));
    }
    validate_stdlib_only(&source, &files)
}

/// Executes the public and hidden checks against an already materialized
/// candidate. This never copies the fixture: callers supply the isolated
/// workspace and keep ownership of its cleanup/recovery lifecycle.
pub fn verify_materialized_fixture(
    repository_root: &Path,
    fixture_id: &str,
    kind: &str,
    materialized: &Path,
) -> Result<CandidateReceipt, String> {
    verify_materialized_fixture_with_profile(
        repository_root,
        fixture_id,
        kind,
        materialized,
        |_, _, forbidden| Ok(sandbox_profile(forbidden)),
    )
}

pub fn verify_materialized_fixture_with_profile(
    repository_root: &Path,
    fixture_id: &str,
    kind: &str,
    materialized: &Path,
    profile: impl FnOnce(&Path, &Path, &[PathBuf]) -> Result<String, String>,
) -> Result<CandidateReceipt, String> {
    if !matches!(kind, "baseline" | "golden" | "trial") || !FIXTURE_IDS.contains(&fixture_id) {
        return Err("fixture candidate identity is invalid".to_string());
    }
    let repository_root = repository_root
        .canonicalize()
        .map_err(|error| format!("invalid repository root: {error}"))?;
    let materialized = materialized
        .canonicalize()
        .map_err(|error| format!("invalid materialized fixture: {error}"))?;
    if !materialized.is_dir() || materialized.starts_with(&repository_root) {
        return Err("materialized fixture is not isolated from the repository".to_string());
    }
    require_network_sandbox()?;
    let cargo = resolve_executable("cargo")?;
    let rustc = resolve_executable("rustc")?;
    let work = PrivateWorkRoot::create(&repository_root)?;
    let mut forbidden_roots = vec![repository_root];
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        if home.is_absolute() && !forbidden_roots.iter().any(|root| root == &home) {
            forbidden_roots.push(home);
        }
    }
    let target_dir = work.path.join("targets").join(kind).join(fixture_id);
    fs::create_dir_all(&target_dir).map_err(|error| error.to_string())?;
    if target_dir.starts_with(&materialized) {
        return Err("Cargo target directory overlaps candidate workspace".to_string());
    }
    let mut environment =
        CleanEnvironment::create(&work.path, &target_dir, &rustc, &forbidden_roots)?;
    environment.sandbox_profile = profile(&materialized, &work.path, &forbidden_roots)?;
    let public_check = run_cargo(
        &cargo,
        &materialized,
        &environment,
        &["test", "--quiet", "--offline", "--locked", "--no-fail-fast"],
    )?;
    let public_passed = public_check.success();
    let build_check = if public_passed {
        Some(run_cargo(
            &cargo,
            &materialized,
            &environment,
            &["build", "--quiet", "--offline", "--locked", "--bins"],
        )?)
    } else {
        None
    };
    let built = build_check.as_ref().is_some_and(CommandResult::success);
    let failed = if !public_passed {
        Some(&public_check)
    } else {
        build_check.as_ref().filter(|check| !check.success())
    };
    let build_diagnostic = failed.map(|check| {
        String::from_utf8_lossy(&check.stderr)
            .chars()
            .take(4096)
            .collect()
    });
    let hidden_result = if built {
        run_hidden_probe(fixture_id, &target_dir, &work.path, &environment)
    } else {
        Err("candidate_build_failed")
    };
    Ok(CandidateReceipt {
        fixture_id: fixture_id.to_string(),
        kind: kind.to_string(),
        public_passed,
        hidden_passed: hidden_result.is_ok(),
        hidden_failure_code: hidden_result.err().map(str::to_string),
        build_diagnostic,
    })
}

fn expected_failure_code(fixture_id: &str) -> &str {
    match fixture_id {
        "unicode_parser_boundary" => "unicode_mid_scalar",
        "permission_scope_matching" => "component_prefix_escape",
        "protocol_tool_interleave" => "interleaved_call_state",
        "atomic_journal_recovery" => "partial_tail_recovery",
        "process_timeout_cleanup" => "descendant_survived_timeout",
        "config_migration" => "legacy_config_rejected",
        _ => "unknown_fixture",
    }
}

fn validate_and_copy_fixture(
    source: &Path,
    destination: &Path,
    expected_id: &str,
) -> Result<(), String> {
    validate_fixture_source(source, expected_id)?;
    let source = source
        .canonicalize()
        .map_err(|error| format!("missing fixture {expected_id}: {error}"))?;
    let mut files = Vec::new();
    collect_fixture_files(&source, &source, &mut files, 0)?;
    files.sort();
    fs::create_dir_all(destination).map_err(|error| error.to_string())?;
    fs::set_permissions(destination, fs::Permissions::from_mode(0o700))
        .map_err(|error| error.to_string())?;
    for relative in files {
        let source_file = source.join(&relative);
        let before = fs::read(&source_file).map_err(|error| error.to_string())?;
        let output = destination.join(&relative);
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let mut output_file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&output)
            .map_err(|error| error.to_string())?;
        output_file
            .write_all(&before)
            .map_err(|error| error.to_string())?;
        output_file.sync_all().map_err(|error| error.to_string())?;
        let after = fs::read(&source_file).map_err(|error| error.to_string())?;
        if before != after || fs::read(&output).map_err(|error| error.to_string())? != before {
            return Err(format!(
                "fixture source changed while copying {}",
                relative.display()
            ));
        }
    }
    Ok(())
}

fn collect_fixture_files(
    root: &Path,
    directory: &Path,
    files: &mut Vec<PathBuf>,
    depth: usize,
) -> Result<(), String> {
    if depth > 8 {
        return Err("fixture directory depth exceeded".to_string());
    }
    let mut entries = fs::read_dir(directory)
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
        let file_type = metadata.file_type();
        if file_type.is_symlink() {
            return Err(format!("fixture symlink rejected: {}", path.display()));
        }
        if file_type.is_dir() {
            if entry.file_name() == OsStr::new("target") || entry.file_name() == OsStr::new(".git")
            {
                return Err(format!("generated directory rejected: {}", path.display()));
            }
            collect_fixture_files(root, &path, files, depth + 1)?;
        } else if file_type.is_file() {
            files.push(
                path.strip_prefix(root)
                    .map_err(|_| "fixture path escaped root".to_string())?
                    .to_path_buf(),
            );
        } else {
            return Err(format!("fixture special file rejected: {}", path.display()));
        }
    }
    Ok(())
}

fn validate_stdlib_only(root: &Path, files: &[PathBuf]) -> Result<(), String> {
    let cargo_toml =
        fs::read_to_string(root.join("Cargo.toml")).map_err(|error| error.to_string())?;
    let mut dependency_section = false;
    for line in cargo_toml.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            dependency_section = trimmed.contains("dependencies");
        } else if dependency_section && !trimmed.is_empty() && !trimmed.starts_with('#') {
            return Err("fixture declares a non-stdlib dependency".to_string());
        }
    }
    if cargo_toml
        .lines()
        .any(|line| line.trim_start().starts_with("build ="))
    {
        return Err("fixture build script rejected".to_string());
    }
    for relative in files
        .iter()
        .filter(|path| path.extension() == Some(OsStr::new("rs")))
    {
        let source = fs::read_to_string(root.join(relative)).map_err(|error| error.to_string())?;
        for forbidden in [
            "std::net",
            "TcpStream",
            "TcpListener",
            "UdpSocket",
            "curl ",
            "wget ",
        ] {
            if source.contains(forbidden) {
                return Err(format!(
                    "fixture source contains forbidden network primitive in {}",
                    relative.display()
                ));
            }
        }
    }
    Ok(())
}

fn run_cargo(
    cargo: &Path,
    candidate: &Path,
    environment: &CleanEnvironment,
    args: &[&str],
) -> Result<CommandResult, String> {
    let manifest = candidate.join("Cargo.toml");
    let mut argv = args.iter().map(OsString::from).collect::<Vec<_>>();
    argv.push(OsString::from("--manifest-path"));
    argv.push(manifest.into_os_string());
    run_exact(
        cargo,
        &argv,
        None,
        candidate,
        environment,
        COMMAND_TIMEOUT,
        MAX_OUTPUT_BYTES,
    )
}

fn run_hidden_probe(
    fixture_id: &str,
    target_dir: &Path,
    work_root: &Path,
    environment: &CleanEnvironment,
) -> Result<(), &'static str> {
    let binary = target_dir.join("debug").join(package_name(fixture_id));
    match fixture_id {
        "unicode_parser_boundary" => {
            expect_output(
                &binary,
                &["1"],
                "éclair",
                "",
                target_dir,
                environment,
                "unicode_mid_scalar",
            )?;
            expect_output(
                &binary,
                &["2"],
                "éclair",
                "é",
                target_dir,
                environment,
                "unicode_mid_scalar",
            )?;
            expect_output(
                &binary,
                &["3"],
                "abé",
                "ab",
                target_dir,
                environment,
                "unicode_mid_scalar",
            )
        }
        "permission_scope_matching" => {
            expect_output(
                &binary,
                &["/workspace/app", "/workspace/app-copy/file"],
                "",
                "deny\n",
                target_dir,
                environment,
                "component_prefix_escape",
            )?;
            expect_output(
                &binary,
                &["/workspace/app", "/workspace/app/../secret"],
                "",
                "deny\n",
                target_dir,
                environment,
                "component_prefix_escape",
            )?;
            expect_output(
                &binary,
                &["workspace/app", "workspace/app/file"],
                "",
                "deny\n",
                target_dir,
                environment,
                "component_prefix_escape",
            )
        }
        "protocol_tool_interleave" => {
            expect_output(
                &binary,
                &[],
                "start a\nargs a A\nstart b\nargs b B\nargs a C\nfinish b\nfinish a\n",
                "b=B\na=AC\n",
                target_dir,
                environment,
                "interleaved_call_state",
            )?;
            expect_rejection(
                &binary,
                "start a\nargs a A\nstart a\nfinish a\n",
                target_dir,
                environment,
                "interleaved_call_state",
            )
        }
        "atomic_journal_recovery" => {
            let journal = work_root.join("partial-journal.bin");
            fs::write(&journal, b"3:one\n3:two\n5:three").map_err(|_| "partial_tail_recovery")?;
            let journal_arg = journal.to_string_lossy().into_owned();
            expect_output(
                &binary,
                &[journal_arg.as_str()],
                "",
                "one\ntwo\n",
                target_dir,
                environment,
                "partial_tail_recovery",
            )?;
            fs::write(&journal, b"3:one\n2:\xc3").map_err(|_| "partial_tail_recovery")?;
            expect_output(
                &binary,
                &[journal_arg.as_str()],
                "",
                "one\n",
                target_dir,
                environment,
                "partial_tail_recovery",
            )
        }
        "process_timeout_cleanup" => {
            probe_process_cleanup(&binary, target_dir, work_root, environment)
        }
        "config_migration" => {
            expect_output(
                &binary,
                &[],
                "version=1\napi_base=https://legacy.invalid/v1\nmodel=legacy-model\n",
                "provider_url=https://legacy.invalid/v1\nmodel=legacy-model\ntimeout_seconds=30\n",
                target_dir,
                environment,
                "legacy_config_rejected",
            )?;
            expect_rejection(&binary, "version=1\napi_base=https://legacy.invalid/v1\nmodel=legacy-model\nunknown=value\n", target_dir, environment, "legacy_config_rejected")
        }
        _ => Err("unknown_fixture"),
    }
}

fn package_name(fixture_id: &str) -> String {
    format!("reasoning-pilot-{}", fixture_id.replace('_', "-"))
}

#[allow(clippy::too_many_arguments)]
fn expect_output(
    binary: &Path,
    args: &[&str],
    stdin: &str,
    expected_stdout: &str,
    current_dir: &Path,
    environment: &CleanEnvironment,
    failure_code: &'static str,
) -> Result<(), &'static str> {
    let argv = args.iter().map(OsString::from).collect::<Vec<_>>();
    let output = run_exact(
        binary,
        &argv,
        Some(stdin.as_bytes()),
        current_dir,
        environment,
        PROBE_TIMEOUT,
        MAX_OUTPUT_BYTES,
    )
    .map_err(|_| failure_code)?;
    if output.success()
        && !output.stdout_truncated
        && !output.stderr_truncated
        && output.stdout == expected_stdout.as_bytes()
    {
        Ok(())
    } else {
        Err(failure_code)
    }
}

fn expect_rejection(
    binary: &Path,
    stdin: &str,
    current_dir: &Path,
    environment: &CleanEnvironment,
    failure_code: &'static str,
) -> Result<(), &'static str> {
    let output = run_exact(
        binary,
        &[],
        Some(stdin.as_bytes()),
        current_dir,
        environment,
        PROBE_TIMEOUT,
        MAX_OUTPUT_BYTES,
    )
    .map_err(|_| failure_code)?;
    if !output.success() && !output.stdout_truncated && !output.stderr_truncated {
        Ok(())
    } else {
        Err(failure_code)
    }
}

fn probe_process_cleanup(
    binary: &Path,
    target_dir: &Path,
    work_root: &Path,
    environment: &CleanEnvironment,
) -> Result<(), &'static str> {
    let helper = target_dir.join("debug/process-fixture-helper");
    let pid_file = work_root.join("descendant.pid");
    let argv = vec![
        helper.into_os_string(),
        pid_file.clone().into_os_string(),
        OsString::from("300"),
    ];
    let output = run_exact(
        binary,
        &argv,
        None,
        target_dir,
        environment,
        PROBE_TIMEOUT,
        MAX_OUTPUT_BYTES,
    )
    .map_err(|_| "descendant_survived_timeout")?;
    if !output.success() || output.stdout != b"timed_out\n" {
        return Err("descendant_survived_timeout");
    }
    let process_metadata = read_process_metadata(&pid_file)?;
    let pid = parse_descendant_metadata(&process_metadata)?;
    // Candidate-written PID text is evidence, never signal authority. The
    // isolated verifier must own the process group, and the kernel must place
    // a surviving descendant in that exact group before cleanup is permitted.
    let own_pid = unsafe { getpid() };
    let own_group = unsafe { getpgrp() };
    if pid <= 1 || pid == own_pid {
        return Err("descendant_survived_timeout");
    }
    if process_exists(pid) && (own_group != own_pid || unsafe { getpgid(pid) } != own_group) {
        return Err("descendant_survived_timeout");
    }
    let deadline = Instant::now() + Duration::from_millis(750);
    while process_exists(pid) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    let survived = process_exists(pid);
    if survived && unsafe { getpgid(pid) } == own_group && own_group == own_pid {
        unsafe {
            kill(pid, SIGKILL);
        }
    }
    let _ = fs::remove_file(pid_file);
    if survived {
        Err("descendant_survived_timeout")
    } else {
        Ok(())
    }
}

fn read_process_metadata(path: &Path) -> Result<String, &'static str> {
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc_flags::O_NOFOLLOW | libc_flags::O_NONBLOCK)
        .open(path)
        .map_err(|_| "descendant_survived_timeout")?;
    let metadata = file.metadata().map_err(|_| "descendant_survived_timeout")?;
    if !metadata.is_file() || metadata.nlink() != 1 || metadata.len() > 4096 {
        return Err("descendant_survived_timeout");
    }
    let mut text = String::new();
    file.take(4097)
        .read_to_string(&mut text)
        .map_err(|_| "descendant_survived_timeout")?;
    if text.len() > 4096 {
        return Err("descendant_survived_timeout");
    }
    Ok(text)
}

// This verifier crate is intentionally stdlib-only. These are the macOS flags;
// require_network_sandbox rejects other platforms before this reader is called.
mod libc_flags {
    pub const O_NOFOLLOW: i32 = 0x100;
    pub const O_NONBLOCK: i32 = 0x4;
}

fn parse_descendant_metadata(text: &str) -> Result<i32, &'static str> {
    let mut values = std::collections::BTreeMap::new();
    for line in text.lines() {
        let (key, value) = line.split_once('=').ok_or("descendant_survived_timeout")?;
        let value: i32 = value.parse().map_err(|_| "descendant_survived_timeout")?;
        if !matches!(key, "helper" | "group" | "descendant")
            || value <= 1
            || values.insert(key, value).is_some()
        {
            return Err("descendant_survived_timeout");
        }
    }
    if values.len() != 3 {
        return Err("descendant_survived_timeout");
    }
    values
        .get("descendant")
        .copied()
        .ok_or("descendant_survived_timeout")
}

fn process_exists(pid: i32) -> bool {
    let result = unsafe { kill(pid, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(EPERM)
}

#[derive(Debug)]
struct CleanEnvironment {
    home: PathBuf,
    cargo_home: PathBuf,
    temp: PathBuf,
    target_dir: PathBuf,
    rustc: PathBuf,
    sandbox_profile: String,
}

impl CleanEnvironment {
    fn create(
        work_root: &Path,
        target_dir: &Path,
        rustc: &Path,
        forbidden_roots: &[PathBuf],
    ) -> Result<Self, String> {
        let environment = Self {
            home: work_root.join("empty-home"),
            cargo_home: work_root.join("empty-cargo-home"),
            temp: work_root.join("tmp"),
            target_dir: target_dir.to_path_buf(),
            rustc: rustc.to_path_buf(),
            sandbox_profile: sandbox_profile(forbidden_roots),
        };
        for directory in [
            &environment.home,
            &environment.cargo_home,
            &environment.temp,
            &environment.target_dir,
        ] {
            fs::create_dir_all(directory).map_err(|error| error.to_string())?;
        }
        Ok(environment)
    }

    fn apply(&self, command: &mut Command) {
        command.env_clear();
        command.env("HOME", &self.home);
        command.env("CARGO_HOME", &self.cargo_home);
        command.env("TMPDIR", &self.temp);
        command.env("CARGO_TARGET_DIR", &self.target_dir);
        command.env("CARGO_NET_OFFLINE", "true");
        command.env("CARGO_TERM_COLOR", "never");
        command.env("CARGO_INCREMENTAL", "0");
        command.env("RUSTC", &self.rustc);
        command.env("RUSTFLAGS", "-Dwarnings");
        command.env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin");
        command.env("LC_ALL", "C");
    }
}

#[derive(Debug)]
struct CommandResult {
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    stdout_truncated: bool,
    stderr_truncated: bool,
    timed_out: bool,
}

impl CommandResult {
    fn success(&self) -> bool {
        self.status.success() && !self.timed_out && !self.stdout_truncated && !self.stderr_truncated
    }
}

#[allow(clippy::too_many_arguments)]
fn run_exact(
    program: &Path,
    args: &[OsString],
    stdin: Option<&[u8]>,
    current_dir: &Path,
    environment: &CleanEnvironment,
    timeout: Duration,
    max_output_bytes: usize,
) -> Result<CommandResult, String> {
    if !program.is_absolute() {
        return Err("verifier program path must be absolute".to_string());
    }
    let mut command = Command::new("/usr/bin/sandbox-exec");
    command
        .arg("-p")
        .arg(&environment.sandbox_profile)
        .arg(program)
        .args(args)
        .current_dir(current_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    environment.apply(&mut command);
    let mut child = command.spawn().map_err(|error| error.to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "missing verifier stdout".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "missing verifier stderr".to_string())?;
    let deadline = Instant::now() + timeout;
    let stdout_reader = spawn_deadline_capture(stdout, max_output_bytes, deadline);
    let stderr_reader = spawn_deadline_capture(stderr, max_output_bytes, deadline);
    if let Some(input) = stdin {
        child
            .stdin
            .take()
            .ok_or_else(|| "missing verifier stdin".to_string())?
            .write_all(input)
            .map_err(|error| error.to_string())?;
    } else {
        drop(child.stdin.take());
    }
    let (status, timed_out) = loop {
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            break (status, false);
        }
        if Instant::now() >= deadline {
            // The verifier owns the direct child only. Never issue a
            // negative-PID signal here: an adversarial fixture must not be
            // able to turn a timeout into a signal against the controller's
            // inherited process group. The process-tree fixture's golden
            // runner explicitly terminates its recorded helper descendants.
            child.kill().map_err(|error| error.to_string())?;
            let status = child.wait().map_err(|error| error.to_string())?;
            break (status, true);
        }
        thread::sleep(Duration::from_millis(10));
    };
    let (stdout, stdout_truncated, stdout_timeout) = stdout_reader
        .join()
        .map_err(|_| "stdout reader panicked".to_string())??;
    let (stderr, stderr_truncated, stderr_timeout) = stderr_reader
        .join()
        .map_err(|_| "stderr reader panicked".to_string())??;
    Ok(CommandResult {
        status,
        stdout,
        stderr,
        stdout_truncated,
        stderr_truncated,
        timed_out: timed_out || stdout_timeout || stderr_timeout,
    })
}

fn sandbox_profile(forbidden_roots: &[PathBuf]) -> String {
    let mut profile = "(version 1) (allow default) (deny network*)".to_string();
    for root in forbidden_roots {
        let escaped = root
            .to_string_lossy()
            .replace('\\', "\\\\")
            .replace('"', "\\\"");
        profile.push_str(&format!(
            " (deny file-read* (subpath \"{escaped}\")) (deny file-write* (subpath \"{escaped}\"))"
        ));
    }
    profile
}

/// A descendant retaining stdout must not extend the verifier deadline or turn
/// its eventual natural exit into evidence that the candidate cleaned it up.
type DeadlineCapture = Result<(Vec<u8>, bool, bool), String>;

fn spawn_deadline_capture<R>(
    mut reader: R,
    limit: usize,
    deadline: Instant,
) -> thread::JoinHandle<DeadlineCapture>
where
    R: Read + std::os::fd::AsRawFd + Send + 'static,
{
    thread::spawn(move || {
        #[cfg(any(target_os = "macos", target_os = "freebsd"))]
        let nonblocking = 0x4;
        #[cfg(target_os = "linux")]
        let nonblocking = 0x800;
        #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "freebsd")))]
        return Err("bounded verifier pipes are unsupported on this platform".into());
        #[cfg(any(target_os = "macos", target_os = "linux", target_os = "freebsd"))]
        {
            let fd = reader.as_raw_fd();
            let flags = unsafe { fcntl(fd, 3) };
            if flags < 0 || unsafe { fcntl(fd, 4, flags | nonblocking) } < 0 {
                return Err("cannot bound verifier pipe reads".into());
            }
            let mut retained = Vec::new();
            let mut truncated = false;
            let mut chunk = [0u8; 8192];
            loop {
                if Instant::now() >= deadline {
                    return Ok((retained, truncated, true));
                }
                match reader.read(&mut chunk) {
                    Ok(0) => return Ok((retained, truncated, false)),
                    Ok(count) => {
                        let keep = limit.saturating_sub(retained.len()).min(count);
                        retained.extend_from_slice(&chunk[..keep]);
                        truncated |= keep < count;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5))
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(error) => return Err(error.to_string()),
                }
            }
        }
    })
}

#[cfg(test)]
fn spawn_capture<R>(
    mut reader: R,
    limit: usize,
) -> thread::JoinHandle<Result<(Vec<u8>, bool), String>>
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut retained = Vec::new();
        let mut truncated = false;
        let mut chunk = [0_u8; 8 * 1024];
        loop {
            let count = reader.read(&mut chunk).map_err(|error| error.to_string())?;
            if count == 0 {
                break;
            }
            let remaining = limit.saturating_sub(retained.len());
            let retained_count = remaining.min(count);
            retained.extend_from_slice(&chunk[..retained_count]);
            truncated |= retained_count < count;
        }
        Ok((retained, truncated))
    })
}

fn resolve_executable(name: &str) -> Result<PathBuf, String> {
    if let Some(configured) = std::env::var_os(name.to_ascii_uppercase()) {
        let configured = PathBuf::from(configured);
        if configured.is_absolute() && configured.is_file() {
            return Ok(configured);
        }
    }
    let search_path = std::env::var_os("PATH").ok_or_else(|| "PATH is unavailable".to_string())?;
    for directory in std::env::split_paths(&search_path) {
        let candidate = directory.join(name);
        if candidate.is_absolute() && candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(format!("unable to resolve absolute {name} executable"))
}

fn require_network_sandbox() -> Result<(), String> {
    let path = Path::new("/usr/bin/sandbox-exec");
    if cfg!(target_os = "macos") && path.is_file() {
        Ok(())
    } else {
        Err("reasoning pilot requires macOS sandbox-exec network isolation".to_string())
    }
}

struct PrivateWorkRoot {
    path: PathBuf,
    marker_device: u64,
    marker_inode: u64,
}

impl PrivateWorkRoot {
    fn create(repository_root: &Path) -> Result<Self, String> {
        let temp = std::env::temp_dir()
            .canonicalize()
            .map_err(|error| format!("invalid temporary directory: {error}"))?;
        if temp.starts_with(repository_root) || repository_root.starts_with(&temp) {
            return Err("temporary work root overlaps repository".to_string());
        }
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_nanos();
        let path = temp.join(format!(
            "reasoning-pilot-hidden-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&path).map_err(|error| error.to_string())?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700))
            .map_err(|error| error.to_string())?;
        let marker = path.join(".reasoning-pilot-owner");
        let mut marker_file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&marker)
            .map_err(|error| error.to_string())?;
        writeln!(marker_file, "pid={}", std::process::id()).map_err(|error| error.to_string())?;
        marker_file.sync_all().map_err(|error| error.to_string())?;
        let metadata = marker.metadata().map_err(|error| error.to_string())?;
        Ok(Self {
            path,
            marker_device: metadata.dev(),
            marker_inode: metadata.ino(),
        })
    }
}

impl Drop for PrivateWorkRoot {
    fn drop(&mut self) {
        let Some(name) = self.path.file_name().and_then(OsStr::to_str) else {
            return;
        };
        if !name.starts_with("reasoning-pilot-hidden-") {
            return;
        }
        let marker = self.path.join(".reasoning-pilot-owner");
        let Ok(metadata) = fs::symlink_metadata(marker) else {
            return;
        };
        if metadata.file_type().is_file()
            && metadata.dev() == self.marker_device
            && metadata.ino() == self.marker_inode
        {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

const SIGKILL: i32 = 9;
const EPERM: i32 = 1;

extern "C" {
    fn getpid() -> i32;
    fn getpgrp() -> i32;
    fn getpgid(pid: i32) -> i32;
    fn kill(pid: i32, signal: i32) -> i32;
    fn fcntl(fd: i32, command: i32, ...) -> i32;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hrs253_pipe_writer_cannot_extend_verifier_deadline() {
        use std::os::unix::net::UnixStream;
        let (reader, mut writer) = UnixStream::pair().unwrap();
        writer.write_all(b"bounded output").unwrap();
        let start = Instant::now();
        let result = spawn_deadline_capture(reader, 7, start + Duration::from_millis(100))
            .join()
            .unwrap()
            .unwrap();
        assert_eq!(result, (b"bounded".to_vec(), true, true));
        assert!(start.elapsed() < Duration::from_secs(2));
        drop(writer);
    }

    #[test]
    fn hrs253_pid_evidence_rejects_broadcast_signals_and_duplicate_fields() {
        assert_eq!(
            parse_descendant_metadata("helper=12\ngroup=10\ndescendant=13"),
            Ok(13)
        );
        for text in [
            "helper=12\ngroup=10\ndescendant=-1",
            "helper=12\ngroup=10\ndescendant=0",
            "helper=12\ngroup=10\ndescendant=1",
            "helper=12\ngroup=10\ndescendant=13\ndescendant=14",
            "descendant=13",
        ] {
            assert!(parse_descendant_metadata(text).is_err());
        }
    }

    #[test]
    fn fixture_inventory_is_stable_and_unique() {
        let mut ids = FIXTURE_IDS.to_vec();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 6);
        assert_eq!(
            expected_failure_code("unicode_parser_boundary"),
            "unicode_mid_scalar"
        );
    }

    #[test]
    fn output_capture_is_bounded_while_draining() {
        let bytes = vec![b'x'; 32];
        let reader = spawn_capture(std::io::Cursor::new(bytes), 8);
        let (captured, truncated) = reader.join().unwrap().unwrap();
        assert_eq!(captured, b"xxxxxxxx");
        assert!(truncated);
    }
}
