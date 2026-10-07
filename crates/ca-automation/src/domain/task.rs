//! Task domain types.
//!
//! Owns `TaskSpec` and serde defaults for task cards. Keep Markdown
//! parsing/rendering in `queue`, execution in `verification`/`app`, and role
//! prompt behavior in `roles`.
//!
//! Read when changing task schema fields or backwards-compatible defaults. Do
//! not read for queue Markdown parsing, Worker execution, or commit behavior.

use serde::{Deserialize, Serialize};

use super::policy::{AutomationLevel, ReviewTier};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct TaskSpec {
    // TaskSpec is the work-order contract between Planner/Rust/Worker. Existing
    // Markdown tasks may omit newer routing fields; serde defaults keep old task
    // cards valid while Rust infers conservative behavior.
    pub(crate) task_id: String,
    #[serde(default)]
    pub(crate) parent_task: Option<String>,
    #[serde(default = "default_task_status")]
    pub(crate) status: String,
    #[serde(default = "default_task_priority")]
    pub(crate) priority: String,
    #[serde(default)]
    pub(crate) review_tier: ReviewTier,
    #[serde(default)]
    pub(crate) automation_level: AutomationLevel,
    #[serde(default)]
    pub(crate) dependencies: Vec<String>,
    pub(crate) title: String,
    pub(crate) kind: String,
    pub(crate) objective: String,
    pub(crate) why_unblocks_progress: String,
    pub(crate) allowed_files: Vec<String>,
    // Guard checks use allowed_files as the commit/review boundary. Expansions
    // such as "tracking docs" are resolved by Rust before matching paths.
    #[serde(default)]
    pub(crate) read_only_files: Vec<String>,
    pub(crate) forbidden_actions: Vec<String>,
    pub(crate) exact_steps: Vec<String>,
    pub(crate) success_criteria: Vec<String>,
    pub(crate) tests_to_run: Vec<String>,
    #[serde(default)]
    pub(crate) stop_conditions: Vec<String>,
    #[serde(default = "default_discovery_budget")]
    pub(crate) discovery_budget: String,
    pub(crate) estimated_minutes: u32,
    pub(crate) requires_network: bool,
    /// If true, the orchestrator should prefer running this task in an
    /// isolated git worktree. None means the orchestrator decides.
    #[serde(default)]
    pub(crate) prefer_worktree: Option<bool>,
}

pub(crate) fn default_task_status() -> String {
    "approved".to_string()
}

pub(crate) fn default_task_priority() -> String {
    "normal".to_string()
}

pub(crate) fn default_discovery_budget() -> String {
    "Read only the allowed and read-only files named by this task unless a stop condition requires escalation.".to_string()
}
