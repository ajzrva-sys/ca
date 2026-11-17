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

/// Host-only immutable bytes prove the exact prepared body at the send boundary.
/// Protocol-prefix and durable registry digests keep their SHA-256 contracts.
#[derive(Clone)]
pub(crate) struct PreparedBodyReceipt {
    body: std::sync::Arc<[u8]>,
    structured_output: Option<crate::headless_contract::StructuredOutputContract>,
    host_instructions: Option<crate::host_instructions::WireReceipt>,
}

impl PreparedBodyReceipt {
    pub(crate) fn new(body: &[u8]) -> Self {
        Self {
            body: std::sync::Arc::from(body),
            structured_output: None,
            host_instructions: None,
        }
    }
    pub(crate) fn matches(&self, body: &[u8]) -> bool {
        self.body.as_ref() == body
    }
    pub(crate) fn bytes(&self) -> std::sync::Arc<[u8]> {
        self.body.clone()
    }
    pub(crate) fn with_structured_output(
        mut self,
        contract: Option<&crate::headless_contract::StructuredOutputContract>,
    ) -> Self {
        self.structured_output = contract.cloned();
        self
    }
    pub(crate) fn structured_output(
        &self,
    ) -> Option<&crate::headless_contract::StructuredOutputContract> {
        self.structured_output.as_ref()
    }
    pub(crate) fn with_host_instructions(
        mut self,
        receipt: Option<crate::host_instructions::WireReceipt>,
    ) -> Self {
        self.host_instructions = receipt;
        self
    }
    pub(crate) fn host_instructions(&self) -> Option<&crate::host_instructions::WireReceipt> {
        self.host_instructions.as_ref()
    }
}

impl std::fmt::Debug for PreparedBodyReceipt {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PreparedBodyReceipt")
            .field("bytes", &self.body.len())
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ReplayBindings {
    pub privacy_body: Option<PreparedBodyReceipt>,
    pub privacy_revision: Option<u64>,
    pub privacy_policy_digest: Option<String>,
    pub privacy_context: Option<std::sync::Arc<crate::privacy::Context>>,
    pub privacy_original: Option<std::sync::Arc<[u8]>>,
    pub responses_items: Vec<usize>,
    pub responses_privacy_prefixes: Vec<(usize, String)>,
    pub google_parts: Vec<usize>,
    pub google_privacy_prefixes: Vec<(usize, String)>,
    pub messages: Vec<(usize, String)>,
    pub request_sequence: u64,
}
#[derive(Clone)]
pub(crate) struct SentPrefix(pub String);

pub(crate) fn responses_prefix(body: &[u8], end: Option<usize>) -> Result<String, ProviderError> {
    let value: Value = serde_json::from_slice(body)
        .map_err(|_| ProviderError::Protocol("Responses prefix is not JSON".into()))?;
    responses_prefix_value(&value, end)
}

fn responses_prefix_value(value: &Value, end: Option<usize>) -> Result<String, ProviderError> {
    let mut prefix = ResponsesPrefix::new(value)?;
    prefix.at(end.unwrap_or(prefix.input.len()))
}

struct ResponsesPrefix<'a> {
    input: &'a [Value],
    hash: Sha256,
    through: usize,
}
impl<'a> ResponsesPrefix<'a> {
    fn new(value: &'a Value) -> Result<Self, ProviderError> {
        let input = value
            .get("input")
            .and_then(Value::as_array)
            .ok_or_else(|| ProviderError::Protocol("Responses prefix has no input array".into()))?;
        let mut hash = Sha256::new();
        hash.update(b"ca-private-responses-prefix-v1\0");
        hash_json(&mut hash, value.get("tools").unwrap_or(&Value::Null))?;
        hash.update([0]);
        hash_json(&mut hash, value.get("instructions").unwrap_or(&Value::Null))?;
        Ok(Self {
            input,
            hash,
            through: 0,
        })
    }
    fn at(&mut self, end: usize) -> Result<String, ProviderError> {
        if end < self.through || end > self.input.len() {
            return Err(ProviderError::Protocol(
                "Responses prefix boundary is invalid".into(),
            ));
        }
        for item in &self.input[self.through..end] {
            self.hash.update([0]);
            hash_json(&mut self.hash, item)?;
        }
        self.through = end;
        let mut hash = self.hash.clone();
        hash.update((end as u64).to_le_bytes());
        Ok(format!("{:x}", hash.finalize()))
    }
}

