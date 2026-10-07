use crate::app::{Message, Role, ToolStatus};
use std::io::{Read, Write};

const MAX_TOOL_NAME_CHARS: usize = 128;

pub(crate) fn render(messages: &[Message], configured_secrets: &[&str]) -> String {
    let mut output = String::from("ca raw transcript\n=================\n");
    if messages.is_empty() {
        output.push_str("\n[No visible conversation]\n");
        return output;
    }

    for message in messages {
        match message.role {
            Role::System => push_section(&mut output, "System", "[System prompt omitted]"),
            Role::User => push_section(
                &mut output,
                "User",
                &safe_visible_text(&message.content, configured_secrets),
            ),
            Role::Assistant => {
                let mut body = String::new();
                if message
                    .thinking
                    .as_deref()
                    .is_some_and(|value| !value.is_empty())
                {
                    body.push_str("[Hidden reasoning omitted]\n");
                }
                let visible = safe_visible_text(&message.content, configured_secrets);
                if visible.is_empty() {
                    body.push_str("[Empty assistant message]\n");
                } else {
                    body.push_str(&visible);
                    body.push('\n');
                }
                if !message.response_items.is_empty() {
                    body.push_str("[Provider-only response items omitted]\n");
                }
                if let Some(calls) = &message.tool_calls {
                    for call in calls {
                        body.push('\n');
                        let name = bounded_tool_name(&call.name, configured_secrets);
                        let status = match call.status {
                            ToolStatus::Pending => "pending",
                            ToolStatus::Running => "running",
                            ToolStatus::Done => "done",
                            ToolStatus::Error => "error",
                        };
                        body.push_str(&format!("Tool: {name} ({status})\n"));
                        if crate::email::is_email_tool(&call.name) {
                            body.push_str("[External private email content omitted]\n");
                        } else {
                            body.push_str("[Tool arguments omitted]\n");
                            if call.output.is_some() {
                                body.push_str("[Tool output omitted]\n");
                            } else {
                                body.push_str("[Tool output unavailable]\n");
                            }
                        }
                    }
                }
                push_section(&mut output, "Assistant", body.trim_end());
            }
            Role::Tool => push_section(&mut output, "Tool", "[Legacy tool message omitted]"),
        }
    }
    output
}

pub(crate) fn present<W: Write, R: Read>(
    writer: &mut W,
    reader: &mut R,
    transcript: &str,
) -> Result<(), String> {
    writer
        .write_all(b"\n")
        .and_then(|()| writer.write_all(transcript.as_bytes()))
        .and_then(|()| {
            if transcript.ends_with('\n') {
                Ok(())
            } else {
                writer.write_all(b"\n")
            }
        })
        .and_then(|()| writer.write_all(b"\nPress Enter to return to ca.\n"))
        .and_then(|()| writer.flush())
        .map_err(|_| "Raw transcript could not be displayed".to_string())?;

    let mut byte = [0_u8; 1];
    loop {
        match reader.read(&mut byte) {
            Ok(0) => return Err("Raw transcript could not read Enter".into()),
            Ok(_) if matches!(byte[0], b'\n' | b'\r') => return Ok(()),
            Ok(_) => {}
            Err(_) => return Err("Raw transcript could not read Enter".into()),
        }
    }
}

pub(crate) fn present_stdio(transcript: &str) -> Result<(), String> {
    let stdout = std::io::stdout();
    let mut output = stdout.lock();
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    present(&mut output, &mut input, transcript)
}

fn push_section(output: &mut String, label: &str, body: &str) {
    output.push('\n');
    output.push_str(label);
    output.push('\n');
    output.push_str(&"-".repeat(label.chars().count()));
    output.push('\n');
    output.push_str(if body.is_empty() {
        "[Empty message]"
    } else {
        body
    });
    output.push('\n');
}

