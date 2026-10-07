//! Telemetry subsystem.
//!
//! Owns append-only cost/event JSONL records, model-call accounting, Rust skip
//! records, and later report commands. Telemetry should observe decisions rather
//! than make them.
//!
//! Read when changing `.orchestrator/cost.jsonl`, events, or cost summaries. Do
//! not read for role prompts, queue parsing, or command allowlists.

pub mod cost;
pub mod events;

// Convenience re-exports so callers can write `crate::telemetry::Foo`.
pub(crate) use cost::{
    append_model_call_record, fallback_prompt_tokens_for_role, format_usize, normalize_cost_role,
    read_model_call_records, CostRoleStats, ModelCallRecord,
};
pub(crate) use events::{
    append_event, persist_run_record, read_orchestrator_events, sanitize_filename,
    OrchestratorEventKind, RunRecord, RunState,
};

// ---------------------------------------------------------------------------
// Shared JSONL reader
// ---------------------------------------------------------------------------

use anyhow::{Context, Result};
use serde::Deserialize;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

/// Generic JSONL reader.  Silently skips lines that fail to deserialize so a
/// single corrupt entry does not block the rest of the pipeline.
pub(crate) fn read_jsonl_file<T>(path: &Path) -> Result<Vec<T>>
where
    T: for<'de> Deserialize<'de>,
{
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let reader = BufReader::new(file);
    let mut values = Vec::new();
    for (index, line) in reader.lines().enumerate() {
        let line = line.with_context(|| format!("read {} line {}", path.display(), index + 1))?;
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<T>(&line) {
            Ok(value) => values.push(value),
            Err(err) => eprintln!(
                "[cost] ignoring malformed JSONL line {} in {}: {err}",
                index + 1,
                path.display()
            ),
        }
    }
    Ok(values)
}
