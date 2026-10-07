use crate::agent::client::{ChatMessage, ChatRequestOptions, ToolDef};
use crate::context::ContextBreakdown;
use serde_json::Value;
use std::ops::Range;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

pub const REQUEST_SNAPSHOT_RENDER_LIMIT: usize = 64 * 1024;
const SNAPSHOT_TEXT_BUDGET: usize = 48 * 1024;
const SNAPSHOT_FRAGMENT_LIMIT: usize = 8 * 1024;
const SNAPSHOT_ITEM_LIMIT: usize = 64;
const SNAPSHOT_ID_LIMIT: usize = 256;
const SNAPSHOT_JSON_DEPTH_LIMIT: usize = 32;
const SNAPSHOT_JSON_NODE_LIMIT: usize = 2_048;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RequestKind {
    MainTurn,
    ToolContinuation,
    SideQuestion,
    SessionTitle,
    ContextCheckpoint,
    BranchHandoff,
    ProjectInit,
    Review,
    Subagent,
    RoleRun,
    #[default]
    Headless,
    HeadlessCheck,
    Scripted,
    ProviderProbe,
    GoalEvaluation,
    WorkflowStep,
    MultiRunCandidate,
    MultiRunJudge,
    MultiRunFusion,
    ChangeWalkthrough,
    Dream,
    Distill,
    Analyze,
    Service,
    Scheduled,
    EmailGateway,
    Compatibility,
}

impl RequestKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::MainTurn => "main turn",
            Self::ToolContinuation => "tool continuation",
            Self::SideQuestion => "side question",
            Self::SessionTitle => "session title",
            Self::ContextCheckpoint => "context checkpoint",
            Self::BranchHandoff => "branch handoff",
            Self::ProjectInit => "project initialization",
            Self::Review => "review",
            Self::Subagent => "subagent",
            Self::RoleRun => "role run",
            Self::Headless => "headless",
            Self::HeadlessCheck => "headless check",
            Self::Scripted => "scripted test",
            Self::ProviderProbe => "provider check",
            Self::GoalEvaluation => "goal evaluation",
            Self::WorkflowStep => "workflow step",
            Self::MultiRunCandidate => "multi-run candidate",
            Self::MultiRunJudge => "multi-run judge",
            Self::MultiRunFusion => "multi-run fusion",
            Self::ChangeWalkthrough => "change walkthrough",
            Self::Dream => "dream maintenance",
            Self::Distill => "distill maintenance",
            Self::Analyze => "analyze maintenance",
            Self::Service => "stdio service",
            Self::Scheduled => "scheduled run",
            Self::EmailGateway => "email gateway",
            Self::Compatibility => "compatibility request",
        }
    }
}

/// Runtime-only namespace for provider prompt-cache routing. The UUID is never
/// serialized or sent directly; adapters receive only a derived digest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptCacheDomain {
    Conversation,
    SideQuestion,
    Agent,
    Isolated(RequestKind),
}

impl PromptCacheDomain {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Conversation => "conversation",
            Self::SideQuestion => "side_question",
            Self::Agent => "agent",
            Self::Isolated(_) => "isolated",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct PromptCacheScope {
    scope_id: Uuid,
    domain: PromptCacheDomain,
}

impl PromptCacheScope {
    pub const fn conversation(scope_id: Uuid) -> Self {
        Self {
            scope_id,
            domain: PromptCacheDomain::Conversation,
        }
    }

    pub const fn side_question(scope_id: Uuid) -> Self {
        Self {
            scope_id,
            domain: PromptCacheDomain::SideQuestion,
        }
    }

    pub const fn agent(scope_id: Uuid) -> Self {
        Self {
            scope_id,
            domain: PromptCacheDomain::Agent,
        }
    }

    pub const fn isolated(scope_id: Uuid, kind: RequestKind) -> Self {
        Self {
            scope_id,
            domain: PromptCacheDomain::Isolated(kind),
        }
    }

    pub(crate) const fn scope_id(self) -> Uuid {
        self.scope_id
    }

