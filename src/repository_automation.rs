//! Native, contained ca worker for one repository automation occurrence.
use crate::agent::client::{ChatBackend, ChatMessage, ChatRequestOptions, ChatStreamItem, ToolDef};
use crate::agent::stream::{AssistantTurnAssembler, ProviderError};
use crate::automation::{AutomationBudgetV1, AutomationUsageCountersV1};
use crate::automation_dispatch::{AutomationDispatch, AutomationDispatchPolicy};
use crate::runtime::{ActiveProfile, RuntimePaths};
use ca_automation::repository::{
    self, RepositoryAcceptance, RepositoryAgentRoute, RepositoryCandidate, RepositoryCheckReceipt,
    RepositoryPricing, RepositoryPublicationState, RepositoryPublicationStore,
    RepositoryReviewReceipt, RepositorySnapshot, RepositoryTask,
};
use ca_automation::runtime::ExecutionContext;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepositoryAutomationOutcome {
    pub integrated: bool,
    pub needs_attention: bool,
    pub summary: String,
    pub candidate: Option<RepositoryCandidate>,
    pub usage: AutomationUsageCountersV1,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OccurrenceCheckpoint {
    task_sha256: String,
    snapshot: RepositorySnapshot,
    worktree: PathBuf,
    phase: String,
    candidate: Option<RepositoryCandidate>,
    checks: Vec<RepositoryCheckReceipt>,
    review: Option<RepositoryReviewReceipt>,
    worker_output: String,
    #[serde(default)]
    usage: Option<AutomationUsageCountersV1>,
}

struct HostCheckPreparer {
    mode: crate::sandbox::SandboxMode,
    allow_network: bool,
    temporary: tempfile::TempDir,
    denied_paths: Vec<PathBuf>,
}

impl repository::RepositoryCheckPreparer for HostCheckPreparer {
    fn prepare(
        &self,
        executable: &Path,
        arguments: &[String],
        candidate_worktree: &Path,
    ) -> anyhow::Result<repository::PreparedRepositoryCheck> {
        let prepared = crate::sandbox::SandboxBackend::new(
            crate::automation_runtime::admitted_sandbox_mode(self.mode),
        )
        .prepare_process_with_denied_paths(
            executable,
            arguments,
            &crate::sandbox::SandboxPolicy {
                roots: vec![candidate_worktree.to_path_buf()],
                read_only_roots: vec![executable.to_path_buf()],
                temporary_directory: self.temporary.path().to_path_buf(),
                network_allowed: self.allow_network,
                hide_other_processes: true,
            },
            &self.denied_paths,
        )?;
        let mut environment = Vec::new();
        for key in ["PATH", "LANG", "LC_ALL", "USER", "LOGNAME"] {
            if let Some(value) = std::env::var_os(key) {
                environment.push((key.into(), value));
            }
        }
        for key in ["HOME", "TMPDIR"] {
            environment.push((key.into(), self.temporary.path().as_os_str().to_owned()));
        }
        Ok(repository::PreparedRepositoryCheck {
            program: prepared.program,
            arguments: prepared.arguments,
            environment,
        })
    }
}

/// The caller already owns the contained process and durable occurrence lease.
#[allow(clippy::too_many_arguments)]
pub async fn run(
    task: &RepositoryTask,
    ctx: &ExecutionContext,
    paths: &RuntimePaths,
    profile: &ActiveProfile,
    budget: &AutomationBudgetV1,
    tools: &[String],
    grants: &[crate::permission::PermissionMatcher],
    allow_network: bool,
) -> Result<RepositoryAutomationOutcome, String> {
    crate::jobs::AUTOMATION_PRIVATE_PATHS
        .scope(
            crate::automation_runtime::private_command_paths(paths, ctx),
            repository::CONTAINED_COMMANDS.scope(
                true,
                Box::pin(run_inner(
                    task,
                    ctx,
                    paths,
                    profile,
                    budget,
                    tools,
                    grants,
                    allow_network,
                )),
            ),
        )
        .await
}

#[allow(clippy::too_many_arguments)]
async fn run_inner(
    task: &RepositoryTask,
    ctx: &ExecutionContext,
    paths: &RuntimePaths,
    profile: &ActiveProfile,
    budget: &AutomationBudgetV1,
    tools: &[String],
    grants: &[crate::permission::PermissionMatcher],
    allow_network: bool,
) -> Result<RepositoryAutomationOutcome, String> {
    task.validate().map_err(|error| error.to_string())?;
    ctx.validate()?;
    let started = std::time::Instant::now();
    let occurrence_root = ctx
        .state_root
        .join("runs")
        .join(&ctx.run_id)
        .join("repository");
    let journal = RepositoryPublicationStore::new(occurrence_root.join("publications"));
    let checkpoint_path = occurrence_root.join("checkpoint.json");
    if let Some(publication) = journal
        .load(&ctx.run_id)
        .map_err(|error| error.to_string())?
    {
        if publication.snapshot.task_sha256 != task.digest().map_err(|error| error.to_string())? {
            return Err("Repository run is bound to a different task".into());
        }
        let _lease = ca_automation::runtime::ResourceLease::acquire(
            &ctx.lease_root,
            &publication.snapshot.resource(),
        )
        .map_err(|error| error.to_string())?;
        let usage =
            ca_automation::runtime::read::<OccurrenceCheckpoint>(&checkpoint_path, 4 * 1024 * 1024)
                .ok()
                .and_then(|checkpoint| checkpoint.usage)
                .unwrap_or_else(|| reserved_usage(budget));
        return repository::reconcile(&publication, &journal)
            .await
            .map(|publication| outcome(publication, usage))
            .map_err(|error| error.to_string());
    }
    if checkpoint_path.exists() {
        let checkpoint: OccurrenceCheckpoint =
            ca_automation::runtime::read(&checkpoint_path, 4 * 1024 * 1024)
                .map_err(|error| error.to_string())?;
        if checkpoint.task_sha256 != task.digest().map_err(|error| error.to_string())? {
            return Err("Repository run checkpoint is bound to a different task".into());
        }
        return Ok(RepositoryAutomationOutcome {
            integrated: false, needs_attention: true,
            summary: format!("Repository execution interrupted during {}; retained worktree {} requires review before repeating effects", checkpoint.phase, checkpoint.worktree.display()),
            candidate: checkpoint.candidate,
            usage: checkpoint.usage.unwrap_or_else(|| reserved_usage(budget)),
        });
    }
    let config = crate::config::ConfigStore::for_read_only_load(
        paths.clone().for_profile(profile).config_file,
        profile.clone(),
    )
    .load()
    .map_err(|error| error.to_string())?;
    let mut split_budget = split_budget(budget)?;
    let declared_checks =
        u32::try_from(task.checks.len()).map_err(|_| "Too many repository host checks")?;
    split_budget.tool_calls = budget
        .tool_calls
        .checked_sub(declared_checks)
        .ok_or("Repository checks exceed its tool-call budget")?
        / 2;
    split_budget.output_bytes = budget.output_bytes / 3;
    let worker_policy = policy(&config, paths, profile, &task.worker_route, &split_budget)?;
    let reviewer_policy = policy(&config, paths, profile, &task.reviewer_route, &split_budget)?;
    let worker = client(&config, &task.worker_route, &worker_policy)?;
    let reviewer = client(&config, &task.reviewer_route, &reviewer_policy)?;
    let snapshot = repository::inspect(task)
        .await
        .map_err(|error| error.to_string())?;
    let worktrees = crate::worktree::WorktreeStore::new(paths.worktrees_dir(profile), true);
    let label = format!("automation-{}", &ctx.run_id[..ctx.run_id.len().min(20)]).to_lowercase();
    let registry_lease = ca_automation::runtime::ResourceLease::acquire(
        &ctx.lease_root,
        &format!("worktree-registry:{}", worktrees.root().display()),
    )
    .map_err(|error| error.to_string())?;
    let record = worktrees
        .create(&snapshot.repository, &label, Some(&snapshot.base_commit))
        .await
        .map_err(|error| error.to_string())?;
    drop(registry_lease);
    let mut checkpoint = OccurrenceCheckpoint {
        task_sha256: task.digest().map_err(|error| error.to_string())?,
        snapshot: snapshot.clone(),
        worktree: record.path.clone(),
        phase: "worker".into(),
        candidate: None,
        checks: Vec::new(),
        review: None,
        worker_output: String::new(),
        usage: None,
    };
    ca_automation::runtime::atomic_write(&checkpoint_path, &checkpoint)
        .map_err(|error| error.to_string())?;
    let workspace =
        crate::workspace::WorkspaceContext::from_primary(record.path.clone(), record.path.clone());
    let grants =
        crate::schedule::rebase_schedule_grants(grants, &snapshot.repository, &record.path)
            .map_err(|error| error.to_string())?;
    let instructions = project_instructions(&record.path).await?;
    let tool_policy = crate::tool_selection::ToolSelectionPolicy::from_cli(
        Some(tools.to_vec()),
        Vec::new(),
        Vec::new(),
    )
    .map_err(|error| error.to_string())?;
    let mut output = BoundedWriter::new(ctx.output_bytes.min(1024 * 1024) as usize);
    let mut errors = BoundedWriter::new(64 * 1024);
    let system = format!("{instructions}\nImplement the requested change in this isolated worktree. The host will capture your resulting tree, run the declared checks, obtain an independent fresh-context review, and integrate it. Allowed changed paths: {}. Do not integrate, push, deploy, install, or schedule work.\n", task.allowed_paths.join(", "));
    let messages = vec![
        ChatMessage {
            role: "system".into(),
            content: crate::image_content::ModelContent::text(&system),
            tool_calls: None,
            tool_call_id: None,
            response_items: Vec::new(),
        },
        ChatMessage {
            role: "user".into(),
            content: crate::image_content::ModelContent::text(&task.prompt),
            tool_calls: None,
            tool_call_id: None,
            response_items: Vec::new(),
        },
    ];
    let run_id = uuid::Uuid::parse_str(&ctx.run_id)
        .unwrap_or_else(|_| uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_OID, ctx.run_id.as_bytes()));
    let exit = Box::pin(crate::headless::run_with_backend(
        &worker,
        messages,
        crate::headless::HeadlessRunConfig {
            workspace_activity_scope: Some(paths.profile_data_dir(profile)),
            automation_policy: Some(worker_policy),
            outcome_sink: None,
            provider: task.worker_route.provider.clone(),
            model: task.worker_route.model.clone(),
            cwd: record.path.clone(),
            workspace: workspace.clone(),
            max_turns: config.agent.max_turns,
            single_turn: false,
            command_timeout_secs: config.agent.command_timeout_secs.min(ctx.runtime_seconds),
            command_output_policy: crate::config::CommandOutputPolicy::Bounded,
            output_format: crate::headless::OutputFormat::Json,
            show_thinking: false,
            printenv: false,
            approval_mode: crate::headless::HeadlessApprovalMode::RequireApproval,
            approval_rules: config.approval_rules.clone(),
            permission_rules: config.permission_rules.clone(),
            session_grants: grants,
            allow_network,
            output_last_message: None,
            check_mode: crate::headless_contract::CheckMode::Disabled,
            convergence: None,
            structured_output: None,
            original_request: task.prompt.clone(),
            verification_instructions: system,
            chat_options: ChatRequestOptions::default(),
            sandbox_mode: crate::automation_runtime::admitted_sandbox_mode(
                crate::sandbox::SandboxMode::from_saved_value(&config.agent.sandbox_mode)
                    .map_err(|error| error.to_string())?,
            ),
            run_id,
            job_owner: crate::jobs::JobOwner::new(None),
            skills: Arc::new(Mutex::new(crate::skills::SkillRegistry::empty())),
            mcp: None,
            mechanistic: crate::config::MechanisticMcpConfig::default(),
            lsp: None,
            email: None,
            tool_policy: tool_policy.clone(),
            tool_constraints: crate::tool_selection::ToolSelectionConstraints {
                skills_enabled: false,
                mcp_enabled: false,
                mechanistic_delegation_enabled: false,
                remote_mechanistic_enabled: false,
                automation_setup_enabled: false,
                automation_proposals_enabled: false,
                ..Default::default()
            },
            tool_profile: crate::agent::registry::ToolProfile::Main,
            startup_warnings: Vec::new(),
            request_kind: crate::request::RequestKind::Scheduled,
            agents: None,
            work_state: Arc::new(Mutex::new(crate::work::WorkState::default())),
            role_run_workspace_identity: None,
            output_redactions: crate::headless::HeadlessOutputRedactions::default(),
        },
        &mut output,
        &mut errors,
    ))
    .await;
    checkpoint.worker_output = String::from_utf8_lossy(&output.bytes).into_owned();
    checkpoint.phase = "capture_candidate".into();
    ca_automation::runtime::atomic_write(&checkpoint_path, &checkpoint)
        .map_err(|error| error.to_string())?;
    if exit.code != 0 {
        return Ok(RepositoryAutomationOutcome {
            integrated: false,
            needs_attention: true,
            summary: format!(
                "Repository worker exited with {}; worktree {} retained",
                exit.code,
                record.path.display()
            ),
            candidate: None,
            usage: reserved_usage(budget),
        });
    }
    let candidate = repository::capture_candidate(task, &snapshot, &record.path, &ctx.run_id)
        .await
        .map_err(|error| error.to_string())?;
    checkpoint.candidate = Some(candidate.clone());
    checkpoint.phase = "host_checks".into();
    ca_automation::runtime::atomic_write(&checkpoint_path, &checkpoint)
        .map_err(|error| error.to_string())?;
    let preparer = HostCheckPreparer {
        mode: crate::automation_runtime::admitted_sandbox_mode(crate::sandbox::SandboxMode::parse(
            &config.agent.sandbox_mode,
        )),
        allow_network,
        temporary: tempfile::tempdir().map_err(|error| error.to_string())?,
        denied_paths: vec![
            paths.config_root.clone(),
            ctx.state_root.clone(),
            ctx.lease_root.clone(),
        ],
    };
    let checks = repository::run_checks_with_preparer(
        task,
        &snapshot,
        &candidate,
        split_budget.output_bytes,
        &preparer,
    )
    .await
    .map_err(|error| error.to_string())?;
    checkpoint.checks = checks.clone();
    checkpoint.phase = "independent_review".into();
    ca_automation::runtime::atomic_write(&checkpoint_path, &checkpoint)
        .map_err(|error| error.to_string())?;
    if checks.len() != task.checks.len() || checks.iter().any(|check| !check.passed) {
        return Ok(RepositoryAutomationOutcome {
            integrated: false,
            needs_attention: true,
            summary: "Declared host checks failed; candidate retained for review".into(),
            candidate: Some(candidate),
            usage: reserved_usage(budget),
        });
    }
    let input = crate::changes::GitChangeService::new(record.path.clone())
        .review_input(crate::changes::ReviewTarget::Base(
            snapshot.base_commit.clone(),
        ))
        .await?;
    if !input.omitted_files.is_empty() || !input.untracked_files.is_empty() {
        return Ok(RepositoryAutomationOutcome {
            integrated: false,
            needs_attention: true,
            summary: "Candidate exceeds complete review scope; retained for review".into(),
            candidate: Some(candidate),
            usage: reserved_usage(budget),
        });
    }
    let review_backend = BudgetedReviewer {
        client: reviewer,
        dispatch: Arc::new(Mutex::new(reviewer_policy.start())),
    };
    let reviewer_tools = crate::tool_selection::ToolSelectionPolicy::default();
    let review = Box::pin(crate::review::run_review(
        &review_backend,
        (&task.reviewer_route.provider, &task.reviewer_route.model),
        &input,
        crate::review::ReviewRunConfig {
            project_instructions: &instructions,
            workspace: &workspace,
            timeout_secs: ctx.runtime_seconds,
            usage_recorder: crate::usage::UsageRecorder::new(
                Arc::new(Mutex::new(crate::usage::UsageLedger::default())),
                crate::usage::UsageKind::Review,
                &task.reviewer_route.provider,
                &task.reviewer_route.model,
            ),
            tool_policy: &reviewer_tools,
            images_supported: false,
            execution_budget: None,
            pricing: None,
        },
    ))
    .await?;
    let receipt = RepositoryReviewReceipt {
        route: task.reviewer_route.clone(),
        candidate_tree: candidate.tree.clone(),
        passed: review.findings.is_empty(),
        report: serde_json::to_value(&review).map_err(|error| error.to_string())?,
    };
    checkpoint.review = Some(receipt.clone());
    let worker_usage = serde_json::from_slice::<serde_json::Value>(&output.bytes)
        .ok()
        .and_then(|value| {
            serde_json::from_value::<AutomationUsageCountersV1>(
                value.get("automation_usage")?.clone(),
            )
            .ok()
        });
    let review_usage = review_backend
        .dispatch
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .snapshot();
    let mut usage = worker_usage
        .and_then(|worker| {
            aggregate_usage(
                &worker,
                &review_usage,
                started.elapsed().as_secs().saturating_add(1),
            )
        })
        .unwrap_or_else(|| reserved_usage(budget));
    usage.tool_calls = usage
        .tool_calls
        .saturating_add(declared_checks)
        .min(budget.tool_calls);
    usage.output_bytes = usage
        .output_bytes
        .saturating_add(
            checks
                .iter()
                .map(|check| check.stdout.len().saturating_add(check.stderr.len()) as u64)
                .sum::<u64>(),
        )
        .min(budget.output_bytes);
    checkpoint.usage = Some(usage.clone());
    checkpoint.phase = "publication".into();
    ca_automation::runtime::atomic_write(&checkpoint_path, &checkpoint)
        .map_err(|error| error.to_string())?;
    if !receipt.passed {
        return Ok(RepositoryAutomationOutcome {
            integrated: false,
            needs_attention: true,
            summary: "Independent review found unresolved issues; candidate retained".into(),
            candidate: Some(candidate),
            usage,
        });
    }
    let publication = repository::integrate(
        task,
        &snapshot,
        RepositoryAcceptance {
            candidate,
            checks,
            review: receipt,
        },
        &ctx.run_id,
        &journal,
        &ctx.lease_root,
    )
    .await
    .map_err(|error| error.to_string())?;
    checkpoint.phase = "completed".into();
    ca_automation::runtime::atomic_write(&checkpoint_path, &checkpoint)
        .map_err(|error| error.to_string())?;
    Ok(outcome(publication, usage))
}

