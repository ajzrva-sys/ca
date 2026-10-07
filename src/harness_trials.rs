//! Resumable coding trials. The host chooses the next frozen lane, owns the
//! budget and verifier, and journals a receipt before admitting it to a cycle.
use crate::agent::client::AgentClient;
use crate::harness_activity::HarnessActivity;
use crate::harness_budget::{BudgetedBackend, FrozenHarnessProvider};
use crate::harness_cases::{validate_frozen_suite, FrozenCodingCase};
use crate::harness_execution::{run_isolated_worker, WorkerExecution};
use crate::harness_verifier::{verify_coding_workspace, VerifiedCodingResult};
use crate::harness_worker::WorkerSetup;
use anyhow::{bail, Context, Result};
use codex_team::self_improvement::{
    canonical_digest, harness_trial_schedule, load_harness_cycle, record_harness_trial,
    HarnessLane, HarnessPlan, HarnessStage, HarnessTrial, LearningVault,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const COMPLETION_CONTRACT: &str = "\nWhen finished, return only a JSON object with one boolean field, completed. Set it to true only if you believe the requested behavior is implemented and verified. Host checks determine success independently.";

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrialHarness {
    #[serde(default, skip_serializing_if = "std::collections::BTreeSet::is_empty")]
    pub baseline_hints: crate::harness_hints::HintPolicy,
    #[serde(default, skip_serializing_if = "std::collections::BTreeSet::is_empty")]
    pub candidate_hints: crate::harness_hints::HintPolicy,
    pub baseline_instructions: String,
    pub candidate_instructions: String,
    pub candidate_binary_digest: String,
    pub candidate_commit: Option<String>,
}
impl TrialHarness {
    pub fn digest(&self) -> Result<String> {
        canonical_digest(self)
    }
    pub(crate) fn validate(&self, plan: &HarnessPlan) -> Result<()> {
        if (self.candidate_hints != self.baseline_hints
            && (self.candidate_commit.is_some()
                || self.candidate_instructions != self.baseline_instructions
                || !crate::harness_hints::single_addition(
                    &self.baseline_hints,
                    &self.candidate_hints,
                )))
            || self.candidate_commit.is_some() != !plan.source_files.is_empty()
            || self.candidate_commit.as_ref().is_some_and(|commit| {
                !matches!(commit.len(), 40 | 64)
                    || !commit.bytes().all(|byte| byte.is_ascii_hexdigit())
            })
            || [&self.baseline_instructions, &self.candidate_instructions]
                .iter()
                .any(|text| text.is_empty() || text.len() > 120 * 1024)
            || self.candidate_binary_digest.len() != 64
            || !self
                .candidate_binary_digest
                .bytes()
                .all(|c| c.is_ascii_hexdigit())
            || (self.candidate_commit.is_none()
                && self.candidate_binary_digest != plan.baseline_binary_digest)
        {
            bail!("invalid frozen trial harness");
        }
        Ok(())
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Attempt {
    plan_digest: String,
    candidate_digest: String,
    case_id: String,
    repetition: u8,
    lane: HarnessLane,
    starting_cost: u64,
    receipt: Option<HarnessTrial>,
    diagnostic: Option<String>,
    verification_artifact: Option<String>,
}
impl Attempt {
    fn path(&self) -> Result<String> {
        Ok(format!(
            "harness/attempts/{}.json",
            canonical_digest(&(
                &self.plan_digest,
                &self.candidate_digest,
                &self.case_id,
                self.repetition,
                self.lane,
            ))?
        ))
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TrialEvidence {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    worker_failure: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    provider_requests: Option<u64>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeSet::is_empty")]
    hints_applied: crate::harness_hints::HintPolicy,
    verification: VerifiedCodingResult,
    worker_exit: i32,
    claimed_completed: Option<bool>,
    files: std::collections::BTreeMap<String, Vec<u8>>,
}

fn validate_saved_attempt(
    vault: &LearningVault,
    attempt: &Attempt,
    expected: &Attempt,
    case: &FrozenCodingCase,
) -> Result<HarnessTrial> {
    if attempt.path()? != expected.path()? {
        bail!("trial journal identity changed");
    }
    let receipt = attempt
        .receipt
        .clone()
        .context("trial has no completed receipt")?;
    if receipt.plan_digest != expected.plan_digest
        || receipt.candidate_digest != expected.candidate_digest
        || receipt.case_id != expected.case_id
        || receipt.repetition != expected.repetition
        || receipt.lane != expected.lane
    {
        bail!("trial receipt identity changed");
    }
    if !receipt.infrastructure_failure {
        let path = attempt
            .verification_artifact
            .as_ref()
            .context("completed trial has no verification artifact")?;
        let evidence: TrialEvidence = vault
            .read_mutable(path)?
            .context("verification artifact is missing")?;
        if path
            != &format!(
                "harness/attempts/{}-verification.json",
                canonical_digest(&evidence)?
            )
            || evidence.verification.case_id != case.case.id
            || evidence.verification.verifier_digest != case.case.verifier_digest
            || canonical_digest(&evidence.files)? != evidence.verification.implementation_digest
        {
            bail!("saved verification evidence changed");
        }
        let passed = evidence.verification.public_passed && evidence.verification.hidden_passed;
        if receipt.verified_pass
            != (passed && evidence.worker_exit == 0 && evidence.claimed_completed == Some(true))
            || receipt.false_completion != (!passed && evidence.claimed_completed == Some(true))
            || receipt.output_digest
                != canonical_digest(&(
                    &case.fixture_id,
                    &case.case.verifier_digest,
                    evidence.verification.public_passed,
                    evidence.verification.hidden_passed,
                ))?
        {
            bail!("saved receipt disagrees with host verification");
        }
    }
    Ok(receipt)
}

/// A favorable score cannot promote middleware that never ran. Exposure is
/// host-checked at the provider boundary and bound into each verification artifact.
pub(crate) fn verify_hint_exposure(vault: &LearningVault, id: &str) -> Result<()> {
    let (inputs, harness) = crate::harness_generation::load_candidate(vault, id)?;
    let added = harness
        .candidate_hints
        .difference(&harness.baseline_hints)
        .copied()
        .collect::<Vec<_>>();
    if added.is_empty() {
        return Ok(());
    }
    if added.len() != 1 {
        bail!("hint experiment changes more than one intervention");
    }
    let (plan, cycle) = load_harness_cycle(vault, id)?;
    let mut observed = 0;
    for trial in &cycle.trials {
        let Some(case) = inputs.cases.iter().find(|case| {
            case.case.id == trial.case_id && case.case.predicted_fix && !case.case.held_out
        }) else {
            continue;
        };
        if trial.lane != HarnessLane::Candidate {
            continue;
        }
        let expected = Attempt {
            plan_digest: plan.digest.clone(),
            candidate_digest: harness.digest()?,
            case_id: trial.case_id.clone(),
            repetition: trial.repetition,
            lane: trial.lane,
            starting_cost: 0,
            receipt: None,
            diagnostic: None,
            verification_artifact: None,
        };
        let attempt: Attempt = vault
            .read_mutable(&expected.path()?)?
            .context("hint trial journal missing")?;
        if validate_saved_attempt(vault, &attempt, &expected, case)? != *trial {
            bail!("hint trial receipt changed");
        }
        let artifact: TrialEvidence = vault
            .read_mutable(
                attempt
                    .verification_artifact
                    .as_deref()
                    .context("hint trial evidence missing")?,
            )?
            .context("hint verification evidence missing")?;
        if !artifact.hints_applied.contains(&added[0]) {
            bail!("added hint was not delivered in every candidate target repetition");
        }
        observed += 1;
    }
    let expected = inputs
        .cases
        .iter()
        .filter(|case| case.case.predicted_fix && !case.case.held_out)
        .count()
        * usize::from(plan.repetitions);
    if observed != expected {
        bail!("hint experiment has incomplete exposure evidence");
    }
    Ok(())
}

/// Pin a checked executable in a private, non-writable worker grant. Launches
/// never race a later rebuild or replacement of the caller's source path.
struct PinnedBinary {
    _directory: tempfile::TempDir,
    path: PathBuf,
}
fn pin_binary(source: &Path, expected: &str) -> Result<PinnedBinary> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("ca");
    let mut input = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(source)?;
    let metadata = input.metadata()?;
    if !metadata.is_file() || metadata.len() > 2 * 1024 * 1024 * 1024 {
        bail!("invalid executable artifact");
    }
    let mut output = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o500)
        .open(&path)?;
    let mut hash = Sha256::new();
    let mut bytes = [0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        let count = input.read(&mut bytes)?;
        if count == 0 {
            break;
        }
        total += count as u64;
        if total > metadata.len() {
            bail!("executable changed while pinning");
        }
        hash.update(&bytes[..count]);
        output.write_all(&bytes[..count])?;
    }
    if total != metadata.len() || format!("{:x}", hash.finalize()) != expected {
        bail!("executable differs from frozen digest");
    }
    output.sync_all()?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o500))?;
    Ok(PinnedBinary {
        _directory: directory,
        path,
    })
}

/// Run at most one trial. A caller can yield to foreground work between calls.
/// Interrupted attempts are never replayed; completed journals can be admitted
/// after a crash without issuing another provider request.
#[allow(clippy::too_many_arguments)]
pub async fn run_next_trial(
    vault: LearningVault,
    cycle_id: &str,
    client: AgentClient,
    provider: FrozenHarnessProvider,
    cases: &[FrozenCodingCase],
    harness: &TrialHarness,
    baseline_binary: &Path,
    candidate_binary: &Path,
    repository: &Path,
) -> Result<Option<HarnessTrial>> {
    let store = vault.clone();
    let _worker = tokio::task::spawn_blocking(move || store.acquire_worker_lock()).await??;
    let (plan, state) = load_harness_cycle(&vault, cycle_id)?;
    crate::harness_vm::validate_backend(&plan).await?;
    let repository_path = crate::harness_snapshot::resolve(&vault, &plan, repository)?;
    let repository = repository_path.as_path();
    validate_frozen_suite(&plan, cases)?;
    harness.validate(&plan)?;
    let candidate_digest = harness.digest()?;
    if state.candidate_digest.as_deref() != Some(&candidate_digest)
        || state.candidate_commit != harness.candidate_commit
    {
        bail!("candidate harness binding changed");
    }
    let Some((case_id, repetition, lane)) =
        harness_trial_schedule(&plan)?
            .into_iter()
            .find(|(id, rep, lane)| {
                !state.trials.iter().any(|trial| {
                    &trial.case_id == id && trial.repetition == *rep && trial.lane == *lane
                })
            })
    else {
        return Ok(None);
    };
    let case = cases
        .iter()
        .find(|case| case.case.id == case_id)
        .context("missing frozen case")?
        .clone();
    let mut attempt = Attempt {
        plan_digest: plan.digest.clone(),
        candidate_digest,
        case_id,
        repetition,
        lane,
        starting_cost: state.settled_microusd,
        receipt: None,
        diagnostic: None,
        verification_artifact: None,
    };
    let broker;
    let path = attempt.path()?;
    {
        let _lock = vault.acquire_lock()?;
        if let Some(previous) = vault.read_mutable::<Attempt>(&path)? {
            if previous.receipt.is_some() {
                let receipt = validate_saved_attempt(&vault, &previous, &attempt, &case)?;
                drop(_lock);
                record_harness_trial(&vault, cycle_id, receipt.clone())?;
                return Ok(Some(receipt));
            }
            let (_, mut current) = load_harness_cycle(&vault, cycle_id)?;
            current.stage = HarnessStage::RecoveryRequired;
            current.reason =
                "Trial was interrupted; its reservations and evidence are retained".into();
            vault.write_mutable(&format!("harness/cycles/{cycle_id}.json"), &current)?;
            bail!("trial was interrupted; preserve its reservations and inspect recovery evidence before continuing");
        }
        let (_, current) = load_harness_cycle(&vault, cycle_id)?;
        if !matches!(
            current.stage,
            HarnessStage::CandidateReady | HarnessStage::Evaluating
        ) || !current.reservations.is_empty()
            || !current.active_reservations.is_empty()
        {
            bail!("cycle cannot start a trial in its current state");
        }
        broker = BudgetedBackend::new(client, vault.clone(), plan.clone(), provider.clone())?;
        vault.write_mutable(&path, &attempt)?;
    }
    let seconds = plan
        .settings
        .maximum_active_seconds
        .saturating_sub(state.active_seconds)
        .min(300);
    let activity = HarnessActivity::begin(vault.clone(), plan.clone(), seconds).await?;
    let broker = broker.in_activity(activity.clone())?;
    let started = Instant::now();
    let result = activity
        .run(async {
            crate::harness_snapshot::verify_contents(&plan, repository).await?;
            let (baseline, baseline_hash) = (
                baseline_binary.to_path_buf(),
                plan.baseline_binary_digest.clone(),
            );
            let host = tokio::task::spawn_blocking(move || pin_binary(&baseline, &baseline_hash))
                .await??;
            let worker = if lane == HarnessLane::Candidate && harness.candidate_commit.is_some() {
                let (binary, digest) = (
                    candidate_binary.to_path_buf(),
                    harness.candidate_binary_digest.clone(),
                );
                Some(tokio::task::spawn_blocking(move || pin_binary(&binary, &digest)).await??)
            } else {
                None
            };
            let temporary = tempfile::tempdir()?;
            let workspace = temporary.path().canonicalize()?.join("workspace");
            let (input, destination) = (case.clone(), workspace.clone());
            tokio::task::spawn_blocking(move || input.materialize(&destination)).await??;
            let measured = Instant::now();
            let instructions = match lane {
                HarnessLane::Baseline => &harness.baseline_instructions,
                HarnessLane::Candidate => &harness.candidate_instructions,
            };
            let remaining = activity.remaining()?;
            let verification_reserve = Duration::from_secs(if plan.vm.is_some() { 60 } else { 8 });
            if remaining <= verification_reserve {
                bail!("insufficient time to execute and verify the trial");
            }
            let execution = WorkerExecution {
                binary: worker
                    .as_ref()
                    .map_or_else(|| host.path.clone(), |worker| worker.path.clone()),
                setup: WorkerSetup {
                    schema: "ca.harness_worker.v1".into(),
                    run_id: crate::harness_automation::worker_run_id()?,
                    workspace: workspace.clone(),
                    provider: provider.source.provider.clone(),
                    model: provider.model.clone(),
                    hints: match lane {
                        HarnessLane::Baseline => harness.baseline_hints.clone(),
                        HarnessLane::Candidate => harness.candidate_hints.clone(),
                    },
                    instructions: format!("{instructions}{COMPLETION_CONTRACT}"),
                    task: case.task()?.into(),
                    maximum_turns: 24,
                    command_timeout_seconds: 60,
                },
                options: provider.options.clone(),
                read_only_roots: [PathBuf::from("/opt/homebrew")]
                    .into_iter()
                    .filter(|p| p.is_dir())
                    .collect(),
                denied_paths: vec![vault.root().into(), repository.canonicalize()?],
                deadline: remaining
                    .saturating_sub(verification_reserve)
                    .min(Duration::from_secs(180)),
            };
            let (worker_result, verified, workspace) = if let Some(vm) = &plan.vm {
                let worker_digest = if worker.is_some() {
                    &harness.candidate_binary_digest
                } else {
                    &plan.baseline_binary_digest
                };
                let (result, files) = crate::harness_vm::worker::run_isolated_worker(
                    vm,
                    &execution,
                    worker_digest,
                    &case,
                    &broker,
                )
                .await?;
                let imported = temporary.path().join("imported-workspace");
                let (input, destination) = (case.clone(), imported.clone());
                tokio::task::spawn_blocking(move || {
                    crate::harness_vm::results::materialize_result(&input, &files, &destination)
                })
                .await??;
                let verified = crate::harness_vm::verifier::verify_workspace(
                    vm,
                    &host.path,
                    &plan.baseline_binary_digest,
                    &imported,
                    &case,
                    activity.remaining()?,
                )
                .await?;
                (result, verified, imported)
            } else {
                let result = run_isolated_worker(&execution, &broker).await?;
                let verified = verify_coding_workspace(
                    &host.path,
                    repository,
                    &workspace,
                    &case,
                    activity.remaining()?,
                )
                .await?;
                (result, verified, workspace)
            };
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Completion {
                completed: bool,
            }
            let claimed = worker_result
                .final_message
                .as_deref()
                .and_then(|text| serde_json::from_str::<Completion>(text).ok());
            let claimed_completed = claimed.map(|value| value.completed);
            let passed = verified.public_passed && verified.hidden_passed;
            let verified_pass =
                passed && worker_result.exit_code == 0 && claimed_completed == Some(true);
            let false_completion = !passed && claimed_completed == Some(true);
            // Equal host-observed behavior, not model text or implementation bytes.
            let output_digest = canonical_digest(&(
                &case.fixture_id,
                &case.case.verifier_digest,
                verified.public_passed,
                verified.hidden_passed,
            ))?;
            let files = case.result_files(&workspace)?;
            if canonical_digest(&files)? != verified.implementation_digest {
                bail!("trial implementation changed after verification");
            }
            let artifact = serde_json::to_value(TrialEvidence {
                worker_failure: worker_result
                    .failure
                    .as_deref()
                    .and_then(crate::learning::sanitized_learning_text)
                    .map(|text| text.chars().take(1024).collect()),
                provider_requests: Some(worker_result.provider_requests),
                hints_applied: worker_result.hints_applied,
                verification: verified,
                worker_exit: worker_result.exit_code,
                claimed_completed,
                files,
            })?;
            let elapsed_millis = measured
                .elapsed()
                .as_millis()
                .try_into()
                .context("trial elapsed time overflow")?;
            Ok((
                verified_pass,
                false_completion,
                output_digest,
                artifact,
                elapsed_millis,
            ))
        })
        .await;
    let (_, after) = load_harness_cycle(&vault, cycle_id)?;
    let cost = after
        .settled_microusd
        .checked_sub(attempt.starting_cost)
        .context("trial cost counter moved backwards")?;
    let (verified_pass, false_completion, output_digest, infrastructure_failure, elapsed_millis) =
        match result {
            Ok((passed, false_completion, digest, artifact, elapsed)) => {
                let artifact_path = format!(
                    "harness/attempts/{}-verification.json",
                    canonical_digest(&artifact)?
                );
                let _lock = vault.acquire_lock()?;
                if let Some(existing) = vault.read_mutable::<serde_json::Value>(&artifact_path)? {
                    if existing != artifact {
                        bail!("verification artifact collision");
                    }
                } else {
                    vault.write_mutable(&artifact_path, &artifact)?;
                }
                attempt.verification_artifact = Some(artifact_path);
                (
                    passed,
                    false_completion,
                    digest,
                    !after.reservations.is_empty(),
                    elapsed,
                )
            }
            Err(error) => {
                attempt.diagnostic =
                    crate::learning::sanitized_learning_text(&format!("{error:#}"))
                        .map(|text| text.chars().take(2048).collect());
                (
                    false,
                    false,
                    canonical_digest(&"incomplete-host-trial")?,
                    true,
                    started
                        .elapsed()
                        .as_millis()
                        .try_into()
                        .context("trial elapsed time overflow")?,
                )
            }
        };
    let receipt = HarnessTrial {
        plan_digest: plan.digest,
        candidate_digest: attempt.candidate_digest.clone(),
        route_digest: plan.route_digest,
        case_id: attempt.case_id.clone(),
        repetition,
        lane,
        verified_pass,
        infrastructure_failure,
        false_completion,
        output_digest,
        cost_microusd: cost,
        elapsed_millis,
    };
    attempt.receipt = Some(receipt.clone());
    {
        let _lock = vault.acquire_lock()?;
        vault.write_mutable(&path, &attempt)?;
    }
    record_harness_trial(&vault, cycle_id, receipt.clone())?;
    Ok(Some(receipt))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::agent::client::ProviderCallPolicy;
    use crate::agent::provider::ProviderProtocol;
    use crate::agent::stream::ProviderStateSourceV1;
    use codex_team::self_improvement::{
        start_harness_cycle, update_harness_cycle, HarnessSettings, SourceRepositorySnapshotV1,
        HARNESS_SCHEMA,
    };

    fn digest(path: &Path) -> String {
        let mut file = std::fs::File::open(path).unwrap();
        let mut hash = Sha256::new();
        let mut buffer = [0u8; 65536];
        loop {
            let n = file.read(&mut buffer).unwrap();
            if n == 0 {
                break;
            }
            hash.update(&buffer[..n]);
        }
        format!("{:x}", hash.finalize())
    }

    #[test]
    fn hrs253_hint_promotion_requires_bound_exposure_in_every_target_repetition() {
        use crate::harness_hints::ExecutionHint;
        let root = tempfile::tempdir().unwrap();
        let vault = LearningVault::open_at(root.path().join("vault")).unwrap();
        let (plan, inputs, mut proposal) = crate::harness_generation::tests::inputs();
        proposal.candidate_instructions = inputs.baseline_instructions.clone();
        proposal.candidate_hints = Some([ExecutionHint::DependencyProbes].into_iter().collect());
        let plan =
            crate::harness_generation::tests::save_fixture(&vault, plan, inputs.clone(), proposal);
        let (_, harness) = crate::harness_generation::load_candidate(&vault, &plan.id).unwrap();
        let case = inputs
            .cases
            .iter()
            .find(|case| case.case.predicted_fix && !case.case.held_out)
            .unwrap();
        assert!(verify_hint_exposure(&vault, &plan.id).is_err());
        // Synthetic artifacts exercise this additional gate, not coding correctness.
        let save_exposure = |delivered| {
            let files =
                std::collections::BTreeMap::from([("src/lib.rs".into(), b"fixture".to_vec())]);
            let artifact = TrialEvidence {
                worker_failure: None,
                provider_requests: None,
                hints_applied: if delivered {
                    [ExecutionHint::DependencyProbes].into_iter().collect()
                } else {
                    Default::default()
                },
                verification: VerifiedCodingResult {
                    case_id: case.case.id.clone(),
                    verifier_digest: case.case.verifier_digest.clone(),
                    implementation_digest: canonical_digest(&files).unwrap(),
                    public_passed: true,
                    hidden_passed: true,
                    failure: None,
                    build_diagnostic: None,
                },
                worker_exit: 0,
                claimed_completed: Some(true),
                files,
            };
            let path = format!(
                "harness/attempts/{}-verification.json",
                canonical_digest(&artifact).unwrap()
            );
            vault.write_mutable(&path, &artifact).unwrap();
            let mut trials = Vec::new();
            for repetition in 0..plan.repetitions {
                let receipt = HarnessTrial {
                    plan_digest: plan.digest.clone(),
                    candidate_digest: harness.digest().unwrap(),
                    route_digest: plan.route_digest.clone(),
                    case_id: case.case.id.clone(),
                    repetition,
                    lane: HarnessLane::Candidate,
                    verified_pass: true,
                    infrastructure_failure: false,
                    false_completion: false,
                    output_digest: canonical_digest(&(
                        &case.fixture_id,
                        &case.case.verifier_digest,
                        true,
                        true,
                    ))
                    .unwrap(),
                    cost_microusd: 80,
                    elapsed_millis: 10,
                };
                let attempt = Attempt {
                    plan_digest: plan.digest.clone(),
                    candidate_digest: harness.digest().unwrap(),
                    case_id: case.case.id.clone(),
                    repetition,
                    lane: HarnessLane::Candidate,
                    starting_cost: 0,
                    receipt: Some(receipt.clone()),
                    diagnostic: None,
                    verification_artifact: Some(path.clone()),
                };
                vault
                    .write_mutable(&attempt.path().unwrap(), &attempt)
                    .unwrap();
                trials.push(receipt);
            }
            update_harness_cycle(&vault, &plan.id, |cycle| {
                cycle.trials = trials;
                Ok(())
            })
            .unwrap();
            path
        };
        save_exposure(false);
        assert!(verify_hint_exposure(&vault, &plan.id)
            .unwrap_err()
            .to_string()
            .contains("not delivered"));
        let path = save_exposure(true);
        verify_hint_exposure(&vault, &plan.id).unwrap();
        vault.remove_mutable(&path).unwrap();
        assert!(verify_hint_exposure(&vault, &plan.id).is_err());
    }

    #[test]
    fn hrs253_pinned_executable_survives_source_replacement_and_rejects_drift() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source");
        std::fs::write(&source, "original executable").unwrap();
        let expected = digest(&source);
        let pinned = pin_binary(&source, &expected).unwrap();
        std::fs::write(&source, "replacement").unwrap();
        assert_eq!(digest(&pinned.path), expected);
        assert!(pin_binary(&source, &expected).is_err());
    }

    pub(crate) fn fixture_plan(
        provider: &FrozenHarnessProvider,
        binary_digest: &str,
    ) -> (HarnessPlan, Vec<FrozenCodingCase>, TrialHarness) {
        let repository = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut cases = crate::harness_cases::freeze_reasoning_cases(repository).unwrap();
        let mut target = cases[0].clone();
        target.case.id = "target".into();
        target.case.predicted_fix = true;
        cases.push(target);
        for (id, source) in [
            (
                "held-a",
                "pub fn truncate_utf8(_: &str, _: usize) -> &str { \"\" }",
            ),
            (
                "held-b",
                "pub fn truncate_utf8(s: &str, _: usize) -> &str { s }",
            ),
        ] {
            let mut held = cases[0].clone();
            held.case.id = id.into();
            held.case.held_out = true;
            held.files
                .insert("src/lib.rs".into(), source.as_bytes().to_vec());
            held.case.input_digest = canonical_digest(&held.files).unwrap();
            cases.push(held);
        }
        let mut plan = HarnessPlan {
            vm: None,
            preparation_seconds: None,
            guidance_source: None,
            activation_executable: None,
            source_files: Default::default(),
            schema: HARNESS_SCHEMA.into(),
            id: "real-trial".into(),
            baseline: SourceRepositorySnapshotV1 {
                schema: "test".into(),
                branch: "main".into(),
                head: "a".repeat(40),
                tracked_tree_digest: "a".repeat(64),
                submodule_digest: "a".repeat(64),
                repository_fingerprint: "a".repeat(64),
            },
            baseline_binary_digest: binary_digest.into(),
            route_digest: provider.route_digest().unwrap(),
            price_digest: provider.price_digest().unwrap(),
            evidence_ids: vec!["task:event".into()],
            cases: cases.iter().map(|case| case.case.clone()).collect(),
            repetitions: 3,
            settings: HarnessSettings::default(),
            digest: String::new(),
        };
        plan.seal().unwrap();
        let harness = TrialHarness {
            baseline_hints: Default::default(),
            candidate_hints: Default::default(),
            baseline_instructions: "Repair the requested code and verify it.".into(),
            candidate_instructions:
                "Repair and verify the requested behavior, checking Unicode boundaries.".into(),
            candidate_binary_digest: binary_digest.into(),
            candidate_commit: None,
        };
        (plan, cases, harness)
    }

    #[tokio::test]
    async fn hrs253_interrupted_attempt_stops_without_replaying_or_resetting_budgets() {
        let provider = crate::harness_budget::tests::frozen();
        let (plan, cases, harness) = fixture_plan(&provider, &"a".repeat(64));
        let directory = tempfile::tempdir().unwrap();
        let vault = LearningVault::open_at(directory.path().join("vault")).unwrap();
        start_harness_cycle(&vault, &plan, 0).unwrap();
        update_harness_cycle(&vault, &plan.id, |state| {
            state.stage = HarnessStage::CandidateReady;
            state.candidate_digest = Some(harness.digest()?);
            state.reserve(&plan, "unfinished-post", 200)?;
            state.reserve_active_time(&plan, "unfinished-work", 30)?;
            Ok(())
        })
        .unwrap();
        let attempt = Attempt {
            plan_digest: plan.digest.clone(),
            candidate_digest: harness.digest().unwrap(),
            case_id: cases[0].case.id.clone(),
            repetition: 0,
            lane: HarnessLane::Baseline,
            starting_cost: 0,
            receipt: None,
            diagnostic: None,
            verification_artifact: None,
        };
        vault
            .write_mutable(&attempt.path().unwrap(), &attempt)
            .unwrap();
        let config = crate::config::ProviderConfig {
            base_url: "http://127.0.0.1:1/v1".into(),
            api_key: "synthetic".into(),
            protocol: ProviderProtocol::ChatCompletions,
            prompt_cache_mode: Default::default(),
            models: vec![provider.model.clone()],
            timeout: Default::default(),
        };
        let client = AgentClient::with_call_policy(
            "fake",
            &config,
            &provider.model,
            ProviderCallPolicy::default(),
            crate::request::RequestStore::default(),
        );
        assert!(run_next_trial(
            vault.clone(),
            &plan.id,
            client,
            provider,
            &cases,
            &harness,
            Path::new("/unused"),
            Path::new("/unused"),
            Path::new(env!("CARGO_MANIFEST_DIR"))
        )
        .await
        .is_err());
        let (_, state) = load_harness_cycle(&vault, &plan.id).unwrap();
        assert_eq!(state.stage, HarnessStage::RecoveryRequired);
        assert_eq!(state.reservations["unfinished-post"], 200);
        assert_eq!(state.active_reservations["unfinished-work"], 30);
        assert!(state.trials.is_empty());
    }

    #[tokio::test]
    #[ignore = "requires CA_HARNESS_TEST_BINARY naming a fresh ca and macOS sandbox"]
    async fn hrs253_real_trial_reserves_verifies_persists_and_recovers_without_replay() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let binary =
            PathBuf::from(std::env::var_os("CA_HARNESS_TEST_BINARY").expect("fresh binary"));
        let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
            .canonicalize()
            .unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        let mut provider = crate::harness_budget::tests::frozen();
        // The real worker advertises coding tools in addition to the task.
        provider.context_tokens = 32768;
        provider.source =
            ProviderStateSourceV1::from_route("fake", ProviderProtocol::ChatCompletions, &url)
                .unwrap();
        let (plan, cases, harness) = fixture_plan(&provider, &digest(&binary));
        let temporary = tempfile::tempdir().unwrap();
        let vault = LearningVault::open_at(temporary.path().join("vault")).unwrap();
        start_harness_cycle(&vault, &plan, 0).unwrap();
        update_harness_cycle(&vault, &plan.id, |state| {
            state.stage = HarnessStage::CandidateReady;
            state.candidate_digest = Some(harness.digest()?);
            Ok(())
        })
        .unwrap();
        let store = vault.clone();
        let repair = std::fs::read_to_string(
            repository.join("tests/reasoning_pilot_golden/unicode_parser_boundary/src/lib.rs"),
        )
        .unwrap();
        let mut server = tokio::spawn(async move {
            for turn in 0..2 {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut bytes = Vec::new();
                let mut chunk = [0; 8192];
                loop {
                    let n = socket.read(&mut chunk).await.unwrap();
                    assert!(n > 0);
                    bytes.extend_from_slice(&chunk[..n]);
                    assert!(bytes.len() <= 4 * 1024 * 1024);
                    if let Some(boundary) = bytes.windows(4).position(|v| v == b"\r\n\r\n") {
                        let headers = std::str::from_utf8(&bytes[..boundary]).unwrap();
                        let length: usize = headers
                            .lines()
                            .find_map(|line| {
                                let (key, value) = line.split_once(':')?;
                                key.eq_ignore_ascii_case("content-length")
                                    .then(|| value.trim().parse().unwrap())
                            })
                            .unwrap();
                        if bytes.len() >= boundary + 4 + length {
                            break;
                        }
                    }
                }
                let (_, state) = load_harness_cycle(&store, "real-trial").unwrap();
                assert_eq!(state.reservations.len(), 1);
                assert_eq!(state.active_reservations.len(), 1);
                assert_eq!(state.charged_requests.len(), turn);
                let delta = if turn == 0 {
                    serde_json::json!({"tool_calls":[{"index":0,"id":"repair","type":"function","function":{"name":"write_file","arguments":serde_json::json!({"path":"src/lib.rs","content":repair}).to_string()}}]})
                } else {
                    serde_json::json!({"content":"{\"completed\":true}"})
                };
                let body = format!(
                    "data: {}\n\ndata: [DONE]\n\n",
                    serde_json::json!({"choices":[{"index":0,"delta":delta,"finish_reason":if turn == 0 {"tool_calls"} else {"stop"}}],"usage":{"prompt_tokens":20,"completion_tokens":10,"total_tokens":30,"prompt_tokens_details":{"cached_tokens":0,"cache_write_tokens":0},"completion_tokens_details":{"reasoning_tokens":0}}})
                );
                socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).as_bytes()).await.unwrap();
            }
        });
        let config = crate::config::ProviderConfig {
            base_url: url,
            api_key: "synthetic-test-token".into(),
            protocol: ProviderProtocol::ChatCompletions,
            prompt_cache_mode: Default::default(),
            models: vec![provider.model.clone()],
            timeout: Default::default(),
        };
        let client = || {
            AgentClient::with_call_policy(
                "fake",
                &config,
                &provider.model,
                ProviderCallPolicy::default(),
                crate::request::RequestStore::default(),
            )
        };
        let receipt = run_next_trial(
            vault.clone(),
            &plan.id,
            client(),
            provider.clone(),
            &cases,
            &harness,
            &binary,
            &binary,
            &repository,
        )
        .await
        .unwrap()
        .unwrap();
        let server_result = tokio::time::timeout(Duration::from_secs(2), &mut server).await;
        server.abort(); // Also owns the listener if a preflight failed before both calls.
        let (_, state) = load_harness_cycle(&vault, &plan.id).unwrap();
        assert!(
            receipt.verified_pass,
            "receipt={receipt:?}; attempts={}",
            std::fs::read_dir(vault.root().join("harness/attempts"))
                .unwrap()
                .map(|entry| std::fs::read_to_string(entry.unwrap().path()).unwrap())
                .collect::<Vec<_>>()
                .join("\n")
        );
        server_result
            .expect("provider did not receive both requests")
            .expect("provider assertions failed");
        assert!(!receipt.infrastructure_failure);
        assert!(!receipt.false_completion);
        assert_eq!(receipt.cost_microusd, 80);
        assert_eq!(state.trials.len(), 1);
        assert_eq!(state.completed_work.len(), 1);
        assert!(state.active_reservations.is_empty() && state.reservations.is_empty());
        // Crash after journal publication but before cycle receipt admission.
        update_harness_cycle(&vault, &plan.id, |state| {
            state.trials.clear();
            state.stage = HarnessStage::CandidateReady;
            Ok(())
        })
        .unwrap();
        let recovered = run_next_trial(
            vault.clone(),
            &plan.id,
            client(),
            provider.clone(),
            &cases,
            &harness,
            &binary,
            &binary,
            &repository,
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(recovered, receipt);
        let (_, state) = load_harness_cycle(&vault, &plan.id).unwrap();
        assert_eq!(state.charged_requests.len(), 2);
        assert_eq!(state.settled_microusd, 80);
        let journal = Attempt {
            plan_digest: plan.digest.clone(),
            candidate_digest: harness.digest().unwrap(),
            case_id: cases[0].case.id.clone(),
            repetition: 0,
            lane: HarnessLane::Baseline,
            starting_cost: 0,
            receipt: None,
            diagnostic: None,
            verification_artifact: None,
        };
        let saved: Attempt = vault
            .read_mutable(&journal.path().unwrap())
            .unwrap()
            .unwrap();
        vault
            .remove_mutable(saved.verification_artifact.as_ref().unwrap())
            .unwrap();
        update_harness_cycle(&vault, &plan.id, |state| {
            state.trials.clear();
            Ok(())
        })
        .unwrap();
        assert!(run_next_trial(
            vault.clone(),
            &plan.id,
            client(),
            provider.clone(),
            &cases,
            &harness,
            &binary,
            &binary,
            &repository
        )
        .await
        .is_err());
        assert_eq!(
            load_harness_cycle(&vault, &plan.id)
                .unwrap()
                .1
                .charged_requests
                .len(),
            2
        );
    }
}
