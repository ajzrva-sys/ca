use crate::agent::client::{ChatMessage, OpenAiResponsesRequestOptions, ToolDef};
use crate::agent::stream::{
    FinishReason, ProviderError, ProviderEvent, ProviderResponseItem, ProviderResponseMetadata,
    ProviderStateSourceV1, UsageInfo,
};
use crate::config::{
    PromptCachePolicy, PromptCacheUsageFormat, ReasoningEffort, ReasoningRequestFormat,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

/// The provider wire protocol selected by one configured provider.
///
/// Only protocols with complete request, stream, usage, tool-continuation, and
/// safe-error adapters are accepted. Missing configuration intentionally keeps
/// the historical Chat Completions behavior.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderProtocol {
    #[default]
    ChatCompletions,
    #[serde(rename = "openai_responses")]
    OpenAiResponses,
    AnthropicMessages,
}

impl ProviderProtocol {
    pub const fn label(self) -> &'static str {
        match self {
            Self::ChatCompletions => "chat_completions",
            Self::OpenAiResponses => "openai_responses",
            Self::AnthropicMessages => "anthropic_messages",
        }
    }

    pub(crate) const fn adapter(self) -> ProviderAdapter {
        ProviderAdapter { protocol: self }
    }
}

impl std::fmt::Display for ProviderProtocol {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.label())
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ProviderAdapter {
    protocol: ProviderProtocol,
}

pub(crate) struct AdapterRequest {
    pub path: &'static str,
    pub body: Vec<u8>,
}

#[derive(Clone)]
pub(crate) struct OpenAiPromptCacheRequest {
    policy: PromptCachePolicy,
    key: Option<String>,
    stable_prefix_messages: Option<usize>,
}

impl OpenAiPromptCacheRequest {
    pub(crate) fn new(policy: PromptCachePolicy, key: Option<String>) -> Self {
        Self {
            policy,
            key,
            stable_prefix_messages: None,
        }
    }

    pub(crate) fn with_stable_prefix_messages(mut self, messages: usize) -> Self {
        self.stable_prefix_messages = Some(messages);
        self
    }
}

impl ProviderAdapter {
    pub(crate) fn protocol(self) -> ProviderProtocol {
        self.protocol
    }

    pub(crate) fn prepare(
        self,
        request: &crate::request::PreparedModelRequest,
        source: &ProviderStateSourceV1,
        contract: Option<crate::provider_contract::FirstPartyContract>,
        responses_options: Option<&OpenAiResponsesRequestOptions>,
        prompt_cache: Option<&OpenAiPromptCacheRequest>,
    ) -> Result<AdapterRequest, ProviderError> {
        match self.protocol {
            ProviderProtocol::ChatCompletions => Ok(AdapterRequest {
                path: "chat/completions",
                body: serialize_chat_completions(request, source, contract)?,
            }),
            ProviderProtocol::OpenAiResponses => Ok(AdapterRequest {
                path: "responses",
                body: serialize_openai_responses(
                    request,
                    source,
                    contract,
                    responses_options,
                    prompt_cache,
                )?,
            }),
            ProviderProtocol::AnthropicMessages => Ok(AdapterRequest {
                path: "messages",
                body: serialize_anthropic_messages(request)?,
            }),
        }
    }

    pub(crate) fn apply_auth(
        self,
        request: reqwest::RequestBuilder,
        credential: &str,
    ) -> reqwest::RequestBuilder {
        match self.protocol {
            ProviderProtocol::ChatCompletions | ProviderProtocol::OpenAiResponses
                if !credential.is_empty() =>
            {
                request.bearer_auth(credential)
            }
            ProviderProtocol::ChatCompletions | ProviderProtocol::OpenAiResponses => request,
            ProviderProtocol::AnthropicMessages if !credential.is_empty() => request
                .header("anthropic-version", "2023-06-01")
                .header("x-api-key", credential),
            ProviderProtocol::AnthropicMessages => {
                request.header("anthropic-version", "2023-06-01")
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn decoder(
        self,
        request: &crate::request::PreparedModelRequest,
        source: &ProviderStateSourceV1,
        contract: Option<crate::provider_contract::FirstPartyContract>,
        request_id_sha256: Option<String>,
    ) -> ProviderStreamDecoder {
        self.decoder_with_cache_usage(request, source, contract, request_id_sha256, None)
    }

    pub(crate) fn decoder_with_cache_usage(
        self,
        request: &crate::request::PreparedModelRequest,
        source: &ProviderStateSourceV1,
        contract: Option<crate::provider_contract::FirstPartyContract>,
        request_id_sha256: Option<String>,
        cache_usage_format: Option<PromptCacheUsageFormat>,
    ) -> ProviderStreamDecoder {
        match self.protocol {
            ProviderProtocol::ChatCompletions => ProviderStreamDecoder::ChatCompletions(
                ChatCompletionsDecoder::with_reasoning_provider_and_cache_usage(
                    chat_reasoning_field(request, contract),
                    contract
                        .is_some_and(|contract| contract.replays_chat_reasoning())
                        .then(|| source.clone()),
                    cache_usage_format,
                ),
            ),
            ProviderProtocol::OpenAiResponses => ProviderStreamDecoder::OpenAiResponses(
                OpenAiResponsesDecoder::for_source_with_cache_usage(
                    source.clone(),
                    request_id_sha256,
                    cache_usage_format,
                ),
            ),
            ProviderProtocol::AnthropicMessages => ProviderStreamDecoder::AnthropicMessages(
                AnthropicMessagesDecoder::new_with_cache_usage(cache_usage_format),
            ),
        }
    }
}

fn chat_reasoning_field(
    request: &crate::request::PreparedModelRequest,
    contract: Option<crate::provider_contract::FirstPartyContract>,
) -> Option<String> {
    if contract.is_some_and(|contract| contract.replays_chat_reasoning()) {
        Some("reasoning_content".into())
    } else {
        request.options.reasoning_stream_field.clone()
    }
}

pub(crate) enum ProviderStreamDecoder {
    ChatCompletions(ChatCompletionsDecoder),
    OpenAiResponses(OpenAiResponsesDecoder),
    AnthropicMessages(AnthropicMessagesDecoder),
}

const DEFAULT_SSE_EVENT_LIMIT: usize = 256 * 1024;
const MAX_CHAT_REASONING_BYTES: usize = 256 * 1024;
const MAX_RESPONSE_OUTPUT_ITEMS: usize = 16;
const MAX_RESPONSE_OUTPUT_BYTES: usize = 256 * 1024;
const MAX_RESPONSE_OUTPUT_DEPTH: usize = 16;
const MAX_RESPONSE_OUTPUT_NODES: usize = 2_048;
const MAX_RESPONSE_OUTPUT_ID_BYTES: usize = 256;
const MAX_PROVIDER_REQUEST_ID_BYTES: usize = 256;

pub(crate) fn provider_request_id_sha256(
    headers: &reqwest::header::HeaderMap,
) -> Result<Option<String>, ProviderError> {
    let mut values = headers
        .get_all("x-request-id")
        .iter()
        .chain(headers.get_all("x-oai-request-id").iter());
    let Some(value) = values.next() else {
        return Ok(None);
    };
    if values.next().is_some() {
        return Err(ProviderError::Protocol(
            "provider returned duplicate request ID headers".into(),
        ));
    }
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes.len() > MAX_PROVIDER_REQUEST_ID_BYTES {
        return Err(ProviderError::Protocol(
            "provider request ID is invalid".into(),
        ));
    }
    let mut hasher = Sha256::new();
    hasher.update(b"ca-provider-x-request-id-v1\0");
    hasher.update(bytes);
    Ok(Some(format!("{:x}", hasher.finalize())))
}

pub(crate) struct ChatCompletionsDecoder {
    buffer: Vec<u8>,
    data_lines: Vec<Vec<u8>>,
    event_size: usize,
    event_limit: usize,
    reasoning_field: Option<String>,
    reasoning_source: Option<ProviderStateSourceV1>,
    reasoning_content: String,
    tool_call_finished: bool,
    reasoning_item_emitted: bool,
    cache_usage_format: Option<PromptCacheUsageFormat>,
}

impl ChatCompletionsDecoder {
    #[cfg(test)]
    pub(crate) fn with_reasoning_field(reasoning_field: Option<String>) -> Self {
        Self::with_reasoning_provider(reasoning_field, None)
    }

    #[cfg(test)]
    fn with_reasoning_provider(
        reasoning_field: Option<String>,
        reasoning_source: Option<ProviderStateSourceV1>,
    ) -> Self {
        Self::with_reasoning_provider_and_cache_usage(reasoning_field, reasoning_source, None)
    }

    fn with_reasoning_provider_and_cache_usage(
        reasoning_field: Option<String>,
        reasoning_source: Option<ProviderStateSourceV1>,
        cache_usage_format: Option<PromptCacheUsageFormat>,
    ) -> Self {
        Self {
            buffer: Vec::new(),
            data_lines: Vec::new(),
            event_size: 0,
            event_limit: DEFAULT_SSE_EVENT_LIMIT,
            reasoning_field,
            reasoning_source,
            reasoning_content: String::new(),
            tool_call_finished: false,
            reasoning_item_emitted: false,
            cache_usage_format,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_event_limit(event_limit: usize) -> Self {
        Self {
            event_limit,
            ..Self::with_reasoning_field(None)
        }
    }

    pub(crate) fn push(&mut self, chunk: &[u8]) -> Result<Vec<ProviderEvent>, ProviderError> {
        self.buffer.extend_from_slice(chunk);
        self.enforce_limit()?;
        let mut events = Vec::new();
        while let Some(newline) = self.buffer.iter().position(|byte| *byte == b'\n') {
            let mut line: Vec<u8> = self.buffer.drain(..=newline).collect();
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            if let Some(event) = self.process_line(&line)? {
                events.extend(event);
            }
        }
        self.enforce_limit()?;
        Ok(events)
    }

    pub(crate) fn finish(&mut self) -> Result<Vec<ProviderEvent>, ProviderError> {
        let mut events = Vec::new();
        if !self.buffer.is_empty() {
            let mut line = std::mem::take(&mut self.buffer);
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            if let Some(event) = self.process_line(&line)? {
                events.extend(event);
            }
        }
        if let Some(event) = self.dispatch()? {
            events.extend(event);
        }
        Ok(events)
    }

    fn enforce_limit(&self) -> Result<(), ProviderError> {
        if self.event_size.saturating_add(self.buffer.len()) > self.event_limit {
            Err(ProviderError::EventTooLarge {
                limit: self.event_limit,
            })
        } else {
            Ok(())
        }
    }

    fn append_reasoning_content(&mut self, reasoning: &str) -> Result<(), ProviderError> {
        if self.reasoning_source.is_none() {
            return Ok(());
        }
        if self.reasoning_content.len().saturating_add(reasoning.len()) > MAX_CHAT_REASONING_BYTES {
            return Err(ProviderError::EventTooLarge {
                limit: MAX_CHAT_REASONING_BYTES,
            });
        }
        self.reasoning_content.push_str(reasoning);
        Ok(())
    }

    fn process_line(&mut self, line: &[u8]) -> Result<Option<Vec<ProviderEvent>>, ProviderError> {
        if line.is_empty() {
            return self.dispatch();
        }
        if line.first() == Some(&b':') {
            return Ok(None);
        }
        let colon = line.iter().position(|byte| *byte == b':');
        let (field, mut value) = match colon {
            Some(index) => (&line[..index], &line[index + 1..]),
            None => (line, &[][..]),
        };
        if field != b"data" {
            return Ok(None);
        }
        if value.first() == Some(&b' ') {
            value = &value[1..];
        }
        let separator = usize::from(!self.data_lines.is_empty());
        self.event_size = self.event_size.saturating_add(separator + value.len());
        if self.event_size > self.event_limit {
            return Err(ProviderError::EventTooLarge {
                limit: self.event_limit,
            });
        }
        self.data_lines.push(value.to_vec());
        Ok(None)
    }

    fn dispatch(&mut self) -> Result<Option<Vec<ProviderEvent>>, ProviderError> {
        if self.data_lines.is_empty() {
            self.event_size = 0;
            return Ok(None);
        }
        let lines = std::mem::take(&mut self.data_lines);
        self.event_size = 0;
        let mut data = Vec::new();
        for (index, line) in lines.into_iter().enumerate() {
            if index > 0 {
                data.push(b'\n');
            }
            data.extend(line);
        }
        let data = String::from_utf8(data).map_err(|_| ProviderError::InvalidUtf8)?;
        if data.trim() == "[DONE]" {
            let mut events = Vec::new();
            if self.tool_call_finished && !self.reasoning_item_emitted {
                if let Some(source) = self.reasoning_source.clone() {
                    if !self.reasoning_content.is_empty() {
                        events.push(ProviderEvent::ResponseItem(
                            ProviderResponseItem::ChatReasoning {
                                source,
                                content: self.reasoning_content.clone(),
                            },
                        ));
                    }
                }
                self.reasoning_item_emitted = true;
            }
            events.push(ProviderEvent::Done);
            return Ok(Some(events));
        }
        let value: serde_json::Value = serde_json::from_str(&data)
            .map_err(|error| ProviderError::MalformedData(error.to_string()))?;
        let events = chat_completions_events(
            &value,
            self.reasoning_field.as_deref(),
            self.cache_usage_format,
        )?;
        for event in &events {
            match event {
                ProviderEvent::Reasoning(reasoning) => self.append_reasoning_content(reasoning)?,
                ProviderEvent::Finish {
                    reason: FinishReason::ToolCalls,
                    ..
                } => self.tool_call_finished = true,
                _ => {}
            }
        }
        Ok(Some(events))
    }
}

fn usage_from_chat_completions(
    value: &serde_json::Value,
    format: Option<PromptCacheUsageFormat>,
) -> Result<Option<UsageInfo>, ProviderError> {
    let Some(usage) = value.get("usage") else {
        return Ok(None);
    };
    if usage.is_null() {
        return Ok(None);
    }
    let prompt_tokens = required_usage_token(usage, "prompt_tokens")?;
    let (cached_prompt_tokens, cache_write_tokens, cache_usage) =
        cache_usage_from_chat_shape(usage, prompt_tokens, format)?;
    let base = UsageInfo {
        prompt_tokens,
        completion_tokens: required_usage_token(usage, "completion_tokens")?,
        total_tokens: required_usage_token(usage, "total_tokens")?,
        cached_prompt_tokens,
        cache_write_tokens,
        reasoning_tokens: optional_usage_pointer(
            usage,
            "/completion_tokens_details/reasoning_tokens",
        )?,
        response_metadata: None,
        cache_usage_override: None,
    };
    base.with_cache_usage(cache_usage).map(Some).ok_or_else(|| {
        ProviderError::MalformedData("provider cache usage exceeds prompt tokens".into())
    })
}

fn cache_usage_from_chat_shape(
    usage: &serde_json::Value,
    prompt_tokens: u32,
    format: Option<PromptCacheUsageFormat>,
) -> Result<(Option<u32>, Option<u32>, crate::agent::stream::CacheUsage), ProviderError> {
    use crate::agent::stream::{CacheCount, CacheUsage};

    let openai_present = usage
        .pointer("/prompt_tokens_details/cached_tokens")
        .is_some()
        || usage
            .pointer("/prompt_tokens_details/cache_write_tokens")
            .is_some();
    let hit_miss_present = usage.get("prompt_cache_hit_tokens").is_some()
        || usage.get("prompt_cache_miss_tokens").is_some();
    let unknown = || (None, None, CacheUsage::UNKNOWN);

    let openai = || -> Result<_, ProviderError> {
        let read = optional_usage_pointer(usage, "/prompt_tokens_details/cached_tokens")?;
        let write = optional_usage_pointer(usage, "/prompt_tokens_details/cache_write_tokens")?;
        let miss = read
            .and_then(|read| prompt_tokens.checked_sub(read))
            .map_or(CacheCount::Unknown, CacheCount::Derived);
        Ok((
            read,
            write,
            CacheUsage::new(
                read.map_or(CacheCount::Unknown, CacheCount::Reported),
                write.map_or(CacheCount::Unknown, CacheCount::Reported),
                miss,
            ),
        ))
    };
    let hit_miss = || -> Result<_, ProviderError> {
        let hit = optional_usage_pointer(usage, "/prompt_cache_hit_tokens")?;
        let miss = optional_usage_pointer(usage, "/prompt_cache_miss_tokens")?;
        let (Some(hit), Some(miss)) = (hit, miss) else {
            return Err(ProviderError::MalformedData(
                "cache hit/miss usage subsets are incomplete".into(),
            ));
        };
        if hit.checked_add(miss) != Some(prompt_tokens) {
            return Err(ProviderError::MalformedData(
                "cache hit/miss usage does not equal prompt tokens".into(),
            ));
        }
        Ok((
            Some(hit),
            None,
            CacheUsage::new(
                CacheCount::Reported(hit),
                CacheCount::Unknown,
                CacheCount::Reported(miss),
            ),
        ))
    };

    match format {
        Some(PromptCacheUsageFormat::Auto) => {
            let recognized = match (openai_present, hit_miss_present) {
                (true, false) => openai(),
                (false, true) => hit_miss(),
                (false, false) => return Ok(unknown()),
                (true, true) => return Ok(unknown()),
            };
            Ok(recognized.unwrap_or_else(|_| unknown()))
        }
        Some(PromptCacheUsageFormat::OpenAiDetails) => {
            if hit_miss_present {
                return Err(ProviderError::MalformedData(
                    "unexpected hit/miss cache usage for openai_details contract".into(),
                ));
            }
            if openai_present {
                openai()
            } else {
                Ok(unknown())
            }
        }
        // A declared hit/miss contract owns cache accounting. Compatible
        // providers may also emit OpenAI-shaped prompt details, but those
        // supplemental fields cannot override or invalidate the authoritative
        // complete hit/miss pair.
        Some(PromptCacheUsageFormat::HitMiss) => hit_miss(),
        Some(PromptCacheUsageFormat::ReadWrite) => Err(ProviderError::MalformedData(
            "read_write cache usage is unavailable in chat_completions".into(),
        )),
        None => match (openai_present, hit_miss_present) {
            (false, false) => Ok(unknown()),
            (true, false) => openai(),
            (false, true) => hit_miss(),
            (true, true) => {
                let openai = openai()?;
                let hit_miss = hit_miss()?;
                if openai.0 == hit_miss.0 && openai.1.is_none() {
                    Ok(hit_miss)
                } else {
                    Err(ProviderError::MalformedData(
                        "provider cache usage fields are inconsistent".into(),
                    ))
                }
            }
        },
    }
}

fn chat_completions_events(
    value: &serde_json::Value,
    reasoning_field: Option<&str>,
    cache_usage_format: Option<PromptCacheUsageFormat>,
) -> Result<Vec<ProviderEvent>, ProviderError> {
    let mut events = Vec::new();
    let usage = usage_from_chat_completions(value, cache_usage_format)?;
    let choices = value
        .get("choices")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| ProviderError::MalformedData("missing choices array".into()))?;
    if choices.is_empty() {
        if let Some(usage) = usage {
            events.push(ProviderEvent::Usage(usage));
        }
        return Ok(events);
    }
    for choice in choices {
        if let Some(delta) = choice.get("delta") {
            if let Some(field) = reasoning_field {
                if let Some(reasoning) = delta.get(field).and_then(serde_json::Value::as_str) {
                    if !reasoning.is_empty() {
                        events.push(ProviderEvent::Reasoning(reasoning.to_string()));
                    }
                }
            }
            if let Some(content) = delta.get("content").and_then(serde_json::Value::as_str) {
                if !content.is_empty() {
                    events.push(ProviderEvent::Text(content.to_string()));
                }
            }
            if let Some(tool_calls) = delta
                .get("tool_calls")
                .and_then(serde_json::Value::as_array)
            {
                for call in tool_calls {
                    let index = call
                        .get("index")
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or(0) as usize;
                    if let Some(id) = call.get("id").and_then(serde_json::Value::as_str) {
                        let name = call
                            .pointer("/function/name")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("");
                        events.push(ProviderEvent::ToolCallStart {
                            index,
                            id: id.to_string(),
                            name: name.to_string(),
                        });
                    }
                    if let Some(arguments) = call
                        .pointer("/function/arguments")
                        .and_then(serde_json::Value::as_str)
                    {
                        if !arguments.is_empty() {
                            events.push(ProviderEvent::ToolCallArg {
                                index,
                                arg: arguments.to_string(),
                            });
                        }
                    }
                }
            }
        }
        if let Some(reason) = choice
            .get("finish_reason")
            .and_then(serde_json::Value::as_str)
        {
            events.push(ProviderEvent::Finish {
                reason: FinishReason::parse(reason),
                usage: usage.clone(),
            });
        }
    }
    Ok(events)
}

#[derive(Debug, Default)]
struct ResponsesCallState {
    id: Option<String>,
    name: Option<String>,
    arguments: String,
    added_arguments: Option<String>,
    started: bool,
}

#[derive(Debug, Default)]
struct ResponsesMetadataState {
    response_id: Option<String>,
    model: Option<String>,
    service_tier: Option<String>,
}

impl ResponsesMetadataState {
    fn observe(&mut self, response: &serde_json::Value) -> Result<(), ProviderError> {
        let observed = response_metadata_fields(response)?;
        reconcile_metadata_field(&mut self.response_id, observed.response_id, "response id")?;
        reconcile_metadata_field(&mut self.model, observed.model, "model")?;
        reconcile_metadata_field(
            &mut self.service_tier,
            observed.service_tier,
            "service tier",
        )
    }

    fn terminal(
        &mut self,
        response: &serde_json::Value,
        request_id_sha256: Option<String>,
    ) -> Result<Option<ProviderResponseMetadata>, ProviderError> {
        let terminal = response_metadata_fields(response)?;
        if self
            .response_id
            .as_ref()
            .zip(terminal.response_id.as_ref())
            .is_some_and(|(earlier, terminal)| earlier != terminal)
            || self
                .model
                .as_ref()
                .zip(terminal.model.as_ref())
                .is_some_and(|(earlier, terminal)| earlier != terminal)
            || self
                .service_tier
                .as_ref()
                .zip(terminal.service_tier.as_ref())
                .is_some_and(|(earlier, terminal)| earlier != terminal && earlier != "auto")
        {
            return Err(ProviderError::Protocol(
                "conflicting provider response metadata".into(),
            ));
        }
        match (terminal.response_id, terminal.model, request_id_sha256) {
            (None, None, None) if terminal.service_tier.is_none() => Ok(None),
            (Some(_), Some(model), Some(request_id_sha256)) => Ok(Some(
                ProviderResponseMetadata::new(request_id_sha256, model, terminal.service_tier)?,
            )),
            _ => Err(ProviderError::Protocol(
                "terminal provider response metadata is incomplete".into(),
            )),
        }
    }
}

fn reconcile_metadata_field(
    retained: &mut Option<String>,
    observed: Option<String>,
    field: &str,
) -> Result<(), ProviderError> {
    if let Some(observed) = observed {
        if retained
            .as_ref()
            .is_some_and(|retained| retained != &observed)
        {
            return Err(ProviderError::Protocol(format!(
                "conflicting provider response {field}"
            )));
        }
        *retained = Some(observed);
    }
    Ok(())
}

fn response_metadata_fields(
    response: &serde_json::Value,
) -> Result<ResponsesMetadataState, ProviderError> {
    let response_id = response
        .get("id")
        .map(|value| bounded_metadata_string(value, "id", 256))
        .transpose()?;
    let model = response
        .get("model")
        .map(|value| bounded_metadata_string(value, "model", 256))
        .transpose()?;
    let service_tier = response
        .get("service_tier")
        .filter(|value| !value.is_null())
        .map(|value| bounded_metadata_string(value, "service_tier", 32))
        .transpose()?;
    Ok(ResponsesMetadataState {
        response_id,
        model,
        service_tier,
    })
}

fn bounded_metadata_string(
    value: &serde_json::Value,
    field: &str,
    maximum: usize,
) -> Result<String, ProviderError> {
    let value = value.as_str().ok_or_else(|| {
        ProviderError::MalformedData(format!("provider response {field} must be text"))
    })?;
    if value.is_empty()
        || value.len() > maximum
        || !value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b':' | b'/')
        })
    {
        return Err(ProviderError::Protocol(format!(
            "provider response {field} is invalid"
        )));
    }
    Ok(value.to_string())
}

pub(crate) struct OpenAiResponsesDecoder {
    buffer: Vec<u8>,
    data_lines: Vec<Vec<u8>>,
    event_size: usize,
    event_limit: usize,
    calls: BTreeMap<usize, ResponsesCallState>,
    text: BTreeMap<(usize, usize), String>,
    reasoning: BTreeMap<(String, usize, usize), String>,
    source: ProviderStateSourceV1,
    output_items: BTreeMap<usize, serde_json::Value>,
    output_item_bytes: usize,
    metadata: ResponsesMetadataState,
    request_id_sha256: Option<String>,
    cache_usage_format: Option<PromptCacheUsageFormat>,
    completed: bool,
    saw_function_call: bool,
}

impl OpenAiResponsesDecoder {
    #[cfg(test)]
    pub(crate) fn new() -> Self {
        Self::for_source(
            ProviderStateSourceV1::for_test("unknown", ProviderProtocol::OpenAiResponses),
            None,
        )
    }

