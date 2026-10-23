//! SDK-backed MCP stdio server.
//!
//! Observe mode is intentionally small: status, doctor, and durable job status.
//! The repository is pinned at startup, no tool accepts a repository override,
//! and every tool result includes structured JSON plus compatible JSON text.

use anyhow::{bail, Context, Result};
use clap::ValueEnum;
use rmcp::{
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
        ListResourcesResult, ListToolsResult, PaginatedRequestParams, ServerCapabilities,
        ServerInfo, Tool, ToolAnnotations,
    },
    service::RequestContext,
    ErrorData as McpError, RoleServer, ServerHandler, ServiceExt,
};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::io::AsyncReadExt;

use super::jobs::{
    validate_job_id, JobProgress, JobRecord, JobState, JobStore, StepProcessProvenance, JOB_SCHEMA,
    STEP_PROCESS_PROVENANCE_SCHEMA,
};
use crate::cancellation::{
    clear_active_child_bridge, prepare_active_child_bridge, read_active_child_process_group,
    ActiveChildProcessEvidence, CancellationToken, MCP_JOB_ID_ENV,
};
use crate::config::{CommitMode, Config, RoleProvider};
use crate::lock::acquire_repo_lock;
use crate::paths::Paths;
use crate::revision::{repository_revision, require_clean_outside_orchestrator};
use crate::state::State;
use crate::task_import::preview as preview_task_import;

const QUERY_TIMEOUT_SECONDS: u64 = 60;
const MIN_JOB_TIMEOUT_SECONDS: u64 = 60;
const MAX_JOB_TIMEOUT_SECONDS: u64 = 21_600;
const DEFAULT_JOB_TIMEOUT_SECONDS: u64 = 7_200;
const MAX_QUERY_STDOUT_BYTES: usize = 1024 * 1024;
const MAX_QUERY_STDERR_BYTES: usize = 1024 * 1024;
const MAX_STEP_STDOUT_BYTES: usize = 16 * 1024 * 1024;
const MAX_STEP_STDERR_BYTES: usize = 1024 * 1024;
const CANCELLATION_GRACE_SECONDS: u64 = 10;
const POLL_HINT_MS: u64 = 2_000;
const MCP_STEP_PROVENANCE_ENV: &str = "CODEX_TEAM_MCP_STEP_PROVENANCE";
const MCP_STEP_BARRIER_FD_ENV: &str = "CODEX_TEAM_MCP_STEP_BARRIER_FD";
const MCP_STEP_BARRIER_RELEASE: u8 = 0xa5;
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "freebsd"))]
const MAX_PROCESS_INSPECTION_BYTES: usize = 4 * 1024 * 1024;

static NEXT_JOB_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "lower")]
pub enum McpAccess {
    #[default]
    Observe,
    Delegate,
    Operate,
}

