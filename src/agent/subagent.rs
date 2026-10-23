use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, RwLock};
use tokio::sync::{mpsc, watch, Mutex, Notify, Semaphore};
use uuid::Uuid;

const DEFAULT_MAX_CONCURRENT: usize = 3;
const DEFAULT_MAX_DEPTH: u8 = 2;
const DEFAULT_MAX_CHILDREN_PER_TURN: usize = 8;
pub const MAX_AUTOMATIC_REVIEWS_PER_TURN: usize = 3;
const DEFAULT_MAX_TURNS: usize = 30;
const DEFAULT_RUNTIME_SECS: u64 = 30 * 60;
const DEFAULT_OUTPUT_BYTES: usize = 256 * 1024;
const MAX_STEER_CHARS: usize = 16_000;
const MAX_PENDING_STEERS: usize = 16;
const MAX_PENDING_STEER_BYTES: usize = 64 * 1024;
const MAX_AWAIT_SECS: u64 = 60 * 60;
const MAX_BATCH_SIZE: usize = 32;
const MAX_SPILL_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(transparent)]
pub struct AgentId(pub Uuid);

impl AgentId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl std::fmt::Display for AgentId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

impl std::str::FromStr for AgentId {
    type Err = uuid::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Uuid::parse_str(value).map(Self)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatus {
    Queued,
    Running,
    WaitingApproval,
    Completed,
    Failed,
    Cancelled,
    TimedOut,
    Interrupted,
}

impl AgentStatus {
    pub fn terminal(&self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Cancelled | Self::TimedOut | Self::Interrupted
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentContextMode {
    Summary,
    FullTranscript,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentGrant {
    pub tools: Vec<String>,
    pub workspace_root_ids: Vec<String>,
    pub path_scopes: Vec<String>,
    pub command_scopes: Vec<String>,
    pub network: bool,
}

#[derive(Debug, Clone)]
pub struct AgentLimits {
    pub max_concurrent: usize,
    pub max_depth: u8,
    pub max_children_per_turn: usize,
    pub max_turns: usize,
    pub runtime: std::time::Duration,
    pub retained_output_bytes: usize,
}

impl Default for AgentLimits {
    fn default() -> Self {
        Self {
            max_concurrent: DEFAULT_MAX_CONCURRENT,
            max_depth: DEFAULT_MAX_DEPTH,
            max_children_per_turn: DEFAULT_MAX_CHILDREN_PER_TURN,
            max_turns: DEFAULT_MAX_TURNS,
            runtime: std::time::Duration::from_secs(DEFAULT_RUNTIME_SECS),
            retained_output_bytes: DEFAULT_OUTPUT_BYTES,
        }
    }
}

impl AgentLimits {
    pub fn from_config(config: &crate::config::AgentConfig) -> Self {
        Self {
            max_concurrent: config.subagent_concurrency.max(1),
            max_depth: config.subagent_max_depth,
            max_children_per_turn: config.subagent_children_per_turn.max(1),
            max_turns: config.subagent_max_turns.max(1),
            runtime: std::time::Duration::from_secs(config.subagent_runtime_secs.max(1)),
            retained_output_bytes: config.subagent_output_bytes.clamp(1024, 4 * 1024 * 1024),
        }
    }
}

#[derive(Debug, Clone)]
pub struct AgentSpawnRequest {
    pub parent: Option<AgentId>,
    pub parent_turn: Uuid,
    pub task_id: Option<crate::work::TaskId>,
    pub task: String,
    pub context_mode: AgentContextMode,
    pub context_summary: String,
    pub grants: AgentGrant,
    /// Immutable workspace snapshot captured when this agent is spawned.
    pub workspace: crate::workspace::WorkspaceContext,
    pub depth: u8,
    /// Optional safe provider/model route. Resolved credentials remain runner-only.
    pub route: Option<AgentRoute>,
    pub purpose: AgentRunPurpose,
    /// Optional bounded JSON Schema for the child's final value.
    pub output_contract: Option<crate::headless_contract::StructuredOutputContract>,
    /// Shared workspace is the compatibility default; worktree isolation is
    /// created by the manager before child admission.
    pub isolation: AgentIsolation,
}

#[derive(
    Debug, Clone, Copy, Default, Serialize, Deserialize, schemars::JsonSchema, PartialEq, Eq,
)]
#[serde(rename_all = "snake_case")]
pub enum AgentIsolation {
    #[default]
    Shared,
    Worktree,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(transparent)]
pub struct AgentBatchId(pub Uuid);

impl AgentBatchId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl std::fmt::Display for AgentBatchId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

impl std::str::FromStr for AgentBatchId {
    type Err = uuid::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Uuid::parse_str(value).map(Self)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentRunPurpose {
    Subagent,
    MultiRunCandidate,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentRunOrigin {
    Conversation,
    AutomaticReview,
    TaskGraph,
    MultiRun,
    PlanWorkflow,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(deny_unknown_fields)]
pub struct AgentRoute {
    pub provider: String,
    pub model: String,
}

#[derive(Clone)]
struct AgentRouteSpec {
    provider: crate::config::ProviderConfig,
    request_store: crate::request::RequestStore,
    images_supported: bool,
    prompt_cache_contract: Option<crate::provider_contract::ResolvedPromptCacheContract>,
}

#[async_trait]
trait AgentRuntimeClientResolver: Send + Sync {
    async fn resolve(
        &self,
        provider_id: &str,
        provider: crate::config::ProviderConfig,
    ) -> Result<crate::config::ProviderConfig, String>;
}

#[derive(Debug, Default)]
struct DefaultAgentRuntimeClientResolver;

#[async_trait]
impl AgentRuntimeClientResolver for DefaultAgentRuntimeClientResolver {
    async fn resolve(
        &self,
        provider_id: &str,
        provider: crate::config::ProviderConfig,
    ) -> Result<crate::config::ProviderConfig, String> {
        crate::provider_connect::resolve_runtime_provider(provider_id, provider).await
    }
}

#[derive(Clone)]
pub struct AgentRouteClients {
    clients: Arc<std::sync::RwLock<HashMap<AgentRoute, AgentRouteSpec>>>,
    runtime_resolver: Arc<dyn AgentRuntimeClientResolver>,
    resolution_gate: Arc<Mutex<()>>,
}

impl std::fmt::Debug for AgentRouteClients {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let clients = self
            .clients
            .read()
            .unwrap_or_else(|poison| poison.into_inner());
        let mut routes = clients.keys().collect::<Vec<_>>();
        routes.sort_by(|left, right| {
            left.provider
                .cmp(&right.provider)
                .then_with(|| left.model.cmp(&right.model))
        });
        formatter
            .debug_struct("AgentRouteClients")
            .field("routes", &routes)
            .finish()
    }
}

impl AgentRouteClients {
    pub fn new(
        clients: impl IntoIterator<
            Item = (
                AgentRoute,
                crate::config::ProviderConfig,
                crate::request::RequestStore,
                bool,
            ),
        >,
    ) -> Self {
        Self {
            clients: Arc::new(std::sync::RwLock::new(
                clients
                    .into_iter()
                    .map(|(route, provider, request_store, images_supported)| {
                        (
                            route,
                            AgentRouteSpec {
                                provider,
                                request_store,
                                images_supported,
                                prompt_cache_contract: None,
                            },
                        )
                    })
                    .collect(),
            )),
            runtime_resolver: Arc::new(DefaultAgentRuntimeClientResolver),
            resolution_gate: Arc::new(Mutex::new(())),
        }
    }

    pub fn new_with_prompt_cache_contracts(
        clients: impl IntoIterator<
            Item = (
                AgentRoute,
                crate::config::ProviderConfig,
                crate::request::RequestStore,
                bool,
                crate::provider_contract::ResolvedPromptCacheContract,
            ),
        >,
    ) -> Self {
        Self {
            clients: Arc::new(std::sync::RwLock::new(
                clients
                    .into_iter()
                    .map(
                        |(route, provider, request_store, images_supported, contract)| {
                            (
                                route,
                                AgentRouteSpec {
                                    provider,
                                    request_store,
                                    images_supported,
                                    prompt_cache_contract: Some(contract),
                                },
                            )
                        },
                    )
                    .collect(),
            )),
            runtime_resolver: Arc::new(DefaultAgentRuntimeClientResolver),
            resolution_gate: Arc::new(Mutex::new(())),
        }
    }

    #[cfg(test)]
    fn with_runtime_resolver(
        clients: impl IntoIterator<
            Item = (
                AgentRoute,
                crate::config::ProviderConfig,
                crate::request::RequestStore,
                bool,
            ),
        >,
        runtime_resolver: Arc<dyn AgentRuntimeClientResolver>,
    ) -> Self {
        let mut route_clients = Self::new(clients);
        route_clients.runtime_resolver = runtime_resolver;
        route_clients
    }

    async fn resolve(
        &self,
        route: &AgentRoute,
    ) -> Result<Option<(crate::agent::AgentClient, bool)>, String> {
        let entry = self
            .clients
            .read()
            .unwrap_or_else(|poison| poison.into_inner())
            .get(route)
            .cloned();
        let Some(entry) = entry else {
            return Ok(None);
        };
        let provider = if route.provider == "openai-codex" {
            let _guard = self.resolution_gate.lock().await;
            self.runtime_resolver
                .resolve(&route.provider, entry.provider)
                .await?
        } else {
            entry.provider
        };
        let mut client = crate::agent::AgentClient::with_request_store(
            &route.provider,
            &provider,
            &route.model,
            entry.request_store,
        );
        if let Some(contract) = entry.prompt_cache_contract {
            client = client.with_prompt_cache_contract(contract);
        }
        Ok(Some((client, entry.images_supported)))
    }

    fn routes(&self) -> Vec<AgentRoute> {
        self.clients
            .read()
            .unwrap_or_else(|poison| poison.into_inner())
            .keys()
            .cloned()
            .collect()
    }

