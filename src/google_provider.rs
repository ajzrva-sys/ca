//! Native GenerateContent lowering and bounded SSE decoding, shared by AI Studio/Vertex.
use crate::agent::client::ChatMessage;
use crate::agent::stream::{
    FinishReason, ProviderError, ProviderEvent, ProviderResponseItem, ProviderStateSourceV1,
    UsageInfo,
};
use crate::request::PreparedModelRequest;
use serde_json::{json, Value};
use std::collections::BTreeMap;

const MAX_BYTES: usize = 1024 * 1024;
const MAX_PARTS: usize = 4096;

const SCHEMA_REJECTION: &str = "tool schema rejected under reject_lossy: ";
fn schema_rejection(translation: &crate::tool_schema::Translation<'_>) -> ProviderError {
    let safe = |value: &str, limit| {
        crate::terminal_text::sanitize_terminal_text(value)
            .chars()
            .take(limit)
            .collect::<String>()
    };
    let losses = translation
        .losses
        .iter()
        .take(31)
        .map(
            |loss| json!({"path":safe(&loss.path,192),"category":loss.category,"count":loss.count}),
        )
        .collect::<Vec<_>>();
    let report = json!({"tool":safe(&translation.tool_name,128),"losses":losses,"omitted":translation.omitted_losses.saturating_add(translation.losses.len().saturating_sub(31))});
    ProviderError::Protocol(format!("{SCHEMA_REJECTION}{report}"))
}
/// Only the adapter's bounded schema-rejection envelope becomes pre-send diagnostics.
pub(crate) fn rejected_schema_diagnostics(error: &ProviderError) -> Option<Vec<String>> {
    let ProviderError::Protocol(message) = error else {
        return None;
    };
    let encoded = message.strip_prefix(SCHEMA_REJECTION)?;
    if encoded.len() > 32 * 1024 {
        return None;
    }
    let report: Value = serde_json::from_str(encoded).ok()?;
    let tool = report["tool"].as_str()?;
    let losses = report["losses"].as_array()?;
    let omitted = report["omitted"].as_u64()?;
    if tool.chars().count() > 128 || losses.len() > 31 || (losses.is_empty() && omitted == 0) {
        return None;
    }
    let safe = |value: &str, limit| {
        crate::terminal_text::sanitize_terminal_text(value)
            .chars()
            .take(limit)
            .collect::<String>()
    };
    let mut diagnostics = Vec::new();
    for loss in losses {
        let path = loss["path"].as_str()?;
        let category = loss["category"].as_str()?;
        let count = loss["count"].as_u64()?;
        if path.chars().count() > 192
            || category.len() > 64
            || !category
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte == b'_')
            || count == 0
        {
            return None;
        }
        diagnostics.push(
            format!(
                "Rejected tool {}: {} at {} ({}).",
                safe(tool, 64),
                category,
                safe(path, 128),
                count
            )
            .chars()
            .take(256)
            .collect(),
        );
    }
    if omitted > 0 {
        diagnostics.push(format!("Additional schema losses omitted: {omitted}."));
    }
    Some(diagnostics)
}
pub(crate) fn model_profile(model: &str) -> Option<crate::config::ModelProfile> {
    let model = model.strip_prefix("models/").unwrap_or(model);
    if !model.starts_with("gemini-")
        || !(model.contains("flash") || model.contains("pro"))
        || ["image", "tts", "audio", "robotics"]
            .iter()
            .any(|kind| model.contains(kind))
    {
        return None;
    }
    use crate::config::ReasoningEffort::{High, Low, Max, Medium, Minimal, None as Off, XHigh};
    let (reasoning_efforts, default_reasoning_effort) = if model.starts_with("gemini-2.5-") {
        let mut efforts = vec![Minimal, Low, Medium, High, XHigh, Max];
        if model.contains("flash") {
            efforts.insert(0, Off);
        }
        let default = if model.contains("flash-lite") {
            Off
        } else {
            High
        };
        (efforts, Some(default))
    } else if model.starts_with("gemini-3") {
        let mut efforts = vec![Low, Medium, High];
        if model.starts_with("gemini-3-pro") {
            efforts.retain(|effort| *effort != Medium);
        }
        if model.contains("flash") && !model.starts_with("gemini-3.8-") {
            efforts.insert(0, Minimal);
        }
        let default = if model.contains("flash-lite") {
            Minimal
        } else if model.starts_with("gemini-3.5-")
            || model.starts_with("gemini-3.6-")
            || model.starts_with("gemini-3.8-")
        {
            Medium
        } else {
            High
        };
        (efforts, Some(default))
    } else {
        (Vec::new(), None)
    };
    Some(crate::config::ModelProfile {
        supports_tools: Some(true),
        supports_images: Some(true),
        reasoning_efforts,
        default_reasoning_effort,
        ..Default::default()
    })
}
pub(crate) fn model_path(model: &str) -> Result<String, ProviderError> {
    let id = model.strip_prefix("models/").unwrap_or(model);
    if id.is_empty()
        || id.len() > 256
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'@'))
    {
        return Err(ProviderError::Protocol(
            "Google model must be a bare model ID or models/ID".into(),
        ));
    }
    Ok(format!("models/{id}:streamGenerateContent?alt=sse"))
}
pub(crate) fn sent_prefix(body: &[u8]) -> Result<String, ProviderError> {
    let mut value: Value = serde_json::from_slice(body)
        .map_err(|_| ProviderError::Protocol("Google request is invalid JSON".into()))?;
    value
        .as_object_mut()
        .ok_or_else(|| ProviderError::Protocol("Google request is not an object".into()))?
        .remove("generationConfig");
    Ok(crate::provider_replay::prefix_digest(&value))
}

