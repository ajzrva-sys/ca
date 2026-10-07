//! Route attempt domain types.
//!
//! These structs are compact durable vocabulary for orchestration routing:
//! which lane Rust chose, what feedback came back, and whether a later watcher
//! can identify avoidable spend. Keep this module pure: no filesystem IO,
//! process execution, or Codex invocation.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq, Hash)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RouteLifecycleStatus {
    Preparing,
    Running,
    Succeeded,
    Failed,
    TimedOut,
    Cancelled,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq, Hash)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ExecutionLane {
    RustEvidence,
    RustBookkeeping,
    SharedWorker,
    FutureWorktreeWorker,
    SeniorDecision,
    PmDirection,
    Stop,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq, Hash)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RouteRegretKind {
    AvoidableWorkerCommand,
    PmWorkerDoubleContext,
    RepeatedFingerprintNoProgress,
    PlannerDueEmptyQueue,
    SeniorAfterRustAutoacceptable,
    RedundantExploration,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq, Default)]
pub(crate) struct TokenUsage {
    #[serde(default)]
    pub(crate) input: usize,
    #[serde(default)]
    pub(crate) output: usize,
    #[serde(default)]
    pub(crate) reasoning: usize,
    #[serde(default)]
    pub(crate) total: usize,
    #[serde(default)]
    pub(crate) cache_read: usize,
    #[serde(default)]
    pub(crate) cache_write: usize,
}

impl TokenUsage {
    pub(crate) fn new(
        input: usize,
        output: usize,
        reasoning: usize,
        cache_read: usize,
        cache_write: usize,
    ) -> Self {
        Self {
            input,
            output,
            reasoning,
            total: input + output + reasoning,
            cache_read,
            cache_write,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub(crate) struct RouteFeedback {
    pub(crate) outcome: String,
    pub(crate) summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) regret_kind: Option<RouteRegretKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) better_lane: Option<ExecutionLane>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) verification_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) commit_sha: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) duration_ms: Option<u128>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub(crate) struct RouteAttempt {
    pub(crate) id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) task_id: Option<String>,
    pub(crate) attempt: u32,
    pub(crate) status: RouteLifecycleStatus,
    pub(crate) started_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) finished_at: Option<String>,
    pub(crate) execution_lane: ExecutionLane,
    pub(crate) route_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) model_tier: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) prompt_receipt_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) evidence_receipt_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) failure_fingerprint: Option<String>,
    #[serde(default)]
    pub(crate) token_usage: TokenUsage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) scope_group: Option<String>,
    #[serde(default)]
    pub(crate) scope_overlap: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) final_feedback: Option<RouteFeedback>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub(crate) struct RouteOutcome {
    pub(crate) attempt_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) task_id: Option<String>,
    pub(crate) status: RouteLifecycleStatus,
    pub(crate) execution_lane: ExecutionLane,
    pub(crate) route_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) model_tier: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) model: Option<String>,
    #[serde(default)]
    pub(crate) token_usage: TokenUsage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) failure_fingerprint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) regret_kind: Option<RouteRegretKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) better_lane: Option<ExecutionLane>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) final_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) finished_at: Option<String>,
}

impl RouteAttempt {
    pub(crate) fn outcome(&self) -> RouteOutcome {
        RouteOutcome {
            attempt_id: self.id.clone(),
            task_id: self.task_id.clone(),
            status: self.status,
            execution_lane: self.execution_lane,
            route_kind: self.route_kind.clone(),
            model_tier: self.model_tier.clone(),
            model: self.model.clone(),
            token_usage: self.token_usage,
            failure_fingerprint: self.failure_fingerprint.clone(),
            regret_kind: self
                .final_feedback
                .as_ref()
                .and_then(|feedback| feedback.regret_kind),
            better_lane: self
                .final_feedback
                .as_ref()
                .and_then(|feedback| feedback.better_lane),
            final_summary: self
                .final_feedback
                .as_ref()
                .map(|feedback| feedback.summary.clone()),
            finished_at: self.finished_at.clone(),
        }
    }
}

