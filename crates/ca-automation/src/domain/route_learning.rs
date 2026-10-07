//! Route learning and explainable lane scorecards.
//!
//! Owns advisory `ExecutionLane` scorecards and route learning fingerprints
//! (positive and negative via `outcome`). Scorecards explain cheaper/safer
//! lanes; live soft-gating is opt-in via `[routing].honor_scorecard_soft_gate`.
//!
//! Read when changing scorecard features or learning dedupe. Do not read for
//! hard-blocking Worker launches (except documented acceptance stops elsewhere).

use serde::{Deserialize, Serialize};

use super::route::ExecutionLane;
use super::task::TaskSpec;

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub(crate) struct TaskRouteProfile {
    pub(crate) fingerprint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) task_id: Option<String>,
    pub(crate) kind: String,
    pub(crate) docs_only: bool,
    pub(crate) safe_command_only: bool,
    pub(crate) has_verification: bool,
    pub(crate) requires_network: bool,
    pub(crate) allowed_file_count: usize,
    pub(crate) review_tier: String,
    pub(crate) automation_level: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) failure_fingerprint: Option<String>,
    pub(crate) estimated_prompt_tokens: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub(crate) struct RouteScoreSignal {
    pub(crate) lane: ExecutionLane,
    pub(crate) weight: i32,
    pub(crate) reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub(crate) struct RouteLaneScore {
    pub(crate) lane: ExecutionLane,
    pub(crate) score: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub(crate) struct RouteScorecard {
    pub(crate) profile_fingerprint: String,
    pub(crate) recommended_lane: ExecutionLane,
    pub(crate) scores: Vec<RouteLaneScore>,
    pub(crate) signals: Vec<RouteScoreSignal>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub(crate) struct PositiveRouteLearning {
    pub(crate) observed_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) task_id: Option<String>,
    pub(crate) profile_fingerprint: String,
    pub(crate) lane: ExecutionLane,
    /// Success outcomes boost the lane; `failed` / `fail*` subtract weight.
    pub(crate) outcome: String,
    pub(crate) summary: String,
}

pub(crate) fn is_negative_route_outcome(outcome: &str) -> bool {
    let lower = outcome.to_ascii_lowercase();
    lower == "failed" || lower.starts_with("fail") || lower == "blocked" || lower == "error"
}

pub(crate) fn task_route_profile(
    task: &TaskSpec,
    safe_command_only: bool,
    failure_fingerprint: Option<String>,
    estimated_prompt_tokens: usize,
) -> TaskRouteProfile {
    let docs_only = !task.allowed_files.is_empty()
        && task.allowed_files.iter().all(|path| {
            let lower = path.to_ascii_lowercase();
            lower.ends_with(".md")
                || lower.starts_with("docs/")
                || lower.starts_with(".agent/")
                || lower == "tracking docs"
                || lower == "next_steps.md"
        });
    let has_verification = !task.tests_to_run.is_empty();
    let fingerprint_seed = [
        task.kind.as_str(),
        if docs_only { "docs" } else { "source" },
        if safe_command_only { "command" } else { "edit" },
        if has_verification {
            "verified"
        } else {
            "unverified"
        },
        if task.requires_network {
            "network"
        } else {
            "local"
        },
        &format!("{:?}", task.review_tier),
        &format!("{:?}", task.automation_level),
        failure_fingerprint.as_deref().unwrap_or("no_failure"),
    ]
    .join("|");
    TaskRouteProfile {
        fingerprint: stable_route_hash(&fingerprint_seed),
        task_id: Some(task.task_id.clone()),
        kind: task.kind.clone(),
        docs_only,
        safe_command_only,
        has_verification,
        requires_network: task.requires_network,
        allowed_file_count: task.allowed_files.len(),
        review_tier: format!("{:?}", task.review_tier),
        automation_level: format!("{:?}", task.automation_level),
        failure_fingerprint,
        estimated_prompt_tokens,
    }
}

pub(crate) fn score_route_lanes(
    profile: &TaskRouteProfile,
    positive_matches: &[PositiveRouteLearning],
    cooldown_roles: &[String],
) -> RouteScorecard {
    let lanes = [
        ExecutionLane::RustEvidence,
        ExecutionLane::RustBookkeeping,
        ExecutionLane::SeniorDecision,
        ExecutionLane::SharedWorker,
        ExecutionLane::PmDirection,
        ExecutionLane::Stop,
    ];
    let mut scores = lanes
        .iter()
        .copied()
        .map(|lane| RouteLaneScore { lane, score: 0 })
        .collect::<Vec<_>>();
    let mut signals = Vec::new();
    let mut add = |lane: ExecutionLane, weight: i32, reason: &str| {
        if let Some(score) = scores.iter_mut().find(|score| score.lane == lane) {
            score.score += weight;
        }
        signals.push(RouteScoreSignal {
            lane,
            weight,
            reason: reason.to_string(),
        });
    };

    if profile.safe_command_only {
        add(
            ExecutionLane::RustEvidence,
            60,
            "safe command-only evidence can run in Rust",
        );
        add(
            ExecutionLane::SharedWorker,
            -25,
            "Worker would spend tokens on deterministic command execution",
        );
    }
    if profile.docs_only {
        add(
            ExecutionLane::RustBookkeeping,
            30,
            "docs/tracking-only work may be Rust bookkeeping",
        );
        if profile.has_verification {
            add(
                ExecutionLane::RustEvidence,
                45,
                "docs-only verified tasks prefer Rust evidence over Worker",
            );
            add(
                ExecutionLane::SharedWorker,
                -20,
                "docs-only verification should not spend Worker tokens",
            );
        }
    }
    if !profile.has_verification {
        add(
            ExecutionLane::PmDirection,
            25,
            "task has no verification commands in its contract",
        );
    }
    if profile.requires_network {
        add(
            ExecutionLane::PmDirection,
            20,
            "task declares network access and needs planning confirmation",
        );
    }
    if profile.failure_fingerprint.is_some() {
        add(
            ExecutionLane::SeniorDecision,
            35,
            "failure fingerprint exists and may need bounded repair judgment",
        );
    }
    if profile.estimated_prompt_tokens > 12_000 {
        add(
            ExecutionLane::RustBookkeeping,
            15,
            "large prompt estimate favors Rust-owned compacting before a role call",
        );
    }
    for learning in positive_matches
        .iter()
        .filter(|learning| learning.profile_fingerprint == profile.fingerprint)
    {
        if is_negative_route_outcome(&learning.outcome) {
            add(
                learning.lane,
                -25,
                "similar past route failed with this lane",
            );
        } else {
            add(
                learning.lane,
                30,
                "similar past route succeeded with this lane",
            );
        }
    }
    for role in cooldown_roles {
        match role.as_str() {
            "worker" | "Worker" => add(
                ExecutionLane::SharedWorker,
                -30,
                "recent Worker tool health has cooldown advisory",
            ),
            "planner" | "Project Manager" => add(
                ExecutionLane::PmDirection,
                -30,
                "recent Project Manager tool health has cooldown advisory",
            ),
            "foreman" | "foreman-review" | "Senior Dev" => add(
                ExecutionLane::SeniorDecision,
                -30,
                "recent Senior tool health has cooldown advisory",
            ),
            _ => {}
        }
    }

    scores.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| lane_rank(left.lane).cmp(&lane_rank(right.lane)))
    });
    let recommended_lane = scores
        .first()
        .map(|score| score.lane)
        .unwrap_or(ExecutionLane::Stop);
    RouteScorecard {
        profile_fingerprint: profile.fingerprint.clone(),
        recommended_lane,
        scores,
        signals,
    }
}

