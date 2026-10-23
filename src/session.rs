use crate::app::{Message, Role, ToolCall, ToolStatus};
use crate::context::{ContextCheckpoint, ContextCheckpointIntegrityError};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use uuid::Uuid;

const SESSION_BUNDLE_STORAGE_VERSION: u32 = 1;
pub const SESSION_HISTORY_PAGE_SIZE: usize = 64;
const MAX_SESSION_MESSAGE_BYTES: usize = 1024 * 1024;
const MAX_SESSION_HISTORY_BYTES: u64 = 64 * 1024 * 1024;
const MAX_SESSION_METADATA_BYTES: u64 = 8 * 1024 * 1024;
const MAX_SESSION_MESSAGES: usize = 1_000_000;
const PUBLIC_SESSION_PREFIX: &str = "ses_";
const PUBLIC_SESSION_BODY_LEN: usize = 22;
const PUBLIC_SESSION_ALPHABET: &[u8; 62] =
    b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
const MAX_AUTOMATIC_TITLE_CELLS: usize = 64;
const MAX_RENAMED_TITLE_CELLS: usize = 100;
const MAX_TITLE_SUBJECT_CHARS: usize = 4_096;

/// Return a stable public identifier without changing the persisted UUID.
pub fn public_session_id(id: Uuid) -> String {
    let mut value = id.as_u128();
    let mut encoded = [b'0'; PUBLIC_SESSION_BODY_LEN];
    for slot in encoded.iter_mut().rev() {
        *slot = PUBLIC_SESSION_ALPHABET[(value % 62) as usize];
        value /= 62;
    }
    debug_assert_eq!(value, 0);
    format!(
        "{PUBLIC_SESSION_PREFIX}{}",
        std::str::from_utf8(&encoded).expect("base62 alphabet is UTF-8")
    )
}

/// Decode only the canonical fixed-width public session form.
pub fn parse_public_session_id(value: &str) -> Option<Uuid> {
    let encoded = value.strip_prefix(PUBLIC_SESSION_PREFIX)?;
    if encoded.len() != PUBLIC_SESSION_BODY_LEN {
        return None;
    }
    let mut decoded = 0_u128;
    for byte in encoded.bytes() {
        let digit = PUBLIC_SESSION_ALPHABET
            .iter()
            .position(|candidate| *candidate == byte)? as u128;
        decoded = decoded.checked_mul(62)?.checked_add(digit)?;
    }
    Some(Uuid::from_u128(decoded))
}

/// Derive a bounded local title from the first real user request.
pub fn automatic_session_title(messages: &[Message]) -> Option<String> {
    messages
        .iter()
        .find(|message| message.role == Role::User)
        .and_then(|message| automatic_title_from_prompt(&message.content))
}

/// Return a bounded copy of the first real request for an isolated title
/// generation request. Attachments stay available to the conversation but do
/// not become title-generation instructions.
pub fn session_title_subject(messages: &[Message]) -> Option<String> {
    let prompt = messages
        .iter()
        .find(|message| message.role == Role::User)?
        .content
        .as_str();
    let prompt = explicit_user_request(prompt).unwrap_or(prompt);
    let prompt = prompt
        .split("\n\n--- Attached file:")
        .next()
        .unwrap_or(prompt)
        .split("\n\n[Image unavailable:")
        .next()
        .unwrap_or(prompt)
        .trim();
    if prompt.is_empty() {
        return None;
    }
    Some(prompt.chars().take(MAX_TITLE_SUBJECT_CHARS).collect())
}

/// Normalize a provider-produced title into the same terminal-safe bounded
/// shape used by automatic local titles.
pub fn clean_generated_session_title(response: &str) -> Option<String> {
    let visible = strip_generated_reasoning(response);
    let line = visible
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && *line != "```")?;
    let line = line.trim_start_matches(['#', '-', '*', '•']).trim();
    let line = line
        .strip_prefix("Title:")
        .or_else(|| line.strip_prefix("title:"))
        .unwrap_or(line)
        .trim()
        .trim_matches(|character| matches!(character, '"' | '\'' | '`'))
        .trim_end_matches(['.', ':', ';']);
    automatic_title_from_prompt(line)
}

/// Identify legacy fallback titles for which the picker should offer a clearer
/// generated replacement. This is advisory only and never mutates a session.
pub fn session_title_needs_repair(title: &str) -> bool {
    let title = title.trim();
    title.is_empty()
        || title.eq_ignore_ascii_case("untitled session")
        || title.starts_with('/')
        || title.starts_with("~/")
        || title.starts_with("# Files mentioned")
        || title.starts_with("My request:")
        || title.starts_with("Screenshot ")
        || title.ends_with('…')
}

fn strip_generated_reasoning(response: &str) -> String {
    let mut remaining = response;
    let mut visible = String::new();
    loop {
        let Some(start) = remaining.find("<think>") else {
            visible.push_str(remaining);
            break;
        };
        visible.push_str(&remaining[..start]);
        let after_start = &remaining[start + "<think>".len()..];
        let Some(end) = after_start.find("</think>") else {
            break;
        };
        remaining = &after_start[end + "</think>".len()..];
    }
    visible
}

fn automatic_title_from_prompt(prompt: &str) -> Option<String> {
    let prompt = explicit_user_request(prompt).unwrap_or(prompt);
    let prompt = prompt
        .split("\n\n--- Attached file:")
        .next()
        .unwrap_or(prompt)
        .split("\n\n[Image unavailable:")
        .next()
        .unwrap_or(prompt);
    let safe = prompt
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect::<String>();
    let normalized = safe.split_whitespace().collect::<Vec<_>>().join(" ");
    let normalized = normalized.trim_start_matches(|character: char| {
        character.is_whitespace() || matches!(character, '#' | '>' | '-' | '*' | '•')
    });
    if normalized.is_empty() {
        return None;
    }
    if unicode_width::UnicodeWidthStr::width(normalized) <= MAX_AUTOMATIC_TITLE_CELLS {
        return Some(normalized.to_string());
    }

    let budget = MAX_AUTOMATIC_TITLE_CELLS.saturating_sub(1);
    let mut used = 0_usize;
    let mut end = 0;
    for (index, grapheme) in
        unicode_segmentation::UnicodeSegmentation::grapheme_indices(normalized, true)
    {
        let width = unicode_width::UnicodeWidthStr::width(grapheme);
        if used.saturating_add(width) > budget {
            break;
        }
        used = used.saturating_add(width);
        end = index + grapheme.len();
    }
    let mut title = normalized[..end].trim_end();
    if let Some(last_space) = title.rfind(char::is_whitespace) {
        let word_safe = title[..last_space].trim_end();
        if unicode_width::UnicodeWidthStr::width(word_safe) >= budget / 2 {
            title = word_safe;
        }
    }
    let title = title.trim_end_matches([',', '.', ':', ';', '!', '?', '-']);
    (!title.is_empty()).then(|| format!("{title}…"))
}

fn explicit_user_request(prompt: &str) -> Option<&str> {
    ["## My request:", "# My request:", "My request:"]
        .into_iter()
        .find_map(|marker| {
            prompt
                .rsplit_once(marker)
                .map(|(_, request)| request.trim())
        })
        .filter(|request| !request.is_empty())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionParent {
    pub session_id: Uuid,
    pub branch_turn: usize,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ForeignTranscriptSource {
    Codex,
    Claude,
    OpenCode,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ImportProvenance {
    pub source: ForeignTranscriptSource,
    pub source_label: String,
    pub source_session_digest: String,
    pub message_count: usize,
    pub skipped_items: usize,
    pub learning_eligible: bool,
    pub imported_at: DateTime<Utc>,
}

pub const MECHANISTIC_CHECKPOINT_SCHEMA: &str = "ca.mechanistic_checkpoint.v1";
pub const MECHANISTIC_CAPSULE_INDEX_SCHEMA: &str = "ca.mechanistic_capsule_index.v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MechanisticCapsuleIndexEntryV1 {
    pub capsule_digest: String,
    pub revision: String,
    #[serde(default)]
    pub call_ids: Vec<String>,
    #[serde(default)]
    pub action_mapping: Vec<String>,
    pub consumption_boundary: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MechanisticCapsuleIndexV1 {
    pub schema: String,
    #[serde(default)]
    pub entries: Vec<MechanisticCapsuleIndexEntryV1>,
}

impl Default for MechanisticCapsuleIndexV1 {
    fn default() -> Self {
        Self {
            schema: MECHANISTIC_CAPSULE_INDEX_SCHEMA.into(),
            entries: Vec::new(),
        }
    }
}

impl MechanisticCapsuleIndexV1 {
    fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn retain_valid(&mut self, message_count: usize) {
        self.entries.retain(|entry| {
            entry.capsule_digest.len() == 64
                && entry.revision.len() == 64
                && is_hex_digest(&entry.capsule_digest)
                && is_hex_digest(&entry.revision)
                && (1..=64).contains(&entry.call_ids.len())
                && entry.call_ids.len() == entry.action_mapping.len()
                && entry
                    .call_ids
                    .iter()
                    .all(|id| safe_checkpoint_atom(id, 128))
                && entry
                    .action_mapping
                    .iter()
                    .all(|name| safe_checkpoint_atom(name, 128))
                && entry
                    .consumption_boundary
                    .is_none_or(|boundary| boundary <= message_count)
        });
        self.entries.truncate(100);
        if self.schema != MECHANISTIC_CAPSULE_INDEX_SCHEMA {
            self.entries.clear();
            self.schema = MECHANISTIC_CAPSULE_INDEX_SCHEMA.into();
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MechanisticCheckpointPhaseV1 {
    VerificationRunning,
    DiagnosisPending,
    RepairPending,
    Blocked,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MechanisticCheckpointV1 {
    pub schema: String,
    pub job_id: String,
    pub expected_revision: String,
    pub phase: MechanisticCheckpointPhaseV1,
    pub request_digest: String,
    pub receipt_digest: Option<String>,
    pub remaining_repair_passes: u8,
    pub server: String,
    pub profile: String,
    #[serde(default)]
    pub changed_paths: Vec<String>,
}

impl MechanisticCheckpointV1 {
    pub fn validate(&self, max_repair_passes: u8) -> Result<(), &'static str> {
        if self.schema != MECHANISTIC_CHECKPOINT_SCHEMA {
            return Err("unsupported mechanistic checkpoint schema");
        }
        if !safe_checkpoint_atom(&self.job_id, 128)
            || !safe_checkpoint_atom(&self.server, 128)
            || !safe_checkpoint_atom(&self.profile, 64)
        {
            return Err("mechanistic checkpoint contains an invalid identity");
        }
        if !is_hex_digest(&self.expected_revision)
            || !is_hex_digest(&self.request_digest)
            || self
                .receipt_digest
                .as_deref()
                .is_some_and(|digest| !is_hex_digest(digest))
        {
            return Err("mechanistic checkpoint contains an invalid digest");
        }
        if self.remaining_repair_passes > max_repair_passes || self.changed_paths.len() > 64 {
            return Err("mechanistic checkpoint exceeds its bounded repair state");
        }
        if self
            .changed_paths
            .iter()
            .any(|path| !safe_checkpoint_relative_path(path))
        {
            return Err("mechanistic checkpoint contains an invalid repository path");
        }
        let mut normalized = self.changed_paths.clone();
        normalized.sort();
        normalized.dedup();
        if normalized != self.changed_paths {
            return Err("mechanistic checkpoint paths must be sorted and unique");
        }
        Ok(())
    }

    fn blocked_malformed() -> Self {
        Self {
            schema: MECHANISTIC_CHECKPOINT_SCHEMA.into(),
            job_id: "malformed".into(),
            expected_revision: "0".repeat(64),
            phase: MechanisticCheckpointPhaseV1::Blocked,
            request_digest: "0".repeat(64),
            receipt_digest: None,
            remaining_repair_passes: 0,
            server: "invalid".into(),
            profile: "invalid".into(),
            changed_paths: Vec::new(),
        }
    }
}

fn deserialize_mechanistic_checkpoint<'de, D>(
    deserializer: D,
) -> Result<Option<MechanisticCheckpointV1>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(value.map(|value| {
        serde_json::from_value(value)
            .unwrap_or_else(|_| MechanisticCheckpointV1::blocked_malformed())
    }))
}

fn deserialize_manual_rule_ids<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let ids = Vec::<String>::deserialize(deserializer)?;
    if ids.len() > 128
        || ids.iter().any(|id| {
            id.is_empty()
                || id.len() > 128
                || !id.bytes().all(|byte| {
                    byte.is_ascii_lowercase()
                        || byte.is_ascii_digit()
                        || matches!(byte, b'-' | b'_' | b'/')
                })
        })
        || ids.windows(2).any(|pair| pair[0] >= pair[1])
    {
        return Err(serde::de::Error::custom(
            "saved manual rule ids are invalid or not sorted",
        ));
    }
    Ok(ids)
}

fn safe_checkpoint_atom(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn is_hex_digest(value: &str) -> bool {
    (value.len() == 40 || value.len() == 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn safe_checkpoint_relative_path(value: &str) -> bool {
    let path = std::path::Path::new(value);
    !value.is_empty()
        && value.len() <= 1_024
        && !path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_)))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AutomationSessionProvenanceV1 {
    pub schema: String,
    pub automation_id: String,
    pub run_id: Uuid,
    pub occurrence_id: String,
    pub kind: String,
}

/// Provider-reported token accounting anchor + compaction failure cooldown
/// (Hermes-informed). Persisted on the session row so gateway re-reads and
/// restarts do not lose the last known usage baseline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextUsageAnchor {
    pub provider_input_tokens: u64,
    pub provider_cached_tokens: u64,
    pub content_fingerprint: String,
    pub recorded_at: DateTime<Utc>,
    pub checkpoint_failure_streak: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_attempt_after: Option<DateTime<Utc>>,
}

impl ContextUsageAnchor {
    pub fn cooldown_seconds(streak: u32) -> i64 {
        match streak {
            0 | 1 => 60,
            2 => 300,
            _ => 900,
        }
    }

    pub fn record_success(
        provider_input_tokens: u64,
        provider_cached_tokens: u64,
        content_fingerprint: impl Into<String>,
        now: DateTime<Utc>,
    ) -> Self {
        Self {
            provider_input_tokens,
            provider_cached_tokens,
            content_fingerprint: content_fingerprint.into(),
            recorded_at: now,
            checkpoint_failure_streak: 0,
            next_attempt_after: None,
        }
    }

    pub fn record_failure(&self, now: DateTime<Utc>) -> Self {
        let streak = self.checkpoint_failure_streak.saturating_add(1);
        Self {
            checkpoint_failure_streak: streak,
            next_attempt_after: Some(
                now + chrono::Duration::seconds(Self::cooldown_seconds(streak)),
            ),
            ..self.clone()
        }
    }

    pub fn allows_attempt(&self, now: DateTime<Utc>) -> bool {
        self.next_attempt_after
            .is_none_or(|deadline| now >= deadline)
    }
}

/// Stable fingerprint over canonical message identity/content for usage anchors.
pub fn context_content_fingerprint(messages: &[Message]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(b"ca-session-usage-anchor-v1\0");
    for message in messages {
        hasher.update(format!("{:?}", message.role).as_bytes());
        hasher.update(message.content.as_bytes());
        if let Some(calls) = message.tool_calls.as_ref() {
            for call in calls {
                hasher.update(call.name.as_bytes());
                hasher.update(call.id.as_bytes());
            }
        }
    }
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionV4 {
    /// Process-local compare-and-swap binding; never exported or model-visible.
    #[serde(skip)]
    pub storage_binding: Option<SessionBinding>,
    pub version: u32,
    pub id: Uuid,
    pub title: String,
    #[serde(default)]
    pub pinned: bool,
    pub workspace: String,
    #[serde(default)]
    pub workspace_roots: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_manual_rule_ids")]
    pub manual_rule_ids: Vec<String>,
    #[serde(default)]
    pub active_skills: Vec<crate::skills::ActiveSkillRef>,
    #[serde(default)]
    pub persistent_skill_refs: Vec<crate::skills::PersistentSkillRef>,
    #[serde(default)]
    pub work_state: crate::work::WorkState,
    #[serde(default)]
    pub review_records: Vec<crate::work::ReviewRecord>,
    #[serde(default)]
    pub agent_summaries: Vec<crate::work::AgentSummary>,
    #[serde(default)]
    pub transcript_annotations: Vec<crate::app::TranscriptMessageAnnotation>,
    #[serde(default)]
    pub transcript_agent_summaries: Vec<crate::app::TranscriptAgentSummary>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub provider: String,
    pub model: String,
    #[serde(default)]
    pub reasoning_effort: Option<crate::config::ReasoningEffort>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub automation_provenance: Option<AutomationSessionProvenanceV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub automation_deliveries: Vec<AutomationSessionProvenanceV1>,
    pub parent: Option<SessionParent>,
    #[serde(
        default,
        skip_serializing_if = "crate::branches::SessionBookmarks::is_empty"
    )]
    pub bookmarks: crate::branches::SessionBookmarks,
    #[serde(
        default,
        skip_serializing_if = "crate::branches::BranchHandoffLedger::is_empty"
    )]
    pub branch_handoffs: crate::branches::BranchHandoffLedger,
    pub messages: Vec<Message>,
    pub context_checkpoint: Option<ContextCheckpoint>,
    #[serde(default)]
    pub queued_inputs: Vec<crate::queue::PendingInput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_inbox: Option<crate::service_inbox::Inbox>,
    #[serde(default)]
    pub fallback_provenance: Vec<crate::fallback::TurnProviderProvenance>,
    #[serde(default)]
    pub import_provenance: Option<ImportProvenance>,
    #[serde(default)]
    pub goal_state: Option<crate::goal::GoalState>,
    #[serde(default)]
    pub workflow_run: Option<crate::workflow::WorkflowRun>,
    #[serde(default)]
    pub attached_worktree: Option<crate::worktree::WorktreeAttachment>,
    #[serde(default, deserialize_with = "deserialize_mechanistic_checkpoint")]
    pub mechanistic_checkpoint: Option<MechanisticCheckpointV1>,
    #[serde(default, skip_serializing_if = "MechanisticCapsuleIndexV1::is_empty")]
    pub mechanistic_capsules: MechanisticCapsuleIndexV1,
    #[serde(
        default,
        skip_serializing_if = "crate::session_artifacts::SessionArtifactLedger::is_empty"
    )]
    pub artifacts: crate::session_artifacts::SessionArtifactLedger,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_usage_anchor: Option<ContextUsageAnchor>,
}

pub type SessionV2 = SessionV4;

impl SessionV4 {
    pub fn new(
        title: impl Into<String>,
        workspace: impl Into<String>,
        provider: impl Into<String>,
        model: impl Into<String>,
        messages: Vec<Message>,
    ) -> Self {
        let now = Utc::now();
        Self {
            storage_binding: None,
            version: 4,
            id: Uuid::new_v4(),
            title: title.into(),
            pinned: false,
            workspace: workspace.into(),
            workspace_roots: Vec::new(),
            manual_rule_ids: Vec::new(),
            active_skills: Vec::new(),
            persistent_skill_refs: Vec::new(),
            work_state: crate::work::WorkState::default(),
            review_records: Vec::new(),
            agent_summaries: Vec::new(),
            transcript_annotations: Vec::new(),
            transcript_agent_summaries: Vec::new(),
            created_at: now,
            updated_at: now,
            provider: provider.into(),
            model: model.into(),
            reasoning_effort: None,
            automation_provenance: None,
            automation_deliveries: Vec::new(),
            parent: None,
            bookmarks: crate::branches::SessionBookmarks::default(),
            branch_handoffs: crate::branches::BranchHandoffLedger::default(),
            messages,
            context_checkpoint: None,
            queued_inputs: Vec::new(),
            service_inbox: None,
            fallback_provenance: Vec::new(),
            import_provenance: None,
            goal_state: None,
            workflow_run: None,
            attached_worktree: None,
            mechanistic_checkpoint: None,
            mechanistic_capsules: MechanisticCapsuleIndexV1::default(),
            artifacts: crate::session_artifacts::SessionArtifactLedger::default(),
            context_usage_anchor: None,
        }
    }

    pub fn record_context_usage_success(
        &mut self,
        provider_input_tokens: u64,
        provider_cached_tokens: u64,
        now: DateTime<Utc>,
    ) {
        let fingerprint = context_content_fingerprint(&self.messages);
        self.context_usage_anchor = Some(ContextUsageAnchor::record_success(
            provider_input_tokens,
            provider_cached_tokens,
            fingerprint,
            now,
        ));
    }

    pub fn record_context_checkpoint_failure(&mut self, now: DateTime<Utc>) {
        let fingerprint = context_content_fingerprint(&self.messages);
        self.context_usage_anchor = Some(match self.context_usage_anchor.take() {
            Some(existing) => existing.record_failure(now),
            None => ContextUsageAnchor {
                provider_input_tokens: 0,
                provider_cached_tokens: 0,
                content_fingerprint: fingerprint,
                recorded_at: now,
                checkpoint_failure_streak: 0,
                next_attempt_after: None,
            }
            .record_failure(now),
        });
    }

