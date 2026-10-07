//! Host-owned interventions. Selection is explicit; observing a pattern alone
//! never enables it. Decisions use completed tool calls in the current user turn.
use crate::agent::client::ChatMessage;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionHint {
    DependencyProbes,
    ForegroundWaits,
    BehavioralVerification,
}
pub type HintPolicy = BTreeSet<ExecutionHint>;

impl ExecutionHint {
    pub fn text(self) -> &'static str {
        match self {
            Self::DependencyProbes => "Combine dependency checks and use the results already gathered before probing again.",
            Self::ForegroundWaits => "Use a bounded job-status check while long-running work continues.",
            Self::BehavioralVerification => "Compilation and formatting checks do not verify the requested behavior. Run its focused acceptance check.",
        }
    }
    fn classify(command: &str) -> Option<Self> {
        let command = command.trim();
        if command.starts_with("which ")
            || command.starts_with("command -v ")
            || command.ends_with(" --version")
        {
            Some(Self::DependencyProbes)
        } else if command.starts_with("sleep ") {
            Some(Self::ForegroundWaits)
        } else if command.starts_with("cargo check")
            || command.starts_with("cargo build")
            || command == "git diff --check"
        {
            Some(Self::BehavioralVerification)
        } else {
            None
        }
    }
}

/// One experiment adds one intervention, keeping existing interventions fixed.
/// This prevents attributing a bundle of prompt and middleware changes to one hint.
pub(crate) fn single_addition(baseline: &HintPolicy, candidate: &HintPolicy) -> bool {
    baseline.is_subset(candidate) && candidate.len() == baseline.len() + 1
}

/// Return only reminders whose third completed occurrence is in the latest tool
/// batch. Reconstructing from history keeps retries deterministic without adding
/// mutable state to a session or repeatedly appending reminders to its transcript.
pub(crate) fn reminders(messages: &[ChatMessage], policy: &HintPolicy) -> Vec<ExecutionHint> {
    if policy.is_empty() || messages.last().is_none_or(|m| m.role != "tool") {
        return Vec::new();
    }
    let start = messages
        .iter()
        .rposition(|m| m.role == "user")
        .map_or(0, |i| i + 1);
    let history = &messages[start..];
    if history.len() > 1024 {
        return Vec::new();
    }
    let mut calls = BTreeMap::new();
    let mut duplicate = BTreeSet::new();
    let mut completed = BTreeSet::new();
    let mut counts = BTreeMap::<ExecutionHint, usize>::new();
    let mut emitted = Vec::new();
    let mut latest_batch = None;
    for (index, message) in history.iter().enumerate() {
        if message.role == "assistant" {
            for call in message.tool_calls.iter().flatten() {
                if calls.len() >= 256 {
                    return Vec::new();
                }
                if calls.insert(call.id.as_str(), (index, call)).is_some() {
                    duplicate.insert(call.id.as_str());
                }
            }
        } else if message.role == "tool" {
            let Some(id) = message.tool_call_id.as_deref() else {
                continue;
            };
            let Some((batch, call)) = calls.get(id) else {
                continue;
            };
            latest_batch = Some(*batch);
            if !completed.insert(id) {
                duplicate.insert(id);
            }
            if duplicate.contains(id)
                || call.function.name != "run_command"
                || call.function.arguments.len() > 16 * 1024
            {
                continue;
            }
            let Some(result) = message.content.as_str() else {
                continue;
            };
            // Refusals are not executed commands. Unknown structured/image results
            // do not establish the completed invocation needed for an intervention.
            if result.contains("Tool denied by user") || result.contains("Approval required") {
                continue;
            }
            let Ok(arguments) = serde_json::from_str::<serde_json::Value>(&call.function.arguments)
            else {
                continue;
            };
            let Some(hint) = arguments
                .get("command")
                .and_then(|v| v.as_str())
                .and_then(ExecutionHint::classify)
            else {
                continue;
            };
            if !policy.contains(&hint) {
                continue;
            }
            let count = counts.entry(hint).or_default();
            *count += 1;
            if *count == 3 && emitted.len() < 2 {
                emitted.push((hint, *batch));
            }
        }
    }
    // Ambiguous history can otherwise trigger before its duplicate is encountered.
    if !duplicate.is_empty() {
        return Vec::new();
    }
    emitted
        .into_iter()
        .filter_map(|(hint, batch)| (Some(batch) == latest_batch).then_some(hint))
        .collect()
}

pub(crate) fn reminder_text(messages: &[ChatMessage], policy: &HintPolicy) -> String {
    reminders(messages, policy)
        .into_iter()
        .map(|hint| format!("\n\nExecution reminder: {}", hint.text()))
        .collect()
}

