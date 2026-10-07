use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

/// Durable, credential-free identity for the provider route that issued opaque
/// continuation state. The raw endpoint is never persisted or formatted.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderStateSourceV1 {
    pub provider: String,
    pub protocol: crate::agent::provider::ProviderProtocol,
    pub endpoint_sha256: String,
}

impl ProviderStateSourceV1 {
    pub(crate) fn from_route(
        provider: impl Into<String>,
        protocol: crate::agent::provider::ProviderProtocol,
        base_url: &str,
    ) -> Result<Self, ProviderError> {
        let canonical_endpoint = canonical_provider_endpoint(base_url)?;
        let mut hasher = Sha256::new();
        hasher.update(b"ca-provider-endpoint-v1\0");
        hasher.update(canonical_endpoint.as_bytes());
        Ok(Self {
            provider: provider.into(),
            protocol,
            endpoint_sha256: format!("{:x}", hasher.finalize()),
        })
    }

    #[cfg(test)]
    pub(crate) fn for_test(
        provider: impl Into<String>,
        protocol: crate::agent::provider::ProviderProtocol,
    ) -> Self {
        Self::from_route(provider, protocol, "https://example.invalid/v1")
            .expect("static test provider route")
    }
}

fn canonical_provider_endpoint(base_url: &str) -> Result<String, ProviderError> {
    let mut url = reqwest::Url::parse(base_url.trim())
        .map_err(|_| ProviderError::Protocol("provider base URL is invalid".into()))?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.host_str().is_none()
    {
        return Err(ProviderError::Protocol(
            "provider base URL cannot identify continuation state".into(),
        ));
    }
    let trimmed_path = url.path().trim_end_matches('/').to_string();
    url.set_path(if trimmed_path.is_empty() {
        "/"
    } else {
        &trimmed_path
    });
    Ok(url.to_string().trim_end_matches('/').to_string())
}

impl std::fmt::Debug for ProviderStateSourceV1 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProviderStateSourceV1")
            .field("provider", &self.provider)
            .field("protocol", &self.protocol)
            .field("endpoint_sha256", &self.endpoint_sha256)
            .finish()
    }
}

/// Provider-issued reasoning state that must be replayed when `ca` manages
/// conversation history locally.
///
/// `source` prevents opaque state from crossing provider, protocol, or endpoint
/// boundaries. Responses items also retain `output_index` so replay order is deterministic
/// without adding that stream-only index to the provider's JSON item.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProviderResponseItem {
    ResponsesOutput {
        source: ProviderStateSourceV1,
        output_index: usize,
        item: Value,
    },
    ChatReasoning {
        source: ProviderStateSourceV1,
        content: String,
    },
}

impl std::fmt::Debug for ProviderResponseItem {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ResponsesOutput {
                source,
                output_index,
                ..
            } => formatter
                .debug_struct("ResponsesOutput")
                .field("source", source)
                .field("output_index", output_index)
                .field("item", &"[REDACTED]")
                .finish(),
            Self::ChatReasoning { source, .. } => formatter
                .debug_struct("ChatReasoning")
                .field("source", source)
                .field("content", &"[REDACTED]")
                .finish(),
        }
    }
}

const MAX_PROVIDER_MODEL_BYTES: usize = 256;
const MAX_PROVIDER_SERVICE_TIER_BYTES: usize = 32;

/// Bounded terminal metadata returned by a provider for one physical request.
///
/// The provider request ID is irreversibly hashed before storage. Debug output
/// redacts every field so request routing metadata cannot leak through logs.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProviderResponseMetadata {
    request_id_sha256: String,
    model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    service_tier: Option<String>,
}

impl ProviderResponseMetadata {
    pub(crate) fn new(
        request_id_sha256: String,
        model: String,
        service_tier: Option<String>,
    ) -> Result<Self, ProviderError> {
        if !is_lower_hex_sha256(&request_id_sha256)
            || !is_safe_provider_label(&model, MAX_PROVIDER_MODEL_BYTES)
            || service_tier.as_deref().is_some_and(|value| {
                !is_safe_provider_label(value, MAX_PROVIDER_SERVICE_TIER_BYTES)
            })
        {
            return Err(ProviderError::Protocol(
                "provider response metadata is invalid".into(),
            ));
        }
        Ok(Self {
            request_id_sha256,
            model,
            service_tier,
        })
    }

    pub fn request_id_sha256(&self) -> &str {
        &self.request_id_sha256
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn service_tier(&self) -> Option<&str> {
        self.service_tier.as_deref()
    }
}

impl std::fmt::Debug for ProviderResponseMetadata {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProviderResponseMetadata")
            .field("request_id_sha256", &"[REDACTED]")
            .field("model", &"[REDACTED]")
            .field("service_tier", &"[REDACTED]")
            .finish()
    }
}

fn is_lower_hex_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn is_safe_provider_label(value: &str, maximum: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b':' | b'/')
        })
}

#[derive(Clone, Serialize, Deserialize)]
pub struct UsageInfo {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cached_prompt_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_metadata: Option<ProviderResponseMetadata>,
    #[serde(skip)]
    pub(crate) cache_usage_override: Option<CacheUsage>,
}

/// A normalized cache token count together with its provenance.
///
/// This type is intentionally not serialized. `UsageInfo` is part of the
/// stable headless/service wire shape, so provider-neutral cache accounting is
/// exposed as an internal view over its existing optional fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheCount {
    Unknown,
    Reported(u32),
    Derived(u32),
}