    fn replace(&self, replacement: AgentRouteClients) {
        let replacement = replacement
            .clients
            .read()
            .unwrap_or_else(|poison| poison.into_inner())
            .clone();
        *self
            .clients
            .write()
            .unwrap_or_else(|poison| poison.into_inner()) = replacement;
    }
}

impl Default for AgentRouteClients {
    fn default() -> Self {
        Self::new(std::iter::empty())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentEvent {
    pub sequence: u64,
    pub at: DateTime<Utc>,
    pub message: String,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentExecutionPhase {
    #[default]
    Queued,
    WaitingForResponse,
    Thinking,
    Tool,
    WaitingApproval,
    Finished,
}

impl AgentExecutionPhase {
    pub fn label(self) -> &'static str {
        match self {
            Self::Queued => "Queued",
            Self::WaitingForResponse => "Waiting for response…",
            Self::Thinking => "Thinking",
            Self::Tool => "Running tool",
            Self::WaitingApproval => "Waiting for approval",
            Self::Finished => "Finished",
        }
    }
}

#[derive(Debug, Clone)]
pub struct AgentMutationReceipt {
    pub agent_id: AgentId,
    pub parent_turn: Uuid,
    pub mutation: crate::undo::FileMutationBatchReceipt,
    pub aggregate: bool,
    pub aggregate_started_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct AgentSnapshot {
    pub id: AgentId,
    pub status: AgentStatus,
    pub origin: AgentRunOrigin,
    pub purpose: AgentRunPurpose,
    pub task_id: Option<crate::work::TaskId>,
    pub task: String,
    pub role: Option<String>,
    pub model: Option<String>,
    pub provider: Option<String>,
    pub phase: AgentExecutionPhase,
    pub parent: Option<AgentId>,
    pub depth: u8,
    pub context_mode: AgentContextMode,
    pub grants: AgentGrant,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub result_summary: Option<String>,
    pub result_sha256: Option<String>,
    pub structured_result: Option<serde_json::Value>,
    pub structured_repair_attempted: bool,
    pub spill: Option<AgentSpillReceipt>,
    pub worktree: Option<AgentWorktreeReceipt>,
    pub attributed_jobs: Vec<AgentJobAttribution>,
    pub missed_steer: Vec<String>,
    pub recent_output: String,
    pub successful_tools: Vec<String>,
    pub current_action: Option<String>,
    pub events: Vec<AgentEvent>,
    pub graph_budget_usage: Option<crate::agent_graph::AgentGraphBudgetUsageV1>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentSpillReceipt {
    pub path: PathBuf,
    pub sha256: String,
    pub bytes: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentWorktreeReceipt {
    pub id: Uuid,
    pub path: PathBuf,
    pub branch: String,
    pub base_commit: String,
    pub dirty: Option<bool>,
    pub changed_entries: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentJobAttribution {
    pub job_id: String,
    pub parent_turn: Uuid,
    pub background: bool,
    pub status: String,
}

fn merge_attributed_jobs(
    retained: &mut Vec<AgentJobAttribution>,
    current: Vec<AgentJobAttribution>,
) {
    for job in current {
        if let Some(existing) = retained.iter_mut().find(|item| item.job_id == job.job_id) {
            *existing = job;
        } else {
            retained.push(job);
        }
    }
    retained.sort_by(|left, right| left.job_id.cmp(&right.job_id));
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentActivitySnapshot {
    pub id: AgentId,
    pub parent_turn: Uuid,
    pub parent: Option<AgentId>,
    pub origin: AgentRunOrigin,
    pub purpose: AgentRunPurpose,
    pub status: AgentStatus,
    pub task: String,
    pub role: Option<String>,
    pub model: Option<String>,
    pub provider: Option<String>,
    pub phase: AgentExecutionPhase,
    pub current_action: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub result_summary: Option<String>,
    pub recent_output: String,
    pub events: Vec<AgentEvent>,
}

#[derive(Debug)]
pub enum AgentCommand {
    Message { id: Uuid, text: String },
    Approval(bool),
}

#[derive(Debug)]
pub enum RunnerEvent {
    Phase(AgentExecutionPhase),
    Mutation(crate::undo::FileMutationBatchReceipt),
    AggregateMutation {
        batch: crate::undo::FileMutationBatchReceipt,
        started_at: Option<DateTime<Utc>>,
    },
    HarnessPrepared {
        snapshot: crate::harness_registry::ComponentSnapshot,
        model: String,
        provider: String,
    },
    ToolEvidence {
        turn: u64,
        call_id: String,
        name: String,
        succeeded: bool,
        denied: bool,
        output: String,
    },
    Activity(String),
    Output(String),
    ToolStarted {
        name: String,
    },
    ToolCompleted {
        name: String,
        succeeded: bool,
    },
    ApprovalRequired(String),
    SteerDelivered {
        id: Uuid,
    },
}

pub struct AgentRunContext {
    pub id: AgentId,
    pub request: AgentSpawnRequest,
    pub limits: AgentLimits,
    pub commands: mpsc::Receiver<AgentCommand>,
    pending_steers: VecDeque<(Uuid, String)>,
    pub cancelled: watch::Receiver<bool>,
    pub events: mpsc::UnboundedSender<RunnerEvent>,
    pub job_owner: crate::jobs::JobOwner,
    pub graph_budget: Option<crate::agent_graph::AgentAutonomyBudget>,
    pub(crate) frozen_policy: Option<Arc<FrozenChildPolicy>>,
}

/// A human permission ceiling captured at admission, never replaced by a retry.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FrozenChildPolicy {
    pub approval: crate::app::ApprovalMode,
    pub approval_rules: Vec<crate::config::ApprovalRule>,
    pub permission_rules: Vec<crate::permission::PermissionRule>,
    pub session_grants: Vec<crate::permission::PermissionMatcher>,
}

impl FrozenChildPolicy {
    pub(crate) fn from_authority(authority: &crate::automatic_review::ReviewAuthority) -> Self {
        Self {
            approval: authority.approval.clone(),
            approval_rules: authority.approval_rules.clone(),
            permission_rules: authority.permission_rules.clone(),
            session_grants: authority.session_grants.clone(),
        }
    }
}

#[async_trait]
pub trait AgentRunner: Send + Sync {
    async fn run(&self, context: AgentRunContext) -> Result<String, String>;

    fn replace_route_clients(&self, _clients: AgentRouteClients) {}

    /// Configured route identities only; listing never resolves credentials.
    fn available_routes(&self) -> Vec<AgentRoute> {
        Vec::new()
    }

    fn skill_constraint_ids(&self) -> (Option<Vec<String>>, Option<Vec<String>>) {
        (None, None)
    }

    fn verification_sandbox_mode(&self) -> Option<crate::sandbox::SandboxMode> {
        None
    }
}

#[derive(Clone)]
pub struct ProviderAgentRunner {
    pub client: crate::agent::AgentClient,
    pub route_clients: AgentRouteClients,
    pub sandbox_mode: crate::sandbox::SandboxMode,
    pub command_timeout_secs: u64,
    pub approval_rules: Vec<crate::config::ApprovalRule>,
    pub permission_rules: Vec<crate::permission::PermissionRule>,
    pub session_grants: Arc<std::sync::Mutex<crate::permission::SessionGrantStore>>,
    pub skills: Arc<std::sync::Mutex<crate::skills::SkillRegistry>>,
    pub task_skill_snapshot: Arc<std::sync::Mutex<String>>,
    pub manager_link: Arc<std::sync::OnceLock<std::sync::Weak<AgentManager>>>,
    pub usage_recorder: crate::usage::UsageRecorder,
    pub tool_policy: crate::tool_selection::ToolSelectionPolicy,
    pub skills_enabled: bool,
    pub images_supported: bool,
    pub pricing: Arc<std::collections::HashMap<String, crate::usage::UsagePricing>>,
}

#[async_trait]
impl AgentRunner for ProviderAgentRunner {
    fn available_routes(&self) -> Vec<AgentRoute> {
        self.route_clients.routes()
    }

    fn verification_sandbox_mode(&self) -> Option<crate::sandbox::SandboxMode> {
        Some(self.sandbox_mode)
    }

    async fn run(&self, mut context: AgentRunContext) -> Result<String, String> {
        use crate::agent::client::{ChatMessage, FunctionCall, ToolCallMsg};
        let (client, images_supported) = if let Some(route) = context.request.route.as_ref() {
            let (client, images_supported) =
                self.route_clients.resolve(route).await?.ok_or_else(|| {
                    format!(
                        "subagent route is unavailable: {}/{}",
                        route.provider, route.model
                    )
                })?;
            (client, images_supported)
        } else {
            (self.client.clone(), self.images_supported)
        };
        let client = if context.graph_budget.is_some() {
            client.with_one_attempt_policy()
        } else {
            client
        };
        let (usage_kind, request_kind) = match context.request.purpose {
            AgentRunPurpose::Subagent => (
                crate::usage::UsageKind::Subagent,
                crate::request::RequestKind::Subagent,
            ),
            AgentRunPurpose::MultiRunCandidate => (
                crate::usage::UsageKind::MultiRunCandidate,
                crate::request::RequestKind::MultiRunCandidate,
            ),
        };
        let usage_recorder = context.request.route.as_ref().map_or_else(
            || {
                self.usage_recorder
                    .reroute(usage_kind, client.provider_id(), client.model_id())
            },
            |route| {
                self.usage_recorder
                    .reroute(usage_kind, &route.provider, &route.model)
            },
        );
        let registry = crate::agent::registry::ToolRegistry::builtins();
        let workspace = context.request.workspace.clone();
        let granted: std::collections::HashSet<_> = context
            .request
            .grants
            .tools
            .iter()
            .map(|name| name.to_ascii_lowercase())
            .collect();
        let (skill_activation_ids, skill_view_ids) = self.skill_constraint_ids();
        let authorized_tools = crate::tool_selection::resolve_internal_tools(
            registry,
            crate::agent::registry::ToolProfile::Subagent,
            &self.tool_policy,
            &crate::tool_selection::ToolSelectionConstraints {
                tools_supported: true,
                images_supported,
                skills_enabled: self.skills_enabled,
                skill_activation_ids,
                skill_view_ids,
                mcp_enabled: false,
                mechanistic_delegation_enabled: false,
                native_mechanistic_enabled: false,
                transparent_mechanistic_enabled: false,
                remote_mechanistic_enabled: false,
                automation_setup_enabled: false,
                lsp_enabled: false,
                memory_generation_enabled: false,
                automation_proposals_enabled: false,
                email_enabled: false,
                allowed_names: Some(granted.clone()),
            },
        )
        .map_err(|error| format!("tool selection failed: {error}"))?
        .into_definitions();
        let discovery = crate::tool_discovery::ToolDiscoverySession::default();
        let active_skills = self
            .skills
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .active_prompt();
        let task_skills = self
            .task_skill_snapshot
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .clone();
        let structured_instruction = context.request.output_contract.as_ref().map_or(
            String::new(),
            |_| {
                "\nA structured result is required. Finish by calling report_structured_output exactly once as the sole final tool call.".into()
            },
        );
        let mut messages = vec![
            ChatMessage {
                role: "system".into(),
                content: crate::image_content::ModelContent::text(format!(
                    "You are a bounded child coding agent. Use only granted tools. Never widen workspace roots or network authority.{}\n\nState summary:\n{}{}{}",
                    structured_instruction,
                    context.request.context_summary,
                    active_skills,
                    task_skills
                )),
                tool_calls: None,
                tool_call_id: None,
                response_items: Vec::new(),
            },
            ChatMessage {
                role: "user".into(),
                content: crate::image_content::ModelContent::text(context.request.task.clone()),
                tool_calls: None,
                tool_call_id: None,
                response_items: Vec::new(),
            },
        ];
        let owner = context.job_owner;
        let mut structured_repair_attempted = false;
        for turn in 0..context.limits.max_turns {
            let mut tools = discovery.advertise(authorized_tools.clone(), &self.tool_policy);
            if let Some(contract) = &context.request.output_contract {
                tools.push(contract.tool_definition());
            }
            let advertised_tool_names = tools
                .iter()
                .map(|tool| tool.function.name.clone())
                .collect::<std::collections::HashSet<_>>();
            drain_child_steering(&mut context, &mut messages);
            let _ = context
                .events
                .send(RunnerEvent::Activity(format!("provider turn {}", turn + 1)));
            let mut usage_span = usage_recorder.begin();
            if let Some(budget) = &context.graph_budget {
                budget
                    .reserve_request()
                    .map_err(|limit| format!("agent graph autonomy budget exhausted: {limit:?}"))?;
            }
            let prepared = crate::request::PreparedModelRequest::new(
                request_kind,
                client.provider_id(),
                client.model_id(),
                messages.clone(),
                tools.clone(),
                crate::agent::client::ChatRequestOptions::default(),
            )
            .with_prompt_cache(
                crate::request::PromptCacheScope::agent(context.id.0),
                messages
                    .iter()
                    .take_while(|message| matches!(message.role.as_str(), "system" | "developer"))
                    .count(),
            );
            let prepared = client.prepare_harness_request(prepared).await;
            if let Ok(source) = client.route_identity() {
                let _ = context.events.send(RunnerEvent::HarnessPrepared {
                    snapshot: crate::harness_registry::capture_prepared_on_route(
                        &prepared, &source,
                    ),
                    model: client.model_id().into(),
                    provider: client.provider_id().into(),
                });
            }
            let _ = context
                .events
                .send(RunnerEvent::Phase(AgentExecutionPhase::WaitingForResponse));
            let stream_result = tokio::select! {
                _ = wait_cancelled(context.cancelled.clone()) => return Err("cancelled".into()),
                result = client.stream_prepared(&prepared) => result.map_err(|error| error.to_string()),
            };
            let mut stream = match stream_result {
                Ok(stream) => stream,
                Err(error) => {
                    usage_span.fail(None);
                    return Err(error);
                }
            };
            let assembled_result = tokio::select! {
                _ = wait_cancelled(context.cancelled.clone()) => return Err("cancelled".into()),
                result = crate::agent::stream::consume_stream(&mut stream, |event| {
                    if matches!(event, crate::agent::stream::ProviderEvent::Reasoning(_)) {
                        let _ = context.events.send(RunnerEvent::Phase(AgentExecutionPhase::Thinking));
                    }
                    if let crate::agent::stream::ProviderEvent::Usage(usage) = event {
                        usage_span.observe(usage);
                    }
                }) => result.map_err(|error| error.to_string()),
            };
            let assembled = match assembled_result {
                Ok(assembled) => {
                    usage_span.complete(assembled.usage.clone());
                    assembled
                }
                Err(error) => {
                    usage_span.fail(None);
                    return Err(error);
                }
            };
            if let Some(budget) = &context.graph_budget {
                let pricing =
                    self.pricing
                        .get(&format!("{}/{}", client.provider_id(), client.model_id()));
                budget
                    .observe_request(assembled.usage.as_ref(), pricing)
                    .map_err(|limit| format!("agent graph autonomy budget exhausted: {limit:?}"))?;
                let retained_bytes = assembled
                    .content
                    .len()
                    .saturating_add(assembled.reasoning.as_ref().map_or(0, String::len))
                    .saturating_add(
                        assembled
                            .tool_calls
                            .iter()
                            .map(|call| call.arguments.len())
                            .sum::<usize>(),
                    );
                budget
                    .retain_output(retained_bytes)
                    .map_err(|limit| format!("agent graph autonomy budget exhausted: {limit:?}"))?;
            }
            let calls: Vec<_> = assembled
                .tool_calls
                .into_iter()
                .map(|call| (call.id, call.name, call.arguments))
                .collect();
            messages.push(ChatMessage {
                role: "assistant".into(),
                content: crate::image_content::ModelContent::text(assembled.content.clone()),
                tool_calls: (!calls.is_empty()).then(|| {
                    calls
                        .iter()
                        .map(|(id, name, arguments)| ToolCallMsg {
                            id: id.clone(),
                            call_type: "function".into(),
                            function: FunctionCall {
                                name: name.clone(),
                                arguments: arguments.clone(),
                            },
                        })
                        .collect()
                }),
                tool_call_id: None,
                response_items: assembled.response_items,
            });
            if calls.is_empty() {
                if context.request.output_contract.is_none() {
                    cleanup_owned_jobs(owner).await;
                    return Ok(assembled.content);
                }
                let error = "structured output response did not call report_structured_output";
                if structured_repair_attempted {
                    cleanup_owned_jobs(owner).await;
                    return Err(format!(
                        "structured output remained invalid after one repair turn: {error}"
                    ));
                }
                structured_repair_attempted = true;
                let _ = context.events.send(RunnerEvent::Activity(
                    "structured output repair turn 1 of 1".into(),
                ));
                messages.push(ChatMessage {
                    role: "system".into(),
                    content: crate::image_content::ModelContent::text(format!(
                        "The final structured result was rejected: {}. Correct it now by calling report_structured_output exactly once as the sole final tool call. This is the only repair turn.",
                        crate::terminal_text::sanitize_terminal_text(error)
                    )),
                    tool_calls: None,
                    tool_call_id: None,
                    response_items: Vec::new(),
                });
                continue;
            }
            if let Some(contract) = &context.request.output_contract {
                if !calls
                    .iter()
                    .any(|(_, name, _)| name == crate::headless_contract::STRUCTURED_OUTPUT_TOOL)
                {
                    // Ordinary tool work may continue before the final structured call.
                } else {
                    let validation = if calls.len() != 1
                        || calls[0].1 != crate::headless_contract::STRUCTURED_OUTPUT_TOOL
                    {
                        Err("report_structured_output must be the sole final tool call".into())
                    } else {
                        contract.validate_arguments(&calls[0].2)
                    };
                    match validation {
                        Ok(value) => {
                            cleanup_owned_jobs(owner).await;
                            return serde_json::to_string(&value).map_err(|error| {
                                format!("structured output encoding failed: {error}")
                            });
                        }
                        Err(error) if structured_repair_attempted => {
                            cleanup_owned_jobs(owner).await;
                            return Err(format!(
                                "structured output remained invalid after one repair turn: {}",
                                crate::terminal_text::sanitize_terminal_text(&error)
                            ));
                        }
                        Err(error) => {
                            structured_repair_attempted = true;
                            for (call_id, _, _) in &calls {
                                messages.push(ChatMessage {
                                    role: "tool".into(),
                                    content: crate::image_content::ModelContent::text(format!(
                                        "Structured output rejected: {}",
                                        crate::terminal_text::sanitize_terminal_text(&error)
                                    )),
                                    tool_calls: None,
                                    tool_call_id: Some(call_id.clone()),
                                    response_items: Vec::new(),
                                });
                            }
                            let _ = context.events.send(RunnerEvent::Activity(
                                "structured output repair turn 1 of 1".into(),
                            ));
                            messages.push(ChatMessage {
                            role: "system".into(),
                            content: crate::image_content::ModelContent::text(format!(
                                "The final structured result was rejected: {}. Correct it now by calling report_structured_output exactly once as the sole final tool call. This is the only repair turn.",
                                crate::terminal_text::sanitize_terminal_text(&error)
                            )),
                            tool_calls: None,
                            tool_call_id: None,
                            response_items: Vec::new(),
                        });
                            continue;
                        }
                    }
                }
            }
            let discovery_batch = discovery.begin_batch();
            let execution_grants = context.request.grants.clone();
            let frozen_policy = context.frozen_policy.clone();
            let mut prefetched_reads = std::collections::HashMap::new();
            for (call_index, (call_id, name, raw_arguments)) in calls.iter().cloned().enumerate() {
                if let Some(budget) = context
                    .graph_budget
                    .as_ref()
                    .filter(|_| !prefetched_reads.contains_key(&call_index))
                {
                    budget.reserve_tool_call().map_err(|limit| {
                        format!("agent graph autonomy budget exhausted: {limit:?}")
                    })?;
                }
                if !advertised_tool_names.contains(&name) {
                    cleanup_owned_jobs(owner).await;
                    return Err(format!(
                        "provider attempted unavailable subagent tool: {name}"
                    ));
                }
                let parsed: serde_json::Value = serde_json::from_str(&raw_arguments)
                    .map_err(|error| format!("invalid {name} arguments: {error}"))?;
                if !prefetched_reads.contains_key(&call_index) {
                    let _ = context
                        .events
                        .send(RunnerEvent::ToolStarted { name: name.clone() });
                }
                let policy_prompt = matches!(
                    child_permission_decision(
                        self,
                        frozen_policy.as_deref(),
                        &workspace,
                        context.id,
                        &name,
                        &parsed
                    ),
                    Ok(crate::approval::BrokerDecision::ExplicitPrompt)
                        | Ok(crate::approval::BrokerDecision::NetworkPrompt)
                );
                let native_context = ChildToolContext {
                    runner: self,
                    id: context.id,
                    owner,
                    grant: &execution_grants,
                    workspace: &workspace,
                    policy_approved: false,
                    frozen_policy: frozen_policy.as_deref(),
                };
                let result = if let Some(result) = prefetched_reads.remove(&call_index) {
                    result
                } else if granted.contains(&name)
                    && !policy_prompt
                    && registry
                        .supports_native_read(&name, crate::agent::registry::ToolProfile::Subagent)
                {
                    match prepare_child_tool(&native_context, &name, &parsed) {
                        Err(result) => *result,
                        Ok(prepared) => {
                            let mut run = vec![crate::agent::tools::PreparedNativeRead {
                                name: name.clone(),
                                args: prepared.arguments,
                            }];
                            for (_, next_name, raw) in calls
                                .iter()
                                .skip(call_index + 1)
                                .take(crate::agent::tools::MAX_NATIVE_READ_CONCURRENCY - 1)
                            {
                                if !advertised_tool_names.contains(next_name)
                                    || !granted.contains(next_name)
                                    || !registry.supports_native_read(
                                        next_name,
                                        crate::agent::registry::ToolProfile::Subagent,
                                    )
                                {
                                    break;
                                }
                                let Ok(arguments) = serde_json::from_str(raw) else {
                                    break;
                                };
                                let Ok(prepared) =
                                    prepare_child_tool(&native_context, next_name, &arguments)
                                else {
                                    break;
                                };
                                if let Some(budget) = &context.graph_budget {
                                    budget.reserve_tool_call().map_err(|limit| {
                                        format!("agent graph autonomy budget exhausted: {limit:?}")
                                    })?;
                                }
                                let _ = context.events.send(RunnerEvent::ToolStarted {
                                    name: next_name.clone(),
                                });
                                run.push(crate::agent::tools::PreparedNativeRead {
                                    name: next_name.clone(),
                                    args: prepared.arguments,
                                });
                            }
                            let tool_context = crate::agent::registry::ToolContext {
                                recovery: crate::owned_tools::current_recovery(),
                                cwd: workspace.cwd().to_path_buf(),
                                timeout_secs: self.command_timeout_secs,
                                profile: crate::agent::registry::ToolProfile::Subagent,
                                workspace_roots: workspace.execution_roots(),
                                sandbox_mode: self.sandbox_mode,
                                network_allowed: false,
                                job_owner: owner,
                                job_execution_policy: crate::jobs::JobExecutionPolicy::default(),
                            };
                            let mut results =
                                crate::agent::tools::execute_native_read_run(&run, &tool_context)
                                    .await
                                    .into_iter();
                            let first = results.next().expect("nonempty admitted native run");
                            for (offset, result) in results.enumerate() {
                                prefetched_reads.insert(call_index + offset + 1, result);
                            }
                            first
                        }
                    }
                } else if name == "spawn_agent" && granted.contains(&name) && !policy_prompt {
                    execute_child_nested_spawn(&context, &native_context, &parsed).await
                } else if name == "request_graph_clarification" {
                    if context.graph_budget.is_none() {
                        crate::agent::registry::ToolExecutionResult::Failure(
                            "request_graph_clarification is available only inside an agent graph task"
                                .into(),
                        )
                    } else {
                        match serde_json::from_value::<
                            crate::agent::registry::arguments::RequestGraphClarificationArgs,
                        >(parsed)
                        {
                            Ok(args) => await_graph_clarification(&mut context, args).await,
                            Err(error) => crate::agent::registry::ToolExecutionResult::Failure(
                                format!("invalid request_graph_clarification arguments: {error}"),
                            ),
                        }
                    }
                } else if !granted.contains(&name) || policy_prompt {
                    if self
                        .manager_link
                        .get()
                        .and_then(std::sync::Weak::upgrade)
                        .is_some_and(|manager| {
                            manager
                                .headless_noninteractive
                                .load(std::sync::atomic::Ordering::Acquire)
                        })
                    {
                        return Err(format!(
                            "headless child invocation {name} requires interactive approval"
                        ));
                    }
                    let approved = await_child_approval(
                        &mut context,
                        if !granted.contains(&name) {
                            format!("approval required for out-of-scope tool {name}")
                        } else {
                            format!("permission policy requires approval for {name}")
                        },
                    )
                    .await?;
                    if !approved {
                        crate::agent::registry::ToolExecutionResult::Denied(
                            "Denied by parent approval".into(),
                        )
                    } else if name == crate::tool_discovery::SEARCH_TOOLS_NAME {
                        execute_child_discovery(&native_context, &parsed, true, &discovery)
                    } else if name == "spawn_agent" {
                        let approved_context = ChildToolContext {
                            policy_approved: true,
                            ..native_context
                        };
                        execute_child_nested_spawn(&context, &approved_context, &parsed).await
                    } else {
                        execute_child_tool(
                            ChildToolContext {
                                runner: self,
                                id: context.id,
                                owner,
                                grant: &context.request.grants,
                                workspace: &workspace,
                                policy_approved: true,
                                frozen_policy: frozen_policy.as_deref(),
                            },
                            &name,
                            &parsed,
                        )
                        .await
                    }
                } else if name == crate::tool_discovery::SEARCH_TOOLS_NAME {
                    execute_child_discovery(&native_context, &parsed, false, &discovery)
                } else {
                    execute_child_tool(
                        ChildToolContext {
                            runner: self,
                            id: context.id,
                            owner,
                            grant: &context.request.grants,
                            workspace: &workspace,
                            policy_approved: false,
                            frozen_policy: frozen_policy.as_deref(),
                        },
                        &name,
                        &parsed,
                    )
                    .await
                };
                discovery.touch(&name);
                if let Some(mutation) = result.mutation_batch() {
                    let _ = context.events.send(RunnerEvent::Mutation(mutation.clone()));
                }
                let _ = context.events.send(RunnerEvent::ToolEvidence {
                    turn: turn as u64,
                    call_id: call_id.clone(),
                    name: name.clone(),
                    succeeded: result.is_success(),
                    denied: matches!(
                        &result,
                        crate::agent::registry::ToolExecutionResult::Denied(_)
                    ),
                    output: crate::learning::sanitized_learning_text(result.persistent_text())
                        .unwrap_or_else(|| "[private evidence omitted]".into())
                        .chars()
                        .take(1024)
                        .collect(),
                });
                let _ = context.events.send(RunnerEvent::Output(format!(
                    "{name}: {}\n",
                    result.model_text()
                )));
                if let Some(budget) = &context.graph_budget {
                    budget
                        .retain_output(result.model_text().len())
                        .map_err(|limit| {
                            format!("agent graph autonomy budget exhausted: {limit:?}")
                        })?;
                    if !result.is_success() {
                        budget.observe_tool_failure().map_err(|limit| {
                            format!("agent graph autonomy budget exhausted: {limit:?}")
                        })?;
                    }
                }
                let _ = context.events.send(RunnerEvent::ToolCompleted {
                    name: name.clone(),
                    succeeded: result.is_success(),
                });
                messages.push(ChatMessage {
                    role: "tool".into(),
                    content: crate::image_content::ModelContent::text(result.model_text())
                        .with_prunable_tool_result(crate::context_pruning::qualified_result(
                            &name,
                            &raw_arguments,
                            result.is_success() && result.image().is_none(),
                        ))
                        .with_tool_result_status(result.is_success()),
                    tool_calls: None,
                    tool_call_id: Some(call_id.clone()),
                    response_items: Vec::new(),
                });
                if let Some(image) = result.image() {
                    messages.push(ChatMessage {
                        role: "user".into(),
                        content: crate::image_content::ModelContent::image_from_tool(
                            &call_id,
                            image.clone(),
                        ),
                        tool_calls: None,
                        tool_call_id: None,
                        response_items: Vec::new(),
                    });
                }
            }
            discovery_batch.commit();
        }
        cleanup_owned_jobs(owner).await;
        Err(format!(
            "subagent exceeded maximum {} turns",
            context.limits.max_turns
        ))
    }

    fn replace_route_clients(&self, clients: AgentRouteClients) {
        self.route_clients.replace(clients);
    }

    fn skill_constraint_ids(&self) -> (Option<Vec<String>>, Option<Vec<String>>) {
        let registry = self
            .skills
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        (
            Some(registry.activation_selectors()),
            Some(registry.viewable_instance_ids()),
        )
    }
}

fn resolve_policy_prompt(
    policy: &FrozenChildPolicy,
    name: &str,
    prepared: &crate::workspace::PreparedToolInvocation,
    decision: crate::approval::BrokerDecision,
) -> crate::approval::BrokerDecision {
    if decision != crate::approval::BrokerDecision::Prompt {
        return decision;
    }
    match &policy.approval {
        crate::app::ApprovalMode::RunEverything => crate::approval::BrokerDecision::AllowedByRule,
        crate::app::ApprovalMode::AutoReview
            if crate::permission::auto_review_safe_invocation(name, &prepared.arguments) =>
        {
            crate::approval::BrokerDecision::AllowedByRule
        }
        _ => crate::approval::BrokerDecision::ExplicitPrompt,
    }
}

fn prepared_policy_decision(
    policy: &FrozenChildPolicy,
    name: &str,
    prepared: &crate::workspace::PreparedToolInvocation,
) -> crate::approval::BrokerDecision {
    use crate::approval::BrokerDecision;
    let decision = match crate::permission::evaluate(
        name,
        &prepared.arguments,
        &policy.approval_rules,
        &policy.permission_rules,
        &policy.session_grants,
    ) {
        crate::permission::PermissionOutcome::Deny => BrokerDecision::DeniedByRule,
        crate::permission::PermissionOutcome::Ask => BrokerDecision::ExplicitPrompt,
        _ if crate::permission::host_authorized_workflow_shape(name, &prepared.arguments) => {
            BrokerDecision::AllowedByRule
        }
        _ if prepared.network_requested => BrokerDecision::NetworkPrompt,
        crate::permission::PermissionOutcome::Allow => BrokerDecision::AllowedByRule,
        crate::permission::PermissionOutcome::NoMatch => BrokerDecision::Prompt,
    };
    resolve_policy_prompt(policy, name, prepared, decision)
}

fn restrictive_decision(
    left: crate::approval::BrokerDecision,
    right: crate::approval::BrokerDecision,
) -> crate::approval::BrokerDecision {
    use crate::approval::BrokerDecision;
    let rank = |decision| match decision {
        BrokerDecision::DeniedByRule => 5,
        BrokerDecision::ExplicitPrompt => 4,
        BrokerDecision::NetworkPrompt => 3,
        BrokerDecision::Prompt => 2,
        BrokerDecision::AllowedByRule => 1,
    };
    if rank(left) >= rank(right) {
        left
    } else {
        right
    }
}

fn prepare_child_permission(
    runner: &ProviderAgentRunner,
    frozen_policy: Option<&FrozenChildPolicy>,
    workspace: &crate::workspace::WorkspaceContext,
    id: AgentId,
    name: &str,
    arguments: &serde_json::Value,
) -> Result<
    (
        crate::workspace::PreparedToolInvocation,
        crate::approval::BrokerDecision,
    ),
    String,
> {
    let fallback_grants = runner
        .session_grants
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .snapshot();
    let (rules, permissions, grants) = frozen_policy.map_or(
        (
            &runner.approval_rules,
            &runner.permission_rules,
            &fallback_grants,
        ),
        |policy| {
            (
                &policy.approval_rules,
                &policy.permission_rules,
                &policy.session_grants,
            )
        },
    );
    let (prepared, mut decision) = crate::approval::ApprovalBroker::with_policy(
        workspace,
        rules,
        permissions,
        grants,
        crate::approval::ApprovalOrigin::Agent(id.to_string()),
    )
    .prepare(name, arguments, crate::agent::registry::ToolOrigin::BuiltIn)?;
    if let Some(policy) = frozen_policy {
        decision = resolve_policy_prompt(policy, name, &prepared, decision);
    }
    // Evaluate revocations against the *same* canonical invocation. A second
    // prepare would permit a symlink replacement to remap the authorized path.
    if let Some(policy) = runner
        .manager_link
        .get()
        .and_then(std::sync::Weak::upgrade)
        .and_then(|manager| manager.current_permission_policy())
    {
        decision =
            restrictive_decision(decision, prepared_policy_decision(&policy, name, &prepared));
    }
    Ok((prepared, decision))
}

fn child_permission_decision(
    runner: &ProviderAgentRunner,
    frozen_policy: Option<&FrozenChildPolicy>,
    workspace: &crate::workspace::WorkspaceContext,
    id: AgentId,
    name: &str,
    arguments: &serde_json::Value,
) -> Result<crate::approval::BrokerDecision, String> {
    prepare_child_permission(runner, frozen_policy, workspace, id, name, arguments)
        .map(|(_, decision)| decision)
}

async fn execute_child_nested_spawn(
    context: &AgentRunContext,
    tool_context: &ChildToolContext<'_>,
    arguments: &serde_json::Value,
) -> crate::agent::registry::ToolExecutionResult {
    match prepare_child_tool(tool_context, "spawn_agent", arguments) {
        Ok(prepared) => {
            execute_nested_spawn(
                context,
                &prepared.arguments,
                &tool_context.runner.manager_link,
            )
            .await
        }
        Err(result) => *result,
    }
}

async fn execute_nested_spawn(
    context: &AgentRunContext,
    arguments: &serde_json::Value,
    manager_link: &std::sync::OnceLock<std::sync::Weak<AgentManager>>,
) -> crate::agent::registry::ToolExecutionResult {
    let Some(manager) = manager_link.get().and_then(std::sync::Weak::upgrade) else {
        return crate::agent::registry::ToolExecutionResult::Failure(
            "agent manager is unavailable".into(),
        );
    };
    let Some(task) = arguments.get("task").and_then(serde_json::Value::as_str) else {
        return crate::agent::registry::ToolExecutionResult::Failure(
            "nested spawn requires task".into(),
        );
    };
    let requested: Vec<String> = arguments
        .get("tools")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .map(str::to_string)
        .collect();
    if requested.is_empty()
        || requested
            .iter()
            .any(|tool| !context.request.grants.tools.contains(tool))
    {
        return crate::agent::registry::ToolExecutionResult::Denied(
            "Nested agent tool grants must be a non-empty subset of the parent grants".into(),
        );
    }
    let (route, selection) = match resolve_child_route(
        &manager,
        arguments,
        task,
        &context.request.context_summary,
        context.request.route.clone(),
        false,
    ) {
        Ok(resolved) => resolved,
        Err(error) => return crate::agent::registry::ToolExecutionResult::Failure(error),
    };
    let output_contract = match arguments.get("output_schema") {
        None | Some(serde_json::Value::Null) => None,
        Some(schema) => {
            match crate::headless_contract::StructuredOutputContract::from_value(schema.clone()) {
                Ok(contract) => Some(contract),
                Err(error) => return crate::agent::registry::ToolExecutionResult::Failure(error),
            }
        }
    };
    let isolation = match arguments
        .get("isolation")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
    {
        Ok(value) => value.unwrap_or_default(),
        Err(_) => {
            return crate::agent::registry::ToolExecutionResult::Failure(
                "isolation must be shared or worktree".into(),
            )
        }
    };
    let request = AgentSpawnRequest {
        parent: Some(context.id),
        parent_turn: context.request.parent_turn,
        task_id: None,
        task: task.to_string(),
        context_mode: AgentContextMode::Summary,
        context_summary: context.request.context_summary.clone(),
        grants: AgentGrant {
            tools: requested,
            workspace_root_ids: context.request.grants.workspace_root_ids.clone(),
            path_scopes: context.request.grants.path_scopes.clone(),
            command_scopes: context.request.grants.command_scopes.clone(),
            network: context.request.grants.network,
        },
        workspace: context.request.workspace.clone(),
        depth: context.request.depth.saturating_add(1),
        route: route.clone(),
        purpose: context.request.purpose,
        output_contract,
        isolation,
    };
    let origin = if context.graph_budget.is_some() {
        AgentRunOrigin::TaskGraph
    } else {
        AgentRunOrigin::Conversation
    };
    match manager
        .spawn_with_inherited_policy(request, origin, context.frozen_policy.clone())
        .await
    {
        Ok(id) => crate::agent::registry::ToolExecutionResult::Success(format!(
            "Spawned nested agent {id}{}",
            child_route_suffix(route.as_ref(), selection.as_ref())
        )),
        Err(error) => crate::agent::registry::ToolExecutionResult::Failure(error.to_string()),
    }
}

async fn await_child_approval(
    context: &mut AgentRunContext,
    message: String,
) -> Result<bool, String> {
    let _ = context.events.send(RunnerEvent::ApprovalRequired(message));
    loop {
        tokio::select! {
            _ = wait_cancelled(context.cancelled.clone()) => return Err("cancelled".into()),
            command = context.commands.recv() => match command {
                Some(AgentCommand::Approval(approved)) => return Ok(approved),
                Some(AgentCommand::Message { id, text }) => {
                    context.pending_steers.push_back((id, text));
                    let _ = context.events.send(RunnerEvent::Activity(
                        "steering retained for the next provider boundary".into(),
                    ));
                }
                None => return Err("agent control channel closed".into()),
            }
        }
    }
}

pub const MAX_CLARIFICATION_QUESTION_CHARS: usize = 400;
pub const MAX_CLARIFICATION_CHOICES: usize = 8;
pub const MAX_CLARIFICATION_CHOICE_CHARS: usize = 80;
pub const MAX_CLARIFICATION_ANSWER_CHARS: usize = 2_000;

fn valid_clarification_text(value: &str, maximum: usize) -> bool {
    !value.is_empty() && value.len() <= maximum && !value.chars().any(char::is_control)
}

/// The silo'd lane's typed question channel. A graph-task child calls
/// `request_graph_clarification` and parks until the coordinator answers
/// through the steering channel; the answer becomes the tool result and the
/// lane resumes. Only one question can be pending at a time because the lane
/// is parked inside this call.
async fn await_graph_clarification(
    context: &mut AgentRunContext,
    args: crate::agent::registry::arguments::RequestGraphClarificationArgs,
) -> crate::agent::registry::ToolExecutionResult {
    use crate::agent::registry::ToolExecutionResult;
    if !valid_clarification_text(&args.question, MAX_CLARIFICATION_QUESTION_CHARS)
        || args.choices.len() > MAX_CLARIFICATION_CHOICES
        || args
            .choices
            .iter()
            .any(|choice| !valid_clarification_text(choice, MAX_CLARIFICATION_CHOICE_CHARS))
    {
        return ToolExecutionResult::Failure("invalid request_graph_clarification bounds".into());
    }
    let mut prompt = format!("clarification: {}", args.question.trim());
    if !args.choices.is_empty() {
        prompt.push_str(&format!(" [choices: {}]", args.choices.join(", ")));
    }
    let _ = context.events.send(RunnerEvent::ApprovalRequired(prompt));
    tokio::select! {
        _ = wait_cancelled(context.cancelled.clone()) => {
            ToolExecutionResult::Failure("clarification cancelled".into())
        }
        command = context.commands.recv() => match command {
            Some(AgentCommand::Message { id, text }) => {
                let _ = context.events.send(RunnerEvent::SteerDelivered { id });
                if !text.is_empty()
                    && (text.len() > MAX_CLARIFICATION_ANSWER_CHARS
                        || text.chars().any(char::is_control))
                {
                    return ToolExecutionResult::Failure(
                        "clarification answer exceeds bounds".into(),
                    );
                }
                ToolExecutionResult::Success(text)
            }
            Some(AgentCommand::Approval(true)) => ToolExecutionResult::Success(
                "no answer provided; proceed with your best judgment".into(),
            ),
            Some(AgentCommand::Approval(false)) => {
                ToolExecutionResult::Denied("clarification declined by the coordinator".into())
            }
            None => ToolExecutionResult::Failure("agent control channel closed".into()),
        }
    }
}

fn drain_child_steering(
    context: &mut AgentRunContext,
    messages: &mut Vec<crate::agent::client::ChatMessage>,
) {
    while let Ok(command) = context.commands.try_recv() {
        if let AgentCommand::Message { id, text } = command {
            context.pending_steers.push_back((id, text));
        }
    }
    while let Some((id, text)) = context.pending_steers.pop_front() {
        messages.push(crate::agent::client::ChatMessage {
            role: "user".into(),
            content: crate::image_content::ModelContent::text(text),
            tool_calls: None,
            tool_call_id: None,
            response_items: Vec::new(),
        });
        let _ = context.events.send(RunnerEvent::SteerDelivered { id });
    }
}

struct ChildToolContext<'a> {
    runner: &'a ProviderAgentRunner,
    id: AgentId,
    owner: crate::jobs::JobOwner,
    grant: &'a AgentGrant,
    workspace: &'a crate::workspace::WorkspaceContext,
    policy_approved: bool,
    frozen_policy: Option<&'a FrozenChildPolicy>,
}

fn execute_child_discovery(
    context: &ChildToolContext<'_>,
    arguments: &serde_json::Value,
    policy_approved: bool,
    discovery: &crate::tool_discovery::ToolDiscoverySession,
) -> crate::agent::registry::ToolExecutionResult {
    let context = ChildToolContext {
        policy_approved,
        ..*context
    };
    match prepare_child_tool(
        &context,
        crate::tool_discovery::SEARCH_TOOLS_NAME,
        arguments,
    ) {
        Ok(prepared) => discovery.search(&prepared.arguments),
        Err(result) => *result,
    }
}

fn prepare_child_tool(
    context: &ChildToolContext<'_>,
    name: &str,
    arguments: &serde_json::Value,
) -> Result<
    crate::workspace::PreparedToolInvocation,
    Box<crate::agent::registry::ToolExecutionResult>,
> {
    let runner = context.runner;
    let registry = crate::agent::registry::ToolRegistry::builtins();
    if let Err(error) = registry.validate(name, arguments) {
        return Err(Box::new(
            crate::agent::registry::ToolExecutionResult::Failure(error.to_string()),
        ));
    }
    let (prepared, decision) = match prepare_child_permission(
        runner,
        context.frozen_policy,
        context.workspace,
        context.id,
        name,
        arguments,
    ) {
        Ok(result) => result,
        Err(error) => {
            return Err(Box::new(
                crate::agent::registry::ToolExecutionResult::Failure(error),
            ))
        }
    };
    if decision == crate::approval::BrokerDecision::DeniedByRule {
        return Err(Box::new(
            crate::agent::registry::ToolExecutionResult::Denied(
                "Denied by persistent approval rule".into(),
            ),
        ));
    }
    if matches!(
        decision,
        crate::approval::BrokerDecision::ExplicitPrompt
            | crate::approval::BrokerDecision::NetworkPrompt
    ) && !context.policy_approved
    {
        return Err(Box::new(
            crate::agent::registry::ToolExecutionResult::Denied(
                "Child permission requires parent approval".into(),
            ),
        ));
    }
    let Some(spec) = registry.get(name) else {
        return Err(Box::new(
            crate::agent::registry::ToolExecutionResult::Failure(format!("unknown tool: {name}")),
        ));
    };
    for resource in &spec.resources {
        let Some(value) = prepared
            .arguments
            .get(resource.field)
            .and_then(serde_json::Value::as_str)
        else {
            continue;
        };
        let typed_read = spec.mutation == crate::agent::registry::ToolMutation::ReadOnly
            && !spec.capabilities.iter().any(|capability| {
                matches!(
                    capability,
                    crate::agent::registry::ToolCapability::Process
                        | crate::agent::registry::ToolCapability::Git
                )
            });
        let within_scope = match resource.kind {
            crate::agent::registry::ResourceKind::Path
            | crate::agent::registry::ResourceKind::WorkingDirectory => {
                typed_read
                    || context
                        .grant
                        .path_scopes
                        .iter()
                        .any(|scope| std::path::Path::new(value).starts_with(scope))
            }
            crate::agent::registry::ResourceKind::Command => context
                .grant
                .command_scopes
                .iter()
                .any(|scope| scope == value),
            crate::agent::registry::ResourceKind::Url => context.grant.network,
            crate::agent::registry::ResourceKind::JobId => true,
        };
        if !within_scope {
            return Err(Box::new(
                crate::agent::registry::ToolExecutionResult::Denied(format!(
                    "Child resource is outside inherited scope: {}",
                    resource.field
                )),
            ));
        }
    }
    if prepared.network_requested && !context.grant.network {
        return Err(Box::new(
            crate::agent::registry::ToolExecutionResult::Denied(
                "Child network access requires an explicit inherited grant".into(),
            ),
        ));
    }
    Ok(prepared)
}

async fn execute_child_tool(
    context: ChildToolContext<'_>,
    name: &str,
    arguments: &serde_json::Value,
) -> crate::agent::registry::ToolExecutionResult {
    let mut prepared = match prepare_child_tool(&context, name, arguments) {
        Ok(prepared) => prepared,
        Err(result) => return *result,
    };
    if let Err(error) = crate::formatting::prepare_with_authority(
        name,
        &mut prepared.arguments,
        context.workspace,
        context.runner.sandbox_mode,
        |invocation| prepare_child_tool(&context, "run_command", invocation).is_ok(),
    )
    .await
    {
        return crate::agent::registry::ToolExecutionResult::Failure(error);
    }
    let runner = context.runner;
    crate::agent::tools::execute_prepared_tool_in_workspace_policy_owned_with_skills(
        name,
        &prepared.arguments,
        context.workspace,
        runner.command_timeout_secs,
        runner.sandbox_mode,
        context.grant.network && prepared.network_requested,
        context.owner,
        &runner.skills,
    )
    .await
}

fn child_path_scopes(
    workspace: &crate::workspace::WorkspaceContext,
    arguments: &serde_json::Value,
) -> Result<Vec<String>, String> {
    let assigned = arguments
        .get("assigned_files")
        .and_then(serde_json::Value::as_array);
    if assigned.is_none_or(Vec::is_empty) {
        return Ok(workspace
            .execution_roots()
            .iter()
            .map(|root| root.to_string_lossy().into_owned())
            .collect());
    }

    assigned
        .into_iter()
        .flatten()
        .map(|value| {
            let raw = value
                .as_str()
                .ok_or_else(|| "assigned_files entries must be paths".to_string())?;
            let resolved = workspace
                .resolve(raw, crate::workspace::PathOperation::Read)
                .map_err(|error| error.to_string())?;
            if workspace.root_relative(&resolved).is_none()
                && !workspace.has_session_write(&resolved)
            {
                return Err(format!(
                    "assigned file is outside workspace roots: {}",
                    resolved.display()
                ));
            }
            Ok(resolved.to_string_lossy().into_owned())
        })
        .collect()
}

async fn cleanup_owned_jobs(owner: crate::jobs::JobOwner) {
    crate::browser::BrowserManager::process()
        .shutdown_owner(owner)
        .await;
    let manager = crate::jobs::JobManager::process();
    let jobs = manager.list_owned(owner).await;
    for job in jobs {
        if !job.background && !job.status.is_terminal() {
            let _ = manager.kill_owned(job.id, owner).await;
        }
    }
}

pub async fn execute_manager_tool(
    name: &str,
    arguments: &serde_json::Value,
    manager: &Arc<AgentManager>,
    work_state: &Arc<std::sync::Mutex<crate::work::WorkState>>,
    workspace: &crate::workspace::WorkspaceContext,
) -> Option<crate::agent::registry::ToolExecutionResult> {
    use crate::agent::registry::ToolExecutionResult;
    let failure = |error: AgentManagerError| ToolExecutionResult::Failure(error.to_string());
    let result = match name {
        "spawn_agent" => {
            let task = arguments.get("task")?.as_str()?.to_string();
            let requested: Vec<String> = arguments
                .get("tools")?
                .as_array()?
                .iter()
                .filter_map(|value| value.as_str().map(str::to_string))
                .collect();
            let available: std::collections::HashSet<String> =
                match manager.available_subagent_tools() {
                    Ok(tools) => tools
                        .into_iter()
                        .map(|definition| definition.function.name)
                        .collect(),
                    Err(error) => return Some(failure(error)),
                };
            if requested.is_empty() || requested.iter().any(|tool| !available.contains(tool)) {
                return Some(ToolExecutionResult::Failure(
                    "spawn_agent requires explicit valid non-agent tool grants".into(),
                ));
            }
            if manager.is_headless_read_only()
                && arguments
                    .get("commands")
                    .and_then(serde_json::Value::as_array)
                    .is_some_and(|commands| !commands.is_empty())
            {
                return Some(ToolExecutionResult::Denied(
                    "headless children cannot receive shell command grants".into(),
                ));
            }
            let task_id = arguments
                .get("task_id")
                .and_then(serde_json::Value::as_str)
                .and_then(|value| Uuid::parse_str(value).ok())
                .map(crate::work::TaskId);
            let work_summary = work_state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .render_plan();
            let assigned_files = arguments
                .get("assigned_files")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(serde_json::Value::as_str)
                .collect::<Vec<_>>()
                .join(", ");
            let context_summary = format!(
                "Goal: {task}\nApproved plan/task:\n{work_summary}\nConstraints: exact tool grants; workspace roots do not widen; network denied\nCheckpoint: none\nAssigned files: {assigned_files}"
            );
            let (route, selection) = match resolve_child_route(
                manager,
                arguments,
                &task,
                &context_summary,
                manager.default_delegation_route_for_turn(manager.parent_turn()),
                manager.learned_delegation_enabled(),
            ) {
                Ok(resolved) => resolved,
                Err(error) => return Some(ToolExecutionResult::Failure(error)),
            };
            let request = AgentSpawnRequest {
                parent: None,
                parent_turn: manager.parent_turn(),
                task_id,
                task,
                context_mode: AgentContextMode::Summary,
                context_summary,
                grants: AgentGrant {
                    tools: requested,
                    workspace_root_ids: workspace
                        .roots()
                        .iter()
                        .map(|root| root.id.to_string())
                        .collect(),
                    path_scopes: match child_path_scopes(workspace, arguments) {
                        Ok(scopes) => scopes,
                        Err(error) => return Some(ToolExecutionResult::Failure(error)),
                    },
                    command_scopes: arguments
                        .get("commands")
                        .and_then(serde_json::Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(serde_json::Value::as_str)
                        .map(str::to_string)
                        .collect(),
                    network: false,
                },
                workspace: workspace.clone(),
                depth: 1,
                route: route.clone(),
                purpose: AgentRunPurpose::Subagent,
                output_contract: match arguments.get("output_schema") {
                    None | Some(serde_json::Value::Null) => None,
                    Some(schema) => {
                        match crate::headless_contract::StructuredOutputContract::from_value(
                            schema.clone(),
                        ) {
                            Ok(contract) => Some(contract),
                            Err(error) => return Some(ToolExecutionResult::Failure(error)),
                        }
                    }
                },
                isolation: match arguments
                    .get("isolation")
                    .and_then(serde_json::Value::as_str)
                {
                    None | Some("shared") => AgentIsolation::Shared,
                    Some("worktree") => AgentIsolation::Worktree,
                    Some(_) => {
                        return Some(ToolExecutionResult::Failure(
                            "isolation must be shared or worktree".into(),
                        ))
                    }
                },
            };
            match manager.spawn(request).await {
                Ok(id) => ToolExecutionResult::Success(format!(
                    "Spawned agent {id}{}",
                    child_route_suffix(route.as_ref(), selection.as_ref())
                )),
                Err(error) => failure(error),
            }
        }
        "spawn_agent_batch" => {
            let tasks = arguments.get("tasks")?.as_array()?;
            let mut requests = Vec::with_capacity(tasks.len());
            let mut route_details = Vec::with_capacity(tasks.len());
            for task in tasks {
                match build_spawn_request(task, manager, work_state, workspace) {
                    Ok((request, selection)) => {
                        route_details.push(child_route_suffix(
                            request.route.as_ref(),
                            selection.as_ref(),
                        ));
                        requests.push(request);
                    }
                    Err(error) => return Some(ToolExecutionResult::Failure(error)),
                }
            }
            match manager
                .spawn_batch(requests, AgentRunOrigin::Conversation)
                .await
            {
                Ok((batch_id, ids)) => {
                    let rows = ids
                        .iter()
                        .zip(route_details.iter())
                        .enumerate()
                        .map(|(index, (id, details))| format!("{}. {id}{details}", index + 1))
                        .collect::<Vec<_>>();
                    ToolExecutionResult::Success(format!(
                        "Spawned batch {batch_id}\n{}",
                        rows.join("\n")
                    ))
                }
                Err(error) => failure(error),
            }
        }
        "await_agent" => {
            let id = parse_agent_id(arguments)?;
            let timeout_secs = match arguments.get("timeout_secs") {
                None | Some(serde_json::Value::Null) => None,
                Some(value) => match value.as_u64() {
                    Some(value) if value <= MAX_AWAIT_SECS => Some(value),
                    _ => {
                        return Some(ToolExecutionResult::Failure(format!(
                            "timeout_secs must be an integer from 0 to {MAX_AWAIT_SECS}"
                        )))
                    }
                },
            };
            match manager.await_agent_bounded(id, timeout_secs).await {
                Ok(snapshot) => ToolExecutionResult::Success(format_agent_snapshot(&snapshot)),
                Err(error) => failure(error),
            }
        }
        "await_agent_batch" => {
            let id = arguments
                .get("batch_id")?
                .as_str()?
                .parse::<AgentBatchId>()
                .ok()?;
            let timeout_secs = match arguments.get("timeout_secs") {
                None | Some(serde_json::Value::Null) => None,
                Some(value) => match value.as_u64() {
                    Some(value) if value <= MAX_AWAIT_SECS => Some(value),
                    _ => {
                        return Some(ToolExecutionResult::Failure(format!(
                            "timeout_secs must be an integer from 0 to {MAX_AWAIT_SECS}"
                        )))
                    }
                },
            };
            match manager.await_batch_bounded(id, timeout_secs).await {
                Ok(snapshot) => ToolExecutionResult::Success(format_agent_batch(&snapshot)),
                Err(error) => failure(error),
            }
        }
        "send_agent_message" => {
            let id = parse_agent_id(arguments)?;
            let message = arguments.get("message")?.as_str()?.to_string();
            match manager.send_message(id, message).await {
                Ok(steer_id) => {
                    ToolExecutionResult::Success(format!("Steering {steer_id} queued for {id}"))
                }
                Err(error) => failure(error),
            }
        }
        "cancel_agent" => {
            let id = parse_agent_id(arguments)?;
            match manager.cancel(id).await {
                Ok(()) => ToolExecutionResult::Success(format!("Cancellation requested for {id}")),
                Err(error) => failure(error),
            }
        }
        "set_agent_spawning" => {
            let paused = arguments.get("paused")?.as_bool()?;
            manager.set_spawns_paused(paused);
            ToolExecutionResult::Success(if paused {
                "Future child spawning paused; existing children continue".into()
            } else {
                "Future child spawning resumed".into()
            })
        }
        "cancel_agent_job" => {
            let id = parse_agent_id(arguments)?;
            let job_id = arguments
                .get("job_id")?
                .as_str()?
                .parse::<crate::jobs::JobId>()
                .ok()?;
            match manager.cancel_attributed_job(id, job_id).await {
                Ok(()) => ToolExecutionResult::Success(format!(
                    "Cancellation requested for attributed job {job_id}"
                )),
                Err(error) => failure(error),
            }
        }
        "list_agent_routes" => ToolExecutionResult::Success(
            json!({"routes": manager.available_routes(), "authentication": "not_checked"})
                .to_string(),
        ),
        "list_agents" => {
            let snapshots = manager.list().await;
            ToolExecutionResult::Success(if snapshots.is_empty() {
                "No child agents".into()
            } else {
                snapshots
                    .iter()
                    .map(format_agent_snapshot)
                    .collect::<Vec<_>>()
                    .join("\n")
            })
        }
        "task" => {
            let name = arguments.get("subagent_name")?.as_str()?;
            let task = arguments.get("prompt")?.as_str()?.to_string();
            let definition = builtin_subagents()
                .into_iter()
                .find(|agent| agent.name == name);
            let Some(definition) = definition else {
                return Some(ToolExecutionResult::Failure(format!(
                    "unknown subagent: {name}"
                )));
            };
            if manager.is_headless_read_only() && definition.tools.is_empty() {
                return Some(ToolExecutionResult::Denied(format!(
                    "named agent {name} is mutating or unbounded and cannot run headlessly"
                )));
            }
            let mut requested: Vec<_> = if definition.tools.is_empty() {
                match manager.available_subagent_tools() {
                    Ok(tools) => tools
                        .into_iter()
                        .filter(|definition| definition.function.name != "spawn_agent")
                        .map(|definition| definition.function.name)
                        .collect(),
                    Err(error) => return Some(failure(error)),
                }
            } else {
                definition.tools
            };
            let available = match manager.available_subagent_tools() {
                Ok(tools) => tools
                    .into_iter()
                    .map(|definition| definition.function.name)
                    .collect::<HashSet<_>>(),
                Err(error) => return Some(failure(error)),
            };
            // A named preset is an implicit capability ceiling. Narrow it to
            // this run's admitted tools; explicit spawn grants remain strict.
            requested.retain(|tool| available.contains(tool.as_str()));
            if requested.is_empty() {
                return Some(ToolExecutionResult::Failure(format!(
                    "named agent {name} has no available tools under the run policy"
                )));
            }
            let work_summary = work_state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .render_plan();
            let context_summary = format!(
                "Goal: {task}\nApproved plan/task:\n{work_summary}\nConstraints: legacy foreground compatibility with exact tool grants; network denied\nCheckpoint: none\nAssigned files: none"
            );
            let (route, selection) = match resolve_child_route(
                manager,
                arguments,
                &task,
                &context_summary,
                manager.default_delegation_route_for_turn(manager.parent_turn()),
                manager.learned_delegation_enabled(),
            ) {
                Ok(resolved) => resolved,
                Err(error) => return Some(ToolExecutionResult::Failure(error)),
            };
            let request = AgentSpawnRequest {
                parent: None,
                parent_turn: manager.parent_turn(),
                task_id: None,
                task,
                context_mode: AgentContextMode::Summary,
                context_summary,
                grants: AgentGrant {
                    tools: requested,
                    workspace_root_ids: workspace
                        .roots()
                        .iter()
                        .map(|root| root.id.to_string())
                        .collect(),
                    path_scopes: workspace
                        .execution_roots()
                        .iter()
                        .map(|root| root.to_string_lossy().into_owned())
                        .collect(),
                    command_scopes: Vec::new(),
                    network: false,
                },
                workspace: workspace.clone(),
                depth: 1,
                route: route.clone(),
                purpose: AgentRunPurpose::Subagent,
                output_contract: None,
                isolation: AgentIsolation::Shared,
            };
            match manager.spawn(request).await {
                Ok(id) => {
                    let _ = manager
                        .set_role(
                            id,
                            match name {
                                "explore" => "Explorer",
                                "plan" => "Planner",
                                _ => "Worker",
                            },
                        )
                        .await;
                    match manager.await_agent(id).await {
                        Ok(snapshot) if snapshot.status == AgentStatus::Completed => {
                            let summary = snapshot
                                .result_summary
                                .unwrap_or_else(|| "completed".into());
                            let details = child_route_suffix(route.as_ref(), selection.as_ref());
                            ToolExecutionResult::Success(if details.is_empty() {
                                summary
                            } else {
                                format!("{}\n\n{summary}", details.trim_start_matches(" · "))
                            })
                        }
                        Ok(snapshot) => {
                            ToolExecutionResult::Failure(format_agent_snapshot(&snapshot))
                        }
                        Err(error) => failure(error),
                    }
                }
                Err(error) => failure(error),
            }
        }
        _ => return None,
    };
    Some(result)
}

fn parse_agent_id(arguments: &serde_json::Value) -> Option<AgentId> {
    arguments.get("agent_id")?.as_str()?.parse().ok()
}

fn build_spawn_request(
    arguments: &serde_json::Value,
    manager: &AgentManager,
    work_state: &Arc<std::sync::Mutex<crate::work::WorkState>>,
    workspace: &crate::workspace::WorkspaceContext,
) -> Result<(AgentSpawnRequest, Option<crate::pricing::RouteSelection>), String> {
    let task = arguments
        .get("task")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "batch task requires task".to_string())?
        .to_string();
    let requested = arguments
        .get("tools")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "batch task requires tools".to_string())?
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| "tool grants must be strings".to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let available = manager
        .available_subagent_tools()
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|definition| definition.function.name)
        .collect::<HashSet<_>>();
    if requested.is_empty() || requested.iter().any(|tool| !available.contains(tool)) {
        return Err("spawn_agent_batch requires explicit valid non-agent tool grants".into());
    }
    if manager.is_headless_read_only()
        && arguments
            .get("commands")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|commands| !commands.is_empty())
    {
        return Err("headless children cannot receive shell command grants".into());
    }
    let task_id = arguments
        .get("task_id")
        .and_then(serde_json::Value::as_str)
        .map(|value| {
            Uuid::parse_str(value)
                .map(crate::work::TaskId)
                .map_err(|_| "task_id must be a UUID".to_string())
        })
        .transpose()?;
    let work_summary = work_state
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .render_plan();
    let assigned_files = arguments
        .get("assigned_files")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .map(|value| {
            value
                .as_str()
                .ok_or_else(|| "assigned_files must contain strings".to_string())
        })
        .collect::<Result<Vec<_>, _>>()?
        .join(", ");
    let context_summary = format!(
        "Goal: {task}\nApproved plan/task:\n{work_summary}\nConstraints: exact tool grants; workspace roots do not widen; network denied\nCheckpoint: none\nAssigned files: {assigned_files}"
    );
    let (route, selection) = resolve_child_route(
        manager,
        arguments,
        &task,
        &context_summary,
        manager.default_delegation_route_for_turn(manager.parent_turn()),
        manager.learned_delegation_enabled(),
    )?;
    let output_contract = match arguments.get("output_schema") {
        None | Some(serde_json::Value::Null) => None,
        Some(schema) => {
            Some(crate::headless_contract::StructuredOutputContract::from_value(schema.clone())?)
        }
    };
    let isolation = arguments
        .get("isolation")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|_| "isolation must be shared or worktree".to_string())?
        .unwrap_or_default();
    let command_scopes = arguments
        .get("commands")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .map(|value| {
            value
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| "commands must contain strings".to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok((
        AgentSpawnRequest {
            parent: None,
            parent_turn: manager.parent_turn(),
            task_id,
            task,
            context_mode: AgentContextMode::Summary,
            context_summary,
            grants: AgentGrant {
                tools: requested,
                workspace_root_ids: workspace
                    .roots()
                    .iter()
                    .map(|root| root.id.to_string())
                    .collect(),
                path_scopes: child_path_scopes(workspace, arguments)?,
                command_scopes,
                network: false,
            },
            workspace: workspace.clone(),
            depth: 1,
            route,
            purpose: AgentRunPurpose::Subagent,
            output_contract,
            isolation,
        },
        selection,
    ))
}

fn requested_complexity(
    arguments: &serde_json::Value,
) -> Result<Option<crate::pricing::Complexity>, String> {
    arguments
        .get("complexity")
        .filter(|value| !value.is_null())
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|_| "complexity must be simple, standard, or complex".to_string())
}

fn resolve_child_route(
    manager: &AgentManager,
    arguments: &serde_json::Value,
    task: &str,
    context_summary: &str,
    inherited: Option<AgentRoute>,
    learned_default: bool,
) -> Result<(Option<AgentRoute>, Option<crate::pricing::RouteSelection>), String> {
    if let Some(value) = arguments.get("route").filter(|value| !value.is_null()) {
        if arguments
            .get("complexity")
            .is_some_and(|value| !value.is_null())
        {
            return Err("route and complexity cannot be combined; choose an exact route or automatic cost routing".into());
        }
        let route: AgentRoute = serde_json::from_value(value.clone())
            .map_err(|_| "route must contain only provider and model strings".to_string())?;
        if route.provider.trim().is_empty() || route.model.trim().is_empty() {
            return Err(
                "route provider and model must be nonempty exact configured identities".into(),
            );
        }
        if !manager.available_routes().contains(&route) {
            return Err(format!(
                "subagent route is unavailable: {}; use list_agent_routes for configured routes",
                route_identity(&route)
            ));
        }
        return Ok((Some(route), None));
    }
    let complexity = requested_complexity(arguments)?
        .or_else(|| learned_default.then_some(crate::pricing::Complexity::Standard));
    match complexity {
        Some(complexity) => {
            let selection = manager.select_delegation_route(complexity, task, context_summary)?;
            let route = AgentRoute {
                provider: selection.provider.clone(),
                model: selection.model.clone(),
            };
            Ok((Some(route), Some(selection)))
        }
        None => Ok((inherited, None)),
    }
}

fn route_identity(route: &AgentRoute) -> String {
    format!(
        "{}/{}",
        crate::terminal_text::sanitize_terminal_text(&route.provider),
        crate::terminal_text::sanitize_terminal_text(&route.model)
    )
}

fn child_route_suffix(
    route: Option<&AgentRoute>,
    selection: Option<&crate::pricing::RouteSelection>,
) -> String {
    let mut details = route.map_or_else(String::new, |route| {
        format!(" · route={}", route_identity(route))
    });
    if let Some(selection) = selection {
        details.push_str(&format!(" · routed {}", selection.render()));
    }
    details
}

pub fn format_agent_snapshot(snapshot: &AgentSnapshot) -> String {
    let route = match (&snapshot.provider, &snapshot.model) {
        (Some(provider), Some(model)) => format!(
            " route={}",
            route_identity(&AgentRoute {
                provider: provider.clone(),
                model: model.clone()
            })
        ),
        _ => String::new(),
    };
    let mut output = format!(
        "{} [{:?}]{} task={} output={}",
        snapshot.id,
        snapshot.status,
        route,
        crate::terminal_text::sanitize_terminal_text(&snapshot.task),
        crate::terminal_text::sanitize_terminal_text(
            snapshot
                .result_summary
                .as_deref()
                .unwrap_or(&snapshot.recent_output)
        )
    );
    if let Some(digest) = &snapshot.result_sha256 {
        output.push_str(&format!(" sha256={digest}"));
    }
    if snapshot.structured_result.is_some() {
        output.push_str(" structured=validated");
    }
    if snapshot.structured_repair_attempted {
        output.push_str(" structured_repair=1/1");
    }
    if let Some(spill) = &snapshot.spill {
        output.push_str(&format!(
            " spill={} spill_sha256={} spill_bytes={}{}",
            spill.path.display(),
            spill.sha256,
            spill.bytes,
            if spill.truncated {
                " spill_truncated=true"
            } else {
                ""
            }
        ));
    }
    if let Some(worktree) = &snapshot.worktree {
        output.push_str(&format!(
            " worktree={} branch={} dirty={}",
            worktree.path.display(),
            worktree.branch,
            worktree
                .dirty
                .map_or_else(|| "unknown".into(), |dirty| dirty.to_string())
        ));
    }
    if !snapshot.attributed_jobs.is_empty() {
        output.push_str(&format!(
            " background_jobs={}",
            snapshot
                .attributed_jobs
                .iter()
                .map(|job| format!("{}:{}", job.job_id, job.status))
                .collect::<Vec<_>>()
                .join(",")
        ));
    }
    for message in &snapshot.missed_steer {
        output.push_str(&format!(
            " missed_steer={}",
            crate::terminal_text::sanitize_terminal_text(message)
        ));
    }
    output
}

pub fn format_agent_batch(snapshot: &AgentBatchSnapshot) -> String {
    let completed = snapshot
        .agents
        .iter()
        .filter(|agent| agent.status == AgentStatus::Completed)
        .count();
    let mut lines = vec![format!(
        "batch {} terminal={} completed={}/{}",
        snapshot.id,
        snapshot.terminal,
        completed,
        snapshot.agents.len()
    )];
    lines.extend(snapshot.agents.iter().map(format_agent_snapshot));
    lines.join("\n")
}

#[derive(Clone)]
struct LifecycleHooks {
    runner: crate::hooks::HookRunner,
    owner: crate::hooks::HookOwner,
}

struct AgentRecord {
    outcome: Option<crate::session_outcome::ScopedOutcome>,
    snapshot: AgentSnapshot,
    parent_turn: Uuid,
    command_tx: mpsc::Sender<AgentCommand>,
    cancel_tx: watch::Sender<bool>,
    notify: Arc<Notify>,
    pending_steers: Vec<(Uuid, String)>,
    output_contract: Option<crate::headless_contract::StructuredOutputContract>,
    full_output: String,
    spill_truncated: bool,
    lifecycle_stop_claimed: bool,
    job_owner: crate::jobs::JobOwner,
    graph_budget: Option<crate::agent_graph::AgentAutonomyBudget>,
}

struct AgentManagerInner {
    records: HashMap<AgentId, AgentRecord>,
    batches: HashMap<AgentBatchId, Vec<AgentId>>,
    mutations: Vec<AgentMutationReceipt>,
    incomplete_mutation_turns: HashSet<Uuid>,
}

#[derive(Debug, Clone)]
pub struct AgentBatchSnapshot {
    pub id: AgentBatchId,
    pub agents: Vec<AgentSnapshot>,
    pub terminal: bool,
}

pub struct AgentManager {
    inner: Arc<Mutex<AgentManagerInner>>,
    runner: Arc<dyn AgentRunner>,
    limits: AgentLimits,
    semaphore: Arc<Semaphore>,
    current_parent_turn: std::sync::Mutex<Uuid>,
    outcome_parent: std::sync::Mutex<Option<(Uuid, crate::session_outcome::OutcomeTracker)>>,
    tool_policy: crate::tool_selection::ToolSelectionPolicy,
    skills_enabled: bool,
    images_supported: bool,
    delegation_router: std::sync::RwLock<Option<crate::pricing::DelegationRouter>>,
    learned_delegation: std::sync::atomic::AtomicBool,
    delegation_allowlist: std::sync::RwLock<HashSet<AgentRoute>>,
    parent_route: std::sync::RwLock<Option<AgentRoute>>,
    default_delegation_route: std::sync::RwLock<Option<AgentRoute>>,
    headless_read_only: std::sync::atomic::AtomicBool,
    spawns_paused: std::sync::RwLock<bool>,
    headless_noninteractive: std::sync::atomic::AtomicBool,
    parent_constraints: std::sync::RwLock<HashMap<Uuid, String>>,
    frozen_parent_routes: std::sync::RwLock<HashMap<Uuid, Option<AgentRoute>>>,
    frozen_parent_policies: std::sync::RwLock<HashMap<Uuid, Arc<FrozenChildPolicy>>>,
    current_permission_policy: std::sync::RwLock<Option<Arc<FrozenChildPolicy>>>,
    worktree_store: std::sync::RwLock<Option<crate::worktree::WorktreeStore>>,
    artifact_root: PathBuf,
    lifecycle_hooks: std::sync::RwLock<HashMap<Uuid, LifecycleHooks>>,
    recovery_contexts: std::sync::RwLock<HashMap<Uuid, crate::recovery::RecoveryContext>>,
    activity_tx: watch::Sender<Vec<AgentActivitySnapshot>>,
    graph_budget:
        std::sync::Mutex<Option<(crate::work::PlanId, crate::agent_graph::AgentAutonomyBudget)>>,
}

impl std::fmt::Debug for AgentManager {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AgentManager")
            .field("limits", &self.limits)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AgentManagerError {
    #[error("subagent depth {depth} exceeds limit {limit}")]
    Depth { depth: u8, limit: u8 },
    #[error("parent turn already spawned the maximum {0} children")]
    ChildrenPerTurn(usize),
    #[error("unknown agent: {0}")]
    Unknown(AgentId),
    #[error("agent is not awaiting approval")]
    NotWaitingApproval,
    #[error("agent command channel is closed")]
    ChannelClosed,
    #[error("agent is already terminal: {0}")]
    AlreadyTerminal(AgentId),
    #[error("invalid steering message: {0}")]
    InvalidSteer(String),
    #[error("invalid agent wait: {0}")]
    InvalidWait(String),
    #[error("agent steering queue is full")]
    SteerQueueFull,
    #[error("tool selection failed: {0}")]
    ToolSelection(String),
    #[error("subagent tool grant is not available: {0}")]
    InvalidToolGrant(String),
    #[error("future subagent spawning is paused")]
    SpawnsPaused,
    #[error("invalid agent batch: {0}")]
    InvalidBatch(String),
    #[error("unknown agent batch: {0}")]
    UnknownBatch(AgentBatchId),
    #[error("agent worktree isolation is unavailable: {0}")]
    Worktree(String),
    #[error("agent graph autonomy budget is unavailable")]
    GraphBudgetUnavailable,
    #[error("agent graph autonomy budget exhausted: {0:?}")]
    GraphBudget(crate::agent_graph::AgentBudgetLimitKind),
}

impl AgentManager {
    pub fn new(runner: Arc<dyn AgentRunner>, limits: AgentLimits) -> Arc<Self> {
        Self::new_with_tool_policy(
            runner,
            limits,
            crate::tool_selection::ToolSelectionPolicy::default(),
            true,
            false,
        )
    }

    pub fn new_with_tool_policy(
        runner: Arc<dyn AgentRunner>,
        limits: AgentLimits,
        tool_policy: crate::tool_selection::ToolSelectionPolicy,
        skills_enabled: bool,
        images_supported: bool,
    ) -> Arc<Self> {
        let (activity_tx, _) = watch::channel(Vec::new());
        Arc::new(Self {
            inner: Arc::new(Mutex::new(AgentManagerInner {
                records: HashMap::new(),
                batches: HashMap::new(),
                mutations: Vec::new(),
                incomplete_mutation_turns: HashSet::new(),
            })),
            runner,
            semaphore: Arc::new(Semaphore::new(limits.max_concurrent)),
            current_parent_turn: std::sync::Mutex::new(Uuid::new_v4()),
            outcome_parent: std::sync::Mutex::new(None),
            limits,
            tool_policy,
            skills_enabled,
            images_supported,
            delegation_router: std::sync::RwLock::new(None),
            learned_delegation: std::sync::atomic::AtomicBool::new(false),
            delegation_allowlist: std::sync::RwLock::new(HashSet::new()),
            parent_route: std::sync::RwLock::new(None),
            default_delegation_route: std::sync::RwLock::new(None),
            headless_read_only: std::sync::atomic::AtomicBool::new(false),
            spawns_paused: std::sync::RwLock::new(false),
            headless_noninteractive: std::sync::atomic::AtomicBool::new(false),
            parent_constraints: std::sync::RwLock::new(HashMap::new()),
            frozen_parent_routes: std::sync::RwLock::new(HashMap::new()),
            frozen_parent_policies: std::sync::RwLock::new(HashMap::new()),
            current_permission_policy: std::sync::RwLock::new(None),
            worktree_store: std::sync::RwLock::new(None),
            artifact_root: std::env::temp_dir()
                .join(format!("ca-agent-artifacts-{}", Uuid::new_v4())),
            lifecycle_hooks: std::sync::RwLock::new(HashMap::new()),
            recovery_contexts: std::sync::RwLock::new(HashMap::new()),
            activity_tx,
            graph_budget: std::sync::Mutex::new(None),
        })
    }

    /// Bind only interactive lifecycle events to the exact parent request owner.
    /// A spawn captures this binding, so later pane/turn changes cannot steal its effects.
    pub fn set_lifecycle_hooks(
        &self,
        hooks: Option<(crate::hooks::HookRunner, crate::hooks::HookOwner)>,
    ) -> Result<(), &'static str> {
        let mut bindings = self
            .lifecycle_hooks
            .write()
            .unwrap_or_else(|p| p.into_inner());
        let Some((runner, owner)) = hooks else {
            bindings.clear();
            return Ok(());
        };
        let turn = owner
            .turn_id
            .ok_or("Lifecycle hooks require a parent turn owner")?;
        if !bindings.contains_key(&turn) && bindings.len() >= 128 {
            return Err("Lifecycle hook owner limit reached; retire completed turns");
        }
        bindings.insert(turn, LifecycleHooks { runner, owner });
        Ok(())
    }

    pub fn clear_lifecycle_hooks_for_turn(&self, parent_turn: Uuid) {
        self.frozen_parent_policies
            .write()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&parent_turn);
        self.parent_constraints
            .write()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&parent_turn);
        self.lifecycle_hooks
            .write()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&parent_turn);
        self.recovery_contexts
            .write()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&parent_turn);
    }

