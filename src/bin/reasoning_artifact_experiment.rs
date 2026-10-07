//! Offline experiment for a provider-neutral planner/executor handoff artifact.
//!
//! This binary is intentionally isolated behind the
//! `planner-executor-experiment` Cargo feature. It reads explicit JSON inputs,
//! writes results only to stdout/stderr, and performs no network I/O or command
//! execution.

#[path = "reasoning_artifact_experiment/fixtures.rs"]
mod fixtures;

#[path = "reasoning_artifact_experiment/recovery.rs"]
pub mod recovery;

#[path = "reasoning_artifact_experiment/pilot.rs"]
pub mod pilot;

#[path = "reasoning_artifact_experiment/trial_state.rs"]
pub mod trial_state;

#[path = "reasoning_artifact_experiment/receipt_store.rs"]
pub mod receipt_store;

#[path = "reasoning_artifact_experiment/controller_ipc.rs"]
pub mod controller_ipc;

#[path = "reasoning_artifact_experiment/conversation.rs"]
pub mod conversation;

#[path = "reasoning_artifact_experiment/controller.rs"]
pub mod controller;

#[path = "reasoning_artifact_experiment/artifact.rs"]
pub mod artifact;

#[path = "reasoning_artifact_experiment/arm.rs"]
pub mod arm;

#[path = "reasoning_artifact_experiment/trial_runner.rs"]
pub mod trial_runner;

#[path = "../reasoning_experiment_protocol.rs"]
pub mod reasoning_experiment_protocol;

#[path = "../reasoning_experiment_ledger.rs"]
pub mod reasoning_experiment_ledger;

#[path = "../workspace_materialization.rs"]
#[expect(
    dead_code,
    reason = "This standalone experiment uses only part of the shared host materialization API"
)]
pub mod workspace_materialization;

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use std::process::ExitCode;

use anyhow::{anyhow, bail, Context, Result};
use chrono::DateTime;
use clap::{Parser, Subcommand};
use schemars::{schema_for, JsonSchema};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const ARTIFACT_SCHEMA_VERSION: &str = "reasoning_artifact.v1";
const RUNS_SCHEMA_VERSION: &str = "planner_executor_cost_runs.v1";
const SUMMARY_SCHEMA_VERSION: &str = "planner_executor_cost_summary.v1";
const MAX_ARTIFACT_BYTES: u64 = 1_048_576;
const MAX_RUNS_BYTES: u64 = 16 * 1_048_576;
const MAX_CHECKPOINT_FILE_BYTES: u64 = 512 * 1_048_576;
const MAX_CHECKPOINT_TOTAL_BYTES: u64 = 512 * 1_048_576;
const MAX_TEXT_BYTES: usize = 16_384;
const MAX_SHORT_TEXT_BYTES: usize = 1_024;
const MAX_ID_BYTES: usize = 128;
const MAX_PATH_BYTES: usize = 1_024;
const MAX_ITEMS: usize = 256;
const MAX_TRIALS: usize = 10_000;
const MAX_REQUESTS_PER_TRIAL: usize = 1_000;
const MAX_TOOL_FEES_PER_REQUEST: usize = 128;
const MAX_TOKEN_COUNT: u64 = 1_000_000_000_000;
const MAX_TOOL_CALLS: u64 = 1_000_000_000;
const MONEY_SCALE: u32 = 18;
const MONEY_FACTOR: i128 = 1_000_000_000_000_000_000;

