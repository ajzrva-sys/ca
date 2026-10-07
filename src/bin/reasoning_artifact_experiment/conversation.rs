//! In-memory RND-097 Responses conversation state.
//!
//! A conversation belongs to exactly one provider route and one experiment
//! phase. Completed Responses output items may be replayed inside that phase,
//! but this type has no serialization contract and must never enter a receipt,
//! checkpoint, artifact, or cross-model handoff.

use crate::reasoning_experiment_protocol::{
    ExperimentModelRouteV1, MAX_INPUT_TOKENS_PER_REQUEST, MAX_OUTPUT_TOKENS_PER_REQUEST,
};
use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;

const MAX_CONTROLLER_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
const MAX_PROMPT_BYTES: usize = 256 * 1024;
const MAX_TOOL_ARGUMENT_BYTES: usize = 64 * 1024;
const MAX_TOOL_CALLS_PER_TURN: usize = 40;
const MAX_RESPONSE_ITEMS_PER_TURN: usize = 128;

#[derive(Clone, PartialEq, Eq)]
pub struct PendingToolCall {
    pub id: String,
    pub name: String,
    pub arguments_json: String,
}

impl std::fmt::Debug for PendingToolCall {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PendingToolCall")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("arguments_json", &"[REDACTED]")
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnDisposition {
    Stop { content: String },
    ToolCalls(Vec<PendingToolCall>),
}

pub struct PhaseConversation {
    route: ExperimentModelRouteV1,
    input: Vec<Value>,
    tools: Vec<Value>,
    pending_calls: Vec<PendingToolCall>,
    output_schema: Option<(String, Value)>,
}

impl std::fmt::Debug for PhaseConversation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PhaseConversation")
            .field("route", &self.route)
            .field("input_items", &self.input.len())
            .field("tools", &self.tools.len())
            .field("pending_calls", &self.pending_calls.len())
            .field(
                "output_schema",
                &self.output_schema.as_ref().map(|_| "[DECLARED]"),
            )
            .finish()
    }
}

impl PhaseConversation {
    pub fn new(
        route: ExperimentModelRouteV1,
        system_prompt: &str,
        task_prompt: &str,
        tools_enabled: bool,
    ) -> Result<Self> {
        Self::new_with_output_schema(route, system_prompt, task_prompt, tools_enabled, None)
    }

    pub fn new_with_output_schema(
        route: ExperimentModelRouteV1,
        system_prompt: &str,
        task_prompt: &str,
        tools_enabled: bool,
        output_schema: Option<(&str, Value)>,
    ) -> Result<Self> {
        route
            .validate()
            .map_err(|error| anyhow!(error.to_string()))?;
        validate_nonempty_text("system prompt", system_prompt, MAX_PROMPT_BYTES)?;
        validate_nonempty_text("task prompt", task_prompt, MAX_PROMPT_BYTES)?;
        let input = vec![
            json!({"role": "system", "content": system_prompt}),
            json!({"role": "user", "content": task_prompt}),
        ];
        let tools = if tools_enabled {
            experiment_tools()
        } else {
            Vec::new()
        };
        let conversation = Self {
            route,
            input,
            tools,
            pending_calls: Vec::new(),
            output_schema: output_schema
                .map(|(name, schema)| validate_output_schema(name, schema))
                .transpose()?,
        };
        conversation.validate_context_bound()?;
        Ok(conversation)
    }

    pub fn route(&self) -> &ExperimentModelRouteV1 {
        &self.route
    }

    pub fn request_json(&self) -> Result<String> {
        if !self.pending_calls.is_empty() {
            bail!("all pending tool calls must be completed before the next provider request");
        }
        self.validate_context_bound()?;
        let mut request = Map::new();
        request.insert("model".into(), Value::String(self.route.model.clone()));
        request.insert("input".into(), Value::Array(self.input.clone()));
        request.insert("stream".into(), Value::Bool(true));
        request.insert(
            "reasoning".into(),
            json!({"effort": self.route.reasoning_effort}),
        );
        request.insert("include".into(), json!(["reasoning.encrypted_content"]));
        request.insert(
            "max_output_tokens".into(),
            Value::from(MAX_OUTPUT_TOKENS_PER_REQUEST),
        );
        request.insert("store".into(), Value::Bool(false));
        request.insert(
            "service_tier".into(),
            Value::String(self.route.service_tier.clone()),
        );
        if !self.tools.is_empty() {
            request.insert("tools".into(), Value::Array(self.tools.clone()));
        }
        if let Some((name, schema)) = &self.output_schema {
            request.insert(
                "text".into(),
                json!({
                    "format": {
                        "type": "json_schema",
                        "name": name,
                        "strict": true,
                        "schema": schema
                    }
                }),
            );
        }
        serde_json::to_string(&Value::Object(request)).context("serialize Responses request")
    }