impl CacheCount {
    pub const fn tokens(self) -> Option<u32> {
        match self {
            Self::Unknown => None,
            Self::Reported(tokens) | Self::Derived(tokens) => Some(tokens),
        }
    }

    pub const fn is_reported(self) -> bool {
        matches!(self, Self::Reported(_))
    }

    pub const fn is_unknown(self) -> bool {
        matches!(self, Self::Unknown)
    }
}

/// Provider-neutral cache accounting.
///
/// Cache writes are a subset of misses, not a third disjoint partition of
/// prompt tokens. When only the legacy cached-input field is available, misses
/// are derived as `prompt - read` while an omitted write count remains unknown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheUsage {
    pub read: CacheCount,
    pub write: CacheCount,
    pub miss: CacheCount,
}

impl CacheUsage {
    pub const UNKNOWN: Self = Self {
        read: CacheCount::Unknown,
        write: CacheCount::Unknown,
        miss: CacheCount::Unknown,
    };

    pub const fn new(read: CacheCount, write: CacheCount, miss: CacheCount) -> Self {
        Self { read, write, miss }
    }

    pub fn from_legacy(
        prompt_tokens: u32,
        cached_prompt_tokens: Option<u32>,
        cache_write_tokens: Option<u32>,
    ) -> Self {
        let read = cached_prompt_tokens.map_or(CacheCount::Unknown, CacheCount::Reported);
        let write = cache_write_tokens.map_or(CacheCount::Unknown, CacheCount::Reported);
        let miss = cached_prompt_tokens
            .and_then(|cached| prompt_tokens.checked_sub(cached))
            .map_or(CacheCount::Unknown, CacheCount::Derived);
        Self { read, write, miss }
    }

    pub fn valid_for_prompt(self, prompt_tokens: u32) -> bool {
        let read = self.read.tokens();
        let miss = self.miss.tokens();
        let write = self.write.tokens();

        if read.is_some_and(|tokens| tokens > prompt_tokens)
            || miss.is_some_and(|tokens| tokens > prompt_tokens)
            || write.is_some_and(|tokens| tokens > prompt_tokens)
        {
            return false;
        }
        if let (Some(read), Some(miss)) = (read, miss) {
            if read.checked_add(miss) != Some(prompt_tokens) {
                return false;
            }
        }
        if let (Some(write), Some(miss)) = (write, miss) {
            if write > miss {
                return false;
            }
        }
        true
    }

    pub const fn has_unknown_detail(self) -> bool {
        self.read.is_unknown() || self.write.is_unknown() || self.miss.is_unknown()
    }
}

impl UsageInfo {
    pub fn cache_usage(&self) -> CacheUsage {
        self.cache_usage_override.unwrap_or_else(|| {
            CacheUsage::from_legacy(
                self.prompt_tokens,
                self.cached_prompt_tokens,
                self.cache_write_tokens,
            )
        })
    }

    pub(crate) fn with_cache_usage(mut self, cache_usage: CacheUsage) -> Option<Self> {
        if !cache_usage.valid_for_prompt(self.prompt_tokens) {
            return None;
        }
        self.cache_usage_override = Some(cache_usage);
        Some(self)
    }
}

impl std::fmt::Debug for UsageInfo {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("UsageInfo")
            .field("prompt_tokens", &self.prompt_tokens)
            .field("completion_tokens", &self.completion_tokens)
            .field("total_tokens", &self.total_tokens)
            .field("cached_prompt_tokens", &self.cached_prompt_tokens)
            .field("cache_write_tokens", &self.cache_write_tokens)
            .field("reasoning_tokens", &self.reasoning_tokens)
            .field("response_metadata", &self.response_metadata)
            .finish()
    }
}

/// Token equality intentionally excludes response metadata: pricing and usage
/// deduplication compare token facts, while callers inspect terminal metadata
/// explicitly through `response_metadata`.
impl PartialEq for UsageInfo {
    fn eq(&self, other: &Self) -> bool {
        self.prompt_tokens == other.prompt_tokens
            && self.completion_tokens == other.completion_tokens
            && self.total_tokens == other.total_tokens
            && self.cached_prompt_tokens == other.cached_prompt_tokens
            && self.cache_write_tokens == other.cache_write_tokens
            && self.reasoning_tokens == other.reasoning_tokens
    }
}