fn outcome(
    publication: repository::RepositoryPublication,
    usage: AutomationUsageCountersV1,
) -> RepositoryAutomationOutcome {
    let integrated = publication.state == RepositoryPublicationState::Integrated;
    RepositoryAutomationOutcome {
        integrated,
        needs_attention: !integrated,
        summary: publication.summary,
        candidate: Some(publication.acceptance.candidate),
        usage,
    }
}

fn reserved_usage(budget: &AutomationBudgetV1) -> AutomationUsageCountersV1 {
    AutomationUsageCountersV1 {
        provider_requests: budget.provider_requests,
        input_tokens: budget.input_tokens,
        output_tokens: budget.output_tokens,
        tool_calls: budget.tool_calls,
        runtime_secs: budget.runtime_secs,
        output_bytes: budget.output_bytes,
        cost_usd: budget.max_cost_usd,
    }
}

fn aggregate_usage(
    worker: &AutomationUsageCountersV1,
    reviewer: &AutomationUsageCountersV1,
    runtime_secs: u64,
) -> Option<AutomationUsageCountersV1> {
    Some(AutomationUsageCountersV1 {
        provider_requests: worker
            .provider_requests
            .checked_add(reviewer.provider_requests)?,
        input_tokens: worker.input_tokens.checked_add(reviewer.input_tokens)?,
        output_tokens: worker.output_tokens.checked_add(reviewer.output_tokens)?,
        tool_calls: worker.tool_calls.checked_add(reviewer.tool_calls)?,
        runtime_secs,
        output_bytes: worker.output_bytes.checked_add(reviewer.output_bytes)?,
        cost_usd: worker.cost_usd.checked_add(reviewer.cost_usd)?,
    })
}