    /// Derive integrity metadata for a legacy checkpoint, or validate the
    /// digest on a current checkpoint. Callers must provide the complete
    /// canonical history, not a recent-history page.
    pub fn derive_or_validate_context_checkpoint(
        &mut self,
    ) -> Result<(), ContextCheckpointIntegrityError> {
        if let Some(checkpoint) = self.context_checkpoint.as_mut() {
            checkpoint.derive_or_validate_source(&self.messages)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSummary {
    pub storage_key: String,
    pub id: Uuid,
    pub title: String,
    pub pinned: bool,
    pub workspace: String,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionFamilyMetadata {
    pub summary: SessionSummary,
    pub parent: Option<SessionParent>,
    pub created_at: DateTime<Utc>,
    pub archived: bool,
}

impl SessionSummary {
    pub fn matches(&self, query: &str) -> bool {
        let query = query.trim().to_ascii_lowercase();
        query.is_empty()
            || self.title.to_ascii_lowercase().contains(&query)
            || self.id.to_string().contains(&query)
            || public_session_id(self.id)
                .to_ascii_lowercase()
                .contains(&query)
            || self.workspace.to_ascii_lowercase().contains(&query)
            || self.storage_key.to_ascii_lowercase().contains(&query)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimelineEntry {
    pub turn_index: usize,
    pub through_message_count: usize,
    pub title: String,
    pub completed_at: DateTime<Utc>,
}

#[derive(Clone, Serialize, Deserialize)]
struct SessionBundleMetadataV1 {
    storage_version: u32,
    #[serde(default)]
    metadata_revision: u64,
    storage_key: String,
    id: Uuid,
    generation: Uuid,
    message_count: usize,
    committed_bytes: u64,
    offsets: Vec<u64>,
    history_sha256: String,
    session: serde_json::Value,
}

impl std::fmt::Debug for SessionBundleMetadataV1 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SessionBundleMetadataV1")
            .field("storage_version", &self.storage_version)
            .field("storage_key", &self.storage_key)
            .field("id", &self.id)
            .field("generation", &self.generation)
            .field("message_count", &self.message_count)
            .field("committed_bytes", &self.committed_bytes)
            .field("history_sha256", &"[OMITTED]")
            .field("session", &"[OMITTED]")
            .finish()
    }
}

#[derive(Debug, Clone)]
pub struct SessionHistoryPage {
    pub storage_key: String,
    pub session_id: Uuid,
    pub storage_generation: Uuid,
    pub start_index: usize,
    pub total_messages: usize,
    pub messages: Vec<Message>,
}

#[derive(Debug, Clone)]
pub struct RecentSessionLoad {
    pub session: SessionV2,
    pub storage_key: String,
    pub storage_generation: Option<Uuid>,
    pub start_index: usize,
    pub total_messages: usize,
    pub paged: bool,
}

#[derive(Debug, Serialize)]
struct SessionLogRecord<'a> {
    index: usize,
    message: &'a Message,
}

#[derive(Debug, Deserialize)]
struct OwnedSessionLogRecord {
    index: usize,
    message: Message,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionAutomationBinding {
    pub storage_key: String,
    pub session_id: Uuid,
    pub storage_generation: Option<Uuid>,
    pub message_count: usize,
    pub history_sha256: String,
    pub archived: bool,
    pub committed: SessionBinding,
}

/// The exact committed history and metadata a writer originally loaded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionBinding {
    pub session_id: Uuid,
    pub storage_generation: Option<Uuid>,
    pub message_count: usize,
    pub history_sha256: String,
    pub metadata_revision: u64,
    pub metadata_sha256: String,
}

#[derive(Debug, Clone)]
pub struct SessionStore {
    root: PathBuf,
    archive_root: Option<PathBuf>,
    identity_roots: Vec<PathBuf>,
    access: crate::session_access::SessionAccessCoordinator,
    identity_scan_limits: IdentityScanLimits,
}

#[derive(Debug, Clone, Copy)]
struct IdentityScanLimits {
    max_candidates: usize,
    max_uncached_bytes: u64,
    timeout: std::time::Duration,
}

impl Default for IdentityScanLimits {
    fn default() -> Self {
        Self {
            max_candidates: 16_384,
            max_uncached_bytes: 8 * 1024 * 1024,
            timeout: std::time::Duration::from_millis(25),
        }
    }
}

struct IdentityScanBudget {
    limits: IdentityScanLimits,
    deadline: std::time::Instant,
    candidates: usize,
    uncached_bytes: u64,
}

impl IdentityScanBudget {
    fn new(limits: IdentityScanLimits) -> Self {
        let now = std::time::Instant::now();
        Self {
            deadline: now.checked_add(limits.timeout).unwrap_or(now),
            limits,
            candidates: 0,
            uncached_bytes: 0,
        }
    }

    fn check(&self) -> io::Result<()> {
        if std::time::Instant::now() >= self.deadline {
            return Err(identity_inventory_incomplete("reader deadline reached"));
        }
        Ok(())
    }

    fn candidate(&mut self) -> io::Result<()> {
        self.check()?;
        if self.candidates >= self.limits.max_candidates {
            return Err(identity_inventory_incomplete(
                "directory entry limit reached",
            ));
        }
        self.candidates += 1;
        Ok(())
    }

    fn reserve_probe(&mut self, bytes: u64) -> io::Result<()> {
        self.check()?;
        self.uncached_bytes = self
            .uncached_bytes
            .checked_add(bytes)
            .filter(|bytes| *bytes <= self.limits.max_uncached_bytes)
            .ok_or_else(|| identity_inventory_incomplete("uncached byte limit reached"))?;
        Ok(())
    }
}

fn identity_inventory_incomplete(reason: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::WouldBlock,
        format!(
            "session identity inventory is incomplete; prepare on a worker and retry ({reason})"
        ),
    )
}

impl SessionStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        Self {
            access: crate::session_access::SessionAccessCoordinator::for_session_root(&root),
            identity_roots: vec![root.clone()],
            root,
            archive_root: None,
            identity_scan_limits: IdentityScanLimits::default(),
        }
    }

    pub fn with_archive(root: impl Into<PathBuf>, archive_root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        let archive_root = archive_root.into();
        Self {
            access: crate::session_access::SessionAccessCoordinator::for_session_root(&root),
            identity_roots: vec![root.clone(), archive_root.clone()],
            root,
            archive_root: Some(archive_root),
            identity_scan_limits: IdentityScanLimits::default(),
        }
    }

    pub fn try_writer_lease(
        &self,
        session_id: Uuid,
    ) -> io::Result<crate::session_access::SessionWriterLease> {
        self.access.try_writer(session_id)
    }

    /// Warm compact identity probes on a worker before acquiring/using a writer.
    /// The final owned operation still restats all locations and validates its
    /// exact target schema/history/binding; this is not a writer capability.
    pub fn prepare_identity_inventory(&self) -> io::Result<()> {
        self.identity_inventory(
            None,
            IdentityScanLimits {
                max_candidates: 16_384,
                max_uncached_bytes: 256 * 1024 * 1024,
                timeout: std::time::Duration::from_secs(2),
            },
        )?;
        Ok(())
    }

    pub fn access_coordinator(&self) -> &crate::session_access::SessionAccessCoordinator {
        &self.access
    }

    /// Read committed identity without hydrating bundle history. The caller
    /// holding a writer lease may compare this with its in-memory snapshot.
    pub fn committed_binding(&self, session_id: Uuid) -> io::Result<SessionBinding> {
        self.unique_identity_location(session_id)?;
        let (key, path) = self.resolve_path(&session_id.to_string())?;
        read_committed_binding(&path, &key, session_id)
    }

    pub fn load_owned(
        &self,
        key_or_id: &str,
        lease: &crate::session_access::SessionWriterLease,
    ) -> io::Result<SessionV2> {
        let session = self.load(key_or_id)?;
        self.access.validate(lease, session.id)?;
        self.unique_identity_location(session.id)?;
        Ok(session)
    }

    fn in_root(&self, root: PathBuf) -> Self {
        Self {
            root,
            archive_root: self.archive_root.clone(),
            identity_roots: self.identity_roots.clone(),
            access: self.access.clone(),
            identity_scan_limits: self.identity_scan_limits,
        }
    }

    /// A writer must have one metadata location, even when an interrupted
    /// archive move left identical hard links in both roots. Canonicalizing
    /// directories avoids counting aliases, but files are deliberately not
    /// deduplicated by inode: each surviving location can diverge on save.
    fn unique_identity_location(&self, session_id: Uuid) -> io::Result<Option<PathBuf>> {
        self.identity_inventory(Some(session_id), self.identity_scan_limits)
    }

    fn identity_inventory(
        &self,
        session_id: Option<Uuid>,
        limits: IdentityScanLimits,
    ) -> io::Result<Option<PathBuf>> {
        let mut budget = IdentityScanBudget::new(limits);
        let mut roots = std::collections::HashSet::new();
        let mut location = None;
        for root in &self.identity_roots {
            budget.check()?;
            let root = match std::fs::canonicalize(root) {
                Ok(root) => root,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            };
            if !roots.insert(root.clone()) {
                continue;
            }
            for entry in std::fs::read_dir(&root)? {
                budget.candidate()?;
                let path = entry?.path();
                if path.extension().and_then(|value| value.to_str()) != Some("json") {
                    continue;
                }
                let Some(key) = path.file_stem().and_then(|value| value.to_str()) else {
                    continue;
                };
                let direct_identity =
                    session_id.is_some() && Uuid::parse_str(key).ok() == session_id;
                let probe = match read_session_identity_probe(&path, &mut budget) {
                    Ok(probe) => probe,
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Err(error),
                    Err(error) if direct_identity => return Err(error),
                    Err(error) if error.kind() == io::ErrorKind::InvalidData => continue,
                    Err(_) => {
                        return Err(identity_inventory_incomplete("file could not be observed"))
                    }
                };
                let Some(session_id) = session_id else {
                    continue;
                };
                let identity = probe.outer_id.or_else(|| {
                    (probe.version == Some(1))
                        .then(|| Uuid::new_v5(&Uuid::NAMESPACE_URL, key.as_bytes()))
                });
                if identity != Some(session_id)
                    && probe.inner_id != Some(session_id)
                    && !direct_identity
                {
                    continue;
                }
                if identity != Some(session_id) || probe.inner_id.is_some_and(|id| id != session_id)
                {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "saved session identity metadata cannot be validated: UUID mismatch",
                    ));
                }
                if location.replace(path).is_some() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "saved session identity exists in multiple locations; resolve the duplicate before writing",
                    ));
                }
            }
        }
        budget.check()?;
        Ok(location)
    }

    pub fn list(&self) -> io::Result<Vec<SessionSummary>> {
        if !self.root.exists() {
            return Ok(Vec::new());
        }
        let mut summaries = Vec::new();
        for entry in std::fs::read_dir(&self.root)? {
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => continue,
            };
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let storage_key = match path.file_stem().and_then(|value| value.to_str()) {
                Some(value) => value.to_string(),
                None => continue,
            };
            if let Ok(summary) = read_session_summary(&path, &storage_key) {
                summaries.push(summary);
            }
        }
        summaries.sort_by(|left, right| {
            right
                .pinned
                .cmp(&left.pinned)
                .then_with(|| right.updated_at.cmp(&left.updated_at))
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(summaries)
    }

    pub fn list_archived(&self) -> io::Result<Vec<SessionSummary>> {
        self.in_root(self.archive_root()?).list()
    }

    /// Reads bundle metadata without loading history. Legacy single files are
    /// decoded once because they have no separate metadata representation.
    pub fn family_metadata(&self) -> io::Result<Vec<SessionFamilyMetadata>> {
        let mut family = read_family_root(&self.root, false)?;
        if let Some(archive) = &self.archive_root {
            family.extend(read_family_root(archive, true)?);
        }
        Ok(family)
    }

    pub fn set_pinned(
        &self,
        storage_key: &str,
        expected_uuid: Uuid,
        pinned: bool,
    ) -> io::Result<()> {
        let lease = self.try_writer_lease(expected_uuid)?;
        self.set_pinned_owned(storage_key, expected_uuid, pinned, &lease)
    }

    pub fn set_pinned_owned(
        &self,
        storage_key: &str,
        expected_uuid: Uuid,
        pinned: bool,
        lease: &crate::session_access::SessionWriterLease,
    ) -> io::Result<()> {
        self.access.validate(lease, expected_uuid)?;
        validate_storage_key(storage_key)?;
        self.unique_identity_location(expected_uuid)?;
        let path = self.root.join(format!("{storage_key}.json"));
        if let Some(mut metadata) = read_bundle_metadata_if_present(&path, storage_key)? {
            verify_bundle_id(&metadata, expected_uuid)?;
            let session = metadata_session(&metadata, Vec::new())?;
            let mut value = metadata.session;
            let object = value.as_object_mut().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "session metadata is not an object",
                )
            })?;
            object.insert("pinned".into(), serde_json::Value::Bool(pinned));
            object.insert(
                "updated_at".into(),
                serde_json::to_value(session.updated_at).map_err(io::Error::other)?,
            );
            metadata.session = value;
            metadata.metadata_revision = next_metadata_revision(metadata.metadata_revision)?;
            return write_bundle_metadata(&path, &metadata);
        }
        let mut session = read_session_file(&path, storage_key)?;
        verify_session_id(&session, expected_uuid)?;
        session.version = 4;
        session.pinned = pinned;
        atomic_write_with(&path, |file| {
            serde_json::to_writer_pretty(&mut *file, &session).map_err(io::Error::other)?;
            file.write_all(b"\n")
        })
    }

    pub fn rename(
        &self,
        storage_key: &str,
        expected_uuid: Uuid,
        title: &str,
        archived: bool,
    ) -> io::Result<()> {
        let lease = self.try_writer_lease(expected_uuid)?;
        self.rename_owned(storage_key, expected_uuid, title, archived, &lease)
    }

    pub fn rename_owned(
        &self,
        storage_key: &str,
        expected_uuid: Uuid,
        title: &str,
        archived: bool,
        lease: &crate::session_access::SessionWriterLease,
    ) -> io::Result<()> {
        self.access.validate(lease, expected_uuid)?;
        let title = title.trim();
        if title.is_empty()
            || title.chars().any(char::is_control)
            || unicode_width::UnicodeWidthStr::width(title) > MAX_RENAMED_TITLE_CELLS
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "session name must be 1 to 100 display cells without control characters",
            ));
        }
        validate_storage_key(storage_key)?;
        self.unique_identity_location(expected_uuid)?;
        let root = if archived {
            self.archive_root()?
        } else {
            self.root.clone()
        };
        let path = root.join(format!("{storage_key}.json"));
        if let Some(mut metadata) = read_bundle_metadata_if_present(&path, storage_key)? {
            verify_bundle_id(&metadata, expected_uuid)?;
            let mut value = metadata.session;
            let object = value.as_object_mut().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "session metadata is not an object",
                )
            })?;
            object.insert("title".into(), serde_json::Value::String(title.to_string()));
            metadata.session = value;
            metadata.metadata_revision = next_metadata_revision(metadata.metadata_revision)?;
            return write_bundle_metadata(&path, &metadata);
        }
        let mut session = read_session_file(&path, storage_key)?;
        verify_session_id(&session, expected_uuid)?;
        session.version = 4;
        session.title = title.to_string();
        atomic_write_with(&path, |file| {
            serde_json::to_writer_pretty(&mut *file, &session).map_err(io::Error::other)?;
            file.write_all(b"\n")
        })
    }

    pub fn archive(&self, storage_key: &str, expected_uuid: Uuid) -> io::Result<()> {
        let _lease = self.try_writer_lease(expected_uuid)?;
        validate_storage_key(storage_key)?;
        let archive = self.archive_root()?;
        if archive.join(format!("{storage_key}.json")).exists() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "archive destination already exists",
            ));
        }
        self.unique_identity_location(expected_uuid)?;
        move_exact_session(&self.root, &archive, storage_key, expected_uuid)
    }

    pub fn restore(&self, storage_key: &str, expected_uuid: Uuid) -> io::Result<()> {
        let _lease = self.try_writer_lease(expected_uuid)?;
        validate_storage_key(storage_key)?;
        if self.root.join(format!("{storage_key}.json")).exists() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "restore destination already exists",
            ));
        }
        self.unique_identity_location(expected_uuid)?;
        move_exact_session(
            &self.archive_root()?,
            &self.root,
            storage_key,
            expected_uuid,
        )
    }

    fn archive_root(&self) -> io::Result<PathBuf> {
        self.archive_root.clone().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::Unsupported,
                "saved-session archive is not configured",
            )
        })
    }

    pub fn load(&self, key_or_id: &str) -> io::Result<SessionV2> {
        let direct = self.root.join(format!("{key_or_id}.json"));
        if direct.is_file() {
            return read_session_file(&direct, key_or_id);
        }
        let requested_id = Uuid::parse_str(key_or_id)
            .ok()
            .or_else(|| parse_public_session_id(key_or_id));
        for summary in self.list()? {
            if requested_id == Some(summary.id) {
                let path = self.root.join(format!("{}.json", summary.storage_key));
                return read_session_file(&path, &summary.storage_key);
            }
        }
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("session not found: {key_or_id}"),
        ))
    }

    pub fn load_archived(&self, key_or_id: &str) -> io::Result<SessionV2> {
        self.in_root(self.archive_root()?).load(key_or_id)
    }

    pub fn automation_snapshot(
        &self,
        session_id: Uuid,
    ) -> io::Result<(SessionV2, SessionAutomationBinding)> {
        self.unique_identity_location(session_id)?;
        match self.automation_snapshot_in_root(session_id, false) {
            Ok(result) => Ok(result),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                self.automation_snapshot_in_root(session_id, true)
            }
            Err(error) => Err(error),
        }
    }

    fn automation_snapshot_in_root(
        &self,
        session_id: Uuid,
        archived: bool,
    ) -> io::Result<(SessionV2, SessionAutomationBinding)> {
        let store = if archived {
            self.in_root(self.archive_root()?)
        } else {
            self.in_root(self.root.clone())
        };
        let summary = store
            .list()?
            .into_iter()
            .find(|summary| summary.id == session_id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "session not found"))?;
        let path = store.root.join(format!("{}.json", summary.storage_key));
        let session = read_session_file(&path, &summary.storage_key)?;
        let committed = session
            .storage_binding
            .clone()
            .ok_or_else(|| io::Error::other("saved session has no commit binding"))?;
        let (storage_generation, message_count, history_sha256) =
            if let Some(metadata) = read_bundle_metadata_if_present(&path, &summary.storage_key)? {
                validate_bundle_files(&path, &metadata)?;
                (
                    Some(metadata.generation),
                    metadata.message_count,
                    metadata.history_sha256,
                )
            } else {
                (
                    None,
                    session.messages.len(),
                    hash_log_records(&session.messages)?,
                )
            };
        Ok((
            session,
            SessionAutomationBinding {
                storage_key: summary.storage_key,
                session_id,
                storage_generation,
                message_count,
                history_sha256,
                archived,
                committed,
            },
        ))
    }

    pub fn append_automation_delivery_owned(
        &self,
        binding: &SessionAutomationBinding,
        provenance: AutomationSessionProvenanceV1,
        automation_name: &str,
        prompt: &str,
        result: &str,
        lease: &crate::session_access::SessionWriterLease,
    ) -> io::Result<()> {
        self.access.validate(lease, binding.session_id)?;
        self.unique_identity_location(binding.session_id)?;
        let root = if binding.archived {
            self.archive_root()?
        } else {
            self.root.clone()
        };
        let store = self.in_root(root.clone());
        let path = root.join(format!("{}.json", binding.storage_key));
        let mut session = read_session_file(&path, &binding.storage_key)?;
        verify_session_id(&session, binding.session_id)?;
        if let Some(existing) = session
            .automation_deliveries
            .iter()
            .find(|item| item.run_id == provenance.run_id)
        {
            if existing == &provenance {
                return Ok(());
            }
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "automation delivery identity collides with different provenance",
            ));
        }
        let (generation, message_count, history_sha256) =
            if let Some(metadata) = read_bundle_metadata_if_present(&path, &binding.storage_key)? {
                validate_bundle_files(&path, &metadata)?;
                (
                    Some(metadata.generation),
                    metadata.message_count,
                    metadata.history_sha256,
                )
            } else {
                let session = read_session_file(&path, &binding.storage_key)?;
                (
                    None,
                    session.messages.len(),
                    hash_log_records(&session.messages)?,
                )
            };
        if generation != binding.storage_generation
            || message_count != binding.message_count
            || history_sha256 != binding.history_sha256
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "session changed after automation dispatch",
            ));
        }
        if session.storage_binding.as_ref() != Some(&binding.committed) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "session metadata changed after automation dispatch",
            ));
        }
        let now = Utc::now();
        session.messages.push(Message {
            role: Role::User,
            content: format!(
                "[Automation: {} · occurrence {}]\n\n{}",
                automation_name, provenance.occurrence_id, prompt
            ),
            tool_calls: None,
            thinking: None,
            response_items: Vec::new(),
            timestamp: now,
        });
        session.messages.push(Message {
            role: Role::Assistant,
            content: result.to_string(),
            tool_calls: None,
            thinking: None,
            response_items: Vec::new(),
            timestamp: now,
        });
        if session.automation_deliveries.len() >= 512 {
            session.automation_deliveries.remove(0);
        }
        session.automation_deliveries.push(provenance);
        let mut session = session;
        store.save_owned(&mut session, lease)?;
        Ok(())
    }

    fn resolve_path(&self, key_or_id: &str) -> io::Result<(String, PathBuf)> {
        let direct = self.root.join(format!("{key_or_id}.json"));
        if direct.is_file() {
            validate_storage_key(key_or_id)?;
            return Ok((key_or_id.into(), direct));
        }
        let requested_id = Uuid::parse_str(key_or_id)
            .ok()
            .or_else(|| parse_public_session_id(key_or_id));
        for summary in self.list()? {
            if requested_id == Some(summary.id) {
                let path = self.root.join(format!("{}.json", summary.storage_key));
                return Ok((summary.storage_key, path));
            }
        }
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("session not found: {key_or_id}"),
        ))
    }

    pub fn load_recent(&self, key_or_id: &str, page_size: usize) -> io::Result<RecentSessionLoad> {
        let (storage_key, path) = self.resolve_path(key_or_id)?;
        let Some(metadata) = read_bundle_metadata_if_present(&path, &storage_key)? else {
            let session = read_session_file(&path, &storage_key)?;
            let total_messages = session.messages.len();
            return Ok(RecentSessionLoad {
                session,
                storage_key,
                storage_generation: None,
                start_index: 0,
                total_messages,
                paged: false,
            });
        };
        validate_page_size(page_size)?;
        validate_bundle_files(&path, &metadata)?;
        let start_index = metadata.message_count.saturating_sub(page_size);
        let messages =
            read_bundle_message_range(&path, &metadata, start_index, metadata.message_count)?;
        let total_messages = metadata.message_count;
        let mut session = metadata_session(&metadata, messages)?;
        if start_index == 0 {
            session
                .derive_or_validate_context_checkpoint()
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        }
        Ok(RecentSessionLoad {
            session,
            storage_key,
            storage_generation: Some(metadata.generation),
            start_index,
            total_messages,
            paged: start_index > 0,
        })
    }

    /// Dashboard observations never hydrate legacy whole-file histories. At
    /// most four committed records and 8 MiB of bundle metadata are examined.
    pub fn load_recent_committed_bounded(
        &self,
        session_id: Uuid,
        page_size: usize,
    ) -> io::Result<RecentSessionLoad> {
        if page_size == 0 || page_size > 4 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "committed previews require 1-4 messages",
            ));
        }
        let direct_key = session_id.to_string();
        let direct = self.root.join(format!("{direct_key}.json"));
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(100);
        let mut remaining = MAX_SESSION_METADATA_BYTES;
        let mut candidates = Vec::new();
        if direct.exists() {
            candidates.push((direct_key, direct));
        } else {
            for entry in std::fs::read_dir(&self.root)?.take(512) {
                if std::time::Instant::now() >= deadline {
                    break;
                }
                let path = entry?.path();
                if path.extension().and_then(|value| value.to_str()) == Some("json") {
                    if let Some(key) = path.file_stem().and_then(|value| value.to_str()) {
                        candidates.push((key.to_owned(), path));
                    }
                }
            }
        }
        for (storage_key, path) in candidates {
            if std::time::Instant::now() >= deadline {
                break;
            }
            let Ok(metadata) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            if !metadata.is_file()
                || metadata.file_type().is_symlink()
                || metadata.len() > remaining
            {
                continue;
            }
            remaining -= metadata.len();
            let Ok(Some(metadata)) = read_bundle_metadata_if_present(&path, &storage_key) else {
                continue;
            };
            if metadata.id != session_id {
                continue;
            }
            validate_bundle_files(&path, &metadata)?;
            let start_index = metadata.message_count.saturating_sub(page_size);
            let first_byte = metadata.offsets.get(start_index).copied().unwrap_or(0);
            if metadata.committed_bytes.saturating_sub(first_byte)
                > (page_size * (MAX_SESSION_MESSAGE_BYTES + 1)) as u64
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "committed preview exceeds its byte bound",
                ));
            }
            let messages =
                read_bundle_message_range(&path, &metadata, start_index, metadata.message_count)?;
            let session = metadata_session(&metadata, messages)?;
            return Ok(RecentSessionLoad {
                session,
                storage_key,
                storage_generation: Some(metadata.generation),
                start_index,
                total_messages: metadata.message_count,
                paged: start_index > 0,
            });
        }
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            "bounded committed bundle preview is unavailable",
        ))
    }

    pub fn load_page(
        &self,
        storage_key: &str,
        expected_uuid: Uuid,
        expected_generation: Uuid,
        before_index: usize,
        page_size: usize,
    ) -> io::Result<SessionHistoryPage> {
        validate_storage_key(storage_key)?;
        validate_page_size(page_size)?;
        let path = self.root.join(format!("{storage_key}.json"));
        let metadata = read_bundle_metadata_if_present(&path, storage_key)?.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::Unsupported,
                "legacy sessions do not have paged history",
            )
        })?;
        verify_bundle_id(&metadata, expected_uuid)?;
        if metadata.generation != expected_generation {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "saved session history changed; resume the session again",
            ));
        }
        validate_bundle_files(&path, &metadata)?;
        if before_index > metadata.message_count {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "history page boundary exceeds the session",
            ));
        }
        let start_index = before_index.saturating_sub(page_size);
        let messages = read_bundle_message_range(&path, &metadata, start_index, before_index)?;
        Ok(SessionHistoryPage {
            storage_key: storage_key.into(),
            session_id: metadata.id,
            storage_generation: metadata.generation,
            start_index,
            total_messages: metadata.message_count,
            messages,
        })
    }

    pub fn save(&self, session: &mut SessionV2) -> io::Result<PathBuf> {
        let lease = self.try_writer_lease(session.id)?;
        self.save_owned(session, &lease)
    }

    pub fn save_owned(
        &self,
        session: &mut SessionV2,
        lease: &crate::session_access::SessionWriterLease,
    ) -> io::Result<PathBuf> {
        self.access.validate(lease, session.id)?;
        validate_import_provenance(session)?;
        let location = self.unique_identity_location(session.id)?;
        std::fs::create_dir_all(&self.root)?;
        let target = if let Some(location) = location {
            let target_root = std::fs::canonicalize(&self.root)?;
            if location.parent() != Some(target_root.as_path()) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "saved session identity belongs to another storage root",
                ));
            }
            self.root.join(
                location
                    .file_name()
                    .ok_or_else(|| io::Error::other("invalid session storage name"))?,
            )
        } else {
            self.root.join(format!("{}.json", session.id))
        };
        if target.exists() {
            let storage_key = target
                .file_stem()
                .and_then(|value| value.to_str())
                .ok_or_else(|| io::Error::other("invalid session storage name"))?;
            let current_binding = read_committed_binding(&target, storage_key, session.id)?;
            if session.storage_binding.as_ref() != Some(&current_binding) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "saved session changed since it was loaded; refresh before saving",
                ));
            }
        } else if session.storage_binding.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "saved session was moved or removed since it was loaded",
            ));
        }
        session.version = 4;
        session.updated_at = Utc::now();
        save_session_bundle(&target, session)?;
        let storage_key = target
            .file_stem()
            .and_then(|value| value.to_str())
            .ok_or_else(|| io::Error::other("invalid session storage name"))?;
        let metadata = read_bundle_metadata_if_present(&target, storage_key)?
            .ok_or_else(|| io::Error::other("saved session commit is missing"))?;
        session.storage_binding = Some(binding_for_metadata(&metadata)?);
        Ok(target)
    }

    pub fn save_inert_import(&self, session: &mut SessionV2) -> io::Result<PathBuf> {
        session.service_inbox = None;
        let _lease = self.try_writer_lease(session.id)?;
        validate_import_provenance(session)?;
        if self.unique_identity_location(session.id)?.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "inert import session identity already exists",
            ));
        }
        if session.import_provenance.is_none() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "inert import save requires bounded import provenance",
            ));
        }
        if session.updated_at < session.created_at {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "inert import timestamps are inconsistent",
            ));
        }
        session.version = 4;
        std::fs::create_dir_all(&self.root)?;
        let target = self.root.join(format!("{}.json", session.id));
        if target.exists() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "inert import session identity already exists",
            ));
        }
        save_session_bundle(&target, session)?;
        let metadata = read_bundle_metadata_if_present(&target, &session.id.to_string())?
            .ok_or_else(|| io::Error::other("saved import commit is missing"))?;
        session.storage_binding = Some(binding_for_metadata(&metadata)?);
        Ok(target)
    }

    pub fn delete(&self, storage_key: &str, expected_uuid: Uuid) -> io::Result<()> {
        self.delete_with(storage_key, expected_uuid, |path| {
            std::fs::remove_file(path)
        })
    }

    /// Permanently remove one saved session from the active list or the archive.
    /// Recovery state for that session is removed with it. The sessions directory
    /// itself stays in place.
    pub fn purge(&self, storage_key: &str, expected_uuid: Uuid, archived: bool) -> io::Result<()> {
        if archived {
            self.in_root(self.archive_root()?)
                .delete(storage_key, expected_uuid)
        } else {
            self.delete(storage_key, expected_uuid)
        }
    }

    fn delete_with<F>(
        &self,
        storage_key: &str,
        expected_uuid: Uuid,
        mut unlink: F,
    ) -> io::Result<()>
    where
        F: FnMut(&Path) -> io::Result<()>,
    {
        let _lease = self.try_writer_lease(expected_uuid)?;
        validate_storage_key(storage_key)?;
        self.unique_identity_location(expected_uuid)?;
        let path = self.root.join(format!("{storage_key}.json"));
        if let Some(metadata) = read_bundle_metadata_if_present(&path, storage_key)? {
            verify_bundle_id(&metadata, expected_uuid)?;
            validate_bundle_files(&path, &metadata)?;
            unlink(&path)?;
            unlink(&bundle_log_path(&path, &metadata)).map_err(|error| {
                io::Error::new(
                    error.kind(),
                    format!("session was deleted, but history cleanup failed: {error}"),
                )
            })?;
            self.purge_side_state(expected_uuid);
            return Ok(());
        }
        let session = read_session_file(&path, storage_key)?;
        verify_session_id(&session, expected_uuid)?;
        unlink(&path)?;
        self.purge_side_state(expected_uuid);
        Ok(())
    }

    /// Drop per-session recovery state left behind after the session bundle is
    /// gone. Recovery journals key off the session id and have nothing left to
    /// recover once the conversation is deleted. The `.session-access` lock is
    /// intentionally left in place: it is the flock target for this very lease,
    /// and unlinking it would let two processes lock different inodes.
    fn purge_side_state(&self, expected_uuid: Uuid) {
        let scope = self.access.scope();
        let _ = std::fs::remove_dir_all(scope.join("recovery").join(expected_uuid.to_string()));
    }

    pub fn delete_retention_batch(
        &self,
        candidates: &[crate::session_retention::RetentionCandidate],
        current_id: Option<Uuid>,
    ) -> Result<
        crate::session_retention::RetentionDeleteOutcome,
        crate::session_retention::RetentionDeleteFailure,
    > {
        self.delete_retention_batch_with(candidates, current_id, |path| std::fs::remove_file(path))
    }

    fn delete_retention_batch_with<F>(
        &self,
        candidates: &[crate::session_retention::RetentionCandidate],
        current_id: Option<Uuid>,
        mut unlink: F,
    ) -> Result<
        crate::session_retention::RetentionDeleteOutcome,
        crate::session_retention::RetentionDeleteFailure,
    >
    where
        F: FnMut(&Path) -> io::Result<()>,
    {
        let mut storage_keys = std::collections::HashSet::new();
        let mut ids = std::collections::HashSet::new();
        for candidate in candidates {
            if !storage_keys.insert(candidate.storage_key.as_str()) || !ids.insert(candidate.id) {
                return Err(retention_delete_failure(
                    "retention preview contains a duplicate session".into(),
                    0,
                    candidates.len(),
                ));
            }
            if let Err(error) = self.validate_retention_candidate(candidate, current_id) {
                return Err(retention_delete_failure(
                    format!("retention preflight failed: {error}"),
                    0,
                    candidates.len(),
                ));
            }
            if let Err(error) = self.unique_identity_location(candidate.id) {
                return Err(retention_delete_failure(
                    format!("retention identity preflight failed: {error}"),
                    0,
                    candidates.len(),
                ));
            }
        }

        let mut removed = 0;
        for candidate in candidates {
            let _lease = self.try_writer_lease(candidate.id).map_err(|error| {
                retention_delete_failure(
                    format!("retention ownership failed: {error}"),
                    removed,
                    candidates.len().saturating_sub(removed),
                )
            })?;
            self.unique_identity_location(candidate.id)
                .map_err(|error| {
                    retention_delete_failure(
                        format!("retention identity changed before deletion: {error}"),
                        removed,
                        candidates.len().saturating_sub(removed),
                    )
                })?;
            if let Err(error) = self.validate_retention_candidate(candidate, current_id) {
                return Err(retention_delete_failure(
                    format!("retention changed before deletion: {error}"),
                    removed,
                    candidates.len().saturating_sub(removed),
                ));
            }
            let path = self.root.join(format!("{}.json", candidate.storage_key));
            let bundle_log = match read_bundle_metadata_if_present(&path, &candidate.storage_key) {
                Ok(Some(metadata)) => Some(bundle_log_path(&path, &metadata)),
                Ok(None) => None,
                Err(error) => {
                    return Err(retention_delete_failure(
                        format!("retention changed before deletion: {error}"),
                        removed,
                        candidates.len().saturating_sub(removed),
                    ));
                }
            };
            if let Err(error) = unlink(&path) {
                return Err(retention_delete_failure(
                    format!(
                        "could not delete session {}: {error}",
                        candidate.storage_key
                    ),
                    removed,
                    candidates.len().saturating_sub(removed),
                ));
            }
            removed += 1;
            if let Some(log_path) = bundle_log {
                if let Err(error) = unlink(&log_path) {
                    return Err(retention_delete_failure(
                        format!(
                            "session {} was deleted, but history cleanup failed: {error}",
                            candidate.storage_key
                        ),
                        removed,
                        candidates.len().saturating_sub(removed),
                    ));
                }
            }
        }
        Ok(crate::session_retention::RetentionDeleteOutcome { removed })
    }

    fn validate_retention_candidate(
        &self,
        candidate: &crate::session_retention::RetentionCandidate,
        current_id: Option<Uuid>,
    ) -> io::Result<()> {
        validate_storage_key(&candidate.storage_key)?;
        if current_id == Some(candidate.id) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "the current session is protected",
            ));
        }
        let path = self.root.join(format!("{}.json", candidate.storage_key));
        let metadata = std::fs::symlink_metadata(&path)?;
        if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "session path is not a regular file",
            ));
        }
        let summary = read_session_summary(&path, &candidate.storage_key)?;
        if summary.id != candidate.id {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "saved session changed; refresh and try again",
            ));
        }
        if summary.pinned {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "a session became pinned",
            ));
        }
        Ok(())
    }

    pub fn branch(
        &self,
        source: &SessionV2,
        through_message_count: usize,
        branch_turn: usize,
        title: impl Into<String>,
    ) -> io::Result<SessionV2> {
        let mut branch = branch_in_memory(source, through_message_count, branch_turn, title)?;
        self.save(&mut branch)?;
        Ok(branch)
    }
}

