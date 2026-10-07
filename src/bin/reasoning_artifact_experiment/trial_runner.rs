//! One-slot RND-097 controller orchestration.
//!
//! This module owns no provider credential. Provider and tool calls are
//! injected one-call transports; the production wrapper uses the isolated
//! framed workers while tests use deterministic local receipts.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::arm::{
    bind_and_persist_planner_handoff, load_fixture_inputs, prepare_executor_phase,
    prepare_planner_phase, prepare_recovery_phase, validate_executor_completion,
};
use crate::controller::PhaseRunResultV1;
use crate::fixtures::FixtureCandidateReceiptV1;
use crate::pilot::{
    inspect_progress, load_official_price_card, load_preregistration,
    open_or_materialize_trial_workspace, open_trial_journal, PilotProgressReportV1,
    PilotTrialSlotV1,
};
use crate::reasoning_experiment_ledger::{AttoUsd, ExperimentLedger};
use crate::reasoning_experiment_protocol::{
    ExperimentArm, ExperimentModelRouteV1, ExperimentPhase, ExperimentUsageV1,
    ProviderCallReceiptV1, ProviderCallSpecV1, ToolCallReceiptV1, ToolCallSpecV1,
};
use crate::trial_state::TrialPhaseV1;

const TRIAL_RESULT_SCHEMA_V1: &str = "ca.rnd097.trial_result.v1";
const TRIAL_RESULT_DIRECTORY: &str = "trial-results-v1";
const TARGET_DIRECTORY: &str = "cargo-targets-v1";
const MAX_TRIAL_RESULT_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PhaseSummaryV1 {
    pub phase: ExperimentPhase,
    pub route: ExperimentModelRouteV1,
    pub semantic_requests: u16,
    pub physical_posts: u16,
    pub tool_calls: u16,
    pub usage: ExperimentUsageV1,
    pub elapsed_millis: u64,
    pub completion_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TrialResultV1 {
    pub schema_version: String,
    pub preregistration_sha256: String,
    pub trial_id: String,
    pub slot: PilotTrialSlotV1,
    pub planner: Option<PhaseSummaryV1>,
    pub executor: PhaseSummaryV1,
    pub recovery: Option<PhaseSummaryV1>,
    pub initial_verifier: FixtureCandidateReceiptV1,
    pub verifier: FixtureCandidateReceiptV1,
    pub recovery_artifact_sha256: String,
    pub final_manifest_sha256: String,
    pub verified_success: bool,
    pub false_acceptance: bool,
    pub completed_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ArmMetricsV1 {
    pub arm: ExperimentArm,
    pub trials: u16,
    pub verified_successes: u16,
    pub false_acceptances: u16,
    pub recovery_trials: u16,
    pub successful_recoveries: u16,
    pub planner_requests: u64,
    pub executor_requests: u64,
    pub tool_calls: u64,
    pub latency_millis: u64,
    pub usage: ExperimentUsageV1,
    pub actual_cost_usd: String,
    pub dollars_per_verified_success: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PairedBootstrapIntervalV1 {
    pub arm: ExperimentArm,
    pub comparator: ExperimentArm,
    pub pairs: u16,
    pub success_difference_low: String,
    pub success_difference_high: String,
    pub cost_difference_usd_low: String,
    pub cost_difference_usd_high: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PilotRunReportV1 {
    pub schema_version: String,
    pub progress: PilotProgressReportV1,
    pub settled_actual_usd: String,
    pub unknown_reserved_usd: String,
    pub outstanding_reserved_usd: String,
    pub arms: Vec<ArmMetricsV1>,
    pub paired_bootstrap_intervals: Vec<PairedBootstrapIntervalV1>,
    pub interpretation: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalibrationStateV1 {
    Incomplete,
    Passed,
    Failed,
}

pub fn calibration_state(experiment_dir: &Path) -> Result<CalibrationStateV1> {
    let progress = inspect_progress(experiment_dir)?;
    let results = load_trial_results(experiment_dir, &progress.preregistration_sha256)?;
    let calibration = results
        .iter()
        .filter(|result| result.slot.included_calibration)
        .collect::<Vec<_>>();
    if calibration.len() < 6 {
        return Ok(CalibrationStateV1::Incomplete);
    }
    if calibration.len() != 6
        || calibration
            .iter()
            .any(|result| !result.verified_success || result.false_acceptance)
    {
        return Ok(CalibrationStateV1::Failed);
    }
    let ledger = ExperimentLedger::open(&experiment_dir.canonicalize()?.join("ledger"))
        .map_err(|error| anyhow!("cannot inspect calibration ledger: {error}"))?;
    let snapshot = ledger
        .snapshot()
        .map_err(|error| anyhow!("cannot inspect calibration spend: {error}"))?;
    if snapshot.unknown_reserved_atto_usd != AttoUsd::ZERO
        || snapshot.outstanding_reserved_atto_usd != AttoUsd::ZERO
    {
        return Ok(CalibrationStateV1::Failed);
    }
    Ok(CalibrationStateV1::Passed)
}

pub fn build_report(experiment_dir: &Path) -> Result<PilotRunReportV1> {
    let progress = inspect_progress(experiment_dir)?;
    let results = load_trial_results(experiment_dir, &progress.preregistration_sha256)?;
    let price = if results.is_empty() {
        None
    } else {
        Some(load_official_price_card(experiment_dir)?)
    };
    let ledger_root = experiment_dir.canonicalize()?.join("ledger");
    let ledger = match fs::symlink_metadata(&ledger_root) {
        Ok(_) => Some(
            ExperimentLedger::open(&ledger_root)
                .map_err(|error| anyhow!("cannot read experiment ledger: {error}"))?,
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    let mut arms = Vec::with_capacity(ExperimentArm::ALL.len());
    for arm in ExperimentArm::ALL {
        let selected = results
            .iter()
            .filter(|result| result.slot.arm == arm)
            .collect::<Vec<_>>();
        let mut usage = empty_usage();
        let mut cost = AttoUsd::ZERO;
        let mut planner_requests = 0_u64;
        let mut executor_requests = 0_u64;
        let mut tool_calls = 0_u64;
        let mut latency_millis = 0_u64;
        for result in &selected {
            if let Some(planner) = &result.planner {
                add_aggregate_usage(&mut usage, planner.usage)?;
                let price = price
                    .as_ref()
                    .ok_or_else(|| anyhow!("trial results require their frozen price card"))?;
                cost = cost
                    .checked_add(
                        price
                            .price_card
                            .exact_cost_for_usage(&planner.route, planner.usage)
                            .map_err(|error| anyhow!("cannot price planner usage: {error}"))?,
                    )
                    .map_err(|error| anyhow!("planner cost overflow: {error}"))?;
                planner_requests =
                    planner_requests.saturating_add(u64::from(planner.physical_posts));
                tool_calls = tool_calls.saturating_add(u64::from(planner.tool_calls));
                latency_millis = latency_millis.saturating_add(planner.elapsed_millis);
            }
            add_aggregate_usage(&mut usage, result.executor.usage)?;
            let price = price
                .as_ref()
                .ok_or_else(|| anyhow!("trial results require their frozen price card"))?;
            cost = cost
                .checked_add(
                    price
                        .price_card
                        .exact_cost_for_usage(&result.executor.route, result.executor.usage)
                        .map_err(|error| anyhow!("cannot price executor usage: {error}"))?,
                )
                .map_err(|error| anyhow!("executor cost overflow: {error}"))?;
            executor_requests =
                executor_requests.saturating_add(u64::from(result.executor.physical_posts));
            tool_calls = tool_calls.saturating_add(u64::from(result.executor.tool_calls));
            latency_millis = latency_millis.saturating_add(result.executor.elapsed_millis);
            if let Some(recovery) = &result.recovery {
                add_aggregate_usage(&mut usage, recovery.usage)?;
                cost = cost
                    .checked_add(
                        price
                            .price_card
                            .exact_cost_for_usage(&recovery.route, recovery.usage)
                            .map_err(|error| anyhow!("cannot price recovery usage: {error}"))?,
                    )
                    .map_err(|error| anyhow!("recovery cost overflow: {error}"))?;
                executor_requests =
                    executor_requests.saturating_add(u64::from(recovery.physical_posts));
                tool_calls = tool_calls.saturating_add(u64::from(recovery.tool_calls));
                latency_millis = latency_millis.saturating_add(recovery.elapsed_millis);
            }
        }
        let successes = selected
            .iter()
            .filter(|result| result.verified_success)
            .count() as u16;
        arms.push(ArmMetricsV1 {
            arm,
            trials: selected.len() as u16,
            verified_successes: successes,
            false_acceptances: selected
                .iter()
                .filter(|result| result.false_acceptance)
                .count() as u16,
            recovery_trials: selected
                .iter()
                .filter(|result| result.recovery.is_some())
                .count() as u16,
            successful_recoveries: selected
                .iter()
                .filter(|result| {
                    result.recovery.is_some()
                        && result.verified_success
                        && !(result.initial_verifier.public_passed
                            && result.initial_verifier.hidden_passed)
                })
                .count() as u16,
            planner_requests,
            executor_requests,
            tool_calls,
            latency_millis,
            usage,
            actual_cost_usd: cost.to_string(),
            dollars_per_verified_success: (successes > 0)
                .then(|| format_ratio_usd(cost, u128::from(successes))),
        });
    }
    let (settled, unknown, outstanding) = match ledger {
        Some(ledger) => {
            let snapshot = ledger
                .snapshot()
                .map_err(|error| anyhow!("cannot read ledger snapshot: {error}"))?;
            (
                snapshot.settled_actual_atto_usd.to_string(),
                snapshot.unknown_reserved_atto_usd.to_string(),
                snapshot.outstanding_reserved_atto_usd.to_string(),
            )
        }
        None => (
            AttoUsd::ZERO.to_string(),
            AttoUsd::ZERO.to_string(),
            AttoUsd::ZERO.to_string(),
        ),
    };
    Ok(PilotRunReportV1 {
        schema_version: "ca.rnd097.pilot_report.v1".into(),
        progress,
        settled_actual_usd: settled,
        unknown_reserved_usd: unknown,
        outstanding_reserved_usd: outstanding,
        arms,
        paired_bootstrap_intervals: paired_bootstrap_intervals(&results, price.as_ref())?,
        interpretation: "Underpowered descriptive pilot; never sufficient to enable production routing automatically.".into(),
    })
}

fn paired_bootstrap_intervals(
    results: &[TrialResultV1],
    price: Option<&crate::pilot::OfficialPriceCardReceiptV1>,
) -> Result<Vec<PairedBootstrapIntervalV1>> {
    let Some(price) = price else {
        return Ok(Vec::new());
    };
    let baseline = results
        .iter()
        .filter(|result| result.slot.arm == ExperimentArm::S0)
        .map(|result| {
            (
                (result.slot.fixture_id.as_str(), result.slot.repetition),
                result,
            )
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut intervals = Vec::new();
    for arm in ExperimentArm::ALL
        .into_iter()
        .filter(|arm| *arm != ExperimentArm::S0)
    {
        let mut observations = Vec::new();
        for result in results.iter().filter(|result| result.slot.arm == arm) {
            let Some(comparator) =
                baseline.get(&(result.slot.fixture_id.as_str(), result.slot.repetition))
            else {
                continue;
            };
            let success_delta =
                i8::from(result.verified_success) - i8::from(comparator.verified_success);
            let cost_delta = i128::try_from(trial_cost_atto(result, &price.price_card)?)?
                - i128::try_from(trial_cost_atto(comparator, &price.price_card)?)?;
            observations.push((success_delta, cost_delta));
        }
        if observations.is_empty() {
            continue;
        }
        let mut success_samples = Vec::with_capacity(10_000);
        let mut cost_samples = Vec::with_capacity(10_000);
        let mut state = 0x9e37_79b9_7f4a_7c15_u64 ^ u64::from(arm as u8);
        for _ in 0..10_000 {
            let mut success_sum = 0_i64;
            let mut cost_sum = 0_i128;
            for _ in 0..observations.len() {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                let observation = observations[state as usize % observations.len()];
                success_sum += i64::from(observation.0);
                cost_sum = cost_sum
                    .checked_add(observation.1)
                    .ok_or_else(|| anyhow!("paired bootstrap cost overflow"))?;
            }
            success_samples.push(success_sum);
            cost_samples.push(cost_sum);
        }
        success_samples.sort_unstable();
        cost_samples.sort_unstable();
        let denominator = observations.len() as i128;
        intervals.push(PairedBootstrapIntervalV1 {
            arm,
            comparator: ExperimentArm::S0,
            pairs: observations.len() as u16,
            success_difference_low: format_signed_ratio(
                success_samples[249] as i128,
                denominator,
                6,
            ),
            success_difference_high: format_signed_ratio(
                success_samples[9_749] as i128,
                denominator,
                6,
            ),
            cost_difference_usd_low: format_signed_atto_ratio(cost_samples[249], denominator),
            cost_difference_usd_high: format_signed_atto_ratio(cost_samples[9_749], denominator),
        });
    }
    Ok(intervals)
}

fn trial_cost_atto(
    result: &TrialResultV1,
    price: &crate::reasoning_experiment_ledger::FrozenPriceCardV1,
) -> Result<u128> {
    let mut cost = price
        .exact_cost_for_usage(&result.executor.route, result.executor.usage)
        .map_err(|error| anyhow!("cannot price paired executor usage: {error}"))?;
    if let Some(planner) = &result.planner {
        cost = cost
            .checked_add(
                price
                    .exact_cost_for_usage(&planner.route, planner.usage)
                    .map_err(|error| anyhow!("cannot price paired planner usage: {error}"))?,
            )
            .map_err(|error| anyhow!("paired planner cost overflow: {error}"))?;
    }
    if let Some(recovery) = &result.recovery {
        cost = cost
            .checked_add(
                price
                    .exact_cost_for_usage(&recovery.route, recovery.usage)
                    .map_err(|error| anyhow!("cannot price paired recovery usage: {error}"))?,
            )
            .map_err(|error| anyhow!("paired recovery cost overflow: {error}"))?;
    }
    Ok(cost.as_atto())
}

fn format_signed_ratio(numerator: i128, denominator: i128, decimals: u32) -> String {
    let negative = numerator < 0;
    let numerator = numerator.unsigned_abs();
    let denominator = denominator as u128;
    let scale = 10_u128.pow(decimals);
    let scaled = numerator.saturating_mul(scale) / denominator;
    let sign = if negative { "-" } else { "" };
    format!(
        "{sign}{}.{:0width$}",
        scaled / scale,
        scaled % scale,
        width = decimals as usize
    )
}

fn format_signed_atto_ratio(numerator: i128, denominator: i128) -> String {
    let negative = numerator < 0;
    let value = numerator.unsigned_abs() / denominator as u128;
    let sign = if negative { "-" } else { "" };
    format!(
        "{sign}{}.{:018}",
        value / 1_000_000_000_000_000_000,
        value % 1_000_000_000_000_000_000
    )
}

/// Production one-slot entry point. It creates or reopens the price-bound
/// ledger, then delegates every physical request to the credential-only broker
/// and every local action to the clean-environment tool worker.
pub fn execute_trial(experiment_dir: &Path, slot_index: u16) -> Result<TrialResultV1> {
    let preregistration = load_preregistration(experiment_dir)?;
    crate::controller_ipc::invoke_provider_preflight(
        &preregistration.worker_path,
        experiment_dir,
        Duration::from_secs(10),
    )?;
    let _ledger = ensure_ledger(experiment_dir, &preregistration)?;
    let worker = preregistration.worker_path.clone();
    execute_trial_with(
        experiment_dir,
        slot_index,
        |call| {
            crate::controller_ipc::invoke_provider_worker(
                &worker,
                experiment_dir,
                call,
                Duration::from_secs(610),
            )
        },
        |call| {
            crate::controller_ipc::invoke_tool_worker(
                &worker,
                experiment_dir,
                call,
                Duration::from_millis(call.timeout_millis.saturating_add(5_000)),
            )
        },
        crate::fixtures::verify_trial_workspace,
    )
}

pub fn execute_trial_with<P, T, V>(
    experiment_dir: &Path,
    slot_index: u16,
    mut invoke_provider: P,
    mut invoke_tool: T,
    mut verify_workspace: V,
) -> Result<TrialResultV1>
where
    P: FnMut(&ProviderCallSpecV1) -> Result<ProviderCallReceiptV1>,
    T: FnMut(&ToolCallSpecV1) -> Result<ToolCallReceiptV1>,
    V: FnMut(&Path, &str, &Path) -> Result<FixtureCandidateReceiptV1>,
{
    let preregistration = load_preregistration(experiment_dir)?;
    let slot = preregistration
        .slots
        .get(slot_index as usize)
        .cloned()
        .ok_or_else(|| anyhow!("trial slot is outside preregistration"))?;
    let lifecycle = open_or_materialize_trial_workspace(experiment_dir, slot_index)?;
    let target_root = create_owner_subdirectory(experiment_dir, TARGET_DIRECTORY)?;
    let target = tempfile::Builder::new()
        .prefix(&format!("slot-{slot_index:03}-"))
        .tempdir_in(&target_root)
        .context("cannot create external trial Cargo target")?;
    #[cfg(unix)]
    fs::set_permissions(target.path(), fs::Permissions::from_mode(0o700))?;
    let target_path = target.path().canonicalize()?;
    let context = lifecycle.trial_context(&target_path)?;
    let fixture = load_fixture_inputs(&context.workspace_root, &context.fixture_id)?;

    let planner = if matches!(slot.arm, ExperimentArm::S0 | ExperimentArm::C0) {
        None
    } else {
        let prepared = prepare_planner_phase(&context, &fixture)?;
        let route = prepared.worker_spec.route.clone();
        let phase = lifecycle.run_prepared_phase_with(
            &prepared,
            |call| invoke_provider(call),
            |call| invoke_tool(call),
        )?;
        let handoff = bind_and_persist_planner_handoff(&context, &fixture, phase.content())?;
        Some((
            summarize_phase(ExperimentPhase::Planner, route, &phase),
            handoff,
        ))
    };

    let prepared = prepare_executor_phase(
        &context,
        &fixture,
        planner.as_ref().map(|(_, handoff)| handoff),
    )?;
    let executor_route = prepared.worker_spec.route.clone();
    let executor = lifecycle.run_prepared_phase_with(
        &prepared,
        |call| invoke_provider(call),
        |call| invoke_tool(call),
    )?;
    let _completion = validate_executor_completion(executor.content())?;
    let initial_verifier = verify_workspace(
        &preregistration.repository_root,
        &slot.fixture_id,
        lifecycle.workspace_path(),
    )?;
    if initial_verifier.fixture_id != slot.fixture_id || initial_verifier.kind != "trial" {
        bail!("trial verifier returned a mismatched identity");
    }
    let initial_success = initial_verifier.public_passed && initial_verifier.hidden_passed;
    let (recovery, verifier) = if slot.arm == ExperimentArm::ScG && !initial_success {
        let handoff = planner
            .as_ref()
            .map(|(_, handoff)| handoff)
            .ok_or_else(|| anyhow!("SC-G recovery lacks its planner handoff"))?;
        let failure_code = initial_verifier
            .hidden_failure_code
            .as_deref()
            .unwrap_or("verification_failed");
        let prepared = prepare_recovery_phase(&context, &fixture, handoff, failure_code)?;
        let route = prepared.worker_spec.route.clone();
        let phase = lifecycle.run_prepared_phase_with(
            &prepared,
            |call| invoke_provider(call),
            |call| invoke_tool(call),
        )?;
        let _completion = validate_executor_completion(phase.content())?;
        let verifier = verify_workspace(
            &preregistration.repository_root,
            &slot.fixture_id,
            lifecycle.workspace_path(),
        )?;
        (
            Some(summarize_phase(ExperimentPhase::Recovery, route, &phase)),
            verifier,
        )
    } else {
        (None, initial_verifier.clone())
    };
    if verifier.fixture_id != slot.fixture_id || verifier.kind != "trial" {
        bail!("final trial verifier returned a mismatched identity");
    }
    let verified_success = verifier.public_passed && verifier.hidden_passed;
    let false_acceptance = initial_verifier.public_passed && !initial_verifier.hidden_passed;
    let proof = lifecycle.export_and_prove()?;
    let recovery_receipt = proof.receipt().clone();
    let result = TrialResultV1 {
        schema_version: TRIAL_RESULT_SCHEMA_V1.into(),
        preregistration_sha256: context.preregistration_sha256,
        trial_id: context.trial_id.clone(),
        slot: slot.clone(),
        planner: planner.map(|(summary, _)| summary),
        executor: summarize_phase(ExperimentPhase::Executor, executor_route, &executor),
        recovery,
        initial_verifier,
        verifier,
        recovery_artifact_sha256: recovery_receipt.recovery_artifact_sha256,
        final_manifest_sha256: recovery_receipt.final_manifest_sha256,
        verified_success,
        false_acceptance,
        completed_at: Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
    };
    persist_trial_result(experiment_dir, &result)?;
    let journal = open_trial_journal(experiment_dir)?;
    journal.append(
        &context.trial_id,
        slot_index,
        TrialPhaseV1::VerifierCompleted,
        None,
        Some(&result.recovery_artifact_sha256),
        (!verified_success).then_some("verifier_failed"),
    )?;
    proof.cleanup_after_persisting()?;
    journal.append(
        &context.trial_id,
        slot_index,
        if verified_success {
            TrialPhaseV1::Settled
        } else {
            TrialPhaseV1::Failed
        },
        None,
        Some(&result.recovery_artifact_sha256),
        (!verified_success).then_some("verifier_failed"),
    )?;
    drop(target);
    Ok(result)
}

fn summarize_phase(
    phase: ExperimentPhase,
    route: ExperimentModelRouteV1,
    result: &PhaseRunResultV1,
) -> PhaseSummaryV1 {
    PhaseSummaryV1 {
        phase,
        route,
        semantic_requests: result.semantic_requests,
        physical_posts: result.physical_posts,
        tool_calls: result.tool_calls,
        usage: result.usage,
        elapsed_millis: result.elapsed_millis,
        completion_sha256: sha256_hex(result.content().as_bytes()),
    }
}

fn ensure_ledger(
    experiment_dir: &Path,
    preregistration: &crate::pilot::PilotPreregistrationV1,
) -> Result<ExperimentLedger> {
    let price = load_official_price_card(experiment_dir)?;
    let ledger_root = experiment_dir.canonicalize()?.join("ledger");
    let ledger = match fs::symlink_metadata(&ledger_root) {
        Ok(_) => ExperimentLedger::open(&ledger_root)
            .map_err(|error| anyhow!("cannot reopen experiment ledger: {error}"))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => ExperimentLedger::create(
            &ledger_root,
            &preregistration.experiment_id.to_string(),
            &price.preregistration_sha256,
            AttoUsd::DEFAULT_CAP,
            price.price_card.clone(),
        )
        .map_err(|error| anyhow!("cannot create experiment ledger: {error}"))?,
        Err(error) => return Err(error.into()),
    };
    if ledger.header().experiment_id != preregistration.experiment_id.to_string()
        || ledger.header().preregistration_sha256 != price.preregistration_sha256
        || ledger.header().cap_atto_usd != AttoUsd::DEFAULT_CAP
        || ledger.header().price_card_sha256 != price.price_card_sha256
    {
        bail!("experiment ledger differs from the immutable paid-run contract");
    }
    Ok(ledger)
}

fn persist_trial_result(experiment_dir: &Path, result: &TrialResultV1) -> Result<PathBuf> {
    validate_trial_result(result)?;
    let root = create_owner_subdirectory(experiment_dir, TRIAL_RESULT_DIRECTORY)?;
    let path = root.join(format!("{:03}.json", result.slot.slot_index));
    let bytes = serde_json::to_vec(result)?;
    if bytes.len() > MAX_TRIAL_RESULT_BYTES {
        bail!("trial result exceeds its byte bound");
    }
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options
        .mode(0o600)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
    let mut file = options.open(&path)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    File::open(&root)?.sync_all()?;
    Ok(path)
}

fn validate_trial_result(result: &TrialResultV1) -> Result<()> {
    if result.schema_version != TRIAL_RESULT_SCHEMA_V1
        || result.trial_id.is_empty()
        || result.slot.slot_index >= 72
        || result.initial_verifier.fixture_id != result.slot.fixture_id
        || result.initial_verifier.kind != "trial"
        || result.verifier.fixture_id != result.slot.fixture_id
        || result.verifier.kind != "trial"
        || result.verified_success
            != (result.verifier.public_passed && result.verifier.hidden_passed)
        || result.false_acceptance
            != (result.initial_verifier.public_passed && !result.initial_verifier.hidden_passed)
        || result.recovery.is_some()
            != (result.slot.arm == ExperimentArm::ScG
                && !(result.initial_verifier.public_passed
                    && result.initial_verifier.hidden_passed))
    {
        bail!("trial result is invalid");
    }
    for digest in [
        &result.preregistration_sha256,
        &result.executor.completion_sha256,
        &result.recovery_artifact_sha256,
        &result.final_manifest_sha256,
    ] {
        if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            bail!("trial result contains an invalid digest");
        }
    }
    Ok(())
}

fn load_trial_results(
    experiment_dir: &Path,
    preregistration_sha256: &str,
) -> Result<Vec<TrialResultV1>> {
    let root = experiment_dir.canonicalize()?.join(TRIAL_RESULT_DIRECTORY);
    let metadata = match fs::symlink_metadata(&root) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("trial result directory is invalid");
    }
    #[cfg(unix)]
    if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 {
        bail!("trial result directory is not owner-only");
    }
    let mut entries = fs::read_dir(&root)?.collect::<std::io::Result<Vec<_>>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    if entries.len() > 72 {
        bail!("trial result count exceeds the preregistered slot count");
    }
    let mut results = Vec::with_capacity(entries.len());
    for entry in entries {
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || metadata.len() > MAX_TRIAL_RESULT_BYTES as u64
        {
            bail!("trial result file is invalid");
        }
        #[cfg(unix)]
        if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 {
            bail!("trial result file is not owner-only");
        }
        let bytes = fs::read(&path)?;
        let result: TrialResultV1 =
            serde_json::from_slice(&bytes).context("trial result is not strict JSON")?;
        validate_trial_result(&result)?;
        if result.preregistration_sha256 != preregistration_sha256
            || entry.file_name()
                != std::ffi::OsString::from(format!("{:03}.json", result.slot.slot_index))
        {
            bail!("trial result is not bound to this preregistration or slot");
        }
        results.push(result);
    }
    Ok(results)
}

fn empty_usage() -> ExperimentUsageV1 {
    ExperimentUsageV1 {
        regular_input_tokens: 0,
        cached_input_tokens: 0,
        cache_write_tokens: 0,
        regular_output_tokens: 0,
        reasoning_output_tokens: 0,
    }
}

fn add_aggregate_usage(total: &mut ExperimentUsageV1, value: ExperimentUsageV1) -> Result<()> {
    total.regular_input_tokens = total
        .regular_input_tokens
        .checked_add(value.regular_input_tokens)
        .ok_or_else(|| anyhow!("aggregate usage overflow"))?;
    total.cached_input_tokens = total
        .cached_input_tokens
        .checked_add(value.cached_input_tokens)
        .ok_or_else(|| anyhow!("aggregate usage overflow"))?;
    total.cache_write_tokens = total
        .cache_write_tokens
        .checked_add(value.cache_write_tokens)
        .ok_or_else(|| anyhow!("aggregate usage overflow"))?;
    total.regular_output_tokens = total
        .regular_output_tokens
        .checked_add(value.regular_output_tokens)
        .ok_or_else(|| anyhow!("aggregate usage overflow"))?;
    total.reasoning_output_tokens = total
        .reasoning_output_tokens
        .checked_add(value.reasoning_output_tokens)
        .ok_or_else(|| anyhow!("aggregate usage overflow"))?;
    Ok(())
}

fn format_ratio_usd(cost: AttoUsd, denominator: u128) -> String {
    let quotient = cost.as_atto() / denominator;
    let whole = quotient / 1_000_000_000_000_000_000;
    let fraction = quotient % 1_000_000_000_000_000_000;
    format!("{whole}.{fraction:018}")
}

fn create_owner_subdirectory(parent: &Path, name: &str) -> Result<PathBuf> {
    let parent = parent.canonicalize()?;
    let metadata = fs::symlink_metadata(&parent)?;
    #[cfg(unix)]
    if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 {
        bail!("trial result parent is not owner-only");
    }
    let path = parent.join(name);
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            bail!("trial result directory is invalid")
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(&path)?;
            #[cfg(unix)]
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
        }
        Err(error) => return Err(error.into()),
    }
    let path = path.canonicalize()?;
    if !path.starts_with(&parent) {
        bail!("trial result directory escaped its parent");
    }
    Ok(path)
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::{FixtureCheckReceiptV1, FIXTURE_CHECK_SCHEMA_VERSION, FIXTURE_IDS};
    use crate::pilot::{persist_fixture_receipt, preregister};
    use crate::reasoning_experiment_protocol::{
        ToolCallOutcomeV1, PROVIDER_CALL_RECEIPT_SCHEMA_V1, TOOL_CALL_RECEIPT_SCHEMA_V1,
    };
    use serde_json::json;

    fn successful_fixture_check() -> FixtureCheckReceiptV1 {
        let candidates = FIXTURE_IDS
            .into_iter()
            .flat_map(|fixture_id| {
                [
                    FixtureCandidateReceiptV1 {
                        fixture_id: fixture_id.into(),
                        kind: "baseline".into(),
                        public_passed: true,
                        hidden_passed: false,
                        hidden_failure_code: Some(
                            crate::fixtures::expected_failure_code(fixture_id).into(),
                        ),
                    },
                    FixtureCandidateReceiptV1 {
                        fixture_id: fixture_id.into(),
                        kind: "golden".into(),
                        public_passed: true,
                        hidden_passed: true,
                        hidden_failure_code: None,
                    },
                ]
            })
            .collect();
        FixtureCheckReceiptV1 {
            schema_version: FIXTURE_CHECK_SCHEMA_VERSION.into(),
            passed: true,
            network_isolation: "sandbox-exec-deny-network".into(),
            candidates,
        }
    }

    fn private_experiment() -> (tempfile::TempDir, PathBuf, u16) {
        let outer = tempfile::tempdir().unwrap();
        let parent = outer.path().join("experiments");
        fs::create_dir(&parent).unwrap();
        #[cfg(unix)]
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o700)).unwrap();
        let experiment = parent.join("trial-runner");
        let repository = Path::new(env!("CARGO_MANIFEST_DIR"));
        preregister(repository, &experiment, &std::env::current_exe().unwrap()).unwrap();
        persist_fixture_receipt(&experiment, repository, &successful_fixture_check()).unwrap();
        let preregistration = load_preregistration(&experiment).unwrap();
        let slot = preregistration
            .slots
            .iter()
            .find(|slot| slot.arm == ExperimentArm::S0)
            .unwrap()
            .slot_index;
        (outer, experiment, slot)
    }

    fn completed_provider_receipt(
        call: &ProviderCallSpecV1,
        controller_response_json: String,
    ) -> ProviderCallReceiptV1 {
        ProviderCallReceiptV1 {
            schema_version: PROVIDER_CALL_RECEIPT_SCHEMA_V1.into(),
            call_id: call.call_id.clone(),
            channel_id: call.channel_id.clone(),
            nonce: call.nonce.clone(),
            worker_spec_sha256: call.worker_spec_sha256.clone(),
            request_sha256: call.request_sha256.clone(),
            reservation_id: format!("reservation-{}", call.call_id),
            x_client_request_id_sha256: sha256_hex(call.x_client_request_id.as_bytes()),
            x_request_id_sha256: Some(sha256_hex(b"request-id")),
            returned_model: Some(call.route.model.clone()),
            returned_service_tier: Some("default".into()),
            usage: Some(ExperimentUsageV1 {
                regular_input_tokens: 10,
                cached_input_tokens: 0,
                cache_write_tokens: 0,
                regular_output_tokens: 5,
                reasoning_output_tokens: 1,
            }),
            elapsed_millis: 1,
            outcome: crate::reasoning_experiment_protocol::ProviderCallOutcomeV1::Completed {
                controller_response_sha256: sha256_hex(controller_response_json.as_bytes()),
                controller_response_json,
            },
        }
    }

    fn terminal_response(content: String) -> String {
        serde_json::to_string(&json!({
            "content": content,
            "reasoning": "private phase reasoning",
            "response_items": [{
                "output_index": 0,
                "item": {"type":"message","id":"msg-1","role":"assistant","content":[{"type":"output_text","text":content}]}
            }],
            "tool_calls": [],
            "finish_reason": "stop"
        }))
        .unwrap()
    }

    #[test]
    fn direct_trial_runs_one_tool_loop_verifies_exports_and_cleans() {
        let (_outer, experiment, slot) = private_experiment();
        let golden = fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/reasoning_pilot_golden/unicode_parser_boundary/src/lib.rs"),
        )
        .unwrap();
        let mut request_index = 0_u8;
        let result = execute_trial_with(
            &experiment,
            slot,
            |call| {
                let response = if request_index == 0 {
                    request_index += 1;
                    let arguments = serde_json::to_string(&json!({
                        "path": "src/lib.rs",
                        "content": golden.clone(),
                    }))
                    .unwrap();
                    serde_json::to_string(&json!({
                        "content": "",
                        "reasoning": "private reasoning must remain in memory",
                        "response_items": [{
                            "output_index": 0,
                            "item": {"type":"function_call","id":"fc-1","call_id":"call-1","name":"write_file","arguments":arguments}
                        }],
                        "tool_calls": [{"index":0,"id":"call-1","name":"write_file","arguments":arguments}],
                        "finish_reason": "tool_calls"
                    }))
                    .unwrap()
                } else {
                    let completion = json!({
                        "schema_version":"ca.rnd097.executor_completion.v1",
                        "summary":"fixed fixture",
                        "required_verifier_attempted":true
                    })
                    .to_string();
                    serde_json::to_string(&json!({
                        "content": completion,
                        "reasoning": "private final reasoning",
                        "response_items": [{
                            "output_index": 0,
                            "item": {"type":"message","id":"msg-1","role":"assistant","content":[{"type":"output_text","text":completion}]}
                        }],
                        "tool_calls": [],
                        "finish_reason": "stop"
                    }))
                    .unwrap()
                };
                let receipt = completed_provider_receipt(call, response);
                receipt.validate_against(call).unwrap();
                Ok(receipt)
            },
            |call| {
                let value: serde_json::Value = serde_json::from_str(&call.arguments_json)?;
                let relative = value["path"]
                    .as_str()
                    .ok_or_else(|| anyhow!("missing path"))?;
                let content = value["content"]
                    .as_str()
                    .ok_or_else(|| anyhow!("missing content"))?;
                fs::write(call.workspace_root.join(relative), content)?;
                let result_json = json!({"path":relative,"written":true}).to_string();
                let receipt = ToolCallReceiptV1 {
                    schema_version: TOOL_CALL_RECEIPT_SCHEMA_V1.into(),
                    call_id: call.call_id.clone(),
                    channel_id: call.channel_id.clone(),
                    nonce: call.nonce.clone(),
                    worker_spec_sha256: call.worker_spec_sha256.clone(),
                    arguments_sha256: call.arguments_sha256.clone(),
                    elapsed_millis: 1,
                    outcome: ToolCallOutcomeV1::Completed,
                    result_sha256: Some(sha256_hex(result_json.as_bytes())),
                    result_json: Some(result_json),
                };
                receipt.validate_against(call).unwrap();
                Ok(receipt)
            },
            |_repository, fixture_id, workspace| {
                assert_eq!(
                    fs::read_to_string(workspace.join("src/lib.rs"))?,
                    fs::read_to_string(
                        Path::new(env!("CARGO_MANIFEST_DIR"))
                            .join("tests/reasoning_pilot_golden")
                            .join(fixture_id)
                            .join("src/lib.rs")
                    )?
                );
                Ok(FixtureCandidateReceiptV1 {
                    fixture_id: fixture_id.into(),
                    kind: "trial".into(),
                    public_passed: true,
                    hidden_passed: true,
                    hidden_failure_code: None,
                })
            },
        );
        if let Err(error) = &result {
            let events = open_trial_journal(&experiment).unwrap().events().unwrap();
            panic!("trial failed: {error:#}; events={events:?}");
        }
        let result = result.unwrap();
        assert!(result.verified_success);
        assert!(!result.false_acceptance);
        assert_eq!(result.executor.semantic_requests, 2);
        assert_eq!(result.executor.tool_calls, 1);
        let retained = fs::read_to_string(
            experiment
                .join(TRIAL_RESULT_DIRECTORY)
                .join(format!("{slot:03}.json")),
        )
        .unwrap();
        assert!(!retained.contains("private reasoning"));
        assert!(!retained.contains("fixed fixture"));
        let events = open_trial_journal(&experiment).unwrap().events().unwrap();
        assert_eq!(events.last().unwrap().phase, TrialPhaseV1::Settled);
    }

    #[test]
    fn scg_runs_exactly_one_fresh_strong_recovery_after_deterministic_failure() {
        let (_outer, experiment, _) = private_experiment();
        let preregistration = load_preregistration(&experiment).unwrap();
        let slot = preregistration
            .slots
            .iter()
            .find(|slot| slot.arm == ExperimentArm::ScG)
            .unwrap()
            .slot_index;
        let proposal = json!({
            "schema_version":"ca.rnd097.planner_proposal.v1",
            "objective":"Repair the Unicode boundary.",
            "evidence":[{"path":"src/lib.rs","observation":"The byte slice may split a scalar."}],
            "unresolved_decisions":[],
            "risks":["protocol"],
            "steps":[{
                "id":"edit",
                "instructions":"Move the index to a valid character boundary.",
                "depends_on":[],
                "affected_paths":["src/lib.rs"],
                "verification_commands":[["cargo","test","--quiet"]]
            }]
        })
        .to_string();
        let completion = json!({
            "schema_version":"ca.rnd097.executor_completion.v1",
            "summary":"phase complete",
            "required_verifier_attempted":true
        })
        .to_string();
        let mut phases = Vec::new();
        let mut verifier_calls = 0_u8;
        let result = execute_trial_with(
            &experiment,
            slot,
            |call| {
                phases.push(call.phase);
                let content = if call.phase == ExperimentPhase::Planner {
                    proposal.clone()
                } else {
                    completion.clone()
                };
                Ok(completed_provider_receipt(call, terminal_response(content)))
            },
            |_| Err(anyhow!("no tool call is expected")),
            |_repository, fixture_id, _workspace| {
                verifier_calls += 1;
                Ok(FixtureCandidateReceiptV1 {
                    fixture_id: fixture_id.into(),
                    kind: "trial".into(),
                    public_passed: true,
                    hidden_passed: verifier_calls > 1,
                    hidden_failure_code: (verifier_calls == 1)
                        .then_some("unicode_mid_scalar".into()),
                })
            },
        )
        .unwrap();
        assert_eq!(
            phases,
            [
                ExperimentPhase::Planner,
                ExperimentPhase::Executor,
                ExperimentPhase::Recovery
            ]
        );
        assert_eq!(verifier_calls, 2);
        assert!(result.recovery.is_some());
        assert!(result.verified_success);
        assert!(result.false_acceptance);
    }

    #[test]
    fn empty_report_is_offline_bounded_and_labels_the_pilot() {
        let (_outer, experiment, _) = private_experiment();
        let report = build_report(&experiment).unwrap();
        assert_eq!(report.arms.len(), 6);
        assert!(report.arms.iter().all(|arm| arm.trials == 0));
        assert!(report.paired_bootstrap_intervals.is_empty());
        assert_eq!(report.settled_actual_usd, "0.000000000000000000");
        assert!(report.interpretation.contains("Underpowered"));
    }
}