fn split_budget(budget: &AutomationBudgetV1) -> Result<AutomationBudgetV1, String> {
    let mut split = budget.clone();
    split.provider_requests /= 2;
    split.input_tokens /= 2;
    split.output_tokens /= 2;
    split.tool_calls /= 2;
    split.output_bytes /= 2;
    split.max_cost_usd = crate::usage::DecimalUsd::from_parts(
        budget
            .max_cost_usd
            .coefficient()
            .checked_mul(5)
            .ok_or("Repository cost split overflow")?,
        budget.max_cost_usd.scale().saturating_add(1),
    )
    .or_else(|| {
        crate::usage::DecimalUsd::from_parts(
            budget.max_cost_usd.coefficient() / 2,
            budget.max_cost_usd.scale(),
        )
    })
    .ok_or("Repository budget cannot be split")?;
    if split.provider_requests == 0
        || split.input_tokens == 0
        || split.output_tokens == 0
        || split.tool_calls == 0
        || split.output_bytes < 256
        || split.max_cost_usd == crate::usage::DecimalUsd::ZERO
    {
        return Err(
            "Repository task requires capacity for both its worker and independent reviewer".into(),
        );
    }
    Ok(split)
}

fn policy(
    config: &crate::config::CaConfig,
    paths: &RuntimePaths,
    profile: &ActiveProfile,
    route: &RepositoryAgentRoute,
    budget: &AutomationBudgetV1,
) -> Result<AutomationDispatchPolicy, String> {
    let binding = match &route.pricing {
        RepositoryPricing::Metered { price_card_sha256 } => {
            crate::automation::PricingBindingV1::Metered {
                price_card_sha256: price_card_sha256.clone(),
            }
        }
        RepositoryPricing::LocalZero => crate::automation::PricingBindingV1::LocalZero,
    };
    AutomationDispatchPolicy::for_run(
        config,
        paths,
        profile,
        &route.provider,
        &route.model,
        ChatRequestOptions::default(),
        budget.clone(),
        &binding,
    )
}