fn safe_visible_text(text: &str, configured_secrets: &[&str]) -> String {
    let controls_removed = text
        .chars()
        .any(|character| character.is_control() && !matches!(character, '\n' | '\t'));
    let mut lines = Vec::new();
    for line in text.split('\n') {
        if contains_embedded_media(line) {
            lines.push("[Binary/media payload omitted]".to_string());
            continue;
        }
        let safe = crate::request::safe_text(line, configured_secrets);
        lines.push(redact_secret_shaped_tokens(&safe));
    }
    let mut safe = lines.join("\n");
    if controls_removed {
        if !safe.is_empty() && !safe.ends_with('\n') {
            safe.push('\n');
        }
        safe.push_str("[Terminal control sequences removed]");
    }
    safe
}

fn contains_embedded_media(line: &str) -> bool {
    let lowercase = line.to_ascii_lowercase();
    [
        "data:image/",
        "data:audio/",
        "data:video/",
        "data:application/octet-stream",
    ]
    .iter()
    .any(|marker| lowercase.contains(marker))
}

fn redact_secret_shaped_tokens(line: &str) -> String {
    let mut output = Vec::new();
    for token in line.split_inclusive(char::is_whitespace) {
        let bare = token.trim_end_matches(char::is_whitespace);
        let suffix = &token[bare.len()..];
        if is_secret_shaped(bare) {
            output.push(format!("[REDACTED]{suffix}"));
        } else {
            output.push(token.to_string());
        }
    }
    output.concat()
}

fn is_secret_shaped(token: &str) -> bool {
    let trimmed = token.trim_matches(|value: char| {
        matches!(
            value,
            '"' | '\'' | '`' | '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';'
        )
    });
    (trimmed.starts_with("sk-") && trimmed.len() >= 12)
        || (trimmed.starts_with("ghp_") && trimmed.len() >= 12)
        || (trimmed.starts_with("xoxb-") && trimmed.len() >= 12)
        || (trimmed.starts_with("AKIA") && trimmed.len() >= 12)
}

