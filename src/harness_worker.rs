//! Credential-free coding worker protocol. The parent owns provider access,
//! pricing, deadlines, sandbox grants and all acceptance verification.
use crate::agent::client::{ChatBackend, ChatMessage, ChatStreamItem, ToolDef};
use crate::agent::stream::{ProviderError, ProviderEvent};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::mpsc;

pub const WORKER_FLAG: &str = "--internal-harness-worker";
pub const MAX_FRAME: usize = 4 * 1024 * 1024;
pub const CODING_TOOLS: &[&str] = &[
    "read_file",
    "write_file",
    "edit_file",
    "apply_patch",
    "run_command",
    "glob",
    "list_dir",
];

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerSetup {
    pub schema: String,
    pub run_id: uuid::Uuid,
    pub workspace: PathBuf,
    pub provider: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "std::collections::BTreeSet::is_empty")]
    pub hints: crate::harness_hints::HintPolicy,
    pub instructions: String,
    pub task: String,
    pub maximum_turns: usize,
    pub command_timeout_seconds: u64,
}
impl WorkerSetup {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != "ca.harness_worker.v1"
            || !(1..=24).contains(&self.maximum_turns)
            || !(1..=60).contains(&self.command_timeout_seconds)
            || self.instructions.len() > 128 * 1024
            || self.task.is_empty()
            || self.task.len() > 128 * 1024
            || self.provider.is_empty()
            || self.model.is_empty()
            || self.provider.len() > 128
            || self.model.len() > 256
            || !self.workspace.is_absolute()
        {
            return Err("invalid bounded harness worker setup".into());
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkerMessage {
    ProviderRequest {
        sequence: u64,
        messages: Vec<serde_json::Value>,
        tools: Vec<serde_json::Value>,
    },
    Completed {
        exit_code: i32,
        final_message: Option<String>,
        failure: Option<String>,
    },
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderReply {
    pub sequence: u64,
    pub events: Vec<ProviderEvent>,
    pub failed: bool,
}

pub fn read_frame<R: Read, T: serde::de::DeserializeOwned>(reader: &mut R) -> std::io::Result<T> {
    let mut prefix = [0u8; 4];
    reader.read_exact(&mut prefix)?;
    let length = u32::from_be_bytes(prefix) as usize;
    if length == 0 || length > MAX_FRAME {
        return Err(std::io::Error::other("harness frame exceeds bound"));
    }
    let mut bytes = vec![0; length];
    reader.read_exact(&mut bytes)?;
    serde_json::from_slice(&bytes).map_err(|_| std::io::Error::other("invalid harness frame"))
}
pub fn write_frame<W: Write, T: Serialize>(writer: &mut W, value: &T) -> std::io::Result<()> {
    let bytes = serde_json::to_vec(value)
        .map_err(|_| std::io::Error::other("cannot encode harness frame"))?;
    if bytes.len() > MAX_FRAME {
        return Err(std::io::Error::other("harness frame exceeds bound"));
    }
    writer.write_all(&(bytes.len() as u32).to_be_bytes())?;
    writer.write_all(&bytes)?;
    writer.flush()
}

struct ParentBackend {
    hints: crate::harness_hints::HintPolicy,
    sequence: AtomicU64,
}
#[async_trait::async_trait]
impl ChatBackend for ParentBackend {
    async fn stream_chat(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDef],
    ) -> Result<mpsc::UnboundedReceiver<ChatStreamItem>, ProviderError> {
        let mut messages = messages.to_vec();
        crate::harness_hints::apply(&mut messages, &self.hints);
        let sequence = self.sequence.fetch_add(1, Ordering::SeqCst);
        let encode_error = || ProviderError::Protocol("harness request encoding failed".into());
        let request = WorkerMessage::ProviderRequest {
            sequence,
            messages: messages
                .iter()
                .map(serde_json::to_value)
                .collect::<Result<_, _>>()
                .map_err(|_| encode_error())?,
            tools: tools
                .iter()
                .map(serde_json::to_value)
                .collect::<Result<_, _>>()
                .map_err(|_| encode_error())?,
        };
        let reply = tokio::task::spawn_blocking(move || -> std::io::Result<ProviderReply> {
            write_frame(&mut std::io::stdout().lock(), &request)?;
            read_frame(&mut std::io::stdin().lock())
        })
        .await
        .map_err(|_| encode_error())?
        .map_err(|_| ProviderError::Protocol("harness parent disconnected".into()))?;
        if reply.sequence != sequence || reply.failed || reply.events.len() > 16384 {
            return Err(ProviderError::Protocol(
                "harness provider request did not complete".into(),
            ));
        }
        let (sender, receiver) = mpsc::unbounded_channel();
        for event in reply.events {
            let _ = sender.send(Ok(event));
        }
        Ok(receiver)
    }
}

struct BoundedOutput(Vec<u8>);
impl Write for BoundedOutput {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.0.len().saturating_add(bytes.len()) > MAX_FRAME {
            return Err(std::io::Error::other("worker output bound reached"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// No runtime settings or credential files are loaded on this path. The parent
/// launches this entire executable in a required, offline process sandbox.
pub async fn run_stdio() -> i32 {
    if crate::jobs::inherit_harness_worker_containment().is_err() {
        return 2;
    }
    let setup = match tokio::task::spawn_blocking(|| {
        read_frame::<_, WorkerSetup>(&mut std::io::stdin().lock())
    })
    .await
    {
        Ok(Ok(setup)) => setup,
        _ => return 2,
    };
    if setup.validate().is_err()
        || std::env::current_dir()
            .ok()
            .and_then(|p| p.canonicalize().ok())
            != setup.workspace.canonicalize().ok()
    {
        return 2;
    }
    let workspace = crate::workspace::WorkspaceContext::from_primary(
        setup.workspace.clone(),
        setup.workspace.clone(),
    );
    let message = |role: &str, text: &str| ChatMessage {
        role: role.into(),
        content: crate::image_content::ModelContent::text(text),
        tool_calls: None,
        tool_call_id: None,
        response_items: Vec::new(),
    };
    let messages = vec![
        message("system", &setup.instructions),
        message("user", &setup.task),
    ];
    let config = crate::headless::HeadlessRunConfig {
        native_mechanistic: false,
        proactive_delegation: false,
        review_changes: false,
        workspace_activity_scope: None,
        automation_policy: None,
        outcome_sink: None,
        provider: setup.provider,
        model: setup.model,
        cwd: setup.workspace,
        workspace,
        max_turns: setup.maximum_turns,
        single_turn: false,
        command_timeout_secs: setup.command_timeout_seconds,
        command_output_policy: crate::config::CommandOutputPolicy::Bounded,
        output_format: crate::headless::OutputFormat::Json,
        show_thinking: false,
        printenv: false,
        approval_mode: crate::headless::HeadlessApprovalMode::Yolo,
        approval_rules: Vec::new(),
        permission_rules: Vec::new(),
        session_grants: Vec::new(),
        allow_network: false,
        output_last_message: None,
        check_mode: crate::headless_contract::CheckMode::Disabled,
        convergence: None,
        structured_output: None,
        original_request: setup.task,
        verification_instructions: setup.instructions,
        chat_options: Default::default(),
        sandbox_mode: crate::sandbox::SandboxMode::PolicyOnly,
        run_id: setup.run_id,
        job_owner: crate::jobs::JobOwner::new(None),
        skills: std::sync::Arc::new(std::sync::Mutex::new(crate::skills::SkillRegistry::empty())),
        mcp: None,
        mechanistic: crate::config::MechanisticMcpConfig::default(),
        lsp: None,
        email: None,
        tool_policy: crate::tool_selection::ToolSelectionPolicy::replacing(CODING_TOOLS),
        tool_constraints: crate::tool_selection::ToolSelectionConstraints {
            allowed_names: Some(CODING_TOOLS.iter().map(|s| s.to_string()).collect()),
            ..Default::default()
        },
        tool_profile: crate::agent::registry::ToolProfile::Main,
        startup_warnings: Vec::new(),
        request_kind: crate::request::RequestKind::Headless,
        agents: None,
        work_state: std::sync::Arc::new(std::sync::Mutex::new(crate::work::WorkState::default())),
        role_run_workspace_identity: None,
        output_redactions: Default::default(),
    };
    let backend = ParentBackend {
        hints: setup.hints,
        sequence: AtomicU64::new(0),
    };
    let mut output = BoundedOutput(Vec::new());
    let result = crate::headless::run_with_backend(
        &backend,
        messages,
        config,
        &mut output,
        &mut std::io::sink(),
    )
    .await;
    let final_message = serde_json::from_slice::<serde_json::Value>(&output.0)
        .ok()
        .and_then(|v| {
            v.get("final_message")
                .and_then(|v| v.as_str())
                .map(|s| s.chars().take(32768).collect())
        });
    let failure = serde_json::from_slice::<serde_json::Value>(&output.0)
        .ok()
        .and_then(|value| {
            value
                .get("events")?
                .as_array()?
                .iter()
                .rev()
                .find_map(|event| {
                    let event = event.get("event")?;
                    if event.get("type")?.as_str()? != "failure" {
                        return None;
                    }
                    crate::learning::sanitized_learning_text(event.get("message")?.as_str()?)
                        .map(|text| text.chars().take(1024).collect::<String>())
                })
        });
    let response = WorkerMessage::Completed {
        exit_code: result.code,
        final_message,
        failure,
    };
    if write_frame(&mut std::io::stdout().lock(), &response).is_err() {
        return 8;
    }
    result.code
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hrs253_worker_frames_reject_truncation_oversize_and_extra_fields() {
        let message = WorkerMessage::Completed {
            exit_code: 0,
            final_message: Some("done".into()),
            failure: None,
        };
        let mut bytes = Vec::new();
        write_frame(&mut bytes, &message).unwrap();
        assert!(matches!(
            read_frame::<_, WorkerMessage>(&mut bytes.as_slice()).unwrap(),
            WorkerMessage::Completed { exit_code: 0, .. }
        ));
        assert!(read_frame::<_, WorkerMessage>(&mut &bytes[..bytes.len() - 1]).is_err());
        assert!(read_frame::<_, WorkerMessage>(&mut &(u32::MAX.to_be_bytes())[..]).is_err());
        let mut invalid = Vec::new();
        write_frame(&mut invalid,&serde_json::json!({"kind":"completed","exit_code":0,"final_message":null,"verified_pass":true})).unwrap();
        assert!(read_frame::<_, WorkerMessage>(&mut invalid.as_slice()).is_err());
    }
}
