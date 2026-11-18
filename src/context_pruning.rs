//! Request-only reduction of host-qualified tool results. Canonical history is never edited.
use crate::agent::client::{ChatMessage, ToolCallMsg};
use crate::app::{ToolCall, ToolStatus};
use crate::config::CaConfig;
use crate::request::RequestKind;
use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::collections::{HashMap, HashSet};
use std::sync::{OnceLock, RwLock};

// The qualification control ablates this one semantic feature boundary.
const SEMANTIC_EVIDENCE: bool = true;

fn coding_request(kind: RequestKind) -> bool {
    matches!(
        kind,
        RequestKind::MainTurn
            | RequestKind::ToolContinuation
            | RequestKind::Headless
            | RequestKind::Service
            | RequestKind::Subagent
            | RequestKind::Scheduled
            | RequestKind::WorkflowStep
            | RequestKind::RoleRun
            | RequestKind::MultiRunCandidate
    )
}

/// Fixed first-party request instruction; privacy recognizes only this exact
/// text in an actual system-instruction envelope, never in user/source data.
pub(crate) const EVIDENCE_GUIDANCE: &str = "Reuse retained reads and citations unless the task requests a reread or the evidence is missing, stale, or uncertain. For new reads, use the smallest relevant range. Omit start_line when it is 1; use end_line=1 for first-line facts. Expand when needed and honor explicit whole-file requests. State requested facts, citations, and unresolved conditions directly. Effects of isolated commands stay in their copies. For tasks that only ask to run, inspect, and report, leave originals unchanged and describe copy effects separately. Do not mirror copy changes into originals, including with edit_file, unless applying them is part of the task. A preserve-other-bytes constraint does not itself request an edit. Use original evidence for original-state claims and inspect only relevant copy files for copy-state claims.";

pub(crate) fn evidence_guidance(kind: RequestKind) -> Option<&'static str> {
    (SEMANTIC_EVIDENCE
        && coding_request(kind)
        && !policy().read().unwrap_or_else(|p| p.into_inner()).disabled)
        .then_some(EVIDENCE_GUIDANCE)
}

pub(crate) fn fixed_evidence_instruction(role: &str, text: Option<&str>) -> bool {
    role == "system" && text == Some(EVIDENCE_GUIDANCE)
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct PruningConfig {
    pub enabled: bool,
}
impl PruningConfig {
    pub fn is_default(&self) -> bool {
        self.enabled
    }
}
impl Default for PruningConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PruningStats {
    pub results: usize,
    pub saved_tokens: u64,
    pub added_tokens: u64,
}
impl PruningStats {
    pub fn apply(self, breakdown: &mut crate::context::ContextBreakdown) {
        breakdown.conversation = breakdown
            .conversation
            .saturating_add(self.added_tokens)
            .saturating_sub(self.saved_tokens);
        breakdown.total = breakdown
            .total
            .saturating_add(self.added_tokens)
            .saturating_sub(self.saved_tokens);
    }
}
#[derive(Default)]
struct RuntimePolicy {
    disabled: bool,
    windows: HashMap<String, u64>,
}
fn policy() -> &'static RwLock<RuntimePolicy> {
    static POLICY: OnceLock<RwLock<RuntimePolicy>> = OnceLock::new();
    POLICY.get_or_init(|| RwLock::new(RuntimePolicy::default()))
}
pub fn install(config: &CaConfig) {
    let mut windows = config
        .model_context
        .iter()
        .filter_map(|(key, value)| {
            crate::context::automatic_threshold(
                value.context_window_tokens,
                value.compact_at_tokens,
            )
            .map(|n| (key.clone(), n))
        })
        .collect::<HashMap<_, _>>();
    for (key, value) in &config.model_profiles {
        if let Some(n) = crate::context::automatic_threshold(
            value.context_window_tokens,
            value.compact_at_tokens,
        ) {
            windows.insert(key.clone(), n);
        }
    }
    for (provider_id, provider) in &config.providers {
        for model in &provider.models {
            if let Some(window) = config.context_for_model(provider_id, model).and_then(|c| {
                crate::context::automatic_threshold(c.context_window_tokens, c.compact_at_tokens)
            }) {
                windows.insert(format!("{provider_id}/{model}"), window);
            }
        }
    }
    *policy().write().unwrap_or_else(|p| p.into_inner()) = RuntimePolicy {
        disabled: !config.context_pruning.enabled,
        windows,
    };
}
pub fn project_request(
    messages: &mut [ChatMessage],
    kind: RequestKind,
    provider: &str,
    model: &str,
) -> PruningStats {
    if !coding_request(kind) {
        return PruningStats::default();
    }
    let settings = policy().read().unwrap_or_else(|p| p.into_inner());
    let window = settings
        .windows
        .get(&format!("{provider}/{model}"))
        .or_else(|| settings.windows.get(model))
        .copied();
    let disabled = settings.disabled;
    drop(settings);
    let cold = crate::prompt_cache_warm::temperature(provider, model)
        == crate::prompt_cache_warm::Temperature::Cold;
    let mut stats = if cold {
        offload_cold_tool_bodies(messages, &cold_spill_dir(), true)
    } else {
        PruningStats::default()
    };
    let pruned = project(messages, !disabled, window);
    stats.results += pruned.results;
    stats.saved_tokens += pruned.saved_tokens;
    stats.added_tokens += pruned.added_tokens;
    stats
}

