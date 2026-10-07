//! Deterministic, shell-free release-readiness auditing.
//!
//! Static repository checks run before the fixed validation matrix. A dirty or
//! incomplete checkout therefore produces a receipt without starting Cargo.

use anyhow::Error as AnyhowError;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Read};
use std::path::{Component, Path};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const RECEIPT_SCHEMA: &str = "codex-team.release-readiness.v1";
const REQUIRED_RUST_VERSION: &str = "1.97.1";
const MAX_GIT_OUTPUT_BYTES: usize = 8 * 1024 * 1024;
const MAX_SMALL_FILE_BYTES: u64 = 1024 * 1024;
const MAX_DOC_FILE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_LOCK_FILE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_ACTIVE_PLANS: usize = 128;
const MAX_SNAPSHOT_ENTRIES: usize = 200_000;
const MAX_SNAPSHOT_DEPTH: usize = 64;
const GATE_POLL_INTERVAL: Duration = Duration::from_millis(25);
const GATE_TERMINATION_GRACE: Duration = Duration::from_secs(1);
const RELEASE_ARTIFACTS: &[(&str, &str)] = &[
    ("ca", "target/release/ca"),
    ("codex_team", "target/release/codex-team"),
];

#[derive(Debug, Clone, Serialize, Eq, PartialEq)]
pub struct ReleaseInputDigest {
    path: String,
    sha256: String,
}

#[derive(Debug, Clone, Serialize, Eq, PartialEq)]
pub struct ReleaseArtifactDigest {
    path: String,
    byte_size: u64,
    sha256: String,
}

#[derive(Debug, Clone, Copy, Serialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
enum CheckStatus {
    Passed,
    Failed,
    TimedOut,
    Infrastructure,
    Skipped,
}

#[derive(Debug, Clone, Serialize, Eq, PartialEq)]
pub struct ReleaseReadinessCheck {
    id: String,
    status: CheckStatus,
    summary: String,
}

#[derive(Debug, Clone, Serialize, Eq, PartialEq)]
pub struct ReleaseReadinessReceipt {
    schema: String,
    audit_complete: bool,
    ready: bool,
    source_revision: String,
    working_tree_digest: String,
    tracked_paths_digest: String,
    inputs: Vec<ReleaseInputDigest>,
    artifacts: Vec<ReleaseArtifactDigest>,
    checks: Vec<ReleaseReadinessCheck>,
    blockers: Vec<String>,
    summary: String,
    receipt_digest: String,
}

impl ReleaseReadinessReceipt {
    pub fn exit_code(&self) -> u8 {
        if !self.audit_complete {
            2
        } else if self.ready {
            0
        } else {
            1
        }
    }

    pub fn summary(&self) -> &str {
        &self.summary
    }
}

#[derive(Debug)]
pub struct ReleaseReadinessExit {
    code: u8,
    summary: String,
}

impl ReleaseReadinessExit {
    pub fn from_receipt(receipt: &ReleaseReadinessReceipt) -> Option<Self> {
        let code = receipt.exit_code();
        (code != 0).then(|| Self {
            code,
            summary: receipt.summary.clone(),
        })
    }
}

impl fmt::Display for ReleaseReadinessExit {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.summary)
    }
}

impl std::error::Error for ReleaseReadinessExit {}

pub fn requested_exit_code(error: &AnyhowError) -> Option<u8> {
    error
        .downcast_ref::<ReleaseReadinessExit>()
        .map(|exit| exit.code)
}

#[derive(Debug, Clone, Copy)]
pub struct FixedGate {
    pub id: &'static str,
    pub program: &'static str,
    pub args: &'static [&'static str],
    pub cargo_offline: bool,
    pub timeout_seconds: u64,
}

const FIXED_GATES: &[FixedGate] = &[
    FixedGate {
        id: "format",
        program: "cargo",
        args: &["fmt", "--all", "--", "--check"],
        cargo_offline: true,
        timeout_seconds: 300,
    },
    FixedGate {
        id: "clippy_default",
        program: "cargo",
        args: &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--locked",
            "--offline",
            "--",
            "-D",
            "warnings",
        ],
        cargo_offline: true,
        timeout_seconds: 900,
    },
    FixedGate {
        id: "clippy_all_features",
        program: "cargo",
        args: &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--all-features",
            "--locked",
            "--offline",
            "--",
            "-D",
            "warnings",
        ],
        cargo_offline: true,
        timeout_seconds: 900,
    },
    FixedGate {
        id: "test_default",
        program: "cargo",
        args: &[
            "test",
            "--workspace",
            "--locked",
            "--offline",
            "--no-fail-fast",
        ],
        cargo_offline: true,
        timeout_seconds: 1_800,
    },
    FixedGate {
        id: "test_all_features",
        program: "cargo",
        args: &[
            "test",
            "--workspace",
            "--all-targets",
            "--all-features",
            "--locked",
            "--offline",
            "--no-fail-fast",
        ],
        cargo_offline: true,
        timeout_seconds: 1_800,
    },
    FixedGate {
        id: "release_default",
        program: "cargo",
        args: &["build", "--workspace", "--release", "--locked", "--offline"],
        cargo_offline: true,
        timeout_seconds: 1_200,
    },
    FixedGate {
        id: "release_all_features",
        program: "cargo",
        args: &[
            "build",
            "--workspace",
            "--release",
            "--all-features",
            "--locked",
            "--offline",
        ],
        cargo_offline: true,
        timeout_seconds: 1_200,
    },
    FixedGate {
        id: "ca_help",
        program: "target/release/ca",
        args: &["--help"],
        cargo_offline: false,
        timeout_seconds: 60,
    },
    FixedGate {
        id: "ca_version",
        program: "target/release/ca",
        args: &["--version"],
        cargo_offline: false,
        timeout_seconds: 60,
    },
    FixedGate {
        id: "codex_team_help",
        program: "target/release/codex-team",
        args: &["--help"],
        cargo_offline: false,
        timeout_seconds: 60,
    },
    FixedGate {
        id: "codex_team_version",
        program: "target/release/codex-team",
        args: &["--version"],
        cargo_offline: false,
        timeout_seconds: 60,
    },
    FixedGate {
        id: "diff_hygiene",
        program: "git",
        args: &["diff", "--check"],
        cargo_offline: false,
        timeout_seconds: 60,
    },
];

/// The host engineering runner shares the same fixed release matrix.
pub fn fixed_release_gates() -> &'static [FixedGate] {
    FIXED_GATES
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum GateOutcome {
    Passed,
    Failed,
    TimedOut,
    Infrastructure,
}

trait GateRunner {
    fn run(&self, root: &Path, gate: &FixedGate) -> GateOutcome;
}

struct SystemGateRunner;

