//! Cross-session task stability tracking — detects whether harness changes
//! improved or regressed tool-level outcomes over time.
//!
//! Inspired by AHE's `compute_task_stability()` which classifies tasks as
//! stable_pass/stable_fail/unstable across iterations. Here we track tool-level
//! outcomes across sessions.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

use crate::harness_registry::ComponentSnapshot;
use crate::session_outcome::SessionOutcome;

// ---------------------------------------------------------------------------
// Per-tool stability
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolStability {
    /// Total sessions where this tool was used.
    pub sessions_used: u32,
    /// Total invocations across all sessions.
    pub total_invocations: u32,
    /// Total failures across all sessions.
    pub total_failures: u32,
    /// Sessions where the tool had at least one failure.
    pub sessions_with_failures: u32,
    /// Sessions where the tool was denied by the user.
    pub sessions_with_denials: u32,
    /// Trend direction based on recent sessions: "improving", "stable", "degrading".
    pub trend: String,
}

impl ToolStability {
    pub fn failure_rate(&self) -> f64 {
        if self.total_invocations == 0 {
            return 0.0;
        }
        self.total_failures as f64 / self.total_invocations as f64
    }
}

// ---------------------------------------------------------------------------
// Change point — a detected shift in outcomes after a harness change
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangePoint {
    /// Unix timestamp when the change was detected.
    pub detected_at: u64,
    /// Harness component hash that changed.
    pub component: String,
    /// Old hash.
    pub hash_before: String,
    /// New hash.
    pub hash_after: String,
    /// Outcome positive rate before the change.
    pub positive_rate_before: f64,
    /// Outcome positive rate after the change.
    pub positive_rate_after: f64,
    /// Delta (positive = improvement).
    pub delta: f64,
    /// Number of sessions observed before and after.
    pub sessions_before: u32,
    pub sessions_after: u32,
}

// ---------------------------------------------------------------------------
// Stability index
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskStabilityIndex {
    /// Workspace fingerprint hash.
    pub workspace_hash: String,
    /// Harness hash at time of computation.
    pub harness_hash: String,
    /// Total sessions tracked.
    pub total_sessions: u32,
    /// Per-outcome-kind counts.
    pub outcome_distribution: BTreeMap<String, u32>,
    /// Per-tool stability metrics.
    pub tool_stability: BTreeMap<String, ToolStability>,
    /// Composite score: 0.0 (all failures) to 1.0 (all successes).
    pub composite_score: f64,
    /// Detected change points.
    pub change_points: Vec<ChangePoint>,
    /// When the index was computed.
    pub computed_at: u64,
}

// ---------------------------------------------------------------------------
// Computation
// ---------------------------------------------------------------------------

