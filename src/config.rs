use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ApprovalRule {
    AllowTool(String),
    DenyTool(String),
    AllowPath { tool: String, path_pattern: String },
    DenyPath { tool: String, path_pattern: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaConfig {
    #[serde(
        default,
        skip_serializing_if = "crate::formatting::FormatterConfig::is_default"
    )]
    pub formatting: crate::formatting::FormatterConfig,
    #[serde(
        default,
        skip_serializing_if = "crate::context_pruning::PruningConfig::is_default"
    )]
    pub context_pruning: crate::context_pruning::PruningConfig,
    #[serde(default)]
    pub providers: HashMap<String, ProviderConfig>,
    #[serde(default)]
    pub defaults: DefaultsConfig,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub route_presets: BTreeMap<String, RoutePreset>,
    #[serde(default, skip_serializing_if = "DelegationConfig::is_default")]
    pub delegation: DelegationConfig,
    #[serde(default)]
    pub agent: AgentConfig,
    #[serde(default)]
    pub approval_rules: Vec<ApprovalRule>,
    #[serde(default)]
    pub permission_rules: Vec<crate::permission::PermissionRule>,
    #[serde(default)]
    pub model_context: HashMap<String, ModelContextConfig>,
    #[serde(default)]
    pub model_profiles: HashMap<String, ModelProfile>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub model_prompt_cache: HashMap<String, ModelPromptCacheConfig>,
    #[serde(default)]
    pub skills: SkillsConfig,
    #[serde(default)]
    pub mcp: McpConfig,
    #[serde(default)]
    pub lsp: LspConfig,
    #[serde(default)]
    pub review: ReviewConfig,
    #[serde(default)]
    pub pricing: HashMap<String, crate::usage::UsagePricing>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub provider_billing: HashMap<String, crate::pricing::ProviderBillingConfig>,
    #[serde(default)]
    pub trusted_workspaces: Vec<String>,
    #[serde(default)]
    pub hooks: Vec<HookConfig>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub keymap: crate::keymap::KeymapConfig,
    #[serde(default)]
    pub sessions: SessionsConfig,
    #[serde(default)]
    pub fallbacks: Vec<crate::fallback::ProviderFallbackRule>,
    #[serde(
        default,
        skip_serializing_if = "crate::provider_filter::ProviderFilterConfig::is_disabled"
    )]
    pub provider_filter: crate::provider_filter::ProviderFilterConfig,
    #[serde(default)]
    pub memory: MemoryConfig,
    #[serde(default)]
    pub learning: crate::learning::LearningConfig,
    #[serde(
        default,
        skip_serializing_if = "crate::harness_idle::AutomationConfig::is_default"
    )]
    pub harness_improvement: crate::harness_idle::AutomationConfig,
    #[serde(default)]
    pub voice: VoiceConfig,
    #[serde(default)]
    pub email: EmailConfig,
    #[serde(default)]
    pub goals: crate::goal::GoalConfig,
    #[serde(
        default,
        skip_serializing_if = "crate::ui_settings::UiConfig::is_default"
    )]
    pub ui: crate::ui_settings::UiConfig,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct EmailConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub executable: Option<PathBuf>,
    #[serde(default)]
    pub config_path: Option<PathBuf>,
    #[serde(default = "default_email_version")]
    pub version: String,
    #[serde(default = "default_email_timeout")]
    pub timeout_secs: u64,
    #[serde(default = "default_email_output_bytes")]
    pub max_output_bytes: usize,
    #[serde(default)]
    pub accounts: BTreeMap<String, EmailAccountConfig>,
    #[serde(default, skip_serializing_if = "EmailGatewayConfig::is_disabled")]
    pub gateway: EmailGatewayConfig,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct EmailAccountConfig {
    pub himalaya_account: String,
    pub from: String,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct EmailGatewayConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub account_id: String,
    #[serde(default)]
    pub address: String,
    #[serde(default)]
    pub credential_ref: String,
    #[serde(default)]
    pub imap: EmailTlsEndpoint,
    #[serde(default)]
    pub smtp: EmailTlsEndpoint,
    #[serde(default = "default_inbox_folder")]
    pub inbox_folder: String,
    #[serde(default = "default_drafts_folder")]
    pub drafts_folder: String,
    #[serde(default = "default_gateway_poll_secs")]
    pub poll_secs: u64,
    #[serde(default = "default_gateway_raw_bytes")]
    pub max_raw_bytes: usize,
    #[serde(default = "default_gateway_body_bytes")]
    pub max_body_bytes: usize,
    #[serde(default = "default_gateway_response_bytes")]
    pub max_response_bytes: usize,
    #[serde(default = "default_gateway_batch")]
    pub max_messages_per_poll: usize,
    #[serde(default)]
    pub sender_allowlist: Vec<String>,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct EmailTlsEndpoint {
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub port: u16,
    #[serde(default)]
    pub mode: EmailTlsMode,
}

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EmailTlsMode {
    #[default]
    Imaps,
    StartTlsRequired,
}

impl Default for EmailConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            executable: None,
            config_path: None,
            version: default_email_version(),
            timeout_secs: default_email_timeout(),
            max_output_bytes: default_email_output_bytes(),
            accounts: BTreeMap::new(),
            gateway: EmailGatewayConfig::default(),
        }
    }
}

impl Default for EmailGatewayConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            account_id: String::new(),
            address: String::new(),
            credential_ref: String::new(),
            imap: EmailTlsEndpoint::default(),
            smtp: EmailTlsEndpoint::default(),
            inbox_folder: default_inbox_folder(),
            drafts_folder: default_drafts_folder(),
            poll_secs: default_gateway_poll_secs(),
            max_raw_bytes: default_gateway_raw_bytes(),
            max_body_bytes: default_gateway_body_bytes(),
            max_response_bytes: default_gateway_response_bytes(),
            max_messages_per_poll: default_gateway_batch(),
            sender_allowlist: Vec::new(),
            provider: None,
            model: None,
        }
    }
}

impl Default for EmailTlsEndpoint {
    fn default() -> Self {
        Self {
            host: String::new(),
            port: 0,
            mode: EmailTlsMode::Imaps,
        }
    }
}

impl EmailGatewayConfig {
    pub fn is_disabled(&self) -> bool {
        !self.enabled && self == &Self::default()
    }
}

impl std::fmt::Debug for EmailConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("EmailConfig")
            .field("enabled", &self.enabled)
            .field("executable_configured", &self.executable.is_some())
            .field("config_path_configured", &self.config_path.is_some())
            .field("version", &self.version)
            .field("timeout_secs", &self.timeout_secs)
            .field("max_output_bytes", &self.max_output_bytes)
            .field("account_count", &self.accounts.len())
            .field("gateway", &self.gateway)
            .finish()
    }
}

impl std::fmt::Debug for EmailGatewayConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("EmailGatewayConfig")
            .field("enabled", &self.enabled)
            .field("account_configured", &!self.account_id.is_empty())
            .field("address", &"[PRIVATE EMAIL ADDRESS]")
            .field("credential_ref", &"[REDACTED KEYCHAIN REFERENCE]")
            .field("imap", &self.imap)
            .field("smtp", &self.smtp)
            .field("inbox_folder", &self.inbox_folder)
            .field("drafts_folder", &self.drafts_folder)
            .field("poll_secs", &self.poll_secs)
            .field("max_raw_bytes", &self.max_raw_bytes)
            .field("max_body_bytes", &self.max_body_bytes)
            .field("max_response_bytes", &self.max_response_bytes)
            .field("max_messages_per_poll", &self.max_messages_per_poll)
            .field("sender_allowlist_count", &self.sender_allowlist.len())
            .field("provider_configured", &self.provider.is_some())
            .field("model_configured", &self.model.is_some())
            .finish()
    }
}

impl std::fmt::Debug for EmailAccountConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("EmailAccountConfig")
            .field("himalaya_account", &"[PRIVATE]")
            .field("from", &"[PRIVATE EMAIL ADDRESS]")
            .finish()
    }
}

impl EmailConfig {
    fn validate(&self) -> anyhow::Result<()> {
        if self.version != "2.0" {
            anyhow::bail!("email version must be exactly 2.0");
        }
        if !(1..=120).contains(&self.timeout_secs) {
            anyhow::bail!("email timeout_secs must be between 1 and 120");
        }
        if !(1024..=1024 * 1024).contains(&self.max_output_bytes) {
            anyhow::bail!("email max_output_bytes must be between 1024 and 1048576");
        }
        self.gateway.validate(&self.accounts)?;
        if !self.enabled {
            return Ok(());
        }
        let executable = self
            .executable
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("email is enabled but no executable is configured"))?;
        if !executable.is_absolute() {
            anyhow::bail!("email executable must be an absolute path");
        }
        if self
            .config_path
            .as_ref()
            .is_some_and(|path| !path.is_absolute())
        {
            anyhow::bail!("email config_path must be an absolute path");
        }
        if self.accounts.is_empty() || self.accounts.len() > 32 {
            anyhow::bail!("enabled email requires 1 to 32 configured accounts");
        }
        let mut upstream = std::collections::BTreeSet::new();
        for (account_id, account) in &self.accounts {
            crate::email::types::AccountId::parse(account_id.clone())
                .map_err(|error| anyhow::anyhow!(error))?;
            crate::email::types::AccountId::parse(account.himalaya_account.clone())
                .map_err(|error| anyhow::anyhow!(error))?;
            crate::email::types::EmailAddress::parse(&account.from)
                .map_err(|error| anyhow::anyhow!(error))?;
            if !upstream.insert(account.himalaya_account.to_ascii_lowercase()) {
                anyhow::bail!("email Himalaya account mappings must be unique");
            }
        }
        Ok(())
    }
}

impl EmailGatewayConfig {
    pub(crate) fn validate(
        &self,
        personal_accounts: &BTreeMap<String, EmailAccountConfig>,
    ) -> anyhow::Result<()> {
        if !self.enabled {
            return Ok(());
        }
        let account = crate::email::types::AccountId::parse(self.account_id.clone())
            .map_err(|error| anyhow::anyhow!(error))?;
        if personal_accounts.contains_key(account.as_str()) {
            anyhow::bail!(
                "email gateway account_id must be dedicated and distinct from EML-100 accounts"
            );
        }
        let address = crate::email::types::EmailAddress::parse(&self.address)
            .map_err(|error| anyhow::anyhow!(error))?;
        if self.credential_ref != crate::email::secrets::gateway_reference(account.as_str())
            || crate::email::secrets::gateway_reference_account(&self.credential_ref)
                != Some(account.as_str())
        {
            anyhow::bail!(
                "email gateway credential_ref must be the exact dedicated Keychain reference"
            );
        }
        validate_email_endpoint(&self.imap, "IMAP")?;
        validate_email_endpoint(&self.smtp, "SMTP")?;
        crate::email::types::FolderName::parse(self.inbox_folder.clone())
            .map_err(|error| anyhow::anyhow!(error))?;
        crate::email::types::FolderName::parse(self.drafts_folder.clone())
            .map_err(|error| anyhow::anyhow!(error))?;
        if !(1..=3600).contains(&self.poll_secs)
            || !(16 * 1024..=10 * 1024 * 1024).contains(&self.max_raw_bytes)
            || !(1024..=1024 * 1024).contains(&self.max_body_bytes)
            || !(256..=256 * 1024).contains(&self.max_response_bytes)
            || !(1..=64).contains(&self.max_messages_per_poll)
            || self.sender_allowlist.len() > 256
        {
            anyhow::bail!("email gateway bounds are invalid");
        }
        let mut allowlist = std::collections::BTreeSet::new();
        for sender in &self.sender_allowlist {
            let sender = crate::email::types::EmailAddress::parse(sender)
                .map_err(|error| anyhow::anyhow!(error))?;
            if sender.as_str() == address.as_str() || !allowlist.insert(sender.as_str().to_owned())
            {
                anyhow::bail!(
                    "email gateway sender allowlist must contain unique non-self addresses"
                );
            }
        }
        if self
            .provider
            .as_deref()
            .is_some_and(|value| !valid_email_atom(value))
            || self.model.as_deref().is_some_and(|value| {
                value.is_empty() || value.len() > 256 || value.chars().any(char::is_control)
            })
        {
            anyhow::bail!("email gateway provider/model selection is invalid");
        }
        Ok(())
    }
}

fn validate_email_endpoint(endpoint: &EmailTlsEndpoint, label: &str) -> anyhow::Result<()> {
    let host_ok = !endpoint.host.is_empty()
        && endpoint.host.len() <= 253
        && endpoint
            .host
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
        && !endpoint.host.starts_with('.')
        && !endpoint.host.ends_with('.');
    if !host_ok || endpoint.port == 0 {
        anyhow::bail!("email gateway {label} TLS endpoint is invalid");
    }
    Ok(())
}

fn valid_email_atom(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn default_email_version() -> String {
    "2.0".into()
}

fn default_email_timeout() -> u64 {
    30
}

fn default_email_output_bytes() -> usize {
    512 * 1024
}

fn default_inbox_folder() -> String {
    "INBOX".into()
}
fn default_drafts_folder() -> String {
    "Drafts".into()
}
fn default_gateway_poll_secs() -> u64 {
    30
}
fn default_gateway_raw_bytes() -> usize {
    2 * 1024 * 1024
}
fn default_gateway_body_bytes() -> usize {
    128 * 1024
}
fn default_gateway_response_bytes() -> usize {
    32 * 1024
}
fn default_gateway_batch() -> usize {
    16
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct VoiceConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub backend: Option<VoiceBackendConfig>,
    #[serde(default = "default_voice_max_seconds")]
    pub max_seconds: u64,
}

impl Default for VoiceConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            backend: None,
            max_seconds: default_voice_max_seconds(),
        }
    }
}

impl std::fmt::Debug for VoiceConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VoiceConfig")
            .field("enabled", &self.enabled)
            .field(
                "backend",
                &self.backend.as_ref().map(VoiceBackendConfig::kind),
            )
            .field("max_seconds", &self.max_seconds)
            .finish()
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum VoiceBackendConfig {
    Local {
        executable: PathBuf,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default = "default_voice_transcription_timeout")]
        timeout_secs: u64,
    },
    Provider {
        provider: String,
        model: String,
        #[serde(default = "default_voice_transcription_timeout")]
        timeout_secs: u64,
    },
}

impl VoiceBackendConfig {
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Local { .. } => "local",
            Self::Provider { .. } => "provider",
        }
    }
}

impl VoiceConfig {
    fn validate(&self, providers: &HashMap<String, ProviderConfig>) -> anyhow::Result<()> {
        if !(1..=300).contains(&self.max_seconds) {
            anyhow::bail!("voice max_seconds must be between 1 and 300");
        }
        if !self.enabled {
            return Ok(());
        }
        let backend = self
            .backend
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("voice is enabled but no backend is configured"))?;
        let timeout_secs = match backend {
            VoiceBackendConfig::Local {
                executable,
                args,
                timeout_secs,
            } => {
                if !executable.is_absolute() {
                    anyhow::bail!("voice local executable must be an absolute path");
                }
                let placeholders = args
                    .iter()
                    .map(|argument| argument.matches("{audio}").count())
                    .sum::<usize>();
                if placeholders != 1 {
                    anyhow::bail!(
                        "voice local arguments must contain exactly one {{audio}} placeholder"
                    );
                }
                *timeout_secs
            }
            VoiceBackendConfig::Provider {
                provider,
                model,
                timeout_secs,
            } => {
                if !providers.contains_key(provider) {
                    anyhow::bail!("voice provider is not configured: {provider}");
                }
                if model.trim().is_empty() || model.len() > 256 {
                    anyhow::bail!("voice transcription model must be 1 to 256 characters");
                }
                *timeout_secs
            }
        };
        if !(1..=120).contains(&timeout_secs) {
            anyhow::bail!("voice transcription timeout must be between 1 and 120 seconds");
        }
        Ok(())
    }
}

fn default_voice_max_seconds() -> u64 {
    60
}

fn default_voice_transcription_timeout() -> u64 {
    30
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryConfig {
    #[serde(default = "default_cache_budget_mib")]
    pub cache_budget_mib: u64,
    #[serde(default)]
    pub user: MemoryScopeConfig,
    #[serde(default)]
    pub workspace: MemoryScopeConfig,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_hybrid: Option<LocalHybridMemoryConfig>,
}

fn default_cache_budget_mib() -> u64 {
    256
}
impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            cache_budget_mib: default_cache_budget_mib(),
            user: MemoryScopeConfig::default(),
            workspace: MemoryScopeConfig::default(),
            local_hybrid: None,
        }
    }
}

