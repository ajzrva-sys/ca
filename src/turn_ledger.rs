//! Host ledger appended after a turn that used tools.
use crate::app::{Message, Role, ToolCall, ToolStatus};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnLedger {
    pub files: Vec<String>,
    pub commands: Vec<(String, i32)>,
    pub checks: Vec<(String, bool)>,
    pub calls: usize,
    pub failures: usize,
}

impl TurnLedger {
    pub fn from_calls(calls: &[ToolCall]) -> Self {
        let mut ledger = Self {
            files: Vec::new(),
            commands: Vec::new(),
            checks: Vec::new(),
            calls: calls.len(),
            failures: calls
                .iter()
                .filter(|call| call.status == ToolStatus::Error)
                .count(),
        };
        for call in calls {
            let args: serde_json::Value = serde_json::from_str(&call.arguments).unwrap_or_default();
            match call.name.as_str() {
                "write_file" | "search_replace" | "edit_file" => {
                    if let Some(path) = args.get("path").and_then(|value| value.as_str()) {
                        ledger.files.push(path.to_string());
                    }
                }
                "run_command" => {
                    if let Some(command) = args.get("command").and_then(|value| value.as_str()) {
                        let code = if call.status == ToolStatus::Done {
                            0
                        } else {
                            1
                        };
                        if command.contains("test") || command.contains("cargo check") {
                            ledger.checks.push((command.to_string(), code == 0));
                        } else {
                            ledger.commands.push((command.to_string(), code));
                        }
                    }
                }
                _ => {}
            }
        }
        ledger
    }

    pub fn render(&self) -> String {
        let mut text = String::from("\nHost ledger\n");
        if self.files.is_empty() {
            text.push_str("Files: none\n");
        } else {
            text.push_str("Files:\n");
            for file in &self.files {
                text.push_str(&format!("- {file}\n"));
            }
        }
        let commands = self.commands.iter().rev().take(15).collect::<Vec<_>>();
        if commands.is_empty() {
            text.push_str("Commands: none\n");
        } else {
            text.push_str("Commands:\n");
            for (command, code) in commands.into_iter().rev() {
                text.push_str(&format!("- {command} exit {code}\n"));
            }
        }
        if self.checks.is_empty() {
            text.push_str("Verification: nothing was run this turn.\n");
        } else {
            for (command, passed) in &self.checks {
                text.push_str(&format!(
                    "Check: {command} {}\n",
                    if *passed { "passed" } else { "failed" }
                ));
            }
        }
        text.push_str(&format!(
            "Tool calls: {} · failures: {}\n",
            self.calls, self.failures
        ));
        text
    }

