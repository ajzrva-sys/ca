//! Private, bounded native content. Display text is never a source of signatures.
use crate::agent::client::ChatMessage;
use crate::agent::stream::{ProviderError, ProviderResponseItem, ProviderStateSourceV1};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const MAX_CAPTURE_BYTES: usize = 1024 * 1024;
const MAX_CAPTURE_BLOCKS: usize = 128;

pub(crate) fn prefix_digest(value: &Value) -> String {
    format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(value).expect("JSON serializes"))
    )
}

pub(crate) fn prefix_from_body(body: &[u8]) -> Value {
    let value: Value = serde_json::from_slice(body).expect("validated provider body");
    json!({"system": value.get("system").cloned().unwrap_or(json!([])),
        "tools": value.get("tools").cloned().unwrap_or(json!([])), "messages": value["messages"]})
}

pub(crate) struct AnthropicCapture {
    source: ProviderStateSourceV1,
    model: String,
    prefix: String,
    blocks: Vec<Value>,
    arguments: Vec<String>,
    bytes: usize,
    eligible: bool,
}

impl std::fmt::Debug for AnthropicCapture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnthropicCapture")
            .field("blocks", &self.blocks.len())
            .field("bytes", &self.bytes)
            .finish_non_exhaustive()
    }
}

impl AnthropicCapture {
    pub(crate) fn bind_sent_prefix(&mut self, prefix: &str) {
        self.prefix = prefix.to_owned();
    }
    pub(crate) fn new(source: ProviderStateSourceV1, model: String, body: &[u8]) -> Self {
        Self {
            source,
            model,
            prefix: prefix_digest(&prefix_from_body(body)),
            blocks: Vec::new(),
            arguments: Vec::new(),
            bytes: 0,
            eligible: true,
        }
    }

    pub(crate) fn accept(
        &mut self,
        frame: &Value,
    ) -> Result<Option<ProviderResponseItem>, ProviderError> {
        let kind = frame["type"].as_str().unwrap_or("");
        if matches!(kind, "content_block_start" | "content_block_delta") {
            self.bytes = self
                .bytes
                .saturating_add(serde_json::to_vec(frame).map_or(MAX_CAPTURE_BYTES, |v| v.len()));
            if self.bytes > MAX_CAPTURE_BYTES {
                return Err(ProviderError::EventTooLarge {
                    limit: MAX_CAPTURE_BYTES,
                });
            }
        }
        match kind {
            "content_block_start" => {
                if frame["index"].as_u64() != Some(self.blocks.len() as u64)
                    || self.blocks.len() >= MAX_CAPTURE_BLOCKS
                {
                    return Err(ProviderError::Protocol(
                        "native content block order or count is invalid".into(),
                    ));
                }
                self.blocks.push(frame["content_block"].clone());
                self.arguments.push(String::new());
            }
            "content_block_delta" => {
                let index = frame["index"]
                    .as_u64()
                    .ok_or_else(|| ProviderError::Protocol("missing native block index".into()))?
                    as usize;
                let block = self.blocks.get_mut(index).ok_or_else(|| {
                    ProviderError::Protocol("native delta without a block".into())
                })?;
                let delta = &frame["delta"];
                match delta["type"].as_str().unwrap_or("") {
                    "text_delta" => append(block, "text", &delta["text"]),
                    "thinking_delta" => append(block, "thinking", &delta["thinking"]),
                    "signature_delta" => {
                        block["signature"] = delta["signature"].clone();
                    }
                    "input_json_delta" => {
                        self.arguments[index]
                            .push_str(delta["partial_json"].as_str().unwrap_or(""));
                    }
                    "citations_delta" => {
                        if block.get("citations").is_none() {
                            block["citations"] = json!([]);
                        }
                        if let Some(citations) = block["citations"].as_array_mut() {
                            citations.push(delta["citation"].clone());
                        }
                    }
                    _ => {}
                }
            }
            "content_block_stop" => {
                let index = frame["index"]
                    .as_u64()
                    .ok_or_else(|| ProviderError::Protocol("missing native block index".into()))?
                    as usize;
                let block = self
                    .blocks
                    .get_mut(index)
                    .ok_or_else(|| ProviderError::Protocol("native stop without a block".into()))?;
                if block["type"] == "tool_use" && !self.arguments[index].is_empty() {
                    let input: Value =
                        serde_json::from_str(&self.arguments[index]).map_err(|_| {
                            ProviderError::Protocol("native tool input is malformed".into())
                        })?;
                    if !input.is_object() {
                        return Err(ProviderError::Protocol(
                            "native tool input is not an object".into(),
                        ));
                    }
                    block["input"] = input;
                }
                if block["type"] == "thinking"
                    && block["signature"].as_str().is_none_or(str::is_empty)
                {
                    return Err(ProviderError::Protocol(
                        "thinking block has no complete signature".into(),
                    ));
                }
            }
            "message_delta" => {
                if let Some(reason) = frame["delta"]["stop_reason"].as_str() {
                    self.eligible = matches!(reason, "end_turn" | "stop_sequence" | "tool_use");
                }
            }
            "message_stop" if !self.eligible => return Ok(None),
            "message_stop" => {
                return Ok(Some(ProviderResponseItem::NativeContent {
                    source: self.source.clone(),
                    model: self.model.clone(),
                    prefix_sha256: self.prefix.clone(),
                    content: std::mem::take(&mut self.blocks),
                }))
            }
            _ => {}
        }
        Ok(None)
    }
}