    pub fn set_recovery_context(
        &self,
        parent_turn: Uuid,
        context: crate::recovery::RecoveryContext,
    ) -> Result<(), &'static str> {
        let mut contexts = self
            .recovery_contexts
            .write()
            .unwrap_or_else(|p| p.into_inner());
        if !contexts.contains_key(&parent_turn) && contexts.len() >= 128 {
            return Err("Child recovery owner limit reached; retire completed turns");
        }
        contexts.insert(parent_turn, context);
        Ok(())
    }

    fn recovery_context_for(
        &self,
        request: &AgentSpawnRequest,
        id: AgentId,
    ) -> Option<crate::recovery::RecoveryContext> {
        self.recovery_contexts
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .get(&request.parent_turn)
            .map(|context| context.with_agent(id.to_string(), request.workspace.execution_roots()))
    }

    fn lifecycle_hooks_for(&self, id: AgentId, parent_turn: Uuid) -> Option<LifecycleHooks> {
        self.lifecycle_hooks
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .get(&parent_turn)
            .map(|binding| LifecycleHooks {
                runner: binding.runner.clone(),
                owner: binding.owner.for_child(id.0, parent_turn),
            })
    }

    fn available_subagent_tools(
        &self,
    ) -> Result<Vec<crate::agent::client::ToolDef>, AgentManagerError> {
        let registry = crate::agent::registry::ToolRegistry::builtins();
        let (skill_activation_ids, skill_view_ids) = self.runner.skill_constraint_ids();
        let capability_allowed = registry.specs().iter().filter_map(|spec| {
            (!spec
                .capabilities
                .contains(&crate::agent::registry::ToolCapability::Subagent)
                || spec.name.as_str() == "spawn_agent")
                .then_some(spec.name.as_str())
        });
        crate::tool_selection::resolve_internal_tools(
            registry,
            crate::agent::registry::ToolProfile::Subagent,
            &self.tool_policy,
            &crate::tool_selection::ToolSelectionConstraints {
                skills_enabled: self.skills_enabled,
                skill_activation_ids,
                skill_view_ids,
                mcp_enabled: false,
                images_supported: self.images_supported,
                ..crate::tool_selection::ToolSelectionConstraints::default()
            }
            .restricted_to(capability_allowed),
        )
        .map(crate::tool_selection::ResolvedToolSet::into_definitions)
        .map_err(|error| AgentManagerError::ToolSelection(error.to_string()))
    }

    pub fn available_tool_names(&self) -> Result<Vec<String>, AgentManagerError> {
        let mut names = self
            .available_subagent_tools()?
            .into_iter()
            .map(|definition| definition.function.name)
            .filter(|name| {
                !matches!(
                    name.as_str(),
                    "spawn_agent"
                        | "propose_plan"
                        | "configure_agent_graph"
                        | "activate_skill"
                        | "propose_memory"
                )
            })
            .collect::<Vec<_>>();
        names.sort();
        names.dedup();
        Ok(names)
    }

    pub fn available_routes(&self) -> Vec<AgentRoute> {
        let mut routes = self.runner.available_routes();
        routes.sort_by(|left, right| {
            left.provider
                .cmp(&right.provider)
                .then_with(|| left.model.cmp(&right.model))
        });
        routes.dedup();
        routes
    }

    pub fn begin_parent_turn(&self) -> Uuid {
        let turn = Uuid::new_v4();
        *self
            .current_parent_turn
            .lock()
            .unwrap_or_else(|poison| poison.into_inner()) = turn;
        turn
    }

    pub fn begin_parent_task(&self, outcome: &crate::session_outcome::OutcomeTracker) -> Uuid {
        let turn = self.begin_parent_turn();
        *self
            .outcome_parent
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = Some((turn, outcome.clone()));
        turn
    }