impl Eq for UsageInfo {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FinishReason {
    Stop,
    ToolCalls,
    Length,
    ContentFilter,
    Other(String),
}

impl FinishReason {
    pub(crate) fn parse(value: &str) -> Self {
        match value {
            "stop" => Self::Stop,
            "tool_calls" | "function_call" => Self::ToolCalls,
            "length" => Self::Length,
            "content_filter" => Self::ContentFilter,
            other => Self::Other(other.to_string()),
        }
    }
}

impl std::fmt::Display for FinishReason {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Stop => formatter.write_str("stop"),
            Self::ToolCalls => formatter.write_str("tool_calls"),
            Self::Length => formatter.write_str("length"),
            Self::ContentFilter => formatter.write_str("content_filter"),
            Self::Other(value) => formatter.write_str(value),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProviderEvent {
    Text(String),
    Reasoning(String),
    ResponseItem(ProviderResponseItem),
    ToolCallStart {
        index: usize,
        id: String,
        name: String,
    },
    ToolCallArg {
        index: usize,
        arg: String,
    },
    Finish {
        reason: FinishReason,
        usage: Option<UsageInfo>,
    },
    Usage(UsageInfo),
    Done,
}

/// Whether an event makes replaying the physical provider request unsafe.
/// Terminal metadata stays attempt-local until a stream either reaches this
/// boundary or completes cleanly.
pub(crate) fn provider_event_has_usable_output(event: &ProviderEvent) -> bool {
    match event {
        ProviderEvent::Text(text) | ProviderEvent::Reasoning(text) => !text.is_empty(),
        ProviderEvent::ToolCallStart { .. } => true,
        ProviderEvent::ToolCallArg { arg, .. } => !arg.is_empty(),
        ProviderEvent::ResponseItem(_)
        | ProviderEvent::Finish { .. }
        | ProviderEvent::Usage(_)
        | ProviderEvent::Done => false,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderTransportFailure {
    Timeout,
    Connect,
    Request,
}

impl std::fmt::Display for ProviderTransportFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Timeout => "request timed out",
            Self::Connect => "could not connect",
            Self::Request => "request failed",
        })
    }
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum ProviderError {
    #[error("provider HTTP {status}")]
    Http { status: u16 },
    #[error("provider transport failed: {0}")]
    Transport(ProviderTransportFailure),
    #[error("provider API key environment variable {0} is not set")]
    MissingEnvironment(String),
    #[error("provider event contains invalid UTF-8")]
    InvalidUtf8,
    #[error("provider event exceeded {limit} bytes")]
    EventTooLarge { limit: usize },
    #[error("malformed provider data: {0}")]
    MalformedData(String),
    #[error("provider stream ended before [DONE] and a complete finish reason")]
    PrematureEof,
    #[error("provider response was incomplete")]
    IncompleteFinish(FinishReason),
    #[error("provider response failed")]
    ResponseFailed,
    #[error("provider response was incomplete")]
    ResponseIncomplete,
    #[error("provider response reached its output token limit")]
    OutputTokenLimit,
    #[error("provider response was refused")]
    ResponseRefused,
    #[error("unresolved tool call at index {index}: {detail}")]
    UnresolvedToolCall { index: usize, detail: String },
    #[error("provider protocol violation: {0}")]
    Protocol(String),
    #[error("provider stream anomaly: {0}")]
    StreamAnomaly(StreamAnomalyKind),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamAnomalyKind {
    RepetitionLoop,
    SuspectedToolEnvelope,
}

impl std::fmt::Display for StreamAnomalyKind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::RepetitionLoop => "repetition loop detected",
            Self::SuspectedToolEnvelope => "suspected tool envelope in assistant text",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssembledToolCall {
    pub index: usize,
    pub id: String,
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssembledAssistantTurn {
    pub content: String,
    pub reasoning: Option<String>,
    pub response_items: Vec<ProviderResponseItem>,
    pub tool_calls: Vec<AssembledToolCall>,
    pub usage: Option<UsageInfo>,
    pub finish_reason: FinishReason,
}

#[derive(Debug, Default)]
struct PartialCall {
    id: Option<String>,
    name: Option<String>,
    arguments: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AssistantTurnLimits {
    max_text_bytes: usize,
    max_reasoning_bytes: usize,
    max_tool_argument_bytes: usize,
    max_tool_calls: usize,
    max_turn_bytes: usize,
}

impl AssistantTurnLimits {
    pub const fn new(
        max_text_bytes: usize,
        max_reasoning_bytes: usize,
        max_tool_argument_bytes: usize,
        max_tool_calls: usize,
        max_turn_bytes: usize,
    ) -> Self {
        Self {
            max_text_bytes,
            max_reasoning_bytes,
            max_tool_argument_bytes,
            max_tool_calls,
            max_turn_bytes,
        }
    }
}

#[derive(Debug, Default)]
pub struct AssistantTurnAssembler {
    content: String,
    reasoning: String,
    response_items: Vec<ProviderResponseItem>,
    tool_calls: BTreeMap<usize, PartialCall>,
    usage: Option<UsageInfo>,
    finish_reason: Option<FinishReason>,
    done: bool,
    limits: Option<AssistantTurnLimits>,
    text_bytes: usize,
    reasoning_bytes: usize,
    tool_argument_bytes: usize,
    turn_bytes: usize,
}

impl AssistantTurnAssembler {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_limits(limits: AssistantTurnLimits) -> Self {
        Self {
            limits: Some(limits),
            ..Self::default()
        }
    }

    #[cfg(test)]
    pub fn partial_text(&self) -> &str {
        &self.content
    }

    pub fn push(&mut self, event: &ProviderEvent) -> Result<(), ProviderError> {
        if self.done {
            return Err(ProviderError::Protocol(
                "event received after [DONE]".into(),
            ));
        }
        self.enforce_limits(event)?;
        match event {
            ProviderEvent::Text(text) => self.content.push_str(text),
            ProviderEvent::Reasoning(text) => self.reasoning.push_str(text),
            ProviderEvent::ResponseItem(item) => self.response_items.push(item.clone()),
            ProviderEvent::ToolCallStart { index, id, name } => {
                let call = self.tool_calls.entry(*index).or_default();
                if call.id.as_deref().is_some_and(|existing| existing != id)
                    || call
                        .name
                        .as_deref()
                        .is_some_and(|existing| existing != name)
                {
                    return Err(ProviderError::Protocol(format!(
                        "conflicting tool call fragments at index {index}"
                    )));
                }
                call.id = Some(id.clone());
                call.name = Some(name.clone());
            }
            ProviderEvent::ToolCallArg { index, arg } => {
                self.tool_calls
                    .entry(*index)
                    .or_default()
                    .arguments
                    .push_str(arg);
            }
            ProviderEvent::Finish { reason, usage } => {
                if self.finish_reason.replace(reason.clone()).is_some() {
                    return Err(ProviderError::Protocol("duplicate finish reason".into()));
                }
                if usage.is_some() {
                    self.usage = usage.clone();
                }
            }
            ProviderEvent::Usage(usage) => self.usage = Some(usage.clone()),
            ProviderEvent::Done => self.done = true,
        }
        Ok(())
    }

    fn enforce_limits(&mut self, event: &ProviderEvent) -> Result<(), ProviderError> {
        let Some(limits) = self.limits else {
            return Ok(());
        };
        let text_increment = match event {
            ProviderEvent::Text(value) => value.len(),
            _ => 0,
        };
        let reasoning_increment = match event {
            ProviderEvent::Reasoning(value) => value.len(),
            _ => 0,
        };
        let argument_increment = match event {
            ProviderEvent::ToolCallArg { arg, .. } => arg.len(),
            _ => 0,
        };
        let new_tool_call = match event {
            ProviderEvent::ToolCallStart { index, .. }
            | ProviderEvent::ToolCallArg { index, .. } => !self.tool_calls.contains_key(index),
            _ => false,
        };
        let turn_increment = provider_event_payload_bytes(event)?;

        let next_text = bounded_sum(self.text_bytes, text_increment, limits.max_text_bytes)?;
        let next_reasoning = bounded_sum(
            self.reasoning_bytes,
            reasoning_increment,
            limits.max_reasoning_bytes,
        )?;
        let next_arguments = bounded_sum(
            self.tool_argument_bytes,
            argument_increment,
            limits.max_tool_argument_bytes,
        )?;
        let next_tool_calls = bounded_sum(
            self.tool_calls.len(),
            usize::from(new_tool_call),
            limits.max_tool_calls,
        )?;
        let next_turn = bounded_sum(self.turn_bytes, turn_increment, limits.max_turn_bytes)?;

        debug_assert_eq!(
            next_tool_calls,
            self.tool_calls.len() + usize::from(new_tool_call)
        );
        self.text_bytes = next_text;
        self.reasoning_bytes = next_reasoning;
        self.tool_argument_bytes = next_arguments;
        self.turn_bytes = next_turn;
        Ok(())
    }

    pub fn finish(self) -> Result<AssembledAssistantTurn, ProviderError> {
        if !self.done {
            return Err(ProviderError::PrematureEof);
        }
        let reason = self.finish_reason.ok_or(ProviderError::PrematureEof)?;
        if matches!(
            reason,
            FinishReason::Length | FinishReason::ContentFilter | FinishReason::Other(_)
        ) {
            return Err(ProviderError::IncompleteFinish(reason));
        }
        if reason == FinishReason::Stop && !self.tool_calls.is_empty() {
            return Err(ProviderError::Protocol(
                "stop finish reason included unresolved tool calls".into(),
            ));
        }
        if reason == FinishReason::ToolCalls && self.tool_calls.is_empty() {
            return Err(ProviderError::Protocol(
                "tool_calls finish reason contained no tool calls".into(),
            ));
        }
        let mut tool_calls = Vec::new();
        for (index, call) in self.tool_calls {
            let id = call.id.filter(|value| !value.is_empty()).ok_or_else(|| {
                ProviderError::UnresolvedToolCall {
                    index,
                    detail: "missing id".into(),
                }
            })?;
            let name = call.name.filter(|value| !value.is_empty()).ok_or_else(|| {
                ProviderError::UnresolvedToolCall {
                    index,
                    detail: "missing name".into(),
                }
            })?;
            serde_json::from_str::<Value>(&call.arguments).map_err(|error| {
                ProviderError::UnresolvedToolCall {
                    index,
                    detail: format!("invalid arguments JSON: {error}"),
                }
            })?;
            tool_calls.push(AssembledToolCall {
                index,
                id,
                name,
                arguments: call.arguments,
            });
        }
        Ok(AssembledAssistantTurn {
            content: self.content,
            reasoning: (!self.reasoning.is_empty()).then_some(self.reasoning),
            response_items: self.response_items,
            tool_calls,
            usage: self.usage,
            finish_reason: reason,
        })
    }
}

fn bounded_sum(current: usize, increment: usize, maximum: usize) -> Result<usize, ProviderError> {
    current
        .checked_add(increment)
        .filter(|total| *total <= maximum)
        .ok_or_else(provider_response_limit_error)
}

fn provider_response_limit_error() -> ProviderError {
    ProviderError::Protocol("provider response exceeded configured limits".into())
}

fn provider_event_payload_bytes(event: &ProviderEvent) -> Result<usize, ProviderError> {
    match event {
        ProviderEvent::Text(value) | ProviderEvent::Reasoning(value) => Ok(value.len()),
        ProviderEvent::ResponseItem(item) => serde_json::to_vec(item)
            .map(|encoded| encoded.len())
            .map_err(|_| provider_response_limit_error()),
        ProviderEvent::ToolCallStart { id, name, .. } => id
            .len()
            .checked_add(name.len())
            .ok_or_else(provider_response_limit_error),
        ProviderEvent::ToolCallArg { arg, .. } => Ok(arg.len()),
        ProviderEvent::Finish { reason, usage } => {
            let reason_bytes = match reason {
                FinishReason::Other(value) => value.len(),
                _ => 0,
            };
            let usage_bytes = usage
                .as_ref()
                .map(serde_json::to_vec)
                .transpose()
                .map_err(|_| provider_response_limit_error())?
                .map_or(0, |encoded| encoded.len());
            reason_bytes
                .checked_add(usage_bytes)
                .ok_or_else(provider_response_limit_error)
        }
        ProviderEvent::Usage(usage) => serde_json::to_vec(usage)
            .map(|encoded| encoded.len())
            .map_err(|_| provider_response_limit_error()),
        ProviderEvent::Done => Ok(0),
    }
}

pub async fn consume_stream<F>(
    receiver: &mut tokio::sync::mpsc::UnboundedReceiver<Result<ProviderEvent, ProviderError>>,
    mut on_event: F,
) -> Result<AssembledAssistantTurn, ProviderError>
where
    F: FnMut(&ProviderEvent),
{
    let mut assembler = AssistantTurnAssembler::new();
    while let Some(item) = receiver.recv().await {
        let event = item?;
        assembler.push(&event)?;
        on_event(&event);
    }
    assembler.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::provider::{ChatCompletionsDecoder, ProviderProtocol};

    #[test]
    fn provider_state_source_normalizes_equivalent_routes_and_separates_boundaries() {
        let canonical = ProviderStateSourceV1::from_route(
            "deepseek",
            ProviderProtocol::ChatCompletions,
            "https://API.DeepSeek.com:443/v1///",
        )
        .unwrap();
        let equivalent = ProviderStateSourceV1::from_route(
            "deepseek",
            ProviderProtocol::ChatCompletions,
            " https://api.deepseek.com/v1 ",
        )
        .unwrap();
        assert_eq!(canonical, equivalent);
        assert_eq!(canonical.endpoint_sha256.len(), 64);
        assert!(canonical
            .endpoint_sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)));

        for different in [
            ProviderStateSourceV1::from_route(
                "mimo",
                ProviderProtocol::ChatCompletions,
                "https://api.deepseek.com/v1",
            )
            .unwrap(),
            ProviderStateSourceV1::from_route(
                "deepseek",
                ProviderProtocol::OpenAiResponses,
                "https://api.deepseek.com/v1",
            )
            .unwrap(),
            ProviderStateSourceV1::from_route(
                "deepseek",
                ProviderProtocol::ChatCompletions,
                "https://api.deepseek.com:8443/v1",
            )
            .unwrap(),
            ProviderStateSourceV1::from_route(
                "deepseek",
                ProviderProtocol::ChatCompletions,
                "https://api.deepseek.com/v2",
            )
            .unwrap(),
        ] {
            assert_ne!(canonical, different);
        }
    }

    #[test]
    fn provider_state_source_rejects_credential_or_request_components_and_redacts_endpoint() {
        for invalid in [
            "ftp://api.example.invalid/v1",
            "https://user@example.invalid/v1",
            "https://example.invalid/v1?token=secret",
            "https://example.invalid/v1#secret",
            "not a URL",
        ] {
            assert!(ProviderStateSourceV1::from_route(
                "safe",
                ProviderProtocol::ChatCompletions,
                invalid,
            )
            .is_err());
        }

        let source = ProviderStateSourceV1::from_route(
            "safe",
            ProviderProtocol::ChatCompletions,
            "https://sentinel-route.example.invalid/private/v1",
        )
        .unwrap();
        let debug = format!("{source:?}");
        let json = serde_json::to_string(&source).unwrap();
        assert!(!debug.contains("sentinel-route"));
        assert!(!debug.contains("private/v1"));
        assert!(!json.contains("sentinel-route"));
        assert!(!json.contains("private/v1"));
    }

    #[test]
    fn provider_response_item_debug_redacts_opaque_reasoning_payloads() {
        let responses = ProviderResponseItem::ResponsesOutput {
            source: ProviderStateSourceV1::for_test(
                "openai",
                crate::agent::provider::ProviderProtocol::OpenAiResponses,
            ),
            output_index: 7,
            item: serde_json::json!({
                "id": "rs_secret",
                "type": "reasoning",
                "encrypted_content": "sentinel-encrypted-reasoning"
            }),
        };
        let responses_debug = format!("{responses:?}");
        assert!(responses_debug.contains("ResponsesOutput"));
        assert!(responses_debug.contains("openai"));
        assert!(responses_debug.contains('7'));
        assert!(responses_debug.contains("[REDACTED]"));
        assert!(!responses_debug.contains("rs_secret"));
        assert!(!responses_debug.contains("sentinel-encrypted-reasoning"));

        let chat = ProviderResponseItem::ChatReasoning {
            source: ProviderStateSourceV1::for_test(
                "deepseek",
                crate::agent::provider::ProviderProtocol::ChatCompletions,
            ),
            content: "sentinel-chat-reasoning".into(),
        };
        let chat_debug = format!("{chat:?}");
        assert!(chat_debug.contains("ChatReasoning"));
        assert!(chat_debug.contains("deepseek"));
        assert!(chat_debug.contains("[REDACTED]"));
        assert!(!chat_debug.contains("sentinel-chat-reasoning"));
    }

    #[test]
    fn usage_metadata_is_bounded_redacted_backward_compatible_and_not_token_identity() {
        let metadata = ProviderResponseMetadata::new(
            "a".repeat(64),
            "gpt-5.6-sol".into(),
            Some("default".into()),
        )
        .unwrap();
        let with_metadata = UsageInfo {
            prompt_tokens: 10,
            completion_tokens: 2,
            total_tokens: 12,
            cached_prompt_tokens: Some(3),
            cache_write_tokens: Some(4),
            reasoning_tokens: Some(1),
            response_metadata: Some(metadata.clone()),
            cache_usage_override: None,
        };
        let mut token_only = with_metadata.clone();
        token_only.response_metadata = None;
        assert_eq!(with_metadata, token_only);
        assert_eq!(metadata.request_id_sha256(), "a".repeat(64));
        assert_eq!(metadata.model(), "gpt-5.6-sol");
        assert_eq!(metadata.service_tier(), Some("default"));
        let debug = format!("{with_metadata:?}");
        for forbidden in ["gpt-5.6-sol", "default", &"a".repeat(64)] {
            assert!(!debug.contains(forbidden));
        }

        let restored: UsageInfo =
            serde_json::from_str(r#"{"prompt_tokens":1,"completion_tokens":2,"total_tokens":3}"#)
                .unwrap();
        assert_eq!(restored.cache_write_tokens, None);
        assert_eq!(restored.response_metadata, None);
        assert!(
            ProviderResponseMetadata::new("b".repeat(64), "unsafe model".into(), None,).is_err()
        );
    }

    #[test]
    fn cache_usage_preserves_reported_zero_and_derives_misses_without_changing_wire_shape() {
        let usage = UsageInfo {
            prompt_tokens: 10,
            completion_tokens: 2,
            total_tokens: 12,
            cached_prompt_tokens: Some(4),
            cache_write_tokens: Some(0),
            reasoning_tokens: None,
            response_metadata: None,
            cache_usage_override: None,
        };
        assert_eq!(
            usage.cache_usage(),
            CacheUsage::new(
                CacheCount::Reported(4),
                CacheCount::Reported(0),
                CacheCount::Derived(6),
            )
        );
        assert!(usage.cache_usage().valid_for_prompt(10));
        let encoded = serde_json::to_value(&usage).unwrap();
        assert!(encoded.get("cache_usage").is_none());
        assert!(encoded.get("cache_miss_tokens").is_none());

        let reported_miss = usage
            .clone()
            .with_cache_usage(CacheUsage::new(
                CacheCount::Reported(4),
                CacheCount::Reported(0),
                CacheCount::Reported(6),
            ))
            .unwrap();
        assert_eq!(reported_miss.cache_usage().miss, CacheCount::Reported(6));
        assert!(!format!("{reported_miss:?}").contains("cache_usage_override"));
        assert!(serde_json::to_value(&reported_miss)
            .unwrap()
            .get("cache_usage_override")
            .is_none());
        assert!(usage
            .clone()
            .with_cache_usage(CacheUsage::new(
                CacheCount::Reported(4),
                CacheCount::Unknown,
                CacheCount::Reported(7),
            ))
            .is_none());

        let unknown = UsageInfo {
            cached_prompt_tokens: None,
            cache_write_tokens: None,
            ..usage
        }
        .cache_usage();
        assert_eq!(unknown, CacheUsage::UNKNOWN);
        assert!(unknown.has_unknown_detail());
    }

    #[test]
    fn cache_usage_validates_writes_as_a_subset_of_misses() {
        assert!(CacheUsage::new(
            CacheCount::Reported(6),
            CacheCount::Reported(4),
            CacheCount::Reported(4),
        )
        .valid_for_prompt(10));
        assert!(!CacheUsage::new(
            CacheCount::Reported(6),
            CacheCount::Reported(5),
            CacheCount::Reported(4),
        )
        .valid_for_prompt(10));
        assert!(!CacheUsage::new(
            CacheCount::Reported(6),
            CacheCount::Unknown,
            CacheCount::Reported(5),
        )
        .valid_for_prompt(10));
    }

    fn feed_bytewise(input: &[u8]) -> Result<Vec<ProviderEvent>, ProviderError> {
        let mut decoder = ChatCompletionsDecoder::with_reasoning_field(None);
        let mut events = Vec::new();
        for byte in input {
            events.extend(decoder.push(&[*byte])?);
        }
        events.extend(decoder.finish()?);
        Ok(events)
    }

    #[test]
    fn bytewise_utf8_crlf_comments_multiline_and_done_are_lossless() {
        let input = concat!(
            ": keepalive\r\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"hé🙂\"},\r\n",
            "data: \"finish_reason\":null}]}\r\n\r\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\r\n\r\n",
            "data: [DONE]\r\n\r\n"
        );

        let events = feed_bytewise(input.as_bytes()).unwrap();

        assert_eq!(
            events,
            vec![
                ProviderEvent::Text("hé🙂".into()),
                ProviderEvent::Finish {
                    reason: FinishReason::Stop,
                    usage: None,
                },
                ProviderEvent::Done,
            ]
        );
    }

    #[test]
    fn usage_only_event_is_preserved() {
        let events = feed_bytewise(
            b"data: {\"choices\":[],\"usage\":{\"prompt_tokens\":3,\"completion_tokens\":2,\"total_tokens\":5}}\n\n",
        )
        .unwrap();
        assert_eq!(
            events,
            vec![ProviderEvent::Usage(UsageInfo {
                prompt_tokens: 3,
                completion_tokens: 2,
                total_tokens: 5,
                cached_prompt_tokens: None,
                cache_write_tokens: None,
                reasoning_tokens: None,
                response_metadata: None,
                cache_usage_override: None,
            })]
        );
    }

    #[test]
    fn detailed_usage_subsets_are_preserved() {
        let events = feed_bytewise(
            br#"data: {"choices":[],"usage":{"prompt_tokens":30,"completion_tokens":20,"total_tokens":50,"prompt_tokens_details":{"cached_tokens":12,"cache_write_tokens":4},"completion_tokens_details":{"reasoning_tokens":7}}}

"#,
        )
        .unwrap();
        assert_eq!(
            events,
            vec![ProviderEvent::Usage(UsageInfo {
                prompt_tokens: 30,
                completion_tokens: 20,
                total_tokens: 50,
                cached_prompt_tokens: Some(12),
                cache_write_tokens: Some(4),
                reasoning_tokens: Some(7),
                response_metadata: None,
                cache_usage_override: None,
            })]
        );
    }

    #[test]
    fn configured_reasoning_field_is_byte_split_and_assembled_separately() {
        let input = concat!(
            "data: {\"choices\":[{\"delta\":{\"analysis\":\"why🙂\",\"content\":\"answer\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n"
        );
        let mut decoder = ChatCompletionsDecoder::with_reasoning_field(Some("analysis".into()));
        let mut assembler = AssistantTurnAssembler::new();
        let mut events = Vec::new();
        for byte in input.as_bytes() {
            events.extend(decoder.push(&[*byte]).unwrap());
        }
        events.extend(decoder.finish().unwrap());
        for event in &events {
            assembler.push(event).unwrap();
        }
        assert_eq!(events[0], ProviderEvent::Reasoning("why🙂".into()));
        assert_eq!(events[1], ProviderEvent::Text("answer".into()));
        let turn = assembler.finish().unwrap();
        assert_eq!(turn.reasoning.as_deref(), Some("why🙂"));
        assert_eq!(turn.content, "answer");
    }

    #[test]
    fn malformed_json_invalid_utf8_and_oversized_events_fail() {
        let mut malformed = ChatCompletionsDecoder::with_reasoning_field(None);
        assert!(matches!(
            malformed.push(b"data: {no}\n\n"),
            Err(ProviderError::MalformedData(_))
        ));

        let mut utf8 = ChatCompletionsDecoder::with_reasoning_field(None);
        assert_eq!(
            utf8.push(b"data: \xff\n\n").unwrap_err(),
            ProviderError::InvalidUtf8
        );

        let mut oversized = ChatCompletionsDecoder::with_event_limit(8);
        assert_eq!(
            oversized.push(b"data: 123456789\n").unwrap_err(),
            ProviderError::EventTooLarge { limit: 8 }
        );

        assert!(matches!(
            feed_bytewise(
                b"data: {\"choices\":[],\"usage\":{\"prompt_tokens\":4294967296,\"completion_tokens\":0,\"total_tokens\":4294967296}}\n\n"
            ),
            Err(ProviderError::MalformedData(_))
        ));
    }

    #[test]
    fn assembler_rejects_premature_eof_and_incomplete_finish_reasons() {
        let mut premature = AssistantTurnAssembler::new();
        premature
            .push(&ProviderEvent::Text("partial".into()))
            .unwrap();
        assert_eq!(premature.partial_text(), "partial");
        assert_eq!(premature.finish().unwrap_err(), ProviderError::PrematureEof);

        for reason in [FinishReason::Length, FinishReason::ContentFilter] {
            let mut assembler = AssistantTurnAssembler::new();
            assembler
                .push(&ProviderEvent::Finish {
                    reason: reason.clone(),
                    usage: None,
                })
                .unwrap();
            assembler.push(&ProviderEvent::Done).unwrap();
            assert_eq!(
                assembler.finish().unwrap_err(),
                ProviderError::IncompleteFinish(reason)
            );
        }
    }

    #[test]
    fn assembler_validates_tool_identity_arguments_and_finish_reason() {
        let mut assembler = AssistantTurnAssembler::new();
        assembler
            .push(&ProviderEvent::ToolCallStart {
                index: 0,
                id: "call-1".into(),
                name: "read_file".into(),
            })
            .unwrap();
        assembler
            .push(&ProviderEvent::ToolCallArg {
                index: 0,
                arg: "{\"path\":\"src/main.rs\"}".into(),
            })
            .unwrap();
        assembler
            .push(&ProviderEvent::Finish {
                reason: FinishReason::ToolCalls,
                usage: None,
            })
            .unwrap();
        assembler.push(&ProviderEvent::Done).unwrap();

        let turn = assembler.finish().unwrap();
        assert_eq!(turn.tool_calls.len(), 1);
        assert_eq!(turn.tool_calls[0].id, "call-1");

        let mut unresolved = AssistantTurnAssembler::new();
        unresolved
            .push(&ProviderEvent::ToolCallArg {
                index: 3,
                arg: "{}".into(),
            })
            .unwrap();
        unresolved
            .push(&ProviderEvent::Finish {
                reason: FinishReason::ToolCalls,
                usage: None,
            })
            .unwrap();
        unresolved.push(&ProviderEvent::Done).unwrap();
        assert!(matches!(
            unresolved.finish(),
            Err(ProviderError::UnresolvedToolCall { index: 3, .. })
        ));
    }

    #[test]
    fn bounded_assembler_counts_fragmented_text_reasoning_and_arguments() {
        let limit_error =
            ProviderError::Protocol("provider response exceeded configured limits".into());

        let mut text =
            AssistantTurnAssembler::with_limits(AssistantTurnLimits::new(4, 100, 100, 8, 1_000));
        text.push(&ProviderEvent::Text("ab".into())).unwrap();
        text.push(&ProviderEvent::Text("cd".into())).unwrap();
        assert_eq!(
            text.push(&ProviderEvent::Text("sentinel-text".into())),
            Err(limit_error.clone())
        );

        let mut reasoning =
            AssistantTurnAssembler::with_limits(AssistantTurnLimits::new(100, 4, 100, 8, 1_000));
        reasoning
            .push(&ProviderEvent::Reasoning("ab".into()))
            .unwrap();
        reasoning
            .push(&ProviderEvent::Reasoning("cd".into()))
            .unwrap();
        assert_eq!(
            reasoning.push(&ProviderEvent::Reasoning("sentinel-reasoning".into())),
            Err(limit_error.clone())
        );

        let mut arguments =
            AssistantTurnAssembler::with_limits(AssistantTurnLimits::new(100, 100, 4, 8, 1_000));
        arguments
            .push(&ProviderEvent::ToolCallArg {
                index: 0,
                arg: "ab".into(),
            })
            .unwrap();
        arguments
            .push(&ProviderEvent::ToolCallArg {
                index: 0,
                arg: "cd".into(),
            })
            .unwrap();
        assert_eq!(
            arguments.push(&ProviderEvent::ToolCallArg {
                index: 0,
                arg: "sentinel-arguments".into(),
            }),
            Err(limit_error)
        );
    }

    #[test]
    fn bounded_assembler_limits_unique_tool_calls_and_aggregate_turn_bytes() {
        let limit_error =
            ProviderError::Protocol("provider response exceeded configured limits".into());
        let mut calls =
            AssistantTurnAssembler::with_limits(AssistantTurnLimits::new(100, 100, 100, 1, 1_000));
        calls
            .push(&ProviderEvent::ToolCallArg {
                index: 0,
                arg: "{}".into(),
            })
            .unwrap();
        assert_eq!(
            calls.push(&ProviderEvent::ToolCallStart {
                index: 1,
                id: "sentinel-call-id".into(),
                name: "sentinel-call-name".into(),
            }),
            Err(limit_error.clone())
        );

        let mut aggregate =
            AssistantTurnAssembler::with_limits(AssistantTurnLimits::new(100, 100, 100, 8, 5));
        aggregate.push(&ProviderEvent::Text("ab".into())).unwrap();
        aggregate
            .push(&ProviderEvent::Reasoning("cd".into()))
            .unwrap();
        aggregate.push(&ProviderEvent::Text("e".into())).unwrap();
        let error = aggregate
            .push(&ProviderEvent::Text("sentinel-aggregate".into()))
            .unwrap_err();
        assert_eq!(error, limit_error);
        assert!(!error.to_string().contains("sentinel"));
    }

    #[test]
    fn bounded_assembler_counts_opaque_response_items_in_aggregate_limit() {
        let canary = "sentinel-opaque-provider-item";
        let item = ProviderResponseItem::ResponsesOutput {
            source: ProviderStateSourceV1::for_test(
                "openai",
                crate::agent::provider::ProviderProtocol::OpenAiResponses,
            ),
            output_index: 0,
            item: serde_json::json!({"encrypted_content": canary}),
        };
        let mut assembler = AssistantTurnAssembler::with_limits(AssistantTurnLimits::new(
            1_000, 1_000, 1_000, 8, 8,
        ));

        let error = assembler
            .push(&ProviderEvent::ResponseItem(item))
            .unwrap_err();

        assert_eq!(
            error,
            ProviderError::Protocol("provider response exceeded configured limits".into())
        );
        assert!(!error.to_string().contains(canary));
    }

    #[tokio::test]
    async fn shared_consumer_propagates_transport_error_and_clean_channel_eof() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        tx.send(Ok(ProviderEvent::Text("partial".into()))).unwrap();
        tx.send(Err(ProviderError::Transport(
            ProviderTransportFailure::Connect,
        )))
        .unwrap();
        drop(tx);
        assert_eq!(
            consume_stream(&mut rx, |_| {}).await.unwrap_err(),
            ProviderError::Transport(ProviderTransportFailure::Connect)
        );

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        tx.send(Ok(ProviderEvent::Finish {
            reason: FinishReason::Stop,
            usage: None,
        }))
        .unwrap();
        drop(tx);
        assert_eq!(
            consume_stream(&mut rx, |_| {}).await.unwrap_err(),
            ProviderError::PrematureEof
        );
    }
}
