//! Host-owned engineering gates in a private checkout. Commands come only from
//! the fixed release matrix; provider output never supplies commands or passes.
use crate::harness_activity::HarnessActivity;
use anyhow::{bail, Context, Result};
use codex_team::release_readiness::{fixed_release_gates, FixedGate};
use codex_team::self_improvement::{
    canonical_digest, load_harness_cycle, update_harness_cycle, HarnessStage, LearningVault,
    SourceRepositorySnapshotV1,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::io::AsyncReadExt;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GateReceipt {
    pub(crate) spec_digest: String,
    pub(crate) passed: bool,
    pub(crate) output_digest: String,
    pub(crate) diagnostic: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) artifact_digests: BTreeMap<String, String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EngineeringJournal {
    plan_digest: String,
    candidate_digest: String,
    source: SourceRepositorySnapshotV1,
    workspace: PathBuf,
    started: Option<String>,
    gates: BTreeMap<String, GateReceipt>,
    artifact_digest: Option<String>,
}

pub fn report(vault: &LearningVault, id: &str) -> Result<Option<serde_json::Value>> {
    let (plan, state) = load_harness_cycle(vault, id)?;
    let Some(journal) =
        vault.read_mutable::<EngineeringJournal>(&format!("harness/engineering/{id}.json"))?
    else {
        return Ok(None);
    };
    if journal.plan_digest != plan.digest
        || journal.source != plan.baseline
        || state.candidate_digest.as_deref() != Some(journal.candidate_digest.as_str())
    {
        bail!("engineering report identity changed");
    }
    for (id, receipt) in &journal.gates {
        let spec = fixed_release_gates()
            .iter()
            .find(|gate| gate.id == id)
            .context("unknown engineering gate")?;
        if spec_digest(spec)? != receipt.spec_digest {
            bail!("engineering gate specification changed");
        }
    }
    crate::harness_vm::engineering::validate_receipts(
        plan.vm.is_some(),
        &journal.gates,
        journal.artifact_digest.as_deref(),
    )?;
    Ok(Some(
        serde_json::json!({"gates":journal.gates,"in_progress":journal.started,
        "artifact_digest":journal.artifact_digest,"required_gates":fixed_release_gates().len()}),
    ))
}

pub(crate) fn spec_digest(gate: &FixedGate) -> Result<String> {
    canonical_digest(&(
        gate.id,
        gate.program,
        gate.args,
        gate.cargo_offline,
        gate.timeout_seconds,
    ))
}

// Legacy activities own a child group; native activities retain ca's group.
struct Group(Option<i32>);
impl Drop for Group {
    fn drop(&mut self) {
        if let Some(group) = self.0 {
            unsafe {
                libc::kill(-group, libc::SIGKILL);
            }
        }
    }
}

async fn drain(mut reader: impl tokio::io::AsyncRead + Unpin) -> Result<(String, String)> {
    let mut digest = Sha256::new();
    let mut buffer = [0; 8192];
    let mut total = 0usize;
    let mut excerpt = Vec::new();
    loop {
        let n = reader.read(&mut buffer).await?;
        if n == 0 {
            break;
        }
        total = total.saturating_add(n);
        if total > 32 * 1024 * 1024 {
            bail!("engineering output exceeded bound");
        }
        // Opt-in diagnostics for the ignored real-workspace probe. Production
        // retains only bounded excerpts; the gate's clean environment prevents
        // this test setting from propagating into tested subprocesses.
        #[cfg(test)]
        if std::env::var_os("CA_HARNESS_TRACE_OUTPUT").as_deref() == Some(std::ffi::OsStr::new("1"))
        {
            use std::io::Write;
            std::io::stderr().write_all(&buffer[..n])?;
        }
        digest.update(&buffer[..n]);
        excerpt.extend_from_slice(&buffer[..n]);
        if excerpt.len() > 8192 {
            excerpt.drain(..excerpt.len() - 8192);
        }
    }
    Ok((
        format!("{:x}", digest.finalize()),
        String::from_utf8_lossy(&excerpt).into_owned(),
    ))
}

pub(crate) struct CommandOutput {
    pub(crate) passed: bool,
    pub(crate) out: (String, String),
    pub(crate) err: (String, String),
}

pub(crate) async fn execute_capture(
    command: tokio::process::Command,
    timeout: Duration,
) -> Result<CommandOutput> {
    execute_capture_with_input(command, timeout, std::process::Stdio::null()).await
}

pub(crate) async fn execute_capture_with_input(
    mut command: tokio::process::Command,
    timeout: Duration,
    input: std::process::Stdio,
) -> Result<CommandOutput> {
    command
        .stdin(input)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .process_group(crate::jobs::owned_child_process_group());
    let mut child = command.spawn()?;
    let pid = i32::try_from(child.id().context("engineering child PID missing")?)?;
    let group = Group((!crate::jobs::worker_contained()).then_some(pid));
    let stdout = child.stdout.take().context("missing engineering output")?;
    let stderr = child
        .stderr
        .take()
        .context("missing engineering error output")?;
    let result = tokio::time::timeout(timeout, async {
        let (status, out, err) = tokio::try_join!(
            child.wait(),
            async { drain(stdout).await.map_err(std::io::Error::other) },
            async { drain(stderr).await.map_err(std::io::Error::other) }
        )?;
        Ok::<_, anyhow::Error>(CommandOutput {
            passed: status.success(),
            out,
            err,
        })
    })
    .await;
    drop(group);
    if result.is_err() || result.as_ref().is_ok_and(|r| r.is_err()) {
        let _ = child.kill().await;
        let _ = child.wait().await;
    }
    result.context("engineering command timed out")?
}

async fn execute(
    command: tokio::process::Command,
    timeout: Duration,
) -> Result<(bool, String, Option<String>)> {
    let output = execute_capture(command, timeout).await?;
    let digest = canonical_digest(&(&output.out.0, &output.err.0))?;
    let diagnostic = if output.passed {
        None
    } else {
        Some(failure_diagnostic(&output))
    };
    Ok((output.passed, digest, diagnostic))
}

pub(crate) fn failure_diagnostic(output: &CommandOutput) -> String {
    fn tail(text: &str) -> String {
        let redacted = crate::learning::sanitized_recall_text(text, 8192)
            .unwrap_or_else(|| "details unavailable".into());
        let skip = redacted.chars().count().saturating_sub(2000);
        redacted.chars().skip(skip).collect()
    }
    format!(
        "stderr:\n{}\nstdout:\n{}",
        tail(&output.err.1),
        tail(&output.out.1)
    )
}

pub(crate) async fn source_snapshot(root: &Path) -> Result<SourceRepositorySnapshotV1> {
    let temporary = tempfile::tempdir()?;
    std::fs::create_dir(temporary.path().join("home"))?;
    async fn query(root: &Path, temporary: &Path, args: &[&str]) -> Result<(String, String)> {
        let mut command = fixed_git_command(root, temporary).await?;
        command.args(args);
        let output = execute_capture(command, Duration::from_secs(30)).await?;
        if !output.passed {
            bail!("engineering source inspection failed");
        }
        Ok(output.out)
    }
    let top = query(root, temporary.path(), &["rev-parse", "--show-toplevel"])
        .await?
        .1;
    if Path::new(top.trim()).canonicalize()? != root.canonicalize()? {
        bail!("engineering source must be repository root");
    }
    let branch = query(root, temporary.path(), &["branch", "--show-current"])
        .await?
        .1
        .trim()
        .to_owned();
    let head = query(root, temporary.path(), &["rev-parse", "--verify", "HEAD"])
        .await?
        .1
        .trim()
        .to_owned();
    if branch.is_empty()
        || !matches!(head.len(), 40 | 64)
        || !head.bytes().all(|b| b.is_ascii_hexdigit())
    {
        bail!("engineering source needs a named branch and valid commit");
    }
    let status = query(
        root,
        temporary.path(),
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
    )
    .await?;
    if status.0 != format!("{:x}", Sha256::digest([])) {
        bail!("engineering source is dirty");
    }
    let tracked_tree_digest = query(root, temporary.path(), &["ls-files", "-s", "-z"])
        .await?
        .0;
    let submodule_digest = query(
        root,
        temporary.path(),
        &["submodule", "status", "--recursive"],
    )
    .await?
    .0;
    let repository_fingerprint = canonical_digest(&serde_json::json!({
        "branch": branch, "head": head, "tracked_tree_digest": tracked_tree_digest, "submodule_digest": submodule_digest,
    }))?;
    Ok(SourceRepositorySnapshotV1 {
        schema: "codex-team.source_repository_snapshot.v1".into(),
        branch,
        head,
        tracked_tree_digest,
        submodule_digest,
        repository_fingerprint,
    })
}

/// Git metadata and local object transport run in the trusted coordinator.
/// Enumerating configured filter names is bounded blocking work; no repository
/// helper may execute before a physical command's sandbox boundary.
pub(crate) async fn fixed_git_command(
    root: &Path,
    temporary: &Path,
) -> Result<tokio::process::Command> {
    let root = root.to_path_buf();
    let command =
        tokio::task::spawn_blocking(move || ca_automation::git_metadata::command_no_filters(&root))
            .await??;
    let mut command = tokio::process::Command::from(command);
    command
        .env("HOME", temporary.join("home"))
        .env("TMPDIR", temporary)
        .env("GIT_ATTR_NOSYSTEM", "1")
        .env("GIT_NO_REPLACE_OBJECTS", "1");
    Ok(command)
}

fn allowance(vault: &LearningVault, id: &str, maximum: u64) -> Result<u64> {
    let (plan, _) = load_harness_cycle(vault, id)?;
    update_harness_cycle(vault, id, |state| {
        if !matches!(
            state.stage,
            HarnessStage::CandidateReady | HarnessStage::Evaluating
        ) {
            bail!("cycle cannot start engineering work in its current state");
        }
        let seconds = plan
            .settings
            .maximum_active_seconds
            .saturating_sub(state.active_seconds)
            .min(maximum);
        if seconds == 0 {
            state.stage = HarnessStage::BudgetExhausted;
            state.reason = "Engineering activity budget exhausted".into();
            bail!("engineering activity budget exhausted");
        }
        Ok(seconds)
    })
}

async fn audit_checkout(
    vault: &LearningVault,
    id: &str,
    workspace: &Path,
    expected: &SourceRepositorySnapshotV1,
    artifact: bool,
) -> Result<Option<String>> {
    let (plan, _) = load_harness_cycle(vault, id)?;
    let seconds = allowance(vault, id, 60)?;
    let activity = HarnessActivity::begin(vault.clone(), plan.clone(), seconds).await?;
    activity
        .run(async {
            crate::harness_snapshot::verify_contents(&plan, workspace).await?;
            if source_snapshot(workspace).await? != *expected {
                bail!("engineering source identity changed");
            }
            if artifact {
                let path = workspace.join("target/release/ca");
                Ok(Some(
                    tokio::task::spawn_blocking(move || artifact_digest(&path)).await??,
                ))
            } else {
                Ok(None)
            }
        })
        .await
}

pub(crate) fn clean_environment(command: &mut tokio::process::Command, temporary: &Path) {
    command
        .env_clear()
        .env("HOME", temporary.join("home"))
        .env("TMPDIR", temporary)
        .env(
            "PATH",
            "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin",
        )
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_ATTR_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .env("CARGO_NET_OFFLINE", "true")
        .env("CARGO_INCREMENTAL", "0");
}

pub(crate) async fn run_gate(
    workspace: &Path,
    gate: &FixedGate,
    seconds: u64,
) -> Result<GateReceipt> {
    let temporary = tempfile::tempdir()?;
    std::fs::create_dir(temporary.path().join("home"))?;
    let cargo_home = temporary.path().join("cargo");
    std::fs::create_dir(&cargo_home)?;
    let mut read_only_roots = vec![workspace.to_path_buf()];
    for path in ["/opt/homebrew", "/usr/local"] {
        if Path::new(path).is_dir() {
            read_only_roots.push(path.into());
        }
    }
    // Only offline dependency caches are shared. Cargo configuration and stored
    // registry credentials are never part of the granted Cargo home.
    if let Some(home) = std::env::var_os("HOME") {
        for name in ["registry", "git"] {
            let cache = PathBuf::from(&home).join(".cargo").join(name);
            if cache.is_dir() {
                let cache = cache.canonicalize()?;
                std::os::unix::fs::symlink(&cache, cargo_home.join(name))?;
                read_only_roots.push(cache);
            }
        }
    }
    let target = workspace.join("target");
    std::fs::create_dir_all(&target)?;
    let program = match gate.program {
        "cargo" => [
            "/opt/homebrew/bin/cargo",
            "/usr/local/bin/cargo",
            "/usr/bin/cargo",
        ]
        .into_iter()
        .map(PathBuf::from)
        .find(|path| path.is_file())
        .context("Cargo unavailable")?,
        "git" => PathBuf::from("/usr/bin/git"),
        relative if relative.starts_with("target/release/") => workspace.join(relative),
        _ => bail!("unknown fixed engineering program"),
    };
    let policy = crate::sandbox::SandboxPolicy {
        roots: vec![target],
        read_only_roots,
        temporary_directory: temporary.path().into(),
        network_allowed: false,
        hide_other_processes: true,
    };
    let prepared = crate::sandbox::SandboxBackend::new(crate::sandbox::SandboxMode::Required)
        .prepare_process_with_denied_paths(
            &program,
            &gate.args.iter().map(|s| (*s).into()).collect::<Vec<_>>(),
            &policy,
            &[],
        )?;
    let mut command = tokio::process::Command::new(prepared.program);
    command.args(prepared.arguments).current_dir(workspace);
    clean_environment(&mut command, temporary.path());
    command
        .env("CARGO_HOME", cargo_home)
        .env("CARGO_TARGET_DIR", workspace.join("target"));
    let (passed, output_digest, diagnostic) =
        execute(command, Duration::from_secs(seconds)).await?;
    Ok(GateReceipt {
        spec_digest: spec_digest(gate)?,
        passed,
        output_digest,
        diagnostic,
        artifact_digests: BTreeMap::new(),
    })
}

pub(crate) async fn run_gate_selected(
    workspace: &Path,
    gate: &FixedGate,
    seconds: u64,
    vm: Option<&codex_team::self_improvement::FrozenHarnessVm>,
) -> Result<GateReceipt> {
    match vm {
        Some(vm) => crate::harness_vm::engineering::run_gate(vm, workspace, gate, seconds).await,
        None => run_gate(workspace, gate, seconds).await,
    }
}

/// Check an unchanged guidance binary's source using a private checkout. Source
/// candidate support will bind this same gate runner to its inspected commit.
pub async fn check_guidance(vault: LearningVault, id: &str, repository: &Path) -> Result<bool> {
    crate::harness_idle::boundary(&vault, id).await?;
    let _worker = vault.acquire_worker_lock()?;
    let (plan, state) = load_harness_cycle(&vault, id)?;
    let repository_path = crate::harness_snapshot::resolve(&vault, &plan, repository)?;
    let repository = repository_path.as_path();
    let (_, harness) = crate::harness_generation::load_candidate(&vault, id)?;
    if !matches!(
        state.stage,
        HarnessStage::CandidateReady | HarnessStage::Evaluating
    ) {
        bail!("cycle cannot start engineering checks in its current state");
    }
    update_harness_cycle(&vault, id, |state| {
        state.engineering_checks_passed = false;
        if !state.active_reservations.is_empty() || !state.reservations.is_empty() {
            state.stage = HarnessStage::RecoveryRequired;
            state.reason = "Unsettled work prevents engineering checks".into();
            bail!("unsettled work prevents engineering checks");
        }
        Ok(())
    })?;
    let path = format!("harness/engineering/{id}.json");
    let existing = vault.read_mutable::<EngineeringJournal>(&path)?;
    let mut journal = if let Some(journal) = existing {
        if journal.plan_digest != plan.digest
            || journal.candidate_digest != harness.digest()?
            || journal.source != plan.baseline
        {
            bail!("engineering journal binding changed");
        }
        if journal.started.is_some() {
            update_harness_cycle(&vault, id, |state| {
                state.stage = HarnessStage::RecoveryRequired;
                state.reason = "Interrupted engineering gate; reservations retained".into();
                Ok(())
            })?;
            bail!("engineering gate interrupted; no automatic replay");
        }
        crate::harness_vm::engineering::validate_receipts(
            plan.vm.is_some(),
            &journal.gates,
            journal.artifact_digest.as_deref(),
        )?;
        journal
    } else {
        let root = vault.resolve_relative("harness/engineering")?;
        std::fs::create_dir_all(&root)?;
        let directory = tempfile::Builder::new()
            .prefix("checkout-")
            .tempdir_in(&root)?
            .keep();
        let workspace = directory.join("source");
        let journal = EngineeringJournal {
            plan_digest: plan.digest.clone(),
            candidate_digest: harness.digest()?,
            source: plan.baseline.clone(),
            workspace: workspace.clone(),
            started: Some("checkout".into()),
            gates: BTreeMap::new(),
            artifact_digest: None,
        };
        {
            let _lock = vault.acquire_lock()?;
            vault.write_mutable(&path, &journal)?;
        }
        let seconds = allowance(&vault, id, 120)?;
        let activity = HarnessActivity::begin(vault.clone(), plan.clone(), seconds).await?;
        activity
            .run(async {
                if source_snapshot(repository).await? != plan.baseline {
                    bail!("engineering baseline source changed");
                }
                let mut command = fixed_git_command(repository, &directory).await?;
                command
                    .args(["clone", "--local", "--no-hardlinks", "--quiet", "--"])
                    .arg(repository.canonicalize()?)
                    .arg(&workspace);
                let (passed, _, _) = execute(command, Duration::from_secs(seconds)).await?;
                if !passed {
                    bail!("private engineering checkout failed");
                }
                if source_snapshot(&workspace).await? != plan.baseline {
                    bail!("private engineering checkout changed source");
                }
                Ok(())
            })
            .await?;
        let mut journal = journal;
        journal.started = None;
        {
            let _lock = vault.acquire_lock()?;
            vault.write_mutable(&path, &journal)?;
        }
        journal
    };
    let root = vault
        .resolve_relative("harness/engineering")?
        .canonicalize()?;
    let workspace = journal.workspace.canonicalize()?;
    if !workspace.starts_with(&root) {
        bail!("engineering checkout identity changed");
    }
    audit_checkout(&vault, id, &workspace, &journal.source, false).await?;
    for gate in fixed_release_gates() {
        crate::harness_idle::boundary(&vault, id).await?;
        if let Some(receipt) = journal.gates.get(gate.id) {
            if receipt.spec_digest != spec_digest(gate)? {
                bail!("engineering gate specification changed");
            }
            if !receipt.passed {
                return Ok(false);
            }
            continue;
        }
        let (_, state) = load_harness_cycle(&vault, id)?;
        if !matches!(
            state.stage,
            HarnessStage::CandidateReady | HarnessStage::Evaluating
        ) {
            return Ok(false);
        }
        let seconds = allowance(&vault, id, gate.timeout_seconds)?;
        journal.started = Some(gate.id.into());
        {
            let _lock = vault.acquire_lock()?;
            vault.write_mutable(&path, &journal)?;
        }
        let activity = HarnessActivity::begin(vault.clone(), plan.clone(), seconds).await?;
        let receipt = activity
            .run(run_gate_selected(
                &workspace,
                gate,
                seconds,
                plan.vm.as_ref(),
            ))
            .await?;
        let passed = receipt.passed;
        journal.gates.insert(gate.id.into(), receipt);
        journal.started = None;
        {
            let _lock = vault.acquire_lock()?;
            vault.write_mutable(&path, &journal)?;
        }
        if !passed {
            return Ok(false);
        }
    }
    // Keep the compiled artifact digest with the gate receipts. Guidance still
    // executes the original, separately pinned baseline binary.
    let artifact = audit_checkout(&vault, id, &workspace, &journal.source, true)
        .await?
        .context("missing engineering artifact")?;
    if journal
        .artifact_digest
        .as_ref()
        .is_some_and(|saved| saved != &artifact)
    {
        bail!("verified engineering artifact changed");
    }
    journal.artifact_digest = Some(artifact);
    crate::harness_vm::engineering::validate_receipts(
        plan.vm.is_some(),
        &journal.gates,
        journal.artifact_digest.as_deref(),
    )?;
    {
        let _lock = vault.acquire_lock()?;
        vault.write_mutable(&path, &journal)?;
    }
    update_harness_cycle(&vault, id, |state| {
        if state.candidate_digest.as_deref() != Some(journal.candidate_digest.as_str())
            || !state.reservations.is_empty()
            || !state.active_reservations.is_empty()
        {
            bail!("engineering admission binding changed");
        }
        if matches!(
            state.stage,
            HarnessStage::CandidateReady | HarnessStage::Evaluating | HarnessStage::Paused
        ) {
            state.engineering_checks_passed = true;
        }
        Ok(())
    })?;
    Ok(true)
}

/// Reinspect completed engineering evidence at activation, without rerunning a gate.
pub(crate) async fn verify_guidance_ready(
    vault: &LearningVault,
    id: &str,
    activity: &HarnessActivity,
) -> Result<()> {
    let (plan, cycle) = load_harness_cycle(vault, id)?;
    crate::harness_trials::verify_hint_exposure(vault, id)?;
    report(vault, id)?.context("guidance engineering report missing")?;
    let journal: EngineeringJournal = vault
        .read_mutable(&format!("harness/engineering/{id}.json"))?
        .context("guidance engineering journal missing")?;
    if !cycle.engineering_checks_passed
        || journal.started.is_some()
        || journal.gates.len() != fixed_release_gates().len()
        || fixed_release_gates().iter().any(|gate| {
            journal
                .gates
                .get(gate.id)
                .is_none_or(|receipt| !receipt.passed)
        })
        || journal.artifact_digest.is_none()
    {
        bail!("guidance engineering checks are incomplete");
    }
    let workspace = journal.workspace.canonicalize()?;
    if !workspace.starts_with(
        vault
            .resolve_relative("harness/engineering")?
            .canonicalize()?,
    ) {
        bail!("guidance engineering checkout changed");
    }
    if !activity.matches(vault, &plan) {
        bail!("guidance inspection belongs to another activity");
    }
    activity.remaining()?;
    if source_snapshot(&workspace).await? != plan.baseline {
        bail!("guidance engineering source changed");
    }
    crate::harness_snapshot::verify_contents(&plan, &workspace).await?;
    let binary = workspace.join("target/release/ca");
    let artifact = tokio::task::spawn_blocking(move || artifact_digest(&binary)).await??;
    activity.remaining()?;
    if Some(artifact) != journal.artifact_digest {
        bail!("guidance engineering artifact changed");
    }
    Ok(())
}

pub(crate) fn artifact_digest(path: &Path) -> Result<String> {
    use std::io::Read;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > 2 * 1024 * 1024 * 1024 {
        bail!("invalid engineering artifact");
    }
    let mut hash = Sha256::new();
    let mut buffer = [0; 65536];
    let mut total = 0;
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > metadata.len() {
            bail!("engineering artifact changed while hashing");
        }
        hash.update(&buffer[..n]);
    }
    if total != metadata.len() {
        bail!("engineering artifact changed while hashing");
    }
    Ok(format!("{:x}", hash.finalize()))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[tokio::test]
    async fn hrs253_engineering_diagnostics_retain_failure_tail_and_full_digest() {
        let mut bytes = vec![b'x'; 24 * 1024];
        bytes.extend_from_slice(b"\nfinal compiler failure\n");
        let expected = format!("{:x}", Sha256::digest(&bytes));
        let (digest, excerpt) = drain(bytes.as_slice()).await.unwrap();
        assert_eq!(digest, expected);
        assert_eq!(excerpt.len(), 8192);
        assert!(excerpt.ends_with("\nfinal compiler failure\n"));
        let diagnostic = failure_diagnostic(&CommandOutput {
            passed: false,
            out: (
                String::new(),
                format!("{}\nFAILED: behavior regression", "x".repeat(8192)),
            ),
            err: (
                String::new(),
                format!("{}\nerror: test failed", "warning\n".repeat(1024)),
            ),
        });
        assert!(diagnostic.contains("FAILED: behavior regression"));
        assert!(diagnostic.contains("error: test failed"));
        assert!(diagnostic.chars().count() <= 4096);
    }

    #[tokio::test]
    #[ignore = "runs a fixed real-ca engineering gate in a retained private snapshot; may take 30 minutes"]
    async fn hrs253_real_ca_engineering_profile() {
        let gate_id =
            std::env::var("CA_HARNESS_ENGINEERING_GATE").unwrap_or_else(|_| "test_default".into());
        let gate = fixed_release_gates()
            .iter()
            .find(|gate| gate.id == gate_id)
            .expect("select an existing fixed gate");
        let root = tempfile::Builder::new()
            .prefix("ca-hrs253-real-gate-")
            .tempdir()
            .unwrap()
            .keep();
        let vault = LearningVault::open_at(root.join("vault")).unwrap();
        let repository = Path::new(env!("CARGO_MANIFEST_DIR"));
        let (snapshot, owner) =
            crate::harness_snapshot::capture(&vault, repository, env!("CA_SOURCE_DIGEST")).unwrap();
        let _retained_inputs = owner.keep();
        eprintln!(
            "retained engineering snapshot: {}",
            snapshot.repository.display()
        );
        let receipt = run_gate(&snapshot.repository, gate, gate.timeout_seconds)
            .await
            .unwrap();
        std::fs::write(
            root.join("receipt.json"),
            serde_json::to_vec_pretty(&receipt).unwrap(),
        )
        .unwrap();
        assert!(receipt.passed, "{}: {:?}", gate.id, receipt.diagnostic);
    }
    #[test]
    #[ignore = "exports a bounded current-source snapshot for the dedicated VM integration test"]
    fn hrs253_export_current_source_for_vm() {
        let root = tempfile::Builder::new()
            .prefix("ca-hrs253-vm-source-")
            .tempdir()
            .unwrap()
            .keep();
        let vault = LearningVault::open_at(root.join("vault")).unwrap();
        let (snapshot, owner) = crate::harness_snapshot::capture(
            &vault,
            Path::new(env!("CARGO_MANIFEST_DIR")),
            env!("CA_SOURCE_DIGEST"),
        )
        .unwrap();
        let _retained_inputs = owner.keep();
        let receipt = serde_json::json!({
            "repository": snapshot.repository,
            "source_digest": env!("CA_SOURCE_DIGEST"),
            "build_digest": env!("CA_BUILD_DIGEST"),
            "purpose": "VM test input export; not an engineering gate receipt",
        });
        std::fs::write(
            root.join("input.json"),
            serde_json::to_vec_pretty(&receipt).unwrap(),
        )
        .unwrap();
        eprintln!("VM_INPUT_RECEIPT={}", root.join("input.json").display());
    }

    use codex_team::self_improvement::observe_clean_repository;

    fn git(root: &Path, args: &[&str]) {
        let result = std::process::Command::new("git")
            .args(args)
            .current_dir(root)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "git failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }

    #[tokio::test]
    async fn aut259_harness_source_metadata_never_executes_local_git_helpers() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().canonicalize().unwrap();
        let repository = root.join("repository");
        std::fs::create_dir(&repository).unwrap();
        std::fs::write(repository.join("source.txt"), "baseline\n").unwrap();
        std::fs::write(
            repository.join(".gitattributes"),
            "source.txt filter=fixture\n",
        )
        .unwrap();
        git(&repository, &["init", "-q"]);
        git(&repository, &["add", "."]);
        git(
            &repository,
            &[
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@invalid",
                "commit",
                "-qm",
                "baseline",
            ],
        );
        let baseline = source_snapshot(&repository).await.unwrap();
        let filter_marker = root.join("filter-ran");
        let monitor_marker = root.join("monitor-ran");
        let filter = format!("touch '{}'; cat", filter_marker.display());
        let monitor = format!("touch '{}'", monitor_marker.display());
        git(&repository, &["config", "filter.fixture.clean", &filter]);
        git(&repository, &["config", "filter.fixture.smudge", &filter]);
        git(&repository, &["config", "core.fsmonitor", &monitor]);
        assert_eq!(source_snapshot(&repository).await.unwrap(), baseline);
        assert!(!filter_marker.exists());
        assert!(!monitor_marker.exists());
    }
    pub(crate) fn source(root: &Path) {
        std::fs::create_dir_all(root.join("src/bin")).unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"engineering-fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        std::fs::write(root.join(".gitignore"), "/target/\n").unwrap();
        for name in ["ca", "codex-team"] {
            // This tiny engineering fixture emulates the protected identity
            // command. It is not a real ca build or a coding-quality receipt.
            let program = format!(
                "fn main() {{\n    if std::env::args().nth(1).as_deref() == Some(\"--internal-harness-build-identity\") {{\n        println!(\"{}\");\n        return;\n    }}\n    println!(\"fixture 0.1.0\");\n}}\n",
                env!("CA_BUILD_DIGEST")
            );
            std::fs::write(root.join(format!("src/bin/{name}.rs")), program).unwrap();
        }
        let status = std::process::Command::new("/opt/homebrew/bin/cargo")
            .args(["generate-lockfile", "--offline"])
            .current_dir(root)
            .status()
            .unwrap();
        assert!(status.success());
        git(root, &["init", "-b", "main"]);
        git(root, &["config", "user.name", "Fixture"]);
        git(root, &["config", "user.email", "fixture@example.invalid"]);
        git(root, &["config", "commit.gpgsign", "false"]);
        git(root, &["add", "."]);
        git(root, &["commit", "-m", "fixture"]);
    }

    #[tokio::test]
    #[ignore = "requires macOS sandbox and local Cargo; executes complete fixed matrix in a disposable project"]
    async fn hrs253_real_engineering_gates_are_budgeted_and_recover_without_rerun() {
        let temp = tempfile::tempdir().unwrap();
        let repository = temp.path().join("repository");
        source(&repository);
        let vault = LearningVault::open_at(temp.path().join("vault")).unwrap();
        let snapshot = observe_clean_repository(&repository).unwrap();
        let plan = crate::harness_generation::tests::saved_fixture(&vault, snapshot.clone());
        let passed = check_guidance(vault.clone(), &plan.id, &repository)
            .await
            .unwrap();
        if !passed {
            let journal: EngineeringJournal = vault
                .read_mutable(&format!("harness/engineering/{}.json", plan.id))
                .unwrap()
                .unwrap();
            panic!(
                "engineering gates failed: {}",
                serde_json::to_string(&journal.gates).unwrap()
            );
        }
        let (_, first) = load_harness_cycle(&vault, &plan.id).unwrap();
        assert!(first.engineering_checks_passed);
        assert!(first.active_seconds > 0);
        assert!(first.active_reservations.is_empty());
        assert_eq!(first.settled_microusd, 0);
        let journal: EngineeringJournal = vault
            .read_mutable(&format!("harness/engineering/{}.json", plan.id))
            .unwrap()
            .unwrap();
        assert_eq!(journal.gates.len(), fixed_release_gates().len());
        assert!(journal.started.is_none());
        assert!(check_guidance(vault.clone(), &plan.id, &repository)
            .await
            .unwrap());
        let (_, second) = load_harness_cycle(&vault, &plan.id).unwrap();
        assert_eq!(second.completed_work.len(), first.completed_work.len() + 2);
        assert!(second.active_seconds >= first.active_seconds);
        let recovered: EngineeringJournal = vault
            .read_mutable(&format!("harness/engineering/{}.json", plan.id))
            .unwrap()
            .unwrap();
        assert_eq!(
            canonical_digest(&journal.gates).unwrap(),
            canonical_digest(&recovered.gates).unwrap()
        );
        assert_eq!(observe_clean_repository(&repository).unwrap(), snapshot);
        std::fs::write(journal.workspace.join("target/release/ca"), "changed").unwrap();
        assert!(check_guidance(vault.clone(), &plan.id, &repository)
            .await
            .is_err());
        assert!(
            !load_harness_cycle(&vault, &plan.id)
                .unwrap()
                .1
                .engineering_checks_passed
        );
    }

    #[test]
    fn hrs253_engineering_artifact_rejects_symlink() {
        let temp = tempfile::tempdir().unwrap();
        let original = temp.path().join("original");
        std::fs::write(&original, "artifact").unwrap();
        let link = temp.path().join("link");
        std::os::unix::fs::symlink(&original, &link).unwrap();
        assert!(artifact_digest(&original).is_ok());
        assert!(artifact_digest(&link).is_err());
    }
    #[tokio::test]
    #[ignore = "requires macOS sandbox and local Cargo"]
    async fn hrs253_failed_engineering_gate_is_saved_and_not_replayed() {
        let temp = tempfile::tempdir().unwrap();
        let repository = temp.path().join("repository");
        source(&repository);
        std::fs::write(
            repository.join("src/bin/ca.rs"),
            "fn main(){println!(\"bad format\");}\n",
        )
        .unwrap();
        git(&repository, &["add", "."]);
        git(&repository, &["commit", "-m", "format failure"]);
        let vault = LearningVault::open_at(temp.path().join("vault")).unwrap();
        let plan = crate::harness_generation::tests::saved_fixture(
            &vault,
            observe_clean_repository(&repository).unwrap(),
        );
        assert!(!check_guidance(vault.clone(), &plan.id, &repository)
            .await
            .unwrap());
        let (_, first) = load_harness_cycle(&vault, &plan.id).unwrap();
        assert!(!first.engineering_checks_passed);
        let journal: EngineeringJournal = vault
            .read_mutable(&format!("harness/engineering/{}.json", plan.id))
            .unwrap()
            .unwrap();
        assert_eq!(journal.gates.len(), 1);
        assert!(!journal.gates["format"].passed);
        assert!(journal.gates["format"].diagnostic.is_some());
        assert!(!check_guidance(vault.clone(), &plan.id, &repository)
            .await
            .unwrap());
        assert_eq!(
            load_harness_cycle(&vault, &plan.id)
                .unwrap()
                .1
                .completed_work
                .len(),
            first.completed_work.len() + 1
        );
    }

    #[tokio::test]
    async fn hrs253_engineering_timeout_and_cancellation_terminate_owned_child() {
        for cancelled in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let marker = temp.path().join("pid");
            let mut command = tokio::process::Command::new("/bin/sh");
            command
                .args(["-c", "echo $$ > \"$1\"; exec /bin/sleep 60", "fixture"])
                .arg(&marker);
            let task = tokio::spawn(execute(
                command,
                Duration::from_millis(if cancelled { 60000 } else { 300 }),
            ));
            let mut pid = None;
            let mut readiness = String::from("PID marker not yet published");
            for _ in 0..100 {
                match std::fs::read_to_string(&marker) {
                    Ok(contents) => {
                        // Shell redirection creates the marker before echo writes its PID.
                        if contents.ends_with('\n') {
                            pid = contents.trim().parse::<i32>().ok().filter(|pid| *pid > 1);
                            if pid.is_some() {
                                break;
                            }
                        }
                        readiness = format!("incomplete or invalid PID marker: {contents:?}");
                    }
                    Err(error) => readiness = format!("could not read PID marker: {error}"),
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            let Some(pid) = pid else {
                task.abort();
                let result = task.await;
                panic!(
                    "engineering child PID readiness failed (cancelled={cancelled}): \
                     {readiness}; owned task retired: {result:?}"
                );
            };
            assert!(pid > 1);
            if cancelled {
                task.abort();
                assert!(task.await.unwrap_err().is_cancelled());
            } else {
                assert!(task.await.unwrap().is_err());
            }
            for _ in 0..100 {
                if unsafe { libc::kill(pid, 0) } != 0 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            assert_ne!(unsafe { libc::kill(pid, 0) }, 0);
        }
    }
    #[test]
    fn hrs253_engineering_exhaustion_is_durable_and_does_not_erase_pause() {
        use codex_team::self_improvement::start_harness_cycle;
        let provider = crate::harness_budget::tests::frozen();
        let (plan, _, _) = crate::harness_trials::tests::fixture_plan(&provider, &"a".repeat(64));
        let temp = tempfile::tempdir().unwrap();
        let vault = LearningVault::open_at(temp.path().join("vault")).unwrap();
        start_harness_cycle(&vault, &plan, 0).unwrap();
        update_harness_cycle(&vault, &plan.id, |state| {
            state.stage = HarnessStage::CandidateReady;
            state.active_seconds = plan.settings.maximum_active_seconds;
            Ok(())
        })
        .unwrap();
        assert!(allowance(&vault, &plan.id, 60).is_err());
        assert_eq!(
            load_harness_cycle(&vault, &plan.id).unwrap().1.stage,
            HarnessStage::BudgetExhausted
        );
        update_harness_cycle(&vault, &plan.id, |state| {
            state.stage = HarnessStage::Paused;
            Ok(())
        })
        .unwrap();
        assert!(allowance(&vault, &plan.id, 60).is_err());
        assert_eq!(
            load_harness_cycle(&vault, &plan.id).unwrap().1.stage,
            HarnessStage::Paused
        );
    }
}
