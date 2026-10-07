//! Host-owned, recoverable activation artifacts. Signing never changes the
//! evaluated artifact, and preparation never replaces the active executable.
use crate::harness_activity::HarnessActivity;
use crate::harness_engineering::{artifact_digest, clean_environment, execute_capture};
use anyhow::{bail, Context, Result};
use codex_team::self_improvement::{
    evaluate_harness, load_harness_cycle, update_harness_cycle, HarnessPlan, HarnessStage,
    HarnessVerdict, LearningVault,
};
use serde::{Deserialize, Serialize};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const IDENTITY_FLAG: &str = "--internal-harness-build-identity";

/// Read the compiled identity from the exact signed artifact inside the same
/// offline sandbox used for smoke checks. This command never loads user state.
async fn identity(path: &Path, temporary: &Path) -> Result<String> {
    let policy = crate::sandbox::SandboxPolicy {
        roots: vec![temporary.into()],
        read_only_roots: vec![path.into()],
        temporary_directory: temporary.into(),
        network_allowed: false,
        hide_other_processes: true,
    };
    let prepared = crate::sandbox::SandboxBackend::new(crate::sandbox::SandboxMode::Required)
        .prepare_process(path, &[IDENTITY_FLAG.into()], &policy)?;
    let mut command = tokio::process::Command::new(prepared.program);
    command.args(prepared.arguments).current_dir(temporary);
    clean_environment(&mut command, temporary);
    let output = execute_capture(command, Duration::from_secs(15)).await?;
    let build = output.out.1.trim();
    if !output.passed || build.len() != 64 || !build.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!("candidate did not report a valid compiled harness identity");
    }
    Ok(build.into())
}

async fn identity_selected(
    path: &Path,
    temporary: &Path,
    vm: Option<&codex_team::self_improvement::FrozenHarnessVm>,
    allowance: Duration,
) -> Result<String> {
    if let Some(vm) = vm {
        let outputs = crate::harness_vm::probes::run(vm, path, &[IDENTITY_FLAG], allowance).await?;
        let value = outputs[0].out.1.trim();
        if value.len() != 64 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
            bail!("candidate did not report a valid compiled harness identity");
        }
        Ok(value.into())
    } else {
        identity(path, temporary).await
    }
}