impl GateRunner for SystemGateRunner {
    fn run(&self, root: &Path, gate: &FixedGate) -> GateOutcome {
        run_gate_process(
            root,
            Path::new(gate.program),
            gate.args,
            gate.cargo_offline,
            Duration::from_secs(gate.timeout_seconds),
            &[],
        )
    }
}

fn run_gate_process(
    root: &Path,
    program: &Path,
    args: &[&str],
    cargo_offline: bool,
    timeout: Duration,
    extra_env: &[(&str, &OsStr)],
) -> GateOutcome {
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if cargo_offline {
        command
            .env("CARGO_NET_OFFLINE", "true")
            .env("CARGO_INCREMENTAL", "0");
    }
    for (name, value) in extra_env {
        command.env(name, value);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(_) => return GateOutcome::Infrastructure,
    };
    let child_id = child.id();
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if terminate_gate_process_group(&mut child, child_id, true, GATE_TERMINATION_GRACE)
                    .is_err()
                {
                    return GateOutcome::Infrastructure;
                }
                return if status.success() {
                    GateOutcome::Passed
                } else {
                    GateOutcome::Failed
                };
            }
            Ok(None) if started.elapsed() >= timeout => {
                return if terminate_gate_process_group(
                    &mut child,
                    child_id,
                    false,
                    GATE_TERMINATION_GRACE,
                )
                .is_ok()
                {
                    GateOutcome::TimedOut
                } else {
                    GateOutcome::Infrastructure
                };
            }
            Ok(None) => thread::sleep(GATE_POLL_INTERVAL),
            Err(_) => {
                let _ = terminate_gate_process_group(
                    &mut child,
                    child_id,
                    false,
                    GATE_TERMINATION_GRACE,
                );
                return GateOutcome::Infrastructure;
            }
        }
    }
}

#[cfg(unix)]
fn terminate_gate_process_group(
    child: &mut Child,
    child_id: u32,
    mut child_reaped: bool,
    grace: Duration,
) -> io::Result<()> {
    signal_gate_process_group(child_id, libc::SIGTERM)?;
    if wait_for_gate_process_group(child, child_id, &mut child_reaped, grace)? {
        return Ok(());
    }

    signal_gate_process_group(child_id, libc::SIGKILL)?;
    if wait_for_gate_process_group(child, child_id, &mut child_reaped, grace)? {
        return Ok(());
    }
    Err(io::Error::new(
        io::ErrorKind::TimedOut,
        "release gate process group survived cleanup",
    ))
}

#[cfg(unix)]
fn wait_for_gate_process_group(
    child: &mut Child,
    child_id: u32,
    child_reaped: &mut bool,
    grace: Duration,
) -> io::Result<bool> {
    let started = Instant::now();
    loop {
        if !*child_reaped {
            *child_reaped = child.try_wait()?.is_some();
        }
        if *child_reaped && !gate_process_group_exists(child_id)? {
            return Ok(true);
        }
        if started.elapsed() >= grace {
            return Ok(false);
        }
        thread::sleep(GATE_POLL_INTERVAL);
    }
}

#[cfg(unix)]
fn signal_gate_process_group(child_id: u32, signal: i32) -> io::Result<()> {
    let process_group = i32::try_from(child_id)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid process group"))?;
    if process_group <= 0 || process_group == unsafe { libc::getpgrp() } {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "unsafe release gate process group",
        ));
    }
    let result = unsafe { libc::kill(-process_group, signal) };
    if result == 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) {
        Ok(())
    } else {
        Err(error)
    }
}

#[cfg(unix)]
fn gate_process_group_exists(child_id: u32) -> io::Result<bool> {
    let process_group = i32::try_from(child_id)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid process group"))?;
    let result = unsafe { libc::kill(-process_group, 0) };
    if result == 0 {
        return Ok(true);
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) {
        Ok(false)
    } else {
        Err(error)
    }
}

#[cfg(not(unix))]
fn terminate_gate_process_group(
    child: &mut Child,
    _child_id: u32,
    child_reaped: bool,
    _grace: Duration,
) -> io::Result<()> {
    if child_reaped {
        return Ok(());
    }
    match child.kill() {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::InvalidInput => {}
        Err(error) => return Err(error),
    }
    child.wait().map(|_| ())
}

#[derive(Default)]
struct AuditState {
    audit_complete: bool,
    source_revision: String,
    working_tree_digest: String,
    tracked_paths_digest: String,
    inputs: BTreeMap<String, String>,
    artifacts: BTreeMap<String, ReleaseArtifactDigest>,
    checks: Vec<ReleaseReadinessCheck>,
    blockers: BTreeSet<String>,
}

impl AuditState {
    fn new() -> Self {
        Self {
            audit_complete: true,
            source_revision: "unknown".to_string(),
            working_tree_digest: sha256_hex(&[]),
            tracked_paths_digest: sha256_hex(&[]),
            ..Self::default()
        }
    }

    fn check(&mut self, id: impl Into<String>, passed: bool, summary: impl Into<String>) {
        let id = id.into();
        if !passed {
            self.blockers.insert(id.clone());
        }
        self.checks.push(ReleaseReadinessCheck {
            id,
            status: if passed {
                CheckStatus::Passed
            } else {
                CheckStatus::Failed
            },
            summary: summary.into(),
        });
    }

    fn skipped(&mut self, id: impl Into<String>, summary: impl Into<String>) {
        self.checks.push(ReleaseReadinessCheck {
            id: id.into(),
            status: CheckStatus::Skipped,
            summary: summary.into(),
        });
    }

    fn outcome(&mut self, id: impl Into<String>, status: CheckStatus, summary: impl Into<String>) {
        let id = id.into();
        if status != CheckStatus::Passed && status != CheckStatus::Skipped {
            self.blockers.insert(id.clone());
        }
        self.checks.push(ReleaseReadinessCheck {
            id,
            status,
            summary: summary.into(),
        });
    }

    fn infrastructure_failure(&mut self, code: &'static str) {
        self.audit_complete = false;
        self.check("audit_infrastructure", false, code);
    }