#[allow(dead_code)]
pub(crate) fn route_transition_allowed(
    from: RouteLifecycleStatus,
    to: RouteLifecycleStatus,
) -> bool {
    use RouteLifecycleStatus::*;
    match (from, to) {
        (Preparing, Running | Cancelled | Failed) => true,
        (Running, Succeeded | Failed | TimedOut | Cancelled) => true,
        (Succeeded | Failed | TimedOut | Cancelled, _) => false,
        (Preparing, Preparing) | (Running, Running) => true,
        _ => false,
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum RouteDecisionAction {
    RunRustEvidence,
    RetryWorker,
    SeniorRepair,
    SkipPlanner,
    Planner,
    Stop,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct RouteDecision {
    pub(crate) action: RouteDecisionAction,
    pub(crate) reason: String,
    pub(crate) regret_kind: Option<RouteRegretKind>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct RouteRetryContext {
    pub(crate) safe_command_only_evidence: bool,
    pub(crate) queue_has_approved_work: bool,
    pub(crate) same_failure_fingerprint: bool,
    pub(crate) fingerprint_retry_count: u32,
    pub(crate) failure_count: u32,
    pub(crate) max_task_failures: u32,
    pub(crate) verification_improved: bool,
    pub(crate) scope_overlap: bool,
}

pub(crate) fn decide_active_regret_route(context: &RouteRetryContext) -> RouteDecision {
    if context.safe_command_only_evidence {
        return RouteDecision {
            action: RouteDecisionAction::RunRustEvidence,
            reason: "safe command-only evidence should run in Rust before Worker".to_string(),
            regret_kind: Some(RouteRegretKind::AvoidableWorkerCommand),
        };
    }
    if context.queue_has_approved_work {
        return RouteDecision {
            action: RouteDecisionAction::SkipPlanner,
            reason: "approved or recoverable backlog should bypass stale planner_due".to_string(),
            regret_kind: Some(RouteRegretKind::PlannerDueEmptyQueue),
        };
    }
    if context.same_failure_fingerprint
        && context.fingerprint_retry_count > 1
        && !context.verification_improved
    {
        let action = if context.fingerprint_retry_count > context.max_task_failures
            || context.failure_count > context.max_task_failures
        {
            RouteDecisionAction::Stop
        } else {
            RouteDecisionAction::SeniorRepair
        };
        return RouteDecision {
            action,
            reason: "same failure fingerprint repeated without verification improvement"
                .to_string(),
            regret_kind: Some(RouteRegretKind::RepeatedFingerprintNoProgress),
        };
    }
    if context.scope_overlap {
        return RouteDecision {
            action: RouteDecisionAction::SeniorRepair,
            reason: "scope overlap needs Senior sequencing before another Worker attempt"
                .to_string(),
            regret_kind: None,
        };
    }
    RouteDecision {
        action: RouteDecisionAction::RetryWorker,
        reason: "no active regret rule blocked retry".to_string(),
        regret_kind: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_attempt_lifecycle_transitions_are_bounded() {
        assert!(route_transition_allowed(
            RouteLifecycleStatus::Preparing,
            RouteLifecycleStatus::Running
        ));
        assert!(route_transition_allowed(
            RouteLifecycleStatus::Running,
            RouteLifecycleStatus::Succeeded
        ));
        assert!(!route_transition_allowed(
            RouteLifecycleStatus::Succeeded,
            RouteLifecycleStatus::Running
        ));
    }

    #[test]
    fn route_regret_decision_stops_repeated_fingerprint_without_progress() {
        let decision = decide_active_regret_route(&RouteRetryContext {
            safe_command_only_evidence: false,
            queue_has_approved_work: false,
            same_failure_fingerprint: true,
            fingerprint_retry_count: 3,
            failure_count: 3,
            max_task_failures: 2,
            verification_improved: false,
            scope_overlap: false,
        });
        assert_eq!(decision.action, RouteDecisionAction::Stop);
        assert_eq!(
            decision.regret_kind,
            Some(RouteRegretKind::RepeatedFingerprintNoProgress)
        );
    }

    #[test]
    fn route_regret_decision_routes_command_only_to_rust() {
        let decision = decide_active_regret_route(&RouteRetryContext {
            safe_command_only_evidence: true,
            queue_has_approved_work: false,
            same_failure_fingerprint: false,
            fingerprint_retry_count: 0,
            failure_count: 0,
            max_task_failures: 2,
            verification_improved: false,
            scope_overlap: false,
        });
        assert_eq!(decision.action, RouteDecisionAction::RunRustEvidence);
        assert_eq!(
            decision.regret_kind,
            Some(RouteRegretKind::AvoidableWorkerCommand)
        );
    }
}