pub(crate) async fn smoke_selected(
    path: &Path,
    temporary: &Path,
    vm: Option<&codex_team::self_improvement::FrozenHarnessVm>,
    allowance: Duration,
) -> Result<Vec<String>> {
    if let Some(vm) = vm {
        Ok(
            crate::harness_vm::probes::run(vm, path, &["--version", "--help"], allowance)
                .await?
                .into_iter()
                .map(|output| output.out.0)
                .collect(),
        )
    } else {
        smoke(path, temporary).await
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreparedArtifacts {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guidance: Option<crate::harness_guidance::SourceGuidance>,
    pub plan_digest: String,
    pub candidate_digest: String,
    pub trials_digest: String,
    pub destination: PathBuf,
    pub directory: PathBuf,
    pub baseline_digest: String,
    pub evaluated_digest: String,
    pub destination_mode: u32,
    pub signed_digest: Option<String>,
    pub baseline_smoke: Vec<String>,
    pub candidate_smoke: Vec<String>,
    pub ready: bool,
}

fn journal_path(id: &str) -> String {
    format!("harness/source/{id}-activation.json")
}

pub(crate) fn executable_metadata(path: &Path) -> Result<std::fs::Metadata> {
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.nlink() != 1
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o022 != 0
        || metadata.mode() & 0o111 == 0
        || metadata.len() == 0
        || metadata.len() > 2 * 1024 * 1024 * 1024
    {
        bail!("activation requires an owned regular single-link executable");
    }
    Ok(metadata)
}

pub(crate) fn copy_binary(source: &Path, destination: &Path, expected: &str) -> Result<()> {
    use sha2::{Digest, Sha256};
    use std::io::{Read, Write};
    let mut input = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(source)?;
    let before = executable_metadata(source)?;
    let opened = input.metadata()?;
    if before.dev() != opened.dev() || before.ino() != opened.ino() {
        bail!("activation input changed before copying");
    }
    let mut output = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o700)
        .custom_flags(libc::O_NOFOLLOW)
        .open(destination)?;
    let mut hash = Sha256::new();
    let mut total = 0u64;
    let mut buffer = [0u8; 65536];
    loop {
        let count = input.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        total += count as u64;
        if total > opened.len() {
            bail!("activation input grew while copying");
        }
        hash.update(&buffer[..count]);
        output.write_all(&buffer[..count])?;
    }
    if total != opened.len() || format!("{:x}", hash.finalize()) != expected {
        bail!("activation input differs from frozen bytes");
    }
    output.sync_all()?;
    Ok(())
}

pub(crate) async fn signature(path: &Path, sign: bool, temporary: &Path) -> Result<()> {
    if !cfg!(target_os = "macos") {
        bail!("automatic signed activation is currently supported on macOS");
    }
    let mut command = tokio::process::Command::new("/usr/bin/codesign");
    if sign {
        command.args(["--force", "--sign", "-"]);
    } else {
        command.args(["--verify", "--strict", "--verbose=2"]);
    }
    command.arg(path);
    clean_environment(&mut command, temporary);
    if !execute_capture(command, Duration::from_secs(30))
        .await?
        .passed
    {
        bail!("activation artifact code signing verification failed");
    }
    Ok(())
}

pub(crate) async fn smoke(path: &Path, temporary: &Path) -> Result<Vec<String>> {
    let home = temporary.join("home");
    std::fs::create_dir_all(&home)?;
    let policy = crate::sandbox::SandboxPolicy {
        roots: vec![temporary.into()],
        read_only_roots: vec![path.into()],
        temporary_directory: temporary.into(),
        network_allowed: false,
        hide_other_processes: true,
    };
    let mut digests = Vec::new();
    for flag in ["--version", "--help"] {
        let prepared = crate::sandbox::SandboxBackend::new(crate::sandbox::SandboxMode::Required)
            .prepare_process(path, &[flag.into()], &policy)?;
        let mut command = tokio::process::Command::new(prepared.program);
        command.args(prepared.arguments).current_dir(temporary);
        clean_environment(&mut command, temporary);
        let output = execute_capture(command, Duration::from_secs(15)).await?;
        if !output.passed || output.out.1.is_empty() {
            bail!("activation executable failed its offline version/help smoke");
        }
        digests.push(output.out.0);
    }
    Ok(digests)
}

pub(crate) fn validate_binding(
    journal: &PreparedArtifacts,
    plan: &HarnessPlan,
    candidate: &str,
    trials: &str,
) -> Result<()> {
    let destination = plan
        .activation_executable
        .as_ref()
        .context("activation destination was not frozen")?;
    if journal.plan_digest != plan.digest
        || &journal.destination != destination
        || journal.baseline_digest != plan.baseline_binary_digest
        || journal.candidate_digest != candidate
        || journal.trials_digest != trials
        || journal.directory.parent() != destination.parent()
        || !journal
            .directory
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with(".ca-harness-"))
        || journal.directory.canonicalize()? != journal.directory
    {
        bail!("activation journal binding changed");
    }
    let directory = std::fs::symlink_metadata(&journal.directory)?;
    if !directory.is_dir()
        || directory.uid() != unsafe { libc::geteuid() }
        || directory.mode() & 0o077 != 0
    {
        bail!("activation recovery directory is not private");
    }
    Ok(())
}

pub fn report(vault: &LearningVault, id: &str) -> Result<Option<PreparedArtifacts>> {
    let (plan, cycle) = load_harness_cycle(vault, id)?;
    let journal = vault.read_mutable::<PreparedArtifacts>(&journal_path(id))?;
    if let Some(journal) = &journal {
        let decision = evaluate_harness(
            &plan,
            cycle
                .candidate_digest
                .as_deref()
                .context("candidate missing")?,
            &cycle.trials,
            cycle.engineering_checks_passed,
        )?;
        validate_binding(
            journal,
            &plan,
            cycle.candidate_digest.as_deref().unwrap(),
            &decision.trials_digest,
        )?;
    }
    Ok(journal)
}

pub async fn prepare(vault: LearningVault, id: &str) -> Result<PreparedArtifacts> {
    Box::pin(prepare_inner(vault, id)).await
}

