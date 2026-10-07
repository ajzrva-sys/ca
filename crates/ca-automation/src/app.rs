//! `codex-team` is intentionally more of a tiny local CI/orchestration engine
//! than a classic "agent loop."  The high-level rule is:
//!
//! - AI roles may propose intent or make bounded judgment calls.
//! - Rust owns state transitions, command validation, execution, evidence, role
//!   routing, and commits.
//! - Prose is advisory; typed structs and persisted JSON/Markdown state are the
//!   authority.
//!
//! App conductor for the `codex-team` orchestration engine.
//!
//! Owns the high-level cycle loop and delegates to lower-level subsystems as
//! they are split out. During modularization this file intentionally preserves
//! behavior first; later passes can move pure queue, guard, verification, and
//! role helpers behind narrower module APIs.
//!
//! Section map (search the banners below):
//! 1. Public CLI entrypoints — `init` / `step` / `run` / `status` / …
//! 2. Cycle core — `step_once*` phases and role call gates
//! 3. Worker path — snapshot / control-plane / guard / verify / accept / commit
//! 4. Queue recovery, promotion, and deterministic lanes
//! 5. Markdown queue status mutators (exact task-ID, single Status)
//! 6. Verification recipe parsing / allowlist helpers
//! 7. Terminal rendering and prompt-size utilities (presentation only)
//! 8. Filesystem guard and persistence helpers
//! 9. Prompt/schema seed writes into `.orchestrator/`
//!
//! Read when changing cycle routing, acceptance, or not-yet-extracted helpers.
//! Prefer `docs/AGENT_MAP.md` and subsystem modules before editing broadly here.

use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, NaiveDateTime, Utc};
use globset::{Glob, GlobSet};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs::{self, File};
use std::hash::{Hash, Hasher};
use std::io::{self, BufRead, BufReader, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use walkdir::WalkDir;

pub(crate) mod receipts;
mod support;

use receipts::{
    emit_json_receipt, new_cycle_id, CycleReceipt, DispositionReceipt, DoctorCheck, DoctorReceipt,
    EpisodeErrorKindCount, LearningCooldownReceipt, LearningSignalReceipt, LearningSnapshot,
    ProviderHintReceipt, StatusReceipt, PROTOCOL_VERSION,
};

use crate::ca::{CaProcessConfig, CaRoleAuthority, CaRunRequest, CaRunner, RealCaRunner};
use crate::cancellation::{install_process_signal_handlers, CancellationToken, MCP_JOB_ID_ENV};
use crate::codex::prompt_pack::{
    estimate_tokens, ContextPack, ContextPackBuilder, ContextPackSection, PromptReceipt,
    PromptSectionCategory, PromptSectionPriority,
};
use crate::codex::run::{
    CodexRoleConfig, CodexRunHooks, CodexRunRequest, CodexRunStart, CodexRunner, RealCodexRunner,
};
use crate::domain::{
    acceptance_contract_from_task, classify_tool_failure, compact_tool_excerpt,
    decide_active_regret_route, dedupe_positive_learnings, derive_status, score_route_lanes,
    suggest_fallback_provider, summarize_tool_cooldowns, task_route_profile, AcceptanceContract,
    AutomationLevel, DerivedStatus, DerivedStatusInput, Event, ExecutionLane,
    PositiveRouteLearning, ReviewTier, RiskLevel, RouteAttempt, RouteDecisionAction, RouteFeedback,
    RouteLifecycleStatus, RouteOutcome, RouteRegretKind, RouteRetryContext, RouteScorecard,
    TaskPolicy, TaskRouteProfile, TaskSpec, TokenUsage, ToolCooldownSummary, ToolFailureKind,
    ToolHealthRecord, ToolHealthStatus,
};
pub(crate) use crate::git::*;
use crate::lock::{acquire_repo_lock, acquire_repo_locks, RepoLockTarget};
use crate::paths::Paths;
use crate::prompts::{
    DEFAULT_AGENTS_MD, FOREMAN_BATCH_PROMPT, FOREMAN_BATCH_SCHEMA, FOREMAN_REPAIR_PROMPT,
    FOREMAN_REPAIR_SCHEMA, FOREMAN_REVIEW_PROMPT, FOREMAN_REVIEW_SCHEMA, MASTER_PROMPT,
    MASTER_SCHEMA, PLANNER_PROMPT, PLANNER_SCHEMA, WORKER_PROMPT, WORKER_SCHEMA,
};
use crate::queue::next_steps::{
    active_queue_task_ids, clean_markdown_value, clean_output_token, conditional_run_if_satisfied,
    heading_task_id, is_approved_queue_label, is_next_steps_task_section, markdown_field_value,
    markdown_list_after, next_steps_archive_marker, next_steps_completed_task_ids,
    next_steps_outputs, next_steps_state_from_text, next_steps_task_section_bounds,
    parse_approved_next_steps_tasks, parse_pending_next_steps_tasks,
    pending_task_dependencies_satisfied, strip_numbered_markdown_item, NextStepsOutputMap,
    NextStepsQueueState,
};
use crate::repair::{
    compiler_errors_from_text, failure_fingerprint_from_text, first_useful_output_line,
    patch_context_mismatch_count, FailureFingerprint, FailureFingerprintKind, WorkerRetryBrief,
};
use crate::verification::isolation::IsolationSession;
use crate::verification::recipe::{
    CommandKind, FailurePolicy, Recipe, RecipeSpec, VerificationCommand,
};
#[cfg(test)]
pub(crate) use crate::verification::runner::classify_deterministic_result;
use crate::verification::runner::{
    run_deterministic_command, run_isolated_deterministic_command,
    run_isolated_deterministic_command_in_session, DeterministicCommandResult,
    DeterministicResultCategory,
};
pub(crate) use crate::verification::{CommandEvidence, EvidenceReport, VerificationPlan};
use support::*;

// Presentation helpers (extracted from the former rendering section).
#[allow(unused_imports)]
use crate::presentation::{
    build_planner_queue_brief, display_path, format_count, format_duration, frame_rule,
    frame_width, friendly_command, is_codex_tool_stderr_detail, is_high_signal_success_output,
    is_live_start_command, is_non_actionable_stderr, is_rg_no_matches, is_swiftpm_command,
    is_swiftpm_sandbox_failure, mission_summary_rows, preview_text, print_codex_role_header,
    print_doctor_receipt, print_frame_row, print_role_header, print_status, render_jsonl_stream,
    render_project_manager_compact_prompt, render_prompt, render_prompt_limited, render_schema,
    render_stderr_stream, report_agent_role_warnings, report_role_tier_warnings,
    rg_no_matches_activity, role_label, role_strength_score, strip_ansi,
    summarize_codex_tool_stderr, truncate_center, wrap_terminal_text, PromptLimits, Theme,
};

// Re-export for modules that import this from `crate::app`.
pub(crate) use crate::presentation::truncate_inline;

pub(crate) use crate::machine_json::MachineJsonModeGuard;

use crate::task_import::{
    begin_apply_transaction, ensure_import_receipts_complete, persist_apply_receipt, prepare_apply,
    preview as preview_task_import, MutationBlockers,
};
use crate::telemetry::{
    append_event, append_model_call_record, fallback_prompt_tokens_for_role, format_usize,
    normalize_cost_role, persist_run_record, read_jsonl_file, read_model_call_records,
    read_orchestrator_events, sanitize_filename, CostRoleStats, ModelCallRecord,
    OrchestratorEventKind, RunRecord, RunState,
};

pub(crate) fn machine_json_mode() -> bool {
    crate::machine_json::enabled()
}

// Under `--json`, redirect human progress to stderr so stdout stays a single
// machine receipt object (Hermes / unattended supervisors parse stdout whole).
macro_rules! println {
    () => {{
        if machine_json_mode() {
            ::std::eprintln!()
        } else {
            ::std::println!()
        }
    }};
    ($($arg:tt)*) => {{
        if machine_json_mode() {
            ::std::eprintln!($($arg)*)
        } else {
            ::std::println!($($arg)*)
        }
    }};
}

use crate::config::{
    default_foreman_label, default_foreman_role, default_intern_label, default_master_label,
    default_planner_label, default_worker_label, CommitMode, Config, LiveOutputStyle, RoleConfig,
};
#[allow(unused_imports)] // re-exported for `app_tests` via `super::*`
pub(crate) use crate::config::{
    default_planner_role, default_recipe_harvest_recent_logs, default_worker_role, InternSupport,
    PlannerContextMode, RoleProvider,
};
pub(crate) use crate::state::{
    HumanDispositionDecision, HumanDispositionPause, SeniorBatchApproval, State,
};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct AdoptionScaffoldRecord {
    recorded_at: String,
    created_files: Vec<String>,
    note: String,
}

pub(crate) use crate::guard::report::GuardReport;
use crate::guard::{
    build_guard_report, compile_globs, diff_control_plane, diff_snapshots,
    expand_allowed_file_patterns, fingerprint_file, normalize_glob_pattern, snapshot_control_plane,
    snapshot_repo,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CeoOutput {
    project_complete: bool,
    #[serde(alias = "needs_foreman")]
    needs_planner: bool,
    roadmap_status: String,
    pub(crate) summary: String,
    warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ProjectManagerOutput {
    project_complete: bool,
    needs_master: bool,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    work_plan: PlannerWorkPlan,
    tasks: Vec<TaskSpec>,
    warnings: Vec<String>,
    #[serde(default)]
    pub(crate) commit_requests: Vec<RustCommitRequest>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PlannerWorkPlan {
    #[serde(default)]
    summary: String,
    #[serde(default)]
    goals: Vec<String>,
    #[serde(default)]
    task_sequence: Vec<PlannerWorkPlanItem>,
    #[serde(default)]
    test_strategy: Vec<String>,
    #[serde(default)]
    pm_return_triggers: Vec<String>,
}

impl PlannerWorkPlan {
    fn is_empty(&self) -> bool {
        self.summary.trim().is_empty()
            && self.goals.is_empty()
            && self.task_sequence.is_empty()
            && self.test_strategy.is_empty()
            && self.pm_return_triggers.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PlannerWorkPlanItem {
    id: String,
    pub(crate) title: String,
    kind: String,
    status: String,
    #[serde(default)]
    depends_on: Vec<String>,
    #[serde(default)]
    tests: Vec<String>,
    #[serde(default)]
    notes: String,
    #[serde(default)]
    estimated_minutes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WorkerOutput {
    // WorkerOutput is intentionally split:
    // - commands_run/tests_run are historical/debug claims from Worker.
    // - handoff_to_rust is the authoritative final verification request.
    // - orchestration_request is advisory: Worker can request a bounded queue
    //   action, but Rust validates and applies any state transition.
    // Rust still validates and may reject every handoff/directive command.
    task_id: String,
    status: String,
    summary: String,
    files_changed: Vec<String>,
    commands_run: Vec<String>,
    tests_run: Vec<String>,
    handoff_to_rust: Vec<VerificationCommand>,
    #[serde(default)]
    orchestration_request: Option<WorkerOrchestrationRequest>,
    #[serde(default)]
    blocker: String,
    confidence: String,
}

fn enforce_worker_claimed_file_evidence(paths: &Paths, mut worker: WorkerOutput) -> WorkerOutput {
    // Chat-only local providers (mlx_lm/ollama) can claim files_changed without
    // writing anything. Rust refuses `done` unless claimed paths exist on disk.
    if !worker.status.eq_ignore_ascii_case("done") || worker.files_changed.is_empty() {
        return worker;
    }
    let missing: Vec<String> = worker
        .files_changed
        .iter()
        .filter(|path| {
            let trimmed = path.trim().trim_matches('`');
            !trimmed.is_empty() && !paths.root.join(trimmed).is_file()
        })
        .cloned()
        .collect();
    if missing.is_empty() {
        return worker;
    }
    let summary = format!(
        "Worker claimed status `done` but missing claimed file(s): {}",
        missing.join(", ")
    );
    println!("[Guard] {summary}");
    worker.status = "blocked".to_string();
    worker.blocker = summary.clone();
    worker.summary = summary;
    worker.confidence = "high".to_string();
    worker
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WorkerOrchestrationRequest {
    kind: WorkerOrchestrationKind,
    reason: String,
    #[serde(default)]
    lease_paths: Vec<String>,
    #[serde(default)]
    child_tasks: Vec<WorkerProposedTask>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
enum WorkerOrchestrationKind {
    SplitTask,
    RetrySameTask,
    RouteToPlanner,
    ExpandLease,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WorkerProposedTask {
    title: String,
    objective: String,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    pub(crate) allowed_files: Vec<String>,
    #[serde(default)]
    read_only_files: Vec<String>,
    #[serde(default)]
    exact_steps: Vec<String>,
    #[serde(default)]
    success_criteria: Vec<String>,
    #[serde(default)]
    tests_to_run: Vec<String>,
    #[serde(default)]
    pub(crate) stop_conditions: Vec<String>,
    #[serde(default)]
    estimated_minutes: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DeterministicRecipe {
    // Harvested recipes are a memory of boring command chains that have already
    // worked.  They are candidates, not permissions; replay still passes through
    // the typed recipe/command allowlist.
    schema_version: u32,
    recipe_id: String,
    status: String,
    source_task_id: String,
    #[serde(default)]
    parent_task: Option<String>,
    title: String,
    kind: String,
    created_at: String,
    allowed_files: Vec<String>,
    read_only_files: Vec<String>,
    forbidden_actions: Vec<String>,
    commands: Vec<DeterministicRecipeCommand>,
    #[serde(default = "default_recipe_occurrences")]
    occurrences: usize,
    #[serde(default)]
    source_logs: Vec<String>,
    #[serde(default)]
    updated_at: Option<String>,
    evidence: String,
    rust_policy: String,
}

fn default_recipe_occurrences() -> usize {
    1
}

type DeterministicRecipeCommand = VerificationCommand;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SeniorDevReviewOutput {
    // Legacy review output still has commit/task fields for backward compatible
    // parsing, but Rust ignores task manufacturing and performs accepted-task
    // bookkeeping/commits itself.
    task_id: String,
    accepted: bool,
    #[serde(default)]
    reason_code: String,
    #[serde(default)]
    commit_created: bool,
    #[serde(default)]
    commit_sha: String,
    needs_master: bool,
    #[serde(default)]
    needs_planner: bool,
    project_complete: bool,
    summary: String,
    problems: Vec<String>,
    #[serde(default)]
    replacement_tasks: Vec<TaskSpec>,
    #[serde(default)]
    followup_tasks: Vec<TaskSpec>,
    #[serde(default)]
    warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SeniorRepairOutput {
    // Repair mode is not a normal code review.  It is a decision packet for what
    // Rust should do after mechanical retry is exhausted.
    task_id: String,
    decision: SeniorRepairDecision,
    reason_code: String,
    repair_brief: String,
    #[serde(default)]
    needs_planner: bool,
    #[serde(default)]
    needs_master: bool,
    summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SeniorBatchApprovalOutput {
    // Senior batch mode is a technical gate, not a review.  Senior cannot create
    // tasks, mark work done, or run git; Rust validates the returned task IDs.
    // Progress actions are the foreman lane: Senior may classify boring work for
    // Rust or one bounded Worker card, but Rust still executes/queues it.
    approved: bool,
    batch_id: String,
    task_ids: Vec<String>,
    reason_code: String,
    max_tasks: usize,
    allowed_kinds: Vec<String>,
    forbidden_scope: Vec<String>,
    requires_stop_on: Vec<String>,
    verification_required: Vec<String>,
    auto_accept_policy: String,
    needs_planner: bool,
    needs_master: bool,
    summary: String,
    warnings: Vec<String>,
    #[serde(default)]
    progress_actions: Vec<SeniorProgressAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SeniorProgressAction {
    action_id: String,
    kind: SeniorProgressActionKind,
    title: String,
    summary: String,
    #[serde(default)]
    pub(crate) commit_message: String,
    #[serde(default)]
    allowed_files: Vec<String>,
    #[serde(default)]
    read_only_files: Vec<String>,
    #[serde(default)]
    commands: Vec<String>,
    #[serde(default)]
    success_criteria: Vec<String>,
    #[serde(default)]
    stop_conditions: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
enum SeniorProgressActionKind {
    RustEvidenceJob,
    RustBookkeepingJob,
    RustCommitJob,
    WorkerEditTask,
    SeniorDecision,
    PmDirection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RustCommitRequest {
    pub(crate) request_id: String,
    pub(crate) title: String,
    pub(crate) summary: String,
    pub(crate) allowed_files: Vec<String>,
    pub(crate) verification_commands: Vec<String>,
    pub(crate) commit_message: String,
    pub(crate) stop_conditions: Vec<String>,
}

impl RustCommitRequest {
    fn from_senior_progress_action(action: &SeniorProgressAction) -> Self {
        Self {
            request_id: action.action_id.clone(),
            title: action.title.clone(),
            summary: action.summary.clone(),
            allowed_files: action.allowed_files.clone(),
            verification_commands: action.commands.clone(),
            commit_message: action.commit_message.clone(),
            stop_conditions: action.stop_conditions.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
enum SeniorRepairDecision {
    // Senior cannot accept/commit in repair mode.  It can only choose the next
    // bounded route, and Rust applies that route.
    RetryWorkerSameTask,
    PlannerDue,
    CeoDue,
    AbandonTask,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CompactReviewContextPack {
    // This is the "receipt, not the movie" packet.  Senior Dev should receive
    // enough evidence to make one decision without raw logs or broad roadmap text.
    task_id: String,
    mode: String,
    guard_status: String,
    changed_files: Vec<String>,
    guard_violations: Vec<String>,
    risk_level: String,
    diff_stat: String,
    verification_status: String,
    verification_table: Vec<String>,
    rejected_commands: Vec<String>,
    failure_fingerprint: Option<FailureFingerprint>,
    exact_question: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
enum RoleCallGate {
    // Role gates are currently recorded mostly as skip telemetry, but the type
    // names the policy: every expensive AI call needs a concrete reason.
    Allowed,
    Skipped { reason: RoleSkipReason },
    Blocked { reason: RoleSkipReason },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
enum RoleSkipReason {
    MechanicalRepairRetry,
    ExistingBacklogAvailable,
    RustPromotionAvailable,
    SeniorBatchActive,
    SeniorBatchApprovedWorkerContinuation,
    NoStrategicReason,
    GuardViolationNeedsPlanning,
    EnvironmentBlockerRecorded,
    RetryBudgetExhausted,
}

impl RoleSkipReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::MechanicalRepairRetry => "mechanical_repair_retry",
            Self::ExistingBacklogAvailable => "existing_backlog_available",
            Self::RustPromotionAvailable => "rust_promotion_available",
            Self::SeniorBatchActive => "senior_batch_active",
            Self::SeniorBatchApprovedWorkerContinuation => {
                "senior_batch_approved_worker_continuation"
            }
            Self::NoStrategicReason => "no_strategic_reason",
            Self::GuardViolationNeedsPlanning => "guard_violation_needs_planning",
            Self::EnvironmentBlockerRecorded => "environment_blocker_recorded",
            Self::RetryBudgetExhausted => "retry_budget_exhausted",
        }
    }
}

// ---------------------------------------------------------------------------
// Public CLI entrypoints
// ---------------------------------------------------------------------------
//
// Thin wrappers from `cli.rs`: acquire repo lock when mutating, load config/state,
// run one cycle or multi-cycle loop, and emit human or `--json` receipts.

pub fn init(
    repo: PathBuf,
    mission: String,
    commit_mode: Option<CommitMode>,
    require_clean_start: bool,
) -> Result<()> {
    let paths = Paths::new(&repo)?;
    let _repo_lock = acquire_repo_lock(&paths, "init")?;
    let config_preexisting = paths.config.exists();
    if config_preexisting {
        let cfg = load_config(&paths)?;
        if commit_mode.is_some_and(|requested| requested != cfg.commit_mode) {
            bail!(
                "existing config uses commit_mode = {:?}; init will not rewrite it",
                cfg.commit_mode
            );
        }
        if require_clean_start && !cfg.require_clean_start {
            bail!(
                "existing config has require_clean_start = false; edit it explicitly instead of rewriting through init"
            );
        }
    }
    init_repo(&paths, &mission)?;
    install_orchestrator_runtime_exclude(&paths)?;
    if !config_preexisting {
        let mut cfg = load_config(&paths)?;
        if let Some(commit_mode) = commit_mode {
            cfg.commit_mode = commit_mode;
        }
        cfg.require_clean_start = require_clean_start;
        save_config(&paths, &cfg)?;
    }
    println!("Initialized orchestrator in {}", paths.orch.display());
    Ok(())
}

pub fn commit_pending(
    repo: PathBuf,
    id: String,
    expected_digest: String,
    expected_revision: String,
    json_output: bool,
) -> Result<()> {
    let _json_guard = json_output.then(MachineJsonModeGuard::enter);
    let paths = Paths::new(&repo)?;
    let _repo_lock = acquire_repo_lock(&paths, "commit-pending")?;
    let cfg = load_config(&paths)?;
    if cfg.commit_mode != CommitMode::Manual {
        bail!("commit-pending requires commit_mode = \"manual\"");
    }
    let mut state = load_state(&paths)?;
    let recovered = reconcile_pending_commit_retry(
        &paths,
        &mut state,
        &id,
        &expected_digest,
        &expected_revision,
    )?;
    let sha = if let Some(sha) = recovered {
        sha
    } else {
        let revision = crate::revision::repository_revision(&paths)?;
        if revision != expected_revision {
            bail!("repository revision is stale; no files were staged or committed");
        }
        apply_pending_commit_for_revision(
            &paths,
            &mut state,
            &id,
            &expected_digest,
            &expected_revision,
        )?
    };
    save_state(&paths, &state).with_context(|| {
        format!(
            "Git commit {} succeeded, but orchestrator state could not be saved; recovery evidence remains at {}",
            truncate_inline(&sha, 12),
            paths.orch.join("pending-commit-recovery.json").display()
        )
    })?;
    clear_pending_commit_recovery(&paths);
    let payload = json!({
        "protocol_version": PROTOCOL_VERSION,
        "outcome": "committed",
        "repo": paths.root.display().to_string(),
        "pending_id": id,
        "commit_sha": sha.clone(),
        "revision": crate::revision::repository_revision(&paths)?,
    });
    if json_output {
        emit_json_receipt(&payload)?;
    } else {
        println!("Committed pending intent at {}", truncate_inline(&sha, 12));
    }
    Ok(())
}

/// Preview or digest-bound apply one strict externally-authored task. Preview
/// performs no business mutation. Apply holds the repository lock across
/// revalidation, state persistence, revision calculation, and receipt write.
pub fn task_import(
    repo: PathBuf,
    input_path: PathBuf,
    accept_digest: Option<String>,
    expected_revision: Option<String>,
    json_output: bool,
) -> Result<()> {
    let _json_guard = json_output.then(MachineJsonModeGuard::enter);
    let paths = Paths::new(&repo)?;
    let _repo_lock = acquire_repo_lock(&paths, "task-import")?;
    if !paths.config.exists() || !paths.state.exists() {
        bail!(
            "repository is not initialized; run `codex-team init --repo {}` first",
            paths.root.display()
        );
    }
    let input = read_task_import_input(&input_path)?;
    match (accept_digest, expected_revision) {
        (None, None) => {
            let receipt = preview_task_import(&paths, &input)?;
            if json_output {
                emit_json_receipt(&receipt)?;
            } else {
                println!("{}", serde_json::to_string_pretty(&receipt)?);
            }
        }
        (Some(accept_digest), Some(expected_revision)) => {
            let mut state = load_state(&paths)?;
            let before = state.clone();
            let plan = prepare_apply(
                &paths,
                &input,
                &accept_digest,
                &expected_revision,
                &state.queue,
                &state.imported_tasks,
                MutationBlockers {
                    active_pause: state.human_disposition_required.is_some(),
                    pending_commit: state.pending_commit.is_some(),
                },
            )?;
            if !plan.already_applied {
                begin_apply_transaction(&paths, &plan)?;
                state.queue.push(plan.task.clone());
                state
                    .imported_tasks
                    .insert(plan.record.task_id.clone(), plan.record.clone());
                // A digest-approved imported task is stronger than stale
                // adoption/planning flags and must execute without PM/CEO
                // reinterpretation.
                state.needs_reconciliation = false;
                state.planner_due = false;
                state.master_due = false;
                state.project_complete = false;
                save_state(&paths, &state)?;
            }
            let revision_after = crate::revision::repository_revision(&paths)?;
            let receipt = match persist_apply_receipt(&paths, &plan, revision_after) {
                Ok(receipt) => receipt,
                Err(error) if !plan.already_applied => {
                    // Only Rust-owned state changed before receipt persistence.
                    // Restore it without touching project files or Git state.
                    if let Err(rollback) = save_state(&paths, &before) {
                        return Err(error).context(format!(
                            "task import receipt failed and state rollback also failed: {rollback:#}"
                        ));
                    }
                    return Err(error).context("task import receipt failed; restored state");
                }
                Err(error) => return Err(error),
            };
            let _ = append_event(
                &paths,
                OrchestratorEventKind::StateTransitionApplied,
                Some(&receipt.task_id),
                json!({
                    "transition": "typed_task_import_applied",
                    "outcome": receipt.outcome,
                    "import_digest": receipt.import_digest,
                    "revision_after": receipt.revision_after,
                }),
            );
            if json_output {
                emit_json_receipt(&receipt)?;
            } else {
                println!("{}", serde_json::to_string_pretty(&receipt)?);
            }
        }
        _ => bail!(
            "task import apply requires both --accept-digest and --expected-revision; omit both for preview"
        ),
    }
    Ok(())
}

fn read_task_import_input(path: &Path) -> Result<Vec<u8>> {
    const MAX_BYTES: usize = 1024 * 1024;
    let mut bytes = Vec::new();
    if path == Path::new("-") {
        io::stdin()
            .take((MAX_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .context("read task import from stdin")?;
    } else {
        let metadata = fs::metadata(path)
            .with_context(|| format!("inspect task import input {}", path.display()))?;
        if !metadata.is_file() {
            bail!("task import input must be a regular file or - for stdin");
        }
        if metadata.len() > MAX_BYTES as u64 {
            bail!("task import input exceeds the 1 MiB limit");
        }
        File::open(path)
            .with_context(|| format!("open task import input {}", path.display()))?
            .take((MAX_BYTES + 1) as u64)
            .read_to_end(&mut bytes)?;
    }
    if bytes.len() > MAX_BYTES {
        bail!("task import input exceeds the 1 MiB limit");
    }
    Ok(bytes)
}

pub fn verify_builtin(repo: PathBuf, check: String, json_output: bool) -> Result<()> {
    let _json_guard = json_output.then(MachineJsonModeGuard::enter);
    let paths = Paths::new(&repo)?;
    let check = crate::verification::ca::BuiltinCheck::parse(&check)
        .ok_or_else(|| anyhow!("unknown built-in check `{check}`"))?;
    let receipt = crate::verification::ca::run_builtin_check(&paths, check)?;
    if json_output {
        emit_json_receipt(&receipt)?;
    } else {
        println!("{}", serde_json::to_string_pretty(&receipt)?);
    }
    if !receipt.passed {
        bail!("{}", receipt.summary);
    }
    Ok(())
}

pub fn verify_release_readiness(repo: PathBuf, json_output: bool) -> Result<()> {
    let _json_guard = json_output.then(MachineJsonModeGuard::enter);
    let receipt = crate::release_readiness::audit_release_readiness(&repo);
    if json_output {
        emit_json_receipt(&receipt)?;
    } else {
        println!("{}", serde_json::to_string_pretty(&receipt)?);
    }
    if let Some(exit) = crate::release_readiness::ReleaseReadinessExit::from_receipt(&receipt) {
        return Err(exit.into());
    }
    Ok(())
}

pub fn step(repo: PathBuf, json_output: bool) -> Result<()> {
    step_with_expected_revision(repo, json_output, None)
}

pub fn step_with_expected_revision(
    repo: PathBuf,
    json_output: bool,
    expected_revision: Option<String>,
) -> Result<()> {
    match step_inner(repo.clone(), json_output, expected_revision) {
        Ok(()) => Ok(()),
        Err(err) if json_output => {
            let _ = emit_step_error_receipt_best_effort(&repo, &err);
            Err(err)
        }
        Err(err) => Err(err),
    }
}

fn step_inner(repo: PathBuf, json_output: bool, expected_revision: Option<String>) -> Result<()> {
    install_process_signal_handlers()?;
    let _json_guard = json_output.then(MachineJsonModeGuard::enter);
    let paths = Paths::new(&repo)?;
    let _repo_lock = acquire_repo_lock(&paths, "step")?;
    if let Some(expected) = expected_revision.as_deref() {
        if expected.len() != 64
            || expected != expected.to_ascii_lowercase()
            || !expected.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            bail!("--expected-revision must be a 64-character lowercase SHA-256 digest");
        }
        let current = crate::revision::repository_revision(&paths)?;
        if current != expected {
            bail!("repository revision changed before step; no cycle mutation was performed");
        }
    }
    let mut cfg = load_config(&paths)?;
    cfg.validate_ca()?;
    if cfg.requires_verification_isolation() {
        crate::verification::isolation::require_isolation_backend()?;
    }
    ensure_codex_ready_if_needed(&cfg)?;
    ensure_git_ready(&paths, &cfg)?;
    let mut state = load_state(&paths)?;
    ensure_import_receipts_complete(&paths, &state.queue, &state.imported_tasks)?;
    ensure_no_pending_commit(&state)?;
    ensure_clean_start(&paths, &cfg)?;
    reopen_if_next_steps_unfinished(&paths, &mut state)?;
    settle_idle_escalation_if_no_active_work(&paths, &mut state)?;
    maybe_trip_local_provider_failure_budget(&paths, &cfg)?;
    let cycle_id = new_cycle_id();
    let provider_hint = advisory_provider_hint(&paths, &cfg, &state);
    if cfg.routing.allow_provider_override && cfg.roles.worker.provider != RoleProvider::Ca {
        apply_advisory_provider_override(&mut cfg, provider_hint.as_ref());
        // Cooldown-driven auto-routing: when the current worker provider's model
        // has an active cooldown advisory, automatically switch to the cheapest
        // available alternative provider for this cycle.
        let records = read_model_call_records(&paths).unwrap_or_default();
        let cooldowns = current_tool_cooldowns(&paths, &records);
        let current_provider = cfg.roles.worker.provider.as_str();
        let current_model = &cfg.roles.worker.model;
        let mut available = vec!["codex"];
        if cfg.ollama.enabled {
            available.push("ollama");
        }
        if cfg.mlx_lm.enabled {
            available.push("mlx_lm");
        }
        if let Some(fallback) =
            suggest_fallback_provider(current_provider, current_model, &cooldowns, &available)
        {
            eprintln!(
                "[routing] cooldown advisory for provider={current_provider} model={current_model} \
                 — auto-routing to fallback={fallback}"
            );
            match fallback.as_str() {
                "ollama" => cfg.roles.worker.provider = RoleProvider::Ollama,
                "mlx_lm" => cfg.roles.worker.provider = RoleProvider::MlxLm,
                "codex" => cfg.roles.worker.provider = RoleProvider::Codex,
                _ => {}
            }
        }
    }
    let outcome = if should_stop_for_no_work(&paths, &state)? {
        try_commit_control_doc_edits_with_log(
            &paths,
            &cfg,
            &mut state,
            "Update completed project control docs",
        );
        println!("[Team] Project is complete and no queued work remains. Nothing to run.");
        state.master_due = false;
        no_work_cycle_outcome(&state)
    } else {
        let should_continue = step_once(&paths, &cfg, &mut state)?;
        if state.pending_commit.is_some() || state.human_disposition_required.is_some() {
            "paused"
        } else if should_continue {
            "continue"
        } else {
            "stopped"
        }
    };
    append_event(
        &paths,
        OrchestratorEventKind::StateTransitionApplied,
        None,
        json!({
            "transition": "cycle_receipt",
            "cycle_id": cycle_id,
            "outcome": outcome,
        }),
    )?;
    save_state(&paths, &state)?;
    if json_output {
        let mut receipt = CycleReceipt::from_state(
            cycle_id.clone(),
            paths.root.display().to_string(),
            outcome,
            &state,
            paths.events.display().to_string(),
        );
        receipt.commit_mode = Some(cfg.commit_mode.as_str().to_string());
        receipt.require_clean_start = Some(cfg.require_clean_start);
        receipt.revision = crate::revision::repository_revision(&paths).ok();
        receipt.pause_fingerprint = state.human_disposition_required.as_ref().and_then(|pause| {
            crate::revision::human_disposition_pause_fingerprint(&paths, pause).ok()
        });
        receipt.provider_hint = provider_hint.clone();
        receipt.learning = Some(build_learning_snapshot(&paths, &state));
        emit_json_receipt(&receipt)?;
        let _ = append_supervisor_episode(&paths, &receipt);
    } else {
        print_status(&paths, &state);
    }
    let _ = clear_local_provider_failure_budget_on_success(&paths, outcome);
    Ok(())
}

fn emit_step_error_receipt_best_effort(repo: &Path, err: &anyhow::Error) -> Result<()> {
    let cycle_id = new_cycle_id();
    let paths = Paths::new(repo).ok();
    let state = paths.as_ref().and_then(|p| load_state(p).ok());
    let events_path = paths
        .as_ref()
        .map(|p| p.events.display().to_string())
        .unwrap_or_default();
    let internal_summary = format!("{err:#}");
    let kind = classify_step_error_kind(&internal_summary);
    let role = infer_role_from_error(&internal_summary);
    let run_artifact = infer_run_artifact_from_error(&internal_summary);
    let summary = sanitize_step_error_summary(kind, &internal_summary);
    let mut receipt = CycleReceipt::error(
        cycle_id.clone(),
        repo.display().to_string(),
        state.as_ref(),
        events_path,
        kind,
        &summary,
        role.clone(),
        run_artifact,
    );
    if let Some(paths) = paths.as_ref() {
        if let Some(state) = state.as_ref() {
            receipt.learning = Some(build_learning_snapshot(paths, state));
        }
        if let Ok(cfg) = load_config(paths) {
            receipt.commit_mode = Some(cfg.commit_mode.as_str().to_string());
            receipt.require_clean_start = Some(cfg.require_clean_start);
        }
        receipt.revision = crate::revision::repository_revision(paths).ok();
    }
    emit_json_receipt(&receipt)?;
    if let Some(paths) = paths.as_ref() {
        let _ = append_supervisor_episode(paths, &receipt);
        let _ = record_negative_learning_from_error(
            paths,
            state.as_ref(),
            kind,
            &summary,
            role.as_deref(),
        );
        let _ = record_local_provider_failure(paths, kind, &summary);
        let _ = append_event(
            paths,
            OrchestratorEventKind::StateTransitionApplied,
            None,
            json!({
                "transition": "cycle_receipt",
                "cycle_id": cycle_id,
                "outcome": "error",
                "error_kind": kind,
            }),
        );
    }
    Ok(())
}

fn classify_step_error_kind(summary: &str) -> &'static str {
    let lower = summary.to_ascii_lowercase();
    if lower.contains("failed to parse") || lower.contains("malformed") {
        "malformed_output"
    } else if lower.contains("timed out") || lower.contains("timeout") {
        "timeout"
    } else if (lower.contains("codex ") && lower.contains(" failed"))
        || lower.contains("role execution failed")
    {
        "provider_failed"
    } else if lower.contains("connection refused") || lower.contains("unreachable") {
        "provider_unreachable"
    } else if lower.contains("enabled is false") {
        "provider_disabled"
    } else if lower.contains("lock") {
        "lock_contention"
    } else {
        "error"
    }
}

fn sanitize_step_error_summary(kind: &str, internal_summary: &str) -> String {
    match kind {
        "malformed_output" => {
            "malformed structured role output; provider payload omitted from receipt".to_string()
        }
        "provider_failed" => {
            "role provider process failed; provider payload omitted from receipt".to_string()
        }
        "provider_unreachable" => "role provider was unreachable".to_string(),
        "provider_disabled" => "configured role provider is disabled".to_string(),
        "timeout" => "role or deterministic process timed out".to_string(),
        _ => truncate_inline(internal_summary, 500),
    }
}

fn infer_role_from_error(summary: &str) -> Option<String> {
    let lower = summary.to_ascii_lowercase();
    for role in [
        "worker",
        "planner",
        "project manager",
        "foreman",
        "senior",
        "master",
        "ceo",
        "ollama",
        "mlx_lm",
        "mlx",
    ] {
        if lower.contains(role) {
            return Some(role.to_string());
        }
    }
    None
}

fn infer_run_artifact_from_error(summary: &str) -> Option<String> {
    for token in summary.split_whitespace() {
        if token.contains(".orchestrator/runs/") || token.ends_with(".ollama.json") {
            return Some(
                token
                    .trim_matches(|c| c == '"' || c == '\'' || c == ',')
                    .to_string(),
            );
        }
    }
    None
}

fn should_skip_ai_planner_when_idle(paths: &Paths, state: &State) -> Result<bool> {
    if !state.queue.is_empty()
        || state.planner_due
        || state.master_due
        || state.needs_reconciliation
        || state.retry_worker_same_task.is_some()
        || state.senior_dev_repair_order.is_some()
        || state.senior_batch_approval.is_some()
        || state.human_disposition_required.is_some()
    {
        return Ok(false);
    }
    let next = read_next_steps_queue_state(paths);
    // Unfinished/approved active work still needs Senior/Planner paths.
    Ok(!next.unfinished && !next.approved)
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct LocalProviderFailureBudget {
    consecutive_malformed: u32,
    last_error_kind: String,
    last_summary: String,
}

fn local_provider_failure_budget_path(paths: &Paths) -> PathBuf {
    paths
        .observations
        .join("local_provider_failure_budget.json")
}

fn load_local_provider_failure_budget(paths: &Paths) -> LocalProviderFailureBudget {
    let path = local_provider_failure_budget_path(paths);
    read_text(&path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn save_local_provider_failure_budget(
    paths: &Paths,
    budget: &LocalProviderFailureBudget,
) -> Result<()> {
    fs::create_dir_all(&paths.observations)?;
    write_text(
        &local_provider_failure_budget_path(paths),
        &format!("{}\n", serde_json::to_string_pretty(budget)?),
    )
}

fn record_local_provider_failure(paths: &Paths, kind: &str, summary: &str) -> Result<()> {
    let mut budget = load_local_provider_failure_budget(paths);
    if kind == "malformed_output" || kind == "provider_unreachable" {
        budget.consecutive_malformed = budget.consecutive_malformed.saturating_add(1);
    } else {
        budget.consecutive_malformed = 0;
    }
    budget.last_error_kind = kind.to_string();
    budget.last_summary = summary.chars().take(500).collect();
    save_local_provider_failure_budget(paths, &budget)
}

fn clear_local_provider_failure_budget_on_success(paths: &Paths, outcome: &str) -> Result<()> {
    if matches!(outcome, "error") {
        return Ok(());
    }
    let mut budget = load_local_provider_failure_budget(paths);
    if budget.consecutive_malformed == 0 {
        return Ok(());
    }
    budget.consecutive_malformed = 0;
    budget.last_error_kind.clear();
    budget.last_summary.clear();
    save_local_provider_failure_budget(paths, &budget)
}

fn maybe_trip_local_provider_failure_budget(paths: &Paths, cfg: &Config) -> Result<()> {
    let uses_local = cfg.roles.any_role_uses_ollama() || cfg.roles.any_role_uses_mlx_lm();
    if !uses_local {
        return Ok(());
    }
    let budget = load_local_provider_failure_budget(paths);
    let max = cfg.routing.local_provider_failure_budget.max(1);
    if budget.consecutive_malformed >= max {
        bail!(
            "local provider failure budget tripped after {} consecutive malformed/unreachable failures (last={}): {}; fix provider/schema or raise [routing].local_provider_failure_budget",
            budget.consecutive_malformed,
            budget.last_error_kind,
            budget.last_summary
        );
    }
    Ok(())
}

fn append_supervisor_episode(paths: &Paths, receipt: &CycleReceipt) -> Result<()> {
    fs::create_dir_all(&paths.observations)?;
    let path = paths.observations.join("supervisor_episodes.jsonl");
    let episode = json!({
        "observed_at": Utc::now().to_rfc3339(),
        "cycle_id": receipt.cycle_id,
        "outcome": receipt.outcome,
        "error_kind": receipt.error_kind,
        "error_summary": receipt.error_summary,
        "role": receipt.role,
        "queue_len": receipt.queue_len,
        "provider_hint": receipt.provider_hint,
    });
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    writeln!(file, "{}", serde_json::to_string(&episode)?)?;
    Ok(())
}

fn supervisor_episodes_path(paths: &Paths) -> PathBuf {
    paths.observations.join("supervisor_episodes.jsonl")
}

fn tool_failure_from_supervisor_episode(
    error_kind: Option<&str>,
    summary: &str,
) -> ToolFailureKind {
    match error_kind {
        Some("malformed_output") => ToolFailureKind::MalformedOutput,
        Some("timeout") => ToolFailureKind::Timeout,
        Some("provider_unreachable") | Some("provider_disabled") => {
            ToolFailureKind::ModelUnavailable
        }
        Some("provider_failed") => ToolFailureKind::ProcessFailed,
        Some("lock_contention") => ToolFailureKind::ProcessFailed,
        _ => classify_tool_failure(
            None,
            error_kind.is_some_and(|kind| kind == "timeout"),
            summary,
        ),
    }
}

fn normalize_supervisor_episode_role(role: Option<&str>) -> String {
    match role.map(|value| value.to_ascii_lowercase()).as_deref() {
        Some("ollama") | Some("mlx") | Some("mlx_lm") => "worker".to_string(),
        Some("project manager") => "planner".to_string(),
        Some("senior") | Some("senior dev") | Some("foreman-review") => "foreman".to_string(),
        Some(other) => other.to_string(),
        None => "supervisor".to_string(),
    }
}

fn synthesize_tool_health_from_supervisor_episodes(paths: &Paths) -> Vec<ToolHealthRecord> {
    let path = supervisor_episodes_path(paths);
    if !path.exists() {
        return Vec::new();
    }
    let Ok(raw): Result<Vec<Value>, _> = read_jsonl_file(&path) else {
        return Vec::new();
    };
    raw.into_iter()
        .filter_map(|episode| {
            let outcome = episode.get("outcome")?.as_str()?;
            if outcome != "error" {
                return None;
            }
            let observed_at = episode
                .get("observed_at")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_string();
            let error_kind = episode
                .get("error_kind")
                .and_then(|value| value.as_str())
                .map(str::to_string);
            let summary = episode
                .get("error_summary")
                .and_then(|value| value.as_str())
                .unwrap_or_default();
            let role = normalize_supervisor_episode_role(
                episode.get("role").and_then(|value| value.as_str()),
            );
            let model = format!(
                "supervisor_episode:{}",
                error_kind.as_deref().unwrap_or("error")
            );
            let failure_kind = tool_failure_from_supervisor_episode(error_kind.as_deref(), summary);
            Some(ToolHealthRecord {
                observed_at,
                role,
                task_id: None,
                model,
                execution_engine: None,
                route_provider: None,
                status: ToolHealthStatus::Failure,
                failure_kind: Some(failure_kind),
                exit_code: None,
                timed_out: error_kind.as_deref() == Some("timeout"),
                excerpt: compact_tool_excerpt(summary, 500),
            })
        })
        .collect()
}

fn recent_episode_error_kind_counts(paths: &Paths, limit: usize) -> Vec<EpisodeErrorKindCount> {
    let path = supervisor_episodes_path(paths);
    if !path.exists() {
        return Vec::new();
    }
    let Ok(raw): Result<Vec<Value>, _> = read_jsonl_file(&path) else {
        return Vec::new();
    };
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for episode in raw.into_iter().rev().take(limit) {
        if episode.get("outcome").and_then(|v| v.as_str()) != Some("error") {
            continue;
        }
        let kind = episode
            .get("error_kind")
            .and_then(|v| v.as_str())
            .unwrap_or("error")
            .to_string();
        *counts.entry(kind).or_default() += 1;
    }
    let mut out = counts
        .into_iter()
        .map(|(error_kind, count)| EpisodeErrorKindCount { error_kind, count })
        .collect::<Vec<_>>();
    out.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| left.error_kind.cmp(&right.error_kind))
    });
    out
}

fn build_learning_snapshot(paths: &Paths, state: &State) -> LearningSnapshot {
    let catalog = crate::learning::catalog_summary(paths, state.queue.first()).unwrap_or_default();
    let records = read_model_call_records(paths).unwrap_or_default();
    let cooldowns = current_tool_cooldowns(paths, &records)
        .into_iter()
        .filter(|summary| summary.cooldown_advisory)
        .take(5)
        .map(|summary| LearningCooldownReceipt {
            role: summary.role,
            failure_kind: summary.failure_kind.as_str().to_string(),
            recent_failures: summary.recent_failures,
            cooldown_advisory: summary.cooldown_advisory,
        })
        .collect::<Vec<_>>();
    let recent_episode_error_kinds = recent_episode_error_kind_counts(paths, 32);
    let Some(task) = state.queue.first() else {
        return LearningSnapshot {
            task_id: None,
            recommended_lane: None,
            top_signals: Vec::new(),
            cooldowns,
            recent_episode_error_kinds,
            catalog_healthy: catalog.healthy,
            proposal_count: catalog.proposal_count,
            approved_matching_count: catalog.approved_matching_count,
            quarantine_count: catalog.quarantine_count,
            execution_count: catalog.execution_count,
            ai_calls_avoided: catalog.ai_calls_avoided,
            estimated_tokens_avoided: catalog.estimated_tokens_avoided,
            applicable_recipe_id: catalog.applicable_recipe_id,
            activation_state: catalog.activation_state,
        };
    };
    let record = route_scorecard_for_task(
        paths,
        task,
        ExecutionLane::SharedWorker,
        "learning_snapshot",
        None,
        0,
    );
    let top_signals = record
        .scorecard
        .signals
        .iter()
        .take(5)
        .map(|signal| LearningSignalReceipt {
            lane: route_lane_label(signal.lane).to_string(),
            weight: signal.weight,
            reason: signal.reason.clone(),
        })
        .collect();
    LearningSnapshot {
        task_id: Some(task.task_id.clone()),
        recommended_lane: Some(route_lane_label(record.scorecard.recommended_lane).to_string()),
        top_signals,
        cooldowns,
        recent_episode_error_kinds,
        catalog_healthy: catalog.healthy,
        proposal_count: catalog.proposal_count,
        approved_matching_count: catalog.approved_matching_count,
        quarantine_count: catalog.quarantine_count,
        execution_count: catalog.execution_count,
        ai_calls_avoided: catalog.ai_calls_avoided,
        estimated_tokens_avoided: catalog.estimated_tokens_avoided,
        applicable_recipe_id: catalog.applicable_recipe_id,
        activation_state: catalog.activation_state,
    }
}

fn record_negative_learning_from_error(
    paths: &Paths,
    state: Option<&State>,
    error_kind: &str,
    summary: &str,
    role: Option<&str>,
) -> Result<()> {
    let Some(state) = state else {
        return Ok(());
    };
    let Some(task) = state.queue.first() else {
        return Ok(());
    };
    let lane = route_lane_for_role(role.unwrap_or("worker"));
    record_positive_route_learning(
        paths,
        task,
        lane,
        "failed",
        &format!("{error_kind}: {summary}"),
    )
}

fn try_honor_scorecard_soft_gate(
    paths: &Paths,
    cfg: &Config,
    state: &mut State,
    task: &TaskSpec,
    scorecard_record: &RouteScorecardRecord,
) -> Result<Option<bool>> {
    if !cfg.routing.honor_scorecard_soft_gate {
        return Ok(None);
    }
    let recommended = scorecard_record.scorecard.recommended_lane;
    let records = read_model_call_records(paths).unwrap_or_default();
    let worker_cooldown = current_tool_cooldowns(paths, &records)
        .into_iter()
        .any(|summary| summary.cooldown_advisory && summary.role.eq_ignore_ascii_case("worker"));

    if matches!(
        recommended,
        ExecutionLane::RustEvidence | ExecutionLane::RustBookkeeping
    ) && is_deterministic_task_candidate(task)
    {
        let summary = format!(
            "scorecard soft gate skipped Worker for {}; recommended_lane={}",
            task.task_id,
            route_lane_label(recommended)
        );
        println!("[routing] {summary}");
        push_event(
            state,
            "routing",
            "scorecard_soft_gate_skip_worker",
            &summary,
        );
        let _ = append_event(
            paths,
            OrchestratorEventKind::StateTransitionApplied,
            Some(&task.task_id),
            json!({
                "transition": "scorecard_soft_gate_skip_worker",
                "recommended_lane": route_lane_label(recommended),
            }),
        );
        return Ok(Some(!state.project_complete));
    }

    if worker_cooldown && !matches!(recommended, ExecutionLane::SharedWorker) {
        let reason = format!(
            "Scorecard soft gate: Worker tool-health cooldown is advisory and recommended_lane is {}. Skipping Worker until human disposition.",
            route_lane_label(recommended)
        );
        state.human_disposition_required = Some(HumanDispositionPause {
            reason: reason.clone(),
            options: vec![
                "recheck after fixing provider/tool health".to_string(),
                "gate and continue with Worker anyway".to_string(),
                "preserve current pause".to_string(),
            ],
            source: "scorecard_soft_gate".to_string(),
        });
        push_event(state, "routing", "scorecard_soft_gate_pause", &reason);
        let _ = append_event(
            paths,
            OrchestratorEventKind::StateTransitionApplied,
            Some(&task.task_id),
            json!({
                "transition": "scorecard_soft_gate_pause",
                "recommended_lane": route_lane_label(recommended),
                "worker_cooldown": true,
            }),
        );
        println!("[routing] {reason}");
        return Ok(Some(false));
    }

    Ok(None)
}

fn advisory_provider_hint(
    paths: &Paths,
    cfg: &Config,
    state: &State,
) -> Option<ProviderHintReceipt> {
    let _ = paths;
    let task = state.queue.first()?;
    let uses_mlx = cfg.roles.any_role_uses_mlx_lm();
    let uses_ollama = cfg.roles.any_role_uses_ollama();
    let docs_only = !task.allowed_files.is_empty()
        && task.allowed_files.iter().all(|path| {
            let lower = path.to_ascii_lowercase();
            lower.ends_with(".md") || lower.starts_with("docs/")
        });
    // Prefer Rust deterministic work for docs-only verify/status envelopes.
    if docs_only && is_deterministic_task_candidate(task) {
        return Some(ProviderHintReceipt {
            recommended_provider: "rust".to_string(),
            reason: format!(
                "docs-only mechanical task {} — prefer Rust deterministic lane (advisory; config still authoritative)",
                task.task_id
            ),
        });
    }
    // Advisory only: suggest cheapest local provider for docs-only tasks when enabled.
    if docs_only && cfg.mlx_lm.enabled {
        return Some(ProviderHintReceipt {
            recommended_provider: "mlx_lm".to_string(),
            reason: format!(
                "docs-only task {} — prefer mlx_lm when server is healthy (advisory; config still authoritative)",
                task.task_id
            ),
        });
    }
    if docs_only && cfg.ollama.enabled && !uses_mlx {
        return Some(ProviderHintReceipt {
            recommended_provider: "ollama".to_string(),
            reason: format!(
                "docs-only task {} — prefer ollama when server is healthy (advisory)",
                task.task_id
            ),
        });
    }
    if uses_mlx {
        return Some(ProviderHintReceipt {
            recommended_provider: "mlx_lm".to_string(),
            reason: "config already routes one or more roles to mlx_lm".to_string(),
        });
    }
    if uses_ollama {
        return Some(ProviderHintReceipt {
            recommended_provider: "ollama".to_string(),
            reason: "config already routes one or more roles to ollama".to_string(),
        });
    }
    None
}

fn apply_advisory_provider_override(cfg: &mut Config, hint: Option<&ProviderHintReceipt>) {
    if cfg.roles.worker.provider == RoleProvider::Ca {
        // `ca` is an execution engine boundary, not a model-route suggestion.
        // Never substitute another engine for an explicitly ca-backed role.
        return;
    }
    let Some(hint) = hint else {
        return;
    };
    match hint.recommended_provider.as_str() {
        "mlx_lm" if cfg.mlx_lm.enabled => {
            cfg.roles.worker.provider = RoleProvider::MlxLm;
            println!(
                "[Team] [routing] allow_provider_override applied worker provider=mlx_lm for this cycle only"
            );
        }
        "ollama" if cfg.ollama.enabled => {
            cfg.roles.worker.provider = RoleProvider::Ollama;
            println!(
                "[Team] [routing] allow_provider_override applied worker provider=ollama for this cycle only"
            );
        }
        _ => {}
    }
}

pub fn providers_command(repo: PathBuf, json_output: bool) -> Result<()> {
    let _json_guard = json_output.then(MachineJsonModeGuard::enter);
    let paths = Paths::new(&repo)?;
    let _repo_lock = acquire_repo_lock(&paths, "providers")?;
    let cfg = if paths.config.exists() {
        load_config(&paths)?
    } else {
        Config::default()
    };
    let state = load_state(&paths).unwrap_or_default();
    let ollama = crate::providers::ollama::probe_ollama_health(&cfg.ollama);
    let mlx = crate::providers::mlx_lm::probe_mlx_lm_health(&cfg.mlx_lm);
    let payload = json!({
        "protocol_version": PROTOCOL_VERSION,
        "repo": paths.root.display().to_string(),
        "routing": {
            "allow_provider_override": cfg.routing.allow_provider_override,
            "local_provider_failure_budget": cfg.routing.local_provider_failure_budget,
            "honor_scorecard_soft_gate": cfg.routing.honor_scorecard_soft_gate,
        },
        "roles": {
            "master": { "provider": cfg.roles.master.provider.as_str(), "model": cfg.roles.master.model },
            "planner": { "provider": cfg.roles.planner.provider.as_str(), "model": cfg.roles.planner.model },
            "foreman": { "provider": cfg.roles.foreman.provider.as_str(), "model": cfg.roles.foreman.model },
            "worker": { "provider": cfg.roles.worker.provider.as_str(), "model": cfg.roles.worker.model },
        },
        "ollama": {
            "enabled": ollama.enabled,
            "reachable": ollama.reachable,
            "base_url": ollama.base_url,
            "detail": ollama.detail,
        },
        "mlx_lm": {
            "enabled": mlx.enabled,
            "reachable": mlx.reachable,
            "base_url": mlx.base_url,
            "detail": mlx.detail,
            "model_count": mlx.model_count,
        },
        "provider_hint": advisory_provider_hint(&paths, &cfg, &state),
    });
    if json_output {
        emit_json_receipt(&payload)?;
    } else {
        println!("{}", serde_json::to_string_pretty(&payload)?);
    }
    Ok(())
}

pub fn run_cycles(
    repo: PathBuf,
    max_cycles: Option<usize>,
    pause_on_escalation: bool,
    auto_continue: bool,
) -> Result<()> {
    install_process_signal_handlers()?;
    let paths = Paths::new(&repo)?;
    let _repo_lock = acquire_repo_lock(&paths, "run")?;
    let mut cfg = load_config(&paths)?;
    if let Some(n) = max_cycles {
        cfg.max_cycles = n;
    }
    if pause_on_escalation {
        cfg.auto_escalate_to_master = false;
    }
    ensure_auto_continue_allowed(&cfg, auto_continue)?;
    cfg.validate_ca()?;
    if cfg.requires_verification_isolation() {
        crate::verification::isolation::require_isolation_backend()?;
    }
    ensure_codex_ready_if_needed(&cfg)?;
    ensure_git_ready(&paths, &cfg)?;
    let mut state = load_state(&paths)?;
    ensure_import_receipts_complete(&paths, &state.queue, &state.imported_tasks)?;
    ensure_no_pending_commit(&state)?;
    ensure_clean_start(&paths, &cfg)?;
    let mut prompt_between_cycles = !auto_continue;
    println!("[Team] codex-team v{}", env!("CARGO_PKG_VERSION"));
    for cycle in 1..=cfg.max_cycles {
        reopen_if_next_steps_unfinished(&paths, &mut state)?;
        settle_idle_escalation_if_no_active_work(&paths, &mut state)?;
        if should_stop_for_no_work(&paths, &state)? {
            try_commit_control_doc_edits_with_log(
                &paths,
                &cfg,
                &mut state,
                "Update completed project control docs",
            );
            println!("[Team] Project is complete and no queued work remains. Stopping.");
            state.master_due = false;
            save_state(&paths, &state)?;
            break;
        }
        println!("\n[Team] Cycle {cycle}/{}", cfg.max_cycles);
        let should_continue = step_once(&paths, &cfg, &mut state)?;
        save_state(&paths, &state)?;
        print_status(&paths, &state);
        if state.pending_commit.is_some() {
            println!("[Team] Paused for explicit pending-commit approval.");
            break;
        }
        reopen_if_next_steps_unfinished(&paths, &mut state)?;
        settle_idle_escalation_if_no_active_work(&paths, &mut state)?;
        if should_stop_for_no_work(&paths, &state)? {
            try_commit_control_doc_edits_with_log(
                &paths,
                &cfg,
                &mut state,
                "Update completed project control docs",
            );
            println!("[Team] Project is complete and no queued work remains. Stopping.");
            state.master_due = false;
            save_state(&paths, &state)?;
            break;
        }

        let has_follow_up = should_continue
            || !state.queue.is_empty()
            || state.master_due
            || state.planner_due
            || state.needs_reconciliation
            || state.retry_worker_same_task.is_some()
            || state.senior_dev_repair_order.is_some()
            || state.senior_batch_approval.is_some();
        if !has_follow_up {
            println!("[Team] Pausing after escalation or empty queue.");
            break;
        }

        if prompt_between_cycles {
            match prompt_continue_after_cycle(cycle, &state)? {
                ContinueChoice::Once => {}
                ContinueChoice::Auto => {
                    ensure_auto_continue_allowed(&cfg, true)?;
                    prompt_between_cycles = false;
                    println!("[Team] Auto-continuing through remaining work.");
                }
                ContinueChoice::Stop => {
                    println!("[Team] Pausing before next job.");
                    break;
                }
            }
        }

        if !should_continue {
            if state.master_due && cfg.auto_escalate_to_master {
                println!("[Team] Escalating to CEO on the next cycle.");
                continue;
            }
            if state.planner_due {
                println!("[Team] Returning to Project Manager on the next cycle.");
                continue;
            }
            if let Some(task_id) = &state.retry_worker_same_task {
                println!("[Team] Retrying Worker task {task_id} on the next cycle.");
                continue;
            }
            if let Some(task_id) = &state.senior_dev_repair_order {
                println!(
                    "[Team] Requesting Senior Dev repair order for {task_id} on the next cycle."
                );
                continue;
            }
            if let Some(batch) = &state.senior_batch_approval {
                println!(
                    "[Team] Continuing Senior-approved batch {} on the next cycle.",
                    batch.batch_id
                );
                continue;
            }
            println!("[Team] Pausing after escalation or empty queue.");
            break;
        }
    }
    Ok(())
}

fn ensure_auto_continue_allowed(cfg: &Config, auto_continue: bool) -> Result<()> {
    if !auto_continue {
        return Ok(());
    }
    if cfg.commit_mode == CommitMode::Manual {
        bail!(
            "run --auto-continue is unavailable when commit_mode = \"manual\"; use one supervised step at a time"
        );
    }
    if cfg.roles.any_role_uses_ca() {
        bail!(
            "run --auto-continue is unavailable while any role uses provider = \"ca\"; use one supervised step at a time"
        );
    }
    Ok(())
}

pub fn status(repo: PathBuf, json_output: bool, observe: bool) -> Result<()> {
    let _json_guard = json_output.then(MachineJsonModeGuard::enter);
    let paths = Paths::new(&repo)?;
    if !paths.config.exists() && !paths.state.exists() {
        let revision = crate::revision::repository_revision(&paths)?;
        if json_output {
            return emit_json_receipt(&json!({
                "protocol_version": PROTOCOL_VERSION,
                "repo": paths.root.display().to_string(), "initialized": false,
                "orchestration_available": false, "observe_only": true,
                "revision": revision, "queue_len":0,
            }));
        }
        println!(
            "Repository {} is available for typed queries; orchestration is not initialized.",
            paths.root.display()
        );
        return Ok(());
    }
    let lock_label = if observe { "status-observe" } else { "status" };
    let _repo_lock = acquire_repo_lock(&paths, lock_label)?;
    let mut state = load_state(&paths)?;
    if !observe {
        reopen_if_next_steps_unfinished(&paths, &mut state)?;
        sync_state_queue_with_next_steps(&paths, &mut state);
        recover_approved_next_steps_work(&paths, &mut state)?;
        settle_idle_escalation_if_no_active_work(&paths, &mut state)?;
        save_state(&paths, &state)?;
    }
    if json_output {
        let cfg = load_config(&paths).unwrap_or_default();
        let mut receipt = StatusReceipt::from_state(
            paths.root.display().to_string(),
            &state,
            paths.events.display().to_string(),
            observe,
        );
        receipt.commit_mode = Some(cfg.commit_mode.as_str().to_string());
        receipt.require_clean_start = Some(cfg.require_clean_start);
        receipt.revision = crate::revision::repository_revision(&paths).ok();
        receipt.pause_fingerprint = state.human_disposition_required.as_ref().and_then(|pause| {
            crate::revision::human_disposition_pause_fingerprint(&paths, pause).ok()
        });
        receipt.provider_hint = advisory_provider_hint(&paths, &cfg, &state);
        receipt.learning = Some(build_learning_snapshot(&paths, &state));
        emit_json_receipt(&receipt)?;
        return Ok(());
    }
    print_status(&paths, &state);
    println!("\nQueue:");
    for task in &state.queue {
        println!("- {} [{}] {}", task.task_id, task.kind, task.title);
    }
    let non_ai_tasks =
        parse_approved_next_steps_tasks(&read_text_or_empty(&paths.next_steps_non_ai));
    if !non_ai_tasks.is_empty() {
        println!("\nNon-AI queue:");
        for task in &non_ai_tasks {
            println!("- {} [{}] {}", task.task_id, task.kind, task.title);
        }
    }
    Ok(())
}

pub fn set_mission(repo: PathBuf, mission: String) -> Result<()> {
    let mission = mission.trim();
    if mission.is_empty() {
        bail!("mission cannot be empty");
    }

    let paths = Paths::new(&repo)?;
    let _repo_lock = acquire_repo_lock(&paths, "mission")?;
    let mut state = load_state(&paths)?;
    let old_mission = state.mission.clone();
    if old_mission == mission {
        println!("[Rust] Mission already set: {mission}");
        print_status(&paths, &state);
        return Ok(());
    }

    state.mission = mission.to_string();
    let roadmap_updated = update_roadmap_mission_text(&paths, &old_mission, mission)?;
    push_event(
        &mut state,
        "rust",
        "mission-updated",
        &format!(
            "Mission changed from `{}` to `{}`; roadmap_updated={}.",
            old_mission, mission, roadmap_updated
        ),
    );
    save_state(&paths, &state)?;

    println!("[Rust] Mission updated: {mission}");
    if roadmap_updated {
        println!("[Rust] Updated ROADMAP.md mission text.");
    } else {
        println!("[Rust] ROADMAP.md unchanged; no matching stored mission text found.");
    }
    print_status(&paths, &state);
    Ok(())
}

pub fn models(
    repo: PathBuf,
    role: Option<String>,
    model: Option<String>,
    effort: Option<String>,
    timeout_seconds: Option<u64>,
) -> Result<()> {
    let paths = Paths::new(&repo)?;
    let _repo_lock = acquire_repo_lock(&paths, "models")?;
    let mut cfg = load_config(&paths)?;
    let updating = model.is_some() || effort.is_some() || timeout_seconds.is_some();
    if updating && role.is_none() {
        bail!("--role is required when setting --model, --effort, or --timeout-seconds");
    }

    if let Some(role_key) = role.as_deref() {
        let role_cfg = role_config_mut(&mut cfg, role_key)?;
        if let Some(model) = model {
            let model = model.trim();
            if model.is_empty() {
                bail!("model cannot be empty");
            }
            role_cfg.model = model.to_string();
        }
        if let Some(effort) = effort {
            validate_effort(&effort)?;
            role_cfg.effort = effort;
        }
        if let Some(timeout_seconds) = timeout_seconds {
            if timeout_seconds == 0 {
                bail!("timeout-seconds must be greater than zero");
            }
            role_cfg.timeout_seconds = timeout_seconds;
        }
        if updating {
            save_config(&paths, &cfg)?;
            println!("[Rust] Updated role config for `{role_key}`.");
        }
    }

    print_models_config(&cfg);
    Ok(())
}

fn role_config_mut<'a>(cfg: &'a mut Config, role_key: &str) -> Result<&'a mut RoleConfig> {
    match role_key {
        "master" => Ok(&mut cfg.roles.master),
        "planner" => Ok(&mut cfg.roles.planner),
        "foreman" => Ok(&mut cfg.roles.foreman),
        "worker" => Ok(&mut cfg.roles.worker),
        _ => bail!("unknown role `{role_key}`; expected master, planner, foreman, or worker"),
    }
}

fn validate_effort(effort: &str) -> Result<()> {
    match effort {
        "low" | "medium" | "high" | "xhigh" => Ok(()),
        _ => bail!("unsupported effort `{effort}`; expected low, medium, high, or xhigh"),
    }
}

fn print_models_config(cfg: &Config) {
    println!("roles:");
    println!(
        "- master: provider={} model={} effort={} timeout_seconds={}",
        cfg.roles.master.provider.as_str(),
        cfg.roles.master.model,
        cfg.roles.master.effort,
        cfg.roles.master.timeout_seconds
    );
    println!(
        "- planner: provider={} model={} effort={} timeout_seconds={}",
        cfg.roles.planner.provider.as_str(),
        cfg.roles.planner.model,
        cfg.roles.planner.effort,
        cfg.roles.planner.timeout_seconds
    );
    println!(
        "- foreman: provider={} model={} effort={} timeout_seconds={}",
        cfg.roles.foreman.provider.as_str(),
        cfg.roles.foreman.model,
        cfg.roles.foreman.effort,
        cfg.roles.foreman.timeout_seconds
    );
    println!(
        "- worker: provider={} model={} effort={} timeout_seconds={}",
        cfg.roles.worker.provider.as_str(),
        cfg.roles.worker.model,
        cfg.roles.worker.effort,
        cfg.roles.worker.timeout_seconds
    );
    println!(
        "ollama: enabled={} base_url={}",
        cfg.ollama.enabled,
        crate::providers::ollama::normalize_base_url(&cfg.ollama.base_url)
    );
    println!(
        "mlx_lm: enabled={} base_url={}",
        cfg.mlx_lm.enabled,
        crate::providers::openai_compat::normalize_base_url(&cfg.mlx_lm.base_url)
    );
}

fn update_roadmap_mission_text(
    paths: &Paths,
    old_mission: &str,
    new_mission: &str,
) -> Result<bool> {
    if !paths.roadmap.exists() || old_mission.trim().is_empty() {
        return Ok(false);
    }
    let text = read_text(&paths.roadmap)?;
    let Some(updated) = replace_roadmap_mission_text(&text, old_mission, new_mission) else {
        return Ok(false);
    };
    if updated == text {
        return Ok(false);
    }
    write_text(&paths.roadmap, &updated)?;
    Ok(true)
}

fn replace_roadmap_mission_text(
    text: &str,
    old_mission: &str,
    new_mission: &str,
) -> Option<String> {
    if old_mission.trim().is_empty() {
        return None;
    }

    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let content = line.trim_end_matches('\n').trim_end_matches('\r');
        let trimmed = content.trim_start();
        if trimmed.starts_with("Mission:") {
            let line_start = offset;
            let line_end = offset + content.len();
            let replacement = if content.contains(old_mission) {
                content.replacen(old_mission, new_mission, 1)
            } else {
                let indent_len = content.len() - trimmed.len();
                format!("{}Mission: {}", &content[..indent_len], new_mission)
            };
            let mut updated = String::with_capacity(text.len() + new_mission.len());
            updated.push_str(&text[..line_start]);
            updated.push_str(&replacement);
            updated.push_str(&text[line_end..]);
            return Some(updated);
        }
        offset += line.len();
    }

    let heading_start = text.find("## Mission")?;
    let section_body_start = text[heading_start..]
        .find('\n')
        .map(|index| heading_start + index + 1)
        .unwrap_or(text.len());
    let section_body = &text[section_body_start..];
    let section_end = section_body
        .find("\n## ")
        .map(|index| section_body_start + index)
        .unwrap_or(text.len());
    let section = &text[section_body_start..section_end];
    let relative = section.find(old_mission)?;
    let start = section_body_start + relative;
    let end = start + old_mission.len();
    let mut updated = String::with_capacity(text.len() + new_mission.len());
    updated.push_str(&text[..start]);
    updated.push_str(new_mission);
    updated.push_str(&text[end..]);
    Some(updated)
}

pub fn disposition(
    repo: PathBuf,
    decision: String,
    refresh_report: bool,
    json_output: bool,
) -> Result<()> {
    disposition_with_preconditions(repo, decision, refresh_report, json_output, None, None)
}

pub fn disposition_with_preconditions(
    repo: PathBuf,
    decision: String,
    refresh_report: bool,
    json_output: bool,
    expected_revision: Option<String>,
    expected_pause_fingerprint: Option<String>,
) -> Result<()> {
    let _json_guard = json_output.then(MachineJsonModeGuard::enter);
    let paths = Paths::new(&repo)?;
    let _repo_lock = acquire_repo_lock(&paths, "disposition")?;
    let mut state = load_state(&paths)?;
    match (
        expected_revision.as_deref(),
        expected_pause_fingerprint.as_deref(),
    ) {
        (None, None) => {}
        (Some(expected_revision), Some(expected_fingerprint)) => {
            for (label, value) in [
                ("--expected-revision", expected_revision),
                ("--pause-fingerprint", expected_fingerprint),
            ] {
                if value.len() != 64
                    || value != value.to_ascii_lowercase()
                    || !value.bytes().all(|byte| byte.is_ascii_hexdigit())
                {
                    bail!("{label} must be a 64-character lowercase SHA-256 digest");
                }
            }
            let current_revision = crate::revision::repository_revision(&paths)?;
            if current_revision != expected_revision {
                bail!("repository revision is stale; no disposition was recorded");
            }
            let pause = state
                .human_disposition_required
                .as_ref()
                .ok_or_else(|| anyhow!("no active human-disposition pause to fingerprint"))?;
            let current_fingerprint =
                crate::revision::human_disposition_pause_fingerprint(&paths, pause)?;
            if current_fingerprint != expected_fingerprint {
                bail!("human-disposition pause fingerprint is stale; no disposition was recorded");
            }
        }
        _ => bail!("--expected-revision and --pause-fingerprint must be supplied together"),
    }
    let choice = parse_human_disposition_choice(&decision);
    if refresh_report && !matches!(choice, HumanDispositionChoice::Stop) {
        bail!("--refresh-report can only be used with --decision keep-paused");
    }
    if refresh_report {
        refresh_dirty_adoption_report_for_active_pause(&paths, &mut state)?;
    }
    let outcome = if matches!(choice, HumanDispositionChoice::Stop) {
        if state.human_disposition_required.is_some() {
            println!("[Team] Keeping human-disposition pause active.");
            "kept_paused"
        } else {
            println!("[Team] No active human-disposition pause; nothing changed.");
            "no_pause"
        }
    } else {
        let pause = state
            .human_disposition_required
            .clone()
            .ok_or_else(|| anyhow!("no active human-disposition pause to answer"))?;
        apply_human_disposition_choice(&paths, &mut state, choice, &pause)?;
        "recorded"
    };
    save_state(&paths, &state)?;
    if json_output {
        let mut receipt = DispositionReceipt::from_state(
            paths.root.display().to_string(),
            outcome,
            &decision,
            refresh_report,
            &state,
            paths.events.display().to_string(),
        );
        receipt.revision = crate::revision::repository_revision(&paths).ok();
        receipt.pause_fingerprint = state.human_disposition_required.as_ref().and_then(|pause| {
            crate::revision::human_disposition_pause_fingerprint(&paths, pause).ok()
        });
        emit_json_receipt(&receipt)?;
    } else {
        print_status(&paths, &state);
    }
    Ok(())
}

fn refresh_dirty_adoption_report_for_active_pause(paths: &Paths, state: &mut State) -> Result<()> {
    let pause = state
        .human_disposition_required
        .as_ref()
        .ok_or_else(|| anyhow!("no active human-disposition pause to refresh"))?;
    if pause.source != "rust_dirty_adoption" {
        bail!(
            "can only refresh dirty adoption reports for rust_dirty_adoption pauses; active source is {}",
            pause.source
        );
    }
    let snapshot = current_repo_observation_snapshot_with_options(paths, true)?;
    fs::create_dir_all(&paths.observations)?;
    let report_path = paths.observations.join("dirty_adoption_report.md");
    let report = render_dirty_adoption_report(paths, &snapshot);
    write_text(&report_path, &report)?;
    let report_label = report_path
        .strip_prefix(&paths.root)
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| report_path.display().to_string());
    push_event(
        state,
        "rust",
        "dirty-adoption-report-refreshed",
        &format!(
            "Refreshed dirty adoption report for {} dirty file(s); report: {report_label}",
            snapshot.changed_files.len()
        ),
    );
    append_event(
        paths,
        OrchestratorEventKind::StateTransitionApplied,
        None,
        json!({
            "transition": "dirty_adoption_report_refreshed",
            "dirty_count": snapshot.changed_files.len(),
            "report": report_label,
        }),
    )?;
    println!("[Rust] Refreshed dirty adoption report: {report_label}");
    Ok(())
}

pub fn cost(repo: PathBuf) -> Result<()> {
    let paths = Paths::new(&repo)?;
    print_cost_report(&paths)
}

pub fn route_bench(repo: PathBuf, json_output: bool) -> Result<()> {
    let paths = Paths::new(&repo)?;
    let _repo_lock = acquire_repo_lock(&paths, "route-bench")?;
    let report = build_route_benchmark_report(&paths)?;
    write_route_benchmark_report(&paths, &report)?;
    if json_output {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print!("{}", render_route_benchmark_report(&report));
    }
    Ok(())
}

pub fn observe(
    repo: PathBuf,
    history: bool,
    max_commits: usize,
    codex_sessions: Option<PathBuf>,
) -> Result<()> {
    let paths = Paths::new(&repo)?;
    let _repo_lock = acquire_repo_lock(&paths, "observe")?;
    let mut state = load_state(&paths)?;
    let observed = observe_repo_change_state(&paths, &mut state)?;
    let history_observed = if history {
        observe_git_history(&paths, max_commits)?
    } else {
        0
    };
    let codex_session_observed = if let Some(session_path) = codex_sessions.as_deref() {
        observe_codex_session_exports(&paths, session_path)?
    } else {
        0
    };
    save_state(&paths, &state)?;
    if observed == 0 {
        println!("[Rust] No new repo change observations.");
    } else {
        println!(
            "[Rust] Recorded {observed} changed file(s) in {}.",
            paths.observations.display()
        );
    }
    if history {
        println!(
            "[Rust] Mined {history_observed} commit observation(s) into {}.",
            paths.observations.display()
        );
    }
    if codex_sessions.is_some() {
        println!(
            "[Rust] Mined {codex_session_observed} Codex session export(s) into {}.",
            paths.observations.display()
        );
    }
    Ok(())
}

pub fn learn(repo: PathBuf, global_memory: bool) -> Result<()> {
    let paths = Paths::new(&repo)?;
    let _repo_lock = acquire_repo_lock(&paths, "learn")?;
    let cfg = if paths.config.exists() {
        load_config(&paths)?
    } else {
        Config::default()
    };
    if !global_memory {
        println!(
            "[Rust] Local observations live in {}. Pass `--global` to promote them to {}.",
            paths.observations.display(),
            global_observations_dir(&cfg).display()
        );
        return Ok(());
    }

    let summary = promote_local_observations_to_global(&paths, &cfg)?;
    println!(
        "[Rust] Promoted local observations to global memory at {}.",
        summary.global_dir.display()
    );
    println!(
        "[Rust] Added {} Codex session observation(s) and {} git commit observation(s).",
        summary.codex_sessions_added, summary.git_commits_added
    );
    println!(
        "[Rust] Global memory now has {} verification hint file(s) and {} non-tracking affinity pair(s).",
        summary.verification_hint_files, summary.non_tracking_affinity_pairs
    );
    Ok(())
}

pub fn recipe_list(repo: PathBuf, include_global: bool, json_output: bool) -> Result<()> {
    let paths = Paths::new(&repo)?;
    let _repo_lock = acquire_repo_lock(&paths, "recipe-list")?;
    let receipt = crate::learning::list_recipes(&paths, include_global)?;
    if json_output {
        emit_json_receipt(&receipt)
    } else {
        println!("{}", serde_json::to_string_pretty(&receipt)?);
        Ok(())
    }
}

pub fn recipe_preview(repo: PathBuf, recipe_id: String, json_output: bool) -> Result<()> {
    let paths = Paths::new(&repo)?;
    let _repo_lock = acquire_repo_lock(&paths, "recipe-preview")?;
    let receipt = crate::learning::preview_recipe(&paths, &recipe_id)?;
    if json_output {
        emit_json_receipt(&receipt)
    } else {
        println!("{}", serde_json::to_string_pretty(&receipt)?);
        Ok(())
    }
}

pub fn recipe_approve(
    repo: PathBuf,
    recipe_id: String,
    expected_digest: String,
    expected_revision: String,
    json_output: bool,
) -> Result<()> {
    let paths = Paths::new(&repo)?;
    let _repo_lock = acquire_repo_lock(&paths, "recipe-approve")?;
    let mut cfg = load_config(&paths)?;
    cfg.require_clean_start = true;
    ensure_clean_start(&paths, &cfg)?;
    let state = load_state(&paths)?;
    ensure_no_pending_commit(&state)?;
    let receipt =
        crate::learning::approve_recipe(&paths, &recipe_id, &expected_digest, &expected_revision)?;
    if json_output {
        emit_json_receipt(&receipt)
    } else {
        println!("Approved global evidence recipe {}.", receipt.recipe_id);
        Ok(())
    }
}

pub fn recipe_reject(
    repo: PathBuf,
    recipe_id: String,
    expected_digest: String,
    json_output: bool,
) -> Result<()> {
    let paths = Paths::new(&repo)?;
    let _repo_lock = acquire_repo_lock(&paths, "recipe-reject")?;
    let receipt = crate::learning::reject_recipe(&paths, &recipe_id, &expected_digest)?;
    if json_output {
        emit_json_receipt(&receipt)
    } else {
        println!("Rejected recipe proposal {}.", receipt.recipe_id);
        Ok(())
    }
}

pub fn recipe_reactivate(
    repo: PathBuf,
    recipe_id: String,
    expected_digest: String,
    expected_revision: String,
    json_output: bool,
) -> Result<()> {
    let paths = Paths::new(&repo)?;
    let _repo_lock = acquire_repo_lock(&paths, "recipe-reactivate")?;
    let mut cfg = load_config(&paths)?;
    cfg.require_clean_start = true;
    ensure_clean_start(&paths, &cfg)?;
    let receipt = crate::learning::reactivate_recipe(
        &paths,
        &recipe_id,
        &expected_digest,
        &expected_revision,
    )?;
    if json_output {
        emit_json_receipt(&receipt)
    } else {
        println!(
            "Reactivated recipe {} for this repository.",
            receipt.recipe_id
        );
        Ok(())
    }
}

pub fn recipe_retire(recipe_id: String, expected_digest: String, json_output: bool) -> Result<()> {
    let receipt = crate::learning::retire_recipe(&recipe_id, &expected_digest)?;
    if json_output {
        emit_json_receipt(&receipt)
    } else {
        println!("Retired global recipe revision {}.", receipt.recipe_id);
        Ok(())
    }
}

pub fn self_improve_enroll(
    ca_repo: PathBuf,
    orchestrator_repo: PathBuf,
    install_login_service: bool,
    ca_profiles: Vec<String>,
    ca_execution_profile: String,
    confirmation: String,
) -> Result<()> {
    let vault = crate::self_improvement::LearningVault::machine()?;
    let inputs = crate::self_improvement::default_enrollment_inputs(
        ca_repo,
        orchestrator_repo,
        install_login_service,
        ca_profiles,
        ca_execution_profile,
    )?;
    let receipt = crate::self_improvement::enroll(&vault, &inputs, &confirmation)?;
    emit_json_receipt(&receipt)
}

fn knowledge_enrollment_inputs(
    supervisor_binary: PathBuf,
    ca_binary: PathBuf,
    codex_team_binary: PathBuf,
    ca_profiles: Vec<String>,
    ca_execution_profile: String,
    evidence_roots: Vec<String>,
    install_login_service: bool,
) -> Result<crate::self_improvement::KnowledgeEnrollmentInputs> {
    let evidence_roots = evidence_roots
        .into_iter()
        .map(|value| {
            let (kind, path) = value
                .split_once('=')
                .context("evidence roots must use KIND=/absolute/path")?;
            if kind.is_empty() || path.is_empty() {
                bail!("evidence roots must use KIND=/absolute/path");
            }
            Ok((kind.to_string(), PathBuf::from(path)))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(crate::self_improvement::KnowledgeEnrollmentInputs {
        install_login_service,
        supervisor_binary,
        ca_binary,
        codex_team_binary,
        ca_evidence_profiles: ca_profiles,
        ca_execution_profile,
        evidence_roots,
    })
}

pub fn self_improve_knowledge_preview(
    supervisor_binary: PathBuf,
    ca_binary: PathBuf,
    codex_team_binary: PathBuf,
    ca_profiles: Vec<String>,
    ca_execution_profile: String,
    evidence_roots: Vec<String>,
    install_login_service: bool,
) -> Result<()> {
    let inputs = knowledge_enrollment_inputs(
        supervisor_binary,
        ca_binary,
        codex_team_binary,
        ca_profiles,
        ca_execution_profile,
        evidence_roots,
        install_login_service,
    )?;
    let contract = crate::self_improvement::knowledge_enrollment_contract(&inputs)?;
    let confirmation_digest = crate::self_improvement::canonical_digest(&contract)?;
    emit_json_receipt(&serde_json::json!({
        "schema": "codex-team.knowledge_enrollment_preview.v1",
        "contract": contract,
        "confirmation_digest": confirmation_digest,
    }))
}

#[allow(clippy::too_many_arguments)]
pub fn self_improve_knowledge_enroll(
    supervisor_binary: PathBuf,
    ca_binary: PathBuf,
    codex_team_binary: PathBuf,
    ca_profiles: Vec<String>,
    ca_execution_profile: String,
    evidence_roots: Vec<String>,
    install_login_service: bool,
    confirmation: String,
) -> Result<()> {
    let inputs = knowledge_enrollment_inputs(
        supervisor_binary,
        ca_binary,
        codex_team_binary,
        ca_profiles,
        ca_execution_profile,
        evidence_roots,
        install_login_service,
    )?;
    let vault = crate::self_improvement::LearningVault::machine()?;
    let receipt = crate::self_improvement::enroll_knowledge(&vault, &inputs, &confirmation)?;
    emit_json_receipt(&receipt)
}

pub fn self_improve_status(_json_output: bool) -> Result<()> {
    let receipt = match crate::self_improvement::LearningVault::existing_machine()? {
        Some(vault) => crate::self_improvement::status(&vault)?,
        None => crate::self_improvement::unenrolled_status()?,
    };
    emit_json_receipt(&receipt)
}

pub fn self_improve_doctor(_json_output: bool) -> Result<()> {
    let receipt = match crate::self_improvement::LearningVault::existing_machine()? {
        Some(vault) => crate::self_improvement::doctor(&vault)?,
        None => crate::self_improvement::unenrolled_doctor()?,
    };
    emit_json_receipt(&receipt)?;
    if !receipt.healthy {
        bail!("self-improvement doctor: unhealthy (healthy=false in JSON receipt)");
    }
    Ok(())
}

pub fn self_improve_pause(reason: String, _json_output: bool) -> Result<()> {
    let vault = crate::self_improvement::LearningVault::existing_machine()?
        .context("self-improvement is not enrolled")?;
    emit_json_receipt(&crate::self_improvement::pause(&vault, &reason)?)
}

pub fn self_improve_resume(expected_revision: String, _json_output: bool) -> Result<()> {
    let vault = crate::self_improvement::LearningVault::existing_machine()?
        .context("self-improvement is not enrolled")?;
    emit_json_receipt(&crate::self_improvement::resume(
        &vault,
        &expected_revision,
    )?)
}

pub fn self_improve_rollback(
    component: crate::self_improvement::ComponentV1,
    _json_output: bool,
) -> Result<()> {
    let vault = crate::self_improvement::LearningVault::existing_machine()?
        .context("self-improvement is not enrolled")?;
    emit_json_receipt(&crate::self_improvement::rollback(&vault, component)?)
}

pub fn self_improve_objectives(id: Option<String>, _json_output: bool) -> Result<()> {
    let objectives = match crate::self_improvement::LearningVault::existing_machine()? {
        Some(vault) => crate::self_improvement::list_objectives(&vault, id.as_deref())?,
        None if id.is_none() => Vec::new(),
        None => bail!("self-improvement is not enrolled"),
    };
    emit_json_receipt(&serde_json::json!({
        "schema": "codex-team.self_improvement_objectives.v1",
        "objectives": objectives,
    }))
}

pub fn self_improve_traces(id: Option<String>, _json_output: bool) -> Result<()> {
    let traces = match crate::self_improvement::LearningVault::existing_machine()? {
        Some(vault) => crate::self_improvement::list_traces(&vault, id.as_deref())?,
        None if id.is_none() => Vec::new(),
        None => bail!("self-improvement is not enrolled"),
    };
    emit_json_receipt(&serde_json::json!({
        "schema": "codex-team.self_improvement_traces.v1",
        "traces": traces,
    }))
}

pub fn self_improve_deployments(_json_output: bool) -> Result<()> {
    let deployments = match crate::self_improvement::LearningVault::existing_machine()? {
        Some(vault) => crate::self_improvement::list_deployments(&vault)?,
        None => Vec::new(),
    };
    emit_json_receipt(&serde_json::json!({
        "schema": "codex-team.self_improvement_deployments.v1",
        "deployments": deployments,
    }))
}

pub fn self_improve_worker(vault_root: PathBuf, expected_revision: String) -> Result<()> {
    crate::cancellation::install_process_signal_handlers()?;
    let vault = crate::self_improvement::LearningVault::existing_at(vault_root)?
        .context("self-improvement vault does not exist")?;
    let status = crate::self_improvement::status(&vault)?;
    if status.revision != expected_revision {
        bail!("stale self-improvement worker revision; no learning stage started");
    }
    if !status.running_allowed {
        bail!("self-improvement worker requires healthy, enrolled, unpaused state");
    }
    let doctor = crate::self_improvement::doctor(&vault)?;
    if !doctor.healthy {
        bail!(
            "self-improvement worker preflight failed: {}",
            doctor.issues.join("; ")
        );
    }
    let expected_binary = crate::self_improvement::active_component_binary(&vault, "codex-team")?;
    let current_binary = std::fs::canonicalize(std::env::current_exe()?)?;
    if current_binary != std::fs::canonicalize(&expected_binary)? {
        bail!("self-improvement worker was not launched from the active verified slot");
    }
    let ca_binary = crate::self_improvement::active_component_binary(&vault, "ca")?;
    let execution_profile = status
        .ca_execution_profile
        .as_ref()
        .context("self-improvement enrollment has no ca execution profile")?;
    let bridge = crate::self_improvement::ProcessCaLearningBridgeClient {
        binary: ca_binary.clone(),
    };
    let runner = crate::self_improvement::CaRoleRunLearningRunner {
        binary: ca_binary,
        profile: execution_profile.name.clone(),
        vault: vault.clone(),
    };
    let detail = crate::self_improvement::run_learning_stage(&vault, &bridge, &runner)?;
    emit_json_receipt(&serde_json::json!({
        "schema": "codex-team.self_improvement_worker.v1",
        "completed_one_stage": true,
        "detail": detail,
        "source_changes": false,
        "binary_activation": false,
    }))
}

pub fn doctor_command(repo: PathBuf, json_output: bool) -> Result<()> {
    let _json_guard = json_output.then(MachineJsonModeGuard::enter);
    let paths = Paths::new(&repo)?;
    if json_output {
        let receipt = build_doctor_receipt(&paths)?;
        emit_json_receipt(&receipt)?;
        // Exit non-zero after emitting JSON so supervisors can branch on status
        // without always parsing; `healthy` in the receipt remains authoritative.
        if !receipt.healthy {
            anyhow::bail!("doctor: unhealthy (healthy=false in JSON receipt)");
        }
    } else {
        doctor(&paths)?;
    }
    Ok(())
}

pub fn export_commands(repo: PathBuf, task: Option<String>) -> Result<()> {
    let paths = Paths::new(&repo)?;
    let _repo_lock = acquire_repo_lock(&paths, "commands")?;
    export_manual_command_scripts(&paths, task.as_deref())
}

pub fn watch(repo: PathBuf, task: Option<String>, self_heal: bool) -> Result<()> {
    let paths = Paths::new(&repo)?;
    let _repo_lock = acquire_repo_lock(&paths, "watch")?;
    let cfg = load_config(&paths)?;
    ensure_git_ready(&paths, &cfg)?;
    let mut state = load_state(&paths)?;
    reopen_if_next_steps_unfinished(&paths, &mut state)?;
    let watch = build_watch_plan(&paths, task.as_deref(), &state)?;
    let report = build_efficiency_watch_report(&paths, &state, watch)?;
    let report_path = write_efficiency_watch_report(&paths, &report)?;
    append_event(
        &paths,
        OrchestratorEventKind::WatchDebounced,
        task.as_deref(),
        json!({
            "watch": report.watch,
            "alert_count": report.alerts.len(),
            "report": display_path(&paths, &report_path),
        }),
    )?;
    let self_heal_report = if self_heal {
        Some(apply_efficiency_self_heal(&paths, &cfg, &report)?)
    } else {
        None
    };
    println!(
        "watch: {} file pattern(s), task={}",
        report.watch.paths.len(),
        task.as_deref().unwrap_or("(auto)")
    );
    println!("report: {}", display_path(&paths, &report_path));
    if report.alerts.is_empty() {
        println!("alerts: none");
    } else {
        println!("alerts:");
        for alert in &report.alerts {
            println!("- [{}] {}: {}", alert.severity, alert.kind, alert.summary);
        }
    }
    if !report.suggestions.is_empty() {
        println!("suggestions:");
        for suggestion in &report.suggestions {
            println!("- {suggestion}");
        }
    }
    if let Some(self_heal_report) = self_heal_report {
        println!("self-heal:");
        if self_heal_report.actions.is_empty() {
            println!("- no safe automatic action available");
        } else {
            for action in &self_heal_report.actions {
                println!("- [{}] {}: {}", action.status, action.kind, action.summary);
            }
        }
    }
    Ok(())
}

pub fn watch_loop(
    repo: PathBuf,
    orchestrator_repo: Option<PathBuf>,
    orchestrator_mission: String,
    max_cycles: usize,
    self_heal: bool,
    orchestrator_cycles: usize,
) -> Result<()> {
    if max_cycles == 0 {
        bail!("--max-cycles must be greater than 0");
    }
    let target_paths = Paths::new(&repo)?;
    let orchestrator_paths = orchestrator_repo.as_deref().map(Paths::new).transpose()?;
    let mut lock_targets = vec![RepoLockTarget::from_paths(&target_paths, "watch-loop")];
    if let Some(paths) = orchestrator_paths.as_ref() {
        lock_targets.push(RepoLockTarget::from_paths(paths, "watch-loop-orchestrator"));
    }
    let _repo_locks = acquire_repo_locks(lock_targets)?;

    let target_cfg = load_config(&target_paths)?;
    ensure_codex_ready_if_needed(&target_cfg)?;
    ensure_git_ready(&target_paths, &target_cfg)?;
    if let Some(paths) = orchestrator_paths.as_ref() {
        ensure_orchestrator_repo_initialized(paths, &orchestrator_mission)?;
        let cfg = load_config(paths)?;
        ensure_codex_ready_if_needed(&cfg)?;
        ensure_git_ready(paths, &cfg)?;
    }

    let started_at = Utc::now().to_rfc3339();
    let mut cycles = Vec::new();
    let mut stopped_reason = "max_cycles_reached".to_string();
    println!("[WatchLoop] codex-team v{}", env!("CARGO_PKG_VERSION"));
    println!(
        "[WatchLoop] target={} max_cycles={} self_heal={} orchestrator_repo={}",
        target_paths.root.display(),
        max_cycles,
        self_heal,
        orchestrator_paths
            .as_ref()
            .map(|paths| paths.root.display().to_string())
            .unwrap_or_else(|| "(none)".to_string())
    );
    for iteration in 1..=max_cycles {
        println!("\n[WatchLoop] Iteration {iteration}/{max_cycles}: target step");
        let auto_gated_before_step = self_heal && try_auto_gate_watch_loop_pause(&target_paths)?;
        let target_human_disposition_mode = if auto_gated_before_step {
            watch_loop_human_disposition_mode()
        } else {
            watch_loop_target_human_disposition_mode(&target_paths, self_heal)
        };
        let mut target_outcome = run_one_supervised_cycle_with_human_mode(
            &target_paths,
            &target_cfg,
            target_human_disposition_mode,
        )?;
        if target_outcome.human_pause && self_heal && try_auto_gate_watch_loop_pause(&target_paths)?
        {
            target_outcome.human_pause = false;
            target_outcome.should_continue = true;
            target_outcome.stopped_reason = Some("human_disposition_auto_gated".to_string());
        }
        let (watch_report, self_heal_report, post_heal_report) =
            run_efficiency_watch_once(&target_paths, &target_cfg, self_heal)?;
        let watch_alert_count = watch_report.alerts.len();
        let post_heal_alert_count = post_heal_report.alerts.len();
        let self_heal_action_count = self_heal_report
            .as_ref()
            .map(|report| report.actions.len())
            .unwrap_or(0);
        let mut cycle = EfficiencyLoopCycle {
            iteration,
            target_step_ran: target_outcome.step_ran,
            target_should_continue: target_outcome.should_continue,
            target_human_pause: target_outcome.human_pause,
            watch_alert_count,
            post_heal_alert_count,
            self_heal_action_count,
            orchestrator_feedback_path: None,
            orchestrator_cycles_run: 0,
            stop_reason: target_outcome.stopped_reason.clone(),
        };
        println!(
            "[WatchLoop] watcher alerts: before_self_heal={} after_self_heal={} self_heal_actions={}",
            watch_alert_count, post_heal_alert_count, self_heal_action_count
        );

        if target_outcome.human_pause {
            stopped_reason = "target_human_disposition_pause".to_string();
            cycle.stop_reason = Some(stopped_reason.clone());
            cycles.push(cycle);
            break;
        }
        if post_heal_alert_count == 0 {
            stopped_reason = "target_watch_clean".to_string();
            cycle.stop_reason = Some(stopped_reason.clone());
            cycles.push(cycle);
            break;
        }

        let Some(orchestrator_paths) = orchestrator_paths.as_ref() else {
            stopped_reason = "unresolved_alerts_without_orchestrator_repo".to_string();
            cycle.stop_reason = Some(stopped_reason.clone());
            cycles.push(cycle);
            break;
        };
        let feedback_path = record_external_efficiency_feedback(
            orchestrator_paths,
            &target_paths.root,
            iteration,
            &post_heal_report,
        )?;
        cycle.orchestrator_feedback_path = Some(display_path(orchestrator_paths, &feedback_path));
        println!(
            "[WatchLoop] wrote orchestrator efficiency feedback: {}",
            display_path(orchestrator_paths, &feedback_path)
        );

        let orchestrator_cfg = load_config(orchestrator_paths)?;
        let repair_cycles = orchestrator_cycles.max(1);
        let mut repair_stop_reason = None;
        for repair_cycle in 1..=repair_cycles {
            println!(
                "[WatchLoop] orchestrator repair cycle {repair_cycle}/{repair_cycles}: {}",
                orchestrator_paths.root.display()
            );
            let orchestrator_human_disposition_mode =
                watch_loop_orchestrator_human_disposition_mode(orchestrator_paths, self_heal);
            let mut outcome = run_one_supervised_cycle_with_human_mode(
                orchestrator_paths,
                &orchestrator_cfg,
                orchestrator_human_disposition_mode,
            )?;
            cycle.orchestrator_cycles_run += usize::from(outcome.step_ran);
            if outcome.human_pause
                && self_heal
                && try_auto_gate_watch_loop_pause(orchestrator_paths)?
            {
                outcome.human_pause = false;
                outcome.should_continue = true;
                outcome.stopped_reason = Some("human_disposition_auto_gated".to_string());
            }
            if let Some(reason) = orchestrator_repair_block_reason(&outcome) {
                println!("[WatchLoop] stopping: {reason}");
                repair_stop_reason = Some(reason);
                break;
            }
            if !outcome.should_continue {
                break;
            }
        }
        if let Some(reason) = repair_stop_reason {
            stopped_reason = reason;
            cycle.stop_reason = Some(stopped_reason.clone());
            cycles.push(cycle);
            break;
        }
        cycles.push(cycle);
    }

    let report = EfficiencyLoopReport {
        started_at,
        completed_at: Utc::now().to_rfc3339(),
        target_repo: target_paths.root.display().to_string(),
        orchestrator_repo: orchestrator_paths
            .as_ref()
            .map(|paths| paths.root.display().to_string()),
        max_cycles,
        self_heal,
        orchestrator_cycles,
        stopped_reason,
        cycles,
    };
    let report_path = write_efficiency_loop_report(&target_paths, &report)?;
    println!(
        "[WatchLoop] stopped: {}; report: {}",
        report.stopped_reason,
        display_path(&target_paths, &report_path)
    );
    Ok(())
}

fn watch_loop_target_human_disposition_mode(
    paths: &Paths,
    self_heal: bool,
) -> HumanDispositionMode {
    watch_loop_auto_gate_human_disposition_mode(
        paths,
        self_heal,
        watch_loop_human_disposition_mode(),
    )
}

fn watch_loop_orchestrator_human_disposition_mode(
    paths: &Paths,
    self_heal: bool,
) -> HumanDispositionMode {
    watch_loop_auto_gate_human_disposition_mode(paths, self_heal, HumanDispositionMode::Stop)
}

fn watch_loop_auto_gate_human_disposition_mode(
    paths: &Paths,
    self_heal: bool,
    fallback: HumanDispositionMode,
) -> HumanDispositionMode {
    if self_heal {
        if let Ok(state) = load_state(paths) {
            if state
                .human_disposition_required
                .as_ref()
                .is_some_and(pause_is_auto_gateable)
            {
                println!(
                    "[WatchLoop] auto-disposition: approving explicit safety gate for PM-created pause."
                );
                return HumanDispositionMode::Preset(HumanDispositionChoice::ApproveGate);
            }
        }
    }
    fallback
}

fn try_auto_gate_watch_loop_pause(paths: &Paths) -> Result<bool> {
    let mut state = load_state(paths)?;
    let Some(pause) = state.human_disposition_required.clone() else {
        return Ok(false);
    };
    if !pause_is_auto_gateable(&pause) {
        return Ok(false);
    }
    println!("[WatchLoop] auto-disposition: approving explicit safety gate for PM-created pause.");
    apply_human_disposition_choice(
        paths,
        &mut state,
        HumanDispositionChoice::ApproveGate,
        &pause,
    )?;
    save_state(paths, &state)?;
    print_status(paths, &state);
    Ok(true)
}

fn pause_is_auto_gateable(pause: &HumanDispositionPause) -> bool {
    if pause.source != "project_manager" {
        return false;
    }
    let text = std::iter::once(pause.reason.as_str())
        .chain(pause.options.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join("\n")
        .to_ascii_lowercase();
    (text.contains("explicit safety gate")
        || text.contains("approval gate")
        || text.contains("safety/approval gate"))
        && !text.contains("rust_dirty_adoption")
}

fn watch_loop_human_disposition_mode() -> HumanDispositionMode {
    if io::stdin().is_terminal() {
        HumanDispositionMode::Prompt
    } else {
        HumanDispositionMode::Stop
    }
}

fn orchestrator_repair_block_reason(outcome: &SupervisedCycleOutcome) -> Option<String> {
    if outcome.human_pause {
        return Some("orchestrator_human_disposition_pause".to_string());
    }
    if !outcome.step_ran {
        return Some(format!(
            "orchestrator_{}",
            outcome
                .stopped_reason
                .as_deref()
                .unwrap_or("no_runnable_cycle")
        ));
    }
    if !outcome.should_continue {
        return Some(format!(
            "orchestrator_{}",
            outcome
                .stopped_reason
                .as_deref()
                .unwrap_or("repair_stopped")
        ));
    }
    None
}

fn ensure_orchestrator_repo_initialized(paths: &Paths, mission: &str) -> Result<()> {
    if paths.config.exists() {
        return Ok(());
    }
    println!(
        "[WatchLoop] initializing orchestrator repo state at {}",
        paths.orch.display()
    );
    init_repo(paths, mission)
}

fn run_one_supervised_cycle_with_human_mode(
    paths: &Paths,
    cfg: &Config,
    human_disposition_mode: HumanDispositionMode,
) -> Result<SupervisedCycleOutcome> {
    ensure_git_ready(paths, cfg)?;
    let mut state = load_state(paths)?;
    ensure_import_receipts_complete(paths, &state.queue, &state.imported_tasks)?;
    ensure_no_pending_commit(&state)?;
    ensure_clean_start(paths, cfg)?;
    reopen_if_next_steps_unfinished(paths, &mut state)?;
    settle_idle_escalation_if_no_active_work(paths, &mut state)?;
    let had_human_pause = state.human_disposition_required.is_some();
    if had_human_pause && human_disposition_mode == HumanDispositionMode::Stop {
        save_state(paths, &state)?;
        return Ok(SupervisedCycleOutcome {
            step_ran: false,
            should_continue: false,
            human_pause: true,
            stopped_reason: Some("human_disposition_pause_active".to_string()),
        });
    }
    if !had_human_pause && should_stop_for_no_work(paths, &state)? {
        try_commit_control_doc_edits_with_log(
            paths,
            cfg,
            &mut state,
            "Update completed project control docs",
        );
        state.master_due = false;
        save_state(paths, &state)?;
        return Ok(SupervisedCycleOutcome {
            step_ran: false,
            should_continue: false,
            human_pause: false,
            stopped_reason: Some("no_work".to_string()),
        });
    }

    let runner = RealCodexRunner;
    let should_continue =
        step_once_with_runner_mode(paths, cfg, &mut state, &runner, human_disposition_mode)?;
    let human_pause = state.human_disposition_required.is_some();
    let stopped_reason = if human_pause {
        Some("human_disposition_pause_active".to_string())
    } else if had_human_pause && !should_continue {
        Some("human_disposition_recorded_stop".to_string())
    } else {
        None
    };
    save_state(paths, &state)?;
    print_status(paths, &state);
    Ok(SupervisedCycleOutcome {
        step_ran: true,
        should_continue,
        human_pause,
        stopped_reason,
    })
}

fn run_efficiency_watch_once(
    paths: &Paths,
    cfg: &Config,
    self_heal: bool,
) -> Result<(
    EfficiencyWatchReport,
    Option<EfficiencySelfHealReport>,
    EfficiencyWatchReport,
)> {
    let mut state = load_state(paths)?;
    reopen_if_next_steps_unfinished(paths, &mut state)?;
    sync_state_queue_with_next_steps(paths, &mut state);
    settle_idle_escalation_if_no_active_work(paths, &mut state)?;
    save_state(paths, &state)?;
    let watch = build_watch_plan(paths, None, &state)?;
    let report = build_efficiency_watch_report(paths, &state, watch)?;
    let report_path = write_efficiency_watch_report(paths, &report)?;
    append_event(
        paths,
        OrchestratorEventKind::WatchDebounced,
        None,
        json!({
            "mode": "watch_loop_target_watch",
            "watch": report.watch,
            "alert_count": report.alerts.len(),
            "report": display_path(paths, &report_path),
        }),
    )?;
    let self_heal_report = if self_heal {
        Some(apply_efficiency_self_heal(paths, cfg, &report)?)
    } else {
        None
    };
    let post_state = load_state(paths)?;
    let post_watch = build_watch_plan(paths, None, &post_state)?;
    let post_heal_report = build_efficiency_watch_report(paths, &post_state, post_watch)?;
    if self_heal {
        write_efficiency_watch_report(paths, &post_heal_report)?;
    }
    Ok((report, self_heal_report, post_heal_report))
}

fn record_external_efficiency_feedback(
    orchestrator_paths: &Paths,
    target_repo: &Path,
    iteration: usize,
    report: &EfficiencyWatchReport,
) -> Result<PathBuf> {
    fs::create_dir_all(&orchestrator_paths.observations)?;
    let dir = orchestrator_paths
        .observations
        .join("external_efficiency_feedback");
    fs::create_dir_all(&dir)?;
    let stamp = Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    let stem = format!("target-efficiency-{stamp}-iter-{iteration}");
    let json_path = dir.join(format!("{stem}.json"));
    let md_path = dir.join(format!("{stem}.md"));
    let has_mechanistic_handoff_alert = report
        .alerts
        .iter()
        .any(|alert| alert.mechanistic_handoff.is_some());
    let instruction = if has_mechanistic_handoff_alert {
        "Use normal orchestrator routing to reduce this token/time churn. Mechanistic handoff alerts should become typed Rust recipes or pre-Worker routing fixes when safe; unsupported commands need recipe/router repair before Worker sees them again."
    } else {
        "Use normal orchestrator routing to reduce this token/time churn. Prefer Rust-owned deterministic evidence jobs, context-pack improvements, prompt linting, and queue/routing fixes before adding new role loops."
    };
    let payload = json!({
        "recorded_at": Utc::now().to_rfc3339(),
        "target_repo": target_repo.display().to_string(),
        "iteration": iteration,
        "alert_count": report.alerts.len(),
        "alerts": report.alerts,
        "suggestions": report.suggestions,
        "instruction": instruction,
    });
    write_text(
        &json_path,
        &format!("{}\n", serde_json::to_string_pretty(&payload)?),
    )?;
    write_text(
        &md_path,
        &render_external_efficiency_feedback_markdown(target_repo, iteration, report),
    )?;

    let mut state = load_state(orchestrator_paths)?;
    state.planner_due = true;
    push_event(
        &mut state,
        "watcher",
        "external-efficiency-feedback",
        &format!(
            "Target {} produced {} efficiency alert(s); feedback packet: {}",
            target_repo.display(),
            report.alerts.len(),
            display_path(orchestrator_paths, &md_path)
        ),
    );
    save_state(orchestrator_paths, &state)?;
    append_event(
        orchestrator_paths,
        OrchestratorEventKind::WatchDebounced,
        None,
        json!({
            "mode": "external_efficiency_feedback_recorded",
            "target_repo": target_repo.display().to_string(),
            "iteration": iteration,
            "alert_count": report.alerts.len(),
            "json": display_path(orchestrator_paths, &json_path),
            "markdown": display_path(orchestrator_paths, &md_path),
        }),
    )?;
    Ok(md_path)
}

fn render_external_efficiency_feedback_markdown(
    target_repo: &Path,
    iteration: usize,
    report: &EfficiencyWatchReport,
) -> String {
    let mut out = String::new();
    out.push_str("# External Efficiency Feedback\n\n");
    out.push_str(&format!("- Target repo: `{}`\n", target_repo.display()));
    out.push_str(&format!("- Iteration: `{iteration}`\n"));
    out.push_str(&format!(
        "- Watch generated at: `{}`\n",
        report.generated_at
    ));
    out.push_str(&format!("- Alert count: `{}`\n\n", report.alerts.len()));
    out.push_str("## Contract\n\n");
    out.push_str("Reduce token/time churn through normal orchestrator work. Prefer Rust-owned deterministic evidence jobs, context-pack improvements, prompt linting, queue/routing fixes, and compact receipts before adding new role loops.\n\n");
    if report
        .alerts
        .iter()
        .any(|alert| alert.mechanistic_handoff.is_some())
    {
        out.push_str("Mechanistic handoff alerts should become typed Rust recipes or pre-Worker routing fixes when safe. Unsupported commands should be repaired in the orchestrator repo before Worker is allowed to run the same live process again.\n\n");
    }
    if report.alerts.is_empty() {
        out.push_str("No alerts remain.\n");
        return out;
    }
    out.push_str("## Alerts\n\n");
    for alert in &report.alerts {
        out.push_str(&format!(
            "### {}: {}\n\n{}\n\n",
            alert.severity, alert.kind, alert.summary
        ));
        if !alert.evidence.is_empty() {
            out.push_str("Evidence:\n");
            for item in &alert.evidence {
                out.push_str(&format!("- {item}\n"));
            }
            out.push('\n');
        }
    }
    if !report.suggestions.is_empty() {
        out.push_str("## Suggestions\n\n");
        for suggestion in &report.suggestions {
            out.push_str(&format!("- {suggestion}\n"));
        }
        out.push('\n');
    }
    out
}

fn write_efficiency_loop_report(paths: &Paths, report: &EfficiencyLoopReport) -> Result<PathBuf> {
    fs::create_dir_all(&paths.observations)?;
    let json_path = paths.observations.join("efficiency_loop.json");
    let md_path = paths.observations.join("efficiency_loop.md");
    write_text(
        &json_path,
        &format!("{}\n", serde_json::to_string_pretty(report)?),
    )?;
    write_text(&md_path, &render_efficiency_loop_report(report))?;
    append_event(
        paths,
        OrchestratorEventKind::WatchDebounced,
        None,
        json!({
            "mode": "efficiency_watch_loop",
            "stopped_reason": report.stopped_reason,
            "cycle_count": report.cycles.len(),
            "report": display_path(paths, &md_path),
        }),
    )?;
    Ok(md_path)
}

fn render_efficiency_loop_report(report: &EfficiencyLoopReport) -> String {
    let mut out = String::new();
    out.push_str("# Efficiency Watch Loop\n\n");
    out.push_str(&format!("- Started at: `{}`\n", report.started_at));
    out.push_str(&format!("- Completed at: `{}`\n", report.completed_at));
    out.push_str(&format!("- Target repo: `{}`\n", report.target_repo));
    out.push_str(&format!(
        "- Orchestrator repo: `{}`\n",
        report.orchestrator_repo.as_deref().unwrap_or("(none)")
    ));
    out.push_str(&format!("- Stop reason: `{}`\n", report.stopped_reason));
    out.push_str(&format!("- Cycles: `{}`\n\n", report.cycles.len()));
    out.push_str("## Cycles\n\n");
    for cycle in &report.cycles {
        out.push_str(&format!(
            "### Iteration {}\n\n- Target step ran: `{}`\n- Target should continue: `{}`\n- Target human pause: `{}`\n- Watch alerts: `{}`\n- Post-heal alerts: `{}`\n- Self-heal actions: `{}`\n- Orchestrator cycles run: `{}`\n",
            cycle.iteration,
            cycle.target_step_ran,
            cycle.target_should_continue,
            cycle.target_human_pause,
            cycle.watch_alert_count,
            cycle.post_heal_alert_count,
            cycle.self_heal_action_count,
            cycle.orchestrator_cycles_run
        ));
        if let Some(path) = &cycle.orchestrator_feedback_path {
            out.push_str(&format!("- Orchestrator feedback: `{path}`\n"));
        }
        if let Some(reason) = &cycle.stop_reason {
            out.push_str(&format!("- Stop reason: `{reason}`\n"));
        }
        out.push('\n');
    }
    out
}

fn init_repo(paths: &Paths, mission: &str) -> Result<()> {
    let roadmap_preexisting = paths.roadmap.exists();
    let next_steps_preexisting = paths.next_steps.exists();
    let next_steps_non_ai_preexisting = paths.next_steps_non_ai.exists();
    let agents_preexisting = paths.agents.exists();

    fs::create_dir_all(&paths.prompts)?;
    fs::create_dir_all(&paths.schemas)?;
    fs::create_dir_all(&paths.tasks)?;
    fs::create_dir_all(&paths.runs)?;
    fs::create_dir_all(&paths.recipes)?;
    fs::create_dir_all(&paths.observations)?;
    fs::create_dir_all(&paths.evidence)?;

    write_if_missing(&paths.config, &toml::to_string_pretty(&Config::default())?)?;
    write_if_missing(
        &paths.roadmap,
        &format!("# ROADMAP\n\nMission: {mission}\n\nStatus: not planned yet.\n"),
    )?;
    write_if_missing(
        &paths.next_steps,
        "# NEXT_STEPS\n\nNo approved tasks yet.\n",
    )?;
    write_if_missing(
        &paths.next_steps_non_ai,
        "# NEXT_STEPS_non-ai\n\nNo approved deterministic tasks yet.\n",
    )?;
    write_if_missing(&paths.agents, DEFAULT_AGENTS_MD)?;
    record_adoption_scaffold(
        paths,
        &[
            ("ROADMAP.md", roadmap_preexisting),
            ("NEXT_STEPS.md", next_steps_preexisting),
            ("NEXT_STEPS_non-ai.md", next_steps_non_ai_preexisting),
            ("AGENTS.md", agents_preexisting),
        ],
    )?;

    if roadmap_preexisting {
        println!("Preserved existing ROADMAP.md");
    }
    if next_steps_preexisting {
        println!("Preserved existing NEXT_STEPS.md");
    }
    if agents_preexisting {
        println!("Preserved existing AGENTS.md");
    }

    write_text(&paths.prompts.join("master.md"), MASTER_PROMPT)?;
    write_text(&paths.prompts.join("planner.md"), PLANNER_PROMPT)?;
    write_text(
        &paths.prompts.join("foreman_review.md"),
        FOREMAN_REVIEW_PROMPT,
    )?;
    write_text(
        &paths.prompts.join("foreman_batch.md"),
        FOREMAN_BATCH_PROMPT,
    )?;
    write_text(&paths.prompts.join("worker.md"), WORKER_PROMPT)?;

    write_text(
        &paths.schemas.join("master.schema.json"),
        &render_schema(MASTER_SCHEMA),
    )?;
    write_text(
        &paths.schemas.join("planner.schema.json"),
        &render_schema(PLANNER_SCHEMA),
    )?;
    write_text(
        &paths.schemas.join("worker.schema.json"),
        &render_schema(WORKER_SCHEMA),
    )?;
    write_text(
        &paths.schemas.join("foreman_review.schema.json"),
        &render_schema(FOREMAN_REVIEW_SCHEMA),
    )?;
    write_text(
        &paths.schemas.join("foreman_batch.schema.json"),
        &render_schema(FOREMAN_BATCH_SCHEMA),
    )?;

    if !paths.state.exists() {
        let adopting_existing_project =
            roadmap_preexisting || next_steps_preexisting || agents_preexisting;
        let state = State {
            mission: mission.to_string(),
            master_due: !adopting_existing_project,
            needs_reconciliation: adopting_existing_project,
            ..State::default()
        };
        save_state(paths, &state)?;
    }

    if find_git_root(&paths.root).is_none() {
        println!(
            "Warning: {} is not inside a Git repository. The default config requires Git before `step` or `run`.",
            paths.root.display()
        );
    }

    Ok(())
}

fn record_adoption_scaffold(paths: &Paths, root_docs: &[(&str, bool)]) -> Result<()> {
    let created_files = root_docs
        .iter()
        .filter(|&(_path, preexisting)| !*preexisting)
        .map(|(path, _preexisting)| (*path).to_string())
        .collect::<Vec<_>>();
    let record = AdoptionScaffoldRecord {
        recorded_at: Utc::now().to_rfc3339(),
        created_files,
        note: "Root files created by codex-team init; report classification only, not approval."
            .to_string(),
    };
    write_text(
        &paths.observations.join("adoption_scaffold.json"),
        &format!("{}\n", serde_json::to_string_pretty(&record)?),
    )
}

// ---------------------------------------------------------------------------
// Cycle core (`step_once*`)
// ---------------------------------------------------------------------------
//
// Orders deterministic drains, queue recovery, dirty-adoption pauses, role gates,
// Worker/Senior/PM/CEO calls, and returns whether another cycle should run.
// Non-TTY human-disposition mode stops instead of blocking on stdin.

fn step_once(paths: &Paths, cfg: &Config, state: &mut State) -> Result<bool> {
    let runner = RealCodexRunner;
    step_once_with_runner(paths, cfg, state, &runner)
}

fn step_once_with_runner(
    paths: &Paths,
    cfg: &Config,
    state: &mut State,
    runner: &dyn CodexRunner,
) -> Result<bool> {
    step_once_with_runner_mode(
        paths,
        cfg,
        state,
        runner,
        watch_loop_human_disposition_mode(),
    )
}

fn step_once_with_runner_mode(
    paths: &Paths,
    cfg: &Config,
    state: &mut State,
    runner: &dyn CodexRunner,
    human_disposition_mode: HumanDispositionMode,
) -> Result<bool> {
    ensure_import_receipts_complete(paths, &state.queue, &state.imported_tasks)?;
    ensure_no_pending_commit(state)?;
    ensure_clean_start(paths, cfg)?;
    // Ordering matters here:
    // 1. Let Rust drain deterministic/bookkeeping work.
    // 2. Recover/promote already-authored backlog before asking Planner.
    // 3. Execute or retry one concrete task.
    // 4. Escalate to AI only when typed evidence says Rust cannot proceed.
    reopen_if_next_steps_unfinished(paths, state)?;
    sync_state_queue_with_next_steps(paths, state);
    automatic_recipe_harvest(paths, cfg, state)?;
    observe_repo_change_state(paths, state)?;
    pause_for_large_dirty_adoption_if_needed(paths, state)?;
    if human_disposition_pause_can_clear(paths, state) {
        println!(
            "[Team] Clearing human-disposition pause because active approved/retry work exists."
        );
        state.human_disposition_required = None;
        push_event(
            state,
            "rust",
            "human-disposition-pause-cleared",
            "Active approved or retry work appeared after a human-disposition pause.",
        );
    }
    if let Some(pause) = state.human_disposition_required.clone() {
        print_human_disposition_pause(&pause);
        if human_disposition_mode == HumanDispositionMode::Stop {
            push_event(
                state,
                "rust",
                "human-disposition-stop",
                "Non-interactive supervised loop stopped on a human-disposition pause.",
            );
            append_event(
                paths,
                OrchestratorEventKind::StateTransitionApplied,
                None,
                json!({
                    "transition": "non_interactive_human_disposition_stop",
                    "source": pause.source,
                    "reason": pause.reason,
                }),
            )?;
            return Ok(false);
        }
        let choice = match human_disposition_mode {
            HumanDispositionMode::Prompt => prompt_human_disposition_choice(&pause)?,
            HumanDispositionMode::Preset(choice) => choice,
            HumanDispositionMode::Stop => HumanDispositionChoice::Stop,
        };
        return apply_human_disposition_choice(paths, state, choice, &pause);
    }
    if let Some(should_continue) = run_next_non_ai_task(paths, cfg, state)? {
        return Ok(should_continue);
    }
    if let Some(should_continue) = skip_next_false_conditional_task(paths, cfg, state)? {
        return Ok(should_continue);
    }
    if state.project_complete {
        try_commit_control_doc_edits_with_log(
            paths,
            cfg,
            state,
            "Update completed project control docs",
        );
        return Ok(false);
    }
    let master_label = cfg.role_labels.master.as_str();
    let planner_label = cfg.role_labels.planner.as_str();
    let foreman_label = cfg.role_labels.foreman.as_str();
    let worker_label = cfg.role_labels.worker.as_str();

    let planner_due_before_recovery = state.planner_due;
    let recovered = recover_approved_next_steps_work(paths, state)?;
    if recovered > 0 {
        println!("[Team] Queued {recovered} approved task(s) from NEXT_STEPS.md.");
        if planner_due_before_recovery {
            let _ = record_rust_model_skip(
                paths,
                "planner",
                state.queue.first().map(|task| task.task_id.as_str()),
                cfg,
                "Project Manager skipped because Rust recovered approved NEXT_STEPS.md work",
            );
            state.planner_due = false;
        }
    }
    if recovered == 0
        && state.queue.is_empty()
        && state.master_due
        && !state.planner_due
        && !state.needs_reconciliation
    {
        // Dirty technical evidence from a just-finished Worker/retry should be
        // reconciled by the bounded source/test lane before asking CEO for new
        // direction. Tracking docs remain advisory context for this repair lane.
        let queued = try_queue_dirty_tree_technical_continuation(paths, cfg, state, runner)?;
        if state.pending_commit.is_some() {
            return Ok(false);
        }
        if queued > 0 {
            state.master_due = false;
            println!("[Team] Skipped CEO; queued {queued} dirty technical reconciliation task(s).");
        }
    }
    if recovered == 0 && state.queue.is_empty() && !state.master_due && !state.planner_due {
        // Planner may leave gated pending tasks in NEXT_STEPS.md. Rust no longer
        // approves those by itself; Senior Dev can approve a bounded technical
        // continuation batch, and Rust enforces it mechanically.
        let queued = try_queue_senior_batch_continuation(paths, cfg, state, runner, false)?;
        if state.pending_commit.is_some() {
            return Ok(false);
        }
        if queued > 0 {
            println!("[Team] Queued {queued} task(s) from Senior-approved backlog continuation.");
        }
        if state.progress_action_completed_this_step && state.queue.is_empty() {
            state.progress_action_completed_this_step = false;
            return Ok(!state.project_complete);
        }
    }

    if state.needs_reconciliation {
        // Reconciliation is for adopting an existing project state, not for
        // ordinary per-task bookkeeping.
        println!("[{planner_label}] Reconciling existing project docs...");
        let (mut plan, pm_changed_files) =
            run_project_manager_with_mutation_snapshot(paths, cfg, state, runner)?;
        normalize_planner_task_control_doc_scopes(paths, state, &mut plan);
        state.last_planner_summary = Some(plan.summary.clone());
        push_event(state, "planner", "reconcile", &plan.summary);
        persist_planner_work_plan(paths, state, &plan)?;
        apply_non_master_project_complete_claim(
            paths,
            state,
            "Project Manager",
            plan.project_complete,
            &plan.summary,
        )?;
        state.needs_reconciliation = false;
        match handle_pm_mutations_after_plan(paths, cfg, state, &plan, pm_changed_files)? {
            PmMutationDisposition::None => {}
            PmMutationDisposition::AutoCommitted => return Ok(!state.project_complete),
            PmMutationDisposition::Blocked => return Ok(false),
        }
        let pm_commits = handle_pm_commit_requests(paths, cfg, state, &plan)?;
        if state.pending_commit.is_some() {
            return Ok(false);
        }
        if pm_commits > 0 && plan.tasks.is_empty() {
            return Ok(!state.project_complete);
        }
        println!(
            "[{planner_label}] Reconciliation produced {} queued task(s)",
            plan.tasks.len()
        );
        if plan.tasks.is_empty() {
            settle_idle_escalation_if_no_active_work(paths, state)?;
        }
        if plan.needs_master {
            state.master_due = true;
            settle_idle_escalation_if_no_active_work(paths, state)?;
            return Ok(false);
        }
        for task in plan.tasks {
            enqueue_back(state, task);
        }
        if state.queue.is_empty() {
            if let Some(should_continue) = run_next_non_ai_task(paths, cfg, state)? {
                return Ok(should_continue);
            }
            if let Some(should_continue) = skip_next_false_conditional_task(paths, cfg, state)? {
                return Ok(should_continue);
            }
        }
        if state.project_complete {
            return Ok(false);
        }
    }

    if state.master_due && !state.queue.is_empty() && paths.roadmap.exists() {
        // Strategic direction is expensive and usually irrelevant while there is
        // already approved local work.  Clear stale CEO due flags and log the skip.
        let _gate = RoleCallGate::Skipped {
            reason: RoleSkipReason::ExistingBacklogAvailable,
        };
        let _ = record_rust_model_skip(
            paths,
            "master",
            state.queue.first().map(|task| task.task_id.as_str()),
            cfg,
            "CEO skipped because Rust has queued local work to execute",
        );
        state.master_due = false;
    }

    if state.master_due || !paths.roadmap.exists() {
        println!("[{master_label}] Updating roadmap / direction...");
        let master = run_ceo(paths, cfg, state, runner)?;
        state.project_complete = master.project_complete;
        state.master_due = false;
        state.planner_due = master.needs_planner && !master.project_complete;
        state.last_master_summary = Some(master.summary.clone());
        push_event(state, "master", &master.roadmap_status, &master.summary);
        println!("[{master_label}] Roadmap status: {}", master.roadmap_status);
        reopen_if_next_steps_unfinished(paths, state)?;
        settle_idle_escalation_if_no_active_work(paths, state)?;
        try_commit_control_doc_edits_with_log(paths, cfg, state, "Update CEO roadmap direction");
        if state.pending_commit.is_some() {
            return Ok(false);
        }
        if state.project_complete {
            return Ok(false);
        }
    }

    if state.queue.is_empty()
        && !state.planner_due
        && !state.needs_reconciliation
        && !state.master_due
    {
        // Second chance continuation after CEO/reconciliation may have changed docs.
        let queued = try_queue_senior_batch_continuation(paths, cfg, state, runner, true)?;
        if state.pending_commit.is_some() {
            return Ok(false);
        }
        if queued > 0 {
            println!("[Team] Queued {queued} task(s) from Senior-approved backlog continuation.");
        }
        if state.progress_action_completed_this_step && state.queue.is_empty() {
            state.progress_action_completed_this_step = false;
            return Ok(!state.project_complete);
        }
    }

    if state.planner_due && !state.queue.is_empty() {
        // The user explicitly wanted Planner avoidance when known work exists.
        // A queued task is stronger evidence than a stale planner_due flag.
        let _gate = RoleCallGate::Skipped {
            reason: RoleSkipReason::ExistingBacklogAvailable,
        };
        let _ = record_rust_model_skip(
            paths,
            "planner",
            state.queue.first().map(|task| task.task_id.as_str()),
            cfg,
            "Project Manager skipped because Rust has queued approved work",
        );
        state.planner_due = false;
    }

    if state.queue.is_empty()
        && state.planner_due
        && !state.master_due
        && !state.needs_reconciliation
    {
        let queued = try_queue_senior_batch_continuation(paths, cfg, state, runner, true)?;
        if state.pending_commit.is_some() {
            return Ok(false);
        }
        if queued > 0 {
            println!(
                "[Team] Skipped Project Manager; queued {queued} task(s) from Senior-approved backlog continuation."
            );
        }
        if state.progress_action_completed_this_step && state.queue.is_empty() {
            state.progress_action_completed_this_step = false;
            return Ok(!state.project_complete);
        }
    }

    if state.queue.is_empty()
        && !state.planner_due
        && !state.master_due
        && !state.needs_reconciliation
        && should_skip_ai_planner_when_idle(paths, state)?
    {
        println!(
            "[Team] Empty queue with no unfinished/approved active work and no escalation flags; stopping without Planner/CEO."
        );
        return Ok(false);
    }

    if state.queue.is_empty() || state.planner_due {
        // Planner is now the fallback: it runs only when Rust has no queued or
        // promotable work, or when policy/evidence says planning is required.
        println!("[{planner_label}] Compiling approved work orders...");
        let (mut plan, pm_changed_files) =
            run_project_manager_with_mutation_snapshot(paths, cfg, state, runner)?;
        normalize_planner_task_control_doc_scopes(paths, state, &mut plan);
        state.planner_due = false;
        state.last_planner_summary = Some(plan.summary.clone());
        push_event(state, "planner", "plan", &plan.summary);
        persist_planner_work_plan(paths, state, &plan)?;
        apply_non_master_project_complete_claim(
            paths,
            state,
            "Project Manager",
            plan.project_complete,
            &plan.summary,
        )?;
        match handle_pm_mutations_after_plan(paths, cfg, state, &plan, pm_changed_files)? {
            PmMutationDisposition::None => {}
            PmMutationDisposition::AutoCommitted => return Ok(!state.project_complete),
            PmMutationDisposition::Blocked => return Ok(false),
        }
        let pm_commits = handle_pm_commit_requests(paths, cfg, state, &plan)?;
        if state.pending_commit.is_some() {
            return Ok(false);
        }
        if pm_commits > 0 && plan.tasks.is_empty() {
            return Ok(!state.project_complete);
        }
        println!("[{planner_label}] Produced {} task(s)", plan.tasks.len());
        let next_steps_state = read_next_steps_queue_state(paths);
        if plan.tasks.is_empty() {
            if planner_requests_human_disposition_pause(&plan) {
                let pause = human_disposition_pause_from_planner(&plan);
                print_human_disposition_pause(&pause);
                state.human_disposition_required = Some(pause);
                state.master_due = false;
                state.planner_due = false;
                state.needs_reconciliation = false;
                state.retry_worker_same_task = None;
                state.retry_worker_brief = None;
                state.senior_dev_repair_order = None;
                push_event(state, "planner", "human-disposition-pause", &plan.summary);
                return Ok(false);
            }
            if let Some(candidate) = planner_zero_work_reconciliation_candidate(paths, &plan) {
                append_event(
                    paths,
                    OrchestratorEventKind::BatchCandidateFormed,
                    Some(&candidate.task_id),
                    json!({
                        "mode": "planner_zero_work_reconciliation",
                        "candidate_task_id": candidate.task_id,
                        "allowed_files": candidate.allowed_files,
                    }),
                )?;
                if try_auto_commit_pm_control_doc_reconciliation(paths, cfg, state, &candidate)? {
                    return Ok(!state.project_complete);
                }
                println!(
                    "[{foreman_label}] Evaluating PM-identified control-doc reconciliation {}...",
                    candidate.task_id
                );
                let approval = run_senior_dev_batch_approval(
                    paths,
                    cfg,
                    state,
                    std::slice::from_ref(&candidate),
                    runner,
                )?;
                let queued =
                    apply_senior_batch_approval(paths, cfg, state, &[candidate], approval)?;
                if state.pending_commit.is_some() {
                    return Ok(false);
                }
                if queued > 0 {
                    println!(
                        "[Team] Queued {queued} task(s) from Senior-approved PM reconciliation."
                    );
                }
            }
            settle_idle_escalation_if_no_active_work(paths, state)?;
        }
        if state.project_complete && next_steps_state.unfinished {
            println!(
                "[Team] Project Manager marked complete, but NEXT_STEPS.md still has unfinished work; escalating."
            );
            state.project_complete = false;
            state.master_due = true;
            return Ok(false);
        }
        if plan.needs_master {
            state.master_due = !state.project_complete;
            state.planner_due = false;
            settle_idle_escalation_if_no_active_work(paths, state)?;
            return Ok(false);
        }
        for task in plan.tasks {
            enqueue_back(state, task);
        }
        if state.queue.is_empty() {
            if let Some(should_continue) = run_next_non_ai_task(paths, cfg, state)? {
                return Ok(should_continue);
            }
            if let Some(should_continue) = skip_next_false_conditional_task(paths, cfg, state)? {
                return Ok(should_continue);
            }
        }
        if !state.project_complete
            && state.queue.is_empty()
            && !state.master_due
            && !next_steps_state.unfinished
            && !next_steps_non_ai_has_unfinished_task(paths)
        {
            println!(
                "[Team] Project Manager produced no work and no active queues remain; stopping without CEO escalation."
            );
            state.master_due = false;
            state.planner_due = false;
            return Ok(false);
        }
        if state.project_complete || state.queue.is_empty() {
            state.planner_due =
                !state.project_complete && read_next_steps_queue_state(paths).unfinished;
            settle_idle_escalation_if_no_active_work(paths, state)?;
            return Ok(false);
        }
    }

    // -----------------------------------------------------------------------
    // Worker path: one selected task → snapshot → guard/control-plane → verify
    // → accept/commit or repair route. Rust owns every durable side effect.
    // -----------------------------------------------------------------------
    let task = state.queue.remove(0);
    // Retry context is consumed exactly once for the matching task.  This keeps
    // stale repair briefs from contaminating unrelated future work.
    let retry_brief = if state.retry_worker_same_task.as_deref() == Some(task.task_id.as_str()) {
        state
            .retry_worker_brief
            .take()
            .filter(|brief| brief.task_id == task.task_id)
            .map(|brief| refresh_retry_brief_from_latest_worker_log(paths, &task, brief))
    } else {
        None
    };
    if state.retry_worker_same_task.as_deref() == Some(task.task_id.as_str()) {
        println!(
            "[Rust] Retrying Worker task {} from structured retry lane.",
            task.task_id
        );
        if let Some(brief) = &retry_brief {
            if let Some(summary) = &brief.diagnostic_summary {
                println!("[Rust] Retry diagnostic: {}", truncate_inline(summary, 220));
            } else if !brief.blocker.trim().is_empty() {
                println!(
                    "[Rust] Retry blocker brief: {}",
                    truncate_inline(&brief.blocker, 220)
                );
            }
        }
        state.retry_worker_same_task = None;
    }
    if state.senior_dev_repair_order.as_deref() == Some(task.task_id.as_str()) {
        state.senior_dev_repair_order = None;
    }
    let policy = task_policy_from_task(&task);
    let policy_findings = validate_task_policy(paths, &task, &policy);
    if !policy_findings.is_empty() {
        // Policy failures happen before Worker.  If the typed task is invalid,
        // do not spend a code-editing call trying to interpret prose around it.
        append_event(
            paths,
            OrchestratorEventKind::PolicyViolation,
            Some(&task.task_id),
            json!({"findings": policy_findings}),
        )?;
        let summary = format!(
            "{} has {} Rust policy violation(s); returning to Project Manager.",
            task.task_id,
            policy_findings.len()
        );
        push_event(state, "policy", "violation", &summary);
        expire_senior_batch(paths, state, "policy_violation", Some(&task))?;
        state.planner_due = true;
        return Ok(false);
    }
    if let Some(should_continue) = try_run_mechanistic_handoff_task(paths, cfg, state, &task)? {
        return Ok(should_continue);
    }
    write_task_card(paths, &task)?;
    append_event(
        paths,
        OrchestratorEventKind::TaskSelected,
        Some(&task.task_id),
        json!({"title": task.title.clone(), "kind": task.kind.clone()}),
    )?;
    let affected = compute_affected_work(paths, Some(&task))?;
    append_event(
        paths,
        OrchestratorEventKind::AffectedWorkComputed,
        Some(&task.task_id),
        json!({"affected": affected}),
    )?;
    let (deterministic_queue_path, deterministic_queue_label) =
        deterministic_task_queue_target(paths, &task);
    if let Some(result) = try_run_deterministic_task(
        paths,
        cfg,
        state,
        &task,
        deterministic_queue_path,
        deterministic_queue_label,
    )? {
        if state.pending_commit.is_some() {
            if !state
                .queue
                .iter()
                .any(|queued| queued.task_id == task.task_id)
            {
                state.queue.insert(0, task.clone());
            }
            state.last_review_summary = Some(format!(
                "{} is verified and paused for explicit commit approval.",
                task.task_id
            ));
            return Ok(false);
        }
        // Rust-only work can finish without Worker or Senior Dev.  This lane is
        // intentionally narrow: tracking-doc verification/status tasks only.
        state.last_worker_summary = Some(result.summary.clone());
        state.last_review_summary = Some(result.summary.clone());
        push_event(state, "deterministic", &result.status, &result.summary);
        if result.status == "done" {
            state.completed_tasks += 1;
            if cfg.master_every_completed_tasks > 0
                && state
                    .completed_tasks
                    .is_multiple_of(cfg.master_every_completed_tasks)
                && state.queue.is_empty()
            {
                state.master_due = true;
            }
        } else {
            let n = state.failed_tasks.entry(task.task_id.clone()).or_insert(0);
            *n += 1;
            if result.confidence != "high" {
                state.planner_due = true;
                return Ok(false);
            }
        }
        return Ok(!state.project_complete);
    }
    let acceptance_contract = acceptance_contract_from_task(&task);
    if let Some(should_continue) =
        try_block_worker_for_acceptance_contract(paths, cfg, state, &task, &acceptance_contract)?
    {
        return Ok(should_continue);
    }
    let scorecard = route_scorecard_for_task(
        paths,
        &task,
        ExecutionLane::SharedWorker,
        "worker_launch",
        None,
        0,
    );
    let _ = record_route_scorecard(paths, &scorecard);
    if let Some(should_continue) =
        try_honor_scorecard_soft_gate(paths, cfg, state, &task, &scorecard)?
    {
        return Ok(should_continue);
    }
    println!(
        "[{worker_label}] Executing task {}: {}",
        task.task_id, task.title
    );
    let support_brief = build_task_support_brief(paths, cfg, &task);
    let before_worker = snapshot_repo(paths, cfg)?;
    let before_control = snapshot_control_plane(paths)?;
    let worker_result = run_worker(
        paths,
        cfg,
        state,
        &task,
        &support_brief,
        retry_brief.as_ref(),
        runner,
    );
    let after_worker = snapshot_repo(paths, cfg)?;
    let after_control = snapshot_control_plane(paths)?;
    ensure_import_receipts_complete(paths, &state.queue, &state.imported_tasks)?;
    let control_mutations = diff_control_plane(&before_control, &after_control)
        .into_iter()
        // Rust refreshes the active Worker prompt/schema immediately before launch.
        .filter(|path| {
            path != ".orchestrator/prompts/worker.md"
                && path != ".orchestrator/schemas/worker.schema.json"
        })
        .collect::<Vec<_>>();
    if !control_mutations.is_empty() {
        let summary = format!(
            "{} mutated orchestrator control plane outside Rust authority: {}",
            task.task_id,
            control_mutations.join(", ")
        );
        println!("[Guard] Control-plane violation: {summary}");
        push_event(state, "guard", "control_plane_violation", &summary);
        append_event(
            paths,
            OrchestratorEventKind::PolicyViolation,
            Some(&task.task_id),
            json!({"mutations": control_mutations.clone()}),
        )?;
        expire_senior_batch(paths, state, "control_plane_violation", Some(&task))?;
        state.planner_due = true;
        return Err(anyhow!(summary));
    }
    let worker = worker_result?;
    let worker = enforce_worker_claimed_file_evidence(paths, worker);
    let guard_report = build_guard_report(cfg, &task, &before_worker, &after_worker)?;
    append_event(
        paths,
        OrchestratorEventKind::GuardChecked,
        Some(&task.task_id),
        json!({
            "status": guard_report.status.clone(),
            "changed_files": guard_report.changed_files.clone(),
            "violations": guard_report.violations.clone(),
        }),
    )?;
    if !guard_report.violations.is_empty() {
        let summary = format!(
            "{} changed files outside its allowed file set: {}",
            task.task_id,
            guard_report.violations.join(", ")
        );
        println!("[Guard] Violation: {summary}");
        push_event(state, "guard", "violation", &summary);
        expire_senior_batch(paths, state, "guard_violation", Some(&task))?;
    }
    state.last_worker_summary = Some(worker.summary.clone());
    push_event(state, "worker", &worker.status, &worker.summary);
    println!(
        "[{worker_label}] Task {} -> {}",
        task.task_id, worker.status
    );
    if let Some(summary) =
        apply_worker_orchestration_request(paths, cfg, state, &task, &worker, &guard_report)?
    {
        state.last_review_summary = Some(summary.clone());
        push_event(state, "rust-orchestration", "worker-directive", &summary);
        println!("[Rust] {summary}");
        return Ok(false);
    }

    let rust_verification = if worker.status == "done" {
        // Worker edits; Rust verifies.  Full final chains should flow through
        // WorkerOutput.handoff_to_rust and task tests, then be deduped here.
        run_task_verification_deterministically(paths, cfg, &task, &worker)?
    } else {
        json!({
            "status": "skipped",
            "reason": format!("Worker returned status `{}`", worker.status),
            "plan": null,
            "report": null,
            "commands": [],
            "rejected_commands": []
        })
    };
    if let Some(evidence_report) = evidence_report_from_value(&rust_verification) {
        if let Some(outcome) = maybe_auto_accept_worker_task(
            paths,
            cfg,
            state,
            &task,
            &worker,
            &guard_report,
            &evidence_report,
        )? {
            // Auto-accept means "Rust has clean scope + green evidence + low
            // risk", not "the model gets trusted."  It skips Senior Dev only for
            // boring, machine-checkable work.
            println!(
                "[Rust] Auto-accepted {}{}",
                task.task_id,
                outcome
                    .commit_sha
                    .as_ref()
                    .map(|sha| format!("; commit {}", truncate_inline(sha, 12)))
                    .unwrap_or_default()
            );
            if state.pending_commit.is_some() {
                if !state
                    .queue
                    .iter()
                    .any(|queued| queued.task_id == task.task_id)
                {
                    state.queue.insert(0, task.clone());
                }
                state.last_review_summary = Some(format!(
                    "{} is verified and paused for explicit commit approval.",
                    task.task_id
                ));
                return Ok(false);
            }
            state.project_complete = outcome.project_complete;
            state.last_review_summary = Some(outcome.summary.clone());
            push_event(state, "rust-review", &outcome.reason_code, &outcome.summary);
            if let Some(summary) =
                record_worker_deterministic_recipe(paths, &task, &worker, &rust_verification)?
            {
                println!("[Rust] {summary}");
                push_event(state, "deterministic", "recipe-candidate", &summary);
            }
            state.completed_tasks += 1;
            record_senior_batch_task_success(paths, state, &task)?;
            if cfg.master_every_completed_tasks > 0
                && state
                    .completed_tasks
                    .is_multiple_of(cfg.master_every_completed_tasks)
                && state.queue.is_empty()
                && state.senior_batch_approval.is_none()
            {
                state.master_due = true;
            }
            return Ok(!state.project_complete);
        }
    }

    if worker.status == "done" {
        if let Some(evidence_report) = evidence_report_from_value(&rust_verification) {
            if !evidence_report_is_clean(&evidence_report)
                && guard_report.violations.is_empty()
                && evidence_report.rejected_commands.is_empty()
            {
                if let Some(failed_command) = first_failed_command(&evidence_report) {
                    let failure_count = {
                        let n = state.failed_tasks.entry(task.task_id.clone()).or_insert(0);
                        *n += 1;
                        *n
                    };
                    let fingerprint = failure_fingerprint_from_command_evidence(failed_command);
                    let fingerprint_key = format!("{}|{}", task.task_id, fingerprint.key());
                    let fingerprint_retry_count = {
                        let n = state
                            .failed_fingerprints
                            .entry(fingerprint_key)
                            .or_insert(0);
                        *n += 1;
                        *n
                    };
                    let mut retry_brief = build_verification_failure_retry_brief(
                        paths,
                        &task,
                        &worker,
                        &guard_report,
                        &evidence_report,
                        failed_command,
                        failure_count,
                        fingerprint_retry_count,
                    );
                    let route = route_worker_done_verification_failure(
                        &evidence_report,
                        &guard_report,
                        failed_command,
                        failure_count,
                        cfg.max_task_failures,
                        retry_brief.fingerprint.as_ref(),
                        fingerprint_retry_count,
                    );
                    retry_brief.route = route.as_str().to_string();
                    let route_label = route.as_str();
                    let summary = format!(
                        "{} returned `done`, but Rust verification failed; route: `{route_label}`; fingerprint: `{}`; failed command: {}",
                        task.task_id,
                        retry_brief
                            .fingerprint
                            .as_ref()
                            .map(|fingerprint| fingerprint.kind.as_str())
                            .unwrap_or("unknown"),
                        truncate_inline(&failed_command.command_display, 180)
                    );
                    println!("[Rust] {summary}");
                    state.last_review_summary = Some(summary.clone());
                    push_event(state, "rust-review", "done-verification-failed", &summary);
                    let fingerprint_key_for_regret = retry_brief
                        .fingerprint
                        .as_ref()
                        .map(FailureFingerprint::key);
                    let _ = append_event(
                        paths,
                        OrchestratorEventKind::StateTransitionApplied,
                        Some(&task.task_id),
                        json!({
                            "route": route_label,
                            "reason": "worker_done_verification_failed",
                            "failure_count": failure_count,
                            "failed_command": failed_command,
                            "failure_fingerprint": retry_brief.fingerprint.clone(),
                            "fingerprint_retry_count": fingerprint_retry_count,
                        }),
                    );
                    if route != WorkerNonDoneRoute::RetryWorkerSameTask
                        && fingerprint_retry_count > 1
                    {
                        let _ = record_route_regret_decision(
                            paths,
                            Some(&task.task_id),
                            RouteRegretKind::RepeatedFingerprintNoProgress,
                            ExecutionLane::SeniorDecision,
                            fingerprint_key_for_regret.as_deref(),
                            &summary,
                        );
                    }

                    if route == WorkerNonDoneRoute::RetryWorkerSameTask {
                        let _gate = RoleCallGate::Skipped {
                            reason: RoleSkipReason::MechanicalRepairRetry,
                        };
                        if !state
                            .queue
                            .iter()
                            .any(|queued| queued.task_id == task.task_id)
                        {
                            state.queue.insert(0, task.clone());
                        }
                        state.retry_worker_same_task = Some(task.task_id.clone());
                        let retry_brief = annotate_retry_brief_progress(state, retry_brief);
                        state.retry_worker_brief = Some(retry_brief);
                        state.senior_dev_repair_order = None;
                        state.planner_due = false;
                        state.master_due = false;
                        let _ = record_rust_model_skip(
                            paths,
                            "foreman-review",
                            Some(&task.task_id),
                            cfg,
                            "done_worker_verification_failed_retry",
                        );
                        return Ok(false);
                    }

                    if route == WorkerNonDoneRoute::SeniorDevRepairOrder {
                        expire_senior_batch(
                            paths,
                            state,
                            "verification_failure_needs_senior_repair",
                            Some(&task),
                        )?;
                        state.retry_worker_same_task = None;
                        state.retry_worker_brief = None;
                        state.senior_dev_repair_order = None;
                        println!(
                            "[{foreman_label}] Requesting bounded verification repair decision for {}...",
                            task.task_id
                        );
                        append_event(
                            paths,
                            OrchestratorEventKind::SeniorReviewRequested,
                            Some(&task.task_id),
                            json!({
                                "worker_status": worker.status.clone(),
                                "mode": "verification_repair",
                                "failure_fingerprint": retry_brief.fingerprint.clone(),
                            }),
                        )?;
                        let repair = run_senior_dev_repair(
                            paths,
                            cfg,
                            state,
                            &task,
                            &worker,
                            &guard_report,
                            &rust_verification,
                            &retry_brief,
                            runner,
                        )?;
                        apply_senior_repair_decision(paths, state, &task, repair, retry_brief)?;
                        return Ok(false);
                    }

                    expire_senior_batch(
                        paths,
                        state,
                        "verification_failure_needs_planning",
                        Some(&task),
                    )?;
                    state.retry_worker_same_task = None;
                    state.retry_worker_brief = None;
                    state.senior_dev_repair_order = None;
                    state.planner_due = true;
                    let _ = record_rust_model_skip(
                        paths,
                        "foreman-review",
                        Some(&task.task_id),
                        cfg,
                        "done_worker_verification_failed_planner_route",
                    );
                    return Ok(false);
                }
            }
        }
    }

    if worker.status != "done" {
        // Non-done work enters the repair router.  Compile/blocker cases should
        // usually retry Worker with a small Rust-generated brief; Senior Dev is
        // reserved for retry exhaustion or ambiguous judgment.
        let blocker = worker.blocker.trim();
        let failure_count = {
            let n = state.failed_tasks.entry(task.task_id.clone()).or_insert(0);
            *n += 1;
            *n
        };
        let mut retry_brief = build_worker_retry_brief(
            paths,
            cfg,
            &task,
            &worker,
            &guard_report,
            WorkerRetryOptions {
                attempt: failure_count,
                fingerprint_retry_count: 0,
                run_diagnostic: guard_report.violations.is_empty(),
            },
        )?;
        let fingerprint = retry_brief
            .fingerprint
            .clone()
            .unwrap_or_else(|| failure_fingerprint_from_worker(&worker, &guard_report));
        let fingerprint_key = format!("{}|{}", task.task_id, fingerprint.key());
        let fingerprint_retry_count = {
            let n = state
                .failed_fingerprints
                .entry(fingerprint_key)
                .or_insert(0);
            *n += 1;
            *n
        };
        retry_brief.fingerprint_retry_count = fingerprint_retry_count;
        retry_brief.repair_instruction = repair_instruction_for_fingerprint(&task, &fingerprint);
        if let Some(hint) = &retry_brief.patch_context_hint {
            retry_brief.repair_instruction.push(' ');
            retry_brief.repair_instruction.push_str(hint);
        }
        let retry_brief = annotate_retry_brief_progress(state, retry_brief);
        let route = route_worker_non_done(
            &worker,
            &guard_report,
            failure_count,
            cfg.max_task_failures,
            Some(&fingerprint),
            fingerprint_retry_count,
        );
        let route_label = route.as_str();
        let summary = if blocker.is_empty() {
            format!(
                "{} returned `{}` before deterministic verification; Rust route: `{route_label}`; fingerprint: `{}`.",
                task.task_id,
                worker.status,
                fingerprint.kind.as_str()
            )
        } else {
            format!(
                "{} returned `{}` before deterministic verification; Rust route: `{route_label}`; fingerprint: `{}`. Blocker: {}",
                task.task_id,
                worker.status,
                fingerprint.kind.as_str(),
                truncate_inline(blocker, 500)
            )
        };
        println!("[Rust] {summary}");
        state.last_review_summary = Some(summary.clone());
        push_event(state, "rust-review", "worker-not-done", &summary);
        let fingerprint_key_for_regret = fingerprint.key();
        let _ = append_event(
            paths,
            OrchestratorEventKind::StateTransitionApplied,
            Some(&task.task_id),
            json!({
                "route": route_label,
                "reason": "worker_not_done",
                "worker_status": worker.status.clone(),
                "failure_count": failure_count,
                "failure_fingerprint": fingerprint,
                "fingerprint_retry_count": fingerprint_retry_count,
            }),
        );
        if route != WorkerNonDoneRoute::RetryWorkerSameTask && fingerprint_retry_count > 1 {
            let _ = record_route_regret_decision(
                paths,
                Some(&task.task_id),
                RouteRegretKind::RepeatedFingerprintNoProgress,
                ExecutionLane::SeniorDecision,
                Some(&fingerprint_key_for_regret),
                &summary,
            );
        }
        if route == WorkerNonDoneRoute::RetryWorkerSameTask {
            // Keep the dirty tree and immediately requeue the same task.  This is
            // how mechanical compile fixes avoid a Senior Dev token spend.
            let _gate = RoleCallGate::Skipped {
                reason: RoleSkipReason::MechanicalRepairRetry,
            };
            if !state
                .queue
                .iter()
                .any(|queued| queued.task_id == task.task_id)
            {
                state.queue.insert(0, task.clone());
            }
            state.retry_worker_same_task = Some(task.task_id.clone());
            state.retry_worker_brief = Some(annotate_retry_brief_progress(state, retry_brief));
            state.senior_dev_repair_order = None;
            state.planner_due = false;
            state.master_due = false;
            let _ = record_rust_model_skip(
                paths,
                "foreman-review",
                Some(&task.task_id),
                cfg,
                "Worker returned non-done status; Rust scheduled same-task Worker retry",
            );
            return Ok(false);
        }
        if route == WorkerNonDoneRoute::SeniorDevRepairOrder {
            // Senior repair mode sees a compact decision packet and cannot accept
            // work.  It only selects the next safe route.
            expire_senior_batch(
                paths,
                state,
                "worker_failure_needs_senior_repair",
                Some(&task),
            )?;
            state.retry_worker_same_task = None;
            state.retry_worker_brief = None;
            state.senior_dev_repair_order = None;
            println!(
                "[{foreman_label}] Requesting bounded repair decision for {}...",
                task.task_id
            );
            append_event(
                paths,
                OrchestratorEventKind::SeniorReviewRequested,
                Some(&task.task_id),
                json!({
                    "worker_status": worker.status.clone(),
                    "mode": "repair",
                    "failure_fingerprint": retry_brief.fingerprint.clone(),
                }),
            )?;
            let repair = run_senior_dev_repair(
                paths,
                cfg,
                state,
                &task,
                &worker,
                &guard_report,
                &rust_verification,
                &retry_brief,
                runner,
            )?;
            apply_senior_repair_decision(paths, state, &task, repair, retry_brief)?;
            return Ok(false);
        } else {
            expire_senior_batch(paths, state, "worker_failure_needs_planning", Some(&task))?;
            state.retry_worker_same_task = None;
            state.retry_worker_brief = None;
            state.planner_due = true;
            let skip_reason = if fingerprint.is_environment_or_terminal() {
                RoleSkipReason::EnvironmentBlockerRecorded
            } else {
                RoleSkipReason::GuardViolationNeedsPlanning
            };
            let _gate = RoleCallGate::Skipped {
                reason: skip_reason,
            };
            let _ = record_rust_model_skip(
                paths,
                "foreman-review",
                Some(&task.task_id),
                cfg,
                &format!(
                    "Worker returned non-done status; Rust routed to planner ({})",
                    skip_reason.as_str()
                ),
            );
            return Ok(false);
        }
    }

    if state.senior_dev_repair_order.as_deref() == Some(task.task_id.as_str()) {
        println!(
            "[{foreman_label}] Preparing repair-order review for {}...",
            task.task_id
        );
    }
    println!("[{foreman_label}] Reviewing {worker_label} output...");
    // Normal Senior review remains the fallback for high/unknown risk, failed
    // verification, or work not eligible for Rust auto-accept.
    append_event(
        paths,
        OrchestratorEventKind::SeniorReviewRequested,
        Some(&task.task_id),
        json!({"worker_status": worker.status.clone()}),
    )?;
    let mut review = run_senior_dev_review(
        paths,
        cfg,
        state,
        &task,
        &worker,
        &guard_report,
        rust_verification.clone(),
        runner,
    )?;
    validate_foreman_review_for_task(paths, &task, &mut review);
    if review.accepted {
        if let Some(evidence_report) = evidence_report_from_value(&rust_verification) {
            // Even after Senior accepts, Rust still owns the state transition:
            // mark done, append evidence notes, stage allowed files, and commit.
            let reason_code = if review.reason_code.trim().is_empty() {
                "senior_dev_accepted"
            } else {
                review.reason_code.trim()
            };
            match rust_accept_worker_task(
                paths,
                cfg,
                state,
                WorkerAcceptance {
                    task: &task,
                    guard_report: &guard_report,
                    evidence_report: &evidence_report,
                    reason_code,
                    commit_message: &format!("Complete {}", task.task_id),
                },
            ) {
                Ok(commit_sha) => {
                    review.commit_created = commit_sha.is_some();
                    review.commit_sha = commit_sha.unwrap_or_default();
                }
                Err(err) => {
                    review.accepted = false;
                    review.needs_planner = true;
                    review.problems.push(err.to_string());
                    review.summary = format!(
                        "Senior Dev accepted {}, but Rust-owned bookkeeping/commit failed: {err}",
                        task.task_id
                    );
                }
            }
        } else {
            review.accepted = false;
            review.needs_planner = true;
            review.summary = format!(
                "Senior Dev accepted {}, but Rust verification evidence was unavailable.",
                task.task_id
            );
        }
    }
    if state.pending_commit.is_some() {
        if !state
            .queue
            .iter()
            .any(|queued| queued.task_id == task.task_id)
        {
            state.queue.insert(0, task.clone());
        }
        state.last_review_summary = Some(format!(
            "{} was accepted and is paused for explicit commit approval.",
            task.task_id
        ));
        return Ok(false);
    }
    state.last_review_summary = Some(review.summary.clone());
    push_event(state, "foreman", "review", &review.summary);
    apply_non_master_project_complete_claim(
        paths,
        state,
        "Senior Dev",
        review.project_complete,
        &review.summary,
    )?;
    println!(
        "[{foreman_label}] Review: {}{}",
        if review.accepted {
            "accepted"
        } else {
            "rejected"
        },
        if review.commit_created {
            format!("; commit {}", truncate_inline(&review.commit_sha, 12))
        } else {
            String::new()
        }
    );

    if worker.status == "done" && (!review.accepted || !guard_report.violations.is_empty()) {
        let n = state.failed_tasks.entry(task.task_id.clone()).or_insert(0);
        *n += 1;
    }

    if review.accepted {
        state.senior_dev_repair_order = None;
        state.retry_worker_same_task = None;
        let _ = record_positive_route_learning(
            paths,
            &task,
            ExecutionLane::SeniorDecision,
            "senior_accepted_verified_worker",
            &review.summary,
        );
        if let Some(summary) =
            record_worker_deterministic_recipe(paths, &task, &worker, &rust_verification)?
        {
            println!("[Rust] {summary}");
            push_event(state, "deterministic", "recipe-candidate", &summary);
        }
        state.completed_tasks += 1;
        record_senior_batch_task_success(paths, state, &task)?;
        if cfg.master_every_completed_tasks > 0
            && state
                .completed_tasks
                .is_multiple_of(cfg.master_every_completed_tasks)
            && state.queue.is_empty()
            && state.senior_batch_approval.is_none()
        {
            state.master_due = true;
        }
    }

    if !review.replacement_tasks.is_empty() || !review.followup_tasks.is_empty() {
        let summary = "Senior Dev returned task definitions, but only Project Manager may manufacture work orders; sending back to Project Manager.";
        push_event(state, "foreman", "task-definition-rejected", summary);
        expire_senior_batch(
            paths,
            state,
            "senior_returned_task_definitions",
            Some(&task),
        )?;
        state.planner_due = true;
    }

    let task_failures = *state.failed_tasks.get(&task.task_id).unwrap_or(&0);
    if review.needs_planner || (!review.accepted && !review.needs_master) {
        state.senior_dev_repair_order = None;
        expire_senior_batch(paths, state, "senior_review_needs_planner", Some(&task))?;
        state.planner_due = true;
        return Ok(false);
    }
    if review.needs_master || task_failures > cfg.max_task_failures {
        state.senior_dev_repair_order = None;
        expire_senior_batch(paths, state, "senior_review_needs_master", Some(&task))?;
        state.master_due = true;
        return Ok(false);
    }

    Ok(!state.project_complete)
}

fn enqueue_back(state: &mut State, task: TaskSpec) -> bool {
    if !task.status.eq_ignore_ascii_case("approved") {
        return false;
    }
    if !state
        .queue
        .iter()
        .any(|existing| existing.task_id == task.task_id)
    {
        state.queue.push(task);
        true
    } else {
        false
    }
}

// ---------------------------------------------------------------------------
// Queue recovery, promotion, and deterministic task lanes
// ---------------------------------------------------------------------------
//
// Rust reads Markdown queues because they are still the human-facing source of
// task state.  These helpers recover approved work, promote dependency-satisfied
// pending work, and drain non-AI verification/bookkeeping tasks before any model
// call is considered.

fn enqueue_approved_next_steps_tasks(paths: &Paths, state: &mut State) -> Result<usize> {
    let text = read_text_or_empty(&paths.next_steps);
    let tasks = parse_approved_next_steps_tasks(&text);
    let mut queued = 0;
    for task in tasks {
        // A digest-bound imported task is state-owned.  Never manufacture a
        // second copy from the human-facing Markdown queue, even after the
        // imported task has completed and its immutable origin record remains.
        if state.imported_tasks.contains_key(&task.task_id) {
            continue;
        }
        if task_suppressed_by_preserved_dirty_baseline(state, &task) {
            continue;
        }
        if enqueue_back(state, task) {
            queued += 1;
        }
    }
    Ok(queued)
}

fn sync_state_queue_with_next_steps(paths: &Paths, state: &mut State) -> usize {
    if state.queue.is_empty() || !paths.next_steps.exists() {
        return 0;
    }
    let text = read_text_or_empty(&paths.next_steps);
    let approved_ids: HashSet<String> = parse_approved_next_steps_tasks(&text)
        .into_iter()
        .filter(|task| !task_suppressed_by_preserved_dirty_baseline(state, task))
        .map(|task| task.task_id)
        .collect();
    let before = state.queue.len();
    let preserves_dirty = preserves_dirty_baseline(state);
    state.queue.retain(|task| {
        state.imported_tasks.contains_key(&task.task_id)
            || (approved_ids.contains(&task.task_id)
                && !(preserves_dirty && task.task_id.starts_with("DIRTY-")))
    });
    before - state.queue.len()
}

fn try_queue_senior_batch_continuation(
    paths: &Paths,
    cfg: &Config,
    state: &mut State,
    runner: &dyn CodexRunner,
    log_no_candidates: bool,
) -> Result<usize> {
    if !paths.next_steps.exists()
        || !state.queue.is_empty()
        || state.master_due
        || state.needs_reconciliation
    {
        return Ok(0);
    }
    let text = read_text_or_empty(&paths.next_steps);
    if parse_approved_next_steps_tasks(&text)
        .into_iter()
        .any(|task| !task_suppressed_by_preserved_dirty_baseline(state, &task))
    {
        return Ok(0);
    }

    if state.senior_batch_approval.is_some() {
        let queued = queue_next_task_from_active_senior_batch(paths, cfg, state)?;
        if queued > 0 {
            return Ok(queued);
        }
    }

    let candidates = senior_batch_candidate_tasks_from_text(&text);
    if candidates.is_empty() {
        let queued = try_queue_dirty_tree_technical_continuation(paths, cfg, state, runner)?;
        if queued > 0 {
            return Ok(queued);
        }
        if log_no_candidates {
            println!(
                "[Rust] Skipped Sr. Dev approval: no eligible pending Worker tasks in NEXT_STEPS.md."
            );
            let _ = record_rust_model_skip(
                paths,
                "foreman-review",
                None,
                cfg,
                "no eligible pending Worker tasks in NEXT_STEPS.md",
            );
        }
        return Ok(0);
    }
    append_event(
        paths,
        OrchestratorEventKind::BatchCandidateFormed,
        None,
        json!({
            "mode": "senior_batch_approval",
            "candidate_task_ids": candidates
                .iter()
                .map(|task| task.task_id.clone())
                .collect::<Vec<_>>(),
        }),
    )?;

    println!(
        "[{}] Evaluating {} backlog task(s) for technical batch continuation...",
        cfg.role_labels.foreman,
        candidates.len()
    );
    let approval = run_senior_dev_batch_approval(paths, cfg, state, &candidates, runner)?;
    apply_senior_batch_approval(paths, cfg, state, &candidates, approval)
}

fn try_queue_dirty_tree_technical_continuation(
    paths: &Paths,
    cfg: &Config,
    state: &mut State,
    runner: &dyn CodexRunner,
) -> Result<usize> {
    if !paths.next_steps.exists() || !state.queue.is_empty() || state.needs_reconciliation {
        return Ok(0);
    }
    if preserves_dirty_baseline(state) {
        return Ok(0);
    }
    let Some(candidate) = dirty_tree_continuation_candidate(paths, state)? else {
        return Ok(0);
    };
    let policy = task_policy_from_task(&candidate);
    let policy_findings = validate_task_policy(paths, &candidate, &policy);
    if !policy_findings.is_empty() {
        append_event(
            paths,
            OrchestratorEventKind::PolicyViolation,
            Some(&candidate.task_id),
            json!({
                "mode": "senior_dirty_tree_continuation_precheck",
                "findings": policy_findings,
            }),
        )?;
        let summary = format!(
            "{} dirty-tree continuation has {} Rust policy violation(s); returning to Project Manager before Senior review.",
            candidate.task_id,
            policy_findings.len()
        );
        push_event(state, "policy", "dirty_continuation_rejected", &summary);
        state.planner_due = true;
        return Ok(0);
    }
    append_event(
        paths,
        OrchestratorEventKind::BatchCandidateFormed,
        Some(&candidate.task_id),
        json!({
            "mode": "senior_dirty_tree_continuation",
            "candidate_task_id": candidate.task_id,
            "allowed_files": candidate.allowed_files,
        }),
    )?;
    println!(
        "[{}] Evaluating dirty-tree technical continuation {}...",
        cfg.role_labels.foreman, candidate.task_id
    );
    let approval =
        run_senior_dev_batch_approval(paths, cfg, state, std::slice::from_ref(&candidate), runner)?;
    apply_senior_batch_approval(paths, cfg, state, &[candidate], approval)
}

fn senior_batch_candidate_tasks_from_text(text: &str) -> Vec<TaskSpec> {
    let done_ids = next_steps_completed_task_ids(text);
    let outputs = next_steps_outputs(text);
    parse_pending_next_steps_tasks(text)
        .into_iter()
        .filter(|task| pending_task_dependencies_satisfied(task, &done_ids, &outputs))
        .filter(pending_task_ready_for_senior_batch)
        .collect()
}

fn pending_task_ready_for_senior_batch(task: &TaskSpec) -> bool {
    pending_task_ready_for_rust_approval(task) && !task.requires_network
}

fn dirty_tree_continuation_candidate(paths: &Paths, state: &State) -> Result<Option<TaskSpec>> {
    if preserves_dirty_baseline(state) {
        return Ok(None);
    }
    let dirty_files = git_dirty_name_only(paths)?;
    if dirty_files.is_empty() {
        return Ok(None);
    }
    let candidate_files = dirty_files
        .into_iter()
        .filter(|path| !path.starts_with(".orchestrator/"))
        .collect::<Vec<_>>();
    if candidate_files.is_empty()
        || candidate_files.len() > 12
        || candidate_files
            .iter()
            .any(|path| !dirty_tree_continuation_file_allowed(path))
    {
        return Ok(None);
    }
    let mut allowed_files = candidate_files
        .into_iter()
        .filter(|path| dirty_tree_continuation_technical_file(path))
        .collect::<Vec<_>>();
    if allowed_files.is_empty() {
        return Ok(None);
    }
    allowed_files.sort();
    allowed_files.dedup();
    let tests_to_run = dirty_tree_verification_commands(paths, &allowed_files);
    if tests_to_run.is_empty()
        || !tests_to_run
            .iter()
            .all(|command| is_deterministic_command_allowed(command))
    {
        return Ok(None);
    }

    let task_id = format!("DIRTY-{}", Utc::now().timestamp_millis());
    let previous_summary = state
        .last_review_summary
        .as_deref()
        .or(state.last_worker_summary.as_deref())
        .unwrap_or("recent accepted or rejected technical evidence");
    Ok(Some(TaskSpec {
        task_id: task_id.clone(),
        parent_task: None,
        status: "approved".to_string(),
        priority: "normal".to_string(),
        review_tier: ReviewTier::AiReviewOnFailure,
        automation_level: AutomationLevel::AiWorkerRustReview,
        dependencies: Vec::new(),
        title: "Reconcile Dirty Technical Evidence".to_string(),
        kind: "repair".to_string(),
        objective: format!(
            "Reconcile the current dirty source/test evidence set without changing project direction. Dirty files: {}.",
            allowed_files.join(", ")
        ),
        why_unblocks_progress: format!(
            "Rust detected a dirty technical evidence set after prior work: {}",
            truncate_inline(previous_summary, 300)
        ),
        allowed_files,
        read_only_files: Vec::new(),
        forbidden_actions: vec![
            "Do not add public sockets, service exposure, external publishing, production transport, or client traffic.".to_string(),
            "Do not broaden beyond the dirty files named by this task.".to_string(),
            "Do not edit tracking docs; Rust owns bookkeeping after acceptance.".to_string(),
        ],
        exact_steps: vec![
            "Continue from the current dirty tree.".to_string(),
            "Inspect only the dirty allowed files and make the smallest repair/reconciliation needed for the existing local evidence to pass.".to_string(),
            "Do not add new roadmap direction, new task scope, or unrelated cleanup.".to_string(),
            "If the dirty evidence requires files or behavior outside the allowed set, return blocked.".to_string(),
        ],
        success_criteria: vec![
            "The dirty source/test evidence is internally consistent and bounded to the allowed files.".to_string(),
            "Rust final verification passes for the listed commands.".to_string(),
            "No additional files outside the task envelope are changed by Worker.".to_string(),
        ],
        tests_to_run,
        stop_conditions: vec![
            "Stop if repair requires files outside the allowed dirty set.".to_string(),
            "Stop if repair requires product direction, architecture judgment, or live/public behavior.".to_string(),
            "Stop if verification requires commands outside the deterministic allowlist.".to_string(),
        ],
        discovery_budget: "Read only the dirty allowed files unless a stop condition requires escalation.".to_string(),
        estimated_minutes: 30,
        requires_network: false,
        prefer_worktree: None,
    }))
}

fn dirty_tree_continuation_file_allowed(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    if lower.contains("cargo.toml")
        || lower.contains("cargo.lock")
        || lower.contains("package.swift")
        || lower.contains("package.resolved")
        || lower.contains(".xcodeproj/")
        || lower.contains(".xcworkspace/")
    {
        return false;
    }
    lower.starts_with("sources/")
        || lower.starts_with("src/")
        || lower.starts_with("tests/")
        || lower.starts_with("test/")
        || is_tracking_doc_path(path)
        || lower.starts_with("docs/")
        || lower.ends_with(".md")
}

fn dirty_tree_continuation_technical_file(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.starts_with("sources/")
        || lower.starts_with("src/")
        || lower.starts_with("tests/")
        || lower.starts_with("test/")
}

fn dirty_tree_verification_commands(paths: &Paths, files: &[String]) -> Vec<String> {
    let mut commands = Vec::new();
    let has_swift = files.iter().any(|path| {
        path.ends_with(".swift") || path.starts_with("Sources/") || path.starts_with("Tests/")
    }) || paths.root.join("Package.swift").exists();
    let has_rust = files.iter().any(|path| {
        path.ends_with(".rs") || path.starts_with("src/") || path.starts_with("tests/")
    }) || paths.root.join("Cargo.toml").exists();

    if has_swift {
        let filters = files
            .iter()
            .filter_map(|path| swift_test_filter_from_path(path))
            .collect::<BTreeSet<_>>();
        commands.extend(
            filters
                .into_iter()
                .map(|filter| format!("swift test --filter {filter}")),
        );
        if commands.is_empty() && files.iter().any(|path| path.starts_with("Tests/")) {
            commands.push("swift test".to_string());
        }
        if files.iter().any(|path| {
            path.ends_with(".swift") || path.starts_with("Sources/") || path == "Package.swift"
        }) {
            commands.push("swift build".to_string());
        }
    } else if has_rust {
        commands.push("cargo test --quiet".to_string());
        commands.push("cargo build".to_string());
    }
    commands.push("git diff --check".to_string());
    commands.sort();
    commands.dedup();
    commands
}

pub(crate) fn swift_test_filter_from_path(path: &str) -> Option<String> {
    let lower = path.to_ascii_lowercase();
    if !(lower.starts_with("tests/") || lower.starts_with("test/")) || !lower.ends_with(".swift") {
        return None;
    }
    let stem = Path::new(path).file_stem()?.to_str()?.trim();
    (!stem.is_empty()).then(|| stem.to_string())
}

fn planner_zero_work_reconciliation_candidate(
    paths: &Paths,
    plan: &ProjectManagerOutput,
) -> Option<TaskSpec> {
    if plan.project_complete || plan.needs_master || !plan.tasks.is_empty() {
        return None;
    }
    let planner_text = std::iter::once(plan.summary.as_str())
        .chain(plan.warnings.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join("\n");
    let lower = planner_text.to_ascii_lowercase();
    let describes_reconciliation = lower.contains("reconcile")
        || lower.contains("reconciling")
        || lower.contains("stale")
        || lower.contains("contradiction")
        || lower.contains("out of sync")
        || lower.contains("queue heads");
    let mentions_control_docs = lower.contains("control-doc")
        || lower.contains("control doc")
        || lower.contains("roadmap.md")
        || lower.contains("next_steps")
        || lower.contains("continuity.md")
        || lower.contains("task.md");
    if !describes_reconciliation || !mentions_control_docs {
        return None;
    }

    let allowed_files = mentioned_control_doc_paths(paths, &planner_text);
    if allowed_files.is_empty() {
        return None;
    }
    let task_id = format!("RECONCILE-{}", Utc::now().timestamp_millis());
    Some(TaskSpec {
        task_id,
        parent_task: None,
        status: "approved".to_string(),
        priority: "normal".to_string(),
        review_tier: ReviewTier::AiReviewOnFailure,
        automation_level: AutomationLevel::AiWorkerRustReview,
        dependencies: Vec::new(),
        title: "Reconcile PM-Identified Control Docs".to_string(),
        kind: "docs_only".to_string(),
        objective: format!(
            "Reconcile the PM-identified stale control-doc state without approving new product direction. PM summary: {}",
            truncate_inline(&plan.summary, 400)
        ),
        why_unblocks_progress: "The Project Manager found stale control-doc state but emitted no executable task; Senior can gate this bounded docs-only continuation so Rust does not loop back into PM.".to_string(),
        allowed_files,
        read_only_files: Vec::new(),
        forbidden_actions: vec![
            "Do not edit source or test files.".to_string(),
            "Do not approve new runtime behavior, public sockets, service exposure, external publishing, production transport, or client traffic.".to_string(),
            "Do not invent new roadmap direction; reconcile only the stale facts PM identified.".to_string(),
        ],
        exact_steps: vec![
            "Continue from the current tree and inspect only the allowed control docs.".to_string(),
            "Reconcile stale queue/phase wording to the newest accepted task evidence already recorded in the control docs.".to_string(),
            "If reconciliation requires source/test changes or new product direction, return blocked.".to_string(),
        ],
        success_criteria: vec![
            "The named control docs no longer contradict the newest accepted task state.".to_string(),
            "No source/test/runtime behavior changes are made.".to_string(),
            "Rust final verification passes.".to_string(),
        ],
        tests_to_run: vec!["git diff --check".to_string()],
        stop_conditions: vec![
            "Stop if required edits are outside the allowed control docs.".to_string(),
            "Stop if the latest accepted state cannot be determined from existing docs.".to_string(),
            "Stop if the task would approve implementation work rather than reconcile docs.".to_string(),
        ],
        discovery_budget: "Read only the allowed control docs unless a stop condition requires escalation.".to_string(),
        estimated_minutes: 20,
        requires_network: false,
        prefer_worktree: None,
    })
}

fn planner_requests_human_disposition_pause(plan: &ProjectManagerOutput) -> bool {
    if !plan.tasks.is_empty() {
        return false;
    }
    let text = std::iter::once(plan.summary.as_str())
        .chain(plan.warnings.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join("\n")
        .to_ascii_lowercase();
    let mentions_disposition = text.contains("human disposition")
        || text.contains("human/ceo disposition")
        || text.contains("master/human should choose")
        || text.contains("must choose")
        || text.contains("choose one explicit")
        || text.contains("disposition is decided")
        || text.contains("until disposition");
    let mentions_quarantine_or_dirty = text.contains("quarantined")
        || text.contains("quarantine")
        || text.contains("dirty set")
        || text.contains("dirty-set")
        || text.contains("dirty files")
        || text.contains("dirty worktree");
    let says_no_work = text.contains("both queues should remain empty")
        || text.contains("keep both queues empty")
        || text.contains("no new tasks")
        || text.contains("no worker or deterministic tasks")
        || text.contains("no worker or rust task")
        || text.contains("no further worker")
        || text.contains("no deterministic work");

    mentions_disposition && (mentions_quarantine_or_dirty || says_no_work)
}

fn human_disposition_pause_from_planner(plan: &ProjectManagerOutput) -> HumanDispositionPause {
    HumanDispositionPause {
        reason: truncate_inline(&plan.summary, 700),
        options: vec![
            "preserve/quarantine the dirty set as user-owned context".to_string(),
            "revert or abandon the dirty set outside Worker".to_string(),
            "isolate the dirty set onto a separate branch or patch".to_string(),
            "approve a new explicit safety gate for the dirty set".to_string(),
        ],
        source: "project_manager".to_string(),
    }
}

fn human_disposition_pause_can_clear(paths: &Paths, state: &State) -> bool {
    state
        .human_disposition_required
        .as_ref()
        .is_some_and(|pause| pause.source != "rust_dirty_adoption")
        && (!state.queue.is_empty()
            || state.retry_worker_same_task.is_some()
            || state.senior_dev_repair_order.is_some()
            || read_next_steps_queue_state(paths).approved
            || read_next_steps_non_ai_queue_state(paths).approved)
}

fn print_human_disposition_pause(pause: &HumanDispositionPause) {
    println!(
        "[Team] Paused: dirty/quarantined work needs human disposition before Rust calls PM/CEO again."
    );
    println!("[Team] Reason: {}", truncate_inline(&pause.reason, 260));
    if !pause.options.is_empty() {
        println!("[Team] Options: {}", pause.options.join(" | "));
    }
}

fn mentioned_control_doc_paths(paths: &Paths, text: &str) -> Vec<String> {
    let mut candidates = vec![
        ("ROADMAP.md".to_string(), paths.root.join("ROADMAP.md")),
        (
            "Docs/ROADMAP.md".to_string(),
            paths.root.join("Docs/ROADMAP.md"),
        ),
        (
            "docs/ROADMAP.md".to_string(),
            paths.root.join("docs/ROADMAP.md"),
        ),
        (
            "Docs/.INDEX.md".to_string(),
            paths.root.join("Docs/.INDEX.md"),
        ),
        (
            "docs/.INDEX.md".to_string(),
            paths.root.join("docs/.INDEX.md"),
        ),
        (
            "NEXT_STEPS.md".to_string(),
            paths.root.join("NEXT_STEPS.md"),
        ),
        (
            "NEXT_STEPS_non-ai.md".to_string(),
            paths.root.join("NEXT_STEPS_non-ai.md"),
        ),
        (
            "CONTINUITY.md".to_string(),
            paths.root.join("CONTINUITY.md"),
        ),
        ("PARITY.md".to_string(), paths.root.join("PARITY.md")),
        ("TESTING.md".to_string(), paths.root.join("TESTING.md")),
        (
            "ARCHITECTURE.md".to_string(),
            paths.root.join("ARCHITECTURE.md"),
        ),
        ("QUALITY.md".to_string(), paths.root.join("QUALITY.md")),
        ("AGENTS.md".to_string(), paths.root.join("AGENTS.md")),
        ("TASK.md".to_string(), paths.root.join("TASK.md")),
        (
            "docs/PRE_LIVE_SECURITY_REVIEW.md".to_string(),
            paths.root.join("docs/PRE_LIVE_SECURITY_REVIEW.md"),
        ),
        (
            "Docs/PRE_LIVE_SECURITY_REVIEW.md".to_string(),
            paths.root.join("Docs/PRE_LIVE_SECURITY_REVIEW.md"),
        ),
    ];
    for dir in [
        paths.root.join("Docs/exec-plans/active"),
        paths.root.join("docs/exec-plans/active"),
    ] {
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|ext| ext.to_str()) != Some("md") {
                    continue;
                }
                if let Some(rel) = relative_slash_path(&paths.root, &path) {
                    candidates.push((rel, path));
                }
            }
        }
    }
    let lower = text.to_ascii_lowercase();
    let mut seen_canonical = BTreeSet::new();
    let mut files = Vec::new();
    for (path, absolute) in candidates {
        let path_lower = path.to_ascii_lowercase();
        let basename_lower = path
            .rsplit('/')
            .next()
            .unwrap_or(&path)
            .to_ascii_lowercase();
        let mentioned = lower.contains(&path_lower)
            || (path == "ROADMAP.md" && lower.contains("root") && lower.contains("roadmap.md"))
            || (path == "NEXT_STEPS_non-ai.md"
                && (lower.contains("next_steps_non-ai") || lower.contains("next_steps non-ai")))
            || (path == "NEXT_STEPS.md" && lower.contains("next_steps.md"))
            || (path == "CONTINUITY.md" && lower.contains(&basename_lower))
            || (path == "PARITY.md" && lower.contains(&basename_lower))
            || (path == "TESTING.md" && lower.contains(&basename_lower))
            || (path == "ARCHITECTURE.md" && lower.contains(&basename_lower))
            || (path == "QUALITY.md" && lower.contains(&basename_lower))
            || (path == "AGENTS.md" && lower.contains(&basename_lower))
            || (path == "TASK.md" && lower.contains(&basename_lower));
        if mentioned && absolute.exists() {
            let canonical = fs::canonicalize(&absolute).unwrap_or_else(|_| absolute.clone());
            if seen_canonical.insert(canonical) {
                files.push(path.to_string());
            }
        }
    }
    files.sort();
    files.dedup();
    files
}

fn queue_next_task_from_active_senior_batch(
    paths: &Paths,
    cfg: &Config,
    state: &mut State,
) -> Result<usize> {
    let Some(batch) = state.senior_batch_approval.clone() else {
        return Ok(0);
    };
    if batch.remaining_successes == 0 || batch.task_ids.is_empty() {
        expire_senior_batch(paths, state, "success_budget_spent", None)?;
        return Ok(0);
    }
    let text = read_text_or_empty(&paths.next_steps);
    let candidates = senior_batch_active_candidate_tasks_from_text(&text)
        .into_iter()
        .filter(|task| !task_suppressed_by_preserved_dirty_baseline(state, task))
        .collect::<Vec<_>>();
    let by_id = candidates
        .into_iter()
        .map(|task| (task.task_id.clone(), task))
        .collect::<HashMap<_, _>>();
    for task_id in &batch.task_ids {
        let Some(task) = by_id.get(task_id) else {
            continue;
        };
        if !senior_batch_allows_task(&batch, task) {
            expire_senior_batch(paths, state, "task_outside_batch_envelope", Some(task))?;
            return Ok(0);
        }
        ensure_task_approved_in_next_steps(paths, task)?;
        if enqueue_back(state, task.clone()) {
            state.planner_due = false;
            state.master_due = false;
            let _ = record_rust_model_skip(
                paths,
                "planner",
                Some(&task.task_id),
                cfg,
                RoleSkipReason::SeniorBatchActive.as_str(),
            );
            append_event(
                paths,
                OrchestratorEventKind::StateTransitionApplied,
                Some(&task.task_id),
                json!({
                    "route": "worker_continuation",
                    "batch_id": batch.batch_id,
                    "reason": "senior_batch_active"
                }),
            )?;
            println!(
                "[Rust] Queued {} from Senior-approved batch {}.",
                task.task_id, batch.batch_id
            );
            return Ok(1);
        }
    }
    expire_senior_batch(paths, state, "no_remaining_eligible_tasks", None)?;
    Ok(0)
}

fn apply_senior_batch_approval(
    paths: &Paths,
    cfg: &Config,
    state: &mut State,
    candidates: &[TaskSpec],
    approval: SeniorBatchApprovalOutput,
) -> Result<usize> {
    state.last_foreman_summary = Some(approval.summary.clone());
    state.last_review_summary = Some(approval.summary.clone());
    push_event(
        state,
        "foreman",
        &format!("batch-{}", approval.reason_code),
        &approval.summary,
    );

    if approval.needs_master {
        state.master_due = true;
    }
    if approval.needs_planner {
        state.planner_due = true;
    }
    let progress = apply_senior_progress_actions(paths, cfg, state, &approval.progress_actions)?;
    if state.pending_commit.is_some() {
        state.progress_action_completed_this_step = progress.rust_actions_run > 0;
        return Ok(0);
    }
    if progress.needs_planner {
        state.planner_due = true;
    }
    if progress.rust_actions_run > 0 || progress.worker_queued > 0 {
        state.progress_action_completed_this_step = progress.rust_actions_run > 0;
        if progress.worker_queued > 0 {
            return Ok(progress.worker_queued);
        }
        if !approval.approved {
            return Ok(progress.worker_queued);
        }
    }
    if !approval.approved {
        return Ok(0);
    }

    let candidate_ids = candidates
        .iter()
        .map(|task| task.task_id.clone())
        .collect::<HashSet<_>>();
    let mut task_ids = Vec::new();
    for task_id in approval.task_ids {
        if candidate_ids.contains(&task_id) && !task_ids.contains(&task_id) {
            task_ids.push(task_id);
        }
    }
    if task_ids.is_empty() {
        state.planner_due = true;
        append_event(
            paths,
            OrchestratorEventKind::SeniorBatchExpired,
            None,
            json!({
                "reason": "senior_returned_no_valid_candidate_ids",
                "summary": approval.summary
            }),
        )?;
        return Ok(progress.worker_queued);
    }

    let max_tasks = approval.max_tasks.min(task_ids.len()).max(1);
    task_ids.truncate(max_tasks);
    let now = Utc::now().to_rfc3339();
    let mut allowed_kinds = approval.allowed_kinds;
    if allowed_kinds.is_empty() {
        allowed_kinds = candidates
            .iter()
            .filter(|task| task_ids.contains(&task.task_id))
            .map(classify_senior_batch_kind)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
    }
    let batch_id = if approval.batch_id.trim().is_empty() {
        format!("senior-batch-{}", Utc::now().timestamp_millis())
    } else {
        approval.batch_id.trim().to_string()
    };
    state.senior_batch_approval = Some(SeniorBatchApproval {
        batch_id: batch_id.clone(),
        task_ids,
        max_tasks,
        remaining_successes: max_tasks,
        allowed_kinds,
        forbidden_scope: approval.forbidden_scope,
        requires_stop_on: approval.requires_stop_on,
        verification_required: approval.verification_required,
        auto_accept_policy: approval.auto_accept_policy,
        summary: approval.summary,
        created_at: now.clone(),
        updated_at: now,
    });
    state.planner_due = false;
    state.master_due = false;
    if let Some(batch) = &state.senior_batch_approval {
        for task_id in &batch.task_ids {
            if let Some(task) = candidates.iter().find(|task| &task.task_id == task_id) {
                ensure_task_approved_in_next_steps(paths, task)?;
            }
        }
    }
    append_event(
        paths,
        OrchestratorEventKind::SeniorBatchApproved,
        None,
        json!({"batch": state.senior_batch_approval.clone()}),
    )?;
    let _ = record_rust_model_skip(
        paths,
        "planner",
        None,
        cfg,
        RoleSkipReason::SeniorBatchApprovedWorkerContinuation.as_str(),
    );
    let queued = queue_next_task_from_active_senior_batch(paths, cfg, state)?;
    Ok(queued + progress.worker_queued)
}

#[derive(Debug, Clone, Copy, Default)]
struct SeniorProgressApplyResult {
    rust_actions_run: usize,
    worker_queued: usize,
    needs_planner: bool,
}

fn apply_senior_progress_actions(
    paths: &Paths,
    cfg: &Config,
    state: &mut State,
    actions: &[SeniorProgressAction],
) -> Result<SeniorProgressApplyResult> {
    let mut result = SeniorProgressApplyResult::default();
    if actions.is_empty() {
        return Ok(result);
    }
    if actions.len() > 5 {
        append_event(
            paths,
            OrchestratorEventKind::PolicyViolation,
            None,
            json!({
                "mode": "senior_progress_actions_rejected",
                "reason": "too_many_actions",
                "count": actions.len(),
            }),
        )?;
        result.needs_planner = true;
        return Ok(result);
    }

    for action in actions {
        match action.kind {
            SeniorProgressActionKind::RustEvidenceJob
            | SeniorProgressActionKind::RustBookkeepingJob => {
                match run_senior_rust_evidence_job(paths, cfg, action) {
                    Ok(receipt) => {
                        result.rust_actions_run += 1;
                        state.last_review_summary = Some(format!(
                            "Rust completed Senior progress action {} with status {}.",
                            action.action_id, receipt.status
                        ));
                        push_event(
                            state,
                            "deterministic",
                            "senior-progress-evidence",
                            &format!(
                                "{} -> {} ({})",
                                action.action_id, receipt.status, receipt.blocker_classification
                            ),
                        );
                    }
                    Err(err) => {
                        append_event(
                            paths,
                            OrchestratorEventKind::PolicyViolation,
                            Some(&action.action_id),
                            json!({
                                "mode": "senior_progress_rust_job_rejected",
                                "reason": format!("{err:#}"),
                            }),
                        )?;
                        result.needs_planner = true;
                    }
                }
            }
            SeniorProgressActionKind::RustCommitJob => {
                let request = RustCommitRequest::from_senior_progress_action(action);
                match execute_rust_commit_request(paths, cfg, state, &request, "senior_dev") {
                    Ok(Some(sha)) => {
                        result.rust_actions_run += 1;
                        state.last_review_summary = Some(format!(
                            "Rust committed Senior progress action {} at {}.",
                            action.action_id,
                            truncate_inline(&sha, 12)
                        ));
                        push_event(
                            state,
                            "deterministic",
                            "senior-progress-commit",
                            &format!(
                                "{} -> committed ({})",
                                action.action_id,
                                truncate_inline(&sha, 12)
                            ),
                        );
                    }
                    Ok(None) if state.pending_commit.is_some() => {
                        result.rust_actions_run += 1;
                        state.last_review_summary = Some(format!(
                            "Senior progress action {} is paused for explicit commit approval.",
                            action.action_id
                        ));
                        break;
                    }
                    Ok(None) => {}
                    Err(err) => {
                        append_event(
                            paths,
                            OrchestratorEventKind::PolicyViolation,
                            Some(&action.action_id),
                            json!({
                                "mode": "senior_progress_rust_commit_rejected",
                                "reason": format!("{err:#}"),
                            }),
                        )?;
                        result.needs_planner = true;
                    }
                }
            }
            SeniorProgressActionKind::WorkerEditTask => {
                if result.worker_queued > 0 {
                    append_event(
                        paths,
                        OrchestratorEventKind::PolicyViolation,
                        Some(&action.action_id),
                        json!({
                            "mode": "senior_progress_worker_task_skipped",
                            "reason": "only_one_worker_edit_task_per_single_worktree_cycle",
                        }),
                    )?;
                    continue;
                }
                let task = senior_progress_worker_task(action);
                let policy = task_policy_from_task(&task);
                let policy_findings = validate_task_policy(paths, &task, &policy);
                if !policy_findings.is_empty() {
                    append_event(
                        paths,
                        OrchestratorEventKind::PolicyViolation,
                        Some(&task.task_id),
                        json!({
                            "mode": "senior_progress_worker_task_policy_rejected",
                            "findings": policy_findings,
                        }),
                    )?;
                    result.needs_planner = true;
                    continue;
                }
                if enqueue_back(state, task.clone()) {
                    result.worker_queued += 1;
                    state.planner_due = false;
                    state.master_due = false;
                    append_event(
                        paths,
                        OrchestratorEventKind::StateTransitionApplied,
                        Some(&task.task_id),
                        json!({
                            "mode": "senior_progress_worker_task_queued",
                            "title": task.title,
                        }),
                    )?;
                    record_rust_model_skip(
                        paths,
                        "planner",
                        Some(&task.task_id),
                        cfg,
                        "senior_progress_worker_task_queued_without_pm",
                    )?;
                }
            }
            SeniorProgressActionKind::SeniorDecision => {
                state.last_review_summary = Some(action.summary.clone());
                push_event(
                    state,
                    "foreman",
                    "senior-progress-decision",
                    &action.summary,
                );
            }
            SeniorProgressActionKind::PmDirection => {
                result.needs_planner = true;
                append_event(
                    paths,
                    OrchestratorEventKind::StateTransitionApplied,
                    Some(&action.action_id),
                    json!({
                        "mode": "senior_progress_requested_pm_direction",
                        "summary": action.summary,
                    }),
                )?;
            }
        }
    }
    Ok(result)
}

fn senior_progress_worker_task(action: &SeniorProgressAction) -> TaskSpec {
    TaskSpec {
        task_id: clean_progress_action_id(&action.action_id),
        parent_task: None,
        status: "approved".to_string(),
        priority: "normal".to_string(),
        review_tier: ReviewTier::AiReviewOnFailure,
        automation_level: AutomationLevel::AiWorkerRustReview,
        dependencies: Vec::new(),
        title: action.title.clone(),
        kind: "implementation".to_string(),
        objective: action.summary.clone(),
        why_unblocks_progress:
            "Senior Dev classified this as the one bounded Worker edit task for the current progress batch."
                .to_string(),
        allowed_files: action.allowed_files.clone(),
        read_only_files: action.read_only_files.clone(),
        forbidden_actions: vec![
            "Do not broaden beyond the Senior progress action envelope.".to_string(),
            "Do not run long deterministic benchmark/test jobs; request a Rust evidence job instead."
                .to_string(),
        ],
        exact_steps: vec![action.summary.clone()],
        success_criteria: non_empty_or_default(
            &action.success_criteria,
            vec!["Rust final verification passes for the bounded edit.".to_string()],
        ),
        tests_to_run: action.commands.clone(),
        stop_conditions: non_empty_or_default(
            &action.stop_conditions,
            vec!["Stop if the edit needs files outside the allowed scope.".to_string()],
        ),
        discovery_budget: "Read only the named allowed/read-only files unless a stop condition requires escalation.".to_string(),
        estimated_minutes: 30,
        requires_network: false,
        prefer_worktree: None,
    }
}

pub(crate) fn clean_progress_action_id(action_id: &str) -> String {
    let clean = action_id
        .trim()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>();
    if clean.is_empty() {
        format!("FOREMAN-{}", Utc::now().timestamp_millis())
    } else {
        clean
    }
}

fn run_senior_rust_evidence_job(
    paths: &Paths,
    cfg: &Config,
    action: &SeniorProgressAction,
) -> Result<RustEvidenceJobReceipt> {
    if !cfg.deterministic_tasks {
        bail!("deterministic_tasks=false");
    }
    let mut commands = Vec::new();
    let mut rejected_commands = Vec::new();
    for (index, command_text) in action.commands.iter().enumerate() {
        let trimmed = command_text.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Some(command) = parse_verification_command(trimmed, "senior.progress") else {
            rejected_commands.push(format!("{trimmed} (not a safe typed recipe)"));
            continue;
        };
        if !is_deterministic_command_structured_allowed(&command) {
            rejected_commands.push(format!(
                "{} (not allowed for deterministic execution)",
                command.display()
            ));
            continue;
        }
        commands.push(with_stable_command_metadata(
            &action.action_id,
            "senior-progress",
            index,
            command,
        ));
    }
    if commands.is_empty() && rejected_commands.is_empty() {
        bail!("Rust evidence job requires at least one deterministic command");
    }
    let plan_fingerprint = commands
        .iter()
        .map(VerificationCommand::fingerprint)
        .chain(rejected_commands.iter().cloned())
        .collect::<Vec<_>>()
        .join("\n");
    let mut plan = VerificationPlan {
        plan_id: format!(
            "{}-{}",
            sanitize_filename(&action.action_id),
            &stable_hash_hex(&plan_fingerprint)[..12]
        ),
        task_id: clean_progress_action_id(&action.action_id),
        parent_task: None,
        source: "senior.progress.rust_evidence_job".to_string(),
        commands,
        rejected_commands,
    };
    let envelope = senior_progress_worker_task(action);
    let scorecard = route_scorecard_for_task(
        paths,
        &envelope,
        ExecutionLane::RustEvidence,
        "rust_evidence_job",
        None,
        0,
    );
    let _ = record_route_scorecard(paths, &scorecard);
    reject_disallowed_swiftstacker_report_roots(paths, &envelope, &mut plan)?;
    println!(
        "[Rust] Running Senior progress evidence job {} with {} command(s)",
        action.action_id,
        plan.commands.len()
    );
    let report =
        execute_verification_plan_with_policy(paths, &plan, cfg.requires_verification_isolation())?;
    let evidence_path = persist_evidence_report(paths, &report)?;
    let receipt = rust_evidence_job_receipt(paths, action, &report, &evidence_path);
    write_rust_evidence_job_receipt(paths, &receipt)?;
    let route_finished_at = Utc::now().to_rfc3339();
    let route_status = if receipt.status == "passed" {
        RouteLifecycleStatus::Succeeded
    } else {
        RouteLifecycleStatus::Failed
    };
    record_route_attempt(
        paths,
        &RouteAttempt {
            id: route_attempt_id(
                "rust_evidence_job",
                Some(&action.action_id),
                ExecutionLane::RustEvidence,
            ),
            task_id: Some(action.action_id.clone()),
            attempt: 1,
            status: route_status,
            started_at: report.started_at.clone(),
            finished_at: Some(route_finished_at),
            execution_lane: ExecutionLane::RustEvidence,
            route_kind: "rust_evidence_job".to_string(),
            model_tier: Some("rust".to_string()),
            model: None,
            prompt_receipt_id: None,
            evidence_receipt_id: Some(receipt.receipt_id.clone()),
            failure_fingerprint: None,
            token_usage: TokenUsage::default(),
            scope_group: Some(action.allowed_files.join("|")),
            scope_overlap: false,
            final_feedback: Some(RouteFeedback {
                outcome: receipt.status.clone(),
                summary: format!(
                    "Rust evidence job ran {} command(s); blocker={}",
                    receipt.command_count, receipt.blocker_classification
                ),
                regret_kind: None,
                better_lane: None,
                verification_status: Some(report.status.clone()),
                commit_sha: None,
                duration_ms: Some(
                    report
                        .commands
                        .iter()
                        .map(|command| command.duration_ms)
                        .sum(),
                ),
            }),
        },
    )?;
    if receipt.status == "passed" {
        let _ = record_positive_route_learning(
            paths,
            &envelope,
            ExecutionLane::RustEvidence,
            "rust_evidence_passed",
            &format!(
                "Rust evidence job `{}` passed with {} command(s).",
                action.action_id, receipt.command_count
            ),
        );
    }
    append_event(
        paths,
        OrchestratorEventKind::EvidenceSummarized,
        Some(&action.action_id),
        json!({
            "mode": "senior_progress_rust_evidence_job",
            "receipt_id": receipt.receipt_id,
            "status": receipt.status,
            "evidence_path": receipt.evidence_path,
            "command_count": receipt.command_count,
            "blocker_classification": receipt.blocker_classification,
        }),
    )?;
    record_rust_model_skip(
        paths,
        "worker",
        Some(&action.action_id),
        cfg,
        "senior_progress_rust_evidence_job",
    )?;
    Ok(receipt)
}

fn try_run_mechanistic_handoff_task(
    paths: &Paths,
    cfg: &Config,
    state: &mut State,
    task: &TaskSpec,
) -> Result<Option<bool>> {
    if cfg.deterministic_tasks {
        let selected = match crate::learning::select_applicable_recipe(paths, task) {
            Ok(selected) => selected,
            Err(_) => {
                let reason = format!(
                    "Learned recipe selection for task {} requires operator review because the catalog is ambiguous or unhealthy; AI fallback is disabled.",
                    task.task_id
                );
                state.human_disposition_required = Some(HumanDispositionPause {
                    reason: reason.clone(),
                    options: vec![
                        "inspect recipe list and preview the competing revisions".to_string(),
                        "retire or reject the inappropriate revision".to_string(),
                        "leave the task paused".to_string(),
                    ],
                    source: "learning_recipe_selection".to_string(),
                });
                state.last_review_summary = Some(reason.clone());
                push_event(state, "learning", "recipe-selection-paused", &reason);
                record_rust_model_skip(
                    paths,
                    "worker",
                    Some(&task.task_id),
                    cfg,
                    "learned recipe selection required operator review",
                )?;
                return Ok(Some(false));
            }
        };
        if let Some(selected) = selected {
            println!(
                "[Rust] Running approved learned recipe {} for evidence-only task {}.",
                selected.approved.recipe_id, task.task_id
            );
            let execution = match crate::learning::execute_applicable_recipe(paths, task, &selected)
            {
                Ok(execution) => execution,
                Err(_) => {
                    let reason = format!(
                        "Learned recipe {} could not run in the required isolated environment; the task remains unfinished and AI fallback is disabled.",
                        selected.approved.recipe_id
                    );
                    state.human_disposition_required = Some(HumanDispositionPause {
                        reason: reason.clone(),
                        options: vec![
                            "inspect doctor and recipe compatibility".to_string(),
                            "retry only after isolation is healthy".to_string(),
                            "leave the task paused".to_string(),
                        ],
                        source: "learning_recipe_stopped".to_string(),
                    });
                    state.last_review_summary = Some(reason.clone());
                    push_event(state, "learning", "recipe-stopped", &reason);
                    record_rust_model_skip(
                        paths,
                        "worker",
                        Some(&task.task_id),
                        cfg,
                        "approved learned recipe unavailable without AI fallback",
                    )?;
                    return Ok(Some(false));
                }
            };
            if execution.disposition != crate::learning::ExecutionDisposition::Passed {
                let reason = format!(
                    "Learned recipe {} stopped with disposition {:?}; the task remains unfinished and AI fallback is disabled.",
                    execution.recipe_id, execution.disposition
                );
                state.human_disposition_required = Some(HumanDispositionPause {
                    reason: reason.clone(),
                    options: vec![
                        "review recipe evidence and repository compatibility".to_string(),
                        "reactivate an exact reviewed revision".to_string(),
                        "leave the recipe quarantined".to_string(),
                    ],
                    source: "learning_recipe_quarantine".to_string(),
                });
                state.last_review_summary = Some(reason.clone());
                push_event(state, "learning", "recipe-quarantined", &reason);
                record_rust_model_skip(
                    paths,
                    "worker",
                    Some(&task.task_id),
                    cfg,
                    "approved learned recipe stopped without AI fallback",
                )?;
                return Ok(Some(false));
            }
            let receipt = RustEvidenceJobReceipt {
                receipt_id: execution.receipt_id.clone(),
                action_id: task.task_id.clone(),
                kind: SeniorProgressActionKind::RustEvidenceJob,
                title: task.title.clone(),
                status: "passed".to_string(),
                evidence_path: format!(
                    ".orchestrator/recipes/executions/{}.json",
                    execution.receipt_id
                ),
                command_count: execution.commands.len(),
                rejected_commands: Vec::new(),
                command_summaries: selected
                    .approved
                    .definition
                    .commands
                    .iter()
                    .zip(execution.commands.iter())
                    .map(|(command, result)| {
                        format!("{} -> {}", command.display(), result.category)
                    })
                    .collect(),
                blocker_classification: "none".to_string(),
                completed_at: execution.finished_at,
            };
            return finalize_mechanistic_evidence_task(
                paths,
                cfg,
                state,
                task,
                receipt,
                "learned_recipe_rust_evidence_job",
            );
        }
    }
    if !cfg.deterministic_tasks || !task_is_command_only_evidence(task) {
        return Ok(None);
    }
    let (safe_commands, rejected_commands) = mechanistic_task_command_safety(task);
    if safe_commands.is_empty() || !rejected_commands.is_empty() {
        let summary = format!(
            "{} is command-only but has {} unsafe or unsupported deterministic command(s); returning to Project Manager before Worker.",
            task.task_id,
            rejected_commands.len().max(usize::from(safe_commands.is_empty()))
        );
        println!("[Rust] {summary}");
        state.last_review_summary = Some(summary.clone());
        state.planner_due = true;
        push_event(
            state,
            "deterministic",
            "mechanistic-handoff-rejected",
            &summary,
        );
        append_event(
            paths,
            OrchestratorEventKind::PolicyViolation,
            Some(&task.task_id),
            json!({
                "mode": "mechanistic_handoff_rejected",
                "rejected_commands": rejected_commands,
            }),
        )?;
        record_rust_model_skip(
            paths,
            "worker",
            Some(&task.task_id),
            cfg,
            "command_only_task_failed_mechanistic_handoff_validation",
        )?;
        return Ok(Some(false));
    }

    let action = SeniorProgressAction {
        action_id: task.task_id.clone(),
        kind: SeniorProgressActionKind::RustEvidenceJob,
        title: task.title.clone(),
        summary: task.objective.clone(),
        commit_message: String::new(),
        allowed_files: task.allowed_files.clone(),
        read_only_files: task.read_only_files.clone(),
        commands: safe_commands,
        success_criteria: task.success_criteria.clone(),
        stop_conditions: task.stop_conditions.clone(),
    };
    println!(
        "[Rust] Running command-only task {} as Rust evidence before Worker.",
        task.task_id
    );
    let receipt = run_senior_rust_evidence_job(paths, cfg, &action)?;
    finalize_mechanistic_evidence_task(
        paths,
        cfg,
        state,
        task,
        receipt,
        "mechanistic_handoff_rust_evidence_job",
    )
}

fn finalize_mechanistic_evidence_task(
    paths: &Paths,
    cfg: &Config,
    state: &mut State,
    task: &TaskSpec,
    receipt: RustEvidenceJobReceipt,
    mode: &str,
) -> Result<Option<bool>> {
    let done = receipt.status == "passed";
    if let Some(queue_path) = queue_path_containing_task(paths, &task.task_id) {
        let queue_before = capture_rust_owned_file(paths, &queue_path)?;
        let status = if done { "Done." } else { "Blocked." };
        mark_task_status_from_rust_evidence_receipt(&queue_path, task, status, &receipt)?;
        let commit_message = format!("Record {} Rust evidence", task.task_id);
        let commit_result = if done {
            commit_completed_queue_bookkeeping(
                paths,
                cfg,
                state,
                task,
                &queue_path,
                &commit_message,
                &queue_before,
            )
        } else {
            commit_queue_bookkeeping(
                paths,
                cfg,
                state,
                &queue_path,
                &commit_message,
                &queue_before,
            )
        };
        commit_result?;
        if state.pending_commit.is_some() {
            if done
                && !state
                    .queue
                    .iter()
                    .any(|queued| queued.task_id == task.task_id)
            {
                state.queue.insert(0, task.clone());
            }
            state.last_review_summary = Some(format!(
                "{} evidence is paused for explicit commit approval.",
                task.task_id
            ));
            return Ok(Some(false));
        }
    }
    let summary = format!(
        "{} ran as a Rust evidence job before Worker; status `{}`; receipt `{}`.",
        task.task_id, receipt.status, receipt.receipt_id
    );
    state.last_worker_summary = Some(summary.clone());
    state.last_review_summary = Some(summary.clone());
    push_event(state, "deterministic", "mechanistic-handoff", &summary);
    append_event(
        paths,
        OrchestratorEventKind::StateTransitionApplied,
        Some(&task.task_id),
        json!({
            "mode": mode,
            "receipt_id": receipt.receipt_id,
            "status": receipt.status,
            "blocker_classification": receipt.blocker_classification,
        }),
    )?;
    record_rust_model_skip(paths, "foreman-review", Some(&task.task_id), cfg, mode)?;
    if done {
        state.completed_tasks += 1;
        record_senior_batch_task_success(paths, state, task)?;
        Ok(Some(!state.project_complete))
    } else {
        let n = state.failed_tasks.entry(task.task_id.clone()).or_insert(0);
        *n += 1;
        state.planner_due = true;
        Ok(Some(false))
    }
}

fn try_block_worker_for_acceptance_contract(
    paths: &Paths,
    cfg: &Config,
    state: &mut State,
    task: &TaskSpec,
    contract: &AcceptanceContract,
) -> Result<Option<bool>> {
    if !contract.blocks_worker_launch() {
        return Ok(None);
    }
    let reason = contract
        .blocker_reason
        .as_deref()
        .unwrap_or("missing_acceptance_contract");
    let summary = format!(
        "{} is a risky migration/refactor task without characterization evidence; routing to Project Manager before Worker.",
        task.task_id
    );
    println!("[Rust] {summary}");
    state.last_review_summary = Some(summary.clone());
    state.planner_due = true;
    push_event(state, "policy", reason, &summary);
    if let Some(queue_path) = queue_path_containing_task(paths, &task.task_id) {
        let queue_before = capture_rust_owned_file(paths, &queue_path)?;
        mark_task_blocked_by_acceptance_contract(&queue_path, task, contract, &summary)?;
        let _ = commit_queue_bookkeeping(
            paths,
            cfg,
            state,
            &queue_path,
            &format!("Block {} pending characterization", task.task_id),
            &queue_before,
        );
    }
    append_event(
        paths,
        OrchestratorEventKind::PolicyViolation,
        Some(&task.task_id),
        json!({
            "mode": "acceptance_contract_blocked_worker",
            "reason": reason,
            "acceptance_contract": contract,
        }),
    )?;
    record_rust_model_skip(paths, "worker", Some(&task.task_id), cfg, reason)?;
    Ok(Some(false))
}

fn mark_task_blocked_by_acceptance_contract(
    queue_path: &Path,
    task: &TaskSpec,
    contract: &AcceptanceContract,
    summary: &str,
) -> Result<()> {
    let text = read_text_or_empty(queue_path);
    let mut lines = text.lines().map(str::to_string).collect::<Vec<_>>();
    let notes = vec![
        "- Acceptance contract notes:".to_string(),
        format!("  - Status: Blocked by Rust before Worker (`{}`).", contract
            .blocker_reason
            .as_deref()
            .unwrap_or("missing_acceptance_contract")),
        format!("  - Summary: {summary}"),
        "  - Required characterization evidence: golden, snapshot, parity, baseline, fixture, or characterization verification in the task contract.".to_string(),
    ];
    if let Some((start, end)) = next_steps_task_section_bounds(&lines, &task.task_id) {
        replace_task_status_in_section(&mut lines, start, end, "- Status: Blocked.");
        let insert_at = next_steps_task_section_bounds(&lines, &task.task_id)
            .map(|(_, section_end)| section_end.min(lines.len()))
            .unwrap_or_else(|| end.min(lines.len()));
        lines.splice(insert_at..insert_at, notes);
    } else {
        lines.push(format!("### {} - {}", task.task_id, task.title));
        lines.push("- Status: Blocked.".to_string());
        lines.extend(notes);
    }
    let mut updated = lines.join("\n");
    updated.push('\n');
    write_text(queue_path, &updated)
}

fn task_is_command_only_evidence(task: &TaskSpec) -> bool {
    if task.tests_to_run.is_empty() || task.requires_network {
        return false;
    }
    // Empty allowed_files is vacuously safe. Docs-verify paths are allowed only
    // when wording stays evidence/verify (edit smells are rejected below).
    if !task.allowed_files.iter().all(|path| {
        is_tracking_doc_path(path)
            || is_mechanistic_artifact_output_path(path)
            || is_docs_verify_path(path)
    }) {
        return false;
    }
    let text = format!(
        "{}\n{}\n{}\n{}\n{}",
        task.kind,
        task.title,
        task.objective,
        task.exact_steps.join("\n"),
        task.success_criteria.join("\n")
    )
    .to_ascii_lowercase();
    let looks_like_evidence = [
        "command-only",
        "command only",
        "evidence",
        "benchmark",
        "build",
        "test",
        "verify",
        "verification",
        "validation",
        "capture",
        "run ",
    ]
    .iter()
    .any(|needle| text.contains(needle));
    let looks_like_edit = [
        "implement",
        "edit ",
        "modify",
        "update ",
        "write ",
        "refactor",
        "fix ",
        "patch",
        "wire ",
        "source edit",
        "code change",
        "change source",
    ]
    .iter()
    .any(|needle| text.contains(needle));
    looks_like_evidence && !looks_like_edit
}

fn is_mechanistic_artifact_output_path(path: &str) -> bool {
    let path = path.trim_matches('`');
    path == "Benchmarks/Reports*"
        || path.starts_with("Benchmarks/Reports")
        || path.starts_with("Benchmarks/Reports*/")
}

fn mechanistic_task_command_safety(task: &TaskSpec) -> (Vec<String>, Vec<String>) {
    let mut safe = Vec::new();
    let mut rejected = Vec::new();
    for command in &task.tests_to_run {
        let trimmed = command.trim();
        let Some(parsed) = parse_verification_command(trimmed, "mechanistic.task") else {
            rejected.push(format!("{trimmed} (not a safe typed recipe)"));
            continue;
        };
        if !is_deterministic_command_structured_allowed(&parsed) {
            rejected.push(format!(
                "{} (not allowed for deterministic execution)",
                parsed.display()
            ));
            continue;
        }
        safe.push(parsed.display());
    }
    safe.sort();
    safe.dedup();
    rejected.sort();
    rejected.dedup();
    (safe, rejected)
}

fn queue_path_containing_task(paths: &Paths, task_id: &str) -> Option<PathBuf> {
    if queue_markdown_contains_task(&paths.next_steps, task_id) {
        Some(paths.next_steps.clone())
    } else if queue_markdown_contains_task(&paths.next_steps_non_ai, task_id) {
        Some(paths.next_steps_non_ai.clone())
    } else {
        None
    }
}

fn mark_task_status_from_rust_evidence_receipt(
    queue_path: &Path,
    task: &TaskSpec,
    status: &str,
    receipt: &RustEvidenceJobReceipt,
) -> Result<()> {
    let text = read_text(queue_path)?;
    let mut lines = text.lines().map(str::to_string).collect::<Vec<_>>();
    let Some((start, end)) = next_steps_task_section_bounds(&lines, &task.task_id) else {
        return Ok(());
    };
    replace_task_status_in_section(&mut lines, start, end, &format!("- Status: {status}"));
    let mut notes = vec![
        "- Rust evidence-job notes:".to_string(),
        format!(
            "  - Worker skipped; Rust ran command-only evidence receipt `{}`.",
            receipt.receipt_id
        ),
        format!(
            "  - Status: `{}`; blocker classification: `{}`.",
            receipt.status, receipt.blocker_classification
        ),
    ];
    if !receipt.command_summaries.is_empty() {
        notes.push("  - Commands:".to_string());
        for summary in &receipt.command_summaries {
            notes.push(format!("    - {summary}"));
        }
    }
    if !receipt.rejected_commands.is_empty() {
        notes.push("  - Rejected commands:".to_string());
        for command in &receipt.rejected_commands {
            notes.push(format!("    - `{command}`"));
        }
    }
    let insert_at = next_steps_task_section_bounds(&lines, &task.task_id)
        .map(|(_, section_end)| section_end.min(lines.len()))
        .unwrap_or_else(|| end.min(lines.len()));
    lines.splice(insert_at..insert_at, notes);
    let mut updated = lines.join("\n");
    updated.push('\n');
    write_text(queue_path, &updated)
}

fn rust_evidence_job_receipt(
    paths: &Paths,
    action: &SeniorProgressAction,
    report: &EvidenceReport,
    evidence_path: &Path,
) -> RustEvidenceJobReceipt {
    let blocker_classification = if !report.rejected_commands.is_empty() {
        "rejected_commands"
    } else if report.status == "passed" {
        "passed"
    } else if report
        .commands
        .iter()
        .any(|command| command.category == "knownEnvironmentBlocker")
    {
        "environment_blocker"
    } else {
        "command_failed"
    }
    .to_string();
    RustEvidenceJobReceipt {
        receipt_id: format!(
            "{}-{}",
            sanitize_filename(&action.action_id),
            Utc::now().timestamp_millis().unsigned_abs()
        ),
        action_id: action.action_id.clone(),
        kind: action.kind,
        title: action.title.clone(),
        status: report.status.clone(),
        evidence_path: display_path(paths, evidence_path),
        command_count: report.commands.len(),
        rejected_commands: report.rejected_commands.clone(),
        command_summaries: report
            .commands
            .iter()
            .map(command_evidence_retry_summary)
            .collect(),
        blocker_classification,
        completed_at: report.completed_at.clone(),
    }
}

fn write_rust_evidence_job_receipt(
    paths: &Paths,
    receipt: &RustEvidenceJobReceipt,
) -> Result<PathBuf> {
    let dir = paths.observations.join("evidence_jobs");
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.json", sanitize_filename(&receipt.receipt_id)));
    write_text(
        &path,
        &format!("{}\n", serde_json::to_string_pretty(receipt)?),
    )?;
    Ok(path)
}

fn senior_batch_active_candidate_tasks_from_text(text: &str) -> Vec<TaskSpec> {
    let mut tasks = senior_batch_candidate_tasks_from_text(text);
    let mut seen = tasks
        .iter()
        .map(|task| task.task_id.clone())
        .collect::<HashSet<_>>();
    for task in parse_approved_next_steps_tasks(text) {
        if pending_task_ready_for_senior_batch(&task) && seen.insert(task.task_id.clone()) {
            tasks.push(task);
        }
    }
    tasks
}

fn preserves_dirty_baseline(state: &State) -> bool {
    state
        .human_disposition_decision
        .as_ref()
        .is_some_and(|decision| decision.decision == "preserve_quarantine_stop")
}

fn task_suppressed_by_preserved_dirty_baseline(state: &State, task: &TaskSpec) -> bool {
    preserves_dirty_baseline(state) && task.task_id.starts_with("DIRTY-")
}

fn ensure_task_approved_in_next_steps(paths: &Paths, task: &TaskSpec) -> Result<()> {
    let text = read_text_or_empty(&paths.next_steps);
    let lines = text.lines().map(str::to_string).collect::<Vec<_>>();
    if next_steps_task_section_bounds(&lines, &task.task_id).is_some() {
        approve_pending_task_in_next_steps(paths, task)?;
    } else {
        append_senior_continuation_task_to_next_steps(paths, task)?;
    }
    ensure_active_worker_queue_entry(paths, task)
}

fn append_senior_continuation_task_to_next_steps(paths: &Paths, task: &TaskSpec) -> Result<()> {
    let mut text = read_text_or_empty(&paths.next_steps);
    if !text.ends_with('\n') {
        text.push('\n');
    }
    if !text.ends_with("\n\n") {
        text.push('\n');
    }
    text.push_str(&format!(
        "### {} - {}\n\
- Task ID: `{}`\n\
- Title: {}\n\
- Objective: {}\n\
- Status: Approved.\n\
- Task type: {}.\n\
- Review tier: {}\n\
- Automation level: {}\n\
- Priority: {}\n\
- Allowed files:\n{}\n\
- Forbidden files/actions:\n{}\n\
- Exact steps:\n{}\n\
- Verification commands:\n{}\n\
- Success criteria:\n{}\n\
- Stop conditions:\n{}\n\
- Discovery budget: {}\n",
        task.task_id,
        task.title,
        task.task_id,
        task.title,
        task.objective,
        task.kind,
        serde_json::to_string(&task.review_tier)?.trim_matches('"'),
        serde_json::to_string(&task.automation_level)?.trim_matches('"'),
        task.priority,
        markdown_bullet_list(&task.allowed_files),
        markdown_bullet_list(&task.forbidden_actions),
        numbered_markdown_list(&task.exact_steps),
        markdown_bullet_list(&task.tests_to_run),
        markdown_bullet_list(&task.success_criteria),
        markdown_bullet_list(&task.stop_conditions),
        task.discovery_budget
    ));
    write_text(&paths.next_steps, &text)
}

fn ensure_active_worker_queue_entry(paths: &Paths, task: &TaskSpec) -> Result<()> {
    let text = read_text_or_empty(&paths.next_steps);
    let (active_queue_seen, active_task_ids) = active_queue_task_ids(&text);
    if !active_queue_seen || active_task_ids.contains(&task.task_id) {
        return Ok(());
    }

    let mut lines = text.lines().map(str::to_string).collect::<Vec<_>>();
    let insert_after = lines
        .iter()
        .position(|line| is_approved_queue_label(line.trim()))
        .map(|idx| idx + 1);
    let entry = format!(
        "  - `{}` is approved by Senior Dev dirty/backlog continuation.",
        task.task_id
    );
    if let Some(idx) = insert_after {
        lines.insert(idx, entry);
    } else {
        lines.insert(0, entry);
        lines.insert(0, "- Approved Worker task queue:".to_string());
    }
    let mut updated = lines.join("\n");
    updated.push('\n');
    write_text(&paths.next_steps, &updated)
}

fn markdown_bullet_list(items: &[String]) -> String {
    if items.is_empty() {
        "  - None.".to_string()
    } else {
        items
            .iter()
            .map(|item| format!("  - `{}`", item))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn numbered_markdown_list(items: &[String]) -> String {
    if items.is_empty() {
        "  1. Complete the task exactly as approved.".to_string()
    } else {
        items
            .iter()
            .enumerate()
            .map(|(idx, item)| format!("  {}. {}", idx + 1, item))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn senior_batch_allows_task(batch: &SeniorBatchApproval, task: &TaskSpec) -> bool {
    if !batch.task_ids.contains(&task.task_id) || !pending_task_ready_for_senior_batch(task) {
        return false;
    }
    let allowed = batch
        .allowed_kinds
        .iter()
        .map(|kind| normalize_batch_kind(kind))
        .collect::<HashSet<_>>();
    if allowed.is_empty() {
        return true;
    }
    let classified = normalize_batch_kind(&classify_senior_batch_kind(task));
    let task_kind = normalize_batch_kind(&task.kind);
    allowed.contains(&classified) || allowed.contains(&task_kind)
}

fn classify_senior_batch_kind(task: &TaskSpec) -> String {
    if inferred_docs_only_auto_accept_candidate(task) {
        return "docs_only".to_string();
    }
    let task_text = format!(
        "{}\n{}\n{}\n{}",
        task.task_id, task.kind, task.title, task.objective
    )
    .to_ascii_lowercase();
    if task_text.contains("repair") {
        return "repair".to_string();
    }
    if task
        .allowed_files
        .iter()
        .all(|path| path.to_ascii_lowercase().starts_with("tests/"))
    {
        return "test_only".to_string();
    }
    if task_text.contains("local")
        || task_text.contains("loopback")
        || task_text.contains("snapshot")
        || task_text.contains("sanitized")
        || task_text.contains("bounded")
    {
        return "local_snapshot".to_string();
    }
    "implementation".to_string()
}

fn normalize_batch_kind(kind: &str) -> String {
    kind.trim()
        .trim_matches('`')
        .to_ascii_lowercase()
        .replace(['-', ' '], "_")
}

fn expire_senior_batch(
    paths: &Paths,
    state: &mut State,
    reason: &str,
    task: Option<&TaskSpec>,
) -> Result<()> {
    let Some(batch) = state.senior_batch_approval.take() else {
        return Ok(());
    };
    append_event(
        paths,
        OrchestratorEventKind::SeniorBatchExpired,
        task.map(|task| task.task_id.as_str()),
        json!({
            "batch_id": batch.batch_id,
            "reason": reason,
            "remaining_task_ids": batch.task_ids,
            "remaining_successes": batch.remaining_successes,
        }),
    )?;
    Ok(())
}

fn record_senior_batch_task_success(
    paths: &Paths,
    state: &mut State,
    task: &TaskSpec,
) -> Result<()> {
    let Some(batch) = state.senior_batch_approval.as_mut() else {
        return Ok(());
    };
    if !batch.task_ids.contains(&task.task_id) {
        return Ok(());
    }
    batch.task_ids.retain(|task_id| task_id != &task.task_id);
    batch.remaining_successes = batch.remaining_successes.saturating_sub(1);
    batch.updated_at = Utc::now().to_rfc3339();
    if batch.remaining_successes == 0 || batch.task_ids.is_empty() {
        expire_senior_batch(paths, state, "completed", Some(task))?;
    }
    Ok(())
}

fn deterministic_probe_summaries(paths: &Paths, parent_task: &str) -> Vec<String> {
    let text = read_text_or_empty(&paths.next_steps_non_ai);
    let lines = text.lines().map(str::to_string).collect::<Vec<_>>();
    let mut summaries = Vec::new();
    let mut idx = 0;
    while idx < lines.len() {
        if !lines[idx].trim_start().starts_with("### ") {
            idx += 1;
            continue;
        }
        let start = idx;
        let end = lines
            .iter()
            .enumerate()
            .skip(start + 1)
            .find_map(|(line_idx, line)| line.trim_start().starts_with("### ").then_some(line_idx))
            .unwrap_or(lines.len());
        let section = &lines[start..end];
        let parent = markdown_field_value(section, "Parent task")
            .map(|value| clean_markdown_value(&value).trim_matches('`').to_string());
        let status = markdown_field_value(section, "Status").map(|value| {
            clean_markdown_value(&value)
                .trim_end_matches('.')
                .to_string()
        });
        if parent.as_deref() == Some(parent_task)
            && matches!(status.as_deref(), Some("Done") | Some("Blocked"))
        {
            let mut notes = Vec::new();
            for line in section {
                let trimmed = line.trim();
                if trimmed.starts_with("### ")
                    || trimmed.starts_with("- Status:")
                    || trimmed.contains("Deterministic execution notes")
                    || trimmed.starts_with("- Parent task:")
                    || line.starts_with("  - ")
                    || line.starts_with("    - ")
                {
                    notes.push(trimmed.to_string());
                }
            }
            if !notes.is_empty() {
                summaries.push(truncate_inline(&notes.join(" "), 1_000));
            }
        }
        idx = end;
    }
    summaries
}

pub(crate) fn deterministic_recipe_summaries(paths: &Paths, limit: usize) -> Vec<String> {
    let Ok(entries) = fs::read_dir(&paths.recipes) else {
        return Vec::new();
    };
    let mut recipes = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(recipe) = serde_json::from_str::<DeterministicRecipe>(&text) else {
            continue;
        };
        recipes.push(recipe);
    }
    recipes.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    recipes
        .into_iter()
        .take(limit)
        .map(|recipe| {
            let commands = recipe
                .commands
                .iter()
                .map(|command| format!("`{}`", command.display()))
                .collect::<Vec<_>>()
                .join(", ");
            let occurrences = if recipe.occurrences > 1 {
                format!("; seen {}x", recipe.occurrences)
            } else {
                String::new()
            };
            truncate_inline(
                &format!(
                    "{} from {} [{}{}]: {}",
                    recipe.recipe_id, recipe.source_task_id, recipe.status, occurrences, commands
                ),
                1_000,
            )
        })
        .collect()
}

fn recent_rust_evidence_job_receipts(paths: &Paths, limit: usize) -> Vec<RustEvidenceJobReceipt> {
    let dir = paths.observations.join("evidence_jobs");
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut receipts = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(receipt) = serde_json::from_str::<RustEvidenceJobReceipt>(&text) else {
            continue;
        };
        receipts.push(receipt);
    }
    receipts.sort_by(|a, b| b.completed_at.cmp(&a.completed_at));
    receipts.truncate(limit);
    receipts
}

fn automatic_recipe_harvest(paths: &Paths, cfg: &Config, state: &mut State) -> Result<usize> {
    // Harvesting never approves work.  It records successful command chains as
    // advisory recipe candidates that must be revalidated before reuse.
    if !cfg.automatic_recipe_harvest || !cfg.deterministic_tasks {
        return Ok(0);
    }
    fs::create_dir_all(&paths.recipes)?;
    let migrated = crate::learning::migrate_legacy_observations(paths)?;
    if !paths.runs.exists() {
        return Ok(migrated);
    }
    let mut harvested = 0usize;
    for path in recent_stdout_jsonl_logs(paths, cfg.recipe_harvest_recent_logs)? {
        let commands = harvest_recipe_commands_from_log(&path)?;
        if commands.is_empty() {
            continue;
        }
        let evidence_id = format!("run-log-{}", stable_hash_hex(&display_path(paths, &path)));
        let _ = crate::learning::observe_successful_commands(
            paths,
            commands.clone(),
            vec![evidence_id],
        )?;
        if upsert_harvested_recipe(paths, &path, commands)? {
            harvested += 1;
        }
    }
    if harvested > 0 {
        update_recipe_index(paths)?;
        let summary =
            format!("Harvested or promoted {harvested} deterministic recipe candidate(s).");
        println!("[Rust] {summary}");
        push_event(state, "deterministic", "recipe-harvest", &summary);
    }
    Ok(harvested.saturating_add(migrated))
}

fn recent_stdout_jsonl_logs(paths: &Paths, limit: usize) -> Result<Vec<PathBuf>> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(&paths.runs)? {
        let entry = entry?;
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !name.ends_with(".stdout.jsonl") {
            continue;
        }
        let modified = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .ok();
        entries.push((modified, path));
    }
    entries.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
    Ok(entries
        .into_iter()
        .take(limit)
        .map(|(_, path)| path)
        .collect())
}

fn harvest_recipe_commands_from_log(path: &Path) -> Result<Vec<VerificationCommand>> {
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let reader = BufReader::new(file);
    let mut commands = Vec::new();
    let mut seen = HashSet::new();
    for line in reader.lines() {
        let line = line?;
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let Some(item) = value.get("item") else {
            continue;
        };
        if value.get("type").and_then(Value::as_str) != Some("item.completed")
            || item.get("type").and_then(Value::as_str) != Some("command_execution")
            || item.get("exit_code").and_then(Value::as_i64) != Some(0)
        {
            continue;
        }
        let Some(command) = item.get("command").and_then(Value::as_str) else {
            continue;
        };
        let command = friendly_command(command);
        let Some(command) = parse_verification_command(&command, "log.harvest") else {
            continue;
        };
        if is_recipe_harvest_command_allowed(&command) && seen.insert(command.fingerprint()) {
            commands.push(command);
        }
    }
    Ok(commands)
}

fn is_recipe_harvest_command_allowed(command: &VerificationCommand) -> bool {
    is_deterministic_command_structured_allowed(command)
        && ((command.program == "swift"
            && matches!(
                command.args.first().map(String::as_str),
                Some("test" | "build")
            ))
            || (command.program == "cargo"
                && matches!(
                    command.args.first().map(String::as_str),
                    Some("test" | "check" | "build")
                ))
            || (command.program == "npm"
                && (command.args.first().map(String::as_str) == Some("test")
                    || command.args.first().map(String::as_str) == Some("run")
                        && command.args.get(1).map(String::as_str) == Some("test")))
            || (command.program == "pnpm"
                && command.args.first().map(String::as_str) == Some("test"))
            || (command.program == "yarn"
                && command.args.first().map(String::as_str) == Some("test"))
            || command.program == "pytest"
            || (command.program == "python"
                && command.args.first().map(String::as_str) == Some("-m")
                && command.args.get(1).map(String::as_str) == Some("pytest"))
            || (command.program == "git"
                && command.args == ["diff".to_string(), "--check".to_string()])
            || is_local_debug_smoke_command_structured_allowed(command))
}

fn upsert_harvested_recipe(
    paths: &Paths,
    log_path: &Path,
    command_list: Vec<VerificationCommand>,
) -> Result<bool> {
    let command_fingerprints = command_list
        .iter()
        .map(VerificationCommand::fingerprint)
        .collect::<Vec<_>>()
        .join("\n");
    let recipe_id = format!("harvest-{}", stable_hash_hex(&command_fingerprints));
    let path = paths.recipes.join(format!("{recipe_id}.json"));
    let log_label = display_path(paths, log_path);
    let now = Utc::now().to_rfc3339();
    let commands = command_list
        .into_iter()
        .map(|command| command.with_source("log.harvest"))
        .collect::<Vec<_>>();

    let mut recipe = if path.exists() {
        serde_json::from_str::<DeterministicRecipe>(&read_text(&path)?)?
    } else {
        DeterministicRecipe {
            schema_version: 1,
            recipe_id: recipe_id.clone(),
            status: "candidate".to_string(),
            source_task_id: "log-harvest".to_string(),
            parent_task: None,
            title: "Harvested deterministic command chain".to_string(),
            kind: "verification".to_string(),
            created_at: now.clone(),
            allowed_files: vec!["NEXT_STEPS_non-ai.md".to_string()],
            read_only_files: Vec::new(),
            forbidden_actions: vec![
                "Do not edit source files.".to_string(),
                "Do not approve tasks or roadmap direction.".to_string(),
            ],
            commands,
            occurrences: 0,
            source_logs: Vec::new(),
            updated_at: None,
            evidence: "Harvested from successful command executions in Codex JSONL logs. Advisory command recipe only.".to_string(),
            rust_policy: "run/classify/record only; no source edits, no task invention, no approval".to_string(),
        }
    };

    if recipe.source_logs.iter().any(|source| source == &log_label) {
        return Ok(false);
    }
    recipe.source_logs.push(log_label);
    recipe.occurrences = recipe.occurrences.saturating_add(1).max(1);
    recipe.updated_at = Some(now);
    recipe.status = if recipe.occurrences >= 3 {
        "auto-runnable".to_string()
    } else if recipe.occurrences >= 2 {
        "trusted".to_string()
    } else {
        "candidate".to_string()
    };
    write_text(
        &path,
        &format!("{}\n", serde_json::to_string_pretty(&recipe)?),
    )?;
    Ok(true)
}

pub(crate) fn stable_hash_hex(text: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn record_worker_deterministic_recipe(
    paths: &Paths,
    task: &TaskSpec,
    worker: &WorkerOutput,
    rust_verification: &Value,
) -> Result<Option<String>> {
    let commands = collect_recipe_commands(task, worker, rust_verification);
    if commands.is_empty() {
        return Ok(None);
    }
    fs::create_dir_all(&paths.recipes)?;
    let evidence_id = format!(
        "accepted-task-{}",
        stable_hash_hex(&format!("{}:{}", task.task_id, task.title))
    );
    let proposal =
        crate::learning::observe_successful_commands(paths, commands.clone(), vec![evidence_id])?;
    let autonomously_approved = proposal
        .as_ref()
        .map(|proposal| {
            crate::learning::autonomously_approve_observed_recipe_if_enrolled(paths, task, proposal)
        })
        .transpose()?
        .flatten();
    let recipe_id = format!("{}-deterministic", sanitize_filename(&task.task_id));
    let recipe = DeterministicRecipe {
        schema_version: 1,
        recipe_id: recipe_id.clone(),
        status: "candidate".to_string(),
        source_task_id: task.task_id.clone(),
        parent_task: task.parent_task.clone(),
        title: task.title.clone(),
        kind: task.kind.clone(),
        created_at: Utc::now().to_rfc3339(),
        allowed_files: task.allowed_files.clone(),
        read_only_files: task.read_only_files.clone(),
        forbidden_actions: task.forbidden_actions.clone(),
        commands,
        occurrences: 1,
        source_logs: Vec::new(),
        updated_at: None,
        evidence: "Derived from an accepted task's explicit verification contract, Worker handoff requests, and passed Rust deterministic verification. Rust may reuse only the command list after normal task-card safety checks; this recipe does not authorize source edits or task approval.".to_string(),
        rust_policy: "run/classify/record only; no source edits, no task invention, no approval".to_string(),
    };
    let path = paths.recipes.join(format!("{recipe_id}.json"));
    write_text(
        &path,
        &format!("{}\n", serde_json::to_string_pretty(&recipe)?),
    )?;
    update_recipe_index(paths)?;
    Ok(Some(if let Some(approved) = autonomously_approved {
        format!(
            "Recorded and autonomously isolated-approved evidence-only recipe {} revision {} with {} command(s).",
            approved.recipe_id,
            approved.revision_digest,
            recipe.commands.len()
        )
    } else {
        format!(
            "Recorded deterministic recipe candidate {} with {} command(s).",
            recipe.recipe_id,
            recipe.commands.len()
        )
    }))
}

fn collect_recipe_commands(
    task: &TaskSpec,
    worker: &WorkerOutput,
    rust_verification: &Value,
) -> Vec<DeterministicRecipeCommand> {
    let mut seen = HashSet::new();
    let mut commands = Vec::new();
    for command in &task.tests_to_run {
        push_recipe_command(&mut commands, &mut seen, command, "task.verification");
    }
    for command in &worker.handoff_to_rust {
        push_recipe_verification_command(
            &mut commands,
            &mut seen,
            command.clone().with_source("worker.handoff_to_rust"),
        );
    }
    if let Some(items) = rust_verification
        .get("commands")
        .and_then(|value| value.as_array())
    {
        for item in items {
            if !rust_verification_command_item_passed(item) {
                continue;
            }
            if let Some(command) = item
                .get("structured_command")
                .or_else(|| item.get("command"))
            {
                if let Ok(command) = serde_json::from_value::<VerificationCommand>(command.clone())
                {
                    push_recipe_verification_command(
                        &mut commands,
                        &mut seen,
                        command.with_source("rust.verification"),
                    );
                    continue;
                }
            }
            if let Some(command) = item
                .get("command")
                .or_else(|| item.get("command_display"))
                .and_then(|value| value.as_str())
            {
                push_recipe_command(&mut commands, &mut seen, command, "rust.verification");
            }
        }
    }
    commands
}

fn rust_verification_command_item_passed(item: &Value) -> bool {
    if let Some(success) = item.get("success").and_then(Value::as_bool) {
        return success;
    }
    if let Some(category) = item.get("category").and_then(Value::as_str) {
        return category == "passed";
    }
    if let Some(exit_code) = item.get("exit_code").and_then(Value::as_i64) {
        return exit_code == 0;
    }
    true
}

fn push_recipe_command(
    commands: &mut Vec<DeterministicRecipeCommand>,
    seen: &mut HashSet<String>,
    command: &str,
    source: &str,
) {
    if let Some(command) = parse_verification_command(command, source) {
        push_recipe_verification_command(commands, seen, command);
    }
}

fn push_recipe_verification_command(
    commands: &mut Vec<DeterministicRecipeCommand>,
    seen: &mut HashSet<String>,
    command: VerificationCommand,
) {
    if !is_deterministic_command_structured_allowed(&command) {
        return;
    }
    if seen.insert(command.fingerprint()) {
        commands.push(command);
    }
}

fn update_recipe_index(paths: &Paths) -> Result<()> {
    let summaries = deterministic_recipe_summaries(paths, 100);
    let mut text = String::from("# Deterministic Recipes\n\n");
    text.push_str("Rust-generated candidate recipes derived from accepted Worker tasks. These are advisory command recipes only: they do not authorize source edits, task approval, or roadmap decisions.\n\n");
    if summaries.is_empty() {
        text.push_str("No deterministic recipe candidates recorded yet.\n");
    } else {
        for summary in summaries {
            text.push_str("- ");
            text.push_str(&summary);
            text.push('\n');
        }
    }
    write_text(&paths.recipes.join("README.md"), &text)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RepoObservationSnapshot {
    observed_at: String,
    git_head: Option<String>,
    dirty_hash: String,
    changed_files: Vec<ObservedChangedFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RepoChangeJournalEntry {
    observed_at: String,
    git_head: Option<String>,
    dirty_hash: String,
    changed_files: Vec<ObservedChangedFile>,
    verification_hints: Vec<String>,
    note: String,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
enum AdoptionReportClass {
    UserBaseline,
    OrchestratorInit,
    GeneratedOrIgnored,
    LegacyUnknown,
}

impl AdoptionReportClass {
    fn as_str(self) -> &'static str {
        match self {
            Self::UserBaseline => "user_baseline",
            Self::OrchestratorInit => "orchestrator_init",
            Self::GeneratedOrIgnored => "generated_or_ignored",
            Self::LegacyUnknown => "legacy_unknown",
        }
    }

    fn heading(self) -> &'static str {
        match self {
            Self::UserBaseline => "User Baseline Files (capped)",
            Self::OrchestratorInit => "Orchestrator Init Files (capped)",
            Self::GeneratedOrIgnored => "Generated/Ignored Files (capped)",
            Self::LegacyUnknown => "Legacy Unknown Files (capped)",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct FileAffinityIndex {
    updated_at: String,
    all_pairs: BTreeMap<String, u64>,
    source_test_pairs: BTreeMap<String, u64>,
    source_pairs: BTreeMap<String, u64>,
    test_pairs: BTreeMap<String, u64>,
    tracking_doc_pairs: BTreeMap<String, u64>,
    non_tracking_pairs: BTreeMap<String, u64>,
    pairs: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct VerificationHintIndex {
    updated_at: String,
    hints_by_file: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Clone)]
struct GlobalLearningSummary {
    global_dir: PathBuf,
    codex_sessions_added: usize,
    git_commits_added: usize,
    verification_hint_files: usize,
    non_tracking_affinity_pairs: usize,
}

#[derive(Debug, Clone)]
struct ObservationHint {
    scope: &'static str,
    file: String,
    command: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CodexSessionObservation {
    observed_at: String,
    source_file: String,
    session_id: Option<String>,
    cwd: Option<String>,
    model: Option<String>,
    effort: Option<String>,
    event_count: u64,
    command_count: u64,
    failed_command_count: u64,
    patch_count: u64,
    changed_files: Vec<String>,
    command_patterns: BTreeMap<String, u64>,
    #[serde(default)]
    verification_command_patterns: BTreeMap<String, u64>,
    #[serde(default)]
    exploration_command_patterns: BTreeMap<String, u64>,
    #[serde(default)]
    git_mutation_command_patterns: BTreeMap<String, u64>,
    #[serde(default)]
    status_command_patterns: BTreeMap<String, u64>,
    #[serde(default)]
    other_command_patterns: BTreeMap<String, u64>,
    failure_patterns: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct CodexSessionIndex {
    updated_at: String,
    session_count: u64,
    command_patterns: BTreeMap<String, u64>,
    #[serde(default)]
    verification_command_patterns: BTreeMap<String, u64>,
    #[serde(default)]
    exploration_command_patterns: BTreeMap<String, u64>,
    #[serde(default)]
    git_mutation_command_patterns: BTreeMap<String, u64>,
    #[serde(default)]
    status_command_patterns: BTreeMap<String, u64>,
    #[serde(default)]
    other_command_patterns: BTreeMap<String, u64>,
    failure_patterns: BTreeMap<String, u64>,
    file_activity: BTreeMap<String, u64>,
    cwd_counts: BTreeMap<String, u64>,
    model_counts: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CodexCommandCategory {
    Verification,
    Exploration,
    GitMutation,
    Status,
    Other,
}

fn observe_codex_session_exports(paths: &Paths, input: &Path) -> Result<usize> {
    let files = codex_session_export_files(input)?;
    if files.is_empty() {
        return Ok(0);
    }
    fs::create_dir_all(&paths.observations)?;
    let seen_path = paths.observations.join("codex_sessions_seen.json");
    let mut seen = read_json_file::<BTreeSet<String>>(&seen_path).unwrap_or_default();
    populate_seen_codex_session_export_keys_from_journal(paths, &mut seen);
    let mut observed = 0usize;
    for file in files {
        let key = codex_session_file_key(&file)?;
        if !seen.insert(key) {
            continue;
        }
        let observation = mine_codex_session_export(&file)?;
        append_jsonl(
            &paths.observations.join("codex_sessions_journal.jsonl"),
            &serde_json::to_value(&observation)?,
        )?;
        observed += 1;
    }
    write_text(
        &seen_path,
        &format!("{}\n", serde_json::to_string_pretty(&seen)?),
    )?;
    update_codex_session_index(paths)?;
    Ok(observed)
}

fn codex_session_export_files(input: &Path) -> Result<Vec<PathBuf>> {
    if input.is_file() {
        return Ok(vec![input.to_path_buf()]);
    }
    if !input.is_dir() {
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    for entry in fs::read_dir(input)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("jsonl") {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

fn codex_session_file_key(path: &Path) -> Result<String> {
    let meta = fs::metadata(path)?;
    let modified = meta
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    Ok(format!("{}:{}:{}", path.display(), meta.len(), modified))
}

fn populate_seen_codex_session_export_keys_from_journal(
    paths: &Paths,
    seen: &mut BTreeSet<String>,
) {
    let journal = paths.observations.join("codex_sessions_journal.jsonl");
    for line in read_text_or_empty(&journal).lines() {
        let Ok(observation) = serde_json::from_str::<CodexSessionObservation>(line) else {
            continue;
        };
        let path = Path::new(&observation.source_file);
        if let Ok(key) = codex_session_file_key(path) {
            seen.insert(key);
        }
    }
}

fn mine_codex_session_export(path: &Path) -> Result<CodexSessionObservation> {
    let file =
        File::open(path).with_context(|| format!("open Codex session {}", path.display()))?;
    let reader = BufReader::new(file);
    let mut session_id = None;
    let mut cwd = None;
    let mut model = None;
    let mut effort = None;
    let mut event_count = 0u64;
    let mut command_count = 0u64;
    let mut failed_command_count = 0u64;
    let mut patch_count = 0u64;
    let mut changed_files = BTreeSet::new();
    let mut command_patterns = BTreeMap::new();
    let mut verification_command_patterns = BTreeMap::new();
    let mut exploration_command_patterns = BTreeMap::new();
    let mut git_mutation_command_patterns = BTreeMap::new();
    let mut status_command_patterns = BTreeMap::new();
    let mut other_command_patterns = BTreeMap::new();
    let mut failure_patterns = BTreeMap::new();

    for line in reader.lines() {
        let line = line?;
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        event_count += 1;
        let payload = value.get("payload").unwrap_or(&Value::Null);
        match value.get("type").and_then(Value::as_str) {
            Some("session_meta") => {
                session_id = payload
                    .get("id")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .or(session_id);
                cwd = payload
                    .get("cwd")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .or(cwd);
            }
            Some("turn_context") => {
                cwd = payload
                    .get("cwd")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .or(cwd);
                model = payload
                    .get("model")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .or(model);
                effort = payload
                    .get("effort")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .or(effort);
            }
            Some("event_msg") => {
                if payload.get("type").and_then(Value::as_str) == Some("exec_command_end") {
                    command_count += 1;
                    let command = codex_event_command_display(payload);
                    let pattern = codex_command_pattern(&command);
                    *command_patterns.entry(pattern.clone()).or_insert(0) += 1;
                    let category = codex_command_category(&command);
                    let bucket = codex_command_bucket_mut(
                        category,
                        &mut verification_command_patterns,
                        &mut exploration_command_patterns,
                        &mut git_mutation_command_patterns,
                        &mut status_command_patterns,
                        &mut other_command_patterns,
                    );
                    *bucket.entry(pattern.clone()).or_insert(0) += 1;
                    let exit_code = payload.get("exit_code").and_then(Value::as_i64);
                    if exit_code.unwrap_or(0) != 0
                        || payload.get("status").and_then(Value::as_str) == Some("failed")
                    {
                        failed_command_count += 1;
                        let failure = classify_codex_session_failure(payload, &command);
                        *failure_patterns.entry(failure).or_insert(0) += 1;
                    }
                } else if payload.get("type").and_then(Value::as_str) == Some("patch_apply_end") {
                    patch_count += 1;
                    if let Some(changes) = payload.get("changes").and_then(Value::as_object) {
                        for file in changes.keys() {
                            changed_files.insert(file.clone());
                        }
                    }
                    if payload.get("success").and_then(Value::as_bool) == Some(false) {
                        *failure_patterns
                            .entry("apply_patch_context_or_patch_failure".to_string())
                            .or_insert(0) += 1;
                    }
                }
            }
            _ => {}
        }
    }

    Ok(CodexSessionObservation {
        observed_at: Utc::now().to_rfc3339(),
        source_file: path.to_string_lossy().to_string(),
        session_id,
        cwd,
        model,
        effort,
        event_count,
        command_count,
        failed_command_count,
        patch_count,
        changed_files: changed_files.into_iter().collect(),
        command_patterns,
        verification_command_patterns,
        exploration_command_patterns,
        git_mutation_command_patterns,
        status_command_patterns,
        other_command_patterns,
        failure_patterns,
    })
}

fn codex_event_command_display(payload: &Value) -> String {
    if let Some(parts) = payload.get("command").and_then(Value::as_array) {
        if parts.len() >= 3
            && parts.get(1).and_then(Value::as_str) == Some("-lc")
            && parts.get(2).and_then(Value::as_str).is_some()
        {
            return parts
                .get(2)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
        }
        return parts
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(" ");
    }
    payload
        .get("command")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn codex_command_pattern(command: &str) -> String {
    let normalized = canonical_codex_command(command);
    let trimmed = normalized.as_str();
    if trimmed.starts_with("swift test --filter ") {
        return trimmed
            .split_whitespace()
            .take(4)
            .collect::<Vec<_>>()
            .join(" ");
    }
    if trimmed.starts_with("swift build --product ") {
        return trimmed
            .split_whitespace()
            .take(4)
            .collect::<Vec<_>>()
            .join(" ");
    }
    trimmed
        .split_whitespace()
        .take(3)
        .collect::<Vec<_>>()
        .join(" ")
}

fn codex_command_category(command: &str) -> CodexCommandCategory {
    let normalized = canonical_codex_command(command);
    let without_env = normalized.as_str();
    let first = without_env.split_whitespace().next().unwrap_or_default();

    if without_env.starts_with("swift test")
        || without_env.starts_with("swift build")
        || without_env.starts_with("cargo test")
        || without_env.starts_with("cargo build")
        || without_env.starts_with("cargo fmt")
        || without_env.starts_with("cargo check")
        || without_env.starts_with("npm test")
        || without_env.starts_with("npm run test")
        || without_env.starts_with("npm run build")
        || without_env.starts_with("yarn test")
        || without_env.starts_with("yarn build")
        || without_env.starts_with("pnpm test")
        || without_env.starts_with("pnpm build")
        || without_env.contains(" git diff --check")
        || without_env == "git diff --check"
    {
        return CodexCommandCategory::Verification;
    }

    if without_env.starts_with("git add ")
        || without_env.starts_with("git commit ")
        || without_env.starts_with("git mv ")
        || without_env.starts_with("git rm ")
        || without_env.starts_with("git restore ")
        || without_env.starts_with("git checkout ")
        || without_env.starts_with("git reset ")
    {
        return CodexCommandCategory::GitMutation;
    }

    if matches!(
        first,
        "sed" | "nl" | "rg" | "grep" | "cat" | "head" | "tail" | "ls" | "find"
    ) || without_env.starts_with("git diff ")
        || without_env.starts_with("git show ")
        || without_env.starts_with("git log ")
        || without_env.starts_with("git rev-parse ")
        || without_env.starts_with("git ls-files")
    {
        return CodexCommandCategory::Exploration;
    }

    if without_env == "git status"
        || without_env.starts_with("git status ")
        || without_env == "pwd"
        || without_env == "true"
        || without_env == "date"
        || without_env.starts_with("wc ")
    {
        return CodexCommandCategory::Status;
    }

    CodexCommandCategory::Other
}

fn canonical_codex_command(command: &str) -> String {
    let without_cd = trim_leading_cd(command.trim());
    trim_leading_env_assignments(without_cd).to_string()
}

fn trim_leading_cd(command: &str) -> &str {
    let mut rest = command.trim_start();
    loop {
        let trimmed = rest.trim_start();
        if !trimmed.starts_with("cd ") {
            return trimmed;
        }
        let Some((_, tail)) = trimmed.split_once("&&") else {
            return trimmed;
        };
        rest = tail;
    }
}

fn trim_leading_env_assignments(command: &str) -> &str {
    let mut rest = command.trim_start();
    loop {
        let Some((first, tail)) = rest.split_once(char::is_whitespace) else {
            return rest;
        };
        if first.contains('=') && !first.starts_with('-') {
            rest = tail.trim_start();
            continue;
        }
        return rest;
    }
}

fn codex_command_bucket_mut<'a>(
    category: CodexCommandCategory,
    verification: &'a mut BTreeMap<String, u64>,
    exploration: &'a mut BTreeMap<String, u64>,
    git_mutation: &'a mut BTreeMap<String, u64>,
    status: &'a mut BTreeMap<String, u64>,
    other: &'a mut BTreeMap<String, u64>,
) -> &'a mut BTreeMap<String, u64> {
    match category {
        CodexCommandCategory::Verification => verification,
        CodexCommandCategory::Exploration => exploration,
        CodexCommandCategory::GitMutation => git_mutation,
        CodexCommandCategory::Status => status,
        CodexCommandCategory::Other => other,
    }
}

fn merge_counts(target: &mut BTreeMap<String, u64>, source: BTreeMap<String, u64>) {
    for (key, count) in source {
        *target.entry(key).or_insert(0) += count;
    }
}

fn normalize_codex_session_changed_file(file: &str, cwd: Option<&str>, repo_root: &Path) -> String {
    let path = Path::new(file);
    if path.is_absolute() {
        if let Ok(relative) = path.strip_prefix(repo_root) {
            return relative.to_string_lossy().to_string();
        }
        if let Some(cwd) = cwd {
            if let Ok(relative) = path.strip_prefix(cwd) {
                return relative.to_string_lossy().to_string();
            }
        }
    }
    file.to_string()
}

fn classify_codex_session_failure(payload: &Value, command: &str) -> String {
    let output = [
        payload
            .get("stdout")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        payload
            .get("stderr")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        payload
            .get("aggregated_output")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    ]
    .join("\n");
    let fingerprint = failure_fingerprint_from_text(Some(command), None, &output);
    if fingerprint.kind != FailureFingerprintKind::Unknown {
        return fingerprint.kind.as_str().to_string();
    }
    let lower = output.to_ascii_lowercase();
    if lower.contains("no such file or directory") {
        "missing_path".to_string()
    } else if lower.contains("no matches")
        || lower.contains("exit code: 1") && command.starts_with("rg ")
    {
        "rg_no_matches".to_string()
    } else if command.trim().starts_with("swift test") {
        "swift_test_failure".to_string()
    } else if command.trim().starts_with("swift build") {
        "swift_build_failure".to_string()
    } else {
        "command_failed".to_string()
    }
}

fn update_codex_session_index(paths: &Paths) -> Result<()> {
    update_codex_session_index_in_dir(&paths.observations, Some(&paths.root))
}

fn update_codex_session_index_in_dir(
    observations_dir: &Path,
    repo_root: Option<&Path>,
) -> Result<()> {
    let journal = observations_dir.join("codex_sessions_journal.jsonl");
    let mut index = CodexSessionIndex {
        updated_at: Utc::now().to_rfc3339(),
        ..CodexSessionIndex::default()
    };
    let mut indexed_sessions = BTreeSet::new();
    for line in read_text_or_empty(&journal).lines() {
        let Ok(observation) = serde_json::from_str::<CodexSessionObservation>(line) else {
            continue;
        };
        let session_key = codex_session_observation_key(&observation);
        if !indexed_sessions.insert(session_key) {
            continue;
        }
        index.session_count += 1;
        let cwd = observation.cwd.clone();
        if let Some(cwd) = cwd.as_ref() {
            *index.cwd_counts.entry(cwd.clone()).or_insert(0) += 1;
        }
        if let Some(model) = observation.model.as_ref() {
            *index.model_counts.entry(model.clone()).or_insert(0) += 1;
        }
        if observation.command_patterns.is_empty() {
            merge_counts(
                &mut index.verification_command_patterns,
                observation.verification_command_patterns,
            );
            merge_counts(
                &mut index.exploration_command_patterns,
                observation.exploration_command_patterns,
            );
            merge_counts(
                &mut index.git_mutation_command_patterns,
                observation.git_mutation_command_patterns,
            );
            merge_counts(
                &mut index.status_command_patterns,
                observation.status_command_patterns,
            );
            merge_counts(
                &mut index.other_command_patterns,
                observation.other_command_patterns,
            );
        } else {
            for (pattern, count) in &observation.command_patterns {
                *index.command_patterns.entry(pattern.clone()).or_insert(0) += *count;
                let categorized_pattern = codex_command_pattern(pattern);
                let bucket = codex_command_bucket_mut(
                    codex_command_category(pattern),
                    &mut index.verification_command_patterns,
                    &mut index.exploration_command_patterns,
                    &mut index.git_mutation_command_patterns,
                    &mut index.status_command_patterns,
                    &mut index.other_command_patterns,
                );
                *bucket.entry(categorized_pattern).or_insert(0) += *count;
            }
        }
        for (pattern, count) in observation.failure_patterns {
            *index.failure_patterns.entry(pattern).or_insert(0) += count;
        }
        for file in observation.changed_files {
            let normalized = if let Some(repo_root) = repo_root {
                normalize_codex_session_changed_file(&file, cwd.as_deref(), repo_root)
            } else {
                file
            };
            *index.file_activity.entry(normalized).or_insert(0) += 1;
        }
    }
    write_text(
        &observations_dir.join("codex_sessions_index.json"),
        &format!("{}\n", serde_json::to_string_pretty(&index)?),
    )
}

fn codex_session_observation_key(observation: &CodexSessionObservation) -> String {
    format!(
        "{}|{}|{}|{}",
        observation.source_file,
        observation.session_id.as_deref().unwrap_or_default(),
        observation.event_count,
        observation.command_count
    )
}

pub(crate) fn update_history_index(paths: &Paths) -> Result<()> {
    update_history_index_in_dir(&paths.observations)
}

fn update_history_index_in_dir(observations_dir: &Path) -> Result<()> {
    let history_path = observations_dir.join("git_history_journal.jsonl");
    if !history_path.exists() {
        return Ok(());
    }
    let mut commit_count = 0usize;
    let mut file_counts = BTreeMap::<String, u64>::new();
    let mut hint_counts = BTreeMap::<String, u64>::new();
    for line in read_text_or_empty(&history_path).lines() {
        let Ok(observation) = serde_json::from_str::<GitHistoryObservation>(line) else {
            continue;
        };
        commit_count += 1;
        for file in observation.changed_files {
            *file_counts.entry(file.path).or_insert(0) += 1;
        }
        for hint in observation.verification_hints {
            *hint_counts.entry(hint).or_insert(0) += 1;
        }
    }
    write_text(
        &observations_dir.join("git_history_index.json"),
        &format!(
            "{}\n",
            serde_json::to_string_pretty(&json!({
                "updated_at": Utc::now().to_rfc3339(),
                "commit_count": commit_count,
                "file_counts": file_counts,
                "hint_counts": hint_counts,
                "note": "Advisory history-derived local memory. Does not approve tasks or commands."
            }))?
        ),
    )
}

fn observe_repo_change_state(paths: &Paths, state: &mut State) -> Result<usize> {
    if find_git_root(&paths.root).is_none() {
        return Ok(0);
    }
    let snapshot = current_repo_observation_snapshot(paths)?;
    let snapshot_path = paths.observations.join("repo_snapshot.json");
    let previous = read_json_file::<RepoObservationSnapshot>(&snapshot_path).ok();
    if previous.as_ref().is_some_and(|old| {
        old.dirty_hash == snapshot.dirty_hash && old.git_head == snapshot.git_head
    }) {
        return Ok(0);
    }

    fs::create_dir_all(&paths.observations)?;
    write_text(
        &snapshot_path,
        &format!("{}\n", serde_json::to_string_pretty(&snapshot)?),
    )?;

    if snapshot.changed_files.is_empty() {
        return Ok(0);
    }

    let observed_count = snapshot.changed_files.len();
    let hints = observation_verification_hints(&snapshot.changed_files);
    let entry = RepoChangeJournalEntry {
        observed_at: snapshot.observed_at.clone(),
        git_head: snapshot.git_head.clone(),
        dirty_hash: snapshot.dirty_hash.clone(),
        changed_files: snapshot.changed_files.clone(),
        verification_hints: hints.clone(),
        note: "Advisory local repo observation. This records external or pre-cycle dirty state; it does not approve tasks or commits.".to_string(),
    };
    append_jsonl(
        &paths.observations.join("change_journal.jsonl"),
        &serde_json::to_value(&entry)?,
    )?;
    update_file_affinity_index(paths, &snapshot.changed_files)?;
    update_verification_hint_index(paths, &snapshot.changed_files, &hints)?;
    append_event(
        paths,
        OrchestratorEventKind::SnapshotCompared,
        None,
        json!({
            "mode": "repo_change_observation",
            "changed_files": snapshot.changed_files,
            "verification_hints": hints,
            "dirty_hash": snapshot.dirty_hash,
        }),
    )?;
    let summary = format!(
        "Observed {} dirty repo file(s); wrote advisory change journal.",
        entry.changed_files.len()
    );
    push_event(state, "rust-observer", "repo-change", &summary);
    println!("[Rust] {summary}");
    Ok(observed_count)
}

fn pause_for_large_dirty_adoption_if_needed(paths: &Paths, state: &mut State) -> Result<bool> {
    if !state.needs_reconciliation || state.human_disposition_required.is_some() {
        return Ok(false);
    }
    if find_git_root(&paths.root).is_none() {
        return Ok(false);
    }
    let snapshot = current_repo_observation_snapshot(paths)?;
    if !dirty_adoption_requires_human_disposition(&snapshot.changed_files) {
        return Ok(false);
    }

    fs::create_dir_all(&paths.observations)?;
    let report_path = paths.observations.join("dirty_adoption_report.md");
    let report = render_dirty_adoption_report(paths, &snapshot);
    write_text(&report_path, &report)?;
    let report_label = report_path
        .strip_prefix(&paths.root)
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| report_path.display().to_string());
    let reason = format!(
        "Rust detected {} dirty adoption file(s) requiring human disposition during adoption/reconciliation. Report: {report_label}. Choose a human disposition before Worker, Project Manager, or CEO continues.",
        snapshot.changed_files.len()
    );
    state.human_disposition_required = Some(HumanDispositionPause {
        reason: reason.clone(),
        options: vec![
            "preserve/quarantine the dirty set as user-owned context".to_string(),
            "revert or abandon the dirty set outside Worker".to_string(),
            "isolate the dirty set onto a separate branch or patch".to_string(),
            "approve a new explicit safety gate for the dirty set".to_string(),
        ],
        source: "rust_dirty_adoption".to_string(),
    });
    push_event(
        state,
        "rust",
        "dirty-adoption-pause",
        &format!(
            "Paused adoption for {} dirty file(s); report: {report_label}",
            snapshot.changed_files.len()
        ),
    );
    append_event(
        paths,
        OrchestratorEventKind::StateTransitionApplied,
        None,
        json!({
            "transition": "dirty_adoption_human_disposition_required",
            "dirty_count": snapshot.changed_files.len(),
            "report": report_label,
        }),
    )?;
    println!("[Rust] {reason}");
    Ok(true)
}

fn dirty_adoption_requires_human_disposition(changed_files: &[ObservedChangedFile]) -> bool {
    if changed_files.is_empty() {
        return false;
    }
    let has_untracked = changed_files.iter().any(|file| file.status == "??");
    let has_tracked = changed_files.iter().any(|file| file.status != "??");
    let categories = changed_files
        .iter()
        .map(|file| file.category.as_str())
        .collect::<BTreeSet<_>>();
    changed_files.len() > 12 || (has_untracked && has_tracked) || categories.len() > 3
}

fn render_dirty_adoption_report(paths: &Paths, snapshot: &RepoObservationSnapshot) -> String {
    let mut by_status: BTreeMap<String, usize> = BTreeMap::new();
    let mut by_top_level: BTreeMap<String, usize> = BTreeMap::new();
    let scaffold_record = adoption_scaffold_record(paths);
    let created_scaffold_files = scaffold_record
        .as_ref()
        .map(|record| {
            record
                .created_files
                .iter()
                .cloned()
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    let scaffold_provenance_missing = scaffold_record.is_none();
    let mut by_class: BTreeMap<String, usize> = BTreeMap::new();
    let mut class_examples: BTreeMap<AdoptionReportClass, Vec<String>> = BTreeMap::new();
    for file in &snapshot.changed_files {
        *by_status.entry(file.status.clone()).or_default() += 1;
        *by_top_level
            .entry(top_level_path_bucket(&file.path))
            .or_default() += 1;
        let class = dirty_adoption_report_class(
            &file.path,
            &created_scaffold_files,
            scaffold_provenance_missing,
        );
        *by_class.entry(class.as_str().to_string()).or_default() += 1;
        let examples = class_examples.entry(class).or_default();
        if examples.len() < 40 {
            examples.push(format!("- `{}` {}", file.status, file.path));
        }
    }
    let tracked = snapshot
        .changed_files
        .iter()
        .filter(|file| file.status != "??")
        .take(40)
        .map(|file| format!("- `{}` {}", file.status, file.path))
        .collect::<Vec<_>>();
    let untracked = snapshot
        .changed_files
        .iter()
        .filter(|file| file.status == "??")
        .take(40)
        .map(|file| format!("- `{}` {}", file.status, file.path))
        .collect::<Vec<_>>();

    format!(
        "# Dirty Adoption Report\n\nObserved at: {}\nGit head: {}\nDirty hash: {}\nTotal dirty files: {}\n\n## Counts By Status\n{}\n\n## Counts By Top-Level Path\n{}\n\n## Counts By Adoption Class\n{}\n\n## Modified Tracked Files (capped)\n{}\n\n## Untracked Files (capped)\n{}\n\n## {}\n{}\n\n## {}\n{}\n\n## {}\n{}\n\n## {}\n{}\n\nRust generated this report during adoption/reconciliation. `user_baseline` means dirty files Rust should treat as human-owned context; `orchestrator_init` means files recorded as created by `codex-team init`; `generated_or_ignored` means files under generated artifact roots; `legacy_unknown` means root files that `codex-team init` may create but whose provenance is unknown because no adoption scaffold record exists. This report does not approve, stage, commit, revert, or quarantine any file.\n",
        snapshot.observed_at,
        snapshot.git_head.as_deref().unwrap_or("unknown"),
        snapshot.dirty_hash,
        snapshot.changed_files.len(),
        render_count_lines(&by_status),
        render_count_lines(&by_top_level),
        render_count_lines(&by_class),
        render_report_lines(&tracked),
        render_report_lines(&untracked),
        AdoptionReportClass::UserBaseline.heading(),
        render_report_lines(
            class_examples
                .get(&AdoptionReportClass::UserBaseline)
                .map(Vec::as_slice)
                .unwrap_or(&[])
        ),
        AdoptionReportClass::OrchestratorInit.heading(),
        render_report_lines(
            class_examples
                .get(&AdoptionReportClass::OrchestratorInit)
                .map(Vec::as_slice)
                .unwrap_or(&[])
        ),
        AdoptionReportClass::GeneratedOrIgnored.heading(),
        render_report_lines(
            class_examples
                .get(&AdoptionReportClass::GeneratedOrIgnored)
                .map(Vec::as_slice)
                .unwrap_or(&[])
        ),
        AdoptionReportClass::LegacyUnknown.heading(),
        render_report_lines(
            class_examples
                .get(&AdoptionReportClass::LegacyUnknown)
                .map(Vec::as_slice)
                .unwrap_or(&[])
        ),
    )
}

fn adoption_scaffold_record(paths: &Paths) -> Option<AdoptionScaffoldRecord> {
    read_json_file::<AdoptionScaffoldRecord>(&paths.observations.join("adoption_scaffold.json"))
        .ok()
}

fn dirty_adoption_report_class(
    path: &str,
    created_scaffold_files: &BTreeSet<String>,
    scaffold_provenance_missing: bool,
) -> AdoptionReportClass {
    if path == ".orchestrator" || path.starts_with(".orchestrator/") {
        return AdoptionReportClass::OrchestratorInit;
    }
    if created_scaffold_files.contains(path) {
        return AdoptionReportClass::OrchestratorInit;
    }
    if scaffold_provenance_missing && is_legacy_unknown_adoption_root_doc(path) {
        return AdoptionReportClass::LegacyUnknown;
    }
    if is_generated_or_ignored_adoption_path(path) {
        return AdoptionReportClass::GeneratedOrIgnored;
    }
    AdoptionReportClass::UserBaseline
}

fn is_legacy_unknown_adoption_root_doc(path: &str) -> bool {
    matches!(
        path,
        "ROADMAP.md" | "NEXT_STEPS.md" | "NEXT_STEPS_non-ai.md" | "AGENTS.md"
    )
}

fn is_generated_or_ignored_adoption_path(path: &str) -> bool {
    path == "analysis/codex-wrapper"
        || path.starts_with("analysis/codex-wrapper/")
        || path == "analysis/graph"
        || path.starts_with("analysis/graph/")
        || path.starts_with("analysis/graph-")
        || path.starts_with("Benchmarks/Reports")
        || path == "Benchmarks/Stacks"
        || path.starts_with("Benchmarks/Stacks/")
        || path == "SwiftStacker.app"
        || path.starts_with("SwiftStacker.app/")
        || path == "HeliconFocus.app"
        || path.starts_with("HeliconFocus.app/")
        || path == "HeliconFocus.dmg"
        || path == "export"
        || path.starts_with("export/")
}

fn render_count_lines(counts: &BTreeMap<String, usize>) -> String {
    if counts.is_empty() {
        return "- none".to_string();
    }
    counts
        .iter()
        .map(|(key, count)| format!("- `{key}`: {count}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_report_lines(lines: &[String]) -> String {
    if lines.is_empty() {
        "- none".to_string()
    } else {
        lines.join("\n")
    }
}

fn top_level_path_bucket(path: &str) -> String {
    path.split('/').next().unwrap_or(path).to_string()
}

fn current_repo_observation_snapshot(paths: &Paths) -> Result<RepoObservationSnapshot> {
    current_repo_observation_snapshot_with_options(paths, false)
}

fn current_repo_observation_snapshot_with_options(
    paths: &Paths,
    include_orchestrator: bool,
) -> Result<RepoObservationSnapshot> {
    let porcelain = Command::new("git")
        .args(["status", "--porcelain=v1", "--untracked-files=all"])
        .current_dir(&paths.root)
        .output()
        .with_context(|| "run git status --porcelain")?;
    if !porcelain.status.success() {
        bail!("git status --porcelain failed");
    }
    let status_text = String::from_utf8_lossy(&porcelain.stdout);
    let mut changed_files = Vec::new();
    for line in status_text.lines() {
        if let Some(file) = parse_git_porcelain_line(line) {
            if file.path == ".orchestrator/codex-team.lock"
                || file.path.ends_with("/codex-team.lock")
            {
                // Runtime lock files are ownership artifacts, not adoption inventory.
                continue;
            }
            if !include_orchestrator
                && (file.path.starts_with(".orchestrator/") || file.path == ".orchestrator")
            {
                continue;
            }
            changed_files.push(file);
        }
    }
    changed_files.sort_by(|a, b| a.path.cmp(&b.path).then(a.status.cmp(&b.status)));
    changed_files.dedup_by(|a, b| a.path == b.path && a.status == b.status);
    let dirty_material = format!(
        "{}\n{}",
        git_head(paths).unwrap_or_default(),
        changed_files
            .iter()
            .map(|file| format!("{} {}", file.status, file.path))
            .collect::<Vec<_>>()
            .join("\n")
    );
    Ok(RepoObservationSnapshot {
        observed_at: Utc::now().to_rfc3339(),
        git_head: git_head(paths),
        dirty_hash: stable_hash_hex(&dirty_material),
        changed_files,
    })
}

pub(crate) fn update_file_affinity_index(
    paths: &Paths,
    changed_files: &[ObservedChangedFile],
) -> Result<()> {
    let path = paths.observations.join("file_affinity.json");
    let mut index = read_json_file::<FileAffinityIndex>(&path).unwrap_or_default();
    normalize_file_affinity_index(&mut index);
    index.updated_at = Utc::now().to_rfc3339();
    add_affinity_pairs_to_index(&mut index, changed_files);
    write_text(
        &path,
        &format!("{}\n", serde_json::to_string_pretty(&index)?),
    )
}

fn add_affinity_pairs_to_index(
    index: &mut FileAffinityIndex,
    changed_files: &[ObservedChangedFile],
) {
    let mut files = changed_files
        .iter()
        .map(|file| (file.path.clone(), file.category.clone()))
        .collect::<Vec<_>>();
    files.sort_by(|a, b| a.0.cmp(&b.0));
    files.dedup_by(|a, b| a.0 == b.0);
    for i in 0..files.len() {
        for j in (i + 1)..files.len() {
            let key = format!("{} || {}", files[i].0, files[j].0);
            *index.all_pairs.entry(key.clone()).or_insert(0) += 1;
            *index.pairs.entry(key.clone()).or_insert(0) += 1;
            let left = files[i].1.as_str();
            let right = files[j].1.as_str();
            if left == "tracking_doc" || right == "tracking_doc" {
                *index.tracking_doc_pairs.entry(key.clone()).or_insert(0) += 1;
            } else {
                *index.non_tracking_pairs.entry(key.clone()).or_insert(0) += 1;
            }
            match (left, right) {
                ("source", "test") | ("test", "source") => {
                    *index.source_test_pairs.entry(key).or_insert(0) += 1;
                }
                ("source", "source") => {
                    *index.source_pairs.entry(key).or_insert(0) += 1;
                }
                ("test", "test") => {
                    *index.test_pairs.entry(key).or_insert(0) += 1;
                }
                _ => {}
            }
        }
    }
}

fn normalize_file_affinity_index(index: &mut FileAffinityIndex) {
    if index.all_pairs.is_empty() && !index.pairs.is_empty() {
        index.all_pairs = index.pairs.clone();
    }
    index.source_test_pairs.clear();
    index.source_pairs.clear();
    index.test_pairs.clear();
    index.tracking_doc_pairs.clear();
    index.non_tracking_pairs.clear();
    for (key, count) in index.all_pairs.clone() {
        let Some((left_path, right_path)) = key.split_once(" || ") else {
            continue;
        };
        let left = observed_file_category(left_path);
        let right = observed_file_category(right_path);
        if left == "tracking_doc" || right == "tracking_doc" {
            index.tracking_doc_pairs.insert(key.clone(), count);
        } else {
            index.non_tracking_pairs.insert(key.clone(), count);
        }
        match (left, right) {
            ("source", "test") | ("test", "source") => {
                index.source_test_pairs.insert(key, count);
            }
            ("source", "source") => {
                index.source_pairs.insert(key, count);
            }
            ("test", "test") => {
                index.test_pairs.insert(key, count);
            }
            _ => {}
        }
    }
}

pub(crate) fn rebuild_file_affinity_from_observation_journals(paths: &Paths) -> Result<()> {
    rebuild_file_affinity_from_observation_dir(&paths.observations)
}

fn rebuild_file_affinity_from_observation_dir(observations_dir: &Path) -> Result<()> {
    let mut index = FileAffinityIndex {
        updated_at: Utc::now().to_rfc3339(),
        ..FileAffinityIndex::default()
    };
    let history_path = observations_dir.join("git_history_journal.jsonl");
    for line in read_text_or_empty(&history_path).lines() {
        if let Ok(observation) = serde_json::from_str::<GitHistoryObservation>(line) {
            add_affinity_pairs_to_index(&mut index, &observation.changed_files);
        }
    }
    let current_path = observations_dir.join("change_journal.jsonl");
    for line in read_text_or_empty(&current_path).lines() {
        if let Ok(observation) = serde_json::from_str::<RepoChangeJournalEntry>(line) {
            add_affinity_pairs_to_index(&mut index, &observation.changed_files);
        }
    }
    write_text(
        &observations_dir.join("file_affinity.json"),
        &format!("{}\n", serde_json::to_string_pretty(&index)?),
    )
}

pub(crate) fn update_verification_hint_index(
    paths: &Paths,
    changed_files: &[ObservedChangedFile],
    hints: &[String],
) -> Result<()> {
    if hints.is_empty() {
        return Ok(());
    }
    let path = paths.observations.join("verification_hints.json");
    let mut index = read_json_file::<VerificationHintIndex>(&path).unwrap_or_default();
    index.updated_at = Utc::now().to_rfc3339();
    for file in changed_files {
        let entry = index.hints_by_file.entry(file.path.clone()).or_default();
        for hint in hints {
            if !entry.contains(hint) {
                entry.push(hint.clone());
            }
        }
        entry.sort();
    }
    write_text(
        &path,
        &format!("{}\n", serde_json::to_string_pretty(&index)?),
    )
}

fn promote_local_observations_to_global(
    paths: &Paths,
    cfg: &Config,
) -> Result<GlobalLearningSummary> {
    let global_dir = global_observations_dir(cfg);
    fs::create_dir_all(&global_dir)?;
    let codex_sessions_added = merge_codex_session_observations(
        &paths.observations.join("codex_sessions_journal.jsonl"),
        &global_dir.join("codex_sessions_journal.jsonl"),
    )?;
    let git_commits_added = merge_git_history_observations(
        &paths.observations.join("git_history_journal.jsonl"),
        &global_dir.join("git_history_journal.jsonl"),
    )?;
    update_codex_session_index_in_dir(&global_dir, None)?;
    update_history_index_in_dir(&global_dir)?;
    rebuild_file_affinity_from_observation_dir(&global_dir)?;
    rebuild_verification_hints_from_history_dir(&global_dir)?;

    let hint_index =
        read_json_file::<VerificationHintIndex>(&global_dir.join("verification_hints.json"))
            .unwrap_or_default();
    let affinity_index =
        read_json_file::<FileAffinityIndex>(&global_dir.join("file_affinity.json"))
            .unwrap_or_default();
    append_event(
        paths,
        OrchestratorEventKind::GlobalMemoryApplied,
        None,
        json!({
            "mode": "learn_global",
            "global_dir": global_dir.display().to_string(),
            "codex_sessions_added": codex_sessions_added,
            "git_commits_added": git_commits_added,
            "verification_hint_files": hint_index.hints_by_file.len(),
            "non_tracking_affinity_pairs": affinity_index.non_tracking_pairs.len(),
        }),
    )?;

    Ok(GlobalLearningSummary {
        global_dir,
        codex_sessions_added,
        git_commits_added,
        verification_hint_files: hint_index.hints_by_file.len(),
        non_tracking_affinity_pairs: affinity_index.non_tracking_pairs.len(),
    })
}

fn merge_codex_session_observations(local_journal: &Path, global_journal: &Path) -> Result<usize> {
    let mut seen = BTreeSet::new();
    for line in read_text_or_empty(global_journal).lines() {
        let Ok(observation) = serde_json::from_str::<CodexSessionObservation>(line) else {
            continue;
        };
        seen.insert(codex_session_observation_key(&observation));
    }
    let mut added = 0usize;
    for line in read_text_or_empty(local_journal).lines() {
        let Ok(observation) = serde_json::from_str::<CodexSessionObservation>(line) else {
            continue;
        };
        if seen.insert(codex_session_observation_key(&observation)) {
            append_jsonl(global_journal, &serde_json::to_value(&observation)?)?;
            added += 1;
        }
    }
    Ok(added)
}

fn rebuild_verification_hints_from_history_dir(observations_dir: &Path) -> Result<()> {
    let mut index = VerificationHintIndex {
        updated_at: Utc::now().to_rfc3339(),
        ..VerificationHintIndex::default()
    };
    let history_path = observations_dir.join("git_history_journal.jsonl");
    for line in read_text_or_empty(&history_path).lines() {
        let Ok(observation) = serde_json::from_str::<GitHistoryObservation>(line) else {
            continue;
        };
        for file in observation.changed_files {
            let entry = index.hints_by_file.entry(file.path).or_default();
            for hint in &observation.verification_hints {
                if !entry.contains(hint) {
                    entry.push(hint.clone());
                }
            }
            entry.sort();
        }
    }
    write_text(
        &observations_dir.join("verification_hints.json"),
        &format!("{}\n", serde_json::to_string_pretty(&index)?),
    )
}

fn global_observations_dir(cfg: &Config) -> PathBuf {
    expand_user_path(&cfg.global_observations_dir)
}

fn expand_user_path(path: &str) -> PathBuf {
    if path == "~" {
        return std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(path));
    }
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(path)
}

fn recover_approved_next_steps_work(paths: &Paths, state: &mut State) -> Result<usize> {
    if !state.queue.is_empty() {
        return Ok(0);
    }
    let recovered = enqueue_approved_next_steps_tasks(paths, state)?;
    if recovered > 0 {
        state.needs_reconciliation = false;
        state.planner_due = false;
        state.master_due = false;
    }
    Ok(recovered)
}

#[cfg(test)]
fn promote_pending_next_steps_work(paths: &Paths, state: &mut State) -> Result<usize> {
    if !state.queue.is_empty() || !paths.next_steps.exists() {
        return Ok(0);
    }
    let text = read_text_or_empty(&paths.next_steps);
    if !parse_approved_next_steps_tasks(&text).is_empty() {
        return Ok(0);
    }
    let done_ids = next_steps_completed_task_ids(&text);
    let outputs = next_steps_outputs(&text);
    let mut promoted = 0;
    for task in parse_pending_next_steps_tasks(&text) {
        if !pending_task_dependencies_satisfied(&task, &done_ids, &outputs) {
            continue;
        }
        if !pending_task_ready_for_rust_approval(&task) {
            continue;
        }
        approve_pending_task_in_next_steps(paths, &task)?;
        append_event(
            paths,
            OrchestratorEventKind::StateTransitionApplied,
            Some(&task.task_id),
            json!({
                "from": "pending approval",
                "to": "approved",
                "mode": "rust_existing_backlog_approval"
            }),
        )?;
        if enqueue_back(state, task.clone()) {
            println!(
                "[Rust] Approved existing pending task {} from NEXT_STEPS.md.",
                task.task_id
            );
            promoted += 1;
        }
    }
    if promoted > 0 {
        state.needs_reconciliation = false;
        state.planner_due = false;
        state.master_due = false;
    }
    Ok(promoted)
}

fn run_next_non_ai_task(paths: &Paths, cfg: &Config, state: &mut State) -> Result<Option<bool>> {
    // Non-AI queue drain: execute only the first approved deterministic task, then
    // return to the outer cycle so the user sees progress and can stop.
    if !paths.next_steps_non_ai.exists() {
        return Ok(None);
    }
    let text = read_text_or_empty(&paths.next_steps_non_ai);
    let approved_tasks = parse_approved_next_steps_tasks(&text);
    let Some(task) = approved_tasks.into_iter().next() else {
        if let Some(should_continue) = try_run_non_ai_patch_packet(paths, cfg, state, &text)? {
            return Ok(Some(should_continue));
        }
        if try_enqueue_non_ai_command_packet_task(paths, state, &text)? {
            return Ok(None);
        }
        if non_ai_command_packet_all_tasks_completed(paths, &text) {
            return Ok(None);
        }
        if next_steps_non_ai_has_unparsed_execution_packet(&text) {
            let summary = "NEXT_STEPS_non-ai.md contains a command packet, but no approved deterministic task card that Rust can execute. Pausing before CEO/PM/Worker; convert the packet into an approved rust-only verification task or move source-edit work to NEXT_STEPS.md.";
            pause_for_non_ai_queue_blocker(state, summary);
            println!("[Rust] {summary}");
            return Ok(Some(false));
        }
        return Ok(None);
    };
    if !cfg.deterministic_tasks {
        println!("[Rust] NEXT_STEPS_non-ai.md has approved work, but deterministic_tasks=false.");
        return Ok(Some(false));
    }
    if !is_deterministic_task_candidate(&task)
        || !task
            .tests_to_run
            .iter()
            .all(|command| is_deterministic_command_allowed(command))
    {
        let summary = format!(
            "NEXT_STEPS_non-ai.md task {} is not safe for deterministic execution; leaving it untouched and pausing before CEO/PM/Worker.",
            task.task_id
        );
        pause_for_non_ai_queue_blocker(state, &summary);
        println!("[Rust] {summary}");
        return Ok(Some(false));
    }

    match try_run_deterministic_task(
        paths,
        cfg,
        state,
        &task,
        &paths.next_steps_non_ai,
        "NEXT_STEPS_non-ai.md",
    )? {
        Some(_) if state.pending_commit.is_some() => {
            if !state
                .queue
                .iter()
                .any(|queued| queued.task_id == task.task_id)
            {
                state.queue.insert(0, task.clone());
            }
            state.last_review_summary = Some(format!(
                "{} is verified and paused for explicit commit approval.",
                task.task_id
            ));
            Ok(Some(false))
        }
        Some(result) if result.status == "done" => {
            state.completed_tasks += 1;
            state.last_worker_summary = Some(result.summary.clone());
            state.last_review_summary = Some(result.summary.clone());
            push_event(state, "deterministic", "done", &result.summary);
            if !next_steps_has_unfinished_task(paths)?
                && !next_steps_non_ai_has_unfinished_task(paths)
            {
                state.master_due = false;
                state.planner_due = false;
                state.needs_reconciliation = false;
            }
            Ok(Some(true))
        }
        Some(result) => {
            let n = state.failed_tasks.entry(task.task_id.clone()).or_insert(0);
            *n += 1;
            state.last_worker_summary = Some(result.summary.clone());
            push_event(state, "deterministic", &result.status, &result.summary);
            if result.confidence != "high" {
                state.planner_due = true;
            }
            Ok(Some(false))
        }
        None => Ok(Some(false)),
    }
}

fn try_run_non_ai_patch_packet(
    paths: &Paths,
    cfg: &Config,
    state: &mut State,
    text: &str,
) -> Result<Option<bool>> {
    let Some(packet) = first_non_ai_command_packet(text) else {
        return Ok(None);
    };
    let patch = non_ai_packet_unified_patch(&packet);
    if patch.trim().is_empty() {
        return Ok(None);
    }
    let patch_files = match unified_diff_paths(&patch) {
        Ok(files) if !files.is_empty() => files,
        Ok(_) => {
            let summary = format!(
                "NEXT_STEPS_non-ai.md command packet `{}` has a patch block, but Rust could not find changed paths.",
                packet.title
            );
            pause_for_non_ai_queue_blocker(state, &summary);
            println!("[Rust] {summary}");
            return Ok(Some(false));
        }
        Err(err) => {
            let summary = format!(
                "NEXT_STEPS_non-ai.md command packet `{}` has an unsafe patch block: {err:#}.",
                packet.title
            );
            pause_for_non_ai_queue_blocker(state, &summary);
            println!("[Rust] {summary}");
            return Ok(Some(false));
        }
    };

    let allowed =
        non_ai_packet_allowed_files(paths, &packet).unwrap_or_else(|| patch_files.clone());
    let allowed_globs = compile_globs(&expand_allowed_file_patterns(&allowed))?;
    let outside_allowed = patch_files
        .iter()
        .filter(|path| !allowed_globs.is_match(path.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    if !outside_allowed.is_empty() {
        let summary = format!(
            "NEXT_STEPS_non-ai.md command packet `{}` patch touches files outside Edit Targets: {}.",
            packet.title,
            outside_allowed.join(", ")
        );
        pause_for_non_ai_queue_blocker(state, &summary);
        println!("[Rust] {summary}");
        return Ok(Some(false));
    }

    let mut task = non_ai_patch_task_from_packet(paths, &packet, patch_files.clone());
    let commands = non_ai_packet_validation_commands(&packet);
    task.tests_to_run = if commands.is_empty() {
        vec!["git diff --check".to_string()]
    } else {
        commands
    };

    println!(
        "[Rust] Applying NEXT_STEPS_non-ai.md patch packet `{}` as {} without Worker.",
        packet.title, task.task_id
    );
    apply_unified_patch_with_git(paths, &task, &patch)?;

    let mut results = Vec::new();
    for command in &task.tests_to_run {
        let Some(command) = parse_verification_command(command, "non-ai.patch.validation") else {
            let summary = format!(
                "{} patch validation command is not in Rust's deterministic allowlist: {}",
                task.task_id,
                truncate_inline(command, 160)
            );
            pause_for_non_ai_queue_blocker(state, &summary);
            println!("[Rust] {summary}");
            return Ok(Some(false));
        };
        let result = run_verification_command_with_policy(paths, cfg, &command)?;
        let should_stop = !result.category.is_success_like();
        results.push(result);
        if should_stop {
            break;
        }
    }

    let final_category = results
        .iter()
        .find(|result| !result.category.is_success_like())
        .map(|result| result.category);
    if final_category.is_some() {
        mark_task_status_mechanically(&paths.next_steps_non_ai, &task, "Blocked.", &results)?;
        let summary = format!(
            "{} Rust patch packet applied, but validation did not pass; leaving changes uncommitted for inspection.",
            task.task_id
        );
        state.last_worker_summary = Some(summary.clone());
        state.last_review_summary = Some(summary.clone());
        push_event(state, "deterministic", "blocked", &summary);
        println!("[Rust] {summary}");
        return Ok(Some(false));
    }

    let queue_before = capture_rust_owned_file(paths, &paths.next_steps_non_ai)?;
    mark_task_status_mechanically(&paths.next_steps_non_ai, &task, "Done.", &results)?;
    let commit_sha = commit_rust_patch_task(
        paths,
        cfg,
        state,
        &task,
        &paths.next_steps_non_ai,
        &patch_files,
        &queue_before,
    )?;
    if state.pending_commit.is_some() {
        if !state
            .queue
            .iter()
            .any(|queued| queued.task_id == task.task_id)
        {
            state.queue.insert(0, task.clone());
        }
        state.last_review_summary = Some(format!(
            "{} is verified and paused for explicit commit approval.",
            task.task_id
        ));
        return Ok(Some(false));
    }
    let summary = format!(
        "{} was applied and verified by the Rust patch lane{}; Worker/CEO/PM were skipped.",
        task.task_id,
        commit_sha
            .as_ref()
            .map(|sha| format!("; commit {}", truncate_inline(sha, 12)))
            .unwrap_or_default()
    );
    println!("[Rust] {summary}");
    state.completed_tasks += 1;
    state.master_due = false;
    state.planner_due = false;
    state.needs_reconciliation = false;
    state.retry_worker_same_task = None;
    state.retry_worker_brief = None;
    state.senior_dev_repair_order = None;
    state.last_worker_summary = Some(summary.clone());
    state.last_review_summary = Some(summary.clone());
    push_event(state, "deterministic", "rust-patch-done", &summary);
    let _ = record_rust_model_skip(
        paths,
        "worker",
        Some(&task.task_id),
        cfg,
        "non-ai patch packet applied by Rust",
    );
    let _ = record_rust_model_skip(
        paths,
        "foreman-review",
        Some(&task.task_id),
        cfg,
        "non-ai patch packet verified by Rust",
    );
    append_event(
        paths,
        OrchestratorEventKind::StateTransitionApplied,
        Some(&task.task_id),
        json!({
            "source": "NEXT_STEPS_non-ai.md",
            "mode": "non_ai_patch_applied",
            "files": patch_files,
            "commit": commit_sha,
        }),
    )?;
    Ok(Some(true))
}

fn non_ai_patch_task_from_packet(
    paths: &Paths,
    packet: &NonAiCommandPacket,
    patch_files: Vec<String>,
) -> TaskSpec {
    let mut allowed_files = non_ai_packet_allowed_files(paths, packet).unwrap_or_default();
    allowed_files.extend(patch_files);
    allowed_files.sort();
    allowed_files.dedup();
    TaskSpec {
        task_id: format!("{}-PATCH", non_ai_packet_task_id(&packet.title)),
        parent_task: None,
        status: "approved".to_string(),
        priority: "P1".to_string(),
        review_tier: ReviewTier::RustOnly,
        automation_level: AutomationLevel::RustPatch,
        dependencies: Vec::new(),
        title: format!("{} - Rust Patch", packet.title),
        kind: "rust_patch".to_string(),
        objective: non_ai_packet_objective(packet),
        why_unblocks_progress: "NEXT_STEPS_non-ai.md included an explicit unified diff, so Rust can apply and verify it without Worker.".to_string(),
        allowed_files,
        read_only_files: non_ai_packet_read_only_files(packet),
        forbidden_actions: vec![
            "Do not launch Worker, CEO, Project Manager, or Senior Dev for this explicit patch packet.".to_string(),
            "Do not apply patch hunks outside the packet's Edit Targets.".to_string(),
        ],
        exact_steps: vec![
            "Apply the exact unified diff from NEXT_STEPS_non-ai.md with `git apply --check` followed by `git apply`.".to_string(),
            "Run deterministic validation commands from the packet.".to_string(),
            "Commit only the patched files and NEXT_STEPS_non-ai.md if validation passes.".to_string(),
        ],
        success_criteria: vec![
            "The explicit patch applies cleanly.".to_string(),
            "All deterministic validation commands pass or are classified by Rust.".to_string(),
        ],
        tests_to_run: Vec::new(),
        stop_conditions: non_ai_packet_section_items(packet, "Stop Conditions"),
        discovery_budget: "No discovery required; Rust applies the explicit patch only.".to_string(),
        estimated_minutes: 5,
        requires_network: false,
        prefer_worktree: None,
    }
}

fn non_ai_packet_unified_patch(packet: &NonAiCommandPacket) -> String {
    let mut blocks = Vec::new();
    let mut in_patch_fence = false;
    let mut current = Vec::new();
    for line in packet.body.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            let fence = trimmed.trim_start_matches("```").trim();
            if in_patch_fence {
                in_patch_fence = false;
                let block = current.join("\n");
                if block.contains("diff --git ") || block.contains("\n--- ") {
                    blocks.push(block);
                }
                current.clear();
            } else {
                in_patch_fence = matches!(fence, "diff" | "patch");
                current.clear();
            }
            continue;
        }
        if in_patch_fence {
            current.push(line.to_string());
        }
    }
    blocks.join("\n")
}

fn unified_diff_paths(patch: &str) -> Result<Vec<String>> {
    let mut paths = BTreeSet::new();
    for line in patch.lines() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            let parts = rest.split_whitespace().collect::<Vec<_>>();
            if parts.len() >= 2 {
                collect_unified_diff_path(parts[0], &mut paths)?;
                collect_unified_diff_path(parts[1], &mut paths)?;
            }
        } else if let Some(path) = line.strip_prefix("--- ") {
            collect_unified_diff_path(path.split_whitespace().next().unwrap_or(path), &mut paths)?;
        } else if let Some(path) = line.strip_prefix("+++ ") {
            collect_unified_diff_path(path.split_whitespace().next().unwrap_or(path), &mut paths)?;
        }
    }
    Ok(paths.into_iter().collect())
}

fn collect_unified_diff_path(raw: &str, paths: &mut BTreeSet<String>) -> Result<()> {
    let path = raw.trim();
    if path == "/dev/null" {
        return Ok(());
    }
    let path = path
        .strip_prefix("a/")
        .or_else(|| path.strip_prefix("b/"))
        .unwrap_or(path);
    if path.starts_with('/')
        || path.contains("..")
        || path.starts_with(".orchestrator/")
        || path.trim().is_empty()
    {
        bail!("unsafe patch path `{raw}`");
    }
    paths.insert(path.to_string());
    Ok(())
}

fn try_enqueue_non_ai_command_packet_task(
    paths: &Paths,
    state: &mut State,
    text: &str,
) -> Result<bool> {
    let tasks = non_ai_command_packet_tasks_from_text(paths, text);
    if tasks.is_empty() {
        return Ok(false);
    }
    let completed_ids = next_steps_completed_task_ids(&read_text_or_empty(&paths.next_steps));
    let base_task_id = tasks
        .first()
        .and_then(|task| task.parent_task.clone())
        .unwrap_or_else(|| tasks[0].task_id.clone());
    let base_title = tasks
        .first()
        .map(|task| base_non_ai_packet_title(&task.title))
        .unwrap_or_default();
    let tasks = tasks
        .into_iter()
        .filter(|task| !completed_ids.contains(&task.task_id))
        .collect::<Vec<_>>();
    if tasks.is_empty() {
        return Ok(false);
    }

    let before = state.queue.len();
    state.queue.retain(|queued| {
        queued.task_id != base_task_id
            && queued.parent_task.as_deref() != Some(base_task_id.as_str())
            && normalize_task_title(&base_non_ai_packet_title(&queued.title))
                != normalize_task_title(&base_title)
    });
    let removed = before.saturating_sub(state.queue.len());
    for task in tasks.iter().rev().cloned() {
        if !state
            .queue
            .iter()
            .any(|queued| queued.task_id == task.task_id)
        {
            state.queue.insert(0, task);
        }
    }
    state.master_due = false;
    state.planner_due = false;
    state.needs_reconciliation = false;
    state.retry_worker_same_task = None;
    state.retry_worker_brief = None;
    state.senior_dev_repair_order = None;
    let first_task_id = state
        .queue
        .first()
        .map(|task| task.task_id.clone())
        .unwrap_or_else(|| base_task_id.clone());
    let materialized_count = tasks.len();
    let summary = if removed > 0 {
        format!(
            "Rust materialized NEXT_STEPS_non-ai.md command packet `{base_title}` as {materialized_count} staged Worker task(s), starting with `{}`; removed {removed} stale queued duplicate(s); CEO/PM were skipped.",
            first_task_id
        )
    } else {
        format!(
            "Rust materialized NEXT_STEPS_non-ai.md command packet `{base_title}` as {materialized_count} staged Worker task(s), starting with `{}`; CEO/PM were skipped.",
            first_task_id
        )
    };
    println!("[Rust] {summary}");
    push_event(state, "deterministic", "materialized-worker-task", &summary);
    append_event(
        paths,
        OrchestratorEventKind::StateTransitionApplied,
        Some(&first_task_id),
        json!({
            "source": "NEXT_STEPS_non-ai.md",
            "mode": "command_packet_materialized",
            "title": base_title,
            "task_count": materialized_count,
            "task_ids": tasks.iter().map(|task| task.task_id.clone()).collect::<Vec<_>>(),
            "stale_duplicates_removed": removed,
        }),
    )?;
    Ok(true)
}

fn non_ai_command_packet_all_tasks_completed(paths: &Paths, text: &str) -> bool {
    let tasks = non_ai_command_packet_tasks_from_text(paths, text);
    if tasks.is_empty() {
        return false;
    }
    let completed_ids = next_steps_completed_task_ids(&read_text_or_empty(&paths.next_steps));
    tasks
        .iter()
        .all(|task| completed_ids.contains(&task.task_id))
}

fn non_ai_command_packet_tasks_from_text(paths: &Paths, text: &str) -> Vec<TaskSpec> {
    let Some(task) = non_ai_command_packet_task_from_text(paths, text) else {
        return Vec::new();
    };
    vec![task]
}

fn non_ai_command_packet_task_from_text(paths: &Paths, text: &str) -> Option<TaskSpec> {
    let packet = first_non_ai_command_packet(text)?;
    let allowed_files = non_ai_packet_allowed_files(paths, &packet)?;
    let tests_to_run = non_ai_packet_validation_commands(&packet);
    if tests_to_run.is_empty() {
        return None;
    }

    let mut exact_steps = non_ai_packet_section_items(&packet, "Required Source Changes");
    if exact_steps.is_empty() {
        exact_steps = non_ai_packet_section_items(&packet, "Source edit/codegen objective");
    }
    if exact_steps.is_empty() {
        exact_steps.push(format!(
            "Implement the `{}` command packet exactly as written in `NEXT_STEPS_non-ai.md`.",
            packet.title
        ));
    }
    let preflight = non_ai_packet_shell_commands(&packet, "Preflight Commands")
        .into_iter()
        .filter(|command| !is_swiftpm_command(command))
        .collect::<Vec<_>>();
    if !preflight.is_empty() {
        exact_steps.insert(
            0,
            format!(
                "Before editing, run non-Swift preflight commands from `NEXT_STEPS_non-ai.md`: {}. Do not run SwiftPM build/test preflight inside Worker; Rust final verification owns those commands.",
                preflight.join("; ")
            ),
        );
    }
    exact_steps.insert(
        0,
        "If any SwiftPM build/test command is sandbox/cache-blocked inside Worker, record the known environment blocker and continue implementation; Rust final verification owns pass/fail classification.".to_string(),
    );
    let discovery = non_ai_packet_shell_commands(&packet, "Discovery Commands");
    if !discovery.is_empty() {
        exact_steps.insert(
            usize::min(1, exact_steps.len()),
            format!(
                "Use only the packet discovery commands unless a compile error identifies a nearby required type: {}.",
                discovery.join("; ")
            ),
        );
    }

    let mut stop_conditions = non_ai_packet_section_items(&packet, "Stop Conditions");
    if stop_conditions.is_empty() {
        stop_conditions.push(
            "Stop if implementation requires files or behavior outside the command packet."
                .to_string(),
        );
    }

    let mut forbidden_actions = stop_conditions
        .iter()
        .map(|item| {
            if item.to_ascii_lowercase().starts_with("stop ") {
                item.clone()
            } else {
                format!("Stop if {item}")
            }
        })
        .collect::<Vec<_>>();
    forbidden_actions.extend([
        "Do not call CEO or Project Manager for this packet; return blocked if the packet is underspecified.".to_string(),
        "Do not stop before source edits solely because a SwiftPM build/test command is sandbox/cache-blocked inside Worker; record the known environment blocker and let Rust final verification classify it.".to_string(),
        "Do not stage, commit, revert, or clean unrelated files.".to_string(),
        "Do not edit `.orchestrator/` artifacts.".to_string(),
    ]);

    Some(TaskSpec {
        task_id: non_ai_packet_task_id(&packet.title),
        parent_task: None,
        status: "approved".to_string(),
        priority: "P1".to_string(),
        review_tier: ReviewTier::AlwaysAiReview,
        automation_level: AutomationLevel::AiWorkerAiReview,
        dependencies: Vec::new(),
        title: packet.title.clone(),
        kind: "implementation".to_string(),
        objective: non_ai_packet_objective(&packet),
        why_unblocks_progress: format!(
            "Rust materialized this directly from `NEXT_STEPS_non-ai.md` in {} so CEO/PM do not reinterpret a concrete packet.",
            paths.root.display()
        ),
        allowed_files,
        read_only_files: non_ai_packet_read_only_files(&packet),
        forbidden_actions,
        exact_steps,
        success_criteria: non_ai_packet_success_criteria(&packet),
        tests_to_run,
        stop_conditions,
        discovery_budget: "Use only the command packet's named edit targets, discovery commands, and nearby compile-error context.".to_string(),
        estimated_minutes: 60,
        requires_network: false,
        prefer_worktree: None,
    })
}

#[derive(Debug, Clone)]
struct NonAiCommandPacket {
    title: String,
    body: String,
}

fn first_non_ai_command_packet(text: &str) -> Option<NonAiCommandPacket> {
    let lines = text.lines().collect::<Vec<_>>();
    let start = lines.iter().position(|line| {
        line.trim_start()
            .to_ascii_lowercase()
            .starts_with("## command packet")
    })?;
    let heading = lines[start].trim();
    let title = heading
        .trim_start_matches('#')
        .trim()
        .strip_prefix("Command Packet:")
        .or_else(|| {
            heading
                .trim_start_matches('#')
                .trim()
                .strip_prefix("Command Packet -")
        })
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("NEXT_STEPS_non-ai Command Packet")
        .to_string();
    let end = lines
        .iter()
        .enumerate()
        .skip(start + 1)
        .find_map(|(idx, line)| {
            let trimmed = line.trim_start();
            (trimmed.starts_with("## ") && !trimmed.starts_with("### ")).then_some(idx)
        })
        .unwrap_or(lines.len());
    Some(NonAiCommandPacket {
        title,
        body: lines[start + 1..end].join("\n"),
    })
}

fn non_ai_packet_task_id(title: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    title.to_ascii_lowercase().hash(&mut hasher);
    format!("NONAI-{:08X}", hasher.finish() as u32)
}

fn normalize_task_title(title: &str) -> String {
    title
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(|ch| ch.to_lowercase())
        .collect()
}

fn base_non_ai_packet_title(title: &str) -> String {
    title
        .split_once(" - Slice ")
        .map(|(base, _)| base)
        .unwrap_or(title)
        .to_string()
}

fn non_ai_packet_allowed_files(paths: &Paths, packet: &NonAiCommandPacket) -> Option<Vec<String>> {
    let mut files = non_ai_packet_section_items(packet, "Edit Targets")
        .into_iter()
        .map(|item| item.trim_matches('`').to_string())
        .filter(|item| {
            !item.is_empty()
                && !item.starts_with("Implementation should stay")
                && !item.starts_with("Source edit")
        })
        .collect::<Vec<_>>();
    files = expand_non_ai_allowed_file_targets(paths, files);
    files.sort();
    files.dedup();
    (!files.is_empty()).then_some(files)
}

fn expand_non_ai_allowed_file_targets(paths: &Paths, files: Vec<String>) -> Vec<String> {
    let mut expanded = Vec::new();
    for file in files {
        if !file.contains('*') && !file.contains('?') && !file.contains('[') {
            expanded.push(file);
            continue;
        }
        let Ok(glob) = Glob::new(&file) else {
            continue;
        };
        let matcher = glob.compile_matcher();
        for entry in WalkDir::new(&paths.root)
            .into_iter()
            .filter_entry(|entry| !is_ignored_index_entry(entry.path()))
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_file())
        {
            let Ok(relative) = entry.path().strip_prefix(&paths.root) else {
                continue;
            };
            let relative = relative.to_string_lossy().to_string();
            if matcher.is_match(relative.as_str()) {
                expanded.push(relative);
            }
        }
    }
    expanded.sort();
    expanded.dedup();
    expanded
}

fn non_ai_packet_read_only_files(packet: &NonAiCommandPacket) -> Vec<String> {
    let mut files = vec![
        "AGENTS.md".to_string(),
        "ROADMAP.md".to_string(),
        "NEXT_STEPS_non-ai.md".to_string(),
    ];
    for command in non_ai_packet_shell_commands(packet, "Discovery Commands") {
        for token in command.split_whitespace() {
            let cleaned = token.trim_matches(|ch: char| {
                ch == '`' || ch == '"' || ch == '\'' || ch == ',' || ch == ';'
            });
            if cleaned.starts_with("Sources/") || cleaned.starts_with("Tests/") {
                files.push(cleaned.to_string());
            }
        }
    }
    files.sort();
    files.dedup();
    files
}

fn non_ai_packet_objective(packet: &NonAiCommandPacket) -> String {
    let goal = non_ai_packet_section_text(packet, "Goal");
    if goal.trim().is_empty() {
        format!(
            "Implement the bounded source-edit command packet `{}` from `NEXT_STEPS_non-ai.md`.",
            packet.title
        )
    } else {
        truncate_inline(&clean_markdown_value(&goal.replace('\n', " ")), 700)
    }
}

fn non_ai_packet_success_criteria(packet: &NonAiCommandPacket) -> Vec<String> {
    let mut criteria = non_ai_packet_section_items(packet, "Required Source Changes");
    if criteria.is_empty() {
        criteria.push(format!("`{}` is implemented as described.", packet.title));
    }
    criteria.push("All Rust-listed post-edit validation commands pass.".to_string());
    criteria
}

fn non_ai_packet_validation_commands(packet: &NonAiCommandPacket) -> Vec<String> {
    let mut commands = non_ai_packet_shell_commands(packet, "Post-Edit Validation Commands");
    if commands.is_empty() {
        commands = non_ai_packet_shell_commands(packet, "Shared Post-Edit Validation");
    }
    commands
        .into_iter()
        .filter(|command| is_deterministic_command_allowed(command))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn non_ai_packet_section_items(packet: &NonAiCommandPacket, heading: &str) -> Vec<String> {
    non_ai_packet_section_text(packet, heading)
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if let Some(item) = trimmed.strip_prefix("- ") {
                Some(clean_markdown_value(item))
            } else {
                strip_numbered_markdown_item(trimmed).map(clean_markdown_value)
            }
        })
        .filter(|item| !item.is_empty())
        .collect()
}

fn non_ai_packet_shell_commands(packet: &NonAiCommandPacket, heading: &str) -> Vec<String> {
    let section = non_ai_packet_section_text(packet, heading);
    let mut commands = Vec::new();
    let mut in_shell_fence = false;
    for line in section.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            let fence = trimmed.trim_start_matches("```").trim();
            if in_shell_fence {
                in_shell_fence = false;
            } else {
                in_shell_fence = fence.is_empty() || fence == "sh" || fence == "bash";
            }
            continue;
        }
        if in_shell_fence
            && !trimmed.is_empty()
            && !trimmed.starts_with('#')
            && !trimmed.starts_with("cd ")
        {
            commands.push(trimmed.to_string());
        }
    }
    commands
}

fn non_ai_packet_section_text(packet: &NonAiCommandPacket, heading: &str) -> String {
    let target = heading.trim().to_ascii_lowercase();
    let mut collecting = false;
    let mut out = Vec::new();
    for line in packet.body.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("### ") {
            let current = trimmed
                .trim_start_matches('#')
                .trim()
                .trim_end_matches(':')
                .to_ascii_lowercase();
            if collecting {
                break;
            }
            collecting = current == target;
            continue;
        }
        if collecting {
            out.push(line);
        }
    }
    out.join("\n")
}

fn pause_for_non_ai_queue_blocker(state: &mut State, summary: &str) {
    state.master_due = false;
    state.planner_due = false;
    state.needs_reconciliation = false;
    state.retry_worker_same_task = None;
    state.retry_worker_brief = None;
    state.senior_dev_repair_order = None;
    state.senior_batch_approval = None;
    state.last_review_summary = Some(summary.to_string());
    push_event(state, "deterministic", "blocked", summary);
}

fn next_steps_non_ai_has_unparsed_execution_packet(text: &str) -> bool {
    if text.trim().is_empty() {
        return false;
    }
    let lower = text.to_ascii_lowercase();
    let has_packet_heading = text.lines().any(|line| {
        let trimmed = line.trim_start().to_ascii_lowercase();
        trimmed.starts_with("## command packet")
            || trimmed.starts_with("## command roadmap")
            || trimmed.starts_with("### slice ")
    });
    let has_command_envelope = lower.contains("```sh")
        && (lower.contains("preflight commands")
            || lower.contains("discovery commands")
            || lower.contains("post-edit validation")
            || lower.contains("commit commands"));

    has_packet_heading || has_command_envelope
}

fn skip_next_false_conditional_task(
    paths: &Paths,
    cfg: &Config,
    state: &mut State,
) -> Result<Option<bool>> {
    if !cfg.deterministic_tasks || !paths.next_steps.exists() || !state.queue.is_empty() {
        return Ok(None);
    }
    let text = read_text_or_empty(&paths.next_steps);
    let Some(skip) = find_false_conditional_task_to_skip(&text) else {
        return Ok(None);
    };

    let queue_before = capture_rust_owned_file(paths, &paths.next_steps)?;
    mark_conditional_task_skipped_by_gate(&paths.next_steps, &skip)?;
    let commit_sha = commit_queue_bookkeeping(
        paths,
        cfg,
        state,
        &paths.next_steps,
        &format!("Skip {} by conditional gate", skip.task_id),
        &queue_before,
    )?;
    if state.pending_commit.is_some() {
        state.last_review_summary = Some(format!(
            "{} is paused for explicit commit approval.",
            skip.task_id
        ));
        return Ok(Some(false));
    }
    let summary = format!(
        "{} was marked Skipped by gate by the Rust conditional gate: {}",
        skip.task_id, skip.evidence
    );
    println!(
        "[Rust] Task {} -> skipped by gate{}",
        skip.task_id,
        commit_sha
            .as_ref()
            .map(|sha| format!("; commit {}", truncate_inline(sha, 12)))
            .unwrap_or_default()
    );
    state.last_worker_summary = Some(summary.clone());
    state.last_review_summary = Some(summary.clone());
    push_event(state, "deterministic", "skipped", &summary);
    Ok(Some(true))
}

fn deterministic_task_queue_target<'a>(
    paths: &'a Paths,
    task: &TaskSpec,
) -> (&'a Path, &'static str) {
    if queue_markdown_contains_task(&paths.next_steps, &task.task_id) {
        (&paths.next_steps, "NEXT_STEPS.md")
    } else {
        (&paths.next_steps_non_ai, "NEXT_STEPS_non-ai.md")
    }
}

fn queue_markdown_contains_task(path: &Path, task_id: &str) -> bool {
    let lines = read_text_or_empty(path)
        .lines()
        .map(str::to_string)
        .collect::<Vec<_>>();
    next_steps_task_section_bounds(&lines, task_id).is_some()
}

#[derive(Debug, Clone)]
struct ConditionalSkip {
    task_id: String,
    evidence: String,
}

fn find_false_conditional_task_to_skip(text: &str) -> Option<ConditionalSkip> {
    let outputs = next_steps_outputs(text);
    let (active_queue_seen, active_task_ids) = active_queue_task_ids(text);
    let mut heading: Option<String> = None;
    let mut body: Vec<String> = Vec::new();
    let mut archived_sections = false;

    for line in text.lines() {
        if next_steps_archive_marker(line) {
            if let Some(skip) = false_conditional_skip_for_section(
                heading.as_deref(),
                &body,
                archived_sections,
                active_queue_seen,
                &active_task_ids,
                &outputs,
            ) {
                return Some(skip);
            }
            heading = None;
            body.clear();
            archived_sections = true;
            continue;
        }
        if line.trim_start().starts_with("### ") {
            if let Some(skip) = false_conditional_skip_for_section(
                heading.as_deref(),
                &body,
                archived_sections,
                active_queue_seen,
                &active_task_ids,
                &outputs,
            ) {
                return Some(skip);
            }
            if archived_sections {
                heading = None;
            } else {
                heading = Some(line.trim().to_string());
            }
            body.clear();
        } else if heading.is_some() {
            body.push(line.to_string());
        }
    }

    false_conditional_skip_for_section(
        heading.as_deref(),
        &body,
        archived_sections,
        active_queue_seen,
        &active_task_ids,
        &outputs,
    )
}

fn false_conditional_skip_for_section(
    heading: Option<&str>,
    body: &[String],
    archived_sections: bool,
    active_queue_seen: bool,
    active_task_ids: &HashSet<String>,
    outputs: &NextStepsOutputMap,
) -> Option<ConditionalSkip> {
    if archived_sections || !is_next_steps_task_section(heading, body) {
        return None;
    }
    let status = clean_markdown_value(&markdown_field_value(body, "Status")?);
    if !status.eq_ignore_ascii_case("conditional approved") {
        return None;
    }
    let task_id =
        markdown_field_value(body, "Task ID").unwrap_or_else(|| heading_task_id(heading, ""));
    if task_id.is_empty() || (active_queue_seen && !active_task_ids.contains(&task_id)) {
        return None;
    }
    if conditional_run_if_satisfied(body, outputs) != Some(false) {
        return None;
    }
    Some(ConditionalSkip {
        task_id,
        evidence: conditional_gate_evidence(body, outputs),
    })
}

fn conditional_gate_evidence(body: &[String], outputs: &NextStepsOutputMap) -> String {
    let mut parts = Vec::new();
    for condition in markdown_list_after(body, "Run if") {
        if condition.trim().eq_ignore_ascii_case("OR") {
            continue;
        }
        let condition = clean_markdown_value(&condition);
        let Some((left, expected)) = condition.split_once("==") else {
            parts.push(format!("unparsed condition `{condition}`"));
            continue;
        };
        let left = left.trim().trim_matches('`');
        let expected = clean_output_token(expected);
        let Some((task_id, output_name)) = left.split_once(".Outputs.") else {
            parts.push(format!("unparsed condition `{condition}`"));
            continue;
        };
        match outputs
            .get(task_id.trim())
            .and_then(|task| task.get(output_name.trim()))
        {
            Some(actual) => parts.push(format!(
                "`{}.Outputs.{}` is `{actual}`, expected `{expected}`",
                task_id.trim(),
                output_name.trim()
            )),
            None => parts.push(format!(
                "`{}.Outputs.{}` is missing, expected `{expected}`",
                task_id.trim(),
                output_name.trim()
            )),
        }
    }
    if parts.is_empty() {
        "run-if condition evaluated false".to_string()
    } else {
        parts.join("; ")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RustEvidenceJobReceipt {
    receipt_id: String,
    action_id: String,
    kind: SeniorProgressActionKind,
    title: String,
    status: String,
    evidence_path: String,
    command_count: usize,
    rejected_commands: Vec<String>,
    command_summaries: Vec<String>,
    blocker_classification: String,
    completed_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AttemptCapsule {
    task_id: String,
    attempt: u32,
    worker_status: String,
    objective: String,
    files_touched: Vec<String>,
    failure_id: Option<String>,
    failure_fingerprint: Option<FailureFingerprint>,
    same_failure_as_prior_attempt: bool,
    diff_summary: String,
    diagnostic_command: Option<String>,
    diagnostic_category: Option<String>,
    diagnostic_summary: Option<String>,
    top_compiler_errors: Vec<String>,
    patch_context_hint: Option<String>,
    next_instruction: String,
    omitted_context: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum Probe {
    // Probe is future-facing scaffolding for bounded Rust-owned discovery.  The
    // goal is to replace broad shell exploration with compact observations.
    GitStatusShort,
    GitDiffStat,
    GitDiffNameOnly,
    Ripgrep {
        pattern: String,
        paths: Vec<String>,
        max_matches: usize,
    },
    ReadFileSlice {
        path: String,
        start_line: usize,
        end_line: usize,
    },
    ListFiles {
        root: String,
        max_results: usize,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq, Hash)]
#[allow(dead_code)]
struct VerificationCacheKey {
    command: VerificationCommand,
    git_head: Option<String>,
    cwd: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AffectedWork {
    // Incremental-analysis scaffold.  The current heuristics are simple, but the
    // type shape is ready for richer repo maps, dependency graphs, and matrices.
    changed_files: Vec<PathBuf>,
    affected_modules: Vec<String>,
    affected_recipes: Vec<RecipeSpec>,
    #[serde(default)]
    dependency_recipes: Vec<DependencyRecipe>,
    likely_tests: Vec<RecipeSpec>,
    confidence: AffectedConfidence,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
enum AffectedConfidence {
    High,
    Medium,
    Low,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
struct VerificationMatrix {
    id: String,
    base_recipe: Recipe,
    axes: Vec<MatrixAxis>,
    max_expansions: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
#[serde(tag = "axis", content = "value", rename_all = "camelCase")]
enum MatrixAxis {
    Package(String),
    Feature(String),
    TestFilter(String),
    Toolchain(String),
    Platform(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DiagnosticFinding {
    // Machine-readable diagnostic extracted from structured tool output or a
    // conservative problem matcher.  Prompts should receive findings, not logs.
    source: FindingSource,
    severity: Severity,
    path: Option<PathBuf>,
    line: Option<u32>,
    column: Option<u32>,
    code: Option<String>,
    message: String,
    raw_excerpt: Option<String>,
}

#[allow(dead_code)]
type Finding = DiagnosticFinding;

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(tag = "source", content = "name", rename_all = "camelCase")]
enum FindingSource {
    CargoCheck,
    CargoClippy,
    CargoTest,
    Nextest,
    Rustfmt,
    GitDiffCheck,
    Ripgrep,
    Guard,
    ProblemMatcher(String),
    CustomPolicy,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
enum Severity {
    Info,
    Warning,
    Error,
    Critical,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq)]
#[allow(dead_code)]
#[serde(rename_all = "camelCase")]
enum TestOutcome {
    Passed,
    Failed,
    Flaky,
    Skipped,
    TimedOut,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
struct TestEvidence {
    // Test evidence is modeled separately so future runners can represent flaky,
    // retried, skipped, or timed-out tests without overloading command exit codes.
    test_name: String,
    outcome: TestOutcome,
    attempts: u8,
    duration_ms: u128,
    stdout_tail: String,
    stderr_tail: String,
    failure_message: Option<String>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct ProblemMatcher {
    name: String,
    pattern: ProblemPattern,
    file_group: Option<usize>,
    line_group: Option<usize>,
    column_group: Option<usize>,
    message_group: usize,
    severity: Severity,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
enum ProblemPattern {
    SwiftCompiler,
    RustCompiler,
    GenericFileLine,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
struct ParsedDiff {
    files: HashMap<String, BTreeSet<u32>>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq)]
#[allow(dead_code)]
#[serde(rename_all = "camelCase")]
enum DiffFilterMode {
    AddedLines,
    DiffContext { context_lines: usize },
    ChangedFiles,
    NoFilter,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
struct FilteredFindings {
    in_diff: Vec<Finding>,
    nearby_diff: Vec<Finding>,
    outside_diff_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WatchPlan {
    task_id: Option<String>,
    paths: Vec<String>,
    debounce_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EfficiencyWatchReport {
    generated_at: String,
    task_id: Option<String>,
    prompt_token_threshold: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    derived_status: Option<DerivedStatus>,
    alerts: Vec<EfficiencyWatchAlert>,
    suggestions: Vec<String>,
    watch: WatchPlan,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RouteScorecardRecord {
    observed_at: String,
    route_kind: String,
    task_id: Option<String>,
    current_lane: ExecutionLane,
    profile: TaskRouteProfile,
    scorecard: RouteScorecard,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RouteBenchmarkReport {
    generated_at: String,
    profiles: Vec<RouteBenchmarkEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RouteBenchmarkEntry {
    name: String,
    synthetic: bool,
    profile: TaskRouteProfile,
    recommended_lane: ExecutionLane,
    actual_lane: Option<ExecutionLane>,
    scorecard: RouteScorecard,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ToolHealthSummaryReport {
    generated_at: String,
    cooldowns: Vec<ToolCooldownSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct RouteRegretReport {
    generated_at: String,
    attempts: usize,
    outcomes: usize,
    calls_avoided: usize,
    estimated_tokens_saved: usize,
    repeated_failures_stopped: usize,
    rust_evidence_substitutions: usize,
    redundant_explorations: usize,
    #[serde(default)]
    estimated_tokens_saved_by_better_lane: BTreeMap<String, usize>,
    by_kind: BTreeMap<String, usize>,
    recent: Vec<RouteRegretEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
struct RouteRegretEntry {
    attempt_id: String,
    task_id: Option<String>,
    kind: String,
    route_kind: String,
    execution_lane: String,
    better_lane: Option<String>,
    tokens: usize,
    failure_fingerprint: Option<String>,
    summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
struct EfficiencyWatchAlert {
    severity: String,
    kind: String,
    summary: String,
    evidence: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    mechanistic_handoff: Option<MechanisticHandoffAlert>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
struct MechanisticHandoffAlert {
    candidate_id: String,
    command: String,
    command_fingerprint: String,
    source_log: Option<String>,
    source_task_id: Option<String>,
    prompt_tokens: Option<usize>,
    observed_duration_ms: Option<u128>,
    receipt_exists: bool,
    action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
struct MechanisticHandoffReport {
    generated_at: String,
    candidates: Vec<MechanisticHandoffCandidate>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
struct MechanisticHandoffCandidate {
    candidate_id: String,
    command: String,
    command_fingerprint: String,
    source: String,
    source_log: Option<String>,
    source_task_id: Option<String>,
    prompt_tokens: Option<usize>,
    observed_duration_ms: Option<u128>,
    receipt_exists: bool,
    action: String,
    reason: String,
    materialized_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EfficiencyLoopReport {
    started_at: String,
    completed_at: String,
    target_repo: String,
    orchestrator_repo: Option<String>,
    max_cycles: usize,
    self_heal: bool,
    orchestrator_cycles: usize,
    stopped_reason: String,
    cycles: Vec<EfficiencyLoopCycle>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EfficiencyLoopCycle {
    iteration: usize,
    target_step_ran: bool,
    target_should_continue: bool,
    target_human_pause: bool,
    watch_alert_count: usize,
    post_heal_alert_count: usize,
    self_heal_action_count: usize,
    orchestrator_feedback_path: Option<String>,
    orchestrator_cycles_run: usize,
    stop_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SupervisedCycleOutcome {
    step_ran: bool,
    should_continue: bool,
    human_pause: bool,
    stopped_reason: Option<String>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum HumanDispositionMode {
    Prompt,
    Stop,
    #[allow(dead_code)]
    Preset(HumanDispositionChoice),
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct SuppressedVerificationHints {
    updated_at: String,
    hints: Vec<SuppressedVerificationHint>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
struct SuppressedVerificationHint {
    command_display: String,
    semantic_fingerprint: String,
    source: String,
    task_id: String,
    category: String,
    suppressed_at: String,
    reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EfficiencySelfHealReport {
    generated_at: String,
    actions: Vec<EfficiencySelfHealAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
struct EfficiencySelfHealAction {
    kind: String,
    status: String,
    summary: String,
    files_changed: Vec<String>,
}

#[derive(Debug, Clone)]
struct StaleObservationHintCommand {
    task_id: String,
    command_display: String,
    category: String,
    source: String,
    completed_at: String,
    semantic_fingerprint: String,
}

#[derive(Debug, Clone, Deserialize)]
struct EvidenceCommandEvent {
    task_id: String,
    report_id: String,
    completed_at: String,
    command: CommandEvidence,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
#[serde(tag = "operation", rename_all = "camelCase")]
enum MechanicalPatch {
    // Future Rust patch lane: AI can propose these operations, but Rust should
    // enforce exact anchors/match counts and allowed-file scope before applying.
    ReplaceExact {
        path: PathBuf,
        find: String,
        replace: String,
    },
    InsertAfter {
        path: PathBuf,
        anchor: String,
        text: String,
    },
    AppendIfMissing {
        path: PathBuf,
        marker: Option<String>,
        text: String,
    },
    RemoveLineMatching {
        path: PathBuf,
        pattern: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
#[serde(tag = "operation", rename_all = "camelCase")]
enum StructuralPatch {
    AstRewrite {
        language: String,
        pattern: String,
        replacement: String,
        paths: Vec<PathBuf>,
        max_matches: usize,
    },
    StructuralSearchReplace {
        matcher: String,
        rewrite: String,
        paths: Vec<PathBuf>,
        max_matches: usize,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq, Hash)]
#[allow(dead_code)]
#[serde(rename_all = "camelCase")]
enum FormatRecipe {
    CargoFmt,
    DprintCheck,
    DprintFmt,
    SwiftFormat,
    PrettierCheck,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq, Hash)]
#[allow(dead_code)]
struct ActionCacheKey {
    // Conservative cache scaffold.  Same-Git-HEAD reuse is safe enough to start;
    // relevant-file hashing can arrive later without changing caller contracts.
    recipe_fingerprint: String,
    cwd: String,
    git_head: Option<String>,
    relevant_file_hashes: Vec<(PathBuf, String)>,
    env_fingerprint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
struct CachedActionResult {
    key: ActionCacheKey,
    result: CommandEvidence,
    created_at: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
enum AiEscalationReason {
    NoEligibleRustTask,
    AffectedWorkUnknown,
    PatchGenerationNeeded,
    VerificationFailed,
    AmbiguousFailure,
    HighRiskDiff,
    PolicyConflict,
    GuardViolation,
    PlannerNeeded,
    HumanJudgmentNeeded,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
struct AiEscalation {
    // Every model call should eventually have one of these.  It documents why
    // Rust could not proceed deterministically.
    role: String,
    task_id: Option<String>,
    reason: AiEscalationReason,
    evidence_refs: Vec<String>,
    compact_context: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
struct RepoIndex {
    // Lightweight repo-map seed.  It is intentionally regex-based for now; the
    // value is ranking small symbol facts, not building a full compiler model.
    files: Vec<FileEntry>,
    manifests: Vec<PathBuf>,
    test_files: Vec<PathBuf>,
    docs: Vec<PathBuf>,
    symbols: Vec<SymbolEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
struct FileEntry {
    path: PathBuf,
    language: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
struct SymbolEntry {
    path: PathBuf,
    language: String,
    kind: String,
    name: String,
    line: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
struct WorkerContextPack {
    task: TaskSpec,
    policy: TaskPolicy,
    probes: Vec<Value>,
    evidence_refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
struct ReviewContextPack {
    // Future normal-review packet.  The current compact pack is used for Senior
    // calls; this richer shape is here for role-specific context split.
    task: TaskSpec,
    diff_summary: DiffSummary,
    guard_report: GuardReport,
    findings: FilteredFindings,
    evidence_report: Option<EvidenceReport>,
    risk: RiskLevel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
struct PlannerContextPack {
    queue_state: Value,
    blocked_patterns: Vec<String>,
    strategic_constraints: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
struct StrategicContextPack {
    mission: String,
    roadmap_excerpt: String,
    queue_summary: Value,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq, Hash)]
#[allow(dead_code, clippy::enum_variant_names)]
#[serde(rename_all = "camelCase")]
enum DependencyRecipe {
    CargoDeny,
    CargoVet,
    CargoAudit,
    CargoMachete,
    CargoPublicApiDiff,
    CargoMsrvCheck,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
struct DependencyFinding {
    // Supply-chain policy should be evidence, not model judgment.  This shape is
    // ready for cargo-deny/audit/vet/public-api style findings.
    source: DependencyRecipe,
    severity: Severity,
    crate_name: Option<String>,
    manifest_path: Option<PathBuf>,
    message: String,
    requires_ai: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq, Hash)]
#[allow(dead_code)]
#[serde(rename_all = "camelCase")]
enum SnapshotPolicy {
    MustMatch,
    AllowChangedButRequireReview,
    AutoAcceptWithRedactions,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
struct SnapshotRecord {
    // Snapshot records let Rust compare generated artifacts semantically and
    // escalate only when a meaningful diff appears.
    snapshot_id: String,
    policy: SnapshotPolicy,
    semantic_hash: String,
    redacted_fields: Vec<String>,
    body: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
#[serde(tag = "source", rename_all = "camelCase")]
enum ReproSource {
    Proptest,
    CargoFuzz,
    UnitTest,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
#[serde(tag = "recipe", rename_all = "camelCase")]
enum RobustnessRecipe {
    ProptestReplay,
    ProptestFocused { test_filter: String },
    CargoFuzzSmoke { target: String, seconds: u64 },
    CargoFuzzReplayCorpus { target: String },
    CargoFuzzMinimizeCrash { target: String, artifact: PathBuf },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
struct ReproCase {
    source: ReproSource,
    test_name: String,
    seed: Option<String>,
    corpus_file: Option<PathBuf>,
    minimized_input: Option<PathBuf>,
    failure_summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
#[serde(tag = "recipe", rename_all = "camelCase")]
enum TestQualityRecipe {
    CoverageChangedFiles,
    CoverageThreshold {
        min_line: f64,
        min_branch: Option<f64>,
    },
    MutationSmokeChangedFiles,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
#[serde(tag = "recipe", rename_all = "camelCase")]
enum BudgetRecipe {
    CriterionCompare {
        bench: String,
        baseline_ref: String,
    },
    HyperfineCompare {
        command: RecipeSpec,
        baseline_ms: f64,
    },
    BinarySizeBudget {
        binary: String,
        max_bytes: u64,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq, Hash)]
#[allow(dead_code)]
#[serde(rename_all = "camelCase")]
enum BudgetStatus {
    Passed,
    Warn,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
struct BudgetFinding {
    metric: String,
    before: f64,
    after: f64,
    delta: f64,
    threshold: f64,
    status: BudgetStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
#[serde(tag = "profile", rename_all = "camelCase")]
enum SandboxProfile {
    NativeReadOnly,
    NativeWriteAllowed,
    Bubblewrap {
        network: bool,
        writable_paths: Vec<PathBuf>,
        readonly_paths: Vec<PathBuf>,
    },
    NixDevShell {
        flake_attr: Option<String>,
    },
    Container {
        image: String,
        network: bool,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq, Hash)]
#[allow(dead_code)]
#[serde(rename_all = "camelCase")]
enum WorkspaceVerificationMode {
    WorkingTree,
    StagedOnly,
    TempWorktreeFromIndex,
    TempWorktreeFromPatch,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
struct BatchCandidate {
    task_ids: Vec<String>,
    combined_patch_id: String,
    verification_plan: VerificationPlan,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
#[serde(tag = "strategy", rename_all = "camelCase")]
enum RegressionSearch {
    BetweenCommits {
        good: String,
        bad: String,
        recipe: RecipeSpec,
        max_steps: usize,
    },
    BetweenTaskPatches {
        candidates: Vec<String>,
        recipe: RecipeSpec,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code, clippy::enum_variant_names)]
#[serde(tag = "recipe", rename_all = "camelCase")]
enum FeatureRecipe {
    CargoHackEachFeature,
    CargoHackFeaturePowerset { max_depth: usize },
    CargoHackNoDefaultFeatures,
    CargoHackAllFeatures,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq, Hash)]
#[allow(dead_code)]
#[serde(rename_all = "camelCase")]
enum UiTestRecipe {
    Trybuild,
    Compiletest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
#[serde(tag = "recipe", rename_all = "camelCase")]
enum ReleaseRecipe {
    ChangelogPreview,
    VersionBumpCheck,
    SemverCheck,
    PublishDryRun,
    ReleasePrDraft,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
struct VersionChange {
    crate_name: String,
    from: Option<String>,
    to: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
struct ReleaseReadiness {
    changelog_diff: String,
    version_changes: Vec<VersionChange>,
    semver_findings: Vec<Finding>,
    publish_dry_run_ok: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
struct Annotation {
    severity: Severity,
    path: Option<PathBuf>,
    line: Option<u32>,
    column: Option<u32>,
    title: String,
    message: String,
    source: FindingSource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
struct AiTrajectory {
    task_id: Option<String>,
    role: String,
    escalation_reason: AiEscalationReason,
    prompt_hash: String,
    context_pack_hash: String,
    model_output: Value,
    parsed_output: Value,
    validation_errors: Vec<String>,
    resulting_events: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq, Hash)]
#[allow(dead_code)]
#[serde(tag = "key", rename_all = "camelCase")]
enum ComputeKey {
    RepoIndex,
    ChangedFiles,
    ParsedDiff,
    AffectedWork { task_id: String },
    VerificationPlan { task_id: String },
    EvidenceSummary { task_id: String },
    RiskClassification { task_id: String },
    WorkerContextPack { task_id: String },
    ReviewContextPack { task_id: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
struct ComputeNode {
    key: ComputeKey,
    dependencies: Vec<ComputeKey>,
    fingerprint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
struct FileDependencyGraph {
    imports: HashMap<PathBuf, Vec<PathBuf>>,
    reverse_imports: HashMap<PathBuf, Vec<PathBuf>>,
    tests_for_file: HashMap<PathBuf, Vec<PathBuf>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
struct RankedRepoMap {
    entries: Vec<RepoMapEntry>,
    token_budget: usize,
    ranking_reason: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
struct RepoMapEntry {
    path: PathBuf,
    symbols: Vec<SymbolSummary>,
    score: f64,
    why: Vec<RepoMapReason>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
struct SymbolSummary {
    kind: String,
    name: String,
    line: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[allow(dead_code)]
#[serde(rename_all = "camelCase")]
enum RepoMapReason {
    MentionsTaskTerm,
    ImportsChangedFile,
    ChangedRecently,
    CoAccessedWithAllowedFile,
    DefinesReferencedSymbol,
    TestForChangedModule,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
struct HookManifest {
    hooks: HashMap<String, HookDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
struct HookDefinition {
    recipes: Vec<RecipeSpec>,
    parallel: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DiffSummary {
    changed_files: Vec<String>,
    insertions: u64,
    deletions: u64,
    touches_source: bool,
    touches_tests: bool,
    touches_docs: bool,
    touches_tracking_docs: bool,
    touches_dependency_or_lockfile: bool,
    touches_generated: bool,
    touches_security_sensitive: bool,
}

#[derive(Debug, Clone)]
struct ReviewOutcome {
    project_complete: bool,
    commit_sha: Option<String>,
    summary: String,
    reason_code: String,
}

fn try_run_deterministic_task(
    paths: &Paths,
    cfg: &Config,
    state: &mut State,
    task: &TaskSpec,
    queue_path: &Path,
    queue_label: &str,
) -> Result<Option<WorkerOutput>> {
    if !cfg.deterministic_tasks || !is_deterministic_task_candidate(task) {
        return Ok(None);
    }

    let label = "Rust";
    println!(
        "[{label}] Running deterministic task {}: {}",
        task.task_id, task.title
    );

    let mut commands = Vec::new();
    for command in &task.tests_to_run {
        if command.trim().is_empty() {
            continue;
        }
        let Some(command) = parse_verification_command(command, "task.verification") else {
            println!(
                "[{label}] Deterministic lane skipped {}; command needs Worker judgment: {}",
                task.task_id,
                truncate_inline(command, 120)
            );
            return Ok(None);
        };
        let result = run_verification_command_with_policy(paths, cfg, &command)?;
        if result.category == DeterministicResultCategory::CommandFailed {
            println!(
                "[{label}] Deterministic lane stopped {}; `{}` exited {}",
                task.task_id,
                truncate_inline(&command.display(), 100),
                result.exit_code
            );
        }
        let should_stop = !result.category.is_success_like();
        commands.push(result);
        if should_stop {
            break;
        }
    }

    let final_category = commands
        .iter()
        .find(|result| !result.category.is_success_like())
        .map(|result| result.category);

    if final_category == Some(DeterministicResultCategory::CommandFailed) {
        let last = commands.last().expect("command failure requires a result");
        let failure_summary = rust_result_summary(task, last);
        let summary = format!(
            "{} deterministic command failed and remains unfinished. Rust support summary: {}",
            task.task_id, failure_summary.summary
        );
        let _ = record_rust_model_skip(
            paths,
            "worker",
            Some(&task.task_id),
            cfg,
            "deterministic task failed before Worker was needed",
        );
        let _ = record_rust_model_skip(
            paths,
            "foreman-review",
            Some(&task.task_id),
            cfg,
            "deterministic task remains unfinished; no Senior Dev review launched",
        );
        return Ok(Some(WorkerOutput {
            task_id: task.task_id.clone(),
            status: "blocked".to_string(),
            summary,
            files_changed: Vec::new(),
            commands_run: commands
                .iter()
                .map(|result| result.command.display())
                .collect(),
            tests_run: commands
                .iter()
                .map(|result| result.command.display())
                .collect(),
            handoff_to_rust: Vec::new(),
            orchestration_request: None,
            blocker: last.evidence.clone(),
            confidence: "medium".to_string(),
        }));
    }

    let status = if final_category == Some(DeterministicResultCategory::KnownEnvironmentBlocker) {
        "Blocked."
    } else {
        "Done."
    };
    let blocker = commands
        .iter()
        .find(|result| !result.category.is_success_like())
        .map(|result| result.evidence.clone())
        .unwrap_or_default();
    let queue_before = capture_rust_owned_file(paths, queue_path)?;
    mark_task_status_mechanically(queue_path, task, status, &commands)?;
    let commit_sha = commit_deterministic_task(paths, cfg, state, task, queue_path, &queue_before)?;
    let _ = append_event(
        paths,
        OrchestratorEventKind::StateTransitionApplied,
        Some(&task.task_id),
        json!({"queue": queue_label, "status": status, "mode": "deterministic"}),
    );
    let status_word = status.trim_end_matches('.');
    let summary = if commands.is_empty() {
        format!(
            "{} was marked {status_word} by the deterministic Rust lane; {queue_label} was updated and committed.",
            task.task_id,
        )
    } else {
        format!(
            "{} was marked {status_word} by the deterministic Rust lane after {} command(s); {queue_label} was updated and committed.",
            task.task_id,
            commands.len()
        )
    };
    println!(
        "[{label}] Task {} -> {}{}",
        task.task_id,
        status_word.to_ascii_lowercase(),
        commit_sha
            .as_ref()
            .map(|sha| format!("; commit {}", truncate_inline(sha, 12)))
            .unwrap_or_default()
    );
    let _ = record_rust_model_skip(
        paths,
        "worker",
        Some(&task.task_id),
        cfg,
        "deterministic task completed by Rust",
    );
    let _ = record_rust_model_skip(
        paths,
        "foreman-review",
        Some(&task.task_id),
        cfg,
        "deterministic task accepted by Rust",
    );
    if status_word.eq_ignore_ascii_case("Done") {
        let _ = record_positive_route_learning(
            paths,
            task,
            ExecutionLane::RustEvidence,
            "deterministic_done",
            &summary,
        );
    }

    Ok(Some(WorkerOutput {
        task_id: task.task_id.clone(),
        status: status_word.to_ascii_lowercase(),
        summary,
        files_changed: vec![queue_label.to_string()],
        commands_run: commands
            .iter()
            .map(|result| result.command.display())
            .collect(),
        tests_run: commands
            .iter()
            .map(|result| result.command.display())
            .collect(),
        handoff_to_rust: Vec::new(),
        orchestration_request: None,
        blocker,
        confidence: "high".to_string(),
    }))
}

fn run_task_verification_deterministically(
    paths: &Paths,
    cfg: &Config,
    task: &TaskSpec,
    worker: &WorkerOutput,
) -> Result<Value> {
    // Public entry point for "Rust verifies Worker."  The returned JSON keeps
    // existing prompt plumbing compatible, while the embedded report is typed.
    if !cfg.deterministic_tasks {
        return Ok(json!({
            "status": "skipped",
            "reason": "deterministic_tasks=false",
            "plan": null,
            "report": null,
            "commands": [],
            "rejected_commands": []
        }));
    }
    let plan = build_verification_plan_with_observation_hints(paths, cfg, task, worker)?;
    if plan.commands.is_empty() && plan.rejected_commands.is_empty() {
        return Ok(json!({
            "status": "skipped",
            "reason": "task and Worker handoff have no verification commands",
            "plan": plan,
            "report": null,
            "commands": [],
            "rejected_commands": []
        }));
    }

    println!(
        "[Rust] Verifying task {} with {} deterministic command(s)",
        task.task_id,
        plan.commands.len()
    );
    let report =
        execute_verification_plan_with_policy(paths, &plan, cfg.requires_verification_isolation())?;
    let evidence_path = persist_evidence_report(paths, &report)?;
    let verification_duration_ms: u128 = report
        .commands
        .iter()
        .map(|command| command.duration_ms)
        .sum();
    println!(
        "[Rust] Verification {} -> {} in {}",
        task.task_id,
        report.status,
        format_duration(Duration::from_millis(verification_duration_ms as u64))
    );
    append_event(
        paths,
        OrchestratorEventKind::EvidenceSummarized,
        Some(&task.task_id),
        json!({
            "report_id": report.report_id.clone(),
            "status": report.status.clone(),
            "evidence_path": display_path(paths, &evidence_path),
            "command_count": report.commands.len(),
        }),
    )?;
    Ok(json!({
        "status": report.status,
        "plan": plan,
        "report": report,
        "evidence_path": display_path(paths, &evidence_path),
        "commands": report.commands,
        "rejected_commands": report.rejected_commands,
    }))
}

fn build_verification_plan(task: &TaskSpec, worker: &WorkerOutput) -> VerificationPlan {
    // Merge sources in priority order: task card first, Worker handoff second.
    // This lets Planner specify required checks while Worker can add final
    // evidence commands discovered during implementation.
    let (commands, mut rejected_commands) = merged_verification_commands(task, worker);
    rejected_commands.sort();
    rejected_commands.dedup();
    let plan_fingerprint = commands
        .iter()
        .map(VerificationCommand::fingerprint)
        .collect::<Vec<_>>()
        .join("\n");
    VerificationPlan {
        plan_id: format!(
            "{}-{}",
            sanitize_filename(&task.task_id),
            &stable_hash_hex(&plan_fingerprint)[..12]
        ),
        task_id: task.task_id.clone(),
        parent_task: task.parent_task.clone(),
        source: "task.verification+worker.handoff_to_rust".to_string(),
        commands,
        rejected_commands,
    }
}

fn build_verification_plan_with_observation_hints(
    paths: &Paths,
    cfg: &Config,
    task: &TaskSpec,
    worker: &WorkerOutput,
) -> Result<VerificationPlan> {
    let mut plan = build_verification_plan(task, worker);
    if task_suppresses_observation_verification_hints(task, worker) {
        reject_disallowed_swiftstacker_report_roots(paths, task, &mut plan)?;
        return Ok(plan);
    }
    let hints = observation_hints_for_task(paths, cfg, task, worker);
    if hints.is_empty() {
        reject_disallowed_swiftstacker_report_roots(paths, task, &mut plan)?;
        return Ok(plan);
    }

    let mut seen = plan
        .commands
        .iter()
        .map(VerificationCommand::semantic_fingerprint)
        .collect::<HashSet<_>>();
    let known_failed_hint_semantics = known_failed_testing_command_fingerprints(paths);
    let stale_hint_semantics = stale_observation_hint_command_fingerprints(paths);
    let mut global_added = Vec::new();
    let mut local_added = Vec::new();
    let mut added_index = 0usize;
    for hint in hints {
        let Some(command) = parse_verification_command(&hint.command, hint.scope) else {
            continue;
        };
        if !is_deterministic_command_structured_allowed(&command) {
            continue;
        }
        let semantic = command.semantic_fingerprint();
        if known_failed_hint_semantics.contains(&semantic) {
            plan.rejected_commands.push(format!(
                "{} (known failing in TESTING.md; observation hint skipped)",
                command.display()
            ));
            continue;
        }
        if stale_hint_semantics.contains(&semantic) {
            plan.rejected_commands.push(format!(
                "{} (recently failed as observation hint; skipped)",
                command.display()
            ));
            continue;
        }
        if !seen.insert(semantic) {
            continue;
        }
        let source_tag = if hint.scope == "global_memory.verification_hint" {
            "global-memory"
        } else {
            "local-memory"
        };
        let command = with_stable_command_metadata(&task.task_id, source_tag, added_index, command);
        added_index += 1;
        if hint.scope == "global_memory.verification_hint" {
            global_added.push(json!({
                "file": hint.file,
                "command": command.display(),
            }));
        } else {
            local_added.push(json!({
                "file": hint.file,
                "command": command.display(),
            }));
        }
        plan.commands.push(command);
    }
    if !global_added.is_empty() || !local_added.is_empty() {
        let plan_fingerprint = plan
            .commands
            .iter()
            .map(VerificationCommand::fingerprint)
            .collect::<Vec<_>>()
            .join("\n");
        plan.plan_id = format!(
            "{}-{}",
            sanitize_filename(&task.task_id),
            &stable_hash_hex(&plan_fingerprint)[..12]
        );
        plan.source = "task.verification+worker.handoff_to_rust+observation_hints".to_string();
        let _ = append_event(
            paths,
            OrchestratorEventKind::GlobalMemoryApplied,
            Some(&task.task_id),
            json!({
                "mode": "verification_plan_hints",
                "local_hints_added": local_added,
                "global_hints_added": global_added,
            }),
        );
    }
    reject_disallowed_swiftstacker_report_roots(paths, task, &mut plan)?;
    Ok(plan)
}

fn task_suppresses_observation_verification_hints(task: &TaskSpec, worker: &WorkerOutput) -> bool {
    let editable_paths = task
        .allowed_files
        .iter()
        .chain(worker.files_changed.iter())
        .filter(|path| !path.trim().is_empty())
        .collect::<Vec<_>>();
    !editable_paths.is_empty()
        && editable_paths
            .iter()
            .all(|path| is_docs_or_tracking_doc_pattern(path))
}

fn is_docs_or_tracking_doc_pattern(path: &str) -> bool {
    let trimmed = path.trim().trim_matches('`').trim_start_matches("./");
    let lower = trimmed.to_ascii_lowercase();
    trimmed == "tracking docs"
        || is_tracking_doc_path(trimmed)
        || lower.starts_with("docs/")
        || lower.starts_with(".agent/")
        || lower.ends_with(".md")
}

fn known_failed_testing_command_fingerprints(paths: &Paths) -> HashSet<String> {
    let testing_path = paths.root.join("TESTING.md");
    let Ok(text) = read_text(&testing_path) else {
        return HashSet::new();
    };
    testing_known_failed_commands(&text)
        .into_iter()
        .filter_map(|command| {
            parse_verification_command(&command, "testing.known_failed")
                .map(|command| command.semantic_fingerprint())
        })
        .collect()
}

fn stale_observation_hint_command_fingerprints(paths: &Paths) -> HashSet<String> {
    let mut fingerprints = suppressed_verification_hint_fingerprints(paths);
    fingerprints.extend(
        failed_observation_hint_commands(paths)
            .into_iter()
            .map(|command| command.semantic_fingerprint),
    );
    fingerprints
}

fn suppressed_verification_hint_fingerprints(paths: &Paths) -> HashSet<String> {
    read_suppressed_verification_hints(paths)
        .into_iter()
        .flat_map(|suppressed| suppressed.hints)
        .map(|hint| hint.semantic_fingerprint)
        .collect()
}

fn read_suppressed_verification_hints(paths: &Paths) -> Option<SuppressedVerificationHints> {
    read_json_file::<SuppressedVerificationHints>(
        &paths
            .observations
            .join("suppressed_verification_hints.json"),
    )
    .ok()
}

fn failed_observation_hint_commands(paths: &Paths) -> Vec<StaleObservationHintCommand> {
    if !paths.evidence.exists() {
        return Vec::new();
    }
    let Ok(entries) = fs::read_dir(&paths.evidence) else {
        return Vec::new();
    };
    let mut by_key = BTreeMap::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
            continue;
        }
        collect_failed_observation_hint_commands_from_file(&path, &mut by_key);
    }
    let mut commands = by_key.into_values().collect::<Vec<_>>();
    commands.sort_by(|left, right| {
        right
            .completed_at
            .cmp(&left.completed_at)
            .then_with(|| left.task_id.cmp(&right.task_id))
            .then_with(|| left.command_display.cmp(&right.command_display))
    });
    commands
}

fn collect_failed_observation_hint_commands_from_file(
    path: &Path,
    by_key: &mut BTreeMap<String, StaleObservationHintCommand>,
) {
    let Ok(file) = File::open(path) else {
        return;
    };
    for line in BufReader::new(file)
        .lines()
        .map_while(std::result::Result::ok)
    {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(report) = serde_json::from_str::<EvidenceReport>(trimmed) {
            for command in report.commands {
                record_failed_observation_hint_command(
                    by_key,
                    &report.task_id,
                    &report.report_id,
                    &report.completed_at,
                    command,
                );
            }
            continue;
        }
        if let Ok(event) = serde_json::from_str::<EvidenceCommandEvent>(trimmed) {
            record_failed_observation_hint_command(
                by_key,
                &event.task_id,
                &event.report_id,
                &event.completed_at,
                event.command,
            );
        }
    }
}

fn record_failed_observation_hint_command(
    by_key: &mut BTreeMap<String, StaleObservationHintCommand>,
    task_id: &str,
    _report_id: &str,
    completed_at: &str,
    command: CommandEvidence,
) {
    if command.success || !is_observation_hint_source(&command.command.source) {
        return;
    }
    let semantic_fingerprint = command.command.semantic_fingerprint();
    let key = semantic_fingerprint.clone();
    let should_replace = by_key
        .get(&key)
        .map(|existing| completed_at > existing.completed_at.as_str())
        .unwrap_or(true);
    if should_replace {
        by_key.insert(
            key,
            StaleObservationHintCommand {
                task_id: task_id.to_string(),
                command_display: command.command_display.clone(),
                category: command.category.clone(),
                source: command.command.source.clone(),
                completed_at: completed_at.to_string(),
                semantic_fingerprint,
            },
        );
    }
}

fn is_observation_hint_source(source: &str) -> bool {
    matches!(
        source,
        "local_memory.verification_hint" | "global_memory.verification_hint"
    )
}

fn testing_known_failed_commands(text: &str) -> BTreeSet<String> {
    let mut commands = BTreeSet::new();
    for line in text.lines() {
        let cells = markdown_table_cells(line);
        if cells.len() < 2 {
            continue;
        }
        let command = testing_command_cell_value(&cells[0]);
        if command.is_empty() || command.eq_ignore_ascii_case("command") {
            continue;
        }
        if testing_status_marks_known_failure(&cells[1]) {
            commands.insert(command);
        }
    }
    commands
}

fn markdown_table_cells(line: &str) -> Vec<String> {
    let trimmed = line.trim();
    if !trimmed.starts_with('|') {
        return Vec::new();
    }
    trimmed
        .trim_matches('|')
        .split('|')
        .map(|cell| cell.trim().to_string())
        .collect()
}

fn testing_command_cell_value(cell: &str) -> String {
    let trimmed = cell.trim();
    if let Some(start) = trimmed.find('`') {
        let after_start = &trimmed[start + 1..];
        if let Some(end) = after_start.find('`') {
            return after_start[..end].trim().to_string();
        }
    }
    trimmed.trim_matches('`').trim().to_string()
}

fn testing_status_marks_known_failure(status: &str) -> bool {
    let lower = status.to_ascii_lowercase();
    lower.contains("fail")
        || lower.contains("blocked")
        || lower.contains("broken")
        || lower.contains("known blocker")
        || lower.contains("known failure")
}

pub(crate) fn reject_disallowed_swiftstacker_report_roots(
    paths: &Paths,
    task: &TaskSpec,
    plan: &mut VerificationPlan,
) -> Result<()> {
    let allowed_patterns = expand_allowed_file_patterns(&task.allowed_files);
    let allowed = compile_globs(&allowed_patterns)?;
    let mut retained = Vec::new();
    for command in plan.commands.drain(..) {
        if let Some(report_root) = swiftstacker_report_root(&command) {
            let report_path = paths.root.join(report_root);
            if report_path.exists() && !task_allows_report_root_overwrite(&allowed, report_root) {
                plan.rejected_commands.push(format!(
                    "{} (existing report root `{report_root}` is not explicitly allowed)",
                    command.display()
                ));
                continue;
            }
        }
        retained.push(command);
    }
    plan.commands = retained;
    plan.rejected_commands.sort();
    plan.rejected_commands.dedup();
    Ok(())
}

fn swiftstacker_report_root(command: &VerificationCommand) -> Option<&str> {
    if command.program != "swift" {
        return None;
    }
    let args = command.args.as_slice();
    if args.len() < 8
        || args.first().map(String::as_str) != Some("run")
        || args.get(1).map(String::as_str) != Some("-c")
        || args.get(2).map(String::as_str) != Some("release")
        || args.get(3).map(String::as_str) != Some("SwiftStackerBench")
    {
        return None;
    }
    args.windows(2)
        .find(|window| window[0] == "--report-root")
        .map(|window| window[1].as_str())
}

fn task_allows_report_root_overwrite(allowed: &GlobSet, report_root: &str) -> bool {
    allowed.is_match(report_root)
        || allowed.is_match(format!("{report_root}/").as_str())
        || allowed.is_match(format!("{report_root}/benchmark-report.json").as_str())
        || allowed.is_match(format!("{report_root}/suite-summary.json").as_str())
}

fn observation_hints_for_task(
    paths: &Paths,
    cfg: &Config,
    task: &TaskSpec,
    worker: &WorkerOutput,
) -> Vec<ObservationHint> {
    let files = observation_hint_candidate_files(task, worker);
    if files.is_empty() {
        return Vec::new();
    }
    let mut hints = Vec::new();
    hints.extend(load_verification_hints_for_files(
        &paths.observations,
        "local_memory.verification_hint",
        &files,
    ));
    hints.extend(load_verification_hints_for_files(
        &global_observations_dir(cfg),
        "global_memory.verification_hint",
        &files,
    ));
    hints
}

fn observation_hint_candidate_files(task: &TaskSpec, worker: &WorkerOutput) -> Vec<String> {
    let mut files = BTreeSet::new();
    for file in task
        .allowed_files
        .iter()
        .chain(task.read_only_files.iter())
        .chain(worker.files_changed.iter())
    {
        let file = normalize_observation_hint_file(file);
        if file.is_empty()
            || file.contains('*')
            || file.contains('{')
            || file.contains('[')
            || observed_file_category(&file) == "tracking_doc"
        {
            continue;
        }
        files.insert(file);
    }
    files.into_iter().collect()
}

fn normalize_observation_hint_file(file: &str) -> String {
    file.trim().trim_start_matches("./").to_string()
}

fn load_verification_hints_for_files(
    observations_dir: &Path,
    scope: &'static str,
    files: &[String],
) -> Vec<ObservationHint> {
    let index =
        read_json_file::<VerificationHintIndex>(&observations_dir.join("verification_hints.json"))
            .unwrap_or_default();
    let mut hints = Vec::new();
    for file in files {
        let Some(commands) = index.hints_by_file.get(file) else {
            continue;
        };
        for command in commands {
            hints.push(ObservationHint {
                scope,
                file: file.clone(),
                command: command.clone(),
            });
        }
    }
    hints
}

fn merged_verification_commands(
    task: &TaskSpec,
    worker: &WorkerOutput,
) -> (Vec<VerificationCommand>, Vec<String>) {
    // Two dedupe layers:
    // - full fingerprint avoids duplicate IDs/context
    // - semantic fingerprint prevents task+worker from running the same check twice
    let mut seen = HashSet::new();
    let mut seen_semantic = HashSet::new();
    let mut commands = Vec::new();
    let mut rejected = Vec::new();
    for (index, command) in task.tests_to_run.iter().enumerate() {
        let trimmed = command.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(command) = parse_verification_command(trimmed, "task.verification") {
            let command = with_stable_command_metadata(&task.task_id, "task", index, command);
            if seen.insert(command.fingerprint()) {
                seen_semantic.insert(command.semantic_fingerprint());
                commands.push(command);
            }
        } else {
            rejected.push(trimmed.to_string());
        }
    }
    for (index, command) in worker.handoff_to_rust.iter().enumerate() {
        let command = command
            .clone()
            .with_source(if command.source.trim().is_empty() {
                "worker.handoff_to_rust"
            } else {
                &command.source
            });
        let command = normalize_worker_handoff_command(command);
        if seen_semantic.contains(&command.semantic_fingerprint()) {
            continue;
        }
        if is_deterministic_command_structured_allowed(&command) {
            let command = with_stable_command_metadata(&task.task_id, "worker", index, command);
            if seen.insert(command.fingerprint()) {
                seen_semantic.insert(command.semantic_fingerprint());
                commands.push(command);
            }
        } else {
            rejected.push(command.display());
        }
    }
    (commands, rejected)
}

fn normalize_worker_handoff_command(mut command: VerificationCommand) -> VerificationCommand {
    // Codex often reports Rust-owned checks with the absolute repo cwd it used
    // internally.  Verification always executes from `paths.root`, so collapse
    // absolute/"." cwd metadata back to repo-root instead of rejecting an
    // otherwise safe command or accidentally running outside the repository.
    if let Some(cwd) = command.cwd.as_deref().map(str::trim) {
        if cwd.is_empty() || cwd == "." || Path::new(cwd).is_absolute() {
            command.cwd = None;
        }
    }
    command
}

pub(crate) fn with_stable_command_metadata(
    task_id: &str,
    source_tag: &str,
    index: usize,
    command: VerificationCommand,
) -> VerificationCommand {
    let kind = infer_command_kind(&command);
    let id = if command.id.trim().is_empty() {
        format!(
            "{}:{}:{}:{}",
            sanitize_filename(task_id),
            source_tag,
            index + 1,
            &stable_hash_hex(&command.fingerprint())[..8]
        )
    } else {
        command.id.clone()
    };
    command.with_id_and_metadata(id, kind)
}

fn infer_command_kind(command: &VerificationCommand) -> CommandKind {
    match command.program.as_str() {
        "git" => CommandKind::Git,
        "rg" => CommandKind::Search,
        "swift" | "cargo" => match command.args.first().map(String::as_str) {
            Some("test") => CommandKind::Test,
            Some("build" | "check") => CommandKind::Build,
            _ => CommandKind::Unknown,
        },
        "npm" | "pnpm" | "yarn" | "pytest" | "python" => CommandKind::Test,
        _ if is_local_debug_smoke_command_structured_allowed(command) => CommandKind::Smoke,
        _ => CommandKind::Unknown,
    }
}

#[cfg(test)]
pub(crate) fn execute_verification_plan(
    paths: &Paths,
    plan: &VerificationPlan,
) -> Result<EvidenceReport> {
    execute_verification_plan_with_policy(paths, plan, false)
}

fn run_verification_command_with_policy(
    paths: &Paths,
    cfg: &Config,
    command: &VerificationCommand,
) -> Result<DeterministicCommandResult> {
    if cfg.requires_verification_isolation() {
        run_isolated_deterministic_command(paths, command)
    } else {
        run_deterministic_command(paths, command)
    }
}

pub(crate) fn execute_verification_plan_with_policy(
    paths: &Paths,
    plan: &VerificationPlan,
    require_isolation: bool,
) -> Result<EvidenceReport> {
    // This is the CI runner core.  It records events before/after each command,
    // persists a run record, honors failure policy, and returns one compact report.
    let started_at = Utc::now();
    let mut evidence = Vec::new();
    let isolation = require_isolation
        .then(|| IsolationSession::new(paths))
        .transpose()?;
    let _ = append_event(
        paths,
        OrchestratorEventKind::RecipePlanned,
        Some(&plan.task_id),
        json!({
            "plan_id": plan.plan_id,
            "command_count": plan.commands.len(),
            "rejected_commands": plan.rejected_commands,
        }),
    );
    for command in &plan.commands {
        let run_id = format!(
            "{}-{}-{}",
            sanitize_filename(&plan.task_id),
            sanitize_filename(&command.id),
            Utc::now().timestamp_millis().unsigned_abs()
        );
        let _ = append_event(
            paths,
            OrchestratorEventKind::CacheMiss,
            Some(&plan.task_id),
            json!({"command_id": command.id.clone(), "policy": "sameGitHead"}),
        );
        let run_started = Utc::now();
        let _ = persist_run_record(
            paths,
            &RunRecord {
                run_id: run_id.clone(),
                task_id: Some(plan.task_id.clone()),
                recipe_id: command.id.clone(),
                state: RunState::Running,
                started_at: Some(run_started.to_rfc3339()),
                finished_at: None,
                duration_ms: None,
                result_ref: None,
            },
        );
        let _ = append_event(
            paths,
            OrchestratorEventKind::RecipeStarted,
            Some(&plan.task_id),
            json!({"run_id": run_id, "command_id": command.id.clone(), "command": command.display()}),
        );
        let result = if let Some(isolation) = isolation.as_ref() {
            run_isolated_deterministic_command_in_session(paths, command, isolation)?
        } else {
            run_deterministic_command(paths, command)?
        };
        let should_stop =
            !result.category.is_success_like() && command.failure_policy == FailurePolicy::Stop;
        let state = match result.category {
            DeterministicResultCategory::Passed | DeterministicResultCategory::NoMatches => {
                RunState::Passed
            }
            DeterministicResultCategory::TimedOut => RunState::TimedOut,
            DeterministicResultCategory::KnownEnvironmentBlocker
            | DeterministicResultCategory::CommandFailed => RunState::Failed,
        };
        let result_ref = format!("evidence/{}.jsonl", sanitize_filename(&plan.task_id));
        let _ = persist_run_record(
            paths,
            &RunRecord {
                run_id: run_id.clone(),
                task_id: Some(plan.task_id.clone()),
                recipe_id: command.id.clone(),
                state,
                started_at: Some(run_started.to_rfc3339()),
                finished_at: Some(Utc::now().to_rfc3339()),
                duration_ms: Some(result.duration_ms),
                result_ref: Some(result_ref),
            },
        );
        let _ = append_event(
            paths,
            OrchestratorEventKind::RecipeFinished,
            Some(&plan.task_id),
            json!({
                "run_id": run_id,
                "command_id": command.id.clone(),
                "category": result.category.as_str(),
                "duration_ms": result.duration_ms,
                "exit_code": result.exit_code,
            }),
        );
        evidence.push(command_evidence_from_result(result));
        if should_stop {
            break;
        }
    }

    let mut status = evidence
        .iter()
        .find(|result| result.category != "passed" && result.category != "noMatches")
        .map(|result| result.category.as_str())
        .unwrap_or("passed")
        .to_string();
    if !plan.rejected_commands.is_empty() {
        status = if evidence.is_empty() {
            "skipped".to_string()
        } else {
            "rejectedCommands".to_string()
        };
    }
    let completed_at = Utc::now();
    let summary = if evidence.is_empty() {
        format!(
            "{} verification produced no executable command evidence.",
            plan.task_id
        )
    } else {
        format!(
            "{} verification `{}` after {} command(s).",
            plan.task_id,
            status,
            evidence.len()
        )
    };
    Ok(EvidenceReport {
        report_id: format!(
            "{}-{}",
            plan.plan_id,
            completed_at.timestamp_millis().unsigned_abs()
        ),
        plan_id: plan.plan_id.clone(),
        task_id: plan.task_id.clone(),
        status,
        started_at: started_at.to_rfc3339(),
        completed_at: completed_at.to_rfc3339(),
        summary,
        commands: evidence,
        rejected_commands: plan.rejected_commands.clone(),
    })
}

fn command_evidence_from_result(result: DeterministicCommandResult) -> CommandEvidence {
    let command_display = result.command.display();
    CommandEvidence {
        id: result.command.id.clone(),
        command: result.command,
        command_display,
        exit_code: result.exit_code,
        success: result.category.is_success_like(),
        category: result.category.as_str().to_string(),
        duration_ms: result.duration_ms,
        stdout_tail: result.stdout_tail,
        stderr_tail: result.stderr_tail,
        evidence_summary: result.evidence,
    }
}

pub(crate) fn persist_evidence_report(paths: &Paths, report: &EvidenceReport) -> Result<PathBuf> {
    // Evidence is written twice:
    // - date JSONL for cross-task timelines
    // - task JSONL for focused task replay/debugging
    fs::create_dir_all(&paths.evidence)?;
    let date_path = paths
        .evidence
        .join(format!("{}.jsonl", Utc::now().format("%Y%m%d")));
    let mut date_file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&date_path)
        .with_context(|| format!("open evidence report {}", date_path.display()))?;
    writeln!(date_file, "{}", serde_json::to_string(report)?)?;

    let task_path = paths
        .evidence
        .join(format!("{}.jsonl", sanitize_filename(&report.task_id)));
    let mut task_file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&task_path)
        .with_context(|| format!("open task evidence {}", task_path.display()))?;
    if report.commands.is_empty() {
        writeln!(
            task_file,
            "{}",
            serde_json::to_string(&json!({
                "event": "report",
                "report_id": report.report_id,
                "plan_id": report.plan_id,
                "task_id": report.task_id,
                "status": report.status,
                "summary": report.summary,
                "started_at": report.started_at,
                "completed_at": report.completed_at,
                "rejected_commands": report.rejected_commands,
            }))?
        )?;
    } else {
        for command in &report.commands {
            writeln!(
                task_file,
                "{}",
                serde_json::to_string(&json!({
                    "event": "command",
                    "report_id": report.report_id,
                    "plan_id": report.plan_id,
                    "task_id": report.task_id,
                    "status": report.status,
                    "started_at": report.started_at,
                    "completed_at": report.completed_at,
                    "command": command,
                    "rejected_commands": report.rejected_commands,
                }))?
            )?;
        }
    }
    Ok(task_path)
}

fn evidence_report_from_value(value: &Value) -> Option<EvidenceReport> {
    value
        .get("report")
        .and_then(|report| serde_json::from_value::<EvidenceReport>(report.clone()).ok())
}

fn evidence_report_is_clean(report: &EvidenceReport) -> bool {
    report.status == "passed"
        && report.rejected_commands.is_empty()
        && report.commands.iter().all(|command| command.success)
}

fn effective_review_tier(task: &TaskSpec) -> ReviewTier {
    if is_deterministic_task_candidate(task) {
        ReviewTier::RustOnly
    } else {
        task.review_tier
    }
}

fn effective_automation_level(task: &TaskSpec) -> AutomationLevel {
    if is_deterministic_task_candidate(task) {
        AutomationLevel::RustOnly
    } else {
        task.automation_level
    }
}

fn inferred_machine_checkable_auto_accept_candidate(task: &TaskSpec) -> bool {
    if task.requires_network || task.tests_to_run.is_empty() {
        return false;
    }
    let has_source = task.allowed_files.iter().any(|path| {
        let lower = path.to_ascii_lowercase();
        lower.starts_with("sources/") || lower.starts_with("src/")
    });
    let has_tests = task.allowed_files.iter().any(|path| {
        let lower = path.to_ascii_lowercase();
        lower.starts_with("tests/") || lower.starts_with("test/")
    });
    let has_diff_check = task
        .tests_to_run
        .iter()
        .any(|command| command.trim() == "git diff --check");
    if !has_source || !has_tests || !has_diff_check {
        return false;
    }
    let task_text = format!(
        "{}\n{}\n{}\n{}",
        task.kind,
        task.title,
        task.objective,
        task.forbidden_actions.join("\n")
    )
    .to_ascii_lowercase();
    let local_or_closed_scope = task_text.contains("local")
        || task_text.contains("no public")
        || task_text.contains("without public")
        || task_text.contains("disabled-by-default");
    let safety_metadata_shape = task_text.contains("disabled")
        || task_text.contains("refus")
        || task_text.contains("no public")
        || task_text.contains("sanitized")
        || task_text.contains("bounded")
        || task_text.contains("fail-closed")
        || task_text.contains("controlled")
        || task_text.contains("loopback")
        || task_text.contains("handoff");
    if !local_or_closed_scope || !safety_metadata_shape {
        return false;
    }
    true
}

fn inferred_docs_only_auto_accept_candidate(task: &TaskSpec) -> bool {
    if task.requires_network || task.tests_to_run.is_empty() {
        return false;
    }
    let all_allowed_docs = task.allowed_files.iter().all(|path| {
        let lower = path.to_ascii_lowercase();
        lower.starts_with("docs/")
            || lower.ends_with(".md")
            || lower == "tracking docs"
            || is_tracking_doc_path(path)
    });
    if !all_allowed_docs {
        return false;
    }
    task.tests_to_run.iter().all(|command| {
        let trimmed = command.trim();
        trimmed == "git diff --check" || trimmed.starts_with("rg ")
    })
}

fn maybe_auto_accept_worker_task(
    paths: &Paths,
    cfg: &Config,
    state: &mut State,
    task: &TaskSpec,
    worker: &WorkerOutput,
    guard_report: &GuardReport,
    evidence_report: &EvidenceReport,
) -> Result<Option<ReviewOutcome>> {
    // Conservative by design.  Returning None is cheap; accepting a risky task is
    // expensive.  Auto-accept requires explicit/legacy eligibility, done Worker,
    // clean guard, clean evidence, and low risk.
    let review_tier = effective_review_tier(task);
    let automation_level = effective_automation_level(task);
    let legacy_machine_checkable = inferred_machine_checkable_auto_accept_candidate(task);
    let legacy_docs_only = inferred_docs_only_auto_accept_candidate(task);
    if (!matches!(
        automation_level,
        AutomationLevel::RustOnly | AutomationLevel::AiWorkerRustReview
    ) || !matches!(
        review_tier,
        ReviewTier::RustOnly | ReviewTier::AutoAcceptIfVerified | ReviewTier::AiReviewOnFailure
    )) && !legacy_docs_only
        && !legacy_machine_checkable
    {
        return Ok(None);
    }
    if worker.status != "done" || !guard_report.violations.is_empty() {
        return Ok(None);
    }
    if let Some(batch) = state
        .senior_batch_approval
        .as_ref()
        .filter(|batch| batch.task_ids.contains(&task.task_id))
    {
        if batch.auto_accept_policy == "always_senior_review" {
            return Ok(None);
        }
    }
    if !evidence_report_is_clean(evidence_report) {
        return Ok(None);
    }
    let diff_summary = build_diff_summary(paths, &guard_report.changed_files)?;
    let risk = classify_risk(task, &diff_summary);
    if risk != RiskLevel::Low {
        return Ok(None);
    }

    let summary = format!(
        "Rust auto-accepted {}: low-risk diff, guard clean, and verification evidence passed.",
        task.task_id
    );
    let commit_sha = rust_accept_worker_task(
        paths,
        cfg,
        state,
        WorkerAcceptance {
            task,
            guard_report,
            evidence_report,
            reason_code: "rust_auto_accept_verified",
            commit_message: &format!("Complete {} with Rust auto-accept", task.task_id),
        },
    )?;
    let _ = record_rust_model_skip(
        paths,
        "foreman-review",
        Some(&task.task_id),
        cfg,
        "low-risk verified Worker task auto-accepted by Rust",
    );
    let _ = append_event(
        paths,
        OrchestratorEventKind::AutoAccepted,
        Some(&task.task_id),
        json!({"reason": "rust_auto_accept_verified", "commit_sha": commit_sha.clone()}),
    );
    let _ = record_positive_route_learning(
        paths,
        task,
        ExecutionLane::SharedWorker,
        "worker_verified_auto_accept",
        &summary,
    );
    Ok(Some(ReviewOutcome {
        project_complete: false,
        commit_sha,
        summary,
        reason_code: "rust_auto_accept_verified".to_string(),
    }))
}

struct WorkerAcceptance<'a> {
    task: &'a TaskSpec,
    guard_report: &'a GuardReport,
    evidence_report: &'a EvidenceReport,
    reason_code: &'a str,
    commit_message: &'a str,
}

fn rust_accept_worker_task(
    paths: &Paths,
    cfg: &Config,
    state: &mut State,
    acceptance: WorkerAcceptance<'_>,
) -> Result<Option<String>> {
    // Transactional accept: prepare queue/tracking notes, commit the verified
    // tree, and roll queue docs back if Git commit fails.
    let task = acceptance.task;
    let evidence_report = acceptance.evidence_report;
    let reason_code = acceptance.reason_code;
    let imported = state.imported_tasks.contains_key(&task.task_id);
    let tracking_candidates = [
        "CONTINUITY.md",
        "TASK.md",
        "docs/ROADMAP.md",
        "Docs/ROADMAP.md",
        "Docs/.INDEX.md",
    ];
    let mut rust_owned_snapshots = Vec::new();
    if !imported {
        rust_owned_snapshots.push(capture_rust_owned_file(paths, &paths.next_steps)?);
        let allowed = compile_globs(&expand_allowed_file_patterns(&task.allowed_files))?;
        for rel in tracking_candidates {
            let path = paths.root.join(rel);
            if allowed.is_match(rel) && path.exists() {
                rust_owned_snapshots.push(capture_rust_owned_file(paths, &path)?);
            }
        }
    }

    let mutation_result = if imported {
        // Imported tasks are intentionally not projected back into ca's (or
        // any target repository's) Markdown control documents.  Their durable
        // completion record lives in orchestrator state, events, and evidence.
        Ok(())
    } else {
        (|| -> Result<()> {
            mark_task_done_with_evidence(&paths.next_steps, task, evidence_report, reason_code)?;
            append_tracking_doc_completion_notes(paths, task, evidence_report, reason_code)?;
            Ok(())
        })()
    };
    if let Err(err) = mutation_result {
        restore_rust_owned_files(paths, &rust_owned_snapshots)
            .context("restore queue/tracking docs after bookkeeping failure")?;
        return Err(err).context("prepare accepted-task queue/tracking docs");
    }
    let mut changed_snapshots = Vec::new();
    for snapshot in &rust_owned_snapshots {
        match rust_owned_file_changed(paths, snapshot) {
            Ok(true) => changed_snapshots.push(snapshot.clone()),
            Ok(false) => {}
            Err(err) => {
                restore_rust_owned_files(paths, &rust_owned_snapshots)
                    .context("restore queue/tracking docs after fingerprint failure")?;
                return Err(err).context("fingerprint accepted-task queue/tracking docs");
            }
        }
    }
    match commit_accepted_worker_task(
        paths,
        cfg,
        state,
        AcceptedWorkerCommitRequest {
            task,
            guard_report: acceptance.guard_report,
            message: acceptance.commit_message,
            rust_owned_file_snapshots: &changed_snapshots,
            verification_evidence_id: &evidence_report.report_id,
        },
    ) {
        Ok(commit_sha) => Ok(commit_sha),
        Err(err) => {
            restore_rust_owned_files(paths, &rust_owned_snapshots)
                .context("restore queue/tracking docs after accepted-task commit failure")?;
            Err(err).context("accepted-task commit failed; restored queue/tracking docs")
        }
    }
}

fn append_tracking_doc_completion_notes(
    paths: &Paths,
    task: &TaskSpec,
    report: &EvidenceReport,
    reason_code: &str,
) -> Result<Vec<String>> {
    // v1 tracking-doc patch lane.  Only files already allowed by the task get
    // touched, which keeps Rust bookkeeping inside the same guard boundary.
    let candidates = [
        "CONTINUITY.md",
        "TASK.md",
        "docs/ROADMAP.md",
        "Docs/ROADMAP.md",
        "Docs/.INDEX.md",
    ];
    let allowed = compile_globs(&expand_allowed_file_patterns(&task.allowed_files))?;
    let mut changed = Vec::new();
    for rel in candidates {
        if !allowed.is_match(rel) {
            continue;
        }
        let path = paths.root.join(rel);
        if !path.exists() {
            continue;
        }
        let marker = format!("Rust completion note for `{}`", task.task_id);
        let mut text = read_text(&path)?;
        if text.contains(&marker) {
            continue;
        }
        if !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(&format!(
            "\n- {marker}: `{reason_code}` with evidence `{}` (`{}`).\n",
            report.report_id, report.status
        ));
        write_text(&path, &text)?;
        changed.push(rel.to_string());
    }
    Ok(changed)
}

// ---------------------------------------------------------------------------
// Accept / Done bookkeeping and verified-tree commits
// ---------------------------------------------------------------------------
//
// Queue Done notes and git commits are Rust-owned. Accepted Worker commits stage
// guard-changed files plus queue/tracking docs only (no silent dirty absorption).

fn mark_task_done_with_evidence(
    queue_path: &Path,
    task: &TaskSpec,
    report: &EvidenceReport,
    reason_code: &str,
) -> Result<()> {
    // Markdown is still human-facing state, so update it mechanically and include
    // enough evidence to audit the transition without reopening raw logs.
    let text = read_text_or_empty(queue_path);
    let mut lines = text.lines().map(str::to_string).collect::<Vec<_>>();
    let Some((start, end)) = next_steps_task_section_bounds(&lines, &task.task_id) else {
        return append_missing_worker_task_record(queue_path, task, report, reason_code);
    };
    let notes = rust_review_notes(report, reason_code);
    replace_task_status_in_section(&mut lines, start, end, "- Status: Done.");

    let insert_at = next_steps_task_section_bounds(&lines, &task.task_id)
        .map(|(_, section_end)| section_end.min(lines.len()))
        .unwrap_or_else(|| end.min(lines.len()));
    lines.splice(insert_at..insert_at, notes);
    let mut updated = lines.join("\n");
    updated.push('\n');
    write_text(queue_path, &updated)
}

fn rust_review_notes(report: &EvidenceReport, reason_code: &str) -> Vec<String> {
    let mut notes = vec![
        "- Rust review notes:".to_string(),
        format!("  - Decision: `{reason_code}`."),
        format!(
            "  - Evidence report: `{}` (`{}`).",
            report.report_id, report.status
        ),
    ];
    if report.commands.is_empty() {
        notes.push("  - No deterministic commands were executed.".to_string());
    } else {
        notes.push("  - Verification evidence:".to_string());
        for command in &report.commands {
            notes.push(format!(
                "    - `{}` exited {} (`{}`) in {}ms: {}.",
                command.command_display,
                command.exit_code,
                command.category,
                command.duration_ms,
                command.evidence_summary
            ));
        }
    }
    notes
}

fn append_missing_worker_task_record(
    queue_path: &Path,
    task: &TaskSpec,
    report: &EvidenceReport,
    reason_code: &str,
) -> Result<()> {
    let mut text = read_text_or_empty(queue_path);
    if !text.ends_with('\n') {
        text.push('\n');
    }
    if text.trim().is_empty() {
        text.push_str("# NEXT_STEPS\n\n");
    }
    text.push_str(&format!(
        "\n### {} - {}\n- Task ID: `{}`\n- Status: Done.\n- Task type: {}.\n- Objective: {}\n",
        task.task_id, task.title, task.task_id, task.kind, task.objective
    ));
    if let Some(parent) = &task.parent_task {
        text.push_str(&format!("- Parent task: `{parent}`\n"));
    }
    if !task.allowed_files.is_empty() {
        text.push_str("- Allowed files:\n");
        for file in &task.allowed_files {
            text.push_str(&format!("  - `{file}`\n"));
        }
    }
    if !task.tests_to_run.is_empty() {
        text.push_str("- Verification commands:\n");
        for command in &task.tests_to_run {
            text.push_str(&format!("  - `{command}`\n"));
        }
    }
    for note in rust_review_notes(report, reason_code) {
        text.push_str(&note);
        text.push('\n');
    }
    write_text(queue_path, &text)
}

fn build_diff_summary(paths: &Paths, changed_files: &[String]) -> Result<DiffSummary> {
    // Small risk classifier input: enough to decide "boring" versus "needs
    // judgment" without parsing full diffs into model context.
    let mut insertions = 0u64;
    let mut deletions = 0u64;
    if find_git_root(&paths.root).is_some() && !changed_files.is_empty() {
        let mut args = vec![
            "diff".to_string(),
            "--numstat".to_string(),
            "--".to_string(),
        ];
        args.extend(changed_files.iter().cloned());
        if let Ok(output) = Command::new("git")
            .args(&args)
            .current_dir(&paths.root)
            .output()
        {
            if output.status.success() {
                for line in String::from_utf8_lossy(&output.stdout).lines() {
                    let parts = line.split_whitespace().collect::<Vec<_>>();
                    insertions += parts
                        .first()
                        .and_then(|part| part.parse::<u64>().ok())
                        .unwrap_or(0);
                    deletions += parts
                        .get(1)
                        .and_then(|part| part.parse::<u64>().ok())
                        .unwrap_or(0);
                }
            }
        }
    }
    let lower_paths = changed_files
        .iter()
        .map(|path| path.to_ascii_lowercase())
        .collect::<Vec<_>>();
    Ok(DiffSummary {
        changed_files: changed_files.to_vec(),
        insertions,
        deletions,
        touches_source: lower_paths
            .iter()
            .any(|path| path.starts_with("src/") || path.starts_with("sources/")),
        touches_tests: lower_paths
            .iter()
            .any(|path| path.starts_with("test/") || path.starts_with("tests/")),
        touches_docs: lower_paths
            .iter()
            .any(|path| path.ends_with(".md") || path.starts_with("docs/")),
        touches_tracking_docs: changed_files.iter().any(|path| is_tracking_doc_path(path)),
        touches_dependency_or_lockfile: lower_paths.iter().any(|path| {
            matches!(
                path.as_str(),
                "cargo.toml"
                    | "cargo.lock"
                    | "package.json"
                    | "package-lock.json"
                    | "pnpm-lock.yaml"
                    | "yarn.lock"
                    | "package.swift"
                    | "gemfile"
                    | "gemfile.lock"
            )
        }),
        touches_generated: lower_paths.iter().any(|path| {
            path.contains("generated")
                || path.ends_with(".pb.swift")
                || path.ends_with(".pb.rs")
                || path.ends_with(".gen.ts")
        }),
        touches_security_sensitive: lower_paths.iter().any(|path| {
            [
                "security",
                "auth",
                "crypto",
                "secret",
                "keychain",
                "migration",
                "signing",
            ]
            .iter()
            .any(|needle| path.contains(needle))
        }),
    })
}

fn classify_risk(task: &TaskSpec, diff: &DiffSummary) -> RiskLevel {
    // Risk classification is intentionally biased toward escalation.  Low risk is
    // a narrow allowlist; unknown or sensitive work should reach Senior/Planner.
    let task_text = format!(
        "{}\n{}\n{}\n{}",
        task.kind,
        task.title,
        task.objective,
        task.forbidden_actions.join("\n")
    )
    .to_ascii_lowercase();
    let source_auto_accept_candidate = source_auto_accept_risk_candidate(task, diff);
    let docs_auto_accept_candidate = docs_only_auto_accept_risk_candidate(task, diff);
    if !source_auto_accept_candidate
        && !docs_auto_accept_candidate
        && (task.requires_network
            || mentions_unnegated_risky_phrase(&task_text, "public diagnostic")
            || mentions_unnegated_risky_phrase(&task_text, "live routing"))
    {
        return RiskLevel::Critical;
    }
    if diff.touches_dependency_or_lockfile
        || diff.touches_generated
        || (!docs_auto_accept_candidate && diff.touches_security_sensitive)
        || (!source_auto_accept_candidate
            && !docs_auto_accept_candidate
            && diff.changed_files.len() > 8)
        || (!source_auto_accept_candidate
            && !docs_auto_accept_candidate
            && diff.insertions + diff.deletions > 800)
    {
        return RiskLevel::High;
    }
    if source_auto_accept_candidate || docs_auto_accept_candidate {
        return RiskLevel::Low;
    }
    if diff.touches_source {
        return RiskLevel::Medium;
    }
    if diff.changed_files.len() <= 4
        && diff.insertions + diff.deletions <= 400
        && (diff.touches_docs || diff.touches_tests || diff.touches_tracking_docs)
    {
        return RiskLevel::Low;
    }
    RiskLevel::Medium
}

fn source_auto_accept_risk_candidate(task: &TaskSpec, diff: &DiffSummary) -> bool {
    let explicitly_routed = task.review_tier == ReviewTier::AutoAcceptIfVerified
        && matches!(
            task.automation_level,
            AutomationLevel::AiWorkerRustReview | AutomationLevel::RustOnly
        );
    if !explicitly_routed && !inferred_machine_checkable_auto_accept_candidate(task) {
        return false;
    }
    if task.requires_network
        || !diff.touches_source
        || !diff.touches_tests
        || diff.touches_dependency_or_lockfile
        || diff.touches_generated
        || diff.touches_security_sensitive
        || diff.changed_files.len() > 12
        || diff.insertions + diff.deletions > 900
    {
        return false;
    }
    let all_paths_are_local_source_test_or_docs = diff.changed_files.iter().all(|path| {
        let lower = path.to_ascii_lowercase();
        lower.starts_with("sources/")
            || lower.starts_with("src/")
            || lower.starts_with("tests/")
            || lower.starts_with("test/")
            || lower.starts_with("docs/")
            || lower.ends_with(".md")
            || is_tracking_doc_path(path)
    });
    if !all_paths_are_local_source_test_or_docs {
        return false;
    }
    let task_text = format!(
        "{}\n{}\n{}\n{}",
        task.kind,
        task.title,
        task.objective,
        task.forbidden_actions.join("\n")
    )
    .to_ascii_lowercase();
    let dangerous_intent = [
        "dependency",
        "lockfile",
        "migration",
        "credential",
        "secret",
        "key storage",
        "crypto",
        "public api",
        "public diagnostic",
        "live routing",
        "enable routing",
        "production socket",
        "public socket",
        "network access",
    ]
    .iter()
    .any(|needle| {
        if *needle == "live routing"
            && task_text.contains("kill switch")
            && task_text.contains("disabled")
        {
            return false;
        }
        mentions_unnegated_risky_phrase(&task_text, needle)
    });
    !dangerous_intent
}

fn docs_only_auto_accept_risk_candidate(task: &TaskSpec, diff: &DiffSummary) -> bool {
    if !inferred_docs_only_auto_accept_candidate(task)
        || task.requires_network
        || diff.touches_source
        || diff.touches_tests
        || diff.touches_dependency_or_lockfile
        || diff.touches_generated
        || diff.changed_files.len() > 10
        || diff.insertions + diff.deletions > 700
    {
        return false;
    }
    diff.changed_files.iter().all(|path| {
        let lower = path.to_ascii_lowercase();
        lower.starts_with("docs/") || lower.ends_with(".md") || is_tracking_doc_path(path)
    })
}

fn mentions_unnegated_risky_phrase(text: &str, phrase: &str) -> bool {
    let mut start = 0;
    while let Some(offset) = text[start..].find(phrase) {
        let phrase_start = start + offset;
        let phrase_end = phrase_start + phrase.len();
        let prefix_start = phrase_start.saturating_sub(96);
        let prefix = &text[prefix_start..phrase_start];
        let local_end = (phrase_end + 96).min(text.len());
        let local = &text[prefix_start..local_end];
        let direct_negated = [
            format!("no {phrase}"),
            format!("without {phrase}"),
            format!("does not {phrase}"),
            format!("do not {phrase}"),
            format!("do not add {phrase}"),
            format!("do not run {phrase}"),
            format!("do not enable {phrase}"),
            format!("do not expose {phrase}"),
            format!("do not open {phrase}"),
            format!("must not {phrase}"),
            format!("must not add {phrase}"),
            format!("must not run {phrase}"),
            format!("must not enable {phrase}"),
            format!("must not expose {phrase}"),
            format!("must not open {phrase}"),
            format!("forbid {phrase}"),
            format!("forbidden {phrase}"),
            format!("refuse {phrase}"),
            format!("refuses {phrase}"),
            format!("{phrase} disabled"),
            format!("{phrase} remain disabled"),
            format!("{phrase} remains disabled"),
            format!("{phrase} kept disabled"),
            format!("{phrase} stay disabled"),
            format!("{phrase} stays disabled"),
            format!("{phrase} not approved"),
        ]
        .iter()
        .any(|form| local.contains(form));
        let clause_negated = [
            "no ",
            "without ",
            "do not ",
            "does not ",
            "must not ",
            "forbid",
            "forbidden",
            "refuse",
            "refuses",
            "disabled",
            "remain disabled",
            "remains disabled",
        ]
        .iter()
        .any(|marker| prefix.contains(marker));
        if !direct_negated && !clause_negated {
            return true;
        }
        start = phrase_end;
    }
    false
}

fn task_policy_from_task(task: &TaskSpec) -> TaskPolicy {
    TaskPolicy {
        allowed_file_patterns: task.allowed_files.clone(),
        forbidden_file_patterns: task.forbidden_actions.clone(),
        max_diff_lines: Some(800),
        allow_network: task.requires_network,
        allow_dependency_changes: false,
        allow_lockfile_changes: false,
        allow_public_api_changes: false,
        review_tier: task.review_tier,
        automation_level: task.automation_level,
    }
}

fn validate_task_policy(
    paths: &Paths,
    task: &TaskSpec,
    policy: &TaskPolicy,
) -> Vec<DiagnosticFinding> {
    let mut findings = Vec::new();
    if policy.allowed_file_patterns.is_empty() {
        findings.push(policy_finding(
            "Task policy has no allowed file patterns; Worker scope would be unbounded.",
        ));
    }
    if task.requires_network && !policy.allow_network {
        findings.push(policy_finding(
            "Task requires network but policy does not allow network access.",
        ));
    }
    if !policy.allow_dependency_changes
        && task
            .allowed_files
            .iter()
            .any(|path| is_dependency_or_lockfile_path(path))
    {
        findings.push(policy_finding(
            "Task allows dependency or manifest changes while policy forbids dependency changes.",
        ));
    }
    if task_requires_renderer_campaign_preflight(task)
        && !repo_has_renderer_campaign_preflight(paths)
    {
        findings.push(policy_finding(
            "Renderer/campaign-affecting task is missing a cited seven-field preflight packet. Fill the `Renderer/Campaign Preflight` template from analysis/process_guardrails.md in CONTINUITY.md or an active ExecPlan before Worker runs.",
        ));
    }
    findings
}

fn policy_finding(message: &str) -> DiagnosticFinding {
    DiagnosticFinding {
        source: FindingSource::CustomPolicy,
        severity: Severity::Error,
        path: None,
        line: None,
        column: None,
        code: Some("task-policy".to_string()),
        message: message.to_string(),
        raw_excerpt: None,
    }
}

fn is_dependency_or_lockfile_path(path: &str) -> bool {
    matches!(
        path.to_ascii_lowercase().as_str(),
        "cargo.toml"
            | "cargo.lock"
            | "package.json"
            | "package-lock.json"
            | "pnpm-lock.yaml"
            | "yarn.lock"
            | "package.swift"
            | "gemfile"
            | "gemfile.lock"
    )
}

fn task_requires_renderer_campaign_preflight(task: &TaskSpec) -> bool {
    task.allowed_files
        .iter()
        .chain(task.read_only_files.iter())
        .map(|path| normalize_glob_pattern(path))
        .any(|path| path_requires_renderer_campaign_preflight(&path))
}

fn path_requires_renderer_campaign_preflight(path: &str) -> bool {
    let path = path.trim_matches('`').trim().replace('\\', "/");
    let lower = path.to_ascii_lowercase();
    lower.starts_with("sources/")
        || lower == "sources"
        || lower.starts_with("tests/")
        || lower == "tests"
        || lower.starts_with("scripts/")
        || lower == "scripts"
        || lower.starts_with("benchmarks/reports")
        || lower.contains("swiftstackerbenchmarkcli")
        || lower.contains("benchmarkdiagnosticsartifacts")
        || (lower.starts_with("analysis/")
            && [
                "campaign",
                "renderer",
                "benchmark",
                "parity",
                "rejected_hypotheses",
                "process_guardrails",
            ]
            .iter()
            .any(|needle| lower.contains(needle)))
}

fn repo_has_renderer_campaign_preflight(paths: &Paths) -> bool {
    let mut candidates = vec![paths.root.join("CONTINUITY.md")];
    for dir in [
        paths.root.join("docs/exec-plans/active"),
        paths.root.join("Docs/exec-plans/active"),
    ] {
        if !dir.exists() {
            continue;
        }
        for entry in WalkDir::new(dir)
            .max_depth(1)
            .into_iter()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_type().is_file())
        {
            candidates.push(entry.path().to_path_buf());
        }
    }
    candidates
        .iter()
        .filter_map(|path| fs::read_to_string(path).ok())
        .any(|text| text_contains_renderer_campaign_preflight(&text))
}

fn text_contains_renderer_campaign_preflight(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("renderer/campaign preflight")
        && [
            "current accepted baseline",
            "current first unresolved mismatch",
            "files/artifacts proving the mismatch",
            "solved areas that must not be reopened",
            "metrics that are diagnostic only",
            "product promotion gates",
            "exact rejection condition",
        ]
        .iter()
        .all(|needle| lower.contains(needle))
}

fn compute_affected_work(paths: &Paths, task: Option<&TaskSpec>) -> Result<AffectedWork> {
    let changed_files = git_name_only(paths)
        .unwrap_or_default()
        .into_iter()
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    let seed_files = if changed_files.is_empty() {
        task.map(|task| {
            task.allowed_files
                .iter()
                .chain(task.read_only_files.iter())
                .map(PathBuf::from)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
    } else {
        changed_files.clone()
    };
    let mut modules = Vec::new();
    let mut likely_tests = Vec::new();
    for path in &seed_files {
        if let Some(module) = module_name_from_path(path) {
            modules.push(module.clone());
            if path.components().any(|component| {
                component.as_os_str() == "tests" || component.as_os_str() == "Tests"
            }) {
                if let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) {
                    likely_tests.push(recipe_spec_from_words(
                        vec!["cargo".to_string(), "test".to_string(), stem.to_string()],
                        "affected-work",
                    ));
                }
            }
        }
    }
    let mut likely_tests = likely_tests.into_iter().flatten().collect::<Vec<_>>();
    likely_tests.sort_by(|a, b| a.id.cmp(&b.id));
    likely_tests.dedup_by(|a, b| a.id == b.id);
    modules.sort();
    modules.dedup();
    let affected_recipes = if likely_tests.is_empty() {
        vec![recipe_spec_from_words(
            vec!["git".to_string(), "diff".to_string(), "--check".to_string()],
            "affected-work",
        )]
        .into_iter()
        .flatten()
        .collect()
    } else {
        likely_tests.clone()
    };
    let dependency_recipes = dependency_policy_recipes_for_paths(
        &seed_files
            .iter()
            .map(|path| path.to_string_lossy().to_string())
            .collect::<Vec<_>>(),
    );
    let confidence = if !likely_tests.is_empty() || !dependency_recipes.is_empty() {
        AffectedConfidence::Medium
    } else if !seed_files.is_empty() {
        AffectedConfidence::Low
    } else {
        AffectedConfidence::Unknown
    };
    Ok(AffectedWork {
        changed_files: seed_files,
        affected_modules: modules,
        affected_recipes,
        dependency_recipes,
        likely_tests,
        confidence,
    })
}

fn module_name_from_path(path: &Path) -> Option<String> {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .map(|stem| stem.trim_end_matches("Tests").to_string())
        .filter(|stem| !stem.is_empty())
}

fn build_watch_plan(paths: &Paths, task_id: Option<&str>, state: &State) -> Result<WatchPlan> {
    let task = task_id.and_then(|id| state.queue.iter().find(|task| task.task_id == id));
    let mut paths_to_watch = task
        .map(|task| {
            task.allowed_files
                .iter()
                .chain(task.read_only_files.iter())
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| {
            git_name_only(paths).unwrap_or_else(|| vec!["NEXT_STEPS.md".to_string()])
        });
    paths_to_watch.sort();
    paths_to_watch.dedup();
    Ok(WatchPlan {
        task_id: task_id.map(str::to_string),
        paths: paths_to_watch,
        debounce_ms: 750,
    })
}

fn build_efficiency_watch_report(
    paths: &Paths,
    state: &State,
    watch: WatchPlan,
) -> Result<EfficiencyWatchReport> {
    let records = read_model_call_records(paths)?;
    let prompt_token_threshold = 10_000usize;
    let mut alerts = Vec::new();
    alerts.extend(efficiency_prompt_alerts(&records, prompt_token_threshold));
    alerts.extend(efficiency_adjacent_role_alerts(
        &records,
        prompt_token_threshold,
    ));
    alerts.extend(efficiency_state_alerts(state));
    alerts.extend(efficiency_stale_observation_hint_alerts(paths));
    alerts.extend(efficiency_pm_file_edit_alerts(paths));
    alerts.extend(efficiency_pm_worker_doc_bounce_alerts(paths, &records));
    alerts.extend(efficiency_worker_deterministic_command_alerts(
        paths, &records,
    ));
    alerts.extend(tool_health_watch_alerts(paths, &records));
    let derived_status = build_derived_status(paths, state);
    if matches!(
        derived_status.status,
        crate::domain::DerivedStatusKind::NoSignal
            | crate::domain::DerivedStatusKind::BlockedByToolHealth
    ) {
        alerts.push(EfficiencyWatchAlert {
            severity: "warn".to_string(),
            kind: format!("derived_status_{}", derived_status.status.as_str()),
            summary: derived_status.summary.clone(),
            evidence: derived_status.evidence.clone(),
            mechanistic_handoff: None,
        });
    }
    let _ = record_redundant_exploration_from_worker_logs(paths, &records);
    alerts.extend(efficiency_route_regret_alerts(paths));
    let suggestions = efficiency_watch_suggestions(&alerts);
    Ok(EfficiencyWatchReport {
        generated_at: Utc::now().to_rfc3339(),
        task_id: watch.task_id.clone(),
        prompt_token_threshold,
        derived_status: Some(derived_status),
        alerts,
        suggestions,
        watch,
    })
}

fn efficiency_prompt_alerts(
    records: &[ModelCallRecord],
    prompt_token_threshold: usize,
) -> Vec<EfficiencyWatchAlert> {
    records
        .iter()
        .rev()
        .filter(|record| !record.skipped_by_rust)
        .take(6)
        .filter(|record| record.prompt_token_estimate >= prompt_token_threshold)
        .map(|record| EfficiencyWatchAlert {
            severity: "warn".to_string(),
            kind: "prompt_bloat".to_string(),
            summary: format!(
                "{} prompt was ~{} tokens for {}.",
                normalize_cost_role(&record.role),
                format_usize(record.prompt_token_estimate),
                record
                    .task_id
                    .as_deref()
                    .unwrap_or("repo-level orchestration")
            ),
            evidence: vec![
                format!("timestamp={}", record.timestamp),
                format!("reason={}", record.skip_or_call_reason),
            ],
            mechanistic_handoff: None,
        })
        .collect()
}

fn efficiency_adjacent_role_alerts(
    records: &[ModelCallRecord],
    prompt_token_threshold: usize,
) -> Vec<EfficiencyWatchAlert> {
    let calls = records
        .iter()
        .filter(|record| !record.skipped_by_rust)
        .collect::<Vec<_>>();
    let mut alerts = Vec::new();
    for window in calls.windows(2).rev().take(8) {
        let first = window[0];
        let second = window[1];
        let first_role = normalize_cost_role(&first.role);
        let second_role = normalize_cost_role(&second.role);
        if first_role == "Project Manager"
            && second_role == "Worker"
            && first.prompt_token_estimate >= prompt_token_threshold
            && second.prompt_token_estimate >= prompt_token_threshold
        {
            alerts.push(EfficiencyWatchAlert {
                severity: "high".to_string(),
                kind: "pm_worker_double_context".to_string(),
                summary: format!(
                    "Project Manager and Worker ran back-to-back with large prompts (~{} + ~{} tokens).",
                    format_usize(first.prompt_token_estimate),
                    format_usize(second.prompt_token_estimate)
                ),
                evidence: vec![
                    format!("pm_at={}", first.timestamp),
                    format!("worker_at={}", second.timestamp),
                    format!(
                        "worker_task={}",
                        second.task_id.as_deref().unwrap_or("(none)")
                    ),
                ],
                mechanistic_handoff: None,
            });
        }
    }
    alerts
}

fn efficiency_state_alerts(state: &State) -> Vec<EfficiencyWatchAlert> {
    let mut alerts = Vec::new();
    for (task_id, failures) in &state.failed_tasks {
        let active_failure = state.retry_worker_same_task.as_deref() == Some(task_id.as_str())
            || state.senior_dev_repair_order.as_deref() == Some(task_id.as_str())
            || state.queue.iter().any(|task| task.task_id == *task_id);
        if *failures >= 2 && active_failure {
            alerts.push(EfficiencyWatchAlert {
                severity: "warn".to_string(),
                kind: "repeated_task_failure".to_string(),
                summary: format!("{task_id} has failed or retried {failures} time(s)."),
                evidence: vec![
                    "Repeated failures should trigger a compact repair brief or Senior gate instead of another full Worker prompt."
                        .to_string(),
                ],
                mechanistic_handoff: None,
            });
        }
    }
    if state.planner_due && state.queue.is_empty() && state.retry_worker_same_task.is_none() {
        alerts.push(EfficiencyWatchAlert {
            severity: "info".to_string(),
            kind: "planner_due_empty_queue".to_string(),
            summary:
                "Project Manager is due with an empty queue; compact queue/control-doc summary should be enough."
                    .to_string(),
            evidence: vec!["queue=[] and retry_worker_same_task=null".to_string()],
            mechanistic_handoff: None,
        });
    }
    alerts
}

fn efficiency_stale_observation_hint_alerts(paths: &Paths) -> Vec<EfficiencyWatchAlert> {
    let suppressed = suppressed_verification_hint_fingerprints(paths);
    failed_observation_hint_commands(paths)
        .into_iter()
        .filter(|command| !suppressed.contains(&command.semantic_fingerprint))
        .take(5)
        .map(|command| EfficiencyWatchAlert {
            severity: "warn".to_string(),
            kind: "stale_verification_hint".to_string(),
            summary: format!(
                "Observation hint `{}` failed recently and should be skipped until refreshed.",
                command.command_display
            ),
            evidence: vec![
                format!("task={}", command.task_id),
                format!("source={}", command.source),
                format!("category={}", command.category),
                format!("completed_at={}", command.completed_at),
            ],
            mechanistic_handoff: None,
        })
        .collect()
}

fn efficiency_pm_file_edit_alerts(paths: &Paths) -> Vec<EfficiencyWatchAlert> {
    let Ok(events) = read_orchestrator_events(paths) else {
        return Vec::new();
    };
    events
        .iter()
        .rev()
        .filter_map(|event| {
            let mode = event.payload.get("mode").and_then(Value::as_str)?;
            match mode {
                "pm_mutated_files_detected" => {
                    let files = event
                        .payload
                        .get("changed_files")
                        .and_then(Value::as_array)
                        .map(|values| values.len())
                        .unwrap_or(0);
                    Some(EfficiencyWatchAlert {
                        severity: "high".to_string(),
                        kind: "pm_file_edit".to_string(),
                        summary: format!(
                            "Project Manager edited {files} file(s); PM should be read-only."
                        ),
                        evidence: vec![format!("event_at={}", event.timestamp)],
                        mechanistic_handoff: None,
                    })
                }
                "pm_control_doc_reconciliation_auto_committed" => Some(EfficiencyWatchAlert {
                    severity: "info".to_string(),
                    kind: "pm_control_doc_edit_self_healed".to_string(),
                    summary:
                        "Project Manager control-doc edits were mechanically committed by Rust."
                            .to_string(),
                    evidence: vec![format!("event_at={}", event.timestamp)],
                    mechanistic_handoff: None,
                }),
                _ => None,
            }
        })
        .take(3)
        .collect()
}

fn efficiency_pm_worker_doc_bounce_alerts(
    paths: &Paths,
    records: &[ModelCallRecord],
) -> Vec<EfficiencyWatchAlert> {
    if !records.windows(2).rev().take(8).any(|window| {
        normalize_cost_role(&window[0].role) == "Project Manager"
            && normalize_cost_role(&window[1].role) == "Worker"
    }) {
        return Vec::new();
    }
    let Ok(events) = read_orchestrator_events(paths) else {
        return Vec::new();
    };
    let recent_doc_worker_guard = events.iter().rev().take(20).find(|event| {
        event.kind == OrchestratorEventKind::GuardChecked
            && event
                .payload
                .get("changed_files")
                .and_then(Value::as_array)
                .is_some_and(|files| {
                    !files.is_empty()
                        && files
                            .iter()
                            .filter_map(Value::as_str)
                            .all(is_docs_or_tracking_doc_pattern)
                })
    });
    recent_doc_worker_guard
        .map(|event| {
            vec![EfficiencyWatchAlert {
                severity: "warn".to_string(),
                kind: "pm_worker_doc_bounce".to_string(),
                summary: "Project Manager and Worker both spent a cycle around doc-only changes."
                    .to_string(),
                evidence: vec![format!("worker_guard_at={}", event.timestamp)],
                mechanistic_handoff: None,
            }]
        })
        .unwrap_or_default()
}

fn efficiency_worker_deterministic_command_alerts(
    paths: &Paths,
    records: &[ModelCallRecord],
) -> Vec<EfficiencyWatchAlert> {
    mechanistic_handoff_candidates_from_worker_logs(paths, records)
        .into_iter()
        .map(|candidate| {
            let mut evidence = vec![
                format!("command_fingerprint={}", candidate.command_fingerprint),
                format!("action={}", candidate.action),
                format!("receipt_exists={}", candidate.receipt_exists),
            ];
            if let Some(log) = &candidate.source_log {
                evidence.push(format!("log={log}"));
            }
            if let Some(task_id) = &candidate.source_task_id {
                evidence.push(format!("task={task_id}"));
            }
            if let Some(tokens) = candidate.prompt_tokens {
                evidence.push(format!("prompt_tokens={tokens}"));
            }
            if let Some(duration_ms) = candidate.observed_duration_ms {
                evidence.push(format!("observed_duration_ms={duration_ms}"));
            }
            EfficiencyWatchAlert {
                severity: if candidate.action == "unsupported_sent_to_orchestrator" {
                    "high".to_string()
                } else {
                    "warn".to_string()
                },
                kind: "worker_ran_deterministic_command".to_string(),
                summary: format!(
                    "Worker ran deterministic command `{}`; Rust evidence jobs should own this.",
                    truncate_inline(&candidate.command, 160)
                ),
                evidence,
                mechanistic_handoff: Some(MechanisticHandoffAlert {
                    candidate_id: candidate.candidate_id,
                    command: candidate.command,
                    command_fingerprint: candidate.command_fingerprint,
                    source_log: candidate.source_log,
                    source_task_id: candidate.source_task_id,
                    prompt_tokens: candidate.prompt_tokens,
                    observed_duration_ms: candidate.observed_duration_ms,
                    receipt_exists: candidate.receipt_exists,
                    action: candidate.action,
                }),
            }
        })
        .take(3)
        .collect()
}

fn efficiency_route_regret_alerts(paths: &Paths) -> Vec<EfficiencyWatchAlert> {
    let Ok(report) = build_route_regret_report(paths) else {
        return Vec::new();
    };
    report
        .recent
        .iter()
        .take(5)
        .map(|entry| {
            let mut evidence = vec![
                format!("attempt={}", entry.attempt_id),
                format!("route_kind={}", entry.route_kind),
                format!("execution_lane={}", entry.execution_lane),
                format!("estimated_saved_tokens={}", entry.tokens),
            ];
            if let Some(task_id) = &entry.task_id {
                evidence.push(format!("task={task_id}"));
            }
            if let Some(better_lane) = &entry.better_lane {
                evidence.push(format!("better_lane={better_lane}"));
            }
            EfficiencyWatchAlert {
                severity: match entry.kind.as_str() {
                    "repeated_fingerprint_no_progress" | "pm_worker_double_context" => {
                        "high".to_string()
                    }
                    _ => "warn".to_string(),
                },
                kind: format!("route_regret_{}", entry.kind),
                summary: entry.summary.clone().unwrap_or_else(|| {
                    format!(
                        "Route memory recorded `{}` and suggests `{}`.",
                        entry.kind,
                        entry.better_lane.as_deref().unwrap_or("a cheaper lane")
                    )
                }),
                evidence,
                mechanistic_handoff: None,
            }
        })
        .collect()
}

fn record_redundant_exploration_from_worker_logs(
    paths: &Paths,
    records: &[ModelCallRecord],
) -> usize {
    if !paths.runs.exists() {
        return 0;
    }
    let Ok(logs) = recent_stdout_jsonl_logs(paths, 12) else {
        return 0;
    };
    let mut recorded = read_route_outcomes(paths)
        .unwrap_or_default()
        .into_iter()
        .filter(|outcome| outcome.regret_kind == Some(RouteRegretKind::RedundantExploration))
        .filter_map(|outcome| {
            outcome.failure_fingerprint.map(|fingerprint| {
                redundant_exploration_record_key(outcome.task_id.as_deref(), &fingerprint)
            })
        })
        .collect::<HashSet<_>>();
    let mut inserted = 0usize;
    for path in logs.into_iter().filter(|path| {
        path.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with("-worker.stdout.jsonl"))
    }) {
        let task_id = worker_model_record_for_log(records, &path).and_then(|record| {
            record
                .task_id
                .as_deref()
                .map(str::trim)
                .filter(|task_id| !task_id.is_empty())
                .map(str::to_string)
        });
        for finding in redundant_exploration_findings_from_worker_log(&path) {
            let key = redundant_exploration_record_key(task_id.as_deref(), &finding.fingerprint);
            if !recorded.insert(key) {
                continue;
            }
            let source = finding
                .source_log
                .as_deref()
                .unwrap_or("unknown worker log");
            let summary = format!(
                "Worker repeated read/search command `{}` {} times in `{}`.",
                truncate_inline(&finding.command, 120),
                finding.occurrences,
                source
            );
            if record_route_regret_observation(
                paths,
                task_id.as_deref(),
                RouteRegretKind::RedundantExploration,
                ExecutionLane::SharedWorker,
                ExecutionLane::RustBookkeeping,
                Some(&finding.fingerprint),
                &summary,
            )
            .is_ok()
            {
                inserted += 1;
            }
        }
    }
    inserted
}

fn worker_model_record_for_log<'a>(
    records: &'a [ModelCallRecord],
    path: &Path,
) -> Option<&'a ModelCallRecord> {
    let log_started = worker_log_timestamp(path);
    if let Some(log_started) = log_started {
        let mut closest = None;
        for record in records
            .iter()
            .filter(|record| normalize_cost_role(&record.role) == "Worker")
        {
            let Ok(timestamp) = DateTime::parse_from_rfc3339(&record.timestamp) else {
                continue;
            };
            let delta = timestamp
                .with_timezone(&Utc)
                .signed_duration_since(log_started)
                .num_seconds()
                .unsigned_abs();
            if closest
                .as_ref()
                .is_none_or(|(best_delta, _): &(u64, &ModelCallRecord)| delta < *best_delta)
            {
                closest = Some((delta, record));
            }
        }
        if let Some((delta, record)) = closest {
            if delta <= 3_600 {
                return Some(record);
            }
        }
    }
    records
        .iter()
        .rev()
        .find(|record| normalize_cost_role(&record.role) == "Worker")
}

fn worker_log_timestamp(path: &Path) -> Option<DateTime<Utc>> {
    let name = path.file_name()?.to_str()?;
    let stamp = name.split('-').next()?;
    NaiveDateTime::parse_from_str(stamp, "%Y%m%dT%H%M%SZ")
        .ok()
        .map(|timestamp| timestamp.and_utc())
}

fn redundant_exploration_record_key(task_id: Option<&str>, fingerprint: &str) -> String {
    format!("{}|{}", task_id.unwrap_or("repo"), fingerprint)
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct RedundantExplorationFinding {
    fingerprint: String,
    command: String,
    occurrences: usize,
    source_log: Option<String>,
}

fn redundant_exploration_findings_from_worker_log(path: &Path) -> Vec<RedundantExplorationFinding> {
    let Ok(file) = File::open(path) else {
        return Vec::new();
    };
    let source_log = path
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_string);
    let reader = BufReader::new(file);
    let mut commands = BTreeMap::<String, (String, usize)>::new();
    for line in reader.lines().map_while(Result::ok) {
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let Some(item) = value.get("item") else {
            continue;
        };
        if value.get("type").and_then(Value::as_str) != Some("item.completed")
            || item.get("type").and_then(Value::as_str) != Some("command_execution")
        {
            continue;
        }
        let Some(command) = item.get("command").and_then(Value::as_str) else {
            continue;
        };
        let command = friendly_command(command);
        let Some(fingerprint) = redundant_exploration_fingerprint(&command) else {
            continue;
        };
        let entry = commands.entry(fingerprint).or_insert((command, 0));
        entry.1 += 1;
    }
    commands
        .into_iter()
        .filter_map(|(fingerprint, (command, occurrences))| {
            (occurrences > 1).then(|| RedundantExplorationFinding {
                fingerprint,
                command,
                occurrences,
                source_log: source_log.clone(),
            })
        })
        .collect()
}

fn redundant_exploration_fingerprint(command: &str) -> Option<String> {
    let normalized = command.split_whitespace().collect::<Vec<_>>().join(" ");
    let command = normalized.trim();
    if command.is_empty() {
        return None;
    }
    let read_like = command.starts_with("sed ")
        || command.starts_with("cat ")
        || command.starts_with("head ")
        || command.starts_with("tail ");
    let search_like = command.starts_with("rg ");
    if !read_like && !search_like {
        return None;
    }
    Some(stable_hash_hex(&format!("worker-exploration\0{command}")))
}

fn mechanistic_handoff_candidates_from_worker_logs(
    paths: &Paths,
    records: &[ModelCallRecord],
) -> Vec<MechanisticHandoffCandidate> {
    let Ok(entries) = fs::read_dir(&paths.runs) else {
        return Vec::new();
    };
    let latest_worker = records
        .iter()
        .rev()
        .find(|record| normalize_cost_role(&record.role) == "Worker");
    let mut worker_logs = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with("-worker.stdout.jsonl"))
        })
        .collect::<Vec<_>>();
    worker_logs.sort();
    worker_logs
        .into_iter()
        .rev()
        .take(6)
        .filter_map(|path| {
            let text = read_text(&path).ok()?;
            let command = worker_log_long_deterministic_command(&text)?;
            let source_log = path
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_string);
            Some(mechanistic_handoff_candidate_for_command(
                paths,
                &command.command,
                "worker_log",
                source_log,
                latest_worker.and_then(|record| record.task_id.clone()),
                latest_worker.map(|record| record.prompt_token_estimate),
                command.observed_duration_ms,
            ))
        })
        .take(3)
        .collect()
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct WorkerDeterministicCommand {
    command: String,
    observed_duration_ms: Option<u128>,
}

fn worker_log_long_deterministic_command(text: &str) -> Option<WorkerDeterministicCommand> {
    for line in text.lines().rev() {
        let normalized = line
            .replace("\\n", "\n")
            .replace("\\\"", "\"")
            .replace("\\/", "/");
        for line in normalized.lines() {
            let Some(command) = extract_running_command_from_line(line) else {
                continue;
            };
            if command.contains("SwiftStackerBench")
                || command == "swift build -c release --product SwiftStackerBench"
                || command.starts_with("swift test")
                || command.starts_with("swift build")
            {
                return Some(WorkerDeterministicCommand {
                    command,
                    observed_duration_ms: extract_running_command_duration_ms(line),
                });
            }
        }
    }
    None
}

fn mechanistic_handoff_candidate_for_command(
    paths: &Paths,
    command_text: &str,
    source: &str,
    source_log: Option<String>,
    source_task_id: Option<String>,
    prompt_tokens: Option<usize>,
    observed_duration_ms: Option<u128>,
) -> MechanisticHandoffCandidate {
    let parsed = parse_verification_command(command_text, "mechanistic.watch");
    let (command_display, command_fingerprint, receipt_exists, action, reason) = if let Some(
        command,
    ) = parsed
    {
        let command_display = command.display();
        let command_fingerprint = stable_hash_hex(&command.semantic_fingerprint());
        let receipt_exists = rust_evidence_receipt_exists_for_command(paths, &command_display);
        let allowed = is_deterministic_command_structured_allowed(&command);
        let action = if !allowed {
            "unsafe_rejected"
        } else if receipt_exists {
            "already_materialized"
        } else {
            "materialized"
        };
        let reason = if allowed {
            "Command is a safe typed recipe; future command-only work can run as a Rust evidence job."
        } else {
            "Command parsed but failed the final deterministic allowlist."
        };
        (
            command_display,
            command_fingerprint,
            receipt_exists,
            action.to_string(),
            reason.to_string(),
        )
    } else {
        (
            command_text.trim().to_string(),
            stable_hash_hex(command_text.trim()),
            false,
            "unsupported_sent_to_orchestrator".to_string(),
            "Command is deterministic-looking but not representable as a safe typed recipe yet."
                .to_string(),
        )
    };
    let id_seed = format!(
        "{}\n{}\n{}\n{}",
        source,
        source_log.as_deref().unwrap_or(""),
        source_task_id.as_deref().unwrap_or(""),
        command_fingerprint
    );
    MechanisticHandoffCandidate {
        candidate_id: format!("MECH-{}", &stable_hash_hex(&id_seed)[..12]),
        command: command_display,
        command_fingerprint,
        source: source.to_string(),
        source_log,
        source_task_id,
        prompt_tokens,
        observed_duration_ms,
        receipt_exists,
        action,
        reason,
        materialized_at: Utc::now().to_rfc3339(),
    }
}

fn rust_evidence_receipt_exists_for_command(paths: &Paths, command_display: &str) -> bool {
    recent_rust_evidence_job_receipts(paths, 100)
        .into_iter()
        .any(|receipt| {
            receipt
                .command_summaries
                .iter()
                .any(|summary| summary.contains(&format!("`{command_display}`")))
        })
}

fn extract_running_command_from_line(line: &str) -> Option<String> {
    let start = line.find("Running ")? + "Running ".len();
    let rest = &line[start..];
    let end = rest
        .find(" for ")
        .or_else(|| rest.find("; raw log"))
        .unwrap_or(rest.len());
    let command = rest[..end]
        .trim()
        .trim_matches('"')
        .trim_matches('`')
        .trim();
    (!command.is_empty()).then(|| command.to_string())
}

fn extract_running_command_duration_ms(line: &str) -> Option<u128> {
    let marker = " for ";
    let start = line.find(marker)? + marker.len();
    let rest = &line[start..];
    let raw = rest.split([';', '"', '}']).next().unwrap_or(rest).trim();
    parse_human_duration_ms(raw)
}

fn parse_human_duration_ms(raw: &str) -> Option<u128> {
    let raw = raw.trim();
    if let Some(ms) = raw.strip_suffix("ms") {
        return ms.trim().parse::<u128>().ok();
    }
    if let Some(seconds) = raw.strip_suffix('s') {
        if let Some((minutes, seconds)) = seconds.split_once('m') {
            let minutes = minutes.trim().parse::<u128>().ok()?;
            let seconds = seconds.trim().parse::<u128>().ok()?;
            return Some((minutes * 60 + seconds) * 1_000);
        }
        return seconds
            .trim()
            .parse::<u128>()
            .ok()
            .map(|value| value * 1_000);
    }
    None
}

fn efficiency_watch_suggestions(alerts: &[EfficiencyWatchAlert]) -> Vec<String> {
    let mut suggestions = BTreeSet::new();
    for alert in alerts {
        match alert.kind.as_str() {
            "prompt_bloat" | "pm_worker_double_context" => {
                suggestions.insert(
                    "Compact PM/Worker prompt packets; pass queue summaries and task cards instead of full control docs."
                        .to_string(),
                );
                suggestions.insert(
                    "Add section-level prompt token accounting so large prompt contributors are visible."
                        .to_string(),
                );
            }
            "repeated_task_failure" => {
                suggestions.insert(
                    "Prefer a Rust-generated retry brief or Senior repair route before another full Worker call."
                        .to_string(),
                );
            }
            "planner_due_empty_queue" => {
                suggestions.insert(
                    "Run Project Manager with compact context only, or materialize deterministic work before PM."
                        .to_string(),
                );
            }
            "stale_verification_hint" => {
                suggestions.insert(
                    "Suppress failed observation-memory verification hints until a task explicitly refreshes them with passing evidence."
                        .to_string(),
                );
            }
            "pm_file_edit" | "pm_control_doc_edit_self_healed" | "pm_worker_doc_bounce" => {
                suggestions.insert(
                    "Keep Project Manager read-only; let Rust commit safe control-doc reconciliation or route one bounded Worker task."
                        .to_string(),
                );
            }
            "worker_ran_deterministic_command" => {
                suggestions.insert(
                    "Route long deterministic builds, tests, and SwiftStacker benchmarks to Senior progress rust_evidence_job actions."
                        .to_string(),
                );
            }
            "tool_health_cooldown" => {
                suggestions.insert(
                    "Check recent tool-health evidence before retrying the same role/model; cooldown is advisory until fallback policy exists."
                        .to_string(),
                );
            }
            kind if kind.starts_with("derived_status_") => {
                suggestions.insert(
                    "Use derived status as an operator triage hint; persisted routing flags remain the source of truth."
                        .to_string(),
                );
            }
            kind if kind.starts_with("route_regret_") => {
                suggestions.insert(
                    "Use route memory before launching another role: prefer Rust evidence/bookkeeping for repeated high-confidence regret cases."
                        .to_string(),
                );
            }
            _ => {}
        }
    }
    suggestions.into_iter().collect()
}

fn apply_efficiency_self_heal(
    paths: &Paths,
    cfg: &Config,
    report: &EfficiencyWatchReport,
) -> Result<EfficiencySelfHealReport> {
    fs::create_dir_all(&paths.observations)?;
    let mut actions = Vec::new();
    if report.alerts.iter().any(|alert| {
        matches!(
            alert.kind.as_str(),
            "prompt_bloat" | "pm_worker_double_context" | "planner_due_empty_queue"
        )
    }) {
        actions.push(self_heal_compact_planner_context(paths, cfg)?);
    }
    if report
        .alerts
        .iter()
        .any(|alert| alert.kind == "stale_verification_hint")
    {
        actions.push(self_heal_suppress_stale_verification_hints(paths)?);
    }
    if report
        .alerts
        .iter()
        .any(|alert| alert.kind == "worker_ran_deterministic_command")
    {
        actions.push(self_heal_materialize_mechanistic_handoffs(paths, report)?);
    }

    let self_heal_report = EfficiencySelfHealReport {
        generated_at: Utc::now().to_rfc3339(),
        actions,
    };
    write_efficiency_self_heal_report(paths, &self_heal_report)?;
    append_event(
        paths,
        OrchestratorEventKind::WatchDebounced,
        None,
        json!({
            "mode": "efficiency_self_heal",
            "action_count": self_heal_report.actions.len(),
        }),
    )?;
    Ok(self_heal_report)
}

fn self_heal_compact_planner_context(
    paths: &Paths,
    cfg: &Config,
) -> Result<EfficiencySelfHealAction> {
    if cfg.planner_context == PlannerContextMode::Compact {
        return Ok(EfficiencySelfHealAction {
            kind: "compact_planner_context".to_string(),
            status: "already_applied".to_string(),
            summary: "planner_context is already compact".to_string(),
            files_changed: Vec::new(),
        });
    }
    set_config_planner_context_compact(paths)?;
    Ok(EfficiencySelfHealAction {
        kind: "compact_planner_context".to_string(),
        status: "applied".to_string(),
        summary: "set planner_context to compact after prompt-bloat alert".to_string(),
        files_changed: vec![display_path(paths, &paths.config)],
    })
}

fn set_config_planner_context_compact(paths: &Paths) -> Result<()> {
    let mut text = read_text(&paths.config)?;
    let mut replaced = false;
    let mut lines = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("planner_context") {
            let indent = &line[..line.len() - trimmed.len()];
            lines.push(format!("{indent}planner_context = \"compact\""));
            replaced = true;
        } else {
            lines.push(line.to_string());
        }
    }
    text = lines.join("\n");
    if !text.ends_with('\n') {
        text.push('\n');
    }
    if !replaced {
        text.push_str("planner_context = \"compact\"\n");
    }
    write_text(&paths.config, &text)
}

fn self_heal_suppress_stale_verification_hints(paths: &Paths) -> Result<EfficiencySelfHealAction> {
    let now = Utc::now().to_rfc3339();
    let mut suppressed = read_suppressed_verification_hints(paths).unwrap_or_default();
    let mut by_fingerprint = suppressed
        .hints
        .into_iter()
        .map(|hint| (hint.semantic_fingerprint.clone(), hint))
        .collect::<BTreeMap<_, _>>();
    let stale = failed_observation_hint_commands(paths);
    let mut added = 0usize;
    for command in stale {
        let fingerprint = command.semantic_fingerprint.clone();
        if by_fingerprint.contains_key(&fingerprint) {
            continue;
        }
        by_fingerprint.insert(
            fingerprint.clone(),
            SuppressedVerificationHint {
                command_display: command.command_display,
                semantic_fingerprint: fingerprint,
                source: command.source,
                task_id: command.task_id,
                category: command.category,
                suppressed_at: now.clone(),
                reason: "failed as observation-memory verification hint".to_string(),
            },
        );
        added += 1;
    }
    suppressed = SuppressedVerificationHints {
        updated_at: now,
        hints: by_fingerprint.into_values().collect(),
    };
    suppressed.hints.sort_by(|left, right| {
        left.command_display
            .cmp(&right.command_display)
            .then_with(|| left.task_id.cmp(&right.task_id))
    });
    let path = paths
        .observations
        .join("suppressed_verification_hints.json");
    write_text(
        &path,
        &format!("{}\n", serde_json::to_string_pretty(&suppressed)?),
    )?;
    Ok(EfficiencySelfHealAction {
        kind: "suppress_stale_verification_hints".to_string(),
        status: if added == 0 {
            "already_applied".to_string()
        } else {
            "applied".to_string()
        },
        summary: format!(
            "recorded {} stale verification hint suppression(s); {} total",
            added,
            suppressed.hints.len()
        ),
        files_changed: vec![display_path(paths, &path)],
    })
}

fn self_heal_materialize_mechanistic_handoffs(
    paths: &Paths,
    report: &EfficiencyWatchReport,
) -> Result<EfficiencySelfHealAction> {
    let mut candidates = report
        .alerts
        .iter()
        .filter_map(|alert| {
            alert
                .mechanistic_handoff
                .as_ref()
                .map(|handoff| (alert, handoff))
        })
        .map(|(alert, handoff)| MechanisticHandoffCandidate {
            candidate_id: handoff.candidate_id.clone(),
            command: handoff.command.clone(),
            command_fingerprint: handoff.command_fingerprint.clone(),
            source: "watch_alert".to_string(),
            source_log: handoff.source_log.clone(),
            source_task_id: handoff.source_task_id.clone(),
            prompt_tokens: handoff.prompt_tokens,
            observed_duration_ms: handoff.observed_duration_ms,
            receipt_exists: handoff.receipt_exists,
            action: handoff.action.clone(),
            reason: alert.summary.clone(),
            materialized_at: Utc::now().to_rfc3339(),
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        left.command_fingerprint
            .cmp(&right.command_fingerprint)
            .then_with(|| left.source_log.cmp(&right.source_log))
    });
    candidates.dedup_by(|left, right| {
        left.command_fingerprint == right.command_fingerprint && left.source_log == right.source_log
    });
    let report = MechanisticHandoffReport {
        generated_at: Utc::now().to_rfc3339(),
        candidates,
    };
    let path = write_mechanistic_handoff_report(paths, &report)?;
    let materialized = report
        .candidates
        .iter()
        .filter(|candidate| candidate.action == "materialized")
        .count();
    let unsupported = report
        .candidates
        .iter()
        .filter(|candidate| candidate.action == "unsupported_sent_to_orchestrator")
        .count();
    append_event(
        paths,
        OrchestratorEventKind::WatchDebounced,
        None,
        json!({
            "mode": "mechanistic_handoffs_materialized",
            "candidate_count": report.candidates.len(),
            "materialized": materialized,
            "unsupported": unsupported,
            "report": display_path(paths, &path),
        }),
    )?;
    Ok(EfficiencySelfHealAction {
        kind: "mechanistic_handoff".to_string(),
        status: if materialized > 0 {
            "applied".to_string()
        } else {
            "already_applied".to_string()
        },
        summary: format!(
            "recorded {} mechanistic handoff candidate(s); {} safe materialized, {} unsupported for orchestrator repair",
            report.candidates.len(),
            materialized,
            unsupported
        ),
        files_changed: vec![display_path(paths, &path)],
    })
}

fn write_mechanistic_handoff_report(
    paths: &Paths,
    report: &MechanisticHandoffReport,
) -> Result<PathBuf> {
    fs::create_dir_all(&paths.observations)?;
    let json_path = paths.observations.join("mechanistic_handoffs.json");
    let md_path = paths.observations.join("mechanistic_handoffs.md");
    write_text(
        &json_path,
        &format!("{}\n", serde_json::to_string_pretty(report)?),
    )?;
    write_text(&md_path, &render_mechanistic_handoff_report(report))?;
    Ok(md_path)
}

fn render_mechanistic_handoff_report(report: &MechanisticHandoffReport) -> String {
    let mut out = String::new();
    out.push_str("# Mechanistic Handoffs\n\n");
    out.push_str(&format!("- Generated at: `{}`\n", report.generated_at));
    out.push_str(&format!("- Candidates: `{}`\n\n", report.candidates.len()));
    if report.candidates.is_empty() {
        out.push_str("No mechanistic handoff candidates were available.\n");
        return out;
    }
    out.push_str("## Candidates\n\n");
    for candidate in &report.candidates {
        out.push_str(&format!(
            "### {}: {}\n\n",
            candidate.action, candidate.candidate_id
        ));
        out.push_str(&format!("- Command: `{}`\n", candidate.command));
        out.push_str(&format!(
            "- Fingerprint: `{}`\n",
            candidate.command_fingerprint
        ));
        out.push_str(&format!("- Source: `{}`\n", candidate.source));
        if let Some(log) = &candidate.source_log {
            out.push_str(&format!("- Source log: `{log}`\n"));
        }
        if let Some(task_id) = &candidate.source_task_id {
            out.push_str(&format!("- Source task: `{task_id}`\n"));
        }
        if let Some(tokens) = candidate.prompt_tokens {
            out.push_str(&format!("- Prompt tokens: `{tokens}`\n"));
        }
        if let Some(duration_ms) = candidate.observed_duration_ms {
            out.push_str(&format!("- Observed duration ms: `{duration_ms}`\n"));
        }
        out.push_str(&format!(
            "- Matching Rust receipt exists: `{}`\n",
            candidate.receipt_exists
        ));
        out.push_str(&format!("- Reason: {}\n\n", candidate.reason));
    }
    out.push_str("Watcher self-heal records routing candidates only; Rust evidence jobs run through the normal step router.\n");
    out
}

fn write_efficiency_self_heal_report(
    paths: &Paths,
    report: &EfficiencySelfHealReport,
) -> Result<PathBuf> {
    fs::create_dir_all(&paths.observations)?;
    let report_path = paths.observations.join("efficiency_self_heal.md");
    write_text(&report_path, &render_efficiency_self_heal_report(report))?;
    let json_path = paths.observations.join("efficiency_self_heal.json");
    write_text(
        &json_path,
        &format!("{}\n", serde_json::to_string_pretty(report)?),
    )?;
    Ok(report_path)
}

fn render_efficiency_self_heal_report(report: &EfficiencySelfHealReport) -> String {
    let mut out = String::new();
    out.push_str("# Efficiency Self-Heal\n\n");
    out.push_str(&format!("- Generated at: `{}`\n\n", report.generated_at));
    if report.actions.is_empty() {
        out.push_str("No safe automatic action was available.\n");
        return out;
    }
    out.push_str("## Actions\n\n");
    for action in &report.actions {
        out.push_str(&format!(
            "### {}: {}\n\n{}\n\n",
            action.status, action.kind, action.summary
        ));
        if !action.files_changed.is_empty() {
            out.push_str("Files changed:\n");
            for file in &action.files_changed {
                out.push_str(&format!("- `{file}`\n"));
            }
            out.push('\n');
        }
    }
    out.push_str("Self-heal is limited to Rust-owned orchestrator config and observation data.\n");
    out
}

fn write_efficiency_watch_report(paths: &Paths, report: &EfficiencyWatchReport) -> Result<PathBuf> {
    fs::create_dir_all(&paths.observations)?;
    if let Ok(route_report) = build_route_regret_report(paths) {
        let _ = write_route_regret_report(paths, &route_report);
    }
    let _ = write_tool_health_summary_report(paths);
    let report_path = paths.observations.join("efficiency_watch.md");
    write_text(&report_path, &render_efficiency_watch_report(report))?;
    let json_path = paths.observations.join("efficiency_watch.json");
    write_text(
        &json_path,
        &format!("{}\n", serde_json::to_string_pretty(report)?),
    )?;
    Ok(report_path)
}

fn write_tool_health_summary_report(paths: &Paths) -> Result<()> {
    let records = read_model_call_records(paths).unwrap_or_default();
    let report = ToolHealthSummaryReport {
        generated_at: Utc::now().to_rfc3339(),
        cooldowns: current_tool_cooldowns(paths, &records),
    };
    let json_path = paths.observations.join("tool_health.json");
    let md_path = paths.observations.join("tool_health.md");
    write_text(
        &json_path,
        &format!("{}\n", serde_json::to_string_pretty(&report)?),
    )?;
    write_text(&md_path, &render_tool_health_summary_report(&report))
}

fn render_tool_health_summary_report(report: &ToolHealthSummaryReport) -> String {
    let mut out = String::new();
    out.push_str("# Tool Health\n\n");
    out.push_str(&format!("- Generated at: `{}`\n", report.generated_at));
    out.push_str(&format!(
        "- Cooldown summaries: `{}`\n\n",
        report.cooldowns.len()
    ));
    if report.cooldowns.is_empty() {
        out.push_str("No recent tool-health failures were classified.\n");
        return out;
    }
    for summary in &report.cooldowns {
        out.push_str(&format!(
            "- `{}` `{}` `{}` failures=`{}` advisory=`{}` last=`{}`",
            normalize_cost_role(&summary.role),
            summary.model,
            summary.failure_kind.as_str(),
            summary.recent_failures,
            summary.cooldown_advisory,
            summary.last_observed_at
        ));
        if !summary.excerpt.is_empty() {
            out.push_str(&format!(
                " excerpt=\"{}\"",
                truncate_inline(&summary.excerpt, 120)
            ));
        }
        out.push('\n');
    }
    out.push_str("\nCooldowns are advisory; they do not suppress role calls by themselves.\n");
    out
}

fn render_efficiency_watch_report(report: &EfficiencyWatchReport) -> String {
    let mut out = String::new();
    out.push_str("# Efficiency Watch\n\n");
    out.push_str(&format!("- Generated at: `{}`\n", report.generated_at));
    out.push_str(&format!(
        "- Task: `{}`\n",
        report.task_id.as_deref().unwrap_or("(auto)")
    ));
    out.push_str(&format!(
        "- Prompt alert threshold: `{} tokens`\n",
        report.prompt_token_threshold
    ));
    out.push_str(&format!(
        "- Watch paths: `{}`\n\n",
        report.watch.paths.len()
    ));
    if let Some(derived) = &report.derived_status {
        out.push_str("## Derived Status\n\n");
        out.push_str(&format!(
            "- Status: `{}`\n- Summary: {}\n",
            derived.status.as_str(),
            derived.summary
        ));
        if !derived.evidence.is_empty() {
            out.push_str("- Evidence: ");
            out.push_str(&derived.evidence.join("; "));
            out.push('\n');
        }
        out.push('\n');
    }
    if report.alerts.is_empty() {
        out.push_str("## Alerts\n\nNo inefficiency alerts detected.\n\n");
    } else {
        out.push_str("## Alerts\n\n");
        for alert in &report.alerts {
            out.push_str(&format!(
                "### {}: {}\n\n{}\n\n",
                alert.severity, alert.kind, alert.summary
            ));
            if !alert.evidence.is_empty() {
                out.push_str("Evidence:\n");
                for item in &alert.evidence {
                    out.push_str(&format!("- {item}\n"));
                }
                out.push('\n');
            }
            if let Some(handoff) = &alert.mechanistic_handoff {
                out.push_str("Mechanistic handoff:\n");
                out.push_str(&format!("- Action: `{}`\n", handoff.action));
                out.push_str(&format!("- Candidate: `{}`\n", handoff.candidate_id));
                out.push_str(&format!("- Command: `{}`\n", handoff.command));
                out.push_str(&format!(
                    "- Fingerprint: `{}`\n",
                    handoff.command_fingerprint
                ));
                out.push_str(&format!(
                    "- Matching Rust receipt exists: `{}`\n",
                    handoff.receipt_exists
                ));
                if let Some(tokens) = handoff.prompt_tokens {
                    out.push_str(&format!("- Prompt tokens: `{tokens}`\n"));
                }
                if let Some(duration_ms) = handoff.observed_duration_ms {
                    out.push_str(&format!("- Observed duration ms: `{duration_ms}`\n"));
                }
                out.push('\n');
            }
        }
    }
    if !report.suggestions.is_empty() {
        out.push_str("## Suggestions\n\n");
        for suggestion in &report.suggestions {
            out.push_str(&format!("- {suggestion}\n"));
        }
        out.push('\n');
    }
    out.push_str("This watcher report is advisory. It does not approve, queue, run, stage, commit, or revert work.\n");
    out
}

#[allow(dead_code)]
fn action_cache_key_for_recipe(
    paths: &Paths,
    recipe: &RecipeSpec,
    relevant_files: &[String],
) -> ActionCacheKey {
    let git_head = git_head(paths);
    let mut file_hashes = relevant_files
        .iter()
        .filter_map(|path| {
            let full_path = paths.root.join(path);
            fingerprint_file(&full_path).ok().map(|fingerprint| {
                (
                    PathBuf::from(path),
                    format!("{:x}", fingerprint.content_hash),
                )
            })
        })
        .collect::<Vec<_>>();
    file_hashes.sort_by(|a, b| a.0.cmp(&b.0));
    ActionCacheKey {
        recipe_fingerprint: stable_hash_hex(&serde_json::to_string(recipe).unwrap_or_default()),
        cwd: recipe.cwd.clone().unwrap_or_else(|| ".".to_string()),
        git_head,
        relevant_file_hashes: file_hashes,
        env_fingerprint: "default-env-v1".to_string(),
    }
}

#[allow(dead_code)]
fn build_repo_index(paths: &Paths, max_files: usize) -> RepoIndex {
    let mut files = Vec::new();
    let mut manifests = Vec::new();
    let mut test_files = Vec::new();
    let mut docs = Vec::new();
    let mut symbols = Vec::new();
    for entry in WalkDir::new(&paths.root)
        .into_iter()
        .filter_entry(|entry| !is_ignored_index_entry(entry.path()))
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_file())
        .take(max_files)
    {
        let Ok(rel) = entry.path().strip_prefix(&paths.root) else {
            continue;
        };
        let rel = rel.to_path_buf();
        let language = language_for_path(&rel).map(str::to_string);
        if is_manifest_path(&rel) {
            manifests.push(rel.clone());
        }
        if rel.components().any(|component| {
            component
                .as_os_str()
                .to_string_lossy()
                .eq_ignore_ascii_case("tests")
        }) || rel
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| name.contains("Test"))
            .unwrap_or(false)
        {
            test_files.push(rel.clone());
        }
        if rel.extension().and_then(|ext| ext.to_str()) == Some("md") {
            docs.push(rel.clone());
        }
        if let Some(language) = language.as_deref() {
            symbols.extend(extract_symbol_entries(entry.path(), &rel, language));
        }
        files.push(FileEntry {
            path: rel,
            language,
        });
    }
    RepoIndex {
        files,
        manifests,
        test_files,
        docs,
        symbols,
    }
}

#[allow(dead_code)]
fn is_ignored_index_entry(path: &Path) -> bool {
    path.components().any(|component| {
        matches!(
            component.as_os_str().to_str(),
            Some(".git" | ".orchestrator" | "target" | ".build" | "node_modules")
        )
    })
}

#[allow(dead_code)]
fn is_manifest_path(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|name| name.to_str()),
        Some("Cargo.toml" | "Package.swift" | "package.json" | "pyproject.toml")
    )
}

#[allow(dead_code)]
fn language_for_path(path: &Path) -> Option<&'static str> {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("rs") => Some("rust"),
        Some("swift") => Some("swift"),
        Some("py") => Some("python"),
        Some("js") | Some("jsx") => Some("javascript"),
        Some("ts") | Some("tsx") => Some("typescript"),
        Some("md") => Some("markdown"),
        _ => None,
    }
}

#[allow(dead_code)]
fn extract_symbol_entries(full_path: &Path, rel_path: &Path, language: &str) -> Vec<SymbolEntry> {
    let text = read_text_or_empty(full_path);
    text.lines()
        .enumerate()
        .filter_map(|(idx, line)| {
            let trimmed = line.trim_start();
            let (kind, rest) = if language == "rust" {
                if let Some(rest) = trimmed.strip_prefix("fn ") {
                    ("fn", rest)
                } else if let Some(rest) = trimmed.strip_prefix("struct ") {
                    ("struct", rest)
                } else {
                    let rest = trimmed.strip_prefix("enum ")?;
                    ("enum", rest)
                }
            } else if language == "swift" {
                if let Some(rest) = trimmed.strip_prefix("func ") {
                    ("func", rest)
                } else if let Some(rest) = trimmed.strip_prefix("struct ") {
                    ("struct", rest)
                } else if let Some(rest) = trimmed.strip_prefix("enum ") {
                    ("enum", rest)
                } else {
                    let rest = trimmed.strip_prefix("class ")?;
                    ("class", rest)
                }
            } else {
                return None;
            };
            let name = rest
                .split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
                .next()
                .unwrap_or("")
                .to_string();
            (!name.is_empty()).then(|| SymbolEntry {
                path: rel_path.to_path_buf(),
                language: language.to_string(),
                kind: kind.to_string(),
                name,
                line: Some(idx as u32 + 1),
            })
        })
        .collect()
}

#[allow(dead_code)]
fn dependency_policy_recipes_for_paths(paths: &[String]) -> Vec<DependencyRecipe> {
    let mut recipes = Vec::new();
    let touches_rust_manifest = paths.iter().any(|path| {
        let lower = path.to_ascii_lowercase();
        lower == "cargo.toml" || lower.ends_with("/cargo.toml") || lower == "cargo.lock"
    });
    if touches_rust_manifest {
        recipes.extend([
            DependencyRecipe::CargoDeny,
            DependencyRecipe::CargoAudit,
            DependencyRecipe::CargoMachete,
            DependencyRecipe::CargoPublicApiDiff,
            DependencyRecipe::CargoMsrvCheck,
        ]);
    }
    recipes.sort_by_key(|recipe| format!("{recipe:?}"));
    recipes.dedup();
    recipes
}

#[allow(dead_code)]
fn workspace_verification_mode_for_diff(diff: &DiffSummary) -> WorkspaceVerificationMode {
    if diff.changed_files.is_empty() {
        WorkspaceVerificationMode::WorkingTree
    } else if diff.touches_source
        || diff.touches_tests
        || diff.touches_dependency_or_lockfile
        || diff.touches_security_sensitive
    {
        WorkspaceVerificationMode::TempWorktreeFromPatch
    } else if diff.touches_docs || diff.touches_tracking_docs {
        WorkspaceVerificationMode::StagedOnly
    } else {
        WorkspaceVerificationMode::WorkingTree
    }
}

#[allow(dead_code)]
fn snapshot_record(snapshot_id: &str, policy: SnapshotPolicy, mut body: Value) -> SnapshotRecord {
    let mut redacted_fields = Vec::new();
    if matches!(policy, SnapshotPolicy::AutoAcceptWithRedactions) {
        redact_unstable_snapshot_fields(&mut body, "$", &mut redacted_fields);
    }
    let semantic_hash = stable_hash_hex(&serde_json::to_string(&body).unwrap_or_default());
    SnapshotRecord {
        snapshot_id: snapshot_id.to_string(),
        policy,
        semantic_hash,
        redacted_fields,
        body,
    }
}

#[allow(dead_code)]
fn redact_unstable_snapshot_fields(value: &mut Value, path: &str, redacted: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            let keys = map.keys().cloned().collect::<Vec<_>>();
            for key in keys {
                let child_path = format!("{path}.{key}");
                if is_unstable_snapshot_key(&key) {
                    map.insert(key, Value::String("<redacted>".to_string()));
                    redacted.push(child_path);
                } else if let Some(child) = map.get_mut(&key) {
                    redact_unstable_snapshot_fields(child, &child_path, redacted);
                }
            }
        }
        Value::Array(items) => {
            for (idx, item) in items.iter_mut().enumerate() {
                redact_unstable_snapshot_fields(item, &format!("{path}[{idx}]"), redacted);
            }
        }
        _ => {}
    }
}

#[allow(dead_code)]
fn is_unstable_snapshot_key(key: &str) -> bool {
    matches!(
        key,
        "timestamp"
            | "started_at"
            | "completed_at"
            | "finished_at"
            | "duration_ms"
            | "run_id"
            | "report_id"
            | "prompt_hash"
            | "context_pack_hash"
    )
}

#[allow(dead_code)]
fn rank_repo_map(index: &RepoIndex, terms: &[String], token_budget: usize) -> RankedRepoMap {
    let normalized_terms = terms
        .iter()
        .map(|term| term.to_ascii_lowercase())
        .filter(|term| !term.trim().is_empty())
        .collect::<Vec<_>>();
    let mut entries_by_path: HashMap<PathBuf, RepoMapEntry> = HashMap::new();
    for file in &index.files {
        let path_text = file.path.to_string_lossy().to_ascii_lowercase();
        let mut score = 0.0;
        let mut why = Vec::new();
        if normalized_terms
            .iter()
            .any(|term| !term.is_empty() && path_text.contains(term))
        {
            score += 2.0;
            why.push(RepoMapReason::MentionsTaskTerm);
        }
        if index.test_files.iter().any(|test| test == &file.path) {
            score += 0.5;
            why.push(RepoMapReason::TestForChangedModule);
        }
        let symbols = index
            .symbols
            .iter()
            .filter(|symbol| symbol.path == file.path)
            .map(|symbol| {
                let symbol_text = symbol.name.to_ascii_lowercase();
                if normalized_terms
                    .iter()
                    .any(|term| symbol_text.contains(term))
                {
                    score += 3.0;
                    if !why.contains(&RepoMapReason::DefinesReferencedSymbol) {
                        why.push(RepoMapReason::DefinesReferencedSymbol);
                    }
                }
                SymbolSummary {
                    kind: symbol.kind.clone(),
                    name: symbol.name.clone(),
                    line: symbol.line,
                }
            })
            .collect::<Vec<_>>();
        if score > 0.0 {
            entries_by_path.insert(
                file.path.clone(),
                RepoMapEntry {
                    path: file.path.clone(),
                    symbols,
                    score,
                    why,
                },
            );
        }
    }
    let mut entries = entries_by_path.into_values().collect::<Vec<_>>();
    entries.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.path.cmp(&b.path))
    });
    let approx_tokens_per_entry = 48usize;
    let max_entries = (token_budget / approx_tokens_per_entry).max(1);
    entries.truncate(max_entries);
    RankedRepoMap {
        entries,
        token_budget,
        ranking_reason: vec![
            "task terms in paths".to_string(),
            "task terms in symbol names".to_string(),
            "test-file convention".to_string(),
        ],
    }
}

fn is_docs_lane_verification_command(command: &str) -> bool {
    let trimmed = command.trim();
    matches!(
        trimmed,
        "git diff --check" | "git status --short" | "git diff --stat"
    ) || trimmed.starts_with("rg ")
}

fn is_deterministic_task_candidate(task: &TaskSpec) -> bool {
    // Deterministic candidates are tracking-doc or docs-only verify/status tasks.
    // Anything that smells like implementation should stay in Worker land.
    if task.tests_to_run.is_empty() {
        return false;
    }
    let tracking_only = task
        .allowed_files
        .iter()
        .all(|path| is_tracking_doc_path(path));
    let docs_verify_only = !task.allowed_files.is_empty()
        && task
            .allowed_files
            .iter()
            .all(|path| is_docs_verify_path(path))
        && task
            .tests_to_run
            .iter()
            .all(|command| is_docs_lane_verification_command(command));
    if !tracking_only && !docs_verify_only {
        return false;
    }
    let text = format!(
        "{}\n{}\n{}\n{}",
        task.kind,
        task.title,
        task.objective,
        task.exact_steps.join("\n")
    )
    .to_ascii_lowercase();
    let looks_mechanical = [
        "mechanical",
        "verification",
        "validate",
        "read-only",
        "read only",
        "docs-only",
        "documentation-only",
        "command-only",
        "status",
    ]
    .iter()
    .any(|needle| text.contains(needle));
    let looks_like_implementation = [
        "implement",
        "add production",
        "wire ",
        "refactor",
        "create source",
        "modify source",
        "edit sources",
        "rewrite ",
        "author ",
    ]
    .iter()
    .any(|needle| text.contains(needle));
    looks_mechanical && !looks_like_implementation
}

fn is_docs_verify_path(path: &str) -> bool {
    if is_tracking_doc_path(path) {
        return true;
    }
    let lower = path.trim_matches('`').to_ascii_lowercase();
    lower == "readme.md" || (lower.starts_with("docs/") && lower.ends_with(".md"))
}

pub(crate) fn is_tracking_doc_path(path: &str) -> bool {
    let path = path.trim_matches('`');
    matches!(
        path,
        "NEXT_STEPS.md"
            | "NEXT_STEPS_non-ai.md"
            | "CONTINUITY.md"
            | "PARITY.md"
            | "TESTING.md"
            | "ARCHITECTURE.md"
            | "QUALITY.md"
            | "AGENTS.md"
            | "TASK.md"
            | "PLAN.md"
            | "ROADMAP.md"
            | "docs/ROADMAP.md"
            | "Docs/ROADMAP.md"
            | "Docs/.INDEX.md"
            | "docs/.INDEX.md"
    ) || path.starts_with("Docs/exec-plans/active/")
        || path.starts_with("docs/exec-plans/active/")
}

fn is_deterministic_command_allowed(command: &str) -> bool {
    parse_verification_command(command, "legacy.string").is_some()
}

#[cfg(test)]
use crate::verification::registry::parse_legacy_recipe_spec;
pub(crate) use crate::verification::registry::{
    is_deterministic_command_structured_allowed, is_local_debug_smoke_command_structured_allowed,
    parse_verification_command, recipe_spec_from_words, recipe_spec_to_command,
};

#[allow(dead_code)]
fn parse_verification_words(parts: Vec<String>, source: &str) -> Option<VerificationCommand> {
    recipe_spec_from_words(parts, source).map(recipe_spec_to_command)
}

#[allow(dead_code)]
fn parse_cargo_json_diagnostics(output: &str, source: FindingSource) -> Vec<DiagnosticFinding> {
    let mut findings = Vec::new();
    for line in output.lines() {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if value.get("reason").and_then(Value::as_str) != Some("compiler-message") {
            continue;
        }
        let Some(message) = value.get("message") else {
            continue;
        };
        let severity = match message.get("level").and_then(Value::as_str).unwrap_or("") {
            "error" => Severity::Error,
            "warning" => Severity::Warning,
            _ => Severity::Info,
        };
        let code = message
            .get("code")
            .and_then(|code| code.get("code"))
            .and_then(Value::as_str)
            .map(str::to_string);
        let rendered = message
            .get("rendered")
            .and_then(Value::as_str)
            .or_else(|| message.get("message").and_then(Value::as_str))
            .unwrap_or("")
            .to_string();
        let span = message
            .get("spans")
            .and_then(Value::as_array)
            .and_then(|spans| {
                spans.iter().find(|span| {
                    span.get("is_primary")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
                })
            })
            .or_else(|| {
                message
                    .get("spans")
                    .and_then(Value::as_array)
                    .and_then(|spans| spans.first())
            });
        findings.push(DiagnosticFinding {
            source: source.clone(),
            severity,
            path: span
                .and_then(|span| span.get("file_name"))
                .and_then(Value::as_str)
                .map(PathBuf::from),
            line: span
                .and_then(|span| span.get("line_start"))
                .and_then(Value::as_u64)
                .map(|line| line as u32),
            column: span
                .and_then(|span| span.get("column_start"))
                .and_then(Value::as_u64)
                .map(|column| column as u32),
            code,
            message: truncate_inline(&rendered, 500),
            raw_excerpt: Some(truncate_inline(&rendered, 500)),
        });
    }
    findings
}

#[allow(dead_code)]
fn default_problem_matchers() -> Vec<ProblemMatcher> {
    vec![
        ProblemMatcher {
            name: "swift-compiler".to_string(),
            pattern: ProblemPattern::SwiftCompiler,
            file_group: Some(1),
            line_group: Some(2),
            column_group: Some(3),
            message_group: 4,
            severity: Severity::Error,
        },
        ProblemMatcher {
            name: "rust-compiler".to_string(),
            pattern: ProblemPattern::RustCompiler,
            file_group: Some(1),
            line_group: Some(2),
            column_group: Some(3),
            message_group: 4,
            severity: Severity::Error,
        },
        ProblemMatcher {
            name: "generic-file-line".to_string(),
            pattern: ProblemPattern::GenericFileLine,
            file_group: Some(1),
            line_group: Some(2),
            column_group: None,
            message_group: 3,
            severity: Severity::Error,
        },
    ]
}

#[allow(dead_code)]
fn match_problem_output(output: &str, matcher: &ProblemMatcher) -> Vec<DiagnosticFinding> {
    output
        .lines()
        .filter_map(|line| match_problem_line(line, matcher))
        .collect()
}

#[allow(dead_code)]
fn match_problem_line(line: &str, matcher: &ProblemMatcher) -> Option<DiagnosticFinding> {
    let captures = match matcher.pattern {
        ProblemPattern::SwiftCompiler => parse_colon_finding(line, true),
        ProblemPattern::RustCompiler => parse_colon_finding(line, false),
        ProblemPattern::GenericFileLine => parse_colon_finding(line, false),
    }?;
    let _configured_groups = (
        matcher.file_group,
        matcher.line_group,
        matcher.column_group,
        matcher.message_group,
    );
    Some(DiagnosticFinding {
        source: FindingSource::ProblemMatcher(matcher.name.clone()),
        severity: matcher.severity,
        path: Some(PathBuf::from(captures.0)),
        line: captures.1,
        column: captures.2,
        code: None,
        message: captures.3,
        raw_excerpt: Some(line.to_string()),
    })
}

#[allow(dead_code)]
fn parse_colon_finding(
    line: &str,
    require_error_word: bool,
) -> Option<(String, Option<u32>, Option<u32>, String)> {
    if require_error_word && !line.contains(": error:") {
        return None;
    }
    let parts = line.splitn(5, ':').collect::<Vec<_>>();
    if parts.len() < 4 {
        return None;
    }
    let file = parts[0].trim();
    let line_no = parts
        .get(1)
        .and_then(|part| part.trim().parse::<u32>().ok());
    let col_no = parts
        .get(2)
        .and_then(|part| part.trim().parse::<u32>().ok());
    let message = if parts.len() >= 5 {
        parts[4].trim()
    } else {
        parts[3].trim()
    };
    if file.is_empty() || message.is_empty() {
        return None;
    }
    Some((file.to_string(), line_no, col_no, message.to_string()))
}

#[allow(dead_code)]
fn parse_unified_diff(diff: &str) -> ParsedDiff {
    let mut files: HashMap<String, BTreeSet<u32>> = HashMap::new();
    let mut current_file: Option<String> = None;
    let mut new_line: Option<u32> = None;
    for line in diff.lines() {
        if let Some(path) = line.strip_prefix("+++ b/") {
            current_file = Some(path.to_string());
            files.entry(path.to_string()).or_default();
            continue;
        }
        if let Some(hunk) = line.strip_prefix("@@ ") {
            if let Some(start) = hunk.split_whitespace().find(|part| part.starts_with('+')) {
                let start = start
                    .trim_start_matches('+')
                    .split(',')
                    .next()
                    .and_then(|num| num.parse::<u32>().ok());
                new_line = start;
            }
            continue;
        }
        let Some(path) = current_file.as_ref() else {
            continue;
        };
        let Some(line_no) = new_line else {
            continue;
        };
        if line.starts_with('+') && !line.starts_with("+++") {
            files.entry(path.clone()).or_default().insert(line_no);
            new_line = line_no.checked_add(1);
        } else if line.starts_with('-') && !line.starts_with("---") {
            // Deletions do not advance the new-file line counter.
        } else {
            new_line = line_no.checked_add(1);
        }
    }
    ParsedDiff { files }
}

#[allow(dead_code)]
fn filter_findings_to_diff(
    findings: Vec<Finding>,
    parsed_diff: &ParsedDiff,
    mode: DiffFilterMode,
) -> FilteredFindings {
    if mode == DiffFilterMode::NoFilter {
        return FilteredFindings {
            in_diff: findings,
            nearby_diff: Vec::new(),
            outside_diff_count: 0,
        };
    }
    let mut in_diff = Vec::new();
    let mut nearby_diff = Vec::new();
    let mut outside_diff_count = 0;
    for finding in findings {
        let Some(path) = finding.path.as_ref().and_then(|path| path.to_str()) else {
            outside_diff_count += 1;
            continue;
        };
        let Some(lines) = parsed_diff.files.get(path) else {
            outside_diff_count += 1;
            continue;
        };
        match mode {
            DiffFilterMode::ChangedFiles => in_diff.push(finding),
            DiffFilterMode::AddedLines => {
                if finding
                    .line
                    .map(|line| lines.contains(&line))
                    .unwrap_or(false)
                {
                    in_diff.push(finding);
                } else {
                    outside_diff_count += 1;
                }
            }
            DiffFilterMode::DiffContext { context_lines } => {
                let near = finding.line.map(|line| {
                    lines.iter().any(|changed| {
                        let delta = (*changed as i64 - line as i64).unsigned_abs() as usize;
                        delta <= context_lines
                    })
                });
                if near == Some(true) {
                    nearby_diff.push(finding);
                } else {
                    outside_diff_count += 1;
                }
            }
            DiffFilterMode::NoFilter => unreachable!(),
        }
    }
    FilteredFindings {
        in_diff,
        nearby_diff,
        outside_diff_count,
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

fn mark_task_status_mechanically(
    queue_path: &Path,
    task: &TaskSpec,
    status: &str,
    commands: &[DeterministicCommandResult],
) -> Result<()> {
    let text = read_text(queue_path)?;
    let mut lines = text.lines().map(str::to_string).collect::<Vec<_>>();
    let Some((start, end)) = next_steps_task_section_bounds(&lines, &task.task_id) else {
        return append_missing_deterministic_task_record(queue_path, task, status, commands);
    };
    let notes = deterministic_execution_notes(commands);
    replace_task_status_in_section(&mut lines, start, end, &format!("- Status: {status}"));

    let insert_at = next_steps_task_section_bounds(&lines, &task.task_id)
        .map(|(_, section_end)| section_end.min(lines.len()))
        .unwrap_or_else(|| end.min(lines.len()));
    lines.splice(insert_at..insert_at, notes);
    let mut updated = lines.join("\n");
    updated.push('\n');
    write_text(queue_path, &updated)
}

fn deterministic_execution_notes(commands: &[DeterministicCommandResult]) -> Vec<String> {
    let mut notes = vec![
        "- Deterministic execution notes:".to_string(),
        "  - Classified and recorded by the Rust deterministic lane without launching Worker or Senior Dev.".to_string(),
    ];
    if commands.is_empty() {
        notes.push("  - No verification commands were listed.".to_string());
    } else {
        notes.push("  - Verification commands:".to_string());
        for result in commands {
            notes.push(format!(
                "    - `{}` exited {} (`{}`): {}.",
                result.command.display(),
                result.exit_code,
                result.category.as_str(),
                result.evidence
            ));
        }
    }
    notes
}

fn append_missing_deterministic_task_record(
    queue_path: &Path,
    task: &TaskSpec,
    status: &str,
    commands: &[DeterministicCommandResult],
) -> Result<()> {
    let mut text = read_text_or_empty(queue_path);
    if !text.ends_with('\n') {
        text.push('\n');
    }
    if text.trim().is_empty() {
        text.push_str("# NEXT_STEPS_non-ai\n\n");
    }
    text.push_str(&format!(
        "\n### {}: {}\n- Task ID: `{}`\n- Status: {status}\n- Task type: Command-only verification.\n- Objective: {}\n",
        task.task_id,
        task.title,
        task.task_id,
        task.objective
    ));
    if let Some(parent) = &task.parent_task {
        text.push_str(&format!("- Parent task: `{parent}`\n"));
    }
    if !task.allowed_files.is_empty() {
        text.push_str("- Allowed files:\n");
        for file in &task.allowed_files {
            text.push_str(&format!("  - `{file}`\n"));
        }
    }
    if !task.tests_to_run.is_empty() {
        text.push_str("- Verification commands:\n");
        for command in &task.tests_to_run {
            text.push_str(&format!("  - `{command}`\n"));
        }
    }
    for note in deterministic_execution_notes(commands) {
        text.push_str(&note);
        text.push('\n');
    }
    write_text(queue_path, &text)
}

fn mark_conditional_task_skipped_by_gate(queue_path: &Path, skip: &ConditionalSkip) -> Result<()> {
    let text = read_text(queue_path)?;
    let mut lines = text.lines().map(str::to_string).collect::<Vec<_>>();
    let Some((start, end)) = next_steps_task_section_bounds(&lines, &skip.task_id) else {
        bail!("could not find NEXT_STEPS.md section for {}", skip.task_id);
    };
    replace_task_status_in_section(&mut lines, start, end, "- Status: Skipped by gate.");

    let notes = vec![
        "- Gate evaluation notes:".to_string(),
        "  - Skipped by Rust deterministic gate without launching Worker, Senior Dev, or Project Manager.".to_string(),
        format!("  - Matched output: {}.", skip.evidence),
    ];
    let insert_at = next_steps_task_section_bounds(&lines, &skip.task_id)
        .map(|(_, section_end)| section_end.min(lines.len()))
        .unwrap_or_else(|| end.min(lines.len()));
    lines.splice(insert_at..insert_at, notes);
    let mut updated = lines.join("\n");
    updated.push('\n');
    write_text(queue_path, &updated)
}

fn markdown_line_has_label(line: &str, label: &str) -> bool {
    let trimmed = line.trim_start();
    let lower = trimmed.to_ascii_lowercase();
    let bullet = format!("- {}:", label.to_ascii_lowercase());
    let bare = format!("{}:", label.to_ascii_lowercase());
    lower.starts_with(&bullet) || lower.starts_with(&bare)
}

fn replace_task_status_in_section(
    lines: &mut Vec<String>,
    start: usize,
    end: usize,
    status_line: &str,
) {
    let mut status_indexes = Vec::new();
    for (idx, line) in lines
        .iter()
        .enumerate()
        .take(end.min(lines.len()))
        .skip(start)
    {
        if markdown_line_has_label(line, "Status") {
            status_indexes.push(idx);
        }
    }
    if status_indexes.is_empty() {
        lines.insert(start + 1, status_line.to_string());
        return;
    }
    let first = status_indexes[0];
    lines[first] = status_line.to_string();
    for &idx in status_indexes.iter().skip(1).rev() {
        lines.remove(idx);
    }
}

fn export_manual_command_scripts(paths: &Paths, task_id: Option<&str>) -> Result<()> {
    let mut tasks = parse_approved_next_steps_tasks(&read_text_or_empty(&paths.next_steps));
    tasks.extend(parse_approved_next_steps_tasks(&read_text_or_empty(
        &paths.next_steps_non_ai,
    )));
    if let Some(task_id) = task_id {
        tasks.retain(|task| task.task_id == task_id);
    }
    if tasks.is_empty() {
        println!("No approved parsed NEXT_STEPS.md tasks with verification commands found.");
        return Ok(());
    }

    let out_dir = paths.orch.join("manual-commands");
    fs::create_dir_all(&out_dir)?;
    let mut index = String::from("# Manual Commands\n\n");
    index.push_str("Generated from approved `NEXT_STEPS.md` tasks. These scripts do not update task status, run models, or commit changes.\n\n");
    let mut written = 0usize;

    for task in tasks {
        if task.tests_to_run.is_empty() {
            continue;
        }
        let script_name = format!("{}.sh", sanitize_filename(&task.task_id));
        let script_path = out_dir.join(&script_name);
        write_text(&script_path, &manual_command_script(paths, &task))?;
        make_executable(&script_path)?;
        written += 1;

        index.push_str(&format!("## {} - {}\n\n", task.task_id, task.title));
        index.push_str(&format!(
            "- Script: `{}`\n",
            display_path(paths, &script_path)
        ));
        index.push_str("- Commands:\n");
        for command in &task.tests_to_run {
            index.push_str(&format!("  - `{}`\n", command));
        }
        index.push('\n');
    }

    let index_path = out_dir.join("README.md");
    write_text(&index_path, &index)?;
    println!(
        "Wrote {written} manual command script(s) to {}",
        display_path(paths, &out_dir)
    );
    println!("Index: {}", display_path(paths, &index_path));
    Ok(())
}

fn manual_command_script(paths: &Paths, task: &TaskSpec) -> String {
    let mut script = String::new();
    script.push_str("#!/usr/bin/env bash\n");
    script.push_str("set -euo pipefail\n\n");
    script.push_str(&format!("# Task: {} - {}\n", task.task_id, task.title));
    script.push_str("# Generated by codex-team. Review before running.\n\n");
    script.push_str(&format!(
        "cd {}\n",
        shell_quote(&paths.root.to_string_lossy())
    ));
    script.push_str("mkdir -p .build/codex-team-caches/clang-module-cache .build/codex-team-caches/swift-module-cache\n");
    script.push_str(
        "export CLANG_MODULE_CACHE_PATH=\"$PWD/.build/codex-team-caches/clang-module-cache\"\n",
    );
    script.push_str(
        "export SWIFT_MODULE_CACHE_PATH=\"$PWD/.build/codex-team-caches/swift-module-cache\"\n\n",
    );
    for command in &task.tests_to_run {
        script.push_str("printf '\\n$ %s\\n' ");
        script.push_str(&shell_quote(command));
        script.push('\n');
        script.push_str(command);
        script.push('\n');
    }
    script
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(perms.mode() | 0o755);
    fs::set_permissions(path, perms)?;
    Ok(())
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<()> {
    Ok(())
}

fn validate_foreman_review_for_task(
    paths: &Paths,
    task: &TaskSpec,
    review: &mut SeniorDevReviewOutput,
) {
    if review.task_id != task.task_id {
        let summary = format!(
            "Senior Dev review task mismatch: selected task was `{}`, but review reported `{}`. Rejecting review to prevent cross-task acceptance.",
            task.task_id, review.task_id
        );
        review.accepted = false;
        review.commit_created = false;
        review.needs_master = false;
        review.needs_planner = true;
        review.project_complete = false;
        review.problems.push(summary.clone());
        review.warnings.push(summary.clone());
        review.summary = summary;
        return;
    }

    let _ = paths;
}

fn apply_senior_repair_decision(
    paths: &Paths,
    state: &mut State,
    task: &TaskSpec,
    repair: SeniorRepairOutput,
    mut retry_brief: WorkerRetryBrief,
) -> Result<()> {
    // Apply only routing, never acceptance.  Repair mode is deliberately unable
    // to mark a task done or commit, which keeps incomplete work out of history.
    if repair.task_id != task.task_id {
        let summary = format!(
            "Senior repair task mismatch: selected task was `{}`, but repair reported `{}`. Routing to Planner.",
            task.task_id, repair.task_id
        );
        state.planner_due = true;
        state.last_review_summary = Some(summary.clone());
        push_event(state, "foreman-repair", "task-mismatch", &summary);
        append_event(
            paths,
            OrchestratorEventKind::StateTransitionApplied,
            Some(&task.task_id),
            json!({"route": "planner_due", "reason": "senior_repair_task_mismatch"}),
        )?;
        return Ok(());
    }

    state.last_review_summary = Some(repair.summary.clone());
    push_event(
        state,
        "foreman-repair",
        repair.reason_code.as_str(),
        &repair.summary,
    );
    append_event(
        paths,
        OrchestratorEventKind::StateTransitionApplied,
        Some(&task.task_id),
        json!({
            "route": format!("{:?}", repair.decision),
            "reason": repair.reason_code.clone(),
            "mode": "senior_repair",
        }),
    )?;

    match repair.decision {
        SeniorRepairDecision::RetryWorkerSameTask => {
            retry_brief.route = WorkerNonDoneRoute::RetryWorkerSameTask.as_str().to_string();
            let previous_instruction = retry_brief.repair_instruction.clone();
            retry_brief.repair_instruction = format!(
                "SENIOR REPAIR DIRECTIVE: {} Rust retry context: {}",
                repair.repair_brief.trim(),
                previous_instruction.trim()
            );
            retry_brief.blocker = format!(
                "Senior repair decision: {}. {}",
                repair.summary.trim(),
                repair.repair_brief.trim()
            );
            if !state
                .queue
                .iter()
                .any(|queued| queued.task_id == task.task_id)
            {
                state.queue.insert(0, task.clone());
            }
            state.retry_worker_same_task = Some(task.task_id.clone());
            state.retry_worker_brief = Some(annotate_retry_brief_progress(state, retry_brief));
            state.senior_dev_repair_order = None;
            state.planner_due = false;
            state.master_due = false;
        }
        SeniorRepairDecision::PlannerDue => {
            state.retry_worker_same_task = None;
            state.retry_worker_brief = None;
            state.senior_dev_repair_order = None;
            state.planner_due = true;
        }
        SeniorRepairDecision::CeoDue => {
            state.retry_worker_same_task = None;
            state.retry_worker_brief = None;
            state.senior_dev_repair_order = None;
            state.master_due = true;
        }
        SeniorRepairDecision::AbandonTask => {
            state.retry_worker_same_task = None;
            state.retry_worker_brief = None;
            state.senior_dev_repair_order = None;
            state.planner_due = false;
            state.master_due = false;
        }
    }
    if repair.needs_planner {
        state.planner_due = true;
    }
    if repair.needs_master {
        state.master_due = true;
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum ContinueChoice {
    Once,
    Auto,
    Stop,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum HumanDispositionChoice {
    PreserveAndStop,
    Recheck,
    ApproveGate,
    Stop,
}

fn prompt_continue_after_cycle(cycle: usize, state: &State) -> Result<ContinueChoice> {
    // Interactive loop brake.  `run --auto-continue` bypasses this, but the
    // default is one visible job at a time so the human can steer long sessions.
    println!();
    println!(
        "[Team] Finished cycle {cycle}. completed={} queued={} retry_worker={} senior_repair={} senior_batch={} ceo_due={} pm_due={} reconciliation={}",
        state.completed_tasks,
        state.queue.len(),
        state.retry_worker_same_task.as_deref().unwrap_or("none"),
        state.senior_dev_repair_order.as_deref().unwrap_or("none"),
        state
            .senior_batch_approval
            .as_ref()
            .map(|batch| batch.batch_id.as_str())
            .unwrap_or("none"),
        state.master_due,
        state.planner_due,
        state.needs_reconciliation
    );
    print!("[Team] Continue to next job? [Enter/y] one more, [a] auto-continue, [n] stop: ");
    io::stdout().flush()?;

    let mut input = String::new();
    if io::stdin().read_line(&mut input)? == 0 {
        println!();
        println!("[Team] No prompt input available; stopping. Use `--auto-continue` for unattended runs.");
        return Ok(ContinueChoice::Stop);
    }
    Ok(parse_continue_choice(input.trim()))
}

fn prompt_human_disposition_choice(
    pause: &HumanDispositionPause,
) -> Result<HumanDispositionChoice> {
    println!();
    println!("[Team] Human input required before orchestration continues.");
    println!("[Team] Reason: {}", truncate_inline(&pause.reason, 320));
    println!("[Team] Choose disposition:");
    println!("  [p] preserve/quarantine dirty set as user-owned context and stop");
    println!("  [r] I handled it externally; re-check queues/tree with PM");
    println!("  [g] approve creating an explicit safety/approval gate");
    println!("  [n] stop and keep this pause active");
    print!("[Team] Disposition? [p/r/g/n]: ");
    io::stdout().flush()?;

    let mut input = String::new();
    if io::stdin().read_line(&mut input)? == 0 {
        println!();
        println!("[Team] No prompt input available; keeping the human-disposition pause active.");
        return Ok(HumanDispositionChoice::Stop);
    }
    Ok(parse_human_disposition_choice(input.trim()))
}

fn parse_human_disposition_choice(input: &str) -> HumanDispositionChoice {
    let answer = input.trim().to_ascii_lowercase();
    match answer.as_str() {
        "p" | "preserve" | "quarantine" => HumanDispositionChoice::PreserveAndStop,
        "r" | "recheck" | "handled" | "done" => HumanDispositionChoice::Recheck,
        "g" | "gate" | "approve" | "approval" => HumanDispositionChoice::ApproveGate,
        "n" | "no" | "q" | "quit" | "stop" | "keep-paused" | "keep_paused" | "" => {
            HumanDispositionChoice::Stop
        }
        other => {
            println!("[Team] Unrecognized disposition `{other}`; keeping pause active.");
            HumanDispositionChoice::Stop
        }
    }
}

fn apply_human_disposition_choice(
    paths: &Paths,
    state: &mut State,
    choice: HumanDispositionChoice,
    pause: &HumanDispositionPause,
) -> Result<bool> {
    match choice {
        HumanDispositionChoice::Stop => {
            println!("[Team] Keeping human-disposition pause active.");
            Ok(false)
        }
        HumanDispositionChoice::PreserveAndStop => {
            record_human_disposition_decision(
                paths,
                state,
                "preserve_quarantine_stop",
                "Human chose to preserve/quarantine the dirty set as user-owned context and stop.",
            )?;
            state.human_disposition_required = None;
            state.master_due = false;
            state.planner_due = false;
            state.needs_reconciliation = false;
            println!("[Team] Recorded preserve/quarantine disposition; stopping without PM/CEO.");
            Ok(false)
        }
        HumanDispositionChoice::Recheck => {
            record_human_disposition_decision(
                paths,
                state,
                "recheck_after_external_handling",
                "Human says the disposition was handled externally; PM may re-check queues/tree.",
            )?;
            state.human_disposition_required = None;
            state.master_due = false;
            state.planner_due = true;
            state.needs_reconciliation = false;
            println!("[Team] Recorded external handling; PM will re-check on the next cycle.");
            Ok(true)
        }
        HumanDispositionChoice::ApproveGate => {
            record_human_disposition_decision(
                paths,
                state,
                "approve_explicit_gate",
                &format!(
                    "Human approved creating an explicit safety/approval gate for this pause. Original reason: {}",
                    truncate_inline(&pause.reason, 500)
                ),
            )?;
            state.human_disposition_required = None;
            state.master_due = false;
            state.planner_due = true;
            state.needs_reconciliation = false;
            println!("[Team] Recorded gate approval; PM will draft/queue an explicit gate next.");
            Ok(true)
        }
    }
}

fn record_human_disposition_decision(
    paths: &Paths,
    state: &mut State,
    decision: &str,
    summary: &str,
) -> Result<()> {
    state.human_disposition_decision = Some(HumanDispositionDecision {
        decision: decision.to_string(),
        summary: summary.to_string(),
        decided_at: Utc::now().to_rfc3339(),
    });
    push_event(state, "human", decision, summary);
    append_event(
        paths,
        OrchestratorEventKind::StateTransitionApplied,
        None,
        json!({
            "mode": "human_disposition",
            "decision": decision,
            "summary": summary,
        }),
    )
}

fn parse_continue_choice(input: &str) -> ContinueChoice {
    let answer = input.trim().to_ascii_lowercase();
    match answer.as_str() {
        "" | "y" | "yes" => ContinueChoice::Once,
        "a" | "auto" | "always" => ContinueChoice::Auto,
        "n" | "no" | "q" | "quit" | "stop" => ContinueChoice::Stop,
        other => {
            println!("[Team] Unrecognized answer `{other}`; stopping to be safe.");
            ContinueChoice::Stop
        }
    }
}

fn next_steps_done(paths: &Paths, state: &State) -> Result<bool> {
    Ok(state.queue.is_empty()
        && !next_steps_has_unfinished_task(paths)?
        && !next_steps_non_ai_has_unfinished_task(paths))
}

fn should_stop_for_no_work(paths: &Paths, state: &State) -> Result<bool> {
    Ok(state.project_complete
        && next_steps_done(paths, state)?
        && !read_next_steps_queue_state(paths).future_work
        && !read_next_steps_non_ai_queue_state(paths).future_work
        && !state.master_due
        && !state.planner_due
        && !state.needs_reconciliation
        && state.retry_worker_same_task.is_none()
        && state.senior_dev_repair_order.is_none()
        && state.senior_batch_approval.is_none()
        && state.human_disposition_required.is_none())
}

fn no_work_cycle_outcome(state: &State) -> &'static str {
    if state.pending_commit.is_some() {
        "paused"
    } else {
        "no_work"
    }
}

fn next_steps_has_unfinished_task(paths: &Paths) -> Result<bool> {
    Ok(read_next_steps_queue_state(paths).unfinished)
}

fn next_steps_non_ai_has_unfinished_task(paths: &Paths) -> bool {
    read_next_steps_non_ai_queue_state(paths).unfinished
}

fn read_next_steps_queue_state(paths: &Paths) -> NextStepsQueueState {
    let text = read_text_or_empty(&paths.next_steps);
    let mut state = next_steps_state_from_text(&text);
    if !parse_approved_next_steps_tasks(&text).is_empty() {
        state.approved = true;
        state.unfinished = true;
    }
    state
}

fn read_next_steps_non_ai_queue_state(paths: &Paths) -> NextStepsQueueState {
    let text = read_text_or_empty(&paths.next_steps_non_ai);
    next_steps_state_from_text(&text)
}

// ---------------------------------------------------------------------------
// Markdown queue status mutators and recovery helpers
// ---------------------------------------------------------------------------
//
// NEXT_STEPS.md remains editable by humans and models, so parsing is tolerant.
// Rust only acts on clearly labeled sections/status fields; ambiguous prose is
// left for Planner rather than guessed into executable work. Status updates use
// exact task-ID section bounds and collapse to a single `- Status:` line.

fn reopen_if_next_steps_unfinished(paths: &Paths, state: &mut State) -> Result<()> {
    let next_steps = read_next_steps_queue_state(paths);
    let non_ai = read_next_steps_non_ai_queue_state(paths);
    if state.project_complete && (next_steps.unfinished || non_ai.unfinished) {
        println!(
            "[Team] State says complete, but NEXT_STEPS has unfinished active work. Reopening."
        );
        state.project_complete = false;
        state.master_due = false;
        state.needs_reconciliation = true;
    } else if state.project_complete && (next_steps.future_work || non_ai.future_work) {
        println!(
            "[Team] State says complete, but NEXT_STEPS has future draft/blocked handoff work. Reopening for planning."
        );
        state.project_complete = false;
        state.master_due = false;
        state.planner_due = true;
    }
    Ok(())
}

fn settle_idle_escalation_if_no_active_work(paths: &Paths, state: &mut State) -> Result<bool> {
    if !state.project_complete {
        return Ok(false);
    }
    if !paths.next_steps.exists()
        || !state.queue.is_empty()
        || state.senior_batch_approval.is_some()
        || state.recent_events.is_empty()
    {
        return Ok(false);
    }

    let next_steps = read_next_steps_queue_state(paths);
    if next_steps.approved || next_steps.unfinished {
        return Ok(false);
    }

    let had_pending_loop_flag = state.master_due || state.planner_due || state.needs_reconciliation;
    if had_pending_loop_flag {
        state.master_due = false;
        state.planner_due = false;
        state.needs_reconciliation = false;
    }
    Ok(had_pending_loop_flag)
}

fn apply_non_master_project_complete_claim(
    paths: &Paths,
    state: &mut State,
    role: &str,
    project_complete: bool,
    summary: &str,
) -> Result<()> {
    if !project_complete {
        state.project_complete = false;
        return Ok(());
    }

    // Mission completion is strategic. PM can say "this slice is done"; Senior
    // can say "this batch is technically safe"; neither should close the
    // top-level mission. Route closure claims to Master/CEO so broad goals do
    // not collapse into "the current queue is empty."
    state.project_complete = false;
    state.master_due = true;
    let gate_summary = format!(
        "{role} claimed mission completion, but Rust requires Master/CEO confirmation before setting COMPLETE=yes. Claim: {}",
        truncate_inline(summary, 260)
    );
    println!(
        "[Rust] {role} claimed mission complete; requiring Master/CEO confirmation before COMPLETE=yes."
    );
    push_event(state, "completion-gate", "needs-master", &gate_summary);
    append_event(
        paths,
        OrchestratorEventKind::StateTransitionApplied,
        None,
        json!({
            "transition": "project_complete_claim_gated",
            "role": role,
            "routed_to": "master",
            "summary": summary,
        }),
    )?;
    Ok(())
}

fn pending_task_ready_for_rust_approval(task: &TaskSpec) -> bool {
    if task.requires_network
        || task.allowed_files.is_empty()
        || task.forbidden_actions.is_empty()
        || task.exact_steps.is_empty()
        || task.success_criteria.is_empty()
        || task.tests_to_run.is_empty()
        || task.stop_conditions.is_empty()
    {
        return false;
    }
    task.tests_to_run
        .iter()
        .all(|command| is_deterministic_command_allowed(command))
}

fn approve_pending_task_in_next_steps(paths: &Paths, task: &TaskSpec) -> Result<()> {
    let text = read_text(&paths.next_steps)?;
    let mut lines = text.lines().map(str::to_string).collect::<Vec<_>>();
    let Some((start, end)) = next_steps_task_section_bounds(&lines, &task.task_id) else {
        bail!(
            "could not find NEXT_STEPS.md section for pending task {}",
            task.task_id
        );
    };
    replace_task_status_in_section(&mut lines, start, end, "- Status: Approved.");
    let mut updated = lines.join("\n");
    updated.push('\n');
    write_text(&paths.next_steps, &updated)
}

// ---------------------------------------------------------------------------
// AI role invocation and compact context construction
// ---------------------------------------------------------------------------
//
// Role functions prepare prompts/schemas, call Codex, and parse structured JSON.
// They should not mutate authoritative task state except through explicit
// state-transition helpers such as Rust acceptance or repair routing.

struct RoleRunners<'a> {
    codex: &'a dyn CodexRunner,
    ca: &'a dyn CaRunner,
}

struct SanitizedRoleFailure {
    kind: ToolFailureKind,
    timed_out: bool,
    summary: String,
}

fn sanitized_role_failure(provider: RoleProvider, error: &anyhow::Error) -> SanitizedRoleFailure {
    let detail = format!("{error:#}");
    let lower = detail.to_ascii_lowercase();
    let timed_out = lower.contains("timed out") || lower.contains("timeout");
    let provider_disabled = lower.contains("enabled is false");
    let kind = if provider_disabled {
        ToolFailureKind::ModelUnavailable
    } else if lower.contains("cancelled") || lower.contains("canceled") {
        ToolFailureKind::Cancelled
    } else {
        match classify_tool_failure(None, timed_out, &detail) {
            ToolFailureKind::Unknown => ToolFailureKind::ProcessFailed,
            kind => kind,
        }
    };
    SanitizedRoleFailure {
        kind,
        timed_out,
        summary: if provider_disabled {
            format!(
                "{} role execution failed: provider enabled is false (failure_kind={})",
                provider.as_str(),
                kind.as_str()
            )
        } else {
            format!(
                "{} role execution failed (failure_kind={}, timed_out={timed_out}); provider payload omitted",
                provider.as_str(),
                kind.as_str()
            )
        },
    }
}

#[allow(clippy::too_many_arguments)]
fn run_role(
    paths: &Paths,
    cfg: &Config,
    runner: &dyn CodexRunner,
    authority: CaRoleAuthority<'_>,
    role_name: &str,
    role: &RoleConfig,
    task_id: Option<&str>,
    prompt: &str,
    schema_path: &Path,
) -> Result<String> {
    let ca_runner = RealCaRunner;
    run_role_with_runners(
        paths,
        cfg,
        &RoleRunners {
            codex: runner,
            ca: &ca_runner,
        },
        authority,
        role_name,
        role,
        task_id,
        prompt,
        schema_path,
    )
}

#[allow(clippy::too_many_arguments)]
fn run_role_with_runners(
    paths: &Paths,
    cfg: &Config,
    runners: &RoleRunners<'_>,
    authority: CaRoleAuthority<'_>,
    role_name: &str,
    role: &RoleConfig,
    task_id: Option<&str>,
    prompt: &str,
    schema_path: &Path,
) -> Result<String> {
    ensure_role_runtime_authority(
        role.provider,
        &role.sandbox,
        &authority,
        std::env::var_os(MCP_JOB_ID_ENV).is_some(),
    )?;
    let effective_provider = if role.provider == RoleProvider::Ca {
        cfg.ca.route_provider.as_str()
    } else {
        role.provider.as_str()
    };
    let learned = if cfg.self_improvement.enabled
        && cfg
            .self_improvement
            .providers
            .iter()
            .any(|provider| provider == effective_provider)
    {
        crate::self_improvement::learned_prompt_context_if_enrolled(
            effective_provider,
            &role.model,
            role_name,
        )?
    } else {
        None
    };
    let learned_prompt = learned.as_ref().map(|context| {
        format!(
            "{prompt}\n\n# Machine-learned context\n{}",
            context.prompt_fragment
        )
    });
    let prompt = learned_prompt.as_deref().unwrap_or(prompt);
    let _foreground_learning_lease = if cfg.self_improvement.enabled {
        match crate::self_improvement::begin_foreground_learning_lease_if_enrolled(
            effective_provider,
            &role.model,
            role_name,
        ) {
            Ok(lease) => lease,
            Err(error) => {
                eprintln!("[learning] foreground lease unavailable; background worker will fail closed: {error:#}");
                None
            }
        }
    } else {
        None
    };
    let started_at = Utc::now().to_rfc3339();
    let timer = Instant::now();
    let input_tokens = estimate_tokens(prompt);
    let result = match role.provider {
        RoleProvider::Codex => runners.codex.run_codex(
            paths,
            &cfg.codex_bin,
            build_codex_run_request(paths, cfg, role_name, role, task_id, prompt, schema_path),
        ),
        RoleProvider::Ollama => crate::providers::ollama::run_ollama_role(
            paths,
            cfg,
            crate::providers::ollama::OllamaRoleRequest {
                role_name,
                role,
                task_id,
                prompt,
                schema_path,
            },
        ),
        RoleProvider::MlxLm => crate::providers::mlx_lm::run_mlx_lm_role(
            paths,
            cfg,
            crate::providers::mlx_lm::MlxLmRoleRequest {
                role_name,
                role,
                task_id,
                prompt,
                schema_path,
            },
        ),
        RoleProvider::Ca => {
            cfg.validate_ca()?;
            let bin = fs::canonicalize(&cfg.ca.bin)
                .with_context(|| format!("canonicalize [ca].bin {}", cfg.ca.bin))?;
            let workspace = fs::canonicalize(&paths.root).with_context(|| {
                format!("canonicalize target workspace {}", paths.root.display())
            })?;
            if bin.starts_with(&workspace) {
                bail!(
                    "[ca].bin must be outside the governed workspace so Worker builds cannot replace the authority backend"
                )
            }
            runners.ca.run_ca(
                paths,
                CaProcessConfig {
                    bin: &bin,
                    profile: &cfg.ca.profile,
                    route_provider: &cfg.ca.route_provider,
                },
                CaRunRequest {
                    role_name,
                    role,
                    task_id,
                    prompt,
                    schema_path,
                    authority,
                },
                &CancellationToken::default(),
            )
        }
    };
    let role_failure = result
        .as_ref()
        .err()
        .map(|error| sanitized_role_failure(role.provider, error));
    let finished_at = Utc::now().to_rfc3339();
    let duration_ms = timer.elapsed().as_millis();
    let (status, output_tokens, outcome, summary) = match &result {
        Ok(output) => (
            RouteLifecycleStatus::Succeeded,
            estimate_tokens(output),
            "succeeded".to_string(),
            format!("{} completed", normalize_cost_role(role_name)),
        ),
        Err(_) => (
            RouteLifecycleStatus::Failed,
            0,
            "failed".to_string(),
            role_failure
                .as_ref()
                .expect("failed role result has sanitized metadata")
                .summary
                .clone(),
        ),
    };
    if role.provider == RoleProvider::Ca {
        let call_reason = match &result {
            Ok(_) => format!(
                "execution_engine=ca route_provider={} role call succeeded",
                cfg.ca.route_provider
            ),
            Err(_) => format!(
                "execution_engine=ca route_provider={} {}",
                cfg.ca.route_provider,
                role_failure
                    .as_ref()
                    .expect("failed ca result has sanitized metadata")
                    .summary
            ),
        };
        let _ = append_model_call_record(
            paths,
            &ModelCallRecord {
                timestamp: finished_at.clone(),
                role: role_name.to_string(),
                task_id: task_id.map(str::to_string),
                model: role.model.clone(),
                execution_engine: Some("ca".to_string()),
                route_provider: Some(cfg.ca.route_provider.clone()),
                effort: role.effort.clone(),
                prompt_token_estimate: input_tokens,
                output_char_count: result.as_ref().map_or(0, |output| output.len()),
                skipped_by_rust: false,
                skip_or_call_reason: call_reason,
            },
        );
        let health = match &result {
            Ok(_) => ToolHealthRecord {
                observed_at: finished_at.clone(),
                role: role_name.to_string(),
                task_id: task_id.map(str::to_string),
                model: role.model.clone(),
                execution_engine: Some("ca".to_string()),
                route_provider: Some(cfg.ca.route_provider.clone()),
                status: ToolHealthStatus::Success,
                failure_kind: None,
                exit_code: Some(0),
                timed_out: false,
                excerpt: String::new(),
            },
            Err(_) => {
                let failure = role_failure
                    .as_ref()
                    .expect("failed ca result has sanitized metadata");
                ToolHealthRecord {
                    observed_at: finished_at.clone(),
                    role: role_name.to_string(),
                    task_id: task_id.map(str::to_string),
                    model: role.model.clone(),
                    execution_engine: Some("ca".to_string()),
                    route_provider: Some(cfg.ca.route_provider.clone()),
                    status: ToolHealthStatus::Failure,
                    failure_kind: Some(failure.kind),
                    exit_code: None,
                    timed_out: failure.timed_out,
                    excerpt: failure.summary.clone(),
                }
            }
        };
        let _ = append_role_tool_health_record(paths, &health);
    }
    let lane = route_lane_for_role(role_name);
    let attempt = RouteAttempt {
        id: route_attempt_id(role_name, task_id, lane),
        task_id: task_id.map(str::to_string),
        attempt: 1,
        status,
        started_at,
        finished_at: Some(finished_at),
        execution_lane: lane,
        route_kind: if role.provider == RoleProvider::Ca {
            format!("{role_name}:engine=ca:route={}", cfg.ca.route_provider)
        } else {
            role_name.to_string()
        },
        model_tier: Some(route_role_tier(role_name).to_string()),
        model: Some(role.model.clone()),
        prompt_receipt_id: None,
        evidence_receipt_id: None,
        failure_fingerprint: None,
        token_usage: TokenUsage::new(input_tokens, output_tokens, 0, 0, 0),
        scope_group: None,
        scope_overlap: false,
        final_feedback: Some(RouteFeedback {
            outcome,
            summary,
            regret_kind: None,
            better_lane: None,
            verification_status: None,
            commit_sha: None,
            duration_ms: Some(duration_ms),
        }),
    };
    if let Err(err) = record_route_attempt(paths, &attempt) {
        eprintln!("[route] failed to record route attempt: {err:#}");
    }
    let trace_provider = effective_provider;
    let trace_response = match &result {
        Ok(output) => output.as_str(),
        Err(_) => role_failure
            .as_ref()
            .expect("failed role result has sanitized metadata")
            .summary
            .as_str(),
    };
    if let Err(error) = crate::self_improvement::record_role_trace_if_enrolled(
        trace_provider,
        &role.model,
        role_name,
        prompt,
        trace_response,
        task_id,
        &paths.root.display().to_string(),
        &attempt.started_at,
        attempt
            .finished_at
            .as_deref()
            .unwrap_or(&attempt.started_at),
        duration_ms as u64,
        u64::try_from(input_tokens).unwrap_or(u64::MAX),
        u64::try_from(output_tokens).unwrap_or(u64::MAX),
        attempt
            .final_feedback
            .as_ref()
            .map(|feedback| feedback.outcome.as_str())
            .unwrap_or("unknown"),
    ) {
        eprintln!("[learning] trace capture failed closed for this observation: {error:#}");
    }
    match result {
        Ok(output) => Ok(output),
        Err(_) => Err(anyhow!(
            role_failure
                .expect("failed role result has sanitized metadata")
                .summary
        )),
    }
}

fn ensure_role_runtime_authority(
    provider: RoleProvider,
    sandbox: &str,
    authority: &CaRoleAuthority<'_>,
    mcp_owned_step: bool,
) -> Result<()> {
    if mcp_owned_step && provider == RoleProvider::Codex {
        if matches!(authority, CaRoleAuthority::Worker { .. }) {
            bail!(
                "MCP operate refuses a Codex-backed Worker because its workspace-wide sandbox cannot protect orchestrator runtime authority; configure provider = \"ca\" for the Worker"
            )
        }
        if sandbox != "read-only" {
            bail!("MCP operate requires sandbox = \"read-only\" for Codex-backed advisory roles")
        }
    }
    Ok(())
}

fn build_codex_run_request<'a>(
    paths: &'a Paths,
    cfg: &'a Config,
    role_name: &'a str,
    role: &'a RoleConfig,
    task_id: Option<&'a str>,
    prompt: &'a str,
    schema_path: &'a Path,
) -> CodexRunRequest<'a> {
    let role_config = CodexRoleConfig {
        model: role.model.clone(),
        effort: role.effort.clone(),
        sandbox: role.sandbox.clone(),
        approval_policy: role.approval_policy.clone(),
        web_search: role.web_search.clone(),
        timeout_seconds: role.timeout_seconds,
    };
    let hooks = if cfg.live_output {
        codex_run_hooks(paths, cfg, role_name, role)
    } else {
        CodexRunHooks::default()
    };
    CodexRunRequest {
        role_name,
        role: role_config,
        task_id,
        prompt,
        schema_path,
        live_output: cfg.live_output,
        cancellation: CancellationToken::default(),
        hooks,
    }
}

fn codex_run_hooks<'a>(
    paths: &'a Paths,
    cfg: &'a Config,
    role_name: &'a str,
    role: &'a RoleConfig,
) -> CodexRunHooks<'a> {
    let role_label = role_label(cfg, role_name);
    let role_for_header = role.clone();
    let start_label = role_label.clone();
    let start_style = cfg.live_output_style;
    let start_theme = Theme::new(cfg.live_output_color);
    let root = paths.root.clone();
    let on_start = Box::new(move |start: &CodexRunStart<'_>| {
        let stdout_label = start
            .stdout_path
            .strip_prefix(&root)
            .map(|rel| rel.display().to_string())
            .unwrap_or_else(|_| start.stdout_path.display().to_string());
        match start_style {
            LiveOutputStyle::Boxed => {
                print_role_header(
                    &start_theme,
                    &start_label,
                    &role_for_header,
                    start.prompt_tokens,
                    &stdout_label,
                    start.timeout_seconds,
                );
                println!("{}", start_theme.section(&format!("{start_label} STREAM")));
            }
            LiveOutputStyle::Codex => {
                print_codex_role_header(
                    &start_theme,
                    &start_label,
                    &role_for_header,
                    start.prompt_tokens,
                    &stdout_label,
                );
            }
        }
    });

    let stdout_label = role_label.clone();
    let stdout_root = paths.root.clone();
    let stdout_preview_lines = cfg.live_output_preview_lines;
    let stdout_preview_chars = cfg.live_output_preview_chars;
    let stdout_verbose_success = cfg.live_output_verbose_success;
    let stdout_theme = Theme::new(cfg.live_output_color);
    let stdout_style = cfg.live_output_style;
    let stdout = Box::new(
        move |stdout: std::process::ChildStdout, stdout_path: PathBuf| {
            let raw_log_label = stdout_path
                .strip_prefix(&stdout_root)
                .map(|rel| rel.display().to_string())
                .unwrap_or_else(|_| stdout_path.display().to_string());
            render_jsonl_stream(
                stdout,
                stdout_path,
                stdout_label,
                raw_log_label,
                stdout_preview_lines,
                stdout_preview_chars,
                stdout_verbose_success,
                stdout_theme,
                stdout_style,
            )
        },
    );

    let stderr_label = role_label.clone();
    let stderr_theme = Theme::new(cfg.live_output_color);
    let stderr_style = cfg.live_output_style;
    let stderr = Box::new(
        move |stderr: std::process::ChildStderr, stderr_path: PathBuf| {
            render_stderr_stream(
                stderr,
                stderr_path,
                stderr_label,
                stderr_theme,
                stderr_style,
            )
        },
    );

    let end_label = role_label.clone();
    let end_theme = Theme::new(cfg.live_output_color);
    let end_style = cfg.live_output_style;
    let on_stream_end = Box::new(move || {
        if end_style == LiveOutputStyle::Boxed {
            println!("{}", end_theme.section(&format!("END {end_label} STREAM")));
        }
    });

    let success_label = role_label;
    let success_theme = Theme::new(cfg.live_output_color);
    let success_style = cfg.live_output_style;
    let on_success = Box::new(move |elapsed: Duration| {
        let elapsed = format_duration(elapsed);
        match success_style {
            LiveOutputStyle::Boxed => println!(
                "{} {}",
                success_theme.tag(&success_label),
                success_theme.status(&format!("Codex completed successfully in {elapsed}"), true)
            ),
            LiveOutputStyle::Codex => println!(
                "\n{}",
                success_theme.dim(&format!(
                    "{success_label} completed successfully in {elapsed}"
                ))
            ),
        }
    });

    CodexRunHooks {
        on_start: Some(on_start),
        stdout: Some(stdout),
        stderr: Some(stderr),
        on_stream_end: Some(on_stream_end),
        on_success: Some(on_success),
    }
}

fn run_ceo(
    paths: &Paths,
    cfg: &Config,
    state: &State,
    runner: &dyn CodexRunner,
) -> Result<CeoOutput> {
    let ca_runner = RealCaRunner;
    run_ceo_with_runners(
        paths,
        cfg,
        state,
        &RoleRunners {
            codex: runner,
            ca: &ca_runner,
        },
    )
}

fn run_ceo_with_runners(
    paths: &Paths,
    cfg: &Config,
    state: &State,
    runners: &RoleRunners<'_>,
) -> Result<CeoOutput> {
    let prompt_path = paths.prompts.join("master.md");
    write_text(&prompt_path, MASTER_PROMPT)?;
    let schema_path = paths.schemas.join("master.schema.json");
    write_text(&schema_path, &render_schema(MASTER_SCHEMA))?;
    let prompt = render_prompt(
        &read_text(&prompt_path)?,
        paths,
        state,
        json!({"role": "master", "instruction": "Update ROADMAP.md and optionally NEXT_STEPS.md. Do not implement."}),
    )?;
    let raw = run_role_with_runners(
        paths,
        cfg,
        runners,
        CaRoleAuthority::ReadOnly,
        "master",
        &cfg.roles.master,
        None,
        &prompt,
        &schema_path,
    )?;
    parse_json_response(&raw)
}

fn run_project_manager(
    paths: &Paths,
    cfg: &Config,
    state: &State,
    runner: &dyn CodexRunner,
) -> Result<ProjectManagerOutput> {
    let ca_runner = RealCaRunner;
    run_project_manager_with_runners(
        paths,
        cfg,
        state,
        &RoleRunners {
            codex: runner,
            ca: &ca_runner,
        },
    )
}

fn run_project_manager_with_runners(
    paths: &Paths,
    cfg: &Config,
    state: &State,
    runners: &RoleRunners<'_>,
) -> Result<ProjectManagerOutput> {
    let prompt_path = paths.prompts.join("planner.md");
    write_text(&prompt_path, PLANNER_PROMPT)?;
    let schema_path = paths.schemas.join("planner.schema.json");
    write_text(&schema_path, &render_schema(PLANNER_SCHEMA))?;
    let extra = json!({
        "role": "planner",
        "mode": "compile_work_orders",
        "queue_len": state.queue.len(),
        "planner_context": cfg.planner_context.as_str(),
        "rust_queue_brief": build_planner_queue_brief(paths, state),
    });
    let prompt = if cfg.planner_context == PlannerContextMode::Compact {
        render_project_manager_compact_prompt(&read_text(&prompt_path)?, paths, state, extra)?
    } else {
        render_prompt(&read_text(&prompt_path)?, paths, state, extra)?
    };
    let raw = run_role_with_runners(
        paths,
        cfg,
        runners,
        CaRoleAuthority::ReadOnly,
        "planner",
        &cfg.roles.planner,
        None,
        &prompt,
        &schema_path,
    )?;
    parse_json_response(&raw)
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum PmMutationDisposition {
    None,
    AutoCommitted,
    Blocked,
}

fn run_project_manager_with_mutation_snapshot(
    paths: &Paths,
    cfg: &Config,
    state: &State,
    runner: &dyn CodexRunner,
) -> Result<(ProjectManagerOutput, Vec<String>)> {
    let before = snapshot_repo(paths, cfg)?;
    let plan = run_project_manager(paths, cfg, state, runner)?;
    let after = snapshot_repo(paths, cfg)?;
    Ok((plan, diff_snapshots(&before, &after)))
}

fn handle_pm_mutations_after_plan(
    paths: &Paths,
    cfg: &Config,
    state: &mut State,
    plan: &ProjectManagerOutput,
    changed_files: Vec<String>,
) -> Result<PmMutationDisposition> {
    if changed_files.is_empty() {
        return Ok(PmMutationDisposition::None);
    }
    append_event(
        paths,
        OrchestratorEventKind::PolicyViolation,
        None,
        json!({
            "mode": "pm_mutated_files_detected",
            "changed_files": changed_files.clone(),
            "task_count": plan.tasks.len(),
            "summary": plan.summary.clone(),
        }),
    )?;

    if plan.tasks.is_empty() {
        if let Some(candidate) = planner_zero_work_reconciliation_candidate(paths, plan) {
            if try_auto_commit_pm_control_doc_reconciliation(paths, cfg, state, &candidate)? {
                append_event(
                    paths,
                    OrchestratorEventKind::StateTransitionApplied,
                    Some(&candidate.task_id),
                    json!({
                        "mode": "pm_mutation_auto_committed_control_docs",
                        "candidate_task_id": candidate.task_id,
                    }),
                )?;
                return Ok(PmMutationDisposition::AutoCommitted);
            }
        }
    }

    let summary = format!(
        "Project Manager mutated {} file(s) outside the zero-task control-doc auto-commit lane; stopping before Worker.",
        changed_files.len()
    );
    println!("[Rust] {summary}");
    state.last_review_summary = Some(summary.clone());
    state.last_worker_summary =
        Some("Worker skipped because Project Manager mutated files.".to_string());
    state.planner_due = false;
    state.retry_worker_same_task = None;
    state.retry_worker_brief = None;
    state.senior_dev_repair_order = None;
    push_event(state, "policy", "pm-mutated-files", &summary);
    record_rust_model_skip(
        paths,
        "worker",
        None,
        cfg,
        "pm_mutated_files_stopped_before_worker",
    )?;
    record_rust_model_skip(
        paths,
        "foreman-review",
        None,
        cfg,
        "pm_mutated_files_stopped_before_senior",
    )?;
    Ok(PmMutationDisposition::Blocked)
}

fn normalize_planner_task_control_doc_scopes(
    paths: &Paths,
    state: &mut State,
    plan: &mut ProjectManagerOutput,
) {
    if plan.tasks.is_empty() {
        return;
    }
    for task in &mut plan.tasks {
        if !planner_task_allows_control_doc_scope_inference(task) {
            continue;
        }
        let text = planner_task_scope_text(&plan.summary, &plan.warnings, task);
        let inferred = mentioned_control_doc_paths(paths, &text);
        let mut added = Vec::new();
        for path in inferred {
            if !task.allowed_files.iter().any(|existing| existing == &path) {
                task.allowed_files.push(path.clone());
                added.push(path);
            }
        }
        if !added.is_empty() {
            task.allowed_files.sort();
            task.allowed_files.dedup();
            push_event(
                state,
                "rust",
                "planner-task-scope-normalized",
                &format!(
                    "Added PM-mentioned control docs to {} allowed files: {}.",
                    task.task_id,
                    added.join(", ")
                ),
            );
        }
    }
}

fn planner_task_allows_control_doc_scope_inference(task: &TaskSpec) -> bool {
    if task
        .allowed_files
        .iter()
        .chain(task.read_only_files.iter())
        .any(|path| is_tracking_doc_path(path))
    {
        return true;
    }
    let text = format!(
        "{}\n{}\n{}\n{}\n{}",
        task.kind,
        task.title,
        task.objective,
        task.exact_steps.join("\n"),
        task.success_criteria.join("\n")
    )
    .to_ascii_lowercase();
    [
        "doc",
        "control",
        "reconcile",
        "classify",
        "evidence",
        "report",
        "capture",
        "continuity",
        "parity",
        "execplan",
        "exec plan",
        "next_steps",
    ]
    .iter()
    .any(|needle| text.contains(needle))
}

fn planner_task_scope_text(summary: &str, warnings: &[String], task: &TaskSpec) -> String {
    let mut text = String::new();
    for part in [
        summary,
        task.title.as_str(),
        task.kind.as_str(),
        task.objective.as_str(),
        task.why_unblocks_progress.as_str(),
        task.discovery_budget.as_str(),
    ] {
        text.push_str(part);
        text.push('\n');
    }
    for parts in [
        warnings,
        &task.allowed_files,
        &task.read_only_files,
        &task.exact_steps,
        &task.success_criteria,
        &task.stop_conditions,
    ] {
        for part in parts {
            text.push_str(part);
            text.push('\n');
        }
    }
    text
}

fn persist_planner_work_plan(
    paths: &Paths,
    state: &mut State,
    plan: &ProjectManagerOutput,
) -> Result<()> {
    if plan.work_plan.is_empty() {
        return Ok(());
    }
    fs::create_dir_all(&paths.observations)?;
    let json_path = paths.observations.join("planner_work_plan.json");
    let md_path = paths.observations.join("planner_work_plan.md");
    write_text(
        &json_path,
        &format!("{}\n", serde_json::to_string_pretty(&plan.work_plan)?),
    )?;
    write_text(
        &md_path,
        &render_planner_work_plan_markdown(&plan.work_plan),
    )?;
    push_event(
        state,
        "planner",
        "work-plan",
        &format!(
            "Persisted PM work plan with {} goal(s), {} sequence item(s), and {} test strategy item(s).",
            plan.work_plan.goals.len(),
            plan.work_plan.task_sequence.len(),
            plan.work_plan.test_strategy.len()
        ),
    );
    Ok(())
}

fn render_planner_work_plan_markdown(plan: &PlannerWorkPlan) -> String {
    let mut out = String::new();
    out.push_str("# Project Manager Work Plan\n\n");
    if !plan.summary.trim().is_empty() {
        out.push_str("## Summary\n\n");
        out.push_str(plan.summary.trim());
        out.push_str("\n\n");
    }
    if !plan.goals.is_empty() {
        out.push_str("## Goals\n\n");
        for goal in &plan.goals {
            out.push_str(&format!("- {}\n", goal.trim()));
        }
        out.push('\n');
    }
    if !plan.task_sequence.is_empty() {
        out.push_str("## Task Sequence\n\n");
        for item in &plan.task_sequence {
            out.push_str(&format!(
                "- `{}` [{}] {}",
                item.id.trim(),
                item.status.trim(),
                item.title.trim()
            ));
            if !item.kind.trim().is_empty() {
                out.push_str(&format!(" ({})", item.kind.trim()));
            }
            if item.estimated_minutes > 0 {
                out.push_str(&format!(" - ~{} min", item.estimated_minutes));
            }
            out.push('\n');
            if !item.depends_on.is_empty() {
                out.push_str(&format!("  Depends on: {}\n", item.depends_on.join(", ")));
            }
            if !item.tests.is_empty() {
                out.push_str(&format!("  Tests: {}\n", item.tests.join("; ")));
            }
            if !item.notes.trim().is_empty() {
                out.push_str(&format!("  Notes: {}\n", item.notes.trim()));
            }
        }
        out.push('\n');
    }
    if !plan.test_strategy.is_empty() {
        out.push_str("## Test Strategy\n\n");
        for test in &plan.test_strategy {
            out.push_str(&format!("- {}\n", test.trim()));
        }
        out.push('\n');
    }
    if !plan.pm_return_triggers.is_empty() {
        out.push_str("## Return To PM When\n\n");
        for trigger in &plan.pm_return_triggers {
            out.push_str(&format!("- {}\n", trigger.trim()));
        }
        out.push('\n');
    }
    out
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum WorkerNonDoneRoute {
    // Non-done Worker outcomes are not reviews.  They are routing states.
    RetryWorkerSameTask,
    SeniorDevRepairOrder,
    PlannerDue,
}

impl WorkerNonDoneRoute {
    fn as_str(self) -> &'static str {
        match self {
            Self::RetryWorkerSameTask => "retry_worker_same_task",
            Self::SeniorDevRepairOrder => "senior_dev_repair_order",
            Self::PlannerDue => "planner_due",
        }
    }
}

fn apply_worker_orchestration_request(
    paths: &Paths,
    cfg: &Config,
    state: &mut State,
    task: &TaskSpec,
    worker: &WorkerOutput,
    guard_report: &GuardReport,
) -> Result<Option<String>> {
    let Some(request) = worker.orchestration_request.as_ref() else {
        return Ok(None);
    };
    if worker.status.eq_ignore_ascii_case("done") {
        return Ok(None);
    }
    let reason = clean_orchestration_text(&request.reason, 500);
    let summary_prefix = format!(
        "{} requested Rust orchestration `{}`: {}",
        task.task_id,
        worker_orchestration_kind_label(request.kind),
        reason
    );
    append_event(
        paths,
        OrchestratorEventKind::StateTransitionApplied,
        Some(&task.task_id),
        json!({
            "mode": "worker_orchestration_request",
            "kind": worker_orchestration_kind_label(request.kind),
            "reason": reason,
        }),
    )?;

    match request.kind {
        WorkerOrchestrationKind::SplitTask => {
            let child_tasks = match worker_split_child_tasks(task, request, guard_report) {
                Ok(tasks) => tasks,
                Err(problem) => {
                    expire_senior_batch(
                        paths,
                        state,
                        "worker_orchestration_split_invalid",
                        Some(task),
                    )?;
                    state.planner_due = true;
                    state.retry_worker_same_task = None;
                    state.retry_worker_brief = None;
                    state.senior_dev_repair_order = None;
                    let summary = format!(
                        "{summary_prefix}. Rust rejected the split request and routed to Planner: {problem}"
                    );
                    append_event(
                        paths,
                        OrchestratorEventKind::PolicyViolation,
                        Some(&task.task_id),
                        json!({"mode": "worker_orchestration_split", "problem": problem}),
                    )?;
                    return Ok(Some(summary));
                }
            };
            for child in child_tasks.iter().rev().cloned() {
                enqueue_front_unique(state, child);
            }
            state.planner_due = false;
            state.master_due = false;
            state.retry_worker_same_task = None;
            state.retry_worker_brief = None;
            state.senior_dev_repair_order = None;
            let _ = record_rust_model_skip(
                paths,
                "planner",
                Some(&task.task_id),
                cfg,
                "Worker requested a bounded split that Rust validated",
            );
            let summary = format!(
                "{summary_prefix}. Rust validated and queued {} bounded child task(s).",
                child_tasks.len()
            );
            append_event(
                paths,
                OrchestratorEventKind::StateTransitionApplied,
                Some(&task.task_id),
                json!({
                    "mode": "worker_orchestration_split_applied",
                    "child_task_ids": child_tasks.iter().map(|task| task.task_id.clone()).collect::<Vec<_>>(),
                }),
            )?;
            Ok(Some(summary))
        }
        WorkerOrchestrationKind::RetrySameTask => {
            if !guard_report.violations.is_empty() {
                expire_senior_batch(
                    paths,
                    state,
                    "worker_orchestration_retry_guard_violation",
                    Some(task),
                )?;
                state.planner_due = true;
                return Ok(Some(format!(
                    "{summary_prefix}. Rust refused same-task retry because guard violations exist: {}",
                    guard_report.violations.join(", ")
                )));
            }
            let mut retry_brief = build_worker_retry_brief(
                paths,
                cfg,
                task,
                worker,
                guard_report,
                WorkerRetryOptions {
                    attempt: 1,
                    fingerprint_retry_count: 0,
                    run_diagnostic: false,
                },
            )?;
            retry_brief.route = WorkerNonDoneRoute::RetryWorkerSameTask.as_str().to_string();
            retry_brief.repair_instruction = format!(
                "Worker requested a same-task orchestration retry: {} {}",
                reason, retry_brief.repair_instruction
            );
            enqueue_front_unique(state, task.clone());
            state.retry_worker_same_task = Some(task.task_id.clone());
            state.retry_worker_brief = Some(annotate_retry_brief_progress(state, retry_brief));
            state.planner_due = false;
            state.master_due = false;
            state.senior_dev_repair_order = None;
            Ok(Some(format!(
                "{summary_prefix}. Rust queued one same-task retry with a structured retry brief."
            )))
        }
        WorkerOrchestrationKind::RouteToPlanner => {
            expire_senior_batch(
                paths,
                state,
                "worker_orchestration_planner_route",
                Some(task),
            )?;
            state.planner_due = true;
            state.retry_worker_same_task = None;
            state.retry_worker_brief = None;
            state.senior_dev_repair_order = None;
            Ok(Some(format!(
                "{summary_prefix}. Rust accepted the route-to-Planner request."
            )))
        }
        WorkerOrchestrationKind::ExpandLease => {
            let mut lease = state
                .worker_file_leases
                .remove(&task.task_id)
                .unwrap_or_else(|| {
                    crate::context::lease::FileLeaseSet::for_task(
                        &task.task_id,
                        task.allowed_files.iter().take(2).cloned().collect(),
                    )
                });
            let granted = lease.expand(&request.lease_paths);
            state.worker_file_leases.insert(task.task_id.clone(), lease);
            enqueue_front_unique(state, task.clone());
            state.retry_worker_same_task = Some(task.task_id.clone());
            state.planner_due = false;
            state.master_due = false;
            Ok(Some(format!(
                "{summary_prefix}. Rust granted {} leased file(s) and requeued the Worker: {}",
                granted.len(),
                if granted.is_empty() {
                    "none (cap or path safety blocked expansion)".to_string()
                } else {
                    granted.join(", ")
                }
            )))
        }
    }
}

fn worker_orchestration_kind_label(kind: WorkerOrchestrationKind) -> &'static str {
    match kind {
        WorkerOrchestrationKind::SplitTask => "split_task",
        WorkerOrchestrationKind::RetrySameTask => "retry_same_task",
        WorkerOrchestrationKind::RouteToPlanner => "route_to_planner",
        WorkerOrchestrationKind::ExpandLease => "expand_lease",
    }
}

fn worker_split_child_tasks(
    parent: &TaskSpec,
    request: &WorkerOrchestrationRequest,
    guard_report: &GuardReport,
) -> std::result::Result<Vec<TaskSpec>, String> {
    if !guard_report.violations.is_empty() {
        return Err(format!(
            "guard violations must be resolved before splitting: {}",
            guard_report.violations.join(", ")
        ));
    }
    if request.child_tasks.is_empty() || request.child_tasks.len() > 6 {
        return Err("split_task requires 1..=6 child_tasks".to_string());
    }
    let parent_allowed = compile_globs(&expand_allowed_file_patterns(&parent.allowed_files))
        .map_err(|err| format!("parent allowed-file globs are invalid: {err:#}"))?;
    let read_scope = compile_globs(&expand_allowed_file_patterns(
        &parent
            .allowed_files
            .iter()
            .chain(parent.read_only_files.iter())
            .cloned()
            .collect::<Vec<_>>(),
    ))
    .map_err(|err| format!("parent read scope globs are invalid: {err:#}"))?;
    let mut tasks = Vec::new();
    let mut previous_id: Option<String> = None;
    for (index, child) in request.child_tasks.iter().enumerate() {
        let title = clean_orchestration_text(&child.title, 120);
        let objective = clean_orchestration_text(&child.objective, 700);
        if title.is_empty() || objective.is_empty() {
            return Err(format!(
                "child task {} needs non-empty title and objective",
                index + 1
            ));
        }
        let mut allowed_files = child
            .allowed_files
            .iter()
            .map(|path| path.trim().trim_start_matches("./").to_string())
            .filter(|path| !path.is_empty())
            .collect::<Vec<_>>();
        allowed_files.sort();
        allowed_files.dedup();
        if allowed_files.is_empty() || allowed_files.len() > 20 {
            return Err(format!(
                "child task {} needs 1..=20 allowed files",
                index + 1
            ));
        }
        if let Some(path) = allowed_files
            .iter()
            .find(|path| !parent_allowed.is_match(path.as_str()))
        {
            return Err(format!(
                "child task {} requested file outside parent scope: {}",
                index + 1,
                path
            ));
        }
        let mut read_only_files = child
            .read_only_files
            .iter()
            .map(|path| path.trim().trim_start_matches("./").to_string())
            .filter(|path| !path.is_empty())
            .collect::<Vec<_>>();
        read_only_files.sort();
        read_only_files.dedup();
        if let Some(path) = read_only_files
            .iter()
            .find(|path| !read_scope.is_match(path.as_str()))
        {
            return Err(format!(
                "child task {} requested read-only file outside parent scope: {}",
                index + 1,
                path
            ));
        }
        if child.tests_to_run.is_empty()
            || !child
                .tests_to_run
                .iter()
                .all(|command| is_deterministic_command_allowed(command))
        {
            return Err(format!(
                "child task {} needs deterministic tests_to_run commands",
                index + 1
            ));
        }
        let task_id = format!("{}-W{}", parent.task_id, index + 1);
        let mut dependencies = Vec::new();
        if let Some(previous) = previous_id.clone() {
            dependencies.push(previous);
        } else {
            dependencies.extend(parent.dependencies.clone());
        }
        previous_id = Some(task_id.clone());
        let mut forbidden_actions = parent.forbidden_actions.clone();
        forbidden_actions
            .push("Do not broaden beyond this Rust-validated Worker split child task.".to_string());
        let exact_steps = non_empty_or_default(
            &child.exact_steps,
            vec!["Implement only this bounded child slice.".to_string()],
        );
        let success_criteria = non_empty_or_default(
            &child.success_criteria,
            vec!["Rust final verification passes for this child slice.".to_string()],
        );
        let stop_conditions =
            non_empty_or_default(&child.stop_conditions, parent.stop_conditions.clone());
        tasks.push(TaskSpec {
            task_id,
            parent_task: Some(parent.task_id.clone()),
            status: "approved".to_string(),
            priority: parent.priority.clone(),
            review_tier: parent.review_tier,
            automation_level: parent.automation_level,
            dependencies,
            title,
            kind: child.kind.clone().unwrap_or_else(|| parent.kind.clone()),
            objective,
            why_unblocks_progress: format!(
                "Worker requested a bounded split of `{}` and Rust validated this child scope.",
                parent.task_id
            ),
            allowed_files,
            read_only_files,
            forbidden_actions,
            exact_steps,
            success_criteria,
            tests_to_run: child.tests_to_run.clone(),
            stop_conditions,
            discovery_budget: parent.discovery_budget.clone(),
            estimated_minutes: child
                .estimated_minutes
                .unwrap_or(parent.estimated_minutes)
                .max(1),
            requires_network: false,
            prefer_worktree: None,
        });
    }
    Ok(tasks)
}

fn clean_orchestration_text(text: &str, max_chars: usize) -> String {
    truncate_inline(&text.replace(['\n', '\r'], " "), max_chars)
        .trim()
        .to_string()
}

fn non_empty_or_default(values: &[String], default: Vec<String>) -> Vec<String> {
    let values = values
        .iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if values.is_empty() {
        default
    } else {
        values
    }
}

fn enqueue_front_unique(state: &mut State, task: TaskSpec) {
    if state
        .queue
        .iter()
        .any(|queued| queued.task_id == task.task_id)
    {
        return;
    }
    state.queue.insert(0, task);
}

fn route_worker_non_done(
    worker: &WorkerOutput,
    guard_report: &GuardReport,
    failure_count: u32,
    max_task_failures: u32,
    fingerprint: Option<&FailureFingerprint>,
    fingerprint_retry_count: u32,
) -> WorkerNonDoneRoute {
    // Route policy:
    // - scope/policy ambiguity goes to planning
    // - known terminal/environment failures stop retry loops
    // - mechanical failures retry until their cap
    // - exhausted/known-but-not-retryable failures get Senior repair routing
    if !guard_report.violations.is_empty()
        || worker_blocker_requests_planner(worker)
        || fingerprint
            .map(FailureFingerprint::is_policy_scope)
            .unwrap_or(false)
    {
        return WorkerNonDoneRoute::PlannerDue;
    }
    if fingerprint
        .map(FailureFingerprint::is_environment_or_terminal)
        .unwrap_or(false)
    {
        return WorkerNonDoneRoute::PlannerDue;
    }
    let active_decision = decide_active_regret_route(&RouteRetryContext {
        safe_command_only_evidence: false,
        queue_has_approved_work: false,
        same_failure_fingerprint: fingerprint_retry_count > 1,
        fingerprint_retry_count,
        failure_count,
        max_task_failures,
        verification_improved: false,
        scope_overlap: false,
    });
    if matches!(
        active_decision.action,
        RouteDecisionAction::SeniorRepair | RouteDecisionAction::Stop
    ) {
        return WorkerNonDoneRoute::SeniorDevRepairOrder;
    }
    if fingerprint_retry_count > max_task_failures {
        return WorkerNonDoneRoute::SeniorDevRepairOrder;
    }
    if let Some(fingerprint) = fingerprint {
        if fingerprint.is_mechanical_retryable() && failure_count <= max_task_failures {
            return WorkerNonDoneRoute::RetryWorkerSameTask;
        }
        if fingerprint.kind != FailureFingerprintKind::Unknown {
            return WorkerNonDoneRoute::SeniorDevRepairOrder;
        }
    }
    if failure_count <= max_task_failures {
        return WorkerNonDoneRoute::RetryWorkerSameTask;
    }
    WorkerNonDoneRoute::SeniorDevRepairOrder
}

fn first_failed_command(report: &EvidenceReport) -> Option<&CommandEvidence> {
    report.commands.iter().find(|command| !command.success)
}

fn route_worker_done_verification_failure(
    report: &EvidenceReport,
    guard_report: &GuardReport,
    failed_command: &CommandEvidence,
    failure_count: u32,
    max_task_failures: u32,
    fingerprint: Option<&FailureFingerprint>,
    fingerprint_retry_count: u32,
) -> WorkerNonDoneRoute {
    // Worker said "done"; Rust has the receipt saying otherwise.  This is still
    // usually a repair state, not a review state, when scope is clean and the
    // failing command is locally actionable.
    if !guard_report.violations.is_empty() {
        return WorkerNonDoneRoute::PlannerDue;
    }
    if !report.rejected_commands.is_empty() {
        return WorkerNonDoneRoute::SeniorDevRepairOrder;
    }
    if fingerprint
        .map(FailureFingerprint::is_environment_or_terminal)
        .unwrap_or(false)
        || failed_command.category == DeterministicResultCategory::TimedOut.as_str()
        || failed_command.category == DeterministicResultCategory::KnownEnvironmentBlocker.as_str()
    {
        return WorkerNonDoneRoute::PlannerDue;
    }
    if failure_count > max_task_failures || fingerprint_retry_count > max_task_failures {
        return WorkerNonDoneRoute::SeniorDevRepairOrder;
    }
    let active_decision = decide_active_regret_route(&RouteRetryContext {
        safe_command_only_evidence: false,
        queue_has_approved_work: false,
        same_failure_fingerprint: fingerprint_retry_count > 1,
        fingerprint_retry_count,
        failure_count,
        max_task_failures,
        verification_improved: false,
        scope_overlap: false,
    });
    if matches!(
        active_decision.action,
        RouteDecisionAction::SeniorRepair | RouteDecisionAction::Stop
    ) {
        return WorkerNonDoneRoute::SeniorDevRepairOrder;
    }
    if let Some(fingerprint) = fingerprint {
        if fingerprint.is_mechanical_retryable() {
            return WorkerNonDoneRoute::RetryWorkerSameTask;
        }
        if fingerprint.kind == FailureFingerprintKind::Unknown && fingerprint_retry_count > 1 {
            return WorkerNonDoneRoute::SeniorDevRepairOrder;
        }
    }
    if is_locally_actionable_test_failure(failed_command) {
        return WorkerNonDoneRoute::RetryWorkerSameTask;
    }
    WorkerNonDoneRoute::SeniorDevRepairOrder
}

fn is_locally_actionable_test_failure(command: &CommandEvidence) -> bool {
    command.category == DeterministicResultCategory::CommandFailed.as_str()
        && matches!(command.command.kind, CommandKind::Test)
}

#[allow(clippy::too_many_arguments)]
fn build_verification_failure_retry_brief(
    paths: &Paths,
    task: &TaskSpec,
    worker: &WorkerOutput,
    guard_report: &GuardReport,
    report: &EvidenceReport,
    failed_command: &CommandEvidence,
    attempt: u32,
    fingerprint_retry_count: u32,
) -> WorkerRetryBrief {
    // This is the post-Worker verification version of `build_worker_retry_brief`:
    // Worker already claimed success, so the failed Rust command is the primary
    // authority and Worker prose is only supporting context.
    let command_text = command_evidence_text(failed_command);
    let worker_log_text = latest_worker_stdout_log_text(paths).unwrap_or_default();
    let combined_retry_text = format!(
        "{}\n{}\n{}\n{}",
        worker.summary, worker.blocker, command_text, worker_log_text
    );
    let fingerprint = refine_unknown_fingerprint_from_text(
        failure_fingerprint_from_command_evidence(failed_command),
        Some(&failed_command.command_display),
        Some(&failed_command.category),
        &combined_retry_text,
    );
    let mut top_compiler_errors = compiler_errors_from_text(&combined_retry_text);
    top_compiler_errors.truncate(3);
    let patch_context_mismatch_count = patch_context_mismatch_count(&combined_retry_text);
    let patch_context_hint = (patch_context_mismatch_count >= 2).then(|| {
        "Repeated apply_patch context mismatches were observed. Before patching again, re-read the exact line ranges you will edit and patch only the freshly observed text.".to_string()
    });
    let mut repair_instruction = format!(
        "You previously returned done, but Rust final verification failed. Continue from the dirty tree and fix only the failing verification evidence. {}",
        repair_instruction_for_fingerprint(task, &fingerprint)
    );
    if let Some(hint) = &patch_context_hint {
        repair_instruction.push(' ');
        repair_instruction.push_str(hint);
    }
    WorkerRetryBrief {
        task_id: task.task_id.clone(),
        attempt,
        route: WorkerNonDoneRoute::RetryWorkerSameTask.as_str().to_string(),
        worker_status: "done_but_verification_failed".to_string(),
        blocker: format!(
            "{} verification failed: {}",
            report.report_id,
            command_evidence_retry_summary(failed_command)
        ),
        changed_files: guard_report.changed_files.clone(),
        diff_stat: git_diff_stat_for_changed_files(paths, &guard_report.changed_files),
        diagnostic_command: Some(failed_command.command_display.clone()),
        diagnostic_category: Some(failed_command.category.clone()),
        diagnostic_summary: Some(command_evidence_retry_summary(failed_command)),
        fingerprint: Some(fingerprint),
        fingerprint_retry_count,
        top_compiler_errors,
        patch_context_mismatch_count,
        patch_context_hint,
        repair_instruction,
        verification_improved: false,
        prior_content_hashes: Vec::new(),
    }
}

fn failure_fingerprint_from_worker(
    worker: &WorkerOutput,
    guard_report: &GuardReport,
) -> FailureFingerprint {
    // Prefer guard evidence over Worker prose.  A scope violation is a policy
    // failure even if Worker summarized it as a compile or implementation issue.
    if !guard_report.violations.is_empty() {
        return FailureFingerprint {
            kind: FailureFingerprintKind::PolicyScopeViolation,
            file: guard_report.violations.first().cloned(),
            line: None,
            message: format!(
                "changed files outside allowed scope: {}",
                guard_report.violations.join(", ")
            ),
            command: None,
        };
    }
    failure_fingerprint_from_text(
        None,
        None,
        &format!("{}\n{}", worker.summary, worker.blocker),
    )
}

fn failure_fingerprint_from_command_evidence(evidence: &CommandEvidence) -> FailureFingerprint {
    // Retry diagnostics produce CommandEvidence; classify that before falling
    // back to model-authored blocker text.
    failure_fingerprint_from_text(
        Some(&evidence.command_display),
        Some(&evidence.category),
        &format!(
            "{}\n{}\n{}",
            evidence.evidence_summary, evidence.stdout_tail, evidence.stderr_tail
        ),
    )
}

fn worker_blocker_requests_planner(worker: &WorkerOutput) -> bool {
    // Worker can ask for planning when the task itself is wrong or unauthorized.
    // This is intentionally phrase-based and narrow to avoid "planner please" as
    // a generic escape hatch.
    let text = format!("{} {}", worker.summary, worker.blocker).to_ascii_lowercase();
    [
        "ambiguous task",
        "task is ambiguous",
        "requires planner",
        "requires project manager",
        "needs planner",
        "needs project manager",
        "not approved",
        "not authorized",
        "outside the approved scope",
        "outside allowed files",
        "missing roadmap",
    ]
    .iter()
    .any(|needle| text.contains(needle))
}

#[derive(Clone, Copy)]
struct WorkerRetryOptions {
    attempt: u32,
    fingerprint_retry_count: u32,
    run_diagnostic: bool,
}

fn build_worker_retry_brief(
    paths: &Paths,
    cfg: &Config,
    task: &TaskSpec,
    worker: &WorkerOutput,
    guard_report: &GuardReport,
    options: WorkerRetryOptions,
) -> Result<WorkerRetryBrief> {
    // A retry brief is created from evidence, not vibes.  When allowed, Rust runs
    // one bounded diagnostic command first so Worker gets the freshest error.
    let diagnostic = if options.run_diagnostic {
        run_worker_retry_diagnostic(paths, cfg, task, worker)?
    } else {
        None
    };
    let diagnostic_text = diagnostic
        .as_ref()
        .map(command_evidence_text)
        .unwrap_or_default();
    let worker_log_text = latest_worker_stdout_log_text(paths).unwrap_or_default();
    let combined_retry_text = format!(
        "{}\n{}\n{}\n{}",
        worker.summary, worker.blocker, diagnostic_text, worker_log_text
    );
    let fingerprint = if let Some(diagnostic) = diagnostic.as_ref() {
        refine_unknown_fingerprint_from_text(
            failure_fingerprint_from_command_evidence(diagnostic),
            Some(&diagnostic.command_display),
            Some(&diagnostic.category),
            &combined_retry_text,
        )
    } else if !guard_report.violations.is_empty() {
        failure_fingerprint_from_worker(worker, guard_report)
    } else {
        failure_fingerprint_from_text(None, None, &combined_retry_text)
    };
    let mut top_compiler_errors = compiler_errors_from_text(&combined_retry_text);
    top_compiler_errors.truncate(3);
    let patch_context_mismatch_count = patch_context_mismatch_count(&combined_retry_text);
    let patch_context_hint = (patch_context_mismatch_count >= 2).then(|| {
        "Repeated apply_patch context mismatches were observed. Before patching again, re-read the exact line ranges you will edit and patch only the freshly observed text.".to_string()
    });
    let mut repair_instruction = repair_instruction_for_fingerprint(task, &fingerprint);
    if let Some(hint) = &patch_context_hint {
        repair_instruction.push(' ');
        repair_instruction.push_str(hint);
    }
    Ok(WorkerRetryBrief {
        task_id: task.task_id.clone(),
        attempt: options.attempt,
        route: WorkerNonDoneRoute::RetryWorkerSameTask.as_str().to_string(),
        worker_status: worker.status.clone(),
        blocker: worker.blocker.clone(),
        changed_files: guard_report.changed_files.clone(),
        diff_stat: git_diff_stat_for_changed_files(paths, &guard_report.changed_files),
        diagnostic_command: diagnostic
            .as_ref()
            .map(|evidence| evidence.command_display.clone()),
        diagnostic_category: diagnostic
            .as_ref()
            .map(|evidence| evidence.category.clone()),
        diagnostic_summary: diagnostic.as_ref().map(command_evidence_retry_summary),
        fingerprint: Some(fingerprint),
        fingerprint_retry_count: options.fingerprint_retry_count,
        top_compiler_errors,
        patch_context_mismatch_count,
        patch_context_hint,
        repair_instruction,
        verification_improved: false,
        prior_content_hashes: Vec::new(),
    })
}

fn refine_unknown_fingerprint_from_text(
    fingerprint: FailureFingerprint,
    command: Option<&str>,
    category: Option<&str>,
    combined_text: &str,
) -> FailureFingerprint {
    if fingerprint.kind != FailureFingerprintKind::Unknown {
        return fingerprint;
    }
    let fallback = failure_fingerprint_from_text(command, category, combined_text);
    if fallback.kind == FailureFingerprintKind::Unknown {
        fingerprint
    } else {
        fallback
    }
}

fn repair_instruction_for_fingerprint(task: &TaskSpec, fingerprint: &FailureFingerprint) -> String {
    let location = match (&fingerprint.file, fingerprint.line) {
        (Some(file), Some(line)) => format!("{file}:{line}"),
        (Some(file), None) => file.clone(),
        _ => "the changed allowed files".to_string(),
    };
    if fingerprint.kind == FailureFingerprintKind::SwiftInitializerLabelMismatch {
        format!(
            "Continue `{}` from the current dirty tree. Fix only `{}` at {}. Align stored properties, public init parameters, and all constructor call sites. Do not add duplicate fields. Do not broaden scope, do not edit tracking docs unless the task explicitly allows them, and hand final verification back to Rust.",
            task.task_id,
            fingerprint.kind.as_str(),
            location
        )
    } else if fingerprint.kind == FailureFingerprintKind::ApplyPatchContextMismatch {
        format!(
            "Continue `{}` from the current dirty tree. Fix only `{}` at {}. Re-read the exact line ranges you will edit before applying a patch, then patch only the freshly observed context. Do not broaden scope, do not edit tracking docs unless the task explicitly allows them, and hand final verification back to Rust.",
            task.task_id,
            fingerprint.kind.as_str(),
            location
        )
    } else {
        format!(
            "Continue `{}` from the current dirty tree. Fix only `{}` at {}; do not broaden scope, do not edit tracking docs unless the task explicitly allows them, and hand final verification back to Rust.",
            task.task_id,
            fingerprint.kind.as_str(),
            location
        )
    }
}

fn command_evidence_text(evidence: &CommandEvidence) -> String {
    format!(
        "{}\n{}\n{}",
        evidence.evidence_summary, evidence.stdout_tail, evidence.stderr_tail
    )
}

fn latest_worker_stdout_log_text(paths: &Paths) -> Option<String> {
    let mut candidates = fs::read_dir(&paths.runs)
        .ok()?
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let name = path.file_name()?.to_str()?.to_string();
            if !name.ends_with("-worker.stdout.jsonl") {
                return None;
            }
            let modified = entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .ok();
            Some((modified, path))
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
    let path = candidates.into_iter().next()?.1;
    fs::read_to_string(path)
        .ok()
        .map(|text| output_tail(&text, 40_000))
}

#[derive(Debug, Clone)]
struct WorkerLogFailedCommand {
    command: String,
    context: String,
}

fn refresh_retry_brief_from_latest_worker_log(
    paths: &Paths,
    task: &TaskSpec,
    mut brief: WorkerRetryBrief,
) -> WorkerRetryBrief {
    let Some(worker_log_text) = latest_worker_stdout_log_text(paths) else {
        return brief;
    };
    let Some(failed) = worker_log_latest_failed_command(&worker_log_text) else {
        return brief;
    };
    let Some(command) = parse_verification_command(&failed.command, "worker.failed_log") else {
        return brief;
    };
    let command_display = command.display();
    if brief.diagnostic_command.as_deref() == Some(command_display.as_str()) {
        return brief;
    }
    let evidence = CommandEvidence {
        id: format!("{}:worker-log-failure", sanitize_filename(&task.task_id)),
        command,
        command_display,
        exit_code: 1,
        success: false,
        category: DeterministicResultCategory::CommandFailed
            .as_str()
            .to_string(),
        duration_ms: 0,
        stdout_tail: output_tail(&failed.context, 2_000),
        stderr_tail: output_tail(&failed.context, 2_000),
        evidence_summary: first_useful_output_line(&failed.context)
            .unwrap_or_else(|| "failed command observed in latest Worker log".to_string()),
    };
    let combined = command_evidence_text(&evidence);
    let fingerprint = failure_fingerprint_from_command_evidence(&evidence);
    let mut top_compiler_errors = compiler_errors_from_text(&combined);
    top_compiler_errors.truncate(3);
    let patch_context_mismatch_count = patch_context_mismatch_count(&worker_log_text);
    let patch_context_hint = (patch_context_mismatch_count >= 2).then(|| {
        "Repeated apply_patch context mismatches were observed. Before patching again, re-read the exact line ranges you will edit and patch only the freshly observed text.".to_string()
    });
    let mut repair_instruction = format!(
        "Rust refreshed this retry from the latest Worker log. Fix only this failed verification command: `{}`. {}",
        evidence.command_display,
        repair_instruction_for_fingerprint(task, &fingerprint)
    );
    if let Some(hint) = &patch_context_hint {
        repair_instruction.push(' ');
        repair_instruction.push_str(hint);
    }
    brief.diagnostic_command = Some(evidence.command_display.clone());
    brief.diagnostic_category = Some(evidence.category.clone());
    brief.diagnostic_summary = Some(command_evidence_retry_summary(&evidence));
    brief.fingerprint = Some(fingerprint);
    brief.top_compiler_errors = top_compiler_errors;
    brief.patch_context_mismatch_count = patch_context_mismatch_count;
    brief.patch_context_hint = patch_context_hint;
    brief.repair_instruction = repair_instruction;
    brief.blocker = format!(
        "Latest Worker log failure: {}",
        command_evidence_retry_summary(&evidence)
    );
    brief
}

fn worker_log_failed_verification_command(text: &str) -> Option<VerificationCommand> {
    // Codex raw streams are JSONL, but the human-visible event text is still
    // embedded in those lines. Prefer the latest explicit failed command so a
    // retry diagnostic reruns the failure Worker actually hit, not merely the
    // first command in the task plan.
    worker_log_failed_command_strings(text)
        .into_iter()
        .rev()
        .find_map(|command| parse_verification_command(&command, "worker.failed_log"))
}

fn worker_log_failed_command_strings(text: &str) -> Vec<String> {
    worker_log_failed_commands(text)
        .into_iter()
        .map(|failed| failed.command)
        .collect()
}

fn worker_log_latest_failed_command(text: &str) -> Option<WorkerLogFailedCommand> {
    worker_log_failed_commands(text).into_iter().last()
}

fn worker_log_failed_commands(text: &str) -> Vec<WorkerLogFailedCommand> {
    let mut commands = Vec::new();
    let lines = text.lines().collect::<Vec<_>>();
    for (index, raw_line) in lines.iter().enumerate() {
        let normalized = raw_line
            .replace("\\n", "\n")
            .replace("\\\"", "\"")
            .replace("\\/", "/");
        for line in normalized.lines() {
            if let Some(command) = extract_failed_command_from_line(line) {
                let context = lines
                    .iter()
                    .skip(index)
                    .take(18)
                    .copied()
                    .collect::<Vec<_>>()
                    .join("\n")
                    .replace("\\n", "\n")
                    .replace("\\\"", "\"")
                    .replace("\\/", "/");
                commands.push(WorkerLogFailedCommand { command, context });
            }
        }
    }
    commands
}

fn extract_failed_command_from_line(line: &str) -> Option<String> {
    let start = line.find("Failed ")? + "Failed ".len();
    let rest = &line[start..];
    let end = rest
        .find(" exit ")
        .or_else(|| rest.find(" exited "))
        .or_else(|| rest.find(" for "))?;
    let command = rest[..end]
        .trim()
        .trim_matches('"')
        .trim_matches('`')
        .trim();
    (!command.is_empty()).then(|| command.to_string())
}

fn select_worker_retry_diagnostic_command(
    paths: &Paths,
    task: &TaskSpec,
    worker: &WorkerOutput,
) -> Option<VerificationCommand> {
    if let Some(worker_log_text) = latest_worker_stdout_log_text(paths) {
        if let Some(command) = worker_log_failed_verification_command(&worker_log_text) {
            return Some(command);
        }
    }
    let plan = build_verification_plan(task, worker);
    if !worker.status.eq_ignore_ascii_case("done") && !worker.files_changed.is_empty() {
        if let Some(command) = plan
            .commands
            .iter()
            .find(|command| !is_git_diff_check_command(command))
            .cloned()
        {
            return Some(command);
        }
    }
    plan.commands.first().cloned()
}

fn run_worker_retry_diagnostic(
    paths: &Paths,
    cfg: &Config,
    task: &TaskSpec,
    worker: &WorkerOutput,
) -> Result<Option<CommandEvidence>> {
    // Use only one command. Prefer a failed command observed in the Worker log;
    // fall back to the first command from the final verification plan. The goal
    // is to fingerprint the current failure cheaply, not to run a full chain.
    let Some(command) = select_worker_retry_diagnostic_command(paths, task, worker) else {
        return Ok(None);
    };
    let mut plan = build_verification_plan(task, worker);
    let mut command = with_stable_command_metadata(task.task_id.as_str(), "retry", 0, command);
    command.source = "worker.retry_diagnostic".to_string();
    command.failure_policy = FailurePolicy::Stop;
    plan.plan_id = format!(
        "{}-retry-diagnostic-{}",
        sanitize_filename(&task.task_id),
        Utc::now().timestamp_millis().unsigned_abs()
    );
    plan.source = "worker.retry_diagnostic".to_string();
    plan.commands = vec![command.clone()];
    plan.rejected_commands.clear();
    println!(
        "[Rust] Running retry diagnostic for {}: {}",
        task.task_id,
        truncate_inline(&command.display(), 140)
    );
    let report =
        execute_verification_plan_with_policy(paths, &plan, cfg.requires_verification_isolation())?;
    let evidence_path = persist_evidence_report(paths, &report)?;
    let _ = append_event(
        paths,
        OrchestratorEventKind::EvidenceSummarized,
        Some(&task.task_id),
        json!({
            "report_id": report.report_id.clone(),
            "status": report.status.clone(),
            "evidence_path": display_path(paths, &evidence_path),
            "mode": "worker_retry_diagnostic",
        }),
    );
    Ok(report.commands.into_iter().next())
}

fn command_evidence_retry_summary(evidence: &CommandEvidence) -> String {
    let useful = first_useful_output_line(&evidence.stderr_tail)
        .or_else(|| first_useful_output_line(&evidence.stdout_tail));
    match useful {
        Some(line) => format!(
            "`{}` -> `{}` exit {} in {}: {}",
            evidence.command_display,
            evidence.category,
            evidence.exit_code,
            format_duration(Duration::from_millis(evidence.duration_ms as u64)),
            truncate_inline(&line, 300)
        ),
        None => format!(
            "`{}` -> `{}` exit {} in {}: {}",
            evidence.command_display,
            evidence.category,
            evidence.exit_code,
            format_duration(Duration::from_millis(evidence.duration_ms as u64)),
            truncate_inline(&evidence.evidence_summary, 300)
        ),
    }
}

fn senior_batch_worker_context(state: &State, task: &TaskSpec) -> Value {
    let Some(batch) = state.senior_batch_approval.as_ref() else {
        return Value::Null;
    };
    if !batch.task_ids.contains(&task.task_id) {
        return Value::Null;
    }
    json!({
        "mode": "worker_continuation",
        "batch_id": batch.batch_id.clone(),
        "remaining_successes": batch.remaining_successes,
        "allowed_kinds": batch.allowed_kinds.clone(),
        "forbidden_scope": batch.forbidden_scope.clone(),
        "requires_stop_on": batch.requires_stop_on.clone(),
        "verification_required": batch.verification_required.clone(),
        "auto_accept_policy": batch.auto_accept_policy.clone(),
        "instruction": "You are not inventing or approving roadmap work. Continue this existing Senior-approved gated backlog item only if it is mechanically implied by accepted prior evidence. If the task card, gate evidence, allowed files, or forbidden scope do not line up, return blocked."
    })
}

fn build_attempt_capsule(
    task: &TaskSpec,
    retry: Option<&WorkerRetryBrief>,
) -> Option<AttemptCapsule> {
    let retry = retry?;
    let failure_id = retry.fingerprint.as_ref().map(FailureFingerprint::key);
    Some(AttemptCapsule {
        task_id: task.task_id.clone(),
        attempt: retry.attempt,
        worker_status: retry.worker_status.clone(),
        objective: truncate_inline(&task.objective, 700),
        files_touched: retry.changed_files.clone(),
        failure_id,
        failure_fingerprint: retry.fingerprint.clone(),
        same_failure_as_prior_attempt: retry.fingerprint_retry_count > 0,
        diff_summary: truncate_inline(&retry.diff_stat, 1_200),
        diagnostic_command: retry.diagnostic_command.clone(),
        diagnostic_category: retry.diagnostic_category.clone(),
        diagnostic_summary: retry
            .diagnostic_summary
            .as_ref()
            .map(|summary| truncate_inline(summary, 1_200)),
        top_compiler_errors: retry
            .top_compiler_errors
            .iter()
            .take(3)
            .map(|diagnostic| truncate_inline(&diagnostic.display(), 400))
            .collect(),
        patch_context_hint: retry
            .patch_context_hint
            .as_ref()
            .map(|hint| truncate_inline(hint, 500)),
        next_instruction: truncate_inline(&retry.repair_instruction, 1_200),
        omitted_context: vec![
            "full previous Worker transcript".to_string(),
            "full build/test log".to_string(),
            "full git diff".to_string(),
            "unchanged role policy repeated from prior prompt".to_string(),
        ],
    })
}

fn retry_log_section(capsule: Option<&AttemptCapsule>) -> String {
    let Some(capsule) = capsule else {
        return "No retry logs included for this first attempt.".to_string();
    };
    if capsule.same_failure_as_prior_attempt {
        return format!(
            "Same failure fingerprint as prior attempt: {}. Full duplicate log omitted. Diagnostic summary: {}",
            capsule.failure_id.as_deref().unwrap_or("(unknown)"),
            capsule
                .diagnostic_summary
                .as_deref()
                .unwrap_or("no diagnostic summary")
        );
    }
    let mut lines = Vec::new();
    if let Some(summary) = &capsule.diagnostic_summary {
        lines.push(format!("Diagnostic summary: {summary}"));
    }
    for diagnostic in &capsule.top_compiler_errors {
        lines.push(format!("Compiler diagnostic: {diagnostic}"));
    }
    if lines.is_empty() {
        "No retry log diagnostics available; full logs omitted by default.".to_string()
    } else {
        lines.join("\n")
    }
}

fn retry_diff_section(capsule: Option<&AttemptCapsule>) -> String {
    let Some(capsule) = capsule else {
        return "No retry diff included for this first attempt.".to_string();
    };
    if capsule.same_failure_as_prior_attempt {
        return format!(
            "Same failure fingerprint as prior attempt: {}. Full duplicate diff omitted. Diff summary: {}",
            capsule.failure_id.as_deref().unwrap_or("(unknown)"),
            capsule.diff_summary
        );
    }
    format!(
        "Changed files: {}\nDiff summary: {}",
        if capsule.files_touched.is_empty() {
            "none recorded".to_string()
        } else {
            capsule.files_touched.join(", ")
        },
        capsule.diff_summary
    )
}

fn annotate_retry_brief_progress(state: &State, mut brief: WorkerRetryBrief) -> WorkerRetryBrief {
    if let Some(prior) = state.retry_worker_brief.as_ref() {
        if prior.task_id == brief.task_id {
            brief.verification_improved = prior.diagnostic_summary != brief.diagnostic_summary
                || prior.diff_stat != brief.diff_stat
                || prior.blocker != brief.blocker;
            brief.prior_content_hashes = prior
                .prior_content_hashes
                .iter()
                .cloned()
                .chain([
                    ("diff".to_string(), prior.diff_stat.clone()),
                    (
                        "logs".to_string(),
                        prior.diagnostic_summary.clone().unwrap_or_default(),
                    ),
                    (
                        "previous_attempts".to_string(),
                        prior.repair_instruction.clone(),
                    ),
                ])
                .collect();
            brief.prior_content_hashes.sort();
            brief.prior_content_hashes.dedup_by(|a, b| a.0 == b.0);
        }
    }
    brief
}

fn build_worker_context_pack(
    paths: &Paths,
    cfg: &Config,
    state: &State,
    task: &TaskSpec,
    support_brief: &TaskSupportBrief,
    retry_brief: Option<&WorkerRetryBrief>,
) -> Result<ContextPack> {
    let attempt_capsule = build_attempt_capsule(task, retry_brief);
    let task_json = serde_json::to_string_pretty(task)?;
    let repo_hints = {
        let mut hint_cache = crate::context::lease::RepoHintCache::default();
        let cache_key = format!("task:{}", task.task_id);
        if let Some(cached) = hint_cache.get(&cache_key) {
            cached.to_string()
        } else {
            let fresh = serde_json::to_string_pretty(&json!({
                "support_brief": support_brief,
                "queue_snapshot": build_queue_snapshot_summary(paths, state),
                "senior_batch_context": senior_batch_worker_context(state, task),
                "deterministic_probe_summaries": deterministic_probe_summaries(paths, &task.task_id),
                "deterministic_recipe_summaries": deterministic_recipe_summaries(paths, 5),
                "rust_evidence_job_receipts": recent_rust_evidence_job_receipts(paths, 5),
            }))?;
            hint_cache.put(cache_key, fresh.clone());
            let _ = hint_cache;
            fresh
        }
    };
    let previous_attempts = serde_json::to_string_pretty(&json!({
        "attempt_capsule": attempt_capsule.clone(),
        "policy": "Retries use compact capsules only; full old transcripts are not included by default."
    }))?;
    let logs = retry_log_section(attempt_capsule.as_ref());
    let diff = retry_diff_section(attempt_capsule.as_ref());
    let guidelines = r#"Return only the JSON object required by the Worker schema.
Rust owns final deterministic verification, output capture, sequencing, queue state, and commits.
Use the task card as authority. If more context is needed, request a bounded lease expansion instead of scanning broadly."#;
    let prior_hashes = retry_brief
        .map(|brief| {
            brief
                .prior_content_hashes
                .iter()
                .cloned()
                .collect::<std::collections::BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    let delta_mode = retry_brief.is_some() && !prior_hashes.is_empty();
    let mut logs_section = ContextPackSection::new(
        "Logs",
        PromptSectionCategory::Logs,
        PromptSectionPriority::Medium,
        "compressed retry diagnostics only",
        logs.clone(),
        2_000,
        500,
    )
    .fallback_summary("Logs compressed; full raw logs omitted.");
    let mut diff_section = ContextPackSection::new(
        "Diff",
        PromptSectionCategory::Diff,
        PromptSectionPriority::Medium,
        "diff stat and changed-file summary only",
        diff.clone(),
        1_500,
        375,
    )
    .fallback_summary("Diff compressed; full diff omitted.");
    let mut previous_section = ContextPackSection::new(
        "Previous Attempts",
        PromptSectionCategory::PreviousAttempts,
        PromptSectionPriority::High,
        "compact Attempt Capsule for retry/escalation",
        previous_attempts.clone(),
        3_000,
        750,
    )
    .fallback_summary("Attempt capsule truncated; full prior transcript omitted.");
    if delta_mode {
        if prior_hashes.get("logs").is_some_and(|hash| hash == &logs) {
            logs_section = logs_section.deduplicated(logs);
            logs_section.content =
                "Unchanged since prior Attempt Capsule; full logs omitted.".to_string();
        }
        if prior_hashes.get("diff").is_some_and(|hash| hash == &diff) {
            diff_section = diff_section.deduplicated(diff);
            diff_section.content =
                "Unchanged since prior Attempt Capsule; full diff omitted.".to_string();
        }
        if prior_hashes
            .get("previous_attempts")
            .is_some_and(|hash| hash == &previous_attempts)
        {
            previous_section = previous_section.deduplicated(previous_attempts);
            previous_section.content =
                "Unchanged Attempt Capsule fields omitted; refer to fingerprint only.".to_string();
        }
    }

    Ok(ContextPackBuilder::new(
        "worker",
        Some(task.task_id.clone()),
        if delta_mode {
            "worker_attempt_delta"
        } else {
            "worker_attempt"
        },
        cfg.worker_prompt_hard_max_tokens,
    )
    .section(
        ContextPackSection::new(
            "Role Prompt",
            PromptSectionCategory::RolePrompt,
            PromptSectionPriority::High,
            "compact Worker rules and response contract",
            WORKER_PROMPT,
            9_000,
            2_250,
        )
        .fallback_summary(
            "Worker rules truncated; task card and JSON schema remain authoritative.",
        ),
    )
    .section(
        ContextPackSection::new(
            "Task Text",
            PromptSectionCategory::TaskText,
            PromptSectionPriority::Critical,
            "selected Worker task card",
            task_json,
            7_000,
            1_750,
        )
        .fallback_summary("Task card truncated to selected task metadata and allowed scope."),
    )
    .section(
        ContextPackSection::new(
            "Repo Hints",
            PromptSectionCategory::RepoHints,
            PromptSectionPriority::High,
            "Rust-generated support, queue, recipe, and evidence receipts",
            repo_hints,
            7_000,
            1_750,
        )
        .fallback_summary("Repo hints truncated to compact queue/support summaries."),
    )
    .section(ContextPackSection::new(
        "File Contents",
        PromptSectionCategory::FileContents,
        PromptSectionPriority::Low,
        "leased file contents only; default prompt embeds none",
        match state.worker_file_leases.get(&task.task_id) {
            Some(lease) => crate::context::lease::render_leased_file_contents(&paths.root, lease)
                .unwrap_or_else(|err| format!("Lease render failed: {err}")),
            None => "No file contents embedded in the initial Worker prompt.".to_string(),
        },
        4_000,
        1_000,
    ))
    .section(logs_section)
    .section(diff_section)
    .section(previous_section)
    .section(ContextPackSection::new(
        "Guidelines",
        PromptSectionCategory::Guidelines,
        PromptSectionPriority::Critical,
        "short execution rules that prevent scope and verification churn",
        guidelines,
        1_500,
        375,
    ))
    .build())
}

fn worker_prompt_lint_rejection_reason(pack: &ContextPack) -> Option<String> {
    let mut reasons = Vec::new();
    let prompt = &pack.prompt;

    for header in ["## Role Prompt", "## Logs", "## Guidelines"] {
        let count = prompt.matches(header).count();
        if count > 1 {
            reasons.push(format!(
                "duplicate {} blocks were included",
                header.trim_start_matches("## ").to_ascii_lowercase()
            ));
        }
    }

    if prompt.contains("\nuser:")
        || prompt.contains("\nassistant:")
        || prompt.contains("BEGIN TRANSCRIPT")
        || prompt.contains("END TRANSCRIPT")
    {
        reasons.push(
            "full prior transcript markers were included instead of compact retry context"
                .to_string(),
        );
    }

    if prompt.contains("## File Contents")
        && !prompt.contains("No file contents embedded in the initial Worker prompt.")
        && !prompt.contains("### ")
    {
        reasons.push("unrelated file contents were included in the Worker prompt".to_string());
    }

    if prompt.contains("diff --git ")
        || prompt.contains("\n@@ ")
        || prompt.contains("\n--- a/")
        || prompt.contains("\n+++ b/")
    {
        reasons
            .push("full diff payload was included instead of the compact diff summary".to_string());
    }

    if reasons.is_empty() {
        None
    } else {
        Some(reasons.join("; "))
    }
}

fn worker_retry_no_progress_rejection_reason(
    state: &State,
    task: &TaskSpec,
    cfg: &Config,
    retry_brief: Option<&WorkerRetryBrief>,
) -> Option<String> {
    let retry_brief = retry_brief?;
    let fingerprint = retry_brief.fingerprint.as_ref()?;
    let failure_count = state.failed_tasks.get(&task.task_id).copied().unwrap_or(0);
    let decision = decide_active_regret_route(&RouteRetryContext {
        safe_command_only_evidence: false,
        queue_has_approved_work: false,
        same_failure_fingerprint: retry_brief.fingerprint_retry_count > 1,
        fingerprint_retry_count: retry_brief.fingerprint_retry_count,
        failure_count,
        max_task_failures: cfg.max_task_failures,
        verification_improved: retry_brief.verification_improved,
        scope_overlap: false,
    });

    if matches!(
        decision.action,
        RouteDecisionAction::SeniorRepair | RouteDecisionAction::Stop
    ) {
        Some(format!(
            "same failure fingerprint repeated without verification improvement: {} ({})",
            fingerprint.key(),
            decision.reason
        ))
    } else {
        None
    }
}

fn synthetic_blocked_worker_output(
    task_id: &str,
    summary: String,
    blocker: String,
    confidence: &str,
) -> WorkerOutput {
    WorkerOutput {
        task_id: task_id.to_string(),
        status: "blocked".to_string(),
        summary,
        files_changed: Vec::new(),
        commands_run: Vec::new(),
        tests_run: Vec::new(),
        handoff_to_rust: Vec::new(),
        orchestration_request: None,
        blocker,
        confidence: confidence.to_string(),
    }
}

fn persist_prompt_receipt(paths: &Paths, receipt: &PromptReceipt) -> Result<PathBuf> {
    let dir = paths.observations.join("prompt_receipts");
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.json", sanitize_filename(&receipt.receipt_id)));
    write_text(
        &path,
        &format!("{}\n", serde_json::to_string_pretty(receipt)?),
    )?;
    append_event(
        paths,
        OrchestratorEventKind::PromptReceiptLogged,
        receipt.task_id.as_deref(),
        json!({
            "receipt_id": receipt.receipt_id.clone(),
            "role": receipt.role.clone(),
            "mode": receipt.mode.clone(),
            "total_token_estimate": receipt.total_token_estimate,
            "hard_max_tokens": receipt.hard_max_tokens,
            "rejected": receipt.rejected,
            "path": display_path(paths, &path),
        }),
    )?;
    Ok(path)
}

fn run_worker(
    paths: &Paths,
    cfg: &Config,
    state: &State,
    task: &TaskSpec,
    support_brief: &TaskSupportBrief,
    retry_brief: Option<&WorkerRetryBrief>,
    runner: &dyn CodexRunner,
) -> Result<WorkerOutput> {
    let ca_runner = RealCaRunner;
    run_worker_with_runners(
        paths,
        cfg,
        state,
        task,
        support_brief,
        retry_brief,
        &RoleRunners {
            codex: runner,
            ca: &ca_runner,
        },
    )
}

#[allow(clippy::too_many_arguments)]
fn run_worker_with_runners(
    paths: &Paths,
    cfg: &Config,
    state: &State,
    task: &TaskSpec,
    support_brief: &TaskSupportBrief,
    retry_brief: Option<&WorkerRetryBrief>,
    runners: &RoleRunners<'_>,
) -> Result<WorkerOutput> {
    if cfg.roles.worker.provider == RoleProvider::Ca && task.requires_network {
        bail!("ca Worker does not support tasks that require network access");
    }
    // Worker gets the bounded task, optional Rust retry context, and compact
    // deterministic hints.  It should edit code, not own final verification.
    let prompt_path = paths.prompts.join("worker.md");
    write_text(&prompt_path, WORKER_PROMPT)?;
    let schema_path = paths.schemas.join("worker.schema.json");
    write_text(&schema_path, &render_schema(WORKER_SCHEMA))?;
    let pack = build_worker_context_pack(paths, cfg, state, task, support_brief, retry_brief)?;
    let mut receipt = pack.receipt.clone();
    if let Some(reason) = worker_prompt_lint_rejection_reason(&pack) {
        receipt.rejected = true;
        receipt.rejection_reason = Some(reason.clone());
        let receipt_path = persist_prompt_receipt(paths, &receipt)?;
        println!(
            "[Rust] Worker prompt lint rejected {} before model launch; receipt {}",
            task.task_id,
            display_path(paths, &receipt_path)
        );
        return Ok(synthetic_blocked_worker_output(
            &task.task_id,
            format!("Worker prompt lint rejected before model launch: {}", reason),
            format!(
                "Worker prompt lint rejected before model launch: {}. The prompt shape is outside the approved scope, so Rust must request planning instead of launching Worker.",
                reason
            ),
            "high",
        ));
    }
    if let Some(reason) = worker_retry_no_progress_rejection_reason(state, task, cfg, retry_brief) {
        receipt.rejected = true;
        receipt.rejection_reason = Some(reason.clone());
        let receipt_path = persist_prompt_receipt(paths, &receipt)?;
        println!(
            "[Rust] Worker retry stopped {} before model launch; receipt {}",
            task.task_id,
            display_path(paths, &receipt_path)
        );
        return Ok(synthetic_blocked_worker_output(
            &task.task_id,
            format!("Worker retry stopped before model launch: {}", reason),
            reason,
            "medium",
        ));
    }
    let receipt_path = persist_prompt_receipt(paths, &receipt)?;
    println!(
        "[Rust] Worker context pack {}: ~{} tokens, ~{}s context review; receipt {}",
        task.task_id,
        pack.receipt.total_token_estimate,
        pack.receipt.estimated_context_review_seconds,
        display_path(paths, &receipt_path)
    );
    if pack.receipt.rejected {
        bail!(
            "Worker prompt for {} exceeds hard token budget: estimated {} > {}. Receipt: {}",
            task.task_id,
            pack.receipt.total_token_estimate,
            pack.receipt.hard_max_tokens,
            display_path(paths, &receipt_path)
        );
    }
    let raw = run_role_with_runners(
        paths,
        cfg,
        runners,
        CaRoleAuthority::Worker {
            allowed_files: &task.allowed_files,
        },
        "worker",
        &cfg.roles.worker,
        Some(&task.task_id),
        &pack.prompt,
        &schema_path,
    )?;
    parse_worker_json_response(&raw, &task.task_id)
}

fn parse_worker_json_response(raw: &str, task_id: &str) -> Result<WorkerOutput> {
    let mut value = crate::providers::json_hydrate::parse_and_hydrate_role_json(raw)
        .map_err(|_| malformed_role_output("worker JSON hydration", raw))?;
    crate::providers::json_hydrate::stamp_known_task_id(&mut value, task_id);
    serde_json::from_value(value).map_err(|_| malformed_role_output("worker typed validation", raw))
}

#[allow(clippy::too_many_arguments)]
fn run_senior_dev_review(
    paths: &Paths,
    cfg: &Config,
    state: &State,
    task: &TaskSpec,
    worker: &WorkerOutput,
    guard_report: &GuardReport,
    rust_verification: Value,
    runner: &dyn CodexRunner,
) -> Result<SeniorDevReviewOutput> {
    let ca_runner = RealCaRunner;
    run_senior_dev_review_with_runners(
        paths,
        cfg,
        state,
        task,
        worker,
        guard_report,
        rust_verification,
        &RoleRunners {
            codex: runner,
            ca: &ca_runner,
        },
    )
}

#[allow(clippy::too_many_arguments)]
fn run_senior_dev_review_with_runners(
    paths: &Paths,
    cfg: &Config,
    state: &State,
    task: &TaskSpec,
    worker: &WorkerOutput,
    guard_report: &GuardReport,
    rust_verification: Value,
    runners: &RoleRunners<'_>,
) -> Result<SeniorDevReviewOutput> {
    // Normal Senior review is now a fallback path.  The prompt still includes
    // legacy context, but `review_context` is the compact receipt Senior should
    // primarily reason from.
    let prompt_path = paths.prompts.join("foreman_review.md");
    write_text(&prompt_path, FOREMAN_REVIEW_PROMPT)?;
    let schema_path = paths.schemas.join("foreman_review.schema.json");
    write_text(&schema_path, &render_schema(FOREMAN_REVIEW_SCHEMA))?;
    let review_assist = build_review_assist_packet(paths, task, guard_report, &rust_verification);
    let review_context = build_compact_review_context_pack(
        paths,
        task,
        guard_report,
        &rust_verification,
        None,
        "review",
    );
    let queue_snapshot = build_queue_snapshot_summary(paths, state);
    let prompt = render_prompt_limited(
        &read_text(&prompt_path)?,
        paths,
        state,
        json!({
            "role": "foreman",
            "mode": "review",
            "task": task,
            "worker_result": compact_worker_review_result(worker),
            "orchestrator_guard_report": guard_report,
            "rust_verification": rust_verification,
            "review_assist": review_assist,
            "review_context": review_context,
            "queue_snapshot": queue_snapshot,
            "deterministic_probe_summaries": deterministic_probe_summaries(paths, &task.task_id),
            "deterministic_recipe_summaries": deterministic_recipe_summaries(paths, 5)
        }),
        PromptLimits::senior_review(),
    )?;
    let raw = run_role_with_runners(
        paths,
        cfg,
        runners,
        CaRoleAuthority::ReadOnly,
        "foreman-review",
        &cfg.roles.foreman,
        Some(&task.task_id),
        &prompt,
        &schema_path,
    )?;
    parse_json_response(&raw)
}

fn compact_worker_review_result(worker: &WorkerOutput) -> Value {
    json!({
        "task_id": &worker.task_id,
        "status": &worker.status,
        "summary": truncate(&worker.summary, 1_200),
        "files_changed": compact_string_list(&worker.files_changed, 24, 240),
        "commands_run": compact_string_list(&worker.commands_run, 10, 320),
        "tests_run": compact_string_list(&worker.tests_run, 10, 320),
        "handoff_to_rust": worker.handoff_to_rust.iter().take(8).collect::<Vec<_>>(),
        "orchestration_request": &worker.orchestration_request,
        "blocker": truncate(&worker.blocker, 1_000),
        "confidence": &worker.confidence,
        "omitted_counts": {
            "files_changed": worker.files_changed.len().saturating_sub(24),
            "commands_run": worker.commands_run.len().saturating_sub(10),
            "tests_run": worker.tests_run.len().saturating_sub(10),
            "handoff_to_rust": worker.handoff_to_rust.len().saturating_sub(8)
        }
    })
}

fn compact_string_list(values: &[String], max_items: usize, max_chars: usize) -> Vec<String> {
    values
        .iter()
        .take(max_items)
        .map(|value| truncate(value, max_chars))
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn run_senior_dev_repair(
    paths: &Paths,
    cfg: &Config,
    state: &State,
    task: &TaskSpec,
    worker: &WorkerOutput,
    guard_report: &GuardReport,
    rust_verification: &Value,
    retry_brief: &WorkerRetryBrief,
    runner: &dyn CodexRunner,
) -> Result<SeniorRepairOutput> {
    // Repair mode intentionally bypasses the broad renderer.  It sends only the
    // compact state and evidence needed to choose the next route.
    let prompt_path = paths.prompts.join("foreman_repair.md");
    write_text(&prompt_path, FOREMAN_REPAIR_PROMPT)?;
    let schema_path = paths.schemas.join("foreman_repair.schema.json");
    write_text(&schema_path, &render_schema(FOREMAN_REPAIR_SCHEMA))?;
    let review_context = build_compact_review_context_pack(
        paths,
        task,
        guard_report,
        rust_verification,
        retry_brief.fingerprint.as_ref(),
        "repair",
    );
    let prompt = render_senior_repair_prompt(
        &read_text(&prompt_path)?,
        state,
        json!({
            "role": "foreman",
            "mode": "repair",
            "task": task,
            "worker_result": worker,
            "orchestrator_guard_report": guard_report,
            "rust_verification": rust_verification,
            "retry_context": retry_brief,
            "review_assist": build_review_assist_packet(paths, task, guard_report, rust_verification),
            "review_context": review_context,
            "queue_snapshot": build_queue_snapshot_summary(paths, state),
        }),
    )?;
    let raw = run_role(
        paths,
        cfg,
        runner,
        CaRoleAuthority::ReadOnly,
        "foreman-review",
        &cfg.roles.foreman,
        Some(&task.task_id),
        &prompt,
        &schema_path,
    )?;
    parse_json_response(&raw)
}

fn run_senior_dev_batch_approval(
    paths: &Paths,
    cfg: &Config,
    state: &State,
    candidates: &[TaskSpec],
    runner: &dyn CodexRunner,
) -> Result<SeniorBatchApprovalOutput> {
    // Batch mode is a compact technical gate.  Senior gets only existing
    // candidate task cards and Rust's gate summary, never broad logs.
    let prompt_path = paths.prompts.join("foreman_batch.md");
    write_text(&prompt_path, FOREMAN_BATCH_PROMPT)?;
    let schema_path = paths.schemas.join("foreman_batch.schema.json");
    write_text(&schema_path, &render_schema(FOREMAN_BATCH_SCHEMA))?;
    append_event(
        paths,
        OrchestratorEventKind::SeniorReviewRequested,
        None,
        json!({
            "mode": "batch_approval",
            "candidate_task_ids": candidates
                .iter()
                .map(|task| task.task_id.clone())
                .collect::<Vec<_>>(),
        }),
    )?;
    let prompt = render_senior_batch_prompt(
        &read_text(&prompt_path)?,
        state,
        build_senior_batch_context(paths, state, candidates),
    )?;
    let raw = run_role(
        paths,
        cfg,
        runner,
        CaRoleAuthority::ReadOnly,
        "foreman-review",
        &cfg.roles.foreman,
        None,
        &prompt,
        &schema_path,
    )?;
    parse_json_response(&raw)
}

fn render_senior_batch_prompt(template: &str, state: &State, extra: Value) -> Result<String> {
    Ok(format!(
        r#"{template}

## Mission
{mission}

## Compact Orchestrator State
```json
{state_json}
```

## Extra run context
```json
{extra}
```

Return only the JSON object required by the provided output schema. Do not wrap it in Markdown.
"#,
        template = template,
        mission = state.mission,
        state_json = serde_json::to_string_pretty(&json!({
            "project_complete": state.project_complete,
            "completed_tasks": state.completed_tasks,
            "queue_len": state.queue.len(),
            "planner_due": state.planner_due,
            "master_due": state.master_due,
            "active_senior_batch": state.senior_batch_approval.clone(),
            "last_review_summary": state.last_review_summary.clone(),
            "last_planner_summary": state.last_planner_summary.clone(),
        }))?,
        extra = serde_json::to_string_pretty(&extra)?,
    ))
}

fn build_senior_batch_context(paths: &Paths, _state: &State, candidates: &[TaskSpec]) -> Value {
    let next_steps = read_text_or_empty(&paths.next_steps);
    let done_ids = next_steps_completed_task_ids(&next_steps)
        .into_iter()
        .collect::<Vec<_>>();
    let outputs = next_steps_outputs(&next_steps);
    json!({
        "role": "foreman",
        "mode": "batch_approval",
        "exact_question": "Can Worker continue this existing gated batch without Project Manager?",
        "rules": {
            "pm_owns": "direction, backlog creation, roadmap reshaping",
            "senior_owns": "technical safety of existing task cards",
            "rust_owns": "gate checks, scope enforcement, verification, queue state, commits",
            "worker_owns": "one selected task at a time"
        },
        "done_task_ids": done_ids,
        "known_outputs": outputs,
        "recent_rust_evidence_job_receipts": recent_rust_evidence_job_receipts(paths, 5),
        "candidate_tasks": candidates.iter().map(|task| {
            json!({
                "task_id": task.task_id.clone(),
                "title": task.title.clone(),
                "kind": task.kind.clone(),
                "origin": if task.task_id.starts_with("DIRTY-") {
                    "rust_generated_dirty_tree_continuation"
                } else {
                    "next_steps_backlog"
                },
                "batch_kind": classify_senior_batch_kind(task),
                "review_tier": task.review_tier,
                "automation_level": task.automation_level,
                "dependencies": task.dependencies.clone(),
                "objective": truncate_inline(&task.objective, 600),
                "allowed_files": task.allowed_files.clone(),
                "forbidden_actions": task.forbidden_actions.clone(),
                "verification_commands": task.tests_to_run.clone(),
                "stop_conditions": task.stop_conditions.clone(),
                "success_criteria": task.success_criteria.clone(),
            })
        }).collect::<Vec<_>>(),
        "default_batch_policy": {
            "max_tasks": 3,
            "allowed_kinds": ["docs_only", "repair", "local_snapshot", "test_only"],
            "auto_accept_policy": "low_risk_verified_only",
            "stop_on": [
                "guard violation",
                "unknown verification failure",
                "high-risk diff",
                "scope expansion",
                "missing dependency evidence"
            ]
        }
    })
}

fn render_senior_repair_prompt(template: &str, state: &State, extra: Value) -> Result<String> {
    // Keep repair prompts tiny: no full ROADMAP/NEXT_STEPS blobs, no raw logs.
    // Senior sees enough to route, not enough to reinvent the task.
    Ok(format!(
        r#"{template}

## Mission
{mission}

## Compact Orchestrator State
```json
{state_json}
```

## Extra run context
```json
{extra}
```

Return only the JSON object required by the provided output schema. Do not wrap it in Markdown.
"#,
        template = template,
        mission = state.mission,
        state_json = serde_json::to_string_pretty(&json!({
            "project_complete": state.project_complete,
            "retry_worker_same_task": state.retry_worker_same_task,
            "senior_dev_repair_order": state.senior_dev_repair_order,
            "planner_due": state.planner_due,
            "master_due": state.master_due,
            "completed_tasks": state.completed_tasks,
        }))?,
        extra = serde_json::to_string_pretty(&extra)?,
    ))
}

fn build_compact_review_context_pack(
    paths: &Paths,
    task: &TaskSpec,
    guard_report: &GuardReport,
    rust_verification: &Value,
    fingerprint: Option<&FailureFingerprint>,
    mode: &str,
) -> CompactReviewContextPack {
    // Convert local evidence into a terse decision packet.  This is the main
    // token-saver for Senior calls when auto-accept is not permitted.
    let diff_stat = git_diff_stat_for_changed_files(paths, &guard_report.changed_files);
    let risk_level = build_diff_summary(paths, &guard_report.changed_files)
        .map(|diff| classify_risk(task, &diff).as_str().to_string())
        .unwrap_or_else(|_| "unknown".to_string());
    let verification_status = rust_verification
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let mut verification_table = Vec::new();
    let mut rejected_commands = Vec::new();
    if let Some(report) = evidence_report_from_value(rust_verification) {
        for command in report.commands.iter().take(12) {
            verification_table.push(format!(
                "{} => {} exit {} in {}",
                command.command_display,
                command.category,
                command.exit_code,
                format_duration(Duration::from_millis(command.duration_ms as u64))
            ));
        }
        rejected_commands = report.rejected_commands;
    } else if let Some(commands) = rust_verification.get("commands").and_then(Value::as_array) {
        for command in commands.iter().take(12) {
            verification_table.push(truncate_inline(&command.to_string(), 180));
        }
    }
    if let Some(rejected) = rust_verification
        .get("rejected_commands")
        .and_then(Value::as_array)
    {
        rejected_commands.extend(
            rejected
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string),
        );
        rejected_commands.sort();
        rejected_commands.dedup();
    }
    let exact_question = if mode == "repair" {
        format!(
            "Choose the next bounded route for `{}`: retry same Worker task, planner, CEO, or abandon.",
            task.task_id
        )
    } else {
        format!(
            "Decide whether `{}` should be accepted from Rust evidence or rejected/escalated.",
            task.task_id
        )
    };
    CompactReviewContextPack {
        task_id: task.task_id.clone(),
        mode: mode.to_string(),
        guard_status: guard_report.status.clone(),
        changed_files: guard_report.changed_files.clone(),
        guard_violations: guard_report.violations.clone(),
        risk_level,
        diff_stat,
        verification_status,
        verification_table,
        rejected_commands,
        failure_fingerprint: fingerprint.cloned(),
        exact_question,
    }
}

fn route_attempts_path(paths: &Paths) -> PathBuf {
    paths.observations.join("route_attempts.jsonl")
}

fn route_outcomes_path(paths: &Paths) -> PathBuf {
    paths.observations.join("route_outcomes.jsonl")
}

fn route_regret_json_path(paths: &Paths) -> PathBuf {
    paths.observations.join("route_regret.json")
}

fn route_regret_md_path(paths: &Paths) -> PathBuf {
    paths.observations.join("route_regret.md")
}

fn route_learnings_path(paths: &Paths) -> PathBuf {
    paths.observations.join("route_learnings.jsonl")
}

fn route_scorecards_path(paths: &Paths) -> PathBuf {
    paths.observations.join("route_scorecards.jsonl")
}

fn read_positive_route_learnings(paths: &Paths) -> Vec<PositiveRouteLearning> {
    let path = route_learnings_path(paths);
    if !path.exists() {
        return Vec::new();
    }
    dedupe_positive_learnings(read_jsonl_file(&path).unwrap_or_default())
}

fn record_positive_route_learning(
    paths: &Paths,
    task: &TaskSpec,
    lane: ExecutionLane,
    outcome: &str,
    summary: &str,
) -> Result<()> {
    fs::create_dir_all(&paths.observations)?;
    let profile = task_route_profile(task, task_is_command_only_evidence(task), None, 0);
    let learning = PositiveRouteLearning {
        observed_at: Utc::now().to_rfc3339(),
        task_id: Some(task.task_id.clone()),
        profile_fingerprint: profile.fingerprint,
        lane,
        outcome: outcome.to_string(),
        summary: truncate_inline(summary, 500),
    };
    let mut existing = read_positive_route_learnings(paths);
    existing.push(learning);
    let deduped = dedupe_positive_learnings(existing);
    write_text(&route_learnings_path(paths), "")?;
    for learning in deduped {
        append_jsonl(
            &route_learnings_path(paths),
            &serde_json::to_value(learning)?,
        )?;
    }
    Ok(())
}

fn route_scorecard_for_task(
    paths: &Paths,
    task: &TaskSpec,
    current_lane: ExecutionLane,
    route_kind: &str,
    failure_fingerprint: Option<String>,
    estimated_prompt_tokens: usize,
) -> RouteScorecardRecord {
    let records = read_model_call_records(paths).unwrap_or_default();
    let cooldown_roles = current_tool_cooldowns(paths, &records)
        .into_iter()
        .filter(|summary| summary.cooldown_advisory)
        .map(|summary| summary.role)
        .collect::<Vec<_>>();
    let profile = task_route_profile(
        task,
        task_is_command_only_evidence(task),
        failure_fingerprint,
        estimated_prompt_tokens,
    );
    let learnings = read_positive_route_learnings(paths);
    let scorecard = score_route_lanes(&profile, &learnings, &cooldown_roles);
    RouteScorecardRecord {
        observed_at: Utc::now().to_rfc3339(),
        route_kind: route_kind.to_string(),
        task_id: Some(task.task_id.clone()),
        current_lane,
        profile,
        scorecard,
    }
}

fn record_route_scorecard(paths: &Paths, record: &RouteScorecardRecord) -> Result<()> {
    fs::create_dir_all(&paths.observations)?;
    append_jsonl(
        &route_scorecards_path(paths),
        &serde_json::to_value(record)?,
    )
}

fn route_benchmark_json_path(paths: &Paths) -> PathBuf {
    paths.observations.join("route_benchmark.json")
}

fn route_benchmark_md_path(paths: &Paths) -> PathBuf {
    paths.observations.join("route_benchmark.md")
}

fn build_route_benchmark_report(paths: &Paths) -> Result<RouteBenchmarkReport> {
    let records = read_model_call_records(paths).unwrap_or_default();
    let cooldown_roles = current_tool_cooldowns(paths, &records)
        .into_iter()
        .filter(|summary| summary.cooldown_advisory)
        .map(|summary| summary.role)
        .collect::<Vec<_>>();
    let learnings = read_positive_route_learnings(paths);
    let outcomes = read_route_outcomes(paths).unwrap_or_default();
    let mut actual_by_task = BTreeMap::new();
    for outcome in outcomes {
        if let Some(task_id) = outcome.task_id {
            actual_by_task.insert(task_id, outcome.execution_lane);
        }
    }
    let mut entries = Vec::new();
    for (name, task, safe_command_only, failure_fingerprint, estimated_tokens) in
        synthetic_route_benchmark_tasks()
    {
        let profile = task_route_profile(
            &task,
            safe_command_only,
            failure_fingerprint,
            estimated_tokens,
        );
        let scorecard = score_route_lanes(&profile, &learnings, &cooldown_roles);
        entries.push(RouteBenchmarkEntry {
            name,
            synthetic: true,
            actual_lane: None,
            recommended_lane: scorecard.recommended_lane,
            profile,
            scorecard,
        });
    }
    let next_tasks = parse_approved_next_steps_tasks(&read_text_or_empty(&paths.next_steps));
    for task in next_tasks.into_iter().take(12) {
        let profile = task_route_profile(&task, task_is_command_only_evidence(&task), None, 0);
        let scorecard = score_route_lanes(&profile, &learnings, &cooldown_roles);
        entries.push(RouteBenchmarkEntry {
            name: format!("observed:{}", task.task_id),
            synthetic: false,
            actual_lane: actual_by_task.get(&task.task_id).copied(),
            recommended_lane: scorecard.recommended_lane,
            profile,
            scorecard,
        });
    }
    Ok(RouteBenchmarkReport {
        generated_at: Utc::now().to_rfc3339(),
        profiles: entries,
    })
}

fn write_route_benchmark_report(paths: &Paths, report: &RouteBenchmarkReport) -> Result<()> {
    fs::create_dir_all(&paths.observations)?;
    write_text(
        &route_benchmark_json_path(paths),
        &format!("{}\n", serde_json::to_string_pretty(report)?),
    )?;
    write_text(
        &route_benchmark_md_path(paths),
        &render_route_benchmark_report(report),
    )
}

fn render_route_benchmark_report(report: &RouteBenchmarkReport) -> String {
    let mut out = String::new();
    out.push_str("# Route Benchmark\n\n");
    out.push_str(&format!("- Generated at: `{}`\n", report.generated_at));
    out.push_str(&format!("- Profiles: `{}`\n\n", report.profiles.len()));
    if report.profiles.is_empty() {
        out.push_str("No route profiles were available.\n");
        return out;
    }
    out.push_str("| Profile | Synthetic | Recommended | Actual | Top score |\n");
    out.push_str("|---|---:|---|---|---:|\n");
    for entry in &report.profiles {
        let top_score = entry
            .scorecard
            .scores
            .first()
            .map(|score| score.score)
            .unwrap_or_default();
        out.push_str(&format!(
            "| `{}` | `{}` | `{}` | `{}` | `{}` |\n",
            entry.name,
            entry.synthetic,
            route_lane_label(entry.recommended_lane),
            entry.actual_lane.map(route_lane_label).unwrap_or("unknown"),
            top_score
        ));
    }
    out.push_str("\nRoute benchmark is advisory and does not call Codex or execute commands.\n");
    out
}

fn synthetic_route_benchmark_tasks() -> Vec<(String, TaskSpec, bool, Option<String>, usize)> {
    vec![
        (
            "synthetic:command-only-evidence".to_string(),
            synthetic_route_benchmark_task(
                "BENCH-CMD",
                "evidence",
                "Run deterministic status evidence.",
                vec!["NEXT_STEPS.md"],
                vec!["git diff --check"],
            ),
            true,
            None,
            500,
        ),
        (
            "synthetic:docs-bookkeeping".to_string(),
            synthetic_route_benchmark_task(
                "BENCH-DOC",
                "docs",
                "Update route operations docs.",
                vec!["docs/OPERATIONS.md"],
                vec!["git diff --check"],
            ),
            false,
            None,
            800,
        ),
        (
            "synthetic:source-edit".to_string(),
            synthetic_route_benchmark_task(
                "BENCH-SRC",
                "implementation",
                "Implement a bounded source fix with verification.",
                vec!["src/lib.rs"],
                vec!["cargo test route_bench"],
            ),
            false,
            None,
            8_000,
        ),
        (
            "synthetic:repeated-fingerprint".to_string(),
            synthetic_route_benchmark_task(
                "BENCH-FAIL",
                "repair",
                "Repair repeated compile failure.",
                vec!["src/lib.rs"],
                vec!["cargo test route_bench"],
            ),
            false,
            Some("rust_type_error:src/lib.rs".to_string()),
            9_000,
        ),
    ]
}

fn synthetic_route_benchmark_task(
    task_id: &str,
    kind: &str,
    objective: &str,
    allowed_files: Vec<&str>,
    tests_to_run: Vec<&str>,
) -> TaskSpec {
    TaskSpec {
        task_id: task_id.to_string(),
        parent_task: None,
        status: "approved".to_string(),
        priority: "normal".to_string(),
        review_tier: ReviewTier::AiReviewOnFailure,
        automation_level: AutomationLevel::AiWorkerRustReview,
        dependencies: Vec::new(),
        title: objective.to_string(),
        kind: kind.to_string(),
        objective: objective.to_string(),
        why_unblocks_progress: "route benchmark fixture".to_string(),
        allowed_files: allowed_files.into_iter().map(str::to_string).collect(),
        read_only_files: Vec::new(),
        forbidden_actions: Vec::new(),
        exact_steps: Vec::new(),
        success_criteria: vec!["Verification evidence passes.".to_string()],
        tests_to_run: tests_to_run.into_iter().map(str::to_string).collect(),
        stop_conditions: vec!["Stop if scope expands beyond allowed files.".to_string()],
        discovery_budget: crate::domain::default_discovery_budget(),
        estimated_minutes: 10,
        requires_network: false,
        prefer_worktree: None,
    }
}

pub(crate) fn route_attempt_id(
    route_kind: &str,
    task_id: Option<&str>,
    lane: ExecutionLane,
) -> String {
    let seed = format!(
        "{}|{}|{:?}|{}",
        route_kind,
        task_id.unwrap_or("repo"),
        lane,
        Utc::now().timestamp_nanos_opt().unwrap_or_default()
    );
    format!("ROUTE-{}", &stable_hash_hex(&seed)[..12])
}

fn route_lane_label(lane: ExecutionLane) -> &'static str {
    match lane {
        ExecutionLane::RustEvidence => "rust_evidence",
        ExecutionLane::RustBookkeeping => "rust_bookkeeping",
        ExecutionLane::SharedWorker => "shared_worker",
        ExecutionLane::FutureWorktreeWorker => "future_worktree_worker",
        ExecutionLane::SeniorDecision => "senior_decision",
        ExecutionLane::PmDirection => "pm_direction",
        ExecutionLane::Stop => "stop",
    }
}

fn route_regret_label(kind: RouteRegretKind) -> &'static str {
    match kind {
        RouteRegretKind::AvoidableWorkerCommand => "avoidable_worker_command",
        RouteRegretKind::PmWorkerDoubleContext => "pm_worker_double_context",
        RouteRegretKind::RepeatedFingerprintNoProgress => "repeated_fingerprint_no_progress",
        RouteRegretKind::PlannerDueEmptyQueue => "planner_due_empty_queue",
        RouteRegretKind::SeniorAfterRustAutoacceptable => "senior_after_rust_autoacceptable",
        RouteRegretKind::RedundantExploration => "redundant_exploration",
    }
}

fn route_lane_for_role(role_name: &str) -> ExecutionLane {
    match role_name {
        "worker" => ExecutionLane::SharedWorker,
        "foreman" | "foreman-review" => ExecutionLane::SeniorDecision,
        "planner" | "master" => ExecutionLane::PmDirection,
        _ => ExecutionLane::Stop,
    }
}

fn route_role_tier(role_name: &str) -> &'static str {
    match role_name {
        "worker" => "worker",
        "foreman" | "foreman-review" => "senior",
        "planner" => "planner",
        "master" => "ceo",
        _ => "unknown",
    }
}

fn route_skip_feedback(
    role_name: &str,
    reason: &str,
) -> (Option<RouteRegretKind>, Option<ExecutionLane>) {
    let reason_lower = reason.to_ascii_lowercase();
    if role_name == "worker"
        && (reason_lower.contains("rust_evidence_job")
            || reason_lower.contains("mechanistic_handoff"))
    {
        return (
            Some(RouteRegretKind::AvoidableWorkerCommand),
            Some(ExecutionLane::RustEvidence),
        );
    }
    if role_name == "planner"
        && (reason_lower.contains("approved")
            || reason_lower.contains("planner_due")
            || reason_lower.contains("queued"))
    {
        return (
            Some(RouteRegretKind::PlannerDueEmptyQueue),
            Some(ExecutionLane::RustBookkeeping),
        );
    }
    if reason_lower.contains("pm_worker_doc_bounce") || reason_lower.contains("pm_control_doc") {
        return (
            Some(RouteRegretKind::PmWorkerDoubleContext),
            Some(ExecutionLane::RustBookkeeping),
        );
    }
    if reason_lower.contains("autoacceptable") || reason_lower.contains("auto-acceptable") {
        return (
            Some(RouteRegretKind::SeniorAfterRustAutoacceptable),
            Some(ExecutionLane::RustBookkeeping),
        );
    }
    if reason_lower.contains("repeated_fingerprint") {
        return (
            Some(RouteRegretKind::RepeatedFingerprintNoProgress),
            Some(ExecutionLane::SeniorDecision),
        );
    }
    (None, None)
}

pub(crate) fn record_route_attempt(paths: &Paths, attempt: &RouteAttempt) -> Result<()> {
    fs::create_dir_all(&paths.observations)?;
    append_jsonl(&route_attempts_path(paths), &serde_json::to_value(attempt)?)?;
    append_jsonl(
        &route_outcomes_path(paths),
        &serde_json::to_value(attempt.outcome())?,
    )?;
    append_event(
        paths,
        OrchestratorEventKind::RouteAttemptRecorded,
        attempt.task_id.as_deref(),
        json!({
            "attempt_id": &attempt.id,
            "route_kind": &attempt.route_kind,
            "status": attempt.status,
            "execution_lane": attempt.execution_lane,
            "regret_kind": attempt.final_feedback.as_ref().and_then(|feedback| feedback.regret_kind),
            "better_lane": attempt.final_feedback.as_ref().and_then(|feedback| feedback.better_lane),
        }),
    )?;
    Ok(())
}

fn record_route_regret_decision(
    paths: &Paths,
    task_id: Option<&str>,
    regret_kind: RouteRegretKind,
    better_lane: ExecutionLane,
    failure_fingerprint: Option<&str>,
    summary: &str,
) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    let attempt = RouteAttempt {
        id: route_attempt_id("active_regret_route", task_id, ExecutionLane::Stop),
        task_id: task_id.map(str::to_string),
        attempt: 1,
        status: RouteLifecycleStatus::Cancelled,
        started_at: now.clone(),
        finished_at: Some(now),
        execution_lane: ExecutionLane::Stop,
        route_kind: "active_regret_route".to_string(),
        model_tier: Some("rust".to_string()),
        model: None,
        prompt_receipt_id: None,
        evidence_receipt_id: None,
        failure_fingerprint: failure_fingerprint.map(str::to_string),
        token_usage: TokenUsage::default(),
        scope_group: None,
        scope_overlap: false,
        final_feedback: Some(RouteFeedback {
            outcome: "routed_before_model_call".to_string(),
            summary: summary.to_string(),
            regret_kind: Some(regret_kind),
            better_lane: Some(better_lane),
            verification_status: None,
            commit_sha: None,
            duration_ms: None,
        }),
    };
    record_route_attempt(paths, &attempt)
}

fn record_route_regret_observation(
    paths: &Paths,
    task_id: Option<&str>,
    regret_kind: RouteRegretKind,
    execution_lane: ExecutionLane,
    better_lane: ExecutionLane,
    failure_fingerprint: Option<&str>,
    summary: &str,
) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    let attempt = RouteAttempt {
        id: route_attempt_id("route_regret_observation", task_id, execution_lane),
        task_id: task_id.map(str::to_string),
        attempt: 1,
        status: RouteLifecycleStatus::Succeeded,
        started_at: now.clone(),
        finished_at: Some(now),
        execution_lane,
        route_kind: "route_regret_observation".to_string(),
        model_tier: Some(route_lane_label(execution_lane).to_string()),
        model: None,
        prompt_receipt_id: None,
        evidence_receipt_id: None,
        failure_fingerprint: failure_fingerprint.map(str::to_string),
        token_usage: TokenUsage::default(),
        scope_group: None,
        scope_overlap: false,
        final_feedback: Some(RouteFeedback {
            outcome: "observed_after_model_call".to_string(),
            summary: summary.to_string(),
            regret_kind: Some(regret_kind),
            better_lane: Some(better_lane),
            verification_status: None,
            commit_sha: None,
            duration_ms: None,
        }),
    };
    record_route_attempt(paths, &attempt)
}

fn read_route_attempts(paths: &Paths) -> Result<Vec<RouteAttempt>> {
    let path = route_attempts_path(paths);
    if !path.exists() {
        return Ok(Vec::new());
    }
    read_jsonl_file(&path)
}

fn read_route_outcomes(paths: &Paths) -> Result<Vec<RouteOutcome>> {
    let path = route_outcomes_path(paths);
    if !path.exists() {
        return Ok(Vec::new());
    }
    read_jsonl_file(&path)
}

fn estimated_saved_tokens_for_route(outcome: &RouteOutcome) -> usize {
    if outcome.token_usage.total > 0 {
        return outcome.token_usage.total;
    }
    match outcome.regret_kind {
        Some(RouteRegretKind::AvoidableWorkerCommand)
        | Some(RouteRegretKind::RepeatedFingerprintNoProgress)
        | Some(RouteRegretKind::RedundantExploration) => fallback_prompt_tokens_for_role("Worker"),
        Some(RouteRegretKind::PlannerDueEmptyQueue) => {
            fallback_prompt_tokens_for_role("Project Manager")
        }
        Some(RouteRegretKind::PmWorkerDoubleContext) => {
            fallback_prompt_tokens_for_role("Project Manager")
                + fallback_prompt_tokens_for_role("Worker")
        }
        Some(RouteRegretKind::SeniorAfterRustAutoacceptable) => {
            fallback_prompt_tokens_for_role("Senior Dev")
        }
        None => 0,
    }
}

fn build_route_regret_report(paths: &Paths) -> Result<RouteRegretReport> {
    let attempts = read_route_attempts(paths)?;
    let outcomes = read_route_outcomes(paths)?;
    let mut by_kind = BTreeMap::new();
    let mut recent = Vec::new();
    let mut estimated_tokens_saved = 0usize;
    let mut repeated_failures_stopped = 0usize;
    let mut rust_evidence_substitutions = 0usize;
    let mut redundant_explorations = 0usize;
    let mut estimated_tokens_saved_by_better_lane = BTreeMap::new();
    for outcome in outcomes
        .iter()
        .filter(|outcome| outcome.regret_kind.is_some())
    {
        let kind = outcome.regret_kind.expect("filtered some");
        let label = route_regret_label(kind).to_string();
        *by_kind.entry(label.clone()).or_insert(0) += 1;
        let estimated_tokens = estimated_saved_tokens_for_route(outcome);
        estimated_tokens_saved += estimated_tokens;
        if let Some(better_lane) = outcome.better_lane {
            *estimated_tokens_saved_by_better_lane
                .entry(route_lane_label(better_lane).to_string())
                .or_insert(0) += estimated_tokens;
        }
        if kind == RouteRegretKind::RepeatedFingerprintNoProgress {
            repeated_failures_stopped += 1;
        }
        if kind == RouteRegretKind::RedundantExploration {
            redundant_explorations += 1;
        }
        if kind == RouteRegretKind::AvoidableWorkerCommand
            && outcome.better_lane == Some(ExecutionLane::RustEvidence)
        {
            rust_evidence_substitutions += 1;
        }
        recent.push(RouteRegretEntry {
            attempt_id: outcome.attempt_id.clone(),
            task_id: outcome.task_id.clone(),
            kind: label,
            route_kind: outcome.route_kind.clone(),
            execution_lane: route_lane_label(outcome.execution_lane).to_string(),
            better_lane: outcome
                .better_lane
                .map(|lane| route_lane_label(lane).to_string()),
            tokens: estimated_tokens,
            failure_fingerprint: outcome.failure_fingerprint.clone(),
            summary: outcome.final_summary.clone(),
        });
    }
    recent.reverse();
    recent.truncate(12);
    Ok(RouteRegretReport {
        generated_at: Utc::now().to_rfc3339(),
        attempts: attempts.len(),
        outcomes: outcomes.len(),
        calls_avoided: outcomes
            .iter()
            .filter(|outcome| outcome.status == RouteLifecycleStatus::Cancelled)
            .count(),
        estimated_tokens_saved,
        repeated_failures_stopped,
        rust_evidence_substitutions,
        redundant_explorations,
        estimated_tokens_saved_by_better_lane,
        by_kind,
        recent,
    })
}

fn write_route_regret_report(paths: &Paths, report: &RouteRegretReport) -> Result<()> {
    fs::create_dir_all(&paths.observations)?;
    write_text(
        &route_regret_json_path(paths),
        &format!("{}\n", serde_json::to_string_pretty(report)?),
    )?;
    write_text(
        &route_regret_md_path(paths),
        &render_route_regret_report(report),
    )?;
    append_event(
        paths,
        OrchestratorEventKind::RouteRegretSummarized,
        None,
        json!({
            "attempts": report.attempts,
            "outcomes": report.outcomes,
            "calls_avoided": report.calls_avoided,
            "estimated_tokens_saved": report.estimated_tokens_saved,
            "repeated_failures_stopped": report.repeated_failures_stopped,
            "rust_evidence_substitutions": report.rust_evidence_substitutions,
            "redundant_explorations": report.redundant_explorations,
            "estimated_tokens_saved_by_better_lane": report.estimated_tokens_saved_by_better_lane,
        }),
    )?;
    Ok(())
}

fn render_route_regret_report(report: &RouteRegretReport) -> String {
    let mut out = String::new();
    out.push_str("# Route Regret\n\n");
    out.push_str(&format!("- Generated at: `{}`\n", report.generated_at));
    out.push_str(&format!("- Attempts: `{}`\n", report.attempts));
    out.push_str(&format!("- Outcomes: `{}`\n", report.outcomes));
    out.push_str(&format!("- Calls avoided: `{}`\n", report.calls_avoided));
    out.push_str(&format!(
        "- Estimated tokens saved: `~{}`\n",
        format_usize(report.estimated_tokens_saved)
    ));
    out.push_str(&format!(
        "- Repeated failures stopped: `{}`\n",
        report.repeated_failures_stopped
    ));
    out.push_str(&format!(
        "- Rust evidence substitutions: `{}`\n\n",
        report.rust_evidence_substitutions
    ));
    out.push_str(&format!(
        "- Redundant explorations: `{}`\n\n",
        report.redundant_explorations
    ));
    if report.by_kind.is_empty() {
        out.push_str("## Regret Kinds\n\nNo route regret recorded yet.\n\n");
    } else {
        out.push_str("## Regret Kinds\n\n");
        for (kind, count) in &report.by_kind {
            out.push_str(&format!("- `{kind}`: `{count}`\n"));
        }
        out.push('\n');
    }
    if !report.estimated_tokens_saved_by_better_lane.is_empty() {
        out.push_str("## Estimated Savings By Suggested Lane\n\n");
        for (lane, tokens) in &report.estimated_tokens_saved_by_better_lane {
            out.push_str(&format!(
                "- `{lane}`: `~{}` tokens\n",
                format_usize(*tokens)
            ));
        }
        out.push('\n');
    }
    if !report.recent.is_empty() {
        out.push_str("## Recent\n\n");
        for entry in &report.recent {
            out.push_str(&format!(
                "- `{}` `{}` via `{}`",
                entry.kind, entry.attempt_id, entry.execution_lane
            ));
            if let Some(task_id) = &entry.task_id {
                out.push_str(&format!(" task `{task_id}`"));
            }
            if let Some(better_lane) = &entry.better_lane {
                out.push_str(&format!(" better `{better_lane}`"));
            }
            if entry.tokens > 0 {
                out.push_str(&format!(" saved `~{}` tokens", format_usize(entry.tokens)));
            }
            if let Some(fingerprint) = &entry.failure_fingerprint {
                out.push_str(&format!(
                    " fingerprint `{}`",
                    truncate_inline(fingerprint, 64)
                ));
            }
            if let Some(summary) = &entry.summary {
                out.push_str(&format!(" - {}", truncate_inline(summary, 180)));
            }
            out.push('\n');
        }
        out.push('\n');
    }
    out.push_str(
        "Route memory is advisory. It stores compact routing receipts, not raw transcripts.\n",
    );
    out
}

pub(crate) fn record_rust_model_skip(
    paths: &Paths,
    role_name: &str,
    task_id: Option<&str>,
    cfg: &Config,
    reason: &str,
) -> Result<()> {
    let role = match role_name {
        "master" => &cfg.roles.master,
        "planner" => &cfg.roles.planner,
        "foreman" | "foreman-review" => &cfg.roles.foreman,
        "worker" => &cfg.roles.worker,
        _ => &cfg.roles.worker,
    };
    append_model_call_record(
        paths,
        &ModelCallRecord {
            timestamp: Utc::now().to_rfc3339(),
            role: role_name.to_string(),
            task_id: task_id.map(str::to_string),
            model: role.model.clone(),
            execution_engine: None,
            route_provider: None,
            effort: role.effort.clone(),
            prompt_token_estimate: 0,
            output_char_count: 0,
            skipped_by_rust: true,
            skip_or_call_reason: reason.to_string(),
        },
    )?;
    let lane = route_lane_for_role(role_name);
    let (regret_kind, better_lane) = route_skip_feedback(role_name, reason);
    let now = Utc::now().to_rfc3339();
    let attempt = RouteAttempt {
        id: route_attempt_id("rust_model_skip", task_id, lane),
        task_id: task_id.map(str::to_string),
        attempt: 1,
        status: RouteLifecycleStatus::Cancelled,
        started_at: now.clone(),
        finished_at: Some(now),
        execution_lane: lane,
        route_kind: "rust_model_skip".to_string(),
        model_tier: Some(route_role_tier(role_name).to_string()),
        model: Some(role.model.clone()),
        prompt_receipt_id: None,
        evidence_receipt_id: None,
        failure_fingerprint: None,
        token_usage: TokenUsage::default(),
        scope_group: None,
        scope_overlap: false,
        final_feedback: Some(RouteFeedback {
            outcome: "skipped_by_rust".to_string(),
            summary: reason.to_string(),
            regret_kind,
            better_lane,
            verification_status: None,
            commit_sha: None,
            duration_ms: None,
        }),
    };
    record_route_attempt(paths, &attempt)
}

fn print_cost_report(paths: &Paths) -> Result<()> {
    let records = read_model_call_records(paths)?;
    let learned = crate::learning::catalog_summary(paths, None).unwrap_or_default();
    let events = read_orchestrator_events(paths)?;
    let tool_cooldowns = current_tool_cooldowns(paths, &records);
    let mut by_role: HashMap<String, CostRoleStats> = HashMap::new();
    let mut prompt_samples: HashMap<String, Vec<usize>> = HashMap::new();
    let mut reasons: HashMap<String, usize> = HashMap::new();

    for record in &records {
        let role = normalize_cost_role(&record.role).to_string();
        let entry = by_role.entry(role.clone()).or_default();
        if record.skipped_by_rust {
            entry.skipped += 1;
        } else {
            entry.calls += 1;
            entry.prompt_tokens += record.prompt_token_estimate;
            entry.output_chars += record.output_char_count;
            if record.prompt_token_estimate > 0 {
                prompt_samples
                    .entry(role.clone())
                    .or_default()
                    .push(record.prompt_token_estimate);
            }
        }
        *reasons
            .entry(record.skip_or_call_reason.clone())
            .or_insert(0) += 1;
    }

    for (role, stats) in by_role.iter_mut() {
        let saved_per_skip = prompt_samples
            .get(role)
            .and_then(|samples| {
                if samples.is_empty() {
                    None
                } else {
                    Some(samples.iter().sum::<usize>() / samples.len())
                }
            })
            .unwrap_or_else(|| fallback_prompt_tokens_for_role(role));
        stats.estimated_saved_tokens = stats.skipped * saved_per_skip;
    }

    let rust_verification_commands = events
        .iter()
        .filter(|event| event.kind == OrchestratorEventKind::RecipeFinished)
        .count();
    let auto_accepts = events
        .iter()
        .filter(|event| event.kind == OrchestratorEventKind::AutoAccepted)
        .count();
    let senior_skipped = by_role
        .get("Senior Dev")
        .map(|stats| stats.skipped)
        .unwrap_or(0);
    let senior_calls = by_role
        .get("Senior Dev")
        .map(|stats| stats.calls)
        .unwrap_or(0);
    let estimated_saved = by_role
        .values()
        .map(|stats| stats.estimated_saved_tokens)
        .sum::<usize>();
    let route_report = build_route_regret_report(paths).unwrap_or_else(|_| RouteRegretReport {
        generated_at: Utc::now().to_rfc3339(),
        ..RouteRegretReport::default()
    });

    println!("codex-team cost");
    println!("repo: {}", paths.root.display());
    println!(
        "ledger: {}",
        if paths.cost.exists() {
            display_path(paths, &paths.cost)
        } else {
            "(missing .orchestrator/cost.jsonl)".to_string()
        }
    );
    println!();
    println!("Summary:");
    println!(
        "- Worker calls: {}",
        by_role.get("Worker").map(|stats| stats.calls).unwrap_or(0)
    );
    println!("- Senior Dev calls skipped by Rust: {senior_skipped}");
    println!("- Senior Dev calls made: {senior_calls}");
    println!(
        "- Planner calls: {}",
        by_role
            .get("Project Manager")
            .map(|stats| stats.calls)
            .unwrap_or(0)
    );
    println!(
        "- CEO calls: {}",
        by_role.get("CEO").map(|stats| stats.calls).unwrap_or(0)
    );
    println!("- Rust verification commands: {rust_verification_commands}");
    println!("- Rust auto-accepts: {auto_accepts}");
    println!("- Learned recipe executions: {}", learned.execution_count);
    println!("- Learned recipe quarantines: {}", learned.quarantine_count);
    println!(
        "- Learned recipe AI calls avoided: {}",
        learned.ai_calls_avoided
    );
    println!(
        "- Learned recipe estimated tokens saved: ~{}",
        format_usize(learned.estimated_tokens_avoided as usize)
    );
    println!(
        "- Tool health cooldown advisories: {}",
        tool_cooldowns
            .iter()
            .filter(|summary| summary.cooldown_advisory)
            .count()
    );
    println!(
        "- Estimated prompt tokens saved: ~{}",
        format_usize(estimated_saved)
    );
    if route_report.outcomes > 0 {
        println!("- Route attempts recorded: {}", route_report.attempts);
        println!("- Route outcomes recorded: {}", route_report.outcomes);
        println!("- Route calls avoided: {}", route_report.calls_avoided);
        println!(
            "- Route-estimated tokens saved: ~{}",
            format_usize(route_report.estimated_tokens_saved)
        );
        println!(
            "- Rust evidence substitutions: {}",
            route_report.rust_evidence_substitutions
        );
        println!(
            "- Repeated failures stopped: {}",
            route_report.repeated_failures_stopped
        );
        println!(
            "- Redundant explorations observed: {}",
            route_report.redundant_explorations
        );
        if !route_report
            .estimated_tokens_saved_by_better_lane
            .is_empty()
        {
            println!("- Route savings by suggested lane:");
            for (lane, tokens) in &route_report.estimated_tokens_saved_by_better_lane {
                println!("  - {lane}: ~{} tokens", format_usize(*tokens));
            }
        }
    }
    println!();
    println!("By role:");
    let mut roles = by_role.keys().cloned().collect::<Vec<_>>();
    roles.sort();
    for role in roles {
        let stats = &by_role[&role];
        println!(
            "- {role}: calls={} skipped={} prompt_tokens~{} output_chars={} saved~{}",
            stats.calls,
            stats.skipped,
            format_usize(stats.prompt_tokens),
            stats.output_chars,
            format_usize(stats.estimated_saved_tokens)
        );
    }
    if !reasons.is_empty() {
        println!();
        println!("Top reasons:");
        let mut reason_counts = reasons.into_iter().collect::<Vec<_>>();
        reason_counts.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        for (reason, count) in reason_counts.into_iter().take(8) {
            println!("- {count}x {reason}");
        }
    }
    if !tool_cooldowns.is_empty() {
        println!();
        println!("Tool health:");
        for summary in tool_cooldowns.iter().take(8) {
            println!(
                "- {} {} `{}` failures={} advisory={} last={}",
                normalize_cost_role(&summary.role),
                summary.model,
                summary.failure_kind.as_str(),
                summary.recent_failures,
                summary.cooldown_advisory,
                summary.last_observed_at
            );
        }
    }
    Ok(())
}

fn tool_health_path(paths: &Paths) -> PathBuf {
    paths.observations.join("tool_health.jsonl")
}

fn append_role_tool_health_record(paths: &Paths, record: &ToolHealthRecord) -> Result<()> {
    append_jsonl(&tool_health_path(paths), &serde_json::to_value(record)?)
}

fn read_tool_health_records(paths: &Paths, records: &[ModelCallRecord]) -> Vec<ToolHealthRecord> {
    let path = tool_health_path(paths);
    let mut health = if path.exists() {
        read_jsonl_file(&path).unwrap_or_default()
    } else {
        Vec::new()
    };
    if health.is_empty() {
        health.extend(synthesize_tool_health_from_cost_records(records));
    }
    // Fold supervisor hard-fail episodes into advisory tool health / route scorecards.
    health.extend(synthesize_tool_health_from_supervisor_episodes(paths));
    health
}

fn synthesize_tool_health_from_cost_records(records: &[ModelCallRecord]) -> Vec<ToolHealthRecord> {
    records
        .iter()
        .filter(|record| !record.skipped_by_rust)
        .filter_map(|record| {
            let reason = record.skip_or_call_reason.to_ascii_lowercase();
            let looks_failed = reason.contains("timed out")
                || reason.contains("exited")
                || reason.contains("failed")
                || reason.contains("missing output");
            if !looks_failed {
                return None;
            }
            let timed_out = reason.contains("timed out");
            let kind = classify_tool_failure(None, timed_out, &record.skip_or_call_reason);
            Some(ToolHealthRecord {
                observed_at: record.timestamp.clone(),
                role: record.role.clone(),
                task_id: record.task_id.clone(),
                model: record.model.clone(),
                execution_engine: record.execution_engine.clone(),
                route_provider: record.route_provider.clone(),
                status: ToolHealthStatus::Failure,
                failure_kind: Some(kind),
                exit_code: None,
                timed_out,
                excerpt: compact_tool_excerpt(&record.skip_or_call_reason, 500),
            })
        })
        .collect()
}

pub(crate) fn current_tool_cooldowns(
    paths: &Paths,
    records: &[ModelCallRecord],
) -> Vec<ToolCooldownSummary> {
    summarize_tool_cooldowns(&read_tool_health_records(paths, records))
}

fn tool_health_watch_alerts(
    paths: &Paths,
    records: &[ModelCallRecord],
) -> Vec<EfficiencyWatchAlert> {
    current_tool_cooldowns(paths, records)
        .into_iter()
        .filter(|summary| summary.cooldown_advisory)
        .take(5)
        .map(|summary| EfficiencyWatchAlert {
            severity: "warn".to_string(),
            kind: "tool_health_cooldown".to_string(),
            summary: format!(
                "{} model `{}` has recent `{}` failure(s); cooldown is advisory.",
                normalize_cost_role(&summary.role),
                summary.model,
                summary.failure_kind.as_str()
            ),
            evidence: vec![
                format!("recent_failures={}", summary.recent_failures),
                format!("last_observed_at={}", summary.last_observed_at),
                format!("excerpt={}", truncate_inline(&summary.excerpt, 180)),
            ],
            mechanistic_handoff: None,
        })
        .collect()
}

pub(crate) fn build_derived_status(paths: &Paths, state: &State) -> DerivedStatus {
    let cfg = load_config(paths).ok();
    let records = read_model_call_records(paths).unwrap_or_default();
    let cooldowns = current_tool_cooldowns(paths, &records);
    let next_steps = next_steps_state_from_text(&read_text_or_empty(&paths.next_steps));
    let next_steps_non_ai =
        next_steps_state_from_text(&read_text_or_empty(&paths.next_steps_non_ai));
    derive_status(&DerivedStatusInput {
        project_complete: state.project_complete,
        queue_len: state.queue.len(),
        approved_docs_work: next_steps.approved || next_steps_non_ai.approved,
        master_due: state.master_due,
        planner_due: state.planner_due,
        retry_worker: state.retry_worker_same_task.is_some(),
        senior_repair: state.senior_dev_repair_order.is_some(),
        senior_batch: state.senior_batch_approval.is_some(),
        human_pause: state.human_disposition_required.is_some(),
        no_signal: cfg
            .as_ref()
            .is_some_and(|cfg| recent_role_run_has_no_signal(paths, cfg, &records)),
        tool_cooldown_blocking: cooldowns.iter().any(|summary| summary.cooldown_advisory),
    })
}

fn recent_role_run_has_no_signal(paths: &Paths, cfg: &Config, records: &[ModelCallRecord]) -> bool {
    let Ok(entries) = fs::read_dir(&paths.runs) else {
        return false;
    };
    let mut prompts = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(".prompt.md"))
        })
        .filter_map(|path| {
            let modified = fs::metadata(&path).and_then(|meta| meta.modified()).ok()?;
            Some((modified, path))
        })
        .collect::<Vec<_>>();
    prompts.sort_by_key(|entry| std::cmp::Reverse(entry.0));
    prompts
        .into_iter()
        .take(6)
        .any(|(modified, prompt)| role_prompt_has_no_signal(paths, cfg, records, &prompt, modified))
}

fn role_prompt_has_no_signal(
    paths: &Paths,
    cfg: &Config,
    records: &[ModelCallRecord],
    prompt_path: &Path,
    modified: SystemTime,
) -> bool {
    let Some(file_name) = prompt_path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let Some(run_id) = file_name.strip_suffix(".prompt.md") else {
        return false;
    };
    let Some(role) = run_id.split_once('-').map(|(_, role)| role) else {
        return false;
    };
    let threshold = Duration::from_secs(role_timeout_seconds(cfg, role).saturating_add(60));
    if SystemTime::now()
        .duration_since(modified)
        .unwrap_or_default()
        < threshold
    {
        return false;
    }
    if paths.runs.join(format!("{run_id}.last.json")).exists() {
        return false;
    }
    let stdout_path = paths.runs.join(format!("{run_id}.stdout.jsonl"));
    if read_text_or_empty(&stdout_path).contains("completed") {
        return false;
    }
    let modified_rfc3339 = system_time_rfc3339(modified);
    !records.iter().any(|record| {
        record.role == role
            && record.timestamp.as_str() >= modified_rfc3339.as_str()
            && !record.skip_or_call_reason.trim().is_empty()
    })
}

fn role_timeout_seconds(cfg: &Config, role: &str) -> u64 {
    match role {
        "master" => cfg.roles.master.timeout_seconds,
        "planner" => cfg.roles.planner.timeout_seconds,
        "foreman" | "foreman-review" => cfg.roles.foreman.timeout_seconds,
        "worker" => cfg.roles.worker.timeout_seconds,
        _ => cfg.roles.worker.timeout_seconds,
    }
}

fn system_time_rfc3339(time: SystemTime) -> String {
    let datetime: DateTime<Utc> = time.into();
    datetime.to_rfc3339()
}

// ---------------------------------------------------------------------------
fn parse_json_response<T>(raw: &str) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    // Soft-hydrate first so local OpenAI-compat models (Ollama/MLX) that emit
    // null strings / invented enums can still deserialize into Rust types.
    if let Ok(value) = crate::providers::json_hydrate::parse_and_hydrate_role_json(raw) {
        return serde_json::from_value(value)
            .map_err(|_| malformed_role_output("typed validation", raw));
    }

    let trimmed = raw.trim();
    if let Ok(value) = serde_json::from_str::<T>(trimmed) {
        return Ok(value);
    }

    if let Some(fenced) = extract_json_fence(trimmed) {
        if let Ok(value) = serde_json::from_str::<T>(&fenced) {
            return Ok(value);
        }
    }

    let start = trimmed
        .find('{')
        .ok_or_else(|| malformed_role_output("missing JSON object", raw))?;
    let end = trimmed
        .rfind('}')
        .ok_or_else(|| malformed_role_output("incomplete JSON object", raw))?;
    let slice = &trimmed[start..=end];
    serde_json::from_str::<T>(slice).map_err(|_| malformed_role_output("JSON decoding", raw))
}

fn malformed_role_output(stage: &str, raw: &str) -> anyhow::Error {
    anyhow!(
        "malformed structured role output ({stage}; output_bytes={}); provider payload omitted",
        raw.len()
    )
}

fn extract_json_fence(s: &str) -> Option<String> {
    let marker = "```json";
    let start = s.find(marker)? + marker.len();
    let rest = &s[start..];
    let end = rest.find("```")?;
    Some(rest[..end].trim().to_string())
}

fn load_config(paths: &Paths) -> Result<Config> {
    if !paths.config.exists() {
        bail!(
            "missing config: run `codex-team init --repo {}` first",
            paths.root.display()
        );
    }
    let text = read_text(&paths.config)?;
    let mut cfg: Config = toml::from_str(&text)?;
    migrate_legacy_middle_role_to_planner(&mut cfg, &text);
    normalize_legacy_role_labels(&mut cfg);
    Ok(cfg)
}

fn save_config(paths: &Paths, cfg: &Config) -> Result<()> {
    write_text(&paths.config, &toml::to_string_pretty(cfg)?)
}

fn migrate_legacy_middle_role_to_planner(cfg: &mut Config, raw_config: &str) {
    if raw_config.contains("[roles.middle]") && !raw_config.contains("[roles.planner]") {
        cfg.roles.planner = cfg.roles.foreman.clone();
        cfg.roles.foreman = default_foreman_role();
    }
}

fn normalize_legacy_role_labels(cfg: &mut Config) {
    if cfg.role_labels.master == "God"
        || cfg.role_labels.master == "Top"
        || cfg.role_labels.master == "Master"
    {
        cfg.role_labels.master = default_master_label();
    }
    if cfg.role_labels.planner == "Planner"
        || cfg.role_labels.planner == "Middle"
        || cfg.role_labels.planner == "Senior Developer"
    {
        cfg.role_labels.planner = default_planner_label();
    }
    if cfg.role_labels.foreman == "Foreman"
        || cfg.role_labels.foreman == "Master"
        || cfg.role_labels.foreman == "Middle"
    {
        cfg.role_labels.foreman = default_foreman_label();
    }
    if cfg.role_labels.worker == "Slave" || cfg.role_labels.worker == "Bottom" {
        cfg.role_labels.worker = default_worker_label();
    }
    if cfg.role_labels.intern == "Bottom" || cfg.role_labels.intern == "Slave" {
        cfg.role_labels.intern = default_intern_label();
    }
}

fn load_state(paths: &Paths) -> Result<State> {
    if !paths.state.exists() {
        bail!(
            "missing state: run `codex-team init --repo {}` first",
            paths.root.display()
        );
    }
    let text = read_text(&paths.state)?;
    Ok(serde_json::from_str(&text)?)
}

fn save_state(paths: &Paths, state: &State) -> Result<()> {
    let mut state = state.clone();
    if state.recent_events.len() > 50 {
        let keep_from = state.recent_events.len() - 50;
        state.recent_events = state.recent_events.split_off(keep_from);
    }
    write_text(&paths.state, &serde_json::to_string_pretty(&state)?)
}

pub(crate) fn push_event(state: &mut State, role: &str, kind: &str, summary: &str) {
    state.recent_events.push(Event {
        ts: Utc::now().to_rfc3339(),
        role: role.to_string(),
        kind: kind.to_string(),
        summary: truncate(summary, 500),
    });
}

// ---------------------------------------------------------------------------
// Filesystem guard and persistence helpers
// ---------------------------------------------------------------------------
//
// The guard compares repo snapshots around Worker execution.  It is the local
// enforcement layer for TaskSpec.allowed_files and protects against model drift.

fn write_task_card(paths: &Paths, task: &TaskSpec) -> Result<()> {
    let path = paths
        .tasks
        .join(format!("{}.json", sanitize_filename(&task.task_id)));
    write_text(&path, &serde_json::to_string_pretty(task)?)
}

pub(crate) fn relative_slash_path(root: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    if rel.as_os_str().is_empty() {
        return Some(String::new());
    }
    Some(
        rel.components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/"),
    )
}

fn validate_codex_binary(bin: &str) -> Result<()> {
    let file_name = Path::new(bin)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(bin);
    if !file_name.starts_with("codex") {
        bail!("codex_bin must point to a codex executable, got {bin:?}");
    }
    Ok(())
}

/// Codex is opt-in per role. Do not force a Codex binary when every role uses
/// local providers (mlx_lm / ollama). Fail closed only if Codex is configured.
fn ensure_codex_ready_if_needed(cfg: &Config) -> Result<()> {
    if cfg.roles.any_role_uses(RoleProvider::Codex) {
        validate_codex_binary(&cfg.codex_bin)?;
    }
    Ok(())
}

const DOCTOR_COMMAND_TIMEOUT: Duration = Duration::from_secs(10);
const DOCTOR_COMMAND_GRACE: Duration = Duration::from_millis(500);
const DOCTOR_VERSION_OUTPUT_LIMIT: usize = 64 * 1024;
const DOCTOR_MODEL_OUTPUT_LIMIT: usize = 1024 * 1024;

fn run_bounded_ca_version_check(bin: &Path) -> Result<()> {
    run_bounded_ca_version_check_with_timeout(bin, DOCTOR_COMMAND_TIMEOUT)
}

fn run_bounded_ca_version_check_with_timeout(bin: &Path, timeout: Duration) -> Result<()> {
    run_bounded_doctor_command(
        bin,
        &["--version"],
        "ca --version",
        timeout,
        DOCTOR_VERSION_OUTPUT_LIMIT,
    )?;
    Ok(())
}

fn run_bounded_codex_version_check(bin: &Path) -> Result<()> {
    run_bounded_doctor_command(
        bin,
        &["--version"],
        "codex --version",
        DOCTOR_COMMAND_TIMEOUT,
        DOCTOR_VERSION_OUTPUT_LIMIT,
    )?;
    Ok(())
}

fn run_bounded_doctor_command(
    bin: &Path,
    args: &[&str],
    label: &str,
    timeout: Duration,
    stdout_limit: usize,
) -> Result<Vec<u8>> {
    let mut command = Command::new(bin);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            command.pre_exec(|| {
                if libc::setpgid(0, 0) != 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    let mut child = command
        .spawn()
        .map_err(|error| anyhow!("{label} launch failed ({:?})", error.kind()))?;
    let child_id = child.id();
    let stdout = child
        .stdout
        .take()
        .with_context(|| format!("{label} stdout was unavailable"))?;
    let stderr = child
        .stderr
        .take()
        .with_context(|| format!("{label} stderr was unavailable"))?;
    let stdout_reader =
        std::thread::spawn(move || read_doctor_stream_bounded(stdout, stdout_limit));
    let stderr_reader =
        std::thread::spawn(move || read_doctor_stream_bounded(stderr, DOCTOR_VERSION_OUTPUT_LIMIT));

    let started = Instant::now();
    let status = loop {
        if started.elapsed() >= timeout {
            terminate_doctor_process_group(&mut child, child_id, DOCTOR_COMMAND_GRACE);
            let _ = join_doctor_stream_reader(stdout_reader, label);
            let _ = join_doctor_stream_reader(stderr_reader, label);
            bail!("{label} timed out after {} ms", timeout.as_millis());
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(error) => {
                terminate_doctor_process_group(&mut child, child_id, DOCTOR_COMMAND_GRACE);
                let _ = join_doctor_stream_reader(stdout_reader, label);
                let _ = join_doctor_stream_reader(stderr_reader, label);
                return Err(error).with_context(|| format!("poll {label}"));
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    };

    // A malformed doctor command may leave descendants holding the pipes.
    // Reap the complete process group before joining either bounded reader.
    terminate_doctor_process_group(&mut child, child_id, DOCTOR_COMMAND_GRACE);
    let (stdout, stdout_exceeded) = join_doctor_stream_reader(stdout_reader, label)?;
    let (_, stderr_exceeded) = join_doctor_stream_reader(stderr_reader, label)?;
    if stdout_exceeded || stderr_exceeded {
        bail!("{label} output exceeded its diagnostic limit");
    }
    if !status.success() {
        bail!("{label} failed with status {status} (child output redacted)");
    }
    Ok(stdout)
}

fn read_doctor_stream_bounded(mut stream: impl Read, limit: usize) -> io::Result<(Vec<u8>, bool)> {
    let mut bytes = Vec::new();
    let mut total = 0usize;
    let mut buffer = [0u8; 8192];
    loop {
        let read = stream.read(&mut buffer)?;
        if read == 0 {
            return Ok((bytes, false));
        }
        total = total.saturating_add(read);
        if total > limit {
            return Ok((bytes, true));
        }
        bytes.extend_from_slice(&buffer[..read]);
    }
}

fn join_doctor_stream_reader(
    reader: std::thread::JoinHandle<io::Result<(Vec<u8>, bool)>>,
    label: &str,
) -> Result<(Vec<u8>, bool)> {
    reader
        .join()
        .map_err(|_| anyhow!("{label} diagnostic reader panicked"))?
        .with_context(|| format!("read bounded {label} diagnostics"))
}

fn terminate_doctor_process_group(child: &mut std::process::Child, child_id: u32, grace: Duration) {
    #[cfg(unix)]
    unsafe {
        libc::kill(-(child_id as i32), libc::SIGTERM);
    }
    #[cfg(not(unix))]
    {
        let _ = child.kill();
    }

    let started = Instant::now();
    let mut reaped = false;
    while started.elapsed() < grace {
        if !reaped && matches!(child.try_wait(), Ok(Some(_))) {
            reaped = true;
        }
        #[cfg(unix)]
        if reaped && !doctor_process_group_exists(child_id) {
            return;
        }
        #[cfg(not(unix))]
        if reaped {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }

    #[cfg(unix)]
    unsafe {
        libc::kill(-(child_id as i32), libc::SIGKILL);
    }
    #[cfg(not(unix))]
    {
        let _ = child.kill();
    }
    if !reaped {
        let _ = child.wait();
    }
}

#[cfg(unix)]
fn doctor_process_group_exists(child_id: u32) -> bool {
    let result = unsafe { libc::kill(-(child_id as i32), 0) };
    if result == 0 {
        return true;
    }
    io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
}

fn build_doctor_receipt(paths: &Paths) -> Result<DoctorReceipt> {
    let cfg = if paths.config.exists() {
        load_config(paths)?
    } else {
        Config::default()
    };

    let mut checks = Vec::new();
    let initialized = paths.orch.exists();
    checks.push(DoctorCheck::new(
        "initialized".to_string(),
        initialized,
        if initialized {
            paths.orch.display().to_string()
        } else {
            "missing .orchestrator".to_string()
        },
    ));
    checks.push(DoctorCheck::new(
        "config".to_string(),
        paths.config.exists(),
        if paths.config.exists() {
            paths.config.display().to_string()
        } else {
            "missing; using defaults for doctor".to_string()
        },
    ));

    let isolation_required = cfg.requires_verification_isolation();
    match crate::verification::isolation::require_isolation_backend() {
        Ok(readiness) => checks.push(DoctorCheck::new(
            "verification_isolation".to_string(),
            true,
            format!("{}: {}", readiness.backend, readiness.summary),
        )),
        Err(_) => checks.push(DoctorCheck::new(
            "verification_isolation".to_string(),
            !isolation_required,
            if isolation_required {
                "required verification isolation is unavailable; supervised execution and learned recipe approval are disabled".to_string()
            } else {
                "optional for this legacy configuration; learned recipe approval remains disabled".to_string()
            },
        )),
    }

    match crate::learning::catalog_summary(paths, None) {
        Ok(summary) => checks.push(DoctorCheck::new(
            "learning_catalog".to_string(),
            summary.healthy,
            format!(
                "proposals={} matching_approved={} quarantines={} executions={} ai_calls_avoided={}",
                summary.proposal_count,
                summary.approved_matching_count,
                summary.quarantine_count,
                summary.execution_count,
                summary.ai_calls_avoided
            ),
        )),
        Err(_) => checks.push(DoctorCheck::new(
            "learning_catalog".to_string(),
            false,
            "machine learning catalog is unreadable or failed integrity validation".to_string(),
        )),
    }

    if cfg.roles.any_role_uses(RoleProvider::Codex) {
        match validate_codex_binary(&cfg.codex_bin) {
            Ok(()) => match run_bounded_codex_version_check(Path::new(&cfg.codex_bin)) {
                Ok(()) => checks.push(DoctorCheck::new(
                    "codex".to_string(),
                    true,
                    format!(
                        "{} --version succeeded (child output redacted)",
                        cfg.codex_bin
                    ),
                )),
                Err(error) => checks.push(DoctorCheck::new(
                    "codex".to_string(),
                    false,
                    error.to_string(),
                )),
            },
            Err(err) => {
                checks.push(DoctorCheck::new(
                    "codex".to_string(),
                    false,
                    err.to_string(),
                ));
            }
        }
    } else {
        checks.push(DoctorCheck::new(
            "codex".to_string(),
            true,
            "skipped: no role uses provider=codex".to_string(),
        ));
    }

    if cfg.roles.any_role_uses_ca() {
        match cfg.validate_ca() {
            Err(error) => checks.push(DoctorCheck::new("ca".to_string(), false, error.to_string())),
            Ok(()) => {
                let bin =
                    fs::canonicalize(&cfg.ca.bin).unwrap_or_else(|_| PathBuf::from(&cfg.ca.bin));
                match run_bounded_ca_version_check(&bin) {
                    Ok(()) => checks.push(DoctorCheck::new(
                        "ca_version".to_string(),
                        true,
                        "ca --version succeeded (child output redacted)".to_string(),
                    )),
                    Err(error) => checks.push(DoctorCheck::new(
                        "ca_version".to_string(),
                        false,
                        error.to_string(),
                    )),
                }
                match RealCaRunner.preflight(CaProcessConfig {
                    bin: &bin,
                    profile: &cfg.ca.profile,
                    route_provider: &cfg.ca.route_provider,
                }) {
                    Ok(receipt) => {
                        let route = receipt
                            .routes
                            .iter()
                            .find(|route| route.provider == cfg.ca.route_provider);
                        let missing = [
                            ("master", &cfg.roles.master),
                            ("planner", &cfg.roles.planner),
                            ("foreman", &cfg.roles.foreman),
                            ("worker", &cfg.roles.worker),
                        ]
                        .into_iter()
                        .filter(|(_, role)| role.provider == RoleProvider::Ca)
                        .filter(|(_, role)| {
                            !route.is_some_and(|route| route.models.contains(&role.model))
                        })
                        .map(|(name, role)| format!("{name}:{}", role.model))
                        .collect::<Vec<_>>();
                        checks.push(DoctorCheck::new(
                            "ca_role_run_preflight".to_string(),
                            receipt.ready && missing.is_empty(),
                            if !receipt.ready {
                                format!("ca role-run is not ready: {}", receipt.issues.join("; "))
                            } else if !missing.is_empty() {
                                format!("ca profile is missing configured models: {}", missing.join(", "))
                            } else {
                                format!(
                                    "profile={} route={} restricted role-run ready; role sandbox/approval/web fields do not widen authority",
                                    cfg.ca.profile, cfg.ca.route_provider
                                )
                            },
                        ));
                    }
                    Err(error) => checks.push(DoctorCheck::new(
                        "ca_role_run_preflight".to_string(),
                        false,
                        error.to_string(),
                    )),
                }
            }
        }
    } else {
        checks.push(DoctorCheck::new(
            "ca".to_string(),
            true,
            "skipped: no role uses provider=ca".to_string(),
        ));
    }

    match find_git_root(&paths.root) {
        Some(root) => checks.push(DoctorCheck::new(
            "git".to_string(),
            true,
            format!("found at {}", root.display()),
        )),
        None if cfg.require_git => checks.push(DoctorCheck::new(
            "git".to_string(),
            false,
            "missing; `step` and `run` will stop until the project is a Git repo".to_string(),
        )),
        None => checks.push(DoctorCheck::new(
            "git".to_string(),
            true,
            "missing; allowed because require_git = false".to_string(),
        )),
    }

    let runtime_ignored = orchestrator_runtime_is_ignored(paths).unwrap_or(false);
    checks.push(DoctorCheck::new(
        "orchestrator_runtime_ignored".to_string(),
        !initialized || runtime_ignored,
        if !initialized {
            "skipped until init".to_string()
        } else if runtime_ignored {
            ".orchestrator/state.json is ignored by repository-local Git exclude".to_string()
        } else {
            ".orchestrator/state.json is not ignored; rerun init to repair info/exclude".to_string()
        },
    ));

    for (name, exists) in [
        ("ROADMAP.md", paths.roadmap.exists()),
        ("NEXT_STEPS.md", paths.next_steps.exists()),
        ("NEXT_STEPS_non-ai.md", paths.next_steps_non_ai.exists()),
        ("AGENTS.md", paths.agents.exists()),
        ("prompts", paths.prompts.exists()),
        ("schemas", paths.schemas.exists()),
    ] {
        checks.push(DoctorCheck::new(
            format!("project_file:{name}"),
            exists || !initialized,
            if exists {
                "present".to_string()
            } else if initialized {
                "missing".to_string()
            } else {
                "skipped until init".to_string()
            },
        ));
    }

    checks.push(DoctorCheck::new(
        "roles".to_string(),
        true,
        format!(
            "master={}/{}/{} planner={}/{}/{} foreman={}/{}/{} worker={}/{}/{}",
            cfg.roles.master.provider.as_str(),
            cfg.roles.master.model,
            cfg.roles.master.effort,
            cfg.roles.planner.provider.as_str(),
            cfg.roles.planner.model,
            cfg.roles.planner.effort,
            cfg.roles.foreman.provider.as_str(),
            cfg.roles.foreman.model,
            cfg.roles.foreman.effort,
            cfg.roles.worker.provider.as_str(),
            cfg.roles.worker.model,
            cfg.roles.worker.effort
        ),
    ));

    let ollama_needed = cfg.ollama.enabled || cfg.roles.any_role_uses_ollama();
    if cfg.roles.any_role_uses_ollama() && !cfg.ollama.enabled {
        checks.push(DoctorCheck::new(
            "ollama".to_string(),
            false,
            "a role has provider=ollama but [ollama].enabled is false".to_string(),
        ));
    } else if ollama_needed {
        let health = crate::providers::ollama::probe_ollama_health(&cfg.ollama);
        checks.push(DoctorCheck::new(
            "ollama".to_string(),
            health.reachable,
            format!(
                "base_url={} reachable={} {}",
                health.base_url, health.reachable, health.detail
            ),
        ));
    } else {
        checks.push(DoctorCheck::new(
            "ollama".to_string(),
            true,
            format!(
                "disabled (base_url={})",
                crate::providers::ollama::normalize_base_url(&cfg.ollama.base_url)
            ),
        ));
    }

    let mlx_needed = cfg.mlx_lm.enabled || cfg.roles.any_role_uses_mlx_lm();
    if cfg.roles.any_role_uses_mlx_lm() && !cfg.mlx_lm.enabled {
        checks.push(DoctorCheck::new(
            "mlx_lm".to_string(),
            false,
            "a role has provider=mlx_lm but [mlx_lm].enabled is false".to_string(),
        ));
    } else if mlx_needed {
        let health = crate::providers::mlx_lm::probe_mlx_lm_health(&cfg.mlx_lm);
        checks.push(DoctorCheck::new(
            "mlx_lm".to_string(),
            health.reachable,
            format!(
                "base_url={} reachable={} {}",
                health.base_url, health.reachable, health.detail
            ),
        ));
    } else {
        checks.push(DoctorCheck::new(
            "mlx_lm".to_string(),
            true,
            format!(
                "disabled (base_url={})",
                crate::providers::openai_compat::normalize_base_url(&cfg.mlx_lm.base_url)
            ),
        ));
    }

    let mlx_ids = if cfg.mlx_lm.enabled || cfg.roles.any_role_uses_mlx_lm() {
        crate::providers::mlx_lm::list_mlx_lm_model_ids(&cfg.mlx_lm).ok()
    } else {
        None
    };
    let ollama_ids = if cfg.ollama.enabled || cfg.roles.any_role_uses_ollama() {
        crate::providers::ollama::list_ollama_model_ids(&cfg.ollama).ok()
    } else {
        None
    };

    let codex_models = if cfg.roles.any_role_uses(RoleProvider::Codex) {
        available_codex_models(&cfg.codex_bin)
    } else {
        Ok(BTreeSet::new())
    };
    if let Ok(models) = codex_models {
        for (role, role_cfg) in [
            ("CEO", &cfg.roles.master),
            ("Project Manager", &cfg.roles.planner),
            ("Senior Dev", &cfg.roles.foreman),
            ("Worker", &cfg.roles.worker),
        ] {
            match role_cfg.provider {
                RoleProvider::MlxLm => {
                    let ok = mlx_ids
                        .as_ref()
                        .map(|ids| {
                            ids.iter()
                                .any(|id| id == &role_cfg.model || id.starts_with(&role_cfg.model))
                        })
                        .unwrap_or(false);
                    checks.push(DoctorCheck::new(
                        format!("model:{role}"),
                        ok || mlx_ids.is_none(),
                        if mlx_ids.is_none() {
                            format!(
                                "{} via mlx_lm (model list unavailable; server down?)",
                                role_cfg.model
                            )
                        } else if ok {
                            format!("{} listed by mlx_lm /v1/models", role_cfg.model)
                        } else {
                            format!("{} not listed by mlx_lm /v1/models", role_cfg.model)
                        },
                    ));
                }
                RoleProvider::Ollama => {
                    let ok = ollama_ids
                        .as_ref()
                        .map(|ids| {
                            ids.iter()
                                .any(|id| id == &role_cfg.model || id.starts_with(&role_cfg.model))
                        })
                        .unwrap_or(false);
                    checks.push(DoctorCheck::new(
                        format!("model:{role}"),
                        ok || ollama_ids.is_none(),
                        if ollama_ids.is_none() {
                            format!(
                                "{} via ollama (model list unavailable; server down?)",
                                role_cfg.model
                            )
                        } else if ok {
                            format!("{} listed by ollama /api/tags", role_cfg.model)
                        } else {
                            format!("{} not listed by ollama /api/tags", role_cfg.model)
                        },
                    ));
                }
                RoleProvider::Codex => {
                    let ok = models.contains(&role_cfg.model);
                    checks.push(DoctorCheck::new(
                        format!("model:{role}"),
                        ok,
                        if ok {
                            format!("{} available", role_cfg.model)
                        } else {
                            format!("{} not listed by `codex debug models`", role_cfg.model)
                        },
                    ));
                }
                RoleProvider::Ca => {
                    let ok = cfg.validate_ca().is_ok();
                    checks.push(DoctorCheck::new(
                        format!("model:{role}"),
                        ok,
                        if ok {
                            format!(
                                "{} delegated to ca profile {} via {}",
                                role_cfg.model, cfg.ca.profile, cfg.ca.route_provider
                            )
                        } else {
                            "invalid or disabled [ca] integration configuration".to_string()
                        },
                    ));
                }
            }
        }
    } else {
        checks.push(DoctorCheck::new(
            "model_list".to_string(),
            true,
            "unable to read `codex debug models`; skipped Codex model checks".to_string(),
        ));
        for (role, role_cfg) in [
            ("CEO", &cfg.roles.master),
            ("Project Manager", &cfg.roles.planner),
            ("Senior Dev", &cfg.roles.foreman),
            ("Worker", &cfg.roles.worker),
        ] {
            if role_cfg.provider == RoleProvider::MlxLm {
                let ok = mlx_ids
                    .as_ref()
                    .map(|ids| {
                        ids.iter()
                            .any(|id| id == &role_cfg.model || id.starts_with(&role_cfg.model))
                    })
                    .unwrap_or(false);
                checks.push(DoctorCheck::new(
                    format!("model:{role}"),
                    ok || mlx_ids.is_none(),
                    format!("{} via mlx_lm", role_cfg.model),
                ));
            } else if role_cfg.provider == RoleProvider::Ollama {
                let ok = ollama_ids
                    .as_ref()
                    .map(|ids| {
                        ids.iter()
                            .any(|id| id == &role_cfg.model || id.starts_with(&role_cfg.model))
                    })
                    .unwrap_or(false);
                checks.push(DoctorCheck::new(
                    format!("model:{role}"),
                    ok || ollama_ids.is_none(),
                    format!("{} via ollama", role_cfg.model),
                ));
            }
        }
    }

    Ok(DoctorReceipt::new(
        paths.root.display().to_string(),
        initialized,
        checks,
    ))
}

fn doctor(paths: &Paths) -> Result<()> {
    let receipt = build_doctor_receipt(paths)?;
    print_doctor_receipt(&receipt);
    let cfg = if paths.config.exists() {
        load_config(paths)?
    } else {
        Config::default()
    };
    report_role_tier_warnings(&cfg);
    report_agent_role_warnings();
    Ok(())
}

fn available_codex_models(codex_bin: &str) -> Result<BTreeSet<String>> {
    let stdout = run_bounded_doctor_command(
        Path::new(codex_bin),
        &["debug", "models"],
        "codex debug models",
        DOCTOR_COMMAND_TIMEOUT,
        DOCTOR_MODEL_OUTPUT_LIMIT,
    )?;
    let value: Value = serde_json::from_slice(&stdout)
        .context("codex debug models returned malformed bounded JSON")?;
    let models = value
        .get("models")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("codex debug models did not return a models array"))?
        .iter()
        .filter_map(|model| {
            model
                .get("slug")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .collect();
    Ok(models)
}

fn write_if_missing(path: &Path, text: &str) -> Result<()> {
    if !path.exists() {
        write_text(path, text)?;
    }
    Ok(())
}

fn read_text(path: &Path) -> Result<String> {
    crate::fs_util::read_text(path)
}

pub(crate) fn read_text_or_empty(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_default()
}

pub(crate) fn read_json_file<T>(path: &Path) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let text = read_text(path)?;
    Ok(serde_json::from_str(&text)?)
}

pub(crate) fn append_jsonl(path: &Path, value: &Value) -> Result<()> {
    crate::fs_util::append_jsonl_bounded(path, value)
}

pub(crate) fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let mut out = String::new();
    for c in s.chars().take(max_chars) {
        out.push(c);
    }
    out.push_str("\n...[truncated]...");
    out
}

// ---------------------------------------------------------------------------
// Prompt and schema contracts
// ---------------------------------------------------------------------------
//
// These constants are runtime artifacts, not just help text.  The CLI writes
// them into `.orchestrator/prompts` / `.orchestrator/schemas` on each role call
// so local runs are reproducible and inspectable.  Keep prompts aligned with the
// Rust routing rules above: AI may propose/decide, Rust owns transitions.

pub(crate) fn write_text(path: &Path, text: &str) -> Result<()> {
    crate::fs_util::write_text_atomic(path, text)
}

#[cfg(test)]
#[path = "app_tests.rs"]
mod app_tests;
