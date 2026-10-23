//! Request-only reduction of host-qualified tool results. Canonical history is never edited.
use crate::agent::client::{ChatMessage, ToolCallMsg};
use crate::app::{ToolCall, ToolStatus};
use crate::config::CaConfig;
use crate::request::RequestKind;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::{OnceLock, RwLock};

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
}
impl PruningStats {
    pub fn apply(self, breakdown: &mut crate::context::ContextBreakdown) {
        breakdown.conversation = breakdown.conversation.saturating_sub(self.saved_tokens);
        breakdown.total = breakdown.total.saturating_sub(self.saved_tokens);
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
    if !matches!(
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
    ) {
        return PruningStats::default();
    }
    let settings = policy().read().unwrap_or_else(|p| p.into_inner());
    let window = settings
        .windows
        .get(&format!("{provider}/{model}"))
        .or_else(|| settings.windows.get(model))
        .copied();
    project(messages, !settings.disabled, window)
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
        if index >= keep_from || protected_by_budget || !message.content.is_prunable_tool_result() {
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
        return PruningStats::default();
    }
    let stats = PruningStats {
        results: candidates.len(),
        saved_tokens: savings,
    };
    for (index, marker) in candidates {
        messages[index].content = crate::image_content::ModelContent::text(marker);
    }
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
            id: "c".into(),
            name: "read_file".into(),
            arguments: "{}".into(),
            status: ToolStatus::Done,
            output: Some("text".into()),
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
}
