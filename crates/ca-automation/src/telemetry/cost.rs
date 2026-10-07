//! Model-call cost telemetry.
//!
//! Owns `.orchestrator/cost.jsonl` writes and reads, skipped/called model
//! records, role normalization, and future `codex-team cost` aggregation.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;

use crate::paths::Paths;

// ---------------------------------------------------------------------------
// Cost data types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ModelCallRecord {
    // Cost telemetry is intentionally local and append-only.  It lets the CLI
    // prove skipped role calls and token estimates without depending on provider
    // billing APIs.
    pub(crate) timestamp: String,
    pub(crate) role: String,
    pub(crate) task_id: Option<String>,
    pub(crate) model: String,
    /// Process engine that executed the role (for example `ca`). Older ledger
    /// rows omit this field and continue to deserialize as legacy records.
    #[serde(default, alias = "provider", skip_serializing_if = "Option::is_none")]
    pub(crate) execution_engine: Option<String>,
    /// Provider route selected inside an execution engine. This is distinct
    /// from the orchestrator's role provider and is populated for `ca` runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) route_provider: Option<String>,
    pub(crate) effort: String,
    pub(crate) prompt_token_estimate: usize,
    pub(crate) output_char_count: usize,
    pub(crate) skipped_by_rust: bool,
    pub(crate) skip_or_call_reason: String,
}

#[derive(Debug, Default)]
pub(crate) struct CostRoleStats {
    pub(crate) calls: usize,
    pub(crate) skipped: usize,
    pub(crate) prompt_tokens: usize,
    pub(crate) output_chars: usize,
    pub(crate) estimated_saved_tokens: usize,
}

// ---------------------------------------------------------------------------
// Cost JSONL append / read
// ---------------------------------------------------------------------------

pub(crate) fn append_model_call_record(paths: &Paths, record: &ModelCallRecord) -> Result<()> {
    fs::create_dir_all(&paths.orch)?;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&paths.cost)
        .with_context(|| format!("open cost ledger {}", paths.cost.display()))?;
    writeln!(file, "{}", serde_json::to_string(record)?)?;
    Ok(())
}

pub(crate) fn read_model_call_records(paths: &Paths) -> Result<Vec<ModelCallRecord>> {
    if !paths.cost.exists() {
        return Ok(Vec::new());
    }
    super::read_jsonl_file(&paths.cost)
}

// ---------------------------------------------------------------------------
// Pure helpers
// ---------------------------------------------------------------------------

pub(crate) fn normalize_cost_role(role: &str) -> &str {
    match role {
        "master" => "CEO",
        "planner" => "Project Manager",
        "foreman" | "foreman-review" => "Senior Dev",
        "worker" => "Worker",
        other => other,
    }
}

pub(crate) fn fallback_prompt_tokens_for_role(role: &str) -> usize {
    match role {
        "CEO" => 20_000,
        "Project Manager" => 18_000,
        "Senior Dev" => 20_000,
        "Worker" => 17_000,
        _ => 10_000,
    }
}

pub(crate) fn format_usize(value: usize) -> String {
    let text = value.to_string();
    let mut out = String::new();
    for (index, ch) in text.chars().rev().enumerate() {
        if index > 0 && index % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out.chars().rev().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_cost_rows_deserialize_without_execution_identity() {
        let record: ModelCallRecord = serde_json::from_str(
            r#"{"timestamp":"2026-08-20T00:00:00Z","role":"worker","task_id":null,"model":"gpt-test","effort":"high","prompt_token_estimate":1,"output_char_count":2,"skipped_by_rust":false,"skip_or_call_reason":"called"}"#,
        )
        .expect("legacy cost row");
        assert!(record.execution_engine.is_none());
        assert!(record.route_provider.is_none());

        let provider_record: ModelCallRecord = serde_json::from_str(
            r#"{"timestamp":"2026-08-20T00:00:00Z","role":"worker","task_id":null,"model":"local","provider":"ollama","effort":"high","prompt_token_estimate":1,"output_char_count":2,"skipped_by_rust":false,"skip_or_call_reason":"called"}"#,
        )
        .expect("provider-era cost row");
        assert_eq!(provider_record.execution_engine.as_deref(), Some("ollama"));
    }
}