    fn finalize(mut self) -> ReleaseReadinessReceipt {
        self.checks.sort_by(|left, right| left.id.cmp(&right.id));
        let inputs = self
            .inputs
            .into_iter()
            .map(|(path, sha256)| ReleaseInputDigest { path, sha256 })
            .collect::<Vec<_>>();
        let artifacts = self.artifacts.into_values().collect::<Vec<_>>();
        let blockers = self.blockers.into_iter().collect::<Vec<_>>();
        let ready = self.audit_complete && blockers.is_empty();
        let summary = if !self.audit_complete {
            "release readiness audit could not complete".to_string()
        } else if ready {
            "release candidate passed all readiness checks".to_string()
        } else {
            format!("{} release readiness checks failed", blockers.len())
        };
        let mut receipt = ReleaseReadinessReceipt {
            schema: RECEIPT_SCHEMA.to_string(),
            audit_complete: self.audit_complete,
            ready,
            source_revision: self.source_revision,
            working_tree_digest: self.working_tree_digest,
            tracked_paths_digest: self.tracked_paths_digest,
            inputs,
            artifacts,
            checks: self.checks,
            blockers,
            summary,
            receipt_digest: String::new(),
        };
        receipt.receipt_digest = receipt_digest(&receipt);
        receipt
    }
}

pub fn audit_release_readiness(repo: &Path) -> ReleaseReadinessReceipt {
    audit_with_runner(repo, &SystemGateRunner)
}

fn audit_with_runner(repo: &Path, runner: &dyn GateRunner) -> ReleaseReadinessReceipt {
    let mut state = AuditState::new();
    let root = match fs::canonicalize(repo) {
        Ok(root) if root.is_dir() => root,
        _ => {
            state.infrastructure_failure("repository_root_unavailable");
            skip_fixed_gates(&mut state, "static audit did not complete");
            skip_release_artifacts(&mut state, "static audit did not complete");
            return state.finalize();
        }
    };

    let start_status = match collect_static_checks(&root, &mut state) {
        Ok(status) => status,
        Err(code) => {
            state.infrastructure_failure(code);
            skip_fixed_gates(&mut state, "static audit did not complete");
            skip_release_artifacts(&mut state, "static audit did not complete");
            return state.finalize();
        }
    };

    let pre_gate_status = match git_stdout(
        &root,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
    ) {
        Ok(status) => status,
        Err(code) => {
            state.infrastructure_failure(code);
            skip_fixed_gates(&mut state, "source stability could not be verified");
            skip_release_artifacts(&mut state, "source stability could not be verified");
            return state.finalize();
        }
    };
    let source_stable = start_status == pre_gate_status;
    state.check(
        "source_stable_before_gates",
        source_stable,
        if source_stable {
            "release inputs stayed stable during static audit"
        } else {
            "release inputs changed during static audit"
        },
    );

    if !state.blockers.is_empty() {
        skip_fixed_gates(&mut state, "static release prerequisites failed");
        skip_release_artifacts(&mut state, "static release prerequisites failed");
        return state.finalize();
    }

    let mut gate_failed = false;
    for gate in FIXED_GATES {
        if gate_failed {
            state.skipped(
                format!("gate_{}", gate.id),
                "an earlier fixed release gate failed",
            );
            continue;
        }
        match runner.run(&root, gate) {
            GateOutcome::Passed => state.outcome(
                format!("gate_{}", gate.id),
                CheckStatus::Passed,
                "fixed release gate passed",
            ),
            GateOutcome::Failed => {
                state.outcome(
                    format!("gate_{}", gate.id),
                    CheckStatus::Failed,
                    "fixed release gate failed",
                );
                gate_failed = true;
            }
            GateOutcome::TimedOut => {
                state.outcome(
                    format!("gate_{}", gate.id),
                    CheckStatus::TimedOut,
                    "fixed release gate timed out",
                );
                gate_failed = true;
            }
            GateOutcome::Infrastructure => {
                state.outcome(
                    format!("gate_{}", gate.id),
                    CheckStatus::Infrastructure,
                    "fixed release gate could not be executed safely",
                );
                state.infrastructure_failure("fixed_gate_runner_failed");
                gate_failed = true;
            }
        }
    }

    if gate_failed {
        skip_release_artifacts(&mut state, "fixed release gates did not pass");
    } else {
        inspect_release_artifacts(&root, &mut state);
    }

    if state.audit_complete {
        match git_stdout(
            &root,
            &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
        ) {
            Ok(status) => state.check(
                "working_tree_clean_after_gates",
                status.is_empty(),
                if status.is_empty() {
                    "fixed release gates left the checkout clean"
                } else {
                    "fixed release gates changed the checkout"
                },
            ),
            Err(code) => state.infrastructure_failure(code),
        }
    }
    state.finalize()
}