/// Optional native-memory retrieval through one explicitly managed local model.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LocalHybridMemoryConfig {
    pub runtime_id: String,
    pub model_id: String,
    #[serde(default = "default_embedding_context")]
    pub context_size: u32,
    #[serde(default = "default_embedding_startup_seconds")]
    pub startup_timeout_secs: u64,
    #[serde(default = "default_embedding_similarity")]
    pub minimum_similarity_percent: u8,
}
fn default_embedding_context() -> u32 {
    2048
}
fn default_embedding_startup_seconds() -> u64 {
    30
}
fn default_embedding_similarity() -> u8 {
    70
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryScopeConfig {
    #[serde(default = "default_true")]
    pub recall: bool,
    #[serde(default = "default_true")]
    pub generation: bool,
}

impl Default for MemoryScopeConfig {
    fn default() -> Self {
        Self {
            recall: true,
            generation: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct SessionsConfig {
    #[serde(default)]
    pub retention_days: Option<u64>,
    #[serde(default)]
    pub retention_count: Option<u64>,
    #[serde(default)]
    pub track_reads: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReviewRouteConfig {
    pub provider: String,
    pub model: String,
}

fn default_review_convergence_rounds() -> u8 {
    3
}

fn default_review_convergence_requests() -> u32 {
    48
}

fn default_review_convergence_output_bytes() -> usize {
    512 * 1024
}

fn default_review_convergence_cost() -> Option<crate::usage::DecimalUsd> {
    Some(crate::usage::DecimalUsd::from_parts(1, 0).expect("one dollar is a valid exact decimal"))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReviewConvergenceConfig {
    #[serde(default)]
    pub routes: Vec<ReviewRouteConfig>,
    #[serde(default = "default_review_convergence_rounds")]
    pub max_rounds: u8,
    #[serde(default = "default_review_convergence_requests")]
    pub max_provider_requests: u32,
    #[serde(default = "default_review_convergence_output_bytes")]
    pub max_output_bytes: usize,
    #[serde(default = "default_review_convergence_cost")]
    pub max_cost_usd: Option<crate::usage::DecimalUsd>,
}

impl Default for ReviewConvergenceConfig {
    fn default() -> Self {
        Self {
            routes: Vec::new(),
            max_rounds: default_review_convergence_rounds(),
            max_provider_requests: default_review_convergence_requests(),
            max_output_bytes: default_review_convergence_output_bytes(),
            max_cost_usd: default_review_convergence_cost(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReviewConfig {
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub convergence: ReviewConvergenceConfig,
}

fn default_hook_timeout() -> u64 {
    30
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HookConfig {
    pub event: String,
    pub command: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_hook_timeout")]
    pub timeout_secs: u64,
    #[serde(default)]
    pub fail_closed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct SkillsConfig {
    #[serde(default)]
    pub compatibility_roots: Vec<PathBuf>,
    #[serde(default)]
    pub disabled: Vec<String>,
    /// Non-secret, profile-scoped skill settings keyed as `<skill>.<setting>`.
    #[serde(default)]
    pub config: BTreeMap<String, crate::skills::SkillSettingValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct McpConfig {
    #[serde(default)]
    pub servers: HashMap<String, McpServerConfig>,
    #[serde(default)]
    pub trusted_projects: Vec<ProjectMcpTrust>,
    #[serde(default)]
    pub mechanistic: MechanisticMcpConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MechanisticMcpConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub server: String,
    #[serde(default = "default_mechanistic_max_wait_seconds")]
    pub max_wait_seconds: u64,
    #[serde(default = "default_true")]
    pub automatic_preflight: bool,
    #[serde(default = "default_true")]
    pub automatic_verification: bool,
    #[serde(default = "default_mechanistic_preflight_timeout_seconds")]
    pub preflight_timeout_seconds: u64,
    #[serde(default)]
    pub full_suite: MechanisticFullSuitePolicy,
    #[serde(default = "default_mechanistic_max_repair_passes")]
    pub max_repair_passes: u8,
    #[serde(default)]
    pub allow_unisolated_evidence: bool,
    #[serde(default = "default_true")]
    pub transparent_routing: bool,
    #[serde(default = "default_mechanistic_route_action_budget")]
    pub route_action_budget: u8,
    #[serde(default = "default_mechanistic_route_time_budget_seconds")]
    pub route_time_budget_seconds: u64,
    #[serde(default = "default_mechanistic_capsule_bytes")]
    pub capsule_bytes: usize,
    #[serde(default = "default_mechanistic_warmup_action_budget")]
    pub warmup_action_budget: u8,
    #[serde(default)]
    pub remote: RemoteMechanisticConfig,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RemoteMechanisticConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub config: PathBuf,
    #[serde(default = "default_remote_mechanistic_max_wait_seconds")]
    pub max_wait_seconds: u64,
    /// Exact environment entries passed only to the owned remote codex-team
    /// child. Values may use `${NAME}` references and are never debug-printed.
    #[serde(default)]
    pub env: HashMap<String, String>,
}

impl std::fmt::Debug for RemoteMechanisticConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RemoteMechanisticConfig")
            .field("enabled", &self.enabled)
            .field("config", &self.config)
            .field("max_wait_seconds", &self.max_wait_seconds)
            .field("environment_count", &self.env.len())
            .finish()
    }
}

impl Default for RemoteMechanisticConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            config: PathBuf::new(),
            max_wait_seconds: default_remote_mechanistic_max_wait_seconds(),
            env: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MechanisticFullSuitePolicy {
    ReleaseOnly,
    #[default]
    ReleaseOrUnscoped,
}

impl Default for MechanisticMcpConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            server: String::new(),
            max_wait_seconds: default_mechanistic_max_wait_seconds(),
            automatic_preflight: true,
            automatic_verification: true,
            preflight_timeout_seconds: default_mechanistic_preflight_timeout_seconds(),
            full_suite: MechanisticFullSuitePolicy::default(),
            max_repair_passes: default_mechanistic_max_repair_passes(),
            allow_unisolated_evidence: false,
            transparent_routing: true,
            route_action_budget: default_mechanistic_route_action_budget(),
            route_time_budget_seconds: default_mechanistic_route_time_budget_seconds(),
            capsule_bytes: default_mechanistic_capsule_bytes(),
            warmup_action_budget: default_mechanistic_warmup_action_budget(),
            remote: RemoteMechanisticConfig::default(),
        }
    }
}

impl MechanisticMcpConfig {
    fn validate(&self, servers: &HashMap<String, McpServerConfig>) -> anyhow::Result<()> {
        if !(30..=7_200).contains(&self.max_wait_seconds) {
            anyhow::bail!("mcp.mechanistic max_wait_seconds must be between 30 and 7200");
        }
        if !(1..=30).contains(&self.preflight_timeout_seconds) {
            anyhow::bail!("mcp.mechanistic preflight_timeout_seconds must be between 1 and 30");
        }
        if self.max_repair_passes > 2 {
            anyhow::bail!("mcp.mechanistic max_repair_passes must be between 0 and 2");
        }
        if !(1..=64).contains(&self.route_action_budget) {
            anyhow::bail!("mcp.mechanistic route_action_budget must be between 1 and 64");
        }
        if !(1..=120).contains(&self.route_time_budget_seconds) {
            anyhow::bail!("mcp.mechanistic route_time_budget_seconds must be between 1 and 120");
        }
        if !(4 * 1024..=64 * 1024).contains(&self.capsule_bytes) {
            anyhow::bail!("mcp.mechanistic capsule_bytes must be between 4096 and 65536");
        }
        if self.warmup_action_budget > 8 {
            anyhow::bail!("mcp.mechanistic warmup_action_budget must be between 0 and 8");
        }
        if !(30..=7_200).contains(&self.remote.max_wait_seconds) {
            anyhow::bail!("mcp.mechanistic.remote max_wait_seconds must be between 30 and 7200");
        }
        if self.remote.enabled {
            if !self.enabled {
                anyhow::bail!(
                    "mcp.mechanistic.remote requires mechanistic delegation to be enabled"
                );
            }
            if !self.remote.config.is_absolute() {
                anyhow::bail!("mcp.mechanistic.remote config must be an absolute path");
            }
            for name in self.remote.env.keys() {
                let mut bytes = name.bytes();
                if name.len() > 128
                    || !bytes
                        .next()
                        .is_some_and(|byte| byte.is_ascii_uppercase() || byte == b'_')
                    || !bytes.all(|byte| {
                        byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_'
                    })
                {
                    anyhow::bail!(
                        "mcp.mechanistic.remote env keys must use safe uppercase ASCII identifiers"
                    );
                }
            }
        }
        if !self.enabled {
            return Ok(());
        }
        if self.server.is_empty()
            || self.server.len() > 64
            || !self
                .server
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            anyhow::bail!("mcp.mechanistic server must name one configured MCP server");
        }
        let server = servers.get(&self.server).ok_or_else(|| {
            anyhow::anyhow!(
                "mcp.mechanistic server is not configured in the active profile: {}",
                self.server
            )
        })?;
        if !server.enabled {
            anyhow::bail!("mcp.mechanistic server must be enabled");
        }
        if server.transport != McpTransport::Stdio {
            anyhow::bail!("mcp.mechanistic server must use local stdio transport");
        }
        if server.network {
            anyhow::bail!("mcp.mechanistic server must declare network = false");
        }
        if self.remote.enabled {
            let command = Path::new(&server.command);
            if !command.is_absolute()
                || command.file_name().and_then(|name| name.to_str()) != Some("codex-team")
            {
                anyhow::bail!(
                    "mcp.mechanistic.remote requires the server command to be the absolute codex-team binary"
                );
            }
        }
        Ok(())
    }
}

fn default_mechanistic_max_wait_seconds() -> u64 {
    900
}

fn default_mechanistic_preflight_timeout_seconds() -> u64 {
    10
}

fn default_mechanistic_max_repair_passes() -> u8 {
    2
}

fn default_mechanistic_route_action_budget() -> u8 {
    16
}

fn default_mechanistic_route_time_budget_seconds() -> u64 {
    30
}

fn default_mechanistic_capsule_bytes() -> usize {
    16 * 1024
}

fn default_mechanistic_warmup_action_budget() -> u8 {
    4
}

fn default_remote_mechanistic_max_wait_seconds() -> u64 {
    1_800
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct LspConfig {
    #[serde(default)]
    pub servers: HashMap<String, LspServerConfig>,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LspServerConfig {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    pub language_id: String,
    #[serde(default)]
    pub extensions: Vec<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_lsp_timeout")]
    pub timeout_secs: u64,
}

impl std::fmt::Debug for LspServerConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LspServerConfig")
            .field("command_configured", &(!self.command.is_empty()))
            .field("argument_count", &self.args.len())
            .field("language_id", &self.language_id)
            .field("extensions", &self.extensions)
            .field("enabled", &self.enabled)
            .field("timeout_secs", &self.timeout_secs)
            .finish()
    }
}

fn default_lsp_timeout() -> u64 {
    10
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum McpTransport {
    #[default]
    Stdio,
    StreamableHttp,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum McpOAuthFlow {
    #[default]
    Browser,
    Device,
}

#[derive(Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct McpOAuthConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub flow: McpOAuthFlow,
    #[serde(default)]
    pub issuer: Option<String>,
    #[serde(default)]
    pub client_id: Option<String>,
    #[serde(default)]
    pub client_secret: Option<String>,
    #[serde(default)]
    pub client_metadata_url: Option<String>,
    #[serde(default)]
    pub scopes: Vec<String>,
}

impl std::fmt::Debug for McpOAuthConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("McpOAuthConfig")
            .field("enabled", &self.enabled)
            .field("flow", &self.flow)
            .field("issuer_pinned", &self.issuer.is_some())
            .field("client_id_configured", &self.client_id.is_some())
            .field("client_secret_configured", &self.client_secret.is_some())
            .field(
                "client_metadata_url_configured",
                &self.client_metadata_url.is_some(),
            )
            .field("scope_count", &self.scopes.len())
            .finish()
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct McpServerConfig {
    #[serde(default)]
    pub transport: McpTransport,
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_mcp_timeout")]
    pub timeout_secs: u64,
    #[serde(default)]
    pub network: bool,
    #[serde(default)]
    pub oauth: McpOAuthConfig,
}

impl std::fmt::Debug for McpServerConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("McpServerConfig")
            .field("transport", &self.transport)
            .field("command_configured", &(!self.command.is_empty()))
            .field("argument_count", &self.args.len())
            .field("environment_count", &self.env.len())
            .field("url_configured", &self.url.is_some())
            .field("header_count", &self.headers.len())
            .field("enabled", &self.enabled)
            .field("timeout_secs", &self.timeout_secs)
            .field("network", &self.network)
            .field("oauth", &self.oauth)
            .finish()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectMcpTrust {
    pub workspace: PathBuf,
    pub sha256: String,
    #[serde(default)]
    pub network: bool,
}

fn default_true() -> bool {
    true
}

fn default_mcp_timeout() -> u64 {
    30
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningEffort {
    None,
    Minimal,
    Low,
    Medium,
    High,
    #[serde(rename = "xhigh", alias = "x_high")]
    XHigh,
    Max,
}

impl std::fmt::Display for ReasoningEffort {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::None => "none",
            Self::Minimal => "minimal",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::XHigh => "xhigh",
            Self::Max => "max",
        })
    }
}

impl std::str::FromStr for ReasoningEffort {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "none" => Ok(Self::None),
            "minimal" => Ok(Self::Minimal),
            "low" => Ok(Self::Low),
            "medium" => Ok(Self::Medium),
            "high" => Ok(Self::High),
            "xhigh" => Ok(Self::XHigh),
            "max" => Ok(Self::Max),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningRequestFormat {
    ReasoningEffort,
    ReasoningObject,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModelInputModality {
    Text,
    Image,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolCallFormat {
    DeepseekDsml,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ModelProfile {
    #[serde(default)]
    pub context_window_tokens: Option<u64>,
    #[serde(default)]
    pub compact_at_tokens: Option<u64>,
    #[serde(default)]
    pub supports_tools: Option<bool>,
    #[serde(default)]
    pub supports_images: Option<bool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub input_modalities: Vec<ModelInputModality>,
    #[serde(default)]
    pub reasoning_efforts: Vec<ReasoningEffort>,
    #[serde(default)]
    pub default_reasoning_effort: Option<ReasoningEffort>,
    #[serde(default)]
    pub reasoning_request_format: Option<ReasoningRequestFormat>,
    #[serde(default)]
    pub reasoning_stream_field: Option<String>,
    #[serde(default)]
    pub tool_call_format: Option<ToolCallFormat>,
}

impl ModelProfile {
    pub fn image_input_support(&self) -> Option<bool> {
        if !self.input_modalities.is_empty() {
            return Some(self.input_modalities.contains(&ModelInputModality::Image));
        }
        self.supports_images
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ModelContextConfig {
    #[serde(default)]
    pub context_window_tokens: Option<u64>,
    #[serde(default)]
    pub compact_at_tokens: Option<u64>,
}

/// Exact per-model prompt-cache configuration.
///
/// Request and usage formats describe provider wire capabilities. They are
/// intentionally closed enums: custom providers may opt into a format that ca
/// implements, but cannot inject arbitrary request fields. Exact first-party
/// routes obtain these formats from `provider_contract`; their user config may
/// override only `policy`.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ModelPromptCacheConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_format: Option<PromptCacheRequestFormat>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage_format: Option<PromptCacheUsageFormat>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy: Option<PromptCachePolicy>,
}

impl ModelPromptCacheConfig {
    pub const fn is_empty(&self) -> bool {
        self.request_format.is_none() && self.usage_format.is_none() && self.policy.is_none()
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PromptCacheRequestFormat {
    /// The provider decides whether and how to cache; ca sends no cache-control
    /// fields. Only the automatic policy is truthful for this format.
    ProviderManaged,
    /// OpenAI Responses prompt_cache_key/options/breakpoint controls.
    #[serde(rename = "openai_responses_explicit")]
    OpenAiResponsesExplicit,
}

impl PromptCacheRequestFormat {
    pub const fn label(self) -> &'static str {
        match self {
            Self::ProviderManaged => "provider_managed",
            Self::OpenAiResponsesExplicit => "openai_responses_explicit",
        }
    }

    pub const fn supported_policies(self) -> &'static [PromptCachePolicy] {
        match self {
            Self::ProviderManaged => &[PromptCachePolicy::Automatic],
            Self::OpenAiResponsesExplicit => &PromptCachePolicy::ALL,
        }
    }

    pub const fn supports_policy(self, policy: PromptCachePolicy) -> bool {
        match self {
            Self::ProviderManaged => matches!(policy, PromptCachePolicy::Automatic),
            Self::OpenAiResponsesExplicit => true,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PromptCacheUsageFormat {
    /// Explicitly enable protocol-compatible usage auto-detection. This is
    /// distinct from an omitted format, which leaves the capability undeclared.
    Auto,
    /// OpenAI-compatible cached/cache-write token detail objects.
    #[serde(rename = "openai_details")]
    OpenAiDetails,
    /// Complete cache-hit/cache-miss prompt token totals.
    HitMiss,
    /// Separate cache-read/cache-creation input token totals.
    ReadWrite,
}

impl PromptCacheUsageFormat {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::OpenAiDetails => "openai_details",
            Self::HitMiss => "hit_miss",
            Self::ReadWrite => "read_write",
        }
    }
}

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PromptCachePolicy {
    #[default]
    Automatic,
    StablePrefix,
    Off,
}

impl PromptCachePolicy {
    pub const ALL: [Self; 3] = [Self::Automatic, Self::StablePrefix, Self::Off];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Automatic => "automatic",
            Self::StablePrefix => "stable_prefix",
            Self::Off => "off",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelPromptCacheUpdate {
    pub provider: String,
    pub model: String,
    pub request_format: Option<PromptCacheRequestFormat>,
    pub usage_format: Option<PromptCacheUsageFormat>,
    pub policy: Option<PromptCachePolicy>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextLimitUpdate {
    pub provider: String,
    pub model: String,
    pub context_window_tokens: Option<u64>,
    pub compact_at_tokens: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextLimitSettings {
    pub configured: ModelContextConfig,
    pub fallback: ModelContextConfig,
    pub editing_profile: bool,
}

/// Per-provider timeout and retry overrides.
/// All fields are optional; unspecified values fall back to
/// [`ProviderCallPolicy::default()`].
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderTimeoutConfig {
    /// TCP connection timeout in seconds. Default: 10.
    pub connect_timeout_secs: Option<u64>,
    /// Timeout to receive HTTP response headers in seconds. Default: 60.
    pub headers_timeout_secs: Option<u64>,
    /// Timeout for first body bytes in the response body in seconds. Default: 60.
    pub first_event_timeout_secs: Option<u64>,
    /// Stall timeout between received body bytes in seconds. Default: 120.
    /// Any non-empty chunk counts as liveness, including SSE keep-alives.
    pub no_progress_timeout_secs: Option<u64>,
    /// Maximum retry attempts (clamped 1..=2). Default: 2.
    pub max_attempts: Option<usize>,
    /// Delay between retries in milliseconds. Default: 250.
    pub retry_delay_ms: Option<u64>,
    /// Maximum Retry-After header value in seconds. Default: 5.
    pub max_retry_after_secs: Option<u64>,
}

impl ProviderTimeoutConfig {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// Apply configured overrides onto the default policy.
    pub fn to_call_policy(&self) -> crate::agent::client::ProviderCallPolicy {
        let defaults = crate::agent::client::ProviderCallPolicy::default();
        crate::agent::client::ProviderCallPolicy {
            connect_timeout: self
                .connect_timeout_secs
                .map(std::time::Duration::from_secs)
                .unwrap_or(defaults.connect_timeout),
            headers_timeout: self
                .headers_timeout_secs
                .map(std::time::Duration::from_secs)
                .unwrap_or(defaults.headers_timeout),
            first_event_timeout: self
                .first_event_timeout_secs
                .map(std::time::Duration::from_secs)
                .unwrap_or(defaults.first_event_timeout),
            no_progress_timeout: self
                .no_progress_timeout_secs
                .map(std::time::Duration::from_secs)
                .unwrap_or(defaults.no_progress_timeout),
            max_attempts: self.max_attempts.unwrap_or(defaults.max_attempts),
            retry_delay: self
                .retry_delay_ms
                .map(std::time::Duration::from_millis)
                .unwrap_or(defaults.retry_delay),
            max_retry_after: self
                .max_retry_after_secs
                .map(std::time::Duration::from_secs)
                .unwrap_or(defaults.max_retry_after),
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub google: Option<crate::google_auth::Config>,
    #[serde(
        default,
        skip_serializing_if = "crate::tool_schema::Policy::is_default"
    )]
    pub tool_schema_policy: crate::tool_schema::Policy,
    pub base_url: String,
    pub api_key: String,
    #[serde(default)]
    pub protocol: crate::agent::ProviderProtocol,
    #[serde(default, skip_serializing_if = "PromptCacheMode::is_default")]
    pub prompt_cache_mode: PromptCacheMode,
    #[serde(default)]
    pub models: Vec<String>,
    #[serde(default, skip_serializing_if = "ProviderTimeoutConfig::is_default")]
    pub timeout: ProviderTimeoutConfig,
}

/// Secret-free model metadata read directly from one named profile.
///
/// This DTO deliberately does not contain provider endpoints, credential
/// declarations, or any optional-extension configuration. It is safe to use
/// for the offline `role-run --preflight` response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleRunModelShape {
    pub id: String,
    pub profile: ModelProfile,
}

/// Secret-free provider metadata read directly from one named profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleRunProviderShape {
    pub id: String,
    pub models: Vec<RoleRunModelShape>,
}

/// The bounded, secret-free subset of a named profile needed by role-run
/// preflight.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleRunProfileShape {
    pub profile: String,
    pub sandbox_mode: crate::sandbox::SandboxMode,
    pub providers: Vec<RoleRunProviderShape>,
}

/// The minimal configuration returned at the normal role-run execution
/// boundary. Only the selected provider credential is resolved. Debug output
/// remains safe because `ProviderConfig` redacts its endpoint and credential.
#[derive(Clone)]
pub struct RoleRunSelectedRoute {
    pub provider_id: String,
    pub model_id: String,
    pub provider: ProviderConfig,
    pub model_profile: ModelProfile,
    pub reasoning_effort: Option<ReasoningEffort>,
    pub sandbox_mode: crate::sandbox::SandboxMode,
    pub permission_rules: Vec<crate::permission::PermissionRule>,
    pub legacy_approval_rules: Vec<ApprovalRule>,
}

impl std::fmt::Debug for RoleRunSelectedRoute {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RoleRunSelectedRoute")
            .field("provider_id", &self.provider_id)
            .field("model_id", &self.model_id)
            .field("provider", &self.provider)
            .field("model_profile", &self.model_profile)
            .field("reasoning_effort", &self.reasoning_effort)
            .field("sandbox_mode", &self.sandbox_mode)
            .field("permission_rule_count", &self.permission_rules.len())
            .field(
                "legacy_approval_rule_count",
                &self.legacy_approval_rules.len(),
            )
            .finish()
    }
}

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PromptCacheMode {
    #[default]
    Implicit,
    StablePrefix,
    Off,
}

impl PromptCacheMode {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Implicit => "implicit",
            Self::StablePrefix => "stable_prefix",
            Self::Off => "off",
        }
    }

    pub const fn is_default(&self) -> bool {
        matches!(self, Self::Implicit)
    }

    pub const fn policy(self) -> PromptCachePolicy {
        match self {
            Self::Implicit => PromptCachePolicy::Automatic,
            Self::StablePrefix => PromptCachePolicy::StablePrefix,
            Self::Off => PromptCachePolicy::Off,
        }
    }
}

impl std::fmt::Debug for ProviderConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut s = formatter.debug_struct("ProviderConfig");
        s.field("endpoint", &"[REDACTED]")
            .field("credential", &"[REDACTED]")
            .field("protocol", &self.protocol.label())
            .field("prompt_cache_mode", &self.prompt_cache_mode.label())
            .field("model_count", &self.models.len());
        if !self.timeout.is_default() {
            s.field("timeout", &self.timeout);
        }
        s.finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderApiKeyUpdate {
    Preserve,
    Environment(String),
    Keychain,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderApiKeySource {
    ApplicationDefaultCredentials,
    None,
    Environment(String),
    Keychain,
    Hidden,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderManagementEntry {
    pub id: String,
    pub base_url: String,
    pub api_key_source: ProviderApiKeySource,
    pub configured_models: Vec<String>,
    pub configured: bool,
    pub connected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderManagementSnapshot {
    pub default_provider: String,
    pub default_model: String,
    pub sandbox_mode: String,
    pub approval_mode: String,
    pub providers: Vec<ProviderManagementEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderSettingsUpdate {
    pub google: Option<crate::google_auth::Config>,
    pub provider: String,
    pub base_url: String,
    pub api_key: ProviderApiKeyUpdate,
    pub prompt_cache_mode: PromptCacheMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelSelection {
    pub provider: String,
    pub model: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderDeleteUpdate {
    pub provider: String,
    pub replacement: Option<ModelSelection>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderModelUpdate {
    pub google: Option<crate::google_auth::Config>,
    pub provider: String,
    pub base_url: String,
    pub api_key: ProviderApiKeyUpdate,
    pub model: String,
    pub original_model: Option<String>,
    pub require_new_provider: bool,
    pub select_saved_model: bool,
    pub protocol_override: Option<crate::agent::ProviderProtocol>,
}

pub struct ProviderCredentialRollback {
    raw: String,
    existed: bool,
    provider: String,
    previous_secret: Option<Vec<u8>>,
    credential_changed: bool,
}

impl std::fmt::Debug for ProviderCredentialRollback {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProviderCredentialRollback")
            .field("provider", &self.provider)
            .field("credential", &"[REDACTED]")
            .finish()
    }
}

impl Drop for ProviderCredentialRollback {
    fn drop(&mut self) {
        if let Some(secret) = self.previous_secret.as_mut() {
            secret.fill(0);
        }
    }
}

pub const OPENAI_PROVIDER_TEMPLATE_ID: &str = "openai";
pub const OPENAI_PROVIDER_TEMPLATE_BASE_URL: &str = "https://api.openai.com/v1";
pub const OPENAI_PROVIDER_TEMPLATE_API_KEY_ENV: &str = "OPENAI_API_KEY";
pub const OPENAI_PROVIDER_TEMPLATE_MODEL: &str = "gpt-5.6-sol";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DefaultsConfig {
    #[serde(default = "default_provider")]
    pub provider: String,
    #[serde(default = "default_model")]
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RoutePreset {
    pub provider: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<ReasoningEffort>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DelegationRoute {
    pub provider: String,
    pub model: String,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DelegationMode {
    #[default]
    Same,
    Fixed,
    Learned,
}

impl DelegationMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Same => "same as current agent",
            Self::Fixed => "fixed configured agent",
            Self::Learned => "learned healthy route",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DelegationConfig {
    #[serde(default, skip_serializing_if = "is_same_delegation_mode")]
    pub mode: DelegationMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route: Option<DelegationRoute>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_routes: Vec<String>,
}

impl DelegationConfig {
    fn is_default(&self) -> bool {
        self.mode == DelegationMode::Same && self.route.is_none() && self.allowed_routes.is_empty()
    }

    pub fn effective_mode(&self) -> DelegationMode {
        if self.mode == DelegationMode::Same && self.route.is_some() {
            // Backward compatibility: the historical shape had only `route`.
            DelegationMode::Fixed
        } else {
            self.mode
        }
    }
}

fn is_same_delegation_mode(mode: &DelegationMode) -> bool {
    *mode == DelegationMode::Same
}

fn validate_delegation_route(config: &CaConfig, route: &DelegationRoute) -> anyhow::Result<()> {
    validate_provider_id(&route.provider)?;
    validate_model_id(&route.model)?;
    let provider = config.providers.get(&route.provider).ok_or_else(|| {
        anyhow::anyhow!("delegation provider {} is not configured", route.provider)
    })?;
    if !provider.models.iter().any(|model| model == &route.model) {
        anyhow::bail!(
            "delegation model {}/{} is not configured",
            route.provider,
            route.model
        );
    }
    Ok(())
}

fn validate_delegation(config: &CaConfig) -> anyhow::Result<()> {
    if let Some(route) = &config.delegation.route {
        validate_delegation_route(config, route)?;
    }
    match config.delegation.effective_mode() {
        DelegationMode::Same => {
            if config.delegation.route.is_some() {
                anyhow::bail!("same-agent delegation cannot contain a fixed route");
            }
        }
        DelegationMode::Fixed => {
            if config.delegation.route.is_none() {
                anyhow::bail!("fixed delegation requires delegation.route");
            }
        }
        DelegationMode::Learned => {
            if config.delegation.allowed_routes.is_empty() {
                anyhow::bail!("learned delegation requires at least one allowed route");
            }
        }
    }
    let mut unique = std::collections::BTreeSet::new();
    for allowed in &config.delegation.allowed_routes {
        let (provider, model) = allowed
            .split_once('/')
            .ok_or_else(|| anyhow::anyhow!("allowed delegation routes must be provider/model"))?;
        let route = DelegationRoute {
            provider: provider.to_string(),
            model: model.to_string(),
        };
        validate_delegation_route(config, &route)?;
        if !unique.insert(allowed) {
            anyhow::bail!("duplicate allowed delegation route {allowed}");
        }
    }
    Ok(())
}

pub fn validate_route_preset_name(name: &str) -> anyhow::Result<()> {
    if name.is_empty()
        || name.len() > 64
        || !name.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'_'
        })
    {
        anyhow::bail!(
            "preset names must be 1-64 lowercase ASCII letters, digits, hyphens, or underscores"
        );
    }
    Ok(())
}

fn validate_route_preset_shape(name: &str, preset: &RoutePreset) -> anyhow::Result<()> {
    validate_route_preset_name(name)?;
    validate_provider_id(&preset.provider)?;
    validate_model_id(&preset.model)?;
    Ok(())
}

pub fn validate_route_preset_for_config(
    config: &CaConfig,
    preset: &RoutePreset,
) -> anyhow::Result<()> {
    let provider = config
        .providers
        .get(&preset.provider)
        .ok_or_else(|| anyhow::anyhow!("preset provider {} is not configured", preset.provider))?;
    if !provider.models.iter().any(|model| model == &preset.model) {
        anyhow::bail!(
            "preset model {}/{} is not configured",
            preset.provider,
            preset.model
        );
    }
    if let Some(effort) = preset.reasoning_effort {
        let profile = config
            .effective_model_profile(&preset.provider, &preset.model)
            .ok_or_else(|| anyhow::anyhow!("preset reasoning is unavailable for this model"))?;
        if !profile.reasoning_efforts.contains(&effort) {
            anyhow::bail!("preset reasoning effort {effort} is not configured for this model");
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    /// Ordinary tasks delegate useful independent work without a special mode.
    #[serde(default = "default_keep_awake")]
    pub proactive_delegation: bool,
    /// Host-owned exact-candidate review before completing source changes.
    #[serde(default = "default_keep_awake")]
    pub review_changes: bool,
    #[serde(default = "default_max_turns")]
    pub max_turns: usize,
    #[serde(default = "default_idle_inhibition")]
    pub keep_awake: bool,
    #[serde(default = "default_approval")]
    pub approval_mode: String,
    #[serde(default = "default_command_timeout")]
    pub command_timeout_secs: u64,
    #[serde(default)]
    pub command_output: CommandOutputPolicy,
    #[serde(default)]
    pub tool_discovery: ToolDiscoveryMode,
    #[serde(default = "default_sandbox_mode")]
    pub sandbox_mode: String,
    #[serde(default = "default_subagent_concurrency")]
    pub subagent_concurrency: usize,
    #[serde(default = "default_subagent_depth")]
    pub subagent_max_depth: u8,
    #[serde(default = "default_subagent_children")]
    pub subagent_children_per_turn: usize,
    #[serde(default = "default_subagent_turns")]
    pub subagent_max_turns: usize,
    #[serde(default = "default_subagent_runtime")]
    pub subagent_runtime_secs: u64,
    #[serde(default = "default_subagent_output")]
    pub subagent_output_bytes: usize,
    #[serde(default)]
    pub modes: HashMap<String, CustomModeConfig>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ToolDiscoveryMode {
    #[default]
    Eager,
    Lazy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum CommandOutputPolicy {
    #[default]
    Bounded,
    Semantic,
}

impl CommandOutputPolicy {
    pub const ALL: [Self; 2] = [Self::Bounded, Self::Semantic];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Bounded => "Bounded",
            Self::Semantic => "Semantic",
        }
    }

    pub const fn config_value(self) -> &'static str {
        match self {
            Self::Bounded => "bounded",
            Self::Semantic => "semantic",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CustomModeConfig {
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub tools: Vec<String>,
}

fn default_provider() -> String {
    "deepseek".into()
}
fn default_model() -> String {
    "auto".into()
}
fn default_max_turns() -> usize {
    100
}
fn default_keep_awake() -> bool {
    true
}
fn default_idle_inhibition() -> bool {
    // FreeBSD has no native idle-inhibition backend. An explicit request still
    // reports unavailability; ordinary runs do not request an unsupported feature.
    !cfg!(target_os = "freebsd")
}
fn default_approval() -> String {
    "auto-review".into()
}
fn default_command_timeout() -> u64 {
    30
}
fn default_sandbox_mode() -> String {
    "required".into()
}
fn default_subagent_concurrency() -> usize {
    3
}
fn default_subagent_depth() -> u8 {
    2
}
fn default_subagent_children() -> usize {
    8
}
fn default_subagent_turns() -> usize {
    30
}
fn default_subagent_runtime() -> u64 {
    30 * 60
}
fn default_subagent_output() -> usize {
    256 * 1024
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            proactive_delegation: true,
            review_changes: true,
            max_turns: default_max_turns(),
            keep_awake: default_idle_inhibition(),
            approval_mode: default_approval(),
            command_timeout_secs: default_command_timeout(),
            command_output: CommandOutputPolicy::Bounded,
            tool_discovery: ToolDiscoveryMode::Eager,
            sandbox_mode: default_sandbox_mode(),
            subagent_concurrency: default_subagent_concurrency(),
            subagent_max_depth: default_subagent_depth(),
            subagent_children_per_turn: default_subagent_children(),
            subagent_max_turns: default_subagent_turns(),
            subagent_runtime_secs: default_subagent_runtime(),
            subagent_output_bytes: default_subagent_output(),
            modes: HashMap::new(),
        }
    }
}

impl Default for CaConfig {
    fn default() -> Self {
        Self {
            context_pruning: crate::context_pruning::PruningConfig::default(),
            formatting: crate::formatting::FormatterConfig::default(),
            providers: HashMap::new(),
            defaults: DefaultsConfig {
                provider: default_provider(),
                model: default_model(),
            },
            route_presets: BTreeMap::new(),
            delegation: DelegationConfig::default(),
            agent: AgentConfig::default(),
            approval_rules: Vec::new(),
            permission_rules: Vec::new(),
            model_context: HashMap::new(),
            model_profiles: HashMap::new(),
            model_prompt_cache: HashMap::new(),
            skills: SkillsConfig::default(),
            mcp: McpConfig::default(),
            lsp: LspConfig::default(),
            review: ReviewConfig::default(),
            pricing: HashMap::new(),
            provider_billing: HashMap::new(),
            trusted_workspaces: Vec::new(),
            hooks: Vec::new(),
            keymap: crate::keymap::KeymapConfig::new(),
            sessions: SessionsConfig::default(),
            fallbacks: Vec::new(),
            provider_filter: crate::provider_filter::ProviderFilterConfig::default(),
            memory: MemoryConfig::default(),
            learning: crate::learning::LearningConfig::default(),
            harness_improvement: crate::harness_idle::AutomationConfig::default(),
            voice: VoiceConfig::default(),
            email: EmailConfig::default(),
            goals: crate::goal::GoalConfig::default(),
            ui: crate::ui_settings::UiConfig::default(),
        }
    }
}

impl CaConfig {
    pub fn session_retention_settings(
        &self,
    ) -> anyhow::Result<crate::session_retention::RetentionSettings> {
        let count = self
            .sessions
            .retention_count
            .map(usize::try_from)
            .transpose()
            .map_err(|_| anyhow::anyhow!("retention count is too large"))?;
        crate::session_retention::RetentionSettings {
            days: self.sessions.retention_days,
            count,
        }
        .validate()
        .map_err(anyhow::Error::msg)
    }

    pub fn model_profile(&self, provider: &str, model: &str) -> Option<&ModelProfile> {
        self.model_profiles.get(&format!("{provider}/{model}"))
    }

    pub fn model_prompt_cache(
        &self,
        provider: &str,
        model: &str,
    ) -> Option<&ModelPromptCacheConfig> {
        self.model_prompt_cache.get(&format!("{provider}/{model}"))
    }

    pub fn effective_model_profile(&self, provider: &str, model: &str) -> Option<ModelProfile> {
        self.model_profile(provider, model).cloned().or_else(|| {
            self.providers.get(provider).and_then(|provider_config| {
                crate::provider_contract::built_in_model_profile(provider, provider_config, model)
            })
        })
    }

    pub fn context_for_model(&self, provider: &str, model: &str) -> Option<ModelContextConfig> {
        let configured_profile = self.model_profile(provider, model);
        let built_in_profile = self.providers.get(provider).and_then(|provider_config| {
            crate::provider_contract::built_in_model_profile(provider, provider_config, model)
        });
        let exact = self.model_context.get(&format!("{provider}/{model}"));
        let legacy = self.model_context.get(model);
        let context_window_tokens = configured_profile
            .and_then(|profile| profile.context_window_tokens)
            .or_else(|| exact.and_then(|context| context.context_window_tokens))
            .or_else(|| legacy.and_then(|context| context.context_window_tokens))
            .or_else(|| {
                built_in_profile
                    .as_ref()
                    .and_then(|profile| profile.context_window_tokens)
            });
        let compact_at_tokens = configured_profile
            .and_then(|profile| profile.compact_at_tokens)
            .or_else(|| exact.and_then(|context| context.compact_at_tokens))
            .or_else(|| legacy.and_then(|context| context.compact_at_tokens))
            .or_else(|| {
                built_in_profile
                    .as_ref()
                    .and_then(|profile| profile.compact_at_tokens)
            });
        (context_window_tokens.is_some() || compact_at_tokens.is_some()).then_some(
            ModelContextConfig {
                context_window_tokens,
                compact_at_tokens,
            },
        )
    }

    pub fn context_limit_fallback(
        &self,
        provider: &str,
        model: &str,
        editing_profile: bool,
    ) -> ModelContextConfig {
        let exact = editing_profile
            .then(|| self.model_context.get(&format!("{provider}/{model}")))
            .flatten();
        let legacy = self.model_context.get(model);
        let built_in = self.providers.get(provider).and_then(|provider_config| {
            crate::provider_contract::built_in_model_profile(provider, provider_config, model)
        });
        ModelContextConfig {
            context_window_tokens: exact
                .and_then(|context| context.context_window_tokens)
                .or_else(|| legacy.and_then(|context| context.context_window_tokens))
                .or_else(|| {
                    built_in
                        .as_ref()
                        .and_then(|profile| profile.context_window_tokens)
                }),
            compact_at_tokens: exact
                .and_then(|context| context.compact_at_tokens)
                .or_else(|| legacy.and_then(|context| context.compact_at_tokens))
                .or_else(|| {
                    built_in
                        .as_ref()
                        .and_then(|profile| profile.compact_at_tokens)
                }),
        }
    }

    pub fn context_limit_settings(&self, provider: &str, model: &str) -> ContextLimitSettings {
        let exact_key = format!("{provider}/{model}");
        let editing_profile = self.model_profiles.contains_key(&exact_key);
        let configured = if editing_profile {
            self.model_profiles
                .get(&exact_key)
                .map(|profile| ModelContextConfig {
                    context_window_tokens: profile.context_window_tokens,
                    compact_at_tokens: profile.compact_at_tokens,
                })
                .unwrap_or_default()
        } else {
            self.model_context
                .get(&exact_key)
                .cloned()
                .unwrap_or_default()
        };
        ContextLimitSettings {
            configured,
            fallback: self.context_limit_fallback(provider, model, editing_profile),
            editing_profile,
        }
    }

    pub fn load() -> anyhow::Result<Self> {
        let paths = crate::runtime::RuntimePaths::discover()?;
        ConfigStore::new(paths.config_file).load()
    }

    pub fn default_config() -> Self {
        let mut cfg = Self::default();
        cfg.providers.insert(
            "deepseek".into(),
            ProviderConfig {
                google: None,
                tool_schema_policy: Default::default(),
                base_url: "https://api.deepseek.com/v1".into(),
                api_key: "${DEEPSEEK_API_KEY}".into(),
                protocol: crate::agent::ProviderProtocol::default(),
                prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
                models: vec!["deepseek-v4-flash".into(), "deepseek-v4-pro".into()],
                timeout: ProviderTimeoutConfig::default(),
            },
        );
        cfg.providers.insert(
            "ofox".into(),
            ProviderConfig {
                google: None,
                tool_schema_policy: Default::default(),
                base_url: "https://api.ofox.ai/v1".into(),
                api_key: "${OFOX_API_KEY}".into(),
                protocol: crate::agent::ProviderProtocol::default(),
                prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
                models: vec!["volcengine/doubao-seed-2.0-pro".into()],
                timeout: ProviderTimeoutConfig::default(),
            },
        );
        cfg.providers.insert(
            "ollama".into(),
            ProviderConfig {
                google: None,
                tool_schema_policy: Default::default(),
                base_url: "http://127.0.0.1:11434/v1".into(),
                api_key: "ollama".into(),
                protocol: crate::agent::ProviderProtocol::default(),
                prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
                models: vec!["deepseek-coder-v2:16b".into()],
                timeout: ProviderTimeoutConfig::default(),
            },
        );
        cfg.providers.insert(
            "mimo".into(),
            ProviderConfig {
                google: None,
                tool_schema_policy: Default::default(),
                base_url: "https://token-plan-sgp.xiaomimimo.com/v1".into(),
                api_key: "${MIMO_API_KEY}".into(),
                protocol: crate::agent::ProviderProtocol::OpenAiResponses,
                prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
                models: vec!["mimo-v2.5".into(), "mimo-v2.5-pro".into()],
                timeout: ProviderTimeoutConfig::default(),
            },
        );
        cfg.defaults.provider = "deepseek".into();
        cfg.defaults.model = "auto".into();
        cfg
    }
}

#[derive(Clone)]
pub struct ConfigStore {
    path: PathBuf,
    provider_secrets: Arc<dyn crate::provider_secrets::ProviderSecretStore>,
    profile: crate::runtime::ActiveProfile,
    create_on_load: bool,
    writes_enabled: bool,
}

const ROLE_RUN_PROFILE_CONFIG_MAX_BYTES: u64 = 1024 * 1024;
const ROLE_RUN_MAX_PROVIDERS: usize = 64;
const ROLE_RUN_MAX_MODELS_PER_PROVIDER: usize = 256;
const ROLE_RUN_MAX_TOTAL_MODELS: usize = 1024;
const ROLE_RUN_MAX_PROVIDER_ID_BYTES: usize = 128;

struct RoleRunReadOnlyProviderSecretStore {
    files: crate::provider_secrets::FileProviderSecretStore,
}

impl std::fmt::Debug for RoleRunReadOnlyProviderSecretStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RoleRunReadOnlyProviderSecretStore([REDACTED])")
    }
}

impl crate::provider_secrets::ProviderSecretStore for RoleRunReadOnlyProviderSecretStore {
    fn load(&self, account: &str) -> anyhow::Result<Option<Vec<u8>>> {
        let provider = account
            .strip_prefix("profile/")
            .and_then(|scoped| scoped.split_once("/provider/"))
            .map_or(account, |(_, provider)| provider);
        crate::provider_secrets::ProviderSecretStore::load(&self.files, provider)
    }

    fn store(&self, _account: &str, _secret: &[u8]) -> anyhow::Result<()> {
        anyhow::bail!("role-run credential access is read-only")
    }

    fn delete(&self, _account: &str) -> anyhow::Result<()> {
        anyhow::bail!("role-run credential access is read-only")
    }
}

fn role_run_profile_name(profile_name: &str) -> anyhow::Result<String> {
    match role_run_named_profile(profile_name)? {
        crate::runtime::ActiveProfile::Named(name) => Ok(name),
        crate::runtime::ActiveProfile::Default | crate::runtime::ActiveProfile::Bare => {
            unreachable!("role_run_named_profile returns only named profiles")
        }
    }
}

fn role_run_named_profile(profile_name: &str) -> anyhow::Result<crate::runtime::ActiveProfile> {
    if matches!(profile_name, "default" | "bare") {
        anyhow::bail!("role-run requires a nonreserved named profile");
    }
    match crate::runtime::ActiveProfile::parse(Some(profile_name)) {
        Ok(profile @ crate::runtime::ActiveProfile::Named(_)) => Ok(profile),
        Ok(crate::runtime::ActiveProfile::Default | crate::runtime::ActiveProfile::Bare) => {
            anyhow::bail!("role-run requires a nonreserved named profile")
        }
        Err(_) => anyhow::bail!(
            "role-run profile names must be 1-64 lowercase ASCII letters, digits, '-' or '_'"
        ),
    }
}

fn read_role_run_profile_config(path: &Path) -> anyhow::Result<CaConfig> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|_| anyhow::anyhow!("role-run profile configuration is unavailable"))?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() > ROLE_RUN_PROFILE_CONFIG_MAX_BYTES
    {
        anyhow::bail!("role-run profile configuration must be a bounded regular file");
    }

    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
    }
    let file = options
        .open(path)
        .map_err(|_| anyhow::anyhow!("role-run profile configuration is unavailable"))?;
    let opened_metadata = file
        .metadata()
        .map_err(|_| anyhow::anyhow!("role-run profile configuration is unavailable"))?;
    if !opened_metadata.is_file() || opened_metadata.len() > ROLE_RUN_PROFILE_CONFIG_MAX_BYTES {
        anyhow::bail!("role-run profile configuration must be a bounded regular file");
    }
    let mut bytes = Vec::with_capacity(opened_metadata.len() as usize);
    file.take(ROLE_RUN_PROFILE_CONFIG_MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| anyhow::anyhow!("role-run profile configuration could not be read"))?;
    if bytes.len() as u64 > ROLE_RUN_PROFILE_CONFIG_MAX_BYTES {
        anyhow::bail!("role-run profile configuration exceeds the size limit");
    }
    let raw = std::str::from_utf8(&bytes)
        .map_err(|_| anyhow::anyhow!("role-run profile configuration must be UTF-8"))?;
    toml::from_str(raw)
        .map_err(|_| anyhow::anyhow!("role-run profile configuration is invalid TOML"))
}

fn role_run_profile_shape(
    config: &CaConfig,
    profile_name: String,
) -> anyhow::Result<RoleRunProfileShape> {
    let sandbox_mode = crate::sandbox::SandboxMode::from_saved_value(&config.agent.sandbox_mode)
        .map_err(|_| anyhow::anyhow!("role-run profile has an invalid saved sandbox mode"))?;
    if config.providers.len() > ROLE_RUN_MAX_PROVIDERS {
        anyhow::bail!("role-run profile configures too many providers");
    }
    validate_role_run_legacy_denies(&config.approval_rules)?;
    for rule in &config.permission_rules {
        rule.validate()
            .map_err(|_| anyhow::anyhow!("role-run profile contains an invalid permission rule"))?;
    }

    let mut total_models = 0usize;
    let mut providers = Vec::with_capacity(config.providers.len());
    for (provider_id, provider) in &config.providers {
        validate_role_run_provider_id(provider_id)?;
        validate_base_url(&provider.base_url, true)
            .map_err(|_| anyhow::anyhow!("role-run provider has an invalid HTTP base URL"))?;
        if provider.models.len() > ROLE_RUN_MAX_MODELS_PER_PROVIDER {
            anyhow::bail!("role-run provider configures too many models");
        }
        total_models = total_models
            .checked_add(provider.models.len())
            .ok_or_else(|| anyhow::anyhow!("role-run profile configures too many models"))?;
        if total_models > ROLE_RUN_MAX_TOTAL_MODELS {
            anyhow::bail!("role-run profile configures too many models");
        }

        let mut seen_models = std::collections::HashSet::with_capacity(provider.models.len());
        let mut models = Vec::with_capacity(provider.models.len());
        for model_id in &provider.models {
            validate_model_id(model_id)
                .map_err(|_| anyhow::anyhow!("role-run provider has an invalid model ID"))?;
            if !seen_models.insert(model_id.as_str()) {
                anyhow::bail!("role-run provider contains a duplicate model ID");
            }
            let model_profile = config
                .effective_model_profile(provider_id, model_id)
                .unwrap_or_default();
            validate_role_run_model_profile(&model_profile)?;
            validate_model_tool_call_format(
                &model_profile,
                provider.protocol,
                &format!("{provider_id}/{model_id}"),
            )?;
            models.push(RoleRunModelShape {
                id: model_id.clone(),
                profile: model_profile,
            });
        }
        providers.push(RoleRunProviderShape {
            id: provider_id.clone(),
            models,
        });
    }
    providers.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(RoleRunProfileShape {
        profile: profile_name,
        sandbox_mode,
        providers,
    })
}

fn validate_role_run_legacy_denies(rules: &[ApprovalRule]) -> anyhow::Result<()> {
    for rule in rules {
        let matcher = match rule {
            ApprovalRule::DenyTool(tool) => {
                crate::permission::PermissionMatcher::Tool { tool: tool.clone() }
            }
            ApprovalRule::DenyPath { tool, path_pattern } => {
                crate::permission::PermissionMatcher::Path {
                    tool: tool.clone(),
                    path_pattern: path_pattern.clone(),
                }
            }
            ApprovalRule::AllowTool(_) | ApprovalRule::AllowPath { .. } => continue,
        };
        matcher.validate().map_err(|_| {
            anyhow::anyhow!("role-run profile contains an invalid legacy deny rule")
        })?;
    }
    Ok(())
}

fn validate_role_run_provider_id(provider_id: &str) -> anyhow::Result<()> {
    if provider_id.is_empty()
        || provider_id.len() > ROLE_RUN_MAX_PROVIDER_ID_BYTES
        || provider_id.trim() != provider_id
        || !provider_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        anyhow::bail!("role-run provider has an invalid provider ID");
    }
    Ok(())
}

fn validate_role_run_model_profile(profile: &ModelProfile) -> anyhow::Result<()> {
    for value in [profile.context_window_tokens, profile.compact_at_tokens]
        .into_iter()
        .flatten()
    {
        if value == 0 || value > i64::MAX as u64 {
            anyhow::bail!("role-run model has invalid context limits");
        }
    }
    if let Some(threshold) = profile.compact_at_tokens {
        let window = profile.context_window_tokens.ok_or_else(|| {
            anyhow::anyhow!("role-run model checkpoint threshold has no context window")
        })?;
        if threshold >= window {
            anyhow::bail!("role-run model checkpoint threshold is outside its context window");
        }
    }
    for (index, effort) in profile.reasoning_efforts.iter().enumerate() {
        if profile.reasoning_efforts[..index].contains(effort) {
            anyhow::bail!("role-run model contains duplicate reasoning efforts");
        }
    }
    if profile
        .default_reasoning_effort
        .is_some_and(|effort| !profile.reasoning_efforts.contains(&effort))
    {
        anyhow::bail!("role-run model default reasoning effort is unsupported");
    }
    if profile
        .reasoning_stream_field
        .as_ref()
        .is_some_and(|field| {
            field.is_empty() || field.len() > 128 || field.chars().any(char::is_control)
        })
    {
        anyhow::bail!("role-run model has an invalid reasoning stream field");
    }
    Ok(())
}

impl std::fmt::Debug for ConfigStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ConfigStore")
            .field("path", &self.path)
            .field("profile", &self.profile.label())
            .field("writes_enabled", &self.writes_enabled)
            .field("provider_secrets", &"[REDACTED]")
            .finish()
    }
}

impl ConfigStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            provider_secrets: crate::provider_secrets::system_provider_secret_store(
                &crate::runtime::ActiveProfile::Default,
            ),
            profile: crate::runtime::ActiveProfile::Default,
            create_on_load: true,
            writes_enabled: true,
        }
    }

    pub fn for_profile(path: impl Into<PathBuf>, profile: crate::runtime::ActiveProfile) -> Self {
        let create_on_load = matches!(profile, crate::runtime::ActiveProfile::Default);
        let writes_enabled = !matches!(profile, crate::runtime::ActiveProfile::Bare);
        Self {
            path: path.into(),
            provider_secrets: crate::provider_secrets::system_provider_secret_store(&profile),
            profile,
            create_on_load,
            writes_enabled,
        }
    }

    /// Load profile defaults in memory without creating a missing config file.
    /// Explicit later mutations still target this exact profile path.
    pub fn for_process_only_load(
        path: impl Into<PathBuf>,
        profile: crate::runtime::ActiveProfile,
    ) -> Self {
        let writes_enabled = !matches!(profile, crate::runtime::ActiveProfile::Bare);
        Self {
            path: path.into(),
            provider_secrets: crate::provider_secrets::system_provider_secret_store(&profile),
            profile,
            create_on_load: false,
            writes_enabled,
        }
    }

    /// Construct the normal-execution role-run store for one strict named
    /// profile. Unlike the general profile store this boundary never creates
    /// a missing config and never permits a configuration write.
    pub fn for_role_run_execution(
        path: impl Into<PathBuf>,
        profile_name: &str,
    ) -> anyhow::Result<Self> {
        let profile = role_run_named_profile(profile_name)?;
        let credential_root = crate::runtime::RuntimePaths::discover()?
            .profile_data_dir(&profile)
            .join("credentials");
        Ok(Self {
            path: path.into(),
            provider_secrets: Arc::new(RoleRunReadOnlyProviderSecretStore {
                files: crate::provider_secrets::FileProviderSecretStore::new(credential_root),
            }),
            profile,
            create_on_load: false,
            writes_enabled: false,
        })
    }

    /// Load one profile without creating or migrating its configuration file.
    /// Runtime credential resolution remains available to an explicitly
    /// authorized caller, but the load itself is strictly read-only.
    pub fn for_read_only_load(
        path: impl Into<PathBuf>,
        profile: crate::runtime::ActiveProfile,
    ) -> Self {
        Self {
            path: path.into(),
            provider_secrets: crate::provider_secrets::system_provider_secret_store(&profile),
            profile,
            create_on_load: false,
            writes_enabled: false,
        }
    }

    #[cfg(test)]
    pub fn with_secret_store(
        path: impl Into<PathBuf>,
        provider_secrets: Arc<dyn crate::provider_secrets::ProviderSecretStore>,
    ) -> Self {
        Self {
            path: path.into(),
            provider_secrets,
            profile: crate::runtime::ActiveProfile::Default,
            create_on_load: true,
            writes_enabled: true,
        }
    }

    #[cfg(test)]
    pub fn with_profile_secret_store(
        path: impl Into<PathBuf>,
        profile: crate::runtime::ActiveProfile,
        provider_secrets: Arc<dyn crate::provider_secrets::ProviderSecretStore>,
    ) -> Self {
        let create_on_load = matches!(profile, crate::runtime::ActiveProfile::Default);
        let writes_enabled = !matches!(profile, crate::runtime::ActiveProfile::Bare);
        Self {
            path: path.into(),
            provider_secrets,
            profile,
            create_on_load,
            writes_enabled,
        }
    }

    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    pub fn profile_label(&self) -> &str {
        self.profile.label()
    }

    pub fn creates_on_load(&self) -> bool {
        self.create_on_load
    }

    /// Read the bounded, secret-free shape of one strict named profile.
    ///
    /// This is intentionally an associated function rather than a normal
    /// `ConfigStore` load: it does not construct or consult a credential
    /// store, inspect environment-variable values, migrate legacy values,
    /// create a missing file or directory, or write any configuration.
    pub fn load_role_run_profile_shape(
        path: impl AsRef<Path>,
        profile_name: &str,
    ) -> anyhow::Result<RoleRunProfileShape> {
        let profile_name = role_run_profile_name(profile_name)?;
        let config = read_role_run_profile_config(path.as_ref())?;
        role_run_profile_shape(&config, profile_name)
    }

    /// Resolve exactly one configured route for normal role execution.
    ///
    /// The raw profile is read without migration or writes. Only the selected
    /// provider's credential declaration is resolved; credentials and
    /// extension settings belonging to every other provider remain untouched.
    pub fn load_role_run_selected_route(
        &self,
        provider_id: &str,
        model_id: &str,
        reasoning_effort: Option<ReasoningEffort>,
    ) -> anyhow::Result<RoleRunSelectedRoute> {
        self.load_role_run_selected_route_with_env(
            provider_id,
            model_id,
            reasoning_effort,
            crate::run_environment::var,
        )
    }

    fn load_role_run_selected_route_with_env<F>(
        &self,
        provider_id: &str,
        model_id: &str,
        reasoning_effort: Option<ReasoningEffort>,
        resolver: F,
    ) -> anyhow::Result<RoleRunSelectedRoute>
    where
        F: Fn(&str) -> Option<String>,
    {
        let profile_name = match &self.profile {
            crate::runtime::ActiveProfile::Named(name) => role_run_profile_name(name)?,
            crate::runtime::ActiveProfile::Default | crate::runtime::ActiveProfile::Bare => {
                anyhow::bail!("role-run requires a nonreserved named profile")
            }
        };
        let config = read_role_run_profile_config(&self.path)?;
        let shape = role_run_profile_shape(&config, profile_name)?;
        if shape.sandbox_mode != crate::sandbox::SandboxMode::Required {
            anyhow::bail!("role-run requires agent.sandbox_mode = \"required\"");
        }
        validate_role_run_provider_id(provider_id)?;
        validate_model_id(model_id)?;
        let provider_shape = shape
            .providers
            .iter()
            .find(|provider| provider.id == provider_id)
            .ok_or_else(|| anyhow::anyhow!("role-run provider is not configured"))?;
        let model_shape = provider_shape
            .models
            .iter()
            .find(|model| model.id == model_id)
            .ok_or_else(|| anyhow::anyhow!("role-run model is not configured for provider"))?;
        if let Some(effort) = reasoning_effort {
            if !model_shape.profile.reasoning_efforts.contains(&effort) {
                anyhow::bail!("role-run reasoning effort is not supported by the selected route");
            }
        }

        let mut provider = config
            .providers
            .get(provider_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("role-run provider is not configured"))?;
        self.resolve_role_run_provider_credential(provider_id, &mut provider, &resolver)?;
        provider.models = vec![model_id.to_string()];
        Ok(RoleRunSelectedRoute {
            provider_id: provider_id.to_string(),
            model_id: model_id.to_string(),
            provider,
            model_profile: model_shape.profile.clone(),
            reasoning_effort,
            sandbox_mode: shape.sandbox_mode,
            permission_rules: config.permission_rules,
            legacy_approval_rules: config.approval_rules,
        })
    }

    fn resolve_role_run_provider_credential<F>(
        &self,
        provider_id: &str,
        provider: &mut ProviderConfig,
        resolver: &F,
    ) -> anyhow::Result<()>
    where
        F: Fn(&str) -> Option<String>,
    {
        let raw_api_key = provider.api_key.trim();
        if let Some(reference_provider) = self.keychain_reference_provider(raw_api_key) {
            if reference_provider != provider_id {
                anyhow::bail!(
                    "saved API-key reference for role-run provider points to a different provider"
                );
            }
            let bytes = self
                .provider_secrets
                .load(&self.credential_account_for_reference(raw_api_key, provider_id))
                .map_err(|_| {
                    anyhow::anyhow!("saved credential for role-run provider could not be read")
                })?
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "saved credential for role-run provider is unavailable; restore it or replace the config entry with an environment variable"
                    )
                })?;
            provider.api_key = String::from_utf8(bytes)
                .map_err(|_| anyhow::anyhow!("API key for role-run provider is not valid UTF-8"))?;
        } else if crate::provider_secrets::resembles_saved_reference(raw_api_key) {
            anyhow::bail!("saved API-key reference for role-run provider is invalid");
        } else if provider_id.eq_ignore_ascii_case("mimo")
            && provider.api_key.trim() == "${XIAOMI_API_KEY}"
        {
            provider.api_key = resolver("XIAOMI_API_KEY")
                .or_else(|| resolver("MIMO_API_KEY"))
                .unwrap_or_else(|| provider.api_key.clone());
        } else {
            provider.api_key = expand_env_vars_with(&provider.api_key, resolver);
        }
        Ok(())
    }

    pub fn load(&self) -> anyhow::Result<CaConfig> {
        self.load_with_env(crate::run_environment::var)
    }

    pub fn load_with_env<F>(&self, resolver: F) -> anyhow::Result<CaConfig>
    where
        F: Fn(&str) -> Option<String>,
    {
        if !self.path.exists() && self.create_on_load {
            let raw = toml::to_string_pretty(&CaConfig::default_config())?;
            self.atomic_write(&raw)?;
        }
        let (raw, migrated_approval_mode) = self.migrate_legacy_approval_mode(self.raw_text()?)?;
        let mut config: CaConfig = toml::from_str(&raw)?;
        for (key, value) in &config.skills.config {
            let (selector, setting) = key
                .rsplit_once('.')
                .ok_or_else(|| anyhow::anyhow!("skill config key must be <skill>.<setting>"))?;
            if selector.is_empty() || key.len() > 512 {
                anyhow::bail!("skill config selector is invalid");
            }
            crate::skills::validate_setting_key(setting)
                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
            if [
                "secret",
                "token",
                "password",
                "credential",
                "api_key",
                "apikey",
            ]
            .iter()
            .any(|marker| setting.to_ascii_lowercase().contains(marker))
            {
                anyhow::bail!("credential-like skill config keys are not allowed");
            }
            crate::skills::validate_setting_value_safety(value)
                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        }
        validate_model_prompt_cache(&config)?;
        validate_pricing(&config)?;
        validate_review_config(&config)?;
        validate_provider_billing(&config)?;
        for (name, preset) in &config.route_presets {
            validate_route_preset_shape(name, preset)?;
        }
        validate_delegation(&config)?;
        validate_hooks(&config.hooks)?;
        config.mcp.mechanistic.validate(&config.mcp.servers)?;
        config.goals.validate()?;
        config.learning.validate()?;
        config.ui.validate()?;
        config.voice.validate(&config.providers)?;
        config.email.validate()?;
        crate::fallback::validate_rules(&config)?;
        for rule in &config.permission_rules {
            rule.validate()?;
        }
        config.session_retention_settings()?;
        crate::keymap::Keymap::from_config(&config.keymap)?;
        upgrade_discontinued_builtin_models(&mut config);
        for (name, provider) in &mut config.providers {
            let raw_api_key = provider.api_key.trim();
            if let Some(reference_provider) = self.keychain_reference_provider(raw_api_key) {
                if reference_provider != name {
                    anyhow::bail!(
                        "saved API-key reference for provider {name} points to a different provider"
                    );
                }
                let bytes = self
                    .provider_secrets
                    .load(&self.secret_account(name))?
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "saved credential for provider {name} is unavailable; restore it or replace the config entry with an environment variable"
                        )
                    })?;
                provider.api_key = String::from_utf8(bytes).map_err(|_| {
                    anyhow::anyhow!("API key for provider {name} isn't valid UTF-8")
                })?;
            } else if crate::provider_secrets::resembles_saved_reference(raw_api_key) {
                anyhow::bail!("saved API-key reference for provider {name} is invalid");
            } else if name.eq_ignore_ascii_case("mimo")
                && provider.api_key.trim() == "${XIAOMI_API_KEY}"
            {
                provider.api_key = resolver("XIAOMI_API_KEY")
                    .or_else(|| resolver("MIMO_API_KEY"))
                    .unwrap_or_else(|| provider.api_key.clone());
            } else {
                provider.api_key = expand_env_vars_with(&provider.api_key, &resolver);
            }
        }
        for server in config.mcp.servers.values_mut() {
            for value in server.env.values_mut() {
                *value = expand_env_vars_with(value, &resolver);
            }
        }
        for value in config.mcp.mechanistic.remote.env.values_mut() {
            *value = expand_env_vars_with(value, &resolver);
        }
        if migrated_approval_mode && self.writes_enabled {
            self.atomic_write(&raw)?;
        }
        crate::formatting::validate(&config.formatting).map_err(anyhow::Error::msg)?;
        config.provider_filter.validate()?;
        crate::provider_filter::install(&config.provider_filter)?;
        crate::formatting::install(&config).map_err(anyhow::Error::msg)?;
        crate::context_pruning::install(&config);
        Ok(config)
    }

    fn migrate_legacy_approval_mode(&self, raw: String) -> anyhow::Result<(String, bool)> {
        let mut document = raw.parse::<toml_edit::DocumentMut>()?;
        let legacy = document
            .get("agent")
            .and_then(toml_edit::Item::as_table)
            .and_then(|agent| agent.get("approval_mode"))
            .and_then(toml_edit::Item::as_str)
            == Some("allowlist");
        if !legacy {
            return Ok((raw, false));
        }
        let value = document
            .get_mut("agent")
            .and_then(toml_edit::Item::as_table_mut)
            .and_then(|agent| agent.get_mut("approval_mode"))
            .and_then(toml_edit::Item::as_value_mut)
            .ok_or_else(|| anyhow::anyhow!("legacy approval mode is not a TOML value"))?;
        let decoration = value.decor().clone();
        *value = toml_edit::Value::from("auto-review");
        *value.decor_mut() = decoration;
        let migrated = document.to_string();
        Ok((migrated, true))
    }

    pub fn provider_api_key_source(&self, provider: &str) -> anyhow::Result<ProviderApiKeySource> {
        let raw = self.raw_text()?;
        let config: CaConfig = toml::from_str(&raw)?;
        if config
            .providers
            .get(provider)
            .is_some_and(crate::google_auth::is_vertex)
        {
            return Ok(ProviderApiKeySource::ApplicationDefaultCredentials);
        }
        let Some(value) = config
            .providers
            .get(provider)
            .map(|provider| provider.api_key.trim())
        else {
            return Ok(ProviderApiKeySource::None);
        };
        Ok(self.provider_api_key_source_value(provider, value))
    }

    pub fn provider_management_entries(&self) -> anyhow::Result<Vec<ProviderManagementEntry>> {
        let mut entries = self.provider_management_snapshot()?.providers;
        for (id, base_url) in [
            ("mimo", "https://api.xiaomimimo.com/v1"),
            ("openai-codex", "https://chatgpt.com/backend-api/codex"),
            ("deepseek", "https://api.deepseek.com/v1"),
            ("openai", "https://api.openai.com/v1"),
        ] {
            if entries.iter().all(|entry| entry.id != id) {
                entries.push(ProviderManagementEntry {
                    id: id.into(),
                    base_url: base_url.into(),
                    api_key_source: ProviderApiKeySource::None,
                    configured_models: Vec::new(),
                    configured: false,
                    connected: false,
                });
            }
        }
        if let Some(codex) = entries.iter_mut().find(|entry| entry.id == "openai-codex") {
            codex.connected = codex.configured
                && matches!(
                    crate::codex_auth::auth_status(),
                    crate::codex_auth::CodexAuthStatus::Connected
                );
            if codex.connected {
                codex.api_key_source = ProviderApiKeySource::Hidden;
            }
        }
        entries.sort_by_key(|entry| {
            ["mimo", "openai-codex", "deepseek", "openai"]
                .iter()
                .position(|id| *id == entry.id)
                .unwrap_or(usize::MAX)
        });
        Ok(entries)
    }

    pub fn provider_management_snapshot(&self) -> anyhow::Result<ProviderManagementSnapshot> {
        self.provider_management_snapshot_with_secret_check(true)
    }

    pub fn provider_management_snapshot_offline(
        &self,
    ) -> anyhow::Result<ProviderManagementSnapshot> {
        self.provider_management_snapshot_with_secret_check(false)
    }

    fn provider_management_snapshot_with_secret_check(
        &self,
        check_saved_secret: bool,
    ) -> anyhow::Result<ProviderManagementSnapshot> {
        let raw = self.raw_text()?;
        let config: CaConfig = toml::from_str(&raw)?;
        let mut entries = config
            .providers
            .iter()
            .map(|(id, provider)| ProviderManagementEntry {
                api_key_source: if crate::google_auth::is_vertex(provider) {
                    ProviderApiKeySource::ApplicationDefaultCredentials
                } else {
                    self.provider_api_key_source_value(id, provider.api_key.trim())
                },
                id: id.clone(),
                base_url: safe_provider_base_url(&provider.base_url),
                configured_models: provider.models.clone(),
                configured: true,
                connected: if check_saved_secret {
                    self.provider_credential_available(id, provider.api_key.trim())
                } else {
                    self.provider_credential_declared(id, provider.api_key.trim())
                },
            })
            .collect::<Vec<_>>();
        entries.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(ProviderManagementSnapshot {
            default_provider: config.defaults.provider,
            default_model: config.defaults.model,
            sandbox_mode: config.agent.sandbox_mode,
            approval_mode: config.agent.approval_mode,
            providers: entries,
        })
    }

    pub fn provider_billing_metadata(
        &self,
    ) -> anyhow::Result<HashMap<String, crate::pricing::ProviderBillingConfig>> {
        let raw = self.raw_text()?;
        let config: CaConfig = toml::from_str(&raw)?;
        validate_provider_billing(&config)?;
        Ok(config.provider_billing)
    }

    pub fn update_provider_billing(
        &self,
        provider: &str,
        billing: &crate::pricing::ProviderBillingConfig,
    ) -> anyhow::Result<()> {
        validate_provider_id(provider)?;
        let raw = self.raw_text()?;
        let mut parsed: CaConfig = toml::from_str(&raw)?;
        let configured = parsed
            .providers
            .get(provider)
            .ok_or_else(|| anyhow::anyhow!("provider {provider} is no longer configured"))?;
        billing
            .validate_for_provider(provider, &configured.base_url)
            .map_err(anyhow::Error::msg)?;
        parsed
            .provider_billing
            .insert(provider.to_string(), billing.clone());

        #[derive(Serialize)]
        struct BillingPatch<'a> {
            provider_billing: &'a HashMap<String, crate::pricing::ProviderBillingConfig>,
        }
        let mut replacement = toml_edit::ser::to_document(&BillingPatch {
            provider_billing: &parsed.provider_billing,
        })?;
        let mut document = raw.parse::<toml_edit::DocumentMut>()?;
        document["provider_billing"] = replacement
            .remove("provider_billing")
            .ok_or_else(|| anyhow::anyhow!("failed to serialize provider billing"))?;
        let updated = document.to_string();
        let validated: CaConfig = toml::from_str(&updated)?;
        validate_provider_billing(&validated)?;
        self.atomic_write(&updated)
    }

    #[allow(dead_code)] // legacy config compatibility; new writes use permission_rules
    pub fn append_approval_rule(&self, rule: &ApprovalRule) -> anyhow::Result<()> {
        let raw = self.raw_text()?;
        let mut document = raw.parse::<toml_edit::DocumentMut>()?;
        let mut config: CaConfig = toml::from_str(&raw)?;
        config.approval_rules.push(rule.clone());
        #[derive(Serialize)]
        struct Rules<'a> {
            approval_rules: &'a [ApprovalRule],
        }
        let mut replacement = toml_edit::ser::to_document(&Rules {
            approval_rules: &config.approval_rules,
        })?;
        document["approval_rules"] = replacement
            .remove("approval_rules")
            .ok_or_else(|| anyhow::anyhow!("failed to serialize approval rules"))?;
        self.atomic_write(&document.to_string())
    }

    pub fn append_permission_rule(
        &self,
        rule: &crate::permission::PermissionRule,
    ) -> anyhow::Result<()> {
        rule.validate()?;
        let raw = self.raw_text()?;
        let mut document = raw.parse::<toml_edit::DocumentMut>()?;
        let mut config: CaConfig = toml::from_str(&raw)?;
        config.permission_rules.push(rule.clone());
        replace_permission_rules(&mut document, &config.permission_rules)?;
        self.atomic_write(&document.to_string())
    }

    pub fn remove_permission_rule(&self, index: usize) -> anyhow::Result<()> {
        let raw = self.raw_text()?;
        let mut document = raw.parse::<toml_edit::DocumentMut>()?;
        let mut config: CaConfig = toml::from_str(&raw)?;
        if index >= config.permission_rules.len() {
            anyhow::bail!("permission rule no longer exists");
        }
        config.permission_rules.remove(index);
        replace_permission_rules(&mut document, &config.permission_rules)?;
        self.atomic_write(&document.to_string())
    }

    pub fn set_memory_scope(
        &self,
        scope: crate::memory::MemoryScope,
        recall: bool,
        generation: bool,
    ) -> anyhow::Result<()> {
        let raw = self.raw_text()?;
        let mut document = raw.parse::<toml_edit::DocumentMut>()?;
        let mut config: CaConfig = toml::from_str(&raw)?;
        let settings = match scope {
            crate::memory::MemoryScope::User => &mut config.memory.user,
            crate::memory::MemoryScope::Workspace => &mut config.memory.workspace,
        };
        settings.recall = recall;
        settings.generation = generation;
        #[derive(Serialize)]
        struct MemoryPatch<'a> {
            memory: &'a MemoryConfig,
        }
        let mut replacement = toml_edit::ser::to_document(&MemoryPatch {
            memory: &config.memory,
        })?;
        document["memory"] = replacement
            .remove("memory")
            .ok_or_else(|| anyhow::anyhow!("failed to serialize memory settings"))?;
        self.atomic_write(&document.to_string())
    }

    pub fn set_learning_enabled(
        &self,
        kind: crate::learning::MaintenanceKind,
        enabled: bool,
    ) -> anyhow::Result<()> {
        let raw = self.raw_text()?;
        let mut document = raw.parse::<toml_edit::DocumentMut>()?;
        let mut config: CaConfig = toml::from_str(&raw)?;
        config.learning.settings_mut(kind).enabled = enabled;
        #[derive(Serialize)]
        struct LearningPatch<'a> {
            learning: &'a crate::learning::LearningConfig,
        }
        let mut replacement = toml_edit::ser::to_document(&LearningPatch {
            learning: &config.learning,
        })?;
        document["learning"] = replacement
            .remove("learning")
            .ok_or_else(|| anyhow::anyhow!("failed to serialize learning settings"))?;
        self.atomic_write(&document.to_string())
    }

    pub fn remove_approval_rule(&self, index: usize) -> anyhow::Result<()> {
        let raw = self.raw_text()?;
        let mut document = raw.parse::<toml_edit::DocumentMut>()?;
        let mut config: CaConfig = toml::from_str(&raw)?;
        if index >= config.approval_rules.len() {
            anyhow::bail!("legacy approval rule no longer exists");
        }
        config.approval_rules.remove(index);
        #[derive(Serialize)]
        struct Rules<'a> {
            approval_rules: &'a [ApprovalRule],
        }
        let mut replacement = toml_edit::ser::to_document(&Rules {
            approval_rules: &config.approval_rules,
        })?;
        document["approval_rules"] = replacement
            .remove("approval_rules")
            .ok_or_else(|| anyhow::anyhow!("failed to serialize approval rules"))?;
        self.atomic_write(&document.to_string())
    }

    pub fn select_model(&self, provider: &str, model: &str) -> anyhow::Result<()> {
        if model.is_empty()
            || model.len() > 256
            || model.trim() != model
            || model.chars().any(char::is_control)
        {
            anyhow::bail!("invalid model ID");
        }
        let raw = self.raw_text()?;
        let mut document = raw.parse::<toml_edit::DocumentMut>()?;
        let config: CaConfig = toml::from_str(&raw)?;
        if !config.providers.contains_key(provider) {
            anyhow::bail!("provider {provider} is not configured");
        }
        let provider_table = document["providers"]
            .get_mut(provider)
            .and_then(toml_edit::Item::as_table_mut)
            .ok_or_else(|| anyhow::anyhow!("provider {provider} must be a TOML table"))?;
        if !provider_table.contains_key("models") {
            provider_table["models"] = toml_edit::value(toml_edit::Array::new());
        }
        let models = provider_table["models"]
            .as_array_mut()
            .ok_or_else(|| anyhow::anyhow!("provider {provider} models must be an array"))?;
        if !models
            .iter()
            .any(|configured| configured.as_str() == Some(model))
        {
            models.push(model);
        }
        document["defaults"]["provider"] = toml_edit::value(provider);
        document["defaults"]["model"] = toml_edit::value(model);
        self.atomic_write(&document.to_string())
    }

    pub fn select_review_model(&self, provider: &str, model: &str) -> anyhow::Result<()> {
        if provider.is_empty()
            || model.is_empty()
            || model.len() > 256
            || model.trim() != model
            || model.chars().any(char::is_control)
        {
            anyhow::bail!("invalid review model selection");
        }
        let raw = self.raw_text()?;
        let mut document = raw.parse::<toml_edit::DocumentMut>()?;
        let config: CaConfig = toml::from_str(&raw)?;
        if !config.providers.contains_key(provider) {
            anyhow::bail!("provider {provider} is not configured");
        }
        let provider_table = document["providers"]
            .get_mut(provider)
            .and_then(toml_edit::Item::as_table_mut)
            .ok_or_else(|| anyhow::anyhow!("provider {provider} must be a TOML table"))?;
        if !provider_table.contains_key("models") {
            provider_table["models"] = toml_edit::value(toml_edit::Array::new());
        }
        let models = provider_table["models"]
            .as_array_mut()
            .ok_or_else(|| anyhow::anyhow!("provider {provider} models must be an array"))?;
        if !models
            .iter()
            .any(|configured| configured.as_str() == Some(model))
        {
            models.push(model);
        }
        document["review"]["provider"] = toml_edit::value(provider);
        document["review"]["model"] = toml_edit::value(model);
        self.atomic_write(&document.to_string())
    }

    pub fn update_sandbox_mode(&self, mode: crate::sandbox::SandboxMode) -> anyhow::Result<()> {
        let raw = self.raw_text()?;
        let mut document = raw.parse::<toml_edit::DocumentMut>()?;
        if let Some(value) = document["agent"]["sandbox_mode"].as_value_mut() {
            let decoration = value.decor().clone();
            *value = toml_edit::Value::from(mode.as_str());
            *value.decor_mut() = decoration;
        } else {
            document["agent"]["sandbox_mode"] = toml_edit::value(mode.as_str());
        }
        self.atomic_write(&document.to_string())
    }

    pub fn update_approval_mode(&self, mode: &str) -> anyhow::Result<()> {
        if !matches!(mode, "auto-review" | "ask") {
            anyhow::bail!("approval mode must be auto-review or ask");
        }
        let raw = self.raw_text()?;
        let mut document = raw.parse::<toml_edit::DocumentMut>()?;
        if !document.contains_key("agent") {
            document["agent"] = toml_edit::Item::Table(toml_edit::Table::new());
        }
        let agent = document["agent"]
            .as_table_mut()
            .ok_or_else(|| anyhow::anyhow!("agent must be a TOML table"))?;
        if let Some(value) = agent
            .get_mut("approval_mode")
            .and_then(toml_edit::Item::as_value_mut)
        {
            let decoration = value.decor().clone();
            *value = toml_edit::Value::from(mode);
            *value.decor_mut() = decoration;
        } else {
            agent["approval_mode"] = toml_edit::value(mode);
        }
        self.atomic_write(&document.to_string())
    }

    pub fn update_ui_theme(&self, theme: crate::ui_settings::UiTheme) -> anyhow::Result<()> {
        let raw = self.raw_text()?;
        let mut document = raw.parse::<toml_edit::DocumentMut>()?;
        if !document.contains_key("ui") {
            document["ui"] = toml_edit::Item::Table(toml_edit::Table::new());
        }
        let table = document["ui"]
            .as_table_mut()
            .ok_or_else(|| anyhow::anyhow!("ui must be a TOML table"))?;
        if let Some(value) = table
            .get_mut("theme")
            .and_then(toml_edit::Item::as_value_mut)
        {
            let decoration = value.decor().clone();
            *value = toml_edit::Value::from(theme.config_value());
            *value.decor_mut() = decoration;
        } else {
            table["theme"] = toml_edit::value(theme.config_value());
        }
        let updated = document.to_string();
        let config: CaConfig = toml::from_str(&updated)?;
        config.ui.validate()?;
        self.atomic_write(&updated)
    }

    pub fn update_notification(
        &self,
        notification: crate::ui_settings::NotificationStyle,
    ) -> anyhow::Result<()> {
        let raw = self.raw_text()?;
        let mut document = raw.parse::<toml_edit::DocumentMut>()?;
        if !document.contains_key("ui") {
            document["ui"] = toml_edit::Item::Table(toml_edit::Table::new());
        }
        let table = document["ui"]
            .as_table_mut()
            .ok_or_else(|| anyhow::anyhow!("ui must be a TOML table"))?;
        if let Some(value) = table
            .get_mut("notification")
            .and_then(toml_edit::Item::as_value_mut)
        {
            let decoration = value.decor().clone();
            *value = toml_edit::Value::from(notification.config_value());
            *value.decor_mut() = decoration;
        } else {
            table["notification"] = toml_edit::value(notification.config_value());
        }
        let updated = document.to_string();
        let config: CaConfig = toml::from_str(&updated)?;
        config.ui.validate()?;
        self.atomic_write(&updated)
    }

    pub fn update_compact_mode(&self, compact: bool) -> anyhow::Result<()> {
        let raw = self.raw_text()?;
        let mut document = raw.parse::<toml_edit::DocumentMut>()?;
        if !document.contains_key("ui") {
            document["ui"] = toml_edit::Item::Table(toml_edit::Table::new());
        }
        let table = document["ui"]
            .as_table_mut()
            .ok_or_else(|| anyhow::anyhow!("ui must be a TOML table"))?;
        if let Some(value) = table
            .get_mut("compact_mode")
            .and_then(toml_edit::Item::as_value_mut)
        {
            let decoration = value.decor().clone();
            *value = toml_edit::Value::from(compact);
            *value.decor_mut() = decoration;
        } else {
            table["compact_mode"] = toml_edit::value(compact);
        }
        let updated = document.to_string();
        self.atomic_write(&updated)
    }

    pub fn ui_theme_is_explicit(&self) -> anyhow::Result<bool> {
        if !self.path.is_file() {
            return Ok(false);
        }
        let raw = std::fs::read_to_string(&self.path)?;
        let document = raw.parse::<toml_edit::DocumentMut>()?;
        Ok(document
            .get("ui")
            .and_then(toml_edit::Item::as_table)
            .is_some_and(|ui| ui.contains_key("theme")))
    }

    pub fn update_status_indicators(&self, enabled: bool) -> anyhow::Result<()> {
        let raw = self.raw_text()?;
        let mut document = raw.parse::<toml_edit::DocumentMut>()?;
        if !document.contains_key("ui") {
            document["ui"] = toml_edit::Item::Table(toml_edit::Table::new());
        }
        let table = document["ui"]
            .as_table_mut()
            .ok_or_else(|| anyhow::anyhow!("ui must be a TOML table"))?;
        if let Some(value) = table
            .get_mut("status_indicators")
            .and_then(toml_edit::Item::as_value_mut)
        {
            let decoration = value.decor().clone();
            *value = toml_edit::Value::from(enabled);
            *value.decor_mut() = decoration;
        } else {
            table["status_indicators"] = toml_edit::value(enabled);
        }
        let updated = document.to_string();
        let config: CaConfig = toml::from_str(&updated)?;
        config.ui.validate()?;
        self.atomic_write(&updated)
    }

    pub fn update_mouse(&self, mouse: crate::ui_settings::MouseMode) -> anyhow::Result<()> {
        let raw = self.raw_text()?;
        let mut document = raw.parse::<toml_edit::DocumentMut>()?;
        if !document.contains_key("ui") {
            document["ui"] = toml_edit::Item::Table(toml_edit::Table::new());
        }
        let table = document["ui"]
            .as_table_mut()
            .ok_or_else(|| anyhow::anyhow!("ui must be a TOML table"))?;
        if let Some(value) = table
            .get_mut("mouse")
            .and_then(toml_edit::Item::as_value_mut)
        {
            let decoration = value.decor().clone();
            *value = toml_edit::Value::from(mouse.config_value());
            *value.decor_mut() = decoration;
        } else {
            table["mouse"] = toml_edit::value(mouse.config_value());
        }
        let updated = document.to_string();
        let config: CaConfig = toml::from_str(&updated)?;
        config.ui.validate()?;
        self.atomic_write(&updated)
    }

    pub fn update_command_output(&self, policy: CommandOutputPolicy) -> anyhow::Result<()> {
        let raw = self.raw_text()?;
        let mut document = raw.parse::<toml_edit::DocumentMut>()?;
        if !document.contains_key("agent") {
            document["agent"] = toml_edit::Item::Table(toml_edit::Table::new());
        }
        let table = document["agent"]
            .as_table_mut()
            .ok_or_else(|| anyhow::anyhow!("agent must be a TOML table"))?;
        if let Some(value) = table
            .get_mut("command_output")
            .and_then(toml_edit::Item::as_value_mut)
        {
            let decoration = value.decor().clone();
            *value = toml_edit::Value::from(policy.config_value());
            *value.decor_mut() = decoration;
        } else {
            table["command_output"] = toml_edit::value(policy.config_value());
        }
        let updated = document.to_string();
        let _: CaConfig = toml::from_str(&updated)?;
        self.atomic_write(&updated)
    }

    pub fn update_skill_config(
        &self,
        key: &str,
        setting: &crate::skills::SkillSettingValue,
    ) -> anyhow::Result<()> {
        if !self.writes_enabled {
            anyhow::bail!("skill configuration is read-only in this profile");
        }
        crate::skills::validate_setting_value_safety(setting)
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        let (_, leaf) = key
            .rsplit_once('.')
            .ok_or_else(|| anyhow::anyhow!("skill config key must be <skill>.<setting>"))?;
        if leaf.is_empty()
            || [
                "secret",
                "token",
                "password",
                "credential",
                "api_key",
                "apikey",
            ]
            .iter()
            .any(|marker| leaf.to_ascii_lowercase().contains(marker))
        {
            anyhow::bail!("credential-like skill config keys are not allowed");
        }
        let raw = self.raw_text()?;
        let mut document = raw.parse::<toml_edit::DocumentMut>()?;
        if !document.contains_key("skills") {
            document["skills"] = toml_edit::Item::Table(toml_edit::Table::new());
        }
        let skills = document["skills"]
            .as_table_mut()
            .ok_or_else(|| anyhow::anyhow!("skills must be a TOML table"))?;
        if !skills.contains_key("config") {
            skills["config"] = toml_edit::Item::Table(toml_edit::Table::new());
        }
        let config = skills["config"]
            .as_table_mut()
            .ok_or_else(|| anyhow::anyhow!("skills.config must be a TOML table"))?;
        config[key] = match setting {
            crate::skills::SkillSettingValue::String(value) => toml_edit::value(value.clone()),
            crate::skills::SkillSettingValue::Integer(value) => toml_edit::value(*value),
            crate::skills::SkillSettingValue::Boolean(value) => toml_edit::value(*value),
            crate::skills::SkillSettingValue::Strings(values) => {
                let mut array = toml_edit::Array::new();
                for value in values {
                    array.push(value.as_str());
                }
                toml_edit::Item::Value(toml_edit::Value::Array(array))
            }
        };
        let updated = document.to_string();
        let _: CaConfig = toml::from_str(&updated)?;
        self.atomic_write(&updated)
    }

    pub fn update_route_preset(&self, name: &str, preset: &RoutePreset) -> anyhow::Result<()> {
        validate_route_preset_shape(name, preset)?;
        let raw = self.raw_text()?;
        let config: CaConfig = toml::from_str(&raw)?;
        validate_route_preset_for_config(&config, preset)?;
        let mut document = raw.parse::<toml_edit::DocumentMut>()?;
        if !document.contains_key("route_presets") {
            document["route_presets"] = toml_edit::Item::Table(toml_edit::Table::new());
        }
        let presets = document["route_presets"]
            .as_table_mut()
            .ok_or_else(|| anyhow::anyhow!("route_presets must be a TOML table"))?;
        if !presets.contains_key(name) {
            presets.insert(name, toml_edit::Item::Table(toml_edit::Table::new()));
        }
        let table = presets
            .get_mut(name)
            .and_then(toml_edit::Item::as_table_mut)
            .ok_or_else(|| anyhow::anyhow!("route_presets.{name} must be a TOML table"))?;
        patch_toml_string(table, "provider", &preset.provider);
        patch_toml_string(table, "model", &preset.model);
        match preset.reasoning_effort {
            Some(effort) => patch_toml_string(table, "reasoning_effort", &effort.to_string()),
            None => {
                table.remove("reasoning_effort");
            }
        }
        let updated = document.to_string();
        let parsed: CaConfig = toml::from_str(&updated)?;
        for (name, preset) in &parsed.route_presets {
            validate_route_preset_shape(name, preset)?;
        }
        self.atomic_write(&updated)
    }

    pub fn update_delegation_route(&self, route: Option<&DelegationRoute>) -> anyhow::Result<()> {
        let raw = self.raw_text()?;
        let config: CaConfig = toml::from_str(&raw)?;
        if let Some(route) = route {
            validate_delegation_route(&config, route)?;
        }
        let mut document = raw.parse::<toml_edit::DocumentMut>()?;
        match route {
            Some(route) => {
                if !document.contains_key("delegation") {
                    document["delegation"] = toml_edit::Item::Table(toml_edit::Table::new());
                }
                let delegation = document["delegation"]
                    .as_table_mut()
                    .ok_or_else(|| anyhow::anyhow!("delegation must be a TOML table"))?;
                patch_toml_string(delegation, "mode", "fixed");
                delegation.remove("allowed_routes");
                if !delegation.contains_key("route") {
                    delegation.insert("route", toml_edit::Item::Table(toml_edit::Table::new()));
                }
                let table = delegation
                    .get_mut("route")
                    .and_then(toml_edit::Item::as_table_mut)
                    .ok_or_else(|| anyhow::anyhow!("delegation.route must be a TOML table"))?;
                patch_toml_string(table, "provider", &route.provider);
                patch_toml_string(table, "model", &route.model);
            }
            None => {
                document.remove("delegation");
            }
        }
        let updated = document.to_string();
        let parsed: CaConfig = toml::from_str(&updated)?;
        validate_delegation(&parsed)?;
        self.atomic_write(&updated)
    }

    pub fn update_delegation_mode(&self, mode: DelegationMode) -> anyhow::Result<()> {
        let raw = self.raw_text()?;
        let config: CaConfig = toml::from_str(&raw)?;
        let mut document = raw.parse::<toml_edit::DocumentMut>()?;
        match mode {
            DelegationMode::Same => {
                document.remove("delegation");
            }
            DelegationMode::Fixed => {
                anyhow::bail!("select a configured provider/model to enable fixed delegation");
            }
            DelegationMode::Learned => {
                if !document.contains_key("delegation") {
                    document["delegation"] = toml_edit::Item::Table(toml_edit::Table::new());
                }
                let table = document["delegation"]
                    .as_table_mut()
                    .ok_or_else(|| anyhow::anyhow!("delegation must be a TOML table"))?;
                patch_toml_string(table, "mode", "learned");
                table.remove("route");
                let mut routes = config
                    .providers
                    .iter()
                    .flat_map(|(provider, definition)| {
                        definition
                            .models
                            .iter()
                            .map(move |model| format!("{provider}/{model}"))
                    })
                    .collect::<Vec<_>>();
                routes.sort();
                routes.dedup();
                if routes.is_empty() {
                    anyhow::bail!("learned delegation requires at least one configured route");
                }
                let mut values = toml_edit::Array::new();
                for route in routes {
                    values.push(route);
                }
                table["allowed_routes"] = toml_edit::value(values);
            }
        }
        let updated = document.to_string();
        let parsed: CaConfig = toml::from_str(&updated)?;
        validate_delegation(&parsed)?;
        self.atomic_write(&updated)
    }

    pub fn update_statusline(
        &self,
        update: &crate::ui_settings::StatuslineUpdate,
    ) -> anyhow::Result<()> {
        let raw = self.raw_text()?;
        let mut config: CaConfig = toml::from_str(&raw)?;
        config.ui.statusline.fields = update.fields.clone();
        config.ui.statusline.custom_enabled = update.custom_enabled;
        config.ui.validate()?;

        let mut document = raw.parse::<toml_edit::DocumentMut>()?;
        if !document.contains_key("ui") {
            document["ui"] = toml_edit::Item::Table(toml_edit::Table::new());
        }
        let ui = document["ui"]
            .as_table_mut()
            .ok_or_else(|| anyhow::anyhow!("ui must be a TOML table"))?;
        if !ui.contains_key("statusline") {
            ui["statusline"] = toml_edit::Item::Table(toml_edit::Table::new());
        }
        let statusline = ui["statusline"]
            .as_table_mut()
            .ok_or_else(|| anyhow::anyhow!("ui.statusline must be a TOML table"))?;
        let mut fields = toml_edit::Array::new();
        for field in &update.fields {
            fields.push(field.config_value());
        }
        statusline["fields"] = toml_edit::value(fields);
        if let Some(value) = statusline
            .get_mut("custom_enabled")
            .and_then(toml_edit::Item::as_value_mut)
        {
            let decoration = value.decor().clone();
            *value = toml_edit::Value::from(update.custom_enabled);
            *value.decor_mut() = decoration;
        } else {
            statusline["custom_enabled"] = toml_edit::value(update.custom_enabled);
        }
        self.atomic_write(&document.to_string())
    }

    pub fn update_hook_enabled(
        &self,
        index: usize,
        expected_event: &str,
        enabled: bool,
    ) -> anyhow::Result<()> {
        let raw = self.raw_text()?;
        let config: CaConfig = toml::from_str(&raw)?;
        validate_hooks(&config.hooks)?;
        let hook = config
            .hooks
            .get(index)
            .ok_or_else(|| anyhow::anyhow!("hook entry changed; reload /hooks"))?;
        if hook.event != expected_event {
            anyhow::bail!("hook entry changed; reload /hooks");
        }
        if hook.enabled == enabled {
            return Ok(());
        }

        let mut document = raw.parse::<toml_edit::DocumentMut>()?;
        let hooks = document
            .get_mut("hooks")
            .and_then(toml_edit::Item::as_array_of_tables_mut)
            .ok_or_else(|| anyhow::anyhow!("hooks must be an array of tables"))?;
        let table = hooks
            .get_mut(index)
            .ok_or_else(|| anyhow::anyhow!("hook entry changed; reload /hooks"))?;
        let raw_event = table
            .get("event")
            .and_then(toml_edit::Item::as_str)
            .ok_or_else(|| anyhow::anyhow!("hook entry changed; reload /hooks"))?;
        if raw_event != expected_event {
            anyhow::bail!("hook entry changed; reload /hooks");
        }
        if let Some(value) = table
            .get_mut("enabled")
            .and_then(toml_edit::Item::as_value_mut)
        {
            let decoration = value.decor().clone();
            *value = toml_edit::Value::from(enabled);
            *value.decor_mut() = decoration;
        } else {
            table["enabled"] = toml_edit::value(enabled);
        }
        self.atomic_write(&document.to_string())
    }

    pub fn hook_configs(&self) -> anyhow::Result<Vec<HookConfig>> {
        let raw = self.raw_text()?;
        let config: CaConfig = toml::from_str(&raw)?;
        validate_hooks(&config.hooks)?;
        Ok(config.hooks)
    }

    pub fn update_context_limits(&self, update: &ContextLimitUpdate) -> anyhow::Result<()> {
        validate_provider_id(&update.provider)?;
        validate_model_id(&update.model)?;
        for (label, value) in [
            ("context window", update.context_window_tokens),
            ("checkpoint threshold", update.compact_at_tokens),
        ] {
            if value == Some(0) {
                anyhow::bail!("{label} must be a positive integer");
            }
            if value.is_some_and(|value| value > i64::MAX as u64) {
                anyhow::bail!("{label} is too large for TOML");
            }
        }

        let raw = self.raw_text()?;
        let config: CaConfig = toml::from_str(&raw)?;
        if !config.providers.contains_key(&update.provider) {
            anyhow::bail!("provider {} is not configured", update.provider);
        }
        let exact_key = format!("{}/{}", update.provider, update.model);
        let editing_profile = config.model_profiles.contains_key(&exact_key);
        let fallback =
            config.context_limit_fallback(&update.provider, &update.model, editing_profile);
        let effective_window = update
            .context_window_tokens
            .or(fallback.context_window_tokens);
        let effective_threshold = update.compact_at_tokens.or(fallback.compact_at_tokens);
        if let Some(threshold) = effective_threshold {
            let window = effective_window.ok_or_else(|| {
                anyhow::anyhow!("set a context window before setting a checkpoint threshold")
            })?;
            if threshold >= window {
                anyhow::bail!("checkpoint threshold must be below the context window");
            }
        }

        let section_name = if editing_profile {
            "model_profiles"
        } else {
            "model_context"
        };
        let exact_exists = if editing_profile {
            true
        } else {
            config.model_context.contains_key(&exact_key)
        };
        if !exact_exists
            && update.context_window_tokens.is_none()
            && update.compact_at_tokens.is_none()
        {
            return Ok(());
        }

        let mut document = raw.parse::<toml_edit::DocumentMut>()?;
        if !document.contains_key(section_name) {
            document[section_name] = toml_edit::Item::Table(toml_edit::Table::new());
        }
        let section = document[section_name]
            .as_table_mut()
            .ok_or_else(|| anyhow::anyhow!("{section_name} must be a TOML table"))?;
        if !section.contains_key(&exact_key) {
            section.insert(&exact_key, toml_edit::Item::Table(toml_edit::Table::new()));
        }
        let table = section
            .get_mut(&exact_key)
            .and_then(toml_edit::Item::as_table_mut)
            .ok_or_else(|| anyhow::anyhow!("{section_name}.{exact_key} must be a TOML table"))?;
        patch_optional_toml_integer(table, "context_window_tokens", update.context_window_tokens);
        patch_optional_toml_integer(table, "compact_at_tokens", update.compact_at_tokens);
        self.atomic_write(&document.to_string())
    }

    pub fn update_model_prompt_cache(&self, update: &ModelPromptCacheUpdate) -> anyhow::Result<()> {
        validate_provider_id(&update.provider)?;
        validate_model_id(&update.model)?;
        let raw = self.raw_text()?;
        let config: CaConfig = toml::from_str(&raw)?;
        if !config.providers.contains_key(&update.provider) {
            anyhow::bail!("provider {} is not configured", update.provider);
        }
        let exact_key = format!("{}/{}", update.provider, update.model);
        if !config.model_prompt_cache.contains_key(&exact_key)
            && update.request_format.is_none()
            && update.usage_format.is_none()
            && update.policy.is_none()
        {
            return Ok(());
        }

        let mut document = raw.parse::<toml_edit::DocumentMut>()?;
        if !document.contains_key("model_prompt_cache") {
            document["model_prompt_cache"] = toml_edit::Item::Table(toml_edit::Table::new());
        }
        let section = document["model_prompt_cache"]
            .as_table_mut()
            .ok_or_else(|| anyhow::anyhow!("model_prompt_cache must be a TOML table"))?;
        if !section.contains_key(&exact_key) {
            section.insert(&exact_key, toml_edit::Item::Table(toml_edit::Table::new()));
        }
        let table = section
            .get_mut(&exact_key)
            .and_then(toml_edit::Item::as_table_mut)
            .ok_or_else(|| {
                anyhow::anyhow!("model_prompt_cache.{exact_key} must be a TOML table")
            })?;
        patch_optional_toml_string(
            table,
            "request_format",
            update.request_format.map(PromptCacheRequestFormat::label),
        );
        patch_optional_toml_string(
            table,
            "usage_format",
            update.usage_format.map(PromptCacheUsageFormat::label),
        );
        patch_optional_toml_string(table, "policy", update.policy.map(PromptCachePolicy::label));
        let updated = document.to_string();
        let parsed: CaConfig = toml::from_str(&updated)?;
        validate_model_prompt_cache(&parsed)?;
        self.atomic_write(&updated)
    }

    pub fn update_session_retention(
        &self,
        settings: crate::session_retention::RetentionSettings,
    ) -> anyhow::Result<()> {
        let settings = settings.validate().map_err(anyhow::Error::msg)?;
        let raw = self.raw_text()?;
        let config: CaConfig = toml::from_str(&raw)?;
        if config.session_retention_settings()? == settings {
            return Ok(());
        }
        let mut document = raw.parse::<toml_edit::DocumentMut>()?;
        if !document.contains_key("sessions") {
            document["sessions"] = toml_edit::Item::Table(toml_edit::Table::new());
        }
        let table = document["sessions"]
            .as_table_mut()
            .ok_or_else(|| anyhow::anyhow!("sessions must be a TOML table"))?;
        patch_optional_toml_integer(table, "retention_days", settings.days);
        patch_optional_toml_integer(
            table,
            "retention_count",
            settings.count.map(|count| count as u64),
        );
        self.atomic_write(&document.to_string())
    }

    pub fn update_keymap_binding(
        &self,
        action: crate::keymap::KeyAction,
        chord: Option<&crate::keymap::KeyChord>,
    ) -> anyhow::Result<()> {
        let raw = self.raw_text()?;
        let mut document = raw.parse::<toml_edit::DocumentMut>()?;
        if !document.contains_key("keymap") {
            document["keymap"] = toml_edit::Item::Table(toml_edit::Table::new());
        }
        let table = document["keymap"]
            .as_table_mut()
            .ok_or_else(|| anyhow::anyhow!("keymap must be a TOML table"))?;
        match chord {
            Some(chord) => {
                table[action.config_key()] = toml_edit::value(chord.canonical());
            }
            None => {
                table.remove(action.config_key());
            }
        }
        let updated = document.to_string();
        let parsed: CaConfig = toml::from_str(&updated)?;
        crate::keymap::Keymap::from_config(&parsed.keymap)?;
        self.atomic_write(&updated)
    }

    pub fn upsert_provider_model_with_secret(
        &self,
        update: &ProviderModelUpdate,
        secret: Option<&crate::provider_secrets::SecretValue>,
    ) -> anyhow::Result<()> {
        validate_provider_model_update(update)?;
        validate_secret_destination(&update.api_key, secret)?;
        let existed = self.path.is_file();
        let raw = self.raw_text()?;
        let updated = provider_model_update_document(
            &raw,
            update,
            &self.keychain_reference(&update.provider),
        )?;
        self.atomic_write(&updated)?;

        if let (ProviderApiKeyUpdate::Keychain, Some(secret)) = (&update.api_key, secret) {
            let stored = secret.with_secret(|value| {
                self.provider_secrets
                    .store(&self.secret_account(&update.provider), value)
            });
            if let Err(error) = stored {
                if let Err(rollback_error) = self.restore_config(&raw, existed) {
                    anyhow::bail!(
                        "could not save the API key: {error}; config restore also failed: {rollback_error}"
                    );
                }
                anyhow::bail!("could not save the API key: {error}");
            }
        }
        Ok(())
    }

    pub fn update_provider_with_secret(
        &self,
        update: &ProviderSettingsUpdate,
        secret: Option<&crate::provider_secrets::SecretValue>,
    ) -> anyhow::Result<()> {
        validate_provider_settings_update(update)?;
        validate_secret_destination(&update.api_key, secret)?;
        let existed = self.path.is_file();
        let raw = self.raw_text()?;
        let updated = provider_settings_update_document(
            &raw,
            update,
            &self.keychain_reference(&update.provider),
        )?;
        self.atomic_write(&updated)?;

        if let (ProviderApiKeyUpdate::Keychain, Some(secret)) = (&update.api_key, secret) {
            let stored = secret.with_secret(|value| {
                self.provider_secrets
                    .store(&self.secret_account(&update.provider), value)
            });
            if let Err(error) = stored {
                if let Err(rollback_error) = self.restore_config(&raw, existed) {
                    anyhow::bail!(
                        "could not save the API key: {error}; config restore also failed: {rollback_error}"
                    );
                }
                anyhow::bail!("could not save the API key: {error}");
            }
        }
        Ok(())
    }

    pub fn delete_provider(&self, update: &ProviderDeleteUpdate) -> anyhow::Result<()> {
        validate_provider_id(&update.provider)?;
        let existed = self.path.is_file();
        let raw = self.raw_text()?;
        let raw_config: CaConfig = toml::from_str(&raw)?;
        let provider = raw_config.providers.get(&update.provider).ok_or_else(|| {
            anyhow::anyhow!("provider {} is no longer configured", update.provider)
        })?;
        let removes_keychain = self.keychain_reference_provider(provider.api_key.trim())
            == Some(update.provider.as_str());
        let updated = provider_delete_document(&raw, update)?;
        self.atomic_write(&updated)?;

        if removes_keychain {
            if let Err(error) = self
                .provider_secrets
                .delete(&self.secret_account(&update.provider))
            {
                if let Err(rollback_error) = self.restore_config(&raw, existed) {
                    anyhow::bail!(
                        "could not delete the saved API key: {error}; config restore also failed: {rollback_error}"
                    );
                }
                anyhow::bail!("could not delete the saved API key: {error}");
            }
        }
        Ok(())
    }

    pub fn writes_enabled(&self) -> bool {
        self.writes_enabled
    }

    pub fn stage_authenticated_provider(
        &self,
        provider: &str,
        config: &ProviderConfig,
        starter_model: &str,
        secret: Option<&crate::provider_secrets::SecretValue>,
    ) -> anyhow::Result<ProviderCredentialRollback> {
        self.stage_authenticated_provider_with_billing(
            provider,
            config,
            starter_model,
            secret,
            None,
        )
    }

    pub fn stage_authenticated_provider_with_billing(
        &self,
        provider: &str,
        config: &ProviderConfig,
        starter_model: &str,
        secret: Option<&crate::provider_secrets::SecretValue>,
        billing: Option<&crate::pricing::ProviderBillingConfig>,
    ) -> anyhow::Result<ProviderCredentialRollback> {
        validate_provider_id(provider)?;
        validate_model_id(starter_model)?;
        validate_base_url(&config.base_url, true)?;
        if secret.is_some_and(crate::provider_secrets::SecretValue::is_empty) {
            anyhow::bail!("credential is required");
        }
        let existed = self.path.is_file();
        let raw = self.raw_text()?;
        let parsed: CaConfig = toml::from_str(&raw)?;
        let previous_secret = if parsed.providers.get(provider).is_some_and(|previous| {
            self.keychain_reference_provider(previous.api_key.trim()) == Some(provider)
        }) {
            self.provider_secrets.load(&self.secret_account(provider))?
        } else {
            None
        };
        let mut document = raw.parse::<toml_edit::DocumentMut>()?;
        if !document.contains_key("providers") {
            document["providers"] = toml_edit::Item::Table(toml_edit::Table::new());
        }
        let providers = document["providers"]
            .as_table_mut()
            .ok_or_else(|| anyhow::anyhow!("providers must be a TOML table"))?;
        if !providers.contains_key(provider) {
            providers.insert(provider, toml_edit::Item::Table(toml_edit::Table::new()));
        }
        let table = providers
            .get_mut(provider)
            .and_then(toml_edit::Item::as_table_mut)
            .ok_or_else(|| anyhow::anyhow!("provider {provider} must be a TOML table"))?;
        table["base_url"] = toml_edit::value(config.base_url.trim());
        table["api_key"] = toml_edit::value(if secret.is_some() {
            self.keychain_reference(provider)
        } else {
            String::new()
        });
        table["protocol"] = toml_edit::value(config.protocol.label());
        let mut models = config.models.clone();
        if !models.iter().any(|model| model == starter_model) {
            models.push(starter_model.to_string());
        }
        let mut model_values = toml_edit::Array::new();
        for model in models {
            model_values.push(model);
        }
        table["models"] = toml_edit::value(model_values);
        let mut billing_config = parsed.provider_billing.clone();
        match billing {
            Some(billing) => {
                billing
                    .validate_for_provider(provider, &config.base_url)
                    .map_err(anyhow::Error::msg)?;
                billing_config.insert(provider.to_string(), billing.clone());
            }
            None => {
                billing_config.remove(provider);
            }
        }
        #[derive(Serialize)]
        struct BillingPatch<'a> {
            provider_billing: &'a HashMap<String, crate::pricing::ProviderBillingConfig>,
        }
        let mut replacement = toml_edit::ser::to_document(&BillingPatch {
            provider_billing: &billing_config,
        })?;
        if billing_config.is_empty() {
            document.remove("provider_billing");
        } else {
            document["provider_billing"] = replacement
                .remove("provider_billing")
                .ok_or_else(|| anyhow::anyhow!("failed to serialize provider billing"))?;
        }
        self.atomic_write(&document.to_string())?;
        if let Some(secret) = secret {
            let result = secret.with_secret(|bytes| {
                self.provider_secrets
                    .store(&self.secret_account(provider), bytes)
            });
            if let Err(error) = result {
                let _ = self.restore_config(&raw, existed);
                anyhow::bail!("could not save the credential: {error}");
            }
        }
        Ok(ProviderCredentialRollback {
            raw,
            existed,
            provider: provider.to_string(),
            previous_secret,
            credential_changed: secret.is_some(),
        })
    }

    pub fn rollback_authenticated_provider(
        &self,
        mut rollback: ProviderCredentialRollback,
    ) -> anyhow::Result<()> {
        self.restore_config(&rollback.raw, rollback.existed)?;
        if rollback.credential_changed {
            match rollback.previous_secret.as_deref() {
                Some(secret) => self
                    .provider_secrets
                    .store(&self.secret_account(&rollback.provider), secret)?,
                None => self
                    .provider_secrets
                    .delete(&self.secret_account(&rollback.provider))?,
            }
        }
        if let Some(secret) = rollback.previous_secret.as_mut() {
            secret.fill(0);
        }
        Ok(())
    }

    fn atomic_write(&self, content: &str) -> anyhow::Result<()> {
        if !self.writes_enabled {
            anyhow::bail!(
                "Persistent configuration changes are unavailable in bare mode; rerun without --profile bare"
            );
        }
        let parent = self
            .path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("config path has no parent"))?;
        std::fs::create_dir_all(parent)?;
        let temp = parent.join(format!(".config.toml.{}.tmp", Uuid::new_v4()));
        let result = (|| -> anyhow::Result<()> {
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
                let mode = std::fs::metadata(&self.path)
                    .map(|metadata| metadata.permissions().mode())
                    .unwrap_or(0o600);
                options.mode(mode);
            }
            let mut file = options.open(&temp)?;
            file.write_all(content.as_bytes())?;
            file.sync_all()?;
            std::fs::rename(&temp, &self.path)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temp);
        }
        result
    }

    fn raw_text(&self) -> anyhow::Result<String> {
        if self.path.is_file() {
            Ok(std::fs::read_to_string(&self.path)?)
        } else {
            Ok(toml::to_string_pretty(&CaConfig::default_config())?)
        }
    }

    fn restore_config(&self, raw: &str, existed: bool) -> anyhow::Result<()> {
        if existed {
            self.atomic_write(raw)
        } else {
            match std::fs::remove_file(&self.path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(error.into()),
            }
        }
    }

    fn secret_account(&self, provider: &str) -> String {
        self.profile.secret_namespace().map_or_else(
            || provider.to_string(),
            |profile| format!("profile/{profile}/provider/{provider}"),
        )
    }

    fn keychain_reference(&self, provider: &str) -> String {
        self.profile.secret_namespace().map_or_else(
            || crate::provider_secrets::credential_reference(provider),
            |profile| crate::provider_secrets::profile_credential_reference(profile, provider),
        )
    }

    fn keychain_reference_provider<'a>(&self, value: &'a str) -> Option<&'a str> {
        match &self.profile {
            crate::runtime::ActiveProfile::Named(expected) => {
                crate::provider_secrets::profile_credential_reference_parts(value)
                    .or_else(|| crate::provider_secrets::profile_keychain_reference_parts(value))
                    .and_then(|(profile, provider)| (profile == expected).then_some(provider))
                    .or_else(|| crate::provider_secrets::credential_reference_provider(value))
                    .or_else(|| crate::provider_secrets::keychain_reference_provider(value))
            }
            crate::runtime::ActiveProfile::Default | crate::runtime::ActiveProfile::Bare => {
                crate::provider_secrets::credential_reference_provider(value)
                    .or_else(|| crate::provider_secrets::keychain_reference_provider(value))
            }
        }
    }

    fn credential_account_for_reference(&self, value: &str, provider: &str) -> String {
        if crate::provider_secrets::credential_reference_provider(value) == Some(provider)
            || crate::provider_secrets::keychain_reference_provider(value) == Some(provider)
        {
            provider.to_string()
        } else {
            self.secret_account(provider)
        }
    }

    fn provider_api_key_source_value(&self, provider: &str, value: &str) -> ProviderApiKeySource {
        if value.is_empty() {
            ProviderApiKeySource::None
        } else if let Some(environment) = exact_environment_reference(value) {
            ProviderApiKeySource::Environment(environment.to_string())
        } else if self.keychain_reference_provider(value) == Some(provider) {
            ProviderApiKeySource::Keychain
        } else {
            ProviderApiKeySource::Hidden
        }
    }

    fn provider_credential_available(&self, provider: &str, value: &str) -> bool {
        if provider == "openai-codex" || value.is_empty() {
            return false;
        }
        if let Some(environment) = exact_environment_reference(value) {
            return crate::run_environment::var(environment).is_some_and(|value| !value.is_empty())
                || (provider.eq_ignore_ascii_case("mimo")
                    && environment == "XIAOMI_API_KEY"
                    && crate::run_environment::var("MIMO_API_KEY")
                        .is_some_and(|value| !value.is_empty()));
        }
        if self.keychain_reference_provider(value) == Some(provider) {
            return match self.provider_secrets.load(&self.secret_account(provider)) {
                Ok(Some(mut secret)) => {
                    let available = !secret.is_empty();
                    secret.fill(0);
                    available
                }
                Ok(None) | Err(_) => false,
            };
        }
        !crate::provider_secrets::resembles_saved_reference(value)
    }

    fn provider_credential_declared(&self, provider: &str, value: &str) -> bool {
        if provider == "openai-codex" || value.is_empty() {
            return false;
        }
        if let Some(environment) = exact_environment_reference(value) {
            return crate::run_environment::var(environment).is_some_and(|value| !value.is_empty())
                || (provider.eq_ignore_ascii_case("mimo")
                    && environment == "XIAOMI_API_KEY"
                    && crate::run_environment::var("MIMO_API_KEY")
                        .is_some_and(|value| !value.is_empty()));
        }
        self.keychain_reference_provider(value) == Some(provider)
            || !crate::provider_secrets::resembles_saved_reference(value)
    }
}

