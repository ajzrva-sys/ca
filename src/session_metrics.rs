use crate::activity::{TurnCompletion, TurnLifecycle};
use crate::app::ToolStatus;
use crate::usage::UsageCursor;
use std::collections::{BTreeMap, HashSet};
use std::time::Duration;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ToolStatusTotals {
    pub completed: u64,
    pub failed: u64,
}

impl ToolStatusTotals {
    pub fn total(self) -> u64 {
        self.completed.saturating_add(self.failed)
    }
}

#[derive(Debug, Default)]
pub struct SessionSittingMetrics {
    usage_cursor: UsageCursor,
    completed_work: Duration,
    seen_tools: HashSet<(u64, String)>,
    tools: BTreeMap<String, ToolStatusTotals>,
}

impl SessionSittingMetrics {
    pub fn reset(&mut self, usage_cursor: UsageCursor) {
        self.usage_cursor = usage_cursor;
        self.completed_work = Duration::ZERO;
        self.seen_tools.clear();
        self.tools.clear();
    }

    pub fn usage_cursor(&self) -> UsageCursor {
        self.usage_cursor
    }

    pub fn record_completion(&mut self, completion: TurnCompletion) {
        self.completed_work = self.completed_work.saturating_add(completion.duration);
    }

    pub fn worked_time(&self, lifecycle: &TurnLifecycle) -> Duration {
        let active = lifecycle
            .turn_started_at
            .filter(|_| lifecycle.is_active())
            .map_or(Duration::ZERO, |started| started.elapsed());
        self.completed_work.saturating_add(active)
    }

    pub fn record_tool(
        &mut self,
        turn_generation: u64,
        id: &str,
        name: &str,
        status: ToolStatus,
    ) -> bool {
        if !matches!(status, ToolStatus::Done | ToolStatus::Error)
            || !self.seen_tools.insert((turn_generation, id.into()))
        {
            return false;
        }
        let totals = self.tools.entry(name.into()).or_default();
        match status {
            ToolStatus::Done => totals.completed = totals.completed.saturating_add(1),
            ToolStatus::Error => totals.failed = totals.failed.saturating_add(1),
            ToolStatus::Pending | ToolStatus::Running => unreachable!(),
        }
        true
    }

    pub fn tool_total(&self) -> u64 {
        self.tools.values().map(|totals| totals.total()).sum()
    }

    pub fn tools(&self) -> &BTreeMap<String, ToolStatusTotals> {
        &self.tools
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::activity::{TurnCompletion, TurnOutcome};

    #[test]
    fn session_metrics_deduplicate_tools_and_reset_the_sitting() {
        let mut metrics = SessionSittingMetrics::default();
        assert!(metrics.record_tool(1, "call-1", "read_file", ToolStatus::Done));
        assert!(!metrics.record_tool(1, "call-1", "read_file", ToolStatus::Error));
        assert!(metrics.record_tool(2, "call-1", "read_file", ToolStatus::Error));
        assert_eq!(metrics.tool_total(), 2);
        assert_eq!(metrics.tools()["read_file"].completed, 1);
        assert_eq!(metrics.tools()["read_file"].failed, 1);

        metrics.record_completion(TurnCompletion {
            outcome: TurnOutcome::Completed,
            duration: Duration::from_secs(3),
            was_paused: false,
        });
        assert_eq!(
            metrics.worked_time(&TurnLifecycle::default()),
            Duration::from_secs(3)
        );
        metrics.reset(UsageCursor::default());
        assert_eq!(metrics.tool_total(), 0);
        assert_eq!(
            metrics.worked_time(&TurnLifecycle::default()),
            Duration::ZERO
        );
    }
}