fn append(block: &mut Value, field: &str, delta: &Value) {
    let mut value = block[field].as_str().unwrap_or("").to_owned();
    value.push_str(delta.as_str().unwrap_or(""));
    block[field] = Value::String(value);
}

pub(crate) fn anthropic_replay(
    message: &ChatMessage,
    source: &ProviderStateSourceV1,
    model: &str,
    prefix: Option<&Value>,
    valid: &mut bool,
) -> Result<Option<Vec<Value>>, ProviderError> {
    let mut found = None;
    for item in &message.response_items {
        let ProviderResponseItem::NativeContent {
            source: origin,
            model: original_model,
            prefix_sha256,
            content,
        } = item
        else {
            continue;
        };
        if origin != source || original_model != model {
            *valid = false;
            continue;
        }
        if found.is_some() {
            return Err(ProviderError::Protocol(
                "duplicate native replay state".into(),
            ));
        }
        if content.len() > MAX_CAPTURE_BLOCKS
            || serde_json::to_vec(content).map_or(usize::MAX, |v| v.len()) > MAX_CAPTURE_BYTES
        {
            return Err(ProviderError::EventTooLarge {
                limit: MAX_CAPTURE_BYTES,
            });
        }
        validate_assistant(message, content)?;
        *valid &= prefix.is_none_or(|prefix| *prefix_sha256 == prefix_digest(prefix));
        found = Some(
            content
                .iter()
                .filter(|block| {
                    *valid
                        || !matches!(
                            block["type"].as_str(),
                            Some("thinking" | "redacted_thinking")
                        )
                })
                .cloned()
                .collect(),
        );
    }
    Ok(found)
}

fn validate_assistant(message: &ChatMessage, content: &[Value]) -> Result<(), ProviderError> {
    let mut text = content
        .iter()
        .filter(|block| block["type"] == "text")
        .filter_map(|block| block["text"].as_str())
        .collect::<String>();
    let calls = content
        .iter()
        .filter(|block| block["type"] == "tool_use")
        .collect::<Vec<_>>();
    let expected = message.tool_calls.as_deref().unwrap_or_default();
    if message.content.as_str() != Some(text.as_str()) {
        crate::provider_filter::restore_view_in_place(&mut text);
    }
    if message.content.as_str() != Some(text.as_str())
        || calls.len() != expected.len()
        || calls.iter().zip(expected).any(|(block, call)| {
            block["id"] != call.id
                || block["name"] != call.function.name
                || !tool_input_matches(&call.function.arguments, &block["input"])
        })
    {
        return Err(ProviderError::Protocol(
            "native replay differs from canonical assistant content".into(),
        ));
    }
    Ok(())
}

