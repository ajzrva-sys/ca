//! Verification plan DTOs.
//!
//! Owns the `VerificationPlan` shape (merged validated commands for a task).
//! Plan construction, event writes, and state mutation remain in `app`; process
//! execution stays in `runner`.
//!
//! Read when changing the plan JSON shape. Do not read for allowlist validation
//! or command spawn.

use serde::{Deserialize, Serialize};

use crate::verification::recipe::VerificationCommand;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct VerificationPlan {
    // A plan is the merged, validated set of commands Rust intends to run for a
    // task.  Task card commands are ordered before Worker handoff commands.
    pub(crate) plan_id: String,
    pub(crate) task_id: String,
    #[serde(default)]
    pub(crate) parent_task: Option<String>,
    pub(crate) source: String,
    pub(crate) commands: Vec<VerificationCommand>,
    pub(crate) rejected_commands: Vec<String>,
}