    pub(crate) const fn domain(self) -> PromptCacheDomain {
        self.domain
    }
}

impl std::fmt::Debug for PromptCacheScope {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PromptCacheScope")
            .field("scope_id", &"[REDACTED]")
            .field("domain", &self.domain)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PromptCacheIntent {
    pub scope: PromptCacheScope,
    /// Count of leading semantic messages that form the explicit stable epoch.
    /// A provider adapter may mark only the final message in this prefix.
    pub stable_prefix_messages: usize,
}

#[derive(Debug, Clone)]
pub struct PromptAssembly {
    pub messages: Vec<ChatMessage>,
    pub stable_prefix_messages: usize,
    pub segments: Vec<PromptSegment>,
}

impl PromptAssembly {
    /// Reconcile typed lifetime ranges after one-request messages were
    /// inserted into the already assembled prompt. `inserted_indices` are
    /// final-message indices in ascending order.
    pub(crate) fn mark_inserted_messages(
        &mut self,
        inserted_indices: &[usize],
        lifetime: PromptSegmentLifetime,
    ) {
        if inserted_indices.is_empty() {
            return;
        }
        let Some(original_len) = self.messages.len().checked_sub(inserted_indices.len()) else {
            return;
        };
        let mut lifetimes = vec![PromptSegmentLifetime::AppendOnly; original_len];
        for segment in &self.segments {
            for item in lifetimes
                .iter_mut()
                .take(segment.messages.end.min(original_len))
                .skip(segment.messages.start.min(original_len))
            {
                *item = segment.lifetime;
            }
        }
        for &index in inserted_indices {
            if index > lifetimes.len() {
                return;
            }
            lifetimes.insert(index, lifetime);
        }
        if lifetimes.len() != self.messages.len() {
            return;
        }

        self.segments.clear();
        let mut start = 0;
        while start < lifetimes.len() {
            let current = lifetimes[start];
            let mut end = start + 1;
            while end < lifetimes.len() && lifetimes[end] == current {
                end += 1;
            }
            self.segments.push(PromptSegment {
                lifetime: current,
                messages: start..end,
            });
            start = end;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptSegmentLifetime {
    SessionEpoch,
    AppendOnly,
    LogicalTurn,
    RequestOnly,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptSegment {
    pub lifetime: PromptSegmentLifetime,
    pub messages: Range<usize>,
}

/// Exact provider route identity used only while deriving a private cache key.
/// Neither the endpoint digest nor the derived key is persisted in sessions or
/// request snapshots.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct RouteCacheIdentity {
    pub provider: String,
    pub endpoint_sha256: String,
    pub protocol: String,
    pub model: String,
    pub contract_version: String,
    pub request_layout_version: String,
}

impl std::fmt::Debug for RouteCacheIdentity {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RouteCacheIdentity")
            .field("provider", &self.provider)
            .field("endpoint_sha256", &"[REDACTED]")
            .field("protocol", &self.protocol)
            .field("model", &self.model)
            .field("contract_version", &self.contract_version)
            .field("request_layout_version", &self.request_layout_version)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct EffectivePromptCachePlan {
    pub policy: crate::config::PromptCachePolicy,
    private_key: Option<String>,
    pub stable_boundary: Option<usize>,
    prefix_sha256: String,
    canonical_tools_sha256: String,
}

impl EffectivePromptCachePlan {
    pub(crate) fn new(
        policy: crate::config::PromptCachePolicy,
        private_key: Option<String>,
        stable_boundary: Option<usize>,
        prefix_sha256: String,
        canonical_tools_sha256: String,
    ) -> Self {
        Self {
            policy,
            private_key,
            stable_boundary,
            prefix_sha256,
            canonical_tools_sha256,
        }
    }

    pub(crate) fn private_key(&self) -> Option<&str> {
        self.private_key.as_deref()
    }
}

impl std::fmt::Debug for EffectivePromptCachePlan {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("EffectivePromptCachePlan")
            .field("policy", &self.policy)
            .field(
                "private_key",
                &self.private_key.as_ref().map(|_| "[REDACTED]"),
            )
            .field("stable_boundary", &self.stable_boundary)
            .field("prefix_sha256", &"[REDACTED]")
            .field("canonical_tools_sha256", &"[REDACTED]")
            .finish()
    }
}

#[derive(Clone)]
pub struct PreparedModelRequest {
    pub(crate) guidance_prepared: bool,
    pub(crate) runtime_source: Option<crate::agent::stream::ProviderStateSourceV1>,
    pub kind: RequestKind,
    pub harness_components:
        std::collections::BTreeMap<crate::harness_registry::HarnessComponent, String>,
    pub provider: String,
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub tools: Vec<ToolDef>,
    pub options: ChatRequestOptions,
    pub context_breakdown: Option<ContextBreakdown>,
    pub prompt_cache: Option<PromptCacheIntent>,
}

impl std::fmt::Debug for PreparedModelRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PreparedModelRequest")
            .field("kind", &self.kind)
            .field("provider", &self.provider)
            .field("model", &self.model)
            .field("message_count", &self.messages.len())
            .field("tool_count", &self.tools.len())
            .field("has_context_breakdown", &self.context_breakdown.is_some())
            .field("has_prompt_cache_intent", &self.prompt_cache.is_some())
            .finish()
    }
}

impl PreparedModelRequest {
    pub fn new(
        kind: RequestKind,
        provider: impl Into<String>,
        model: impl Into<String>,
        messages: Vec<ChatMessage>,
        tools: Vec<ToolDef>,
        options: ChatRequestOptions,
    ) -> Self {
        Self {
            kind,
            guidance_prepared: false,
            runtime_source: None,
            harness_components: Default::default(),
            provider: provider.into(),
            model: model.into(),
            messages,
            tools,
            options,
            context_breakdown: None,
            prompt_cache: None,
        }
    }

    pub fn with_harness_component(
        mut self,
        component: crate::harness_registry::HarnessComponent,
        content: &[u8],
    ) -> Self {
        use sha2::{Digest, Sha256};
        self.harness_components
            .insert(component, format!("{:x}", Sha256::digest(content)));
        self
    }

    pub fn with_context_breakdown(mut self, breakdown: ContextBreakdown) -> Self {
        self.context_breakdown = Some(breakdown);
        self
    }