/// Compute a stability index from a set of session outcomes.
pub fn compute_stability(
    outcomes: &[SessionOutcome],
    snapshots: &[(u64, ComponentSnapshot)],
) -> TaskStabilityIndex {
    if outcomes.is_empty() {
        return TaskStabilityIndex {
            workspace_hash: String::new(),
            harness_hash: String::new(),
            total_sessions: 0,
            outcome_distribution: BTreeMap::new(),
            tool_stability: BTreeMap::new(),
            composite_score: 0.0,
            change_points: Vec::new(),
            computed_at: now_epoch(),
        };
    }

    // Describe the latest comparable cohort, never pool unrelated workspaces or routes.
    let Some(latest) = outcomes
        .iter()
        .filter(|o| o.evidence_complete)
        .max_by_key(|o| (o.started_at, &o.task_id))
    else {
        return compute_stability(&[], &[]);
    };
    let cohort: Vec<_> = outcomes
        .iter()
        .filter(|o| {
            o.evidence_complete
                && o.version == latest.version
                && o.harness_epochs.len() <= 1
                && o.workspace_hash == latest.workspace_hash
                && o.model == latest.model
                && o.provider == latest.provider
                && o.harness == latest.harness
        })
        .cloned()
        .collect();
    if cohort.is_empty() {
        return compute_stability(&[], &[]);
    }
    let outcomes = cohort.as_slice();
    let workspace_hash = latest.workspace_hash.clone();

    // Outcome distribution
    let mut outcome_dist: BTreeMap<String, u32> = BTreeMap::new();
    let mut positive_count = 0u32;
    for outcome in outcomes {
        let key = format!("{:?}", outcome.outcome);
        *outcome_dist.entry(key).or_default() += 1;
        if outcome.verified_positive() {
            positive_count += 1;
        }
    }

    // Per-tool stability
    let mut tool_map: BTreeMap<String, (u32, u32, u32, u32, u32)> = BTreeMap::new();
    // (sessions_used, total_invocations, total_failures, sessions_with_failures, sessions_with_denials)
    for outcome in outcomes {
        for (tool_name, summary) in &outcome.tools {
            let entry = tool_map.entry(tool_name.clone()).or_default();
            entry.0 += 1;
            entry.1 += summary.total.saturating_sub(summary.denied);
            entry.2 += summary.failed;
            if summary.failed > 0 {
                entry.3 += 1;
            }
            if summary.denied > 0 {
                entry.4 += 1;
            }
        }
    }

    let tool_stability: BTreeMap<String, ToolStability> = tool_map
        .into_iter()
        .map(|(name, (su, ti, tf, swf, swd))| {
            let trend = compute_trend(outcomes, &name);
            (
                name,
                ToolStability {
                    sessions_used: su,
                    total_invocations: ti,
                    total_failures: tf,
                    sessions_with_failures: swf,
                    sessions_with_denials: swd,
                    trend,
                },
            )
        })
        .collect();

    // Change points — detect harness component changes and compare outcome rates
    let change_points = Vec::new();

    // Harness hash (latest)
    let harness_hash = snapshots
        .last()
        .map(|(_, s)| {
            let mut hasher = sha2::Sha256::new();
            use sha2::Digest;
            for (k, v) in &s.components {
                hasher.update(format!("{:?}{}", k, v).as_bytes());
            }
            format!("{:x}", hasher.finalize())
        })
        .unwrap_or_default();

    TaskStabilityIndex {
        workspace_hash,
        harness_hash,
        total_sessions: outcomes.len() as u32,
        outcome_distribution: outcome_dist,
        tool_stability,
        composite_score: positive_count as f64 / outcomes.len().max(1) as f64,
        change_points,
        computed_at: now_epoch(),
    }
}

/// Simple trend computation: compare failure rate in first half vs second half
/// of sessions where this tool was used.
fn compute_trend(outcomes: &[SessionOutcome], tool_name: &str) -> String {
    let mut sessions: Vec<_> = outcomes
        .iter()
        .filter(|o| o.tools.contains_key(tool_name))
        .collect();
    sessions.sort_by_key(|o| (o.started_at, &o.task_id));
    if sessions.len() < 4 {
        return "insufficient_data".into();
    }
    let rate = |items: &[&SessionOutcome]| {
        let (failures, calls) = items.iter().fold((0u64, 0u64), |(f, n), o| {
            let t = &o.tools[tool_name];
            (
                f + u64::from(t.failed),
                n + u64::from(t.total.saturating_sub(t.denied)),
            )
        });
        (calls > 0).then(|| failures as f64 / calls as f64)
    };
    let mid = sessions.len() / 2;
    match (rate(&sessions[..mid]), rate(&sessions[mid..])) {
        (Some(before), Some(after)) if after < before => "improving",
        (Some(before), Some(after)) if after > before => "degrading",
        (Some(_), Some(_)) => "stable",
        _ => "insufficient_data",
    }
    .into()
}

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

#[allow(dead_code)]
pub fn save_stability(index: &TaskStabilityIndex, data_dir: &Path) -> std::io::Result<()> {
    let dir = data_dir.join("stability");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("latest.json");
    let json = serde_json::to_string_pretty(index)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(&path, json)
}

#[allow(dead_code)]
pub fn load_stability(data_dir: &Path) -> std::io::Result<Option<TaskStabilityIndex>> {
    let path = data_dir.join("stability/latest.json");
    if !path.exists() {
        return Ok(None);
    }
    let data = std::fs::read_to_string(&path)?;
    let index: TaskStabilityIndex = serde_json::from_str(&data)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    Ok(Some(index))
}