    pub fn accept_broker_turn(&mut self, raw: &str) -> Result<TurnDisposition> {
        validate_nonempty_text("controller response", raw, MAX_CONTROLLER_RESPONSE_BYTES)?;
        if !self.pending_calls.is_empty() {
            bail!("a broker turn cannot be accepted while tool calls are pending");
        }
        let turn: ControllerTurnV1 =
            serde_json::from_str(raw).context("decode controller response")?;
        validate_text("assistant content", &turn.content, MAX_PROMPT_BYTES)?;
        if let Some(reasoning) = &turn.reasoning {
            validate_text("assistant reasoning", reasoning, MAX_PROMPT_BYTES)?;
        }
        if turn.response_items.len() > MAX_RESPONSE_ITEMS_PER_TURN
            || turn.tool_calls.len() > MAX_TOOL_CALLS_PER_TURN
        {
            bail!("controller response exceeds the per-turn item bound");
        }

        let mut seen_output_indices = BTreeSet::new();
        let mut replay_items = Vec::with_capacity(turn.response_items.len());
        for response_item in turn.response_items {
            if !seen_output_indices.insert(response_item.output_index) {
                bail!("controller response contains a duplicate output index");
            }
            validate_response_item(&response_item.item)?;
            replay_items.push((response_item.output_index, response_item.item));
        }
        replay_items.sort_by_key(|(index, _)| *index);

        let mut seen_tool_indices = BTreeSet::new();
        let mut seen_tool_ids = BTreeSet::new();
        let mut calls = Vec::with_capacity(turn.tool_calls.len());
        for call in turn.tool_calls {
            if !seen_tool_indices.insert(call.index) || !seen_tool_ids.insert(call.id.clone()) {
                bail!("controller response contains duplicate tool identity");
            }
            validate_identifier("tool call id", &call.id)?;
            if !matches!(
                call.name.as_str(),
                "read_file" | "write_file" | "run_command"
            ) {
                bail!("controller requested an unsupported experiment tool");
            }
            validate_nonempty_text("tool arguments", &call.arguments, MAX_TOOL_ARGUMENT_BYTES)?;
            let arguments: Value =
                serde_json::from_str(&call.arguments).context("decode tool arguments")?;
            if !arguments.is_object() {
                bail!("tool arguments must be a JSON object");
            }
            calls.push((
                call.index,
                PendingToolCall {
                    id: call.id,
                    name: call.name,
                    arguments_json: call.arguments,
                },
            ));
        }
        calls.sort_by_key(|(index, _)| *index);
        let calls = calls.into_iter().map(|(_, call)| call).collect::<Vec<_>>();

        match turn.finish_reason.as_str() {
            "stop" if calls.is_empty() => {}
            "tool_calls" if !calls.is_empty() => {}
            _ => bail!("controller response has an inconsistent finish reason"),
        }
        validate_tool_items_match_calls(&replay_items, &calls)?;

        let candidate_bytes = replay_items
            .iter()
            .try_fold(0usize, |total, (_, item)| {
                serde_json::to_vec(item)
                    .ok()
                    .and_then(|bytes| total.checked_add(bytes.len()))
            })
            .ok_or_else(|| anyhow!("controller replay items exceed their aggregate bound"))?;
        if candidate_bytes > MAX_CONTROLLER_RESPONSE_BYTES {
            bail!("controller replay items exceed their aggregate bound");
        }

        self.input
            .extend(replay_items.into_iter().map(|(_, item)| item));
        self.pending_calls = calls.clone();
        self.validate_context_bound()?;
        if calls.is_empty() {
            Ok(TurnDisposition::Stop {
                content: turn.content,
            })
        } else {
            Ok(TurnDisposition::ToolCalls(calls))
        }
    }

    pub fn accept_tool_result(&mut self, call_id: &str, result_json: &str) -> Result<()> {
        let next = self
            .pending_calls
            .first()
            .ok_or_else(|| anyhow!("no tool call is awaiting a result"))?;
        if next.id != call_id {
            bail!("tool results must be supplied in exact call order");
        }
        validate_nonempty_text("tool result", result_json, MAX_CONTROLLER_RESPONSE_BYTES)?;
        serde_json::from_str::<Value>(result_json).context("decode tool result")?;
        let item = json!({
            "type": "function_call_output",
            "call_id": call_id,
            "output": result_json,
        });
        let encoded = serde_json::to_vec(&item).context("encode tool result")?;
        if encoded.len() > MAX_CONTROLLER_RESPONSE_BYTES {
            bail!("tool result item exceeds its bound");
        }
        self.input.push(item);
        self.pending_calls.remove(0);
        self.validate_context_bound()
    }

    pub fn pending_calls(&self) -> &[PendingToolCall] {
        &self.pending_calls
    }