fn collect_static_checks(root: &Path, state: &mut AuditState) -> Result<Vec<u8>, &'static str> {
    let revision = git_stdout(root, &["rev-parse", "--verify", "HEAD"])?;
    let revision = String::from_utf8(revision).map_err(|_| "git_revision_not_utf8")?;
    let revision = revision.trim();
    if (!matches!(revision.len(), 40 | 64))
        || !revision.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("git_revision_invalid");
    }
    state.source_revision = revision.to_ascii_lowercase();
    state.check("git_repository", true, "repository HEAD resolved");

    let status = git_stdout(
        root,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
    )?;
    state.working_tree_digest = sha256_hex(&status);
    state.inputs.insert(
        "git/working-tree-status-v1".to_string(),
        state.working_tree_digest.clone(),
    );
    state.check(
        "working_tree_clean",
        status.is_empty(),
        if status.is_empty() {
            "tracked and untracked release inputs are clean"
        } else {
            "tracked or untracked release inputs are dirty"
        },
    );

    let tracked = git_stdout(root, &["ls-files", "-z"])?;
    state.tracked_paths_digest = sha256_hex(&tracked);
    state.inputs.insert(
        "git/tracked-paths-v1".to_string(),
        state.tracked_paths_digest.clone(),
    );
    let tracked_entries = nul_entries(&tracked).collect::<Vec<_>>();

    let root_manifest = read_input(root, "Cargo.toml", MAX_SMALL_FILE_BYTES, state)?;
    let lock = read_input(root, "Cargo.lock", MAX_LOCK_FILE_BYTES, state)?;
    let toolchain = read_input(root, "rust-toolchain.toml", MAX_SMALL_FILE_BYTES, state)?;
    let workflow = read_input(
        root,
        ".github/workflows/ci.yml",
        MAX_SMALL_FILE_BYTES,
        state,
    )?;

    let mut required_paths = vec![
        "Cargo.toml".to_string(),
        "Cargo.lock".to_string(),
        "rust-toolchain.toml".to_string(),
        ".github/workflows/ci.yml".to_string(),
        "NEXT_STEPS.md".to_string(),
        "docs/.INDEX.md".to_string(),
    ];
    let workspace_members = root_manifest
        .as_deref()
        .and_then(parse_workspace_members)
        .unwrap_or_default();
    let workspace_shape_valid = !workspace_members.is_empty();
    for member in &workspace_members {
        if let Some(manifest) = workspace_member_manifest(member) {
            if manifest != "Cargo.toml" {
                required_paths.push(manifest.clone());
                let _ = read_input(root, &manifest, MAX_SMALL_FILE_BYTES, state)?;
            }
        }
    }
    required_paths.sort();
    required_paths.dedup();
    let required_tracked = workspace_shape_valid
        && required_paths
            .iter()
            .all(|path| tracked_contains(&tracked_entries, path));
    state.check(
        "workspace_members_tracked",
        required_tracked,
        if required_tracked {
            "workspace manifests and release control files are tracked"
        } else {
            "a workspace member or release control file is not tracked"
        },
    );

    let inputs_present =
        root_manifest.is_some() && lock.is_some() && toolchain.is_some() && workflow.is_some();
    state.check(
        "required_release_inputs",
        inputs_present,
        if inputs_present {
            "lockfile, workflow, toolchain, and root manifest are regular bounded files"
        } else {
            "a required release input is missing, linked, oversized, or unreadable"
        },
    );

    let toolchain_valid = toolchain
        .as_deref()
        .is_some_and(toolchain_contract_is_valid);
    state.check(
        "toolchain_contract",
        toolchain_valid,
        if toolchain_valid {
            "Rust 1.97.1 with minimal rustfmt and Clippy is pinned"
        } else {
            "rust-toolchain.toml does not match the release pin"
        },
    );

    let workflow_valid = workflow.as_deref().is_some_and(workflow_contract_is_valid);
    state.check(
        "workflow_contract",
        workflow_valid,
        if workflow_valid {
            "root CI is read-only, SHA-pinned, and runs the release contract"
        } else {
            "root CI is missing a pinned or required release control"
        },
    );

    let protected_untracked = !tracked_entries.iter().any(|entry| {
        *entry == b"oracles"
            || entry.starts_with(b"oracles/")
            || *entry == b"reference"
            || entry.starts_with(b"reference/")
    });
    state.check(
        "protected_trees_untracked",
        protected_untracked,
        if protected_untracked {
            "oracle and reference trees are absent from tracked release source"
        } else {
            "oracle or reference content is tracked in the release source"
        },
    );

    let active_plans_consistent =
        active_plan_contract_is_valid(root, state, &tracked_entries, &mut required_paths)?;
    state.check(
        "active_plan_consistency",
        active_plans_consistent,
        if active_plans_consistent {
            "active plans have matching queue and index records"
        } else {
            "an active plan is missing a matching queue, index, status, or tracked record"
        },
    );

    let snapshots_clean = !snapshot_residue_exists(root)?;
    state.check(
        "snapshot_hygiene",
        snapshots_clean,
        if snapshots_clean {
            "no pending .snap.new files exist"
        } else {
            "pending .snap.new files exist"
        },
    );

    let runtime_state_absent = match fs::symlink_metadata(root.join(".orchestrator")) {
        Ok(_) => false,
        Err(error) if error.kind() == io::ErrorKind::NotFound => true,
        Err(_) => return Err("runtime_state_metadata_failed"),
    };
    state.check(
        "runtime_state_absent",
        runtime_state_absent,
        if runtime_state_absent {
            "repository-local runtime state is absent"
        } else {
            "repository-local runtime state exists"
        },
    );

    Ok(status)
}

fn skip_fixed_gates(state: &mut AuditState, reason: &'static str) {
    for gate in FIXED_GATES {
        state.skipped(format!("gate_{}", gate.id), reason);
    }
}

fn skip_release_artifacts(state: &mut AuditState, reason: &'static str) {
    for (id, _) in RELEASE_ARTIFACTS {
        state.skipped(format!("artifact_{id}"), reason);
    }
}

fn inspect_release_artifacts(root: &Path, state: &mut AuditState) {
    for (id, relative) in RELEASE_ARTIFACTS {
        match release_artifact_digest(root, relative) {
            Some(artifact) => {
                state.artifacts.insert((*relative).to_string(), artifact);
                state.check(
                    format!("artifact_{id}"),
                    true,
                    "release artifact is a regular file with recorded size and SHA-256",
                );
            }
            None => state.check(
                format!("artifact_{id}"),
                false,
                "release artifact is missing, linked, non-regular, or unreadable",
            ),
        }
    }
}

fn release_artifact_digest(root: &Path, relative: &str) -> Option<ReleaseArtifactDigest> {
    if !safe_relative_path(relative) {
        return None;
    }
    let path = root.join(relative);
    let before = fs::symlink_metadata(&path).ok()?;
    if before.file_type().is_symlink() || !before.is_file() {
        return None;
    }

    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options.open(&path).ok()?;
    let opened = file.metadata().ok()?;
    if !opened.is_file() || !same_artifact_file(&before, &opened) {
        return None;
    }

    let mut hasher = Sha256::new();
    let mut byte_size = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer).ok()?;
        if count == 0 {
            break;
        }
        byte_size = byte_size.checked_add(count as u64)?;
        hasher.update(&buffer[..count]);
    }
    if byte_size != opened.len() {
        return None;
    }

    let after = fs::symlink_metadata(&path).ok()?;
    if after.file_type().is_symlink()
        || !after.is_file()
        || !same_artifact_file(&opened, &after)
        || after.len() != byte_size
    {
        return None;
    }
    Some(ReleaseArtifactDigest {
        path: relative.to_string(),
        byte_size,
        sha256: format!("{:x}", hasher.finalize()),
    })
}

#[cfg(unix)]
fn same_artifact_file(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    left.dev() == right.dev()
        && left.ino() == right.ino()
        && left.len() == right.len()
        && left.mtime() == right.mtime()
        && left.mtime_nsec() == right.mtime_nsec()
        && left.ctime() == right.ctime()
        && left.ctime_nsec() == right.ctime_nsec()
}

#[cfg(not(unix))]
fn same_artifact_file(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    left.len() == right.len() && left.modified().ok() == right.modified().ok()
}

fn git_stdout(root: &Path, args: &[&str]) -> Result<Vec<u8>, &'static str> {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|_| "git_command_unavailable")?;
    if !output.status.success() {
        return Err("git_command_failed");
    }
    if output.stdout.len() > MAX_GIT_OUTPUT_BYTES {
        return Err("git_output_exceeded_limit");
    }
    Ok(output.stdout)
}

fn nul_entries(bytes: &[u8]) -> impl Iterator<Item = &[u8]> {
    bytes
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
}

fn tracked_contains(entries: &[&[u8]], path: &str) -> bool {
    entries.contains(&path.as_bytes())
}

