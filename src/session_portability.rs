use crate::app::{Message, Role, ToolCall, ToolStatus};
use crate::session::SessionV2;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub const PORTABLE_SCHEMA: &str = "ca.session-export.v1";
pub const MAX_PORTABLE_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_PORTABLE_MESSAGES: usize = 4_096;
const MAX_TITLE_BYTES: usize = 1_024;
const MAX_MESSAGE_BYTES: usize = 512 * 1024;
const MAX_THINKING_BYTES: usize = 512 * 1024;
const MAX_TOOL_CALLS_PER_MESSAGE: usize = 128;
const MAX_TOOL_ID_BYTES: usize = 256;
const MAX_TOOL_NAME_BYTES: usize = 256;
const MAX_TOOL_ARGUMENT_BYTES: usize = 512 * 1024;
const MAX_TOOL_OUTPUT_BYTES: usize = 512 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Markdown,
    Json,
}

#[derive(Clone)]
pub struct ImportDefaults {
    pub workspace: String,
    pub provider: String,
    pub model: String,
    pub reasoning_effort: Option<crate::config::ReasoningEffort>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportWriteOutcome {
    Written,
    NeedsOverwrite,
}

#[derive(Debug, thiserror::Error)]
pub enum PortabilityError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Invalid(String),
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PortableSession {
    created_at: DateTime<Utc>,
    messages: Vec<PortableMessage>,
    schema: String,
    source_model: String,
    source_provider: String,
    title: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PortableMessage {
    content: String,
    role: String,
    thinking: Option<String>,
    timestamp: DateTime<Utc>,
    tool_calls: Option<Vec<PortableToolCall>>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PortableToolCall {
    arguments: String,
    id: String,
    name: String,
    output: Option<String>,
    status: String,
}

pub fn encode_session(
    session: &SessionV2,
    format: ExportFormat,
) -> Result<Vec<u8>, PortabilityError> {
    validate_session_for_export(session)?;
    let portable = PortableSession {
        schema: PORTABLE_SCHEMA.into(),
        title: session.title.clone(),
        source_provider: session.provider.clone(),
        source_model: session.model.clone(),
        created_at: session.created_at,
        messages: session.messages.iter().map(PortableMessage::from).collect(),
    };
    let mut bytes = match format {
        ExportFormat::Json => serde_json::to_vec_pretty(&portable)?,
        ExportFormat::Markdown => render_markdown(&portable).into_bytes(),
    };
    if matches!(format, ExportFormat::Json) {
        bytes.push(b'\n');
    }
    validate_total_size(bytes.len())?;
    Ok(bytes)
}

pub fn import_session_bytes(
    bytes: &[u8],
    defaults: ImportDefaults,
) -> Result<SessionV2, PortabilityError> {
    validate_total_size(bytes.len())?;
    let portable: PortableSession = serde_json::from_slice(bytes)?;
    if portable.schema != PORTABLE_SCHEMA {
        return Err(PortabilityError::Invalid(format!(
            "unsupported session schema: {}",
            crate::terminal_text::sanitize_terminal_text(&portable.schema)
        )));
    }
    if portable.messages.len() > MAX_PORTABLE_MESSAGES {
        return Err(PortabilityError::Invalid(format!(
            "portable session has more than {MAX_PORTABLE_MESSAGES} messages"
        )));
    }
    validate_field("title", &portable.title, MAX_TITLE_BYTES)?;

    let mut seen_tool_ids = HashSet::new();
    let mut messages = Vec::with_capacity(portable.messages.len());
    for (message_index, portable_message) in portable.messages.into_iter().enumerate() {
        messages.push(import_message(
            portable_message,
            message_index,
            &mut seen_tool_ids,
        )?);
    }
    crate::agent::AgentClient::build_messages(None, &messages, None, &[])
        .map_err(|error| PortabilityError::Invalid(format!("invalid conversation: {error}")))?;

    let mut imported = SessionV2::new(
        sanitize(&portable.title),
        defaults.workspace,
        defaults.provider,
        defaults.model,
        messages,
    );
    imported.reasoning_effort = defaults.reasoning_effort;
    Ok(imported)
}

pub fn write_export(
    path: &Path,
    bytes: &[u8],
    overwrite: bool,
) -> Result<ExportWriteOutcome, PortabilityError> {
    validate_total_size(bytes.len())?;
    let parent = path
        .parent()
        .ok_or_else(|| PortabilityError::Invalid("export path has no parent directory".into()))?;
    if !parent.is_dir() {
        return Err(PortabilityError::Invalid(
            "export parent directory does not exist".into(),
        ));
    }
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| PortabilityError::Invalid("export path needs a UTF-8 filename".into()))?;
    if file_name.is_empty() {
        return Err(PortabilityError::Invalid(
            "export path needs a filename".into(),
        ));
    }

    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(PortabilityError::Invalid(
                "refusing to replace a symbolic-link export target".into(),
            ));
        }
        Ok(metadata) if !metadata.is_file() => {
            return Err(PortabilityError::Invalid(
                "export target is not a regular file".into(),
            ));
        }
        Ok(_) if !overwrite => return Ok(ExportWriteOutcome::NeedsOverwrite),
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }

    let temporary = parent.join(format!(".{file_name}.{}.tmp", Uuid::new_v4()));
    let write_result = (|| -> Result<ExportWriteOutcome, PortabilityError> {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);

        if overwrite {
            std::fs::rename(&temporary, path)?;
            Ok(ExportWriteOutcome::Written)
        } else {
            match std::fs::hard_link(&temporary, path) {
                Ok(()) => {
                    std::fs::remove_file(&temporary)?;
                    Ok(ExportWriteOutcome::Written)
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    Ok(ExportWriteOutcome::NeedsOverwrite)
                }
                Err(error) => Err(error.into()),
            }
        }
    })();
    if temporary.exists() {
        let _ = std::fs::remove_file(&temporary);
    }
    write_result
}

pub fn read_import(path: &Path) -> Result<Vec<u8>, PortabilityError> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(PortabilityError::Invalid(
            "import source is not a regular file".into(),
        ));
    }
    if metadata.len() > MAX_PORTABLE_BYTES as u64 {
        return Err(PortabilityError::Invalid(
            "portable session exceeds the 4 MiB limit".into(),
        ));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take((MAX_PORTABLE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    validate_total_size(bytes.len())?;
    Ok(bytes)
}

pub fn resolve_path(path: &str, cwd: &Path) -> PathBuf {
    let path = Path::new(path);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    }
}

impl From<&Message> for PortableMessage {
    fn from(message: &Message) -> Self {
        Self {
            role: match message.role {
                Role::System => "system",
                Role::User => "user",
                Role::Assistant => "assistant",
                Role::Tool => "tool",
            }
            .into(),
            content: message.content.clone(),
            thinking: message.thinking.clone(),
            timestamp: message.timestamp,
            tool_calls: message
                .tool_calls
                .as_ref()
                .map(|calls| calls.iter().map(PortableToolCall::from).collect()),
        }
    }
}

impl From<&ToolCall> for PortableToolCall {
    fn from(call: &ToolCall) -> Self {
        let private = crate::email::is_email_tool(&call.name);
        Self {
            id: call.id.clone(),
            name: call.name.clone(),
            arguments: if private {
                crate::email::private_arguments_marker(&call.name)
            } else {
                call.arguments.clone()
            },
            status: match call.status {
                ToolStatus::Pending => "pending",
                ToolStatus::Running => "running",
                ToolStatus::Done => "done",
                ToolStatus::Error => "error",
            }
            .into(),
            output: if private {
                call.output
                    .as_ref()
                    .map(|_| crate::email::private_result_marker(&call.name))
            } else {
                call.output.clone()
            },
        }
    }
}

fn validate_session_for_export(session: &SessionV2) -> Result<(), PortabilityError> {
    if session.messages.len() > MAX_PORTABLE_MESSAGES {
        return Err(PortabilityError::Invalid(format!(
            "session has more than {MAX_PORTABLE_MESSAGES} messages"
        )));
    }
    validate_field("title", &session.title, MAX_TITLE_BYTES)?;
    for (index, message) in session.messages.iter().enumerate() {
        validate_field(
            &format!("message {} content", index + 1),
            &message.content,
            MAX_MESSAGE_BYTES,
        )?;
        if let Some(thinking) = &message.thinking {
            validate_field(
                &format!("message {} thinking", index + 1),
                thinking,
                MAX_THINKING_BYTES,
            )?;
        }
        let calls = message.tool_calls.as_deref().unwrap_or_default();
        if calls.len() > MAX_TOOL_CALLS_PER_MESSAGE {
            return Err(PortabilityError::Invalid(format!(
                "message {} has more than {MAX_TOOL_CALLS_PER_MESSAGE} tool calls",
                index + 1
            )));
        }
        for call in calls {
            validate_tool_fields(call)?;
        }
    }
    Ok(())
}

fn import_message(
    portable: PortableMessage,
    index: usize,
    seen_tool_ids: &mut HashSet<String>,
) -> Result<Message, PortabilityError> {
    let role = match portable.role.as_str() {
        "user" => Role::User,
        "assistant" => Role::Assistant,
        "system" => {
            return Err(PortabilityError::Invalid(format!(
                "message {} uses a system role, which cannot be imported",
                index + 1
            )));
        }
        "tool" => {
            return Err(PortabilityError::Invalid(format!(
                "message {} uses a legacy tool role, which cannot be imported",
                index + 1
            )));
        }
        other => {
            return Err(PortabilityError::Invalid(format!(
                "message {} has invalid role {}",
                index + 1,
                crate::terminal_text::sanitize_terminal_text(other)
            )));
        }
    };
    validate_field("message content", &portable.content, MAX_MESSAGE_BYTES)?;
    if let Some(thinking) = &portable.thinking {
        validate_field("message thinking", thinking, MAX_THINKING_BYTES)?;
    }
    let calls = portable.tool_calls.unwrap_or_default();
    if !calls.is_empty() && role != Role::Assistant {
        return Err(PortabilityError::Invalid(format!(
            "message {} attaches tool calls to a non-assistant role",
            index + 1
        )));
    }
    if calls.len() > MAX_TOOL_CALLS_PER_MESSAGE {
        return Err(PortabilityError::Invalid(format!(
            "message {} has more than {MAX_TOOL_CALLS_PER_MESSAGE} tool calls",
            index + 1
        )));
    }
    let mut imported_calls = Vec::with_capacity(calls.len());
    for call in calls {
        let status = match call.status.as_str() {
            "done" => ToolStatus::Done,
            "error" => ToolStatus::Error,
            "pending" | "running" => {
                return Err(PortabilityError::Invalid(format!(
                    "unresolved tool call {} cannot be imported",
                    crate::terminal_text::sanitize_terminal_text(&call.id)
                )));
            }
            other => {
                return Err(PortabilityError::Invalid(format!(
                    "tool call has invalid status {}",
                    crate::terminal_text::sanitize_terminal_text(other)
                )));
            }
        };
        validate_portable_tool_fields(&call)?;
        if call.output.is_none() {
            return Err(PortabilityError::Invalid(format!(
                "unresolved tool call {} has no result",
                crate::terminal_text::sanitize_terminal_text(&call.id)
            )));
        }
        if !seen_tool_ids.insert(call.id.clone()) {
            return Err(PortabilityError::Invalid(format!(
                "duplicate tool call ID {}",
                crate::terminal_text::sanitize_terminal_text(&call.id)
            )));
        }
        let arguments: serde_json::Value = serde_json::from_str(&call.arguments).map_err(|_| {
            PortabilityError::Invalid(format!(
                "tool call {} arguments are not valid JSON",
                crate::terminal_text::sanitize_terminal_text(&call.id)
            ))
        })?;
        if !arguments.is_object() {
            return Err(PortabilityError::Invalid(format!(
                "tool call {} arguments must be a JSON object",
                crate::terminal_text::sanitize_terminal_text(&call.id)
            )));
        }
        imported_calls.push(ToolCall {
            id: sanitize(&call.id),
            name: sanitize(&call.name),
            arguments: sanitize(&call.arguments),
            status,
            output: call.output.map(|output| sanitize(&output)),
        });
    }

    Ok(Message {
        role,
        content: sanitize(&portable.content),
        tool_calls: (!imported_calls.is_empty()).then_some(imported_calls),
        thinking: portable.thinking.map(|thinking| sanitize(&thinking)),
        timestamp: portable.timestamp,
        response_items: Vec::new(),
    })
}

fn validate_tool_fields(call: &ToolCall) -> Result<(), PortabilityError> {
    validate_field("tool call ID", &call.id, MAX_TOOL_ID_BYTES)?;
    validate_field("tool name", &call.name, MAX_TOOL_NAME_BYTES)?;
    validate_field("tool arguments", &call.arguments, MAX_TOOL_ARGUMENT_BYTES)?;
    if let Some(output) = &call.output {
        validate_field("tool output", output, MAX_TOOL_OUTPUT_BYTES)?;
    }
    Ok(())
}

fn validate_portable_tool_fields(call: &PortableToolCall) -> Result<(), PortabilityError> {
    validate_field("tool call ID", &call.id, MAX_TOOL_ID_BYTES)?;
    validate_field("tool name", &call.name, MAX_TOOL_NAME_BYTES)?;
    validate_field("tool arguments", &call.arguments, MAX_TOOL_ARGUMENT_BYTES)?;
    if let Some(output) = &call.output {
        validate_field("tool output", output, MAX_TOOL_OUTPUT_BYTES)?;
    }
    Ok(())
}

fn validate_field(label: &str, value: &str, limit: usize) -> Result<(), PortabilityError> {
    if value.len() > limit {
        Err(PortabilityError::Invalid(format!(
            "{label} exceeds the {limit}-byte limit"
        )))
    } else {
        Ok(())
    }
}

fn validate_total_size(size: usize) -> Result<(), PortabilityError> {
    if size > MAX_PORTABLE_BYTES {
        Err(PortabilityError::Invalid(
            "portable session exceeds the 4 MiB limit".into(),
        ))
    } else {
        Ok(())
    }
}

fn sanitize(value: &str) -> String {
    crate::terminal_text::sanitize_terminal_text(value)
}

fn render_markdown(session: &PortableSession) -> String {
    let mut output = String::new();
    output.push_str("# ");
    output.push_str(&sanitize(&session.title).replace('\n', " "));
    output.push_str("\n\n");
    output.push_str(&format!(
        "- Source model: `{}/{}`\n- Created: {}\n- Format: `{PORTABLE_SCHEMA}`\n\n",
        sanitize(&session.source_provider).replace('`', "\\`"),
        sanitize(&session.source_model).replace('`', "\\`"),
        session.created_at.to_rfc3339()
    ));
    for message in &session.messages {
        let role = match message.role.as_str() {
            "user" => "User",
            "assistant" => "Assistant",
            "system" => "System",
            "tool" => "Tool",
            _ => "Message",
        };
        output.push_str("## ");
        output.push_str(role);
        output.push_str("\n\n");
        output.push_str(&sanitize(&message.content));
        output.push_str("\n\n");
        if let Some(thinking) = &message.thinking {
            output.push_str("### Reasoning\n\n");
            push_indented(&mut output, &sanitize(thinking));
        }
        if let Some(calls) = &message.tool_calls {
            for call in calls {
                output.push_str("### Tool: ");
                output.push_str(&sanitize(&call.name).replace('\n', " "));
                output.push_str("\n\n");
                output.push_str(&format!("Status: `{}`\n\nArguments:\n\n", call.status));
                push_indented(&mut output, &sanitize(&call.arguments));
                output.push_str("Result:\n\n");
                match &call.output {
                    Some(result) => push_indented(&mut output, &sanitize(result)),
                    None => output.push_str("    [unresolved tool call]\n\n"),
                }
            }
        }
    }
    output
}

fn push_indented(output: &mut String, value: &str) {
    for line in value.lines() {
        output.push_str("    ");
        output.push_str(line);
        output.push('\n');
    }
    if value.is_empty() {
        output.push_str("    \n");
    }
    output.push('\n');
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::provider::ProviderProtocol;
    use crate::agent::stream::{ProviderResponseItem, ProviderStateSourceV1};
    use crate::app::{Message, Role, ToolCall, ToolStatus};
    use crate::queue::PendingInput;
    use crate::session::{SessionParent, SessionV2};
    use chrono::Utc;
    use serde_json::Value;
    use tempfile::tempdir;
    use uuid::Uuid;

    fn message(role: Role, content: &str) -> Message {
        Message {
            role,
            content: content.into(),
            tool_calls: None,
            thinking: None,
            timestamp: Utc::now(),
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

    fn resolved_session() -> SessionV2 {
        let mut session = SessionV2::new(
            "Portable",
            "/private/workspace",
            "source-provider",
            "source-model",
            vec![message(Role::User, "hello"), message(Role::Assistant, "hi")],
        );
        session.parent = Some(SessionParent {
            session_id: Uuid::new_v4(),
            branch_turn: 3,
        });
        session.pinned = true;
        session.workspace_roots.push("/secret/root".into());
        session.queued_inputs.push(PendingInput::new("do later"));
        session
    }

    fn defaults() -> ImportDefaults {
        ImportDefaults {
            workspace: "/safe/current".into(),
            provider: "safe-provider".into(),
            model: "safe-model".into(),
            reasoning_effort: None,
        }
    }

    #[test]
    fn normalized_json_excludes_internal_runtime_and_lineage_fields() {
        let source = resolved_session();
        let bytes = encode_session(&source, ExportFormat::Json).unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        let object = value.as_object().unwrap();
        assert_eq!(object.get("schema").unwrap(), PORTABLE_SCHEMA);
        assert_eq!(
            object.keys().cloned().collect::<Vec<_>>(),
            vec![
                "created_at",
                "messages",
                "schema",
                "source_model",
                "source_provider",
                "title"
            ]
        );
        for forbidden in [
            "id",
            "workspace",
            "parent",
            "pinned",
            "queued_inputs",
            "active_skills",
            "work_state",
            "review_records",
            "agent_summaries",
            "context_checkpoint",
            "import_provenance",
        ] {
            assert!(object.get(forbidden).is_none(), "found {forbidden}");
        }
    }

    #[test]
    fn portable_json_and_markdown_omit_opaque_provider_state() {
        let sentinel = "opaque-portable-provider-state-sentinel";
        let mut source = resolved_session();
        source.messages[1].response_items =
            vec![provider_state(sentinel), responses_provider_state(sentinel)];

        let json = encode_session(&source, ExportFormat::Json).unwrap();
        let markdown = encode_session(&source, ExportFormat::Markdown).unwrap();
        for output in [&json, &markdown] {
            let text = std::str::from_utf8(output).unwrap();
            assert!(text.contains("hi"));
            assert!(!text.contains(sentinel));
            assert!(!text.contains("response_items"));
        }

        let value: Value = serde_json::from_slice(&json).unwrap();
        assert!(value["messages"][1].get("response_items").is_none());
        let imported = import_session_bytes(&json, defaults()).unwrap();
        assert!(imported.messages[1].response_items.is_empty());
    }

    #[test]
    fn portable_exports_omit_session_artifact_ledger() {
        let temp = tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let workspace =
            crate::workspace::WorkspaceContext::from_primary(root.clone(), root.clone());
        let mut source = resolved_session();
        let receipt =
            crate::undo::FileMutationBatchReceipt::single(crate::undo::FileMutationReceipt::new(
                root.join("artifact-path-sentinel.txt"),
                "write_file",
                crate::undo::FileState::Missing,
                crate::undo::FileState::Utf8("artifact-content-sentinel".into()),
            ));
        source
            .artifacts
            .record_mutation_batch(&workspace, &receipt, 2, Utc::now());

        for bytes in [
            encode_session(&source, ExportFormat::Json).unwrap(),
            encode_session(&source, ExportFormat::Markdown).unwrap(),
        ] {
            let text = String::from_utf8(bytes).unwrap();
            assert!(!text.contains("ca.session-artifacts.v1"));
            assert!(!text.contains("artifact-path-sentinel"));
            assert!(!text.contains("artifact-content-sentinel"));
        }
    }

    #[test]
    fn portable_formats_scrub_external_private_email() {
        let sentinel = "portable-private-email-sentinel";
        let mut source = resolved_session();
        source.messages[1].tool_calls = Some(vec![ToolCall {
            id: "email-call".into(),
            name: "email_read".into(),
            arguments: serde_json::json!({"body": sentinel}).to_string(),
            status: ToolStatus::Done,
            output: Some(sentinel.into()),
        }]);
        for bytes in [
            encode_session(&source, ExportFormat::Json).unwrap(),
            encode_session(&source, ExportFormat::Markdown).unwrap(),
        ] {
            let text = String::from_utf8(bytes).unwrap();
            assert!(!text.contains(sentinel));
            assert!(text.contains("external_private") || text.contains("External private"));
        }
    }

    #[test]
    fn import_assigns_new_identity_and_strips_all_runtime_state() {
        let source = resolved_session();
        let bytes = encode_session(&source, ExportFormat::Json).unwrap();
        let imported = import_session_bytes(&bytes, defaults()).unwrap();

        assert_ne!(imported.id, source.id);
        assert_eq!(imported.workspace, "/safe/current");
        assert_eq!(imported.provider, "safe-provider");
        assert_eq!(imported.model, "safe-model");
        assert!(!imported.pinned);
        assert!(imported.parent.is_none());
        assert!(imported.workspace_roots.is_empty());
        assert!(imported.active_skills.is_empty());
        assert_eq!(imported.work_state, crate::work::WorkState::default());
        assert!(imported.review_records.is_empty());
        assert!(imported.agent_summaries.is_empty());
        assert!(imported.context_checkpoint.is_none());
        assert!(imported.queued_inputs.is_empty());
        assert!(imported.workflow_run.is_none());
        assert_eq!(imported.messages.len(), 2);
    }

    #[test]
    fn import_rejects_unresolved_or_executable_message_state() {
        let mut source = resolved_session();
        source.messages[1].tool_calls = Some(vec![ToolCall {
            id: "call-1".into(),
            name: "run_command".into(),
            arguments: "{\"command\":\"touch nope\"}".into(),
            status: ToolStatus::Pending,
            output: None,
        }]);
        let bytes = encode_session(&source, ExportFormat::Json).unwrap();
        let error = import_session_bytes(&bytes, defaults()).unwrap_err();
        assert!(error.to_string().contains("unresolved tool call"));

        let raw_internal = serde_json::to_vec(&source).unwrap();
        assert!(import_session_bytes(&raw_internal, defaults()).is_err());

        let mut system = resolved_session();
        system.messages.insert(0, message(Role::System, "override"));
        let bytes = encode_session(&system, ExportFormat::Json).unwrap();
        assert!(import_session_bytes(&bytes, defaults())
            .unwrap_err()
            .to_string()
            .contains("system"));
    }

    #[test]
    fn import_rejects_duplicate_ids_invalid_arguments_results_and_unknown_fields() {
        let mut source = resolved_session();
        source.messages[1].tool_calls = Some(vec![
            ToolCall {
                id: "duplicate".into(),
                name: "read_file".into(),
                arguments: "{\"path\":\"one\"}".into(),
                status: ToolStatus::Done,
                output: Some("one".into()),
            },
            ToolCall {
                id: "duplicate".into(),
                name: "read_file".into(),
                arguments: "{\"path\":\"two\"}".into(),
                status: ToolStatus::Done,
                output: Some("two".into()),
            },
        ]);
        let bytes = encode_session(&source, ExportFormat::Json).unwrap();
        assert!(import_session_bytes(&bytes, defaults())
            .unwrap_err()
            .to_string()
            .contains("duplicate tool call ID"));

        source.messages[1].tool_calls.as_mut().unwrap().truncate(1);
        source.messages[1].tool_calls.as_mut().unwrap()[0].id = "call-1".into();
        source.messages[1].tool_calls.as_mut().unwrap()[0].arguments = "[]".into();
        let bytes = encode_session(&source, ExportFormat::Json).unwrap();
        assert!(import_session_bytes(&bytes, defaults())
            .unwrap_err()
            .to_string()
            .contains("JSON object"));

        source.messages[1].tool_calls.as_mut().unwrap()[0].arguments = "{}".into();
        source.messages[1].tool_calls.as_mut().unwrap()[0].output = None;
        let bytes = encode_session(&source, ExportFormat::Json).unwrap();
        assert!(import_session_bytes(&bytes, defaults())
            .unwrap_err()
            .to_string()
            .contains("no result"));

        let mut value: Value = serde_json::from_slice(
            &encode_session(&resolved_session(), ExportFormat::Json).unwrap(),
        )
        .unwrap();
        value
            .as_object_mut()
            .unwrap()
            .insert("workspace".into(), Value::String("/unsafe".into()));
        assert!(import_session_bytes(&serde_json::to_vec(&value).unwrap(), defaults()).is_err());
    }

    #[test]
    fn import_sanitizes_terminal_controls_before_persistence() {
        let mut source = resolved_session();
        source.title = "bad\u{1b}]0;owned\u{7}title".into();
        source.messages[0].content = "hello\u{1b}[2Jworld".into();
        let bytes = encode_session(&source, ExportFormat::Json).unwrap();
        let imported = import_session_bytes(&bytes, defaults()).unwrap();
        assert!(!imported.title.contains('\u{1b}'));
        assert!(!imported.messages[0].content.contains('\u{1b}'));
        assert!(imported.messages[0].content.contains("world"));
    }

    #[test]
    fn bounds_are_rejected_without_partial_output() {
        let mut source = resolved_session();
        source.messages = (0..=MAX_PORTABLE_MESSAGES)
            .map(|index| message(Role::User, &format!("message {index}")))
            .collect();
        assert!(encode_session(&source, ExportFormat::Json)
            .unwrap_err()
            .to_string()
            .contains("messages"));

        let oversized = vec![b' '; MAX_PORTABLE_BYTES + 1];
        assert!(import_session_bytes(&oversized, defaults())
            .unwrap_err()
            .to_string()
            .contains("4 MiB"));
    }

    #[test]
    fn markdown_is_bounded_and_labels_roles_and_tool_results() {
        let mut source = resolved_session();
        source.messages[1].tool_calls = Some(vec![ToolCall {
            id: "call-1".into(),
            name: "read_file".into(),
            arguments: "{\"path\":\"README.md\"}".into(),
            status: ToolStatus::Done,
            output: Some("contents".into()),
        }]);
        let text =
            String::from_utf8(encode_session(&source, ExportFormat::Markdown).unwrap()).unwrap();
        assert!(text.starts_with("# Portable\n"));
        assert!(text.contains("## User\n"));
        assert!(text.contains("## Assistant\n"));
        assert!(text.contains("Tool: read_file"));
        assert!(text.contains("contents"));
        assert!(text.len() <= MAX_PORTABLE_BYTES);
    }

    #[test]
    fn export_requires_confirmation_and_never_follows_symlink_targets() {
        let root = tempdir().unwrap();
        let target = root.path().join("session.json");
        std::fs::write(&target, b"original").unwrap();
        assert_eq!(
            write_export(&target, b"replacement", false).unwrap(),
            ExportWriteOutcome::NeedsOverwrite
        );
        assert_eq!(std::fs::read(&target).unwrap(), b"original");
        assert_eq!(
            write_export(&target, b"replacement", true).unwrap(),
            ExportWriteOutcome::Written
        );
        assert_eq!(std::fs::read(&target).unwrap(), b"replacement");

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let victim = root.path().join("victim");
            let link = root.path().join("linked.json");
            std::fs::write(&victim, b"victim").unwrap();
            symlink(&victim, &link).unwrap();
            assert!(write_export(&link, b"attack", true).is_err());
            assert_eq!(std::fs::read(victim).unwrap(), b"victim");
        }
    }

    #[test]
    fn import_reads_only_bounded_regular_files() {
        let root = tempdir().unwrap();
        let file = root.path().join("portable.json");
        std::fs::write(&file, b"{}").unwrap();
        assert_eq!(read_import(&file).unwrap(), b"{}");

        let oversized = root.path().join("oversized.json");
        std::fs::write(&oversized, vec![b'x'; MAX_PORTABLE_BYTES + 1]).unwrap();
        assert!(read_import(&oversized)
            .unwrap_err()
            .to_string()
            .contains("4 MiB"));

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let link = root.path().join("link.json");
            symlink(&file, &link).unwrap();
            assert!(read_import(&link).is_err());
        }
    }
}