    fn validate_context_bound(&self) -> Result<()> {
        let bytes = serde_json::to_vec(&self.input)
            .context("encode Responses context for a conservative bound")?;
        let conservative_max = usize::try_from(MAX_INPUT_TOKENS_PER_REQUEST)
            .unwrap_or(usize::MAX)
            .saturating_mul(4);
        if bytes.len() > conservative_max {
            bail!("Responses context exceeds the conservative 64K-token byte bound");
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ControllerTurnV1 {
    content: String,
    reasoning: Option<String>,
    response_items: Vec<ControllerResponseItemV1>,
    tool_calls: Vec<ControllerToolCallV1>,
    finish_reason: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ControllerResponseItemV1 {
    output_index: usize,
    item: Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ControllerToolCallV1 {
    index: usize,
    id: String,
    name: String,
    arguments: String,
}

fn experiment_tools() -> Vec<Value> {
    vec![
        json!({
            "type": "function",
            "name": "read_file",
            "description": "Read one explicitly granted UTF-8 workspace file.",
            "strict": true,
            "parameters": {
                "type": "object",
                "properties": {"path": {"type": "string"}},
                "required": ["path"],
                "additionalProperties": false
            }
        }),
        json!({
            "type": "function",
            "name": "write_file",
            "description": "Atomically rewrite one explicitly granted UTF-8 workspace file.",
            "strict": true,
            "parameters": {
                "type": "object",
                "properties": {
                    "path": {"type": "string"},
                    "content": {"type": "string"}
                },
                "required": ["path", "content"],
                "additionalProperties": false
            }
        }),
        json!({
            "type": "function",
            "name": "run_command",
            "description": "Run one exact non-shell Cargo or Rustc argv under the experiment sandbox.",
            "strict": true,
            "parameters": {
                "type": "object",
                "properties": {
                    "executable": {"type": "string"},
                    "arguments": {"type": "array", "items": {"type": "string"}}
                },
                "required": ["executable", "arguments"],
                "additionalProperties": false
            }
        }),
    ]
}

fn validate_output_schema(name: &str, schema: Value) -> Result<(String, Value)> {
    validate_identifier("output schema name", name)?;
    if !schema.is_object()
        || serde_json::to_vec(&schema)
            .context("encode output schema")?
            .len()
            > MAX_PROMPT_BYTES
    {
        bail!("output schema is invalid or exceeds its bound");
    }
    Ok((name.into(), schema))
}

fn validate_response_item(item: &Value) -> Result<()> {
    let object = item
        .as_object()
        .ok_or_else(|| anyhow!("Responses replay item must be an object"))?;
    let kind = object
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("Responses replay item lacks a type"))?;
    if !matches!(kind, "reasoning" | "message" | "function_call") {
        bail!("Responses replay item has an unsupported type");
    }
    Ok(())
}

fn validate_tool_items_match_calls(
    replay_items: &[(usize, Value)],
    calls: &[PendingToolCall],
) -> Result<()> {
    let raw_ids = replay_items
        .iter()
        .filter_map(|(_, item)| {
            (item.get("type").and_then(Value::as_str) == Some("function_call"))
                .then(|| item.get("call_id").and_then(Value::as_str))
                .flatten()
        })
        .collect::<Vec<_>>();
    if raw_ids.len() != calls.len() || raw_ids.iter().zip(calls).any(|(raw, call)| *raw != call.id)
    {
        bail!("Responses function-call items do not match the controller tool calls");
    }
    Ok(())
}

fn validate_identifier(label: &str, value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 256
        || value
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
    {
        bail!("{label} is invalid");
    }
    Ok(())
}

fn validate_nonempty_text(label: &str, value: &str, maximum: usize) -> Result<()> {
    validate_text(label, value, maximum)?;
    if value.is_empty() {
        bail!("{label} is empty");
    }
    Ok(())
}

fn validate_text(label: &str, value: &str, maximum: usize) -> Result<()> {
    if value.len() > maximum || value.chars().any(|character| character == '\0') {
        bail!("{label} is invalid or exceeds its bound");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reasoning_experiment_protocol::CHEAP_MODEL;

    fn conversation(tools: bool) -> PhaseConversation {
        PhaseConversation::new(
            ExperimentModelRouteV1::openai(CHEAP_MODEL),
            "Fix the fixture and obey the tool policy.",
            "Repair the parser boundary bug.",
            tools,
        )
        .unwrap()
    }

    #[test]
    fn request_freezes_responses_options_and_exact_tools() {
        let request: Value =
            serde_json::from_str(&conversation(true).request_json().unwrap()).unwrap();
        assert_eq!(request["model"], CHEAP_MODEL);
        assert_eq!(request["stream"], true);
        assert_eq!(request["store"], false);
        assert_eq!(request["service_tier"], "default");
        assert_eq!(request["reasoning"]["effort"], "high");
        assert_eq!(request["include"], json!(["reasoning.encrypted_content"]));
        assert_eq!(request["tools"].as_array().unwrap().len(), 3);
        assert!(request.get("previous_response_id").is_none());
        assert!(request.get("prompt_cache_key").is_none());

        let no_tools: Value =
            serde_json::from_str(&conversation(false).request_json().unwrap()).unwrap();
        assert!(no_tools.get("tools").is_none());

        let structured = PhaseConversation::new_with_output_schema(
            ExperimentModelRouteV1::openai(CHEAP_MODEL),
            "Return the exact schema.",
            "Plan the fixture.",
            false,
            Some((
                "planner_proposal",
                json!({
                    "type": "object",
                    "properties": {"objective": {"type": "string"}},
                    "required": ["objective"],
                    "additionalProperties": false
                }),
            )),
        )
        .unwrap();
        let structured: Value = serde_json::from_str(&structured.request_json().unwrap()).unwrap();
        assert_eq!(structured["text"]["format"]["type"], "json_schema");
        assert_eq!(structured["text"]["format"]["strict"], true);
        assert_eq!(structured["text"]["format"]["name"], "planner_proposal");
    }

    #[test]
    fn completed_output_sequence_and_tool_result_replay_in_exact_order() {
        let mut conversation = conversation(true);
        let turn = json!({
            "content": "",
            "reasoning": "private-display-only",
            "response_items": [
                {"output_index": 2, "item": {
                    "type": "function_call", "id": "fc_1", "call_id": "call_1",
                    "name": "read_file", "arguments": "{\"path\":\"src/lib.rs\"}",
                    "status": "completed"
                }},
                {"output_index": 0, "item": {
                    "type": "reasoning", "id": "rs_1",
                    "encrypted_content": "opaque-sentinel", "summary": []
                }}
            ],
            "tool_calls": [{
                "index": 0, "id": "call_1", "name": "read_file",
                "arguments": "{\"path\":\"src/lib.rs\"}"
            }],
            "finish_reason": "tool_calls"
        });
        let disposition = conversation
            .accept_broker_turn(&serde_json::to_string(&turn).unwrap())
            .unwrap();
        assert!(matches!(disposition, TurnDisposition::ToolCalls(_)));
        conversation
            .accept_tool_result("call_1", r#"{"kind":"read","content":"fixture"}"#)
            .unwrap();

        let request: Value = serde_json::from_str(&conversation.request_json().unwrap()).unwrap();
        let input = request["input"].as_array().unwrap();
        assert_eq!(input[2]["type"], "reasoning");
        assert_eq!(input[3]["type"], "function_call");
        assert_eq!(input[4]["type"], "function_call_output");
        assert_eq!(input[4]["call_id"], "call_1");
        assert_eq!(input[4]["output"], r#"{"kind":"read","content":"fixture"}"#);
    }

    #[test]
    fn invalid_turn_does_not_mutate_conversation() {
        let mut conversation = conversation(true);
        let before = conversation.request_json().unwrap();
        let invalid = json!({
            "content": "",
            "reasoning": null,
            "response_items": [
                {"output_index": 0, "item": {"type": "reasoning", "id": "r"}},
                {"output_index": 0, "item": {"type": "reasoning", "id": "r2"}}
            ],
            "tool_calls": [{
                "index": 0, "id": "call_1", "name": "shell",
                "arguments": "{}"
            }],
            "finish_reason": "tool_calls"
        });
        assert!(conversation
            .accept_broker_turn(&serde_json::to_string(&invalid).unwrap())
            .is_err());
        assert_eq!(conversation.request_json().unwrap(), before);
        assert!(conversation.pending_calls().is_empty());
    }

    #[test]
    fn debug_redacts_prompts_arguments_and_opaque_items() {
        let mut conversation = conversation(true);
        let turn = json!({
            "content": "",
            "reasoning": "reasoning-sentinel",
            "response_items": [{"output_index": 0, "item": {
                "type": "function_call", "id": "fc", "call_id": "call_opaque",
                "name": "read_file", "arguments": "{\"path\":\"secret.rs\"}"
            }}],
            "tool_calls": [{
                "index": 0, "id": "call_opaque", "name": "read_file",
                "arguments": "{\"path\":\"secret.rs\"}"
            }],
            "finish_reason": "tool_calls"
        });
        let calls = match conversation
            .accept_broker_turn(&serde_json::to_string(&turn).unwrap())
            .unwrap()
        {
            TurnDisposition::ToolCalls(calls) => calls,
            TurnDisposition::Stop { .. } => panic!("expected tool calls"),
        };
        let debug = format!("{conversation:?} {calls:?}");
        for sentinel in ["reasoning-sentinel", "secret.rs", "call_opaque\",\"name"] {
            assert!(!debug.contains(sentinel));
        }
    }
}
