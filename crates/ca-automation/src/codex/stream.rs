//! Codex stream parsing.
//!
//! Pure helpers for JSONL/stdout parsing and model observation summaries. This
//! module should not decide routing policy or mutate state.

#![allow(dead_code)]

use serde_json::Value;

pub(crate) fn parse_jsonl_line(line: &str) -> Option<Value> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    let value = serde_json::from_str::<Value>(trimmed).ok()?;
    value.get("type").and_then(Value::as_str)?;
    Some(value)
}

pub(crate) fn parse_jsonl_stream(raw: &str) -> Vec<Value> {
    raw.lines().filter_map(parse_jsonl_line).collect()
}

pub(crate) fn extract_final_agent_message(events: &[Value]) -> Option<String> {
    events
        .iter()
        .filter(|event| event.get("type").and_then(Value::as_str) == Some("item.completed"))
        .filter_map(|event| event.get("item"))
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("agent_message"))
        .filter_map(|item| item.get("text").and_then(Value::as_str))
        .next_back()
        .map(str::to_string)
}

pub(crate) fn extract_completed_commands(events: &[Value]) -> Vec<String> {
    events
        .iter()
        .filter(|event| event.get("type").and_then(Value::as_str) == Some("item.completed"))
        .filter_map(|event| event.get("item"))
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("command_execution"))
        .filter_map(|item| item.get("command").and_then(Value::as_str))
        .map(str::to_string)
        .collect()
}

pub(crate) fn extract_changed_files(events: &[Value]) -> Vec<String> {
    let mut files = Vec::new();
    for item in events
        .iter()
        .filter(|event| event.get("type").and_then(Value::as_str) == Some("item.completed"))
        .filter_map(|event| event.get("item"))
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("file_change"))
    {
        if let Some(path) = item.get("path").and_then(Value::as_str) {
            files.push(path.to_string());
        }
        if let Some(changes) = item.get("changes").and_then(Value::as_array) {
            files.extend(
                changes
                    .iter()
                    .filter_map(|change| change.get("path").and_then(Value::as_str))
                    .map(str::to_string),
            );
        }
    }
    files.sort();
    files.dedup();
    files
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_jsonl_line_requires_valid_json_with_type() {
        assert_eq!(
            parse_jsonl_line(r#"{"type":"thread.started","thread_id":"abc"}"#)
                .and_then(|value| value.get("type").cloned()),
            Some(Value::String("thread.started".to_string()))
        );
        assert!(parse_jsonl_line("").is_none());
        assert!(parse_jsonl_line("   ").is_none());
        assert!(parse_jsonl_line("not json").is_none());
        assert!(parse_jsonl_line(r#"{"item":{}}"#).is_none());
        assert!(parse_jsonl_line(r#"{"type":42}"#).is_none());
    }

    #[test]
    fn parse_jsonl_stream_skips_empty_and_malformed_lines() {
        let raw = [
            r#"{"type":"turn.started"}"#,
            "",
            "garbage",
            r#"{"type":"turn.completed"}"#,
        ]
        .join("\n");

        let events = parse_jsonl_stream(&raw);
        assert_eq!(events.len(), 2);
        assert_eq!(
            events[0].get("type").and_then(Value::as_str),
            Some("turn.started")
        );
        assert_eq!(
            events[1].get("type").and_then(Value::as_str),
            Some("turn.completed")
        );
    }

    #[test]
    fn extract_final_agent_message_uses_last_completed_agent_item() {
        let raw = [
            r#"{"type":"item.started","item":{"id":"1","type":"agent_message","text":"started"}}"#,
            r#"{"type":"item.completed","item":{"id":"1","type":"agent_message","text":"first"}}"#,
            r#"{"type":"item.completed","item":{"id":"2","type":"agent_message","text":"second"}}"#,
        ]
        .join("\n");
        let events = parse_jsonl_stream(&raw);

        assert_eq!(
            extract_final_agent_message(&events).as_deref(),
            Some("second")
        );
    }

    #[test]
    fn extract_completed_commands_preserves_order() {
        let raw = [
            r#"{"type":"item.completed","item":{"id":"1","type":"command_execution","command":"cargo test","status":"done"}}"#,
            r#"{"type":"item.completed","item":{"id":"2","type":"command_execution","command":"cargo fmt -- --check","status":"done"}}"#,
        ]
        .join("\n");
        let events = parse_jsonl_stream(&raw);

        assert_eq!(
            extract_completed_commands(&events),
            vec!["cargo test".to_string(), "cargo fmt -- --check".to_string()]
        );
    }

    #[test]
    fn extract_changed_files_supports_path_and_changes_shapes_and_deduplicates() {
        let raw = [
            r#"{"type":"item.completed","item":{"id":"1","type":"file_change","path":"src/a.rs","action":"modify"}}"#,
            r#"{"type":"item.completed","item":{"id":"2","type":"file_change","changes":[{"path":"src/a.rs"},{"path":"src/b.rs"}]}}"#,
        ]
        .join("\n");
        let events = parse_jsonl_stream(&raw);

        assert_eq!(
            extract_changed_files(&events),
            vec!["src/a.rs".to_string(), "src/b.rs".to_string()]
        );
    }
}