const COLD_BODY_BYTES: usize = 1024;

fn cold_spill_dir() -> std::path::PathBuf {
    static DIR: OnceLock<std::path::PathBuf> = OnceLock::new();
    DIR.get_or_init(|| std::env::temp_dir().join("ca-cold-tool-bodies"))
        .clone()
}

/// When the route cache is already expired, replace old tool bodies on this
/// request copy with a marker and a path. A warm or unknown route is unchanged.
/// The latest tool batch stays. The stored transcript is not this slice.
pub fn offload_cold_tool_bodies(
    messages: &mut [ChatMessage],
    spill_dir: &std::path::Path,
    cold: bool,
) -> PruningStats {
    if !cold {
        return PruningStats::default();
    }
    let calls = paired(messages);
    let incomplete = messages
        .iter()
        .enumerate()
        .filter(|(_, message)| {
            message.tool_calls.as_ref().is_some_and(|batch| {
                batch
                    .iter()
                    .any(|call| !calls.contains_key(call.id.as_str()))
            })
        })
        .map(|(index, _)| index)
        .collect::<HashSet<_>>();
    let latest_batch = calls.values().map(|(_, index)| *index).max();
    let mut replacements = Vec::new();
    for (index, message) in messages.iter().enumerate() {
        if message.role != "tool" {
            continue;
        }
        let Some(text) = message.content.as_str() else {
            continue;
        };
        if text.len() < COLD_BODY_BYTES {
            continue;
        }
        let Some(id) = message.tool_call_id.as_deref() else {
            continue;
        };
        let Some((_, batch)) = calls.get(id) else {
            continue;
        };
        if Some(*batch) == latest_batch || incomplete.contains(batch) {
            continue;
        }
        let digest = format!("{:x}", sha2::Sha256::digest(text.as_bytes()));
        let path = spill_dir.join(format!("{digest}.txt"));
        if !path.exists()
            && (std::fs::create_dir_all(spill_dir).is_err() || std::fs::write(&path, text).is_err())
        {
            continue;
        }
        let marker = format!(
            "[Earlier tool result saved at {}. The cache for this route had expired, so this request keeps the path instead of the body.]",
            path.display()
        );
        let before = crate::context::estimate_text_tokens(text);
        let after = crate::context::estimate_text_tokens(&marker);
        replacements.push((index, marker, before, after));
    }
    let mut stats = PruningStats::default();
    for (index, marker, before, after) in replacements {
        messages[index].content = crate::image_content::ModelContent::context_marker(marker)
            .with_tool_result_status(true);
        stats.results += 1;
        stats.saved_tokens += before.saturating_sub(after);
        stats.added_tokens += after.saturating_sub(before);
    }
    stats
}