pub(crate) fn apply(messages: &mut [ChatMessage], policy: &HintPolicy) {
    let suffix = reminder_text(messages, policy);
    if suffix.is_empty() {
        return;
    }
    if let Some(message) = messages.iter_mut().find(|m| m.role == "system") {
        if let Some(text) = message.content.as_str() {
            message.content = crate::image_content::ModelContent::text(format!("{text}{suffix}"));
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::agent::client::{FunctionCall, ToolCallMsg};
    fn message(role: &str, text: &str) -> ChatMessage {
        ChatMessage {
            role: role.into(),
            content: crate::image_content::ModelContent::text(text),
            tool_calls: None,
            tool_call_id: None,
            response_items: Vec::new(),
        }
    }
    pub(crate) fn command(messages: &mut Vec<ChatMessage>, id: &str, command: &str) {
        let mut assistant = message("assistant", "");
        assistant.tool_calls = Some(vec![ToolCallMsg {
            id: id.into(),
            call_type: "function".into(),
            function: FunctionCall {
                name: "run_command".into(),
                arguments: serde_json::json!({"command":command}).to_string(),
            },
        }]);
        messages.push(assistant);
        let mut result = message("tool", "done");
        result.tool_call_id = Some(id.into());
        messages.push(result);
    }
    fn all() -> HintPolicy {
        [
            ExecutionHint::DependencyProbes,
            ExecutionHint::ForegroundWaits,
            ExecutionHint::BehavioralVerification,
        ]
        .into_iter()
        .collect()
    }
    #[test]
    fn hrs253_hint_requires_enabled_policy_and_third_completed_call() {
        let mut messages = vec![message("user", "fix the behavior")];
        for (index, text) in ["which cargo", "command -v rustc", "cargo --version"]
            .into_iter()
            .enumerate()
        {
            command(&mut messages, &index.to_string(), text);
            assert_eq!(reminders(&messages, &all()).len(), usize::from(index == 2));
        }
        assert!(reminders(&messages, &HintPolicy::new()).is_empty());
        assert_eq!(
            reminders(&messages, &all()),
            vec![ExecutionHint::DependencyProbes]
        );
        assert!(!ExecutionHint::DependencyProbes.text().is_empty());
        command(&mut messages, "next", "cargo test");
        assert!(reminders(&messages, &all()).is_empty());
    }
    #[test]
    fn hrs253_hints_are_bounded_per_turn_and_reset_on_new_user_input() {
        let mut messages = vec![message("user", "fix")];
        for (kind, text) in ["which cargo", "sleep 10", "cargo check"]
            .into_iter()
            .enumerate()
        {
            for step in 0..3 {
                command(&mut messages, &format!("{kind}-{step}"), text);
            }
            assert_eq!(reminders(&messages, &all()).len(), usize::from(kind < 2));
        }
        messages.push(message("user", "next task"));
        for step in 0..3 {
            command(&mut messages, &format!("new-{step}"), "cargo check");
        }
        assert_eq!(
            reminders(&messages, &all()),
            vec![ExecutionHint::BehavioralVerification]
        );
        assert!(single_addition(
            &HintPolicy::new(),
            &[ExecutionHint::DependencyProbes].into_iter().collect()
        ));
        assert!(!single_addition(&HintPolicy::new(), &all()));
    }
    #[test]
    fn hrs253_hints_handle_parallel_batches_without_repeating_old_reminders() {
        let mut sequential = Vec::new();
        for (index, text) in [
            "which cargo",
            "sleep 1",
            "which rustc",
            "sleep 2",
            "cargo --version",
            "sleep 3",
        ]
        .into_iter()
        .enumerate()
        {
            command(&mut sequential, &index.to_string(), text);
        }
        let calls = sequential
            .iter()
            .flat_map(|m| m.tool_calls.iter().flatten().cloned())
            .collect();
        let mut batch = message("assistant", "");
        batch.tool_calls = Some(calls);
        let mut messages = vec![message("user", "fix"), batch];
        messages.extend(sequential.into_iter().filter(|m| m.role == "tool"));
        assert_eq!(
            reminders(&messages, &all()),
            vec![
                ExecutionHint::DependencyProbes,
                ExecutionHint::ForegroundWaits
            ]
        );
        command(&mut messages, "next", "cargo check");
        assert!(reminders(&messages, &all()).is_empty());
        messages.extend((0..1024).map(|_| message("assistant", "oversized history")));
        messages.push(message("tool", "done"));
        assert!(reminders(&messages, &all()).is_empty());
    }

    #[test]
    fn hrs253_hints_ignore_denied_unfinished_and_ambiguous_calls() {
        let mut messages = vec![message("user", "fix")];
        for step in 0..3 {
            command(&mut messages, &step.to_string(), "which cargo");
        }
        let result = messages.pop().unwrap();
        assert!(reminders(&messages, &all()).is_empty());
        messages.push(result);
        messages.last_mut().unwrap().content =
            crate::image_content::ModelContent::text("Tool denied by user");
        assert!(reminders(&messages, &all()).is_empty());
        messages.last_mut().unwrap().content = crate::image_content::ModelContent::text("done");
        let repeated = messages.last().unwrap().clone();
        messages.push(repeated);
        assert!(reminders(&messages, &all()).is_empty());
        messages.pop();
        command(&mut messages, "2", "which cargo");
        assert!(reminders(&messages, &all()).is_empty());
    }
}
