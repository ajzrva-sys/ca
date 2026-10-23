//! Audited provider-template catalog for the provider setup UI.
//!
//! The entries mirror the 40 rows in the Hermes Agent provider page as of
//! 2026-08-12.  A row is actionable only when its authentication and wire
//! format fit one of ca's existing provider adapters.  Keeping unsupported
//! integrations in the catalog makes the boundary visible without pretending
//! that a bearer token can stand in for OAuth, cloud SDK signing, or a
//! subprocess protocol.

use crate::agent::ProviderProtocol;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdapterCompatibility {
    ChatCompletionsBearerOrNone,
    OpenAiResponsesBearerOrNone,
    AnthropicMessagesApiKey,
    GoogleNative,
}

impl AdapterCompatibility {
    #[cfg(test)]
    pub const fn protocol(self) -> ProviderProtocol {
        match self {
            Self::ChatCompletionsBearerOrNone => ProviderProtocol::ChatCompletions,
            Self::OpenAiResponsesBearerOrNone => ProviderProtocol::OpenAiResponses,
            Self::AnthropicMessagesApiKey => ProviderProtocol::AnthropicMessages,
            Self::GoogleNative => ProviderProtocol::GoogleGenerateContent,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::ChatCompletionsBearerOrNone => "ready · chat",
            Self::OpenAiResponsesBearerOrNone => "ready · responses",
            Self::AnthropicMessagesApiKey => "ready · messages",
            Self::GoogleNative => "ready · Google",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Availability {
    Ready(AdapterCompatibility),
    RequiresOAuth,
    RequiresAwsSdk,
    RequiresExternalProcess,
    RequiresCustomTransport,
}

impl Availability {
    #[cfg(test)]
    pub const fn adapter(self) -> Option<AdapterCompatibility> {
        match self {
            Self::Ready(adapter) => Some(adapter),
            Self::RequiresOAuth
            | Self::RequiresAwsSdk
            | Self::RequiresExternalProcess
            | Self::RequiresCustomTransport => None,
        }
    }

    pub const fn is_installable(self) -> bool {
        matches!(self, Self::Ready(_))
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Ready(adapter) => adapter.label(),
            Self::RequiresOAuth => "needs OAuth",
            Self::RequiresAwsSdk => "needs AWS SDK",
            Self::RequiresExternalProcess => "needs subprocess",
            Self::RequiresCustomTransport => "needs adapter",
        }
    }

    pub const fn reason(self) -> &'static str {
        match self {
            Self::Ready(AdapterCompatibility::ChatCompletionsBearerOrNone) => {
                "compatible with ca's Chat Completions bearer/no-auth adapter"
            }
            Self::Ready(AdapterCompatibility::OpenAiResponsesBearerOrNone) => {
                "compatible with ca's OpenAI Responses bearer/no-auth adapter"
            }
            Self::Ready(AdapterCompatibility::AnthropicMessagesApiKey) => {
                "compatible with ca's Anthropic Messages x-api-key adapter"
            }
            Self::Ready(AdapterCompatibility::GoogleNative) => {
                "Native Gemini using an API key or Vertex Application Default Credentials"
            }
            Self::RequiresOAuth => "Needs OAuth login and refreshable credential storage",
            Self::RequiresAwsSdk => "Needs AWS credentials and signed Bedrock requests",
            Self::RequiresExternalProcess => "Needs the local Copilot ACP subprocess",
            Self::RequiresCustomTransport => "Needs provider-specific auth or protocol routing",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderTemplate {
    /// Stable identity for this catalog row.
    pub id: &'static str,
    pub display_name: &'static str,
    pub search_aliases: &'static [&'static str],
    /// Provider identifier written to ca's configuration when actionable.
    pub provider_id: &'static str,
    pub base_url: Option<&'static str>,
    pub api_key_env: Option<&'static str>,
    pub starter_model: Option<&'static str>,
    pub protocol: Option<ProviderProtocol>,
    pub availability: Availability,
}

impl ProviderTemplate {
    pub fn matches(self, query: &str) -> bool {
        let query = query.trim().to_ascii_lowercase();
        if query.is_empty() {
            return true;
        }
        query.split_whitespace().all(|needle| {
            self.search_fields()
                .any(|field| field.to_ascii_lowercase().contains(needle))
        })
    }

    pub const fn is_installable(self) -> bool {
        self.availability.is_installable()
    }

    fn search_fields(self) -> impl Iterator<Item = &'static str> {
        [self.id, self.display_name, self.provider_id]
            .into_iter()
            .chain(self.search_aliases.iter().copied())
            .chain(self.api_key_env)
            .chain(self.starter_model)
    }
}

const CHAT: Availability = Availability::Ready(AdapterCompatibility::ChatCompletionsBearerOrNone);
const RESPONSES: Availability =
    Availability::Ready(AdapterCompatibility::OpenAiResponsesBearerOrNone);
const MESSAGES: Availability = Availability::Ready(AdapterCompatibility::AnthropicMessagesApiKey);

macro_rules! template {
    ($id:literal, $name:literal, [$($alias:literal),* $(,)?], $provider:literal,
     $base:expr, $env:expr, $model:expr, $protocol:expr, $availability:expr) => {
        ProviderTemplate {
            id: $id,
            display_name: $name,
            search_aliases: &[$($alias),*],
            provider_id: $provider,
            base_url: $base,
            api_key_env: $env,
            starter_model: $model,
            protocol: $protocol,
            availability: $availability,
        }
    };
}

pub const ALL: &[ProviderTemplate] = &[
    template!(
        "nous-portal",
        "Nous Portal",
        ["nous", "subscription"],
        "nous-portal",
        Some("https://inference-api.nousresearch.com/v1"),
        None,
        None,
        Some(ProviderProtocol::ChatCompletions),
        Availability::RequiresOAuth
    ),
    template!(
        "openai-codex",
        "OpenAI Codex",
        ["chatgpt", "codex oauth"],
        "openai-codex",
        Some("https://chatgpt.com/backend-api/codex"),
        None,
        Some("gpt-5.6-sol"),
        Some(ProviderProtocol::OpenAiResponses),
        Availability::Ready(AdapterCompatibility::OpenAiResponsesBearerOrNone)
    ),
    template!(
        "github-copilot",
        "GitHub Copilot",
        ["copilot", "github token", "gh"],
        "github-copilot",
        Some("https://api.githubcopilot.com"),
        Some("COPILOT_GITHUB_TOKEN"),
        Some("gpt-5.4"),
        None,
        Availability::RequiresCustomTransport
    ),
    template!(
        "github-copilot-acp",
        "GitHub Copilot ACP",
        ["copilot acp", "copilot cli", "stdio"],
        "github-copilot-acp",
        None,
        None,
        Some("copilot-acp"),
        None,
        Availability::RequiresExternalProcess
    ),
    template!(
        "anthropic",
        "Anthropic",
        ["claude", "claude code", "anthropic api"],
        "anthropic",
        Some("https://api.anthropic.com/v1"),
        Some("ANTHROPIC_API_KEY"),
        Some("claude-sonnet-4-6"),
        Some(ProviderProtocol::AnthropicMessages),
        MESSAGES
    ),
    template!(
        "openrouter",
        "OpenRouter",
        ["open router", "aggregator"],
        "openrouter",
        Some("https://openrouter.ai/api/v1"),
        Some("OPENROUTER_API_KEY"),
        Some("anthropic/claude-fable-5"),
        Some(ProviderProtocol::ChatCompletions),
        CHAT
    ),
    template!(
        "fireworks",
        "Fireworks AI",
        ["fireworks-ai", "fw"],
        "fireworks",
        Some("https://api.fireworks.ai/inference/v1"),
        Some("FIREWORKS_API_KEY"),
        Some("accounts/fireworks/models/kimi-k2p6"),
        Some(ProviderProtocol::ChatCompletions),
        Availability::RequiresCustomTransport
    ),
    template!(
        "novita",
        "NovitaAI",
        ["novita ai", "novita-ai"],
        "novita",
        Some("https://api.novita.ai/openai/v1"),
        Some("NOVITA_API_KEY"),
        Some("moonshotai/kimi-k2.5"),
        Some(ProviderProtocol::ChatCompletions),
        Availability::RequiresCustomTransport
    ),
    template!(
        "ai-gateway",
        "AI Gateway (Vercel)",
        ["vercel", "vercel ai gateway", "aigateway"],
        "ai-gateway",
        Some("https://ai-gateway.vercel.sh/v1"),
        Some("AI_GATEWAY_API_KEY"),
        Some("moonshotai/kimi-k2.6"),
        Some(ProviderProtocol::ChatCompletions),
        Availability::RequiresCustomTransport
    ),
    template!(
        "zai",
        "z.ai / GLM",
        ["z ai", "zhipu", "glm"],
        "zai",
        Some("https://api.z.ai/api/paas/v4"),
        Some("GLM_API_KEY"),
        Some("glm-5.2"),
        Some(ProviderProtocol::ChatCompletions),
        Availability::RequiresCustomTransport
    ),
    template!(
        "kimi-coding",
        "Kimi / Moonshot",
        ["kimi", "moonshot", "kimi coding"],
        "kimi-coding",
        Some("https://api.moonshot.ai/v1"),
        Some("KIMI_API_KEY"),
        Some("kimi-k3"),
        Some(ProviderProtocol::ChatCompletions),
        Availability::RequiresCustomTransport
    ),
    template!(
        "kimi-coding-cn",
        "Kimi / Moonshot (China)",
        ["kimi china", "kimi-cn", "moonshot-cn"],
        "kimi-coding-cn",
        Some("https://api.moonshot.cn/v1"),
        Some("KIMI_CN_API_KEY"),
        Some("kimi-k3"),
        Some(ProviderProtocol::ChatCompletions),
        Availability::RequiresCustomTransport
    ),
    template!(
        "arcee",
        "Arcee AI",
        ["arcee-ai", "arceeai", "trinity"],
        "arcee",
        Some("https://api.arcee.ai/api/v1"),
        Some("ARCEEAI_API_KEY"),
        Some("trinity-large-thinking"),
        Some(ProviderProtocol::ChatCompletions),
        CHAT
    ),
    template!(
        "gmi",
        "GMI Cloud",
        ["gmi-cloud", "gmicloud"],
        "gmi",
        Some("https://api.gmi-serving.com/v1"),
        Some("GMI_API_KEY"),
        Some("zai-org/GLM-5.1-FP8"),
        Some(ProviderProtocol::ChatCompletions),
        Availability::RequiresCustomTransport
    ),
    template!(
        "actual",
        "Actual Computer",
        ["actual-computer", "actualcomputer", "aci", "local daemon"],
        "actual",
        Some("https://api.actual.inc/v1"),
        Some("ACTUAL_API_KEY"),
        None,
        Some(ProviderProtocol::OpenAiResponses),
        RESPONSES
    ),
    template!(
        "minimax",
        "MiniMax",
        ["minimax global"],
        "minimax",
        Some("https://api.minimax.io/anthropic/v1"),
        Some("MINIMAX_API_KEY"),
        Some("MiniMax-M2.7"),
        Some(ProviderProtocol::AnthropicMessages),
        Availability::RequiresCustomTransport
    ),
    template!(
        "minimax-cn",
        "MiniMax China",
        ["minimax china", "minimaxi"],
        "minimax-cn",
        Some("https://api.minimaxi.com/anthropic/v1"),
        Some("MINIMAX_CN_API_KEY"),
        Some("MiniMax-M2.7"),
        Some(ProviderProtocol::AnthropicMessages),
        Availability::RequiresCustomTransport
    ),
    template!(
        "xai",
        "xAI (Grok) — Responses API",
        ["x ai", "grok", "xai responses"],
        "xai",
        Some("https://api.x.ai/v1"),
        Some("XAI_API_KEY"),
        Some("grok-build-0.1"),
        Some(ProviderProtocol::OpenAiResponses),
        Availability::RequiresCustomTransport
    ),
    template!(
        "xai-oauth",
        "xAI Grok OAuth (SuperGrok)",
        ["xai oauth", "grok oauth", "premium+"],
        "xai-oauth",
        Some("https://api.x.ai/v1"),
        None,
        Some("grok-build-0.1"),
        Some(ProviderProtocol::OpenAiResponses),
        Availability::RequiresOAuth
    ),
    template!(
        "alibaba",
        "Qwen Cloud (Alibaba DashScope)",
        ["qwen", "dashscope", "qwen cloud"],
        "alibaba",
        Some("https://dashscope-intl.aliyuncs.com/compatible-mode/v1"),
        Some("DASHSCOPE_API_KEY"),
        Some("qwen3.7-max"),
        Some(ProviderProtocol::ChatCompletions),
        CHAT
    ),
    template!(
        "alibaba-coding-plan",
        "Alibaba Cloud (Coding Plan)",
        ["alibaba coding", "alibaba_coding", "dashscope coding"],
        "alibaba-coding-plan",
        Some("https://coding-intl.dashscope.aliyuncs.com/v1"),
        Some("DASHSCOPE_API_KEY"),
        Some("qwen3.7-plus"),
        Some(ProviderProtocol::ChatCompletions),
        CHAT
    ),
    template!(
        "kilocode",
        "Kilo Code",
        ["kilo", "kilo-code", "kilo gateway"],
        "kilocode",
        Some("https://api.kilo.ai/api/gateway"),
        Some("KILOCODE_API_KEY"),
        Some("anthropic/claude-opus-4.6"),
        Some(ProviderProtocol::ChatCompletions),
        Availability::RequiresCustomTransport
    ),
    template!(
        "xiaomi",
        "Xiaomi MiMo",
        ["mimo", "xiaomi-mimo"],
        "mimo",
        Some("https://api.xiaomimimo.com/v1"),
        Some("XIAOMI_API_KEY"),
        Some("mimo-v2.5-pro"),
        Some(ProviderProtocol::OpenAiResponses),
        RESPONSES
    ),
    template!(
        "tencent-tokenhub",
        "Tencent TokenHub",
        ["tencent", "tokenhub", "tencentmaas", "hy3"],
        "tencent-tokenhub",
        Some("https://tokenhub.tencentmaas.com/v1"),
        Some("TOKENHUB_API_KEY"),
        Some("hy3-preview"),
        Some(ProviderProtocol::ChatCompletions),
        CHAT
    ),
    template!(
        "opencode-zen",
        "OpenCode Zen",
        ["opencode", "zen"],
        "opencode-zen",
        Some("https://opencode.ai/zen/v1"),
        Some("OPENCODE_ZEN_API_KEY"),
        Some("kimi-k2.5"),
        Some(ProviderProtocol::ChatCompletions),
        Availability::RequiresCustomTransport
    ),
    template!(
        "opencode-go",
        "OpenCode Go",
        ["opencode", "go subscription"],
        "opencode-go",
        Some("https://opencode.ai/zen/go/v1"),
        Some("OPENCODE_GO_API_KEY"),
        Some("kimi-k3"),
        Some(ProviderProtocol::ChatCompletions),
        Availability::RequiresCustomTransport
    ),
    template!(
        "deepseek",
        "DeepSeek",
        ["deep seek", "deepseek direct"],
        "deepseek",
        Some("https://api.deepseek.com/v1"),
        Some("DEEPSEEK_API_KEY"),
        Some("deepseek-v4-pro"),
        Some(ProviderProtocol::ChatCompletions),
        CHAT
    ),
    template!(
        "huggingface",
        "Hugging Face",
        ["hf", "hugging face", "inference providers"],
        "huggingface",
        Some("https://router.huggingface.co/v1"),
        Some("HF_TOKEN"),
        Some("moonshotai/Kimi-K2.5"),
        Some(ProviderProtocol::ChatCompletions),
        Availability::RequiresCustomTransport
    ),
    template!(
        "gemini",
        "Google / Gemini",
        ["google ai studio", "google", "gemini api"],
        "gemini",
        Some("https://generativelanguage.googleapis.com/v1beta"),
        Some("GOOGLE_API_KEY"),
        Some("gemini-3.6-flash"),
        Some(ProviderProtocol::GoogleGenerateContent),
        Availability::Ready(AdapterCompatibility::GoogleNative)
    ),
    template!(
        "vertex",
        "Google Vertex AI",
        ["google cloud", "gcp", "adc", "service account"],
        "vertex",
        Some("https://aiplatform.googleapis.com/v1"),
        None,
        Some("gemini-3.1-pro-preview"),
        Some(ProviderProtocol::GoogleGenerateContent),
        Availability::Ready(AdapterCompatibility::GoogleNative)
    ),
    template!(
        "openai-api",
        "OpenAI API (direct)",
        ["openai", "open ai", "gpt", "responses"],
        "openai",
        Some(crate::config::OPENAI_PROVIDER_TEMPLATE_BASE_URL),
        Some(crate::config::OPENAI_PROVIDER_TEMPLATE_API_KEY_ENV),
        Some(crate::config::OPENAI_PROVIDER_TEMPLATE_MODEL),
        Some(ProviderProtocol::OpenAiResponses),
        RESPONSES
    ),
    template!(
        "azure-foundry",
        "Azure AI Foundry",
        ["azure", "azure openai", "foundry"],
        "azure-foundry",
        None,
        None,
        None,
        None,
        Availability::RequiresCustomTransport
    ),
    template!(
        "bedrock",
        "AWS Bedrock",
        ["amazon", "aws", "converse", "sigv4"],
        "bedrock",
        None,
        None,
        Some("us.anthropic.claude-sonnet-4-6"),
        None,
        Availability::RequiresAwsSdk
    ),
    template!(
        "nvidia",
        "NVIDIA Build",
        ["nvidia nim", "build.nvidia.com", "nemotron"],
        "nvidia",
        Some("https://integrate.api.nvidia.com/v1"),
        Some("NVIDIA_API_KEY"),
        Some("nvidia/nemotron-3-ultra-550b-a55b"),
        Some(ProviderProtocol::ChatCompletions),
        Availability::RequiresCustomTransport
    ),
    template!(
        "ollama-cloud",
        "Ollama Cloud",
        ["ollama", "ollama.com"],
        "ollama-cloud",
        Some("https://ollama.com/v1"),
        Some("OLLAMA_API_KEY"),
        Some("gpt-oss:120b"),
        Some(ProviderProtocol::ChatCompletions),
        CHAT
    ),
    template!(
        "qwen-oauth",
        "Qwen OAuth",
        ["qwen portal", "qwen login", "pkce"],
        "qwen-oauth",
        Some("https://portal.qwen.ai/v1"),
        None,
        Some("qwen3-coder-plus"),
        Some(ProviderProtocol::ChatCompletions),
        Availability::RequiresOAuth
    ),
    template!(
        "minimax-oauth",
        "MiniMax OAuth",
        ["minimax login", "minimax coding plan"],
        "minimax-oauth",
        Some("https://api.minimax.io/anthropic"),
        None,
        Some("MiniMax-M2.7"),
        Some(ProviderProtocol::AnthropicMessages),
        Availability::RequiresOAuth
    ),
    template!(
        "stepfun",
        "StepFun",
        ["step fun", "step plan"],
        "stepfun",
        Some("https://api.stepfun.ai/step_plan/v1"),
        Some("STEPFUN_API_KEY"),
        Some("step-3.5-flash"),
        Some(ProviderProtocol::ChatCompletions),
        CHAT
    ),
    template!(
        "lmstudio",
        "LM Studio",
        ["lm studio", "local server", "localhost"],
        "lmstudio",
        Some("http://127.0.0.1:1234/v1"),
        None,
        None,
        Some(ProviderProtocol::ChatCompletions),
        CHAT
    ),
    template!(
        "custom-endpoint",
        "Custom Endpoint",
        ["custom", "openai compatible", "self hosted", "local"],
        "custom",
        None,
        None,
        None,
        Some(ProviderProtocol::ChatCompletions),
        CHAT
    ),
];

pub fn get(id: &str) -> Option<&'static ProviderTemplate> {
    ALL.iter().find(|template| template.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn catalog_has_the_exact_hermes_row_count_and_unique_ids() {
        let expected: HashSet<_> = [
            "nous-portal",
            "openai-codex",
            "github-copilot",
            "github-copilot-acp",
            "anthropic",
            "openrouter",
            "fireworks",
            "novita",
            "ai-gateway",
            "zai",
            "kimi-coding",
            "kimi-coding-cn",
            "arcee",
            "gmi",
            "actual",
            "minimax",
            "minimax-cn",
            "xai",
            "xai-oauth",
            "alibaba",
            "alibaba-coding-plan",
            "kilocode",
            "xiaomi",
            "tencent-tokenhub",
            "opencode-zen",
            "opencode-go",
            "deepseek",
            "huggingface",
            "gemini",
            "vertex",
            "openai-api",
            "azure-foundry",
            "bedrock",
            "nvidia",
            "ollama-cloud",
            "qwen-oauth",
            "minimax-oauth",
            "stepfun",
            "lmstudio",
            "custom-endpoint",
        ]
        .into_iter()
        .collect();
        assert_eq!(ALL.len(), 40);
        let ids: HashSet<_> = ALL.iter().map(|template| template.id).collect();
        assert_eq!(ids, expected);
        assert!(ALL.iter().all(|template| valid_slug(template.id)));
        assert!(ALL.iter().all(|template| valid_slug(template.provider_id)));
    }

    #[test]
    fn openai_direct_preserves_the_exact_ca_template() {
        let openai = get("openai-api").expect("OpenAI direct row");
        assert_eq!(openai.provider_id, "openai");
        assert_eq!(openai.base_url, Some("https://api.openai.com/v1"));
        assert_eq!(openai.api_key_env, Some("OPENAI_API_KEY"));
        assert_eq!(openai.starter_model, Some("gpt-5.6-sol"));
        assert_eq!(openai.protocol, Some(ProviderProtocol::OpenAiResponses));
        assert_eq!(
            openai.availability,
            Availability::Ready(AdapterCompatibility::OpenAiResponsesBearerOrNone)
        );
        assert_eq!(get("xiaomi").unwrap().provider_id, "mimo");
    }

    #[test]
    fn catalog_classification_matches_existing_adapter_boundaries() {
        let ready: Vec<_> = ALL
            .iter()
            .filter(|template| matches!(template.availability, Availability::Ready(_)))
            .collect();
        let installable: Vec<_> = ALL
            .iter()
            .filter(|template| template.is_installable())
            .collect();
        assert_eq!(ready.len(), 17);
        assert_eq!(installable.len(), 17);
        assert_eq!(
            installable
                .iter()
                .filter(|template| template.availability == CHAT)
                .count(),
            10
        );
        assert_eq!(
            installable
                .iter()
                .filter(|template| template.availability == RESPONSES)
                .count(),
            4
        );
        assert_eq!(
            installable
                .iter()
                .filter(|template| template.availability == MESSAGES)
                .count(),
            1
        );
        for template in installable {
            let adapter = template.availability.adapter().expect("ready adapter");
            assert_eq!(template.protocol, Some(adapter.protocol()));
            if template.id != "custom-endpoint" {
                assert!(template.base_url.is_some());
            }
        }
    }

    #[test]
    fn search_covers_names_aliases_and_multi_word_queries() {
        assert!(get("github-copilot-acp").unwrap().matches("stdio"));
        assert!(get("ai-gateway").unwrap().matches("vercel gateway"));
        assert!(get("tencent-tokenhub").unwrap().matches("tencentmaas"));
        assert!(get("huggingface").unwrap().matches("HF"));
        assert!(get("huggingface").unwrap().matches("HF_TOKEN"));
        assert!(get("minimax").unwrap().matches("MINIMAX_API_KEY"));
        assert!(get("openai-api").unwrap().matches("gpt-5.6-sol"));
        assert!(get("openai-api").unwrap().matches("open ai responses"));
        assert!(get("deepseek").unwrap().matches(""));
        assert!(!get("deepseek").unwrap().matches("anthropic"));
    }

    #[test]
    fn static_urls_and_environment_names_are_safe() {
        for template in ALL {
            if let Some(base_url) = template.base_url {
                let parsed = reqwest::Url::parse(base_url).expect("valid static URL");
                assert!(matches!(parsed.scheme(), "https" | "http"));
                assert!(parsed.username().is_empty());
                assert!(parsed.password().is_none());
                assert!(parsed.query().is_none());
                assert!(parsed.fragment().is_none());
                if parsed.scheme() == "http" {
                    assert!(matches!(parsed.host_str(), Some("127.0.0.1" | "localhost")));
                }
            }
            if let Some(environment) = template.api_key_env {
                assert!(valid_environment_name(environment));
            }
        }
    }

    #[test]
    fn unavailable_rows_explain_the_missing_integration() {
        for template in ALL.iter().filter(|template| !template.is_installable()) {
            assert!(!template.availability.label().is_empty());
            assert!(!template.availability.reason().is_empty());
            assert!(template.availability.adapter().is_none());
        }
        assert_eq!(
            get("openai-codex").unwrap().availability,
            Availability::Ready(AdapterCompatibility::OpenAiResponsesBearerOrNone)
        );
        assert_eq!(
            get("bedrock").unwrap().availability,
            Availability::RequiresAwsSdk
        );
        assert_eq!(
            get("github-copilot-acp").unwrap().availability,
            Availability::RequiresExternalProcess
        );
        for id in ["minimax", "minimax-cn"] {
            assert_eq!(
                get(id).unwrap().availability,
                Availability::RequiresCustomTransport,
                "MiniMax M2.x thinking blocks must not be advertised through ca's lossy replay"
            );
        }
        assert_eq!(
            get("gemini").unwrap().availability,
            Availability::Ready(AdapterCompatibility::GoogleNative),
            "Gemini 3 thought signatures survive native tool-call replay"
        );
        assert_eq!(get("deepseek").unwrap().availability, CHAT);
        assert_eq!(get("xiaomi").unwrap().availability, RESPONSES);
        assert_eq!(get("openai-api").unwrap().availability, RESPONSES);
        for id in [
            "fireworks",
            "novita",
            "ai-gateway",
            "zai",
            "kimi-coding-cn",
            "gmi",
            "xai",
            "kilocode",
            "huggingface",
            "nvidia",
        ] {
            assert_eq!(
                get(id).unwrap().availability,
                Availability::RequiresCustomTransport,
                "{id} must not be installable before reasoning-state replay or a two-step gateway fixture"
            );
        }
    }

    fn valid_slug(value: &str) -> bool {
        !value.is_empty()
            && value.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
            })
    }

    fn valid_environment_name(value: &str) -> bool {
        let mut bytes = value.bytes();
        bytes
            .next()
            .is_some_and(|byte| byte == b'_' || byte.is_ascii_alphabetic())
            && bytes.all(|byte| byte == b'_' || byte.is_ascii_alphanumeric())
    }
}
