use crate::agent::stream::{
    ProviderError, ProviderEvent, ProviderResponseItem, ProviderTransportFailure,
};
use crate::app::{Message, Role, StreamingState};
use crate::config::{ProviderConfig, ReasoningEffort, ReasoningRequestFormat};
use futures::StreamExt;
use reqwest::Client;
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub struct ProviderCallPolicy {
    pub connect_timeout: std::time::Duration,
    pub headers_timeout: std::time::Duration,
    pub first_event_timeout: std::time::Duration,
    pub no_progress_timeout: std::time::Duration,
    pub max_attempts: usize,
    pub retry_delay: std::time::Duration,
    pub max_retry_after: std::time::Duration,
}

impl Default for ProviderCallPolicy {
    fn default() -> Self {
        Self {
            connect_timeout: std::time::Duration::from_secs(10),
            headers_timeout: std::time::Duration::from_secs(60),
            first_event_timeout: std::time::Duration::from_secs(60),
            no_progress_timeout: std::time::Duration::from_secs(120),
            max_attempts: 2,
            retry_delay: std::time::Duration::from_millis(250),
            max_retry_after: std::time::Duration::from_secs(5),
        }
    }
}

impl ProviderCallPolicy {
    /// Policy for callers that reserve and account for every physical POST.
    /// Ordinary clients continue to use [`ProviderCallPolicy::default`].
    pub fn one_attempt() -> Self {
        Self {
            max_attempts: 1,
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)] // complete service-tier wire enum; ordinary builds currently select Default only
pub enum OpenAiServiceTier {
    Auto,
    Default,
    Flex,
    Priority,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenAiResponsesRequestOptions {
    max_output_tokens: Option<u32>,
    store: bool,
    service_tier: Option<OpenAiServiceTier>,
}

#[cfg(any(test, feature = "planner-executor-experiment"))]
const MAX_OPENAI_CLIENT_REQUEST_ID_BYTES: usize = 512;

impl OpenAiResponsesRequestOptions {
    #[cfg(any(test, feature = "planner-executor-experiment"))]
    pub const MAX_OUTPUT_TOKENS: u32 = 8_192;

    /// Exact bounded options used by the isolated planner/executor pilot.
    #[cfg(any(test, feature = "planner-executor-experiment"))]
    pub fn capped(service_tier: Option<OpenAiServiceTier>) -> Self {
        Self {
            max_output_tokens: Some(Self::MAX_OUTPUT_TOKENS),
            store: false,
            service_tier,
        }
    }

    pub fn codex() -> Self {
        Self {
            max_output_tokens: None,
            store: false,
            service_tier: None,
        }
    }

    pub fn max_output_tokens(self) -> Option<u32> {
        self.max_output_tokens
    }

    pub fn store(self) -> bool {
        self.store
    }