    #[cfg(test)]
    fn with_request_id_for_test(request_id: &str) -> Self {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            "x-request-id",
            reqwest::header::HeaderValue::try_from(request_id).unwrap(),
        );
        Self::for_source(
            ProviderStateSourceV1::for_test("unknown", ProviderProtocol::OpenAiResponses),
            provider_request_id_sha256(&headers).unwrap(),
        )
    }

    #[cfg(test)]
    fn for_source(source: ProviderStateSourceV1, request_id_sha256: Option<String>) -> Self {
        Self::for_source_with_cache_usage(source, request_id_sha256, None)
    }

    fn for_source_with_cache_usage(
        source: ProviderStateSourceV1,
        request_id_sha256: Option<String>,
        cache_usage_format: Option<PromptCacheUsageFormat>,
    ) -> Self {
        Self {
            buffer: Vec::new(),
            data_lines: Vec::new(),
            event_size: 0,
            event_limit: DEFAULT_SSE_EVENT_LIMIT,
            calls: BTreeMap::new(),
            text: BTreeMap::new(),
            reasoning: BTreeMap::new(),
            source,
            output_items: BTreeMap::new(),
            output_item_bytes: 0,
            metadata: ResponsesMetadataState::default(),
            request_id_sha256,
            cache_usage_format,
            completed: false,
            saw_function_call: false,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_event_limit(event_limit: usize) -> Self {
        Self {
            event_limit,
            ..Self::new()
        }
    }

    pub(crate) fn push(&mut self, chunk: &[u8]) -> Result<Vec<ProviderEvent>, ProviderError> {
        self.buffer.extend_from_slice(chunk);
        self.enforce_limit()?;
        let mut events = Vec::new();
        while let Some(newline) = self.buffer.iter().position(|byte| *byte == b'\n') {
            let mut line: Vec<u8> = self.buffer.drain(..=newline).collect();
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            if let Some(event) = self.process_line(&line)? {
                events.extend(event);
            }
        }
        self.enforce_limit()?;
        Ok(events)
    }

    pub(crate) fn finish(&mut self) -> Result<Vec<ProviderEvent>, ProviderError> {
        let mut events = Vec::new();
        if !self.buffer.is_empty() {
            let mut line = std::mem::take(&mut self.buffer);
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            if let Some(event) = self.process_line(&line)? {
                events.extend(event);
            }
        }
        if let Some(event) = self.dispatch()? {
            events.extend(event);
        }
        Ok(events)
    }

    fn enforce_limit(&self) -> Result<(), ProviderError> {
        if self.event_size.saturating_add(self.buffer.len()) > self.event_limit {
            Err(ProviderError::EventTooLarge {
                limit: self.event_limit,
            })
        } else {
            Ok(())
        }
    }

    fn process_line(&mut self, line: &[u8]) -> Result<Option<Vec<ProviderEvent>>, ProviderError> {
        if line.is_empty() {
            return self.dispatch();
        }
        if line.first() == Some(&b':') {
            return Ok(None);
        }
        let colon = line.iter().position(|byte| *byte == b':');
        let (field, mut value) = match colon {
            Some(index) => (&line[..index], &line[index + 1..]),
            None => (line, &[][..]),
        };
        if field != b"data" {
            return Ok(None);
        }
        if value.first() == Some(&b' ') {
            value = &value[1..];
        }
        let separator = usize::from(!self.data_lines.is_empty());
        self.event_size = self.event_size.saturating_add(separator + value.len());
        if self.event_size > self.event_limit {
            return Err(ProviderError::EventTooLarge {
                limit: self.event_limit,
            });
        }
        self.data_lines.push(value.to_vec());
        Ok(None)
    }

    fn dispatch(&mut self) -> Result<Option<Vec<ProviderEvent>>, ProviderError> {
        if self.data_lines.is_empty() {
            self.event_size = 0;
            return Ok(None);
        }
        let lines = std::mem::take(&mut self.data_lines);
        self.event_size = 0;
        let mut data = Vec::new();
        for (index, line) in lines.into_iter().enumerate() {
            if index > 0 {
                data.push(b'\n');
            }
            data.extend(line);
        }
        let data = String::from_utf8(data).map_err(|_| ProviderError::InvalidUtf8)?;
        let value: serde_json::Value = serde_json::from_str(&data)
            .map_err(|error| ProviderError::MalformedData(error.to_string()))?;
        Ok(Some(self.translate(&value)?))
    }

    fn translate(
        &mut self,
        value: &serde_json::Value,
    ) -> Result<Vec<ProviderEvent>, ProviderError> {
        if self.completed {
            return Err(ProviderError::Protocol(
                "event received after response.completed".into(),
            ));
        }
        let event_type = required_string(value, "type")?;
        let mut events = Vec::new();
        match event_type {
            "response.created" | "response.queued" | "response.in_progress" => {
                let response = value.get("response").ok_or_else(|| {
                    ProviderError::MalformedData("missing response lifecycle metadata".into())
                })?;
                self.metadata.observe(response)?;
            }
            "response.output_item.added" => {
                let item = value.get("item").ok_or_else(|| {
                    ProviderError::MalformedData("missing response output item".into())
                })?;
                match required_string(item, "type")? {
                    "function_call" => {
                        let index = required_index(value, "output_index")?;
                        let id = required_string(item, "call_id")?;
                        let name = required_string(item, "name")?;
                        self.start_call(index, id, name, &mut events)?;
                        if let Some(arguments) = item
                            .get("arguments")
                            .and_then(serde_json::Value::as_str)
                            .filter(|value| !value.is_empty())
                        {
                            self.calls.entry(index).or_default().added_arguments =
                                Some(arguments.to_string());
                        }
                    }
                    "message" | "reasoning" => {}
                    _ => {
                        return Err(ProviderError::Protocol(
                            "unsupported response output item".into(),
                        ))
                    }
                }
            }
            "response.output_item.done" => {
                let item = value.get("item").ok_or_else(|| {
                    ProviderError::MalformedData("missing completed response output item".into())
                })?;
                let index = required_index(value, "output_index")?;
                self.reconcile_completed_output_item(index, item, &mut events)?;
            }
            "response.output_text.delta" => {
                let output_index = required_index(value, "output_index")?;
                let content_index = required_index(value, "content_index")?;
                let delta = required_string(value, "delta")?;
                if !delta.is_empty() {
                    self.text
                        .entry((output_index, content_index))
                        .or_default()
                        .push_str(delta);
                    events.push(ProviderEvent::Text(delta.to_string()));
                }
            }
            "response.output_text.done" => {
                let key = (
                    required_index(value, "output_index")?,
                    required_index(value, "content_index")?,
                );
                let full = required_string(value, "text")?;
                reconcile_text(&mut self.text, key, full, ProviderEvent::Text, &mut events)?;
            }
            "response.reasoning_text.delta" | "response.reasoning_summary_text.delta" => {
                let output_index = required_index(value, "output_index")?;
                let part_index = value
                    .get("content_index")
                    .or_else(|| value.get("summary_index"))
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0) as usize;
                let delta = required_string(value, "delta")?;
                if !delta.is_empty() {
                    self.reasoning
                        .entry((event_type.to_string(), output_index, part_index))
                        .or_default()
                        .push_str(delta);
                    events.push(ProviderEvent::Reasoning(delta.to_string()));
                }
            }
            "response.reasoning_text.done" | "response.reasoning_summary_text.done" => {
                let output_index = required_index(value, "output_index")?;
                let part_index = value
                    .get("content_index")
                    .or_else(|| value.get("summary_index"))
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0) as usize;
                let delta_type = event_type.replace(".done", ".delta");
                let full = required_string(value, "text")?;
                reconcile_text(
                    &mut self.reasoning,
                    (delta_type, output_index, part_index),
                    full,
                    ProviderEvent::Reasoning,
                    &mut events,
                )?;
            }
            "response.function_call_arguments.delta" => {
                let index = required_index(value, "output_index")?;
                let delta = required_string(value, "delta")?;
                if !delta.is_empty() {
                    self.calls
                        .entry(index)
                        .or_default()
                        .arguments
                        .push_str(delta);
                    events.push(ProviderEvent::ToolCallArg {
                        index,
                        arg: delta.to_string(),
                    });
                }
            }
            "response.function_call_arguments.done" => {
                let index = required_index(value, "output_index")?;
                let arguments = required_string(value, "arguments")?;
                self.reconcile_call_arguments(index, arguments, &mut events)?;
            }
            "response.content_part.added"
            | "response.content_part.done"
            | "response.reasoning_summary_part.added"
            | "response.reasoning_summary_part.done" => {}
            "response.completed" => {
                let response = value.get("response").ok_or_else(|| {
                    ProviderError::MalformedData("missing completed response".into())
                })?;
                let status = response.get("status").and_then(serde_json::Value::as_str);
                if status != Some("completed") {
                    return Err(if status == Some("incomplete") {
                        responses_incomplete_error(response)
                    } else {
                        ProviderError::ResponseIncomplete
                    });
                }
                let response_metadata = self
                    .metadata
                    .terminal(response, self.request_id_sha256.clone())?;
                let usage = responses_usage(response, response_metadata, self.cache_usage_format)?;
                if let Some(output) = response.get("output") {
                    let output = output.as_array().ok_or_else(|| {
                        ProviderError::MalformedData(
                            "completed response output must be an array".into(),
                        )
                    })?;
                    for (index, item) in output.iter().enumerate() {
                        self.reconcile_completed_output_item(index, item, &mut events)?;
                    }
                }
                events.extend(self.output_items.iter().map(|(output_index, item)| {
                    ProviderEvent::ResponseItem(ProviderResponseItem::ResponsesOutput {
                        source: self.source.clone(),
                        output_index: *output_index,
                        item: item.clone(),
                    })
                }));
                self.completed = true;
                events.push(ProviderEvent::Finish {
                    reason: if self.saw_function_call {
                        FinishReason::ToolCalls
                    } else {
                        FinishReason::Stop
                    },
                    usage,
                });
                events.push(ProviderEvent::Done);
            }
            "response.failed" | "error" => return Err(ProviderError::ResponseFailed),
            "response.incomplete" => {
                return Err(value
                    .get("response")
                    .map(responses_incomplete_error)
                    .unwrap_or(ProviderError::ResponseIncomplete));
            }
            "response.refusal.delta" | "response.refusal.done" => {
                return Err(ProviderError::ResponseRefused);
            }
            _ => {
                return Err(ProviderError::Protocol(
                    "unsupported response stream event".into(),
                ))
            }
        }
        Ok(events)
    }

    fn reconcile_completed_output_item(
        &mut self,
        output_index: usize,
        item: &serde_json::Value,
        events: &mut Vec<ProviderEvent>,
    ) -> Result<(), ProviderError> {
        match validate_response_output_item(item)? {
            ResponsesOutputKind::Reasoning => {}
            ResponsesOutputKind::Message => {
                for (content_index, text) in
                    response_message_text_parts(item)?.into_iter().enumerate()
                {
                    reconcile_text(
                        &mut self.text,
                        (output_index, content_index),
                        text,
                        ProviderEvent::Text,
                        events,
                    )?;
                }
            }
            ResponsesOutputKind::FunctionCall => {
                let call_id = required_string(item, "call_id")?;
                let name = required_string(item, "name")?;
                self.start_call(output_index, call_id, name, events)?;
                self.reconcile_call_arguments(
                    output_index,
                    required_string(item, "arguments")?,
                    events,
                )?;
            }
        }
        self.record_response_output_item(output_index, item)
    }

    fn record_response_output_item(
        &mut self,
        output_index: usize,
        item: &serde_json::Value,
    ) -> Result<(), ProviderError> {
        validate_response_output_item(item)?;
        match self.output_items.get(&output_index) {
            Some(existing) if existing == item => Ok(()),
            Some(_) => Err(ProviderError::Protocol(
                "conflicting completed response output item".into(),
            )),
            None => {
                if self.output_items.len() >= MAX_RESPONSE_OUTPUT_ITEMS {
                    return Err(ProviderError::Protocol(
                        "too many response output items".into(),
                    ));
                }
                let item_bytes = response_output_item_bytes(item)?;
                let byte_limit = self.event_limit.min(MAX_RESPONSE_OUTPUT_BYTES);
                if self.output_item_bytes.saturating_add(item_bytes) > byte_limit {
                    return Err(ProviderError::EventTooLarge { limit: byte_limit });
                }
                let id = required_string(item, "id")?;
                if self.output_items.values().any(|existing| {
                    existing.get("id").and_then(serde_json::Value::as_str) == Some(id)
                }) {
                    return Err(ProviderError::Protocol(
                        "duplicate response output item id".into(),
                    ));
                }
                self.output_item_bytes += item_bytes;
                self.output_items.insert(output_index, item.clone());
                Ok(())
            }
        }
    }

    fn start_call(
        &mut self,
        index: usize,
        id: &str,
        name: &str,
        events: &mut Vec<ProviderEvent>,
    ) -> Result<(), ProviderError> {
        let call = self.calls.entry(index).or_default();
        if call.id.as_deref().is_some_and(|existing| existing != id)
            || call
                .name
                .as_deref()
                .is_some_and(|existing| existing != name)
        {
            return Err(ProviderError::Protocol(
                "conflicting response function-call identity".into(),
            ));
        }
        call.id = Some(id.to_string());
        call.name = Some(name.to_string());
        if !call.started {
            call.started = true;
            self.saw_function_call = true;
            events.push(ProviderEvent::ToolCallStart {
                index,
                id: id.to_string(),
                name: name.to_string(),
            });
        }
        Ok(())
    }