/// A graph worker has one human turn, so ordinary multi-turn age pruning cannot
/// retire its reads. Project only already-seen, successful read-only results;
/// keep the newest complete batch exact, including private source receipts.
pub(crate) fn project_graph_worker(
    canonical: &[ChatMessage],
    messages: &mut [ChatMessage],
    tools: &[crate::agent::client::ToolDef],
    remaining_tokens: u64,
    remaining_requests: u32,
) -> PruningStats {
    let enabled = !policy().read().unwrap_or_else(|p| p.into_inner()).disabled;
    project_worker(
        canonical,
        messages,
        tools,
        remaining_tokens,
        remaining_requests,
        enabled,
    )
}

fn project_worker(
    canonical: &[ChatMessage],
    messages: &mut [ChatMessage],
    tools: &[crate::agent::client::ToolDef],
    remaining_tokens: u64,
    remaining_requests: u32,
    enabled: bool,
) -> PruningStats {
    if !enabled {
        return PruningStats::default();
    }
    let target = remaining_tokens / u64::from(remaining_requests.max(1));
    let mut estimate = estimate_worker_request(messages, tools);
    let calls = paired(messages);
    let protected_bases = crate::context_observation::retained_basis_indices(canonical, messages);
    let incomplete = messages
        .iter()
        .enumerate()
        .filter(|(_, message)| {
            message.tool_calls.as_ref().is_some_and(|batch| {
                batch
                    .iter()
                    .any(|call| !calls.contains_key(call.id.as_str()))
            })
        })
        .map(|(index, _)| index)
        .collect::<HashSet<_>>();
    let latest_batch = calls.values().map(|(_, index)| *index).max();
    let mut replacements = Vec::new();
    for (index, message) in messages.iter().enumerate() {
        if estimate <= target {
            break;
        }
        if protected_bases.contains(&index)
            || message.role != "tool"
            || !message.content.is_prunable_tool_result()
            || message.content.tool_result_success() != Some(true)
        {
            continue;
        }
        let Some((call, batch)) = message.tool_call_id.as_deref().and_then(|id| calls.get(id))
        else {
            continue;
        };
        if Some(*batch) == latest_batch
            || incomplete.contains(batch)
            || !matches!(
                call.function.name.as_str(),
                "read_file" | "list_dir" | "search_files" | "glob"
            )
            || !qualified_result(&call.function.name, &call.function.arguments, true)
        {
            continue;
        }
        let Some(text) = message.content.as_str() else {
            continue;
        };
        let safe_id = call
            .id
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
            .take(128)
            .collect::<String>();
        // No source bytes or receipt are sliced/relabelled. The unchanged call
        // arguments retain the read/search location; canonical results remain
        // in the worker's local history, subject to its existing output limits.
        let marker = format!("[Earlier successful {} result from call {} omitted from this request to fit the shared graph token budget. Full result remains in this worker's local history. Use the original call arguments to repeat a narrow read/search if needed.]", call.function.name, safe_id);
        let before = crate::context::estimate_text_tokens(text);
        let after = crate::context::estimate_text_tokens(&marker);
        if before > after {
            estimate = estimate.saturating_sub(before - after);
            replacements.push((index, marker, before - after));
        }
    }
    let mut stats = PruningStats::default();
    for (index, marker, saved) in replacements {
        messages[index].content = crate::image_content::ModelContent::context_marker(marker)
            .with_tool_result_status(true);
        stats.results += 1;
        stats.saved_tokens += saved;
    }
    stats
}

