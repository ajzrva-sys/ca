//! Derived status vocabulary.
//!
//! Owns operator-facing derived status kinds and pure derivation from persisted
//! routing flags plus recent observation evidence. Persisted state flags remain
//! authoritative for routing; derived status is triage UI.
//!
//! Read when changing operator status labels or priority. Do not read for
//! state-machine transitions in `app`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DerivedStatusKind {
    Ready,
    Working,
    WaitingForHuman,
    NeedsPm,
    NeedsSenior,
    NeedsWorker,
    BlockedByToolHealth,
    NoSignal,
    Complete,
}

impl DerivedStatusKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Working => "working",
            Self::WaitingForHuman => "waiting_for_human",
            Self::NeedsPm => "needs_pm",
            Self::NeedsSenior => "needs_senior",
            Self::NeedsWorker => "needs_worker",
            Self::BlockedByToolHealth => "blocked_by_tool_health",
            Self::NoSignal => "no_signal",
            Self::Complete => "complete",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq, Default)]
pub(crate) struct DerivedStatusInput {
    pub(crate) project_complete: bool,
    pub(crate) queue_len: usize,
    pub(crate) approved_docs_work: bool,
    pub(crate) master_due: bool,
    pub(crate) planner_due: bool,
    pub(crate) retry_worker: bool,
    pub(crate) senior_repair: bool,
    pub(crate) senior_batch: bool,
    pub(crate) human_pause: bool,
    pub(crate) no_signal: bool,
    pub(crate) tool_cooldown_blocking: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub(crate) struct DerivedStatus {
    pub(crate) status: DerivedStatusKind,
    pub(crate) summary: String,
    pub(crate) evidence: Vec<String>,
}

pub(crate) fn derive_status(input: &DerivedStatusInput) -> DerivedStatus {
    let (status, summary) = if input.project_complete
        && input.queue_len == 0
        && !input.planner_due
        && !input.master_due
    {
        (
            DerivedStatusKind::Complete,
            "project complete and no active work",
        )
    } else if input.human_pause {
        (
            DerivedStatusKind::WaitingForHuman,
            "human disposition pause is active",
        )
    } else if input.no_signal {
        (
            DerivedStatusKind::NoSignal,
            "recent role run has no terminal output signal",
        )
    } else if input.tool_cooldown_blocking {
        (
            DerivedStatusKind::BlockedByToolHealth,
            "recent tool health suggests an advisory cooldown",
        )
    } else if input.senior_repair || input.senior_batch {
        (
            DerivedStatusKind::NeedsSenior,
            "Senior Dev routing or continuation is active",
        )
    } else if input.retry_worker || input.queue_len > 0 || input.approved_docs_work {
        (
            DerivedStatusKind::NeedsWorker,
            "approved or retryable work is ready for Worker/Rust lanes",
        )
    } else if input.planner_due || input.master_due {
        (
            DerivedStatusKind::NeedsPm,
            "planning direction is due before more Worker work",
        )
    } else {
        (DerivedStatusKind::Ready, "no immediate route is active")
    };
    let evidence = vec![
        format!("queue_len={}", input.queue_len),
        format!("planner_due={}", input.planner_due),
        format!("master_due={}", input.master_due),
        format!("retry_worker={}", input.retry_worker),
        format!("senior_repair={}", input.senior_repair),
        format!("human_pause={}", input.human_pause),
    ];
    DerivedStatus {
        status,
        summary: summary.to_string(),
        evidence,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derived_status_prioritizes_no_signal_before_route_needs() {
        let status = derive_status(&DerivedStatusInput {
            queue_len: 1,
            no_signal: true,
            ..DerivedStatusInput::default()
        });

        assert_eq!(status.status, DerivedStatusKind::NoSignal);
    }

    #[test]
    fn derived_status_reports_worker_when_queue_has_work() {
        let status = derive_status(&DerivedStatusInput {
            queue_len: 1,
            ..DerivedStatusInput::default()
        });

        assert_eq!(status.status, DerivedStatusKind::NeedsWorker);
    }
}
