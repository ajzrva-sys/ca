//! Request-copy repairs for a wedged tail and one empty-reply retry.
//! Session history is not rewritten here.

use crate::agent::client::ChatMessage;
use crate::image_content::ModelContent;

pub const EMPTY_REPLY_REMINDER: &str =
    "The previous reply had no visible text. Continue with the answer or the next tool call.";
pub const EMPTY_REPLY_MARKER: &str = "[empty reply]";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmptyReply {
    Keep,
    Retry,
    Stop,
}

pub fn empty_reply(content: &str, already_retried: bool) -> EmptyReply {
    if !content.trim().is_empty() {
        return EmptyReply::Keep;
    }
    if already_retried {
        EmptyReply::Stop
    } else {
        EmptyReply::Retry
    }
}

/// Merge a trailing user/user pair and stub an assistant tool call that has no result.
pub fn repair_request_tail(messages: &mut Vec<ChatMessage>) {
    stub_missing_tool_results(messages);
    merge_user_tail(messages);
}

fn stub_missing_tool_results(messages: &mut Vec<ChatMessage>) {
    let mut index = 0;
    while index < messages.len() {
        if messages[index].role != "assistant" {
            index += 1;
            continue;
        }
        let Some(calls) = messages[index].tool_calls.clone() else {
            index += 1;
            continue;
        };
        let mut insert_at = index + 1;
        while insert_at < messages.len() && messages[insert_at].role == "tool" {
            insert_at += 1;
        }
        for call in &calls {
            let present = messages[index + 1..insert_at]
                .iter()
                .any(|message| message.tool_call_id.as_deref() == Some(call.id.as_str()));
            if !present {
                messages.insert(insert_at, stub_tool_message(&call.id));
                insert_at += 1;
            }
        }
        index = insert_at;
    }
}

fn stub_tool_message(id: &str) -> ChatMessage {
    ChatMessage {
        role: "tool".into(),
        content: ModelContent::text(
            "Error: tool result missing; the call was closed before a result was recorded.",
        ),
        tool_calls: None,
        tool_call_id: Some(id.to_string()),
        response_items: Vec::new(),
    }
}

fn merge_user_tail(messages: &mut Vec<ChatMessage>) {
    if messages.len() < 2 {
        return;
    }
    let last = messages.len() - 1;
    if messages[last].role != "user" || messages[last - 1].role != "user" {
        return;
    }
    let Some(left) = messages[last - 1].content.as_str().map(str::to_string) else {
        return;
    };
    let Some(right) = messages[last].content.as_str().map(str::to_string) else {
        return;
    };
    messages[last - 1].content = ModelContent::text(format!("{left}\n\n{right}"));
    messages.pop();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::client::{FunctionCall, ToolCallMsg};

    fn user(text: &str) -> ChatMessage {
        ChatMessage {
            role: "user".into(),
            content: ModelContent::text(text),
            tool_calls: None,
            tool_call_id: None,
            response_items: Vec::new(),
        }
    }

    fn assistant_call(id: &str) -> ChatMessage {
        ChatMessage {
            role: "assistant".into(),
            content: ModelContent::text(""),
            tool_calls: Some(vec![ToolCallMsg {
                id: id.into(),
                call_type: "function".into(),
                function: FunctionCall {
                    name: "read_file".into(),
                    arguments: "{}".into(),
                    arguments_retrieved_spans: Vec::new(),
                },
            }]),
            tool_call_id: None,
            response_items: Vec::new(),
        }
    }

    #[test]
    fn a_wedged_tail_is_repaired_on_the_copy_only() {
        let mut messages = vec![
            user("first"),
            assistant_call("call-1"),
            user("second"),
            user("third"),
        ];
        let stored = messages.clone();
        repair_request_tail(&mut messages);
        assert_eq!(stored[3].content.as_str(), Some("third"));
        assert_eq!(messages.len(), 4);
        assert_eq!(messages[1].role, "assistant");
        assert_eq!(messages[2].role, "tool");
        assert_eq!(messages[2].tool_call_id.as_deref(), Some("call-1"));
        assert!(messages[2]
            .content
            .as_str()
            .unwrap()
            .contains("tool result missing"));
        assert_eq!(messages[3].content.as_str(), Some("second\n\nthird"));
        assert_eq!(empty_reply("  ", false), EmptyReply::Retry);
        assert_eq!(empty_reply("  ", true), EmptyReply::Stop);
        assert_eq!(empty_reply("answer", true), EmptyReply::Keep);
    }
}