pub(crate) fn dedupe_positive_learnings(
    learnings: Vec<PositiveRouteLearning>,
) -> Vec<PositiveRouteLearning> {
    let mut by_key = std::collections::BTreeMap::new();
    for learning in learnings {
        let key = format!(
            "{}|{:?}|{}",
            learning.profile_fingerprint, learning.lane, learning.outcome
        );
        by_key.insert(key, learning);
    }
    by_key.into_values().collect()
}

fn lane_rank(lane: ExecutionLane) -> usize {
    match lane {
        ExecutionLane::RustEvidence => 0,
        ExecutionLane::RustBookkeeping => 1,
        ExecutionLane::SeniorDecision => 2,
        ExecutionLane::SharedWorker => 3,
        ExecutionLane::PmDirection => 4,
        ExecutionLane::FutureWorktreeWorker => 5,
        ExecutionLane::Stop => 6,
    }
}

pub(crate) fn stable_route_hash(text: &str) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{default_discovery_budget, AutomationLevel, ReviewTier};

    fn task() -> TaskSpec {
        TaskSpec {
            task_id: "TASK-1".to_string(),
            parent_task: None,
            status: "approved".to_string(),
            priority: "normal".to_string(),
            review_tier: ReviewTier::AiReviewOnFailure,
            automation_level: AutomationLevel::AiWorkerRustReview,
            dependencies: Vec::new(),
            title: "Capture evidence".to_string(),
            kind: "evidence".to_string(),
            objective: "Run deterministic evidence".to_string(),
            why_unblocks_progress: "needed".to_string(),
            allowed_files: vec!["NEXT_STEPS.md".to_string()],
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
        }
    }

    #[test]
    fn route_learning_prefers_rust_for_command_only_evidence() {
        let profile = task_route_profile(&task(), true, None, 500);

        let scorecard = score_route_lanes(&profile, &[], &[]);

        assert_eq!(scorecard.recommended_lane, ExecutionLane::RustEvidence);
    }

    #[test]
    fn route_learning_uses_stable_tie_order() {
        let mut task = task();
        task.tests_to_run.clear();
        task.allowed_files = vec!["src/lib.rs".to_string()];
        let profile = task_route_profile(&task, false, None, 500);

        let scorecard = score_route_lanes(&profile, &[], &[]);

        assert_eq!(scorecard.recommended_lane, ExecutionLane::PmDirection);
        assert_eq!(scorecard.profile_fingerprint, profile.fingerprint);
    }

    #[test]
    fn route_learning_deduplicates_positive_memory_by_profile_lane_outcome() {
        let learning = PositiveRouteLearning {
            observed_at: "2026-07-06T00:00:00Z".to_string(),
            task_id: Some("TASK-1".to_string()),
            profile_fingerprint: "abc".to_string(),
            lane: ExecutionLane::SharedWorker,
            outcome: "verified".to_string(),
            summary: "ok".to_string(),
        };

        let deduped = dedupe_positive_learnings(vec![learning.clone(), learning]);

        assert_eq!(deduped.len(), 1);
    }

    #[test]
    fn route_learning_penalizes_failed_lane_memory() {
        let profile = task_route_profile(&task(), true, None, 500);
        let negative = PositiveRouteLearning {
            observed_at: "2026-07-14T00:00:00Z".to_string(),
            task_id: Some("TASK-1".to_string()),
            profile_fingerprint: profile.fingerprint.clone(),
            lane: ExecutionLane::SharedWorker,
            outcome: "failed".to_string(),
            summary: "provider unreachable".to_string(),
        };

        let scorecard = score_route_lanes(&profile, &[negative], &[]);

        assert!(
            scorecard
                .scores
                .iter()
                .any(|score| score.lane == ExecutionLane::SharedWorker && score.score < 0),
            "failed SharedWorker memory should subtract score"
        );
        assert!(scorecard.signals.iter().any(|signal| {
            signal.reason.contains("failed") && signal.lane == ExecutionLane::SharedWorker
        }));
    }

    #[test]
    fn route_learning_prefers_rust_for_docs_only_verified_tasks() {
        let mut task = task();
        task.allowed_files = vec!["docs/HERMES.md".to_string()];
        task.kind = "docs_only".to_string();
        let profile = task_route_profile(&task, false, None, 500);
        assert!(profile.docs_only);

        let scorecard = score_route_lanes(&profile, &[], &[]);

        assert_eq!(scorecard.recommended_lane, ExecutionLane::RustEvidence);
    }
}