fn replace_permission_rules(
    document: &mut toml_edit::DocumentMut,
    rules: &[crate::permission::PermissionRule],
) -> anyhow::Result<()> {
    #[derive(Serialize)]
    struct Rules<'a> {
        permission_rules: &'a [crate::permission::PermissionRule],
    }
    let mut replacement = toml_edit::ser::to_document(&Rules {
        permission_rules: rules,
    })?;
    document["permission_rules"] = replacement
        .remove("permission_rules")
        .ok_or_else(|| anyhow::anyhow!("failed to serialize permission rules"))?;
    Ok(())
}

fn validate_secret_destination(
    api_key: &ProviderApiKeyUpdate,
    secret: Option<&crate::provider_secrets::SecretValue>,
) -> anyhow::Result<()> {
    match (api_key, secret) {
        (ProviderApiKeyUpdate::Keychain, Some(secret)) if !secret.is_empty() => Ok(()),
        (ProviderApiKeyUpdate::Keychain, _) => anyhow::bail!("API key is required"),
        (_, Some(_)) => anyhow::bail!("direct API key has no Keychain destination"),
        _ => Ok(()),
    }
}

fn patch_optional_toml_integer(table: &mut toml_edit::Table, key: &str, value: Option<u64>) {
    match value {
        Some(value) => {
            if let Some(current) = table.get_mut(key).and_then(toml_edit::Item::as_value_mut) {
                let decoration = current.decor().clone();
                *current = toml_edit::Value::from(value as i64);
                *current.decor_mut() = decoration;
            } else {
                table[key] = toml_edit::value(value as i64);
            }
        }
        None => {
            table.remove(key);
        }
    }
}