pub(crate) fn estimate_worker_request(
    messages: &[ChatMessage],
    tools: &[crate::agent::client::ToolDef],
) -> u64 {
    messages.iter().fold(
        crate::context::estimate_serialized_tokens(&tools),
        |total, message| {
            total
                .saturating_add(8)
                .saturating_add(
                    message
                        .content
                        .as_str()
                        .map_or(0, crate::context::estimate_text_tokens),
                )
                .saturating_add(
                    message
                        .tool_calls
                        .as_ref()
                        .map_or(0, crate::context::estimate_serialized_tokens),
                )
        },
    )
}

/// Eligibility is host provenance, never inferred from tool-output strings.
pub fn qualified(call: &ToolCall) -> bool {
    qualified_result(
        &call.name,
        &call.arguments,
        call.status == ToolStatus::Done && call.output.is_some(),
    )
}
pub fn qualified_result(name: &str, arguments: &str, success: bool) -> bool {
    success
        && matches!(
            name,
            "read_file"
                | "list_dir"
                | "search_files"
                | "glob"
                | "run_command"
                | "git_status"
                | "git_diff"
                | "git_log"
                | "lsp_definition"
                | "lsp_references"
                | "lsp_hover"
                | "lsp_implementation"
                | "lsp_call_hierarchy"
        )
        && !arguments.to_ascii_lowercase().contains("skill.md")
        && !arguments.contains("skills/")
}

fn paired(messages: &[ChatMessage]) -> HashMap<&str, (&ToolCallMsg, usize)> {
    let mut calls = HashMap::new();
    let mut invalid = HashSet::new();
    let mut completed = HashSet::new();
    for (index, message) in messages.iter().enumerate() {
        if let Some(batch) = &message.tool_calls {
            for call in batch {
                if calls.insert(call.id.as_str(), (call, index)).is_some() {
                    invalid.insert(call.id.as_str());
                }
            }
        }
        if let Some(id) = message.tool_call_id.as_deref() {
            if !completed.insert(id) || !calls.contains_key(id) {
                invalid.insert(id);
            }
        }
    }
    calls.retain(|id, _| completed.contains(id) && !invalid.contains(id));
    calls
}