fn tool_input_matches(arguments: &str, input: &Value) -> bool {
    let canonical = serde_json::from_str::<Value>(arguments).ok();
    if canonical.as_ref() == Some(input) {
        return true;
    }
    // Restore string values, as the tool assembler does, without treating JSON
    // escaping or object keys as provider-visible text.
    canonical == Some(crate::provider_filter::restore_local_value(input))
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ReplayBindings {
    pub google_parts: Vec<usize>,
    pub messages: Vec<(usize, String)>,
    pub request_sequence: u64,
}
#[derive(Clone)]
pub(crate) struct SentPrefix(pub String);

/// Protect exact provider-issued assistant arrays from transformations, then
/// check signatures against the actual outgoing prefix. Stop rules see final bytes.
pub(crate) fn prepare_outbound(
    body: &[u8],
    bindings: &ReplayBindings,
    filter: &crate::provider_filter::ProviderFilter,
) -> Result<(Vec<u8>, Vec<String>), ProviderError> {
    if !bindings.google_parts.is_empty() {
        let mut value: Value = serde_json::from_slice(body)
            .map_err(|_| ProviderError::Protocol("Google request is not JSON".into()))?;
        let mut originals = Vec::new();
        for index in &bindings.google_parts {
            let parts = value
                .pointer_mut(&format!("/contents/{index}/parts"))
                .ok_or_else(|| {
                    ProviderError::Protocol("Google replay binding has no parts".into())
                })?;
            originals.push((*index, std::mem::replace(parts, json!([0]))));
        }
        let filtered =
            filter.prepare_request(&serde_json::to_vec(&value).expect("JSON serializes"))?;
        value = serde_json::from_slice(&filtered)
            .map_err(|_| ProviderError::Protocol("filter produced invalid Google JSON".into()))?;
        for (index, parts) in originals {
            value["contents"][index]["parts"] = parts;
        }
        let body = serde_json::to_vec(&value).expect("JSON serializes");
        filter.scan_prepared_request(&body)?;
        return Ok((body, vec![]));
    }
    if bindings.messages.is_empty() {
        return Ok((filter.prepare_request(body)?, Vec::new()));
    }
    let mut value: Value = serde_json::from_slice(body)
        .map_err(|_| ProviderError::Protocol("native request is not JSON".into()))?;
    let mut originals = Vec::new();
    for (index, _) in &bindings.messages {
        let content = value
            .pointer_mut(&format!("/messages/{index}/content"))
            .ok_or_else(|| {
                ProviderError::Protocol("native replay binding has no content".into())
            })?;
        originals.push((*index, std::mem::replace(content, json!([0]))));
    }
    let filtered = filter.prepare_request(&serde_json::to_vec(&value).expect("JSON serializes"))?;
    value = serde_json::from_slice(&filtered)
        .map_err(|_| ProviderError::Protocol("filter produced invalid native JSON".into()))?;
    for (index, content) in originals {
        value["messages"][index]["content"] = content;
    }
    let mut valid = true;
    let mut diagnostics = Vec::new();
    for (index, expected) in &bindings.messages {
        let prefix = json!({"system": value.get("system").cloned().unwrap_or(json!([])), "tools":value.get("tools").cloned().unwrap_or(json!([])), "messages": &value["messages"].as_array().ok_or_else(|| ProviderError::Protocol("native history is invalid".into()))?[..*index]});
        if valid && *expected != prefix_digest(&prefix) {
            diagnostics.push(
                "Native thinking replay omitted: final outgoing request prefix changed.".into(),
            );
            valid = false;
        }
        if !valid {
            if let Some(content) = value["messages"][*index]["content"].as_array_mut() {
                content.retain(|block| {
                    !matches!(
                        block["type"].as_str(),
                        Some("thinking" | "redacted_thinking")
                    )
                });
            }
        }
    }
    // A redacted-only assistant can disappear after invalidation. Its canonical
    // record is retained; only the wire projection is shortened.
    if !valid {
        value["messages"].as_array_mut().unwrap().retain(|message| {
            message["role"] != "assistant"
                || message["content"]
                    .as_array()
                    .is_none_or(|content| !content.is_empty())
        });
    }
    let bytes = serde_json::to_vec(&value).expect("JSON serializes");
    filter.scan_prepared_request(&bytes)?;
    Ok((bytes, diagnostics))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::client::{FunctionCall, ToolCallMsg};
    use crate::agent::ProviderProtocol;
    fn capture() -> AnthropicCapture {
        AnthropicCapture::new(
            ProviderStateSourceV1::for_test("anthropic", ProviderProtocol::AnthropicMessages),
            "claude-test".into(),
            br#"{"messages":[{"role":"user","content":[{"type":"text","text":"hi"}]}]}"#,
        )
    }
    #[test]
    fn native_signed_empty_and_redacted_blocks_roundtrip_without_display_reconstruction() {
        let mut c = capture();
        let frames = [
            json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"old"}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"opaque"}}),
            json!({"type":"content_block_stop","index":0}),
            json!({"type":"content_block_start","index":1,"content_block":{"type":"redacted_thinking","data":"private"}}),
            json!({"type":"content_block_stop","index":1}),
        ];
        for frame in &frames {
            assert!(c.accept(frame).unwrap().is_none());
        }
        let item = c.accept(&json!({"type":"message_stop"})).unwrap().unwrap();
        assert!(!format!("{item:?}").contains("opaque"));
        let restored: ProviderResponseItem =
            serde_json::from_slice(&serde_json::to_vec(&item).unwrap()).unwrap();
        assert_eq!(item, restored);
        let message = ChatMessage {
            role: "assistant".into(),
            content: crate::image_content::ModelContent::text(""),
            tool_calls: None,
            tool_call_id: None,
            response_items: vec![restored],
        };
        let prefix = prefix_from_body(
            br#"{"messages":[{"role":"user","content":[{"type":"text","text":"hi"}]}]}"#,
        );
        let mut valid = true;
        let replay = anthropic_replay(&message, &c.source, &c.model, Some(&prefix), &mut valid)
            .unwrap()
            .unwrap();
        assert!(valid);
        assert_eq!(replay[0]["signature"], "opaque");
        assert_eq!(replay[1]["data"], "private");
        let mut changed = prefix;
        changed["system"] = json!([{"type":"text","text":"new instructions"}]);
        assert!(
            anthropic_replay(&message, &c.source, &c.model, Some(&changed), &mut valid)
                .unwrap()
                .unwrap()
                .is_empty()
        );
        assert!(!valid);
        assert_eq!(message.response_items, vec![item]);
    }
    #[test]
    fn native_unsigned_or_out_of_order_capture_is_rejected() {
        let mut c = capture();
        assert!(c.accept(&json!({"type":"content_block_start","index":2,"content_block":{"type":"text","text":""}})).is_err());
        let mut c = capture();
        c.accept(&json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"why"}})).unwrap();
        assert!(c
            .accept(&json!({"type":"content_block_stop","index":0}))
            .is_err());
    }
    #[test]
    fn native_tool_replay_restores_escaped_values_without_rewriting_keys() {
        use crate::provider_filter::{with_test_filter, ContentAdjustment, ProviderFilterConfig};
        for original in ["quoted\"path.txt", "back\\slash.txt", "line\nbreak.txt"] {
            with_test_filter(
                &ProviderFilterConfig {
                    enabled: true,
                    adjustments: vec![ContentAdjustment {
                        from: original.into(),
                        to: "masked-path".into(),
                    }],
                    ..Default::default()
                },
                || {
                    let input = json!({"path":"masked-path", "nested":["masked-path"], "masked-path":"ordinary"});
                    let arguments =
                        crate::agent::stream::restored_tool_arguments(&input.to_string());
                    assert_eq!(
                        serde_json::from_str::<Value>(&arguments).unwrap(),
                        json!({"path":original,"nested":[original],"masked-path":"ordinary"})
                    );
                    let source = ProviderStateSourceV1::for_test(
                        "anthropic",
                        ProviderProtocol::AnthropicMessages,
                    );
                    let prefix = json!({"system":[],"tools":[],"messages":[]});
                    let content = vec![
                        json!({"type":"thinking","thinking":"","signature":"exact-signature"}),
                        json!({"type":"tool_use","id":"call-1","name":"read_file","input":input}),
                    ];
                    let message = ChatMessage {
                        role: "assistant".into(),
                        content: crate::image_content::ModelContent::text(""),
                        tool_calls: Some(vec![ToolCallMsg {
                            id: "call-1".into(),
                            call_type: "function".into(),
                            function: FunctionCall {
                                name: "read_file".into(),
                                arguments,
                            },
                        }]),
                        tool_call_id: None,
                        response_items: vec![ProviderResponseItem::NativeContent {
                            source: source.clone(),
                            model: "claude-test".into(),
                            prefix_sha256: prefix_digest(&prefix),
                            content: content.clone(),
                        }],
                    };
                    let mut valid = true;
                    assert_eq!(
                        anthropic_replay(
                            &message,
                            &source,
                            "claude-test",
                            Some(&prefix),
                            &mut valid
                        )
                        .unwrap(),
                        Some(content)
                    );
                    assert!(valid);
                },
            );
        }
    }
}