async fn prepare_inner(vault: LearningVault, id: &str) -> Result<PreparedArtifacts> {
    let _worker = vault.acquire_worker_lock()?;
    let (plan, cycle) = load_harness_cycle(&vault, id)?;
    if cycle.stage != HarnessStage::ReadyToPromote
        || !cycle.reservations.is_empty()
        || !cycle.active_reservations.is_empty()
        || plan.source_files.is_empty()
    {
        bail!("activation preparation requires a settled source promotion decision");
    }
    let destination = plan
        .activation_executable
        .clone()
        .context("activation destination was not frozen")?;
    if destination.canonicalize()? != destination {
        bail!("activation destination changed");
    }
    let (inputs, harness, artifact) = crate::harness_source_build::load_candidate(&vault, id)?;
    let candidate = harness.digest()?;
    let decision = evaluate_harness(
        &plan,
        &candidate,
        &cycle.trials,
        cycle.engineering_checks_passed,
    )?;
    if decision.verdict != HarnessVerdict::Promote {
        bail!("trials do not authorize activation");
    }
    let imported =
        crate::harness_source_checkout::report(&vault, id)?.context("source checkout missing")?;
    if imported["import"]["candidate_digest"] != candidate
        || imported["import"]["trials_digest"] != decision.trials_digest
    {
        bail!("verified source import missing or changed");
    }
    let existing = report(&vault, id)?;
    if existing.as_ref().is_some_and(|journal| !journal.ready) {
        update_harness_cycle(&vault, id, |state| {
            if state.stage == HarnessStage::ReadyToPromote {
                state.stage = HarnessStage::RecoveryRequired;
                state.reason =
                    "Interrupted activation preparation; recovery artifacts retained".into();
            }
            Ok(())
        })?;
        bail!("activation preparation was interrupted; retained artifacts require recovery");
    }
    let maximum = plan
        .settings
        .maximum_active_seconds
        .saturating_sub(cycle.active_seconds)
        .min(180);
    let activity = HarnessActivity::begin(vault.clone(), plan.clone(), maximum).await?;
    let result = activity
        .run(async {
            let source = artifact.clone();
            let target = destination.clone();
            let expected = plan.baseline_binary_digest.clone();
            let evaluated = harness.candidate_binary_digest.clone();
            let mode = tokio::task::spawn_blocking(move || -> Result<u32> {
                let metadata = executable_metadata(&target)?;
                executable_metadata(&source)?;
                if artifact_digest(&target)? != expected || artifact_digest(&source)? != evaluated {
                    bail!("baseline or candidate executable changed before activation");
                }
                Ok(metadata.mode() & 0o777)
            })
            .await??;
            let temporary = tempfile::tempdir()?;
            let mut journal = if let Some(journal) = existing {
                if journal.destination_mode != mode
                    || journal.evaluated_digest != harness.candidate_binary_digest
                {
                    bail!("activation executable metadata changed");
                }
                journal
            } else {
                let directory = tempfile::Builder::new()
                    .prefix(".ca-harness-")
                    .tempdir_in(destination.parent().context("destination parent missing")?)?
                    .keep();
                std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))?;
                let journal = PreparedArtifacts {
                    guidance: None,
                    plan_digest: plan.digest.clone(),
                    candidate_digest: candidate.clone(),
                    trials_digest: decision.trials_digest.clone(),
                    destination: destination.clone(),
                    directory,
                    baseline_digest: plan.baseline_binary_digest.clone(),
                    evaluated_digest: harness.candidate_binary_digest.clone(),
                    destination_mode: mode,
                    signed_digest: None,
                    baseline_smoke: Vec::new(),
                    candidate_smoke: Vec::new(),
                    ready: false,
                };
                vault.write_mutable(&journal_path(id), &journal)?;
                let copy = journal.clone();
                tokio::task::spawn_blocking(move || -> Result<()> {
                    copy_binary(
                        &copy.destination,
                        &copy.directory.join("baseline"),
                        &copy.baseline_digest,
                    )?;
                    copy_binary(
                        &artifact,
                        &copy.directory.join("candidate"),
                        &copy.evaluated_digest,
                    )?;
                    std::fs::File::open(&copy.directory)?.sync_all()?;
                    Ok(())
                })
                .await??;
                signature(&journal.directory.join("candidate"), true, temporary.path()).await?;
                journal
            };
            let baseline = journal.directory.join("baseline");
            let candidate_path = journal.directory.join("candidate");
            let before_smoke = journal.clone();
            let before_smoke_digest = tokio::task::spawn_blocking(move || -> Result<String> {
                executable_metadata(&before_smoke.directory.join("baseline"))?;
                executable_metadata(&before_smoke.directory.join("candidate"))?;
                if artifact_digest(&before_smoke.directory.join("baseline"))?
                    != before_smoke.baseline_digest
                {
                    bail!("baseline recovery copy changed before verification");
                }
                let digest = artifact_digest(&before_smoke.directory.join("candidate"))?;
                if before_smoke
                    .signed_digest
                    .as_ref()
                    .is_some_and(|expected| expected != &digest)
                {
                    bail!("signed candidate changed before verification");
                }
                Ok(digest)
            })
            .await??;
            signature(&baseline, false, temporary.path()).await?;
            signature(&candidate_path, false, temporary.path()).await?;
            let baseline_smoke = smoke_selected(
                &baseline,
                temporary.path(),
                plan.vm.as_ref(),
                activity.remaining()?,
            )
            .await?;
            let candidate_smoke = smoke_selected(
                &candidate_path,
                temporary.path(),
                plan.vm.as_ref(),
                activity.remaining()?,
            )
            .await?;
            let guidance = if harness.candidate_instructions != crate::harness_guidance::BASELINE
                || !harness.candidate_hints.is_empty()
            {
                Some(crate::harness_guidance::SourceGuidance::from_evaluated(
                    identity_selected(
                        &candidate_path,
                        temporary.path(),
                        plan.vm.as_ref(),
                        activity.remaining()?,
                    )
                    .await?,
                    &inputs.provider,
                    &harness,
                )?)
            } else {
                None
            };
            let checked = journal.clone();
            let signed = tokio::task::spawn_blocking(move || -> Result<String> {
                if executable_metadata(&checked.destination)?.mode() & 0o777
                    != checked.destination_mode
                {
                    bail!("activation destination mode changed during preparation");
                }
                executable_metadata(&checked.directory.join("baseline"))?;
                executable_metadata(&checked.directory.join("candidate"))?;
                if artifact_digest(&checked.destination)? != checked.baseline_digest
                    || artifact_digest(&checked.directory.join("baseline"))?
                        != checked.baseline_digest
                {
                    bail!("baseline changed during activation preparation");
                }
                artifact_digest(&checked.directory.join("candidate"))
            })
            .await??;
            if signed != before_smoke_digest {
                bail!("signed candidate changed during verification");
            }
            if journal.ready
                && (journal.signed_digest.as_deref() != Some(&signed)
                    || journal.guidance != guidance
                    || journal.baseline_smoke != baseline_smoke
                    || journal.candidate_smoke != candidate_smoke)
            {
                bail!("prepared activation artifacts changed");
            }
            journal.signed_digest = Some(signed);
            journal.guidance = guidance;
            journal.baseline_smoke = baseline_smoke;
            journal.candidate_smoke = candidate_smoke;
            journal.ready = true;
            vault.write_mutable(&journal_path(id), &journal)?;
            Ok(journal)
        })
        .await;
    if result.is_err() {
        update_harness_cycle(&vault, id, |state| {
            if state.stage == HarnessStage::ReadyToPromote {
                state.stage = HarnessStage::RecoveryRequired;
                state.reason = "Activation preparation failed; preserve recovery artifacts".into();
            }
            Ok(())
        })?;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[tokio::test]
    #[ignore = "requires fresh CA_HARNESS_TEST_BINARY and macOS sandbox"]
    async fn hrs253_signed_artifact_reports_its_compiled_identity_offline() {
        let binary =
            PathBuf::from(std::env::var_os("CA_HARNESS_TEST_BINARY").expect("fresh binary"));
        let temporary = tempfile::tempdir().unwrap();
        std::fs::create_dir(temporary.path().join("home")).unwrap();
        signature(&binary, false, temporary.path()).await.unwrap();
        assert_eq!(
            identity(&binary, temporary.path()).await.unwrap(),
            env!("CA_BUILD_DIGEST")
        );
    }

    #[test]
    fn hrs253_activation_copy_rejects_links_drift_and_unsafe_modes() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        std::fs::write(&source, b"frozen executable bytes").unwrap();
        std::fs::set_permissions(&source, std::fs::Permissions::from_mode(0o700)).unwrap();
        let digest = artifact_digest(&source).unwrap();
        let copied = root.path().join("copy");
        copy_binary(&source, &copied, &digest).unwrap();
        assert_eq!(artifact_digest(&copied).unwrap(), digest);
        assert!(copy_binary(&source, &root.path().join("wrong"), &"a".repeat(64)).is_err());
        std::os::unix::fs::symlink(&source, root.path().join("link")).unwrap();
        assert!(copy_binary(
            &root.path().join("link"),
            &root.path().join("link-copy"),
            &digest
        )
        .is_err());
        std::fs::hard_link(&source, root.path().join("hard")).unwrap();
        assert!(copy_binary(&source, &root.path().join("hard-copy"), &digest).is_err());
        std::fs::remove_file(root.path().join("hard")).unwrap();
        std::fs::set_permissions(&source, std::fs::Permissions::from_mode(0o777)).unwrap();
        assert!(copy_binary(&source, &root.path().join("unsafe-copy"), &digest).is_err());
        assert_eq!(std::fs::read(&source).unwrap(), b"frozen executable bytes");
    }
}
