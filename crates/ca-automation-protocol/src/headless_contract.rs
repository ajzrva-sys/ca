//! Shared headless output wire contract types.
//!
//! These types codify the `ca.headless.v2` JSONL protocol that ca emits
//! and codex-team parses. Defining them in the protocol crate makes the
//! contract explicit and testable across both consumers.

use serde::{Deserialize, Serialize};

/// Schema identifier for the v2 headless output format.
pub const CA_HEADLESS_SCHEMA: &str = "ca.headless.v2";

/// Event kinds codex-team cares about when parsing ca's headless JSONL output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HeadlessEventKind {
    RunStart,
    TurnStart,
    AssistantOutput,
    ReasoningOutput,
    ToolStart,
    ToolFinish,
    StructuredOutput,
    MainCompletion,
    CheckStart,
    CheckReport,
    Usage,
    ApprovalRequired,
    Warning,
    Failure,
}

/// The final output envelope codex-team expects from a completed ca headless run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeadlessOutputContract {
    pub schema: String,
    pub run_id: String,
    pub success: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structured_output: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification: Option<serde_json::Value>,
}

/// A single JSONL envelope line emitted during a headless run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeadlessEnvelopeContract {
    pub schema: String,
    pub sequence: u64,
    pub run_id: String,
    pub event: serde_json::Value,
}

/// Tool result status as reported in ToolFinish events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolResultStatus {
    Success,
    Error,
    Denied,
    Timeout,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_constant_matches_ca() {
        assert_eq!(CA_HEADLESS_SCHEMA, "ca.headless.v2");
    }

    #[test]
    fn event_kind_round_trips() {
        let kinds = [
            HeadlessEventKind::RunStart,
            HeadlessEventKind::ToolFinish,
            HeadlessEventKind::Failure,
        ];
        for kind in &kinds {
            let json = serde_json::to_string(kind).unwrap();
            let parsed: HeadlessEventKind = serde_json::from_str(&json).unwrap();
            assert_eq!(*kind, parsed);
        }
    }

    #[test]
    fn output_contract_deserializes() {
        let json = r#"{
            "schema": "ca.headless.v2",
            "run_id": "test-run",
            "success": true,
            "final_message": "done"
        }"#;
        let contract: HeadlessOutputContract = serde_json::from_str(json).unwrap();
        assert_eq!(contract.schema, CA_HEADLESS_SCHEMA);
        assert!(contract.success);
    }
}
