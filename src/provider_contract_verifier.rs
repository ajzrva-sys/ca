//! Explicit, capped verification of the first-party provider contracts.
//!
//! This path is deliberately separate from ordinary doctor probes: each route
//! receives at most one tool-call request and one continuation request, with no
//! retries, usage ledger, session, trace, or provider-hosted tools.

use crate::agent::client::{
    AgentClient, ChatMessage, ChatRequestOptions, FunctionCall, ProviderCallPolicy, ToolCallMsg,
    ToolDef, ToolFunction,
};
use crate::agent::stream::{AssistantTurnAssembler, ProviderError};
use crate::config::{CaConfig, ProviderConfig};
use clap::ValueEnum;
use serde::Serialize;
use serde_json::json;
use std::io::Write;

const REPORT_SCHEMA: &str = "ca.provider-contract-doctor.v1";
const TOOL_NAME: &str = "ca_contract_echo";
const MAX_STREAM_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum ProviderContractTarget {
    Openai,
    Mimo,
    Deepseek,
}

impl ProviderContractTarget {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Openai => "openai",
            Self::Mimo => "mimo",
            Self::Deepseek => "deepseek",
        }
    }

    fn accepts(self, family: crate::provider_contract::FirstPartyProvider) -> bool {
        use crate::provider_contract::FirstPartyProvider;
        matches!(
            (self, family),
            (
                Self::Openai,
                FirstPartyProvider::OpenAiDirect | FirstPartyProvider::OpenAiCodex
            ) | (Self::Mimo, FirstPartyProvider::Mimo)
                | (Self::Deepseek, FirstPartyProvider::DeepSeek)
        )
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ProviderContractDoctorReport {
    schema: &'static str,
    pub ready: bool,
    pub routes: Vec<ProviderContractRouteReport>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ProviderContractRouteReport {
    pub contract: String,
    pub provider: String,
    pub model: String,
    pub protocol: String,
    pub requests: u8,
    pub authenticated: bool,
    pub streamed: bool,
    pub tool_call: bool,
    pub continued: bool,
    pub terminal_response: bool,
    pub usage_reported: bool,
    pub cache_usage_reported: bool,
    pub status: &'static str,
}

impl ProviderContractRouteReport {
    fn unavailable(target: ProviderContractTarget) -> Self {
        Self {
            contract: target.label().into(),
            provider: target.label().into(),
            model: String::new(),
            protocol: String::new(),
            requests: 0,
            authenticated: false,
            streamed: false,
            tool_call: false,
            continued: false,
            terminal_response: false,
            usage_reported: false,
            cache_usage_reported: false,
            status: "not_configured",
        }
    }

    fn route(
        target: ProviderContractTarget,
        provider: &str,
        model: &str,
        protocol: crate::agent::ProviderProtocol,
    ) -> Self {
        Self {
            contract: target.label().into(),
            provider: provider.into(),
            model: model.into(),
            protocol: protocol.label().into(),
            requests: 0,
            authenticated: false,
            streamed: false,
            tool_call: false,
            continued: false,
            terminal_response: false,
            usage_reported: false,
            cache_usage_reported: false,
            status: "failed",
        }
    }
}

#[async_trait::async_trait]
trait RuntimeProviderResolver: Send + Sync {
    async fn resolve(&self, provider: &str, config: ProviderConfig) -> Result<ProviderConfig, ()>;
}

struct DefaultRuntimeProviderResolver;

#[async_trait::async_trait]
impl RuntimeProviderResolver for DefaultRuntimeProviderResolver {
    async fn resolve(&self, provider: &str, config: ProviderConfig) -> Result<ProviderConfig, ()> {
        crate::provider_connect::resolve_runtime_provider(provider, config)
            .await
            .map_err(|_| ())
    }
}

#[async_trait::async_trait]
trait ContractTurnExecutor: Send + Sync {
    async fn turn(
        &self,
        client: &AgentClient,
        provider: &str,
        model: &str,
        messages: &[ChatMessage],
        tools: &[ToolDef],
        options: &ChatRequestOptions,
    ) -> Result<crate::agent::stream::AssembledAssistantTurn, ProviderError>;
}

struct DefaultContractTurnExecutor;

#[async_trait::async_trait]
impl ContractTurnExecutor for DefaultContractTurnExecutor {
    async fn turn(
        &self,
        client: &AgentClient,
        provider: &str,
        model: &str,
        messages: &[ChatMessage],
        tools: &[ToolDef],
        options: &ChatRequestOptions,
    ) -> Result<crate::agent::stream::AssembledAssistantTurn, ProviderError> {
        stream_turn(client, provider, model, messages, tools, options).await
    }
}

pub async fn verify(
    config: &CaConfig,
    targets: &[ProviderContractTarget],
) -> ProviderContractDoctorReport {
    verify_with_dependencies(
        config,
        targets,
        &DefaultRuntimeProviderResolver,
        &DefaultContractTurnExecutor,
    )
    .await
}

#[cfg(test)]
async fn verify_with_resolver(
    config: &CaConfig,
    targets: &[ProviderContractTarget],
    resolver: &dyn RuntimeProviderResolver,
) -> ProviderContractDoctorReport {
    verify_with_dependencies(config, targets, resolver, &DefaultContractTurnExecutor).await
}

async fn verify_with_dependencies(
    config: &CaConfig,
    targets: &[ProviderContractTarget],
    resolver: &dyn RuntimeProviderResolver,
    executor: &dyn ContractTurnExecutor,
) -> ProviderContractDoctorReport {
    let mut routes = Vec::new();
    for &target in targets {
        let selected = matching_routes(config, target);
        if selected.is_empty() {
            routes.push(ProviderContractRouteReport::unavailable(target));
            continue;
        }
        for (provider_id, model, provider) in selected {
            routes.push(
                verify_route(target, &provider_id, &model, provider, resolver, executor).await,
            );
        }
    }
    let ready = !routes.is_empty() && routes.iter().all(|route| route.status == "passed");
    ProviderContractDoctorReport {
        schema: REPORT_SCHEMA,
        ready,
        routes,
    }
}

fn matching_routes(
    config: &CaConfig,
    target: ProviderContractTarget,
) -> Vec<(String, String, ProviderConfig)> {
    let mut routes = Vec::new();
    for (provider_id, provider) in &config.providers {
        let Some(family) = crate::provider_contract::route_family(provider_id, provider) else {
            continue;
        };
        if !target.accepts(family) {
            continue;
        }
        let preferred = (config.defaults.provider == *provider_id)
            .then_some(config.defaults.model.as_str())
            .filter(|model| {
                crate::provider_contract::resolve(provider_id, provider, model).is_some()
            });
        let model = preferred.or_else(|| {
            provider.models.iter().map(String::as_str).find(|model| {
                crate::provider_contract::resolve(provider_id, provider, model).is_some()
            })
        });
        if let Some(model) = model {
            routes.push((provider_id.clone(), model.into(), provider.clone()));
        }
    }
    routes
}

async fn verify_route(
    target: ProviderContractTarget,
    provider_id: &str,
    model: &str,
    provider: ProviderConfig,
    resolver: &dyn RuntimeProviderResolver,
    executor: &dyn ContractTurnExecutor,
) -> ProviderContractRouteReport {
    let mut report =
        ProviderContractRouteReport::route(target, provider_id, model, provider.protocol);
    let Ok(provider) = resolver.resolve(provider_id, provider).await else {
        report.status = "authentication_failed";
        return report;
    };
    let Some(contract) = crate::provider_contract::resolve(provider_id, &provider, model) else {
        report.status = "route_changed";
        return report;
    };
    let profile = contract.model_profile();
    let options = ChatRequestOptions {
        reasoning_effort: profile.default_reasoning_effort,
        reasoning_request_format: profile.reasoning_request_format,
        reasoning_stream_field: profile.reasoning_stream_field,
        tool_call_format: profile.tool_call_format,
    };
    let client = AgentClient::with_call_policy(
        provider_id,
        &provider,
        model,
        ProviderCallPolicy::one_attempt(),
        crate::request::RequestStore::default(),
    )
    .without_trace();
    let nonce = uuid::Uuid::new_v4().simple().to_string();
    let tools = vec![contract_tool()];
    let mut messages = vec![ChatMessage {
        role: "user".into(),
        content: crate::image_content::ModelContent::text(format!(
            "Call {TOOL_NAME} exactly once with nonce {nonce}. Do not answer before the tool result."
        )),
        tool_calls: None,
        tool_call_id: None,
        response_items: Vec::new(),
    }];

    report.requests = 1;
    let first = executor
        .turn(&client, provider_id, model, &messages, &tools, &options)
        .await;
    let Ok(first) = first else {
        report.status = "first_request_failed";
        return report;
    };
    report.authenticated = true;
    report.streamed = true;
    observe_usage(&mut report, first.usage.as_ref());
    let [call] = first.tool_calls.as_slice() else {
        report.status = "tool_contract_failed";
        return report;
    };
    let valid_arguments = serde_json::from_str::<serde_json::Value>(&call.arguments)
        .ok()
        .and_then(|value| {
            value
                .get("nonce")
                .and_then(|value| value.as_str())
                .map(str::to_owned)
        })
        .is_some_and(|value| value == nonce);
    if call.name != TOOL_NAME || !valid_arguments {
        report.status = "tool_contract_failed";
        return report;
    }
    report.tool_call = true;

    messages.push(ChatMessage {
        role: "assistant".into(),
        content: crate::image_content::ModelContent::text(first.content),
        tool_calls: Some(vec![ToolCallMsg {
            id: call.id.clone(),
            call_type: "function".into(),
            function: FunctionCall {
                name: call.name.clone(),
                arguments: call.arguments.clone(),
            },
        }]),
        tool_call_id: None,
        response_items: first.response_items,
    });
    messages.push(ChatMessage {
        role: "tool".into(),
        content: crate::image_content::ModelContent::text("{\"ok\":true}"),
        tool_calls: None,
        tool_call_id: Some(call.id.clone()),
        response_items: Vec::new(),
    });

    report.requests = 2;
    let second = executor
        .turn(&client, provider_id, model, &messages, &tools, &options)
        .await;
    let Ok(second) = second else {
        report.status = "continuation_failed";
        return report;
    };
    report.continued = true;
    observe_usage(&mut report, second.usage.as_ref());
    if !second.tool_calls.is_empty() || second.content.trim().is_empty() {
        report.status = "terminal_response_failed";
        return report;
    }
    report.terminal_response = true;
    report.status = if report.usage_reported && report.cache_usage_reported {
        "passed"
    } else if !report.usage_reported {
        "missing_usage"
    } else {
        "missing_cache_usage"
    };
    report
}

async fn stream_turn(
    client: &AgentClient,
    provider: &str,
    model: &str,
    messages: &[ChatMessage],
    tools: &[ToolDef],
    options: &ChatRequestOptions,
) -> Result<crate::agent::stream::AssembledAssistantTurn, ProviderError> {
    let prepared = crate::request::PreparedModelRequest::new(
        crate::request::RequestKind::ProviderProbe,
        provider,
        model,
        messages.to_vec(),
        tools.to_vec(),
        options.clone(),
    );
    let mut receiver = client.stream_prepared(&prepared).await?;
    let mut assembler = AssistantTurnAssembler::new();
    let mut received_bytes = 0_usize;
    while let Some(item) = receiver.recv().await {
        let event = item?;
        received_bytes = received_bytes.saturating_add(match &event {
            crate::agent::stream::ProviderEvent::Text(value)
            | crate::agent::stream::ProviderEvent::Reasoning(value) => value.len(),
            crate::agent::stream::ProviderEvent::ToolCallStart { id, name, .. } => {
                id.len().saturating_add(name.len())
            }
            crate::agent::stream::ProviderEvent::ToolCallArg { arg, .. } => arg.len(),
            crate::agent::stream::ProviderEvent::ResponseItem(item) => match item {
                crate::agent::stream::ProviderResponseItem::ResponsesOutput { item, .. } => {
                    serde_json::to_vec(item).map_or(MAX_STREAM_BYTES + 1, |value| value.len())
                }
                crate::agent::stream::ProviderResponseItem::ChatReasoning { content, .. } => {
                    content.len()
                }
            },
            crate::agent::stream::ProviderEvent::Finish { .. }
            | crate::agent::stream::ProviderEvent::Usage(_)
            | crate::agent::stream::ProviderEvent::Done => 0,
        });
        if received_bytes > MAX_STREAM_BYTES {
            return Err(ProviderError::EventTooLarge {
                limit: MAX_STREAM_BYTES,
            });
        }
        assembler.push(&event)?;
    }
    assembler.finish()
}

fn contract_tool() -> ToolDef {
    ToolDef {
        def_type: "function".into(),
        function: ToolFunction {
            name: TOOL_NAME.into(),
            description: "Return a locally supplied synthetic verification result.".into(),
            parameters: json!({
                "type": "object",
                "properties": { "nonce": { "type": "string" } },
                "required": ["nonce"],
                "additionalProperties": false
            }),
        },
    }
}

fn observe_usage(
    report: &mut ProviderContractRouteReport,
    usage: Option<&crate::agent::stream::UsageInfo>,
) {
    if let Some(usage) = usage {
        report.usage_reported = true;
        let cache = usage.cache_usage();
        report.cache_usage_reported |=
            cache.read.is_reported() || cache.write.is_reported() || cache.miss.is_reported();
    }
}

pub fn write_report(
    report: &ProviderContractDoctorReport,
    format: crate::headless::OutputFormat,
    mut writer: impl Write,
) -> std::io::Result<()> {
    match format {
        crate::headless::OutputFormat::Json => {
            serde_json::to_writer_pretty(&mut writer, report)?;
            writeln!(writer)
        }
        crate::headless::OutputFormat::Text => {
            writeln!(writer, "Provider contract verification")?;
            for route in &report.routes {
                let label = if route.model.is_empty() {
                    route.provider.clone()
                } else {
                    format!("{}/{}", route.provider, route.model)
                };
                writeln!(
                    writer,
                    "{} · {} · {} request{}",
                    label,
                    route.status,
                    route.requests,
                    if route.requests == 1 { "" } else { "s" }
                )?;
            }
            writeln!(
                writer,
                "{}",
                if report.ready { "ready" } else { "not ready" }
            )
        }
        crate::headless::OutputFormat::Jsonl => unreachable!("CLI rejects JSONL doctor output"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProviderTimeoutConfig;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct RejectingResolver;

    struct PassthroughResolver;

    #[async_trait::async_trait]
    impl RuntimeProviderResolver for PassthroughResolver {
        async fn resolve(
            &self,
            _provider: &str,
            config: ProviderConfig,
        ) -> Result<ProviderConfig, ()> {
            Ok(config)
        }
    }

    struct FakeExecutor {
        calls: AtomicUsize,
        fail_on: Option<usize>,
    }

    #[async_trait::async_trait]
    impl ContractTurnExecutor for FakeExecutor {
        async fn turn(
            &self,
            _client: &AgentClient,
            _provider: &str,
            _model: &str,
            messages: &[ChatMessage],
            _tools: &[ToolDef],
            _options: &ChatRequestOptions,
        ) -> Result<crate::agent::stream::AssembledAssistantTurn, ProviderError> {
            let call = self.calls.fetch_add(1, Ordering::AcqRel) + 1;
            if self.fail_on == Some(call) {
                return Err(ProviderError::Http { status: 500 });
            }
            let usage = Some(crate::agent::stream::UsageInfo {
                prompt_tokens: 4,
                completion_tokens: 2,
                total_tokens: 6,
                cached_prompt_tokens: Some(0),
                cache_write_tokens: Some(0),
                reasoning_tokens: Some(0),
                response_metadata: None,
                cache_usage_override: None,
            });
            if call == 1 {
                let prompt = messages[0].content.as_str().unwrap();
                let nonce = prompt
                    .split_once("with nonce ")
                    .and_then(|(_, suffix)| suffix.split_once('.'))
                    .map(|(nonce, _)| nonce)
                    .unwrap();
                Ok(crate::agent::stream::AssembledAssistantTurn {
                    content: String::new(),
                    reasoning: None,
                    response_items: Vec::new(),
                    tool_calls: vec![crate::agent::stream::AssembledToolCall {
                        index: 0,
                        id: "call-1".into(),
                        name: TOOL_NAME.into(),
                        arguments: serde_json::json!({"nonce": nonce}).to_string(),
                    }],
                    usage,
                    finish_reason: crate::agent::stream::FinishReason::ToolCalls,
                })
            } else {
                Ok(crate::agent::stream::AssembledAssistantTurn {
                    content: "OK".into(),
                    reasoning: None,
                    response_items: Vec::new(),
                    tool_calls: Vec::new(),
                    usage,
                    finish_reason: crate::agent::stream::FinishReason::Stop,
                })
            }
        }
    }

    #[async_trait::async_trait]
    impl RuntimeProviderResolver for RejectingResolver {
        async fn resolve(
            &self,
            _provider: &str,
            _config: ProviderConfig,
        ) -> Result<ProviderConfig, ()> {
            Err(())
        }
    }

    fn official_config() -> CaConfig {
        let mut config = CaConfig::default();
        config.providers.clear();
        config.providers.insert(
            "openai".into(),
            ProviderConfig {
                base_url: "https://api.openai.com/v1".into(),
                api_key: "not-used".into(),
                protocol: crate::agent::ProviderProtocol::OpenAiResponses,
                prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
                models: vec!["gpt-5.6-terra".into()],
                timeout: ProviderTimeoutConfig::default(),
            },
        );
        config.defaults.provider = "openai".into();
        config.defaults.model = "gpt-5.6-terra".into();
        config
    }

    #[tokio::test]
    async fn authentication_failure_is_preflight_and_reports_zero_requests() {
        let report = verify_with_resolver(
            &official_config(),
            &[ProviderContractTarget::Openai],
            &RejectingResolver,
        )
        .await;
        assert!(!report.ready);
        assert_eq!(report.routes[0].status, "authentication_failed");
        assert_eq!(report.routes[0].requests, 0);
        let rendered = serde_json::to_string(&report).unwrap();
        assert!(!rendered.contains("not-used"));
        assert!(!rendered.contains("Call ca_contract_echo"));
        assert!(!rendered.contains("nonce"));
    }

    #[tokio::test]
    async fn verifier_is_capped_at_two_turns_and_reports_partial_failures() {
        for (fail_on, expected_status, expected_calls) in [
            (None, "passed", 2),
            (Some(1), "first_request_failed", 1),
            (Some(2), "continuation_failed", 2),
        ] {
            let executor = FakeExecutor {
                calls: AtomicUsize::new(0),
                fail_on,
            };
            let report = verify_with_dependencies(
                &official_config(),
                &[ProviderContractTarget::Openai],
                &PassthroughResolver,
                &executor,
            )
            .await;
            assert_eq!(report.routes[0].status, expected_status);
            assert_eq!(report.routes[0].requests, expected_calls as u8);
            assert_eq!(executor.calls.load(Ordering::Acquire), expected_calls);
            assert!(report.routes[0].requests <= 2);
        }
    }

    #[test]
    fn exact_official_routes_are_selected_and_proxies_are_ignored() {
        let mut config = official_config();
        config.providers.insert(
            "mimo".into(),
            ProviderConfig {
                base_url: "https://proxy.example/v1".into(),
                api_key: "secret".into(),
                protocol: crate::agent::ProviderProtocol::OpenAiResponses,
                prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
                models: vec!["mimo-v2.5-pro".into()],
                timeout: ProviderTimeoutConfig::default(),
            },
        );
        assert_eq!(
            matching_routes(&config, ProviderContractTarget::Openai).len(),
            1
        );
        assert!(matching_routes(&config, ProviderContractTarget::Mimo).is_empty());
    }

    #[test]
    fn report_output_is_versioned_and_contains_no_provider_payloads() {
        let report = ProviderContractDoctorReport {
            schema: REPORT_SCHEMA,
            ready: false,
            routes: vec![ProviderContractRouteReport::unavailable(
                ProviderContractTarget::Deepseek,
            )],
        };
        let mut output = Vec::new();
        write_report(&report, crate::headless::OutputFormat::Json, &mut output).unwrap();
        let output = String::from_utf8(output).unwrap();
        assert!(output.contains(REPORT_SCHEMA));
        assert!(output.contains("not_configured"));
        assert!(!output.contains("arguments"));
        assert!(!output.contains("reasoning"));
    }
}