    /// Continue an already owned parent request without replacing its identity.
    pub fn resume_parent_task(
        &self,
        turn: Uuid,
        outcome: &crate::session_outcome::OutcomeTracker,
    ) -> Result<(), &'static str> {
        if self.current_parent_turn() != turn {
            return Err("Parent turn ownership changed; retry the request explicitly");
        }
        *self
            .outcome_parent
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = Some((turn, outcome.clone()));
        Ok(())
    }

    fn outcome_parent_for(&self, turn: Uuid) -> Option<crate::session_outcome::OutcomeTracker> {
        self.outcome_parent
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .filter(|(id, _)| *id == turn)
            .map(|(_, outcome)| outcome.clone())
    }

    pub fn subscribe_activity(&self) -> watch::Receiver<Vec<AgentActivitySnapshot>> {
        self.activity_tx.subscribe()
    }

    pub async fn available_graph_slots(&self) -> usize {
        let parent_turn = self.parent_turn();
        let inner = self.inner.lock().await;
        let active = inner
            .records
            .values()
            .filter(|record| !record.snapshot.status.terminal())
            .count();
        let used_this_turn = inner
            .records
            .values()
            .filter(|record| {
                record.parent_turn == parent_turn
                    && record.snapshot.origin != AgentRunOrigin::AutomaticReview
            })
            .count();
        self.limits.max_concurrent.saturating_sub(active).min(
            self.limits
                .max_children_per_turn
                .saturating_sub(used_this_turn),
        )
    }

    pub async fn remaining_children_this_turn(&self) -> usize {
        let parent_turn = self.parent_turn();
        let inner = self.inner.lock().await;
        let used = inner
            .records
            .values()
            .filter(|record| {
                record.parent_turn == parent_turn
                    && record.snapshot.origin != AgentRunOrigin::AutomaticReview
            })
            .count();
        self.limits.max_children_per_turn.saturating_sub(used)
    }

    pub fn current_parent_turn(&self) -> Uuid {
        *self
            .current_parent_turn
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    pub fn replace_route_clients(&self, clients: AgentRouteClients) {
        self.runner.replace_route_clients(clients);
    }

    pub fn verification_sandbox_mode(&self) -> Option<crate::sandbox::SandboxMode> {
        self.runner.verification_sandbox_mode()
    }

    pub fn configure_graph_budget(
        &self,
        plan_id: crate::work::PlanId,
        limits: crate::agent_graph::AgentGraphBudgetLimitsV1,
        usage: crate::agent_graph::AgentGraphBudgetUsageV1,
    ) -> Result<crate::agent_graph::AgentAutonomyBudget, AgentManagerError> {
        let mut configured = self
            .graph_budget
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if let Some((configured_plan, budget)) = configured.as_ref() {
            if *configured_plan == plan_id && budget.limits() == limits {
                return Ok(budget.clone());
            }
        }
        let budget = crate::agent_graph::AgentAutonomyBudget::new(limits, usage)
            .map_err(|_| AgentManagerError::GraphBudgetUnavailable)?;
        *configured = Some((plan_id, budget.clone()));
        Ok(budget)
    }

    pub fn graph_budget_snapshot(
        &self,
        plan_id: crate::work::PlanId,
    ) -> Option<crate::agent_graph::AgentGraphBudgetUsageV1> {
        self.graph_budget
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .as_ref()
            .filter(|(configured_plan, _)| *configured_plan == plan_id)
            .map(|(_, budget)| budget.snapshot())
    }

    fn current_graph_budget(&self) -> Option<crate::agent_graph::AgentAutonomyBudget> {
        self.graph_budget
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .as_ref()
            .map(|(_, budget)| budget.clone())
    }

    pub fn replace_delegation_router(&self, router: Option<crate::pricing::DelegationRouter>) {
        *self
            .delegation_router
            .write()
            .unwrap_or_else(|poison| poison.into_inner()) = router;
    }

    pub fn configure_learned_delegation(
        &self,
        enabled: bool,
        allowed: impl IntoIterator<Item = AgentRoute>,
    ) {
        self.learned_delegation
            .store(enabled, std::sync::atomic::Ordering::Release);
        *self
            .delegation_allowlist
            .write()
            .unwrap_or_else(|poison| poison.into_inner()) = allowed.into_iter().collect();
    }

    pub fn replace_parent_route(&self, route: Option<AgentRoute>) {
        *self
            .parent_route
            .write()
            .unwrap_or_else(|poison| poison.into_inner()) = route;
    }

    pub fn replace_default_delegation_route(&self, route: Option<AgentRoute>) {
        *self
            .default_delegation_route
            .write()
            .unwrap_or_else(|poison| poison.into_inner()) = route;
    }

    fn parent_route(&self) -> Option<AgentRoute> {
        self.parent_route
            .read()
            .unwrap_or_else(|poison| poison.into_inner())
            .clone()
    }

    pub fn default_delegation_route(&self) -> Option<AgentRoute> {
        self.default_delegation_route
            .read()
            .unwrap_or_else(|poison| poison.into_inner())
            .clone()
            .or_else(|| self.parent_route())
    }

    pub fn default_delegation_route_for_turn(&self, turn: Uuid) -> Option<AgentRoute> {
        self.frozen_parent_routes
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .get(&turn)
            .cloned()
            .unwrap_or_else(|| self.default_delegation_route())
    }

    /// Freeze the actual provider request route, honoring only a configured
    /// fixed delegation route over it. Later UI selection cannot retarget it.
    /// Fallback/automatic replacement may retarget a turn while no children
    /// are running under the previous freeze.
    pub fn freeze_parent_request_route(
        &self,
        turn: Uuid,
        actual: AgentRoute,
    ) -> Result<(), String> {
        let route = self
            .default_delegation_route
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
            .unwrap_or(actual);
        let mut routes = self
            .frozen_parent_routes
            .write()
            .unwrap_or_else(|p| p.into_inner());
        match routes.get(&turn).and_then(|previous| previous.as_ref()) {
            Some(previous) if previous != &route => {
                if self.has_active_children_for_turn(turn) {
                    return Err("child route is frozen for this parent request".into());
                }
                routes.insert(turn, Some(route));
            }
            Some(_) => {}
            None => {
                routes.insert(turn, Some(route));
            }
        }
        Ok(())
    }

    /// Allow fallback/replacement to retarget a parent turn's frozen route
    /// while no children are running under the previous freeze.
    pub fn replace_parent_request_route(
        &self,
        turn: Uuid,
        actual: AgentRoute,
    ) -> Result<(), String> {
        if self.has_active_children_for_turn(turn) {
            return Err("child route is frozen for this parent request".into());
        }
        self.frozen_parent_routes
            .write()
            .unwrap_or_else(|p| p.into_inner())
            .insert(turn, Some(actual));
        Ok(())
    }

    fn has_active_children_for_turn(&self, turn: Uuid) -> bool {
        self.activity_tx
            .borrow()
            .iter()
            .any(|child| child.parent_turn == turn && !child.status.terminal())
    }

    pub fn has_parent_constraints(&self, turn: Uuid) -> bool {
        self.parent_constraints
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .contains_key(&turn)
    }

    pub(crate) fn frozen_parent_policy(&self, turn: Uuid) -> Option<Arc<FrozenChildPolicy>> {
        self.frozen_parent_policies
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .get(&turn)
            .cloned()
    }

    pub(crate) fn current_permission_policy(&self) -> Option<Arc<FrozenChildPolicy>> {
        self.current_permission_policy
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    pub(crate) fn refresh_permission_policy(&self, policy: FrozenChildPolicy) {
        *self
            .current_permission_policy
            .write()
            .unwrap_or_else(|p| p.into_inner()) = Some(Arc::new(policy));
    }

    pub(crate) fn set_parent_permission_policy(
        &self,
        turn: Uuid,
        policy: FrozenChildPolicy,
    ) -> Result<(), String> {
        let mut snapshots = self
            .frozen_parent_policies
            .write()
            .unwrap_or_else(|p| p.into_inner());
        if let Some(previous) = snapshots.get(&turn) {
            return if previous.as_ref() == &policy {
                Ok(())
            } else {
                Err("child permission policy is frozen for this parent turn".into())
            };
        }
        if snapshots.len() >= 128 {
            let current = self.current_parent_turn();
            let active = self
                .activity_tx
                .borrow()
                .iter()
                .filter(|child| !child.status.terminal())
                .map(|child| child.parent_turn)
                .collect::<HashSet<_>>();
            snapshots.retain(|previous, _| *previous == current || active.contains(previous));
            if snapshots.len() >= 128 {
                return Err("active child permission snapshot limit reached".into());
            }
        }
        snapshots.insert(turn, Arc::new(policy));
        Ok(())
    }

    pub fn enforce_headless_read_only(&self) {
        self.headless_noninteractive
            .store(true, std::sync::atomic::Ordering::Release);
        self.headless_read_only
            .store(true, std::sync::atomic::Ordering::Release);
    }

    /// Ordinary headless workers retain the selected parent tool policy and
    /// explicit path/command grants. Approval requiring a UI still fails.
    pub fn allow_headless_granted_workers(&self) {
        self.headless_noninteractive
            .store(true, std::sync::atomic::Ordering::Release);
        self.headless_read_only
            .store(false, std::sync::atomic::Ordering::Release);
    }

    pub fn set_parent_constraints(&self, turn: Uuid, text: &str) -> Result<(), String> {
        if text.len() > 64 * 1024 {
            return Err("inherited child constraints exceed 64 KiB".into());
        }
        let mut snapshots = self
            .parent_constraints
            .write()
            .unwrap_or_else(|p| p.into_inner());
        if let Some(previous) = snapshots.get(&turn) {
            return if previous == text {
                Ok(())
            } else {
                Err("child constraints are frozen for this parent turn".into())
            };
        }
        if snapshots.len() >= 128 {
            let current = self.current_parent_turn();
            let active_turns = self
                .activity_tx
                .borrow()
                .iter()
                .filter(|child| !child.status.terminal())
                .map(|child| child.parent_turn)
                .collect::<HashSet<_>>();
            snapshots.retain(|previous, _| *previous == current || active_turns.contains(previous));
            self.frozen_parent_routes
                .write()
                .unwrap_or_else(|p| p.into_inner())
                .retain(|previous, _| *previous == turn || snapshots.contains_key(previous));
            if snapshots.len() >= 128 {
                return Err("active child constraint snapshot limit reached".into());
            }
        }
        snapshots.insert(turn, text.into());
        // Do not treat a missing default delegation route as a freeze; a later
        // freeze_parent_request_route with the actual route must succeed.
        if let Some(route) = self.default_delegation_route() {
            self.frozen_parent_routes
                .write()
                .unwrap_or_else(|p| p.into_inner())
                .entry(turn)
                .or_insert(Some(route));
        }
        Ok(())
    }

    pub async fn snapshot_mutations_for_turn(
        &self,
        turn: Uuid,
    ) -> Result<Vec<AgentMutationReceipt>, String> {
        let inner = self.inner.lock().await;
        if inner.incomplete_mutation_turns.contains(&turn) {
            return Err("child mutation evidence exceeded its bounded retention".into());
        }
        Ok(inner
            .mutations
            .iter()
            .filter(|receipt| receipt.parent_turn == turn)
            .cloned()
            .collect())
    }

    pub async fn take_mutations_for_turn(
        &self,
        turn: Uuid,
    ) -> Result<Vec<AgentMutationReceipt>, String> {
        let mut inner = self.inner.lock().await;
        if inner.incomplete_mutation_turns.contains(&turn) {
            return Err("child mutation evidence exceeded its bounded retention".into());
        }
        let mut matching = Vec::new();
        let mut retained = Vec::new();
        for receipt in inner.mutations.drain(..) {
            if receipt.parent_turn == turn {
                matching.push(receipt);
            } else {
                retained.push(receipt);
            }
        }
        inner.mutations = retained;
        Ok(matching)
    }

    pub async fn set_role(&self, id: AgentId, role: &str) -> Result<(), AgentManagerError> {
        let mut inner = self.inner.lock().await;
        let record = inner
            .records
            .get_mut(&id)
            .ok_or(AgentManagerError::Unknown(id))?;
        record.snapshot.role = Some(safe_activity_text(role, 64));
        publish_activity(&inner, &self.activity_tx);
        Ok(())
    }

    fn is_headless_read_only(&self) -> bool {
        self.headless_read_only
            .load(std::sync::atomic::Ordering::Acquire)
    }

    pub fn set_worktree_store(&self, store: Option<crate::worktree::WorktreeStore>) {
        *self
            .worktree_store
            .write()
            .unwrap_or_else(|poison| poison.into_inner()) = store;
    }

    pub fn set_spawns_paused(&self, paused: bool) {
        *self
            .spawns_paused
            .write()
            .unwrap_or_else(|poison| poison.into_inner()) = paused;
    }

    pub fn spawns_paused(&self) -> bool {
        *self
            .spawns_paused
            .read()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    pub async fn active_count(&self) -> usize {
        self.inner
            .lock()
            .await
            .records
            .values()
            .filter(|record| !record.snapshot.status.terminal())
            .count()
    }

    fn select_delegation_route(
        &self,
        complexity: crate::pricing::Complexity,
        task: &str,
        context_summary: &str,
    ) -> Result<crate::pricing::RouteSelection, String> {
        let allowlist = self
            .delegation_allowlist
            .read()
            .unwrap_or_else(|poison| poison.into_inner());
        let restrict_routes = self.learned_delegation_enabled() && !allowlist.is_empty();
        self.delegation_router
            .read()
            .unwrap_or_else(|poison| poison.into_inner())
            .as_ref()
            .ok_or_else(|| "automatic delegation routing is unavailable".to_string())?
            .select_matching(complexity, task, context_summary, |provider, model| {
                !restrict_routes
                    || allowlist
                        .iter()
                        .any(|route| route.provider == provider && route.model == model)
            })
    }

    fn learned_delegation_enabled(&self) -> bool {
        self.learned_delegation
            .load(std::sync::atomic::Ordering::Acquire)
    }

    fn parent_turn(&self) -> Uuid {
        self.current_parent_turn()
    }

    pub async fn spawn(
        self: &Arc<Self>,
        request: AgentSpawnRequest,
    ) -> Result<AgentId, AgentManagerError> {
        self.spawn_with_origin(request, AgentRunOrigin::Conversation)
            .await
    }

    pub async fn spawn_with_origin(
        self: &Arc<Self>,
        request: AgentSpawnRequest,
        origin: AgentRunOrigin,
    ) -> Result<AgentId, AgentManagerError> {
        self.spawn_with_inherited_policy(request, origin, None)
            .await
    }

    async fn spawn_with_inherited_policy(
        self: &Arc<Self>,
        mut request: AgentSpawnRequest,
        origin: AgentRunOrigin,
        inherited_policy: Option<Arc<FrozenChildPolicy>>,
    ) -> Result<AgentId, AgentManagerError> {
        if self.spawns_paused() {
            return Err(AgentManagerError::SpawnsPaused);
        }
        if let Some(constraints) = self
            .parent_constraints
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .get(&request.parent_turn)
        {
            if !constraints.is_empty() {
                request.context_summary.push_str(
                    "\n\nInherited human and project instructions (frozen for this turn):\n",
                );
                request.context_summary.push_str(constraints);
            }
        }
        if request.route.is_none() && request.purpose == AgentRunPurpose::Subagent {
            request.route = self.default_delegation_route_for_turn(request.parent_turn);
        }
        self.validate_spawn_request(&request)?;
        let graph_budget = if origin == AgentRunOrigin::TaskGraph {
            let budget = self
                .current_graph_budget()
                .ok_or(AgentManagerError::GraphBudgetUnavailable)?;
            budget
                .reserve_child_spawn()
                .map_err(AgentManagerError::GraphBudget)?;
            Some(budget)
        } else {
            None
        };
        let outcome_parent = self.outcome_parent_for(request.parent_turn);
        let id = AgentId::new();
        let (request, worktree) = self.prepare_isolation(id, request).await?;
        let job_owner = crate::jobs::JobOwner::new(None);
        let (command_tx, command_rx) = mpsc::channel(32);
        let (cancel_tx, cancel_rx) = watch::channel(false);
        let notify = Arc::new(Notify::new());
        let rollback_worktree = worktree.clone();
        let mut frozen_policy = None;
        let admission = {
            let mut inner = self.inner.lock().await;
            let paused = self
                .spawns_paused
                .read()
                .unwrap_or_else(|poison| poison.into_inner());
            let count = inner
                .records
                .values()
                .filter(|record| {
                    record.parent_turn == request.parent_turn
                        && (record.snapshot.origin == AgentRunOrigin::AutomaticReview)
                            == (origin == AgentRunOrigin::AutomaticReview)
                })
                .count();
            let limit = if origin == AgentRunOrigin::AutomaticReview {
                MAX_AUTOMATIC_REVIEWS_PER_TURN
            } else {
                self.limits.max_children_per_turn
            };
            if *paused {
                Err(AgentManagerError::SpawnsPaused)
            } else if count >= limit {
                Err(AgentManagerError::ChildrenPerTurn(limit))
            } else {
                frozen_policy = inherited_policy
                    .clone()
                    .or_else(|| self.frozen_parent_policy(request.parent_turn));
                insert_agent_record(
                    &mut inner,
                    id,
                    origin,
                    &request,
                    worktree,
                    job_owner,
                    command_tx,
                    cancel_tx,
                    notify.clone(),
                    graph_budget.clone(),
                    outcome_parent.as_ref(),
                );
                publish_activity(&inner, &self.activity_tx);
                Ok(())
            }
        };
        if let Err(error) = admission {
            self.rollback_prepared_worktree(rollback_worktree.as_ref())
                .await;
            return Err(error);
        }
        let lifecycle_hooks = self.lifecycle_hooks_for(id, request.parent_turn);
        let recovery_context = self.recovery_context_for(&request, id);
        let manager = self.clone();
        tokio::spawn(async move {
            manager
                .run_record(
                    id,
                    request,
                    command_rx,
                    cancel_rx,
                    notify,
                    job_owner,
                    graph_budget,
                    lifecycle_hooks,
                    recovery_context,
                    frozen_policy,
                )
                .await;
        });
        Ok(id)
    }

    pub async fn spawn_batch(
        self: &Arc<Self>,
        mut requests: Vec<AgentSpawnRequest>,
        origin: AgentRunOrigin,
    ) -> Result<(AgentBatchId, Vec<AgentId>), AgentManagerError> {
        if self.spawns_paused() {
            return Err(AgentManagerError::SpawnsPaused);
        }
        if requests.len() < 2 || requests.len() > MAX_BATCH_SIZE {
            return Err(AgentManagerError::InvalidBatch(format!(
                "batch size must be from 2 to {MAX_BATCH_SIZE}"
            )));
        }
        let parent_turn = requests[0].parent_turn;
        if requests
            .iter()
            .any(|request| request.parent_turn != parent_turn)
        {
            return Err(AgentManagerError::InvalidBatch(
                "all batch tasks must share one parent turn".into(),
            ));
        }
        for request in &mut requests {
            if let Some(constraints) = self
                .parent_constraints
                .read()
                .unwrap_or_else(|p| p.into_inner())
                .get(&request.parent_turn)
            {
                if !constraints.is_empty() {
                    request.context_summary.push_str(
                        "\n\nInherited human and project instructions (frozen for this turn):\n",
                    );
                    request.context_summary.push_str(constraints);
                }
            }
            if request.route.is_none() && request.purpose == AgentRunPurpose::Subagent {
                request.route = self.default_delegation_route_for_turn(request.parent_turn);
            }
            self.validate_spawn_request(request)?;
        }
        let graph_budget = if origin == AgentRunOrigin::TaskGraph {
            let budget = self
                .current_graph_budget()
                .ok_or(AgentManagerError::GraphBudgetUnavailable)?;
            for _ in &requests {
                budget
                    .reserve_child_spawn()
                    .map_err(AgentManagerError::GraphBudget)?;
            }
            Some(budget)
        } else {
            None
        };
        let outcome_parent = self.outcome_parent_for(parent_turn);
        let mut prepared = Vec::with_capacity(requests.len());
        for request in requests {
            let id = AgentId::new();
            match self.prepare_isolation(id, request).await {
                Ok((request, worktree)) => prepared.push((id, request, worktree)),
                Err(error) => {
                    for (_, _, worktree) in &prepared {
                        self.rollback_prepared_worktree(worktree.as_ref()).await;
                    }
                    return Err(error);
                }
            }
        }
        let batch_id = AgentBatchId::new();
        let ids = prepared.iter().map(|(id, _, _)| *id).collect::<Vec<_>>();
        let mut launches = Vec::with_capacity(prepared.len());
        let mut prepared = Some(prepared);
        let admission = {
            let mut inner = self.inner.lock().await;
            let paused = self
                .spawns_paused
                .read()
                .unwrap_or_else(|poison| poison.into_inner());
            let used = inner
                .records
                .values()
                .filter(|record| {
                    record.parent_turn == parent_turn
                        && (record.snapshot.origin == AgentRunOrigin::AutomaticReview)
                            == (origin == AgentRunOrigin::AutomaticReview)
                })
                .count();
            let limit = if origin == AgentRunOrigin::AutomaticReview {
                MAX_AUTOMATIC_REVIEWS_PER_TURN
            } else {
                self.limits.max_children_per_turn
            };
            if *paused {
                Err(AgentManagerError::SpawnsPaused)
            } else if used.saturating_add(ids.len()) > limit {
                Err(AgentManagerError::ChildrenPerTurn(limit))
            } else {
                for (id, request, worktree) in prepared.take().unwrap_or_default() {
                    let (command_tx, command_rx) = mpsc::channel(32);
                    let (cancel_tx, cancel_rx) = watch::channel(false);
                    let notify = Arc::new(Notify::new());
                    let job_owner = crate::jobs::JobOwner::new(None);
                    insert_agent_record(
                        &mut inner,
                        id,
                        origin,
                        &request,
                        worktree,
                        job_owner,
                        command_tx,
                        cancel_tx,
                        notify.clone(),
                        graph_budget.clone(),
                        outcome_parent.as_ref(),
                    );
                    let frozen_policy = self.frozen_parent_policy(request.parent_turn);
                    launches.push((
                        id,
                        request,
                        command_rx,
                        cancel_rx,
                        notify,
                        job_owner,
                        frozen_policy,
                    ));
                }
                inner.batches.insert(batch_id, ids.clone());
                publish_activity(&inner, &self.activity_tx);
                Ok(())
            }
        };
        if let Err(error) = admission {
            for (_, _, worktree) in prepared.as_deref().unwrap_or_default() {
                self.rollback_prepared_worktree(worktree.as_ref()).await;
            }
            return Err(error);
        }
        for (id, request, command_rx, cancel_rx, notify, job_owner, frozen_policy) in launches {
            let lifecycle_hooks = self.lifecycle_hooks_for(id, request.parent_turn);
            let recovery_context = self.recovery_context_for(&request, id);
            let manager = self.clone();
            let record_graph_budget = graph_budget.clone();
            tokio::spawn(async move {
                manager
                    .run_record(
                        id,
                        request,
                        command_rx,
                        cancel_rx,
                        notify,
                        job_owner,
                        record_graph_budget,
                        lifecycle_hooks,
                        recovery_context,
                        frozen_policy,
                    )
                    .await;
            });
        }
        Ok((batch_id, ids))
    }

    pub async fn await_batch_bounded(
        &self,
        id: AgentBatchId,
        timeout_secs: Option<u64>,
    ) -> Result<AgentBatchSnapshot, AgentManagerError> {
        if timeout_secs.is_some_and(|seconds| seconds > MAX_AWAIT_SECS) {
            return Err(AgentManagerError::InvalidWait(format!(
                "await timeout exceeds {MAX_AWAIT_SECS} seconds"
            )));
        }
        let ids = self
            .inner
            .lock()
            .await
            .batches
            .get(&id)
            .cloned()
            .ok_or(AgentManagerError::UnknownBatch(id))?;
        let collect = async {
            let mut agents = Vec::with_capacity(ids.len());
            for agent_id in &ids {
                agents.push(self.await_agent(*agent_id).await?);
            }
            Ok::<_, AgentManagerError>(agents)
        };
        let agents = match timeout_secs {
            None => collect.await?,
            Some(0) => {
                let mut agents = Vec::with_capacity(ids.len());
                for agent_id in &ids {
                    agents.push(self.snapshot(*agent_id).await?);
                }
                agents
            }
            Some(seconds) => {
                match tokio::time::timeout(std::time::Duration::from_secs(seconds), collect).await {
                    Ok(result) => result?,
                    Err(_) => {
                        let mut agents = Vec::with_capacity(ids.len());
                        for agent_id in &ids {
                            agents.push(self.snapshot(*agent_id).await?);
                        }
                        agents
                    }
                }
            }
        };
        let terminal = agents.iter().all(|agent| agent.status.terminal());
        Ok(AgentBatchSnapshot {
            id,
            agents,
            terminal,
        })
    }

    fn validate_spawn_request(&self, request: &AgentSpawnRequest) -> Result<(), AgentManagerError> {
        let available = self
            .available_subagent_tools()?
            .into_iter()
            .map(|definition| definition.function.name)
            .collect::<HashSet<_>>();
        if let Some(tool) = request
            .grants
            .tools
            .iter()
            .find(|tool| !available.contains(tool.as_str()))
        {
            return Err(AgentManagerError::InvalidToolGrant(tool.clone()));
        }
        if request.depth > self.limits.max_depth {
            return Err(AgentManagerError::Depth {
                depth: request.depth,
                limit: self.limits.max_depth,
            });
        }
        Ok(())
    }

    async fn prepare_isolation(
        &self,
        id: AgentId,
        mut request: AgentSpawnRequest,
    ) -> Result<(AgentSpawnRequest, Option<AgentWorktreeReceipt>), AgentManagerError> {
        if request.isolation == AgentIsolation::Shared {
            return Ok((request, None));
        }
        let store = self
            .worktree_store
            .read()
            .unwrap_or_else(|poison| poison.into_inner())
            .clone()
            .ok_or_else(|| AgentManagerError::Worktree("store is not configured".into()))?;
        let source = request
            .workspace
            .roots()
            .iter()
            .find(|root| root.primary)
            .map(|root| root.path.clone())
            .ok_or_else(|| {
                AgentManagerError::Worktree("primary workspace root is missing".into())
            })?;
        let label = format!("agent-{}", &id.0.simple().to_string()[..8]);
        let record = store
            .create(&source, &label, None)
            .await
            .map_err(|error| AgentManagerError::Worktree(error.to_string()))?;
        let (_, workspace) = store
            .attach_context(&record.id.to_string())
            .await
            .map_err(|error| AgentManagerError::Worktree(error.to_string()))?;
        request.workspace = workspace;
        request.grants.workspace_root_ids = request
            .workspace
            .roots()
            .iter()
            .map(|root| root.id.to_string())
            .collect();
        request.grants.path_scopes = request
            .workspace
            .roots()
            .iter()
            .map(|root| root.path.to_string_lossy().into_owned())
            .collect();
        Ok((
            request,
            Some(AgentWorktreeReceipt {
                id: record.id,
                path: record.path,
                branch: record.branch,
                base_commit: record.base_commit,
                dirty: None,
                changed_entries: None,
            }),
        ))
    }

    async fn rollback_prepared_worktree(&self, receipt: Option<&AgentWorktreeReceipt>) {
        let Some(receipt) = receipt else {
            return;
        };
        let store = self
            .worktree_store
            .read()
            .unwrap_or_else(|poison| poison.into_inner())
            .clone();
        if let Some(store) = store {
            let _ = store.cleanup(&receipt.id.to_string(), None).await;
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_record(
        self: Arc<Self>,
        id: AgentId,
        mut request: AgentSpawnRequest,
        commands: mpsc::Receiver<AgentCommand>,
        cancelled: watch::Receiver<bool>,
        notify: Arc<Notify>,
        job_owner: crate::jobs::JobOwner,
        graph_budget: Option<crate::agent_graph::AgentAutonomyBudget>,
        lifecycle_hooks: Option<LifecycleHooks>,
        mut recovery_context: Option<crate::recovery::RecoveryContext>,
        frozen_policy: Option<Arc<FrozenChildPolicy>>,
    ) {
        let lifecycle_cwd = request.workspace.cwd().to_path_buf();
        let lifecycle_model = request.route.as_ref().map(|route| route.model.clone());
        let lifecycle_roots = request.workspace.execution_roots();
        if let Some(context) = recovery_context.take() {
            let store = context.store.clone();
            let roots = lifecycle_roots.clone();
            let activity =
                tokio::task::spawn_blocking(move || store.try_workspace_activity(&roots))
                    .await
                    .map_err(|_| "Workspace activity worker stopped".to_string())
                    .and_then(|result| result.map_err(|error| error.to_string()));
            match activity {
                Ok(activity) => recovery_context = Some(context.with_activity(activity)),
                Err(error) => {
                    drop(context);
                    let receipt = self
                        .snapshot(id)
                        .await
                        .ok()
                        .and_then(|snapshot| snapshot.worktree);
                    self.rollback_prepared_worktree(receipt.as_ref()).await;
                    self.finish(
                        id,
                        AgentStatus::Failed,
                        Some(format!("Workspace activity unavailable: {error}")),
                    )
                    .await;
                    cleanup_owned_jobs(job_owner).await;
                    if let Some(binding) = &lifecycle_hooks {
                        self.notify_child_stop(
                            id,
                            binding,
                            &lifecycle_cwd,
                            lifecycle_model.as_deref(),
                            &lifecycle_roots,
                        )
                        .await;
                    }
                    notify.notify_waiters();
                    return;
                }
            }
        }
        if let Some(binding) = &lifecycle_hooks {
            let payload = crate::hooks::HookPayload::lifecycle(
                crate::hooks::HookEvent::SubagentStart,
                &binding.owner,
                &lifecycle_cwd,
                lifecycle_model.as_deref(),
                "admitted",
                "queued",
            );
            let start = tokio::select! {
                biased;
                _ = wait_cancelled(cancelled.clone()) => None,
                result = binding.runner.run_event(&binding.owner, crate::hooks::HookEvent::SubagentStart, &payload, &lifecycle_roots) => Some(result),
            };
            match start {
                None => {
                    drop(recovery_context.take());
                    self.finish(id, AgentStatus::Cancelled, None).await;
                    cleanup_owned_jobs(job_owner).await;
                    self.notify_child_stop(
                        id,
                        binding,
                        &lifecycle_cwd,
                        lifecycle_model.as_deref(),
                        &lifecycle_roots,
                    )
                    .await;
                    notify.notify_waiters();
                    return;
                }
                Some(batch) if batch.denied.is_some() || batch.fail_closed => {
                    drop(recovery_context.take());
                    let receipt = self
                        .snapshot(id)
                        .await
                        .ok()
                        .and_then(|snapshot| snapshot.worktree);
                    self.rollback_prepared_worktree(receipt.as_ref()).await;
                    self.finish(
                        id,
                        AgentStatus::Failed,
                        Some("Subagent start hook denied execution".into()),
                    )
                    .await;
                    cleanup_owned_jobs(job_owner).await;
                    self.notify_child_stop(
                        id,
                        binding,
                        &lifecycle_cwd,
                        lifecycle_model.as_deref(),
                        &lifecycle_roots,
                    )
                    .await;
                    notify.notify_waiters();
                    return;
                }
                Some(_) => {
                    for context in binding.runner.take_contexts(&binding.owner) {
                        request
                            .context_summary
                            .push_str("\n\nSubagent start hook context:\n");
                        request.context_summary.push_str(&context);
                    }
                }
            }
        }
        let permit = tokio::select! {
            permit = self.semaphore.clone().acquire_owned() => permit.ok(),
            _ = wait_cancelled(cancelled.clone()) => None,
        };
        let Some(_permit) = permit else {
            drop(recovery_context.take());
            self.finish(id, AgentStatus::Cancelled, None).await;
            cleanup_owned_jobs(job_owner).await;
            if let Some(binding) = &lifecycle_hooks {
                self.notify_child_stop(
                    id,
                    binding,
                    &lifecycle_cwd,
                    lifecycle_model.as_deref(),
                    &lifecycle_roots,
                )
                .await;
            }
            notify.notify_waiters();
            return;
        };
        if *cancelled.borrow() {
            drop(_permit);
            drop(recovery_context.take());
            self.finish(id, AgentStatus::Cancelled, None).await;
            cleanup_owned_jobs(job_owner).await;
            if let Some(binding) = &lifecycle_hooks {
                self.notify_child_stop(
                    id,
                    binding,
                    &lifecycle_cwd,
                    lifecycle_model.as_deref(),
                    &lifecycle_roots,
                )
                .await;
            }
            notify.notify_waiters();
            return;
        }
        self.set_status(id, AgentStatus::Running, "started").await;
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let manager_cancelled = cancelled.clone();
        let context = AgentRunContext {
            id,
            request,
            limits: self.limits.clone(),
            commands,
            pending_steers: VecDeque::new(),
            cancelled,
            events: event_tx,
            job_owner,
            graph_budget,
            frozen_policy,
        };
        let runner = self.runner.clone();
        let runtime = self.limits.runtime;
        let mut task = tokio::spawn(async move {
            if *context.cancelled.borrow() {
                return Err("cancelled before runner startup".into());
            }
            crate::owned_tools::with_recovery(recovery_context, runner.run(context)).await
        });
        let timeout = tokio::time::sleep(runtime);
        tokio::pin!(timeout);
        loop {
            tokio::select! {
                event = event_rx.recv() => {
                    if let Some(event) = event {
                        self.apply_event(id, event).await;
                    }
                }
                result = &mut task => {
                    while let Ok(event) = event_rx.try_recv() {
                        self.apply_event(id, event).await;
                    }
                    match result {
                        _ if *manager_cancelled.borrow() => self.finish(id, AgentStatus::Cancelled, None).await,
                        Ok(Ok(summary)) => self.finish(id, AgentStatus::Completed, Some(summary)).await,
                        Ok(Err(error)) => self.finish(id, AgentStatus::Failed, Some(error)).await,
                        Err(error) if error.is_cancelled() => self.finish(id, AgentStatus::Cancelled, None).await,
                        Err(error) => self.finish(id, AgentStatus::Failed, Some(error.to_string())).await,
                    }
                    break;
                }
                _ = &mut timeout => {
                    task.abort();
                    let _ = (&mut task).await;
                    while let Ok(event) = event_rx.try_recv() { self.apply_event(id, event).await; }
                    self.finish(id, AgentStatus::TimedOut, None).await;
                    break;
                }
                _ = wait_cancelled(manager_cancelled.clone()) => {
                    task.abort();
                    let _ = (&mut task).await;
                    while let Ok(event) = event_rx.try_recv() { self.apply_event(id, event).await; }
                    self.finish(id, AgentStatus::Cancelled, None).await;
                    break;
                }
            }
        }
        drop(_permit);
        cleanup_owned_jobs(job_owner).await;
        if let Some(binding) = &lifecycle_hooks {
            self.notify_child_stop(
                id,
                binding,
                &lifecycle_cwd,
                lifecycle_model.as_deref(),
                &lifecycle_roots,
            )
            .await;
        }
        notify.notify_waiters();
    }

    async fn notify_child_stop(
        &self,
        id: AgentId,
        binding: &LifecycleHooks,
        cwd: &std::path::Path,
        model: Option<&str>,
        roots: &[PathBuf],
    ) {
        let status = {
            let mut inner = self.inner.lock().await;
            let Some(record) = inner.records.get_mut(&id) else {
                return;
            };
            if !record.snapshot.status.terminal() || record.lifecycle_stop_claimed {
                return;
            }
            record.lifecycle_stop_claimed = true;
            record.snapshot.status.clone()
        };
        let status = match status {
            AgentStatus::Completed => "completed",
            AgentStatus::Failed => "failed",
            AgentStatus::Cancelled => "cancelled",
            AgentStatus::TimedOut => "timed_out",
            AgentStatus::Interrupted => "interrupted",
            _ => unreachable!("terminal status checked"),
        };
        let payload = crate::hooks::HookPayload::lifecycle(
            crate::hooks::HookEvent::SubagentStop,
            &binding.owner,
            cwd,
            model,
            "terminal",
            status,
        );
        let outcome = binding
            .runner
            .run_event(
                &binding.owner,
                crate::hooks::HookEvent::SubagentStop,
                &payload,
                roots,
            )
            .await;
        binding.runner.discard_owner(&binding.owner);
        if outcome.denied.is_some() || outcome.fail_closed {
            let mut inner = self.inner.lock().await;
            if let Some(record) = inner.records.get_mut(&id) {
                push_event(
                    record,
                    "subagent stop hook reported failure; terminal outcome retained".into(),
                );
                publish_activity(&inner, &self.activity_tx);
            }
        }
    }

    async fn apply_event(&self, id: AgentId, event: RunnerEvent) {
        let mut inner = self.inner.lock().await;
        if matches!(
            &event,
            RunnerEvent::Mutation(_) | RunnerEvent::AggregateMutation { .. }
        ) {
            let aggregate = matches!(&event, RunnerEvent::AggregateMutation { .. });
            let (mutation, aggregate_started_at) = match event {
                RunnerEvent::Mutation(mutation) => (mutation, None),
                RunnerEvent::AggregateMutation { batch, started_at } => (batch, started_at),
                _ => unreachable!(),
            };
            if let Some(turn) = inner.records.get(&id).map(|record| record.parent_turn) {
                let retained_bytes: usize = inner
                    .mutations
                    .iter()
                    .flat_map(|receipt| receipt.mutation.receipts())
                    .map(|receipt| {
                        [&receipt.before, &receipt.after]
                            .iter()
                            .map(|state| match state {
                                crate::undo::FileState::Missing => 0,
                                crate::undo::FileState::Utf8(text) => text.len(),
                            })
                            .sum::<usize>()
                    })
                    .sum();
                let incoming_bytes: usize = mutation
                    .receipts()
                    .iter()
                    .map(|receipt| {
                        [&receipt.before, &receipt.after]
                            .iter()
                            .map(|state| match state {
                                crate::undo::FileState::Missing => 0,
                                crate::undo::FileState::Utf8(text) => text.len(),
                            })
                            .sum::<usize>()
                    })
                    .sum();
                if inner.mutations.len() >= 256
                    || retained_bytes.saturating_add(incoming_bytes) > 16 * 1024 * 1024
                {
                    inner.incomplete_mutation_turns.insert(turn);
                } else {
                    inner.mutations.push(AgentMutationReceipt {
                        agent_id: id,
                        parent_turn: turn,
                        mutation,
                        aggregate,
                        aggregate_started_at,
                    });
                }
            }
            return;
        }
        let Some(record) = inner.records.get_mut(&id) else {
            return;
        };
        match event {
            RunnerEvent::Mutation(_) | RunnerEvent::AggregateMutation { .. } => unreachable!(),
            RunnerEvent::Phase(phase) => record.snapshot.phase = phase,
            RunnerEvent::HarnessPrepared {
                snapshot,
                model,
                provider,
            } => {
                record.snapshot.model = Some(model.clone());
                record.snapshot.provider = Some(provider.clone());
                if let Some(outcome) = &mut record.outcome {
                    outcome.set_model(Some(model), Some(provider));
                    outcome.set_runtime_harness(snapshot);
                }
            }
            RunnerEvent::ToolEvidence {
                turn,
                call_id,
                name,
                succeeded,
                denied,
                output,
            } => {
                if let Some(outcome) = &mut record.outcome {
                    if outcome.record_tool(turn, &call_id, &name, succeeded, denied) {
                        outcome.event(
                            "tool_evidence",
                            Some(&name),
                            Some(&call_id),
                            Some(succeeded && !denied),
                            &output,
                        );
                    }
                }
            }
            RunnerEvent::Activity(message) => push_event(record, message),
            RunnerEvent::Output(output) => {
                push_event(record, output.clone());
                if record.full_output.len() < MAX_SPILL_BYTES {
                    let remaining = MAX_SPILL_BYTES.saturating_sub(record.full_output.len());
                    record.spill_truncated |= output.len() > remaining;
                    record
                        .full_output
                        .push_str(utf8_prefix_owned(&output, remaining));
                } else if !output.is_empty() {
                    record.spill_truncated = true;
                }
                record.snapshot.recent_output.push_str(&output);
                trim_tail(
                    &mut record.snapshot.recent_output,
                    self.limits.retained_output_bytes,
                );
            }
            RunnerEvent::ToolStarted { name } => {
                let name = safe_activity_text(&name, 128);
                record.snapshot.phase = AgentExecutionPhase::Tool;
                record.snapshot.current_action = Some(name.clone());
                push_event(record, format!("started {name}"));
            }
            RunnerEvent::ToolCompleted { name, succeeded } => {
                if succeeded && !record.snapshot.successful_tools.contains(&name) {
                    record.snapshot.successful_tools.push(name.clone());
                    record.snapshot.successful_tools.sort();
                }
                record.snapshot.current_action = None;
                push_event(
                    record,
                    format!("{} {name}", if succeeded { "completed" } else { "failed" }),
                );
            }
            RunnerEvent::ApprovalRequired(message) => {
                record.snapshot.status = AgentStatus::WaitingApproval;
                record.snapshot.phase = AgentExecutionPhase::WaitingApproval;
                push_event(record, message);
            }
            RunnerEvent::SteerDelivered { id } => {
                if let Some(index) = record
                    .pending_steers
                    .iter()
                    .position(|(pending_id, _)| *pending_id == id)
                {
                    record.pending_steers.remove(index);
                }
                push_event(record, format!("steering {id} delivered"));
            }
        }
        record.snapshot.structured_repair_attempted |= record
            .snapshot
            .events
            .last()
            .is_some_and(|event| event.message.contains("structured output repair turn"));
        publish_activity(&inner, &self.activity_tx);
    }

    async fn set_status(&self, id: AgentId, status: AgentStatus, event: &str) {
        let mut inner = self.inner.lock().await;
        if let Some(record) = inner.records.get_mut(&id) {
            record.snapshot.status = status;
            if record.snapshot.status.terminal() {
                record.snapshot.phase = AgentExecutionPhase::Finished;
            }
            push_event(record, event.into());
            publish_activity(&inner, &self.activity_tx);
        }
    }

    async fn finish(&self, id: AgentId, status: AgentStatus, summary: Option<String>) {
        // A terminal activity row is also a writer barrier: owned process
        // groups must be reaped and their final bytes captured first.
        let owner = self
            .inner
            .lock()
            .await
            .records
            .get(&id)
            .map(|record| (record.job_owner, record.parent_turn));
        if let Some((owner, turn)) = owner {
            let started_at = crate::command_mutations::started_at(owner);
            cleanup_owned_jobs(owner).await;
            match crate::command_mutations::finish(owner).await {
                Ok(batches) => {
                    for batch in batches {
                        self.apply_event(id, RunnerEvent::AggregateMutation { batch, started_at })
                            .await;
                    }
                }
                Err(_) => {
                    self.inner
                        .lock()
                        .await
                        .incomplete_mutation_turns
                        .insert(turn);
                }
            }
        }
        let mut inner = self.inner.lock().await;
        let Some(record) = inner.records.get_mut(&id) else {
            return;
        };
        if record.snapshot.status.terminal() {
            return;
        }
        let mut status = status;
        let mut full_summary = summary
            .map(|value| crate::terminal_text::sanitize_terminal_text(&value))
            .unwrap_or_default();
        if status == AgentStatus::TimedOut && full_summary.is_empty() {
            full_summary = render_timeout_diagnostic(&record.snapshot);
        }
        if status == AgentStatus::Completed {
            if let Some(contract) = &record.output_contract {
                match serde_json::from_str::<serde_json::Value>(&full_summary)
                    .map_err(|error| format!("structured output result is invalid JSON: {error}"))
                    .and_then(|value| {
                        contract.validate_value(&value)?;
                        Ok(value)
                    }) {
                    Ok(value) => record.snapshot.structured_result = Some(value),
                    Err(error) => {
                        status = AgentStatus::Failed;
                        full_summary = format!(
                            "structured output failed terminal validation: {}",
                            crate::terminal_text::sanitize_terminal_text(&error)
                        );
                    }
                }
            }
        }
        if let Some(outcome) = &mut record.outcome {
            match status {
                AgentStatus::Completed => outcome.record_assistant_turn(),
                AgentStatus::Cancelled | AgentStatus::TimedOut => outcome.record_interrupt(),
                _ => outcome.record_failure(),
            }
        }
        if !full_summary.is_empty() && record.full_output.len() < MAX_SPILL_BYTES {
            let remaining = MAX_SPILL_BYTES.saturating_sub(record.full_output.len());
            if !record.full_output.is_empty() && remaining > 0 {
                record.full_output.push('\n');
            }
            let remaining = MAX_SPILL_BYTES.saturating_sub(record.full_output.len());
            record.spill_truncated |= full_summary.len() > remaining;
            record
                .full_output
                .push_str(utf8_prefix_owned(&full_summary, remaining));
        } else if !full_summary.is_empty() {
            record.spill_truncated = true;
        }
        let spill_payload = (record.full_output.len() > self.limits.retained_output_bytes
            || full_summary.len() > self.limits.retained_output_bytes)
            .then(|| record.full_output.clone());
        let spill_was_truncated = record.spill_truncated;
        let job_owner = record.job_owner;
        let parent_turn = record.parent_turn;
        let worktree_id = record
            .snapshot
            .worktree
            .as_ref()
            .map(|worktree| worktree.id);
        record.snapshot.status = status;
        record.snapshot.phase = AgentExecutionPhase::Finished;
        record.snapshot.result_summary = (!full_summary.is_empty()).then(|| {
            let mut value = full_summary;
            trim_tail(&mut value, self.limits.retained_output_bytes);
            value
        });
        record.snapshot.result_sha256 = record.snapshot.result_summary.as_ref().map(|value| {
            format!(
                "{:x}",
                <sha2::Sha256 as sha2::Digest>::digest(value.as_bytes())
            )
        });
        record.snapshot.missed_steer = record
            .pending_steers
            .drain(..)
            .map(|(_, mut text)| {
                text = crate::terminal_text::sanitize_terminal_text(&text);
                trim_tail(&mut text, MAX_PENDING_STEER_BYTES);
                text
            })
            .collect();
        record.snapshot.updated_at = Utc::now();
        record.snapshot.current_action = None;
        if !record.snapshot.missed_steer.is_empty() {
            push_event(
                record,
                format!(
                    "{} steering message(s) missed the final provider boundary",
                    record.snapshot.missed_steer.len()
                ),
            );
        }
        push_event(record, "finished".into());
        publish_activity(&inner, &self.activity_tx);
        drop(inner);

        let jobs = crate::jobs::JobManager::process()
            .list_owned(job_owner)
            .await
            .into_iter()
            .filter(|job| job.background)
            .map(|job| AgentJobAttribution {
                job_id: job.id.to_string(),
                parent_turn,
                background: true,
                status: job.status.label(),
            })
            .collect::<Vec<_>>();
        let spill = if let Some(payload) = spill_payload {
            let artifact_root = self.artifact_root.clone();
            tokio::task::spawn_blocking(move || {
                write_agent_spill(&artifact_root, id, &payload, spill_was_truncated)
            })
            .await
            .map_err(|error| error.to_string())
            .and_then(|result| result)
            .map(Some)
        } else {
            Ok(None)
        };
        let worktree_status = if let Some(worktree_id) = worktree_id {
            let store = self
                .worktree_store
                .read()
                .unwrap_or_else(|poison| poison.into_inner())
                .clone();
            match store {
                Some(store) => store.status(&worktree_id.to_string()).await.ok(),
                None => None,
            }
        } else {
            None
        };
        let mut inner = self.inner.lock().await;
        if let Some(record) = inner.records.get_mut(&id) {
            record.snapshot.attributed_jobs = jobs;
            if record.snapshot.status == AgentStatus::TimedOut
                && !record.snapshot.attributed_jobs.is_empty()
            {
                let jobs = record
                    .snapshot
                    .attributed_jobs
                    .iter()
                    .map(|job| format!("{}:{}", job.job_id, job.status))
                    .collect::<Vec<_>>()
                    .join(",");
                let summary = record
                    .snapshot
                    .result_summary
                    .get_or_insert_with(String::new);
                if !summary.is_empty() {
                    summary.push('\n');
                }
                summary.push_str(&format!("attributed_background_jobs={jobs}"));
                trim_tail(summary, self.limits.retained_output_bytes);
                record.snapshot.result_sha256 = Some(format!(
                    "{:x}",
                    <sha2::Sha256 as sha2::Digest>::digest(summary.as_bytes())
                ));
            }
            match spill {
                Ok(spill) => record.snapshot.spill = spill,
                Err(error) => push_event(
                    record,
                    format!(
                        "spill artifact unavailable: {}",
                        safe_activity_text(&error, 512)
                    ),
                ),
            }
            if let (Some(receipt), Some(status)) =
                (record.snapshot.worktree.as_mut(), worktree_status)
            {
                receipt.dirty = Some(status.dirty);
                receipt.changed_entries = Some(status.changed_entries);
            }
            publish_activity(&inner, &self.activity_tx);
        }
    }

    pub async fn list(&self) -> Vec<AgentSnapshot> {
        let mut ids = self
            .inner
            .lock()
            .await
            .records
            .values()
            .map(|record| (record.snapshot.created_at, record.snapshot.id))
            .collect::<Vec<_>>();
        ids.sort_by_key(|(created_at, _)| *created_at);
        let mut snapshots = Vec::with_capacity(ids.len());
        for (_, id) in ids {
            if let Ok(snapshot) = self.snapshot(id).await {
                snapshots.push(snapshot);
            }
        }
        snapshots
    }

    pub fn summaries_for_session(&self) -> Vec<crate::work::AgentSummary> {
        let Ok(inner) = self.inner.try_lock() else {
            return Vec::new();
        };
        inner
            .records
            .values()
            .map(|record| crate::work::AgentSummary {
                id: crate::work::AgentSummaryId(record.snapshot.id.0),
                status: if record.snapshot.status.terminal() {
                    format!("{:?}", record.snapshot.status)
                } else {
                    "Interrupted".into()
                },
                summary: record
                    .snapshot
                    .result_summary
                    .clone()
                    .unwrap_or_else(|| record.snapshot.task.clone()),
                updated_at: record.snapshot.updated_at,
            })
            .collect()
    }

    pub async fn snapshot(&self, id: AgentId) -> Result<AgentSnapshot, AgentManagerError> {
        let (snapshot, job_owner, parent_turn) = {
            let inner = self.inner.lock().await;
            let record = inner
                .records
                .get(&id)
                .ok_or(AgentManagerError::Unknown(id))?;
            let mut snapshot = record.snapshot.clone();
            snapshot.graph_budget_usage =
                record.graph_budget.as_ref().map(|budget| budget.snapshot());
            (snapshot, record.job_owner, record.parent_turn)
        };
        if !snapshot.status.terminal() && snapshot.attributed_jobs.is_empty() {
            return Ok(snapshot);
        }
        let current = crate::jobs::JobManager::process()
            .list_owned(job_owner)
            .await
            .into_iter()
            .filter(|job| job.background)
            .map(|job| AgentJobAttribution {
                job_id: job.id.to_string(),
                parent_turn,
                background: true,
                status: job.status.label(),
            })
            .collect::<Vec<_>>();
        let mut inner = self.inner.lock().await;
        let record = inner
            .records
            .get_mut(&id)
            .ok_or(AgentManagerError::Unknown(id))?;
        merge_attributed_jobs(&mut record.snapshot.attributed_jobs, current);
        Ok(record.snapshot.clone())
    }

    pub async fn await_agent(&self, id: AgentId) -> Result<AgentSnapshot, AgentManagerError> {
        loop {
            let (snapshot, notify) = {
                let inner = self.inner.lock().await;
                let record = inner
                    .records
                    .get(&id)
                    .ok_or(AgentManagerError::Unknown(id))?;
                (record.snapshot.clone(), record.notify.clone())
            };
            if snapshot.status.terminal() {
                return Ok(snapshot);
            }
            notify.notified().await;
        }
    }

    pub async fn await_agent_bounded(
        &self,
        id: AgentId,
        timeout_secs: Option<u64>,
    ) -> Result<AgentSnapshot, AgentManagerError> {
        match timeout_secs {
            None => self.await_agent(id).await,
            Some(0) => self.snapshot(id).await,
            Some(seconds) if seconds <= MAX_AWAIT_SECS => {
                match tokio::time::timeout(
                    std::time::Duration::from_secs(seconds),
                    self.await_agent(id),
                )
                .await
                {
                    Ok(result) => result,
                    Err(_) => self.snapshot(id).await,
                }
            }
            Some(_) => Err(AgentManagerError::InvalidWait(format!(
                "await timeout exceeds {MAX_AWAIT_SECS} seconds"
            ))),
        }
    }

    pub async fn send_message(
        &self,
        id: AgentId,
        message: String,
    ) -> Result<Uuid, AgentManagerError> {
        if message.trim().is_empty() {
            return Err(AgentManagerError::InvalidSteer(
                "message must not be empty".into(),
            ));
        }
        if message.chars().count() > MAX_STEER_CHARS || message.len() > MAX_PENDING_STEER_BYTES {
            return Err(AgentManagerError::InvalidSteer(format!(
                "message exceeds {MAX_STEER_CHARS} characters"
            )));
        }
        let mut inner = self.inner.lock().await;
        let record = inner
            .records
            .get_mut(&id)
            .ok_or(AgentManagerError::Unknown(id))?;
        if record.snapshot.status.terminal() {
            return Err(AgentManagerError::AlreadyTerminal(id));
        }
        let pending_bytes = record
            .pending_steers
            .iter()
            .map(|(_, text)| text.len())
            .sum::<usize>();
        if record.pending_steers.len() >= MAX_PENDING_STEERS
            || pending_bytes.saturating_add(message.len()) > MAX_PENDING_STEER_BYTES
        {
            return Err(AgentManagerError::SteerQueueFull);
        }
        let steer_id = Uuid::new_v4();
        record
            .command_tx
            .try_send(AgentCommand::Message {
                id: steer_id,
                text: message.clone(),
            })
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => AgentManagerError::SteerQueueFull,
                mpsc::error::TrySendError::Closed(_) => AgentManagerError::ChannelClosed,
            })?;
        record.pending_steers.push((steer_id, message));
        push_event(record, format!("steering {steer_id} queued"));
        publish_activity(&inner, &self.activity_tx);
        Ok(steer_id)
    }

    pub async fn resolve_approval(
        &self,
        id: AgentId,
        approved: bool,
    ) -> Result<(), AgentManagerError> {
        let sender = {
            let inner = self.inner.lock().await;
            let record = inner
                .records
                .get(&id)
                .ok_or(AgentManagerError::Unknown(id))?;
            if record.snapshot.status != AgentStatus::WaitingApproval {
                return Err(AgentManagerError::NotWaitingApproval);
            }
            record.command_tx.clone()
        };
        self.set_status(id, AgentStatus::Running, "approval resolved")
            .await;
        sender
            .send(AgentCommand::Approval(approved))
            .await
            .map_err(|_| AgentManagerError::ChannelClosed)?;
        Ok(())
    }

    /// Ask every non-terminal child to stop. Does not wait for the children.
    pub async fn request_cancel_all(&self) {
        let senders: Vec<_> = self
            .inner
            .lock()
            .await
            .records
            .values()
            .filter(|record| !record.snapshot.status.terminal())
            .map(|record| record.cancel_tx.clone())
            .collect();
        for sender in senders {
            let _ = sender.send(true);
        }
    }

    pub async fn cancel(&self, id: AgentId) -> Result<(), AgentManagerError> {
        let sender = {
            let inner = self.inner.lock().await;
            inner
                .records
                .get(&id)
                .map(|record| record.cancel_tx.clone())
                .ok_or(AgentManagerError::Unknown(id))?
        };
        let _ = sender.send(true);
        Ok(())
    }

    pub async fn cancel_attributed_job(
        &self,
        id: AgentId,
        job_id: crate::jobs::JobId,
    ) -> Result<(), AgentManagerError> {
        let owner = {
            let inner = self.inner.lock().await;
            let record = inner
                .records
                .get(&id)
                .ok_or(AgentManagerError::Unknown(id))?;
            if !record
                .snapshot
                .attributed_jobs
                .iter()
                .any(|job| job.job_id == job_id.to_string())
            {
                return Err(AgentManagerError::InvalidBatch(
                    "job is not attributed to that child".into(),
                ));
            }
            record.job_owner
        };
        crate::jobs::JobManager::process()
            .kill_owned(job_id, owner)
            .await
            .map_err(AgentManagerError::InvalidBatch)
    }

    pub async fn await_attributed_job(
        &self,
        id: AgentId,
        job_id: crate::jobs::JobId,
    ) -> Result<crate::jobs::JobInfo, AgentManagerError> {
        let owner = {
            let inner = self.inner.lock().await;
            let record = inner
                .records
                .get(&id)
                .ok_or(AgentManagerError::Unknown(id))?;
            if !record
                .snapshot
                .attributed_jobs
                .iter()
                .any(|job| job.job_id == job_id.to_string())
            {
                return Err(AgentManagerError::InvalidBatch(
                    "job is not attributed to that child".into(),
                ));
            }
            record.job_owner
        };
        crate::jobs::JobManager::process()
            .await_job_owned(job_id, owner)
            .await
            .map_err(AgentManagerError::InvalidBatch)
    }

    pub async fn shutdown(&self) {
        let senders: Vec<_> = self
            .inner
            .lock()
            .await
            .records
            .values()
            .filter(|record| !record.snapshot.status.terminal())
            .map(|record| record.cancel_tx.clone())
            .collect();
        for sender in senders {
            let _ = sender.send(true);
        }
        let ids: Vec<_> = self.list().await.into_iter().map(|item| item.id).collect();
        for id in ids {
            let _ =
                tokio::time::timeout(std::time::Duration::from_secs(2), self.await_agent(id)).await;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn insert_agent_record(
    inner: &mut AgentManagerInner,
    id: AgentId,
    origin: AgentRunOrigin,
    request: &AgentSpawnRequest,
    worktree: Option<AgentWorktreeReceipt>,
    job_owner: crate::jobs::JobOwner,
    command_tx: mpsc::Sender<AgentCommand>,
    cancel_tx: watch::Sender<bool>,
    notify: Arc<Notify>,
    graph_budget: Option<crate::agent_graph::AgentAutonomyBudget>,
    outcome_parent: Option<&crate::session_outcome::OutcomeTracker>,
) {
    let parent = match request.parent {
        Some(parent) => inner
            .records
            .get(&parent)
            .and_then(|record| record.outcome.as_deref()),
        None => outcome_parent,
    };
    let outcome = parent.map(|parent| {
        let mut child = parent.child(id.0, &request.workspace.cwd().to_string_lossy());
        child.event(
            "delegated_task",
            None,
            None,
            None,
            "Delegated execution admitted",
        );
        crate::session_outcome::ScopedOutcome(child)
    });
    let now = Utc::now();
    inner.records.insert(
        id,
        AgentRecord {
            outcome,
            snapshot: AgentSnapshot {
                id,
                status: AgentStatus::Queued,
                origin,
                purpose: request.purpose,
                task_id: request.task_id,
                task: request.task.clone(),
                role: Some(
                    if origin == AgentRunOrigin::AutomaticReview {
                        "Reviewer"
                    } else {
                        "Worker"
                    }
                    .into(),
                ),
                model: request.route.as_ref().map(|route| route.model.clone()),
                provider: request.route.as_ref().map(|route| route.provider.clone()),
                phase: AgentExecutionPhase::Queued,
                parent: request.parent,
                depth: request.depth,
                context_mode: request.context_mode.clone(),
                grants: request.grants.clone(),
                created_at: now,
                updated_at: now,
                result_summary: None,
                result_sha256: None,
                structured_result: None,
                structured_repair_attempted: false,
                spill: None,
                worktree,
                attributed_jobs: Vec::new(),
                missed_steer: Vec::new(),
                recent_output: String::new(),
                successful_tools: Vec::new(),
                current_action: None,
                events: Vec::new(),
                graph_budget_usage: graph_budget.as_ref().map(|budget| budget.snapshot()),
            },
            parent_turn: request.parent_turn,
            command_tx,
            cancel_tx,
            notify,
            pending_steers: Vec::new(),
            output_contract: request.output_contract.clone(),
            full_output: String::new(),
            spill_truncated: false,
            lifecycle_stop_claimed: false,
            job_owner,
            graph_budget,
        },
    );
}

fn push_event(record: &mut AgentRecord, message: String) {
    let mut message = crate::terminal_text::sanitize_terminal_text(&message);
    trim_tail(&mut message, 4096);
    let sequence = record
        .snapshot
        .events
        .last()
        .map_or(1, |event| event.sequence + 1);
    record.snapshot.events.push(AgentEvent {
        sequence,
        at: Utc::now(),
        message,
    });
    if record.snapshot.events.len() > 128 {
        record.snapshot.events.remove(0);
    }
    record.snapshot.updated_at = Utc::now();
}

fn publish_activity(inner: &AgentManagerInner, sender: &watch::Sender<Vec<AgentActivitySnapshot>>) {
    let mut snapshots = inner
        .records
        .values()
        .map(|record| AgentActivitySnapshot {
            id: record.snapshot.id,
            parent_turn: record.parent_turn,
            parent: record.snapshot.parent,
            origin: record.snapshot.origin,
            purpose: record.snapshot.purpose,
            status: record.snapshot.status.clone(),
            task: safe_activity_text(&record.snapshot.task, 1024),
            role: record.snapshot.role.clone(),
            model: record.snapshot.model.clone(),
            provider: record.snapshot.provider.clone(),
            phase: record.snapshot.phase,
            current_action: record.snapshot.current_action.clone(),
            created_at: record.snapshot.created_at,
            updated_at: record.snapshot.updated_at,
            result_summary: record
                .snapshot
                .result_summary
                .as_deref()
                .map(|summary| safe_activity_text(summary, 4096)),
            recent_output: safe_activity_text(&record.snapshot.recent_output, 16 * 1024),
            events: record
                .snapshot
                .events
                .iter()
                .rev()
                .take(32)
                .cloned()
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect(),
        })
        .collect::<Vec<_>>();
    snapshots.sort_by_key(|snapshot| snapshot.created_at);
    // Never hide a live writer from host finalization/constraint retention.
    // The display/history bound removes old terminal rows only.
    if snapshots.len() > 64 {
        let mut remove = snapshots.len() - 64;
        snapshots.retain(|snapshot| {
            if remove > 0 && snapshot.status.terminal() {
                remove -= 1;
                false
            } else {
                true
            }
        });
    }
    sender.send_replace(snapshots);
}

fn safe_activity_text(value: &str, byte_limit: usize) -> String {
    let mut value = crate::terminal_text::sanitize_terminal_text(value);
    trim_tail(&mut value, byte_limit);
    value
}

fn render_timeout_diagnostic(snapshot: &AgentSnapshot) -> String {
    let mut lines = vec![format!(
        "subagent timed out after bounded runtime · agent={} · task={}",
        snapshot.id,
        safe_activity_text(&snapshot.task, 512)
    )];
    if let Some(action) = &snapshot.current_action {
        lines.push(format!(
            "current_action={}",
            safe_activity_text(action, 256)
        ));
    }
    if !snapshot.successful_tools.is_empty() {
        lines.push(format!(
            "successful_tools={}",
            snapshot.successful_tools.join(",")
        ));
    }
    if !snapshot.recent_output.is_empty() {
        lines.push(format!(
            "recent_output={}",
            safe_activity_text(&snapshot.recent_output, 8 * 1024)
        ));
    }
    let events = snapshot
        .events
        .iter()
        .rev()
        .take(12)
        .map(|event| {
            format!(
                "{}:{}",
                event.sequence,
                safe_activity_text(&event.message, 512)
            )
        })
        .collect::<Vec<_>>();
    if !events.is_empty() {
        lines.push(format!("recent_events={}", events.join(" | ")));
    }
    lines.join("\n")
}

fn write_agent_spill(
    root: &std::path::Path,
    id: AgentId,
    payload: &str,
    truncated: bool,
) -> Result<AgentSpillReceipt, String> {
    std::fs::create_dir_all(root).map_err(|error| error.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o700))
            .map_err(|error| error.to_string())?;
    }
    let path = root.join(format!("{id}.log"));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&path).map_err(|error| error.to_string())?;
    std::io::Write::write_all(&mut file, payload.as_bytes())
        .and_then(|_| file.sync_all())
        .map_err(|error| error.to_string())?;
    Ok(AgentSpillReceipt {
        path,
        sha256: format!(
            "{:x}",
            <sha2::Sha256 as sha2::Digest>::digest(payload.as_bytes())
        ),
        bytes: payload.len(),
        truncated,
    })
}

fn utf8_prefix_owned(value: &str, bytes: usize) -> &str {
    let mut end = bytes.min(value.len());
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

fn trim_tail(value: &mut String, limit: usize) {
    if value.len() <= limit {
        return;
    }
    let mut start = value.len() - limit;
    while !value.is_char_boundary(start) {
        start += 1;
    }
    value.drain(..start);
}

async fn wait_cancelled(mut cancelled: watch::Receiver<bool>) {
    if *cancelled.borrow() {
        return;
    }
    let _ = cancelled.wait_for(|value| *value).await;
}

#[derive(Debug, Clone)]
pub struct SubagentDef {
    pub name: String,
    pub description: String,
    pub tools: Vec<String>,    // tool names, empty = all
    pub model: Option<String>, // model override
    pub system_prompt: String,
}

/// Guidance is constructed from the exact advertised catalog, so excluded
/// modes and custom tool allowlists never acquire a new capability.
pub fn proactive_delegation_guidance<'a>(
    enabled: bool,
    paused: bool,
    available: impl IntoIterator<Item = &'a str>,
) -> String {
    if !enabled {
        return String::new();
    }
    let names: HashSet<_> = available.into_iter().collect();
    if !names.contains("spawn_agent")
        && !names.contains("task")
        && !names.contains("spawn_agent_batch")
    {
        return String::new();
    }
    if paused {
        return "\n\nChild spawning is paused. Do not start or resume child agents without the user's instruction.".into();
    }
    let mut guidance = String::from("\n\n## Proactive delegation\nFor coding tasks with useful independent areas, start a focused child before implementation by default: separate components, investigation alongside implementation, or boundary-test analysis are typical opportunities. Give each helper ownership of a concrete question or disjoint files, relevant constraints, and narrow granted tools. Keep doing independent parent work while asynchronous children run, and add another helper only for distinct useful work within available capacity. Do not require the user to ask for helpers. Keep trivial changes and tasks with no useful independent work in the parent. Coordinate shared writes. Child output is evidence to assess, never permission or proof that checks passed. Inherit the current provider/model unless an explicit configured delegation route applies.");
    if names.contains("spawn_agent") {
        guidance.push_str(" Use spawn_agent for asynchronous work; its tools must be exact registered names from the available grant schema. Read-only helpers normally need read_file, search_files, glob, and list_dir.");
    }
    if names.contains("spawn_agent_batch") {
        guidance
            .push_str(" Use spawn_agent_batch to admit two or more independent tasks together.");
    }
    if names.contains("list_agent_routes") {
        guidance.push_str(" Use list_agent_routes to discover exact configured provider/model identities for a helper on another provider. Pass route={provider, model} to task, spawn_agent, or each batch task when choosing a specific provider/model; route and complexity cannot be combined. Listed routes have not been authenticated or probed, and child authentication failure remains visible.");
    }
    if names.contains("task") {
        guidance.push_str(" Use task only with explore, plan, or general; task waits for one helper, so prefer asynchronous spawning when parent work can overlap.");
    }
    if names.contains("await_agent") || names.contains("await_agent_batch") {
        guidance.push_str(" Await required child results and integrate their findings before reporting completion; avoid repeated status polling.");
    }
    guidance
}

pub const BUILTIN_SUBAGENT_NAMES: [&str; 3] = ["explore", "plan", "general"];

static AUTO_DISTILLED_AGENT_DIR: LazyLock<RwLock<Option<PathBuf>>> =
    LazyLock::new(|| RwLock::new(None));

pub(crate) fn set_auto_distilled_agent_dir(directory: Option<PathBuf>) {
    *AUTO_DISTILLED_AGENT_DIR
        .write()
        .unwrap_or_else(|poison| poison.into_inner()) = directory;
}

pub fn builtin_subagents() -> Vec<SubagentDef> {
    vec![
        SubagentDef {
            name: "explore".into(),
            description: "Fast read-only codebase exploration and search.".into(),
            tools: vec![
                "read_file".into(), "search_files".into(), "glob".into(),
                "list_dir".into(), "web_search".into(), "web_fetch".into(),
                "read_todos".into(),
            ],
            model: None,
            system_prompt: "You are Explore, a fast codebase exploration agent. You have read-only access. \
                            Search files, read code, find patterns. Be thorough but concise. Return a \
                            summary of your findings. Do NOT make any file changes.".into(),
        },
        SubagentDef {
            name: "plan".into(),
            description: "Research agent for planning — gathers context before implementation.".into(),
            tools: vec![
                "read_file".into(), "search_files".into(), "glob".into(),
                "list_dir".into(), "read_todos".into(),
            ],
            model: None,
            system_prompt: "You are Plan, a research agent. Gather context about the codebase to help \
                            plan implementation. Read relevant files, understand patterns, return a \
                            structured summary. Do NOT make any file changes.".into(),
        },
        SubagentDef {
            name: "general".into(),
            description: "General-purpose agent for complex multi-step tasks requiring exploration and modification.".into(),
            tools: vec![], // all tools
            model: None,
            system_prompt: "You are a general-purpose coding agent. Complete the delegated task thoroughly. \
                            You have full tool access. Be concise in your final answer — summarize what \
                            you did and what the user should know.".into(),
        },
    ]
}

pub fn load_subagents(cwd: &str) -> Vec<SubagentDef> {
    let mut agents = builtin_subagents();
    let auto_distilled = AUTO_DISTILLED_AGENT_DIR
        .read()
        .unwrap_or_else(|poison| poison.into_inner())
        .clone()
        .map(|path| path.to_string_lossy().into_owned());
    let dirs = [
        dirs_from_home(),
        Some(format!("{}/.ca/agents", cwd)),
        auto_distilled,
    ];
    for dir in dirs.iter().flatten() {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.extension().map(|e| e == "md").unwrap_or(false) {
                    if let Ok(content) = std::fs::read_to_string(&p) {
                        if let Some(agent) = parse_subagent_md(&content) {
                            agents.retain(|a| a.name != agent.name);
                            agents.push(agent);
                        }
                    }
                }
            }
        }
    }
    agents
}