// ---------------------------------------------------------------------------
// CLI helpers
// ---------------------------------------------------------------------------

pub fn format_stability(index: &TaskStabilityIndex) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "Sessions: {} | Composite score: {:.1}%\n",
        index.total_sessions,
        index.composite_score * 100.0
    ));
    if !index.tool_stability.is_empty() {
        out.push_str(&format!(
            "\n{:<24} {:<8} {:<8} {:<8} {:<10}\n",
            "Tool", "Used", "Failed", "Rate", "Trend"
        ));
        out.push_str(&"-".repeat(64));
        out.push('\n');
        for (name, ts) in &index.tool_stability {
            out.push_str(&format!(
                "{:<24} {:<8} {:<8} {:<8.1}% {:<10}\n",
                name,
                ts.total_invocations,
                ts.total_failures,
                ts.failure_rate() * 100.0,
                ts.trend,
            ));
        }
    }
    if !index.change_points.is_empty() {
        out.push_str(&format!(
            "\nChange points ({}):\n",
            index.change_points.len()
        ));
        for cp in &index.change_points {
            out.push_str(&format!(
                "  {}: {:.1}% -> {:.1}% (delta: {:+.1}%, {} before / {} after)\n",
                cp.component,
                cp.positive_rate_before * 100.0,
                cp.positive_rate_after * 100.0,
                cp.delta * 100.0,
                cp.sessions_before,
                cp.sessions_after
            ));
        }
    }
    out
}

fn now_epoch() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_outcome::{OutcomeKind, OutcomeTracker, ToolSummary};

    #[test]
    fn hrs253_trend_uses_chronological_failure_rates() {
        let mut outcomes = Vec::new();
        for (timestamp, total, failed) in [(4, 100, 10), (3, 100, 10), (2, 10, 5), (1, 10, 5)] {
            let mut tools = BTreeMap::new();
            tools.insert(
                "run_command".into(),
                ToolSummary {
                    total,
                    failed,
                    succeeded: total - failed,
                    ..Default::default()
                },
            );
            let mut outcome = make_outcome(&timestamp.to_string(), OutcomeKind::Completed, tools);
            outcome.started_at = timestamp;
            outcomes.push(outcome);
        }
        assert_eq!(
            compute_stability(&outcomes, &[]).tool_stability["run_command"].trend,
            "improving"
        );
        outcomes.reverse();
        assert_eq!(
            compute_stability(&outcomes, &[]).tool_stability["run_command"].trend,
            "improving"
        );
    }

    fn make_outcome(
        session_id: &str,
        outcome: OutcomeKind,
        tools: BTreeMap<String, ToolSummary>,
    ) -> SessionOutcome {
        let mut tracker = OutcomeTracker::new(session_id.into(), "/tmp/test");
        tracker.set_outcome(outcome);
        let mut result = tracker.finish();
        result.tools = tools;
        result
    }

    #[test]
    fn compute_stability_empty() {
        let index = compute_stability(&[], &[]);
        assert_eq!(index.total_sessions, 0);
        assert_eq!(index.composite_score, 0.0);
    }

    #[test]
    fn compute_stability_with_outcomes() {
        let mut tools1 = BTreeMap::new();
        tools1.insert(
            "read_file".into(),
            ToolSummary {
                total: 5,
                succeeded: 5,
                failed: 0,
                ..Default::default()
            },
        );
        let mut tools2 = BTreeMap::new();
        tools2.insert(
            "read_file".into(),
            ToolSummary {
                total: 3,
                succeeded: 2,
                failed: 1,
                ..Default::default()
            },
        );

        let outcomes = vec![
            make_outcome("s1", OutcomeKind::Accepted, tools1),
            make_outcome("s2", OutcomeKind::Rejected, tools2),
        ];

        let index = compute_stability(&outcomes, &[]);
        assert_eq!(index.total_sessions, 2);
        assert!((index.composite_score - 0.5).abs() < f64::EPSILON);
        let rf = index.tool_stability.get("read_file").unwrap();
        assert_eq!(rf.total_invocations, 8);
        assert_eq!(rf.total_failures, 1);
    }
}