pub(crate) type LoweredContent = (Vec<u8>, Vec<String>, Vec<usize>);

pub(crate) fn body(
    request: &PreparedModelRequest,
    source: &ProviderStateSourceV1,
    policy: crate::tool_schema::Policy,
) -> Result<LoweredContent, ProviderError> {
    let mut system = Vec::new();
    let mut contents: Vec<Value> = Vec::new();
    let mut calls = BTreeMap::new();
    let mut wire_ids = BTreeMap::new();
    let mut replay = Vec::new();
    let mut diagnostics = Vec::new();
    let mut declarations = Vec::new();
    for tool in &request.tools {
        let t = crate::tool_schema::translate(
            &tool.function.name,
            &tool.function.parameters,
            crate::tool_schema::Profile::Google,
            crate::tool_schema::Policy::Compatible,
        )
        .map_err(ProviderError::Protocol)?;
        if policy == crate::tool_schema::Policy::RejectLossy
            && (!t.losses.is_empty() || t.omitted_losses > 0)
        {
            return Err(schema_rejection(&t));
        }
        for loss in t.losses {
            if diagnostics.len() < 32 {
                diagnostics.push(format!(
                    "Tool {}: {} at {} ({}).",
                    t.tool_name, loss.category, loss.path, loss.count
                ));
            }
        }
        if t.omitted_losses > 0 && diagnostics.len() < 32 {
            diagnostics.push(format!(
                "Additional schema losses omitted: {}.",
                t.omitted_losses
            ));
        }
        declarations.push(json!({"name":tool.function.name,"description":tool.function.description,"parameters":t.schema}));
    }
    let tools = if declarations.is_empty() {
        json!([])
    } else {
        json!([{"functionDeclarations":declarations}])
    };
    let mut valid = true;
    for message in &request.messages {
        match message.role.as_str() {
            "system" | "developer" => {
                system.push(json!({"text":message.content.as_str().ok_or_else(||ProviderError::Protocol("Google instructions must contain text".into()))?}));
            }
            "user" => contents.push(json!({"role":"user","parts":user_parts(message)})),
            "assistant" => {
                let mut parts = Vec::new();
                if let Some(text) = message.content.as_str().filter(|v| !v.is_empty()) {
                    parts.push(json!({"text":text}));
                }
                for call in message.tool_calls.as_deref().unwrap_or_default() {
                    let args: Value =
                        serde_json::from_str(&call.function.arguments).map_err(|_| {
                            ProviderError::Protocol("Google tool arguments are invalid JSON".into())
                        })?;
                    if !args.is_object() {
                        return Err(ProviderError::Protocol(
                            "Google tool arguments must be an object".into(),
                        ));
                    }
                    calls.insert(call.id.clone(), call.function.name.clone());
                    parts.push(json!({"functionCall":{"name":call.function.name,"args":args}}));
                }
                let mut selected = false;
                for item in &message.response_items {
                    if let ProviderResponseItem::NativeContent {
                        source: origin,
                        model,
                        prefix_sha256: _,
                        content,
                    } = item
                    {
                        if origin != source || model != &request.model {
                            valid = false;
                            continue;
                        }
                        if selected {
                            return Err(ProviderError::Protocol(
                                "duplicate Google replay state".into(),
                            ));
                        }
                        selected = true;
                        if content.len() > MAX_PARTS
                            || serde_json::to_vec(content).map_or(usize::MAX, |v| v.len())
                                > MAX_BYTES
                        {
                            return Err(ProviderError::EventTooLarge { limit: MAX_BYTES });
                        }
                        validate_parts(message, content)?;
                        for (part, call) in content
                            .iter()
                            .filter_map(|part| part.get("functionCall"))
                            .zip(message.tool_calls.as_deref().unwrap_or_default())
                        {
                            if let Some(wire_id) = part["id"].as_str() {
                                wire_ids.insert(call.id.clone(), wire_id.to_owned());
                            }
                        }
                        parts = content.clone();
                    }
                }
                if parts.is_empty() {
                    parts.push(json!({"text":""}));
                }
                if selected {
                    replay.push(contents.len());
                }
                contents.push(json!({"role":"model","parts":parts}));
            }
            "tool" => {
                let name = message
                    .tool_call_id
                    .as_ref()
                    .and_then(|id| calls.get(id))
                    .ok_or_else(|| {
                        ProviderError::Protocol("Google tool result has no matching call".into())
                    })?;
                let mut response = json!({"name":name,"response":{"result":message.content.as_str().unwrap_or("")}});
                if let Some(id) = message
                    .tool_call_id
                    .as_ref()
                    .and_then(|id| wire_ids.get(id))
                {
                    response["id"] = json!(id);
                }
                let part = json!({"functionResponse":response});
                if let Some(previous) = contents.last_mut().filter(|previous| {
                    previous["role"] == "user"
                        && previous["parts"].as_array().is_some_and(|parts| {
                            parts
                                .iter()
                                .all(|part| part.get("functionResponse").is_some())
                        })
                }) {
                    previous["parts"].as_array_mut().unwrap().push(part);
                } else {
                    contents.push(json!({"role":"user","parts":[part]}));
                }
            }
            _ => {
                return Err(ProviderError::Protocol(
                    "unsupported Google message role".into(),
                ));
            }
        }
    }
    if !valid {
        diagnostics.push(
            "Native thinking replay omitted: provider, endpoint, credential or model changed."
                .into(),
        );
    }
    let mut value = json!({"tools":tools,"contents":contents});
    if !system.is_empty() {
        value["systemInstruction"] = json!({"parts":system});
    }
    if let Some(effort) = request.options.reasoning_effort {
        use crate::config::ReasoningEffort::*;
        let config = if request
            .model
            .strip_prefix("models/")
            .unwrap_or(&request.model)
            .starts_with("gemini-2.5-")
        {
            json!({"thinkingBudget":match effort{None=>0,Minimal|Low=>1024,Medium=>4096,High=>8192,XHigh=>16384,Max=>24576}})
        } else {
            json!({"thinkingLevel":match effort{None|Minimal=>"MINIMAL",Low=>"LOW",Medium=>"MEDIUM",High|XHigh|Max=>"HIGH"}})
        };
        value["generationConfig"] = json!({"thinkingConfig":config});
    }
    let bytes = serde_json::to_vec(&value)
        .map_err(|_| ProviderError::Protocol("Google request serialization failed".into()))?;
    Ok((bytes, diagnostics, replay))
}
fn user_parts(message: &ChatMessage) -> Vec<Value> {
    message
        .content
        .blocks()
        .iter()
        .map(|block| match block {
            crate::image_content::ModelContentBlock::Text(text) => json!({"text":text}),
            crate::image_content::ModelContentBlock::Image(image) => {
                json!({"inlineData":{"mimeType":"image/png","data":image.base64()}})
            }
        })
        .collect()
}
fn validate_parts(message: &ChatMessage, parts: &[Value]) -> Result<(), ProviderError> {
    let mut text = parts
        .iter()
        .filter(|p| p["thought"] != true)
        .filter_map(|p| p["text"].as_str())
        .collect::<String>();
    if message.content.as_str() != Some(text.as_str()) {
        crate::provider_filter::restore_view_in_place(&mut text);
    }
    let calls = parts
        .iter()
        .filter_map(|p| p.get("functionCall"))
        .collect::<Vec<_>>();
    let expected = message.tool_calls.as_deref().unwrap_or_default();
    if message.content.as_str() != Some(text.as_str())
        || calls.len() != expected.len()
        || calls.iter().zip(expected).any(|(call, expected)| {
            call["name"] != expected.function.name || {
                let canonical = serde_json::from_str::<Value>(&expected.function.arguments).ok();
                let args = call.get("args").cloned().unwrap_or_else(|| json!({}));
                canonical.as_ref() != Some(&args)
                    && canonical != Some(crate::provider_filter::restore_local_value(&args))
            }
        })
    {
        return Err(ProviderError::Protocol(
            "Google replay differs from canonical assistant content".into(),
        ));
    }
    Ok(())
}

