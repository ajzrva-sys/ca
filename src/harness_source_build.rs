//! Compile and admit an exact source candidate only after the fixed host-owned
//! engineering matrix. A proposal or a successful build alone is insufficient.
use crate::harness_activity::HarnessActivity;
use crate::harness_engineering::{artifact_digest, run_gate_selected, spec_digest, GateReceipt};
use crate::harness_source_checkout::SourceCheckout;
use crate::harness_trials::TrialHarness;
use anyhow::{bail, Context, Result};
use codex_team::release_readiness::fixed_release_gates;
use codex_team::self_improvement::{
    canonical_digest, load_harness_cycle, update_harness_cycle, HarnessStage, LearningVault,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceBuild {
    plan_digest: String,
    checkout_digest: String,
    started: Option<String>,
    gates: BTreeMap<String, GateReceipt>,
    artifact_digest: Option<String>,
}

pub fn load_candidate(
    vault: &LearningVault,
    id: &str,
) -> Result<(
    crate::harness_generation::GuidanceInputs,
    TrialHarness,
    PathBuf,
)> {
    let (plan, state) = load_harness_cycle(vault, id)?;
    let saved =
        crate::harness_source_generation::load(vault, id)?.context("no source generation")?;
    let checkout: SourceCheckout = vault
        .read_mutable(&format!("harness/source-checkouts/{id}.json"))?
        .context("source checkout missing")?;
    let build: SourceBuild = vault
        .read_mutable(&format!("harness/source-builds/{id}.json"))?
        .context("source build missing")?;
    validate(&build, &checkout, &plan, &saved)?;
    let harness = harness(&build, &checkout, &saved)?;
    harness.validate(&plan)?;
    if state.candidate_digest.as_deref() != Some(harness.digest()?.as_str())
        || state.candidate_commit != harness.candidate_commit
    {
        bail!("source candidate admission changed");
    }
    Ok((
        saved.inputs.shared,
        harness,
        checkout.workspace.join("target/release/ca"),
    ))
}
fn validate(
    build: &SourceBuild,
    checkout: &SourceCheckout,
    plan: &codex_team::self_improvement::HarnessPlan,
    saved: &crate::harness_source_generation::SourceGeneration,
) -> Result<()> {
    saved.validate(plan)?;
    if checkout.started
        || checkout.candidate.is_none()
        || checkout.plan_digest != plan.digest
        || checkout.generation_digest != canonical_digest(saved)?
        || build.plan_digest != plan.digest
        || build.checkout_digest != canonical_digest(checkout)?
    {
        bail!("source build identity changed");
    }
    for (id, receipt) in &build.gates {
        let gate = fixed_release_gates()
            .iter()
            .find(|gate| gate.id == id)
            .context("unknown source engineering gate")?;
        if receipt.spec_digest != spec_digest(gate)? {
            bail!("source engineering gate changed");
        }
    }
    crate::harness_vm::engineering::validate_receipts(
        plan.vm.is_some(),
        &build.gates,
        build.artifact_digest.as_deref(),
    )?;
    Ok(())
}
fn harness(
    build: &SourceBuild,
    checkout: &SourceCheckout,
    saved: &crate::harness_source_generation::SourceGeneration,
) -> Result<TrialHarness> {
    if build.started.is_some()
        || build.gates.len() != fixed_release_gates().len()
        || build.gates.values().any(|gate| !gate.passed)
    {
        bail!("source engineering checks are incomplete or failed");
    }
    Ok(TrialHarness {
        baseline_hints: saved.inputs.shared.baseline_hints.clone(),
        candidate_hints: saved.inputs.shared.baseline_hints.clone(),
        baseline_instructions: saved.inputs.shared.baseline_instructions.clone(),
        candidate_instructions: saved.inputs.shared.baseline_instructions.clone(),
        candidate_binary_digest: build
            .artifact_digest
            .clone()
            .context("source artifact missing")?,
        candidate_commit: Some(
            checkout
                .candidate
                .as_ref()
                .context("source commit missing")?
                .head
                .clone(),
        ),
    })
}

fn allowance(vault: &LearningVault, id: &str, maximum: u64) -> Result<u64> {
    let (plan, _) = load_harness_cycle(vault, id)?;
    update_harness_cycle(vault, id, |state| {
        if !matches!(
            state.stage,
            HarnessStage::EvaluationFrozen
                | HarnessStage::CandidateReady
                | HarnessStage::Evaluating
        ) {
            bail!("source engineering work is paused or terminal");
        }
        let seconds = plan
            .settings
            .maximum_active_seconds
            .saturating_sub(state.active_seconds)
            .min(maximum);
        if seconds == 0 {
            state.stage = HarnessStage::BudgetExhausted;
            state.reason = "Source engineering activity budget exhausted".into();
            bail!("source engineering activity budget exhausted");
        }
        Ok(seconds)
    })
}

pub async fn build_and_admit(vault: LearningVault, id: &str) -> Result<TrialHarness> {
    let result = build_inner(vault.clone(), id).await;
    if result.is_err()
        && vault
            .read_mutable::<SourceBuild>(&format!("harness/source-builds/{id}.json"))?
            .is_some()
    {
        update_harness_cycle(&vault, id, |state| {
            if matches!(
                state.stage,
                HarnessStage::EvaluationFrozen
                    | HarnessStage::CandidateReady
                    | HarnessStage::Evaluating
            ) {
                state.stage = HarnessStage::RecoveryRequired;
                state.reason = "Source engineering interrupted or its evidence changed".into();
            }
            Ok(())
        })?;
    }
    result
}

async fn build_inner(vault: LearningVault, id: &str) -> Result<TrialHarness> {
    crate::harness_idle::boundary(&vault, id).await?;
    let _worker = vault.acquire_worker_lock()?;
    let (plan, state) = load_harness_cycle(&vault, id)?;
    if !matches!(
        state.stage,
        HarnessStage::EvaluationFrozen | HarnessStage::CandidateReady | HarnessStage::Evaluating
    ) || !state.reservations.is_empty()
        || !state.active_reservations.is_empty()
    {
        bail!("source build requires settled work in an active cycle");
    }
    let saved =
        crate::harness_source_generation::load(&vault, id)?.context("no source generation")?;
    let checkout: SourceCheckout = vault
        .read_mutable(&format!("harness/source-checkouts/{id}.json"))?
        .context("source checkout missing")?;
    let path = format!("harness/source-builds/{id}.json");
    let mut build = vault
        .read_mutable::<SourceBuild>(&path)?
        .unwrap_or(SourceBuild {
            plan_digest: plan.digest.clone(),
            checkout_digest: canonical_digest(&checkout)?,
            started: None,
            gates: BTreeMap::new(),
            artifact_digest: None,
        });
    validate(&build, &checkout, &plan, &saved)?;
    update_harness_cycle(&vault, id, |state| {
        state.engineering_checks_passed = false;
        Ok(())
    })?;
    if build.started.is_some() {
        update_harness_cycle(&vault, id, |state| {
            state.stage = HarnessStage::RecoveryRequired;
            state.reason = "Interrupted source engineering gate; no automatic replay".into();
            Ok(())
        })?;
        bail!("source engineering gate requires recovery");
    }
    let workspace = checkout.workspace.canonicalize()?;
    if !workspace.starts_with(
        vault
            .resolve_relative("harness/source-checkouts")?
            .canonicalize()?,
    ) {
        bail!("source build escaped private checkout root");
    }
    let temporary = tempfile::tempdir()?;
    std::fs::create_dir(temporary.path().join("home"))?;
    let activity =
        HarnessActivity::begin(vault.clone(), plan.clone(), allowance(&vault, id, 90)?).await?;
    activity
        .run(crate::harness_source_checkout::audit(
            &checkout,
            &saved,
            &plan,
            temporary.path(),
        ))
        .await?;
    for gate in fixed_release_gates() {
        crate::harness_idle::boundary(&vault, id).await?;
        if let Some(receipt) = build.gates.get(gate.id) {
            if !receipt.passed {
                bail!("saved source engineering gate failed: {}", gate.id);
            }
            continue;
        }
        let seconds = allowance(&vault, id, gate.timeout_seconds)?;
        build.started = Some(gate.id.into());
        {
            let _lock = vault.acquire_lock()?;
            vault.write_mutable(&path, &build)?;
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
        build.gates.insert(gate.id.into(), receipt);
        build.started = None;
        {
            let _lock = vault.acquire_lock()?;
            vault.write_mutable(&path, &build)?;
        }
        if !passed {
            update_harness_cycle(&vault, id, |state| {
                if state.stage == HarnessStage::Paused {
                    state.resume_stage = Some(HarnessStage::Rejected);
                } else {
                    state.stage = HarnessStage::Rejected;
                }
                state.reason = format!("Source engineering gate failed: {}", gate.id);
                Ok(())
            })?;
            bail!("source engineering gate failed: {}", gate.id);
        }
    }
    let activity =
        HarnessActivity::begin(vault.clone(), plan.clone(), allowance(&vault, id, 90)?).await?;
    let digest = activity
        .run(async {
            crate::harness_source_checkout::audit(&checkout, &saved, &plan, temporary.path())
                .await?;
            let artifact = workspace.join("target/release/ca");
            tokio::task::spawn_blocking(move || artifact_digest(&artifact)).await?
        })
        .await?;
    if build
        .artifact_digest
        .as_ref()
        .is_some_and(|saved| saved != &digest)
    {
        bail!("source engineering artifact changed");
    }
    build.artifact_digest = Some(digest);
    crate::harness_vm::engineering::validate_receipts(
        plan.vm.is_some(),
        &build.gates,
        build.artifact_digest.as_deref(),
    )?;
    let harness = harness(&build, &checkout, &saved)?;
    harness.validate(&plan)?;
    {
        let _lock = vault.acquire_lock()?;
        vault.write_mutable(&path, &build)?;
    }
    let digest = harness.digest()?;
    update_harness_cycle(&vault, id, |state| {
        if !state.reservations.is_empty() || !state.active_reservations.is_empty() {
            bail!("unsettled source work prevents admission");
        }
        if state
            .candidate_digest
            .as_ref()
            .is_some_and(|existing| existing != &digest)
            || state
                .candidate_commit
                .as_ref()
                .is_some_and(|existing| Some(existing) != harness.candidate_commit.as_ref())
        {
            bail!("source candidate changed before admission");
        }
        match state.stage {
            HarnessStage::EvaluationFrozen => state.stage = HarnessStage::CandidateReady,
            HarnessStage::Paused if state.resume_stage == Some(HarnessStage::EvaluationFrozen) => {
                state.resume_stage = Some(HarnessStage::CandidateReady)
            }
            HarnessStage::CandidateReady | HarnessStage::Evaluating | HarnessStage::Paused => {}
            _ => bail!("source cycle became terminal before admission"),
        }
        state.candidate_digest = Some(digest);
        state.candidate_commit = harness.candidate_commit.clone();
        state.engineering_checks_passed = true;
        Ok(())
    })?;
    Ok(harness)
}

pub fn report(vault: &LearningVault, id: &str) -> Result<Option<serde_json::Value>> {
    let (plan, _) = load_harness_cycle(vault, id)?;
    let Some(build) =
        vault.read_mutable::<SourceBuild>(&format!("harness/source-builds/{id}.json"))?
    else {
        return Ok(None);
    };
    let checkout: SourceCheckout = vault
        .read_mutable(&format!("harness/source-checkouts/{id}.json"))?
        .context("source checkout missing")?;
    let saved =
        crate::harness_source_generation::load(vault, id)?.context("source generation missing")?;
    validate(&build, &checkout, &plan, &saved)?;
    Ok(Some(
        serde_json::json!({"gates":build.gates,"started":build.started,"artifact_digest":build.artifact_digest,"required_gates":fixed_release_gates().len()}),
    ))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::harness_source_generation::{
        SourceEdit, SourceGeneration, SourceInputs, SourceProposal,
    };
    use codex_team::self_improvement::{start_harness_cycle, HarnessPlan};
    async fn fixture(
        root: &std::path::Path,
        bad_format: bool,
    ) -> (LearningVault, HarnessPlan, PathBuf) {
        Box::pin(fixture_with_activation(root, bad_format, false)).await
    }
    pub(crate) async fn fixture_with_activation(
        root: &std::path::Path,
        bad_format: bool,
        activation: bool,
    ) -> (LearningVault, HarnessPlan, PathBuf) {
        Box::pin(fixture_with_backend(root, bad_format, activation, None)).await
    }
    pub(crate) async fn fixture_with_backend(
        root: &std::path::Path,
        bad_format: bool,
        activation: bool,
        vm: Option<codex_team::self_improvement::FrozenHarnessVm>,
    ) -> (LearningVault, HarnessPlan, PathBuf) {
        let repository = root.join("repository");
        crate::harness_engineering::tests::source(&repository);
        let (mut plan, mut shared, _) = crate::harness_generation::tests::inputs();
        plan.vm = vm;
        if activation {
            shared
                .baseline_hints
                .insert(crate::harness_hints::ExecutionHint::DependencyProbes);
            let gate = fixed_release_gates()
                .iter()
                .find(|gate| gate.id == "release_all_features")
                .unwrap();
            assert!(
                run_gate_selected(&repository, gate, 120, plan.vm.as_ref())
                    .await
                    .unwrap()
                    .passed
            );
            let directory = root.join("installed");
            std::fs::create_dir(&directory).unwrap();
            let destination = directory.join("ca");
            std::fs::copy(repository.join("target/release/ca"), &destination).unwrap();
            plan.baseline_binary_digest = artifact_digest(&destination).unwrap();
            plan.activation_executable = Some(destination.canonicalize().unwrap());
        }
        plan.baseline = crate::harness_engineering::source_snapshot(&repository)
            .await
            .unwrap();
        plan.seal().unwrap();
        let text = std::fs::read_to_string(repository.join("src/bin/ca.rs")).unwrap();
        let mut inputs = SourceInputs {
            shared,
            files: BTreeMap::from([("src/bin/ca.rs".into(), text.clone())]),
        };
        plan.source_files
            .insert("src/bin/ca.rs".into(), canonical_digest(&text).unwrap());
        plan.seal().unwrap();
        inputs.shared.plan_digest = plan.digest.clone();
        let proposal = SourceProposal {
            evidence_ids: vec!["task:event".into()],
            root_cause: "fixture hypothesis".into(),
            prediction: "fixture prediction".into(),
            edits: vec![SourceEdit {
                path: "src/bin/ca.rs".into(),
                before_digest: canonical_digest(&text).unwrap(),
                old: "    println!(\"fixture 0.1.0\");".into(),
                new: if bad_format {
                    "println!(\"candidate\");"
                } else {
                    "    println!(\"candidate 0.1.1\");"
                }
                .into(),
            }],
        };
        let saved = SourceGeneration {
            inputs_digest: canonical_digest(&inputs).unwrap(),
            proposal_digest: Some(canonical_digest(&proposal).unwrap()),
            result_digest: Some(
                canonical_digest(&proposal.apply(&inputs, &plan).unwrap()).unwrap(),
            ),
            inputs,
            proposal: Some(proposal),
        };
        let vault = LearningVault::open_at(root.join("vault")).unwrap();
        start_harness_cycle(&vault, &plan, 0).unwrap();
        vault
            .write_mutable(
                &format!("harness/source-generations/{}.json", plan.id),
                &saved,
            )
            .unwrap();
        crate::harness_source_checkout::prepare(vault.clone(), &plan.id, &repository)
            .await
            .unwrap();
        (vault, plan, repository)
    }
    #[tokio::test]
    async fn hrs253_source_build_interrupted_gate_requires_recovery_without_replay() {
        let temp = tempfile::tempdir().unwrap();
        let (vault, plan, repository) =
            crate::harness_source_checkout::tests::fixture(temp.path()).await;
        let checkout =
            crate::harness_source_checkout::prepare(vault.clone(), &plan.id, &repository)
                .await
                .unwrap();
        let journal = SourceBuild {
            plan_digest: plan.digest.clone(),
            checkout_digest: canonical_digest(&checkout).unwrap(),
            started: Some("release_all_features".into()),
            gates: BTreeMap::new(),
            artifact_digest: None,
        };
        vault
            .write_mutable(&format!("harness/source-builds/{}.json", plan.id), &journal)
            .unwrap();
        assert!(build_and_admit(vault.clone(), &plan.id).await.is_err());
        let (_, state) = load_harness_cycle(&vault, &plan.id).unwrap();
        assert_eq!(state.stage, HarnessStage::RecoveryRequired);
        assert!(state.candidate_digest.is_none());
        assert!(!checkout.workspace.join("target").exists());
        assert_eq!(
            report(&vault, &plan.id).unwrap().unwrap()["started"],
            "release_all_features"
        );
    }

    #[tokio::test]
    #[ignore = "runs the fixed release matrix in a disposable macOS sandbox"]
    async fn hrs253_source_build_admits_exact_artifact_and_rechecks_it_on_recovery() {
        let temp = tempfile::tempdir().unwrap();
        let (vault, plan, repository) = fixture(temp.path(), false).await;
        let harness = build_and_admit(vault.clone(), &plan.id).await.unwrap();
        let (_, state) = load_harness_cycle(&vault, &plan.id).unwrap();
        assert_eq!(state.stage, HarnessStage::CandidateReady);
        assert!(state.engineering_checks_passed);
        assert_eq!(state.candidate_digest, Some(harness.digest().unwrap()));
        let (_, loaded, artifact) = load_candidate(&vault, &plan.id).unwrap();
        assert_eq!(loaded.digest().unwrap(), harness.digest().unwrap());
        assert_eq!(
            artifact_digest(&artifact).unwrap(),
            harness.candidate_binary_digest
        );
        assert_eq!(
            crate::harness_engineering::source_snapshot(&repository)
                .await
                .unwrap(),
            plan.baseline
        );
        let report_before = report(&vault, &plan.id).unwrap().unwrap();
        let recovered = build_and_admit(vault.clone(), &plan.id).await.unwrap();
        assert_eq!(recovered.digest().unwrap(), harness.digest().unwrap());
        assert_eq!(report(&vault, &plan.id).unwrap().unwrap(), report_before);
        use std::io::Write;
        std::fs::OpenOptions::new()
            .append(true)
            .open(&artifact)
            .unwrap()
            .write_all(b"tampered")
            .unwrap();
        assert!(build_and_admit(vault.clone(), &plan.id).await.is_err());
        assert!(
            !load_harness_cycle(&vault, &plan.id)
                .unwrap()
                .1
                .engineering_checks_passed
        );
        assert_eq!(
            crate::harness_engineering::source_snapshot(&repository)
                .await
                .unwrap(),
            plan.baseline
        );
    }
    #[tokio::test]
    #[ignore = "runs the fixed release matrix in a disposable macOS sandbox"]
    async fn hrs253_verified_source_import_preserves_branch_and_rechecks_receipts() {
        use codex_team::self_improvement::{harness_trial_schedule, HarnessLane, HarnessTrial};
        let temp = tempfile::tempdir().unwrap();
        let (vault, plan, repository) =
            Box::pin(fixture_with_activation(temp.path(), false, true)).await;
        let harness = build_and_admit(vault.clone(), &plan.id).await.unwrap();
        assert!(crate::harness_source_checkout::import_candidate(
            vault.clone(),
            &plan.id,
            &repository,
        )
        .await
        .is_err());
        update_harness_cycle(&vault, &plan.id, |state| {
            state.trials = harness_trial_schedule(&plan)?
                .into_iter()
                .map(|(case_id, repetition, lane)| HarnessTrial {
                    plan_digest: plan.digest.clone(),
                    candidate_digest: harness.digest().unwrap(),
                    route_digest: plan.route_digest.clone(),
                    case_id,
                    repetition,
                    lane,
                    verified_pass: true,
                    infrastructure_failure: false,
                    false_completion: false,
                    output_digest: "a".repeat(64),
                    cost_microusd: if lane == HarnessLane::Baseline {
                        100
                    } else {
                        80
                    },
                    elapsed_millis: 1,
                })
                .collect();
            state.decide(&plan)
        })
        .unwrap();
        let git = |arguments: &[&str]| {
            std::process::Command::new("/usr/bin/git")
                .args(arguments)
                .current_dir(&repository)
                .output()
                .unwrap()
        };
        let commit = harness.candidate_commit.as_deref().unwrap();
        assert!(!git(&["cat-file", "-e", commit]).status.success());
        let refs = git(&["show-ref"]).stdout;
        let fetch_head = repository.join(".git/FETCH_HEAD");
        std::fs::write(&fetch_head, b"existing fetch receipt\n").unwrap();
        crate::harness_source_checkout::import_candidate(vault.clone(), &plan.id, &repository)
            .await
            .unwrap();
        assert!(git(&["cat-file", "-e", commit]).status.success());
        assert_eq!(git(&["show-ref"]).stdout, refs);
        assert_eq!(
            std::fs::read(&fetch_head).unwrap(),
            b"existing fetch receipt\n"
        );
        assert_eq!(
            crate::harness_engineering::source_snapshot(&repository)
                .await
                .unwrap(),
            plan.baseline
        );
        let receipt = format!("harness/source/{}-import.json", plan.id);
        let saved = vault
            .read_mutable::<serde_json::Value>(&receipt)
            .unwrap()
            .unwrap();
        crate::harness_source_checkout::import_candidate(vault.clone(), &plan.id, &repository)
            .await
            .unwrap();
        assert_eq!(
            vault
                .read_mutable::<serde_json::Value>(&receipt)
                .unwrap()
                .unwrap(),
            saved
        );
        std::fs::write(repository.join("user-edit.txt"), "unfinished work").unwrap();
        assert!(crate::harness_source_checkout::import_candidate(
            vault.clone(),
            &plan.id,
            &repository,
        )
        .await
        .is_err());
        assert_eq!(
            std::fs::read_to_string(repository.join("user-edit.txt")).unwrap(),
            "unfinished work"
        );
        std::fs::remove_file(repository.join("user-edit.txt")).unwrap();
        update_harness_cycle(&vault, &plan.id, |state| {
            state.reserve_active_time(&plan, "interrupted-import", 1)
        })
        .unwrap();
        assert!(crate::harness_source_checkout::import_candidate(
            vault.clone(),
            &plan.id,
            &repository,
        )
        .await
        .is_err());
        assert_eq!(
            load_harness_cycle(&vault, &plan.id)
                .unwrap()
                .1
                .active_reservations["interrupted-import"],
            1
        );
        // Test-only settlement lets the independent evidence-drift check proceed.
        update_harness_cycle(&vault, &plan.id, |state| {
            state.settle_active_time(&plan, "interrupted-import", 1)
        })
        .unwrap();
        update_harness_cycle(&vault, &plan.id, |state| {
            state.trials[0].infrastructure_failure = true;
            Ok(())
        })
        .unwrap();
        assert!(crate::harness_source_checkout::import_candidate(
            vault.clone(),
            &plan.id,
            &repository
        )
        .await
        .is_err());
        assert_eq!(
            crate::harness_engineering::source_snapshot(&repository)
                .await
                .unwrap(),
            plan.baseline
        );
        update_harness_cycle(&vault, &plan.id, |state| {
            state.trials[0].infrastructure_failure = false;
            Ok(())
        })
        .unwrap();
        let staged = crate::harness_activation::prepare(vault.clone(), &plan.id)
            .await
            .unwrap();
        assert!(staged.ready);
        assert_eq!(
            staged.destination,
            *plan.activation_executable.as_ref().unwrap()
        );
        assert_eq!(
            artifact_digest(&staged.destination).unwrap(),
            plan.baseline_binary_digest
        );
        assert_eq!(
            artifact_digest(&staged.directory.join("baseline")).unwrap(),
            plan.baseline_binary_digest
        );
        assert_eq!(
            artifact_digest(&staged.directory.join("candidate")).unwrap(),
            staged.signed_digest.as_deref().unwrap()
        );
        assert_ne!(staged.baseline_smoke, staged.candidate_smoke);
        assert_eq!(staged.baseline_smoke.len(), 2);
        assert_eq!(
            crate::harness_activation::prepare(vault.clone(), &plan.id)
                .await
                .unwrap(),
            staged
        );
        let activation_path = format!("harness/source/{}-activation.json", plan.id);
        let mut interrupted = staged.clone();
        interrupted.ready = false;
        vault.write_mutable(&activation_path, &interrupted).unwrap();
        assert!(crate::harness_activation::prepare(vault.clone(), &plan.id)
            .await
            .is_err());
        assert_eq!(
            load_harness_cycle(&vault, &plan.id).unwrap().1.stage,
            HarnessStage::RecoveryRequired
        );
        assert_eq!(
            artifact_digest(&staged.destination).unwrap(),
            plan.baseline_binary_digest
        );
        // Restore the fixture's completed journal for the independent tamper check.
        vault.write_mutable(&activation_path, &staged).unwrap();
        update_harness_cycle(&vault, &plan.id, |state| {
            state.stage = HarnessStage::ReadyToPromote;
            Ok(())
        })
        .unwrap();
        std::fs::write(staged.directory.join("candidate"), b"changed").unwrap();
        assert!(crate::harness_activation::prepare(vault.clone(), &plan.id)
            .await
            .is_err());
        assert_eq!(
            load_harness_cycle(&vault, &plan.id).unwrap().1.stage,
            HarnessStage::RecoveryRequired
        );
        assert_eq!(
            artifact_digest(&staged.destination).unwrap(),
            plan.baseline_binary_digest
        );
    }

    #[tokio::test]
    #[ignore = "runs a fixed formatting gate in a disposable macOS sandbox"]
    async fn hrs253_source_build_failure_never_admits_a_candidate_or_replays_gate() {
        let temp = tempfile::tempdir().unwrap();
        let (vault, plan, repository) = fixture(temp.path(), true).await;
        assert!(build_and_admit(vault.clone(), &plan.id).await.is_err());
        let (_, state) = load_harness_cycle(&vault, &plan.id).unwrap();
        assert_eq!(state.stage, HarnessStage::Rejected);
        assert!(state.candidate_digest.is_none());
        assert!(!state.engineering_checks_passed);
        let before = report(&vault, &plan.id).unwrap();
        assert!(build_and_admit(vault.clone(), &plan.id).await.is_err());
        assert_eq!(report(&vault, &plan.id).unwrap(), before);
        assert_eq!(
            crate::harness_engineering::source_snapshot(&repository)
                .await
                .unwrap(),
            plan.baseline
        );
    }
}