fn patch_optional_toml_string(table: &mut toml_edit::Table, key: &str, value: Option<&str>) {
    match value {
        Some(value) => patch_toml_string(table, key, value),
        None => {
            table.remove(key);
        }
    }
}

fn patch_toml_string(table: &mut toml_edit::Table, key: &str, value: &str) {
    if let Some(current) = table.get_mut(key).and_then(toml_edit::Item::as_value_mut) {
        let decoration = current.decor().clone();
        *current = toml_edit::Value::from(value);
        *current.decor_mut() = decoration;
    } else {
        table[key] = toml_edit::value(value);
    }
}

pub(crate) fn safe_provider_base_url(value: &str) -> String {
    let Ok(mut url) = reqwest::Url::parse(value.trim()) else {
        return crate::terminal_text::sanitize_terminal_text(value);
    };
    if !url.username().is_empty() || url.password().is_some() {
        let _ = url.set_username("");
        let _ = url.set_password(None);
    }
    url.set_query(None);
    url.set_fragment(None);
    crate::terminal_text::sanitize_terminal_text(url.as_str())
}

fn write_google_settings(table: &mut toml_edit::Table, google: &crate::google_auth::Config) {
    let mut settings = toml_edit::Table::new();
    settings["mode"] = toml_edit::value(match google.mode {
        crate::google_auth::Mode::AiStudio => "ai_studio",
        crate::google_auth::Mode::Vertex => "vertex",
    });
    for (key, value) in [
        ("project_id", &google.project_id),
        ("location", &google.location),
        ("quota_project_id", &google.quota_project_id),
    ] {
        if let Some(value) = value {
            settings[key] = toml_edit::value(value);
        }
    }
    table["google"] = toml_edit::Item::Table(settings);
}

fn provider_settings_update_document(
    raw: &str,
    update: &ProviderSettingsUpdate,
    keychain_reference: &str,
) -> anyhow::Result<String> {
    let mut document = raw.parse::<toml_edit::DocumentMut>()?;
    let config: CaConfig = toml::from_str(raw)?;
    if !config.providers.contains_key(&update.provider) {
        anyhow::bail!("provider {} is no longer configured", update.provider);
    }
    let table = document["providers"]
        .get_mut(&update.provider)
        .and_then(toml_edit::Item::as_table_mut)
        .ok_or_else(|| anyhow::anyhow!("provider {} must be a TOML table", update.provider))?;
    if let Some(google) = &update.google {
        if config.providers[&update.provider].protocol
            != crate::agent::ProviderProtocol::GoogleGenerateContent
        {
            anyhow::bail!("Google settings require google_generate_content");
        }
        write_google_settings(table, google);
    }
    table["base_url"] = toml_edit::value(update.base_url.trim());
    match &update.api_key {
        ProviderApiKeyUpdate::Preserve => {}
        ProviderApiKeyUpdate::Environment(environment) => {
            table["api_key"] = toml_edit::value(format!("${{{environment}}}"));
        }
        ProviderApiKeyUpdate::Keychain => {
            table["api_key"] = toml_edit::value(keychain_reference);
        }
    }
    if update.prompt_cache_mode.is_default() {
        table.remove("prompt_cache_mode");
    } else {
        table["prompt_cache_mode"] = toml_edit::value(update.prompt_cache_mode.label());
    }
    let updated = document.to_string();
    let parsed: CaConfig = toml::from_str(&updated)?;
    validate_model_prompt_cache(&parsed)?;
    Ok(updated)
}

fn provider_delete_document(raw: &str, update: &ProviderDeleteUpdate) -> anyhow::Result<String> {
    let mut document = raw.parse::<toml_edit::DocumentMut>()?;
    let config: CaConfig = toml::from_str(raw)?;
    if !config.providers.contains_key(&update.provider) {
        anyhow::bail!("provider {} is no longer configured", update.provider);
    }

    if let Some(replacement) = &update.replacement {
        if replacement.provider == update.provider {
            anyhow::bail!("replacement model must use a different provider");
        }
        validate_provider_id(&replacement.provider)?;
        validate_model_id(&replacement.model)?;
        if !config.providers.contains_key(&replacement.provider) {
            anyhow::bail!(
                "replacement provider {} is no longer configured",
                replacement.provider
            );
        }
        let replacement_table = document["providers"]
            .get_mut(&replacement.provider)
            .and_then(toml_edit::Item::as_table_mut)
            .ok_or_else(|| {
                anyhow::anyhow!("provider {} must be a TOML table", replacement.provider)
            })?;
        if !replacement_table.contains_key("models") {
            replacement_table["models"] = toml_edit::value(toml_edit::Array::new());
        }
        let models = replacement_table["models"].as_array_mut().ok_or_else(|| {
            anyhow::anyhow!("provider {} models must be an array", replacement.provider)
        })?;
        if !models
            .iter()
            .any(|model| model.as_str() == Some(replacement.model.as_str()))
        {
            models.push(replacement.model.as_str());
        }
        document["defaults"]["provider"] = toml_edit::value(&replacement.provider);
        document["defaults"]["model"] = toml_edit::value(&replacement.model);
    } else if config.defaults.provider == update.provider {
        anyhow::bail!("another provider model is required before deleting the default provider");
    }

    let providers = document["providers"]
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("providers must be a TOML table"))?;
    providers.remove(&update.provider);

    remove_provider_scoped_keys(&mut document, "model_profiles", &update.provider)?;
    remove_provider_scoped_keys(&mut document, "model_context", &update.provider)?;
    remove_provider_scoped_keys(&mut document, "model_prompt_cache", &update.provider)?;
    remove_provider_fallback_rules(&mut document, &update.provider)?;
    if let Some(provider_billing) = document
        .get_mut("provider_billing")
        .and_then(toml_edit::Item::as_table_mut)
    {
        provider_billing.remove(&update.provider);
    }
    if config
        .delegation
        .route
        .as_ref()
        .is_some_and(|route| route.provider == update.provider)
    {
        document.remove("delegation");
    }

    let deleting_default_provider = config.defaults.provider == update.provider;
    if let Some(review) = document
        .get_mut("review")
        .and_then(toml_edit::Item::as_table_mut)
    {
        if config.review.provider.as_deref() == Some(update.provider.as_str()) {
            review.remove("provider");
            review.remove("model");
        } else if deleting_default_provider && config.review.provider.is_none() {
            review.remove("model");
        }
    }
    let updated = document.to_string();
    let parsed: CaConfig = toml::from_str(&updated)?;
    validate_model_prompt_cache(&parsed)?;
    Ok(updated)
}

fn remove_provider_scoped_keys(
    document: &mut toml_edit::DocumentMut,
    table_name: &str,
    provider: &str,
) -> anyhow::Result<()> {
    let Some(table) = document
        .get_mut(table_name)
        .and_then(toml_edit::Item::as_table_mut)
    else {
        return Ok(());
    };
    let prefix = format!("{provider}/");
    let keys = table
        .iter()
        .filter(|(key, _)| key.starts_with(&prefix))
        .map(|(key, _)| key.to_string())
        .collect::<Vec<_>>();
    for key in keys {
        table.remove(&key);
    }
    Ok(())
}

fn remove_provider_fallback_rules(
    document: &mut toml_edit::DocumentMut,
    provider: &str,
) -> anyhow::Result<()> {
    let Some(rules) = document
        .get_mut("fallbacks")
        .and_then(toml_edit::Item::as_array_of_tables_mut)
    else {
        return Ok(());
    };
    rules.retain(|rule| {
        rule.get("provider").and_then(toml_edit::Item::as_str) != Some(provider)
            && rule
                .get("fallback_provider")
                .and_then(toml_edit::Item::as_str)
                != Some(provider)
    });
    if rules.is_empty() {
        document.remove("fallbacks");
    }
    Ok(())
}

fn rename_provider_fallback_model(
    document: &mut toml_edit::DocumentMut,
    provider: &str,
    original: &str,
    replacement: &str,
) -> anyhow::Result<()> {
    let Some(rules) = document
        .get_mut("fallbacks")
        .and_then(toml_edit::Item::as_array_of_tables_mut)
    else {
        return Ok(());
    };
    for rule in rules.iter_mut() {
        if rule.get("provider").and_then(toml_edit::Item::as_str) == Some(provider)
            && rule.get("model").and_then(toml_edit::Item::as_str) == Some(original)
        {
            rule["model"] = toml_edit::value(replacement);
        }
        if rule
            .get("fallback_provider")
            .and_then(toml_edit::Item::as_str)
            == Some(provider)
            && rule.get("fallback_model").and_then(toml_edit::Item::as_str) == Some(original)
        {
            rule["fallback_model"] = toml_edit::value(replacement);
        }
    }
    Ok(())
}

fn provider_model_update_document(
    raw: &str,
    update: &ProviderModelUpdate,
    keychain_reference: &str,
) -> anyhow::Result<String> {
    let mut document = raw.parse::<toml_edit::DocumentMut>()?;
    let config: CaConfig = toml::from_str(raw)?;
    let existing = config.providers.get(&update.provider).cloned();

    if update.require_new_provider && existing.is_some() {
        anyhow::bail!(
            "provider {} already exists; select it to edit",
            update.provider
        );
    }

    let mut provider = existing.unwrap_or_else(|| ProviderConfig {
        google: None,
        tool_schema_policy: Default::default(),
        base_url: String::new(),
        api_key: String::new(),
        protocol: crate::agent::ProviderProtocol::default(),
        prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
        models: Vec::new(),
        timeout: ProviderTimeoutConfig::default(),
    });
    if !update.base_url.trim().is_empty() {
        provider.base_url = update.base_url.trim().to_string();
    }
    if provider.base_url.is_empty() {
        anyhow::bail!("base URL is required for a new provider");
    }
    match &update.api_key {
        ProviderApiKeyUpdate::Preserve => {}
        ProviderApiKeyUpdate::Environment(environment) => {
            provider.api_key = format!("${{{environment}}}");
        }
        ProviderApiKeyUpdate::Keychain => {
            provider.api_key = keychain_reference.to_string();
        }
    }

    match update.original_model.as_deref() {
        Some(original) => {
            let Some(index) = provider.models.iter().position(|model| model == original) else {
                anyhow::bail!("model {original} is no longer configured");
            };
            if original != update.model
                && provider.models.iter().any(|model| model == &update.model)
            {
                anyhow::bail!("model {} already exists", update.model);
            }
            provider.models[index] = update.model.clone();
        }
        None => {
            if provider.models.iter().any(|model| model == &update.model) {
                anyhow::bail!("model {} already exists", update.model);
            }
            provider.models.push(update.model.clone());
        }
    }

    if !document.contains_key("providers") {
        document["providers"] = toml_edit::Item::Table(toml_edit::Table::new());
    }
    let providers = document["providers"]
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("providers must be a TOML table"))?;
    if !providers.contains_key(&update.provider) {
        providers.insert(
            &update.provider,
            toml_edit::Item::Table(toml_edit::Table::new()),
        );
    }
    let table = providers
        .get_mut(&update.provider)
        .and_then(toml_edit::Item::as_table_mut)
        .ok_or_else(|| anyhow::anyhow!("provider {} must be a TOML table", update.provider))?;
    table["base_url"] = toml_edit::value(&provider.base_url);
    table["api_key"] = toml_edit::value(&provider.api_key);
    if let Some(google) = &update.google {
        if update.protocol_override.unwrap_or(provider.protocol)
            != crate::agent::ProviderProtocol::GoogleGenerateContent
        {
            anyhow::bail!("Google settings require google_generate_content");
        }
        write_google_settings(table, google);
    }
    if let Some(protocol) = update.protocol_override {
        table["protocol"] = toml_edit::value(protocol.label());
    }
    let mut models = toml_edit::Array::new();
    for model in &provider.models {
        models.push(model.as_str());
    }
    table["models"] = toml_edit::value(models);

    if let Some(original) = update.original_model.as_deref() {
        if original != update.model {
            rename_exact_model_key(
                &mut document,
                "model_profiles",
                &format!("{}/{}", update.provider, original),
                &format!("{}/{}", update.provider, update.model),
            )?;
            rename_exact_model_key(
                &mut document,
                "model_context",
                &format!("{}/{}", update.provider, original),
                &format!("{}/{}", update.provider, update.model),
            )?;
            rename_exact_model_key(
                &mut document,
                "model_prompt_cache",
                &format!("{}/{}", update.provider, original),
                &format!("{}/{}", update.provider, update.model),
            )?;
            rename_provider_fallback_model(
                &mut document,
                &update.provider,
                original,
                &update.model,
            )?;
            if config
                .delegation
                .route
                .as_ref()
                .is_some_and(|route| route.provider == update.provider && route.model == original)
            {
                let route = document["delegation"]["route"]
                    .as_table_mut()
                    .ok_or_else(|| anyhow::anyhow!("delegation.route must be a TOML table"))?;
                patch_toml_string(route, "model", &update.model);
            }
        }
    }
    if update.select_saved_model {
        document["defaults"]["provider"] = toml_edit::value(&update.provider);
        document["defaults"]["model"] = toml_edit::value(&update.model);
    }
    let updated = document.to_string();
    let parsed: CaConfig = toml::from_str(&updated)?;
    validate_model_prompt_cache(&parsed)?;
    Ok(updated)
}

impl ConfigStore {
    pub fn trust_project_mcp(&self, trust: &ProjectMcpTrust) -> anyhow::Result<()> {
        let raw = self.raw_text()?;
        let mut document = raw.parse::<toml_edit::DocumentMut>()?;
        let mut config: CaConfig = toml::from_str(&raw)?;
        config
            .mcp
            .trusted_projects
            .retain(|current| current.workspace != trust.workspace);
        config.mcp.trusted_projects.push(trust.clone());
        #[derive(Serialize)]
        struct McpPatch<'a> {
            trusted_projects: &'a [ProjectMcpTrust],
        }
        let mut replacement = toml_edit::ser::to_document(&McpPatch {
            trusted_projects: &config.mcp.trusted_projects,
        })?;
        document["mcp"]["trusted_projects"] = replacement
            .remove("trusted_projects")
            .ok_or_else(|| anyhow::anyhow!("failed to serialize MCP project trust"))?;
        self.atomic_write(&document.to_string())
    }

    pub fn trust_workspace(&self, workspace_path: &str) -> anyhow::Result<()> {
        let raw = self.raw_text()?;
        let mut document = raw.parse::<toml_edit::DocumentMut>()?;
        let mut config: CaConfig = toml::from_str(&raw)?;
        if !config
            .trusted_workspaces
            .contains(&workspace_path.to_string())
        {
            config.trusted_workspaces.push(workspace_path.to_string());
        }
        let mut array = toml_edit::Array::new();
        for ws in &config.trusted_workspaces {
            array.push(ws.as_str());
        }
        document["trusted_workspaces"] = toml_edit::value(array);
        self.atomic_write(&document.to_string())
    }
}

fn upgrade_discontinued_builtin_models(config: &mut CaConfig) {
    const LEGACY_DEEPSEEK_MODELS: [&str; 2] = ["deepseek-chat", "deepseek-reasoner"];
    const CURRENT_DEEPSEEK_MODELS: [&str; 2] = ["deepseek-v4-flash", "deepseek-v4-pro"];

    let defaults = &mut config.defaults;
    for (name, provider) in &mut config.providers {
        let official_deepseek = reqwest::Url::parse(provider.base_url.trim())
            .ok()
            .and_then(|url| url.host_str().map(str::to_owned))
            .as_deref()
            == Some("api.deepseek.com");
        if !official_deepseek {
            continue;
        }
        provider
            .models
            .retain(|model| !LEGACY_DEEPSEEK_MODELS.contains(&model.as_str()));
        for model in CURRENT_DEEPSEEK_MODELS {
            if !provider.models.iter().any(|configured| configured == model) {
                provider.models.push(model.into());
            }
        }
        if defaults.provider == *name && LEGACY_DEEPSEEK_MODELS.contains(&defaults.model.as_str()) {
            defaults.model = "deepseek-v4-flash".into();
        }
    }
}

fn validate_provider_model_update(update: &ProviderModelUpdate) -> anyhow::Result<()> {
    validate_provider_id(&update.provider)?;
    validate_model_id(&update.model)?;
    validate_base_url(&update.base_url, false)?;
    validate_api_key_update(&update.api_key)?;
    if let Some(google) = &update.google {
        google.validate().map_err(anyhow::Error::msg)?;
        if update.protocol_override.is_some_and(|protocol| {
            protocol != crate::agent::ProviderProtocol::GoogleGenerateContent
        }) || (update.require_new_provider
            && update.protocol_override
                != Some(crate::agent::ProviderProtocol::GoogleGenerateContent))
        {
            anyhow::bail!(
                "Google settings require google_generate_content while adding a provider"
            );
        }
    }
    if update.protocol_override.is_some() && !update.require_new_provider {
        anyhow::bail!("provider protocol can only be set while adding a new provider");
    }
    Ok(())
}

fn validate_provider_settings_update(update: &ProviderSettingsUpdate) -> anyhow::Result<()> {
    validate_provider_id(&update.provider)?;
    validate_base_url(&update.base_url, true)?;
    if let Some(google) = &update.google {
        google.validate().map_err(anyhow::Error::msg)?;
    }
    validate_api_key_update(&update.api_key)
}