pub(crate) struct Decoder {
    buffer: Vec<u8>,
    data: Vec<String>,
    parts: Vec<Value>,
    source: ProviderStateSourceV1,
    model: String,
    prefix: String,
    bytes: usize,
    terminal: Option<FinishReason>,
    usage: Option<UsageInfo>,
    done: bool,
    calls: usize,
    nonce: String,
}
impl Decoder {
    pub(crate) fn for_request(
        request: &PreparedModelRequest,
        source: ProviderStateSourceV1,
    ) -> Self {
        let body = serde_json::to_vec(&json!({"messages":request.messages,"tools":request.tools}))
            .expect("typed request serializes");
        Self::new(request, source, &body)
    }
    pub(crate) fn bind_sent_prefix(&mut self, prefix: &str) {
        self.prefix = prefix.to_owned();
    }
    pub(crate) fn new(
        request: &PreparedModelRequest,
        source: ProviderStateSourceV1,
        body: &[u8],
    ) -> Self {
        let mut prefix: Value = serde_json::from_slice(body).expect("validated Google body");
        prefix.as_object_mut().unwrap().remove("generationConfig");
        Self {
            buffer: vec![],
            data: vec![],
            parts: vec![],
            source,
            model: request.model.clone(),
            prefix: crate::provider_replay::prefix_digest(&prefix),
            bytes: 0,
            terminal: None,
            usage: None,
            done: false,
            calls: 0,
            nonce: uuid::Uuid::new_v4().to_string(),
        }
    }
    pub(crate) fn push(&mut self, chunk: &[u8]) -> Result<Vec<ProviderEvent>, ProviderError> {
        let mut out = Vec::new();
        // Bound incomplete lines before extending, even if one network chunk is huge.
        for line in chunk.split_inclusive(|b| *b == b'\n') {
            if self.buffer.len().saturating_add(line.len()) > MAX_BYTES {
                return Err(ProviderError::EventTooLarge { limit: MAX_BYTES });
            }
            self.buffer.extend_from_slice(line);
            if self.buffer.last() == Some(&b'\n') {
                let line = String::from_utf8(std::mem::take(&mut self.buffer))
                    .map_err(|_| ProviderError::InvalidUtf8)?;
                if line.trim().is_empty() {
                    out.extend(self.dispatch()?);
                } else if let Some(data) = line.strip_prefix("data:") {
                    if self.data.len() >= 4096 {
                        return Err(ProviderError::EventTooLarge { limit: MAX_BYTES });
                    }
                    self.data.push(data.trim().to_owned());
                    if self
                        .data
                        .iter()
                        .map(|line| line.len().saturating_add(1))
                        .sum::<usize>()
                        > MAX_BYTES
                    {
                        return Err(ProviderError::EventTooLarge { limit: MAX_BYTES });
                    }
                }
            }
        }
        Ok(out)
    }
    fn dispatch(&mut self) -> Result<Vec<ProviderEvent>, ProviderError> {
        if self.data.is_empty() {
            return Ok(vec![]);
        }
        let data = std::mem::take(&mut self.data).join("\n");
        if self.done {
            return Err(ProviderError::Protocol(
                "Google event after terminal response".into(),
            ));
        }
        let value: Value = serde_json::from_str(&data)
            .map_err(|_| ProviderError::Protocol("Google stream contains malformed JSON".into()))?;
        if value.get("error").is_some() {
            return Err(ProviderError::ResponseFailed);
        }
        if value
            .pointer("/promptFeedback/blockReason")
            .and_then(Value::as_str)
            .is_some_and(|reason| !matches!(reason, "" | "BLOCK_REASON_UNSPECIFIED"))
        {
            return Err(ProviderError::ResponseRefused);
        }
        let mut events = Vec::new();
        if let Some(candidates) = value["candidates"].as_array() {
            if candidates.len() > 1 {
                return Err(ProviderError::Protocol(
                    "Google returned multiple candidates".into(),
                ));
            }
            if let Some(candidate) = candidates.first() {
                if self.terminal.is_some() && candidate.get("content").is_some() {
                    return Err(ProviderError::Protocol(
                        "Google content arrived after finish reason".into(),
                    ));
                }
                if let Some(parts) = candidate
                    .pointer("/content/parts")
                    .and_then(Value::as_array)
                {
                    for part in parts {
                        if part
                            .get("thoughtSignature")
                            .is_some_and(|signature| !signature.is_string())
                        {
                            return Err(ProviderError::Protocol(
                                "Google thought signature is not opaque text".into(),
                            ));
                        }
                        self.bytes = self.bytes.saturating_add(
                            serde_json::to_vec(part).map_or(MAX_BYTES, |v| v.len()),
                        );
                        if self.parts.len() >= MAX_PARTS || self.bytes > MAX_BYTES {
                            return Err(ProviderError::EventTooLarge { limit: MAX_BYTES });
                        }
                        if let Some(text) = part["text"].as_str() {
                            if !text.is_empty() {
                                events.push(if part["thought"] == true {
                                    ProviderEvent::Reasoning(text.into())
                                } else {
                                    ProviderEvent::Text(text.into())
                                });
                            }
                        }
                        if let Some(call) = part.get("functionCall") {
                            let name = call["name"].as_str().filter(|v| !v.is_empty()).ok_or_else(
                                || {
                                    ProviderError::Protocol(
                                        "Google function call has no name".into(),
                                    )
                                },
                            )?;
                            let args = call.get("args").cloned().unwrap_or_else(|| json!({}));
                            if !args.is_object() {
                                return Err(ProviderError::Protocol(
                                    "Google function call arguments must be an object".into(),
                                ));
                            }
                            let id = call["id"].as_str().map(str::to_owned).unwrap_or_else(|| {
                                format!("google-call-{}-{}", self.nonce, self.calls)
                            });
                            events.push(ProviderEvent::ToolCallStart {
                                index: self.calls,
                                id,
                                name: name.into(),
                            });
                            events.push(ProviderEvent::ToolCallArg {
                                index: self.calls,
                                arg: args.to_string(),
                            });
                            self.calls += 1;
                        }
                        self.parts.push(part.clone());
                    }
                }
                if let Some(reason) = candidate["finishReason"]
                    .as_str()
                    .filter(|reason| !matches!(*reason, "" | "FINISH_REASON_UNSPECIFIED"))
                {
                    if self.terminal.is_some() {
                        return Err(ProviderError::Protocol(
                            "duplicate Google finish reason".into(),
                        ));
                    }
                    self.terminal = Some(match reason {
                        "STOP" => {
                            if self.calls > 0 {
                                FinishReason::ToolCalls
                            } else {
                                FinishReason::Stop
                            }
                        }
                        "MAX_TOKENS" => FinishReason::Length,
                        "SAFETY" | "RECITATION" | "BLOCKLIST" | "PROHIBITED_CONTENT" | "SPII" => {
                            return Err(ProviderError::ResponseRefused);
                        }
                        other => FinishReason::Other(other.into()),
                    });
                }
            }
        }
        if let Some(usage) = value.get("usageMetadata") {
            let token = |key: &str| -> Result<u32, ProviderError> {
                match usage.get(key) {
                    None | Some(Value::Null) => Ok(0),
                    Some(value) => value
                        .as_u64()
                        .and_then(|n| u32::try_from(n).ok())
                        .ok_or_else(|| ProviderError::Protocol("Google usage is invalid".into())),
                }
            };
            let input = token("promptTokenCount")?;
            let reasoning = token("thoughtsTokenCount")?;
            let output = token("candidatesTokenCount")?
                .checked_add(reasoning)
                .ok_or_else(|| ProviderError::Protocol("Google usage overflow".into()))?;
            let cached = token("cachedContentTokenCount")?;
            if cached > input {
                return Err(ProviderError::Protocol(
                    "Google cache usage exceeds input tokens".into(),
                ));
            }
            self.usage = Some(UsageInfo {
                prompt_tokens: input,
                completion_tokens: output,
                total_tokens: token("totalTokenCount")?,
                cached_prompt_tokens: usage
                    .get("cachedContentTokenCount")
                    .filter(|value| !value.is_null())
                    .map(|_| cached),
                cache_write_tokens: None,
                reasoning_tokens: usage
                    .get("thoughtsTokenCount")
                    .filter(|value| !value.is_null())
                    .map(|_| reasoning),
                response_metadata: None,
                cache_usage_override: None,
            });
        }
        Ok(events)
    }
    pub(crate) fn finish(&mut self) -> Result<Vec<ProviderEvent>, ProviderError> {
        if !self.buffer.is_empty() {
            let tail = String::from_utf8(std::mem::take(&mut self.buffer))
                .map_err(|_| ProviderError::InvalidUtf8)?;
            if let Some(data) = tail.strip_prefix("data:") {
                self.data.push(data.trim().into());
            } else if !tail.trim().is_empty() {
                return Err(ProviderError::PrematureEof);
            }
        }
        let mut out = self.dispatch()?;
        let reason = self.terminal.clone().ok_or(ProviderError::PrematureEof)?;
        if matches!(reason, FinishReason::Stop | FinishReason::ToolCalls) {
            out.push(ProviderEvent::ResponseItem(
                ProviderResponseItem::NativeContent {
                    source: self.source.clone(),
                    model: self.model.clone(),
                    prefix_sha256: self.prefix.clone(),
                    content: self.parts.clone(),
                },
            ));
        }
        out.push(ProviderEvent::Finish {
            reason,
            usage: self.usage.clone(),
        });
        out.push(ProviderEvent::Done);
        self.done = true;
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::client::{
        ChatRequestOptions, FunctionCall, ToolCallMsg, ToolDef, ToolFunction,
    };
    use crate::agent::{stream::AssistantTurnAssembler, ProviderProtocol};
    use crate::image_content::ModelContent;
    fn message(role: &str, text: &str) -> ChatMessage {
        ChatMessage {
            role: role.into(),
            content: ModelContent::text(text),
            tool_calls: None,
            tool_call_id: None,
            response_items: vec![],
        }
    }
    fn request() -> PreparedModelRequest {
        PreparedModelRequest::new(
            crate::request::RequestKind::Headless,
            "fixture",
            "gemini-3-flash",
            vec![message("system", "instructions"), message("user", "hi")],
            vec![],
            ChatRequestOptions::default(),
        )
    }
    fn source() -> ProviderStateSourceV1 {
        ProviderStateSourceV1::for_test("fixture", ProviderProtocol::GoogleGenerateContent)
    }
    fn frame(value: Value) -> Vec<u8> {
        format!("data: {value}\n\n").into_bytes()
    }
    fn decode(
        value: Value,
        fragment: usize,
    ) -> Result<crate::agent::stream::AssembledAssistantTurn, ProviderError> {
        let request = request();
        let mut decoder = Decoder::new(&request, source(), b"{}");
        let mut assembler = AssistantTurnAssembler::new();
        for bytes in frame(value).chunks(fragment) {
            for event in decoder.push(bytes)? {
                assembler.push(&event)?;
            }
        }
        for event in decoder.finish()? {
            assembler.push(&event)?;
        }
        assembler.finish()
    }
    #[test]
    fn google_fragmented_parallel_calls_preserve_every_original_part_and_usage() {
        let parts = json!([{"text":"","thoughtSignature":"empty"},{"text":"reason","thought":true,"thoughtSignature":"reason"},{"functionCall":{"name":"read_file","args":{"path":"one"},"id":"wire-one"},"thoughtSignature":"call"},{"functionCall":{"name":"read_file","args":{"path":"two"}}}]);
        let turn=decode(json!({"candidates":[{"content":{"parts":parts},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":10,"candidatesTokenCount":3,"thoughtsTokenCount":2,"cachedContentTokenCount":4,"totalTokenCount":15}}),1).unwrap();
        assert_eq!(turn.tool_calls.len(), 2);
        assert_eq!(turn.tool_calls[0].id, "wire-one");
        assert_eq!(turn.reasoning.as_deref(), Some("reason"));
        let ProviderResponseItem::NativeContent { content, .. } = &turn.response_items[0] else {
            panic!()
        };
        assert_eq!(*content, parts.as_array().unwrap().clone());
        let usage = turn.usage.unwrap();
        assert_eq!(usage.completion_tokens, 5);
        assert_eq!(usage.reasoning_tokens, Some(2));
        assert_eq!(usage.cached_prompt_tokens, Some(4));
    }
    #[test]
    fn google_opaque_only_complete_response_retains_state_but_partial_does_not() {
        let turn=decode(json!({"candidates":[{"content":{"parts":[{"text":"","thoughtSignature":"opaque"}]},"finishReason":"STOP"}]}),7).unwrap();
        assert_eq!(turn.response_items.len(), 1);
        let mut decoder = Decoder::new(&request(), source(), b"{}");
        let events=decoder.push(&frame(json!({"candidates":[{"content":{"parts":[{"text":"partial","thoughtSignature":"opaque"}]}}]}))).unwrap();
        assert!(events
            .iter()
            .all(|event| !matches!(event, ProviderEvent::ResponseItem(_))));
        assert!(matches!(decoder.finish(), Err(ProviderError::PrematureEof)));
    }
    #[test]
    fn google_refusal_truncation_malformed_and_stream_limits_fail_closed() {
        assert!(matches!(
            decode(json!({"promptFeedback":{"blockReason":"SAFETY"}}), 5),
            Err(ProviderError::ResponseRefused)
        ));
        assert!(matches!(
            decode(
                json!({"candidates":[{"content":{"parts":[{"text":"cut"}]},"finishReason":"MAX_TOKENS"}]}),
                3
            ),
            Err(ProviderError::IncompleteFinish(FinishReason::Length))
        ));
        assert!(decode(json!({"candidates":[{"content":{"parts":[{"functionCall":{"name":"bad","args":[]}}]},"finishReason":"STOP"}]}),9).is_err());
        assert!(decode(json!({"candidates":[{"content":{"parts":[{"text":"","thoughtSignature":9}]},"finishReason":"STOP"}]}),9).is_err());
        let mut decoder = Decoder::new(&request(), source(), b"{}");
        assert!(matches!(
            decoder.push(&vec![b'x'; MAX_BYTES + 1]),
            Err(ProviderError::EventTooLarge { .. })
        ));
    }
    #[test]
    fn google_native_roundtrip_keeps_parallel_ids_signatures_and_combines_results() {
        let mut request = request();
        let parts = vec![
            json!({"text":"","thoughtSignature":"empty"}),
            json!({"functionCall":{"name":"read_file","args":{"path":"one"},"id":"wire-one"},"thoughtSignature":"call"}),
            json!({"functionCall":{"name":"read_file","args":{"path":"two"}}}),
        ];
        let mut assistant = message("assistant", "");
        assistant.tool_calls = Some(vec![
            ToolCallMsg {
                id: "local-one".into(),
                call_type: "function".into(),
                function: FunctionCall {
                    name: "read_file".into(),
                    arguments: r#"{"path":"one"}"#.into(),
                },
            },
            ToolCallMsg {
                id: "local-two".into(),
                call_type: "function".into(),
                function: FunctionCall {
                    name: "read_file".into(),
                    arguments: r#"{"path":"two"}"#.into(),
                },
            },
        ]);
        assistant.response_items = vec![ProviderResponseItem::NativeContent {
            source: source(),
            model: request.model.clone(),
            prefix_sha256: "digest".into(),
            content: parts.clone(),
        }];
        let persisted = serde_json::to_vec(&assistant.response_items).unwrap();
        assistant.response_items = serde_json::from_slice(&persisted).unwrap();
        request.messages.push(assistant);
        for id in ["local-one", "local-two"] {
            let mut result = message("tool", "result");
            result.tool_call_id = Some(id.into());
            request.messages.push(result);
        }
        let (bytes, diagnostics, indices) =
            body(&request, &source(), crate::tool_schema::Policy::Compatible).unwrap();
        let sent: Value = serde_json::from_slice(&bytes).unwrap();
        assert!(diagnostics.is_empty());
        assert_eq!(indices, vec![1]);
        assert_eq!(sent["contents"][1]["parts"], json!(parts));
        assert_eq!(sent["contents"][2]["parts"].as_array().unwrap().len(), 2);
        assert_eq!(
            sent["contents"][2]["parts"][0]["functionResponse"]["id"],
            "wire-one"
        );
        assert!(sent["contents"][2]["parts"][1]["functionResponse"]
            .get("id")
            .is_none());
        request.model = "gemini-other".into();
        let (bytes, diagnostics, indices) =
            body(&request, &source(), crate::tool_schema::Policy::Compatible).unwrap();
        assert!(!diagnostics.is_empty());
        assert!(indices.is_empty());
        assert!(!String::from_utf8(bytes)
            .unwrap()
            .contains("thoughtSignature"));
    }
    #[test]
    fn google_images_instructions_schema_loss_and_strict_rejection() {
        let mut request = request();
        let image = crate::image_content::RawClipboardImage::new(1, 1, vec![1, 2, 3, 255])
            .unwrap()
            .normalize()
            .unwrap();
        request.messages[1].content = ModelContent::multimodal("look", &[image]);
        request.tools.push(ToolDef{def_type:"function".into(),function:ToolFunction{name:"example".into(),description:"example".into(),parameters:json!({"type":"object","properties":{"value":{"type":"integer","minimum":1}},"additionalProperties":false})}});
        let (bytes, losses, _) =
            body(&request, &source(), crate::tool_schema::Policy::Compatible).unwrap();
        let sent: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            sent["systemInstruction"]["parts"][0]["text"],
            "instructions"
        );
        assert_eq!(
            sent["contents"][0]["parts"][1]["inlineData"]["mimeType"],
            "image/png"
        );
        assert_eq!(
            sent["tools"][0]["functionDeclarations"][0]["parameters"]["properties"]["value"]
                ["minimum"],
            1
        );
        assert!(!losses.is_empty());
        assert!(body(&request, &source(), crate::tool_schema::Policy::RejectLossy).is_err());
        for bad in ["../escape", "id?key=x", "models/a/b", ""] {
            assert!(model_path(bad).is_err());
        }
        assert_eq!(
            model_path("models/gemini-3-flash").unwrap(),
            "models/gemini-3-flash:streamGenerateContent?alt=sse"
        );
    }
}
