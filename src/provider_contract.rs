//! First-party provider contracts that are safe to apply only to exact official
//! routes. Generic OpenAI-compatible endpoints intentionally receive none of
//! these model or wire assumptions.

use crate::agent::ProviderProtocol;
use crate::config::{
    CaConfig, ModelInputModality, ModelProfile, PromptCachePolicy, PromptCacheRequestFormat,
    PromptCacheUsageFormat, ProviderConfig, ReasoningEffort, ReasoningRequestFormat,
    ToolCallFormat,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirstPartyProvider {
    OpenAiDirect,
    OpenAiCodex,
    Mimo,
    DeepSeek,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FirstPartyContract {
    family: FirstPartyProvider,
    protocol: ProviderProtocol,
    model: FirstPartyModel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FirstPartyModel {
    OpenAi56,
    OpenAi6Astra,
    OpenAi61Sol,
    Mimo25,
    Mimo25Pro,
    Mimo26Pro,
    DeepSeekV4,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatReasoningWire {
    Generic,
    Mimo {
        enabled: bool,
    },
    DeepSeek {
        enabled: bool,
        effort: Option<ReasoningEffort>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptCacheContractSource {
    FirstParty,
    ModelConfig,
    Unspecified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptCachePolicySource {
    ModelConfig,
    LegacyProvider,
    ContractDefault,
    Unspecified,
}

/// Fully resolved prompt-cache behavior for one exact provider route and
/// model. `usage_format == None` leaves the capability undeclared (legacy
/// callers may retain protocol-compatible parsing); `Some(Auto)` is an
/// explicit model-scoped opt-in to automatic detection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPromptCacheContract {
    contract_id: String,
    request_format: Option<PromptCacheRequestFormat>,
    usage_format: Option<PromptCacheUsageFormat>,
    policy: Option<PromptCachePolicy>,
    contract_source: PromptCacheContractSource,
    policy_source: PromptCachePolicySource,
}

impl ResolvedPromptCacheContract {
    pub(crate) fn contract_id(&self) -> &str {
        &self.contract_id
    }

    pub(crate) const fn request_format(&self) -> Option<PromptCacheRequestFormat> {
        self.request_format
    }

    pub(crate) const fn usage_format(&self) -> Option<PromptCacheUsageFormat> {
        self.usage_format
    }

    pub(crate) const fn policy(&self) -> Option<PromptCachePolicy> {
        self.policy
    }

    pub(crate) const fn contract_source(&self) -> PromptCacheContractSource {
        self.contract_source
    }

    pub(crate) const fn policy_source(&self) -> PromptCachePolicySource {
        self.policy_source
    }

    pub(crate) const fn first_party_formats(&self) -> bool {
        matches!(self.contract_source, PromptCacheContractSource::FirstParty)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PromptCacheContractMetadata {
    id: &'static str,
    request_format: PromptCacheRequestFormat,
    usage_format: PromptCacheUsageFormat,
}

impl FirstPartyContract {
    pub(crate) const fn family(self) -> FirstPartyProvider {
        self.family
    }

    pub const fn is_direct_openai(self) -> bool {
        matches!(self.family, FirstPartyProvider::OpenAiDirect)
    }

    pub const fn is_openai(self) -> bool {
        matches!(
            self.family,
            FirstPartyProvider::OpenAiDirect | FirstPartyProvider::OpenAiCodex
        )
    }

    pub const fn replays_chat_reasoning(self) -> bool {
        matches!(
            (self.family, self.protocol),
            (
                FirstPartyProvider::Mimo | FirstPartyProvider::DeepSeek,
                ProviderProtocol::ChatCompletions
            )
        )
    }

    pub const fn includes_encrypted_reasoning(self) -> bool {
        self.is_openai() && matches!(self.protocol, ProviderProtocol::OpenAiResponses)
    }

    pub(crate) const fn tool_call_format(self) -> Option<ToolCallFormat> {
        match (self.model, self.protocol) {
            (FirstPartyModel::DeepSeekV4, ProviderProtocol::ChatCompletions) => {
                Some(ToolCallFormat::DeepseekDsml)
            }
            _ => None,
        }
    }

    pub const fn supports_images(self) -> bool {
        matches!(
            self.model,
            FirstPartyModel::OpenAi56
                | FirstPartyModel::OpenAi6Astra
                | FirstPartyModel::OpenAi61Sol
                | FirstPartyModel::Mimo25
        )
    }

    pub fn chat_reasoning_wire(self, effort: Option<ReasoningEffort>) -> ChatReasoningWire {
        match (self.family, self.protocol) {
            (FirstPartyProvider::Mimo, ProviderProtocol::ChatCompletions) => {
                ChatReasoningWire::Mimo {
                    enabled: !matches!(effort, Some(ReasoningEffort::None)),
                }
            }
            (FirstPartyProvider::DeepSeek, ProviderProtocol::ChatCompletions) => {
                let (enabled, effort) = match effort {
                    Some(ReasoningEffort::None) => (false, None),
                    Some(ReasoningEffort::XHigh | ReasoningEffort::Max) => {
                        (true, Some(ReasoningEffort::Max))
                    }
                    Some(_) | None => (true, Some(ReasoningEffort::High)),
                };
                ChatReasoningWire::DeepSeek { enabled, effort }
            }
            _ => ChatReasoningWire::Generic,
        }
    }

    pub fn responses_reasoning_effort(
        self,
        effort: Option<ReasoningEffort>,
    ) -> Result<Option<ReasoningEffort>, &'static str> {
        match (self.family, self.protocol, effort) {
            (_, ProviderProtocol::OpenAiResponses, None) => Ok(None),
            (
                FirstPartyProvider::OpenAiDirect | FirstPartyProvider::OpenAiCodex,
                ProviderProtocol::OpenAiResponses,
                effort,
            ) => Ok(effort),
            (
                FirstPartyProvider::Mimo,
                ProviderProtocol::OpenAiResponses,
                Some(ReasoningEffort::None),
            ) => Ok(Some(ReasoningEffort::None)),
            (FirstPartyProvider::Mimo, ProviderProtocol::OpenAiResponses, Some(_)) => {
                Ok(Some(ReasoningEffort::High))
            }
            (
                FirstPartyProvider::DeepSeek,
                ProviderProtocol::OpenAiResponses,
                Some(ReasoningEffort::None),
            ) => Err("DeepSeek Responses does not support disabling reasoning"),
            (
                FirstPartyProvider::DeepSeek,
                ProviderProtocol::OpenAiResponses,
                Some(ReasoningEffort::XHigh | ReasoningEffort::Max),
            ) => Ok(Some(ReasoningEffort::Max)),
            (FirstPartyProvider::DeepSeek, ProviderProtocol::OpenAiResponses, Some(_)) => {
                Ok(Some(ReasoningEffort::High))
            }
            _ => Ok(effort),
        }
    }

    pub fn model_profile(self) -> ModelProfile {
        let (context_window_tokens, reasoning_efforts, default_reasoning_effort) =
            match (self.family, self.protocol) {
                (
                    FirstPartyProvider::OpenAiDirect | FirstPartyProvider::OpenAiCodex,
                    ProviderProtocol::OpenAiResponses,
                ) => (
                    1_050_000,
                    vec![
                        ReasoningEffort::None,
                        ReasoningEffort::Low,
                        ReasoningEffort::Medium,
                        ReasoningEffort::High,
                        ReasoningEffort::XHigh,
                        ReasoningEffort::Max,
                    ],
                    Some(ReasoningEffort::Medium),
                ),
                (FirstPartyProvider::Mimo, _) => (
                    1_048_576,
                    vec![ReasoningEffort::None, ReasoningEffort::High],
                    Some(ReasoningEffort::High),
                ),
                (FirstPartyProvider::DeepSeek, ProviderProtocol::ChatCompletions) => (
                    1_000_000,
                    vec![
                        ReasoningEffort::None,
                        ReasoningEffort::High,
                        ReasoningEffort::Max,
                    ],
                    Some(ReasoningEffort::High),
                ),
                (FirstPartyProvider::DeepSeek, ProviderProtocol::OpenAiResponses) => (
                    1_000_000,
                    vec![ReasoningEffort::High, ReasoningEffort::Max],
                    Some(ReasoningEffort::High),
                ),
                _ => (0, Vec::new(), None),
            };
        // Astra and Sol 6.1 require reasoning; keep their efforts distinct from GPT-5.6.
        let (context_window_tokens, reasoning_efforts, default_reasoning_effort) = if matches!(
            self.model,
            FirstPartyModel::OpenAi6Astra | FirstPartyModel::OpenAi61Sol
        ) {
            (
                if self.family == FirstPartyProvider::OpenAiCodex {
                    272_000
                } else {
                    context_window_tokens
                },
                reasoning_efforts
                    .into_iter()
                    .filter(|effort| *effort != ReasoningEffort::None)
                    .collect(),
                if self.model == FirstPartyModel::OpenAi61Sol
                    && self.family == FirstPartyProvider::OpenAiCodex
                {
                    Some(ReasoningEffort::Low)
                } else {
                    default_reasoning_effort
                },
            )
        } else {
            (
                context_window_tokens,
                reasoning_efforts,
                default_reasoning_effort,
            )
        };
        ModelProfile {
            context_window_tokens: (context_window_tokens > 0).then_some(context_window_tokens),
            compact_at_tokens: None,
            supports_tools: Some(true),
            supports_images: Some(self.supports_images()),
            input_modalities: if self.supports_images() {
                vec![ModelInputModality::Text, ModelInputModality::Image]
            } else {
                vec![ModelInputModality::Text]
            },
            reasoning_efforts,
            default_reasoning_effort,
            reasoning_request_format: Some(ReasoningRequestFormat::ReasoningObject),
            reasoning_stream_field: matches!(self.protocol, ProviderProtocol::ChatCompletions)
                .then(|| "reasoning_content".into()),
            tool_call_format: self.tool_call_format(),
        }
    }

    fn prompt_cache_metadata(self) -> PromptCacheContractMetadata {
        match self.family {
            FirstPartyProvider::OpenAiDirect => PromptCacheContractMetadata {
                id: "openai_direct_v1",
                request_format: PromptCacheRequestFormat::OpenAiResponsesExplicit,
                usage_format: PromptCacheUsageFormat::OpenAiDetails,
            },
            FirstPartyProvider::OpenAiCodex => PromptCacheContractMetadata {
                id: "openai_codex_v1",
                request_format: PromptCacheRequestFormat::ProviderManaged,
                usage_format: PromptCacheUsageFormat::OpenAiDetails,
            },
            FirstPartyProvider::Mimo => PromptCacheContractMetadata {
                id: "mimo_v1",
                request_format: PromptCacheRequestFormat::ProviderManaged,
                usage_format: PromptCacheUsageFormat::OpenAiDetails,
            },
            FirstPartyProvider::DeepSeek if self.protocol == ProviderProtocol::OpenAiResponses => {
                PromptCacheContractMetadata {
                    id: "deepseek_v1",
                    request_format: PromptCacheRequestFormat::ProviderManaged,
                    usage_format: PromptCacheUsageFormat::OpenAiDetails,
                }
            }
            FirstPartyProvider::DeepSeek => PromptCacheContractMetadata {
                id: "deepseek_v1",
                request_format: PromptCacheRequestFormat::ProviderManaged,
                usage_format: PromptCacheUsageFormat::HitMiss,
            },
        }
    }
}

/// Resolve prompt-cache wire behavior and effective policy for one exact route.
///
/// First-party request/usage formats are immutable and endpoint/protocol/model
/// gated by `resolve`. A custom route receives formats only from its exact
/// `model_prompt_cache` entry. Legacy provider-wide policy remains effective
/// solely for direct OpenAI, matching the behavior of existing configurations.
pub fn resolve_prompt_cache(
    config: &CaConfig,
    provider_id: &str,
    provider: &ProviderConfig,
    model: &str,
) -> ResolvedPromptCacheContract {
    let configured = config.model_prompt_cache(provider_id, model);
    let mut built_in = resolve_built_in_prompt_cache(provider_id, provider, model);
    if built_in.first_party_formats() {
        if let Some(policy) = configured.and_then(|item| item.policy) {
            built_in.policy = Some(policy);
            built_in.policy_source = PromptCachePolicySource::ModelConfig;
        }
        return built_in;
    }

    let request_format = configured.and_then(|item| item.request_format);
    let usage_format = configured.and_then(|item| item.usage_format);
    let policy = configured
        .and_then(|item| item.policy)
        .or_else(|| request_format.map(|_| PromptCachePolicy::Automatic));
    let policy_source = if configured.and_then(|item| item.policy).is_some() {
        PromptCachePolicySource::ModelConfig
    } else if request_format.is_some() {
        PromptCachePolicySource::ContractDefault
    } else {
        PromptCachePolicySource::Unspecified
    };
    let contract_source = if request_format.is_some() || usage_format.is_some() {
        PromptCacheContractSource::ModelConfig
    } else {
        PromptCacheContractSource::Unspecified
    };
    let request_id = request_format.map_or("none", PromptCacheRequestFormat::label);
    let usage_id = usage_format.map_or("unspecified", PromptCacheUsageFormat::label);
    let contract_id = if matches!(contract_source, PromptCacheContractSource::ModelConfig) {
        format!("custom_v1:{request_id}:{usage_id}")
    } else {
        "unspecified_v1".into()
    };
    ResolvedPromptCacheContract {
        contract_id,
        request_format,
        usage_format,
        policy,
        contract_source,
        policy_source,
    }
}

/// Resolve only exact built-in and legacy provider behavior. This compatibility
/// entry point exists for clients that have not yet been given the full
/// `CaConfig`; callers with model-scoped configuration should prefer
/// `resolve_prompt_cache`.
pub fn resolve_built_in_prompt_cache(
    provider_id: &str,
    provider: &ProviderConfig,
    model: &str,
) -> ResolvedPromptCacheContract {
    let Some(first_party) = resolve(provider_id, provider, model) else {
        return ResolvedPromptCacheContract {
            contract_id: "unspecified_v1".into(),
            request_format: None,
            usage_format: None,
            policy: None,
            contract_source: PromptCacheContractSource::Unspecified,
            policy_source: PromptCachePolicySource::Unspecified,
        };
    };
    let metadata = first_party.prompt_cache_metadata();
    let (policy, policy_source) =
        if first_party.is_direct_openai() && !provider.prompt_cache_mode.is_default() {
            (
                provider.prompt_cache_mode.policy(),
                PromptCachePolicySource::LegacyProvider,
            )
        } else {
            (
                PromptCachePolicy::Automatic,
                PromptCachePolicySource::ContractDefault,
            )
        };
    ResolvedPromptCacheContract {
        contract_id: metadata.id.into(),
        request_format: Some(metadata.request_format),
        usage_format: Some(metadata.usage_format),
        policy: Some(policy),
        contract_source: PromptCacheContractSource::FirstParty,
        policy_source,
    }
}

pub fn resolve(
    provider_id: &str,
    provider: &ProviderConfig,
    model: &str,
) -> Option<FirstPartyContract> {
    let family = route_family(provider_id, provider)?;
    let model = match family {
        FirstPartyProvider::OpenAiDirect | FirstPartyProvider::OpenAiCodex
            if matches!(model, "gpt-5.6-sol" | "gpt-5.6-terra" | "gpt-5.6-luna") =>
        {
            FirstPartyModel::OpenAi56
        }
        FirstPartyProvider::OpenAiDirect | FirstPartyProvider::OpenAiCodex
            if model == "gpt-6-astra" =>
        {
            FirstPartyModel::OpenAi6Astra
        }
        FirstPartyProvider::OpenAiDirect | FirstPartyProvider::OpenAiCodex
            if model == "gpt-6.1-sol" =>
        {
            FirstPartyModel::OpenAi61Sol
        }
        FirstPartyProvider::Mimo if model == "mimo-v2.5" => FirstPartyModel::Mimo25,
        FirstPartyProvider::Mimo if model == "mimo-v2.5-pro" => FirstPartyModel::Mimo25Pro,
        FirstPartyProvider::Mimo if model == "mimo-v2.6-pro" => FirstPartyModel::Mimo26Pro,
        FirstPartyProvider::DeepSeek
            if matches!(model, "deepseek-v4-flash" | "deepseek-v4-pro") =>
        {
            FirstPartyModel::DeepSeekV4
        }
        _ => return None,
    };
    Some(FirstPartyContract {
        family,
        protocol: provider.protocol,
        model,
    })
}

pub fn route_family(provider_id: &str, provider: &ProviderConfig) -> Option<FirstPartyProvider> {
    let family = match provider_id {
        "openai" if exact_https_endpoint(&provider.base_url, "api.openai.com", &["/v1"]) => {
            FirstPartyProvider::OpenAiDirect
        }
        "openai-codex"
            if exact_https_endpoint(&provider.base_url, "chatgpt.com", &["/backend-api/codex"]) =>
        {
            FirstPartyProvider::OpenAiCodex
        }
        "mimo" if official_mimo_endpoint(&provider.base_url) => FirstPartyProvider::Mimo,
        "deepseek"
            if exact_https_endpoint(&provider.base_url, "api.deepseek.com", &["/", "/v1"]) =>
        {
            FirstPartyProvider::DeepSeek
        }
        _ => return None,
    };
    if !protocol_supported(family, provider.protocol) {
        return None;
    }
    Some(family)
}

pub fn built_in_model_profile(
    provider_id: &str,
    provider: &ProviderConfig,
    model: &str,
) -> Option<ModelProfile> {
    if provider.protocol == ProviderProtocol::GoogleGenerateContent {
        return crate::google_provider::model_profile(model);
    }
    resolve(provider_id, provider, model).map(FirstPartyContract::model_profile)
}

fn protocol_supported(family: FirstPartyProvider, protocol: ProviderProtocol) -> bool {
    match family {
        FirstPartyProvider::OpenAiDirect | FirstPartyProvider::OpenAiCodex => {
            protocol == ProviderProtocol::OpenAiResponses
        }
        FirstPartyProvider::Mimo | FirstPartyProvider::DeepSeek => matches!(
            protocol,
            ProviderProtocol::ChatCompletions | ProviderProtocol::OpenAiResponses
        ),
    }
}

fn official_mimo_endpoint(value: &str) -> bool {
    exact_https_endpoint(value, "api.xiaomimimo.com", &["/v1"])
        || [
            "token-plan-cn.xiaomimimo.com",
            "token-plan-sgp.xiaomimimo.com",
            "token-plan-ams.xiaomimimo.com",
        ]
        .into_iter()
        .any(|host| exact_https_endpoint(value, host, &["/v1"]))
}

fn exact_https_endpoint(value: &str, host: &str, paths: &[&str]) -> bool {
    let Ok(url) = url::Url::parse(value) else {
        return false;
    };
    url.scheme() == "https"
        && url.host_str() == Some(host)
        && url.port_or_known_default() == Some(443)
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
        && paths.contains(&url.path().trim_end_matches('/').if_empty_then("/"))
}

trait EmptyPathExt {
    fn if_empty_then<'a>(&'a self, fallback: &'a str) -> &'a str;
}

impl EmptyPathExt for str {
    fn if_empty_then<'a>(&'a self, fallback: &'a str) -> &'a str {
        if self.is_empty() {
            fallback
        } else {
            self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ModelPromptCacheConfig, PromptCacheMode, ProviderTimeoutConfig};

    fn provider(base_url: &str, protocol: ProviderProtocol) -> ProviderConfig {
        ProviderConfig {
            google: None,
            tool_schema_policy: Default::default(),
            base_url: base_url.into(),
            api_key: "secret-never-debugged".into(),
            protocol,
            prompt_cache_mode: PromptCacheMode::Implicit,
            models: Vec::new(),
            timeout: ProviderTimeoutConfig::default(),
        }
    }

    #[test]
    fn exact_official_routes_resolve_and_named_proxies_do_not() {
        assert!(resolve(
            "openai",
            &provider(
                "https://api.openai.com/v1",
                ProviderProtocol::OpenAiResponses
            ),
            "gpt-5.6-terra"
        )
        .is_some());
        assert!(resolve(
            "mimo",
            &provider(
                "https://token-plan-sgp.xiaomimimo.com/v1/",
                ProviderProtocol::OpenAiResponses
            ),
            "mimo-v2.5"
        )
        .is_some());
        assert!(resolve(
            "deepseek",
            &provider(
                "https://api.deepseek.com",
                ProviderProtocol::ChatCompletions
            ),
            "deepseek-v4-pro"
        )
        .is_some());
        assert!(resolve(
            "openai",
            &provider(
                "https://proxy.example/v1",
                ProviderProtocol::OpenAiResponses
            ),
            "gpt-5.6-sol"
        )
        .is_none());
        assert!(resolve(
            "mimo",
            &provider(
                "https://api.xiaomimimo.com.evil/v1",
                ProviderProtocol::OpenAiResponses
            ),
            "mimo-v2.5"
        )
        .is_none());
    }

    #[test]
    fn astra_reasoning_contract_is_exact_and_profile_overrides_win() {
        assert_eq!(
            serde_json::from_str::<ReasoningEffort>("\"x_high\"").unwrap(),
            ReasoningEffort::XHigh
        );
        assert_eq!(
            serde_json::from_str::<ReasoningEffort>("\"xhigh\"").unwrap(),
            ReasoningEffort::XHigh
        );
        for (id, endpoint, context) in [
            ("openai", "https://api.openai.com/v1", 1_050_000),
            (
                "openai-codex",
                "https://chatgpt.com/backend-api/codex",
                272_000,
            ),
        ] {
            let provider = provider(endpoint, ProviderProtocol::OpenAiResponses);
            let profile = built_in_model_profile(id, &provider, "gpt-6-astra").unwrap();
            assert_eq!(profile.context_window_tokens, Some(context));
            assert_eq!(
                profile.default_reasoning_effort,
                Some(ReasoningEffort::Medium)
            );
            assert_eq!(profile.image_input_support(), Some(true));
            assert_eq!(
                profile.reasoning_efforts,
                vec![
                    ReasoningEffort::Low,
                    ReasoningEffort::Medium,
                    ReasoningEffort::High,
                    ReasoningEffort::XHigh,
                    ReasoningEffort::Max
                ]
            );
            assert!(resolve(id, &provider, "gpt-6-astra-unknown").is_none());
            let mut config = CaConfig::default();
            config.providers.insert(id.into(), provider);
            config
                .model_profiles
                .insert(format!("{id}/gpt-6-astra"), ModelProfile::default());
            assert!(config
                .effective_model_profile(id, "gpt-6-astra")
                .unwrap()
                .reasoning_efforts
                .is_empty());
        }
        assert!(resolve(
            "openai-codex",
            &provider(
                "https://proxy.example/v1",
                ProviderProtocol::OpenAiResponses
            ),
            "gpt-6-astra"
        )
        .is_none());
        assert!(resolve(
            "openai",
            &provider(
                "https://api.openai.com/v1",
                ProviderProtocol::ChatCompletions
            ),
            "gpt-6-astra"
        )
        .is_none());
    }

    #[test]
    fn sol_61_reasoning_contract_is_exact_and_profile_overrides_win() {
        for (id, endpoint, context, default_effort) in [
            (
                "openai",
                "https://api.openai.com/v1",
                1_050_000,
                ReasoningEffort::Medium,
            ),
            (
                "openai-codex",
                "https://chatgpt.com/backend-api/codex",
                272_000,
                ReasoningEffort::Low,
            ),
        ] {
            let provider = provider(endpoint, ProviderProtocol::OpenAiResponses);
            let contract = resolve(id, &provider, "gpt-6.1-sol").unwrap();
            let profile = contract.model_profile();
            assert_eq!(profile.context_window_tokens, Some(context));
            assert_eq!(profile.supports_tools, Some(true));
            assert_eq!(profile.image_input_support(), Some(true));
            assert_eq!(
                profile.reasoning_efforts,
                vec![
                    ReasoningEffort::Low,
                    ReasoningEffort::Medium,
                    ReasoningEffort::High,
                    ReasoningEffort::XHigh,
                    ReasoningEffort::Max
                ]
            );
            assert_eq!(profile.default_reasoning_effort, Some(default_effort));
            assert_eq!(
                profile.reasoning_request_format,
                Some(ReasoningRequestFormat::ReasoningObject)
            );
            assert!(contract.includes_encrypted_reasoning());
            assert!(resolve(id, &provider, "gpt-6.1-sol-unknown").is_none());

            let mut config = CaConfig::default();
            config.providers.insert(id.into(), provider);
            config
                .model_profiles
                .insert(format!("{id}/gpt-6.1-sol"), ModelProfile::default());
            assert!(config
                .effective_model_profile(id, "gpt-6.1-sol")
                .unwrap()
                .reasoning_efforts
                .is_empty());
        }
        for (id, endpoint, protocol) in [
            (
                "openai-codex",
                "https://proxy.example/v1",
                ProviderProtocol::OpenAiResponses,
            ),
            (
                "openai",
                "https://api.openai.com/v1",
                ProviderProtocol::ChatCompletions,
            ),
            (
                "custom",
                "https://api.openai.com/v1",
                ProviderProtocol::OpenAiResponses,
            ),
        ] {
            assert!(resolve(id, &provider(endpoint, protocol), "gpt-6.1-sol").is_none());
        }
    }

    #[test]
    fn built_in_capabilities_are_model_and_protocol_truthful() {
        let openai = resolve(
            "openai",
            &provider(
                "https://api.openai.com/v1",
                ProviderProtocol::OpenAiResponses,
            ),
            "gpt-5.6-luna",
        )
        .unwrap()
        .model_profile();
        assert_eq!(openai.context_window_tokens, Some(1_050_000));
        assert_eq!(
            openai.reasoning_efforts,
            vec![
                ReasoningEffort::None,
                ReasoningEffort::Low,
                ReasoningEffort::Medium,
                ReasoningEffort::High,
                ReasoningEffort::XHigh,
                ReasoningEffort::Max
            ]
        );
        assert_eq!(openai.image_input_support(), Some(true));

        let mimo_pro = resolve(
            "mimo",
            &provider(
                "https://api.xiaomimimo.com/v1",
                ProviderProtocol::OpenAiResponses,
            ),
            "mimo-v2.5-pro",
        )
        .unwrap()
        .model_profile();
        assert_eq!(mimo_pro.image_input_support(), Some(false));
        assert_eq!(
            mimo_pro.reasoning_efforts,
            vec![ReasoningEffort::None, ReasoningEffort::High]
        );

        let deepseek_responses = resolve(
            "deepseek",
            &provider(
                "https://api.deepseek.com/v1",
                ProviderProtocol::OpenAiResponses,
            ),
            "deepseek-v4-flash",
        )
        .unwrap()
        .model_profile();
        assert_eq!(
            deepseek_responses.reasoning_efforts,
            vec![ReasoningEffort::High, ReasoningEffort::Max]
        );
        assert_eq!(deepseek_responses.image_input_support(), Some(false));
    }

    #[test]
    fn provider_reasoning_mappings_are_closed() {
        let mimo = resolve(
            "mimo",
            &provider(
                "https://api.xiaomimimo.com/v1",
                ProviderProtocol::OpenAiResponses,
            ),
            "mimo-v2.5",
        )
        .unwrap();
        assert_eq!(
            mimo.responses_reasoning_effort(Some(ReasoningEffort::Low)),
            Ok(Some(ReasoningEffort::High))
        );
        let deepseek = resolve(
            "deepseek",
            &provider(
                "https://api.deepseek.com/v1",
                ProviderProtocol::ChatCompletions,
            ),
            "deepseek-v4-pro",
        )
        .unwrap();
        assert_eq!(
            deepseek.chat_reasoning_wire(Some(ReasoningEffort::XHigh)),
            ChatReasoningWire::DeepSeek {
                enabled: true,
                effort: Some(ReasoningEffort::Max)
            }
        );
    }

    #[test]
    fn mimo_v26_pro_contract_replays_reasoning_and_enables_thinking() {
        let contract = resolve(
            "mimo",
            &provider(
                "https://token-plan-sgp.xiaomimimo.com/v1",
                ProviderProtocol::ChatCompletions,
            ),
            "mimo-v2.6-pro",
        )
        .expect("mimo-v2.6-pro must resolve on official token-plan endpoints");
        assert!(contract.replays_chat_reasoning());
        assert_eq!(
            contract.chat_reasoning_wire(None),
            ChatReasoningWire::Mimo { enabled: true }
        );
        let profile = contract.model_profile();
        assert_eq!(
            profile.reasoning_stream_field.as_deref(),
            Some("reasoning_content")
        );
        assert_eq!(
            profile.reasoning_efforts,
            vec![ReasoningEffort::None, ReasoningEffort::High]
        );
    }

    #[test]
    fn dsml_is_automatic_only_for_exact_deepseek_v4_chat_contracts() {
        let chat = resolve(
            "deepseek",
            &provider(
                "https://api.deepseek.com/v1",
                ProviderProtocol::ChatCompletions,
            ),
            "deepseek-v4-pro",
        )
        .unwrap();
        assert_eq!(chat.tool_call_format(), Some(ToolCallFormat::DeepseekDsml));

        let responses = resolve(
            "deepseek",
            &provider(
                "https://api.deepseek.com/v1",
                ProviderProtocol::OpenAiResponses,
            ),
            "deepseek-v4-pro",
        )
        .unwrap();
        assert_eq!(responses.tool_call_format(), None);
        assert!(resolve(
            "deepseek",
            &provider(
                "https://compatible.example/v1",
                ProviderProtocol::ChatCompletions,
            ),
            "deepseek-v4-pro",
        )
        .is_none());
    }

    #[test]
    fn config_uses_user_profiles_and_legacy_context_before_built_ins() {
        let mut config = crate::config::CaConfig::default();
        config.providers.insert(
            "openai".into(),
            provider(
                "https://api.openai.com/v1",
                ProviderProtocol::OpenAiResponses,
            ),
        );
        let built_in = config
            .effective_model_profile("openai", "gpt-5.6-sol")
            .unwrap();
        assert_eq!(built_in.context_window_tokens, Some(1_050_000));
        assert_eq!(
            config
                .context_for_model("openai", "gpt-5.6-sol")
                .unwrap()
                .context_window_tokens,
            Some(1_050_000)
        );

        config.model_context.insert(
            "openai/gpt-5.6-sol".into(),
            crate::config::ModelContextConfig {
                context_window_tokens: Some(900_000),
                compact_at_tokens: Some(700_000),
            },
        );
        assert_eq!(
            config.context_for_model("openai", "gpt-5.6-sol").unwrap(),
            crate::config::ModelContextConfig {
                context_window_tokens: Some(900_000),
                compact_at_tokens: Some(700_000),
            }
        );

        config.model_profiles.insert(
            "openai/gpt-5.6-sol".into(),
            ModelProfile {
                context_window_tokens: Some(42_000),
                supports_tools: Some(false),
                ..ModelProfile::default()
            },
        );
        let overridden = config
            .effective_model_profile("openai", "gpt-5.6-sol")
            .unwrap();
        assert_eq!(overridden.context_window_tokens, Some(42_000));
        assert_eq!(overridden.supports_tools, Some(false));
        assert!(overridden.reasoning_efforts.is_empty());
        assert_eq!(
            config
                .context_for_model("openai", "gpt-5.6-sol")
                .unwrap()
                .context_window_tokens,
            Some(42_000)
        );
    }

    #[test]
    fn first_party_prompt_cache_contracts_are_exact_and_provider_specific() {
        let cases = [
            (
                "openai",
                "https://api.openai.com/v1",
                ProviderProtocol::OpenAiResponses,
                "gpt-5.6-terra",
                "openai_direct_v1",
                PromptCacheRequestFormat::OpenAiResponsesExplicit,
                PromptCacheUsageFormat::OpenAiDetails,
            ),
            (
                "openai-codex",
                "https://chatgpt.com/backend-api/codex",
                ProviderProtocol::OpenAiResponses,
                "gpt-5.6-terra",
                "openai_codex_v1",
                PromptCacheRequestFormat::ProviderManaged,
                PromptCacheUsageFormat::OpenAiDetails,
            ),
            (
                "mimo",
                "https://api.xiaomimimo.com/v1",
                ProviderProtocol::ChatCompletions,
                "mimo-v2.5",
                "mimo_v1",
                PromptCacheRequestFormat::ProviderManaged,
                PromptCacheUsageFormat::OpenAiDetails,
            ),
            (
                "deepseek",
                "https://api.deepseek.com/v1",
                ProviderProtocol::ChatCompletions,
                "deepseek-v4-pro",
                "deepseek_v1",
                PromptCacheRequestFormat::ProviderManaged,
                PromptCacheUsageFormat::HitMiss,
            ),
            (
                "deepseek",
                "https://api.deepseek.com/v1",
                ProviderProtocol::OpenAiResponses,
                "deepseek-v4-flash",
                "deepseek_v1",
                PromptCacheRequestFormat::ProviderManaged,
                PromptCacheUsageFormat::OpenAiDetails,
            ),
        ];
        for (id, url, protocol, model, contract_id, request, usage) in cases {
            let resolved = resolve_built_in_prompt_cache(id, &provider(url, protocol), model);
            assert_eq!(resolved.contract_id(), contract_id);
            assert_eq!(resolved.request_format(), Some(request));
            assert_eq!(resolved.usage_format(), Some(usage));
            assert_eq!(resolved.policy(), Some(PromptCachePolicy::Automatic));
            assert_eq!(
                resolved.contract_source(),
                PromptCacheContractSource::FirstParty
            );
        }

        let mut proxy_provider = provider(
            "https://proxy.example/v1",
            ProviderProtocol::OpenAiResponses,
        );
        proxy_provider.prompt_cache_mode = PromptCacheMode::Off;
        let proxy = resolve_built_in_prompt_cache("openai", &proxy_provider, "gpt-5.6-terra");
        assert_eq!(proxy.contract_id(), "unspecified_v1");
        assert_eq!(proxy.request_format(), None);
        assert_eq!(proxy.usage_format(), None);
        assert_eq!(proxy.policy(), None);
    }

    #[test]
    fn exact_policy_overrides_legacy_provider_policy_without_replacing_formats() {
        let mut openai = provider(
            "https://api.openai.com/v1",
            ProviderProtocol::OpenAiResponses,
        );
        openai.prompt_cache_mode = PromptCacheMode::Off;
        let mut config = CaConfig::default();
        config.providers.insert("openai".into(), openai.clone());

        let legacy = resolve_prompt_cache(&config, "openai", &openai, "gpt-5.6-sol");
        assert_eq!(legacy.policy(), Some(PromptCachePolicy::Off));
        assert_eq!(
            legacy.policy_source(),
            PromptCachePolicySource::LegacyProvider
        );

        config.model_prompt_cache.insert(
            "openai/gpt-5.6-sol".into(),
            ModelPromptCacheConfig {
                policy: Some(PromptCachePolicy::StablePrefix),
                ..ModelPromptCacheConfig::default()
            },
        );
        let exact = resolve_prompt_cache(&config, "openai", &openai, "gpt-5.6-sol");
        assert_eq!(exact.contract_id(), "openai_direct_v1");
        assert_eq!(
            exact.request_format(),
            Some(PromptCacheRequestFormat::OpenAiResponsesExplicit)
        );
        assert_eq!(exact.policy(), Some(PromptCachePolicy::StablePrefix));
        assert_eq!(exact.policy_source(), PromptCachePolicySource::ModelConfig);
    }

    #[test]
    fn custom_contract_is_model_scoped_and_has_a_stable_derived_id() {
        let custom = provider(
            "https://compatible.example/v1",
            ProviderProtocol::OpenAiResponses,
        );
        let mut config = CaConfig::default();
        config.providers.insert("custom".into(), custom.clone());
        config.model_prompt_cache.insert(
            "custom/model-a".into(),
            ModelPromptCacheConfig {
                request_format: Some(PromptCacheRequestFormat::OpenAiResponsesExplicit),
                usage_format: Some(PromptCacheUsageFormat::Auto),
                policy: None,
            },
        );

        let configured = resolve_prompt_cache(&config, "custom", &custom, "model-a");
        assert_eq!(
            configured.contract_id(),
            "custom_v1:openai_responses_explicit:auto"
        );
        assert_eq!(configured.policy(), Some(PromptCachePolicy::Automatic));
        assert_eq!(
            configured.policy_source(),
            PromptCachePolicySource::ContractDefault
        );

        let other = resolve_prompt_cache(&config, "custom", &custom, "model-b");
        assert_eq!(other.contract_id(), "unspecified_v1");
        assert_eq!(other.policy(), None);
    }
}