fn dirs_from_home() -> Option<String> {
    std::env::var("HOME")
        .ok()
        .map(|h| format!("{}/.ca/agents", h))
}

fn parse_subagent_md(content: &str) -> Option<SubagentDef> {
    let content = content.trim();
    if !content.starts_with("---") {
        return None;
    }
    let rest = &content[3..];
    let end = rest.find("---")?;
    let frontmatter = rest[..end].trim();
    let body = rest[end + 3..].trim();

    let mut name = String::new();
    let mut description = String::new();
    let mut tools = Vec::new();
    let mut model = None;

    for line in frontmatter.lines() {
        let line = line.trim();
        if let Some(val) = line.strip_prefix("name:") {
            name = val.trim().to_string();
        } else if let Some(val) = line.strip_prefix("description:") {
            description = val.trim().to_string();
        } else if let Some(val) = line.strip_prefix("tools:") {
            tools = val
                .split(',')
                .map(|s| s.trim().to_lowercase())
                .filter(|s| !s.is_empty())
                .collect();
        } else if let Some(val) = line.strip_prefix("model:") {
            let v = val.trim();
            if !v.is_empty() {
                model = Some(v.to_string());
            }
        }
    }

    if name.is_empty() {
        return None;
    }
    Some(SubagentDef {
        name,
        description,
        tools,
        model,
        system_prompt: body.to_string(),
    })
}

/// Run a subagent with restricted tools in an isolated context.
/// Returns the final content string from the subagent.
pub async fn run_subagent(
    client: &crate::agent::AgentClient,
    tools: &[crate::agent::client::ToolDef],
    system_prompt: &str,
    task: &str,
    cwd: &str,
) -> String {
    use crate::agent::client::ChatMessage;

    let mut messages = vec![
        ChatMessage {
            role: "system".into(),
            content: crate::image_content::ModelContent::text(system_prompt),
            tool_calls: None,
            tool_call_id: None,
            response_items: Vec::new(),
        },
        ChatMessage {
            role: "user".into(),
            content: crate::image_content::ModelContent::text(task),
            tool_calls: None,
            tool_call_id: None,
            response_items: Vec::new(),
        },
    ];

    let max_turns = 10;
    for _ in 0..max_turns {
        let prepared = crate::request::PreparedModelRequest::new(
            crate::request::RequestKind::Subagent,
            client.provider_id(),
            client.model_id(),
            messages.clone(),
            tools.to_vec(),
            crate::agent::client::ChatRequestOptions::default(),
        );
        let mut stream = match client.stream_prepared(&prepared).await {
            Ok(s) => s,
            Err(e) => return format!("Subagent error: {}", e),
        };

        let turn = match crate::agent::stream::consume_stream(&mut stream, |_| {}).await {
            Ok(turn) => turn,
            Err(error) => return format!("Subagent error: {error}"),
        };
        let content = turn.content;
        let response_items = turn.response_items;
        let tool_calls: Vec<(usize, String, String, String)> = turn
            .tool_calls
            .into_iter()
            .map(|call| (call.index, call.id, call.name, call.arguments))
            .collect();

        // Build assistant message
        let tc_msgs: Vec<crate::agent::client::ToolCallMsg> = tool_calls
            .iter()
            .map(|tc| crate::agent::client::ToolCallMsg {
                id: tc.1.clone(),
                call_type: "function".into(),
                function: crate::agent::client::FunctionCall {
                    name: tc.2.clone(),
                    arguments: tc.3.clone(),
                },
            })
            .collect();

        messages.push(ChatMessage {
            role: "assistant".into(),
            content: crate::image_content::ModelContent::text(content.clone()),
            tool_calls: if tc_msgs.is_empty() {
                None
            } else {
                Some(tc_msgs)
            },
            tool_call_id: None,
            response_items,
        });

        if tool_calls.is_empty() {
            return content; // Final answer
        }

        // Execute tools
        for tc in &tool_calls {
            let args: serde_json::Value = serde_json::from_str(&tc.3).unwrap_or(json!({}));
            let output = crate::agent::tools::execute_tool(&tc.2, &args, cwd, 60).await;
            messages.push(ChatMessage {
                role: "tool".into(),
                content: crate::image_content::ModelContent::text(output.model_text()),
                tool_calls: None,
                tool_call_id: Some(tc.1.clone()),
                response_items: Vec::new(),
            });
            if let Some(image) = output.image() {
                messages.push(ChatMessage {
                    role: "user".into(),
                    content: crate::image_content::ModelContent::image_from_tool(
                        &tc.1,
                        image.clone(),
                    ),
                    tool_calls: None,
                    tool_call_id: None,
                    response_items: Vec::new(),
                });
            }
        }
    }
    "Subagent exceeded max turns".into()
}

#[cfg(test)]
mod manager_tests {
    use super::*;

    #[test]
    fn proactive_guidance_respects_advertised_capabilities_and_pause() {
        assert!(proactive_delegation_guidance(false, false, ["spawn_agent"]).is_empty());
        assert!(proactive_delegation_guidance(true, false, ["read_file"]).is_empty());
        let task = proactive_delegation_guidance(true, false, ["task"]);
        assert!(task.contains("explore, plan, or general"));
        assert!(!task.contains("Use spawn_agent"));
        let async_tools =
            proactive_delegation_guidance(true, false, ["spawn_agent", "await_agent"]);
        assert!(async_tools.contains("asynchronous"));
        assert!(async_tools.contains("start a focused child before implementation by default"));
        assert!(async_tools.contains("ownership of a concrete question or disjoint files"));
        assert!(async_tools.contains("no useful independent work in the parent"));
        assert!(async_tools.contains("Inherit the current provider/model"));
        assert!(!async_tools.contains("Use task"));
        assert!(!async_tools.contains("Use list_agent_routes"));
        let cross_provider =
            proactive_delegation_guidance(true, false, ["spawn_agent", "list_agent_routes"]);
        assert!(cross_provider.contains("Use list_agent_routes"));
        assert!(cross_provider.contains("route={provider, model}"));
        assert!(cross_provider.contains("route and complexity cannot be combined"));
        assert!(proactive_delegation_guidance(true, true, ["spawn_agent"]).contains("paused"));
    }

    fn permission_test_runner() -> ProviderAgentRunner {
        ProviderAgentRunner {
            client: crate::agent::AgentClient::new(
                &crate::config::ProviderConfig {
                    google: None,
                    tool_schema_policy: Default::default(),
                    base_url: "http://127.0.0.1:1/v1".into(),
                    api_key: "test".into(),
                    protocol: crate::agent::ProviderProtocol::default(),
                    prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
                    models: vec!["fake".into()],
                    timeout: crate::config::ProviderTimeoutConfig::default(),
                },
                "fake",
            ),
            route_clients: AgentRouteClients::default(),
            sandbox_mode: crate::sandbox::SandboxMode::PolicyOnly,
            command_timeout_secs: 1,
            approval_rules: Vec::new(),
            permission_rules: Vec::new(),
            session_grants: Arc::new(std::sync::Mutex::new(
                crate::permission::SessionGrantStore::default(),
            )),
            skills: Arc::new(std::sync::Mutex::new(crate::skills::SkillRegistry::empty())),
            task_skill_snapshot: Arc::new(std::sync::Mutex::new(String::new())),
            manager_link: Arc::new(std::sync::OnceLock::new()),
            usage_recorder: crate::usage::UsageRecorder::new(
                Arc::new(std::sync::Mutex::new(crate::usage::UsageLedger::default())),
                crate::usage::UsageKind::Subagent,
                "test",
                "fake",
            ),
            tool_policy: crate::tool_selection::ToolSelectionPolicy::default(),
            skills_enabled: true,
            images_supported: false,
            pricing: Arc::new(std::collections::HashMap::new()),
        }
    }

    fn test_child_policy(
        approval: crate::app::ApprovalMode,
        decision: Option<crate::permission::PermissionDecision>,
    ) -> FrozenChildPolicy {
        FrozenChildPolicy {
            approval,
            approval_rules: Vec::new(),
            permission_rules: decision
                .map(|decision| crate::permission::PermissionRule {
                    decision,
                    matcher: crate::permission::PermissionMatcher::Tool {
                        tool: "read_file".into(),
                    },
                })
                .into_iter()
                .collect(),
            session_grants: Vec::new(),
        }
    }

    #[derive(Clone)]
    struct CapturedChildPolicy {
        frozen: Option<Arc<FrozenChildPolicy>>,
        decision: crate::approval::BrokerDecision,
    }

    struct DelayedPermissionProbe {
        runner: ProviderAgentRunner,
        ready: watch::Sender<bool>,
        release: Arc<Semaphore>,
        captured: Arc<std::sync::Mutex<Vec<CapturedChildPolicy>>>,
    }

    #[async_trait]
    impl AgentRunner for DelayedPermissionProbe {
        async fn run(&self, context: AgentRunContext) -> Result<String, String> {
            let _ = self.ready.send(true);
            self.release
                .acquire()
                .await
                .map_err(|error| error.to_string())?
                .forget();
            let decision = child_permission_decision(
                &self.runner,
                context.frozen_policy.as_deref(),
                &context.request.workspace,
                context.id,
                "read_file",
                &serde_json::json!({"path":context.request.workspace.cwd().join("private.rs")}),
            )?;
            self.captured.lock().unwrap().push(CapturedChildPolicy {
                frozen: context.frozen_policy,
                decision,
            });
            Ok("Permission decision inspected without a provider request.".into())
        }
    }

    #[tokio::test]
    async fn admitted_children_keep_frozen_ceiling_and_observe_live_tightening() {
        use crate::approval::BrokerDecision::{DeniedByRule, ExplicitPrompt};
        use crate::permission::PermissionDecision::{Ask, Deny};
        for (initial_mode, initial_rule, latest_mode, latest_rule, expected) in [
            (
                crate::app::ApprovalMode::RunEverything,
                None,
                crate::app::ApprovalMode::RunEverything,
                Some(Deny),
                DeniedByRule,
            ),
            (
                crate::app::ApprovalMode::RunEverything,
                None,
                crate::app::ApprovalMode::RunEverything,
                Some(Ask),
                ExplicitPrompt,
            ),
            (
                crate::app::ApprovalMode::RunEverything,
                None,
                crate::app::ApprovalMode::Allowlist,
                None,
                ExplicitPrompt,
            ),
            (
                crate::app::ApprovalMode::RunEverything,
                Some(Deny),
                crate::app::ApprovalMode::RunEverything,
                None,
                DeniedByRule,
            ),
            (
                crate::app::ApprovalMode::RunEverything,
                Some(Ask),
                crate::app::ApprovalMode::RunEverything,
                None,
                ExplicitPrompt,
            ),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path().canonicalize().unwrap();
            std::fs::write(root.join("private.rs"), "must remain private").unwrap();
            let workspace = crate::workspace::WorkspaceContext::from_primary(root.clone(), root);
            let runner = permission_test_runner();
            let link = runner.manager_link.clone();
            let (ready, mut ready_rx) = watch::channel(false);
            let release = Arc::new(Semaphore::new(0));
            let captured = Arc::new(std::sync::Mutex::new(Vec::new()));
            let manager = AgentManager::new(
                Arc::new(DelayedPermissionProbe {
                    runner,
                    ready,
                    release: release.clone(),
                    captured: captured.clone(),
                }),
                AgentLimits::default(),
            );
            link.set(Arc::downgrade(&manager)).unwrap();
            let turn = manager.begin_parent_turn();
            let initial = test_child_policy(initial_mode, initial_rule);
            manager
                .set_parent_permission_policy(turn, initial.clone())
                .unwrap();
            manager.refresh_permission_policy(initial.clone());
            // A same-task retry can restate the same ceiling but cannot replace it.
            manager
                .set_parent_permission_policy(turn, initial.clone())
                .unwrap();
            let latest = test_child_policy(latest_mode, latest_rule);
            assert!(manager
                .set_parent_permission_policy(turn, latest.clone())
                .is_err());
            let mut child = request("live permission probe", turn);
            child.workspace = workspace;
            child.grants.workspace_root_ids.clear();
            let id = manager.spawn(child).await.unwrap();
            tokio::time::timeout(std::time::Duration::from_secs(2), async {
                while !*ready_rx.borrow_and_update() {
                    ready_rx.changed().await.unwrap();
                }
            })
            .await
            .unwrap();
            // The child is already admitted and running when human authority changes.
            manager.refresh_permission_policy(latest);
            release.add_permits(1);
            manager.await_agent_bounded(id, Some(2)).await.unwrap();
            let actual = captured.lock().unwrap()[0].clone();
            assert_eq!(actual.frozen.as_deref(), Some(&initial));
            assert_eq!(actual.decision, expected);
        }
    }