    fn reconcile_call_arguments(
        &mut self,
        index: usize,
        full: &str,
        events: &mut Vec<ProviderEvent>,
    ) -> Result<(), ProviderError> {
        let call = self.calls.get_mut(&index).ok_or_else(|| {
            ProviderError::Protocol("function arguments completed before function item".into())
        })?;
        if !call.started {
            return Err(ProviderError::Protocol(
                "function arguments completed before function identity".into(),
            ));
        }
        if let Some(added) = call.added_arguments.as_deref() {
            if added != full {
                return Err(ProviderError::Protocol(
                    "conflicting response function-call arguments".into(),
                ));
            }
        }
        let suffix = full.strip_prefix(&call.arguments).ok_or_else(|| {
            ProviderError::Protocol("conflicting response function-call arguments".into())
        })?;
        if !suffix.is_empty() {
            events.push(ProviderEvent::ToolCallArg {
                index,
                arg: suffix.to_string(),
            });
            call.arguments.push_str(suffix);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResponsesOutputKind {
    Reasoning,
    Message,
    FunctionCall,
}

fn responses_incomplete_error(response: &serde_json::Value) -> ProviderError {
    match response
        .pointer("/incomplete_details/reason")
        .and_then(serde_json::Value::as_str)
    {
        Some("max_output_tokens") => ProviderError::OutputTokenLimit,
        _ => ProviderError::ResponseIncomplete,
    }
}

fn validate_response_output_item(
    item: &serde_json::Value,
) -> Result<ResponsesOutputKind, ProviderError> {
    let object = item.as_object().ok_or_else(|| {
        ProviderError::MalformedData("response output item must be an object".into())
    })?;
    let id = object
        .get("id")
        .and_then(serde_json::Value::as_str)
        .filter(|id| !id.is_empty())
        .ok_or_else(|| ProviderError::MalformedData("response output item is missing id".into()))?;
    if id.len() > MAX_RESPONSE_OUTPUT_ID_BYTES {
        return Err(ProviderError::MalformedData(
            "response output item id is too long".into(),
        ));
    }
    if object
        .get("status")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|status| status != "completed")
    {
        return Err(ProviderError::MalformedData(
            "response output item is not completed".into(),
        ));
    }
    let kind = match required_string(item, "type")? {
        "reasoning" => {
            validate_response_reasoning_parts(object, "summary", "summary_text")?;
            validate_response_reasoning_parts(object, "content", "reasoning_text")?;
            if object
                .get("encrypted_content")
                .is_some_and(|content| !content.is_string() && !content.is_null())
            {
                return Err(ProviderError::MalformedData(
                    "response reasoning encrypted_content must be text or null".into(),
                ));
            }
            ResponsesOutputKind::Reasoning
        }
        "message" => {
            if object.get("role").and_then(serde_json::Value::as_str) != Some("assistant") {
                return Err(ProviderError::MalformedData(
                    "response message item must have assistant role".into(),
                ));
            }
            let content = object
                .get("content")
                .and_then(serde_json::Value::as_array)
                .ok_or_else(|| {
                    ProviderError::MalformedData(
                        "response message item content must be an array".into(),
                    )
                })?;
            for part in content {
                let part = part.as_object().ok_or_else(|| {
                    ProviderError::MalformedData(
                        "response message content must be an object".into(),
                    )
                })?;
                if part.get("type").and_then(serde_json::Value::as_str) != Some("output_text")
                    || part
                        .get("text")
                        .and_then(serde_json::Value::as_str)
                        .is_none()
                {
                    return Err(ProviderError::MalformedData(
                        "response message content has invalid type or text".into(),
                    ));
                }
                if part
                    .get("annotations")
                    .is_some_and(|annotations| !annotations.is_array())
                {
                    return Err(ProviderError::MalformedData(
                        "response message annotations must be an array".into(),
                    ));
                }
            }
            ResponsesOutputKind::Message
        }
        "function_call" => {
            for field in ["call_id", "name"] {
                let value = required_string(item, field)?;
                if value.is_empty() || value.len() > MAX_RESPONSE_OUTPUT_ID_BYTES {
                    return Err(ProviderError::MalformedData(format!(
                        "response function-call {field} is empty or too long"
                    )));
                }
            }
            required_string(item, "arguments")?;
            ResponsesOutputKind::FunctionCall
        }
        _ => {
            return Err(ProviderError::Protocol(
                "unsupported response output item".into(),
            ));
        }
    };
    let mut nodes = 0usize;
    let mut stack = vec![(item, 1usize)];
    while let Some((value, depth)) = stack.pop() {
        if depth > MAX_RESPONSE_OUTPUT_DEPTH {
            return Err(ProviderError::Protocol(
                "response output item exceeds JSON depth limit".into(),
            ));
        }
        nodes = nodes.saturating_add(1);
        if nodes > MAX_RESPONSE_OUTPUT_NODES {
            return Err(ProviderError::Protocol(
                "response output item exceeds JSON node limit".into(),
            ));
        }
        match value {
            serde_json::Value::Array(values) => {
                stack.extend(values.iter().map(|value| (value, depth + 1)));
            }
            serde_json::Value::Object(values) => {
                stack.extend(values.values().map(|value| (value, depth + 1)));
            }
            _ => {}
        }
    }
    Ok(kind)
}

fn validate_response_reasoning_parts(
    object: &serde_json::Map<String, serde_json::Value>,
    field: &str,
    expected_type: &str,
) -> Result<(), ProviderError> {
    let Some(parts) = object.get(field) else {
        return Ok(());
    };
    let parts = parts.as_array().ok_or_else(|| {
        ProviderError::MalformedData(format!("response reasoning item {field} must be an array"))
    })?;
    for part in parts {
        let part = part.as_object().ok_or_else(|| {
            ProviderError::MalformedData(format!(
                "response reasoning item {field} entry must be an object"
            ))
        })?;
        if part.get("type").and_then(serde_json::Value::as_str) != Some(expected_type)
            || part
                .get("text")
                .and_then(serde_json::Value::as_str)
                .is_none()
        {
            return Err(ProviderError::MalformedData(format!(
                "response reasoning item {field} entry has invalid type or text"
            )));
        }
    }
    Ok(())
}

fn response_message_text_parts(item: &serde_json::Value) -> Result<Vec<&str>, ProviderError> {
    if validate_response_output_item(item)? != ResponsesOutputKind::Message {
        return Err(ProviderError::Protocol(
            "response output item is not a message".into(),
        ));
    }
    Ok(item["content"]
        .as_array()
        .expect("validated response message content")
        .iter()
        .map(|part| {
            part.get("text")
                .and_then(serde_json::Value::as_str)
                .expect("validated response message text")
        })
        .collect())
}

fn response_output_item_bytes(item: &serde_json::Value) -> Result<usize, ProviderError> {
    validate_response_output_item(item)?;
    serde_json::to_vec(item)
        .map(|bytes| bytes.len())
        .map_err(|error| ProviderError::MalformedData(error.to_string()))
}

fn response_output_items_for_replay<'a>(
    message: &'a ChatMessage,
    source: &ProviderStateSourceV1,
) -> Result<Option<Vec<&'a serde_json::Value>>, ProviderError> {
    let mut selected = message
        .response_items
        .iter()
        .filter_map(|response_item| match response_item {
            ProviderResponseItem::ResponsesOutput {
                source: item_source,
                output_index,
                item,
            } if item_source == source => Some((*output_index, item)),
            _ => None,
        })
        .collect::<Vec<_>>();
    if selected.is_empty() {
        return Ok(None);
    }
    if selected.len() > MAX_RESPONSE_OUTPUT_ITEMS {
        return Err(ProviderError::Protocol(
            "too many response output items".into(),
        ));
    }
    selected.sort_by_key(|(output_index, _)| *output_index);
    let mut ids = BTreeSet::new();
    let mut total_bytes = 0usize;
    for pair in selected.windows(2) {
        if pair[0].0 == pair[1].0 {
            return Err(ProviderError::Protocol(
                "duplicate response output index".into(),
            ));
        }
    }
    for (_, item) in &selected {
        let bytes = response_output_item_bytes(item)?;
        total_bytes = total_bytes.saturating_add(bytes);
        if total_bytes > MAX_RESPONSE_OUTPUT_BYTES {
            return Err(ProviderError::EventTooLarge {
                limit: MAX_RESPONSE_OUTPUT_BYTES,
            });
        }
        let id = required_string(item, "id")?;
        if !ids.insert(id) {
            return Err(ProviderError::Protocol(
                "duplicate response output item id".into(),
            ));
        }
    }
    let selected = selected
        .into_iter()
        .map(|(_, item)| item)
        .collect::<Vec<_>>();
    validate_response_output_replay(message, &selected)?;
    Ok(Some(selected))
}

fn validate_response_output_replay(
    message: &ChatMessage,
    items: &[&serde_json::Value],
) -> Result<(), ProviderError> {
    let canonical_text = message.content.as_str().ok_or_else(|| {
        ProviderError::Protocol("response assistant history must contain text".into())
    })?;
    let mut output_text = String::new();
    let mut output_calls = Vec::new();
    for item in items {
        match validate_response_output_item(item)? {
            ResponsesOutputKind::Reasoning => {}
            ResponsesOutputKind::Message => {
                for text in response_message_text_parts(item)? {
                    output_text.push_str(text);
                }
            }
            ResponsesOutputKind::FunctionCall => output_calls.push((
                required_string(item, "call_id")?,
                required_string(item, "name")?,
                required_string(item, "arguments")?,
            )),
        }
    }
    if output_text != canonical_text {
        return Err(ProviderError::Protocol(
            "response output message does not match canonical assistant text".into(),
        ));
    }
    let canonical_calls = message.tool_calls.as_deref().unwrap_or_default();
    if output_calls.len() != canonical_calls.len()
        || output_calls
            .iter()
            .zip(canonical_calls)
            .any(|((call_id, name, arguments), call)| {
                call.call_type != "function"
                    || *call_id != call.id
                    || *name != call.function.name
                    || *arguments != call.function.arguments
            })
    {
        return Err(ProviderError::Protocol(
            "response output function calls do not match canonical assistant tool calls".into(),
        ));
    }
    Ok(())
}

fn required_string<'a>(
    value: &'a serde_json::Value,
    field: &str,
) -> Result<&'a str, ProviderError> {
    value
        .get(field)
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| ProviderError::MalformedData(format!("missing string field {field}")))
}

fn required_index(value: &serde_json::Value, field: &str) -> Result<usize, ProviderError> {
    value
        .get(field)
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(|| ProviderError::MalformedData(format!("missing integer field {field}")))
}

fn reconcile_text<K: Ord + Clone>(
    emitted: &mut BTreeMap<K, String>,
    key: K,
    full: &str,
    event: fn(String) -> ProviderEvent,
    events: &mut Vec<ProviderEvent>,
) -> Result<(), ProviderError> {
    let current = emitted.entry(key).or_default();
    let suffix = full
        .strip_prefix(current.as_str())
        .ok_or_else(|| ProviderError::Protocol("conflicting completed response text".into()))?;
    if !suffix.is_empty() {
        events.push(event(suffix.to_string()));
        current.push_str(suffix);
    }
    Ok(())
}

fn required_usage_token(usage: &serde_json::Value, field: &str) -> Result<u32, ProviderError> {
    let value = usage
        .get(field)
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| ProviderError::MalformedData(format!("missing usage field {field}")))?;
    u32::try_from(value)
        .map_err(|_| ProviderError::MalformedData(format!("usage field {field} is too large")))
}

fn optional_usage_pointer(
    usage: &serde_json::Value,
    pointer: &str,
) -> Result<Option<u32>, ProviderError> {
    let Some(value) = usage.pointer(pointer) else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    let value = value.as_u64().ok_or_else(|| {
        ProviderError::MalformedData(format!("usage field {pointer} is not an integer"))
    })?;
    Ok(Some(u32::try_from(value).map_err(|_| {
        ProviderError::MalformedData(format!("usage field {pointer} is too large"))
    })?))
}

fn responses_usage(
    value: &serde_json::Value,
    response_metadata: Option<ProviderResponseMetadata>,
    format: Option<PromptCacheUsageFormat>,
) -> Result<Option<UsageInfo>, ProviderError> {
    let Some(usage) = value.get("usage") else {
        return Ok(None);
    };
    if usage.is_null() {
        return Ok(None);
    }
    let prompt_tokens = required_usage_token(usage, "input_tokens")?;
    let parse_cache = || -> Result<_, ProviderError> {
        use crate::agent::stream::{CacheCount, CacheUsage};
        let read = optional_usage_pointer(usage, "/input_tokens_details/cached_tokens")?;
        let write = optional_usage_pointer(usage, "/input_tokens_details/cache_write_tokens")?;
        let miss = read
            .and_then(|read| prompt_tokens.checked_sub(read))
            .map_or(CacheCount::Unknown, CacheCount::Derived);
        Ok((
            read,
            write,
            CacheUsage::new(
                read.map_or(CacheCount::Unknown, CacheCount::Reported),
                write.map_or(CacheCount::Unknown, CacheCount::Reported),
                miss,
            ),
        ))
    };
    let cache_present = usage
        .pointer("/input_tokens_details/cached_tokens")
        .is_some()
        || usage
            .pointer("/input_tokens_details/cache_write_tokens")
            .is_some();
    let (cached_prompt_tokens, cache_write_tokens, cache_usage) = match format {
        Some(PromptCacheUsageFormat::HitMiss | PromptCacheUsageFormat::ReadWrite) => {
            return Err(ProviderError::MalformedData(
                "configured cache usage shape is incompatible with openai_responses".into(),
            ));
        }
        Some(PromptCacheUsageFormat::Auto) if cache_present => {
            parse_cache().unwrap_or((None, None, crate::agent::stream::CacheUsage::UNKNOWN))
        }
        Some(PromptCacheUsageFormat::Auto) => {
            (None, None, crate::agent::stream::CacheUsage::UNKNOWN)
        }
        Some(PromptCacheUsageFormat::OpenAiDetails) | None if cache_present => parse_cache()?,
        Some(PromptCacheUsageFormat::OpenAiDetails) | None => {
            (None, None, crate::agent::stream::CacheUsage::UNKNOWN)
        }
    };
    let base = UsageInfo {
        prompt_tokens,
        completion_tokens: required_usage_token(usage, "output_tokens")?,
        total_tokens: required_usage_token(usage, "total_tokens")?,
        cached_prompt_tokens,
        cache_write_tokens,
        reasoning_tokens: optional_usage_pointer(usage, "/output_tokens_details/reasoning_tokens")?,
        response_metadata,
        cache_usage_override: None,
    };
    base.with_cache_usage(cache_usage).map(Some).ok_or_else(|| {
        ProviderError::MalformedData("provider cache usage exceeds prompt tokens".into())
    })
}

#[derive(Debug)]
enum AnthropicContentBlock {
    Text,
    Thinking,
    ToolUse {
        arguments: String,
        initial_arguments: Option<String>,
    },
    Ignored,
}

#[derive(Debug, Default)]
struct AnthropicUsageState {
    input_tokens: u32,
    cache_creation_input_tokens: Option<u32>,
    cache_read_input_tokens: Option<u32>,
    output_tokens: u32,
    observed: bool,
}

impl AnthropicUsageState {
    fn update_initial(
        &mut self,
        usage: &serde_json::Value,
        cache_usage_format: Option<PromptCacheUsageFormat>,
    ) -> Result<(), ProviderError> {
        self.input_tokens = anthropic_token(usage, "input_tokens")?;
        self.output_tokens = anthropic_optional_token(usage, "output_tokens")?.unwrap_or(0);
        let cache_tokens = || -> Result<(Option<u32>, Option<u32>), ProviderError> {
            Ok((
                anthropic_optional_token(usage, "cache_creation_input_tokens")?,
                anthropic_optional_token(usage, "cache_read_input_tokens")?,
            ))
        };
        let (write, read) = if cache_usage_format == Some(PromptCacheUsageFormat::Auto) {
            cache_tokens().unwrap_or((None, None))
        } else {
            cache_tokens()?
        };
        self.cache_creation_input_tokens = write;
        self.cache_read_input_tokens = read;
        self.observed = true;
        Ok(())
    }

    fn update_delta(&mut self, usage: &serde_json::Value) -> Result<(), ProviderError> {
        if let Some(value) = anthropic_optional_token(usage, "output_tokens")? {
            self.output_tokens = value;
            self.observed = true;
        }
        Ok(())
    }

    fn canonical(&self) -> Result<Option<UsageInfo>, ProviderError> {
        if !self.observed {
            return Ok(None);
        }
        let prompt_tokens = self
            .input_tokens
            .checked_add(self.cache_creation_input_tokens.unwrap_or(0))
            .and_then(|tokens| tokens.checked_add(self.cache_read_input_tokens.unwrap_or(0)))
            .ok_or_else(|| ProviderError::MalformedData("Anthropic input usage overflow".into()))?;
        let total_tokens = prompt_tokens
            .checked_add(self.output_tokens)
            .ok_or_else(|| ProviderError::MalformedData("Anthropic total usage overflow".into()))?;
        use crate::agent::stream::{CacheCount, CacheUsage};
        let miss = self
            .cache_read_input_tokens
            .and_then(|read| prompt_tokens.checked_sub(read))
            .map_or(CacheCount::Unknown, CacheCount::Derived);
        let base = UsageInfo {
            prompt_tokens,
            completion_tokens: self.output_tokens,
            total_tokens,
            cached_prompt_tokens: self.cache_read_input_tokens,
            cache_write_tokens: self.cache_creation_input_tokens,
            reasoning_tokens: None,
            response_metadata: None,
            cache_usage_override: None,
        };
        base.with_cache_usage(CacheUsage::new(
            self.cache_read_input_tokens
                .map_or(CacheCount::Unknown, CacheCount::Reported),
            self.cache_creation_input_tokens
                .map_or(CacheCount::Unknown, CacheCount::Reported),
            miss,
        ))
        .map(Some)
        .ok_or_else(|| ProviderError::MalformedData("Anthropic cache usage is invalid".into()))
    }
}

pub(crate) struct AnthropicMessagesDecoder {
    buffer: Vec<u8>,
    data_lines: Vec<Vec<u8>>,
    event_size: usize,
    event_limit: usize,
    blocks: BTreeMap<usize, AnthropicContentBlock>,
    usage: AnthropicUsageState,
    started: bool,
    finished: bool,
    completed: bool,
    cache_usage_format: Option<PromptCacheUsageFormat>,
}

impl AnthropicMessagesDecoder {
    #[cfg(test)]
    pub(crate) fn new() -> Self {
        Self::new_with_cache_usage(None)
    }