fn hash_json(hash: &mut Sha256, value: &Value) -> Result<(), ProviderError> {
    struct Writer<'a>(&'a mut Sha256);
    impl std::io::Write for Writer<'_> {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.update(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    serde_json::to_writer(Writer(hash), value)
        .map_err(|_| ProviderError::Protocol("Responses prefix serialization failed".into()))
}

pub(crate) fn google_prefix(body: &[u8], end: Option<usize>) -> Result<String, ProviderError> {
    let value: Value = serde_json::from_slice(body)
        .map_err(|_| ProviderError::Protocol("Google prefix is not JSON".into()))?;
    let mut prefix = GooglePrefix::new(&value)?;
    prefix.at(end.unwrap_or(prefix.contents.len()))
}

// Preserve the existing Google receipt digest (the canonical request excluding
// generationConfig), while hashing long conversation prefixes only once.
struct GooglePrefix<'a> {
    contents: &'a [Value],
    hash: Sha256,
    tail: Vec<u8>,
    through: usize,
}

impl<'a> GooglePrefix<'a> {
    fn new(value: &'a Value) -> Result<Self, ProviderError> {
        let object = value
            .as_object()
            .ok_or_else(|| ProviderError::Protocol("Google prefix is not an object".into()))?;
        let contents = value
            .get("contents")
            .and_then(Value::as_array)
            .ok_or_else(|| ProviderError::Protocol("Google prefix has no contents array".into()))?;
        let mut head = vec![b'{'];
        let mut tail = Vec::new();
        let mut after_contents = false;
        let mut first = true;
        // Preserve Map::remove's exact key order, including swap removal when
        // serde_json's preserve_order feature is enabled. Copy only header
        // keys; cloning values here would duplicate the complete conversation.
        let mut fields = object
            .iter()
            .map(|(key, _)| (key.clone(), Value::Null))
            .collect::<serde_json::Map<_, _>>();
        fields.remove("generationConfig");
        for key in fields.keys() {
            let item = &object[key];
            let output = if after_contents { &mut tail } else { &mut head };
            if !first {
                output.push(b',');
            }
            first = false;
            serde_json::to_writer(&mut *output, key).map_err(|_| {
                ProviderError::Protocol("Google prefix key cannot be encoded".into())
            })?;
            output.push(b':');
            if key == "contents" {
                output.push(b'[');
                tail.push(b']');
                after_contents = true;
            } else {
                serde_json::to_writer(output, item).map_err(|_| {
                    ProviderError::Protocol("Google prefix value cannot be encoded".into())
                })?;
            }
        }
        tail.push(b'}');
        let mut hash = Sha256::new();
        hash.update(head);
        Ok(Self {
            contents,
            hash,
            tail,
            through: 0,
        })
    }

    fn at(&mut self, end: usize) -> Result<String, ProviderError> {
        if end < self.through || end > self.contents.len() {
            return Err(ProviderError::Protocol(
                "Google prefix boundary is invalid".into(),
            ));
        }
        for (index, content) in self.contents[self.through..end].iter().enumerate() {
            if self.through + index > 0 {
                self.hash.update(b",");
            }
            hash_json(&mut self.hash, content)?;
        }
        self.through = end;
        let mut hash = self.hash.clone();
        hash.update(&self.tail);
        Ok(format!("{:x}", hash.finalize()))
    }
}