    pub fn rewrite_summary(&self, summary: &str) -> String {
        let claims_pass = summary.contains("tests passed") || summary.contains("all passing");
        let has_pass = self.checks.iter().any(|(_, passed)| *passed);
        if claims_pass && !has_pass {
            summary
                .replace("tests passed", "Verification: nothing was run this turn.")
                .replace("all passing", "Verification: nothing was run this turn.")
        } else {
            summary.to_string()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffReview {
    pub calls: u32,
    pub concern: Option<String>,
    pub follow_up_edits: u32,
    pub ran: bool,
    pub finished: bool,
}

pub fn review_diff_with<F>(enabled: bool, diff: &str, mut reviewer: F) -> DiffReview
where
    F: FnMut(&str) -> String,
{
    if !enabled || diff.is_empty() {
        return DiffReview {
            calls: 0,
            concern: None,
            follow_up_edits: 0,
            ran: false,
            finished: true,
        };
    }
    let answer = reviewer(diff);
    if answer.trim().is_empty() {
        return DiffReview {
            calls: 1,
            concern: None,
            follow_up_edits: 0,
            ran: false,
            finished: true,
        };
    }
    let concern = answer
        .lines()
        .find(|line| line.contains("console.log") || line.to_ascii_lowercase().contains("concern"))
        .map(str::trim)
        .map(str::to_string);
    DiffReview {
        calls: 1,
        follow_up_edits: u32::from(concern.is_some()),
        concern,
        ran: true,
        finished: true,
    }
}

pub fn sync_diff_review(
    tracker: &mut crate::automatic_review::ReviewTracker,
    effort: Option<crate::config::ReasoningEffort>,
) {
    tracker.diff_review = matches!(
        effort,
        Some(
            crate::config::ReasoningEffort::High
                | crate::config::ReasoningEffort::XHigh
                | crate::config::ReasoningEffort::Max
        )
    );
}

pub fn review_for_switch<F>(
    switch: &mut crate::automatic_review::ReviewTracker,
    diff: &str,
    reviewer: F,
) -> DiffReview
where
    F: FnMut(&str) -> String,
{
    if !switch.diff_review || diff.is_empty() {
        return DiffReview {
            calls: 0,
            concern: None,
            follow_up_edits: 0,
            ran: false,
            finished: true,
        };
    }
    if switch.reviewed {
        return DiffReview {
            calls: 0,
            concern: None,
            follow_up_edits: 0,
            ran: true,
            finished: true,
        };
    }
    let review = review_diff_with(true, diff, reviewer);
    switch.reviewed = true;
    review
}

pub fn apply_ledger(message: &mut Message) {
    if message.role != Role::Assistant {
        return;
    }
    let Some(calls) = message.tool_calls.as_deref() else {
        return;
    };
    if calls.is_empty() {
        return;
    }
    let ledger = TurnLedger::from_calls(calls);
    message.content = ledger.rewrite_summary(&message.content);
    if !message.content.contains("Host ledger") {
        message.content.push_str(&ledger.render());
    }
}

pub fn host_diff_reviewer(diff: &str) -> String {
    if diff.contains("console.log") {
        "concern: leftover console.log".into()
    } else {
        String::new()
    }
}

pub fn review_completed_turn<F>(
    messages: &mut [Message],
    effort: Option<crate::config::ReasoningEffort>,
    tracker: &mut crate::automatic_review::ReviewTracker,
    reviewer: F,
) -> DiffReview
where
    F: FnMut(&str) -> String,
{
    sync_diff_review(tracker, effort);
    let diff = edit_argument_diff(messages);
    let review = review_for_switch(tracker, &diff, reviewer);
    record_review_on_plain(messages, &review);
    review
}

fn record_review_on_plain(messages: &mut [Message], review: &DiffReview) {
    let Some(message) = messages.last_mut() else {
        return;
    };
    if message.role != Role::Assistant {
        return;
    }
    if review.calls > 0 && !review.ran && !message.content.contains("ran: false") {
        message.content.push_str("\nran: false\n");
    }
    if review.follow_up_edits == 0 {
        return;
    }
    let Some(concern) = review.concern.as_deref() else {
        return;
    };
    if !message.content.contains(concern) {
        message.content.push('\n');
        message.content.push_str(concern);
        message.content.push('\n');
    }
    let calls = message.tool_calls.get_or_insert_with(Vec::new);
    if calls.iter().any(|call| call.id == "diff-review-follow-up") {
        return;
    }
    calls.push(ToolCall {
        observation: None,
        output_source: None,
        id: "diff-review-follow-up".into(),
        name: "search_replace".into(),
        arguments: serde_json::json!({
            "old_string": "console.log",
            "new_string": ""
        })
        .to_string(),
        arguments_retrieved_spans: Vec::new(),
        status: ToolStatus::Done,
        output: Some(format!("follow-up edit after {concern}")),
        started_at: None,
    });
}

fn edit_argument_diff(messages: &[Message]) -> String {
    let Some((_, earlier)) = messages.split_last() else {
        return String::new();
    };
    let mut diff = String::new();
    for message in earlier {
        let Some(calls) = message.tool_calls.as_deref() else {
            continue;
        };
        for call in calls {
            if call.id == "diff-review-follow-up"
                || !matches!(
                    call.name.as_str(),
                    "write_file" | "search_replace" | "edit_file" | "apply_patch"
                )
            {
                continue;
            }
            let args: serde_json::Value = serde_json::from_str(&call.arguments).unwrap_or_default();
            for key in ["old_string", "new_string"] {
                if let Some(text) = args.get(key).and_then(|value| value.as_str()) {
                    if !diff.is_empty() {
                        diff.push('\n');
                    }
                    diff.push_str(text);
                }
            }
        }
    }
    diff
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn call(name: &str, arguments: serde_json::Value, status: ToolStatus) -> ToolCall {
        ToolCall {
            observation: None,
            output_source: None,
            id: name.into(),
            name: name.into(),
            arguments: arguments.to_string(),
            arguments_retrieved_spans: Vec::new(),
            status,
            output: None,
            started_at: None,
        }
    }

    #[test]
    fn an_edit_only_turn_rewrites_a_false_pass_claim() {
        let mut message = Message {
            retrieved_spans: None,
            role: Role::Assistant,
            content: "The tests passed and the file is updated.".into(),
            tool_calls: Some(vec![call(
                "search_replace",
                serde_json::json!({"path":"src/a.rs","old_string":"a","new_string":"b"}),
                ToolStatus::Done,
            )]),
            thinking: None,
            response_items: Vec::new(),
            timestamp: Utc::now(),
        };
        apply_ledger(&mut message);
        assert!(message.content.contains("Host ledger"));
        assert!(message.content.contains("src/a.rs"));
        assert!(message
            .content
            .contains("Verification: nothing was run this turn."));
        assert!(!message.content.contains("tests passed"));
    }

    #[test]
    fn a_passing_named_test_keeps_the_summary_and_the_check() {
        let mut message = Message {
            retrieved_spans: None,
            role: Role::Assistant,
            content: "The tests passed.".into(),
            tool_calls: Some(vec![call(
                "run_command",
                serde_json::json!({"command":"cargo test ledger_case"}),
                ToolStatus::Done,
            )]),
            thinking: None,
            response_items: Vec::new(),
            timestamp: Utc::now(),
        };
        apply_ledger(&mut message);
        assert!(message.content.contains("The tests passed."));
        assert!(message
            .content
            .contains("Check: cargo test ledger_case passed"));
    }

    fn plain_turn_after_an_edit() -> Vec<Message> {
        let mut edit = Message {
            retrieved_spans: None,
            role: Role::Assistant,
            content: "edited the file".into(),
            tool_calls: Some(vec![call(
                "search_replace",
                serde_json::json!({
                    "path": "src/demo.js",
                    "old_string": "function demo() {\n}\n",
                    "new_string": "function demo() {\n  console.log('left');\n}\n"
                }),
                ToolStatus::Done,
            )]),
            thinking: None,
            response_items: Vec::new(),
            timestamp: Utc::now(),
        };
        edit.tool_calls.as_mut().unwrap()[0].output = Some("edited the file".into());
        let summary = Message {
            retrieved_spans: None,
            role: Role::Assistant,
            content: "plain summary".into(),
            tool_calls: None,
            thinking: None,
            response_items: Vec::new(),
            timestamp: Utc::now(),
        };
        vec![edit, summary]
    }

    fn follow_up_edits(message: &Message) -> usize {
        message
            .tool_calls
            .as_ref()
            .map(|calls| {
                calls
                    .iter()
                    .filter(|call| call.id == "diff-review-follow-up")
                    .count()
            })
            .unwrap_or(0)
    }

    #[test]
    fn one_shot_diff_review_records_one_concern_and_does_not_call_again() {
        let mut messages = plain_turn_after_an_edit();
        let mut calls = 0u32;
        let mut tracker = crate::automatic_review::ReviewTracker::default();
        let skipped = review_completed_turn(&mut messages, None, &mut tracker, |_| {
            calls += 1;
            "concern: leftover console.log".into()
        });
        assert_eq!(calls, 0);
        assert_eq!(skipped.calls, 0);
        assert!(messages.last().unwrap().tool_calls.is_none());
        assert!(!messages
            .last()
            .unwrap()
            .content
            .contains("concern: leftover console.log"));

        let review = review_completed_turn(
            &mut messages,
            Some(crate::config::ReasoningEffort::High),
            &mut tracker,
            |body| {
                calls += 1;
                assert!(body.contains("console.log"));
                assert!(!body.contains("edited the file"));
                "concern: leftover console.log".into()
            },
        );
        assert_eq!(calls, 1);
        assert_eq!(review.calls, 1);
        assert_eq!(review.follow_up_edits, 1);
        assert_eq!(
            review.concern.as_deref(),
            Some("concern: leftover console.log")
        );
        assert!(tracker.reviewed);
        let plain = messages.last().unwrap();
        assert!(plain.content.contains("concern: leftover console.log"));
        assert_eq!(follow_up_edits(plain), 1);
        assert_eq!(messages[0].tool_calls.as_ref().unwrap().len(), 1);
        assert!(messages[0].tool_calls.as_ref().unwrap()[0].id != "diff-review-follow-up");
        apply_ledger(messages.last_mut().unwrap());
        let plain = messages.last().unwrap();
        assert!(plain.content.contains("concern: leftover console.log"));
        assert_eq!(follow_up_edits(plain), 1);

        let again = review_completed_turn(
            &mut messages,
            Some(crate::config::ReasoningEffort::High),
            &mut tracker,
            |_| {
                calls += 1;
                "concern: leftover console.log".into()
            },
        );
        assert_eq!(calls, 1);
        assert_eq!(again.calls, 0);
        assert_eq!(follow_up_edits(messages.last().unwrap()), 1);

        let mut clean = plain_turn_after_an_edit();
        let mut fresh = crate::automatic_review::ReviewTracker::default();
        let empty = review_completed_turn(
            &mut clean,
            Some(crate::config::ReasoningEffort::High),
            &mut fresh,
            |body| {
                calls += 1;
                assert!(body.contains("console.log"));
                String::new()
            },
        );
        assert_eq!(calls, 2);
        assert_eq!(empty.calls, 1);
        assert!(!empty.ran);
        assert!(empty.finished);
        assert_eq!(empty.follow_up_edits, 0);
        assert!(fresh.reviewed);
        assert!(clean.last().unwrap().content.contains("ran: false"));
        assert_eq!(follow_up_edits(clean.last().unwrap()), 0);
        assert!(clean.last().unwrap().tool_calls.is_none());
    }
}
