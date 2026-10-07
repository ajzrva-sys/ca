use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AgentMode {
    Agent,
    Plan,
    Ask,
    Debug,
    Custom(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ApprovalMode {
    Allowlist,
    AutoReview,
    RunEverything,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Success,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackgroundActivityRef {
    Job(crate::jobs::JobId),
    Watch(ca_watch::WatchId),
}

#[derive(Debug, Clone)]
pub enum MechanisticUiRequest {
    Readiness,
    Recover(String),
    Cancel(String),
    Feedback {
        job_id: String,
        category: crate::mechanistic_delegation::DelegationFeedbackCategory,
        correction_confirmed: bool,
    },
}

#[derive(Debug, Clone)]
pub struct UiToast {
    pub message: String,
    pub kind: ToastKind,
    pub expires_at: std::time::Instant,
}

impl UiToast {
    const LIFETIME: std::time::Duration = std::time::Duration::from_millis(1_600);

    pub fn new(message: impl Into<String>, kind: ToastKind) -> Self {
        Self {
            message: message.into(),
            kind,
            expires_at: std::time::Instant::now() + Self::LIFETIME,
        }
    }

    pub fn is_visible(&self) -> bool {
        std::time::Instant::now() < self.expires_at
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum VimMode {
    Normal,
    Insert,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimelineMode {
    Browse,
    Rewind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolStatus {
    Pending,
    Running,
    Done,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Message {
    pub role: Role,
    pub content: String,
    pub tool_calls: Option<Vec<ToolCall>>,
    pub thinking: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub response_items: Vec<crate::agent::stream::ProviderResponseItem>,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TranscriptMessageAnnotation {
    pub message_timestamp: chrono::DateTime<chrono::Utc>,
    pub turn_id: uuid::Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_turn_duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_outcome: Option<TranscriptTurnOutcome>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptTurnOutcome {
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TranscriptAgentSummary {
    pub agent_id: crate::agent::subagent::AgentId,
    pub turn_id: uuid::Uuid,
    pub purpose: crate::agent::subagent::AgentRunPurpose,
    pub task: String,
    pub status: crate::agent::subagent::AgentStatus,
    pub summary: String,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentActivityFocusState {
    pub turn_id: uuid::Uuid,
    pub selected_agent: Option<crate::agent::subagent::AgentId>,
    pub detail_open: bool,
    pub detail_scroll: u16,
    pub message_draft: Option<String>,
    pub message_cursor: usize,
    pub cancel_confirmation: bool,
    pub cancel_confirm_selected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
    pub status: ToolStatus,
    pub output: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryBoundary {
    pub user_message_index: usize,
    pub through_message_count: Option<usize>,
}

impl RetryBoundary {
    pub const fn replay(user_message_index: usize) -> Self {
        Self {
            user_message_index,
            through_message_count: None,
        }
    }

    pub const fn continuation(user_message_index: usize, through_message_count: usize) -> Self {
        Self {
            user_message_index,
            through_message_count: Some(through_message_count),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileActivityKind {
    Created,
    Edited,
    Deleted,
    Changed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileActivityPresentation {
    pub path: std::path::PathBuf,
    pub kind: FileActivityKind,
}

impl FileActivityPresentation {
    pub fn from_receipt(receipt: &crate::undo::FileMutationReceipt) -> Self {
        let kind = match (&receipt.before, &receipt.after) {
            (crate::undo::FileState::Missing, crate::undo::FileState::Utf8(_)) => {
                FileActivityKind::Created
            }
            (crate::undo::FileState::Utf8(_), crate::undo::FileState::Utf8(_)) => {
                FileActivityKind::Edited
            }
            (crate::undo::FileState::Utf8(_), crate::undo::FileState::Missing) => {
                FileActivityKind::Deleted
            }
            _ => FileActivityKind::Changed,
        };
        Self {
            path: receipt.path.clone(),
            kind,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TranscriptBlockKey {
    ToolCalls { first_call_id: String },
}

impl TranscriptBlockKey {
    pub fn tool_calls(first_call_id: &str) -> Option<Self> {
        (!first_call_id.is_empty()).then(|| Self::ToolCalls {
            first_call_id: first_call_id.to_string(),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelInfo {
    pub id: String,
    pub provider: String,
    pub display_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteSelection {
    pub model: Option<ModelInfo>,
    pub reasoning_effort: Option<crate::config::ReasoningEffort>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OneTurnRouteState {
    pub baseline: RouteSelection,
    pub consumed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutePresetRow {
    pub name: String,
    pub preset: crate::config::RoutePreset,
}

#[derive(Debug, Clone, Default)]
pub struct RoutePresetPickerState {
    pub rows: Vec<RoutePresetRow>,
    pub selected_idx: usize,
    pub error: Option<String>,
}

impl RoutePresetPickerState {
    pub fn selected(&self) -> Option<&RoutePresetRow> {
        self.rows.get(self.selected_idx)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReasoningPickerState {
    pub efforts: Vec<crate::config::ReasoningEffort>,
    pub selected_idx: usize,
}

impl ReasoningPickerState {
    pub fn selected(&self) -> Option<crate::config::ReasoningEffort> {
        self.efforts.get(self.selected_idx).copied()
    }
}

#[derive(Debug, Clone)]
pub struct RoutePresetOverwriteConfirmation {
    pub name: String,
    pub preset: crate::config::RoutePreset,
    pub save_selected: bool,
    pub busy: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelPickerTarget {
    Current,
    Review,
    Delegation,
    MultiRun(crate::multi_run::CandidateId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigHubAction {
    Profile(String),
    CurrentModel,
    Delegation {
        selected: Option<crate::config::DelegationRoute>,
        configured: Vec<crate::config::DelegationRoute>,
    },
    DelegationMode(crate::config::DelegationMode),
    ProviderSetup,
    Reasoning,
    ReviewModel,
    Mcp,
    SelfImprovement,
    Skills,
    Hooks,
    Sandbox,
    ContextLimits,
    SessionRetention,
    Notification,
    Mouse,
    CommandOutput,
    ConfigFile(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigHubItem {
    pub label: String,
    pub value: String,
    pub keywords: String,
    pub action: ConfigHubAction,
}

impl ConfigHubItem {
    pub fn matches(&self, filter: &str) -> bool {
        let filter = filter.to_ascii_lowercase();
        filter.is_empty()
            || self.label.to_ascii_lowercase().contains(&filter)
            || self.value.to_ascii_lowercase().contains(&filter)
            || self.keywords.to_ascii_lowercase().contains(&filter)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigHubState {
    pub filter: String,
    pub selected_idx: usize,
    pub items: Vec<ConfigHubItem>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SandboxConfirmationState {
    pub target: crate::sandbox::SandboxMode,
    pub confirm_selected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxHubState {
    pub saved_mode: crate::sandbox::SandboxMode,
    pub effective_mode: crate::sandbox::SandboxMode,
    pub backend_available: bool,
    pub writable_roots: Vec<String>,
    pub selected_idx: usize,
    pub confirmation: Option<SandboxConfirmationState>,
    pub busy: bool,
    pub writes_enabled: bool,
    pub notice: Option<String>,
    pub error: Option<String>,
}

impl SandboxHubState {
    pub const MODES: [crate::sandbox::SandboxMode; 3] = [
        crate::sandbox::SandboxMode::Required,
        crate::sandbox::SandboxMode::PolicyOnly,
        crate::sandbox::SandboxMode::Off,
    ];

    pub fn selected_mode(&self) -> crate::sandbox::SandboxMode {
        Self::MODES[self.selected_idx.min(Self::MODES.len() - 1)]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextLimitField {
    Window,
    Threshold,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextLimitEditorState {
    pub provider: String,
    pub model: String,
    pub window_input: String,
    pub threshold_input: String,
    pub fallback_window: Option<u64>,
    pub fallback_threshold: Option<u64>,
    pub editing_profile: bool,
    pub field: ContextLimitField,
    pub cursor: usize,
    pub saving: bool,
    pub writes_enabled: bool,
    pub error: Option<String>,
}

impl ContextLimitEditorState {
    pub fn active_input(&self) -> &str {
        match self.field {
            ContextLimitField::Window => &self.window_input,
            ContextLimitField::Threshold => &self.threshold_input,
        }
    }

    pub fn active_input_mut(&mut self) -> &mut String {
        match self.field {
            ContextLimitField::Window => &mut self.window_input,
            ContextLimitField::Threshold => &mut self.threshold_input,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelPromptCacheField {
    RequestFormat,
    UsageFormat,
    Policy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelPromptCacheEditorState {
    pub provider: String,
    pub model: String,
    pub protocol: crate::agent::ProviderProtocol,
    pub request_format: Option<crate::config::PromptCacheRequestFormat>,
    pub usage_format: Option<crate::config::PromptCacheUsageFormat>,
    pub policy_override: Option<crate::config::PromptCachePolicy>,
    pub inherited_policy: Option<crate::config::PromptCachePolicy>,
    pub contract_id: String,
    pub contract_source: crate::provider_contract::PromptCacheContractSource,
    pub policy_source: crate::provider_contract::PromptCachePolicySource,
    pub formats_locked: bool,
    pub field: ModelPromptCacheField,
    pub saving: bool,
    pub writes_enabled: bool,
    pub error: Option<String>,
}

impl ModelPromptCacheEditorState {
    pub fn for_model(
        config: &crate::config::CaConfig,
        provider: impl Into<String>,
        model: impl Into<String>,
        writes_enabled: bool,
    ) -> Result<Self, String> {
        let provider = provider.into();
        let model = model.into();
        let provider_config = config
            .providers
            .get(&provider)
            .ok_or_else(|| format!("Provider {provider} is not configured"))?;
        let configured = config.model_prompt_cache(&provider, &model);
        let resolved = crate::provider_contract::resolve_prompt_cache(
            config,
            &provider,
            provider_config,
            &model,
        );
        let formats_locked = resolved.first_party_formats();
        Ok(Self {
            provider,
            model,
            protocol: provider_config.protocol,
            request_format: if formats_locked {
                resolved.request_format()
            } else {
                configured.and_then(|item| item.request_format)
            },
            usage_format: if formats_locked {
                resolved.usage_format()
            } else {
                configured.and_then(|item| item.usage_format)
            },
            policy_override: configured.and_then(|item| item.policy),
            inherited_policy: resolved.policy(),
            contract_id: resolved.contract_id().to_string(),
            contract_source: resolved.contract_source(),
            policy_source: resolved.policy_source(),
            formats_locked,
            field: if formats_locked {
                ModelPromptCacheField::Policy
            } else {
                ModelPromptCacheField::RequestFormat
            },
            saving: false,
            writes_enabled,
            error: None,
        })
    }

    pub fn effective_policy(&self) -> Option<crate::config::PromptCachePolicy> {
        self.policy_override.or_else(|| {
            if self.formats_locked {
                self.inherited_policy
            } else {
                self.request_format
                    .map(|_| crate::config::PromptCachePolicy::Automatic)
            }
        })
    }

    pub fn supported_policies(&self) -> &'static [crate::config::PromptCachePolicy] {
        self.request_format
            .map(crate::config::PromptCacheRequestFormat::supported_policies)
            .unwrap_or(&[])
    }

    pub fn update(&self) -> crate::config::ModelPromptCacheUpdate {
        crate::config::ModelPromptCacheUpdate {
            provider: self.provider.clone(),
            model: self.model.clone(),
            request_format: if self.formats_locked {
                None
            } else {
                self.request_format
            },
            usage_format: if self.formats_locked {
                None
            } else {
                self.usage_format
            },
            policy: self.policy_override,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderEditorField {
    Provider,
    BaseUrl,
    ApiKey,
    ApiKeyEnv,
    PromptCache,
    Model,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderEditorMode {
    AddProvider,
    EditProvider,
    AddModel,
    EditModel,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProviderEditorRequest {
    AddProvider,
    AddOpenAiProvider,
    AddProviderTemplate(String),
    AddDiscoveredProvider(crate::provider_discovery::LocalProviderCandidate),
    EditProvider(String),
    AddModel(ModelInfo),
    EditModel(ModelInfo),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderEditorOrigin {
    ModelPicker,
    ProviderManager,
    ProviderSetup,
    ProviderDiscovery,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderDiscoveryState {
    pub candidates: Vec<crate::provider_discovery::LocalProviderCandidate>,
    pub selected_idx: usize,
    pub scanning: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderTemplateReturn {
    Providers,
    SetupProviders,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderManagerView {
    Providers,
    Models { provider: String },
    Templates { return_to: ProviderTemplateReturn },
    SetupProviders,
    SetupProvider { provider: String },
    SetupFinish,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProviderDeleteConfirmation {
    pub provider: String,
    pub configured_model_count: usize,
    pub removes_keychain: bool,
    pub replacement: Option<ModelInfo>,
    pub confirm_selected: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProviderManagerState {
    pub view: ProviderManagerView,
    pub providers: Vec<crate::config::ProviderManagementEntry>,
    pub provider_filter: String,
    pub selected_provider_idx: usize,
    pub model_filter: String,
    pub selected_model_idx: usize,
    pub template_filter: String,
    pub selected_template_idx: usize,
    pub delete_confirmation: Option<ProviderDeleteConfirmation>,
    pub busy: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderAuthStage {
    MethodPicker,
    RegionPicker,
    PlanPicker,
    TermPicker,
    Starting,
    WaitingBrowser,
    WaitingDevice,
    SecretInput,
    PlanUsdInput,
    PriceConfirmation,
    Probing,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MimoSetupMethod {
    SavedTokenPlan,
    Browser,
    ManualTokenPlan,
}

#[derive(Debug)]
pub struct ProviderAuthState {
    pub provider: crate::provider_connect::FirstClassProvider,
    pub stage: ProviderAuthStage,
    pub url: Option<String>,
    pub code: Option<String>,
    pub prompt: String,
    pub input: crate::provider_secrets::SecretValue,
    pub cursor: usize,
    pub metadata_input: String,
    pub metadata_cursor: usize,
    pub selected_index: usize,
    pub mimo_method: Option<MimoSetupMethod>,
    pub saved_mimo_region: Option<crate::pricing::MimoRegion>,
    pub mimo_region: Option<crate::pricing::MimoRegion>,
    pub mimo_tier: Option<crate::pricing::MimoPlanTier>,
    pub mimo_term: Option<crate::pricing::MimoPlanTerm>,
    pub billing: Option<crate::pricing::ProviderBillingConfig>,
    pub error: Option<String>,
    pub bare: bool,
    pub generation: u64,
}

impl ProviderAuthState {
    pub fn mimo_method_options(&self) -> Vec<MimoSetupMethod> {
        let mut methods = Vec::with_capacity(3);
        if self.saved_mimo_region.is_some() {
            methods.push(MimoSetupMethod::SavedTokenPlan);
        }
        methods.push(MimoSetupMethod::Browser);
        methods.push(MimoSetupMethod::ManualTokenPlan);
        methods
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderModelDraft {
    pub original_model: Option<String>,
    pub provider: String,
    pub base_url: String,
    pub api_key_env: String,
    pub model: String,
}

#[derive(Debug)]
pub struct ProviderEditorState {
    pub mode: ProviderEditorMode,
    pub origin: ProviderEditorOrigin,
    pub draft: ProviderModelDraft,
    pub api_key: crate::provider_secrets::SecretValue,
    pub credential_configured: bool,
    pub credential_in_keychain: bool,
    pub protocol_override: Option<crate::agent::ProviderProtocol>,
    pub prompt_cache_mode: crate::config::PromptCacheMode,
    pub field: ProviderEditorField,
    pub cursor: usize,
    pub error: Option<String>,
    pub saving: bool,
}

#[derive(Debug, Clone)]
pub struct StreamingState {
    pub content: String,
    pub tool_calls: Vec<PartialToolCall>,
    pub thinking: Option<String>,
    pub response_items: Vec<crate::agent::stream::ProviderResponseItem>,
    pub last_usage: Option<crate::agent::stream::UsageInfo>,
    pub usage_handle: crate::usage::UsageHandle,
    pub reasoning_started_at: Option<std::time::Instant>,
    pub reasoning_duration: Option<std::time::Duration>,
    pub generation_started_at: Option<std::time::Instant>,
}

#[derive(Debug, Clone, Default)]
pub struct ActiveTurnPromptState {
    pub stable_system: String,
    pub logical_turn_system: String,
    pub skill_prompt: String,
    pub transient_user_prompts: Vec<String>,
    pub pasted_images: Vec<crate::image_content::ModelImage>,
    pub canonical_tools: Option<Vec<crate::agent::client::ToolDef>>,
}

impl StreamingState {
    pub fn observe_generation_at(&mut self, now: std::time::Instant) {
        self.generation_started_at.get_or_insert(now);
    }

    pub fn observe_reasoning_at(&mut self, now: std::time::Instant) {
        self.observe_generation_at(now);
        if self.reasoning_duration.is_none() {
            self.reasoning_started_at.get_or_insert(now);
        }
    }

    pub fn finish_reasoning_at(&mut self, now: std::time::Instant) {
        if self.reasoning_duration.is_some() {
            return;
        }
        if let Some(started) = self.reasoning_started_at.take() {
            self.reasoning_duration = Some(now.saturating_duration_since(started));
        }
    }

    pub fn visible_reasoning_duration(&self) -> Option<std::time::Duration> {
        self.reasoning_duration
            .or_else(|| self.reasoning_started_at.map(|started| started.elapsed()))
    }

    pub fn generation_duration_at(&self, now: std::time::Instant) -> Option<std::time::Duration> {
        self.generation_started_at
            .map(|started| now.saturating_duration_since(started))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BtwStatus {
    Editing,
    Running,
    Complete,
    Failed(String),
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BtwState {
    pub question: String,
    pub answer: String,
    pub status: BtwStatus,
    pub provider: String,
    pub model: String,
    /// UTF-8 byte offset into `question`.
    pub cursor: usize,
    pub scroll: u16,
    pub generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingApproval {
    pub tool_name: String,
    pub tool_args: String,
    pub tool_id: String,
    pub network_requested: bool,
    pub email_preview: Option<Box<crate::email::types::ExternalCommunicationPreview>>,
    pub email_draft_ref: Option<String>,
}

/// Result of resolving the current approval item.
#[derive(Debug, Clone, PartialEq)]
pub enum ResolutionAction {
    /// Advance to next queued approval item.
    Advance {
        next: PendingApproval,
        resolved: usize,
        total: usize,
    },
    /// Batch exhausted — trigger provider continuation exactly once.
    Continue,
    /// No current item exists; resolution was a no-op.
    Idle,
}

/// Classification of how a tool call was pre-resolved.
#[derive(Debug, Clone, PartialEq)]
pub enum ResolvedToolStatus {
    /// Tool was executed (allowed by rule or approved by user).
    Done,
    /// Tool was denied (by rule or user).
    Error,
}

/// A pre-resolved tool result carrying its classification.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedToolResult {
    pub tool_id: String,
    pub output: String,
    pub status: ResolvedToolStatus,
    pub image: Option<crate::image_content::ModelImage>,
    pub memory_proposal: Option<crate::memory::MemoryProposal>,
    pub mutation: Option<crate::undo::FileMutationBatchReceipt>,
    pub command_observation: Option<crate::command_output::CommandObservation>,
}

/// Outcome of a cancellation: the tool_id that was cancelled and
/// the queue transition action (advance, continue, or idle).
#[derive(Debug, Clone, PartialEq)]
pub struct CancelOutcome {
    pub cancelled_tool_id: String,
    pub action: ResolutionAction,
}

/// Ordered queue of tool calls awaiting user approval.
///
/// Fields are private; callers interact through cohesive methods:
/// `begin_batch`, `current`, `progress`, `mark_in_flight`/`clear_in_flight`,
/// `resolve`, and `clear`.
#[derive(Debug, Clone)]
pub struct ApprovalQueue {
    remaining: VecDeque<PendingApproval>,
    current: Option<PendingApproval>,
    in_flight: bool,
    total: usize,
    resolved: usize,
}

impl ApprovalQueue {
    pub fn new() -> Self {
        Self {
            remaining: VecDeque::new(),
            current: None,
            in_flight: false,
            total: 0,
            resolved: 0,
        }
    }

    /// Atomically populate the queue with a new batch.
    /// Clears any previous state and advances to the first item.
    pub fn begin_batch(&mut self, approvals: Vec<PendingApproval>) {
        self.remaining.clear();
        self.current = None;
        self.in_flight = false;
        self.total = approvals.len();
        self.resolved = 0;
        self.remaining = approvals.into();
        self.advance();
    }

    /// True when there is a current item being decided, remaining items,
    /// or an approval execution in flight.
    pub fn is_active(&self) -> bool {
        self.current.is_some() || !self.remaining.is_empty() || self.in_flight
    }

    /// Read-only access to the current approval item.
    pub fn current(&self) -> Option<&PendingApproval> {
        self.current.as_ref()
    }

    /// Progress as (resolved, total).
    pub fn progress(&self) -> (usize, usize) {
        (self.resolved, self.total)
    }

    /// Whether an async execution is in flight for the current item.
    pub fn is_in_flight(&self) -> bool {
        self.in_flight
    }

    /// Mark the current item as in-flight (async execution spawned).
    /// Returns false if there is no current item or already in flight.
    pub fn mark_in_flight(&mut self) -> bool {
        if self.current.is_none() || self.in_flight {
            return false;
        }
        self.in_flight = true;
        true
    }

    /// Clear the in-flight flag (async execution completed).
    pub fn clear_in_flight(&mut self) {
        self.in_flight = false;
    }

    /// Replace the tool_args of the current approval item.
    pub fn update_current_args(&mut self, new_args: &str) {
        if let Some(ref mut current) = self.current {
            current.tool_args = new_args.to_string();
        }
    }

    /// Pop the front of `remaining` into `current`.
    /// Returns true if a new current was set.
    pub fn advance(&mut self) -> bool {
        self.current = self.remaining.pop_front();
        self.current.is_some()
    }

    /// Resolve the current item: increment resolved, clear current/in_flight,
    /// advance to the next item. Returns `Advance` with next item or
    /// `Continue` when the batch is exhausted. Returns `Idle` when no
    /// current item exists (repeated/idle resolution is a no-op).
    pub fn resolve(&mut self) -> ResolutionAction {
        if self.current.is_none() {
            return ResolutionAction::Idle;
        }
        self.resolved += 1;
        self.current = None;
        self.in_flight = false;

        if self.advance() {
            ResolutionAction::Advance {
                next: self
                    .current
                    .as_ref()
                    .expect("advance() returned true so current is Some")
                    .clone(),
                resolved: self.resolved,
                total: self.total,
            }
        } else {
            ResolutionAction::Continue
        }
    }

    /// Complete an in-flight approval by validating the tool_id matches
    /// the current in-flight item. Returns true if matched and resolved,
    /// false if the id is stale/duplicate (no-op).
    pub fn complete_in_flight(&mut self, tool_id: &str) -> ResolutionAction {
        if !self.in_flight {
            return ResolutionAction::Idle;
        }
        match &self.current {
            Some(current) if current.tool_id == tool_id => {
                // Proceed with resolution
                self.resolved += 1;
                self.current = None;
                self.in_flight = false;

                if self.advance() {
                    ResolutionAction::Advance {
                        next: self
                            .current
                            .as_ref()
                            .expect("advance() returned true so current is Some")
                            .clone(),
                        resolved: self.resolved,
                        total: self.total,
                    }
                } else {
                    ResolutionAction::Continue
                }
            }
            _ => ResolutionAction::Idle,
        }
    }

    /// Cancel the current item as a single cohesive transition:
    /// increment resolved, clear current/in_flight, and advance to the
    /// next item. Returns the cancelled tool_id and the queue's
    /// transition action (Advance with next item, Continue when
    /// exhausted). Returns None when there is no current item.
    pub fn cancel_current(&mut self) -> Option<CancelOutcome> {
        let tool_id = self.current.as_ref()?.tool_id.clone();
        self.resolved += 1;
        self.current = None;
        self.in_flight = false;

        let action = if self.advance() {
            ResolutionAction::Advance {
                next: self
                    .current
                    .as_ref()
                    .expect("advance() returned true so current is Some")
                    .clone(),
                resolved: self.resolved,
                total: self.total,
            }
        } else {
            ResolutionAction::Continue
        };

        Some(CancelOutcome {
            cancelled_tool_id: tool_id,
            action,
        })
    }

    /// Clear all queue state (for reset/reuse).
    pub fn clear(&mut self) {
        self.remaining.clear();
        self.current = None;
        self.in_flight = false;
        self.total = 0;
        self.resolved = 0;
    }
}

impl Default for ApprovalQueue {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct PartialToolCall {
    pub index: usize,
    pub id: Option<String>,
    pub name: Option<String>,
    pub arguments: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionDeleteConfirmation {
    pub storage_key: String,
    pub session_id: uuid::Uuid,
    pub title: String,
    pub delete_selected: bool,
    pub busy: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceDeleteConfirmation {
    pub trace_id: uuid::Uuid,
    pub bytes: u64,
    pub delete_selected: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectInitPhase {
    Preparing,
    Preview,
    Writing,
}

#[derive(Debug, Clone)]
pub struct ProjectInitState {
    pub root: std::path::PathBuf,
    pub draft: Option<crate::project_init::ProjectInitDraft>,
    pub phase: ProjectInitPhase,
    pub scroll: u16,
    pub create_selected: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SessionPickerView {
    #[default]
    Active,
    Archive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionTransferDirection {
    Archive,
    Restore,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionArchiveConfirmation {
    pub storage_key: String,
    pub session_id: uuid::Uuid,
    pub title: String,
    pub archive_selected: bool,
    pub busy: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRenameState {
    pub storage_key: String,
    pub session_id: uuid::Uuid,
    pub title: String,
    pub cursor: usize,
    pub archived: bool,
    pub busy: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionHistoryState {
    pub storage_key: String,
    pub session_id: uuid::Uuid,
    pub storage_generation: uuid::Uuid,
    pub start_index: usize,
    pub total_messages: usize,
    pub busy: bool,
    pub request_older: bool,
    pub request_full: bool,
    pub resume_submit: bool,
    pub generation: u64,
    pub error: Option<String>,
}

impl SessionHistoryState {
    pub fn is_complete(&self) -> bool {
        self.start_index == 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionPortabilityOperation {
    Export {
        format: crate::session_portability::ExportFormat,
        overwrite: bool,
    },
    Import,
    ForeignImport {
        source: crate::session::ForeignTranscriptSource,
        commit: bool,
        learning_eligible: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionPortabilityRequest {
    pub operation: SessionPortabilityOperation,
    pub path: std::path::PathBuf,
    pub generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionPortabilityState {
    pub operation: SessionPortabilityOperation,
    pub path: std::path::PathBuf,
    pub generation: u64,
    pub busy: bool,
    pub awaiting_overwrite: bool,
    pub overwrite_selected: bool,
    pub import_selected: bool,
    pub import_learning_eligible: bool,
    pub import_preview: Option<crate::foreign_import::ForeignImportPreview>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionRetentionMode {
    Overview,
    Edit,
    Preview,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionRetentionField {
    Days,
    Count,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionRetentionBusy {
    Loading,
    Saving,
    Deleting,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionRetentionOperation {
    Load,
    Save(crate::session_retention::RetentionSettings),
    Delete {
        preview: crate::session_retention::RetentionPreview,
        current_id: Option<uuid::Uuid>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRetentionRequest {
    pub generation: u64,
    pub operation: SessionRetentionOperation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRetentionState {
    pub settings: crate::session_retention::RetentionSettings,
    pub mode: SessionRetentionMode,
    pub field: SessionRetentionField,
    pub days_input: String,
    pub count_input: String,
    pub cursor: usize,
    pub preview: Option<crate::session_retention::RetentionPreview>,
    pub preview_selected_idx: usize,
    pub delete_selected: bool,
    pub busy: Option<SessionRetentionBusy>,
    pub writes_enabled: bool,
    pub sessions_enabled: bool,
    pub active_count: usize,
    pub archived_count: usize,
    pub generation: u64,
    pub notice: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeymapCaptureState {
    pub action: crate::keymap::KeyAction,
    pub first: Option<crate::keymap::KeyStroke>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeymapHubState {
    pub filter: String,
    pub selected_idx: usize,
    pub capture: Option<KeymapCaptureState>,
    pub saving: bool,
    pub writes_enabled: bool,
    pub error: Option<String>,
    pub notice: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistorySearchState {
    pub filter: String,
    pub selected_idx: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptStashPickerState {
    pub entries: Vec<crate::prompt_stash::PromptDraft>,
    pub selected_idx: usize,
    pub snapshot_at: chrono::DateTime<chrono::Utc>,
    pub busy: bool,
    pub writes_enabled: bool,
    pub delete_confirmation: Option<uuid::Uuid>,
    pub notice: Option<String>,
    pub error: Option<String>,
}

impl PromptStashPickerState {
    pub fn selected(&self) -> Option<&crate::prompt_stash::PromptDraft> {
        let index = self.selected_idx.min(self.entries.len().saturating_sub(1));
        self.entries.get(index)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookManagerState {
    pub filter: String,
    pub selected_idx: usize,
    pub entries: Vec<crate::hooks::HookManagementEntry>,
    pub network_enforcement: String,
    pub config_path: String,
    pub busy: bool,
    pub testing: bool,
    pub writes_enabled: bool,
    pub execution_enabled: bool,
    pub error: Option<String>,
    pub notice: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionEntrySource {
    Legacy(usize),
    Rich(usize),
    Session(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionManagerEntry {
    pub source: PermissionEntrySource,
    pub decision: crate::permission::PermissionDecision,
    pub matcher: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionDeleteConfirmation {
    pub source: PermissionEntrySource,
    pub label: String,
    pub delete_selected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionManagerState {
    pub filter: String,
    pub selected_idx: usize,
    pub entries: Vec<PermissionManagerEntry>,
    pub confirmation: Option<PermissionDeleteConfirmation>,
    pub busy: bool,
    pub writes_enabled: bool,
    pub generation: u64,
    pub error: Option<String>,
    pub notice: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryManagerView {
    User,
    Workspace,
    Proposals,
}

impl MemoryManagerView {
    pub fn next(self) -> Self {
        match self {
            Self::User => Self::Workspace,
            Self::Workspace => Self::Proposals,
            Self::Proposals => Self::User,
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct MemoryEditorState {
    pub id: Option<uuid::Uuid>,
    pub scope: crate::memory::MemoryScope,
    pub kind: crate::memory::MemoryKind,
    pub text: String,
    pub cursor: usize,
}

impl std::fmt::Debug for MemoryEditorState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MemoryEditorState")
            .field("id", &self.id)
            .field("scope", &self.scope)
            .field("kind", &self.kind)
            .field("text", &"[MEMORY DRAFT REDACTED]")
            .field("cursor", &self.cursor)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryConfirmationAction {
    Delete {
        scope: crate::memory::MemoryScope,
        id: uuid::Uuid,
    },
    AcceptProposal {
        id: uuid::Uuid,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryConfirmation {
    pub action: MemoryConfirmationAction,
    pub confirm_selected: bool,
}

#[derive(Debug, Clone)]
pub struct MemoryManagerState {
    pub filter: String,
    pub selected_idx: usize,
    pub view: MemoryManagerView,
    pub editor: Option<MemoryEditorState>,
    pub confirmation: Option<MemoryConfirmation>,
    pub busy: bool,
    pub writes_enabled: bool,
    pub user_recall: bool,
    pub user_generation: bool,
    pub workspace_recall: bool,
    pub workspace_generation: bool,
    pub generation: u64,
    pub error: Option<String>,
    pub notice: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskGraphConfirmationAction {
    Start,
    Cancel,
    Retry(crate::work::TaskId),
    Review(crate::work::TaskId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskGraphConfirmation {
    pub action: TaskGraphConfirmationAction,
    pub confirm_selected: bool,
}

#[derive(Debug, Clone)]
pub struct TaskGraphManagerState {
    pub filter: String,
    pub selected_idx: usize,
    pub confirmation: Option<TaskGraphConfirmation>,
    pub error: Option<String>,
    pub notice: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MultiRunConfirmationAction {
    Start,
    Cancel,
    Judge,
    Fuse,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultiRunConfirmation {
    pub action: MultiRunConfirmationAction,
    pub confirm_selected: bool,
}

#[derive(Debug, Clone)]
pub struct MultiRunManagerState {
    pub draft_request: String,
    pub context_snapshot: String,
    pub pricing: std::collections::HashMap<String, crate::usage::UsagePricing>,
    pub cursor: usize,
    pub candidate_count: usize,
    pub editing: bool,
    pub run: Option<crate::multi_run::MultiRunState>,
    pub confirmation: Option<MultiRunConfirmation>,
    pub busy: bool,
    pub error: Option<String>,
    pub notice: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookManagerOperation {
    Toggle {
        index: usize,
        event: crate::hooks::HookEvent,
        enabled: bool,
    },
    Test {
        index: usize,
        event: crate::hooks::HookEvent,
    },
    Reload,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookManagerRequest {
    pub generation: u64,
    pub operation: HookManagerOperation,
}

#[derive(Debug, Clone)]
pub struct PendingKeyChord {
    pub first: crate::keymap::KeyStroke,
    pub started_at: std::time::Instant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalManagerMode {
    List,
    Create { name: String, cursor: usize },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalKillConfirmation {
    pub name: String,
    pub kill_selected: bool,
}

#[derive(Debug, Clone)]
pub struct TerminalManagerState {
    pub sessions: Vec<crate::terminal_sessions::TerminalSession>,
    pub selected_idx: usize,
    pub filter: String,
    pub version: Option<String>,
    pub unavailable: bool,
    pub busy: bool,
    pub mode: TerminalManagerMode,
    pub kill_confirmation: Option<TerminalKillConfirmation>,
    pub notice: Option<String>,
    pub error: Option<String>,
}

impl TerminalManagerState {
    pub fn filtered_indices(&self) -> Vec<usize> {
        let filter = self.filter.to_ascii_lowercase();
        self.sessions
            .iter()
            .enumerate()
            .filter(|(_, session)| {
                filter.is_empty() || session.name.to_ascii_lowercase().contains(&filter)
            })
            .map(|(index, _)| index)
            .collect()
    }

    pub fn selected_session(&self) -> Option<&crate::terminal_sessions::TerminalSession> {
        let filtered = self.filtered_indices();
        let index = filtered.get(self.selected_idx.min(filtered.len().saturating_sub(1)))?;
        self.sessions.get(*index)
    }
}

#[derive(Debug, Clone)]
pub struct ThemeManagerState {
    pub saved_theme: crate::ui_settings::UiTheme,
    pub selected_idx: usize,
    pub filter: String,
    pub busy: bool,
    pub notice: Option<String>,
    pub error: Option<String>,
}

impl ThemeManagerState {
    pub fn filtered_themes(&self) -> Vec<crate::ui_settings::UiTheme> {
        let filter = self.filter.to_ascii_lowercase();
        crate::ui_settings::UiTheme::ALL
            .into_iter()
            .filter(|theme| {
                filter.is_empty() || theme.label().to_ascii_lowercase().contains(&filter)
            })
            .collect()
    }

    pub fn selected_theme(&self) -> Option<crate::ui_settings::UiTheme> {
        let themes = self.filtered_themes();
        themes
            .get(self.selected_idx.min(themes.len().saturating_sub(1)))
            .copied()
    }
}

#[derive(Debug, Clone)]
pub struct StatuslineManagerState {
    pub selected_idx: usize,
    pub fields: Vec<crate::ui_settings::StatusField>,
    pub custom_configured: bool,
    pub custom_enabled: bool,
    pub refresh_secs: u64,
    pub busy: bool,
    pub testing: bool,
    pub notice: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NotificationPickerState {
    pub saved: crate::ui_settings::NotificationStyle,
    pub selected_idx: usize,
    pub busy: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopyPickerPart {
    pub label: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CopyPickerMessage {
    pub role: Role,
    pub summary: String,
    pub parts: Vec<CopyPickerPart>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyPickerLevel {
    Messages,
    Parts { message_index: usize },
}

#[derive(Debug, Clone, PartialEq)]
pub struct CopyPickerState {
    pub messages: Vec<CopyPickerMessage>,
    pub level: CopyPickerLevel,
    pub filter: String,
    pub selected_idx: usize,
}

impl CopyPickerState {
    pub fn filtered_message_indices(&self) -> Vec<usize> {
        let filter = self.filter.to_ascii_lowercase();
        self.messages
            .iter()
            .enumerate()
            .filter_map(|(index, message)| {
                (filter.is_empty() || message.summary.to_ascii_lowercase().contains(&filter))
                    .then_some(index)
            })
            .collect()
    }

    pub fn filtered_part_indices(&self, message_index: usize) -> Vec<usize> {
        let filter = self.filter.to_ascii_lowercase();
        self.messages
            .get(message_index)
            .into_iter()
            .flat_map(|message| message.parts.iter().enumerate())
            .filter_map(|(index, part)| {
                (filter.is_empty()
                    || part.label.to_ascii_lowercase().contains(&filter)
                    || part.value.to_ascii_lowercase().contains(&filter))
                .then_some(index)
            })
            .collect()
    }

    pub fn visible_len(&self) -> usize {
        match self.level {
            CopyPickerLevel::Messages => self.filtered_message_indices().len(),
            CopyPickerLevel::Parts { message_index } => {
                self.filtered_part_indices(message_index).len()
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct CommandOutputPickerState {
    pub saved: crate::config::CommandOutputPolicy,
    pub selected_idx: usize,
    pub busy: bool,
    pub error: Option<String>,
}

impl CommandOutputPickerState {
    pub fn selected(&self) -> crate::config::CommandOutputPolicy {
        crate::config::CommandOutputPolicy::ALL[self
            .selected_idx
            .min(crate::config::CommandOutputPolicy::ALL.len() - 1)]
    }
}

#[derive(Debug, Clone)]
pub struct OutputFullConfirmation {
    pub rerun: crate::command_output::FullOutputRerun,
    pub run_selected: bool,
    pub busy: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ScratchDeleteConfirmation {
    pub id: uuid::Uuid,
    pub delete_selected: bool,
    pub error: Option<String>,
}

impl NotificationPickerState {
    pub fn selected(&self) -> crate::ui_settings::NotificationStyle {
        crate::ui_settings::NotificationStyle::ALL[self
            .selected_idx
            .min(crate::ui_settings::NotificationStyle::ALL.len() - 1)]
    }
}

#[derive(Debug, Clone, Default)]
pub struct StatusCommandState {
    pub output: Option<String>,
    pub error: Option<String>,
    pub running: bool,
    pub generation: u64,
    pub requested: bool,
    pub last_started: Option<std::time::Instant>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LearnedRecipeDecision {
    ApproveGlobal,
    Reject,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LearnedRecipeRequest {
    Load {
        server: String,
        recipe_id: Option<String>,
    },
    Decide {
        server: String,
        recipe_id: String,
        digest: String,
        repository_revision: String,
        decision: LearnedRecipeDecision,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LearnedRecipeReview {
    pub server: String,
    pub recipe_id: String,
    pub digest: String,
    pub repository_revision: String,
    pub markdown: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelfImproveDecision {
    Pause,
    Resume,
    RollbackCa,
    RollbackCodexTeam,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelfImproveRequest {
    Load {
        server: String,
    },
    Decide {
        server: String,
        revision: String,
        decision: SelfImproveDecision,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelfImproveReview {
    pub server: String,
    pub revision: String,
    pub paused: bool,
    pub markdown: String,
}

#[derive(Debug, Clone)]
pub struct SupportBundleConfirmation {
    pub plan: crate::support_bundle::SupportBundlePlan,
    pub create_selected: bool,
    pub busy: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SetupImportConfirmation {
    pub preview: crate::setup_import::SetupImportPreview,
    pub apply_selected: bool,
    pub busy: bool,
    pub error: Option<String>,
    pub scroll: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetupImportOperation {
    Preview,
    Apply { digest: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupImportRequest {
    pub source: std::path::PathBuf,
    pub scope: crate::setup_import::SetupImportScope,
    pub operation: SetupImportOperation,
    pub generation: u64,
}

#[derive(Debug, Clone)]
pub struct ReviewConvergenceUiRecord {
    pub target: crate::changes::ReviewTarget,
    pub input: crate::changes::ReviewInput,
    pub outcome: crate::review_convergence::ReviewConvergenceRound,
    pub max_cost_usd: crate::usage::DecimalUsd,
    pub original_request: String,
    pub revision: crate::acceptance::AcceptanceRevisionV1,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcceptanceUiRequest {
    Create,
    Latest,
    List,
    Export(std::path::PathBuf),
}

#[derive(Debug)]
pub struct AppState {
    pub pane_id: crate::panes::PaneId,
    pub pane_deck: Option<Box<crate::panes::PaneDeck>>,
    pub pane_request: Option<crate::panes::PaneRequest>,
    pub pane_split_generation: u64,
    pub pane_split_busy: bool,
    pub pane_split_loaded: Option<Box<crate::session::RecentSessionLoad>>,
    pub(crate) pane_close_generation: u64,
    pub(crate) pane_close_busy: bool,
    pub(crate) pane_close_committed: bool,
    pub runtime_diagnostics: crate::runtime_diagnostics::RuntimeDiagnostics,
    pub debug_render_overlay: bool,
    pub active_profile: crate::runtime::ActiveProfile,
    pub tool_policy: crate::tool_selection::ToolSelectionPolicy,
    pub active_tool_names: std::collections::HashSet<String>,
    pub(crate) owned_tools: crate::owned_tools::OwnedToolContext,
    pub(crate) retired_mutations: Vec<crate::recovery::RecoveryContext>,
    pub(crate) recovery_retiring: std::collections::HashSet<uuid::Uuid>,
    pub(crate) discovery_batch: Option<crate::tool_discovery::DiscoveryBatch>,
    pub config_path: Option<String>,
    pub run_capabilities: crate::runtime::RunCapabilities,
    pub effective_sandbox_mode: crate::sandbox::SandboxMode,
    pub mode: AgentMode,
    pub approval: ApprovalMode,
    pub session_grants: std::sync::Arc<std::sync::Mutex<crate::permission::SessionGrantStore>>,
    pub input: String,
    /// Process-local visual and atomic-element state for `input`.
    pub composer_ui: crate::tui::composer::ComposerUiState,
    pub input_history: Vec<String>,
    pub history_idx: Option<usize>,
    pub messages: Vec<Message>,
    pub transcript_annotations: Vec<TranscriptMessageAnnotation>,
    pub show_transcript_timestamps: bool,
    pub show_assistant_footers: bool,
    pub transcript_agent_summaries: Vec<TranscriptAgentSummary>,
    pub active_agent_turn_id: Option<uuid::Uuid>,
    pub live_agent_activity: Vec<crate::agent::subagent::AgentActivitySnapshot>,
    pub pending_agent_completions: VecDeque<TranscriptAgentSummary>,
    pub pending_job_completions: VecDeque<crate::jobs::JobInfo>,
    pub agent_activity_focus: Option<AgentActivityFocusState>,
    pub streaming: Option<StreamingState>,
    pub active_turn_prompt: Option<ActiveTurnPromptState>,
    pub model_picker_open: bool,
    pub model_picker_target: ModelPickerTarget,
    pub model_filter: String,
    pub model_list: Vec<ModelInfo>,
    pub delegation_model_routes: std::collections::HashSet<(String, String)>,
    pub selected_model_idx: usize,
    pub current_model: Option<ModelInfo>,
    pub model_preferences: crate::model_preferences::ModelPreferences,
    pub model_preferences_store: crate::model_preferences::ModelPreferencesStore,
    pub one_turn_route: Option<OneTurnRouteState>,
    pub route_preset_picker: Option<RoutePresetPickerState>,
    pub route_preset_overwrite: Option<RoutePresetOverwriteConfirmation>,
    pub discovered_models: std::collections::HashMap<String, Vec<String>>,
    /// Last successful native Codex catalogs; never populated from fallback lists.
    pub codex_catalogs: std::collections::HashMap<String, Vec<String>>,
    pub model_catalog_refresh_requested: bool,
    pub model_catalog_refresh_provider: Option<String>,
    pub model_catalog_refreshing: bool,
    pub model_catalog_loaded: bool,
    pub model_catalog_generation: u64,
    pub model_catalog_status: Option<String>,
    pub model_catalog_provider_status: std::collections::HashMap<String, String>,
    pub provider_checks:
        std::collections::HashMap<String, crate::provider_probe::ProviderCheckStatus>,
    pub provider_check_generation: u64,
    pub provider_check_running: Option<String>,
    pub provider_manager: Option<ProviderManagerState>,
    pub provider_auth: Option<ProviderAuthState>,
    pub provider_auth_generation: u64,
    pub provider_editor: Option<ProviderEditorState>,
    pub provider_discovery: Option<ProviderDiscoveryState>,
    pub provider_discovery_requested: bool,
    pub provider_discovery_generation: u64,
    pub btw: Option<BtwState>,
    pub btw_start_requested: bool,
    pub btw_generation: u64,
    pub voice: Option<crate::voice::VoiceState>,
    pub voice_generation: u64,
    pub config_hub: Option<ConfigHubState>,
    pub sandbox_hub: Option<SandboxHubState>,
    pub context_limit_editor: Option<ContextLimitEditorState>,
    pub model_prompt_cache_editor: Option<ModelPromptCacheEditorState>,
    pub keymap: crate::keymap::Keymap,
    pub keymap_customized: std::collections::HashSet<crate::keymap::KeyAction>,
    pub keymap_hub: Option<KeymapHubState>,
    pub history_search: Option<HistorySearchState>,
    pub prompt_stash_store: crate::prompt_stash::PromptStashStore,
    pub prompt_stash: Option<PromptStashPickerState>,
    pub hook_manager: Option<HookManagerState>,
    pub permission_manager: Option<PermissionManagerState>,
    pub permission_manager_generation: u64,
    pub memory_manager: Option<MemoryManagerState>,
    pub memory_manager_generation: u64,
    pub user_memory_store: Option<crate::memory::MemoryStore>,
    pub workspace_memory_store: Option<crate::memory::MemoryStore>,
    pub maintenance_request: Option<crate::learning::MaintenanceRequest>,
    pub user_memory_records: Vec<crate::memory::MemoryRecord>,
    pub workspace_memory_records: Vec<crate::memory::MemoryRecord>,
    pub memory_proposals: Vec<crate::memory::MemoryProposal>,
    pub hook_manager_request: Option<HookManagerRequest>,
    pub hook_manager_generation: u64,
    pub hook_running: bool,
    pub(crate) active_hook_lifecycle: Option<(u64, crate::hooks::HookInvocation)>,
    pub(crate) active_hook_abort: Option<tokio::task::AbortHandle>,
    pub hook_blocked: bool,
    pub hook_blocked_event: Option<crate::hooks::HookEvent>,
    pub pending_key_chord: Option<PendingKeyChord>,
    pub reasoning_effort: Option<crate::config::ReasoningEffort>,
    pub reasoning_picker: Option<ReasoningPickerState>,
    pub show_thinking: bool,
    pub slash_palette_open: bool,
    pub slash_filter: String,
    pub selected_slash_idx: usize,
    pub(crate) prompt_commands: crate::prompt_commands::PromptCommandRegistry,
    pub(crate) rules: crate::rules::RuleRegistry,
    pub(crate) manual_rule_ids: std::collections::BTreeSet<String>,
    pub(crate) explicit_rule_paths: Vec<std::path::PathBuf>,
    pub(crate) rules_home: Option<std::path::PathBuf>,
    pub(crate) exclude_workspace_context: bool,
    pub(crate) pending_git_context: Option<crate::local_automation::GitContextSnapshot>,
    pub(crate) asset_mutation_preview: Option<crate::knowledge_assets::AssetMutationPreview>,
    pub(crate) skill_bundles: crate::skill_bundles::SkillBundleRegistry,
    pub(crate) learning_ledger: Option<crate::learning_journey::LearningLedger>,
    pub(crate) skill_curator: Option<crate::skill_curator::SkillCuratorStore>,
    pub(crate) active_skill_invocation_group: Option<String>,
    pub(crate) pending_bundle_invocation: Option<(String, String)>,
    pub(crate) recall_request: Option<crate::session_recall::SessionSearchRequest>,
    pub recall_busy: bool,
    pub recall_generation: u64,
    pub pending_input_action: Option<crate::tui::commands::InputAction>,
    pub status_message: Option<String>,
    pub toast: Option<UiToast>,
    pub should_quit: bool,
    pub transcript_scroll: crate::tui::transcript::TranscriptScrollState,
    pub(crate) transcript_cache: crate::tui::render::TranscriptCache,
    pub viewport_width: u16,
    pub viewport_height: u16,
    /// UTF-8 byte offset into `input`; always `<= input.len()` and on a char boundary.
    pub input_cursor: usize,
    pub cwd: String,
    pub workspace: crate::workspace::WorkspaceContext,
    pub(crate) workspace_catalog: Option<crate::workspace_catalog::WorkspaceCatalogStore>,
    pub skills: std::sync::Arc<std::sync::Mutex<crate::skills::SkillRegistry>>,
    /// Exact, digest-pinned loads for only the currently executing user task.
    pub task_skill_loads: crate::skills::SkillLoadSet,
    pub task_skill_snapshot: std::sync::Arc<std::sync::Mutex<String>>,
    pub mcp: Option<std::sync::Arc<crate::mcp::McpManager>>,
    pub lsp: Option<std::sync::Arc<crate::lsp::LspManager>>,
    pub email: Option<std::sync::Arc<crate::email::EmailManager>>,
    pub mcp_request: Option<String>,
    pub mcp_task_active: bool,
    pub mcp_cancel_requested: bool,
    pub mcp_generation: u64,
    pub(crate) direnv_request: Option<crate::run_environment::DirenvPreview>,
    pub(crate) direnv_preview: Option<crate::run_environment::DirenvPreview>,
    pub direnv_busy: bool,
    pub direnv_generation: u64,
    pub direnv_allowed: bool,
    pub learned_recipe_request: Option<LearnedRecipeRequest>,
    pub learned_recipe_review: Option<LearnedRecipeReview>,
    pub(crate) learning_review_request: Option<crate::codex_learning::TuiLearningRequest>,
    pub(crate) learning_review_view: Option<crate::codex_learning::TuiLearningView>,
    pub(crate) learning_review_progress: Option<crate::codex_learning::TuiLearningProgress>,
    pub(crate) learning_review_transcript_index: Option<usize>,
    pub learning_review_busy: bool,
    pub learning_review_generation: u64,
    pub learning_review_pager_open: bool,
    pub learning_review_error: Option<String>,
    pub learning_review_interaction: bool,
    pub self_improve_request: Option<SelfImproveRequest>,
    pub self_improve_review: Option<SelfImproveReview>,
    pub agents: Option<std::sync::Arc<crate::agent::subagent::AgentManager>>,
    pub agent_activity_rx:
        Option<tokio::sync::watch::Receiver<Vec<crate::agent::subagent::AgentActivitySnapshot>>>,
    pub agents_request: Option<String>,
    pub agent_activity_action_pending: bool,
    pub review_request: Option<String>,
    pub review_in_flight: bool,
    pub review_records: Vec<crate::work::ReviewRecord>,
    pub review_convergence: Option<ReviewConvergenceUiRecord>,
    pub acceptance_request: Option<AcceptanceUiRequest>,
    pub acceptance_running: bool,
    pub job_owner: crate::jobs::JobOwner,
    pub pending_workspace_root: Option<std::path::PathBuf>,
    pub vim_mode: bool,
    pub vim_state: VimMode,
    pub turn_start: Option<std::time::Instant>,
    pub plan_proposal: Option<crate::work::PlanReview>,
    pub plan_reader: Option<crate::work::PlanReview>,
    pub plan_reader_max_scroll: std::cell::Cell<u16>,
    pub plan_execute_requested: Option<crate::work::PlanReview>,
    pub plan_turn_start_revision: Option<u64>,
    pub work_state: std::sync::Arc<std::sync::Mutex<crate::work::WorkState>>,
    pub task_graph_manager: Option<TaskGraphManagerState>,
    pub agent_graph_busy: bool,
    pub agent_graph_cancel_pending: bool,
    pub multi_run_manager: Option<MultiRunManagerState>,
    pub multi_run_stashed: Option<crate::multi_run::MultiRunState>,
    pub multi_run_generation: u64,
    pub pager_open: bool,
    pub pager_content: String,
    pub pager_scroll: u16,
    pub pager_selection: crate::tui::transcript::TranscriptScrollState,
    pub audit_requested: bool,
    pub audit_running: bool,
    pub audit_generation: u64,
    pub support_bundle_requested: bool,
    pub support_bundle_since: Option<chrono::DateTime<chrono::Utc>>,
    pub support_bundle_running: bool,
    pub support_bundle_generation: u64,
    pub support_bundle_confirmation: Option<SupportBundleConfirmation>,
    pub setup_import_request: Option<SetupImportRequest>,
    pub setup_import_running: bool,
    pub setup_import_generation: u64,
    pub setup_import_confirmation: Option<SetupImportConfirmation>,
    pub provider_usage_requested: bool,
    pub provider_usage_running: bool,
    pub provider_usage_generation: u64,
    pub(crate) provider_usage_cache: crate::provider_usage::ProviderAccountUsageCache,
    pub raw_transcript_requested: bool,
    pub provider_trace: Option<crate::provider_trace::ProviderTraceHub>,
    pub trace_delete_confirmation: Option<TraceDeleteConfirmation>,
    pub project_init: Option<ProjectInitState>,
    pub project_init_requested: bool,
    pub project_init_generation: u64,
    pub yank_buffer: String,
    pub show_line_numbers: bool,
    pub ghost_text: Option<String>,
    pub interaction: Option<crate::interaction::InteractionState>,
    pub pending_privilege_secret: Option<crate::privilege::PendingPrivilegeSecret>,
    pub manual_resolutions: crate::interaction::ManualResolutionQueue,
    pub kill_ring: Vec<String>,
    pub kill_ring_idx: Option<usize>,
    pub input_queue: crate::queue::InputQueue,
    pub restored_queue_ids: std::collections::HashSet<uuid::Uuid>,
    pub queue_view_open: bool,
    pub turn_lifecycle: crate::activity::TurnLifecycle,
    pub keep_awake_lease: Option<crate::keep_awake::KeepAwakeLease>,
    pub keep_awake_process_override: bool,
    pub session_sitting: crate::session_metrics::SessionSittingMetrics,
    pub soft_pause: crate::activity::SoftPauseState,
    pub loop_guard: crate::loop_guard::LoopGuard,
    pub pending_loop_incident: Option<crate::loop_guard::LoopIncident>,
    pub loop_guard_interaction: Option<crate::loop_guard::LoopGuardInteraction>,
    pub loop_guard_replan_prompt: Option<String>,
    pub active_turn_start_message_count: Option<usize>,
    pub retry_boundary: Option<RetryBoundary>,
    pub(crate) turn_recovery: crate::turn_recovery::State,
    pub retry_requested: bool,
    pub active_turn_route: Option<crate::fallback::TurnProviderRoute>,
    pub fallback_provenance: Vec<crate::fallback::TurnProviderProvenance>,
    pub external_editor_pending: Option<String>,
    pub session_name: String,
    pub session_title_generation: u64,
    pub session_title_pending: bool,
    pub session_title_running: bool,
    pub current_session: Option<crate::session::SessionV2>,
    pub(crate) session_store: Option<crate::session::SessionStore>,
    pub(crate) session_writer: Option<crate::session_access::SessionWriterLease>,
    pub(crate) session_access_generation: u64,
    pub(crate) workspace_activity: Option<crate::recovery::WorkspaceActivityLease>,
    pub(crate) memory_turn_generation: u64,
    pub(crate) memory_turn_in_flight: bool,
    pub(crate) memory_turn_abort: Option<tokio::task::AbortHandle>,
    pub(crate) prepared_memory_query: Option<(String, String)>,
    pub(crate) session_access_in_flight: bool,
    pub(crate) hook_runner: Option<crate::hooks::HookRunner>,
    pub artifact_ledger: crate::session_artifacts::SessionArtifactLedger,
    pub session_history: Option<SessionHistoryState>,
    pub copy_all_after_history: bool,
    pub session_load_generation: u64,
    pub(crate) session_load_cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    pub session_load_busy: bool,
    pub new_session_starting: bool,
    pub input_height: u16,
    pub approval_queue: ApprovalQueue,
    pub expanded_blocks: Vec<TranscriptBlockKey>,
    pub session_picker_open: bool,
    pub dashboard: crate::tui::dashboard::DashboardState,
    pub automation_ui: crate::automation_ui::AutomationUiState,
    pub usage_modal: Option<crate::tui::dashboard::UsageModalState>,
    pub pending_composer_submit: bool,
    pub plan_feedback_dispatch: Option<uuid::Uuid>,
    pub(crate) selected_plan_feedback: Option<(
        crate::work::PlanReview,
        crate::plan_feedback::FeedbackAnchor,
    )>,
    pub(crate) session_action_request: Option<crate::session_navigation::SessionActionRequest>,
    pub(crate) session_action_generation: u64,
    pub(crate) session_action_busy: bool,
    pub(crate) recovery_preview: Option<crate::recovery::RecoveryPreview>,
    pub(crate) branch_handoff_request: Option<uuid::Uuid>,
    pub(crate) branch_handoff_preview: Option<crate::branch_handoff::HandoffPreview>,
    pub session_list: Vec<crate::session::SessionSummary>,
    pub session_archive_list: Vec<crate::session::SessionSummary>,
    pub session_picker_view: SessionPickerView,
    pub selected_session_idx: usize,
    pub session_filter: String,
    pub session_time_offset_seconds: i32,
    pub session_picker_error: Option<String>,
    pub session_picker_busy: bool,
    pub session_management_generation: u64,
    pub session_archive_confirmation: Option<SessionArchiveConfirmation>,
    pub session_delete_confirmation: Option<SessionDeleteConfirmation>,
    pub session_rename: Option<SessionRenameState>,
    pub session_portability: Option<SessionPortabilityState>,
    pub session_portability_request: Option<SessionPortabilityRequest>,
    pub session_portability_generation: u64,
    pub session_retention: Option<SessionRetentionState>,
    pub session_retention_request: Option<SessionRetentionRequest>,
    pub session_retention_generation: u64,
    pub timeline_open: bool,
    pub timeline_entries: Vec<crate::session::TimelineEntry>,
    pub selected_timeline_idx: usize,
    pub timeline_mode: TimelineMode,
    pub cursor_blink_visible: bool,
    pub cursor_blink_ticks: u8,
    pub undo_stack: crate::undo::UndoStack,
    pub undo_request: Option<crate::undo::UndoDirection>,
    pub undo_busy: bool,
    pub undo_generation: u64,
    pub token_count: (u32, u32), // (session_total_prompt, session_total_completion)
    pub usage_ledger: std::sync::Arc<std::sync::Mutex<crate::usage::UsageLedger>>,
    pub official_pricing: Option<crate::pricing::OfficialPriceCatalog>,
    pub request_store: crate::request::RequestStore,
    pub context_checkpoint: Option<crate::context::ContextCheckpoint>,
    pub pending_context_checkpoint: Option<crate::context::ContextCheckpoint>,
    pub context_breakdown: Option<crate::context::ContextBreakdown>,
    pub current_context_tokens: u64,
    pub checkpoint_in_flight: bool,
    pub(crate) checkpoint_abort: Option<tokio::task::AbortHandle>,
    pub(crate) checkpoint_writer: Option<crate::session_access::SessionWriterLease>,
    pub(crate) checkpoint_hook_owner: Option<crate::hooks::HookOwner>,
    pub checkpoint_request: Option<crate::context::ContextCheckpointRequest>,
    pub last_auto_checkpoint_attempt: Option<usize>,
    pub checkpoint_generation: u64,
    pub change_review: crate::changes::ChangeReviewState,
    pub changes_load_requested: bool,
    pub change_walkthrough: Option<crate::change_walkthrough::ChangeWalkthroughState>,
    pub change_walkthrough_requested: bool,
    pub change_walkthrough_generation: u64,
    pub jobs_view_open: bool,
    pub job_list: Vec<crate::jobs::JobInfo>,
    pub watch_list: Vec<ca_watch::WatchSnapshotV1>,
    pub delegation_list: Vec<crate::mechanistic_delegation::DelegationActivitySnapshot>,
    pub mechanistic_request: Option<MechanisticUiRequest>,
    pub mechanistic_changed_paths: std::collections::BTreeSet<String>,
    pub mechanistic_unscoped_mutation: bool,
    pub mechanistic_repository_revision_before: Option<String>,
    pub mechanistic_verification_context: Option<String>,
    pub mechanistic_verification_in_flight: bool,
    pub mechanistic_verification_failed: bool,
    pub mechanistic_repair_passes: u8,
    pub mechanistic_verification_suppressed: bool,
    pub mechanistic_release_requested: bool,
    pub mechanistic_evidence: crate::mechanistic_delegation::EvidenceLedger,
    pub mechanistic_checkpoint: Option<crate::session::MechanisticCheckpointV1>,
    pub mechanistic_capsules: crate::session::MechanisticCapsuleIndexV1,
    pub mechanistic_pending_capsule: Option<codex_team_protocol::MechanisticEvidenceCapsuleV1>,
    pub mechanistic_resume_requested: bool,
    pub mechanistic_provider_continuation_authorized: bool,
    pub mechanistic_preflight_pending_assessment: bool,
    pub delegation_correction_confirmation: Option<String>,
    pub latest_background_activity: Option<BackgroundActivityRef>,
    pub selected_job_idx: usize,
    pub job_detail_open: bool,
    pub job_scroll: u16,
    pub job_stdin_target: Option<crate::jobs::JobId>,
    pub foreground_job: Option<crate::jobs::JobId>,
    pub foreground_promotion_armed_at: Option<(crate::jobs::JobId, std::time::Instant)>,
    pub jobs_refresh_requested: bool,
    pub terminal_manager: Option<TerminalManagerState>,
    pub terminal_refresh_requested: bool,
    pub ui_theme: crate::ui_settings::UiTheme,
    pub terminal_theme_auto: bool,
    pub terminal_appearance: Option<crate::terminal_awareness::TerminalAppearance>,
    pub theme_sync_requested: bool,
    pub terminal_focused: bool,
    pub terminal_hyperlinks_supported: bool,
    pub status_indicators_enabled: bool,
    pub notification_style: crate::ui_settings::NotificationStyle,
    pub mouse_mode: crate::ui_settings::MouseMode,
    pub ui_compact_mode: bool,
    pub command_output_policy: crate::config::CommandOutputPolicy,
    pub command_output_picker: Option<CommandOutputPickerState>,
    pub command_presentations:
        std::collections::HashMap<String, crate::command_output::SemanticPresentation>,
    pub file_activity_presentations: std::collections::HashMap<String, FileActivityPresentation>,
    pub latest_semantic_command: Option<crate::command_output::FullOutputRerun>,
    pub output_full_confirmation: Option<OutputFullConfirmation>,
    pub scratch_delete_confirmation: Option<ScratchDeleteConfirmation>,
    pub statusline_fields: Vec<crate::ui_settings::StatusField>,
    pub statusline_custom_configured: bool,
    pub statusline_custom_enabled: bool,
    pub statusline_refresh_secs: u64,
    pub latest_generation_speed_tenths: Option<u64>,
    pub sitting_cost: crate::usage::SittingCostPresentation,
    pub status_git_branch: Option<String>,
    pub theme_manager: Option<ThemeManagerState>,
    pub notification_picker: Option<NotificationPickerState>,
    pub copy_picker: Option<CopyPickerState>,
    pub statusline_manager: Option<StatuslineManagerState>,
    pub status_command: StatusCommandState,
    pub shell_history: crate::context::ShellHistory,
    pub status_set_at: Option<std::time::Instant>,
    pub stream_generation: u64,
    pub file_picker_open: bool,
    pub file_picker_filter: String,
    pub file_picker_phase: crate::attachments::PickerPhase,
    pub file_picker_generation: u64,
    pub file_picker_discovered: usize,
    pub file_picker_truncated: bool,
    pub file_picker_error: Option<String>,
    pub file_picker_index: std::sync::Arc<Vec<crate::attachments::IndexedFile>>,
    pub file_picker_index_ready: bool,
    pub file_picker_resources: Vec<crate::mcp::McpResourceInfo>,
    pub file_picker_entries: Vec<crate::attachments::PickerEntry>,
    pub file_picker_matches: usize,
    pub attachment_frecency: crate::attachments::FrecencyMap,
    pub attachment_frecency_generation: u64,
    pub composer_attachments: Vec<crate::attachments::ComposerAttachment>,
    pub pending_attachment_context: Option<String>,
    pub active_attachment_context: Option<String>,
    pub attachment_resolution_generation: u64,
    pub attachment_resolution_busy: bool,
    pub attachment_resolution_raw: Option<String>,
    pub selected_file_idx: usize,
    pub pasted_images: Vec<crate::image_content::ModelImage>,
    pub transient_tool_images: std::collections::HashMap<String, crate::image_content::ModelImage>,
    pub image_attachment_busy: bool,
    pub image_attachment_generation: u64,
    pub workspace_trust_prompt: bool,
    pub hook_context: Vec<String>,
    pub memory_recall: crate::memory::MemoryRecall,
    pub goal_state: Option<crate::goal::GoalState>,
    pub goal_evaluating: bool,
    pub goal_generation: u64,
    pub goal_continuation_prompt: Option<String>,
    pub goal_resume_requested: bool,
    pub workflow_preview: Option<crate::workflow::WorkflowPreview>,
    pub workflow_run: Option<crate::workflow::WorkflowRun>,
    pub workflow_loaded: Option<crate::workflow::LoadedWorkflow>,
    pub workflow_execution: Option<crate::workflow::WorkflowExecutionContext>,
    pub workflow_control_prompt: Option<crate::workflow::WorkflowControlPrompt>,
    pub workflow_usable_output: bool,
    pub workflow_tool_started: bool,
    pub workflow_start_requested: bool,
    pub workflow_next_requested: bool,
    pub workflow_clear_confirmation: Option<bool>,
    pub workflow_clear_requested: bool,
    pub workflow_pause_requested: bool,
    pub workflow_builtin_request: Option<crate::workflow::BuiltinWorkflowRequest>,
    pub workflow_builtin_busy: bool,
    pub worktree_store: Option<crate::worktree::WorktreeStore>,
    pub attached_worktree: Option<crate::worktree::WorktreeRecord>,
    pub worktree_restore_skills: Option<Vec<crate::skills::ActiveSkillRef>>,
    pub worktree_request: Option<crate::worktree::WorktreeActionRequest>,
    pub worktree_generation: u64,
    pub worktree_busy: bool,
    pub worktree_cleanup_confirmation: Option<crate::worktree::WorktreeCleanupConfirmation>,
    pub worktree_setup_confirmation: Option<crate::worktree_setup::SetupConfirmation>,
    /// Tracks user session outcomes for harness improvement telemetry.
    pub outcome_tracker: crate::session_outcome::OutcomeTracker,
}

impl AppState {
    pub fn model_is_configured_only(&self, model: &ModelInfo) -> bool {
        self.codex_catalogs
            .get(&model.provider)
            .is_some_and(|models| !models.contains(&model.id))
    }

    pub fn show_toast(&mut self, message: impl Into<String>, kind: ToastKind) {
        self.toast = Some(UiToast::new(message, kind));
    }

    pub fn former_active_session_count(&self) -> usize {
        let current_id = self.current_session.as_ref().map(|session| session.id);
        self.session_list
            .iter()
            .filter(|summary| Some(summary.id) != current_id)
            .count()
    }

    pub fn has_previous_sessions(&self) -> bool {
        self.former_active_session_count() > 0 || !self.session_archive_list.is_empty()
    }

    pub fn session_picker_visible_summaries(&self) -> Vec<&crate::session::SessionSummary> {
        let active = self.session_picker_view == SessionPickerView::Active;
        let current_id = self.current_session.as_ref().map(|session| session.id);
        let summaries = if active {
            &self.session_list
        } else {
            &self.session_archive_list
        };
        summaries
            .iter()
            .filter(|summary| !active || Some(summary.id) != current_id)
            .filter(|summary| summary.matches(&self.session_filter))
            .collect()
    }

    /// Take the composer payload at a dispatch or persistence boundary.
    /// Registered large-paste markers are expanded by range before the draft's
    /// process-only UI state is cleared.
    pub fn take_composer_text(&mut self) -> String {
        let expanded = self.composer_ui.expanded_text(&self.input);
        self.input.clear();
        self.input_cursor = 0;
        self.composer_ui.clear_for_new_draft();
        expanded
    }

    pub fn expanded_composer_text(&self) -> String {
        self.composer_ui.expanded_text(&self.input)
    }

    /// Return all blinking cursors to their visible phase after user input.
    pub fn reset_cursor_blink(&mut self) {
        self.cursor_blink_visible = true;
        self.cursor_blink_ticks = 0;
    }

    /// Advance the shared 500 ms cursor phase from the TUI's 50 ms idle tick.
    pub fn advance_cursor_blink(&mut self) {
        self.cursor_blink_ticks = self.cursor_blink_ticks.saturating_add(1);
        if self.cursor_blink_ticks >= 10 {
            self.cursor_blink_ticks = 0;
            self.cursor_blink_visible = !self.cursor_blink_visible;
        }
    }

    /// Build a fresh interactive pane from process-owned configuration while leaving all
    /// conversation, approval, goal, workflow, job, queue, undo, and session state isolated.
    pub fn new_pane_from(&self) -> Self {
        let mut pane = Self::new(self.shell_history.clone());
        pane.pane_deck = None;
        pane.runtime_diagnostics = self.runtime_diagnostics.clone();
        pane.active_profile = self.active_profile.clone();
        pane.tool_policy = self.tool_policy.clone();
        pane.active_tool_names = self.active_tool_names.clone();
        pane.config_path = self.config_path.clone();
        pane.run_capabilities = self.run_capabilities;
        pane.effective_sandbox_mode = self.effective_sandbox_mode;
        pane.mode = self.mode.clone();
        pane.approval = self.approval.clone();
        pane.model_list = self.model_list.clone();
        pane.delegation_model_routes = self.delegation_model_routes.clone();
        pane.current_model = self.current_model.clone();
        pane.reasoning_effort = self.reasoning_effort;
        pane.model_preferences = self.model_preferences.clone();
        pane.model_preferences_store = self.model_preferences_store.clone();
        pane.discovered_models = self.discovered_models.clone();
        pane.codex_catalogs = self.codex_catalogs.clone();
        pane.model_catalog_loaded = self.model_catalog_loaded;
        pane.model_catalog_status = self.model_catalog_status.clone();
        pane.model_catalog_provider_status = self.model_catalog_provider_status.clone();
        pane.provider_checks = self.provider_checks.clone();
        pane.keymap = self.keymap.clone();
        pane.keymap_customized = self.keymap_customized.clone();
        pane.prompt_stash_store = self.prompt_stash_store.clone();
        pane.prompt_commands = self.prompt_commands.clone();
        pane.rules = self.rules.clone();
        pane.rules_home = self.rules_home.clone();
        pane.exclude_workspace_context = self.exclude_workspace_context;
        pane.skill_bundles = self.skill_bundles.clone();
        pane.learning_ledger = self.learning_ledger.clone();
        pane.skill_curator = self.skill_curator.clone();
        pane.cwd = self.cwd.clone();
        pane.workspace = self.workspace.clone();
        pane.workspace_catalog = self.workspace_catalog.clone();
        pane.skills = self.skills.clone();
        pane.mcp = self.mcp.clone();
        pane.lsp = self.lsp.clone();
        pane.email = self.email.clone();
        pane.direnv_allowed = self.direnv_allowed;
        pane.user_memory_store = self.user_memory_store.clone();
        pane.workspace_memory_store = self.workspace_memory_store.clone();
        pane.user_memory_records = self.user_memory_records.clone();
        pane.workspace_memory_records = self.workspace_memory_records.clone();
        pane.memory_recall = self.memory_recall.clone();
        pane.worktree_store = self.worktree_store.clone();
        pane.provider_trace = self.provider_trace.clone();
        pane.show_line_numbers = self.show_line_numbers;
        pane.show_thinking = self.show_thinking;
        pane.show_transcript_timestamps = self.show_transcript_timestamps;
        pane.show_assistant_footers = self.show_assistant_footers;
        pane.vim_mode = self.vim_mode;
        pane.ui_theme = self.ui_theme;
        pane.terminal_theme_auto = self.terminal_theme_auto;
        pane.terminal_appearance = self.terminal_appearance;
        pane.terminal_focused = self.terminal_focused;
        pane.terminal_hyperlinks_supported = self.terminal_hyperlinks_supported;
        pane.status_indicators_enabled = self.status_indicators_enabled;
        pane.notification_style = self.notification_style;
        pane.mouse_mode = self.mouse_mode;
        pane.command_output_policy = self.command_output_policy;
        pane.statusline_fields = self.statusline_fields.clone();
        pane.statusline_custom_configured = self.statusline_custom_configured;
        pane.statusline_custom_enabled = self.statusline_custom_enabled;
        pane.statusline_refresh_secs = self.statusline_refresh_secs;
        pane.status_git_branch = self.status_git_branch.clone();
        pane.keep_awake_process_override = self.keep_awake_process_override;
        pane.usage_ledger = self.usage_ledger.clone();
        pane.official_pricing = self.official_pricing.clone();
        pane.request_store = self.request_store.clone();
        pane.session_time_offset_seconds = self.session_time_offset_seconds;
        pane.session_name = "New pane".into();
        pane.status_message = Some("New isolated session pane".into());
        pane
    }

    pub fn new(shell_history: crate::context::ShellHistory) -> Self {
        Self {
            pane_id: crate::panes::PaneId::PRIMARY,
            pane_deck: Some(Box::new(crate::panes::PaneDeck::single())),
            pane_request: None,
            pane_split_generation: 0,
            pane_split_busy: false,
            pane_split_loaded: None,
            pane_close_generation: 0,
            pane_close_busy: false,
            pane_close_committed: false,
            runtime_diagnostics: crate::runtime_diagnostics::RuntimeDiagnostics::default(),
            debug_render_overlay: false,
            active_profile: crate::runtime::ActiveProfile::Default,
            tool_policy: crate::tool_selection::ToolSelectionPolicy::default(),
            active_tool_names: std::collections::HashSet::new(),
            owned_tools: crate::owned_tools::OwnedToolContext::default(),
            retired_mutations: Vec::new(),
            recovery_retiring: std::collections::HashSet::new(),
            discovery_batch: None,
            config_path: None,
            run_capabilities: crate::runtime::RunCapabilities::for_profile(
                &crate::runtime::ActiveProfile::Default,
            ),
            effective_sandbox_mode: crate::sandbox::SandboxMode::Required,
            mode: AgentMode::Agent,
            approval: ApprovalMode::Allowlist,
            session_grants: std::sync::Arc::new(std::sync::Mutex::new(
                crate::permission::SessionGrantStore::default(),
            )),
            input: String::new(),
            composer_ui: crate::tui::composer::ComposerUiState::default(),
            input_history: Vec::new(),
            history_idx: None,
            messages: Vec::new(),
            transcript_annotations: Vec::new(),
            show_transcript_timestamps: false,
            show_assistant_footers: false,
            transcript_agent_summaries: Vec::new(),
            active_agent_turn_id: None,
            live_agent_activity: Vec::new(),
            pending_agent_completions: VecDeque::new(),
            pending_job_completions: VecDeque::new(),
            agent_activity_focus: None,
            streaming: None,
            active_turn_prompt: None,
            model_picker_open: false,
            model_picker_target: ModelPickerTarget::Current,
            model_filter: String::new(),
            model_list: Vec::new(),
            delegation_model_routes: std::collections::HashSet::new(),
            selected_model_idx: 0,
            current_model: None,
            model_preferences: crate::model_preferences::ModelPreferences::default(),
            model_preferences_store: crate::model_preferences::ModelPreferencesStore::unavailable(),
            one_turn_route: None,
            route_preset_picker: None,
            route_preset_overwrite: None,
            discovered_models: std::collections::HashMap::new(),
            codex_catalogs: std::collections::HashMap::new(),
            model_catalog_refresh_requested: false,
            model_catalog_refresh_provider: None,
            model_catalog_refreshing: false,
            model_catalog_loaded: false,
            model_catalog_generation: 0,
            model_catalog_status: None,
            model_catalog_provider_status: std::collections::HashMap::new(),
            provider_checks: std::collections::HashMap::new(),
            provider_check_generation: 0,
            provider_check_running: None,
            provider_manager: None,
            provider_auth: None,
            provider_auth_generation: 0,
            provider_editor: None,
            provider_discovery: None,
            provider_discovery_requested: false,
            provider_discovery_generation: 0,
            btw: None,
            btw_start_requested: false,
            btw_generation: 0,
            voice: None,
            voice_generation: 0,
            config_hub: None,
            sandbox_hub: None,
            context_limit_editor: None,
            model_prompt_cache_editor: None,
            keymap: crate::keymap::Keymap::default(),
            keymap_customized: std::collections::HashSet::new(),
            keymap_hub: None,
            history_search: None,
            prompt_stash_store: crate::prompt_stash::PromptStashStore::unavailable(),
            prompt_stash: None,
            pending_key_chord: None,
            reasoning_effort: None,
            reasoning_picker: None,
            show_thinking: false,
            slash_palette_open: false,
            slash_filter: String::new(),
            selected_slash_idx: 0,
            prompt_commands: crate::prompt_commands::PromptCommandRegistry::default(),
            rules: crate::rules::RuleRegistry::default(),
            manual_rule_ids: std::collections::BTreeSet::new(),
            explicit_rule_paths: Vec::new(),
            rules_home: None,
            exclude_workspace_context: false,
            pending_git_context: None,
            asset_mutation_preview: None,
            skill_bundles: crate::skill_bundles::SkillBundleRegistry::default(),
            learning_ledger: None,
            skill_curator: None,
            active_skill_invocation_group: None,
            pending_bundle_invocation: None,
            recall_request: None,
            recall_busy: false,
            recall_generation: 0,
            pending_input_action: None,
            status_message: None,
            toast: None,
            should_quit: false,
            transcript_scroll: crate::tui::transcript::TranscriptScrollState::default(),
            transcript_cache: crate::tui::render::TranscriptCache::default(),
            viewport_width: 80,
            viewport_height: 0,
            input_cursor: 0,
            cwd: String::new(),
            workspace: crate::workspace::WorkspaceContext::uninitialized(),
            workspace_catalog: None,
            skills: std::sync::Arc::new(std::sync::Mutex::new(
                crate::skills::SkillRegistry::empty(),
            )),
            task_skill_loads: crate::skills::SkillLoadSet::default(),
            task_skill_snapshot: std::sync::Arc::new(std::sync::Mutex::new(String::new())),
            mcp: None,
            lsp: None,
            email: None,
            mcp_request: None,
            mcp_task_active: false,
            mcp_cancel_requested: false,
            mcp_generation: 0,
            direnv_request: None,
            direnv_preview: None,
            direnv_busy: false,
            direnv_generation: 0,
            direnv_allowed: true,
            learned_recipe_request: None,
            learned_recipe_review: None,
            learning_review_request: None,
            learning_review_view: None,
            learning_review_progress: None,
            learning_review_transcript_index: None,
            learning_review_busy: false,
            learning_review_generation: 0,
            learning_review_pager_open: false,
            learning_review_error: None,
            learning_review_interaction: false,
            self_improve_request: None,
            self_improve_review: None,
            agents: None,
            agent_activity_rx: None,
            agents_request: None,
            agent_activity_action_pending: false,
            review_request: None,
            review_in_flight: false,
            review_records: Vec::new(),
            review_convergence: None,
            acceptance_request: None,
            acceptance_running: false,
            job_owner: crate::jobs::JobOwner::new(None),
            pending_workspace_root: None,
            vim_mode: false,
            vim_state: VimMode::Normal,
            turn_start: None,
            plan_proposal: None,
            plan_reader: None,
            plan_reader_max_scroll: std::cell::Cell::new(0),
            plan_execute_requested: None,
            plan_turn_start_revision: None,
            work_state: std::sync::Arc::new(std::sync::Mutex::new(
                crate::work::WorkState::default(),
            )),
            task_graph_manager: None,
            agent_graph_busy: false,
            agent_graph_cancel_pending: false,
            multi_run_manager: None,
            multi_run_stashed: None,
            multi_run_generation: 0,
            pager_open: false,
            pager_content: String::new(),
            pager_scroll: 0,
            pager_selection: crate::tui::transcript::TranscriptScrollState::default(),
            audit_requested: false,
            audit_running: false,
            audit_generation: 0,
            support_bundle_requested: false,
            support_bundle_since: None,
            support_bundle_running: false,
            support_bundle_generation: 0,
            support_bundle_confirmation: None,
            setup_import_request: None,
            setup_import_running: false,
            setup_import_generation: 0,
            setup_import_confirmation: None,
            provider_usage_requested: false,
            provider_usage_running: false,
            provider_usage_generation: 0,
            provider_usage_cache: crate::provider_usage::ProviderAccountUsageCache::default(),
            raw_transcript_requested: false,
            provider_trace: None,
            trace_delete_confirmation: None,
            project_init: None,
            project_init_requested: false,
            project_init_generation: 0,
            yank_buffer: String::new(),
            show_line_numbers: false,
            ghost_text: None,
            interaction: None,
            pending_privilege_secret: None,
            manual_resolutions: crate::interaction::ManualResolutionQueue::default(),
            kill_ring: Vec::new(),
            kill_ring_idx: None,
            input_queue: crate::queue::InputQueue::default(),
            restored_queue_ids: std::collections::HashSet::new(),
            queue_view_open: false,
            turn_lifecycle: crate::activity::TurnLifecycle::default(),
            keep_awake_lease: None,
            keep_awake_process_override: false,
            session_sitting: crate::session_metrics::SessionSittingMetrics::default(),
            soft_pause: crate::activity::SoftPauseState::default(),
            loop_guard: crate::loop_guard::LoopGuard::default(),
            pending_loop_incident: None,
            loop_guard_interaction: None,
            loop_guard_replan_prompt: None,
            active_turn_start_message_count: None,
            retry_boundary: None,
            turn_recovery: crate::turn_recovery::State::default(),
            retry_requested: false,
            active_turn_route: None,
            fallback_provenance: Vec::new(),
            external_editor_pending: None,
            session_name: String::new(),
            session_title_generation: 0,
            session_title_pending: false,
            session_title_running: false,
            current_session: None,
            session_store: None,
            session_writer: None,
            session_access_generation: 0,
            workspace_activity: None,
            memory_turn_generation: 0,
            memory_turn_in_flight: false,
            memory_turn_abort: None,
            prepared_memory_query: None,
            session_access_in_flight: false,
            hook_runner: None,
            artifact_ledger: crate::session_artifacts::SessionArtifactLedger::default(),
            session_history: None,
            copy_all_after_history: false,
            session_load_generation: 0,
            session_load_cancel: None,
            session_load_busy: false,
            new_session_starting: false,
            input_height: 3,
            approval_queue: ApprovalQueue::new(),
            expanded_blocks: Vec::new(),
            session_picker_open: false,
            dashboard: crate::tui::dashboard::DashboardState::default(),
            automation_ui: crate::automation_ui::AutomationUiState::default(),
            usage_modal: None,
            pending_composer_submit: false,
            plan_feedback_dispatch: None,
            selected_plan_feedback: None,
            session_action_request: None,
            session_action_generation: 0,
            session_action_busy: false,
            recovery_preview: None,
            branch_handoff_request: None,
            branch_handoff_preview: None,
            session_list: Vec::new(),
            session_archive_list: Vec::new(),
            session_picker_view: SessionPickerView::Active,
            selected_session_idx: 0,
            session_filter: String::new(),
            session_time_offset_seconds: chrono::Local::now().offset().local_minus_utc(),
            session_picker_error: None,
            session_picker_busy: false,
            session_management_generation: 0,
            session_archive_confirmation: None,
            session_delete_confirmation: None,
            session_rename: None,
            session_portability: None,
            session_portability_request: None,
            session_portability_generation: 0,
            session_retention: None,
            session_retention_request: None,
            session_retention_generation: 0,
            timeline_open: false,
            timeline_entries: Vec::new(),
            selected_timeline_idx: 0,
            timeline_mode: TimelineMode::Browse,
            cursor_blink_visible: true,
            cursor_blink_ticks: 0,
            undo_stack: crate::undo::UndoStack::new(),
            undo_request: None,
            undo_busy: false,
            undo_generation: 0,
            token_count: (0, 0),
            usage_ledger: std::sync::Arc::new(std::sync::Mutex::new(
                crate::usage::UsageLedger::default(),
            )),
            official_pricing: None,
            request_store: crate::request::RequestStore::default(),
            context_checkpoint: None,
            pending_context_checkpoint: None,
            context_breakdown: None,
            current_context_tokens: 0,
            checkpoint_in_flight: false,
            checkpoint_abort: None,
            checkpoint_writer: None,
            checkpoint_hook_owner: None,
            checkpoint_request: None,
            last_auto_checkpoint_attempt: None,
            checkpoint_generation: 0,
            change_review: crate::changes::ChangeReviewState::default(),
            changes_load_requested: false,
            change_walkthrough: None,
            change_walkthrough_requested: false,
            change_walkthrough_generation: 0,
            jobs_view_open: false,
            job_list: Vec::new(),
            watch_list: Vec::new(),
            delegation_list: Vec::new(),
            mechanistic_request: None,
            mechanistic_changed_paths: std::collections::BTreeSet::new(),
            mechanistic_unscoped_mutation: false,
            mechanistic_repository_revision_before: None,
            mechanistic_verification_context: None,
            mechanistic_verification_in_flight: false,
            mechanistic_verification_failed: false,
            mechanistic_repair_passes: 0,
            mechanistic_verification_suppressed: false,
            mechanistic_release_requested: false,
            mechanistic_evidence: crate::mechanistic_delegation::EvidenceLedger::default(),
            mechanistic_checkpoint: None,
            mechanistic_capsules: crate::session::MechanisticCapsuleIndexV1::default(),
            mechanistic_pending_capsule: None,
            mechanistic_resume_requested: false,
            mechanistic_provider_continuation_authorized: false,
            mechanistic_preflight_pending_assessment: false,
            delegation_correction_confirmation: None,
            latest_background_activity: None,
            selected_job_idx: 0,
            job_detail_open: false,
            job_scroll: 0,
            job_stdin_target: None,
            foreground_job: None,
            foreground_promotion_armed_at: None,
            jobs_refresh_requested: false,
            terminal_manager: None,
            terminal_refresh_requested: false,
            ui_theme: crate::ui_settings::UiTheme::Default,
            terminal_theme_auto: true,
            terminal_appearance: None,
            theme_sync_requested: false,
            terminal_focused: true,
            terminal_hyperlinks_supported: false,
            status_indicators_enabled: false,
            notification_style: crate::ui_settings::NotificationStyle::Off,
            mouse_mode: crate::ui_settings::MouseMode::Scroll,
            ui_compact_mode: false,
            command_output_policy: crate::config::CommandOutputPolicy::Bounded,
            command_output_picker: None,
            command_presentations: std::collections::HashMap::new(),
            file_activity_presentations: std::collections::HashMap::new(),
            latest_semantic_command: None,
            output_full_confirmation: None,
            scratch_delete_confirmation: None,
            statusline_fields: crate::ui_settings::StatuslineConfig::default().fields,
            statusline_custom_configured: false,
            statusline_custom_enabled: false,
            statusline_refresh_secs: crate::ui_settings::DEFAULT_STATUS_REFRESH_SECS,
            latest_generation_speed_tenths: None,
            sitting_cost: crate::usage::SittingCostPresentation::Unknown,
            status_git_branch: None,
            theme_manager: None,
            notification_picker: None,
            copy_picker: None,
            statusline_manager: None,
            status_command: StatusCommandState::default(),
            shell_history,
            status_set_at: None,
            stream_generation: 0,
            file_picker_open: false,
            file_picker_filter: String::new(),
            file_picker_phase: crate::attachments::PickerPhase::Idle,
            file_picker_generation: 0,
            file_picker_discovered: 0,
            file_picker_truncated: false,
            file_picker_error: None,
            file_picker_index: std::sync::Arc::new(Vec::new()),
            file_picker_index_ready: false,
            file_picker_resources: Vec::new(),
            file_picker_entries: Vec::new(),
            file_picker_matches: 0,
            attachment_frecency: std::collections::BTreeMap::new(),
            attachment_frecency_generation: 0,
            composer_attachments: Vec::new(),
            pending_attachment_context: None,
            active_attachment_context: None,
            attachment_resolution_generation: 0,
            attachment_resolution_busy: false,
            attachment_resolution_raw: None,
            selected_file_idx: 0,
            pasted_images: Vec::new(),
            transient_tool_images: std::collections::HashMap::new(),
            image_attachment_busy: false,
            image_attachment_generation: 0,
            workspace_trust_prompt: false,
            hook_context: Vec::new(),
            memory_recall: crate::memory::MemoryRecall::default(),
            goal_state: None,
            goal_evaluating: false,
            goal_generation: 0,
            goal_continuation_prompt: None,
            goal_resume_requested: false,
            workflow_preview: None,
            workflow_run: None,
            workflow_loaded: None,
            workflow_execution: None,
            workflow_control_prompt: None,
            workflow_usable_output: false,
            workflow_tool_started: false,
            workflow_start_requested: false,
            workflow_next_requested: false,
            workflow_clear_confirmation: None,
            workflow_clear_requested: false,
            workflow_pause_requested: false,
            workflow_builtin_request: None,
            workflow_builtin_busy: false,
            worktree_store: None,
            attached_worktree: None,
            worktree_restore_skills: None,
            worktree_request: None,
            worktree_generation: 0,
            worktree_busy: false,
            worktree_cleanup_confirmation: None,
            worktree_setup_confirmation: None,
            hook_manager: None,
            permission_manager: None,
            permission_manager_generation: 0,
            memory_manager: None,
            memory_manager_generation: 0,
            user_memory_store: None,
            workspace_memory_store: None,
            maintenance_request: None,
            user_memory_records: Vec::new(),
            workspace_memory_records: Vec::new(),
            memory_proposals: Vec::new(),
            hook_manager_request: None,
            hook_manager_generation: 0,
            hook_running: false,
            active_hook_lifecycle: None,
            active_hook_abort: None,
            hook_blocked: false,
            hook_blocked_event: None,
            outcome_tracker: crate::session_outcome::OutcomeTracker::new(String::new(), ""),
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new(crate::context::ShellHistory::new())
    }
}

#[cfg(test)]
mod tui127_tests {
    use super::*;

    fn streaming() -> StreamingState {
        StreamingState {
            content: String::new(),
            tool_calls: Vec::new(),
            thinking: None,
            response_items: Vec::new(),
            last_usage: None,
            usage_handle: crate::usage::UsageHandle::test_handle(),
            reasoning_started_at: None,
            reasoning_duration: None,
            generation_started_at: None,
        }
    }

    #[test]
    fn thought_phase_starts_once_and_freezes_at_first_boundary() {
        let start = std::time::Instant::now();
        let mut state = streaming();
        state.observe_reasoning_at(start);
        state.observe_reasoning_at(start + std::time::Duration::from_millis(40));
        state.finish_reasoning_at(start + std::time::Duration::from_millis(651));
        state.finish_reasoning_at(start + std::time::Duration::from_secs(2));

        assert_eq!(
            state.reasoning_duration,
            Some(std::time::Duration::from_millis(651))
        );
        assert_eq!(state.generation_started_at, Some(start));
    }

    #[test]
    fn missing_reasoning_never_invents_a_duration() {
        let start = std::time::Instant::now();
        let mut state = streaming();
        state.observe_generation_at(start);
        state.finish_reasoning_at(start + std::time::Duration::from_millis(20));

        assert_eq!(state.reasoning_duration, None);
        assert_eq!(
            state.generation_duration_at(start + std::time::Duration::from_millis(80)),
            Some(std::time::Duration::from_millis(80))
        );
    }

    #[test]
    fn direct_completion_without_provider_output_has_unknown_timing() {
        let mut state = streaming();
        state.finish_reasoning_at(std::time::Instant::now());

        assert_eq!(state.reasoning_duration, None);
        assert_eq!(state.generation_started_at, None);
        assert_eq!(state.visible_reasoning_duration(), None);
    }
}