fn validate_model_prompt_cache(config: &CaConfig) -> anyhow::Result<()> {
    for (key, profile) in &config.model_profiles {
        if profile.tool_call_format.is_none() {
            continue;
        }
        let Some((provider_id, model)) = key.split_once('/') else {
            anyhow::bail!("model profile key must use exact provider/model syntax: {key}");
        };
        validate_provider_id(provider_id)?;
        validate_model_id(model)?;
        let provider = config.providers.get(provider_id).ok_or_else(|| {
            anyhow::anyhow!("model profile provider is not configured: {provider_id}")
        })?;
        validate_model_tool_call_format(profile, provider.protocol, key)?;
    }
    for (key, cache) in &config.model_prompt_cache {
        let Some((provider_id, model)) = key.split_once('/') else {
            anyhow::bail!("model prompt-cache key must use exact provider/model syntax: {key}");
        };
        validate_provider_id(provider_id)?;
        validate_model_id(model)?;
        let provider = config.providers.get(provider_id).ok_or_else(|| {
            anyhow::anyhow!("model prompt-cache provider is not configured: {provider_id}")
        })?;
        let first_party = crate::provider_contract::resolve(provider_id, provider, model);
        if first_party.is_some() && (cache.request_format.is_some() || cache.usage_format.is_some())
        {
            anyhow::bail!(
                "first-party model prompt-cache formats are built in; configure policy only: {key}"
            );
        }

        if first_party.is_none() {
            if matches!(
                cache.request_format,
                Some(PromptCacheRequestFormat::OpenAiResponsesExplicit)
            ) && provider.protocol != crate::agent::ProviderProtocol::OpenAiResponses
            {
                anyhow::bail!(
                    "openai_responses_explicit prompt caching requires the openai_responses protocol: {key}"
                );
            }
            match cache.usage_format {
                Some(PromptCacheUsageFormat::Auto) => {}
                Some(PromptCacheUsageFormat::OpenAiDetails)
                    if provider.protocol == crate::agent::ProviderProtocol::AnthropicMessages =>
                {
                    anyhow::bail!(
                        "openai_details cache usage is incompatible with anthropic_messages: {key}"
                    );
                }
                Some(PromptCacheUsageFormat::HitMiss)
                    if provider.protocol != crate::agent::ProviderProtocol::ChatCompletions =>
                {
                    anyhow::bail!(
                        "hit_miss cache usage requires the chat_completions protocol: {key}"
                    );
                }
                Some(PromptCacheUsageFormat::ReadWrite)
                    if provider.protocol != crate::agent::ProviderProtocol::AnthropicMessages =>
                {
                    anyhow::bail!(
                        "read_write cache usage requires the anthropic_messages protocol: {key}"
                    );
                }
                _ => {}
            }
            if cache.policy.is_some() && cache.request_format.is_none() {
                anyhow::bail!("custom model prompt-cache policy requires a request_format: {key}");
            }
        }

        let resolved =
            crate::provider_contract::resolve_prompt_cache(config, provider_id, provider, model);
        if let Some(policy) = resolved.policy() {
            let Some(request_format) = resolved.request_format() else {
                anyhow::bail!("model prompt-cache policy has no request format: {key}");
            };
            if !request_format.supports_policy(policy) {
                anyhow::bail!(
                    "prompt-cache policy {} is unsupported by request format {}: {key}",
                    policy.label(),
                    request_format.label()
                );
            }
        }
    }
    Ok(())
}

fn validate_model_tool_call_format(
    profile: &ModelProfile,
    protocol: crate::agent::ProviderProtocol,
    key: &str,
) -> anyhow::Result<()> {
    if profile.tool_call_format.is_none() {
        return Ok(());
    }
    if protocol != crate::agent::ProviderProtocol::ChatCompletions {
        anyhow::bail!("tool-call text formats require the chat_completions protocol: {key}");
    }
    if profile.supports_tools == Some(false) {
        anyhow::bail!("tool-call text formats require a tool-capable model: {key}");
    }
    Ok(())
}

fn validate_provider_id(provider: &str) -> anyhow::Result<()> {
    let provider = provider.trim();
    if provider.is_empty()
        || !provider
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "._-".contains(character))
    {
        anyhow::bail!("provider must use letters, numbers, '.', '_' or '-'");
    }
    Ok(())
}

fn validate_model_id(model: &str) -> anyhow::Result<()> {
    if model.trim().is_empty() || model.trim() != model {
        anyhow::bail!("model ID is required and cannot start or end with spaces");
    }
    if model.len() > 256 || model.chars().any(char::is_control) {
        anyhow::bail!("invalid model ID");
    }
    Ok(())
}

fn validate_pricing(config: &CaConfig) -> anyhow::Result<()> {
    for key in config.pricing.keys() {
        let Some((provider, model)) = key.split_once('/') else {
            anyhow::bail!("pricing key must use exact provider/model syntax: {key}");
        };
        validate_provider_id(provider)?;
        validate_model_id(model)?;
        if model
            .split('/')
            .any(|component| component == "." || component == "..")
        {
            anyhow::bail!("pricing model ID contains a traversal-like component: {key}");
        }
        if !config.providers.contains_key(provider) {
            anyhow::bail!("pricing provider is not configured: {provider}");
        }
    }
    Ok(())
}

fn validate_review_config(config: &CaConfig) -> anyhow::Result<()> {
    const MAX_REVIEW_OUTPUT_BYTES: usize = 1024 * 1024;
    const MAX_REVIEW_PROVIDER_REQUESTS: u32 = 48;
    let convergence = &config.review.convergence;
    if convergence.max_rounds == 0 || convergence.max_rounds > 3 {
        anyhow::bail!("review convergence max_rounds must be between 1 and 3");
    }
    if convergence.max_provider_requests < 2
        || convergence.max_provider_requests > MAX_REVIEW_PROVIDER_REQUESTS
    {
        anyhow::bail!(
            "review convergence max_provider_requests must be between 2 and {MAX_REVIEW_PROVIDER_REQUESTS}"
        );
    }
    if convergence.max_output_bytes == 0 || convergence.max_output_bytes > MAX_REVIEW_OUTPUT_BYTES {
        anyhow::bail!(
            "review convergence max_output_bytes must be between 1 and {MAX_REVIEW_OUTPUT_BYTES}"
        );
    }
    if convergence
        .max_cost_usd
        .is_some_and(|cost| cost == crate::usage::DecimalUsd::ZERO)
    {
        anyhow::bail!("review convergence max_cost_usd must be greater than zero");
    }
    if convergence.routes.is_empty() {
        return Ok(());
    }
    if convergence.routes.len() != 2 {
        anyhow::bail!("review convergence requires exactly two routes");
    }
    for route in &convergence.routes {
        validate_provider_id(&route.provider)?;
        validate_model_id(&route.model)?;
        let provider = config.providers.get(&route.provider).ok_or_else(|| {
            anyhow::anyhow!(
                "review convergence provider is not configured: {}",
                route.provider
            )
        })?;
        if !provider.models.iter().any(|model| model == &route.model) {
            anyhow::bail!(
                "review convergence model is not configured: {}/{}",
                route.provider,
                route.model
            );
        }
        if config
            .effective_model_profile(&route.provider, &route.model)
            .and_then(|profile| profile.supports_tools)
            == Some(false)
        {
            anyhow::bail!(
                "review convergence route does not support tools: {}/{}",
                route.provider,
                route.model
            );
        }
    }
    if convergence.routes[0] == convergence.routes[1] {
        anyhow::bail!("review convergence routes must be distinct provider/model pairs");
    }
    if convergence.max_cost_usd.is_none() {
        anyhow::bail!("review convergence requires an exact max_cost_usd ceiling");
    }
    Ok(())
}

fn validate_provider_billing(config: &CaConfig) -> anyhow::Result<()> {
    for (id, provider) in &config.providers {
        if provider.protocol == crate::agent::ProviderProtocol::GoogleGenerateContent {
            provider
                .google
                .clone()
                .unwrap_or_default()
                .validate()
                .map_err(|error| anyhow::anyhow!("provider {id}: {error}"))?;
            for model in &provider.models {
                crate::google_provider::model_path(model).map_err(anyhow::Error::msg)?;
            }
        } else if provider.google.is_some() {
            anyhow::bail!("provider {id}: google settings require google_generate_content");
        }
    }
    for (provider, billing) in &config.provider_billing {
        let configured = config
            .providers
            .get(provider)
            .ok_or_else(|| anyhow::anyhow!("billing provider is not configured: {provider}"))?;
        billing
            .validate_for_provider(provider, &configured.base_url)
            .map_err(anyhow::Error::msg)?;
    }
    Ok(())
}

fn validate_hooks(hooks: &[HookConfig]) -> anyhow::Result<()> {
    const MAX_HOOKS: usize = 128;
    const MAX_COMMAND_BYTES: usize = 16 * 1024;
    const MAX_TIMEOUT_SECS: u64 = 600;

    if hooks.len() > MAX_HOOKS {
        anyhow::bail!("at most {MAX_HOOKS} hooks may be configured");
    }
    for (index, hook) in hooks.iter().enumerate() {
        if crate::hooks::HookEvent::parse(&hook.event).is_none() {
            anyhow::bail!("hooks[{index}] has an unsupported event");
        }
        if hook.command.trim().is_empty()
            || hook.command.len() > MAX_COMMAND_BYTES
            || hook.command.chars().any(|character| character == '\0')
        {
            anyhow::bail!("hooks[{index}] has an invalid command");
        }
        if hook.timeout_secs == 0 || hook.timeout_secs > MAX_TIMEOUT_SECS {
            anyhow::bail!(
                "hooks[{index}] timeout must be between 1 and {MAX_TIMEOUT_SECS} seconds"
            );
        }
    }
    Ok(())
}

fn validate_base_url(base_url: &str, required: bool) -> anyhow::Result<()> {
    let base_url = base_url.trim();
    if required && base_url.is_empty() {
        anyhow::bail!("base URL is required");
    }
    if !base_url.is_empty()
        && (!(base_url.starts_with("https://") || base_url.starts_with("http://"))
            || base_url.chars().any(char::is_whitespace))
    {
        anyhow::bail!("base URL must start with http:// or https:// and contain no spaces");
    }
    if !base_url.is_empty() {
        let parsed = reqwest::Url::parse(base_url)
            .map_err(|_| anyhow::anyhow!("base URL must be a valid http:// or https:// URL"))?;
        if !parsed.username().is_empty() || parsed.password().is_some() {
            anyhow::bail!("base URL must not contain embedded credentials");
        }
        if parsed.query().is_some() || parsed.fragment().is_some() {
            anyhow::bail!("base URL must not contain a query or fragment");
        }
    }
    Ok(())
}

fn validate_api_key_update(api_key: &ProviderApiKeyUpdate) -> anyhow::Result<()> {
    if let ProviderApiKeyUpdate::Environment(environment) = api_key {
        let mut characters = environment.chars();
        let valid_first = characters
            .next()
            .is_some_and(|character| character == '_' || character.is_ascii_alphabetic());
        if !valid_first
            || !characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
        {
            anyhow::bail!("API key environment variable must be a valid name");
        }
    }
    Ok(())
}

fn exact_environment_reference(value: &str) -> Option<&str> {
    let environment = value.strip_prefix("${")?.strip_suffix('}')?;
    let mut characters = environment.chars();
    let valid_first = characters
        .next()
        .is_some_and(|character| character == '_' || character.is_ascii_alphabetic());
    (valid_first
        && characters.all(|character| character == '_' || character.is_ascii_alphanumeric()))
    .then_some(environment)
}

#[derive(Clone)]
pub(crate) struct LearningExecutionProfilePlan {
    pub(crate) contents: String,
    pub digest: String,
    pub providers: Vec<(String, Vec<String>)>,
}

#[derive(Serialize)]
struct LearningExecutionProfileDocument {
    providers: BTreeMap<String, ProviderConfig>,
    defaults: DefaultsConfig,
    agent: AgentConfig,
    model_context: BTreeMap<String, ModelContextConfig>,
    model_profiles: BTreeMap<String, ModelProfile>,
}

impl std::fmt::Debug for LearningExecutionProfilePlan {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LearningExecutionProfilePlan")
            .field("digest", &self.digest)
            .field("providers", &self.providers)
            .field("contents", &"[SECRET-FREE PROFILE METADATA]")
            .finish()
    }
}

pub(crate) fn plan_learning_execution_profile(
    source: &Path,
    target: &Path,
) -> anyhow::Result<LearningExecutionProfilePlan> {
    use sha2::{Digest as _, Sha256};

    let source = read_role_run_profile_config(source)?;
    let mut providers = source
        .providers
        .into_iter()
        .filter(|(_, provider)| {
            let credential = provider.api_key.trim();
            provider.models.iter().any(|model| !model.is_empty())
                && (credential.is_empty()
                    || exact_environment_reference(credential).is_some()
                    || crate::provider_secrets::resembles_saved_reference(credential))
        })
        .collect::<BTreeMap<_, _>>();
    providers.retain(|provider_id, provider| {
        provider_id == "openai-codex"
            || !provider.api_key.trim().is_empty()
            || provider.protocol == crate::agent::ProviderProtocol::OpenAiResponses
            || crate::google_auth::is_vertex(provider)
    });
    if providers.len() < 2 {
        anyhow::bail!(
            "learning execution requires at least two configured providers with external credential references"
        );
    }
    let mut routes = providers
        .iter()
        .map(|(id, provider)| (id.clone(), provider.models.clone()))
        .collect::<Vec<_>>();
    routes.sort_by(|left, right| left.0.cmp(&right.0));
    for (_, models) in &mut routes {
        models.sort();
        models.dedup();
    }
    let (default_provider, default_model) = routes
        .iter()
        .find_map(|(provider, models)| {
            models
                .first()
                .map(|model| (provider.clone(), model.clone()))
        })
        .ok_or_else(|| anyhow::anyhow!("learning execution has no configured model"))?;
    let planned = LearningExecutionProfileDocument {
        providers,
        defaults: DefaultsConfig {
            provider: default_provider,
            model: default_model,
        },
        agent: AgentConfig {
            approval_mode: "never".to_string(),
            sandbox_mode: "required".to_string(),
            ..AgentConfig::default()
        },
        model_context: source.model_context.into_iter().collect(),
        model_profiles: source.model_profiles.into_iter().collect(),
    };
    let contents = toml::to_string_pretty(&planned)?;
    let parsed: CaConfig = toml::from_str(&contents)?;
    role_run_profile_shape(&parsed, "ca-learning".to_string())?;
    let digest = format!("{:x}", Sha256::digest(contents.as_bytes()));
    if let Ok(existing) = std::fs::read(target) {
        if existing != contents.as_bytes() {
            anyhow::bail!("learning execution profile already exists with different content");
        }
    }
    Ok(LearningExecutionProfilePlan {
        contents,
        digest,
        providers: routes,
    })
}

#[cfg(test)]
pub(crate) fn install_learning_execution_profile(
    target: &Path,
    plan: &LearningExecutionProfilePlan,
) -> anyhow::Result<bool> {
    use std::io::Write as _;

    if let Ok(existing) = std::fs::read(target) {
        if existing == plan.contents.as_bytes() {
            return Ok(false);
        }
        anyhow::bail!("learning execution profile changed after preview");
    }
    let parent = target
        .parent()
        .ok_or_else(|| anyhow::anyhow!("learning execution profile has no parent"))?;
    std::fs::create_dir_all(parent)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    let mut file = options.open(target)?;
    file.write_all(plan.contents.as_bytes())?;
    file.sync_all()?;
    Ok(true)
}

fn rename_exact_model_key(
    document: &mut toml_edit::DocumentMut,
    section: &str,
    old: &str,
    new: &str,
) -> anyhow::Result<()> {
    let Some(table) = document
        .get_mut(section)
        .and_then(toml_edit::Item::as_table_mut)
    else {
        return Ok(());
    };
    if table.contains_key(new) {
        anyhow::bail!("{section} already contains {new}");
    }
    if let Some(item) = table.remove(old) {
        table.insert(new, item);
    }
    Ok(())
}

pub fn expand_env_vars(value: &str) -> String {
    expand_env_vars_with(value, &crate::run_environment::var)
}

fn expand_env_vars_with<F>(value: &str, resolver: &F) -> String
where
    F: Fn(&str) -> Option<String>,
{
    let mut result = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '$' && chars.peek() == Some(&'{') {
            chars.next(); // skip {
            let mut var = String::new();
            for c in chars.by_ref() {
                if c == '}' {
                    break;
                }
                var.push(c);
            }
            if let Some(val) = resolver(&var) {
                result.push_str(&val);
            } else {
                result.push_str(&format!("${{{}}}", var));
            }
        } else {
            result.push(c);
        }
    }
    result
}

#[cfg(test)]
mod config_store_tests {
    use super::*;
    use crate::provider_secrets::{ProviderSecretStore, SecretValue};
    use crate::runtime::RuntimePaths;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    fn assert_semantic_agent_defaults(agent: &AgentConfig) {
        assert!(agent.proactive_delegation);
        assert!(agent.review_changes);
        assert_eq!(agent.max_turns, 100);
        assert_eq!(agent.keep_awake, !cfg!(target_os = "freebsd"));
        assert_eq!(agent.approval_mode, "auto-review");
        assert_eq!(agent.command_timeout_secs, 30);
        assert_eq!(agent.sandbox_mode, "required");
        assert_eq!(agent.subagent_concurrency, 3);
        assert_eq!(agent.subagent_max_depth, 2);
        assert_eq!(agent.subagent_children_per_turn, 8);
        assert_eq!(agent.subagent_max_turns, 30);
        assert_eq!(agent.subagent_runtime_secs, 30 * 60);
        assert_eq!(agent.subagent_output_bytes, 256 * 1024);
        assert!(agent.modes.is_empty());
    }

    #[test]
    fn legacy_agent_defaults_delegate_and_review_but_explicit_choices_survive() {
        let legacy: AgentConfig = toml::from_str("").unwrap();
        assert!(legacy.proactive_delegation && legacy.review_changes);
        let explicit: AgentConfig =
            toml::from_str("proactive_delegation = false\nreview_changes = false\n").unwrap();
        assert!(!explicit.proactive_delegation && !explicit.review_changes);
    }

    #[derive(Default)]
    struct FakeProviderSecrets {
        values: Mutex<HashMap<String, Vec<u8>>>,
        fail_store: std::sync::atomic::AtomicBool,
        load_calls: std::sync::atomic::AtomicUsize,
    }

    impl ProviderSecretStore for FakeProviderSecrets {
        fn load(&self, provider: &str) -> anyhow::Result<Option<Vec<u8>>> {
            self.load_calls
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Ok(self.values.lock().unwrap().get(provider).cloned())
        }

        fn store(&self, provider: &str, secret: &[u8]) -> anyhow::Result<()> {
            if self.fail_store.load(std::sync::atomic::Ordering::Relaxed) {
                anyhow::bail!("fake Keychain refused the write");
            }
            self.values
                .lock()
                .unwrap()
                .insert(provider.to_string(), secret.to_vec());
            Ok(())
        }

        fn delete(&self, provider: &str) -> anyhow::Result<()> {
            if self.fail_store.load(std::sync::atomic::Ordering::Relaxed) {
                anyhow::bail!("fake Keychain refused the delete");
            }
            self.values.lock().unwrap().remove(provider);
            Ok(())
        }
    }

    fn role_run_profile_fixture() -> &'static str {
        r#"# role-run fixture must remain byte-for-byte stable
approval_rules = [{ AllowTool = "read_file" }]
permission_rules = [{ decision = "deny", kind = "tool", tool = "write_file" }]

[providers.openai]
base_url = "https://api.example.invalid/v1"
api_key = "${ROLE_RUN_SELECTED_SECRET}"
models = ["gpt-safe"]

[providers.other]
base_url = "https://other.example.invalid/v1"
api_key = "${ROLE_RUN_OTHER_SECRET}"
models = ["other-safe"]

[defaults]
provider = "openai"
model = "gpt-safe"

[agent]
approval_mode = "allowlist"
sandbox_mode = "required"

[model_profiles."openai/gpt-safe"]
context_window_tokens = 8192
compact_at_tokens = 6144
supports_tools = true
supports_images = false
reasoning_efforts = ["low", "high"]
default_reasoning_effort = "high"
reasoning_request_format = "reasoning_effort"

[mcp.servers.canary]
command = "/must-not-run"
env = { TOKEN = "${ROLE_RUN_MCP_SECRET}" }
"#
    }

    #[test]
    fn learning_execution_profile_is_secret_free_restricted_and_idempotent() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("config.toml");
        let target = temp.path().join("learning/config.toml");
        let raw = format!(
            "{}\n[providers.literal]\nbase_url = \"https://literal.example.invalid/v1\"\napi_key = \"literal-secret-canary\"\nmodels = [\"literal-model\"]\n",
            role_run_profile_fixture()
        );
        std::fs::write(&source, raw).unwrap();

        let plan = plan_learning_execution_profile(&source, &target).unwrap();
        let repeated = plan_learning_execution_profile(&source, &target).unwrap();
        assert_eq!(plan.digest, repeated.digest);
        assert_eq!(plan.contents, repeated.contents);
        assert_eq!(plan.providers.len(), 2);
        assert!(!plan.contents.contains("literal-secret-canary"));
        assert!(!plan.contents.contains("literal-model"));
        assert!(!plan.contents.contains("must-not-run"));
        assert!(plan.contents.contains("approval_mode = \"never\""));
        assert!(plan.contents.contains("sandbox_mode = \"required\""));

        assert!(install_learning_execution_profile(&target, &plan).unwrap());
        assert!(!install_learning_execution_profile(&target, &plan).unwrap());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), plan.contents);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(
                std::fs::metadata(&target).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn role_run_profile_names_are_strict_named_only() {
        for valid in ["work", "codex-team", "team_2"] {
            role_run_profile_name(valid).unwrap();
        }
        for invalid in [
            "",
            "default",
            "bare",
            "Work",
            "two words",
            "../escape",
            "-leading",
        ] {
            let error = role_run_profile_name(invalid).unwrap_err().to_string();
            assert!(error.contains("role-run"), "unexpected error: {error}");
        }
    }

    #[test]
    fn role_run_profile_shape_is_secret_free_offline_and_mutation_free() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("work.toml");
        let original = role_run_profile_fixture();
        std::fs::write(&path, original).unwrap();

        let shape = ConfigStore::load_role_run_profile_shape(&path, "codex-team").unwrap();

        assert_eq!(shape.profile, "codex-team");
        assert_eq!(shape.sandbox_mode, crate::sandbox::SandboxMode::Required);
        assert_eq!(shape.providers.len(), 2);
        assert_eq!(shape.providers[0].id, "openai");
        assert_eq!(shape.providers[0].models[0].id, "gpt-safe");
        let profile = &shape.providers[0].models[0].profile;
        assert_eq!(profile.supports_tools, Some(true));
        assert_eq!(profile.supports_images, Some(false));
        assert_eq!(
            profile.reasoning_efforts,
            [ReasoningEffort::Low, ReasoningEffort::High]
        );
        let rendered = format!("{shape:?}");
        for secret_or_endpoint in [
            "ROLE_RUN_SELECTED_SECRET",
            "ROLE_RUN_OTHER_SECRET",
            "api.example.invalid",
            "other.example.invalid",
            "allowlist",
        ] {
            assert!(!rendered.contains(secret_or_endpoint));
        }
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 1);
    }

    #[test]
    fn role_run_profile_shape_never_creates_missing_state_or_migrates_legacy_approval() {
        let temp = tempfile::tempdir().unwrap();
        let missing = temp.path().join("profiles/codex-team.toml");

        let error = ConfigStore::load_role_run_profile_shape(&missing, "codex-team")
            .unwrap_err()
            .to_string();

        assert!(error.contains("unavailable"));
        assert!(!missing.parent().unwrap().exists());

        let existing = temp.path().join("legacy.toml");
        let original = role_run_profile_fixture();
        std::fs::write(&existing, original).unwrap();
        ConfigStore::load_role_run_profile_shape(&existing, "codex-team").unwrap();
        assert_eq!(std::fs::read_to_string(existing).unwrap(), original);
    }

    #[test]
    fn role_run_profile_shape_strictly_validates_sandbox_routes_and_bounds() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("work.toml");

        for (replacement, expected) in [
            (
                "sandbox_mode = \"policy_only\"",
                "invalid saved sandbox mode",
            ),
            ("models = [\" gpt-safe\"]", "invalid model ID"),
        ] {
            let raw = if replacement.starts_with("sandbox") {
                role_run_profile_fixture().replace("sandbox_mode = \"required\"", replacement)
            } else {
                role_run_profile_fixture().replace("models = [\"gpt-safe\"]", replacement)
            };
            std::fs::write(&path, raw).unwrap();
            let error = ConfigStore::load_role_run_profile_shape(&path, "codex-team")
                .unwrap_err()
                .to_string();
            assert!(error.contains(expected), "unexpected error: {error}");
        }

        let oversized = temp.path().join("oversized.toml");
        let file = std::fs::File::create(&oversized).unwrap();
        file.set_len(ROLE_RUN_PROFILE_CONFIG_MAX_BYTES + 1).unwrap();
        let error = ConfigStore::load_role_run_profile_shape(&oversized, "codex-team")
            .unwrap_err()
            .to_string();
        assert!(error.contains("bounded regular file"));
    }

    #[test]
    fn role_run_rejects_invalid_legacy_denies_before_credential_resolution() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("work.toml");
        let secrets = Arc::new(FakeProviderSecrets::default());
        let store = ConfigStore::with_profile_secret_store(
            &path,
            crate::runtime::ActiveProfile::Named("work".into()),
            secrets.clone(),
        );
        let invalid_rules = [
            r#"approval_rules = [{ DenyTool = "deny-tool-secret-canary" }]"#,
            r#"approval_rules = [{ DenyPath = { tool = "read_file", path_pattern = "../deny-path-secret-canary/**" } }]"#,
            r#"approval_rules = [{ DenyPath = { tool = "", path_pattern = "/safe/**" } }]"#,
            r#"approval_rules = [{ DenyPath = { tool = "read_file", path_pattern = "/invalid/[" } }]"#,
        ];

        for invalid_rule in invalid_rules {
            let raw = role_run_profile_fixture().replace(
                r#"approval_rules = [{ AllowTool = "read_file" }]"#,
                invalid_rule,
            );
            std::fs::write(&path, raw).unwrap();

            let preflight_error =
                ConfigStore::load_role_run_profile_shape(&path, "work").unwrap_err();
            assert_eq!(
                preflight_error.to_string(),
                "role-run profile contains an invalid legacy deny rule"
            );
            let execution_error = store
                .load_role_run_selected_route_with_env(
                    "openai",
                    "gpt-safe",
                    Some(ReasoningEffort::High),
                    |_| panic!("invalid deny must fail before environment resolution"),
                )
                .unwrap_err();
            let diagnostic = execution_error.to_string();
            assert_eq!(
                diagnostic,
                "role-run profile contains an invalid legacy deny rule"
            );
            assert!(!diagnostic.contains("secret-canary"));
            assert_eq!(
                secrets
                    .load_calls
                    .load(std::sync::atomic::Ordering::Relaxed),
                0
            );
        }
    }

    #[test]
    fn role_run_accepts_and_retains_valid_legacy_denies() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("work.toml");
        let raw = role_run_profile_fixture().replace(
            r#"approval_rules = [{ AllowTool = "read_file" }]"#,
            r#"approval_rules = [
  { DenyTool = "write_file" },
  { DenyPath = { tool = "read_file", path_pattern = "/workspace/protected/**" } },
]"#,
        );
        std::fs::write(&path, raw).unwrap();
        let store = ConfigStore::with_profile_secret_store(
            &path,
            crate::runtime::ActiveProfile::Named("work".into()),
            Arc::new(FakeProviderSecrets::default()),
        );

        ConfigStore::load_role_run_profile_shape(&path, "work").unwrap();
        let route = store
            .load_role_run_selected_route_with_env(
                "openai",
                "gpt-safe",
                Some(ReasoningEffort::High),
                |name| (name == "ROLE_RUN_SELECTED_SECRET").then(|| "selected".into()),
            )
            .unwrap();

        assert_eq!(route.legacy_approval_rules.len(), 2);
        assert!(matches!(
            route.legacy_approval_rules[0],
            ApprovalRule::DenyTool(ref tool) if tool == "write_file"
        ));
        assert!(matches!(
            route.legacy_approval_rules[1],
            ApprovalRule::DenyPath { ref tool, ref path_pattern }
                if tool == "read_file" && path_pattern == "/workspace/protected/**"
        ));
    }

    #[cfg(unix)]
    #[test]
    fn role_run_profile_shape_rejects_symlink_files() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("target.toml");
        let link = temp.path().join("work.toml");
        std::fs::write(&target, role_run_profile_fixture()).unwrap();
        symlink(&target, &link).unwrap();

        let error = ConfigStore::load_role_run_profile_shape(&link, "codex-team")
            .unwrap_err()
            .to_string();
        assert!(error.contains("bounded regular file"));
    }

    #[test]
    fn role_run_execution_resolves_only_the_selected_route_without_writing() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("work.toml");
        let selected_reference =
            crate::provider_secrets::profile_credential_reference("work", "openai");
        let other_reference =
            crate::provider_secrets::profile_credential_reference("work", "other");
        let original = role_run_profile_fixture()
            .replace("${ROLE_RUN_SELECTED_SECRET}", &selected_reference)
            .replace("${ROLE_RUN_OTHER_SECRET}", &other_reference);
        std::fs::write(&path, &original).unwrap();
        let secrets = Arc::new(FakeProviderSecrets::default());
        secrets.values.lock().unwrap().insert(
            "profile/work/provider/openai".into(),
            b"selected-canary".to_vec(),
        );
        secrets.values.lock().unwrap().insert(
            "profile/work/provider/other".into(),
            b"other-canary".to_vec(),
        );
        let store = ConfigStore::with_profile_secret_store(
            &path,
            crate::runtime::ActiveProfile::Named("work".into()),
            secrets.clone(),
        );

        let route = store
            .load_role_run_selected_route_with_env(
                "openai",
                "gpt-safe",
                Some(ReasoningEffort::High),
                |_| panic!("selected keychain route must not consult environment values"),
            )
            .unwrap();

        assert_eq!(route.provider_id, "openai");
        assert_eq!(route.model_id, "gpt-safe");
        assert_eq!(route.provider.models, ["gpt-safe"]);
        assert_eq!(route.provider.api_key, "selected-canary");
        assert_eq!(route.reasoning_effort, Some(ReasoningEffort::High));
        assert_eq!(route.sandbox_mode, crate::sandbox::SandboxMode::Required);
        assert_eq!(route.permission_rules.len(), 1);
        assert_eq!(route.legacy_approval_rules.len(), 1);
        assert_eq!(
            secrets
                .load_calls
                .load(std::sync::atomic::Ordering::Relaxed),
            1
        );
        assert!(!format!("{route:?}").contains("selected-canary"));
        assert!(!format!("{route:?}").contains("other-canary"));
        assert_eq!(std::fs::read_to_string(path).unwrap(), original);
    }

    #[test]
    fn role_run_execution_resolves_only_selected_provider_environment_values() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("work.toml");
        let original = role_run_profile_fixture();
        std::fs::write(&path, original).unwrap();
        let store = ConfigStore::with_profile_secret_store(
            &path,
            crate::runtime::ActiveProfile::Named("work".into()),
            Arc::new(FakeProviderSecrets::default()),
        );
        let resolved_names = std::cell::RefCell::new(Vec::new());

        let route = store
            .load_role_run_selected_route_with_env(
                "openai",
                "gpt-safe",
                Some(ReasoningEffort::Low),
                |name| {
                    resolved_names.borrow_mut().push(name.to_string());
                    (name == "ROLE_RUN_SELECTED_SECRET").then(|| "selected-env-canary".into())
                },
            )
            .unwrap();

        assert_eq!(route.provider.api_key, "selected-env-canary");
        assert_eq!(resolved_names.into_inner(), ["ROLE_RUN_SELECTED_SECRET"]);
        assert_eq!(std::fs::read_to_string(path).unwrap(), original);
    }

    #[test]
    fn role_run_profile_parse_errors_do_not_echo_secret_canaries() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("work.toml");
        std::fs::write(
            &path,
            "[providers.bad]\napi_key = \"PARSE-SECRET-CANARY\"\nmodels = [\n",
        )
        .unwrap();

        let error = ConfigStore::load_role_run_profile_shape(&path, "work")
            .unwrap_err()
            .to_string();

        assert_eq!(error, "role-run profile configuration is invalid TOML");
        assert!(!error.contains("PARSE-SECRET-CANARY"));
    }

    #[test]
    fn role_run_read_only_store_reuses_profile_credentials_without_mutation() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("profile-data/credentials");
        let writable = crate::provider_secrets::FileProviderSecretStore::new(root.clone());
        ProviderSecretStore::store(&writable, "openai", b"existing-profile-secret").unwrap();
        let credential = root.join("openai");
        let before = std::fs::read(&credential).unwrap();
        let store = RoleRunReadOnlyProviderSecretStore {
            files: crate::provider_secrets::FileProviderSecretStore::new(root),
        };

        let loaded = ProviderSecretStore::load(&store, "profile/work/provider/openai")
            .unwrap()
            .unwrap();

        assert_eq!(loaded, b"existing-profile-secret");
        assert!(ProviderSecretStore::store(&store, "openai", b"replacement").is_err());
        assert!(ProviderSecretStore::delete(&store, "openai").is_err());
        assert_eq!(std::fs::read(credential).unwrap(), before);
    }

    #[test]
    fn role_run_execution_rejects_route_effort_and_weak_sandbox_before_credentials() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("work.toml");
        let reference = crate::provider_secrets::profile_credential_reference("work", "openai");
        let original =
            role_run_profile_fixture().replace("${ROLE_RUN_SELECTED_SECRET}", &reference);
        std::fs::write(&path, &original).unwrap();
        let secrets = Arc::new(FakeProviderSecrets::default());
        let store = ConfigStore::with_profile_secret_store(
            &path,
            crate::runtime::ActiveProfile::Named("work".into()),
            secrets.clone(),
        );

        let error = store
            .load_role_run_selected_route_with_env(
                "openai",
                "gpt-safe",
                Some(ReasoningEffort::Max),
                |_| panic!("invalid route must not resolve environment values"),
            )
            .unwrap_err()
            .to_string();
        assert!(error.contains("not supported"));
        assert_eq!(
            secrets
                .load_calls
                .load(std::sync::atomic::Ordering::Relaxed),
            0
        );

        let weak = original.replace("sandbox_mode = \"required\"", "sandbox_mode = \"off\"");
        std::fs::write(&path, weak).unwrap();
        let error = store
            .load_role_run_selected_route_with_env("openai", "gpt-safe", None, |_| {
                panic!("weak sandbox must not resolve environment values")
            })
            .unwrap_err()
            .to_string();
        assert!(error.contains("sandbox_mode = \"required\""));
        assert_eq!(
            secrets
                .load_calls
                .load(std::sync::atomic::Ordering::Relaxed),
            0
        );
    }

    fn direct_key_update(provider: &str) -> ProviderModelUpdate {
        ProviderModelUpdate {
            google: None,
            provider: provider.into(),
            base_url: "https://example.invalid/v1".into(),
            api_key: ProviderApiKeyUpdate::Keychain,
            model: "model-v1".into(),
            original_model: None,
            require_new_provider: true,
            select_saved_model: true,
            protocol_override: None,
        }
    }

    #[test]
    fn missing_provider_protocol_defaults_without_rewriting_config() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        let raw = r#"# protocol intentionally omitted
[providers.local]
base_url = "http://127.0.0.1:11434/v1"
api_key = ""
models = ["demo"]

[defaults]
provider = "local"
model = "demo"
"#;
        std::fs::write(&path, raw).unwrap();

        let config = ConfigStore::new(&path).load_with_env(|_| None).unwrap();

        assert_eq!(
            config.providers["local"].protocol,
            crate::agent::ProviderProtocol::ChatCompletions
        );
        assert_eq!(std::fs::read_to_string(path).unwrap(), raw);
    }

    #[test]
    fn openai_template_persists_explicit_responses_without_changing_defaults() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        let original = r#"# keep template install comment
[providers.current]
base_url = "https://current.invalid/v1"
api_key = "${CURRENT_API_KEY}"
models = ["current-model"]