fn read_input(
    root: &Path,
    relative: &str,
    max_bytes: u64,
    state: &mut AuditState,
) -> Result<Option<Vec<u8>>, &'static str> {
    if !safe_relative_path(relative) {
        return Ok(None);
    }
    let path = root.join(relative);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("release_input_metadata_failed"),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > max_bytes {
        return Ok(None);
    }
    let bytes = fs::read(path).map_err(|_| "release_input_read_failed")?;
    if bytes.len() as u64 > max_bytes {
        return Ok(None);
    }
    state
        .inputs
        .insert(relative.replace('\\', "/"), sha256_hex(&bytes));
    Ok(Some(bytes))
}

fn safe_relative_path(path: &str) -> bool {
    !path.is_empty()
        && Path::new(path)
            .components()
            .all(|component| matches!(component, Component::Normal(_) | Component::CurDir))
}

fn parse_workspace_members(bytes: &[u8]) -> Option<Vec<String>> {
    let text = std::str::from_utf8(bytes).ok()?;
    let value = text.parse::<toml::Value>().ok()?;
    let members = value
        .get("workspace")?
        .get("members")?
        .as_array()?
        .iter()
        .map(|member| member.as_str().map(str::to_string))
        .collect::<Option<Vec<_>>>()?;
    if members.is_empty()
        || members.len() > 64
        || members
            .iter()
            .any(|member| workspace_member_manifest(member).is_none())
    {
        return None;
    }
    let unique = members.iter().collect::<BTreeSet<_>>();
    (unique.len() == members.len()).then_some(members)
}

fn workspace_member_manifest(member: &str) -> Option<String> {
    if member == "." {
        return Some("Cargo.toml".to_string());
    }
    if !safe_relative_path(member)
        || member.contains('*')
        || member.contains('?')
        || member.contains('[')
    {
        return None;
    }
    Some(format!("{}/Cargo.toml", member.trim_end_matches('/')))
}

fn toolchain_contract_is_valid(bytes: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return false;
    };
    let Ok(value) = text.parse::<toml::Value>() else {
        return false;
    };
    let Some(toolchain) = value.get("toolchain") else {
        return false;
    };
    let channel = toolchain.get("channel").and_then(toml::Value::as_str);
    let profile = toolchain.get("profile").and_then(toml::Value::as_str);
    let Some(components) = toolchain.get("components").and_then(toml::Value::as_array) else {
        return false;
    };
    let Some(components) = components
        .iter()
        .map(toml::Value::as_str)
        .collect::<Option<BTreeSet<_>>>()
    else {
        return false;
    };
    channel == Some(REQUIRED_RUST_VERSION)
        && profile == Some("minimal")
        && components == BTreeSet::from(["clippy", "rustfmt"])
}

fn workflow_contract_is_valid(bytes: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return false;
    };
    let mut action_count = 0usize;
    for line in text.lines() {
        let line = line.trim();
        let value = line
            .strip_prefix("uses:")
            .or_else(|| line.strip_prefix("- uses:"));
        let Some(value) = value else {
            continue;
        };
        action_count += 1;
        let value = value
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .trim_matches(['\'', '"']);
        if value.starts_with("./") {
            continue;
        }
        let Some((_, revision)) = value.rsplit_once('@') else {
            return false;
        };
        if revision.len() != 40 || !revision.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return false;
        }
    }
    const REQUIRED_TEXT: &[&str] = &[
        "permissions:",
        "contents: read",
        "persist-credentials: false",
        "runs-on: ubuntu-24.04",
        "cargo fetch --locked",
        "cargo build --package codex-team --release --locked --offline",
        "target/release/codex-team verify release-readiness --repo . --json",
        "git status --porcelain=v1 --untracked-files=all",
    ];
    action_count > 0
        && !text.contains("permissions: write-all")
        && REQUIRED_TEXT.iter().all(|required| text.contains(required))
}

fn active_plan_contract_is_valid(
    root: &Path,
    state: &mut AuditState,
    tracked: &[&[u8]],
    required_paths: &mut Vec<String>,
) -> Result<bool, &'static str> {
    let next_steps = read_input(root, "NEXT_STEPS.md", MAX_DOC_FILE_BYTES, state)?;
    let index = read_input(root, "docs/.INDEX.md", MAX_DOC_FILE_BYTES, state)?;
    let (Some(next_steps), Some(index)) = (next_steps, index) else {
        return Ok(false);
    };
    let Ok(next_steps) = std::str::from_utf8(&next_steps) else {
        return Ok(false);
    };
    let Ok(index) = std::str::from_utf8(&index) else {
        return Ok(false);
    };
    let directory = root.join("docs/exec-plans/active");
    let metadata = match fs::symlink_metadata(&directory) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(_) => return Err("active_plan_directory_metadata_failed"),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Ok(false);
    }
    let mut entries = fs::read_dir(directory)
        .map_err(|_| "active_plan_directory_read_failed")?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "active_plan_entry_read_failed")?;
    entries.sort_by_key(|entry| entry.file_name());
    let plans = entries
        .into_iter()
        .filter(|entry| entry.path().extension().and_then(|value| value.to_str()) == Some("md"))
        .filter(|entry| entry.file_name() != "README.md")
        .collect::<Vec<_>>();
    if plans.len() > MAX_ACTIVE_PLANS {
        return Ok(false);
    }

    let mut valid = true;
    for entry in plans {
        let Some(file_name) = entry.file_name().to_str().map(str::to_string) else {
            valid = false;
            continue;
        };
        let Some(task_id) = task_id_from_plan_name(&file_name) else {
            valid = false;
            continue;
        };
        let relative = format!("docs/exec-plans/active/{file_name}");
        required_paths.push(relative.clone());
        let Some(bytes) = read_input(root, &relative, MAX_DOC_FILE_BYTES, state)? else {
            valid = false;
            continue;
        };
        let Ok(plan) = std::str::from_utf8(&bytes) else {
            valid = false;
            continue;
        };
        valid &= tracked_contains(tracked, &relative);
        valid &= plan.lines().any(|line| line.trim() == "Status: ACTIVE");
        valid &= queue_has_active_task(next_steps, &task_id);
        valid &= index.contains(&file_name);
    }
    Ok(valid)
}

fn task_id_from_plan_name(file_name: &str) -> Option<String> {
    let stem = file_name.strip_suffix(".md")?;
    let mut parts = stem.splitn(3, '-');
    let prefix = parts.next()?;
    let number = parts.next()?;
    let slug = parts.next()?;
    if prefix.len() < 2
        || prefix.len() > 8
        || !prefix.bytes().all(|byte| byte.is_ascii_uppercase())
        || number.is_empty()
        || !number.bytes().all(|byte| byte.is_ascii_digit())
        || slug.is_empty()
    {
        return None;
    }
    Some(format!("{prefix}-{number}"))
}

