//! Persistent orchestrator state.
//!
//! Owns `State` and small routing flags persisted in `.orchestrator/state.json`
//! (including Worker file leases). Large observations belong in JSONL
//! evidence/event streams, not state.
//!
//! Load/save IO and most transition effects remain in `app`.
//!
//! Read when changing persisted routing flags. Do not read for prompt text,
//! compiler diagnostics, or Git commands.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::context::lease::FileLeaseSet;
use crate::domain::{Event, TaskSpec};
use crate::repair::WorkerRetryBrief;
use crate::task_import::ImportedTaskRegistry;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct State {
    // Persistent state is deliberately small: large observations go to JSONL
    // evidence/event files, while this file stores routing flags and summaries.
    pub(crate) mission: String,
    #[serde(default)]
    pub(crate) project_complete: bool,
    #[serde(default, alias = "top_due")]
    pub(crate) master_due: bool,
    #[serde(default)]
    pub(crate) planner_due: bool,
    #[serde(default)]
    pub(crate) needs_reconciliation: bool,
    #[serde(default)]
    pub(crate) retry_worker_same_task: Option<String>,
    // A Rust-generated, narrow repair packet.  When present, Worker should
    // continue from the dirty tree and repair the fingerprinted issue only.
    #[serde(default)]
    pub(crate) retry_worker_brief: Option<WorkerRetryBrief>,
    // Set only when Rust has exhausted mechanical retry and wants Senior Dev to
    // route a bounded repair.  Senior still does not commit or mutate queues.
    #[serde(default)]
    pub(crate) senior_dev_repair_order: Option<String>,
    // Senior-approved continuation window.  PM still owns direction; this lets
    // Senior say an existing technical lane is safe for Worker to keep executing
    // while Rust enforces task IDs, scope, and verification one task at a time.
    #[serde(default)]
    pub(crate) senior_batch_approval: Option<SeniorBatchApproval>,
    // Terminal pause used when AI planning identifies a dirty/quarantined set
    // that requires a human policy choice.  This prevents PM -> CEO -> PM loops
    // when the correct next step is outside the orchestrator's authority.
    #[serde(default)]
    pub(crate) human_disposition_required: Option<HumanDispositionPause>,
    #[serde(default)]
    pub(crate) human_disposition_decision: Option<HumanDispositionDecision>,
    /// At most one approved-but-not-committed mutation may exist. Older state
    /// files deserialize with no pending intent.
    #[serde(default)]
    pub(crate) pending_commit: Option<PendingCommit>,
    #[serde(default)]
    pub(crate) completed_tasks: u64,
    #[serde(default)]
    pub(crate) failed_tasks: HashMap<String, u32>,
    // Fingerprint counts prevent infinite loops on the same mechanical failure
    // while still allowing a different failure in the same task to get a retry.
    #[serde(default)]
    pub(crate) failed_fingerprints: HashMap<String, u32>,
    #[serde(default)]
    pub(crate) queue: Vec<TaskSpec>,
    /// Immutable origin records for tasks admitted through the digest-bound
    /// external import contract. Imported tasks remain state-owned and are not
    /// reconciled against project Markdown queues.
    #[serde(default)]
    pub(crate) imported_tasks: ImportedTaskRegistry,
    // Bounded Worker file leases for context packs (CHURN-003).
    #[serde(default)]
    pub(crate) worker_file_leases: HashMap<String, FileLeaseSet>,
    #[serde(default, alias = "last_top_summary")]
    pub(crate) last_master_summary: Option<String>,
    #[serde(default)]
    pub(crate) last_planner_summary: Option<String>,
    #[serde(default, alias = "last_middle_summary")]
    pub(crate) last_foreman_summary: Option<String>,
    #[serde(default, alias = "last_bottom_summary")]
    pub(crate) last_worker_summary: Option<String>,
    #[serde(default)]
    pub(crate) last_review_summary: Option<String>,
    #[serde(default)]
    pub(crate) recent_events: Vec<Event>,
    #[serde(default, skip)]
    pub(crate) progress_action_completed_this_step: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub(crate) struct PendingPathFingerprint {
    pub(crate) path: String,
    pub(crate) exists: bool,
    pub(crate) file_type: String,
    /// Git tree mode that will be recorded for this path (for example,
    /// `100644`, `100755`, or `120000`).  Older pending intents omit this
    /// field and therefore fail closed when revalidated against a live path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) git_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) content_sha256: Option<String>,
}

/// One UTF-8 control-document update authored by Rust after role/verification
/// work completed. Manual mode keeps the pre-update bytes in the worktree and
/// applies `after_content` only after an exact pending-commit approval.
#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub(crate) struct PendingDeferredFileWrite {
    pub(crate) path: String,
    pub(crate) before_fingerprint: PendingPathFingerprint,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) before_content: Option<String>,
    pub(crate) after_content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct PendingCommit {
    pub(crate) id: String,
    pub(crate) digest: String,
    pub(crate) source_lane: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) import_digest: Option<String>,
    pub(crate) base_head: String,
    pub(crate) dirty_paths: Vec<String>,
    pub(crate) path_fingerprints: Vec<PendingPathFingerprint>,
    /// Rust-authored queue/tracking bytes that are absent from the worktree
    /// while approval is pending. They are digest-bound with the rest of the
    /// intent and applied immediately before staging.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) deferred_file_writes: Vec<PendingDeferredFileWrite>,
    pub(crate) commit_message: String,
    #[serde(default)]
    pub(crate) guard_evidence_ids: Vec<String>,
    #[serde(default)]
    pub(crate) verification_evidence_ids: Vec<String>,
    /// Opaque, Rust-authored bookkeeping transition applied only after commit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) deferred_transition: Option<Value>,
    pub(crate) created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct HumanDispositionPause {
    pub(crate) reason: String,
    pub(crate) options: Vec<String>,
    pub(crate) source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct HumanDispositionDecision {
    pub(crate) decision: String,
    pub(crate) summary: String,
    pub(crate) decided_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SeniorBatchApproval {
    // Durable continuation contract from Senior Dev.  It names existing backlog
    // task IDs only; Rust still checks every task card before queueing it.
    pub(crate) batch_id: String,
    pub(crate) task_ids: Vec<String>,
    pub(crate) max_tasks: usize,
    pub(crate) remaining_successes: usize,
    pub(crate) allowed_kinds: Vec<String>,
    pub(crate) forbidden_scope: Vec<String>,
    pub(crate) requires_stop_on: Vec<String>,
    pub(crate) verification_required: Vec<String>,
    pub(crate) auto_accept_policy: String,
    pub(crate) summary: String,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_state_defaults_to_no_pending_commit() {
        let state: State = serde_json::from_str(r#"{"mission":"legacy"}"#).unwrap();
        assert!(state.pending_commit.is_none());
        assert_eq!(state.mission, "legacy");
    }
}
