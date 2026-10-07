//! Tests for the app conductor and not-yet-extracted subsystems.
//!
//! This file remains a child module of `app`, so tests can exercise private
//! helpers while keeping production orchestration code easier to scan.

// Tests double as the executable spec for the orchestrator's safety policy.
// Prefer adding focused tests here whenever a routing/verification invariant
// changes; that keeps future prompt edits from silently weakening Rust gates.
#![allow(clippy::field_reassign_with_default)]
use super::*;
use crate::codex::prompt_pack::{
    ContextPack, PromptReceipt, PromptReceiptSection, PromptSectionCategory, PromptSectionPriority,
};
use crate::config::default_recipe_harvest_recent_logs;
use crate::domain::default_discovery_budget;
use crate::prompts::{
    FOREMAN_BATCH_SCHEMA, FOREMAN_REPAIR_PROMPT, FOREMAN_REPAIR_SCHEMA, FOREMAN_REVIEW_PROMPT,
    FOREMAN_REVIEW_SCHEMA, MASTER_SCHEMA, PLANNER_SCHEMA, TASK_SCHEMA_JSON, WORKER_SCHEMA,
};
use crate::task_import::ImportedTaskRecord;
use std::cell::RefCell;
use std::collections::{BTreeSet, VecDeque};
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_root(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("codex-team-{name}-{nonce}"))
}

fn sample_task(allowed_files: Vec<String>) -> TaskSpec {
    TaskSpec {
        task_id: "T001".to_string(),
        parent_task: None,
        status: "approved".to_string(),
        priority: "normal".to_string(),
        review_tier: ReviewTier::default(),
        automation_level: AutomationLevel::default(),
        dependencies: vec![],
        title: "sample".to_string(),
        kind: "implementation".to_string(),
        objective: "sample".to_string(),
        why_unblocks_progress: "sample".to_string(),
        allowed_files,
        read_only_files: vec![],
        forbidden_actions: vec![],
        exact_steps: vec!["edit the file".to_string()],
        success_criteria: vec!["change is present".to_string()],
        tests_to_run: vec![],
        stop_conditions: vec!["stop if blocked".to_string()],
        discovery_budget: default_discovery_budget(),
        estimated_minutes: 5,
        requires_network: false,
        prefer_worktree: None,
    }
}

fn sample_worker_output(task_id: &str) -> WorkerOutput {
    WorkerOutput {
        task_id: task_id.to_string(),
        status: "done".to_string(),
        summary: "done".to_string(),
        files_changed: Vec::new(),
        commands_run: Vec::new(),
        tests_run: Vec::new(),
        handoff_to_rust: Vec::new(),
        orchestration_request: None,
        blocker: String::new(),
        confidence: "high".to_string(),
    }
}

fn sample_prompt_receipt(task_id: &str) -> PromptReceipt {
    PromptReceipt {
        receipt_id: format!("{task_id}-receipt"),
        role: "worker".to_string(),
        task_id: Some(task_id.to_string()),
        mode: "worker_attempt".to_string(),
        generated_at: "2026-06-29T00:00:00Z".to_string(),
        hard_max_tokens: 8_000,
        total_bytes: 0,
        total_token_estimate: 0,
        estimated_context_review_seconds: 1,
        rejected: false,
        rejection_reason: None,
        sections: Vec::new(),
    }
}

fn sample_prompt_section(
    name: &str,
    category: PromptSectionCategory,
    content: &str,
) -> PromptReceiptSection {
    PromptReceiptSection {
        name: name.to_string(),
        category,
        priority: PromptSectionPriority::High,
        reason_included: "test".to_string(),
        max_bytes: content.len(),
        max_tokens: content.len().max(1),
        bytes: content.len(),
        token_estimate: content.len().max(1),
        truncated: false,
        deduplicated_from_previous: false,
        content_hash: "hash".to_string(),
        rendered_hash: "rendered".to_string(),
    }
}

fn sample_context_pack(prompt: &str) -> ContextPack {
    ContextPack {
        prompt: prompt.to_string(),
        receipt: PromptReceipt {
            sections: vec![
                sample_prompt_section("Role Prompt", PromptSectionCategory::RolePrompt, "role"),
                sample_prompt_section("Logs", PromptSectionCategory::Logs, "logs"),
                sample_prompt_section("Diff", PromptSectionCategory::Diff, "diff"),
            ],
            ..sample_prompt_receipt("TASK-1")
        },
    }
}

fn renderer_campaign_preflight_text() -> String {
    r#"
Renderer/Campaign Preflight
1. Current accepted baseline: Benchmarks/ReportsParityCampaign/current; hard-scene MAE 0.1.
2. Current first unresolved mismatch: source ownership bucket.
3. Files/artifacts proving the mismatch: analysis/current_mismatch.md row 4.
4. Solved areas that must not be reopened: crop/reassignment accepted in analysis/accepted.md.
5. Metrics that are diagnostic only: oracle, local-reference, and post-hoc metrics.
6. Product promotion gates: ordinary hard-scene, hard-edge, guard-scene, and suite gates.
7. Exact rejection condition for the next proposed change: reject on any ordinary gate regression.
"#
    .to_string()
}

#[derive(Debug)]
struct FakeCodexCall {
    role_name: String,
    task_id: Option<String>,
    schema_file: String,
    prompt: String,
}

#[derive(Debug)]
struct FakeCodexRunner {
    responses: RefCell<VecDeque<String>>,
    calls: RefCell<Vec<FakeCodexCall>>,
}

impl FakeCodexRunner {
    fn new(responses: Vec<String>) -> Self {
        Self {
            responses: RefCell::new(VecDeque::from(responses)),
            calls: RefCell::new(Vec::new()),
        }
    }
}

impl CodexRunner for FakeCodexRunner {
    fn run_codex(
        &self,
        _paths: &Paths,
        _codex_bin: &str,
        request: CodexRunRequest<'_>,
    ) -> Result<String> {
        self.calls.borrow_mut().push(FakeCodexCall {
            role_name: request.role_name.to_string(),
            task_id: request.task_id.map(str::to_string),
            schema_file: request
                .schema_path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_string(),
            prompt: request.prompt.to_string(),
        });
        self.responses
            .borrow_mut()
            .pop_front()
            .ok_or_else(|| anyhow!("fake Codex runner has no queued response"))
    }
}

struct FailingCodexRunner {
    error: String,
}

impl CodexRunner for FailingCodexRunner {
    fn run_codex(
        &self,
        _paths: &Paths,
        _codex_bin: &str,
        _request: CodexRunRequest<'_>,
    ) -> Result<String> {
        Err(anyhow!(self.error.clone()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum FakeCaAuthority {
    ReadOnly,
    Worker(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FakeCaCall {
    role_name: String,
    task_id: Option<String>,
    model: String,
    route_provider: String,
    authority: FakeCaAuthority,
}

#[derive(Debug)]
struct FakeCaRunner {
    calls: RefCell<Vec<FakeCaCall>>,
    responses: RefCell<VecDeque<String>>,
    fail: bool,
}

impl FakeCaRunner {
    fn succeeding() -> Self {
        Self {
            calls: RefCell::new(Vec::new()),
            responses: RefCell::new(VecDeque::new()),
            fail: false,
        }
    }

    fn with_responses(responses: Vec<String>) -> Self {
        Self {
            calls: RefCell::new(Vec::new()),
            responses: RefCell::new(VecDeque::from(responses)),
            fail: false,
        }
    }

    fn failing() -> Self {
        Self {
            calls: RefCell::new(Vec::new()),
            responses: RefCell::new(VecDeque::new()),
            fail: true,
        }
    }
}

impl CaRunner for FakeCaRunner {
    fn run_ca(
        &self,
        _paths: &Paths,
        config: CaProcessConfig<'_>,
        request: CaRunRequest<'_>,
        _cancellation: &CancellationToken,
    ) -> Result<String> {
        let authority = match request.authority {
            CaRoleAuthority::ReadOnly => FakeCaAuthority::ReadOnly,
            CaRoleAuthority::Worker { allowed_files } => {
                FakeCaAuthority::Worker(allowed_files.to_vec())
            }
        };
        self.calls.borrow_mut().push(FakeCaCall {
            role_name: request.role_name.to_string(),
            task_id: request.task_id.map(str::to_string),
            model: request.role.model.clone(),
            route_provider: config.route_provider.to_string(),
            authority,
        });
        if self.fail {
            bail!("fake ca role failure")
        }
        Ok(self
            .responses
            .borrow_mut()
            .pop_front()
            .unwrap_or_else(|| "{}".to_string()))
    }

    fn preflight(&self, _config: CaProcessConfig<'_>) -> Result<crate::ca::CaPreflightReceipt> {
        bail!("fake ca preflight is not used by role dispatch tests")
    }
}

#[derive(Debug)]
struct MutatingCodexRunner {
    response: String,
    path: PathBuf,
    content: String,
    calls: RefCell<Vec<FakeCodexCall>>,
}

impl MutatingCodexRunner {
    fn new(response: String, path: PathBuf, content: String) -> Self {
        Self {
            response,
            path,
            content,
            calls: RefCell::new(Vec::new()),
        }
    }
}

impl CodexRunner for MutatingCodexRunner {
    fn run_codex(
        &self,
        _paths: &Paths,
        _codex_bin: &str,
        request: CodexRunRequest<'_>,
    ) -> Result<String> {
        write_text(&self.path, &self.content)?;
        self.calls.borrow_mut().push(FakeCodexCall {
            role_name: request.role_name.to_string(),
            task_id: request.task_id.map(str::to_string),
            schema_file: request
                .schema_path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_string(),
            prompt: request.prompt.to_string(),
        });
        Ok(self.response.clone())
    }
}

fn vc(command: &str) -> VerificationCommand {
    parse_verification_command(command, "test").expect("test command should parse")
}

#[test]
fn step_refuses_live_repo_lock_before_config_load() -> Result<()> {
    let root = temp_root("step-lock-refusal");
    fs::create_dir_all(&root)?;
    let paths = Paths::new(&root)?;
    let guard = acquire_repo_lock(&paths, "test")?;

    let err = step(root.clone(), false).expect_err("step should refuse a live repo lock");

    assert!(err.to_string().contains("repo lock"));
    drop(guard);
    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn run_refuses_live_repo_lock_before_config_load() -> Result<()> {
    let root = temp_root("run-lock-refusal");
    fs::create_dir_all(&root)?;
    let paths = Paths::new(&root)?;
    let guard = acquire_repo_lock(&paths, "test")?;

    let err = run_cycles(root.clone(), Some(1), false, true)
        .expect_err("run should refuse a live repo lock");

    assert!(err.to_string().contains("repo lock"));
    drop(guard);
    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn run_auto_continue_rejects_manual_commit_mode_before_cycle_work() -> Result<()> {
    let root = temp_root("run-auto-continue-manual");
    let paths = Paths::new(&root)?;
    init_repo(&paths, "test auto-continue policy")?;
    let mut cfg = load_config(&paths)?;
    cfg.commit_mode = CommitMode::Manual;
    save_config(&paths, &cfg)?;

    let err = run_cycles(root.clone(), Some(1), false, true)
        .expect_err("manual commit mode must reject auto-continue");

    assert!(err.to_string().contains("commit_mode = \"manual\""));
    assert!(err.to_string().contains("one supervised step"));
    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn run_auto_continue_rejects_ca_role_before_ca_preflight() -> Result<()> {
    let root = temp_root("run-auto-continue-ca");
    let paths = Paths::new(&root)?;
    init_repo(&paths, "test auto-continue ca policy")?;
    let mut cfg = load_config(&paths)?;
    cfg.ca.enabled = true;
    cfg.ca.bin = "/missing/ca-is-not-consulted".to_string();
    cfg.roles.master.provider = RoleProvider::Ca;
    save_config(&paths, &cfg)?;

    let err = run_cycles(root.clone(), Some(1), false, true)
        .expect_err("a ca-backed role must reject auto-continue");

    assert!(err.to_string().contains("provider = \"ca\""));
    assert!(err.to_string().contains("one supervised step"));
    assert!(!err.to_string().contains("canonicalize"));
    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn auto_continue_policy_preserves_legacy_auto_commit_codex_mode() {
    let cfg = Config::default();
    ensure_auto_continue_allowed(&cfg, true).expect("legacy auto/Codex mode remains available");
}

#[test]
fn watch_loop_deduplicates_target_and_orchestrator_repo_lock() -> Result<()> {
    let root = temp_root("watch-loop-lock-dedup");
    fs::create_dir_all(&root)?;
    let paths = Paths::new(&root)?;

    let err = watch_loop(
        root.clone(),
        Some(root.clone()),
        "test mission".to_string(),
        1,
        false,
        1,
    )
    .expect_err("missing config should stop after deduplicated lock acquisition");

    assert!(err.to_string().contains("missing config"));
    assert!(!paths.lock.exists());
    fs::remove_dir_all(root)?;
    Ok(())
}

#[derive(Debug, serde::Deserialize, PartialEq)]
struct ParseJsonFixture {
    ok: bool,
    value: String,
}

#[test]
fn parse_json_response_accepts_direct_json() -> Result<()> {
    let parsed: ParseJsonFixture = parse_json_response(r#"{"ok":true,"value":"direct"}"#)?;

    assert_eq!(
        parsed,
        ParseJsonFixture {
            ok: true,
            value: "direct".to_string()
        }
    );
    Ok(())
}

#[test]
fn parse_json_response_accepts_fenced_json() -> Result<()> {
    let parsed: ParseJsonFixture = parse_json_response(
        "Here is the result:\n```json\n{\"ok\":true,\"value\":\"fenced\"}\n```",
    )?;

    assert_eq!(parsed.value, "fenced");
    Ok(())
}

#[test]
fn parse_json_response_extracts_json_object_from_mixed_prose() -> Result<()> {
    let parsed: ParseJsonFixture =
        parse_json_response("before {\"ok\":true,\"value\":\"mixed\"} after")?;

    assert_eq!(parsed.value, "mixed");
    Ok(())
}

#[test]
fn parse_json_response_rejects_missing_or_invalid_json() {
    let missing = parse_json_response::<ParseJsonFixture>("no object here")
        .expect_err("missing JSON should be rejected");
    assert!(missing
        .to_string()
        .contains("malformed structured role output"));

    let invalid = parse_json_response::<ParseJsonFixture>("before {not json} after")
        .expect_err("invalid JSON should be rejected");
    assert!(invalid
        .to_string()
        .contains("malformed structured role output"));
}

#[test]
fn malformed_role_output_errors_and_public_receipt_state_omit_secret_payload() -> Result<()> {
    const CANARY: &str = "ROLE_OUTPUT_SECRET_CANARY_7bc7f0";
    let root = temp_root("malformed-output-sanitization");
    let paths = Paths::new(&root)?;
    init_repo(&paths, "test sanitized role errors")?;
    let raw = format!("before {{not json: {CANARY}}} after");

    let err = parse_json_response::<ParseJsonFixture>(&raw)
        .expect_err("malformed role output must be rejected");
    let internal = format!("{err:#}");
    assert!(internal.contains("malformed structured role output"));
    assert!(!internal.contains(CANARY));
    let worker_error = parse_worker_json_response(&raw, "T-CANARY")
        .expect_err("malformed Worker output must be rejected");
    assert!(worker_error.to_string().contains("worker JSON hydration"));
    assert!(!format!("{worker_error:#}").contains(CANARY));
    assert_eq!(classify_step_error_kind(&internal), "malformed_output");
    let public = sanitize_step_error_summary("malformed_output", &raw);
    assert!(!public.contains(CANARY));
    assert!(public.len() <= 100);
    let cycle_receipt = receipts::CycleReceipt::error(
        "cycle-canary".to_string(),
        root.display().to_string(),
        None,
        paths.events.display().to_string(),
        "malformed_output",
        &public,
        Some("worker".to_string()),
        None,
    );
    assert!(!serde_json::to_string(&cycle_receipt)?.contains(CANARY));

    emit_step_error_receipt_best_effort(
        &root,
        &anyhow!("malformed structured role output: {CANARY}"),
    )?;
    let episode = read_text(&supervisor_episodes_path(&paths))?;
    let budget = read_text(&local_provider_failure_budget_path(&paths))?;
    let events = read_text(&paths.events)?;
    assert!(!episode.contains(CANARY));
    assert!(!budget.contains(CANARY));
    assert!(!events.contains(CANARY));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn provider_boundary_and_cycle_receipt_omit_codex_failure_payload() -> Result<()> {
    const CANARY: &str = "CODEX_BOUNDARY_SECRET_CANARY_31f49a";
    let root = temp_root("codex-boundary-sanitization");
    let paths = Paths::new(&root)?;
    init_repo(&paths, "test sanitized Codex boundary")?;
    let cfg = load_config(&paths)?;
    let runner = FailingCodexRunner {
        error: format!("codex worker failed: stderr/model payload {CANARY}"),
    };
    let schema = paths.schemas.join("planner.schema.json");

    let error = run_role(
        &paths,
        &cfg,
        &runner,
        CaRoleAuthority::ReadOnly,
        "planner",
        &cfg.roles.planner,
        None,
        "safe prompt",
        &schema,
    )
    .expect_err("provider failure must propagate as sanitized metadata");

    let error_text = format!("{error:#}");
    assert!(error_text.contains("failure_kind=process_failed"));
    assert!(!error_text.contains(CANARY));
    let attempts = read_text(&route_attempts_path(&paths))?;
    assert!(!attempts.contains(CANARY));
    assert!(attempts.contains("provider payload omitted"));

    let receipt_summary = sanitize_step_error_summary(
        classify_step_error_kind(&error_text),
        &format!("{error_text}: {CANARY}"),
    );
    let cycle_receipt = receipts::CycleReceipt::error(
        "cycle-codex-canary".to_string(),
        root.display().to_string(),
        None,
        paths.events.display().to_string(),
        "provider_failed",
        &receipt_summary,
        Some("planner".to_string()),
        None,
    );
    assert!(!serde_json::to_string(&cycle_receipt)?.contains(CANARY));

    emit_step_error_receipt_best_effort(&root, &error)?;
    let episodes = read_text(&supervisor_episodes_path(&paths))?;
    let events = read_text(&paths.events)?;
    assert!(!episodes.contains(CANARY));
    assert!(!events.contains(CANARY));
    assert!(episodes.contains("provider payload omitted from receipt"));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn step_once_routes_planner_through_fake_codex_runner() -> Result<()> {
    let root = temp_root("fake-codex-runner-planner");
    let paths = Paths::new(&root)?;
    init_repo(&paths, "test fake runner seam")?;
    let mut cfg = load_config(&paths)?;
    cfg.codex_bin = root
        .join("definitely-not-real-codex")
        .to_string_lossy()
        .to_string();
    cfg.live_output = false;
    let mut state = load_state(&paths)?;
    state.master_due = false;
    state.planner_due = true;
    state.needs_reconciliation = false;
    state.project_complete = false;
    state.queue.clear();
    let fake = FakeCodexRunner::new(vec![serde_json::to_string(&ProjectManagerOutput {
        project_complete: false,
        needs_master: false,
        summary: "fake planner found no approved work".to_string(),
        work_plan: PlannerWorkPlan::default(),
        tasks: Vec::new(),
        warnings: Vec::new(),
        commit_requests: Vec::new(),
    })?]);

    let should_continue = step_once_with_runner(&paths, &cfg, &mut state, &fake)?;

    assert!(!should_continue);
    let calls = fake.calls.borrow();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].role_name, "planner");
    assert_eq!(calls[0].task_id, None);
    assert_eq!(calls[0].schema_file, "planner.schema.json");
    assert!(calls[0].prompt.contains("rust_queue_brief"));
    assert_eq!(fs::read_dir(&paths.runs)?.count(), 0);

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn manual_ceo_pending_commit_stops_before_planner_in_same_cycle() -> Result<()> {
    let root = temp_root("manual-ceo-pending-stops-cycle");
    let paths = Paths::new(&root)?;
    init_repo(&paths, "test manual CEO pending stop")?;
    init_git_repo(&root)?;
    let mut cfg = load_config(&paths)?;
    cfg.commit_mode = CommitMode::Manual;
    cfg.codex_bin = root
        .join("definitely-not-real-codex")
        .to_string_lossy()
        .to_string();
    cfg.live_output = false;
    let mut state = load_state(&paths)?;
    state.master_due = true;
    state.planner_due = false;
    state.needs_reconciliation = false;
    state.project_complete = false;
    state.queue.clear();
    let runner = MutatingCodexRunner::new(
        serde_json::to_string(&json!({
            "project_complete": false,
            "needs_planner": true,
            "roadmap_status": "active",
            "summary": "Roadmap updated; Planner would normally run next.",
            "warnings": []
        }))?,
        paths.roadmap.clone(),
        "# Roadmap\n\nManual CEO update awaiting approval.\n".to_string(),
    );

    let should_continue = step_once_with_runner(&paths, &cfg, &mut state, &runner)?;

    assert!(!should_continue);
    let pending = state
        .pending_commit
        .as_ref()
        .expect("CEO control-doc update should create a manual pending intent");
    assert_eq!(pending.source_lane, "ceo_control_docs");
    assert_eq!(pending.dirty_paths, vec!["ROADMAP.md".to_string()]);
    assert_eq!(head_commit_subject(&root)?, "initial");
    let calls = runner.calls.borrow();
    assert_eq!(
        calls.len(),
        1,
        "Planner must not run after CEO creates pending"
    );
    assert_eq!(calls[0].role_name, "master");
    assert!(state.queue.is_empty());

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn manual_pm_commit_request_stops_before_enqueuing_or_worker() -> Result<()> {
    crate::verification::isolation::enable_test_isolation_backend();
    let root = temp_root("manual-pm-pending-stops-cycle");
    let paths = Paths::new(&root)?;
    init_repo(&paths, "test manual PM pending stop")?;
    write_text(&root.join("generated/receipt.txt"), "before\n")?;
    init_git_repo(&root)?;
    write_text(
        &root.join("generated/receipt.txt"),
        "before\nmanual target\n",
    )?;
    let mut cfg = load_config(&paths)?;
    cfg.commit_mode = CommitMode::Manual;
    cfg.guard.ignored_paths.push("generated/**".to_string());
    cfg.codex_bin = root
        .join("definitely-not-real-codex")
        .to_string_lossy()
        .to_string();
    cfg.live_output = false;
    let mut state = load_state(&paths)?;
    state.master_due = false;
    state.planner_due = true;
    state.needs_reconciliation = false;
    state.project_complete = false;
    state.queue.clear();
    let mut follow_on_task = sample_task(vec!["README.md".to_string()]);
    follow_on_task.task_id = "PM-FOLLOW-ON".to_string();
    follow_on_task.title = "Worker must not run".to_string();
    follow_on_task.forbidden_actions = vec!["Do not broaden scope.".to_string()];
    let mut worker = sample_worker_output(&follow_on_task.task_id);
    worker.status = "blocked".to_string();
    worker.confidence = "low".to_string();
    worker.blocker = "This response must never be consumed.".to_string();
    let runner = FakeCodexRunner::new(vec![
        serde_json::to_string(&ProjectManagerOutput {
            project_complete: false,
            needs_master: false,
            summary: "Commit the generated receipt, then send the follow-on to Worker.".to_string(),
            work_plan: PlannerWorkPlan::default(),
            tasks: vec![follow_on_task],
            warnings: Vec::new(),
            commit_requests: vec![RustCommitRequest {
                request_id: "PM-MANUAL-001".to_string(),
                title: "Commit generated receipt".to_string(),
                summary: "The generated receipt is ready for explicit commit approval.".to_string(),
                allowed_files: vec!["generated/receipt.txt".to_string()],
                verification_commands: vec![
                    "rg -n 'manual target' generated/receipt.txt".to_string()
                ],
                commit_message: "Commit manual PM generated receipt".to_string(),
                stop_conditions: vec!["Stop before any follow-on work.".to_string()],
            }],
        })?,
        serde_json::to_string(&worker)?,
    ]);

    let should_continue = step_once_with_runner(&paths, &cfg, &mut state, &runner)?;

    assert!(!should_continue);
    let pending = state
        .pending_commit
        .as_ref()
        .expect("PM commit request should create a manual pending intent");
    assert_eq!(
        pending.source_lane,
        "project_manager.role_authorized_rust_commit"
    );
    assert_eq!(
        pending.dirty_paths,
        vec!["generated/receipt.txt".to_string()]
    );
    assert_eq!(head_commit_subject(&root)?, "initial");
    let calls = runner.calls.borrow();
    assert_eq!(
        calls.len(),
        1,
        "Worker must not run after PM creates pending"
    );
    assert_eq!(calls[0].role_name, "planner");
    assert!(
        state.queue.is_empty(),
        "PM tasks must not enqueue after pending"
    );

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn planner_work_plan_is_persisted_without_extra_pm_cycles() -> Result<()> {
    let root = temp_root("planner-work-plan-persisted");
    let paths = Paths::new(&root)?;
    init_repo(&paths, "test planner work plan")?;
    let mut cfg = load_config(&paths)?;
    cfg.codex_bin = root
        .join("definitely-not-real-codex")
        .to_string_lossy()
        .to_string();
    cfg.live_output = false;
    let mut state = load_state(&paths)?;
    state.master_due = false;
    state.planner_due = true;
    state.needs_reconciliation = false;
    state.project_complete = false;
    state.queue.clear();
    let fake = FakeCodexRunner::new(vec![serde_json::to_string(&ProjectManagerOutput {
        project_complete: false,
        needs_master: false,
        summary: "PM produced a multi-step map but no immediate worker task.".to_string(),
        work_plan: PlannerWorkPlan {
            summary: "Evidence gate first, then one bounded implementation slice.".to_string(),
            goals: vec![
                "Capture a fresh benchmark receipt".to_string(),
                "Use the receipt to choose one local source edit".to_string(),
            ],
            task_sequence: vec![PlannerWorkPlanItem {
                id: "PAR-010".to_string(),
                title: "Capture bounded evidence".to_string(),
                kind: "rust_evidence".to_string(),
                status: "ready".to_string(),
                depends_on: Vec::new(),
                tests: vec!["swift build -c release --product SwiftStackerBench".to_string()],
                notes: "Rust should run the long command and persist a receipt.".to_string(),
                estimated_minutes: 20,
            }],
            test_strategy: vec!["Use fresh Benchmarks/Reports* roots.".to_string()],
            pm_return_triggers: vec!["Evidence contradicts the active phase gate.".to_string()],
        },
        tasks: Vec::new(),
        warnings: Vec::new(),
        commit_requests: Vec::new(),
    })?]);

    let should_continue = step_once_with_runner(&paths, &cfg, &mut state, &fake)?;

    assert!(!should_continue);
    assert!(paths.observations.join("planner_work_plan.md").exists());
    assert!(paths.observations.join("planner_work_plan.json").exists());
    let markdown = read_text(&paths.observations.join("planner_work_plan.md"))?;
    assert!(markdown.contains("Evidence gate first"));
    assert!(markdown.contains("PAR-010"));
    assert!(state
        .recent_events
        .iter()
        .any(|event| event.role == "planner" && event.kind == "work-plan"));
    assert_eq!(fake.calls.borrow().len(), 1);

    fs::remove_dir_all(root)?;
    Ok(())
}

fn command_evidence(command: &str, category: &str, stderr_tail: &str) -> CommandEvidence {
    let parsed = vc(command);
    CommandEvidence {
        id: "failed-command".to_string(),
        command_display: parsed.display(),
        command: parsed,
        exit_code: 1,
        success: false,
        category: category.to_string(),
        duration_ms: 1_250,
        stdout_tail: "Building for debugging...\n".to_string(),
        stderr_tail: stderr_tail.to_string(),
        evidence_summary: "exit 1".to_string(),
    }
}

fn pretty_json<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string_pretty(value).expect("support packet serializes")
}

fn evidence_report(commands: Vec<CommandEvidence>) -> EvidenceReport {
    EvidenceReport {
        report_id: "report-1".to_string(),
        plan_id: "plan-1".to_string(),
        task_id: "T001".to_string(),
        status: "commandFailed".to_string(),
        started_at: Utc::now().to_rfc3339(),
        completed_at: Utc::now().to_rfc3339(),
        summary: "failed".to_string(),
        commands,
        rejected_commands: Vec::new(),
    }
}

fn clean_guard(changed_files: Vec<String>) -> GuardReport {
    GuardReport {
        enabled: true,
        status: "ok".to_string(),
        changed_files,
        allowed_patterns: Vec::new(),
        ignored_patterns: Vec::new(),
        violations: Vec::new(),
    }
}

#[test]
fn worker_non_done_clean_first_failure_routes_to_same_task_retry() {
    let mut worker = sample_worker_output("TASK-191");
    worker.status = "blocked".to_string();
    worker.summary = "partially wired; needs one more local edit pass".to_string();
    worker.blocker = "finish AppCoordinator call-site integration before verification".to_string();
    let guard = clean_guard(vec!["Sources/App/AppCoordinator.swift".to_string()]);
    let fingerprint = failure_fingerprint_from_worker(&worker, &guard);

    assert_eq!(
        route_worker_non_done(&worker, &guard, 1, 2, Some(&fingerprint), 1),
        WorkerNonDoneRoute::RetryWorkerSameTask
    );
}

#[test]
fn worker_non_done_after_retry_budget_routes_to_senior_repair() {
    let mut worker = sample_worker_output("TASK-191");
    worker.status = "blocked".to_string();
    worker.blocker = "compile error remains in AppLoopHarness.swift".to_string();
    let guard = clean_guard(vec!["Sources/App/AppLoopHarness.swift".to_string()]);
    let fingerprint = FailureFingerprint {
        kind: FailureFingerprintKind::SwiftCompileError,
        file: Some("Sources/App/AppLoopHarness.swift".to_string()),
        line: Some(42),
        message: "compile error remains".to_string(),
        command: Some("swift test --filter AppCoordinatorTests".to_string()),
    };

    assert_eq!(
        route_worker_non_done(&worker, &guard, 3, 2, Some(&fingerprint), 3),
        WorkerNonDoneRoute::SeniorDevRepairOrder
    );
}

#[test]
fn worker_non_done_repeated_fingerprint_routes_to_senior_before_budget() {
    let mut worker = sample_worker_output("TASK-191");
    worker.status = "blocked".to_string();
    worker.blocker = "same compile error remains in AppLoopHarness.swift".to_string();
    let guard = clean_guard(vec!["Sources/App/AppLoopHarness.swift".to_string()]);
    let fingerprint = FailureFingerprint {
        kind: FailureFingerprintKind::SwiftCompileError,
        file: Some("Sources/App/AppLoopHarness.swift".to_string()),
        line: Some(42),
        message: "same compile error remains".to_string(),
        command: Some("swift test --filter AppCoordinatorTests".to_string()),
    };

    assert_eq!(
        route_worker_non_done(&worker, &guard, 2, 3, Some(&fingerprint), 2),
        WorkerNonDoneRoute::SeniorDevRepairOrder
    );
}

#[test]
fn worker_retry_no_progress_rejection_reason_blocks_repeated_fingerprint_retries() {
    let mut state = State::default();
    state.failed_tasks.insert("TASK-191".to_string(), 1);
    let mut task = sample_task(vec!["Sources/App/AppLoopHarness.swift".to_string()]);
    task.task_id = "TASK-191".to_string();
    let cfg = Config::default();
    let retry_brief = WorkerRetryBrief {
        task_id: task.task_id.clone(),
        attempt: 2,
        route: WorkerNonDoneRoute::RetryWorkerSameTask.as_str().to_string(),
        worker_status: "blocked".to_string(),
        blocker: "same compile error remains in AppLoopHarness.swift".to_string(),
        changed_files: vec!["Sources/App/AppLoopHarness.swift".to_string()],
        diff_stat: "1 file changed".to_string(),
        diagnostic_command: Some("swift test --filter AppCoordinatorTests".to_string()),
        diagnostic_category: Some("commandFailed".to_string()),
        diagnostic_summary: Some("same compile error remains".to_string()),
        fingerprint: Some(FailureFingerprint {
            kind: FailureFingerprintKind::SwiftCompileError,
            file: Some("Sources/App/AppLoopHarness.swift".to_string()),
            line: Some(42),
            message: "same compile error remains".to_string(),
            command: Some("swift test --filter AppCoordinatorTests".to_string()),
        }),
        fingerprint_retry_count: 2,
        repair_instruction: "fix only the compile error".to_string(),
        ..WorkerRetryBrief::default()
    };

    let reason = worker_retry_no_progress_rejection_reason(&state, &task, &cfg, Some(&retry_brief))
        .expect("repeated fingerprint retry should stop");

    assert!(reason.contains("same failure fingerprint repeated"));
    assert!(reason.contains("verification improvement"));
}

#[test]
fn worker_non_done_with_guard_violation_routes_to_planner() {
    let mut worker = sample_worker_output("TASK-175");
    worker.status = "blocked".to_string();
    let mut guard = clean_guard(vec!["TASK.md".to_string()]);
    guard.status = "violation".to_string();
    guard.violations = vec!["TASK.md".to_string()];

    assert_eq!(
        route_worker_non_done(&worker, &guard, 1, 2, None, 1),
        WorkerNonDoneRoute::PlannerDue
    );
}

#[test]
fn worker_blocked_for_unapproved_dirty_scope_routes_to_planner() {
    let mut worker = sample_worker_output("TASK-237");
    worker.status = "blocked".to_string();
    worker.summary =
        "Classified post-closure dirty tree and found unapproved Session public-observation changes."
            .to_string();
    worker.blocker = "`TASK-237` is Blocked because the dirty worktree contains unapproved Session public-observation behavior/configuration/test and packaging/docs changes across `Sources/`, `Tests/`, and public diagnostic runbook/tooling areas, indicating a new approval/safety gate is required before any further Worker implementation.".to_string();
    let guard = clean_guard(vec![
        "NEXT_STEPS.md".to_string(),
        "CONTINUITY.md".to_string(),
        "TASK.md".to_string(),
    ]);
    let fingerprint = failure_fingerprint_from_worker(&worker, &guard);

    assert_eq!(
        fingerprint.kind,
        FailureFingerprintKind::PolicyScopeViolation
    );
    assert_eq!(
        route_worker_non_done(&worker, &guard, 1, 2, Some(&fingerprint), 1),
        WorkerNonDoneRoute::PlannerDue
    );
}

#[test]
fn retry_summary_extracts_first_useful_compiler_error() {
    let evidence = CommandEvidence {
        id: "swift-test".to_string(),
        command: vc("swift test --filter AppCoordinatorTests"),
        command_display: "swift test --filter AppCoordinatorTests".to_string(),
        exit_code: 1,
        success: false,
        category: "commandFailed".to_string(),
        duration_ms: 1_250,
        stdout_tail: "Building for debugging...\n".to_string(),
        stderr_tail: "/tmp/Foo.swift:10:5: error: missing argument label\n".to_string(),
        evidence_summary: "exit 1".to_string(),
    };

    let summary = command_evidence_retry_summary(&evidence);
    assert!(summary.contains("missing argument label"));
    assert!(summary.contains("commandFailed"));
}

#[test]
fn failure_fingerprint_classifies_common_mechanical_patterns() {
    let swift = failure_fingerprint_from_text(
        Some("swift test --filter AppCoordinatorTests"),
        Some("commandFailed"),
        "/tmp/Foo.swift:10:5: error: missing argument label 'bar:' in call",
    );
    assert_eq!(
        swift.kind,
        FailureFingerprintKind::SwiftInitializerLabelMismatch
    );
    assert_eq!(swift.file.as_deref(), Some("/tmp/Foo.swift"));
    assert_eq!(swift.line, Some(10));

    let lock = failure_fingerprint_from_text(
        Some("swift test --filter FooTests"),
        Some("commandFailed"),
        "Another instance of SwiftPM is already running using '/repo/.build', waiting",
    );
    assert_eq!(lock.kind, FailureFingerprintKind::SwiftPmLockContention);

    let patch = failure_fingerprint_from_text(
        None,
        None,
        "Tool note: apply_patch context mismatch; raw stderr log has details",
    );
    assert_eq!(
        patch.kind,
        FailureFingerprintKind::ApplyPatchContextMismatch
    );

    let rg = failure_fingerprint_from_text(Some("rg -n Needle README.md"), Some("noMatches"), "");
    assert_eq!(rg.kind, FailureFingerprintKind::RipgrepNoMatches);

    let test = failure_fingerprint_from_text(
        Some("swift test --filter AppCoordinatorTests/testSpecificFailure"),
        Some("commandFailed"),
        "/tmp/AppCoordinatorTests.swift:9868: error: -[AppCoordinatorTests testSpecificFailure] : XCTAssertEqual failed: (\"0\") is not equal to (\"1\")",
    );
    assert_eq!(test.kind, FailureFingerprintKind::TestAssertionFailure);
}

#[test]
fn swift_argument_label_errors_classify_as_initializer_label_mismatch() {
    let output = r#"
/Users/me/AppLoopHarness.swift:857:43: error: incorrect argument labels in call (have 'tickCount:processedDeliveryCount:loopbackSendIntentCount:', expected 'tickCount:processedDeliveryCount:')
/Users/me/AppLoopHarness.swift:788:50: error: extra arguments at positions #42, #43, #44 in call
/Users/me/AppLoopHarness.swift:496:9: error: invalid redeclaration of 'loopbackSendIntentCount'
"#;

    let fingerprint = failure_fingerprint_from_text(
        Some("swift test --filter AppLoopHarnessTests"),
        Some("commandFailed"),
        output,
    );
    assert_eq!(
        fingerprint.kind,
        FailureFingerprintKind::SwiftInitializerLabelMismatch
    );
    assert_eq!(
        fingerprint.file.as_deref(),
        Some("/Users/me/AppLoopHarness.swift")
    );
    assert_eq!(fingerprint.line, Some(857));
}

#[test]
fn compiler_error_extraction_keeps_top_three_file_line_messages() {
    let output = r#"
/repo/A.swift:1:2: error: first failure
/repo/B.swift:3:4: error: second failure
/repo/C.swift:5:6: error: third failure
/repo/D.swift:7:8: error: fourth failure
"#;
    let errors = compiler_errors_from_text(output);

    assert_eq!(errors.len(), 3);
    assert_eq!(errors[0].file.as_deref(), Some("/repo/A.swift"));
    assert_eq!(errors[0].line, Some(1));
    assert_eq!(errors[0].column, Some(2));
    assert_eq!(errors[0].message, "first failure");
    assert_eq!(errors[2].message, "third failure");
}

#[test]
fn initializer_label_repair_instruction_is_specific() {
    let task = sample_task(vec!["Sources/App/Foo.swift".to_string()]);
    let fingerprint = FailureFingerprint {
        kind: FailureFingerprintKind::SwiftInitializerLabelMismatch,
        file: Some("Sources/App/Foo.swift".to_string()),
        line: Some(42),
        message: "extra argument in call".to_string(),
        command: Some("swift test --filter FooTests".to_string()),
    };
    let instruction = repair_instruction_for_fingerprint(&task, &fingerprint);

    assert!(instruction.contains("Align stored properties"));
    assert!(instruction.contains("public init parameters"));
    assert!(instruction.contains("constructor call sites"));
    assert!(instruction.contains("Do not add duplicate fields"));
}

#[test]
fn repeated_apply_patch_mismatches_count_for_retry_hint() {
    let text = "\
Tool note: apply_patch context mismatch; raw stderr log has details
patch rejected: writing outside context
Tool note: apply_patch context mismatch; raw stderr log has details
";
    assert!(patch_context_mismatch_count(text) >= 2);
}

#[test]
fn environment_fingerprint_does_not_same_task_retry() {
    let worker = sample_worker_output("TASK-200");
    let guard = clean_guard(vec!["Sources/App/Foo.swift".to_string()]);
    let fingerprint = FailureFingerprint {
        kind: FailureFingerprintKind::KnownEnvironmentBlocker,
        file: None,
        line: None,
        message: "operation not permitted while binding loopback".to_string(),
        command: Some("swift test --filter Smoke".to_string()),
    };

    assert_eq!(
        route_worker_non_done(&worker, &guard, 1, 2, Some(&fingerprint), 1),
        WorkerNonDoneRoute::PlannerDue
    );
}

#[test]
fn worker_done_swift_compile_verification_failure_retries_same_task() {
    let failed = command_evidence(
        "swift test --filter AppCoordinatorTests",
        "commandFailed",
        "/tmp/Foo.swift:10:5: error: missing argument label 'bar:' in call",
    );
    let report = evidence_report(vec![failed.clone()]);
    let guard = clean_guard(vec!["Sources/App/Foo.swift".to_string()]);
    let fingerprint = failure_fingerprint_from_command_evidence(&failed);

    assert_eq!(
        first_failed_command(&report).map(|cmd| &cmd.id),
        Some(&failed.id)
    );
    assert_eq!(
        route_worker_done_verification_failure(
            &report,
            &guard,
            &failed,
            1,
            2,
            Some(&fingerprint),
            1
        ),
        WorkerNonDoneRoute::RetryWorkerSameTask
    );
}

#[test]
fn worker_done_focused_test_failure_retries_once_even_when_unknown() {
    let failed = command_evidence(
        "swift test --filter AppCoordinatorTests/testSpecificFailure",
        "commandFailed",
        "Test Case '-[AppCoordinatorTests testSpecificFailure]' failed.",
    );
    let report = evidence_report(vec![failed.clone()]);
    let guard = clean_guard(vec!["Tests/AppTests/FooTests.swift".to_string()]);
    let fingerprint = FailureFingerprint {
        kind: FailureFingerprintKind::Unknown,
        file: None,
        line: None,
        message: "test assertion failed".to_string(),
        command: Some(failed.command_display.clone()),
    };

    assert_eq!(
        route_worker_done_verification_failure(
            &report,
            &guard,
            &failed,
            1,
            3,
            Some(&fingerprint),
            1
        ),
        WorkerNonDoneRoute::RetryWorkerSameTask
    );
    assert_eq!(
        route_worker_done_verification_failure(
            &report,
            &guard,
            &failed,
            2,
            3,
            Some(&fingerprint),
            2
        ),
        WorkerNonDoneRoute::SeniorDevRepairOrder
    );
}

#[test]
fn worker_done_verification_rejected_commands_do_not_retry() {
    let failed = command_evidence("swift test --filter FooTests", "commandFailed", "failure");
    let mut report = evidence_report(vec![failed.clone()]);
    report.rejected_commands = vec!["pwd && swift test".to_string()];
    let guard = clean_guard(vec!["Sources/App/Foo.swift".to_string()]);
    let fingerprint = failure_fingerprint_from_command_evidence(&failed);

    assert_eq!(
        route_worker_done_verification_failure(
            &report,
            &guard,
            &failed,
            1,
            3,
            Some(&fingerprint),
            1
        ),
        WorkerNonDoneRoute::SeniorDevRepairOrder
    );
}

#[test]
fn worker_done_environment_verification_failure_routes_away_from_retry() {
    let failed = command_evidence(
        "swift test --filter SmokeTests",
        "knownEnvironmentBlocker",
        "Operation not permitted while binding UDP loopback",
    );
    let report = evidence_report(vec![failed.clone()]);
    let guard = clean_guard(vec!["Sources/App/Foo.swift".to_string()]);
    let fingerprint = failure_fingerprint_from_command_evidence(&failed);

    assert_eq!(
        route_worker_done_verification_failure(
            &report,
            &guard,
            &failed,
            1,
            3,
            Some(&fingerprint),
            1
        ),
        WorkerNonDoneRoute::PlannerDue
    );
}

#[test]
fn verification_failure_retry_brief_includes_failed_command_receipt() -> Result<()> {
    let root = temp_root("done-verification-retry-brief");
    fs::create_dir_all(root.join(".orchestrator/runs"))?;
    let paths = Paths::new(&root)?;
    let mut task = sample_task(vec!["Sources/App/Foo.swift".to_string()]);
    task.task_id = "TASK-191".to_string();
    let worker = sample_worker_output(&task.task_id);
    let guard = clean_guard(vec!["Sources/App/Foo.swift".to_string()]);
    let failed = command_evidence(
        "swift test --filter FooTests",
        "commandFailed",
        "/tmp/Foo.swift:10:5: error: missing argument label 'bar:' in call\n/tmp/Foo.swift:11:5: error: extra argument in call",
    );
    let report = evidence_report(vec![failed.clone()]);

    let brief = build_verification_failure_retry_brief(
        &paths, &task, &worker, &guard, &report, &failed, 1, 1,
    );

    assert_eq!(brief.worker_status, "done_but_verification_failed");
    assert_eq!(
        brief.diagnostic_command.as_deref(),
        Some("swift test --filter FooTests")
    );
    assert!(brief
        .diagnostic_summary
        .as_deref()
        .unwrap_or_default()
        .contains("missing argument label"));
    assert_eq!(brief.top_compiler_errors.len(), 2);
    assert!(brief
        .repair_instruction
        .contains("Rust final verification failed"));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn worker_retry_diagnostic_prefers_failed_command_from_worker_log() {
    let log = r#"
{"msg":"Running swift test --filter AppLoopHarnessTests/testOneTickProcessesLoopbackSendIntentSnapshotsOncePerPreparedRecord"}
{"msg":"Ran swift test --filter AppLoopHarnessTests/testOneTickProcessesLoopbackSendIntentSnapshotsOncePerPreparedRecord for 568ms"}
{"msg":"Failed swift test --filter AppCoordinatorTests/testLoopbackSendAdapterHandoffSurfacesSanitizedAdminEvidence exit 1 for 537ms\n  /tmp/AppCoordinatorTests.swift:9868: error: XCTAssertEqual failed"}
"#;

    let command = worker_log_failed_verification_command(log)
        .expect("failed AppCoordinator command should be extracted");
    assert_eq!(
        command.display(),
        "swift test --filter AppCoordinatorTests/testLoopbackSendAdapterHandoffSurfacesSanitizedAdminEvidence"
    );
}

#[test]
fn blocked_dirty_retry_diagnostic_skips_diff_check_for_build_command() -> Result<()> {
    let root = temp_root("blocked-dirty-retry-diagnostic");
    fs::create_dir_all(root.join(".orchestrator/runs"))?;
    let paths = Paths::new(&root)?;
    let mut task = sample_task(vec!["Sources/App/Foo.swift".to_string()]);
    task.tests_to_run = vec![
        "git diff --check".to_string(),
        "swift build --product app".to_string(),
    ];
    let mut worker = sample_worker_output(&task.task_id);
    worker.status = "blocked".to_string();
    worker.files_changed = vec!["Sources/App/Foo.swift".to_string()];

    let command = select_worker_retry_diagnostic_command(&paths, &task, &worker)
        .expect("dirty blocked task should get a retry diagnostic");

    assert_eq!(command.display(), "swift build --product app");

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn unknown_retry_fingerprint_refines_from_worker_log_compiler_error() {
    let unknown = FailureFingerprint {
        kind: FailureFingerprintKind::Unknown,
        file: None,
        line: None,
        message: "command exited 1: Building for debugging...".to_string(),
        command: Some("swift build --product app".to_string()),
    };
    let combined = r#"
Worker blocker: build failed after partial edits.
/tmp/Project/Sources/App/SessionCycleResult.swift:74:9: error: invalid redeclaration of 'sessionCloseReason'
"#;

    let refined = refine_unknown_fingerprint_from_text(
        unknown,
        Some("swift build --product app"),
        Some("commandFailed"),
        combined,
    );

    assert_eq!(
        refined.kind,
        FailureFingerprintKind::SwiftInitializerLabelMismatch
    );
    assert_eq!(refined.line, Some(74));
    assert!(refined
        .message
        .contains("invalid redeclaration of 'sessionCloseReason'"));
}

#[test]
fn retry_brief_refreshes_stale_passed_diagnostic_from_latest_worker_log() {
    let root = temp_root("refresh-stale-retry-brief");
    fs::create_dir_all(root.join(".orchestrator/runs")).unwrap();
    write_text(
        &root.join(".orchestrator/runs/20260428T000000Z-worker.stdout.jsonl"),
        r#"
{"msg":"Failed swift test --filter AppCoordinatorTests/testLoopbackSendAdapterHandoffSurfacesSanitizedAdminEvidence exit 1 for 537ms\n  /tmp/AppCoordinatorTests.swift:9868: error: -[AppCoordinatorTests testLoopbackSendAdapterHandoffSurfacesSanitizedAdminEvidence] : XCTAssertEqual failed: (\"0\") is not equal to (\"1\")"}
"#,
    )
    .unwrap();
    let paths = Paths::new(&root).unwrap();
    let task = sample_task(vec!["Sources/App/Foo.swift".to_string()]);
    let stale = WorkerRetryBrief {
        task_id: task.task_id.clone(),
        attempt: 1,
        route: WorkerNonDoneRoute::RetryWorkerSameTask.as_str().to_string(),
        worker_status: "blocked".to_string(),
        diagnostic_command: Some("swift test --filter AppLoopHarnessTests/testOneTickProcessesLoopbackSendIntentSnapshotsOncePerPreparedRecord".to_string()),
        diagnostic_category: Some("passed".to_string()),
        diagnostic_summary: Some("`swift test --filter AppLoopHarnessTests/testOneTickProcessesLoopbackSendIntentSnapshotsOncePerPreparedRecord` -> `passed` exit 0".to_string()),
        ..WorkerRetryBrief::default()
    };

    let refreshed = refresh_retry_brief_from_latest_worker_log(&paths, &task, stale);

    assert_eq!(
        refreshed.diagnostic_command.as_deref(),
        Some("swift test --filter AppCoordinatorTests/testLoopbackSendAdapterHandoffSurfacesSanitizedAdminEvidence")
    );
    assert_eq!(
        refreshed
            .fingerprint
            .as_ref()
            .map(|fingerprint| fingerprint.kind),
        Some(FailureFingerprintKind::TestAssertionFailure)
    );
    assert!(refreshed
        .repair_instruction
        .contains("Rust refreshed this retry"));

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn verification_failure_retry_skip_reason_is_recorded_in_cost_ledger() -> Result<()> {
    let root = temp_root("done-verification-cost-skip");
    fs::create_dir_all(root.join(".orchestrator"))?;
    let paths = Paths::new(&root)?;
    let cfg = Config::default();

    record_rust_model_skip(
        &paths,
        "foreman-review",
        Some("TASK-191"),
        &cfg,
        "done_worker_verification_failed_retry",
    )?;

    let cost = read_text(&paths.cost)?;
    assert!(cost.contains("\"role\":\"foreman-review\""));
    assert!(cost.contains("\"skipped_by_rust\":true"));
    assert!(cost.contains("done_worker_verification_failed_retry"));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn route_attempt_persistence_writes_attempt_outcome_and_regret_report() -> Result<()> {
    let root = temp_root("route-attempt-persistence");
    fs::create_dir_all(root.join(".orchestrator"))?;
    let paths = Paths::new(&root)?;
    let now = Utc::now().to_rfc3339();
    let attempt = RouteAttempt {
        id: "ROUTE-test".to_string(),
        task_id: Some("PAR-004".to_string()),
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
        failure_fingerprint: Some("swift_compile_error|Foo.swift|42|same".to_string()),
        token_usage: TokenUsage::default(),
        scope_group: None,
        scope_overlap: false,
        final_feedback: Some(RouteFeedback {
            outcome: "routed_before_model_call".to_string(),
            summary: "same failure repeated".to_string(),
            regret_kind: Some(RouteRegretKind::RepeatedFingerprintNoProgress),
            better_lane: Some(ExecutionLane::SeniorDecision),
            verification_status: None,
            commit_sha: None,
            duration_ms: None,
        }),
    };

    record_route_attempt(&paths, &attempt)?;
    let attempts = read_route_attempts(&paths)?;
    let outcomes = read_route_outcomes(&paths)?;
    assert_eq!(attempts.len(), 1);
    assert_eq!(outcomes.len(), 1);
    assert_eq!(
        outcomes[0].regret_kind,
        Some(RouteRegretKind::RepeatedFingerprintNoProgress)
    );

    let report = build_route_regret_report(&paths)?;
    assert_eq!(report.attempts, 1);
    assert_eq!(report.calls_avoided, 1);
    assert_eq!(report.repeated_failures_stopped, 1);
    assert!(report.estimated_tokens_saved >= fallback_prompt_tokens_for_role("Worker"));
    write_route_regret_report(&paths, &report)?;
    let markdown = read_text(&route_regret_md_path(&paths))?;
    assert!(markdown.contains("# Route Regret"));
    assert!(markdown.contains("repeated_fingerprint_no_progress"));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn route_regret_records_mechanistic_worker_skip_as_rust_evidence_substitution() -> Result<()> {
    let root = temp_root("route-regret-worker-skip");
    fs::create_dir_all(root.join(".orchestrator"))?;
    let paths = Paths::new(&root)?;
    let cfg = Config::default();

    record_rust_model_skip(
        &paths,
        "worker",
        Some("BENCH-001"),
        &cfg,
        "mechanistic_handoff_rust_evidence_job",
    )?;

    let report = build_route_regret_report(&paths)?;
    assert_eq!(report.calls_avoided, 1);
    assert_eq!(report.rust_evidence_substitutions, 1);
    assert_eq!(
        report.by_kind.get("avoidable_worker_command").copied(),
        Some(1)
    );

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn route_regret_records_redundant_worker_exploration_once_per_fingerprint() -> Result<()> {
    let root = temp_root("route-regret-redundant-exploration");
    fs::create_dir_all(root.join(".orchestrator/runs"))?;
    let paths = Paths::new(&root)?;
    append_model_call_record(
        &paths,
        &ModelCallRecord {
            timestamp: "2026-07-06T14:00:00Z".to_string(),
            role: "worker".to_string(),
            task_id: Some("READ-001".to_string()),
            model: "gpt-5.4-mini".to_string(),
            execution_engine: None,
            route_provider: None,
            effort: "medium".to_string(),
            prompt_token_estimate: 4_200,
            output_char_count: 600,
            skipped_by_rust: false,
            skip_or_call_reason: "called".to_string(),
        },
    )?;
    write_text(
        &paths.runs.join("20260706T140000Z-worker.stdout.jsonl"),
        &[
            r#"{"type":"item.completed","item":{"type":"command_execution","command":"/bin/zsh -lc 'sed -n '\"'\"'1,80p'\"'\"' README.md'","exit_code":0}}"#,
            r#"{"type":"item.completed","item":{"type":"command_execution","command":"/bin/zsh -lc 'sed -n '\"'\"'1,80p'\"'\"' README.md'","exit_code":0}}"#,
            r#"{"type":"item.completed","item":{"type":"command_execution","command":"cargo test route_regret","exit_code":0}}"#,
        ]
        .join("\n"),
    )?;
    let records = read_model_call_records(&paths)?;

    assert_eq!(
        record_redundant_exploration_from_worker_logs(&paths, &records),
        1
    );
    assert_eq!(
        record_redundant_exploration_from_worker_logs(&paths, &records),
        0
    );

    let report = build_route_regret_report(&paths)?;
    assert_eq!(report.calls_avoided, 0);
    assert_eq!(report.redundant_explorations, 1);
    assert_eq!(
        report.by_kind.get("redundant_exploration").copied(),
        Some(1)
    );
    assert_eq!(
        report
            .estimated_tokens_saved_by_better_lane
            .get("rust_bookkeeping")
            .copied(),
        Some(fallback_prompt_tokens_for_role("Worker"))
    );
    let entry = report
        .recent
        .iter()
        .find(|entry| entry.kind == "redundant_exploration")
        .expect("recent redundant exploration entry");
    assert!(entry.failure_fingerprint.is_some());
    assert!(entry
        .summary
        .as_deref()
        .is_some_and(|summary| summary.contains("sed -n")));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn route_regret_dedupes_redundant_worker_exploration_by_task_and_fingerprint() -> Result<()> {
    let root = temp_root("route-regret-redundant-exploration-tasks");
    fs::create_dir_all(root.join(".orchestrator/runs"))?;
    let paths = Paths::new(&root)?;
    for (timestamp, task_id, run_id) in [
        (
            "2026-07-06T13:00:00Z",
            "READ-OLD",
            "20260706T130000Z-worker.stdout.jsonl",
        ),
        (
            "2026-07-06T14:00:00Z",
            "READ-NEW",
            "20260706T140000Z-worker.stdout.jsonl",
        ),
    ] {
        append_model_call_record(
            &paths,
            &ModelCallRecord {
                timestamp: timestamp.to_string(),
                role: "worker".to_string(),
                task_id: Some(task_id.to_string()),
                model: "gpt-5.4-mini".to_string(),
                execution_engine: None,
                route_provider: None,
                effort: "medium".to_string(),
                prompt_token_estimate: 4_200,
                output_char_count: 600,
                skipped_by_rust: false,
                skip_or_call_reason: "called".to_string(),
            },
        )?;
        write_text(
            &paths.runs.join(run_id),
            &[
                r#"{"type":"item.completed","item":{"type":"command_execution","command":"cat README.md","exit_code":0}}"#,
                r#"{"type":"item.completed","item":{"type":"command_execution","command":"cat README.md","exit_code":0}}"#,
            ]
            .join("\n"),
        )?;
    }
    let records = read_model_call_records(&paths)?;

    assert_eq!(
        record_redundant_exploration_from_worker_logs(&paths, &records),
        2
    );
    assert_eq!(
        record_redundant_exploration_from_worker_logs(&paths, &records),
        0
    );

    let report = build_route_regret_report(&paths)?;
    assert_eq!(report.redundant_explorations, 2);
    let tasks = report
        .recent
        .iter()
        .filter(|entry| entry.kind == "redundant_exploration")
        .filter_map(|entry| entry.task_id.as_deref())
        .collect::<BTreeSet<_>>();
    assert!(tasks.contains("READ-OLD"));
    assert!(tasks.contains("READ-NEW"));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn efficiency_watch_flags_route_regret_alerts() -> Result<()> {
    let root = temp_root("efficiency-watch-route-regret");
    fs::create_dir_all(root.join(".orchestrator"))?;
    let paths = Paths::new(&root)?;
    record_route_regret_decision(
        &paths,
        Some("PAR-003"),
        RouteRegretKind::RepeatedFingerprintNoProgress,
        ExecutionLane::SeniorDecision,
        Some("swift_compile_error|Foo.swift|42|same"),
        "same failure fingerprint repeated without progress",
    )?;

    let report = build_efficiency_watch_report(
        &paths,
        &State::default(),
        WatchPlan {
            task_id: None,
            paths: vec!["NEXT_STEPS.md".to_string()],
            debounce_ms: 750,
        },
    )?;

    assert!(report
        .alerts
        .iter()
        .any(|alert| alert.kind == "route_regret_repeated_fingerprint_no_progress"));
    assert!(report
        .suggestions
        .iter()
        .any(|suggestion| suggestion.contains("Use route memory")));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn efficiency_watch_records_redundant_worker_exploration_alert() -> Result<()> {
    let root = temp_root("efficiency-watch-redundant-exploration");
    fs::create_dir_all(root.join(".orchestrator/runs"))?;
    let paths = Paths::new(&root)?;
    append_model_call_record(
        &paths,
        &ModelCallRecord {
            timestamp: "2026-07-06T14:30:00Z".to_string(),
            role: "worker".to_string(),
            task_id: Some("READ-002".to_string()),
            model: "gpt-5.4-mini".to_string(),
            execution_engine: None,
            route_provider: None,
            effort: "medium".to_string(),
            prompt_token_estimate: 4_500,
            output_char_count: 500,
            skipped_by_rust: false,
            skip_or_call_reason: "called".to_string(),
        },
    )?;
    write_text(
        &paths.runs.join("20260706T143000Z-worker.stdout.jsonl"),
        &[
            r#"{"type":"item.completed","item":{"type":"command_execution","command":"rg \"RouteRegretKind\" src/app.rs","exit_code":0}}"#,
            r#"{"type":"item.completed","item":{"type":"command_execution","command":"rg \"RouteRegretKind\" src/app.rs","exit_code":0}}"#,
        ]
        .join("\n"),
    )?;

    let report = build_efficiency_watch_report(
        &paths,
        &State::default(),
        WatchPlan {
            task_id: None,
            paths: vec!["NEXT_STEPS.md".to_string()],
            debounce_ms: 750,
        },
    )?;

    assert!(report
        .alerts
        .iter()
        .any(|alert| alert.kind == "route_regret_redundant_exploration"));
    assert!(report
        .suggestions
        .iter()
        .any(|suggestion| suggestion.contains("Use route memory")));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn efficiency_watch_flags_tool_health_cooldown() -> Result<()> {
    let root = temp_root("efficiency-watch-tool-health");
    fs::create_dir_all(root.join(".orchestrator/observations"))?;
    let paths = Paths::new(&root)?;
    append_jsonl(
        &tool_health_path(&paths),
        &serde_json::to_value(ToolHealthRecord {
            observed_at: "2026-07-06T14:30:00Z".to_string(),
            role: "worker".to_string(),
            task_id: Some("TASK-1".to_string()),
            model: "gpt-5.4-mini".to_string(),
            execution_engine: None,
            route_provider: None,
            status: ToolHealthStatus::Failure,
            failure_kind: Some(crate::domain::ToolFailureKind::RateLimited),
            exit_code: Some(1),
            timed_out: false,
            excerpt: "429 rate limit".to_string(),
        })?,
    )?;

    let report = build_efficiency_watch_report(
        &paths,
        &State::default(),
        WatchPlan {
            task_id: None,
            paths: vec!["NEXT_STEPS.md".to_string()],
            debounce_ms: 750,
        },
    )?;

    assert!(report
        .alerts
        .iter()
        .any(|alert| alert.kind == "tool_health_cooldown"));
    assert!(report
        .suggestions
        .iter()
        .any(|suggestion| suggestion.contains("tool-health")));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn route_bench_writes_synthetic_report() -> Result<()> {
    let root = temp_root("route-bench-report");
    fs::create_dir_all(&root)?;
    route_bench(root.clone(), false)?;

    let paths = Paths::new(&root)?;
    let markdown = read_text(&paths.observations.join("route_benchmark.md"))?;
    let json = read_text(&paths.observations.join("route_benchmark.json"))?;

    assert!(markdown.contains("# Route Benchmark"));
    assert!(markdown.contains("synthetic:command-only-evidence"));
    assert!(json.contains("\"recommended_lane\""));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn efficiency_watch_flags_back_to_back_large_pm_worker_prompts() -> Result<()> {
    let root = temp_root("efficiency-watch-large-prompts");
    fs::create_dir_all(root.join(".orchestrator"))?;
    let paths = Paths::new(&root)?;
    append_model_call_record(
        &paths,
        &ModelCallRecord {
            timestamp: "2026-06-29T18:10:51Z".to_string(),
            role: "planner".to_string(),
            task_id: None,
            model: "gpt-5.4".to_string(),
            execution_engine: None,
            route_provider: None,
            effort: "medium".to_string(),
            prompt_token_estimate: 11_200,
            output_char_count: 1200,
            skipped_by_rust: false,
            skip_or_call_reason: "called".to_string(),
        },
    )?;
    append_model_call_record(
        &paths,
        &ModelCallRecord {
            timestamp: "2026-06-29T18:12:39Z".to_string(),
            role: "worker".to_string(),
            task_id: Some("PAR-004".to_string()),
            model: "gpt-5.4-mini".to_string(),
            execution_engine: None,
            route_provider: None,
            effort: "medium".to_string(),
            prompt_token_estimate: 11_000,
            output_char_count: 900,
            skipped_by_rust: false,
            skip_or_call_reason: "called".to_string(),
        },
    )?;
    let report = build_efficiency_watch_report(
        &paths,
        &State::default(),
        WatchPlan {
            task_id: None,
            paths: vec!["NEXT_STEPS.md".to_string()],
            debounce_ms: 750,
        },
    )?;

    assert!(report
        .alerts
        .iter()
        .any(|alert| alert.kind == "pm_worker_double_context"));
    assert!(
        report
            .alerts
            .iter()
            .filter(|alert| alert.kind == "prompt_bloat")
            .count()
            >= 2
    );
    assert!(report
        .suggestions
        .iter()
        .any(|suggestion| suggestion.contains("Compact PM/Worker prompt packets")));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn efficiency_watch_report_writes_advisory_markdown_and_json() -> Result<()> {
    let root = temp_root("efficiency-watch-report");
    fs::create_dir_all(root.join(".orchestrator"))?;
    let paths = Paths::new(&root)?;
    let state = State {
        failed_tasks: HashMap::from([("PAR-003".to_string(), 3)]),
        retry_worker_same_task: Some("PAR-003".to_string()),
        ..State::default()
    };
    let report = build_efficiency_watch_report(
        &paths,
        &state,
        WatchPlan {
            task_id: Some("PAR-003".to_string()),
            paths: vec!["Sources/App.swift".to_string()],
            debounce_ms: 750,
        },
    )?;
    let report_path = write_efficiency_watch_report(&paths, &report)?;

    let markdown = read_text(&report_path)?;
    assert!(markdown.contains("# Efficiency Watch"));
    assert!(markdown.contains("repeated_task_failure"));
    assert!(markdown.contains("does not approve, queue, run, stage, commit, or revert work"));
    let json = read_text(&paths.observations.join("efficiency_watch.json"))?;
    assert!(json.contains("PAR-003"));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn efficiency_watch_flags_stale_verification_hint() -> Result<()> {
    let root = temp_root("efficiency-watch-stale-hint");
    fs::create_dir_all(root.join(".orchestrator"))?;
    let paths = Paths::new(&root)?;
    fs::create_dir_all(&paths.evidence)?;
    let failed_command =
        vc("swift test --filter TiledRendererTests").with_source("local_memory.verification_hint");
    let failed_display = failed_command.display();
    write_text(
        &paths.evidence.join("PAR-003.jsonl"),
        &format!(
            "{}\n",
            serde_json::to_string(&json!({
                "event": "command",
                "task_id": "PAR-003",
                "report_id": "report-stale-hint",
                "plan_id": "PAR-003-plan",
                "status": "commandFailed",
                "started_at": "2026-06-29T18:00:00Z",
                "completed_at": "2026-06-29T18:01:00Z",
                "rejected_commands": [],
                "command": CommandEvidence {
                id: "hint-1".to_string(),
                command: failed_command,
                command_display: failed_display,
                exit_code: 1,
                success: false,
                category: "commandFailed".to_string(),
                duration_ms: 32_000,
                stdout_tail: "Test run failed.".to_string(),
                stderr_tail: String::new(),
                evidence_summary: "command exited 1".to_string(),
                },
            }))?
        ),
    )?;

    let report = build_efficiency_watch_report(
        &paths,
        &State::default(),
        WatchPlan {
            task_id: None,
            paths: vec!["NEXT_STEPS.md".to_string()],
            debounce_ms: 750,
        },
    )?;

    assert!(report
        .alerts
        .iter()
        .any(|alert| alert.kind == "stale_verification_hint"
            && alert.summary.contains("TiledRendererTests")));
    assert!(report
        .suggestions
        .iter()
        .any(|suggestion| suggestion.contains("Suppress failed observation-memory")));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn efficiency_watch_flags_pm_file_edits() -> Result<()> {
    let root = temp_root("efficiency-watch-pm-file-edits");
    fs::create_dir_all(root.join(".orchestrator"))?;
    let paths = Paths::new(&root)?;
    append_event(
        &paths,
        OrchestratorEventKind::PolicyViolation,
        None,
        json!({
            "mode": "pm_mutated_files_detected",
            "changed_files": ["Sources/App.swift"],
        }),
    )?;

    let report = build_efficiency_watch_report(
        &paths,
        &State::default(),
        WatchPlan {
            task_id: None,
            paths: vec!["NEXT_STEPS.md".to_string()],
            debounce_ms: 750,
        },
    )?;

    assert!(report
        .alerts
        .iter()
        .any(|alert| alert.kind == "pm_file_edit"));
    assert!(report
        .suggestions
        .iter()
        .any(|suggestion| { suggestion.contains("Keep Project Manager read-only") }));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn efficiency_watch_flags_pm_worker_doc_bounce() -> Result<()> {
    let root = temp_root("efficiency-watch-pm-worker-doc-bounce");
    fs::create_dir_all(root.join(".orchestrator"))?;
    let paths = Paths::new(&root)?;
    append_model_call_record(
        &paths,
        &ModelCallRecord {
            timestamp: "2026-06-29T18:10:51Z".to_string(),
            role: "planner".to_string(),
            task_id: None,
            model: "gpt-5.4".to_string(),
            execution_engine: None,
            route_provider: None,
            effort: "medium".to_string(),
            prompt_token_estimate: 2_000,
            output_char_count: 100,
            skipped_by_rust: false,
            skip_or_call_reason: "called".to_string(),
        },
    )?;
    append_model_call_record(
        &paths,
        &ModelCallRecord {
            timestamp: "2026-06-29T18:12:39Z".to_string(),
            role: "worker".to_string(),
            task_id: Some("DOC-001".to_string()),
            model: "gpt-5.4-mini".to_string(),
            execution_engine: None,
            route_provider: None,
            effort: "medium".to_string(),
            prompt_token_estimate: 2_000,
            output_char_count: 100,
            skipped_by_rust: false,
            skip_or_call_reason: "called".to_string(),
        },
    )?;
    append_event(
        &paths,
        OrchestratorEventKind::GuardChecked,
        Some("DOC-001"),
        json!({
            "status": "ok",
            "changed_files": ["NEXT_STEPS.md", "CONTINUITY.md"],
            "violations": [],
        }),
    )?;

    let report = build_efficiency_watch_report(
        &paths,
        &State::default(),
        WatchPlan {
            task_id: Some("DOC-001".to_string()),
            paths: vec!["NEXT_STEPS.md".to_string()],
            debounce_ms: 750,
        },
    )?;

    assert!(report
        .alerts
        .iter()
        .any(|alert| alert.kind == "pm_worker_doc_bounce"));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn efficiency_watch_suggests_rust_evidence_job_for_worker_deterministic_command() -> Result<()> {
    let root = temp_root("efficiency-watch-worker-deterministic");
    fs::create_dir_all(root.join(".orchestrator/runs"))?;
    let paths = Paths::new(&root)?;
    append_model_call_record(
        &paths,
        &ModelCallRecord {
            timestamp: "2026-06-29T20:57:34Z".to_string(),
            role: "worker".to_string(),
            task_id: Some("PAR-010".to_string()),
            model: "gpt-5.4-mini".to_string(),
            execution_engine: None,
            route_provider: None,
            effort: "medium".to_string(),
            prompt_token_estimate: 5_225,
            output_char_count: 900,
            skipped_by_rust: false,
            skip_or_call_reason: "called".to_string(),
        },
    )?;
    write_text(
        &paths.runs.join("20260629T181239Z-worker.stdout.jsonl"),
        r#"{"msg":"Running swift build -c release --product SwiftStackerBench for 586ms"}"#,
    )?;

    let report = build_efficiency_watch_report(
        &paths,
        &State::default(),
        WatchPlan {
            task_id: None,
            paths: vec!["NEXT_STEPS.md".to_string()],
            debounce_ms: 750,
        },
    )?;

    assert!(report
        .alerts
        .iter()
        .any(|alert| alert.kind == "worker_ran_deterministic_command"));
    let handoff = report
        .alerts
        .iter()
        .find_map(|alert| alert.mechanistic_handoff.as_ref())
        .expect("worker deterministic alert should carry handoff details");
    assert_eq!(handoff.action, "materialized");
    assert_eq!(handoff.prompt_tokens, Some(5_225));
    assert_eq!(handoff.observed_duration_ms, Some(586));
    assert!(report
        .suggestions
        .iter()
        .any(|suggestion| suggestion.contains("rust_evidence_job")));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn efficiency_self_heal_writes_mechanistic_handoff_observations() -> Result<()> {
    let root = temp_root("efficiency-watch-mechanistic-self-heal");
    fs::create_dir_all(root.join(".orchestrator/runs"))?;
    let paths = Paths::new(&root)?;
    write_text(
        &paths.runs.join("20260629T181239Z-worker.stdout.jsonl"),
        r#"{"msg":"Running swift build -c release --product SwiftStackerBench for 586ms"}"#,
    )?;
    let report = build_efficiency_watch_report(
        &paths,
        &State::default(),
        WatchPlan {
            task_id: None,
            paths: vec!["NEXT_STEPS.md".to_string()],
            debounce_ms: 750,
        },
    )?;

    let self_heal = apply_efficiency_self_heal(&paths, &Config::default(), &report)?;

    assert!(self_heal
        .actions
        .iter()
        .any(|action| action.kind == "mechanistic_handoff"));
    let json = read_text(&paths.observations.join("mechanistic_handoffs.json"))?;
    assert!(json.contains("swift build -c release --product SwiftStackerBench"));
    assert!(json.contains("\"action\": \"materialized\""));
    let markdown = read_text(&paths.observations.join("mechanistic_handoffs.md"))?;
    assert!(markdown.contains("# Mechanistic Handoffs"));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn external_efficiency_feedback_names_mechanistic_recipe_repair() -> Result<()> {
    let target_root = temp_root("efficiency-watch-mechanistic-target");
    fs::create_dir_all(target_root.join(".orchestrator/runs"))?;
    let target_paths = Paths::new(&target_root)?;
    write_text(
        &target_paths
            .runs
            .join("20260629T181239Z-worker.stdout.jsonl"),
        r#"{"msg":"Running swift run -c release --disable-sandbox SwiftStackerBench --suite Benchmarks/Suites/focus_stack_dataset_suite.json --report-root Benchmarks/ReportsDiagnostics/replay-v1 for 2m42s"}"#,
    )?;
    let report = build_efficiency_watch_report(
        &target_paths,
        &State::default(),
        WatchPlan {
            task_id: None,
            paths: vec!["NEXT_STEPS.md".to_string()],
            debounce_ms: 750,
        },
    )?;
    let handoff = report
        .alerts
        .iter()
        .find_map(|alert| alert.mechanistic_handoff.as_ref())
        .expect("unsupported deterministic-looking command should be reported");
    assert_eq!(handoff.action, "unsupported_sent_to_orchestrator");

    let orchestrator_root = temp_root("efficiency-watch-mechanistic-orchestrator");
    let orchestrator_paths = Paths::new(&orchestrator_root)?;
    init_repo(&orchestrator_paths, "Improve orchestration efficiency")?;
    let feedback =
        record_external_efficiency_feedback(&orchestrator_paths, &target_root, 1, &report)?;

    let markdown = read_text(&feedback)?;
    assert!(markdown.contains("Mechanistic handoff alerts should become typed Rust recipes"));
    let json = read_text(
        &orchestrator_paths
            .observations
            .join("external_efficiency_feedback")
            .read_dir()?
            .find_map(|entry| {
                let path = entry.ok()?.path();
                (path.extension().and_then(|ext| ext.to_str()) == Some("json")).then_some(path)
            })
            .expect("feedback json should exist"),
    )?;
    assert!(json.contains("recipe/router repair"));

    fs::remove_dir_all(target_root)?;
    fs::remove_dir_all(orchestrator_root)?;
    Ok(())
}

#[test]
fn efficiency_watch_ignores_old_prompt_bloat_outside_recent_window() -> Result<()> {
    let root = temp_root("efficiency-watch-recent-prompt-window");
    fs::create_dir_all(root.join(".orchestrator"))?;
    let paths = Paths::new(&root)?;
    append_model_call_record(
        &paths,
        &ModelCallRecord {
            timestamp: "2026-06-29T18:10:51Z".to_string(),
            role: "planner".to_string(),
            task_id: None,
            model: "gpt-5.4".to_string(),
            execution_engine: None,
            route_provider: None,
            effort: "medium".to_string(),
            prompt_token_estimate: 12_100,
            output_char_count: 1200,
            skipped_by_rust: false,
            skip_or_call_reason: "plannerNeeded".to_string(),
        },
    )?;
    for index in 0..6 {
        append_model_call_record(
            &paths,
            &ModelCallRecord {
                timestamp: format!("2026-06-29T18:2{index}:51Z"),
                role: "worker".to_string(),
                task_id: Some(format!("PAR-00{index}")),
                model: "gpt-5.4-mini".to_string(),
                execution_engine: None,
                route_provider: None,
                effort: "medium".to_string(),
                prompt_token_estimate: 7_500,
                output_char_count: 700,
                skipped_by_rust: false,
                skip_or_call_reason: "patchGenerationNeeded".to_string(),
            },
        )?;
    }

    let report = build_efficiency_watch_report(
        &paths,
        &State::default(),
        WatchPlan {
            task_id: None,
            paths: vec!["NEXT_STEPS.md".to_string()],
            debounce_ms: 750,
        },
    )?;

    assert!(!report
        .alerts
        .iter()
        .any(|alert| alert.kind == "prompt_bloat"));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn efficiency_watch_noop_self_heal_does_not_hide_later_prompt_bloat() -> Result<()> {
    let root = temp_root("efficiency-watch-noop-self-heal");
    fs::create_dir_all(root.join(".orchestrator"))?;
    let paths = Paths::new(&root)?;
    fs::create_dir_all(&paths.observations)?;
    write_text(
        &paths.observations.join("efficiency_self_heal.json"),
        &format!(
            "{}\n",
            serde_json::to_string_pretty(&EfficiencySelfHealReport {
                generated_at: "2026-06-29T18:59:05Z".to_string(),
                actions: vec![EfficiencySelfHealAction {
                    kind: "compact_planner_context".to_string(),
                    status: "already_applied".to_string(),
                    summary: "planner_context is already compact".to_string(),
                    files_changed: Vec::new(),
                }],
            })?
        ),
    )?;
    append_model_call_record(
        &paths,
        &ModelCallRecord {
            timestamp: "2026-06-29T19:15:28Z".to_string(),
            role: "foreman-review".to_string(),
            task_id: Some("PAR-007".to_string()),
            model: "gpt-5.5".to_string(),
            execution_engine: None,
            route_provider: None,
            effort: "high".to_string(),
            prompt_token_estimate: 13_739,
            output_char_count: 1015,
            skipped_by_rust: false,
            skip_or_call_reason: "humanJudgmentNeeded".to_string(),
        },
    )?;

    let report = build_efficiency_watch_report(
        &paths,
        &State::default(),
        WatchPlan {
            task_id: None,
            paths: vec!["NEXT_STEPS.md".to_string()],
            debounce_ms: 750,
        },
    )?;

    assert!(report
        .alerts
        .iter()
        .any(|alert| { alert.kind == "prompt_bloat" && alert.summary.contains("Senior Dev") }));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn efficiency_watch_hides_already_suppressed_stale_verification_hint() -> Result<()> {
    let root = temp_root("efficiency-watch-suppressed-stale-hint");
    fs::create_dir_all(root.join(".orchestrator"))?;
    let paths = Paths::new(&root)?;
    fs::create_dir_all(&paths.evidence)?;
    fs::create_dir_all(&paths.observations)?;
    let failed_command =
        vc("swift test --filter TiledRendererTests").with_source("local_memory.verification_hint");
    let failed_display = failed_command.display();
    let semantic_fingerprint = failed_command.semantic_fingerprint();
    write_text(
        &paths.evidence.join("PAR-003.jsonl"),
        &format!(
            "{}\n",
            serde_json::to_string(&json!({
                "event": "command",
                "task_id": "PAR-003",
                "report_id": "report-stale-hint",
                "plan_id": "PAR-003-plan",
                "status": "commandFailed",
                "started_at": "2026-06-29T18:00:00Z",
                "completed_at": "2026-06-29T18:01:00Z",
                "rejected_commands": [],
                "command": CommandEvidence {
                    id: "hint-1".to_string(),
                    command: failed_command,
                    command_display: failed_display,
                    exit_code: 1,
                    success: false,
                    category: "commandFailed".to_string(),
                    duration_ms: 32_000,
                    stdout_tail: "Test run failed.".to_string(),
                    stderr_tail: String::new(),
                    evidence_summary: "command exited 1".to_string(),
                },
            }))?
        ),
    )?;
    write_text(
        &paths
            .observations
            .join("suppressed_verification_hints.json"),
        &format!(
            "{}\n",
            serde_json::to_string_pretty(&SuppressedVerificationHints {
                updated_at: "2026-06-29T18:59:05Z".to_string(),
                hints: vec![SuppressedVerificationHint {
                    command_display: "swift test --filter TiledRendererTests".to_string(),
                    semantic_fingerprint,
                    source: "local_memory.verification_hint".to_string(),
                    task_id: "PAR-003".to_string(),
                    category: "commandFailed".to_string(),
                    suppressed_at: "2026-06-29T18:59:05Z".to_string(),
                    reason: "failed as observation-memory verification hint".to_string(),
                }],
            })?
        ),
    )?;

    let report = build_efficiency_watch_report(
        &paths,
        &State::default(),
        WatchPlan {
            task_id: None,
            paths: vec!["NEXT_STEPS.md".to_string()],
            debounce_ms: 750,
        },
    )?;

    assert!(!report
        .alerts
        .iter()
        .any(|alert| alert.kind == "stale_verification_hint"));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn self_heal_switches_full_planner_context_to_compact_on_prompt_bloat() -> Result<()> {
    let root = temp_root("self-heal-compact-planner");
    fs::create_dir_all(root.join(".orchestrator"))?;
    let paths = Paths::new(&root)?;
    let mut cfg = Config {
        planner_context: PlannerContextMode::Full,
        ..Config::default()
    };
    write_text(&paths.config, &toml::to_string_pretty(&cfg)?)?;
    append_model_call_record(
        &paths,
        &ModelCallRecord {
            timestamp: "2026-06-29T18:40:13Z".to_string(),
            role: "planner".to_string(),
            task_id: None,
            model: "gpt-5.4".to_string(),
            execution_engine: None,
            route_provider: None,
            effort: "medium".to_string(),
            prompt_token_estimate: 12_100,
            output_char_count: 800,
            skipped_by_rust: false,
            skip_or_call_reason: "plannerNeeded".to_string(),
        },
    )?;
    let report = build_efficiency_watch_report(
        &paths,
        &State::default(),
        WatchPlan {
            task_id: None,
            paths: vec!["NEXT_STEPS.md".to_string()],
            debounce_ms: 750,
        },
    )?;

    let self_heal = apply_efficiency_self_heal(&paths, &cfg, &report)?;
    cfg = toml::from_str(&read_text(&paths.config)?)?;

    assert_eq!(cfg.planner_context, PlannerContextMode::Compact);
    assert!(self_heal
        .actions
        .iter()
        .any(|action| { action.kind == "compact_planner_context" && action.status == "applied" }));
    assert!(paths.observations.join("efficiency_self_heal.md").exists());

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn self_heal_persists_stale_verification_hint_suppressions() -> Result<()> {
    let root = temp_root("self-heal-stale-hints");
    fs::create_dir_all(root.join(".orchestrator"))?;
    let paths = Paths::new(&root)?;
    let cfg = Config::default();
    let failed_command =
        vc("swift test --filter TiledRendererTests").with_source("local_memory.verification_hint");
    let failed_display = failed_command.display();
    persist_evidence_report(
        &paths,
        &EvidenceReport {
            report_id: "report-stale-hint".to_string(),
            plan_id: "PAR-003-plan".to_string(),
            task_id: "PAR-003".to_string(),
            status: "commandFailed".to_string(),
            started_at: "2026-06-29T18:00:00Z".to_string(),
            completed_at: "2026-06-29T18:01:00Z".to_string(),
            summary: "failed stale hint".to_string(),
            commands: vec![CommandEvidence {
                id: "hint-1".to_string(),
                command: failed_command,
                command_display: failed_display,
                exit_code: 1,
                success: false,
                category: "commandFailed".to_string(),
                duration_ms: 32_000,
                stdout_tail: "Test run failed.".to_string(),
                stderr_tail: String::new(),
                evidence_summary: "command exited 1".to_string(),
            }],
            rejected_commands: Vec::new(),
        },
    )?;
    let report = build_efficiency_watch_report(
        &paths,
        &State::default(),
        WatchPlan {
            task_id: None,
            paths: vec!["NEXT_STEPS.md".to_string()],
            debounce_ms: 750,
        },
    )?;

    let self_heal = apply_efficiency_self_heal(&paths, &cfg, &report)?;
    let suppressed = read_suppressed_verification_hints(&paths).expect("suppression file exists");

    assert!(self_heal.actions.iter().any(|action| {
        action.kind == "suppress_stale_verification_hints" && action.status == "applied"
    }));
    assert_eq!(suppressed.hints.len(), 1);
    assert_eq!(
        suppressed.hints[0].command_display,
        "swift test --filter TiledRendererTests"
    );

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn watch_loop_noninteractive_step_stops_on_human_disposition_pause() -> Result<()> {
    let root = temp_root("watch-loop-human-pause");
    fs::create_dir_all(root.join(".orchestrator"))?;
    let paths = Paths::new(&root)?;
    let cfg = Config::default();
    let mut state = State {
        human_disposition_required: Some(HumanDispositionPause {
            reason: "dirty baseline needs a human choice".to_string(),
            options: vec!["preserve".to_string(), "recheck".to_string()],
            source: "rust_dirty_adoption".to_string(),
        }),
        ..State::default()
    };
    let runner = FakeCodexRunner::new(Vec::new());

    let should_continue = step_once_with_runner_mode(
        &paths,
        &cfg,
        &mut state,
        &runner,
        HumanDispositionMode::Stop,
    )?;

    assert!(!should_continue);
    assert!(state.human_disposition_required.is_some());
    assert!(runner.calls.borrow().is_empty());
    assert!(state
        .recent_events
        .iter()
        .any(|event| event.kind == "human-disposition-stop"));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn watch_loop_prompt_mode_recheck_clears_pause_and_marks_pm_due() -> Result<()> {
    let root = temp_root("watch-loop-human-recheck");
    fs::create_dir_all(root.join(".orchestrator"))?;
    write_text(&root.join("README.md"), "fixture\n")?;
    let paths = Paths::new(&root)?;
    let cfg = Config::default();
    let state = State {
        mission: "test mission".to_string(),
        human_disposition_required: Some(HumanDispositionPause {
            reason: "control docs need a human choice".to_string(),
            options: vec!["recheck".to_string(), "gate".to_string()],
            source: "project_manager".to_string(),
        }),
        ..State::default()
    };
    save_state(&paths, &state)?;
    init_git_repo(&root)?;

    let outcome = run_one_supervised_cycle_with_human_mode(
        &paths,
        &cfg,
        HumanDispositionMode::Preset(HumanDispositionChoice::Recheck),
    )?;
    let updated = load_state(&paths)?;

    assert!(outcome.step_ran);
    assert!(outcome.should_continue);
    assert!(!outcome.human_pause);
    assert!(outcome.stopped_reason.is_none());
    assert!(updated.human_disposition_required.is_none());
    assert!(updated.planner_due);
    assert_eq!(
        updated
            .human_disposition_decision
            .as_ref()
            .map(|decision| decision.decision.as_str()),
        Some("recheck_after_external_handling")
    );

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn watch_loop_prompt_mode_preserve_records_stop_without_pm() -> Result<()> {
    let root = temp_root("watch-loop-human-preserve");
    fs::create_dir_all(root.join(".orchestrator"))?;
    write_text(&root.join("README.md"), "fixture\n")?;
    let paths = Paths::new(&root)?;
    let cfg = Config::default();
    let state = State {
        mission: "test mission".to_string(),
        human_disposition_required: Some(HumanDispositionPause {
            reason: "dirty baseline needs a human choice".to_string(),
            options: vec!["preserve".to_string(), "recheck".to_string()],
            source: "project_manager".to_string(),
        }),
        ..State::default()
    };
    save_state(&paths, &state)?;
    init_git_repo(&root)?;

    let outcome = run_one_supervised_cycle_with_human_mode(
        &paths,
        &cfg,
        HumanDispositionMode::Preset(HumanDispositionChoice::PreserveAndStop),
    )?;
    let updated = load_state(&paths)?;

    assert!(outcome.step_ran);
    assert!(!outcome.should_continue);
    assert!(!outcome.human_pause);
    assert_eq!(
        outcome.stopped_reason.as_deref(),
        Some("human_disposition_recorded_stop")
    );
    assert!(updated.human_disposition_required.is_none());
    assert!(!updated.planner_due);
    assert_eq!(
        updated
            .human_disposition_decision
            .as_ref()
            .map(|decision| decision.decision.as_str()),
        Some("preserve_quarantine_stop")
    );

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn watch_loop_self_heal_auto_gates_pm_gateable_pause() -> Result<()> {
    let root = temp_root("watch-loop-auto-gate-pm-pause");
    fs::create_dir_all(root.join(".orchestrator"))?;
    write_text(&root.join("README.md"), "fixture\n")?;
    let paths = Paths::new(&root)?;
    let cfg = Config::default();
    let state = State {
        mission: "test mission".to_string(),
        human_disposition_required: Some(HumanDispositionPause {
            reason: "PM needs an explicit safety gate before continuing.".to_string(),
            options: vec![
                "preserve/quarantine the dirty set as user-owned context".to_string(),
                "approve a new explicit safety gate for the dirty set".to_string(),
            ],
            source: "project_manager".to_string(),
        }),
        ..State::default()
    };
    save_state(&paths, &state)?;
    init_git_repo(&root)?;

    assert_eq!(
        watch_loop_target_human_disposition_mode(&paths, true),
        HumanDispositionMode::Preset(HumanDispositionChoice::ApproveGate)
    );
    let outcome = run_one_supervised_cycle_with_human_mode(
        &paths,
        &cfg,
        watch_loop_target_human_disposition_mode(&paths, true),
    )?;
    let updated = load_state(&paths)?;

    assert!(outcome.step_ran);
    assert!(outcome.should_continue);
    assert!(!outcome.human_pause);
    assert!(updated.human_disposition_required.is_none());
    assert!(updated.planner_due);
    assert_eq!(
        updated
            .human_disposition_decision
            .as_ref()
            .map(|decision| decision.decision.as_str()),
        Some("approve_explicit_gate")
    );

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn watch_loop_auto_gate_helper_applies_active_pm_gateable_pause() -> Result<()> {
    let root = temp_root("watch-loop-auto-gate-helper");
    fs::create_dir_all(root.join(".orchestrator"))?;
    let paths = Paths::new(&root)?;
    let state = State {
        mission: "test mission".to_string(),
        human_disposition_required: Some(HumanDispositionPause {
            reason: "PM needs an explicit safety gate before continuing.".to_string(),
            options: vec!["approve a new explicit safety gate for the dirty set".to_string()],
            source: "project_manager".to_string(),
        }),
        ..State::default()
    };
    save_state(&paths, &state)?;

    assert!(try_auto_gate_watch_loop_pause(&paths)?);
    let updated = load_state(&paths)?;
    assert!(updated.human_disposition_required.is_none());
    assert!(updated.planner_due);
    assert_eq!(
        updated
            .human_disposition_decision
            .as_ref()
            .map(|decision| decision.decision.as_str()),
        Some("approve_explicit_gate")
    );

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn watch_loop_orchestrator_self_heal_auto_gates_pm_gateable_pause() -> Result<()> {
    let root = temp_root("watch-loop-orchestrator-auto-gate-helper");
    fs::create_dir_all(root.join(".orchestrator"))?;
    let paths = Paths::new(&root)?;
    let state = State {
        mission: "test mission".to_string(),
        human_disposition_required: Some(HumanDispositionPause {
            reason: "PM needs an explicit safety gate before continuing.".to_string(),
            options: vec!["approve a new explicit safety gate for the dirty set".to_string()],
            source: "project_manager".to_string(),
        }),
        ..State::default()
    };
    save_state(&paths, &state)?;

    assert_eq!(
        watch_loop_orchestrator_human_disposition_mode(&paths, true),
        HumanDispositionMode::Preset(HumanDispositionChoice::ApproveGate)
    );
    assert_eq!(
        watch_loop_orchestrator_human_disposition_mode(&paths, false),
        HumanDispositionMode::Stop
    );

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn watch_loop_self_heal_does_not_auto_gate_dirty_adoption_pause() -> Result<()> {
    let root = temp_root("watch-loop-no-auto-gate-dirty-adoption");
    fs::create_dir_all(root.join(".orchestrator"))?;
    let paths = Paths::new(&root)?;
    let state = State {
        mission: "test mission".to_string(),
        human_disposition_required: Some(HumanDispositionPause {
            reason: "dirty adoption baseline needs a human choice".to_string(),
            options: vec![
                "preserve/quarantine the dirty set as user-owned context".to_string(),
                "approve a new explicit safety gate for the dirty set".to_string(),
            ],
            source: "rust_dirty_adoption".to_string(),
        }),
        ..State::default()
    };
    save_state(&paths, &state)?;

    assert_ne!(
        watch_loop_orchestrator_human_disposition_mode(&paths, true),
        HumanDispositionMode::Preset(HumanDispositionChoice::ApproveGate)
    );

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn watch_loop_auto_gate_helper_ignores_dirty_adoption_pause() -> Result<()> {
    let root = temp_root("watch-loop-auto-gate-helper-dirty-adoption");
    fs::create_dir_all(root.join(".orchestrator"))?;
    let paths = Paths::new(&root)?;
    let state = State {
        mission: "test mission".to_string(),
        human_disposition_required: Some(HumanDispositionPause {
            reason: "dirty adoption baseline needs a human choice".to_string(),
            options: vec!["approve a new explicit safety gate for the dirty set".to_string()],
            source: "rust_dirty_adoption".to_string(),
        }),
        ..State::default()
    };
    save_state(&paths, &state)?;

    assert!(!try_auto_gate_watch_loop_pause(&paths)?);
    let updated = load_state(&paths)?;
    assert!(updated.human_disposition_required.is_some());
    assert!(updated.human_disposition_decision.is_none());

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn watch_loop_records_external_feedback_and_sets_orchestrator_planner_due() -> Result<()> {
    let root = temp_root("watch-loop-feedback");
    let paths = Paths::new(&root)?;
    init_repo(&paths, "Improve orchestrator efficiency")?;
    let target_root = temp_root("watch-loop-target");
    fs::create_dir_all(&target_root)?;
    let report = EfficiencyWatchReport {
        generated_at: "2026-06-29T18:00:00Z".to_string(),
        task_id: None,
        prompt_token_threshold: 10_000,
        derived_status: None,
        alerts: vec![EfficiencyWatchAlert {
            severity: "high".to_string(),
            kind: "pm_worker_double_context".to_string(),
            summary: "PM and Worker both read a huge context.".to_string(),
            evidence: vec!["pm_at=2026-06-29T18:00:00Z".to_string()],
            mechanistic_handoff: None,
        }],
        suggestions: vec!["Use context-pack receipts and Rust evidence jobs.".to_string()],
        watch: WatchPlan {
            task_id: None,
            paths: vec!["NEXT_STEPS.md".to_string()],
            debounce_ms: 750,
        },
    };

    let feedback_path = record_external_efficiency_feedback(&paths, &target_root, 2, &report)?;
    let state = load_state(&paths)?;
    let markdown = read_text(&feedback_path)?;
    let feedback_dir = paths.observations.join("external_efficiency_feedback");

    assert!(state.planner_due);
    assert!(state
        .recent_events
        .iter()
        .any(|event| { event.role == "watcher" && event.kind == "external-efficiency-feedback" }));
    assert!(markdown.contains("External Efficiency Feedback"));
    assert!(markdown.contains("pm_worker_double_context"));
    assert!(feedback_dir.exists());
    assert!(fs::read_dir(feedback_dir)?.flatten().any(|entry| entry
        .path()
        .extension()
        .and_then(|ext| ext.to_str())
        == Some("json")));

    fs::remove_dir_all(root)?;
    fs::remove_dir_all(target_root)?;
    Ok(())
}

#[test]
fn watch_loop_initializes_missing_orchestrator_repo_state() -> Result<()> {
    let root = temp_root("watch-loop-init-orchestrator");
    fs::create_dir_all(&root)?;
    let paths = Paths::new(&root)?;

    ensure_orchestrator_repo_initialized(
        &paths,
        "Improve codex-team orchestration efficiency while preserving safe bounded automation",
    )?;

    assert!(paths.config.exists());
    assert!(paths.state.exists());
    let state = load_state(&paths)?;
    assert_eq!(
        state.mission,
        "Improve codex-team orchestration efficiency while preserving safe bounded automation"
    );

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn watch_loop_classifies_blocked_orchestrator_repair_as_stop_reason() {
    let pause = SupervisedCycleOutcome {
        step_ran: false,
        should_continue: false,
        human_pause: true,
        stopped_reason: Some("human_disposition_pause_active".to_string()),
    };
    assert_eq!(
        orchestrator_repair_block_reason(&pause).as_deref(),
        Some("orchestrator_human_disposition_pause")
    );

    let no_work = SupervisedCycleOutcome {
        step_ran: false,
        should_continue: false,
        human_pause: false,
        stopped_reason: Some("no_work".to_string()),
    };
    assert_eq!(
        orchestrator_repair_block_reason(&no_work).as_deref(),
        Some("orchestrator_no_work")
    );

    let stopped_after_repair = SupervisedCycleOutcome {
        step_ran: true,
        should_continue: false,
        human_pause: false,
        stopped_reason: None,
    };
    assert_eq!(
        orchestrator_repair_block_reason(&stopped_after_repair).as_deref(),
        Some("orchestrator_repair_stopped")
    );

    let continuing = SupervisedCycleOutcome {
        step_ran: true,
        should_continue: true,
        human_pause: false,
        stopped_reason: None,
    };
    assert!(orchestrator_repair_block_reason(&continuing).is_none());
}

#[test]
fn mission_command_updates_state_and_matching_roadmap_section() -> Result<()> {
    let root = temp_root("mission-command-section");
    fs::create_dir_all(&root)?;
    let paths = Paths::new(&root)?;
    init_repo(&paths, "Old mission")?;
    write_text(
        &paths.roadmap,
        "# ROADMAP\n\n## Mission\n\nOld mission. Keep clean-room context.\n\n## Current phase\n\nPhase 1.\n",
    )?;
    let mut state = load_state(&paths)?;
    state.planner_due = true;
    state.queue.push(sample_task(vec!["README.md".to_string()]));
    save_state(&paths, &state)?;

    set_mission(root.clone(), "New mission".to_string())?;

    let state = load_state(&paths)?;
    let roadmap = read_text(&paths.roadmap)?;
    assert_eq!(state.mission, "New mission");
    assert!(state.planner_due);
    assert_eq!(state.queue.len(), 1);
    assert!(state
        .recent_events
        .iter()
        .any(|event| { event.role == "rust" && event.kind == "mission-updated" }));
    assert!(roadmap.contains("New mission. Keep clean-room context."));
    assert!(!roadmap.contains("Old mission"));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn mission_command_updates_init_mission_line() -> Result<()> {
    let root = temp_root("mission-command-init-line");
    fs::create_dir_all(&root)?;
    let paths = Paths::new(&root)?;
    init_repo(&paths, "Old mission")?;

    set_mission(root.clone(), "New mission".to_string())?;

    let roadmap = read_text(&paths.roadmap)?;
    assert!(roadmap.contains("Mission: New mission"));
    assert!(!roadmap.contains("Mission: Old mission"));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn mission_command_rejects_blank_mission() -> Result<()> {
    let root = temp_root("mission-command-blank");
    fs::create_dir_all(&root)?;
    let paths = Paths::new(&root)?;
    init_repo(&paths, "Old mission")?;

    let result = set_mission(root.clone(), "   ".to_string());

    assert!(result.is_err());
    let state = load_state(&paths)?;
    assert_eq!(state.mission, "Old mission");

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn models_command_updates_role_model_effort_and_timeout() -> Result<()> {
    let root = temp_root("models-command-update");
    fs::create_dir_all(&root)?;
    let paths = Paths::new(&root)?;
    init_repo(&paths, "test models command")?;

    models(
        root.clone(),
        Some("worker".to_string()),
        Some("gpt-fast-worker".to_string()),
        Some("low".to_string()),
        Some(120),
    )?;

    let cfg = load_config(&paths)?;
    assert_eq!(cfg.roles.worker.model, "gpt-fast-worker");
    assert_eq!(cfg.roles.worker.effort, "low");
    assert_eq!(cfg.roles.worker.timeout_seconds, 120);
    let config_text = read_text(&paths.config)?;
    assert!(config_text.contains("gpt-fast-worker"));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn models_command_requires_role_for_updates() -> Result<()> {
    let root = temp_root("models-command-requires-role");
    fs::create_dir_all(&root)?;
    let paths = Paths::new(&root)?;
    init_repo(&paths, "test models command")?;

    let result = models(
        root.clone(),
        None,
        Some("gpt-fast-worker".to_string()),
        None,
        None,
    );

    assert!(result.is_err());
    let cfg = load_config(&paths)?;
    assert_eq!(cfg.roles.worker.model, default_worker_role().model);

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn models_command_rejects_unknown_effort() -> Result<()> {
    let root = temp_root("models-command-bad-effort");
    fs::create_dir_all(&root)?;
    let paths = Paths::new(&root)?;
    init_repo(&paths, "test models command")?;

    let result = models(
        root.clone(),
        Some("planner".to_string()),
        None,
        Some("warp".to_string()),
        None,
    );

    assert!(result.is_err());
    let cfg = load_config(&paths)?;
    assert_eq!(cfg.roles.planner.effort, default_planner_role().effort);

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn watch_loop_report_renders_cycle_summary() -> Result<()> {
    let root = temp_root("watch-loop-report");
    fs::create_dir_all(root.join(".orchestrator/observations"))?;
    let paths = Paths::new(&root)?;
    let report = EfficiencyLoopReport {
        started_at: "2026-06-29T18:00:00Z".to_string(),
        completed_at: "2026-06-29T18:01:00Z".to_string(),
        target_repo: "/tmp/target".to_string(),
        orchestrator_repo: Some("/tmp/orchestrator".to_string()),
        max_cycles: 3,
        self_heal: true,
        orchestrator_cycles: 1,
        stopped_reason: "target_watch_clean".to_string(),
        cycles: vec![EfficiencyLoopCycle {
            iteration: 1,
            target_step_ran: true,
            target_should_continue: false,
            target_human_pause: false,
            watch_alert_count: 1,
            post_heal_alert_count: 0,
            self_heal_action_count: 1,
            orchestrator_feedback_path: None,
            orchestrator_cycles_run: 0,
            stop_reason: Some("target_watch_clean".to_string()),
        }],
    };

    let path = write_efficiency_loop_report(&paths, &report)?;
    let markdown = read_text(&path)?;
    let json = read_text(&paths.observations.join("efficiency_loop.json"))?;

    assert!(markdown.contains("# Efficiency Watch Loop"));
    assert!(markdown.contains("target_watch_clean"));
    assert!(json.contains("\"stopped_reason\": \"target_watch_clean\""));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn verification_plan_skips_persisted_suppressed_observation_hint() -> Result<()> {
    let root = temp_root("self-heal-suppression-used");
    let paths = Paths::new(&root)?;
    fs::create_dir_all(&paths.observations)?;
    let command =
        vc("swift test --filter TiledRendererTests").with_source("local_memory.verification_hint");
    write_text(
        &paths
            .observations
            .join("suppressed_verification_hints.json"),
        &format!(
            "{}\n",
            serde_json::to_string_pretty(&SuppressedVerificationHints {
                updated_at: "2026-06-29T18:01:00Z".to_string(),
                hints: vec![SuppressedVerificationHint {
                    command_display: command.display(),
                    semantic_fingerprint: command.semantic_fingerprint(),
                    source: "local_memory.verification_hint".to_string(),
                    task_id: "PAR-003".to_string(),
                    category: "commandFailed".to_string(),
                    suppressed_at: "2026-06-29T18:01:00Z".to_string(),
                    reason: "failed as observation-memory verification hint".to_string(),
                }],
            })?
        ),
    )?;
    write_text(
        &paths.observations.join("verification_hints.json"),
        &format!(
            "{}\n",
            serde_json::to_string_pretty(&VerificationHintIndex {
                updated_at: "now".to_string(),
                hints_by_file: BTreeMap::from([(
                    "Tests/SwiftStackerCoreTests/TiledRendererTests.swift".to_string(),
                    vec!["swift test --filter TiledRendererTests".to_string()],
                )]),
            })?
        ),
    )?;
    let mut task = sample_task(vec![
        "Tests/SwiftStackerCoreTests/TiledRendererTests.swift".to_string()
    ]);
    task.task_id = "PAR-006".to_string();
    task.tests_to_run = vec!["git diff --check".to_string()];
    let worker = sample_worker_output(&task.task_id);

    let plan =
        build_verification_plan_with_observation_hints(&paths, &Config::default(), &task, &worker)?;

    assert_eq!(plan.commands.len(), 1);
    assert!(plan.rejected_commands.iter().any(|rejected| rejected
        .contains("swift test --filter TiledRendererTests")
        && rejected.contains("recently failed as observation hint")));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn senior_repair_schema_deserializes_decision_only_output() -> Result<()> {
    let output: SeniorRepairOutput = serde_json::from_value(json!({
        "task_id": "TASK-191",
        "decision": "retry_worker_same_task",
        "reason_code": "bounded_repair",
        "repair_brief": "Fix only the missing argument label in AppLoopHarness.swift.",
        "needs_planner": false,
        "needs_master": false,
        "summary": "Mechanical repair remains bounded."
    }))?;

    assert_eq!(output.decision, SeniorRepairDecision::RetryWorkerSameTask);
    assert_eq!(output.task_id, "TASK-191");
    Ok(())
}

#[test]
fn senior_repair_decision_requeues_same_task_or_sets_due_flags() -> Result<()> {
    let root = temp_root("senior-repair-route");
    fs::create_dir_all(root.join(".orchestrator"))?;
    let paths = Paths::new(&root)?;
    let task = sample_task(vec!["Sources/App/Foo.swift".to_string()]);
    let mut state = State::default();
    let retry_brief = WorkerRetryBrief {
        task_id: task.task_id.clone(),
        attempt: 2,
        route: "senior_dev_repair_order".to_string(),
        worker_status: "blocked".to_string(),
        blocker: "compile error".to_string(),
        changed_files: vec!["Sources/App/Foo.swift".to_string()],
        diff_stat: "1 file changed".to_string(),
        diagnostic_command: Some("swift test --filter FooTests".to_string()),
        diagnostic_category: Some("commandFailed".to_string()),
        diagnostic_summary: Some("missing argument label".to_string()),
        fingerprint: Some(FailureFingerprint {
            kind: FailureFingerprintKind::SwiftInitializerLabelMismatch,
            file: Some("Sources/App/Foo.swift".to_string()),
            line: Some(9),
            message: "missing argument label".to_string(),
            command: Some("swift test --filter FooTests".to_string()),
        }),
        fingerprint_retry_count: 2,
        repair_instruction: "fix only the missing label".to_string(),
        ..WorkerRetryBrief::default()
    };
    let retry = SeniorRepairOutput {
        task_id: task.task_id.clone(),
        decision: SeniorRepairDecision::RetryWorkerSameTask,
        reason_code: "bounded_repair".to_string(),
        repair_brief: "Fix only the missing argument label.".to_string(),
        needs_planner: false,
        needs_master: false,
        summary: "Retry is bounded.".to_string(),
    };

    apply_senior_repair_decision(&paths, &mut state, &task, retry, retry_brief.clone())?;
    assert_eq!(
        state.retry_worker_same_task.as_deref(),
        Some(task.task_id.as_str())
    );
    assert_eq!(state.queue.len(), 1);
    assert!(!state.planner_due);

    let planner = SeniorRepairOutput {
        task_id: task.task_id.clone(),
        decision: SeniorRepairDecision::PlannerDue,
        reason_code: "planner_needed".to_string(),
        repair_brief: String::new(),
        needs_planner: true,
        needs_master: false,
        summary: "Needs allowed-file repair.".to_string(),
    };
    apply_senior_repair_decision(&paths, &mut state, &task, planner, retry_brief)?;
    assert!(state.planner_due);

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn compact_repair_prompt_omits_full_control_docs() -> Result<()> {
    let state = State {
        mission: "mission".to_string(),
        ..State::default()
    };
    let prompt = render_senior_repair_prompt(
        FOREMAN_REPAIR_PROMPT,
        &state,
        json!({"review_context": {"task_id": "TASK-191"}}),
    )?;
    assert!(prompt.contains("Compact Orchestrator State"));
    assert!(!prompt.contains("## ROADMAP.md"));
    assert!(!prompt.contains("## NEXT_STEPS.md"));
    Ok(())
}

#[test]
fn compact_planner_prompt_stays_small_with_large_control_docs() -> Result<()> {
    let root = temp_root("compact-planner-prompt-size");
    let paths = Paths::new(&root)?;
    write_text(&paths.roadmap, &large_control_doc("ROADMAP", 900))?;
    write_text(&paths.next_steps, &large_control_doc("NEXT", 1_200))?;
    write_text(&paths.next_steps_non_ai, &large_control_doc("NONAI", 700))?;
    let state = State {
        mission: "Work on obtaining parity with Helicon Focus".to_string(),
        ..State::default()
    };

    let prompt = render_project_manager_compact_prompt(
        PLANNER_PROMPT,
        &paths,
        &state,
        json!({
            "role": "planner",
            "mode": "compile_work_orders",
            "rust_queue_brief": build_planner_queue_brief(&paths, &state),
        }),
    )?;

    assert!(prompt.contains("Compact Project Manager Context Mode"));
    assert!(prompt.contains("<!-- compact excerpt: middle omitted -->"));
    assert!(
        prompt.len() < 30_000,
        "compact planner prompt was {} chars",
        prompt.len()
    );

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn worker_prompt_stays_task_centered_with_large_control_docs() -> Result<()> {
    let root = temp_root("compact-worker-prompt-size");
    let paths = Paths::new(&root)?;
    write_text(&paths.roadmap, &large_control_doc("ROADMAP", 900))?;
    write_text(&paths.next_steps, &large_control_doc("NEXT", 1_200))?;
    write_text(&paths.next_steps_non_ai, &large_control_doc("NONAI", 700))?;
    let mut task = sample_task(vec![
        "CONTINUITY.md".to_string(),
        "NEXT_STEPS.md".to_string(),
    ]);
    task.task_id = "PAR-005".to_string();
    task.title = "Classify Replay Blocker".to_string();
    let state = State {
        mission: "Work on obtaining parity with Helicon Focus".to_string(),
        ..State::default()
    };

    let prompt = render_prompt_limited(
        WORKER_PROMPT,
        &paths,
        &state,
        json!({
            "role": "worker",
            "task": task,
            "support_brief": {"summary": "compact support"},
            "queue_snapshot": {"approved": 0},
        }),
        PromptLimits::worker_like(),
    )?;

    assert!(prompt.contains("PAR-005"));
    assert!(prompt.contains("...[truncated]..."));
    assert!(
        prompt.len() < 25_000,
        "compact worker prompt was {} chars",
        prompt.len()
    );

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn prompt_receipt_records_worker_sections_and_time_estimate() -> Result<()> {
    let root = temp_root("worker-context-pack-receipt");
    let paths = Paths::new(&root)?;
    fs::create_dir_all(&paths.observations)?;
    let cfg = Config::default();
    let mut task = sample_task(vec!["Sources/App/Foo.swift".to_string()]);
    task.task_id = "CHURN-001".to_string();
    task.title = "Bounded edit".to_string();
    let state = State {
        mission: "Reduce token churn".to_string(),
        ..State::default()
    };
    let support = build_task_support_brief(&paths, &cfg, &task);

    let pack = build_worker_context_pack(&paths, &cfg, &state, &task, &support, None)?;

    assert!(!pack.receipt.rejected);
    assert_eq!(pack.receipt.role, "worker");
    assert_eq!(pack.receipt.task_id.as_deref(), Some("CHURN-001"));
    assert!(pack.receipt.total_token_estimate > 0);
    assert!(pack.receipt.estimated_context_review_seconds > 0);
    assert_eq!(
        pack.receipt.hard_max_tokens,
        cfg.worker_prompt_hard_max_tokens
    );
    assert!(pack.prompt.contains("## Task Text"));
    assert!(pack.prompt.contains("## Repo Hints"));
    assert!(pack.prompt.contains("## Previous Attempts"));
    let categories = pack
        .receipt
        .sections
        .iter()
        .map(|section| section.category)
        .collect::<BTreeSet<_>>();
    assert!(categories.contains(&PromptSectionCategory::RolePrompt));
    assert!(categories.contains(&PromptSectionCategory::TaskText));
    assert!(categories.contains(&PromptSectionCategory::RepoHints));
    assert!(categories.contains(&PromptSectionCategory::FileContents));
    assert!(categories.contains(&PromptSectionCategory::Logs));
    assert!(categories.contains(&PromptSectionCategory::Diff));
    assert!(categories.contains(&PromptSectionCategory::PreviousAttempts));
    assert!(categories.contains(&PromptSectionCategory::Guidelines));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn worker_prompt_over_budget_rejects_before_runner_call() -> Result<()> {
    let root = temp_root("worker-context-pack-budget-reject");
    let paths = Paths::new(&root)?;
    init_repo(&paths, "Reduce token churn")?;
    let mut cfg = Config::default();
    cfg.worker_prompt_hard_max_tokens = 1;
    let mut task = sample_task(vec!["Sources/App/Foo.swift".to_string()]);
    task.task_id = "CHURN-002".to_string();
    let state = State::default();
    let support = build_task_support_brief(&paths, &cfg, &task);
    let runner = FakeCodexRunner::new(vec![serde_json::to_string(&sample_worker_output(
        &task.task_id,
    ))?]);

    let err = run_worker(&paths, &cfg, &state, &task, &support, None, &runner)
        .expect_err("over-budget prompt should fail before model launch");

    assert!(err.to_string().contains("exceeds hard token budget"));
    assert!(runner.calls.borrow().is_empty());
    let receipt_dir = paths.observations.join("prompt_receipts");
    let receipts = fs::read_dir(&receipt_dir)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    assert_eq!(receipts.len(), 1);
    let receipt: PromptReceipt = serde_json::from_str(&read_text_or_empty(&receipts[0]))?;
    assert!(receipt.rejected);
    assert_eq!(receipt.task_id.as_deref(), Some("CHURN-002"));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn worker_prompt_lint_rejects_bloated_prompt_shapes() {
    let pack = sample_context_pack(
        r#"
## Role Prompt
worker rules

## Logs
assistant: raw prior transcript

## Logs
assistant: duplicate raw prior transcript

## File Contents
pub fn unrelated() {}

## Diff
diff --git a/src/app.rs b/src/app.rs
@@ -1,2 +1,2 @@
-old
+new
"#,
    );

    let reason = worker_prompt_lint_rejection_reason(&pack)
        .expect("bloated prompt shapes should be rejected");

    assert!(reason.contains("duplicate logs blocks"));
    assert!(reason.contains("full prior transcript markers"));
    assert!(reason.contains("unrelated file contents"));
    assert!(reason.contains("full diff payload"));
}

#[test]
fn attempt_capsule_omits_duplicate_retry_logs_and_diffs() -> Result<()> {
    let root = temp_root("attempt-capsule-duplicate-retry");
    fs::create_dir_all(&root)?;
    let paths = Paths::new(&root)?;
    let cfg = Config::default();
    let mut task = sample_task(vec!["Sources/App/Foo.swift".to_string()]);
    task.task_id = "CHURN-003".to_string();
    task.objective = "Fix only the local compile failure.".to_string();
    let state = State::default();
    let support = build_task_support_brief(&paths, &cfg, &task);
    let retry = WorkerRetryBrief {
        task_id: task.task_id.clone(),
        attempt: 2,
        route: WorkerNonDoneRoute::RetryWorkerSameTask.as_str().to_string(),
        worker_status: "blocked".to_string(),
        blocker: "RAW_LOG_SHOULD_NOT_APPEAR ".repeat(300),
        changed_files: vec!["Sources/App/Foo.swift".to_string()],
        diff_stat: format!(
            "Sources/App/Foo.swift | 4 ++--\n{}RAW_DIFF_SHOULD_NOT_APPEAR",
            "x".repeat(2_000)
        ),
        diagnostic_command: Some("swift test --filter FooTests".to_string()),
        diagnostic_category: Some("commandFailed".to_string()),
        diagnostic_summary: Some("Sources/App/Foo.swift:9: missing argument label".to_string()),
        fingerprint: Some(FailureFingerprint {
            kind: FailureFingerprintKind::SwiftCompileError,
            file: Some("Sources/App/Foo.swift".to_string()),
            line: Some(9),
            message: "missing argument label".to_string(),
            command: Some("swift test --filter FooTests".to_string()),
        }),
        fingerprint_retry_count: 1,
        repair_instruction: "Fix only the missing argument label.".to_string(),
        ..WorkerRetryBrief::default()
    };

    let pack = build_worker_context_pack(&paths, &cfg, &state, &task, &support, Some(&retry))?;

    assert!(pack
        .prompt
        .contains("Same failure fingerprint as prior attempt"));
    assert!(pack.prompt.contains("Full duplicate log omitted"));
    assert!(pack.prompt.contains("Full duplicate diff omitted"));
    assert!(pack.prompt.contains("full previous Worker transcript"));
    assert!(!pack.prompt.contains("RAW_LOG_SHOULD_NOT_APPEAR"));
    assert!(!pack.prompt.contains("RAW_DIFF_SHOULD_NOT_APPEAR"));
    let log_section = pack
        .receipt
        .sections
        .iter()
        .find(|section| section.category == PromptSectionCategory::Logs)
        .expect("logs section should be recorded");
    assert!(log_section.token_estimate <= log_section.max_tokens);
    let diff_section = pack
        .receipt
        .sections
        .iter()
        .find(|section| section.category == PromptSectionCategory::Diff)
        .expect("diff section should be recorded");
    assert!(diff_section.token_estimate <= diff_section.max_tokens);

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn senior_review_prompt_uses_compact_worker_receipt() -> Result<()> {
    let root = temp_root("compact-senior-review-prompt-size");
    let paths = Paths::new(&root)?;
    write_text(&paths.roadmap, &large_control_doc("ROADMAP", 900))?;
    write_text(&paths.next_steps, &large_control_doc("NEXT", 1_200))?;
    write_text(&paths.next_steps_non_ai, &large_control_doc("NONAI", 700))?;
    let mut task = sample_task(vec![
        "CONTINUITY.md".to_string(),
        "Docs/exec-plans/active/replay.md".to_string(),
        "NEXT_STEPS.md".to_string(),
    ]);
    task.task_id = "PAR-007".to_string();
    task.title = "Run one bounded reference-backed replay rerun".to_string();
    let mut worker = sample_worker_output("PAR-007");
    worker.summary = "bounded evidence captured\n".repeat(400);
    worker.blocker = "fixture decode blocker\n".repeat(300);
    worker.files_changed = (0..80)
        .map(|index| format!("Docs/generated/very-long-path-{index}.md"))
        .collect();
    worker.commands_run = (0..40)
        .map(|index| {
            format!(
                "swift run -c release SwiftStackerBench --suite Benchmarks/Suites/suite.json --report-root Benchmarks/ReportsDiagnostics/run-{index} --enable-second-pass-merge-replay-diagnostics"
            )
        })
        .collect();
    worker.tests_run = (0..40)
        .map(|index| format!("swift test --filter ReplayEvidenceTests{index}"))
        .collect();
    let state = State {
        mission: "Work on obtaining parity with Helicon Focus".to_string(),
        ..State::default()
    };

    let prompt = render_prompt_limited(
        FOREMAN_REVIEW_PROMPT,
        &paths,
        &state,
        json!({
            "role": "foreman",
            "mode": "review",
            "task": task,
            "worker_result": compact_worker_review_result(&worker),
            "orchestrator_guard_report": clean_guard(vec!["CONTINUITY.md".to_string()]),
            "rust_verification": {"status": "passed"},
            "review_context": {"summary": "compact review context"},
            "queue_snapshot": {"approved": 0},
        }),
        PromptLimits::senior_review(),
    )?;

    assert!(prompt.contains("omitted_counts"));
    assert!(prompt.contains("...[truncated]..."));
    assert!(
        prompt.len() < 20_000,
        "compact Senior review prompt was {} chars",
        prompt.len()
    );

    fs::remove_dir_all(root)?;
    Ok(())
}

fn large_control_doc(label: &str, lines: usize) -> String {
    let mut text = format!("# {label}\n\n");
    for index in 0..lines {
        text.push_str(&format!(
            "- {label} line {index}: preserve mission, parity evidence, blockers, task history, and safe local execution constraints.\n"
        ));
    }
    text
}

#[test]
fn tracking_doc_completion_notes_append_only_to_allowed_docs() -> Result<()> {
    let root = temp_root("tracking-doc-notes");
    fs::create_dir_all(root.join("docs"))?;
    write_text(&root.join("CONTINUITY.md"), "continuity\n")?;
    write_text(&root.join("TASK.md"), "task\n")?;
    write_text(&root.join("docs/ROADMAP.md"), "roadmap\n")?;
    let paths = Paths::new(&root)?;
    let mut task = sample_task(vec![
        "CONTINUITY.md".to_string(),
        "docs/ROADMAP.md".to_string(),
    ]);
    task.task_id = "TASK-191".to_string();
    let report = EvidenceReport {
        report_id: "report-1".to_string(),
        plan_id: "plan-1".to_string(),
        task_id: task.task_id.clone(),
        status: "passed".to_string(),
        started_at: Utc::now().to_rfc3339(),
        completed_at: Utc::now().to_rfc3339(),
        summary: "passed".to_string(),
        commands: Vec::new(),
        rejected_commands: Vec::new(),
    };

    let changed = append_tracking_doc_completion_notes(&paths, &task, &report, "accepted_clean")?;
    assert_eq!(changed, vec!["CONTINUITY.md", "docs/ROADMAP.md"]);
    assert!(read_text(&root.join("CONTINUITY.md"))?.contains("Rust completion note"));
    assert!(!read_text(&root.join("TASK.md"))?.contains("Rust completion note"));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn tracking_doc_completion_notes_include_uppercase_docs_variants() -> Result<()> {
    let root = temp_root("tracking-doc-notes-uppercase");
    fs::create_dir_all(root.join("Docs"))?;
    write_text(&root.join("Docs/ROADMAP.md"), "roadmap\n")?;
    write_text(&root.join("Docs/.INDEX.md"), "index\n")?;
    let paths = Paths::new(&root)?;
    let mut task = sample_task(vec![
        "Docs/ROADMAP.md".to_string(),
        "Docs/.INDEX.md".to_string(),
    ]);
    task.task_id = "TASK-DOCS".to_string();
    let report = EvidenceReport {
        report_id: "report-1".to_string(),
        plan_id: "plan-1".to_string(),
        task_id: task.task_id.clone(),
        status: "passed".to_string(),
        started_at: Utc::now().to_rfc3339(),
        completed_at: Utc::now().to_rfc3339(),
        summary: "passed".to_string(),
        commands: Vec::new(),
        rejected_commands: Vec::new(),
    };

    let changed = append_tracking_doc_completion_notes(&paths, &task, &report, "accepted_clean")?;

    assert_eq!(
        changed,
        vec!["Docs/ROADMAP.md".to_string(), "Docs/.INDEX.md".to_string()]
    );
    assert!(read_text(&root.join("Docs/ROADMAP.md"))?.contains("Rust completion note"));
    assert!(read_text(&root.join("Docs/.INDEX.md"))?.contains("Rust completion note"));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn parity_preflight_blocks_renderer_campaign_task_without_packet() -> Result<()> {
    let root = temp_root("parity-preflight-missing");
    fs::create_dir_all(root.join(".orchestrator"))?;
    let paths = Paths::new(&root)?;
    let task = sample_task(vec![
        "Sources/SwiftStackerCore/FocusStackRenderer.swift".to_string(),
        "Tests/SwiftStackerCoreTests/TiledRendererTests.swift".to_string(),
    ]);
    let policy = task_policy_from_task(&task);

    let findings = validate_task_policy(&paths, &task, &policy);

    assert!(findings.iter().any(|finding| {
        finding.message.contains("analysis/process_guardrails.md")
            && finding.message.contains("Renderer/Campaign Preflight")
    }));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn parity_preflight_accepts_packet_from_active_execplan() -> Result<()> {
    let root = temp_root("parity-preflight-active-plan");
    fs::create_dir_all(root.join("docs/exec-plans/active"))?;
    fs::create_dir_all(root.join(".orchestrator"))?;
    write_text(
        &root.join("docs/exec-plans/active/current-renderer.md"),
        &renderer_campaign_preflight_text(),
    )?;
    let paths = Paths::new(&root)?;
    let task = sample_task(vec!["Scripts/analyze_current_campaign.py".to_string()]);
    let policy = task_policy_from_task(&task);

    let findings = validate_task_policy(&paths, &task, &policy);

    assert!(findings.is_empty());

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn parity_preflight_does_not_block_docs_only_planning_task() -> Result<()> {
    let root = temp_root("parity-preflight-docs-only");
    fs::create_dir_all(root.join(".orchestrator"))?;
    let paths = Paths::new(&root)?;
    let task = sample_task(vec![
        "CONTINUITY.md".to_string(),
        "PARITY.md".to_string(),
        "Docs/exec-plans/active/current.md".to_string(),
    ]);
    let policy = task_policy_from_task(&task);

    let findings = validate_task_policy(&paths, &task, &policy);

    assert!(findings.is_empty());

    fs::remove_dir_all(root)?;
    Ok(())
}

fn init_git_repo(root: &Path) -> Result<()> {
    let init = Command::new("git").arg("init").current_dir(root).status()?;
    assert!(init.success());
    configure_test_git_identity(root)?;
    let add = Command::new("git")
        .args(["add", "."])
        .current_dir(root)
        .status()?;
    assert!(add.success());
    let commit = Command::new("git")
        .args([
            "-c",
            "user.name=Codex Test",
            "-c",
            "user.email=codex@example.invalid",
            "commit",
            "-m",
            "initial",
        ])
        .current_dir(root)
        .status()?;
    assert!(commit.success());
    Ok(())
}

fn configure_test_git_identity(root: &Path) -> Result<()> {
    // Later commit-tree operations need a repository identity too; the initial
    // commit's -c overrides do not persist beyond that one Git subprocess.
    for (key, value) in [
        ("user.name", "Codex Test"),
        ("user.email", "codex@example.invalid"),
    ] {
        let configured = Command::new("git")
            .args(["config", "--local", key, value])
            .current_dir(root)
            .status()?;
        assert!(configured.success());
    }
    Ok(())
}

#[test]
fn markdown_queue_sync_retains_digest_bound_imported_task() -> Result<()> {
    let root = temp_root("imported-task-sync");
    fs::create_dir_all(&root)?;
    write_text(
        &root.join("NEXT_STEPS.md"),
        "# NEXT_STEPS\n\nNo imported tasks.\n",
    )?;
    let paths = Paths::new(&root)?;
    let task = sample_task(vec!["README.md".to_string()]);
    let mut state = State::default();
    state.queue.push(task.clone());
    state.imported_tasks.insert(
        task.task_id.clone(),
        ImportedTaskRecord {
            schema: "codex-team.imported_task_record.v1".to_string(),
            task_id: task.task_id.clone(),
            import_digest: "a".repeat(64),
            source: "ca".to_string(),
            base_head: "b".repeat(40),
            imported_at: Utc::now().to_rfc3339(),
            receipt_path: ".orchestrator/imports/T001.json".to_string(),
        },
    );

    assert_eq!(sync_state_queue_with_next_steps(&paths, &mut state), 0);
    assert_eq!(state.queue.len(), 1);
    assert_eq!(state.queue[0].task_id, task.task_id);

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn advisory_override_never_substitutes_an_explicit_ca_worker_engine() {
    let mut cfg = Config::default();
    cfg.ca.enabled = true;
    cfg.roles.worker.provider = RoleProvider::Ca;
    cfg.mlx_lm.enabled = true;
    apply_advisory_provider_override(
        &mut cfg,
        Some(&ProviderHintReceipt {
            recommended_provider: "mlx_lm".to_string(),
            reason: "cheaper advisory".to_string(),
        }),
    );
    assert_eq!(cfg.roles.worker.provider, RoleProvider::Ca);
}

#[test]
fn imported_worker_acceptance_leaves_next_steps_byte_identical() -> Result<()> {
    let root = temp_root("imported-task-acceptance-markdown");
    fs::create_dir_all(&root)?;
    write_text(&root.join("README.md"), "before\n")?;
    let next_steps = "# NEXT_STEPS\n\nHuman-owned queue bytes stay exact.\n";
    write_text(&root.join("NEXT_STEPS.md"), next_steps)?;
    init_git_repo(&root)?;
    let paths = Paths::new(&root)?;
    let cfg = Config::default();
    let task = sample_task(vec!["README.md".to_string()]);
    let mut state = State::default();
    state.imported_tasks.insert(
        task.task_id.clone(),
        ImportedTaskRecord {
            schema: "codex-team.imported_task_record.v1".to_string(),
            task_id: task.task_id.clone(),
            import_digest: "c".repeat(64),
            source: "ca".to_string(),
            base_head: "d".repeat(40),
            imported_at: Utc::now().to_rfc3339(),
            receipt_path: ".orchestrator/imports/T001.json".to_string(),
        },
    );
    write_text(&root.join("README.md"), "before\nimported edit\n")?;
    let guard = clean_guard(vec!["README.md".to_string()]);
    let mut report = evidence_report(Vec::new());
    report.status = "passed".to_string();
    report.summary = "passed".to_string();

    let commit = rust_accept_worker_task(
        &paths,
        &cfg,
        &mut state,
        WorkerAcceptance {
            task: &task,
            guard_report: &guard,
            evidence_report: &report,
            reason_code: "rust_auto_accept_verified",
            commit_message: "Complete imported task",
        },
    )?;

    assert!(commit.is_some());
    assert_eq!(read_text(&paths.next_steps)?, next_steps);
    assert!(!head_commit_files(&root)?
        .lines()
        .any(|line| line == "NEXT_STEPS.md"));

    fs::remove_dir_all(root)?;
    Ok(())
}

fn head_commit_files(root: &Path) -> Result<String> {
    let output = Command::new("git")
        .args(["show", "--name-only", "--format=", "HEAD"])
        .current_dir(root)
        .output()?;
    assert!(output.status.success());
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn head_commit_subject(root: &Path) -> Result<String> {
    let output = Command::new("git")
        .args(["log", "--format=%s", "-1"])
        .current_dir(root)
        .output()?;
    assert!(output.status.success());
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[test]
fn worker_guard_reports_only_unallowed_changes() -> Result<()> {
    let root = temp_root("guard");
    fs::create_dir_all(root.join("src"))?;
    write_text(&root.join("src/lib.rs"), "before")?;
    write_text(&root.join("README.md"), "before")?;
    write_text(&root.join("NEXT_STEPS.md"), "before")?;

    let paths = Paths::new(&root)?;
    let cfg = Config::default();
    let before = snapshot_repo(&paths, &cfg)?;

    write_text(&root.join("src/lib.rs"), "after")?;
    write_text(&root.join("README.md"), "after")?;
    write_text(&root.join("NEXT_STEPS.md"), "after")?;
    write_text(&root.join("target/generated.txt"), "ignored")?;

    let after = snapshot_repo(&paths, &cfg)?;
    let report = build_guard_report(
        &cfg,
        &sample_task(vec!["src/**".to_string()]),
        &before,
        &after,
    )?;

    assert!(report.changed_files.contains(&"src/lib.rs".to_string()));
    assert!(report.changed_files.contains(&"README.md".to_string()));
    assert!(!report
        .changed_files
        .contains(&"target/generated.txt".to_string()));
    assert_eq!(
        report.violations,
        vec!["NEXT_STEPS.md".to_string(), "README.md".to_string()]
    );

    let tracking_report = build_guard_report(
        &cfg,
        &sample_task(vec!["src/**".to_string(), "tracking docs".to_string()]),
        &before,
        &after,
    )?;
    assert_eq!(tracking_report.violations, vec!["README.md".to_string()]);
    assert!(tracking_report
        .allowed_patterns
        .contains(&"NEXT_STEPS.md".to_string()));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn guard_ignores_generated_swiftstacker_artifacts_but_not_analysis_scripts() -> Result<()> {
    let root = temp_root("guard-swiftstacker-artifacts");
    fs::create_dir_all(root.join("Benchmarks/Reports/run"))?;
    fs::create_dir_all(root.join("Benchmarks/ReportsDataset/run"))?;
    fs::create_dir_all(root.join("Benchmarks/Stacks/private-stack"))?;
    fs::create_dir_all(root.join("SwiftStacker.app/Contents"))?;
    fs::create_dir_all(root.join("HeliconFocus.app/Contents"))?;
    fs::create_dir_all(root.join("export"))?;
    fs::create_dir_all(root.join("analysis/codex-wrapper/run-001/prompts"))?;
    fs::create_dir_all(root.join("analysis/graph-smoke"))?;
    fs::create_dir_all(root.join("analysis"))?;
    fs::create_dir_all(root.join("Scripts"))?;
    write_text(&root.join("analysis/report.md"), "old\n")?;
    write_text(&root.join("analysis/score_summary.csv"), "old\n")?;
    write_text(&root.join("Scripts/probe.py"), "old\n")?;
    let paths = Paths::new(&root)?;
    let cfg = Config::default();
    let task = sample_task(vec![
        "analysis/report.md".to_string(),
        "analysis/score_summary.csv".to_string(),
        "Scripts/probe.py".to_string(),
    ]);

    let before = snapshot_repo(&paths, &cfg)?;
    write_text(
        &root.join("Benchmarks/Reports/run/benchmark-report.json"),
        "{}\n",
    )?;
    write_text(
        &root.join("Benchmarks/ReportsDataset/run/suite-summary.json"),
        "{}\n",
    )?;
    write_text(
        &root.join("Benchmarks/Stacks/private-stack/source_001.jpg"),
        "image\n",
    )?;
    write_text(
        &root.join("SwiftStacker.app/Contents/Info.plist"),
        "plist\n",
    )?;
    write_text(
        &root.join("HeliconFocus.app/Contents/Info.plist"),
        "plist\n",
    )?;
    write_text(&root.join("export/100000000_probe.c"), "export\n")?;
    write_text(
        &root.join("analysis/codex-wrapper/run-001/prompts/batch-0000.md"),
        "prompt\n",
    )?;
    write_text(&root.join("analysis/graph-smoke/symbols.tsv"), "symbols\n")?;
    write_text(&root.join("analysis/report.md"), "new\n")?;
    write_text(&root.join("analysis/score_summary.csv"), "new\n")?;
    write_text(&root.join("Scripts/probe.py"), "new\n")?;
    let after = snapshot_repo(&paths, &cfg)?;

    let report = build_guard_report(&cfg, &task, &before, &after)?;
    assert_eq!(
        report.changed_files,
        vec![
            "Scripts/probe.py".to_string(),
            "analysis/report.md".to_string(),
            "analysis/score_summary.csv".to_string()
        ]
    );
    assert!(report.violations.is_empty());

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn legacy_role_config_loads_with_new_labels() -> Result<()> {
    let raw = r#"
top_every_completed_tasks = 3
auto_escalate_to_top = true

[role_labels]
top = "God"
middle = "Master"
bottom = "Slave"

[guard]
enforce_bottom_allowed_files = true

[roles.top]
model = "gpt-5.5"
effort = "high"
sandbox = "workspace-write"
approval_policy = "never"
web_search = "cached"
timeout_seconds = 1800

[roles.middle]
model = "gpt-5.4"
effort = "medium"
sandbox = "workspace-write"
approval_policy = "never"
web_search = "cached"
timeout_seconds = 1800

[roles.bottom]
model = "gpt-5.4-mini"
effort = "low"
sandbox = "workspace-write"
approval_policy = "never"
web_search = "disabled"
timeout_seconds = 900
"#;
    let mut cfg: Config = toml::from_str(raw)?;

    migrate_legacy_middle_role_to_planner(&mut cfg, raw);
    normalize_legacy_role_labels(&mut cfg);

    assert_eq!(cfg.master_every_completed_tasks, 3);
    assert!(cfg.auto_escalate_to_master);
    assert!(!cfg.intern_preflight);
    assert_eq!(cfg.intern_support, InternSupport::AutoCheap);
    assert!(cfg.intern_read_only_commands);
    assert!(cfg.deterministic_tasks);
    assert!(cfg.automatic_recipe_harvest);
    assert_eq!(
        cfg.recipe_harvest_recent_logs,
        default_recipe_harvest_recent_logs()
    );
    assert_eq!(cfg.planner_context, PlannerContextMode::Compact);
    assert!(cfg.guard.enforce_worker_allowed_files);
    assert_eq!(cfg.role_labels.master, "CEO");
    assert_eq!(cfg.role_labels.planner, "Project Manager");
    assert_eq!(cfg.role_labels.foreman, "Senior Dev");
    assert_eq!(cfg.role_labels.worker, "Worker");
    assert_eq!(cfg.role_labels.intern, "Intern");
    assert_eq!(cfg.roles.master.model, "gpt-5.5");
    assert_eq!(cfg.roles.planner.model, "gpt-5.4");
    assert_eq!(cfg.roles.foreman.model, "gpt-5.5");
    assert_eq!(cfg.roles.foreman.effort, "high");
    assert_eq!(cfg.roles.worker.model, "gpt-5.4-mini");
    assert_eq!(cfg.roles.intern.model, "gpt-5.3-codex-spark");
    assert_eq!(cfg.roles.intern.effort, "low");
    assert_eq!(cfg.roles.intern.sandbox, "read-only");
    Ok(())
}

#[test]
fn default_roles_make_pm_cheaper_than_senior_gatekeeper() {
    let cfg = Config::default();

    assert_eq!(cfg.roles.planner.model, "gpt-5.4");
    assert_eq!(cfg.roles.planner.effort, "medium");
    assert_eq!(cfg.roles.foreman.model, "gpt-5.5");
    assert_eq!(cfg.roles.foreman.effort, "high");
    assert!(role_strength_score(&cfg.roles.foreman) > role_strength_score(&cfg.roles.planner));
}

#[test]
fn continue_prompt_choices_parse() {
    assert_eq!(parse_continue_choice(""), ContinueChoice::Once);
    assert_eq!(parse_continue_choice("y"), ContinueChoice::Once);
    assert_eq!(parse_continue_choice("yes"), ContinueChoice::Once);
    assert_eq!(parse_continue_choice("a"), ContinueChoice::Auto);
    assert_eq!(parse_continue_choice("auto"), ContinueChoice::Auto);
    assert_eq!(parse_continue_choice("n"), ContinueChoice::Stop);
    assert_eq!(parse_continue_choice("stop"), ContinueChoice::Stop);
    assert_eq!(parse_continue_choice("wat"), ContinueChoice::Stop);
}

#[test]
fn human_disposition_prompt_choices_parse() {
    assert_eq!(
        parse_human_disposition_choice("p"),
        HumanDispositionChoice::PreserveAndStop
    );
    assert_eq!(
        parse_human_disposition_choice("quarantine"),
        HumanDispositionChoice::PreserveAndStop
    );
    assert_eq!(
        parse_human_disposition_choice("r"),
        HumanDispositionChoice::Recheck
    );
    assert_eq!(
        parse_human_disposition_choice("gate"),
        HumanDispositionChoice::ApproveGate
    );
    assert_eq!(
        parse_human_disposition_choice(""),
        HumanDispositionChoice::Stop
    );
    assert_eq!(
        parse_human_disposition_choice("keep-paused"),
        HumanDispositionChoice::Stop
    );
    assert_eq!(
        parse_human_disposition_choice("wat"),
        HumanDispositionChoice::Stop
    );
}

#[test]
fn mission_summary_shows_active_human_disposition_pause() -> Result<()> {
    let root = temp_root("summary-active-human-pause");
    fs::create_dir_all(&root)?;
    let paths = Paths::new(&root)?;
    let state = State {
        human_disposition_required: Some(HumanDispositionPause {
            source: "rust_dirty_adoption".to_string(),
            reason: "choose a disposition before roles continue".to_string(),
            options: vec!["preserve".to_string(), "recheck".to_string()],
        }),
        ..State::default()
    };

    let rows = mission_summary_rows(&paths, &state);

    assert!(rows.iter().any(|(label, value)| {
        label == "PAUSED" && value.contains("human_disposition source=rust_dirty_adoption")
    }));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn mission_summary_hides_resolved_human_disposition_decision() -> Result<()> {
    let root = temp_root("summary-resolved-human-decision");
    fs::create_dir_all(&root)?;
    let paths = Paths::new(&root)?;
    let state = State {
        human_disposition_decision: Some(HumanDispositionDecision {
            decision: "preserve_quarantine_stop".to_string(),
            summary:
                "Human chose to preserve/quarantine the dirty set as user-owned context and stop."
                    .to_string(),
            decided_at: "2026-06-29T00:00:00Z".to_string(),
        }),
        ..State::default()
    };

    let rows = mission_summary_rows(&paths, &state);

    assert!(!rows.iter().any(|(label, _)| label == "HUMAN"));
    assert!(!rows
        .iter()
        .any(|(_, value)| value.contains("preserve_quarantine_stop")));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn live_renderer_helpers_make_codex_json_human_sized() {
    assert_eq!(
        friendly_command("/bin/zsh -lc 'git status --short'"),
        "git status --short"
    );

    let preview = preview_text("one\ntwo\nthree\nfour", 2, 100);
    assert_eq!(preview.lines, vec!["one".to_string(), "two".to_string()]);
    assert!(preview.truncated);

    assert_eq!(truncate_inline("abc\ndef", 20), "abc def");
    assert!(is_high_signal_success_output("swift build"));
    assert!(is_high_signal_success_output("cargo test"));
    assert!(is_high_signal_success_output("npm test"));
    assert!(is_high_signal_success_output("python -m pytest tests"));
    assert!(is_high_signal_success_output("git status --short"));
    assert!(!is_high_signal_success_output("sed -n '1,20p' README.md"));
    assert!(!is_live_start_command("git status --short"));
    assert!(!is_live_start_command("sed -n '1,20p' README.md"));
    assert!(is_live_start_command("swift build"));
    assert!(is_live_start_command("go test ./..."));
    assert!(is_non_actionable_stderr(
        "ERROR codex_core::session: failed to record rollout items"
    ));
    assert!(summarize_codex_tool_stderr(
        "ERROR codex_core::tools::router: error=apply_patch verification failed"
    )
    .is_some());
    assert!(summarize_codex_tool_stderr(
        "ERROR codex_core::tools::router: error=exec_command failed for `/bin/zsh -lc`"
    )
    .is_some());
    assert!(is_codex_tool_stderr_detail(
        "- expected apply_patch context line"
    ));
    assert!(!is_codex_tool_stderr_detail(
        "2026-04-27T00:00:00Z ERROR another subsystem"
    ));
    assert!(is_rg_no_matches("rg -n \"Needle\" Sources Tests", Some(1)));
    assert_eq!(
        rg_no_matches_activity("rg -n \"Needle\" Sources Tests"),
        "No matches for Needle"
    );
    assert!(is_swiftpm_sandbox_failure(
        "swift test --filter AppCoordinatorTests",
        Some(1),
        "sandbox-exec: sandbox_apply: Operation not permitted"
    ));
    assert!(is_swiftpm_sandbox_failure(
            "swift build --product app-admin",
            Some(1),
            "error opening '/Users/me/.cache/clang/ModuleCache/Swift.swiftmodule' for output: /Users/me/.cache/clang/ModuleCache: Operation not permitted"
        ));
    assert!(!is_swiftpm_sandbox_failure(
        "swift test --filter AppCoordinatorTests",
        Some(1),
        "XCTAssertEqual failed"
    ));
    assert!(is_deterministic_command_allowed(
        "swift test --filter AppCoordinatorTests"
    ));
    assert!(is_deterministic_command_allowed(
        "swift build -c release --product SwiftStackerBench"
    ));
    assert!(is_deterministic_command_allowed(
        "swift run -c release SwiftStackerBench --suite Benchmarks/suite.json --report-root Benchmarks/Reports"
    ));
    assert!(is_deterministic_command_allowed(
        "swift run -c release SwiftStackerBench --suite Benchmarks/focus_stack_dataset_suite.json --report-root Benchmarks/ReportsDataset --generate-gallery"
    ));
    assert!(is_deterministic_command_allowed(
        "rg -n \"Needle\" NEXT_STEPS.md"
    ));
    assert!(!is_deterministic_command_allowed(
        "swift test --filter A && git diff --check"
    ));
    assert!(!is_deterministic_command_allowed(
        "swift build -c release --product SwiftStacker"
    ));
    assert!(!is_deterministic_command_allowed(
        "swift run -c release SwiftStackerBench --suite /tmp/suite.json --report-root Benchmarks/Reports"
    ));
    assert!(!is_deterministic_command_allowed(
        "swift run -c release SwiftStackerBench --suite Benchmarks/suite.json --report-root /tmp/report"
    ));
    assert!(!is_deterministic_command_allowed(
        "swift run -c release SwiftStackerBench --suite Benchmarks/suite.json --report-root analysis/Reports"
    ));
    assert!(is_deterministic_command_allowed(
        ".build/debug/app --local-transport-send-receive-smoke"
    ));
    assert!(is_deterministic_command_allowed("cargo check"));
    assert!(is_deterministic_command_allowed("cargo test parser_tests"));
    assert!(is_deterministic_command_allowed("git diff --stat"));
    assert!(!is_deterministic_command_allowed(
        ".build/debug/app --public-diagnostic"
    ));
    assert!(!is_deterministic_command_allowed("unknown-tool --version"));
    assert!(!is_deterministic_command_allowed("cargo test & echo nope"));
    assert!(!is_deterministic_command_allowed(
        ".build/debug/../tool --local-smoke"
    ));
    assert!(!is_deterministic_command_allowed(
        ".build/debug/app --local-smoke && git diff --check"
    ));
    assert_eq!(
        classify_deterministic_result("rg -n \"Needle\" Sources", 1, ""),
        DeterministicResultCategory::NoMatches
    );
    assert_eq!(
        classify_deterministic_result(
            "swift build --product app-admin",
            1,
            "sandbox-exec: sandbox_apply: Operation not permitted"
        ),
        DeterministicResultCategory::KnownEnvironmentBlocker
    );
    assert_eq!(
        classify_deterministic_result("swift test --filter FooTests", 1, "XCTAssert failed"),
        DeterministicResultCategory::CommandFailed
    );
    assert!(is_deterministic_command_structured_allowed(&vc(
        "rg -n \"Needle\" Sources Tests"
    )));
    assert!(is_deterministic_command_structured_allowed(&vc(
        "git status --short"
    )));
    assert!(!is_deterministic_command_allowed(
        "rg -n Foo README.md | cat"
    ));
}

#[test]
fn recipe_parser_converts_legacy_commands_to_typed_recipes() {
    let swift = parse_legacy_recipe_spec("swift build --product app-admin", "test")
        .expect("swift product build parses");
    assert!(matches!(
        swift.recipe,
        Recipe::SwiftBuild {
            product: Some(ref product)
        } if product == "app-admin"
    ));
    let command = recipe_spec_to_command(swift);
    assert_eq!(command.display(), "swift build --product app-admin");

    let release_build =
        parse_legacy_recipe_spec("swift build -c release --product SwiftStackerBench", "test")
            .expect("swift release product build parses");
    assert!(matches!(
        release_build.recipe,
        Recipe::SwiftReleaseBuild { ref product } if product == "SwiftStackerBench"
    ));
    let command = recipe_spec_to_command(release_build);
    assert_eq!(
        command.display(),
        "swift build -c release --product SwiftStackerBench"
    );

    let benchmark = parse_legacy_recipe_spec(
        "swift run -c release SwiftStackerBench --suite Benchmarks/focus_stack_dataset_suite.json --report-root Benchmarks/ReportsDataset --generate-gallery",
        "test",
    )
    .expect("swiftstacker benchmark run parses");
    assert!(matches!(
        benchmark.recipe,
        Recipe::SwiftStackerBenchmarkRun {
            ref suite,
            ref report_root,
            generate_gallery: true,
        } if suite == "Benchmarks/focus_stack_dataset_suite.json"
            && report_root == "Benchmarks/ReportsDataset"
    ));
    let command = recipe_spec_to_command(benchmark);
    assert_eq!(
        command.display(),
        "swift run -c release SwiftStackerBench --suite Benchmarks/focus_stack_dataset_suite.json --report-root Benchmarks/ReportsDataset --generate-gallery"
    );

    let rg = parse_legacy_recipe_spec("rg -n Needle Sources Tests", "test").expect("rg parses");
    assert!(matches!(rg.recipe, Recipe::Ripgrep { ref pattern, .. } if pattern == "Needle"));

    assert!(parse_legacy_recipe_spec("cargo test && git diff --check", "test").is_none());
    assert!(parse_legacy_recipe_spec("unknown-tool --version", "test").is_none());
}

#[test]
fn verification_rejects_existing_swiftstacker_report_root_without_explicit_allow() -> Result<()> {
    let root = temp_root("verification-report-root-reject");
    fs::create_dir_all(root.join(".orchestrator"))?;
    fs::create_dir_all(root.join("Benchmarks/ReportsDataset"))?;
    let paths = Paths::new(&root)?;
    let cfg = Config::default();
    let mut task = sample_task(vec![
        "Sources/SwiftStackerCore/FocusStackRenderer.swift".to_string()
    ]);
    task.tests_to_run = vec![
        "swift run -c release SwiftStackerBench --suite Benchmarks/focus_stack_dataset_suite.json --report-root Benchmarks/ReportsDataset --generate-gallery".to_string(),
    ];
    let worker = sample_worker_output(&task.task_id);

    let plan = build_verification_plan_with_observation_hints(&paths, &cfg, &task, &worker)?;

    assert!(plan.commands.is_empty());
    assert_eq!(plan.rejected_commands.len(), 1);
    assert!(plan.rejected_commands[0].contains("existing report root"));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn verification_allows_existing_swiftstacker_report_root_when_task_names_it() -> Result<()> {
    let root = temp_root("verification-report-root-allow");
    fs::create_dir_all(root.join(".orchestrator"))?;
    fs::create_dir_all(root.join("Benchmarks/ReportsDataset"))?;
    let paths = Paths::new(&root)?;
    let cfg = Config::default();
    let mut task = sample_task(vec![
        "Sources/SwiftStackerCore/FocusStackRenderer.swift".to_string(),
        "Benchmarks/ReportsDataset/**".to_string(),
    ]);
    task.tests_to_run = vec![
        "swift run -c release SwiftStackerBench --suite Benchmarks/focus_stack_dataset_suite.json --report-root Benchmarks/ReportsDataset --generate-gallery".to_string(),
    ];
    let worker = sample_worker_output(&task.task_id);

    let plan = build_verification_plan_with_observation_hints(&paths, &cfg, &task, &worker)?;

    assert_eq!(plan.commands.len(), 1);
    assert!(plan.rejected_commands.is_empty());

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn cargo_json_diagnostics_parse_into_findings() {
    let line = json!({
        "reason": "compiler-message",
        "message": {
            "level": "error",
            "message": "cannot find value `x` in this scope",
            "code": {"code": "E0425"},
            "spans": [{
                "file_name": "src/main.rs",
                "line_start": 12,
                "column_start": 5,
                "is_primary": true
            }]
        }
    })
    .to_string();
    let findings = parse_cargo_json_diagnostics(&line, FindingSource::CargoCheck);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].severity, Severity::Error);
    assert_eq!(findings[0].path.as_deref(), Some(Path::new("src/main.rs")));
    assert_eq!(findings[0].line, Some(12));
    assert_eq!(findings[0].code.as_deref(), Some("E0425"));
}

#[test]
fn problem_matcher_and_diff_filter_reduce_findings() {
    let matcher = default_problem_matchers()
        .into_iter()
        .find(|matcher| matcher.name == "swift-compiler")
        .unwrap();
    let findings = match_problem_output(
        "Sources/Foo.swift:42:9: error: cannot find 'bar' in scope",
        &matcher,
    );
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].line, Some(42));

    let diff = parse_unified_diff(
        r#"diff --git a/Sources/Foo.swift b/Sources/Foo.swift
--- a/Sources/Foo.swift
+++ b/Sources/Foo.swift
@@ -40,3 +40,4 @@
 context
+let bar = 1
 unchanged
"#,
    );
    let filtered = filter_findings_to_diff(
        findings,
        &diff,
        DiffFilterMode::DiffContext { context_lines: 2 },
    );
    assert_eq!(filtered.nearby_diff.len(), 1);
    assert_eq!(filtered.outside_diff_count, 0);
}

#[test]
fn action_cache_key_is_stable_for_same_recipe_and_head() -> Result<()> {
    let root = temp_root("action-cache-key");
    write_text(&root.join("README.md"), "hello\n")?;
    init_git_repo(&root)?;
    let paths = Paths::new(&root)?;
    let recipe = parse_legacy_recipe_spec("rg -n hello README.md", "test").unwrap();
    let key_one = action_cache_key_for_recipe(&paths, &recipe, &["README.md".to_string()]);
    let key_two = action_cache_key_for_recipe(&paths, &recipe, &["README.md".to_string()]);
    assert_eq!(key_one, key_two);
    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn dependency_policy_recipes_trigger_on_manifest_changes() {
    let recipes =
        dependency_policy_recipes_for_paths(&["Cargo.toml".to_string(), "src/main.rs".to_string()]);
    assert!(recipes.contains(&DependencyRecipe::CargoDeny));
    assert!(recipes.contains(&DependencyRecipe::CargoAudit));
    assert!(recipes.contains(&DependencyRecipe::CargoPublicApiDiff));

    let none = dependency_policy_recipes_for_paths(&["src/main.rs".to_string()]);
    assert!(none.is_empty());
}

#[test]
fn snapshot_hash_redacts_unstable_fields() {
    let first = snapshot_record(
        "evidence",
        SnapshotPolicy::AutoAcceptWithRedactions,
        json!({
            "run_id": "run-a",
            "summary": "same",
            "nested": {"duration_ms": 12}
        }),
    );
    let second = snapshot_record(
        "evidence",
        SnapshotPolicy::AutoAcceptWithRedactions,
        json!({
            "run_id": "run-b",
            "summary": "same",
            "nested": {"duration_ms": 99}
        }),
    );
    assert_eq!(first.semantic_hash, second.semantic_hash);
    assert!(first
        .redacted_fields
        .iter()
        .any(|field| field == "$.run_id"));
    assert!(first
        .redacted_fields
        .iter()
        .any(|field| field == "$.nested.duration_ms"));
}

#[test]
fn workspace_verification_mode_prefers_temp_patch_for_source_changes() {
    let source = DiffSummary {
        changed_files: vec!["src/main.rs".to_string()],
        insertions: 10,
        deletions: 1,
        touches_source: true,
        touches_tests: false,
        touches_docs: false,
        touches_tracking_docs: false,
        touches_dependency_or_lockfile: false,
        touches_generated: false,
        touches_security_sensitive: false,
    };
    assert_eq!(
        workspace_verification_mode_for_diff(&source),
        WorkspaceVerificationMode::TempWorktreeFromPatch
    );

    let docs = DiffSummary {
        changed_files: vec!["README.md".to_string()],
        touches_source: false,
        touches_tests: false,
        touches_docs: true,
        touches_tracking_docs: false,
        ..source
    };
    assert_eq!(
        workspace_verification_mode_for_diff(&docs),
        WorkspaceVerificationMode::StagedOnly
    );
}

#[test]
fn ranked_repo_map_prioritizes_symbols_and_tests() {
    let index = RepoIndex {
        files: vec![
            FileEntry {
                path: PathBuf::from("src/coordinator.rs"),
                language: Some("rust".to_string()),
            },
            FileEntry {
                path: PathBuf::from("tests/coordinator_tests.rs"),
                language: Some("rust".to_string()),
            },
        ],
        manifests: vec![],
        test_files: vec![PathBuf::from("tests/coordinator_tests.rs")],
        docs: vec![],
        symbols: vec![SymbolEntry {
            path: PathBuf::from("src/coordinator.rs"),
            language: "rust".to_string(),
            kind: "struct".to_string(),
            name: "AppCoordinator".to_string(),
            line: Some(12),
        }],
    };
    let map = rank_repo_map(&index, &["coordinator".to_string()], 128);
    assert_eq!(map.entries[0].path, PathBuf::from("src/coordinator.rs"));
    assert!(map.entries[0]
        .why
        .contains(&RepoMapReason::DefinesReferencedSymbol));
}

#[test]
fn deterministic_lane_only_accepts_tracking_doc_verification_tasks() {
    let mut task = sample_task(vec!["NEXT_STEPS.md".to_string()]);
    task.kind = "verification".to_string();
    task.title = "Validate documented command".to_string();
    task.objective = "Run verification commands and record status.".to_string();
    task.exact_steps = vec!["Run the listed verification commands.".to_string()];
    task.tests_to_run = vec!["git diff --check".to_string()];
    assert!(is_deterministic_task_candidate(&task));

    task.allowed_files = vec!["Sources/lib.rs".to_string()];
    assert!(!is_deterministic_task_candidate(&task));

    task.allowed_files = vec!["NEXT_STEPS.md".to_string()];
    task.exact_steps = vec!["Implement the production gate.".to_string()];
    assert!(!is_deterministic_task_candidate(&task));
}

#[test]
fn deterministic_lane_accepts_docs_only_mechanical_verify_tasks() {
    let mut task = sample_task(vec!["docs/HERMES.md".to_string()]);
    task.kind = "docs_only".to_string();
    task.title = "Validate Hermes docs".to_string();
    task.objective = "Run docs-only verification commands.".to_string();
    task.exact_steps = vec!["Run read-only verification.".to_string()];
    task.tests_to_run = vec!["git diff --check".to_string()];
    assert!(is_deterministic_task_candidate(&task));

    task.tests_to_run = vec!["git status --short".to_string()];
    assert!(is_deterministic_task_candidate(&task));

    task.tests_to_run = vec!["git diff --stat".to_string()];
    assert!(is_deterministic_task_candidate(&task));

    task.allowed_files = vec!["README.md".to_string()];
    task.tests_to_run = vec!["git diff --check".to_string()];
    assert!(is_deterministic_task_candidate(&task));

    task.allowed_files = vec!["docs/HERMES.md".to_string()];
    task.tests_to_run = vec!["cargo test".to_string()];
    assert!(!is_deterministic_task_candidate(&task));

    task.tests_to_run = vec!["git diff --check".to_string()];
    task.exact_steps = vec!["Rewrite the Hermes guide from scratch.".to_string()];
    assert!(!is_deterministic_task_candidate(&task));
}

#[test]
fn command_only_evidence_accepts_docs_verify_envelope() {
    let mut task = sample_task(vec!["docs/HERMES.md".to_string()]);
    task.kind = "verification".to_string();
    task.title = "Capture command-only docs evidence".to_string();
    task.objective = "Run command-only verification evidence for docs.".to_string();
    task.exact_steps = vec!["Run the command-only evidence recipe.".to_string()];
    task.success_criteria = vec!["Evidence receipt is captured.".to_string()];
    task.tests_to_run = vec!["git status --short".to_string()];
    assert!(task_is_command_only_evidence(&task));

    task.allowed_files = vec!["src/app.rs".to_string()];
    assert!(!task_is_command_only_evidence(&task));

    task.allowed_files = vec!["docs/HERMES.md".to_string()];
    task.objective = "Update Hermes docs and verify.".to_string();
    assert!(!task_is_command_only_evidence(&task));
}

#[test]
fn supervisor_error_episodes_feed_tool_health_cooldowns() -> Result<()> {
    let root = temp_root("supervisor-episode-tool-health");
    fs::create_dir_all(root.join(".orchestrator/observations"))?;
    let paths = Paths::new(&root)?;
    append_jsonl(
        &supervisor_episodes_path(&paths),
        &json!({
            "observed_at": "2026-07-14T12:00:00Z",
            "cycle_id": "cyc-1",
            "outcome": "error",
            "error_kind": "provider_unreachable",
            "error_summary": "ollama connection refused",
            "role": "ollama",
            "queue_len": 1,
        }),
    )?;
    let synthesized = synthesize_tool_health_from_supervisor_episodes(&paths);
    assert_eq!(synthesized.len(), 1);
    assert_eq!(synthesized[0].role, "worker");
    assert_eq!(
        synthesized[0].failure_kind,
        Some(ToolFailureKind::ModelUnavailable)
    );
    let cooldowns = current_tool_cooldowns(&paths, &[]);
    assert!(cooldowns.iter().any(|summary| {
        summary.role == "worker" && summary.cooldown_advisory && summary.recent_failures >= 1
    }));
    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn worker_output_requires_and_deserializes_handoff_to_rust() -> Result<()> {
    assert!(WORKER_SCHEMA.contains("\"handoff_to_rust\""));
    assert!(WORKER_SCHEMA.contains("\"orchestration_request\""));
    let output: WorkerOutput = serde_json::from_value(json!({
        "task_id": "TASK-1",
        "status": "done",
        "summary": "done",
        "files_changed": [],
        "commands_run": [],
        "tests_run": [],
        "handoff_to_rust": [{
            "id": "foo-tests",
            "program": "swift",
            "args": ["test", "--filter", "FooTests"],
            "cwd": null,
            "timeout_seconds": null,
            "kind": "test",
            "failure_policy": "stop",
            "expected_exit": null,
            "tags": ["swift", "test"],
            "source": "worker.handoff_to_rust"
        }],
        "blocker": "",
        "confidence": "high"
    }))?;
    assert_eq!(
        output.handoff_to_rust[0].display(),
        "swift test --filter FooTests"
    );
    assert!(serde_json::from_value::<WorkerOutput>(json!({
        "task_id": "TASK-1",
        "status": "done",
        "summary": "done",
        "files_changed": [],
        "commands_run": [],
        "tests_run": [],
        "blocker": "",
        "confidence": "high"
    }))
    .is_err());
    Ok(())
}

#[test]
fn worker_schema_requires_every_strict_object_property() -> Result<()> {
    let schema: serde_json::Value = serde_json::from_str(WORKER_SCHEMA)?;

    assert_schema_object_requires_all_properties(&schema)?;
    assert_schema_object_requires_all_properties(
        schema
            .pointer("/properties/orchestration_request")
            .context("missing orchestration_request schema")?,
    )?;
    assert_schema_object_requires_all_properties(
        schema
            .pointer("/properties/orchestration_request/properties/child_tasks/items")
            .context("missing child task schema")?,
    )?;

    Ok(())
}

fn assert_schema_object_requires_all_properties(schema: &serde_json::Value) -> Result<()> {
    let properties = schema
        .get("properties")
        .and_then(|value| value.as_object())
        .context("schema object must declare properties")?;
    let required = schema
        .get("required")
        .and_then(|value| value.as_array())
        .context("schema object must declare required")?
        .iter()
        .map(|value| value.as_str().context("required entries must be strings"))
        .collect::<Result<BTreeSet<_>>>()?;
    let property_keys = properties
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();

    assert_eq!(required, property_keys);
    Ok(())
}

fn assert_schema_strict_recursively(schema: &serde_json::Value) -> Result<()> {
    if schema.get("properties").is_some() {
        assert_eq!(
            schema
                .get("additionalProperties")
                .and_then(|value| value.as_bool()),
            Some(false),
            "object schemas must set additionalProperties=false"
        );
        assert_schema_object_requires_all_properties(schema)?;
        if let Some(properties) = schema.get("properties").and_then(|value| value.as_object()) {
            for property in properties.values() {
                assert_schema_strict_recursively(property)?;
            }
        }
    }
    if let Some(items) = schema.get("items") {
        assert_schema_strict_recursively(items)?;
    }
    if let Some(one_of) = schema.get("oneOf").and_then(|value| value.as_array()) {
        for variant in one_of {
            assert_schema_strict_recursively(variant)?;
        }
    }
    if let Some(any_of) = schema.get("anyOf").and_then(|value| value.as_array()) {
        for variant in any_of {
            assert_schema_strict_recursively(variant)?;
        }
    }
    Ok(())
}

#[test]
fn role_schemas_require_every_strict_object_property() -> Result<()> {
    for (name, raw) in [
        ("master", MASTER_SCHEMA.to_string()),
        ("planner", render_schema(PLANNER_SCHEMA)),
        ("worker", WORKER_SCHEMA.to_string()),
        ("foreman_review", FOREMAN_REVIEW_SCHEMA.to_string()),
        ("foreman_repair", FOREMAN_REPAIR_SCHEMA.to_string()),
        ("foreman_batch", FOREMAN_BATCH_SCHEMA.to_string()),
        ("task", TASK_SCHEMA_JSON.to_string()),
    ] {
        let schema: serde_json::Value =
            serde_json::from_str(&raw).with_context(|| format!("{name} schema must parse"))?;
        assert_schema_strict_recursively(&schema)
            .with_context(|| format!("{name} schema failed strict property=required invariant"))?;
    }
    Ok(())
}

#[test]
fn worker_output_deserializes_orchestration_request() -> Result<()> {
    let output: WorkerOutput = serde_json::from_value(json!({
        "task_id": "TASK-1",
        "status": "blocked",
        "summary": "too broad",
        "files_changed": [],
        "commands_run": [],
        "tests_run": [],
        "handoff_to_rust": [],
        "orchestration_request": {
            "kind": "split_task",
            "reason": "split into DTO and runtime slices",
            "child_tasks": [{
                "title": "DTO slice",
                "objective": "Add DTO fields only.",
                "allowed_files": ["Sources/Foo.swift"],
                "tests_to_run": ["git diff --check"]
            }]
        },
        "blocker": "task is too broad",
        "confidence": "medium"
    }))?;

    let request = output.orchestration_request.expect("request should parse");
    assert_eq!(request.kind, WorkerOrchestrationKind::SplitTask);
    assert_eq!(request.child_tasks.len(), 1);
    Ok(())
}

#[test]
fn worker_split_orchestration_queues_bounded_child_tasks() -> Result<()> {
    let root = temp_root("worker-split-orchestration");
    fs::create_dir_all(root.join(".orchestrator"))?;
    let paths = Paths::new(&root)?;
    let cfg = Config::default();
    let mut state = State::default();
    let task = sample_task(vec![
        "Sources/Foo.swift".to_string(),
        "Tests/FooTests.swift".to_string(),
    ]);
    let mut worker = sample_worker_output(&task.task_id);
    worker.status = "blocked".to_string();
    worker.summary = "needs split".to_string();
    worker.orchestration_request = Some(WorkerOrchestrationRequest {
        kind: WorkerOrchestrationKind::SplitTask,
        reason: "DTO and tests are separable".to_string(),
        lease_paths: Vec::new(),
        child_tasks: vec![
            WorkerProposedTask {
                title: "DTO slice".to_string(),
                objective: "Update DTO fields.".to_string(),
                kind: None,
                allowed_files: vec!["Sources/Foo.swift".to_string()],
                read_only_files: Vec::new(),
                exact_steps: vec!["Edit DTO only.".to_string()],
                success_criteria: vec!["DTO compiles.".to_string()],
                tests_to_run: vec!["git diff --check".to_string()],
                stop_conditions: Vec::new(),
                estimated_minutes: Some(5),
            },
            WorkerProposedTask {
                title: "Test slice".to_string(),
                objective: "Update tests.".to_string(),
                kind: None,
                allowed_files: vec!["Tests/FooTests.swift".to_string()],
                read_only_files: Vec::new(),
                exact_steps: vec!["Edit tests only.".to_string()],
                success_criteria: vec!["Tests compile.".to_string()],
                tests_to_run: vec!["git diff --check".to_string()],
                stop_conditions: Vec::new(),
                estimated_minutes: Some(5),
            },
        ],
    });
    let guard = GuardReport {
        enabled: true,
        status: "clean".to_string(),
        changed_files: Vec::new(),
        allowed_patterns: task.allowed_files.clone(),
        ignored_patterns: Vec::new(),
        violations: Vec::new(),
    };

    let summary =
        apply_worker_orchestration_request(&paths, &cfg, &mut state, &task, &worker, &guard)?
            .expect("split request should be handled");

    assert!(summary.contains("queued 2 bounded child task"));
    assert_eq!(state.queue.len(), 2);
    assert_eq!(state.queue[0].task_id, "T001-W1");
    assert_eq!(state.queue[1].task_id, "T001-W2");
    assert_eq!(state.queue[1].dependencies, vec!["T001-W1"]);
    assert!(!state.planner_due);
    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn worker_split_orchestration_outside_scope_routes_to_planner() -> Result<()> {
    let root = temp_root("worker-split-orchestration-outside");
    fs::create_dir_all(root.join(".orchestrator"))?;
    let paths = Paths::new(&root)?;
    let cfg = Config::default();
    let mut state = State::default();
    let task = sample_task(vec!["Sources/Foo.swift".to_string()]);
    let mut worker = sample_worker_output(&task.task_id);
    worker.status = "blocked".to_string();
    worker.orchestration_request = Some(WorkerOrchestrationRequest {
        kind: WorkerOrchestrationKind::SplitTask,
        reason: "needs extra file".to_string(),
        lease_paths: Vec::new(),
        child_tasks: vec![WorkerProposedTask {
            title: "Bad slice".to_string(),
            objective: "Edit outside scope.".to_string(),
            kind: None,
            allowed_files: vec!["Sources/Bar.swift".to_string()],
            read_only_files: Vec::new(),
            exact_steps: Vec::new(),
            success_criteria: Vec::new(),
            tests_to_run: vec!["git diff --check".to_string()],
            stop_conditions: Vec::new(),
            estimated_minutes: None,
        }],
    });
    let guard = GuardReport {
        enabled: true,
        status: "clean".to_string(),
        changed_files: Vec::new(),
        allowed_patterns: task.allowed_files.clone(),
        ignored_patterns: Vec::new(),
        violations: Vec::new(),
    };

    let summary =
        apply_worker_orchestration_request(&paths, &cfg, &mut state, &task, &worker, &guard)?
            .expect("invalid split should be handled");

    assert!(summary.contains("Rust rejected the split request"));
    assert!(state.queue.is_empty());
    assert!(state.planner_due);
    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn merged_verification_commands_deduplicate_task_and_worker_handoff() {
    let mut task = sample_task(vec!["Sources/Foo.swift".to_string()]);
    task.tests_to_run = vec![
        "swift test --filter FooTests".to_string(),
        "git diff --check".to_string(),
    ];
    let mut worker = sample_worker_output(&task.task_id);
    let mut duplicate_with_absolute_cwd = vc("swift test --filter FooTests");
    duplicate_with_absolute_cwd.cwd = Some("/tmp/repo".to_string());
    worker.handoff_to_rust = vec![
        duplicate_with_absolute_cwd,
        vc("swift build --product demo"),
    ];

    let (commands, rejected) = merged_verification_commands(&task, &worker);
    assert!(rejected.is_empty());
    assert_eq!(
        commands
            .iter()
            .map(VerificationCommand::display)
            .collect::<Vec<_>>(),
        vec![
            "swift test --filter FooTests",
            "git diff --check",
            "swift build --product demo",
        ]
    );
}

#[test]
fn worker_handoff_absolute_cwd_is_normalized_to_repo_root() {
    let mut task = sample_task(vec!["Tests/FooTests.swift".to_string()]);
    task.tests_to_run = vec!["git diff --check".to_string()];
    let mut worker = sample_worker_output(&task.task_id);
    let mut focused_test = vc("swift test --filter SessionCycleSenderTests");
    focused_test.cwd = Some("/tmp/project/app".to_string());
    worker.handoff_to_rust = vec![focused_test];

    let (commands, rejected) = merged_verification_commands(&task, &worker);

    assert!(rejected.is_empty());
    let displays = commands
        .iter()
        .map(VerificationCommand::display)
        .collect::<Vec<_>>();
    assert!(displays.contains(&"swift test --filter SessionCycleSenderTests".to_string()));
    assert!(commands.iter().all(|command| command.cwd.is_none()));
}

#[test]
fn merged_verification_commands_deduplicate_worker_expected_exit_zero() {
    let mut task = sample_task(vec!["Sources/Foo.swift".to_string()]);
    task.tests_to_run = vec![
        "swift test --filter FooTests".to_string(),
        "swift build --product demo".to_string(),
        "git diff --check".to_string(),
    ];
    let mut worker = sample_worker_output(&task.task_id);
    let mut test = vc("swift test --filter FooTests");
    test.expected_exit = Some(0);
    let mut build = vc("swift build --product demo");
    build.expected_exit = Some(0);
    let mut diff = vc("git diff --check");
    diff.expected_exit = Some(0);
    worker.handoff_to_rust = vec![test, build, diff];

    let (commands, rejected) = merged_verification_commands(&task, &worker);

    assert!(rejected.is_empty());
    assert_eq!(
        commands
            .iter()
            .map(VerificationCommand::display)
            .collect::<Vec<_>>(),
        vec![
            "swift test --filter FooTests",
            "swift build --product demo",
            "git diff --check",
        ]
    );
}

#[test]
fn worker_handoff_allows_swift_version_probe() {
    let task = sample_task(vec!["Sources/Foo.swift".to_string()]);
    let mut worker = sample_worker_output(&task.task_id);
    worker.handoff_to_rust = vec![vc("swift --version")];

    let (commands, rejected) = merged_verification_commands(&task, &worker);

    assert!(rejected.is_empty());
    assert_eq!(commands[0].display(), "swift --version");
}

#[test]
fn rust_verification_runs_safe_handoff_and_reports_rejections() -> Result<()> {
    let root = temp_root("handoff-verification");
    fs::create_dir_all(root.join(".orchestrator"))?;
    write_text(&root.join("README.md"), "hello\n")?;
    let paths = Paths::new(&root)?;
    let cfg = Config::default();
    let mut task = sample_task(vec!["Sources/Foo.swift".to_string()]);
    task.tests_to_run = vec!["rg -n \"hello\" README.md".to_string()];
    let mut worker = sample_worker_output(&task.task_id);
    worker.handoff_to_rust = vec![
        vc("rg -n \"hello\" README.md"),
        VerificationCommand::new(
            ".build/debug/app".to_string(),
            vec!["--public-diagnostic".to_string()],
            "worker.handoff_to_rust",
        ),
    ];

    let report = run_task_verification_deterministically(&paths, &cfg, &task, &worker)?;
    assert_eq!(report["status"], "rejectedCommands");
    assert_eq!(report["commands"].as_array().unwrap().len(), 1);
    assert_eq!(
        report["commands"][0]["command_display"],
        "rg -n hello README.md"
    );
    assert_eq!(report["commands"][0]["category"], "passed");
    assert_eq!(
        report["rejected_commands"][0],
        ".build/debug/app --public-diagnostic"
    );

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn verification_pipeline_persists_evidence_jsonl() -> Result<()> {
    let root = temp_root("evidence-jsonl");
    fs::create_dir_all(root.join(".orchestrator"))?;
    write_text(&root.join("README.md"), "hello evidence\n")?;
    let paths = Paths::new(&root)?;
    let cfg = Config::default();
    let mut task = sample_task(vec!["README.md".to_string()]);
    task.tests_to_run = vec!["rg -n evidence README.md".to_string()];
    let worker = sample_worker_output(&task.task_id);

    let report = run_task_verification_deterministically(&paths, &cfg, &task, &worker)?;

    assert_eq!(report["status"], "passed");
    assert!(report["plan"]["plan_id"]
        .as_str()
        .unwrap()
        .starts_with("T001-"));
    assert_eq!(report["report"]["commands"][0]["category"], "passed");
    assert!(report["evidence_path"]
        .as_str()
        .unwrap()
        .contains(".orchestrator/evidence/"));
    let evidence_text = fs::read_dir(&paths.evidence)?
        .filter_map(|entry| entry.ok())
        .map(|entry| read_text(&entry.path()))
        .collect::<Result<Vec<_>>>()?
        .join("\n");
    assert!(evidence_text.contains("\"task_id\":\"T001\""));
    assert!(evidence_text.contains("\"command_display\":\"rg -n evidence README.md\""));
    assert!(evidence_text.contains("\"event\":\"command\""));
    assert!(paths.evidence.join("T001.jsonl").exists());

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn verification_failure_policy_collects_later_evidence() -> Result<()> {
    let root = temp_root("evidence-collect");
    fs::create_dir_all(root.join(".orchestrator"))?;
    write_text(&root.join("README.md"), "hello\n")?;
    let paths = Paths::new(&root)?;
    let mut first = VerificationCommand::new(
        "rg".to_string(),
        vec!["--definitely-not-a-real-rg-flag".to_string()],
        "test",
    );
    first.failure_policy = FailurePolicy::Collect;
    first.id = "bad-rg".to_string();
    first.kind = CommandKind::Search;
    let mut second = vc("rg -n hello README.md");
    second.id = "good-rg".to_string();
    let plan = VerificationPlan {
        plan_id: "collect-plan".to_string(),
        task_id: "T001".to_string(),
        parent_task: None,
        source: "test".to_string(),
        commands: vec![first, second],
        rejected_commands: Vec::new(),
    };

    let report = execute_verification_plan(&paths, &plan)?;

    assert_eq!(report.status, "commandFailed");
    assert_eq!(report.commands.len(), 2);
    assert_eq!(report.commands[0].id, "bad-rg");
    assert_eq!(report.commands[1].id, "good-rg");

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn rust_auto_accepts_low_risk_verified_docs_task() -> Result<()> {
    let root = temp_root("auto-accept-docs");
    fs::create_dir_all(root.join(".orchestrator"))?;
    write_text(&root.join("README.md"), "before\n")?;
    write_text(
        &root.join("NEXT_STEPS.md"),
        r#"
# NEXT_STEPS

### T001 - Update README
- Task ID: `T001`
- Status: Approved.
- Allowed files:
  - `README.md`
"#,
    )?;
    init_git_repo(&root)?;
    write_text(&root.join("README.md"), "before\nauto accepted\n")?;
    let paths = Paths::new(&root)?;
    let cfg = Config::default();
    let mut task = sample_task(vec!["README.md".to_string()]);
    task.review_tier = ReviewTier::AutoAcceptIfVerified;
    task.automation_level = AutomationLevel::AiWorkerRustReview;
    task.tests_to_run = vec!["rg -n 'auto accepted' README.md".to_string()];
    let worker = sample_worker_output(&task.task_id);
    let before = HashMap::new();
    let after = HashMap::from([(
        "README.md".to_string(),
        fingerprint_file(&root.join("README.md"))?,
    )]);
    let guard = build_guard_report(&cfg, &task, &before, &after)?;
    let verification = run_task_verification_deterministically(&paths, &cfg, &task, &worker)?;
    let report = evidence_report_from_value(&verification).expect("evidence report");

    let outcome = maybe_auto_accept_worker_task(
        &paths,
        &cfg,
        &mut State::default(),
        &task,
        &worker,
        &guard,
        &report,
    )?
    .expect("low-risk docs task should auto-accept");

    assert!(outcome.commit_sha.is_some());
    assert!(read_text(&root.join("NEXT_STEPS.md"))?.contains("- Status: Done."));
    let log = Command::new("git")
        .args(["log", "--oneline", "-1"])
        .current_dir(&root)
        .output()?;
    assert!(String::from_utf8_lossy(&log.stdout).contains("Rust auto-accept"));
    let cost = read_text(&paths.cost)?;
    assert!(cost.contains("\"role\":\"foreman-review\""));
    assert!(cost.contains("\"skipped_by_rust\":true"));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn accepted_worker_commit_excludes_preexisting_allowed_dirty_files() -> Result<()> {
    let root = temp_root("accepted-commit-preexisting-dirty");
    write_text(&root.join("README.md"), "before\n")?;
    write_text(
        &root.join("NEXT_STEPS.md"),
        r#"
# NEXT_STEPS

### T001 - Update README
- Task ID: `T001`
- Status: Approved.
- Allowed files:
  - `README.md`
"#,
    )?;
    init_git_repo(&root)?;
    write_text(&root.join("README.md"), "before\nretry edit\n")?;
    let paths = Paths::new(&root)?;
    let cfg = Config::default();
    let mut task = sample_task(vec!["README.md".to_string()]);
    task.tests_to_run = vec!["rg -n 'retry edit' README.md".to_string()];
    let current = fingerprint_file(&root.join("README.md"))?;
    let before = HashMap::from([("README.md".to_string(), current.clone())]);
    let after = HashMap::from([("README.md".to_string(), current)]);
    let guard = build_guard_report(&cfg, &task, &before, &after)?;
    assert!(guard.changed_files.is_empty());
    let worker = sample_worker_output(&task.task_id);
    let verification = run_task_verification_deterministically(&paths, &cfg, &task, &worker)?;
    let report = evidence_report_from_value(&verification).expect("evidence report");

    let commit_sha = rust_accept_worker_task(
        &paths,
        &cfg,
        &mut State::default(),
        WorkerAcceptance {
            task: &task,
            guard_report: &guard,
            evidence_report: &report,
            reason_code: "accepted_clean",
            commit_message: "Complete T001",
        },
    )?;

    assert!(commit_sha.is_some());
    let show = Command::new("git")
        .args(["show", "--name-only", "--format=", "HEAD"])
        .current_dir(&root)
        .output()?;
    let show = String::from_utf8_lossy(&show.stdout);
    assert!(
        !show.lines().any(|line| line == "README.md"),
        "pre-existing dirty files outside the guard delta must stay unstaged"
    );
    assert!(show.lines().any(|line| line == "NEXT_STEPS.md"));
    let status = Command::new("git")
        .args(["status", "--short", "--", "README.md"])
        .current_dir(&root)
        .output()?;
    assert!(!String::from_utf8_lossy(&status.stdout).trim().is_empty());

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn ceo_control_doc_commit_stages_only_control_docs() -> Result<()> {
    let root = temp_root("ceo-control-doc-commit");
    write_text(&root.join("ROADMAP.md"), "before\n")?;
    write_text(&root.join("README.md"), "before\n")?;
    init_git_repo(&root)?;
    write_text(&root.join("ROADMAP.md"), "before\nceo update\n")?;
    write_text(&root.join("README.md"), "before\nuser scratch\n")?;
    let paths = Paths::new(&root)?;

    let commit_sha = commit_control_doc_edits(
        &paths,
        &Config::default(),
        &mut State::default(),
        "Update CEO roadmap direction",
    )?;

    assert!(commit_sha.is_some());
    let show = Command::new("git")
        .args(["show", "--name-only", "--format=", "HEAD"])
        .current_dir(&root)
        .output()?;
    let show = String::from_utf8_lossy(&show.stdout);
    assert!(show.lines().any(|line| line == "ROADMAP.md"));
    assert!(!show.lines().any(|line| line == "README.md"));
    let status = Command::new("git")
        .args(["status", "--short", "--", "README.md"])
        .current_dir(&root)
        .output()?;
    assert!(String::from_utf8_lossy(&status.stdout).contains("README.md"));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn rust_auto_accepts_verified_docs_packet_even_when_card_requests_review() -> Result<()> {
    let root = temp_root("auto-accept-docs-packet");
    for dir in [".orchestrator", "docs"] {
        fs::create_dir_all(root.join(dir))?;
    }
    write_text(
        &root.join("docs/PRE_LIVE_SECURITY_REVIEW.md"),
        "protected credential\nLoopback\nexternal registry\npublic listing\nservice\nkill switch\nproduction traffic\n",
    )?;
    write_text(&root.join("docs/ROADMAP.md"), "before\n")?;
    write_text(&root.join("CONTINUITY.md"), "before\n")?;
    write_text(&root.join("TASK.md"), "before\n")?;
    write_text(
        &root.join("NEXT_STEPS.md"),
        r#"
# NEXT_STEPS

### T001 - Pre-Live Router Experiment Approval Packet
- Task ID: `T001`
- Status: Approved.
- Review tier: always_ai_review
- Automation level: ai_worker_ai_review
- Allowed files:
  - `docs/PRE_LIVE_SECURITY_REVIEW.md`
  - `docs/ROADMAP.md`
  - `NEXT_STEPS.md`
  - `CONTINUITY.md`
  - `TASK.md`
"#,
    )?;
    init_git_repo(&root)?;
    let paths = Paths::new(&root)?;
    let cfg = Config::default();
    let before = snapshot_repo(&paths, &cfg)?;
    write_text(
            &root.join("docs/PRE_LIVE_SECURITY_REVIEW.md"),
            "protected credential\nLoopback\nmessage dispatch\nexternal registry\npublic listing\nservice\nkill switch\nproduction traffic\nnon-executable approval packet\n",
        )?;
    write_text(&root.join("docs/ROADMAP.md"), "updated docs roadmap\n")?;
    write_text(&root.join("CONTINUITY.md"), "updated continuity\n")?;
    write_text(&root.join("TASK.md"), "updated task\n")?;
    write_text(
        &root.join("NEXT_STEPS.md"),
        r#"
# NEXT_STEPS

### T001 - Pre-Live Router Experiment Approval Packet
- Task ID: `T001`
- Status: Approved.
- Review tier: always_ai_review
- Automation level: ai_worker_ai_review
- Allowed files:
  - `docs/PRE_LIVE_SECURITY_REVIEW.md`
  - `docs/ROADMAP.md`
  - `NEXT_STEPS.md`
  - `CONTINUITY.md`
  - `TASK.md`
"#,
    )?;
    let after = snapshot_repo(&paths, &cfg)?;
    let mut task = sample_task(vec![
        "docs/PRE_LIVE_SECURITY_REVIEW.md".to_string(),
        "docs/ROADMAP.md".to_string(),
        "NEXT_STEPS.md".to_string(),
        "CONTINUITY.md".to_string(),
        "TASK.md".to_string(),
    ]);
    task.title = "Pre-Live Router Experiment Approval Packet".to_string();
    task.kind = "documentation".to_string();
    task.review_tier = ReviewTier::AlwaysAiReview;
    task.automation_level = AutomationLevel::AiWorkerAiReview;
    task.tests_to_run = vec![
            "rg -n \"protected credential|Loopback|message dispatch|external registry|public listing|service|kill switch|production traffic\" docs/PRE_LIVE_SECURITY_REVIEW.md docs/ROADMAP.md NEXT_STEPS.md CONTINUITY.md TASK.md".to_string(),
            "git diff --check".to_string(),
        ];
    let worker = sample_worker_output(&task.task_id);
    let guard = build_guard_report(&cfg, &task, &before, &after)?;
    assert!(guard.violations.is_empty());
    let verification = run_task_verification_deterministically(&paths, &cfg, &task, &worker)?;
    let report = evidence_report_from_value(&verification).expect("evidence report");
    let diff_summary = build_diff_summary(&paths, &guard.changed_files)?;
    assert!(inferred_docs_only_auto_accept_candidate(&task));
    assert_eq!(classify_risk(&task, &diff_summary), RiskLevel::Low);

    let outcome = maybe_auto_accept_worker_task(
        &paths,
        &cfg,
        &mut State::default(),
        &task,
        &worker,
        &guard,
        &report,
    )?
    .expect("verified docs-only packet should auto-accept despite conservative card routing");

    assert!(outcome.commit_sha.is_some());
    assert!(read_text(&root.join("NEXT_STEPS.md"))?.contains("- Status: Done."));
    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn rust_auto_accept_rejects_source_diff_even_when_verified() -> Result<()> {
    let root = temp_root("auto-accept-source");
    fs::create_dir_all(root.join("src"))?;
    write_text(&root.join("src/lib.rs"), "pub fn value() -> u8 { 1 }\n")?;
    let paths = Paths::new(&root)?;
    let cfg = Config::default();
    let mut task = sample_task(vec!["src/lib.rs".to_string()]);
    task.review_tier = ReviewTier::AutoAcceptIfVerified;
    task.automation_level = AutomationLevel::AiWorkerRustReview;
    let worker = sample_worker_output(&task.task_id);
    let guard = GuardReport {
        enabled: true,
        status: "ok".to_string(),
        changed_files: vec!["src/lib.rs".to_string()],
        allowed_patterns: vec!["src/lib.rs".to_string()],
        ignored_patterns: Vec::new(),
        violations: Vec::new(),
    };
    let report = EvidenceReport {
        report_id: "r1".to_string(),
        plan_id: "p1".to_string(),
        task_id: task.task_id.clone(),
        status: "passed".to_string(),
        started_at: Utc::now().to_rfc3339(),
        completed_at: Utc::now().to_rfc3339(),
        summary: "passed".to_string(),
        commands: vec![CommandEvidence {
            id: "c1".to_string(),
            command: vc("rg -n value src/lib.rs"),
            command_display: "rg -n value src/lib.rs".to_string(),
            exit_code: 0,
            success: true,
            category: "passed".to_string(),
            duration_ms: 1,
            stdout_tail: String::new(),
            stderr_tail: String::new(),
            evidence_summary: "exit 0".to_string(),
        }],
        rejected_commands: Vec::new(),
    };

    let outcome = maybe_auto_accept_worker_task(
        &paths,
        &cfg,
        &mut State::default(),
        &task,
        &worker,
        &guard,
        &report,
    )?;

    assert!(outcome.is_none());
    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn rust_auto_accepts_small_verified_source_and_test_slice_when_tier_allows() -> Result<()> {
    let root = temp_root("auto-accept-source-tests");
    fs::create_dir_all(root.join("Sources/App"))?;
    fs::create_dir_all(root.join("Tests/AppTests"))?;
    write_text(&root.join("Sources/App/Foo.swift"), "struct Foo {}\n")?;
    write_text(
        &root.join("Tests/AppTests/FooTests.swift"),
        "final class FooTests {}\n",
    )?;
    write_text(
        &root.join("NEXT_STEPS.md"),
        r#"
# NEXT_STEPS

### T001 - Add Foo Evidence
- Task ID: `T001`
- Status: Approved.
- Review tier: `auto_accept_if_verified`
- Automation level: `ai_worker_rust_review`
- Allowed files:
  - `Sources/App/Foo.swift`
  - `Tests/AppTests/FooTests.swift`
  - tracking docs
"#,
    )?;
    init_git_repo(&root)?;
    let paths = Paths::new(&root)?;
    let cfg = Config::default();
    let before = snapshot_repo(&paths, &cfg)?;
    write_text(
        &root.join("Sources/App/Foo.swift"),
        "struct Foo {\n    let value = 1\n}\n",
    )?;
    write_text(
        &root.join("Tests/AppTests/FooTests.swift"),
        "final class FooTests {\n    func testFoo() {}\n}\n",
    )?;
    write_text(
        &root.join("NEXT_STEPS.md"),
        r#"
# NEXT_STEPS

### T001 - Add Foo Evidence
- Task ID: `T001`
- Status: Approved.
- Review tier: `auto_accept_if_verified`
- Automation level: `ai_worker_rust_review`
- Allowed files:
  - `Sources/App/Foo.swift`
  - `Tests/AppTests/FooTests.swift`
  - tracking docs
"#,
    )?;
    let after = snapshot_repo(&paths, &cfg)?;
    let mut task = sample_task(vec![
        "Sources/App/Foo.swift".to_string(),
        "Tests/AppTests/FooTests.swift".to_string(),
        "tracking docs".to_string(),
    ]);
    task.review_tier = ReviewTier::AutoAcceptIfVerified;
    task.automation_level = AutomationLevel::AiWorkerRustReview;
    task.forbidden_actions = vec![
            "No network access. No public socket behavior. No production traffic. No dependency changes. No migrations."
                .to_string(),
        ];
    let worker = sample_worker_output(&task.task_id);
    let guard = build_guard_report(&cfg, &task, &before, &after)?;
    assert!(guard.violations.is_empty());
    let diff_summary = build_diff_summary(&paths, &guard.changed_files)?;
    assert_eq!(classify_risk(&task, &diff_summary), RiskLevel::Low);
    let report = EvidenceReport {
        report_id: "r-source".to_string(),
        plan_id: "p-source".to_string(),
        task_id: task.task_id.clone(),
        status: "passed".to_string(),
        started_at: Utc::now().to_rfc3339(),
        completed_at: Utc::now().to_rfc3339(),
        summary: "passed".to_string(),
        commands: vec![CommandEvidence {
            id: "c1".to_string(),
            command: vc("rg -n Foo Sources/App/Foo.swift"),
            command_display: "rg -n Foo Sources/App/Foo.swift".to_string(),
            exit_code: 0,
            success: true,
            category: "passed".to_string(),
            duration_ms: 1,
            stdout_tail: String::new(),
            stderr_tail: String::new(),
            evidence_summary: "exit 0".to_string(),
        }],
        rejected_commands: Vec::new(),
    };

    let outcome = maybe_auto_accept_worker_task(
        &paths,
        &cfg,
        &mut State::default(),
        &task,
        &worker,
        &guard,
        &report,
    )?
    .expect("small verified source+test task should auto-accept");

    assert!(outcome.commit_sha.is_some());
    assert!(read_text(&root.join("NEXT_STEPS.md"))?.contains("- Status: Done."));
    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn accepted_worker_commit_expands_ignored_directory_to_dirty_files() -> Result<()> {
    let root = temp_root("accepted-commit-ignored-directory");
    fs::create_dir_all(root.join("Sources/App"))?;
    fs::create_dir_all(root.join("Tests/AppTests"))?;
    write_text(&root.join(".gitignore"), "Tests/\n")?;
    write_text(&root.join("Sources/App/Foo.swift"), "struct Foo {}\n")?;
    write_text(
        &root.join("Tests/AppTests/FooTests.swift"),
        "final class FooTests {}\n",
    )?;
    write_text(
        &root.join("NEXT_STEPS.md"),
        r#"
# NEXT_STEPS

### T001 - Add Foo Evidence
- Task ID: `T001`
- Status: Approved.
- Review tier: `ai_review_on_failure`
- Automation level: `ai_worker_rust_review`
- Allowed files:
  - `Sources/App/Foo.swift`
  - `Tests/AppTests/*.swift`
  - tracking docs
"#,
    )?;
    let init = Command::new("git")
        .arg("init")
        .current_dir(&root)
        .status()?;
    assert!(init.success());
    configure_test_git_identity(&root)?;
    let add_regular = Command::new("git")
        .args([
            "add",
            ".gitignore",
            "Sources/App/Foo.swift",
            "NEXT_STEPS.md",
        ])
        .current_dir(&root)
        .status()?;
    assert!(add_regular.success());
    let add_ignored_test = Command::new("git")
        .args(["add", "-f", "Tests/AppTests/FooTests.swift"])
        .current_dir(&root)
        .status()?;
    assert!(add_ignored_test.success());
    let commit = Command::new("git")
        .args([
            "-c",
            "user.name=Codex Test",
            "-c",
            "user.email=codex@example.invalid",
            "commit",
            "-m",
            "initial",
        ])
        .current_dir(&root)
        .status()?;
    assert!(commit.success());

    write_text(
        &root.join("Sources/App/Foo.swift"),
        "struct Foo {\n    let value = 1\n}\n",
    )?;
    write_text(
        &root.join("Tests/AppTests/FooTests.swift"),
        "final class FooTests {\n    func testFoo() {}\n}\n",
    )?;
    let paths = Paths::new(&root)?;
    let mut task = sample_task(vec![
        "Sources/App/Foo.swift".to_string(),
        "Tests/AppTests/*.swift".to_string(),
        "tracking docs".to_string(),
    ]);
    task.task_id = "T001".to_string();
    let guard = GuardReport {
        enabled: true,
        status: "ok".to_string(),
        changed_files: vec!["Sources/App/Foo.swift".to_string(), "Tests".to_string()],
        allowed_patterns: task.allowed_files.clone(),
        ignored_patterns: Vec::new(),
        violations: Vec::new(),
    };
    let report = EvidenceReport {
        report_id: "r-ignored-dir".to_string(),
        plan_id: "p-ignored-dir".to_string(),
        task_id: task.task_id.clone(),
        status: "passed".to_string(),
        started_at: Utc::now().to_rfc3339(),
        completed_at: Utc::now().to_rfc3339(),
        summary: "passed".to_string(),
        commands: vec![CommandEvidence {
            id: "c1".to_string(),
            command: vc("git diff --check"),
            command_display: "git diff --check".to_string(),
            exit_code: 0,
            success: true,
            category: "passed".to_string(),
            duration_ms: 1,
            stdout_tail: String::new(),
            stderr_tail: String::new(),
            evidence_summary: "exit 0".to_string(),
        }],
        rejected_commands: Vec::new(),
    };

    let outcome = rust_accept_worker_task(
        &paths,
        &Config::default(),
        &mut State::default(),
        WorkerAcceptance {
            task: &task,
            guard_report: &guard,
            evidence_report: &report,
            reason_code: "senior_dev_accepted",
            commit_message: "Complete T001",
        },
    )?;

    assert!(outcome.is_some());
    let committed_files = Command::new("git")
        .args(["diff-tree", "--no-commit-id", "--name-only", "-r", "HEAD"])
        .current_dir(&root)
        .output()?;
    assert!(committed_files.status.success());
    let committed_files = String::from_utf8_lossy(&committed_files.stdout);
    assert!(committed_files.contains("Sources/App/Foo.swift"));
    assert!(committed_files.contains("Tests/AppTests/FooTests.swift"));
    assert!(committed_files.contains("NEXT_STEPS.md"));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn rust_auto_accepts_legacy_machine_checkable_source_and_test_slice() -> Result<()> {
    let root = temp_root("auto-accept-legacy-source-tests");
    fs::create_dir_all(root.join("Sources/App"))?;
    fs::create_dir_all(root.join("Tests/AppTests"))?;
    write_text(&root.join("Sources/App/Foo.swift"), "struct Foo {}\n")?;
    write_text(
        &root.join("Tests/AppTests/FooTests.swift"),
        "final class FooTests {}\n",
    )?;
    write_text(
        &root.join("NEXT_STEPS.md"),
        r#"
# NEXT_STEPS

### T001 - Local Foo Skeleton
- Task ID: `T001`
- Status: Approved.
- Allowed files:
  - `Sources/App/Foo.swift`
  - `Tests/AppTests/FooTests.swift`
  - tracking docs
"#,
    )?;
    init_git_repo(&root)?;
    let paths = Paths::new(&root)?;
    let cfg = Config::default();
    let before = snapshot_repo(&paths, &cfg)?;
    write_text(
        &root.join("Sources/App/Foo.swift"),
        "struct Foo {\n    let value = 1\n}\n",
    )?;
    write_text(
        &root.join("Tests/AppTests/FooTests.swift"),
        "final class FooTests {\n    func testFoo() {}\n}\n",
    )?;
    let after = snapshot_repo(&paths, &cfg)?;
    let mut task = sample_task(vec![
        "Sources/App/Foo.swift".to_string(),
        "Tests/AppTests/FooTests.swift".to_string(),
        "tracking docs".to_string(),
    ]);
    task.title = "Local Foo Skeleton".to_string();
    task.objective =
        "Add a local-only skeleton while production traffic remains disabled.".to_string();
    task.tests_to_run = vec![
        "swift test --filter FooTests".to_string(),
        "git diff --check".to_string(),
    ];
    task.forbidden_actions = vec![
            "No network access. No public socket behavior. No production traffic. No dependency changes. No migrations."
                .to_string(),
        ];
    let worker = sample_worker_output(&task.task_id);
    let guard = build_guard_report(&cfg, &task, &before, &after)?;
    assert!(guard.violations.is_empty());
    let diff_summary = build_diff_summary(&paths, &guard.changed_files)?;
    assert!(inferred_machine_checkable_auto_accept_candidate(&task));
    assert_eq!(classify_risk(&task, &diff_summary), RiskLevel::Low);
    let report = EvidenceReport {
        report_id: "r-legacy-source".to_string(),
        plan_id: "p-legacy-source".to_string(),
        task_id: task.task_id.clone(),
        status: "passed".to_string(),
        started_at: Utc::now().to_rfc3339(),
        completed_at: Utc::now().to_rfc3339(),
        summary: "passed".to_string(),
        commands: vec![CommandEvidence {
            id: "c1".to_string(),
            command: vc("swift test --filter FooTests"),
            command_display: "swift test --filter FooTests".to_string(),
            exit_code: 0,
            success: true,
            category: "passed".to_string(),
            duration_ms: 1,
            stdout_tail: String::new(),
            stderr_tail: String::new(),
            evidence_summary: "exit 0".to_string(),
        }],
        rejected_commands: Vec::new(),
    };

    let outcome = maybe_auto_accept_worker_task(
        &paths,
        &cfg,
        &mut State::default(),
        &task,
        &worker,
        &guard,
        &report,
    )?
    .expect("legacy machine-checkable source+test task should auto-accept");

    assert!(outcome.commit_sha.is_some());
    assert!(read_text(&root.join("NEXT_STEPS.md"))?.contains("- Status: Done."));
    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn rust_auto_accepts_conservative_disabled_metadata_slice_when_verified() -> Result<()> {
    let root = temp_root("auto-accept-disabled-metadata");
    for dir in ["Sources/App", "Tests/AppTests", "docs"] {
        fs::create_dir_all(root.join(dir))?;
    }
    for file in [
        "Sources/App/ProductionKillSwitch.swift",
        "Sources/App/AppAdminSnapshot.swift",
        "Sources/App/AppCoordinator.swift",
        "Tests/AppTests/ProductionKillSwitchTests.swift",
        "Tests/AppTests/AppCoordinatorTests.swift",
        "NEXT_STEPS.md",
        "CONTINUITY.md",
        "TASK.md",
        "docs/ROADMAP.md",
    ] {
        write_text(&root.join(file), "baseline\n")?;
    }
    write_text(
        &root.join("NEXT_STEPS.md"),
        r#"
# NEXT_STEPS

### T001 - Production Traffic Kill Switch And Rate Limit Snapshot
- Task ID: `T001`
- Status: Approved.
- Review tier: always_ai_review
- Automation level: ai_worker_ai_review
- Allowed files:
  - `Sources/App/ProductionKillSwitch.swift`
  - `Sources/App/AppAdminSnapshot.swift`
  - `Sources/App/AppCoordinator.swift`
  - `Tests/AppTests/ProductionKillSwitchTests.swift`
  - `Tests/AppTests/AppCoordinatorTests.swift`
  - tracking docs
"#,
    )?;
    init_git_repo(&root)?;
    let paths = Paths::new(&root)?;
    let cfg = Config::default();
    let before = snapshot_repo(&paths, &cfg)?;
    for file in [
        "Sources/App/ProductionKillSwitch.swift",
        "Sources/App/AppAdminSnapshot.swift",
        "Sources/App/AppCoordinator.swift",
        "Tests/AppTests/ProductionKillSwitchTests.swift",
        "Tests/AppTests/AppCoordinatorTests.swift",
        "NEXT_STEPS.md",
        "CONTINUITY.md",
        "TASK.md",
        "docs/ROADMAP.md",
    ] {
        let mut text = String::from("baseline\n");
        for idx in 0..80 {
            text.push_str(&format!("// bounded sanitized metadata line {idx}\n"));
        }
        if file == "NEXT_STEPS.md" {
            text = r#"
# NEXT_STEPS

### T001 - Production Traffic Kill Switch And Rate Limit Snapshot
- Task ID: `T001`
- Status: Approved.
- Review tier: always_ai_review
- Automation level: ai_worker_ai_review
- Allowed files:
  - `Sources/App/ProductionKillSwitch.swift`
  - `Sources/App/AppAdminSnapshot.swift`
  - `Sources/App/AppCoordinator.swift`
  - `Tests/AppTests/ProductionKillSwitchTests.swift`
  - `Tests/AppTests/AppCoordinatorTests.swift`
  - tracking docs
"#
            .to_string();
            for idx in 0..80 {
                text.push_str(&format!("<!-- bounded sanitized metadata line {idx} -->\n"));
            }
        }
        write_text(&root.join(file), &text)?;
    }
    let after = snapshot_repo(&paths, &cfg)?;
    let mut task = sample_task(vec![
        "Sources/App/ProductionKillSwitch.swift".to_string(),
        "Sources/App/AppAdminSnapshot.swift".to_string(),
        "Sources/App/AppCoordinator.swift".to_string(),
        "Tests/AppTests/ProductionKillSwitchTests.swift".to_string(),
        "Tests/AppTests/AppCoordinatorTests.swift".to_string(),
        "tracking docs".to_string(),
    ]);
    task.title = "Production Traffic Kill Switch And Rate Limit Snapshot".to_string();
    task.objective = "Add sanitized disabled-by-default kill switch, queue budget, and rate-limit snapshots required before any public routing experiment.".to_string();
    task.review_tier = ReviewTier::AlwaysAiReview;
    task.automation_level = AutomationLevel::AiWorkerAiReview;
    task.tests_to_run = vec![
        "swift test --filter ProductionKillSwitchTests".to_string(),
        "swift test --filter AppCoordinatorTests".to_string(),
        "git diff --check".to_string(),
    ];
    task.forbidden_actions = vec![
            "Do not enable routing. No public sockets, services, client traffic, production traffic, live external registry publish, live public listing publish, or live message send. Do not add public diagnostics."
                .to_string(),
        ];
    let worker = sample_worker_output(&task.task_id);
    let guard = build_guard_report(&cfg, &task, &before, &after)?;
    assert!(guard.violations.is_empty());
    let diff_summary = build_diff_summary(&paths, &guard.changed_files)?;
    assert!(inferred_machine_checkable_auto_accept_candidate(&task));
    assert_eq!(classify_risk(&task, &diff_summary), RiskLevel::Low);
    let report = EvidenceReport {
        report_id: "r-disabled-metadata".to_string(),
        plan_id: "p-disabled-metadata".to_string(),
        task_id: task.task_id.clone(),
        status: "passed".to_string(),
        started_at: Utc::now().to_rfc3339(),
        completed_at: Utc::now().to_rfc3339(),
        summary: "passed".to_string(),
        commands: vec![CommandEvidence {
            id: "c1".to_string(),
            command: vc("swift test --filter ProductionKillSwitchTests"),
            command_display: "swift test --filter ProductionKillSwitchTests".to_string(),
            exit_code: 0,
            success: true,
            category: "passed".to_string(),
            duration_ms: 1,
            stdout_tail: String::new(),
            stderr_tail: String::new(),
            evidence_summary: "exit 0".to_string(),
        }],
        rejected_commands: Vec::new(),
    };

    let outcome = maybe_auto_accept_worker_task(
        &paths,
        &cfg,
        &mut State::default(),
        &task,
        &worker,
        &guard,
        &report,
    )?
    .expect(
        "verified disabled metadata slice should auto-accept despite conservative card routing",
    );

    assert!(outcome.commit_sha.is_some());
    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn rust_auto_accepts_controlled_loopback_send_adapter_shape_when_verified() -> Result<()> {
    let root = temp_root("auto-accept-loopback-send-adapter");
    for dir in ["Sources/App", "Tests/AppTests", "docs"] {
        fs::create_dir_all(root.join(dir))?;
    }
    for file in [
        "Sources/App/LoopbackSendAdapter.swift",
        "Sources/App/AppAdminSnapshot.swift",
        "Sources/App/AppCoordinator.swift",
        "Tests/AppTests/LoopbackSendAdapterTests.swift",
        "Tests/AppTests/AppCoordinatorTests.swift",
        "Tests/AppTests/ProductionSafetyFlagRegressionTests.swift",
        "NEXT_STEPS.md",
        "CONTINUITY.md",
        "TASK.md",
        "docs/ROADMAP.md",
    ] {
        write_text(&root.join(file), "baseline\n")?;
    }
    write_text(
        &root.join("NEXT_STEPS.md"),
        r#"
# NEXT_STEPS

### T001 - Controlled Loopback Send Adapter
- Task ID: `T001`
- Status: Approved.
- Allowed files:
  - `Sources/App/LoopbackSendAdapter.swift`
  - `Sources/App/AppAdminSnapshot.swift`
  - `Sources/App/AppCoordinator.swift`
  - `Tests/AppTests/LoopbackSendAdapterTests.swift`
  - `Tests/AppTests/AppCoordinatorTests.swift`
  - `Tests/AppTests/ProductionSafetyFlagRegressionTests.swift`
  - tracking docs
"#,
    )?;
    init_git_repo(&root)?;
    let paths = Paths::new(&root)?;
    let cfg = Config::default();
    let before = snapshot_repo(&paths, &cfg)?;
    for file in [
        "Sources/App/LoopbackSendAdapter.swift",
        "Sources/App/AppAdminSnapshot.swift",
        "Sources/App/AppCoordinator.swift",
        "Tests/AppTests/LoopbackSendAdapterTests.swift",
        "Tests/AppTests/AppCoordinatorTests.swift",
        "Tests/AppTests/ProductionSafetyFlagRegressionTests.swift",
        "NEXT_STEPS.md",
        "CONTINUITY.md",
        "TASK.md",
        "docs/ROADMAP.md",
    ] {
        let mut text = String::from("baseline\n");
        for idx in 0..50 {
            text.push_str(&format!(
                "// sanitized loopback handoff evidence line {idx}\n"
            ));
        }
        if file == "NEXT_STEPS.md" {
            text = r#"
# NEXT_STEPS

### T001 - Controlled Loopback Send Adapter
- Task ID: `T001`
- Status: Approved.
- Allowed files:
  - `Sources/App/LoopbackSendAdapter.swift`
  - `Sources/App/AppAdminSnapshot.swift`
  - `Sources/App/AppCoordinator.swift`
  - `Tests/AppTests/LoopbackSendAdapterTests.swift`
  - `Tests/AppTests/AppCoordinatorTests.swift`
  - `Tests/AppTests/ProductionSafetyFlagRegressionTests.swift`
  - tracking docs
"#
            .to_string();
            for idx in 0..50 {
                text.push_str(&format!(
                    "<!-- sanitized loopback handoff evidence line {idx} -->\n"
                ));
            }
        }
        write_text(&root.join(file), &text)?;
    }
    let after = snapshot_repo(&paths, &cfg)?;
    let mut task = sample_task(vec![
        "Sources/App/LoopbackSendAdapter.swift".to_string(),
        "Sources/App/AppAdminSnapshot.swift".to_string(),
        "Sources/App/AppCoordinator.swift".to_string(),
        "Tests/AppTests/LoopbackSendAdapterTests.swift".to_string(),
        "Tests/AppTests/AppCoordinatorTests.swift".to_string(),
        "Tests/AppTests/ProductionSafetyFlagRegressionTests.swift".to_string(),
        "tracking docs".to_string(),
    ]);
    task.title = "Controlled Loopback Send Adapter".to_string();
    task.objective = "Add a local-only controlled Loopback loopback send adapter that records sanitized sentLocally/rejected handoff evidence.".to_string();
    task.tests_to_run = vec![
        "swift test --filter LoopbackSendAdapterTests".to_string(),
        "swift test --filter AppCoordinatorTests".to_string(),
        "swift test --filter ProductionSafetyFlagRegressionTests".to_string(),
        "swift build --product app-admin".to_string(),
        "git diff --check".to_string(),
    ];
    task.forbidden_actions = vec![
            "Do not introduce production send paths, public socket behavior, service exposure, production traffic, live external registry publish, live public listing publish, or live message send."
                .to_string(),
        ];
    let worker = sample_worker_output(&task.task_id);
    let guard = build_guard_report(&cfg, &task, &before, &after)?;
    assert!(guard.violations.is_empty());
    let diff_summary = build_diff_summary(&paths, &guard.changed_files)?;
    assert!(inferred_machine_checkable_auto_accept_candidate(&task));
    assert_eq!(classify_risk(&task, &diff_summary), RiskLevel::Low);
    let report = EvidenceReport {
        report_id: "r-loopback-adapter".to_string(),
        plan_id: "p-loopback-adapter".to_string(),
        task_id: task.task_id.clone(),
        status: "passed".to_string(),
        started_at: Utc::now().to_rfc3339(),
        completed_at: Utc::now().to_rfc3339(),
        summary: "passed".to_string(),
        commands: vec![CommandEvidence {
            id: "c1".to_string(),
            command: vc("swift test --filter LoopbackSendAdapterTests"),
            command_display: "swift test --filter LoopbackSendAdapterTests".to_string(),
            exit_code: 0,
            success: true,
            category: "passed".to_string(),
            duration_ms: 1,
            stdout_tail: String::new(),
            stderr_tail: String::new(),
            evidence_summary: "exit 0".to_string(),
        }],
        rejected_commands: Vec::new(),
    };

    let outcome = maybe_auto_accept_worker_task(
        &paths,
        &cfg,
        &mut State::default(),
        &task,
        &worker,
        &guard,
        &report,
    )?
    .expect("verified controlled loopback adapter shape should auto-accept");

    assert!(outcome.commit_sha.is_some());
    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn markdown_task_parser_reads_review_routing_fields() {
    let tasks = parse_approved_next_steps_tasks(
        r#"
# NEXT_STEPS

- Approved Worker task queue: `TASK-001`

### TASK-001 - Routed Task
- Task ID: `TASK-001`
- Status: Approved.
- Review tier: `auto_accept_if_verified`
- Automation level: `ai_worker_rust_review`
- Allowed files:
  - `README.md`
"#,
    );
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].review_tier, ReviewTier::AutoAcceptIfVerified);
    assert_eq!(
        tasks[0].automation_level,
        AutomationLevel::AiWorkerRustReview
    );
}

#[test]
fn automatic_recipe_harvest_promotes_repeated_successful_command_chains() -> Result<()> {
    let root = temp_root("recipe-harvest");
    let paths = Paths::new(&root)?;
    fs::create_dir_all(&paths.runs)?;
    fs::create_dir_all(&paths.recipes)?;
    let log_one = paths.runs.join("20260428T000001Z-worker.stdout.jsonl");
    let log_two = paths
        .runs
        .join("20260428T000002Z-foreman-review.stdout.jsonl");
    let log_body = format!(
        "{}\n{}\n{}\n{}\n",
        command_completed_json("swift test --filter FooTests", 0),
        command_completed_json("rg -n \"Foo\" Sources", 0),
        command_completed_json(".build/debug/app --local-smoke", 0),
        command_completed_json("swift test --filter BrokenTests", 1),
    );
    write_text(&log_one, &log_body)?;
    write_text(&log_two, &log_body)?;

    let mut cfg = Config::default();
    cfg.recipe_harvest_recent_logs = 10;
    let mut state = State::default();
    let harvested = automatic_recipe_harvest(&paths, &cfg, &mut state)?;
    let summaries = deterministic_recipe_summaries(&paths, 5);

    assert_eq!(harvested, 2);
    assert_eq!(summaries.len(), 1);
    assert!(summaries[0].contains("[trusted; seen 2x]"));
    assert!(summaries[0].contains("swift test --filter FooTests"));
    assert!(summaries[0].contains(".build/debug/app --local-smoke"));
    assert!(!summaries[0].contains("rg -n"));
    assert!(!summaries[0].contains("BrokenTests"));
    assert!(read_text(&paths.recipes.join("README.md"))?.contains("trusted"));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn repo_observer_records_dirty_files_affinity_and_hints() -> Result<()> {
    let root = temp_root("repo-observer");
    fs::create_dir_all(root.join("Sources/App"))?;
    fs::create_dir_all(root.join("Tests/AppTests"))?;
    fs::create_dir_all(root.join(".orchestrator"))?;
    write_text(&root.join("Sources/App/AppCoordinator.swift"), "old\n")?;
    write_text(
        &root.join("Tests/AppTests/AppCoordinatorTests.swift"),
        "old\n",
    )?;
    Command::new("git")
        .arg("init")
        .current_dir(&root)
        .output()?;
    Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(&root)
        .output()?;
    Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(&root)
        .output()?;
    Command::new("git")
        .args(["add", "."])
        .current_dir(&root)
        .output()?;
    Command::new("git")
        .args(["commit", "-m", "initial"])
        .current_dir(&root)
        .output()?;
    write_text(&root.join("Sources/App/AppCoordinator.swift"), "new\n")?;
    write_text(
        &root.join("Tests/AppTests/AppCoordinatorTests.swift"),
        "new\n",
    )?;
    write_text(&root.join(".orchestrator/internal.tmp"), "ignored\n")?;

    let paths = Paths::new(&root)?;
    let mut state = State::default();
    observe_repo_change_state(&paths, &mut state)?;
    observe_repo_change_state(&paths, &mut state)?;

    let journal = read_text(&paths.observations.join("change_journal.jsonl"))?;
    assert_eq!(journal.lines().count(), 1);
    assert!(journal.contains("Sources/App/AppCoordinator.swift"));
    assert!(!journal.contains(".orchestrator/internal.tmp"));
    assert!(journal.contains("swift test --filter AppCoordinatorTests"));

    let affinity = read_text(&paths.observations.join("file_affinity.json"))?;
    assert!(affinity
        .contains("Sources/App/AppCoordinator.swift || Tests/AppTests/AppCoordinatorTests.swift"));
    let affinity_index: FileAffinityIndex = serde_json::from_str(&affinity)?;
    assert!(affinity_index.source_test_pairs.contains_key(
        "Sources/App/AppCoordinator.swift || Tests/AppTests/AppCoordinatorTests.swift"
    ));
    let hints = read_text(&paths.observations.join("verification_hints.json"))?;
    assert!(hints.contains("swift test --filter AppCoordinatorTests"));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn git_history_parser_extracts_commits_files_and_hints() {
    let text = "\
__CODEX_COMMIT__abc123\tWire AppCoordinator loopback
M\tSources/App/AppCoordinator.swift
M\tTests/AppTests/AppCoordinatorTests.swift

__CODEX_COMMIT__def456\tDocs only
M\tROADMAP.md
R100\tOLD.md\tNEXT_STEPS.md
";

    let observations = parse_git_history_observations(text);

    assert_eq!(observations.len(), 2);
    assert_eq!(observations[0].commit, "abc123");
    assert_eq!(observations[0].subject, "Wire AppCoordinator loopback");
    assert!(observations[0]
        .changed_files
        .iter()
        .any(|file| file.path == "Sources/App/AppCoordinator.swift"));
    assert!(observations[0]
        .verification_hints
        .contains(&"swift test --filter AppCoordinatorTests".to_string()));
    assert!(observations[1]
        .changed_files
        .iter()
        .any(|file| file.path == "NEXT_STEPS.md" && file.status == "R100"));
}

#[test]
fn file_affinity_splits_tracking_docs_from_source_test_signal() -> Result<()> {
    let root = temp_root("affinity-splits-tracking-docs");
    let paths = Paths::new(&root)?;
    fs::create_dir_all(&paths.observations)?;
    let changed = vec![
        ObservedChangedFile {
            path: "NEXT_STEPS.md".to_string(),
            status: "M".to_string(),
            category: "tracking_doc".to_string(),
        },
        ObservedChangedFile {
            path: "Sources/App/AppCoordinator.swift".to_string(),
            status: "M".to_string(),
            category: "source".to_string(),
        },
        ObservedChangedFile {
            path: "Tests/AppTests/AppCoordinatorTests.swift".to_string(),
            status: "M".to_string(),
            category: "test".to_string(),
        },
    ];

    update_file_affinity_index(&paths, &changed)?;

    let index: FileAffinityIndex =
        serde_json::from_str(&read_text(&paths.observations.join("file_affinity.json"))?)?;
    let source_test =
        "Sources/App/AppCoordinator.swift || Tests/AppTests/AppCoordinatorTests.swift";
    let tracking_source = "NEXT_STEPS.md || Sources/App/AppCoordinator.swift";
    assert_eq!(index.source_test_pairs.get(source_test), Some(&1));
    assert_eq!(index.non_tracking_pairs.get(source_test), Some(&1));
    assert_eq!(index.tracking_doc_pairs.get(tracking_source), Some(&1));
    assert!(!index.source_test_pairs.contains_key(tracking_source));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn file_affinity_migrates_legacy_flat_pairs_into_split_buckets() {
    let mut index = FileAffinityIndex {
        pairs: BTreeMap::from([
            (
                "NEXT_STEPS.md || Sources/App/AppCoordinator.swift".to_string(),
                7,
            ),
            (
                "Sources/App/AppCoordinator.swift || Tests/AppTests/AppCoordinatorTests.swift"
                    .to_string(),
                5,
            ),
        ]),
        ..FileAffinityIndex::default()
    };

    normalize_file_affinity_index(&mut index);

    assert_eq!(
        index
            .source_test_pairs
            .get("Sources/App/AppCoordinator.swift || Tests/AppTests/AppCoordinatorTests.swift"),
        Some(&5)
    );
    assert_eq!(
        index
            .tracking_doc_pairs
            .get("NEXT_STEPS.md || Sources/App/AppCoordinator.swift"),
        Some(&7)
    );
    assert_eq!(index.all_pairs.len(), 2);
}

#[test]
fn codex_session_import_mines_commands_failures_and_patch_files() -> Result<()> {
    let root = temp_root("codex-session-import");
    let paths = Paths::new(&root)?;
    fs::create_dir_all(&paths.observations)?;
    let export = root.join("session.jsonl");
    let rows = vec![
        json!({
            "timestamp": "2026-04-26T00:00:00Z",
            "type": "session_meta",
            "payload": {
                "id": "session-1",
                "cwd": "/tmp/project"
            }
        }),
        json!({
            "timestamp": "2026-04-26T00:00:01Z",
            "type": "turn_context",
            "payload": {
                "cwd": "/tmp/project",
                "model": "gpt-5.5",
                "effort": "high"
            }
        }),
        json!({
            "timestamp": "2026-04-26T00:00:02Z",
            "type": "event_msg",
            "payload": {
                "type": "exec_command_end",
                "command": ["/bin/zsh", "-lc", "swift test --filter AppCoordinatorTests"],
                "cwd": "/tmp/project",
                "exit_code": 1,
                "status": "failed",
                "stdout": "",
                "stderr": "",
                "aggregated_output": "Tests/AppTests/AppCoordinatorTests.swift:10: error: XCTAssertEqual failed"
            }
        }),
        json!({
            "timestamp": "2026-04-26T00:00:03Z",
            "type": "event_msg",
            "payload": {
                "type": "exec_command_end",
                "command": ["/bin/zsh", "-lc", "git status --short"],
                "cwd": "/tmp/project",
                "exit_code": 0,
                "status": "completed",
                "aggregated_output": ""
            }
        }),
        json!({
            "timestamp": "2026-04-26T00:00:04Z",
            "type": "event_msg",
            "payload": {
                "type": "exec_command_end",
                "command": ["/bin/zsh", "-lc", "sed -n '1,220p' Sources/App/AppCoordinator.swift"],
                "cwd": "/tmp/project",
                "exit_code": 0,
                "status": "completed",
                "aggregated_output": ""
            }
        }),
        json!({
            "timestamp": "2026-04-26T00:00:05Z",
            "type": "event_msg",
            "payload": {
                "type": "exec_command_end",
                "command": ["/bin/zsh", "-lc", "SWIFT_appD_RESEED_CERTS=/tmp/certs swift test --filter ProductionSafetyFlagRegressionTests"],
                "cwd": "/tmp/project",
                "exit_code": 0,
                "status": "completed",
                "aggregated_output": ""
            }
        }),
        json!({
            "timestamp": "2026-04-26T00:00:06Z",
            "type": "event_msg",
            "payload": {
                "type": "exec_command_end",
                "command": ["/bin/zsh", "-lc", "cd /tmp/project && swift build --product app-admin"],
                "cwd": "/tmp/project",
                "exit_code": 0,
                "status": "completed",
                "aggregated_output": ""
            }
        }),
        json!({
            "timestamp": "2026-04-26T00:00:07Z",
            "type": "event_msg",
            "payload": {
                "type": "exec_command_end",
                "command": ["/bin/zsh", "-lc", "git add NEXT_STEPS.md"],
                "cwd": "/tmp/project",
                "exit_code": 0,
                "status": "completed",
                "aggregated_output": ""
            }
        }),
        json!({
            "timestamp": "2026-04-26T00:00:08Z",
            "type": "event_msg",
            "payload": {
                "type": "patch_apply_end",
                "success": true,
                "changes": {
                    "/tmp/project/Sources/App/AppCoordinator.swift": {
                        "type": "update"
                    }
                }
            }
        }),
    ];
    write_text(
        &export,
        &rows
            .into_iter()
            .map(|row| row.to_string())
            .collect::<Vec<_>>()
            .join("\n"),
    )?;

    let mined = observe_codex_session_exports(&paths, &export)?;

    assert_eq!(mined, 1);
    let journal = read_text(&paths.observations.join("codex_sessions_journal.jsonl"))?;
    assert!(journal.contains("swift test --filter AppCoordinatorTests"));
    assert!(journal.contains("test_assertion_failure"));
    assert!(journal.contains("AppCoordinator.swift"));
    let index: CodexSessionIndex = serde_json::from_str(&read_text(
        &paths.observations.join("codex_sessions_index.json"),
    )?)?;
    assert_eq!(index.session_count, 1);
    assert_eq!(
        index
            .command_patterns
            .get("swift test --filter AppCoordinatorTests"),
        Some(&1)
    );
    assert_eq!(
        index
            .verification_command_patterns
            .get("swift test --filter AppCoordinatorTests"),
        Some(&1)
    );
    assert_eq!(
        index
            .verification_command_patterns
            .get("swift test --filter ProductionSafetyFlagRegressionTests"),
        Some(&1)
    );
    assert_eq!(
        index
            .verification_command_patterns
            .get("swift build --product app-admin"),
        Some(&1)
    );
    assert_eq!(
        index.status_command_patterns.get("git status --short"),
        Some(&1)
    );
    assert_eq!(
        index.exploration_command_patterns.get("sed -n '1,220p'"),
        Some(&1)
    );
    assert_eq!(
        index
            .git_mutation_command_patterns
            .get("git add NEXT_STEPS.md"),
        Some(&1)
    );
    assert_eq!(
        index.failure_patterns.get("test_assertion_failure"),
        Some(&1)
    );
    assert_eq!(
        index.file_activity.get("Sources/App/AppCoordinator.swift"),
        Some(&1)
    );

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn learn_global_promotes_local_session_and_git_observations() -> Result<()> {
    let root = temp_root("learn-global");
    let paths = Paths::new(&root)?;
    fs::create_dir_all(&paths.observations)?;
    let global_dir = root.join("global-observations");
    let cfg = Config {
        global_observations_dir: global_dir.to_string_lossy().to_string(),
        ..Config::default()
    };
    let session = CodexSessionObservation {
        observed_at: "2026-04-28T00:00:00Z".to_string(),
        source_file: root.join("session.jsonl").to_string_lossy().to_string(),
        session_id: Some("s1".to_string()),
        cwd: Some(root.to_string_lossy().to_string()),
        model: Some("gpt-5.5".to_string()),
        effort: Some("high".to_string()),
        event_count: 3,
        command_count: 1,
        failed_command_count: 0,
        patch_count: 1,
        changed_files: vec!["Sources/App/AppCoordinator.swift".to_string()],
        command_patterns: BTreeMap::from([(
            "swift test --filter AppCoordinatorTests".to_string(),
            1,
        )]),
        verification_command_patterns: BTreeMap::new(),
        exploration_command_patterns: BTreeMap::new(),
        git_mutation_command_patterns: BTreeMap::new(),
        status_command_patterns: BTreeMap::new(),
        other_command_patterns: BTreeMap::new(),
        failure_patterns: BTreeMap::new(),
    };
    append_jsonl(
        &paths.observations.join("codex_sessions_journal.jsonl"),
        &serde_json::to_value(&session)?,
    )?;
    let history = GitHistoryObservation {
        observed_at: "2026-04-28T00:00:00Z".to_string(),
        commit: "abc123".to_string(),
        subject: "Router node".to_string(),
        changed_files: vec![ObservedChangedFile {
            path: "Sources/App/AppCoordinator.swift".to_string(),
            status: "M".to_string(),
            category: "source".to_string(),
        }],
        verification_hints: vec!["swift test --filter AppCoordinatorTests".to_string()],
    };
    append_jsonl(
        &paths.observations.join("git_history_journal.jsonl"),
        &serde_json::to_value(&history)?,
    )?;

    let summary = promote_local_observations_to_global(&paths, &cfg)?;

    assert_eq!(summary.codex_sessions_added, 1);
    assert_eq!(summary.git_commits_added, 1);
    assert!(global_dir.join("codex_sessions_index.json").exists());
    let hints: VerificationHintIndex =
        serde_json::from_str(&read_text(&global_dir.join("verification_hints.json"))?)?;
    assert_eq!(
        hints
            .hints_by_file
            .get("Sources/App/AppCoordinator.swift")
            .expect("global hint"),
        &vec!["swift test --filter AppCoordinatorTests".to_string()]
    );

    let second = promote_local_observations_to_global(&paths, &cfg)?;
    assert_eq!(second.codex_sessions_added, 0);
    assert_eq!(second.git_commits_added, 0);

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn verification_plan_uses_local_hints_before_global_hints() -> Result<()> {
    let root = temp_root("verification-memory-hints");
    let paths = Paths::new(&root)?;
    fs::create_dir_all(&paths.observations)?;
    fs::create_dir_all(paths.events.parent().unwrap())?;
    let global_dir = root.join("global-observations");
    fs::create_dir_all(&global_dir)?;
    let cfg = Config {
        global_observations_dir: global_dir.to_string_lossy().to_string(),
        ..Config::default()
    };
    write_text(
        &paths.observations.join("verification_hints.json"),
        &format!(
            "{}\n",
            serde_json::to_string_pretty(&VerificationHintIndex {
                updated_at: "now".to_string(),
                hints_by_file: BTreeMap::from([(
                    "Sources/App/AppCoordinator.swift".to_string(),
                    vec!["swift test --filter AppCoordinatorTests".to_string()],
                )]),
            })?
        ),
    )?;
    write_text(
        &global_dir.join("verification_hints.json"),
        &format!(
            "{}\n",
            serde_json::to_string_pretty(&VerificationHintIndex {
                updated_at: "now".to_string(),
                hints_by_file: BTreeMap::from([(
                    "Sources/App/AppCoordinator.swift".to_string(),
                    vec![
                        "swift test --filter AppCoordinatorTests".to_string(),
                        "swift test --filter ProductionSafetyFlagRegressionTests".to_string(),
                    ],
                )]),
            })?
        ),
    )?;
    let task = TaskSpec {
        task_id: "TASK-1".to_string(),
        parent_task: None,
        status: "approved".to_string(),
        priority: "normal".to_string(),
        review_tier: ReviewTier::default(),
        automation_level: AutomationLevel::default(),
        dependencies: Vec::new(),
        title: "Router memory".to_string(),
        kind: "implementation".to_string(),
        objective: "Use observation hints".to_string(),
        why_unblocks_progress: "Keeps verification focused".to_string(),
        allowed_files: vec!["Sources/App/AppCoordinator.swift".to_string()],
        read_only_files: Vec::new(),
        forbidden_actions: Vec::new(),
        exact_steps: Vec::new(),
        success_criteria: Vec::new(),
        tests_to_run: vec!["git diff --check".to_string()],
        stop_conditions: Vec::new(),
        discovery_budget: default_discovery_budget(),
        estimated_minutes: 5,
        requires_network: false,
        prefer_worktree: None,
    };
    let worker = WorkerOutput {
        task_id: "TASK-1".to_string(),
        status: "done".to_string(),
        summary: "done".to_string(),
        files_changed: vec!["Sources/App/AppCoordinator.swift".to_string()],
        commands_run: Vec::new(),
        tests_run: Vec::new(),
        handoff_to_rust: Vec::new(),
        orchestration_request: None,
        blocker: String::new(),
        confidence: "medium".to_string(),
    };

    let plan = build_verification_plan_with_observation_hints(&paths, &cfg, &task, &worker)?;
    let displays = plan
        .commands
        .iter()
        .map(VerificationCommand::display)
        .collect::<Vec<_>>();

    assert_eq!(
        displays,
        vec![
            "git diff --check".to_string(),
            "swift test --filter AppCoordinatorTests".to_string(),
            "swift test --filter ProductionSafetyFlagRegressionTests".to_string(),
        ]
    );
    assert!(read_text(&paths.events)?.contains("globalMemoryApplied"));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn verification_plan_skips_observation_hints_for_docs_only_task() -> Result<()> {
    let root = temp_root("verification-memory-hints-docs-only");
    let paths = Paths::new(&root)?;
    fs::create_dir_all(&paths.observations)?;
    write_text(
        &paths.observations.join("verification_hints.json"),
        &format!(
            "{}\n",
            serde_json::to_string_pretty(&VerificationHintIndex {
                updated_at: "now".to_string(),
                hints_by_file: BTreeMap::from([
                    (
                        "TESTING.md".to_string(),
                        vec![
                            "swift build".to_string(),
                            "swift test --filter BenchmarkSuiteTests".to_string(),
                        ],
                    ),
                    (
                        "Package.swift".to_string(),
                        vec!["swift test --filter TiledRendererTests".to_string()],
                    ),
                ]),
            })?
        ),
    )?;
    let mut task = sample_task(vec![
        "TESTING.md".to_string(),
        "CONTINUITY.md".to_string(),
        "NEXT_STEPS.md".to_string(),
        "NEXT_STEPS_non-ai.md".to_string(),
    ]);
    task.task_id = "DOC-001".to_string();
    task.kind = "verification".to_string();
    task.tests_to_run = vec!["git diff --check".to_string()];
    task.read_only_files = vec![
        "Package.swift".to_string(),
        "Benchmarks/README.md".to_string(),
    ];
    let mut worker = sample_worker_output(&task.task_id);
    worker.files_changed = vec!["TESTING.md".to_string()];
    worker.commands_run = vec![
        "swift test".to_string(),
        "swift test --filter BenchmarkSuiteTests".to_string(),
    ];
    worker.tests_run = worker.commands_run.clone();
    worker.handoff_to_rust = vec![vc("git diff --check")];

    let plan =
        build_verification_plan_with_observation_hints(&paths, &Config::default(), &task, &worker)?;
    let displays = plan
        .commands
        .iter()
        .map(VerificationCommand::display)
        .collect::<Vec<_>>();

    assert_eq!(displays, vec!["git diff --check".to_string()]);
    assert_eq!(plan.source, "task.verification+worker.handoff_to_rust");
    assert!(plan.rejected_commands.is_empty());

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn verification_plan_skips_testing_known_failed_observation_hint() -> Result<()> {
    let root = temp_root("verification-memory-hints-known-failed");
    let paths = Paths::new(&root)?;
    fs::create_dir_all(&paths.observations)?;
    write_text(
        &root.join("TESTING.md"),
        r#"# TESTING

| Command | Status | Notes |
|---|---|---|
| `swift test --filter BenchmarkSuiteTests` | Ran, failed (exit 1; signal 10) | SwiftPM matched zero tests and helper failed. |
| `swift build` | Passed | Baseline compile check. |
"#,
    )?;
    write_text(
        &paths.observations.join("verification_hints.json"),
        &format!(
            "{}\n",
            serde_json::to_string_pretty(&VerificationHintIndex {
                updated_at: "now".to_string(),
                hints_by_file: BTreeMap::from([(
                    "Sources/SwiftStackerBenchSupport/BenchmarkDiagnosticsArtifacts.swift"
                        .to_string(),
                    vec![
                        "swift build".to_string(),
                        "swift test --filter BenchmarkSuiteTests".to_string(),
                    ],
                )]),
            })?
        ),
    )?;
    let mut task = sample_task(vec![
        "Sources/SwiftStackerBenchSupport/BenchmarkDiagnosticsArtifacts.swift".to_string(),
        "Tests/SwiftStackerCoreTests/BenchmarkSuiteTests.swift".to_string(),
    ]);
    task.task_id = "PAR-003".to_string();
    task.kind = "implementation".to_string();
    task.tests_to_run = vec![
        "swift build -c release --product SwiftStackerBench".to_string(),
        "git diff --check".to_string(),
    ];
    let mut worker = sample_worker_output(&task.task_id);
    worker.files_changed =
        vec!["Sources/SwiftStackerBenchSupport/BenchmarkDiagnosticsArtifacts.swift".to_string()];

    let plan =
        build_verification_plan_with_observation_hints(&paths, &Config::default(), &task, &worker)?;
    let displays = plan
        .commands
        .iter()
        .map(VerificationCommand::display)
        .collect::<Vec<_>>();

    assert_eq!(
        displays,
        vec![
            "swift build -c release --product SwiftStackerBench".to_string(),
            "git diff --check".to_string(),
            "swift build".to_string(),
        ]
    );
    assert!(plan.rejected_commands.iter().any(|command| command
        .contains("swift test --filter BenchmarkSuiteTests")
        && command.contains("known failing in TESTING.md")));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn verification_plan_skips_recent_failed_observation_hint() -> Result<()> {
    let root = temp_root("verification-memory-hints-stale");
    let paths = Paths::new(&root)?;
    fs::create_dir_all(&paths.observations)?;
    write_text(
        &paths.observations.join("verification_hints.json"),
        &format!(
            "{}\n",
            serde_json::to_string_pretty(&VerificationHintIndex {
                updated_at: "now".to_string(),
                hints_by_file: BTreeMap::from([(
                    "Tests/SwiftStackerCoreTests/TiledRendererTests.swift".to_string(),
                    vec![
                        "swift build".to_string(),
                        "swift test --filter TiledRendererTests".to_string(),
                    ],
                )]),
            })?
        ),
    )?;
    let failed_command =
        vc("swift test --filter TiledRendererTests").with_source("local_memory.verification_hint");
    let failed_display = failed_command.display();
    persist_evidence_report(
        &paths,
        &EvidenceReport {
            report_id: "report-stale-hint".to_string(),
            plan_id: "PAR-003-plan".to_string(),
            task_id: "PAR-003".to_string(),
            status: "commandFailed".to_string(),
            started_at: "2026-06-29T18:00:00Z".to_string(),
            completed_at: "2026-06-29T18:01:00Z".to_string(),
            summary: "failed stale hint".to_string(),
            commands: vec![CommandEvidence {
                id: "hint-1".to_string(),
                command: failed_command,
                command_display: failed_display,
                exit_code: 1,
                success: false,
                category: "commandFailed".to_string(),
                duration_ms: 32_000,
                stdout_tail: "Test run failed.".to_string(),
                stderr_tail: String::new(),
                evidence_summary: "command exited 1".to_string(),
            }],
            rejected_commands: Vec::new(),
        },
    )?;
    let mut task = sample_task(vec![
        "Tests/SwiftStackerCoreTests/TiledRendererTests.swift".to_string()
    ]);
    task.task_id = "PAR-004".to_string();
    task.tests_to_run = vec!["git diff --check".to_string()];
    let mut worker = sample_worker_output(&task.task_id);
    worker.files_changed = vec!["Tests/SwiftStackerCoreTests/TiledRendererTests.swift".to_string()];

    let plan =
        build_verification_plan_with_observation_hints(&paths, &Config::default(), &task, &worker)?;
    let displays = plan
        .commands
        .iter()
        .map(VerificationCommand::display)
        .collect::<Vec<_>>();

    assert_eq!(
        displays,
        vec!["git diff --check".to_string(), "swift build".to_string()]
    );
    assert!(plan.rejected_commands.iter().any(|command| command
        .contains("swift test --filter TiledRendererTests")
        && command.contains("recently failed as observation hint")));

    fs::remove_dir_all(root)?;
    Ok(())
}

fn command_completed_json(command: &str, exit_code: i32) -> String {
    json!({
        "type": "item.completed",
        "item": {
            "id": format!("cmd-{exit_code}-{command}"),
            "type": "command_execution",
            "command": command,
            "aggregated_output": "",
            "exit_code": exit_code,
            "status": "completed"
        }
    })
    .to_string()
}

#[test]
fn deterministic_non_ai_queue_marks_rg_no_matches_done() -> Result<()> {
    let root = temp_root("non-ai-no-matches");
    fs::create_dir_all(root.join(".orchestrator"))?;
    write_text(&root.join("README.md"), "hello\n")?;
    write_text(
        &root.join("NEXT_STEPS_non-ai.md"),
        r#"
# NEXT_STEPS_non-ai

### NA-001 - Verify Missing Pattern
- Task ID: `NA-001`
- Status: Approved.
- Task type: Command-only verification.
- Objective: Run a deterministic read-only verification command.
- Allowed files:
  - `NEXT_STEPS_non-ai.md`
- Exact steps:
  1. Run the listed verification command.
- Verification commands:
  - `rg -n "definitely-not-here" README.md`
"#,
    )?;

    let paths = Paths::new(&root)?;
    let mut cfg = Config::default();
    cfg.intern_support = InternSupport::Off;
    let mut state = State::default();

    let should_continue = run_next_non_ai_task(&paths, &cfg, &mut state)?;
    let updated = read_text(&paths.next_steps_non_ai)?;

    assert_eq!(should_continue, Some(true));
    assert!(updated.contains("- Status: Done."));
    assert!(updated.contains("`noMatches`"));
    assert!(updated.contains("rg exited 1 with no matches"));
    assert_eq!(state.completed_tasks, 1);

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn non_ai_command_packet_without_task_card_pauses_before_ai_roles() -> Result<()> {
    let root = temp_root("non-ai-command-packet");
    fs::create_dir_all(root.join(".orchestrator"))?;
    write_text(
        &root.join("NEXT_STEPS_non-ai.md"),
        r#"
# NEXT_STEPS_non-ai

## Command Packet: Bounded Runtime

This packet is for a non-AI executor or human.

### Preflight Commands

```sh
git status --short
git diff --check
```

### Required Source Changes

- Add bounded session fields before running validation.
"#,
    )?;

    let paths = Paths::new(&root)?;
    let cfg = Config::default();
    let mut state = State {
        master_due: true,
        planner_due: true,
        needs_reconciliation: true,
        retry_worker_same_task: Some("TASK-249".to_string()),
        ..State::default()
    };

    let should_continue = run_next_non_ai_task(&paths, &cfg, &mut state)?;

    assert_eq!(should_continue, Some(false));
    assert!(!state.master_due);
    assert!(!state.planner_due);
    assert!(!state.needs_reconciliation);
    assert!(state.retry_worker_same_task.is_none());
    assert!(state
        .last_review_summary
        .as_deref()
        .unwrap_or_default()
        .contains("no approved deterministic task card"));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn non_ai_command_packet_with_edit_targets_materializes_worker_without_pm() -> Result<()> {
    let root = temp_root("non-ai-packet-materializes");
    fs::create_dir_all(root.join(".orchestrator"))?;
    fs::create_dir_all(root.join("Sources/App"))?;
    fs::create_dir_all(root.join("Tests/AppTests"))?;
    write_text(
        &root.join("Sources/App/SessionRunSnapshot.swift"),
        "// snapshot\n",
    )?;
    write_text(
        &root.join("Sources/App/SessionPayloadSessionSender.swift"),
        "// sender\n",
    )?;
    write_text(
        &root.join("Sources/App/SessionCycleResult.swift"),
        "// result\n",
    )?;
    write_text(
        &root.join("Sources/App/AppCoordinator+ManagedConfigRecordLookupSessionCycleSend.swift"),
        "// cycle send\n",
    )?;
    write_text(
        &root.join("Tests/AppTests/AppCoordinatorTests+SessionRun.swift"),
        "// tests\n",
    )?;
    write_text(
        &root.join("NEXT_STEPS_non-ai.md"),
        r#"
# NEXT_STEPS_non-ai

## Command Packet: Bounded Session Workflow Runtime

### Goal

Move from one queued ConfigRecord lookup per manual bounded Session cycle to a
bounded multi-action manual cycle.

### Preflight Commands

```sh
git status --short
git diff --check
find Sources Tests -name '*.swift' -print0 | xargs -0 wc -l | awk '$2 != "total" && $1 > 200 { print }'
```

### Discovery Commands

```sh
rg -n "SessionCycle" Sources/App Tests/AppTests
```

### Edit Targets

- `Sources/App/SessionCycleSender.swift`
- `Sources/App/AppCoordinator+ManagedConfigRecordLookupSessionCycle*.swift`
- `Sources/App/AppCoordinator+SessionCycleRun.swift`
- `Tests/AppTests/FakeSessionCycleSender.swift`

### Required Source Changes

1. Add a bounded cycle-window concept to the bounded session cycle request/result.
2. Continue reading inbound Session frames after sends until frame limit or timeout.

### Post-Edit Validation Commands

```sh
git diff --check
find Sources Tests -name '*.swift' -print0 | xargs -0 wc -l | awk '$2 != "total" && $1 > 200 { print }'
swift build --product app
swift test --filter Session
swift test
```

### Stop Conditions

- validation fails;
- source edits require public sockets;
"#,
    )?;
    let paths = Paths::new(&root)?;
    let cfg = Config::default();
    let title = "Bounded Session Workflow Runtime".to_string();
    let materialized_id = non_ai_packet_task_id(&title);
    let mut state = State {
        master_due: true,
        planner_due: true,
        needs_reconciliation: true,
        retry_worker_same_task: Some("TASK-249".to_string()),
        queue: vec![TaskSpec {
            task_id: materialized_id.clone(),
            title: title.clone(),
            ..sample_task(vec!["Sources/App/AppCoordinator.swift".to_string()])
        }],
        ..State::default()
    };

    let should_continue = run_next_non_ai_task(&paths, &cfg, &mut state)?;

    assert_eq!(should_continue, None);
    assert_eq!(state.queue.len(), 1);
    let task = &state.queue[0];
    assert_eq!(task.task_id, materialized_id);
    assert_eq!(task.parent_task.as_deref(), None);
    assert_eq!(task.title, title);
    assert_eq!(task.kind, "implementation");
    assert!(task
        .allowed_files
        .contains(&"Sources/App/SessionCycleSender.swift".to_string()));
    assert!(task.allowed_files.contains(
        &"Sources/App/AppCoordinator+ManagedConfigRecordLookupSessionCycleSend.swift".to_string()
    ));
    assert!(task
        .allowed_files
        .contains(&"Sources/App/AppCoordinator+SessionCycleRun.swift".to_string()));
    assert!(task
        .allowed_files
        .contains(&"Tests/AppTests/FakeSessionCycleSender.swift".to_string()));
    assert!(!task
        .allowed_files
        .contains(&"Sources/App/SessionRunSnapshot.swift".to_string()));
    assert!(state
        .queue
        .iter()
        .all(|queued| { queued.allowed_files.iter().all(|path| !path.contains('*')) }));
    assert!(task
        .tests_to_run
        .contains(&"swift build --product app".to_string()));
    assert!(task.tests_to_run.contains(&"swift test".to_string()));
    assert!(!task
        .tests_to_run
        .iter()
        .any(|command| command.contains("xargs")));
    assert!(!state.master_due);
    assert!(!state.planner_due);
    assert!(!state.needs_reconciliation);
    assert!(state.retry_worker_same_task.is_none());
    assert!(task
        .forbidden_actions
        .iter()
        .any(|rule| rule.contains("SwiftPM build/test command is sandbox/cache-blocked")));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn completed_non_ai_command_packet_does_not_pause_empty_queue() -> Result<()> {
    let root = temp_root("non-ai-packet-completed-no-pause");
    fs::create_dir_all(root.join(".orchestrator"))?;
    write_text(
        &root.join("NEXT_STEPS_non-ai.md"),
        r#"
# NEXT_STEPS_non-ai

## Command Packet: Bounded Session Workflow Runtime

### Goal

Move from one queued ConfigRecord lookup per manual bounded Session cycle to a
bounded multi-action manual cycle.

### Edit Targets

- `Sources/App/SessionCycleSender.swift`

### Required Source Changes

1. Add a bounded cycle-window concept to the bounded session cycle request/result.

### Post-Edit Validation Commands

```sh
git diff --check
swift build --product app
swift test
```
"#,
    )?;
    let paths = Paths::new(&root)?;
    let packet_text = read_text(&root.join("NEXT_STEPS_non-ai.md"))?;
    let tasks = non_ai_command_packet_tasks_from_text(&paths, &packet_text);
    assert!(!tasks.is_empty());
    let mut next_steps = "# NEXT_STEPS\n\n".to_string();
    for task in &tasks {
        next_steps.push_str(&format!(
            "### {} - {}\n- Task ID: `{}`\n- Status: Done.\n\n",
            task.task_id, task.title, task.task_id
        ));
    }
    write_text(&root.join("NEXT_STEPS.md"), &next_steps)?;
    let cfg = Config::default();
    let mut state = State {
        master_due: true,
        planner_due: false,
        needs_reconciliation: false,
        ..State::default()
    };

    let should_continue = run_next_non_ai_task(&paths, &cfg, &mut state)?;

    assert_eq!(should_continue, None);
    assert!(state.last_review_summary.is_none());
    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn non_ai_command_packet_with_patch_applies_without_worker() -> Result<()> {
    let root = temp_root("non-ai-patch-packet");
    fs::create_dir_all(root.join(".orchestrator"))?;
    fs::create_dir_all(root.join("Sources"))?;
    write_text(&root.join("Sources/Foo.swift"), "let value = 1\n")?;
    Command::new("git")
        .arg("init")
        .current_dir(&root)
        .output()?;
    Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(&root)
        .output()?;
    Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(&root)
        .output()?;
    Command::new("git")
        .args(["add", "."])
        .current_dir(&root)
        .output()?;
    Command::new("git")
        .args(["commit", "-m", "initial"])
        .current_dir(&root)
        .output()?;
    write_text(
        &root.join("NEXT_STEPS_non-ai.md"),
        r#"
# NEXT_STEPS_non-ai

## Command Packet: Apply Exact Foo Patch

### Edit Targets

- `Sources/Foo.swift`

### Rust Patch

```diff
diff --git a/Sources/Foo.swift b/Sources/Foo.swift
--- a/Sources/Foo.swift
+++ b/Sources/Foo.swift
@@ -1 +1 @@
-let value = 1
+let value = 2
```

### Post-Edit Validation Commands

```sh
git diff --check
```
"#,
    )?;

    let paths = Paths::new(&root)?;
    let cfg = Config::default();
    let mut state = State {
        master_due: true,
        planner_due: true,
        ..State::default()
    };

    let should_continue = run_next_non_ai_task(&paths, &cfg, &mut state)?;

    assert_eq!(should_continue, Some(true));
    assert_eq!(
        read_text(&root.join("Sources/Foo.swift"))?,
        "let value = 2\n"
    );
    assert_eq!(state.completed_tasks, 1);
    assert!(!state.master_due);
    assert!(!state.planner_due);
    assert!(read_text(&paths.next_steps_non_ai)?.contains("- Status: Done."));
    let status = Command::new("git")
        .args(["status", "--short"])
        .current_dir(&root)
        .output()?;
    let status_text = String::from_utf8_lossy(&status.stdout);
    let non_orchestrator_status = status_text
        .lines()
        .filter(|line| !line.contains(".orchestrator/"))
        .collect::<Vec<_>>();
    assert!(non_orchestrator_status.is_empty());

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn deterministic_unclassified_failure_stays_unfinished() -> Result<()> {
    let root = temp_root("non-ai-unclassified");
    fs::create_dir_all(root.join(".orchestrator"))?;
    write_text(&root.join("README.md"), "hello\n")?;
    write_text(
        &root.join("NEXT_STEPS_non-ai.md"),
        r#"
# NEXT_STEPS_non-ai

### NA-002 - Bad Regex Probe
- Task ID: `NA-002`
- Status: Approved.
- Task type: Command-only verification.
- Objective: Run a deterministic read-only verification command.
- Allowed files:
  - `NEXT_STEPS_non-ai.md`
- Exact steps:
  1. Run the listed verification command.
- Verification commands:
  - `rg -n "[" README.md`
"#,
    )?;

    let paths = Paths::new(&root)?;
    let mut cfg = Config::default();
    cfg.intern_support = InternSupport::Off;
    let mut state = State::default();

    let should_continue = run_next_non_ai_task(&paths, &cfg, &mut state)?;
    let updated = read_text(&paths.next_steps_non_ai)?;

    assert_eq!(should_continue, Some(false));
    assert!(updated.contains("- Status: Approved."));
    assert!(!updated.contains("Deterministic execution notes"));
    assert!(state.planner_due);

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn deterministic_status_update_can_record_known_blocker() -> Result<()> {
    let root = temp_root("non-ai-known-blocker");
    fs::create_dir_all(root.join(".orchestrator"))?;
    write_text(
        &root.join("NEXT_STEPS_non-ai.md"),
        r#"
# NEXT_STEPS_non-ai

### NA-003 - SwiftPM Probe
- Task ID: `NA-003`
- Status: Approved.
- Task type: Command-only verification.
- Objective: Run a deterministic command and record the environment result.
- Allowed files:
  - `NEXT_STEPS_non-ai.md`
- Verification commands:
  - `swift build --product demo`
"#,
    )?;

    let mut task = sample_task(vec!["NEXT_STEPS_non-ai.md".to_string()]);
    task.task_id = "NA-003".to_string();
    let result = DeterministicCommandResult {
        command: vc("swift build --product demo"),
        exit_code: 1,
        duration_ms: 0,
        category: DeterministicResultCategory::KnownEnvironmentBlocker,
        evidence: "known SwiftPM sandbox/module-cache blocker".to_string(),
        stdout_tail: String::new(),
        stderr_tail: String::new(),
    };

    mark_task_status_mechanically(
        &root.join("NEXT_STEPS_non-ai.md"),
        &task,
        "Blocked.",
        &[result],
    )?;
    let updated = read_text(&root.join("NEXT_STEPS_non-ai.md"))?;

    assert!(updated.contains("- Status: Blocked."));
    assert!(updated.contains("`knownEnvironmentBlocker`"));
    assert!(updated.contains("known SwiftPM sandbox/module-cache blocker"));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn deterministic_pm_returned_task_without_markdown_card_records_non_ai_entry() -> Result<()> {
    let root = temp_root("pm-deterministic-missing-card");
    fs::create_dir_all(root.join(".orchestrator"))?;
    write_text(
        &root.join("NEXT_STEPS.md"),
        "# NEXT_STEPS\n\nNo approved Worker tasks.\n",
    )?;
    write_text(
        &root.join("NEXT_STEPS_non-ai.md"),
        "# NEXT_STEPS_non-ai\n\nNo approved deterministic tasks yet.\n",
    )?;
    let paths = Paths::new(&root)?;
    let mut task = sample_task(vec!["NEXT_STEPS_non-ai.md".to_string()]);
    task.task_id = "TASK-236V".to_string();
    task.title = "Verify Post-TASK235 Local Run Test Closure".to_string();
    task.kind = "verification".to_string();
    task.objective = "Run command-only post-TASK-235 closure evidence.".to_string();
    task.tests_to_run = vec!["git diff --check".to_string()];

    let (queue_path, queue_label) = deterministic_task_queue_target(&paths, &task);
    assert_eq!(queue_label, "NEXT_STEPS_non-ai.md");
    assert_eq!(queue_path, paths.next_steps_non_ai.as_path());

    let result = DeterministicCommandResult {
        command: vc("git diff --check"),
        exit_code: 0,
        duration_ms: 20,
        category: DeterministicResultCategory::Passed,
        evidence: "exit 0".to_string(),
        stdout_tail: String::new(),
        stderr_tail: String::new(),
    };
    mark_task_status_mechanically(queue_path, &task, "Done.", &[result])?;
    let updated_non_ai = read_text(&paths.next_steps_non_ai)?;
    let updated_next_steps = read_text(&paths.next_steps)?;

    assert!(updated_non_ai.contains("### TASK-236V: Verify Post-TASK235 Local Run Test Closure"));
    assert!(updated_non_ai.contains("- Status: Done."));
    assert!(updated_non_ai.contains("`git diff --check` exited 0 (`passed`): exit 0."));
    assert!(!updated_next_steps.contains("TASK-236V"));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn deterministic_queue_bookkeeping_can_commit_ignored_non_ai_queue() -> Result<()> {
    let root = temp_root("ignored-non-ai-bookkeeping");
    fs::create_dir_all(root.join(".orchestrator"))?;
    write_text(&root.join(".gitignore"), "NEXT_STEPS_non-ai.md\n")?;
    write_text(&root.join("README.md"), "hello\n")?;
    Command::new("git")
        .arg("init")
        .current_dir(&root)
        .output()?;
    Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(&root)
        .output()?;
    Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(&root)
        .output()?;
    Command::new("git")
        .args(["add", ".gitignore", "README.md"])
        .current_dir(&root)
        .output()?;
    Command::new("git")
        .args(["commit", "-m", "initial"])
        .current_dir(&root)
        .output()?;
    write_text(
        &root.join("NEXT_STEPS_non-ai.md"),
        r#"
# NEXT_STEPS_non-ai

### NA-004 - Diff Check
- Task ID: `NA-004`
- Status: Approved.
- Task type: Command-only verification.
- Objective: Run a deterministic command and record the result.
- Allowed files:
  - `NEXT_STEPS_non-ai.md`
- Verification commands:
  - `git diff --check`
"#,
    )?;

    let paths = Paths::new(&root)?;
    let cfg = Config::default();
    let mut state = State::default();

    let should_continue = run_next_non_ai_task(&paths, &cfg, &mut state)?;

    assert_eq!(should_continue, Some(true));
    assert_eq!(state.completed_tasks, 1);
    let updated = read_text(&paths.next_steps_non_ai)?;
    assert!(updated.contains("- Status: Done."));
    assert!(updated.contains("Deterministic execution notes"));

    let ls_files = Command::new("git")
        .args(["ls-files", "--", "NEXT_STEPS_non-ai.md"])
        .current_dir(&root)
        .output()?;
    assert_eq!(
        String::from_utf8_lossy(&ls_files.stdout).trim(),
        "NEXT_STEPS_non-ai.md"
    );
    let status = Command::new("git")
        .args(["status", "--short"])
        .current_dir(&root)
        .output()?;
    let status_text = String::from_utf8_lossy(&status.stdout);
    let non_orchestrator_status = status_text
        .lines()
        .filter(|line| !line.contains(".orchestrator/"))
        .collect::<Vec<_>>();
    assert!(non_orchestrator_status.is_empty());

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn accepted_worker_task_without_markdown_card_records_next_steps_entry() -> Result<()> {
    let root = temp_root("worker-missing-card");
    fs::create_dir_all(root.join(".orchestrator"))?;
    write_text(
        &root.join("NEXT_STEPS.md"),
        "# NEXT_STEPS\n\nNo approved Worker tasks.\n",
    )?;
    let path = root.join("NEXT_STEPS.md");
    let mut task = sample_task(vec!["TASK.md".to_string()]);
    task.task_id = "TASK-237V".to_string();
    task.title = "Classify Post-Closure Dirty Worktree".to_string();
    task.kind = "verification".to_string();
    task.objective = "Classify dirty worktree state without source edits.".to_string();
    task.tests_to_run = vec![
        "git status --short".to_string(),
        "git diff --check".to_string(),
    ];
    let mut report = evidence_report(vec![CommandEvidence {
        id: "cmd-1".to_string(),
        command: vc("git diff --check"),
        command_display: "git diff --check".to_string(),
        exit_code: 0,
        success: true,
        category: "passed".to_string(),
        duration_ms: 20,
        stdout_tail: String::new(),
        stderr_tail: String::new(),
        evidence_summary: "exit 0".to_string(),
    }]);
    report.status = "passed".to_string();

    mark_task_done_with_evidence(&path, &task, &report, "rust_auto_accept_verified")?;
    let updated = read_text(&path)?;

    assert!(updated.contains("### TASK-237V - Classify Post-Closure Dirty Worktree"));
    assert!(updated.contains("- Status: Done."));
    assert!(updated.contains("- Rust review notes:"));
    assert!(updated.contains("`git diff --check` exited 0 (`passed`) in 20ms: exit 0."));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn deterministic_probe_summaries_follow_parent_task() -> Result<()> {
    let root = temp_root("probe-summary");
    fs::create_dir_all(root.join(".orchestrator"))?;
    write_text(
        &root.join("NEXT_STEPS_non-ai.md"),
        r#"
# NEXT_STEPS_non-ai

### PROBE-001 - Parent Probe
- Task ID: `PROBE-001`
- Parent task: `TASK-200`
- Status: Done.
- Deterministic execution notes:
  - Classified and recorded by the Rust deterministic lane without launching Worker or Senior Dev.
  - Verification commands:
    - `git diff --check` exited 0 (`passed`): exit 0.

### PROBE-002 - Other Probe
- Task ID: `PROBE-002`
- Parent task: `TASK-201`
- Status: Done.
- Deterministic execution notes:
  - Not relevant.
"#,
    )?;

    let paths = Paths::new(&root)?;
    let summaries = deterministic_probe_summaries(&paths, "TASK-200");

    assert_eq!(summaries.len(), 1);
    assert!(summaries[0].contains("PROBE-001"));
    assert!(summaries[0].contains("git diff --check"));
    assert!(!summaries[0].contains("PROBE-002"));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn accepted_worker_task_records_deterministic_recipe_candidate() -> Result<()> {
    let root = temp_root("recipe-candidate");
    fs::create_dir_all(root.join(".orchestrator"))?;
    let paths = Paths::new(&root)?;
    fs::create_dir_all(&paths.recipes)?;

    let mut task = sample_task(vec!["Sources/Foo.swift".to_string()]);
    task.task_id = "TASK-200".to_string();
    task.title = "Sample implementation".to_string();
    task.tests_to_run = vec![
        "swift test --filter FooTests".to_string(),
        "git diff --check".to_string(),
        "swift test --filter FooTests && git diff --check".to_string(),
    ];
    let worker = WorkerOutput {
        task_id: "TASK-200".to_string(),
        status: "done".to_string(),
        summary: "done".to_string(),
        files_changed: vec!["Sources/Foo.swift".to_string()],
        commands_run: vec![
            "rg -n \"Foo\" Sources Tests".to_string(),
            "swift test --filter FooTests; git diff --check".to_string(),
        ],
        tests_run: vec!["swift test --filter FooTests".to_string()],
        handoff_to_rust: vec![
            vc(".build/debug/app --local-transport-send-receive-smoke"),
            VerificationCommand::new(
                ".build/debug/app".to_string(),
                vec!["--public-diagnostic".to_string()],
                "worker.handoff_to_rust",
            ),
        ],
        orchestration_request: None,
        blocker: String::new(),
        confidence: "high".to_string(),
    };
    let rust_verification = json!({
        "status": "passed",
        "commands": [
            {"command": "swift build --product demo", "exit_code": 0}
        ]
    });

    let summary = record_worker_deterministic_recipe(&paths, &task, &worker, &rust_verification)?
        .expect("recipe should be recorded");
    let recipe_text = read_text(&paths.recipes.join("TASK-200-deterministic.json"))?;
    let recipe: DeterministicRecipe = serde_json::from_str(&recipe_text)?;

    assert!(summary.contains("TASK-200-deterministic"));
    assert_eq!(recipe.status, "candidate");
    assert_eq!(recipe.source_task_id, "TASK-200");
    assert_eq!(
        recipe
            .commands
            .iter()
            .map(VerificationCommand::display)
            .collect::<Vec<_>>(),
        vec![
            "swift test --filter FooTests",
            "git diff --check",
            ".build/debug/app --local-transport-send-receive-smoke",
            "swift build --product demo"
        ]
    );
    assert!(!recipe_text.contains("worker.tests_run"));
    assert!(!recipe_text.contains("worker.commands_run"));
    assert!(!recipe_text.contains("rg -n Foo Sources Tests"));
    assert!(!recipe_text.contains("&&"));
    assert!(!recipe_text.contains("; git"));
    assert!(!recipe_text.contains("--public-diagnostic"));
    assert!(read_text(&paths.recipes.join("README.md"))?
        .contains("TASK-200-deterministic from TASK-200"));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn deterministic_recipe_excludes_worker_diagnostic_command_failures() -> Result<()> {
    let root = temp_root("recipe-diagnostic-failures");
    fs::create_dir_all(root.join(".orchestrator"))?;
    let paths = Paths::new(&root)?;
    fs::create_dir_all(&paths.recipes)?;

    let mut task = sample_task(vec![
        "TESTING.md".to_string(),
        "CONTINUITY.md".to_string(),
        "NEXT_STEPS.md".to_string(),
        "NEXT_STEPS_non-ai.md".to_string(),
    ]);
    task.task_id = "DOC-001".to_string();
    task.title = "Verify Focused Test Commands In TESTING.md".to_string();
    task.kind = "verification".to_string();
    task.tests_to_run = vec!["git diff --check".to_string()];
    let mut worker = sample_worker_output(&task.task_id);
    worker.commands_run = vec![
        "swift test".to_string(),
        "swift test --filter BenchmarkSuiteTests".to_string(),
    ];
    worker.tests_run = worker.commands_run.clone();
    worker.handoff_to_rust = vec![vc("git diff --check")];
    let rust_verification = json!({
        "status": "passed",
        "commands": [
            {
                "command_display": "git diff --check",
                "exit_code": 0,
                "category": "passed"
            },
            {
                "command_display": "swift test --filter BenchmarkSuiteTests",
                "exit_code": 1,
                "category": "commandFailed"
            }
        ]
    });

    let summary = record_worker_deterministic_recipe(&paths, &task, &worker, &rust_verification)?
        .expect("recipe should be recorded");
    let recipe_text = read_text(&paths.recipes.join("DOC-001-deterministic.json"))?;
    let recipe: DeterministicRecipe = serde_json::from_str(&recipe_text)?;

    assert!(summary.contains("1 command"));
    assert_eq!(
        recipe
            .commands
            .iter()
            .map(VerificationCommand::display)
            .collect::<Vec<_>>(),
        vec!["git diff --check"]
    );
    assert!(!recipe_text.contains("swift test"));
    assert!(!recipe_text.contains("BenchmarkSuiteTests"));
    assert!(!recipe_text.contains("worker.tests_run"));

    fs::remove_dir_all(root)?;
    Ok(())
}

mod legacy_intern_support_contract {
    use super::*;

    // Keep these together while `app.rs` is split up.  When legacy config loading
    // and Rust support packets move to narrower modules, move this whole contract
    // suite with them.
    #[test]
    fn legacy_intern_config_is_accepted_but_not_serialized() {
        let raw = r#"
intern_preflight = true
intern_support = "auto-cheap"
intern_read_only_commands = true

[role_labels]
intern = "Legacy Intern"

[roles.intern]
model = "legacy-model"
effort = "low"
sandbox = "read-only"
approval_policy = "never"
web_search = "disabled"
timeout_seconds = 123
"#;
        let cfg: Config = toml::from_str(raw).expect("legacy intern config should load");

        assert!(cfg.intern_preflight);
        assert_eq!(cfg.intern_support, InternSupport::AutoCheap);
        assert!(cfg.intern_read_only_commands);
        assert_eq!(cfg.role_labels.intern, "Legacy Intern");
        assert_eq!(cfg.roles.intern.model, "legacy-model");

        let generated =
            toml::to_string_pretty(&Config::default()).expect("default config serializes");
        assert!(!generated.contains("intern_preflight"));
        assert!(!generated.contains("intern_support"));
        assert!(!generated.contains("intern_read_only_commands"));
        assert!(!generated.contains("[roles.intern]"));
        assert!(!generated.contains("intern = \"Intern\""));
    }

    #[test]
    fn rust_support_brief_includes_scope_order_warnings_and_network_risk() -> Result<()> {
        let root = temp_root("support-brief");
        fs::create_dir_all(root.join("Sources"))?;
        write_text(&root.join("Sources/lib.swift"), "func sample() {}\n")?;
        let paths = Paths::new(&root)?;
        let mut task = sample_task(vec!["NEXT_STEPS.md".to_string()]);
        task.read_only_files = vec!["Sources/lib.swift".to_string()];
        task.forbidden_actions = vec!["Do not touch public API.".to_string()];
        task.tests_to_run = vec!["swift test --filter FooTests".to_string()];
        task.stop_conditions = vec!["Stop if production code changes are needed.".to_string()];
        task.requires_network = true;
        let mut cfg = Config::default();
        cfg.intern_preflight = true;
        cfg.intern_support = InternSupport::AutoCheap;

        let brief = build_task_support_brief(&paths, &cfg, &task);
        assert!(brief.summary.contains("Rust support brief"));
        assert_eq!(brief.relevant_files.len(), 2);
        assert!(brief
            .suggested_first_reads
            .contains(&"Sources/lib.swift".to_string()));
        assert_eq!(
            brief.forbidden_actions,
            vec!["Do not touch public API.".to_string()]
        );
        assert_eq!(
            brief.verification_commands,
            vec!["swift test --filter FooTests".to_string()]
        );
        assert!(brief.stop_conditions[0].contains("Stop if"));
        assert!(brief.risks.iter().any(|risk| risk.contains("Stop if")));
        assert!(brief.risks.iter().any(|risk| risk.contains("network")));
        assert!(brief
            .warnings
            .iter()
            .any(|warning| warning.contains("NEXT_STEPS.md")));
        assert!(brief
            .warnings
            .iter()
            .any(|warning| warning.contains("no AI Intern role was launched")));
        assert_eq!(
            pretty_json(&brief),
            r#"{
  "task_id": "T001",
  "summary": "Rust support brief for T001: sample. Use the task card as authority; edit only allowed files, inspect only read-only files, run the listed checks, and stop on declared blockers.",
  "objective": "sample",
  "relevant_files": [
    "NEXT_STEPS.md",
    "Sources/lib.swift"
  ],
  "suggested_first_reads": [
    "NEXT_STEPS.md",
    "Sources/lib.swift"
  ],
  "allowed_files": [
    "NEXT_STEPS.md"
  ],
  "read_only_files": [
    "Sources/lib.swift"
  ],
  "forbidden_actions": [
    "Do not touch public API."
  ],
  "verification_commands": [
    "swift test --filter FooTests"
  ],
  "stop_conditions": [
    "Stop if production code changes are needed."
  ],
  "acceptance_contract": {
    "task_id": "T001",
    "definition_of_done": [
      "change is present"
    ],
    "verification_commands": [
      "swift test --filter FooTests"
    ],
    "review_tier": "always_ai_review",
    "automation_level": "ai_worker_ai_review",
    "allowed_files": [
      "NEXT_STEPS.md"
    ],
    "read_only_files": [
      "Sources/lib.swift"
    ],
    "stop_conditions": [
      "Stop if production code changes are needed."
    ],
    "characterization_required": false,
    "characterization_satisfied": false
  },
  "risks": [
    "Stop if production code changes are needed.",
    "Task declares network access; Worker should confirm this is explicitly approved."
  ],
  "warnings": [
    "Named support file does not exist: NEXT_STEPS.md",
    "Generated by Rust support layer; no AI Intern role was launched."
  ]
}"#
        );

        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn deterministic_failure_summary_classifies_failures_and_routes() {
        let mut task = sample_task(vec!["Sources/lib.swift".to_string()]);
        task.read_only_files = vec!["Tests/FooTests.swift".to_string()];

        let result = DeterministicCommandResult {
            command: vc("swift test --filter FooTests"),
            exit_code: 1,
            duration_ms: 0,
            category: DeterministicResultCategory::CommandFailed,
            evidence: "command exited 1: XCTAssert failed".to_string(),
            stdout_tail: "Test Case '-[FooTests testExample]' failed\n".to_string(),
            stderr_tail: "Sources/lib.swift:12:5: error: cannot find 'foo' in scope\n".to_string(),
        };
        let rejected = vec!["rm -rf target".to_string()];
        let summary = rust_result_summary_with_rejections(&task, &result, &rejected);

        assert_eq!(summary.category, FailureSummaryCategory::CompileFailure);
        assert!(summary.summary.contains("Rust classified"));
        assert!(summary
            .top_diagnostics
            .iter()
            .any(|diagnostic| diagnostic.contains("Sources/lib.swift:12:5")));
        assert!(summary
            .failed_tests
            .iter()
            .any(|name| name.contains("FooTests")));
        assert_eq!(summary.rejected_commands, rejected);
        assert_eq!(summary.suggested_route, FailureSummaryRoute::RetrySameTask);
        assert!(!summary.needs_senior_dev);
        assert_eq!(
            pretty_json(&summary),
            r#"{
  "category": "compileFailure",
  "summary": "Rust classified `swift test --filter FooTests` as `compileFailure` for T001: command exited 1: XCTAssert failed",
  "likely_files_or_tests": [
    "Sources/lib.swift",
    "Tests/FooTests.swift"
  ],
  "top_diagnostics": [
    "Sources/lib.swift:12:5: cannot find 'foo' in scope"
  ],
  "failed_tests": [
    "-[FooTests testExample]"
  ],
  "rejected_commands": [
    "rm -rf target"
  ],
  "suggested_route": "retry_same_task",
  "needs_senior_dev": false,
  "warnings": [
    "Generated by Rust support layer; no AI Intern role was launched."
  ]
}"#
        );

        let env = DeterministicCommandResult {
            category: DeterministicResultCategory::KnownEnvironmentBlocker,
            evidence: "xcodebuild not available".to_string(),
            stderr_tail: "Operation not permitted".to_string(),
            ..result.clone()
        };
        assert_eq!(
            rust_result_summary(&task, &env).category,
            FailureSummaryCategory::EnvironmentBlocker
        );
        assert_eq!(
            rust_result_summary(&task, &env).suggested_route,
            FailureSummaryRoute::Planner
        );

        let timeout = DeterministicCommandResult {
            category: DeterministicResultCategory::TimedOut,
            evidence: "timed out".to_string(),
            stderr_tail: String::new(),
            ..result.clone()
        };
        assert_eq!(
            rust_result_summary(&task, &timeout).category,
            FailureSummaryCategory::TimeoutFailure
        );

        let no_match = DeterministicCommandResult {
            command: vc("rg -n Missing README.md"),
            category: DeterministicResultCategory::NoMatches,
            evidence: "rg exited 1 with no matches".to_string(),
            stdout_tail: String::new(),
            stderr_tail: String::new(),
            ..result.clone()
        };
        assert_eq!(
            rust_result_summary(&task, &no_match).category,
            FailureSummaryCategory::NoMatches
        );

        let schema = DeterministicCommandResult {
            evidence: "schema validation failed".to_string(),
            stderr_tail: "schema validation failed".to_string(),
            ..result.clone()
        };
        assert_eq!(
            rust_result_summary(&task, &schema).category,
            FailureSummaryCategory::SchemaFailure
        );

        let safety = DeterministicCommandResult {
            evidence: "forbidden command rejected".to_string(),
            stderr_tail: "forbidden command rejected by safety policy".to_string(),
            ..result.clone()
        };
        assert_eq!(
            rust_result_summary(&task, &safety).category,
            FailureSummaryCategory::SafetyFailure
        );
    }

    #[test]
    fn queue_snapshot_reports_mixed_queues_and_stale_completion() -> Result<()> {
        let root = temp_root("queue-snapshot");
        fs::create_dir_all(root.join(".orchestrator"))?;
        write_text(
            &root.join("NEXT_STEPS.md"),
            r#"
## Active Queue Summary
- Approved Worker task queue:
  - `TASK-200`

### TASK-199 - Done Thing
- Task ID: `TASK-199`
- Status: Done.

### TASK-200 - Worker Thing
- Task ID: `TASK-200`
- Status: Approved.
- Allowed files:
  - `README.md`

### TASK-201 - Blocked Thing
- Task ID: `TASK-201`
- Status: Blocked.

### TASK-202 - Pending Thing
- Task ID: `TASK-202`
- Status: Pending approval.

### TASK-300 - Future Thing
- Task ID: `TASK-300`
- Status: Draft. Requires explicit approval before implementation.
"#,
        )?;
        write_text(
            &root.join("NEXT_STEPS_non-ai.md"),
            r#"
## Active Queue Summary
- Approved Worker task queue:
  - `NA-001`

### NA-001 - Deterministic Thing
- Task ID: `NA-001`
- Status: Approved.
- Allowed files:
  - `NEXT_STEPS_non-ai.md`
- Verification commands:
  - `rg -n hello README.md`
"#,
        )?;
        let paths = Paths::new(&root)?;
        let mut state = State::default();
        state.project_complete = true;
        state.queue = vec![sample_task(vec!["README.md".to_string()])];
        state.queue[0].task_id = "TASK-200".to_string();

        let snapshot = build_queue_snapshot_summary(&paths, &state);

        assert_eq!(snapshot.approved_worker_count, 1);
        assert_eq!(snapshot.approved_non_ai_count, 1);
        assert_eq!(snapshot.active_task_ids, vec!["TASK-200".to_string()]);
        assert_eq!(snapshot.done_count, 1);
        assert_eq!(snapshot.blocked_count, 1);
        assert_eq!(snapshot.pending_count, 1);
        assert!(snapshot.next_steps_unfinished);
        assert!(snapshot.non_ai_unfinished);
        assert!(snapshot.future_work);
        assert!(snapshot.stale_project_complete);
        assert_eq!(
            snapshot.deterministic_queue_status,
            DeterministicQueueStatus::ApprovedWorkAvailable
        );
        assert_eq!(snapshot.next_runnable_task, Some("TASK-200".to_string()));
        assert_eq!(
            pretty_json(&snapshot),
            r#"{
  "approved_worker_count": 1,
  "approved_non_ai_count": 1,
  "active_task_ids": [
    "TASK-200"
  ],
  "done_count": 1,
  "blocked_count": 1,
  "pending_count": 1,
  "next_steps_unfinished": true,
  "non_ai_unfinished": true,
  "future_work": true,
  "stale_project_complete": true,
  "deterministic_queue_status": "approved_work_available",
  "next_runnable_task": "TASK-200"
}"#
        );

        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn review_assist_packet_groups_evidence_and_bookkeeping() -> Result<()> {
        let root = temp_root("review-assist");
        fs::create_dir_all(&root)?;
        let paths = Paths::new(&root)?;
        let mut task = sample_task(vec!["src/lib.rs".to_string()]);
        task.forbidden_actions = vec![
            "Do not edit outside allowed files.".to_string(),
            "No unsafe network or secret handling.".to_string(),
        ];
        let guard = GuardReport {
            enabled: true,
            status: "violated".to_string(),
            changed_files: vec![
                "src/lib.rs".to_string(),
                "tests/lib_test.rs".to_string(),
                "docs/notes.md".to_string(),
                "Cargo.toml".to_string(),
                "assets/logo.txt".to_string(),
            ],
            allowed_patterns: vec!["src/lib.rs".to_string()],
            ignored_patterns: Vec::new(),
            violations: vec!["tests/lib_test.rs".to_string()],
        };
        let mut report = evidence_report(vec![command_evidence(
            "cargo test failing_case",
            "commandFailed",
            "test failing_case ... FAILED",
        )]);
        report.rejected_commands = vec!["rm -rf target".to_string()];
        let rust_verification = json!({ "report": report });

        let packet = build_review_assist_packet(&paths, &task, &guard, &rust_verification);

        assert_eq!(packet.guard_status, "violated");
        assert_eq!(
            packet.guard_violations,
            vec!["tests/lib_test.rs".to_string()]
        );
        assert_eq!(
            packet.changed_files_by_kind.get("source"),
            Some(&vec!["src/lib.rs".to_string()])
        );
        assert_eq!(
            packet.changed_files_by_kind.get("test"),
            Some(&vec!["tests/lib_test.rs".to_string()])
        );
        assert_eq!(
            packet.changed_files_by_kind.get("docs"),
            Some(&vec!["docs/notes.md".to_string()])
        );
        assert_eq!(
            packet.changed_files_by_kind.get("config"),
            Some(&vec!["Cargo.toml".to_string()])
        );
        assert_eq!(
            packet.changed_files_by_kind.get("other"),
            Some(&vec!["assets/logo.txt".to_string()])
        );
        assert!(!packet.diff_stat.is_empty());
        assert!(packet
            .verification_results
            .iter()
            .any(|result| result.contains("commandFailed")));
        assert_eq!(packet.rejected_commands, vec!["rm -rf target".to_string()]);
        assert_eq!(packet.risk_flags.len(), 2);
        assert!(!packet.tracking_docs_bookkeeping_only);
        assert_eq!(
            pretty_json(&packet),
            r#"{
  "changed_files_by_kind": {
    "config": [
      "Cargo.toml"
    ],
    "docs": [
      "docs/notes.md"
    ],
    "other": [
      "assets/logo.txt"
    ],
    "source": [
      "src/lib.rs"
    ],
    "test": [
      "tests/lib_test.rs"
    ]
  },
  "guard_status": "violated",
  "guard_violations": [
    "tests/lib_test.rs"
  ],
  "diff_stat": "git unavailable",
  "verification_results": [
    "cargo test failing_case => commandFailed exit 1"
  ],
  "rejected_commands": [
    "rm -rf target"
  ],
  "risk_flags": [
    "Do not edit outside allowed files.",
    "No unsafe network or secret handling."
  ],
  "tracking_docs_bookkeeping_only": false
}"#
        );

        let docs_guard = clean_guard(vec!["NEXT_STEPS.md".to_string()]);
        let docs_packet = build_review_assist_packet(&paths, &task, &docs_guard, &json!({}));
        assert!(docs_packet.tracking_docs_bookkeeping_only);

        fs::remove_dir_all(root)?;
        Ok(())
    }
}

#[test]
fn no_work_stop_requires_explicit_project_completion() -> Result<()> {
    let root = temp_root("terminal-no-work");
    fs::create_dir_all(root.join(".orchestrator"))?;
    write_text(
        &root.join("NEXT_STEPS.md"),
        r#"
## Active Queue Summary
- Approved Worker task queue:
  - None currently.

### TASK-199 - Completed Thing
- Status: Done.
"#,
    )?;
    write_text(
        &root.join("NEXT_STEPS_non-ai.md"),
        "# NEXT_STEPS_non-ai\n\nNo approved deterministic tasks yet.\n",
    )?;

    let paths = Paths::new(&root)?;
    let mut state = State::default();

    assert!(next_steps_done(&paths, &state)?);
    assert!(!should_stop_for_no_work(&paths, &state)?);

    state.project_complete = true;
    assert!(should_stop_for_no_work(&paths, &state)?);

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn no_work_cycle_reports_paused_when_control_docs_created_pending_intent() {
    let mut state = State::default();
    assert_eq!(no_work_cycle_outcome(&state), "no_work");

    state.pending_commit = Some(crate::state::PendingCommit {
        id: "pending-no-work".to_string(),
        digest: "digest".to_string(),
        source_lane: "control_docs".to_string(),
        task_id: None,
        import_digest: None,
        base_head: "head".to_string(),
        dirty_paths: Vec::new(),
        path_fingerprints: Vec::new(),
        deferred_file_writes: Vec::new(),
        commit_message: "Update completed project control docs".to_string(),
        guard_evidence_ids: Vec::new(),
        verification_evidence_ids: Vec::new(),
        deferred_transition: None,
        created_at: "2026-08-20T00:00:00Z".to_string(),
    });

    assert_eq!(no_work_cycle_outcome(&state), "paused");
}

#[test]
fn rust_skips_false_conditional_branch_without_models() -> Result<()> {
    let root = temp_root("skip-false-conditional");
    fs::create_dir_all(root.join(".orchestrator"))?;
    write_text(
        &root.join("NEXT_STEPS_non-ai.md"),
        "# NEXT_STEPS_non-ai\n\nNo approved deterministic tasks yet.\n",
    )?;
    write_text(
        &root.join("NEXT_STEPS.md"),
        r#"
## Active Queue Summary
- Approved Worker task queue:
  - `TASK-153B`

### TASK-152 Result
- Task ID: `TASK-152`
- Status: Done.
- Output: `Outputs: loopback_udp_bind=passed`

### TASK-153B - Failure Branch
- Task ID: `TASK-153B`
- Status: Conditional approved.
- Run if:
  - `TASK-152.Outputs.loopback_udp_bind == failed`
  - OR
  - `TASK-152.Outputs.loopback_udp_bind == blocked`
- Allowed files:
  - `NEXT_STEPS.md`
- Implementation:
  1. Record failure.
- Verification commands:
  - `git diff --check`
"#,
    )?;

    let paths = Paths::new(&root)?;
    let cfg = Config::default();
    let mut state = State::default();

    let should_continue = skip_next_false_conditional_task(&paths, &cfg, &mut state)?;

    assert_eq!(should_continue, Some(true));
    let updated = read_text(&root.join("NEXT_STEPS.md"))?;
    assert!(updated.contains("- Status: Skipped by gate."));
    assert!(updated.contains("Skipped by Rust deterministic gate"));
    assert!(updated.contains("loopback_udp_bind"));
    assert!(state.last_worker_summary.unwrap().contains("TASK-153B"));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn rust_promotes_well_formed_pending_task_when_dependencies_are_done() -> Result<()> {
    let root = temp_root("promote-pending");
    fs::create_dir_all(root.join(".orchestrator"))?;
    write_text(
        &root.join("NEXT_STEPS.md"),
        r#"
## Active Queue Summary
- Approved Worker task queue:
  - None.

### TASK-200 - Previous Slice
- Task ID: `TASK-200`
- Status: Done.
- Outputs: previous_slice=present

### TASK-201 - Next Safe Slice
- Task ID: `TASK-201`
- Title: Next Safe Slice
- Objective: Do the next bounded thing.
- Status: Pending approval.
- Depends on: `TASK-200`
- Task type: Implementation.
- Allowed files:
  - `src/main.rs`
- Forbidden files/actions:
  - Do not touch network behavior.
- Exact steps:
  1. Make the small edit.
- Verification commands:
  - `cargo test`
  - `git diff --check`
- Success criteria:
  - Evidence is recorded.
- Stop conditions:
  - Stop if scope expands.
"#,
    )?;

    let paths = Paths::new(&root)?;
    let mut state = State::default();

    let promoted = promote_pending_next_steps_work(&paths, &mut state)?;

    assert_eq!(promoted, 1);
    assert_eq!(state.queue.len(), 1);
    assert_eq!(state.queue[0].task_id, "TASK-201");
    assert!(!state.planner_due);
    assert!(!state.master_due);
    assert!(!state.needs_reconciliation);
    let updated = read_text(&root.join("NEXT_STEPS.md"))?;
    assert!(updated.contains("- Status: Approved."));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn rust_does_not_promote_pending_task_with_unsatisfied_dependency() -> Result<()> {
    let root = temp_root("promote-pending-missing-dep");
    fs::create_dir_all(root.join(".orchestrator"))?;
    write_text(
        &root.join("NEXT_STEPS.md"),
        r#"
### TASK-202 - Blocked Slice
- Task ID: `TASK-202`
- Title: Blocked Slice
- Objective: Wait for dependency.
- Status: Pending approval.
- Depends on: `TASK-201`
- Allowed files:
  - `src/main.rs`
- Forbidden files/actions:
  - Do not touch network behavior.
- Exact steps:
  1. Make the small edit.
- Verification commands:
  - `cargo test`
- Success criteria:
  - Evidence is recorded.
- Stop conditions:
  - Stop if scope expands.
"#,
    )?;

    let paths = Paths::new(&root)?;
    let mut state = State::default();

    assert_eq!(promote_pending_next_steps_work(&paths, &mut state)?, 0);
    assert!(state.queue.is_empty());
    let updated = read_text(&root.join("NEXT_STEPS.md"))?;
    assert!(updated.contains("- Status: Pending approval."));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn rust_does_not_promote_malformed_pending_task() -> Result<()> {
    let root = temp_root("promote-pending-malformed");
    fs::create_dir_all(root.join(".orchestrator"))?;
    write_text(
        &root.join("NEXT_STEPS.md"),
        r#"
### TASK-203 - Missing Verification
- Task ID: `TASK-203`
- Title: Missing Verification
- Objective: This card is not ready.
- Status: Pending approval.
- Allowed files:
  - `src/main.rs`
- Forbidden files/actions:
  - Do not touch network behavior.
- Exact steps:
  1. Make the small edit.
- Success criteria:
  - Evidence is recorded.
- Stop conditions:
  - Stop if scope expands.
"#,
    )?;

    let paths = Paths::new(&root)?;
    let mut state = State::default();

    assert_eq!(promote_pending_next_steps_work(&paths, &mut state)?, 0);
    assert!(state.queue.is_empty());

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn senior_batch_approval_queues_existing_pending_task_without_pm() -> Result<()> {
    let root = temp_root("senior-batch-queues-pending");
    fs::create_dir_all(root.join(".orchestrator"))?;
    write_text(
        &root.join("NEXT_STEPS.md"),
        r#"
### TASK-300 - Done Dependency
- Task ID: `TASK-300`
- Status: Done.
- Outputs: previous_output=present

### TASK-301 - Tiny Continuation
- Task ID: `TASK-301`
- Title: Tiny Continuation
- Objective: Continue the local bounded lane.
- Status: Pending approval.
- Depends on: `TASK-300`
- Task type: Implementation.
- Review tier: auto_accept_if_verified
- Automation level: ai_worker_rust_review
- Allowed files:
  - `src/main.rs`
- Forbidden files/actions:
  - Do not broaden scope.
- Exact steps:
  1. Make the local bounded edit.
- Verification commands:
  - `cargo test`
  - `git diff --check`
- Success criteria:
  - Evidence is recorded.
- Stop conditions:
  - Stop if scope expands.
"#,
    )?;
    let paths = Paths::new(&root)?;
    let cfg = Config::default();
    let mut state = State::default();
    let candidates =
        senior_batch_candidate_tasks_from_text(&read_text(&root.join("NEXT_STEPS.md"))?);

    let queued = apply_senior_batch_approval(
        &paths,
        &cfg,
        &mut state,
        &candidates,
        SeniorBatchApprovalOutput {
            approved: true,
            batch_id: "batch-301".to_string(),
            task_ids: vec!["TASK-301".to_string()],
            reason_code: "approved_bounded_batch".to_string(),
            max_tasks: 1,
            allowed_kinds: vec!["local_snapshot".to_string()],
            forbidden_scope: vec!["public sockets".to_string()],
            requires_stop_on: vec!["scope expansion".to_string()],
            verification_required: vec!["cargo test".to_string()],
            auto_accept_policy: "low_risk_verified_only".to_string(),
            needs_planner: false,
            needs_master: false,
            summary: "Senior approves one bounded continuation task.".to_string(),
            warnings: Vec::new(),
            progress_actions: Vec::new(),
        },
    )?;

    assert_eq!(queued, 1);
    assert_eq!(state.queue.len(), 1);
    assert_eq!(state.queue[0].task_id, "TASK-301");
    assert!(!state.planner_due);
    assert!(state.senior_batch_approval.is_some());
    let next_steps = read_text(&root.join("NEXT_STEPS.md"))?;
    assert!(next_steps.contains("- Status: Approved."));
    let cost = read_text(&root.join(".orchestrator/cost.jsonl"))?;
    assert!(cost.contains("senior_batch_approved_worker_continuation"));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn senior_batch_rejects_task_ids_outside_candidate_set() -> Result<()> {
    let root = temp_root("senior-batch-invalid-id");
    fs::create_dir_all(root.join(".orchestrator"))?;
    write_text(
        &root.join("NEXT_STEPS.md"),
        r#"
### TASK-302 - Tiny Continuation
- Task ID: `TASK-302`
- Title: Tiny Continuation
- Objective: Continue the local bounded lane.
- Status: Pending approval.
- Task type: Implementation.
- Allowed files:
  - `src/main.rs`
- Forbidden files/actions:
  - Do not broaden scope.
- Exact steps:
  1. Make the local bounded edit.
- Verification commands:
  - `cargo test`
- Success criteria:
  - Evidence is recorded.
- Stop conditions:
  - Stop if scope expands.
"#,
    )?;
    let paths = Paths::new(&root)?;
    let cfg = Config::default();
    let mut state = State::default();
    let candidates =
        senior_batch_candidate_tasks_from_text(&read_text(&root.join("NEXT_STEPS.md"))?);

    let queued = apply_senior_batch_approval(
        &paths,
        &cfg,
        &mut state,
        &candidates,
        SeniorBatchApprovalOutput {
            approved: true,
            batch_id: "bad-batch".to_string(),
            task_ids: vec!["TASK-999".to_string()],
            reason_code: "approved_bounded_batch".to_string(),
            max_tasks: 1,
            allowed_kinds: vec!["implementation".to_string()],
            forbidden_scope: Vec::new(),
            requires_stop_on: Vec::new(),
            verification_required: vec!["cargo test".to_string()],
            auto_accept_policy: "low_risk_verified_only".to_string(),
            needs_planner: false,
            needs_master: false,
            summary: "Invalid approval.".to_string(),
            warnings: Vec::new(),
            progress_actions: Vec::new(),
        },
    )?;

    assert_eq!(queued, 0);
    assert!(state.queue.is_empty());
    assert!(state.senior_batch_approval.is_none());
    assert!(state.planner_due);

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn senior_batch_schema_deserializes() -> Result<()> {
    let output: SeniorBatchApprovalOutput = serde_json::from_str(
        r#"{
          "approved": true,
          "batch_id": "batch-1",
          "task_ids": ["TASK-1"],
          "reason_code": "approved_bounded_batch",
          "max_tasks": 1,
          "allowed_kinds": ["docs_only"],
          "forbidden_scope": ["public sockets"],
          "requires_stop_on": ["guard violation"],
          "verification_required": ["git diff --check"],
          "auto_accept_policy": "low_risk_verified_only",
          "needs_planner": false,
          "needs_master": false,
          "summary": "approved",
          "warnings": [],
          "progress_actions": []
        }"#,
    )?;

    assert!(output.approved);
    assert_eq!(output.task_ids, vec!["TASK-1".to_string()]);
    Ok(())
}

#[test]
fn foreman_progress_schema_accepts_rust_commit_action_kind() -> Result<()> {
    let output: SeniorBatchApprovalOutput = serde_json::from_str(
        r#"{
          "approved": false,
          "batch_id": "progress-batch",
          "task_ids": [],
          "reason_code": "no_safe_batch",
          "max_tasks": 0,
          "allowed_kinds": [],
          "forbidden_scope": [],
          "requires_stop_on": [],
          "verification_required": [],
          "auto_accept_policy": "low_risk_verified_only",
          "needs_planner": false,
          "needs_master": false,
          "summary": "progress",
          "warnings": [],
          "progress_actions": [
            {"action_id":"A1","kind":"rust_evidence_job","title":"t","summary":"s","allowed_files":[],"read_only_files":[],"commands":["git status --short"],"success_criteria":[],"stop_conditions":[]},
            {"action_id":"A2","kind":"rust_commit_job","title":"t","summary":"s","commit_message":"Commit t","allowed_files":["README.md"],"read_only_files":[],"commands":["git diff --check"],"success_criteria":[],"stop_conditions":[]},
            {"action_id":"A3","kind":"worker_edit_task","title":"t","summary":"s","allowed_files":["README.md"],"read_only_files":[],"commands":["git diff --check"],"success_criteria":[],"stop_conditions":[]},
            {"action_id":"A4","kind":"senior_decision","title":"t","summary":"s","allowed_files":[],"read_only_files":[],"commands":[],"success_criteria":[],"stop_conditions":[]},
            {"action_id":"A5","kind":"pm_direction","title":"t","summary":"s","allowed_files":[],"read_only_files":[],"commands":[],"success_criteria":[],"stop_conditions":[]}
          ]
        }"#,
    )?;

    assert_eq!(output.progress_actions.len(), 5);
    assert_eq!(
        output.progress_actions[0].kind,
        SeniorProgressActionKind::RustEvidenceJob
    );
    assert_eq!(
        output.progress_actions[1].kind,
        SeniorProgressActionKind::RustCommitJob
    );
    assert_eq!(
        output.progress_actions[4].kind,
        SeniorProgressActionKind::PmDirection
    );
    Ok(())
}

#[test]
fn foreman_progress_rust_evidence_job_runs_without_worker_task() -> Result<()> {
    let root = temp_root("foreman-progress-rust-evidence");
    let paths = Paths::new(&root)?;
    init_repo(&paths, "test foreman rust evidence job")?;
    init_git_repo(&root)?;
    let cfg = Config::default();
    let mut state = State::default();
    let queued = apply_senior_batch_approval(
        &paths,
        &cfg,
        &mut state,
        &[],
        SeniorBatchApprovalOutput {
            approved: false,
            batch_id: "progress-batch".to_string(),
            task_ids: Vec::new(),
            reason_code: "no_safe_batch".to_string(),
            max_tasks: 0,
            allowed_kinds: Vec::new(),
            forbidden_scope: Vec::new(),
            requires_stop_on: Vec::new(),
            verification_required: Vec::new(),
            auto_accept_policy: "low_risk_verified_only".to_string(),
            needs_planner: false,
            needs_master: false,
            summary: "Run Rust evidence first.".to_string(),
            warnings: Vec::new(),
            progress_actions: vec![SeniorProgressAction {
                action_id: "EVIDENCE-001".to_string(),
                kind: SeniorProgressActionKind::RustEvidenceJob,
                title: "Capture Git Status Receipt".to_string(),
                summary: "Rust should capture a compact git status receipt.".to_string(),
                commit_message: String::new(),
                allowed_files: Vec::new(),
                read_only_files: Vec::new(),
                commands: vec!["git status --short".to_string()],
                success_criteria: vec!["Receipt is persisted.".to_string()],
                stop_conditions: vec!["Stop if command is unsafe.".to_string()],
            }],
        },
    )?;

    assert_eq!(queued, 0);
    assert!(state.queue.is_empty());
    assert!(state.progress_action_completed_this_step);
    let receipts = recent_rust_evidence_job_receipts(&paths, 5);
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].action_id, "EVIDENCE-001");
    assert_eq!(receipts[0].command_count, 1);
    let cost = read_text(&paths.cost)?;
    assert!(cost.contains("senior_progress_rust_evidence_job"));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn foreman_progress_rust_commit_job_commits_selected_allowed_files() -> Result<()> {
    let root = temp_root("foreman-progress-rust-commit");
    let paths = Paths::new(&root)?;
    init_repo(&paths, "test foreman rust commit job")?;
    write_text(&root.join("README.md"), "before\n")?;
    write_text(&root.join("SCRATCH.md"), "scratch before\n")?;
    init_git_repo(&root)?;
    write_text(&root.join("README.md"), "before\nsenior selected commit\n")?;
    write_text(&root.join("SCRATCH.md"), "scratch before\nleft dirty\n")?;
    let cfg = Config::default();
    let mut state = State::default();

    let queued = apply_senior_batch_approval(
        &paths,
        &cfg,
        &mut state,
        &[],
        SeniorBatchApprovalOutput {
            approved: false,
            batch_id: "progress-commit-batch".to_string(),
            task_ids: Vec::new(),
            reason_code: "no_safe_batch".to_string(),
            max_tasks: 0,
            allowed_kinds: Vec::new(),
            forbidden_scope: Vec::new(),
            requires_stop_on: Vec::new(),
            verification_required: Vec::new(),
            auto_accept_policy: "low_risk_verified_only".to_string(),
            needs_planner: false,
            needs_master: false,
            summary: "Commit the bounded dirty README slice.".to_string(),
            warnings: Vec::new(),
            progress_actions: vec![SeniorProgressAction {
                action_id: "SENIOR-COMMIT-001".to_string(),
                kind: SeniorProgressActionKind::RustCommitJob,
                title: "Commit README slice".to_string(),
                summary: "README is already dirty and verified.".to_string(),
                commit_message: "Commit Senior selected README slice".to_string(),
                allowed_files: vec!["README.md".to_string()],
                read_only_files: Vec::new(),
                commands: vec!["rg -n 'senior selected commit' README.md".to_string()],
                success_criteria: vec!["README commit exists.".to_string()],
                stop_conditions: vec!["Stop if unrelated files would be staged.".to_string()],
            }],
        },
    )?;

    assert_eq!(queued, 0);
    assert!(state.progress_action_completed_this_step);
    assert_eq!(
        head_commit_subject(&root)?,
        "Commit Senior selected README slice"
    );
    let files = head_commit_files(&root)?;
    assert!(files.lines().any(|line| line == "README.md"));
    assert!(!files.lines().any(|line| line == "SCRATCH.md"));
    let scratch_status = Command::new("git")
        .args(["status", "--short", "--", "SCRATCH.md"])
        .current_dir(&root)
        .output()?;
    assert!(String::from_utf8_lossy(&scratch_status.stdout).contains("SCRATCH.md"));
    let events = read_text(&paths.events)?;
    assert!(events.contains("role_authorized_rust_commit"));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn manual_foreman_commit_pending_stops_before_batch_and_queue_mutation() -> Result<()> {
    crate::verification::isolation::enable_test_isolation_backend();
    let root = temp_root("manual-foreman-pending-stops-cycle");
    fs::create_dir_all(&root)?;
    write_text(&root.join("README.md"), "before\n")?;
    write_text(
        &root.join("NEXT_STEPS.md"),
        r#"
### TASK-302 - Tiny Continuation
- Task ID: `TASK-302`
- Title: Tiny Continuation
- Objective: Continue only after the explicit commit approval.
- Status: Pending approval.
- Task type: Implementation.
- Allowed files:
  - `src/main.rs`
- Forbidden files/actions:
  - Do not broaden scope.
- Exact steps:
  1. Make the local bounded edit.
- Verification commands:
  - `git diff --check`
- Success criteria:
  - Evidence is recorded.
- Stop conditions:
  - Stop if scope expands.
"#,
    )?;
    let paths = Paths::new(&root)?;
    init_repo(&paths, "test manual Senior pending stop")?;
    init_git_repo(&root)?;
    write_text(&root.join("README.md"), "before\nsenior manual target\n")?;
    let mut cfg = Config::default();
    cfg.commit_mode = CommitMode::Manual;
    let mut state = State::default();
    let candidates =
        senior_batch_candidate_tasks_from_text(&read_text(&root.join("NEXT_STEPS.md"))?);
    assert_eq!(candidates.len(), 1);

    let queued = apply_senior_batch_approval(
        &paths,
        &cfg,
        &mut state,
        &candidates,
        SeniorBatchApprovalOutput {
            approved: true,
            batch_id: "must-not-be-created".to_string(),
            task_ids: vec!["TASK-302".to_string()],
            reason_code: "bounded_commit_then_batch".to_string(),
            max_tasks: 1,
            allowed_kinds: vec!["implementation".to_string()],
            forbidden_scope: Vec::new(),
            requires_stop_on: Vec::new(),
            verification_required: vec!["git diff --check".to_string()],
            auto_accept_policy: "low_risk_verified_only".to_string(),
            needs_planner: false,
            needs_master: false,
            summary: "Commit README, then approve the follow-on batch.".to_string(),
            warnings: Vec::new(),
            progress_actions: vec![SeniorProgressAction {
                action_id: "SENIOR-MANUAL-001".to_string(),
                kind: SeniorProgressActionKind::RustCommitJob,
                title: "Commit README slice".to_string(),
                summary: "README is ready for explicit approval.".to_string(),
                commit_message: "Commit manual Senior README slice".to_string(),
                allowed_files: vec!["README.md".to_string()],
                read_only_files: Vec::new(),
                commands: vec!["rg -n 'senior manual target' README.md".to_string()],
                success_criteria: vec!["README commit exists.".to_string()],
                stop_conditions: vec!["Stop before batch approval.".to_string()],
            }],
        },
    )?;

    assert_eq!(queued, 0);
    let pending = state
        .pending_commit
        .as_ref()
        .expect("Senior commit action should create a manual pending intent");
    assert_eq!(
        pending.source_lane,
        "senior_dev.role_authorized_rust_commit"
    );
    assert_eq!(pending.dirty_paths, vec!["README.md".to_string()]);
    assert_eq!(head_commit_subject(&root)?, "initial");
    assert!(state.senior_batch_approval.is_none());
    assert!(state.queue.is_empty());
    assert!(state.progress_action_completed_this_step);
    let next_steps = read_text(&root.join("NEXT_STEPS.md"))?;
    assert!(next_steps.contains("- Status: Pending approval."));
    assert!(!next_steps.contains("approved by Senior Dev"));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn foreman_progress_rust_commit_job_failed_verification_creates_no_commit() -> Result<()> {
    let root = temp_root("foreman-progress-rust-commit-failed");
    let paths = Paths::new(&root)?;
    init_repo(&paths, "test foreman rust commit job failure")?;
    write_text(&root.join("README.md"), "before\n")?;
    init_git_repo(&root)?;
    write_text(&root.join("README.md"), "before\nchanged but unverified\n")?;
    let cfg = Config::default();
    let mut state = State::default();

    let queued = apply_senior_batch_approval(
        &paths,
        &cfg,
        &mut state,
        &[],
        SeniorBatchApprovalOutput {
            approved: false,
            batch_id: "progress-commit-fail-batch".to_string(),
            task_ids: Vec::new(),
            reason_code: "no_safe_batch".to_string(),
            max_tasks: 0,
            allowed_kinds: Vec::new(),
            forbidden_scope: Vec::new(),
            requires_stop_on: Vec::new(),
            verification_required: Vec::new(),
            auto_accept_policy: "low_risk_verified_only".to_string(),
            needs_planner: false,
            needs_master: false,
            summary: "Try a commit whose verification fails.".to_string(),
            warnings: Vec::new(),
            progress_actions: vec![SeniorProgressAction {
                action_id: "SENIOR-COMMIT-FAIL".to_string(),
                kind: SeniorProgressActionKind::RustCommitJob,
                title: "Commit README slice".to_string(),
                summary: "README is dirty but verification should fail.".to_string(),
                commit_message: "This commit must not happen".to_string(),
                allowed_files: vec!["README.md".to_string()],
                read_only_files: Vec::new(),
                commands: vec!["rg -n 'missing marker' README.md".to_string()],
                success_criteria: Vec::new(),
                stop_conditions: vec!["Stop if verification fails.".to_string()],
            }],
        },
    )?;

    assert_eq!(queued, 0);
    assert_eq!(head_commit_subject(&root)?, "initial");
    assert!(state.planner_due);
    let status = Command::new("git")
        .args(["status", "--short", "--", "README.md"])
        .current_dir(&root)
        .output()?;
    assert!(String::from_utf8_lossy(&status.stdout).contains("README.md"));
    let events = read_text(&paths.events)?;
    assert!(events.contains("senior_progress_rust_commit_rejected"));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn foreman_progress_rust_commit_job_rejects_direct_git_commit_command() -> Result<()> {
    let root = temp_root("foreman-progress-rust-commit-git-command");
    let paths = Paths::new(&root)?;
    init_repo(&paths, "test foreman rust commit command rejection")?;
    write_text(&root.join("README.md"), "before\n")?;
    init_git_repo(&root)?;
    write_text(&root.join("README.md"), "before\nchanged\n")?;
    let cfg = Config::default();
    let mut state = State::default();

    apply_senior_batch_approval(
        &paths,
        &cfg,
        &mut state,
        &[],
        SeniorBatchApprovalOutput {
            approved: false,
            batch_id: "progress-commit-git-command-batch".to_string(),
            task_ids: Vec::new(),
            reason_code: "no_safe_batch".to_string(),
            max_tasks: 0,
            allowed_kinds: Vec::new(),
            forbidden_scope: Vec::new(),
            requires_stop_on: Vec::new(),
            verification_required: Vec::new(),
            auto_accept_policy: "low_risk_verified_only".to_string(),
            needs_planner: false,
            needs_master: false,
            summary: "Reject direct git commands inside Rust commit requests.".to_string(),
            warnings: Vec::new(),
            progress_actions: vec![SeniorProgressAction {
                action_id: "SENIOR-COMMIT-GIT".to_string(),
                kind: SeniorProgressActionKind::RustCommitJob,
                title: "Commit README slice".to_string(),
                summary: "This uses an unsafe direct git command.".to_string(),
                commit_message: "This commit must not happen".to_string(),
                allowed_files: vec!["README.md".to_string()],
                read_only_files: Vec::new(),
                commands: vec!["git commit -m nope".to_string()],
                success_criteria: Vec::new(),
                stop_conditions: vec!["Stop if commands are unsafe.".to_string()],
            }],
        },
    )?;

    assert_eq!(head_commit_subject(&root)?, "initial");
    assert!(state.planner_due);
    let events = read_text(&paths.events)?;
    assert!(events.contains("senior_progress_rust_commit_rejected"));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn foreman_progress_queues_one_bounded_worker_edit_task() -> Result<()> {
    let root = temp_root("foreman-progress-worker-edit");
    fs::create_dir_all(root.join(".orchestrator"))?;
    let paths = Paths::new(&root)?;
    let cfg = Config::default();
    let mut state = State::default();

    let queued = apply_senior_batch_approval(
        &paths,
        &cfg,
        &mut state,
        &[],
        SeniorBatchApprovalOutput {
            approved: false,
            batch_id: "progress-worker-batch".to_string(),
            task_ids: Vec::new(),
            reason_code: "no_safe_batch".to_string(),
            max_tasks: 0,
            allowed_kinds: Vec::new(),
            forbidden_scope: Vec::new(),
            requires_stop_on: Vec::new(),
            verification_required: Vec::new(),
            auto_accept_policy: "low_risk_verified_only".to_string(),
            needs_planner: false,
            needs_master: false,
            summary: "Queue one Worker edit.".to_string(),
            warnings: Vec::new(),
            progress_actions: vec![
                SeniorProgressAction {
                    action_id: "WORK-001".to_string(),
                    kind: SeniorProgressActionKind::WorkerEditTask,
                    title: "Bounded README Edit".to_string(),
                    summary: "Make one bounded README edit.".to_string(),
                    commit_message: String::new(),
                    allowed_files: vec!["README.md".to_string()],
                    read_only_files: Vec::new(),
                    commands: vec!["git diff --check".to_string()],
                    success_criteria: vec!["README.md has the note.".to_string()],
                    stop_conditions: vec!["Stop if source edits are needed.".to_string()],
                },
                SeniorProgressAction {
                    action_id: "WORK-002".to_string(),
                    kind: SeniorProgressActionKind::WorkerEditTask,
                    title: "Second Edit".to_string(),
                    summary: "This should not queue in the same worktree cycle.".to_string(),
                    commit_message: String::new(),
                    allowed_files: vec!["CHANGELOG.md".to_string()],
                    read_only_files: Vec::new(),
                    commands: vec!["git diff --check".to_string()],
                    success_criteria: Vec::new(),
                    stop_conditions: Vec::new(),
                },
            ],
        },
    )?;

    assert_eq!(queued, 1);
    assert_eq!(state.queue.len(), 1);
    assert_eq!(state.queue[0].task_id, "WORK-001");
    let events = read_text(&paths.events)?;
    assert!(events.contains("senior_progress_worker_task_queued"));
    assert!(events.contains("only_one_worker_edit_task_per_single_worktree_cycle"));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn dirty_tree_candidate_is_bounded_to_dirty_source_and_test_files() -> Result<()> {
    let root = temp_root("dirty-tree-candidate");
    fs::create_dir_all(root.join("Sources/App"))?;
    fs::create_dir_all(root.join("Tests/AppTests"))?;
    fs::create_dir_all(root.join(".orchestrator"))?;
    write_text(&root.join("Sources/App/AppCoordinator.swift"), "old\n")?;
    write_text(
        &root.join("Tests/AppTests/AppCoordinatorTests.swift"),
        "old\n",
    )?;
    Command::new("git")
        .arg("init")
        .current_dir(&root)
        .output()?;
    Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(&root)
        .output()?;
    Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(&root)
        .output()?;
    Command::new("git")
        .args(["add", "."])
        .current_dir(&root)
        .output()?;
    Command::new("git")
        .args(["commit", "-m", "initial"])
        .current_dir(&root)
        .output()?;
    write_text(&root.join("Sources/App/AppCoordinator.swift"), "new\n")?;
    write_text(
        &root.join("Tests/AppTests/AppCoordinatorTests.swift"),
        "new\n",
    )?;
    let paths = Paths::new(&root)?;
    let state = State {
        last_review_summary: Some("recent AppCoordinator evidence failed".to_string()),
        ..State::default()
    };

    let task = dirty_tree_continuation_candidate(&paths, &state)?
        .expect("dirty source/test files should produce a candidate");

    assert!(task.task_id.starts_with("DIRTY-"));
    assert_eq!(task.kind, "repair");
    assert!(task
        .allowed_files
        .contains(&"Sources/App/AppCoordinator.swift".to_string()));
    assert!(task
        .allowed_files
        .contains(&"Tests/AppTests/AppCoordinatorTests.swift".to_string()));
    assert!(task
        .tests_to_run
        .contains(&"swift test --filter AppCoordinatorTests".to_string()));
    assert!(task.tests_to_run.contains(&"git diff --check".to_string()));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn dirty_tree_candidate_ignores_tracking_docs_for_technical_repair() -> Result<()> {
    let root = temp_root("dirty-tree-candidate-ignores-docs");
    fs::create_dir_all(root.join("Sources/App"))?;
    fs::create_dir_all(root.join(".orchestrator"))?;
    write_text(&root.join("Sources/App/AppCoordinator.swift"), "old\n")?;
    write_text(&root.join("CONTINUITY.md"), "old\n")?;
    init_git_repo(&root)?;
    write_text(&root.join("Sources/App/AppCoordinator.swift"), "new\n")?;
    write_text(&root.join("CONTINUITY.md"), "new\n")?;
    let paths = Paths::new(&root)?;

    let task = dirty_tree_continuation_candidate(&paths, &State::default())?
        .expect("dirty source file should produce a technical candidate");

    assert!(task
        .allowed_files
        .contains(&"Sources/App/AppCoordinator.swift".to_string()));
    assert!(!task.allowed_files.contains(&"CONTINUITY.md".to_string()));
    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn dirty_tree_candidate_is_suppressed_after_preserve_disposition() -> Result<()> {
    let root = temp_root("dirty-tree-preserve-suppresses-candidate");
    fs::create_dir_all(root.join("Sources/App"))?;
    fs::create_dir_all(root.join("Tests/AppTests"))?;
    fs::create_dir_all(root.join(".orchestrator/tasks"))?;
    write_text(&root.join("Sources/App/AppCoordinator.swift"), "old\n")?;
    write_text(
        &root.join("Tests/AppTests/AppCoordinatorTests.swift"),
        "old\n",
    )?;
    write_text(&root.join("NEXT_STEPS.md"), "# Next Steps\n")?;
    init_git_repo(&root)?;
    write_text(&root.join("Sources/App/AppCoordinator.swift"), "new\n")?;
    write_text(
        &root.join("Tests/AppTests/AppCoordinatorTests.swift"),
        "new\n",
    )?;
    let paths = Paths::new(&root)?;
    let cfg = Config::default();
    let mut state = State {
        human_disposition_decision: Some(HumanDispositionDecision {
            decision: "preserve_quarantine_stop".to_string(),
            summary: "preserve the dirty baseline".to_string(),
            decided_at: "2026-06-29T00:00:00Z".to_string(),
        }),
        ..State::default()
    };
    let fake = FakeCodexRunner::new(Vec::new());

    assert!(dirty_tree_continuation_candidate(&paths, &state)?.is_none());
    let queued = try_queue_dirty_tree_technical_continuation(&paths, &cfg, &mut state, &fake)?;

    assert_eq!(queued, 0);
    assert!(state.queue.is_empty());
    assert_eq!(fake.calls.borrow().len(), 0);
    assert!(fs::read_dir(root.join(".orchestrator/tasks"))?
        .next()
        .is_none());
    let next_steps = read_text(&root.join("NEXT_STEPS.md"))?;
    assert!(!next_steps.contains("DIRTY-"));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn preserve_disposition_ignores_stale_approved_dirty_next_steps_work() -> Result<()> {
    let root = temp_root("dirty-tree-preserve-ignores-stale-approved");
    fs::create_dir_all(root.join(".orchestrator"))?;
    write_text(
        &root.join("NEXT_STEPS.md"),
        r#"
### DIRTY-123 - Reconcile Dirty Technical Evidence
- Task ID: `DIRTY-123`
- Title: Reconcile Dirty Technical Evidence
- Objective: Do not run after preserve.
- Status: Approved.
- Task type: repair.
- Allowed files:
  - `Sources/App/AppCoordinator.swift`
- Verification commands:
  - `git diff --check`
- Success criteria:
  - Do not run.
- Stop conditions:
  - Stop.
"#,
    )?;
    let paths = Paths::new(&root)?;
    let mut state = State {
        human_disposition_decision: Some(HumanDispositionDecision {
            decision: "preserve_quarantine_stop".to_string(),
            summary: "preserve the dirty baseline".to_string(),
            decided_at: "2026-06-29T00:00:00Z".to_string(),
        }),
        ..State::default()
    };

    let recovered = recover_approved_next_steps_work(&paths, &mut state)?;

    assert_eq!(recovered, 0);
    assert!(state.queue.is_empty());
    state.queue.push(sample_task(vec!["README.md".to_string()]));
    state.queue[0].task_id = "DIRTY-123".to_string();
    let removed = sync_state_queue_with_next_steps(&paths, &mut state);
    assert_eq!(removed, 1);
    assert!(state.queue.is_empty());

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn dirty_tree_policy_failure_skips_senior_and_durable_writes() -> Result<()> {
    let root = temp_root("dirty-tree-policy-skips-senior");
    fs::create_dir_all(root.join("Sources/SwiftStackerCore"))?;
    fs::create_dir_all(root.join("Tests/SwiftStackerTests"))?;
    fs::create_dir_all(root.join(".orchestrator/tasks"))?;
    write_text(
        &root.join("Sources/SwiftStackerCore/FocusStackRenderer.swift"),
        "old\n",
    )?;
    write_text(
        &root.join("Tests/SwiftStackerTests/FocusStackRendererTests.swift"),
        "old\n",
    )?;
    write_text(&root.join("NEXT_STEPS.md"), "# Next Steps\n")?;
    init_git_repo(&root)?;
    write_text(
        &root.join("Sources/SwiftStackerCore/FocusStackRenderer.swift"),
        "new\n",
    )?;
    write_text(
        &root.join("Tests/SwiftStackerTests/FocusStackRendererTests.swift"),
        "new\n",
    )?;
    let paths = Paths::new(&root)?;
    let cfg = Config::default();
    let mut state = State::default();
    let fake = FakeCodexRunner::new(Vec::new());

    let queued = try_queue_dirty_tree_technical_continuation(&paths, &cfg, &mut state, &fake)?;

    assert_eq!(queued, 0);
    assert!(state.queue.is_empty());
    assert!(state.planner_due);
    assert_eq!(fake.calls.borrow().len(), 0);
    assert!(fs::read_dir(root.join(".orchestrator/tasks"))?
        .next()
        .is_none());
    let next_steps = read_text(&root.join("NEXT_STEPS.md"))?;
    assert!(!next_steps.contains("DIRTY-"));
    assert!(read_text(&paths.events)?.contains("senior_dirty_tree_continuation_precheck"));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn dirty_tree_adoption_pause_writes_report_for_large_mixed_tree() -> Result<()> {
    let root = temp_root("dirty-adoption-pause");
    fs::create_dir_all(root.join("Sources/App"))?;
    fs::create_dir_all(root.join("analysis"))?;
    fs::create_dir_all(root.join("Scripts"))?;
    fs::create_dir_all(root.join(".orchestrator"))?;
    write_text(&root.join("Sources/App/AppCoordinator.swift"), "old\n")?;
    write_text(&root.join("analysis/rejected_hypotheses.md"), "old\n")?;
    init_git_repo(&root)?;

    write_text(&root.join("Sources/App/AppCoordinator.swift"), "new\n")?;
    write_text(&root.join("analysis/rejected_hypotheses.md"), "new\n")?;
    for idx in 0..13 {
        write_text(
            &root.join(format!("Scripts/analyze_candidate_{idx}.py")),
            "print('probe')\n",
        )?;
    }

    let paths = Paths::new(&root)?;
    let mut state = State {
        needs_reconciliation: true,
        ..State::default()
    };

    assert!(pause_for_large_dirty_adoption_if_needed(
        &paths, &mut state
    )?);
    let pause = state
        .human_disposition_required
        .as_ref()
        .expect("dirty adoption should set a pause");
    assert_eq!(pause.source, "rust_dirty_adoption");
    assert!(pause.reason.contains("dirty adoption file"));
    assert!(!human_disposition_pause_can_clear(&paths, &state));

    let report = read_text(&root.join(".orchestrator/observations/dirty_adoption_report.md"))?;
    assert!(report.contains("# Dirty Adoption Report"));
    assert!(report.contains("## Counts By Status"));
    assert!(report.contains("## Modified Tracked Files (capped)"));
    assert!(report.contains("Sources/App/AppCoordinator.swift"));
    assert!(report.contains("analysis/rejected_hypotheses.md"));
    assert!(report.contains("Scripts/analyze_candidate_0.py"));
    assert!(report.contains("does not approve, stage, commit, revert, or quarantine"));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn dirty_adoption_report_classifies_init_scaffold_and_generated_roots() -> Result<()> {
    let root = temp_root("dirty-adoption-report-classes");
    fs::create_dir_all(root.join(".orchestrator/observations"))?;
    let paths = Paths::new(&root)?;
    record_adoption_scaffold(
        &paths,
        &[
            ("ROADMAP.md", false),
            ("NEXT_STEPS.md", true),
            ("NEXT_STEPS_non-ai.md", false),
            ("AGENTS.md", true),
        ],
    )?;
    let snapshot = RepoObservationSnapshot {
        observed_at: "2026-06-29T00:00:00Z".to_string(),
        git_head: Some("abc123".to_string()),
        dirty_hash: "hash".to_string(),
        changed_files: vec![
            ObservedChangedFile {
                path: ".orchestrator/config.toml".to_string(),
                status: "??".to_string(),
                category: "other".to_string(),
            },
            ObservedChangedFile {
                path: "ROADMAP.md".to_string(),
                status: "??".to_string(),
                category: "tracking_doc".to_string(),
            },
            ObservedChangedFile {
                path: "NEXT_STEPS_non-ai.md".to_string(),
                status: "??".to_string(),
                category: "tracking_doc".to_string(),
            },
            ObservedChangedFile {
                path: "AGENTS.md".to_string(),
                status: "??".to_string(),
                category: "doc".to_string(),
            },
            ObservedChangedFile {
                path: "Sources/App.swift".to_string(),
                status: "M".to_string(),
                category: "source".to_string(),
            },
            ObservedChangedFile {
                path: "analysis/codex-wrapper/run/manifest.json".to_string(),
                status: "??".to_string(),
                category: "other".to_string(),
            },
            ObservedChangedFile {
                path: "Benchmarks/Reports/run/suite-summary.json".to_string(),
                status: "??".to_string(),
                category: "other".to_string(),
            },
        ],
    };

    let report = render_dirty_adoption_report(&paths, &snapshot);

    assert!(report.contains("## Counts By Adoption Class"));
    assert!(report.contains("- `orchestrator_init`: 3"));
    assert!(report.contains("- `generated_or_ignored`: 2"));
    assert!(report.contains("- `user_baseline`: 2"));
    let init_section = report
        .split("## Orchestrator Init Files (capped)")
        .nth(1)
        .and_then(|section| section.split("## Generated/Ignored Files (capped)").next())
        .expect("orchestrator init section");
    assert!(init_section.contains("ROADMAP.md"));
    assert!(init_section.contains("NEXT_STEPS_non-ai.md"));
    assert!(init_section.contains(".orchestrator/config.toml"));
    assert!(!init_section.contains("AGENTS.md"));
    let generated_section = report
        .split("## Generated/Ignored Files (capped)")
        .nth(1)
        .expect("generated section");
    assert!(generated_section.contains("analysis/codex-wrapper/run/manifest.json"));
    assert!(generated_section.contains("Benchmarks/Reports/run/suite-summary.json"));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn helicon_adoption_dry_run_pauses_before_role_calls() -> Result<()> {
    let root = temp_root("helicon-adoption-dry-run");
    fs::create_dir_all(root.join("Sources/SwiftStackerCore"))?;
    fs::create_dir_all(root.join("analysis"))?;
    fs::create_dir_all(root.join("Scripts"))?;
    write_text(
        &root.join("Sources/SwiftStackerCore/FocusStackRenderer.swift"),
        "old\n",
    )?;
    write_text(&root.join("AGENTS.md"), "# Agent rules\n")?;
    write_text(
        &root.join("NEXT_STEPS.md"),
        r#"# Next Steps

## DOC-001: Verify Focused Test Commands In TESTING.md

Status: Ready
Priority: High

Allowed files:
- `TESTING.md`
- `CONTINUITY.md`
- `NEXT_STEPS.md`

Tests:
- `swift test`
"#,
    )?;
    init_git_repo(&root)?;

    assert!(!root.join(".orchestrator").exists());
    assert!(!root.join("ROADMAP.md").exists());

    write_text(
        &root.join("Sources/SwiftStackerCore/FocusStackRenderer.swift"),
        "new\n",
    )?;
    write_text(&root.join("analysis/current_frontier.md"), "frontier\n")?;
    for idx in 0..13 {
        write_text(
            &root.join(format!("Scripts/analyze_candidate_{idx}.py")),
            "print('probe')\n",
        )?;
    }

    let paths = Paths::new(&root)?;
    init_repo(&paths, "adopt helicon-shaped repo")?;
    let mut cfg = load_config(&paths)?;
    cfg.live_output = false;
    let mut state = load_state(&paths)?;
    assert!(state.needs_reconciliation);
    let fake = FakeCodexRunner::new(Vec::new());

    let should_continue = step_once_with_runner(&paths, &cfg, &mut state, &fake)?;

    assert!(!should_continue);
    assert!(state.human_disposition_required.is_some());
    assert_eq!(fake.calls.borrow().len(), 0);
    let report = read_text(&root.join(".orchestrator/observations/dirty_adoption_report.md"))?;
    assert!(report.contains("# Dirty Adoption Report"));
    assert!(report.contains("Sources/SwiftStackerCore/FocusStackRenderer.swift"));
    assert!(report.contains("Scripts/analyze_candidate_0.py"));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn senior_dirty_tree_approval_writes_durable_worker_task() -> Result<()> {
    let root = temp_root("dirty-tree-approval-writes-task");
    fs::create_dir_all(root.join(".orchestrator"))?;
    write_text(
        &root.join("NEXT_STEPS.md"),
        r#"
## Active Queue Summary
- Approved Worker task queue:
  - none.
"#,
    )?;
    let paths = Paths::new(&root)?;
    let cfg = Config::default();
    let mut state = State::default();
    let mut task = sample_task(vec![
        "Sources/App/AppCoordinator.swift".to_string(),
        "Tests/AppTests/AppCoordinatorTests.swift".to_string(),
    ]);
    task.task_id = "DIRTY-123".to_string();
    task.title = "Reconcile Dirty Technical Evidence".to_string();
    task.kind = "repair".to_string();
    task.review_tier = ReviewTier::AiReviewOnFailure;
    task.automation_level = AutomationLevel::AiWorkerRustReview;
    task.tests_to_run = vec![
        "swift test --filter AppCoordinatorTests".to_string(),
        "git diff --check".to_string(),
    ];

    let queued = apply_senior_batch_approval(
        &paths,
        &cfg,
        &mut state,
        &[task.clone()],
        SeniorBatchApprovalOutput {
            approved: true,
            batch_id: "dirty-batch".to_string(),
            task_ids: vec![task.task_id.clone()],
            reason_code: "approved_bounded_batch".to_string(),
            max_tasks: 1,
            allowed_kinds: vec!["repair".to_string()],
            forbidden_scope: vec!["production traffic".to_string()],
            requires_stop_on: vec!["scope expansion".to_string()],
            verification_required: task.tests_to_run.clone(),
            auto_accept_policy: "low_risk_verified_only".to_string(),
            needs_planner: false,
            needs_master: false,
            summary: "Senior approves dirty tree repair.".to_string(),
            warnings: Vec::new(),
            progress_actions: Vec::new(),
        },
    )?;

    assert_eq!(queued, 1);
    assert_eq!(state.queue[0].task_id, "DIRTY-123");
    let next_steps = read_text(&root.join("NEXT_STEPS.md"))?;
    assert!(next_steps.contains("`DIRTY-123` is approved by Senior Dev"));
    assert!(next_steps.contains("### DIRTY-123 - Reconcile Dirty Technical Evidence"));
    assert!(next_steps.contains("- Status: Approved."));
    assert!(parse_approved_next_steps_tasks(&next_steps)
        .iter()
        .any(|parsed| parsed.task_id == "DIRTY-123"));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn planner_zero_work_reconciliation_forms_docs_only_candidate() -> Result<()> {
    let root = temp_root("planner-zero-reconcile-candidate");
    fs::create_dir_all(root.join("docs"))?;
    fs::create_dir_all(root.join(".orchestrator"))?;
    write_text(&root.join("ROADMAP.md"), "stale root roadmap\n")?;
    write_text(
        &root.join("NEXT_STEPS_non-ai.md"),
        "stale deterministic queue\n",
    )?;
    write_text(&root.join("NEXT_STEPS.md"), "fresh queue\n")?;
    write_text(&root.join("docs/ROADMAP.md"), "fresh docs roadmap\n")?;
    let paths = Paths::new(&root)?;
    let plan = ProjectManagerOutput {
        project_complete: false,
        needs_master: false,
        summary: "Control docs show a contradiction: root ROADMAP.md and NEXT_STEPS_non-ai.md still describe stale post-TASK-195 state. Reconcile those files to post-TASK-197 before approving new execution.".to_string(),
        work_plan: PlannerWorkPlan::default(),
        tasks: Vec::new(),
        warnings: Vec::new(),
        commit_requests: Vec::new(),
    };

    let task = planner_zero_work_reconciliation_candidate(&paths, &plan)
        .expect("PM stale-doc diagnosis should produce a bounded docs candidate");

    assert!(task.task_id.starts_with("RECONCILE-"));
    assert_eq!(task.kind, "docs_only");
    assert_eq!(
        task.allowed_files,
        vec!["NEXT_STEPS_non-ai.md".to_string(), "ROADMAP.md".to_string()]
    );
    assert_eq!(task.tests_to_run, vec!["git diff --check".to_string()]);
    assert!(task
        .forbidden_actions
        .iter()
        .any(|rule| rule.contains("Do not edit source or test files")));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn pm_control_doc_reconciliation_auto_commits_without_senior_or_worker() -> Result<()> {
    let root = temp_root("pm-control-doc-auto-commit");
    let paths = Paths::new(&root)?;
    init_repo(&paths, "test pm control-doc reconciliation")?;
    init_git_repo(&root)?;
    write_text(
        &paths.next_steps,
        "# NEXT_STEPS\n\nPM reconciled accepted PAR-007 state.\n",
    )?;
    write_text(
        &paths.next_steps_non_ai,
        "# NEXT_STEPS_non-ai\n\nNo deterministic follow-up.\n",
    )?;
    let mut cfg = Config::default();
    cfg.codex_bin = root
        .join("definitely-not-real-codex")
        .to_string_lossy()
        .to_string();
    cfg.live_output = false;
    let mut state = State {
        mission: "test pm control-doc reconciliation".to_string(),
        master_due: false,
        planner_due: true,
        needs_reconciliation: false,
        project_complete: false,
        ..State::default()
    };
    let fake = FakeCodexRunner::new(vec![serde_json::to_string(&ProjectManagerOutput {
        project_complete: false,
        needs_master: false,
        summary: "Control docs were stale after PAR-007. Reconcile NEXT_STEPS.md and NEXT_STEPS_non-ai.md to the accepted state before further work.".to_string(),
        work_plan: PlannerWorkPlan::default(),
        tasks: Vec::new(),
        warnings: Vec::new(),
        commit_requests: Vec::new(),
    })?]);

    let should_continue = step_once_with_runner(&paths, &cfg, &mut state, &fake)?;

    assert!(should_continue);
    assert!(state.queue.is_empty());
    assert!(state
        .last_review_summary
        .as_deref()
        .is_some_and(|summary| summary.contains("without Senior/Worker")));
    let calls = fake.calls.borrow();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].role_name, "planner");
    let show = Command::new("git")
        .args(["show", "--name-only", "--format=", "HEAD"])
        .current_dir(&root)
        .output()?;
    let show = String::from_utf8_lossy(&show.stdout);
    assert!(show.lines().any(|line| line == "NEXT_STEPS.md"));
    assert!(show.lines().any(|line| line == "NEXT_STEPS_non-ai.md"));
    assert!(!read_text(&paths.next_steps)?.contains("RECONCILE-"));
    let cost = read_text(&paths.cost)?;
    assert!(cost.contains("\"role\":\"foreman-review\""));
    assert!(cost.contains("\"role\":\"worker\""));
    assert!(cost.contains("pm_control_doc_reconciliation_auto_committed"));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn pm_mutated_control_docs_zero_tasks_auto_commits_without_worker() -> Result<()> {
    let root = temp_root("pm-mutated-control-doc-auto-commit");
    let paths = Paths::new(&root)?;
    init_repo(&paths, "test pm mutating control docs")?;
    init_git_repo(&root)?;
    let mut cfg = Config::default();
    cfg.codex_bin = root
        .join("definitely-not-real-codex")
        .to_string_lossy()
        .to_string();
    cfg.live_output = false;
    let mut state = State {
        mission: "test pm mutating control docs".to_string(),
        master_due: false,
        planner_due: true,
        needs_reconciliation: false,
        project_complete: false,
        ..State::default()
    };
    let response = serde_json::to_string(&ProjectManagerOutput {
        project_complete: false,
        needs_master: false,
        summary: "Control docs were stale. Reconcile NEXT_STEPS.md before further work."
            .to_string(),
        work_plan: PlannerWorkPlan::default(),
        tasks: Vec::new(),
        warnings: Vec::new(),
        commit_requests: Vec::new(),
    })?;
    let runner = MutatingCodexRunner::new(
        response,
        paths.next_steps.clone(),
        "# NEXT_STEPS\n\nPM reconciled the queue.\n".to_string(),
    );

    let should_continue = step_once_with_runner(&paths, &cfg, &mut state, &runner)?;

    assert!(should_continue);
    assert!(state.queue.is_empty());
    assert_eq!(runner.calls.borrow().len(), 1);
    assert_eq!(runner.calls.borrow()[0].role_name, "planner");
    let show = Command::new("git")
        .args(["show", "--name-only", "--format=", "HEAD"])
        .current_dir(&root)
        .output()?;
    assert!(String::from_utf8_lossy(&show.stdout)
        .lines()
        .any(|line| line == "NEXT_STEPS.md"));
    let events = read_text(&paths.events)?;
    assert!(events.contains("pm_mutated_files_detected"));
    assert!(events.contains("pm_mutation_auto_committed_control_docs"));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn pm_commit_request_commits_selected_allowed_files_only() -> Result<()> {
    let root = temp_root("pm-rust-commit-selected");
    let paths = Paths::new(&root)?;
    init_repo(&paths, "test pm rust commit request")?;
    write_text(&root.join("README.md"), "before\n")?;
    write_text(&root.join("SCRATCH.md"), "scratch before\n")?;
    init_git_repo(&root)?;
    write_text(&root.join("README.md"), "before\npm selected commit\n")?;
    write_text(&root.join("SCRATCH.md"), "scratch before\nleft dirty\n")?;
    let mut cfg = Config::default();
    cfg.codex_bin = root
        .join("definitely-not-real-codex")
        .to_string_lossy()
        .to_string();
    cfg.live_output = false;
    let mut state = State {
        mission: "test pm rust commit request".to_string(),
        master_due: false,
        planner_due: true,
        needs_reconciliation: false,
        project_complete: false,
        ..State::default()
    };
    let response = serde_json::to_string(&ProjectManagerOutput {
        project_complete: false,
        needs_master: false,
        summary: "Commit the selected README slice and leave unrelated dirty files alone."
            .to_string(),
        work_plan: PlannerWorkPlan::default(),
        tasks: Vec::new(),
        warnings: Vec::new(),
        commit_requests: vec![RustCommitRequest {
            request_id: "PM-COMMIT-001".to_string(),
            title: "Commit README slice".to_string(),
            summary: "README is already dirty and verified.".to_string(),
            allowed_files: vec!["README.md".to_string()],
            verification_commands: vec!["rg -n 'pm selected commit' README.md".to_string()],
            commit_message: "Commit PM selected README slice".to_string(),
            stop_conditions: vec!["Stop if unrelated files would be staged.".to_string()],
        }],
    })?;
    let runner = FakeCodexRunner::new(vec![response]);

    let should_continue = step_once_with_runner(&paths, &cfg, &mut state, &runner)?;

    assert!(should_continue);
    assert_eq!(
        head_commit_subject(&root)?,
        "Commit PM selected README slice"
    );
    let files = head_commit_files(&root)?;
    assert!(files.lines().any(|line| line == "README.md"));
    assert!(!files.lines().any(|line| line == "SCRATCH.md"));
    let scratch_status = Command::new("git")
        .args(["status", "--short", "--", "SCRATCH.md"])
        .current_dir(&root)
        .output()?;
    assert!(String::from_utf8_lossy(&scratch_status.stdout).contains("SCRATCH.md"));
    let events = read_text(&paths.events)?;
    assert!(events.contains("pm_role_authorized_rust_commit"));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn pm_commit_request_failed_verification_creates_no_commit() -> Result<()> {
    let root = temp_root("pm-rust-commit-failed-verification");
    let paths = Paths::new(&root)?;
    init_repo(&paths, "test pm rust commit request rejection")?;
    write_text(&root.join("README.md"), "before\n")?;
    init_git_repo(&root)?;
    write_text(&root.join("README.md"), "before\nno matching marker\n")?;
    let mut cfg = Config::default();
    cfg.codex_bin = root
        .join("definitely-not-real-codex")
        .to_string_lossy()
        .to_string();
    cfg.live_output = false;
    let mut state = State {
        mission: "test pm rust commit request rejection".to_string(),
        master_due: false,
        planner_due: true,
        needs_reconciliation: false,
        project_complete: false,
        ..State::default()
    };
    let response = serde_json::to_string(&ProjectManagerOutput {
        project_complete: false,
        needs_master: false,
        summary: "Try a commit with failing verification.".to_string(),
        work_plan: PlannerWorkPlan::default(),
        tasks: Vec::new(),
        warnings: Vec::new(),
        commit_requests: vec![RustCommitRequest {
            request_id: "PM-COMMIT-FAIL".to_string(),
            title: "Commit README slice".to_string(),
            summary: "README is dirty but verification should fail.".to_string(),
            allowed_files: vec!["README.md".to_string()],
            verification_commands: vec!["rg -n 'missing marker' README.md".to_string()],
            commit_message: "This commit must not happen".to_string(),
            stop_conditions: vec!["Stop if verification fails.".to_string()],
        }],
    })?;
    let runner = FakeCodexRunner::new(vec![response]);

    let should_continue = step_once_with_runner(&paths, &cfg, &mut state, &runner)?;

    assert!(!should_continue);
    assert_eq!(head_commit_subject(&root)?, "initial");
    let status = Command::new("git")
        .args(["status", "--short", "--", "README.md"])
        .current_dir(&root)
        .output()?;
    assert!(String::from_utf8_lossy(&status.stdout).contains("README.md"));
    let events = read_text(&paths.events)?;
    assert!(events.contains("pm_role_authorized_rust_commit_rejected"));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn pm_source_edits_are_flagged_and_do_not_proceed_to_worker() -> Result<()> {
    let root = temp_root("pm-source-edit-blocked");
    fs::create_dir_all(root.join("Sources/App"))?;
    write_text(&root.join("Sources/App/App.swift"), "old\n")?;
    let paths = Paths::new(&root)?;
    init_repo(&paths, "test pm source mutation guard")?;
    init_git_repo(&root)?;
    let mut cfg = Config::default();
    cfg.codex_bin = root
        .join("definitely-not-real-codex")
        .to_string_lossy()
        .to_string();
    cfg.live_output = false;
    let mut task = sample_task(vec!["Sources/App/App.swift".to_string()]);
    task.task_id = "PM-EDIT-001".to_string();
    let response = serde_json::to_string(&ProjectManagerOutput {
        project_complete: false,
        needs_master: false,
        summary: "Approve a bounded source edit.".to_string(),
        work_plan: PlannerWorkPlan::default(),
        tasks: vec![task],
        warnings: Vec::new(),
        commit_requests: Vec::new(),
    })?;
    let runner = MutatingCodexRunner::new(
        response,
        root.join("Sources/App/App.swift"),
        "old\npm touched source\n".to_string(),
    );
    let mut state = State {
        mission: "test pm source mutation guard".to_string(),
        master_due: false,
        planner_due: true,
        needs_reconciliation: false,
        project_complete: false,
        ..State::default()
    };

    let should_continue = step_once_with_runner(&paths, &cfg, &mut state, &runner)?;

    assert!(!should_continue);
    assert!(state.queue.is_empty());
    assert!(state
        .last_review_summary
        .as_deref()
        .is_some_and(|summary| summary.contains("Project Manager mutated")));
    assert_eq!(runner.calls.borrow().len(), 1);
    assert_eq!(runner.calls.borrow()[0].role_name, "planner");
    let events = read_text(&paths.events)?;
    assert!(events.contains("pm_mutated_files_detected"));
    let cost = read_text(&paths.cost)?;
    assert!(cost.contains("pm_mutated_files_stopped_before_worker"));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn approved_backlog_skips_pm_even_when_planner_due_is_stale() -> Result<()> {
    let root = temp_root("approved-backlog-skips-stale-pm");
    write_text(&root.join("README.md"), "readme\n")?;
    write_text(
        &root.join("NEXT_STEPS.md"),
        r#"# NEXT_STEPS

### TASK-401 - Touch Readme
- Task ID: `TASK-401`
- Title: Touch Readme
- Objective: Make a bounded README note.
- Status: Approved.
- Task type: docs_only.
- Review tier: auto_accept_if_verified
- Automation level: ai_worker_rust_review
- Allowed files:
  - `README.md`
- Forbidden files/actions:
  - Do not edit source files.
- Exact steps:
  1. Update README.md.
- Verification commands:
  - `git diff --check`
- Success criteria:
  - README.md is updated.
- Stop conditions:
  - Stop if source edits are needed.
"#,
    )?;
    let paths = Paths::new(&root)?;
    init_repo(&paths, "test stale planner bypass")?;
    init_git_repo(&root)?;
    let mut cfg = Config::default();
    cfg.codex_bin = root
        .join("definitely-not-real-codex")
        .to_string_lossy()
        .to_string();
    cfg.live_output = false;
    let worker_response = serde_json::to_string(&WorkerOutput {
        task_id: "TASK-401".to_string(),
        status: "done".to_string(),
        summary: "README note is already bounded.".to_string(),
        files_changed: Vec::new(),
        commands_run: Vec::new(),
        tests_run: Vec::new(),
        handoff_to_rust: Vec::new(),
        orchestration_request: None,
        blocker: String::new(),
        confidence: "high".to_string(),
    })?;
    let fake = FakeCodexRunner::new(vec![worker_response]);
    let mut state = State {
        mission: "test stale planner bypass".to_string(),
        planner_due: true,
        needs_reconciliation: false,
        master_due: false,
        ..State::default()
    };

    let _ = step_once_with_runner(&paths, &cfg, &mut state, &fake)?;

    let calls = fake.calls.borrow();
    assert!(calls.iter().any(|call| call.role_name == "worker"));
    assert!(!calls.iter().any(|call| call.role_name == "planner"));
    let cost = read_text(&paths.cost)?;
    assert!(
        cost.contains("Project Manager skipped because Rust recovered approved NEXT_STEPS.md work")
    );

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn command_only_evidence_task_runs_in_rust_before_worker() -> Result<()> {
    let root = temp_root("mechanistic-command-only-rust-before-worker");
    let paths = Paths::new(&root)?;
    init_repo(&paths, "test mechanistic command handoff")?;
    write_text(
        &root.join("NEXT_STEPS.md"),
        r#"# NEXT_STEPS

### MECH-001 - Capture Command-Only Evidence
- Task ID: `MECH-001`
- Title: Capture Command-Only Evidence
- Objective: Capture command-only evidence by running a safe git status command.
- Why: The command produces evidence without source edits.
- Status: Approved.
- Task type: verification
- Review tier: ai_review_on_failure
- Automation level: ai_worker_rust_review
- Allowed files:
  - `NEXT_STEPS.md`
- Forbidden files/actions:
  - Do not edit source files.
- Exact steps:
  1. Run the command-only evidence recipe.
- Verification commands:
  - `git status --short`
- Success criteria:
  - Rust evidence receipt is persisted.
- Stop conditions:
  - Stop if the command is unsafe.
"#,
    )?;
    init_git_repo(&root)?;
    let mut cfg = Config::default();
    cfg.live_output = false;
    let fake = FakeCodexRunner::new(Vec::new());
    let mut state = State {
        mission: "test mechanistic command handoff".to_string(),
        master_due: false,
        planner_due: false,
        needs_reconciliation: false,
        ..State::default()
    };

    let should_continue = step_once_with_runner(&paths, &cfg, &mut state, &fake)?;

    assert!(should_continue);
    assert_eq!(fake.calls.borrow().len(), 0);
    assert!(state.queue.is_empty());
    assert_eq!(state.completed_tasks, 1);
    let receipts = recent_rust_evidence_job_receipts(&paths, 5);
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].action_id, "MECH-001");
    let cost = read_text(&paths.cost)?;
    assert!(cost.contains("senior_progress_rust_evidence_job"));
    assert!(cost.contains("mechanistic_handoff_rust_evidence_job"));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn acceptance_contract_blocks_risky_refactor_before_worker() -> Result<()> {
    let root = temp_root("acceptance-contract-block");
    let paths = Paths::new(&root)?;
    init_repo(&paths, "test acceptance contract")?;
    write_text(
        &root.join("NEXT_STEPS.md"),
        r#"# NEXT_STEPS

### RISK-001 - Refactor Parser
- Task ID: `RISK-001`
- Title: Refactor Parser
- Objective: Refactor parser architecture across modules.
- Why: It reduces churn.
- Status: Approved.
- Task type: implementation
- Review tier: ai_review_on_failure
- Automation level: ai_worker_rust_review
- Allowed files:
  - `src/lib.rs`
- Forbidden files/actions:
  - Do not edit outside allowed files.
- Exact steps:
  1. Refactor the parser.
- Verification commands:
  - `cargo test parser`
- Success criteria:
  - Parser tests pass.
- Stop conditions:
  - Stop if scope expands.
"#,
    )?;
    let mut task = sample_task(vec!["src/lib.rs".to_string()]);
    task.task_id = "RISK-001".to_string();
    task.title = "Refactor Parser".to_string();
    task.kind = "implementation".to_string();
    task.objective = "Refactor parser architecture across modules.".to_string();
    task.tests_to_run = vec!["cargo test parser".to_string()];
    task.success_criteria = vec!["Parser tests pass.".to_string()];
    let contract = acceptance_contract_from_task(&task);
    let mut state = State::default();
    let cfg = Config::default();

    let blocked =
        try_block_worker_for_acceptance_contract(&paths, &cfg, &mut state, &task, &contract)?;

    assert_eq!(blocked, Some(false));
    assert!(state.planner_due);
    assert!(read_text(&paths.next_steps)?.contains("- Status: Blocked."));
    assert!(read_text(&paths.cost)?.contains("missing_characterization_contract"));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn source_edit_task_with_validation_commands_still_calls_worker() -> Result<()> {
    let root = temp_root("mechanistic-source-edit-still-worker");
    let paths = Paths::new(&root)?;
    init_repo(&paths, "test source edit stays worker")?;
    write_text(
        &root.join("NEXT_STEPS.md"),
        r#"# NEXT_STEPS

### EDIT-001 - Edit README
- Task ID: `EDIT-001`
- Title: Edit README
- Objective: Edit README with a bounded note.
- Why: This requires a file edit, so Worker should own the edit.
- Status: Approved.
- Task type: docs_only
- Review tier: ai_review_on_failure
- Automation level: ai_worker_rust_review
- Allowed files:
  - `README.md`
- Forbidden files/actions:
  - Do not edit source files.
- Exact steps:
  1. Edit README with a bounded note.
- Verification commands:
  - `git diff --check`
- Success criteria:
  - README is updated.
- Stop conditions:
  - Stop if more files are needed.
"#,
    )?;
    init_git_repo(&root)?;
    let mut cfg = Config::default();
    cfg.live_output = false;
    let worker_response = serde_json::to_string(&WorkerOutput {
        task_id: "EDIT-001".to_string(),
        status: "blocked".to_string(),
        summary: "Worker was called for source/doc edit work.".to_string(),
        files_changed: Vec::new(),
        commands_run: Vec::new(),
        tests_run: Vec::new(),
        handoff_to_rust: Vec::new(),
        orchestration_request: None,
        blocker: "No edit made in test.".to_string(),
        confidence: "high".to_string(),
    })?;
    let fake = FakeCodexRunner::new(vec![worker_response]);
    let mut state = State {
        mission: "test source edit stays worker".to_string(),
        master_due: false,
        planner_due: false,
        needs_reconciliation: false,
        ..State::default()
    };

    let _ = step_once_with_runner(&paths, &cfg, &mut state, &fake)?;

    let calls = fake.calls.borrow();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].role_name, "worker");
    assert_eq!(calls[0].task_id.as_deref(), Some("EDIT-001"));
    assert!(recent_rust_evidence_job_receipts(&paths, 5).is_empty());

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn command_only_task_with_unsafe_command_stops_before_worker() -> Result<()> {
    let root = temp_root("mechanistic-unsafe-command-before-worker");
    let paths = Paths::new(&root)?;
    init_repo(&paths, "test unsafe command handoff rejection")?;
    write_text(
        &root.join("NEXT_STEPS.md"),
        r#"# NEXT_STEPS

### MECH-UNSAFE - Capture Unsafe Command Evidence
- Task ID: `MECH-UNSAFE`
- Title: Capture Unsafe Command Evidence
- Objective: Capture command-only evidence from an unsupported shell command.
- Why: The command should be rejected before Worker.
- Status: Approved.
- Task type: verification
- Review tier: ai_review_on_failure
- Automation level: ai_worker_rust_review
- Allowed files:
  - `NEXT_STEPS.md`
- Forbidden files/actions:
  - Do not edit source files.
- Exact steps:
  1. Run command-only evidence only.
- Verification commands:
  - `swift test --filter Bad && echo nope`
- Success criteria:
  - Rust rejects the unsafe command before Worker.
- Stop conditions:
  - Stop if the command is unsafe.
"#,
    )?;
    init_git_repo(&root)?;
    let mut cfg = Config::default();
    cfg.live_output = false;
    let fake = FakeCodexRunner::new(Vec::new());
    let mut state = State {
        mission: "test unsafe command handoff rejection".to_string(),
        master_due: false,
        planner_due: false,
        needs_reconciliation: false,
        ..State::default()
    };

    let should_continue = step_once_with_runner(&paths, &cfg, &mut state, &fake)?;

    assert!(!should_continue);
    assert_eq!(fake.calls.borrow().len(), 0);
    assert!(state.planner_due);
    assert!(recent_rust_evidence_job_receipts(&paths, 5).is_empty());
    let events = read_text(&paths.events)?;
    assert!(events.contains("mechanistic_handoff_rejected"));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn pm_control_doc_reconciliation_falls_back_without_dirty_named_docs() -> Result<()> {
    let root = temp_root("pm-control-doc-no-dirty-named-docs");
    let paths = Paths::new(&root)?;
    init_repo(&paths, "test pm control-doc fallback")?;
    init_git_repo(&root)?;
    let plan = ProjectManagerOutput {
        project_complete: false,
        needs_master: false,
        summary: "Control docs are stale. Reconcile NEXT_STEPS.md before further work.".to_string(),
        work_plan: PlannerWorkPlan::default(),
        tasks: Vec::new(),
        warnings: Vec::new(),
        commit_requests: Vec::new(),
    };
    let candidate = planner_zero_work_reconciliation_candidate(&paths, &plan)
        .expect("candidate should still be available for Senior fallback");
    let cfg = Config::default();
    let mut state = State::default();

    let handled =
        try_auto_commit_pm_control_doc_reconciliation(&paths, &cfg, &mut state, &candidate)?;

    assert!(!handled);
    assert!(state.last_review_summary.is_none());
    let status = Command::new("git")
        .args(["status", "--short"])
        .current_dir(&root)
        .output()?;
    assert!(String::from_utf8_lossy(&status.stdout).trim().is_empty());

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn pm_control_doc_reconciliation_falls_back_with_non_control_dirty_file() -> Result<()> {
    let root = temp_root("pm-control-doc-non-control-dirty");
    let paths = Paths::new(&root)?;
    init_repo(&paths, "test pm control-doc fallback dirty source")?;
    write_text(&root.join("README.md"), "readme\n")?;
    init_git_repo(&root)?;
    write_text(&paths.next_steps, "# NEXT_STEPS\n\nPM doc edit.\n")?;
    write_text(&root.join("README.md"), "readme\nuser scratch\n")?;
    let plan = ProjectManagerOutput {
        project_complete: false,
        needs_master: false,
        summary: "Control docs are stale. Reconcile NEXT_STEPS.md before further work.".to_string(),
        work_plan: PlannerWorkPlan::default(),
        tasks: Vec::new(),
        warnings: Vec::new(),
        commit_requests: Vec::new(),
    };
    let candidate = planner_zero_work_reconciliation_candidate(&paths, &plan)
        .expect("candidate should still be available for Senior fallback");
    let cfg = Config::default();
    let mut state = State::default();

    let handled =
        try_auto_commit_pm_control_doc_reconciliation(&paths, &cfg, &mut state, &candidate)?;

    assert!(!handled);
    let cached = Command::new("git")
        .args(["diff", "--cached", "--name-only"])
        .current_dir(&root)
        .output()?;
    assert!(String::from_utf8_lossy(&cached.stdout).trim().is_empty());
    let status = Command::new("git")
        .args(["status", "--short"])
        .current_dir(&root)
        .output()?;
    let status = String::from_utf8_lossy(&status.stdout);
    assert!(status.contains("NEXT_STEPS.md"));
    assert!(status.contains("README.md"));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn control_doc_detection_accepts_uppercase_docs_paths() -> Result<()> {
    let root = temp_root("uppercase-docs-control");
    fs::create_dir_all(root.join("Docs"))?;
    fs::create_dir_all(root.join(".orchestrator"))?;
    write_text(&root.join("Docs/ROADMAP.md"), "docs roadmap\n")?;
    write_text(&root.join("Docs/.INDEX.md"), "index\n")?;
    let paths = Paths::new(&root)?;

    let files = mentioned_control_doc_paths(
        &paths,
        "Reconcile Docs/ROADMAP.md and Docs/.INDEX.md before approving execution.",
    );

    assert_eq!(
        files,
        vec!["Docs/.INDEX.md".to_string(), "Docs/ROADMAP.md".to_string()]
    );
    assert!(is_tracking_doc_path("Docs/ROADMAP.md"));
    assert!(is_tracking_doc_path("Docs/.INDEX.md"));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn planner_task_scope_normalization_adds_pm_mentioned_control_docs() -> Result<()> {
    let root = temp_root("planner-scope-control-docs");
    fs::create_dir_all(root.join("Docs/exec-plans/active"))?;
    fs::create_dir_all(root.join(".orchestrator"))?;
    write_text(&root.join("CONTINUITY.md"), "before\n")?;
    write_text(&root.join("PARITY.md"), "before\n")?;
    write_text(&root.join("NEXT_STEPS.md"), "before\n")?;
    write_text(&root.join("Docs/exec-plans/active/phase-3.md"), "before\n")?;
    let paths = Paths::new(&root)?;
    let mut task = sample_task(vec![
        "Benchmarks/ReportsDiagnostics/phase-3-review/**".to_string()
    ]);
    task.task_id = "PAR-011".to_string();
    task.kind = "docs_only".to_string();
    task.title = "Classify repeated evidence blocker".to_string();
    task.objective =
        "Reconcile the repeated evidence blocker from existing report roots.".to_string();
    let mut plan = ProjectManagerOutput {
        project_complete: false,
        needs_master: false,
        summary: "Rust/Worker should later update CONTINUITY.md, PARITY.md, NEXT_STEPS.md, and Docs/exec-plans/active/phase-3.md; no source files should change.".to_string(),
        work_plan: PlannerWorkPlan::default(),
        tasks: vec![task],
        warnings: Vec::new(),
        commit_requests: Vec::new(),
    };
    let mut state = State::default();

    normalize_planner_task_control_doc_scopes(&paths, &mut state, &mut plan);

    let task = &plan.tasks[0];
    for expected in [
        "CONTINUITY.md",
        "PARITY.md",
        "NEXT_STEPS.md",
        "Docs/exec-plans/active/phase-3.md",
    ] {
        assert!(
            task.allowed_files.iter().any(|path| path == expected),
            "missing inferred allowed file {expected}; got {:?}",
            task.allowed_files
        );
    }
    assert!(state
        .recent_events
        .iter()
        .any(|event| event.kind == "planner-task-scope-normalized"));

    let cfg = Config::default();
    let before = snapshot_repo(&paths, &cfg)?;
    write_text(&root.join("CONTINUITY.md"), "after\n")?;
    write_text(&root.join("PARITY.md"), "after\n")?;
    write_text(&root.join("NEXT_STEPS.md"), "after\n")?;
    write_text(&root.join("Docs/exec-plans/active/phase-3.md"), "after\n")?;
    let after = snapshot_repo(&paths, &cfg)?;
    let guard = build_guard_report(&cfg, task, &before, &after)?;
    assert!(guard.violations.is_empty(), "{guard:?}");

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn planner_task_scope_normalization_does_not_broaden_plain_source_task() -> Result<()> {
    let root = temp_root("planner-scope-source-not-broadened");
    fs::create_dir_all(root.join("Sources/App"))?;
    fs::create_dir_all(root.join(".orchestrator"))?;
    write_text(&root.join("CONTINUITY.md"), "before\n")?;
    write_text(&root.join("Sources/App/App.swift"), "before\n")?;
    let paths = Paths::new(&root)?;
    let mut task = sample_task(vec!["Sources/App/App.swift".to_string()]);
    task.task_id = "SRC-001".to_string();
    task.kind = "implementation".to_string();
    task.title = "Implement source slice".to_string();
    task.objective = "Change the source behavior only.".to_string();
    let mut plan = ProjectManagerOutput {
        project_complete: false,
        needs_master: false,
        summary: "Later bookkeeping may update CONTINUITY.md after acceptance.".to_string(),
        work_plan: PlannerWorkPlan::default(),
        tasks: vec![task],
        warnings: Vec::new(),
        commit_requests: Vec::new(),
    };
    let mut state = State::default();

    normalize_planner_task_control_doc_scopes(&paths, &mut state, &mut plan);

    assert!(!plan.tasks[0]
        .allowed_files
        .iter()
        .any(|path| path == "CONTINUITY.md"));
    assert!(state.recent_events.is_empty());

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn planner_human_dirty_disposition_request_pauses_without_ceo_bounce() {
    let plan = ProjectManagerOutput {
        project_complete: false,
        needs_master: true,
        summary: "Master/human should choose one explicit disposition for the quarantined dirty set; both queues should remain empty until disposition is decided.".to_string(),
        work_plan: PlannerWorkPlan::default(),
        tasks: Vec::new(),
        warnings: Vec::new(),
        commit_requests: Vec::new(),
    };

    assert!(planner_requests_human_disposition_pause(&plan));
    let pause = human_disposition_pause_from_planner(&plan);
    assert_eq!(pause.source, "project_manager");
    assert!(pause
        .options
        .iter()
        .any(|option| option.contains("approve a new explicit safety gate")));

    let normal_plan = ProjectManagerOutput {
        project_complete: false,
        needs_master: false,
        summary: "Control docs are stale; reconcile ROADMAP.md.".to_string(),
        work_plan: PlannerWorkPlan::default(),
        tasks: Vec::new(),
        warnings: Vec::new(),
        commit_requests: Vec::new(),
    };

    assert!(!planner_requests_human_disposition_pause(&normal_plan));
}

#[test]
fn human_disposition_pause_clears_only_when_active_work_exists() -> Result<()> {
    let root = temp_root("human-disposition-clear");
    fs::create_dir_all(root.join(".orchestrator"))?;
    let paths = Paths::new(&root)?;
    let mut state = State {
        mission: "test".to_string(),
        human_disposition_required: Some(HumanDispositionPause {
            reason: "dirty set requires human choice".to_string(),
            options: Vec::new(),
            source: "project_manager".to_string(),
        }),
        ..State::default()
    };

    assert!(!human_disposition_pause_can_clear(&paths, &state));

    state.retry_worker_same_task = Some("TASK-1".to_string());
    assert!(human_disposition_pause_can_clear(&paths, &state));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn human_disposition_gate_choice_records_decision_and_routes_to_pm() -> Result<()> {
    let root = temp_root("human-disposition-gate-choice");
    fs::create_dir_all(root.join(".orchestrator"))?;
    let paths = Paths::new(&root)?;
    let pause = HumanDispositionPause {
        reason: "dirty set requires explicit gate".to_string(),
        options: Vec::new(),
        source: "project_manager".to_string(),
    };
    let mut state = State {
        mission: "test".to_string(),
        human_disposition_required: Some(pause.clone()),
        ..State::default()
    };

    let should_continue = apply_human_disposition_choice(
        &paths,
        &mut state,
        HumanDispositionChoice::ApproveGate,
        &pause,
    )?;

    assert!(should_continue);
    assert!(state.human_disposition_required.is_none());
    assert!(state.planner_due);
    assert_eq!(
        state
            .human_disposition_decision
            .as_ref()
            .map(|decision| decision.decision.as_str()),
        Some("approve_explicit_gate")
    );
    assert!(read_text(&paths.events)?.contains("human_disposition"));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn human_disposition_recheck_command_records_decision_without_role_call() -> Result<()> {
    let root = temp_root("human-disposition-command-recheck");
    fs::create_dir_all(root.join(".orchestrator"))?;
    let paths = Paths::new(&root)?;
    save_state(
        &paths,
        &State {
            mission: "test".to_string(),
            needs_reconciliation: true,
            human_disposition_required: Some(HumanDispositionPause {
                reason: "dirty adoption needs a human".to_string(),
                options: Vec::new(),
                source: "rust_dirty_adoption".to_string(),
            }),
            ..State::default()
        },
    )?;

    disposition(root.clone(), "recheck".to_string(), false, false)?;

    let state = load_state(&Paths::new(&root)?)?;
    assert!(state.human_disposition_required.is_none());
    assert!(state.planner_due);
    assert!(!state.master_due);
    assert_eq!(state.completed_tasks, 0);
    assert_eq!(
        state
            .human_disposition_decision
            .as_ref()
            .map(|decision| decision.decision.as_str()),
        Some("recheck_after_external_handling")
    );
    assert!(read_text(&paths.events)?.contains("human_disposition"));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn guarded_human_disposition_rejects_stale_fingerprint_without_mutation() -> Result<()> {
    let root = temp_root("human-disposition-guarded");
    fs::create_dir_all(&root)?;
    write_text(&root.join("README.md"), "baseline\n")?;
    init_git_repo(&root)?;
    let paths = Paths::new(&root)?;
    let pause = HumanDispositionPause {
        reason: "review this exact pause".to_string(),
        options: vec!["recheck".to_string()],
        source: "project_manager".to_string(),
    };
    save_state(
        &paths,
        &State {
            mission: "test".to_string(),
            human_disposition_required: Some(pause.clone()),
            ..State::default()
        },
    )?;
    let revision = crate::revision::repository_revision(&paths)?;
    let fingerprint = crate::revision::human_disposition_pause_fingerprint(&paths, &pause)?;

    let error = disposition_with_preconditions(
        root.clone(),
        "recheck".to_string(),
        false,
        true,
        Some(revision.clone()),
        Some("0".repeat(64)),
    )
    .unwrap_err();
    assert!(error.to_string().contains("fingerprint is stale"));
    let unchanged = load_state(&paths)?;
    assert_eq!(
        unchanged.human_disposition_required.as_ref().map(|active| (
            &active.reason,
            &active.options,
            &active.source
        )),
        Some((&pause.reason, &pause.options, &pause.source))
    );
    assert!(unchanged.human_disposition_decision.is_none());

    disposition_with_preconditions(
        root.clone(),
        "recheck".to_string(),
        false,
        false,
        Some(revision),
        Some(fingerprint),
    )?;
    let applied = load_state(&paths)?;
    assert!(applied.human_disposition_required.is_none());
    assert_eq!(
        applied
            .human_disposition_decision
            .as_ref()
            .map(|decision| decision.decision.as_str()),
        Some("recheck_after_external_handling")
    );

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn human_disposition_keep_paused_refreshes_legacy_dirty_report_without_clearing_pause() -> Result<()>
{
    let root = temp_root("human-disposition-refresh-report");
    fs::create_dir_all(root.join(".orchestrator/observations"))?;
    fs::create_dir_all(root.join("Sources"))?;
    fs::create_dir_all(root.join("analysis/codex-wrapper/run"))?;
    write_text(&root.join("Sources/App.swift"), "old\n")?;
    init_git_repo(&root)?;
    write_text(
        &root.join(".orchestrator/state.json"),
        &serde_json::to_string_pretty(&State {
            mission: "test".to_string(),
            needs_reconciliation: true,
            human_disposition_required: Some(HumanDispositionPause {
                reason: "dirty adoption needs a human".to_string(),
                options: Vec::new(),
                source: "rust_dirty_adoption".to_string(),
            }),
            ..State::default()
        })?,
    )?;
    write_text(&root.join(".orchestrator/config.toml"), "config\n")?;
    write_text(&root.join("ROADMAP.md"), "init or user?\n")?;
    write_text(&root.join("NEXT_STEPS_non-ai.md"), "init or user?\n")?;
    write_text(
        &root.join("analysis/codex-wrapper/run/manifest.json"),
        "{}\n",
    )?;
    write_text(&root.join("Sources/App.swift"), "new\n")?;

    disposition(root.clone(), "keep-paused".to_string(), true, false)?;

    let paths = Paths::new(&root)?;
    let state = load_state(&paths)?;
    assert_eq!(
        state
            .human_disposition_required
            .as_ref()
            .map(|pause| pause.source.as_str()),
        Some("rust_dirty_adoption")
    );
    assert!(state.human_disposition_decision.is_none());
    assert!(!state.planner_due);
    assert!(!state.master_due);
    let report = read_text(&root.join(".orchestrator/observations/dirty_adoption_report.md"))?;
    assert!(report.contains("## Counts By Adoption Class"));
    assert!(
        report.contains("- `orchestrator_init`: 2"),
        "unexpected adoption class counts:\n{report}"
    );
    assert!(report.contains("- `legacy_unknown`: 2"));
    assert!(report.contains("- `generated_or_ignored`: 1"));
    assert!(report.contains("- `user_baseline`: 1"));
    assert!(report.contains(".orchestrator/config.toml"));
    assert!(report.contains(".orchestrator/state.json"));
    assert!(report.contains("ROADMAP.md"));
    assert!(report.contains("NEXT_STEPS_non-ai.md"));
    assert!(report.contains("analysis/codex-wrapper/run/manifest.json"));
    assert!(read_text(&paths.events)?.contains("dirty_adoption_report_refreshed"));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn human_disposition_refresh_rejects_non_adoption_pause() -> Result<()> {
    let root = temp_root("human-disposition-refresh-non-adoption");
    fs::create_dir_all(root.join(".orchestrator"))?;
    let paths = Paths::new(&root)?;
    save_state(
        &paths,
        &State {
            mission: "test".to_string(),
            human_disposition_required: Some(HumanDispositionPause {
                reason: "project manager pause".to_string(),
                options: Vec::new(),
                source: "project_manager".to_string(),
            }),
            ..State::default()
        },
    )?;

    let err = disposition(root.clone(), "keep-paused".to_string(), true, false)
        .expect_err("refresh should reject non-adoption pauses");
    assert!(err.to_string().contains("rust_dirty_adoption"));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn human_disposition_refresh_rejects_clearing_decision() -> Result<()> {
    let root = temp_root("human-disposition-refresh-clearing");
    fs::create_dir_all(root.join(".orchestrator"))?;
    let paths = Paths::new(&root)?;
    save_state(
        &paths,
        &State {
            mission: "test".to_string(),
            human_disposition_required: Some(HumanDispositionPause {
                reason: "dirty adoption needs a human".to_string(),
                options: Vec::new(),
                source: "rust_dirty_adoption".to_string(),
            }),
            ..State::default()
        },
    )?;

    let err = disposition(root.clone(), "preserve".to_string(), true, false)
        .expect_err("refresh should not combine with a clearing decision");
    assert!(err.to_string().contains("keep-paused"));
    let state = load_state(&paths)?;
    assert!(state.human_disposition_required.is_some());

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn mlx_lm_role_dispatch_fails_closed_when_disabled() -> Result<()> {
    let root = temp_root("mlx-lm-dispatch-disabled");
    let paths = Paths::new(&root)?;
    init_repo(&paths, "mlx_lm disabled")?;
    let mut cfg = load_config(&paths)?;
    cfg.mlx_lm.enabled = false;
    cfg.roles.worker.provider = RoleProvider::MlxLm;
    cfg.roles.worker.model = "mlx-community/Qwen3-8B-4bit".to_string();
    cfg.live_output = false;
    let fake = FakeCodexRunner::new(Vec::new());
    let err = run_role(
        &paths,
        &cfg,
        &fake,
        CaRoleAuthority::ReadOnly,
        "worker",
        &cfg.roles.worker,
        Some("T001"),
        "prompt",
        &paths.schemas.join("worker.schema.json"),
    )
    .expect_err("disabled mlx_lm must fail closed");
    assert!(err.to_string().contains("enabled is false"));
    assert!(fake.calls.borrow().is_empty());

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn ollama_role_dispatch_fails_closed_when_disabled() -> Result<()> {
    let root = temp_root("ollama-dispatch-disabled");
    let paths = Paths::new(&root)?;
    init_repo(&paths, "ollama disabled")?;
    let mut cfg = load_config(&paths)?;
    cfg.ollama.enabled = false;
    cfg.roles.worker.provider = RoleProvider::Ollama;
    cfg.roles.worker.model = "qwen2.5-coder:32b".to_string();
    cfg.live_output = false;
    let fake = FakeCodexRunner::new(Vec::new());
    let err = run_role(
        &paths,
        &cfg,
        &fake,
        CaRoleAuthority::ReadOnly,
        "worker",
        &cfg.roles.worker,
        Some("T001"),
        "prompt",
        &paths.schemas.join("worker.schema.json"),
    )
    .expect_err("disabled ollama must fail closed");
    assert!(err.to_string().contains("enabled is false"));
    assert!(fake.calls.borrow().is_empty());

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn ca_role_dispatch_is_injectable_for_every_role_and_never_falls_back() -> Result<()> {
    let root = temp_root("ca-all-role-dispatch");
    let paths = Paths::new(&root)?;
    init_repo(&paths, "injectable ca role dispatch")?;
    let ca_bin_root = temp_root("ca-all-role-dispatch-bin");
    fs::create_dir_all(&ca_bin_root)?;
    let ca_bin = ca_bin_root.join("fake-ca");
    write_text(&ca_bin, "#!/bin/sh\nexit 0\n")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&ca_bin)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&ca_bin, permissions)?;
    }

    let mut cfg = load_config(&paths)?;
    cfg.ca.enabled = true;
    cfg.ca.bin = ca_bin.display().to_string();
    cfg.ca.profile = "codex-team".to_string();
    cfg.ca.route_provider = "openai".to_string();
    cfg.roles.master.provider = RoleProvider::Ca;
    cfg.roles.planner.provider = RoleProvider::Ca;
    cfg.roles.foreman.provider = RoleProvider::Ca;
    cfg.roles.worker.provider = RoleProvider::Ca;

    let codex = FakeCodexRunner::new(Vec::new());
    let ca = FakeCaRunner::succeeding();
    let runners = RoleRunners {
        codex: &codex,
        ca: &ca,
    };
    let worker_allowed = vec!["src/lib.rs".to_string(), "tests/**".to_string()];

    run_role_with_runners(
        &paths,
        &cfg,
        &runners,
        CaRoleAuthority::ReadOnly,
        "master",
        &cfg.roles.master,
        None,
        "master prompt",
        &paths.schemas.join("master.schema.json"),
    )?;
    run_role_with_runners(
        &paths,
        &cfg,
        &runners,
        CaRoleAuthority::ReadOnly,
        "planner",
        &cfg.roles.planner,
        None,
        "planner prompt",
        &paths.schemas.join("planner.schema.json"),
    )?;
    run_role_with_runners(
        &paths,
        &cfg,
        &runners,
        CaRoleAuthority::ReadOnly,
        "foreman-review",
        &cfg.roles.foreman,
        Some("INT-148"),
        "review prompt",
        &paths.schemas.join("foreman_review.schema.json"),
    )?;
    run_role_with_runners(
        &paths,
        &cfg,
        &runners,
        CaRoleAuthority::Worker {
            allowed_files: &worker_allowed,
        },
        "worker",
        &cfg.roles.worker,
        Some("INT-148"),
        "worker prompt",
        &paths.schemas.join("worker.schema.json"),
    )?;

    assert!(codex.calls.borrow().is_empty());
    let calls = ca.calls.borrow();
    assert_eq!(calls.len(), 4);
    assert_eq!(calls[0].role_name, "master");
    assert_eq!(calls[0].task_id, None);
    assert_eq!(calls[0].authority, FakeCaAuthority::ReadOnly);
    assert_eq!(calls[1].role_name, "planner");
    assert_eq!(calls[1].authority, FakeCaAuthority::ReadOnly);
    assert_eq!(calls[2].role_name, "foreman-review");
    assert_eq!(calls[2].task_id.as_deref(), Some("INT-148"));
    assert_eq!(calls[2].authority, FakeCaAuthority::ReadOnly);
    assert_eq!(calls[3].role_name, "worker");
    assert_eq!(calls[3].model, cfg.roles.worker.model);
    assert_eq!(
        calls[3].authority,
        FakeCaAuthority::Worker(worker_allowed.clone())
    );
    assert!(calls.iter().all(|call| call.route_provider == "openai"));
    drop(calls);

    let cost = read_model_call_records(&paths)?;
    assert_eq!(cost.len(), 4);
    assert!(cost.iter().all(|record| {
        record.execution_engine.as_deref() == Some("ca")
            && record.route_provider.as_deref() == Some("openai")
    }));
    assert!(cost.iter().all(|record| record
        .skip_or_call_reason
        .contains("execution_engine=ca route_provider=openai")));
    let health: Vec<ToolHealthRecord> = read_jsonl_file(&tool_health_path(&paths))?;
    assert_eq!(health.len(), 4);
    assert!(health.iter().all(|record| {
        record.status == ToolHealthStatus::Success
            && record.execution_engine.as_deref() == Some("ca")
            && record.route_provider.as_deref() == Some("openai")
    }));

    let failing_ca = FakeCaRunner::failing();
    let failing_runners = RoleRunners {
        codex: &codex,
        ca: &failing_ca,
    };
    let error = run_role_with_runners(
        &paths,
        &cfg,
        &failing_runners,
        CaRoleAuthority::ReadOnly,
        "planner",
        &cfg.roles.planner,
        None,
        "planner prompt",
        &paths.schemas.join("planner.schema.json"),
    )
    .expect_err("a ca failure must propagate without provider substitution");
    assert!(error
        .to_string()
        .contains("ca role execution failed (failure_kind=process_failed"));
    assert!(!error.to_string().contains("fake ca role failure"));
    assert!(codex.calls.borrow().is_empty());

    fs::remove_dir_all(root)?;
    fs::remove_dir_all(ca_bin_root)?;
    Ok(())
}

#[test]
fn mcp_owned_codex_worker_is_rejected_before_provider_traffic() {
    let allowed = vec!["src/lib.rs".to_string()];
    let error = ensure_role_runtime_authority(
        RoleProvider::Codex,
        "workspace-write",
        &CaRoleAuthority::Worker {
            allowed_files: &allowed,
        },
        true,
    )
    .expect_err("MCP-owned Codex Worker must fail closed");
    assert!(error.to_string().contains("configure provider = \"ca\""));

    ensure_role_runtime_authority(
        RoleProvider::Codex,
        "read-only",
        &CaRoleAuthority::ReadOnly,
        true,
    )
    .expect("MCP-owned advisory Codex roles remain read-only compatible");
    ensure_role_runtime_authority(
        RoleProvider::Ca,
        "workspace-write",
        &CaRoleAuthority::Worker {
            allowed_files: &allowed,
        },
        true,
    )
    .expect("MCP-owned ca Worker has exact process-scoped authority");

    let advisory_error = ensure_role_runtime_authority(
        RoleProvider::Codex,
        "workspace-write",
        &CaRoleAuthority::ReadOnly,
        true,
    )
    .expect_err("writable MCP-owned advisory Codex role must fail closed");
    assert!(advisory_error
        .to_string()
        .contains("requires sandbox = \"read-only\""));
}

#[test]
fn orchestrator_role_entrypoints_assign_ca_authority_by_role() -> Result<()> {
    let root = temp_root("ca-role-entrypoint-authority");
    let paths = Paths::new(&root)?;
    init_repo(&paths, "ca role entrypoint authority")?;
    let ca_bin_root = temp_root("ca-role-entrypoint-authority-bin");
    fs::create_dir_all(&ca_bin_root)?;
    let ca_bin = ca_bin_root.join("fake-ca");
    write_text(&ca_bin, "#!/bin/sh\nexit 0\n")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&ca_bin)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&ca_bin, permissions)?;
    }

    let mut cfg = load_config(&paths)?;
    cfg.ca.enabled = true;
    cfg.ca.bin = ca_bin.display().to_string();
    cfg.ca.profile = "codex-team".to_string();
    cfg.ca.route_provider = "openai".to_string();
    for role in [
        &mut cfg.roles.master,
        &mut cfg.roles.planner,
        &mut cfg.roles.foreman,
        &mut cfg.roles.worker,
    ] {
        role.provider = RoleProvider::Ca;
        role.model = "gpt-test".to_string();
    }

    let mut task = sample_task(vec!["src/lib.rs".to_string(), "tests/**".to_string()]);
    task.task_id = "INT-148".to_string();
    task.forbidden_actions = vec!["no network".to_string()];
    task.tests_to_run = vec!["cargo test role_run --no-fail-fast".to_string()];
    let state = load_state(&paths)?;
    let worker_output = sample_worker_output(&task.task_id);
    let ca = FakeCaRunner::with_responses(vec![
        serde_json::to_string(&json!({
            "project_complete": false,
            "needs_planner": false,
            "roadmap_status": "active",
            "summary": "continue",
            "warnings": []
        }))?,
        serde_json::to_string(&json!({
            "project_complete": false,
            "needs_master": false,
            "summary": "planned",
            "work_plan": {},
            "tasks": [],
            "warnings": [],
            "commit_requests": []
        }))?,
        serde_json::to_string(&worker_output)?,
        serde_json::to_string(&json!({
            "task_id": "INT-148",
            "accepted": true,
            "reason_code": "verified",
            "commit_created": false,
            "commit_sha": "",
            "needs_master": false,
            "needs_planner": false,
            "project_complete": false,
            "summary": "accepted",
            "problems": [],
            "replacement_tasks": [],
            "followup_tasks": [],
            "warnings": []
        }))?,
    ]);
    let codex = FakeCodexRunner::new(Vec::new());
    let runners = RoleRunners {
        codex: &codex,
        ca: &ca,
    };

    run_ceo_with_runners(&paths, &cfg, &state, &runners)?;
    run_project_manager_with_runners(&paths, &cfg, &state, &runners)?;
    let support = build_task_support_brief(&paths, &cfg, &task);
    let worker = run_worker_with_runners(&paths, &cfg, &state, &task, &support, None, &runners)?;
    run_senior_dev_review_with_runners(
        &paths,
        &cfg,
        &state,
        &task,
        &worker,
        &clean_guard(Vec::new()),
        json!({"status": "passed"}),
        &runners,
    )?;

    assert!(codex.calls.borrow().is_empty());
    let calls = ca.calls.borrow();
    assert_eq!(
        calls
            .iter()
            .map(|call| call.role_name.as_str())
            .collect::<Vec<_>>(),
        vec!["master", "planner", "worker", "foreman-review"]
    );
    assert!(calls[..2]
        .iter()
        .all(|call| call.authority == FakeCaAuthority::ReadOnly));
    assert_eq!(
        calls[2].authority,
        FakeCaAuthority::Worker(task.allowed_files.clone())
    );
    assert_eq!(calls[3].authority, FakeCaAuthority::ReadOnly);

    fs::remove_dir_all(root)?;
    fs::remove_dir_all(ca_bin_root)?;
    Ok(())
}

#[test]
fn hermes_cycle_receipt_includes_pause_and_cycle_id() {
    let state = State {
        mission: "hermes bridge".to_string(),
        planner_due: true,
        master_due: false,
        human_disposition_required: Some(HumanDispositionPause {
            reason: "needs disposition".to_string(),
            options: vec!["preserve".to_string(), "gate".to_string()],
            source: "planner".to_string(),
        }),
        queue: vec![sample_task(vec!["README.md".to_string()])],
        ..State::default()
    };
    let receipt = receipts::CycleReceipt::from_state(
        "cycle-test".to_string(),
        "/tmp/repo".to_string(),
        "paused",
        &state,
        "/tmp/repo/.orchestrator/events.jsonl".to_string(),
    );
    assert_eq!(receipt.protocol_version, receipts::PROTOCOL_VERSION);
    assert_eq!(receipt.cycle_id, "cycle-test");
    assert_eq!(receipt.outcome, "paused");
    assert!(receipt.human_disposition_required);
    assert_eq!(receipt.queue_len, 1);
    let pause = receipt.pause.as_ref().expect("pause object");
    assert_eq!(pause.source, "planner");
    assert_eq!(pause.options.len(), 2);
    let value = serde_json::to_value(&receipt).expect("serialize");
    assert_eq!(value["protocol_version"], 1);
    assert!(value.get("pause").is_some());
    assert!(value.get("cycle_id").is_some());
}

#[test]
fn hermes_status_receipt_observe_flag_and_queue() {
    let state = State {
        mission: "observe".to_string(),
        project_complete: false,
        queue: vec![sample_task(vec!["a.rs".to_string()])],
        human_disposition_required: None,
        ..State::default()
    };
    let receipt = receipts::StatusReceipt::from_state(
        "/tmp/repo".to_string(),
        &state,
        "/tmp/events".to_string(),
        true,
    );
    assert!(receipt.observe);
    assert_eq!(receipt.queue.len(), 1);
    assert_eq!(receipt.queue[0].task_id, "T001");
    assert!(receipt.human_disposition_required.is_none());
}

#[test]
fn hermes_learning_snapshot_includes_recommended_lane_and_episode_counts() -> Result<()> {
    let root = temp_root("learning-snapshot");
    fs::create_dir_all(root.join(".orchestrator/observations"))?;
    let paths = Paths::new(&root)?;
    append_jsonl(
        &supervisor_episodes_path(&paths),
        &json!({
            "observed_at": "2026-07-14T12:00:00Z",
            "cycle_id": "cyc-1",
            "outcome": "error",
            "error_kind": "provider_unreachable",
            "error_summary": "connection refused",
            "role": "worker",
            "queue_len": 1,
        }),
    )?;
    let mut state = State::default();
    let mut task = sample_task(vec!["docs/HERMES.md".to_string()]);
    task.kind = "docs_only".to_string();
    task.tests_to_run = vec!["git diff --check".to_string()];
    state.queue.push(task);
    let snapshot = build_learning_snapshot(&paths, &state);
    assert_eq!(snapshot.task_id.as_deref(), Some("T001"));
    assert!(snapshot.recommended_lane.is_some());
    assert!(snapshot
        .recent_episode_error_kinds
        .iter()
        .any(|entry| entry.error_kind == "provider_unreachable" && entry.count >= 1));
    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn scorecard_soft_gate_defaults_off_and_pauses_when_enabled() -> Result<()> {
    let root = temp_root("soft-gate");
    fs::create_dir_all(root.join(".orchestrator/observations"))?;
    let paths = Paths::new(&root)?;
    let mut cfg = Config::default();
    assert!(!cfg.routing.honor_scorecard_soft_gate);
    cfg.routing.honor_scorecard_soft_gate = true;
    append_jsonl(
        &tool_health_path(&paths),
        &serde_json::to_value(ToolHealthRecord {
            observed_at: "2026-07-14T12:00:00Z".to_string(),
            role: "worker".to_string(),
            task_id: None,
            model: "test".to_string(),
            execution_engine: None,
            route_provider: None,
            status: ToolHealthStatus::Failure,
            failure_kind: Some(ToolFailureKind::ModelUnavailable),
            exit_code: None,
            timed_out: false,
            excerpt: "unreachable".to_string(),
        })?,
    )?;
    let mut state = State::default();
    let task = sample_task(vec!["src/lib.rs".to_string()]);
    let scorecard =
        route_scorecard_for_task(&paths, &task, ExecutionLane::SharedWorker, "test", None, 0);
    let gated = try_honor_scorecard_soft_gate(&paths, &cfg, &mut state, &task, &scorecard)?;
    assert_eq!(gated, Some(false));
    assert!(state.human_disposition_required.is_some());
    assert_eq!(
        state
            .human_disposition_required
            .as_ref()
            .map(|p| p.source.as_str()),
        Some("scorecard_soft_gate")
    );
    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn hermes_disposition_receipt_outcomes() {
    let mut state = State {
        mission: "disposition".to_string(),
        human_disposition_required: Some(HumanDispositionPause {
            reason: "dirty".to_string(),
            options: vec!["preserve".to_string()],
            source: "rust_dirty_adoption".to_string(),
        }),
        ..State::default()
    };
    let kept = receipts::DispositionReceipt::from_state(
        "/tmp/repo".to_string(),
        "kept_paused",
        "keep-paused",
        true,
        &state,
        "/tmp/events".to_string(),
    );
    assert_eq!(kept.outcome, "kept_paused");
    assert!(kept.human_disposition_required);
    assert!(kept.refresh_report);

    state.human_disposition_required = None;
    let recorded = receipts::DispositionReceipt::from_state(
        "/tmp/repo".to_string(),
        "recorded",
        "recheck",
        false,
        &state,
        "/tmp/events".to_string(),
    );
    assert_eq!(recorded.outcome, "recorded");
    assert!(!recorded.human_disposition_required);
    assert!(recorded.pause.is_none());
}

#[test]
fn hermes_doctor_receipt_healthy_when_all_checks_ok() {
    let receipt = receipts::DoctorReceipt::new(
        "/tmp/repo".to_string(),
        true,
        vec![
            receipts::DoctorCheck::new("git", true, "ok"),
            receipts::DoctorCheck::new("codex", true, "ok"),
        ],
    );
    assert!(receipt.healthy);
    assert_eq!(receipt.checks[0].severity, "ok");
    let unhealthy = receipts::DoctorReceipt::new(
        "/tmp/repo".to_string(),
        true,
        vec![receipts::DoctorCheck::new("git", false, "missing")],
    );
    assert!(!unhealthy.healthy);
    assert_eq!(unhealthy.checks[0].severity, "error");
}

#[test]
fn doctor_skips_codex_when_no_role_uses_codex() -> Result<()> {
    let root = temp_root("doctor-skip-codex");
    fs::create_dir_all(root.join(".orchestrator"))?;
    let paths = Paths::new(&root)?;
    let mut cfg = Config::default();
    cfg.roles.master.provider = RoleProvider::MlxLm;
    cfg.roles.planner.provider = RoleProvider::MlxLm;
    cfg.roles.foreman.provider = RoleProvider::MlxLm;
    cfg.roles.worker.provider = RoleProvider::MlxLm;
    cfg.mlx_lm.enabled = false; // avoid live server requirement for this unit check
    cfg.codex_bin = "codex-definitely-missing-xyz".to_string();
    write_text(&paths.config, &toml::to_string_pretty(&cfg)?)?;
    let receipt = build_doctor_receipt(&paths)?;
    let codex = receipt
        .checks
        .iter()
        .find(|c| c.name == "codex")
        .expect("codex check");
    assert!(
        codex.ok,
        "codex check should be skipped ok: {}",
        codex.detail
    );
    assert!(codex.detail.contains("no role uses provider=codex"));
    fs::remove_dir_all(root)?;
    Ok(())
}

#[cfg(unix)]
#[test]
fn ca_doctor_version_check_is_bounded_and_never_reports_child_output() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let root = temp_root("doctor-ca-version-redaction");
    fs::create_dir_all(&root)?;
    let failing = root.join("fake-ca-version-failure");
    write_text(
        &failing,
        "#!/bin/sh\necho CA_SECRET_CANARY\necho CA_SECRET_CANARY >&2\nexit 7\n",
    )?;
    let mut permissions = fs::metadata(&failing)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&failing, permissions)?;
    let error = run_bounded_ca_version_check_with_timeout(&failing, Duration::from_secs(1))
        .expect_err("nonzero version check should fail");
    let diagnostic = error.to_string();
    assert!(diagnostic.contains("status"));
    assert!(!diagnostic.contains("CA_SECRET_CANARY"));

    let hanging = root.join("fake-ca-version-hang");
    write_text(&hanging, "#!/bin/sh\nsleep 10\n")?;
    let mut permissions = fs::metadata(&hanging)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&hanging, permissions)?;
    let started = Instant::now();
    let error = run_bounded_ca_version_check_with_timeout(&hanging, Duration::from_millis(100))
        .expect_err("hanging version check should time out");
    assert!(error.to_string().contains("timed out"));
    assert!(started.elapsed() < Duration::from_secs(3));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[cfg(unix)]
#[test]
fn doctor_ca_version_receipt_redacts_success_output() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let root = temp_root("doctor-ca-version-success-redaction");
    let paths = Paths::new(&root)?;
    init_repo(&paths, "doctor ca version redaction")?;
    let ca_bin = root.join("fake-ca-doctor");
    write_text(
        &ca_bin,
        "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then\n  echo CA_SECRET_CANARY\n  exit 0\nfi\nprintf '%s\\n' '{\"schema\":\"ca.role_run_preflight.v1\",\"ready\":true,\"profile\":\"codex-team\",\"sandbox_required\":true,\"routes\":[{\"provider\":\"openai\",\"models\":[\"gpt-test\"]}],\"issues\":[]}'\n",
    )?;
    let mut permissions = fs::metadata(&ca_bin)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&ca_bin, permissions)?;

    let mut cfg = load_config(&paths)?;
    cfg.ca.enabled = true;
    cfg.ca.bin = ca_bin.display().to_string();
    cfg.ca.profile = "codex-team".to_string();
    cfg.ca.route_provider = "openai".to_string();
    for role in [
        &mut cfg.roles.master,
        &mut cfg.roles.planner,
        &mut cfg.roles.foreman,
        &mut cfg.roles.worker,
    ] {
        role.provider = RoleProvider::Ca;
        role.model = "gpt-test".to_string();
    }
    save_config(&paths, &cfg)?;

    let receipt = build_doctor_receipt(&paths)?;
    let version = receipt
        .checks
        .iter()
        .find(|check| check.name == "ca_version")
        .expect("ca version check");
    assert!(version.ok, "unexpected detail: {}", version.detail);
    assert!(version.detail.contains("output redacted"));
    assert!(!version.detail.contains("CA_SECRET_CANARY"));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[cfg(unix)]
#[test]
fn codex_doctor_commands_are_bounded_and_receipt_redacts_child_output() -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;

    const CANARY: &str = "CODEX_DOCTOR_SECRET_CANARY_7f31";
    let root = temp_root("doctor-codex-command-safety");
    let paths = Paths::new(&root)?;
    init_repo(&paths, "doctor codex command safety")?;
    let mut cfg = load_config(&paths)?;
    let models = serde_json::to_string(&json!({
        "models": [
            {"slug": cfg.roles.master.model.clone()},
            {"slug": cfg.roles.planner.model.clone()},
            {"slug": cfg.roles.foreman.model.clone()},
            {"slug": cfg.roles.worker.model.clone()},
        ]
    }))?;
    let codex_bin = root.join("codex-fake-doctor");
    write_text(
        &codex_bin,
        &format!(
            "#!/bin/sh\n\
             if [ \"$1\" = '--version' ]; then\n\
               printf '%s\\n' '{CANARY}'\n\
               printf '%s\\n' '{CANARY}' >&2\n\
               exit 0\n\
             fi\n\
             if [ \"$1\" = 'debug' ] && [ \"$2\" = 'models' ]; then\n\
               printf '%s\\n' '{models}'\n\
               printf '%s\\n' '{CANARY}' >&2\n\
               exit 0\n\
             fi\n\
             exit 9\n"
        ),
    )?;
    let mut permissions = fs::metadata(&codex_bin)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&codex_bin, permissions)?;
    cfg.codex_bin = codex_bin.display().to_string();
    save_config(&paths, &cfg)?;

    let receipt = build_doctor_receipt(&paths)?;
    let receipt_json = serde_json::to_string(&receipt)?;
    assert!(!receipt_json.contains(CANARY));
    let codex = receipt
        .checks
        .iter()
        .find(|check| check.name == "codex")
        .expect("codex version check");
    assert!(codex.ok, "unexpected detail: {}", codex.detail);
    assert!(codex.detail.contains("output redacted"));

    let hanging = root.join("codex-fake-doctor-hang");
    write_text(&hanging, "#!/bin/sh\nsleep 10\n")?;
    let mut permissions = fs::metadata(&hanging)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&hanging, permissions)?;
    let started = Instant::now();
    let error = run_bounded_doctor_command(
        &hanging,
        &["--version"],
        "codex --version",
        Duration::from_millis(100),
        DOCTOR_VERSION_OUTPUT_LIMIT,
    )
    .expect_err("hanging Codex doctor command should time out");
    assert!(error.to_string().contains("timed out"));
    assert!(started.elapsed() < Duration::from_secs(3));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn ensure_codex_ready_if_needed_skips_when_mlx_only() -> Result<()> {
    let mut cfg = Config::default();
    cfg.roles.master.provider = RoleProvider::MlxLm;
    cfg.roles.planner.provider = RoleProvider::MlxLm;
    cfg.roles.foreman.provider = RoleProvider::MlxLm;
    cfg.roles.worker.provider = RoleProvider::MlxLm;
    cfg.codex_bin = "not-a-codex-name".to_string();
    ensure_codex_ready_if_needed(&cfg)?;
    cfg.roles.worker.provider = RoleProvider::Codex;
    assert!(ensure_codex_ready_if_needed(&cfg).is_err());
    Ok(())
}

#[test]
fn hermes_cycle_error_receipt_has_additive_fields() {
    let receipt = receipts::CycleReceipt::error(
        "cycle-1".to_string(),
        "/tmp/repo".to_string(),
        None,
        "/tmp/events.jsonl".to_string(),
        "malformed_output",
        "failed to parse JSON response",
        Some("worker".to_string()),
        Some(".orchestrator/runs/x.ollama.json".to_string()),
    );
    assert_eq!(receipt.outcome, "error");
    assert_eq!(receipt.error_kind.as_deref(), Some("malformed_output"));
    assert!(receipt
        .error_summary
        .as_deref()
        .unwrap_or("")
        .contains("failed to parse"));
    assert_eq!(receipt.role.as_deref(), Some("worker"));
}

#[test]
fn hermes_hydrate_worker_null_blocker_parses() {
    let raw = r#"{
      "task_id": "T1",
      "status": "done",
      "summary": null,
      "files_changed": [],
      "commands_run": [],
      "tests_run": [],
      "handoff_to_rust": [{"id":"c1","program":"true","args":[],"cwd":null,"timeout_seconds":1,"kind":"smoke","failure_policy":"collect","expected_exit":0,"tags":[],"source":"t"}],
      "blocker": null,
      "confidence": "high"
    }"#;
    let worker: WorkerOutput = parse_json_response(raw).expect("hydrate+parse");
    assert_eq!(worker.summary, "");
    assert_eq!(worker.blocker, "");
    assert!(worker.handoff_to_rust[0].cwd.is_none());
}

#[test]
fn worker_json_stamps_missing_task_id_from_known_context() {
    let raw = r#"{
      "status": "done",
      "summary": "Created STATUS.md",
      "files_changed": ["STATUS.md"],
      "commands_run": [],
      "tests_run": [],
      "handoff_to_rust": [],
      "blocker": null,
      "confidence": "high"
    }"#;
    let worker = parse_worker_json_response(raw, "MLX-DOG-001").expect("stamp+parse");
    assert_eq!(worker.task_id, "MLX-DOG-001");
    assert_eq!(worker.status, "done");
    assert_eq!(worker.files_changed, vec!["STATUS.md".to_string()]);
}

#[test]
fn enforce_worker_claimed_file_evidence_blocks_missing_paths() -> Result<()> {
    let root = temp_root("worker-claimed-files");
    let paths = Paths::new(&root)?;
    fs::create_dir_all(&paths.root)?;
    let worker = WorkerOutput {
        task_id: "MLX-DOG-001".to_string(),
        status: "done".to_string(),
        summary: "Created STATUS.md".to_string(),
        files_changed: vec!["STATUS.md".to_string()],
        commands_run: Vec::new(),
        tests_run: Vec::new(),
        handoff_to_rust: Vec::new(),
        orchestration_request: None,
        blocker: String::new(),
        confidence: "high".to_string(),
    };
    let blocked = enforce_worker_claimed_file_evidence(&paths, worker);
    assert_eq!(blocked.status, "blocked");
    assert!(blocked.blocker.contains("STATUS.md"));

    write_text(&root.join("STATUS.md"), "MLX dogfood active\n")?;
    let worker = WorkerOutput {
        task_id: "MLX-DOG-001".to_string(),
        status: "done".to_string(),
        summary: "Created STATUS.md".to_string(),
        files_changed: vec!["STATUS.md".to_string()],
        commands_run: Vec::new(),
        tests_run: Vec::new(),
        handoff_to_rust: Vec::new(),
        orchestration_request: None,
        blocker: String::new(),
        confidence: "high".to_string(),
    };
    let ok = enforce_worker_claimed_file_evidence(&paths, worker);
    assert_eq!(ok.status, "done");
    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn hermes_machine_json_mode_redirects_println_progress() {
    assert!(!machine_json_mode());
    let _guard = MachineJsonModeGuard::enter();
    assert!(machine_json_mode());
    // Progress path uses shadowed println!; this only asserts the guard toggles.
    drop(_guard);
    assert!(!machine_json_mode());
}

#[test]
fn hermes_status_observe_does_not_save_recovered_queue() -> Result<()> {
    let root = temp_root("hermes-status-observe");
    let paths = Paths::new(&root)?;
    init_repo(&paths, "hermes observe")?;
    write_text(
        &paths.next_steps,
        r#"
### OBS-1 - Observe Work
- Task ID: `OBS-1`
- Title: Observe Work
- Objective: Stay unqueued under observe.
- Status: Ready
- Priority: normal
- Allowed files:
  - `README.md`
- Exact steps:
  1. Do nothing in observe mode.
- Verification commands:
  - `git diff --check`
- Success criteria:
  - `observe leaves queue alone`
"#,
    )?;
    let before = load_state(&paths)?;
    assert!(before.queue.is_empty());
    status(root.clone(), true, true)?;
    let after = load_state(&paths)?;
    assert!(
        after.queue.is_empty(),
        "observe-only status must not recover/save queue work"
    );
    status(root.clone(), true, false)?;
    let refreshed = load_state(&paths)?;
    assert!(
        !refreshed.queue.is_empty() || refreshed.planner_due || !refreshed.mission.is_empty(),
        "mutating status should be allowed to refresh state"
    );

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn hermes_disposition_json_records_keep_paused() -> Result<()> {
    let root = temp_root("hermes-disposition-json");
    fs::create_dir_all(root.join(".orchestrator"))?;
    let paths = Paths::new(&root)?;
    save_state(
        &paths,
        &State {
            mission: "json disposition".to_string(),
            human_disposition_required: Some(HumanDispositionPause {
                reason: "dirty adoption needs a human".to_string(),
                options: vec!["preserve".to_string()],
                source: "rust_dirty_adoption".to_string(),
            }),
            ..State::default()
        },
    )?;

    disposition(root.clone(), "keep-paused".to_string(), false, true)?;
    let state = load_state(&paths)?;
    assert!(state.human_disposition_required.is_some());

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn recovered_approved_tasks_can_clear_stale_escalation_flags() -> Result<()> {
    let root = temp_root("recover-approved");
    fs::create_dir_all(root.join(".orchestrator"))?;
    write_text(
        &root.join("NEXT_STEPS.md"),
        r#"
### TASK-130 - Ready Work
- Task ID: `TASK-130`
- Title: Ready Work
- Objective: Do one ready thing.
- Status: Approved.
- Allowed files:
  - `README.md`
"#,
    )?;

    let paths = Paths::new(&root)?;
    let mut state = State {
        master_due: true,
        planner_due: true,
        needs_reconciliation: true,
        ..State::default()
    };

    let recovered = recover_approved_next_steps_work(&paths, &mut state)?;

    assert_eq!(recovered, 1);
    assert_eq!(state.queue.len(), 1);
    assert!(!state.master_due);
    assert!(!state.planner_due);
    assert!(!state.needs_reconciliation);

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn idle_escalation_guard_clears_model_bounce_without_active_work() -> Result<()> {
    let root = temp_root("idle-guard");
    fs::create_dir_all(root.join(".orchestrator"))?;
    write_text(
        &root.join("NEXT_STEPS.md"),
        r#"
## Active Queue Summary
- Approved Worker task queue:
  - None.

The detailed task records below are archived continuity evidence. They remain
useful for provenance, but they do not reopen approval.

### TASK-001 - Old Blocker
- Status: Blocked.
"#,
    )?;

    let paths = Paths::new(&root)?;
    let mut state = State {
        project_complete: true,
        master_due: true,
        planner_due: true,
        needs_reconciliation: true,
        recent_events: vec![Event {
            ts: "2026-04-27T00:00:00Z".to_string(),
            role: "planner".to_string(),
            kind: "plan".to_string(),
            summary: "no approved tasks".to_string(),
        }],
        ..State::default()
    };

    assert!(settle_idle_escalation_if_no_active_work(
        &paths, &mut state
    )?);
    assert!(!state.master_due);
    assert!(!state.planner_due);
    assert!(!state.needs_reconciliation);

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn idle_escalation_guard_keeps_open_mission_flags() -> Result<()> {
    let root = temp_root("idle-guard-open-mission");
    fs::create_dir_all(root.join(".orchestrator"))?;
    write_text(
        &root.join("NEXT_STEPS.md"),
        r#"
## Active Queue Summary
- Approved Worker task queue:
  - None.

### TASK-001 - Old Blocker
- Status: Blocked.
"#,
    )?;

    let paths = Paths::new(&root)?;
    let mut state = State {
        master_due: true,
        planner_due: true,
        needs_reconciliation: true,
        recent_events: vec![Event {
            ts: "2026-04-27T00:00:00Z".to_string(),
            role: "planner".to_string(),
            kind: "plan".to_string(),
            summary: "no approved tasks".to_string(),
        }],
        ..State::default()
    };

    assert!(!settle_idle_escalation_if_no_active_work(
        &paths, &mut state
    )?);
    assert!(state.master_due);
    assert!(state.planner_due);
    assert!(state.needs_reconciliation);

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn idle_escalation_guard_preserves_first_adoption_pass() -> Result<()> {
    let root = temp_root("idle-guard-first-pass");
    fs::create_dir_all(root.join(".orchestrator"))?;
    write_text(
        &root.join("NEXT_STEPS.md"),
        "# NEXT_STEPS\n\nNo approved tasks yet.\n",
    )?;

    let paths = Paths::new(&root)?;
    let mut state = State {
        master_due: true,
        needs_reconciliation: true,
        ..State::default()
    };

    assert!(!settle_idle_escalation_if_no_active_work(
        &paths, &mut state
    )?);
    assert!(state.master_due);
    assert!(state.needs_reconciliation);

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn foreman_review_rejects_cross_task_acceptance() -> Result<()> {
    let root = temp_root("review-task-mismatch");
    fs::create_dir_all(root.join(".orchestrator"))?;
    write_text(
        &root.join("NEXT_STEPS.md"),
        r#"
### TASK-131 - Selected Task
- Task ID: `TASK-131`
- Status: Done.
- Allowed files:
  - `README.md`
"#,
    )?;
    let paths = Paths::new(&root)?;
    let task = TaskSpec {
        task_id: "TASK-131".to_string(),
        ..sample_task(vec!["README.md".to_string()])
    };
    let mut review = SeniorDevReviewOutput {
        task_id: "TASK-132".to_string(),
        accepted: true,
        reason_code: "accepted_clean".to_string(),
        commit_created: false,
        commit_sha: "abc123".to_string(),
        needs_master: false,
        needs_planner: false,
        project_complete: false,
        summary: "Accepted TASK-132 by mistake.".to_string(),
        problems: vec![],
        replacement_tasks: vec![],
        followup_tasks: vec![],
        warnings: vec![],
    };

    validate_foreman_review_for_task(&paths, &task, &mut review);
    assert!(!review.accepted);
    assert!(review.needs_planner);
    assert!(review.summary.contains("task mismatch"));

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn stale_queued_done_tasks_are_removed_from_state() -> Result<()> {
    let root = temp_root("stale-queue");
    fs::create_dir_all(root.join(".orchestrator"))?;
    write_text(
        &root.join("NEXT_STEPS.md"),
        r#"
### TASK-132 - Done Task
- Task ID: `TASK-132`
- Status: Done.
- Allowed files:
  - `README.md`
"#,
    )?;
    let paths = Paths::new(&root)?;
    let mut state = State {
        queue: vec![TaskSpec {
            task_id: "TASK-132".to_string(),
            ..sample_task(vec!["README.md".to_string()])
        }],
        ..State::default()
    };

    assert_eq!(sync_state_queue_with_next_steps(&paths, &mut state), 1);
    assert!(state.queue.is_empty());

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn project_complete_reopens_when_next_steps_has_pending_work() -> Result<()> {
    let root = temp_root("reopen");
    fs::create_dir_all(root.join(".orchestrator"))?;
    write_text(&root.join("ROADMAP.md"), "## Roadmap\n")?;
    write_text(
        &root.join("NEXT_STEPS.md"),
        "### TASK-001 - Pending\n- Status: Pending approval.\n",
    )?;

    let paths = Paths::new(&root)?;
    let mut state = State {
        project_complete: true,
        ..State::default()
    };

    reopen_if_next_steps_unfinished(&paths, &mut state)?;
    assert!(!state.project_complete);
    assert!(state.needs_reconciliation);

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn project_complete_reopens_when_next_steps_has_future_handoff_work() -> Result<()> {
    let root = temp_root("reopen-future-work");
    fs::create_dir_all(root.join(".orchestrator"))?;
    write_text(&root.join("ROADMAP.md"), "## Roadmap\n")?;
    write_text(
        &root.join("NEXT_STEPS.md"),
        r#"
## Active Queue Summary
- Approved Worker task queue:
  - None.

### TASK-236 - Bounded Session Data-Phase Observation Session
- Task ID: `TASK-236`
- Status: Draft. Requires explicit approval before implementation.
"#,
    )?;
    write_text(
        &root.join("NEXT_STEPS_non-ai.md"),
        "# NEXT_STEPS_non-ai\n\nNo approved deterministic tasks yet.\n",
    )?;

    let paths = Paths::new(&root)?;
    let mut state = State {
        project_complete: true,
        ..State::default()
    };

    reopen_if_next_steps_unfinished(&paths, &mut state)?;
    assert!(!state.project_complete);
    assert!(state.planner_due);
    assert!(!should_stop_for_no_work(&paths, &state)?);

    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn non_master_completion_claim_routes_to_master_confirmation() -> Result<()> {
    let root = temp_root("completion-gate");
    fs::create_dir_all(root.join(".orchestrator"))?;
    write_text(
        &root.join("NEXT_STEPS.md"),
        "# NEXT_STEPS\n\nNo approved Worker tasks.\n",
    )?;
    write_text(
        &root.join("NEXT_STEPS_non-ai.md"),
        "# NEXT_STEPS_non-ai\n\nNo approved deterministic tasks yet.\n",
    )?;

    let paths = Paths::new(&root)?;
    let mut state = State {
        mission: "Get an demo service running locally with documented run/test steps".to_string(),
        ..State::default()
    };

    apply_non_master_project_complete_claim(
        &paths,
        &mut state,
        "Project Manager",
        true,
        "The current local diagnostic queue is empty.",
    )?;

    assert!(!state.project_complete);
    assert!(state.master_due);
    assert!(!should_stop_for_no_work(&paths, &state)?);
    assert!(state
        .recent_events
        .iter()
        .any(|event| event.role == "completion-gate" && event.kind == "needs-master"));

    fs::remove_dir_all(root)?;
    Ok(())
}
