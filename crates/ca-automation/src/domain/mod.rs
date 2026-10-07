//! Canonical domain vocabulary.
//!
//! Owns shared task, policy, event, route, acceptance, derived-status,
//! route-learning, and tool-health types extracted from the conductor. Domain
//! code should stay boring: serde/std basics only, no file IO, process
//! execution, Codex calls, or terminal rendering.
//!
//! Read when changing persisted type shapes or default serde behavior. Do not
//! read for orchestration flow, command execution, or prompt text.
//! See `src/domain/README.md`.

pub mod acceptance;
pub mod derived_status;
pub mod error_kind;
pub mod event;
pub mod policy;
pub mod role;
pub mod route;
pub mod route_learning;
pub mod task;
pub mod tool_health;

pub(crate) use acceptance::{acceptance_contract_from_task, AcceptanceContract};
pub(crate) use derived_status::{
    derive_status, DerivedStatus, DerivedStatusInput, DerivedStatusKind,
};
pub(crate) use event::{Event, OrchestratorEvent, OrchestratorEventKind};
pub(crate) use policy::{AutomationLevel, ReviewTier, RiskLevel, TaskPolicy};
pub(crate) use route::{
    decide_active_regret_route, ExecutionLane, RouteAttempt, RouteDecisionAction, RouteFeedback,
    RouteLifecycleStatus, RouteOutcome, RouteRegretKind, RouteRetryContext, TokenUsage,
};
pub(crate) use route_learning::{
    dedupe_positive_learnings, score_route_lanes, task_route_profile, PositiveRouteLearning,
    RouteScorecard, TaskRouteProfile,
};
pub(crate) use task::{default_discovery_budget, default_task_priority, TaskSpec};
pub(crate) use tool_health::{
    classify_tool_failure, compact_tool_excerpt, suggest_fallback_provider,
    summarize_tool_cooldowns, ToolCooldownSummary, ToolFailureKind, ToolHealthRecord,
    ToolHealthStatus,
};
