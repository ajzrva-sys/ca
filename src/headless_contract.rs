use crate::agent::client::{ToolDef, ToolFunction};
use crate::agent::stream::UsageInfo;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::Read;
use std::path::Path;
use uuid::Uuid;

pub const SCHEMA_V2: &str = "ca.headless.v2";
pub const STRUCTURED_OUTPUT_TOOL: &str = "report_structured_output";
pub const VERIFICATION_TOOL: &str = "report_verification";
const MAX_SCHEMA_BYTES: u64 = 256 * 1024;
const MAX_SCHEMA_DEPTH: usize = 32;
const MAX_SCHEMA_NODES: usize = 4_096;
const MAX_SCHEMA_PROPERTIES: usize = 512;
const MAX_STRUCTURED_OUTPUT_BYTES: usize = 1024 * 1024;
const MAX_REPORT_ITEMS: usize = 32;
const MAX_REPORT_TEXT: usize = 8 * 1024;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CheckMode {
    #[default]
    Disabled,
    VerifyOnly,
    Repair,
}

impl CheckMode {
    pub fn enabled(self) -> bool {
        !matches!(self, Self::Disabled)
    }

    pub fn repair_requested(self) -> bool {
        matches!(self, Self::Repair)
    }
}

#[derive(Debug, Clone)]
pub struct StructuredOutputContract {
    schema: Value,
}

impl StructuredOutputContract {
    pub fn load(path: &Path) -> Result<Self, String> {
        let link_metadata = std::fs::symlink_metadata(path)
            .map_err(|error| format!("could not read output schema metadata: {error}"))?;
        if link_metadata.file_type().is_symlink() || !link_metadata.is_file() {
            return Err("output schema must be a regular file, not a symlink".into());
        }
        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW);
        }
        let file = options
            .open(path)
            .map_err(|error| format!("could not open output schema: {error}"))?;
        let metadata = file
            .metadata()
            .map_err(|error| format!("could not read output schema metadata: {error}"))?;
        if !metadata.is_file() {
            return Err("output schema must be a regular file, not a symlink".into());
        }
        if metadata.len() > MAX_SCHEMA_BYTES {
            return Err(format!(
                "output schema exceeds the {MAX_SCHEMA_BYTES}-byte limit"
            ));
        }
        let mut bytes = Vec::new();
        file.take(MAX_SCHEMA_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| format!("could not read output schema: {error}"))?;
        if bytes.len() as u64 > MAX_SCHEMA_BYTES {
            return Err(format!(
                "output schema exceeds the {MAX_SCHEMA_BYTES}-byte limit"
            ));
        }
        Self::from_bytes(&bytes)
    }

    /// Build a structured-output contract from an already-bound schema
    /// snapshot. Role-run uses this entry point after opening the schema
    /// relative to its workspace directory descriptor, so validation cannot
    /// be redirected by reopening a model-controlled path.
    pub(crate) fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() as u64 > MAX_SCHEMA_BYTES {
            return Err(format!(
                "output schema exceeds the {MAX_SCHEMA_BYTES}-byte limit"
            ));
        }
        let schema: Value = serde_json::from_slice(bytes)
            .map_err(|error| format!("output schema is not valid UTF-8 JSON: {error}"))?;
        if schema.get("type").and_then(Value::as_str) != Some("object") {
            return Err("output schema root must declare type object".into());
        }
        validate_json_bounds(&schema, MAX_SCHEMA_DEPTH, MAX_SCHEMA_NODES, true)?;
        jsonschema::options()
            .with_draft(jsonschema::Draft::Draft202012)
            .build(&schema)
            .map_err(|error| format!("output schema is invalid: {error}"))?;
        Ok(Self { schema })
    }

    pub(crate) fn from_value(schema: Value) -> Result<Self, String> {
        let bytes = serde_json::to_vec(&schema)
            .map_err(|error| format!("output schema could not be encoded: {error}"))?;
        Self::from_bytes(&bytes)
    }

    pub fn tool_definition(&self) -> ToolDef {
        let mut nested_schema = self.schema.clone();
        rewrite_local_references(&mut nested_schema, "#/properties/value");
        ToolDef {
            def_type: "function".into(),
            function: ToolFunction {
                name: STRUCTURED_OUTPUT_TOOL.into(),
                description:
                    "Return the final result exactly once as locally validated structured JSON."
                        .into(),
                parameters: json!({
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["value"],
                    "properties": { "value": nested_schema }
                }),
            },
        }
    }

    pub fn validate_arguments(&self, arguments: &str) -> Result<Value, String> {
        if arguments.len() > MAX_STRUCTURED_OUTPUT_BYTES {
            return Err(format!(
                "structured output exceeds the {MAX_STRUCTURED_OUTPUT_BYTES}-byte limit"
            ));
        }
        let arguments: Value = serde_json::from_str(arguments)
            .map_err(|error| format!("structured output arguments are invalid JSON: {error}"))?;
        let object = arguments
            .as_object()
            .ok_or_else(|| "structured output arguments must be an object".to_string())?;
        if object.len() != 1 || !object.contains_key("value") {
            return Err("structured output arguments must contain only value".into());
        }
        let value = object.get("value").cloned().unwrap_or(Value::Null);
        validate_json_bounds(&value, MAX_SCHEMA_DEPTH, MAX_SCHEMA_NODES, false)?;
        let validator = jsonschema::options()
            .with_draft(jsonschema::Draft::Draft202012)
            .build(&self.schema)
            .map_err(|error| format!("output schema is invalid: {error}"))?;
        validator
            .validate(&value)
            .map_err(|error| format!("structured output failed local validation: {error}"))?;
        let encoded = serde_json::to_vec(&value)
            .map_err(|error| format!("structured output could not be encoded: {error}"))?;
        if encoded.len() > MAX_STRUCTURED_OUTPUT_BYTES {
            return Err(format!(
                "structured output exceeds the {MAX_STRUCTURED_OUTPUT_BYTES}-byte limit"
            ));
        }
        Ok(value)
    }

    pub(crate) fn validate_value(&self, value: &Value) -> Result<(), String> {
        let arguments = serde_json::to_string(&json!({ "value": value }))
            .map_err(|error| format!("structured output could not be encoded: {error}"))?;
        self.validate_arguments(&arguments).map(|_| ())
    }
}