[defaults]
provider = "current"
model = "current-model"
"#;
        std::fs::write(&path, original).unwrap();
        let store = ConfigStore::new(&path);
        let update = ProviderModelUpdate {
            google: None,
            provider: OPENAI_PROVIDER_TEMPLATE_ID.into(),
            base_url: OPENAI_PROVIDER_TEMPLATE_BASE_URL.into(),
            api_key: ProviderApiKeyUpdate::Environment(OPENAI_PROVIDER_TEMPLATE_API_KEY_ENV.into()),
            model: OPENAI_PROVIDER_TEMPLATE_MODEL.into(),
            original_model: None,
            require_new_provider: true,
            select_saved_model: false,
            protocol_override: Some(crate::agent::ProviderProtocol::OpenAiResponses),
        };

        store
            .upsert_provider_model_with_secret(&update, None)
            .unwrap();

        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(saved.contains("# keep template install comment"));
        assert!(saved.contains("protocol = \"openai_responses\""));
        let config: CaConfig = toml::from_str(&saved).unwrap();
        let openai = &config.providers[OPENAI_PROVIDER_TEMPLATE_ID];
        assert_eq!(openai.base_url, OPENAI_PROVIDER_TEMPLATE_BASE_URL);
        assert_eq!(
            openai.api_key,
            format!("${{{OPENAI_PROVIDER_TEMPLATE_API_KEY_ENV}}}")
        );
        assert_eq!(openai.models, [OPENAI_PROVIDER_TEMPLATE_MODEL]);
        assert_eq!(
            openai.protocol,
            crate::agent::ProviderProtocol::OpenAiResponses
        );
        assert_eq!(config.defaults.provider, "current");
        assert_eq!(config.defaults.model, "current-model");

        let before_duplicate = saved;
        let error = store
            .upsert_provider_model_with_secret(&update, None)
            .unwrap_err();
        assert!(error.to_string().contains("already exists"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before_duplicate);

        let mut invalid_edit = update.clone();
        invalid_edit.require_new_provider = false;
        invalid_edit.model = "another-model".into();
        let error = store
            .upsert_provider_model_with_secret(&invalid_edit, None)
            .unwrap_err();
        assert!(error
            .to_string()
            .contains("only be set while adding a new provider"));
        assert_eq!(std::fs::read_to_string(path).unwrap(), before_duplicate);

        let bare_path = temp.path().join("bare-config.toml");
        std::fs::write(&bare_path, original).unwrap();
        let bare = ConfigStore::for_profile(&bare_path, crate::runtime::ActiveProfile::Bare);
        let error = bare
            .upsert_provider_model_with_secret(&update, None)
            .unwrap_err();
        assert!(error.to_string().contains("bare mode"));
        assert_eq!(std::fs::read_to_string(bare_path).unwrap(), original);
    }

    #[test]
    fn voice_is_disabled_by_default_and_validates_closed_backends() {
        let mut config = CaConfig::default_config();
        assert!(!config.voice.enabled);
        assert!(config.voice.validate(&config.providers).is_ok());

        config.voice.enabled = true;
        assert!(config
            .voice
            .validate(&config.providers)
            .unwrap_err()
            .to_string()
            .contains("no backend"));
        config.voice.backend = Some(VoiceBackendConfig::Local {
            executable: PathBuf::from("relative-transcriber"),
            args: vec!["{audio}".into()],
            timeout_secs: 30,
        });
        assert!(config
            .voice
            .validate(&config.providers)
            .unwrap_err()
            .to_string()
            .contains("absolute"));
        config.voice.backend = Some(VoiceBackendConfig::Provider {
            provider: "missing".into(),
            model: "whisper".into(),
            timeout_secs: 30,
        });
        assert!(config
            .voice
            .validate(&config.providers)
            .unwrap_err()
            .to_string()
            .contains("not configured"));
    }

    #[test]
    fn voice_debug_is_secret_free() {
        let config = VoiceConfig {
            enabled: true,
            backend: Some(VoiceBackendConfig::Provider {
                provider: "safe-id".into(),
                model: "secret-model-value".into(),
                timeout_secs: 30,
            }),
            max_seconds: 60,
        };
        let debug = format!("{config:?}");
        assert!(debug.contains("provider"));
        assert!(!debug.contains("safe-id"));
        assert!(!debug.contains("secret-model-value"));
    }

    #[test]
    fn explicit_chat_completions_protocol_loads_and_unknown_protocol_fails_before_resolution() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        let write = |protocol: &str| {
            std::fs::write(
                &path,
                format!(
                    r#"[providers.local]
base_url = "http://127.0.0.1:11434/v1"
api_key = "${{MUST_NOT_RESOLVE}}"
protocol = "{protocol}"
models = ["demo"]
"#,
                ),
            )
            .unwrap();
        };
        write("chat_completions");
        let loaded = ConfigStore::new(&path)
            .load_with_env(|_| Some("resolved-only-for-supported-protocol".into()))
            .unwrap();
        assert_eq!(
            loaded.providers["local"].protocol,
            crate::agent::ProviderProtocol::ChatCompletions
        );

        write("openai_responses");
        let loaded = ConfigStore::new(&path)
            .load_with_env(|_| Some("resolved-for-responses".into()))
            .unwrap();
        assert_eq!(
            loaded.providers["local"].protocol,
            crate::agent::ProviderProtocol::OpenAiResponses
        );

        write("anthropic_messages");
        let loaded = ConfigStore::new(&path)
            .load_with_env(|_| Some("resolved-for-anthropic".into()))
            .unwrap();
        assert_eq!(
            loaded.providers["local"].protocol,
            crate::agent::ProviderProtocol::AnthropicMessages
        );

        write("responses");
        let resolver_called = std::cell::Cell::new(false);
        let error = ConfigStore::new(&path)
            .load_with_env(|_| {
                resolver_called.set(true);
                None
            })
            .unwrap_err()
            .to_string();
        assert!(!resolver_called.get());
        assert!(error.contains("unknown variant `responses`"));
    }

    #[test]
    fn raw_placeholder_survives_runtime_resolution_and_rule_patch() {
        let temp = tempfile::tempdir().unwrap();
        let paths = RuntimePaths::from_home(temp.path());
        let store = ConfigStore::new(paths.config_file.clone());
        std::fs::create_dir_all(paths.config_file.parent().unwrap()).unwrap();
        std::fs::write(
            &paths.config_file,
            r#"# retain this comment
[providers.test]
base_url = "https://example.invalid/v1"
api_key = "${CA_TEST_SECRET}"
models = ["test-model"]

[defaults]
provider = "test"
model = "test-model"
"#,
        )
        .unwrap();

        let resolved = store
            .load_with_env(|name| (name == "CA_TEST_SECRET").then(|| "resolved-value".into()))
            .unwrap();
        assert_eq!(resolved.providers["test"].api_key, "resolved-value");

        store
            .append_approval_rule(&ApprovalRule::AllowTool("read_file".into()))
            .unwrap();
        store.select_model("test", "other-model").unwrap();

        let raw = std::fs::read_to_string(&paths.config_file).unwrap();
        assert!(raw.contains("${CA_TEST_SECRET}"));
        assert!(!raw.contains("resolved-value"));
        assert!(raw.contains("# retain this comment"));
        let reparsed: CaConfig = toml::from_str(&raw).unwrap();
        assert_eq!(reparsed.defaults.model, "other-model");
        assert_eq!(
            reparsed.approval_rules,
            vec![ApprovalRule::AllowTool("read_file".into())]
        );
    }

    #[test]
    fn rich_permission_patch_preserves_legacy_rules_comments_and_unrelated_toml() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        std::fs::write(
            &path,
            r#"# keep top
approval_rules = [{ AllowTool = "read_file" }] # keep legacy spelling

[providers.local]
base_url = "http://127.0.0.1:11434/v1"
api_key = ""
models = ["demo"]

[custom]
keep = "yes"
"#,
        )
        .unwrap();
        let store = ConfigStore::new(&path);
        let rule = crate::permission::PermissionRule {
            decision: crate::permission::PermissionDecision::Ask,
            matcher: crate::permission::PermissionMatcher::CommandPrefix {
                prefix: vec!["cargo".into(), "test".into()],
            },
        };

        store.append_permission_rule(&rule).unwrap();

        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(
            raw.contains("approval_rules = [{ AllowTool = \"read_file\" }] # keep legacy spelling")
        );
        assert!(raw.contains("# keep top"));
        assert!(raw.contains("keep = \"yes\""));
        let loaded = store.load_with_env(|_| None).unwrap();
        assert_eq!(loaded.approval_rules.len(), 1);
        assert_eq!(loaded.permission_rules, vec![rule]);
    }

    #[test]
    fn invalid_rich_permissions_fail_load_before_runtime_resolution() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        std::fs::write(
            &path,
            r#"permission_rules = [{ decision = "allow", kind = "path", tool = "write_file", path_pattern = "../escape/**" }]

[providers.local]
base_url = "http://127.0.0.1:11434/v1"
api_key = ""
models = ["demo"]
"#,
        )
        .unwrap();

        let error = ConfigStore::new(&path)
            .load_with_env(|_| None)
            .unwrap_err()
            .to_string();
        assert!(error.contains("canonical absolute globs"));
    }

    #[test]
    fn removing_rich_and_legacy_permissions_is_exact_and_comment_preserving() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        std::fs::write(
            &path,
            r#"# keep me
approval_rules = [{ AllowTool = "read_file" }, { DenyTool = "write_file" }]
permission_rules = [
  { decision = "ask", kind = "tool", tool = "run_command" },
  { decision = "allow", kind = "tool", tool = "search_files" },
]

[providers.local]
base_url = "http://127.0.0.1:11434/v1"
api_key = ""
models = ["demo"]
"#,
        )
        .unwrap();
        let store = ConfigStore::new(&path);

        store.remove_permission_rule(0).unwrap();
        store.remove_approval_rule(1).unwrap();

        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains("# keep me"));
        let parsed: CaConfig = toml::from_str(&raw).unwrap();
        assert_eq!(
            parsed.approval_rules,
            vec![ApprovalRule::AllowTool("read_file".into())]
        );
        assert_eq!(parsed.permission_rules.len(), 1);
        assert_eq!(
            parsed.permission_rules[0].matcher.safe_label(),
            "search_files"
        );
        assert!(store.remove_permission_rule(9).is_err());
        assert!(store.remove_approval_rule(9).is_err());
    }

    #[test]
    fn legacy_xiaomi_key_reference_resolves_official_mimo_environment_name() {
        let temp = tempfile::tempdir().unwrap();
        let paths = RuntimePaths::from_home(temp.path());
        let store = ConfigStore::new(paths.config_file.clone());
        std::fs::create_dir_all(paths.config_file.parent().unwrap()).unwrap();
        std::fs::write(
            &paths.config_file,
            r#"[providers.mimo]
base_url = "https://token-plan-sgp.xiaomimimo.com/v1"
api_key = "${XIAOMI_API_KEY}"
models = ["mimo-v2.5"]

[defaults]
provider = "mimo"
model = "mimo-v2.5"
"#,
        )
        .unwrap();

        let loaded = store
            .load_with_env(|name| (name == "MIMO_API_KEY").then(|| "resolved-mimo-key".into()))
            .unwrap();
        assert_eq!(loaded.providers["mimo"].api_key, "resolved-mimo-key");

        store.select_model("mimo", "mimo-v2.5-pro").unwrap();
        let raw = std::fs::read_to_string(paths.config_file).unwrap();
        assert!(raw.contains("${XIAOMI_API_KEY}"));
        assert!(!raw.contains("resolved-mimo-key"));
    }

    #[test]
    fn provider_manager_always_includes_four_first_class_connection_rows() {
        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join("config.toml");
        std::fs::write(
            &config_path,
            r#"[defaults]
provider = "local"
model = "auto"
"#,
        )
        .unwrap();
        let store = ConfigStore::new(&config_path);

        let entries = store.provider_management_entries().unwrap();

        assert_eq!(
            entries
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>(),
            ["mimo", "openai-codex", "deepseek", "openai"]
        );
        assert!(entries.iter().all(|entry| !entry.configured));
        assert!(entries.iter().all(|entry| !entry.connected));
    }

    #[test]
    fn provider_api_key_env_exposes_only_exact_safe_references() {
        let temp = tempfile::tempdir().unwrap();
        let paths = RuntimePaths::from_home(temp.path());
        let store = ConfigStore::new(paths.config_file.clone());
        std::fs::create_dir_all(paths.config_file.parent().unwrap()).unwrap();
        std::fs::write(
            &paths.config_file,
            r#"[providers.safe]
base_url = "https://safe.invalid/v1"
api_key = "${SAFE_API_KEY}"
models = ["safe-model"]

[providers.literal]
base_url = "https://literal.invalid/v1"
api_key = "literal-secret"
models = ["literal-model"]

[providers.composite]
base_url = "https://composite.invalid/v1"
api_key = "Bearer ${COMPOSITE_API_KEY}"
models = ["composite-model"]

[defaults]
provider = "safe"
model = "safe-model"
"#,
        )
        .unwrap();

        assert_eq!(
            store.provider_api_key_source("safe").unwrap(),
            ProviderApiKeySource::Environment("SAFE_API_KEY".into())
        );
        assert_eq!(
            store.provider_api_key_source("literal").unwrap(),
            ProviderApiKeySource::Hidden
        );
        assert_eq!(
            store.provider_api_key_source("composite").unwrap(),
            ProviderApiKeySource::Hidden
        );
        assert_eq!(
            store.provider_api_key_source("missing").unwrap(),
            ProviderApiKeySource::None
        );
    }

    #[test]
    fn direct_provider_key_is_resolved_from_injected_store_and_never_written_to_config() {
        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join("config.toml");
        std::fs::write(&config_path, "# keep me\n").unwrap();
        let secrets = Arc::new(FakeProviderSecrets::default());
        let store = ConfigStore::with_secret_store(&config_path, secrets.clone());
        let secret = SecretValue::from_text("not-a-real-secret");

        store
            .upsert_provider_model_with_secret(&direct_key_update("mimo"), Some(&secret))
            .unwrap();

        let raw = std::fs::read_to_string(&config_path).unwrap();
        assert!(raw.contains("# keep me"));
        assert!(raw.contains("credential://ca/provider/mimo"));
        assert!(!raw.contains("not-a-real-secret"));
        assert_eq!(
            store.provider_api_key_source("mimo").unwrap(),
            ProviderApiKeySource::Keychain
        );
        assert_eq!(
            store.load().unwrap().providers["mimo"].api_key,
            "not-a-real-secret"
        );
    }

    #[test]
    fn failed_keychain_write_restores_original_config_without_exposing_secret() {
        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join("config.toml");
        let original = "# unchanged after failure\n";
        std::fs::write(&config_path, original).unwrap();
        let secrets = Arc::new(FakeProviderSecrets::default());
        secrets
            .fail_store
            .store(true, std::sync::atomic::Ordering::Relaxed);
        let store = ConfigStore::with_secret_store(&config_path, secrets);
        let secret = SecretValue::from_text("must-not-leak");

        let error = store
            .upsert_provider_model_with_secret(&direct_key_update("deepseek"), Some(&secret))
            .unwrap_err()
            .to_string();

        assert!(error.contains("could not save the API key"));
        assert!(!error.contains("must-not-leak"));
        assert_eq!(std::fs::read_to_string(config_path).unwrap(), original);
        assert_eq!(secret.with_secret(|value| value.to_vec()), b"must-not-leak");
    }

    #[test]
    fn missing_keychain_item_fails_before_a_reference_can_reach_provider_runtime() {
        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join("config.toml");
        std::fs::write(
            &config_path,
            r#"[providers.mimo]
base_url = "https://example.invalid/v1"
api_key = "keychain://ca/provider/mimo"
models = ["model-v1"]

[defaults]
provider = "mimo"
model = "model-v1"
"#,
        )
        .unwrap();
        let store =
            ConfigStore::with_secret_store(&config_path, Arc::new(FakeProviderSecrets::default()));

        let error = store.load().unwrap_err().to_string();
        assert!(error.contains("saved credential for provider mimo is unavailable"));
        assert!(!error.contains("Authorization"));
    }

    #[test]
    fn discontinued_official_deepseek_aliases_upgrade_in_memory_only() {
        let temp = tempfile::tempdir().unwrap();
        let paths = RuntimePaths::from_home(temp.path());
        let store = ConfigStore::new(paths.config_file.clone());
        std::fs::create_dir_all(paths.config_file.parent().unwrap()).unwrap();
        std::fs::write(
            &paths.config_file,
            r#"[providers.deepseek]
base_url = "https://api.deepseek.com/v1"
api_key = "${DEEPSEEK_API_KEY}"
models = ["deepseek-chat", "deepseek-reasoner"]

[defaults]
provider = "deepseek"
model = "deepseek-reasoner"
"#,
        )
        .unwrap();

        let loaded = store.load_with_env(|_| None).unwrap();

        assert_eq!(
            loaded.providers["deepseek"].models,
            ["deepseek-v4-flash", "deepseek-v4-pro"]
        );
        assert_eq!(loaded.defaults.model, "deepseek-v4-flash");
        let raw = std::fs::read_to_string(paths.config_file).unwrap();
        assert!(raw.contains("deepseek-chat"));
        assert!(raw.contains("deepseek-reasoner"));
        assert!(!raw.contains("deepseek-v4-flash"));
    }

    #[test]
    fn provider_model_editor_preserves_secrets_and_migrates_exact_model_keys() {
        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join("config.toml");
        std::fs::write(
            &config_path,
            r#"# keep this comment
[providers.test]
base_url = "https://old.example/v1"
api_key = "${TEST_API_KEY}"
models = ["old-model", "other-model"]

[providers.untouched]
base_url = "http://127.0.0.1:11434/v1"
api_key = "${UNTOUCHED_KEY}"
models = ["local"]

[defaults]
provider = "test"
model = "old-model"

[model_profiles."test/old-model"]
supports_tools = true

[model_context."test/old-model"]
context_window_tokens = 1000

[model_prompt_cache."test/old-model"]
usage_format = "auto"

[[fallbacks]]
# rename the primary side
provider = "test"
model = "old-model"
fallback_provider = "untouched"
fallback_model = "local"

[[fallbacks]]
# rename the fallback side
provider = "untouched"
model = "local"
fallback_provider = "test"
fallback_model = "old-model"
"#,
        )
        .unwrap();
        let store = ConfigStore::new(&config_path);

        store
            .upsert_provider_model_with_secret(
                &ProviderModelUpdate {
                    google: None,
                    provider: "test".into(),
                    base_url: "https://new.example/v1".into(),
                    api_key: ProviderApiKeyUpdate::Preserve,
                    model: "new-model".into(),
                    require_new_provider: false,
                    select_saved_model: true,
                    original_model: Some("old-model".into()),
                    protocol_override: None,
                },
                None,
            )
            .unwrap();

        let raw = std::fs::read_to_string(&config_path).unwrap();
        assert!(raw.contains("# keep this comment"));
        assert!(raw.contains("${TEST_API_KEY}"));
        assert!(raw.contains("${UNTOUCHED_KEY}"));
        assert!(!raw.contains("test/old-model"));
        let config: CaConfig = toml::from_str(&raw).unwrap();
        assert_eq!(
            config.providers["test"].models,
            ["new-model", "other-model"]
        );
        assert_eq!(config.providers["test"].base_url, "https://new.example/v1");
        assert_eq!(config.defaults.model, "new-model");
        assert!(config.model_profiles.contains_key("test/new-model"));
        assert!(config.model_context.contains_key("test/new-model"));
        assert_eq!(
            config
                .model_prompt_cache("test", "new-model")
                .and_then(|item| item.usage_format),
            Some(PromptCacheUsageFormat::Auto)
        );
        assert_eq!(config.fallbacks[0].model, "new-model");
        assert_eq!(config.fallbacks[0].fallback_model, "local");
        assert_eq!(config.fallbacks[1].model, "local");
        assert_eq!(config.fallbacks[1].fallback_model, "new-model");
        assert!(raw.contains("# rename the primary side"));
        assert!(raw.contains("# rename the fallback side"));

        store
            .upsert_provider_model_with_secret(
                &ProviderModelUpdate {
                    google: None,
                    provider: "test".into(),
                    base_url: String::new(),
                    api_key: ProviderApiKeyUpdate::Preserve,
                    model: "added-model".into(),
                    require_new_provider: false,
                    select_saved_model: true,
                    original_model: None,
                    protocol_override: None,
                },
                None,
            )
            .unwrap();
        let raw = std::fs::read_to_string(&config_path).unwrap();
        let config: CaConfig = toml::from_str(&raw).unwrap();
        assert_eq!(
            config.providers["test"].models,
            ["new-model", "other-model", "added-model"]
        );
        assert_eq!(config.providers["test"].api_key, "${TEST_API_KEY}");
        assert_eq!(config.defaults.model, "added-model");
    }

    #[test]
    fn provider_management_snapshot_is_safe_and_keeps_zero_model_providers() {
        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join("config.toml");
        std::fs::write(
            &config_path,
            r#"[providers.empty]
base_url = "https://empty.invalid/v1"
api_key = ""
models = []

[providers.environment]
base_url = "https://environment.invalid/v1"
api_key = "${SAFE_PROVIDER_KEY}"
models = ["one"]

[providers.literal]
base_url = "https://endpoint-user:endpoint-password@literal.invalid/v1?api_key=query-secret"
api_key = "must-never-escape"
models = ["two"]
"#,
        )
        .unwrap();
        let snapshot = ConfigStore::new(config_path)
            .provider_management_snapshot()
            .unwrap();
        let entries = &snapshot.providers;

        assert_eq!(entries.len(), 3);
        assert!(snapshot.default_provider.is_empty());
        assert!(snapshot.default_model.is_empty());
        assert_eq!(snapshot.sandbox_mode, "required");
        assert_eq!(snapshot.approval_mode, "auto-review");
        assert_eq!(entries[0].id, "empty");
        assert!(entries[0].configured_models.is_empty());
        assert_eq!(entries[0].api_key_source, ProviderApiKeySource::None);
        assert_eq!(
            entries[1].api_key_source,
            ProviderApiKeySource::Environment("SAFE_PROVIDER_KEY".into())
        );
        assert_eq!(entries[2].api_key_source, ProviderApiKeySource::Hidden);
        assert_eq!(entries[2].base_url, "https://literal.invalid/v1");
        assert!(!format!("{entries:?}").contains("must-never-escape"));
        assert!(!format!("{entries:?}").contains("endpoint-password"));
        assert!(!format!("{entries:?}").contains("query-secret"));
        assert!(!format!("{snapshot:?}").contains("must-never-escape"));
    }

    #[test]
    fn offline_provider_snapshot_never_reads_saved_secret_material() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        std::fs::write(
            &path,
            r#"[providers.mimo]
base_url = "https://token-plan-sgp.xiaomimimo.com/v1"
api_key = "credential://ca/provider/mimo"
models = ["mimo-v2.5-pro"]

[defaults]
provider = "mimo"
model = "mimo-v2.5-pro"
"#,
        )
        .unwrap();
        let secrets = Arc::new(FakeProviderSecrets::default());
        let store = ConfigStore::with_secret_store(&path, secrets.clone());

        let snapshot = store.provider_management_snapshot_offline().unwrap();

        assert!(snapshot.providers[0].connected);
        assert_eq!(
            secrets
                .load_calls
                .load(std::sync::atomic::Ordering::Relaxed),
            0
        );
    }

    #[test]
    fn provider_only_edit_preserves_models_defaults_scoped_settings_and_comments() {
        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join("config.toml");
        std::fs::write(
            &config_path,
            r#"# preserve provider settings comment
[providers.test]
base_url = "https://old.invalid/v1"
api_key = "${OLD_KEY}"
protocol = "openai_responses"
models = ["one", "two"]

[defaults]
provider = "test"
model = "two"

[model_profiles."test/two"]
supports_tools = true

[model_context."test/two"]
context_window_tokens = 4096

[review]
provider = "test"
model = "one"
"#,
        )
        .unwrap();
        let store = ConfigStore::new(&config_path);
        store
            .update_provider_with_secret(
                &ProviderSettingsUpdate {
                    google: None,
                    provider: "test".into(),
                    base_url: "https://new.invalid/v1".into(),
                    api_key: ProviderApiKeyUpdate::Environment("NEW_KEY".into()),
                    prompt_cache_mode: PromptCacheMode::StablePrefix,
                },
                None,
            )
            .unwrap();

        let raw = std::fs::read_to_string(&config_path).unwrap();
        assert!(raw.contains("# preserve provider settings comment"));
        assert!(raw.contains("${NEW_KEY}"));
        assert!(!raw.contains("${OLD_KEY}"));
        let config: CaConfig = toml::from_str(&raw).unwrap();
        assert_eq!(config.providers["test"].models, ["one", "two"]);
        assert_eq!(
            config.providers["test"].protocol,
            crate::agent::ProviderProtocol::OpenAiResponses
        );
        assert!(raw.contains("protocol = \"openai_responses\""));
        assert!(raw.contains("prompt_cache_mode = \"stable_prefix\""));
        assert_eq!(
            config.providers["test"].prompt_cache_mode,
            PromptCacheMode::StablePrefix
        );
        assert_eq!(config.defaults.provider, "test");
        assert_eq!(config.defaults.model, "two");
        assert!(config.model_profiles.contains_key("test/two"));
        assert!(config.model_context.contains_key("test/two"));
        assert_eq!(config.review.provider.as_deref(), Some("test"));
        assert_eq!(config.review.model.as_deref(), Some("one"));
    }

    #[test]
    fn reasoning_extensions_and_prompt_cache_modes_round_trip_compatibly() {
        assert_eq!(
            "none".parse::<ReasoningEffort>().unwrap(),
            ReasoningEffort::None
        );
        assert_eq!(
            "xhigh".parse::<ReasoningEffort>().unwrap(),
            ReasoningEffort::XHigh
        );
        assert_eq!(ReasoningEffort::None.to_string(), "none");
        assert_eq!(ReasoningEffort::XHigh.to_string(), "xhigh");

        let implicit: ProviderConfig = toml::from_str(
            r#"base_url = "https://api.openai.com/v1"
api_key = "${OPENAI_API_KEY}"
protocol = "openai_responses"
models = ["gpt-5.6-terra"]
"#,
        )
        .unwrap();
        assert_eq!(implicit.prompt_cache_mode, PromptCacheMode::Implicit);

        let stable: ProviderConfig = toml::from_str(
            r#"base_url = "https://api.openai.com/v1"
api_key = "${OPENAI_API_KEY}"
protocol = "openai_responses"
prompt_cache_mode = "stable_prefix"
models = ["gpt-5.6-terra"]
"#,
        )
        .unwrap();
        assert_eq!(stable.prompt_cache_mode, PromptCacheMode::StablePrefix);
    }

    #[test]
    fn model_prompt_cache_schema_round_trips_without_migrating_legacy_provider_mode() {
        let raw = r#"
[providers.openai]
base_url = "https://api.openai.com/v1"
api_key = "${OPENAI_API_KEY}"
protocol = "openai_responses"
prompt_cache_mode = "stable_prefix"
models = ["gpt-5.6-terra"]

[providers.custom]
base_url = "https://compatible.example/v1"
api_key = "${CUSTOM_API_KEY}"
protocol = "openai_responses"
models = ["model-a"]

[defaults]
provider = "openai"
model = "gpt-5.6-terra"

[model_prompt_cache."openai/gpt-5.6-terra"]
policy = "off"

[model_prompt_cache."custom/model-a"]
request_format = "openai_responses_explicit"
usage_format = "auto"
policy = "stable_prefix"
"#;
        let config: CaConfig = toml::from_str(raw).unwrap();
        validate_model_prompt_cache(&config).unwrap();
        assert_eq!(
            config.providers["openai"].prompt_cache_mode,
            PromptCacheMode::StablePrefix
        );
        assert_eq!(
            config.model_prompt_cache("openai", "gpt-5.6-terra"),
            Some(&ModelPromptCacheConfig {
                request_format: None,
                usage_format: None,
                policy: Some(PromptCachePolicy::Off),
            })
        );
        assert_eq!(
            config.model_prompt_cache("custom", "model-a"),
            Some(&ModelPromptCacheConfig {
                request_format: Some(PromptCacheRequestFormat::OpenAiResponsesExplicit),
                usage_format: Some(PromptCacheUsageFormat::Auto),
                policy: Some(PromptCachePolicy::StablePrefix),
            })
        );
    }

    #[test]
    fn deepseek_dsml_model_profile_opt_in_is_closed_and_protocol_validated() {
        let valid = r#"
[providers.proxy]
base_url = "https://compatible.example/v1"
api_key = "${PROXY_API_KEY}"
protocol = "chat_completions"
models = ["deepseek-v4"]

[defaults]
provider = "proxy"
model = "deepseek-v4"

[model_profiles."proxy/deepseek-v4"]
supports_tools = true
tool_call_format = "deepseek_dsml"
"#;
        let config: CaConfig = toml::from_str(valid).unwrap();
        validate_model_prompt_cache(&config).unwrap();
        assert_eq!(
            config.model_profiles["proxy/deepseek-v4"].tool_call_format,
            Some(ToolCallFormat::DeepseekDsml)
        );
        let round_trip: CaConfig = toml::from_str(&toml::to_string(&config).unwrap()).unwrap();
        assert_eq!(
            round_trip.model_profiles["proxy/deepseek-v4"],
            config.model_profiles["proxy/deepseek-v4"]
        );

        let responses = valid.replace("chat_completions", "openai_responses");
        assert!(
            validate_model_prompt_cache(&toml::from_str(&responses).unwrap())
                .unwrap_err()
                .to_string()
                .contains("chat_completions")
        );

        let toolless = valid.replace("supports_tools = true", "supports_tools = false");
        assert!(
            validate_model_prompt_cache(&toml::from_str(&toolless).unwrap())
                .unwrap_err()
                .to_string()
                .contains("tool-capable")
        );
    }

    #[test]
    fn model_prompt_cache_validation_rejects_unsupported_contract_combinations() {
        let first_party_formats = r#"
[providers.mimo]
base_url = "https://api.xiaomimimo.com/v1"
api_key = "${MIMO_API_KEY}"
protocol = "chat_completions"
models = ["mimo-v2.5"]
[defaults]
provider = "mimo"
model = "mimo-v2.5"
[model_prompt_cache."mimo/mimo-v2.5"]
request_format = "provider_managed"
"#;
        let config: CaConfig = toml::from_str(first_party_formats).unwrap();
        assert!(validate_model_prompt_cache(&config)
            .unwrap_err()
            .to_string()
            .contains("formats are built in"));

        let unsupported_policy = first_party_formats.replace(
            "request_format = \"provider_managed\"",
            "policy = \"stable_prefix\"",
        );
        let config: CaConfig = toml::from_str(&unsupported_policy).unwrap();
        assert!(validate_model_prompt_cache(&config)
            .unwrap_err()
            .to_string()
            .contains("unsupported"));

        let custom_without_format = unsupported_policy
            .replace("providers.mimo", "providers.custom")
            .replace("api.xiaomimimo.com", "compatible.example")
            .replace("mimo/mimo-v2.5", "custom/mimo-v2.5")
            .replace("provider = \"mimo\"", "provider = \"custom\"");
        let config: CaConfig = toml::from_str(&custom_without_format).unwrap();
        assert!(validate_model_prompt_cache(&config)
            .unwrap_err()
            .to_string()
            .contains("requires a request_format"));
    }

    #[test]
    fn model_prompt_cache_validation_rejects_malformed_keys_and_protocol_dialects() {
        let chat = r#"
[providers.custom]
base_url = "https://compatible.example/v1"
api_key = "${CUSTOM_API_KEY}"
protocol = "chat_completions"
models = ["model-a"]
[defaults]
provider = "custom"
model = "model-a"
[model_prompt_cache."custom/model-a"]
request_format = "openai_responses_explicit"
usage_format = "auto"
policy = "automatic"
"#;
        let error = |raw: &str| {
            let config: CaConfig = toml::from_str(raw).unwrap();
            validate_model_prompt_cache(&config)
                .unwrap_err()
                .to_string()
        };

        assert!(error(chat).contains("requires the openai_responses protocol"));
        assert!(error(&chat.replace(
            "request_format = \"openai_responses_explicit\"\nusage_format = \"auto\"",
            "request_format = \"provider_managed\"\nusage_format = \"read_write\""
        ))
        .contains("requires the anthropic_messages protocol"));
        assert!(error(
            &chat
                .replace(
                    "protocol = \"chat_completions\"",
                    "protocol = \"openai_responses\""
                )
                .replace("usage_format = \"auto\"", "usage_format = \"hit_miss\"")
        )
        .contains("requires the chat_completions protocol"));
        assert!(error(&chat.replace("custom/model-a", "custom").replace(
            "request_format = \"openai_responses_explicit\"",
            "request_format = \"provider_managed\""
        ))
        .contains("exact provider/model syntax"));
    }

    #[test]
    fn model_prompt_cache_update_is_targeted_and_preserves_legacy_config() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        std::fs::write(
            &path,
            r#"# preserve cache comment
[providers.custom]
base_url = "https://compatible.example/v1"
api_key = "${CUSTOM_API_KEY}"
protocol = "openai_responses"
prompt_cache_mode = "off"
models = ["model-a", "model-b"]
[defaults]
provider = "custom"
model = "model-a"
"#,
        )
        .unwrap();
        ConfigStore::new(&path)
            .update_model_prompt_cache(&ModelPromptCacheUpdate {
                provider: "custom".into(),
                model: "model-a".into(),
                request_format: Some(PromptCacheRequestFormat::OpenAiResponsesExplicit),
                usage_format: Some(PromptCacheUsageFormat::OpenAiDetails),
                policy: Some(PromptCachePolicy::StablePrefix),
            })
            .unwrap();

        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(saved.contains("# preserve cache comment"));
        assert!(saved.contains("prompt_cache_mode = \"off\""));
        let config: CaConfig = toml::from_str(&saved).unwrap();
        assert!(config.model_prompt_cache("custom", "model-b").is_none());
        assert_eq!(
            config
                .model_prompt_cache("custom", "model-a")
                .and_then(|item| item.policy),
            Some(PromptCachePolicy::StablePrefix)
        );
    }

    #[test]
    fn provider_config_debug_redacts_runtime_authority() {
        let provider = ProviderConfig {
            google: None,
            tool_schema_policy: Default::default(),
            base_url: "https://user:password@example.invalid/v1?token=query-secret".into(),
            api_key: "sentinel-provider-secret".into(),
            protocol: crate::agent::ProviderProtocol::ChatCompletions,
            prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
            models: vec!["model".into()],
            timeout: ProviderTimeoutConfig::default(),
        };

        let debug = format!("{provider:?}");

        assert!(debug.contains("chat_completions"));
        assert!(debug.contains("model_count"));
        for forbidden in [
            "sentinel-provider-secret",
            "password",
            "query-secret",
            "example.invalid",
        ] {
            assert!(!debug.contains(forbidden));
        }
    }

    #[test]
    fn add_provider_rejects_duplicate_without_writing() {
        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join("config.toml");
        let original = r#"# duplicate must preserve this file
[providers.test]
base_url = "https://old.invalid/v1"
api_key = "${TEST_KEY}"
models = ["one"]
"#;
        std::fs::write(&config_path, original).unwrap();
        let store = ConfigStore::new(&config_path);
        let error = store
            .upsert_provider_model_with_secret(
                &ProviderModelUpdate {
                    google: None,
                    provider: "test".into(),
                    base_url: "https://new.invalid/v1".into(),
                    api_key: ProviderApiKeyUpdate::Environment("NEW_KEY".into()),
                    model: "two".into(),
                    original_model: None,
                    require_new_provider: true,
                    select_saved_model: true,
                    protocol_override: None,
                },
                None,
            )
            .unwrap_err();

        assert!(error.to_string().contains("already exists"));
        assert_eq!(std::fs::read_to_string(config_path).unwrap(), original);
    }

    #[test]
    fn provider_delete_cleans_exact_scope_keychain_and_configures_replacement() {
        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join("config.toml");
        std::fs::write(
            &config_path,
            r#"[providers.old]
base_url = "https://old.invalid/v1"
api_key = "keychain://ca/provider/old"
models = ["old-one", "old-two"]

# deletion keeps unrelated content
[providers.other]
base_url = "https://other.invalid/v1"
api_key = "${OTHER_KEY}"
models = ["steady"]

[providers.spare]
base_url = "https://spare.invalid/v1"
api_key = ""
models = ["safe"]

[defaults]
provider = "old"
model = "old-one"

[model_profiles."old/old-one"]
supports_tools = true
[model_profiles."oldish/keep"]
supports_tools = true
[model_profiles."other/keep"]
supports_tools = true

[model_context."old/old-two"]
context_window_tokens = 1000
[model_context."oldish/keep"]
context_window_tokens = 2000

[model_prompt_cache."old/old-two"]
usage_format = "auto"
[model_prompt_cache."other/keep"]
usage_format = "auto"

[review]
provider = "old"
model = "old-two"

[[fallbacks]]
# remove exact primary reference
provider = "old"
model = "old-one"
fallback_provider = "other"
fallback_model = "steady"

[[fallbacks]]
# remove exact fallback reference
provider = "other"
model = "steady"
fallback_provider = "old"
fallback_model = "old-two"

[[fallbacks]]
# preserve unrelated fallback rule
provider = "other"
model = "steady"
fallback_provider = "spare"
fallback_model = "safe"
causes = ["timeout"]
"#,
        )
        .unwrap();
        let secrets = Arc::new(FakeProviderSecrets::default());
        secrets
            .values
            .lock()
            .unwrap()
            .insert("old".into(), b"saved-key".to_vec());
        let store = ConfigStore::with_secret_store(&config_path, secrets.clone());

        store
            .delete_provider(&ProviderDeleteUpdate {
                provider: "old".into(),
                replacement: Some(ModelSelection {
                    provider: "other".into(),
                    model: "live-only".into(),
                }),
            })
            .unwrap();

        assert!(!secrets.values.lock().unwrap().contains_key("old"));
        let raw = std::fs::read_to_string(&config_path).unwrap();
        assert!(raw.contains("# deletion keeps unrelated content"));
        let config: CaConfig = toml::from_str(&raw).unwrap();
        assert!(!config.providers.contains_key("old"));
        assert_eq!(config.providers["other"].models, ["steady", "live-only"]);
        assert_eq!(config.defaults.provider, "other");
        assert_eq!(config.defaults.model, "live-only");
        assert!(!config.model_profiles.contains_key("old/old-one"));
        assert!(!config.model_context.contains_key("old/old-two"));
        assert!(config.model_prompt_cache("old", "old-two").is_none());
        assert!(config.model_profiles.contains_key("oldish/keep"));
        assert!(config.model_context.contains_key("oldish/keep"));
        assert!(config.model_profiles.contains_key("other/keep"));
        assert_eq!(
            config
                .model_prompt_cache("other", "keep")
                .and_then(|item| item.usage_format),
            Some(PromptCacheUsageFormat::Auto)
        );
        assert_eq!(config.review, ReviewConfig::default());
        assert_eq!(config.fallbacks.len(), 1);
        assert_eq!(config.fallbacks[0].provider, "other");
        assert_eq!(config.fallbacks[0].fallback_provider, "spare");
        assert_eq!(
            config.fallbacks[0].causes,
            [crate::fallback::ProviderFallbackCause::Timeout]
        );
        assert!(raw.contains("# preserve unrelated fallback rule"));
        assert!(!raw.contains("# remove exact primary reference"));
        assert!(!raw.contains("# remove exact fallback reference"));
    }

    #[test]
    fn provider_delete_without_replacement_or_with_keychain_failure_is_atomic() {
        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join("config.toml");
        let original = r#"# remains byte-for-byte identical
[providers.only]
base_url = "https://only.invalid/v1"
api_key = "keychain://ca/provider/only"
models = ["one"]

[defaults]
provider = "only"
model = "one"
"#;
        std::fs::write(&config_path, original).unwrap();
        let secrets = Arc::new(FakeProviderSecrets::default());
        secrets
            .values
            .lock()
            .unwrap()
            .insert("only".into(), b"saved-key".to_vec());
        let store = ConfigStore::with_secret_store(&config_path, secrets.clone());

        let error = store
            .delete_provider(&ProviderDeleteUpdate {
                provider: "only".into(),
                replacement: None,
            })
            .unwrap_err();
        assert!(error.to_string().contains("another provider model"));
        assert_eq!(std::fs::read_to_string(&config_path).unwrap(), original);
        assert!(secrets.values.lock().unwrap().contains_key("only"));

        std::fs::write(
            &config_path,
            format!(
                "{original}\n[providers.backup]\nbase_url = \"https://backup.invalid/v1\"\napi_key = \"${{BACKUP_KEY}}\"\nmodels = [\"backup\"]\n"
            ),
        )
        .unwrap();
        let before_failed_delete = std::fs::read_to_string(&config_path).unwrap();
        secrets
            .fail_store
            .store(true, std::sync::atomic::Ordering::Relaxed);
        let error = store
            .delete_provider(&ProviderDeleteUpdate {
                provider: "only".into(),
                replacement: Some(ModelSelection {
                    provider: "backup".into(),
                    model: "backup".into(),
                }),
            })
            .unwrap_err();
        assert!(error
            .to_string()
            .contains("could not delete the saved API key"));
        assert_eq!(
            std::fs::read_to_string(config_path).unwrap(),
            before_failed_delete
        );
        assert!(secrets.values.lock().unwrap().contains_key("only"));
    }

    #[test]
    fn provider_model_editor_validates_before_writing_and_stores_env_references() {
        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join("config.toml");
        std::fs::write(&config_path, "# untouched\n").unwrap();
        let store = ConfigStore::new(&config_path);
        let before = std::fs::read_to_string(&config_path).unwrap();

        let error = store
            .upsert_provider_model_with_secret(
                &ProviderModelUpdate {
                    google: None,
                    provider: "bad provider".into(),
                    base_url: "https://example.invalid/v1".into(),
                    api_key: ProviderApiKeyUpdate::Environment("literal-secret!".into()),
                    model: "coder".into(),
                    require_new_provider: false,
                    select_saved_model: true,
                    original_model: None,
                    protocol_override: None,
                },
                None,
            )
            .unwrap_err();
        assert!(error.to_string().contains("provider must use"));
        assert_eq!(std::fs::read_to_string(&config_path).unwrap(), before);

        store
            .upsert_provider_model_with_secret(
                &ProviderModelUpdate {
                    google: None,
                    provider: "new-provider".into(),
                    base_url: "https://example.invalid/v1".into(),
                    api_key: ProviderApiKeyUpdate::Environment("NEW_PROVIDER_API_KEY".into()),
                    model: "coder/v1".into(),
                    require_new_provider: true,
                    select_saved_model: true,
                    original_model: None,
                    protocol_override: None,
                },
                None,
            )
            .unwrap();
        let raw = std::fs::read_to_string(&config_path).unwrap();
        assert!(raw.contains("${NEW_PROVIDER_API_KEY}"));
        assert!(!raw.contains("protocol ="));
        let config: CaConfig = toml::from_str(&raw).unwrap();
        assert_eq!(config.providers["new-provider"].models, ["coder/v1"]);
        assert_eq!(config.defaults.provider, "new-provider");
        assert_eq!(config.defaults.model, "coder/v1");
    }

    #[test]
    fn selecting_a_discovered_model_persists_fallback_and_default_atomically() {
        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join("config.toml");
        std::fs::write(
            &config_path,
            r#"# keep selection comment
[providers.mimo]
base_url = "https://example.invalid/v1"
api_key = "${MIMO_KEY}"
models = ["mimo-v2.5"]

[defaults]
provider = "mimo"
model = "mimo-v2.5"
"#,
        )
        .unwrap();
        let store = ConfigStore::new(&config_path);

        store.select_model("mimo", "mimo-v2.5-pro").unwrap();
        store.select_model("mimo", "mimo-v2.5-pro").unwrap();

        let raw = std::fs::read_to_string(&config_path).unwrap();
        assert!(raw.contains("# keep selection comment"));
        assert!(raw.contains("${MIMO_KEY}"));
        let config: CaConfig = toml::from_str(&raw).unwrap();
        assert_eq!(
            config.providers["mimo"].models,
            ["mimo-v2.5", "mimo-v2.5-pro"]
        );
        assert_eq!(config.defaults.provider, "mimo");
        assert_eq!(config.defaults.model, "mimo-v2.5-pro");
    }

    #[test]
    fn selecting_review_model_preserves_default_credentials_and_comments() {
        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join("config.toml");
        std::fs::write(
            &config_path,
            r#"# keep this comment
[defaults]
provider = "safe"
model = "chat"

[providers.safe]
base_url = "https://safe.invalid/v1"
api_key = "${SAFE_KEY}"
models = ["chat"]
"#,
        )
        .unwrap();
        let store = ConfigStore::new(&config_path);

        store.select_review_model("safe", "reviewer").unwrap();

        let raw = std::fs::read_to_string(&config_path).unwrap();
        assert!(raw.contains("# keep this comment"));
        assert!(raw.contains("api_key = \"${SAFE_KEY}\""));
        let config: CaConfig = toml::from_str(&raw).unwrap();
        assert_eq!(config.defaults.provider, "safe");
        assert_eq!(config.defaults.model, "chat");
        assert_eq!(config.review.provider.as_deref(), Some("safe"));
        assert_eq!(config.review.model.as_deref(), Some("reviewer"));
        assert!(config.providers["safe"].models.contains(&"reviewer".into()));
    }

    #[test]
    fn absent_config_is_created_below_the_injected_root() {
        let temp = tempfile::tempdir().unwrap();
        let paths = RuntimePaths::from_home(temp.path());
        let store = ConfigStore::new(paths.config_file.clone());

        let loaded = store.load_with_env(|_| None).unwrap();

        assert!(paths.config_file.is_file());
        assert!(loaded.providers.contains_key("deepseek"));
        let raw = std::fs::read_to_string(paths.config_file).unwrap();
        assert!(raw.contains("${DEEPSEEK_API_KEY}"));
        assert!(raw.contains("${MIMO_API_KEY}"));
        assert!(!raw.contains("${XIAOMI_API_KEY}"));
        assert_eq!(
            loaded.providers["deepseek"].models,
            ["deepseek-v4-flash", "deepseek-v4-pro"]
        );
        assert!(loaded.providers["mimo"]
            .models
            .contains(&"mimo-v2.5-pro".to_string()));
    }

    #[test]
    fn mcp_config_is_backward_compatible_and_resolves_env_without_rewriting_secret() {
        let temp = tempfile::tempdir().unwrap();
        let paths = RuntimePaths::from_home(temp.path());
        let store = ConfigStore::new(paths.config_file.clone());
        std::fs::create_dir_all(paths.config_file.parent().unwrap()).unwrap();
        std::fs::write(
            &paths.config_file,
            r#"[mcp.servers.local]
command = "/usr/bin/env"
args = ["python3", "server.py"]
env = { TOKEN = "${MCP_TEST_TOKEN}" }
network = false

[mcp.servers.remote]
transport = "streamable_http"
url = "https://mcp.example.invalid/rpc"
headers = { Authorization = "${MCP_HTTP_TOKEN}" }
network = true

[mcp.servers.oauth]
transport = "streamable_http"
url = "https://oauth-mcp.example.invalid/rpc"
network = true

[mcp.servers.oauth.oauth]
enabled = true
issuer = "https://login.example.invalid/tenant"
client_id = "ca-client"
client_secret = "${MCP_CLIENT_SECRET}"
scopes = ["tools.read"]
"#,
        )
        .unwrap();

        let config = store
            .load_with_env(|name| (name == "MCP_TEST_TOKEN").then(|| "resolved-secret".into()))
            .unwrap();
        assert_eq!(config.mcp.servers["local"].env["TOKEN"], "resolved-secret");
        assert_eq!(config.mcp.servers["local"].timeout_secs, 30);
        assert!(config.mcp.servers["local"].enabled);
        assert_eq!(
            config.mcp.servers["remote"].transport,
            McpTransport::StreamableHttp
        );
        assert_eq!(
            config.mcp.servers["remote"].headers["Authorization"],
            "${MCP_HTTP_TOKEN}"
        );
        assert_eq!(
            config.mcp.servers["oauth"].oauth.client_secret.as_deref(),
            Some("${MCP_CLIENT_SECRET}")
        );
        let debug = format!("{:?}", config.mcp.servers["oauth"]);
        assert!(!debug.contains("MCP_CLIENT_SECRET"));
        assert!(!debug.contains("login.example.invalid"));

        let trust = ProjectMcpTrust {
            workspace: PathBuf::from("/tmp/workspace"),
            sha256: "abc123".into(),
            network: false,
        };
        store.trust_project_mcp(&trust).unwrap();
        let raw = std::fs::read_to_string(paths.config_file).unwrap();
        assert!(raw.contains("${MCP_TEST_TOKEN}"));
        assert!(raw.contains("${MCP_HTTP_TOKEN}"));
        assert!(raw.contains("${MCP_CLIENT_SECRET}"));
        assert!(!raw.contains("resolved-secret"));
        let reparsed: CaConfig = toml::from_str(&raw).unwrap();
        assert_eq!(reparsed.mcp.trusted_projects, vec![trust]);
    }

    #[test]
    fn old_config_without_mcp_defaults_to_an_empty_registry() {
        let config: CaConfig = toml::from_str("[defaults]\nprovider = 'p'\nmodel = 'm'\n").unwrap();
        assert!(config.mcp.servers.is_empty());
        assert!(config.mcp.trusted_projects.is_empty());
    }

    #[test]
    fn omitted_agent_table_uses_semantic_defaults_without_rewriting() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        let raw = "[defaults]\nprovider = 'p'\nmodel = 'm'\n";
        std::fs::write(&path, raw).unwrap();

        let loaded = ConfigStore::new(path.clone())
            .load_with_env(|_| None)
            .unwrap();

        assert_semantic_agent_defaults(&loaded.agent);
        assert_eq!(std::fs::read_to_string(path).unwrap(), raw);
    }

    #[test]
    fn partial_agent_table_defaults_omitted_fields_and_preserves_explicit_values() {
        let config: CaConfig = toml::from_str(
            r#"
[defaults]
provider = "p"
model = "m"

[agent]
max_turns = 7
approval_mode = "ask"
subagent_max_depth = 1
"#,
        )
        .unwrap();

        assert_eq!(config.agent.max_turns, 7);
        assert_eq!(config.agent.approval_mode, "ask");
        assert_eq!(config.agent.subagent_max_depth, 1);
        assert_eq!(config.agent.command_timeout_secs, 30);
        assert_eq!(config.agent.sandbox_mode, "required");
        assert_eq!(config.agent.subagent_concurrency, 3);
        assert_eq!(config.agent.subagent_children_per_turn, 8);
        assert_eq!(config.agent.subagent_max_turns, 30);
        assert_eq!(config.agent.subagent_runtime_secs, 30 * 60);
        assert_eq!(config.agent.subagent_output_bytes, 256 * 1024);
        assert!(config.agent.modes.is_empty());
    }

    #[test]
    fn legacy_allowlist_approval_mode_migrates_exactly_and_preserves_toml() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        let raw = r#"# keep migration header
