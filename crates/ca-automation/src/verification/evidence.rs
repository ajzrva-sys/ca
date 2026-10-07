//! Evidence report DTOs.
//!
//! Owns `EvidenceReport` and `CommandEvidence` shapes. JSONL writes, persistence
//! helpers, and compact AI-facing receipts remain in `app` until extraction can
//! preserve behavior exactly.
//!
//! Read when changing evidence JSON fields. Do not read for process execution
//! (`runner`) or recipe allowlists (`recipe` / `validate`).

use serde::{Deserialize, Serialize};

use crate::verification::recipe::VerificationCommand;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct EvidenceReport {
    // EvidenceReport is the durable proof Senior/Rust should reason from.  It is
    // persisted append-only and summarized into prompts instead of streaming raw
    // command output back into expensive model context.
    pub(crate) report_id: String,
    pub(crate) plan_id: String,
    pub(crate) task_id: String,
    pub(crate) status: String,
    pub(crate) started_at: String,
    pub(crate) completed_at: String,
    pub(crate) summary: String,
    pub(crate) commands: Vec<CommandEvidence>,
    pub(crate) rejected_commands: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CommandEvidence {
    // One command's receipt.  stdout/stderr are capped tails; diagnostics and
    // fingerprints should be derived from these compact fields.
    pub(crate) id: String,
    pub(crate) command: VerificationCommand,
    pub(crate) command_display: String,
    pub(crate) exit_code: i32,
    pub(crate) success: bool,
    pub(crate) category: String,
    pub(crate) duration_ms: u128,
    pub(crate) stdout_tail: String,
    pub(crate) stderr_tail: String,
    pub(crate) evidence_summary: String,
}