#[derive(Debug, Clone)]
pub(crate) struct CodexTeamMcp {
    repo: PathBuf,
    executable: PathBuf,
    access: McpAccess,
    #[allow(dead_code)]
    job_timeout: Duration,
    jobs: JobStore,
    active_jobs: ActiveJobs,
    broker: crate::broker::BrokerClient,
    observations: Arc<Mutex<HashMap<String, crate::mechanistic::LocalObservation>>>,
    mutation_gate: Arc<tokio::sync::Mutex<()>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyArgs {}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StepStatusArgs {
    job_id: String,
    #[serde(default)]
    wait_seconds: u8,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StepStartArgs {
    expected_revision: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StepCancelArgs {
    job_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TaskImportApplyArgs {
    task: Value,
    preview_digest: String,
    expected_revision: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CommitPendingArgs {
    pending_id: String,
    pending_digest: String,
    expected_revision: String,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum DispositionDecision {
    Preserve,
    Recheck,
    Gate,
    KeepPaused,
}

impl DispositionDecision {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Preserve => "preserve",
            Self::Recheck => "recheck",
            Self::Gate => "gate",
            Self::KeepPaused => "keep-paused",
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DispositionArgs {
    decision: DispositionDecision,
    expected_revision: String,
    pause_fingerprint: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RecipeListArgs {
    #[serde(default)]
    global: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RecipePreviewArgs {
    recipe_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RecipeApproveArgs {
    recipe_id: String,
    expected_digest: String,
    expected_revision: String,
    human_confirmed: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RecipeRejectArgs {
    recipe_id: String,
    expected_digest: String,
    human_confirmed: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RecipeReactivateArgs {
    recipe_id: String,
    expected_digest: String,
    expected_revision: String,
    human_confirmed: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RecipeRetireArgs {
    recipe_id: String,
    expected_digest: String,
    human_confirmed: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SelfImprovePauseArgs {
    reason: String,
    human_confirmed: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SelfImproveResumeArgs {
    expected_revision: String,
    human_confirmed: bool,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum SelfImproveComponent {
    Ca,
    CodexTeam,
}

impl SelfImproveComponent {
    fn contract_value(self) -> crate::self_improvement::ComponentV1 {
        match self {
            Self::Ca => crate::self_improvement::ComponentV1::Ca,
            Self::CodexTeam => crate::self_improvement::ComponentV1::CodexTeam,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SelfImproveRollbackArgs {
    component: SelfImproveComponent,
    expected_revision: String,
    human_confirmed: bool,
}

#[derive(Debug, Clone, Default)]
struct ActiveJobs {
    inner: Arc<Mutex<HashMap<String, ActiveJob>>>,
}

#[derive(Debug, Clone)]
struct ActiveJob {
    cancellation: CancellationToken,
    process_group: i32,
}

impl ActiveJobs {
    fn insert(&self, job_id: String, job: ActiveJob) -> Result<()> {
        let mut active = self
            .inner
            .lock()
            .map_err(|_| anyhow::anyhow!("active MCP job registry is poisoned"))?;
        if active.insert(job_id.clone(), job).is_some() {
            bail!("MCP job `{job_id}` is already active");
        }
        Ok(())
    }

    fn get(&self, job_id: &str) -> Result<Option<ActiveJob>> {
        let active = self
            .inner
            .lock()
            .map_err(|_| anyhow::anyhow!("active MCP job registry is poisoned"))?;
        Ok(active.get(job_id).cloned())
    }

    fn remove(&self, job_id: &str) -> Result<()> {
        let mut active = self
            .inner
            .lock()
            .map_err(|_| anyhow::anyhow!("active MCP job registry is poisoned"))?;
        active.remove(job_id);
        Ok(())
    }

    fn snapshot(&self) -> Result<HashMap<String, ActiveJob>> {
        let active = self
            .inner
            .lock()
            .map_err(|_| anyhow::anyhow!("active MCP job registry is poisoned"))?;
        Ok(active.clone())
    }

    fn is_empty(&self) -> Result<bool> {
        let active = self
            .inner
            .lock()
            .map_err(|_| anyhow::anyhow!("active MCP job registry is poisoned"))?;
        Ok(active.is_empty())
    }
}

impl CodexTeamMcp {
    pub(crate) fn new(repo: PathBuf, access: McpAccess, job_timeout_seconds: u64) -> Result<Self> {
        validate_job_timeout(job_timeout_seconds)?;
        let repo = pin_git_root(&repo)?;
        if access == McpAccess::Operate {
            validate_operate_startup(&repo)?;
        }
        let executable = std::env::current_exe().context("resolve current codex-team binary")?;
        Ok(Self {
            jobs: JobStore::new(&repo),
            broker: crate::broker::BrokerClient::new(&repo)?,
            observations: Arc::new(Mutex::new(HashMap::new())),
            repo,
            executable,
            access,
            job_timeout: Duration::from_secs(job_timeout_seconds),
            active_jobs: ActiveJobs::default(),
            mutation_gate: Arc::new(tokio::sync::Mutex::new(())),
        })
    }

    #[cfg(test)]
    fn with_executable(mut self, executable: PathBuf) -> Self {
        self.executable = executable;
        self
    }

    fn tool_catalog(&self) -> Vec<Tool> {
        let mut tools = observe_tools();
        if self.access != McpAccess::Observe {
            tools.extend(delegation_tools());
        }
        if self.access == McpAccess::Operate {
            tools.extend(operate_foundation_tools());
        }
        tools
    }

    /// An operate server cannot safely adopt a step whose stdio and child
    /// handle belonged to a crashed predecessor. Terminate every surviving
    /// step session before accepting requests, then retain an Interrupted
    /// record and partial-state latch for explicit operator review.
    async fn recover_running_jobs_on_restart(&self) -> Result<()> {
        for mut record in self.jobs.records()? {
            if record.kind != super::jobs::JobKind::Step || record.status != JobState::Running {
                continue;
            }
            let termination = match record.outer_process_group {
                Some(process_group) if process_group_exists(process_group) => {
                    terminate_recovered_step_tree(
                        &self.repo,
                        &self.executable,
                        &record,
                        process_group,
                    )
                    .await
                }
                Some(_) => Ok(()),
                None if record.launch_nonce.is_some() => Ok(()),
                None => Err(anyhow::anyhow!(
                    "running MCP job lacks a crash-safe launch intent"
                )),
            };
            if let Err(error) = termination {
                record.repository_may_be_partial = true;
                record.updated_at = chrono::Utc::now().to_rfc3339();
                record.error = Some(bounded_message(&format!(
                    "restart could not safely terminate the prior step: {error:#}"
                )));
                self.jobs.save_atomic(&record)?;
                return Err(error).context("recover active MCP step after server restart");
            }
            let _ = clear_active_child_bridge(&self.repo, &record.job_id);
            record.status = JobState::Interrupted;
            record.updated_at = chrono::Utc::now().to_rfc3339();
            record.outer_process_group = None;
            record.active_child_process_group = None;
            record.error = Some("server restarted while the step was active".to_string());
            record.repository_may_be_partial = true;
            self.jobs.save_atomic(&record)?;
        }
        Ok(())
    }

    async fn call_broker(&self, operation: crate::broker::BrokerOperation) -> CallToolResult {
        match self.broker.call(operation.clone()).await {
            Ok(value) => structured_success(value),
            Err(error) if error.safe_local_fallback => {
                if let crate::broker::BrokerOperation::Query { request } = operation {
                    let repo = self.repo.clone();
                    match tokio::task::spawn_blocking(move || {
                        let mut receipt = crate::mechanistic::MechanisticExecutor::new(&repo)?
                            .execute_queries(&request)?;
                        crate::broker::attach_query_capsule(&request, &mut receipt)?;
                        Ok::<_, anyhow::Error>(serde_json::to_value(receipt)?)
                    })
                    .await
                    {
                        Ok(Ok(value)) => structured_success(value),
                        Ok(Err(local)) => {
                            structured_error("QUERY_FAILED", &format!("{local:#}"), false)
                        }
                        Err(local) => {
                            structured_error("QUERY_FAILED", &format!("{local:#}"), false)
                        }
                    }
                } else {
                    structured_error(&error.code, &error.message, error.retryable)
                }
            }
            Err(error) => structured_error(&error.code, &error.message, error.retryable),
        }
    }

    async fn call_delegation_start(
        &self,
        request: ca_automation_protocol::MechanisticRequestV1,
    ) -> CallToolResult {
        if let Err(error) = crate::mechanistic::validate_request(&request) {
            return structured_error("INVALID_ARGUMENTS", &format!("{error:#}"), false);
        }
        if request.actions.len() != 1 || !request.actions[0].is_observation() {
            return self
                .call_broker(crate::broker::BrokerOperation::Start { request })
                .await;
        }
        match self.start_observation(request).await {
            Ok(value) => structured_success(value),
            Err(error) => structured_error("DELEGATION_START_FAILED", &format!("{error:#}"), false),
        }
    }

    async fn start_observation(
        &self,
        request: ca_automation_protocol::MechanisticRequestV1,
    ) -> Result<Value> {
        let revision = repository_revision(&Paths::new(&self.repo)?)?;
        anyhow::ensure!(
            revision == request.expected_revision,
            "repository revision is stale"
        );
        let observation =
            crate::mechanistic::LocalObservation::start(&self.repo, &request.actions[0]).await?;
        let job_id = new_job_id();
        let now = chrono::Utc::now().to_rfc3339();
        let record = JobRecord {
            schema: JOB_SCHEMA.into(),
            job_id: job_id.clone(),
            kind: super::jobs::JobKind::DelegationObservation,
            status: JobState::Running,
            expected_revision: revision,
            started_at: now.clone(),
            updated_at: now,
            outer_process_group: None,
            launch_nonce: None,
            process_provenance: None,
            active_child_process_group: None,
            progress: Some(JobProgress {
                phase: "observing".into(),
                summary: "observing an external process".into(),
            }),
            cycle_receipt: None,
            mechanistic_receipt: None,
            delegation_feedback: None,
            delegation_manifest_digest: None,
            error: None,
            repository_may_be_partial: false,
        };
        self.jobs.save_atomic(&record)?;
        self.observations
            .lock()
            .map_err(|_| anyhow::anyhow!("observation registry is poisoned"))?
            .insert(job_id.clone(), observation.clone());
        let jobs = self.jobs.clone();
        let observation_job = job_id.clone();
        tokio::spawn(async move {
            loop {
                let Ok(snapshot) = observation.snapshot().await else {
                    break;
                };
                if let Ok(mut record) = jobs.load_sync(&observation_job) {
                    if record.status.is_terminal() {
                        break;
                    }
                    record.updated_at = chrono::Utc::now().to_rfc3339();
                    record.progress = Some(JobProgress {
                        phase: snapshot.state.clone(),
                        summary: snapshot.summary.clone(),
                    });
                    record.mechanistic_receipt = serde_json::to_value(&snapshot).ok();
                    if snapshot.terminal {
                        record.status = match snapshot.status {
                            ca_automation_protocol::MechanisticReceiptStatusV1::Cancelled => {
                                JobState::Cancelled
                            }
                            ca_automation_protocol::MechanisticReceiptStatusV1::Failed => {
                                JobState::Failed
                            }
                            _ => JobState::Completed,
                        };
                    }
                    let _ = jobs.save_atomic(&record);
                }
                if snapshot.terminal {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        });
        Ok(
            json!({"schema":"codex-team.mcp_delegation_start.v1", "job_id":job_id, "status":"running", "kind":"observation", "poll_after_ms":250}),
        )
    }

    async fn call_delegation_status(&self, args: StepStatusArgs) -> CallToolResult {
        if self
            .jobs
            .load_sync(&args.job_id)
            .is_ok_and(|record| record.kind == super::jobs::JobKind::DelegationObservation)
        {
            return self.call_step_status(args).await;
        }
        self.call_broker(crate::broker::BrokerOperation::JobStatus {
            job_id: args.job_id,
            wait_seconds: args.wait_seconds,
        })
        .await
    }

    async fn call_delegation_cancel(&self, args: StepCancelArgs) -> CallToolResult {
        let observation = self
            .observations
            .lock()
            .ok()
            .and_then(|map| map.get(&args.job_id).cloned());
        if let Some(observation) = observation {
            observation.cancel().await;
            if let Ok(mut record) = self.jobs.load_sync(&args.job_id) {
                record.status = JobState::Cancelled;
                record.updated_at = chrono::Utc::now().to_rfc3339();
                let _ = self.jobs.save_atomic(&record);
            }
            return structured_success(
                json!({"schema":"codex-team.mcp_delegation_cancel.v1", "job_id":args.job_id, "status":"cancelled", "observed_process_signalled":false}),
            );
        }
        self.call_broker(crate::broker::BrokerOperation::Cancel {
            job_id: args.job_id,
        })
        .await
    }

    async fn call_status(&self) -> CallToolResult {
        match self
            .query_cli(&[
                "status",
                "--repo",
                path_text(&self.repo),
                "--json",
                "--observe",
            ])
            .await
        {
            Ok(receipt) => structured_success(receipt),
            Err(error) => structured_error("STATUS_FAILED", &format!("{error:#}"), false),
        }
    }

    async fn call_doctor(&self) -> CallToolResult {
        match self
            .query_cli(&["doctor", "--repo", path_text(&self.repo), "--json"])
            .await
        {
            Ok(receipt) => {
                if receipt
                    .get("healthy")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    structured_success(receipt)
                } else {
                    structured_tool_error_value(receipt)
                }
            }
            Err(error) => structured_error("DOCTOR_FAILED", &format!("{error:#}"), false),
        }
    }

    async fn call_step_status(&self, args: StepStatusArgs) -> CallToolResult {
        match self.jobs.wait(&args.job_id, args.wait_seconds).await {
            Ok(record) => match serde_json::to_value(record) {
                Ok(value) => structured_success(value),
                Err(error) => structured_error("JOB_SERIALIZE_FAILED", &error.to_string(), false),
            },
            Err(error) => structured_error("JOB_STATUS_FAILED", &format!("{error:#}"), false),
        }
    }

    async fn call_recipe_list(&self, args: RecipeListArgs) -> CallToolResult {
        let mut command = vec!["recipe", "list", "--repo", path_text(&self.repo)];
        if args.global {
            command.push("--global");
        }
        command.push("--json");
        match self.query_cli(&command).await {
            Ok(receipt) => structured_success(receipt),
            Err(error) => structured_error("RECIPE_LIST_FAILED", &format!("{error:#}"), false),
        }
    }

    async fn call_recipe_preview(&self, args: RecipePreviewArgs) -> CallToolResult {
        let command = [
            "recipe",
            "preview",
            "--repo",
            path_text(&self.repo),
            "--id",
            args.recipe_id.as_str(),
            "--json",
        ];
        match self.query_cli(&command).await {
            Ok(receipt) => structured_success(receipt),
            Err(error) => structured_error("RECIPE_PREVIEW_FAILED", &format!("{error:#}"), false),
        }
    }

    async fn call_recipe_approve(&self, args: RecipeApproveArgs) -> CallToolResult {
        if !args.human_confirmed {
            return structured_error(
                "HUMAN_CONFIRMATION_REQUIRED",
                "global recipe approval requires explicit human confirmation",
                false,
            );
        }
        if let Err(error) = validate_recipe_mutation_args(
            &args.recipe_id,
            &args.expected_digest,
            Some(&args.expected_revision),
        ) {
            return structured_error("INVALID_ARGUMENT", &error.to_string(), false);
        }
        let command = [
            "recipe",
            "approve",
            "--repo",
            path_text(&self.repo),
            "--id",
            args.recipe_id.as_str(),
            "--expected-digest",
            args.expected_digest.as_str(),
            "--expected-revision",
            args.expected_revision.as_str(),
            "--scope",
            "global",
            "--json",
        ];
        self.call_recipe_mutation(&command, "RECIPE_APPROVE_FAILED")
            .await
    }

    async fn call_recipe_reject(&self, args: RecipeRejectArgs) -> CallToolResult {
        if !args.human_confirmed {
            return structured_error(
                "HUMAN_CONFIRMATION_REQUIRED",
                "recipe rejection requires explicit human confirmation",
                false,
            );
        }
        if let Err(error) =
            validate_recipe_mutation_args(&args.recipe_id, &args.expected_digest, None)
        {
            return structured_error("INVALID_ARGUMENT", &error.to_string(), false);
        }
        let command = [
            "recipe",
            "reject",
            "--repo",
            path_text(&self.repo),
            "--id",
            args.recipe_id.as_str(),
            "--expected-digest",
            args.expected_digest.as_str(),
            "--json",
        ];
        self.call_recipe_mutation(&command, "RECIPE_REJECT_FAILED")
            .await
    }

    async fn call_recipe_reactivate(&self, args: RecipeReactivateArgs) -> CallToolResult {
        if !args.human_confirmed {
            return structured_error(
                "HUMAN_CONFIRMATION_REQUIRED",
                "recipe reactivation requires explicit human confirmation",
                false,
            );
        }
        if let Err(error) = validate_recipe_mutation_args(
            &args.recipe_id,
            &args.expected_digest,
            Some(&args.expected_revision),
        ) {
            return structured_error("INVALID_ARGUMENT", &error.to_string(), false);
        }
        let command = [
            "recipe",
            "reactivate",
            "--repo",
            path_text(&self.repo),
            "--id",
            args.recipe_id.as_str(),
            "--expected-digest",
            args.expected_digest.as_str(),
            "--expected-revision",
            args.expected_revision.as_str(),
            "--json",
        ];
        self.call_recipe_mutation(&command, "RECIPE_REACTIVATE_FAILED")
            .await
    }

    async fn call_recipe_retire(&self, args: RecipeRetireArgs) -> CallToolResult {
        if !args.human_confirmed {
            return structured_error(
                "HUMAN_CONFIRMATION_REQUIRED",
                "global recipe retirement requires explicit human confirmation",
                false,
            );
        }
        if let Err(error) =
            validate_recipe_mutation_args(&args.recipe_id, &args.expected_digest, None)
        {
            return structured_error("INVALID_ARGUMENT", &error.to_string(), false);
        }
        let command = [
            "recipe",
            "retire",
            "--id",
            args.recipe_id.as_str(),
            "--expected-digest",
            args.expected_digest.as_str(),
            "--json",
        ];
        self.call_recipe_mutation(&command, "RECIPE_RETIRE_FAILED")
            .await
    }

    async fn call_recipe_mutation(&self, command: &[&str], code: &str) -> CallToolResult {
        let _mutation = self.mutation_gate.lock().await;
        if let Err(error) = self.jobs.reject_active_job() {
            return structured_error(code, &error.to_string(), false);
        }
        match self.query_cli_with_input(command, None).await {
            Ok(receipt) => structured_success(receipt),
            Err(error) => structured_error(code, &format!("{error:#}"), false),
        }
    }

    async fn call_self_improve_cli(&self, command: &[&str], code: &str) -> CallToolResult {
        match self.query_cli(command).await {
            Ok(receipt) => structured_success(receipt),
            Err(error) => structured_error(code, &format!("{error:#}"), false),
        }
    }

    fn call_self_improve_evaluations(&self) -> CallToolResult {
        let result = (|| -> Result<Value> {
            let Some(vault) = crate::self_improvement::LearningVault::existing_machine()? else {
                return Ok(json!({
                    "schema": "codex-team.self_improvement_evaluations.v1",
                    "evaluations": [],
                    "candidates": [],
                    "transitions": []
                }));
            };
            let evaluations = vault
                .list_immutable::<crate::self_improvement::EvaluationReceiptV1>(
                    "evaluations/receipts",
                )?;
            let candidates = vault
                .list_immutable::<crate::self_improvement::ArtifactCandidateV1>("candidates")?
                .into_iter()
                .rev()
                .take(100)
                .collect::<Vec<_>>();
            let transitions = vault
                .list_immutable::<crate::self_improvement::LearningWorkflowTransitionV1>(
                    "workflows/transitions",
                )?
                .into_iter()
                .rev()
                .take(100)
                .collect::<Vec<_>>();
            Ok(json!({
                "schema": "codex-team.self_improvement_evaluations.v1",
                "evaluations": evaluations,
                "candidates": candidates,
                "transitions": transitions
            }))
        })();
        match result {
            Ok(value) => structured_success(value),
            Err(error) => structured_error(
                "SELF_IMPROVE_EVALUATIONS_FAILED",
                &format!("{error:#}"),
                false,
            ),
        }
    }

    async fn call_self_improve_pause(&self, args: SelfImprovePauseArgs) -> CallToolResult {
        if !args.human_confirmed {
            return structured_error(
                "HUMAN_CONFIRMATION_REQUIRED",
                "pausing autonomous learning requires an explicit host-rendered human action",
                false,
            );
        }
        let _mutation = self.mutation_gate.lock().await;
        let result = (|| -> Result<Value> {
            let vault = crate::self_improvement::LearningVault::existing_machine()?
                .context("self-improvement is not enrolled")?;
            Ok(serde_json::to_value(crate::self_improvement::pause(
                &vault,
                &args.reason,
            )?)?)
        })();
        mutation_result(result, "SELF_IMPROVE_PAUSE_FAILED")
    }

    async fn call_self_improve_resume(&self, args: SelfImproveResumeArgs) -> CallToolResult {
        if !args.human_confirmed {
            return structured_error(
                "HUMAN_CONFIRMATION_REQUIRED",
                "resuming autonomous learning requires an explicit host-rendered human action",
                false,
            );
        }
        if let Err(error) = validate_sha256("expected_revision", &args.expected_revision) {
            return structured_error("INVALID_ARGUMENT", &error.to_string(), false);
        }
        let _mutation = self.mutation_gate.lock().await;
        let result = (|| -> Result<Value> {
            let vault = crate::self_improvement::LearningVault::existing_machine()?
                .context("self-improvement is not enrolled")?;
            Ok(serde_json::to_value(crate::self_improvement::resume(
                &vault,
                &args.expected_revision,
            )?)?)
        })();
        mutation_result(result, "SELF_IMPROVE_RESUME_FAILED")
    }

    async fn call_self_improve_rollback(&self, args: SelfImproveRollbackArgs) -> CallToolResult {
        if !args.human_confirmed {
            return structured_error(
                "HUMAN_CONFIRMATION_REQUIRED",
                "binary rollback requires an explicit host-rendered human action",
                false,
            );
        }
        if let Err(error) = validate_sha256("expected_revision", &args.expected_revision) {
            return structured_error("INVALID_ARGUMENT", &error.to_string(), false);
        }
        let _mutation = self.mutation_gate.lock().await;
        let result = (|| -> Result<Value> {
            let vault = crate::self_improvement::LearningVault::existing_machine()?
                .context("self-improvement is not enrolled")?;
            Ok(serde_json::to_value(
                crate::self_improvement::rollback_with_revision(
                    &vault,
                    args.component.contract_value(),
                    Some(&args.expected_revision),
                )?,
            )?)
        })();
        mutation_result(result, "SELF_IMPROVE_ROLLBACK_FAILED")
    }

    async fn call_step_start(&self, args: StepStartArgs) -> CallToolResult {
        match self.start_step(args).await {
            Ok(value) => structured_success(value),
            Err(error) => structured_error("STEP_START_FAILED", &format!("{error:#}"), false),
        }
    }

    async fn call_step_cancel(&self, args: StepCancelArgs) -> CallToolResult {
        match self.cancel_step(&args.job_id).await {
            Ok(value) => structured_success(value),
            Err(error) => structured_error("STEP_CANCEL_FAILED", &format!("{error:#}"), false),
        }
    }

    fn call_task_import_preview(&self, task: Value) -> CallToolResult {
        let bytes = match serde_json::to_vec(&task) {
            Ok(bytes) if bytes.len() <= 1024 * 1024 => bytes,
            Ok(_) => {
                return structured_error(
                    "INVALID_ARGUMENT",
                    "task import input exceeds the 1 MiB limit",
                    false,
                );
            }
            Err(error) => {
                return structured_error("INVALID_ARGUMENT", &error.to_string(), false);
            }
        };
        let paths = match Paths::new(&self.repo) {
            Ok(paths) => paths,
            Err(error) => {
                return structured_error("TASK_PREVIEW_FAILED", &format!("{error:#}"), false);
            }
        };
        let _lock = match acquire_repo_lock(&paths, "mcp-task-import-preview") {
            Ok(lock) => lock,
            Err(error) => {
                return structured_error("TASK_PREVIEW_FAILED", &format!("{error:#}"), false);
            }
        };
        match preview_task_import(&paths, &bytes).and_then(|receipt| {
            serde_json::to_value(receipt).context("serialize task import preview")
        }) {
            Ok(receipt) => structured_success(receipt),
            Err(error) => structured_error("TASK_PREVIEW_FAILED", &format!("{error:#}"), false),
        }
    }

    async fn call_task_import_apply(&self, args: TaskImportApplyArgs) -> CallToolResult {
        if let Err(error) = validate_sha256("preview_digest", &args.preview_digest) {
            return structured_error("INVALID_ARGUMENT", &error.to_string(), false);
        }
        if let Err(error) = validate_sha256("expected_revision", &args.expected_revision) {
            return structured_error("INVALID_ARGUMENT", &error.to_string(), false);
        }
        let input = match serde_json::to_vec(&args.task) {
            Ok(bytes) if bytes.len() <= 1024 * 1024 => bytes,
            Ok(_) => {
                return structured_error(
                    "INVALID_ARGUMENT",
                    "task import input exceeds the 1 MiB limit",
                    false,
                );
            }
            Err(error) => {
                return structured_error("INVALID_ARGUMENT", &error.to_string(), false);
            }
        };
        let command_args = [
            "task-import",
            "--repo",
            path_text(&self.repo),
            "--input",
            "-",
            "--accept-digest",
            args.preview_digest.as_str(),
            "--expected-revision",
            args.expected_revision.as_str(),
            "--json",
        ];
        let _mutation = self.mutation_gate.lock().await;
        if let Err(error) = self.jobs.reject_active_job() {
            return structured_error("TASK_APPLY_FAILED", &error.to_string(), false);
        }
        match self.query_cli_with_input(&command_args, Some(input)).await {
            Ok(receipt) => structured_success(receipt),
            Err(error) => structured_error("TASK_APPLY_FAILED", &format!("{error:#}"), false),
        }
    }

    async fn call_commit_pending(&self, args: CommitPendingArgs) -> CallToolResult {
        if let Err(error) = validate_job_id(&args.pending_id) {
            return structured_error("INVALID_ARGUMENT", &error.to_string(), false);
        }
        for (label, digest) in [
            ("pending_digest", args.pending_digest.as_str()),
            ("expected_revision", args.expected_revision.as_str()),
        ] {
            if let Err(error) = validate_sha256(label, digest) {
                return structured_error("INVALID_ARGUMENT", &error.to_string(), false);
            }
        }
        let command_args = [
            "commit-pending",
            "--repo",
            path_text(&self.repo),
            "--id",
            args.pending_id.as_str(),
            "--expected-digest",
            args.pending_digest.as_str(),
            "--expected-revision",
            args.expected_revision.as_str(),
            "--json",
        ];
        let _mutation = self.mutation_gate.lock().await;
        if let Err(error) = self.jobs.reject_active_job() {
            return structured_error("COMMIT_PENDING_FAILED", &error.to_string(), false);
        }
        match self.query_cli_with_input(&command_args, None).await {
            Ok(receipt) => structured_success(receipt),
            Err(error) => structured_error("COMMIT_PENDING_FAILED", &format!("{error:#}"), false),
        }
    }

    async fn call_disposition(&self, args: DispositionArgs) -> CallToolResult {
        for (label, digest) in [
            ("expected_revision", args.expected_revision.as_str()),
            ("pause_fingerprint", args.pause_fingerprint.as_str()),
        ] {
            if let Err(error) = validate_sha256(label, digest) {
                return structured_error("INVALID_ARGUMENT", &error.to_string(), false);
            }
        }
        let command_args = [
            "disposition",
            "--repo",
            path_text(&self.repo),
            "--decision",
            args.decision.as_str(),
            "--expected-revision",
            args.expected_revision.as_str(),
            "--pause-fingerprint",
            args.pause_fingerprint.as_str(),
            "--json",
        ];
        let _mutation = self.mutation_gate.lock().await;
        if let Err(error) = self.jobs.reject_active_job() {
            return structured_error("DISPOSITION_FAILED", &error.to_string(), false);
        }
        match self.query_cli_with_input(&command_args, None).await {
            Ok(receipt) => structured_success(receipt),
            Err(error) => structured_error("DISPOSITION_FAILED", &format!("{error:#}"), false),
        }
    }

    async fn start_step(&self, args: StepStartArgs) -> Result<Value> {
        let _mutation = self.mutation_gate.lock().await;
        validate_sha256("expected_revision", &args.expected_revision)?;
        let paths = Paths::new(&self.repo)?;
        let job_id = new_job_id();
        let spawn_nonce = new_step_spawn_nonce()?;
        let mut record = {
            let _lock = acquire_repo_lock(&paths, "mcp-step-start")?;
            validate_operate_startup(&self.repo)?;
            self.jobs.reject_active_job()?;
            let current_revision = repository_revision(&paths)?;
            if current_revision != args.expected_revision {
                bail!("repository revision is stale; inspect status before starting a step");
            }
            require_clean_outside_orchestrator(&paths)?;
            let state = read_state(&paths)?;
            if state.human_disposition_required.is_some() {
                bail!("an active human disposition pause blocks step start");
            }
            if state.pending_commit.is_some() {
                bail!("a pending manual commit blocks step start");
            }
            prepare_active_child_bridge(&self.repo, &job_id)?;
            let now = chrono::Utc::now().to_rfc3339();
            let record = JobRecord {
                schema: JOB_SCHEMA.to_string(),
                job_id: job_id.clone(),
                kind: super::jobs::JobKind::Step,
                status: JobState::Running,
                expected_revision: args.expected_revision.clone(),
                started_at: now.clone(),
                updated_at: now,
                outer_process_group: None,
                launch_nonce: Some(spawn_nonce.clone()),
                process_provenance: None,
                active_child_process_group: None,
                progress: Some(JobProgress {
                    phase: "selecting".to_string(),
                    summary: "step process is starting".to_string(),
                }),
                cycle_receipt: None,
                mechanistic_receipt: None,
                delegation_feedback: None,
                delegation_manifest_digest: None,
                error: None,
                repository_may_be_partial: false,
            };
            self.jobs.save_atomic(&record)?;
            record
        };

        let mut launch_barrier = StepLaunchBarrier::new()?;
        let mut command = tokio::process::Command::new(&self.executable);
        command
            .arg("step")
            .arg("--repo")
            .arg(&self.repo)
            .arg("--json")
            .arg("--expected-revision")
            .arg(&args.expected_revision)
            .current_dir(&self.repo)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env(MCP_JOB_ID_ENV, &job_id)
            .env(MCP_STEP_PROVENANCE_ENV, &spawn_nonce)
            .kill_on_drop(true);
        launch_barrier.configure_child(&mut command);
        configure_step_process_label(&mut command, &job_id, &spawn_nonce);
        configure_step_session(&mut command)?;
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                record.status = JobState::Failed;
                record.updated_at = chrono::Utc::now().to_rfc3339();
                record.error = Some(bounded_message(&format!("failed to launch step: {error}")));
                record.repository_may_be_partial = true;
                self.jobs.save_atomic(&record)?;
                return Err(error).context("launch asynchronous codex-team step");
            }
        };
        launch_barrier.child_spawned();
        let process_group = match child.id().and_then(|pid| i32::try_from(pid).ok()) {
            Some(process_group) => process_group,
            None => {
                let _ = child.start_kill();
                let _ = child.wait().await;
                bail!("step process did not have a valid process-group ID");
            }
        };
        let (process_provenance, process_is_active) = match capture_step_process_provenance(
            process_group,
            &self.executable,
            &self.repo,
            &record,
            &spawn_nonce,
        )
        .await
        {
            Ok(provenance) => (Some(provenance), true),
            Err(error) => {
                if child
                    .try_wait()
                    .context("inspect fast-exiting asynchronous step")?
                    .is_some()
                {
                    (None, false)
                } else {
                    let _ = terminate_child_group(&mut child, process_group).await;
                    record.status = JobState::Failed;
                    record.updated_at = chrono::Utc::now().to_rfc3339();
                    record.error = Some(
                        "failed to establish the asynchronous step process identity".to_string(),
                    );
                    record.repository_may_be_partial = true;
                    let _ = self.jobs.save_atomic(&record);
                    return Err(error).context("establish asynchronous step process provenance");
                }
            }
        };
        record.outer_process_group = process_is_active.then_some(process_group);
        record.process_provenance = process_provenance;
        record.active_child_process_group = None;
        record.updated_at = chrono::Utc::now().to_rfc3339();
        record.progress = Some(JobProgress {
            phase: if process_is_active {
                "role_execution"
            } else {
                "finalizing"
            }
            .to_string(),
            summary: if process_is_active {
                "step process is running"
            } else {
                "step process exited before provenance capture"
            }
            .to_string(),
        });
        if let Err(error) = self.jobs.save_atomic(&record) {
            let _ = terminate_child_group(&mut child, process_group).await;
            record.status = JobState::Failed;
            record.updated_at = chrono::Utc::now().to_rfc3339();
            record.outer_process_group = None;
            record.active_child_process_group = None;
            record.error = Some("failed to persist step process identity".to_string());
            record.repository_may_be_partial = true;
            let _ = self.jobs.save_atomic(&record);
            return Err(error).context("persist asynchronous step process identity");
        }

        let cancellation = CancellationToken::default();
        if process_is_active {
            if let Err(error) = self.active_jobs.insert(
                job_id.clone(),
                ActiveJob {
                    cancellation: cancellation.clone(),
                    process_group,
                },
            ) {
                let _ = terminate_child_group(&mut child, process_group).await;
                record.status = JobState::Failed;
                record.updated_at = chrono::Utc::now().to_rfc3339();
                record.outer_process_group = None;
                record.active_child_process_group = None;
                record.error = Some("failed to register active step process".to_string());
                record.repository_may_be_partial = true;
                let _ = self.jobs.save_atomic(&record);
                return Err(error).context("register asynchronous step process");
            }
            if let Err(error) = launch_barrier.release() {
                let _ = terminate_child_group(&mut child, process_group).await;
                let _ = self.active_jobs.remove(&job_id);
                record.status = JobState::Failed;
                record.updated_at = chrono::Utc::now().to_rfc3339();
                record.outer_process_group = None;
                record.active_child_process_group = None;
                record.error =
                    Some("failed to release the durable step launch barrier".to_string());
                record.repository_may_be_partial = true;
                let _ = self.jobs.save_atomic(&record);
                return Err(error).context("release asynchronous step launch barrier");
            }
        }
        let context = StepJobContext {
            jobs: self.jobs.clone(),
            active_jobs: self.active_jobs.clone(),
            cancellation,
            process_group,
            timeout: self.job_timeout,
            events_path: paths.events.clone(),
            repo: self.repo.clone(),
            job_id: job_id.clone(),
        };
        let background_job_id = job_id.clone();
        tokio::spawn(async move {
            if let Err(error) = run_step_job(&mut child, record, &context).await {
                let _ = terminate_step_tree(&mut child, &context).await;
                if let Ok(mut failed) = context.jobs.load_sync(&background_job_id) {
                    failed.status = JobState::Interrupted;
                    failed.updated_at = chrono::Utc::now().to_rfc3339();
                    failed.outer_process_group = None;
                    failed.active_child_process_group = None;
                    failed.repository_may_be_partial = true;
                    failed.error = Some(bounded_message(&format!(
                        "step supervisor failed: {error:#}"
                    )));
                    let _ = context.jobs.save_atomic(&failed);
                }
                let _ = clear_active_child_bridge(&context.repo, &background_job_id);
                let _ = context.active_jobs.remove(&background_job_id);
            }
        });

        Ok(json!({
            "schema": "codex-team.mcp_step_start.v1",
            "job_id": job_id,
            "status": "running",
            "expected_revision": args.expected_revision,
            "poll_after_ms": POLL_HINT_MS,
        }))
    }

    async fn cancel_step(&self, job_id: &str) -> Result<Value> {
        validate_job_id(job_id)?;
        let record = self.jobs.load(job_id).await?;
        if record.status.is_terminal() {
            return Ok(json!({
                "schema": "codex-team.mcp_step_cancel.v1",
                "job_id": job_id,
                "status": record.status,
                "already_terminal": true,
            }));
        }
        let active = self
            .active_jobs
            .get(job_id)?
            .context("running job is not controlled by this server")?;
        active.cancellation.cancel();
        if let Some(evidence) = read_active_child_process_group(&self.repo, job_id)? {
            validate_nested_process_evidence(&evidence, active.process_group)?;
            signal_nested_process_group(evidence.process_group, libc::SIGTERM)?;
        }
        signal_process_group(active.process_group, libc::SIGTERM)?;
        Ok(json!({
            "schema": "codex-team.mcp_step_cancel.v1",
            "job_id": job_id,
            "status": "cancellation_requested",
            "already_terminal": false,
        }))
    }

    async fn shutdown_jobs(&self) {
        let active = self.active_jobs.snapshot().unwrap_or_default();
        for (job_id, job) in &active {
            job.cancellation.cancel();
            if let Ok(Some(evidence)) = read_active_child_process_group(&self.repo, job_id) {
                if validate_nested_process_evidence(&evidence, job.process_group).is_ok() {
                    let _ = signal_nested_process_group(evidence.process_group, libc::SIGTERM);
                }
            }
            let _ = signal_process_group(job.process_group, libc::SIGTERM);
        }
        let deadline = Instant::now() + Duration::from_secs(CANCELLATION_GRACE_SECONDS);
        while Instant::now() < deadline {
            if self.active_jobs.is_empty().unwrap_or(true) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        for (job_id, job) in self.active_jobs.snapshot().unwrap_or_default() {
            if let Ok(Some(evidence)) = read_active_child_process_group(&self.repo, &job_id) {
                if validate_nested_process_evidence(&evidence, job.process_group).is_ok() {
                    let _ = signal_nested_process_group(evidence.process_group, libc::SIGKILL);
                }
            }
            let _ = signal_process_group(job.process_group, libc::SIGKILL);
        }
        let hard_kill_deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < hard_kill_deadline {
            if self.active_jobs.is_empty().unwrap_or(true) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    async fn query_cli(&self, args: &[&str]) -> Result<Value> {
        self.query_cli_with_input(args, None).await
    }

    async fn query_cli_with_input(&self, args: &[&str], stdin: Option<Vec<u8>>) -> Result<Value> {
        let output = run_bounded(
            &self.executable,
            &self.repo,
            args,
            Duration::from_secs(QUERY_TIMEOUT_SECONDS),
            MAX_QUERY_STDOUT_BYTES,
            MAX_QUERY_STDERR_BYTES,
            stdin,
        )
        .await?;

        let receipt: Value = serde_json::from_slice(&output.stdout)
            .context("codex-team query did not emit one JSON receipt")?;
        if !receipt.is_object() {
            bail!("codex-team query receipt must be a JSON object");
        }
        if !output.success {
            // Doctor intentionally exits nonzero after printing an unhealthy
            // receipt. Preserve it so the caller can inspect every check.
            if args.first() == Some(&"doctor") {
                return Ok(receipt);
            }
            bail!(
                "codex-team query failed: {}",
                sanitized_stderr(&output.stderr)
            );
        }
        Ok(receipt)
    }
}

impl ServerHandler for CodexTeamMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .build(),
        )
        .with_server_info(Implementation::new("codex-team", env!("CARGO_PKG_VERSION")))
        .with_instructions(
            "Inspect status and doctor before acting. Work is pinned to one repository.",
        )
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ListToolsResult, McpError>> + Send + '_ {
        std::future::ready(Ok(ListToolsResult::with_all_items(self.tool_catalog())))
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        self.tool_catalog()
            .into_iter()
            .find(|tool| tool.name.as_ref() == name)
    }

    fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ListResourcesResult, McpError>> + Send + '_ {
        std::future::ready(Ok(ListResourcesResult::with_all_items(Vec::new())))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let args = request.arguments.unwrap_or_default();
        let result = match request.name.as_ref() {
            "repo_query" => {
                match parse_args::<ca_automation_protocol::MechanisticRequestV1>(args) {
                    Ok(request) => {
                        self.call_broker(crate::broker::BrokerOperation::Query { request })
                            .await
                    }
                    Err(result) => result,
                }
            }
            "delegation_start" if self.access != McpAccess::Observe => {
                match parse_args::<ca_automation_protocol::MechanisticRequestV1>(args) {
                    Ok(request) => self.call_delegation_start(request).await,
                    Err(result) => result,
                }
            }
            "delegation_status" if self.access != McpAccess::Observe => {
                match parse_args::<StepStatusArgs>(args) {
                    Ok(args) => self.call_delegation_status(args).await,
                    Err(result) => result,
                }
            }
            "delegation_cancel" if self.access != McpAccess::Observe => {
                match parse_args::<StepCancelArgs>(args) {
                    Ok(args) => self.call_delegation_cancel(args).await,
                    Err(result) => result,
                }
            }
            "delegation_feedback" if self.access != McpAccess::Observe => {
                self.call_broker(crate::broker::BrokerOperation::Feedback {
                    arguments: Value::Object(args),
                })
                .await
            }
            "status" => match parse_args::<EmptyArgs>(args) {
                Ok(_) => self.call_status().await,
                Err(result) => result,
            },
            "doctor" => match parse_args::<EmptyArgs>(args) {
                Ok(_) => self.call_doctor().await,
                Err(result) => result,
            },
            "recipe_list" => match parse_args::<RecipeListArgs>(args) {
                Ok(args) => self.call_recipe_list(args).await,
                Err(result) => result,
            },
            "recipe_preview" => match parse_args::<RecipePreviewArgs>(args) {
                Ok(args) => self.call_recipe_preview(args).await,
                Err(result) => result,
            },
            "self_improve_status" => match parse_args::<EmptyArgs>(args) {
                Ok(_) => {
                    self.call_self_improve_cli(
                        &["self-improve", "status", "--json"],
                        "SELF_IMPROVE_STATUS_FAILED",
                    )
                    .await
                }
                Err(result) => result,
            },
            "self_improve_objectives" => match parse_args::<EmptyArgs>(args) {
                Ok(_) => {
                    self.call_self_improve_cli(
                        &["self-improve", "objectives", "--json"],
                        "SELF_IMPROVE_OBJECTIVES_FAILED",
                    )
                    .await
                }
                Err(result) => result,
            },
            "self_improve_evaluations" => match parse_args::<EmptyArgs>(args) {
                Ok(_) => self.call_self_improve_evaluations(),
                Err(result) => result,
            },
            "self_improve_traces" => match parse_args::<EmptyArgs>(args) {
                Ok(_) => {
                    self.call_self_improve_cli(
                        &["self-improve", "traces", "--json"],
                        "SELF_IMPROVE_TRACES_FAILED",
                    )
                    .await
                }
                Err(result) => result,
            },
            "self_improve_deployments" => match parse_args::<EmptyArgs>(args) {
                Ok(_) => {
                    self.call_self_improve_cli(
                        &["self-improve", "deployments", "--json"],
                        "SELF_IMPROVE_DEPLOYMENTS_FAILED",
                    )
                    .await
                }
                Err(result) => result,
            },
            "step_status" => match parse_args::<StepStatusArgs>(args) {
                Ok(args) if args.wait_seconds <= 20 => self.call_step_status(args).await,
                Ok(_) => structured_error(
                    "INVALID_ARGUMENT",
                    "wait_seconds must be between 0 and 20",
                    false,
                ),
                Err(result) => result,
            },
            "step_start" if self.access == McpAccess::Operate => {
                match parse_args::<StepStartArgs>(args) {
                    Ok(args) => self.call_step_start(args).await,
                    Err(result) => result,
                }
            }
            "step_cancel" if self.access == McpAccess::Operate => {
                match parse_args::<StepCancelArgs>(args) {
                    Ok(args) => self.call_step_cancel(args).await,
                    Err(result) => result,
                }
            }
            "task_import_preview" if self.access == McpAccess::Operate => {
                self.call_task_import_preview(Value::Object(args))
            }
            "task_import_apply" if self.access == McpAccess::Operate => {
                match parse_args::<TaskImportApplyArgs>(args) {
                    Ok(args) => self.call_task_import_apply(args).await,
                    Err(result) => result,
                }
            }
            "commit_pending" if self.access == McpAccess::Operate => {
                match parse_args::<CommitPendingArgs>(args) {
                    Ok(args) => self.call_commit_pending(args).await,
                    Err(result) => result,
                }
            }
            "disposition" if self.access == McpAccess::Operate => {
                match parse_args::<DispositionArgs>(args) {
                    Ok(args) => self.call_disposition(args).await,
                    Err(result) => result,
                }
            }
            "recipe_approve" if self.access == McpAccess::Operate => {
                match parse_args::<RecipeApproveArgs>(args) {
                    Ok(args) => self.call_recipe_approve(args).await,
                    Err(result) => result,
                }
            }
            "recipe_reject" if self.access == McpAccess::Operate => {
                match parse_args::<RecipeRejectArgs>(args) {
                    Ok(args) => self.call_recipe_reject(args).await,
                    Err(result) => result,
                }
            }
            "recipe_reactivate" if self.access == McpAccess::Operate => {
                match parse_args::<RecipeReactivateArgs>(args) {
                    Ok(args) => self.call_recipe_reactivate(args).await,
                    Err(result) => result,
                }
            }
            "recipe_retire" if self.access == McpAccess::Operate => {
                match parse_args::<RecipeRetireArgs>(args) {
                    Ok(args) => self.call_recipe_retire(args).await,
                    Err(result) => result,
                }
            }
            "self_improve_pause" if self.access == McpAccess::Operate => {
                match parse_args::<SelfImprovePauseArgs>(args) {
                    Ok(args) => self.call_self_improve_pause(args).await,
                    Err(result) => result,
                }
            }
            "self_improve_resume" if self.access == McpAccess::Operate => {
                match parse_args::<SelfImproveResumeArgs>(args) {
                    Ok(args) => self.call_self_improve_resume(args).await,
                    Err(result) => result,
                }
            }
            "self_improve_rollback" if self.access == McpAccess::Operate => {
                match parse_args::<SelfImproveRollbackArgs>(args) {
                    Ok(args) => self.call_self_improve_rollback(args).await,
                    Err(result) => result,
                }
            }
            other => structured_error(
                "UNKNOWN_TOOL",
                &format!("unknown or unavailable tool `{other}`"),
                false,
            ),
        };
        Ok(result.into())
    }
}

/// Backward-compatible observe-only entry point. Configurable callers should
/// use [`serve_with_options`].
pub fn serve(repo: PathBuf) -> Result<()> {
    serve_with_options(repo, McpAccess::Observe, DEFAULT_JOB_TIMEOUT_SECONDS)
}

/// Run the MCP stdio server until stdin closes.
pub fn serve_with_options(
    repo: PathBuf,
    access: McpAccess,
    job_timeout_seconds: u64,
) -> Result<()> {
    let server = CodexTeamMcp::new(repo, access, job_timeout_seconds)?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("build MCP async runtime")?;
    runtime.block_on(async move {
        if access == McpAccess::Operate {
            server.recover_running_jobs_on_restart().await?;
        }
        let service = server
            .clone()
            .serve(rmcp::transport::stdio())
            .await
            .context("start MCP stdio service")?;
        let outcome = service.waiting().await.context("run MCP stdio service");
        server.shutdown_jobs().await;
        server.broker.detach().await;
        let observations = server
            .observations
            .lock()
            .map(|map| map.values().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        for observation in observations {
            observation.cancel().await;
        }
        outcome?;
        Ok(())
    })
}

pub const fn default_job_timeout_seconds() -> u64 {
    DEFAULT_JOB_TIMEOUT_SECONDS
}

fn mechanistic_tool(name: &'static str, description: &'static str) -> Tool {
    let value = ca_automation_protocol::mechanistic_request_json_schema();
    Tool::new(
        name,
        description,
        value
            .as_object()
            .expect("request schema is an object")
            .clone(),
    )
    .with_annotations(
        ToolAnnotations::new()
            .read_only(true)
            .idempotent(true)
            .destructive(false)
            .open_world(false),
    )
}
fn delegation_tools() -> Vec<Tool> {
    let job =
        json!({"type":"string", "minLength":1, "maxLength":128, "pattern":"^[A-Za-z0-9_-]+$"});
    vec![
        mechanistic_tool(
            "delegation_start",
            "Start one typed evidence chain or a local observation.",
        ),
        Tool::new(
            "delegation_status",
            "Inspect a durable delegation job.",
            object_schema(
                BTreeMap::from([
                    ("job_id", job.clone()),
                    (
                        "wait_seconds",
                        json!({"type":"integer","minimum":0,"maximum":20,"default":0}),
                    ),
                ]),
                &["job_id"],
            ),
        )
        .with_annotations(ToolAnnotations::new().read_only(true).destructive(false)),
        Tool::new(
            "delegation_cancel",
            "Detach this consumer or stop its local observer; never signal an observed process.",
            object_schema(BTreeMap::from([("job_id", job.clone())]), &["job_id"]),
        ),
        Tool::new(
            "delegation_feedback",
            "Record revision-bound feedback for one completed delegation.",
            object_schema(
                BTreeMap::from([
                    ("job_id", job),
                    (
                        "expected_revision",
                        json!({"type":"string","pattern":"^[a-f0-9]{64}$"}),
                    ),
                    ("outcome", json!({"type":"string"})),
                    ("category", json!({"type":"string"})),
                    ("human_confirmed", json!({"type":"boolean","default":false})),
                ]),
                &["job_id", "expected_revision", "outcome", "category"],
            ),
        ),
    ]
}

fn observe_tools() -> Vec<Tool> {
    let empty = object_schema(BTreeMap::new(), &[]);
    let mut recipe_list_properties = BTreeMap::new();
    recipe_list_properties.insert(
        "global",
        json!({"type": "boolean", "default": false, "description": "Include every machine-local approved revision"}),
    );
    let mut recipe_preview_properties = BTreeMap::new();
    recipe_preview_properties.insert("recipe_id", recipe_id_schema());
    let mut step_status_properties = BTreeMap::new();
    step_status_properties.insert(
        "job_id",
        json!({
            "type": "string",
            "minLength": 1,
            "maxLength": 128,
            "pattern": "^[A-Za-z0-9_-]+$",
            "description": "Exact durable MCP job identifier"
        }),
    );
    step_status_properties.insert(
        "wait_seconds",
        json!({
            "type": "integer",
            "minimum": 0,
            "maximum": 20,
            "default": 0,
            "description": "Bounded long-poll duration"
        }),
    );
    let annotations = || {
        ToolAnnotations::new()
            .read_only(true)
            .idempotent(true)
            .destructive(false)
            .open_world(false)
    };
    vec![
        Tool::new(
            "status",
            "Return observe-only orchestrator status for the pinned repository.",
            empty.clone(),
        )
        .with_annotations(annotations()),
        Tool::new(
            "doctor",
            "Return a typed readiness receipt for the pinned repository.",
            empty.clone(),
        )
        .with_annotations(annotations()),
        mechanistic_tool("repo_query", "Run bounded typed repository queries without a provider."),
        Tool::new(
            "recipe_list",
            "List learned recipe proposals, matching approved revisions, and local activation state.",
            object_schema(recipe_list_properties, &[]),
        )
        .with_annotations(annotations()),
        Tool::new(
            "recipe_preview",
            "Preview one exact learned recipe proposal without mutation or model traffic.",
            object_schema(recipe_preview_properties, &["recipe_id"]),
        )
        .with_annotations(annotations()),
        Tool::new(
            "self_improve_status",
            "Return machine-local autonomous learning, usage, pause, and A/B slot state.",
            empty.clone(),
        )
        .with_annotations(annotations()),
        Tool::new(
            "self_improve_objectives",
            "List immutable autonomous improvement objectives without model traffic.",
            empty.clone(),
        )
        .with_annotations(annotations()),
        Tool::new(
            "self_improve_evaluations",
            "List frozen baseline/candidate evaluation receipts without model traffic.",
            empty.clone(),
        )
        .with_annotations(annotations()),
        Tool::new(
            "self_improve_traces",
            "List structurally scrubbed learning traces without exposing credential fields.",
            empty.clone(),
        )
        .with_annotations(annotations()),
        Tool::new(
            "self_improve_deployments",
            "List immutable source/deployment/rollback history.",
            empty.clone(),
        )
        .with_annotations(annotations()),
        Tool::new(
            "step_status",
            "Read or bounded-long-poll one durable asynchronous step job.",
            object_schema(step_status_properties, &["job_id"]),
        )
        .with_annotations(annotations()),
    ]
}

fn operate_foundation_tools() -> Vec<Tool> {
    let import_schema = embedded_task_import_schema();
    let mut apply_properties = BTreeMap::new();
    apply_properties.insert("task", Value::Object(import_schema.as_ref().clone()));
    apply_properties.insert(
        "preview_digest",
        json!({
            "type": "string",
            "pattern": "^[0-9a-f]{64}$",
            "description": "Exact digest returned by task_import_preview"
        }),
    );
    apply_properties.insert(
        "expected_revision",
        json!({
            "type": "string",
            "pattern": "^[0-9a-f]{64}$",
            "description": "Exact revision returned by task_import_preview"
        }),
    );
    let mut start_properties = BTreeMap::new();
    start_properties.insert(
        "expected_revision",
        json!({
            "type": "string",
            "pattern": "^[0-9a-f]{64}$",
            "description": "Exact repository revision from a fresh status"
        }),
    );
    let mut cancel_properties = BTreeMap::new();
    cancel_properties.insert(
        "job_id",
        json!({
            "type": "string",
            "minLength": 1,
            "maxLength": 128,
            "pattern": "^[A-Za-z0-9_-]+$",
            "description": "Exact active MCP job identifier"
        }),
    );
    let mut commit_properties = BTreeMap::new();
    commit_properties.insert(
        "pending_id",
        json!({
            "type": "string",
            "minLength": 1,
            "maxLength": 128,
            "pattern": "^[A-Za-z0-9_-]+$"
        }),
    );
    commit_properties.insert(
        "pending_digest",
        json!({"type": "string", "pattern": "^[0-9a-f]{64}$"}),
    );
    commit_properties.insert(
        "expected_revision",
        json!({"type": "string", "pattern": "^[0-9a-f]{64}$"}),
    );
    let mut disposition_properties = BTreeMap::new();
    disposition_properties.insert(
        "decision",
        json!({
            "type": "string",
            "enum": ["preserve", "recheck", "gate", "keep-paused"]
        }),
    );
    let mut recipe_approve_properties = recipe_mutation_properties(true);
    let recipe_reject_properties = recipe_mutation_properties(false);
    let recipe_reactivate_properties = recipe_mutation_properties(true);
    let recipe_retire_properties = recipe_mutation_properties(false);
    let mut self_improve_pause_properties = BTreeMap::new();
    self_improve_pause_properties.insert(
        "reason",
        json!({"type": "string", "minLength": 1, "maxLength": 512}),
    );
    self_improve_pause_properties.insert("human_confirmed", human_confirmation_schema());
    let mut self_improve_resume_properties = BTreeMap::new();
    self_improve_resume_properties.insert(
        "expected_revision",
        json!({"type": "string", "pattern": "^[0-9a-f]{64}$"}),
    );
    self_improve_resume_properties.insert("human_confirmed", human_confirmation_schema());
    let mut self_improve_rollback_properties = self_improve_resume_properties.clone();
    self_improve_rollback_properties.insert(
        "component",
        json!({"type": "string", "enum": ["ca", "codex-team"]}),
    );
    disposition_properties.insert(
        "expected_revision",
        json!({"type": "string", "pattern": "^[0-9a-f]{64}$"}),
    );
    disposition_properties.insert(
        "pause_fingerprint",
        json!({"type": "string", "pattern": "^[0-9a-f]{64}$"}),
    );
    let read_only = ToolAnnotations::new()
        .read_only(true)
        .idempotent(true)
        .destructive(false)
        .open_world(false);
    let mutating = |idempotent| {
        ToolAnnotations::new()
            .read_only(false)
            .idempotent(idempotent)
            .destructive(false)
            .open_world(false)
    };
    vec![
        Tool::new(
            "task_import_preview",
            "Validate and digest one typed task without mutating the repository.",
            import_schema,
        )
        .with_annotations(read_only),
        Tool::new(
            "task_import_apply",
            "Apply one exact digest- and revision-bound typed task import.",
            object_schema(
                apply_properties,
                &["task", "preview_digest", "expected_revision"],
            ),
        )
        .with_annotations(mutating(false)),
        Tool::new(
            "step_start",
            "Start one revision-bound asynchronous orchestration step.",
            object_schema(start_properties, &["expected_revision"]),
        )
        .with_annotations(mutating(false)),
        Tool::new(
            "step_cancel",
            "Request cancellation of one exact asynchronous step job.",
            object_schema(cancel_properties, &["job_id"]),
        )
        .with_annotations(mutating(true)),
        Tool::new(
            "commit_pending",
            "Commit one exact, previously reviewed manual pending intent.",
            object_schema(
                commit_properties,
                &["pending_id", "pending_digest", "expected_revision"],
            ),
        )
        .with_annotations(mutating(false)),
        Tool::new(
            "disposition",
            "Record one exact human decision for the revision- and fingerprint-bound active pause.",
            object_schema(
                disposition_properties,
                &["decision", "expected_revision", "pause_fingerprint"],
            ),
        )
        .with_annotations(mutating(false)),
        Tool::new(
            "recipe_approve",
            "Validate and globally approve one exact recipe revision after explicit human confirmation.",
            object_schema(
                std::mem::take(&mut recipe_approve_properties),
                &["recipe_id", "expected_digest", "expected_revision", "human_confirmed"],
            ),
        )
        .with_annotations(mutating(false)),
        Tool::new(
            "recipe_reject",
            "Reject one exact local recipe proposal after explicit human confirmation.",
            object_schema(
                recipe_reject_properties,
                &["recipe_id", "expected_digest", "human_confirmed"],
            ),
        )
        .with_annotations(mutating(true)),
        Tool::new(
            "recipe_reactivate",
            "Revalidate and reactivate one exact non-retired recipe revision for this repository.",
            object_schema(
                recipe_reactivate_properties,
                &["recipe_id", "expected_digest", "expected_revision", "human_confirmed"],
            ),
        )
        .with_annotations(mutating(false)),
        Tool::new(
            "recipe_retire",
            "Globally retire one exact recipe revision after explicit human confirmation.",
            object_schema(
                recipe_retire_properties,
                &["recipe_id", "expected_digest", "human_confirmed"],
            ),
        )
        .with_annotations(mutating(true)),
        Tool::new(
            "self_improve_pause",
            "Persist a host-owned global pause without making a model request.",
            object_schema(self_improve_pause_properties, &["reason", "human_confirmed"]),
        )
        .with_annotations(mutating(true)),
        Tool::new(
            "self_improve_resume",
            "Resume autonomous learning from one exact reviewed supervisor revision.",
            object_schema(
                self_improve_resume_properties,
                &["expected_revision", "human_confirmed"],
            ),
        )
        .with_annotations(mutating(false)),
        Tool::new(
            "self_improve_rollback",
            "Restore one component's prior verified slot and pause for review.",
            object_schema(
                self_improve_rollback_properties,
                &["component", "expected_revision", "human_confirmed"],
            ),
        )
        .with_annotations(mutating(false)),
    ]
}

fn recipe_id_schema() -> Value {
    json!({
        "type": "string",
        "minLength": 1,
        "maxLength": 128,
        "pattern": "^[A-Za-z0-9_.-]+$"
    })
}

fn recipe_mutation_properties(include_revision: bool) -> BTreeMap<&'static str, Value> {
    let mut properties = BTreeMap::new();
    properties.insert("recipe_id", recipe_id_schema());
    properties.insert(
        "expected_digest",
        json!({"type": "string", "pattern": "^[0-9a-f]{64}$"}),
    );
    if include_revision {
        properties.insert(
            "expected_revision",
            json!({"type": "string", "pattern": "^[0-9a-f]{64}$"}),
        );
    }
    properties.insert(
        "human_confirmed",
        json!({"type": "boolean", "const": true, "description": "Set only from an explicit host-rendered human action"}),
    );
    properties
}

fn human_confirmation_schema() -> Value {
    json!({
        "type": "boolean",
        "const": true,
        "description": "Set only from an explicit host-rendered human action"
    })
}

fn mutation_result(result: Result<Value>, code: &str) -> CallToolResult {
    match result {
        Ok(value) => structured_success(value),
        Err(error) => structured_error(code, &format!("{error:#}"), false),
    }
}

fn validate_recipe_mutation_args(
    recipe_id: &str,
    expected_digest: &str,
    expected_revision: Option<&str>,
) -> Result<()> {
    if recipe_id.is_empty()
        || recipe_id.len() > 128
        || !recipe_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        bail!("recipe_id is invalid");
    }
    validate_sha256("expected_digest", expected_digest)?;
    if let Some(revision) = expected_revision {
        validate_sha256("expected_revision", revision)?;
    }
    Ok(())
}

fn embedded_task_import_schema() -> Arc<Map<String, Value>> {
    let schema: Value = serde_json::from_str(include_str!("../../schemas/task_import.schema.json"))
        .expect("embedded task import schema must be valid JSON");
    Arc::new(
        schema
            .as_object()
            .expect("embedded task import schema must be an object")
            .clone(),
    )
}

fn object_schema(properties: BTreeMap<&str, Value>, required: &[&str]) -> Arc<Map<String, Value>> {
    let mut schema = Map::new();
    schema.insert("type".to_string(), json!("object"));
    schema.insert(
        "properties".to_string(),
        Value::Object(
            properties
                .into_iter()
                .map(|(key, value)| (key.to_string(), value))
                .collect(),
        ),
    );
    schema.insert("required".to_string(), json!(required));
    schema.insert("additionalProperties".to_string(), json!(false));
    Arc::new(schema)
}

fn parse_args<T: for<'de> Deserialize<'de>>(args: Map<String, Value>) -> Result<T, CallToolResult> {
    serde_json::from_value(Value::Object(args)).map_err(|error| {
        structured_error(
            "INVALID_ARGUMENT",
            &format!("invalid tool arguments: {error}"),
            false,
        )
    })
}

fn structured_success(value: Value) -> CallToolResult {
    CallToolResult::structured(value)
}

fn structured_tool_error_value(value: Value) -> CallToolResult {
    let text = serde_json::to_string(&value).unwrap_or_else(|_| "{}".to_string());
    let mut result = CallToolResult::error(vec![ContentBlock::text(text)]);
    result.structured_content = Some(value);
    result
}

fn structured_error(code: &str, message: &str, retryable: bool) -> CallToolResult {
    structured_tool_error_value(json!({
        "schema": "codex-team.mcp_error.v1",
        "error": {
            "code": code,
            "message": bounded_message(message),
            "retryable": retryable,
        }
    }))
}

fn bounded_message(message: &str) -> String {
    const LIMIT: usize = 4096;
    if message.len() <= LIMIT {
        return message.to_string();
    }
    let mut end = LIMIT;
    while !message.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &message[..end])
}

fn validate_job_timeout(seconds: u64) -> Result<()> {
    if !(MIN_JOB_TIMEOUT_SECONDS..=MAX_JOB_TIMEOUT_SECONDS).contains(&seconds) {
        bail!(
            "job timeout must be between {MIN_JOB_TIMEOUT_SECONDS} and {MAX_JOB_TIMEOUT_SECONDS} seconds"
        );
    }
    Ok(())
}

fn pin_git_root(repo: &Path) -> Result<PathBuf> {
    let canonical = std::fs::canonicalize(repo)
        .with_context(|| format!("canonicalize MCP repository {}", repo.display()))?;
    if !canonical.is_dir() {
        bail!("MCP repository must be a directory");
    }
    if canonical.to_str().is_none() {
        bail!("MCP repository path must be valid UTF-8 for machine receipts");
    }
    let output = std::process::Command::new("git")
        .args(["-C", path_text(&canonical), "rev-parse", "--show-toplevel"])
        .output()
        .context("locate MCP repository Git root")?;
    if !output.status.success() {
        bail!("MCP repository is not a Git worktree");
    }
    if output.stdout.len() > 16 * 1024 {
        bail!("Git root output exceeded the safety limit");
    }
    let discovered = String::from_utf8(output.stdout).context("Git root was not UTF-8")?;
    let discovered =
        std::fs::canonicalize(discovered.trim()).context("canonicalize discovered Git root")?;
    if discovered != canonical {
        bail!(
            "--repo must name the canonical Git root (resolved root: {})",
            discovered.display()
        );
    }
    Ok(canonical)
}

fn validate_operate_startup(repo: &Path) -> Result<()> {
    let orch = repo.join(".orchestrator");
    let config_path = orch.join("config.toml");
    let state_path = orch.join("state.json");
    let orch_metadata = match std::fs::symlink_metadata(&orch) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            bail!("operate access requires an initialized repository")
        }
        Err(error) => return Err(error).context("inspect orchestrator runtime root"),
    };
    if orch_metadata.file_type().is_symlink() || !orch_metadata.is_dir() {
        bail!("operate runtime root must be a real, non-symlink directory");
    }
    let canonical_orch = std::fs::canonicalize(&orch)?;
    if canonical_orch != repo.join(".orchestrator") {
        bail!("operate runtime root escapes the pinned repository");
    }
    if !config_path.exists() || !state_path.exists() {
        bail!("operate access requires an initialized repository");
    }
    let metadata = std::fs::symlink_metadata(&config_path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > 1024 * 1024 {
        bail!("operate config must be a regular, non-symlink file under 1 MiB");
    }
    let state_metadata = std::fs::symlink_metadata(&state_path)?;
    if state_metadata.file_type().is_symlink()
        || !state_metadata.is_file()
        || state_metadata.len() > 4 * 1024 * 1024
    {
        bail!("operate state must be a regular, non-symlink file under 4 MiB");
    }
    if std::fs::canonicalize(&config_path)? != canonical_orch.join("config.toml")
        || std::fs::canonicalize(&state_path)? != canonical_orch.join("state.json")
    {
        bail!("operate config and state must remain under the pinned repository");
    }
    let text = std::fs::read_to_string(&config_path)?;
    let config: Config = toml::from_str(&text).context("parse operate config")?;
    if config.commit_mode != CommitMode::Manual {
        bail!("operate access requires commit_mode = \"manual\"");
    }
    if !config.require_clean_start {
        bail!("operate access requires require_clean_start = true");
    }
    config.validate_ca()?;
    if config.requires_verification_isolation() {
        crate::verification::isolation::require_isolation_backend()?;
    }
    for (role, role_config) in [
        ("master", &config.roles.master),
        ("planner", &config.roles.planner),
        ("foreman", &config.roles.foreman),
        ("worker", &config.roles.worker),
    ] {
        if !matches!(role_config.provider, RoleProvider::Codex | RoleProvider::Ca) {
            bail!(
                "operate access rejects noncancellable provider `{}` for role `{role}`",
                role_config.provider.as_str()
            );
        }
    }
    Ok(())
}

fn read_state(paths: &Paths) -> Result<State> {
    let metadata = std::fs::symlink_metadata(&paths.state)
        .with_context(|| format!("inspect orchestrator state {}", paths.state.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        bail!("orchestrator state must be a regular, non-symlink file");
    }
    if metadata.len() > 4 * 1024 * 1024 {
        bail!("orchestrator state exceeds the 4 MiB MCP safety limit");
    }
    let bytes = std::fs::read(&paths.state)?;
    serde_json::from_slice(&bytes).context("parse orchestrator state for MCP preconditions")
}

fn validate_sha256(label: &str, value: &str) -> Result<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        bail!("{label} must be a lowercase 64-character SHA-256 digest");
    }
    Ok(())
}

fn new_job_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let sequence = NEXT_JOB_ID.fetch_add(1, Ordering::Relaxed);
    format!("step-{nanos:x}-{sequence:x}")
}

#[cfg(unix)]
fn configure_process_group(command: &mut tokio::process::Command) -> Result<()> {
    command.process_group(0);
    Ok(())
}

#[cfg(not(unix))]
fn configure_process_group(_command: &mut tokio::process::Command) -> Result<()> {
    bail!("MCP operate process-group supervision is unsupported on this platform")
}

#[cfg(unix)]
fn configure_step_session(command: &mut tokio::process::Command) -> Result<()> {
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    Ok(())
}

#[cfg(not(unix))]
fn configure_step_session(_command: &mut tokio::process::Command) -> Result<()> {
    bail!("MCP operate session supervision is unsupported on this platform")
}

#[cfg(unix)]
fn signal_process_group(process_group: i32, signal: i32) -> Result<()> {
    if process_group <= 0 {
        bail!("invalid MCP process-group ID");
    }
    let result = unsafe { libc::kill(-process_group, signal) };
    if result == 0 {
        return Ok(());
    }
    let error = std::io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) {
        return Ok(());
    }
    Err(error).context("signal MCP step process group")
}

#[cfg(not(unix))]
fn signal_process_group(_process_group: i32, _signal: i32) -> Result<()> {
    bail!("MCP operate process-group supervision is unsupported on this platform")
}

struct StepJobContext {
    jobs: JobStore,
    active_jobs: ActiveJobs,
    cancellation: CancellationToken,
    process_group: i32,
    timeout: Duration,
    events_path: PathBuf,
    repo: PathBuf,
    job_id: String,
}

async fn run_step_job(
    child: &mut tokio::process::Child,
    mut record: JobRecord,
    context: &StepJobContext,
) -> Result<()> {
    let stdout = child.stdout.take().context("capture MCP step stdout")?;
    let stderr = child.stderr.take().context("capture MCP step stderr")?;
    let stdout_task = tokio::spawn(read_limited(stdout, MAX_STEP_STDOUT_BYTES));
    let stderr_task = tokio::spawn(read_limited(stderr, MAX_STEP_STDERR_BYTES));
    let started = Instant::now();
    let mut last_progress_check = Instant::now();

    let (status, state, partial, execution_error) = loop {
        if context.cancellation.is_cancelled() {
            match terminate_step_tree(child, context).await {
                Ok(status) => {
                    break (
                        status,
                        JobState::Cancelled,
                        true,
                        Some("step cancellation was requested".to_string()),
                    )
                }
                Err(error) => {
                    break (
                        None,
                        JobState::Interrupted,
                        true,
                        Some(format!("active child supervision failed: {error:#}")),
                    )
                }
            }
        }
        if started.elapsed() >= context.timeout {
            match terminate_step_tree(child, context).await {
                Ok(status) => {
                    break (
                        status,
                        JobState::TimedOut,
                        true,
                        Some(format!(
                            "step exceeded {} seconds",
                            context.timeout.as_secs()
                        )),
                    )
                }
                Err(error) => {
                    break (
                        None,
                        JobState::Interrupted,
                        true,
                        Some(format!("active child supervision failed: {error:#}")),
                    )
                }
            }
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                let state = if status.success() {
                    JobState::Completed
                } else {
                    JobState::Failed
                };
                match terminate_step_tree(child, context).await {
                    Ok(terminated) => break (terminated.or(Some(status)), state, false, None),
                    Err(error) => {
                        break (
                            Some(status),
                            JobState::Interrupted,
                            true,
                            Some(format!("active child supervision failed: {error:#}")),
                        )
                    }
                }
            }
            Ok(None) => {
                let active_child = read_validated_active_child(context)?;
                if record.active_child_process_group != active_child {
                    record.active_child_process_group = active_child;
                    record.updated_at = chrono::Utc::now().to_rfc3339();
                    context.jobs.save_atomic(&record)?;
                }
                if last_progress_check.elapsed() >= Duration::from_millis(500) {
                    if let Some(progress) = event_progress(&context.events_path, &record.started_at)
                    {
                        if record.progress.as_ref() != Some(&progress) {
                            record.progress = Some(progress);
                            record.updated_at = chrono::Utc::now().to_rfc3339();
                            let _ = context.jobs.save_atomic(&record);
                        }
                    }
                    last_progress_check = Instant::now();
                }
                tokio::time::sleep(Duration::from_millis(100)).await
            }
            Err(error) => {
                let _ = terminate_step_tree(child, context).await;
                break (
                    None,
                    JobState::Interrupted,
                    true,
                    Some(format!("failed to wait for step process: {error}")),
                );
            }
        }
    };

    let stdout = match stdout_task.await {
        Ok(Ok(bytes)) => bytes,
        Ok(Err(error)) => {
            record.status = JobState::Failed;
            record.error = Some(bounded_message(&format!("step stdout failed: {error:#}")));
            Vec::new()
        }
        Err(error) => {
            record.status = JobState::Failed;
            record.error = Some(bounded_message(&format!(
                "step stdout task failed: {error}"
            )));
            Vec::new()
        }
    };
    let stderr = match stderr_task.await {
        Ok(Ok(bytes)) => bytes,
        Ok(Err(error)) => {
            record.status = JobState::Failed;
            record.error = Some(bounded_message(&format!("step stderr failed: {error:#}")));
            Vec::new()
        }
        Err(error) => {
            record.status = JobState::Failed;
            record.error = Some(bounded_message(&format!(
                "step stderr task failed: {error}"
            )));
            Vec::new()
        }
    };

    if record.error.is_none() {
        record.status = state;
        record.error = execution_error.map(|error| bounded_message(&error));
        if matches!(state, JobState::Completed | JobState::Failed) {
            match serde_json::from_slice::<Value>(&stdout) {
                Ok(value) if value.is_object() => record.cycle_receipt = Some(value),
                Ok(_) => {
                    record.status = JobState::Failed;
                    record.error = Some("step receipt was not a JSON object".to_string());
                }
                Err(error) => {
                    record.status = JobState::Failed;
                    record.error = Some(bounded_message(&format!(
                        "step emitted malformed JSON receipt: {error}"
                    )));
                }
            }
            if status.is_some_and(|status| !status.success()) && record.error.is_none() {
                record.error = Some(format!(
                    "step process failed: {}",
                    sanitized_stderr(&stderr)
                ));
            }
        }
    }
    record.repository_may_be_partial = partial
        || matches!(
            record.status,
            JobState::Failed | JobState::Cancelled | JobState::TimedOut | JobState::Interrupted
        );
    record.outer_process_group = None;
    record.active_child_process_group = None;
    record.updated_at = chrono::Utc::now().to_rfc3339();
    record.progress = Some(JobProgress {
        phase: "finalizing".to_string(),
        summary: format!("step finished with status {:?}", record.status).to_lowercase(),
    });
    clear_active_child_bridge(&context.repo, &record.job_id)?;
    let save_result = context.jobs.save_atomic(&record);
    let _ = context.active_jobs.remove(&record.job_id);
    save_result?;
    let _ = context.jobs.prune_terminal();
    Ok(())
}

fn event_progress(path: &Path, started_at: &str) -> Option<JobProgress> {
    const TAIL_BYTES: u64 = 64 * 1024;
    let metadata = std::fs::symlink_metadata(path).ok()?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return None;
    }
    let mut file = std::fs::File::open(path).ok()?;
    let offset = metadata.len().saturating_sub(TAIL_BYTES);
    file.seek(SeekFrom::Start(offset)).ok()?;
    let mut bytes = Vec::new();
    file.take(TAIL_BYTES).read_to_end(&mut bytes).ok()?;
    let text = String::from_utf8(bytes).ok()?;
    let mut lines = text.lines();
    if offset > 0 {
        lines.next();
    }
    let lines = lines.collect::<Vec<_>>();
    for line in lines.into_iter().rev() {
        let Ok(event) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let Some(timestamp) = event.get("timestamp").and_then(Value::as_str) else {
            continue;
        };
        if timestamp < started_at {
            continue;
        }
        let Some(kind) = event.get("kind").and_then(Value::as_str) else {
            continue;
        };
        let phase = match kind {
            "taskSelected" | "affectedWorkComputed" => "selecting",
            "promptReceiptLogged" | "routeAttemptRecorded" => "role_execution",
            "recipePlanned" | "recipeStarted" | "recipeFinished" | "diagnosticsParsed"
            | "findingsFiltered" | "evidenceSummarized" => "verification",
            "guardChecked" => "guard",
            "seniorReviewRequested" | "autoAccepted" => "review",
            "commitCreated" => "pending_commit",
            "stateTransitionApplied"
                if event.pointer("/payload/transition").and_then(Value::as_str)
                    == Some("cycle_receipt") =>
            {
                "finalizing"
            }
            _ => continue,
        };
        return Some(JobProgress {
            phase: phase.to_string(),
            summary: format!("orchestrator entered {phase}"),
        });
    }
    None
}

async fn terminate_child_group(
    child: &mut tokio::process::Child,
    process_group: i32,
) -> Option<std::process::ExitStatus> {
    let mut child_status = child.try_wait().ok().flatten();
    if child_status.is_some() && !process_group_exists(process_group) {
        return child_status;
    }
    let _ = signal_process_group(process_group, libc::SIGTERM);
    let deadline = Instant::now() + Duration::from_secs(CANCELLATION_GRACE_SECONDS);
    loop {
        if child_status.is_none() {
            child_status = child.try_wait().ok().flatten();
        }
        if child_status.is_some() && !process_group_exists(process_group) {
            return child_status;
        }
        if Instant::now() >= deadline {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let _ = signal_process_group(process_group, libc::SIGKILL);
    if child_status.is_none() {
        let _ = child.start_kill();
        child_status = tokio::time::timeout(Duration::from_secs(2), child.wait())
            .await
            .ok()
            .and_then(Result::ok);
    }
    let hard_kill_deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < hard_kill_deadline && process_group_exists(process_group) {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    child_status
}

/// Terminate the outer `step` group and every separately grouped role or
/// verification child published through the per-job bridge. The marker is
/// re-read throughout the grace period so a child registered concurrently with
/// cancellation is still discovered before the hard-kill fallback.
async fn terminate_step_tree(
    child: &mut tokio::process::Child,
    context: &StepJobContext,
) -> Result<Option<std::process::ExitStatus>> {
    let mut child_status = child.try_wait().ok().flatten();
    let mut nested_groups = BTreeSet::new();
    let mut bridge_error = discover_nested_group(context, &mut nested_groups).err();
    for process_group in &nested_groups {
        let _ = signal_nested_process_group(*process_group, libc::SIGTERM);
    }
    let _ = signal_process_group(context.process_group, libc::SIGTERM);

    let deadline = Instant::now() + Duration::from_secs(CANCELLATION_GRACE_SECONDS);
    loop {
        if child_status.is_none() {
            child_status = child.try_wait().ok().flatten();
        }
        let previous_len = nested_groups.len();
        if bridge_error.is_none() {
            bridge_error = discover_nested_group(context, &mut nested_groups).err();
        }
        if nested_groups.len() != previous_len {
            for process_group in &nested_groups {
                let _ = signal_nested_process_group(*process_group, libc::SIGTERM);
            }
        }
        let nested_alive = nested_groups.iter().copied().any(process_group_exists);
        if child_status.is_some() && !process_group_exists(context.process_group) && !nested_alive {
            if let Err(error) = clear_active_child_bridge(&context.repo, &context.job_id) {
                bridge_error.get_or_insert(error);
            }
            return match bridge_error {
                Some(error) => Err(error).context("validate MCP active child bridge"),
                None => Ok(child_status),
            };
        }
        if Instant::now() >= deadline {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    if bridge_error.is_none() {
        bridge_error = discover_nested_group(context, &mut nested_groups).err();
    }
    for process_group in &nested_groups {
        let _ = signal_nested_process_group(*process_group, libc::SIGKILL);
    }
    let _ = signal_process_group(context.process_group, libc::SIGKILL);
    if child_status.is_none() {
        child_status = child.wait().await.ok();
    }
    let hard_kill_deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < hard_kill_deadline {
        if bridge_error.is_none() {
            bridge_error = discover_nested_group(context, &mut nested_groups).err();
        }
        let all_gone = !process_group_exists(context.process_group)
            && !nested_groups.iter().copied().any(process_group_exists);
        if all_gone {
            break;
        }
        for process_group in &nested_groups {
            let _ = signal_nested_process_group(*process_group, libc::SIGKILL);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    if let Err(error) = clear_active_child_bridge(&context.repo, &context.job_id) {
        bridge_error.get_or_insert(error);
    }
    match bridge_error {
        Some(error) => Err(error).context("validate MCP active child bridge"),
        None => Ok(child_status),
    }
}

fn discover_nested_group(context: &StepJobContext, groups: &mut BTreeSet<i32>) -> Result<()> {
    let Some(evidence) = read_active_child_process_group(&context.repo, &context.job_id)? else {
        return Ok(());
    };
    // Once this exact group has been OS-validated inside the step session, keep
    // trusting that identity through termination. Its group leader can exit
    // before TERM-ignoring descendants, making a later `getsid(pgid)` return
    // ESRCH even though the already-validated group still needs SIGKILL.
    if groups.contains(&evidence.process_group) {
        return Ok(());
    }
    validate_nested_process_evidence(&evidence, context.process_group)?;
    if evidence.process_group != context.process_group {
        groups.insert(evidence.process_group);
    }
    Ok(())
}

fn read_validated_active_child(context: &StepJobContext) -> Result<Option<i32>> {
    let Some(evidence) = read_active_child_process_group(&context.repo, &context.job_id)? else {
        return Ok(None);
    };
    validate_nested_process_evidence(&evidence, context.process_group)?;
    Ok(Some(evidence.process_group))
}

#[cfg(unix)]
fn validate_nested_process_evidence(
    evidence: &ActiveChildProcessEvidence,
    outer_session: i32,
) -> Result<()> {
    if evidence.owner_process_id != outer_session as u32 {
        bail!("active child marker owner does not match the MCP step process");
    }
    if !nested_process_group_is_safe(evidence.process_group)
        || evidence.process_group == outer_session
    {
        bail!("active child marker names an unsafe process group");
    }
    let session = unsafe { libc::getsid(evidence.process_group) };
    if session < 0 {
        return Err(std::io::Error::last_os_error())
            .context("validate active child process session");
    }
    if session != outer_session {
        bail!("active child marker process group is outside the MCP step session");
    }
    Ok(())
}

#[cfg(not(unix))]
fn validate_nested_process_evidence(
    _evidence: &ActiveChildProcessEvidence,
    _outer_session: i32,
) -> Result<()> {
    bail!("nested MCP process validation is unsupported on this platform")
}

#[cfg(unix)]
fn nested_process_group_is_safe(process_group: i32) -> bool {
    process_group > 0 && process_group != unsafe { libc::getpgrp() }
}

#[cfg(not(unix))]
fn nested_process_group_is_safe(_process_group: i32) -> bool {
    false
}

fn signal_nested_process_group(process_group: i32, signal: i32) -> Result<()> {
    if !nested_process_group_is_safe(process_group) {
        bail!("refusing to signal an unsafe nested MCP process group");
    }
    signal_process_group(process_group, signal)
}

#[cfg(unix)]
#[derive(Debug)]
struct ExpectedStepLaunch {
    executable: PathBuf,
    executable_device: u64,
    executable_inode: u64,
    cwd: PathBuf,
    argv: Vec<Vec<u8>>,
    job_marker: Vec<u8>,
    nonce_marker: Vec<u8>,
    argv_provenance_bound: bool,
}

#[cfg(unix)]
#[derive(Debug)]
struct LiveProcessSnapshot {
    start_token: String,
    executable: PathBuf,
    executable_device: u64,
    executable_inode: u64,
    cwd: PathBuf,
    argv: Vec<Vec<u8>>,
    environment: Option<Vec<Vec<u8>>>,
}

#[cfg(target_os = "macos")]
type ProcessArguments = (Vec<Vec<u8>>, Vec<Vec<u8>>);

#[cfg(unix)]
impl ExpectedStepLaunch {
    fn new(executable: &Path, repo: &Path, record: &JobRecord, spawn_nonce: &str) -> Result<Self> {
        use std::os::unix::ffi::OsStrExt as _;
        use std::os::unix::fs::MetadataExt as _;

        let invoked_executable = executable.to_path_buf();
        let executable = std::fs::canonicalize(executable)
            .context("canonicalize the MCP step executable for provenance")?;
        let process_label = step_process_label(&record.job_id, spawn_nonce);
        let (kernel_executable, argv_prefix, argv_provenance_bound) =
            expected_kernel_launch(&executable, &invoked_executable, &process_label)?;
        let metadata = std::fs::metadata(&kernel_executable)
            .context("inspect the MCP step executable for provenance")?;
        if !metadata.is_file() {
            bail!("MCP step provenance executable is not a regular file");
        }
        let mut argv = argv_prefix;
        argv.extend([
            b"step".to_vec(),
            b"--repo".to_vec(),
            repo.as_os_str().as_bytes().to_vec(),
            b"--json".to_vec(),
            b"--expected-revision".to_vec(),
            record.expected_revision.as_bytes().to_vec(),
        ]);
        Ok(Self {
            executable: kernel_executable,
            executable_device: metadata.dev(),
            executable_inode: metadata.ino(),
            cwd: repo.to_path_buf(),
            argv,
            job_marker: format!("{MCP_JOB_ID_ENV}={}", record.job_id).into_bytes(),
            nonce_marker: format!("{MCP_STEP_PROVENANCE_ENV}={spawn_nonce}").into_bytes(),
            argv_provenance_bound,
        })
    }

    fn validate(&self, snapshot: &LiveProcessSnapshot) -> Result<()> {
        if snapshot.executable != self.executable
            || snapshot.executable_device != self.executable_device
            || snapshot.executable_inode != self.executable_inode
        {
            bail!("live process executable does not match the MCP step launch");
        }
        if snapshot.cwd != self.cwd {
            bail!("live process working directory does not match the pinned repository");
        }
        if snapshot.argv != self.argv {
            bail!("live process arguments do not match the exact MCP step command");
        }
        if let Some(environment) = &snapshot.environment {
            let matching_job_markers = environment
                .iter()
                .filter(|entry| entry.as_slice() == self.job_marker)
                .count();
            let matching_nonce_markers = environment
                .iter()
                .filter(|entry| entry.as_slice() == self.nonce_marker)
                .count();
            if matching_job_markers != 1 || matching_nonce_markers != 1 {
                bail!("live process does not carry the exact MCP step provenance markers");
            }
        }
        Ok(())
    }
}

#[cfg(unix)]
fn expected_kernel_launch(
    executable: &Path,
    invoked_executable: &Path,
    process_label: &str,
) -> Result<(PathBuf, Vec<Vec<u8>>, bool)> {
    use std::os::unix::ffi::OsStrExt as _;

    let mut prefix = [0_u8; 512];
    let mut file = std::fs::File::open(executable)?;
    let count = file.read(&mut prefix)?;
    if !prefix[..count].starts_with(b"#!") {
        return Ok((
            executable.to_path_buf(),
            vec![process_label.as_bytes().to_vec()],
            true,
        ));
    }
    let line_end = prefix[..count]
        .iter()
        .position(|byte| *byte == b'\n')
        .unwrap_or(count);
    let line = &prefix[2..line_end];
    if line.iter().any(|byte| !byte.is_ascii()) {
        bail!("MCP step script has a non-ASCII interpreter directive");
    }
    let fields = line
        .split(|byte| byte.is_ascii_whitespace())
        .filter(|field| !field.is_empty())
        .collect::<Vec<_>>();
    if fields.is_empty() || fields.len() > 2 || !fields[0].starts_with(b"/") {
        bail!("MCP step script must name one absolute interpreter and at most one argument");
    }
    let interpreter = PathBuf::from(std::ffi::OsStr::from_bytes(fields[0]));
    let canonical_interpreter = std::fs::canonicalize(&interpreter)
        .context("canonicalize the MCP step script interpreter")?;
    let mut argv = vec![interpreter.as_os_str().as_bytes().to_vec()];
    if fields.len() == 2 {
        argv.push(fields[1].to_vec());
    }
    argv.push(invoked_executable.as_os_str().as_bytes().to_vec());
    Ok((canonical_interpreter, argv, false))
}

fn step_process_label(job_id: &str, spawn_nonce: &str) -> String {
    format!("codex-team-mcp-step:{job_id}:{spawn_nonce}")
}

#[cfg(unix)]
struct StepLaunchBarrier {
    child_read: Option<std::os::fd::OwnedFd>,
    release_write: Option<std::fs::File>,
}

#[cfg(unix)]
impl StepLaunchBarrier {
    fn new() -> Result<Self> {
        use std::os::fd::FromRawFd as _;

        let mut descriptors = [-1_i32; 2];
        if unsafe { libc::pipe(descriptors.as_mut_ptr()) } != 0 {
            return Err(std::io::Error::last_os_error()).context("create MCP step launch barrier");
        }
        let child_read = unsafe { std::os::fd::OwnedFd::from_raw_fd(descriptors[0]) };
        let release_write = unsafe { std::os::fd::OwnedFd::from_raw_fd(descriptors[1]) };
        set_close_on_exec(&child_read, false)?;
        set_close_on_exec(&release_write, true)?;
        Ok(Self {
            child_read: Some(child_read),
            release_write: Some(std::fs::File::from(release_write)),
        })
    }

    fn configure_child(&self, command: &mut tokio::process::Command) {
        use std::os::fd::AsRawFd as _;

        let descriptor = self
            .child_read
            .as_ref()
            .expect("launch barrier read end exists before spawn")
            .as_raw_fd();
        command.env(MCP_STEP_BARRIER_FD_ENV, descriptor.to_string());
    }

    #[cfg(test)]
    fn configure_test_child(&self, command: &mut std::process::Command) {
        use std::os::fd::AsRawFd as _;

        let descriptor = self
            .child_read
            .as_ref()
            .expect("launch barrier read end exists before spawn")
            .as_raw_fd();
        command.env(MCP_STEP_BARRIER_FD_ENV, descriptor.to_string());
    }

    fn child_spawned(&mut self) {
        self.child_read.take();
    }

    fn release(&mut self) -> Result<()> {
        let mut release_write = self
            .release_write
            .take()
            .context("MCP step launch barrier was already released")?;
        release_write
            .write_all(&[MCP_STEP_BARRIER_RELEASE])
            .context("write MCP step launch barrier release")?;
        drop(release_write);
        Ok(())
    }
}

#[cfg(unix)]
fn set_close_on_exec(descriptor: &std::os::fd::OwnedFd, close_on_exec: bool) -> Result<()> {
    use std::os::fd::AsRawFd as _;

    let raw = descriptor.as_raw_fd();
    let flags = unsafe { libc::fcntl(raw, libc::F_GETFD) };
    if flags < 0 {
        return Err(std::io::Error::last_os_error())
            .context("inspect MCP barrier descriptor flags");
    }
    let updated = if close_on_exec {
        flags | libc::FD_CLOEXEC
    } else {
        flags & !libc::FD_CLOEXEC
    };
    if unsafe { libc::fcntl(raw, libc::F_SETFD, updated) } < 0 {
        return Err(std::io::Error::last_os_error()).context("set MCP barrier descriptor flags");
    }
    Ok(())
}

#[cfg(not(unix))]
struct StepLaunchBarrier;

#[cfg(not(unix))]
impl StepLaunchBarrier {
    fn new() -> Result<Self> {
        bail!("MCP step launch barriers are unsupported on this platform")
    }

    fn configure_child(&self, _command: &mut tokio::process::Command) {}

    fn child_spawned(&mut self) {}

    fn release(&mut self) -> Result<()> {
        bail!("MCP step launch barriers are unsupported on this platform")
    }
}

/// Block an MCP-launched `step` before repository path construction, config
/// loading, lock acquisition, or any mutation. The parent releases exactly one
/// byte only after the live PID/start/argv provenance is durable. If the MCP
/// server dies first, its write end closes and this returns an error without
/// entering the conductor.
pub(crate) fn await_step_launch_barrier() -> Result<()> {
    let Some(raw_descriptor) = std::env::var_os(MCP_STEP_BARRIER_FD_ENV) else {
        return Ok(());
    };
    std::env::remove_var(MCP_STEP_BARRIER_FD_ENV);
    await_step_launch_barrier_inner(&raw_descriptor)
}

#[cfg(unix)]
fn await_step_launch_barrier_inner(raw_descriptor: &std::ffi::OsStr) -> Result<()> {
    use std::os::fd::FromRawFd as _;

    let descriptor = raw_descriptor
        .to_str()
        .context("MCP step launch barrier descriptor was not valid UTF-8")?
        .parse::<i32>()
        .context("MCP step launch barrier descriptor was not an integer")?;
    if descriptor <= libc::STDERR_FILENO || descriptor > 1_048_576 {
        bail!("MCP step launch barrier descriptor is outside the safe range");
    }
    let mut stat = std::mem::MaybeUninit::<libc::stat>::zeroed();
    if unsafe { libc::fstat(descriptor, stat.as_mut_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error())
            .context("inspect MCP step launch barrier descriptor");
    }
    let stat = unsafe { stat.assume_init() };
    if stat.st_mode & libc::S_IFMT != libc::S_IFIFO {
        bail!("MCP step launch barrier descriptor is not a pipe");
    }
    let mut read_end = unsafe { std::fs::File::from_raw_fd(descriptor) };
    let mut release = [0_u8; 1];
    read_end
        .read_exact(&mut release)
        .context("MCP step launch barrier closed before durable authorization")?;
    if release[0] != MCP_STEP_BARRIER_RELEASE {
        bail!("MCP step launch barrier returned an invalid authorization byte");
    }
    Ok(())
}

#[cfg(not(unix))]
fn await_step_launch_barrier_inner(_raw_descriptor: &std::ffi::OsStr) -> Result<()> {
    bail!("MCP step launch barriers are unsupported on this platform")
}

#[cfg(unix)]
fn configure_step_process_label(
    command: &mut tokio::process::Command,
    job_id: &str,
    spawn_nonce: &str,
) {
    command.arg0(step_process_label(job_id, spawn_nonce));
}

#[cfg(not(unix))]
fn configure_step_process_label(
    _command: &mut tokio::process::Command,
    _job_id: &str,
    _spawn_nonce: &str,
) {
}

#[cfg(unix)]
fn new_step_spawn_nonce() -> Result<String> {
    let mut bytes = [0_u8; 32];
    let result = unsafe { libc::getentropy(bytes.as_mut_ptr().cast(), bytes.len()) };
    if result != 0 {
        return Err(std::io::Error::last_os_error())
            .context("generate MCP step process provenance nonce");
    }
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        write!(&mut encoded, "{byte:02x}").expect("writing to a String cannot fail");
    }
    Ok(encoded)
}

#[cfg(not(unix))]
fn new_step_spawn_nonce() -> Result<String> {
    bail!("MCP step process provenance is unsupported on this platform")
}

#[cfg(unix)]
async fn capture_step_process_provenance(
    process_group: i32,
    executable: &Path,
    repo: &Path,
    record: &JobRecord,
    spawn_nonce: &str,
) -> Result<StepProcessProvenance> {
    let expected = ExpectedStepLaunch::new(executable, repo, record, spawn_nonce)?;
    let mut last_error = None;
    for _ in 0..20 {
        match inspect_live_process(process_group).and_then(|snapshot| {
            expected.validate(&snapshot)?;
            Ok(snapshot)
        }) {
            Ok(snapshot) => {
                return Ok(StepProcessProvenance {
                    schema: STEP_PROCESS_PROVENANCE_SCHEMA.to_string(),
                    process_id: u32::try_from(process_group)
                        .context("convert MCP step process ID")?,
                    start_token: snapshot.start_token,
                    spawn_nonce: spawn_nonce.to_string(),
                });
            }
            Err(error) => last_error = Some(error),
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("MCP step process inspection failed")))
}

#[cfg(not(unix))]
async fn capture_step_process_provenance(
    _process_group: i32,
    _executable: &Path,
    _repo: &Path,
    _record: &JobRecord,
    _spawn_nonce: &str,
) -> Result<StepProcessProvenance> {
    bail!("MCP step process provenance is unsupported on this platform")
}

#[cfg(unix)]
fn validate_recovered_outer_process(
    executable: &Path,
    repo: &Path,
    record: &JobRecord,
    process_group: i32,
) -> Result<()> {
    validate_recovered_outer_session(process_group)?;
    let provenance = record
        .process_provenance
        .as_ref()
        .context("running MCP job lacks durable step process provenance")?;
    if provenance.process_id != u32::try_from(process_group).unwrap_or_default() {
        bail!("running MCP job process provenance does not match its process group");
    }
    let expected = ExpectedStepLaunch::new(executable, repo, record, &provenance.spawn_nonce)?;
    let snapshot = inspect_live_process(process_group)?;
    expected.validate(&snapshot)?;
    if !expected.argv_provenance_bound && snapshot.environment.is_none() {
        bail!("live MCP step lacks an OS-visible per-spawn provenance marker");
    }
    if snapshot.start_token != provenance.start_token {
        bail!("live MCP step start identity does not match its durable provenance");
    }
    Ok(())
}

#[cfg(not(unix))]
fn validate_recovered_outer_process(
    _executable: &Path,
    _repo: &Path,
    _record: &JobRecord,
    _process_group: i32,
) -> Result<()> {
    bail!("MCP restart process provenance is unsupported on this platform")
}

#[cfg(target_os = "macos")]
fn inspect_live_process(process_id: i32) -> Result<LiveProcessSnapshot> {
    use std::ffi::CStr;
    use std::os::unix::ffi::OsStringExt as _;
    use std::os::unix::fs::MetadataExt as _;

    if process_id <= 0 {
        bail!("invalid MCP step process ID for provenance inspection");
    }
    let mut bsd_info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::zeroed();
    let bsd_size = std::mem::size_of::<libc::proc_bsdinfo>();
    let bsd_read = unsafe {
        libc::proc_pidinfo(
            process_id,
            libc::PROC_PIDTBSDINFO,
            0,
            bsd_info.as_mut_ptr().cast(),
            i32::try_from(bsd_size).expect("proc_bsdinfo fits in i32"),
        )
    };
    if bsd_read != i32::try_from(bsd_size).expect("proc_bsdinfo fits in i32") {
        return Err(std::io::Error::last_os_error()).context("inspect MCP step BSD identity");
    }
    let bsd_info = unsafe { bsd_info.assume_init() };
    if bsd_info.pbi_pid != u32::try_from(process_id).unwrap_or_default()
        || bsd_info.pbi_pgid != u32::try_from(process_id).unwrap_or_default()
        || bsd_info.pbi_uid != unsafe { libc::geteuid() }
    {
        bail!("live process does not have the expected MCP step ownership and group identity");
    }

    let mut executable_buffer = vec![0_u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    let executable_len = unsafe {
        libc::proc_pidpath(
            process_id,
            executable_buffer.as_mut_ptr().cast(),
            u32::try_from(executable_buffer.len()).expect("process path buffer fits in u32"),
        )
    };
    if executable_len <= 0 {
        return Err(std::io::Error::last_os_error()).context("inspect MCP step executable path");
    }
    let executable = CStr::from_bytes_until_nul(&executable_buffer)
        .context("MCP step executable path was not NUL terminated")?;
    let executable = std::fs::canonicalize(PathBuf::from(std::ffi::OsString::from_vec(
        executable.to_bytes().to_vec(),
    )))
    .context("canonicalize live MCP step executable")?;
    let executable_metadata = std::fs::metadata(&executable)?;

    let mut vnode_info = std::mem::MaybeUninit::<libc::proc_vnodepathinfo>::zeroed();
    let vnode_size = std::mem::size_of::<libc::proc_vnodepathinfo>();
    let vnode_read = unsafe {
        libc::proc_pidinfo(
            process_id,
            libc::PROC_PIDVNODEPATHINFO,
            0,
            vnode_info.as_mut_ptr().cast(),
            i32::try_from(vnode_size).expect("proc_vnodepathinfo fits in i32"),
        )
    };
    if vnode_read != i32::try_from(vnode_size).expect("proc_vnodepathinfo fits in i32") {
        return Err(std::io::Error::last_os_error()).context("inspect MCP step working directory");
    }
    let vnode_info = unsafe { vnode_info.assume_init() };
    let cwd_bytes = unsafe {
        std::slice::from_raw_parts(
            vnode_info.pvi_cdir.vip_path.as_ptr().cast::<u8>(),
            libc::MAXPATHLEN as usize,
        )
    };
    let cwd_end = cwd_bytes
        .iter()
        .position(|byte| *byte == 0)
        .context("MCP step working directory was not NUL terminated")?;
    let cwd = std::fs::canonicalize(PathBuf::from(std::ffi::OsString::from_vec(
        cwd_bytes[..cwd_end].to_vec(),
    )))
    .context("canonicalize live MCP step working directory")?;
    let (argv, environment) = macos_process_arguments(process_id)?;

    Ok(LiveProcessSnapshot {
        start_token: format!(
            "macos:{}:{}",
            bsd_info.pbi_start_tvsec, bsd_info.pbi_start_tvusec
        ),
        executable,
        executable_device: executable_metadata.dev(),
        executable_inode: executable_metadata.ino(),
        cwd,
        argv,
        environment: (!environment.is_empty()).then_some(environment),
    })
}

#[cfg(target_os = "macos")]
fn macos_process_arguments(process_id: i32) -> Result<ProcessArguments> {
    let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, process_id];
    let mut size = 0_usize;
    let size_result = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            u32::try_from(mib.len()).expect("MCP process sysctl MIB fits in u32"),
            std::ptr::null_mut(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if size_result != 0 {
        return Err(std::io::Error::last_os_error()).context("size MCP step process arguments");
    }
    if !(std::mem::size_of::<i32>()..=MAX_PROCESS_INSPECTION_BYTES).contains(&size) {
        bail!("MCP step process arguments exceeded the bounded inspection limit");
    }
    let mut bytes = vec![0_u8; size];
    let read_result = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            u32::try_from(mib.len()).expect("MCP process sysctl MIB fits in u32"),
            bytes.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if read_result != 0 {
        return Err(std::io::Error::last_os_error()).context("read MCP step process arguments");
    }
    if size > bytes.len() {
        bail!("MCP step process argument size changed during inspection");
    }
    bytes.truncate(size);
    if bytes.len() < std::mem::size_of::<i32>() {
        bail!("MCP step process argument buffer was truncated");
    }
    let argc = i32::from_ne_bytes(bytes[..4].try_into().expect("four-byte argc"));
    let argc = usize::try_from(argc).context("MCP step process had a negative argument count")?;
    if argc == 0 || argc > 128 {
        bail!("MCP step process had an invalid argument count");
    }
    let mut cursor = 4;
    cursor = next_nul(&bytes, cursor)? + 1;
    while cursor < bytes.len() && bytes[cursor] == 0 {
        cursor += 1;
    }
    let mut argv = Vec::with_capacity(argc);
    for _ in 0..argc {
        let end = next_nul(&bytes, cursor)?;
        argv.push(bytes[cursor..end].to_vec());
        cursor = end + 1;
    }
    let mut environment = Vec::new();
    while cursor < bytes.len() {
        if bytes[cursor] == 0 {
            cursor += 1;
            continue;
        }
        let end = next_nul(&bytes, cursor)?;
        environment.push(bytes[cursor..end].to_vec());
        if environment.len() > 4_096 {
            bail!("MCP step process environment exceeded the inspection entry limit");
        }
        cursor = end + 1;
    }
    Ok((argv, environment))
}

#[cfg(target_os = "macos")]
fn next_nul(bytes: &[u8], start: usize) -> Result<usize> {
    bytes
        .get(start..)
        .and_then(|suffix| suffix.iter().position(|byte| *byte == 0))
        .map(|offset| start + offset)
        .context("MCP step process argument buffer was not NUL terminated")
}

#[cfg(target_os = "linux")]
fn inspect_live_process(process_id: i32) -> Result<LiveProcessSnapshot> {
    use std::os::unix::ffi::OsStringExt as _;
    use std::os::unix::fs::MetadataExt as _;

    if process_id <= 0 {
        bail!("invalid MCP step process ID for provenance inspection");
    }
    let proc_root = PathBuf::from(format!("/proc/{process_id}"));
    let proc_metadata = std::fs::metadata(&proc_root)?;
    if proc_metadata.uid() != unsafe { libc::geteuid() } {
        bail!("live process is not owned by the MCP server user");
    }
    let stat = read_bounded_process_file(&proc_root.join("stat"), 64 * 1024)?;
    let stat = std::str::from_utf8(&stat).context("MCP step stat was not UTF-8")?;
    let after_name = stat
        .rfind(')')
        .and_then(|index| stat.get(index + 1..))
        .context("MCP step stat was malformed")?;
    let fields = after_name.split_whitespace().collect::<Vec<_>>();
    if fields.len() <= 19 {
        bail!("MCP step stat was truncated");
    }
    let process_group = fields[2].parse::<i32>()?;
    let session = fields[3].parse::<i32>()?;
    if process_group != process_id || session != process_id {
        bail!("live process is not the expected MCP step session leader");
    }
    let executable_link = proc_root.join("exe");
    let executable = std::fs::canonicalize(std::fs::read_link(&executable_link)?)
        .context("canonicalize live MCP step executable")?;
    let executable_metadata = std::fs::metadata(&executable_link)?;
    let cwd = std::fs::canonicalize(std::fs::read_link(proc_root.join("cwd"))?)
        .context("canonicalize live MCP step working directory")?;
    let argv = split_nul_entries(&read_bounded_process_file(
        &proc_root.join("cmdline"),
        MAX_PROCESS_INSPECTION_BYTES,
    )?)?;
    let environment = split_nul_entries(&read_bounded_process_file(
        &proc_root.join("environ"),
        MAX_PROCESS_INSPECTION_BYTES,
    )?)?;
    Ok(LiveProcessSnapshot {
        start_token: format!("linux:{}", fields[19]),
        executable,
        executable_device: executable_metadata.dev(),
        executable_inode: executable_metadata.ino(),
        cwd,
        argv,
        environment: Some(environment),
    })
}

#[cfg(target_os = "linux")]
fn read_bounded_process_file(path: &Path, limit: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take((limit + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        bail!("MCP step process data exceeded the bounded inspection limit");
    }
    Ok(bytes)
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
fn split_nul_entries(bytes: &[u8]) -> Result<Vec<Vec<u8>>> {
    if bytes.is_empty() || *bytes.last().unwrap_or(&1) != 0 {
        bail!("MCP step process data was not NUL terminated");
    }
    let entries = bytes[..bytes.len() - 1]
        .split(|byte| *byte == 0)
        .map(<[u8]>::to_vec)
        .collect::<Vec<_>>();
    if entries.len() > 4_096 {
        bail!("MCP step process data exceeded the inspection entry limit");
    }
    Ok(entries)
}

#[cfg(target_os = "freebsd")]
fn inspect_live_process(process_id: i32) -> Result<LiveProcessSnapshot> {
    use std::os::unix::ffi::OsStringExt as _;
    use std::os::unix::fs::MetadataExt as _;

    let identity = freebsd_process_identity(process_id)?;
    let executable_bytes = freebsd_process_data(process_id, libc::KERN_PROC_PATHNAME)?;
    let executable = std::ffi::CStr::from_bytes_with_nul(&executable_bytes)
        .context("MCP step executable path was not NUL terminated")?;
    let executable = std::fs::canonicalize(PathBuf::from(std::ffi::OsString::from_vec(
        executable.to_bytes().to_vec(),
    )))
    .context("canonicalize live MCP step executable")?;
    let executable_metadata = std::fs::metadata(&executable)?;

    // KERN_PROC_CWD copies out one variable-length kinfo_file, ending after its
    // NUL-terminated path. Validate its fixed header before reading that path.
    let cwd_bytes = freebsd_process_data(process_id, libc::KERN_PROC_CWD)?;
    let path_offset = std::mem::offset_of!(libc::kinfo_file, kf_path);
    if cwd_bytes.len() <= path_offset || cwd_bytes.len() > std::mem::size_of::<libc::kinfo_file>() {
        bail!("MCP step working directory record had an invalid size");
    }
    let field = |offset: usize| -> i32 {
        i32::from_ne_bytes(
            cwd_bytes[offset..offset + 4]
                .try_into()
                .expect("validated kinfo_file header"),
        )
    };
    if usize::try_from(field(std::mem::offset_of!(libc::kinfo_file, kf_structsize)))?
        != cwd_bytes.len()
        || field(std::mem::offset_of!(libc::kinfo_file, kf_type)) != libc::KF_TYPE_VNODE
        || field(std::mem::offset_of!(libc::kinfo_file, kf_fd)) != libc::KF_FD_TYPE_CWD
    {
        bail!("MCP step working directory record did not describe its current directory");
    }
    let cwd = std::ffi::CStr::from_bytes_until_nul(&cwd_bytes[path_offset..])
        .context("MCP step working directory was not NUL terminated")?;
    let cwd = std::fs::canonicalize(PathBuf::from(std::ffi::OsString::from_vec(
        cwd.to_bytes().to_vec(),
    )))
    .context("canonicalize live MCP step working directory")?;
    let argv = split_nul_entries(&freebsd_process_data(process_id, libc::KERN_PROC_ARGS)?)?;
    let environment = split_nul_entries(&freebsd_process_data(process_id, libc::KERN_PROC_ENV)?)?;
    if freebsd_process_identity(process_id)? != identity {
        bail!("MCP step process identity changed during provenance inspection");
    }
    Ok(LiveProcessSnapshot {
        start_token: format!("freebsd:{}:{}", identity.0, identity.1),
        executable,
        executable_device: executable_metadata.dev(),
        executable_inode: executable_metadata.ino(),
        cwd,
        argv,
        environment: Some(environment),
    })
}

#[cfg(target_os = "freebsd")]
fn freebsd_process_identity(process_id: i32) -> Result<(libc::time_t, libc::suseconds_t)> {
    if process_id <= 0 {
        bail!("invalid MCP step process ID for provenance inspection");
    }
    let mut mib = [
        libc::CTL_KERN,
        libc::KERN_PROC,
        libc::KERN_PROC_PID,
        process_id,
    ];
    let mut info = std::mem::MaybeUninit::<libc::kinfo_proc>::zeroed();
    let mut size = std::mem::size_of::<libc::kinfo_proc>();
    let result = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            mib.len() as u32,
            info.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if result != 0 {
        return Err(std::io::Error::last_os_error()).context("inspect MCP step FreeBSD identity");
    }
    if size != std::mem::size_of::<libc::kinfo_proc>() {
        bail!("MCP step FreeBSD identity record was truncated");
    }
    let info = unsafe { info.assume_init() };
    if info.ki_structsize != i32::try_from(size)?
        || info.ki_pid != process_id
        || info.ki_pgid != process_id
        || info.ki_sid != process_id
        || info.ki_uid != unsafe { libc::geteuid() }
    {
        bail!("live process does not have the expected MCP step ownership and session identity");
    }
    Ok((info.ki_start.tv_sec, info.ki_start.tv_usec))
}

#[cfg(target_os = "freebsd")]
fn freebsd_process_data(process_id: i32, selector: i32) -> Result<Vec<u8>> {
    let mut mib = [libc::CTL_KERN, libc::KERN_PROC, selector, process_id];
    let mut bytes = vec![0_u8; MAX_PROCESS_INSPECTION_BYTES];
    let mut size = bytes.len();
    let result = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            mib.len() as u32,
            bytes.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if result != 0 {
        return Err(std::io::Error::last_os_error())
            .context("inspect bounded MCP step process data");
    }
    if size == 0 || size > bytes.len() {
        bail!("MCP step process data had an invalid bounded size");
    }
    bytes.truncate(size);
    Ok(bytes)
}

#[cfg(all(
    unix,
    not(any(target_os = "linux", target_os = "macos", target_os = "freebsd"))
))]
fn inspect_live_process(_process_id: i32) -> Result<LiveProcessSnapshot> {
    bail!("MCP step process inspection is unsupported on this Unix platform")
}

#[cfg(unix)]
fn validate_recovered_outer_session(process_group: i32) -> Result<()> {
    if process_group <= 0 || process_group == unsafe { libc::getpgrp() } {
        bail!("recovered MCP step names an unsafe process group");
    }
    let session = unsafe { libc::getsid(process_group) };
    if session < 0 {
        return Err(std::io::Error::last_os_error()).context("inspect recovered MCP step session");
    }
    if session != process_group {
        bail!("recovered MCP step is not its recorded session leader");
    }
    Ok(())
}

#[cfg(not(unix))]
fn validate_recovered_outer_session(_process_group: i32) -> Result<()> {
    bail!("MCP restart recovery is unsupported on this platform")
}

fn discover_recovered_nested_group(
    repo: &Path,
    record: &JobRecord,
    outer_session: i32,
    groups: &mut BTreeSet<i32>,
) -> Result<()> {
    let Some(evidence) = read_active_child_process_group(repo, &record.job_id)? else {
        return Ok(());
    };
    if groups.contains(&evidence.process_group) {
        return Ok(());
    }
    validate_nested_process_evidence(&evidence, outer_session)?;
    groups.insert(evidence.process_group);
    Ok(())
}

async fn terminate_recovered_step_tree(
    repo: &Path,
    executable: &Path,
    record: &JobRecord,
    outer_session: i32,
) -> Result<()> {
    validate_recovered_outer_process(executable, repo, record, outer_session)?;
    let mut nested_groups = BTreeSet::new();
    discover_recovered_nested_group(repo, record, outer_session, &mut nested_groups)?;
    for process_group in &nested_groups {
        signal_nested_process_group(*process_group, libc::SIGTERM)?;
    }
    signal_process_group(outer_session, libc::SIGTERM)?;

    let deadline = Instant::now() + Duration::from_secs(CANCELLATION_GRACE_SECONDS);
    loop {
        let previous_len = nested_groups.len();
        discover_recovered_nested_group(repo, record, outer_session, &mut nested_groups)?;
        if nested_groups.len() != previous_len {
            for process_group in &nested_groups {
                signal_nested_process_group(*process_group, libc::SIGTERM)?;
            }
        }
        let outer_alive = process_group_exists(outer_session);
        let nested_alive = nested_groups.iter().copied().any(process_group_exists);
        if !outer_alive && !nested_alive {
            return Ok(());
        }
        if Instant::now() >= deadline {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    discover_recovered_nested_group(repo, record, outer_session, &mut nested_groups)?;
    for process_group in &nested_groups {
        signal_nested_process_group(*process_group, libc::SIGKILL)?;
    }
    signal_process_group(outer_session, libc::SIGKILL)?;
    let hard_kill_deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < hard_kill_deadline {
        let outer_alive = process_group_exists(outer_session);
        let nested_alive = nested_groups.iter().copied().any(process_group_exists);
        if !outer_alive && !nested_alive {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    bail!("recovered MCP step process tree remained alive after hard cancellation")
}

#[cfg(unix)]
fn process_group_exists(process_group: i32) -> bool {
    if process_group <= 0 {
        return false;
    }
    let result = unsafe { libc::kill(-process_group, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
}

#[cfg(not(unix))]
fn process_group_exists(_process_group: i32) -> bool {
    false
}

#[derive(Debug)]
struct BoundedOutput {
    success: bool,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

const QUERY_STREAM_DRAIN_GRACE: Duration = Duration::from_secs(2);

async fn run_bounded(
    executable: &Path,
    current_dir: &Path,
    args: &[&str],
    timeout: Duration,
    stdout_limit: usize,
    stderr_limit: usize,
    stdin: Option<Vec<u8>>,
) -> Result<BoundedOutput> {
    let mut command = tokio::process::Command::new(executable);
    command
        .args(args)
        .current_dir(current_dir)
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    configure_process_group(&mut command)?;
    let mut child = command
        .spawn()
        .with_context(|| format!("launch {}", executable.display()))?;
    #[cfg(unix)]
    let process_group = child
        .id()
        .and_then(|pid| i32::try_from(pid).ok())
        .context("query process did not have a valid process-group ID")?;
    let stdin_task = if let Some(bytes) = stdin {
        let mut writer = child.stdin.take().context("open query stdin")?;
        Some(tokio::spawn(async move {
            use tokio::io::AsyncWriteExt as _;
            writer.write_all(&bytes).await?;
            writer.shutdown().await
        }))
    } else {
        None
    };
    let stdout = child.stdout.take().context("capture query stdout")?;
    let stderr = child.stderr.take().context("capture query stderr")?;
    let stdout_task = tokio::spawn(read_limited(stdout, stdout_limit));
    let stderr_task = tokio::spawn(read_limited(stderr, stderr_limit));

    let (status, timed_out) = match tokio::time::timeout(timeout, child.wait()).await {
        Ok(status) => (Some(status.context("wait for query process")?), false),
        Err(_) => {
            #[cfg(unix)]
            let _ = terminate_child_group(&mut child, process_group).await;
            #[cfg(not(unix))]
            {
                let _ = child.start_kill();
                let _ = child.wait().await;
            }
            (None, true)
        }
    };
    #[cfg(unix)]
    let status = terminate_child_group(&mut child, process_group)
        .await
        .or(status);
    let (stdout, stderr, stdin_result) = tokio::join!(
        join_query_io_task(stdout_task, "stdout reader"),
        join_query_io_task(stderr_task, "stderr reader"),
        async {
            match stdin_task {
                Some(task) => join_query_io_task(task, "stdin writer").await.map(Some),
                None => Ok(None),
            }
        }
    );
    let stdout = stdout?;
    let stderr = stderr?;
    stdin_result?;
    if timed_out {
        bail!("codex-team query exceeded the 60 second timeout");
    }
    let status = status.context("query process did not return an exit status")?;
    Ok(BoundedOutput {
        success: status.success(),
        stdout,
        stderr,
    })
}

async fn join_query_io_task<T, E>(
    mut task: tokio::task::JoinHandle<std::result::Result<T, E>>,
    label: &str,
) -> Result<T>
where
    E: Into<anyhow::Error>,
{
    match tokio::time::timeout(QUERY_STREAM_DRAIN_GRACE, &mut task).await {
        Ok(result) => result
            .with_context(|| format!("join query {label}"))?
            .map_err(Into::into)
            .with_context(|| format!("complete query {label}")),
        Err(_) => {
            task.abort();
            let _ = task.await;
            bail!("query {label} remained open after process cleanup")
        }
    }
}

async fn read_limited<R: tokio::io::AsyncRead + Unpin>(reader: R, limit: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .await?;
    if bytes.len() > limit {
        bail!("child output exceeded its safety limit");
    }
    Ok(bytes)
}

fn sanitized_stderr(stderr: &[u8]) -> String {
    if stderr.is_empty() {
        "no diagnostic provided".to_string()
    } else {
        format!("redacted diagnostic ({} bytes)", stderr.len())
    }
}

fn path_text(path: &Path) -> &str {
    path.to_str()
        .expect("MCP repository path was validated as UTF-8")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::CallToolRequestParams;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    const BARRIER_TEST_CANARY_ENV: &str = "CODEX_TEAM_MCP_BARRIER_TEST_CANARY";

    #[cfg(unix)]
    struct TestProcessReaper {
        process_group: i32,
        join: Option<std::thread::JoinHandle<std::io::Result<std::process::ExitStatus>>>,
    }

    #[cfg(unix)]
    impl TestProcessReaper {
        fn new(mut child: std::process::Child) -> Result<Self> {
            let process_group = i32::try_from(child.id())?;
            let join = std::thread::spawn(move || child.wait());
            Ok(Self {
                process_group,
                join: Some(join),
            })
        }

        fn process_group(&self) -> i32 {
            self.process_group
        }

        fn wait(mut self) -> Result<std::process::ExitStatus> {
            let join = self
                .join
                .take()
                .context("test process reaper was already consumed")?;
            join.join()
                .map_err(|_| anyhow::anyhow!("test process reaper thread panicked"))?
                .context("wait for test process")
        }
    }

    #[cfg(unix)]
    impl Drop for TestProcessReaper {
        fn drop(&mut self) {
            let Some(join) = self.join.take() else {
                return;
            };
            let _ = signal_process_group(self.process_group, libc::SIGKILL);
            let _ = join.join();
        }
    }

    #[cfg(unix)]
    #[test]
    fn restart_fixture_reaper_kills_and_waits_when_a_test_exits_early() -> Result<()> {
        use std::os::unix::process::CommandExt as _;

        let mut command = std::process::Command::new("sh");
        command
            .arg("-c")
            .arg("trap '' TERM HUP INT; while :; do sleep 30; done")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0);
        let child = command.spawn()?;
        let process_group;
        {
            let reaper = TestProcessReaper::new(child)?;
            process_group = reaper.process_group();
        }
        assert!(
            !process_group_exists(process_group),
            "test cleanup guard left process group {process_group} behind"
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn mcp_pre_release_barrier_helper() -> Result<()> {
        let Some(canary) = std::env::var_os(BARRIER_TEST_CANARY_ENV) else {
            return Ok(());
        };
        await_step_launch_barrier()?;
        fs::write(PathBuf::from(canary), b"mutation after release\n")?;
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn mcp_nested_process_helper() -> Result<()> {
        use std::os::unix::process::CommandExt as _;

        if std::env::var_os(MCP_JOB_ID_ENV).is_none() {
            return Ok(());
        }
        let repo = std::env::current_dir()?;
        let paths = Paths::new(&repo)?;
        let mut command = std::process::Command::new("sh");
        command
            .arg("-c")
            .arg("trap '' TERM; while :; do sleep 30; done")
            .process_group(0);
        let mut nested = command.spawn()?;
        let _registration =
            crate::cancellation::ActiveChildProcessGroup::register(&paths, nested.id())?;
        fs::write(
            repo.join(".orchestrator/nested-process-group"),
            nested.id().to_string(),
        )?;
        let _ = nested.wait();
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn mcp_query_escaped_pipe_helper() -> Result<()> {
        let cwd = std::env::current_dir()?;
        if !cwd.join(".enable-query-escaped-pipe-helper").exists() {
            return Ok(());
        }
        let mut ready = [0_i32; 2];
        if unsafe { libc::pipe(ready.as_mut_ptr()) } != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let pid = unsafe { libc::fork() };
        if pid < 0 {
            unsafe {
                libc::close(ready[0]);
                libc::close(ready[1]);
            }
            return Err(std::io::Error::last_os_error().into());
        }
        if pid == 0 {
            unsafe {
                libc::close(ready[0]);
                if libc::setsid() < 0 {
                    libc::_exit(2);
                }
                libc::signal(libc::SIGTERM, libc::SIG_IGN);
                libc::signal(libc::SIGHUP, libc::SIG_IGN);
                let _ = libc::write(ready[1], b"1".as_ptr().cast(), 1);
                libc::close(ready[1]);
                loop {
                    libc::sleep(30);
                }
            }
        }
        unsafe {
            libc::close(ready[1]);
            let mut byte = [0_u8; 1];
            let read = libc::read(ready[0], byte.as_mut_ptr().cast(), 1);
            libc::close(ready[0]);
            if read != 1 {
                let _ = libc::kill(pid, libc::SIGKILL);
                return Err(std::io::Error::last_os_error().into());
            }
        }
        fs::write(cwd.join("query-escaped-pipe-holder.pid"), pid.to_string())?;
        Ok(())
    }

    fn temp_git_repo(name: &str) -> Result<PathBuf> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let repo = std::env::temp_dir().join(format!("codex-team-mcp-{name}-{nonce}"));
        fs::create_dir_all(&repo)?;
        let status = std::process::Command::new("git")
            .args(["-C", path_text(&repo), "init", "--quiet"])
            .status()?;
        assert!(status.success());
        for args in [
            vec!["config", "user.email", "mcp-test@example.invalid"],
            vec!["config", "user.name", "MCP Test"],
        ] {
            assert!(std::process::Command::new("git")
                .args(["-C", path_text(&repo)])
                .args(args)
                .status()?
                .success());
        }
        fs::write(repo.join("tracked.txt"), "baseline\n")?;
        assert!(std::process::Command::new("git")
            .args(["-C", path_text(&repo), "add", "tracked.txt"])
            .status()?
            .success());
        assert!(std::process::Command::new("git")
            .args([
                "-C",
                path_text(&repo),
                "commit",
                "--quiet",
                "-m",
                "baseline"
            ])
            .status()?
            .success());
        Ok(repo)
    }

    fn prepare_operate_repo(repo: &Path) -> Result<()> {
        crate::verification::isolation::enable_test_isolation_backend();
        fs::create_dir_all(repo.join(".orchestrator"))?;
        fs::write(
            repo.join(".orchestrator/config.toml"),
            "commit_mode = \"manual\"\nrequire_clean_start = true\n",
        )?;
        fs::write(
            repo.join(".orchestrator/state.json"),
            serde_json::to_vec_pretty(&State::default())?,
        )?;
        Ok(())
    }

    #[test]
    fn observe_catalog_is_exact_and_closed() -> Result<()> {
        let repo = temp_git_repo("catalog")?;
        let server = CodexTeamMcp::new(repo.clone(), McpAccess::Observe, 7_200)?;
        let tools = server.tool_catalog();
        let names: Vec<&str> = tools.iter().map(|tool| tool.name.as_ref()).collect();
        assert_eq!(
            names,
            [
                "status",
                "doctor",
                "repo_query",
                "recipe_list",
                "recipe_preview",
                "self_improve_status",
                "self_improve_objectives",
                "self_improve_evaluations",
                "self_improve_traces",
                "self_improve_deployments",
                "step_status"
            ]
        );
        for tool in tools {
            assert_eq!(
                tool.input_schema.get("additionalProperties"),
                Some(&Value::Bool(false))
            );
            assert_eq!(
                tool.annotations
                    .as_ref()
                    .and_then(|value| value.read_only_hint),
                Some(true)
            );
        }
        fs::remove_dir_all(repo)?;
        Ok(())
    }

    #[test]
    fn pinning_rejects_a_git_subdirectory() -> Result<()> {
        let repo = temp_git_repo("pin")?;
        let child = repo.join("child");
        fs::create_dir(&child)?;
        assert!(pin_git_root(&child).is_err());
        fs::remove_dir_all(repo)?;
        Ok(())
    }

    #[test]
    fn operate_requires_manual_clean_start_config() -> Result<()> {
        crate::verification::isolation::enable_test_isolation_backend();
        let repo = temp_git_repo("operate")?;
        fs::create_dir(repo.join(".orchestrator"))?;
        fs::write(
            repo.join(".orchestrator/state.json"),
            serde_json::to_vec_pretty(&State::default())?,
        )?;
        fs::write(
            repo.join(".orchestrator/config.toml"),
            "commit_mode = \"auto\"\n",
        )?;
        assert!(CodexTeamMcp::new(repo.clone(), McpAccess::Operate, 7_200).is_err());
        fs::write(
            repo.join(".orchestrator/config.toml"),
            "commit_mode = \"manual\"\nrequire_clean_start = true\n",
        )?;
        assert!(CodexTeamMcp::new(repo.clone(), McpAccess::Operate, 7_200).is_ok());
        fs::remove_dir_all(repo)?;
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn operate_rejects_symlinked_orchestrator_runtime_root() -> Result<()> {
        let repo = temp_git_repo("operate-symlink-root")?;
        let outside = repo.parent().unwrap().join(format!(
            "codex-team-mcp-operate-outside-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&outside)?;
        fs::write(
            outside.join("config.toml"),
            "commit_mode = \"manual\"\nrequire_clean_start = true\n",
        )?;
        fs::write(
            outside.join("state.json"),
            serde_json::to_vec_pretty(&State::default())?,
        )?;
        std::os::unix::fs::symlink(&outside, repo.join(".orchestrator"))?;
        assert!(CodexTeamMcp::new(repo.clone(), McpAccess::Operate, 7_200).is_err());
        fs::remove_dir_all(repo)?;
        fs::remove_dir_all(outside)?;
        Ok(())
    }

    #[tokio::test]
    async fn sdk_negotiates_tools_resources_and_structured_errors() -> Result<()> {
        let repo = temp_git_repo("sdk")?;
        let server = CodexTeamMcp::new(repo.clone(), McpAccess::Observe, 7_200)?;
        let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
        let server_task = tokio::spawn(async move {
            server.serve(server_transport).await?.waiting().await?;
            anyhow::Ok(())
        });
        let client = ().serve(client_transport).await?;
        let tools = client.list_tools(None).await?;
        let names: Vec<&str> = tools.tools.iter().map(|tool| tool.name.as_ref()).collect();
        assert_eq!(
            names,
            [
                "status",
                "doctor",
                "repo_query",
                "recipe_list",
                "recipe_preview",
                "self_improve_status",
                "self_improve_objectives",
                "self_improve_evaluations",
                "self_improve_traces",
                "self_improve_deployments",
                "step_status"
            ]
        );
        let resources = client.list_resources(None).await?;
        assert!(resources.resources.is_empty());
        let unknown = client.call_tool(CallToolRequestParams::new("step")).await?;
        assert_eq!(unknown.is_error, Some(true));
        assert_eq!(
            unknown.structured_content.as_ref().unwrap()["error"]["code"],
            "UNKNOWN_TOOL"
        );
        client.cancel().await?;
        server_task.await??;
        fs::remove_dir_all(repo)?;
        Ok(())
    }

    #[tokio::test]
    async fn operate_preview_is_structured_and_mutation_free() -> Result<()> {
        let repo = temp_git_repo("preview")?;
        prepare_operate_repo(&repo)?;
        let paths = Paths::new(&repo)?;
        let revision_before = repository_revision(&paths)?;
        let head = crate::revision::git_head(&paths)?.unwrap();
        let server = CodexTeamMcp::new(repo.clone(), McpAccess::Operate, 7_200)?;
        let (server_transport, client_transport) = tokio::io::duplex(128 * 1024);
        let server_task = tokio::spawn(async move {
            server.serve(server_transport).await?.waiting().await?;
            anyhow::Ok(())
        });
        let client = ().serve(client_transport).await?;
        let tools = client.list_tools(None).await?;
        let names = tools
            .tools
            .iter()
            .map(|tool| tool.name.as_ref())
            .collect::<Vec<_>>();
        for required in [
            "task_import_preview",
            "task_import_apply",
            "step_start",
            "step_cancel",
            "commit_pending",
            "disposition",
            "recipe_approve",
            "recipe_reject",
            "recipe_reactivate",
            "recipe_retire",
            "self_improve_pause",
            "self_improve_resume",
            "self_improve_rollback",
        ] {
            assert!(names.contains(&required));
        }
        assert!(tools.tools.iter().all(|tool| {
            tool.input_schema.get("additionalProperties") == Some(&Value::Bool(false))
        }));
        let task = json!({
            "schema": "codex-team.task_import.v1",
            "source": "ca",
            "base_head": head,
            "task": {
                "task_id": "INT-148",
                "title": "MCP preview test",
                "kind": "protocol",
                "objective": "Prove the preview contract",
                "why_unblocks_progress": "It validates the MCP boundary",
                "allowed_files": ["src/mcp/server.rs"],
                "read_only_files": ["src/mcp/mod.rs"],
                "forbidden_actions": ["No runtime configuration mutation"],
                "steps": ["Preview"],
                "acceptance_criteria": ["Typed receipt"],
                "verification_commands": ["cargo fmt --all --check"],
                "stop_conditions": ["Stop on workspace escape"],
                "discovery_budget": "Read named files only",
                "estimated_minutes": 30
            }
        });
        let preview = client
            .call_tool(
                CallToolRequestParams::new("task_import_preview")
                    .with_arguments(task.as_object().unwrap().clone()),
            )
            .await?;
        assert_eq!(preview.is_error, Some(false));
        assert_eq!(
            preview.structured_content.as_ref().unwrap()["schema"],
            "codex-team.task_import_preview.v1"
        );
        assert_eq!(repository_revision(&paths)?, revision_before);
        let denied = client
            .call_tool(
                CallToolRequestParams::new("recipe_approve").with_arguments(
                    json!({
                        "recipe_id": "learned-test",
                        "expected_digest": "a".repeat(64),
                        "expected_revision": revision_before,
                        "human_confirmed": false
                    })
                    .as_object()
                    .unwrap()
                    .clone(),
                ),
            )
            .await?;
        assert_eq!(denied.is_error, Some(true));
        assert_eq!(
            denied.structured_content.as_ref().unwrap()["error"]["code"],
            "HUMAN_CONFIRMATION_REQUIRED"
        );
        let self_improve_denied = client
            .call_tool(
                CallToolRequestParams::new("self_improve_pause").with_arguments(
                    json!({
                        "reason": "test must not mutate",
                        "human_confirmed": false
                    })
                    .as_object()
                    .unwrap()
                    .clone(),
                ),
            )
            .await?;
        assert_eq!(self_improve_denied.is_error, Some(true));
        assert_eq!(
            self_improve_denied.structured_content.as_ref().unwrap()["error"]["code"],
            "HUMAN_CONFIRMATION_REQUIRED"
        );
        assert_eq!(repository_revision(&paths)?, revision_before);
        client.cancel().await?;
        server_task.await??;
        fs::remove_dir_all(repo)?;
        Ok(())
    }

    #[cfg(unix)]
    fn test_shell() -> &'static str {
        if cfg!(target_os = "freebsd") {
            "/bin/sh"
        } else {
            "/bin/bash"
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn asynchronous_step_persists_a_terminal_receipt() -> Result<()> {
        use std::os::unix::fs::PermissionsExt as _;

        let repo = temp_git_repo("async-step")?;
        prepare_operate_repo(&repo)?;
        let fake = repo.join(".orchestrator/fake-codex-team");
        fs::write(
            &fake,
            format!(
                "#!{}\nprintf '%s\\n' '{{\"protocol_version\":1,\"outcome\":\"no_work\"}}'\n",
                test_shell()
            ),
        )?;
        let mut permissions = fs::metadata(&fake)?.permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&fake, permissions)?;
        let paths = Paths::new(&repo)?;
        let revision = repository_revision(&paths)?;
        let server = CodexTeamMcp::new(repo.clone(), McpAccess::Operate, 60)?.with_executable(fake);
        let started = server
            .start_step(StepStartArgs {
                expected_revision: revision.clone(),
            })
            .await?;
        let job_id = started["job_id"].as_str().unwrap();
        let record = server.jobs.wait(job_id, 5).await?;
        assert_eq!(record.status, JobState::Completed);
        assert_eq!(record.cycle_receipt.unwrap()["outcome"], "no_work");
        assert!(!record.repository_may_be_partial);
        fs::remove_dir_all(repo)?;
        Ok(())
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn step_cancel_terminates_the_process_group() -> Result<()> {
        use std::os::unix::fs::PermissionsExt as _;

        let repo = temp_git_repo("cancel-step")?;
        prepare_operate_repo(&repo)?;
        let fake = repo.join(".orchestrator/fake-slow-codex-team");
        fs::write(&fake, format!("#!{}\nsleep 30 &\nwait $!\n", test_shell()))?;
        let mut permissions = fs::metadata(&fake)?.permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&fake, permissions)?;
        let paths = Paths::new(&repo)?;
        let revision = repository_revision(&paths)?;
        let server = CodexTeamMcp::new(repo.clone(), McpAccess::Operate, 60)?.with_executable(fake);
        let started = server
            .start_step(StepStartArgs {
                expected_revision: revision.clone(),
            })
            .await?;
        let job_id = started["job_id"].as_str().unwrap();
        let running = server.jobs.load(job_id).await?;
        let process_group = running.outer_process_group.unwrap();
        assert!(server
            .start_step(StepStartArgs {
                expected_revision: running.expected_revision.clone(),
            })
            .await
            .is_err());
        let cancelled = server.cancel_step(job_id).await?;
        assert_eq!(cancelled["status"], "cancellation_requested");
        let terminal = server.jobs.wait(job_id, 5).await?;
        assert_eq!(terminal.status, JobState::Cancelled);
        assert!(terminal.repository_may_be_partial);
        let fresh_revision = repository_revision(&paths)?;
        assert_ne!(fresh_revision, revision);
        assert!(server
            .start_step(StepStartArgs {
                expected_revision: revision,
            })
            .await
            .is_err());
        let deadline = Instant::now() + Duration::from_secs(2);
        while unsafe { libc::kill(-process_group, 0) } == 0 && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert_ne!(unsafe { libc::kill(-process_group, 0) }, 0);
        fs::remove_dir_all(repo)?;
        Ok(())
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn hard_fallback_reaps_a_separate_term_ignoring_nested_group() -> Result<()> {
        use std::os::unix::fs::PermissionsExt as _;

        let repo = temp_git_repo("nested-hard-kill")?;
        prepare_operate_repo(&repo)?;
        let fake = repo.join(".orchestrator/fake-nested-codex-team");
        let test_binary = std::env::current_exe()?;
        let quoted_binary = format!("'{}'", path_text(&test_binary).replace('\'', "'\\''"));
        fs::write(
            &fake,
            format!(
                "#!{}\nsleep 1\nexec {quoted_binary} --exact mcp::server::tests::mcp_nested_process_helper --nocapture\n", test_shell()
            ),
        )?;
        let mut permissions = fs::metadata(&fake)?.permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&fake, permissions)?;
        let paths = Paths::new(&repo)?;
        let revision = repository_revision(&paths)?;
        let server = CodexTeamMcp::new(repo.clone(), McpAccess::Operate, 60)?.with_executable(fake);
        let started = server
            .start_step(StepStartArgs {
                expected_revision: revision,
            })
            .await?;
        let job_id = started["job_id"].as_str().unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let nested_group = loop {
            if let Ok(text) = fs::read_to_string(repo.join(".orchestrator/nested-process-group")) {
                let process_group = text.trim().parse::<i32>()?;
                let record = server.jobs.load(job_id).await?;
                if record.active_child_process_group == Some(process_group) {
                    break process_group;
                }
            }
            if Instant::now() >= deadline {
                bail!("nested process group was not published to durable job evidence");
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        };
        server.cancel_step(job_id).await?;
        let terminal = server.jobs.wait(job_id, 20).await?;
        assert_eq!(terminal.status, JobState::Cancelled);
        assert!(terminal.repository_may_be_partial);
        assert_eq!(terminal.active_child_process_group, None);
        assert!(
            !process_group_exists(nested_group),
            "hard fallback left nested process group {nested_group} alive"
        );
        fs::remove_dir_all(repo)?;
        Ok(())
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn forged_marker_outside_step_session_is_rejected_without_signaling() -> Result<()> {
        use std::os::unix::fs::PermissionsExt as _;
        use std::os::unix::process::CommandExt as _;

        let repo = temp_git_repo("forged-marker")?;
        prepare_operate_repo(&repo)?;
        let fake = repo.join(".orchestrator/fake-slow-codex-team");
        fs::write(
            &fake,
            format!("#!{}\nwhile :; do sleep 30; done\n", test_shell()),
        )?;
        let mut permissions = fs::metadata(&fake)?.permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&fake, permissions)?;
        let paths = Paths::new(&repo)?;
        let revision = repository_revision(&paths)?;
        let server = CodexTeamMcp::new(repo.clone(), McpAccess::Operate, 60)?.with_executable(fake);
        let started = server
            .start_step(StepStartArgs {
                expected_revision: revision,
            })
            .await?;
        let job_id = started["job_id"].as_str().unwrap();
        let running = server.jobs.load(job_id).await?;
        let outer = running.outer_process_group.unwrap();

        let mut outsider_command = std::process::Command::new("sh");
        outsider_command
            .arg("-c")
            .arg("trap '' TERM; while :; do sleep 30; done")
            .process_group(0);
        let mut outsider = outsider_command.spawn()?;
        let outsider_group = i32::try_from(outsider.id())?;
        let marker = repo
            .join(".orchestrator/mcp/active")
            .join(format!("{job_id}.json"));
        fs::write(
            &marker,
            serde_json::to_vec(&json!({
                "schema": "codex-team.mcp_active_child.v1",
                "job_id": job_id,
                "process_group": outsider_group,
                "owner_process_id": outer as u32,
            }))?,
        )?;

        let terminal = server.jobs.wait(job_id, 5).await?;
        assert_eq!(terminal.status, JobState::Interrupted);
        assert!(terminal.repository_may_be_partial);
        assert!(process_group_exists(outsider_group));
        let _ = signal_process_group(outsider_group, libc::SIGKILL);
        let _ = outsider.wait();
        fs::remove_dir_all(repo)?;
        Ok(())
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn server_shutdown_cancels_and_reaps_an_active_step_group() -> Result<()> {
        use std::os::unix::fs::PermissionsExt as _;

        let repo = temp_git_repo("shutdown-step")?;
        prepare_operate_repo(&repo)?;
        let fake = repo.join(".orchestrator/fake-shutdown-codex-team");
        fs::write(&fake, format!("#!{}\nsleep 30 &\nwait $!\n", test_shell()))?;
        let mut permissions = fs::metadata(&fake)?.permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&fake, permissions)?;
        let paths = Paths::new(&repo)?;
        let revision = repository_revision(&paths)?;
        let server = CodexTeamMcp::new(repo.clone(), McpAccess::Operate, 60)?.with_executable(fake);
        let started = server
            .start_step(StepStartArgs {
                expected_revision: revision,
            })
            .await?;
        let job_id = started["job_id"].as_str().unwrap();
        let running = server.jobs.load(job_id).await?;
        let process_group = running.outer_process_group.unwrap();

        server.shutdown_jobs().await;

        let terminal = server.jobs.load(job_id).await?;
        assert_eq!(terminal.status, JobState::Cancelled);
        assert!(terminal.repository_may_be_partial);
        assert!(server.active_jobs.is_empty()?);
        assert!(
            !process_group_exists(process_group),
            "server shutdown left step process group {process_group} behind"
        );

        fs::remove_dir_all(repo)?;
        Ok(())
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn restart_terminates_and_records_a_still_alive_orphaned_step() -> Result<()> {
        use std::os::unix::process::CommandExt as _;

        let repo = temp_git_repo("restart-alive-step")?;
        prepare_operate_repo(&repo)?;
        let executable = PathBuf::from(test_shell());
        let server = CodexTeamMcp::new(repo.clone(), McpAccess::Operate, 60)?
            .with_executable(executable.clone());
        let pinned_repo = server.repo.clone();
        fs::write(
            pinned_repo.join("step"),
            "trap 'exit 0' TERM HUP INT\nwhile :; do sleep 30; done\n",
        )?;

        let spawn_nonce = new_step_spawn_nonce()?;
        let now = chrono::Utc::now().to_rfc3339();
        let mut record = JobRecord {
            schema: JOB_SCHEMA.to_string(),
            job_id: "restart-alive-job".to_string(),
            kind: crate::mcp::jobs::JobKind::Step,
            status: JobState::Running,
            expected_revision: "a".repeat(64),
            started_at: now.clone(),
            updated_at: now,
            outer_process_group: None,
            launch_nonce: Some(spawn_nonce.clone()),
            process_provenance: None,
            active_child_process_group: None,
            progress: None,
            cycle_receipt: None,
            mechanistic_receipt: None,
            delegation_feedback: None,
            delegation_manifest_digest: None,
            error: None,
            repository_may_be_partial: false,
        };
        let mut command = std::process::Command::new(&executable);
        command
            .arg0(step_process_label(&record.job_id, &spawn_nonce))
            .args([
                "step",
                "--repo",
                path_text(&pinned_repo),
                "--json",
                "--expected-revision",
                &record.expected_revision,
            ])
            .current_dir(&pinned_repo)
            .env(MCP_JOB_ID_ENV, &record.job_id)
            .env(MCP_STEP_PROVENANCE_ENV, &spawn_nonce)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = command.spawn()?;
        let reaper = TestProcessReaper::new(child)?;
        let process_group = reaper.process_group();
        record.outer_process_group = Some(process_group);
        record.process_provenance = Some(
            capture_step_process_provenance(
                process_group,
                &executable,
                &pinned_repo,
                &record,
                &spawn_nonce,
            )
            .await?,
        );
        server.jobs.save_atomic(&record)?;

        server.recover_running_jobs_on_restart().await?;
        let _ = reaper.wait()?;

        let recovered = server.jobs.load("restart-alive-job").await?;
        assert_eq!(recovered.status, JobState::Interrupted);
        assert!(recovered.repository_may_be_partial);
        assert_eq!(recovered.outer_process_group, None);
        assert!(!process_group_exists(process_group));
        server.jobs.reject_active_job()?;
        assert!(repo
            .join(crate::revision::MCP_PARTIAL_STATE_LATCH)
            .is_file());

        fs::remove_dir_all(repo)?;
        Ok(())
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn pre_provenance_parent_crash_closes_barrier_before_any_mutation() -> Result<()> {
        let repo = temp_git_repo("pre-provenance-crash")?;
        prepare_operate_repo(&repo)?;
        let server = CodexTeamMcp::new(repo.clone(), McpAccess::Operate, 60)?;
        let spawn_nonce = new_step_spawn_nonce()?;
        let now = chrono::Utc::now().to_rfc3339();
        let record = JobRecord {
            schema: JOB_SCHEMA.to_string(),
            job_id: "pre-provenance-crash-job".to_string(),
            kind: crate::mcp::jobs::JobKind::Step,
            status: JobState::Running,
            expected_revision: "a".repeat(64),
            started_at: now.clone(),
            updated_at: now,
            outer_process_group: None,
            launch_nonce: Some(spawn_nonce),
            process_provenance: None,
            active_child_process_group: None,
            progress: None,
            cycle_receipt: None,
            mechanistic_receipt: None,
            delegation_feedback: None,
            delegation_manifest_digest: None,
            error: None,
            repository_may_be_partial: false,
        };
        server.jobs.save_atomic(&record)?;

        let canary = server.repo.join("barrier-mutation-canary");
        let mut barrier = StepLaunchBarrier::new()?;
        let test_binary = std::env::current_exe()?;
        let mut command = std::process::Command::new(test_binary);
        command
            .args([
                "--exact",
                "mcp::server::tests::mcp_pre_release_barrier_helper",
                "--nocapture",
            ])
            .current_dir(&server.repo)
            .env(BARRIER_TEST_CANARY_ENV, &canary)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        barrier.configure_test_child(&mut command);
        let mut child = command.spawn()?;
        barrier.child_spawned();
        drop(barrier);

        let status = child.wait()?;
        assert!(
            !status.success(),
            "barrier EOF unexpectedly authorized work"
        );
        assert!(
            !canary.exists(),
            "child mutated before durable authorization"
        );
        server.recover_running_jobs_on_restart().await?;
        let recovered = server.jobs.load(&record.job_id).await?;
        assert_eq!(recovered.status, JobState::Interrupted);
        assert!(recovered.repository_may_be_partial);
        assert_eq!(recovered.outer_process_group, None);

        fs::remove_dir_all(repo)?;
        Ok(())
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn restart_rejects_a_forged_outsider_session_without_signaling_it() -> Result<()> {
        use std::os::unix::process::CommandExt as _;

        let repo = temp_git_repo("restart-forged-outsider")?;
        prepare_operate_repo(&repo)?;
        let executable = PathBuf::from(test_shell());
        let server = CodexTeamMcp::new(repo.clone(), McpAccess::Operate, 60)?
            .with_executable(executable.clone());
        let pinned_repo = server.repo.clone();
        let job_id = "restart-forged-outsider";
        let spawn_nonce = new_step_spawn_nonce()?;
        let mut command = std::process::Command::new(&executable);
        command
            .args(["-c", "while :; do sleep 30; done"])
            .current_dir(&pinned_repo)
            .env(MCP_JOB_ID_ENV, job_id)
            .env(MCP_STEP_PROVENANCE_ENV, &spawn_nonce)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let mut outsider = command.spawn()?;
        let outsider_group = i32::try_from(outsider.id())?;
        let snapshot = inspect_live_process(outsider_group)?;
        let now = chrono::Utc::now().to_rfc3339();
        let record = JobRecord {
            schema: JOB_SCHEMA.to_string(),
            job_id: job_id.to_string(),
            kind: crate::mcp::jobs::JobKind::Step,
            status: JobState::Running,
            expected_revision: "a".repeat(64),
            started_at: now.clone(),
            updated_at: now,
            outer_process_group: Some(outsider_group),
            launch_nonce: Some(spawn_nonce.clone()),
            process_provenance: Some(StepProcessProvenance {
                schema: STEP_PROCESS_PROVENANCE_SCHEMA.to_string(),
                process_id: u32::try_from(outsider_group)?,
                start_token: snapshot.start_token,
                spawn_nonce,
            }),
            active_child_process_group: None,
            progress: None,
            cycle_receipt: None,
            mechanistic_receipt: None,
            delegation_feedback: None,
            delegation_manifest_digest: None,
            error: None,
            repository_may_be_partial: false,
        };
        server.jobs.save_atomic(&record)?;

        let recovery = server.recover_running_jobs_on_restart().await;
        let outsider_survived = outsider.try_wait()?.is_none();
        let _ = signal_process_group(outsider_group, libc::SIGKILL);
        let _ = outsider.wait();

        let error = recovery.expect_err("forged outsider provenance must fail closed");
        assert!(
            format!("{error:#}").contains("arguments do not match"),
            "unexpected recovery error: {error:#}"
        );
        assert!(outsider_survived, "restart recovery signaled the outsider");
        let retained = server.jobs.load(job_id).await?;
        assert_eq!(retained.status, JobState::Running);
        assert!(retained.repository_may_be_partial);
        assert_eq!(retained.outer_process_group, Some(outsider_group));
        assert!(repo
            .join(crate::revision::MCP_PARTIAL_STATE_LATCH)
            .is_file());

        fs::remove_dir_all(repo)?;
        Ok(())
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn bounded_query_timeout_reaps_a_term_ignoring_descendant() -> Result<()> {
        use std::os::unix::fs::PermissionsExt as _;

        let repo = temp_git_repo("query-timeout-group")?;
        let fake = repo.join("term-ignoring-query");
        let process_group_file = repo.join("query-process-group");
        fs::write(
            &fake,
            "#!/bin/sh\n\
             printf '%s\\n' \"$$\" > \"$1\"\n\
             trap '' TERM\n\
             sh -c 'trap \"\" TERM; while :; do sleep 30; done' &\n\
             wait\n",
        )?;
        let mut permissions = fs::metadata(&fake)?.permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&fake, permissions)?;

        let group_arg = path_text(&process_group_file);
        let error = match run_bounded(
            &fake,
            &repo,
            &[group_arg],
            Duration::from_secs(1),
            1024,
            1024,
            None,
        )
        .await
        {
            Ok(_) => bail!("the TERM-ignoring query unexpectedly completed"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("query exceeded"));
        let process_group = fs::read_to_string(&process_group_file)?
            .trim()
            .parse::<i32>()?;
        assert!(process_group > 0);
        assert!(
            !process_group_exists(process_group),
            "timed-out query process group {process_group} survived cleanup"
        );

        fs::remove_dir_all(repo)?;
        Ok(())
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn normally_exiting_query_reaps_a_pipe_holding_descendant() -> Result<()> {
        use std::os::unix::fs::PermissionsExt as _;

        let repo = temp_git_repo("query-pipe-holder")?;
        let fake = repo.join("pipe-holding-query");
        let process_group_file = repo.join("query-process-group");
        fs::write(
            &fake,
            "#!/bin/sh\n\
             printf '%s\\n' \"$$\" > \"$1\"\n\
             sleep 30 &\n\
             printf '%s\\n' '{\"ready\":true}'\n\
             exit 0\n",
        )?;
        let mut permissions = fs::metadata(&fake)?.permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&fake, permissions)?;

        let group_arg = path_text(&process_group_file);
        let output = tokio::time::timeout(
            Duration::from_secs(3),
            run_bounded(
                &fake,
                &repo,
                &[group_arg],
                Duration::from_secs(2),
                1024,
                1024,
                None,
            ),
        )
        .await
        .context("query supervisor hung on descendant-held output pipes")??;
        assert!(output.success);
        assert_eq!(output.stdout, b"{\"ready\":true}\n");
        let process_group = fs::read_to_string(&process_group_file)?
            .trim()
            .parse::<i32>()?;
        assert!(process_group > 0);
        assert!(
            !process_group_exists(process_group),
            "normally completed query left process group {process_group} behind"
        );

        fs::remove_dir_all(repo)?;
        Ok(())
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn session_escaping_query_pipe_holder_fails_without_hanging() -> Result<()> {
        let repo = temp_git_repo("query-session-escape")?;
        fs::write(repo.join(".enable-query-escaped-pipe-helper"), b"enabled\n")?;
        let test_binary = std::env::current_exe()?;
        let started = Instant::now();
        let error = run_bounded(
            &test_binary,
            &repo,
            &[
                "--exact",
                "mcp::server::tests::mcp_query_escaped_pipe_helper",
                "--nocapture",
            ],
            Duration::from_secs(5),
            16 * 1024,
            16 * 1024,
            None,
        )
        .await
        .expect_err("escaped query pipe holder must fail closed");
        assert!(started.elapsed() < Duration::from_secs(8));
        assert!(error.to_string().contains("remained open"));

        let escaped = fs::read_to_string(repo.join("query-escaped-pipe-holder.pid"))?
            .trim()
            .parse::<i32>()?;
        unsafe {
            libc::kill(escaped, libc::SIGKILL);
        }
        fs::remove_dir_all(repo)?;
        Ok(())
    }

    #[tokio::test]
    async fn stale_step_start_has_no_job_side_effect() -> Result<()> {
        let repo = temp_git_repo("stale-step")?;
        prepare_operate_repo(&repo)?;
        let server = CodexTeamMcp::new(repo.clone(), McpAccess::Operate, 60)?;
        assert!(server
            .start_step(StepStartArgs {
                expected_revision: "0".repeat(64),
            })
            .await
            .is_err());
        assert!(server.jobs.records()?.is_empty());
        fs::remove_dir_all(repo)?;
        Ok(())
    }
}