[defaults]
provider = "p"
model = "m"

[agent]
approval_mode = "allowlist" # keep approval comment
command_timeout_secs = 41
"#;
        std::fs::write(&path, raw).unwrap();

        let config = ConfigStore::new(&path).load_with_env(|_| None).unwrap();

        assert_eq!(config.agent.approval_mode, "auto-review");
        let migrated = std::fs::read_to_string(path).unwrap();
        assert!(migrated.contains("# keep migration header"));
        assert!(migrated.contains("approval_mode = \"auto-review\" # keep approval comment"));
        assert!(migrated.contains("command_timeout_secs = 41"));
        assert!(!migrated.contains("approval_mode = \"allowlist\""));

        let named_path = temp.path().join("named.toml");
        std::fs::write(
            &named_path,
            "[agent]\napproval_mode = \"allowlist\" # named\n",
        )
        .unwrap();
        let named = ConfigStore::for_profile(
            &named_path,
            crate::runtime::ActiveProfile::Named("work".into()),
        );
        assert_eq!(
            named.load_with_env(|_| None).unwrap().agent.approval_mode,
            "auto-review"
        );
        assert!(std::fs::read_to_string(named_path)
            .unwrap()
            .contains("approval_mode = \"auto-review\" # named"));
    }

    #[test]
    fn strict_ask_and_process_only_legacy_approval_modes_are_preserved_safely() {
        let temp = tempfile::tempdir().unwrap();
        let strict_path = temp.path().join("strict.toml");
        let strict = "[agent]\napproval_mode = \"ask\"\n";
        std::fs::write(&strict_path, strict).unwrap();
        let loaded = ConfigStore::new(&strict_path)
            .load_with_env(|_| None)
            .unwrap();
        assert_eq!(loaded.agent.approval_mode, "ask");
        assert_eq!(std::fs::read_to_string(strict_path).unwrap(), strict);

        let bare_path = temp.path().join("bare.toml");
        let legacy = "[agent]\napproval_mode = \"allowlist\"\n";
        std::fs::write(&bare_path, legacy).unwrap();
        let bare = ConfigStore::for_profile(&bare_path, crate::runtime::ActiveProfile::Bare);
        let loaded = bare.load_with_env(|_| None).unwrap();
        assert_eq!(loaded.agent.approval_mode, "auto-review");
        assert_eq!(std::fs::read_to_string(&bare_path).unwrap(), legacy);
        assert!(bare.update_approval_mode("auto-review").is_err());
        assert_eq!(std::fs::read_to_string(bare_path).unwrap(), legacy);

        let invalid_path = temp.path().join("invalid.toml");
        let invalid = "[agent]\napproval_mode = \"allowlist\"\nmax_turns = \"bad\"\n";
        std::fs::write(&invalid_path, invalid).unwrap();
        assert!(ConfigStore::new(&invalid_path)
            .load_with_env(|_| None)
            .is_err());
        assert_eq!(std::fs::read_to_string(invalid_path).unwrap(), invalid);
    }

    #[test]
    fn approval_mode_update_is_canonical_and_preserves_comments() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        std::fs::write(
            &path,
            "[agent]\napproval_mode = \"auto-review\" # keep me\n",
        )
        .unwrap();
        let store = ConfigStore::new(&path);

        store.update_approval_mode("ask").unwrap();
        assert_eq!(store.load().unwrap().agent.approval_mode, "ask");
        assert!(std::fs::read_to_string(&path)
            .unwrap()
            .contains("approval_mode = \"ask\" # keep me"));
        assert!(store.update_approval_mode("allowlist").is_err());
    }

    #[test]
    fn rust_agent_default_matches_serialized_production_defaults() {
        let rust_default = AgentConfig::default();
        let serialized = toml::to_string_pretty(&CaConfig::default_config()).unwrap();
        let production_default: CaConfig = toml::from_str(&serialized).unwrap();
        let production_default = production_default.agent;

        assert_semantic_agent_defaults(&rust_default);
        assert_eq!(rust_default.max_turns, production_default.max_turns);
        assert_eq!(rust_default.approval_mode, production_default.approval_mode);
        assert_eq!(
            rust_default.command_timeout_secs,
            production_default.command_timeout_secs
        );
        assert_eq!(rust_default.sandbox_mode, production_default.sandbox_mode);
        assert_eq!(
            rust_default.subagent_concurrency,
            production_default.subagent_concurrency
        );
        assert_eq!(
            rust_default.subagent_max_depth,
            production_default.subagent_max_depth
        );
        assert_eq!(
            rust_default.subagent_children_per_turn,
            production_default.subagent_children_per_turn
        );
        assert_eq!(
            rust_default.subagent_max_turns,
            production_default.subagent_max_turns
        );
        assert_eq!(
            rust_default.subagent_runtime_secs,
            production_default.subagent_runtime_secs
        );
        assert_eq!(
            rust_default.subagent_output_bytes,
            production_default.subagent_output_bytes
        );
        assert_eq!(rust_default.modes.len(), production_default.modes.len());
    }

    #[test]
    fn explicit_model_profile_overrides_legacy_context_without_name_inference() {
        let config: CaConfig = toml::from_str(
            r#"
[model_context.coder]
context_window_tokens = 1000
compact_at_tokens = 700

[model_profiles."local/coder"]
context_window_tokens = 4000
compact_at_tokens = 3000
supports_tools = true
input_modalities = ["text", "image"]
reasoning_efforts = ["low", "high"]
default_reasoning_effort = "high"
reasoning_request_format = "reasoning_object"
reasoning_stream_field = "analysis"
"#,
        )
        .unwrap();

        assert_eq!(
            config.context_for_model("local", "coder"),
            Some(ModelContextConfig {
                context_window_tokens: Some(4000),
                compact_at_tokens: Some(3000),
            })
        );
        let profile = config.model_profile("local", "coder").unwrap();
        assert_eq!(profile.image_input_support(), Some(true));
        assert_eq!(
            profile.input_modalities,
            vec![ModelInputModality::Text, ModelInputModality::Image]
        );
        assert_eq!(
            profile.reasoning_efforts,
            vec![ReasoningEffort::Low, ReasoningEffort::High]
        );
        assert_eq!(
            profile.default_reasoning_effort,
            Some(ReasoningEffort::High)
        );
        assert!(config.model_profile("other", "coder").is_none());
        assert_eq!(
            config.context_for_model("other", "coder"),
            Some(ModelContextConfig {
                context_window_tokens: Some(1000),
                compact_at_tokens: Some(700),
            })
        );
    }

    #[test]
    fn hook_config_parses_event_command_timeout_and_fail_closed() {
        let toml = r#"
[[hooks]]
event = "pre_tool_use"
command = "/usr/local/bin/check.sh"
timeout_secs = 15
fail_closed = true

[[hooks]]
event = "post_tool_use"
command = "python3 log.py"
"#;
        let config: CaConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.hooks.len(), 2);
        assert_eq!(config.hooks[0].event, "pre_tool_use");
        assert_eq!(config.hooks[0].command, "/usr/local/bin/check.sh");
        assert_eq!(config.hooks[0].timeout_secs, 15);
        assert!(config.hooks[0].fail_closed);
        assert!(config.hooks[0].enabled);
        assert_eq!(config.hooks[1].event, "post_tool_use");
        assert_eq!(config.hooks[1].command, "python3 log.py");
        assert_eq!(config.hooks[1].timeout_secs, 30); // default
        assert!(!config.hooks[1].fail_closed); // default
        assert!(config.hooks[1].enabled);
    }

    #[test]
    fn hook_config_validation_rejects_unknown_events_and_invalid_timeouts() {
        for (event, timeout, expected) in [
            ("unknown", 5, "unsupported event"),
            ("pre_tool_use", 0, "between 1 and 600"),
            ("pre_tool_use", 601, "between 1 and 600"),
        ] {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().join("config.toml");
            std::fs::write(
                &path,
                format!(
                    "[[hooks]]\nevent = \"{event}\"\ncommand = \"true\"\ntimeout_secs = {timeout}\n"
                ),
            )
            .unwrap();
            let error = ConfigStore::new(path)
                .hook_configs()
                .unwrap_err()
                .to_string();
            assert!(error.contains(expected), "unexpected error: {error}");
        }
    }

    #[test]
    fn lifecycle_hook_events_are_opt_in_and_validate() {
        assert!(CaConfig::default().hooks.is_empty());
        for event in [
            "pre_compact",
            "post_compact",
            "subagent_start",
            "subagent_stop",
        ] {
            let hook = HookConfig {
                event: event.into(),
                command: "true".into(),
                enabled: true,
                timeout_secs: 5,
                fail_closed: false,
            };
            validate_hooks(&[hook]).unwrap();
        }
    }

    #[test]
    fn hook_toggle_is_exact_and_preserves_comments_and_unrelated_toml() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        let original = r#"# keep top comment
[[hooks]]
event = "pre_tool_use" # keep event comment
command = "printf secret-command"
enabled = true # keep enabled comment
timeout_secs = 9

[custom]
untouched = "yes"
"#;
        std::fs::write(&path, original).unwrap();
        let store = ConfigStore::new(&path);
        store.update_hook_enabled(0, "pre_tool_use", false).unwrap();
        let updated = std::fs::read_to_string(&path).unwrap();
        assert!(updated.contains("# keep top comment"));
        assert!(updated.contains("event = \"pre_tool_use\" # keep event comment"));
        assert!(updated.contains("enabled = false # keep enabled comment"));
        assert!(updated.contains("command = \"printf secret-command\""));
        assert!(updated.contains("untouched = \"yes\""));

        let before_rejected = updated.clone();
        let error = store
            .update_hook_enabled(0, "post_tool_use", true)
            .unwrap_err()
            .to_string();
        assert!(error.contains("changed"));
        assert_eq!(std::fs::read_to_string(path).unwrap(), before_rejected);
    }

    #[test]
    fn bare_hook_toggle_is_view_only_and_does_not_write() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        let original = "[[hooks]]\nevent = \"pre_tool_use\"\ncommand = \"true\"\nenabled = true\n";
        std::fs::write(&path, original).unwrap();
        let store = ConfigStore::for_profile(&path, crate::runtime::ActiveProfile::Bare);
        let error = store
            .update_hook_enabled(0, "pre_tool_use", false)
            .unwrap_err()
            .to_string();
        assert!(error.contains("bare mode"));
        assert_eq!(std::fs::read_to_string(path).unwrap(), original);
    }

    #[test]
    fn trust_workspace_persists_path_and_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.toml");
        std::fs::write(
            &config_path,
            "[defaults]\nprovider = \"p\"\nmodel = \"m\"\n",
        )
        .unwrap();
        let store = ConfigStore::new(config_path.clone());
        store.trust_workspace("/my/project").unwrap();
        let config = store.load().unwrap();
        assert_eq!(config.trusted_workspaces, ["/my/project"]);

        // Idempotent: calling again doesn't duplicate
        store.trust_workspace("/my/project").unwrap();
        let config = store.load().unwrap();
        assert_eq!(config.trusted_workspaces, ["/my/project"]);

        // Second workspace appends
        store.trust_workspace("/other/project").unwrap();
        let config = store.load().unwrap();
        assert_eq!(config.trusted_workspaces, ["/my/project", "/other/project"]);
    }

    #[test]
    fn optional_skill_roots_and_disabled_ids_are_backward_compatible() {
        let empty: CaConfig = toml::from_str("").unwrap();
        assert!(empty.skills.compatibility_roots.is_empty());
        assert!(empty.skills.disabled.is_empty());

        let configured: CaConfig = toml::from_str(
            r#"
[skills]
compatibility_roots = ["legacy-skills"]
disabled = ["unsafe-skill"]
"#,
        )
        .unwrap();
        assert_eq!(
            configured.skills.compatibility_roots,
            [PathBuf::from("legacy-skills")]
        );
        assert_eq!(configured.skills.disabled, ["unsafe-skill"]);
    }

    #[test]
    fn named_profile_loads_defaults_without_writing_until_first_mutation() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("profiles/work.toml");
        let store =
            ConfigStore::for_profile(&path, crate::runtime::ActiveProfile::Named("work".into()));

        let loaded = store.load_with_env(|_| None).unwrap();
        assert!(!loaded.providers.is_empty());
        assert!(!path.exists());

        store
            .append_approval_rule(&ApprovalRule::AllowTool("read_file".into()))
            .unwrap();
        assert!(path.is_file());
        assert_eq!(
            store.load_with_env(|_| None).unwrap().approval_rules.len(),
            1
        );
    }

    #[test]
    fn named_profile_failed_first_keychain_save_rolls_back_new_file() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("profiles/work.toml");
        let secrets = Arc::new(FakeProviderSecrets::default());
        secrets
            .fail_store
            .store(true, std::sync::atomic::Ordering::Relaxed);
        let store = ConfigStore::with_profile_secret_store(
            &path,
            crate::runtime::ActiveProfile::Named("work".into()),
            secrets,
        );
        let secret = SecretValue::from_text("never-persist-this");

        assert!(store
            .upsert_provider_model_with_secret(&direct_key_update("custom"), Some(&secret))
            .is_err());
        assert!(!path.exists());
    }

    #[test]
    fn token_plan_stage_keeps_secret_out_of_config_and_rolls_back_billing_atomically() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        let original = "[providers.openai]\nbase_url = \"https://api.openai.com/v1\"\napi_key = \"${OPENAI_API_KEY}\"\nmodels = [\"gpt-5.6-sol\"]\n\n[defaults]\nprovider = \"openai\"\nmodel = \"gpt-5.6-sol\"\n";
        std::fs::write(&path, original).unwrap();
        let secrets = Arc::new(FakeProviderSecrets::default());
        let store = ConfigStore::with_secret_store(&path, secrets.clone());
        let secret = SecretValue::from_text("tp-never-write-to-config");
        let provider = ProviderConfig {
            google: None,
            tool_schema_policy: Default::default(),
            base_url: crate::pricing::MimoRegion::Singapore
                .token_plan_url()
                .into(),
            api_key: String::new(),
            protocol: crate::agent::ProviderProtocol::default(),
            prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
            models: vec!["mimo-v2.5-pro".into(), "mimo-v2.5".into()],
            timeout: ProviderTimeoutConfig::default(),
        };
        let billing = crate::pricing::ProviderBillingConfig::mimo_token_plan(
            crate::pricing::MimoRegion::Singapore,
            crate::pricing::MimoPlanTier::Lite,
            crate::pricing::MimoPlanTerm::Monthly,
            "5".parse().unwrap(),
            true,
        )
        .unwrap();

        let rollback = store
            .stage_authenticated_provider_with_billing(
                "mimo",
                &provider,
                "mimo-v2.5-pro",
                Some(&secret),
                Some(&billing),
            )
            .unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(!raw.contains("tp-never-write-to-config"));
        assert!(raw.contains("credential://ca/provider/mimo"));
        assert!(raw.contains("provider_billing"));
        let loaded = store.load().unwrap();
        assert_eq!(loaded.providers["mimo"].api_key, "tp-never-write-to-config");
        assert_eq!(loaded.provider_billing["mimo"], billing);

        store.rollback_authenticated_provider(rollback).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        assert!(secrets.values.lock().unwrap().is_empty());
    }

    #[test]
    fn billing_only_update_preserves_provider_credential_models_and_defaults() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        let original = concat!(
            "[providers.mimo]\n",
            "base_url = \"https://token-plan-sgp.xiaomimimo.com/v1\"\n",
            "api_key = \"credential://ca/provider/mimo\"\n",
            "protocol = \"chat_completions\"\n",
            "models = [\"mimo-v2.5-pro\"]\n\n",
            "[providers.openai-codex]\n",
            "base_url = \"https://chatgpt.com/backend-api/codex\"\n",
            "api_key = \"\"\n",
            "protocol = \"openai_responses\"\n",
            "models = [\"gpt-5.6-sol\"]\n\n",
            "[defaults]\n",
            "provider = \"openai-codex\"\n",
            "model = \"gpt-5.6-sol\"\n",
        );
        std::fs::write(&path, original).unwrap();
        let secrets = Arc::new(FakeProviderSecrets::default());
        secrets
            .store("mimo", b"tp-secret-must-remain-external")
            .unwrap();
        let store = ConfigStore::with_secret_store(&path, secrets.clone());
        let before = store.provider_management_snapshot().unwrap();
        let billing = crate::pricing::ProviderBillingConfig::mimo_token_plan(
            crate::pricing::MimoRegion::Singapore,
            crate::pricing::MimoPlanTier::Pro,
            crate::pricing::MimoPlanTerm::Annual,
            "528".parse().unwrap(),
            false,
        )
        .unwrap();

        store.update_provider_billing("mimo", &billing).unwrap();

        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(!raw.contains("tp-secret-must-remain-external"));
        assert!(raw.contains("credential://ca/provider/mimo"));
        let after = store.provider_management_snapshot().unwrap();
        assert_eq!(after, before);
        let loaded = store.load().unwrap();
        assert_eq!(loaded.provider_billing["mimo"], billing);
        assert_eq!(loaded.defaults.provider, "openai-codex");
        assert_eq!(loaded.defaults.model, "gpt-5.6-sol");
        assert_eq!(loaded.providers["mimo"].models, ["mimo-v2.5-pro"]);
        assert_eq!(
            secrets.load("mimo").unwrap().unwrap(),
            b"tp-secret-must-remain-external"
        );
    }

    #[test]
    fn named_profiles_namespace_keychain_accounts_and_references() {
        let temp = tempfile::tempdir().unwrap();
        let secrets = Arc::new(FakeProviderSecrets::default());
        for profile in ["work", "personal"] {
            let store = ConfigStore::with_profile_secret_store(
                temp.path().join(format!("{profile}.toml")),
                crate::runtime::ActiveProfile::Named(profile.into()),
                secrets.clone(),
            );
            store
                .upsert_provider_model_with_secret(
                    &direct_key_update("custom"),
                    Some(&SecretValue::from_text(profile)),
                )
                .unwrap();
            let raw = std::fs::read_to_string(store.path()).unwrap();
            assert!(raw.contains(&format!(
                "credential://ca/profile/{profile}/provider/custom"
            )));
        }
        let values = secrets.values.lock().unwrap();
        assert_eq!(values["profile/work/provider/custom"], b"work");
        assert_eq!(values["profile/personal/provider/custom"], b"personal");
        assert!(!values.contains_key("custom"));
    }

    #[test]
    fn deleting_provider_from_one_named_profile_keeps_other_profile_secret() {
        let temp = tempfile::tempdir().unwrap();
        let secrets = Arc::new(FakeProviderSecrets::default());
        let make_store = |profile: &str| {
            ConfigStore::with_profile_secret_store(
                temp.path().join(format!("{profile}.toml")),
                crate::runtime::ActiveProfile::Named(profile.into()),
                secrets.clone(),
            )
        };
        let work = make_store("work");
        let personal = make_store("personal");
        for (store, value) in [(&work, "work-key"), (&personal, "personal-key")] {
            store
                .upsert_provider_model_with_secret(
                    &direct_key_update("custom"),
                    Some(&SecretValue::from_text(value)),
                )
                .unwrap();
        }
        work.delete_provider(&ProviderDeleteUpdate {
            provider: "custom".into(),
            replacement: Some(ModelSelection {
                provider: "mimo".into(),
                model: "mimo-v2.5".into(),
            }),
        })
        .unwrap();

        {
            let values = secrets.values.lock().unwrap();
            assert!(!values.contains_key("profile/work/provider/custom"));
            assert_eq!(values["profile/personal/provider/custom"], b"personal-key");
        }
        assert!(personal.load().unwrap().providers.contains_key("custom"));
    }

    #[test]
    fn bare_reads_default_config_but_rejects_all_persistent_writes() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        std::fs::write(
            &path,
            "[defaults]\nprovider = \"mimo\"\nmodel = \"mimo-v2.5\"\n",
        )
        .unwrap();
        let original = std::fs::read_to_string(&path).unwrap();
        let store = ConfigStore::for_profile(&path, crate::runtime::ActiveProfile::Bare);

        assert_eq!(
            store.load_with_env(|_| None).unwrap().defaults.provider,
            "mimo"
        );
        let error = store
            .append_approval_rule(&ApprovalRule::AllowTool("read_file".into()))
            .unwrap_err()
            .to_string();
        assert!(error.contains("unavailable in bare mode"));
        assert_eq!(std::fs::read_to_string(path).unwrap(), original);
    }

    #[test]
    fn sandbox_mode_update_is_strict_and_preserves_unrelated_toml() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        std::fs::write(
            &path,
            r#"# keep top comment
[agent]
sandbox_mode = "required" # keep sandbox comment
command_timeout_secs = 91

[custom]
untouched = "yes"
"#,
        )
        .unwrap();
        let store = ConfigStore::new(&path);

        store
            .update_sandbox_mode(crate::sandbox::SandboxMode::PolicyOnly)
            .unwrap();

        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains("# keep top comment"));
        assert!(raw.contains("sandbox_mode = \"policy-only\" # keep sandbox comment"));
        assert!(raw.contains("command_timeout_secs = 91"));
        assert!(raw.contains("[custom]\nuntouched = \"yes\""));
        assert_eq!(store.load().unwrap().agent.sandbox_mode, "policy-only");
    }

    #[test]
    fn bare_sandbox_mode_update_is_rejected_without_writing() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        let original = "[agent]\nsandbox_mode = \"required\"\n";
        std::fs::write(&path, original).unwrap();
        let store = ConfigStore::for_profile(&path, crate::runtime::ActiveProfile::Bare);

        let error = store
            .update_sandbox_mode(crate::sandbox::SandboxMode::Off)
            .unwrap_err()
            .to_string();

        assert!(error.contains("unavailable in bare mode"));
        assert_eq!(std::fs::read_to_string(path).unwrap(), original);
    }

    #[test]
    fn context_limit_resolution_inherits_each_field_by_exact_precedence() {
        let config: CaConfig = toml::from_str(
            r#"
[model_context.coder]
context_window_tokens = 1000
compact_at_tokens = 700

[model_context."local/coder"]
compact_at_tokens = 1400

[model_profiles."local/coder"]
context_window_tokens = 4000
supports_tools = true
"#,
        )
        .unwrap();

        assert_eq!(
            config.context_for_model("local", "coder"),
            Some(ModelContextConfig {
                context_window_tokens: Some(4000),
                compact_at_tokens: Some(1400),
            })
        );
        assert_eq!(
            config.context_limit_fallback("local", "coder", true),
            ModelContextConfig {
                context_window_tokens: Some(1000),
                compact_at_tokens: Some(1400),
            }
        );
    }

    #[test]
    fn context_limit_profile_update_preserves_capabilities_comments_and_unrelated_toml() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        std::fs::write(
            &path,
            r#"# keep top
[providers.local]
base_url = "http://localhost:11434/v1"
api_key = ""
models = ["coder"]

[model_profiles."local/coder"]
context_window_tokens = 4000 # replace only this value
compact_at_tokens = 3000
supports_tools = true
reasoning_efforts = ["low", "high"]

[custom]
keep = "yes"
"#,
        )
        .unwrap();
        let store = ConfigStore::new(&path);

        store
            .update_context_limits(&ContextLimitUpdate {
                provider: "local".into(),
                model: "coder".into(),
                context_window_tokens: Some(8000),
                compact_at_tokens: None,
            })
            .unwrap();

        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains("# keep top"));
        assert!(raw.contains("context_window_tokens = 8000 # replace only this value"));
        assert!(!raw.contains("compact_at_tokens = 3000"));
        assert!(raw.contains("supports_tools = true"));
        assert!(raw.contains("reasoning_efforts = [\"low\", \"high\"]"));
        assert!(raw.contains("[custom]\nkeep = \"yes\""));
        let loaded = store.load().unwrap();
        assert_eq!(
            loaded.context_for_model("local", "coder"),
            Some(ModelContextConfig {
                context_window_tokens: Some(8000),
                compact_at_tokens: None,
            })
        );
    }

    #[test]
    fn context_limit_update_uses_exact_context_table_and_validates_before_write() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        std::fs::write(
            &path,
            r#"[providers.local]
base_url = "http://localhost:11434/v1"
api_key = ""
models = ["coder"]

[model_context.coder]
context_window_tokens = 16000
compact_at_tokens = 12000
"#,
        )
        .unwrap();
        let store = ConfigStore::new(&path);
        let original = std::fs::read_to_string(&path).unwrap();

        let error = store
            .update_context_limits(&ContextLimitUpdate {
                provider: "local".into(),
                model: "coder".into(),
                context_window_tokens: Some(4096),
                compact_at_tokens: Some(4096),
            })
            .unwrap_err()
            .to_string();
        assert!(error.contains("below"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);

        store
            .update_context_limits(&ContextLimitUpdate {
                provider: "local".into(),
                model: "coder".into(),
                context_window_tokens: Some(32000),
                compact_at_tokens: None,
            })
            .unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains("[model_context.\"local/coder\"]"));
        assert!(raw.contains("context_window_tokens = 32000"));
        assert!(raw.contains("[model_context.coder]"));
        assert!(raw.contains("compact_at_tokens = 12000"));
        assert_eq!(
            store.load().unwrap().context_for_model("local", "coder"),
            Some(ModelContextConfig {
                context_window_tokens: Some(32000),
                compact_at_tokens: Some(12000),
            })
        );
    }

    #[test]
    fn context_limit_rejects_missing_window_and_bare_writes() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        std::fs::write(
            &path,
            r#"[providers.local]
base_url = "http://localhost:11434/v1"
api_key = ""
models = ["coder"]
"#,
        )
        .unwrap();
        let store = ConfigStore::new(&path);
        let update = ContextLimitUpdate {
            provider: "local".into(),
            model: "coder".into(),
            context_window_tokens: None,
            compact_at_tokens: Some(1000),
        };
        assert!(store
            .update_context_limits(&update)
            .unwrap_err()
            .to_string()
            .contains("context window"));

        let original = std::fs::read_to_string(&path).unwrap();
        let bare = ConfigStore::for_profile(&path, crate::runtime::ActiveProfile::Bare);
        let error = bare
            .update_context_limits(&ContextLimitUpdate {
                context_window_tokens: Some(8000),
                compact_at_tokens: Some(6000),
                ..update
            })
            .unwrap_err()
            .to_string();
        assert!(error.contains("unavailable in bare mode"));
        assert_eq!(std::fs::read_to_string(path).unwrap(), original);
    }

    #[test]
    fn context_limit_named_profile_is_created_only_by_a_successful_first_save() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("profiles/work.toml");
        let store =
            ConfigStore::for_profile(&path, crate::runtime::ActiveProfile::Named("work".into()));
        let update = ContextLimitUpdate {
            provider: "deepseek".into(),
            model: "deepseek-v4-flash".into(),
            context_window_tokens: Some(64_000),
            compact_at_tokens: Some(48_000),
        };

        assert!(!path.exists());
        store
            .update_context_limits(&ContextLimitUpdate {
                compact_at_tokens: Some(64_000),
                ..update.clone()
            })
            .unwrap_err();
        assert!(!path.exists());

        store.update_context_limits(&update).unwrap();
        assert!(path.is_file());
        assert_eq!(
            store
                .load()
                .unwrap()
                .context_for_model("deepseek", "deepseek-v4-flash"),
            Some(ModelContextConfig {
                context_window_tokens: Some(64_000),
                compact_at_tokens: Some(48_000),
            })
        );
    }

    #[test]
    fn usage_pricing_requires_quoted_exact_decimals_and_exact_safe_keys() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        let base = r#"[providers.local]