/// Protect unchanged provider-issued assistant arrays, then check signatures
/// against the actual outgoing prefix. Stop rules see final bytes.
pub(crate) fn prepare_outbound(
    body: &[u8],
    bindings: &ReplayBindings,
    filter: &crate::provider_filter::ProviderFilter,
) -> Result<(Vec<u8>, Vec<String>), ProviderError> {
    prepare_projected(body, bindings, filter, None)
}

/// Project host-owned data while immutable provider arrays are removed, then
/// validate their bindings against the actual outgoing prefix after restoration.
pub(crate) fn prepare_outbound_with_projection(
    body: &[u8],
    bindings: &ReplayBindings,
    filter: &crate::provider_filter::ProviderFilter,
    project: &OutboundProjection<'_>,
) -> Result<(Vec<u8>, Vec<String>), ProviderError> {
    prepare_projected(body, bindings, filter, Some(project))
}
pub(crate) type OutboundProjection<'a> = dyn Fn(&[u8]) -> Result<Vec<u8>, ProviderError> + 'a;
struct OptionalProjection<'a>(Option<&'a OutboundProjection<'a>>);
impl OptionalProjection<'_> {
    fn apply<'a>(&self, body: &'a [u8]) -> Result<std::borrow::Cow<'a, [u8]>, ProviderError> {
        match self.0 {
            Some(project) => project(body).map(std::borrow::Cow::Owned),
            None => Ok(std::borrow::Cow::Borrowed(body)),
        }
    }
}
fn prepare_projected(
    body: &[u8],
    bindings: &ReplayBindings,
    filter: &crate::provider_filter::ProviderFilter,
    projection: Option<&OutboundProjection<'_>>,
) -> Result<(Vec<u8>, Vec<String>), ProviderError> {
    prepare_projected_inner(body, bindings, filter, projection, None)
}

pub(crate) fn prepare_outbound_with_host(
    body: &[u8],
    bindings: &ReplayBindings,
    filter: &crate::provider_filter::ProviderFilter,
    projection: Option<&OutboundProjection<'_>>,
    mut host: crate::host_instructions::WireReceipt,
) -> Result<(Vec<u8>, Vec<String>, crate::host_instructions::WireReceipt), ProviderError> {
    let mut value: Value = serde_json::from_slice(body)
        .map_err(|_| ProviderError::Protocol("Host request is not JSON".into()))?;
    host.protect(&mut value)
        .map_err(|detail| ProviderError::PrivacyBlocked { detail })?;
    let protected = serde_json::to_vec(&value).expect("JSON serializes");
    let (body, diagnostics) =
        prepare_projected_inner(&protected, bindings, filter, projection, Some(&mut host))?;
    Ok((body, diagnostics, host))
}

fn restore_host(
    value: &mut Value,
    host: &mut Option<&mut crate::host_instructions::WireReceipt>,
) -> Result<(), ProviderError> {
    if let Some(host) = host {
        host.restore(value)
            .map_err(|detail| ProviderError::PrivacyBlocked { detail })?;
    }
    Ok(())
}