fn retention_delete_failure(
    message: String,
    removed: usize,
    remaining: usize,
) -> crate::session_retention::RetentionDeleteFailure {
    crate::session_retention::RetentionDeleteFailure {
        message,
        removed,
        remaining,
    }
}

fn validate_storage_key(storage_key: &str) -> io::Result<()> {
    let key_path = Path::new(storage_key);
    let mut components = key_path.components();
    let valid_component = matches!(components.next(), Some(std::path::Component::Normal(_)))
        && components.next().is_none()
        && !storage_key.is_empty();
    if valid_component {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "session storage key must be one filename component",
        ))
    }
}

fn verify_session_id(session: &SessionV2, expected_uuid: Uuid) -> io::Result<()> {
    verify_session_identity(session.id, expected_uuid)
}

fn verify_session_identity(actual_uuid: Uuid, expected_uuid: Uuid) -> io::Result<()> {
    if actual_uuid == expected_uuid {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "saved session changed; refresh and try again",
        ))
    }
}

fn move_exact_session(
    source_root: &Path,
    destination_root: &Path,
    storage_key: &str,
    expected_uuid: Uuid,
) -> io::Result<()> {
    validate_storage_key(storage_key)?;
    let source = source_root.join(format!("{storage_key}.json"));
    if let Some(metadata) = read_bundle_metadata_if_present(&source, storage_key)? {
        verify_bundle_id(&metadata, expected_uuid)?;
        validate_bundle_files(&source, &metadata)?;
        std::fs::create_dir_all(destination_root)?;
        let destination = destination_root.join(format!("{storage_key}.json"));
        let source_log = bundle_log_path(&source, &metadata);
        let destination_log = destination_root.join(bundle_log_name(&metadata));
        std::fs::hard_link(&source_log, &destination_log)?;
        if let Err(error) = std::fs::hard_link(&source, &destination) {
            let _ = std::fs::remove_file(&destination_log);
            return Err(error);
        }
        if let Err(error) = std::fs::remove_file(&source) {
            let _ = std::fs::remove_file(&destination);
            let _ = std::fs::remove_file(&destination_log);
            return Err(error);
        }
        if let Err(error) = std::fs::remove_file(&source_log) {
            let rollback = std::fs::hard_link(&destination, &source);
            if rollback.is_ok() {
                let _ = std::fs::remove_file(&destination);
                let _ = std::fs::remove_file(&destination_log);
            }
            return Err(error);
        }
        return Ok(());
    }
    let session = read_session_file(&source, storage_key)?;
    verify_session_id(&session, expected_uuid)?;
    std::fs::create_dir_all(destination_root)?;
    let destination = destination_root.join(format!("{storage_key}.json"));
    std::fs::hard_link(&source, &destination)?;
    if let Err(error) = std::fs::remove_file(&source) {
        let _ = std::fs::remove_file(&destination);
        return Err(error);
    }
    Ok(())
}

pub fn branch_in_memory(
    source: &SessionV2,
    through_message_count: usize,
    branch_turn: usize,
    title: impl Into<String>,
) -> io::Result<SessionV2> {
    if through_message_count > source.messages.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "branch boundary exceeds source history",
        ));
    }
    let mut branch = SessionV2::new(
        title,
        source.workspace.clone(),
        source.provider.clone(),
        source.model.clone(),
        source.messages[..through_message_count].to_vec(),
    );
    branch.parent = Some(SessionParent {
        session_id: source.id,
        branch_turn,
    });
    branch.reasoning_effort = source.reasoning_effort;
    branch.workspace_roots = source.workspace_roots.clone();
    branch.active_skills = source.active_skills.clone();
    branch.persistent_skill_refs = source.persistent_skill_refs.clone();
    branch.work_state = if through_message_count == source.messages.len() {
        source.work_state.fork_full()
    } else {
        crate::work::WorkState::historical_omission()
    };
    branch.review_records = if through_message_count == source.messages.len() {
        source.review_records.clone()
    } else {
        Vec::new()
    };
    branch.agent_summaries.clear();
    let retained_timestamps = branch
        .messages
        .iter()
        .map(|message| message.timestamp)
        .collect::<std::collections::HashSet<_>>();
    branch.transcript_annotations = source
        .transcript_annotations
        .iter()
        .filter(|annotation| retained_timestamps.contains(&annotation.message_timestamp))
        .cloned()
        .collect();
    let retained_turns = branch
        .transcript_annotations
        .iter()
        .map(|annotation| annotation.turn_id)
        .collect::<std::collections::HashSet<_>>();
    branch.transcript_agent_summaries = source
        .transcript_agent_summaries
        .iter()
        .filter(|summary| retained_turns.contains(&summary.turn_id))
        .cloned()
        .collect();
    branch.context_checkpoint = source
        .context_checkpoint
        .clone()
        .filter(|checkpoint| checkpoint.through_message_count <= through_message_count);
    branch
        .derive_or_validate_context_checkpoint()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    branch.fallback_provenance = source
        .fallback_provenance
        .iter()
        .filter(|entry| entry.user_message_index < through_message_count)
        .cloned()
        .collect();
    branch.goal_state = if through_message_count == source.messages.len() {
        source
            .goal_state
            .as_ref()
            .map(crate::goal::GoalState::fork_full)
    } else {
        None
    };
    branch.workflow_run = if through_message_count == source.messages.len() {
        source
            .workflow_run
            .as_ref()
            .map(crate::workflow::WorkflowRun::fork_full)
    } else {
        None
    };
    branch.mechanistic_checkpoint = if through_message_count == source.messages.len() {
        source.mechanistic_checkpoint.clone()
    } else {
        None
    };
    branch.mechanistic_capsules = if through_message_count == source.messages.len() {
        source.mechanistic_capsules.clone()
    } else {
        MechanisticCapsuleIndexV1::default()
    };
    branch.artifacts = source.artifacts.clone();
    branch.artifacts.retain_through(through_message_count);
    branch.attached_worktree = source.attached_worktree.clone();
    branch.bookmarks = source
        .bookmarks
        .retained_prefix(&source.messages, through_message_count);
    // A handoff belongs to its explicit target. A fork starts without imported
    // model context or actionable queued feedback, even at the full boundary.
    branch.branch_handoffs = crate::branches::BranchHandoffLedger::default();
    Ok(branch)
}

pub fn completed_turns(messages: &[Message]) -> Vec<TimelineEntry> {
    let mut entries = Vec::new();
    let mut current_title = String::new();
    let mut saw_user = false;
    for (index, message) in messages.iter().enumerate() {
        if message.role == Role::User {
            saw_user = true;
            current_title = message
                .content
                .lines()
                .next()
                .unwrap_or("User turn")
                .chars()
                .take(72)
                .collect();
        }
        if saw_user && message.role == Role::Assistant && message.tool_calls.is_none() {
            entries.push(TimelineEntry {
                turn_index: entries.len() + 1,
                through_message_count: index + 1,
                title: current_title.clone(),
                completed_at: message.timestamp,
            });
            saw_user = false;
        }
    }
    entries
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SessionV1 {
    version: u32,
    model: String,
    provider: String,
    created: String,
    name: String,
    messages: Vec<SessionMessageV1>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SessionMessageV1 {
    role: String,
    content: String,
    tool_calls: Option<Vec<SessionToolCallV1>>,
    timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SessionToolCallV1 {
    id: String,
    name: String,
    arguments: String,
    output: Option<String>,
}

fn read_session_summary(path: &Path, storage_key: &str) -> io::Result<SessionSummary> {
    Ok(read_family_metadata(path, storage_key, false)?.summary)
}

#[derive(Deserialize)]
struct SessionIdentityProbe {
    id: Option<serde_json::Value>,
    version: Option<serde_json::Value>,
    session: Option<serde_json::Value>,
}

#[derive(Clone)]
struct IdentityObservation {
    outer_id: Option<Uuid>,
    inner_id: Option<Uuid>,
    version: Option<u64>,
}

#[derive(Clone, PartialEq, Eq)]
struct IdentityFileStamp {
    len: u64,
    modified: Option<std::time::SystemTime>,
    #[cfg(unix)]
    unix: [u64; 6],
}

impl IdentityFileStamp {
    fn for_metadata(metadata: &std::fs::Metadata) -> Self {
        Self {
            len: metadata.len(),
            modified: metadata.modified().ok(),
            #[cfg(unix)]
            unix: {
                use std::os::unix::fs::MetadataExt;
                [
                    metadata.dev(),
                    metadata.ino(),
                    metadata.mtime() as u64,
                    metadata.mtime_nsec() as u64,
                    metadata.ctime() as u64,
                    metadata.ctime_nsec() as u64,
                ]
            },
        }
    }
}

#[derive(Clone)]
struct IdentityProbeFailure {
    message: String,
}

#[derive(Clone)]
struct CachedIdentityProbe {
    stamp: IdentityFileStamp,
    result: Result<IdentityObservation, IdentityProbeFailure>,
}

type IdentityProbeCache = std::collections::BTreeMap<PathBuf, CachedIdentityProbe>;

fn identity_probe_cache() -> &'static std::sync::Mutex<IdentityProbeCache> {
    static CACHE: std::sync::OnceLock<std::sync::Mutex<IdentityProbeCache>> =
        std::sync::OnceLock::new();
    CACHE.get_or_init(|| std::sync::Mutex::new(IdentityProbeCache::new()))
}

fn cached_identity_probe(path: &Path, stamp: &IdentityFileStamp) -> Option<CachedIdentityProbe> {
    // Portable mtime alone cannot prove a cached probe is current.
    if !cfg!(unix) {
        return None;
    }
    let cache = match identity_probe_cache().try_lock() {
        Ok(cache) => cache,
        Err(std::sync::TryLockError::Poisoned(error)) => error.into_inner(),
        Err(std::sync::TryLockError::WouldBlock) => return None,
    };
    cache
        .get(path)
        .filter(|probe| &probe.stamp == stamp)
        .cloned()
}

fn cache_identity_probe(path: &Path, probe: CachedIdentityProbe) {
    if !cfg!(unix) {
        return;
    }
    let mut cache = match identity_probe_cache().try_lock() {
        Ok(cache) => cache,
        Err(std::sync::TryLockError::Poisoned(error)) => error.into_inner(),
        Err(std::sync::TryLockError::WouldBlock) => return,
    };
    cache.insert(path.to_path_buf(), probe);
    while cache.len() > 16_384 {
        cache.pop_first();
    }
}

struct IdentityDeadlineReader<R> {
    inner: R,
    deadline: std::time::Instant,
}

impl<R: Read> Read for IdentityDeadlineReader<R> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if std::time::Instant::now() >= self.deadline {
            return Err(identity_inventory_incomplete("reader deadline reached"));
        }
        self.inner.read(output)
    }
}

fn identity_uuid(value: Option<&serde_json::Value>) -> Option<Uuid> {
    value
        .and_then(serde_json::Value::as_str)
        .and_then(|id| Uuid::parse_str(id).ok())
}

/// Stream past unrelated legacy histories once on a worker. A cached compact
/// probe remains usable only while the opened file and path retain their exact
/// device/inode/length/mtime/ctime. Target validation is never cached here.
fn read_session_identity_probe(
    path: &Path,
    budget: &mut IdentityScanBudget,
) -> io::Result<IdentityObservation> {
    budget.check()?;
    let path_metadata = std::fs::symlink_metadata(path)?;
    if !path_metadata.is_file() || path_metadata.file_type().is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "session identity metadata is not a regular file",
        ));
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK);
    }
    let mut file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || std::fs::symlink_metadata(path)?.file_type().is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "session identity metadata is not a regular file",
        ));
    }
    let stamp = IdentityFileStamp::for_metadata(&metadata);
    let stable = |file: &File| -> io::Result<()> {
        let current = std::fs::symlink_metadata(path)?;
        if !current.is_file()
            || current.file_type().is_symlink()
            || IdentityFileStamp::for_metadata(&current) != stamp
            || IdentityFileStamp::for_metadata(&file.metadata()?) != stamp
        {
            return Err(identity_inventory_incomplete(
                "file changed during observation",
            ));
        }
        budget.check()
    };
    if let Some(cached) = cached_identity_probe(path, &stamp) {
        stable(&file)?;
        return cached
            .result
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.message));
    }
    if metadata.len() > MAX_SESSION_HISTORY_BYTES + MAX_SESSION_METADATA_BYTES {
        return Err(identity_inventory_incomplete(
            "file exceeds the legacy probe bound",
        ));
    }
    budget.reserve_probe(metadata.len())?;
    let parsed: Result<SessionIdentityProbe, io::Error> =
        serde_json::from_reader(std::io::BufReader::new(IdentityDeadlineReader {
            inner: (&mut file).take(metadata.len()),
            deadline: budget.deadline,
        }))
        .map_err(|error| {
            io::Error::new(
                error.io_error_kind().unwrap_or(io::ErrorKind::InvalidData),
                error,
            )
        });
    if parsed
        .as_ref()
        .is_err_and(|error| error.kind() != io::ErrorKind::InvalidData)
    {
        return Err(parsed.err().expect("error checked"));
    }
    // Recheck both descriptors after the read, including malformed JSON. A
    // rename, same-length rewrite or restored mtime must invalidate its cache.
    let current = std::fs::symlink_metadata(path)?;
    if !current.is_file()
        || current.file_type().is_symlink()
        || IdentityFileStamp::for_metadata(&current) != stamp
        || IdentityFileStamp::for_metadata(&file.metadata()?) != stamp
    {
        return Err(identity_inventory_incomplete(
            "file changed during observation",
        ));
    }
    budget.check()?;
    let result = parsed
        .map(|probe| IdentityObservation {
            outer_id: identity_uuid(probe.id.as_ref()),
            inner_id: probe
                .session
                .as_ref()
                .and_then(|session| identity_uuid(session.get("id"))),
            version: probe.version.as_ref().and_then(serde_json::Value::as_u64),
        })
        .map_err(|error| IdentityProbeFailure {
            message: error.to_string().chars().take(256).collect(),
        });
    cache_identity_probe(
        path,
        CachedIdentityProbe {
            stamp,
            result: result.clone(),
        },
    );
    result.map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.message))
}

