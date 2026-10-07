//! Task policy domain types.
//!
//! Owns review tiers, automation levels, risk levels, and the typed policy
//! record Rust uses to validate work before calling AI. Policy types should stay
//! pure data: they do not inspect the filesystem, parse Markdown, execute
//! commands, or call Codex.
//!
//! Read when changing task policy shape, risk names, or serde defaults. Do not
//! read for diff scanning, queue parsing, or verification evidence.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub(crate) enum ReviewTier {
    // RustOnly and AutoAcceptIfVerified still require clean evidence/scope; they
    // do not mean "trust Worker." They mean "trust Rust when all checks pass."
    RustOnly,
    AutoAcceptIfVerified,
    AiReviewOnFailure,
    #[default]
    AlwaysAiReview,
    HumanReview,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub(crate) enum AutomationLevel {
    RustOnly,
    RustPatch,
    AiWorkerRustReview,
    #[default]
    AiWorkerAiReview,
    PlanningOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct TaskPolicy {
    // Typed policy is the long-term replacement for prose-only guardrails in task
    // cards. Rust validates this before Worker sees the task.
    pub(crate) allowed_file_patterns: Vec<String>,
    pub(crate) forbidden_file_patterns: Vec<String>,
    pub(crate) max_diff_lines: Option<usize>,
    pub(crate) allow_network: bool,
    pub(crate) allow_dependency_changes: bool,
    pub(crate) allow_lockfile_changes: bool,
    pub(crate) allow_public_api_changes: bool,
    pub(crate) review_tier: ReviewTier,
    pub(crate) automation_level: AutomationLevel,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

impl RiskLevel {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Critical => "critical",
        }
    }
}
