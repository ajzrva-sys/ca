//! Event stream telemetry.
//!
//! Owns `.orchestrator/events.jsonl` writes and reads, run-record persistence,
//! and re-exports of the domain event vocabulary. Telemetry should observe
//! decisions rather than make them.

use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::io::Write;

use crate::paths::Paths;

// Re-export domain event types so callers can get them from `telemetry::events`.
pub(crate) use crate::domain::event::{OrchestratorEvent, OrchestratorEventKind};

// ---------------------------------------------------------------------------
// Event stream append
// ---------------------------------------------------------------------------

pub(crate) fn append_event(
    paths: &Paths,
    kind: OrchestratorEventKind,
    task_id: Option<&str>,
    payload: Value,
) -> Result<()> {
    // Events are the orchestrator's audit trail.  They should answer "why did
    // Rust call or skip a role?" without reconstructing a terminal transcript.
    fs::create_dir_all(&paths.orch)?;
    let event = OrchestratorEvent {
        timestamp: Utc::now().to_rfc3339(),
        kind,
        task_id: task_id.map(str::to_string),
        payload,
    };
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&paths.events)
        .with_context(|| format!("open event stream {}", paths.events.display()))?;
    writeln!(file, "{}", serde_json::to_string(&event)?)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Event stream reads
// ---------------------------------------------------------------------------

pub(crate) fn read_orchestrator_events(paths: &Paths) -> Result<Vec<OrchestratorEvent>> {
    let events_path = paths.orch.join("events.jsonl");
    if !events_path.exists() {
        return Ok(Vec::new());
    }
    super::read_jsonl_file(&events_path)
}

// ---------------------------------------------------------------------------
// Run records
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub(crate) enum RunState {
    Pending,
    Running,
    Passed,
    Failed,
    TimedOut,
    Cancelled,
    Skipped,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RunRecord {
    // Durable run record for one recipe/command execution.  This is intentionally
    // close to a local CI job record so interrupted runs can be inspected later.
    pub(crate) run_id: String,
    pub(crate) task_id: Option<String>,
    pub(crate) recipe_id: String,
    pub(crate) state: RunState,
    pub(crate) started_at: Option<String>,
    pub(crate) finished_at: Option<String>,
    pub(crate) duration_ms: Option<u128>,
    pub(crate) result_ref: Option<String>,
}

pub(crate) fn persist_run_record(paths: &Paths, record: &RunRecord) -> Result<()> {
    fs::create_dir_all(&paths.runs)?;
    let path = paths
        .runs
        .join(format!("{}.json", sanitize_filename(&record.run_id)));
    crate::fs_util::write_text_atomic(
        &path,
        &format!("{}\n", serde_json::to_string_pretty(record)?),
    )
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

pub(crate) fn sanitize_filename(input: &str) -> String {
    input
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}