fn read_family_root(root: &Path, archived: bool) -> io::Result<Vec<SessionFamilyMetadata>> {
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut family = Vec::new();
    for entry in std::fs::read_dir(root)? {
        let Ok(entry) = entry else {
            continue;
        };
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let Some(key) = path.file_stem().and_then(|value| value.to_str()) else {
            continue;
        };
        if let Ok(metadata) = read_family_metadata(&path, key, archived) {
            family.push(metadata);
        }
    }
    family.sort_by_key(|node| (node.summary.id, node.summary.storage_key.clone()));
    Ok(family)
}

fn read_family_metadata(
    path: &Path,
    storage_key: &str,
    archived: bool,
) -> io::Result<SessionFamilyMetadata> {
    let session = if let Some(metadata) = read_bundle_metadata_if_present(path, storage_key)? {
        validate_bundle_files(path, &metadata)?;
        metadata_session(&metadata, Vec::new())?
    } else {
        read_session_file(path, storage_key)?
    };
    Ok(SessionFamilyMetadata {
        summary: SessionSummary {
            storage_key: storage_key.into(),
            id: session.id,
            title: session.title,
            pinned: session.pinned,
            workspace: session.workspace,
            updated_at: session.updated_at,
        },
        parent: session.parent,
        created_at: session.created_at,
        archived,
    })
}

fn read_bundle_metadata_if_present(
    path: &Path,
    storage_key: &str,
) -> io::Result<Option<SessionBundleMetadataV1>> {
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "session metadata path is not a regular file",
        ));
    }
    let maximum = MAX_SESSION_HISTORY_BYTES + MAX_SESSION_METADATA_BYTES;
    if metadata.len() > maximum {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "saved session exceeds the bounded legacy file limit",
        ));
    }
    let mut content = String::new();
    File::open(path)?
        .take(maximum + 1)
        .read_to_string(&mut content)?;
    if content.len() as u64 > maximum {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "saved session grew beyond the bounded legacy file limit",
        ));
    }
    let value: serde_json::Value = serde_json::from_str(&content).map_err(io::Error::other)?;
    if value.get("storage_version").is_none() {
        return Ok(None);
    }
    if metadata.len() > MAX_SESSION_METADATA_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "saved session metadata exceeds the 8 MiB limit",
        ));
    }
    let parsed: SessionBundleMetadataV1 =
        serde_json::from_value(value).map_err(io::Error::other)?;
    if parsed.storage_version != SESSION_BUNDLE_STORAGE_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "unsupported session storage version: {}",
                parsed.storage_version
            ),
        ));
    }
    if parsed.storage_key != storage_key {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "saved session storage identity changed",
        ));
    }
    validate_bundle_metadata(&parsed)?;
    // Identity probes do not cache target schema or sidecar validity. Every
    // exact-target reader/mutator revalidates those independently here.
    metadata_session(&parsed, Vec::new())?;
    validate_bundle_files(path, &parsed)?;
    Ok(Some(parsed))
}

fn validate_bundle_metadata(metadata: &SessionBundleMetadataV1) -> io::Result<()> {
    validate_storage_key(&metadata.storage_key)?;
    if metadata.message_count > MAX_SESSION_MESSAGES
        || metadata.offsets.len() != metadata.message_count
        || metadata.committed_bytes > MAX_SESSION_HISTORY_BYTES
        || metadata.history_sha256.len() != 64
        || !metadata
            .history_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "saved session history metadata exceeds its bounds",
        ));
    }
    if metadata.message_count == 0 {
        if metadata.committed_bytes != 0 || !metadata.offsets.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "empty session history metadata is inconsistent",
            ));
        }
    } else if metadata.offsets.first() != Some(&0)
        || metadata
            .offsets
            .windows(2)
            .any(|window| window[0] >= window[1])
        || metadata
            .offsets
            .last()
            .is_some_and(|offset| *offset >= metadata.committed_bytes)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "saved session history offsets are inconsistent",
        ));
    }
    let object = metadata.session.as_object().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "session metadata is not an object",
        )
    })?;
    if object.contains_key("messages") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "session metadata unexpectedly contains message bodies",
        ));
    }
    if object.get("id").and_then(serde_json::Value::as_str)
        != Some(metadata.id.to_string().as_str())
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "saved session UUID does not match its metadata",
        ));
    }
    Ok(())
}

fn bundle_log_name(metadata: &SessionBundleMetadataV1) -> String {
    format!(
        "{}.{}.turns.jsonl",
        metadata.storage_key, metadata.generation
    )
}

fn bundle_log_path(metadata_path: &Path, metadata: &SessionBundleMetadataV1) -> PathBuf {
    metadata_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(bundle_log_name(metadata))
}

fn validate_bundle_files(path: &Path, metadata: &SessionBundleMetadataV1) -> io::Result<()> {
    validate_bundle_metadata(metadata)?;
    let log_path = bundle_log_path(path, metadata);
    let log_metadata = std::fs::symlink_metadata(&log_path)?;
    if log_metadata.file_type().is_symlink() || !log_metadata.file_type().is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "session history path is not a regular file",
        ));
    }
    if log_metadata.len() < metadata.committed_bytes {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "session history is shorter than its committed metadata",
        ));
    }
    Ok(())
}

fn metadata_session(
    metadata: &SessionBundleMetadataV1,
    messages: Vec<Message>,
) -> io::Result<SessionV2> {
    let mut value = metadata.session.clone();
    value
        .as_object_mut()
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "session metadata is not an object",
            )
        })?
        .insert(
            "messages".into(),
            serde_json::to_value(messages).map_err(io::Error::other)?,
        );
    let mut session: SessionV2 = serde_json::from_value(value).map_err(io::Error::other)?;
    if session.id != metadata.id {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "saved session UUID does not match its metadata",
        ));
    }
    validate_import_provenance(&session)?;
    session.storage_binding = Some(binding_for_metadata(metadata)?);
    Ok(session)
}

fn binding_for_metadata(metadata: &SessionBundleMetadataV1) -> io::Result<SessionBinding> {
    Ok(SessionBinding {
        session_id: metadata.id,
        storage_generation: Some(metadata.generation),
        message_count: metadata.message_count,
        history_sha256: metadata.history_sha256.clone(),
        metadata_revision: metadata.metadata_revision,
        metadata_sha256: format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&metadata.session).map_err(io::Error::other)?)
        ),
    })
}

fn read_committed_binding(
    path: &Path,
    storage_key: &str,
    expected_uuid: Uuid,
) -> io::Result<SessionBinding> {
    if let Some(metadata) = read_bundle_metadata_if_present(path, storage_key)? {
        verify_bundle_id(&metadata, expected_uuid)?;
        validate_bundle_files(path, &metadata)?;
        // Validate the inner UUID and optional metadata without reading history.
        metadata_session(&metadata, Vec::new())?;
        binding_for_metadata(&metadata)
    } else {
        let session = read_session_file(path, storage_key)?;
        verify_session_id(&session, expected_uuid)?;
        binding_for_legacy(&session)
    }
}

fn binding_for_legacy(session: &SessionV2) -> io::Result<SessionBinding> {
    // Legacy reads remain available even when a message is too large for a
    // successful bundle migration. Save still enforces the record limit.
    let mut history = Sha256::new();
    for (index, message) in session.messages.iter().enumerate() {
        history.update(serialize_log_record_inner(index, message, false)?);
    }
    Ok(SessionBinding {
        session_id: session.id,
        storage_generation: None,
        message_count: session.messages.len(),
        history_sha256: format!("{:x}", history.finalize()),
        metadata_revision: 0,
        metadata_sha256: format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(&session_metadata_value(session)?).map_err(io::Error::other)?
            )
        ),
    })
}

fn next_metadata_revision(revision: u64) -> io::Result<u64> {
    revision.checked_add(1).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "session metadata revision exhausted",
        )
    })
}

fn validate_import_provenance(session: &SessionV2) -> io::Result<()> {
    if let Some(workflow) = &session.work_state.plan_workflow {
        workflow
            .validate()
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    }
    session
        .bookmarks
        .validate()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    session
        .branch_handoffs
        .validate(session.id)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    session
        .work_state
        .plan_feedback
        .validate()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let Some(provenance) = &session.import_provenance else {
        return Ok(());
    };
    let valid_label = !provenance.source_label.is_empty()
        && provenance.source_label.len() <= 128
        && !provenance.source_label.contains(['/', '\\'])
        && !provenance.source_label.chars().any(char::is_control);
    let valid_digest = provenance.source_session_digest.len() == 64
        && provenance
            .source_session_digest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase());
    if !valid_label
        || !valid_digest
        || provenance.message_count > MAX_SESSION_MESSAGES
        || provenance.skipped_items > MAX_SESSION_MESSAGES
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "session import provenance exceeds its bounds",
        ));
    }
    Ok(())
}

fn session_metadata_value(session: &SessionV2) -> io::Result<serde_json::Value> {
    let mut value = serde_json::to_value(session).map_err(io::Error::other)?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "session is not an object"))?;
    object.remove("messages");
    Ok(value)
}

fn serialize_log_record(index: usize, message: &Message) -> io::Result<Vec<u8>> {
    serialize_log_record_inner(index, message, true)
}

fn serialize_log_record_inner(
    index: usize,
    message: &Message,
    enforce_limit: bool,
) -> io::Result<Vec<u8>> {
    let mut sanitized = message.clone();
    if let Some(calls) = sanitized.tool_calls.as_mut() {
        for call in calls {
            if crate::email::is_email_tool(&call.name) {
                call.arguments = crate::email::private_arguments_marker(&call.name);
                call.output = call
                    .output
                    .as_ref()
                    .map(|_| crate::email::private_result_marker(&call.name));
            }
        }
    }
    let mut bytes = serde_json::to_vec(&SessionLogRecord {
        index,
        message: &sanitized,
    })
    .map_err(io::Error::other)?;
    if enforce_limit && bytes.len() > MAX_SESSION_MESSAGE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "one saved session message exceeds the 1 MiB record limit",
        ));
    }
    bytes.push(b'\n');
    Ok(bytes)
}

fn build_log_bytes(messages: &[Message]) -> io::Result<(Vec<u8>, Vec<u64>, String)> {
    if messages.len() > MAX_SESSION_MESSAGES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "saved session has too many messages",
        ));
    }
    let mut bytes = Vec::new();
    let mut offsets = Vec::with_capacity(messages.len());
    let mut hasher = Sha256::new();
    for (index, message) in messages.iter().enumerate() {
        offsets.push(bytes.len() as u64);
        let record = serialize_log_record(index, message)?;
        hasher.update(&record);
        bytes.extend_from_slice(&record);
        if bytes.len() as u64 > MAX_SESSION_HISTORY_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "saved session history exceeds the 64 MiB limit",
            ));
        }
    }
    Ok((bytes, offsets, format!("{:x}", hasher.finalize())))
}

fn hash_log_records(messages: &[Message]) -> io::Result<String> {
    let mut hasher = Sha256::new();
    for (index, message) in messages.iter().enumerate() {
        hasher.update(serialize_log_record(index, message)?);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

pub fn canonical_prefix_sha256(messages: &[Message], count: usize) -> io::Result<String> {
    let prefix = messages.get(..count).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "history anchor exceeds its canonical prefix",
        )
    })?;
    hash_log_records(prefix)
}

fn validate_page_size(page_size: usize) -> io::Result<()> {
    if page_size == 0 || page_size > SESSION_HISTORY_PAGE_SIZE {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("history page size must be 1-{SESSION_HISTORY_PAGE_SIZE}"),
        ))
    } else {
        Ok(())
    }
}

fn read_bundle_message_range(
    metadata_path: &Path,
    metadata: &SessionBundleMetadataV1,
    start: usize,
    end: usize,
) -> io::Result<Vec<Message>> {
    if start > end || end > metadata.message_count {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "saved session history range is invalid",
        ));
    }
    if start == end {
        return Ok(Vec::new());
    }
    let log_path = bundle_log_path(metadata_path, metadata);
    let mut file = File::open(log_path)?;
    let start_offset = metadata.offsets[start];
    let end_offset = if end == metadata.message_count {
        metadata.committed_bytes
    } else {
        metadata.offsets[end]
    };
    let byte_count = end_offset.checked_sub(start_offset).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "session history offsets underflow",
        )
    })?;
    if byte_count > MAX_SESSION_HISTORY_BYTES || byte_count > usize::MAX as u64 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "saved session history page exceeds its byte bound",
        ));
    }
    file.seek(SeekFrom::Start(start_offset))?;
    let mut bytes = vec![0; byte_count as usize];
    file.read_exact(&mut bytes)?;
    if start == 0
        && end == metadata.message_count
        && format!("{:x}", Sha256::digest(&bytes)) != metadata.history_sha256
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "saved session history checksum does not match its metadata",
        ));
    }
    if bytes.last() != Some(&b'\n') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "saved session history record is incomplete",
        ));
    }
    let mut messages = Vec::with_capacity(end - start);
    for (expected, line) in (start..end).zip(bytes.split(|byte| *byte == b'\n')) {
        if line.is_empty() {
            continue;
        }
        if line.len() > MAX_SESSION_MESSAGE_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "saved session message exceeds its record bound",
            ));
        }
        let record: OwnedSessionLogRecord =
            serde_json::from_slice(line).map_err(io::Error::other)?;
        if record.index != expected {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "saved session message index is out of order",
            ));
        }
        messages.push(record.message);
    }
    if messages.len() != end - start {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "saved session history has missing records",
        ));
    }
    Ok(messages)
}

fn verify_bundle_id(metadata: &SessionBundleMetadataV1, expected_uuid: Uuid) -> io::Result<()> {
    if metadata.id == expected_uuid {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "saved session changed; refresh and try again",
        ))
    }
}

fn write_bundle_metadata(path: &Path, metadata: &SessionBundleMetadataV1) -> io::Result<()> {
    let mut bytes = serde_json::to_vec_pretty(metadata).map_err(io::Error::other)?;
    bytes.push(b'\n');
    if bytes.len() as u64 > MAX_SESSION_METADATA_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "saved session metadata exceeds the 8 MiB limit",
        ));
    }
    atomic_write_with(path, |file| file.write_all(&bytes))
}

fn write_new_log(path: &Path, bytes: &[u8]) -> io::Result<()> {
    atomic_write_with(path, |file| file.write_all(bytes))
}

fn save_session_bundle(path: &Path, session: &SessionV2) -> io::Result<()> {
    let storage_key = path
        .file_stem()
        .and_then(|value| value.to_str())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid session filename"))?;
    validate_storage_key(storage_key)?;
    let existing = if path.exists() {
        read_bundle_metadata_if_present(path, storage_key)?
    } else {
        None
    };
    if let Some(mut metadata) = existing {
        verify_bundle_id(&metadata, session.id)?;
        validate_bundle_files(path, &metadata)?;
        let physical_len = std::fs::metadata(bundle_log_path(path, &metadata))?.len();
        let exact_prefix = session.messages.len() >= metadata.message_count
            && hash_log_records(&session.messages[..metadata.message_count])?
                == metadata.history_sha256;
        if exact_prefix && physical_len == metadata.committed_bytes {
            let mut appended = Vec::new();
            let mut next_offsets = metadata.offsets.clone();
            let mut cursor = metadata.committed_bytes;
            for (index, message) in session
                .messages
                .iter()
                .enumerate()
                .skip(metadata.message_count)
            {
                next_offsets.push(cursor);
                let record = serialize_log_record(index, message)?;
                cursor = cursor.saturating_add(record.len() as u64);
                if cursor > MAX_SESSION_HISTORY_BYTES {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "saved session history exceeds the 64 MiB limit",
                    ));
                }
                appended.extend_from_slice(&record);
            }
            if !appended.is_empty() {
                let mut log = OpenOptions::new()
                    .append(true)
                    .open(bundle_log_path(path, &metadata))?;
                log.write_all(&appended)?;
                log.sync_all()?;
            }
            metadata.message_count = session.messages.len();
            metadata.committed_bytes = cursor;
            metadata.offsets = next_offsets;
            metadata.history_sha256 = hash_log_records(&session.messages)?;
            metadata.session = session_metadata_value(session)?;
            metadata.metadata_revision = next_metadata_revision(metadata.metadata_revision)?;
            validate_bundle_metadata(&metadata)?;
            return write_bundle_metadata(path, &metadata);
        }
        return rotate_session_bundle(path, session, Some(metadata));
    }
    rotate_session_bundle(path, session, None)
}

fn rotate_session_bundle(
    path: &Path,
    session: &SessionV2,
    previous: Option<SessionBundleMetadataV1>,
) -> io::Result<()> {
    let storage_key = path
        .file_stem()
        .and_then(|value| value.to_str())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid session filename"))?;
    let (bytes, offsets, history_sha256) = build_log_bytes(&session.messages)?;
    let metadata = SessionBundleMetadataV1 {
        storage_version: SESSION_BUNDLE_STORAGE_VERSION,
        metadata_revision: next_metadata_revision(
            previous
                .as_ref()
                .map_or(0, |metadata| metadata.metadata_revision),
        )?,
        storage_key: storage_key.into(),
        id: session.id,
        generation: Uuid::new_v4(),
        message_count: session.messages.len(),
        committed_bytes: bytes.len() as u64,
        offsets,
        history_sha256,
        session: session_metadata_value(session)?,
    };
    validate_bundle_metadata(&metadata)?;
    let log_path = bundle_log_path(path, &metadata);
    write_new_log(&log_path, &bytes)?;
    if let Err(error) = write_bundle_metadata(path, &metadata) {
        let _ = std::fs::remove_file(&log_path);
        return Err(error);
    }
    if let Some(previous) = previous {
        let old_log = bundle_log_path(path, &previous);
        if old_log != log_path {
            let _ = std::fs::remove_file(old_log);
        }
    }
    Ok(())
}

fn read_session_file(path: &Path, storage_key: &str) -> io::Result<SessionV2> {
    if let Some(metadata) = read_bundle_metadata_if_present(path, storage_key)? {
        validate_bundle_files(path, &metadata)?;
        let messages = read_bundle_message_range(path, &metadata, 0, metadata.message_count)?;
        let mut session = metadata_session(&metadata, messages)?;
        session
            .derive_or_validate_context_checkpoint()
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        return Ok(session);
    }
    let content = std::fs::read_to_string(path)?;
    let value: serde_json::Value = serde_json::from_str(&content).map_err(io::Error::other)?;
    match value.get("version").and_then(serde_json::Value::as_u64) {
        Some(2..=4) => {
            let mut session: SessionV2 = serde_json::from_value(value).map_err(io::Error::other)?;
            validate_import_provenance(&session)?;
            session
                .derive_or_validate_context_checkpoint()
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            session.storage_binding = Some(binding_for_legacy(&session)?);
            Ok(session)
        }
        Some(1) | None => {
            let legacy: SessionV1 = serde_json::from_value(value).map_err(io::Error::other)?;
            let mut session = convert_v1(legacy, storage_key);
            session.storage_binding = Some(binding_for_legacy(&session)?);
            Ok(session)
        }
        Some(version) => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unsupported session version: {version}"),
        )),
    }
}

fn convert_v1(legacy: SessionV1, storage_key: &str) -> SessionV2 {
    let created_at = DateTime::parse_from_rfc3339(&legacy.created)
        .map(|value| value.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());
    let id = Uuid::new_v5(&Uuid::NAMESPACE_URL, storage_key.as_bytes());
    SessionV2 {
        storage_binding: None,
        version: 4,
        id,
        title: legacy.name,
        pinned: false,
        workspace: String::new(),
        workspace_roots: Vec::new(),
        manual_rule_ids: Vec::new(),
        active_skills: Vec::new(),
        persistent_skill_refs: Vec::new(),
        work_state: crate::work::WorkState::default(),
        review_records: Vec::new(),
        agent_summaries: Vec::new(),
        transcript_annotations: Vec::new(),
        transcript_agent_summaries: Vec::new(),
        created_at,
        updated_at: created_at,
        provider: legacy.provider,
        model: legacy.model,
        reasoning_effort: None,
        automation_provenance: None,
        automation_deliveries: Vec::new(),
        parent: None,
        bookmarks: crate::branches::SessionBookmarks::default(),
        branch_handoffs: crate::branches::BranchHandoffLedger::default(),
        messages: legacy
            .messages
            .into_iter()
            .map(|message| Message {
                role: match message.role.as_str() {
                    "system" => Role::System,
                    "assistant" => Role::Assistant,
                    "tool" => Role::Tool,
                    _ => Role::User,
                },
                content: message.content,
                tool_calls: message.tool_calls.map(|calls| {
                    calls
                        .into_iter()
                        .map(|call| ToolCall {
                            id: call.id,
                            name: call.name,
                            arguments: call.arguments,
                            status: if call.output.is_some() {
                                ToolStatus::Done
                            } else {
                                ToolStatus::Pending
                            },
                            output: call.output,
                        })
                        .collect()
                }),
                thinking: None,
                timestamp: DateTime::parse_from_rfc3339(&message.timestamp)
                    .map(|value| value.with_timezone(&Utc))
                    .unwrap_or(created_at),
                response_items: Vec::new(),
            })
            .collect(),
        context_checkpoint: None,
        queued_inputs: Vec::new(),
        service_inbox: None,
        fallback_provenance: Vec::new(),
        import_provenance: None,
        goal_state: None,
        workflow_run: None,
        attached_worktree: None,
        mechanistic_checkpoint: None,
        mechanistic_capsules: MechanisticCapsuleIndexV1::default(),
        artifacts: crate::session_artifacts::SessionArtifactLedger::default(),
        context_usage_anchor: None,
    }
}