    fn new_with_cache_usage(cache_usage_format: Option<PromptCacheUsageFormat>) -> Self {
        Self {
            buffer: Vec::new(),
            data_lines: Vec::new(),
            event_size: 0,
            event_limit: DEFAULT_SSE_EVENT_LIMIT,
            blocks: BTreeMap::new(),
            usage: AnthropicUsageState::default(),
            started: false,
            finished: false,
            completed: false,
            cache_usage_format,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_event_limit(event_limit: usize) -> Self {
        Self {
            event_limit,
            ..Self::new()
        }
    }

    pub(crate) fn push(&mut self, chunk: &[u8]) -> Result<Vec<ProviderEvent>, ProviderError> {
        self.buffer.extend_from_slice(chunk);
        self.enforce_limit()?;
        let mut events = Vec::new();
        while let Some(newline) = self.buffer.iter().position(|byte| *byte == b'\n') {
            let mut line: Vec<u8> = self.buffer.drain(..=newline).collect();
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            if let Some(event) = self.process_line(&line)? {
                events.extend(event);
            }
        }
        self.enforce_limit()?;
        Ok(events)
    }

    pub(crate) fn finish(&mut self) -> Result<Vec<ProviderEvent>, ProviderError> {
        let mut events = Vec::new();
        if !self.buffer.is_empty() {
            let mut line = std::mem::take(&mut self.buffer);
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            if let Some(event) = self.process_line(&line)? {
                events.extend(event);
            }
        }
        if let Some(event) = self.dispatch()? {
            events.extend(event);
        }
        Ok(events)
    }

    fn enforce_limit(&self) -> Result<(), ProviderError> {
        if self.event_size.saturating_add(self.buffer.len()) > self.event_limit {
            Err(ProviderError::EventTooLarge {
                limit: self.event_limit,
            })
        } else {
            Ok(())
        }
    }

    fn process_line(&mut self, line: &[u8]) -> Result<Option<Vec<ProviderEvent>>, ProviderError> {
        if line.is_empty() {
            return self.dispatch();
        }
        if line.first() == Some(&b':') {
            return Ok(None);
        }
        let colon = line.iter().position(|byte| *byte == b':');
        let (field, mut value) = match colon {
            Some(index) => (&line[..index], &line[index + 1..]),
            None => (line, &[][..]),
        };
        if field != b"data" {
            return Ok(None);
        }
        if value.first() == Some(&b' ') {
            value = &value[1..];
        }
        let separator = usize::from(!self.data_lines.is_empty());
        self.event_size = self.event_size.saturating_add(separator + value.len());
        if self.event_size > self.event_limit {
            return Err(ProviderError::EventTooLarge {
                limit: self.event_limit,
            });
        }
        self.data_lines.push(value.to_vec());
        Ok(None)
    }

    fn dispatch(&mut self) -> Result<Option<Vec<ProviderEvent>>, ProviderError> {
        if self.data_lines.is_empty() {
            self.event_size = 0;
            return Ok(None);
        }
        let lines = std::mem::take(&mut self.data_lines);
        self.event_size = 0;
        let mut data = Vec::new();
        for (index, line) in lines.into_iter().enumerate() {
            if index > 0 {
                data.push(b'\n');
            }
            data.extend(line);
        }
        let data = String::from_utf8(data).map_err(|_| ProviderError::InvalidUtf8)?;
        let value: serde_json::Value = serde_json::from_str(&data)
            .map_err(|error| ProviderError::MalformedData(error.to_string()))?;
        Ok(Some(self.translate(&value)?))
    }

    fn translate(
        &mut self,
        value: &serde_json::Value,
    ) -> Result<Vec<ProviderEvent>, ProviderError> {
        if self.completed {
            return Err(ProviderError::Protocol(
                "event received after Anthropic message_stop".into(),
            ));
        }
        let event_type = required_string(value, "type")?;
        let mut events = Vec::new();
        match event_type {
            "message_start" => {
                if self.started {
                    return Err(ProviderError::Protocol(
                        "duplicate Anthropic message_start".into(),
                    ));
                }
                let message = value.get("message").ok_or_else(|| {
                    ProviderError::MalformedData("missing Anthropic message".into())
                })?;
                if message
                    .get("content")
                    .and_then(serde_json::Value::as_array)
                    .is_some_and(|content| !content.is_empty())
                {
                    return Err(ProviderError::Protocol(
                        "Anthropic message_start contained nonempty content".into(),
                    ));
                }
                if let Some(usage) = message.get("usage") {
                    self.usage.update_initial(usage, self.cache_usage_format)?;
                }
                self.started = true;
            }
            "content_block_start" => {
                self.require_started()?;
                let index = required_index(value, "index")?;
                if self.blocks.contains_key(&index) {
                    return Err(ProviderError::Protocol(
                        "duplicate Anthropic content block".into(),
                    ));
                }
                let content = value.get("content_block").ok_or_else(|| {
                    ProviderError::MalformedData("missing Anthropic content block".into())
                })?;
                let block = match required_string(content, "type")? {
                    "text" => {
                        if let Some(text) = content
                            .get("text")
                            .and_then(serde_json::Value::as_str)
                            .filter(|text| !text.is_empty())
                        {
                            events.push(ProviderEvent::Text(text.to_string()));
                        }
                        AnthropicContentBlock::Text
                    }
                    "thinking" => {
                        if let Some(thinking) = content
                            .get("thinking")
                            .and_then(serde_json::Value::as_str)
                            .filter(|text| !text.is_empty())
                        {
                            events.push(ProviderEvent::Reasoning(thinking.to_string()));
                        }
                        AnthropicContentBlock::Thinking
                    }
                    "tool_use" => {
                        let id = required_string(content, "id")?;
                        let name = required_string(content, "name")?;
                        let input = content.get("input").ok_or_else(|| {
                            ProviderError::MalformedData("missing Anthropic tool-use input".into())
                        })?;
                        let input_object = input.as_object().ok_or_else(|| {
                            ProviderError::Protocol(
                                "Anthropic tool-use input was not an object".into(),
                            )
                        })?;
                        events.push(ProviderEvent::ToolCallStart {
                            index,
                            id: id.to_string(),
                            name: name.to_string(),
                        });
                        AnthropicContentBlock::ToolUse {
                            arguments: String::new(),
                            initial_arguments: (!input_object.is_empty()).then(|| {
                                serde_json::to_string(input).expect("JSON value serializes")
                            }),
                        }
                    }
                    _ => AnthropicContentBlock::Ignored,
                };
                self.blocks.insert(index, block);
            }
            "content_block_delta" => {
                self.require_started()?;
                let index = required_index(value, "index")?;
                let delta = value.get("delta").ok_or_else(|| {
                    ProviderError::MalformedData("missing Anthropic content delta".into())
                })?;
                let delta_type = required_string(delta, "type")?;
                let block = self.blocks.get_mut(&index).ok_or_else(|| {
                    ProviderError::Protocol("Anthropic delta has no active content block".into())
                })?;
                if delta_type == "refusal_delta" {
                    return Err(ProviderError::ResponseRefused);
                }
                match (delta_type, block) {
                    ("text_delta", AnthropicContentBlock::Text) => {
                        let text = required_string(delta, "text")?;
                        if !text.is_empty() {
                            events.push(ProviderEvent::Text(text.to_string()));
                        }
                    }
                    ("thinking_delta", AnthropicContentBlock::Thinking) => {
                        let thinking = required_string(delta, "thinking")?;
                        if !thinking.is_empty() {
                            events.push(ProviderEvent::Reasoning(thinking.to_string()));
                        }
                    }
                    ("input_json_delta", AnthropicContentBlock::ToolUse { arguments, .. }) => {
                        let partial = required_string(delta, "partial_json")?;
                        if !partial.is_empty() {
                            arguments.push_str(partial);
                            events.push(ProviderEvent::ToolCallArg {
                                index,
                                arg: partial.to_string(),
                            });
                        }
                    }
                    ("signature_delta", AnthropicContentBlock::Thinking)
                    | ("citations_delta", AnthropicContentBlock::Text)
                    | (_, AnthropicContentBlock::Ignored) => {}
                    (unknown, _)
                        if !matches!(
                            unknown,
                            "text_delta" | "thinking_delta" | "input_json_delta"
                        ) => {}
                    _ => {
                        return Err(ProviderError::Protocol(
                            "Anthropic delta did not match its content block".into(),
                        ))
                    }
                }
            }
            "content_block_stop" => {
                self.require_started()?;
                let index = required_index(value, "index")?;
                let block = self.blocks.remove(&index).ok_or_else(|| {
                    ProviderError::Protocol("Anthropic stop has no active content block".into())
                })?;
                if let AnthropicContentBlock::ToolUse {
                    arguments,
                    initial_arguments,
                } = block
                {
                    if let Some(initial) = initial_arguments {
                        if !arguments.is_empty() {
                            return Err(ProviderError::Protocol(
                                "Anthropic tool input conflicted with streamed fragments".into(),
                            ));
                        }
                        events.push(ProviderEvent::ToolCallArg {
                            index,
                            arg: initial,
                        });
                    } else if arguments.is_empty() {
                        events.push(ProviderEvent::ToolCallArg {
                            index,
                            arg: "{}".into(),
                        });
                    }
                }
            }
            "message_delta" => {
                self.require_started()?;
                if let Some(usage) = value.get("usage") {
                    self.usage.update_delta(usage)?;
                }
                let delta = value.get("delta").ok_or_else(|| {
                    ProviderError::MalformedData("missing Anthropic message delta".into())
                })?;
                if let Some(reason) = delta.get("stop_reason").and_then(serde_json::Value::as_str) {
                    if self.finished {
                        return Err(ProviderError::Protocol(
                            "duplicate Anthropic stop reason".into(),
                        ));
                    }
                    if reason == "refusal" {
                        return Err(ProviderError::ResponseRefused);
                    }
                    self.finished = true;
                    events.push(ProviderEvent::Finish {
                        reason: match reason {
                            "end_turn" | "stop_sequence" => FinishReason::Stop,
                            "tool_use" => FinishReason::ToolCalls,
                            "max_tokens" | "model_context_window_exceeded" => FinishReason::Length,
                            other => FinishReason::Other(other.to_string()),
                        },
                        usage: self.usage.canonical()?,
                    });
                }
            }
            "message_stop" => {
                self.require_started()?;
                if !self.blocks.is_empty() {
                    return Err(ProviderError::Protocol(
                        "Anthropic message stopped with active content blocks".into(),
                    ));
                }
                if !self.finished {
                    return Err(ProviderError::PrematureEof);
                }
                self.completed = true;
                events.push(ProviderEvent::Done);
            }
            "error" => return Err(ProviderError::ResponseFailed),
            "ping" => {}
            _ => {}
        }
        Ok(events)
    }

    fn require_started(&self) -> Result<(), ProviderError> {
        if self.started {
            Ok(())
        } else {
            Err(ProviderError::Protocol(
                "Anthropic event arrived before message_start".into(),
            ))
        }
    }
}

fn anthropic_token(value: &serde_json::Value, field: &str) -> Result<u32, ProviderError> {
    anthropic_optional_token(value, field)?
        .ok_or_else(|| ProviderError::MalformedData(format!("missing usage field {field}")))
}

fn anthropic_optional_token(
    value: &serde_json::Value,
    field: &str,
) -> Result<Option<u32>, ProviderError> {
    let Some(value) = value.get(field) else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    let value = value.as_u64().ok_or_else(|| {
        ProviderError::MalformedData(format!("usage field {field} is not an integer"))
    })?;
    Ok(Some(u32::try_from(value).map_err(|_| {
        ProviderError::MalformedData(format!("usage field {field} is too large"))
    })?))
}

impl ProviderStreamDecoder {
    pub(crate) fn push(&mut self, chunk: &[u8]) -> Result<Vec<ProviderEvent>, ProviderError> {
        match self {
            Self::ChatCompletions(decoder) => decoder.push(chunk),
            Self::OpenAiResponses(decoder) => decoder.push(chunk),
            Self::AnthropicMessages(decoder) => decoder.push(chunk),
        }
    }

    pub(crate) fn finish(&mut self) -> Result<Vec<ProviderEvent>, ProviderError> {
        match self {
            Self::ChatCompletions(decoder) => decoder.finish(),
            Self::OpenAiResponses(decoder) => decoder.finish(),
            Self::AnthropicMessages(decoder) => decoder.finish(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct ChatCompletionsRequest {
    model: String,
    messages: Vec<ChatCompletionsMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<ToolDef>>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_effort: Option<ReasoningEffort>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning: Option<ReasoningRequest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking: Option<ThinkingRequest>,
}

#[derive(Debug, Clone, Serialize)]
struct ChatCompletionsMessage {
    role: String,
    content: ChatCompletionsContent,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<crate::agent::client::ToolCallMsg>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
enum ChatCompletionsContent {
    Text(String),
    Blocks(Vec<ChatCompletionsContentBlock>),
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ChatCompletionsContentBlock {
    Text { text: String },
    ImageUrl { image_url: ChatCompletionsImageUrl },
}

#[derive(Debug, Clone, Serialize)]
struct ChatCompletionsImageUrl {
    url: String,
}

#[derive(Debug, Clone, Serialize)]
struct ReasoningRequest {
    effort: ReasoningEffort,
}

#[derive(Debug, Clone, Serialize)]
struct ThinkingRequest {
    #[serde(rename = "type")]
    thinking_type: &'static str,
}

fn chat_completions_request(
    request: &crate::request::PreparedModelRequest,
    source: &ProviderStateSourceV1,
    contract: Option<crate::provider_contract::FirstPartyContract>,
) -> Result<ChatCompletionsRequest, ProviderError> {
    let (reasoning_effort, reasoning, thinking) = match contract
        .map(|contract| contract.chat_reasoning_wire(request.options.reasoning_effort))
        .unwrap_or(crate::provider_contract::ChatReasoningWire::Generic)
    {
        crate::provider_contract::ChatReasoningWire::Mimo { enabled } => (
            None,
            None,
            Some(ThinkingRequest {
                thinking_type: if enabled { "enabled" } else { "disabled" },
            }),
        ),
        crate::provider_contract::ChatReasoningWire::DeepSeek { enabled, effort } => (
            effort,
            None,
            Some(ThinkingRequest {
                thinking_type: if enabled { "enabled" } else { "disabled" },
            }),
        ),
        crate::provider_contract::ChatReasoningWire::Generic => match (
            request.options.reasoning_effort,
            request.options.reasoning_request_format,
        ) {
            (Some(effort), Some(ReasoningRequestFormat::ReasoningEffort)) => {
                (Some(effort), None, None)
            }
            (Some(effort), Some(ReasoningRequestFormat::ReasoningObject)) => {
                (None, Some(ReasoningRequest { effort }), None)
            }
            _ => (None, None, None),
        },
    };
    Ok(ChatCompletionsRequest {
        model: request.model.clone(),
        messages: request
            .messages
            .iter()
            .map(|message| chat_completions_message(message, contract, source))
            .collect::<Result<Vec<_>, _>>()?,
        tools: (!request.tools.is_empty()).then(|| request.tools.clone()),
        stream: true,
        reasoning_effort,
        reasoning,
        thinking,
    })
}

fn chat_completions_message(
    message: &ChatMessage,
    contract: Option<crate::provider_contract::FirstPartyContract>,
    source: &ProviderStateSourceV1,
) -> Result<ChatCompletionsMessage, ProviderError> {
    let content = if let Some(text) = message.content.as_str() {
        ChatCompletionsContent::Text(text.to_string())
    } else {
        if message.role != "user" {
            return Err(ProviderError::Protocol(
                "image content is supported only on user messages".into(),
            ));
        }
        ChatCompletionsContent::Blocks(
            message
                .content
                .blocks()
                .iter()
                .map(|block| match block {
                    crate::image_content::ModelContentBlock::Text(text) => {
                        ChatCompletionsContentBlock::Text { text: text.clone() }
                    }
                    crate::image_content::ModelContentBlock::Image(image) => {
                        ChatCompletionsContentBlock::ImageUrl {
                            image_url: ChatCompletionsImageUrl {
                                url: image.data_url(),
                            },
                        }
                    }
                })
                .collect(),
        )
    };
    let reasoning_content = if message.role == "assistant"
        && message.tool_calls.is_some()
        && contract.is_some_and(|contract| contract.replays_chat_reasoning())
    {
        let mut matches = message.response_items.iter().filter_map(|item| match item {
            ProviderResponseItem::ChatReasoning {
                source: item_source,
                content,
            } if item_source == source => Some(content),
            _ => None,
        });
        let reasoning_content = matches.next().cloned();
        if matches.next().is_some() {
            return Err(ProviderError::Protocol(
                "assistant message contains duplicate Chat reasoning state".into(),
            ));
        }
        if reasoning_content
            .as_ref()
            .is_some_and(|content| content.len() > MAX_CHAT_REASONING_BYTES)
        {
            return Err(ProviderError::EventTooLarge {
                limit: MAX_CHAT_REASONING_BYTES,
            });
        }
        reasoning_content
    } else {
        None
    };
    Ok(ChatCompletionsMessage {
        role: message.role.clone(),
        content,
        reasoning_content,
        tool_calls: message.tool_calls.clone(),
        tool_call_id: message.tool_call_id.clone(),
    })
}

fn serialize_chat_completions(
    request: &crate::request::PreparedModelRequest,
    source: &ProviderStateSourceV1,
    contract: Option<crate::provider_contract::FirstPartyContract>,
) -> Result<Vec<u8>, ProviderError> {
    serde_json::to_vec(&chat_completions_request(request, source, contract)?).map_err(|_| {
        ProviderError::Protocol("could not serialize the prepared provider request".into())
    })
}

#[derive(Debug, Clone, Serialize)]
struct OpenAiResponsesRequest {
    model: String,
    input: Vec<OpenAiResponsesInputItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<OpenAiResponsesTool>>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning: Option<ReasoningRequest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    include: Option<Vec<&'static str>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_output_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    store: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    service_tier: Option<crate::agent::client::OpenAiServiceTier>,
    #[serde(skip_serializing_if = "Option::is_none")]
    prompt_cache_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    prompt_cache_options: Option<PromptCacheOptions>,
}

#[derive(Debug, Clone, Serialize)]
struct PromptCacheOptions {
    mode: &'static str,
}

#[derive(Debug, Clone, Serialize)]
struct PromptCacheBreakpoint {
    mode: &'static str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
enum OpenAiResponsesInputItem {
    Output(serde_json::Value),
    Message {
        role: String,
        content: OpenAiResponsesMessageContent,
    },
    FunctionCall {
        #[serde(rename = "type")]
        item_type: &'static str,
        call_id: String,
        name: String,
        arguments: String,
    },
    FunctionCallOutput {
        #[serde(rename = "type")]
        item_type: &'static str,
        call_id: String,
        output: String,
    },
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
enum OpenAiResponsesMessageContent {
    Text(String),
    Blocks(Vec<OpenAiResponsesContentBlock>),
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum OpenAiResponsesContentBlock {
    InputText {
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        prompt_cache_breakpoint: Option<PromptCacheBreakpoint>,
    },
    InputImage {
        image_url: String,
    },
}

#[derive(Debug, Clone, Serialize)]
struct OpenAiResponsesTool {
    #[serde(rename = "type")]
    tool_type: &'static str,
    name: String,
    description: String,
    parameters: serde_json::Value,
    strict: bool,
}

fn openai_responses_request(
    request: &crate::request::PreparedModelRequest,
    source: &ProviderStateSourceV1,
    contract: Option<crate::provider_contract::FirstPartyContract>,
    options: Option<&OpenAiResponsesRequestOptions>,
    prompt_cache: Option<&OpenAiPromptCacheRequest>,
) -> Result<OpenAiResponsesRequest, ProviderError> {
    let stable_prefix_index = prompt_cache
        .filter(|cache| cache.policy == PromptCachePolicy::StablePrefix)
        .and_then(|cache| cache.stable_prefix_messages)
        .and_then(|count| count.checked_sub(1));
    if prompt_cache.is_some_and(|cache| {
        cache.policy == PromptCachePolicy::StablePrefix && stable_prefix_index.is_none()
    }) {
        return Err(ProviderError::Protocol(
            "stable-prefix caching requires a typed non-empty instruction boundary".into(),
        ));
    }
    if let Some(index) = stable_prefix_index {
        if index >= request.messages.len()
            || request.messages[..=index]
                .iter()
                .any(|message| !matches!(message.role.as_str(), "system" | "developer"))
        {
            return Err(ProviderError::Protocol(
                "prompt cache stable boundary is not a leading instruction prefix".into(),
            ));
        }
    }
    let mut input = Vec::new();
    for (message_index, message) in request.messages.iter().enumerate() {
        match message.role.as_str() {
            "system" | "developer" | "user" => {
                if message.tool_calls.is_some()
                    || message.tool_call_id.is_some()
                    || !message.response_items.is_empty()
                {
                    return Err(ProviderError::Protocol(
                        "invalid tool metadata on response input message".into(),
                    ));
                }
                input.push(OpenAiResponsesInputItem::Message {
                    role: message.role.clone(),
                    content: openai_responses_content(
                        message,
                        contract,
                        stable_prefix_index == Some(message_index),
                    )?,
                });
            }
            "assistant" => {
                let content = message.content.as_str().ok_or_else(|| {
                    ProviderError::Protocol(
                        "image content is supported only on user messages".into(),
                    )
                })?;
                let calls = message.tool_calls.as_deref().unwrap_or_default();
                if message.tool_call_id.is_some() {
                    return Err(ProviderError::Protocol(
                        "invalid tool result identifier on assistant message".into(),
                    ));
                }
                if let Some(items) = response_output_items_for_replay(message, source)? {
                    input.extend(
                        items
                            .into_iter()
                            .map(|item| OpenAiResponsesInputItem::Output(item.clone())),
                    );
                } else {
                    if !content.is_empty() || calls.is_empty() {
                        input.push(OpenAiResponsesInputItem::Message {
                            role: message.role.clone(),
                            content: OpenAiResponsesMessageContent::Text(content.to_string()),
                        });
                    }
                    for call in calls {
                        if call.call_type != "function" {
                            return Err(ProviderError::Protocol(
                                "openai_responses supports function tool calls only".into(),
                            ));
                        }
                        input.push(OpenAiResponsesInputItem::FunctionCall {
                            item_type: "function_call",
                            call_id: call.id.clone(),
                            name: call.function.name.clone(),
                            arguments: call.function.arguments.clone(),
                        });
                    }
                }
            }
            "tool" => {
                let content = message
                    .content
                    .as_str()
                    .ok_or_else(|| ProviderError::Protocol("tool results must be text".into()))?;
                if message.tool_calls.is_some() || !message.response_items.is_empty() {
                    return Err(ProviderError::Protocol(
                        "invalid nested tool call on tool result".into(),
                    ));
                }
                let call_id = message.tool_call_id.clone().ok_or_else(|| {
                    ProviderError::Protocol("tool result is missing call_id".into())
                })?;
                input.push(OpenAiResponsesInputItem::FunctionCallOutput {
                    item_type: "function_call_output",
                    call_id,
                    output: content.to_string(),
                });
            }
            _ => {
                return Err(ProviderError::Protocol(
                    "unsupported response input message role".into(),
                ));
            }
        }
    }
    let tools = if request.tools.is_empty() {
        None
    } else {
        Some(
            request
                .tools
                .iter()
                .map(|tool| {
                    if tool.def_type != "function" {
                        return Err(ProviderError::Protocol(
                            "openai_responses supports function tools only".into(),
                        ));
                    }
                    Ok(OpenAiResponsesTool {
                        tool_type: "function",
                        name: tool.function.name.clone(),
                        description: tool.function.description.clone(),
                        parameters: tool.function.parameters.clone(),
                        strict: false,
                    })
                })
                .collect::<Result<Vec<_>, ProviderError>>()?,
        )
    };
    let reasoning_effort = match contract {
        Some(contract) => contract
            .responses_reasoning_effort(request.options.reasoning_effort)
            .map_err(|message| ProviderError::Protocol(message.into()))?,
        None => request.options.reasoning_effort,
    };
    let (prompt_cache_key, prompt_cache_options) = match prompt_cache {
        Some(OpenAiPromptCacheRequest {
            policy: PromptCachePolicy::Automatic,
            key,
            ..
        }) => (key.clone(), None),
        Some(OpenAiPromptCacheRequest {
            policy: PromptCachePolicy::StablePrefix,
            key,
            ..
        }) => (key.clone(), Some(PromptCacheOptions { mode: "explicit" })),
        Some(OpenAiPromptCacheRequest {
            policy: PromptCachePolicy::Off,
            ..
        }) => (None, Some(PromptCacheOptions { mode: "explicit" })),
        None => (None, None),
    };
    Ok(OpenAiResponsesRequest {
        model: request.model.clone(),
        input,
        tools,
        stream: true,
        reasoning: reasoning_effort.map(|effort| ReasoningRequest { effort }),
        include: contract
            .is_some_and(|contract| contract.includes_encrypted_reasoning())
            .then(|| vec!["reasoning.encrypted_content"]),
        max_output_tokens: options.and_then(|options| options.max_output_tokens()),
        store: if contract.is_some_and(|contract| contract.is_openai()) {
            Some(false)
        } else {
            options.map(|options| options.store())
        },
        service_tier: options.and_then(|options| options.service_tier()),
        prompt_cache_key,
        prompt_cache_options,
    })
}

fn openai_responses_content(
    message: &ChatMessage,
    contract: Option<crate::provider_contract::FirstPartyContract>,
    cache_breakpoint: bool,
) -> Result<OpenAiResponsesMessageContent, ProviderError> {
    if let Some(text) = message.content.as_str() {
        if cache_breakpoint {
            return Ok(OpenAiResponsesMessageContent::Blocks(vec![
                OpenAiResponsesContentBlock::InputText {
                    text: text.to_string(),
                    prompt_cache_breakpoint: Some(PromptCacheBreakpoint { mode: "explicit" }),
                },
            ]));
        }
        return Ok(OpenAiResponsesMessageContent::Text(text.to_string()));
    }
    if message.role != "user" {
        return Err(ProviderError::Protocol(
            "image content is supported only on user messages".into(),
        ));
    }
    if contract.is_some_and(|contract| !contract.supports_images())
        && message
            .content
            .blocks()
            .iter()
            .any(|block| matches!(block, crate::image_content::ModelContentBlock::Image(_)))
    {
        return Err(ProviderError::Protocol(
            "configured first-party model does not support image input".into(),
        ));
    }
    Ok(OpenAiResponsesMessageContent::Blocks(
        message
            .content
            .blocks()
            .iter()
            .map(|block| match block {
                crate::image_content::ModelContentBlock::Text(text) => {
                    OpenAiResponsesContentBlock::InputText {
                        text: text.clone(),
                        prompt_cache_breakpoint: None,
                    }
                }
                crate::image_content::ModelContentBlock::Image(image) => {
                    OpenAiResponsesContentBlock::InputImage {
                        image_url: image.data_url(),
                    }
                }
            })
            .collect(),
    ))
}

fn serialize_openai_responses(
    request: &crate::request::PreparedModelRequest,
    source: &ProviderStateSourceV1,
    contract: Option<crate::provider_contract::FirstPartyContract>,
    options: Option<&OpenAiResponsesRequestOptions>,
    prompt_cache: Option<&OpenAiPromptCacheRequest>,
) -> Result<Vec<u8>, ProviderError> {
    serde_json::to_vec(&openai_responses_request(
        request,
        source,
        contract,
        options,
        prompt_cache,
    )?)
    .map_err(|_| {
        ProviderError::Protocol("could not serialize the prepared provider request".into())
    })
}

#[cfg(test)]
pub(crate) fn openai_responses_bytes_for_test(
    request: &crate::request::PreparedModelRequest,
) -> Result<Vec<u8>, ProviderError> {
    let source = test_state_source_for_request(request, ProviderProtocol::OpenAiResponses);
    serialize_openai_responses(
        request,
        &source,
        test_contract_for_request(request, ProviderProtocol::OpenAiResponses),
        None,
        None,
    )
}

#[cfg(test)]
pub(crate) fn openai_responses_bytes_with_options_for_test(
    request: &crate::request::PreparedModelRequest,
    options: &OpenAiResponsesRequestOptions,
) -> Result<Vec<u8>, ProviderError> {
    let source = test_state_source_for_request(request, ProviderProtocol::OpenAiResponses);
    serialize_openai_responses(
        request,
        &source,
        test_contract_for_request(request, ProviderProtocol::OpenAiResponses),
        Some(options),
        None,
    )
}

const ANTHROPIC_MAX_TOKENS: u32 = 16_384;

#[derive(Debug, Clone, Serialize)]
struct AnthropicMessagesRequest {
    model: String,
    max_tokens: u32,
    messages: Vec<AnthropicMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<Vec<AnthropicSystemBlock>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<AnthropicTool>>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    output_config: Option<AnthropicOutputConfig>,
}

#[derive(Debug, Clone, Serialize)]
struct AnthropicMessage {
    role: String,
    content: Vec<AnthropicContent>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AnthropicContent {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    ToolResult {
        tool_use_id: String,
        content: String,
    },
    Image {
        source: AnthropicImageSource,
    },
}

#[derive(Debug, Clone, Serialize)]
struct AnthropicImageSource {
    #[serde(rename = "type")]
    source_type: &'static str,
    media_type: &'static str,
    data: String,
}

#[derive(Debug, Clone, Serialize)]
struct AnthropicSystemBlock {
    #[serde(rename = "type")]
    block_type: &'static str,
    text: String,
}

#[derive(Debug, Clone, Serialize)]
struct AnthropicTool {
    name: String,
    description: String,
    input_schema: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
struct AnthropicOutputConfig {
    effort: &'static str,
}

fn anthropic_messages_request(
    request: &crate::request::PreparedModelRequest,
) -> Result<AnthropicMessagesRequest, ProviderError> {
    let mut system = Vec::new();
    let mut cursor = 0usize;
    while let Some(message) = request.messages.get(cursor) {
        if !matches!(message.role.as_str(), "system" | "developer") {
            break;
        }
        if message.tool_calls.is_some() || message.tool_call_id.is_some() {
            return Err(ProviderError::Protocol(
                "invalid tool metadata on Anthropic system message".into(),
            ));
        }
        system.push(AnthropicSystemBlock {
            block_type: "text",
            text: anthropic_text_content(message)?.to_string(),
        });
        cursor += 1;
    }

    let mut messages = Vec::new();
    while let Some(message) = request.messages.get(cursor) {
        match message.role.as_str() {
            "system" | "developer" => {
                return Err(ProviderError::Protocol(
                    "Anthropic system messages must precede conversation messages".into(),
                ));
            }
            "user" => {
                if message.tool_calls.is_some() || message.tool_call_id.is_some() {
                    return Err(ProviderError::Protocol(
                        "invalid tool metadata on Anthropic user message".into(),
                    ));
                }
                push_anthropic_message(&mut messages, "user", anthropic_user_content(message)?);
                cursor += 1;
            }
            "assistant" => {
                if message.tool_call_id.is_some() {
                    return Err(ProviderError::Protocol(
                        "invalid tool result identifier on Anthropic assistant message".into(),
                    ));
                }
                let text = anthropic_text_content(message)?;
                let calls = message.tool_calls.as_deref().unwrap_or_default();
                let mut content = Vec::new();
                if !text.is_empty() || calls.is_empty() {
                    content.push(AnthropicContent::Text {
                        text: text.to_string(),
                    });
                }
                for call in calls {
                    if call.call_type != "function" {
                        return Err(ProviderError::Protocol(
                            "Anthropic Messages supports function tool calls only".into(),
                        ));
                    }
                    let input: serde_json::Value = serde_json::from_str(&call.function.arguments)
                        .map_err(|_| {
                        ProviderError::Protocol(
                            "Anthropic tool-call arguments were not valid JSON".into(),
                        )
                    })?;
                    if !input.is_object() {
                        return Err(ProviderError::Protocol(
                            "Anthropic tool-call arguments must be a JSON object".into(),
                        ));
                    }
                    content.push(AnthropicContent::ToolUse {
                        id: call.id.clone(),
                        name: call.function.name.clone(),
                        input,
                    });
                }
                push_anthropic_message(&mut messages, "assistant", content);
                cursor += 1;
            }
            "tool" => {
                let mut content = Vec::new();
                while let Some(tool) = request
                    .messages
                    .get(cursor)
                    .filter(|message| message.role == "tool")
                {
                    if tool.tool_calls.is_some() {
                        return Err(ProviderError::Protocol(
                            "invalid nested tool call on Anthropic tool result".into(),
                        ));
                    }
                    let tool_use_id = tool.tool_call_id.clone().ok_or_else(|| {
                        ProviderError::Protocol("Anthropic tool result is missing call_id".into())
                    })?;
                    content.push(AnthropicContent::ToolResult {
                        tool_use_id,
                        content: anthropic_text_content(tool)?.to_string(),
                    });
                    cursor += 1;
                }
                push_anthropic_message(&mut messages, "user", content);
            }
            _ => {
                return Err(ProviderError::Protocol(
                    "unsupported Anthropic input message role".into(),
                ));
            }
        }
    }

    let tools = if request.tools.is_empty() {
        None
    } else {
        Some(
            request
                .tools
                .iter()
                .map(|tool| {
                    if tool.def_type != "function" {
                        return Err(ProviderError::Protocol(
                            "Anthropic Messages supports function tools only".into(),
                        ));
                    }
                    Ok(AnthropicTool {
                        name: tool.function.name.clone(),
                        description: tool.function.description.clone(),
                        input_schema: tool.function.parameters.clone(),
                    })
                })
                .collect::<Result<Vec<_>, ProviderError>>()?,
        )
    };
    let output_config = request
        .options
        .reasoning_effort
        .map(|effort| AnthropicOutputConfig {
            effort: match effort {
                ReasoningEffort::None | ReasoningEffort::Minimal | ReasoningEffort::Low => "low",
                ReasoningEffort::Medium => "medium",
                ReasoningEffort::High => "high",
                ReasoningEffort::XHigh | ReasoningEffort::Max => "max",
            },
        });
    Ok(AnthropicMessagesRequest {
        model: request.model.clone(),
        max_tokens: ANTHROPIC_MAX_TOKENS,
        messages,
        system: (!system.is_empty()).then_some(system),
        tools,
        stream: true,
        output_config,
    })
}

fn anthropic_user_content(message: &ChatMessage) -> Result<Vec<AnthropicContent>, ProviderError> {
    if let Some(text) = message.content.as_str() {
        return Ok(vec![AnthropicContent::Text {
            text: text.to_string(),
        }]);
    }
    Ok(message
        .content
        .blocks()
        .iter()
        .map(|block| match block {
            crate::image_content::ModelContentBlock::Text(text) => {
                AnthropicContent::Text { text: text.clone() }
            }
            crate::image_content::ModelContentBlock::Image(image) => AnthropicContent::Image {
                source: AnthropicImageSource {
                    source_type: "base64",
                    media_type: image.media_type(),
                    data: image.base64(),
                },
            },
        })
        .collect())
}

fn anthropic_text_content(message: &ChatMessage) -> Result<&str, ProviderError> {
    message.content.as_str().ok_or_else(|| {
        ProviderError::Protocol("anthropic_messages does not support non-text content".into())
    })
}

fn push_anthropic_message(
    messages: &mut Vec<AnthropicMessage>,
    role: &str,
    mut content: Vec<AnthropicContent>,
) {
    if let Some(last) = messages.last_mut().filter(|message| message.role == role) {
        last.content.append(&mut content);
    } else {
        messages.push(AnthropicMessage {
            role: role.to_string(),
            content,
        });
    }
}

fn serialize_anthropic_messages(
    request: &crate::request::PreparedModelRequest,
) -> Result<Vec<u8>, ProviderError> {
    serde_json::to_vec(&anthropic_messages_request(request)?).map_err(|_| {
        ProviderError::Protocol("could not serialize the prepared provider request".into())
    })
}

#[cfg(test)]
pub(crate) fn anthropic_messages_bytes_for_test(
    request: &crate::request::PreparedModelRequest,
) -> Result<Vec<u8>, ProviderError> {
    serialize_anthropic_messages(request)
}

#[cfg(test)]
pub(crate) fn chat_completions_json_for_test(
    request: &crate::request::PreparedModelRequest,
) -> serde_json::Value {
    let source = test_state_source_for_request(request, ProviderProtocol::ChatCompletions);
    serde_json::to_value(
        chat_completions_request(
            request,
            &source,
            test_contract_for_request(request, ProviderProtocol::ChatCompletions),
        )
        .unwrap(),
    )
    .unwrap()
}

#[cfg(test)]
pub(crate) fn chat_completions_bytes_for_test(
    request: &crate::request::PreparedModelRequest,
) -> Vec<u8> {
    let source = test_state_source_for_request(request, ProviderProtocol::ChatCompletions);
    serialize_chat_completions(
        request,
        &source,
        test_contract_for_request(request, ProviderProtocol::ChatCompletions),
    )
    .unwrap()
}

#[cfg(test)]
fn test_contract_for_request(
    request: &crate::request::PreparedModelRequest,
    protocol: ProviderProtocol,
) -> Option<crate::provider_contract::FirstPartyContract> {
    let base_url = match request.provider.as_str() {
        "openai" => "https://api.openai.com/v1",
        "openai-codex" => "https://chatgpt.com/backend-api/codex",
        "mimo" => "https://api.xiaomimimo.com/v1",
        "deepseek" => "https://api.deepseek.com/v1",
        _ => return None,
    };
    let provider = crate::config::ProviderConfig {
        base_url: base_url.into(),
        api_key: String::new(),
        protocol,
        prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
        models: vec![request.model.clone()],
        timeout: crate::config::ProviderTimeoutConfig::default(),
    };
    crate::provider_contract::resolve(&request.provider, &provider, &request.model)
}

#[cfg(test)]
fn test_state_source_for_request(
    request: &crate::request::PreparedModelRequest,
    protocol: ProviderProtocol,
) -> ProviderStateSourceV1 {
    ProviderStateSourceV1::for_test(request.provider.clone(), protocol)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_default_and_wire_name_are_stable() {
        #[derive(Deserialize)]
        struct Wrapper {
            protocol: ProviderProtocol,
        }

        assert_eq!(
            ProviderProtocol::default(),
            ProviderProtocol::ChatCompletions
        );
        assert_eq!(ProviderProtocol::default().label(), "chat_completions");
        assert_eq!(
            toml::from_str::<Wrapper>("protocol = \"chat_completions\"")
                .unwrap()
                .protocol,
            ProviderProtocol::ChatCompletions
        );
        assert_eq!(
            toml::from_str::<Wrapper>("protocol = \"openai_responses\"")
                .unwrap()
                .protocol,
            ProviderProtocol::OpenAiResponses
        );
        assert_eq!(
            toml::from_str::<Wrapper>("protocol = \"anthropic_messages\"")
                .unwrap()
                .protocol,
            ProviderProtocol::AnthropicMessages
        );
        assert!(toml::from_str::<Wrapper>("protocol = \"responses\"").is_err());
    }

    #[test]
    fn chat_completions_request_maps_typed_image_content() {
        let image = crate::image_content::normalize_rgba(1, 1, vec![10, 20, 30, 255]).unwrap();
        let request = crate::request::PreparedModelRequest::new(
            crate::request::RequestKind::MainTurn,
            "local",
            "vision-model",
            vec![ChatMessage {
                role: "user".into(),
                content: crate::image_content::ModelContent::multimodal("inspect", &[image]),
                tool_calls: None,
                tool_call_id: None,
                response_items: Vec::new(),
            }],
            Vec::new(),
            crate::agent::client::ChatRequestOptions::default(),
        );
        let value: serde_json::Value =
            serde_json::from_slice(&chat_completions_bytes_for_test(&request)).unwrap();
        assert_eq!(value["messages"][0]["content"][0]["type"], "text");
        assert_eq!(value["messages"][0]["content"][0]["text"], "inspect");
        assert_eq!(value["messages"][0]["content"][1]["type"], "image_url");
        assert!(value["messages"][0]["content"][1]["image_url"]["url"]
            .as_str()
            .is_some_and(|url| url.starts_with("data:image/png;base64,")));
    }

    #[test]
    fn anthropic_request_maps_system_tools_results_and_effort_exactly() {
        let calls = vec![
            crate::agent::client::ToolCallMsg {
                id: "call-1".into(),
                call_type: "function".into(),
                function: crate::agent::client::FunctionCall {
                    name: "read_file".into(),
                    arguments: "{\"path\":\"a\"}".into(),
                },
            },
            crate::agent::client::ToolCallMsg {
                id: "call-2".into(),
                call_type: "function".into(),
                function: crate::agent::client::FunctionCall {
                    name: "read_file".into(),
                    arguments: "{\"path\":\"b\"}".into(),
                },
            },
        ];
        let request = crate::request::PreparedModelRequest::new(
            crate::request::RequestKind::MainTurn,
            "anthropic",
            "claude-test",
            vec![
                ChatMessage {
                    role: "system".into(),
                    content: crate::image_content::ModelContent::text("rules"),
                    tool_calls: None,
                    tool_call_id: None,
                    response_items: Vec::new(),
                },
                ChatMessage {
                    role: "user".into(),
                    content: crate::image_content::ModelContent::text("read"),
                    tool_calls: None,
                    tool_call_id: None,
                    response_items: Vec::new(),
                },
                ChatMessage {
                    role: "assistant".into(),
                    content: crate::image_content::ModelContent::text("checking"),
                    tool_calls: Some(calls),
                    tool_call_id: None,
                    response_items: Vec::new(),
                },
                ChatMessage {
                    role: "tool".into(),
                    content: crate::image_content::ModelContent::text("A"),
                    tool_calls: None,
                    tool_call_id: Some("call-1".into()),
                    response_items: Vec::new(),
                },
                ChatMessage {
                    role: "tool".into(),
                    content: crate::image_content::ModelContent::text("B"),
                    tool_calls: None,
                    tool_call_id: Some("call-2".into()),
                    response_items: Vec::new(),
                },
            ],
            vec![ToolDef {
                def_type: "function".into(),
                function: crate::agent::client::ToolFunction {
                    name: "read_file".into(),
                    description: "Read a file".into(),
                    parameters: serde_json::json!({"type":"object"}),
                },
            }],
            crate::agent::client::ChatRequestOptions {
                reasoning_effort: Some(ReasoningEffort::Minimal),
                reasoning_request_format: None,
                reasoning_stream_field: None,
                tool_call_format: None,
            },
        );

        assert_eq!(
            anthropic_messages_bytes_for_test(&request).unwrap(),
            br#"{"model":"claude-test","max_tokens":16384,"messages":[{"role":"user","content":[{"type":"text","text":"read"}]},{"role":"assistant","content":[{"type":"text","text":"checking"},{"type":"tool_use","id":"call-1","name":"read_file","input":{"path":"a"}},{"type":"tool_use","id":"call-2","name":"read_file","input":{"path":"b"}}]},{"role":"user","content":[{"type":"tool_result","tool_use_id":"call-1","content":"A"},{"type":"tool_result","tool_use_id":"call-2","content":"B"}]}],"system":[{"type":"text","text":"rules"}],"tools":[{"name":"read_file","description":"Read a file","input_schema":{"type":"object"}}],"stream":true,"output_config":{"effort":"low"}}"#
        );
    }

    #[test]
    fn anthropic_request_maps_images_and_rejects_non_object_tool_input() {
        let image = crate::image_content::normalize_rgba(1, 1, vec![1, 2, 3, 255]).unwrap();
        let request = crate::request::PreparedModelRequest::new(
            crate::request::RequestKind::MainTurn,
            "anthropic",
            "claude-test",
            vec![ChatMessage {
                role: "user".into(),
                content: crate::image_content::ModelContent::multimodal("inspect", &[image]),
                tool_calls: None,
                tool_call_id: None,
                response_items: Vec::new(),
            }],
            Vec::new(),
            crate::agent::client::ChatRequestOptions::default(),
        );
        let bytes = anthropic_messages_bytes_for_test(&request).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["messages"][0]["content"][0]["text"], "inspect");
        assert_eq!(
            value["messages"][0]["content"][1]["source"]["media_type"],
            "image/png"
        );
        assert!(value["messages"][0]["content"][1]["source"]["data"]
            .as_str()
            .is_some_and(|data| !data.is_empty()));

        let request = crate::request::PreparedModelRequest::new(
            crate::request::RequestKind::ToolContinuation,
            "anthropic",
            "claude-test",
            vec![ChatMessage {
                role: "assistant".into(),
                content: crate::image_content::ModelContent::text(String::new()),
                tool_calls: Some(vec![crate::agent::client::ToolCallMsg {
                    id: "call".into(),
                    call_type: "function".into(),
                    function: crate::agent::client::FunctionCall {
                        name: "tool".into(),
                        arguments: "[]".into(),
                    },
                }]),
                tool_call_id: None,
                response_items: Vec::new(),
            }],
            Vec::new(),
            crate::agent::client::ChatRequestOptions::default(),
        );
        assert!(matches!(
            anthropic_messages_bytes_for_test(&request),
            Err(ProviderError::Protocol(_))
        ));
    }

    #[test]
    fn anthropic_decoder_maps_lifecycle_reasoning_text_tools_and_cached_usage() {
        let input = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg-secret\",\"content\":[],\"usage\":{\"input_tokens\":10,\"output_tokens\":1,\"cache_creation_input_tokens\":2,\"cache_read_input_tokens\":3}}}\n\n",
            "event: ping\ndata: {\"type\":\"ping\"}\n\n",
            "event: future_event\ndata: {\"type\":\"future_event\",\"payload\":\"ignored\"}\n\n",
            "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\",\"signature\":\"\"}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"why\"}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"signature_delta\",\"signature\":\"secret-signature\"}}\n\n",
            "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"text_delta\",\"text\":\"answer\"}}\n\n",
            "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":1}\n\n",
            "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":2,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu-1\",\"name\":\"read_file\",\"input\":{}}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":2,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"path\\\":\"}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":2,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"\\\"a\\\"}\"}}\n\n",
            "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":2}\n\n",
            "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"output_tokens\":8}}\n\n",
            "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n"
        );
        let mut decoder = AnthropicMessagesDecoder::new();
        let mut events = Vec::new();
        for byte in input.as_bytes() {
            events.extend(decoder.push(&[*byte]).unwrap());
        }
        events.extend(decoder.finish().unwrap());

        assert_eq!(
            events,
            vec![
                ProviderEvent::Reasoning("why".into()),
                ProviderEvent::Text("answer".into()),
                ProviderEvent::ToolCallStart {
                    index: 2,
                    id: "toolu-1".into(),
                    name: "read_file".into(),
                },
                ProviderEvent::ToolCallArg {
                    index: 2,
                    arg: "{\"path\":".into(),
                },
                ProviderEvent::ToolCallArg {
                    index: 2,
                    arg: "\"a\"}".into(),
                },
                ProviderEvent::Finish {
                    reason: FinishReason::ToolCalls,
                    usage: Some(UsageInfo {
                        prompt_tokens: 15,
                        completion_tokens: 8,
                        total_tokens: 23,
                        cached_prompt_tokens: Some(3),
                        cache_write_tokens: Some(2),
                        reasoning_tokens: None,
                        response_metadata: None,
                        cache_usage_override: None,
                    }),
                },
                ProviderEvent::Done,
            ]
        );
        let usage = events
            .iter()
            .find_map(|event| match event {
                ProviderEvent::Finish {
                    usage: Some(usage), ..
                } => Some(usage),
                _ => None,
            })
            .unwrap();
        assert_eq!(
            usage.cache_usage(),
            crate::agent::stream::CacheUsage::new(
                crate::agent::stream::CacheCount::Reported(3),
                crate::agent::stream::CacheCount::Reported(2),
                crate::agent::stream::CacheCount::Derived(12),
            )
        );
    }

    #[test]
    fn anthropic_decoder_has_safe_errors_and_bounded_known_events() {
        for event in [
            r#"{"type":"error","error":{"message":"sentinel-provider-body"}}"#,
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"refusal_delta","refusal":"sentinel-provider-body"}}"#,
        ] {
            let mut decoder = AnthropicMessagesDecoder::new();
            let error = if event.contains("content_block_delta") {
                decoder
                    .push(b"data: {\"type\":\"message_start\",\"message\":{\"content\":[],\"usage\":{\"input_tokens\":1}}}\n\n")
                    .unwrap();
                decoder
                    .push(b"data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n")
                    .unwrap();
                decoder
                    .push(format!("data: {event}\n\n").as_bytes())
                    .unwrap_err()
            } else {
                decoder
                    .push(format!("data: {event}\n\n").as_bytes())
                    .unwrap_err()
            };
            assert!(matches!(
                error,
                ProviderError::ResponseFailed | ProviderError::ResponseRefused
            ));
            assert!(!error.to_string().contains("sentinel-provider-body"));
        }

        let mut oversized = AnthropicMessagesDecoder::with_event_limit(8);
        assert_eq!(
            oversized.push(b"data: 123456789"),
            Err(ProviderError::EventTooLarge { limit: 8 })
        );
    }

    #[test]
    fn anthropic_decoder_preserves_multiple_tool_blocks_and_initial_input() {
        let input = concat!(
            "data: {\"type\":\"message_start\",\"message\":{\"content\":[],\"usage\":{\"input_tokens\":1}}}\n\n",
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"one\",\"name\":\"first\",\"input\":{\"value\":1}}}\n\n",
            "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"tool_use\",\"id\":\"two\",\"name\":\"second\",\"input\":{}}}\n\n",
            "data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"value\\\":2}\"}}\n\n",
            "data: {\"type\":\"content_block_stop\",\"index\":1}\n\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"output_tokens\":2}}\n\n",
            "data: {\"type\":\"message_stop\"}\n\n"
        );
        let mut decoder = AnthropicMessagesDecoder::new();
        let mut assembler = crate::agent::stream::AssistantTurnAssembler::new();
        for byte in input.as_bytes() {
            for event in decoder.push(&[*byte]).unwrap() {
                assembler.push(&event).unwrap();
            }
        }
        for event in decoder.finish().unwrap() {
            assembler.push(&event).unwrap();
        }
        let turn = assembler.finish().unwrap();

        assert_eq!(turn.tool_calls.len(), 2);
        assert_eq!(turn.tool_calls[0].id, "one");
        assert_eq!(turn.tool_calls[0].arguments, "{\"value\":1}");
        assert_eq!(turn.tool_calls[1].id, "two");
        assert_eq!(turn.tool_calls[1].arguments, "{\"value\":2}");
    }

    #[test]
    fn anthropic_decoder_rejects_malformed_mismatched_and_premature_lifecycle() {
        let mut malformed = AnthropicMessagesDecoder::new();
        assert!(matches!(
            malformed.push(b"data: {bad}\n\n"),
            Err(ProviderError::MalformedData(_))
        ));

        let mut mismatched = AnthropicMessagesDecoder::new();
        mismatched
            .push(b"data: {\"type\":\"message_start\",\"message\":{\"content\":[],\"usage\":{\"input_tokens\":1}}}\n\n")
            .unwrap();
        mismatched
            .push(b"data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n")
            .unwrap();
        assert!(matches!(
            mismatched.push(b"data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{}\"}}\n\n"),
            Err(ProviderError::Protocol(_))
        ));

        let mut premature = AnthropicMessagesDecoder::new();
        premature
            .push(b"data: {\"type\":\"message_start\",\"message\":{\"content\":[],\"usage\":{\"input_tokens\":1}}}\n\n")
            .unwrap();
        assert_eq!(
            premature.push(b"data: {\"type\":\"message_stop\"}\n\n"),
            Err(ProviderError::PrematureEof)
        );
    }

    #[test]
    fn responses_request_maps_text_tools_reasoning_and_tool_replay_exactly() {
        let request = crate::request::PreparedModelRequest::new(
            crate::request::RequestKind::MainTurn,
            "openai",
            "gpt-5.6-sol",
            vec![
                ChatMessage {
                    role: "system".into(),
                    content: crate::image_content::ModelContent::text("rules"),
                    tool_calls: None,
                    tool_call_id: None,
                    response_items: Vec::new(),
                },
                ChatMessage {
                    role: "user".into(),
                    content: crate::image_content::ModelContent::text("read it"),
                    tool_calls: None,
                    tool_call_id: None,
                    response_items: Vec::new(),
                },
                ChatMessage {
                    role: "assistant".into(),
                    content: crate::image_content::ModelContent::text(String::new()),
                    tool_calls: Some(vec![crate::agent::client::ToolCallMsg {
                        id: "call-1".into(),
                        call_type: "function".into(),
                        function: crate::agent::client::FunctionCall {
                            name: "read_file".into(),
                            arguments: "{\"path\":\"input.txt\"}".into(),
                        },
                    }]),
                    tool_call_id: None,
                    response_items: Vec::new(),
                },
                ChatMessage {
                    role: "tool".into(),
                    content: crate::image_content::ModelContent::text("hello"),
                    tool_calls: None,
                    tool_call_id: Some("call-1".into()),
                    response_items: Vec::new(),
                },
            ],
            vec![ToolDef {
                def_type: "function".into(),
                function: crate::agent::client::ToolFunction {
                    name: "read_file".into(),
                    description: "Read a file".into(),
                    parameters: serde_json::json!({"type":"object"}),
                },
            }],
            crate::agent::client::ChatRequestOptions {
                reasoning_effort: Some(ReasoningEffort::High),
                reasoning_request_format: None,
                reasoning_stream_field: None,
                tool_call_format: None,
            },
        );

        assert_eq!(
            openai_responses_bytes_for_test(&request).unwrap(),
            br#"{"model":"gpt-5.6-sol","input":[{"role":"system","content":"rules"},{"role":"user","content":"read it"},{"type":"function_call","call_id":"call-1","name":"read_file","arguments":"{\"path\":\"input.txt\"}"},{"type":"function_call_output","call_id":"call-1","output":"hello"}],"tools":[{"type":"function","name":"read_file","description":"Read a file","parameters":{"type":"object"},"strict":false}],"stream":true,"reasoning":{"effort":"high"},"include":["reasoning.encrypted_content"],"store":false}"#
        );
        let capped = OpenAiResponsesRequestOptions::capped(Some(
            crate::agent::client::OpenAiServiceTier::Default,
        ));
        assert_eq!(
            openai_responses_bytes_with_options_for_test(&request, &capped).unwrap(),
            br#"{"model":"gpt-5.6-sol","input":[{"role":"system","content":"rules"},{"role":"user","content":"read it"},{"type":"function_call","call_id":"call-1","name":"read_file","arguments":"{\"path\":\"input.txt\"}"},{"type":"function_call_output","call_id":"call-1","output":"hello"}],"tools":[{"type":"function","name":"read_file","description":"Read a file","parameters":{"type":"object"},"strict":false}],"stream":true,"reasoning":{"effort":"high"},"include":["reasoning.encrypted_content"],"max_output_tokens":8192,"store":false,"service_tier":"default"}"#
        );
    }

    #[test]
    fn returned_request_id_is_hashed_and_duplicate_or_oversized_headers_fail_closed() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            "x-request-id",
            reqwest::header::HeaderValue::from_static("req-secret"),
        );
        let digest = provider_request_id_sha256(&headers).unwrap().unwrap();
        assert_eq!(digest.len(), 64);
        assert!(!digest.contains("req-secret"));

        let mut codex_headers = reqwest::header::HeaderMap::new();
        codex_headers.insert(
            "x-oai-request-id",
            reqwest::header::HeaderValue::from_static("codex-req-secret"),
        );
        assert_eq!(
            provider_request_id_sha256(&codex_headers)
                .unwrap()
                .unwrap()
                .len(),
            64
        );
        codex_headers.insert(
            "x-request-id",
            reqwest::header::HeaderValue::from_static("conflict"),
        );
        assert!(provider_request_id_sha256(&codex_headers).is_err());

        headers.append(
            "x-request-id",
            reqwest::header::HeaderValue::from_static("req-second"),
        );
        assert!(provider_request_id_sha256(&headers).is_err());

        let mut oversized = reqwest::header::HeaderMap::new();
        oversized.insert(
            "x-request-id",
            reqwest::header::HeaderValue::try_from("x".repeat(257)).unwrap(),
        );
        assert!(provider_request_id_sha256(&oversized).is_err());
    }

    #[test]
    fn responses_request_replays_full_output_sequence_before_tool_continuation() {
        let reasoning_item = serde_json::json!({
            "id": "rs_123",
            "type": "reasoning",
            "summary": [{"type": "summary_text", "text": "kept summary"}],
            "encrypted_content": "encrypted-state",
            "status": "completed"
        });
        let earlier_reasoning_item = serde_json::json!({
            "id": "rs_earlier",
            "type": "reasoning",
            "summary": [],
            "encrypted_content": "earlier-encrypted-state",
            "status": "completed"
        });
        let function_call_item = serde_json::json!({
            "id": "fc_123",
            "type": "function_call",
            "call_id": "call-1",
            "name": "read_file",
            "arguments": "{\"path\":\"input.txt\"}",
            "status": "completed"
        });
        let message_item = serde_json::json!({
            "id": "msg_123",
            "type": "message",
            "role": "assistant",
            "content": [{
                "type": "output_text",
                "text": "I will read it.",
                "annotations": []
            }],
            "status": "completed"
        });
        let request = crate::request::PreparedModelRequest::new(
            crate::request::RequestKind::ToolContinuation,
            "openai",
            "gpt-5.6-sol",
            vec![
                ChatMessage {
                    role: "assistant".into(),
                    content: crate::image_content::ModelContent::text("I will read it."),
                    tool_calls: Some(vec![crate::agent::client::ToolCallMsg {
                        id: "call-1".into(),
                        call_type: "function".into(),
                        function: crate::agent::client::FunctionCall {
                            name: "read_file".into(),
                            arguments: "{\"path\":\"input.txt\"}".into(),
                        },
                    }]),
                    tool_call_id: None,
                    response_items: vec![
                        ProviderResponseItem::ResponsesOutput {
                            source: ProviderStateSourceV1::for_test(
                                "other-provider",
                                ProviderProtocol::OpenAiResponses,
                            ),
                            output_index: 0,
                            item: serde_json::json!({
                                "id": "rs_other",
                                "type": "reasoning",
                                "encrypted_content": "must-not-cross-providers"
                            }),
                        },
                        ProviderResponseItem::ResponsesOutput {
                            source: ProviderStateSourceV1::for_test(
                                "openai",
                                ProviderProtocol::OpenAiResponses,
                            ),
                            output_index: 7,
                            item: reasoning_item.clone(),
                        },
                        ProviderResponseItem::ResponsesOutput {
                            source: ProviderStateSourceV1::for_test(
                                "openai",
                                ProviderProtocol::OpenAiResponses,
                            ),
                            output_index: 2,
                            item: earlier_reasoning_item.clone(),
                        },
                        ProviderResponseItem::ResponsesOutput {
                            source: ProviderStateSourceV1::for_test(
                                "openai",
                                ProviderProtocol::OpenAiResponses,
                            ),
                            output_index: 9,
                            item: function_call_item.clone(),
                        },
                        ProviderResponseItem::ResponsesOutput {
                            source: ProviderStateSourceV1::for_test(
                                "openai",
                                ProviderProtocol::OpenAiResponses,
                            ),
                            output_index: 8,
                            item: message_item.clone(),
                        },
                    ],
                },
                ChatMessage {
                    role: "tool".into(),
                    content: crate::image_content::ModelContent::text("hello"),
                    tool_calls: None,
                    tool_call_id: Some("call-1".into()),
                    response_items: Vec::new(),
                },
            ],
            Vec::new(),
            crate::agent::client::ChatRequestOptions::default(),
        );

        let bytes = openai_responses_bytes_for_test(&request).unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            body["input"],
            serde_json::json!([
                earlier_reasoning_item,
                reasoning_item,
                message_item,
                function_call_item,
                {
                    "type": "function_call_output",
                    "call_id": "call-1",
                    "output": "hello"
                }
            ])
        );
        assert_eq!(
            body["include"],
            serde_json::json!(["reasoning.encrypted_content"])
        );
        assert!(!String::from_utf8_lossy(&bytes).contains("must-not-cross-providers"));
    }

    #[test]
    fn responses_replay_rejects_duplicate_indexes_and_excess_item_count() {
        let reasoning = |output_index: usize, id: String| ProviderResponseItem::ResponsesOutput {
            source: ProviderStateSourceV1::for_test("openai", ProviderProtocol::OpenAiResponses),
            output_index,
            item: serde_json::json!({
                "id": id,
                "type": "reasoning",
                "summary": [],
                "encrypted_content": "encrypted-state",
                "status": "completed"
            }),
        };
        let mut message = ChatMessage {
            role: "assistant".into(),
            content: crate::image_content::ModelContent::text(String::new()),
            tool_calls: None,
            tool_call_id: None,
            response_items: vec![reasoning(4, "rs_a".into()), reasoning(4, "rs_b".into())],
        };
        assert!(matches!(
            response_output_items_for_replay(&message, &ProviderStateSourceV1::for_test("openai", ProviderProtocol::OpenAiResponses)),
            Err(ProviderError::Protocol(message))
                if message.contains("duplicate response output index")
        ));

        message.response_items = (0..=MAX_RESPONSE_OUTPUT_ITEMS)
            .map(|index| reasoning(index, format!("rs_{index}")))
            .collect();
        assert!(matches!(
            response_output_items_for_replay(&message, &ProviderStateSourceV1::for_test("openai", ProviderProtocol::OpenAiResponses)),
            Err(ProviderError::Protocol(message))
                if message.contains("too many response output items")
        ));
    }

    #[test]
    fn responses_replay_rejects_output_that_disagrees_with_canonical_history() {
        let message = ChatMessage {
            role: "assistant".into(),
            content: crate::image_content::ModelContent::text("canonical text"),
            tool_calls: None,
            tool_call_id: None,
            response_items: vec![ProviderResponseItem::ResponsesOutput {
                source: ProviderStateSourceV1::for_test(
                    "openai",
                    ProviderProtocol::OpenAiResponses,
                ),
                output_index: 0,
                item: serde_json::json!({
                    "id": "msg_1",
                    "type": "message",
                    "role": "assistant",
                    "content": [{
                        "type": "output_text",
                        "text": "different provider text",
                        "annotations": []
                    }],
                    "status": "completed"
                }),
            }],
        };

        assert!(matches!(
            response_output_items_for_replay(&message, &ProviderStateSourceV1::for_test("openai", ProviderProtocol::OpenAiResponses)),
            Err(ProviderError::Protocol(message))
                if message.contains("canonical assistant text")
        ));
        assert_eq!(
            response_output_items_for_replay(
                &message,
                &ProviderStateSourceV1::for_test(
                    "another-provider",
                    ProviderProtocol::OpenAiResponses
                )
            )
            .unwrap(),
            None,
            "foreign route state must be ignored so ordinary synthesis remains available"
        );
    }

    #[test]
    fn responses_reasoning_state_is_typed_depth_and_aggregate_byte_bounded() {
        assert_eq!(
            validate_response_output_item(&serde_json::json!({
                "id": "rs_without_status",
                "type": "reasoning",
                "summary": []
            }))
            .unwrap(),
            ResponsesOutputKind::Reasoning,
            "status is optional on completed response output item parameters"
        );

        for malformed in [
            serde_json::json!({
                "id": "rs_bad_summary",
                "type": "reasoning",
                "status": "completed",
                "summary": [{"type": "summary_text", "text": 7}]
            }),
            serde_json::json!({
                "id": "rs_bad_encrypted",
                "type": "reasoning",
                "status": "completed",
                "encrypted_content": {"unexpected": true}
            }),
            serde_json::json!({
                "id": "rs_bad_status",
                "type": "reasoning",
                "status": "incomplete"
            }),
        ] {
            assert!(matches!(
                validate_response_output_item(&malformed),
                Err(ProviderError::MalformedData(_))
            ));
        }

        let mut nested = serde_json::json!("leaf");
        for _ in 0..MAX_RESPONSE_OUTPUT_DEPTH {
            nested = serde_json::json!([nested]);
        }
        let too_deep = serde_json::json!({
            "id": "rs_deep",
            "type": "reasoning",
            "status": "completed",
            "extra": nested
        });
        assert!(matches!(
            validate_response_output_item(&too_deep),
            Err(ProviderError::Protocol(message))
                if message.contains("JSON depth limit")
        ));

        let mut decoder = OpenAiResponsesDecoder::for_source(
            ProviderStateSourceV1::for_test("openai", ProviderProtocol::OpenAiResponses),
            None,
        );
        let payload = "x".repeat(MAX_RESPONSE_OUTPUT_BYTES / MAX_RESPONSE_OUTPUT_ITEMS + 1);
        let mut aggregate_error = None;
        for output_index in 0..MAX_RESPONSE_OUTPUT_ITEMS {
            let item = serde_json::json!({
                "id": format!("rs_{output_index}"),
                "type": "reasoning",
                "summary": [],
                "encrypted_content": payload.clone(),
                "status": "completed"
            });
            if let Err(error) = decoder.record_response_output_item(output_index, &item) {
                aggregate_error = Some(error);
                break;
            }
        }
        assert_eq!(
            aggregate_error,
            Some(ProviderError::EventTooLarge {
                limit: MAX_RESPONSE_OUTPUT_BYTES
            })
        );
    }

    #[test]
    fn responses_request_maps_typed_image_content() {
        let image = crate::image_content::normalize_rgba(1, 1, vec![4, 5, 6, 255]).unwrap();
        let request = crate::request::PreparedModelRequest::new(
            crate::request::RequestKind::MainTurn,
            "openai",
            "gpt-5.6-sol",
            vec![ChatMessage {
                role: "user".into(),
                content: crate::image_content::ModelContent::multimodal("inspect", &[image]),
                tool_calls: None,
                tool_call_id: None,
                response_items: Vec::new(),
            }],
            Vec::new(),
            crate::agent::client::ChatRequestOptions::default(),
        );

        let bytes = openai_responses_bytes_for_test(&request).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["input"][0]["content"][0]["type"], "input_text");
        assert_eq!(value["input"][0]["content"][0]["text"], "inspect");
        assert!(value["input"][0]["content"][1]["image_url"]
            .as_str()
            .is_some_and(|url| url.starts_with("data:image/png;base64,")));
    }

    #[test]
    fn responses_decoder_maps_text_reasoning_tools_usage_finish_and_done() {
        let reasoning_item = serde_json::json!({
            "id": "rs_1",
            "type": "reasoning",
            "summary": [{"type": "summary_text", "text": "why"}],
            "encrypted_content": "encrypted-state",
            "status": "completed"
        });
        let message_item = serde_json::json!({
            "id": "msg_1",
            "type": "message",
            "role": "assistant",
            "content": [{"type": "output_text", "text": "answer", "annotations": []}],
            "status": "completed"
        });
        let function_call_item = serde_json::json!({
            "id": "fc_1",
            "type": "function_call",
            "call_id": "call-1",
            "name": "read_file",
            "arguments": "{\"path\":\"input.txt\"}",
            "status": "completed"
        });
        let input = concat!(
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp-secret\",\"model\":\"gpt-5.6-sol\",\"service_tier\":\"default\"}}\n\n",
            "data: {\"type\":\"response.reasoning_summary_text.delta\",\"output_index\":0,\"summary_index\":0,\"delta\":\"why\"}\n\n",
            "data: {\"type\":\"response.output_text.delta\",\"output_index\":1,\"content_index\":0,\"delta\":\"answer\"}\n\n",
            "data: {\"type\":\"response.output_text.done\",\"output_index\":1,\"content_index\":0,\"text\":\"answer\"}\n\n",
            "data: {\"type\":\"response.output_item.added\",\"output_index\":2,\"item\":{\"type\":\"function_call\",\"call_id\":\"call-1\",\"name\":\"read_file\",\"arguments\":\"\"}}\n\n",
            "data: {\"type\":\"response.function_call_arguments.delta\",\"output_index\":2,\"delta\":\"{\\\"path\\\":\"}\n\n",
            "data: {\"type\":\"response.function_call_arguments.done\",\"output_index\":2,\"arguments\":\"{\\\"path\\\":\\\"input.txt\\\"}\"}\n\n",
            "data: {\"type\":\"response.output_item.done\",\"output_index\":2,\"item\":{\"id\":\"fc_1\",\"type\":\"function_call\",\"call_id\":\"call-1\",\"name\":\"read_file\",\"arguments\":\"{\\\"path\\\":\\\"input.txt\\\"}\",\"status\":\"completed\"}}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-secret\",\"model\":\"gpt-5.6-sol\",\"service_tier\":\"default\",\"status\":\"completed\",\"output\":[{\"id\":\"rs_1\",\"type\":\"reasoning\",\"summary\":[{\"type\":\"summary_text\",\"text\":\"why\"}],\"encrypted_content\":\"encrypted-state\",\"status\":\"completed\"},{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"answer\",\"annotations\":[]}],\"status\":\"completed\"},{\"id\":\"fc_1\",\"type\":\"function_call\",\"call_id\":\"call-1\",\"name\":\"read_file\",\"arguments\":\"{\\\"path\\\":\\\"input.txt\\\"}\",\"status\":\"completed\"}],\"usage\":{\"input_tokens\":30,\"output_tokens\":20,\"total_tokens\":50,\"input_tokens_details\":{\"cached_tokens\":12,\"cache_write_tokens\":4},\"output_tokens_details\":{\"reasoning_tokens\":7}}}}\n\n"
        );
        let mut decoder = OpenAiResponsesDecoder::with_request_id_for_test("req-secret");
        let mut events = Vec::new();
        for byte in input.as_bytes() {
            events.extend(decoder.push(&[*byte]).unwrap());
        }
        events.extend(decoder.finish().unwrap());

        assert_eq!(
            events,
            vec![
                ProviderEvent::Reasoning("why".into()),
                ProviderEvent::Text("answer".into()),
                ProviderEvent::ToolCallStart {
                    index: 2,
                    id: "call-1".into(),
                    name: "read_file".into(),
                },
                ProviderEvent::ToolCallArg {
                    index: 2,
                    arg: "{\"path\":".into(),
                },
                ProviderEvent::ToolCallArg {
                    index: 2,
                    arg: "\"input.txt\"}".into(),
                },
                ProviderEvent::ResponseItem(ProviderResponseItem::ResponsesOutput {
                    source: ProviderStateSourceV1::for_test(
                        "unknown",
                        ProviderProtocol::OpenAiResponses
                    ),
                    output_index: 0,
                    item: reasoning_item,
                }),
                ProviderEvent::ResponseItem(ProviderResponseItem::ResponsesOutput {
                    source: ProviderStateSourceV1::for_test(
                        "unknown",
                        ProviderProtocol::OpenAiResponses
                    ),
                    output_index: 1,
                    item: message_item,
                }),
                ProviderEvent::ResponseItem(ProviderResponseItem::ResponsesOutput {
                    source: ProviderStateSourceV1::for_test(
                        "unknown",
                        ProviderProtocol::OpenAiResponses
                    ),
                    output_index: 2,
                    item: function_call_item,
                }),
                ProviderEvent::Finish {
                    reason: FinishReason::ToolCalls,
                    usage: Some(UsageInfo {
                        prompt_tokens: 30,
                        completion_tokens: 20,
                        total_tokens: 50,
                        cached_prompt_tokens: Some(12),
                        cache_write_tokens: Some(4),
                        reasoning_tokens: Some(7),
                        response_metadata: None,
                        cache_usage_override: None,
                    }),
                },
                ProviderEvent::Done,
            ]
        );
        let metadata = events
            .iter()
            .find_map(|event| match event {
                ProviderEvent::Finish {
                    usage: Some(usage), ..
                } => usage.response_metadata.as_ref(),
                _ => None,
            })
            .unwrap();
        assert_eq!(metadata.model(), "gpt-5.6-sol");
        assert_eq!(metadata.service_tier(), Some("default"));
        assert_eq!(metadata.request_id_sha256().len(), 64);
        assert!(!format!("{metadata:?}").contains("req-secret"));
        let usage = events
            .iter()
            .find_map(|event| match event {
                ProviderEvent::Finish {
                    usage: Some(usage), ..
                } => Some(usage),
                _ => None,
            })
            .unwrap();
        assert_eq!(
            usage.cache_usage(),
            crate::agent::stream::CacheUsage::new(
                crate::agent::stream::CacheCount::Reported(12),
                crate::agent::stream::CacheCount::Reported(4),
                crate::agent::stream::CacheCount::Derived(18),
            )
        );
    }

    #[test]
    fn responses_terminal_metadata_conflicts_and_unbounded_usage_fail_closed() {
        for terminal in [
            r#"{"id":"resp-b","model":"gpt-5.6-sol","service_tier":"default"}"#,
            r#"{"id":"resp-a","model":"gpt-5.6-luna","service_tier":"default"}"#,
            r#"{"id":"resp-a","model":"gpt-5.6-sol","service_tier":"priority"}"#,
        ] {
            let mut decoder = OpenAiResponsesDecoder::with_request_id_for_test("req-a");
            decoder
                .push(b"data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp-a\",\"model\":\"gpt-5.6-sol\",\"service_tier\":\"default\"}}\n\n")
                .unwrap();
            let terminal: serde_json::Value = serde_json::from_str(terminal).unwrap();
            let event = serde_json::json!({
                "type": "response.completed",
                "response": {
                    "id": terminal["id"],
                    "model": terminal["model"],
                    "service_tier": terminal["service_tier"],
                    "status": "completed",
                    "usage": {"input_tokens": 1, "output_tokens": 1, "total_tokens": 2}
                }
            });
            assert!(matches!(
                decoder.push(format!("data: {event}\n\n").as_bytes()),
                Err(ProviderError::Protocol(_))
            ));
        }

        let mut resolved_auto = OpenAiResponsesDecoder::with_request_id_for_test("req-auto");
        resolved_auto
            .push(b"data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp-auto\",\"model\":\"gpt-5.6-sol\",\"service_tier\":\"auto\"}}\n\n")
            .unwrap();
        let completed = resolved_auto
            .push(b"data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-auto\",\"model\":\"gpt-5.6-sol\",\"service_tier\":\"default\",\"status\":\"completed\",\"usage\":{\"input_tokens\":1,\"output_tokens\":1,\"total_tokens\":2}}}\n\n")
            .unwrap();
        let tier = completed.iter().find_map(|event| match event {
            ProviderEvent::Finish {
                usage: Some(usage), ..
            } => usage
                .response_metadata
                .as_ref()
                .and_then(|metadata| metadata.service_tier()),
            _ => None,
        });
        assert_eq!(tier, Some("default"));

        let mut overflow = OpenAiResponsesDecoder::new();
        assert!(matches!(
            overflow.push(
                b"data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"usage\":{\"input_tokens\":4294967296,\"output_tokens\":0,\"total_tokens\":4294967296}}}\n\n"
            ),
            Err(ProviderError::MalformedData(_))
        ));
    }

    #[test]
    fn responses_decoder_retains_complete_output_sequence_for_local_replay() {
        let reasoning_item = serde_json::json!({
            "id": "rs_123",
            "type": "reasoning",
            "summary": [{"type": "summary_text", "text": "kept summary"}],
            "encrypted_content": "encrypted-state",
            "status": "completed"
        });
        let function_call_item = serde_json::json!({
            "id": "fc_1",
            "type": "function_call",
            "call_id": "call-1",
            "name": "read_file",
            "arguments": "{}",
            "status": "completed"
        });
        let input = concat!(
            "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"id\":\"rs_123\",\"type\":\"reasoning\",\"summary\":[]}}\n\n",
            "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"id\":\"rs_123\",\"type\":\"reasoning\",\"summary\":[{\"type\":\"summary_text\",\"text\":\"kept summary\"}],\"encrypted_content\":\"encrypted-state\",\"status\":\"completed\"}}\n\n",
            "data: {\"type\":\"response.output_item.added\",\"output_index\":1,\"item\":{\"type\":\"function_call\",\"call_id\":\"call-1\",\"name\":\"read_file\",\"arguments\":\"\"}}\n\n",
            "data: {\"type\":\"response.output_item.done\",\"output_index\":1,\"item\":{\"id\":\"fc_1\",\"type\":\"function_call\",\"call_id\":\"call-1\",\"name\":\"read_file\",\"arguments\":\"{}\",\"status\":\"completed\"}}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"usage\":{\"input_tokens\":1,\"output_tokens\":2,\"total_tokens\":3}}}\n\n"
        );
        let mut decoder = OpenAiResponsesDecoder::for_source(
            ProviderStateSourceV1::for_test("openai", ProviderProtocol::OpenAiResponses),
            None,
        );
        let mut assembler = crate::agent::stream::AssistantTurnAssembler::new();
        for byte in input.as_bytes() {
            for event in decoder.push(&[*byte]).unwrap() {
                assembler.push(&event).unwrap();
            }
        }
        for event in decoder.finish().unwrap() {
            assembler.push(&event).unwrap();
        }
        let turn = assembler.finish().unwrap();

        assert_eq!(turn.response_items.len(), 2);
        assert_eq!(
            turn.response_items[0],
            ProviderResponseItem::ResponsesOutput {
                source: ProviderStateSourceV1::for_test(
                    "openai",
                    ProviderProtocol::OpenAiResponses
                ),
                output_index: 0,
                item: reasoning_item,
            }
        );
        assert_eq!(
            turn.response_items[1],
            ProviderResponseItem::ResponsesOutput {
                source: ProviderStateSourceV1::for_test(
                    "openai",
                    ProviderProtocol::OpenAiResponses
                ),
                output_index: 1,
                item: function_call_item,
            }
        );
        assert_eq!(turn.tool_calls.len(), 1);
        assert_eq!(turn.tool_calls[0].id, "call-1");
    }

    #[test]
    fn responses_decoder_uses_safe_typed_terminal_failures() {
        for (event, expected) in [
            (
                r#"{"type":"error","message":"sentinel-provider-body"}"#,
                ProviderError::ResponseFailed,
            ),
            (
                r#"{"type":"response.incomplete","response":{"incomplete_details":"sentinel-provider-body"}}"#,
                ProviderError::ResponseIncomplete,
            ),
            (
                r#"{"type":"response.refusal.delta","delta":"sentinel-provider-body"}"#,
                ProviderError::ResponseRefused,
            ),
        ] {
            let mut decoder = OpenAiResponsesDecoder::new();
            let error = decoder
                .push(format!("data: {event}\n\n").as_bytes())
                .unwrap_err();
            assert_eq!(error, expected);
            assert!(!error.to_string().contains("sentinel-provider-body"));
        }
    }

    #[test]
    fn responses_decoder_reports_exact_output_token_limit_safely() {
        for event_type in ["response.incomplete", "response.completed"] {
            let event = serde_json::json!({
                "type": event_type,
                "response": {
                    "status": "incomplete",
                    "incomplete_details": { "reason": "max_output_tokens" },
                    "message": "sentinel-provider-body"
                }
            });
            let mut decoder = OpenAiResponsesDecoder::new();
            let error = decoder
                .push(format!("data: {event}\n\n").as_bytes())
                .unwrap_err();
            assert_eq!(error, ProviderError::OutputTokenLimit);
            assert_eq!(
                error.to_string(),
                "provider response reached its output token limit"
            );
            assert!(!format!("{error:?}").contains("sentinel-provider-body"));
        }
    }

    #[test]
    fn responses_decoder_keeps_other_incomplete_causes_safe_and_unclassified() {
        for event_type in ["response.incomplete", "response.completed"] {
            for details in [
                serde_json::json!({ "reason": "content_filter" }),
                serde_json::json!({ "reason": "sentinel-provider-body" }),
                serde_json::json!({ "reason": "max_output_tokens-sentinel-provider-body" }),
                serde_json::json!({ "reason": 123 }),
                serde_json::json!({ "reason": ["max_output_tokens"] }),
                serde_json::json!({ "reason": null }),
                serde_json::json!({ "other": "max_output_tokens" }),
                serde_json::json!("sentinel-provider-body"),
                serde_json::Value::Null,
            ] {
                let event = serde_json::json!({
                    "type": event_type,
                    "reason": "max_output_tokens",
                    "response": {
                        "status": "incomplete",
                        "incomplete_details": details,
                        "message": "sentinel-provider-body"
                    }
                });
                let mut decoder = OpenAiResponsesDecoder::new();
                let error = decoder
                    .push(format!("data: {event}\n\n").as_bytes())
                    .unwrap_err();
                assert_eq!(error, ProviderError::ResponseIncomplete);
                assert!(!format!("{error:?}").contains("sentinel-provider-body"));
                assert!(!error.to_string().contains("sentinel-provider-body"));
            }
        }

        for response in [
            serde_json::json!({
                "status": "failed",
                "incomplete_details": { "reason": "max_output_tokens" }
            }),
            serde_json::json!({
                "incomplete_details": { "reason": "max_output_tokens" }
            }),
        ] {
            let event = serde_json::json!({ "type": "response.completed", "response": response });
            let mut decoder = OpenAiResponsesDecoder::new();
            assert_eq!(
                decoder
                    .push(format!("data: {event}\n\n").as_bytes())
                    .unwrap_err(),
                ProviderError::ResponseIncomplete
            );
        }
    }

    #[test]
    fn responses_decoder_rejects_conflicting_done_arguments_and_oversized_events() {
        let mut decoder = OpenAiResponsesDecoder::new();
        decoder
            .push(b"data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"type\":\"function_call\",\"call_id\":\"call\",\"name\":\"tool\",\"arguments\":\"\"}}\n\n")
            .unwrap();
        decoder
            .push(b"data: {\"type\":\"response.function_call_arguments.delta\",\"output_index\":0,\"delta\":\"abc\"}\n\n")
            .unwrap();
        assert!(matches!(
            decoder.push(b"data: {\"type\":\"response.function_call_arguments.done\",\"output_index\":0,\"arguments\":\"xyz\"}\n\n"),
            Err(ProviderError::Protocol(_))
        ));

        let mut oversized = OpenAiResponsesDecoder::with_event_limit(8);
        assert_eq!(
            oversized.push(b"data: 123456789"),
            Err(ProviderError::EventTooLarge { limit: 8 })
        );
    }

    #[test]
    fn responses_decoder_rejects_malformed_unknown_and_premature_streams() {
        let mut malformed = OpenAiResponsesDecoder::new();
        assert!(matches!(
            malformed.push(b"data: {not-json}\n\n"),
            Err(ProviderError::MalformedData(_))
        ));

        let mut unknown = OpenAiResponsesDecoder::new();
        assert!(matches!(
            unknown.push(
                b"data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"type\":\"hosted_tool_call\"}}\n\n"
            ),
            Err(ProviderError::Protocol(_))
        ));

        let mut premature = OpenAiResponsesDecoder::new();
        let events = premature
            .push(b"data: {\"type\":\"response.output_text.delta\",\"output_index\":0,\"content_index\":0,\"delta\":\"partial\"}\n\n")
            .unwrap();
        let mut assembler = crate::agent::stream::AssistantTurnAssembler::new();
        for event in events {
            assembler.push(&event).unwrap();
        }
        assert_eq!(assembler.finish().unwrap_err(), ProviderError::PrematureEof);
    }

    #[test]
    fn decoder_maps_reasoning_text_tools_usage_finish_and_done_in_order() {
        let input = concat!(
            "data: {\"choices\":[{\"delta\":{\"analysis\":\"why\",\"content\":\"answer\",\"tool_calls\":[{\"index\":0,\"id\":\"call-1\",\"function\":{\"name\":\"read_file\",\"arguments\":\"{\\\"path\\\":\"}}]},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"input.txt\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}],\"usage\":{\"prompt_tokens\":3,\"completion_tokens\":2,\"total_tokens\":5,\"prompt_tokens_details\":{\"cached_tokens\":1},\"completion_tokens_details\":{\"reasoning_tokens\":1}}}\n\n",
            "data: [DONE]\n\n"
        );
        let mut decoder = ChatCompletionsDecoder::with_reasoning_field(Some("analysis".into()));
        let mut events = Vec::new();
        for byte in input.as_bytes() {
            events.extend(decoder.push(&[*byte]).unwrap());
        }
        events.extend(decoder.finish().unwrap());

        assert_eq!(
            events,
            vec![
                ProviderEvent::Reasoning("why".into()),
                ProviderEvent::Text("answer".into()),
                ProviderEvent::ToolCallStart {
                    index: 0,
                    id: "call-1".into(),
                    name: "read_file".into(),
                },
                ProviderEvent::ToolCallArg {
                    index: 0,
                    arg: "{\"path\":".into(),
                },
                ProviderEvent::ToolCallArg {
                    index: 0,
                    arg: "\"input.txt\"}".into(),
                },
                ProviderEvent::Finish {
                    reason: FinishReason::ToolCalls,
                    usage: Some(UsageInfo {
                        prompt_tokens: 3,
                        completion_tokens: 2,
                        total_tokens: 5,
                        cached_prompt_tokens: Some(1),
                        cache_write_tokens: None,
                        reasoning_tokens: Some(1),
                        response_metadata: None,
                        cache_usage_override: None,
                    }),
                },
                ProviderEvent::Done,
            ]
        );
    }

    #[test]
    fn openai_and_mimo_style_cache_usage_preserves_read_derived_miss_and_optional_write() {
        use crate::agent::stream::{CacheCount, CacheUsage};

        for write in [Some(4), None] {
            let mut details = serde_json::json!({"cached_tokens": 12});
            if let Some(write) = write {
                details["cache_write_tokens"] = serde_json::json!(write);
            }
            let expected = CacheUsage::new(
                CacheCount::Reported(12),
                write.map_or(CacheCount::Unknown, CacheCount::Reported),
                CacheCount::Derived(18),
            );

            let chat = serde_json::json!({
                "usage": {
                    "prompt_tokens": 30,
                    "completion_tokens": 2,
                    "total_tokens": 32,
                    "prompt_tokens_details": details.clone()
                }
            });
            let chat =
                usage_from_chat_completions(&chat, Some(PromptCacheUsageFormat::OpenAiDetails))
                    .unwrap()
                    .unwrap();
            assert_eq!(chat.cached_prompt_tokens, Some(12));
            assert_eq!(chat.cache_write_tokens, write);
            assert_eq!(chat.cache_usage(), expected);

            let responses = serde_json::json!({
                "usage": {
                    "input_tokens": 30,
                    "output_tokens": 2,
                    "total_tokens": 32,
                    "input_tokens_details": details
                }
            });
            let responses = responses_usage(
                &responses,
                None,
                Some(PromptCacheUsageFormat::OpenAiDetails),
            )
            .unwrap()
            .unwrap();
            assert_eq!(responses.cached_prompt_tokens, Some(12));
            assert_eq!(responses.cache_write_tokens, write);
            assert_eq!(responses.cache_usage(), expected);
        }
    }

    #[test]
    fn custom_auto_cache_conflicts_and_malformed_details_preserve_response_with_unknown_usage() {
        use crate::agent::stream::CacheUsage;

        let cache_shapes = [
            serde_json::json!({
                "prompt_tokens_details": {"cached_tokens": 4},
                "prompt_cache_hit_tokens": 3,
                "prompt_cache_miss_tokens": 7
            }),
            serde_json::json!({
                "prompt_tokens_details": {"cached_tokens": "malformed"}
            }),
        ];
        for cache_shape in cache_shapes {
            let mut usage = serde_json::json!({
                "prompt_tokens": 10,
                "completion_tokens": 1,
                "total_tokens": 11
            });
            usage
                .as_object_mut()
                .unwrap()
                .extend(cache_shape.as_object().unwrap().clone());
            let payload = serde_json::json!({
                "choices": [{
                    "delta": {"content": "ok"},
                    "finish_reason": "stop"
                }],
                "usage": usage
            });
            let input = format!("data: {payload}\n\ndata: [DONE]\n\n");
            let mut decoder = ChatCompletionsDecoder::with_reasoning_provider_and_cache_usage(
                None,
                None,
                Some(PromptCacheUsageFormat::Auto),
            );
            let mut events = Vec::new();
            for byte in input.as_bytes() {
                events.extend(decoder.push(&[*byte]).unwrap());
            }
            events.extend(decoder.finish().unwrap());

            assert!(events.contains(&ProviderEvent::Text("ok".into())));
            assert!(events.contains(&ProviderEvent::Done));
            let usage = events
                .iter()
                .find_map(|event| match event {
                    ProviderEvent::Finish {
                        usage: Some(usage), ..
                    } => Some(usage),
                    _ => None,
                })
                .unwrap();
            assert_eq!(usage.cached_prompt_tokens, None);
            assert_eq!(usage.cache_write_tokens, None);
            assert_eq!(usage.cache_usage(), CacheUsage::UNKNOWN);
        }
    }

    #[test]
    fn deepseek_hit_miss_contract_ignores_supplemental_openai_cache_details() {
        use crate::agent::stream::{CacheCount, CacheUsage};

        let supplemental_details = [
            serde_json::json!({"cached_tokens": null}),
            serde_json::json!({"cached_tokens": 5}),
            serde_json::json!({"cached_tokens": "malformed"}),
            serde_json::json!({"cached_tokens": 16, "cache_write_tokens": 99}),
        ];
        for details in supplemental_details {
            let usage = serde_json::json!({
                "prompt_tokens": 17,
                "completion_tokens": 9,
                "total_tokens": 26,
                "prompt_cache_hit_tokens": 5,
                "prompt_cache_miss_tokens": 12,
                "prompt_tokens_details": details
            });
            let text = serde_json::json!({
                "choices": [{
                    "delta": {"content": "ok"},
                    "finish_reason": null
                }]
            });
            let terminal_usage = serde_json::json!({"choices": [], "usage": usage});
            let input = format!("data: {text}\n\ndata: {terminal_usage}\n\ndata: [DONE]\n\n");
            let mut decoder = ChatCompletionsDecoder::with_reasoning_provider_and_cache_usage(
                None,
                None,
                Some(PromptCacheUsageFormat::HitMiss),
            );
            let mut events = Vec::new();
            for byte in input.as_bytes() {
                events.extend(decoder.push(&[*byte]).unwrap());
            }
            events.extend(decoder.finish().unwrap());

            assert!(events.contains(&ProviderEvent::Text("ok".into())));
            assert!(events.contains(&ProviderEvent::Done));
            let usage = events
                .iter()
                .find_map(|event| match event {
                    ProviderEvent::Usage(usage) => Some(usage),
                    _ => None,
                })
                .expect("usage-only terminal chunk must be retained");
            assert_eq!(usage.cached_prompt_tokens, Some(5));
            assert_eq!(usage.cache_write_tokens, None);
            assert_eq!(
                usage.cache_usage(),
                CacheUsage::new(
                    CacheCount::Reported(5),
                    CacheCount::Unknown,
                    CacheCount::Reported(12),
                )
            );
        }
    }

    #[test]
    fn chat_usage_decodes_only_complete_consistent_deepseek_cache_subsets() {
        let complete = serde_json::json!({
            "usage": {
                "prompt_tokens": 17,
                "completion_tokens": 9,
                "total_tokens": 26,
                "prompt_cache_hit_tokens": 5,
                "prompt_cache_miss_tokens": 12,
                "completion_tokens_details": {"reasoning_tokens": 4}
            }
        });
        let decoded = usage_from_chat_completions(&complete, Some(PromptCacheUsageFormat::HitMiss))
            .unwrap()
            .unwrap();
        assert_eq!(
            decoded,
            UsageInfo {
                prompt_tokens: 17,
                completion_tokens: 9,
                total_tokens: 26,
                cached_prompt_tokens: Some(5),
                cache_write_tokens: None,
                reasoning_tokens: Some(4),
                response_metadata: None,
                cache_usage_override: None,
            }
        );
        assert_eq!(
            decoded.cache_usage(),
            crate::agent::stream::CacheUsage::new(
                crate::agent::stream::CacheCount::Reported(5),
                crate::agent::stream::CacheCount::Unknown,
                crate::agent::stream::CacheCount::Reported(12),
            )
        );

        for usage in [
            serde_json::json!({
                "prompt_tokens": 17,
                "completion_tokens": 9,
                "total_tokens": 26,
                "prompt_cache_hit_tokens": 5
            }),
            serde_json::json!({
                "prompt_tokens": 17,
                "completion_tokens": 9,
                "total_tokens": 26,
                "prompt_cache_hit_tokens": 5,
                "prompt_cache_miss_tokens": 11
            }),
            serde_json::json!({
                "prompt_tokens": 17,
                "completion_tokens": 9,
                "total_tokens": 26,
                "prompt_cache_hit_tokens": "five",
                "prompt_cache_miss_tokens": 12
            }),
            serde_json::json!({
                "prompt_tokens": 17,
                "completion_tokens": 9,
                "total_tokens": 26,
                "prompt_cache_hit_tokens": 4294967296_u64,
                "prompt_cache_miss_tokens": 12
            }),
        ] {
            let error = usage_from_chat_completions(
                &serde_json::json!({"usage": usage}),
                Some(PromptCacheUsageFormat::HitMiss),
            )
            .unwrap_err();
            assert!(matches!(error, ProviderError::MalformedData(_)));
        }
    }

    #[test]
    fn resolved_deepseek_responses_uses_cached_input_details() {
        use crate::agent::stream::{CacheCount, CacheUsage};

        let provider = crate::config::ProviderConfig {
            base_url: "https://api.deepseek.com/v1".into(),
            api_key: "${DEEPSEEK_API_KEY}".into(),
            protocol: ProviderProtocol::OpenAiResponses,
            prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
            models: vec!["deepseek-v4-flash".into()],
            timeout: crate::config::ProviderTimeoutConfig::default(),
        };
        let contract = crate::provider_contract::resolve_built_in_prompt_cache(
            "deepseek",
            &provider,
            "deepseek-v4-flash",
        );
        assert_eq!(
            contract.usage_format(),
            Some(PromptCacheUsageFormat::OpenAiDetails)
        );
        let response = serde_json::json!({
            "usage": {
                "input_tokens": 17,
                "input_tokens_details": {"cached_tokens": 5},
                "output_tokens": 9,
                "total_tokens": 26
            }
        });
        let usage = responses_usage(&response, None, contract.usage_format())
            .unwrap()
            .unwrap();
        assert_eq!(usage.cached_prompt_tokens, Some(5));
        assert_eq!(usage.cache_write_tokens, None);
        assert_eq!(
            usage.cache_usage(),
            CacheUsage::new(
                CacheCount::Reported(5),
                CacheCount::Unknown,
                CacheCount::Derived(12),
            )
        );
    }

    #[test]
    fn replayable_chat_reasoning_is_aggregate_byte_bounded() {
        let source = ProviderStateSourceV1::from_route(
            "deepseek",
            ProviderProtocol::ChatCompletions,
            "https://api.deepseek.com/v1",
        )
        .unwrap();
        let mut decoder = ChatCompletionsDecoder::with_reasoning_provider(
            Some("reasoning_content".into()),
            Some(source),
        );
        let fragment = "λ".repeat(MAX_CHAT_REASONING_BYTES / 4 + 1);
        assert!(fragment.len() < MAX_CHAT_REASONING_BYTES);
        assert!(fragment.len().saturating_mul(2) > MAX_CHAT_REASONING_BYTES);
        let event = format!(
            "data: {{\"choices\":[{{\"delta\":{{\"reasoning_content\":{}}},\"finish_reason\":null}}]}}\n\n",
            serde_json::to_string(&fragment).unwrap()
        );
        assert!(event.len() < DEFAULT_SSE_EVENT_LIMIT);

        let events = decoder.push(event.as_bytes()).unwrap();
        assert!(matches!(
            events.as_slice(),
            [ProviderEvent::Reasoning(reasoning)] if reasoning == &fragment
        ));
        assert_eq!(decoder.reasoning_content.len(), fragment.len());

        assert_eq!(
            decoder.push(event.as_bytes()).unwrap_err(),
            ProviderError::EventTooLarge {
                limit: MAX_CHAT_REASONING_BYTES
            }
        );
        assert_eq!(decoder.reasoning_content.len(), fragment.len());
    }

    #[test]
    fn deepseek_and_mimo_chat_decoders_capture_reasoning_content_without_a_profile() {
        let input = concat!(
            "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"  why\\n\"},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"λ  \",\"tool_calls\":[{\"index\":0,\"id\":\"call-1\",\"function\":{\"name\":\"read_file\",\"arguments\":\"{}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n"
        );

        for provider in ["deepseek", "mimo"] {
            let model = if provider == "deepseek" {
                "deepseek-v4-pro"
            } else {
                "mimo-v2.5-pro"
            };
            let request = crate::request::PreparedModelRequest::new(
                crate::request::RequestKind::MainTurn,
                provider,
                model,
                Vec::new(),
                Vec::new(),
                crate::agent::client::ChatRequestOptions::default(),
            );
            let mut decoder = ProviderProtocol::ChatCompletions.adapter().decoder(
                &request,
                &ProviderStateSourceV1::for_test(
                    request.provider.clone(),
                    ProviderProtocol::ChatCompletions,
                ),
                test_contract_for_request(&request, ProviderProtocol::ChatCompletions),
                None,
            );
            let mut assembler = crate::agent::stream::AssistantTurnAssembler::new();
            for byte in input.as_bytes() {
                for event in decoder.push(&[*byte]).unwrap() {
                    assembler.push(&event).unwrap();
                }
            }
            for event in decoder.finish().unwrap() {
                assembler.push(&event).unwrap();
            }
            let turn = assembler.finish().unwrap();
            assert_eq!(turn.reasoning.as_deref(), Some("  why\nλ  "));
            assert_eq!(
                turn.response_items,
                vec![ProviderResponseItem::ChatReasoning {
                    source: ProviderStateSourceV1::for_test(
                        provider,
                        ProviderProtocol::ChatCompletions
                    ),
                    content: "  why\nλ  ".into(),
                }]
            );
        }
    }

    #[test]
    fn generic_chat_decoder_does_not_expose_provider_reasoning_content() {
        let request = crate::request::PreparedModelRequest::new(
            crate::request::RequestKind::MainTurn,
            "openrouter",
            "model",
            Vec::new(),
            Vec::new(),
            crate::agent::client::ChatRequestOptions::default(),
        );
        let mut decoder = ProviderProtocol::ChatCompletions.adapter().decoder(
            &request,
            &ProviderStateSourceV1::for_test(
                request.provider.clone(),
                ProviderProtocol::ChatCompletions,
            ),
            None,
            None,
        );
        let events = decoder
            .push(b"data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"private\",\"content\":\"answer\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n")
            .unwrap();
        assert_eq!(
            events,
            vec![
                ProviderEvent::Text("answer".into()),
                ProviderEvent::Finish {
                    reason: FinishReason::Stop,
                    usage: None,
                },
                ProviderEvent::Done,
            ]
        );
    }

    fn first_party_request(
        provider: &str,
        model: &str,
        effort: Option<ReasoningEffort>,
    ) -> crate::request::PreparedModelRequest {
        crate::request::PreparedModelRequest::new(
            crate::request::RequestKind::MainTurn,
            provider,
            model,
            vec![ChatMessage {
                role: "user".into(),
                content: crate::image_content::ModelContent::text("hello"),
                tool_calls: None,
                tool_call_id: None,
                response_items: Vec::new(),
            }],
            Vec::new(),
            crate::agent::client::ChatRequestOptions {
                reasoning_effort: effort,
                reasoning_request_format: Some(ReasoningRequestFormat::ReasoningObject),
                reasoning_stream_field: None,
                tool_call_format: None,
            },
        )
    }

    #[test]
    fn astra_reasoning_serializes_supported_efforts_on_responses() {
        for provider in ["openai", "openai-codex"] {
            for effort in [
                ReasoningEffort::Low,
                ReasoningEffort::Medium,
                ReasoningEffort::High,
                ReasoningEffort::XHigh,
                ReasoningEffort::Max,
            ] {
                let request = first_party_request(provider, "gpt-6-astra", Some(effort));
                let bytes = openai_responses_bytes_for_test(&request).unwrap();
                let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                assert_eq!(body["model"], "gpt-6-astra");
                assert_eq!(body["reasoning"]["effort"], effort.to_string());
            }
        }
    }

    #[test]
    fn mimo_and_deepseek_chat_use_exact_thinking_contracts() {
        let mimo = first_party_request("mimo", "mimo-v2.5-pro", Some(ReasoningEffort::High));
        let mimo = chat_completions_json_for_test(&mimo);
        assert_eq!(mimo["thinking"]["type"], "enabled");
        assert!(mimo.get("reasoning").is_none());
        assert!(mimo.get("reasoning_effort").is_none());

        let deepseek =
            first_party_request("deepseek", "deepseek-v4-pro", Some(ReasoningEffort::XHigh));
        let deepseek = chat_completions_json_for_test(&deepseek);
        assert_eq!(deepseek["thinking"]["type"], "enabled");
        assert_eq!(deepseek["reasoning_effort"], "max");
        assert!(deepseek.get("reasoning").is_none());

        let disabled =
            first_party_request("deepseek", "deepseek-v4-flash", Some(ReasoningEffort::None));
        let disabled = chat_completions_json_for_test(&disabled);
        assert_eq!(disabled["thinking"]["type"], "disabled");
        assert!(disabled.get("reasoning_effort").is_none());
    }

    #[test]
    fn mimo_and_deepseek_responses_map_effort_and_exclude_openai_fields() {
        let mimo = first_party_request("mimo", "mimo-v2.5", Some(ReasoningEffort::Low));
        let mimo_bytes = openai_responses_bytes_for_test(&mimo).unwrap();
        let mimo: serde_json::Value = serde_json::from_slice(&mimo_bytes).unwrap();
        assert_eq!(mimo["reasoning"]["effort"], "high");
        assert!(mimo.get("include").is_none());
        assert!(mimo.get("store").is_none());
        assert!(mimo.get("prompt_cache_key").is_none());

        let deepseek =
            first_party_request("deepseek", "deepseek-v4-pro", Some(ReasoningEffort::XHigh));
        let deepseek_bytes = openai_responses_bytes_for_test(&deepseek).unwrap();
        let deepseek: serde_json::Value = serde_json::from_slice(&deepseek_bytes).unwrap();
        assert_eq!(deepseek["reasoning"]["effort"], "max");
        assert!(deepseek.get("include").is_none());
        assert!(deepseek.get("store").is_none());

        let unsupported =
            first_party_request("deepseek", "deepseek-v4-flash", Some(ReasoningEffort::None));
        assert!(matches!(
            openai_responses_bytes_for_test(&unsupported),
            Err(ProviderError::Protocol(message))
                if message.contains("does not support disabling reasoning")
        ));
    }

    #[test]
    fn first_party_responses_reject_unsupported_images_before_serialization() {
        let image = crate::image_content::normalize_rgba(1, 1, vec![10, 20, 30, 255]).unwrap();
        for (provider, model) in [("mimo", "mimo-v2.5-pro"), ("deepseek", "deepseek-v4-pro")] {
            let request = crate::request::PreparedModelRequest::new(
                crate::request::RequestKind::MainTurn,
                provider,
                model,
                vec![ChatMessage {
                    role: "user".into(),
                    content: crate::image_content::ModelContent::multimodal(
                        "inspect",
                        std::slice::from_ref(&image),
                    ),
                    tool_calls: None,
                    tool_call_id: None,
                    response_items: Vec::new(),
                }],
                Vec::new(),
                crate::agent::client::ChatRequestOptions::default(),
            );
            assert!(matches!(
                openai_responses_bytes_for_test(&request),
                Err(ProviderError::Protocol(message))
                    if message.contains("does not support image input")
            ));
        }
    }

    #[test]
    fn direct_openai_cache_modes_are_exact_and_scoped() {
        let request = crate::request::PreparedModelRequest::new(
            crate::request::RequestKind::MainTurn,
            "openai",
            "gpt-5.6-sol",
            vec![
                ChatMessage {
                    role: "system".into(),
                    content: crate::image_content::ModelContent::text("stable system"),
                    tool_calls: None,
                    tool_call_id: None,
                    response_items: Vec::new(),
                },
                ChatMessage {
                    role: "developer".into(),
                    content: crate::image_content::ModelContent::text("stable developer"),
                    tool_calls: None,
                    tool_call_id: None,
                    response_items: Vec::new(),
                },
                ChatMessage {
                    role: "user".into(),
                    content: crate::image_content::ModelContent::text("dynamic user"),
                    tool_calls: None,
                    tool_call_id: None,
                    response_items: Vec::new(),
                },
            ],
            Vec::new(),
            crate::agent::client::ChatRequestOptions::default(),
        );
        let source = test_state_source_for_request(&request, ProviderProtocol::OpenAiResponses);
        let contract = test_contract_for_request(&request, ProviderProtocol::OpenAiResponses);

        let implicit = OpenAiPromptCacheRequest::new(
            PromptCachePolicy::Automatic,
            Some("private-cache-key".into()),
        );
        let implicit: serde_json::Value = serde_json::from_slice(
            &serialize_openai_responses(&request, &source, contract, None, Some(&implicit))
                .unwrap(),
        )
        .unwrap();
        assert_eq!(implicit["prompt_cache_key"], "private-cache-key");
        assert!(implicit.get("prompt_cache_options").is_none());
        assert!(implicit
            .to_string()
            .find("prompt_cache_breakpoint")
            .is_none());
        assert_eq!(implicit["store"], false);

        let stable = OpenAiPromptCacheRequest::new(
            PromptCachePolicy::StablePrefix,
            Some("private-cache-key".into()),
        )
        .with_stable_prefix_messages(2);
        let stable: serde_json::Value = serde_json::from_slice(
            &serialize_openai_responses(&request, &source, contract, None, Some(&stable)).unwrap(),
        )
        .unwrap();
        assert_eq!(stable["prompt_cache_options"]["mode"], "explicit");
        assert_eq!(
            stable["input"][1]["content"][0]["prompt_cache_breakpoint"]["mode"],
            "explicit"
        );
        assert!(stable["input"][0]
            .to_string()
            .find("prompt_cache_breakpoint")
            .is_none());
        assert!(stable["input"][2]
            .to_string()
            .find("prompt_cache_breakpoint")
            .is_none());

        let off = OpenAiPromptCacheRequest::new(PromptCachePolicy::Off, None);
        let off: serde_json::Value = serde_json::from_slice(
            &serialize_openai_responses(&request, &source, contract, None, Some(&off)).unwrap(),
        )
        .unwrap();
        assert!(off.get("prompt_cache_key").is_none());
        assert_eq!(off["prompt_cache_options"]["mode"], "explicit");
        assert!(off.to_string().find("prompt_cache_breakpoint").is_none());
    }
}