fn bounded_tool_name(name: &str, configured_secrets: &[&str]) -> String {
    let safe = crate::request::safe_text(name, configured_secrets);
    let mut characters = safe.chars();
    let mut bounded = characters
        .by_ref()
        .take(MAX_TOOL_NAME_CHARS)
        .collect::<String>();
    if characters.next().is_some() {
        bounded.push('…');
    }
    if bounded.is_empty() {
        "[unnamed tool]".into()
    } else {
        bounded
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::provider::ProviderProtocol;
    use crate::agent::stream::{ProviderResponseItem, ProviderStateSourceV1};
    use crate::app::{Message, Role, ToolCall, ToolStatus};
    use chrono::Utc;
    use std::io::{Cursor, Error, ErrorKind};

    fn message(role: Role, content: &str) -> Message {
        Message {
            role,
            content: content.into(),
            tool_calls: None,
            thinking: None,
            response_items: Vec::new(),
            timestamp: Utc::now(),
        }
    }

    #[test]
    fn raw_transcript_preserves_visible_order_and_uses_explicit_omissions() {
        let mut assistant = message(Role::Assistant, "I inspected it.");
        assistant.thinking = Some("private chain of thought".into());
        assistant.response_items = vec![ProviderResponseItem::ChatReasoning {
            source: ProviderStateSourceV1::for_test("test", ProviderProtocol::OpenAiResponses),
            content: "provider-only state".into(),
        }];
        assistant.tool_calls = Some(vec![
            ToolCall {
                id: "call-1".into(),
                name: "read_file".into(),
                arguments: r#"{"path":"/tmp/private"}"#.into(),
                status: ToolStatus::Done,
                output: Some("file body".into()),
            },
            ToolCall {
                id: "call-2".into(),
                name: "email_read".into(),
                arguments: "private email arguments".into(),
                status: ToolStatus::Error,
                output: Some("private email body".into()),
            },
        ]);
        let messages = vec![
            message(Role::System, "secret system prompt"),
            message(Role::User, "Please inspect it."),
            assistant,
            message(Role::Tool, "legacy tool body"),
        ];

        let output = render(&messages, &[]);

        assert!(output.starts_with("ca raw transcript\n================="));
        let system = output.find("[System prompt omitted]").unwrap();
        let user = output.find("Please inspect it.").unwrap();
        let assistant = output.find("I inspected it.").unwrap();
        let read_file = output.find("Tool: read_file (done)").unwrap();
        let email = output.find("Tool: email_read (error)").unwrap();
        let legacy = output.find("[Legacy tool message omitted]").unwrap();
        assert!(system < user && user < assistant && assistant < read_file);
        assert!(read_file < email && email < legacy);
        assert!(output.contains("[Hidden reasoning omitted]"));
        assert!(output.contains("[Provider-only response items omitted]"));
        assert!(output.contains("[Tool arguments omitted]"));
        assert!(output.contains("[Tool output omitted]"));
        assert!(output.contains("[External private email content omitted]"));
        for forbidden in [
            "secret system prompt",
            "private chain of thought",
            "provider-only state",
            "/tmp/private",
            "file body",
            "private email arguments",
            "private email body",
            "legacy tool body",
        ] {
            assert!(!output.contains(forbidden), "leaked {forbidden:?}");
        }
    }

    #[test]
    fn raw_transcript_sanitizes_controls_credentials_and_media() {
        let messages = vec![
            message(
                Role::User,
                "hello\u{1b}]0;stolen\u{7}\napi_key=literal-secret\nknown-value",
            ),
            message(
                Role::Assistant,
                "data:image/png;base64,AAECAwQ=\nAuthorization: Bearer abc",
            ),
        ];

        let output = render(&messages, &["known-value"]);

        assert!(output.contains("[REDACTED]"));
        assert!(output.contains("[Terminal control sequences removed]"));
        assert!(output.contains("[Binary/media payload omitted]"));
        for forbidden in [
            "\u{1b}",
            "literal-secret",
            "known-value",
            "Bearer abc",
            "AAECAwQ=",
        ] {
            assert!(!output.contains(forbidden), "leaked {forbidden:?}");
        }
    }

    #[test]
    fn raw_transcript_bounds_untrusted_tool_names_by_unicode_scalars() {
        let mut assistant = message(Role::Assistant, "done");
        assistant.tool_calls = Some(vec![ToolCall {
            id: "call".into(),
            name: "界".repeat(300),
            arguments: String::new(),
            status: ToolStatus::Running,
            output: None,
        }]);

        let output = render(&[assistant], &[]);
        let tool_line = output
            .lines()
            .find(|line| line.starts_with("Tool: "))
            .unwrap();
        assert!(tool_line.chars().count() <= 150);
        assert!(tool_line.contains('…'));
        assert!(output.contains("[Tool output unavailable]"));
    }

    #[test]
    fn raw_transcript_presenter_prints_then_waits_for_enter() {
        let mut output = Vec::new();
        let mut input = Cursor::new(b"ignored\nremaining".to_vec());

        present(&mut output, &mut input, "transcript without newline").unwrap();

        assert_eq!(
            String::from_utf8(output).unwrap(),
            "\ntranscript without newline\n\nPress Enter to return to ca.\n"
        );
        assert_eq!(input.position(), 8);
    }

    #[test]
    fn raw_transcript_presenter_reports_output_and_input_failures() {
        struct BrokenWriter;
        impl Write for BrokenWriter {
            fn write(&mut self, _buffer: &[u8]) -> std::io::Result<usize> {
                Err(Error::new(ErrorKind::BrokenPipe, "no output"))
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        struct BrokenReader;
        impl Read for BrokenReader {
            fn read(&mut self, _buffer: &mut [u8]) -> std::io::Result<usize> {
                Err(Error::new(ErrorKind::BrokenPipe, "no input"))
            }
        }

        assert_eq!(
            present(&mut BrokenWriter, &mut Cursor::new(b"\n"), "x"),
            Err("Raw transcript could not be displayed".into())
        );
        assert_eq!(
            present(&mut Vec::new(), &mut BrokenReader, "x"),
            Err("Raw transcript could not read Enter".into())
        );
    }
}