fn atomic_write_with<F>(target: &Path, writer: F) -> io::Result<()>
where
    F: FnOnce(&mut File) -> io::Result<()>,
{
    let parent = target
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "session path has no parent"))?;
    std::fs::create_dir_all(parent)?;
    let file_name = target
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("session.json");
    let temporary = parent.join(format!(".{file_name}.{}.tmp", Uuid::new_v4()));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)?;
    if let Err(error) = writer(&mut file).and_then(|_| file.sync_all()) {
        let _ = std::fs::remove_file(&temporary);
        return Err(error);
    }
    drop(file);
    if let Err(error) = std::fs::rename(&temporary, target) {
        let _ = std::fs::remove_file(&temporary);
        return Err(error);
    }
    #[cfg(unix)]
    File::open(parent)?.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::provider::ProviderProtocol;
    use crate::agent::stream::{ProviderResponseItem, ProviderStateSourceV1};
    use tempfile::TempDir;

    fn message(role: Role, content: &str, second: i64) -> Message {
        Message {
            role,
            content: content.into(),
            tool_calls: None,
            thinking: None,
            timestamp: DateTime::from_timestamp(second, 0).unwrap(),
            response_items: Vec::new(),
        }
    }

    fn provider_state(content: &str) -> ProviderResponseItem {
        ProviderResponseItem::ChatReasoning {
            source: ProviderStateSourceV1::for_test("deepseek", ProviderProtocol::ChatCompletions),
            content: content.into(),
        }
    }

    fn responses_provider_state(content: &str) -> ProviderResponseItem {
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
    fn public_session_ids_are_fixed_width_reversible_and_strict() {
        for id in [
            Uuid::nil(),
            Uuid::from_u128(1),
            Uuid::from_u128(0xfeed_beef_dead_cafe_0123_4567_89ab_cdef),
            Uuid::from_u128(u128::MAX),
        ] {
            let public = public_session_id(id);
            assert_eq!(public.len(), 26);
            assert!(public.starts_with("ses_"));
            assert_eq!(parse_public_session_id(&public), Some(id));
        }

        for invalid in [
            "",
            "ses_",
            "session_0000000000000000000000",
            "ses_000000000000000000000!",
            "ses_zzzzzzzzzzzzzzzzzzzzzz",
        ] {
            assert_eq!(parse_public_session_id(invalid), None, "{invalid}");
        }

        let id = Uuid::from_u128(1);
        let summary = SessionSummary {
            storage_key: id.to_string(),
            id,
            title: "Example".into(),
            pinned: false,
            workspace: "/workspace".into(),
            updated_at: Utc::now(),
        };
        assert!(summary.matches(&public_session_id(id).to_ascii_lowercase()));
    }

    #[test]
    fn automatic_title_uses_the_first_user_request_without_attachment_bodies() {
        let messages = vec![
            message(Role::System, "ignore this", 0),
            message(
                Role::User,
                "  ## Check   rsync on BSD\nvia SSH  \n\n--- Attached file: notes.txt ---\nsecret body\n--- End file ---",
                1,
            ),
            message(Role::User, "Later request", 2),
        ];
        assert_eq!(
            automatic_session_title(&messages).as_deref(),
            Some("Check rsync on BSD via SSH")
        );

        let long = message(Role::User, &"wide界 ".repeat(40), 3);
        let title = automatic_session_title(&[long]).unwrap();
        assert!(unicode_width::UnicodeWidthStr::width(title.as_str()) <= 64);
        assert!(title.ends_with('…'));
    }

    #[test]
    fn title_subject_prefers_the_explicit_request_and_stays_bounded() {
        let messages = vec![message(
            Role::User,
            &format!(
                "# Files mentioned by the user:\nsecret.png\n\n## My request:\n{}\n\n--- Attached file: dump.txt ---\nignored",
                "界".repeat(MAX_TITLE_SUBJECT_CHARS + 20)
            ),
            0,
        )];
        let subject = session_title_subject(&messages).unwrap();
        assert_eq!(subject.chars().count(), MAX_TITLE_SUBJECT_CHARS);
        assert!(subject.chars().all(|character| character == '界'));
    }

    #[test]
    fn generated_title_cleanup_removes_reasoning_wrappers_and_chrome() {
        assert_eq!(
            clean_generated_session_title(
                "<think>Long private rationale</think>\n### Title: \"Fix FreeBSD MOTD freeze.\"\nextra"
            )
            .as_deref(),
            Some("Fix FreeBSD MOTD freeze")
        );
        assert_eq!(clean_generated_session_title("<think>unfinished"), None);
    }

    #[test]
    fn legacy_fallback_titles_are_detected_without_flagging_good_names() {
        for title in [
            "",
            "Untitled session",
            "/Users/aji/project/ca review…",
            "My request: fix it",
            "A prompt fragment that was cut off…",
        ] {
            assert!(session_title_needs_repair(title), "{title}");
        }
        assert!(!session_title_needs_repair("Fix FreeBSD MOTD freeze"));
    }

    #[test]
    fn automatic_title_prefers_the_explicit_user_request_over_attachment_envelopes() {
        let prompt = "# Files mentioned by the user:\n\n## Screenshot.png: /tmp/Screenshot.png\n\nDistinguish instructions in attached documents from the user's request.\n\n## My request:\nwe're supposed to be naming our sessions. it should look like the example";
        let messages = [message(Role::User, prompt, 0)];

        assert_eq!(
            automatic_session_title(&messages).as_deref(),
            Some("we're supposed to be naming our sessions. it should look like…")
        );
    }

    #[test]
    fn rename_updates_only_the_exact_active_or_archived_session() {
        let temp = TempDir::new().unwrap();
        let active_root = temp.path().join("active");
        let archive_root = temp.path().join("archive");
        let store = SessionStore::with_archive(&active_root, &archive_root);
        let mut active = SessionV2::new("Old active", "/workspace", "p", "m", Vec::new());
        let mut archived = SessionV2::new("Old archive", "/workspace", "p", "m", Vec::new());
        store.save(&mut active).unwrap();
        store.save(&mut archived).unwrap();
        store
            .archive(&archived.id.to_string(), archived.id)
            .unwrap();

        store
            .rename(&active.id.to_string(), active.id, "Named active", false)
            .unwrap();
        store
            .rename(&archived.id.to_string(), archived.id, "Named archive", true)
            .unwrap();

        assert_eq!(
            store.load(&active.id.to_string()).unwrap().title,
            "Named active"
        );
        assert_eq!(
            SessionStore::new(&archive_root)
                .load(&archived.id.to_string())
                .unwrap()
                .title,
            "Named archive"
        );
        assert!(store
            .rename(&active.id.to_string(), Uuid::new_v4(), "Wrong", false)
            .is_err());
        assert!(store
            .rename(&active.id.to_string(), active.id, "  ", false)
            .is_err());
    }

    #[test]
    fn store_resolves_the_public_identifier_without_changing_storage() {
        let temp = TempDir::new().unwrap();
        let store = SessionStore::new(temp.path());
        let mut session = SessionV2::new("Named", "/workspace", "local", "model", Vec::new());
        session.id = Uuid::from_u128(0xfeed_beef_dead_cafe_0123_4567_89ab_cdef);
        let path = store.save(&mut session).unwrap();
        let public = public_session_id(session.id);

        assert_eq!(store.load(&public).unwrap().id, session.id);
        assert_eq!(
            store
                .load_recent(&public, SESSION_HISTORY_PAGE_SIZE)
                .unwrap()
                .session
                .id,
            session.id
        );
        let expected_storage_key = session.id.to_string();
        assert_eq!(
            path.file_stem().and_then(|stem| stem.to_str()),
            Some(expected_storage_key.as_str())
        );
    }

    #[test]
    fn store_resume_roundtrip_preserves_conversational_memory() {
        let temp = TempDir::new().unwrap();
        let store = SessionStore::new(temp.path());
        let token = "PERSIST_PROBE_CA_249";
        let mut session = SessionV2::new(
            "Resume memory",
            "/workspace",
            "deepseek",
            "deepseek-v4-pro",
            vec![
                message(Role::User, &format!("Remember the word {token}."), 1),
                message(Role::Assistant, &format!("Stored `{token}`."), 2),
            ],
        );
        store.save(&mut session).unwrap();
        let public = public_session_id(session.id);

        let loaded = store.load(&public).expect("resume by public session id");
        assert_eq!(loaded.id, session.id);
        let joined = loaded
            .messages
            .iter()
            .map(|m| m.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            joined.contains(token),
            "resume must retain last-turn memory; got {joined:?}"
        );

        // A distinct unsaved identity must not leak into this store.
        let isolated = SessionV2::new(
            "No session",
            "/workspace",
            "deepseek",
            "deepseek-v4-pro",
            vec![message(Role::User, "should-not-persist", 1)],
        );
        assert!(store.load(&public_session_id(isolated.id)).is_err());
        assert_eq!(store.list().unwrap().len(), 1);
    }

    #[test]
    fn direct_session_record_scrubs_external_private_email() {
        let sentinel = "direct-session-private-email-sentinel";
        let mut assistant = message(Role::Assistant, "", 1);
        assistant.tool_calls = Some(vec![ToolCall {
            id: "email-call".into(),
            name: "email_read".into(),
            arguments: serde_json::json!({"body": sentinel}).to_string(),
            status: crate::app::ToolStatus::Done,
            output: Some(sentinel.into()),
        }]);
        let record = String::from_utf8(serialize_log_record(0, &assistant).unwrap()).unwrap();
        assert!(!record.contains(sentinel));
        assert!(record.contains("external_private"));
    }

    #[test]
    fn stale_save_and_metadata_only_changes_cannot_overwrite_current_commit() {
        let dir = tempfile::tempdir().unwrap();
        let store = SessionStore::new(dir.path());
        let mut current = test_session();
        store.save(&mut current).unwrap();
        let mut stale = current.clone();
        current.title = "new title".into();
        store.save(&mut current).unwrap();
        stale.title = "stale title".into();
        assert!(store.save(&mut stale).is_err());
        assert_eq!(
            store.load(&current.id.to_string()).unwrap().title,
            "new title"
        );
        store
            .set_pinned(&current.id.to_string(), current.id, true)
            .unwrap();
        assert!(store.save(&mut current).is_err());
        let mut fresh = store.load(&current.id.to_string()).unwrap();
        assert!(fresh.pinned);
        store.save(&mut fresh).unwrap();
    }

    #[test]
    fn owned_save_reuses_exact_lease_and_wrong_lease_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let store = SessionStore::new(dir.path());
        let mut current = test_session();
        let lease = store.try_writer_lease(current.id).unwrap();
        store.save_owned(&mut current, &lease).unwrap();
        assert_eq!(
            store.save(&mut current).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        store
            .set_pinned_owned(&current.id.to_string(), current.id, true, &lease)
            .unwrap();
        let wrong = store.try_writer_lease(Uuid::new_v4()).unwrap();
        assert!(store.save_owned(&mut current, &wrong).is_err());
        assert_eq!(
            store
                .delete(&current.id.to_string(), current.id)
                .unwrap_err()
                .kind(),
            io::ErrorKind::WouldBlock
        );
    }

    fn assert_ambiguous_identity_is_read_only(
        store: &SessionStore,
        session: &mut SessionV2,
        key: &str,
        binding: &SessionAutomationBinding,
    ) {
        assert_eq!(
            store.committed_binding(session.id).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(
            store.automation_snapshot(session.id).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        let lease = store.try_writer_lease(session.id).unwrap();
        assert_eq!(
            store.load_owned(key, &lease).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(
            store.save_owned(session, &lease).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(
            store
                .set_pinned_owned(key, session.id, true, &lease)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(
            store
                .rename_owned(key, session.id, "must not land", false, &lease)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(
            store
                .append_automation_delivery_owned(
                    binding,
                    AutomationSessionProvenanceV1 {
                        schema: "ca.automation-delivery.v1".into(),
                        automation_id: "fixture".into(),
                        run_id: Uuid::new_v4(),
                        occurrence_id: "fixture".into(),
                        kind: "heartbeat".into(),
                    },
                    "fixture",
                    "must not land",
                    "must not land",
                    &lease
                )
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        drop(lease);
        assert!(store.archive(key, session.id).is_err());
        assert!(store.restore(key, session.id).is_err());
        assert_eq!(
            store.delete(key, session.id).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        let failure = store
            .delete_retention_batch(&[retention_candidate(key.into(), session)], None)
            .unwrap_err();
        assert_eq!(failure.removed, 0);
        session.import_provenance = Some(ImportProvenance {
            source: ForeignTranscriptSource::Codex,
            source_label: "fixture".into(),
            source_session_digest: "a".repeat(64),
            message_count: session.messages.len(),
            skipped_items: 0,
            learning_eligible: false,
            imported_at: Utc::now(),
        });
        assert!(store.save_inert_import(session).is_err());
        let family = store.family_metadata().unwrap();
        assert!(crate::branches::family_page(&family, session.id, 0, 100)
            .issues
            .contains(&crate::branches::FamilyIssue::DuplicateIdentity(session.id)));
    }

    #[test]
    fn interrupted_archive_hard_links_refuse_every_identity_write_without_changing_either_copy() {
        let dir = tempfile::tempdir().unwrap();
        let active = dir.path().join("sessions");
        let archive = dir.path().join("archive");
        let store = SessionStore::with_archive(&active, &archive);
        let mut session = test_session();
        let path = store.save(&mut session).unwrap();
        let key = path.file_stem().unwrap().to_str().unwrap().to_owned();
        let (_, binding) = store.automation_snapshot(session.id).unwrap();
        let metadata = read_bundle_metadata_if_present(&path, &key)
            .unwrap()
            .unwrap();
        let log = bundle_log_path(&path, &metadata);
        let before_metadata = std::fs::read(&path).unwrap();
        let before_history = std::fs::read(&log).unwrap();
        std::fs::create_dir_all(&archive).unwrap();
        let duplicate = archive.join(path.file_name().unwrap());
        let duplicate_log = archive.join(log.file_name().unwrap());
        std::fs::hard_link(&path, &duplicate).unwrap();
        std::fs::hard_link(&log, &duplicate_log).unwrap();
        assert_ambiguous_identity_is_read_only(&store, &mut session, &key, &binding);
        for copy in [&path, &duplicate] {
            assert_eq!(std::fs::read(copy).unwrap(), before_metadata);
        }
        for copy in [&log, &duplicate_log] {
            assert_eq!(std::fs::read(copy).unwrap(), before_history);
        }
    }

    #[test]
    fn identical_legacy_identity_in_different_storage_keys_refuses_writes() {
        let dir = tempfile::tempdir().unwrap();
        let active = dir.path().join("sessions");
        let store = SessionStore::with_archive(&active, dir.path().join("archive"));
        std::fs::create_dir_all(&active).unwrap();
        let session = test_session();
        let bytes = serde_json::to_vec(&session).unwrap();
        let first = active.join("legacy-first.json");
        let second = active.join("legacy-second.json");
        std::fs::write(&first, &bytes).unwrap();
        let mut loaded = store.load("legacy-first").unwrap();
        let (_, binding) = store.automation_snapshot(session.id).unwrap();
        std::fs::write(&second, &bytes).unwrap();
        assert_ambiguous_identity_is_read_only(&store, &mut loaded, "legacy-first", &binding);
        assert_eq!(std::fs::read(first).unwrap(), bytes);
        assert_eq!(std::fs::read(second).unwrap(), bytes);
    }

    #[test]
    fn malformed_duplicate_inner_identity_cannot_hide_behind_another_storage_key() {
        let dir = tempfile::tempdir().unwrap();
        let store = SessionStore::new(dir.path());
        let mut session = test_session();
        let path = store.save(&mut session).unwrap();
        let before = std::fs::read(&path).unwrap();
        let duplicate = dir.path().join("malformed-duplicate.json");
        let mut value: serde_json::Value = serde_json::from_slice(&before).unwrap();
        value["storage_key"] = "malformed-duplicate".into();
        value["id"] = "invalid UUID".into();
        std::fs::write(&duplicate, serde_json::to_vec(&value).unwrap()).unwrap();
        assert_eq!(
            store.save(&mut session).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(std::fs::read(path).unwrap(), before);
        assert!(duplicate.is_file());
    }

    #[test]
    fn canonical_root_alias_is_one_location_and_archived_identity_cannot_be_recreated() {
        let dir = tempfile::tempdir().unwrap();
        let store = SessionStore::with_archive(dir.path(), dir.path().join("."));
        let mut session = test_session();
        store.save(&mut session).unwrap();
        store.save(&mut session).unwrap();
        let active = dir.path().join("sessions");
        let archive = dir.path().join("archive");
        let store = SessionStore::with_archive(&active, &archive);
        let mut archived = test_session();
        let path = store.save(&mut archived).unwrap();
        let key = path.file_stem().unwrap().to_str().unwrap();
        store.archive(key, archived.id).unwrap();
        let before = std::fs::read(archive.join(format!("{key}.json"))).unwrap();
        archived.storage_binding = None;
        assert_eq!(
            store.save(&mut archived).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(
            std::fs::read(archive.join(format!("{key}.json"))).unwrap(),
            before
        );
        assert!(!path.exists());
    }

    #[test]
    fn legacy_bundle_revision_defaults_and_next_commit_advances() {
        let dir = tempfile::tempdir().unwrap();
        let store = SessionStore::new(dir.path());
        let mut current = test_session();
        let path = store.save(&mut current).unwrap();
        let mut metadata: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        metadata
            .as_object_mut()
            .unwrap()
            .remove("metadata_revision");
        std::fs::write(&path, serde_json::to_vec(&metadata).unwrap()).unwrap();
        let mut loaded = store.load(&current.id.to_string()).unwrap();
        assert_eq!(
            loaded.storage_binding.as_ref().unwrap().metadata_revision,
            0
        );
        store.save(&mut loaded).unwrap();
        assert_eq!(
            loaded.storage_binding.as_ref().unwrap().metadata_revision,
            1
        );
        assert!(serde_json::to_value(&loaded)
            .unwrap()
            .get("storage_binding")
            .is_none());
    }

    #[test]
    fn family_metadata_reads_archived_parent_and_bookmarks_without_history() {
        let root = tempfile::tempdir().unwrap();
        let store =
            SessionStore::with_archive(root.path().join("sessions"), root.path().join("archive"));
        let mut parent = test_session();
        parent
            .bookmarks
            .add(&parent.messages, 2, "first answer")
            .unwrap();
        store.save(&mut parent).unwrap();
        let mut branch = branch_in_memory(&parent, 2, 1, "branch").unwrap();
        let path = store.save(&mut branch).unwrap();
        store.archive(&parent.id.to_string(), parent.id).unwrap();
        let metadata = read_bundle_metadata_if_present(&path, &branch.id.to_string())
            .unwrap()
            .unwrap();
        // Corrupt history at the same byte length: family navigation remains a
        // cheap metadata read while an actual history load detects corruption.
        let log = bundle_log_path(&path, &metadata);
        let bytes = std::fs::read(&log).unwrap();
        std::fs::write(&log, vec![b'x'; bytes.len()]).unwrap();
        let family = store.family_metadata().unwrap();
        assert_eq!(family.len(), 2);
        assert!(
            family
                .iter()
                .find(|n| n.summary.id == parent.id)
                .unwrap()
                .archived
        );
        assert_eq!(
            family
                .iter()
                .find(|n| n.summary.id == branch.id)
                .unwrap()
                .parent
                .as_ref()
                .unwrap()
                .session_id,
            parent.id
        );
        assert!(store.load(&branch.id.to_string()).is_err());
    }

    #[test]
    fn branch_bookmarks_round_trip_and_submitted_feedback_never_becomes_queued_work() {
        let root = tempfile::tempdir().unwrap();
        let store = SessionStore::new(root.path());
        let mut source = test_session();
        source
            .bookmarks
            .add(&source.messages, 2, "first answer")
            .unwrap();
        let submission_id = Uuid::new_v4();
        source
            .work_state
            .plan_feedback
            .notes
            .push(crate::plan_feedback::PlanFeedbackNote {
                id: Uuid::new_v4(),
                plan_id: crate::work::PlanId::new(),
                reviewed_revision: 1,
                content_sha256: "a".repeat(64),
                anchor: crate::plan_feedback::FeedbackAnchor::Text {
                    start: 0,
                    end: 1,
                    quote: "a".into(),
                },
                comment: "revise this".into(),
                status: crate::plan_feedback::FeedbackStatus::Submitted,
                submitted_revision: None,
                submission_id: Some(submission_id),
            });
        source.queued_inputs.push(crate::queue::PendingInput {
            id: submission_id,
            text: "submitted feedback".into(),
            created_at: Utc::now(),
        });
        store.save(&mut source).unwrap();
        let loaded = store.load(&source.id.to_string()).unwrap();
        assert_eq!(loaded.bookmarks, source.bookmarks);
        let branch = branch_in_memory(&loaded, 2, 1, "full").unwrap();
        assert!(branch.queued_inputs.is_empty());
        assert_eq!(
            branch.work_state.plan_feedback.notes[0].status,
            crate::plan_feedback::FeedbackStatus::Historical
        );
        assert!(branch.bookmarks.entries[0].matches_history(&branch.messages));
        let mut invalid = loaded.clone();
        invalid.work_state.plan_feedback.notes[0].comment = "x".repeat(4097);
        assert!(store
            .save(&mut invalid)
            .unwrap_err()
            .to_string()
            .contains("Plan comments"));
        let mut invalid = loaded;
        invalid.bookmarks.entries[0].label = "x".repeat(257);
        assert!(store
            .save(&mut invalid)
            .unwrap_err()
            .to_string()
            .contains("Bookmarks"));
    }

    fn test_session() -> SessionV2 {
        SessionV2::new(
            "Demo",
            "/workspace/ca",
            "local",
            "coder",
            vec![
                message(Role::User, "hello", 1),
                message(Role::Assistant, "hi", 2),
            ],
        )
    }

    #[test]
    fn pre_change_message_records_default_state_and_remain_appendable_prefixes() {
        let old_record = r#"{"index":0,"message":{"role":"Assistant","content":"before","tool_calls":null,"thinking":null,"timestamp":"1970-01-01T00:00:01Z"}}"#;
        let parsed: OwnedSessionLogRecord = serde_json::from_str(old_record).unwrap();
        assert!(parsed.message.response_items.is_empty());

        let expected_record = format!("{old_record}\n").into_bytes();
        assert_eq!(
            serialize_log_record(parsed.index, &parsed.message).unwrap(),
            expected_record
        );

        let root = TempDir::new().unwrap();
        let store = SessionStore::new(root.path());
        let mut session = SessionV2::new(
            "Pre-change",
            "/workspace",
            "deepseek",
            "deepseek-chat",
            vec![parsed.message],
        );
        let path = store.save(&mut session).unwrap();
        let storage_key = path.file_stem().unwrap().to_str().unwrap();
        let before = read_bundle_metadata_if_present(&path, storage_key)
            .unwrap()
            .unwrap();
        let before_bytes = std::fs::read(bundle_log_path(&path, &before)).unwrap();
        assert_eq!(before_bytes, expected_record);
        assert!(!String::from_utf8_lossy(&before_bytes).contains("response_items"));

        let sentinel = "opaque-appended-provider-state-sentinel";
        let mut appended = message(Role::Assistant, "after", 2);
        appended.response_items = vec![provider_state(sentinel)];
        session.messages.push(appended);
        store.save(&mut session).unwrap();

        let after = read_bundle_metadata_if_present(&path, storage_key)
            .unwrap()
            .unwrap();
        let after_bytes = std::fs::read(bundle_log_path(&path, &after)).unwrap();
        assert_eq!(after.generation, before.generation);
        assert!(after_bytes.starts_with(&before_bytes));
        assert_eq!(
            store.load(storage_key).unwrap().messages[1].response_items,
            vec![provider_state(sentinel)]
        );
    }

    #[test]
    fn provider_state_round_trips_full_lazy_pages_and_branches_with_redacted_debug() {
        let sentinels = [
            "opaque-oldest-provider-state-sentinel",
            "opaque-middle-provider-state-sentinel",
            "opaque-newest-provider-state-sentinel",
        ];
        let states = [
            provider_state(sentinels[0]),
            responses_provider_state(sentinels[1]),
            provider_state(sentinels[2]),
        ];
        let mut messages = (0..130)
            .map(|index| {
                message(
                    Role::Assistant,
                    &format!("message-{index:03}"),
                    index as i64 + 1,
                )
            })
            .collect::<Vec<_>>();
        for (index, state) in [0, 65, 129].into_iter().zip(&states) {
            messages[index].response_items = vec![state.clone()];
        }

        let root = TempDir::new().unwrap();
        let store = SessionStore::new(root.path());
        let mut session = SessionV2::new(
            "Provider state",
            "/workspace",
            "deepseek",
            "deepseek-chat",
            messages,
        );
        let path = store.save(&mut session).unwrap();
        let storage_key = path.file_stem().unwrap().to_str().unwrap();

        let full = store.load(storage_key).unwrap();
        for (index, state) in [0, 65, 129].into_iter().zip(&states) {
            assert_eq!(full.messages[index].response_items, vec![state.clone()]);
        }
        let full_debug = format!("{full:?}");
        for sentinel in sentinels {
            assert!(!full_debug.contains(sentinel));
        }
        assert!(full_debug.contains("[REDACTED]"));

        let metadata = read_bundle_metadata_if_present(&path, storage_key)
            .unwrap()
            .unwrap();
        let persisted = std::fs::read_to_string(bundle_log_path(&path, &metadata)).unwrap();
        for sentinel in sentinels {
            assert!(persisted.contains(sentinel));
        }

        let recent = store.load_recent(storage_key, 64).unwrap();
        assert_eq!(recent.start_index, 66);
        assert_eq!(
            recent.session.messages[63].response_items,
            vec![states[2].clone()]
        );
        let middle = store
            .load_page(
                storage_key,
                session.id,
                recent.storage_generation.unwrap(),
                recent.start_index,
                64,
            )
            .unwrap();
        assert_eq!(middle.start_index, 2);
        assert_eq!(middle.messages[63].response_items, vec![states[1].clone()]);
        let oldest = store
            .load_page(
                storage_key,
                session.id,
                middle.storage_generation,
                middle.start_index,
                64,
            )
            .unwrap();
        assert_eq!(oldest.start_index, 0);
        assert_eq!(oldest.messages[0].response_items, vec![states[0].clone()]);

        let branch = store.branch(&full, 66, 1, "Provider-state branch").unwrap();
        assert_eq!(branch.messages.len(), 66);
        assert_eq!(branch.messages[0].response_items, vec![states[0].clone()]);
        assert_eq!(branch.messages[65].response_items, vec![states[1].clone()]);
        let restored_branch = store.load(&branch.id.to_string()).unwrap();
        assert_eq!(
            restored_branch.messages[65].response_items,
            vec![states[1].clone()]
        );
    }

    #[test]
    fn v4_round_trip_and_stable_upsert_use_one_file() {
        let root = TempDir::new().unwrap();
        let store = SessionStore::new(root.path());
        let mut session = test_session();
        session.context_checkpoint = Some(ContextCheckpoint {
            format_version: 1,
            through_message_count: 2,
            summary: "## Goal\nContinue".into(),
            created_at: DateTime::from_timestamp(3, 0).unwrap(),
            source_tokens: 12,
            source_sha256: None,
            anchors: Vec::new(),
            user_quotes: Vec::new(),
            recall_pointer: None,
        });
        session.reasoning_effort = Some(crate::config::ReasoningEffort::High);
        session.attached_worktree =
            Some(crate::worktree::WorktreeAttachment { id: Uuid::new_v4() });
        session.pinned = true;
        session.queued_inputs = vec![
            crate::queue::PendingInput {
                id: Uuid::new_v4(),
                text: "first queued draft".into(),
                created_at: DateTime::from_timestamp(5, 0).unwrap(),
            },
            crate::queue::PendingInput {
                id: Uuid::new_v4(),
                text: "second queued draft".into(),
                created_at: DateTime::from_timestamp(6, 0).unwrap(),
            },
        ];
        session.workspace_roots = vec!["/workspace/shared".into()];
        session.manual_rule_ids = vec!["user/manual".into(), "workspace-0/rust".into()];
        session.active_skills = vec![crate::skills::ActiveSkillRef {
            id: "deploy".into(),
            path: PathBuf::from("/workspace/.ca/skills/deploy/SKILL.md"),
            sha256: "abc123".into(),
        }];
        session.agent_summaries = vec![crate::work::AgentSummary {
            id: crate::work::AgentSummaryId(Uuid::new_v4()),
            status: "Interrupted".into(),
            summary: "unfinished child".into(),
            updated_at: DateTime::from_timestamp(4, 0).unwrap(),
        }];
        let turn_id = Uuid::new_v4();
        let agent_id = crate::agent::subagent::AgentId::new();
        session.transcript_annotations = vec![crate::app::TranscriptMessageAnnotation {
            message_timestamp: session.messages[1].timestamp,
            turn_id,
            reasoning_duration_ms: Some(651),
            provider: None,
            model: None,
            total_turn_duration_ms: None,
            terminal_outcome: None,
        }];
        session.transcript_agent_summaries = vec![crate::app::TranscriptAgentSummary {
            agent_id,
            turn_id,
            purpose: crate::agent::subagent::AgentRunPurpose::Subagent,
            task: "audit UI".into(),
            role: None,
            model: None,
            provider: None,
            created_at: None,
            status: crate::agent::subagent::AgentStatus::Completed,
            summary: "UI audited".into(),
            updated_at: DateTime::from_timestamp(4, 0).unwrap(),
        }];
        session.review_records = vec![crate::work::ReviewRecord {
            id: crate::work::ReviewId::new(),
            scope: "working tree".into(),
            summary: "one issue".into(),
            model: "reviewer".into(),
            findings: vec![crate::review::ReviewFinding {
                severity: crate::review::ReviewSeverity::High,
                title: "Issue".into(),
                rationale: "Rationale".into(),
                file: "src/main.rs".into(),
                line_start: 1,
                line_end: 1,
                confidence: 90,
                suggested_validation: "test".into(),
            }],
            truncated: false,
            created_at: DateTime::from_timestamp(4, 0).unwrap(),
        }];
        let id = session.id;
        let first = store.save(&mut session).unwrap();
        session.title = "Renamed".into();
        let second = store.save(&mut session).unwrap();

        assert_eq!(first, second);
        assert_eq!(store.list().unwrap().len(), 1);
        let restored = store.load(&id.to_string()).unwrap();
        assert_eq!(restored.id, id);
        assert_eq!(restored.version, 4);
        assert_eq!(restored.title, "Renamed");
        assert!(restored.pinned);
        assert_eq!(restored.queued_inputs, session.queued_inputs);
        assert_eq!(restored.messages.len(), 2);
        assert_eq!(restored.workspace_roots, ["/workspace/shared"]);
        assert_eq!(restored.manual_rule_ids, session.manual_rule_ids);
        assert_eq!(restored.active_skills, session.active_skills);
        assert_eq!(restored.agent_summaries, session.agent_summaries);
        assert_eq!(
            restored.transcript_annotations,
            session.transcript_annotations
        );
        assert_eq!(
            restored.transcript_agent_summaries,
            session.transcript_agent_summaries
        );
        assert_eq!(restored.review_records, session.review_records);
        assert_eq!(restored.attached_worktree, session.attached_worktree);
        assert_eq!(
            restored.reasoning_effort,
            Some(crate::config::ReasoningEffort::High)
        );
        let checkpoint = restored.context_checkpoint.unwrap();
        assert_eq!(checkpoint.through_message_count, 2);
        assert_eq!(
            checkpoint.format_version,
            crate::context::CONTEXT_CHECKPOINT_FORMAT_VERSION
        );
        assert!(checkpoint.source_sha256.is_some());
    }

    #[test]
    fn full_session_load_rejects_a_mismatched_current_checkpoint() {
        let root = TempDir::new().unwrap();
        let store = SessionStore::new(root.path());
        let mut session = test_session();
        session.context_checkpoint = Some(ContextCheckpoint {
            format_version: 1,
            through_message_count: 2,
            summary: "## Goal\nContinue".into(),
            created_at: DateTime::from_timestamp(3, 0).unwrap(),
            source_tokens: 12,
            source_sha256: None,
            anchors: Vec::new(),
            user_quotes: Vec::new(),
            recall_pointer: None,
        });
        session.derive_or_validate_context_checkpoint().unwrap();
        session.messages[0].content = "tampered covered source".into();
        let path = root.path().join(format!("{}.json", session.id));
        std::fs::write(&path, serde_json::to_vec_pretty(&session).unwrap()).unwrap();

        let error = store.load(&session.id.to_string()).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error
            .to_string()
            .contains("does not match canonical history"));
    }

    #[test]
    fn branch_derives_legacy_checkpoint_integrity_for_the_retained_prefix() {
        let mut source = test_session();
        source.context_checkpoint = Some(ContextCheckpoint {
            format_version: 1,
            through_message_count: 2,
            summary: "## Goal\nContinue".into(),
            created_at: DateTime::from_timestamp(3, 0).unwrap(),
            source_tokens: 12,
            source_sha256: None,
            anchors: Vec::new(),
            user_quotes: Vec::new(),
            recall_pointer: None,
        });

        let branch = branch_in_memory(&source, 2, 1, "checkpoint branch").unwrap();
        let checkpoint = branch.context_checkpoint.unwrap();

        assert_eq!(
            checkpoint.format_version,
            crate::context::CONTEXT_CHECKPOINT_FORMAT_VERSION
        );
        assert!(checkpoint.source_sha256.is_some());
        assert!(checkpoint.validate_source(&branch.messages).is_ok());
        assert_eq!(source.context_checkpoint.unwrap().format_version, 1);
    }

    #[test]
    fn new_bundle_lists_from_metadata_and_reads_exact_recent_pages() {
        let root = TempDir::new().unwrap();
        let store = SessionStore::new(root.path());
        let mut session = SessionV2::new(
            "Paged",
            "/workspace",
            "local",
            "coder",
            (0..130)
                .map(|index| message(Role::User, &format!("message-{index:03}"), index + 1))
                .collect(),
        );
        let path = store.save(&mut session).unwrap();
        let storage_key = path.file_stem().unwrap().to_str().unwrap();
        let raw: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(raw["storage_version"], 1);
        assert!(raw.get("messages").is_none());
        assert!(raw["session"].get("messages").is_none());

        let summaries = store.list().unwrap();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].title, "Paged");
        let recent = store.load_recent(storage_key, 64).unwrap();
        assert!(recent.paged);
        assert_eq!(recent.start_index, 66);
        assert_eq!(recent.total_messages, 130);
        assert_eq!(recent.session.messages.len(), 64);
        assert_eq!(recent.session.messages[0].content, "message-066");
        let middle = store
            .load_page(
                storage_key,
                session.id,
                recent.storage_generation.unwrap(),
                recent.start_index,
                64,
            )
            .unwrap();
        assert_eq!((middle.start_index, middle.messages.len()), (2, 64));
        assert_eq!(middle.messages[0].content, "message-002");
        let oldest = store
            .load_page(
                storage_key,
                session.id,
                middle.storage_generation,
                middle.start_index,
                64,
            )
            .unwrap();
        assert_eq!((oldest.start_index, oldest.messages.len()), (0, 2));
        assert_eq!(oldest.messages[1].content, "message-001");
    }

    #[test]
    fn bundle_prefix_save_appends_and_metadata_only_pin_keeps_log_bytes() {
        let root = TempDir::new().unwrap();
        let store = SessionStore::new(root.path());
        let mut session = test_session();
        let path = store.save(&mut session).unwrap();
        let storage_key = path.file_stem().unwrap().to_str().unwrap();
        let before = read_bundle_metadata_if_present(&path, storage_key)
            .unwrap()
            .unwrap();
        let before_log = bundle_log_path(&path, &before);
        let before_bytes = std::fs::read(&before_log).unwrap();

        session.messages.push(message(Role::User, "appended", 8));
        store.save(&mut session).unwrap();
        let appended = read_bundle_metadata_if_present(&path, storage_key)
            .unwrap()
            .unwrap();
        assert_eq!(appended.generation, before.generation);
        let appended_bytes = std::fs::read(bundle_log_path(&path, &appended)).unwrap();
        assert!(appended_bytes.starts_with(&before_bytes));
        assert!(appended_bytes.len() > before_bytes.len());

        store.set_pinned(storage_key, session.id, true).unwrap();
        let pinned = read_bundle_metadata_if_present(&path, storage_key)
            .unwrap()
            .unwrap();
        assert_eq!(pinned.generation, appended.generation);
        assert_eq!(
            std::fs::read(bundle_log_path(&path, &pinned)).unwrap(),
            appended_bytes
        );
        assert!(store.load(storage_key).unwrap().pinned);
    }

    #[test]
    fn bundle_rewrite_or_uncommitted_tail_rotates_without_trusting_stale_bytes() {
        let root = TempDir::new().unwrap();
        let store = SessionStore::new(root.path());
        let mut session = test_session();
        let path = store.save(&mut session).unwrap();
        let storage_key = path.file_stem().unwrap().to_str().unwrap();
        let first = read_bundle_metadata_if_present(&path, storage_key)
            .unwrap()
            .unwrap();
        let first_log = bundle_log_path(&path, &first);
        OpenOptions::new()
            .append(true)
            .open(&first_log)
            .unwrap()
            .write_all(b"uncommitted-tail")
            .unwrap();

        session.messages.push(message(Role::User, "after tail", 9));
        store.save(&mut session).unwrap();
        let second = read_bundle_metadata_if_present(&path, storage_key)
            .unwrap()
            .unwrap();
        assert_ne!(second.generation, first.generation);
        let stale_page = store
            .load_page(storage_key, session.id, first.generation, 2, 2)
            .unwrap_err();
        assert_eq!(stale_page.kind(), io::ErrorKind::InvalidData);
        assert!(!first_log.exists());
        assert_eq!(store.load(storage_key).unwrap().messages.len(), 3);

        session.messages[0].content = "rewritten".into();
        store.save(&mut session).unwrap();
        let third = read_bundle_metadata_if_present(&path, storage_key)
            .unwrap()
            .unwrap();
        assert_ne!(third.generation, second.generation);
        assert_eq!(
            store.load(storage_key).unwrap().messages[0].content,
            "rewritten"
        );
        assert!(!format!("{third:?}").contains("rewritten"));
    }

    #[test]
    fn bundle_listing_is_metadata_only_but_paged_reads_validate_records() {
        let root = TempDir::new().unwrap();
        let store = SessionStore::new(root.path());
        let mut session = test_session();
        let path = store.save(&mut session).unwrap();
        let storage_key = path.file_stem().unwrap().to_str().unwrap();
        let metadata = read_bundle_metadata_if_present(&path, storage_key)
            .unwrap()
            .unwrap();
        let log_path = bundle_log_path(&path, &metadata);
        let mut bytes = std::fs::read(&log_path).unwrap();
        bytes[0] = b'!';
        std::fs::write(&log_path, bytes).unwrap();

        let summaries = store.list().unwrap();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].id, session.id);
        assert!(store.load_recent(storage_key, 64).is_err());
    }

    #[test]
    fn bundle_rejects_incomplete_records_bad_offsets_and_symlinked_logs() {
        let root = TempDir::new().unwrap();
        let store = SessionStore::new(root.path());
        let mut session = test_session();
        let path = store.save(&mut session).unwrap();
        let storage_key = path.file_stem().unwrap().to_str().unwrap();
        let metadata = read_bundle_metadata_if_present(&path, storage_key)
            .unwrap()
            .unwrap();
        let log_path = bundle_log_path(&path, &metadata);
        let original = std::fs::read(&log_path).unwrap();

        std::fs::write(&log_path, &original[..original.len() - 1]).unwrap();
        assert!(store.load_recent(storage_key, 64).is_err());
        std::fs::write(&log_path, &original).unwrap();

        let mut malformed = metadata.clone();
        malformed.offsets[1] = 0;
        write_bundle_metadata(&path, &malformed).unwrap();
        assert!(store.load_recent(storage_key, 64).is_err());
        write_bundle_metadata(&path, &metadata).unwrap();

        #[cfg(unix)]
        {
            std::fs::remove_file(&log_path).unwrap();
            let outside = root.path().join("outside");
            std::fs::write(&outside, original).unwrap();
            std::os::unix::fs::symlink(outside, &log_path).unwrap();
            assert!(store.load_recent(storage_key, 64).is_err());
        }
    }

    #[test]
    fn failed_over_bound_legacy_migration_preserves_original_bytes() {
        let root = TempDir::new().unwrap();
        let store = SessionStore::new(root.path());
        let path = root.path().join("large-legacy.json");
        let mut session = test_session();
        session.messages[0].content = "x".repeat(MAX_SESSION_MESSAGE_BYTES + 1);
        let original = serde_json::to_vec_pretty(&session).unwrap();
        std::fs::write(&path, &original).unwrap();

        // Prepare the intentionally large legacy identity outside the bounded
        // writer probe, as normal worker-backed loading does.
        store.prepare_identity_inventory().unwrap();
        let mut loaded = store.load("large-legacy").unwrap();
        let error = store.save(&mut loaded).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert_eq!(
            std::fs::read_dir(root.path())
                .unwrap()
                .flatten()
                .filter(
                    |entry| entry.path().extension().and_then(|value| value.to_str())
                        == Some("jsonl")
                )
                .count(),
            0
        );
    }

    #[test]
    fn bounded_committed_preview_reads_a_bundle_and_never_falls_back_to_whole_legacy_history() {
        let root = TempDir::new().unwrap();
        let store = SessionStore::new(root.path());
        let mut session = test_session();
        store.save(&mut session).unwrap();
        let preview = store.load_recent_committed_bounded(session.id, 1).unwrap();
        assert_eq!(preview.session.id, session.id);
        assert_eq!(preview.session.messages.len(), 1);
        assert_eq!(preview.total_messages, session.messages.len());
        assert!(store.load_recent_committed_bounded(session.id, 5).is_err());
        let mut legacy = test_session();
        legacy.id = Uuid::new_v4();
        std::fs::write(
            root.path().join(format!("{}.json", legacy.id)),
            serde_json::to_vec(&legacy).unwrap(),
        )
        .unwrap();
        assert!(store.load_recent_committed_bounded(legacy.id, 4).is_err());
        assert_eq!(store.load(&legacy.id.to_string()).unwrap().id, legacy.id);
    }

    #[test]
    fn v1_load_is_in_memory_until_next_save() {
        let root = TempDir::new().unwrap();
        let store = SessionStore::new(root.path());
        let path = root.path().join("20260101-legacy.json");
        std::fs::write(
            &path,
            r#"{
                "version": 1,
                "model": "old-model",
                "provider": "old-provider",
                "created": "2026-01-01T00:00:00Z",
                "name": "Legacy",
                "messages": [{"role":"user","content":"hello","tool_calls":null,"timestamp":"2026-01-01T00:00:00Z"}]
            }"#,
        )
        .unwrap();

        let mut loaded = store.load("20260101-legacy").unwrap();
        assert_eq!(loaded.version, 4);
        assert!(!loaded.pinned);
        assert!(loaded.queued_inputs.is_empty());
        assert!(loaded.attached_worktree.is_none());
        assert!(std::fs::read_to_string(&path)
            .unwrap()
            .contains("\"version\": 1"));
        let expected_id = Uuid::new_v5(&Uuid::NAMESPACE_URL, b"20260101-legacy");
        assert_eq!(loaded.id, expected_id);

        store.save(&mut loaded).unwrap();
        assert!(std::fs::read_to_string(&path)
            .unwrap()
            .contains("\"version\": 4"));
        assert_eq!(store.list().unwrap().len(), 1);
    }

    #[test]
    fn v2_loads_without_mutation_and_upgrades_in_the_same_file_on_save() {
        let root = TempDir::new().unwrap();
        let store = SessionStore::new(root.path());
        let id = Uuid::new_v4();
        let path = root.path().join("stable-v2.json");
        std::fs::write(
            &path,
            serde_json::json!({
                "version": 2,
                "id": id,
                "title": "Version two",
                "workspace": "/workspace",
                "created_at": "2026-01-01T00:00:00Z",
                "updated_at": "2026-01-01T00:00:00Z",
                "provider": "local",
                "model": "coder",
                "parent": null,
                "messages": [],
                "context_checkpoint": null
            })
            .to_string(),
        )
        .unwrap();

        let mut loaded = store.load("stable-v2").unwrap();
        assert_eq!(loaded.version, 2);
        assert_eq!(loaded.id, id);
        assert!(loaded.work_state.plan.is_none());
        assert!(loaded.attached_worktree.is_none());
        assert!(std::fs::read_to_string(&path)
            .unwrap()
            .contains("\"version\":2"));

        let saved = store.save(&mut loaded).unwrap();
        assert_eq!(saved, path);
        assert_eq!(loaded.version, 4);
        assert!(std::fs::read_to_string(saved)
            .unwrap()
            .contains("\"version\": 4"));
    }

    #[test]
    fn v3_loads_without_mutation_and_defaults_v4_fields_until_save() {
        let root = TempDir::new().unwrap();
        let store = SessionStore::new(root.path());
        let id = Uuid::new_v4();
        let path = root.path().join("stable-v3.json");
        std::fs::write(
            &path,
            serde_json::json!({
                "version": 3,
                "id": id,
                "title": "Version three",
                "workspace": "/workspace",
                "created_at": "2026-01-01T00:00:00Z",
                "updated_at": "2026-01-01T00:00:00Z",
                "provider": "local",
                "model": "coder",
                "parent": null,
                "messages": [],
                "context_checkpoint": null
            })
            .to_string(),
        )
        .unwrap();

        let mut loaded = store.load("stable-v3").unwrap();
        assert_eq!(loaded.version, 3);
        assert!(!loaded.pinned);
        assert!(loaded.queued_inputs.is_empty());
        assert!(loaded.attached_worktree.is_none());
        assert!(std::fs::read_to_string(&path)
            .unwrap()
            .contains("\"version\":3"));

        store.save(&mut loaded).unwrap();
        assert_eq!(loaded.version, 4);
        assert!(std::fs::read_to_string(&path)
            .unwrap()
            .contains("\"version\": 4"));
    }

    #[test]
    fn legacy_v4_read_is_byte_stable_until_successful_bundle_migration() {
        let root = TempDir::new().unwrap();
        let store = SessionStore::new(root.path());
        let path = root.path().join("stable-v4.json");
        let session = test_session();
        let original = serde_json::to_vec_pretty(&session).unwrap();
        std::fs::write(&path, &original).unwrap();

        let recent = store.load_recent("stable-v4", 64).unwrap();
        assert!(!recent.paged);
        assert_eq!(recent.storage_generation, None);
        assert_eq!(std::fs::read(&path).unwrap(), original);

        let mut loaded = recent.session;
        store.save(&mut loaded).unwrap();
        let metadata = read_bundle_metadata_if_present(&path, "stable-v4")
            .unwrap()
            .unwrap();
        assert_eq!(metadata.id, session.id);
        assert!(bundle_log_path(&path, &metadata).is_file());
        let migrated = store.load("stable-v4").unwrap();
        assert_eq!(migrated.messages.len(), session.messages.len());
        assert_eq!(migrated.messages[0].content, session.messages[0].content);
        assert_eq!(migrated.messages[1].content, session.messages[1].content);
    }

    #[test]
    fn failed_atomic_writer_preserves_existing_target() {
        let root = TempDir::new().unwrap();
        let target = root.path().join("session.json");
        std::fs::write(&target, "original").unwrap();
        let result = atomic_write_with(&target, |file| {
            file.write_all(b"partial")?;
            Err(io::Error::other("injected failure"))
        });

        assert!(result.is_err());
        assert_eq!(std::fs::read_to_string(target).unwrap(), "original");
        assert!(!std::fs::read_dir(root.path())
            .unwrap()
            .flatten()
            .any(|entry| entry.path().extension().and_then(|value| value.to_str()) == Some("tmp")));
    }

    #[test]
    fn summaries_sort_pinned_first_then_by_updated_time_and_match_fields() {
        let root = TempDir::new().unwrap();
        let store = SessionStore::new(root.path());
        let mut older = test_session();
        older.title = "Alpha".into();
        older.pinned = true;
        older.updated_at = DateTime::from_timestamp(5, 0).unwrap();
        let older_path = root.path().join(format!("{}.json", older.id));
        std::fs::write(&older_path, serde_json::to_vec(&older).unwrap()).unwrap();
        let mut newer = test_session();
        newer.title = "Beta".into();
        newer.workspace = "/workspace/beta".into();
        newer.updated_at = DateTime::from_timestamp(10, 0).unwrap();
        let newer_path = root.path().join(format!("{}.json", newer.id));
        std::fs::write(&newer_path, serde_json::to_vec(&newer).unwrap()).unwrap();

        let summaries = store.list().unwrap();
        assert_eq!(summaries[0].id, older.id);
        assert!(summaries[1].matches("Beta"));
        assert!(summaries[1].matches("workspace/beta"));
        assert!(summaries[1].matches(&newer.id.to_string()[..8]));
    }

    #[test]
    fn pinning_is_uuid_checked_atomic_and_preserves_the_storage_key() {
        let root = TempDir::new().unwrap();
        let active = root.path().join("sessions");
        let archive = root.path().join("archive");
        let store = SessionStore::with_archive(active.clone(), archive);
        let mut session = test_session();
        let saved = store.save(&mut session).unwrap();
        let storage_key = saved.file_stem().unwrap().to_str().unwrap();

        store.set_pinned(storage_key, session.id, true).unwrap();

        let pinned = store.load(storage_key).unwrap();
        assert!(pinned.pinned);
        assert_eq!(saved, active.join(format!("{storage_key}.json")));
        let mismatch = store
            .set_pinned(storage_key, Uuid::new_v4(), false)
            .unwrap_err();
        assert_eq!(mismatch.kind(), io::ErrorKind::InvalidData);
        assert!(store.load(storage_key).unwrap().pinned);
        for invalid in ["", ".", "..", "../outside", "nested/session"] {
            assert_eq!(
                store
                    .set_pinned(invalid, session.id, false)
                    .unwrap_err()
                    .kind(),
                io::ErrorKind::InvalidInput
            );
        }
    }

    #[test]
    fn archive_and_restore_move_exactly_one_file_without_rewriting_descendants() {
        let root = TempDir::new().unwrap();
        let active = root.path().join("sessions");
        let archive = root.path().join("archive");
        let store = SessionStore::with_archive(active.clone(), archive.clone());
        let mut source = test_session();
        let source_path = store.save(&mut source).unwrap();
        let source_bytes = std::fs::read(&source_path).unwrap();
        let source_key = source_path.file_stem().unwrap().to_str().unwrap();
        let source_metadata = read_bundle_metadata_if_present(&source_path, source_key)
            .unwrap()
            .unwrap();
        let source_log = bundle_log_path(&source_path, &source_metadata);
        let source_log_bytes = std::fs::read(&source_log).unwrap();
        let mut branch = store.branch(&source, 2, 1, "Branch").unwrap();
        let branch_parent = branch.parent.clone();
        let branch_path = store.save(&mut branch).unwrap();
        let storage_key = source_path.file_stem().unwrap().to_str().unwrap();

        store.archive(storage_key, source.id).unwrap();

        assert!(!source_path.exists());
        let archived_path = archive.join(format!("{storage_key}.json"));
        assert_eq!(std::fs::read(&archived_path).unwrap(), source_bytes);
        let archived_log = bundle_log_path(&archived_path, &source_metadata);
        assert_eq!(std::fs::read(&archived_log).unwrap(), source_log_bytes);
        assert!(!source_log.exists());
        assert!(branch_path.is_file());
        assert_eq!(
            store.load(&branch.id.to_string()).unwrap().parent,
            branch_parent
        );
        assert_eq!(store.list().unwrap().len(), 1);
        assert_eq!(store.list_archived().unwrap()[0].id, source.id);

        store.restore(storage_key, source.id).unwrap();

        assert!(!archived_path.exists());
        assert_eq!(std::fs::read(source_path).unwrap(), source_bytes);
        assert!(!archived_log.exists());
        assert_eq!(std::fs::read(source_log).unwrap(), source_log_bytes);
        assert!(store.list_archived().unwrap().is_empty());
    }

    #[test]
    fn archive_rejects_traversal_uuid_mismatch_and_destination_collision() {
        let root = TempDir::new().unwrap();
        let active = root.path().join("sessions");
        let archive = root.path().join("archive");
        let store = SessionStore::with_archive(active.clone(), archive.clone());
        let mut session = test_session();
        let source_path = store.save(&mut session).unwrap();
        let storage_key = source_path.file_stem().unwrap().to_str().unwrap();

        for invalid in ["", ".", "..", "../outside", "nested/session"] {
            assert_eq!(
                store.archive(invalid, session.id).unwrap_err().kind(),
                io::ErrorKind::InvalidInput
            );
        }
        assert_eq!(
            store
                .archive(storage_key, Uuid::new_v4())
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        std::fs::create_dir_all(&archive).unwrap();
        let collision = archive.join(format!("{storage_key}.json"));
        std::fs::write(&collision, b"do not overwrite").unwrap();
        assert_eq!(
            store.archive(storage_key, session.id).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert!(source_path.is_file());
        assert_eq!(std::fs::read(collision).unwrap(), b"do not overwrite");
        let source_metadata = read_bundle_metadata_if_present(&source_path, storage_key)
            .unwrap()
            .unwrap();
        assert!(!archive.join(bundle_log_name(&source_metadata)).exists());
    }

    #[test]
    fn delete_unlinks_only_the_expected_session_file() {
        let root = TempDir::new().unwrap();
        let store = SessionStore::new(root.path());
        let mut source = test_session();
        let source_path = store.save(&mut source).unwrap();
        let source_key = source_path.file_stem().unwrap().to_str().unwrap();
        let source_metadata = read_bundle_metadata_if_present(&source_path, source_key)
            .unwrap()
            .unwrap();
        let source_log = bundle_log_path(&source_path, &source_metadata);
        let mut branch = store.branch(&source, 2, 1, "Branch").unwrap();
        let branch_parent = branch.parent.clone();
        store.save(&mut branch).unwrap();

        store.delete(&source.id.to_string(), source.id).unwrap();

        assert!(store.load(&source.id.to_string()).is_err());
        assert!(!source_path.exists());
        assert!(!source_log.exists());
        let preserved = store.load(&branch.id.to_string()).unwrap();
        assert_eq!(preserved.parent, branch_parent);
    }

    #[test]
    fn delete_also_drops_the_sessions_recovery_state() {
        let root = TempDir::new().unwrap();
        let store = SessionStore::new(root.path());
        let mut source = test_session();
        store.save(&mut source).unwrap();
        // A recovery journal for this session, as a prior crash or an undo
        // capture would have left behind.
        let recovery = root.path().join("recovery").join(source.id.to_string());
        std::fs::create_dir_all(&recovery).unwrap();
        std::fs::write(recovery.join("journal.json"), b"{}").unwrap();

        store.delete(&source.id.to_string(), source.id).unwrap();

        assert!(!recovery.exists());
    }

    #[test]
    fn purge_removes_one_archived_session_and_leaves_the_directory() {
        let temp = TempDir::new().unwrap();
        let active_root = temp.path().join("sessions");
        let archive_root = temp.path().join("archive");
        let store = SessionStore::with_archive(&active_root, &archive_root);
        let mut kept = SessionV2::new("Kept", "/workspace", "p", "m", Vec::new());
        let mut archived = SessionV2::new("Archived", "/workspace", "p", "m", Vec::new());
        store.save(&mut kept).unwrap();
        store.save(&mut archived).unwrap();
        store
            .archive(&archived.id.to_string(), archived.id)
            .unwrap();
        let recovery = temp.path().join("recovery").join(archived.id.to_string());
        std::fs::create_dir_all(&recovery).unwrap();
        std::fs::write(recovery.join("journal.json"), b"{}").unwrap();
        let archived_path = archive_root.join(format!("{}.json", archived.id));
        assert!(archived_path.is_file());

        store
            .purge(&archived.id.to_string(), archived.id, true)
            .unwrap();

        assert!(!archived_path.exists());
        assert!(!recovery.exists());
        assert!(archive_root.is_dir());
        assert!(active_root.is_dir());
        assert_eq!(store.load(&kept.id.to_string()).unwrap().title, "Kept");
        assert!(store.list_archived().unwrap().is_empty());
    }

    #[test]
    fn delete_rejects_traversal_and_uuid_mismatch_without_unlinking() {
        let root = TempDir::new().unwrap();
        let store = SessionStore::new(root.path());
        let mut session = test_session();
        let path = store.save(&mut session).unwrap();

        for invalid in ["", ".", "..", "../outside", "nested/session"] {
            let error = store.delete(invalid, session.id).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        }
        let mismatch = store
            .delete(&session.id.to_string(), Uuid::new_v4())
            .unwrap_err();
        assert_eq!(mismatch.kind(), io::ErrorKind::InvalidData);
        assert!(path.is_file());
    }

    #[test]
    fn bundle_delete_reports_history_cleanup_failure_after_exact_metadata_unlink() {
        let root = TempDir::new().unwrap();
        let store = SessionStore::new(root.path());
        let mut session = test_session();
        let path = store.save(&mut session).unwrap();
        let storage_key = path.file_stem().unwrap().to_str().unwrap();
        let metadata = read_bundle_metadata_if_present(&path, storage_key)
            .unwrap()
            .unwrap();
        let log_path = bundle_log_path(&path, &metadata);
        let mut calls = 0;

        let error = store
            .delete_with(storage_key, session.id, |target| {
                calls += 1;
                if calls == 2 {
                    Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "fixture denial",
                    ))
                } else {
                    std::fs::remove_file(target)
                }
            })
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        assert!(error.to_string().contains("cleanup failed"));
        assert!(!path.exists());
        assert!(log_path.exists());
    }

    fn retention_candidate(
        storage_key: String,
        session: &SessionV2,
    ) -> crate::session_retention::RetentionCandidate {
        crate::session_retention::RetentionCandidate {
            storage_key,
            id: session.id,
            title: session.title.clone(),
            updated_at: session.updated_at,
            reason: crate::session_retention::RetentionReason::Age,
        }
    }

    #[test]
    fn retention_batch_deletes_only_exact_active_candidates() {
        let root = TempDir::new().unwrap();
        let active = root.path().join("sessions");
        let archive = root.path().join("archive");
        let store = SessionStore::with_archive(&active, &archive);
        let mut current = test_session();
        current.title = "Current".into();
        let current_path = store.save(&mut current).unwrap();
        let mut pinned = test_session();
        pinned.title = "Pinned".into();
        pinned.pinned = true;
        let pinned_path = store.save(&mut pinned).unwrap();
        let mut source = test_session();
        source.title = "Expired source".into();
        let source_path = store.save(&mut source).unwrap();
        let branch = store.branch(&source, 2, 1, "Child branch").unwrap();
        let mut archived = test_session();
        archived.title = "Archived".into();
        let archived_path = store.save(&mut archived).unwrap();
        let archived_key = archived_path.file_stem().unwrap().to_str().unwrap();
        store.archive(archived_key, archived.id).unwrap();

        let source_key = source_path.file_stem().unwrap().to_str().unwrap();
        let outcome = store
            .delete_retention_batch(
                &[retention_candidate(source_key.into(), &source)],
                Some(current.id),
            )
            .unwrap();

        assert_eq!(outcome.removed, 1);
        assert!(!source_path.exists());
        assert!(current_path.is_file());
        assert!(pinned_path.is_file());
        assert_eq!(
            store.load(&branch.id.to_string()).unwrap().parent,
            branch.parent
        );
        assert_eq!(store.list_archived().unwrap()[0].id, archived.id);
    }

    #[test]
    fn retention_batch_preflights_every_candidate_before_unlinking() {
        let root = TempDir::new().unwrap();
        let store = SessionStore::new(root.path());
        let mut ordinary = test_session();
        ordinary.title = "Ordinary".into();
        let ordinary_path = store.save(&mut ordinary).unwrap();
        let ordinary_key = ordinary_path.file_stem().unwrap().to_str().unwrap();
        let mut pinned = test_session();
        pinned.title = "Pinned".into();
        pinned.pinned = true;
        let pinned_path = store.save(&mut pinned).unwrap();
        let pinned_key = pinned_path.file_stem().unwrap().to_str().unwrap();

        let failure = store
            .delete_retention_batch(
                &[
                    retention_candidate(ordinary_key.into(), &ordinary),
                    retention_candidate(pinned_key.into(), &pinned),
                ],
                None,
            )
            .unwrap_err();
        assert_eq!(failure.removed, 0);
        assert_eq!(failure.remaining, 2);
        assert!(failure.message.contains("pinned"));
        assert!(ordinary_path.is_file());
        assert!(pinned_path.is_file());

        let current_failure = store
            .delete_retention_batch(
                &[retention_candidate(ordinary_key.into(), &ordinary)],
                Some(ordinary.id),
            )
            .unwrap_err();
        assert_eq!(current_failure.removed, 0);
        assert!(current_failure.message.contains("current"));
        assert!(ordinary_path.is_file());

        let mut traversal = retention_candidate("../outside".into(), &ordinary);
        let traversal_failure = store
            .delete_retention_batch(&[traversal.clone()], None)
            .unwrap_err();
        assert_eq!(traversal_failure.removed, 0);
        assert!(ordinary_path.is_file());

        traversal.storage_key = ordinary_key.into();
        traversal.id = Uuid::new_v4();
        let mismatch = store
            .delete_retention_batch(&[traversal], None)
            .unwrap_err();
        assert_eq!(mismatch.removed, 0);
        assert!(ordinary_path.is_file());

        let duplicate = retention_candidate(ordinary_key.into(), &ordinary);
        let duplicate_failure = store
            .delete_retention_batch(&[duplicate.clone(), duplicate], None)
            .unwrap_err();
        assert_eq!(duplicate_failure.removed, 0);
        assert!(ordinary_path.is_file());
    }

    #[test]
    fn retention_batch_reports_truthful_partial_unlink_failure() {
        let root = TempDir::new().unwrap();
        let store = SessionStore::new(root.path());
        let mut first = test_session();
        first.title = "First".into();
        let first_path = store.save(&mut first).unwrap();
        let mut second = test_session();
        second.title = "Second".into();
        let second_path = store.save(&mut second).unwrap();
        let candidates = vec![
            retention_candidate(
                first_path.file_stem().unwrap().to_str().unwrap().into(),
                &first,
            ),
            retention_candidate(
                second_path.file_stem().unwrap().to_str().unwrap().into(),
                &second,
            ),
        ];
        let mut calls = 0;
        let failure = store
            .delete_retention_batch_with(&candidates, None, |path| {
                calls += 1;
                if calls == 3 {
                    Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "fixture denial",
                    ))
                } else {
                    std::fs::remove_file(path)
                }
            })
            .unwrap_err();

        assert_eq!(failure.removed, 1);
        assert_eq!(failure.remaining, 1);
        assert!(!first_path.exists());
        assert!(second_path.is_file());
    }

    #[test]
    fn retention_reports_log_cleanup_failure_as_a_completed_logical_delete() {
        let root = TempDir::new().unwrap();
        let store = SessionStore::new(root.path());
        let mut first = test_session();
        first.title = "First".into();
        let first_path = store.save(&mut first).unwrap();
        let first_key = first_path.file_stem().unwrap().to_str().unwrap();
        let first_metadata = read_bundle_metadata_if_present(&first_path, first_key)
            .unwrap()
            .unwrap();
        let first_log = bundle_log_path(&first_path, &first_metadata);
        let mut second = test_session();
        second.title = "Second".into();
        let second_path = store.save(&mut second).unwrap();
        let second_key = second_path.file_stem().unwrap().to_str().unwrap();
        let candidates = vec![
            retention_candidate(first_key.into(), &first),
            retention_candidate(second_key.into(), &second),
        ];
        let mut calls = 0;

        let failure = store
            .delete_retention_batch_with(&candidates, None, |path| {
                calls += 1;
                if calls == 2 {
                    Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "fixture denial",
                    ))
                } else {
                    std::fs::remove_file(path)
                }
            })
            .unwrap_err();

        assert_eq!(failure.removed, 1);
        assert_eq!(failure.remaining, 1);
        assert!(failure.message.contains("history cleanup failed"));
        assert!(!first_path.exists());
        assert!(first_log.exists());
        assert!(second_path.exists());
    }

    #[test]
    fn historical_and_full_branches_do_not_mutate_source() {
        let root = TempDir::new().unwrap();
        let store = SessionStore::new(root.path());
        let mut source = test_session();
        source.messages.extend([
            message(Role::User, "second", 3),
            message(Role::Assistant, "done", 4),
        ]);
        source.workspace_roots = vec!["/workspace/additional".into()];
        source.active_skills = vec![crate::skills::ActiveSkillRef {
            id: "review".into(),
            path: PathBuf::from("/workspace/.ca/skills/review/SKILL.md"),
            sha256: "def456".into(),
        }];
        source.agent_summaries = vec![crate::work::AgentSummary {
            id: crate::work::AgentSummaryId(Uuid::new_v4()),
            status: "Running".into(),
            summary: "must not inherit".into(),
            updated_at: DateTime::from_timestamp(5, 0).unwrap(),
        }];
        source.review_records = vec![crate::work::ReviewRecord {
            id: crate::work::ReviewId::new(),
            scope: "working tree".into(),
            summary: "clean".into(),
            model: "reviewer".into(),
            findings: Vec::new(),
            truncated: false,
            created_at: DateTime::from_timestamp(5, 0).unwrap(),
        }];
        source.pinned = true;
        source.queued_inputs = vec![crate::queue::PendingInput {
            id: Uuid::new_v4(),
            text: "source-only queue".into(),
            created_at: DateTime::from_timestamp(6, 0).unwrap(),
        }];
        source
            .work_state
            .apply_model_mutation(crate::work::WorkMutation::Propose {
                body: None,
                expected_revision: 0,
                title: "Branch work".into(),
                tasks: vec!["One".into()],
            })
            .unwrap();
        store.save(&mut source).unwrap();
        let original = source.messages.clone();

        let historical = store.branch(&source, 2, 1, "Historical").unwrap();
        let full = store.branch(&source, 4, 2, "Full").unwrap();

        assert_eq!(historical.messages.len(), 2);
        assert_eq!(full.messages.len(), 4);
        assert_eq!(historical.workspace_roots, source.workspace_roots);
        assert_eq!(full.workspace_roots, source.workspace_roots);
        assert_eq!(historical.active_skills, source.active_skills);
        assert_eq!(full.active_skills, source.active_skills);
        assert!(historical.agent_summaries.is_empty());
        assert!(full.agent_summaries.is_empty());
        assert!(!historical.pinned);
        assert!(!full.pinned);
        assert!(historical.queued_inputs.is_empty());
        assert!(full.queued_inputs.is_empty());
        assert!(historical.review_records.is_empty());
        assert_eq!(full.review_records, source.review_records);
        assert!(historical.work_state.plan.is_none());
        assert!(historical.work_state.historical_notice.is_some());
        assert_ne!(
            full.work_state.plan.as_ref().unwrap().id,
            source.work_state.plan.as_ref().unwrap().id
        );
        assert_ne!(
            full.work_state.plan.as_ref().unwrap().tasks[0].id,
            source.work_state.plan.as_ref().unwrap().tasks[0].id
        );
        assert_eq!(source.messages.len(), original.len());
        assert_eq!(source.messages[3].content, original[3].content);
        assert_ne!(historical.id, source.id);
        assert_ne!(full.id, source.id);
        assert_eq!(historical.parent.as_ref().unwrap().session_id, source.id);
        let restored_source = store.load(&source.id.to_string()).unwrap();
        assert_eq!(restored_source.messages.len(), 4);
        assert!(restored_source.pinned);
        assert_eq!(restored_source.queued_inputs, source.queued_inputs);
    }

    #[test]
    fn timeline_boundaries_end_at_completed_assistant_exchanges() {
        let messages = vec![
            message(Role::User, "first question", 1),
            message(Role::Assistant, "first answer", 2),
            message(Role::User, "second question", 3),
            Message {
                role: Role::Assistant,
                content: "calling tool".into(),
                tool_calls: Some(vec![ToolCall {
                    id: "call-1".into(),
                    name: "read_file".into(),
                    arguments: "{}".into(),
                    status: ToolStatus::Done,
                    output: Some("ok".into()),
                }]),
                thinking: None,
                timestamp: DateTime::from_timestamp(4, 0).unwrap(),
                response_items: Vec::new(),
            },
            message(Role::Assistant, "second answer", 5),
        ];

        let turns = completed_turns(&messages);
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[0].through_message_count, 2);
        assert_eq!(turns[1].through_message_count, 5);
    }

    #[test]
    fn legacy_v4_without_new_optional_state_defaults_safely() {
        let mut value = serde_json::to_value(test_session()).unwrap();
        value.as_object_mut().unwrap().remove("goal_state");
        value.as_object_mut().unwrap().remove("workflow_run");
        value
            .as_object_mut()
            .unwrap()
            .remove("transcript_annotations");
        value
            .as_object_mut()
            .unwrap()
            .remove("transcript_agent_summaries");
        value.as_object_mut().unwrap().remove("manual_rule_ids");
        let loaded: SessionV2 = serde_json::from_value(value).unwrap();
        assert!(loaded.goal_state.is_none());
        assert!(loaded.workflow_run.is_none());
        assert!(loaded.transcript_annotations.is_empty());
        assert!(loaded.transcript_agent_summaries.is_empty());
        assert!(loaded.manual_rule_ids.is_empty());
    }

    #[test]
    fn saved_manual_rule_ids_are_bounded_sorted_and_safe() {
        let mut value = serde_json::to_value(test_session()).unwrap();
        value["manual_rule_ids"] = serde_json::json!(["workspace-0/z", "user/a"]);
        assert!(serde_json::from_value::<SessionV2>(value.clone()).is_err());
        value["manual_rule_ids"] = serde_json::json!(["user/a", "user/a"]);
        assert!(serde_json::from_value::<SessionV2>(value.clone()).is_err());
        value["manual_rule_ids"] = serde_json::json!(["user/../escape"]);
        assert!(serde_json::from_value::<SessionV2>(value).is_err());
    }

    #[test]
    fn transcript_turn_metadata_is_optional_and_round_trips_when_present() {
        let mut source = test_session();
        let turn_id = Uuid::parse_str("41414141-4141-4141-8141-414141414141").unwrap();
        source.transcript_annotations = vec![crate::app::TranscriptMessageAnnotation {
            message_timestamp: source.messages[1].timestamp,
            turn_id,
            reasoning_duration_ms: Some(651),
            provider: Some("mimo".into()),
            model: Some("mimo-v2.5-pro".into()),
            total_turn_duration_ms: Some(1_400),
            terminal_outcome: Some(crate::app::TranscriptTurnOutcome::Cancelled),
        }];
        let restored: SessionV2 =
            serde_json::from_value(serde_json::to_value(&source).unwrap()).unwrap();
        assert_eq!(
            restored.transcript_annotations,
            source.transcript_annotations
        );

        let mut legacy = serde_json::to_value(source).unwrap();
        let annotation = legacy
            .get_mut("transcript_annotations")
            .and_then(serde_json::Value::as_array_mut)
            .and_then(|annotations| annotations.first_mut())
            .and_then(serde_json::Value::as_object_mut)
            .unwrap();
        annotation.remove("provider");
        annotation.remove("model");
        annotation.remove("total_turn_duration_ms");
        annotation.remove("terminal_outcome");
        let restored: SessionV2 = serde_json::from_value(legacy).unwrap();
        let annotation = &restored.transcript_annotations[0];
        assert_eq!(annotation.provider, None);
        assert_eq!(annotation.model, None);
        assert_eq!(annotation.total_turn_duration_ms, None);
        assert_eq!(annotation.terminal_outcome, None);
        assert_eq!(annotation.reasoning_duration_ms, Some(651));
    }

    #[test]
    fn full_branch_forks_goal_identity_and_historical_branch_omits_goal() {
        let mut source = test_session();
        source.goal_state = Some(
            crate::goal::GoalState::start(
                "finish safely".into(),
                crate::goal::GoalLimits::from_config(crate::goal::GoalConfig::default()).unwrap(),
                Utc::now(),
            )
            .unwrap(),
        );
        let original_id = source.goal_state.as_ref().unwrap().id;
        let full = branch_in_memory(&source, source.messages.len(), 1, "full branch").unwrap();
        let historical = branch_in_memory(&source, 0, 0, "historical branch").unwrap();
        let forked = full.goal_state.unwrap();
        assert_ne!(forked.id, original_id);
        assert_eq!(forked.source_goal_id, Some(original_id));
        assert!(historical.goal_state.is_none());
    }

    #[test]
    fn full_branch_forks_workflow_identity_and_historical_branch_omits_workflow() {
        let loaded = crate::workflow::LoadedWorkflow {
            definition: crate::workflow::WorkflowDefinition {
                version: 1,
                id: "demo".into(),
                title: "Demo".into(),
                steps: vec![crate::workflow::WorkflowStep {
                    id: "one".into(),
                    depends_on: Vec::new(),
                    prompt: "Do one thing".into(),
                    model: None,
                    tools: None,
                    grants: Vec::new(),
                    retry: crate::workflow::WorkflowRetryPolicy::default(),
                    output: None,
                    evidence: vec![crate::workflow::WorkflowEvidenceGate::AssistantOutput],
                }],
            }
            .validate()
            .unwrap(),
            relative_path: "workflow.toml".into(),
            digest: "digest".into(),
        };
        let mut source = test_session();
        source.workflow_run = Some(crate::workflow::WorkflowRun::start(
            &loaded,
            "mimo".into(),
            "mimo-v2.5".into(),
            Utc::now(),
        ));
        let original_id = source.workflow_run.as_ref().unwrap().id;
        let full = branch_in_memory(&source, source.messages.len(), 1, "full branch").unwrap();
        let historical = branch_in_memory(&source, 0, 0, "historical branch").unwrap();
        let forked = full.workflow_run.unwrap();
        assert_ne!(forked.id, original_id);
        assert_eq!(forked.source_run_id, Some(original_id));
        assert_eq!(forked.status, crate::workflow::WorkflowRunStatus::Paused);
        assert!(historical.workflow_run.is_none());
    }

    #[test]
    fn worktree_attachment_is_defaulted_and_preserved_across_branches() {
        let id = Uuid::new_v4();
        let mut source = test_session();
        source.attached_worktree = Some(crate::worktree::WorktreeAttachment { id });

        let full = branch_in_memory(&source, source.messages.len(), 1, "full").unwrap();
        let historical = branch_in_memory(&source, 0, 0, "historical").unwrap();

        assert_eq!(
            full.attached_worktree.as_ref().map(|value| value.id),
            Some(id)
        );
        assert_eq!(
            historical.attached_worktree.as_ref().map(|value| value.id),
            Some(id)
        );
    }

    #[test]
    fn artifact_ledger_defaults_and_respects_branch_message_boundary() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let workspace =
            crate::workspace::WorkspaceContext::from_primary(root.clone(), root.clone());
        let mut source = test_session();
        for (name, boundary) in [("first.txt", 1), ("second.txt", 2)] {
            let receipt = crate::undo::FileMutationBatchReceipt::single(
                crate::undo::FileMutationReceipt::new(
                    root.join(name),
                    "write_file",
                    crate::undo::FileState::Missing,
                    crate::undo::FileState::Utf8(name.into()),
                ),
            );
            source
                .artifacts
                .record_mutation_batch(&workspace, &receipt, boundary, Utc::now());
        }

        let full = branch_in_memory(&source, source.messages.len(), 1, "full").unwrap();
        let historical = branch_in_memory(&source, 1, 0, "historical").unwrap();
        assert_eq!(full.artifacts.entries().len(), 2);
        assert_eq!(historical.artifacts.entries().len(), 1);
        assert_eq!(historical.artifacts.entries()[0].relative_path, "first.txt");

        let mut legacy = serde_json::to_value(test_session()).unwrap();
        legacy.as_object_mut().unwrap().remove("artifacts");
        let restored: SessionV2 = serde_json::from_value(legacy).unwrap();
        assert!(restored.artifacts.is_empty());
    }

    #[test]
    fn fallback_provenance_round_trips_and_branches_only_with_its_user_turn() {
        let temp = TempDir::new().unwrap();
        let store = SessionStore::new(temp.path());
        let mut source = SessionV2::new(
            "Fallback",
            "/workspace",
            "primary",
            "one",
            vec![
                message(Role::User, "first", 1),
                message(Role::Assistant, "answer", 2),
                message(Role::User, "second", 3),
                message(Role::Assistant, "answer", 4),
            ],
        );
        source.fallback_provenance = vec![
            crate::fallback::TurnProviderProvenance {
                user_message_index: 0,
                primary_provider: "primary".into(),
                primary_model: "one".into(),
                provider: "backup".into(),
                model: "two".into(),
                cause: crate::fallback::ProviderFallbackCause::Timeout,
            },
            crate::fallback::TurnProviderProvenance {
                user_message_index: 2,
                primary_provider: "primary".into(),
                primary_model: "one".into(),
                provider: "backup".into(),
                model: "two".into(),
                cause: crate::fallback::ProviderFallbackCause::RateLimited,
            },
        ];
        store.save(&mut source).unwrap();
        let loaded = store.load(&source.id.to_string()).unwrap();
        assert_eq!(loaded.fallback_provenance, source.fallback_provenance);

        let first_turn = branch_in_memory(&loaded, 2, 1, "first").unwrap();
        assert_eq!(first_turn.fallback_provenance.len(), 1);
        assert_eq!(first_turn.fallback_provenance[0].user_message_index, 0);
        let before_first = branch_in_memory(&loaded, 0, 0, "empty").unwrap();
        assert!(before_first.fallback_provenance.is_empty());
    }

    #[test]
    fn transcript_activity_branches_only_with_its_anchoring_turn() {
        let first_turn = Uuid::parse_str("11111111-1111-4111-8111-111111111111").unwrap();
        let second_turn = Uuid::parse_str("22222222-2222-4222-8222-222222222222").unwrap();
        let mut source = SessionV2::new(
            "Activity",
            "/workspace",
            "provider",
            "model",
            vec![
                message(Role::User, "first", 1),
                message(Role::Assistant, "answer one", 2),
                message(Role::User, "second", 3),
                message(Role::Assistant, "answer two", 4),
            ],
        );
        source.transcript_annotations = vec![
            crate::app::TranscriptMessageAnnotation {
                message_timestamp: source.messages[1].timestamp,
                turn_id: first_turn,
                reasoning_duration_ms: Some(10),
                provider: None,
                model: None,
                total_turn_duration_ms: None,
                terminal_outcome: None,
            },
            crate::app::TranscriptMessageAnnotation {
                message_timestamp: source.messages[3].timestamp,
                turn_id: second_turn,
                reasoning_duration_ms: Some(20),
                provider: None,
                model: None,
                total_turn_duration_ms: None,
                terminal_outcome: None,
            },
        ];
        source.transcript_agent_summaries = vec![
            crate::app::TranscriptAgentSummary {
                agent_id: crate::agent::subagent::AgentId::new(),
                turn_id: first_turn,
                purpose: crate::agent::subagent::AgentRunPurpose::Subagent,
                task: "first task".into(),
                role: None,
                model: None,
                provider: None,
                created_at: None,
                status: crate::agent::subagent::AgentStatus::Completed,
                summary: "first summary".into(),
                updated_at: DateTime::from_timestamp(5, 0).unwrap(),
            },
            crate::app::TranscriptAgentSummary {
                agent_id: crate::agent::subagent::AgentId::new(),
                turn_id: second_turn,
                purpose: crate::agent::subagent::AgentRunPurpose::Subagent,
                task: "second task".into(),
                role: None,
                model: None,
                provider: None,
                created_at: None,
                status: crate::agent::subagent::AgentStatus::Completed,
                summary: "second summary".into(),
                updated_at: DateTime::from_timestamp(6, 0).unwrap(),
            },
        ];

        let branch = branch_in_memory(&source, 2, 1, "first only").unwrap();

        assert_eq!(branch.transcript_annotations.len(), 1);
        assert_eq!(branch.transcript_annotations[0].turn_id, first_turn);
        assert_eq!(branch.transcript_agent_summaries.len(), 1);
        assert_eq!(branch.transcript_agent_summaries[0].turn_id, first_turn);
    }

    #[test]
    fn named_profile_session_store_is_isolated_from_default_sessions() {
        let home = TempDir::new().unwrap();
        let default_paths = crate::runtime::RuntimePaths::from_home(home.path());
        let named_paths = default_paths
            .clone()
            .for_profile(&crate::runtime::ActiveProfile::Named("work".into()));
        let default_store = SessionStore::new(&default_paths.sessions_dir);
        let named_store = SessionStore::new(&named_paths.sessions_dir);
        let mut default_session = test_session();
        let mut named_session = test_session();

        default_store.save(&mut default_session).unwrap();
        named_store.save(&mut named_session).unwrap();

        assert_eq!(default_store.list().unwrap()[0].id, default_session.id);
        assert_eq!(named_store.list().unwrap()[0].id, named_session.id);
        assert_ne!(default_paths.sessions_dir, named_paths.sessions_dir);
    }

    #[test]
    fn mechanistic_checkpoint_is_optional_validated_and_branch_scoped() {
        let mut source = test_session();
        source.mechanistic_checkpoint = Some(MechanisticCheckpointV1 {
            schema: MECHANISTIC_CHECKPOINT_SCHEMA.into(),
            job_id: "job_123".into(),
            expected_revision: "a".repeat(64),
            phase: MechanisticCheckpointPhaseV1::VerificationRunning,
            request_digest: "b".repeat(64),
            receipt_digest: None,
            remaining_repair_passes: 2,
            server: "codex_team_delegate".into(),
            profile: "work".into(),
            changed_paths: vec!["src/app.rs".into(), "src/main.rs".into()],
        });
        source
            .mechanistic_checkpoint
            .as_ref()
            .unwrap()
            .validate(2)
            .unwrap();

        let encoded = serde_json::to_value(&source).unwrap();
        let decoded: SessionV2 = serde_json::from_value(encoded).unwrap();
        assert_eq!(
            decoded.mechanistic_checkpoint,
            source.mechanistic_checkpoint
        );

        let full = branch_in_memory(
            &source,
            source.messages.len(),
            source.messages.len(),
            "full",
        )
        .unwrap();
        assert_eq!(full.mechanistic_checkpoint, source.mechanistic_checkpoint);
        let historical = branch_in_memory(&source, 0, 0, "historical").unwrap();
        assert!(historical.mechanistic_checkpoint.is_none());
    }

    #[test]
    fn mechanistic_capsule_index_is_optional_bounded_and_full_branch_only() {
        let mut source = test_session();
        let empty = serde_json::to_value(&source).unwrap();
        assert!(empty.get("mechanistic_capsules").is_none());
        source
            .mechanistic_capsules
            .entries
            .push(MechanisticCapsuleIndexEntryV1 {
                capsule_digest: "a".repeat(64),
                revision: "b".repeat(64),
                call_ids: vec!["call_1".into()],
                action_mapping: vec!["read_file".into()],
                consumption_boundary: Some(source.messages.len()),
            });
        source
            .mechanistic_capsules
            .retain_valid(source.messages.len());
        assert_eq!(source.mechanistic_capsules.entries.len(), 1);
        assert!(serde_json::to_value(&source).unwrap()["mechanistic_capsules"].is_object());

        let full = branch_in_memory(
            &source,
            source.messages.len(),
            completed_turns(&source.messages).len(),
            "full",
        )
        .unwrap();
        assert_eq!(full.mechanistic_capsules, source.mechanistic_capsules);
        let historical = branch_in_memory(&source, 0, 0, "historical").unwrap();
        assert!(historical.mechanistic_capsules.entries.is_empty());

        source.mechanistic_capsules.entries[0].call_ids.clear();
        source
            .mechanistic_capsules
            .retain_valid(source.messages.len());
        assert!(source.mechanistic_capsules.entries.is_empty());

        source
            .mechanistic_capsules
            .entries
            .push(MechanisticCapsuleIndexEntryV1 {
                capsule_digest: "a".repeat(40),
                revision: "b".repeat(64),
                call_ids: vec!["call_1".into()],
                action_mapping: vec!["read_file".into()],
                consumption_boundary: None,
            });
        source
            .mechanistic_capsules
            .retain_valid(source.messages.len());
        assert!(source.mechanistic_capsules.entries.is_empty());
    }

    #[test]
    fn mechanistic_checkpoint_rejects_budget_path_and_identity_escalation() {
        let mut checkpoint = MechanisticCheckpointV1 {
            schema: MECHANISTIC_CHECKPOINT_SCHEMA.into(),
            job_id: "job_123".into(),
            expected_revision: "a".repeat(64),
            phase: MechanisticCheckpointPhaseV1::RepairPending,
            request_digest: "b".repeat(64),
            receipt_digest: Some("c".repeat(64)),
            remaining_repair_passes: 3,
            server: "codex_team_delegate".into(),
            profile: "work".into(),
            changed_paths: vec!["src/main.rs".into()],
        };
        assert!(checkpoint.validate(2).is_err());
        checkpoint.remaining_repair_passes = 1;
        checkpoint.changed_paths = vec!["../outside".into()];
        assert!(checkpoint.validate(2).is_err());
        checkpoint.changed_paths = vec!["src/main.rs".into()];
        checkpoint.server = "https://remote.example".into();
        assert!(checkpoint.validate(2).is_err());
    }

    #[test]
    fn malformed_optional_mechanistic_checkpoint_blocks_without_losing_the_session() {
        let session = test_session();
        let mut value = serde_json::to_value(session).unwrap();
        value["mechanistic_checkpoint"] = serde_json::json!({
            "schema": "unknown",
            "job_id": ["not", "a", "string"]
        });
        let decoded: SessionV2 = serde_json::from_value(value).unwrap();
        let checkpoint = decoded.mechanistic_checkpoint.unwrap();
        assert_eq!(checkpoint.phase, MechanisticCheckpointPhaseV1::Blocked);
        assert!(checkpoint.validate(2).is_ok());
        assert_eq!(checkpoint.profile, "invalid");
    }

    #[test]
    fn context_usage_anchor_records_cooldown_ladder() {
        let mut session = SessionV2::new("t", "/w", "p", "m", vec![]);
        let now = Utc::now();
        session.record_context_usage_success(100, 40, now);
        let anchor = session.context_usage_anchor.clone().unwrap();
        assert_eq!(anchor.provider_input_tokens, 100);
        assert_eq!(anchor.checkpoint_failure_streak, 0);
        assert!(anchor.allows_attempt(now));
        session.record_context_checkpoint_failure(now);
        let a1 = session.context_usage_anchor.clone().unwrap();
        assert_eq!(a1.checkpoint_failure_streak, 1);
        assert!(!a1.allows_attempt(now));
        assert!(a1.allows_attempt(now + chrono::Duration::seconds(60)));
        session.record_context_checkpoint_failure(now);
        session.record_context_checkpoint_failure(now);
        let a3 = session.context_usage_anchor.clone().unwrap();
        assert_eq!(a3.checkpoint_failure_streak, 3);
        assert_eq!(ContextUsageAnchor::cooldown_seconds(3), 900);
        assert!(!a3.allows_attempt(now + chrono::Duration::seconds(300)));
        assert!(a3.allows_attempt(now + chrono::Duration::seconds(900)));
    }

    #[test]
    fn context_content_fingerprint_changes_with_message_content() {
        let a = context_content_fingerprint(&[message(Role::User, "one", 1)]);
        let b = context_content_fingerprint(&[message(Role::User, "two", 1)]);
        assert_ne!(a, b);
        assert_eq!(
            a,
            context_content_fingerprint(&[message(Role::User, "one", 1)])
        );
    }
    #[test]
    fn cold_unrelated_legacy_history_refuses_unprepared_short_identity_scan_without_writing() {
        let root = TempDir::new().unwrap();
        let mut store = SessionStore::new(root.path());
        let mut target = test_session();
        let target_path = store.save(&mut target).unwrap();
        let mut unrelated = test_session();
        unrelated.messages = vec![message(Role::User, &"x".repeat(128 * 1024), 1)];
        let unrelated_path = root.path().join("unrelated-large-legacy.json");
        std::fs::write(&unrelated_path, serde_json::to_vec(&unrelated).unwrap()).unwrap();
        let before = std::fs::read(&target_path).unwrap();
        store.identity_scan_limits.max_uncached_bytes = 16 * 1024;
        store.identity_scan_limits.timeout = std::time::Duration::from_secs(2);
        let lease = store.try_writer_lease(target.id).unwrap();
        target.title = "Must remain uncommitted".into();
        assert!(store.save_owned(&mut target, &lease).is_err());
        assert_eq!(std::fs::read(&target_path).unwrap(), before);
        assert!(std::fs::read(&unrelated_path).unwrap().len() > 128 * 1024);
    }

    #[test]
    fn identity_inventory_candidate_bound_refuses_metadata_write_without_hiding_family_rows() {
        let root = TempDir::new().unwrap();
        let mut store = SessionStore::new(root.path());
        let mut target = test_session();
        let target_path = store.save(&mut target).unwrap();
        for index in 0..8 {
            let session = test_session();
            std::fs::write(
                root.path().join(format!("unrelated-{index}.json")),
                serde_json::to_vec(&session).unwrap(),
            )
            .unwrap();
        }
        let before = std::fs::read(&target_path).unwrap();
        store.identity_scan_limits.max_candidates = 4;
        store.identity_scan_limits.timeout = std::time::Duration::from_secs(2);
        assert!(store
            .rename(&target.id.to_string(), target.id, "Uncommitted", false)
            .is_err());
        assert_eq!(std::fs::read(&target_path).unwrap(), before);
        assert_eq!(store.list().unwrap().len(), 9);
    }

    #[test]
    fn identity_inventory_aggregate_probe_bytes_refuse_write_before_partial_proof_is_trusted() {
        let root = TempDir::new().unwrap();
        let mut store = SessionStore::new(root.path());
        let mut target = test_session();
        let target_path = store.save(&mut target).unwrap();
        for index in 0..3 {
            let mut session = test_session();
            session.messages = vec![message(Role::User, &"x".repeat(12 * 1024), 1)];
            std::fs::write(
                root.path().join(format!("aggregate-{index}.json")),
                serde_json::to_vec(&session).unwrap(),
            )
            .unwrap();
        }
        let before = std::fs::read(&target_path).unwrap();
        store.identity_scan_limits.max_uncached_bytes = 20 * 1024;
        store.identity_scan_limits.timeout = std::time::Duration::from_secs(2);
        assert!(store
            .set_pinned(&target.id.to_string(), target.id, true)
            .is_err());
        assert_eq!(std::fs::read(&target_path).unwrap(), before);
    }

    #[cfg(unix)]
    #[test]
    fn warmed_identity_inventory_skips_unrelated_legacy_history_and_preserves_its_bytes() {
        let root = TempDir::new().unwrap();
        let mut store = SessionStore::new(root.path());
        let mut target = test_session();
        store.save(&mut target).unwrap();
        let mut unrelated = test_session();
        unrelated.messages = vec![message(Role::User, &"x".repeat(256 * 1024), 1)];
        let raw = serde_json::to_vec(&unrelated).unwrap();
        let path = root.path().join("unrelated-large-legacy.json");
        std::fs::write(&path, &raw).unwrap();
        store.prepare_identity_inventory().unwrap();
        store.identity_scan_limits.max_uncached_bytes = 0;
        store.identity_scan_limits.timeout = std::time::Duration::from_secs(2);
        let lease = store.try_writer_lease(target.id).unwrap();
        assert_eq!(
            store.committed_binding(target.id).unwrap().session_id,
            target.id
        );
        target.title = "Committed after worker preparation".into();
        store.save_owned(&mut target, &lease).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), raw);
    }

    #[cfg(unix)]
    #[test]
    fn identity_probe_cache_rejects_same_length_rewrite_even_with_restored_mtime() {
        let root = TempDir::new().unwrap();
        let store = SessionStore::new(root.path());
        let mut target = test_session();
        let target_path = store.save(&mut target).unwrap();
        let unrelated = test_session();
        let path = root.path().join("unrelated-legacy.json");
        let raw = serde_json::to_string(&unrelated).unwrap();
        std::fs::write(&path, &raw).unwrap();
        store.prepare_identity_inventory().unwrap();
        let stamp = std::fs::metadata(&path).unwrap();
        let replacement = raw.replacen(&unrelated.id.to_string(), &target.id.to_string(), 1);
        assert_eq!(replacement.len(), raw.len());
        std::fs::write(&path, replacement).unwrap();
        File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(stamp.modified().unwrap()))
            .unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().modified().unwrap(),
            stamp.modified().unwrap()
        );
        let before = std::fs::read(&target_path).unwrap();
        target.title = "Must not overwrite duplicate".into();
        let error = store.save(&mut target).unwrap_err();
        assert!(error.to_string().contains("multiple locations"));
        assert_eq!(std::fs::read(&target_path).unwrap(), before);
    }

    #[cfg(unix)]
    #[test]
    fn identity_probe_cache_rejects_atomic_replacement_with_preserved_length_and_mtime() {
        let root = TempDir::new().unwrap();
        let store = SessionStore::new(root.path());
        let mut target = test_session();
        let target_path = store.save(&mut target).unwrap();
        let unrelated = test_session();
        let path = root.path().join("unrelated-legacy.json");
        let raw = serde_json::to_string(&unrelated).unwrap();
        std::fs::write(&path, &raw).unwrap();
        store.prepare_identity_inventory().unwrap();
        let stamp = std::fs::metadata(&path).unwrap();
        let replacement = root.path().join("replacement.tmp");
        std::fs::write(
            &replacement,
            raw.replacen(&unrelated.id.to_string(), &target.id.to_string(), 1),
        )
        .unwrap();
        File::options()
            .write(true)
            .open(&replacement)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(stamp.modified().unwrap()))
            .unwrap();
        std::fs::rename(&replacement, &path).unwrap();
        let before = std::fs::read(&target_path).unwrap();
        assert!(store
            .rename(&target.id.to_string(), target.id, "Must not commit", false)
            .unwrap_err()
            .to_string()
            .contains("multiple locations"));
        assert_eq!(std::fs::read(&target_path).unwrap(), before);
    }

    #[test]
    fn warmed_identity_probe_does_not_cache_target_optional_metadata_or_history_validity() {
        let root = TempDir::new().unwrap();
        let store = SessionStore::new(root.path());
        let mut target = test_session();
        let target_path = store.save(&mut target).unwrap();
        store.prepare_identity_inventory().unwrap();
        let before = std::fs::read(&target_path).unwrap();
        let mut metadata: SessionBundleMetadataV1 = serde_json::from_slice(&before).unwrap();
        let history = bundle_log_path(&target_path, &metadata);
        let history_bytes = std::fs::read(&history).unwrap();
        std::fs::write(&history, []).unwrap();
        assert!(store
            .rename(&target.id.to_string(), target.id, "Invalid history", false)
            .is_err());
        assert_eq!(std::fs::read(&target_path).unwrap(), before);
        std::fs::write(&history, history_bytes).unwrap();
        metadata.session["work_state"]["plan_feedback"] =
            serde_json::json!({"notes": "not-an-array"});
        let malformed = serde_json::to_vec(&metadata).unwrap();
        std::fs::write(&target_path, &malformed).unwrap();
        store.prepare_identity_inventory().unwrap();
        assert!(store
            .set_pinned(&target.id.to_string(), target.id, true)
            .is_err());
        assert_eq!(std::fs::read(&target_path).unwrap(), malformed);
    }

    #[test]
    fn identity_scan_deadline_and_oversize_unrelated_file_refuse_incomplete_proof() {
        let root = TempDir::new().unwrap();
        let mut store = SessionStore::new(root.path());
        let mut target = test_session();
        let target_path = store.save(&mut target).unwrap();
        store.prepare_identity_inventory().unwrap();
        let before = std::fs::read(&target_path).unwrap();
        store.identity_scan_limits.timeout = std::time::Duration::ZERO;
        assert_eq!(
            store
                .set_pinned(&target.id.to_string(), target.id, true)
                .unwrap_err()
                .kind(),
            io::ErrorKind::WouldBlock
        );
        let mut reader = IdentityDeadlineReader {
            inner: std::io::Cursor::new(b"ignored"),
            deadline: std::time::Instant::now(),
        };
        assert_eq!(
            reader.read(&mut [0; 1]).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        let oversized = File::create(root.path().join("unrelated-oversized.json")).unwrap();
        oversized
            .set_len(MAX_SESSION_HISTORY_BYTES + MAX_SESSION_METADATA_BYTES + 1)
            .unwrap();
        assert_eq!(
            store.prepare_identity_inventory().unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        assert_eq!(std::fs::read(&target_path).unwrap(), before);
    }

    #[cfg(unix)]
    #[test]
    fn identity_inventory_named_pipe_never_blocks_the_guard() {
        const FILTER: &str = "session::tests::identity_inventory_named_pipe_never_blocks_the_guard";
        const FIXTURE: &str = "CA_SESSION_IDENTITY_FIFO_FIXTURE";
        let id = Uuid::from_u128(0x4f49464f);
        if let Some(root) = std::env::var_os(FIXTURE) {
            let store = SessionStore::new(root);
            assert!(store.committed_binding(id).is_err());
            return;
        }
        let root = TempDir::new().unwrap();
        let path = root.path().join(format!("{id}.json"));
        use std::os::unix::ffi::OsStrExt;
        use std::os::unix::fs::FileTypeExt;
        let name = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([FILTER, "--exact"])
            .env(FIXTURE, root.path())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(status.success());
                break;
            }
            if std::time::Instant::now() >= deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("identity guard blocked opening a named pipe");
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(std::fs::symlink_metadata(path)
            .unwrap()
            .file_type()
            .is_fifo());
    }
}