fn client(
    config: &crate::config::CaConfig,
    route: &RepositoryAgentRoute,
    policy: &AutomationDispatchPolicy,
) -> Result<crate::agent::AgentClient, String> {
    let provider = config
        .providers
        .get(&route.provider)
        .ok_or("Repository route provider is unavailable")?;
    if !provider.models.iter().any(|model| model == &route.model) {
        return Err("Repository route model is unavailable".into());
    }
    if config
        .effective_model_profile(&route.provider, &route.model)
        .and_then(|profile| profile.supports_tools)
        == Some(false)
    {
        return Err("Repository route must support native tools".into());
    }
    let mut call_policy = crate::agent::client::ProviderCallPolicy::one_attempt();
    call_policy.no_progress_timeout = std::time::Duration::from_secs(policy.request_seconds());
    crate::agent::AgentClient::with_call_policy(
        &route.provider,
        provider,
        &route.model,
        call_policy,
        crate::request::RequestStore::default(),
    )
    .with_output_token_cap(policy.output_token_cap())
    .and_then(|client| client.with_input_byte_cap(policy.input_byte_cap()))
    .map_err(|error| error.to_string())
}

async fn project_instructions(root: &Path) -> Result<String, String> {
    let path = root.join("AGENTS.md");
    if !path.exists() {
        return Ok(String::new());
    }
    let metadata = tokio::fs::symlink_metadata(&path)
        .await
        .map_err(|error| error.to_string())?;
    if !metadata.is_file() || metadata.len() > 64 * 1024 {
        return Err("Repository AGENTS.md must be a bounded regular file".into());
    }
    tokio::fs::read_to_string(path)
        .await
        .map_err(|error| error.to_string())
}