fn prepare_projected_inner(
    body: &[u8],
    bindings: &ReplayBindings,
    filter: &crate::provider_filter::ProviderFilter,
    projection: Option<&OutboundProjection<'_>>,
    mut host: Option<&mut crate::host_instructions::WireReceipt>,
) -> Result<(Vec<u8>, Vec<String>), ProviderError> {
    let project = OptionalProjection(projection);
    if !bindings.responses_items.is_empty() {
        let mut value: Value = serde_json::from_slice(body)
            .map_err(|_| ProviderError::Protocol("Responses request is not JSON".into()))?;
        let mut originals = Vec::new();
        for index in &bindings.responses_items {
            let item = value
                .pointer_mut(&format!("/input/{index}"))
                .ok_or_else(|| {
                    ProviderError::Protocol("Responses replay binding has no input item".into())
                })?;
            originals.push((*index, std::mem::replace(item, Value::Null)));
        }
        let filtered = filter.prepare_request_without_final_scan(
            &project.apply(&serde_json::to_vec(&value).expect("JSON serializes"))?,
        )?;
        value = serde_json::from_slice(&filtered).map_err(|_| {
            ProviderError::Protocol("filter produced invalid Responses JSON".into())
        })?;
        for (index, original) in originals {
            let item = value
                .pointer_mut(&format!("/input/{index}"))
                .ok_or_else(|| {
                    ProviderError::Protocol("filter removed a Responses replay item".into())
                })?;
            *item = original;
        }
        restore_host(&mut value, &mut host)?;
        let bytes = serde_json::to_vec(&value).expect("JSON serializes");
        filter.scan_prepared_request(&bytes)?;
        let mut required = bindings
            .responses_privacy_prefixes
            .iter()
            .collect::<Vec<_>>();
        required.sort_by_key(|(end, _)| *end);
        if !required.is_empty() {
            let mut prefix = ResponsesPrefix::new(&value)?;
            for (end, expected) in required {
                if prefix.at(*end)? != *expected {
                    return Err(ProviderError::PrivacyBlocked { detail: "Earlier request content changed under project privacy while opaque reasoning depends on its original prefix; use /filter continue".into() });
                }
            }
        }
        return Ok((bytes, Vec::new()));
    }
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
        let filtered = filter.prepare_request(
            &project.apply(&serde_json::to_vec(&value).expect("JSON serializes"))?,
        )?;
        value = serde_json::from_slice(&filtered)
            .map_err(|_| ProviderError::Protocol("filter produced invalid Google JSON".into()))?;
        let mut image_signature_omitted = false;
        for (index, mut parts) in originals {
            if filter.replaces_images() {
                let parts = parts.as_array_mut().ok_or_else(|| {
                    ProviderError::Protocol("Google replay parts must be an array".into())
                })?;
                for part in parts {
                    let signed = part.get("thoughtSignature").is_some()
                        || part.get("thought_signature").is_some();
                    // Drop only the replaced image's own signature. Other
                    // issued parts and authored function arguments stay exact.
                    if crate::provider_filter::placeholder_google_image_part(part) {
                        image_signature_omitted |= signed;
                    }
                }
            }
            value["contents"][index]["parts"] = parts;
        }
        restore_host(&mut value, &mut host)?;
        let mut required = bindings.google_privacy_prefixes.iter().collect::<Vec<_>>();
        required.sort_by_key(|(end, _)| *end);
        if !required.is_empty() {
            let mut prefix = GooglePrefix::new(&value)?;
            for (end, expected) in required {
                if prefix.at(*end)? != *expected {
                    return Err(ProviderError::PrivacyBlocked { detail: "Earlier request content changed under project privacy while opaque Google history depends on its original prefix; use /filter continue".into() });
                }
            }
        }
        let body = serde_json::to_vec(&value).expect("JSON serializes");
        filter.scan_prepared_request(&body)?;
        let diagnostics = if image_signature_omitted {
            vec!["Native image replay signature omitted: outbound image placeholder replaced its source part.".into()]
        } else {
            Vec::new()
        };
        return Ok((body, diagnostics));
    }
    if bindings.messages.is_empty() {
        let body = filter.prepare_request(&project.apply(body)?)?;
        if host.is_none() {
            return Ok((body, Vec::new()));
        }
        let mut value = serde_json::from_slice(&body)
            .map_err(|_| ProviderError::Protocol("Filtered host request is not JSON".into()))?;
        restore_host(&mut value, &mut host)?;
        let body = serde_json::to_vec(&value).expect("JSON serializes");
        filter.scan_prepared_request(&body)?;
        return Ok((body, Vec::new()));
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
    let filtered = filter
        .prepare_request(&project.apply(&serde_json::to_vec(&value).expect("JSON serializes"))?)?;
    value = serde_json::from_slice(&filtered)
        .map_err(|_| ProviderError::Protocol("filter produced invalid native JSON".into()))?;
    for (index, content) in originals {
        value["messages"][index]["content"] = content;
    }
    restore_host(&mut value, &mut host)?;
    let mut valid = true;
    let mut diagnostics = Vec::new();
    for (index, expected) in &bindings.messages {
        let prefix = json!({"system": value.get("system").cloned().unwrap_or(json!([])), "tools":value.get("tools").cloned().unwrap_or(json!([])), "messages": &value["messages"].as_array().ok_or_else(|| ProviderError::Protocol("native history is invalid".into()))?[..*index]});
        if valid && *expected != prefix_digest(&prefix) {
            if bindings.privacy_context.is_some()
                && value["messages"][*index]["content"]
                    .as_array()
                    .is_some_and(|content| {
                        content.iter().any(|block| {
                            matches!(
                                block["type"].as_str(),
                                Some("thinking" | "redacted_thinking")
                            )
                        })
                    })
            {
                return Err(ProviderError::PrivacyBlocked { detail: "Earlier request content changed under project privacy while opaque Anthropic history depends on its original prefix; use /filter continue".into() });
            }
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

    #[test]
    fn privacy_google_prefix_boundaries_match_existing_canonical_receipts() {
        let mut value = json!({
            "before":"header", "contents":[
                {"role":"user","parts":[{"text":"first"}]},
                {"role":"model","parts":[{"text":"second","thoughtSignature":"opaque"}]}
            ],
            "generationConfig":{"thinkingConfig":{"thinkingLevel":"HIGH"}},
            "systemInstruction":{"parts":[{"text":"public instruction"}]},
            "tools":[{"functionDeclarations":[]}]
        });
        let mut prefix = GooglePrefix::new(&value).unwrap();
        let mut expected = value.clone();
        expected.as_object_mut().unwrap().remove("generationConfig");
        for end in 0..=2 {
            expected["contents"] = json!(&value["contents"].as_array().unwrap()[..end]);
            assert_eq!(prefix.at(end).unwrap(), prefix_digest(&expected));
        }
        assert!(prefix.at(1).is_err());
        value["contents"] = json!([]);
        assert!(google_prefix(&serde_json::to_vec(&value).unwrap(), Some(1)).is_err());
    }
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
                                arguments_retrieved_spans: Vec::new(),
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
                messages: vec![(1, prefix_digest(&prefix))],
                ..Default::default()
            },
        )
    }
    #[test]
    fn native_final_wire_latin_keeps_provider_assistant_replay_unchanged() {
        let filter = ProviderFilter::compile(&ProviderFilterConfig {
            enabled: true,
            page_stand_in: crate::provider_filter::PageStandIn::Latin,
            ..Default::default()
        })
        .unwrap();
        let (body, mut bindings) = fixture();
        bindings.messages[0].1 = prefix_digest(
            &json!({"system":[],"tools":[],"messages":[body["messages"][0].clone()]}),
        );
        let (bytes, diagnostics) =
            prepare_outbound(&serde_json::to_vec(&body).unwrap(), &bindings, &filter).unwrap();
        assert!(diagnostics.is_empty());
        assert_eq!(serde_json::from_slice::<Value>(&bytes).unwrap(), body);

        let google = json!({"contents":[
            {"role":"user","parts":[{"text":"read input.txt"}]},
            {"role":"model","parts":[
                {"text":"private thought","thought":true,"thoughtSignature":"opaque signature"},
                {"functionCall":{"name":"read_file","args":{"path":"input.txt"}}}
            ]}
        ]});
        let google_bindings = ReplayBindings {
            google_parts: vec![1],
            ..Default::default()
        };
        let (bytes, diagnostics) = prepare_outbound(
            &serde_json::to_vec(&google).unwrap(),
            &google_bindings,
            &filter,
        )
        .unwrap();
        assert!(diagnostics.is_empty());
        assert_eq!(serde_json::from_slice::<Value>(&bytes).unwrap(), google);
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