#[cfg(test)]
mod final_wire_tests {
    use super::*;
    use crate::provider_filter::{
        ContentAdjustment, ProviderFilter, ProviderFilterConfig, ProviderFilterKind,
        ProviderFilterRule, ProviderFilterTarget,
    };
    fn fixture() -> (Value, ReplayBindings) {
        let prefix = json!({"system":[],"tools":[],"messages":[{"role":"user","content":[{"type":"text","text":"public request"}]}]});
        let body = json!({"messages":[{"role":"user","content":[{"type":"text","text":"private request"}]},{"role":"assistant","content":[{"type":"thinking","thinking":"private thought","signature":"private signature"},{"type":"redacted_thinking","data":"private opaque"},{"type":"text","text":"private answer"}]}]});
        (
            body,
            ReplayBindings {
                google_parts: Vec::new(),
                messages: vec![(1, prefix_digest(&prefix))],
                request_sequence: 0,
            },
        )
    }
    #[test]
    fn native_final_wire_preserves_issued_arrays_and_binds_filtered_prefix() {
        let filter = ProviderFilter::compile(&ProviderFilterConfig {
            enabled: true,
            adjustments: vec![ContentAdjustment {
                from: "private".into(),
                to: "public".into(),
            }],
            ..Default::default()
        })
        .unwrap();
        let (body, bindings) = fixture();
        let (bytes, diagnostics) =
            prepare_outbound(&serde_json::to_vec(&body).unwrap(), &bindings, &filter).unwrap();
        let sent: Value = serde_json::from_slice(&bytes).unwrap();
        assert!(diagnostics.is_empty());
        assert_eq!(sent["messages"][0]["content"][0]["text"], "public request");
        assert_eq!(
            sent["messages"][1]["content"],
            body["messages"][1]["content"]
        );
        assert_eq!(
            prefix_digest(&prefix_from_body(
                &serde_json::to_vec(&json!({"messages":[sent["messages"][0].clone()]})).unwrap()
            )),
            bindings.messages[0].1
        );
    }
    #[test]
    fn native_final_wire_still_scans_immutable_opaque_bytes_for_stop_rules() {
        let filter = ProviderFilter::compile(&ProviderFilterConfig {
            enabled: true,
            rules: vec![ProviderFilterRule {
                id: "opaque-rule".into(),
                target: ProviderFilterTarget::Request,
                kind: ProviderFilterKind::Literal,
                pattern: "private signature".into(),
            }],
            ..Default::default()
        })
        .unwrap();
        let (body, mut bindings) = fixture();
        bindings.messages[0].1 = prefix_digest(
            &json!({"system":[],"tools":[],"messages":[body["messages"][0].clone()]}),
        );
        assert!(matches!(
            prepare_outbound(&serde_json::to_vec(&body).unwrap(), &bindings, &filter),
            Err(ProviderError::FilterStopped { .. })
        ));
    }
    #[test]
    fn native_final_wire_drops_invalid_thinking_and_retains_canonical_bytes() {
        let filter = ProviderFilter::compile(&ProviderFilterConfig::default()).unwrap();
        let (body, bindings) = fixture();
        let canonical = body.clone();
        let (bytes, diagnostics) =
            prepare_outbound(&serde_json::to_vec(&body).unwrap(), &bindings, &filter).unwrap();
        let sent: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(
            sent["messages"][1]["content"],
            json!([{"type":"text","text":"private answer"}])
        );
        assert_eq!(body, canonical);
    }
}