#[derive(Debug, Parser)]
#[command(
    name = "ca-reasoning-artifact-experiment",
    about = "Offline reasoning-artifact schema, validation, and cost experiment"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Print the JSON Schema for reasoning_artifact.v1.
    Schema,
    /// Strictly validate an artifact and optionally verify repository dependency path digests.
    Validate {
        artifact: PathBuf,
        #[arg(long, value_name = "ROOT")]
        repo: Option<PathBuf>,
    },
    /// Aggregate strict benchmark-run JSON into success-adjusted cost metrics.
    Cost { runs: PathBuf },
    /// Validate every preregistered baseline and golden benchmark fixture.
    FixtureCheck {
        #[arg(long, value_name = "ROOT")]
        repo: PathBuf,
        /// Persist the receipt only under this immutable preregistration.
        #[arg(long, value_name = "DIR")]
        experiment_dir: Option<PathBuf>,
    },
    /// Freeze an offline, deterministic 72-slot RND-097 protocol.
    Preregister {
        #[arg(long, value_name = "ROOT")]
        repo: PathBuf,
        #[arg(long, value_name = "DIR")]
        experiment_dir: PathBuf,
        #[arg(long, value_name = "CA_BINARY")]
        worker: PathBuf,
    },
    /// Fetch and freeze the current public OpenAI pricing evidence for one
    /// preregistered pilot. This makes no model request and reads no account
    /// configuration or credential.
    FreezePriceCard {
        #[arg(long, value_name = "DIR")]
        experiment_dir: PathBuf,
    },
    /// Read immutable pilot state, receipt bindings, and per-slot progress.
    /// This command never creates a journal, workspace, ledger, or worker.
    Report {
        #[arg(long, value_name = "DIR")]
        experiment_dir: PathBuf,
    },
    /// Continue only untouched pending slots under an existing immutable paid
    /// authorization. Ambiguous or interrupted physical calls are never replayed.
    Resume {
        #[arg(long, value_name = "DIR")]
        experiment_dir: PathBuf,
    },
    /// Check the paid-run gate.  Until an immutable official price card is
    /// present, this prints the blockers and exits before config or provider
    /// access.  It intentionally does not start a benchmark yet.
    Run {
        #[arg(long, value_name = "DIR")]
        experiment_dir: PathBuf,
        #[arg(long, value_name = "USD")]
        authorize_max_usd: String,
        /// Explicitly attest that the canonical OpenAI project has a hard
        /// provider-side budget cap. ca cannot inspect or create that cap.
        #[arg(long, value_name = "USD")]
        attest_provider_project_budget_usd: String,
    },
    /// Internal, one-candidate fixture verifier used by `fixture-check`.
    #[command(hide = true)]
    FixtureCandidate {
        #[arg(long, value_name = "ROOT")]
        repo: PathBuf,
        #[arg(long)]
        fixture_id: String,
        #[arg(long)]
        kind: String,
        #[arg(long, value_name = "WORKSPACE")]
        workspace: PathBuf,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
struct ReasoningArtifactV1 {
    schema_version: String,
    artifact_id: String,
    created_at: String,
    planner: PlannerProvenance,
    task: TaskDefinition,
    repo_checkpoint: RepositoryCheckpoint,
    evidence: Vec<EvidenceItem>,
    claims: Vec<Claim>,
    uncertainties: Vec<Uncertainty>,
    plan: Plan,
    executor_contract: ExecutorContract,
    verifier: VerifierContract,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
struct PlannerProvenance {
    provider: String,
    model: String,
    reasoning_effort: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
struct TaskDefinition {
    objective: String,
    constraints: Vec<RequirementItem>,
    success_criteria: Vec<RequirementItem>,
    non_goals: Vec<RequirementItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
struct RequirementItem {
    id: String,
    text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
struct RepositoryCheckpoint {
    repository_id: String,
    base_revision: String,
    task_input_sha256: String,
    worktree_sha256: String,
    path_digests: Vec<PathDigest>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
struct PathDigest {
    path: String,
    sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum EvidenceKind {
    UserRequirement,
    SourceObservation,
    TestResult,
    CommandOutput,
    PrimaryDocumentation,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
struct EvidenceItem {
    id: String,
    kind: EvidenceKind,
    observation: String,
    checkpoint_paths: Vec<String>,
    source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Confidence {
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
struct Claim {
    id: String,
    statement: String,
    confidence: Confidence,
    evidence_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
struct Uncertainty {
    id: String,
    statement: String,
    impact: String,
    resolution: String,
    evidence_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
struct Plan {
    steps: Vec<PlanStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
struct PlanStep {
    id: String,
    title: String,
    instructions: String,
    depends_on: Vec<String>,
    evidence_ids: Vec<String>,
    affected_paths: Vec<String>,
    created_paths: Vec<String>,
    validation: StepValidation,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
struct StepValidation {
    commands: Vec<String>,
    expected_outcomes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
enum ToolCapability {
    ReadRepository,
    SearchRepository,
    EditRepository,
    RunApprovedCommands,
    InspectVersionControl,
    AskForClarification,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
struct ExecutorContract {
    allowed_tools: Vec<ToolCapability>,
    allowed_paths: Vec<String>,
    forbidden_actions: Vec<String>,
    budgets: ExecutorBudgets,
    stop_conditions: Vec<String>,
    escalation_triggers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
struct ExecutorBudgets {
    max_turns: u32,
    max_requests: u32,
    max_input_tokens: u64,
    max_output_tokens: u64,
    max_tool_calls: u64,
    max_wall_time_seconds: u64,
    max_cost_usd: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
struct VerifierContract {
    gates: Vec<VerifierGate>,
    escalation_policy: EscalationPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
struct VerifierGate {
    id: String,
    criterion_ids: Vec<String>,
    step_ids: Vec<String>,
    command: String,
    expected_outcome: String,
    required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum GateFailureAction {
    FailTask,
    Escalate,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum StaleCheckpointAction {
    FailTask,
    RefreshPlan,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
struct EscalationPolicy {
    max_escalations: u32,
    on_required_gate_failure: GateFailureAction,
    on_stale_checkpoint: StaleCheckpointAction,
    planner_refresh_allowed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BenchmarkRuns {
    schema_version: String,
    currency: String,
    experiment_id: String,
    arm_id: String,
    trials: Vec<BenchmarkTrial>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BenchmarkTrial {
    id: String,
    case_id: String,
    repetition: u32,
    outcome: TrialOutcome,
    requests: Vec<CostedRequest>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum TrialOutcome {
    Success,
    Failure,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
enum Stage {
    Planner,
    Executor,
    Verifier,
    Escalation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CostedRequest {
    stage: Stage,
    provider: String,
    model: String,
    // These five counters are disjoint billable categories. Callers must
    // convert provider-reported totals/subsets before constructing this input.
    regular_input_tokens: u64,
    cached_input_tokens: u64,
    cache_write_tokens: u64,
    regular_output_tokens: u64,
    reasoning_output_tokens: u64,
    pricing: RequestPricing,
    tool_fees: Vec<ToolFee>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestPricing {
    price_card_id: String,
    price_card_effective_at: String,
    billing_tier: String,
    input_per_million_usd: String,
    cached_input_per_million_usd: String,
    cache_write_per_million_usd: String,
    output_per_million_usd: String,
    reasoning_output_per_million_usd: String,
    request_fee_usd: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ToolFee {
    name: String,
    calls: u64,
    fee_per_call_usd: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Money {
    atto_usd: i128,
}

#[derive(Debug, Clone, Copy, Default, Serialize)]
struct TokenTotals {
    regular_input: u64,
    cached_input: u64,
    cache_write: u64,
    regular_output: u64,
    reasoning_output: u64,
}

#[derive(Debug, Clone, Default)]
struct StageAccumulator {
    request_count: u64,
    tokens: TokenTotals,
    tool_calls: u64,
    cost: Money,
}

#[derive(Debug, Serialize)]
struct RatioCost {
    decimal_usd: String,
    exact: bool,
    exact_fraction_usd: String,
}

#[derive(Debug, Serialize)]
struct StageCostSummary {
    request_count: u64,
    tokens: TokenTotals,
    tool_calls: u64,
    total_cost_usd: String,
}

#[derive(Debug, Serialize)]
struct StageBreakdown {
    planner: StageCostSummary,
    executor: StageCostSummary,
    verifier: StageCostSummary,
    escalation: StageCostSummary,
}

#[derive(Debug, Serialize)]
struct CostSummary {
    schema_version: &'static str,
    currency: &'static str,
    experiment_id: String,
    arm_id: String,
    trial_count: u64,
    successes: u64,
    failures: u64,
    request_count: u64,
    tokens: TokenTotals,
    tool_calls: u64,
    total_cost_usd: String,
    mean_cost_usd: RatioCost,
    dollars_per_success: Option<RatioCost>,
    stages: StageBreakdown,
}

#[derive(Debug, Serialize)]
struct ValidationOutput<'a> {
    schema_version: &'static str,
    artifact_id: &'a str,
    artifact_sha256: String,
    valid: bool,
    dependency_paths_verified: bool,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error:#}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Schema => {
            let schema = schema_for!(ReasoningArtifactV1);
            println!("{}", serde_json::to_string_pretty(&schema)?);
        }
        Command::Validate { artifact, repo } => {
            let bytes = read_bounded_file(&artifact, MAX_ARTIFACT_BYTES)?;
            let artifact: ReasoningArtifactV1 =
                serde_json::from_slice(&bytes).with_context(|| {
                    format!("strict JSON validation failed for {}", artifact.display())
                })?;
            validate_artifact(&artifact).map_err(validation_error)?;
            if let Some(root) = repo.as_deref() {
                verify_repository_state(root, &artifact)?;
            }
            let output = ValidationOutput {
                schema_version: "reasoning_artifact_validation.v1",
                artifact_id: &artifact.artifact_id,
                artifact_sha256: artifact_sha256(&artifact)?,
                valid: true,
                dependency_paths_verified: repo.is_some(),
            };
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
        Command::Cost { runs } => {
            let bytes = read_bounded_file(&runs, MAX_RUNS_BYTES)?;
            let runs: BenchmarkRuns = serde_json::from_slice(&bytes)
                .with_context(|| format!("strict JSON validation failed for {}", runs.display()))?;
            let summary = aggregate_cost(&runs).map_err(validation_error)?;
            println!("{}", serde_json::to_string_pretty(&summary)?);
        }
        Command::FixtureCheck {
            repo,
            experiment_dir,
        } => {
            let receipt = fixtures::fixture_check(&repo)?;
            if let Some(experiment_dir) = experiment_dir.as_deref() {
                pilot::persist_fixture_receipt(experiment_dir, &repo, &receipt)?;
            }
            println!("{}", serde_json::to_string_pretty(&receipt)?);
            ensure_fixture_check_passed(receipt.passed)?;
        }
        Command::Preregister {
            repo,
            experiment_dir,
            worker,
        } => {
            let receipt = pilot::preregister(&repo, &experiment_dir, &worker)?;
            println!("{}", serde_json::to_string_pretty(&receipt)?);
        }
        Command::FreezePriceCard { experiment_dir } => {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .context("cannot start public price-evidence runtime")?;
            let receipt = runtime.block_on(pilot::freeze_official_price_card(&experiment_dir))?;
            println!("{}", serde_json::to_string_pretty(&receipt)?);
        }
        Command::Report { experiment_dir } => {
            let report = trial_runner::build_report(&experiment_dir)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        Command::Resume { experiment_dir } => {
            let report = trial_runner::build_report(&experiment_dir)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            if !report.progress.resume_safe {
                bail!("resume is blocked by ambiguous or interrupted durable trial state");
            }
            pilot::require_paid_dispatch_gates(&experiment_dir)?;
            run_authorized_pilot(&experiment_dir)?;
        }
        Command::Run {
            experiment_dir,
            authorize_max_usd,
            attest_provider_project_budget_usd,
        } => {
            let attestation = pilot::ensure_provider_budget_attestation(
                &experiment_dir,
                &attest_provider_project_budget_usd,
            )?;
            println!("{}", serde_json::to_string(&attestation)?);
            let gate = pilot::paid_run_preflight(&experiment_dir, &authorize_max_usd)?;
            println!("{}", serde_json::to_string_pretty(&gate)?);
            if !gate.ready {
                bail!("paid run is blocked before provider or configuration access");
            }
            match pilot::load_paid_authorization(&experiment_dir) {
                Ok(receipt) if receipt.authorized_max_usd == authorize_max_usd => {}
                Ok(_) => bail!("stored paid authorization differs from this command"),
                Err(_) => {
                    let receipt =
                        pilot::persist_paid_authorization(&experiment_dir, &authorize_max_usd)?;
                    println!("{}", serde_json::to_string(&receipt)?);
                }
            }
            run_authorized_pilot(&experiment_dir)?;
        }
        Command::FixtureCandidate {
            repo,
            fixture_id,
            kind,
            workspace,
        } => {
            let receipt =
                fixtures::verify_materialized_fixture(&repo, &fixture_id, &kind, &workspace)?;
            println!("{}", serde_json::to_string(&receipt)?);
        }
    }
    Ok(())
}

fn run_authorized_pilot(experiment_dir: &Path) -> Result<()> {
    pilot::require_paid_dispatch_gates(experiment_dir)?;
    loop {
        let progress = pilot::inspect_progress(experiment_dir)?;
        let Some(slot_index) = progress
            .slots
            .iter()
            .find(|slot| {
                slot.phase.is_none()
                    || matches!(
                        slot.phase,
                        Some(trial_state::TrialPhaseV1::FixtureReady)
                            | Some(trial_state::TrialPhaseV1::WorkspaceMaterialized)
                    )
            })
            .map(|slot| slot.slot.slot_index)
        else {
            println!(r#"{{"status":"no_pending_trials"}}"#);
            return Ok(());
        };
        let result = trial_runner::execute_trial(experiment_dir, slot_index)?;
        println!("{}", serde_json::to_string(&result)?);
        match trial_runner::calibration_state(experiment_dir)? {
            trial_runner::CalibrationStateV1::Incomplete
            | trial_runner::CalibrationStateV1::Passed => {}
            trial_runner::CalibrationStateV1::Failed => {
                bail!("included calibration failed; remaining paid trials were not started")
            }
        }
    }
}

fn ensure_fixture_check_passed(passed: bool) -> Result<()> {
    if !passed {
        bail!("fixture check failed");
    }
    Ok(())
}

fn validation_error(errors: Vec<String>) -> anyhow::Error {
    anyhow!("validation failed:\n- {}", errors.join("\n- "))
}

fn artifact_sha256(artifact: &ReasoningArtifactV1) -> Result<String> {
    let canonical =
        serde_json::to_vec(artifact).context("cannot deterministically serialize artifact")?;
    Ok(format!("{:x}", Sha256::digest(canonical)))
}

fn read_bounded_file(path: &Path, max_bytes: u64) -> Result<Vec<u8>> {
    let metadata = fs::metadata(path)
        .with_context(|| format!("cannot inspect input file {}", path.display()))?;
    if !metadata.is_file() {
        bail!("input is not a regular file: {}", path.display());
    }
    if metadata.len() > max_bytes {
        bail!(
            "input file {} is {} bytes; maximum is {max_bytes}",
            path.display(),
            metadata.len()
        );
    }
    let file =
        File::open(path).with_context(|| format!("cannot open input file {}", path.display()))?;
    let mut bytes = Vec::with_capacity(
        usize::try_from(metadata.len().min(max_bytes)).context("input size overflows usize")?,
    );
    file.take(max_bytes + 1)
        .read_to_end(&mut bytes)
        .with_context(|| format!("cannot read input file {}", path.display()))?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > max_bytes {
        bail!(
            "input file {} grew beyond the {max_bytes}-byte maximum while reading",
            path.display()
        );
    }
    Ok(bytes)
}

fn validate_artifact(artifact: &ReasoningArtifactV1) -> std::result::Result<(), Vec<String>> {
    let mut errors = Vec::new();

    if artifact.schema_version != ARTIFACT_SCHEMA_VERSION {
        errors.push(format!(
            "schema_version must be {ARTIFACT_SCHEMA_VERSION:?}"
        ));
    }
    check_id("artifact_id", &artifact.artifact_id, &mut errors);
    if DateTime::parse_from_rfc3339(&artifact.created_at).is_err() {
        errors.push("created_at must be an RFC 3339 timestamp".to_string());
    }
    check_nonempty_text(
        "planner.provider",
        &artifact.planner.provider,
        MAX_SHORT_TEXT_BYTES,
        &mut errors,
    );
    check_nonempty_text(
        "planner.model",
        &artifact.planner.model,
        MAX_SHORT_TEXT_BYTES,
        &mut errors,
    );
    check_nonempty_text(
        "planner.reasoning_effort",
        &artifact.planner.reasoning_effort,
        MAX_SHORT_TEXT_BYTES,
        &mut errors,
    );
    check_nonempty_text(
        "task.objective",
        &artifact.task.objective,
        MAX_TEXT_BYTES,
        &mut errors,
    );
    validate_requirement_items(
        "task.constraints",
        &artifact.task.constraints,
        true,
        &mut errors,
    );
    validate_requirement_items(
        "task.success_criteria",
        &artifact.task.success_criteria,
        true,
        &mut errors,
    );
    validate_requirement_items(
        "task.non_goals",
        &artifact.task.non_goals,
        true,
        &mut errors,
    );

    validate_checkpoint(&artifact.repo_checkpoint, &mut errors);
    validate_evidence_and_claims(artifact, &mut errors);
    validate_plan(artifact, &mut errors);
    validate_executor_contract(&artifact.executor_contract, &mut errors);
    validate_step_authority(artifact, &mut errors);
    validate_verifier(artifact, &mut errors);

    if let Ok(value) = serde_json::to_value(artifact) {
        validate_all_strings(&value, "$", &mut errors);
        detect_secret_values(&value, "$", &mut errors);
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn validate_requirement_items(
    label: &str,
    items: &[RequirementItem],
    require_nonempty: bool,
    errors: &mut Vec<String>,
) {
    check_collection_len(label, items.len(), require_nonempty, MAX_ITEMS, errors);
    let mut ids = BTreeSet::new();
    for (index, item) in items.iter().enumerate() {
        check_id(&format!("{label}[{index}].id"), &item.id, errors);
        if !ids.insert(item.id.as_str()) {
            errors.push(format!("{label} contains duplicate id {:?}", item.id));
        }
        check_nonempty_text(
            &format!("{label}[{index}].text"),
            &item.text,
            MAX_TEXT_BYTES,
            errors,
        );
    }
}

fn validate_checkpoint(checkpoint: &RepositoryCheckpoint, errors: &mut Vec<String>) {
    check_nonempty_text(
        "repo_checkpoint.repository_id",
        &checkpoint.repository_id,
        MAX_SHORT_TEXT_BYTES,
        errors,
    );
    check_nonempty_text(
        "repo_checkpoint.base_revision",
        &checkpoint.base_revision,
        MAX_SHORT_TEXT_BYTES,
        errors,
    );
    check_sha256(
        "repo_checkpoint.task_input_sha256",
        &checkpoint.task_input_sha256,
        errors,
    );
    check_sha256(
        "repo_checkpoint.worktree_sha256",
        &checkpoint.worktree_sha256,
        errors,
    );
    check_collection_len(
        "repo_checkpoint.path_digests",
        checkpoint.path_digests.len(),
        true,
        MAX_ITEMS,
        errors,
    );
    let mut paths = BTreeSet::new();
    for (index, digest) in checkpoint.path_digests.iter().enumerate() {
        check_relative_repo_path(
            &format!("repo_checkpoint.path_digests[{index}].path"),
            &digest.path,
            errors,
        );
        check_sha256(
            &format!("repo_checkpoint.path_digests[{index}].sha256"),
            &digest.sha256,
            errors,
        );
        if !paths.insert(digest.path.as_str()) {
            errors.push(format!(
                "repo_checkpoint.path_digests contains duplicate path {:?}",
                digest.path
            ));
        }
    }
}

fn validate_evidence_and_claims(artifact: &ReasoningArtifactV1, errors: &mut Vec<String>) {
    check_collection_len("evidence", artifact.evidence.len(), true, MAX_ITEMS, errors);
    check_collection_len("claims", artifact.claims.len(), true, MAX_ITEMS, errors);
    check_collection_len(
        "uncertainties",
        artifact.uncertainties.len(),
        true,
        MAX_ITEMS,
        errors,
    );

    let checkpoint_paths: BTreeSet<&str> = artifact
        .repo_checkpoint
        .path_digests
        .iter()
        .map(|item| item.path.as_str())
        .collect();
    let mut evidence_ids = BTreeSet::new();
    for (index, evidence) in artifact.evidence.iter().enumerate() {
        check_id(&format!("evidence[{index}].id"), &evidence.id, errors);
        if !evidence_ids.insert(evidence.id.as_str()) {
            errors.push(format!("evidence contains duplicate id {:?}", evidence.id));
        }
        check_nonempty_text(
            &format!("evidence[{index}].observation"),
            &evidence.observation,
            MAX_TEXT_BYTES,
            errors,
        );
        check_nonempty_text(
            &format!("evidence[{index}].source"),
            &evidence.source,
            MAX_SHORT_TEXT_BYTES,
            errors,
        );
        check_collection_len(
            &format!("evidence[{index}].checkpoint_paths"),
            evidence.checkpoint_paths.len(),
            false,
            MAX_ITEMS,
            errors,
        );
        let mut local_paths = BTreeSet::new();
        for path in &evidence.checkpoint_paths {
            check_relative_repo_path(&format!("evidence[{index}].checkpoint_paths"), path, errors);
            if !local_paths.insert(path.as_str()) {
                errors.push(format!(
                    "evidence item {:?} repeats checkpoint path {path:?}",
                    evidence.id
                ));
            }
            if !checkpoint_paths.contains(path.as_str()) {
                errors.push(format!(
                    "evidence item {:?} references missing checkpoint path {path:?}",
                    evidence.id
                ));
            }
        }
    }

    let mut claim_ids = BTreeSet::new();
    for (index, claim) in artifact.claims.iter().enumerate() {
        check_id(&format!("claims[{index}].id"), &claim.id, errors);
        if !claim_ids.insert(claim.id.as_str()) {
            errors.push(format!("claims contains duplicate id {:?}", claim.id));
        }
        check_nonempty_text(
            &format!("claims[{index}].statement"),
            &claim.statement,
            MAX_TEXT_BYTES,
            errors,
        );
        validate_references(
            &format!("claims[{index}].evidence_ids"),
            &claim.evidence_ids,
            &evidence_ids,
            true,
            errors,
        );
    }

    let mut uncertainty_ids = BTreeSet::new();
    for (index, uncertainty) in artifact.uncertainties.iter().enumerate() {
        check_id(
            &format!("uncertainties[{index}].id"),
            &uncertainty.id,
            errors,
        );
        if !uncertainty_ids.insert(uncertainty.id.as_str()) {
            errors.push(format!(
                "uncertainties contains duplicate id {:?}",
                uncertainty.id
            ));
        }
        check_nonempty_text(
            &format!("uncertainties[{index}].statement"),
            &uncertainty.statement,
            MAX_TEXT_BYTES,
            errors,
        );
        check_nonempty_text(
            &format!("uncertainties[{index}].impact"),
            &uncertainty.impact,
            MAX_TEXT_BYTES,
            errors,
        );
        check_nonempty_text(
            &format!("uncertainties[{index}].resolution"),
            &uncertainty.resolution,
            MAX_TEXT_BYTES,
            errors,
        );
        validate_references(
            &format!("uncertainties[{index}].evidence_ids"),
            &uncertainty.evidence_ids,
            &evidence_ids,
            false,
            errors,
        );
    }
}

fn validate_plan(artifact: &ReasoningArtifactV1, errors: &mut Vec<String>) {
    let steps = &artifact.plan.steps;
    check_collection_len("plan.steps", steps.len(), true, MAX_ITEMS, errors);
    let evidence_ids: BTreeSet<&str> = artifact
        .evidence
        .iter()
        .map(|item| item.id.as_str())
        .collect();
    let mut step_ids = BTreeSet::new();
    for (index, step) in steps.iter().enumerate() {
        check_id(&format!("plan.steps[{index}].id"), &step.id, errors);
        if !step_ids.insert(step.id.as_str()) {
            errors.push(format!("plan.steps contains duplicate id {:?}", step.id));
        }
        check_nonempty_text(
            &format!("plan.steps[{index}].title"),
            &step.title,
            MAX_SHORT_TEXT_BYTES,
            errors,
        );
        check_nonempty_text(
            &format!("plan.steps[{index}].instructions"),
            &step.instructions,
            MAX_TEXT_BYTES,
            errors,
        );
        check_collection_len(
            &format!("plan.steps[{index}].affected_paths"),
            step.affected_paths.len(),
            true,
            MAX_ITEMS,
            errors,
        );
        let mut affected_paths = BTreeSet::new();
        for path in &step.affected_paths {
            check_relative_repo_path(&format!("plan.steps[{index}].affected_paths"), path, errors);
            if !affected_paths.insert(path.as_str()) {
                errors.push(format!(
                    "plan step {:?} repeats affected path {path:?}",
                    step.id
                ));
            }
        }
        check_collection_len(
            &format!("plan.steps[{index}].created_paths"),
            step.created_paths.len(),
            false,
            MAX_ITEMS,
            errors,
        );
        let mut created_paths = BTreeSet::new();
        for path in &step.created_paths {
            check_relative_repo_path(&format!("plan.steps[{index}].created_paths"), path, errors);
            if !created_paths.insert(path.as_str()) {
                errors.push(format!(
                    "plan step {:?} repeats created path {path:?}",
                    step.id
                ));
            }
            if !affected_paths.contains(path.as_str()) {
                errors.push(format!(
                    "plan step {:?} declares created path {path:?} outside affected_paths",
                    step.id
                ));
            }
        }
        validate_references(
            &format!("plan.steps[{index}].evidence_ids"),
            &step.evidence_ids,
            &evidence_ids,
            true,
            errors,
        );
        validate_text_list(
            &format!("plan.steps[{index}].validation.commands"),
            &step.validation.commands,
            true,
            MAX_ITEMS,
            errors,
        );
        for (command_index, command) in step.validation.commands.iter().enumerate() {
            check_command_text(
                &format!("plan.steps[{index}].validation.commands[{command_index}]"),
                command,
                errors,
            );
        }
        validate_text_list(
            &format!("plan.steps[{index}].validation.expected_outcomes"),
            &step.validation.expected_outcomes,
            true,
            MAX_ITEMS,
            errors,
        );
    }

    for (index, step) in steps.iter().enumerate() {
        validate_references(
            &format!("plan.steps[{index}].depends_on"),
            &step.depends_on,
            &step_ids,
            false,
            errors,
        );
        if step.depends_on.iter().any(|id| id == &step.id) {
            errors.push(format!("plan step {:?} cannot depend on itself", step.id));
        }
    }
    if plan_has_cycle(steps) {
        errors.push("plan.steps dependency graph contains a cycle".to_string());
    }
}

fn validate_executor_contract(contract: &ExecutorContract, errors: &mut Vec<String>) {
    check_collection_len(
        "executor_contract.allowed_tools",
        contract.allowed_tools.len(),
        true,
        16,
        errors,
    );
    let unique_tools: BTreeSet<_> = contract.allowed_tools.iter().collect();
    if unique_tools.len() != contract.allowed_tools.len() {
        errors.push("executor_contract.allowed_tools contains duplicates".to_string());
    }
    check_collection_len(
        "executor_contract.allowed_paths",
        contract.allowed_paths.len(),
        true,
        MAX_ITEMS,
        errors,
    );
    for path in &contract.allowed_paths {
        check_relative_repo_path("executor_contract.allowed_paths", path, errors);
    }
    validate_text_list(
        "executor_contract.forbidden_actions",
        &contract.forbidden_actions,
        true,
        MAX_ITEMS,
        errors,
    );
    validate_text_list(
        "executor_contract.stop_conditions",
        &contract.stop_conditions,
        true,
        MAX_ITEMS,
        errors,
    );
    validate_text_list(
        "executor_contract.escalation_triggers",
        &contract.escalation_triggers,
        true,
        MAX_ITEMS,
        errors,
    );

    let budgets = &contract.budgets;
    if budgets.max_turns == 0 || budgets.max_turns > 10_000 {
        errors.push("executor_contract.budgets.max_turns must be in 1..=10000".to_string());
    }
    if budgets.max_requests == 0 || budgets.max_requests > 10_000 {
        errors.push("executor_contract.budgets.max_requests must be in 1..=10000".to_string());
    }
    if budgets.max_input_tokens == 0 || budgets.max_input_tokens > MAX_TOKEN_COUNT {
        errors.push(format!(
            "executor_contract.budgets.max_input_tokens must be in 1..={MAX_TOKEN_COUNT}"
        ));
    }
    if budgets.max_output_tokens == 0 || budgets.max_output_tokens > MAX_TOKEN_COUNT {
        errors.push(format!(
            "executor_contract.budgets.max_output_tokens must be in 1..={MAX_TOKEN_COUNT}"
        ));
    }
    if budgets.max_tool_calls == 0 || budgets.max_tool_calls > MAX_TOOL_CALLS {
        errors.push(format!(
            "executor_contract.budgets.max_tool_calls must be in 1..={MAX_TOOL_CALLS}"
        ));
    }
    if budgets.max_wall_time_seconds == 0 || budgets.max_wall_time_seconds > 604_800 {
        errors.push(
            "executor_contract.budgets.max_wall_time_seconds must be in 1..=604800".to_string(),
        );
    }
    match Money::parse_fee(&budgets.max_cost_usd) {
        Ok(cost) if cost.atto_usd > 0 => {}
        Ok(_) => errors
            .push("executor_contract.budgets.max_cost_usd must be greater than zero".to_string()),
        Err(error) => errors.push(format!(
            "executor_contract.budgets.max_cost_usd is invalid: {error}"
        )),
    }
}

fn validate_step_authority(artifact: &ReasoningArtifactV1, errors: &mut Vec<String>) {
    let allowed_paths: BTreeSet<&str> = artifact
        .executor_contract
        .allowed_paths
        .iter()
        .map(String::as_str)
        .collect();
    let checkpoint_paths: BTreeSet<&str> = artifact
        .repo_checkpoint
        .path_digests
        .iter()
        .map(|item| item.path.as_str())
        .collect();
    let allowed_tools: BTreeSet<&ToolCapability> =
        artifact.executor_contract.allowed_tools.iter().collect();
    let mut created_by: BTreeMap<&str, &str> = BTreeMap::new();
    for step in &artifact.plan.steps {
        for path in &step.created_paths {
            if let Some(first_step) = created_by.insert(path.as_str(), step.id.as_str()) {
                errors.push(format!(
                    "created path {path:?} is declared by both plan steps {first_step:?} and {:?}",
                    step.id
                ));
            }
        }
    }
    for step in &artifact.plan.steps {
        for path in &step.affected_paths {
            if !allowed_paths.contains(path.as_str()) {
                errors.push(format!(
                    "plan step {:?} affects path {path:?} outside executor_contract.allowed_paths",
                    step.id
                ));
            }
            let checkpointed = checkpoint_paths.contains(path.as_str());
            let create_step = created_by.get(path.as_str()).copied();
            let available_from_prior_create = create_step.is_some_and(|creator| {
                creator == step.id
                    || step_depends_transitively_on(&artifact.plan.steps, &step.id, creator)
            });
            if checkpointed == available_from_prior_create {
                errors.push(format!(
                    "plan step {:?} affected path {path:?} must be exactly one of a checkpointed existing path or a created path from this step or its dependencies",
                    step.id
                ));
            }
        }
        if !step.affected_paths.is_empty()
            && !allowed_tools.contains(&ToolCapability::EditRepository)
        {
            errors.push(format!(
                "plan step {:?} affects repository paths but executor_contract.allowed_tools omits edit_repository",
                step.id
            ));
        }
        if !step.validation.commands.is_empty()
            && !allowed_tools.contains(&ToolCapability::RunApprovedCommands)
        {
            errors.push(format!(
                "plan step {:?} has validation commands but executor_contract.allowed_tools omits run_approved_commands",
                step.id
            ));
        }
    }
}

fn step_depends_transitively_on(steps: &[PlanStep], step_id: &str, ancestor_id: &str) -> bool {
    let by_id: BTreeMap<&str, &PlanStep> =
        steps.iter().map(|step| (step.id.as_str(), step)).collect();
    let mut pending = vec![step_id];
    let mut visited = BTreeSet::new();
    while let Some(current) = pending.pop() {
        if !visited.insert(current) {
            continue;
        }
        let Some(step) = by_id.get(current) else {
            continue;
        };
        for dependency in &step.depends_on {
            if dependency == ancestor_id {
                return true;
            }
            pending.push(dependency);
        }
    }
    false
}

fn validate_verifier(artifact: &ReasoningArtifactV1, errors: &mut Vec<String>) {
    let verifier = &artifact.verifier;
    check_collection_len(
        "verifier.gates",
        verifier.gates.len(),
        true,
        MAX_ITEMS,
        errors,
    );
    let criterion_ids: BTreeSet<&str> = artifact
        .task
        .success_criteria
        .iter()
        .map(|item| item.id.as_str())
        .collect();
    let step_ids: BTreeSet<&str> = artifact
        .plan
        .steps
        .iter()
        .map(|item| item.id.as_str())
        .collect();
    let mut gate_ids = BTreeSet::new();
    for (index, gate) in verifier.gates.iter().enumerate() {
        check_id(&format!("verifier.gates[{index}].id"), &gate.id, errors);
        if !gate_ids.insert(gate.id.as_str()) {
            errors.push(format!(
                "verifier.gates contains duplicate id {:?}",
                gate.id
            ));
        }
        validate_references(
            &format!("verifier.gates[{index}].criterion_ids"),
            &gate.criterion_ids,
            &criterion_ids,
            true,
            errors,
        );
        validate_references(
            &format!("verifier.gates[{index}].step_ids"),
            &gate.step_ids,
            &step_ids,
            true,
            errors,
        );
        check_nonempty_text(
            &format!("verifier.gates[{index}].command"),
            &gate.command,
            MAX_TEXT_BYTES,
            errors,
        );
        check_command_text(
            &format!("verifier.gates[{index}].command"),
            &gate.command,
            errors,
        );
        check_nonempty_text(
            &format!("verifier.gates[{index}].expected_outcome"),
            &gate.expected_outcome,
            MAX_TEXT_BYTES,
            errors,
        );
    }

    let covered_criteria: BTreeSet<&str> = verifier
        .gates
        .iter()
        .filter(|gate| gate.required)
        .flat_map(|gate| gate.criterion_ids.iter().map(String::as_str))
        .collect();
    for criterion in &artifact.task.success_criteria {
        if !covered_criteria.contains(criterion.id.as_str()) {
            errors.push(format!(
                "success criterion {:?} is not covered by a required verifier gate",
                criterion.id
            ));
        }
    }
    let covered_steps: BTreeSet<&str> = verifier
        .gates
        .iter()
        .filter(|gate| gate.required)
        .flat_map(|gate| gate.step_ids.iter().map(String::as_str))
        .collect();
    for step in &artifact.plan.steps {
        if !covered_steps.contains(step.id.as_str()) {
            errors.push(format!(
                "plan step {:?} is not covered by a required verifier gate",
                step.id
            ));
        }
    }

    let policy = &verifier.escalation_policy;
    if policy.max_escalations > 8 {
        errors.push("verifier.escalation_policy.max_escalations must be <= 8".to_string());
    }
    if policy.max_escalations == 0
        && (matches!(policy.on_required_gate_failure, GateFailureAction::Escalate)
            || matches!(
                policy.on_stale_checkpoint,
                StaleCheckpointAction::RefreshPlan
            ))
    {
        errors.push(
            "verifier escalation actions require max_escalations greater than zero".to_string(),
        );
    }
    if matches!(
        policy.on_stale_checkpoint,
        StaleCheckpointAction::RefreshPlan
    ) && !policy.planner_refresh_allowed
    {
        errors.push(
            "on_stale_checkpoint=refresh_plan requires planner_refresh_allowed=true".to_string(),
        );
    }
}

fn check_collection_len(
    label: &str,
    len: usize,
    require_nonempty: bool,
    maximum: usize,
    errors: &mut Vec<String>,
) {
    if require_nonempty && len == 0 {
        errors.push(format!("{label} must not be empty"));
    }
    if len > maximum {
        errors.push(format!("{label} has {len} items; maximum is {maximum}"));
    }
}

fn check_nonempty_text(label: &str, value: &str, maximum: usize, errors: &mut Vec<String>) {
    if value.trim().is_empty() {
        errors.push(format!("{label} must not be empty"));
    }
    if value.len() > maximum {
        errors.push(format!(
            "{label} is {} bytes; maximum is {maximum}",
            value.len()
        ));
    }
    if value.contains('\0') {
        errors.push(format!("{label} must not contain NUL bytes"));
    }
}

fn check_command_text(label: &str, value: &str, errors: &mut Vec<String>) {
    if value.chars().any(char::is_control) {
        errors.push(format!(
            "{label} must be one printable line without control characters"
        ));
    }
}

fn check_id(label: &str, value: &str, errors: &mut Vec<String>) {
    check_nonempty_text(label, value, MAX_ID_BYTES, errors);
    let valid = value.bytes().enumerate().all(|(index, byte)| match byte {
        b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' => true,
        b'.' | b'_' | b':' | b'-' => index > 0,
        _ => false,
    });
    if !valid {
        errors.push(format!(
            "{label} must start with an ASCII letter or digit and contain only letters, digits, '.', '_', ':', or '-'"
        ));
    }
}

fn check_sha256(label: &str, value: &str, errors: &mut Vec<String>) {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        errors.push(format!(
            "{label} must be a 64-character lowercase hexadecimal SHA-256 digest"
        ));
    }
}

fn check_relative_repo_path(label: &str, value: &str, errors: &mut Vec<String>) {
    if let Err(error) = parse_relative_repo_path(value) {
        errors.push(format!("{label} is unsafe: {error}"));
    }
}

fn parse_relative_repo_path(value: &str) -> Result<&Path> {
    if value.is_empty() {
        bail!("path must not be empty");
    }
    if value.len() > MAX_PATH_BYTES {
        bail!("path is longer than {MAX_PATH_BYTES} bytes");
    }
    if value.chars().any(char::is_control) {
        bail!("path contains a control character");
    }
    if value.contains('\\') {
        bail!("path must use '/' separators");
    }
    let bytes = value.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        bail!("Windows drive paths are not allowed");
    }
    if value.starts_with('/') {
        bail!("absolute paths are not allowed");
    }
    if value.split('/').any(|segment| segment.is_empty()) {
        bail!("empty path components are not allowed");
    }

    let path = Path::new(value);
    if path.is_absolute() {
        bail!("absolute paths are not allowed");
    }
    for component in path.components() {
        match component {
            Component::Normal(_) => {}
            Component::CurDir => bail!("'.' path components are not allowed"),
            Component::ParentDir => bail!("'..' path components are not allowed"),
            Component::RootDir | Component::Prefix(_) => {
                bail!("rooted or prefixed paths are not allowed")
            }
        }
    }
    Ok(path)
}

fn validate_references(
    label: &str,
    references: &[String],
    known: &BTreeSet<&str>,
    require_nonempty: bool,
    errors: &mut Vec<String>,
) {
    check_collection_len(label, references.len(), require_nonempty, MAX_ITEMS, errors);
    let mut local = BTreeSet::new();
    for reference in references {
        check_id(label, reference, errors);
        if !local.insert(reference.as_str()) {
            errors.push(format!("{label} repeats reference {reference:?}"));
        }
        if !known.contains(reference.as_str()) {
            errors.push(format!("{label} contains missing reference {reference:?}"));
        }
    }
}

fn validate_text_list(
    label: &str,
    values: &[String],
    require_nonempty: bool,
    maximum: usize,
    errors: &mut Vec<String>,
) {
    check_collection_len(label, values.len(), require_nonempty, maximum, errors);
    for (index, value) in values.iter().enumerate() {
        check_nonempty_text(&format!("{label}[{index}]"), value, MAX_TEXT_BYTES, errors);
    }
}

fn plan_has_cycle(steps: &[PlanStep]) -> bool {
    let step_ids: BTreeSet<&str> = steps.iter().map(|step| step.id.as_str()).collect();
    let mut indegree: BTreeMap<&str, usize> = steps
        .iter()
        .map(|step| {
            let known_dependencies = step
                .depends_on
                .iter()
                .filter(|dependency| step_ids.contains(dependency.as_str()))
                .count();
            (step.id.as_str(), known_dependencies)
        })
        .collect();
    let mut dependents: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for step in steps {
        for dependency in &step.depends_on {
            if step_ids.contains(dependency.as_str()) {
                dependents
                    .entry(dependency.as_str())
                    .or_default()
                    .push(step.id.as_str());
            }
        }
    }
    let mut queue: VecDeque<&str> = indegree
        .iter()
        .filter_map(|(id, degree)| (*degree == 0).then_some(*id))
        .collect();
    let mut visited = 0;
    while let Some(id) = queue.pop_front() {
        visited += 1;
        if let Some(children) = dependents.get(id) {
            for child in children {
                if let Some(degree) = indegree.get_mut(child) {
                    *degree -= 1;
                    if *degree == 0 {
                        queue.push_back(child);
                    }
                }
            }
        }
    }
    visited != steps.len()
}

fn validate_all_strings(value: &serde_json::Value, path: &str, errors: &mut Vec<String>) {
    match value {
        serde_json::Value::String(text) => {
            if text.len() > MAX_TEXT_BYTES {
                errors.push(format!(
                    "{path} is {} bytes; global string maximum is {MAX_TEXT_BYTES}",
                    text.len()
                ));
            }
            if text.contains('\0') {
                errors.push(format!("{path} contains a NUL byte"));
            }
        }
        serde_json::Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                validate_all_strings(item, &format!("{path}[{index}]"), errors);
            }
        }
        serde_json::Value::Object(object) => {
            for (key, item) in object {
                validate_all_strings(item, &format!("{path}.{key}"), errors);
            }
        }
        _ => {}
    }
}

fn detect_secret_values(value: &serde_json::Value, path: &str, errors: &mut Vec<String>) {
    match value {
        serde_json::Value::String(text) => {
            if looks_like_secret(text) {
                errors.push(format!(
                    "{path} appears to contain a recognizable credential or secret; artifacts must not contain secrets"
                ));
            }
        }
        serde_json::Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                detect_secret_values(item, &format!("{path}[{index}]"), errors);
            }
        }
        serde_json::Value::Object(object) => {
            for (key, item) in object {
                detect_secret_values(item, &format!("{path}.{key}"), errors);
            }
        }
        _ => {}
    }
}

fn looks_like_secret(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    if lower.contains("-----begin private key-----")
        || lower.contains("-----begin rsa private key-----")
        || lower.contains("authorization: bearer ")
    {
        return true;
    }

    for prefix in [
        "sk-proj-",
        "sk-ant-",
        "ghp_",
        "github_pat_",
        "xoxb-",
        "xoxp-",
        "AIza",
    ] {
        if has_long_prefixed_token(text, prefix, 12) {
            return true;
        }
    }
    for assignment in [
        "api_key=",
        "api-key=",
        "access_token=",
        "password=",
        "client_secret=",
    ] {
        if let Some(index) = lower.find(assignment) {
            let tail = &text[index + assignment.len()..];
            if tail
                .split_whitespace()
                .next()
                .is_some_and(|candidate| candidate.len() >= 8)
            {
                return true;
            }
        }
    }
    false
}

fn has_long_prefixed_token(text: &str, prefix: &str, minimum_tail: usize) -> bool {
    let mut offset = 0;
    while let Some(relative) = text[offset..].find(prefix) {
        let start = offset + relative + prefix.len();
        let token_len = text[start..]
            .bytes()
            .take_while(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
            .count();
        if token_len >= minimum_tail {
            return true;
        }
        offset = start;
        if offset >= text.len() {
            break;
        }
    }
    false
}

fn verify_repository_state(root: &Path, artifact: &ReasoningArtifactV1) -> Result<()> {
    verify_repository_state_with_budget(root, artifact, MAX_CHECKPOINT_TOTAL_BYTES)
}

fn verify_repository_state_with_budget(
    root: &Path,
    artifact: &ReasoningArtifactV1,
    total_byte_budget: u64,
) -> Result<()> {
    let root_metadata = fs::symlink_metadata(root)
        .with_context(|| format!("cannot inspect repository root {}", root.display()))?;
    if root_metadata.file_type().is_symlink() {
        bail!("repository root must not be a symlink: {}", root.display());
    }
    if !root_metadata.is_dir() {
        bail!("repository root is not a directory: {}", root.display());
    }
    let canonical_root = root
        .canonicalize()
        .with_context(|| format!("cannot canonicalize repository root {}", root.display()))?;

    let mut stale = Vec::new();
    let mut total_read = 0_u64;
    for expected in &artifact.repo_checkpoint.path_digests {
        let relative = parse_relative_repo_path(&expected.path)
            .with_context(|| format!("unsafe checkpoint path {:?}", expected.path))?;
        match hash_repo_file_without_symlinks(
            &canonical_root,
            relative,
            &mut total_read,
            total_byte_budget,
        ) {
            Ok(actual) if actual == expected.sha256 => {}
            Ok(actual) => stale.push(format!(
                "{} digest mismatch (expected {}, observed {})",
                expected.path, expected.sha256, actual
            )),
            Err(error) => stale.push(format!("{} cannot be verified: {error:#}", expected.path)),
        }
    }

    let created_paths: BTreeSet<&str> = artifact
        .plan
        .steps
        .iter()
        .flat_map(|step| step.created_paths.iter().map(String::as_str))
        .collect();
    for path in created_paths {
        let relative = parse_relative_repo_path(path)
            .with_context(|| format!("unsafe created path {path:?}"))?;
        if let Err(error) = verify_repo_path_absent_without_symlinks(&canonical_root, relative) {
            stale.push(format!("{path} is not a valid new path: {error:#}"));
        }
    }
    if stale.is_empty() {
        Ok(())
    } else {
        bail!(
            "repository dependency checkpoint is stale:\n- {}",
            stale.join("\n- ")
        )
    }
}

fn hash_repo_file_without_symlinks(
    root: &Path,
    relative: &Path,
    aggregate_read: &mut u64,
    total_byte_budget: u64,
) -> Result<String> {
    let mut file = open_repo_file_without_symlinks(root, relative)?;
    let metadata = file
        .metadata()
        .context("cannot inspect opened checkpoint dependency")?;
    if !metadata.is_file() {
        bail!("checkpoint dependency is not a regular file");
    }
    if metadata.len() > MAX_CHECKPOINT_FILE_BYTES {
        bail!(
            "checkpoint dependency is {} bytes; maximum is {MAX_CHECKPOINT_FILE_BYTES}",
            metadata.len()
        );
    }
    if metadata.len() > total_byte_budget.saturating_sub(*aggregate_read) {
        bail!("checkpoint dependencies exceed the aggregate verification byte budget");
    }
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut file_read = 0_u64;
    loop {
        let read = file
            .read(&mut buffer)
            .context("cannot read checkpoint dependency")?;
        if read == 0 {
            break;
        }
        file_read = file_read
            .checked_add(u64::try_from(read).context("checkpoint read size overflows")?)
            .ok_or_else(|| anyhow!("checkpoint read size overflows"))?;
        *aggregate_read = aggregate_read
            .checked_add(u64::try_from(read).context("checkpoint read size overflows")?)
            .ok_or_else(|| anyhow!("checkpoint aggregate read size overflows"))?;
        if file_read > MAX_CHECKPOINT_FILE_BYTES {
            bail!(
                "checkpoint dependency grew beyond the {MAX_CHECKPOINT_FILE_BYTES}-byte maximum while reading"
            );
        }
        if *aggregate_read > total_byte_budget {
            bail!(
                "checkpoint dependencies grew beyond the aggregate verification byte budget while reading"
            );
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(unix)]
fn open_repo_file_without_symlinks(root: &Path, relative: &Path) -> Result<File> {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;

    let mut directory = File::open(root)
        .with_context(|| format!("cannot open repository root {}", root.display()))?;
    let components: Vec<_> = relative.components().collect();
    for (index, component) in components.iter().enumerate() {
        let Component::Normal(segment) = component else {
            bail!("path contains a non-normal component");
        };
        let name = CString::new(segment.as_bytes()).context("path component contains NUL")?;
        let is_last = index + 1 == components.len();
        let mut flags = libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW;
        if !is_last {
            flags |= libc::O_DIRECTORY;
        } else {
            flags |= libc::O_NONBLOCK;
        }
        // SAFETY: `directory` is a live directory/file descriptor, `name` is
        // NUL-terminated, and a successful descriptor is immediately owned.
        let descriptor = unsafe { libc::openat(directory.as_raw_fd(), name.as_ptr(), flags) };
        if descriptor < 0 {
            let error = io::Error::last_os_error();
            if is_nofollow_symlink_error(&error) {
                bail!("symlink components are not allowed");
            }
            return Err(error).context("cannot open checkpoint path component");
        }
        // SAFETY: `openat` returned a new owned descriptor on success.
        let opened = unsafe { File::from_raw_fd(descriptor) };
        if is_last {
            return Ok(opened);
        }
        directory = opened;
    }
    bail!("checkpoint path has no components")
}

#[cfg(not(unix))]
fn open_repo_file_without_symlinks(root: &Path, relative: &Path) -> Result<File> {
    let mut current = root.to_path_buf();
    let components: Vec<_> = relative.components().collect();
    for (index, component) in components.iter().enumerate() {
        let Component::Normal(segment) = component else {
            bail!("path contains a non-normal component");
        };
        current.push(segment);
        let metadata = fs::symlink_metadata(&current)
            .with_context(|| format!("cannot inspect {}", current.display()))?;
        if metadata.file_type().is_symlink() {
            bail!("symlink components are not allowed: {}", current.display());
        }
        if index + 1 != components.len() && !metadata.is_dir() {
            bail!("intermediate checkpoint component is not a directory");
        }
    }
    File::open(&current)
        .with_context(|| format!("cannot open checkpoint dependency {}", current.display()))
}

#[cfg(unix)]
fn verify_repo_path_absent_without_symlinks(root: &Path, relative: &Path) -> Result<()> {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;

    let mut directory = File::open(root)
        .with_context(|| format!("cannot open repository root {}", root.display()))?;
    let components: Vec<_> = relative.components().collect();
    for (index, component) in components.iter().enumerate() {
        let Component::Normal(segment) = component else {
            bail!("path contains a non-normal component");
        };
        let name = CString::new(segment.as_bytes()).context("path component contains NUL")?;
        let is_last = index + 1 == components.len();
        let mut flags = libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW;
        if !is_last {
            flags |= libc::O_DIRECTORY;
        } else {
            flags |= libc::O_NONBLOCK;
        }
        // SAFETY: see `open_repo_file_without_symlinks`; each component is
        // resolved relative to the previously opened directory descriptor.
        let descriptor = unsafe { libc::openat(directory.as_raw_fd(), name.as_ptr(), flags) };
        if descriptor < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::NotFound {
                return Ok(());
            }
            if is_nofollow_symlink_error(&error) {
                bail!("symlink components are not allowed");
            }
            return Err(error).context("cannot inspect prospective create path component");
        }
        // SAFETY: `openat` returned a new owned descriptor on success.
        let opened = unsafe { File::from_raw_fd(descriptor) };
        if is_last {
            bail!("path already exists");
        }
        directory = opened;
    }
    bail!("path already exists")
}

#[cfg(unix)]
fn is_nofollow_symlink_error(error: &io::Error) -> bool {
    if error.raw_os_error() == Some(libc::ELOOP) {
        return true;
    }
    #[cfg(target_os = "freebsd")]
    if error.raw_os_error() == Some(libc::EMLINK) {
        return true;
    }
    false
}

#[cfg(not(unix))]
fn verify_repo_path_absent_without_symlinks(root: &Path, relative: &Path) -> Result<()> {
    let mut current = root.to_path_buf();
    let components: Vec<_> = relative.components().collect();
    for (index, component) in components.iter().enumerate() {
        let Component::Normal(segment) = component else {
            bail!("path contains a non-normal component");
        };
        current.push(segment);
        match fs::symlink_metadata(&current) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    bail!("symlink components are not allowed: {}", current.display());
                }
                if index + 1 == components.len() {
                    bail!("path already exists");
                }
                if !metadata.is_dir() {
                    bail!("intermediate component is not a directory");
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                return Err(error).with_context(|| format!("cannot inspect {}", current.display()))
            }
        }
    }
    bail!("path already exists")
}

impl Money {
    fn parse_fee(value: &str) -> Result<Self> {
        let (coefficient, scale) = parse_nonnegative_decimal(value, MONEY_SCALE)?;
        let multiplier = checked_pow10(MONEY_SCALE - scale)?;
        let atto_usd = coefficient
            .checked_mul(multiplier)
            .ok_or_else(|| anyhow!("money value overflows"))?;
        Ok(Self { atto_usd })
    }

    fn from_per_million(value: &str, units: u64) -> Result<Self> {
        // At most 12 fractional rate digits keeps token * rate / 1,000,000
        // exactly representable at the 18-decimal atto-dollar scale.
        let (coefficient, scale) = parse_nonnegative_decimal(value, 12)?;
        let exponent = MONEY_SCALE
            .checked_sub(scale + 6)
            .ok_or_else(|| anyhow!("per-million rate has too many decimal places"))?;
        let multiplier = checked_pow10(exponent)?;
        let atto_usd = coefficient
            .checked_mul(i128::from(units))
            .and_then(|amount| amount.checked_mul(multiplier))
            .ok_or_else(|| anyhow!("token cost overflows"))?;
        Ok(Self { atto_usd })
    }

    fn checked_add(self, other: Self) -> Result<Self> {
        Ok(Self {
            atto_usd: self
                .atto_usd
                .checked_add(other.atto_usd)
                .ok_or_else(|| anyhow!("money sum overflows"))?,
        })
    }

    fn checked_mul(self, count: u64) -> Result<Self> {
        Ok(Self {
            atto_usd: self
                .atto_usd
                .checked_mul(i128::from(count))
                .ok_or_else(|| anyhow!("money multiplication overflows"))?,
        })
    }

    fn decimal_string(self) -> String {
        format_scaled_decimal(self.atto_usd, MONEY_SCALE)
    }
}

fn parse_nonnegative_decimal(value: &str, max_scale: u32) -> Result<(i128, u32)> {
    if value.is_empty() {
        bail!("decimal string must not be empty");
    }
    if value.len() > 64 {
        bail!("decimal string is longer than 64 bytes");
    }
    if value.starts_with('+') || value.starts_with('-') {
        bail!("decimal string must be nonnegative and must not include a sign");
    }
    let mut pieces = value.split('.');
    let whole = pieces.next().unwrap_or_default();
    let fraction = pieces.next();
    if pieces.next().is_some() {
        bail!("decimal string may contain at most one decimal point");
    }
    if whole.is_empty() || !whole.bytes().all(|byte| byte.is_ascii_digit()) {
        bail!("decimal string must have an integer part containing only digits");
    }
    let fraction = fraction.unwrap_or_default();
    if !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
        bail!("decimal fraction must contain only digits");
    }
    let scale = u32::try_from(fraction.len()).map_err(|_| anyhow!("decimal scale overflows"))?;
    if scale > max_scale {
        bail!("decimal string has {scale} fractional digits; maximum is {max_scale}");
    }
    let whole_value: i128 = whole.parse().context("decimal integer part overflows")?;
    let multiplier = checked_pow10(scale)?;
    let fraction_value = if fraction.is_empty() {
        0
    } else {
        fraction.parse().context("decimal fraction overflows")?
    };
    let coefficient = whole_value
        .checked_mul(multiplier)
        .and_then(|amount| amount.checked_add(fraction_value))
        .ok_or_else(|| anyhow!("decimal value overflows"))?;
    Ok((coefficient, scale))
}

fn checked_pow10(exponent: u32) -> Result<i128> {
    10_i128
        .checked_pow(exponent)
        .ok_or_else(|| anyhow!("decimal scale overflows"))
}

fn format_scaled_decimal(amount: i128, scale: u32) -> String {
    debug_assert!(amount >= 0);
    let factor = 10_i128.pow(scale);
    let whole = amount / factor;
    let fraction = amount % factor;
    if fraction == 0 {
        return whole.to_string();
    }
    let mut fraction_text = format!("{fraction:0width$}", width = scale as usize);
    while fraction_text.ends_with('0') {
        fraction_text.pop();
    }
    format!("{whole}.{fraction_text}")
}

fn aggregate_cost(runs: &BenchmarkRuns) -> std::result::Result<CostSummary, Vec<String>> {
    let mut errors = Vec::new();
    if runs.schema_version != RUNS_SCHEMA_VERSION {
        errors.push(format!("schema_version must be {RUNS_SCHEMA_VERSION:?}"));
    }
    if runs.currency != "USD" {
        errors.push("currency must be \"USD\"".to_string());
    }
    check_id("experiment_id", &runs.experiment_id, &mut errors);
    check_id("arm_id", &runs.arm_id, &mut errors);
    check_collection_len("trials", runs.trials.len(), true, MAX_TRIALS, &mut errors);

    let mut trial_ids = BTreeSet::new();
    let mut case_repetitions = BTreeSet::new();
    for (trial_index, trial) in runs.trials.iter().enumerate() {
        check_id(&format!("trials[{trial_index}].id"), &trial.id, &mut errors);
        if !trial_ids.insert(trial.id.as_str()) {
            errors.push(format!("trials contains duplicate id {:?}", trial.id));
        }
        check_id(
            &format!("trials[{trial_index}].case_id"),
            &trial.case_id,
            &mut errors,
        );
        if trial.repetition > 1_000_000 {
            errors.push(format!(
                "trials[{trial_index}].repetition must be <= 1000000"
            ));
        }
        if !case_repetitions.insert((trial.case_id.as_str(), trial.repetition)) {
            errors.push(format!(
                "trials repeats case_id {:?} repetition {}",
                trial.case_id, trial.repetition
            ));
        }
        check_collection_len(
            &format!("trials[{trial_index}].requests"),
            trial.requests.len(),
            true,
            MAX_REQUESTS_PER_TRIAL,
            &mut errors,
        );
        for (request_index, request) in trial.requests.iter().enumerate() {
            let label = format!("trials[{trial_index}].requests[{request_index}]");
            validate_costed_request(&label, request, &mut errors);
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }

    match aggregate_valid_costs(runs) {
        Ok(summary) => Ok(summary),
        Err(error) => Err(vec![error.to_string()]),
    }
}

fn validate_costed_request(label: &str, request: &CostedRequest, errors: &mut Vec<String>) {
    check_nonempty_text(
        &format!("{label}.provider"),
        &request.provider,
        MAX_SHORT_TEXT_BYTES,
        errors,
    );
    check_nonempty_text(
        &format!("{label}.model"),
        &request.model,
        MAX_SHORT_TEXT_BYTES,
        errors,
    );
    check_nonempty_text(
        &format!("{label}.pricing.price_card_id"),
        &request.pricing.price_card_id,
        MAX_SHORT_TEXT_BYTES,
        errors,
    );
    if DateTime::parse_from_rfc3339(&request.pricing.price_card_effective_at).is_err() {
        errors.push(format!(
            "{label}.pricing.price_card_effective_at must be an RFC 3339 timestamp"
        ));
    }
    check_nonempty_text(
        &format!("{label}.pricing.billing_tier"),
        &request.pricing.billing_tier,
        MAX_SHORT_TEXT_BYTES,
        errors,
    );
    for (field, tokens) in [
        ("regular_input_tokens", request.regular_input_tokens),
        ("cached_input_tokens", request.cached_input_tokens),
        ("cache_write_tokens", request.cache_write_tokens),
        ("regular_output_tokens", request.regular_output_tokens),
        ("reasoning_output_tokens", request.reasoning_output_tokens),
    ] {
        if tokens > MAX_TOKEN_COUNT {
            errors.push(format!(
                "{label}.{field} is {tokens}; maximum is {MAX_TOKEN_COUNT}"
            ));
        }
    }
    for (field, rate) in [
        (
            "input_per_million_usd",
            request.pricing.input_per_million_usd.as_str(),
        ),
        (
            "cached_input_per_million_usd",
            request.pricing.cached_input_per_million_usd.as_str(),
        ),
        (
            "cache_write_per_million_usd",
            request.pricing.cache_write_per_million_usd.as_str(),
        ),
        (
            "output_per_million_usd",
            request.pricing.output_per_million_usd.as_str(),
        ),
        (
            "reasoning_output_per_million_usd",
            request.pricing.reasoning_output_per_million_usd.as_str(),
        ),
    ] {
        if let Err(error) = Money::from_per_million(rate, 0) {
            errors.push(format!("{label}.pricing.{field} is invalid: {error}"));
        }
    }
    if let Err(error) = Money::parse_fee(&request.pricing.request_fee_usd) {
        errors.push(format!(
            "{label}.pricing.request_fee_usd is invalid: {error}"
        ));
    }
    check_collection_len(
        &format!("{label}.tool_fees"),
        request.tool_fees.len(),
        false,
        MAX_TOOL_FEES_PER_REQUEST,
        errors,
    );
    for (index, fee) in request.tool_fees.iter().enumerate() {
        check_nonempty_text(
            &format!("{label}.tool_fees[{index}].name"),
            &fee.name,
            MAX_SHORT_TEXT_BYTES,
            errors,
        );
        if fee.calls > MAX_TOOL_CALLS {
            errors.push(format!(
                "{label}.tool_fees[{index}].calls is {}; maximum is {MAX_TOOL_CALLS}",
                fee.calls
            ));
        }
        if let Err(error) = Money::parse_fee(&fee.fee_per_call_usd) {
            errors.push(format!(
                "{label}.tool_fees[{index}].fee_per_call_usd is invalid: {error}"
            ));
        }
    }
}

fn aggregate_valid_costs(runs: &BenchmarkRuns) -> Result<CostSummary> {
    let mut stages: BTreeMap<Stage, StageAccumulator> = [
        (Stage::Planner, StageAccumulator::default()),
        (Stage::Executor, StageAccumulator::default()),
        (Stage::Verifier, StageAccumulator::default()),
        (Stage::Escalation, StageAccumulator::default()),
    ]
    .into_iter()
    .collect();
    let mut overall = StageAccumulator::default();
    let mut successes = 0_u64;

    for trial in &runs.trials {
        if matches!(trial.outcome, TrialOutcome::Success) {
            successes = successes
                .checked_add(1)
                .ok_or_else(|| anyhow!("success count overflows"))?;
        }
        for request in &trial.requests {
            let (cost, tokens, tool_calls) = request_cost(request)?;
            add_request_to_accumulator(&mut overall, cost, tokens, tool_calls)?;
            let stage = stages
                .get_mut(&request.stage)
                .ok_or_else(|| anyhow!("missing stage accumulator"))?;
            add_request_to_accumulator(stage, cost, tokens, tool_calls)?;
        }
    }

    let trial_count = u64::try_from(runs.trials.len()).context("trial count overflows")?;
    let failures = trial_count
        .checked_sub(successes)
        .ok_or_else(|| anyhow!("failure count underflows"))?;
    let mean_cost_usd = ratio_cost(overall.cost, trial_count)
        .ok_or_else(|| anyhow!("trial count must be greater than zero"))?;
    let dollars_per_success = ratio_cost(overall.cost, successes);

    let planner = stage_summary(stages.remove(&Stage::Planner).unwrap_or_default());
    let executor = stage_summary(stages.remove(&Stage::Executor).unwrap_or_default());
    let verifier = stage_summary(stages.remove(&Stage::Verifier).unwrap_or_default());
    let escalation = stage_summary(stages.remove(&Stage::Escalation).unwrap_or_default());

    Ok(CostSummary {
        schema_version: SUMMARY_SCHEMA_VERSION,
        currency: "USD",
        experiment_id: runs.experiment_id.clone(),
        arm_id: runs.arm_id.clone(),
        trial_count,
        successes,
        failures,
        request_count: overall.request_count,
        tokens: overall.tokens,
        tool_calls: overall.tool_calls,
        total_cost_usd: overall.cost.decimal_string(),
        mean_cost_usd,
        dollars_per_success,
        stages: StageBreakdown {
            planner,
            executor,
            verifier,
            escalation,
        },
    })
}

fn request_cost(request: &CostedRequest) -> Result<(Money, TokenTotals, u64)> {
    let pricing = &request.pricing;
    let mut cost = Money::default();
    for component in [
        Money::from_per_million(&pricing.input_per_million_usd, request.regular_input_tokens)?,
        Money::from_per_million(
            &pricing.cached_input_per_million_usd,
            request.cached_input_tokens,
        )?,
        Money::from_per_million(
            &pricing.cache_write_per_million_usd,
            request.cache_write_tokens,
        )?,
        Money::from_per_million(
            &pricing.output_per_million_usd,
            request.regular_output_tokens,
        )?,
        Money::from_per_million(
            &pricing.reasoning_output_per_million_usd,
            request.reasoning_output_tokens,
        )?,
        Money::parse_fee(&pricing.request_fee_usd)?,
    ] {
        cost = cost.checked_add(component)?;
    }

    let mut tool_calls = 0_u64;
    for fee in &request.tool_fees {
        tool_calls = tool_calls
            .checked_add(fee.calls)
            .ok_or_else(|| anyhow!("tool-call count overflows"))?;
        cost =
            cost.checked_add(Money::parse_fee(&fee.fee_per_call_usd)?.checked_mul(fee.calls)?)?;
    }

    Ok((
        cost,
        TokenTotals {
            regular_input: request.regular_input_tokens,
            cached_input: request.cached_input_tokens,
            cache_write: request.cache_write_tokens,
            regular_output: request.regular_output_tokens,
            reasoning_output: request.reasoning_output_tokens,
        },
        tool_calls,
    ))
}

fn add_request_to_accumulator(
    accumulator: &mut StageAccumulator,
    cost: Money,
    tokens: TokenTotals,
    tool_calls: u64,
) -> Result<()> {
    accumulator.request_count = accumulator
        .request_count
        .checked_add(1)
        .ok_or_else(|| anyhow!("request count overflows"))?;
    accumulator.tokens = add_token_totals(accumulator.tokens, tokens)?;
    accumulator.tool_calls = accumulator
        .tool_calls
        .checked_add(tool_calls)
        .ok_or_else(|| anyhow!("tool-call count overflows"))?;
    accumulator.cost = accumulator.cost.checked_add(cost)?;
    Ok(())
}

fn add_token_totals(left: TokenTotals, right: TokenTotals) -> Result<TokenTotals> {
    Ok(TokenTotals {
        regular_input: left
            .regular_input
            .checked_add(right.regular_input)
            .ok_or_else(|| anyhow!("regular-input-token count overflows"))?,
        cached_input: left
            .cached_input
            .checked_add(right.cached_input)
            .ok_or_else(|| anyhow!("cached-input-token count overflows"))?,
        cache_write: left
            .cache_write
            .checked_add(right.cache_write)
            .ok_or_else(|| anyhow!("cache-write-token count overflows"))?,
        regular_output: left
            .regular_output
            .checked_add(right.regular_output)
            .ok_or_else(|| anyhow!("regular-output-token count overflows"))?,
        reasoning_output: left
            .reasoning_output
            .checked_add(right.reasoning_output)
            .ok_or_else(|| anyhow!("reasoning-output-token count overflows"))?,
    })
}

fn stage_summary(accumulator: StageAccumulator) -> StageCostSummary {
    StageCostSummary {
        request_count: accumulator.request_count,
        tokens: accumulator.tokens,
        tool_calls: accumulator.tool_calls,
        total_cost_usd: accumulator.cost.decimal_string(),
    }
}

fn ratio_cost(total: Money, divisor: u64) -> Option<RatioCost> {
    if divisor == 0 {
        return None;
    }
    let denominator = i128::from(divisor).checked_mul(MONEY_FACTOR)?;
    let common = gcd_i128(total.atto_usd, denominator);
    let numerator = total.atto_usd / common;
    let denominator = denominator / common;
    let exact_fraction_usd = format!("{numerator}/{denominator}");
    if let Some(decimal_usd) = finite_decimal(numerator, denominator) {
        Some(RatioCost {
            decimal_usd,
            exact: true,
            exact_fraction_usd,
        })
    } else {
        Some(RatioCost {
            decimal_usd: rounded_decimal(numerator, denominator, MONEY_SCALE),
            exact: false,
            exact_fraction_usd,
        })
    }
}

fn gcd_i128(mut left: i128, mut right: i128) -> i128 {
    while right != 0 {
        let remainder = left % right;
        left = right;
        right = remainder;
    }
    left.abs().max(1)
}

fn finite_decimal(numerator: i128, denominator: i128) -> Option<String> {
    let mut reduced_denominator = denominator;
    while reduced_denominator % 2 == 0 {
        reduced_denominator /= 2;
    }
    while reduced_denominator % 5 == 0 {
        reduced_denominator /= 5;
    }
    if reduced_denominator != 1 {
        return None;
    }

    let whole = numerator / denominator;
    let mut remainder = numerator % denominator;
    if remainder == 0 {
        return Some(whole.to_string());
    }
    let mut digits = String::new();
    while remainder != 0 {
        remainder = remainder.checked_mul(10)?;
        let digit = remainder / denominator;
        digits.push(char::from(b'0' + u8::try_from(digit).ok()?));
        remainder %= denominator;
        if digits.len() > 128 {
            return None;
        }
    }
    while digits.ends_with('0') {
        digits.pop();
    }
    Some(format!("{whole}.{digits}"))
}

fn rounded_decimal(numerator: i128, denominator: i128, places: u32) -> String {
    let whole = numerator / denominator;
    let mut remainder = numerator % denominator;
    let mut digits = Vec::with_capacity(places as usize + 1);
    for _ in 0..=places {
        remainder *= 10;
        digits.push((remainder / denominator) as u8);
        remainder %= denominator;
    }
    let round_up = digits.pop().is_some_and(|digit| digit >= 5);
    if round_up {
        let mut index = digits.len();
        while index > 0 {
            index -= 1;
            if digits[index] < 9 {
                digits[index] += 1;
                break;
            }
            digits[index] = 0;
        }
        if index == 0 && digits.first() == Some(&0) {
            return format!("{}.{:0width$}", whole + 1, 0, width = places as usize);
        }
    }
    let fraction: String = digits
        .into_iter()
        .map(|digit| char::from(b'0' + digit))
        .collect();
    format!("{whole}.{fraction}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn fixture_check_cli_requires_and_preserves_explicit_repository_root() {
        let cli = Cli::try_parse_from([
            "ca-reasoning-artifact-experiment",
            "fixture-check",
            "--repo",
            "/tmp/reasoning-fixtures",
            "--experiment-dir",
            "/tmp/reasoning-experiment",
        ])
        .expect("fixture-check should accept an explicit repository root");

        match cli.command {
            Command::FixtureCheck {
                repo,
                experiment_dir,
            } => {
                assert_eq!(repo, PathBuf::from("/tmp/reasoning-fixtures"));
                assert_eq!(
                    experiment_dir,
                    Some(PathBuf::from("/tmp/reasoning-experiment"))
                );
            }
            _ => panic!("fixture-check parsed as the wrong command"),
        }

        assert!(
            Cli::try_parse_from(["ca-reasoning-artifact-experiment", "fixture-check"]).is_err()
        );
    }

    #[test]
    fn preregister_cli_requires_all_offline_identities() {
        let cli = Cli::try_parse_from([
            "ca-reasoning-artifact-experiment",
            "preregister",
            "--repo",
            "/tmp/repo",
            "--experiment-dir",
            "/tmp/experiment",
            "--worker",
            "/tmp/ca",
        ])
        .expect("preregister should accept exact offline identities");
        match cli.command {
            Command::Preregister {
                repo,
                experiment_dir,
                worker,
            } => {
                assert_eq!(repo, PathBuf::from("/tmp/repo"));
                assert_eq!(experiment_dir, PathBuf::from("/tmp/experiment"));
                assert_eq!(worker, PathBuf::from("/tmp/ca"));
            }
            _ => panic!("preregister parsed as the wrong command"),
        }
        assert!(Cli::try_parse_from([
            "ca-reasoning-artifact-experiment",
            "preregister",
            "--repo",
            "/tmp/repo",
        ])
        .is_err());
    }

    #[test]
    fn pilot_status_commands_require_only_a_private_experiment_directory() {
        let report = Cli::try_parse_from([
            "ca-reasoning-artifact-experiment",
            "report",
            "--experiment-dir",
            "/tmp/experiment",
        ])
        .unwrap();
        assert!(matches!(
            report.command,
            Command::Report { experiment_dir } if experiment_dir == Path::new("/tmp/experiment")
        ));

        let resume = Cli::try_parse_from([
            "ca-reasoning-artifact-experiment",
            "resume",
            "--experiment-dir",
            "/tmp/experiment",
        ])
        .unwrap();
        assert!(matches!(
            resume.command,
            Command::Resume { experiment_dir } if experiment_dir == Path::new("/tmp/experiment")
        ));

        let run = Cli::try_parse_from([
            "ca-reasoning-artifact-experiment",
            "run",
            "--experiment-dir",
            "/tmp/experiment",
            "--authorize-max-usd",
            "50.00",
            "--attest-provider-project-budget-usd",
            "50.00",
        ])
        .unwrap();
        assert!(matches!(
            run.command,
            Command::Run {
                experiment_dir,
                authorize_max_usd,
                attest_provider_project_budget_usd,
            }
                if experiment_dir == Path::new("/tmp/experiment")
                    && authorize_max_usd == "50.00"
                    && attest_provider_project_budget_usd == "50.00"
        ));
        assert!(Cli::try_parse_from([
            "ca-reasoning-artifact-experiment",
            "run",
            "--experiment-dir",
            "/tmp/experiment",
        ])
        .is_err());
    }

    #[test]
    fn fixture_candidate_is_hidden_but_requires_all_exact_identities() {
        let mut command = Cli::command();
        let help = command.render_long_help().to_string();
        assert!(!help.contains("fixture-candidate"));
        assert!(Cli::try_parse_from([
            "ca-reasoning-artifact-experiment",
            "fixture-candidate",
            "--repo",
            "/tmp/repo",
            "--fixture-id",
            "unicode_parser_boundary",
            "--kind",
            "baseline",
            "--workspace",
            "/tmp/workspace",
        ])
        .is_ok());
        assert!(Cli::try_parse_from([
            "ca-reasoning-artifact-experiment",
            "fixture-candidate",
            "--repo",
            "/tmp/repo",
        ])
        .is_err());
    }

    #[test]
    fn failed_fixture_receipt_forces_a_nonzero_dispatch_result() {
        ensure_fixture_check_passed(true).expect("a passing receipt should succeed");
        assert_eq!(
            ensure_fixture_check_passed(false)
                .expect_err("a failed receipt must fail the command")
                .to_string(),
            "fixture check failed"
        );
    }

    fn sha256(bytes: &[u8]) -> String {
        format!("{:x}", Sha256::digest(bytes))
    }

    fn requirement(id: &str, text: &str) -> RequirementItem {
        RequirementItem {
            id: id.to_string(),
            text: text.to_string(),
        }
    }

    fn valid_artifact() -> ReasoningArtifactV1 {
        ReasoningArtifactV1 {
            schema_version: ARTIFACT_SCHEMA_VERSION.to_string(),
            artifact_id: "artifact-001".to_string(),
            created_at: "2026-08-12T14:00:00Z".to_string(),
            planner: PlannerProvenance {
                provider: "fixture-provider".to_string(),
                model: "fixture-planner".to_string(),
                reasoning_effort: "high".to_string(),
            },
            task: TaskDefinition {
                objective: "Add a bounded, offline validation experiment.".to_string(),
                constraints: vec![requirement(
                    "constraint-1",
                    "Do not alter the normal runtime path.",
                )],
                success_criteria: vec![requirement(
                    "criterion-1",
                    "The focused test command succeeds.",
                )],
                non_goals: vec![requirement(
                    "non-goal-1",
                    "Do not transfer hidden model cognition.",
                )],
            },
            repo_checkpoint: RepositoryCheckpoint {
                repository_id: "ca".to_string(),
                base_revision: "0123456789abcdef".to_string(),
                task_input_sha256: "0".repeat(64),
                worktree_sha256: "1".repeat(64),
                path_digests: vec![PathDigest {
                    path: "tracked.txt".to_string(),
                    sha256: sha256(b"before"),
                }],
            },
            evidence: vec![EvidenceItem {
                id: "evidence-1".to_string(),
                kind: EvidenceKind::SourceObservation,
                observation: "The checkpointed file contains the baseline fixture.".to_string(),
                checkpoint_paths: vec!["tracked.txt".to_string()],
                source: "local repository inspection".to_string(),
            }],
            claims: vec![Claim {
                id: "claim-1".to_string(),
                statement: "The implementation can remain feature-gated.".to_string(),
                confidence: Confidence::High,
                evidence_ids: vec!["evidence-1".to_string()],
            }],
            uncertainties: vec![Uncertainty {
                id: "uncertainty-1".to_string(),
                statement: "Live model quality is not measured by this offline tool.".to_string(),
                impact: "A production routing decision cannot be made yet.".to_string(),
                resolution: "Run an authorized controlled benchmark later.".to_string(),
                evidence_ids: vec!["evidence-1".to_string()],
            }],
            plan: Plan {
                steps: vec![PlanStep {
                    id: "step-1".to_string(),
                    title: "Implement the isolated validator".to_string(),
                    instructions: "Edit only the feature-gated experiment binary.".to_string(),
                    depends_on: vec![],
                    evidence_ids: vec!["evidence-1".to_string()],
                    affected_paths: vec!["tracked.txt".to_string()],
                    created_paths: vec![],
                    validation: StepValidation {
                        commands: vec![
                            "cargo test --features planner-executor-experiment".to_string()
                        ],
                        expected_outcomes: vec!["All focused tests pass.".to_string()],
                    },
                }],
            },
            executor_contract: ExecutorContract {
                allowed_tools: vec![
                    ToolCapability::ReadRepository,
                    ToolCapability::SearchRepository,
                    ToolCapability::EditRepository,
                    ToolCapability::RunApprovedCommands,
                ],
                allowed_paths: vec!["tracked.txt".to_string()],
                forbidden_actions: vec!["Do not access credentials or the network.".to_string()],
                budgets: ExecutorBudgets {
                    max_turns: 20,
                    max_requests: 20,
                    max_input_tokens: 200_000,
                    max_output_tokens: 40_000,
                    max_tool_calls: 100,
                    max_wall_time_seconds: 3_600,
                    max_cost_usd: "5.00".to_string(),
                },
                stop_conditions: vec!["Stop after all required gates pass.".to_string()],
                escalation_triggers: vec![
                    "Escalate when checkpoint dependencies are stale.".to_string()
                ],
            },
            verifier: VerifierContract {
                gates: vec![VerifierGate {
                    id: "gate-1".to_string(),
                    criterion_ids: vec!["criterion-1".to_string()],
                    step_ids: vec!["step-1".to_string()],
                    command: "cargo test --features planner-executor-experiment".to_string(),
                    expected_outcome: "The command exits successfully.".to_string(),
                    required: true,
                }],
                escalation_policy: EscalationPolicy {
                    max_escalations: 1,
                    on_required_gate_failure: GateFailureAction::Escalate,
                    on_stale_checkpoint: StaleCheckpointAction::RefreshPlan,
                    planner_refresh_allowed: true,
                },
            },
        }
    }

    fn zero_pricing(request_fee_usd: &str) -> RequestPricing {
        RequestPricing {
            price_card_id: "fixture-price-card-2026-08-12".to_string(),
            price_card_effective_at: "2026-08-12T00:00:00Z".to_string(),
            billing_tier: "standard".to_string(),
            input_per_million_usd: "0".to_string(),
            cached_input_per_million_usd: "0".to_string(),
            cache_write_per_million_usd: "0".to_string(),
            output_per_million_usd: "0".to_string(),
            reasoning_output_per_million_usd: "0".to_string(),
            request_fee_usd: request_fee_usd.to_string(),
        }
    }

    fn fee_only_request(stage: Stage, fee: &str) -> CostedRequest {
        CostedRequest {
            stage,
            provider: "fixture-provider".to_string(),
            model: "fixture-model".to_string(),
            regular_input_tokens: 0,
            cached_input_tokens: 0,
            cache_write_tokens: 0,
            regular_output_tokens: 0,
            reasoning_output_tokens: 0,
            pricing: zero_pricing(fee),
            tool_fees: vec![],
        }
    }

    #[test]
    fn valid_artifact_passes_semantic_validation() {
        validate_artifact(&valid_artifact()).expect("valid artifact should pass");
    }

    #[test]
    fn artifact_digest_is_stable_across_round_trip_and_changes_with_content() {
        let artifact = valid_artifact();
        let digest = artifact_sha256(&artifact).unwrap();
        let round_trip: ReasoningArtifactV1 =
            serde_json::from_slice(&serde_json::to_vec_pretty(&artifact).unwrap()).unwrap();
        assert_eq!(artifact_sha256(&round_trip).unwrap(), digest);

        let mut changed = artifact;
        changed.task.objective.push_str(" Changed.");
        assert_ne!(artifact_sha256(&changed).unwrap(), digest);
    }

    #[test]
    fn schema_is_closed_at_the_root() {
        let schema = serde_json::to_value(schema_for!(ReasoningArtifactV1)).unwrap();
        assert_eq!(schema["additionalProperties"], false);
    }

    #[test]
    fn schema_closes_every_object_definition() {
        let schema = serde_json::to_value(schema_for!(ReasoningArtifactV1)).unwrap();
        let definitions = schema["definitions"].as_object().unwrap();
        for (name, definition) in definitions {
            if definition["type"] == "object" {
                assert_eq!(
                    definition["additionalProperties"], false,
                    "definition {name} must reject unknown fields"
                );
            }
        }
    }

    #[test]
    fn strict_json_rejects_opaque_provider_reasoning_fields() {
        let mut value = serde_json::to_value(valid_artifact()).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .insert("encrypted_content".to_string(), serde_json::json!("opaque"));
        let error = serde_json::from_value::<ReasoningArtifactV1>(value).unwrap_err();
        assert!(error.to_string().contains("unknown field"));
    }

    #[test]
    fn semantic_validation_rejects_secrets_unsafe_paths_cycles_missing_refs_and_bounds() {
        let mut secret = valid_artifact();
        secret.task.objective = "Use sk-proj-abcdefghijklmnop for the request".to_string();
        assert!(validate_artifact(&secret)
            .unwrap_err()
            .iter()
            .any(|error| error.contains("credential or secret")));

        for unsafe_path in ["/etc/passwd", "../outside", "src/../outside", "C:/outside"] {
            let mut artifact = valid_artifact();
            artifact.repo_checkpoint.path_digests[0].path = unsafe_path.to_string();
            assert!(validate_artifact(&artifact)
                .unwrap_err()
                .iter()
                .any(|error| error.contains("unsafe")));
        }

        let mut missing_reference = valid_artifact();
        missing_reference.claims[0].evidence_ids = vec!["evidence-missing".to_string()];
        assert!(validate_artifact(&missing_reference)
            .unwrap_err()
            .iter()
            .any(|error| error.contains("missing reference")));

        let mut cyclic = valid_artifact();
        cyclic.plan.steps.push(PlanStep {
            id: "step-2".to_string(),
            title: "Second step".to_string(),
            instructions: "Complete the second step.".to_string(),
            depends_on: vec!["step-1".to_string()],
            evidence_ids: vec!["evidence-1".to_string()],
            affected_paths: vec!["Cargo.toml".to_string()],
            created_paths: vec!["Cargo.toml".to_string()],
            validation: StepValidation {
                commands: vec!["cargo check".to_string()],
                expected_outcomes: vec!["The check succeeds.".to_string()],
            },
        });
        cyclic.plan.steps[0].depends_on = vec!["step-2".to_string()];
        assert!(validate_artifact(&cyclic)
            .unwrap_err()
            .iter()
            .any(|error| error.contains("contains a cycle")));

        let mut oversized = valid_artifact();
        oversized.task.objective = "x".repeat(MAX_TEXT_BYTES + 1);
        assert!(validate_artifact(&oversized)
            .unwrap_err()
            .iter()
            .any(|error| error.contains("maximum")));

        let mut outside_authority = valid_artifact();
        outside_authority.plan.steps[0]
            .affected_paths
            .push("src/main.rs".to_string());
        assert!(validate_artifact(&outside_authority)
            .unwrap_err()
            .iter()
            .any(|error| error.contains("outside executor_contract.allowed_paths")));

        let mut uncovered = valid_artifact();
        uncovered.verifier.gates[0].required = false;
        let errors = validate_artifact(&uncovered).unwrap_err();
        assert!(errors
            .iter()
            .any(|error| error.contains("not covered by a required verifier gate")));
    }

    #[test]
    fn checkpoint_verification_detects_staleness() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("tracked.txt"), b"before").unwrap();
        let artifact = valid_artifact();

        verify_repository_state(directory.path(), &artifact)
            .expect("matching checkpoint should pass");
        fs::write(directory.path().join("tracked.txt"), b"after").unwrap();
        let error = verify_repository_state(directory.path(), &artifact).unwrap_err();
        assert!(error.to_string().contains("dependency checkpoint is stale"));
        assert!(error.to_string().contains("digest mismatch"));
    }

    #[cfg(unix)]
    #[test]
    fn nofollow_symlink_errors_are_classified_by_platform() {
        assert!(is_nofollow_symlink_error(&io::Error::from_raw_os_error(
            libc::ELOOP
        )));
        #[cfg(target_os = "freebsd")]
        assert!(is_nofollow_symlink_error(&io::Error::from_raw_os_error(
            libc::EMLINK
        )));
    }

    #[cfg(unix)]
    #[test]
    fn checkpoint_verification_rejects_symlink_dependencies() {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("real.txt"), b"before").unwrap();
        symlink("real.txt", directory.path().join("tracked.txt")).unwrap();
        let error = verify_repository_state(directory.path(), &valid_artifact())
            .unwrap_err()
            .to_string();
        assert!(error.contains("dependency checkpoint is stale"));
        assert!(error.contains("symlink components are not allowed"));
    }

    #[cfg(unix)]
    #[test]
    fn checkpoint_verification_rejects_fifos_without_blocking() {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;

        let directory = tempfile::tempdir().unwrap();
        let fifo_path = directory.path().join("tracked.txt");
        let fifo = CString::new(fifo_path.as_os_str().as_bytes()).unwrap();
        // SAFETY: the path is a valid NUL-terminated temporary pathname.
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        let error = verify_repository_state(directory.path(), &valid_artifact())
            .unwrap_err()
            .to_string();
        assert!(error.contains("checkpoint dependency is not a regular file"));
    }

    #[test]
    fn plan_targets_require_checkpoint_or_explicit_create_intent() {
        let mut uncovered = valid_artifact();
        uncovered.plan.steps[0].affected_paths = vec!["untracked.txt".to_string()];
        uncovered.executor_contract.allowed_paths = vec!["untracked.txt".to_string()];
        let errors = validate_artifact(&uncovered).unwrap_err();
        assert!(errors.iter().any(|error| {
            error.contains("must be exactly one of a checkpointed existing path")
        }));

        uncovered.plan.steps[0].created_paths = vec!["untracked.txt".to_string()];
        validate_artifact(&uncovered).expect("an explicit create target should be valid");

        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("tracked.txt"), b"before").unwrap();
        verify_repository_state(directory.path(), &uncovered)
            .expect("an absent create target should pass");
        fs::write(directory.path().join("untracked.txt"), b"already here").unwrap();
        let error = verify_repository_state(directory.path(), &uncovered)
            .unwrap_err()
            .to_string();
        assert!(error.contains("path already exists"));
    }

    #[test]
    fn created_paths_have_one_creator_and_flow_only_to_dependent_steps() {
        let mut artifact = valid_artifact();
        artifact.plan.steps[0].affected_paths = vec!["new.txt".to_string()];
        artifact.plan.steps[0].created_paths = vec!["new.txt".to_string()];
        artifact.executor_contract.allowed_paths = vec!["new.txt".to_string()];
        artifact.plan.steps.push(PlanStep {
            id: "step-2".to_string(),
            title: "Refine the created file".to_string(),
            instructions: "Modify the file created by step one.".to_string(),
            depends_on: vec!["step-1".to_string()],
            evidence_ids: vec!["evidence-1".to_string()],
            affected_paths: vec!["new.txt".to_string()],
            created_paths: vec![],
            validation: StepValidation {
                commands: vec!["cargo check".to_string()],
                expected_outcomes: vec!["The check succeeds.".to_string()],
            },
        });
        artifact.verifier.gates[0]
            .step_ids
            .push("step-2".to_string());
        validate_artifact(&artifact).expect("a dependent step may refine a created path");

        let mut parallel = artifact.clone();
        parallel.plan.steps[1].depends_on.clear();
        assert!(validate_artifact(&parallel)
            .unwrap_err()
            .iter()
            .any(|error| error.contains("created path from this step or its dependencies")));

        let mut duplicate = artifact;
        duplicate.plan.steps[1].created_paths = vec!["new.txt".to_string()];
        assert!(validate_artifact(&duplicate)
            .unwrap_err()
            .iter()
            .any(|error| error.contains("declared by both plan steps")));
    }

    #[test]
    fn plan_operations_require_matching_executor_capabilities() {
        let mut no_edit = valid_artifact();
        no_edit
            .executor_contract
            .allowed_tools
            .retain(|tool| *tool != ToolCapability::EditRepository);
        assert!(validate_artifact(&no_edit)
            .unwrap_err()
            .iter()
            .any(|error| error.contains("omits edit_repository")));

        let mut no_commands = valid_artifact();
        no_commands
            .executor_contract
            .allowed_tools
            .retain(|tool| *tool != ToolCapability::RunApprovedCommands);
        assert!(validate_artifact(&no_commands)
            .unwrap_err()
            .iter()
            .any(|error| error.contains("omits run_approved_commands")));
    }

    #[test]
    fn checkpoint_verification_has_an_aggregate_byte_budget() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("tracked.txt"), b"abc").unwrap();
        fs::write(directory.path().join("second.txt"), b"def").unwrap();
        let mut artifact = valid_artifact();
        artifact.repo_checkpoint.path_digests.push(PathDigest {
            path: "second.txt".to_string(),
            sha256: sha256(b"def"),
        });

        let error = verify_repository_state_with_budget(directory.path(), &artifact, 5)
            .unwrap_err()
            .to_string();
        assert!(error.contains("aggregate verification byte budget"));
    }

    #[test]
    fn paths_and_commands_reject_control_characters() {
        let mut artifact = valid_artifact();
        artifact.plan.steps[0].affected_paths = vec!["tracked\n.txt".to_string()];
        assert!(validate_artifact(&artifact)
            .unwrap_err()
            .iter()
            .any(|error| error.contains("control character")));

        let mut artifact = valid_artifact();
        artifact.plan.steps[0].validation.commands = vec!["cargo test\nwhoami".to_string()];
        assert!(validate_artifact(&artifact)
            .unwrap_err()
            .iter()
            .any(|error| error.contains("one printable line")));
    }

    #[test]
    fn exact_cost_aggregation_accounts_for_every_category_and_stage() {
        let planner = CostedRequest {
            stage: Stage::Planner,
            provider: "fixture-provider".to_string(),
            model: "strong".to_string(),
            regular_input_tokens: 1_000_000,
            cached_input_tokens: 500_000,
            cache_write_tokens: 250_000,
            regular_output_tokens: 100_000,
            reasoning_output_tokens: 50_000,
            pricing: RequestPricing {
                price_card_id: "fixture-price-card-2026-08-12".to_string(),
                price_card_effective_at: "2026-08-12T00:00:00Z".to_string(),
                billing_tier: "standard".to_string(),
                input_per_million_usd: "1".to_string(),
                cached_input_per_million_usd: "0.5".to_string(),
                cache_write_per_million_usd: "2".to_string(),
                output_per_million_usd: "10".to_string(),
                reasoning_output_per_million_usd: "20".to_string(),
                request_fee_usd: "0.10".to_string(),
            },
            tool_fees: vec![ToolFee {
                name: "search".to_string(),
                calls: 2,
                fee_per_call_usd: "0.20".to_string(),
            }],
        };
        let runs = BenchmarkRuns {
            schema_version: RUNS_SCHEMA_VERSION.to_string(),
            currency: "USD".to_string(),
            experiment_id: "experiment-1".to_string(),
            arm_id: "planner-executor".to_string(),
            trials: vec![
                BenchmarkTrial {
                    id: "trial-1".to_string(),
                    case_id: "case-1".to_string(),
                    repetition: 0,
                    outcome: TrialOutcome::Success,
                    requests: vec![planner, fee_only_request(Stage::Executor, "0.75")],
                },
                BenchmarkTrial {
                    id: "trial-2".to_string(),
                    case_id: "case-2".to_string(),
                    repetition: 0,
                    outcome: TrialOutcome::Failure,
                    requests: vec![fee_only_request(Stage::Escalation, "1")],
                },
            ],
        };

        let summary = aggregate_cost(&runs).expect("cost fixture should be valid");
        assert_eq!(summary.total_cost_usd, "6");
        assert_eq!(summary.mean_cost_usd.decimal_usd, "3");
        assert!(summary.mean_cost_usd.exact);
        let per_success = summary.dollars_per_success.unwrap();
        assert_eq!(per_success.decimal_usd, "6");
        assert!(per_success.exact);
        assert_eq!(summary.stages.planner.total_cost_usd, "4.25");
        assert_eq!(summary.stages.executor.total_cost_usd, "0.75");
        assert_eq!(summary.stages.verifier.total_cost_usd, "0");
        assert_eq!(summary.stages.escalation.total_cost_usd, "1");
        assert_eq!(summary.tokens.regular_input, 1_000_000);
        assert_eq!(summary.tokens.cached_input, 500_000);
        assert_eq!(summary.tokens.cache_write, 250_000);
        assert_eq!(summary.tokens.regular_output, 100_000);
        assert_eq!(summary.tokens.reasoning_output, 50_000);
        assert_eq!(summary.tool_calls, 2);

        let mut unversioned = runs;
        unversioned.trials[0].requests[0]
            .pricing
            .price_card_effective_at = "undated".to_string();
        assert!(aggregate_cost(&unversioned)
            .unwrap_err()
            .iter()
            .any(|error| error.contains("price_card_effective_at")));
    }

    #[test]
    fn repeating_average_retains_an_exact_fraction() {
        let runs = BenchmarkRuns {
            schema_version: RUNS_SCHEMA_VERSION.to_string(),
            currency: "USD".to_string(),
            experiment_id: "experiment-1".to_string(),
            arm_id: "planner-executor".to_string(),
            trials: vec![
                BenchmarkTrial {
                    id: "trial-1".to_string(),
                    case_id: "case-1".to_string(),
                    repetition: 0,
                    outcome: TrialOutcome::Success,
                    requests: vec![fee_only_request(Stage::Planner, "1")],
                },
                BenchmarkTrial {
                    id: "trial-2".to_string(),
                    case_id: "case-2".to_string(),
                    repetition: 0,
                    outcome: TrialOutcome::Success,
                    requests: vec![fee_only_request(Stage::Planner, "0")],
                },
                BenchmarkTrial {
                    id: "trial-3".to_string(),
                    case_id: "case-3".to_string(),
                    repetition: 0,
                    outcome: TrialOutcome::Success,
                    requests: vec![fee_only_request(Stage::Planner, "0")],
                },
            ],
        };
        let summary = aggregate_cost(&runs).unwrap();
        assert_eq!(summary.mean_cost_usd.exact_fraction_usd, "1/3");
        assert!(!summary.mean_cost_usd.exact);
        assert_eq!(summary.mean_cost_usd.decimal_usd, "0.333333333333333333");
    }
}