fn queue_has_active_task(next_steps: &str, task_id: &str) -> bool {
    let heading_prefix = format!("# {task_id}");
    let mut in_section = false;
    for line in next_steps.lines() {
        if line.starts_with('#') {
            if in_section {
                break;
            }
            in_section = line == heading_prefix
                || line
                    .strip_prefix(&heading_prefix)
                    .is_some_and(|suffix| suffix.starts_with(" — ") || suffix.starts_with(" - "));
            continue;
        }
        if in_section {
            let line = line.trim();
            if line.starts_with("Status:")
                && (line.contains("ACTIVE") || line.contains("IN PROGRESS"))
            {
                return true;
            }
        }
    }
    false
}

fn snapshot_residue_exists(root: &Path) -> Result<bool, &'static str> {
    let mut pending = vec![
        (root.join("src"), 0usize),
        (root.join("crates/codex-team"), 0usize),
    ];
    let mut visited = 0usize;
    while let Some((path, depth)) = pending.pop() {
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(_) => return Err("snapshot_scan_metadata_failed"),
        };
        if metadata.file_type().is_symlink() {
            if path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(".snap.new"))
            {
                return Ok(true);
            }
            continue;
        }
        visited += 1;
        if visited > MAX_SNAPSHOT_ENTRIES || depth > MAX_SNAPSHOT_DEPTH {
            return Err("snapshot_scan_exceeded_limit");
        }
        if metadata.is_file() {
            if path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(".snap.new"))
            {
                return Ok(true);
            }
            continue;
        }
        if !metadata.is_dir() {
            continue;
        }
        let entries = fs::read_dir(&path)
            .map_err(|_| "snapshot_scan_directory_failed")?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| "snapshot_scan_entry_failed")?;
        for entry in entries {
            if entry.file_name() == "target" || entry.file_name() == ".orchestrator" {
                continue;
            }
            pending.push((entry.path(), depth + 1));
        }
    }
    Ok(false)
}