fn rewrite_local_references(value: &mut Value, prefix: &str) {
    match value {
        Value::Object(object) => {
            for key in ["$ref", "$dynamicRef", "$recursiveRef"] {
                if let Some(reference) = object
                    .get_mut(key)
                    .and_then(|value| value.as_str())
                    .map(str::to_string)
                {
                    if let Some(fragment) = reference.strip_prefix('#') {
                        object.insert(key.into(), Value::String(format!("{prefix}{fragment}")));
                    }
                }
            }
            for child in object.values_mut() {
                rewrite_local_references(child, prefix);
            }
        }
        Value::Array(array) => {
            for child in array {
                rewrite_local_references(child, prefix);
            }
        }
        _ => {}
    }
}

fn validate_json_bounds(
    root: &Value,
    max_depth: usize,
    max_nodes: usize,
    schema_mode: bool,
) -> Result<(), String> {
    let mut stack = vec![(root, 1usize)];
    let mut nodes = 0usize;
    let mut properties = 0usize;
    while let Some((value, depth)) = stack.pop() {
        nodes = nodes.saturating_add(1);
        if nodes > max_nodes {
            return Err(format!("JSON exceeds the {max_nodes}-node limit"));
        }
        if depth > max_depth {
            return Err(format!("JSON exceeds the {max_depth}-level depth limit"));
        }
        match value {
            Value::Object(object) => {
                if schema_mode {
                    if let Some(reference) = object.get("$ref").and_then(Value::as_str) {
                        if !reference.starts_with('#') {
                            return Err("output schema may not contain remote references".into());
                        }
                    }
                    for key in ["$dynamicRef", "$recursiveRef"] {
                        if let Some(reference) = object.get(key).and_then(Value::as_str) {
                            if !reference.starts_with('#') {
                                return Err(
                                    "output schema may not contain remote references".into()
                                );
                            }
                        }
                    }
                    if let Some(count) = object
                        .get("properties")
                        .and_then(Value::as_object)
                        .map(|values| values.len())
                    {
                        properties = properties.saturating_add(count);
                        if properties > MAX_SCHEMA_PROPERTIES {
                            return Err(format!(
                                "output schema exceeds the {MAX_SCHEMA_PROPERTIES}-property limit"
                            ));
                        }
                    }
                }
                stack.extend(object.values().map(|value| (value, depth + 1)));
            }
            Value::Array(array) => {
                stack.extend(array.iter().map(|value| (value, depth + 1)));
            }
            Value::String(text) if text.len() > MAX_REPORT_TEXT * 8 => {
                return Err("JSON contains an oversized string".into());
            }
            _ => {}
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct VerificationReport {
    pub passed: bool,
    pub summary: String,
    #[serde(default)]
    pub issues: Vec<String>,
    #[serde(default)]
    pub evidence: Vec<String>,
}

impl VerificationReport {
    pub fn parse(arguments: &str) -> Result<Self, String> {
        if arguments.len() > MAX_STRUCTURED_OUTPUT_BYTES {
            return Err("verification report is too large".into());
        }
        let mut report: Self = serde_json::from_str(arguments)
            .map_err(|error| format!("invalid verification report: {error}"))?;
        if report.summary.trim().is_empty() || report.summary.len() > MAX_REPORT_TEXT {
            return Err("verification summary must be 1 to 8192 bytes".into());
        }
        if report.issues.len() > MAX_REPORT_ITEMS || report.evidence.len() > MAX_REPORT_ITEMS {
            return Err("verification report has too many items".into());
        }
        for item in report.issues.iter().chain(&report.evidence) {
            if item.trim().is_empty() || item.len() > MAX_REPORT_TEXT {
                return Err("verification report item must be 1 to 8192 bytes".into());
            }
        }
        report.summary = crate::terminal_text::sanitize_terminal_text(&report.summary);
        for item in report.issues.iter_mut().chain(&mut report.evidence) {
            *item = crate::terminal_text::sanitize_terminal_text(item);
        }
        Ok(report)
    }
}

pub fn verification_tool_definition() -> ToolDef {
    ToolDef {
        def_type: "function".into(),
        function: ToolFunction {
            name: VERIFICATION_TOOL.into(),
            description:
                "Report whether the completed work satisfies the original request exactly once."
                    .into(),
            parameters: json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["passed", "summary"],
                "properties": {
                    "passed": { "type": "boolean" },
                    "summary": { "type": "string", "minLength": 1, "maxLength": MAX_REPORT_TEXT },
                    "issues": {
                        "type": "array", "maxItems": MAX_REPORT_ITEMS,
                        "items": { "type": "string", "minLength": 1, "maxLength": MAX_REPORT_TEXT }
                    },
                    "evidence": {
                        "type": "array", "maxItems": MAX_REPORT_ITEMS,
                        "items": { "type": "string", "minLength": 1, "maxLength": MAX_REPORT_TEXT }
                    }
                }
            }),
        },
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct VerificationOutcome {
    pub report: VerificationReport,
    pub repair_requested: bool,
    pub tools_executed: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct HeadlessEnvelopeV2 {
    pub schema: &'static str,
    pub sequence: u64,
    pub run_id: Uuid,
    pub event: HeadlessEventV2,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct HeadlessOutputV2 {
    pub schema: &'static str,
    pub run_id: Uuid,
    pub success: bool,
    pub final_message: Option<String>,
    pub structured_output: Option<Value>,
    pub verification: Option<VerificationOutcome>,
    pub events: Vec<HeadlessEnvelopeV2>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HeadlessEventV2 {
    RunStart {
        model: String,
    },
    Warning {
        message: String,
    },
    TurnStart {
        turn: usize,
    },
    AssistantOutput {
        delta: String,
    },
    ReasoningOutput {
        delta: String,
    },
    ToolStart {
        call_id: String,
        name: String,
        arguments: Value,
    },
    ToolFinish {
        call_id: String,
        name: String,
        status: crate::headless::ToolResultStatus,
        output: String,
    },
    EnvironmentSnapshot {
        snapshot: crate::run_environment::EnvironmentSnapshotV1,
    },
    Usage {
        usage: UsageInfo,
    },
    ApprovalRequired {
        call_id: String,
        name: String,
        reason: String,
    },
    MainCompletion {
        message: String,
    },
    CheckStart {
        repair_requested: bool,
    },
    CheckReport {
        outcome: VerificationOutcome,
    },
    ConvergenceStart {
        round: u8,
        routes: [String; 2],
        max_cost_usd: String,
        unpriced_acknowledged: bool,
    },
    ConvergenceReport {
        outcome: crate::review_convergence::ReviewConvergenceRound,
    },
    AcceptancePacket {
        packet_sha256: String,
        stale: bool,
    },
    VerificationFailed {
        summary: String,
        exit_code: i32,
    },
    StructuredOutput {
        value: Value,
    },
    Completion {
        message: String,
    },
    Failure {
        kind: crate::headless::HeadlessFailureKind,
        message: String,
        exit_code: i32,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema_file(value: &Value) -> (tempfile::TempDir, std::path::PathBuf) {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("schema.json");
        std::fs::write(&path, serde_json::to_vec(value).unwrap()).unwrap();
        (temp, path)
    }

    #[test]
    fn schema_is_local_bounded_object_and_output_is_revalidated() {
        let (_temp, path) = schema_file(&json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["answer"],
            "properties": { "answer": { "type": "integer" } }
        }));
        let contract = StructuredOutputContract::load(&path).unwrap();
        assert_eq!(
            contract
                .validate_arguments(r#"{"value":{"answer":7}}"#)
                .unwrap(),
            json!({"answer": 7})
        );
        assert!(contract
            .validate_arguments(r#"{"value":{"answer":"no"}}"#)
            .unwrap_err()
            .contains("local validation"));
    }

    #[test]
    fn schema_rejects_non_object_remote_reference_and_symlink() {
        let (_temp, path) = schema_file(&json!({"type": "string"}));
        assert!(StructuredOutputContract::load(&path).is_err());

        let (_temp, path) = schema_file(&json!({
            "type": "object",
            "properties": { "x": { "$ref": "https://example.invalid/schema" } }
        }));
        assert!(StructuredOutputContract::load(&path)
            .unwrap_err()
            .contains("remote references"));

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let temp = tempfile::tempdir().unwrap();
            let target = temp.path().join("target.json");
            let link = temp.path().join("link.json");
            std::fs::write(&target, r#"{"type":"object"}"#).unwrap();
            symlink(&target, &link).unwrap();
            assert!(StructuredOutputContract::load(&link)
                .unwrap_err()
                .contains("regular file"));
        }
    }

    #[test]
    fn local_references_remain_valid_inside_the_report_wrapper() {
        let (_temp, path) = schema_file(&json!({
            "type": "object",
            "$defs": { "answer": { "type": "integer" } },
            "required": ["answer"],
            "properties": { "answer": { "$ref": "#/$defs/answer" } }
        }));
        let contract = StructuredOutputContract::load(&path).unwrap();
        let parameters = contract.tool_definition().function.parameters;
        let validator = jsonschema::options()
            .with_draft(jsonschema::Draft::Draft202012)
            .build(&parameters)
            .unwrap();
        assert!(validator.validate(&json!({"value":{"answer":7}})).is_ok());
        assert!(validator
            .validate(&json!({"value":{"answer":"bad"}}))
            .is_err());
    }

    #[test]
    fn schema_and_result_bounds_fail_before_validation_work() {
        let temp = tempfile::tempdir().unwrap();
        let oversized = temp.path().join("oversized.json");
        std::fs::write(&oversized, vec![b' '; MAX_SCHEMA_BYTES as usize + 1]).unwrap();
        assert!(StructuredOutputContract::load(&oversized)
            .unwrap_err()
            .contains("byte limit"));

        let mut nested = json!({"type":"integer"});
        for _ in 0..MAX_SCHEMA_DEPTH + 1 {
            nested = json!({"type":"object", "properties":{"next":nested}});
        }
        let (_temp, path) = schema_file(&nested);
        assert!(StructuredOutputContract::load(&path)
            .unwrap_err()
            .contains("depth limit"));

        let (_temp, path) = schema_file(&json!({"type":"object"}));
        let contract = StructuredOutputContract::load(&path).unwrap();
        let oversized_arguments = format!(
            "{{\"value\":{{\"text\":\"{}\"}}}}",
            "x".repeat(MAX_STRUCTURED_OUTPUT_BYTES)
        );
        assert!(contract
            .validate_arguments(&oversized_arguments)
            .unwrap_err()
            .contains("byte limit"));
    }

    #[test]
    fn verification_report_is_strict_bounded_and_sanitized() {
        let report = VerificationReport::parse(
            r#"{"passed":false,"summary":"needs work\u001b[31m","issues":["missing test"],"evidence":[]}"#,
        )
        .unwrap();
        assert!(!report.passed);
        assert!(!report.summary.contains('\u{1b}'));
        assert!(
            VerificationReport::parse(r#"{"passed":true,"summary":"ok","unexpected":true}"#)
                .is_err()
        );
    }
}