    pub fn with_prompt_cache(
        mut self,
        scope: PromptCacheScope,
        stable_prefix_messages: usize,
    ) -> Self {
        self.prompt_cache = Some(PromptCacheIntent {
            scope,
            stable_prefix_messages,
        });
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestEstimates {
    pub instructions: u64,
    pub messages: u64,
    pub schemas: u64,
    pub total: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestMessageSnapshot {
    pub role: String,
    pub content: String,
    pub tool_calls: Vec<RequestToolCallSnapshot>,
    pub omitted_tool_calls: usize,
    pub tool_call_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestToolCallSnapshot {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestSchemaSnapshot {
    pub name: String,
    pub description: String,
    pub parameters: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestPromptCacheSnapshot {
    pub domain: String,
    pub stable_prefix_messages: usize,
    pub stable_prefix_tokens: u64,
    pub boundary_valid: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestSnapshot {
    pub sequence: u64,
    pub kind: RequestKind,
    pub provider: String,
    pub model: String,
    pub protocol: &'static str,
    pub reasoning_effort: Option<String>,
    pub reasoning_format: Option<String>,
    pub estimates: RequestEstimates,
    pub context_breakdown: Option<ContextBreakdown>,
    pub prompt_cache: Option<RequestPromptCacheSnapshot>,
    pub instructions: Vec<String>,
    pub messages: Vec<RequestMessageSnapshot>,
    pub schemas: Vec<RequestSchemaSnapshot>,
    pub omitted_instructions: usize,
    pub omitted_messages: usize,
    pub omitted_schemas: usize,
    pub content_omitted_bytes: usize,
}

impl RequestSnapshot {
    fn from_prepared(
        request: &PreparedModelRequest,
        protocol: &'static str,
        secrets: &[&str],
    ) -> Self {
        let instruction_messages = request
            .messages
            .iter()
            .filter(|message| message.role == "system")
            .collect::<Vec<_>>();
        let conversation_messages = request
            .messages
            .iter()
            .filter(|message| message.role != "system")
            .collect::<Vec<_>>();
        let estimates = RequestEstimates {
            instructions: crate::context::estimate_serialized_tokens(&instruction_messages),
            messages: crate::context::estimate_serialized_tokens(&conversation_messages),
            schemas: crate::context::estimate_serialized_tokens(&request.tools),
            total: 0,
        };
        let mut estimates = estimates;
        estimates.total = estimates
            .instructions
            .saturating_add(estimates.messages)
            .saturating_add(estimates.schemas);
        let prompt_cache = request.prompt_cache.map(|intent| {
            let prefix = request.messages.get(..intent.stable_prefix_messages);
            RequestPromptCacheSnapshot {
                domain: intent.scope.domain().label().into(),
                stable_prefix_messages: intent.stable_prefix_messages,
                stable_prefix_tokens: prefix
                    .map(|messages| crate::context::estimate_serialized_tokens(&messages))
                    .unwrap_or(0),
                boundary_valid: prefix.is_some(),
            }
        });

        let mut budget = TextBudget::new(SNAPSHOT_TEXT_BUDGET);
        let private_markers = match request.kind {
            RequestKind::EmailGateway => Some((
                "[External private email gateway instruction unavailable]",
                "[External private email gateway content unavailable]",
            )),
            RequestKind::Dream | RequestKind::Distill => Some((
                "[Private maintenance instruction unavailable]",
                "[Private maintenance evidence unavailable]",
            )),
            _ => None,
        };
        let instructions = instruction_messages
            .iter()
            .take(SNAPSHOT_ITEM_LIMIT)
            .map(|message| {
                if let Some((instruction_marker, _)) = private_markers {
                    budget.take(instruction_marker)
                } else {
                    safe_value_text(&message.content.safe_snapshot_value(), secrets, &mut budget)
                }
            })
            .collect::<Vec<_>>();
        let mut remaining_tool_calls = SNAPSHOT_ITEM_LIMIT;
        let private_call_ids = request
            .messages
            .iter()
            .flat_map(|message| message.tool_calls.as_deref().unwrap_or_default())
            .filter(|call| crate::email::is_email_tool(&call.function.name))
            .map(|call| call.id.as_str())
            .collect::<std::collections::HashSet<_>>();
        let messages = conversation_messages
            .iter()
            .take(SNAPSHOT_ITEM_LIMIT)
            .map(|message| {
                if let Some((_, content_marker)) = private_markers {
                    RequestMessageSnapshot {
                        role: bounded_identifier(&message.role, secrets),
                        content: budget.take(content_marker),
                        tool_calls: Vec::new(),
                        omitted_tool_calls: message.tool_calls.as_ref().map_or(0, Vec::len),
                        tool_call_id: None,
                    }
                } else {
                    message_snapshot(
                        message,
                        secrets,
                        &mut budget,
                        &mut remaining_tool_calls,
                        &private_call_ids,
                    )
                }
            })
            .collect::<Vec<_>>();
        let schemas = request
            .tools
            .iter()
            .take(SNAPSHOT_ITEM_LIMIT)
            .map(|tool| RequestSchemaSnapshot {
                name: bounded_identifier(&tool.function.name, secrets),
                description: budget.take(&safe_text(&tool.function.description, secrets)),
                parameters: safe_value_text(&tool.function.parameters, secrets, &mut budget),
            })
            .collect::<Vec<_>>();

        Self {
            sequence: 0,
            kind: request.kind,
            provider: bounded_identifier(&request.provider, secrets),
            model: bounded_identifier(&request.model, secrets),
            protocol,
            reasoning_effort: request
                .options
                .reasoning_effort
                .map(|effort| format!("{effort:?}").to_ascii_lowercase()),
            reasoning_format: request
                .options
                .reasoning_request_format
                .map(|format| format!("{format:?}").to_ascii_lowercase()),
            estimates,
            context_breakdown: request.context_breakdown.clone(),
            prompt_cache,
            instructions,
            messages,
            schemas,
            omitted_instructions: instruction_messages
                .len()
                .saturating_sub(SNAPSHOT_ITEM_LIMIT),
            omitted_messages: conversation_messages
                .len()
                .saturating_sub(SNAPSHOT_ITEM_LIMIT),
            omitted_schemas: request.tools.len().saturating_sub(SNAPSHOT_ITEM_LIMIT),
            content_omitted_bytes: budget.omitted,
        }
    }

    pub fn render_text(&self) -> String {
        let mut lines = vec![
            format!("Latest prepared request #{}", self.sequence),
            String::new(),
            format!("Kind: {}", self.kind.label()),
            format!("Provider/model: {}/{}", self.provider, self.model),
            format!("Protocol: {}", self.protocol),
            format!(
                "Reasoning: {}",
                match (&self.reasoning_effort, &self.reasoning_format) {
                    (Some(effort), Some(format)) => format!("{effort} via {format}"),
                    _ => "not configured".into(),
                }
            ),
            String::new(),
            "Estimated request tokens (provider tokenizer unavailable)".into(),
            format!("  instructions: {}", self.estimates.instructions),
            format!("  messages: {}", self.estimates.messages),
            format!("  tool schemas: {}", self.estimates.schemas),
            format!("  total: {}", self.estimates.total),
        ];
        if let Some(cache) = &self.prompt_cache {
            lines.extend([
                String::new(),
                format!(
                    "Declared stable prefix: {} message(s) · ~{} estimated tokens · {} domain",
                    cache.stable_prefix_messages, cache.stable_prefix_tokens, cache.domain
                ),
                if cache.boundary_valid {
                    "This is a request intent; a provider cache hit is not implied.".into()
                } else {
                    "The declared prefix boundary is invalid; provider preparation will fail."
                        .into()
                },
            ]);
        }
        if let Some(context) = &self.context_breakdown {
            lines.extend([
                String::new(),
                "Prepared context categories (estimated tokens)".into(),
                format!("  instructions: {}", context.instructions),
                format!("  skills: {}", context.skills),
                format!("  built-in tools: {}", context.built_in_tools),
                format!("  MCP tools: {}", context.mcp_tools),
                format!("  checkpoint: {}", context.checkpoint),
                format!("  conversation: {}", context.conversation),
                format!("  attachments: {}", context.attachments),
                format!("  total: {}", context.total),
            ]);
        }

        lines.push(String::new());
        lines.push(format!("Instructions ({})", self.instructions.len()));
        if self.instructions.is_empty() {
            lines.push("  (none)".into());
        }
        for (index, instruction) in self.instructions.iter().enumerate() {
            lines.push(format!("\n[system {}]\n{}", index + 1, instruction));
        }
        if self.omitted_instructions > 0 {
            lines.push(format!(
                "  [… {} instruction item(s) omitted]",
                self.omitted_instructions
            ));
        }

        lines.push(String::new());
        lines.push(format!("Messages ({})", self.messages.len()));
        if self.messages.is_empty() {
            lines.push("  (none)".into());
        }
        for (index, message) in self.messages.iter().enumerate() {
            let result_id = message
                .tool_call_id
                .as_ref()
                .map_or_else(String::new, |id| format!(" · result for {id}"));
            lines.push(format!(
                "\n[{} {}{}]\n{}",
                message.role,
                index + 1,
                result_id,
                message.content
            ));
            for call in &message.tool_calls {
                lines.push(format!(
                    "  tool call {} · {}\n    {}",
                    call.id, call.name, call.arguments
                ));
            }
            if message.omitted_tool_calls > 0 {
                lines.push(format!(
                    "  [… {} tool call(s) omitted]",
                    message.omitted_tool_calls
                ));
            }
        }
        if self.omitted_messages > 0 {
            lines.push(format!(
                "  [… {} message item(s) omitted]",
                self.omitted_messages
            ));
        }

        lines.push(String::new());
        lines.push(format!("Tool schemas ({})", self.schemas.len()));
        if self.schemas.is_empty() {
            lines.push("  (none)".into());
        }
        for schema in &self.schemas {
            lines.push(format!(
                "\n[{}]\n{}\nparameters: {}",
                schema.name, schema.description, schema.parameters
            ));
        }
        if self.omitted_schemas > 0 {
            lines.push(format!(
                "  [… {} tool schema(s) omitted]",
                self.omitted_schemas
            ));
        }
        if self.content_omitted_bytes > 0 {
            lines.push(format!(
                "\n[… {} semantic byte(s) omitted by the snapshot bound]",
                self.content_omitted_bytes
            ));
        }

        let rendered = lines.join("\n");
        if rendered.len() <= REQUEST_SNAPSHOT_RENDER_LIMIT {
            rendered
        } else {
            let marker = format!(
                "\n[… {} rendered byte(s) omitted]",
                rendered.len() - REQUEST_SNAPSHOT_RENDER_LIMIT
            );
            let keep = REQUEST_SNAPSHOT_RENDER_LIMIT.saturating_sub(marker.len());
            format!("{}{}", truncate_utf8(&rendered, keep).0, marker)
        }
    }
}

#[derive(Clone, Default)]
pub struct RequestStore {
    inner: Arc<Mutex<RequestStoreInner>>,
}

#[derive(Default)]
struct RequestStoreInner {
    sequence: u64,
    latest: Option<RequestSnapshot>,
    harness: Option<crate::harness_registry::ComponentSnapshot>,
}

impl std::fmt::Debug for RequestStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let inner = self
            .inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        formatter
            .debug_struct("RequestStore")
            .field("sequence", &inner.sequence)
            .field("has_snapshot", &inner.latest.is_some())
            .finish()
    }
}

impl RequestStore {
    pub fn record(&self, request: &PreparedModelRequest, secrets: &[&str]) -> u64 {
        self.record_with_protocol(request, "chat_completions", secrets)
    }

    pub(crate) fn record_with_protocol(
        &self,
        request: &PreparedModelRequest,
        protocol: &'static str,
        secrets: &[&str],
    ) -> u64 {
        self.record_on_route(request, protocol, secrets, None)
    }

    pub(crate) fn record_on_route(
        &self,
        request: &PreparedModelRequest,
        protocol: &'static str,
        secrets: &[&str],
        source: Option<&crate::agent::stream::ProviderStateSourceV1>,
    ) -> u64 {
        let mut snapshot = RequestSnapshot::from_prepared(request, protocol, secrets);
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        inner.sequence = inner.sequence.wrapping_add(1).max(1);
        snapshot.sequence = inner.sequence;
        inner.harness = Some(match source {
            Some(source) => crate::harness_registry::capture_prepared_on_route(request, source),
            None => crate::harness_registry::capture_prepared(request),
        });
        inner.latest = Some(snapshot);
        inner.sequence
    }

    pub fn harness_snapshot(&self) -> Option<crate::harness_registry::ComponentSnapshot> {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .harness
            .clone()
    }

    pub fn snapshot(&self) -> Option<RequestSnapshot> {
        self.inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .latest
            .clone()
    }
}

pub fn render_latest_request(store: &RequestStore) -> String {
    store.snapshot().map_or_else(
        || {
            "No model request has been prepared in this ca process.\n\nOpening /request never starts MCP servers or sends provider traffic."
                .into()
        },
        |snapshot| snapshot.render_text(),
    )
}

fn message_snapshot(
    message: &ChatMessage,
    secrets: &[&str],
    budget: &mut TextBudget,
    remaining_tool_calls: &mut usize,
    private_call_ids: &std::collections::HashSet<&str>,
) -> RequestMessageSnapshot {
    let source_tool_calls = message.tool_calls.as_deref().unwrap_or_default();
    let retained_tool_calls = source_tool_calls.len().min(*remaining_tool_calls);
    let tool_calls = source_tool_calls
        .iter()
        .take(retained_tool_calls)
        .map(|call| RequestToolCallSnapshot {
            id: bounded_identifier(&call.id, secrets),
            name: bounded_identifier(&call.function.name, secrets),
            arguments: if crate::email::is_email_tool(&call.function.name) {
                crate::email::private_arguments_marker(&call.function.name)
            } else {
                safe_json_string(&call.function.arguments, secrets, budget)
            },
        })
        .collect();
    *remaining_tool_calls = remaining_tool_calls.saturating_sub(retained_tool_calls);
    RequestMessageSnapshot {
        role: bounded_identifier(&message.role, secrets),
        content: if message
            .tool_call_id
            .as_deref()
            .is_some_and(|id| private_call_ids.contains(id))
        {
            budget.take("[External private email result unavailable]")
        } else {
            safe_value_text(&message.content.safe_snapshot_value(), secrets, budget)
        },
        tool_calls,
        omitted_tool_calls: source_tool_calls.len().saturating_sub(retained_tool_calls),
        tool_call_id: message
            .tool_call_id
            .as_deref()
            .map(|id| bounded_identifier(id, secrets)),
    }
}

fn safe_json_string(raw: &str, secrets: &[&str], budget: &mut TextBudget) -> String {
    match serde_json::from_str::<Value>(raw) {
        Ok(value) => safe_value_text(&value, secrets, budget),
        Err(_) => budget.take(&safe_text(raw, secrets)),
    }
}

fn safe_value_text(value: &Value, secrets: &[&str], budget: &mut TextBudget) -> String {
    let safe = safe_value(value, secrets, budget, 0);
    let rendered = match safe {
        Value::String(text) => text,
        value => {
            crate::json_compat::to_string_pretty(&value).unwrap_or_else(|_| "[invalid JSON]".into())
        }
    };
    budget.take(&rendered)
}

fn safe_value(value: &Value, secrets: &[&str], budget: &mut TextBudget, depth: usize) -> Value {
    if depth >= SNAPSHOT_JSON_DEPTH_LIMIT {
        return Value::String("[nested content omitted at depth limit]".into());
    }
    if budget.remaining_json_nodes == 0 {
        return Value::String("[nested content omitted by item bound]".into());
    }
    budget.remaining_json_nodes -= 1;
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) => value.clone(),
        Value::String(text) => Value::String(safe_text(text, secrets)),
        Value::Array(values) => {
            let retained = values
                .len()
                .min(SNAPSHOT_ITEM_LIMIT)
                .min(budget.remaining_json_nodes);
            let mut safe = values
                .iter()
                .take(retained)
                .map(|value| safe_value(value, secrets, budget, depth + 1))
                .collect::<Vec<_>>();
            let omitted = values.len().saturating_sub(retained);
            if omitted > 0 {
                safe.push(Value::String(format!(
                    "[… {omitted} array item(s) omitted]"
                )));
            }
            Value::Array(safe)
        }
        Value::Object(values) => {
            let retained = values
                .len()
                .min(SNAPSHOT_ITEM_LIMIT)
                .min(budget.remaining_json_nodes);
            let mut safe = values
                .iter()
                .take(retained)
                .map(|(key, value)| {
                    if sensitive_field(key) {
                        (key.clone(), Value::String("[REDACTED]".into()))
                    } else {
                        (key.clone(), safe_value(value, secrets, budget, depth + 1))
                    }
                })
                .collect::<serde_json::Map<_, _>>();
            let omitted = values.len().saturating_sub(retained);
            if omitted > 0 {
                let marker = (0..)
                    .map(|suffix| {
                        if suffix == 0 {
                            "[ca omitted fields]".to_string()
                        } else {
                            format!("[ca omitted fields {suffix}]")
                        }
                    })
                    .find(|key| !safe.contains_key(key))
                    .expect("an unused bounded marker key exists");
                safe.insert(marker, Value::Number(omitted.into()));
            }
            Value::Object(safe)
        }
    }
}

fn sensitive_field(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase().replace(['-', '_', ' '], "");
    matches!(
        normalized.as_str(),
        "authorization"
            | "proxyauthorization"
            | "apikey"
            | "xapikey"
            | "accesskey"
            | "secretkey"
            | "password"
            | "clientsecret"
            | "accesstoken"
            | "refreshtoken"
            | "token"
            | "secret"
            | "credential"
            | "credentials"
            | "auth"
            | "authentication"
    )
}

pub(crate) fn safe_text(text: &str, secrets: &[&str]) -> String {
    if let Some(metadata) = binary_metadata(text) {
        return metadata;
    }
    let mut safe = crate::terminal_text::sanitize_terminal_text(text);
    for secret in secrets.iter().copied().filter(|secret| !secret.is_empty()) {
        safe = safe.replace(secret, "[REDACTED]");
    }
    safe.lines()
        .map(mask_credential_line)
        .collect::<Vec<_>>()
        .join("\n")
}

fn mask_credential_line(line: &str) -> String {
    let lowercase = line.to_ascii_lowercase();
    for marker in [
        "authorization:",
        "api_key=",
        "api-key=",
        "access_token=",
        "refresh_token=",
        "password=",
        "secret_key=",
    ] {
        if let Some(index) = lowercase.find(marker) {
            let prefix = &line[..index + marker.len()];
            return format!("{prefix}[REDACTED]");
        }
    }
    line.to_string()
}

fn binary_metadata(text: &str) -> Option<String> {
    let lowercase = text.to_ascii_lowercase();
    if !lowercase.starts_with("data:") {
        return None;
    }
    let header = text.split_once(',').map_or(text, |(header, _)| header);
    let media_type = header
        .strip_prefix("data:")
        .and_then(|header| header.split(';').next())
        .filter(|media_type| !media_type.is_empty())
        .unwrap_or("application/octet-stream");
    Some(format!("[binary content: {media_type}; payload omitted]"))
}

fn bounded_identifier(text: &str, secrets: &[&str]) -> String {
    let safe = safe_text(text, secrets);
    let (mut bounded, omitted) = truncate_utf8(&safe, SNAPSHOT_ID_LIMIT);
    if omitted > 0 {
        bounded.push_str(&format!("[…{omitted} bytes omitted]"));
    }
    bounded
}

struct TextBudget {
    remaining: usize,
    omitted: usize,
    remaining_json_nodes: usize,
}

impl TextBudget {
    fn new(limit: usize) -> Self {
        Self {
            remaining: limit,
            omitted: 0,
            remaining_json_nodes: SNAPSHOT_JSON_NODE_LIMIT,
        }
    }

    fn take(&mut self, text: &str) -> String {
        let allowed = self.remaining.min(SNAPSHOT_FRAGMENT_LIMIT);
        let (mut taken, omitted) = truncate_utf8(text, allowed);
        self.remaining = self.remaining.saturating_sub(taken.len());
        self.omitted = self.omitted.saturating_add(omitted);
        if omitted > 0 {
            let marker = format!("\n[… {omitted} byte(s) omitted]");
            if marker.len() <= self.remaining {
                self.remaining -= marker.len();
                taken.push_str(&marker);
            }
        }
        taken
    }
}

fn truncate_utf8(text: &str, limit: usize) -> (String, usize) {
    if text.len() <= limit {
        return (text.to_string(), 0);
    }
    let mut end = limit.min(text.len());
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    (text[..end].to_string(), text.len().saturating_sub(end))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::client::{FunctionCall, ToolCallMsg, ToolFunction};
    use crate::agent::provider::ProviderProtocol;
    use crate::agent::stream::{ProviderResponseItem, ProviderStateSourceV1};

    fn prepared(messages: Vec<ChatMessage>, tools: Vec<ToolDef>) -> PreparedModelRequest {
        PreparedModelRequest::new(
            RequestKind::MainTurn,
            "local",
            "model",
            messages,
            tools,
            ChatRequestOptions::default(),
        )
    }

    #[test]
    fn role_run_request_kind_has_a_stable_public_label() {
        assert_eq!(RequestKind::RoleRun.label(), "role run");
        assert_ne!(RequestKind::RoleRun, RequestKind::Headless);
    }

    fn provider_state(content: &str) -> ProviderResponseItem {
        ProviderResponseItem::ResponsesOutput {
            source: ProviderStateSourceV1::for_test("openai", ProviderProtocol::OpenAiResponses),
            output_index: 0,
            item: serde_json::json!({
                "type": "reasoning",
                "summary": [],
                "encrypted_content": content,
            }),
        }
    }

    #[test]
    fn request_snapshots_and_debug_views_omit_opaque_provider_state() {
        let sentinel = "opaque-request-provider-state-sentinel";
        let request = prepared(
            vec![ChatMessage {
                role: "assistant".into(),
                content: crate::image_content::ModelContent::text("visible answer"),
                tool_calls: None,
                tool_call_id: None,
                response_items: vec![provider_state(sentinel)],
            }],
            Vec::new(),
        );
        let prepared_debug = format!("{request:?}");
        let message_debug = format!("{:?}", request.messages[0]);
        let store = RequestStore::default();
        store.record(&request, &[]);
        let snapshot = store.snapshot().unwrap();
        let snapshot_debug = format!("{snapshot:?}");
        let rendered = snapshot.render_text();
        let store_debug = format!("{store:?}");

        for output in [
            &prepared_debug,
            &message_debug,
            &snapshot_debug,
            &rendered,
            &store_debug,
        ] {
            assert!(!output.contains(sentinel));
        }
        assert!(message_debug.contains("[REDACTED]"));
        assert!(rendered.contains("visible answer"));
    }

    #[test]
    fn email_gateway_request_snapshot_is_metadata_only() {
        let sentinel = "private-email-gateway-body-and-address@example.com";
        let request = PreparedModelRequest::new(
            RequestKind::EmailGateway,
            "local",
            "model",
            vec![
                ChatMessage {
                    role: "system".into(),
                    content: crate::image_content::ModelContent::text(sentinel),
                    tool_calls: None,
                    tool_call_id: None,
                    response_items: Vec::new(),
                },
                ChatMessage {
                    role: "user".into(),
                    content: crate::image_content::ModelContent::text(sentinel),
                    tool_calls: None,
                    tool_call_id: None,
                    response_items: Vec::new(),
                },
            ],
            Vec::new(),
            ChatRequestOptions::default(),
        );
        let store = RequestStore::default();
        store.record(&request, &[]);
        let rendered = store.snapshot().unwrap().render_text();
        assert!(!rendered.contains(sentinel));
        assert!(rendered.contains("External private email gateway"));
    }

    #[test]
    fn maintenance_request_snapshots_keep_corpus_private_and_show_typed_kind() {
        for kind in [RequestKind::Dream, RequestKind::Distill] {
            let sentinel = format!("private-maintenance-evidence-{kind:?}");
            let request = PreparedModelRequest::new(
                kind,
                "fake",
                "model",
                vec![
                    ChatMessage {
                        role: "system".into(),
                        content: crate::image_content::ModelContent::text(sentinel.clone()),
                        tool_calls: None,
                        tool_call_id: None,
                        response_items: Vec::new(),
                    },
                    ChatMessage {
                        role: "user".into(),
                        content: crate::image_content::ModelContent::text(sentinel.clone()),
                        tool_calls: None,
                        tool_call_id: None,
                        response_items: Vec::new(),
                    },
                ],
                Vec::new(),
                ChatRequestOptions::default(),
            );
            let store = RequestStore::default();
            store.record(&request, &[]);
            let rendered = store.snapshot().unwrap().render_text();
            assert!(!rendered.contains(&sentinel));
            assert!(rendered.contains(kind.label()));
            assert!(rendered.contains("Private maintenance"));
        }
    }

    #[test]
    fn request_snapshot_scrubs_external_private_email_arguments_and_results() {
        let sentinel = "request-private-email-sentinel";
        let request = prepared(
            vec![
                ChatMessage {
                    role: "assistant".into(),
                    content: crate::image_content::ModelContent::text(""),
                    tool_calls: Some(vec![ToolCallMsg {
                        id: "email-call".into(),
                        call_type: "function".into(),
                        function: FunctionCall {
                            name: "email_read".into(),
                            arguments: serde_json::json!({"body": sentinel}).to_string(),
                        },
                    }]),
                    tool_call_id: None,
                    response_items: Vec::new(),
                },
                ChatMessage {
                    role: "tool".into(),
                    content: crate::image_content::ModelContent::text(sentinel),
                    tool_calls: None,
                    tool_call_id: Some("email-call".into()),
                    response_items: Vec::new(),
                },
            ],
            Vec::new(),
        );
        let store = RequestStore::default();
        store.record(&request, &[]);
        let rendered = store.snapshot().unwrap().render_text();
        assert!(!rendered.contains(sentinel));
        assert!(rendered.contains("external_private"));
        assert!(rendered.contains("External private email result unavailable"));
    }

    #[test]
    fn snapshot_redacts_runtime_and_structured_secrets_and_binary_content() {
        let secret = "sentinel-runtime-key";
        let request = prepared(
            vec![
                ChatMessage {
                    role: "system".into(),
                    content: crate::image_content::ModelContent::text(format!("system {secret}")),
                    tool_calls: None,
                    tool_call_id: None,
                    response_items: Vec::new(),
                },
                ChatMessage {
                    role: "user".into(),
                    content: crate::image_content::ModelContent::multimodal(
                        format!("Authorization: Bearer {secret}"),
                        &[crate::image_content::normalize_rgba(1, 1, vec![1, 2, 3, 255]).unwrap()],
                    ),
                    tool_calls: Some(vec![ToolCallMsg {
                        id: "call-1".into(),
                        call_type: "function".into(),
                        function: FunctionCall {
                            name: "deploy".into(),
                            arguments: format!(r#"{{"api_key":"{secret}","value":"ok"}}"#),
                        },
                    }]),
                    tool_call_id: None,
                    response_items: Vec::new(),
                },
            ],
            vec![ToolDef {
                def_type: "function".into(),
                function: ToolFunction {
                    name: "deploy".into(),
                    description: format!("uses {secret}"),
                    parameters: serde_json::json!({
                        "type": "object",
                        "properties": {"client_secret": {"default": secret}}
                    }),
                },
            }],
        );
        let store = RequestStore::default();
        store.record(&request, &[secret]);
        let debug = format!("{store:?}");
        let rendered = store.snapshot().unwrap().render_text();
        for forbidden in [secret, "Bearer sentinel"] {
            assert!(!rendered.contains(forbidden));
            assert!(!debug.contains(forbidden));
        }
        assert!(rendered.contains("[REDACTED]"));
        assert!(rendered.contains("[binary content omitted]"));
        assert!(rendered.contains("image/png"));
        assert!(rendered.contains("\"client_secret\": \"[REDACTED]\""));
    }

    #[test]
    fn snapshot_redacts_even_a_short_nonempty_runtime_credential() {
        let request = prepared(
            vec![ChatMessage {
                role: "user".into(),
                content: crate::image_content::ModelContent::text("prefix k suffix"),
                tool_calls: None,
                tool_call_id: None,
                response_items: Vec::new(),
            }],
            Vec::new(),
        );
        let store = RequestStore::default();
        store.record(&request, &["k"]);
        let rendered = store.snapshot().unwrap().render_text();
        assert!(!rendered.contains("prefix k suffix"));
        assert!(rendered.contains("prefix [REDACTED] suffix"));
    }

    #[test]
    fn snapshot_is_utf8_safe_bounded_and_reports_omissions() {
        let messages = (0..100)
            .map(|index| ChatMessage {
                role: "user".into(),
                content: crate::image_content::ModelContent::text(format!(
                    "{index}:{}",
                    "🦀".repeat(10_000)
                )),
                tool_calls: None,
                tool_call_id: None,
                response_items: Vec::new(),
            })
            .collect();
        let request = prepared(messages, Vec::new());
        let store = RequestStore::default();
        store.record(&request, &[]);
        let snapshot = store.snapshot().unwrap();
        let rendered = snapshot.render_text();
        assert!(rendered.len() <= REQUEST_SNAPSHOT_RENDER_LIMIT);
        assert_eq!(snapshot.omitted_messages, 36);
        assert!(snapshot.content_omitted_bytes > 0);
        assert!(rendered.contains("omitted"));
        assert!(std::str::from_utf8(rendered.as_bytes()).is_ok());
    }

    #[test]
    fn snapshot_bounds_global_tool_calls_and_terminal_controls() {
        let mut nested = Value::String("leaf".into());
        for _ in 0..100 {
            nested = serde_json::json!({"child": nested});
        }
        let calls = (0..100)
            .map(|index| ToolCallMsg {
                id: format!("call-{index}"),
                call_type: "function".into(),
                function: FunctionCall {
                    name: "inspect".into(),
                    arguments: r#"{"token":"must-not-appear","value":"ok"}"#.into(),
                },
            })
            .collect();
        let request = prepared(
            vec![ChatMessage {
                role: "user".into(),
                content: crate::image_content::ModelContent::text(format!(
                    "{} before\u{1b}[31mafter",
                    nested
                )),
                tool_calls: Some(calls),
                tool_call_id: None,
                response_items: Vec::new(),
            }],
            Vec::new(),
        );
        let store = RequestStore::default();
        store.record(&request, &[]);
        let snapshot = store.snapshot().unwrap();
        let rendered = snapshot.render_text();

        assert_eq!(snapshot.messages[0].tool_calls.len(), SNAPSHOT_ITEM_LIMIT);
        assert_eq!(snapshot.messages[0].omitted_tool_calls, 36);
        assert!(rendered.contains("36 tool call(s) omitted"));
        assert!(!rendered.contains("must-not-appear"));
        assert!(!rendered.contains('\u{1b}'));
        assert!(rendered.len() <= REQUEST_SNAPSHOT_RENDER_LIMIT);
    }

    #[test]
    fn latest_prepared_request_wins_and_empty_text_is_no_io_guidance() {
        let store = RequestStore::default();
        assert!(render_latest_request(&store).contains("No model request"));
        let first = prepared(Vec::new(), Vec::new());
        store.record(&first, &[]);
        let mut second = prepared(Vec::new(), Vec::new());
        second.kind = RequestKind::ProviderProbe;
        second.provider = "second".into();
        assert_eq!(store.record(&second, &[]), 2);
        let snapshot = store.snapshot().unwrap();
        assert_eq!(snapshot.sequence, 2);
        assert_eq!(snapshot.kind, RequestKind::ProviderProbe);
        assert_eq!(snapshot.provider, "second");
        assert_eq!(snapshot.protocol, "chat_completions");
    }

    #[test]
    fn request_snapshot_reports_declared_stable_prefix_without_private_scope_identity() {
        let scope_id = Uuid::parse_str("00000000-0000-4000-8000-000000000152").unwrap();
        let request = prepared(
            vec![
                ChatMessage {
                    role: "system".into(),
                    content: crate::image_content::ModelContent::text("stable rules"),
                    tool_calls: None,
                    tool_call_id: None,
                    response_items: Vec::new(),
                },
                ChatMessage {
                    role: "user".into(),
                    content: crate::image_content::ModelContent::text("volatile task"),
                    tool_calls: None,
                    tool_call_id: None,
                    response_items: Vec::new(),
                },
            ],
            Vec::new(),
        )
        .with_prompt_cache(PromptCacheScope::conversation(scope_id), 1);
        let store = RequestStore::default();
        store.record(&request, &[]);
        let snapshot = store.snapshot().unwrap();
        let cache = snapshot.prompt_cache.as_ref().expect("cache intent");

        assert_eq!(cache.domain, "conversation");
        assert_eq!(cache.stable_prefix_messages, 1);
        assert!(cache.stable_prefix_tokens > 0);
        let rendered = snapshot.render_text();
        assert!(rendered.contains("Declared stable prefix"));
        assert!(rendered.contains("provider cache hit is not implied"));
        assert!(!rendered.contains(&scope_id.to_string()));
    }
}
