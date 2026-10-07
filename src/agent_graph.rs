use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Read;
use uuid::Uuid;

use crate::work::{PlanId, TaskId};

pub const MAX_GRAPH_TASKS: usize = 64;
pub const MAX_GRAPH_EDGES: usize = 256;
pub const MAX_GRAPH_TOOLS: usize = 32;
pub const MAX_GRAPH_SCOPES: usize = 32;
pub const MAX_GRAPH_ATTEMPTS: u8 = 3;
pub const MAX_ATTEMPT_SUMMARY_BYTES: usize = 4096;
pub const EVIDENCE_CONTRACT_VERSION: u32 = 1;
pub const VERIFICATION_RECEIPT_VERSION: u32 = 1;
pub const AGENT_GRAPH_BUDGET_VERSION: u32 = 1;
pub const DEFAULT_GRAPH_ACTIVE_SECS: u64 = 30 * 60;
pub const DEFAULT_GRAPH_PHYSICAL_REQUESTS: u32 = 30;
pub const DEFAULT_GRAPH_TOTAL_TOKENS: u64 = 100_000;
pub const DEFAULT_GRAPH_OUTPUT_TOKENS_PER_REQUEST: u32 = 8_192;
pub const DEFAULT_GRAPH_TOOL_CALLS: u32 = 120;
pub const DEFAULT_GRAPH_TOOL_FAILURES: u32 = 8;
pub const DEFAULT_GRAPH_CHILD_SPAWNS: u32 = 8;
pub const DEFAULT_GRAPH_RETAINED_OUTPUT_BYTES: u64 = 256 * 1024;
pub const MAX_VERIFICATION_ARTIFACTS: usize = 32;
pub const MAX_VERIFICATION_COMMAND_ARGS: usize = 32;
pub const MAX_VERIFICATION_COMMAND_TIMEOUT_SECS: u64 = 300;
pub const MAX_VERIFICATION_ARTIFACT_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentGraphStatus {
    #[default]
    Idle,
    Running,
    Paused,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentTaskState {
    #[default]
    Pending,
    Ready,
    Running,
    Completed,
    Failed,
    DependencyBlocked,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentEvidenceGate {
    AssistantOutput,
    ToolSuccess { tool: String },
    Artifact(AgentArtifactPredicateV1),
    ClosedCommand(AgentClosedCommandPredicateV1),
    HumanReview,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AgentArtifactKind {
    RegularFile,
    Directory,
    Absent,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentArtifactPredicateV1 {
    pub path: String,
    pub kind: AgentArtifactKind,
    #[serde(default)]
    pub sha256: Option<String>,
    #[serde(default)]
    pub min_bytes: Option<u64>,
    #[serde(default)]
    pub max_bytes: Option<u64>,
    #[serde(default)]
    pub must_change: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentClosedCommandPredicateV1 {
    pub argv: Vec<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    pub timeout_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentHostEvidenceObservationV1 {
    pub gate_sha256: String,
    pub authority: AgentVerificationAuthority,
    pub satisfied: bool,
    #[serde(default)]
    pub observation_sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentWorkspaceSnapshotV1 {
    pub workspace_identity_sha256: String,
    pub source_revision_sha256: String,
    #[serde(default)]
    pub artifact_observations: Vec<AgentHostEvidenceObservationV1>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentHostVerificationV1 {
    pub started: AgentWorkspaceSnapshotV1,
    pub finished: AgentWorkspaceSnapshotV1,
    #[serde(default)]
    pub observations: Vec<AgentHostEvidenceObservationV1>,
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum AgentHostVerificationError {
    #[error("workspace identity could not be captured")]
    WorkspaceIdentity,
    #[error("workspace source revision could not be captured")]
    SourceRevision,
    #[error("verification artifact is outside the assigned workspace")]
    ArtifactOutsideWorkspace,
    #[error("verification artifact contains a symbolic link")]
    ArtifactSymlink,
    #[error("verification artifact metadata could not be read")]
    ArtifactMetadata,
    #[error("verification artifact exceeds the host verification bound")]
    ArtifactTooLarge,
    #[error("verification artifact could not be read")]
    ArtifactRead,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum AgentWorkspaceLane {
    Primary,
    Worktree(Uuid),
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentGraphBudgetInput {
    #[serde(default)]
    pub max_cost_usd: Option<String>,
    #[serde(default)]
    pub allow_unpriced: bool,
    #[serde(default = "default_graph_physical_requests")]
    pub max_physical_requests: u32,
}

fn default_graph_physical_requests() -> u32 {
    DEFAULT_GRAPH_PHYSICAL_REQUESTS
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentGraphBudgetLimitsV1 {
    pub version: u32,
    pub max_active_secs: u64,
    pub max_physical_requests: u32,
    pub max_total_tokens: u64,
    pub max_output_tokens_per_request: u32,
    pub max_tool_calls: u32,
    pub max_tool_failures: u32,
    pub max_child_spawns: u32,
    pub max_retained_output_bytes: u64,
    pub max_cost_usd: Option<crate::usage::DecimalUsd>,
    pub allow_unpriced: bool,
}

impl AgentGraphBudgetLimitsV1 {
    pub fn from_input(input: AgentGraphBudgetInput) -> Result<Self, AgentGraphError> {
        let max_cost_usd = input
            .max_cost_usd
            .as_deref()
            .map(str::parse::<crate::usage::DecimalUsd>)
            .transpose()
            .map_err(|_| AgentGraphError::InvalidBudget)?;
        if input.max_physical_requests == 0
            || input.max_physical_requests > DEFAULT_GRAPH_PHYSICAL_REQUESTS
            || max_cost_usd == Some(crate::usage::DecimalUsd::ZERO)
            || (max_cost_usd.is_none() && !input.allow_unpriced)
        {
            return Err(AgentGraphError::InvalidBudget);
        }
        Ok(Self {
            version: AGENT_GRAPH_BUDGET_VERSION,
            max_active_secs: DEFAULT_GRAPH_ACTIVE_SECS,
            max_physical_requests: input.max_physical_requests,
            max_total_tokens: DEFAULT_GRAPH_TOTAL_TOKENS,
            max_output_tokens_per_request: DEFAULT_GRAPH_OUTPUT_TOKENS_PER_REQUEST,
            max_tool_calls: DEFAULT_GRAPH_TOOL_CALLS,
            max_tool_failures: DEFAULT_GRAPH_TOOL_FAILURES,
            max_child_spawns: DEFAULT_GRAPH_CHILD_SPAWNS,
            max_retained_output_bytes: DEFAULT_GRAPH_RETAINED_OUTPUT_BYTES,
            max_cost_usd,
            allow_unpriced: input.allow_unpriced,
        })
    }

    pub fn valid(&self) -> bool {
        self.version == AGENT_GRAPH_BUDGET_VERSION
            && self.max_active_secs > 0
            && self.max_physical_requests > 0
            && self.max_physical_requests <= DEFAULT_GRAPH_PHYSICAL_REQUESTS
            && self.max_total_tokens > 0
            && self.max_output_tokens_per_request > 0
            && self.max_tool_calls > 0
            && self.max_tool_failures > 0
            && self.max_child_spawns > 0
            && self.max_retained_output_bytes > 0
            && self.max_cost_usd != Some(crate::usage::DecimalUsd::ZERO)
            && (self.max_cost_usd.is_some() || self.allow_unpriced)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentGraphBudgetUsageV1 {
    #[serde(default)]
    pub active_millis: u64,
    #[serde(default)]
    pub physical_requests: u32,
    #[serde(default)]
    pub total_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub tool_calls: u32,
    #[serde(default)]
    pub tool_failures: u32,
    #[serde(default)]
    pub child_spawns: u32,
    #[serde(default)]
    pub retained_output_bytes: u64,
    #[serde(default)]
    pub cost_usd: Option<crate::usage::DecimalUsd>,
    #[serde(default)]
    pub unpriced_requests: u32,
    #[serde(default)]
    pub blocked: Option<AgentBudgetLimitKind>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentBudgetLimitKind {
    ActiveTime,
    PhysicalRequests,
    TotalTokens,
    OutputTokensPerRequest,
    ToolCalls,
    ToolFailures,
    ChildSpawns,
    RetainedOutput,
    Cost,
    UnknownUsage,
    UnpricedRoute,
}

#[derive(Clone)]
pub struct AgentAutonomyBudget {
    inner: std::sync::Arc<std::sync::Mutex<AgentAutonomyBudgetState>>,
}

struct AgentAutonomyBudgetState {
    limits: AgentGraphBudgetLimitsV1,
    usage: AgentGraphBudgetUsageV1,
    active_started: std::time::Instant,
    base_active_millis: u64,
}

impl std::fmt::Debug for AgentAutonomyBudget {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AgentAutonomyBudget")
            .field("usage", &self.snapshot())
            .finish_non_exhaustive()
    }
}

impl AgentAutonomyBudget {
    pub fn new(
        limits: AgentGraphBudgetLimitsV1,
        usage: AgentGraphBudgetUsageV1,
    ) -> Result<Self, AgentGraphError> {
        if !limits.valid() {
            return Err(AgentGraphError::InvalidBudget);
        }
        let base_active_millis = usage.active_millis;
        Ok(Self {
            inner: std::sync::Arc::new(std::sync::Mutex::new(AgentAutonomyBudgetState {
                limits,
                usage,
                active_started: std::time::Instant::now(),
                base_active_millis,
            })),
        })
    }

    pub fn limits(&self) -> AgentGraphBudgetLimitsV1 {
        self.inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .limits
            .clone()
    }

    pub fn snapshot(&self) -> AgentGraphBudgetUsageV1 {
        let mut state = self
            .inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        update_active_time(&mut state);
        state.usage.clone()
    }

    pub fn reserve_request(&self) -> Result<(), AgentBudgetLimitKind> {
        let mut state = self
            .inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        ensure_active(&mut state)?;
        ensure_not_blocked(&state)?;
        if state.usage.physical_requests >= state.limits.max_physical_requests {
            return block_budget(&mut state, AgentBudgetLimitKind::PhysicalRequests);
        }
        state.usage.physical_requests += 1;
        Ok(())
    }

    pub fn observe_request(
        &self,
        usage: Option<&crate::agent::stream::UsageInfo>,
        pricing: Option<&crate::usage::UsagePricing>,
    ) -> Result<(), AgentBudgetLimitKind> {
        let mut state = self
            .inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        ensure_active(&mut state)?;
        ensure_not_blocked(&state)?;
        let Some(usage) = usage else {
            return block_budget(&mut state, AgentBudgetLimitKind::UnknownUsage);
        };
        if usage.completion_tokens > state.limits.max_output_tokens_per_request {
            return block_budget(&mut state, AgentBudgetLimitKind::OutputTokensPerRequest);
        }
        let Some(total_tokens) = state
            .usage
            .total_tokens
            .checked_add(u64::from(usage.total_tokens))
        else {
            return block_budget(&mut state, AgentBudgetLimitKind::TotalTokens);
        };
        if total_tokens > state.limits.max_total_tokens {
            return block_budget(&mut state, AgentBudgetLimitKind::TotalTokens);
        }
        let Some(output_tokens) = state
            .usage
            .output_tokens
            .checked_add(u64::from(usage.completion_tokens))
        else {
            return block_budget(&mut state, AgentBudgetLimitKind::TotalTokens);
        };
        let request_cost = match pricing {
            Some(pricing) => {
                crate::usage::cost_for(pricing, usage).ok_or(AgentBudgetLimitKind::UnknownUsage)
            }
            None if state.limits.allow_unpriced => {
                state.usage.unpriced_requests = state.usage.unpriced_requests.saturating_add(1);
                Ok(crate::usage::DecimalUsd::ZERO)
            }
            None => Err(AgentBudgetLimitKind::UnpricedRoute),
        };
        let request_cost = match request_cost {
            Ok(cost) => cost,
            Err(limit) => return block_budget(&mut state, limit),
        };
        let current_cost = state
            .usage
            .cost_usd
            .unwrap_or(crate::usage::DecimalUsd::ZERO);
        let Some(next_cost) = current_cost.checked_add(request_cost) else {
            return block_budget(&mut state, AgentBudgetLimitKind::Cost);
        };
        if state
            .limits
            .max_cost_usd
            .is_some_and(|maximum| next_cost > maximum)
        {
            return block_budget(&mut state, AgentBudgetLimitKind::Cost);
        }
        state.usage.total_tokens = total_tokens;
        state.usage.output_tokens = output_tokens;
        if pricing.is_some() {
            state.usage.cost_usd = Some(next_cost);
        }
        Ok(())
    }

    pub fn reserve_tool_call(&self) -> Result<(), AgentBudgetLimitKind> {
        let mut state = self
            .inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        ensure_active(&mut state)?;
        ensure_not_blocked(&state)?;
        if state.usage.tool_calls >= state.limits.max_tool_calls {
            return block_budget(&mut state, AgentBudgetLimitKind::ToolCalls);
        }
        state.usage.tool_calls += 1;
        Ok(())
    }

    pub fn observe_tool_failure(&self) -> Result<(), AgentBudgetLimitKind> {
        let mut state = self
            .inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        ensure_active(&mut state)?;
        ensure_not_blocked(&state)?;
        if state.usage.tool_failures >= state.limits.max_tool_failures {
            return block_budget(&mut state, AgentBudgetLimitKind::ToolFailures);
        }
        state.usage.tool_failures += 1;
        Ok(())
    }

    pub fn reserve_child_spawn(&self) -> Result<(), AgentBudgetLimitKind> {
        let mut state = self
            .inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        ensure_active(&mut state)?;
        ensure_not_blocked(&state)?;
        if state.usage.child_spawns >= state.limits.max_child_spawns {
            return block_budget(&mut state, AgentBudgetLimitKind::ChildSpawns);
        }
        state.usage.child_spawns += 1;
        Ok(())
    }

    pub fn retain_output(&self, bytes: usize) -> Result<(), AgentBudgetLimitKind> {
        let mut state = self
            .inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        ensure_active(&mut state)?;
        ensure_not_blocked(&state)?;
        let bytes = u64::try_from(bytes).unwrap_or(u64::MAX);
        let Some(next) = state.usage.retained_output_bytes.checked_add(bytes) else {
            return block_budget(&mut state, AgentBudgetLimitKind::RetainedOutput);
        };
        if next > state.limits.max_retained_output_bytes {
            return block_budget(&mut state, AgentBudgetLimitKind::RetainedOutput);
        }
        state.usage.retained_output_bytes = next;
        Ok(())
    }
}

fn update_active_time(state: &mut AgentAutonomyBudgetState) {
    let elapsed = u64::try_from(state.active_started.elapsed().as_millis()).unwrap_or(u64::MAX);
    state.usage.active_millis = state.base_active_millis.saturating_add(elapsed);
}

fn ensure_active(state: &mut AgentAutonomyBudgetState) -> Result<(), AgentBudgetLimitKind> {
    update_active_time(state);
    if state.usage.active_millis >= state.limits.max_active_secs.saturating_mul(1000) {
        return block_budget(state, AgentBudgetLimitKind::ActiveTime);
    }
    Ok(())
}

fn ensure_not_blocked(state: &AgentAutonomyBudgetState) -> Result<(), AgentBudgetLimitKind> {
    state.usage.blocked.map_or(Ok(()), Err)
}

fn block_budget<T>(
    state: &mut AgentAutonomyBudgetState,
    limit: AgentBudgetLimitKind,
) -> Result<T, AgentBudgetLimitKind> {
    state.usage.blocked.get_or_insert(limit);
    Err(limit)
}

/// Immutable evidence policy generated by the host from a normalized graph
/// definition. Model-provided graph input cannot supply or forge this wrapper.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EvidenceContractV1 {
    pub version: u32,
    #[serde(default)]
    pub plan_id: Option<PlanId>,
    #[serde(default)]
    pub configuration_revision: Option<u64>,
    pub gates: Vec<AgentEvidenceGate>,
}

impl EvidenceContractV1 {
    fn current(
        plan_id: PlanId,
        configuration_revision: u64,
        gates: Vec<AgentEvidenceGate>,
    ) -> Self {
        Self {
            version: EVIDENCE_CONTRACT_VERSION,
            plan_id: Some(plan_id),
            configuration_revision: Some(configuration_revision),
            gates,
        }
    }

    pub(crate) fn matches(
        &self,
        plan_id: PlanId,
        configuration_revision: u64,
        gates: &[AgentEvidenceGate],
    ) -> bool {
        self.version == EVIDENCE_CONTRACT_VERSION
            && self.plan_id == Some(plan_id)
            && self.configuration_revision == Some(configuration_revision)
            && !self.gates.is_empty()
            && self.gates.len() <= MAX_GRAPH_TOOLS
            && self.gates == gates
            && self.gates.iter().all(|gate| match gate {
                AgentEvidenceGate::AssistantOutput => true,
                AgentEvidenceGate::ToolSuccess { tool } => {
                    normalize_name(tool).is_ok_and(|normalized| normalized == *tool)
                }
                AgentEvidenceGate::Artifact(predicate) => {
                    normalize_artifact_predicate(predicate.clone()).as_ref() == Ok(predicate)
                }
                AgentEvidenceGate::ClosedCommand(predicate) => {
                    normalize_closed_command(predicate.clone()).as_ref() == Ok(predicate)
                }
                AgentEvidenceGate::HumanReview => true,
            })
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentVerificationDisposition {
    Pass,
    Fail,
    Inconclusive,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentVerificationAuthority {
    AssistantClaim,
    HostObservation,
    HumanReview,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentVerificationCheckV1 {
    #[serde(default)]
    pub gate: Option<AgentEvidenceGate>,
    #[serde(default)]
    pub gate_sha256: String,
    pub authority: AgentVerificationAuthority,
    pub satisfied: bool,
}

/// Bounded host-owned verification result. It deliberately records no model
/// output, tool arguments, tool output, command text, paths, or credentials.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentVerificationReceiptV1 {
    pub version: u32,
    #[serde(default)]
    pub plan_id: Option<PlanId>,
    #[serde(default)]
    pub configuration_revision: Option<u64>,
    #[serde(default)]
    pub approved_revision: Option<u64>,
    pub task_id: TaskId,
    pub attempt_number: u8,
    pub agent_id: Uuid,
    pub contract_sha256: String,
    #[serde(default)]
    pub workspace_snapshot_sha256: Option<String>,
    #[serde(default)]
    pub host_evidence_sha256: Option<String>,
    pub terminal_status: AgentAttemptStatus,
    pub terminal_event_sequence: Option<u64>,
    pub disposition: AgentVerificationDisposition,
    pub checks: Vec<AgentVerificationCheckV1>,
    pub receipt_sha256: String,
}

fn default_evidence() -> Vec<AgentEvidenceGate> {
    vec![AgentEvidenceGate::AssistantOutput]
}

fn default_max_attempts() -> u8 {
    1
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentTaskGrant {
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default)]
    pub paths: Vec<String>,
    #[serde(default)]
    pub commands: Vec<String>,
    #[serde(default)]
    pub network: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentTaskDefinition {
    #[serde(default)]
    pub depends_on: Vec<TaskId>,
    #[serde(default)]
    pub grant: AgentTaskGrant,
    #[serde(default)]
    pub worktree_id: Option<Uuid>,
    #[serde(default)]
    pub accept_cross_lane_handoff: bool,
    #[serde(default = "default_max_attempts")]
    pub max_attempts: u8,
    #[serde(default = "default_evidence")]
    pub evidence: Vec<AgentEvidenceGate>,
    #[serde(default)]
    pub verification_contract: Option<EvidenceContractV1>,
}

/// One explicit prerequisite relationship in a dependency-aware agent graph.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct AgentDependencyEdge {
    pub prerequisite: TaskId,
    pub dependent: TaskId,
}

impl AgentTaskDefinition {
    pub fn dependency_edges(
        &self,
        dependent: TaskId,
    ) -> impl Iterator<Item = AgentDependencyEdge> + '_ {
        self.depends_on
            .iter()
            .copied()
            .map(move |prerequisite| AgentDependencyEdge {
                prerequisite,
                dependent,
            })
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentAttemptStatus {
    Queued,
    Running,
    WaitingApproval,
    Completed,
    Failed,
    Cancelled,
    TimedOut,
    Interrupted,
    EvidenceFailed,
}

impl AgentAttemptStatus {
    pub fn terminal(self) -> bool {
        !matches!(self, Self::Queued | Self::Running | Self::WaitingApproval)
    }
}

impl From<&crate::agent::subagent::AgentStatus> for AgentAttemptStatus {
    fn from(status: &crate::agent::subagent::AgentStatus) -> Self {
        match status {
            crate::agent::subagent::AgentStatus::Queued => Self::Queued,
            crate::agent::subagent::AgentStatus::Running => Self::Running,
            crate::agent::subagent::AgentStatus::WaitingApproval => Self::WaitingApproval,
            crate::agent::subagent::AgentStatus::Completed => Self::Completed,
            crate::agent::subagent::AgentStatus::Failed => Self::Failed,
            crate::agent::subagent::AgentStatus::Cancelled => Self::Cancelled,
            crate::agent::subagent::AgentStatus::TimedOut => Self::TimedOut,
            crate::agent::subagent::AgentStatus::Interrupted => Self::Interrupted,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentTaskAttempt {
    pub number: u8,
    pub agent_id: Option<Uuid>,
    pub status: AgentAttemptStatus,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub successful_tools: Vec<String>,
    pub summary: Option<String>,
    #[serde(default)]
    pub terminal_event_sequence: Option<u64>,
    #[serde(default)]
    pub verification_receipt: Option<AgentVerificationReceiptV1>,
    #[serde(default)]
    pub workspace_snapshot: Option<AgentWorkspaceSnapshotV1>,
    #[serde(default)]
    pub host_verification: Option<AgentHostVerificationV1>,
    #[serde(default)]
    pub budget_usage: AgentGraphBudgetUsageV1,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentTaskRuntime {
    #[serde(default)]
    pub state: AgentTaskState,
    #[serde(default)]
    pub attempts: Vec<AgentTaskAttempt>,
    pub failure: Option<String>,
}

impl Default for AgentTaskRuntime {
    fn default() -> Self {
        Self {
            state: AgentTaskState::Pending,
            attempts: Vec::new(),
            failure: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentGraphVerificationBindingV1 {
    pub plan_id: PlanId,
    pub configuration_revision: u64,
    pub approved_revision: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentGraphState {
    #[serde(default)]
    pub status: AgentGraphStatus,
    pub updated_at: Option<DateTime<Utc>>,
    pub notice: Option<String>,
    #[serde(default)]
    pub plan_id: Option<PlanId>,
    #[serde(default)]
    pub configuration_revision: Option<u64>,
    #[serde(default)]
    pub approved_revision: Option<u64>,
    #[serde(default)]
    pub budget_limits: Option<AgentGraphBudgetLimitsV1>,
    #[serde(default)]
    pub budget_usage: AgentGraphBudgetUsageV1,
}

impl AgentGraphState {
    pub fn configured(
        plan_id: PlanId,
        configuration_revision: u64,
        budget_limits: AgentGraphBudgetLimitsV1,
    ) -> Self {
        Self {
            plan_id: Some(plan_id),
            configuration_revision: Some(configuration_revision),
            budget_limits: Some(budget_limits),
            ..Self::default()
        }
    }

    pub fn verification_binding(&self) -> Option<AgentGraphVerificationBindingV1> {
        Some(AgentGraphVerificationBindingV1 {
            plan_id: self.plan_id?,
            configuration_revision: self.configuration_revision?,
            approved_revision: self.approved_revision?,
        })
    }
}

#[derive(Debug, Clone, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ConfigureAgentGraphArgs {
    pub expected_revision: u64,
    pub budget: AgentGraphBudgetInput,
    pub tasks: Vec<AgentTaskInput>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentTaskInput {
    pub task_id: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default)]
    pub paths: Vec<String>,
    #[serde(default)]
    pub commands: Vec<String>,
    #[serde(default)]
    pub network: bool,
    #[serde(default)]
    pub worktree_id: Option<String>,
    #[serde(default)]
    pub accept_cross_lane_handoff: bool,
    #[serde(default = "default_max_attempts")]
    pub max_attempts: u8,
    #[serde(default = "default_evidence_input")]
    pub evidence: Vec<AgentEvidenceInput>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentEvidenceInput {
    AssistantOutput,
    ToolSuccess {
        tool: String,
    },
    Artifact {
        path: String,
        artifact_kind: AgentArtifactKind,
        #[serde(default)]
        sha256: Option<String>,
        #[serde(default)]
        min_bytes: Option<u64>,
        #[serde(default)]
        max_bytes: Option<u64>,
        #[serde(default)]
        must_change: bool,
    },
    ClosedCommand {
        argv: Vec<String>,
        #[serde(default)]
        cwd: Option<String>,
        #[serde(default = "default_verification_timeout_secs")]
        timeout_secs: u64,
    },
    HumanReview,
}

fn default_verification_timeout_secs() -> u64 {
    120
}

fn default_evidence_input() -> Vec<AgentEvidenceInput> {
    vec![AgentEvidenceInput::AssistantOutput]
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AgentGraphError {
    #[error("agent graph must configure every plan task exactly once (maximum 64)")]
    InvalidTaskSet,
    #[error("invalid agent graph task ID: {0}")]
    InvalidTaskId(String),
    #[error("agent task {task} has an unknown dependency: {dependency}")]
    UnknownDependency { task: TaskId, dependency: TaskId },
    #[error("agent task cannot depend on itself: {0}")]
    SelfDependency(TaskId),
    #[error("agent graph contains a dependency cycle")]
    Cycle,
    #[error("agent graph has too many dependency edges")]
    TooManyEdges,
    #[error("agent task grants or evidence are invalid")]
    InvalidPolicy,
    #[error("worktree {0} is assigned to more than one agent task")]
    DuplicateWorktree(Uuid),
    #[error("agent graph is not configured")]
    NotConfigured,
    #[error("agent graph is not ready for that transition")]
    InvalidTransition,
    #[error("agent graph requires an approved plan")]
    PlanNotApproved,
    #[error(
        "agent graph verification is missing or stale; reconfigure the graph and reapprove the plan"
    )]
    VerificationContractRequired,
    #[error("agent task is unknown: {0}")]
    UnknownTask(TaskId),
    #[error("agent task has no remaining user-approved attempt")]
    AttemptLimit,
    #[error("agent graph requires an explicit positive cost cap or explicit unpriced-route decision with a physical-request cap")]
    InvalidBudget,
    #[error("writable agent tasks sharing one workspace lane must be dependency ordered")]
    UnserializedWorkspaceLane,
    #[error("cross-workspace dependency requires an explicit handoff task")]
    CrossLaneHandoffRequired,
    #[error("agent graph autonomy budget is exhausted: {0:?}")]
    BudgetExhausted(AgentBudgetLimitKind),
}

pub fn parse_configuration(
    plan_id: PlanId,
    configuration_revision: u64,
    plan_task_ids: &[TaskId],
    inputs: Vec<AgentTaskInput>,
) -> Result<HashMap<TaskId, AgentTaskDefinition>, AgentGraphError> {
    if plan_task_ids.is_empty()
        || plan_task_ids.len() > MAX_GRAPH_TASKS
        || inputs.len() != plan_task_ids.len()
    {
        return Err(AgentGraphError::InvalidTaskSet);
    }
    let known = plan_task_ids.iter().copied().collect::<HashSet<_>>();
    let mut configured = HashMap::new();
    let mut edge_count = 0usize;
    let registry = crate::agent::registry::ToolRegistry::builtins();
    let available_tools = registry
        .definitions(crate::agent::registry::ToolProfile::Subagent)
        .into_iter()
        .map(|definition| definition.function.name)
        .collect::<HashSet<_>>();
    let mut mutating_tools = registry
        .specs()
        .iter()
        .filter(|spec| spec.mutation == crate::agent::registry::ToolMutation::Mutating)
        .map(|spec| spec.name.as_str().to_string())
        .collect::<HashSet<_>>();
    mutating_tools.insert("run_command".into());
    for input in inputs {
        let task_id = parse_task_id(&input.task_id)?;
        if !known.contains(&task_id) || configured.contains_key(&task_id) {
            return Err(AgentGraphError::InvalidTaskSet);
        }
        let mut dependencies = Vec::with_capacity(input.depends_on.len());
        let mut dependency_set = HashSet::new();
        for raw in input.depends_on {
            let dependency = parse_task_id(&raw)?;
            if dependency == task_id {
                return Err(AgentGraphError::SelfDependency(task_id));
            }
            if !known.contains(&dependency) {
                return Err(AgentGraphError::UnknownDependency {
                    task: task_id,
                    dependency,
                });
            }
            if dependency_set.insert(dependency) {
                dependencies.push(dependency);
                edge_count += 1;
            }
        }
        if edge_count > MAX_GRAPH_EDGES {
            return Err(AgentGraphError::TooManyEdges);
        }
        let worktree_id = input
            .worktree_id
            .map(|raw| Uuid::parse_str(&raw).map_err(|_| AgentGraphError::InvalidPolicy))
            .transpose()?;
        let tools = normalize_names(input.tools, MAX_GRAPH_TOOLS)?;
        if tools.iter().any(|tool| !available_tools.contains(tool)) {
            return Err(AgentGraphError::InvalidPolicy);
        }
        let paths = normalize_scopes(input.paths)?;
        let commands = normalize_scopes(input.commands)?;
        if input.max_attempts == 0
            || input.max_attempts > MAX_GRAPH_ATTEMPTS
            || input.evidence.is_empty()
            || input.evidence.len() > MAX_GRAPH_TOOLS
        {
            return Err(AgentGraphError::InvalidPolicy);
        }
        let mut evidence = Vec::new();
        for gate in input.evidence {
            let gate = match gate {
                AgentEvidenceInput::AssistantOutput => AgentEvidenceGate::AssistantOutput,
                AgentEvidenceInput::ToolSuccess { tool } => {
                    let tool = normalize_name(&tool)?;
                    if !tools.contains(&tool) {
                        return Err(AgentGraphError::InvalidPolicy);
                    }
                    AgentEvidenceGate::ToolSuccess { tool }
                }
                AgentEvidenceInput::Artifact {
                    path,
                    artifact_kind,
                    sha256,
                    min_bytes,
                    max_bytes,
                    must_change,
                } => AgentEvidenceGate::Artifact(normalize_artifact_predicate(
                    AgentArtifactPredicateV1 {
                        path,
                        kind: artifact_kind,
                        sha256,
                        min_bytes,
                        max_bytes,
                        must_change,
                    },
                )?),
                AgentEvidenceInput::ClosedCommand {
                    argv,
                    cwd,
                    timeout_secs,
                } => AgentEvidenceGate::ClosedCommand(normalize_closed_command(
                    AgentClosedCommandPredicateV1 {
                        argv,
                        cwd,
                        timeout_secs,
                    },
                )?),
                AgentEvidenceInput::HumanReview => AgentEvidenceGate::HumanReview,
            };
            if !evidence.contains(&gate) {
                evidence.push(gate);
            }
        }
        if evidence
            .iter()
            .filter(|gate| matches!(gate, AgentEvidenceGate::Artifact(_)))
            .count()
            > MAX_VERIFICATION_ARTIFACTS
        {
            return Err(AgentGraphError::InvalidPolicy);
        }
        if evidence.iter().any(|gate| {
            matches!(gate, AgentEvidenceGate::ClosedCommand(predicate)
                if !commands.contains(&predicate.argv.join(" ")))
        }) {
            return Err(AgentGraphError::InvalidPolicy);
        }
        let verification_contract =
            EvidenceContractV1::current(plan_id, configuration_revision, evidence.clone());
        configured.insert(
            task_id,
            AgentTaskDefinition {
                depends_on: dependencies,
                grant: AgentTaskGrant {
                    tools,
                    paths,
                    commands,
                    network: input.network,
                },
                worktree_id,
                accept_cross_lane_handoff: input.accept_cross_lane_handoff,
                max_attempts: input.max_attempts,
                evidence,
                verification_contract: Some(verification_contract),
            },
        );
    }
    if configured.len() != known.len() {
        return Err(AgentGraphError::InvalidTaskSet);
    }
    ensure_acyclic(plan_task_ids, &configured)?;
    ensure_workspace_lanes(plan_task_ids, &configured, &mutating_tools)?;
    Ok(configured)
}

pub fn ready_tasks<'a>(
    tasks: &'a [crate::work::PlanTask],
    graph: &AgentGraphState,
) -> Vec<&'a crate::work::PlanTask> {
    if graph.status != AgentGraphStatus::Running {
        return Vec::new();
    }
    tasks
        .iter()
        .filter(|task| task.agent.is_some() && task.agent_runtime.state == AgentTaskState::Ready)
        .collect()
}

pub fn synchronize(tasks: &mut [crate::work::PlanTask], graph: &mut AgentGraphState) {
    let verification_binding = graph.verification_binding();
    let states = tasks
        .iter()
        .map(|task| {
            (
                task.id,
                (
                    task.agent_runtime.state,
                    task_completion_verified(verification_binding, task),
                ),
            )
        })
        .collect::<HashMap<_, _>>();
    for task in tasks.iter_mut().filter(|task| task.agent.is_some()) {
        if matches!(
            task.agent_runtime.state,
            AgentTaskState::Running
                | AgentTaskState::Completed
                | AgentTaskState::Failed
                | AgentTaskState::Cancelled
        ) {
            continue;
        }
        let dependencies = &task.agent.as_ref().expect("filtered").depends_on;
        task.agent_runtime.state = if dependencies.iter().any(|dependency| {
            matches!(
                states.get(dependency),
                Some((
                    AgentTaskState::Failed
                        | AgentTaskState::Cancelled
                        | AgentTaskState::DependencyBlocked,
                    _
                ))
            )
        }) {
            AgentTaskState::DependencyBlocked
        } else if dependencies.iter().all(|dependency| {
            states
                .get(dependency)
                .is_some_and(|(state, verified)| *state == AgentTaskState::Completed && *verified)
        }) {
            AgentTaskState::Ready
        } else {
            AgentTaskState::Pending
        };
    }
    if !tasks.iter().any(|task| task.agent.is_some()) {
        graph.status = AgentGraphStatus::Idle;
    } else if tasks
        .iter()
        .filter(|task| task.agent.is_some())
        .all(|task| task_completion_verified(verification_binding, task))
    {
        graph.status = AgentGraphStatus::Completed;
        graph.notice = Some("All graph tasks completed with evidence".into());
    } else if tasks
        .iter()
        .filter(|task| task.agent.is_some())
        .any(|task| {
            task.agent_runtime.state == AgentTaskState::Completed
                && !task_completion_verified(verification_binding, task)
        })
    {
        graph.status = AgentGraphStatus::Paused;
        graph.notice = Some(
            "A completed task lacks a current authoritative pass receipt; reconfigure and reapprove"
                .into(),
        );
    } else if tasks
        .iter()
        .filter(|task| task.agent.is_some())
        .any(|task| {
            task.agent_runtime.state == AgentTaskState::Failed
                && task.agent.as_ref().is_some_and(|definition| {
                    task.agent_runtime.attempts.len() >= usize::from(definition.max_attempts)
                })
        })
    {
        graph.status = AgentGraphStatus::Failed;
        graph.notice = Some("A failed task exhausted its approved attempts".into());
    } else if tasks
        .iter()
        .filter(|task| task.agent.is_some())
        .any(|task| task.agent_runtime.state == AgentTaskState::Failed)
    {
        graph.status = AgentGraphStatus::Paused;
        graph.notice = Some("A failed task requires an explicit retry decision".into());
    }
    graph.updated_at = Some(Utc::now());
}

#[derive(Serialize)]
struct VerificationContractBinding<'a> {
    domain: &'static str,
    task_id: TaskId,
    depends_on: &'a [TaskId],
    grant: &'a AgentTaskGrant,
    worktree_id: Option<Uuid>,
    accept_cross_lane_handoff: bool,
    max_attempts: u8,
    evidence: &'a [AgentEvidenceGate],
    verification_contract: &'a Option<EvidenceContractV1>,
}

pub fn verification_contract_sha256(task_id: TaskId, definition: &AgentTaskDefinition) -> String {
    let binding = VerificationContractBinding {
        domain: "ca.agent-evidence-contract.v1",
        task_id,
        depends_on: &definition.depends_on,
        grant: &definition.grant,
        worktree_id: definition.worktree_id,
        accept_cross_lane_handoff: definition.accept_cross_lane_handoff,
        max_attempts: definition.max_attempts,
        evidence: &definition.evidence,
        verification_contract: &definition.verification_contract,
    };
    let encoded = serde_json::to_vec(&binding)
        .expect("agent verification contract contains only serializable values");
    format!("{:x}", Sha256::digest(encoded))
}

pub fn evidence_gate_sha256(gate: &AgentEvidenceGate) -> String {
    let encoded = serde_json::to_vec(&("ca.agent-evidence-gate.v1", gate))
        .expect("agent evidence gate contains only serializable values");
    format!("{:x}", Sha256::digest(encoded))
}

pub fn workspace_snapshot_sha256(snapshot: &AgentWorkspaceSnapshotV1) -> String {
    let encoded = serde_json::to_vec(&("ca.agent-workspace-snapshot.v1", snapshot))
        .expect("agent workspace snapshot contains only serializable values");
    format!("{:x}", Sha256::digest(encoded))
}

pub fn host_verification_sha256(verification: &AgentHostVerificationV1) -> String {
    let encoded = serde_json::to_vec(&("ca.agent-host-verification.v1", verification))
        .expect("agent host verification contains only serializable values");
    format!("{:x}", Sha256::digest(encoded))
}

#[derive(Serialize)]
struct WorkspaceIdentityBinding {
    root_id: String,
    path_sha256: String,
    read_only: bool,
    primary: bool,
    metadata_identity: String,
}

/// Capture the immutable host side of a graph attempt. Callers must run this
/// away from the TUI event loop because it performs bounded filesystem reads
/// and local Git revision discovery.
pub fn capture_workspace_snapshot(
    workspace: &crate::workspace::WorkspaceContext,
    definition: &AgentTaskDefinition,
) -> Result<AgentWorkspaceSnapshotV1, AgentHostVerificationError> {
    let mut identities = Vec::new();
    let mut revisions = Vec::new();
    for root in workspace.roots() {
        let canonical = std::fs::canonicalize(&root.path)
            .map_err(|_| AgentHostVerificationError::WorkspaceIdentity)?;
        if canonical != root.path || !canonical.is_dir() {
            return Err(AgentHostVerificationError::WorkspaceIdentity);
        }
        let metadata = std::fs::metadata(&canonical)
            .map_err(|_| AgentHostVerificationError::WorkspaceIdentity)?;
        #[cfg(unix)]
        let metadata_identity = {
            use std::os::unix::fs::MetadataExt;
            format!("{}:{}:{}", metadata.dev(), metadata.ino(), metadata.mode())
        };
        #[cfg(not(unix))]
        let metadata_identity = format!("{}:{}", metadata.len(), metadata.permissions().readonly());
        identities.push(WorkspaceIdentityBinding {
            root_id: root.id.to_string(),
            path_sha256: format!(
                "{:x}",
                Sha256::digest(canonical.to_string_lossy().as_bytes())
            ),
            read_only: root.read_only,
            primary: root.primary,
            metadata_identity,
        });
        revisions.push(source_revision_for_root(&canonical)?);
    }
    if identities.is_empty() {
        return Err(AgentHostVerificationError::WorkspaceIdentity);
    }
    let workspace_identity_sha256 = format!(
        "{:x}",
        Sha256::digest(
            serde_json::to_vec(&("ca.agent-workspace-identity.v1", &identities))
                .map_err(|_| AgentHostVerificationError::WorkspaceIdentity)?
        )
    );
    let source_revision_sha256 = format!(
        "{:x}",
        Sha256::digest(
            serde_json::to_vec(&("ca.agent-source-revision.v1", &revisions))
                .map_err(|_| AgentHostVerificationError::SourceRevision)?
        )
    );
    let artifact_observations = definition
        .evidence
        .iter()
        .filter_map(|gate| match gate {
            AgentEvidenceGate::Artifact(predicate) => {
                Some(observe_artifact(workspace, gate, predicate))
            }
            _ => None,
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(AgentWorkspaceSnapshotV1 {
        workspace_identity_sha256,
        source_revision_sha256,
        artifact_observations,
    })
}

fn source_revision_for_root(root: &std::path::Path) -> Result<String, AgentHostVerificationError> {
    let output = std::process::Command::new("/usr/bin/git")
        .args(["rev-parse", "--verify", "HEAD"])
        .current_dir(root)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output();
    match output {
        Ok(output) if output.status.success() => {
            if output.stdout.len() > 256 {
                return Err(AgentHostVerificationError::SourceRevision);
            }
            let revision = std::str::from_utf8(&output.stdout)
                .map_err(|_| AgentHostVerificationError::SourceRevision)?
                .trim();
            if revision.len() < 40
                || revision.len() > 64
                || !revision.bytes().all(|byte| byte.is_ascii_hexdigit())
            {
                return Err(AgentHostVerificationError::SourceRevision);
            }
            Ok(revision.to_ascii_lowercase())
        }
        Ok(_) => Ok("non_git_workspace".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok("git_unavailable".into()),
        Err(_) => Err(AgentHostVerificationError::SourceRevision),
    }
}

fn observe_artifact(
    workspace: &crate::workspace::WorkspaceContext,
    gate: &AgentEvidenceGate,
    predicate: &AgentArtifactPredicateV1,
) -> Result<AgentHostEvidenceObservationV1, AgentHostVerificationError> {
    let lexical = workspace.cwd().join(&predicate.path);
    let root = workspace
        .roots()
        .iter()
        .find(|root| lexical.starts_with(&root.path))
        .ok_or(AgentHostVerificationError::ArtifactOutsideWorkspace)?;
    let relative = lexical
        .strip_prefix(&root.path)
        .map_err(|_| AgentHostVerificationError::ArtifactOutsideWorkspace)?;
    let mut cursor = root.path.clone();
    for component in relative.components() {
        cursor.push(component.as_os_str());
        match std::fs::symlink_metadata(&cursor) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(AgentHostVerificationError::ArtifactSymlink);
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(_) => return Err(AgentHostVerificationError::ArtifactMetadata),
        }
    }
    let resolved = workspace
        .resolve(&predicate.path, crate::workspace::PathOperation::Create)
        .map_err(|_| AgentHostVerificationError::ArtifactOutsideWorkspace)?;
    if workspace.root_relative(&resolved).is_none() {
        return Err(AgentHostVerificationError::ArtifactOutsideWorkspace);
    }
    let (actual_kind, size, content_sha256) = match std::fs::symlink_metadata(&resolved) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => ("absent", None, None),
        Err(_) => return Err(AgentHostVerificationError::ArtifactMetadata),
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(AgentHostVerificationError::ArtifactSymlink);
        }
        Ok(metadata) if metadata.is_dir() => ("directory", None, None),
        Ok(metadata) if metadata.is_file() => {
            if metadata.len() > MAX_VERIFICATION_ARTIFACT_BYTES {
                return Err(AgentHostVerificationError::ArtifactTooLarge);
            }
            let mut file = std::fs::File::open(&resolved)
                .map_err(|_| AgentHostVerificationError::ArtifactRead)?;
            let mut digest = Sha256::new();
            let mut bytes = 0u64;
            let mut buffer = [0u8; 32 * 1024];
            loop {
                let read = file
                    .read(&mut buffer)
                    .map_err(|_| AgentHostVerificationError::ArtifactRead)?;
                if read == 0 {
                    break;
                }
                bytes = bytes.saturating_add(read as u64);
                if bytes > MAX_VERIFICATION_ARTIFACT_BYTES {
                    return Err(AgentHostVerificationError::ArtifactTooLarge);
                }
                digest.update(&buffer[..read]);
            }
            (
                "regular_file",
                Some(bytes),
                Some(format!("{:x}", digest.finalize())),
            )
        }
        Ok(_) => ("other", None, None),
    };
    let satisfied = match predicate.kind {
        AgentArtifactKind::Absent => actual_kind == "absent",
        AgentArtifactKind::Directory => actual_kind == "directory",
        AgentArtifactKind::RegularFile => {
            actual_kind == "regular_file"
                && predicate
                    .sha256
                    .as_ref()
                    .is_none_or(|expected| content_sha256.as_ref() == Some(expected))
                && predicate
                    .min_bytes
                    .is_none_or(|minimum| size.is_some_and(|size| size >= minimum))
                && predicate
                    .max_bytes
                    .is_none_or(|maximum| size.is_some_and(|size| size <= maximum))
        }
    };
    let observation_sha256 = format!(
        "{:x}",
        Sha256::digest(
            serde_json::to_vec(&(
                "ca.agent-artifact-observation.v1",
                actual_kind,
                size,
                content_sha256,
            ))
            .map_err(|_| AgentHostVerificationError::ArtifactRead)?
        )
    );
    Ok(AgentHostEvidenceObservationV1 {
        gate_sha256: evidence_gate_sha256(gate),
        authority: AgentVerificationAuthority::HostObservation,
        satisfied,
        observation_sha256: Some(observation_sha256),
    })
}

pub fn finish_workspace_verification(
    started: AgentWorkspaceSnapshotV1,
    finished: AgentWorkspaceSnapshotV1,
    definition: &AgentTaskDefinition,
    additional_observations: Vec<AgentHostEvidenceObservationV1>,
) -> AgentHostVerificationV1 {
    let mut observations = definition
        .evidence
        .iter()
        .filter_map(|gate| match gate {
            AgentEvidenceGate::Artifact(predicate) => {
                let gate_sha256 = evidence_gate_sha256(gate);
                let initial = started
                    .artifact_observations
                    .iter()
                    .find(|value| value.gate_sha256 == gate_sha256);
                let final_observation = finished
                    .artifact_observations
                    .iter()
                    .find(|value| value.gate_sha256 == gate_sha256);
                Some(AgentHostEvidenceObservationV1 {
                    gate_sha256,
                    authority: AgentVerificationAuthority::HostObservation,
                    satisfied: final_observation.is_some_and(|value| value.satisfied)
                        && (!predicate.must_change
                            || initial.and_then(|value| value.observation_sha256.as_ref())
                                != final_observation
                                    .and_then(|value| value.observation_sha256.as_ref())),
                    observation_sha256: final_observation
                        .and_then(|value| value.observation_sha256.clone()),
                })
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    for observation in additional_observations {
        if !observations
            .iter()
            .any(|known| known.gate_sha256 == observation.gate_sha256)
        {
            observations.push(observation);
        }
    }
    AgentHostVerificationV1 {
        started,
        finished,
        observations,
    }
}

fn host_verification_valid(verification: &AgentHostVerificationV1) -> bool {
    verification.started.workspace_identity_sha256.len() == 64
        && verification.started.source_revision_sha256.len() == 64
        && verification.finished.workspace_identity_sha256
            == verification.started.workspace_identity_sha256
        && verification.finished.source_revision_sha256
            == verification.started.source_revision_sha256
        && verification.observations.len() <= MAX_GRAPH_TOOLS
        && verification
            .observations
            .iter()
            .all(|observation| valid_sha256(&observation.gate_sha256))
        && verification
            .observations
            .iter()
            .map(|observation| &observation.gate_sha256)
            .collect::<HashSet<_>>()
            .len()
            == verification.observations.len()
}

#[allow(clippy::too_many_arguments)]
#[allow(dead_code)]
pub fn verify_agent_attempt(
    binding: Option<AgentGraphVerificationBindingV1>,
    task_id: TaskId,
    definition: &AgentTaskDefinition,
    attempt_number: u8,
    agent_id: Uuid,
    terminal_status: AgentAttemptStatus,
    terminal_event_sequence: Option<u64>,
    summary: Option<&str>,
    successful_tools: &[String],
) -> AgentVerificationReceiptV1 {
    verify_agent_attempt_with_host(
        binding,
        task_id,
        definition,
        attempt_number,
        agent_id,
        terminal_status,
        terminal_event_sequence,
        summary,
        successful_tools,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn verify_agent_attempt_with_host(
    binding: Option<AgentGraphVerificationBindingV1>,
    task_id: TaskId,
    definition: &AgentTaskDefinition,
    attempt_number: u8,
    agent_id: Uuid,
    terminal_status: AgentAttemptStatus,
    terminal_event_sequence: Option<u64>,
    summary: Option<&str>,
    successful_tools: &[String],
    host_verification: Option<&AgentHostVerificationV1>,
) -> AgentVerificationReceiptV1 {
    let contract_valid = binding.is_some_and(|binding| {
        definition
            .verification_contract
            .as_ref()
            .is_some_and(|contract| {
                contract.matches(
                    binding.plan_id,
                    binding.configuration_revision,
                    &definition.evidence,
                )
            })
    });
    let checks = if contract_valid {
        definition
            .evidence
            .iter()
            .map(|gate| {
                let gate_sha256 = evidence_gate_sha256(gate);
                let (authority, satisfied) = match gate {
                    AgentEvidenceGate::AssistantOutput => (
                        AgentVerificationAuthority::AssistantClaim,
                        summary.is_some_and(|value| !value.trim().is_empty()),
                    ),
                    AgentEvidenceGate::ToolSuccess { tool } => (
                        AgentVerificationAuthority::HostObservation,
                        successful_tools.contains(tool),
                    ),
                    AgentEvidenceGate::Artifact(_) | AgentEvidenceGate::ClosedCommand(_) => (
                        AgentVerificationAuthority::HostObservation,
                        host_verification.is_some_and(|verification| {
                            verification.observations.iter().any(|observation| {
                                observation.gate_sha256 == gate_sha256
                                    && observation.authority
                                        == AgentVerificationAuthority::HostObservation
                                    && observation.satisfied
                            })
                        }),
                    ),
                    AgentEvidenceGate::HumanReview => (
                        AgentVerificationAuthority::HumanReview,
                        host_verification.is_some_and(|verification| {
                            verification.observations.iter().any(|observation| {
                                observation.gate_sha256 == gate_sha256
                                    && observation.authority
                                        == AgentVerificationAuthority::HumanReview
                                    && observation.satisfied
                            })
                        }),
                    ),
                };
                AgentVerificationCheckV1 {
                    gate: None,
                    gate_sha256,
                    authority,
                    satisfied,
                }
            })
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let has_authoritative_observation = checks.iter().any(|check| {
        matches!(
            check.authority,
            AgentVerificationAuthority::HostObservation | AgentVerificationAuthority::HumanReview
        ) && check.satisfied
    });
    let host_context_valid = host_verification.is_some_and(host_verification_valid);
    let disposition = if terminal_status != AgentAttemptStatus::Completed {
        AgentVerificationDisposition::Fail
    } else if !contract_valid || !host_context_valid {
        AgentVerificationDisposition::Inconclusive
    } else if checks.iter().any(|check| !check.satisfied) {
        AgentVerificationDisposition::Fail
    } else if !has_authoritative_observation
        || terminal_event_sequence.is_none_or(|sequence| sequence == 0)
    {
        AgentVerificationDisposition::Inconclusive
    } else {
        AgentVerificationDisposition::Pass
    };
    let mut receipt = AgentVerificationReceiptV1 {
        version: VERIFICATION_RECEIPT_VERSION,
        plan_id: binding.map(|binding| binding.plan_id),
        configuration_revision: binding.map(|binding| binding.configuration_revision),
        approved_revision: binding.map(|binding| binding.approved_revision),
        task_id,
        attempt_number,
        agent_id,
        contract_sha256: verification_contract_sha256(task_id, definition),
        workspace_snapshot_sha256: host_verification
            .map(|verification| workspace_snapshot_sha256(&verification.started)),
        host_evidence_sha256: host_verification.map(host_verification_sha256),
        terminal_status,
        terminal_event_sequence,
        disposition,
        checks,
        receipt_sha256: String::new(),
    };
    receipt.receipt_sha256 = verification_receipt_sha256(&receipt);
    receipt
}

#[derive(Serialize)]
struct VerificationReceiptBinding<'a> {
    domain: &'static str,
    version: u32,
    plan_id: Option<PlanId>,
    configuration_revision: Option<u64>,
    approved_revision: Option<u64>,
    task_id: TaskId,
    attempt_number: u8,
    agent_id: Uuid,
    contract_sha256: &'a str,
    workspace_snapshot_sha256: &'a Option<String>,
    host_evidence_sha256: &'a Option<String>,
    terminal_status: AgentAttemptStatus,
    terminal_event_sequence: Option<u64>,
    disposition: AgentVerificationDisposition,
    checks: &'a [AgentVerificationCheckV1],
}

pub fn verification_receipt_sha256(receipt: &AgentVerificationReceiptV1) -> String {
    let binding = VerificationReceiptBinding {
        domain: "ca.agent-verification-receipt.v1",
        version: receipt.version,
        plan_id: receipt.plan_id,
        configuration_revision: receipt.configuration_revision,
        approved_revision: receipt.approved_revision,
        task_id: receipt.task_id,
        attempt_number: receipt.attempt_number,
        agent_id: receipt.agent_id,
        contract_sha256: &receipt.contract_sha256,
        workspace_snapshot_sha256: &receipt.workspace_snapshot_sha256,
        host_evidence_sha256: &receipt.host_evidence_sha256,
        terminal_status: receipt.terminal_status,
        terminal_event_sequence: receipt.terminal_event_sequence,
        disposition: receipt.disposition,
        checks: &receipt.checks,
    };
    let encoded = serde_json::to_vec(&binding)
        .expect("agent verification receipt contains only serializable values");
    format!("{:x}", Sha256::digest(encoded))
}

pub fn verification_receipt_valid(
    binding: Option<AgentGraphVerificationBindingV1>,
    task_id: TaskId,
    definition: &AgentTaskDefinition,
    attempt: &AgentTaskAttempt,
) -> bool {
    let Some(receipt) = attempt.verification_receipt.as_ref() else {
        return false;
    };
    let Some(binding) = binding else {
        return false;
    };
    if receipt.version != VERIFICATION_RECEIPT_VERSION
        || receipt.plan_id != Some(binding.plan_id)
        || receipt.configuration_revision != Some(binding.configuration_revision)
        || receipt.approved_revision != Some(binding.approved_revision)
        || receipt.task_id != task_id
        || receipt.attempt_number != attempt.number
        || Some(receipt.agent_id) != attempt.agent_id
        || receipt.terminal_event_sequence != attempt.terminal_event_sequence
        || receipt.workspace_snapshot_sha256
            != attempt
                .host_verification
                .as_ref()
                .map(|verification| workspace_snapshot_sha256(&verification.started))
        || receipt.host_evidence_sha256
            != attempt
                .host_verification
                .as_ref()
                .map(host_verification_sha256)
        || !receipt.terminal_status.terminal()
        || receipt.receipt_sha256 != verification_receipt_sha256(receipt)
    {
        return false;
    }
    let status_matches = match (receipt.terminal_status, receipt.disposition, attempt.status) {
        (
            AgentAttemptStatus::Completed,
            AgentVerificationDisposition::Pass,
            AgentAttemptStatus::Completed,
        )
        | (
            AgentAttemptStatus::Completed,
            AgentVerificationDisposition::Fail | AgentVerificationDisposition::Inconclusive,
            AgentAttemptStatus::EvidenceFailed,
        ) => true,
        (terminal, AgentVerificationDisposition::Fail, observed) => terminal == observed,
        _ => false,
    };
    if !status_matches {
        return false;
    }
    let expected = verify_agent_attempt_with_host(
        Some(binding),
        task_id,
        definition,
        attempt.number,
        receipt.agent_id,
        receipt.terminal_status,
        attempt.terminal_event_sequence,
        attempt.summary.as_deref(),
        &attempt.successful_tools,
        attempt.host_verification.as_ref(),
    );
    expected == *receipt
}

pub fn task_completion_verified(
    binding: Option<AgentGraphVerificationBindingV1>,
    task: &crate::work::PlanTask,
) -> bool {
    if task.status != crate::work::TaskStatus::Completed
        || task.agent_runtime.state != AgentTaskState::Completed
    {
        return false;
    }
    let Some(definition) = task.agent.as_ref() else {
        return false;
    };
    let Some(attempt) = task.agent_runtime.attempts.last() else {
        return false;
    };
    attempt.status == AgentAttemptStatus::Completed
        && attempt
            .verification_receipt
            .as_ref()
            .is_some_and(|receipt| {
                receipt.terminal_status == AgentAttemptStatus::Completed
                    && receipt.disposition == AgentVerificationDisposition::Pass
            })
        && verification_receipt_valid(binding, task.id, definition, attempt)
}

pub fn bounded_summary(value: &str) -> String {
    let value = crate::terminal_text::sanitize_terminal_text(value);
    if value.len() <= MAX_ATTEMPT_SUMMARY_BYTES {
        return value;
    }
    let mut end = MAX_ATTEMPT_SUMMARY_BYTES;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n[{} bytes omitted]", &value[..end], value.len() - end)
}

fn parse_task_id(raw: &str) -> Result<TaskId, AgentGraphError> {
    Uuid::parse_str(raw)
        .map(TaskId)
        .map_err(|_| AgentGraphError::InvalidTaskId(raw.to_string()))
}

fn normalize_names(values: Vec<String>, maximum: usize) -> Result<Vec<String>, AgentGraphError> {
    if values.len() > maximum {
        return Err(AgentGraphError::InvalidPolicy);
    }
    let mut result = Vec::new();
    for value in values {
        let value = normalize_name(&value)?;
        if !result.contains(&value) {
            result.push(value);
        }
    }
    Ok(result)
}

fn normalize_name(value: &str) -> Result<String, AgentGraphError> {
    let value = value.trim().to_ascii_lowercase();
    if value.is_empty()
        || value.len() > 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return Err(AgentGraphError::InvalidPolicy);
    }
    Ok(value)
}

fn normalize_scopes(values: Vec<String>) -> Result<Vec<String>, AgentGraphError> {
    if values.len() > MAX_GRAPH_SCOPES {
        return Err(AgentGraphError::InvalidPolicy);
    }
    let mut result = Vec::new();
    for value in values {
        let value = value.trim().to_string();
        if value.is_empty()
            || value.len() > 512
            || value.chars().any(char::is_control)
            || std::path::Path::new(&value).is_absolute()
        {
            return Err(AgentGraphError::InvalidPolicy);
        }
        if !result.contains(&value) {
            result.push(value);
        }
    }
    Ok(result)
}

fn normalize_relative_path(value: &str) -> Result<String, AgentGraphError> {
    let value = value.trim();
    let path = std::path::Path::new(value);
    if value.is_empty()
        || value.len() > 512
        || value.chars().any(char::is_control)
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Err(AgentGraphError::InvalidPolicy);
    }
    Ok(value.to_string())
}

fn normalize_artifact_predicate(
    mut predicate: AgentArtifactPredicateV1,
) -> Result<AgentArtifactPredicateV1, AgentGraphError> {
    predicate.path = normalize_relative_path(&predicate.path)?;
    if predicate
        .sha256
        .as_ref()
        .is_some_and(|sha256| !valid_sha256(sha256))
        || predicate
            .min_bytes
            .zip(predicate.max_bytes)
            .is_some_and(|(minimum, maximum)| minimum > maximum)
        || (predicate.kind != AgentArtifactKind::RegularFile
            && (predicate.sha256.is_some()
                || predicate.min_bytes.is_some()
                || predicate.max_bytes.is_some()))
    {
        return Err(AgentGraphError::InvalidPolicy);
    }
    Ok(predicate)
}

fn normalize_closed_command(
    mut predicate: AgentClosedCommandPredicateV1,
) -> Result<AgentClosedCommandPredicateV1, AgentGraphError> {
    let total_bytes = predicate.argv.iter().map(String::len).sum::<usize>();
    if predicate.argv.is_empty()
        || predicate.argv.len() > MAX_VERIFICATION_COMMAND_ARGS
        || total_bytes > 16 * 1024
        || predicate.timeout_secs == 0
        || predicate.timeout_secs > MAX_VERIFICATION_COMMAND_TIMEOUT_SECS
        || predicate.argv.iter().any(|argument| {
            argument.is_empty() || argument.len() > 4096 || argument.chars().any(char::is_control)
        })
        || !std::path::Path::new(&predicate.argv[0]).is_absolute()
    {
        return Err(AgentGraphError::InvalidPolicy);
    }
    predicate.cwd = predicate
        .cwd
        .as_deref()
        .map(normalize_relative_path)
        .transpose()?;
    Ok(predicate)
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn task_lane(definition: &AgentTaskDefinition) -> AgentWorkspaceLane {
    definition
        .worktree_id
        .map_or(AgentWorkspaceLane::Primary, AgentWorkspaceLane::Worktree)
}

fn depends_transitively(
    candidate: TaskId,
    prerequisite: TaskId,
    definitions: &HashMap<TaskId, AgentTaskDefinition>,
) -> bool {
    let mut pending = definitions
        .get(&candidate)
        .map(|definition| definition.depends_on.clone())
        .unwrap_or_default();
    let mut visited = HashSet::new();
    while let Some(task) = pending.pop() {
        if task == prerequisite {
            return true;
        }
        if visited.insert(task) {
            if let Some(definition) = definitions.get(&task) {
                pending.extend(definition.depends_on.iter().copied());
            }
        }
    }
    false
}

fn ensure_workspace_lanes(
    order: &[TaskId],
    definitions: &HashMap<TaskId, AgentTaskDefinition>,
    mutating_tools: &HashSet<String>,
) -> Result<(), AgentGraphError> {
    for task in order {
        let definition = &definitions[task];
        let lane = task_lane(definition);
        if definition.depends_on.iter().any(|dependency| {
            definitions
                .get(dependency)
                .is_some_and(|dependency| task_lane(dependency) != lane)
        }) && !definition.accept_cross_lane_handoff
        {
            return Err(AgentGraphError::CrossLaneHandoffRequired);
        }
    }
    let writers = order
        .iter()
        .copied()
        .filter(|task| {
            definitions[task]
                .grant
                .tools
                .iter()
                .any(|tool| mutating_tools.contains(tool))
        })
        .collect::<Vec<_>>();
    for (index, left) in writers.iter().copied().enumerate() {
        for right in writers.iter().copied().skip(index + 1) {
            if task_lane(&definitions[&left]) == task_lane(&definitions[&right])
                && !depends_transitively(left, right, definitions)
                && !depends_transitively(right, left, definitions)
            {
                return Err(AgentGraphError::UnserializedWorkspaceLane);
            }
        }
    }
    Ok(())
}

fn ensure_acyclic(
    order: &[TaskId],
    definitions: &HashMap<TaskId, AgentTaskDefinition>,
) -> Result<(), AgentGraphError> {
    let mut indegree = order
        .iter()
        .map(|id| (*id, 0usize))
        .collect::<HashMap<_, _>>();
    let mut dependents: HashMap<TaskId, Vec<TaskId>> = HashMap::new();
    for (task, definition) in definitions {
        indegree.insert(*task, definition.depends_on.len());
        for dependency in &definition.depends_on {
            dependents.entry(*dependency).or_default().push(*task);
        }
    }
    let mut queue = order
        .iter()
        .copied()
        .filter(|id| indegree.get(id) == Some(&0))
        .collect::<VecDeque<_>>();
    let mut visited = 0usize;
    while let Some(id) = queue.pop_front() {
        visited += 1;
        for dependent in dependents.get(&id).into_iter().flatten() {
            let entry = indegree.get_mut(dependent).expect("known task");
            *entry -= 1;
            if *entry == 0 {
                queue.push_back(*dependent);
            }
        }
    }
    if visited == order.len() {
        Ok(())
    } else {
        Err(AgentGraphError::Cycle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(value: u128) -> TaskId {
        TaskId(Uuid::from_u128(value))
    }

    fn plan_id() -> PlanId {
        PlanId(Uuid::from_u128(100))
    }

    fn binding() -> AgentGraphVerificationBindingV1 {
        AgentGraphVerificationBindingV1 {
            plan_id: plan_id(),
            configuration_revision: 2,
            approved_revision: 3,
        }
    }

    fn stable_host_verification() -> AgentHostVerificationV1 {
        let snapshot = AgentWorkspaceSnapshotV1 {
            workspace_identity_sha256: "1".repeat(64),
            source_revision_sha256: "2".repeat(64),
            artifact_observations: Vec::new(),
        };
        AgentHostVerificationV1 {
            started: snapshot.clone(),
            finished: snapshot,
            observations: Vec::new(),
        }
    }

    fn parse(
        task_ids: &[TaskId],
        inputs: Vec<AgentTaskInput>,
    ) -> Result<HashMap<TaskId, AgentTaskDefinition>, AgentGraphError> {
        parse_configuration(plan_id(), 2, task_ids, inputs)
    }

    fn budget_limits(
        max_physical_requests: u32,
        max_cost_usd: Option<&str>,
        allow_unpriced: bool,
    ) -> AgentGraphBudgetLimitsV1 {
        AgentGraphBudgetLimitsV1::from_input(AgentGraphBudgetInput {
            max_cost_usd: max_cost_usd.map(str::to_string),
            allow_unpriced,
            max_physical_requests,
        })
        .unwrap()
    }

    fn usage(prompt_tokens: u32, completion_tokens: u32) -> crate::agent::stream::UsageInfo {
        crate::agent::stream::UsageInfo {
            prompt_tokens,
            completion_tokens,
            total_tokens: prompt_tokens + completion_tokens,
            cached_prompt_tokens: None,
            cache_write_tokens: None,
            reasoning_tokens: None,
            response_metadata: None,
            cache_usage_override: None,
        }
    }

    fn input(task: TaskId, dependencies: &[TaskId]) -> AgentTaskInput {
        AgentTaskInput {
            task_id: task.to_string(),
            depends_on: dependencies.iter().map(ToString::to_string).collect(),
            tools: vec!["read_file".into()],
            paths: vec!["src".into()],
            commands: Vec::new(),
            network: false,
            worktree_id: None,
            accept_cross_lane_handoff: false,
            max_attempts: 2,
            evidence: vec![
                AgentEvidenceInput::AssistantOutput,
                AgentEvidenceInput::ToolSuccess {
                    tool: "read_file".into(),
                },
            ],
        }
    }

    #[test]
    fn configuration_rejects_unknown_self_duplicate_and_cycles() {
        let one = id(1);
        let two = id(2);
        assert!(matches!(
            parse(&[one], vec![input(one, &[one])]),
            Err(AgentGraphError::SelfDependency(_))
        ));
        assert!(matches!(
            parse(&[one], vec![input(one, &[two])]),
            Err(AgentGraphError::UnknownDependency { .. })
        ));
        assert!(matches!(
            parse(&[one, two], vec![input(one, &[two]), input(two, &[one])]),
            Err(AgentGraphError::Cycle)
        ));
        let shared = Uuid::from_u128(7).to_string();
        let mut first = input(one, &[]);
        first.worktree_id = Some(shared.clone());
        let mut second = input(two, &[]);
        second.worktree_id = Some(shared);
        assert!(parse(&[one, two], vec![first.clone(), second.clone()]).is_ok());
        first.tools.push("write_file".into());
        second.tools.push("write_file".into());
        assert!(matches!(
            parse(&[one, two], vec![first, second]),
            Err(AgentGraphError::UnserializedWorkspaceLane)
        ));
        let mut unavailable = input(one, &[]);
        unavailable.tools = vec!["not_a_tool".into()];
        assert!(matches!(
            parse(&[one], vec![unavailable]),
            Err(AgentGraphError::InvalidPolicy)
        ));
    }

    #[test]
    fn workspace_lanes_serialize_writers_and_require_explicit_cross_lane_handoffs() {
        let one = id(1);
        let two = id(2);
        let mut first = input(one, &[]);
        first.tools.push("write_file".into());
        let mut ordered = input(two, &[one]);
        ordered.tools.push("write_file".into());
        assert!(parse(&[one, two], vec![first.clone(), ordered]).is_ok());

        first.worktree_id = Some(Uuid::from_u128(7).to_string());
        let dependent = input(two, &[one]);
        assert!(matches!(
            parse(&[one, two], vec![first.clone(), dependent.clone()]),
            Err(AgentGraphError::CrossLaneHandoffRequired)
        ));
        let mut accepted = dependent;
        accepted.accept_cross_lane_handoff = true;
        assert!(parse(&[one, two], vec![first, accepted]).is_ok());
    }

    #[test]
    fn autonomy_budget_reserves_before_equality_and_survives_restore() {
        let limits = budget_limits(2, None, true);
        let budget =
            AgentAutonomyBudget::new(limits.clone(), AgentGraphBudgetUsageV1::default()).unwrap();
        budget.reserve_request().unwrap();
        budget.reserve_request().unwrap();
        assert_eq!(
            budget.reserve_request(),
            Err(AgentBudgetLimitKind::PhysicalRequests)
        );
        let usage = budget.snapshot();
        assert_eq!(usage.physical_requests, 2);
        assert_eq!(usage.blocked, Some(AgentBudgetLimitKind::PhysicalRequests));

        let restored = AgentAutonomyBudget::new(limits, usage).unwrap();
        assert_eq!(
            restored.reserve_request(),
            Err(AgentBudgetLimitKind::PhysicalRequests)
        );
        assert_eq!(restored.snapshot().physical_requests, 2);
    }

    #[test]
    fn autonomy_budget_fails_closed_on_unknown_usage_output_and_unpriced_routes() {
        let budget = AgentAutonomyBudget::new(
            budget_limits(3, None, true),
            AgentGraphBudgetUsageV1::default(),
        )
        .unwrap();
        budget.reserve_request().unwrap();
        assert_eq!(
            budget.observe_request(None, None),
            Err(AgentBudgetLimitKind::UnknownUsage)
        );

        let mut output_limits = budget_limits(3, None, true);
        output_limits.max_output_tokens_per_request = 3;
        let output_budget =
            AgentAutonomyBudget::new(output_limits, AgentGraphBudgetUsageV1::default()).unwrap();
        output_budget.reserve_request().unwrap();
        assert_eq!(
            output_budget.observe_request(Some(&usage(1, 4)), None),
            Err(AgentBudgetLimitKind::OutputTokensPerRequest)
        );

        let priced_only = AgentAutonomyBudget::new(
            budget_limits(3, Some("1"), false),
            AgentGraphBudgetUsageV1::default(),
        )
        .unwrap();
        priced_only.reserve_request().unwrap();
        assert_eq!(
            priced_only.observe_request(Some(&usage(1, 0)), None),
            Err(AgentBudgetLimitKind::UnpricedRoute)
        );
    }

    #[test]
    fn autonomy_budget_enforces_exact_cost_tools_failures_children_and_output() {
        let limits = budget_limits(3, Some("0.0000005"), false);
        let budget = AgentAutonomyBudget::new(limits, AgentGraphBudgetUsageV1::default()).unwrap();
        let pricing = crate::usage::UsagePricing {
            input_per_million_usd: "1".parse().unwrap(),
            output_per_million_usd: "1".parse().unwrap(),
            cached_input_per_million_usd: None,
            cache_write_input_per_million_usd: None,
            reasoning_output_per_million_usd: None,
        };
        budget.reserve_request().unwrap();
        assert_eq!(
            budget.observe_request(Some(&usage(1, 0)), Some(&pricing)),
            Err(AgentBudgetLimitKind::Cost)
        );

        let mut limits = budget_limits(3, None, true);
        limits.max_tool_calls = 1;
        limits.max_tool_failures = 1;
        limits.max_child_spawns = 1;
        limits.max_retained_output_bytes = 4;
        let operations =
            AgentAutonomyBudget::new(limits, AgentGraphBudgetUsageV1::default()).unwrap();
        operations.reserve_tool_call().unwrap();
        assert_eq!(
            operations.reserve_tool_call(),
            Err(AgentBudgetLimitKind::ToolCalls)
        );

        let mut limits = budget_limits(3, None, true);
        limits.max_tool_failures = 1;
        let failures =
            AgentAutonomyBudget::new(limits, AgentGraphBudgetUsageV1::default()).unwrap();
        failures.observe_tool_failure().unwrap();
        assert_eq!(
            failures.observe_tool_failure(),
            Err(AgentBudgetLimitKind::ToolFailures)
        );

        let mut limits = budget_limits(3, None, true);
        limits.max_child_spawns = 1;
        let children =
            AgentAutonomyBudget::new(limits, AgentGraphBudgetUsageV1::default()).unwrap();
        children.reserve_child_spawn().unwrap();
        assert_eq!(
            children.reserve_child_spawn(),
            Err(AgentBudgetLimitKind::ChildSpawns)
        );

        let mut limits = budget_limits(3, None, true);
        limits.max_retained_output_bytes = 4;
        let output = AgentAutonomyBudget::new(limits, AgentGraphBudgetUsageV1::default()).unwrap();
        output.retain_output(4).unwrap();
        assert_eq!(
            output.retain_output(1),
            Err(AgentBudgetLimitKind::RetainedOutput)
        );
    }

    #[test]
    fn artifact_verification_is_contained_change_bound_and_source_bound() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = crate::workspace::WorkspaceContext::discover(temp.path()).unwrap();
        let task = id(1);
        let predicate = AgentArtifactPredicateV1 {
            path: "result.txt".into(),
            kind: AgentArtifactKind::RegularFile,
            sha256: Some(format!("{:x}", Sha256::digest(b"done"))),
            min_bytes: Some(4),
            max_bytes: Some(4),
            must_change: true,
        };
        let gate = AgentEvidenceGate::Artifact(predicate);
        let definition = AgentTaskDefinition {
            depends_on: Vec::new(),
            grant: AgentTaskGrant::default(),
            worktree_id: None,
            accept_cross_lane_handoff: false,
            max_attempts: 1,
            evidence: vec![gate.clone()],
            verification_contract: Some(EvidenceContractV1::current(plan_id(), 2, vec![gate])),
        };
        let started = capture_workspace_snapshot(&workspace, &definition).unwrap();
        std::fs::write(temp.path().join("result.txt"), "done").unwrap();
        let finished = capture_workspace_snapshot(&workspace, &definition).unwrap();
        let verification =
            finish_workspace_verification(started, finished, &definition, Vec::new());
        let receipt = verify_agent_attempt_with_host(
            Some(binding()),
            task,
            &definition,
            1,
            Uuid::from_u128(20),
            AgentAttemptStatus::Completed,
            Some(1),
            None,
            &[],
            Some(&verification),
        );
        assert_eq!(receipt.disposition, AgentVerificationDisposition::Pass);

        let mut drifted = verification;
        drifted.finished.source_revision_sha256 = "3".repeat(64);
        let receipt = verify_agent_attempt_with_host(
            Some(binding()),
            task,
            &definition,
            1,
            Uuid::from_u128(20),
            AgentAttemptStatus::Completed,
            Some(1),
            None,
            &[],
            Some(&drifted),
        );
        assert_ne!(receipt.disposition, AgentVerificationDisposition::Pass);
    }

    #[cfg(unix)]
    #[test]
    fn artifact_verification_rejects_symbolic_links() {
        let temp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("result.txt"), "done").unwrap();
        std::os::unix::fs::symlink(outside.path(), temp.path().join("linked")).unwrap();
        let workspace = crate::workspace::WorkspaceContext::discover(temp.path()).unwrap();
        let gate = AgentEvidenceGate::Artifact(AgentArtifactPredicateV1 {
            path: "linked/result.txt".into(),
            kind: AgentArtifactKind::RegularFile,
            sha256: None,
            min_bytes: None,
            max_bytes: None,
            must_change: false,
        });
        let definition = AgentTaskDefinition {
            depends_on: Vec::new(),
            grant: AgentTaskGrant::default(),
            worktree_id: None,
            accept_cross_lane_handoff: false,
            max_attempts: 1,
            evidence: vec![gate],
            verification_contract: None,
        };
        assert_eq!(
            capture_workspace_snapshot(&workspace, &definition),
            Err(AgentHostVerificationError::ArtifactSymlink)
        );
    }

    #[test]
    fn valid_graph_preserves_ordered_edges_and_local_evidence() {
        let one = id(1);
        let two = id(2);
        let definitions = parse(&[one, two], vec![input(one, &[]), input(two, &[one])]).unwrap();
        assert_eq!(definitions[&two].depends_on, [one]);
        assert_eq!(
            definitions[&two].dependency_edges(two).collect::<Vec<_>>(),
            [AgentDependencyEdge {
                prerequisite: one,
                dependent: two,
            }]
        );
        let host = stable_host_verification();
        let receipt = verify_agent_attempt_with_host(
            Some(binding()),
            one,
            &definitions[&one],
            1,
            Uuid::from_u128(11),
            AgentAttemptStatus::Completed,
            Some(7),
            Some("done"),
            &["read_file".into()],
            Some(&host),
        );
        assert_eq!(receipt.disposition, AgentVerificationDisposition::Pass);
        assert_eq!(receipt.plan_id, Some(plan_id()));
        assert_eq!(receipt.configuration_revision, Some(2));
        assert_eq!(receipt.approved_revision, Some(3));
        assert_eq!(receipt.checks.len(), 2);
        assert_eq!(receipt.contract_sha256.len(), 64);
    }

    #[test]
    fn output_claim_without_host_observation_is_inconclusive() {
        let task = id(1);
        let mut definition = parse(&[task], vec![input(task, &[])])
            .unwrap()
            .remove(&task)
            .unwrap();
        definition.evidence = vec![AgentEvidenceGate::AssistantOutput];
        definition.verification_contract = Some(EvidenceContractV1::current(
            plan_id(),
            2,
            definition.evidence.clone(),
        ));

        let receipt = verify_agent_attempt(
            Some(binding()),
            task,
            &definition,
            1,
            Uuid::from_u128(12),
            AgentAttemptStatus::Completed,
            Some(8),
            Some("claimed success"),
            &[],
        );

        assert_eq!(
            receipt.disposition,
            AgentVerificationDisposition::Inconclusive
        );
        assert_eq!(
            receipt.checks[0].authority,
            AgentVerificationAuthority::AssistantClaim
        );
    }

    #[test]
    fn host_observation_without_terminal_event_is_inconclusive() {
        let task = id(1);
        let definition = parse(&[task], vec![input(task, &[])])
            .unwrap()
            .remove(&task)
            .unwrap();

        let receipt = verify_agent_attempt(
            Some(binding()),
            task,
            &definition,
            1,
            Uuid::from_u128(15),
            AgentAttemptStatus::Completed,
            None,
            Some("done"),
            &["read_file".into()],
        );

        assert_eq!(
            receipt.disposition,
            AgentVerificationDisposition::Inconclusive
        );
        assert!(receipt.checks.iter().any(|check| {
            check.authority == AgentVerificationAuthority::HostObservation && check.satisfied
        }));
    }

    #[test]
    fn receipt_round_trip_is_bounded_redacted_and_rejects_tampering() {
        let task = id(1);
        let definition = parse(&[task], vec![input(task, &[])])
            .unwrap()
            .remove(&task)
            .unwrap();
        let agent_id = Uuid::from_u128(13);
        let now = DateTime::from_timestamp(3, 0).unwrap();
        let secret = "receipt-secret-sentinel";
        let host = stable_host_verification();
        let receipt = verify_agent_attempt_with_host(
            Some(binding()),
            task,
            &definition,
            1,
            agent_id,
            AgentAttemptStatus::Completed,
            Some(9),
            Some(secret),
            &["read_file".into()],
            Some(&host),
        );
        assert_eq!(
            receipt,
            verify_agent_attempt_with_host(
                Some(binding()),
                task,
                &definition,
                1,
                agent_id,
                AgentAttemptStatus::Completed,
                Some(9),
                Some(secret),
                &["read_file".into()],
                Some(&host),
            )
        );
        let encoded = serde_json::to_vec(&receipt).unwrap();
        assert!(encoded.len() < 4096);
        assert!(!String::from_utf8_lossy(&encoded).contains(secret));
        assert_eq!(receipt.receipt_sha256.len(), 64);
        let decoded: AgentVerificationReceiptV1 = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded, receipt);

        let mut attempt = AgentTaskAttempt {
            number: 1,
            agent_id: Some(agent_id),
            status: AgentAttemptStatus::Completed,
            started_at: now,
            finished_at: Some(now),
            successful_tools: vec!["read_file".into()],
            summary: Some(secret.into()),
            terminal_event_sequence: Some(9),
            verification_receipt: Some(receipt),
            workspace_snapshot: Some(host.started.clone()),
            host_verification: Some(host),
            budget_usage: AgentGraphBudgetUsageV1::default(),
        };
        assert!(verification_receipt_valid(
            Some(binding()),
            task,
            &definition,
            &attempt
        ));
        let mut revision_tampered = attempt.clone();
        let receipt = revision_tampered.verification_receipt.as_mut().unwrap();
        receipt.approved_revision = Some(4);
        receipt.receipt_sha256 = verification_receipt_sha256(receipt);
        assert!(!verification_receipt_valid(
            Some(binding()),
            task,
            &definition,
            &revision_tampered
        ));
        attempt
            .verification_receipt
            .as_mut()
            .unwrap()
            .contract_sha256 = "0".repeat(64);
        let tampered = attempt.verification_receipt.as_mut().unwrap();
        let tampered_sha256 = verification_receipt_sha256(tampered);
        tampered.receipt_sha256 = tampered_sha256;
        assert!(!verification_receipt_valid(
            Some(binding()),
            task,
            &definition,
            &attempt
        ));
    }

    #[test]
    fn legacy_contract_and_attempt_fields_default_without_becoming_verified() {
        let task = id(1);
        let definition = parse(&[task], vec![input(task, &[])])
            .unwrap()
            .remove(&task)
            .unwrap();
        let mut value = serde_json::to_value(&definition).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .remove("verification_contract");
        let legacy: AgentTaskDefinition = serde_json::from_value(value).unwrap();
        assert!(legacy.verification_contract.is_none());
        let receipt = verify_agent_attempt(
            Some(binding()),
            task,
            &legacy,
            1,
            Uuid::from_u128(14),
            AgentAttemptStatus::Completed,
            None,
            Some("legacy claim"),
            &["read_file".into()],
        );
        assert_eq!(
            receipt.disposition,
            AgentVerificationDisposition::Inconclusive
        );
        assert!(receipt.checks.is_empty());

        let attempt: AgentTaskAttempt = serde_json::from_value(serde_json::json!({
            "number": 1,
            "agent_id": Uuid::from_u128(14),
            "status": "completed",
            "started_at": DateTime::from_timestamp(4, 0).unwrap(),
            "finished_at": DateTime::from_timestamp(4, 0).unwrap(),
            "successful_tools": ["read_file"],
            "summary": "legacy claim"
        }))
        .unwrap();
        assert!(attempt.terminal_event_sequence.is_none());
        assert!(attempt.verification_receipt.is_none());

        let graph: AgentGraphState = serde_json::from_value(serde_json::json!({
            "status": "paused",
            "updated_at": null,
            "notice": "legacy"
        }))
        .unwrap();
        assert!(graph.plan_id.is_none());
        assert!(graph.configuration_revision.is_none());
        assert!(graph.approved_revision.is_none());
        assert!(graph.verification_binding().is_none());

        let current = verify_agent_attempt(
            Some(binding()),
            task,
            &definition,
            1,
            Uuid::from_u128(14),
            AgentAttemptStatus::Completed,
            Some(10),
            Some("done"),
            &["read_file".into()],
        );
        let mut legacy_receipt = serde_json::to_value(&current).unwrap();
        let legacy_receipt = legacy_receipt.as_object_mut().unwrap();
        legacy_receipt.remove("plan_id");
        legacy_receipt.remove("configuration_revision");
        legacy_receipt.remove("approved_revision");
        let legacy_receipt: AgentVerificationReceiptV1 =
            serde_json::from_value(serde_json::Value::Object(legacy_receipt.clone())).unwrap();
        assert!(legacy_receipt.plan_id.is_none());
        assert!(legacy_receipt.configuration_revision.is_none());
        assert!(legacy_receipt.approved_revision.is_none());
        let legacy_attempt = AgentTaskAttempt {
            number: 1,
            agent_id: Some(Uuid::from_u128(14)),
            status: AgentAttemptStatus::Completed,
            started_at: DateTime::from_timestamp(5, 0).unwrap(),
            finished_at: Some(DateTime::from_timestamp(5, 0).unwrap()),
            successful_tools: vec!["read_file".into()],
            summary: Some("done".into()),
            terminal_event_sequence: Some(10),
            verification_receipt: Some(legacy_receipt),
            workspace_snapshot: None,
            host_verification: None,
            budget_usage: AgentGraphBudgetUsageV1::default(),
        };
        assert!(!verification_receipt_valid(
            Some(binding()),
            task,
            &definition,
            &legacy_attempt
        ));
    }

    #[test]
    fn bounded_summary_is_terminal_safe_and_utf8_safe() {
        let value = format!("\u{1b}[31m{}", "é".repeat(MAX_ATTEMPT_SUMMARY_BYTES));
        let bounded = bounded_summary(&value);
        assert!(!bounded.contains('\u{1b}'));
        assert!(bounded.contains("bytes omitted"));
        assert!(bounded.is_char_boundary(bounded.len()));
    }
}