struct BoundedWriter {
    bytes: Vec<u8>,
    limit: usize,
}
impl BoundedWriter {
    fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            limit,
        }
    }
}
impl Write for BoundedWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.bytes.len().saturating_add(bytes.len()) > self.limit {
            return Err(std::io::Error::other(
                "Repository worker output budget exceeded",
            ));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

struct BudgetedReviewer {
    client: crate::agent::AgentClient,
    dispatch: Arc<Mutex<AutomationDispatch>>,
}
#[async_trait::async_trait]
impl ChatBackend for BudgetedReviewer {
    async fn stream_chat(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDef],
    ) -> Result<tokio::sync::mpsc::UnboundedReceiver<ChatStreamItem>, ProviderError> {
        self.stream_prepared(&crate::request::PreparedModelRequest::new(
            crate::request::RequestKind::Review,
            self.client.provider_id(),
            self.client.model_id(),
            messages.to_vec(),
            tools.to_vec(),
            ChatRequestOptions::default(),
        ))
        .await
    }
    async fn stream_prepared(
        &self,
        request: &crate::request::PreparedModelRequest,
    ) -> Result<tokio::sync::mpsc::UnboundedReceiver<ChatStreamItem>, ProviderError> {
        let reservation = self
            .dispatch
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .reserve_request(request)
            .map_err(ProviderError::Protocol)?;
        let mut stream = self.client.stream_prepared(request).await?;
        let dispatch = self.dispatch.clone();
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        tokio::spawn(async move {
            let mut assembler = AssistantTurnAssembler::new();
            let remaining = dispatch
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .remaining();
            let forward = async {
                loop {
                    let item = tokio::select! {
                        item = stream.recv() => item,
                        _ = sender.closed() => return Err(ProviderError::Protocol(
                            "Repository review receiver closed".into()
                        )),
                    };
                    let Some(item) = item else { break };
                    let event = item?;
                    dispatch
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .account_provider_event(&event)
                        .map_err(ProviderError::Protocol)?;
                    assembler.push(&event)?;
                    sender.send(Ok(event)).map_err(|_| {
                        ProviderError::Protocol("Repository review receiver closed".into())
                    })?;
                }
                let turn = assembler.finish()?;
                let tool_count = turn
                    .tool_calls
                    .iter()
                    .filter(|call| call.name != "report_review_findings")
                    .count();
                let mut dispatch = dispatch.lock().unwrap_or_else(|p| p.into_inner());
                dispatch
                    .reserve_tools(tool_count)
                    .map_err(ProviderError::Protocol)?;
                dispatch
                    .settle_request(reservation, turn.usage.as_ref())
                    .map_err(ProviderError::Protocol)
            };
            let result = tokio::time::timeout(remaining, forward)
                .await
                .unwrap_or_else(|_| {
                    Err(ProviderError::Protocol(
                        "Repository review runtime budget exhausted".into(),
                    ))
                });
            if let Err(error) = result {
                let _ = sender.send(Err(error));
            }
        });
        Ok(receiver)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn git(root: &Path, args: &[&str]) -> String {
        let output = tokio::process::Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .await
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().into()
    }

    fn streamed_response(delta: serde_json::Value, reason: &str) -> String {
        let event = serde_json::json!({"id":"fixture", "choices":[{"index":0,"delta":delta,"finish_reason":null}]});
        let finish = serde_json::json!({"id":"fixture", "choices":[{"index":0,"delta":{},"finish_reason":reason}]});
        format!("data: {event}\n\ndata: {finish}\n\ndata: [DONE]\n\n")
    }

    async fn fixture_provider(
        command: &str,
    ) -> (String, tokio::task::JoinHandle<Vec<serde_json::Value>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
        let responses = vec![
            streamed_response(
                serde_json::json!({"tool_calls":[{"index":0,"id":"write","type":"function","function":{"name":"run_command","arguments":serde_json::json!({"command":command}).to_string()}}]}),
                "tool_calls",
            ),
            streamed_response(
                serde_json::json!({"content":"Implementation complete"}),
                "stop",
            ),
            streamed_response(
                serde_json::json!({"tool_calls":[{"index":0,"id":"report","type":"function","function":{"name":"report_review_findings","arguments":"{\"findings\":[],\"summary\":\"The exact candidate is correct\"}"}}]}),
                "tool_calls",
            ),
        ];
        let handle = tokio::spawn(async move {
            let mut requests = Vec::new();
            for body in responses {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut bytes = Vec::new();
                let header_end = loop {
                    let mut chunk = [0_u8; 4096];
                    let count = stream.read(&mut chunk).await.unwrap();
                    assert!(count > 0);
                    bytes.extend_from_slice(&chunk[..count]);
                    assert!(bytes.len() < 1024 * 1024);
                    if let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
                        break end + 4;
                    }
                };
                let headers = String::from_utf8_lossy(&bytes[..header_end]);
                let length: usize = headers
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(|value| value.trim().parse().unwrap())
                    })
                    .unwrap();
                while bytes.len() < header_end + length {
                    let mut chunk = [0_u8; 4096];
                    let count = stream.read(&mut chunk).await.unwrap();
                    assert!(count > 0);
                    bytes.extend_from_slice(&chunk[..count]);
                }
                requests
                    .push(serde_json::from_slice(&bytes[header_end..header_end + length]).unwrap());
                let response = format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                stream.write_all(response.as_bytes()).await.unwrap();
                stream.shutdown().await.unwrap();
            }
            requests
        });
        (endpoint, handle)
    }

    #[tokio::test]
    async fn native_repository_worker_checks_and_reviews_without_mcp_then_integrates() {
        native_repository_fixture(crate::sandbox::SandboxMode::PolicyOnly).await;
    }

    #[tokio::test]
    #[cfg(target_os = "macos")]
    async fn required_repository_commands_and_host_checks_cannot_read_private_run_state() {
        native_repository_fixture(crate::sandbox::SandboxMode::Required).await;
    }

    async fn native_repository_fixture(mode: crate::sandbox::SandboxMode) {
        let temporary = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(temporary.path()).unwrap();
        let repo = root.join("repo");
        std::fs::create_dir(&repo).unwrap();
        git(&repo, &["init", "-b", "main"]).await;
        git(&repo, &["config", "user.name", "Fixture"]).await;
        git(&repo, &["config", "user.email", "fixture@localhost"]).await;
        std::fs::write(repo.join("allowed.txt"), "old\n").unwrap();
        git(&repo, &["add", "."]).await;
        git(&repo, &["commit", "-m", "base"]).await;
        let paths = RuntimePaths::from_home(root.join("home"));
        let private_journal = root.join("state/runs/repository-fixture/repository/checkpoint.json");
        let command = if mode == crate::sandbox::SandboxMode::Required {
            format!(
                "test ! -r '{}' && test ! -r '{}' && printf 'new\\n' > allowed.txt",
                paths.config_file.display(),
                private_journal.display()
            )
        } else {
            "printf 'new\\n' > allowed.txt".into()
        };
        let (endpoint, provider) = fixture_provider(&command).await;
        std::fs::create_dir_all(&paths.config_root).unwrap();
        let mut config = crate::config::CaConfig::default_config();
        config.providers.clear();
        config.providers.insert(
            "fixture".into(),
            crate::config::ProviderConfig {
                base_url: endpoint,
                api_key: "fixture-test-key".into(),
                protocol: Default::default(),
                prompt_cache_mode: Default::default(),
                models: vec!["fixture-model".into()],
                timeout: Default::default(),
            },
        );
        config.defaults.provider = "fixture".into();
        config.defaults.model = "fixture-model".into();
        config.agent.sandbox_mode = mode.as_str().into();
        config.mcp.mechanistic.enabled = false;
        std::fs::write(&paths.config_file, toml::to_string(&config).unwrap()).unwrap();
        let route = RepositoryAgentRoute {
            provider: "fixture".into(),
            model: "fixture-model".into(),
            pricing: RepositoryPricing::LocalZero,
        };
        let executable = std::fs::canonicalize("/bin/sh").unwrap();
        let check_arguments = if mode == crate::sandbox::SandboxMode::Required {
            vec![
                "-c".into(),
                "test \"$(cat allowed.txt)\" = new && test ! -r \"$1\" && test ! -r \"$2\"".into(),
                "host-check".into(),
                paths.config_file.display().to_string(),
                private_journal.display().to_string(),
            ]
        } else {
            vec!["-c".into(), "test \"$(cat allowed.txt)\" = new".into()]
        };
        let task = RepositoryTask {
            repository: repo.clone(),
            target_branch: "main".into(),
            allowed_paths: vec!["allowed.txt".into()],
            prompt: "Update allowed.txt".into(),
            worker_route: route.clone(),
            reviewer_route: route,
            checks: vec![repository::RepositoryCheck {
                executable_sha256: format!(
                    "{:x}",
                    Sha256::digest(std::fs::read(&executable).unwrap())
                ),
                executable,
                args: check_arguments,
                timeout_secs: 5,
            }],
            integrate: true,
        };
        let ctx = ExecutionContext {
            schema: ca_automation_protocol::execution::EXECUTION_SCHEMA.into(),
            run_id: "repository-fixture".into(),
            profile: "default".into(),
            state_root: root.join("state"),
            lease_root: root.join("leases"),
            definition_digest: "a".repeat(64),
            resource_binding_digest: "b".repeat(64),
            policy_digest: "c".repeat(64),
            resources: vec![format!("repository:{}", repo.display())],
            runtime_seconds: 60,
            output_bytes: 1024 * 1024,
        };
        let grants = vec![crate::permission::PermissionMatcher::Invocation {
            tool: "run_command".into(),
            arguments: serde_json::json!({"command":command,"cwd":repo,"background":false,"network":false}),
        }];
        let store = ca_automation::runtime::RunStore::open(&ctx).unwrap();
        store.start().unwrap();
        let _resources = store.acquire_resources().unwrap();
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            ca_automation::runtime::EXECUTION_STORE.scope(
                store.clone(),
                run(
                    &task,
                    &ctx,
                    &paths,
                    &ActiveProfile::Default,
                    &AutomationBudgetV1::per_run_default(),
                    &["run_command".into()],
                    &grants,
                    false,
                ),
            ),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(result.integrated, "{}", result.summary);
        assert_eq!(
            std::fs::read_to_string(repo.join("allowed.txt")).unwrap(),
            "new\n"
        );
        let requests = provider.await.unwrap();
        assert_eq!(requests.len(), 3);
        let review_messages = requests[2]["messages"].as_array().unwrap();
        assert!(!review_messages.iter().any(|message| message["content"]
            .as_str()
            .is_some_and(|content| content.contains("Implementation complete"))));
        assert!(requests[2]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool["function"]["name"] == "report_review_findings"));
        assert!(!requests[2]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool["function"]["name"] == "write_file"));
        let repeated = ca_automation::runtime::EXECUTION_STORE
            .scope(
                store,
                run(
                    &task,
                    &ctx,
                    &paths,
                    &ActiveProfile::Default,
                    &AutomationBudgetV1::per_run_default(),
                    &["run_command".into()],
                    &grants,
                    false,
                ),
            )
            .await
            .unwrap();
        assert!(repeated.integrated);
        assert_eq!(git(&repo, &["rev-list", "--count", "main"]).await, "2");
    }

    #[test]
    fn worker_and_reviewer_split_cannot_widen_whole_run_budget() {
        let full = AutomationBudgetV1::per_run_default();
        let split = split_budget(&full).unwrap();
        assert!(split.provider_requests * 2 <= full.provider_requests);
        assert!(split.input_tokens * 2 <= full.input_tokens);
        assert!(split.output_tokens * 2 <= full.output_tokens);
        assert!(split.tool_calls * 2 <= full.tool_calls);
        assert!(split.output_bytes * 2 <= full.output_bytes);
        assert!(split.max_cost_usd.checked_add(split.max_cost_usd).unwrap() <= full.max_cost_usd);
    }
}