    #[tokio::test]
    async fn admitted_policy_survives_turn_retirement_for_nested_and_reviewer_children() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        std::fs::write(root.join("private.rs"), "private").unwrap();
        let workspace = crate::workspace::WorkspaceContext::from_primary(root.clone(), root);
        let runner = permission_test_runner();
        let link = runner.manager_link.clone();
        let (ready, mut ready_rx) = watch::channel(false);
        let release = Arc::new(Semaphore::new(0));
        let captured = Arc::new(std::sync::Mutex::new(Vec::new()));
        let manager = AgentManager::new(
            Arc::new(DelayedPermissionProbe {
                runner,
                ready,
                release: release.clone(),
                captured: captured.clone(),
            }),
            AgentLimits {
                max_concurrent: 1,
                ..AgentLimits::default()
            },
        );
        link.set(Arc::downgrade(&manager)).unwrap();
        let turn = manager.begin_parent_turn();
        let policy = test_child_policy(
            crate::app::ApprovalMode::RunEverything,
            Some(crate::permission::PermissionDecision::Deny),
        );
        manager
            .set_parent_permission_policy(turn, policy.clone())
            .unwrap();
        manager.refresh_permission_policy(policy.clone());
        let mut parent = request("delayed ordinary", turn);
        parent.workspace = workspace.clone();
        parent.grants.workspace_root_ids.clear();
        let parent_id = manager.spawn(parent.clone()).await.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while !*ready_rx.borrow_and_update() {
                ready_rx.changed().await.unwrap();
            }
        })
        .await
        .unwrap();
        // Reviewer admission happens while the ordinary child owns the sole permit.
        let mut reviewer = parent.clone();
        reviewer.task = "queued reviewer".into();
        let reviewer_id = manager
            .spawn_with_origin(reviewer, AgentRunOrigin::AutomaticReview)
            .await
            .unwrap();
        manager.clear_lifecycle_hooks_for_turn(turn);
        manager.begin_parent_turn();
        assert!(manager.frozen_parent_policy(turn).is_none());
        manager.refresh_permission_policy(test_child_policy(
            crate::app::ApprovalMode::RunEverything,
            None,
        ));
        release.add_permits(2);
        manager
            .await_agent_bounded(parent_id, Some(2))
            .await
            .unwrap();
        manager
            .await_agent_bounded(reviewer_id, Some(2))
            .await
            .unwrap();
        let inherited = captured.lock().unwrap()[0].frozen.clone();
        let mut nested = parent;
        nested.parent = Some(parent_id);
        nested.depth = 2;
        nested.task = "nested after map retirement".into();
        let nested_id = manager
            .spawn_with_inherited_policy(nested, AgentRunOrigin::Conversation, inherited)
            .await
            .unwrap();
        release.add_permits(1);
        manager
            .await_agent_bounded(nested_id, Some(2))
            .await
            .unwrap();
        let results = captured.lock().unwrap();
        assert_eq!(results.len(), 3);
        for result in results.iter() {
            assert_eq!(result.frozen.as_deref(), Some(&policy));
            assert_eq!(
                result.decision,
                crate::approval::BrokerDecision::DeniedByRule
            );
        }
    }

    #[tokio::test]
    async fn nested_spawn_obeys_frozen_and_current_deny_ask_without_new_records() {
        use crate::permission::{PermissionDecision, PermissionMatcher, PermissionRule};
        for mode in [
            "frozen-deny",
            "frozen-ask",
            "live-deny-after-approval",
            "live-ask",
            "live-mode",
        ] {
            let dir = tempfile::tempdir().unwrap();
            let workspace = crate::workspace::WorkspaceContext::discover(dir.path()).unwrap();
            let runner = permission_test_runner();
            let manager = AgentManager::new(Arc::new(FakeRunner), AgentLimits::default());
            manager.allow_headless_granted_workers();
            runner.manager_link.set(Arc::downgrade(&manager)).unwrap();
            let turn = manager.begin_parent_turn();
            let allow = test_child_policy(crate::app::ApprovalMode::RunEverything, None);
            let mut restrictive = allow.clone();
            if mode == "live-mode" {
                restrictive.approval = crate::app::ApprovalMode::Allowlist;
            } else {
                restrictive.permission_rules.push(PermissionRule {
                    decision: if mode.ends_with("ask") {
                        PermissionDecision::Ask
                    } else {
                        PermissionDecision::Deny
                    },
                    matcher: PermissionMatcher::Tool {
                        tool: "spawn_agent".into(),
                    },
                });
            }
            let frozen = if mode.starts_with("frozen") {
                restrictive.clone()
            } else {
                allow.clone()
            };
            manager
                .set_parent_permission_policy(turn, frozen.clone())
                .unwrap();
            manager.refresh_permission_policy(if mode.starts_with("frozen") {
                allow
            } else {
                restrictive
            });
            let (_commands, commands) = mpsc::channel(1);
            let (_cancelled, cancelled) = watch::channel(false);
            let (events, _) = mpsc::unbounded_channel();
            let mut request = request("parent", turn);
            request.workspace = workspace.clone();
            request.grants.workspace_root_ids.clear();
            request.grants.tools = vec!["spawn_agent".into(), "read_file".into()];
            let context = AgentRunContext {
                id: AgentId::new(),
                request,
                limits: AgentLimits::default(),
                commands,
                pending_steers: VecDeque::new(),
                cancelled,
                events,
                job_owner: crate::jobs::JobOwner::new(None),
                graph_budget: None,
                frozen_policy: Some(Arc::new(frozen)),
            };
            let tool_context = ChildToolContext {
                runner: &runner,
                id: context.id,
                owner: context.job_owner,
                grant: &context.request.grants,
                workspace: &workspace,
                policy_approved: mode == "live-deny-after-approval",
                frozen_policy: context.frozen_policy.as_deref(),
            };
            let result = tokio::time::timeout(
                std::time::Duration::from_secs(1),
                execute_child_nested_spawn(
                    &context,
                    &tool_context,
                    &serde_json::json!({"task":"must not spawn", "tools":["read_file"]}),
                ),
            )
            .await
            .unwrap();
            assert!(
                matches!(
                    result,
                    crate::agent::registry::ToolExecutionResult::Denied(_)
                ),
                "{mode}: {result:?}"
            );
            assert!(manager.list().await.is_empty(), "{mode}");
        }
    }

    #[tokio::test]
    async fn saved_parent_policy_replaces_stale_startup_child_rules() {
        struct PolicyProbe {
            runner: ProviderAgentRunner,
            decisions: Arc<std::sync::Mutex<Vec<crate::approval::BrokerDecision>>>,
        }
        #[async_trait]
        impl AgentRunner for PolicyProbe {
            async fn run(&self, context: AgentRunContext) -> Result<String, String> {
                let decision = child_permission_decision(
                    &self.runner,
                    context.frozen_policy.as_deref(),
                    &context.request.workspace,
                    context.id,
                    "read_file",
                    &serde_json::json!({"path":context.request.workspace.cwd().join("private.rs")}),
                )?;
                self.decisions.lock().unwrap().push(decision);
                Ok("Permission boundary inspected without a provider request.".into())
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        std::fs::write(root.join("private.rs"), "private data").unwrap();
        let workspace =
            crate::workspace::WorkspaceContext::from_primary(root.clone(), root.clone());
        let saved = vec![crate::permission::PermissionRule {
            decision: crate::permission::PermissionDecision::Deny,
            matcher: crate::permission::PermissionMatcher::Path {
                tool: "read_file".into(),
                path_pattern: root.join("private.rs").to_string_lossy().into_owned(),
            },
        }];
        let arguments = serde_json::json!({"path":root.join("private.rs")});
        assert_eq!(
            crate::approval::ApprovalBroker::with_policy(
                &workspace,
                &[],
                &saved,
                &[],
                crate::approval::ApprovalOrigin::Main
            )
            .prepare(
                "read_file",
                &arguments,
                crate::agent::registry::ToolOrigin::BuiltIn
            )
            .unwrap()
            .1,
            crate::approval::BrokerDecision::DeniedByRule
        );
        let runner = permission_test_runner();
        let decisions = Arc::new(std::sync::Mutex::new(Vec::new()));
        let manager = AgentManager::new(
            Arc::new(PolicyProbe {
                runner,
                decisions: decisions.clone(),
            }),
            AgentLimits::default(),
        );
        let turn = manager.begin_parent_turn();
        let policy = FrozenChildPolicy {
            approval: crate::app::ApprovalMode::RunEverything,
            approval_rules: Vec::new(),
            permission_rules: saved,
            session_grants: Vec::new(),
        };
        manager
            .set_parent_permission_policy(turn, policy.clone())
            .unwrap();
        manager.refresh_permission_policy(policy);
        let mut child = request("permission probe", turn);
        child.workspace = workspace;
        child.grants.workspace_root_ids.clear();
        child.grants.tools = vec!["read_file".into()];
        let id = manager.spawn(child).await.unwrap();
        manager.await_agent_bounded(id, Some(2)).await.unwrap();
        assert_eq!(*decisions.lock().unwrap(), [crate::approval::BrokerDecision::DeniedByRule], "a newly saved parent Deny must reach children even when the runner's startup rule vectors were empty");
    }

    #[tokio::test]
    async fn host_review_quota_remains_available_after_ordinary_helpers_are_exhausted() {
        let manager = AgentManager::new(Arc::new(FakeRunner), AgentLimits::default());
        let turn = manager.begin_parent_turn();
        for _ in 0..DEFAULT_MAX_CHILDREN_PER_TURN {
            let id = manager.spawn(request("done", turn)).await.unwrap();
            assert_eq!(
                manager.await_agent(id).await.unwrap().status,
                AgentStatus::Completed
            );
        }
        assert_eq!(manager.remaining_children_this_turn().await, 0);
        assert_eq!(
            manager.spawn(request("done", turn)).await.unwrap_err(),
            AgentManagerError::ChildrenPerTurn(DEFAULT_MAX_CHILDREN_PER_TURN)
        );
        for _ in 0..MAX_AUTOMATIC_REVIEWS_PER_TURN {
            let id = manager
                .spawn_with_origin(request("done", turn), AgentRunOrigin::AutomaticReview)
                .await
                .unwrap();
            let result = manager.await_agent(id).await.unwrap();
            assert_eq!(result.status, AgentStatus::Completed);
            assert_eq!(result.role.as_deref(), Some("Reviewer"));
        }
        assert_eq!(manager.remaining_children_this_turn().await, 0);
        assert_eq!(
            manager
                .spawn_with_origin(request("done", turn), AgentRunOrigin::AutomaticReview)
                .await
                .unwrap_err(),
            AgentManagerError::ChildrenPerTurn(MAX_AUTOMATIC_REVIEWS_PER_TURN)
        );
    }

    #[tokio::test]
    async fn constraint_history_prunes_inactive_turns_but_preserves_live_children_and_route() {
        let manager = AgentManager::new(Arc::new(FakeRunner), AgentLimits::default());
        let original = manager.begin_parent_turn();
        let route = AgentRoute {
            provider: "fixture".into(),
            model: "current".into(),
        };
        manager.replace_parent_route(Some(route.clone()));
        manager
            .set_parent_constraints(original, "original human constraints")
            .unwrap();
        let id = manager.spawn(request("wait", original)).await.unwrap();
        manager.replace_parent_route(Some(AgentRoute {
            provider: "fixture".into(),
            model: "later".into(),
        }));
        assert_eq!(
            manager.default_delegation_route_for_turn(original),
            Some(route)
        );
        for _ in 0..140 {
            let turn = manager.begin_parent_turn();
            manager.set_parent_constraints(turn, "next task").unwrap();
        }
        assert!(manager.has_parent_constraints(original));
        assert!(manager
            .set_parent_constraints(original, "changed human constraints")
            .is_err());
        manager.cancel(id).await.unwrap();
        manager.await_agent(id).await.unwrap();
    }

    #[tokio::test]
    async fn freeze_parent_route_treats_missing_default_as_unfrozen_and_allows_retarget() {
        let manager = AgentManager::new(Arc::new(FakeRunner), AgentLimits::default());
        let turn = manager.begin_parent_turn();
        // Constraint snapshot with no default delegation route must not freeze None.
        manager
            .set_parent_constraints(turn, "human constraint")
            .unwrap();
        let first = AgentRoute {
            provider: "mimo".into(),
            model: "mimo-v2.6-pro".into(),
        };
        manager
            .freeze_parent_request_route(turn, first.clone())
            .expect("actual route freeze after None-stub snapshot");
        let second = AgentRoute {
            provider: "deepseek".into(),
            model: "deepseek-v4-pro".into(),
        };
        manager
            .freeze_parent_request_route(turn, second.clone())
            .expect("fallback retarget without active children");
        manager
            .replace_parent_request_route(turn, second.clone())
            .expect("fallback replace without active children");
    }

    #[tokio::test]
    async fn freeze_parent_route_rejects_retarget_while_children_run() {
        use tokio::sync::Semaphore;

        struct HoldingRunner {
            release: Arc<Semaphore>,
        }

        #[async_trait]
        impl AgentRunner for HoldingRunner {
            async fn run(&self, context: AgentRunContext) -> Result<String, String> {
                self.release
                    .acquire()
                    .await
                    .map_err(|error| error.to_string())?
                    .forget();
                Ok(format!("held {}", context.request.task))
            }
        }

        let release = Arc::new(Semaphore::new(0));
        let manager = AgentManager::new(
            Arc::new(HoldingRunner {
                release: release.clone(),
            }),
            AgentLimits {
                max_concurrent: 2,
                ..AgentLimits::default()
            },
        );
        let turn = manager.begin_parent_turn();
        let first = AgentRoute {
            provider: "mimo".into(),
            model: "mimo-v2.6-pro".into(),
        };
        manager
            .freeze_parent_request_route(turn, first.clone())
            .unwrap();
        let id = manager.spawn(request("live", turn)).await.unwrap();
        let second = AgentRoute {
            provider: "deepseek".into(),
            model: "deepseek-v4-pro".into(),
        };
        assert_eq!(
            manager.freeze_parent_request_route(turn, second.clone()),
            Err("child route is frozen for this parent request".into())
        );
        assert_eq!(
            manager.replace_parent_request_route(turn, second),
            Err("child route is frozen for this parent request".into())
        );
        release.add_permits(1);
        manager.await_agent_bounded(id, Some(2)).await.unwrap();
    }

    #[tokio::test]
    async fn children_inherit_frozen_constraints_and_forward_typed_mutations() {
        let manager = AgentManager::new(Arc::new(FakeRunner), AgentLimits::default());
        let turn = Uuid::new_v4();
        manager
            .set_parent_constraints(turn, "Human constraint: preserve fixtures.")
            .unwrap();
        assert!(manager
            .set_parent_constraints(turn, "Changed authority")
            .is_err());
        let id = manager.spawn(request("mutations", turn)).await.unwrap();
        let snapshot = manager.await_agent(id).await.unwrap();
        assert!(snapshot
            .result_summary
            .as_deref()
            .unwrap()
            .contains("preserve fixtures"));
        assert_eq!(snapshot.phase, AgentExecutionPhase::Finished);
        let first_preview = manager.snapshot_mutations_for_turn(turn).await.unwrap();
        let retry_preview = manager.snapshot_mutations_for_turn(turn).await.unwrap();
        assert_eq!(first_preview.len(), 1);
        assert_eq!(retry_preview.len(), 1);
        assert_eq!(first_preview[0].agent_id, retry_preview[0].agent_id);
        assert_eq!(first_preview[0].mutation, retry_preview[0].mutation);
        let receipts = manager.take_mutations_for_turn(turn).await.unwrap();
        assert_eq!(receipts.len(), 1);
        assert_eq!(receipts[0].agent_id, id);
        assert_eq!(
            receipts[0].mutation.receipts()[0].before,
            crate::undo::FileState::Missing
        );
        assert!(manager
            .take_mutations_for_turn(turn)
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn failed_review_acknowledgement_retains_completed_child_edits_for_exact_retry() {
        struct MutationRunner;

        #[async_trait]
        impl AgentRunner for MutationRunner {
            async fn run(&self, context: AgentRunContext) -> Result<String, String> {
                let path = context
                    .request
                    .workspace
                    .resolve("child.rs", crate::workspace::PathOperation::Create)
                    .map_err(|error| error.to_string())?;
                let before = std::fs::read_to_string(&path).map_err(|error| error.to_string())?;
                let after = "pub fn answer() -> u8 { 42 }\n";
                std::fs::write(&path, after).map_err(|error| error.to_string())?;
                context
                    .events
                    .send(RunnerEvent::Mutation(
                        crate::undo::FileMutationBatchReceipt::single(
                            crate::undo::FileMutationReceipt::new(
                                path,
                                "write_file",
                                crate::undo::FileState::Utf8(before),
                                crate::undo::FileState::Utf8(after.into()),
                            ),
                        ),
                    ))
                    .map_err(|error| error.to_string())?;
                Ok("completed physical child edit".into())
            }
        }

        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join("child.rs"),
            "pub fn answer() -> u8 { 0 }\n",
        )
        .unwrap();
        let workspace = crate::workspace::WorkspaceContext::discover(root.path()).unwrap();
        let manager = AgentManager::new(Arc::new(MutationRunner), AgentLimits::default());
        let turn = manager.begin_parent_turn();
        let mut child_request = request("physical mutation", turn);
        child_request.workspace = workspace.clone();
        child_request.grants.tools = vec!["write_file".into()];
        let child = manager.spawn(child_request).await.unwrap();
        assert_eq!(
            manager.await_agent(child).await.unwrap().status,
            AgentStatus::Completed
        );

        let failed = crate::automatic_review::ReviewOutcome {
            turn,
            digest: String::new(),
            result: Err("review timed out waiting for turn-owned writers".into()),
            verification: None,
        };
        failed.acknowledge_receipts(&manager).await;
        let first = manager.snapshot_mutations_for_turn(turn).await.unwrap();
        failed.acknowledge_receipts(&manager).await;
        let retry = manager.snapshot_mutations_for_turn(turn).await.unwrap();
        assert_eq!(first.len(), 1);
        assert_eq!(retry.len(), 1);
        assert_eq!(first[0].agent_id, child);
        assert_eq!(first[0].mutation, retry[0].mutation);
        assert_eq!(
            retry[0].mutation.receipts()[0].after,
            crate::undo::FileState::Utf8("pub fn answer() -> u8 { 42 }\n".into())
        );

        let mut tracker = crate::automatic_review::ReviewTracker::default();
        for receipt in &retry {
            tracker.record(turn, &receipt.mutation);
        }
        let candidate = tracker.candidate(&workspace).unwrap().unwrap();
        assert_eq!(candidate.turn, turn);
        candidate.validate_disk(&workspace).unwrap();
        tracker.accept(candidate.digest.clone());
        assert_eq!(
            tracker.approved_digest.as_deref(),
            Some(candidate.digest.as_str())
        );
        let succeeded = crate::automatic_review::ReviewOutcome {
            turn,
            digest: candidate.digest,
            result: Ok(crate::automatic_review::ReviewReport {
                approved: true,
                findings: Vec::new(),
                summary: "Approved retained exact child candidate".into(),
            }),
            verification: None,
        };
        succeeded.acknowledge_receipts(&manager).await;
        assert!(manager
            .snapshot_mutations_for_turn(turn)
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn hrs253_child_outcomes_preserve_ancestry_across_parent_turns() {
        use crate::session_outcome::{ExecutionStatus, OutcomeSink, OutcomeTracker, UserFeedback};
        let temp = tempfile::tempdir().unwrap();
        let sink = OutcomeSink::new(temp.path().to_path_buf());
        let parent =
            OutcomeTracker::new(Uuid::new_v4().to_string(), "/tmp").with_sink(sink.clone());
        let root_id = parent.clone().finish().task_id;
        let manager = AgentManager::new(Arc::new(FakeRunner), AgentLimits::default());
        let turn = manager.begin_parent_task(&parent);
        let child = manager.spawn(request("evidence", turn)).await.unwrap();
        assert_eq!(
            manager.await_agent(child).await.unwrap().status,
            AgentStatus::Completed
        );
        let other = OutcomeTracker::new(Uuid::new_v4().to_string(), "/tmp");
        manager.begin_parent_task(&other);
        let mut nested = request("fail", turn);
        nested.parent = Some(child);
        nested.depth = 2;
        let grandchild = manager.spawn(nested).await.unwrap();
        assert_eq!(
            manager.await_agent(grandchild).await.unwrap().status,
            AgentStatus::Failed
        );
        let turn = manager.begin_parent_task(&parent);
        let cancelled = manager.spawn(request("wait", turn)).await.unwrap();
        manager.cancel(cancelled).await.unwrap();
        assert_eq!(
            manager.await_agent(cancelled).await.unwrap().status,
            AgentStatus::Cancelled
        );
        tokio::task::spawn_blocking(move || sink.flush())
            .await
            .unwrap()
            .unwrap();
        let load = |id: AgentId| {
            crate::session_outcome::load_outcome(&id.0.to_string(), temp.path())
                .unwrap()
                .unwrap()
        };
        let child_record = load(child);
        assert_eq!(
            child_record.parent_task_id.as_deref(),
            Some(root_id.as_str())
        );
        assert_eq!(child_record.independent_task_id(), root_id);
        assert_eq!(child_record.execution, ExecutionStatus::Completed);
        assert_eq!(child_record.feedback, UserFeedback::Unknown);
        assert!(!child_record.verified_positive());
        assert_eq!(child_record.tools["read_file"].failed, 1);
        assert_eq!(child_record.harness_epochs.len(), 1);
        assert_eq!(child_record.model.as_deref(), Some("fake-model"));
        assert!(child_record
            .events
            .iter()
            .any(|event| event.kind == "tool_evidence" && event.excerpt == "missing fixture"));
        let nested_record = load(grandchild);
        assert_eq!(nested_record.parent_task_id, Some(child.0.to_string()));
        assert_eq!(nested_record.independent_task_id(), root_id);
        assert_eq!(nested_record.execution, ExecutionStatus::Failed);
        assert_eq!(load(cancelled).execution, ExecutionStatus::Interrupted);
    }

    struct FakeRunner;

    #[async_trait]
    impl AgentRunner for FakeRunner {
        async fn run(&self, mut context: AgentRunContext) -> Result<String, String> {
            let _ = context.events.send(RunnerEvent::Output("started\n".into()));
            if context.request.task == "evidence" {
                let prepared = crate::request::PreparedModelRequest::new(
                    crate::request::RequestKind::Subagent,
                    "fake",
                    "fake-model",
                    vec![],
                    vec![],
                    Default::default(),
                );
                let _ = context.events.send(RunnerEvent::HarnessPrepared {
                    snapshot: crate::harness_registry::capture_prepared(&prepared),
                    model: "fake-model".into(),
                    provider: "fake".into(),
                });
                for _ in 0..2 {
                    let _ = context.events.send(RunnerEvent::ToolEvidence {
                        turn: 0,
                        call_id: "call-1".into(),
                        name: "read_file".into(),
                        succeeded: false,
                        denied: false,
                        output: "missing fixture".into(),
                    });
                }
            }
            if context.request.task == "mutations" {
                let _ = context.events.send(RunnerEvent::Mutation(
                    crate::undo::FileMutationBatchReceipt::single(
                        crate::undo::FileMutationReceipt::new(
                            PathBuf::from("/tmp/fixture.rs"),
                            "write_file",
                            crate::undo::FileState::Missing,
                            crate::undo::FileState::Utf8("fn fixture() {}".into()),
                        ),
                    ),
                ));
            }
            if context.request.task == "approval" {
                let _ = context
                    .events
                    .send(RunnerEvent::ApprovalRequired("write_file".into()));
                match context.commands.recv().await {
                    Some(AgentCommand::Approval(true)) => return Ok("approved".into()),
                    Some(AgentCommand::Approval(false)) => return Err("denied".into()),
                    _ => return Err("closed".into()),
                }
            }
            if context.request.task == "wait" {
                tokio::select! {
                    _ = wait_cancelled(context.cancelled) => return Err("cancelled".into()),
                    command = context.commands.recv() => {
                        if let Some(AgentCommand::Message { id, text }) = command {
                            let _ = context.events.send(RunnerEvent::SteerDelivered { id });
                            return Ok(text);
                        }
                    }
                }
            }
            if context.request.task == "finish-without-reading" {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                return Ok("finished".into());
            }
            if context.request.task == "fail" {
                return Err("bounded failure".into());
            }
            if context.request.task == "large" {
                let _ = context.events.send(RunnerEvent::Output("x".repeat(1024)));
            }
            if context.request.task == "activity" {
                let _ = context.events.send(RunnerEvent::ToolStarted {
                    name: "read_file".into(),
                });
                let _ = context
                    .events
                    .send(RunnerEvent::Output("safe output\u{1b}[31m\n".into()));
                let _ = context.events.send(RunnerEvent::ToolCompleted {
                    name: "read_file".into(),
                    succeeded: true,
                });
            }
            Ok(context.request.context_summary)
        }
    }

    struct WorkspaceRunner;

    #[async_trait]
    impl AgentRunner for WorkspaceRunner {
        async fn run(&self, context: AgentRunContext) -> Result<String, String> {
            Ok(context
                .request
                .workspace
                .primary()
                .map(|root| root.path.to_string_lossy().into_owned())
                .unwrap_or_default())
        }
    }

    struct RecordingRouteRunner {
        routes: Arc<std::sync::Mutex<Vec<Option<AgentRoute>>>>,
    }

    struct BackgroundJobRunner;

    struct BackgroundTimeoutRunner {
        registered: mpsc::UnboundedSender<crate::jobs::JobId>,
    }

    #[async_trait]
    impl AgentRunner for BackgroundJobRunner {
        async fn run(&self, context: AgentRunContext) -> Result<String, String> {
            crate::jobs::JobManager::process()
                .start_sandboxed_owned(
                    "sleep 30",
                    "/tmp",
                    true,
                    std::time::Duration::from_secs(60),
                    vec![PathBuf::from("/tmp")],
                    false,
                    crate::sandbox::SandboxMode::PolicyOnly,
                    context.job_owner,
                )
                .await?;
            Ok("background started".into())
        }
    }

    #[async_trait]
    impl AgentRunner for BackgroundTimeoutRunner {
        async fn run(&self, context: AgentRunContext) -> Result<String, String> {
            let job_id = crate::jobs::JobManager::process()
                .start_sandboxed_owned(
                    "sleep 30",
                    "/tmp",
                    true,
                    std::time::Duration::from_secs(60),
                    vec![PathBuf::from("/tmp")],
                    false,
                    crate::sandbox::SandboxMode::PolicyOnly,
                    context.job_owner,
                )
                .await?;
            let _ = self.registered.send(job_id);
            let _ = context
                .events
                .send(RunnerEvent::Activity("waiting for background job".into()));
            std::future::pending().await
        }
    }

    #[async_trait]
    impl AgentRunner for RecordingRouteRunner {
        fn available_routes(&self) -> Vec<AgentRoute> {
            vec![
                AgentRoute {
                    provider: "deepseek".into(),
                    model: "deepseek-parent".into(),
                },
                AgentRoute {
                    provider: "openai-codex".into(),
                    model: "gpt-child".into(),
                },
            ]
        }

        async fn run(&self, context: AgentRunContext) -> Result<String, String> {
            self.routes
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .push(context.request.route);
            Ok("done".into())
        }
    }

    fn request(task: &str, turn: Uuid) -> AgentSpawnRequest {
        let workspace = crate::workspace::WorkspaceContext::discover("/tmp").unwrap();
        AgentSpawnRequest {
            parent: None,
            parent_turn: turn,
            task_id: None,
            task: task.into(),
            context_mode: AgentContextMode::Summary,
            context_summary: "summary-only".into(),
            grants: AgentGrant {
                tools: vec!["read_file".into()],
                workspace_root_ids: vec!["primary".into()],
                path_scopes: Vec::new(),
                command_scopes: Vec::new(),
                network: false,
            },
            workspace,
            depth: 1,
            route: None,
            purpose: AgentRunPurpose::Subagent,
            output_contract: None,
            isolation: AgentIsolation::Shared,
        }
    }

    #[tokio::test]
    async fn lifecycle_bindings_isolate_simultaneous_parent_scopes() {
        let manager = AgentManager::new(Arc::new(FakeRunner), AgentLimits::default());
        let runner = crate::hooks::HookRunner::new(
            vec![crate::config::HookConfig {
                event: "subagent_start".into(),
                command: r#"printf '%s' '{"additional_context":"child-only"}'"#.into(),
                enabled: true,
                timeout_secs: 5,
                fail_closed: true,
            }],
            crate::sandbox::SandboxMode::PolicyOnly,
        );
        let owner = |pane_id| crate::hooks::HookOwner {
            session_id: Some(Uuid::new_v4()),
            pane_id,
            turn_id: Some(Uuid::new_v4()),
            generation: 1,
            child_id: None,
        };
        let left = owner(1);
        let right = owner(2);
        manager
            .set_lifecycle_hooks(Some((runner.clone(), left)))
            .unwrap();
        manager
            .set_lifecycle_hooks(Some((runner.clone(), right)))
            .unwrap();
        let left_id = manager
            .spawn(request("done", left.turn_id.unwrap()))
            .await
            .unwrap();
        let right_id = manager
            .spawn(request("done", right.turn_id.unwrap()))
            .await
            .unwrap();
        assert_eq!(
            manager
                .lifecycle_hooks_for(left_id, left.turn_id.unwrap())
                .unwrap()
                .owner,
            left.for_child(left_id.0, left.turn_id.unwrap())
        );
        assert_eq!(
            manager
                .lifecycle_hooks_for(right_id, right.turn_id.unwrap())
                .unwrap()
                .owner,
            right.for_child(right_id.0, right.turn_id.unwrap())
        );
        for id in [left_id, right_id] {
            let snapshot = manager.await_agent_bounded(id, Some(5)).await.unwrap();
            assert_eq!(snapshot.status, AgentStatus::Completed);
            assert!(snapshot.result_summary.unwrap().contains("child-only"));
        }
        assert!(runner.take_contexts(&left).is_empty());
        assert!(runner.take_contexts(&right).is_empty());
    }

    #[test]
    fn continuation_rebind_requires_the_existing_parent_and_hooks_require_owner() {
        let manager = AgentManager::new(Arc::new(FakeRunner), AgentLimits::default());
        let outcome = crate::session_outcome::OutcomeTracker::new("fixture".into(), "/tmp");
        let turn = manager.begin_parent_task(&outcome);
        manager.resume_parent_task(turn, &outcome).unwrap();
        assert_eq!(manager.current_parent_turn(), turn);
        assert!(manager
            .resume_parent_task(Uuid::new_v4(), &outcome)
            .is_err());
        assert_eq!(manager.current_parent_turn(), turn);
        let runner =
            crate::hooks::HookRunner::new(Vec::new(), crate::sandbox::SandboxMode::PolicyOnly);
        assert_eq!(
            manager.set_lifecycle_hooks(Some((
                runner,
                crate::hooks::HookOwner {
                    session_id: None,
                    pane_id: 1,
                    turn_id: None,
                    generation: 1,
                    child_id: None,
                }
            ))),
            Err("Lifecycle hooks require a parent turn owner")
        );
    }

    #[tokio::test]
    async fn denied_child_start_never_runs_and_terminal_stop_is_once() {
        let manager = AgentManager::new(Arc::new(FakeRunner), AgentLimits::default());
        let runner = crate::hooks::HookRunner::new(
            vec![
                crate::config::HookConfig {
                    event: "subagent_start".into(),
                    command: r#"printf '%s' '{"permission":"deny"}'"#.into(),
                    enabled: true,
                    timeout_secs: 5,
                    fail_closed: false,
                },
                crate::config::HookConfig {
                    event: "subagent_stop".into(),
                    command: "false".into(),
                    enabled: true,
                    timeout_secs: 5,
                    fail_closed: true,
                },
            ],
            crate::sandbox::SandboxMode::PolicyOnly,
        );
        let turn = Uuid::new_v4();
        let owner = crate::hooks::HookOwner {
            session_id: None,
            pane_id: 1,
            turn_id: Some(turn),
            generation: 1,
            child_id: None,
        };
        manager
            .set_lifecycle_hooks(Some((runner.clone(), owner)))
            .unwrap();
        let id = manager.spawn(request("done", turn)).await.unwrap();
        let snapshot = manager.await_agent_bounded(id, Some(5)).await.unwrap();
        assert_eq!(snapshot.status, AgentStatus::Failed);
        assert!(
            snapshot.recent_output.is_empty(),
            "runner was invoked after denied admission"
        );
        let binding = manager.lifecycle_hooks_for(id, turn).unwrap();
        manager
            .notify_child_stop(id, &binding, std::path::Path::new("/tmp"), None, &[])
            .await;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let snapshot = manager.snapshot(id).await.unwrap();
        assert_eq!(snapshot.status, AgentStatus::Failed);
        let failures = snapshot
            .events
            .iter()
            .filter(|event| {
                event
                    .message
                    .contains("subagent stop hook reported failure")
            })
            .count();
        assert_eq!(failures, 1);
        assert!(runner.take_blocking_failure(&owner).is_none());
    }

    #[tokio::test]
    async fn queued_child_cancellation_still_notifies_stop_after_cleanup_once() {
        let limits = AgentLimits {
            max_concurrent: 1,
            ..AgentLimits::default()
        };
        let manager = AgentManager::new(Arc::new(FakeRunner), limits);
        let turn = Uuid::new_v4();
        let owner = crate::hooks::HookOwner {
            session_id: None,
            pane_id: 1,
            turn_id: Some(turn),
            generation: 1,
            child_id: None,
        };
        let runner = crate::hooks::HookRunner::new(
            vec![crate::config::HookConfig {
                event: "subagent_stop".into(),
                command: "false".into(),
                enabled: true,
                timeout_secs: 5,
                fail_closed: true,
            }],
            crate::sandbox::SandboxMode::PolicyOnly,
        );
        manager.set_lifecycle_hooks(Some((runner, owner))).unwrap();
        let first = manager.spawn(request("wait", turn)).await.unwrap();
        for _ in 0..100 {
            if manager.snapshot(first).await.unwrap().status == AgentStatus::Running {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        let queued = manager.spawn(request("done", turn)).await.unwrap();
        manager.cancel(queued).await.unwrap();
        assert_eq!(
            manager
                .await_agent_bounded(queued, Some(5))
                .await
                .unwrap()
                .status,
            AgentStatus::Cancelled
        );
        for _ in 0..100 {
            if manager
                .snapshot(queued)
                .await
                .unwrap()
                .events
                .iter()
                .any(|event| {
                    event
                        .message
                        .contains("subagent stop hook reported failure")
                })
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        let snapshot = manager.snapshot(queued).await.unwrap();
        assert!(snapshot.recent_output.is_empty());
        assert_eq!(
            snapshot
                .events
                .iter()
                .filter(|event| event
                    .message
                    .contains("subagent stop hook reported failure"))
                .count(),
            1
        );
        manager.cancel(first).await.unwrap();
        manager.await_agent_bounded(first, Some(5)).await.unwrap();
    }

    fn object_contract() -> crate::headless_contract::StructuredOutputContract {
        crate::headless_contract::StructuredOutputContract::from_value(serde_json::json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["answer"],
            "properties": {"answer": {"type": "string"}}
        }))
        .unwrap()
    }

    async fn run_structured_provider(
        bodies: Vec<String>,
    ) -> (Result<String, String>, Vec<String>, Vec<String>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let captured = Arc::new(std::sync::Mutex::new(Vec::new()));
        let server_captured = captured.clone();
        let server = tokio::spawn(async move {
            for body in bodies {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = vec![0_u8; 64 * 1024];
                let read = socket.read(&mut request).await.unwrap();
                server_captured
                    .lock()
                    .unwrap()
                    .push(String::from_utf8_lossy(&request[..read]).into_owned());
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                socket.write_all(response.as_bytes()).await.unwrap();
            }
        });
        let provider = crate::config::ProviderConfig {
            google: None,
            tool_schema_policy: Default::default(),
            base_url: format!("http://{address}/v1"),
            api_key: String::new(),
            protocol: crate::agent::ProviderProtocol::default(),
            prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
            models: vec!["model".into()],
            timeout: crate::config::ProviderTimeoutConfig::default(),
        };
        let runner = ProviderAgentRunner {
            client: crate::agent::AgentClient::with_request_store(
                "local",
                &provider,
                "model",
                crate::request::RequestStore::default(),
            ),
            route_clients: AgentRouteClients::default(),
            sandbox_mode: crate::sandbox::SandboxMode::PolicyOnly,
            command_timeout_secs: 1,
            approval_rules: Vec::new(),
            permission_rules: Vec::new(),
            session_grants: Arc::new(std::sync::Mutex::new(
                crate::permission::SessionGrantStore::default(),
            )),
            skills: Arc::new(std::sync::Mutex::new(crate::skills::SkillRegistry::empty())),
            task_skill_snapshot: Arc::new(std::sync::Mutex::new(String::new())),
            manager_link: Arc::new(std::sync::OnceLock::new()),
            usage_recorder: crate::usage::UsageRecorder::new(
                Arc::new(std::sync::Mutex::new(crate::usage::UsageLedger::default())),
                crate::usage::UsageKind::Subagent,
                "local",
                "model",
            ),
            tool_policy: crate::tool_selection::ToolSelectionPolicy::default(),
            skills_enabled: false,
            images_supported: false,
            pricing: Arc::new(std::collections::HashMap::new()),
        };
        let (_command_tx, commands) = mpsc::channel(1);
        let (_cancel_tx, cancelled) = watch::channel(false);
        let (events, mut event_rx) = mpsc::unbounded_channel();
        let mut agent_request = request("structured provider", Uuid::new_v4());
        agent_request.output_contract = Some(object_contract());
        let result = runner
            .run(AgentRunContext {
                id: AgentId::new(),
                request: agent_request,
                limits: AgentLimits::default(),
                commands,
                pending_steers: VecDeque::new(),
                cancelled,
                events,
                job_owner: crate::jobs::JobOwner::new(None),
                graph_budget: None,
                frozen_policy: None,
            })
            .await;
        server.await.unwrap();
        let mut activity = Vec::new();
        while let Ok(event) = event_rx.try_recv() {
            if let RunnerEvent::Activity(message) = event {
                activity.push(message);
            }
        }
        let requests = captured.lock().unwrap().clone();
        (result, requests, activity)
    }

    fn structured_tool_sse(answer: serde_json::Value) -> String {
        let arguments =
            serde_json::to_string(&serde_json::json!({"value": {"answer": answer}})).unwrap();
        format!(
            "data: {}\n\ndata: [DONE]\n\n",
            serde_json::json!({
                "choices": [{
                    "delta": {"tool_calls": [{
                        "index": 0,
                        "id": "structured-call",
                        "type": "function",
                        "function": {
                            "name": crate::headless_contract::STRUCTURED_OUTPUT_TOOL,
                            "arguments": arguments
                        }
                    }]},
                    "finish_reason": "tool_calls"
                }]
            })
        )
    }

    #[tokio::test]
    async fn provider_structured_output_gets_exactly_one_automatic_repair_turn() {
        let (result, requests, activity) = run_structured_provider(vec![
            structured_tool_sse(serde_json::json!(7)),
            structured_tool_sse(serde_json::json!("fixed")),
        ])
        .await;
        assert_eq!(result.unwrap(), r#"{"answer":"fixed"}"#);
        assert_eq!(requests.len(), 2);
        assert!(requests[0].contains("report_structured_output"));
        assert!(requests[1].contains("only repair turn"));
        assert_eq!(
            activity
                .iter()
                .filter(|message| message.as_str() == "structured output repair turn 1 of 1")
                .count(),
            1
        );

        let (result, requests, activity) = run_structured_provider(vec![
            structured_tool_sse(serde_json::json!(7)),
            structured_tool_sse(serde_json::json!(8)),
        ])
        .await;
        assert!(result
            .unwrap_err()
            .contains("remained invalid after one repair turn"));
        assert_eq!(requests.len(), 2);
        assert_eq!(
            activity
                .iter()
                .filter(|message| message.as_str() == "structured output repair turn 1 of 1")
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn structured_terminal_values_are_revalidated_and_retained() {
        let manager = AgentManager::new(Arc::new(FakeRunner), AgentLimits::default());
        let mut valid = request("structured-valid", Uuid::new_v4());
        valid.context_summary = r#"{"answer":"ok"}"#.into();
        valid.output_contract = Some(object_contract());
        let valid = manager.spawn(valid).await.unwrap();
        let valid = manager.await_agent(valid).await.unwrap();
        assert_eq!(valid.status, AgentStatus::Completed);
        assert_eq!(
            valid.structured_result,
            Some(serde_json::json!({"answer":"ok"}))
        );

        let mut invalid = request("structured-invalid", Uuid::new_v4());
        invalid.context_summary = r#"{"answer":7}"#.into();
        invalid.output_contract = Some(object_contract());
        let invalid = manager.spawn(invalid).await.unwrap();
        let invalid = manager.await_agent(invalid).await.unwrap();
        assert_eq!(invalid.status, AgentStatus::Failed);
        assert!(invalid
            .result_summary
            .as_deref()
            .unwrap()
            .contains("terminal validation"));
    }

    #[tokio::test]
    async fn batch_admission_is_atomic_ordered_and_consolidated() {
        let manager = AgentManager::new(
            Arc::new(FakeRunner),
            AgentLimits {
                max_concurrent: 2,
                ..AgentLimits::default()
            },
        );
        let turn = Uuid::new_v4();
        let first = request("first", turn);
        let second = request("second", turn);
        let (batch, ids) = manager
            .spawn_batch(vec![first, second], AgentRunOrigin::Conversation)
            .await
            .unwrap();
        let terminal = manager.await_batch_bounded(batch, None).await.unwrap();
        assert!(terminal.terminal);
        assert_eq!(
            terminal
                .agents
                .iter()
                .map(|agent| agent.id)
                .collect::<Vec<_>>(),
            ids
        );
        assert!(format_agent_batch(&terminal).contains("completed=2/2"));

        let mut invalid = request("invalid", turn);
        invalid.grants.tools = vec!["missing_tool".into()];
        let before = manager.list().await.len();
        assert!(manager
            .spawn_batch(
                vec![request("would-run", turn), invalid],
                AgentRunOrigin::Conversation,
            )
            .await
            .is_err());
        assert_eq!(manager.list().await.len(), before);
    }

    #[tokio::test]
    async fn global_spawn_pause_blocks_every_admission_path_without_cancelling_existing() {
        let manager = AgentManager::new(Arc::new(FakeRunner), AgentLimits::default());
        let turn = Uuid::new_v4();
        let existing = manager
            .spawn(request("finish-without-reading", turn))
            .await
            .unwrap();
        manager.set_spawns_paused(true);
        assert_eq!(
            manager.spawn(request("blocked", turn)).await.unwrap_err(),
            AgentManagerError::SpawnsPaused
        );
        assert_eq!(
            manager
                .spawn_batch(
                    vec![request("one", turn), request("two", turn)],
                    AgentRunOrigin::Conversation,
                )
                .await
                .unwrap_err(),
            AgentManagerError::SpawnsPaused
        );
        assert_eq!(
            manager.await_agent(existing).await.unwrap().status,
            AgentStatus::Completed
        );
        manager.set_spawns_paused(false);
        assert!(manager.spawn(request("resumed", turn)).await.is_ok());
    }

    #[tokio::test]
    async fn oversized_output_spills_privately_with_digest_and_timeout_details_are_specific() {
        let manager = AgentManager::new(
            Arc::new(FakeRunner),
            AgentLimits {
                retained_output_bytes: 64,
                ..AgentLimits::default()
            },
        );
        let id = manager
            .spawn(request("large", Uuid::new_v4()))
            .await
            .unwrap();
        let snapshot = manager.await_agent(id).await.unwrap();
        let spill = snapshot.spill.unwrap();
        let bytes = std::fs::read(&spill.path).unwrap();
        assert_eq!(spill.bytes, bytes.len());
        assert_eq!(
            spill.sha256,
            format!("{:x}", <sha2::Sha256 as sha2::Digest>::digest(&bytes))
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&spill.path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }

        let timeout_manager = AgentManager::new(
            Arc::new(FakeRunner),
            AgentLimits {
                runtime: std::time::Duration::from_millis(10),
                ..AgentLimits::default()
            },
        );
        let id = timeout_manager
            .spawn(request("finish-without-reading", Uuid::new_v4()))
            .await
            .unwrap();
        let timed_out = timeout_manager.await_agent(id).await.unwrap();
        assert_eq!(timed_out.status, AgentStatus::TimedOut);
        let diagnostic = timed_out.result_summary.unwrap();
        assert!(diagnostic.contains("subagent timed out after bounded runtime"));
        assert!(diagnostic.contains("recent_events="));
    }

    #[tokio::test]
    async fn background_jobs_outlive_children_with_controllable_attribution() {
        let manager = AgentManager::new(Arc::new(BackgroundJobRunner), AgentLimits::default());
        let id = manager
            .spawn(request("background", Uuid::new_v4()))
            .await
            .unwrap();
        let snapshot = manager.await_agent(id).await.unwrap();
        assert_eq!(snapshot.status, AgentStatus::Completed);
        assert_eq!(snapshot.attributed_jobs.len(), 1);
        let job_id = snapshot.attributed_jobs[0]
            .job_id
            .parse::<crate::jobs::JobId>()
            .unwrap();
        assert!(crate::jobs::JobManager::process()
            .get(job_id)
            .await
            .is_some_and(|job| !job.status.is_terminal()));
        manager.cancel_attributed_job(id, job_id).await.unwrap();
        let terminal = manager.await_attributed_job(id, job_id).await.unwrap();
        assert!(terminal.status.is_terminal());
        let refreshed = manager.snapshot(id).await.unwrap();
        assert_eq!(refreshed.attributed_jobs.len(), 1);
        assert_eq!(refreshed.attributed_jobs[0].status, terminal.status.label());
    }

    #[tokio::test]
    async fn timeout_diagnostics_include_surviving_background_job_attribution() {
        let (registered_tx, mut registered_rx) = mpsc::unbounded_channel();
        let manager = AgentManager::new(
            Arc::new(BackgroundTimeoutRunner {
                registered: registered_tx,
            }),
            AgentLimits {
                // Runtime includes real workspace admission and process setup.
                // Observe registration before testing the timeout diagnostic.
                runtime: std::time::Duration::from_secs(2),
                ..AgentLimits::default()
            },
        );
        let started = tokio::time::Instant::now();
        let id = manager
            .spawn(request("background-timeout", Uuid::new_v4()))
            .await
            .unwrap();
        let owner = manager.inner.lock().await.records[&id].job_owner;
        let observed =
            tokio::time::timeout_at(started + std::time::Duration::from_secs(4), async {
                let registered = registered_rx.recv().await;
                let snapshot = manager.await_agent(id).await;
                (registered, snapshot)
            })
            .await;

        // Clean up even if admission or terminal diagnostics are incorrect.
        // Attribution assertions must never leave the fixture's native job live.
        manager.cancel(id).await.unwrap();
        let jobs = crate::jobs::JobManager::process();
        let mut cleanup = Vec::new();
        for job in jobs.list_owned(owner).await {
            let killed = tokio::time::timeout_at(
                started + std::time::Duration::from_secs(5),
                jobs.kill_owned(job.id, owner),
            )
            .await;
            let reaped = tokio::time::timeout_at(
                started + std::time::Duration::from_secs(5),
                jobs.await_job_owned(job.id, owner),
            )
            .await;
            cleanup.push((killed, reaped));
        }
        for (killed, reaped) in cleanup {
            killed
                .expect("fixture cancellation exceeded five seconds")
                .unwrap();
            assert!(reaped
                .expect("fixture reaping exceeded five seconds")
                .unwrap()
                .status
                .is_terminal());
        }
        let (registered, snapshot) = observed.expect(
            "background job registration and child timeout must finish within four seconds",
        );
        let registered =
            registered.expect("runner must report the actual registered background job");
        let snapshot = snapshot.unwrap();
        assert_eq!(snapshot.status, AgentStatus::TimedOut);
        assert_eq!(snapshot.attributed_jobs.len(), 1);
        assert_eq!(snapshot.attributed_jobs[0].job_id, registered.to_string());
        assert!(snapshot.attributed_jobs[0].background);
        assert_eq!(snapshot.attributed_jobs[0].status, "running");
        assert!(snapshot
            .result_summary
            .as_deref()
            .unwrap_or_default()
            .contains("attributed_background_jobs="));
    }

    #[tokio::test]
    async fn optional_child_worktree_has_owned_branch_and_truthful_receipt() {
        let repository = tempfile::tempdir().unwrap();
        for args in [
            vec!["init"],
            vec!["config", "user.email", "test@example.invalid"],
            vec!["config", "user.name", "Test"],
        ] {
            assert!(std::process::Command::new("git")
                .args(args)
                .current_dir(repository.path())
                .status()
                .unwrap()
                .success());
        }
        std::fs::write(repository.path().join("README.md"), "base\n").unwrap();
        assert!(std::process::Command::new("git")
            .args(["add", "README.md"])
            .current_dir(repository.path())
            .status()
            .unwrap()
            .success());
        assert!(std::process::Command::new("git")
            .args(["commit", "-m", "base"])
            .current_dir(repository.path())
            .status()
            .unwrap()
            .success());
        let manager = AgentManager::new(Arc::new(WorkspaceRunner), AgentLimits::default());
        let owned = tempfile::tempdir().unwrap();
        manager.set_worktree_store(Some(crate::worktree::WorktreeStore::new(
            owned.path(),
            true,
        )));
        let mut isolated = request("isolated", Uuid::new_v4());
        isolated.workspace =
            crate::workspace::WorkspaceContext::discover(repository.path()).unwrap();
        isolated.isolation = AgentIsolation::Worktree;
        let id = manager.spawn(isolated).await.unwrap();
        let snapshot = manager.await_agent(id).await.unwrap();
        let receipt = snapshot.worktree.unwrap();
        assert!(receipt.branch.starts_with("ca/agent-"));
        assert!(receipt.path.exists());
        assert_eq!(receipt.dirty, Some(false));
        assert_ne!(
            snapshot.result_summary.as_deref(),
            repository.path().canonicalize().unwrap().to_str()
        );
    }

    #[tokio::test]
    async fn agent_spawns_capture_immutable_workspace_snapshots() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let first_workspace = crate::workspace::WorkspaceContext::discover(first.path()).unwrap();
        let second_workspace = crate::workspace::WorkspaceContext::discover(second.path()).unwrap();
        let manager = AgentManager::new(
            Arc::new(WorkspaceRunner),
            AgentLimits {
                max_concurrent: 2,
                ..AgentLimits::default()
            },
        );
        let turn = Uuid::new_v4();
        let mut before_attach = request("workspace", turn);
        before_attach.workspace = first_workspace;
        let first_id = manager.spawn(before_attach).await.unwrap();

        let mut after_attach = request("workspace", turn);
        after_attach.workspace = second_workspace;
        let second_id = manager.spawn(after_attach).await.unwrap();

        let first_result = manager.await_agent(first_id).await.unwrap();
        let second_result = manager.await_agent(second_id).await.unwrap();
        assert_eq!(
            first_result.result_summary.as_deref(),
            first.path().canonicalize().unwrap().to_str()
        );
        assert_eq!(
            second_result.result_summary.as_deref(),
            second.path().canonicalize().unwrap().to_str()
        );
    }

    #[tokio::test]
    async fn provider_runner_keeps_routed_ordinary_children_as_subagents() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let body = concat!(
                "data: {\"choices\":[{\"delta\":{\"content\":\"done\"},\"finish_reason\":null}]}\n\n",
                "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":8,\"completion_tokens\":1,\"total_tokens\":9}}\n\n",
                "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
                "data: [DONE]\n\n"
            );
            for _ in 0..2 {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = vec![0_u8; 16 * 1024];
                let _ = socket.read(&mut request).await.unwrap();
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                socket.write_all(response.as_bytes()).await.unwrap();
            }
        });
        let temp = tempfile::tempdir().unwrap();
        let workspace = crate::workspace::WorkspaceContext::discover(temp.path()).unwrap();
        let ledger = Arc::new(std::sync::Mutex::new(crate::usage::UsageLedger::default()));
        let request_store = crate::request::RequestStore::default();
        let route = AgentRoute {
            provider: "routed".into(),
            model: "routed-model".into(),
        };
        let route_provider = crate::config::ProviderConfig {
            google: None,
            tool_schema_policy: Default::default(),
            base_url: format!("http://{address}/v1"),
            api_key: String::new(),
            protocol: crate::agent::ProviderProtocol::default(),
            prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
            models: vec![route.model.clone()],
            timeout: crate::config::ProviderTimeoutConfig::default(),
        };
        let runner = ProviderAgentRunner {
            client: crate::agent::AgentClient::with_request_store(
                "local",
                &crate::config::ProviderConfig {
                    google: None,
                    tool_schema_policy: Default::default(),
                    base_url: format!("http://{address}/v1"),
                    api_key: String::new(),
                    protocol: crate::agent::ProviderProtocol::default(),
                    prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
                    models: vec!["model".into()],
                    timeout: crate::config::ProviderTimeoutConfig::default(),
                },
                "model",
                request_store.clone(),
            ),
            route_clients: AgentRouteClients::new([(
                route.clone(),
                route_provider,
                request_store.clone(),
                false,
            )]),
            sandbox_mode: crate::sandbox::SandboxMode::PolicyOnly,
            command_timeout_secs: 1,
            approval_rules: Vec::new(),
            permission_rules: Vec::new(),
            session_grants: Arc::new(std::sync::Mutex::new(
                crate::permission::SessionGrantStore::default(),
            )),
            skills: Arc::new(std::sync::Mutex::new(crate::skills::SkillRegistry::empty())),
            task_skill_snapshot: Arc::new(std::sync::Mutex::new(String::new())),
            manager_link: Arc::new(std::sync::OnceLock::new()),
            usage_recorder: crate::usage::UsageRecorder::new(
                ledger.clone(),
                crate::usage::UsageKind::Subagent,
                "local",
                "model",
            ),
            tool_policy: crate::tool_selection::ToolSelectionPolicy::replacing(&["write_file"]),
            skills_enabled: true,
            images_supported: false,
            pricing: Arc::new(std::collections::HashMap::new()),
        };
        let (_command_tx, commands) = mpsc::channel(1);
        let (_cancel_tx, cancelled) = watch::channel(false);
        let (events, _) = mpsc::unbounded_channel();
        let mut agent_request = request("answer", Uuid::new_v4());
        agent_request.grants.tools = vec!["read_file".into()];
        agent_request.workspace = workspace.clone();
        let context = AgentRunContext {
            id: AgentId::new(),
            request: agent_request,
            limits: AgentLimits::default(),
            commands,
            pending_steers: VecDeque::new(),
            cancelled,
            events,
            job_owner: crate::jobs::JobOwner::new(None),
            graph_budget: None,
            frozen_policy: None,
        };

        assert_eq!(runner.run(context).await.unwrap(), "done");
        let snapshot = request_store.snapshot().unwrap();
        assert_eq!(snapshot.kind, crate::request::RequestKind::Subagent);
        assert_eq!(snapshot.provider, "local");
        assert!(snapshot.schemas.is_empty());

        let (_command_tx, commands) = mpsc::channel(1);
        let (_cancel_tx, cancelled) = watch::channel(false);
        let (events, _) = mpsc::unbounded_channel();
        let mut routed_request = request("answer on the selected route", Uuid::new_v4());
        routed_request.route = Some(route);
        routed_request.purpose = AgentRunPurpose::Subagent;
        routed_request.workspace = workspace;
        assert_eq!(
            runner
                .run(AgentRunContext {
                    id: AgentId::new(),
                    request: routed_request,
                    limits: AgentLimits::default(),
                    commands,
                    pending_steers: VecDeque::new(),
                    cancelled,
                    events,
                    job_owner: crate::jobs::JobOwner::new(None),
                    graph_budget: None,
                    frozen_policy: None,
                })
                .await
                .unwrap(),
            "done"
        );
        server.await.unwrap();
        let records = ledger.lock().unwrap().snapshot().records;
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].kind, crate::usage::UsageKind::Subagent);
        assert_eq!(records[1].kind, crate::usage::UsageKind::Subagent);
        assert!(records
            .iter()
            .all(|record| record.outcome == crate::usage::UsageOutcome::Completed));
        assert!(records
            .iter()
            .all(|record| record.usage.as_ref().unwrap().total_tokens == 9));
        let snapshot = request_store.snapshot().unwrap();
        assert_eq!(snapshot.kind, crate::request::RequestKind::Subagent);
        assert_eq!(snapshot.provider, "routed");
        assert_eq!(snapshot.model, "routed-model");
    }

    #[tokio::test]
    async fn missing_request_local_route_fails_before_provider_traffic_or_usage() {
        let ledger = Arc::new(std::sync::Mutex::new(crate::usage::UsageLedger::default()));
        let request_store = crate::request::RequestStore::default();
        let runner = ProviderAgentRunner {
            client: crate::agent::AgentClient::with_request_store(
                "default",
                &crate::config::ProviderConfig {
                    google: None,
                    tool_schema_policy: Default::default(),
                    base_url: "http://127.0.0.1:1/v1".into(),
                    api_key: "not-used".into(),
                    protocol: crate::agent::ProviderProtocol::default(),
                    prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
                    models: vec!["default-model".into()],
                    timeout: crate::config::ProviderTimeoutConfig::default(),
                },
                "default-model",
                request_store.clone(),
            ),
            route_clients: AgentRouteClients::default(),
            sandbox_mode: crate::sandbox::SandboxMode::PolicyOnly,
            command_timeout_secs: 1,
            approval_rules: Vec::new(),
            permission_rules: Vec::new(),
            session_grants: Arc::new(std::sync::Mutex::new(
                crate::permission::SessionGrantStore::default(),
            )),
            skills: Arc::new(std::sync::Mutex::new(crate::skills::SkillRegistry::empty())),
            task_skill_snapshot: Arc::new(std::sync::Mutex::new(String::new())),
            manager_link: Arc::new(std::sync::OnceLock::new()),
            usage_recorder: crate::usage::UsageRecorder::new(
                ledger.clone(),
                crate::usage::UsageKind::Subagent,
                "default",
                "default-model",
            ),
            tool_policy: crate::tool_selection::ToolSelectionPolicy::default(),
            skills_enabled: false,
            images_supported: false,
            pricing: Arc::new(std::collections::HashMap::new()),
        };
        let (_command_tx, commands) = mpsc::channel(1);
        let (_cancel_tx, cancelled) = watch::channel(false);
        let (events, _) = mpsc::unbounded_channel();
        let mut agent_request = request("answer", Uuid::new_v4());
        agent_request.route = Some(AgentRoute {
            provider: "missing".into(),
            model: "missing-model".into(),
        });
        let error = runner
            .run(AgentRunContext {
                id: AgentId::new(),
                request: agent_request,
                limits: AgentLimits::default(),
                commands,
                pending_steers: VecDeque::new(),
                cancelled,
                events,
                job_owner: crate::jobs::JobOwner::new(None),
                graph_budget: None,
                frozen_policy: None,
            })
            .await
            .unwrap_err();
        assert_eq!(
            error,
            "subagent route is unavailable: missing/missing-model"
        );
        assert!(ledger.lock().unwrap().snapshot().records.is_empty());
        assert!(request_store.snapshot().is_none());
    }

    struct FixedRuntimeResolver {
        calls: std::sync::atomic::AtomicUsize,
    }

    #[async_trait]
    impl AgentRuntimeClientResolver for FixedRuntimeResolver {
        async fn resolve(
            &self,
            _provider_id: &str,
            mut provider: crate::config::ProviderConfig,
        ) -> Result<crate::config::ProviderConfig, String> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
            provider.api_key = "runtime-secret".into();
            Ok(provider)
        }
    }

    struct FailingRuntimeResolver;

    #[async_trait]
    impl AgentRuntimeClientResolver for FailingRuntimeResolver {
        async fn resolve(
            &self,
            _provider_id: &str,
            _provider: crate::config::ProviderConfig,
        ) -> Result<crate::config::ProviderConfig, String> {
            Err("runtime credential unavailable".into())
        }
    }

    struct SlowRuntimeResolver {
        active: std::sync::atomic::AtomicUsize,
        maximum: std::sync::atomic::AtomicUsize,
    }

    #[async_trait]
    impl AgentRuntimeClientResolver for SlowRuntimeResolver {
        async fn resolve(
            &self,
            _provider_id: &str,
            provider: crate::config::ProviderConfig,
        ) -> Result<crate::config::ProviderConfig, String> {
            let active = self
                .active
                .fetch_add(1, std::sync::atomic::Ordering::AcqRel)
                + 1;
            self.maximum
                .fetch_max(active, std::sync::atomic::Ordering::AcqRel);
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            self.active
                .fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
            Ok(provider)
        }
    }

    #[tokio::test]
    async fn routed_codex_child_resolves_runtime_credential_before_provider_traffic() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = vec![0_u8; 16 * 1024];
            let count = socket.read(&mut request).await.unwrap();
            let request = String::from_utf8_lossy(&request[..count]).to_ascii_lowercase();
            assert!(request.contains("authorization: bearer runtime-secret\r\n"));
            let body = concat!(
                "data: {\"choices\":[{\"delta\":{\"content\":\"done\"},\"finish_reason\":null}]}\n\n",
                "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":8,\"completion_tokens\":1,\"total_tokens\":9}}\n\n",
                "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
                "data: [DONE]\n\n"
            );
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            socket.write_all(response.as_bytes()).await.unwrap();
        });
        let route = AgentRoute {
            provider: "openai-codex".into(),
            model: "gpt-5.6-sol".into(),
        };
        let request_store = crate::request::RequestStore::default();
        let route_provider = crate::config::ProviderConfig {
            google: None,
            tool_schema_policy: Default::default(),
            base_url: format!("http://{address}/v1"),
            api_key: String::new(),
            protocol: crate::agent::ProviderProtocol::default(),
            prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
            models: vec![route.model.clone()],
            timeout: crate::config::ProviderTimeoutConfig::default(),
        };
        let resolver = Arc::new(FixedRuntimeResolver {
            calls: std::sync::atomic::AtomicUsize::new(0),
        });
        let ledger = Arc::new(std::sync::Mutex::new(crate::usage::UsageLedger::default()));
        let runner = ProviderAgentRunner {
            client: crate::agent::AgentClient::with_request_store(
                "deepseek",
                &route_provider,
                "deepseek-parent",
                request_store.clone(),
            ),
            route_clients: AgentRouteClients::with_runtime_resolver(
                [(route.clone(), route_provider, request_store.clone(), false)],
                resolver.clone(),
            ),
            sandbox_mode: crate::sandbox::SandboxMode::PolicyOnly,
            command_timeout_secs: 1,
            approval_rules: Vec::new(),
            permission_rules: Vec::new(),
            session_grants: Arc::new(std::sync::Mutex::new(
                crate::permission::SessionGrantStore::default(),
            )),
            skills: Arc::new(std::sync::Mutex::new(crate::skills::SkillRegistry::empty())),
            task_skill_snapshot: Arc::new(std::sync::Mutex::new(String::new())),
            manager_link: Arc::new(std::sync::OnceLock::new()),
            usage_recorder: crate::usage::UsageRecorder::new(
                ledger.clone(),
                crate::usage::UsageKind::Subagent,
                "deepseek",
                "deepseek-parent",
            ),
            tool_policy: crate::tool_selection::ToolSelectionPolicy::default(),
            skills_enabled: false,
            images_supported: false,
            pricing: Arc::new(std::collections::HashMap::new()),
        };
        let (_command_tx, commands) = mpsc::channel(1);
        let (_cancel_tx, cancelled) = watch::channel(false);
        let (events, _) = mpsc::unbounded_channel();
        let mut agent_request = request("answer", Uuid::new_v4());
        agent_request.route = Some(route);
        let result = runner
            .run(AgentRunContext {
                id: AgentId::new(),
                request: agent_request,
                limits: AgentLimits::default(),
                commands,
                pending_steers: VecDeque::new(),
                cancelled,
                events,
                job_owner: crate::jobs::JobOwner::new(None),
                graph_budget: None,
                frozen_policy: None,
            })
            .await
            .unwrap();
        assert_eq!(result, "done");
        assert_eq!(resolver.calls.load(std::sync::atomic::Ordering::Acquire), 1);
        assert!(!format!("{:?}", runner.route_clients).contains("runtime-secret"));
        assert!(!format!("{:?}", request_store.snapshot()).contains("runtime-secret"));
        let prepared = request_store.snapshot().unwrap();
        assert_eq!(prepared.provider, "openai-codex");
        assert_eq!(prepared.model, "gpt-5.6-sol");
        let recorded = ledger.lock().unwrap().snapshot().records;
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].provider, "openai-codex");
        assert_eq!(recorded[0].model, "gpt-5.6-sol");
        server.await.unwrap();
    }

    #[tokio::test]
    async fn runtime_credential_failure_stops_before_provider_request_and_usage() {
        let route = AgentRoute {
            provider: "openai-codex".into(),
            model: "gpt-5.6-sol".into(),
        };
        let request_store = crate::request::RequestStore::default();
        let route_provider = crate::config::ProviderConfig {
            google: None,
            tool_schema_policy: Default::default(),
            base_url: "http://127.0.0.1:1/v1".into(),
            api_key: String::new(),
            protocol: crate::agent::ProviderProtocol::default(),
            prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
            models: vec![route.model.clone()],
            timeout: crate::config::ProviderTimeoutConfig::default(),
        };
        let ledger = Arc::new(std::sync::Mutex::new(crate::usage::UsageLedger::default()));
        let runner = ProviderAgentRunner {
            client: crate::agent::AgentClient::with_request_store(
                &route.provider,
                &route_provider,
                &route.model,
                request_store.clone(),
            ),
            route_clients: AgentRouteClients::with_runtime_resolver(
                [(route.clone(), route_provider, request_store.clone(), false)],
                Arc::new(FailingRuntimeResolver),
            ),
            sandbox_mode: crate::sandbox::SandboxMode::PolicyOnly,
            command_timeout_secs: 1,
            approval_rules: Vec::new(),
            permission_rules: Vec::new(),
            session_grants: Arc::new(std::sync::Mutex::new(
                crate::permission::SessionGrantStore::default(),
            )),
            skills: Arc::new(std::sync::Mutex::new(crate::skills::SkillRegistry::empty())),
            task_skill_snapshot: Arc::new(std::sync::Mutex::new(String::new())),
            manager_link: Arc::new(std::sync::OnceLock::new()),
            usage_recorder: crate::usage::UsageRecorder::new(
                ledger.clone(),
                crate::usage::UsageKind::Subagent,
                &route.provider,
                &route.model,
            ),
            tool_policy: crate::tool_selection::ToolSelectionPolicy::default(),
            skills_enabled: false,
            images_supported: false,
            pricing: Arc::new(std::collections::HashMap::new()),
        };
        let (_command_tx, commands) = mpsc::channel(1);
        let (_cancel_tx, cancelled) = watch::channel(false);
        let (events, _) = mpsc::unbounded_channel();
        let mut agent_request = request("answer", Uuid::new_v4());
        agent_request.route = Some(route);
        let error = runner
            .run(AgentRunContext {
                id: AgentId::new(),
                request: agent_request,
                limits: AgentLimits::default(),
                commands,
                pending_steers: VecDeque::new(),
                cancelled,
                events,
                job_owner: crate::jobs::JobOwner::new(None),
                graph_budget: None,
                frozen_policy: None,
            })
            .await
            .unwrap_err();
        assert_eq!(error, "runtime credential unavailable");
        assert!(request_store.snapshot().is_none());
        assert!(ledger.lock().unwrap().snapshot().records.is_empty());
    }

    #[tokio::test]
    async fn route_runtime_resolution_is_serialized_for_concurrent_children() {
        let route = AgentRoute {
            provider: "openai-codex".into(),
            model: "gpt-5.6-sol".into(),
        };
        let provider = crate::config::ProviderConfig {
            google: None,
            tool_schema_policy: Default::default(),
            base_url: "http://127.0.0.1:1/v1".into(),
            api_key: "test".into(),
            protocol: crate::agent::ProviderProtocol::default(),
            prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
            models: vec![route.model.clone()],
            timeout: crate::config::ProviderTimeoutConfig::default(),
        };
        let resolver = Arc::new(SlowRuntimeResolver {
            active: std::sync::atomic::AtomicUsize::new(0),
            maximum: std::sync::atomic::AtomicUsize::new(0),
        });
        let clients = AgentRouteClients::with_runtime_resolver(
            [(
                route.clone(),
                provider,
                crate::request::RequestStore::default(),
                false,
            )],
            resolver.clone(),
        );
        let (left, right) = tokio::join!(clients.resolve(&route), clients.resolve(&route));
        assert!(left.unwrap().is_some());
        assert!(right.unwrap().is_some());
        assert_eq!(
            resolver.maximum.load(std::sync::atomic::Ordering::Acquire),
            1
        );
    }

    #[tokio::test]
    async fn non_codex_routes_bypass_runtime_credential_resolution() {
        let route = AgentRoute {
            provider: "test".into(),
            model: "model".into(),
        };
        let provider = crate::config::ProviderConfig {
            google: None,
            tool_schema_policy: Default::default(),
            base_url: "http://127.0.0.1:1/v1".into(),
            api_key: "test".into(),
            protocol: crate::agent::ProviderProtocol::default(),
            prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
            models: vec![route.model.clone()],
            timeout: crate::config::ProviderTimeoutConfig::default(),
        };
        let resolver = Arc::new(FixedRuntimeResolver {
            calls: std::sync::atomic::AtomicUsize::new(0),
        });
        let clients = AgentRouteClients::with_runtime_resolver(
            [(
                route.clone(),
                provider,
                crate::request::RequestStore::default(),
                false,
            )],
            resolver.clone(),
        );
        assert!(clients.resolve(&route).await.unwrap().is_some());
        assert_eq!(resolver.calls.load(std::sync::atomic::Ordering::Acquire), 0);
    }

    #[tokio::test]
    async fn stable_spawn_list_await_message_cancel_and_bounds() {
        let limits = AgentLimits {
            retained_output_bytes: 64,
            ..AgentLimits::default()
        };
        let manager = AgentManager::new(Arc::new(FakeRunner), limits);
        let turn = Uuid::new_v4();
        let done = manager.spawn(request("done", turn)).await.unwrap();
        let snapshot = manager.await_agent(done).await.unwrap();
        assert_eq!(snapshot.status, AgentStatus::Completed);
        assert_eq!(snapshot.result_summary.as_deref(), Some("summary-only"));

        let waiting = manager.spawn(request("wait", turn)).await.unwrap();
        manager.send_message(waiting, "steer".into()).await.unwrap();
        assert_eq!(
            manager
                .await_agent(waiting)
                .await
                .unwrap()
                .result_summary
                .as_deref(),
            Some("steer")
        );
        let cancelled = manager.spawn(request("wait", turn)).await.unwrap();
        manager.cancel(cancelled).await.unwrap();
        assert!(matches!(
            manager.await_agent(cancelled).await.unwrap().status,
            AgentStatus::Failed | AgentStatus::Cancelled
        ));
        let large = manager
            .spawn(request("large", Uuid::new_v4()))
            .await
            .unwrap();
        let snapshot = manager.await_agent(large).await.unwrap();
        assert!(snapshot.recent_output.len() <= 64);
        assert!(snapshot.events.len() <= 128);
    }

    #[tokio::test]
    async fn accepted_steering_is_delivered_or_retained_and_terminal_steering_is_rejected() {
        let manager = AgentManager::new(Arc::new(FakeRunner), AgentLimits::default());
        let turn = Uuid::new_v4();
        let id = manager
            .spawn(request("finish-without-reading", turn))
            .await
            .unwrap();
        manager
            .send_message(id, "change course".into())
            .await
            .unwrap();
        let terminal = manager.await_agent(id).await.unwrap();
        assert!(terminal.status.terminal());
        assert_eq!(terminal.missed_steer, ["change course"]);
        assert!(format_agent_snapshot(&terminal).contains("missed_steer=change course"));
        let error = manager
            .send_message(id, "too late".into())
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("already terminal"));
    }

    #[tokio::test]
    async fn steering_received_during_approval_reaches_the_next_provider_boundary() {
        let (command_tx, commands) = mpsc::channel(4);
        let (_cancel_tx, cancelled) = watch::channel(false);
        let (events, mut event_rx) = mpsc::unbounded_channel();
        let steer_id = Uuid::new_v4();
        command_tx
            .send(AgentCommand::Message {
                id: steer_id,
                text: "use the narrower path".into(),
            })
            .await
            .unwrap();
        command_tx.send(AgentCommand::Approval(true)).await.unwrap();
        let mut context = AgentRunContext {
            id: AgentId::new(),
            request: request("approval", Uuid::new_v4()),
            limits: AgentLimits::default(),
            commands,
            pending_steers: VecDeque::new(),
            cancelled,
            events,
            job_owner: crate::jobs::JobOwner::new(None),
            graph_budget: None,
            frozen_policy: None,
        };
        assert!(await_child_approval(&mut context, "write_file".into())
            .await
            .unwrap());
        assert_eq!(
            context.pending_steers,
            VecDeque::from([(steer_id, "use the narrower path".into())])
        );
        let mut messages = Vec::new();
        drain_child_steering(&mut context, &mut messages);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content.as_str(), Some("use the narrower path"));
        assert!(matches!(
            event_rx.recv().await,
            Some(RunnerEvent::ApprovalRequired(_))
        ));
        assert!(matches!(
            event_rx.recv().await,
            Some(RunnerEvent::Activity(_))
        ));
        assert!(matches!(
            event_rx.recv().await,
            Some(RunnerEvent::SteerDelivered { id }) if id == steer_id
        ));
    }

    #[tokio::test]
    async fn graph_clarification_answers_bounds_and_declines() {
        use crate::agent::registry::arguments::RequestGraphClarificationArgs;
        let spawn_context =
            |commands: mpsc::Receiver<AgentCommand>, events: mpsc::UnboundedSender<RunnerEvent>| {
                let (cancel_tx, cancelled) = watch::channel(false);
                let budget = crate::agent_graph::AgentAutonomyBudget::new(
                    crate::agent_graph::AgentGraphBudgetLimitsV1::from_input(
                        crate::agent_graph::AgentGraphBudgetInput {
                            max_cost_usd: None,
                            allow_unpriced: true,
                            max_physical_requests:
                                crate::agent_graph::DEFAULT_GRAPH_PHYSICAL_REQUESTS,
                        },
                    )
                    .unwrap(),
                    crate::agent_graph::AgentGraphBudgetUsageV1::default(),
                )
                .unwrap();
                (
                    AgentRunContext {
                        id: AgentId::new(),
                        request: request("clarification", Uuid::new_v4()),
                        limits: AgentLimits::default(),
                        commands,
                        pending_steers: VecDeque::new(),
                        cancelled,
                        events,
                        job_owner: crate::jobs::JobOwner::new(None),
                        graph_budget: Some(budget),
                        frozen_policy: None,
                    },
                    cancel_tx,
                )
            };

        // The coordinator's answer becomes the tool result.
        let (command_tx, commands) = mpsc::channel(4);
        let (events, mut event_rx) = mpsc::unbounded_channel();
        let steer_id = Uuid::new_v4();
        command_tx
            .send(AgentCommand::Message {
                id: steer_id,
                text: "use the wider path".into(),
            })
            .await
            .unwrap();
        let (mut context, _cancel_alive) = spawn_context(commands, events);
        let result = await_graph_clarification(
            &mut context,
            RequestGraphClarificationArgs {
                question: "which path?".into(),
                choices: vec!["narrow".into(), "wide".into()],
            },
        )
        .await;
        assert_eq!(
            result.model_text(),
            "use the wider path",
            "{}",
            result.model_text()
        );
        assert!(matches!(
            event_rx.recv().await,
            Some(RunnerEvent::ApprovalRequired(message))
                if message.starts_with("clarification: which path?")
        ));
        assert!(matches!(
            event_rx.recv().await,
            Some(RunnerEvent::SteerDelivered { id }) if id == steer_id
        ));

        // Out-of-bounds questions fail closed without parking.
        let (_command_tx, commands) = mpsc::channel(4);
        let (events, _event_rx) = mpsc::unbounded_channel();
        let (mut context, _cancel_alive) = spawn_context(commands, events);
        let result = await_graph_clarification(
            &mut context,
            RequestGraphClarificationArgs {
                question: "q".repeat(MAX_CLARIFICATION_QUESTION_CHARS + 1),
                choices: Vec::new(),
            },
        )
        .await;
        assert!(!result.is_success());

        // Declining closes the channel without an answer.
        let (command_tx, commands) = mpsc::channel(4);
        let (events, _event_rx) = mpsc::unbounded_channel();
        command_tx
            .send(AgentCommand::Approval(false))
            .await
            .unwrap();
        let (mut context, _cancel_alive) = spawn_context(commands, events);
        let result = await_graph_clarification(
            &mut context,
            RequestGraphClarificationArgs {
                question: "may I proceed?".into(),
                choices: Vec::new(),
            },
        )
        .await;
        assert!(matches!(
            result,
            crate::agent::registry::ToolExecutionResult::Denied(_)
        ));

        // An oversized or control-bearing answer is rejected.
        let (command_tx, commands) = mpsc::channel(4);
        let (events, _event_rx) = mpsc::unbounded_channel();
        command_tx
            .send(AgentCommand::Message {
                id: Uuid::new_v4(),
                text: "a\u{1b}b".to_string(),
            })
            .await
            .unwrap();
        let (mut context, _cancel_alive) = spawn_context(commands, events);
        let result = await_graph_clarification(
            &mut context,
            RequestGraphClarificationArgs {
                question: "next step?".into(),
                choices: Vec::new(),
            },
        )
        .await;
        assert!(!result.is_success());
    }

    #[tokio::test]
    async fn bounded_wait_returns_live_snapshot_and_terminal_handoff_has_stable_digest() {
        let manager = AgentManager::new(Arc::new(FakeRunner), AgentLimits::default());
        let work = Arc::new(std::sync::Mutex::new(crate::work::WorkState::default()));
        let workspace = crate::workspace::WorkspaceContext::discover("/tmp").unwrap();
        let waiting = manager
            .spawn(request("wait", Uuid::new_v4()))
            .await
            .unwrap();
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            execute_manager_tool(
                "await_agent",
                &serde_json::json!({"agent_id": waiting.to_string(), "timeout_secs": 0}),
                &manager,
                &work,
                &workspace,
            ),
        )
        .await
        .expect("bounded await must not block")
        .unwrap();
        assert!(matches!(
            result,
            crate::agent::registry::ToolExecutionResult::Success(ref output)
                if output.contains("Running") || output.contains("Queued")
        ));
        manager.cancel(waiting).await.unwrap();

        let done = manager
            .spawn(request("done", Uuid::new_v4()))
            .await
            .unwrap();
        let first = manager.await_agent(done).await.unwrap();
        let second = manager.snapshot(done).await.unwrap();
        assert_eq!(first.result_sha256, second.result_sha256);
        let digest = first.result_sha256.as_deref().unwrap();
        assert_eq!(digest.len(), 64);
        assert!(digest.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert!(format_agent_snapshot(&first).contains(&format!("sha256={digest}")));

        let failed = manager
            .spawn(request("fail", Uuid::new_v4()))
            .await
            .unwrap();
        let failed = manager.await_agent(failed).await.unwrap();
        assert_eq!(failed.status, AgentStatus::Failed);
        assert!(failed.result_sha256.is_some());
    }

    #[tokio::test]
    async fn activity_watch_is_structured_bounded_sanitized_and_origin_typed() {
        let manager = AgentManager::new(Arc::new(FakeRunner), AgentLimits::default());
        let mut activity = manager.subscribe_activity();
        let turn = Uuid::new_v4();
        let id = manager.spawn(request("activity", turn)).await.unwrap();
        let terminal = manager.await_agent(id).await.unwrap();
        assert_eq!(terminal.status, AgentStatus::Completed);
        tokio::time::timeout(std::time::Duration::from_secs(1), activity.changed())
            .await
            .unwrap()
            .unwrap();
        let rows = activity.borrow_and_update().clone();
        let row = rows.iter().find(|row| row.id == id).unwrap();
        assert_eq!(row.parent_turn, turn);
        assert_eq!(row.origin, AgentRunOrigin::Conversation);
        assert_eq!(row.purpose, AgentRunPurpose::Subagent);
        assert_eq!(row.status, AgentStatus::Completed);
        assert!(row.current_action.is_none());
        assert!(row
            .events
            .iter()
            .any(|event| event.message == "started read_file"));
        assert!(row
            .events
            .iter()
            .any(|event| event.message == "completed read_file"));
        assert!(!row.recent_output.contains('\u{1b}'));
        assert!(row.events.len() <= 32);
        assert!(row.recent_output.len() <= 16 * 1024);
    }

    #[tokio::test]
    async fn approval_pauses_one_child_and_depth_children_limits_fail_closed() {
        let limits = AgentLimits {
            max_children_per_turn: 1,
            max_depth: 1,
            ..AgentLimits::default()
        };
        let manager = AgentManager::new(Arc::new(FakeRunner), limits);
        let turn = Uuid::new_v4();
        let id = manager.spawn(request("approval", turn)).await.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            loop {
                if manager.snapshot(id).await.unwrap().status == AgentStatus::WaitingApproval {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            manager.spawn(request("done", turn)).await.unwrap_err(),
            AgentManagerError::ChildrenPerTurn(1)
        );
        let mut too_deep = request("done", Uuid::new_v4());
        too_deep.depth = 2;
        assert_eq!(
            manager.spawn(too_deep).await.unwrap_err(),
            AgentManagerError::Depth { depth: 2, limit: 1 }
        );
        manager.resolve_approval(id, true).await.unwrap();
        assert_eq!(
            manager.await_agent(id).await.unwrap().status,
            AgentStatus::Completed
        );
    }

    #[tokio::test]
    async fn concurrency_timeout_and_shutdown_are_bounded() {
        let limits = AgentLimits {
            max_concurrent: 1,
            runtime: std::time::Duration::from_millis(20),
            ..AgentLimits::default()
        };
        let manager = AgentManager::new(Arc::new(FakeRunner), limits);
        let first = manager
            .spawn(request("wait", Uuid::new_v4()))
            .await
            .unwrap();
        let second = manager
            .spawn(request("wait", Uuid::new_v4()))
            .await
            .unwrap();
        assert_eq!(
            manager.await_agent(first).await.unwrap().status,
            AgentStatus::TimedOut
        );
        manager.shutdown().await;
        assert!(manager.await_agent(second).await.unwrap().status.terminal());
    }

    #[tokio::test]
    async fn typed_manager_tools_spawn_list_message_and_reject_grant_widening() {
        let manager = AgentManager::new(Arc::new(FakeRunner), AgentLimits::default());
        let work = Arc::new(std::sync::Mutex::new(crate::work::WorkState::default()));
        let workspace = crate::workspace::WorkspaceContext::discover("/tmp").unwrap();
        let invalid = execute_manager_tool(
            "spawn_agent",
            &serde_json::json!({"task":"done", "tools":["cancel_agent"]}),
            &manager,
            &work,
            &workspace,
        )
        .await
        .unwrap();
        assert!(!invalid.is_success());

        let spawned = execute_manager_tool(
            "spawn_agent",
            &serde_json::json!({"task":"wait", "tools":["read_file"]}),
            &manager,
            &work,
            &workspace,
        )
        .await
        .unwrap();
        assert!(spawned.is_success());
        let snapshot = manager.list().await.remove(0);
        let id = snapshot.id;
        assert_eq!(
            snapshot.grants.path_scopes,
            workspace
                .roots()
                .iter()
                .map(|root| root.path.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
        );
        let sent = execute_manager_tool(
            "send_agent_message",
            &serde_json::json!({"agent_id":id.to_string(), "message":"finish"}),
            &manager,
            &work,
            &workspace,
        )
        .await
        .unwrap();
        assert!(sent.is_success());
        assert_eq!(
            manager.await_agent(id).await.unwrap().status,
            AgentStatus::Completed
        );
        let listed = execute_manager_tool(
            "list_agents",
            &serde_json::json!({}),
            &manager,
            &work,
            &workspace,
        )
        .await
        .unwrap();
        assert!(listed.model_text().contains(&id.to_string()));
    }

    fn learned_allowlist_test_router() -> crate::pricing::DelegationRouter {
        let mut config = crate::config::CaConfig::default();
        config.providers.clear();
        for (provider, base_url, models) in [
            (
                "openai",
                "https://api.openai.com/v1",
                vec!["gpt-5.6-sol", "gpt-5.6-luna"],
            ),
            (
                "deepseek",
                "https://api.deepseek.com/v1",
                vec!["deepseek-v4-pro"],
            ),
        ] {
            config.providers.insert(
                provider.into(),
                crate::config::ProviderConfig {
                    google: None,
                    tool_schema_policy: Default::default(),
                    base_url: base_url.into(),
                    api_key: "fixture-key".into(),
                    protocol: crate::agent::ProviderProtocol::ChatCompletions,
                    prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
                    models: models.into_iter().map(str::to_string).collect(),
                    timeout: crate::config::ProviderTimeoutConfig::default(),
                },
            );
        }
        crate::pricing::DelegationRouter::from_config(
            &config,
            &crate::pricing::OfficialPriceCatalog::seeded(),
            chrono::DateTime::parse_from_rfc3339("2026-08-15T12:00:00Z")
                .unwrap()
                .to_utc(),
        )
    }

    #[test]
    fn learned_route_ranking_selects_a_qualifying_allowed_route() {
        let router = learned_allowlist_test_router();
        let globally_cheapest = router
            .select(crate::pricing::Complexity::Standard, "inspect", "bounded")
            .unwrap();
        assert_eq!(globally_cheapest.provider, "deepseek");
        assert_eq!(globally_cheapest.model, "deepseek-v4-pro");
        let manager = AgentManager::new(Arc::new(FakeRunner), AgentLimits::default());
        manager.replace_delegation_router(Some(router));
        let allowed = AgentRoute {
            provider: "openai".into(),
            model: "gpt-5.6-sol".into(),
        };
        manager.configure_learned_delegation(true, [allowed.clone()]);

        let selected = manager
            .select_delegation_route(crate::pricing::Complexity::Standard, "inspect", "bounded")
            .expect("the eligible allowed route must survive a cheaper disallowed route");
        assert_eq!(selected.provider, allowed.provider);
        assert_eq!(selected.model, allowed.model);
    }

    #[test]
    fn learned_route_ranking_preserves_capability_and_non_learned_behavior() {
        let manager = AgentManager::new(Arc::new(FakeRunner), AgentLimits::default());
        manager.replace_delegation_router(Some(learned_allowlist_test_router()));
        let simple_only = AgentRoute {
            provider: "openai".into(),
            model: "gpt-5.6-luna".into(),
        };
        manager.configure_learned_delegation(true, [simple_only.clone()]);
        assert!(manager
            .select_delegation_route(crate::pricing::Complexity::Standard, "inspect", "bounded")
            .is_err());
        let simple = manager
            .select_delegation_route(crate::pricing::Complexity::Simple, "inspect", "bounded")
            .unwrap();
        assert_eq!(simple.provider, simple_only.provider);
        assert_eq!(simple.model, simple_only.model);

        manager.configure_learned_delegation(false, [simple_only]);
        let unrestricted = manager
            .select_delegation_route(crate::pricing::Complexity::Standard, "inspect", "bounded")
            .unwrap();
        assert_eq!(unrestricted.provider, "deepseek");
        assert_eq!(unrestricted.model, "deepseek-v4-pro");

        manager.configure_learned_delegation(true, []);
        let empty_allowlist = manager
            .select_delegation_route(crate::pricing::Complexity::Standard, "inspect", "bounded")
            .unwrap();
        assert_eq!(empty_allowlist, unrestricted);

        manager.configure_learned_delegation(
            true,
            [AgentRoute {
                provider: "openai".into(),
                model: "unpriced-model".into(),
            }],
        );
        let no_candidate = manager
            .select_delegation_route(crate::pricing::Complexity::Standard, "inspect", "bounded")
            .unwrap_err();
        assert!(no_candidate.contains("no automatic delegation route qualifies"));
    }

    #[test]
    fn learned_route_ranking_does_not_override_exact_route_pins() {
        let manager = AgentManager::new(
            Arc::new(RecordingRouteRunner {
                routes: Arc::new(std::sync::Mutex::new(Vec::new())),
            }),
            AgentLimits::default(),
        );
        manager.replace_delegation_router(Some(crate::pricing::DelegationRouter::default()));
        manager.configure_learned_delegation(
            true,
            [AgentRoute {
                provider: "deepseek".into(),
                model: "deepseek-parent".into(),
            }],
        );
        let (route, selection) = resolve_child_route(
            &manager,
            &json!({"route":{"provider":"openai-codex", "model":"gpt-child"}}),
            "inspect",
            "bounded",
            None,
            true,
        )
        .unwrap();
        assert_eq!(
            route,
            Some(AgentRoute {
                provider: "openai-codex".into(),
                model: "gpt-child".into(),
            })
        );
        assert!(selection.is_none());
    }

    #[tokio::test]
    async fn exact_cross_provider_route_is_honored_before_cost_routing() {
        let routes = Arc::new(std::sync::Mutex::new(Vec::new()));
        let manager = AgentManager::new(
            Arc::new(RecordingRouteRunner {
                routes: routes.clone(),
            }),
            AgentLimits::default(),
        );
        manager.replace_parent_route(Some(AgentRoute {
            provider: "deepseek".into(),
            model: "deepseek-parent".into(),
        }));
        manager.replace_delegation_router(Some(crate::pricing::DelegationRouter::default()));
        let work = Arc::new(std::sync::Mutex::new(crate::work::WorkState::default()));
        let workspace = crate::workspace::WorkspaceContext::discover("/tmp").unwrap();
        let result = execute_manager_tool(
            "spawn_agent",
            &json!({"task":"inspect", "tools":["read_file"],
                "route":{"provider":"openai-codex", "model":"gpt-child"}}),
            &manager,
            &work,
            &workspace,
        )
        .await
        .unwrap();
        assert!(result.is_success(), "{}", result.model_text());
        let child = manager.list().await.remove(0);
        let child = manager.await_agent(child.id).await.unwrap();
        assert_eq!(
            routes.lock().unwrap().as_slice(),
            [Some(AgentRoute {
                provider: "openai-codex".into(),
                model: "gpt-child".into(),
            })]
        );
        assert!(format_agent_snapshot(&child).contains("route=openai-codex/gpt-child"));
    }

    #[tokio::test]
    async fn invalid_exact_routes_and_conflicts_fail_before_child_admission() {
        let manager = AgentManager::new(Arc::new(FakeRunner), AgentLimits::default());
        manager.replace_parent_route(Some(AgentRoute {
            provider: "deepseek".into(),
            model: "deepseek-parent".into(),
        }));
        let work = Arc::new(std::sync::Mutex::new(crate::work::WorkState::default()));
        let workspace = crate::workspace::WorkspaceContext::discover("/tmp").unwrap();
        for arguments in [
            json!({"task":"inspect", "tools":["read_file"],
                "route":{"provider":"missing", "model":"missing"}}),
            json!({"task":"inspect", "tools":["read_file"],
                "route":{"provider":"deepseek", "model":""}}),
            json!({"task":"inspect", "tools":["read_file"],
                "route":{"provider":"deepseek", "model":"deepseek-parent", "unexpected":true}}),
            json!({"task":"inspect", "tools":["read_file"], "complexity":"simple",
                "route":{"provider":"deepseek", "model":"deepseek-parent"}}),
        ] {
            let result =
                execute_manager_tool("spawn_agent", &arguments, &manager, &work, &workspace)
                    .await
                    .unwrap();
            assert!(!result.is_success(), "{}", result.model_text());
            assert!(manager.list().await.is_empty());
        }
        let batch = execute_manager_tool(
            "spawn_agent_batch",
            &json!({"tasks":[
                {"task":"valid", "tools":["read_file"]},
                {"task":"invalid", "tools":["read_file"],
                    "route":{"provider":"missing", "model":"missing"}}
            ]}),
            &manager,
            &work,
            &workspace,
        )
        .await
        .unwrap();
        assert!(!batch.is_success());
        assert!(manager.list().await.is_empty());
    }

    #[tokio::test]
    async fn exact_route_task_and_nested_children_preserve_route_ownership() {
        let routes = Arc::new(std::sync::Mutex::new(Vec::new()));
        let manager = AgentManager::new(
            Arc::new(RecordingRouteRunner {
                routes: routes.clone(),
            }),
            AgentLimits::default(),
        );
        let deepseek = AgentRoute {
            provider: "deepseek".into(),
            model: "deepseek-parent".into(),
        };
        let codex = AgentRoute {
            provider: "openai-codex".into(),
            model: "gpt-child".into(),
        };
        manager.replace_parent_route(Some(deepseek.clone()));
        let work = Arc::new(std::sync::Mutex::new(crate::work::WorkState::default()));
        let workspace = crate::workspace::WorkspaceContext::discover("/tmp").unwrap();
        let task = execute_manager_tool(
            "task",
            &json!({
                "subagent_name":"explore", "prompt":"inspect", "route":codex
            }),
            &manager,
            &work,
            &workspace,
        )
        .await
        .unwrap();
        assert!(task.is_success(), "{}", task.model_text());
        assert!(task.model_text().contains("route=openai-codex/gpt-child"));

        let link = std::sync::OnceLock::new();
        link.set(Arc::downgrade(&manager)).unwrap();
        let (_commands_tx, commands) = mpsc::channel(1);
        let (_cancel_tx, cancelled) = watch::channel(false);
        let (events, _) = mpsc::unbounded_channel();
        let mut parent = request("parent", manager.parent_turn());
        parent.route = Some(codex.clone());
        parent.grants.tools.push("spawn_agent".into());
        let context = AgentRunContext {
            id: AgentId::new(),
            request: parent,
            limits: AgentLimits::default(),
            commands,
            pending_steers: VecDeque::new(),
            cancelled,
            events,
            job_owner: crate::jobs::JobOwner::new(None),
            graph_budget: None,
            frozen_policy: None,
        };
        assert!(execute_nested_spawn(
            &context,
            &json!({"task":"inherited", "tools":["read_file"]}),
            &link
        )
        .await
        .is_success());
        assert!(execute_nested_spawn(
            &context,
            &json!({"task":"exact", "tools":["read_file"], "route":deepseek}),
            &link
        )
        .await
        .is_success());
        let count = manager.list().await.len();
        assert!(!execute_nested_spawn(
            &context,
            &json!({"task":"invalid", "tools":["read_file"],
            "route":{"provider":"missing", "model":"missing"}}),
            &link
        )
        .await
        .is_success());
        assert_eq!(manager.list().await.len(), count);
        for child in manager.list().await {
            manager.await_agent(child.id).await.unwrap();
        }
        let mut actual = routes.lock().unwrap().clone();
        actual.sort_by_key(|route| route.as_ref().map(|route| route.provider.clone()));
        assert_eq!(actual, [Some(deepseek), Some(codex.clone()), Some(codex)]);
    }

    #[tokio::test]
    async fn agent_route_discovery_is_sorted_secret_free_and_does_not_authenticate() {
        let resolver = Arc::new(FixedRuntimeResolver {
            calls: std::sync::atomic::AtomicUsize::new(0),
        });
        let mut runner = permission_test_runner();
        let provider = crate::config::ProviderConfig {
            google: None,
            tool_schema_policy: Default::default(),
            base_url: "http://127.0.0.1:1/private-provider-endpoint".into(),
            api_key: "private-test-credential".into(),
            protocol: crate::agent::ProviderProtocol::default(),
            prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
            models: vec!["gpt-child".into()],
            timeout: crate::config::ProviderTimeoutConfig::default(),
        };
        runner.route_clients = AgentRouteClients::with_runtime_resolver(
            [
                (
                    AgentRoute {
                        provider: "openai-codex".into(),
                        model: "gpt-child".into(),
                    },
                    provider.clone(),
                    crate::request::RequestStore::default(),
                    false,
                ),
                (
                    AgentRoute {
                        provider: "deepseek".into(),
                        model: "deepseek-parent".into(),
                    },
                    provider,
                    crate::request::RequestStore::default(),
                    false,
                ),
            ],
            resolver.clone(),
        );
        let manager = AgentManager::new(Arc::new(runner), AgentLimits::default());
        let work = Arc::new(std::sync::Mutex::new(crate::work::WorkState::default()));
        let workspace = crate::workspace::WorkspaceContext::discover("/tmp").unwrap();
        let result =
            execute_manager_tool("list_agent_routes", &json!({}), &manager, &work, &workspace)
                .await
                .unwrap();
        let output = result.model_text();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(output).unwrap(),
            json!({
                "authentication":"not_checked", "routes":[
                    {"provider":"deepseek", "model":"deepseek-parent"},
                    {"provider":"openai-codex", "model":"gpt-child"}
                ]
            })
        );
        assert!(!output.contains("private-"));
        assert_eq!(resolver.calls.load(std::sync::atomic::Ordering::Acquire), 0);
        assert!(manager.list().await.is_empty());
    }

    #[tokio::test]
    async fn omitted_complexity_inherits_parent_but_explicit_unavailable_route_fails_closed() {
        let routes = Arc::new(std::sync::Mutex::new(Vec::new()));
        let manager = AgentManager::new(
            Arc::new(RecordingRouteRunner {
                routes: routes.clone(),
            }),
            AgentLimits::default(),
        );
        let parent = AgentRoute {
            provider: "openai-codex".into(),
            model: "gpt-5.6-sol".into(),
        };
        manager.replace_parent_route(Some(parent.clone()));
        manager.replace_delegation_router(Some(crate::pricing::DelegationRouter::default()));
        let work = Arc::new(std::sync::Mutex::new(crate::work::WorkState::default()));
        let workspace = crate::workspace::WorkspaceContext::discover("/tmp").unwrap();

        let inherited = execute_manager_tool(
            "spawn_agent",
            &serde_json::json!({"task":"inspect", "tools":["read_file"]}),
            &manager,
            &work,
            &workspace,
        )
        .await
        .unwrap();
        assert!(inherited.is_success());
        let inherited_id = manager.list().await[0].id;
        assert_eq!(
            manager.await_agent(inherited_id).await.unwrap().status,
            AgentStatus::Completed
        );
        assert_eq!(
            routes
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .as_slice(),
            [Some(parent)]
        );

        let explicit = execute_manager_tool(
            "spawn_agent",
            &serde_json::json!({
                "task":"inspect",
                "tools":["read_file"],
                "complexity":"complex"
            }),
            &manager,
            &work,
            &workspace,
        )
        .await
        .unwrap();
        assert!(!explicit.is_success());
        assert!(explicit
            .model_text()
            .contains("no automatic delegation route qualifies for complex complexity"));
        assert_eq!(
            routes
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn child_execution_enforces_prepared_path_command_network_and_job_owner_scopes() {
        let temp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let allowed = temp.path().join("allowed.txt");
        let denied = temp.path().join("denied.txt");
        let outside_read = outside.path().join("outside.txt");
        std::fs::write(&allowed, "ok").unwrap();
        std::fs::write(&denied, "secret").unwrap();
        std::fs::write(&outside_read, "outside").unwrap();
        let workspace = crate::workspace::WorkspaceContext::discover(temp.path()).unwrap();
        let allowed_scope = workspace
            .resolve(
                allowed.to_str().unwrap(),
                crate::workspace::PathOperation::Read,
            )
            .unwrap();
        let mut runner = ProviderAgentRunner {
            client: crate::agent::AgentClient::new(
                &crate::config::ProviderConfig {
                    google: None,
                    tool_schema_policy: Default::default(),
                    base_url: "http://127.0.0.1:1/v1".into(),
                    api_key: "test".into(),
                    protocol: crate::agent::ProviderProtocol::default(),
                    prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
                    models: vec!["fake".into()],
                    timeout: crate::config::ProviderTimeoutConfig::default(),
                },
                "fake",
            ),
            route_clients: AgentRouteClients::default(),
            sandbox_mode: crate::sandbox::SandboxMode::PolicyOnly,
            command_timeout_secs: 1,
            approval_rules: Vec::new(),
            permission_rules: Vec::new(),
            session_grants: Arc::new(std::sync::Mutex::new(
                crate::permission::SessionGrantStore::default(),
            )),
            skills: Arc::new(std::sync::Mutex::new(crate::skills::SkillRegistry::empty())),
            task_skill_snapshot: Arc::new(std::sync::Mutex::new(String::new())),
            manager_link: Arc::new(std::sync::OnceLock::new()),
            usage_recorder: crate::usage::UsageRecorder::new(
                Arc::new(std::sync::Mutex::new(crate::usage::UsageLedger::default())),
                crate::usage::UsageKind::Subagent,
                "test",
                "fake",
            ),
            tool_policy: crate::tool_selection::ToolSelectionPolicy::default(),
            skills_enabled: true,
            images_supported: false,
            pricing: Arc::new(std::collections::HashMap::new()),
        };
        let grant = AgentGrant {
            tools: vec![
                "read_file".into(),
                "write_file".into(),
                "run_command".into(),
            ],
            workspace_root_ids: vec![],
            path_scopes: vec![allowed_scope.to_string_lossy().into_owned()],
            command_scopes: vec!["printf scoped".into()],
            network: false,
        };
        let owner = crate::jobs::JobOwner::new(None);
        let id = AgentId::new();
        let allowed_result = execute_child_tool(
            ChildToolContext {
                runner: &runner,
                id,
                owner,
                grant: &grant,
                workspace: &workspace,
                policy_approved: false,
                frozen_policy: None,
            },
            "read_file",
            &serde_json::json!({"path":allowed}),
        )
        .await;
        assert!(allowed_result.is_success(), "{allowed_result:?}");
        let outside_result = execute_child_tool(
            ChildToolContext {
                runner: &runner,
                id,
                owner,
                grant: &grant,
                workspace: &workspace,
                policy_approved: false,
                frozen_policy: None,
            },
            "read_file",
            &serde_json::json!({"path":outside_read}),
        )
        .await;
        assert!(outside_result.is_success(), "{outside_result:?}");
        runner.permission_rules = vec![crate::permission::PermissionRule {
            decision: crate::permission::PermissionDecision::Ask,
            matcher: crate::permission::PermissionMatcher::Tool {
                tool: "read_file".into(),
            },
        }];
        assert!(matches!(
            execute_child_tool(
                ChildToolContext {
                    runner: &runner,
                    id,
                    owner,
                    grant: &grant,
                    workspace: &workspace,
                    policy_approved: false,
                    frozen_policy: None,
                },
                "read_file",
                &serde_json::json!({"path":allowed}),
            )
            .await,
            crate::agent::registry::ToolExecutionResult::Denied(_)
        ));
        assert!(execute_child_tool(
            ChildToolContext {
                runner: &runner,
                id,
                owner,
                grant: &grant,
                workspace: &workspace,
                policy_approved: true,
                frozen_policy: None,
            },
            "read_file",
            &serde_json::json!({"path":allowed}),
        )
        .await
        .is_success());
        runner.permission_rules.clear();
        assert!(execute_child_tool(
            ChildToolContext {
                runner: &runner,
                id,
                owner,
                grant: &grant,
                workspace: &workspace,
                policy_approved: false,
                frozen_policy: None,
            },
            "read_file",
            &serde_json::json!({"path":denied}),
        )
        .await
        .is_success());
        assert!(matches!(
            execute_child_tool(
                ChildToolContext {
                    runner: &runner,
                    id,
                    owner,
                    grant: &grant,
                    workspace: &workspace,
                    policy_approved: false,
                    frozen_policy: None,
                },
                "write_file",
                &serde_json::json!({"path":denied,"content":"blocked"}),
            )
            .await,
            crate::agent::registry::ToolExecutionResult::Denied(_)
        ));
        assert!(matches!(
            execute_child_tool(
                ChildToolContext {
                    runner: &runner,
                    id,
                    owner,
                    grant: &grant,
                    workspace: &workspace,
                    policy_approved: false,
                    frozen_policy: None,
                },
                "run_command",
                &serde_json::json!({"command":"printf wider"}),
            )
            .await,
            crate::agent::registry::ToolExecutionResult::Denied(_)
        ));
        assert!(matches!(
            execute_child_tool(
                ChildToolContext {
                    runner: &runner,
                    id,
                    owner,
                    grant: &grant,
                    workspace: &workspace,
                    policy_approved: false,
                    frozen_policy: None,
                },
                "run_command",
                &serde_json::json!({"command":"printf scoped", "network":true}),
            )
            .await,
            crate::agent::registry::ToolExecutionResult::Denied(_)
        ));
        cleanup_owned_jobs(owner).await;
    }

    #[tokio::test]
    async fn nested_spawn_inherits_subset_and_obeys_depth_limit() {
        let limits = AgentLimits {
            max_depth: 2,
            ..AgentLimits::default()
        };
        let manager = AgentManager::new(Arc::new(FakeRunner), limits.clone());
        let link = std::sync::OnceLock::new();
        link.set(Arc::downgrade(&manager)).unwrap();
        let (_command_tx, commands) = mpsc::channel(1);
        let (_cancel_tx, cancelled) = watch::channel(false);
        let (events, _) = mpsc::unbounded_channel();
        let context = AgentRunContext {
            id: AgentId::new(),
            request: AgentSpawnRequest {
                parent: None,
                parent_turn: Uuid::new_v4(),
                task_id: None,
                task: "parent".into(),
                context_mode: AgentContextMode::Summary,
                context_summary: "summary".into(),
                grants: AgentGrant {
                    tools: vec!["spawn_agent".into(), "read_file".into()],
                    workspace_root_ids: vec!["primary".into()],
                    path_scopes: vec!["/tmp/file".into()],
                    command_scopes: Vec::new(),
                    network: false,
                },
                workspace: crate::workspace::WorkspaceContext::discover("/tmp").unwrap(),
                depth: 1,
                route: None,
                purpose: AgentRunPurpose::Subagent,
                output_contract: None,
                isolation: AgentIsolation::Shared,
            },
            limits,
            commands,
            pending_steers: VecDeque::new(),
            cancelled,
            events,
            job_owner: crate::jobs::JobOwner::new(None),
            graph_budget: None,
            frozen_policy: None,
        };
        let result = execute_nested_spawn(
            &context,
            &serde_json::json!({"task":"child", "tools":["read_file"]}),
            &link,
        )
        .await;
        assert!(result.is_success());
        let child = manager.list().await.into_iter().next().unwrap();
        assert_eq!(child.parent, Some(context.id));
        assert_eq!(child.depth, 2);
        assert_eq!(child.grants.tools, ["read_file"]);
        assert_eq!(child.grants.path_scopes, ["/tmp/file"]);

        let mut too_deep = context;
        too_deep.request.depth = 2;
        let result = execute_nested_spawn(
            &too_deep,
            &serde_json::json!({"task":"too deep", "tools":["read_file"]}),
            &link,
        )
        .await;
        assert!(!result.is_success());
    }

    #[tokio::test]
    async fn run_policy_restricts_subagent_grants_before_spawn() {
        let manager = AgentManager::new_with_tool_policy(
            Arc::new(FakeRunner),
            AgentLimits::default(),
            crate::tool_selection::ToolSelectionPolicy::from_cli(
                Some(vec!["read_file".into()]),
                vec!["mcp__configured__tool".into()],
                Vec::new(),
            )
            .unwrap(),
            true,
            false,
        );
        let turn = Uuid::new_v4();
        let mut denied = request("denied", turn);
        denied.grants.tools = vec!["write_file".into()];
        assert_eq!(
            manager.spawn(denied).await.unwrap_err(),
            AgentManagerError::InvalidToolGrant("write_file".into())
        );
        assert!(manager.list().await.is_empty());

        let allowed = manager.spawn(request("allowed", turn)).await.unwrap();
        let completed = manager.await_agent(allowed).await.unwrap();
        assert_eq!(completed.status, AgentStatus::Completed);
        assert_eq!(completed.grants.tools, ["read_file"]);
    }

    #[tokio::test]
    async fn named_agent_presets_narrow_to_run_policy_but_explicit_grants_remain_strict() {
        let manager = AgentManager::new_with_tool_policy(
            Arc::new(FakeRunner),
            AgentLimits::default(),
            crate::tool_selection::ToolSelectionPolicy::replacing(&["read_file"]),
            false,
            false,
        );
        manager.enforce_headless_read_only();
        let work = Arc::new(std::sync::Mutex::new(crate::work::WorkState::default()));
        let workspace = crate::workspace::WorkspaceContext::discover("/tmp").unwrap();
        let result = execute_manager_tool(
            "task",
            &json!({"subagent_name":"explore", "prompt":"inspect local evidence"}),
            &manager,
            &work,
            &workspace,
        )
        .await
        .unwrap();
        assert!(result.is_success(), "{}", result.model_text());
        let children = manager.list().await;
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].grants.tools, ["read_file"]);
        assert!(!children[0].grants.network);

        let denied = execute_manager_tool(
            "spawn_agent",
            &json!({"task":"inspect", "tools":["read_file", "web_search"]}),
            &manager,
            &work,
            &workspace,
        )
        .await
        .unwrap();
        assert!(!denied.is_success());
        assert!(denied
            .model_text()
            .contains("explicit valid non-agent tool grants"));
        assert_eq!(manager.list().await.len(), 1);
        manager.shutdown().await;
    }

    #[test]
    fn builtin_read_only_agents_do_not_silently_receive_process_capable_lints() {
        for agent in builtin_subagents()
            .into_iter()
            .filter(|agent| matches!(agent.name.as_str(), "explore" | "plan"))
        {
            assert!(!agent.tools.iter().any(|tool| tool == "read_lints"));
        }
    }
}