pub fn project(messages: &mut [ChatMessage], enabled: bool, capacity: Option<u64>) -> PruningStats {
    if !enabled {
        return PruningStats::default();
    }
    let mut evidence = crate::context_observation::project_deferred(messages, SEMANTIC_EVIDENCE);
    let keep_from = messages
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, m)| m.role == "user" && m.content.tool_result_success().is_none())
        .nth(1)
        .map_or(0, |(i, _)| i);
    let protected = capacity.map_or(40_000, |n| (n / 4).min(40_000));
    let minimum = capacity.map_or(20_000, |n| (n / 10).clamp(1, 20_000));
    let calls = paired(messages);
    // A partially completed tool batch is protected in its entirety.
    let incomplete_batches = messages
        .iter()
        .enumerate()
        .filter(|(_, m)| {
            m.tool_calls.as_ref().is_some_and(|batch| {
                batch
                    .iter()
                    .any(|call| !calls.contains_key(call.id.as_str()))
            })
        })
        .map(|(i, _)| i)
        .collect::<HashSet<_>>();
    let mut candidates = Vec::new();
    let mut retained = 0_u64;
    let mut savings = 0_u64;
    for (index, message) in messages.iter().enumerate().rev() {
        if message.role != "tool" {
            continue;
        }
        let Some(text) = message.content.as_str() else {
            continue;
        };
        let tokens = crate::context::estimate_text_tokens(text);
        let protected_by_budget = retained < protected;
        retained = retained.saturating_add(tokens);
        if index >= keep_from
            || protected_by_budget
            || evidence.frozen(index)
            || !message.content.is_prunable_tool_result()
        {
            continue;
        }
        let Some(id) = message.tool_call_id.as_deref() else {
            continue;
        };
        let Some((call, batch_index)) = calls.get(id) else {
            continue;
        };
        if incomplete_batches.contains(batch_index) {
            continue;
        }
        let safe_id = id
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
            .take(128)
            .collect::<String>();
        let marker = format!(
            "[Earlier result of {} call {} omitted from this request. Full output remains in canonical session history; use session_search/session recall, or repeat the read if unavailable.]",
            call.function.name, safe_id
        );
        let saved = tokens.saturating_sub(crate::context::estimate_text_tokens(&marker));
        if saved > 0 {
            savings = savings.saturating_add(saved);
            candidates.push((index, marker));
        }
    }
    if savings < minimum {
        return evidence.finish(messages);
    }
    let mut legacy = PruningStats::default();
    for (index, marker) in candidates {
        if !evidence.age(index) {
            legacy.results += 1;
            let before = crate::context::estimate_text_tokens(
                messages[index].content.as_str().unwrap_or_default(),
            );
            let after = crate::context::estimate_text_tokens(&marker);
            legacy.saved_tokens += before.saturating_sub(after);
            legacy.added_tokens += after.saturating_sub(before);
        }
        messages[index].content = crate::image_content::ModelContent::context_marker(marker);
    }
    let mut stats = evidence.finish(messages);
    stats.results += legacy.results;
    stats.saved_tokens += legacy.saved_tokens;
    stats.added_tokens += legacy.added_tokens;
    stats
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::client::FunctionCall;
    fn history() -> Vec<ChatMessage> {
        let mut messages = Vec::new();
        for i in 0..5 {
            messages.push(ChatMessage {
                role: "user".into(),
                content: crate::image_content::ModelContent::text(format!("turn {i}")),
                tool_calls: None,
                tool_call_id: None,
                response_items: vec![],
            });
            messages.push(ChatMessage {
                role: "assistant".into(),
                content: crate::image_content::ModelContent::text(""),
                tool_calls: Some(vec![ToolCallMsg {
                    id: format!("call{i}"),
                    call_type: "function".into(),
                    function: FunctionCall {
                        arguments_retrieved_spans: Vec::new(),
                        name: "read_file".into(),
                        arguments: "{}".into(),
                    },
                }]),
                tool_call_id: None,
                response_items: vec![],
            });
            messages.push(ChatMessage {
                role: "tool".into(),
                content: crate::image_content::ModelContent::text("evidence ".repeat(3000))
                    .with_prunable_tool_result(true),
                tool_calls: None,
                tool_call_id: Some(format!("call{i}")),
                response_items: vec![],
            });
        }
        messages
    }

    #[test]
    fn graph_worker_ages_seen_reads_within_one_turn_without_changing_originals() {
        let mut original = history();
        let mut human_seen = false;
        original.retain(|message| {
            if message.role != "user" {
                return true;
            }
            let keep = !human_seen;
            human_seen = true;
            keep
        });
        for message in &mut original {
            if message.role == "tool" {
                message.content = message
                    .content
                    .clone()
                    .with_tool_result_status(true)
                    .with_source_receipt(Some("source-receipt-must-stay-exact".into()));
            }
        }
        let canonical = serde_json::to_vec(&original).unwrap();
        let mut ordinary = original.clone();
        assert_eq!(project(&mut ordinary, true, Some(8_000)).results, 0);
        let mut projected = original.clone();
        let stats = project_worker(&original, &mut projected, &[], 100_000, 24, true);
        assert!(stats.results >= 3);
        assert!(stats.saved_tokens > 10_000);
        assert_eq!(serde_json::to_vec(&original).unwrap(), canonical);
        assert_eq!(
            projected.last().unwrap().content,
            original.last().unwrap().content
        );
        assert_eq!(
            projected.last().unwrap().content.source_receipt(),
            Some("source-receipt-must-stay-exact".into())
        );
        for (before, after) in original.iter().zip(&projected) {
            assert_eq!(before.tool_call_id, after.tool_call_id);
            assert_eq!(
                serde_json::to_vec(&before.tool_calls).unwrap(),
                serde_json::to_vec(&after.tool_calls).unwrap()
            );
            if after.content.is_context_marker() {
                assert!(after
                    .content
                    .as_str()
                    .unwrap()
                    .contains("original call arguments"));
                assert!(after.content.source_receipt().is_none());
            }
        }
        let mut disabled = original.clone();
        assert_eq!(
            project_worker(&original, &mut disabled, &[], 100_000, 24, false).results,
            0
        );
        assert_eq!(serde_json::to_vec(&disabled).unwrap(), canonical);
    }

    #[test]
    fn graph_worker_protects_errors_skills_commands_and_incomplete_batches() {
        let mut messages = history();
        messages[2].content = messages[2].content.clone().with_tool_result_status(false);
        messages[4].tool_calls.as_mut().unwrap()[0]
            .function
            .arguments = r#"{"path":"skills/example/SKILL.md"}"#.into();
        messages[5].content = messages[5].content.clone().with_tool_result_status(true);
        messages[7].tool_calls.as_mut().unwrap()[0].function.name = "run_command".into();
        messages[8].content = messages[8].content.clone().with_tool_result_status(true);
        let mut pending = messages[10].tool_calls.as_ref().unwrap()[0].clone();
        pending.id = "pending-batch-call".into();
        messages[10].tool_calls.as_mut().unwrap().push(pending);
        messages[11].content = messages[11].content.clone().with_tool_result_status(true);
        messages[14].content = messages[14].content.clone().with_tool_result_status(true);
        let before = serde_json::to_vec(&messages).unwrap();
        let canonical = messages.clone();
        assert_eq!(
            project_worker(&canonical, &mut messages, &[], 100, 24, true).results,
            0
        );
        assert_eq!(serde_json::to_vec(&messages).unwrap(), before);
    }
    #[test]
    fn graph_worker_retains_authenticated_duplicate_basis() {
        let mut canonical = history()[..3].to_vec();
        let attach = |message: &mut ChatMessage| {
            let text = message.content.as_str().unwrap().to_owned();
            message.content = message
                .content
                .clone()
                .with_tool_result_status(true)
                .with_context_observation(crate::context_observation::capture_host(
                    "read_file",
                    "{}",
                    &text,
                    true,
                    "/workspace",
                    &["file.txt".into()],
                    Some(&"a".repeat(64)),
                ));
        };
        attach(&mut canonical[2]);
        let mut first = canonical.clone();
        project(&mut first, true, Some(8_000));
        let mut next = canonical[1..3].to_vec();
        next[0].tool_calls.as_mut().unwrap()[0].id = "duplicate".into();
        next[1].tool_call_id = Some("duplicate".into());
        attach(&mut next[1]);
        canonical.extend(next);
        let mut projected = canonical.clone();
        project(&mut projected, true, Some(8_000));
        assert!(projected[4].content.is_context_marker());
        assert!(
            crate::context_observation::retained_basis_indices(&canonical, &projected).contains(&2)
        );
        project_worker(&canonical, &mut projected, &[], 100, 24, true);
        assert_eq!(projected[2].content.as_str(), canonical[2].content.as_str());
        assert!(projected[4]
            .content
            .as_str()
            .unwrap()
            .contains("retained call call0"));
    }

    #[test]
    fn orc290_prunes_old_results_preserves_pairing_recent_turns_and_original() {
        let original = history();
        let bytes = serde_json::to_vec(&original).unwrap();
        let mut projected = original.clone();
        let stats = project(&mut projected, true, Some(8000));
        assert!(stats.results >= 2);
        assert!(stats.saved_tokens > 800);
        assert_eq!(serde_json::to_vec(&original).unwrap(), bytes);
        assert_eq!(
            serde_json::to_vec(&projected[9..]).unwrap(),
            serde_json::to_vec(&original[9..]).unwrap()
        );
        for (before, after) in original.iter().zip(&projected) {
            assert_eq!(before.tool_call_id, after.tool_call_id);
            assert_eq!(
                serde_json::to_value(&before.tool_calls).unwrap(),
                serde_json::to_value(&after.tool_calls).unwrap()
            );
        }
        assert_eq!(
            project(&mut projected.clone(), true, Some(8000)),
            PruningStats::default()
        );
    }
    #[test]
    fn orc290_protects_unknown_disabled_small_and_incomplete() {
        let mut messages = history();
        for message in &mut messages {
            message.content =
                crate::image_content::ModelContent::text(message.content.as_str().unwrap());
        }
        assert_eq!(
            project(&mut messages, true, Some(8000)),
            PruningStats::default()
        );
        assert_eq!(
            project(&mut history(), false, Some(8000)),
            PruningStats::default()
        );
        let mut small = history();
        for message in &mut small {
            if message.role == "tool" {
                message.content = crate::image_content::ModelContent::text("small")
                    .with_prunable_tool_result(true);
            }
        }
        assert_eq!(
            project(&mut small, true, Some(8000)),
            PruningStats::default()
        );
        let mut incomplete = history();
        let call = incomplete[1].tool_calls.as_ref().unwrap()[0].clone();
        incomplete[1]
            .tool_calls
            .as_mut()
            .unwrap()
            .push(ToolCallMsg {
                id: "missing".into(),
                ..call
            });
        project(&mut incomplete, true, Some(8000));
        assert!(incomplete[2]
            .content
            .as_str()
            .unwrap()
            .starts_with("evidence"));
    }
    #[test]
    fn orc290_qualification_requires_known_success_and_protects_skills() {
        let mut call = ToolCall {
            observation: None,
            output_source: None,
            arguments_retrieved_spans: Vec::new(),
            id: "c".into(),
            name: "read_file".into(),
            arguments: "{}".into(),
            status: ToolStatus::Done,
            output: Some("text".into()),
            started_at: None,
        };
        assert!(qualified(&call));
        call.status = ToolStatus::Error;
        assert!(!qualified(&call));
        call.status = ToolStatus::Done;
        call.name = "mcp__private".into();
        assert!(!qualified(&call));
        call.name = "read_file".into();
        call.arguments = "SKILL.md".into();
        assert!(!qualified(&call));
    }
    #[test]
    fn orc290_recent_budget_keeps_the_whole_crossing_result() {
        let mut messages = history();
        for index in [11, 14] {
            messages[index].content = crate::image_content::ModelContent::text("x".repeat(400))
                .with_prunable_tool_result(true);
        }
        let original = messages[8].content.as_str().unwrap().to_owned();
        let stats = project(&mut messages, true, Some(4000));
        assert!(stats.results > 0);
        assert_eq!(messages[8].content.as_str(), Some(original.as_str()));
    }
    #[test]
    fn orc290_savings_threshold_admits_exact_boundary() {
        let mut original = history();
        original[2].content = crate::image_content::ModelContent::text("x".repeat(4000))
            .with_prunable_tool_result(true);
        for index in [5, 8] {
            original[index].content =
                crate::image_content::ModelContent::text(original[index].content.as_str().unwrap());
        }
        let savings = project(&mut original.clone(), true, Some(100)).saved_tokens;
        assert!(savings > 0);
        assert_eq!(
            project(&mut original.clone(), true, Some(savings * 10)).results,
            1
        );
        assert_eq!(
            project(&mut original.clone(), true, Some(savings * 10 + 10)),
            PruningStats::default()
        );
    }
    #[test]
    fn orc290_projection_avoids_an_otherwise_required_checkpoint() {
        let original = history();
        let mut projected = original.clone();
        let estimate = |messages: &[ChatMessage]| {
            messages
                .iter()
                .map(|m| {
                    m.content
                        .as_str()
                        .map(crate::context::estimate_text_tokens)
                        .unwrap_or(0)
                })
                .sum::<u64>()
        };
        let before = estimate(&original);
        let threshold = crate::context::automatic_threshold(Some(40_000), None).unwrap();
        let stats = project(&mut projected, true, Some(40_000));
        assert!(before > threshold);
        assert!(estimate(&projected) < threshold);
        assert!(stats.saved_tokens >= 4_000);
        // Canonical retrieval by original call ID still returns the full evidence.
        assert_eq!(
            original
                .iter()
                .find(|m| m.tool_call_id.as_deref() == Some("call0"))
                .unwrap()
                .content
                .as_str()
                .unwrap(),
            "evidence ".repeat(3000)
        );
    }

    fn tool_turn(id: &str, body: &str) -> Vec<ChatMessage> {
        vec![
            ChatMessage {
                role: "user".into(),
                content: crate::image_content::ModelContent::text("turn"),
                tool_calls: None,
                tool_call_id: None,
                response_items: vec![],
            },
            ChatMessage {
                role: "assistant".into(),
                content: crate::image_content::ModelContent::text(""),
                tool_calls: Some(vec![ToolCallMsg {
                    id: id.into(),
                    call_type: "function".into(),
                    function: FunctionCall {
                        arguments_retrieved_spans: Vec::new(),
                        name: "read_file".into(),
                        arguments: "{}".into(),
                    },
                }]),
                tool_call_id: None,
                response_items: vec![],
            },
            ChatMessage {
                role: "tool".into(),
                content: crate::image_content::ModelContent::text(body)
                    .with_prunable_tool_result(true)
                    .with_tool_result_status(true),
                tool_calls: None,
                tool_call_id: Some(id.into()),
                response_items: vec![],
            },
        ]
    }

    #[test]
    fn cold_request_drops_saved_tool_bodies_and_a_warm_prefix_stays() {
        let large = "evidence ".repeat(200);
        assert!(large.len() >= 1024);
        let mut messages = Vec::new();
        messages.extend(tool_turn("old", &large));
        messages.extend(tool_turn("small", "short"));
        messages.extend(tool_turn("latest", &large));
        let original: Vec<_> = messages
            .iter()
            .map(|message| message.content.as_str().unwrap().to_string())
            .collect();
        let dir = std::env::temp_dir().join(format!("ca-cold-tool-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut warm = messages.clone();
        assert_eq!(offload_cold_tool_bodies(&mut warm, &dir, false).results, 0);
        let warm_text: Vec<_> = warm
            .iter()
            .map(|message| message.content.as_str().unwrap().to_string())
            .collect();
        assert_eq!(warm_text, original);

        let stats = offload_cold_tool_bodies(&mut messages, &dir, true);
        assert_eq!(stats.results, 1);
        let old = messages
            .iter()
            .find(|message| message.tool_call_id.as_deref() == Some("old"))
            .unwrap()
            .content
            .as_str()
            .unwrap();
        assert!(old.contains("saved at"));
        assert!(old.contains(&dir.display().to_string()));
        let saved = std::fs::read_dir(&dir)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        assert_eq!(std::fs::read_to_string(&saved).unwrap(), large);
        std::fs::write(&saved, "already written").unwrap();
        let mut again = Vec::new();
        again.extend(tool_turn("old", &large));
        again.extend(tool_turn("latest", &large));
        offload_cold_tool_bodies(&mut again, &dir, true);
        assert_eq!(std::fs::read_to_string(&saved).unwrap(), "already written");
        assert_eq!(
            messages
                .iter()
                .find(|message| message.tool_call_id.as_deref() == Some("small"))
                .unwrap()
                .content
                .as_str()
                .unwrap(),
            "short"
        );
        assert_eq!(
            messages
                .iter()
                .find(|message| message.tool_call_id.as_deref() == Some("latest"))
                .unwrap()
                .content
                .as_str()
                .unwrap(),
            large
        );
        let mut unknown = tool_turn("only", &large);
        let before = unknown[2].content.as_str().unwrap().to_string();
        project_request(
            &mut unknown,
            RequestKind::MainTurn,
            "unknown-provider",
            "unknown-model",
        );
        assert_eq!(unknown[2].content.as_str().unwrap(), before);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