base_url = "http://127.0.0.1:11434/v1"
api_key = ""
models = ["coder"]

[pricing."local/coder"]
input_per_million_usd = "0.15"
output_per_million_usd = "0.60"
cached_input_per_million_usd = "0.075"
"#;
        std::fs::write(&path, base).unwrap();
        let store = ConfigStore::new(&path);
        let loaded = store.load().unwrap();
        assert_eq!(
            loaded.pricing["local/coder"]
                .input_per_million_usd
                .to_string(),
            "0.15"
        );

        std::fs::write(&path, base.replace("\"0.15\"", "0.15")).unwrap();
        assert!(store.load().unwrap_err().to_string().contains("string"));

        std::fs::write(&path, base.replace("local/coder", "../coder")).unwrap();
        assert!(store.load().unwrap_err().to_string().contains("provider"));
    }

    #[test]
    fn keymap_updates_preserve_comments_validate_conflicts_and_reset_to_defaults() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        std::fs::write(
            &path,
            r#"# keep this comment
[providers.local]
base_url = "http://127.0.0.1:11434/v1"
api_key = ""
models = ["coder"]

[keymap]
cycle_reasoning = "ctrl+shift+t" # keep action comment
"#,
        )
        .unwrap();
        let store = ConfigStore::new(&path);
        assert_eq!(
            crate::keymap::Keymap::from_config(&store.load().unwrap().keymap)
                .unwrap()
                .display_binding(crate::keymap::KeyAction::CycleReasoning),
            "Ctrl+Shift+T"
        );

        store
            .update_keymap_binding(
                crate::keymap::KeyAction::HistorySearch,
                Some(&"ctrl+x ctrl+h".parse().unwrap()),
            )
            .unwrap();
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(saved.contains("# keep this comment"));
        assert!(saved.contains("# keep action comment"));
        assert!(saved.contains("history_search = \"ctrl+x ctrl+h\""));

        let original = saved.clone();
        let error = store
            .update_keymap_binding(
                crate::keymap::KeyAction::OpenEditor,
                Some(&"ctrl+shift+t".parse().unwrap()),
            )
            .unwrap_err()
            .to_string();
        assert!(error.contains("duplicate"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);

        store
            .update_keymap_binding(crate::keymap::KeyAction::CycleReasoning, None)
            .unwrap();
        let loaded = store.load().unwrap();
        assert!(!loaded.keymap.contains_key("cycle_reasoning"));
        assert_eq!(
            crate::keymap::Keymap::from_config(&loaded.keymap)
                .unwrap()
                .display_binding(crate::keymap::KeyAction::CycleReasoning),
            "Ctrl+T"
        );
    }

    #[test]
    fn ui_updates_validate_and_preserve_unrelated_toml_and_comments() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        std::fs::write(
            &path,
            r#"# keep root comment
[agent]
max_turns = 77

[ui]
theme = "default" # keep theme comment
notification = "off" # keep notification comment
status_indicators = false # keep status indicator comment
mouse = "scroll" # keep mouse comment

[ui.statusline]
fields = ["messages", "tokens", "send_hint", "history_hint", "commands_hint"]
custom_command = "printf ready"
custom_enabled = false # keep enabled comment
refresh_secs = 9
"#,
        )
        .unwrap();
        let store = ConfigStore::new(&path);
        store
            .update_ui_theme(crate::ui_settings::UiTheme::Amber)
            .unwrap();
        store
            .update_notification(crate::ui_settings::NotificationStyle::Osc9)
            .unwrap();
        store.update_status_indicators(true).unwrap();
        store
            .update_mouse(crate::ui_settings::MouseMode::Off)
            .unwrap();
        store
            .update_statusline(&crate::ui_settings::StatuslineUpdate {
                fields: vec![
                    crate::ui_settings::StatusField::Model,
                    crate::ui_settings::StatusField::Messages,
                    crate::ui_settings::StatusField::Custom,
                ],
                custom_enabled: true,
            })
            .unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains("# keep root comment"));
        assert!(raw.contains("theme = \"amber\" # keep theme comment"));
        assert!(raw.contains("notification = \"osc9\" # keep notification comment"));
        assert!(raw.contains("status_indicators = true # keep status indicator comment"));
        assert!(raw.contains("mouse = \"off\" # keep mouse comment"));
        assert!(raw.contains("custom_enabled = true # keep enabled comment"));
        assert!(raw.contains("custom_command = \"printf ready\""));
        assert!(raw.contains("refresh_secs = 9"));
        let loaded = store.load().unwrap();
        assert_eq!(loaded.agent.max_turns, 77);
        assert_eq!(loaded.ui.theme, crate::ui_settings::UiTheme::Amber);
        assert_eq!(
            loaded.ui.notification,
            crate::ui_settings::NotificationStyle::Osc9
        );
        assert_eq!(loaded.ui.mouse, crate::ui_settings::MouseMode::Off);
        assert!(loaded.ui.status_indicators);
        assert!(store.ui_theme_is_explicit().unwrap());
        assert_eq!(
            loaded.ui.statusline.fields,
            vec![
                crate::ui_settings::StatusField::Model,
                crate::ui_settings::StatusField::Messages,
                crate::ui_settings::StatusField::Custom,
            ]
        );
        assert!(loaded.ui.statusline.custom_enabled);
    }

    #[test]
    fn invalid_ui_config_fails_before_activation() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        let store = ConfigStore::new(&path);
        for raw in [
            "[ui.statusline]\nfields = [\"messages\", \"messages\"]\n",
            "[ui.statusline]\nrefresh_secs = 0\n",
            "[ui.statusline]\ncustom_enabled = true\n",
            "[ui]\ntheme = \"user-color\"\n",
            "[ui]\nnotification = \"growl\"\n",
            "[ui]\nmouse = \"clicks\"\n",
        ] {
            std::fs::write(&path, raw).unwrap();
            assert!(store.load().is_err(), "accepted invalid UI config: {raw}");
        }
    }

    #[test]
    fn ui_settings_are_named_profile_lazy_and_bare_read_only() {
        let temp = tempfile::tempdir().unwrap();
        let named_path = temp.path().join("profiles/work.toml");
        let named = ConfigStore::for_profile(
            &named_path,
            crate::runtime::ActiveProfile::Named("work".into()),
        );
        assert!(!named_path.exists());
        named
            .update_ui_theme(crate::ui_settings::UiTheme::Ocean)
            .unwrap();
        assert!(named_path.exists());
        assert_eq!(
            named.load().unwrap().ui.theme,
            crate::ui_settings::UiTheme::Ocean
        );

        let default_path = temp.path().join("config.toml");
        std::fs::write(&default_path, "[ui]\ntheme = \"amber\"\n").unwrap();
        let original = std::fs::read_to_string(&default_path).unwrap();
        let bare = ConfigStore::for_profile(&default_path, crate::runtime::ActiveProfile::Bare);
        assert_eq!(
            bare.load().unwrap().ui.theme,
            crate::ui_settings::UiTheme::Amber
        );
        let error = bare
            .update_ui_theme(crate::ui_settings::UiTheme::Mono)
            .unwrap_err()
            .to_string();
        assert!(error.contains("bare mode"));
        assert_eq!(std::fs::read_to_string(default_path).unwrap(), original);
    }

    #[test]
    fn terminal_theme_detection_defaults_to_auto_until_theme_is_explicit() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        let store = ConfigStore::new(&path);
        assert!(!store.ui_theme_is_explicit().unwrap());
        std::fs::write(&path, "[ui]\nstatus_indicators = true\n").unwrap();
        assert!(!store.ui_theme_is_explicit().unwrap());
        store
            .update_ui_theme(crate::ui_settings::UiTheme::Ocean)
            .unwrap();
        assert!(store.ui_theme_is_explicit().unwrap());
        assert!(store.load().unwrap().ui.status_indicators);
    }

    #[test]
    fn keymap_named_profile_is_lazy_and_bare_is_read_only() {
        let temp = tempfile::tempdir().unwrap();
        let named_path = temp.path().join("profiles/work.toml");
        let named = ConfigStore::for_profile(
            &named_path,
            crate::runtime::ActiveProfile::Named("work".into()),
        );
        assert!(!named_path.exists());
        named
            .update_keymap_binding(
                crate::keymap::KeyAction::HistorySearch,
                Some(&"ctrl+h".parse().unwrap()),
            )
            .unwrap();
        assert!(named_path.is_file());
        assert_eq!(
            crate::keymap::Keymap::from_config(&named.load().unwrap().keymap)
                .unwrap()
                .display_binding(crate::keymap::KeyAction::HistorySearch),
            "Ctrl+H"
        );

        let original = std::fs::read_to_string(&named_path).unwrap();
        let bare = ConfigStore::for_profile(&named_path, crate::runtime::ActiveProfile::Bare);
        assert!(bare
            .update_keymap_binding(crate::keymap::KeyAction::HistorySearch, None)
            .unwrap_err()
            .to_string()
            .contains("unavailable in bare mode"));
        assert_eq!(std::fs::read_to_string(named_path).unwrap(), original);
    }

    #[test]
    fn session_retention_defaults_disabled_and_patch_preserves_comments() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        std::fs::write(
            &path,
            r#"# keep root comment
[providers.local]
base_url = "http://127.0.0.1:11434/v1"
api_key = ""
models = ["coder"]

[sessions]
retention_days = 30 # keep age comment
"#,
        )
        .unwrap();
        let store = ConfigStore::new(&path);
        assert_eq!(
            store.load().unwrap().session_retention_settings().unwrap(),
            crate::session_retention::RetentionSettings {
                days: Some(30),
                count: None,
            }
        );

        store
            .update_session_retention(crate::session_retention::RetentionSettings {
                days: Some(45),
                count: Some(100),
            })
            .unwrap();
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(saved.contains("# keep root comment"));
        assert!(saved.contains("retention_days = 45 # keep age comment"));
        assert!(saved.contains("retention_count = 100"));

        store
            .update_session_retention(crate::session_retention::RetentionSettings {
                days: None,
                count: Some(100),
            })
            .unwrap();
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(!saved.contains("retention_days"));
        assert!(saved.contains("retention_count = 100"));
    }

    #[test]
    fn session_read_tracking_is_opt_in_and_backward_compatible() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        std::fs::write(&path, "[sessions]\n").unwrap();
        assert!(!ConfigStore::new(&path).load().unwrap().sessions.track_reads);

        std::fs::write(&path, "[sessions]\ntrack_reads = true\n").unwrap();
        assert!(ConfigStore::new(&path).load().unwrap().sessions.track_reads);
    }

    #[test]
    fn session_retention_rejects_invalid_saved_values_and_bare_writes() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        std::fs::write(&path, "[sessions]\nretention_days = 0\n").unwrap();
        assert!(ConfigStore::new(&path)
            .load()
            .unwrap_err()
            .to_string()
            .contains("positive"));

        std::fs::write(&path, "# unchanged\n[sessions]\nretention_days = 30\n").unwrap();
        let original = std::fs::read_to_string(&path).unwrap();
        let bare = ConfigStore::for_profile(&path, crate::runtime::ActiveProfile::Bare);
        let error = bare
            .update_session_retention(crate::session_retention::RetentionSettings {
                days: Some(60),
                count: None,
            })
            .unwrap_err()
            .to_string();
        assert!(error.contains("unavailable in bare mode"));
        assert_eq!(std::fs::read_to_string(path).unwrap(), original);
    }

    #[test]
    fn invalid_saved_keymap_fails_load_before_runtime_activation() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        std::fs::write(
            &path,
            r#"[keymap]
open_editor = "ctrl+k"
"#,
        )
        .unwrap();
        let error = ConfigStore::new(&path).load().unwrap_err().to_string();
        assert!(error.contains("duplicate key binding Ctrl+K"));

        std::fs::write(&path, "[keymap]\nhistory_search = \"ctrl+c\"\n").unwrap();
        let error = ConfigStore::new(&path).load().unwrap_err().to_string();
        assert!(error.contains("reserved for emergency control"));
    }

    #[test]
    fn discovery_and_local_recall_defaults_are_explicit_opt_ins() {
        let default: CaConfig = toml::from_str("").unwrap();
        assert_eq!(default.agent.tool_discovery, ToolDiscoveryMode::Eager);
        assert!(default.memory.local_hybrid.is_none());
        let configured: CaConfig = toml::from_str("[agent]\ntool_discovery = 'lazy'\n[memory.local_hybrid]\nruntime_id = 'local-runtime'\nmodel_id = 'embedding-model'\n").unwrap();
        assert_eq!(configured.agent.tool_discovery, ToolDiscoveryMode::Lazy);
        let local = configured.memory.local_hybrid.as_ref().unwrap();
        assert_eq!(local.context_size, 2048);
        assert_eq!(local.startup_timeout_secs, 30);
        assert_eq!(local.minimum_similarity_percent, 70);
        let roundtrip: CaConfig = toml::from_str(&toml::to_string(&configured).unwrap()).unwrap();
        assert_eq!(roundtrip.memory, configured.memory);
    }

    #[test]
    fn memory_defaults_on_and_explicit_false_scope_patch_preserves_unrelated_toml() {
        let decoded: CaConfig = toml::from_str("# memory absent\n").unwrap();
        assert_eq!(decoded.memory, MemoryConfig::default());
        assert!(decoded.memory.user.recall && decoded.memory.user.generation);
        assert!(decoded.memory.workspace.recall && decoded.memory.workspace.generation);

        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        std::fs::write(
            &path,
            "# keep root comment\n[agent]\nmax_turns = 9 # keep\n",
        )
        .unwrap();
        let store = ConfigStore::new(&path);
        store
            .set_memory_scope(crate::memory::MemoryScope::Workspace, true, false)
            .unwrap();
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(saved.contains("# keep root comment"));
        assert!(saved.contains("max_turns = 9 # keep"));
        let config: CaConfig = toml::from_str(&saved).unwrap();
        assert!(config.memory.workspace.recall);
        assert!(!config.memory.workspace.generation);
        assert_eq!(config.memory.user, MemoryScopeConfig::default());

        let original = saved;
        let bare = ConfigStore::for_profile(&path, crate::runtime::ActiveProfile::Bare);
        assert!(bare
            .set_memory_scope(crate::memory::MemoryScope::User, true, true)
            .unwrap_err()
            .to_string()
            .contains("bare"));
        assert_eq!(std::fs::read_to_string(path).unwrap(), original);
    }

    #[test]
    fn learning_defaults_on_and_patch_preserves_explicit_memory_false() {
        let decoded: CaConfig = toml::from_str(
            "[memory.user]\nrecall = false\ngeneration = false\n[learning.dream]\nenabled = false\n",
        )
        .unwrap();
        assert!(!decoded.memory.user.recall && !decoded.memory.user.generation);
        assert!(!decoded.learning.dream.enabled);
        assert!(decoded.learning.distill.enabled);

        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        std::fs::write(
            &path,
            "# keep\n[memory.user]\nrecall = false\ngeneration = false\n",
        )
        .unwrap();
        let store = ConfigStore::new(&path);
        store
            .set_learning_enabled(crate::learning::MaintenanceKind::Distill, false)
            .unwrap();
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(saved.contains("# keep"));
        let config: CaConfig = toml::from_str(&saved).unwrap();
        assert!(!config.memory.user.recall && !config.memory.user.generation);
        assert!(config.learning.dream.enabled);
        assert!(!config.learning.distill.enabled);
    }

    #[test]
    fn goal_defaults_are_finite_and_invalid_saved_caps_fail_load() {
        let decoded: CaConfig = toml::from_str("# goals absent\n").unwrap();
        assert_eq!(decoded.goals, crate::goal::GoalConfig::default());

        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        std::fs::write(&path, "[goals]\nmax_continuations = 0\n").unwrap();
        let error = ConfigStore::new(path).load().unwrap_err().to_string();
        assert!(error.contains("goal limits are invalid"));
    }

    fn gateway_config(account: &str) -> EmailGatewayConfig {
        EmailGatewayConfig {
            enabled: true,
            account_id: account.into(),
            address: "gateway@example.com".into(),
            credential_ref: crate::email::secrets::gateway_reference(account),
            imap: EmailTlsEndpoint {
                host: "imap.example.com".into(),
                port: 993,
                mode: EmailTlsMode::Imaps,
            },
            smtp: EmailTlsEndpoint {
                host: "smtp.example.com".into(),
                port: 587,
                mode: EmailTlsMode::StartTlsRequired,
            },
            sender_allowlist: Vec::new(),
            provider: Some("openai".into()),
            model: Some("gpt-5".into()),
            ..Default::default()
        }
    }

    #[test]
    fn gateway_defaults_disabled_and_empty_allowlist_is_fail_closed_not_invalid() {
        assert!(!EmailGatewayConfig::default().enabled);
        let gateway = gateway_config("gateway");
        assert!(gateway.validate(&BTreeMap::new()).is_ok());
        let debug = format!("{gateway:?}");
        assert!(!debug.contains("gateway@example.com"));
        assert!(!debug.contains("keychain://"));
    }

    #[test]
    fn gateway_rejects_personal_account_reuse_wrong_secret_scope_and_unsafe_tls() {
        let mut personal = BTreeMap::new();
        personal.insert(
            "gateway".into(),
            EmailAccountConfig {
                himalaya_account: "personal".into(),
                from: "me@example.com".into(),
            },
        );
        assert!(gateway_config("gateway")
            .validate(&personal)
            .unwrap_err()
            .to_string()
            .contains("dedicated"));
        let mut wrong = gateway_config("gateway");
        wrong.credential_ref = crate::provider_secrets::keychain_reference("gateway");
        assert!(wrong.validate(&BTreeMap::new()).is_err());
        let mut unsafe_endpoint = gateway_config("gateway");
        unsafe_endpoint.imap.host = "https://imap.example.com/path".into();
        assert!(unsafe_endpoint.validate(&BTreeMap::new()).is_err());
    }

    #[test]
    fn route_presets_default_validate_and_save_without_losing_comments() {
        let decoded: CaConfig = toml::from_str("# route presets absent\n").unwrap();
        assert!(decoded.route_presets.is_empty());
        for invalid in ["", "Upper", "has space", "slash/name", &"a".repeat(65)] {
            assert!(validate_route_preset_name(invalid).is_err());
        }
        for valid in ["a", "fast-route", "review_2", &"z".repeat(64)] {
            assert!(validate_route_preset_name(valid).is_ok());
        }

        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        std::fs::write(
            &path,
            "# keep root\n[providers.local]\nbase_url = 'http://localhost/v1'\napi_key = ''\nmodels = ['coder'] # keep models\n",
        )
        .unwrap();
        let store = ConfigStore::new(&path);
        let preset = RoutePreset {
            provider: "local".into(),
            model: "coder".into(),
            reasoning_effort: None,
        };
        store.update_route_preset("daily", &preset).unwrap();
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(saved.contains("# keep root"));
        assert!(saved.contains("# keep models"));
        assert_eq!(store.load().unwrap().route_presets["daily"], preset);

        let with_preset_comment =
            saved.replace("model = \"coder\"", "model = \"coder\" # keep preset model");
        std::fs::write(&path, &with_preset_comment).unwrap();
        let replacement = RoutePreset {
            provider: "local".into(),
            model: "coder".into(),
            reasoning_effort: None,
        };
        store.update_route_preset("daily", &replacement).unwrap();
        let overwritten = std::fs::read_to_string(&path).unwrap();
        assert!(overwritten.contains("# keep preset model"));

        let invalid = RoutePreset {
            provider: "local".into(),
            model: "missing".into(),
            reasoning_effort: None,
        };
        assert!(store.update_route_preset("daily", &invalid).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), overwritten);

        let original = overwritten;
        let bare = ConfigStore::for_profile(&path, crate::runtime::ActiveProfile::Bare);
        assert!(bare.update_route_preset("nope", &preset).is_err());
        assert_eq!(std::fs::read_to_string(path).unwrap(), original);
    }

    #[test]
    fn delegation_defaults_to_parent_and_saves_one_exact_configured_route() {
        let decoded: CaConfig = toml::from_str("# delegation absent\n").unwrap();
        assert!(decoded.delegation.route.is_none());
        assert_eq!(decoded.delegation.effective_mode(), DelegationMode::Same);

        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        std::fs::write(
            &path,
            "# keep root\n[providers.mimo]\nbase_url = 'https://example.invalid/v1'\napi_key = ''\nmodels = ['mimo-v2.5-pro'] # keep models\n",
        )
        .unwrap();
        let store = ConfigStore::new(&path);
        let route = DelegationRoute {
            provider: "mimo".into(),
            model: "mimo-v2.5-pro".into(),
        };

        store.update_delegation_route(Some(&route)).unwrap();
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(saved.contains("# keep root"));
        assert!(saved.contains("# keep models"));
        assert_eq!(store.load().unwrap().delegation.route, Some(route));
        assert_eq!(
            store.load().unwrap().delegation.effective_mode(),
            DelegationMode::Fixed
        );

        store.update_delegation_route(None).unwrap();
        assert!(store.load().unwrap().delegation.route.is_none());
        assert!(!std::fs::read_to_string(&path)
            .unwrap()
            .contains("[delegation"));
    }

    #[test]
    fn learned_delegation_is_explicitly_allowlisted_and_settings_owned() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        std::fs::write(
            &path,
            "# keep root\n[providers.codex]\nbase_url = 'https://example.invalid/v1'\napi_key = ''\nmodels = ['gpt-5.6-sol']\n\n[providers.mimo]\nbase_url = 'https://example.invalid/v1'\napi_key = ''\nmodels = ['mimo-v2.5-pro']\n",
        )
        .unwrap();
        let store = ConfigStore::new(&path);

        store
            .update_delegation_mode(DelegationMode::Learned)
            .unwrap();
        let learned = store.load().unwrap();
        assert_eq!(learned.delegation.effective_mode(), DelegationMode::Learned);
        assert_eq!(
            learned.delegation.allowed_routes,
            ["codex/gpt-5.6-sol", "mimo/mimo-v2.5-pro"]
        );
        assert!(learned.delegation.route.is_none());
        assert!(std::fs::read_to_string(&path)
            .unwrap()
            .contains("# keep root"));

        store.update_delegation_mode(DelegationMode::Same).unwrap();
        assert_eq!(
            store.load().unwrap().delegation.effective_mode(),
            DelegationMode::Same
        );
    }

    #[test]
    fn learned_delegation_rejects_unconfigured_allowlist_routes() {
        let raw = "[providers.mimo]\nbase_url = 'https://example.invalid/v1'\napi_key = ''\nmodels = ['mimo-v2.5']\n\n[delegation]\nmode = 'learned'\nallowed_routes = ['other/model']\n";
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        std::fs::write(&path, raw).unwrap();
        assert!(ConfigStore::new(&path).load().is_err());
    }

    #[test]
    fn delegation_rejects_partial_unknown_and_stale_routes_without_mutation() {
        for invalid in [
            "[delegation.route]\nprovider = 'mimo'\n",
            "[delegation.route]\nmodel = 'mimo-v2.5'\n",
        ] {
            assert!(toml::from_str::<CaConfig>(invalid).is_err());
        }

        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        let original = "[providers.mimo]\nbase_url = 'https://example.invalid/v1'\napi_key = ''\nmodels = ['mimo-v2.5']\n";
        std::fs::write(&path, original).unwrap();
        let store = ConfigStore::new(&path);
        let stale = DelegationRoute {
            provider: "mimo".into(),
            model: "missing".into(),
        };
        assert!(store.update_delegation_route(Some(&stale)).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);

        let bare = ConfigStore::for_profile(&path, crate::runtime::ActiveProfile::Bare);
        assert!(bare.update_delegation_route(None).is_err());
        assert_eq!(std::fs::read_to_string(path).unwrap(), original);
    }

    #[test]
    fn delegation_follows_model_rename_and_resets_when_provider_is_deleted() {
        let raw = "[providers.mimo]\nbase_url = 'https://example.invalid/v1'\napi_key = ''\nmodels = ['old']\n\n[delegation.route]\nprovider = 'mimo'\nmodel = 'old'\n";
        let renamed = provider_model_update_document(
            raw,
            &ProviderModelUpdate {
                google: None,
                provider: "mimo".into(),
                base_url: "https://example.invalid/v1".into(),
                api_key: ProviderApiKeyUpdate::Preserve,
                model: "new".into(),
                original_model: Some("old".into()),
                require_new_provider: false,
                select_saved_model: false,
                protocol_override: None,
            },
            "unused",
        )
        .unwrap();
        assert_eq!(
            toml::from_str::<CaConfig>(&renamed)
                .unwrap()
                .delegation
                .route
                .unwrap()
                .model,
            "new"
        );

        let deleted = provider_delete_document(
            &renamed,
            &ProviderDeleteUpdate {
                provider: "mimo".into(),
                replacement: None,
            },
        )
        .unwrap();
        assert!(toml::from_str::<CaConfig>(&deleted)
            .unwrap()
            .delegation
            .route
            .is_none());
    }

    #[test]
    fn skill_config_updates_are_typed_exact_and_secret_safe() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        std::fs::write(&path, "# keep skill config\n[skills]\n").unwrap();
        let store = ConfigStore::new(&path);
        let key = "workspace:test:demo.root_path";
        store
            .update_skill_config(
                key,
                &crate::skills::SkillSettingValue::String("/safe/path".into()),
            )
            .unwrap();
        store
            .update_skill_config(
                "workspace:test:demo.formats",
                &crate::skills::SkillSettingValue::Strings(vec!["compact".into(), "json".into()]),
            )
            .unwrap();
        let loaded = store.load().unwrap();
        assert_eq!(
            loaded.skills.config[key],
            crate::skills::SkillSettingValue::String("/safe/path".into())
        );
        assert!(std::fs::read_to_string(&path)
            .unwrap()
            .contains("# keep skill config"));
        let before = std::fs::read_to_string(&path).unwrap();
        assert!(store
            .update_skill_config(
                "workspace:test:demo.api_token",
                &crate::skills::SkillSettingValue::String("sk-not-a-real-secret".into()),
            )
            .is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before);

        let bare = ConfigStore::for_profile(&path, crate::runtime::ActiveProfile::Bare);
        assert!(bare
            .update_skill_config(
                key,
                &crate::skills::SkillSettingValue::String("other".into()),
            )
            .is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before);

        std::fs::write(
            &path,
            "[skills.config]\n'demo.api_token' = 'apparently-safe'\n",
        )
        .unwrap();
        assert!(ConfigStore::new(path).load().is_err());
    }

    #[test]
    fn mechanistic_mcp_defaults_off_and_accepts_only_bounded_waits() {
        let config: McpConfig = toml::from_str("").unwrap();
        assert!(!config.mechanistic.enabled);
        assert_eq!(config.mechanistic.max_wait_seconds, 900);
        assert!(config.mechanistic.automatic_preflight);
        assert!(config.mechanistic.automatic_verification);
        assert_eq!(config.mechanistic.preflight_timeout_seconds, 10);
        assert_eq!(
            config.mechanistic.full_suite,
            MechanisticFullSuitePolicy::ReleaseOrUnscoped
        );
        assert_eq!(config.mechanistic.max_repair_passes, 2);
        assert!(!config.mechanistic.allow_unisolated_evidence);
        assert!(config.mechanistic.transparent_routing);
        assert_eq!(config.mechanistic.route_action_budget, 16);
        assert_eq!(config.mechanistic.route_time_budget_seconds, 30);
        assert_eq!(config.mechanistic.capsule_bytes, 16 * 1024);
        assert_eq!(config.mechanistic.warmup_action_budget, 4);
        assert!(!config.mechanistic.remote.enabled);
        assert_eq!(config.mechanistic.remote.max_wait_seconds, 1_800);
        assert!(config.mechanistic.validate(&config.servers).is_ok());

        let parsed: McpConfig = toml::from_str(
            "[mechanistic]\nenabled = true\nserver = 'delegate'\n[mechanistic.remote]\nenabled = true\nconfig = '/private/remote.toml'\nmax_wait_seconds = 600\n[mechanistic.remote.env]\nSEEDBOX_USER = '${SEEDBOX_USER}'\n[servers.delegate]\ntransport = 'stdio'\ncommand = '/usr/bin/codex-team'\nnetwork = false\n",
        )
        .unwrap();
        assert!(parsed.mechanistic.remote.enabled);
        assert_eq!(
            parsed.mechanistic.remote.config,
            PathBuf::from("/private/remote.toml")
        );
        assert_eq!(
            parsed.mechanistic.remote.env["SEEDBOX_USER"],
            "${SEEDBOX_USER}"
        );
        assert!(!format!("{:?}", parsed.mechanistic.remote).contains("SEEDBOX_USER"));
        assert!(parsed.mechanistic.validate(&parsed.servers).is_ok());

        let invalid = MechanisticMcpConfig {
            max_wait_seconds: 29,
            ..MechanisticMcpConfig::default()
        };
        assert!(invalid.validate(&HashMap::new()).is_err());
        let invalid = MechanisticMcpConfig {
            remote: RemoteMechanisticConfig {
                enabled: true,
                config: PathBuf::from("relative.toml"),
                max_wait_seconds: 1_800,
                env: HashMap::new(),
            },
            ..MechanisticMcpConfig::default()
        };
        assert!(invalid.validate(&HashMap::new()).is_err());
        for invalid in [
            MechanisticMcpConfig {
                route_action_budget: 0,
                ..MechanisticMcpConfig::default()
            },
            MechanisticMcpConfig {
                route_time_budget_seconds: 121,
                ..MechanisticMcpConfig::default()
            },
            MechanisticMcpConfig {
                capsule_bytes: 1024,
                ..MechanisticMcpConfig::default()
            },
            MechanisticMcpConfig {
                warmup_action_budget: 9,
                ..MechanisticMcpConfig::default()
            },
        ] {
            assert!(invalid.validate(&HashMap::new()).is_err());
        }
        let invalid = MechanisticMcpConfig {
            preflight_timeout_seconds: 31,
            ..MechanisticMcpConfig::default()
        };
        assert!(invalid.validate(&HashMap::new()).is_err());
        let invalid = MechanisticMcpConfig {
            max_repair_passes: 3,
            ..MechanisticMcpConfig::default()
        };
        assert!(invalid.validate(&HashMap::new()).is_err());
    }

    #[test]
    fn mechanistic_mcp_requires_enabled_local_non_networked_stdio() {
        let mut servers = HashMap::new();
        servers.insert(
            "codex_team_delegate".to_string(),
            McpServerConfig {
                transport: McpTransport::Stdio,
                command: "/usr/bin/true".to_string(),
                args: Vec::new(),
                env: HashMap::new(),
                url: None,
                headers: HashMap::new(),
                enabled: true,
                timeout_secs: 30,
                network: false,
                oauth: McpOAuthConfig::default(),
            },
        );
        let mechanistic = MechanisticMcpConfig {
            enabled: true,
            server: "codex_team_delegate".to_string(),
            max_wait_seconds: 900,
            ..MechanisticMcpConfig::default()
        };
        assert!(mechanistic.validate(&servers).is_ok());

        let remote = MechanisticMcpConfig {
            remote: RemoteMechanisticConfig {
                enabled: true,
                config: PathBuf::from("/private/remote.toml"),
                max_wait_seconds: 1_800,
                env: HashMap::new(),
            },
            ..mechanistic.clone()
        };
        servers.get_mut("codex_team_delegate").unwrap().command = "/usr/bin/codex-team".into();
        assert!(remote.validate(&servers).is_ok());

        servers.get_mut("codex_team_delegate").unwrap().network = true;
        assert!(mechanistic.validate(&servers).is_err());
        servers.get_mut("codex_team_delegate").unwrap().network = false;
        servers.get_mut("codex_team_delegate").unwrap().transport = McpTransport::StreamableHttp;
        assert!(mechanistic.validate(&servers).is_err());
    }

    #[test]
    fn convergence_review_routes_are_additive_bounded_and_distinct() {
        let mut config = CaConfig::default();
        config.providers.insert(
            "first".into(),
            ProviderConfig {
                google: None,
                tool_schema_policy: Default::default(),
                base_url: "http://127.0.0.1:1/v1".into(),
                api_key: "test".into(),
                protocol: crate::agent::ProviderProtocol::default(),
                prompt_cache_mode: PromptCacheMode::Implicit,
                models: vec!["review-a".into()],
                timeout: ProviderTimeoutConfig::default(),
            },
        );
        config.providers.insert(
            "second".into(),
            ProviderConfig {
                google: None,
                tool_schema_policy: Default::default(),
                base_url: "http://127.0.0.1:2/v1".into(),
                api_key: "test".into(),
                protocol: crate::agent::ProviderProtocol::default(),
                prompt_cache_mode: PromptCacheMode::Implicit,
                models: vec!["review-b".into()],
                timeout: ProviderTimeoutConfig::default(),
            },
        );
        config.review.convergence.routes = vec![
            ReviewRouteConfig {
                provider: "first".into(),
                model: "review-a".into(),
            },
            ReviewRouteConfig {
                provider: "second".into(),
                model: "review-b".into(),
            },
        ];
        assert!(validate_review_config(&config).is_ok());

        config.review.convergence.routes[1] = config.review.convergence.routes[0].clone();
        assert!(validate_review_config(&config)
            .unwrap_err()
            .to_string()
            .contains("distinct"));

        config.review.convergence.routes.clear();
        assert!(validate_review_config(&config).is_ok());
        config.review.convergence.routes.push(ReviewRouteConfig {
            provider: "first".into(),
            model: "review-a".into(),
        });
        assert!(validate_review_config(&config)
            .unwrap_err()
            .to_string()
            .contains("exactly two"));
    }

    #[test]
    fn convergence_review_config_rejects_missing_models_and_impossible_budgets() {
        let mut config = CaConfig::default();
        config.providers.insert(
            "first".into(),
            ProviderConfig {
                google: None,
                tool_schema_policy: Default::default(),
                base_url: "http://127.0.0.1:1/v1".into(),
                api_key: "test".into(),
                protocol: crate::agent::ProviderProtocol::default(),
                prompt_cache_mode: PromptCacheMode::Implicit,
                models: vec!["review-a".into()],
                timeout: ProviderTimeoutConfig::default(),
            },
        );
        config.review.convergence.routes = vec![
            ReviewRouteConfig {
                provider: "first".into(),
                model: "missing".into(),
            },
            ReviewRouteConfig {
                provider: "missing".into(),
                model: "review-b".into(),
            },
        ];
        assert!(validate_review_config(&config).is_err());

        config.review.convergence.routes.clear();
        config.review.convergence.max_rounds = 4;
        assert!(validate_review_config(&config).is_err());
        config.review.convergence.max_rounds = 3;
        config.review.convergence.max_provider_requests = 1;
        assert!(validate_review_config(&config).is_err());
        config.review.convergence.max_provider_requests = 48;
        config.review.convergence.max_output_bytes = 0;
        assert!(validate_review_config(&config).is_err());
    }
    #[test]
    fn google_settings_validate_and_persist_additively_without_tokens() {
        let mut update = direct_key_update("vertex");
        update.api_key = ProviderApiKeyUpdate::Preserve;
        update.protocol_override = Some(crate::agent::ProviderProtocol::GoogleGenerateContent);
        update.google = Some(crate::google_auth::Config {
            mode: crate::google_auth::Mode::Vertex,
            project_id: Some("example-project".into()),
            location: Some("global".into()),
            quota_project_id: Some("quota-project".into()),
        });
        validate_provider_model_update(&update).unwrap();
        let output =
            provider_model_update_document("", &update, "unused-keychain-reference").unwrap();
        let config: CaConfig = toml::from_str(&output).unwrap();
        assert_eq!(config.providers["vertex"].google, update.google);
        assert_eq!(config.providers["vertex"].api_key, "");
        assert!(!output.contains("access_token"));
        validate_provider_billing(&config).unwrap();
        let mut invalid = config.clone();
        invalid
            .providers
            .get_mut("vertex")
            .unwrap()
            .google
            .as_mut()
            .unwrap()
            .project_id = None;
        assert!(validate_provider_billing(&invalid).is_err());
        let legacy:ProviderConfig=toml::from_str("base_url='https://generativelanguage.googleapis.com/v1beta/openai'\napi_key='${GOOGLE_API_KEY}'\nmodels=['gemini-existing']").unwrap();
        assert_eq!(
            legacy.protocol,
            crate::agent::ProviderProtocol::ChatCompletions
        );
        assert!(legacy.google.is_none());
    }
}