fn receipt_digest(receipt: &ReleaseReadinessReceipt) -> String {
    #[derive(Serialize)]
    struct CanonicalReceipt<'a> {
        schema: &'a str,
        audit_complete: bool,
        ready: bool,
        source_revision: &'a str,
        working_tree_digest: &'a str,
        tracked_paths_digest: &'a str,
        inputs: &'a [ReleaseInputDigest],
        artifacts: &'a [ReleaseArtifactDigest],
        checks: &'a [ReleaseReadinessCheck],
        blockers: &'a [String],
        summary: &'a str,
    }
    let canonical = CanonicalReceipt {
        schema: &receipt.schema,
        audit_complete: receipt.audit_complete,
        ready: receipt.ready,
        source_revision: &receipt.source_revision,
        working_tree_digest: &receipt.working_tree_digest,
        tracked_paths_digest: &receipt.tracked_paths_digest,
        inputs: &receipt.inputs,
        artifacts: &receipt.artifacts,
        checks: &receipt.checks,
        blockers: &receipt.blockers,
        summary: &receipt.summary,
    };
    let bytes =
        serde_json::to_vec(&canonical).expect("release receipt serialization is infallible");
    let mut hasher = Sha256::new();
    hasher.update(b"codex-team.release-readiness.v1\0");
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};

    static FIXTURE_COUNTER: AtomicU64 = AtomicU64::new(0);

    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn ready(name: &str) -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos();
            let counter = FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "codex-team-release-readiness-{name}-{}-{nonce}-{counter}",
                std::process::id()
            ));
            fs::create_dir_all(&root).expect("create fixture root");
            write(
                &root,
                "Cargo.toml",
                "[workspace]\nmembers = [\".\", \"crates/codex-team\"]\nresolver = \"2\"\n\n[package]\nname = \"fixture-ca\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
            );
            write(&root, "Cargo.lock", "version = 4\n");
            write(
                &root,
                "crates/codex-team/Cargo.toml",
                "[package]\nname = \"fixture-codex-team\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
            );
            write(
                &root,
                "rust-toolchain.toml",
                "[toolchain]\nchannel = \"1.97.1\"\ncomponents = [\"clippy\", \"rustfmt\"]\nprofile = \"minimal\"\n",
            );
            write(&root, ".github/workflows/ci.yml", &fixture_workflow());
            write(
                &root,
                "NEXT_STEPS.md",
                "# Next Steps\n\n# REL-001 — Fixture\n\nStatus: IN PROGRESS\n",
            );
            write(
                &root,
                "docs/.INDEX.md",
                "docs/exec-plans/active/REL-001-fixture.md\n",
            );
            write(
                &root,
                "docs/exec-plans/active/REL-001-fixture.md",
                "# REL-001 — Fixture\n\nStatus: ACTIVE\n",
            );
            write(&root, ".gitignore", "/target/\n");
            git(&root, &["init", "-q", "--initial-branch=master"]);
            git(&root, &["add", "--all"]);
            let status = Command::new("git")
                .args([
                    "-c",
                    "user.name=Release Fixture",
                    "-c",
                    "user.email=fixture@example.invalid",
                    "commit",
                    "-q",
                    "-m",
                    "fixture",
                ])
                .current_dir(&root)
                .env("GIT_AUTHOR_DATE", "2000-01-01T00:00:00Z")
                .env("GIT_COMMITTER_DATE", "2000-01-01T00:00:00Z")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .expect("commit fixture");
            assert!(status.success());
            write_bytes(&root, "target/release/ca", b"fixture ca release binary\n");
            write_bytes(
                &root,
                "target/release/codex-team",
                b"fixture codex-team release binary\n",
            );
            Self { root }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[derive(Default)]
    struct FakeGateRunner {
        calls: RefCell<Vec<String>>,
        outcome: Option<(&'static str, GateOutcome)>,
    }

    impl GateRunner for FakeGateRunner {
        fn run(&self, _root: &Path, gate: &FixedGate) -> GateOutcome {
            self.calls.borrow_mut().push(gate.id.to_string());
            self.outcome
                .filter(|(id, _)| *id == gate.id)
                .map(|(_, outcome)| outcome)
                .unwrap_or(GateOutcome::Passed)
        }
    }

    #[test]
    fn ready_fixture_runs_fixed_gates_and_has_stable_digest() {
        let fixture = Fixture::ready("ready");
        let runner = FakeGateRunner::default();

        let first = audit_with_runner(&fixture.root, &runner);
        let second = audit_with_runner(&fixture.root, &runner);

        assert!(first.audit_complete);
        assert!(first.ready, "{first:#?}");
        assert_eq!(first.exit_code(), 0);
        assert_eq!(first.schema, RECEIPT_SCHEMA);
        assert_eq!(first.receipt_digest, second.receipt_digest);
        assert_eq!(first.artifacts.len(), RELEASE_ARTIFACTS.len());
        assert_eq!(first.artifacts[0].path, "target/release/ca");
        assert_eq!(first.artifacts[0].byte_size, 26);
        assert_eq!(first.artifacts[0].sha256.len(), 64);
        assert_eq!(runner.calls.borrow().len(), FIXED_GATES.len() * 2);
        assert!(!serde_json::to_string(&first)
            .unwrap()
            .contains(fixture.root.to_string_lossy().as_ref()));
    }

    #[test]
    fn dirty_fixture_skips_every_fixed_gate() {
        let fixture = Fixture::ready("dirty");
        write(&fixture.root, "untracked.txt", "dirty\n");
        let runner = FakeGateRunner::default();

        let receipt = audit_with_runner(&fixture.root, &runner);

        assert!(receipt.audit_complete);
        assert!(!receipt.ready);
        assert_eq!(receipt.exit_code(), 1);
        assert!(receipt.blockers.contains(&"working_tree_clean".to_string()));
        assert!(runner.calls.borrow().is_empty());
    }

    #[test]
    fn untracked_ignored_workspace_member_is_a_stable_blocker() {
        let fixture = Fixture::ready("untracked-member");
        git(
            &fixture.root,
            &["rm", "-q", "--cached", "crates/codex-team/Cargo.toml"],
        );
        write(
            &fixture.root,
            ".gitignore",
            "/target/\ncrates/codex-team/\n",
        );
        git(&fixture.root, &["add", ".gitignore"]);
        git_commit(&fixture.root, "ignore member");
        let runner = FakeGateRunner::default();

        let receipt = audit_with_runner(&fixture.root, &runner);

        assert!(receipt
            .blockers
            .contains(&"workspace_members_tracked".to_string()));
        assert!(runner.calls.borrow().is_empty());
    }

    #[test]
    fn workflow_action_tags_and_active_plan_drift_fail_static_audit() {
        let fixture = Fixture::ready("contract-drift");
        let workflow = fixture_workflow().replace(
            "actions/checkout@11bd71901bbe5b1630ceea73d27597364c9af683",
            "actions/checkout@v4",
        );
        write(&fixture.root, ".github/workflows/ci.yml", &workflow);
        write(&fixture.root, "NEXT_STEPS.md", "# Next Steps\n");
        let runner = FakeGateRunner::default();

        let receipt = audit_with_runner(&fixture.root, &runner);

        assert!(receipt.blockers.contains(&"workflow_contract".to_string()));
        assert!(receipt
            .blockers
            .contains(&"active_plan_consistency".to_string()));
        assert!(runner.calls.borrow().is_empty());
    }

    #[test]
    fn declared_file_input_mutations_change_receipt_and_fail_closed() {
        let workflow = fixture_workflow().replace(
            "actions/checkout@11bd71901bbe5b1630ceea73d27597364c9af683",
            "actions/checkout@v4",
        );
        let cases = [
            (
                "lockfile",
                "Cargo.lock",
                "version = 4\n# mutated\n".to_string(),
                "working_tree_clean",
            ),
            (
                "toolchain",
                "rust-toolchain.toml",
                "[toolchain]\nchannel = \"1.96.0\"\ncomponents = [\"clippy\", \"rustfmt\"]\nprofile = \"minimal\"\n".to_string(),
                "toolchain_contract",
            ),
            (
                "workflow",
                ".github/workflows/ci.yml",
                workflow,
                "workflow_contract",
            ),
            (
                "active-plan",
                "docs/exec-plans/active/REL-001-fixture.md",
                "# REL-001 — Fixture\n\nStatus: COMPLETE\n".to_string(),
                "active_plan_consistency",
            ),
        ];

        for (name, path, contents, blocker) in cases {
            let fixture = Fixture::ready(name);
            let runner = FakeGateRunner::default();
            let before = audit_with_runner(&fixture.root, &runner);
            assert!(before.ready, "baseline for {name}: {before:#?}");
            let before_input = input_sha256(&before, path).to_string();

            write(&fixture.root, path, &contents);
            let after = audit_with_runner(&fixture.root, &runner);

            assert!(!after.ready, "mutated {name}: {after:#?}");
            assert!(after.blockers.contains(&blocker.to_string()));
            assert_ne!(before_input, input_sha256(&after, path));
            assert_ne!(before.receipt_digest, after.receipt_digest);
            assert_eq!(runner.calls.borrow().len(), FIXED_GATES.len());
        }
    }

    #[test]
    fn pending_snapshot_and_tracked_oracle_fail_static_audit() {
        let fixture = Fixture::ready("protected-and-snapshot");
        write(&fixture.root, "src/view.snap.new", "pending\n");
        write(&fixture.root, "oracles/evidence.txt", "must not ship\n");
        git(&fixture.root, &["add", "--force", "oracles/evidence.txt"]);
        git_commit(&fixture.root, "track protected input");
        let runner = FakeGateRunner::default();

        let receipt = audit_with_runner(&fixture.root, &runner);

        assert!(receipt.blockers.contains(&"snapshot_hygiene".to_string()));
        assert!(receipt
            .blockers
            .contains(&"protected_trees_untracked".to_string()));
        assert!(runner.calls.borrow().is_empty());
    }

    #[test]
    fn fixed_gate_failure_is_not_an_infrastructure_error() {
        let fixture = Fixture::ready("gate-failure");
        let runner = FakeGateRunner {
            outcome: Some(("test_default", GateOutcome::Failed)),
            ..FakeGateRunner::default()
        };

        let receipt = audit_with_runner(&fixture.root, &runner);

        assert!(receipt.audit_complete);
        assert!(!receipt.ready);
        assert_eq!(receipt.exit_code(), 1);
        assert!(receipt.blockers.contains(&"gate_test_default".to_string()));
        assert!(receipt.artifacts.is_empty());
    }

    #[test]
    fn fixed_gate_timeout_is_a_completed_not_ready_audit() {
        let fixture = Fixture::ready("gate-timeout");
        let runner = FakeGateRunner {
            outcome: Some(("test_default", GateOutcome::TimedOut)),
            ..FakeGateRunner::default()
        };

        let receipt = audit_with_runner(&fixture.root, &runner);

        assert!(receipt.audit_complete);
        assert_eq!(receipt.exit_code(), 1);
        assert!(receipt.blockers.contains(&"gate_test_default".to_string()));
        assert_eq!(
            receipt
                .checks
                .iter()
                .find(|check| check.id == "gate_test_default")
                .map(|check| check.status),
            Some(CheckStatus::TimedOut)
        );
        assert!(receipt.artifacts.is_empty());
    }

    #[test]
    fn fixed_gate_runner_error_uses_exit_two_without_paths() {
        let fixture = Fixture::ready("gate-infrastructure");
        let runner = FakeGateRunner {
            outcome: Some(("format", GateOutcome::Infrastructure)),
            ..FakeGateRunner::default()
        };

        let receipt = audit_with_runner(&fixture.root, &runner);
        let exit = ReleaseReadinessExit::from_receipt(&receipt).unwrap();
        let error = AnyhowError::new(exit);

        assert!(!receipt.audit_complete);
        assert_eq!(receipt.exit_code(), 2);
        assert_eq!(requested_exit_code(&error), Some(2));
        assert!(!serde_json::to_string(&receipt)
            .unwrap()
            .contains(fixture.root.to_string_lossy().as_ref()));
    }

    #[test]
    fn same_size_release_artifact_mutation_changes_hash_and_receipt_digest() {
        let fixture = Fixture::ready("artifact-mutation");
        let runner = FakeGateRunner::default();
        let before = audit_with_runner(&fixture.root, &runner);
        let before_artifact = artifact(&before, "target/release/ca").clone();

        write_bytes(
            &fixture.root,
            "target/release/ca",
            b"mutated ca release binary\n",
        );
        let after = audit_with_runner(&fixture.root, &runner);
        let after_artifact = artifact(&after, "target/release/ca");

        assert!(before.ready && after.ready);
        assert_eq!(before_artifact.byte_size, after_artifact.byte_size);
        assert_ne!(before_artifact.sha256, after_artifact.sha256);
        assert_ne!(before.receipt_digest, after.receipt_digest);
    }

    #[test]
    fn missing_release_artifact_is_a_completed_blocker() {
        let fixture = Fixture::ready("artifact-missing");
        fs::remove_file(fixture.root.join("target/release/codex-team"))
            .expect("remove fixture artifact");
        let runner = FakeGateRunner::default();

        let receipt = audit_with_runner(&fixture.root, &runner);

        assert!(receipt.audit_complete);
        assert_eq!(receipt.exit_code(), 1);
        assert!(receipt
            .blockers
            .contains(&"artifact_codex_team".to_string()));
        assert_eq!(receipt.artifacts.len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn linked_release_artifact_is_a_completed_blocker() {
        use std::os::unix::fs::symlink;

        let fixture = Fixture::ready("artifact-symlink");
        let artifact_path = fixture.root.join("target/release/codex-team");
        fs::remove_file(&artifact_path).expect("remove fixture artifact");
        symlink("ca", &artifact_path).expect("link fixture artifact");
        let runner = FakeGateRunner::default();

        let receipt = audit_with_runner(&fixture.root, &runner);

        assert!(receipt.audit_complete);
        assert_eq!(receipt.exit_code(), 1);
        assert!(receipt
            .blockers
            .contains(&"artifact_codex_team".to_string()));
    }

    const TIMEOUT_HELPER_ENV: &str = "CODEX_TEAM_RELEASE_GATE_TIMEOUT_HELPER";

    #[cfg(unix)]
    #[test]
    fn system_gate_timeout_reaps_its_descendant_process_group() {
        let fixture = Fixture::ready("timeout-cleanup");
        let marker = fixture.root.join("descendant.pid");
        let current_exe = std::env::current_exe().expect("current test executable");
        let environment = [(TIMEOUT_HELPER_ENV, marker.as_os_str())];

        let outcome = run_gate_process(
            &fixture.root,
            &current_exe,
            &[
                "--exact",
                "release_readiness::tests::release_gate_timeout_helper",
                "--nocapture",
            ],
            false,
            Duration::from_secs(2),
            &environment,
        );

        assert_eq!(outcome, GateOutcome::TimedOut);
        let descendant = fs::read_to_string(&marker)
            .expect("timeout helper descendant marker")
            .trim()
            .parse::<i32>()
            .expect("descendant pid");
        assert!(!unix_process_exists(descendant));
    }

    #[test]
    fn release_gate_timeout_helper() {
        let Some(marker) = std::env::var_os(TIMEOUT_HELPER_ENV) else {
            return;
        };
        let terminated = Arc::new(AtomicBool::new(false));
        signal_hook::flag::register(signal_hook::consts::SIGTERM, terminated.clone())
            .expect("register timeout helper termination handler");
        let mut descendant = Command::new("/bin/sleep")
            .arg("30")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn timeout helper descendant");
        fs::write(&marker, descendant.id().to_string()).expect("write descendant marker");
        while !terminated.load(Ordering::Acquire) {
            thread::sleep(Duration::from_millis(10));
        }
        let _ = descendant.wait();
    }

    fn fixture_workflow() -> String {
        "name: CI\non: [push]\npermissions:\n  contents: read\njobs:\n  validate:\n    runs-on: ubuntu-24.04\n    steps:\n      - uses: actions/checkout@11bd71901bbe5b1630ceea73d27597364c9af683\n        with:\n          persist-credentials: false\n      - run: cargo fetch --locked\n      - run: cargo build --package codex-team --release --locked --offline\n      - run: target/release/codex-team verify release-readiness --repo . --json\n      - run: git status --porcelain=v1 --untracked-files=all\n"
            .to_string()
    }

    fn write(root: &Path, relative: &str, contents: &str) {
        write_bytes(root, relative, contents.as_bytes());
    }

    fn write_bytes(root: &Path, relative: &str, contents: &[u8]) {
        let path = root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create fixture parent");
        }
        fs::write(path, contents).expect("write fixture file");
    }

    fn input_sha256<'a>(receipt: &'a ReleaseReadinessReceipt, path: &str) -> &'a str {
        receipt
            .inputs
            .iter()
            .find(|input| input.path == path)
            .map(|input| input.sha256.as_str())
            .expect("receipt input")
    }

    fn artifact<'a>(receipt: &'a ReleaseReadinessReceipt, path: &str) -> &'a ReleaseArtifactDigest {
        receipt
            .artifacts
            .iter()
            .find(|artifact| artifact.path == path)
            .expect("release artifact")
    }

    #[cfg(unix)]
    fn unix_process_exists(process_id: i32) -> bool {
        let result = unsafe { libc::kill(process_id, 0) };
        result == 0 || io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
    }

    fn git(root: &Path, args: &[&str]) {
        let status = Command::new("git")
            .args(args)
            .current_dir(root)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("run fixture git command");
        assert!(status.success(), "git {args:?} failed");
    }

    fn git_commit(root: &Path, message: &str) {
        let status = Command::new("git")
            .args([
                "-c",
                "user.name=Release Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "commit",
                "-q",
                "-m",
                message,
            ])
            .current_dir(root)
            .env("GIT_AUTHOR_DATE", "2000-01-02T00:00:00Z")
            .env("GIT_COMMITTER_DATE", "2000-01-02T00:00:00Z")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("commit fixture mutation");
        assert!(status.success());
    }
}