    pub fn service_tier(self) -> Option<OpenAiServiceTier> {
        self.service_tier
    }
}

fn compressed_request_body(base_url: &str, body: &[u8]) -> (Vec<u8>, bool) {
    if !base_url.contains("api.x.ai") || body.len() < 64 * 1024 {
        return (body.to_vec(), false);
    }
    match zstd::encode_all(std::io::Cursor::new(body), 3) {
        Ok(compressed) if compressed.len() < body.len() => (compressed, true),
        _ => (body.to_vec(), false),
    }
}

/// Dial the provider origin once so the first sampling request is not also the handshake.
pub fn prewarm_origin(base_url: &str) {
    static STARTED: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    if !base_url.contains("api.x.ai") || STARTED.set(()).is_err() {
        return;
    }
    let url = base_url.trim_end_matches('/').to_string();
    tokio::spawn(async move {
        if let Ok(client) = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(2))
            .build()
        {
            let _ = client.get(url).send().await;
        }
    });
}

#[derive(Clone)]
pub struct AgentClient {
    google: Option<crate::google_auth::Config>,
    google_auth: Option<crate::google_auth::Auth>,
    tool_schema_policy: crate::tool_schema::Policy,
    provider_id: String,
    base_url: String,
    api_key: String,
    codex_account_id: Option<String>,
    model: String,
    protocol: crate::agent::provider::ProviderProtocol,
    contract: Option<crate::provider_contract::FirstPartyContract>,
    client: Client,
    policy: ProviderCallPolicy,
    responses_options: Option<OpenAiResponsesRequestOptions>,
    output_token_cap: Option<u32>,
    input_byte_cap: Option<usize>,
    prompt_cache_contract: crate::provider_contract::ResolvedPromptCacheContract,
    openai_client_request_id: Option<String>,
    request_store: crate::request::RequestStore,
    trace_hub: Option<crate::provider_trace::ProviderTraceHub>,
    trace_session_id: Option<uuid::Uuid>,
    privacy_override: Option<std::sync::Arc<crate::privacy::Context>>,
    filter_override: Option<
        std::sync::Arc<std::sync::RwLock<std::sync::Arc<crate::provider_filter::ProviderFilter>>>,
    >,
}

impl std::fmt::Debug for AgentClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AgentClient")
            .field("provider", &self.provider_id)
            .field("model", &self.model)
            .field("protocol", &self.protocol)
            .field("credential", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: crate::image_content::ModelContent,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCallMsg>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub response_items: Vec<ProviderResponseItem>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolCallMsg {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: FunctionCall,
}

#[derive(Debug, Clone, Serialize)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
    #[serde(skip)]
    pub arguments_retrieved_spans: Vec<crate::retrieved_content::RetrievedSpan>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ChatRequestOptions {
    pub reasoning_effort: Option<ReasoningEffort>,
    pub reasoning_request_format: Option<ReasoningRequestFormat>,
    pub reasoning_stream_field: Option<String>,
    pub tool_call_format: Option<crate::config::ToolCallFormat>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolDef {
    #[serde(rename = "type")]
    pub def_type: String,
    pub function: ToolFunction,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolFunction {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

pub type ChatDelta = ProviderEvent;
pub type ChatStreamItem = Result<ProviderEvent, ProviderError>;

#[async_trait::async_trait]
pub trait ChatBackend: Send + Sync {
    /// Only native clients expose a route for bounded host inference. Custom
    /// budgeted backends keep their own authority unless they opt in explicitly.
    fn inference_client(&self) -> Option<AgentClient> {
        None
    }

    async fn prepare_harness_request(
        &self,
        request: crate::request::PreparedModelRequest,
    ) -> crate::request::PreparedModelRequest {
        request
    }

    async fn stream_chat(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDef],
    ) -> Result<mpsc::UnboundedReceiver<ChatStreamItem>, ProviderError>;

    async fn stream_chat_with_options(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDef],
        _options: &ChatRequestOptions,
    ) -> Result<mpsc::UnboundedReceiver<ChatStreamItem>, ProviderError> {
        self.stream_chat(messages, tools).await
    }

    async fn stream_prepared(
        &self,
        request: &crate::request::PreparedModelRequest,
    ) -> Result<mpsc::UnboundedReceiver<ChatStreamItem>, ProviderError> {
        self.stream_chat_with_options(&request.messages, &request.tools, &request.options)
            .await
    }
}

impl AgentClient {
    pub fn provider_id(&self) -> &str {
        &self.provider_id
    }

    pub fn model_id(&self) -> &str {
        &self.model
    }

    #[cfg(test)]
    pub fn new(provider: &ProviderConfig, model: &str) -> Self {
        Self::with_request_store(
            "unknown",
            provider,
            model,
            crate::request::RequestStore::default(),
        )
    }

    pub fn with_request_store(
        provider_id: impl Into<String>,
        provider: &ProviderConfig,
        model: &str,
        request_store: crate::request::RequestStore,
    ) -> Self {
        Self::with_call_policy(
            provider_id,
            provider,
            model,
            provider.timeout.to_call_policy(),
            request_store,
        )
    }

    pub fn with_call_policy(
        provider_id: impl Into<String>,
        provider: &ProviderConfig,
        model: &str,
        policy: ProviderCallPolicy,
        request_store: crate::request::RequestStore,
    ) -> Self {
        let provider_id = provider_id.into();
        let mut policy = policy;
        if matches!(provider_id.as_str(), "xai" | "xai-oauth") && policy.max_attempts == 2 {
            policy.max_attempts = 15;
            if policy.max_retry_after <= std::time::Duration::from_secs(5) {
                policy.max_retry_after = std::time::Duration::from_secs(30);
            }
        }
        let google = (provider.protocol == crate::agent::ProviderProtocol::GoogleGenerateContent)
            .then(|| provider.google.clone().unwrap_or_default());
        let base_url = google
            .as_ref()
            .and_then(|settings| settings.endpoint(&provider.base_url).ok())
            .unwrap_or_else(|| provider.base_url.trim_end_matches('/').to_string());
        let google_auth = google
            .as_ref()
            .filter(|settings| settings.mode == crate::google_auth::Mode::Vertex)
            .cloned()
            .map(crate::google_auth::Auth::new);
        let codex_provider = provider_id == "openai-codex";
        let contract = crate::provider_contract::resolve(&provider_id, provider, model);
        let prompt_cache_contract =
            crate::provider_contract::resolve_built_in_prompt_cache(&provider_id, provider, model);
        let client = Client::builder()
            .connect_timeout(policy.connect_timeout)
            .build()
            .expect("static provider HTTP client configuration is valid");
        Self {
            google,
            google_auth,
            tool_schema_policy: provider.tool_schema_policy,
            codex_account_id: codex_provider
                .then(|| crate::codex_auth::account_id_for_access_token(&provider.api_key))
                .flatten(),
            provider_id,
            base_url,
            api_key: provider.api_key.clone(),
            model: model.to_string(),
            protocol: provider.protocol,
            contract,
            client,
            policy,
            responses_options: (codex_provider
                || contract.is_some_and(|contract| contract.is_direct_openai()))
            .then(OpenAiResponsesRequestOptions::codex),
            output_token_cap: None,
            input_byte_cap: None,
            prompt_cache_contract,
            openai_client_request_id: None,
            request_store,
            trace_hub: crate::provider_trace::process_hub(),
            trace_session_id: None,
            privacy_override: None,
            filter_override: None,
        }
    }

    #[cfg(test)]
    fn with_policy(
        provider_id: impl Into<String>,
        provider: &ProviderConfig,
        model: &str,
        policy: ProviderCallPolicy,
        request_store: crate::request::RequestStore,
    ) -> Self {
        Self::with_call_policy(provider_id, provider, model, policy, request_store)
    }

    #[cfg(test)]
    fn with_codex_account_id_for_test(mut self, account_id: &str) -> Self {
        self.codex_account_id = Some(account_id.into());
        self
    }

    #[cfg(test)]
    fn with_trace_hub_for_test(mut self, hub: crate::provider_trace::ProviderTraceHub) -> Self {
        self.trace_hub = Some(hub);
        self
    }

    #[cfg(test)]
    fn with_contract_for_test(
        mut self,
        contract: crate::provider_contract::FirstPartyContract,
    ) -> Self {
        self.contract = Some(contract);
        self
    }

    pub(crate) fn with_trace_session_id(mut self, session_id: uuid::Uuid) -> Self {
        self.trace_session_id = Some(session_id);
        self
    }

    pub(crate) fn privacy_context(&self) -> Option<std::sync::Arc<crate::privacy::Context>> {
        if crate::privacy::tasks::remote_binding().is_some() {
            return None;
        }
        self.privacy_override
            .clone()
            .or_else(|| self.request_store.privacy_context())
    }

    pub(crate) fn with_privacy_context(
        mut self,
        context: Option<std::sync::Arc<crate::privacy::Context>>,
    ) -> Self {
        self.privacy_override = context;
        self
    }

    fn consumer_chat_shim(&self) -> bool {
        crate::chatgpt_plan_auth::consumer_chat_shim(&self.provider_id)
    }

    fn active_filter(&self) -> std::sync::Arc<crate::provider_filter::ProviderFilter> {
        if crate::chatgpt_plan_auth::skips_provider_filter(&self.provider_id) {
            // ChatGPT chat is a conversation. It does not use the provider
            // filter, including a filter armed only for this test.
            return std::sync::Arc::new(
                crate::provider_filter::ProviderFilter::compile(
                    &crate::provider_filter::ProviderFilterConfig::default(),
                )
                .expect("disabled provider filter compiles"),
            );
        }
        self.filter_override
            .as_ref()
            .map(|slot| {
                slot.read()
                    .unwrap_or_else(|poison| poison.into_inner())
                    .clone()
            })
            .or_else(|| {
                self.privacy_context()
                    .and_then(|context| context.task_filter())
            })
            .unwrap_or_else(crate::provider_filter::current)
    }

    /// An idle process may be armed while a request is waiting for bytes.
    /// Re-read the process slot so that wait cannot keep an unarmed snapshot.
    async fn until_filter_tripped(&self) -> ProviderError {
        let _ = self;
        std::future::pending().await
    }

    /// A connectivity probe uses a fixed local prompt. It must not trip or
    /// observe the process provider filter.
    pub(crate) fn without_provider_filter(mut self) -> Self {
        let disabled = crate::provider_filter::ProviderFilter::compile(
            &crate::provider_filter::ProviderFilterConfig::default(),
        )
        .expect("disabled provider filter compiles");
        self.filter_override = Some(std::sync::Arc::new(std::sync::RwLock::new(
            std::sync::Arc::new(disabled),
        )));
        self
    }

    #[cfg(test)]
    pub(crate) fn with_filter_for_test(
        mut self,
        filter: std::sync::Arc<crate::provider_filter::ProviderFilter>,
    ) -> Self {
        self.filter_override = Some(std::sync::Arc::new(std::sync::RwLock::new(filter)));
        self
    }

    pub(crate) fn with_prompt_cache_contract(
        mut self,
        contract: crate::provider_contract::ResolvedPromptCacheContract,
    ) -> Self {
        self.prompt_cache_contract = contract;
        self
    }

    pub(crate) fn without_trace(mut self) -> Self {
        self.trace_hub = None;
        self.trace_session_id = None;
        self
    }

    /// Disable transport retries for a caller that reserves every physical
    /// provider attempt in a host-owned autonomy budget.
    pub(crate) fn with_one_attempt_policy(mut self) -> Self {
        self.policy.max_attempts = 1;
        self
    }

    /// Bound billable generation for a host-budgeted physical request. Normal
    /// interactive requests retain their existing provider defaults.
    pub(crate) fn with_output_token_cap(mut self, maximum: u32) -> Result<Self, ProviderError> {
        if maximum == 0 {
            return Err(ProviderError::Protocol(
                "output token cap must be positive".into(),
            ));
        }
        self.output_token_cap = Some(maximum);
        Ok(self)
    }

    /// Bound the final serialized body independently of token estimates. This
    /// is an input resource limit, not a provider tokenizer or billing proof.
    pub(crate) fn with_input_byte_cap(mut self, maximum: usize) -> Result<Self, ProviderError> {
        if maximum == 0 {
            return Err(ProviderError::Protocol(
                "input byte cap must be positive".into(),
            ));
        }
        self.input_byte_cap = Some(maximum);
        Ok(self)
    }

    pub(crate) async fn prepare_harness_request(
        &self,
        request: crate::request::PreparedModelRequest,
    ) -> crate::request::PreparedModelRequest {
        if self
            .google
            .as_ref()
            .is_some_and(|settings| settings.validate().is_ok())
        {
            if let Some(auth) = &self.google_auth {
                let _ = auth.identity().await;
            }
        }
        let mut prepared = match self.route_identity() {
            Ok(source) => {
                let mut request = request;
                request.runtime_source = Some(source.clone());
                crate::harness_guidance::prepare(request, source).await
            }
            Err(_) => request,
        };
        prepared.prepare_evidence_guidance();
        if let Err(error) = self.request_store.prepare_imported_history(&mut prepared) {
            prepared.host_context_error = Some(error);
        }
        if self.protocol != crate::agent::provider::ProviderProtocol::ChatCompletions
            && prepared.host_context_error.is_none()
        {
            if let Err(error) = self.request_store.prepare_host_contexts(&mut prepared) {
                prepared.host_context_error = Some(error);
            }
        }
        prepared
    }

    fn cooldown_key(&self) -> Option<crate::provider_cooldown::RouteKey> {
        let credential = self
            .google_auth
            .as_ref()
            .map(|auth| auth.cached_identity())
            .unwrap_or_else(|| Some(self.api_key.clone()))?;
        crate::provider_cooldown::RouteKey::new(
            &self.provider_id,
            &self.model,
            self.protocol,
            &self.base_url,
            &credential,
        )
    }

    pub(crate) fn cooldown(&self) -> Option<crate::provider_cooldown::Cooldown> {
        self.cooldown_key()
            .and_then(|key| crate::provider_cooldown::get(&key))
    }

    pub(crate) fn route_identity(
        &self,
    ) -> Result<crate::agent::stream::ProviderStateSourceV1, ProviderError> {
        let mut source = crate::agent::stream::ProviderStateSourceV1::from_route(
            self.provider_id.clone(),
            self.protocol,
            &self.base_url,
        )?;
        if matches!(
            self.protocol,
            crate::agent::ProviderProtocol::AnthropicMessages
                | crate::agent::ProviderProtocol::GoogleGenerateContent
        ) {
            let credential = if let Some(auth) = &self.google_auth {
                auth.cached_identity().ok_or_else(|| {
                    ProviderError::Protocol("Vertex ADC credential identity is not resolved".into())
                })?
            } else {
                self.api_key.clone()
            };
            source.credential_sha256 = Some(format!(
                "{:x}",
                Sha256::digest(format!("ca-native-credential-v1\0{credential}").as_bytes())
            ));
        }
        if let Some(context) = self.privacy_context().filter(|context| context.enabled()) {
            source.privacy_policy_digest = Some(context.replay_policy_digest());
        }
        if let Some(binding) = crate::privacy::tasks::remote_binding() {
            source.privacy_policy_digest = Some(binding.replay_policy_digest);
        }
        Ok(source)
    }

    #[cfg(any(test, feature = "planner-executor-experiment"))]
    pub fn with_openai_responses_options(
        mut self,
        options: OpenAiResponsesRequestOptions,
    ) -> Result<Self, ProviderError> {
        if self.protocol != crate::agent::provider::ProviderProtocol::OpenAiResponses {
            return Err(ProviderError::Protocol(
                "Responses request options require the openai_responses protocol".into(),
            ));
        }
        self.responses_options = Some(options);
        Ok(self)
    }

    /// Attach OpenAI's documented per-request correlation header.
    ///
    /// A caller-supplied ID is accepted only with a one-attempt policy so one
    /// identifier can never ambiguously cover an automatic retry.
    #[cfg(any(test, feature = "planner-executor-experiment"))]
    pub fn with_openai_client_request_id(
        mut self,
        client_request_id: impl Into<String>,
    ) -> Result<Self, ProviderError> {
        if self.protocol != crate::agent::provider::ProviderProtocol::OpenAiResponses {
            return Err(ProviderError::Protocol(
                "OpenAI client request IDs require the openai_responses protocol".into(),
            ));
        }
        if self.policy.max_attempts != 1 {
            return Err(ProviderError::Protocol(
                "OpenAI client request IDs require a one-attempt call policy".into(),
            ));
        }
        let client_request_id = client_request_id.into();
        if client_request_id.is_empty()
            || client_request_id.len() > MAX_OPENAI_CLIENT_REQUEST_ID_BYTES
            || !client_request_id.is_ascii()
            || reqwest::header::HeaderValue::try_from(client_request_id.as_str()).is_err()
        {
            return Err(ProviderError::Protocol(
                "OpenAI client request ID must be 1..=512 valid ASCII header bytes".into(),
            ));
        }
        self.openai_client_request_id = Some(client_request_id);
        Ok(self)
    }

    pub async fn chat_stream(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDef],
    ) -> Result<mpsc::UnboundedReceiver<ChatStreamItem>, ProviderError> {
        self.chat_stream_with_options(messages, tools, &ChatRequestOptions::default())
            .await
    }

    pub async fn chat_stream_with_options(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDef],
        options: &ChatRequestOptions,
    ) -> Result<mpsc::UnboundedReceiver<ChatStreamItem>, ProviderError> {
        let request = crate::request::PreparedModelRequest::new(
            crate::request::RequestKind::Compatibility,
            self.provider_id.clone(),
            self.model.clone(),
            messages.to_vec(),
            tools.to_vec(),
            options.clone(),
        );
        self.stream_prepared(&request).await
    }

    pub async fn stream_prepared(
        &self,
        request: &crate::request::PreparedModelRequest,
    ) -> Result<mpsc::UnboundedReceiver<ChatStreamItem>, ProviderError> {
        // The plan author is already inside the session scope. That scope has
        // the task filter, URL map, and filename aliases updated by list_dir.
        // The route client's handle can be the parent context, which does not.
        let context = crate::privacy::current().or_else(|| self.privacy_context());
        crate::privacy::scope(context, self.stream_prepared_inner(request)).await
    }

    async fn stream_prepared_inner(
        &self,
        request: &crate::request::PreparedModelRequest,
    ) -> Result<mpsc::UnboundedReceiver<ChatStreamItem>, ProviderError> {
        if self.request_store.privacy_pending() {
            return Err(ProviderError::PrivacyBlocked {
                detail: "Project privacy is loading; retry when the local rules are ready".into(),
            });
        }
        if let Some(settings) = &self.google {
            settings.endpoint(&self.base_url)?;
        }
        if let Some(auth) = &self.google_auth {
            auth.identity().await?;
        }
        let mut effective = self.prepare_harness_request(request.clone()).await;
        if let Some(detail) = &effective.host_context_error {
            return Err(ProviderError::PrivacyBlocked {
                detail: detail.clone(),
            });
        }
        effective.tool_schema_policy = self.tool_schema_policy;
        let privacy = crate::privacy::current().or_else(|| self.privacy_context());
        if self.active_filter().filenames()
            && privacy
                .as_ref()
                .is_none_or(|context| !context.filenames_enabled())
        {
            return Err(ProviderError::PrivacyBlocked { detail: "Filename aliases require a session-owned workspace mapping before any provider request".into() });
        }
        if let Some(context) = privacy.clone() {
            tokio::task::spawn_blocking(move || context.refresh_policy())
                .await
                .map_err(|_| ProviderError::PrivacyBlocked {
                    detail: "Project privacy rules could not be refreshed; retry this request"
                        .into(),
                })?
                .map_err(|detail| ProviderError::PrivacyBlocked { detail })?;
        }
        // The consumer chat bridge keeps the authored text. Project privacy
        // stays in force for every other provider.
        let privacy = if self.consumer_chat_shim() {
            None
        } else {
            privacy.filter(|context| context.enabled())
        };
        let original_request = privacy.as_ref().map(|_| effective.clone());
        if let Some(context) = privacy.clone() {
            effective =
                tokio::task::spawn_blocking(move || project_private_request(context, effective))
                    .await
                    .map_err(|_| ProviderError::PrivacyBlocked {
                        detail: "Project privacy worker stopped; retry this request".into(),
                    })??;
        }
        if let Some(binding) = crate::privacy::tasks::remote_binding() {
            effective.privacy_policy_digest = Some(binding.policy_fingerprint);
            effective.privacy_revision = Some(binding.policy_revision);
        }
        let filter = self.active_filter();
        let projection_source = self.route_identity()?;
        for message in &mut effective.messages {
            // Pruning may replace a whole tool result. Its role remains the
            // authoritative source identity, independent of old text lengths.
            if message.role == "tool" {
                if let Some(text) = message.content.as_str() {
                    message.content = message
                        .content
                        .clone()
                        .with_retrieved_spans(crate::retrieved_content::full_span(text));
                }
            }
            let projected = if privacy.is_some() {
                None
            } else {
                Some(message.content.project_retrieved(&filter)?)
            };
            // Validate canonical content before native adapters compare it to
            // immutable issued output. Same-route replay already retains the
            // original provider spelling, including its stand-ins.
            let native_replay = message.role == "assistant"
                && message.response_items.iter().any(|item| match item {
                    ProviderResponseItem::ResponsesOutput { source, .. } => {
                        source == &projection_source
                    }
                    ProviderResponseItem::NativeContent { source, model, .. } => {
                        source == &projection_source && model == &self.model
                    }
                    ProviderResponseItem::ChatReasoning { .. } => false,
                });
            if !native_replay {
                if let Some(projected) = projected {
                    message.content = projected;
                }
            }
            for call in message.tool_calls.as_mut().into_iter().flatten() {
                let projected = if privacy.is_some() {
                    call.function.arguments.clone()
                } else {
                    crate::retrieved_content::project_argument_string(
                        &call.function.arguments,
                        &call.function.arguments_retrieved_spans,
                        &filter,
                    )?
                };
                if !native_replay {
                    call.function.arguments = projected;
                    call.function.arguments_retrieved_spans.clear();
                }
            }
        }
        let request = &effective;
        if request.model != self.model {
            return Err(ProviderError::Protocol(
                "prepared request model does not match runtime client".into(),
            ));
        }
        if self.provider_id != "unknown" && request.provider != self.provider_id {
            return Err(ProviderError::Protocol(
                "prepared request provider does not match runtime client".into(),
            ));
        }
        let adapter = self.protocol.adapter();
        let state_source = self.route_identity()?;
        let snapshot_sequence = self.request_store.record_on_route(
            request,
            adapter.protocol().label(),
            &[self.api_key.as_str()],
            Some(&state_source),
        );
        if self.google_auth.is_none() {
            if let Some(environment) = unresolved_environment_reference(&self.api_key) {
                return Err(ProviderError::MissingEnvironment(environment.to_string()));
            }
        }
        let prompt_cache = self.prompt_cache_for_request(request, &state_source)?;
        let mut prepared_http = match adapter.prepare(
            request,
            &state_source,
            self.contract,
            self.responses_options.as_ref(),
            prompt_cache.as_ref(),
        ) {
            Ok(prepared) => prepared,
            Err(error) => {
                if let Some(diagnostics) =
                    crate::google_provider::rejected_schema_diagnostics(&error)
                {
                    self.request_store
                        .adapter_diagnostics(snapshot_sequence, &diagnostics);
                    if let Some(trace) = self.trace_hub.as_ref().and_then(|hub| {
                        hub.begin_request_with_session(
                            request,
                            adapter.protocol().label(),
                            &[self.api_key.as_str()],
                            self.trace_session_id,
                        )
                    }) {
                        trace.diagnostics(&diagnostics);
                        trace.outcome(0, "blocked_tool_schema", None);
                    }
                }
                return Err(error);
            }
        };
        let host_instructions = if privacy.is_some() {
            let mut shadow = request.clone();
            let mut replacements = Vec::new();
            for message in &mut shadow.messages {
                message.content = message
                    .content
                    .host_shadow(&mut replacements)
                    .map_err(|detail| ProviderError::PrivacyBlocked { detail })?;
                // Native continuation state is returned byte-for-byte to its
                // exact issuing route and policy. It is not executable text.
                // The adapter decides whether this typed state is replayed;
                // only fields it actually emits acquire a concrete receipt.
                for item in &mut message.response_items {
                    if let ProviderResponseItem::ChatReasoning { source, content } = item {
                        if *source == state_source {
                            let marker =
                                format!("__ca_continuation_{}__", uuid::Uuid::new_v4().simple());
                            replacements.push((marker.clone(), std::mem::replace(content, marker)));
                        }
                    }
                }
            }
            if replacements.is_empty() {
                None
            } else {
                let shadow = adapter.prepare(
                    &shadow,
                    &state_source,
                    self.contract,
                    self.responses_options.as_ref(),
                    prompt_cache.as_ref(),
                )?;
                Some(
                    crate::host_instructions::WireReceipt::bind(
                        &prepared_http.body,
                        &shadow.body,
                        replacements,
                    )
                    .map_err(|detail| ProviderError::PrivacyBlocked { detail })?,
                )
            }
        } else {
            None
        };
        self.request_store
            .adapter_diagnostics(snapshot_sequence, &prepared_http.diagnostics);
        prepared_http.replay.request_sequence = snapshot_sequence;
        prepared_http.replay.privacy_context = privacy;
        prepared_http.replay.privacy_revision = request.privacy_revision;
        prepared_http.replay.privacy_policy_digest = request.privacy_policy_digest.clone();
        if let Some(original) = original_request {
            prepared_http.replay.privacy_original = Some(std::sync::Arc::from(match adapter.prepare(
                &original,
                &state_source,
                self.contract,
                self.responses_options.as_ref(),
                prompt_cache.as_ref(),
            ) {
                Ok(prepared) => prepared.body,
                Err(_) => serde_json::to_vec(&serde_json::json!({
                    "preview_omission": "Original request is unavailable because its native protocol state cannot be lowered independently. Sent displays the final prepared request."
                })).expect("preview omission is serializable"),
            }));
        }
        if let Some(maximum) = self.output_token_cap {
            apply_output_token_cap(
                &mut prepared_http.body,
                self.protocol,
                self.contract
                    .is_some_and(|contract| contract.is_direct_openai()),
                maximum,
            )?;
        }
        if self
            .input_byte_cap
            .is_some_and(|maximum| prepared_http.body.len() > maximum)
        {
            return Err(ProviderError::Protocol(
                "prepared request exceeds input byte cap".into(),
            ));
        }
        if prepared_http.replay.privacy_context.is_some() {
            prepared_http.replay.privacy_body = Some(
                crate::provider_replay::PreparedBodyReceipt::new(&prepared_http.body)
                    .with_structured_output(request.structured_output())
                    .with_host_instructions(host_instructions),
            );
        }
        let trace = self.trace_hub.as_ref().and_then(|hub| {
            hub.begin_request_with_session(
                request,
                adapter.protocol().label(),
                &[self.api_key.as_str()],
                self.trace_session_id,
            )
        });
        if let Some(trace) = &trace {
            trace.diagnostics(&prepared_http.diagnostics);
        }
        self.stream_adapter_request(adapter, prepared_http, request, &state_source, trace)
            .await
    }

    fn prompt_cache_for_request(
        &self,
        request: &crate::request::PreparedModelRequest,
        source: &crate::agent::stream::ProviderStateSourceV1,
    ) -> Result<Option<crate::agent::provider::OpenAiPromptCacheRequest>, ProviderError> {
        use crate::config::{PromptCachePolicy, PromptCacheRequestFormat};

        match self.prompt_cache_contract.request_format() {
            None | Some(PromptCacheRequestFormat::ProviderManaged) => Ok(None),
            Some(PromptCacheRequestFormat::OpenAiResponsesExplicit) => {
                if self.protocol != crate::agent::provider::ProviderProtocol::OpenAiResponses {
                    return Err(ProviderError::Protocol(
                        "explicit prompt caching requires the openai_responses protocol".into(),
                    ));
                }
                let policy = self
                    .prompt_cache_contract
                    .policy()
                    .unwrap_or(PromptCachePolicy::Automatic);
                let Some(intent) = request.prompt_cache else {
                    // Maintenance, probes, reviews and subagents intentionally
                    // opt out of a shared cache namespace.
                    return Ok(Some(crate::agent::provider::OpenAiPromptCacheRequest::new(
                        PromptCachePolicy::Off,
                        None,
                    )));
                };
                if intent.stable_prefix_messages > request.messages.len() {
                    return Err(ProviderError::Protocol(
                        "prompt cache stable boundary exceeds prepared messages".into(),
                    ));
                }
                let route = crate::request::RouteCacheIdentity {
                    provider: source.provider.clone(),
                    endpoint_sha256: source.endpoint_sha256.clone(),
                    protocol: self.protocol.label().into(),
                    model: self.model.clone(),
                    contract_version: self.prompt_cache_contract.contract_id().into(),
                    request_layout_version: "openai_responses_v2".into(),
                };
                let plan = effective_prompt_cache_plan(intent, &route, policy, request)?;
                Ok(Some(
                    crate::agent::provider::OpenAiPromptCacheRequest::new(
                        plan.policy,
                        plan.private_key().map(str::to_owned),
                    )
                    .with_stable_prefix_messages(plan.stable_boundary.unwrap_or(0)),
                ))
            }
        }
    }

    /// Streams one already-validated, bounded Responses body without adding it
    /// to the request store. This is intentionally crate-private and exists
    /// only for RND-097's broker: its request bytes are held in transient IPC,
    /// never in ordinary request snapshots, sessions, or logs.
    #[cfg(feature = "planner-executor-experiment")]
    pub(crate) async fn stream_frozen_openai_responses_body(
        &self,
        body: Vec<u8>,
    ) -> Result<mpsc::UnboundedReceiver<ChatStreamItem>, ProviderError> {
        const MAX_FROZEN_RESPONSES_BODY_BYTES: usize = 1024 * 1024;
        if self.provider_id != "openai"
            || self.protocol != crate::agent::provider::ProviderProtocol::OpenAiResponses
            || self.policy.max_attempts != 1
            || body.is_empty()
            || body.len() > MAX_FROZEN_RESPONSES_BODY_BYTES
        {
            return Err(ProviderError::Protocol(
                "frozen Responses bodies require the canonical one-attempt OpenAI route".into(),
            ));
        }
        if unresolved_environment_reference(&self.api_key).is_some() {
            return Err(ProviderError::Protocol(
                "frozen Responses broker requires a resolved credential".into(),
            ));
        }
        let request = crate::request::PreparedModelRequest::new(
            crate::request::RequestKind::Compatibility,
            self.provider_id.clone(),
            self.model.clone(),
            Vec::new(),
            Vec::new(),
            ChatRequestOptions::default(),
        );
        let source = crate::agent::stream::ProviderStateSourceV1::from_route(
            self.provider_id.clone(),
            self.protocol,
            &self.base_url,
        )?;
        self.stream_adapter_request(
            self.protocol.adapter(),
            crate::agent::provider::AdapterRequest {
                replay: Default::default(),
                diagnostics: Vec::new(),
                path: "responses".into(),
                body,
            },
            &request,
            &source,
            self.trace_hub.as_ref().and_then(|hub| {
                hub.begin_request_with_session(
                    &request,
                    self.protocol.label(),
                    &[self.api_key.as_str()],
                    self.trace_session_id,
                )
            }),
        )
        .await
    }

    /// Traffic-free readiness check for the feature-gated RND-097 broker.
    /// It deliberately returns no credential material or endpoint metadata.
    #[cfg(feature = "planner-executor-experiment")]
    pub(crate) fn validate_frozen_openai_responses_route(&self) -> Result<(), ProviderError> {
        if self.provider_id != "openai"
            || self.protocol != crate::agent::provider::ProviderProtocol::OpenAiResponses
            || self.policy.max_attempts != 1
            || self.api_key.is_empty()
            || unresolved_environment_reference(&self.api_key).is_some()
        {
            return Err(ProviderError::Protocol(
                "frozen Responses broker route is unavailable".into(),
            ));
        }
        Ok(())
    }

    async fn stream_adapter_request(
        &self,
        adapter: crate::agent::provider::ProviderAdapter,
        prepared_http: crate::agent::provider::AdapterRequest,
        request: &crate::request::PreparedModelRequest,
        state_source: &crate::agent::stream::ProviderStateSourceV1,
        trace: Option<crate::provider_trace::ProviderTraceRequest>,
    ) -> Result<mpsc::UnboundedReceiver<ChatStreamItem>, ProviderError> {
        let url = format!("{}/{}", self.base_url, prepared_http.path);
        let tool_call_format = self
            .contract
            .and_then(crate::provider_contract::FirstPartyContract::tool_call_format)
            .or(request.options.tool_call_format);
        if tool_call_format.is_some()
            && self.protocol != crate::agent::provider::ProviderProtocol::ChatCompletions
        {
            return Err(ProviderError::Protocol(
                "tool-call text formats require the chat_completions protocol".into(),
            ));
        }
        crate::agent::stream_anomaly::StreamAnomalyGuard::new(
            request.tools.iter().map(|tool| tool.function.name.as_str()),
        )?;

        let replay = std::sync::Arc::new(prepared_http.replay);
        let attempt_cap = if matches!(self.provider_id.as_str(), "xai" | "xai-oauth") {
            15
        } else {
            2
        };
        let max_attempts = self.policy.max_attempts.clamp(1, attempt_cap);
        let mut attempt = 0usize;
        let opened = open_provider_response(
            self,
            adapter,
            &url,
            &prepared_http.body,
            &replay,
            &mut attempt,
            max_attempts,
            trace.as_ref(),
            None,
        )
        .await?
        .expect("initial request has no receiver to cancel it");
        let (tx, rx) = mpsc::unbounded_channel();
        let retry_client = self.clone();
        let frozen_request = request.clone();
        let frozen_source = state_source.clone();
        let frozen_body = prepared_http.body;
        let stream_trace = trace.clone();

        let privacy_scope = replay.privacy_context.clone();
        tokio::spawn(crate::privacy::scope(privacy_scope, async move {
            let mut opened = opened;
            loop {
                let attempt_started = std::time::Instant::now();
                let body_outcome = consume_provider_body_attempt(
                    opened.response,
                    opened.request_id_sha256,
                    adapter,
                    &frozen_request,
                    &frozen_source,
                    retry_client.contract,
                    retry_client.prompt_cache_contract.usage_format(),
                    tool_call_format,
                    retry_client.policy.first_event_timeout,
                    retry_client.policy.no_progress_timeout,
                    attempt,
                    stream_trace.as_ref(),
                    &tx,
                    &retry_client,
                )
                .await;
                if let BodyAttemptOutcome::Failed {
                    error: ProviderError::Transport(failure),
                    ..
                } = &body_outcome
                {
                    if let Some(key) = retry_client.cooldown_key() {
                        crate::provider_cooldown::record_transport(key, *failure);
                    }
                }
                match body_outcome {
                    BodyAttemptOutcome::Completed => {
                        if let Some(trace) = &stream_trace {
                            trace.outcome(attempt, "completed", None);
                        }
                        return;
                    }
                    BodyAttemptOutcome::ConsumerCancelled => {
                        if let Some(trace) = &stream_trace {
                            trace.outcome(attempt, "consumer_cancelled", None);
                        }
                        return;
                    }
                    BodyAttemptOutcome::Failed { error, committed }
                        if !committed
                            && matches!(error, ProviderError::Transport(_))
                            && attempt < max_attempts =>
                    {
                        if let Some(trace) = &stream_trace {
                            trace.attempt_error(
                                attempt,
                                &error.to_string(),
                                true,
                                Some(retry_client.policy.retry_delay),
                                attempt_started.elapsed(),
                            );
                        }
                        if retry_wait_cancelled(retry_client.policy.retry_delay, Some(&tx)).await {
                            if let Some(trace) = &stream_trace {
                                trace.outcome(attempt, "consumer_cancelled", None);
                            }
                            return;
                        }
                        match open_provider_response(
                            &retry_client,
                            adapter,
                            &url,
                            &frozen_body,
                            &replay,
                            &mut attempt,
                            max_attempts,
                            stream_trace.as_ref(),
                            Some(&tx),
                        )
                        .await
                        {
                            Ok(Some(response)) => opened = response,
                            Ok(None) => {
                                if let Some(trace) = &stream_trace {
                                    trace.outcome(attempt, "consumer_cancelled", None);
                                }
                                return;
                            }
                            Err(error) => {
                                let _ = tx.send(Err(error));
                                return;
                            }
                        }
                    }
                    BodyAttemptOutcome::Failed { error, .. } => {
                        if let Some(trace) = &stream_trace {
                            trace.attempt_error(
                                attempt,
                                &error.to_string(),
                                false,
                                None,
                                attempt_started.elapsed(),
                            );
                            trace.outcome(attempt, "failed", Some(&error.to_string()));
                        }
                        let _ = tx.send(Err(error));
                        return;
                    }
                }
            }
        }));
        Ok(rx)
    }

    pub fn build_messages(
        system_prompt: Option<&str>,
        messages: &[Message],
        streaming: Option<&StreamingState>,
        pasted_images: &[crate::image_content::ModelImage],
    ) -> anyhow::Result<Vec<ChatMessage>> {
        Ok(
            Self::build_segmented_messages(
                system_prompt,
                None,
                messages,
                streaming,
                pasted_images,
            )?
            .messages,
        )
    }

    /// Build the semantic prompt in cache-lifetime order. Leading checkpoint
    /// system messages belong to the stable session epoch; turn-local context
    /// follows that boundary and is frozen by the caller for tool continuations.
    pub fn build_segmented_messages(
        stable_system_prompt: Option<&str>,
        logical_turn_prompt: Option<&str>,
        messages: &[Message],
        _streaming: Option<&StreamingState>,
        pasted_images: &[crate::image_content::ModelImage],
    ) -> anyhow::Result<crate::request::PromptAssembly> {
        // An explicit fresh attachment supersedes only the same legacy
        // attachment section in the outgoing projection. Keep canonical history.
        let mut projected_messages = messages.to_vec();
        let mut refreshed = Vec::new();
        for message in projected_messages.iter_mut().rev() {
            if message.retrieved_spans.is_none() && !refreshed.is_empty() {
                message.content = crate::attachments::remove_legacy_attachment_sections(
                    &message.content,
                    &refreshed,
                );
            }
            if message.role == Role::User {
                if let Some(spans) = message.retrieved_spans.as_ref() {
                    for reference in
                        crate::attachments::refreshed_attachment_references(&message.content, spans)
                    {
                        if !refreshed.contains(&reference) {
                            refreshed.push(reference);
                        }
                    }
                }
            }
        }
        let messages = projected_messages.as_slice();
        let mut result = Vec::new();

        if let Some(sys) = stable_system_prompt {
            result.push(ChatMessage {
                role: "system".into(),
                content: crate::image_content::ModelContent::text(sys),
                tool_calls: None,
                tool_call_id: None,
                response_items: Vec::new(),
            });
        }

        let leading_system_messages = messages
            .iter()
            .take_while(|message| message.role == Role::System)
            .count();
        for message in messages.iter().take(leading_system_messages) {
            result.push(ChatMessage {
                role: "system".into(),
                content: canonical_content(message),
                tool_calls: None,
                tool_call_id: None,
                response_items: Vec::new(),
            });
        }
        let stable_prefix_messages = result.len();

        let append_only_start = result.len();

        // A tool continuation ends in assistant/tool state, but the original
        // turn images still belong to its latest user message. Keeping that
        // multimodal envelope byte-identical is part of the cache epoch.
        let last_user_idx = messages
            .iter()
            .rposition(|message| message.role == Role::User);

        for (i, msg) in messages.iter().enumerate().skip(leading_system_messages) {
            if msg.role == Role::User && Some(i) == last_user_idx && !pasted_images.is_empty() {
                let (text, spans) = crate::image_content::strip_persistence_markers_with_spans(
                    msg.content.clone(),
                    pasted_images,
                    msg.retrieved_spans.as_deref().unwrap_or_default(),
                )?;
                result.push(ChatMessage {
                    role: "user".into(),
                    content: crate::image_content::ModelContent::multimodal(text, pasted_images)
                        .with_retrieved_spans(spans)
                        .with_authored_id(authored_message_key(msg)),
                    tool_calls: None,
                    tool_call_id: None,
                    response_items: Vec::new(),
                });
                if msg.retrieved_spans.is_none() {
                    let content = &mut result.last_mut().expect("image message appended").content;
                    *content = content.clone().with_legacy_provenance();
                }
            } else {
                append_provider_messages(&mut result, msg)?;
            }
        }
        let logical_turn_start = result.len();

        if let Some(turn) = logical_turn_prompt.filter(|turn| !turn.is_empty()) {
            result.push(ChatMessage {
                role: "system".into(),
                content: crate::image_content::ModelContent::text(turn),
                tool_calls: None,
                tool_call_id: None,
                response_items: Vec::new(),
            });
        }

        let mut segments = vec![crate::request::PromptSegment {
            lifetime: crate::request::PromptSegmentLifetime::SessionEpoch,
            messages: 0..stable_prefix_messages,
        }];
        if logical_turn_start > append_only_start {
            segments.push(crate::request::PromptSegment {
                lifetime: crate::request::PromptSegmentLifetime::AppendOnly,
                messages: append_only_start..logical_turn_start,
            });
        }
        if result.len() > logical_turn_start {
            segments.push(crate::request::PromptSegment {
                lifetime: crate::request::PromptSegmentLifetime::LogicalTurn,
                messages: logical_turn_start..result.len(),
            });
        }
        Ok(crate::request::PromptAssembly {
            messages: result,
            stable_prefix_messages,
            segments,
        })
    }
}

fn effective_prompt_cache_plan(
    intent: crate::request::PromptCacheIntent,
    route: &crate::request::RouteCacheIdentity,
    policy: crate::config::PromptCachePolicy,
    request: &crate::request::PreparedModelRequest,
) -> Result<crate::request::EffectivePromptCachePlan, ProviderError> {
    if policy == crate::config::PromptCachePolicy::StablePrefix
        && intent.stable_prefix_messages == 0
    {
        return Err(ProviderError::Protocol(
            "stable-prefix caching requires a non-empty session epoch".into(),
        ));
    }
    let prefix = request
        .messages
        .get(..intent.stable_prefix_messages)
        .ok_or_else(|| ProviderError::Protocol("prompt cache prefix is invalid".into()))?;
    let prefix = serde_json::to_vec(prefix)
        .map_err(|_| ProviderError::Protocol("could not hash prompt cache prefix".into()))?;
    let prefix_sha256 = format!("{:x}", Sha256::digest(prefix));
    let mut tool_hasher = Sha256::new();
    update_tool_digest(&mut tool_hasher, &request.tools);
    let canonical_tools_sha256 = format!("{:x}", tool_hasher.finalize());
    let private_key = (policy != crate::config::PromptCachePolicy::Off)
        .then(|| derive_prompt_cache_key(intent, route, &prefix_sha256, &canonical_tools_sha256));
    let stable_boundary = (policy == crate::config::PromptCachePolicy::StablePrefix)
        .then_some(intent.stable_prefix_messages);
    Ok(crate::request::EffectivePromptCachePlan::new(
        policy,
        private_key,
        stable_boundary,
        prefix_sha256,
        canonical_tools_sha256,
    ))
}

fn derive_prompt_cache_key(
    intent: crate::request::PromptCacheIntent,
    route: &crate::request::RouteCacheIdentity,
    prefix_sha256: &str,
    canonical_tools_sha256: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"ca-prompt-cache-v2\0");
    hasher.update(intent.scope.scope_id().as_bytes());
    hasher.update(b"\0");
    hasher.update(intent.scope.domain().label().as_bytes());
    if let crate::request::PromptCacheDomain::Isolated(kind) = intent.scope.domain() {
        hasher.update(b"\0");
        hasher.update(kind.label().as_bytes());
    }
    hasher.update(b"\0");
    hasher.update(route.provider.as_bytes());
    hasher.update(b"\0");
    hasher.update(route.endpoint_sha256.as_bytes());
    hasher.update(b"\0");
    hasher.update(route.protocol.as_bytes());
    hasher.update(b"\0");
    hasher.update(route.model.as_bytes());
    hasher.update(b"\0");
    hasher.update(route.contract_version.as_bytes());
    hasher.update(b"\0");
    hasher.update(route.request_layout_version.as_bytes());
    hasher.update(b"\0prefix\0");
    hasher.update(prefix_sha256.as_bytes());
    hasher.update(b"\0tools\0");
    hasher.update(canonical_tools_sha256.as_bytes());
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
fn prompt_cache_key_for_request(
    intent: crate::request::PromptCacheIntent,
    source: &crate::agent::stream::ProviderStateSourceV1,
    protocol: crate::agent::provider::ProviderProtocol,
    model: &str,
    contract_id: &str,
    request_layout_version: &str,
    request: &crate::request::PreparedModelRequest,
) -> Result<String, ProviderError> {
    let route = crate::request::RouteCacheIdentity {
        provider: source.provider.clone(),
        endpoint_sha256: source.endpoint_sha256.clone(),
        protocol: protocol.label().into(),
        model: model.into(),
        contract_version: contract_id.into(),
        request_layout_version: request_layout_version.into(),
    };
    effective_prompt_cache_plan(
        intent,
        &route,
        crate::config::PromptCachePolicy::Automatic,
        request,
    )?
    .private_key()
    .map(str::to_owned)
    .ok_or_else(|| ProviderError::Protocol("prompt cache key is disabled".into()))
}

fn update_tool_digest(hasher: &mut Sha256, tools: &[ToolDef]) {
    for tool in tools {
        hasher.update(tool.def_type.as_bytes());
        hasher.update(b"\0");
        hasher.update(tool.function.name.as_bytes());
        hasher.update(b"\0");
        hasher.update(tool.function.description.as_bytes());
        hasher.update(b"\0");
        update_canonical_json_digest(hasher, &tool.function.parameters);
        hasher.update(b"\0");
    }
}

fn update_canonical_json_digest(hasher: &mut Sha256, value: &serde_json::Value) {
    match value {
        serde_json::Value::Null => hasher.update(b"n"),
        serde_json::Value::Bool(value) => hasher.update(if *value { b"t" } else { b"f" }),
        serde_json::Value::Number(value) => hasher.update(value.to_string().as_bytes()),
        serde_json::Value::String(value) => {
            hasher.update(b"s");
            hasher.update(value.as_bytes());
        }
        serde_json::Value::Array(values) => {
            hasher.update(b"[");
            for value in values {
                update_canonical_json_digest(hasher, value);
                hasher.update(b",");
            }
            hasher.update(b"]");
        }
        serde_json::Value::Object(values) => {
            hasher.update(b"{");
            let mut keys = values.keys().collect::<Vec<_>>();
            keys.sort_unstable();
            for key in keys {
                hasher.update(key.as_bytes());
                hasher.update(b":");
                update_canonical_json_digest(hasher, &values[key]);
                hasher.update(b",");
            }
            hasher.update(b"}");
        }
    }
}

struct OpenedProviderResponse {
    response: reqwest::Response,
    request_id_sha256: Option<String>,
}

enum BodyAttemptOutcome {
    Completed,
    ConsumerCancelled,
    Failed {
        error: ProviderError,
        committed: bool,
    },
}

enum BodyAttemptAbort {
    ConsumerCancelled,
    Provider(ProviderError),
}

async fn retry_wait_cancelled(
    delay: std::time::Duration,
    cancellation: Option<&mpsc::UnboundedSender<ChatStreamItem>>,
) -> bool {
    if let Some(cancellation) = cancellation {
        tokio::select! {
            biased;
            _ = cancellation.closed() => true,
            _ = tokio::time::sleep(delay) => false,
        }
    } else {
        tokio::time::sleep(delay).await;
        false
    }
}

#[allow(clippy::too_many_arguments)]
async fn open_provider_response(
    client: &AgentClient,
    adapter: crate::agent::provider::ProviderAdapter,
    url: &str,
    body: &[u8],
    replay: &crate::provider_replay::ReplayBindings,
    attempt: &mut usize,
    max_attempts: usize,
    trace: Option<&crate::provider_trace::ProviderTraceRequest>,
    cancellation: Option<&mpsc::UnboundedSender<ChatStreamItem>>,
) -> Result<Option<OpenedProviderResponse>, ProviderError> {
    loop {
        if cancellation.is_some_and(mpsc::UnboundedSender::is_closed) {
            return Ok(None);
        }
        *attempt += 1;
        let attempt_started = std::time::Instant::now();
        let response = execute_provider_request(
            client,
            adapter,
            url,
            body,
            replay,
            *attempt,
            trace,
            cancellation,
        )
        .await;
        match response {
            Ok(None) => return Ok(None),
            Ok(Some(response)) if response.status().is_success() => {
                if let Some(key) = client.cooldown_key() {
                    crate::provider_cooldown::clear_success(&key, attempt_started);
                }
                if let Some(trace) = trace {
                    trace.attempt_response(
                        *attempt,
                        response.status().as_u16(),
                        response.headers(),
                        false,
                        None,
                        attempt_started.elapsed(),
                    );
                }
                let request_id_sha256 =
                    match crate::agent::provider::provider_request_id_sha256(response.headers()) {
                        Ok(value) => value,
                        Err(error) => {
                            if let Some(trace) = trace {
                                trace.outcome(*attempt, "failed", Some(&error.to_string()));
                            }
                            return Err(error);
                        }
                    };
                return Ok(Some(OpenedProviderResponse {
                    response,
                    request_id_sha256,
                }));
            }
            Ok(Some(response)) => {
                let status = response.status().as_u16();
                let retry = *attempt < max_attempts && retryable_status(status);
                let delay = retry.then(|| retry_delay(&response, &client.policy));
                if let Some(trace) = trace {
                    trace.attempt_response(
                        *attempt,
                        status,
                        response.headers(),
                        retry,
                        delay,
                        attempt_started.elapsed(),
                    );
                }
                let body_excerpt = match read_error_body(
                    response,
                    client.policy.headers_timeout,
                    cancellation,
                    client,
                )
                .await
                {
                    ErrorBodyRead::Cancelled => return Ok(None),
                    ErrorBodyRead::Stopped(error) => {
                        if let Some(trace) = trace {
                            trace.outcome(*attempt, "failed", Some(&error.to_string()));
                        }
                        return Err(error);
                    }
                    ErrorBodyRead::Excerpt(excerpt) => excerpt,
                };
                if client.consumer_chat_shim()
                    && status == 401
                    && *attempt == 1
                    && refresh_consumer_shim_auth().await
                {
                    continue;
                }
                if let Some(message) =
                    chatgpt_plan_usage_message(&client.provider_id, status, body_excerpt.as_deref())
                {
                    let error = ProviderError::http_with_excerpt(status, message);
                    if let Some(trace) = trace {
                        trace.outcome(*attempt, "failed", Some(&error.to_string()));
                    }
                    return Err(error);
                }
                let error = match body_excerpt {
                    Some(excerpt) => ProviderError::http_with_excerpt(status, excerpt),
                    None => ProviderError::http(status),
                };
                if let Some(delay) = delay {
                    if retry_wait_cancelled(delay, cancellation).await {
                        return Ok(None);
                    }
                } else {
                    if let Some(trace) = trace {
                        trace.outcome(*attempt, "failed", Some(&error.to_string()));
                    }
                    return Err(error);
                }
            }
            Err(error) => {
                if let ProviderError::Transport(failure) = &error {
                    if let Some(key) = client.cooldown_key() {
                        crate::provider_cooldown::record_transport(key, *failure);
                    }
                }
                let retry = *attempt < max_attempts && matches!(error, ProviderError::Transport(_));
                let delay = retry.then_some(client.policy.retry_delay);
                if let Some(trace) = trace {
                    trace.attempt_error(
                        *attempt,
                        &error.to_string(),
                        retry,
                        delay,
                        attempt_started.elapsed(),
                    );
                }
                if let Some(delay) = delay {
                    if retry_wait_cancelled(delay, cancellation).await {
                        return Ok(None);
                    }
                } else {
                    if let Some(trace) = trace {
                        trace.outcome(*attempt, "failed", Some(&error.to_string()));
                    }
                    return Err(error);
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn execute_provider_request(
    client: &AgentClient,
    adapter: crate::agent::provider::ProviderAdapter,
    url: &str,
    body: &[u8],
    replay: &crate::provider_replay::ReplayBindings,
    attempt: usize,
    trace: Option<&crate::provider_trace::ProviderTraceRequest>,
    cancellation: Option<&mpsc::UnboundedSender<ChatStreamItem>>,
) -> Result<Option<reqwest::Response>, ProviderError> {
    let request = client
        .client
        .post(url)
        .header("Content-Type", "application/json");
    let request = adapter.apply_auth(
        request,
        if client.google_auth.is_some() {
            ""
        } else {
            &client.api_key
        },
    );
    let request = if let Some(account_id) = &client.codex_account_id {
        request.header("ChatGPT-Account-ID", account_id)
    } else {
        request
    };
    let request = if let Some(client_request_id) = &client.openai_client_request_id {
        request.header("X-Client-Request-Id", client_request_id)
    } else {
        request
    };
    let request = if crate::prose_recovery::xai_requests_doom_loop_check(&client.provider_id, url) {
        request
            .header(
                crate::prose_recovery::DOOM_LOOP_CHECK_HEADER,
                crate::prose_recovery::DOOM_LOOP_WINDOW_TOKENS,
            )
            .header(
                crate::prose_recovery::EXACT_REPETITION_CHECK_HEADER,
                crate::prose_recovery::EXACT_REPETITION_MIN_TOKENS,
            )
    } else {
        request
    };
    let filter = client.active_filter();
    if client.request_store.privacy_pending() {
        return Err(ProviderError::PrivacyBlocked {
            detail: "Project privacy changed while this request was prepared; retry after loading"
                .into(),
        });
    }
    if let Some(context) = client.privacy_context() {
        tokio::task::spawn_blocking(move || context.refresh_policy())
            .await
            .map_err(|_| ProviderError::PrivacyBlocked {
                detail: "Project privacy rules could not be refreshed before transmission".into(),
            })?
            .map_err(|detail| ProviderError::PrivacyBlocked { detail })?;
    }
    if !client.consumer_chat_shim()
        && client
            .privacy_context()
            .is_some_and(|context| context.enabled())
        && replay.privacy_context.is_none()
    {
        return Err(ProviderError::PrivacyBlocked {
            detail: "This provider path lacks a project privacy projection receipt".into(),
        });
    }
    if replay.privacy_context.is_some()
        && replay
            .privacy_body
            .as_ref()
            .is_none_or(|receipt| !receipt.matches(body))
    {
        return Err(ProviderError::PrivacyBlocked {
            detail: "Prepared request changed after privacy projection; prepare it again".into(),
        });
    }
    let mut inspected_private_revision = None;
    let (body, diagnostics) = if let Some(context) = replay.privacy_context.clone() {
        let original = replay.privacy_original.clone();
        let revision = replay
            .privacy_revision
            .ok_or_else(|| ProviderError::PrivacyBlocked {
                detail: "Request lacks its privacy projection receipt".into(),
            })?;
        let raw = replay
            .privacy_body
            .as_ref()
            .expect("privacy prepared body receipt was checked above")
            .bytes();
        let bindings = replay.clone();
        let filter = filter.clone();
        let (body, diagnostics, private_revision) = tokio::task::spawn_blocking(move || {
            let projection = |bytes: &[u8]| context.project_task_wire(bytes).map_err(|detail|ProviderError::PrivacyBlocked{detail});
            let (body, diagnostics, host) = if let Some(host) = bindings.privacy_body.as_ref().and_then(|receipt| receipt.host_instructions()) {
                let (body, diagnostics, host) = crate::provider_replay::prepare_outbound_with_host(&raw, &bindings, &filter, context.has_task_names().then_some(&projection as &crate::provider_replay::OutboundProjection<'_>), host.clone())?;
                (body, diagnostics, Some(host))
            } else {
            let (body, diagnostics) = if context.has_task_names() {
                crate::provider_replay::prepare_outbound_with_projection(&raw, &bindings, &filter,&|bytes|context.project_task_wire(bytes).map_err(|detail|ProviderError::PrivacyBlocked{detail}))?
            } else {crate::provider_replay::prepare_outbound(&raw,&bindings,&filter)?};
                (body, diagnostics, None)
            };
            if bindings.privacy_policy_digest.as_deref() != Some(context.policy_fingerprint().as_str()) {
                return Err(ProviderError::PrivacyBlocked {
                    detail: "Prepared request belongs to different privacy rules; prepare it again".into(),
                });
            }
            let private_revision = context.private_revision();
            context
                .validate_wire_with_receipts(&body, revision, bindings.privacy_policy_digest.as_deref().unwrap_or_default(),
                    bindings.privacy_body.as_ref().and_then(|receipt| receipt.structured_output()), host.as_ref())
                .map_err(|detail| ProviderError::PrivacyBlocked { detail })?;
            context
                .publish_wire(original.as_deref().unwrap_or(&raw), &body)
                .map_err(|detail| ProviderError::PrivacyBlocked { detail })?;
            if context.task_filter().is_some() {
                // URL aliases can be allocated during final wire rewriting.
                // Their receipt must be durable before bytes leave this process.
                context.commit_registry().map_err(|detail| ProviderError::PrivacyBlocked { detail })?;
            }
            if context.private_revision() != private_revision {
                return Err(ProviderError::PrivacyBlocked { detail: "New private values were discovered during inspection; prepare this request again".into() });
            }
            Ok::<_, ProviderError>((body, diagnostics, private_revision))
        })
        .await
        .map_err(|_| ProviderError::PrivacyBlocked {
            detail: "Final privacy inspection worker stopped".into(),
        })??;
        inspected_private_revision = Some(private_revision);
        (body, diagnostics)
    } else if crate::privacy::tasks::remote_binding().is_some() {
        let body = body.to_vec();
        let replay = replay.clone();
        let filter = filter.clone();
        tokio::task::spawn_blocking(move || {
            crate::provider_replay::prepare_outbound_with_projection(
                &body,
                &replay,
                &filter,
                &|bytes| {
                    crate::privacy::tasks::project_remote_wire_blocking(bytes)
                        .map_err(|detail| ProviderError::PrivacyBlocked { detail })
                },
            )
        })
        .await
        .map_err(|_| ProviderError::PrivacyBlocked {
            detail: "Private replay projection worker stopped".into(),
        })??
    } else {
        crate::provider_replay::prepare_outbound(body, replay, &filter)?
    };
    crate::privacy::tasks::validate_remote_wire(&body)
        .await
        .map_err(|detail| ProviderError::PrivacyBlocked { detail })?;
    client
        .request_store
        .adapter_diagnostics(replay.request_sequence, &diagnostics);
    if let Some(trace) = trace {
        trace.diagnostics(&diagnostics);
    }
    let sent_prefix = if adapter.protocol() == crate::agent::ProviderProtocol::GoogleGenerateContent
    {
        Some(crate::provider_replay::SentPrefix(
            crate::google_provider::sent_prefix(&body)?,
        ))
    } else if adapter.protocol() == crate::agent::ProviderProtocol::AnthropicMessages {
        Some(crate::provider_replay::SentPrefix(
            crate::provider_replay::prefix_digest(&crate::provider_replay::prefix_from_body(&body)),
        ))
    } else if adapter.protocol() == crate::agent::ProviderProtocol::OpenAiResponses
        && (replay.privacy_context.is_some() || crate::privacy::tasks::remote_binding().is_some())
    {
        Some(crate::provider_replay::SentPrefix(
            crate::provider_replay::responses_prefix(&body, None)?,
        ))
    } else {
        None
    };
    if filter.shows_banner() || replay.privacy_context.is_some() {
        if matches!(
            adapter.protocol(),
            crate::agent::ProviderProtocol::AnthropicMessages
                | crate::agent::ProviderProtocol::GoogleGenerateContent
        ) || (adapter.protocol() == crate::agent::ProviderProtocol::OpenAiResponses
            && replay.privacy_context.is_some())
        {
            crate::provider_filter::publish_outbound_body(
                &crate::provider_trace::native_diagnostic_body(&body, &[client.api_key.as_str()]),
            );
        } else {
            crate::provider_filter::publish_outbound_body(&body);
        }
    }
    let headers_deadline = tokio::time::Instant::now() + client.policy.headers_timeout;
    let request = if let Some(auth) = &client.google_auth {
        let authentication = tokio::time::timeout_at(headers_deadline, auth.headers());
        let result = if let Some(cancellation) = cancellation {
            tokio::select! { biased; _=cancellation.closed()=>return Ok(None), error=client.until_filter_tripped()=>return Err(error), result=authentication=>result }
        } else {
            tokio::select! { biased; error=client.until_filter_tripped()=>return Err(error), result=authentication=>result }
        };
        let headers =
            result.map_err(|_| ProviderError::Transport(ProviderTransportFailure::Timeout))??;
        request.headers(headers)
    } else {
        request
    };
    let (wire, compressed) = compressed_request_body(&client.base_url, &body);
    let request = if compressed {
        request.header("content-encoding", "zstd")
    } else {
        request
    };
    let request = request
        .body(wire)
        .build()
        .map_err(|error| ProviderError::Transport(classify_transport_error(&error)))?;
    if let Some(context) = replay
        .privacy_context
        .clone()
        .or_else(|| client.privacy_context())
    {
        let revision = replay.privacy_revision;
        let fingerprint = replay.privacy_policy_digest.clone();
        let projected = replay.privacy_context.is_some();
        let plain_chat = client.consumer_chat_shim();
        tokio::task::spawn_blocking(move || {
            context.refresh_policy()?;
            if context.enabled() && !projected && !plain_chat {
                return Err(
                    "Project privacy was enabled after preparation; retry this request".into(),
                );
            }
            if let Some(revision) = revision {
                context.check_policy(revision)?;
                if fingerprint.as_deref() != Some(context.policy_fingerprint().as_str()) {
                    return Err(
                        "Privacy rules changed after request preparation; prepare it again".into(),
                    );
                }
                if inspected_private_revision != Some(context.private_revision()) {
                    return Err(
                        "Private values changed before transmission; retry this request".into(),
                    );
                }
            }
            Ok::<_, String>(())
        })
        .await
        .map_err(|_| ProviderError::PrivacyBlocked {
            detail: "Project privacy worker stopped before transmission".into(),
        })?
        .map_err(|detail| ProviderError::PrivacyBlocked { detail })?;
    }
    if client.request_store.privacy_pending() {
        return Err(ProviderError::PrivacyBlocked {
            detail: "Project privacy changed before transmission; retry this request".into(),
        });
    }
    if let Some(trace) = trace {
        trace.attempt_started(
            attempt,
            request.method().as_str(),
            request.url().as_str(),
            request.headers(),
            &body,
        );
    }
    let execution = tokio::time::timeout_at(headers_deadline, client.client.execute(request));
    let response = if let Some(cancellation) = cancellation {
        tokio::select! {
            biased;
            _ = cancellation.closed() => return Ok(None),
            error = client.until_filter_tripped() => return Err(error),
            response = execution => response,
        }
    } else {
        tokio::select! {
            biased;
            error = client.until_filter_tripped() => return Err(error),
            response = execution => response,
        }
    };
    match response {
        Err(_) => Err(ProviderError::Transport(ProviderTransportFailure::Timeout)),
        Ok(Err(error)) => Err(ProviderError::Transport(classify_transport_error(&error))),
        Ok(Ok(mut response)) => {
            if let Some(prefix) = sent_prefix {
                response.extensions_mut().insert(prefix);
            }
            Ok(Some(response))
        }
    }
}

fn chatgpt_plan_usage_message(
    provider_id: &str,
    status: u16,
    excerpt: Option<&str>,
) -> Option<String> {
    if provider_id != "openai-chatgpt" {
        return None;
    }
    let verdict =
        crate::chatgpt_plan_auth::classify_chatgpt_plan_error(status, excerpt.unwrap_or(""));
    verdict.is_usage_limit.then_some(verdict.user_message)
}

/// Semantic content needs its own carry: SSE framing separates two deltas of
/// one word. Content, reasoning, and distinct calls must never form one match.
#[derive(Default)]
struct ResponseFilterChannels {
    text: Vec<u8>,
    reasoning: Vec<u8>,
    tool_arguments: std::collections::BTreeMap<usize, FilteredToolArguments>,
}

#[derive(Default)]
struct FilteredToolArguments {
    raw: Vec<u8>,
    strings: JsonArgumentStringFilter,
}

impl FilteredToolArguments {
    fn observe(
        &mut self,
        filter: &crate::provider_filter::ProviderFilter,
        argument: &str,
    ) -> Result<(), ProviderError> {
        filter.observe_response(&mut self.raw, argument.as_bytes())?;
        self.strings.observe(filter, argument)?;
        Ok(())
    }

    fn finish(
        &mut self,
        filter: &crate::provider_filter::ProviderFilter,
    ) -> Result<(), ProviderError> {
        filter.observe_response(&mut self.raw, &[])?;
        if filter.watches() {
            self.strings.flush_fragment(filter, true)?;
            filter.observe_response(&mut self.strings.decoded, &[])?;
        }
        Ok(())
    }
}

/// Decode JSON strings incrementally so an argument object may exceed the
/// rolling scan window. Each quoted key/value gets a separate decoded carry.
#[derive(Default)]
struct JsonArgumentStringFilter {
    in_string: bool,
    escaped: bool,
    fragment: String,
    decoded: Vec<u8>,
    inactive_escape: [u8; 12],
    inactive_escape_len: usize,
    inactive_unicode_digits: u8,
    inactive_high_surrogate: bool,
}

impl JsonArgumentStringFilter {
    const FRAGMENT_BYTES: usize = 512;
    // A split Unicode surrogate pair needs at most two six-byte escapes.
    const ESCAPE_SLACK: usize = 12;

    fn observe(
        &mut self,
        filter: &crate::provider_filter::ProviderFilter,
        argument: &str,
    ) -> Result<(), ProviderError> {
        if !filter.watches() {
            self.observe_inactive(argument);
            return Ok(());
        }
        if self.inactive_escape_len > 0 {
            if let Ok(escape) =
                std::str::from_utf8(&self.inactive_escape[..self.inactive_escape_len])
            {
                self.fragment.push_str(escape);
            }
            self.inactive_escape_len = 0;
            self.inactive_unicode_digits = 0;
            self.inactive_high_surrogate = false;
        }
        for ch in argument.chars() {
            if !self.in_string {
                if ch == '"' {
                    self.in_string = true;
                    self.escaped = false;
                    self.fragment.clear();
                    self.decoded.clear();
                }
                continue;
            }
            if ch == '"' && !self.escaped {
                self.flush_fragment(filter, true)?;
                filter.observe_response(&mut self.decoded, &[])?;
                self.in_string = false;
                self.decoded.clear();
                continue;
            }
            self.fragment.push(ch);
            self.escaped = ch == '\\' && !self.escaped;
            if self.fragment.len() >= Self::FRAGMENT_BYTES && !self.escaped {
                self.flush_fragment(filter, false)?;
            }
        }
        self.flush_fragment(filter, false)
    }

    /// While off, retain only quote/escape state and a fixed pending escape.
    /// This neither allocates content buffers nor validates unfiltered JSON.
    fn observe_inactive(&mut self, argument: &str) {
        self.fragment.clear();
        self.decoded.clear();
        for ch in argument.chars() {
            if !self.in_string {
                if ch == '"' {
                    self.in_string = true;
                    self.escaped = false;
                    self.inactive_escape_len = 0;
                }
            } else if self.escaped {
                self.push_inactive_escape(ch);
                self.escaped = false;
                if ch == 'u' {
                    self.inactive_unicode_digits = 4;
                } else {
                    self.inactive_escape_len = 0;
                }
            } else if self.inactive_unicode_digits > 0 {
                self.push_inactive_escape(ch);
                self.inactive_unicode_digits -= 1;
                if self.inactive_unicode_digits == 0 {
                    self.inactive_high_surrogate = self.inactive_escape_len >= 4
                        && std::str::from_utf8(
                            &self.inactive_escape
                                [self.inactive_escape_len - 4..self.inactive_escape_len],
                        )
                        .ok()
                        .and_then(|digits| u16::from_str_radix(digits, 16).ok())
                        .is_some_and(|value| (0xD800..=0xDBFF).contains(&value));
                    if !self.inactive_high_surrogate {
                        self.inactive_escape_len = 0;
                    }
                }
            } else if ch == '\\' {
                if !self.inactive_high_surrogate {
                    self.inactive_escape_len = 0;
                }
                self.inactive_high_surrogate = false;
                self.push_inactive_escape(ch);
                self.escaped = true;
            } else {
                self.inactive_escape_len = 0;
                self.inactive_high_surrogate = false;
                if ch == '"' {
                    self.in_string = false;
                }
            }
        }
    }

    fn push_inactive_escape(&mut self, ch: char) {
        if ch.is_ascii() && self.inactive_escape_len < self.inactive_escape.len() {
            self.inactive_escape[self.inactive_escape_len] = ch as u8;
            self.inactive_escape_len += 1;
        } else {
            self.inactive_escape_len = 0;
        }
    }

    fn flush_fragment(
        &mut self,
        filter: &crate::provider_filter::ProviderFilter,
        complete: bool,
    ) -> Result<(), ProviderError> {
        if self.fragment.is_empty() {
            return Ok(());
        }
        match serde_json::from_str::<String>(&format!("\"{}\"", self.fragment)) {
            Ok(decoded) => {
                filter.observe_response(&mut self.decoded, decoded.as_bytes())?;
                self.fragment.clear();
            }
            Err(_)
                if !complete
                    && self.fragment.len() <= Self::FRAGMENT_BYTES + Self::ESCAPE_SLACK => {}
            Err(_) => {
                return Err(ProviderError::MalformedData(
                    "invalid JSON string escape in filtered tool arguments".into(),
                ))
            }
        }
        Ok(())
    }
}

impl ResponseFilterChannels {
    fn observe(
        &mut self,
        filter: &crate::provider_filter::ProviderFilter,
        events: &[ProviderEvent],
    ) -> Result<(), ProviderError> {
        for event in events {
            match event {
                ProviderEvent::Text(text) => {
                    filter.observe_response(&mut self.text, text.as_bytes())?;
                }
                ProviderEvent::Reasoning(text) => {
                    filter.observe_response(&mut self.reasoning, text.as_bytes())?;
                }
                ProviderEvent::ToolCallStart { name, .. } => {
                    filter.observe_response(&mut Vec::new(), name.as_bytes())?;
                }
                ProviderEvent::ToolCallArg { index, arg } => {
                    self.tool_arguments
                        .entry(*index)
                        .or_default()
                        .observe(filter, arg)?;
                }
                ProviderEvent::Done => self.finish(filter)?,
                _ => {}
            }
        }
        Ok(())
    }

    fn finish(
        &mut self,
        filter: &crate::provider_filter::ProviderFilter,
    ) -> Result<(), ProviderError> {
        filter.observe_response(&mut self.text, &[])?;
        filter.observe_response(&mut self.reasoning, &[])?;
        for arguments in self.tool_arguments.values_mut() {
            arguments.finish(filter)?;
        }
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)]
async fn consume_provider_body_attempt(
    response: reqwest::Response,
    request_id_sha256: Option<String>,
    adapter: crate::agent::provider::ProviderAdapter,
    request: &crate::request::PreparedModelRequest,
    state_source: &crate::agent::stream::ProviderStateSourceV1,
    contract: Option<crate::provider_contract::FirstPartyContract>,
    cache_usage_format: Option<crate::config::PromptCacheUsageFormat>,
    tool_call_format: Option<crate::config::ToolCallFormat>,
    first_event_timeout: std::time::Duration,
    no_progress_timeout: std::time::Duration,
    attempt: usize,
    trace: Option<&crate::provider_trace::ProviderTraceRequest>,
    tx: &mpsc::UnboundedSender<ChatStreamItem>,
    client: &AgentClient,
) -> BodyAttemptOutcome {
    let mut decoder = adapter.decoder_with_cache_usage(
        request,
        state_source,
        contract,
        request_id_sha256,
        cache_usage_format,
    );

    if let Some(prefix) = response
        .extensions()
        .get::<crate::provider_replay::SentPrefix>()
    {
        decoder.bind_sent_prefix(prefix);
    }
    let mut stream = response.bytes_stream();
    let mut anomaly_guard = match crate::agent::stream_anomaly::StreamAnomalyGuard::new(
        request.tools.iter().map(|tool| tool.function.name.as_str()),
    ) {
        Ok(guard) => guard,
        Err(error) => {
            return BodyAttemptOutcome::Failed {
                error,
                committed: false,
            };
        }
    };
    let mut dsml_normalizer = crate::agent::dsml::DsmlStreamNormalizer::new(tool_call_format);
    let mut trace_frames = crate::provider_trace::RawTraceFrameBuffer::default();
    let mut buffered = Vec::new();
    let mut committed = false;
    let mut response_carry = Vec::new();
    let mut response_channels = ResponseFilterChannels::default();
    let mut deadline = tokio::time::Instant::now() + first_event_timeout;
    let mut received_bytes = false;

    loop {
        let next = tokio::select! {
            biased;
            _ = tx.closed() => return BodyAttemptOutcome::ConsumerCancelled,
            error = client.until_filter_tripped() => {
                return BodyAttemptOutcome::Failed {
                    error,
                    committed,
                };
            }
            next = tokio::time::timeout_at(deadline, stream.next()) => next,
        };
        let chunk = match next {
            Ok(Some(chunk)) => chunk,
            Ok(None) => break,
            Err(_) => {
                return BodyAttemptOutcome::Failed {
                    error: ProviderError::Transport(if received_bytes {
                        ProviderTransportFailure::Timeout
                    } else {
                        ProviderTransportFailure::FirstTokenTimeout
                    }),
                    committed,
                };
            }
        };
        let bytes = match chunk {
            Ok(bytes) => bytes,
            Err(error) => {
                return BodyAttemptOutcome::Failed {
                    error: ProviderError::Transport(classify_transport_error(&error)),
                    committed,
                };
            }
        };
        // Any received body bytes prove the stream is alive. Keep-alive SSE
        // comments and partial lines decode to zero events; without this reset
        // a live thinking/prefill stream still trips no_progress_timeout.
        if !bytes.is_empty() {
            received_bytes = true;
            deadline = tokio::time::Instant::now() + no_progress_timeout;
        }
        let filter = client.active_filter();
        if let Err(error) = filter.observe_response(&mut response_carry, &bytes) {
            return BodyAttemptOutcome::Failed { error, committed };
        }
        if let Some(trace) = trace {
            match trace_frames.push(&bytes) {
                Ok(frames) => {
                    for frame in frames {
                        trace.raw_frame(attempt, &frame);
                    }
                }
                Err(()) => trace.raw_frame(attempt, b"data: [frame exceeded trace bound]"),
            }
        }
        let events = match decoder.push(&bytes) {
            Ok(events) => events,
            Err(error) => return BodyAttemptOutcome::Failed { error, committed },
        };
        if !events.is_empty() {
            deadline = tokio::time::Instant::now() + no_progress_timeout;
        }
        if let Err(error) = response_channels.observe(filter.as_ref(), &events) {
            return BodyAttemptOutcome::Failed { error, committed };
        }
        if let Err(abort) = process_provider_events(
            events,
            &mut dsml_normalizer,
            &mut anomaly_guard,
            &mut buffered,
            &mut committed,
            attempt,
            trace,
            tx,
        ) {
            return match abort {
                BodyAttemptAbort::ConsumerCancelled => BodyAttemptOutcome::ConsumerCancelled,
                BodyAttemptAbort::Provider(error) => {
                    BodyAttemptOutcome::Failed { error, committed }
                }
            };
        }
    }
    if let (Some(trace), Some(frame)) = (trace, trace_frames.finish()) {
        trace.raw_frame(attempt, &frame);
    }
    let events = match decoder.finish() {
        Ok(events) => events,
        Err(error) => return BodyAttemptOutcome::Failed { error, committed },
    };
    let filter = client.active_filter();
    if let Err(error) = filter
        .observe_response(&mut response_carry, &[])
        .and_then(|()| response_channels.observe(filter.as_ref(), &events))
        .and_then(|()| response_channels.finish(filter.as_ref()))
    {
        return BodyAttemptOutcome::Failed { error, committed };
    }
    if let Err(abort) = process_provider_events(
        events,
        &mut dsml_normalizer,
        &mut anomaly_guard,
        &mut buffered,
        &mut committed,
        attempt,
        trace,
        tx,
    ) {
        return match abort {
            BodyAttemptAbort::ConsumerCancelled => BodyAttemptOutcome::ConsumerCancelled,
            BodyAttemptAbort::Provider(error) => BodyAttemptOutcome::Failed { error, committed },
        };
    }
    for event in buffered {
        if tx.send(Ok(event)).is_err() {
            return BodyAttemptOutcome::ConsumerCancelled;
        }
    }
    BodyAttemptOutcome::Completed
}

#[allow(clippy::too_many_arguments)]
fn process_provider_events(
    events: Vec<ProviderEvent>,
    dsml_normalizer: &mut crate::agent::dsml::DsmlStreamNormalizer,
    anomaly_guard: &mut crate::agent::stream_anomaly::StreamAnomalyGuard,
    buffered: &mut Vec<ProviderEvent>,
    committed: &mut bool,
    attempt: usize,
    trace: Option<&crate::provider_trace::ProviderTraceRequest>,
    tx: &mpsc::UnboundedSender<ChatStreamItem>,
) -> Result<(), BodyAttemptAbort> {
    for event in events {
        // A text-format normalizer may hold a nonempty prefix internally.
        // Receiving that provider output still makes physical replay unsafe.
        if !*committed && crate::agent::stream::provider_event_has_usable_output(&event) {
            *committed = true;
            for buffered_event in buffered.drain(..) {
                tx.send(Ok(buffered_event))
                    .map_err(|_| BodyAttemptAbort::ConsumerCancelled)?;
            }
        }
        let normalized = dsml_normalizer
            .normalize(event)
            .map_err(BodyAttemptAbort::Provider)?;
        for event in normalized {
            anomaly_guard
                .inspect(&event)
                .map_err(BodyAttemptAbort::Provider)?;
            if let Some(trace) = trace {
                trace.decoded_event(attempt, &event);
            }
            if !*committed {
                if crate::agent::stream::provider_event_has_usable_output(&event) {
                    *committed = true;
                    for buffered_event in buffered.drain(..) {
                        tx.send(Ok(buffered_event))
                            .map_err(|_| BodyAttemptAbort::ConsumerCancelled)?;
                    }
                } else {
                    buffered.push(event);
                    continue;
                }
            }
            tx.send(Ok(event))
                .map_err(|_| BodyAttemptAbort::ConsumerCancelled)?;
        }
    }
    Ok(())
}

fn unresolved_environment_reference(value: &str) -> Option<&str> {
    value
        .strip_prefix("${")
        .and_then(|value| value.strip_suffix('}'))
        .filter(|value| {
            !value.is_empty()
                && value
                    .chars()
                    .all(|character| character == '_' || character.is_ascii_alphanumeric())
        })
}

fn classify_transport_error(error: &reqwest::Error) -> ProviderTransportFailure {
    if error.is_timeout() {
        ProviderTransportFailure::Timeout
    } else if error.is_connect() {
        ProviderTransportFailure::Connect
    } else {
        ProviderTransportFailure::Request
    }
}

const ERROR_BODY_EXCERPT_BYTES: usize = 512;
const ERROR_BODY_READ_LIMIT: usize = 8 * 1024;
const ERROR_BODY_ABSOLUTE_CAP: std::time::Duration = std::time::Duration::from_secs(1);

enum ErrorBodyRead {
    Excerpt(Option<String>),
    Cancelled,
    Stopped(ProviderError),
}

/// Read a provider error body without hanging the retry loop.
///
/// A stall uses `headers_timeout`. Arriving bytes do not extend the one-second
/// cap. Reading stops at 8 KiB and drops the socket. Only a JSON error message
/// is kept. Response-filter matches become a stop and are not copied out.
async fn read_error_body(
    response: reqwest::Response,
    headers_timeout: std::time::Duration,
    cancellation: Option<&mpsc::UnboundedSender<ChatStreamItem>>,
    client: &AgentClient,
) -> ErrorBodyRead {
    let status = response.status().as_u16();
    let headers = response.headers().clone();
    let recorded = client
        .cooldown_key()
        .and_then(|key| crate::provider_cooldown::record_http(key, status, &headers));
    let absolute = tokio::time::Instant::now() + ERROR_BODY_ABSOLUTE_CAP;
    let mut stream = response.bytes_stream();
    let mut collected = Vec::new();
    let mut carry = Vec::new();
    let mut reached_eof = false;
    loop {
        if collected.len() >= ERROR_BODY_READ_LIMIT || tokio::time::Instant::now() >= absolute {
            break;
        }
        let wait_until = (tokio::time::Instant::now() + headers_timeout).min(absolute);
        let next = if let Some(cancellation) = cancellation {
            tokio::select! {
                biased;
                _ = cancellation.closed() => return ErrorBodyRead::Cancelled,
                error = client.until_filter_tripped() => return ErrorBodyRead::Stopped(error),
                next = tokio::time::timeout_at(wait_until, stream.next()) => next,
            }
        } else {
            tokio::select! {
                biased;
                error = client.until_filter_tripped() => return ErrorBodyRead::Stopped(error),
                next = tokio::time::timeout_at(wait_until, stream.next()) => next,
            }
        };
        match next {
            Ok(Some(Ok(chunk))) if !chunk.is_empty() => {
                if let Err(error) = client.active_filter().observe_response(&mut carry, &chunk) {
                    return ErrorBodyRead::Stopped(error);
                }
                let room = ERROR_BODY_READ_LIMIT - collected.len();
                let take = room.min(chunk.len());
                collected.extend_from_slice(&chunk[..take]);
            }
            Ok(Some(Ok(_))) => {}
            Ok(None) => {
                reached_eof = true;
                break;
            }
            Ok(Some(Err(_))) | Err(_) => break,
        }
    }
    drop(stream);
    if let Err(error) = client
        .active_filter()
        .observe_response(&mut Vec::new(), &collected)
    {
        return ErrorBodyRead::Stopped(error);
    }
    if reached_eof {
        if let Err(error) = client.active_filter().observe_response(&mut carry, &[]) {
            return ErrorBodyRead::Stopped(error);
        }
    }
    if let Some(key) = client.cooldown_key() {
        crate::provider_cooldown::record_structured(key, status, &headers, &collected, recorded);
    }
    let raw = String::from_utf8_lossy(&collected);
    if let Some(message) = chatgpt_plan_usage_message(&client.provider_id, status, Some(raw.trim()))
    {
        return ErrorBodyRead::Excerpt(Some(message));
    }
    ErrorBodyRead::Excerpt(excerpt_from_error_bytes(
        &collected,
        &[client.api_key.as_str()],
    ))
}

fn excerpt_from_error_bytes(bytes: &[u8], secrets: &[&str]) -> Option<String> {
    if bytes.is_empty() {
        return None;
    }
    let raw = String::from_utf8_lossy(&bytes[..bytes.len().min(ERROR_BODY_READ_LIMIT)]);
    let message = extract_json_error_message(&raw)?;
    let safe = crate::request::safe_text(&message, secrets);
    static CREDENTIAL_TOKEN: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let credentials = CREDENTIAL_TOKEN.get_or_init(|| {
        regex::Regex::new(r#"(?i)\bbearer\s+[^\s\"'`,;<>]+|sk-[^\s\"'`,;<>]+"#)
            .expect("static credential-token pattern")
    });
    let redacted = credentials.replace_all(&safe, "[REDACTED]");
    let mut excerpt: String = redacted
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect();
    let mut end = ERROR_BODY_EXCERPT_BYTES.min(excerpt.len());
    while !excerpt.is_char_boundary(end) {
        end -= 1;
    }
    excerpt.truncate(end);
    let excerpt = excerpt.trim().to_string();
    (!excerpt.is_empty()).then_some(excerpt)
}

fn extract_json_error_message(raw: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(raw.trim()).ok()?;
    let message = value
        .get("error")
        .and_then(|error| error.get("message"))
        .or_else(|| value.get("message"))
        .or_else(|| value.get("error"))
        .and_then(|message| message.as_str())?;
    Some(message.to_string())
}

/// Append provider messages for a single application message.
///
/// For `Role::Assistant`, emits the assistant message followed by one
/// `role: "tool"` message per completed call (any `Some(output)`).
/// For `Role::User`, emits a plain user message.
/// For `Role::Tool` (legacy), emits tool messages for each completed call.
/// For `Role::System`, emits nothing.
pub(crate) fn project_private_request(
    context: std::sync::Arc<crate::privacy::Context>,
    mut request: crate::request::PreparedModelRequest,
) -> Result<crate::request::PreparedModelRequest, ProviderError> {
    crate::privacy::scope_sync(Some(context.clone()), || {
        request.privacy_policy_digest = Some(context.policy_fingerprint());
        request.privacy_revision = Some(
            context
                .prepare_request(&mut request)
                .map_err(|detail| ProviderError::PrivacyBlocked { detail })?,
        );
        Ok(request)
    })
}

fn canonical_content(message: &Message) -> crate::image_content::ModelContent {
    let content = crate::image_content::ModelContent::text(message.content.clone())
        .with_authored_id(authored_message_key(message));
    match &message.retrieved_spans {
        Some(spans) => content.with_retrieved_spans(spans.clone()),
        None => content.with_legacy_provenance(),
    }
}
fn authored_message_key(message: &Message) -> String {
    let content = crate::image_content::ModelContent::text(message.content.clone())
        .with_retrieved_spans(message.retrieved_spans.clone().unwrap_or_default());
    format!(
        "{:x}",
        Sha256::digest(format!(
            "{}\0{}",
            message.timestamp,
            content
                .authored_text()
                .unwrap_or_else(|_| message.content.clone())
        ))
    )
}

pub(crate) fn append_provider_messages(
    result: &mut Vec<ChatMessage>,
    message: &Message,
) -> anyhow::Result<()> {
    match message.role {
        Role::User => result.push(ChatMessage {
            role: "user".into(),
            content: canonical_content(message),
            tool_calls: None,
            tool_call_id: None,
            response_items: Vec::new(),
        }),
        Role::Assistant => {
            if let Some(calls) = &message.tool_calls {
                validate_tool_results(result, calls)?;
            }
            let tool_calls = message.tool_calls.as_ref().map(|calls| {
                calls
                    .iter()
                    .map(|call| ToolCallMsg {
                        id: call.id.clone(),
                        call_type: "function".into(),
                        function: FunctionCall {
                            name: call.name.clone(),
                            arguments: call.arguments.clone(),
                            arguments_retrieved_spans: call.arguments_retrieved_spans.clone(),
                        },
                    })
                    .collect()
            });
            result.push(ChatMessage {
                role: "assistant".into(),
                content: canonical_content(message),
                tool_calls,
                tool_call_id: None,
                response_items: message.response_items.clone(),
            });
            if let Some(calls) = &message.tool_calls {
                for call in calls {
                    if let Some(output) = &call.output {
                        result.push(ChatMessage {
                            role: "tool".into(),
                            content: crate::image_content::ModelContent::text(output.clone())
                                .with_source_receipt(call.output_source.clone())
                                .with_context_observation(
                                    call.observation
                                        .as_ref()
                                        .filter(|observation| {
                                            observation.matches(&call.name, &call.arguments, output)
                                        })
                                        .cloned(),
                                )
                                .with_prunable_tool_result(crate::context_pruning::qualified(call))
                                .with_tool_result_status(
                                    call.status == crate::app::ToolStatus::Done,
                                ),
                            tool_calls: None,
                            tool_call_id: Some(call.id.clone()),
                            response_items: Vec::new(),
                        });
                    }
                }
            }
        }
        Role::Tool => {
            if let Some(calls) = &message.tool_calls {
                validate_tool_results(result, calls)?;
                for call in calls {
                    let output = call.output.as_ref().expect("validated tool result");
                    result.push(ChatMessage {
                        role: "tool".into(),
                        content: crate::image_content::ModelContent::text(output.clone())
                            .with_source_receipt(call.output_source.clone())
                            .with_context_observation(
                                call.observation
                                    .as_ref()
                                    .filter(|observation| {
                                        observation.matches(&call.name, &call.arguments, output)
                                    })
                                    .cloned(),
                            )
                            .with_prunable_tool_result(crate::context_pruning::qualified(call))
                            .with_tool_result_status(call.status == crate::app::ToolStatus::Done),
                        tool_calls: None,
                        tool_call_id: Some(call.id.clone()),
                        response_items: Vec::new(),
                    });
                }
            }
        }
        Role::System => {}
    }
    Ok(())
}

pub(crate) fn append_transient_tool_images(
    messages: &mut Vec<ChatMessage>,
    images: &std::collections::HashMap<String, crate::image_content::ModelImage>,
) -> Vec<usize> {
    if images.is_empty() {
        return Vec::new();
    }
    let mut expanded = Vec::with_capacity(messages.len().saturating_add(images.len()));
    let mut inserted = Vec::new();
    for message in messages.drain(..) {
        let tool_image = message.tool_call_id.as_ref().and_then(|call_id| {
            images
                .get(call_id)
                .map(|image| (call_id.clone(), image.clone()))
        });
        expanded.push(message);
        if let Some((call_id, image)) = tool_image {
            inserted.push(expanded.len());
            expanded.push(ChatMessage {
                role: "user".into(),
                content: crate::image_content::ModelContent::image_from_tool(&call_id, image),
                tool_calls: None,
                tool_call_id: None,
                response_items: Vec::new(),
            });
        }
    }
    *messages = expanded;
    inserted
}

/// Replace older tool-result images on this request copy when the next send
/// would cross the provider image ceiling. Returns the latched id set.
pub fn retire_outbound_tool_images(
    messages: &mut [ChatMessage],
    latched: &std::collections::HashSet<String>,
) -> std::collections::HashSet<String> {
    let mut carriers = Vec::new();
    let mut reserved_blocks: usize = 0;
    let mut reserved_bytes: usize = 0;
    for message in messages.iter() {
        if let Some(carrier) = message.content.tool_image_carrier() {
            carriers.push(carrier);
        } else {
            let (blocks, bytes) = message.content.image_accounting();
            reserved_blocks = reserved_blocks.saturating_add(blocks);
            reserved_bytes = reserved_bytes.saturating_add(bytes);
        }
    }
    let budget = crate::image_content::fallback_image_request_budget();
    let retire = crate::image_content::tool_image_ids_to_retire(
        &carriers,
        reserved_blocks,
        reserved_bytes,
        budget,
        crate::image_content::IMAGE_RETIRE_BATCH,
        latched,
    );
    for message in messages.iter_mut() {
        if message
            .content
            .tool_image_carrier()
            .is_some_and(|carrier| retire.contains(&carrier.id))
        {
            message.content.retire_images();
        }
    }
    retire
}

fn validate_tool_results(
    result: &[ChatMessage],
    calls: &[crate::app::ToolCall],
) -> anyhow::Result<()> {
    let mut ids = std::collections::HashSet::new();
    for call in calls {
        if call.output.is_none() {
            anyhow::bail!("tool call {} has no result", call.id);
        }
        if !ids.insert(call.id.as_str())
            || result
                .iter()
                .any(|message| message.tool_call_id.as_deref() == Some(call.id.as_str()))
        {
            anyhow::bail!("duplicate tool result for call id {}", call.id);
        }
    }
    Ok(())
}

fn retryable_status(status: u16) -> bool {
    matches!(status, 429 | 500 | 502 | 503 | 504)
}

/// One Codex refresh after the local chatgpt-shim rejects the token. A failed
/// refresh leaves the original HTTP error in place. The test hook never reads
/// or prints a credential.
async fn refresh_consumer_shim_auth() -> bool {
    #[cfg(test)]
    if let Ok(mode) = std::env::var("CA_TEST_CODEX_REFRESH") {
        return mode == "ok";
    }
    let Ok(Some(tokens)) = crate::codex_auth::load_tokens() else {
        return false;
    };
    crate::codex_auth::refresh_tokens(&tokens).await.is_ok()
}

fn retry_delay(response: &reqwest::Response, policy: &ProviderCallPolicy) -> std::time::Duration {
    response
        .headers()
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(std::time::Duration::from_secs)
        .map(|delay| delay.min(policy.max_retry_after))
        .unwrap_or(policy.retry_delay)
}

#[async_trait::async_trait]
impl ChatBackend for AgentClient {
    fn inference_client(&self) -> Option<AgentClient> {
        Some(self.clone())
    }

    async fn prepare_harness_request(
        &self,
        request: crate::request::PreparedModelRequest,
    ) -> crate::request::PreparedModelRequest {
        AgentClient::prepare_harness_request(self, request).await
    }

    async fn stream_chat(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDef],
    ) -> Result<mpsc::UnboundedReceiver<ChatStreamItem>, ProviderError> {
        AgentClient::chat_stream(self, messages, tools).await
    }

    async fn stream_chat_with_options(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDef],
        options: &ChatRequestOptions,
    ) -> Result<mpsc::UnboundedReceiver<ChatStreamItem>, ProviderError> {
        AgentClient::chat_stream_with_options(self, messages, tools, options).await
    }

    async fn stream_prepared(
        &self,
        request: &crate::request::PreparedModelRequest,
    ) -> Result<mpsc::UnboundedReceiver<ChatStreamItem>, ProviderError> {
        AgentClient::stream_prepared(self, request).await
    }
}

#[cfg(test)]
fn request_json_for_test(options: &ChatRequestOptions) -> Value {
    let prepared = crate::request::PreparedModelRequest::new(
        crate::request::RequestKind::Compatibility,
        "test",
        "model",
        Vec::new(),
        Vec::new(),
        options.clone(),
    );
    crate::agent::provider::chat_completions_json_for_test(&prepared)
}

fn apply_output_token_cap(
    body: &mut Vec<u8>,
    protocol: crate::agent::provider::ProviderProtocol,
    direct_openai: bool,
    maximum: u32,
) -> Result<(), ProviderError> {
    use crate::agent::provider::ProviderProtocol;
    let mut value: Value = serde_json::from_slice(body)
        .map_err(|_| ProviderError::Protocol("invalid capped request body".into()))?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| ProviderError::Protocol("invalid capped request object".into()))?;
    if protocol == ProviderProtocol::GoogleGenerateContent {
        let generation = object
            .entry("generationConfig")
            .or_insert_with(|| serde_json::json!({}))
            .as_object_mut()
            .ok_or_else(|| ProviderError::Protocol("invalid Google generation settings".into()))?;
        let previous = generation
            .get("maxOutputTokens")
            .and_then(Value::as_u64)
            .unwrap_or(u64::MAX);
        generation.insert(
            "maxOutputTokens".into(),
            Value::from(previous.min(u64::from(maximum))),
        );
        *body = serde_json::to_vec(&value).map_err(|_| {
            ProviderError::Protocol("cannot serialize capped Google request".into())
        })?;
        return Ok(());
    }
    let field = match protocol {
        ProviderProtocol::GoogleGenerateContent => unreachable!("handled above"),
        ProviderProtocol::OpenAiResponses => "max_output_tokens",
        ProviderProtocol::AnthropicMessages => "max_tokens",
        ProviderProtocol::ChatCompletions if direct_openai => "max_completion_tokens",
        ProviderProtocol::ChatCompletions => "max_tokens",
    };
    // Preserve any tighter adapter limit and remove conflicting aliases.
    let mut cap = u64::from(maximum);
    for key in ["max_tokens", "max_completion_tokens", "max_output_tokens"] {
        if let Some(existing) = object.remove(key) {
            let existing = existing
                .as_u64()
                .filter(|v| *v > 0)
                .ok_or_else(|| ProviderError::Protocol("invalid existing output cap".into()))?;
            cap = cap.min(existing);
        }
    }
    object.insert(field.into(), Value::from(cap));
    *body = serde_json::to_vec(&value)
        .map_err(|_| ProviderError::Protocol("cannot serialize capped request".into()))?;
    Ok(())
}

#[cfg(test)]
#[path = "client_tests.rs"]
mod tests;
