use crate::agent::client::{ChatBackend, ChatMessage, ChatRequestOptions};
use crate::agent::registry::{
    ToolCapability, ToolExecutionResult, ToolHandler, ToolMutation, ToolOrigin, ToolProfile,
    ToolRegistry,
};
use crate::agent::stream::{
    AssembledToolCall, AssistantTurnAssembler, ProviderError, ProviderEvent,
};
use crate::app::{Message, Role, ToolCall, ToolStatus};
use crate::headless::{HeadlessEventV1, HeadlessFailureKind, ToolResultStatus};
use crate::interaction::{
    InteractionAnswer, InteractionChoice, InteractionMode, InteractionRequest, RequestUserInputArgs,
};
use crate::runtime::ActiveProfile;
use async_trait::async_trait;
use futures::{FutureExt, SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::io::Read;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinSet;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::handshake::server::{
    Callback as HandshakeCallback, ErrorResponse as HandshakeErrorResponse,
    Request as HandshakeRequest, Response as HandshakeResponse,
};
use tokio_tungstenite::tungstenite::protocol::{CloseFrame, WebSocketConfig};
use tokio_tungstenite::tungstenite::{http, Message as WebSocketMessage};
use tokio_tungstenite::{accept_hdr_async_with_config, connect_async_with_config, WebSocketStream};
use uuid::Uuid;

pub const SCHEMA: &str = "ca.serve.v1";
const MAX_LINE_BYTES: usize = 64 * 1024;
const MAX_OPERATION_IDS: usize = 4_096;
const MAX_ID_BYTES: usize = 128;
const MAX_PROMPT_BYTES: usize = 48 * 1024;
const MAX_ACP_PROMPT_BYTES: usize = MAX_PROMPT_BYTES + 128 * 1024 + 16 * 1024;
const MAX_SAFE_TEXT_BYTES: usize = 8 * 1024;
const MAX_SAFE_ARGUMENT_BYTES: usize = 8 * 1024;
const MAX_TOOL_NAMES: usize = 256;
const WORKER_CHANNEL_CAPACITY: usize = 256;
const CONTROL_CHANNEL_CAPACITY: usize = 4;
const TOKEN_MIN_BYTES: usize = 32;
const TOKEN_MAX_BYTES: usize = 1024;
const WEBSOCKET_HANDSHAKE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
const WEBSOCKET_CLOSE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

#[derive(Debug, Clone)]
pub struct ServiceCliArgs {
    pub transport: ServiceTransport,
    pub active_profile: ActiveProfile,
    pub allow_network: bool,
    pub trace: bool,
}

#[derive(Debug, Clone)]
pub enum ServiceTransport {
    Stdio,
    WebSocket {
        listen: SocketAddr,
        auth: AuthTokenSource,
    },
}

#[derive(Clone)]
pub enum AuthTokenSource {
    File(PathBuf),
    Environment(String),
}

impl std::fmt::Debug for AuthTokenSource {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::File(_) => formatter.write_str("AuthTokenSource::File(<protected>)"),
            Self::Environment(name) => formatter
                .debug_tuple("AuthTokenSource::Environment")
                .field(name)
                .finish(),
        }
    }
}

#[derive(Clone)]
pub struct AttachCliArgs {
    pub endpoint: String,
    pub auth: AuthTokenSource,
}

impl std::fmt::Debug for AttachCliArgs {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AttachCliArgs")
            .field("endpoint", &"<loopback>")
            .field("auth", &self.auth)
            .finish()
    }
}

struct SecretToken(Vec<u8>);

impl std::fmt::Debug for SecretToken {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("SecretToken(<redacted>)")
    }
}

impl Drop for SecretToken {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}

impl SecretToken {
    fn digest(&self) -> [u8; 32] {
        Sha256::digest(&self.0).into()
    }

    fn authorization_value(&self) -> Result<http::HeaderValue, String> {
        let mut value = Vec::with_capacity(7 + self.0.len());
        value.extend_from_slice(b"Bearer ");
        value.extend_from_slice(&self.0);
        let header = http::HeaderValue::from_bytes(&value)
            .map_err(|_| "authentication token contains unsupported bytes".into());
        value.fill(0);
        header
    }
}

pub fn parse_loopback_listen(value: &str) -> Result<SocketAddr, String> {
    let address = value
        .parse::<SocketAddr>()
        .map_err(|_| "listener must be a literal loopback IP and port".to_string())?;
    if !address.ip().is_loopback() {
        return Err("WebSocket listeners are restricted to literal loopback addresses".into());
    }
    Ok(address)
}

pub fn validate_attach_endpoint(value: &str) -> Result<String, String> {
    let authority = value
        .strip_prefix("ws://")
        .and_then(|rest| rest.split('/').next())
        .ok_or_else(|| "attach endpoint must be a valid ws:// loopback URL".to_string())?;
    let address = authority
        .parse::<SocketAddr>()
        .map_err(|_| "attach endpoint must contain a literal loopback IP and port".to_string())?;
    if !address.ip().is_loopback() {
        return Err("attach endpoint must use a literal loopback IP address".into());
    }
    let url = reqwest::Url::parse(value)
        .map_err(|_| "attach endpoint must be a valid ws:// loopback URL".to_string())?;
    if url.scheme() != "ws" {
        return Err("attach endpoint must use ws://; remote use requires an SSH tunnel".into());
    }
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(
            "attach endpoint cannot contain credentials, query parameters, or a fragment".into(),
        );
    }
    Ok(url.to_string())
}

pub fn auth_token_source(
    file: Option<PathBuf>,
    environment: Option<String>,
) -> Result<AuthTokenSource, String> {
    match (file, environment) {
        (Some(path), None) => Ok(AuthTokenSource::File(path)),
        (None, Some(name)) => {
            validate_environment_name(&name)?;
            Ok(AuthTokenSource::Environment(name))
        }
        (None, None) => {
            Err("exactly one protected token source is required: --auth-file or --auth-env".into())
        }
        (Some(_), Some(_)) => Err("--auth-file and --auth-env cannot be used together".into()),
    }
}

fn validate_environment_name(name: &str) -> Result<(), String> {
    let bytes = name.as_bytes();
    if bytes.is_empty()
        || bytes.len() > 128
        || !(bytes[0].is_ascii_uppercase() || bytes[0] == b'_')
        || !bytes
            .iter()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || *byte == b'_')
    {
        return Err(
            "authentication environment name must use uppercase ASCII letters, digits, and underscores"
                .into(),
        );
    }
    Ok(())
}

fn load_auth_token(source: &AuthTokenSource) -> Result<SecretToken, String> {
    let bytes = match source {
        AuthTokenSource::File(path) => read_protected_token_file(path)?,
        AuthTokenSource::Environment(name) => {
            validate_environment_name(name)?;
            std::env::var_os(name)
                .ok_or_else(|| format!("authentication environment variable {name} is missing"))?
                .to_string_lossy()
                .into_owned()
                .into_bytes()
        }
    };
    normalize_token(bytes)
}

fn read_protected_token_file(path: &Path) -> Result<Vec<u8>, String> {
    #[cfg(not(unix))]
    {
        let _ = path;
        return Err(
            "protected token files are unavailable on this platform; use --auth-env".into(),
        );
    }
    #[cfg(unix)]
    {
        let link_metadata = std::fs::symlink_metadata(path)
            .map_err(|_| "protected token file could not be opened".to_string())?;
        if link_metadata.file_type().is_symlink() || !link_metadata.file_type().is_file() {
            return Err("protected token path must be a non-symlink regular file".into());
        }

        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW);
        }
        let file = options
            .open(path)
            .map_err(|_| "protected token file could not be opened".to_string())?;
        let metadata = file
            .metadata()
            .map_err(|_| "protected token file metadata is unavailable".to_string())?;
        if !metadata.is_file() {
            return Err("protected token path must be a regular file".into());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::{MetadataExt, PermissionsExt};
            if metadata.permissions().mode() & 0o077 != 0 {
                return Err(
                    "protected token file permissions must deny group and other access".into(),
                );
            }
            // SAFETY: geteuid has no preconditions and does not retain pointers.
            let effective_uid = unsafe { libc::geteuid() };
            if metadata.uid() != effective_uid {
                return Err("protected token file must be owned by the current user".into());
            }
        }

        let mut bytes = Vec::new();
        file.take((TOKEN_MAX_BYTES + 3) as u64)
            .read_to_end(&mut bytes)
            .map_err(|_| "protected token file could not be read".to_string())?;
        Ok(bytes)
    }
}

fn normalize_token(mut bytes: Vec<u8>) -> Result<SecretToken, String> {
    if bytes.last() == Some(&b'\n') {
        bytes.pop();
        if bytes.last() == Some(&b'\r') {
            bytes.pop();
        }
    }
    if !(TOKEN_MIN_BYTES..=TOKEN_MAX_BYTES).contains(&bytes.len()) {
        bytes.fill(0);
        return Err(format!(
            "authentication token must contain {TOKEN_MIN_BYTES}–{TOKEN_MAX_BYTES} bytes"
        ));
    }
    if !bytes.iter().all(|byte| (b'!'..=b'~').contains(byte)) {
        bytes.fill(0);
        return Err("authentication token must contain visible ASCII without whitespace".into());
    }
    Ok(SecretToken(bytes))
}

fn constant_time_eq(left: &[u8; 32], right: &[u8; 32]) -> bool {
    left.iter()
        .zip(right)
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        })
        == 0
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct OpEnvelope {
    schema: String,
    id: String,
    op: ServeOp,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ServeOp {
    StartSession(StartSessionOp),
    StartPersistentSession(StartSessionOp),
    LoadPersistentSession {
        session_id: String,
    },
    SubmitInput {
        input_id: String,
        text: String,
        #[serde(default)]
        delivery: crate::service_inbox::Delivery,
    },
    InspectInbox,
    ResumeInbox,
    UserInput {
        text: String,
    },
    ApprovalResponse {
        request_id: String,
        decision: ApprovalDecision,
        #[serde(default)]
        approval_digest: Option<String>,
    },
    InteractionResponse {
        request_id: String,
        #[serde(default)]
        selected: Vec<String>,
        #[serde(default)]
        text: Option<String>,
    },
    Cancel,
    Status,
    Shutdown,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartSessionOp {
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub tools: Option<Vec<String>>,
    #[serde(default)]
    pub include_tools: Vec<String>,
    #[serde(default)]
    pub exclude_tools: Vec<String>,
    #[serde(default)]
    pub show_thinking: bool,
    #[serde(default)]
    pub allow_network: bool,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecision {
    AllowOnce,
    Deny,
    Abort,
}

#[derive(Debug, Clone, Serialize)]
struct EventEnvelope {
    schema: &'static str,
    sequence: u64,
    service_id: Uuid,
    event_id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    parent: Option<String>,
    event: ServeEvent,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServeEvent {
    InputAdmitted {
        session_id: Uuid,
        receipt: crate::service_inbox::Receipt,
    },
    Inbox {
        session_id: Uuid,
        held: bool,
        inputs: Vec<crate::service_inbox::Receipt>,
    },
    Ready {
        transport: &'static str,
    },
    SessionStarted {
        session_id: Uuid,
        provider: String,
        model: String,
        cwd: String,
    },
    SessionEnded {
        session_id: Uuid,
        reason: SessionEndReason,
    },
    TurnStarted {
        session_id: Uuid,
        turn_id: Uuid,
    },
    Runner {
        session_id: Uuid,
        turn_id: Uuid,
        event: HeadlessEventV1,
    },
    ApprovalRequested {
        session_id: Uuid,
        turn_id: Uuid,
        request_id: String,
        call_id: String,
        name: String,
        reason: String,
        network: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        approval_digest: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        email: Option<Value>,
    },
    InteractionRequested {
        session_id: Uuid,
        turn_id: Uuid,
        request_id: String,
        call_id: String,
        question: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        mode: ServiceInteractionMode,
        choices: Vec<ServiceInteractionChoice>,
        allow_other: bool,
    },
    TurnResumed {
        session_id: Uuid,
        turn_id: Uuid,
    },
    TurnCompleted {
        session_id: Uuid,
        turn_id: Uuid,
        steps: usize,
    },
    TurnFailed {
        session_id: Uuid,
        turn_id: Uuid,
    },
    TurnCancelled {
        session_id: Uuid,
        turn_id: Uuid,
    },
    Status {
        session: Option<ServiceStatus>,
    },
    Error {
        kind: ServiceErrorKind,
        message: String,
    },
    Goodbye,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionEndReason {
    Replaced,
    Shutdown,
    Eof,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceErrorKind {
    InvalidEnvelope,
    InvalidOperation,
    InvalidState,
    DuplicateOperation,
    Setup,
    Internal,
}

#[derive(Debug, Clone, Serialize)]
pub struct ServiceStatus {
    pub session_id: Uuid,
    pub provider: String,
    pub model: String,
    pub cwd: String,
    pub turn: ServiceTurnStatus,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ServiceTurnStatus {
    Idle,
    Running {
        turn_id: Uuid,
        waiting_for: Option<PendingKind>,
    },
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PendingKind {
    Approval,
    Interaction,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceInteractionMode {
    FreeText,
    SingleChoice,
    MultiSelect,
    Confirmation,
}

#[derive(Debug, Clone, Serialize)]
pub struct ServiceInteractionChoice {
    pub label: String,
    pub value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

struct EventWriter<W> {
    writer: W,
    service_id: Uuid,
    sequence: u64,
}

impl<W: AsyncWrite + Unpin> EventWriter<W> {
    fn new(writer: W) -> Self {
        Self {
            writer,
            service_id: Uuid::new_v4(),
            sequence: 0,
        }
    }

    async fn emit(
        &mut self,
        parent: Option<&str>,
        event: ServeEvent,
    ) -> Result<(), ServiceIoError> {
        let envelope = EventEnvelope {
            schema: SCHEMA,
            sequence: self.sequence,
            service_id: self.service_id,
            event_id: Uuid::new_v4(),
            parent: parent.map(str::to_string),
            event,
        };
        self.sequence = self.sequence.wrapping_add(1);
        let mut encoded = serde_json::to_vec(&envelope)
            .map_err(|_| ServiceIoError::Other("could not encode service event".into()))?;
        encoded.push(b'\n');
        self.writer
            .write_all(&encoded)
            .await
            .map_err(ServiceIoError::from_io)?;
        self.writer.flush().await.map_err(ServiceIoError::from_io)
    }

    async fn error(
        &mut self,
        parent: Option<&str>,
        kind: ServiceErrorKind,
        message: impl AsRef<str>,
    ) -> Result<(), ServiceIoError> {
        self.emit(
            parent,
            ServeEvent::Error {
                kind,
                message: bounded_safe_text(message.as_ref(), &[]),
            },
        )
        .await
    }
}

#[derive(Debug)]
enum ServiceIoError {
    BrokenPipe,
    Other(String),
}

impl ServiceIoError {
    fn from_io(error: std::io::Error) -> Self {
        if error.kind() == std::io::ErrorKind::BrokenPipe {
            Self::BrokenPipe
        } else {
            Self::Other(error.to_string())
        }
    }
}

#[async_trait]
trait ServiceSessionFactory: Send + Sync {
    fn session_store(&self) -> Result<crate::session::SessionStore, String> {
        Err("Persistent service sessions are unavailable".into())
    }
    async fn create(&self, request: &StartSessionOp) -> Result<ServiceSession, String>;
}

struct ServiceSession {
    id: Uuid,
    provider: String,
    model: String,
    cwd: String,
    messages: Vec<ChatMessage>,
    backend: Arc<dyn ChatBackend>,
    run: ServiceRunConfig,
}

impl std::fmt::Debug for ServiceSession {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ServiceSession")
            .field("id", &self.id)
            .field("provider", &self.provider)
            .field("model", &self.model)
            .field("cwd", &self.cwd)
            .field("message_count", &self.messages.len())
            .field("backend", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

#[derive(Clone)]
struct ServiceRunConfig {
    inbox: Arc<tokio::sync::Mutex<crate::service_inbox::Runtime>>,
    workspace_activity_scope: Option<PathBuf>,
    outcome_sink: Option<crate::session_outcome::OutcomeSink>,
    provider: String,
    model: String,
    workspace: crate::workspace::WorkspaceContext,
    max_turns: usize,
    command_timeout_secs: u64,
    command_output_policy: crate::config::CommandOutputPolicy,
    show_thinking: bool,
    approval_rules: Vec<crate::config::ApprovalRule>,
    permission_rules: Vec<crate::permission::PermissionRule>,
    allow_network: bool,
    chat_options: ChatRequestOptions,
    sandbox_mode: crate::sandbox::SandboxMode,
    skills: Arc<Mutex<crate::skills::SkillRegistry>>,
    task_skill_prompt: String,
    memory_recall: crate::memory::MemoryRecall,
    mcp: Option<Arc<crate::mcp::McpManager>>,
    lsp: Option<Arc<crate::lsp::LspManager>>,
    email: Option<Arc<crate::email::EmailManager>>,
    session_identity: String,
    discovery: crate::tool_discovery::ToolDiscoverySession,
    tool_policy: crate::tool_selection::ToolSelectionPolicy,
    tool_constraints: crate::tool_selection::ToolSelectionConstraints,
    tool_profile: ToolProfile,
    context_window_tokens: Option<u64>,
    job_owner: crate::jobs::JobOwner,
    redaction_secrets: Vec<String>,
}

impl std::fmt::Debug for ServiceRunConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ServiceRunConfig")
            .field("provider", &self.provider)
            .field("model", &self.model)
            .field("max_turns", &self.max_turns)
            .field("show_thinking", &self.show_thinking)
            .field("allow_network", &self.allow_network)
            .field("job_owner", &self.job_owner)
            .field("redaction_secrets", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

struct ActiveTurn {
    id: Uuid,
    parent: String,
    control: mpsc::Sender<TurnControl>,
    task: tokio::task::JoinHandle<()>,
    pending: Option<PendingRequest>,
}

#[derive(Debug, Clone)]
struct PendingRequest {
    id: String,
    kind: PendingKind,
    interaction: Option<InteractionRequest>,
    approval_digest: Option<String>,
}

struct DaemonState {
    session: Option<ServiceSession>,
    turn: Option<ActiveTurn>,
    operation_ids: HashSet<String>,
}

impl Default for DaemonState {
    fn default() -> Self {
        Self {
            session: None,
            turn: None,
            operation_ids: HashSet::with_capacity(64),
        }
    }
}

#[derive(Debug)]
enum TurnControl {
    Approval(ApprovalDecision),
    Interaction {
        selected: Vec<String>,
        text: Option<String>,
    },
}

enum WorkerEvent {
    Runner {
        turn_id: Uuid,
        event: HeadlessEventV1,
    },
    MessagesUpdated {
        turn_id: Uuid,
        messages: Vec<ChatMessage>,
    },
    ApprovalRequested {
        turn_id: Uuid,
        request: PendingRequest,
        call_id: String,
        name: String,
        reason: String,
        network: bool,
        approval_digest: Option<String>,
        email: Option<Value>,
    },
    InteractionRequested {
        turn_id: Uuid,
        request: PendingRequest,
        call_id: String,
        interaction: InteractionRequest,
    },
    Completed {
        turn_id: Uuid,
        messages: Vec<ChatMessage>,
        steps: usize,
    },
    Failed {
        turn_id: Uuid,
    },
    Aborted {
        turn_id: Uuid,
    },
}

enum BoundedLine {
    Line(Vec<u8>),
    TooLong,
    Eof,
}

async fn read_bounded_line<R: AsyncBufRead + Unpin>(
    reader: &mut R,
) -> std::io::Result<BoundedLine> {
    let mut line = Vec::new();
    let mut too_long = false;
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            return if line.is_empty() && !too_long {
                Ok(BoundedLine::Eof)
            } else if too_long {
                Ok(BoundedLine::TooLong)
            } else {
                Ok(BoundedLine::Line(line))
            };
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let take = newline.map_or(available.len(), |index| index + 1);
        if !too_long {
            let without_newline = if newline.is_some() { take - 1 } else { take };
            if line.len().saturating_add(without_newline) > MAX_LINE_BYTES {
                too_long = true;
                line.clear();
            } else {
                line.extend_from_slice(&available[..without_newline]);
            }
        }
        reader.consume(take);
        if newline.is_some() {
            if too_long {
                return Ok(BoundedLine::TooLong);
            }
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            return Ok(BoundedLine::Line(line));
        }
    }
}

async fn run_protocol<R, W, F>(reader: R, writer: W, factory: Arc<F>) -> Result<(), ServiceIoError>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
    F: ServiceSessionFactory + 'static,
{
    run_protocol_with_transport(reader, writer, factory, "stdio").await
}

async fn run_protocol_with_transport<R, W, F>(
    reader: R,
    writer: W,
    factory: Arc<F>,
    transport: &'static str,
) -> Result<(), ServiceIoError>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
    F: ServiceSessionFactory + 'static,
{
    let mut events = EventWriter::new(writer);
    let mut state = DaemonState::default();
    let result = run_protocol_loop(reader, &mut events, factory, &mut state, transport).await;
    if result.is_err() {
        cleanup_daemon_silent(&mut state).await;
    }
    result
}

async fn run_protocol_loop<R, W, F>(
    mut reader: R,
    events: &mut EventWriter<W>,
    factory: Arc<F>,
    state: &mut DaemonState,
    transport: &'static str,
) -> Result<(), ServiceIoError>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
    F: ServiceSessionFactory + 'static,
{
    let (worker_tx, mut worker_rx) = mpsc::channel(WORKER_CHANNEL_CAPACITY);
    events.emit(None, ServeEvent::Ready { transport }).await?;

    loop {
        tokio::select! {
            line = read_bounded_line(&mut reader) => {
                match line.map_err(ServiceIoError::from_io)? {
                    BoundedLine::Eof => {
                        finish_daemon(state, events, SessionEndReason::Eof, None).await?;
                        return Ok(());
                    }
                    BoundedLine::TooLong => {
                        events.error(None, ServiceErrorKind::InvalidEnvelope, format!("operation line exceeds {MAX_LINE_BYTES} bytes")).await?;
                    }
                    BoundedLine::Line(line) if line.is_empty() => {
                        events.error(None, ServiceErrorKind::InvalidEnvelope, "operation line must not be empty").await?;
                    }
                    BoundedLine::Line(line) => {
                        let envelope = match serde_json::from_slice::<OpEnvelope>(&line) {
                            Ok(envelope) => envelope,
                            Err(_) => {
                                events.error(None, ServiceErrorKind::InvalidEnvelope, "operation must be one strict ca.serve.v1 JSON object").await?;
                                continue;
                            }
                        };
                        let parent = envelope.id.clone();
                        if let Err(message) = validate_envelope(&envelope, state) {
                            let kind = if state.operation_ids.contains(&envelope.id) {
                                ServiceErrorKind::DuplicateOperation
                            } else {
                                ServiceErrorKind::InvalidEnvelope
                            };
                            events.error(Some(&parent), kind, message).await?;
                            continue;
                        }
                        state.operation_ids.insert(envelope.id.clone());
                        if handle_operation(
                            envelope,
                            state,
                            events,
                            &factory,
                            &worker_tx,
                        ).await? {
                            return Ok(());
                        }
                    }
                }
            }
            worker = worker_rx.recv() => {
                if let Some(worker) = worker {
                    handle_worker_event(worker, state, events).await?;
                }
            }
        }
        let ready = if state.turn.is_none() {
            if let Some(session) = &state.session {
                let inbox = session.run.inbox.lock().await;
                !inbox.inbox.held
                    && inbox
                        .inbox
                        .inputs
                        .iter()
                        .any(|i| i.status == crate::service_inbox::InputStatus::Pending)
            } else {
                false
            }
        } else {
            false
        };
        if ready {
            start_service_turn(state, events, &worker_tx, "service-inbox".into(), None).await?;
        }
    }
}

async fn cleanup_daemon_silent(state: &mut DaemonState) {
    if let Some(turn) = state.turn.take() {
        turn.task.abort();
        let _ = turn.task.await;
    }
    if let Some(session) = state.session.take() {
        shutdown_session(session).await;
    }
}

fn validate_envelope(envelope: &OpEnvelope, state: &DaemonState) -> Result<(), String> {
    if envelope.schema != SCHEMA {
        return Err(format!("unsupported schema; expected {SCHEMA}"));
    }
    if !valid_wire_id(&envelope.id) {
        return Err("operation id must be 1-128 safe ASCII characters".into());
    }
    if state.operation_ids.contains(&envelope.id) {
        return Err("operation id was already used".into());
    }
    if state.operation_ids.len() >= MAX_OPERATION_IDS {
        return Err("operation id limit reached; restart the service".into());
    }
    Ok(())
}

fn valid_wire_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ID_BYTES
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':'))
}

async fn start_service_turn<W: AsyncWrite + Unpin>(
    state: &mut DaemonState,
    events: &mut EventWriter<W>,
    worker_tx: &mpsc::Sender<WorkerEvent>,
    parent: String,
    text: Option<String>,
) -> Result<bool, ServiceIoError> {
    let Some(session) = state.session.as_mut() else {
        return Ok(false);
    };
    let mut run = session.run.clone();
    let admission = if let Some(text) = text {
        let mut messages = session.messages.clone();
        messages.push(ChatMessage {
            role: "user".into(),
            content: crate::image_content::ModelContent::text(text),
            tool_calls: None,
            tool_call_id: None,
            response_items: Vec::new(),
        });
        match run.inbox.lock().await.checkpoint(messages.clone()).await {
            Ok(()) => {
                session.messages = messages;
                Ok(true)
            }
            Err(error) => Err(error),
        }
    } else {
        run.inbox
            .lock()
            .await
            .promote(&mut session.messages, true)
            .await
    };
    match admission {
        Ok(true) => {}
        Ok(false) => return Ok(false),
        Err(message) => {
            events
                .error(Some(&parent), ServiceErrorKind::Internal, message)
                .await?;
            return Ok(false);
        }
    }
    let text_for_skills = session
        .messages
        .iter()
        .rev()
        .find(|m| m.role == "user")
        .and_then(|m| m.content.as_str())
        .unwrap_or_default()
        .to_string();
    let mut loads = crate::skills::SkillLoadSet::default();
    let task_skill_prompt = {
        let registry = run
            .skills
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let mut result = Ok(());
        for selector in crate::skills::explicit_skill_selectors(&text_for_skills) {
            if let Err(error) = registry.load_for_task(&selector, true, &mut loads) {
                result = Err(format!(
                    "skill invocation ${selector} failed before request: {error}"
                ));
                break;
            }
        }
        result.and_then(|()| {
            registry
                .task_prompt(&loads)
                .map_err(|error| format!("skill load changed before request: {error}"))
        })
    };
    run.task_skill_prompt = match task_skill_prompt {
        Ok(prompt) => prompt,
        Err(error) => {
            events
                .error(Some(&parent), ServiceErrorKind::InvalidOperation, error)
                .await?;
            return Ok(false);
        }
    };
    let turn_id = Uuid::new_v4();
    let session_id = session.id;
    let messages = session.messages.clone();
    let backend = session.backend.clone();
    let (control_tx, control_rx) = mpsc::channel(CONTROL_CHANNEL_CAPACITY);
    let worker = worker_tx.clone();
    let task = tokio::spawn(async move {
        let panic_events = worker.clone();
        let result = std::panic::AssertUnwindSafe(run_turn(
            backend, messages, run, turn_id, worker, control_rx,
        ))
        .catch_unwind()
        .await;
        if result.is_err() {
            fail_turn(
                &panic_events,
                turn_id,
                HeadlessFailureKind::Internal,
                "service turn failed internally".into(),
                8,
            )
            .await;
        }
    });
    state.turn = Some(ActiveTurn {
        id: turn_id,
        parent: parent.clone(),
        control: control_tx,
        task,
        pending: None,
    });
    events
        .emit(
            Some(&parent),
            ServeEvent::TurnStarted {
                session_id,
                turn_id,
            },
        )
        .await?;
    Ok(false)
}

async fn handle_operation<W, F>(
    envelope: OpEnvelope,
    state: &mut DaemonState,
    events: &mut EventWriter<W>,
    factory: &Arc<F>,
    worker_tx: &mpsc::Sender<WorkerEvent>,
) -> Result<bool, ServiceIoError>
where
    W: AsyncWrite + Unpin,
    F: ServiceSessionFactory + 'static,
{
    let persistent_start = matches!(&envelope.op, ServeOp::StartPersistentSession(_));
    let parent = envelope.id;
    match envelope.op {
        ServeOp::StartSession(request) | ServeOp::StartPersistentSession(request) => {
            if state.turn.is_some() {
                events
                    .error(
                        Some(&parent),
                        ServiceErrorKind::InvalidState,
                        "cancel the active turn before replacing the session",
                    )
                    .await?;
                return Ok(false);
            }
            if let Err(message) = validate_start_request(&request) {
                events
                    .error(Some(&parent), ServiceErrorKind::InvalidOperation, message)
                    .await?;
                return Ok(false);
            }
            let replacement = match factory.create(&request).await {
                Ok(session) => session,
                Err(message) => {
                    events
                        .error(Some(&parent), ServiceErrorKind::Setup, message)
                        .await?;
                    return Ok(false);
                }
            };
            if persistent_start {
                let stored = crate::session::SessionV2::new(
                    "Service session",
                    replacement.cwd.clone(),
                    replacement.provider.clone(),
                    replacement.model.clone(),
                    vec![],
                );
                let mut stored = stored;
                stored.id = replacement.id;
                let result = match factory.session_store() {
                    Ok(store) => {
                        replacement
                            .run
                            .inbox
                            .lock()
                            .await
                            .attach(stored, store, replacement.messages.clone())
                            .await
                    }
                    Err(error) => Err(error),
                };
                if let Err(message) = result {
                    shutdown_session(replacement).await;
                    events
                        .error(Some(&parent), ServiceErrorKind::Setup, message)
                        .await?;
                    return Ok(false);
                }
            }
            if let Some(previous) = state.session.take() {
                let id = previous.id;
                shutdown_session(previous).await;
                events
                    .emit(
                        Some(&parent),
                        ServeEvent::SessionEnded {
                            session_id: id,
                            reason: SessionEndReason::Replaced,
                        },
                    )
                    .await?;
            }
            let started = ServeEvent::SessionStarted {
                session_id: replacement.id,
                provider: replacement.provider.clone(),
                model: replacement.model.clone(),
                cwd: replacement.cwd.clone(),
            };
            state.session = Some(replacement);
            events.emit(Some(&parent), started).await?;
        }
        ServeOp::LoadPersistentSession { session_id } => {
            if state.turn.is_some() {
                events
                    .error(
                        Some(&parent),
                        ServiceErrorKind::InvalidState,
                        "cancel the active turn before loading a session",
                    )
                    .await?;
                return Ok(false);
            }
            let loaded = async {
                let id = Uuid::parse_str(&session_id)
                    .map_err(|_| "Expected a saved session UUID".to_string())?;
                let store = factory.session_store()?;
                let source = store.clone();
                let saved = tokio::task::spawn_blocking(move || source.load(&id.to_string()))
                    .await
                    .map_err(|_| "Session load worker stopped".to_string())?
                    .map_err(|e| e.to_string())?;
                let request = StartSessionOp {
                    provider: Some(saved.provider.clone()),
                    model: Some(saved.model.clone()),
                    cwd: Some(saved.workspace.clone()),
                    ..Default::default()
                };
                let mut replacement = factory.create(&request).await?;
                replacement.id = saved.id;
                replacement.run.session_identity = saved.id.to_string();
                let assembly = crate::agent::AgentClient::build_segmented_messages(
                    None,
                    None,
                    &saved.messages,
                    None,
                    &[],
                )
                .map_err(|e| e.to_string())?;
                replacement.messages.extend(assembly.messages);
                replacement
                    .run
                    .inbox
                    .lock()
                    .await
                    .attach(saved, store, replacement.messages.clone())
                    .await?;
                Ok::<_, String>(replacement)
            }
            .await;
            match loaded {
                Ok(replacement) => {
                    if let Some(previous) = state.session.take() {
                        shutdown_session(previous).await;
                    }
                    events
                        .emit(
                            Some(&parent),
                            ServeEvent::SessionStarted {
                                session_id: replacement.id,
                                provider: replacement.provider.clone(),
                                model: replacement.model.clone(),
                                cwd: replacement.cwd.clone(),
                            },
                        )
                        .await?;
                    state.session = Some(replacement);
                }
                Err(message) => {
                    events
                        .error(Some(&parent), ServiceErrorKind::Setup, message)
                        .await?
                }
            }
        }
        ServeOp::SubmitInput {
            input_id,
            text,
            delivery,
        } => {
            let Some(session) = state.session.as_ref() else {
                events
                    .error(
                        Some(&parent),
                        ServiceErrorKind::InvalidState,
                        "start a session before submitting input",
                    )
                    .await?;
                return Ok(false);
            };
            let session_id = session.id;
            let receipt = session
                .run
                .inbox
                .lock()
                .await
                .admit(&input_id, &text, delivery)
                .await;
            match receipt {
                Ok(receipt) => {
                    let pending = receipt.status == crate::service_inbox::InputStatus::Pending;
                    events
                        .emit(
                            Some(&parent),
                            ServeEvent::InputAdmitted {
                                session_id,
                                receipt,
                            },
                        )
                        .await?;
                    if pending && state.turn.is_none() {
                        start_service_turn(state, events, worker_tx, parent, None).await?;
                    }
                }
                Err(message) => {
                    events
                        .error(Some(&parent), ServiceErrorKind::InvalidOperation, message)
                        .await?
                }
            }
        }
        ServeOp::InspectInbox => {
            let Some(session) = state.session.as_ref() else {
                events
                    .error(
                        Some(&parent),
                        ServiceErrorKind::InvalidState,
                        "no service session",
                    )
                    .await?;
                return Ok(false);
            };
            let inbox = session.run.inbox.lock().await;
            events
                .emit(
                    Some(&parent),
                    ServeEvent::Inbox {
                        session_id: session.id,
                        held: inbox.inbox.held,
                        inputs: inbox.inbox.receipts(inbox.durable()),
                    },
                )
                .await?;
        }
        ServeOp::ResumeInbox => {
            let Some(session) = state.session.as_ref() else {
                events
                    .error(
                        Some(&parent),
                        ServiceErrorKind::InvalidState,
                        "no service session",
                    )
                    .await?;
                return Ok(false);
            };
            let result = session.run.inbox.lock().await.resume().await;
            match result {
                Ok(()) => {
                    if state.turn.is_none() {
                        start_service_turn(state, events, worker_tx, parent, None).await?;
                    }
                }
                Err(message) => {
                    events
                        .error(Some(&parent), ServiceErrorKind::Internal, message)
                        .await?
                }
            }
        }
        ServeOp::UserInput { text } => {
            if state.turn.is_some() {
                events
                    .error(
                        Some(&parent),
                        ServiceErrorKind::InvalidState,
                        "one turn is already active; input is not queued",
                    )
                    .await?;
                return Ok(false);
            }
            let Some(_) = state.session.as_ref() else {
                events
                    .error(
                        Some(&parent),
                        ServiceErrorKind::InvalidState,
                        "start a session before submitting input",
                    )
                    .await?;
                return Ok(false);
            };
            if text.trim().is_empty() || text.len() > MAX_PROMPT_BYTES {
                events
                    .error(
                        Some(&parent),
                        ServiceErrorKind::InvalidOperation,
                        format!("input must be 1-{MAX_PROMPT_BYTES} UTF-8 bytes"),
                    )
                    .await?;
                return Ok(false);
            }
            start_service_turn(state, events, worker_tx, parent, Some(text)).await?;
        }
        ServeOp::ApprovalResponse {
            request_id,
            decision,
            approval_digest,
        } => {
            let expected = state
                .turn
                .as_ref()
                .and_then(|turn| turn.pending.as_ref())
                .filter(|pending| pending.id == request_id)
                .and_then(|pending| pending.approval_digest.as_ref());
            if matches!(decision, ApprovalDecision::AllowOnce)
                && expected.map(String::as_str) != approval_digest.as_deref()
            {
                events
                    .error(
                        Some(&parent),
                        ServiceErrorKind::InvalidOperation,
                        "approval does not match the exact pending communication",
                    )
                    .await?;
                return Ok(false);
            }
            respond_to_pending(
                state,
                events,
                &parent,
                request_id,
                PendingKind::Approval,
                TurnControl::Approval(decision),
            )
            .await?;
        }
        ServeOp::InteractionResponse {
            request_id,
            selected,
            text,
        } => {
            let invalid = state
                .turn
                .as_ref()
                .and_then(|turn| turn.pending.as_ref())
                .filter(|pending| {
                    pending.id == request_id && pending.kind == PendingKind::Interaction
                })
                .and_then(|pending| pending.interaction.as_ref())
                .and_then(|interaction| {
                    validate_interaction_answer(interaction, selected.clone(), text.clone()).err()
                });
            if let Some(message) = invalid {
                events
                    .error(Some(&parent), ServiceErrorKind::InvalidOperation, message)
                    .await?;
                return Ok(false);
            }
            respond_to_pending(
                state,
                events,
                &parent,
                request_id,
                PendingKind::Interaction,
                TurnControl::Interaction { selected, text },
            )
            .await?;
        }
        ServeOp::Cancel => {
            if !cancel_active_turn(state, events, Some(&parent)).await? {
                events
                    .error(
                        Some(&parent),
                        ServiceErrorKind::InvalidState,
                        "no turn is active",
                    )
                    .await?;
            }
        }
        ServeOp::Status => {
            events
                .emit(
                    Some(&parent),
                    ServeEvent::Status {
                        session: service_status(state),
                    },
                )
                .await?;
        }
        ServeOp::Shutdown => {
            finish_daemon(state, events, SessionEndReason::Shutdown, Some(&parent)).await?;
            return Ok(true);
        }
    }
    Ok(false)
}

fn validate_start_request(request: &StartSessionOp) -> Result<(), String> {
    for (label, value) in [
        ("provider", request.provider.as_deref()),
        ("model", request.model.as_deref()),
    ] {
        if let Some(value) = value {
            if value.is_empty() || value.len() > 256 || value.contains(['\r', '\n', '\0']) {
                return Err(format!("{label} must be 1-256 safe UTF-8 bytes"));
            }
        }
    }
    if request
        .cwd
        .as_ref()
        .is_some_and(|cwd| cwd.is_empty() || cwd.len() > 4_096 || cwd.contains('\0'))
    {
        return Err("cwd must be 1-4096 safe UTF-8 bytes".into());
    }
    for tools in [
        request.tools.as_deref().unwrap_or_default(),
        request.include_tools.as_slice(),
        request.exclude_tools.as_slice(),
    ] {
        if tools.len() > MAX_TOOL_NAMES {
            return Err(format!(
                "each tool list is limited to {MAX_TOOL_NAMES} names"
            ));
        }
    }
    Ok(())
}

async fn respond_to_pending<W: AsyncWrite + Unpin>(
    state: &mut DaemonState,
    events: &mut EventWriter<W>,
    parent: &str,
    request_id: String,
    kind: PendingKind,
    control: TurnControl,
) -> Result<(), ServiceIoError> {
    let Some(turn) = state.turn.as_mut() else {
        events
            .error(
                Some(parent),
                ServiceErrorKind::InvalidState,
                "no turn is waiting for a response",
            )
            .await?;
        return Ok(());
    };
    let Some(pending) = turn.pending.as_ref() else {
        events
            .error(
                Some(parent),
                ServiceErrorKind::InvalidState,
                "the active turn is not waiting for a response",
            )
            .await?;
        return Ok(());
    };
    if pending.id != request_id || pending.kind != kind {
        events
            .error(
                Some(parent),
                ServiceErrorKind::InvalidOperation,
                "response does not match the exact pending request",
            )
            .await?;
        return Ok(());
    }
    if turn.control.send(control).await.is_err() {
        events
            .error(
                Some(parent),
                ServiceErrorKind::Internal,
                "the active turn no longer accepts responses",
            )
            .await?;
        return Ok(());
    }
    turn.pending = None;
    if let Some(session) = state.session.as_ref() {
        events
            .emit(
                Some(parent),
                ServeEvent::TurnResumed {
                    session_id: session.id,
                    turn_id: turn.id,
                },
            )
            .await?;
    }
    Ok(())
}

fn service_status(state: &DaemonState) -> Option<ServiceStatus> {
    state.session.as_ref().map(|session| ServiceStatus {
        session_id: session.id,
        provider: session.provider.clone(),
        model: session.model.clone(),
        cwd: session.cwd.clone(),
        turn: state.turn.as_ref().map_or(ServiceTurnStatus::Idle, |turn| {
            ServiceTurnStatus::Running {
                turn_id: turn.id,
                waiting_for: turn.pending.as_ref().map(|pending| pending.kind),
            }
        }),
    })
}

async fn handle_worker_event<W: AsyncWrite + Unpin>(
    worker: WorkerEvent,
    state: &mut DaemonState,
    events: &mut EventWriter<W>,
) -> Result<(), ServiceIoError> {
    let turn_id = match &worker {
        WorkerEvent::Runner { turn_id, .. }
        | WorkerEvent::MessagesUpdated { turn_id, .. }
        | WorkerEvent::ApprovalRequested { turn_id, .. }
        | WorkerEvent::InteractionRequested { turn_id, .. }
        | WorkerEvent::Completed { turn_id, .. }
        | WorkerEvent::Failed { turn_id }
        | WorkerEvent::Aborted { turn_id } => *turn_id,
    };
    let Some(turn) = state.turn.as_mut().filter(|turn| turn.id == turn_id) else {
        return Ok(());
    };
    let Some(session) = state.session.as_mut() else {
        return Ok(());
    };
    let parent = turn.parent.clone();
    match worker {
        WorkerEvent::Runner { event, .. } => {
            events
                .emit(
                    Some(&parent),
                    ServeEvent::Runner {
                        session_id: session.id,
                        turn_id,
                        event,
                    },
                )
                .await?;
        }
        WorkerEvent::MessagesUpdated { messages, .. } => {
            session.messages = messages;
        }
        WorkerEvent::ApprovalRequested {
            request,
            call_id,
            name,
            reason,
            network,
            approval_digest,
            email,
            ..
        } => {
            turn.pending = Some(request.clone());
            events
                .emit(
                    Some(&parent),
                    ServeEvent::ApprovalRequested {
                        session_id: session.id,
                        turn_id,
                        request_id: request.id,
                        call_id,
                        name,
                        reason,
                        network,
                        approval_digest,
                        email,
                    },
                )
                .await?;
        }
        WorkerEvent::InteractionRequested {
            mut request,
            call_id,
            interaction,
            ..
        } => {
            request.interaction = Some(interaction.clone());
            turn.pending = Some(request.clone());
            let mode = match interaction.mode {
                InteractionMode::FreeText => ServiceInteractionMode::FreeText,
                InteractionMode::SingleChoice => ServiceInteractionMode::SingleChoice,
                InteractionMode::MultiSelect => ServiceInteractionMode::MultiSelect,
                InteractionMode::Confirmation => ServiceInteractionMode::Confirmation,
                InteractionMode::Masked => {
                    events
                        .error(
                            Some(&parent),
                            ServiceErrorKind::Internal,
                            "masked interactions are unavailable over the service protocol",
                        )
                        .await?;
                    return Ok(());
                }
            };
            let choices = interaction
                .choices
                .into_iter()
                .map(|choice| ServiceInteractionChoice {
                    value: bounded_safe_text(
                        &choice.resolved_value(),
                        &session.run.redaction_refs(),
                    ),
                    label: bounded_safe_text(&choice.label, &session.run.redaction_refs()),
                    description: choice.description.map(|description| {
                        bounded_safe_text(&description, &session.run.redaction_refs())
                    }),
                })
                .collect();
            events
                .emit(
                    Some(&parent),
                    ServeEvent::InteractionRequested {
                        session_id: session.id,
                        turn_id,
                        request_id: request.id,
                        call_id,
                        question: bounded_safe_text(
                            &interaction.question,
                            &session.run.redaction_refs(),
                        ),
                        description: interaction.description.map(|description| {
                            bounded_safe_text(&description, &session.run.redaction_refs())
                        }),
                        mode,
                        choices,
                        allow_other: interaction.allow_other,
                    },
                )
                .await?;
        }
        WorkerEvent::Completed {
            messages, steps, ..
        } => {
            let _ = (&mut turn.task).await;
            if let Some(sink) = &session.run.outcome_sink {
                sink.flush_async().await;
            }
            session.messages = messages;
            scrub_email_chat_messages(&mut session.messages);
            events
                .emit(
                    Some(&parent),
                    ServeEvent::TurnCompleted {
                        session_id: session.id,
                        turn_id,
                        steps,
                    },
                )
                .await?;
            state.turn.take();
        }
        WorkerEvent::Failed { .. } => {
            let _ = (&mut turn.task).await;
            if let Some(sink) = &session.run.outcome_sink {
                sink.flush_async().await;
            }
            scrub_email_chat_messages(&mut session.messages);
            session
                .run
                .inbox
                .lock()
                .await
                .stop(false)
                .await
                .map_err(ServiceIoError::Other)?;
            events
                .emit(
                    Some(&parent),
                    ServeEvent::TurnFailed {
                        session_id: session.id,
                        turn_id,
                    },
                )
                .await?;
            state.turn.take();
        }
        WorkerEvent::Aborted { .. } => {
            session
                .run
                .inbox
                .lock()
                .await
                .stop(false)
                .await
                .map_err(ServiceIoError::Other)?;
            let _ = (&mut turn.task).await;
            if let Some(sink) = &session.run.outcome_sink {
                sink.flush_async().await;
            }
            scrub_email_chat_messages(&mut session.messages);
            events
                .emit(
                    Some(&parent),
                    ServeEvent::TurnCancelled {
                        session_id: session.id,
                        turn_id,
                    },
                )
                .await?;
            state.turn.take();
        }
    }
    Ok(())
}

async fn cancel_active_turn<W: AsyncWrite + Unpin>(
    state: &mut DaemonState,
    events: &mut EventWriter<W>,
    parent: Option<&str>,
) -> Result<bool, ServiceIoError> {
    if let Some(session) = &state.session {
        session
            .run
            .inbox
            .lock()
            .await
            .stop(false)
            .await
            .map_err(ServiceIoError::Other)?;
    }
    let Some(turn) = state.turn.take() else {
        return Ok(false);
    };
    turn.task.abort();
    let _ = turn.task.await;
    if let Some(session) = state.session.as_ref() {
        cleanup_owned_jobs(session.run.job_owner).await;
        if let Some(sink) = &session.run.outcome_sink {
            sink.flush_async().await;
        }
        events
            .emit(
                parent,
                ServeEvent::TurnCancelled {
                    session_id: session.id,
                    turn_id: turn.id,
                },
            )
            .await?;
    }
    Ok(true)
}

async fn finish_daemon<W: AsyncWrite + Unpin>(
    state: &mut DaemonState,
    events: &mut EventWriter<W>,
    reason: SessionEndReason,
    parent: Option<&str>,
) -> Result<(), ServiceIoError> {
    let _ = cancel_active_turn(state, events, parent).await?;
    if let Some(session) = state.session.take() {
        let id = session.id;
        shutdown_session(session).await;
        events
            .emit(
                parent,
                ServeEvent::SessionEnded {
                    session_id: id,
                    reason,
                },
            )
            .await?;
    }
    events.emit(parent, ServeEvent::Goodbye).await
}

async fn shutdown_session(session: ServiceSession) {
    let _ = session.run.inbox.lock().await.stop(false).await;
    cleanup_owned_jobs(session.run.job_owner).await;
    if let Some(sink) = session.run.outcome_sink {
        sink.flush_async().await;
    }
    if let Some(manager) = session.run.mcp {
        manager.shutdown().await;
    }
    if let Some(manager) = session.run.lsp {
        manager.shutdown().await;
    }
}

async fn cleanup_owned_jobs(owner: crate::jobs::JobOwner) {
    crate::browser::BrowserManager::process()
        .shutdown_owner(owner)
        .await;
    let manager = crate::jobs::JobManager::process();
    let jobs = manager.list_owned(owner).await;
    for job in jobs {
        if !job.status.is_terminal() {
            let _ = manager.kill_owned(job.id, owner).await;
        }
    }
}

impl ServiceRunConfig {
    fn redaction_refs(&self) -> Vec<&str> {
        self.redaction_secrets.iter().map(String::as_str).collect()
    }
}

async fn run_turn(
    backend: Arc<dyn ChatBackend>,
    mut messages: Vec<ChatMessage>,
    run: ServiceRunConfig,
    turn_id: Uuid,
    events: mpsc::Sender<WorkerEvent>,
    mut controls: mpsc::Receiver<TurnControl>,
) {
    let _workspace_activity = if let Some(scope) = run.workspace_activity_scope.clone() {
        let roots = run
            .workspace
            .roots()
            .iter()
            .map(|root| root.path.clone())
            .collect::<Vec<_>>();
        let lease = tokio::task::spawn_blocking(move || {
            crate::recovery::RecoveryStore::new(scope).try_workspace_activity(&roots)
        })
        .await
        .map_err(|_| "Workspace activity worker stopped".to_string())
        .and_then(|result| result.map_err(|error| error.to_string()));
        match lease {
            Ok(lease) => {
                crate::headless::WorkspaceActivityGuard::new(Some(lease), None, run.job_owner)
            }
            Err(message) => {
                fail_turn(&events, turn_id, HeadlessFailureKind::Setup, message, 3).await;
                return;
            }
        }
    } else {
        crate::headless::WorkspaceActivityGuard::new(None, None, run.job_owner)
    };
    let tracker = crate::session_outcome::OutcomeTracker::for_run(
        run.session_identity.clone(),
        turn_id,
        &run.workspace.cwd().to_string_lossy(),
    );
    let mut outcome = crate::session_outcome::ScopedOutcome(match run.outcome_sink.clone() {
        Some(sink) => tracker.with_sink(sink),
        None => tracker,
    });
    outcome.set_model(Some(run.model.clone()), Some(run.provider.clone()));
    outcome.record_user_message();
    if send_runner(
        &events,
        turn_id,
        HeadlessEventV1::RunStart {
            model: run.model.clone(),
        },
    )
    .await
    .is_err()
    {
        return;
    }
    let mut prompted_skills = run
        .skills
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .active_refs()
        .into_iter()
        .map(|reference| reference.id)
        .collect::<HashSet<_>>();
    let mut loop_guard = crate::loop_guard::LoopGuard::default();
    let mut loop_guard_replan_prompt = None;
    let _ = tokio::time::timeout(
        std::time::Duration::from_secs(90),
        run.memory_recall.prepare_local_hybrid(),
    )
    .await;
    let memory_prompt = run
        .memory_recall
        .render_for_query_async(
            messages
                .iter()
                .rev()
                .find(|message| message.role == "user")
                .and_then(|message| message.content.as_str())
                .unwrap_or(""),
        )
        .await;

    let mut step = 0;
    loop {
        if step >= run.max_turns {
            break;
        }
        match run.inbox.lock().await.promote(&mut messages, false).await {
            Ok(true) => step = 0,
            Ok(false) => {}
            Err(message) => {
                fail_turn(&events, turn_id, HeadlessFailureKind::Internal, message, 8).await;
                return;
            }
        }
        step += 1;
        if send_runner(&events, turn_id, HeadlessEventV1::TurnStart { turn: step })
            .await
            .is_err()
        {
            return;
        }
        let catalog_registry = if let Some(manager) = &run.mcp {
            manager.active_catalog().await.registry
        } else {
            None
        };
        let registry = catalog_registry
            .as_ref()
            .unwrap_or_else(|| ToolRegistry::builtins());
        let service_registry = match ToolRegistry::new(
            registry
                .specs()
                .iter()
                .filter(|spec| service_tool_allowed(spec))
                .cloned()
                .collect(),
        ) {
            Ok(registry) => registry,
            Err(error) => {
                {
                    outcome.record_failure();
                }
                fail_turn(
                    &events,
                    turn_id,
                    HeadlessFailureKind::Internal,
                    format!("service tool registry failed: {error}"),
                    8,
                )
                .await;
                return;
            }
        };
        let tools = match crate::tool_selection::resolve_tools(
            &service_registry,
            run.tool_profile,
            &run.tool_policy,
            &run.tool_constraints,
        ) {
            Ok(tools) => tools.into_definitions(),
            Err(error) => {
                {
                    outcome.record_failure();
                }
                fail_turn(
                    &events,
                    turn_id,
                    HeadlessFailureKind::Input,
                    format!("tool selection failed: {error}"),
                    2,
                )
                .await;
                return;
            }
        };
        let tools = run.discovery.advertise(tools, &run.tool_policy);
        let advertised = tools
            .iter()
            .map(|tool| tool.function.name.clone())
            .collect::<HashSet<_>>();
        let mut request_messages = messages.clone();
        let stable_prefix_messages = request_messages
            .iter()
            .take_while(|message| matches!(message.role.as_str(), "system" | "developer"))
            .count();
        if let Some(prompt) = loop_guard_replan_prompt.take() {
            request_messages.push(ChatMessage {
                role: "user".into(),
                content: crate::image_content::ModelContent::text(prompt),
                tool_calls: None,
                tool_call_id: None,
                response_items: Vec::new(),
            });
        }
        let mut logical_turn_prompt = run.task_skill_prompt.clone();
        logical_turn_prompt.push_str(&memory_prompt);
        if !logical_turn_prompt.is_empty() {
            request_messages.insert(
                stable_prefix_messages,
                ChatMessage {
                    role: "system".into(),
                    content: crate::image_content::ModelContent::text(logical_turn_prompt),
                    tool_calls: None,
                    tool_call_id: None,
                    response_items: Vec::new(),
                },
            );
        }
        let mut prepared = crate::request::PreparedModelRequest::new(
            crate::request::RequestKind::Service,
            run.provider.clone(),
            run.model.clone(),
            request_messages,
            tools,
            run.chat_options.clone(),
        );
        if let Ok(scope_id) = Uuid::parse_str(&run.session_identity) {
            prepared = prepared.with_prompt_cache(
                crate::request::PromptCacheScope::conversation(scope_id),
                stable_prefix_messages,
            );
        }
        let prepared = backend.prepare_harness_request(prepared).await;
        outcome.set_runtime_harness(crate::harness_registry::capture_prepared(&prepared));
        let mut stream = match backend.stream_prepared(&prepared).await {
            Ok(stream) => stream,
            Err(error) => {
                let (code, kind) = provider_failure(&error);
                {
                    outcome.record_failure();
                }
                fail_turn(
                    &events,
                    turn_id,
                    kind,
                    bounded_safe_text(&error.to_string(), &run.redaction_refs()),
                    code,
                )
                .await;
                return;
            }
        };
        let mut assembler = AssistantTurnAssembler::new();
        let mut stream_error = None;
        while let Some(item) = stream.recv().await {
            match item {
                Ok(event) => {
                    let outbound = match &event {
                        ProviderEvent::Text(delta) => Some(HeadlessEventV1::AssistantOutput {
                            delta: bounded_safe_text(delta, &run.redaction_refs()),
                        }),
                        ProviderEvent::Reasoning(delta) if run.show_thinking => {
                            Some(HeadlessEventV1::ReasoningOutput {
                                delta: bounded_safe_text(delta, &run.redaction_refs()),
                            })
                        }
                        ProviderEvent::Usage(usage)
                        | ProviderEvent::Finish {
                            usage: Some(usage), ..
                        } => Some(HeadlessEventV1::Usage {
                            usage: usage.clone(),
                        }),
                        _ => None,
                    };
                    if let Some(outbound) = outbound {
                        if send_runner(&events, turn_id, outbound).await.is_err() {
                            return;
                        }
                    }
                    if let Err(error) = assembler.push(&event) {
                        stream_error = Some(error);
                        break;
                    }
                }
                Err(error) => {
                    stream_error = Some(error);
                    break;
                }
            }
        }
        let turn = match stream_error.map_or_else(|| assembler.finish(), Err) {
            Ok(turn) => turn,
            Err(error) => {
                let (code, kind) = provider_failure(&error);
                {
                    outcome.record_failure();
                }
                fail_turn(
                    &events,
                    turn_id,
                    kind,
                    bounded_safe_text(&error.to_string(), &run.redaction_refs()),
                    code,
                )
                .await;
                return;
            }
        };

        if turn.tool_calls.is_empty() {
            let safe_message = bounded_safe_text(&turn.content, &run.redaction_refs());
            messages.push(ChatMessage {
                role: "assistant".into(),
                content: crate::image_content::ModelContent::text(turn.content),
                tool_calls: None,
                tool_call_id: None,
                response_items: turn.response_items,
            });
            scrub_email_chat_messages(&mut messages);
            if send_runner(
                &events,
                turn_id,
                HeadlessEventV1::Completion {
                    message: safe_message,
                },
            )
            .await
            .is_err()
            {
                return;
            }
            outcome.record_assistant_turn();
            match run.inbox.lock().await.promote(&mut messages, true).await {
                Ok(true) => {
                    step = 0;
                    continue;
                }
                Ok(false) => {}
                Err(message) => {
                    fail_turn(&events, turn_id, HeadlessFailureKind::Internal, message, 8).await;
                    return;
                }
            }
            if let Err(message) = run.inbox.lock().await.stop(true).await {
                fail_turn(&events, turn_id, HeadlessFailureKind::Internal, message, 8).await;
                return;
            }
            let _ = events
                .send(WorkerEvent::Completed {
                    turn_id,
                    messages,
                    steps: step,
                })
                .await;
            return;
        }

        let mut completed_calls = Vec::new();
        let mut completed_images = std::collections::HashMap::new();
        let mut loop_incident = None;
        let discovery_batch = run.discovery.begin_batch();
        let mut native_results = std::collections::BTreeMap::new();
        let mut native_started = std::collections::HashSet::new();
        for (call_index, call) in turn.tool_calls.iter().enumerate() {
            if !advertised.contains(&call.name) {
                {
                    outcome.record_failure();
                }
                fail_turn(
                    &events,
                    turn_id,
                    HeadlessFailureKind::ProviderProtocol,
                    format!("provider attempted unavailable tool: {}", call.name),
                    6,
                )
                .await;
                return;
            }
            let arguments = match serde_json::from_str::<Value>(&call.arguments) {
                Ok(arguments) => arguments,
                Err(_) => {
                    {
                        outcome.record_failure();
                    }
                    fail_turn(
                        &events,
                        turn_id,
                        HeadlessFailureKind::ProviderProtocol,
                        format!("provider returned invalid arguments for {}", call.name),
                        6,
                    )
                    .await;
                    return;
                }
            };
            let event_arguments = if crate::email::is_email_tool(&call.name) {
                crate::email::metadata_only(&call.name, &arguments, None)
            } else {
                safe_tool_arguments(&arguments, &run.redaction_refs())
            };
            if !native_started.remove(&call_index)
                && send_runner(
                    &events,
                    turn_id,
                    HeadlessEventV1::ToolStart {
                        call_id: bounded_identifier(&call.id),
                        name: bounded_identifier(&call.name),
                        arguments: event_arguments,
                    },
                )
                .await
                .is_err()
            {
                return;
            }
            let activation_id = (call.name == "activate_skill")
                .then(|| arguments.get("id").and_then(Value::as_str))
                .flatten()
                .map(str::to_string);
            if !native_results.contains_key(&call_index) {
                let mut prepared = Vec::new();
                for candidate in turn
                    .tool_calls
                    .iter()
                    .skip(call_index)
                    .take(crate::agent::tools::MAX_NATIVE_READ_CONCURRENCY)
                {
                    if !advertised.contains(&candidate.name) {
                        break;
                    }
                    let Some(read) = prepare_service_native_read(&run, candidate).await else {
                        break;
                    };
                    prepared.push(read);
                }
                if prepared.len() > 1 {
                    for offset in 1..prepared.len() {
                        let candidate = &turn.tool_calls[call_index + offset];
                        let arguments: Value = serde_json::from_str(&candidate.arguments)
                            .expect("admitted read arguments parsed");
                        if send_runner(
                            &events,
                            turn_id,
                            HeadlessEventV1::ToolStart {
                                call_id: bounded_identifier(&candidate.id),
                                name: bounded_identifier(&candidate.name),
                                arguments: safe_tool_arguments(&arguments, &run.redaction_refs()),
                            },
                        )
                        .await
                        .is_err()
                        {
                            return;
                        }
                        native_started.insert(call_index + offset);
                    }
                    let context = crate::agent::registry::ToolContext::native_read(
                        &run.workspace,
                        run.command_timeout_secs,
                        run.tool_profile,
                        run.sandbox_mode,
                        run.job_owner,
                    );
                    for (offset, result) in
                        crate::agent::tools::execute_native_read_run(&prepared, &context)
                            .await
                            .into_iter()
                            .enumerate()
                    {
                        native_results.insert(call_index + offset, result);
                    }
                }
            }
            let execution = match if let Some(result) = native_results.remove(&call_index) {
                ExecuteOutcome::Result(result)
            } else {
                execute_service_call(&run, call, &arguments, turn_id, &events, &mut controls).await
            } {
                ExecuteOutcome::Result(result) => result,
                ExecuteOutcome::Abort => {
                    let _ = events.send(WorkerEvent::Aborted { turn_id }).await;
                    return;
                }
                ExecuteOutcome::Protocol(message) => {
                    {
                        outcome.record_failure();
                    }
                    fail_turn(
                        &events,
                        turn_id,
                        HeadlessFailureKind::ProviderProtocol,
                        message,
                        6,
                    )
                    .await;
                    return;
                }
            };
            if execution.is_success() {
                if let Some(id) = activation_id.filter(|id| prompted_skills.insert(id.clone())) {
                    let block = run
                        .skills
                        .lock()
                        .unwrap_or_else(|poison| poison.into_inner())
                        .active_prompt_for(&id);
                    if let Some(block) = block {
                        if let Some(system) = messages
                            .first_mut()
                            .filter(|message| message.role == "system")
                            .and_then(|message| message.content.as_text_mut())
                        {
                            system.push_str(&block);
                        }
                    }
                }
            }
            let safe_output = if crate::email::is_email_tool(&call.name) {
                crate::email::private_result_marker(&call.name)
            } else {
                bounded_safe_text(execution.model_text(), &run.redaction_refs())
            };
            if outcome.record_tool(
                step as u64,
                &call.id,
                &call.name,
                execution.is_success(),
                matches!(execution, ToolExecutionResult::Denied(_)),
            ) {
                outcome.event(
                    "tool_evidence",
                    Some(&call.name),
                    Some(&call.id),
                    Some(execution.is_success()),
                    &safe_output,
                );
                outcome.persist();
            }
            if send_runner(
                &events,
                turn_id,
                HeadlessEventV1::ToolFinish {
                    call_id: bounded_identifier(&call.id),
                    name: bounded_identifier(&call.name),
                    status: result_status(&execution),
                    output: safe_output,
                },
            )
            .await
            .is_err()
            {
                return;
            }
            if loop_incident.is_none() {
                loop_incident =
                    loop_guard.observe_batch([crate::loop_guard::LoopObservation::from_execution(
                        &call.name,
                        &call.arguments,
                        &execution,
                    )]);
            }
            if let Some(image) = execution.image() {
                completed_images.insert(call.id.clone(), image.clone());
            }
            completed_calls.push(ToolCall {
                id: call.id.clone(),
                name: call.name.clone(),
                arguments: call.arguments.clone(),
                status: if execution.is_success() {
                    ToolStatus::Done
                } else {
                    ToolStatus::Error
                },
                output: Some(crate::command_output::model_text_for_policy(
                    &execution,
                    run.command_output_policy,
                )),
            });
            run.discovery.touch(&call.name);
        }
        let message = Message {
            role: Role::Assistant,
            content: turn.content,
            tool_calls: Some(completed_calls),
            thinking: turn.reasoning,
            response_items: turn.response_items,
            timestamp: chrono::Utc::now(),
        };
        if let Err(error) = crate::agent::client::append_provider_messages(&mut messages, &message)
        {
            {
                outcome.record_failure();
            }
            fail_turn(
                &events,
                turn_id,
                HeadlessFailureKind::Internal,
                bounded_safe_text(&error.to_string(), &run.redaction_refs()),
                8,
            )
            .await;
            return;
        }
        crate::agent::client::append_transient_tool_images(&mut messages, &completed_images);
        discovery_batch.commit();
        if let Err(message) = run.inbox.lock().await.checkpoint(messages.clone()).await {
            fail_turn(&events, turn_id, HeadlessFailureKind::Internal, message, 8).await;
            return;
        }
        if events
            .send(WorkerEvent::MessagesUpdated {
                turn_id,
                messages: messages.clone(),
            })
            .await
            .is_err()
        {
            return;
        }
        if let Some(incident) = loop_incident {
            match wait_for_loop_guard_decision(turn_id, incident, &events, &mut controls).await {
                Some(ServiceLoopDecision::Replan) => {
                    loop_guard.replan();
                    loop_guard_replan_prompt = Some(
                        "A low-yield loop was detected. Replan now and use a materially different approach; do not repeat the same edit or command sequence."
                            .to_string(),
                    );
                }
                Some(ServiceLoopDecision::Continue) => loop_guard.suppress_for_turn(),
                Some(ServiceLoopDecision::Cancel) | None => {
                    let _ = events.send(WorkerEvent::Aborted { turn_id }).await;
                    return;
                }
            }
        }
    }

    {
        outcome.record_failure();
    }
    fail_turn(
        &events,
        turn_id,
        HeadlessFailureKind::MaximumTurns,
        "maximum turns reached without a terminal response".into(),
        7,
    )
    .await;
}

pub(crate) fn scrub_email_chat_messages(messages: &mut [ChatMessage]) {
    let mut private_ids = HashSet::new();
    for message in messages.iter_mut() {
        if let Some(calls) = message.tool_calls.as_mut() {
            for call in calls {
                if crate::email::is_email_tool(&call.function.name) {
                    private_ids.insert(call.id.clone());
                    call.function.arguments =
                        crate::email::private_arguments_marker(&call.function.name);
                }
            }
        }
    }
    for message in messages.iter_mut() {
        if message
            .tool_call_id
            .as_ref()
            .is_some_and(|id| private_ids.contains(id))
        {
            message.content = crate::image_content::ModelContent::text(
                "[External private email result unavailable after active tool loop]",
            );
        }
    }
}

fn service_tool_allowed(spec: &crate::agent::registry::ToolSpec) -> bool {
    if matches!(spec.origin, ToolOrigin::Mcp(_)) {
        return true;
    }
    !spec.capabilities.contains(&ToolCapability::Subagent)
        && !spec.capabilities.contains(&ToolCapability::Privilege)
        && !matches!(
            spec.handler,
            ToolHandler::WatchProcess | ToolHandler::StopWatcher
        )
        && !matches!(
            spec.handler,
            ToolHandler::ProposePlan
                | ToolHandler::ConfigureAgentGraph
                | ToolHandler::ProposeMemory
                | ToolHandler::EditPlan
                | ToolHandler::UpdatePlanTask
                | ToolHandler::RecordPlanEvidence
                | ToolHandler::SwitchMode
                | ToolHandler::SendMessage
        )
}

async fn send_runner(
    events: &mpsc::Sender<WorkerEvent>,
    turn_id: Uuid,
    event: HeadlessEventV1,
) -> Result<(), ()> {
    events
        .send(WorkerEvent::Runner { turn_id, event })
        .await
        .map_err(|_| ())
}

async fn fail_turn(
    events: &mpsc::Sender<WorkerEvent>,
    turn_id: Uuid,
    kind: HeadlessFailureKind,
    message: String,
    exit_code: i32,
) {
    let _ = send_runner(
        events,
        turn_id,
        HeadlessEventV1::Failure {
            kind,
            message,
            exit_code,
        },
    )
    .await;
    let _ = events.send(WorkerEvent::Failed { turn_id }).await;
}

enum ExecuteOutcome {
    Result(ToolExecutionResult),
    Abort,
    Protocol(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ServiceLoopDecision {
    Replan,
    Continue,
    Cancel,
}

async fn wait_for_loop_guard_decision(
    turn_id: Uuid,
    incident: crate::loop_guard::LoopIncident,
    events: &mpsc::Sender<WorkerEvent>,
    controls: &mut mpsc::Receiver<TurnControl>,
) -> Option<ServiceLoopDecision> {
    let interaction = InteractionRequest {
        question: "Low-yield loop detected. How should this turn continue?".into(),
        description: Some(incident.evidence),
        mode: InteractionMode::SingleChoice,
        choices: vec![
            InteractionChoice {
                label: "Replan".into(),
                value: Some("replan".into()),
                description: Some("Require a materially different next approach".into()),
            },
            InteractionChoice {
                label: "Continue".into(),
                value: Some("continue".into()),
                description: Some("Suppress the detector for this turn".into()),
            },
            InteractionChoice {
                label: "Cancel".into(),
                value: Some("cancel".into()),
                description: Some("Stop the current turn".into()),
            },
        ],
        allow_other: false,
    };
    let request = PendingRequest {
        id: wire_request_id("loop"),
        kind: PendingKind::Interaction,
        interaction: Some(interaction.clone()),
        approval_digest: None,
    };
    if events
        .send(WorkerEvent::InteractionRequested {
            turn_id,
            request,
            call_id: "loop_guard".into(),
            interaction,
        })
        .await
        .is_err()
    {
        return None;
    }
    let TurnControl::Interaction { selected, .. } = controls.recv().await? else {
        return None;
    };
    match selected.as_slice() {
        [decision] if decision == "replan" => Some(ServiceLoopDecision::Replan),
        [decision] if decision == "continue" => Some(ServiceLoopDecision::Continue),
        [decision] if decision == "cancel" => Some(ServiceLoopDecision::Cancel),
        _ => None,
    }
}

async fn prepare_service_native_read(
    run: &ServiceRunConfig,
    call: &AssembledToolCall,
) -> Option<crate::agent::tools::PreparedNativeRead> {
    if !ToolRegistry::builtins().supports_native_read(&call.name, run.tool_profile) {
        return None;
    }
    if let Some(manager) = &run.mcp {
        if manager.has_active_tool(&call.name).await {
            return None;
        }
    }
    let raw: Value = serde_json::from_str(&call.arguments).ok()?;
    let broker = crate::approval::ApprovalBroker::with_policy(
        &run.workspace,
        &run.approval_rules,
        &run.permission_rules,
        &[],
        crate::approval::ApprovalOrigin::Main,
    )
    .with_caller(crate::approval::DecisionCaller::Service);
    let (prepared, decision) = broker.prepare(&call.name, &raw, ToolOrigin::BuiltIn).ok()?;
    (decision == crate::approval::BrokerDecision::AllowedByRule && !prepared.network_requested)
        .then_some(crate::agent::tools::PreparedNativeRead {
            name: call.name.clone(),
            args: prepared.arguments,
        })
}

async fn execute_service_call(
    run: &ServiceRunConfig,
    call: &AssembledToolCall,
    raw_arguments: &Value,
    turn_id: Uuid,
    events: &mpsc::Sender<WorkerEvent>,
    controls: &mut mpsc::Receiver<TurnControl>,
) -> ExecuteOutcome {
    let builtins = ToolRegistry::builtins();
    if call.name == "request_user_input" {
        if let Err(error) = builtins.validate(&call.name, raw_arguments) {
            return ExecuteOutcome::Protocol(error.to_string());
        }
        let args = match serde_json::from_value::<RequestUserInputArgs>(raw_arguments.clone()) {
            Ok(args) => args,
            Err(_) => {
                return ExecuteOutcome::Protocol("invalid request_user_input arguments".into())
            }
        };
        let interaction = match InteractionRequest::from_model(args) {
            Ok(interaction) => interaction,
            Err(error) => return ExecuteOutcome::Protocol(error),
        };
        let request_id = wire_request_id("int");
        let pending = PendingRequest {
            id: request_id,
            kind: PendingKind::Interaction,
            interaction: Some(interaction.clone()),
            approval_digest: None,
        };
        if events
            .send(WorkerEvent::InteractionRequested {
                turn_id,
                request: pending,
                call_id: bounded_identifier(&call.id),
                interaction: interaction.clone(),
            })
            .await
            .is_err()
        {
            return ExecuteOutcome::Abort;
        }
        let Some(TurnControl::Interaction { selected, text }) = controls.recv().await else {
            return ExecuteOutcome::Abort;
        };
        return match validate_interaction_answer(&interaction, selected, text) {
            Ok(answer) => match serde_json::to_string(&answer) {
                Ok(output) => ExecuteOutcome::Result(ToolExecutionResult::Success(output)),
                Err(_) => ExecuteOutcome::Protocol("could not encode interaction answer".into()),
            },
            Err(message) => ExecuteOutcome::Result(ToolExecutionResult::Failure(message)),
        };
    }

    if crate::email::is_outbound_tool(&call.name) {
        if let Err(error) = builtins.validate(&call.name, raw_arguments) {
            return ExecuteOutcome::Protocol(error.to_string());
        }
        let Some(manager) = &run.email else {
            return ExecuteOutcome::Result(ToolExecutionResult::Failure(
                "Email manager is unavailable for this session".into(),
            ));
        };
        let broker = crate::approval::ApprovalBroker::with_policy(
            &run.workspace,
            &run.approval_rules,
            &run.permission_rules,
            &[],
            crate::approval::ApprovalOrigin::Main,
        )
        .with_caller(crate::approval::DecisionCaller::Service);
        if matches!(
            broker.prepare(&call.name, raw_arguments, ToolOrigin::BuiltIn),
            Ok((_, crate::approval::BrokerDecision::DeniedByRule))
        ) {
            return ExecuteOutcome::Result(ToolExecutionResult::Denied(
                "Denied by persistent permission rule".into(),
            ));
        }
        let draft_ref = if call.name == "email_reply" {
            let reply = match serde_json::from_value::<crate::email::types::EmailReplyArgs>(
                raw_arguments.clone(),
            ) {
                Ok(reply) => reply,
                Err(_) => return ExecuteOutcome::Protocol("invalid email_reply arguments".into()),
            };
            match manager.prepare_reply(reply).await {
                Ok(draft) => draft.draft_ref,
                Err(error) => {
                    return ExecuteOutcome::Result(ToolExecutionResult::Failure(error.to_string()))
                }
            }
        } else {
            match serde_json::from_value::<crate::email::types::EmailSendArgs>(
                raw_arguments.clone(),
            ) {
                Ok(send) => send.draft_ref,
                Err(_) => return ExecuteOutcome::Protocol("invalid email_send arguments".into()),
            }
        };
        let preview = match manager.preview(&draft_ref).await {
            Ok(preview) => preview,
            Err(error) => {
                return ExecuteOutcome::Result(ToolExecutionResult::Failure(error.to_string()))
            }
        };
        let (approval_digest, metadata) = match crate::email::service_approval_metadata(&preview) {
            Ok(metadata) => metadata,
            Err(error) => {
                return ExecuteOutcome::Result(ToolExecutionResult::Failure(error.to_string()))
            }
        };
        match wait_for_external_approval(call, turn_id, events, controls, approval_digest, metadata)
            .await
        {
            ApprovalDecision::AllowOnce => {}
            ApprovalDecision::Deny => {
                return ExecuteOutcome::Result(ToolExecutionResult::Denied(
                    "Denied by client".into(),
                ))
            }
            ApprovalDecision::Abort => return ExecuteOutcome::Abort,
        }
        let nonce = match manager
            .authorize_once(
                &draft_ref,
                &call.id,
                &run.session_identity,
                std::time::Duration::from_secs(120),
            )
            .await
        {
            Ok(nonce) => nonce,
            Err(error) => {
                return ExecuteOutcome::Result(ToolExecutionResult::Failure(error.to_string()))
            }
        };
        return match manager
            .send_approved(
                &crate::email::types::EmailSendArgs { draft_ref },
                &nonce,
                &call.id,
                &run.session_identity,
            )
            .await
        {
            Ok(receipt) => match serde_json::to_string(&receipt) {
                Ok(live) => ExecuteOutcome::Result(ToolExecutionResult::ExternalPrivateSuccess {
                    live,
                    persistent: crate::email::private_result_marker(&call.name),
                }),
                Err(_) => ExecuteOutcome::Protocol("could not encode email receipt".into()),
            },
            Err(error) => ExecuteOutcome::Result(ToolExecutionResult::Failure(error.to_string())),
        };
    }

    if let Some(manager) = &run.mcp {
        if manager.has_active_tool(&call.name).await {
            if let Err(error) = manager
                .validate_active_tool(&call.name, raw_arguments)
                .await
            {
                return ExecuteOutcome::Protocol(error);
            }
            let server = manager
                .active_tool_server(&call.name)
                .await
                .unwrap_or_else(|| "unknown".into());
            let broker = crate::approval::ApprovalBroker::with_policy(
                &run.workspace,
                &run.approval_rules,
                &run.permission_rules,
                &[],
                crate::approval::ApprovalOrigin::Mcp(server.clone()),
            )
            .with_caller(crate::approval::DecisionCaller::Service);
            let (_, decision) =
                broker.prepare_dynamic(&call.name, raw_arguments, ToolOrigin::Mcp(server), false);
            if decision == crate::approval::BrokerDecision::DeniedByRule {
                return ExecuteOutcome::Result(ToolExecutionResult::Denied(
                    "Denied by persistent permission rule".into(),
                ));
            }
            if decision != crate::approval::BrokerDecision::AllowedByRule {
                match wait_for_approval(
                    call,
                    turn_id,
                    events,
                    controls,
                    "MCP tool execution requires client approval",
                    false,
                )
                .await
                {
                    ApprovalDecision::AllowOnce => {}
                    ApprovalDecision::Deny => {
                        return ExecuteOutcome::Result(ToolExecutionResult::Denied(
                            "Denied by client".into(),
                        ))
                    }
                    ApprovalDecision::Abort => return ExecuteOutcome::Abort,
                }
            }
            return match manager.call_tool(&call.name, raw_arguments.clone()).await {
                Ok(result) => ExecuteOutcome::Result(result),
                Err(error) => ExecuteOutcome::Protocol(error.to_string()),
            };
        }
    }

    let Some(spec) = builtins.get(&call.name) else {
        return ExecuteOutcome::Protocol(format!("unknown tool: {}", call.name));
    };
    let broker = crate::approval::ApprovalBroker::with_policy(
        &run.workspace,
        &run.approval_rules,
        &run.permission_rules,
        &[],
        crate::approval::ApprovalOrigin::Main,
    )
    .with_caller(crate::approval::DecisionCaller::Service);
    let (prepared, decision) = match broker
        .prepare_formatted(
            &call.name,
            raw_arguments,
            ToolOrigin::BuiltIn,
            false,
            run.sandbox_mode,
        )
        .await
    {
        Ok(prepared) => prepared,
        Err(error) => return ExecuteOutcome::Protocol(error),
    };
    if decision == crate::approval::BrokerDecision::DeniedByRule {
        return ExecuteOutcome::Result(ToolExecutionResult::Denied(
            "Denied by persistent permission rule".into(),
        ));
    }
    if prepared.network_requested && !run.allow_network {
        return ExecuteOutcome::Result(ToolExecutionResult::Denied(
            "Network authority was not enabled for this service session".into(),
        ));
    }
    let automatic_skill_activation = call.name == crate::skills::ACTIVATE_SKILL_TOOL_NAME
        && decision == crate::approval::BrokerDecision::Prompt;
    let manual = prepared.network_requested
        || (!automatic_skill_activation
            && matches!(
                decision,
                crate::approval::BrokerDecision::Prompt
                    | crate::approval::BrokerDecision::ExplicitPrompt
                    | crate::approval::BrokerDecision::NetworkPrompt
            ));
    if manual {
        let reason = if prepared.network_requested {
            "network-capable tool execution requires client approval"
        } else if decision == crate::approval::BrokerDecision::ExplicitPrompt {
            "an exact Ask rule requires client approval"
        } else if spec.mutation == ToolMutation::Mutating {
            "mutating tool execution requires client approval"
        } else {
            "tool execution requires client approval"
        };
        match wait_for_approval(
            call,
            turn_id,
            events,
            controls,
            reason,
            prepared.network_requested,
        )
        .await
        {
            ApprovalDecision::AllowOnce => {}
            ApprovalDecision::Deny => {
                return ExecuteOutcome::Result(ToolExecutionResult::Denied(
                    "Denied by client".into(),
                ))
            }
            ApprovalDecision::Abort => return ExecuteOutcome::Abort,
        }
    }

    if call.name == crate::tool_discovery::SEARCH_TOOLS_NAME {
        return ExecuteOutcome::Result(run.discovery.search(&prepared.arguments));
    }
    if call.name == "search_mcp_tools" {
        let Some(manager) = &run.mcp else {
            return ExecuteOutcome::Result(ToolExecutionResult::Failure(
                "MCP discovery is unavailable for this session".into(),
            ));
        };
        let query = prepared
            .arguments
            .get("query")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let server = prepared.arguments.get("server").and_then(Value::as_str);
        let limit = prepared
            .arguments
            .get("limit")
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .unwrap_or(5);
        return ExecuteOutcome::Result(manager.search_tools(query, server, limit).await);
    }
    if crate::lsp::is_lsp_tool(&call.name) {
        let Some(manager) = &run.lsp else {
            return ExecuteOutcome::Result(ToolExecutionResult::Failure(
                "LSP navigation is unavailable for this session".into(),
            ));
        };
        manager.update_workspace(run.workspace.clone()).await;
        return ExecuteOutcome::Result(manager.execute(&call.name, &prepared.arguments).await);
    }
    if crate::email::is_email_tool(&call.name) {
        let Some(manager) = &run.email else {
            return ExecuteOutcome::Result(ToolExecutionResult::Failure(
                "Email manager is unavailable for this session".into(),
            ));
        };
        return ExecuteOutcome::Result(
            manager
                .execute_read_or_draft(&call.name, &prepared.arguments)
                .await,
        );
    }

    ExecuteOutcome::Result(
        crate::agent::tools::execute_prepared_tool_in_workspace_policy_owned_with_skills(
            &call.name,
            &prepared.arguments,
            &run.workspace,
            run.command_timeout_secs,
            run.sandbox_mode,
            run.allow_network && prepared.network_requested,
            run.job_owner,
            &run.skills,
        )
        .await,
    )
}

async fn wait_for_approval(
    call: &AssembledToolCall,
    turn_id: Uuid,
    events: &mpsc::Sender<WorkerEvent>,
    controls: &mut mpsc::Receiver<TurnControl>,
    reason: &str,
    network: bool,
) -> ApprovalDecision {
    let request = PendingRequest {
        id: wire_request_id("apr"),
        kind: PendingKind::Approval,
        interaction: None,
        approval_digest: None,
    };
    if events
        .send(WorkerEvent::ApprovalRequested {
            turn_id,
            request,
            call_id: bounded_identifier(&call.id),
            name: bounded_identifier(&call.name),
            reason: reason.into(),
            network,
            approval_digest: None,
            email: None,
        })
        .await
        .is_err()
    {
        return ApprovalDecision::Abort;
    }
    match controls.recv().await {
        Some(TurnControl::Approval(decision)) => decision,
        _ => ApprovalDecision::Abort,
    }
}

async fn wait_for_external_approval(
    call: &AssembledToolCall,
    turn_id: Uuid,
    events: &mpsc::Sender<WorkerEvent>,
    controls: &mut mpsc::Receiver<TurnControl>,
    approval_digest: String,
    email: Value,
) -> ApprovalDecision {
    let request = PendingRequest {
        id: wire_request_id("email"),
        kind: PendingKind::Approval,
        interaction: None,
        approval_digest: Some(approval_digest.clone()),
    };
    if events
        .send(WorkerEvent::ApprovalRequested {
            turn_id,
            request,
            call_id: bounded_identifier(&call.id),
            name: bounded_identifier(&call.name),
            reason: "exact external communication requires client approval".into(),
            network: false,
            approval_digest: Some(approval_digest),
            email: Some(email),
        })
        .await
        .is_err()
    {
        return ApprovalDecision::Abort;
    }
    match controls.recv().await {
        Some(TurnControl::Approval(decision)) => decision,
        _ => ApprovalDecision::Abort,
    }
}

fn validate_interaction_answer(
    request: &InteractionRequest,
    selected: Vec<String>,
    text: Option<String>,
) -> Result<InteractionAnswer, String> {
    let text = text
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty());
    if text
        .as_ref()
        .is_some_and(|text| text.len() > MAX_PROMPT_BYTES)
    {
        return Err("interaction text exceeds the service input bound".into());
    }
    if selected.len() > 9 {
        return Err("too many interaction choices were selected".into());
    }
    let allowed = request
        .choices
        .iter()
        .map(|choice| choice.resolved_value())
        .collect::<HashSet<_>>();
    let unique = selected.iter().cloned().collect::<HashSet<_>>();
    if unique.len() != selected.len() || selected.iter().any(|value| !allowed.contains(value)) {
        return Err("interaction response contains an unknown or duplicate choice".into());
    }
    match request.mode {
        InteractionMode::FreeText if selected.is_empty() && text.is_some() => {}
        InteractionMode::SingleChoice | InteractionMode::Confirmation
            if (selected.len() == 1 && text.is_none())
                || (selected.is_empty() && request.allow_other && text.is_some()) => {}
        InteractionMode::MultiSelect
            if (!selected.is_empty() && text.is_none())
                || (request.allow_other && text.is_some()) => {}
        InteractionMode::Masked => {
            return Err("masked interactions are unavailable over stdio".into())
        }
        _ => return Err("interaction response does not match the requested mode".into()),
    }
    Ok(InteractionAnswer { selected, text })
}

fn wire_request_id(prefix: &str) -> String {
    format!("{prefix}_{}", Uuid::new_v4().simple())
}

fn provider_failure(error: &ProviderError) -> (i32, HeadlessFailureKind) {
    match error {
        ProviderError::FilterStopped { .. } => (11, HeadlessFailureKind::ProviderFilter),
        ProviderError::Http { .. }
        | ProviderError::Transport(_)
        | ProviderError::MissingEnvironment(_) => (5, HeadlessFailureKind::ProviderTransport),
        _ => (6, HeadlessFailureKind::ProviderProtocol),
    }
}

fn result_status(result: &ToolExecutionResult) -> ToolResultStatus {
    match result {
        ToolExecutionResult::Success(_)
        | ToolExecutionResult::Command { success: true, .. }
        | ToolExecutionResult::SuccessMutation { .. }
        | ToolExecutionResult::SuccessImage { .. }
        | ToolExecutionResult::MemoryProposal { .. }
        | ToolExecutionResult::ExternalPrivateSuccess { .. } => ToolResultStatus::Success,
        ToolExecutionResult::Failure(_) | ToolExecutionResult::Command { success: false, .. } => {
            ToolResultStatus::Failure
        }
        ToolExecutionResult::Denied(_) => ToolResultStatus::Denied,
        ToolExecutionResult::Cancelled(_) => ToolResultStatus::Cancelled,
    }
}

fn bounded_identifier(value: &str) -> String {
    let safe = crate::terminal_text::sanitize_terminal_text(value);
    truncate_with_marker(&safe, 256)
}

fn bounded_safe_text(value: &str, secrets: &[&str]) -> String {
    truncate_with_marker(
        &crate::request::safe_text(value, secrets),
        MAX_SAFE_TEXT_BYTES,
    )
}

fn truncate_with_marker(value: &str, limit: usize) -> String {
    if value.len() <= limit {
        return value.to_string();
    }
    let marker = format!("[… {} bytes omitted]", value.len().saturating_sub(limit));
    let mut end = limit.saturating_sub(marker.len()).min(value.len());
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{}", &value[..end], marker)
}

fn safe_tool_arguments(value: &Value, secrets: &[&str]) -> Value {
    fn visit(value: &Value, key: Option<&str>, secrets: &[&str], depth: usize) -> Value {
        if depth >= 16 {
            return Value::String("[nested content omitted]".into());
        }
        match value {
            Value::String(text) => {
                let mut safe = crate::request::safe_text(text, secrets);
                if key.is_some_and(sensitive_key) {
                    safe = "[REDACTED]".into();
                } else if key.is_some_and(|key| key.eq_ignore_ascii_case("url")) {
                    safe = sanitize_url(&safe);
                }
                Value::String(truncate_with_marker(&safe, 2_048))
            }
            Value::Array(values) => Value::Array(
                values
                    .iter()
                    .take(64)
                    .map(|value| visit(value, None, secrets, depth + 1))
                    .collect(),
            ),
            Value::Object(values) => Value::Object(
                values
                    .iter()
                    .take(64)
                    .map(|(key, value)| (key.clone(), visit(value, Some(key), secrets, depth + 1)))
                    .collect(),
            ),
            other => other.clone(),
        }
    }
    let safe = visit(value, None, secrets, 0);
    if serde_json::to_vec(&safe).is_ok_and(|encoded| encoded.len() <= MAX_SAFE_ARGUMENT_BYTES) {
        safe
    } else {
        serde_json::json!({"ca_notice": "tool arguments omitted by service output bound"})
    }
}

fn sensitive_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase().replace(['-', '_', ' '], "");
    matches!(
        key.as_str(),
        "authorization"
            | "apikey"
            | "accesskey"
            | "secretkey"
            | "password"
            | "token"
            | "secret"
            | "credential"
            | "credentials"
            | "auth"
    )
}

fn sanitize_url(value: &str) -> String {
    let Ok(mut url) = reqwest::Url::parse(value) else {
        return value.into();
    };
    let _ = url.set_username("");
    let _ = url.set_password(None);
    url.set_query(None);
    url.set_fragment(None);
    url.to_string()
}

struct ProductionFactory {
    active_profile: ActiveProfile,
    allow_network: bool,
    startup_cwd: PathBuf,
}

#[async_trait]
impl ServiceSessionFactory for ProductionFactory {
    fn session_store(&self) -> Result<crate::session::SessionStore, String> {
        let paths = crate::runtime::RuntimePaths::discover()
            .map_err(|e| e.to_string())?
            .for_profile(&self.active_profile);
        Ok(crate::session::SessionStore::new(paths.sessions_dir))
    }
    async fn create(&self, request: &StartSessionOp) -> Result<ServiceSession, String> {
        self.create_scoped(request, &[], None).await
    }
}

impl ProductionFactory {
    async fn create_scoped(
        &self,
        request: &StartSessionOp,
        additional_directories: &[PathBuf],
        session_id: Option<Uuid>,
    ) -> Result<ServiceSession, String> {
        if request.allow_network && !self.allow_network {
            return Err(
                "network authority was requested, but ca serve was not started with --allow-network"
                    .into(),
            );
        }
        let paths = crate::runtime::RuntimePaths::discover()
            .map_err(|error| error.to_string())?
            .for_profile(&self.active_profile);
        let mut config = crate::config::ConfigStore::for_profile(
            paths.config_file.clone(),
            self.active_profile.clone(),
        )
        .load()
        .map_err(|error| bounded_safe_text(&error.to_string(), &[]))?;
        crate::plugin::compose_enabled(&paths, &self.active_profile, &mut config)
            .map_err(|error| bounded_safe_text(&error.to_string(), &[]))?;
        if let Some(hub) = crate::provider_trace::process_hub() {
            hub.add_configured_secrets(
                config
                    .providers
                    .values()
                    .map(|provider| provider.api_key.as_str()),
            );
        }
        let provider_id = request
            .provider
            .clone()
            .unwrap_or_else(|| config.defaults.provider.clone());
        let provider = config
            .providers
            .get(&provider_id)
            .cloned()
            .ok_or_else(|| format!("configured provider '{provider_id}' does not exist"))?;
        if reqwest::Url::parse(&provider.base_url)
            .ok()
            .filter(|url| matches!(url.scheme(), "http" | "https"))
            .is_none()
        {
            return Err(format!(
                "configured provider '{provider_id}' has an invalid HTTP endpoint"
            ));
        }
        let model = request.model.clone().unwrap_or_else(|| {
            if provider_id == config.defaults.provider {
                config.defaults.model.clone()
            } else {
                provider.models.first().cloned().unwrap_or_default()
            }
        });
        let model = if model == "auto" {
            provider.models.first().cloned().unwrap_or_default()
        } else {
            model
        };
        if model.is_empty() || !provider.models.contains(&model) {
            return Err(format!(
                "model '{model}' is not configured for provider '{provider_id}'"
            ));
        }
        let prompt_cache_contract = crate::provider_contract::resolve_prompt_cache(
            &config,
            &provider_id,
            &provider,
            &model,
        );
        let mut workspace = crate::workspace::WorkspaceContext::discover(&self.startup_cwd)
            .map_err(|error| error.to_string())?;
        if let Some(cwd) = request.cwd.as_deref() {
            workspace.set_cwd(cwd).map_err(|error| error.to_string())?;
        }
        for directory in additional_directories {
            workspace
                .add_root(directory, false)
                .map_err(|error| error.to_string())?;
        }
        let cwd = workspace.cwd().to_string_lossy().into_owned();
        let capabilities = crate::runtime::RunCapabilities::for_run(&self.active_profile, true);
        let skill_registry = if capabilities.skills {
            crate::discover_skill_registry(&paths, &workspace, &config)
        } else {
            crate::skills::SkillRegistry::empty()
        };
        let recall = crate::memory::MemoryRecall::load(
            &paths,
            &self.active_profile,
            workspace
                .primary()
                .map(|root| root.path.as_path())
                .unwrap_or_else(|| workspace.cwd()),
            config.memory.user.recall,
            config.memory.workspace.recall,
        )
        .map_err(|error| error.to_string())?
        .with_local_hybrid(
            &paths,
            &self.active_profile,
            config.memory.local_hybrid.as_ref(),
        );
        let system =
            crate::build_system_prompt_parts_with_memory(&cwd, false, Some(&skill_registry), "")
                .render();
        let skills = Arc::new(Mutex::new(skill_registry));
        let allow_network = self.allow_network && request.allow_network;
        let sandbox_mode = crate::sandbox::SandboxMode::parse(&config.agent.sandbox_mode);
        let mcp = if capabilities.mcp {
            Some(Arc::new(
                crate::mcp::McpManager::load_with_oauth_store(
                    &config,
                    workspace.clone(),
                    sandbox_mode,
                    allow_network,
                    self.active_profile.clone(),
                    crate::mcp_oauth::secret_store_for_profile(&self.active_profile),
                )
                .map_err(|error| error.to_string())?,
            ))
        } else {
            None
        };
        let lsp = if capabilities.lsp {
            crate::lsp::LspManager::load(&config.lsp, workspace.clone(), sandbox_mode)
                .map_err(|error| error.to_string())?
                .map(Arc::new)
        } else {
            None
        };
        let email = if capabilities.email {
            crate::email::EmailManager::from_config(
                &config.email,
                paths.email_dir(&self.active_profile),
            )
            .map_err(|error| error.to_string())?
        } else {
            None
        };
        let selected_reasoning = config
            .effective_model_profile(&provider_id, &model)
            .and_then(|profile| {
                profile
                    .default_reasoning_effort
                    .filter(|effort| profile.reasoning_efforts.contains(effort))
            });
        let chat_options =
            crate::chat_request_options(&config, &provider_id, &model, selected_reasoning);
        let context_window_tokens = config
            .context_for_model(&provider_id, &model)
            .and_then(|context| context.context_window_tokens);
        let tools_supported = config
            .effective_model_profile(&provider_id, &model)
            .is_none_or(|profile| profile.supports_tools != Some(false));
        let images_supported = config
            .effective_model_profile(&provider_id, &model)
            .and_then(|profile| profile.image_input_support())
            .unwrap_or(false);
        let tool_policy = crate::tool_selection::ToolSelectionPolicy::from_cli(
            request.tools.clone(),
            request.include_tools.clone(),
            request.exclude_tools.clone(),
        )
        .map_err(|error| error.to_string())?
        .with_discovery_mode(config.agent.tool_discovery);
        let redaction_secrets = (!provider.api_key.is_empty())
            .then(|| provider.api_key.clone())
            .into_iter()
            .collect::<Vec<_>>();
        let session_id = session_id.unwrap_or_else(Uuid::new_v4);
        let backend: Arc<dyn ChatBackend> = Arc::new(
            crate::agent::AgentClient::with_request_store(
                provider_id.clone(),
                &provider,
                &model,
                crate::request::RequestStore::default(),
            )
            .with_prompt_cache_contract(prompt_cache_contract)
            .with_trace_session_id(session_id),
        );
        let (skill_activation_ids, skill_view_ids) = {
            let registry = skills.lock().unwrap_or_else(|poison| poison.into_inner());
            (
                registry.activation_selectors(),
                registry.viewable_instance_ids(),
            )
        };
        let lsp_enabled = capabilities.lsp && lsp.is_some();
        Ok(ServiceSession {
            id: session_id,
            provider: provider_id.clone(),
            model: model.clone(),
            cwd,
            messages: vec![ChatMessage {
                role: "system".into(),
                content: crate::image_content::ModelContent::text(system),
                tool_calls: None,
                tool_call_id: None,
                response_items: Vec::new(),
            }],
            backend,
            run: ServiceRunConfig {
                inbox: Arc::new(tokio::sync::Mutex::new(
                    crate::service_inbox::Runtime::default(),
                )),
                workspace_activity_scope: Some(paths.data_dir.clone()),
                outcome_sink: capabilities
                    .learning
                    .then(|| crate::session_outcome::OutcomeSink::new(paths.data_dir.clone())),
                provider: provider_id,
                model,
                workspace,
                max_turns: config.agent.max_turns,
                command_timeout_secs: config.agent.command_timeout_secs,
                command_output_policy: config.agent.command_output,
                show_thinking: request.show_thinking,
                approval_rules: config.approval_rules,
                permission_rules: config.permission_rules,
                allow_network,
                chat_options,
                sandbox_mode,
                skills: skills.clone(),
                task_skill_prompt: String::new(),
                memory_recall: recall,
                mcp,
                lsp: lsp.clone(),
                email,
                session_identity: session_id.to_string(),
                discovery: crate::tool_discovery::ToolDiscoverySession::default(),
                tool_policy,
                tool_constraints: crate::tool_selection::ToolSelectionConstraints {
                    tools_supported,
                    images_supported,
                    skills_enabled: capabilities.skills,
                    skill_activation_ids: Some(skill_activation_ids),
                    skill_view_ids: Some(skill_view_ids),
                    mcp_enabled: capabilities.mcp,
                    native_mechanistic_enabled: false,
                    mechanistic_delegation_enabled: false,
                    transparent_mechanistic_enabled: false,
                    remote_mechanistic_enabled: false,
                    automation_setup_enabled: false,
                    lsp_enabled,
                    memory_generation_enabled: false,
                    automation_proposals_enabled: false,
                    email_enabled: capabilities.email && config.email.enabled,
                    allowed_names: None,
                },
                tool_profile: ToolProfile::Main,
                context_window_tokens,
                job_owner: crate::jobs::JobOwner::new(None),
                redaction_secrets,
            },
        })
    }
}

/// Protocol-neutral local-agent bridge shared by ACP and the native service
/// runner. It deliberately exposes neither provider credentials nor direct
/// filesystem/terminal callbacks to protocol adapters.
#[derive(Clone)]
pub(crate) struct LocalAgentFactory {
    active_profile: ActiveProfile,
    allow_network: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LocalAgentMode {
    Agent,
    Plan,
    Ask,
}

pub(crate) struct LocalAgentSession {
    inner: Option<ServiceSession>,
    mode: LocalAgentMode,
}

pub(crate) struct LocalAgentTurn {
    events: mpsc::Receiver<WorkerEvent>,
    controls: mpsc::Sender<TurnControl>,
    task: tokio::task::JoinHandle<()>,
    latest_messages: Option<Vec<ChatMessage>>,
    cancelled: Arc<AtomicBool>,
    cancellation_notify: Arc<tokio::sync::Notify>,
    context_window_tokens: Option<u64>,
}

#[derive(Debug, Clone)]
pub(crate) enum LocalAgentEvent {
    Runner(HeadlessEventV1),
    ApprovalRequested {
        call_id: String,
        name: String,
        reason: String,
        network: bool,
    },
    InteractionRequested,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Clone)]
pub(crate) struct LocalAgentCancellation {
    abort: tokio::task::AbortHandle,
    cancelled: Arc<AtomicBool>,
    notify: Arc<tokio::sync::Notify>,
}

impl LocalAgentCancellation {
    pub(crate) fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        self.abort.abort();
        self.notify.notify_waiters();
    }

    pub(crate) async fn cancelled(&self) {
        loop {
            let notified = self.notify.notified();
            if self.cancelled.load(Ordering::Acquire) {
                return;
            }
            notified.await;
        }
    }
}

impl LocalAgentFactory {
    pub(crate) fn new(active_profile: ActiveProfile, allow_network: bool) -> Self {
        Self {
            active_profile,
            allow_network,
        }
    }

    async fn create_scoped(
        &self,
        cwd: &Path,
        additional_directories: &[PathBuf],
        provider: Option<String>,
        model: Option<String>,
        session_id: Option<Uuid>,
    ) -> Result<ServiceSession, String> {
        validate_local_roots(cwd, additional_directories)?;
        let startup_cwd = cwd
            .canonicalize()
            .map_err(|_| "ACP cwd must be an existing directory".to_string())?;
        let request = StartSessionOp {
            provider,
            model,
            cwd: Some(startup_cwd.to_string_lossy().into_owned()),
            show_thinking: true,
            allow_network: self.allow_network,
            ..StartSessionOp::default()
        };
        ProductionFactory {
            active_profile: self.active_profile.clone(),
            allow_network: self.allow_network,
            startup_cwd,
        }
        .create_scoped(&request, additional_directories, session_id)
        .await
    }

    pub(crate) async fn new_session(
        &self,
        cwd: &Path,
        additional_directories: &[PathBuf],
    ) -> Result<LocalAgentSession, String> {
        let inner = self
            .create_scoped(cwd, additional_directories, None, None, None)
            .await?;
        Ok(LocalAgentSession {
            inner: Some(inner),
            mode: LocalAgentMode::Agent,
        })
    }

    pub(crate) async fn load_session(
        &self,
        id: Uuid,
        cwd: &Path,
        additional_directories: &[PathBuf],
    ) -> Result<LocalAgentSession, String> {
        validate_local_roots(cwd, additional_directories)?;
        let paths = crate::runtime::RuntimePaths::discover()
            .map_err(|error| error.to_string())?
            .for_profile(&self.active_profile);
        let saved = crate::session::SessionStore::new(paths.sessions_dir)
            .load(&id.to_string())
            .map_err(|error| bounded_safe_text(&error.to_string(), &[]))?;
        let requested_cwd = cwd
            .canonicalize()
            .map_err(|_| "ACP cwd must be an existing directory".to_string())?;
        let saved_cwd = PathBuf::from(&saved.workspace)
            .canonicalize()
            .map_err(|_| "saved session workspace is unavailable".to_string())?;
        if requested_cwd != saved_cwd {
            return Err("ACP load cwd does not match the saved session workspace".into());
        }
        let mut inner = self
            .create_scoped(
                &saved_cwd,
                additional_directories,
                Some(saved.provider.clone()),
                Some(saved.model.clone()),
                Some(saved.id),
            )
            .await?;
        let assembly = crate::agent::AgentClient::build_segmented_messages(
            None,
            None,
            &saved.messages,
            None,
            &[],
        )
        .map_err(|error| format!("saved session history is invalid: {error}"))?;
        inner.messages.extend(assembly.messages);
        Ok(LocalAgentSession {
            inner: Some(inner),
            mode: LocalAgentMode::Agent,
        })
    }
}

fn validate_local_roots(cwd: &Path, additional_directories: &[PathBuf]) -> Result<(), String> {
    if !cwd.is_absolute() {
        return Err("ACP cwd must be absolute".into());
    }
    if additional_directories.len() > 32 {
        return Err("ACP sessions are limited to 32 additional directories".into());
    }
    if additional_directories
        .iter()
        .any(|path| !path.is_absolute())
    {
        return Err("ACP additional directories must be absolute".into());
    }
    Ok(())
}

impl LocalAgentSession {
    #[cfg(test)]
    pub(crate) fn for_test(
        id: Uuid,
        root: &Path,
        backend: Arc<dyn ChatBackend>,
    ) -> Result<Self, String> {
        let workspace = crate::workspace::WorkspaceContext::discover(root)
            .map_err(|error| error.to_string())?;
        let cwd = workspace.cwd().to_string_lossy().into_owned();
        Ok(Self {
            inner: Some(ServiceSession {
                id,
                provider: "fake".into(),
                model: "fake-model".into(),
                cwd,
                messages: vec![ChatMessage {
                    role: "system".into(),
                    content: crate::image_content::ModelContent::text("test system"),
                    tool_calls: None,
                    tool_call_id: None,
                    response_items: Vec::new(),
                }],
                backend,
                run: ServiceRunConfig {
                    inbox: Arc::new(tokio::sync::Mutex::new(
                        crate::service_inbox::Runtime::default(),
                    )),
                    workspace_activity_scope: None,
                    outcome_sink: None,
                    provider: "fake".into(),
                    model: "fake-model".into(),
                    workspace,
                    max_turns: 4,
                    command_timeout_secs: 2,
                    command_output_policy: crate::config::CommandOutputPolicy::Bounded,
                    show_thinking: true,
                    approval_rules: Vec::new(),
                    permission_rules: Vec::new(),
                    allow_network: false,
                    chat_options: ChatRequestOptions::default(),
                    sandbox_mode: crate::sandbox::SandboxMode::PolicyOnly,
                    skills: Arc::new(Mutex::new(crate::skills::SkillRegistry::empty())),
                    task_skill_prompt: String::new(),
                    memory_recall: crate::memory::MemoryRecall::default(),
                    mcp: None,
                    lsp: None,
                    email: None,
                    session_identity: id.to_string(),
                    discovery: crate::tool_discovery::ToolDiscoverySession::default(),
                    tool_policy: crate::tool_selection::ToolSelectionPolicy::default(),
                    tool_constraints: crate::tool_selection::ToolSelectionConstraints::default(),
                    tool_profile: ToolProfile::Main,
                    context_window_tokens: Some(8_192),
                    job_owner: crate::jobs::JobOwner::new(None),
                    redaction_secrets: Vec::new(),
                },
            }),
            mode: LocalAgentMode::Agent,
        })
    }

    pub(crate) fn id(&self) -> Uuid {
        self.inner.as_ref().expect("live local session").id
    }

    pub(crate) fn workspace_roots(&self) -> Vec<PathBuf> {
        self.inner
            .as_ref()
            .expect("live local session")
            .run
            .workspace
            .roots()
            .iter()
            .map(|root| root.path.clone())
            .collect()
    }

    pub(crate) fn mode(&self) -> LocalAgentMode {
        self.mode
    }

    pub(crate) fn set_mode(&mut self, mode: LocalAgentMode) {
        self.mode = mode;
        let session = self.inner.as_mut().expect("live local session");
        session.run.tool_profile = match mode {
            LocalAgentMode::Plan => ToolProfile::Plan,
            LocalAgentMode::Agent | LocalAgentMode::Ask => ToolProfile::Main,
        };
        session.run.tool_constraints.allowed_names =
            matches!(mode, LocalAgentMode::Ask).then(HashSet::new);
        let registry = session
            .run
            .skills
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let mut parts = crate::build_system_prompt_parts_with_memory_and_workspace(
            &session.cwd,
            Some(&session.run.workspace),
            matches!(mode, LocalAgentMode::Plan),
            Some(&registry),
            "",
        );
        if matches!(mode, LocalAgentMode::Ask) {
            crate::apply_agent_mode_guidance(&mut parts, &crate::app::AgentMode::Ask);
        }
        if let Some(system) = session
            .messages
            .first_mut()
            .filter(|message| message.role == "system")
        {
            system.content = crate::image_content::ModelContent::text(parts.render());
        }
    }

    pub(crate) fn start_turn(&mut self, prompt: String) -> Result<LocalAgentTurn, String> {
        if prompt.is_empty() || prompt.len() > MAX_ACP_PROMPT_BYTES || prompt.contains('\0') {
            return Err(format!(
                "ACP assembled prompt must be 1-{MAX_ACP_PROMPT_BYTES} safe UTF-8 bytes"
            ));
        }
        let session = self.inner.as_mut().expect("live local session");
        let mut messages = session.messages.clone();
        messages.push(ChatMessage {
            role: "user".into(),
            content: crate::image_content::ModelContent::text(prompt),
            tool_calls: None,
            tool_call_id: None,
            response_items: Vec::new(),
        });
        let turn_id = Uuid::new_v4();
        let (events_tx, events_rx) = mpsc::channel(WORKER_CHANNEL_CAPACITY);
        let (control_tx, control_rx) = mpsc::channel(CONTROL_CHANNEL_CAPACITY);
        let backend = session.backend.clone();
        let run = session.run.clone();
        let context_window_tokens = run.context_window_tokens;
        let task = tokio::spawn(run_turn(
            backend, messages, run, turn_id, events_tx, control_rx,
        ));
        Ok(LocalAgentTurn {
            events: events_rx,
            controls: control_tx,
            task,
            latest_messages: None,
            cancelled: Arc::new(AtomicBool::new(false)),
            cancellation_notify: Arc::new(tokio::sync::Notify::new()),
            context_window_tokens,
        })
    }

    pub(crate) fn apply_turn(&mut self, turn: &mut LocalAgentTurn) {
        if let Some(messages) = turn.latest_messages.take() {
            self.inner.as_mut().expect("live local session").messages = messages;
        }
    }

    pub(crate) async fn cleanup_after_cancel(&self) {
        cleanup_owned_jobs(
            self.inner
                .as_ref()
                .expect("live local session")
                .run
                .job_owner,
        )
        .await;
    }

    pub(crate) async fn close(&mut self) {
        if let Some(inner) = self.inner.take() {
            shutdown_session(inner).await;
        }
    }
}

impl LocalAgentTurn {
    pub(crate) fn cancellation(&self) -> LocalAgentCancellation {
        LocalAgentCancellation {
            abort: self.task.abort_handle(),
            cancelled: self.cancelled.clone(),
            notify: self.cancellation_notify.clone(),
        }
    }

    pub(crate) fn context_window_tokens(&self) -> Option<u64> {
        self.context_window_tokens
    }

    pub(crate) async fn allow_once(&self) -> Result<(), String> {
        self.controls
            .send(TurnControl::Approval(ApprovalDecision::AllowOnce))
            .await
            .map_err(|_| "ACP turn no longer accepts permission responses".into())
    }

    pub(crate) async fn deny(&self) -> Result<(), String> {
        self.controls
            .send(TurnControl::Approval(ApprovalDecision::Deny))
            .await
            .map_err(|_| "ACP turn no longer accepts permission responses".into())
    }

    pub(crate) async fn next_event(&mut self) -> Option<LocalAgentEvent> {
        loop {
            let Some(event) = self.events.recv().await else {
                return Some(if self.cancelled.load(Ordering::Acquire) {
                    LocalAgentEvent::Cancelled
                } else {
                    LocalAgentEvent::Failed
                });
            };
            match event {
                WorkerEvent::Runner { event, .. } => {
                    return Some(LocalAgentEvent::Runner(event));
                }
                WorkerEvent::MessagesUpdated { messages, .. } => {
                    self.latest_messages = Some(messages);
                }
                WorkerEvent::ApprovalRequested {
                    call_id,
                    name,
                    reason,
                    network,
                    ..
                } => {
                    return Some(LocalAgentEvent::ApprovalRequested {
                        call_id,
                        name,
                        reason,
                        network,
                    });
                }
                WorkerEvent::InteractionRequested { .. } => {
                    return Some(LocalAgentEvent::InteractionRequested);
                }
                WorkerEvent::Completed { messages, .. } => {
                    self.latest_messages = Some(messages);
                    return Some(LocalAgentEvent::Completed);
                }
                WorkerEvent::Failed { .. } => return Some(LocalAgentEvent::Failed),
                WorkerEvent::Aborted { .. } => return Some(LocalAgentEvent::Cancelled),
            }
        }
    }

    pub(crate) async fn join(&mut self) {
        let _ = (&mut self.task).await;
    }
}

impl Drop for LocalAgentTurn {
    fn drop(&mut self) {
        if !self.task.is_finished() {
            self.cancelled.store(true, Ordering::Release);
            self.task.abort();
            self.cancellation_notify.notify_waiters();
        }
    }
}

fn production_factory(args: &ServiceCliArgs) -> Arc<ProductionFactory> {
    let startup_cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        Err(_) => PathBuf::from("."),
    };
    Arc::new(ProductionFactory {
        active_profile: args.active_profile.clone(),
        allow_network: args.allow_network,
        startup_cwd,
    })
}

pub async fn run_production(args: ServiceCliArgs) -> i32 {
    let trace_hub = match crate::runtime::RuntimePaths::discover() {
        Ok(paths) => crate::provider_trace::ProviderTraceHub::new(
            paths.provider_trace_dir(&args.active_profile),
            !matches!(args.active_profile, ActiveProfile::Bare),
        ),
        Err(_) if !args.trace => {
            crate::provider_trace::ProviderTraceHub::new(PathBuf::new(), false)
        }
        Err(_) => {
            eprintln!("ca serve: runtime paths are unavailable");
            return 1;
        }
    };
    if args.trace {
        if let Err(error) = trace_hub.start() {
            eprintln!("ca serve: {error}");
            return 1;
        }
    }
    crate::provider_trace::install_process_hub(trace_hub.clone());
    let diagnostic_hub = trace_hub.clone();
    let diagnostic_watcher = args.trace.then(|| {
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                if let Some(diagnostic) = diagnostic_hub.take_diagnostic() {
                    eprintln!("ca serve: {diagnostic}");
                    break;
                }
                if diagnostic_hub.status().trace_id.is_none() {
                    break;
                }
            }
        })
    });
    let exit_code = match args.transport.clone() {
        ServiceTransport::Stdio => run_production_stdio(args).await,
        ServiceTransport::WebSocket { listen, auth } => {
            run_production_websocket(args, listen, auth).await
        }
    };
    if let Some(diagnostic) = crate::provider_trace::process_hub()
        .as_ref()
        .and_then(crate::provider_trace::ProviderTraceHub::take_diagnostic)
    {
        eprintln!("ca serve: {diagnostic}");
    }
    if let Some(watcher) = diagnostic_watcher {
        watcher.abort();
    }
    trace_hub.stop_and_flush(std::time::Duration::from_secs(2));
    if let Some(diagnostic) = trace_hub.take_diagnostic() {
        eprintln!("ca serve: {diagnostic}");
    }
    exit_code
}

async fn run_production_stdio(args: ServiceCliArgs) -> i32 {
    let factory = production_factory(&args);
    let reader = BufReader::new(tokio::io::stdin());
    match run_protocol(reader, tokio::io::stdout(), factory).await {
        Ok(()) | Err(ServiceIoError::BrokenPipe) => 0,
        Err(ServiceIoError::Other(message)) => {
            eprintln!("ca serve: {}", bounded_safe_text(&message, &[]));
            1
        }
    }
}

fn websocket_config() -> WebSocketConfig {
    let mut config = WebSocketConfig::default();
    config.read_buffer_size = 8 * 1024;
    config.write_buffer_size = 8 * 1024;
    config.max_write_buffer_size = MAX_LINE_BYTES * 4;
    config.max_message_size = Some(MAX_LINE_BYTES);
    config.max_frame_size = Some(MAX_LINE_BYTES);
    config
}

fn handshake_error(status: http::StatusCode, message: &str) -> HandshakeErrorResponse {
    http::Response::builder()
        .status(status)
        .header(http::header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(Some(message.into()))
        .unwrap_or_else(|_| http::Response::new(Some("request refused".into())))
}

fn request_is_authorized(request: &HandshakeRequest, expected_digest: &[u8; 32]) -> bool {
    let Some(value) = request.headers().get(http::header::AUTHORIZATION) else {
        return false;
    };
    let bytes = value.as_bytes();
    let Some(token) = bytes.strip_prefix(b"Bearer ") else {
        return false;
    };
    if !(TOKEN_MIN_BYTES..=TOKEN_MAX_BYTES).contains(&token.len()) {
        return false;
    }
    let candidate: [u8; 32] = Sha256::digest(token).into();
    constant_time_eq(&candidate, expected_digest)
}

async fn accept_authenticated_websocket(
    stream: TcpStream,
    expected_digest: [u8; 32],
    busy: bool,
) -> Result<WebSocketStream<TcpStream>, ()> {
    let callback = AuthHandshakeCallback {
        expected_digest,
        busy,
    };
    match tokio::time::timeout(
        WEBSOCKET_HANDSHAKE_TIMEOUT,
        accept_hdr_async_with_config(stream, callback, Some(websocket_config())),
    )
    .await
    {
        Ok(Ok(socket)) => Ok(socket),
        Ok(Err(_)) | Err(_) => Err(()),
    }
}

struct AuthHandshakeCallback {
    expected_digest: [u8; 32],
    busy: bool,
}

impl HandshakeCallback for AuthHandshakeCallback {
    fn on_request(
        self,
        request: &HandshakeRequest,
        response: HandshakeResponse,
    ) -> Result<HandshakeResponse, HandshakeErrorResponse> {
        if request.headers().contains_key(http::header::ORIGIN) {
            return Err(handshake_error(
                http::StatusCode::FORBIDDEN,
                "browser-origin WebSocket requests are not accepted",
            ));
        }
        if !request_is_authorized(request, &self.expected_digest) {
            return Err(handshake_error(
                http::StatusCode::UNAUTHORIZED,
                "authentication required",
            ));
        }
        if self.busy {
            return Err(handshake_error(
                http::StatusCode::SERVICE_UNAVAILABLE,
                "service already has an active client",
            ));
        }
        Ok(response)
    }
}

struct ActiveConnectionGuard(Arc<AtomicBool>);

impl Drop for ActiveConnectionGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

async fn close_protocol_input(input: &mut Option<tokio::io::DuplexStream>) {
    if let Some(mut input) = input.take() {
        let _ = input.shutdown().await;
    }
}

async fn bridge_websocket<F>(
    socket: WebSocketStream<TcpStream>,
    factory: Arc<F>,
    mut shutdown: watch::Receiver<bool>,
) where
    F: ServiceSessionFactory + 'static,
{
    let (protocol_reader, input) = tokio::io::duplex(MAX_LINE_BYTES * 2);
    let (output, protocol_writer) = tokio::io::duplex(MAX_LINE_BYTES * 2);
    let mut input = Some(input);
    let mut output = BufReader::new(output).lines();
    let mut protocol = tokio::spawn(run_protocol_with_transport(
        BufReader::new(protocol_reader),
        protocol_writer,
        factory,
        "websocket",
    ));
    let (mut sink, mut stream) = socket.split();
    let mut socket_open = true;

    loop {
        tokio::select! {
            changed = shutdown.changed(), if socket_open => {
                if changed.is_err() || *shutdown.borrow() {
                    close_protocol_input(&mut input).await;
                    let _ = sink.send(WebSocketMessage::Close(Some(CloseFrame {
                        code: tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode::Away,
                        reason: "service stopping".into(),
                    }))).await;
                    socket_open = false;
                }
            }
            message = stream.next(), if socket_open => {
                match message {
                    Some(Ok(WebSocketMessage::Text(text)))
                        if text.len() <= MAX_LINE_BYTES
                            && !text.as_str().contains(['\n', '\r']) =>
                    {
                        if let Some(protocol_input) = input.as_mut() {
                            if protocol_input.write_all(text.as_bytes()).await.is_err()
                                || protocol_input.write_all(b"\n").await.is_err()
                            {
                                close_protocol_input(&mut input).await;
                                socket_open = false;
                            }
                        }
                    }
                    Some(Ok(WebSocketMessage::Ping(payload))) => {
                        if sink.send(WebSocketMessage::Pong(payload)).await.is_err() {
                            close_protocol_input(&mut input).await;
                            socket_open = false;
                        }
                    }
                    Some(Ok(WebSocketMessage::Pong(_))) => {}
                    Some(Ok(WebSocketMessage::Close(frame))) => {
                        close_protocol_input(&mut input).await;
                        let _ = sink.send(WebSocketMessage::Close(frame)).await;
                        socket_open = false;
                    }
                    Some(Ok(WebSocketMessage::Binary(_))) | Some(Ok(WebSocketMessage::Frame(_))) => {
                        close_protocol_input(&mut input).await;
                        let _ = sink.send(WebSocketMessage::Close(Some(CloseFrame {
                            code: tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode::Unsupported,
                            reason: "text frames only".into(),
                        }))).await;
                        socket_open = false;
                    }
                    Some(Ok(WebSocketMessage::Text(_))) | Some(Err(_)) | None => {
                        close_protocol_input(&mut input).await;
                        socket_open = false;
                    }
                }
            }
            line = output.next_line() => {
                match line {
                    Ok(Some(line)) if socket_open => {
                        if sink.send(WebSocketMessage::Text(line.into())).await.is_err() {
                            close_protocol_input(&mut input).await;
                            socket_open = false;
                        }
                    }
                    Ok(Some(_)) => {}
                    Ok(None) | Err(_) => break,
                }
            }
        }
    }

    close_protocol_input(&mut input).await;
    if socket_open {
        let _ = sink.send(WebSocketMessage::Close(None)).await;
    }
    if !protocol.is_finished()
        && tokio::time::timeout(WEBSOCKET_CLOSE_TIMEOUT, &mut protocol)
            .await
            .is_err()
    {
        protocol.abort();
        let _ = protocol.await;
    }
}

async fn run_production_websocket(
    args: ServiceCliArgs,
    listen: SocketAddr,
    auth: AuthTokenSource,
) -> i32 {
    if !listen.ip().is_loopback() {
        eprintln!("ca serve: WebSocket listener must use a literal loopback address");
        return 2;
    }
    let token = match load_auth_token(&auth) {
        Ok(token) => token,
        Err(error) => {
            eprintln!("ca serve: {error}");
            return 2;
        }
    };
    let expected_digest = token.digest();
    drop(token);
    let listener = match TcpListener::bind(listen).await {
        Ok(listener) => listener,
        Err(_) => {
            eprintln!("ca serve: loopback listener could not be opened");
            return 1;
        }
    };
    let local_address = match listener.local_addr() {
        Ok(address) => address,
        Err(_) => {
            eprintln!("ca serve: loopback listener address is unavailable");
            return 1;
        }
    };
    eprintln!("ca serve listening on ws://{local_address}");

    let factory = production_factory(&args);
    let active = Arc::new(AtomicBool::new(false));
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            signal = tokio::signal::ctrl_c() => {
                let _ = signal;
                break;
            }
            accepted = listener.accept() => {
                let Ok((stream, peer)) = accepted else {
                    eprintln!("ca serve: loopback listener failed");
                    let _ = shutdown_tx.send(true);
                    while connections.join_next().await.is_some() {}
                    return 1;
                };
                if !peer.ip().is_loopback() {
                    continue;
                }
                let claimed = active
                    .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok();
                let active = active.clone();
                let factory = factory.clone();
                let receiver = shutdown_rx.clone();
                connections.spawn(async move {
                    let socket = accept_authenticated_websocket(stream, expected_digest, !claimed).await;
                    if !claimed {
                        return;
                    }
                    let _guard = ActiveConnectionGuard(active);
                    if let Ok(socket) = socket {
                        bridge_websocket(socket, factory, receiver).await;
                    }
                });
            }
            joined = connections.join_next(), if !connections.is_empty() => {
                let _ = joined;
            }
        }
    }
    let _ = shutdown_tx.send(true);
    while connections.join_next().await.is_some() {}
    0
}

pub async fn run_attach(args: AttachCliArgs) -> i32 {
    let endpoint = match validate_attach_endpoint(&args.endpoint) {
        Ok(endpoint) => endpoint,
        Err(error) => {
            eprintln!("ca attach: {error}");
            return 2;
        }
    };
    let token = match load_auth_token(&args.auth) {
        Ok(token) => token,
        Err(error) => {
            eprintln!("ca attach: {error}");
            return 2;
        }
    };
    let mut request = match endpoint.as_str().into_client_request() {
        Ok(request) => request,
        Err(_) => {
            eprintln!("ca attach: endpoint could not be prepared");
            return 2;
        }
    };
    let authorization = match token.authorization_value() {
        Ok(value) => value,
        Err(error) => {
            eprintln!("ca attach: {error}");
            return 2;
        }
    };
    request
        .headers_mut()
        .insert(http::header::AUTHORIZATION, authorization);
    drop(token);
    let socket = match connect_async_with_config(request, Some(websocket_config()), false).await {
        Ok((socket, _)) => socket,
        Err(_) => {
            eprintln!("ca attach: authentication or loopback connection failed");
            return 4;
        }
    };

    let (mut sink, mut stream) = socket.split();
    let mut input = BufReader::new(tokio::io::stdin());
    let mut output = tokio::io::stdout();
    let mut input_open = true;
    let close_deadline = tokio::time::sleep(WEBSOCKET_CLOSE_TIMEOUT);
    tokio::pin!(close_deadline);
    loop {
        tokio::select! {
            signal = tokio::signal::ctrl_c() => {
                let _ = signal;
                let _ = sink.send(WebSocketMessage::Close(None)).await;
                return 130;
            }
            line = read_bounded_line(&mut input), if input_open => {
                match line {
                    Ok(BoundedLine::Line(line)) => {
                        let text = match String::from_utf8(line) {
                            Ok(text) => text,
                            Err(_) => {
                                eprintln!("ca attach: input must be UTF-8 JSONL");
                                let _ = sink.send(WebSocketMessage::Close(None)).await;
                                return 2;
                            }
                        };
                        if sink.send(WebSocketMessage::Text(text.into())).await.is_err() {
                            return 4;
                        }
                    }
                    Ok(BoundedLine::TooLong) => {
                        eprintln!("ca attach: operation line exceeds {MAX_LINE_BYTES} bytes");
                        let _ = sink.send(WebSocketMessage::Close(None)).await;
                        return 2;
                    }
                    Ok(BoundedLine::Eof) => {
                        input_open = false;
                        close_deadline.as_mut().reset(tokio::time::Instant::now() + WEBSOCKET_CLOSE_TIMEOUT);
                    }
                    Err(_) => {
                        eprintln!("ca attach: stdin could not be read");
                        let _ = sink.send(WebSocketMessage::Close(None)).await;
                        return 1;
                    }
                }
            }
            message = stream.next() => {
                match message {
                    Some(Ok(WebSocketMessage::Text(text)))
                        if text.len() <= MAX_LINE_BYTES
                            && !text.as_str().contains(['\n', '\r']) =>
                    {
                        if output.write_all(text.as_bytes()).await.is_err()
                            || output.write_all(b"\n").await.is_err()
                            || output.flush().await.is_err()
                        {
                            let _ = sink.send(WebSocketMessage::Close(None)).await;
                            return 0;
                        }
                    }
                    Some(Ok(WebSocketMessage::Ping(payload))) => {
                        if sink.send(WebSocketMessage::Pong(payload)).await.is_err() {
                            return 4;
                        }
                    }
                    Some(Ok(WebSocketMessage::Pong(_))) => {}
                    Some(Ok(WebSocketMessage::Close(_))) | None => return 0,
                    Some(Ok(WebSocketMessage::Binary(_)))
                    | Some(Ok(WebSocketMessage::Frame(_)))
                    | Some(Ok(WebSocketMessage::Text(_)))
                    | Some(Err(_)) => {
                        eprintln!("ca attach: server closed the protocol connection");
                        return 4;
                    }
                }
            }
            _ = &mut close_deadline, if !input_open => {
                let _ = sink.send(WebSocketMessage::Close(None)).await;
                return 0;
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::client::{ChatStreamItem, ToolDef};
    use crate::agent::stream::{FinishReason, UsageInfo};
    use std::collections::VecDeque;
    use tokio::io::{AsyncBufReadExt, AsyncReadExt};

    #[test]
    fn stream_anomalies_keep_the_headless_provider_protocol_failure_class() {
        assert_eq!(
            provider_failure(&ProviderError::StreamAnomaly(
                crate::agent::stream::StreamAnomalyKind::SuspectedToolEnvelope,
            )),
            (6, HeadlessFailureKind::ProviderProtocol)
        );
    }

    #[test]
    fn interactive_only_tools_are_never_advertised_by_service() {
        let registry = crate::agent::registry::ToolRegistry::builtins();
        for tool in [
            crate::privilege::TOOL_NAME,
            crate::process_watch::WATCH_PROCESS_TOOL_NAME,
            crate::process_watch::STOP_WATCHER_TOOL_NAME,
        ] {
            assert!(!service_tool_allowed(registry.get(tool).unwrap()), "{tool}");
        }
    }

    #[test]
    fn public_serve_wire_types_keep_the_released_v1_shape() {
        let operation: ServeOp = serde_json::from_value(serde_json::json!({"type":"status"}))
            .expect("public serve operation should decode released v1 JSON");
        assert!(matches!(operation, ServeOp::Status));
        assert_eq!(
            serde_json::to_value(ServeEvent::Goodbye).unwrap(),
            serde_json::json!({"type":"goodbye"})
        );
        assert!(std::any::type_name::<crate::schedule::ScheduleDefinition>()
            .ends_with("schedule::ScheduleDefinition"));
        assert!(std::any::type_name::<crate::plugin::PluginManifest>()
            .ends_with("plugin::PluginManifest"));
    }

    #[tokio::test]
    async fn low_yield_service_interaction_requires_an_explicit_typed_choice() {
        let (event_tx, mut event_rx) = mpsc::channel(2);
        let (control_tx, mut control_rx) = mpsc::channel(2);
        let incident = crate::loop_guard::LoopIncident {
            kind: crate::loop_guard::LoopIncidentKind::RepeatedFailedCommand,
            evidence: "Failing command `cargo test` returned unchanged output 3 times".into(),
            occurrences: 3,
        };

        let wait = wait_for_loop_guard_decision(Uuid::nil(), incident, &event_tx, &mut control_rx);
        let respond = async {
            let event = event_rx.recv().await.expect("interaction event");
            let WorkerEvent::InteractionRequested {
                call_id,
                interaction,
                ..
            } = event
            else {
                panic!("expected structured interaction");
            };
            assert_eq!(call_id, "loop_guard");
            assert_eq!(interaction.mode, InteractionMode::SingleChoice);
            assert_eq!(interaction.choices.len(), 3);
            control_tx
                .send(TurnControl::Interaction {
                    selected: vec!["replan".into()],
                    text: None,
                })
                .await
                .unwrap();
        };
        let (decision, ()) = tokio::join!(wait, respond);
        assert_eq!(decision, Some(ServiceLoopDecision::Replan));
    }

    enum FakeTurn {
        Events(Vec<Result<ProviderEvent, ProviderError>>),
        Pending,
    }

    struct FakeBackend {
        turns: Mutex<VecDeque<FakeTurn>>,
        held: Mutex<Vec<mpsc::UnboundedSender<ChatStreamItem>>>,
        requests: Mutex<Vec<crate::request::PreparedModelRequest>>,
    }

    impl FakeBackend {
        fn new(turns: Vec<FakeTurn>) -> Arc<Self> {
            Arc::new(Self {
                turns: Mutex::new(turns.into()),
                held: Mutex::new(Vec::new()),
                requests: Mutex::new(Vec::new()),
            })
        }

        fn prepared_kinds(&self) -> Vec<crate::request::RequestKind> {
            self.requests
                .lock()
                .unwrap()
                .iter()
                .map(|request| request.kind)
                .collect()
        }

        fn requests(&self) -> Vec<crate::request::PreparedModelRequest> {
            self.requests.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl ChatBackend for FakeBackend {
        async fn stream_chat(
            &self,
            _messages: &[ChatMessage],
            _tools: &[ToolDef],
        ) -> Result<mpsc::UnboundedReceiver<ChatStreamItem>, ProviderError> {
            unreachable!("service requests must use stream_prepared")
        }

        async fn stream_prepared(
            &self,
            request: &crate::request::PreparedModelRequest,
        ) -> Result<mpsc::UnboundedReceiver<ChatStreamItem>, ProviderError> {
            self.requests.lock().unwrap().push(request.clone());
            let turn = self
                .turns
                .lock()
                .unwrap()
                .pop_front()
                .expect("fake turn exists");
            let (sender, receiver) = mpsc::unbounded_channel();
            match turn {
                FakeTurn::Events(events) => {
                    for event in events {
                        sender.send(event).unwrap();
                    }
                }
                FakeTurn::Pending => self.held.lock().unwrap().push(sender),
            }
            Ok(receiver)
        }
    }

    struct FakeFactory {
        backend: Arc<FakeBackend>,
        root: PathBuf,
        redaction_secrets: Vec<String>,
        memory_recall: crate::memory::MemoryRecall,
        skills: Arc<Mutex<crate::skills::SkillRegistry>>,
    }

    #[async_trait]
    impl ServiceSessionFactory for FakeFactory {
        fn session_store(&self) -> Result<crate::session::SessionStore, String> {
            Ok(crate::session::SessionStore::new(
                self.root.join("saved-service"),
            ))
        }
        async fn create(&self, request: &StartSessionOp) -> Result<ServiceSession, String> {
            let mut workspace = crate::workspace::WorkspaceContext::discover(&self.root)
                .map_err(|error| error.to_string())?;
            if let Some(cwd) = request.cwd.as_deref() {
                workspace.set_cwd(cwd).map_err(|error| error.to_string())?;
            }
            let tool_policy = crate::tool_selection::ToolSelectionPolicy::from_cli(
                request.tools.clone(),
                request.include_tools.clone(),
                request.exclude_tools.clone(),
            )
            .map_err(|error| error.to_string())?;
            let cwd = workspace.cwd().to_string_lossy().into_owned();
            let session_id = Uuid::new_v4();
            let (skill_activation_ids, skill_view_ids) = {
                let registry = self
                    .skills
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner());
                (
                    registry.activation_selectors(),
                    registry.viewable_instance_ids(),
                )
            };
            Ok(ServiceSession {
                id: session_id,
                provider: request.provider.clone().unwrap_or_else(|| "fake".into()),
                model: request.model.clone().unwrap_or_else(|| "model".into()),
                cwd,
                messages: vec![ChatMessage {
                    role: "system".into(),
                    content: crate::image_content::ModelContent::text("safe system"),
                    tool_calls: None,
                    tool_call_id: None,
                    response_items: Vec::new(),
                }],
                backend: self.backend.clone(),
                run: ServiceRunConfig {
                    inbox: Arc::new(tokio::sync::Mutex::new(
                        crate::service_inbox::Runtime::default(),
                    )),
                    workspace_activity_scope: None,
                    outcome_sink: None,
                    provider: "fake".into(),
                    model: "model".into(),
                    workspace,
                    max_turns: 4,
                    command_timeout_secs: 2,
                    command_output_policy: crate::config::CommandOutputPolicy::Bounded,
                    show_thinking: request.show_thinking,
                    approval_rules: Vec::new(),
                    permission_rules: Vec::new(),
                    allow_network: request.allow_network,
                    chat_options: ChatRequestOptions::default(),
                    sandbox_mode: crate::sandbox::SandboxMode::PolicyOnly,
                    skills: self.skills.clone(),
                    task_skill_prompt: String::new(),
                    memory_recall: self.memory_recall.clone(),
                    mcp: None,
                    lsp: None,
                    email: None,
                    session_identity: session_id.to_string(),
                    discovery: crate::tool_discovery::ToolDiscoverySession::default(),
                    tool_policy,
                    tool_constraints: crate::tool_selection::ToolSelectionConstraints {
                        tools_supported: true,
                        images_supported: false,
                        skills_enabled: true,
                        skill_activation_ids: Some(skill_activation_ids),
                        skill_view_ids: Some(skill_view_ids),
                        mcp_enabled: false,
                        native_mechanistic_enabled: false,
                        mechanistic_delegation_enabled: false,
                        transparent_mechanistic_enabled: false,
                        remote_mechanistic_enabled: false,
                        automation_setup_enabled: false,
                        lsp_enabled: false,
                        memory_generation_enabled: false,
                        automation_proposals_enabled: false,
                        email_enabled: false,
                        allowed_names: None,
                    },
                    tool_profile: ToolProfile::Main,
                    context_window_tokens: Some(8_192),
                    job_owner: crate::jobs::JobOwner::new(None),
                    redaction_secrets: self.redaction_secrets.clone(),
                },
            })
        }
    }

    struct Harness {
        input: tokio::io::WriteHalf<tokio::io::DuplexStream>,
        output: tokio::io::Lines<BufReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>>,
        task: tokio::task::JoinHandle<Result<(), ServiceIoError>>,
    }

    impl Harness {
        async fn start(factory: Arc<FakeFactory>) -> Self {
            let (client, server) = tokio::io::duplex(1024 * 1024);
            let (client_read, client_write) = tokio::io::split(client);
            let (server_read, server_write) = tokio::io::split(server);
            let task = tokio::spawn(run_protocol(
                BufReader::new(server_read),
                server_write,
                factory,
            ));
            let mut harness = Self {
                input: client_write,
                output: BufReader::new(client_read).lines(),
                task,
            };
            let ready = harness.next().await;
            assert_eq!(event_type(&ready), "ready");
            harness
        }

        async fn send(&mut self, value: Value) {
            let mut encoded = serde_json::to_vec(&value).unwrap();
            encoded.push(b'\n');
            self.input.write_all(&encoded).await.unwrap();
            self.input.flush().await.unwrap();
        }

        async fn next(&mut self) -> Value {
            let line =
                tokio::time::timeout(std::time::Duration::from_secs(3), self.output.next_line())
                    .await
                    .expect("service event deadline")
                    .unwrap()
                    .expect("service event line");
            serde_json::from_str(&line).unwrap()
        }

        async fn until(&mut self, expected: &str) -> Vec<Value> {
            let mut values = Vec::new();
            loop {
                let value = self.next().await;
                let done = event_type(&value) == expected;
                values.push(value);
                if done {
                    return values;
                }
            }
        }

        async fn shutdown(mut self, id: &str) -> Vec<Value> {
            self.send(op(id, serde_json::json!({"type":"shutdown"})))
                .await;
            let events = self.until("goodbye").await;
            drop(self.input);
            assert!(self.task.await.unwrap().is_ok());
            events
        }
    }

    fn factory(root: &std::path::Path, backend: Arc<FakeBackend>) -> Arc<FakeFactory> {
        Arc::new(FakeFactory {
            backend,
            root: root.to_path_buf(),
            redaction_secrets: Vec::new(),
            memory_recall: crate::memory::MemoryRecall::default(),
            skills: Arc::new(Mutex::new(crate::skills::SkillRegistry::empty())),
        })
    }

    fn op(id: &str, op: Value) -> Value {
        serde_json::json!({"schema":SCHEMA,"id":id,"op":op})
    }

    fn event_type(value: &Value) -> &str {
        value["event"]["type"].as_str().unwrap()
    }

    fn runner_type(value: &Value) -> Option<&str> {
        (event_type(value) == "runner")
            .then(|| value["event"]["event"]["type"].as_str())
            .flatten()
    }

    fn text_turn(text: &str) -> FakeTurn {
        FakeTurn::Events(vec![
            Ok(ProviderEvent::Text(text.into())),
            Ok(ProviderEvent::Finish {
                reason: FinishReason::Stop,
                usage: Some(UsageInfo {
                    prompt_tokens: 2,
                    completion_tokens: 1,
                    total_tokens: 3,
                    cached_prompt_tokens: None,
                    cache_write_tokens: None,
                    reasoning_tokens: None,
                    response_metadata: None,
                    cache_usage_override: None,
                }),
            }),
            Ok(ProviderEvent::Done),
        ])
    }

    fn tool_turn(id: &str, name: &str, arguments: Value) -> FakeTurn {
        FakeTurn::Events(vec![
            Ok(ProviderEvent::ToolCallStart {
                index: 0,
                id: id.into(),
                name: name.into(),
            }),
            Ok(ProviderEvent::ToolCallArg {
                index: 0,
                arg: arguments.to_string(),
            }),
            Ok(ProviderEvent::Finish {
                reason: FinishReason::ToolCalls,
                usage: None,
            }),
            Ok(ProviderEvent::Done),
        ])
    }

    async fn start_session(harness: &mut Harness) {
        harness
            .send(op("op_start", serde_json::json!({"type":"start_session"})))
            .await;
        assert_eq!(event_type(&harness.next().await), "session_started");
    }

    #[tokio::test]
    async fn orc290_inbox_steering_waits_for_safe_boundary_and_exact_retries_do_not_repeat() {
        let root = tempfile::tempdir().unwrap();
        let backend = FakeBackend::new(vec![FakeTurn::Pending, text_turn("steering applied")]);
        let mut harness = Harness::start(factory(root.path(), backend.clone())).await;
        start_session(&mut harness).await;
        harness
            .send(op(
                "first",
                serde_json::json!({"type":"user_input","text":"initial"}),
            ))
            .await;
        harness.until("turn_started").await;
        harness.send(op("submit", serde_json::json!({"type":"submit_input","input_id":"steer1","text":"steering","delivery":"steer"}))).await;
        let admitted = harness.until("input_admitted").await;
        let original_receipt = admitted.last().unwrap()["event"]["receipt"].clone();
        assert_eq!(backend.requests().len(), 1);
        let sender = backend.held.lock().unwrap().pop().unwrap();
        sender
            .send(Ok(ProviderEvent::Text("first complete".into())))
            .unwrap();
        sender
            .send(Ok(ProviderEvent::Finish {
                reason: FinishReason::Stop,
                usage: None,
            }))
            .unwrap();
        sender.send(Ok(ProviderEvent::Done)).unwrap();
        drop(sender);
        harness.until("turn_completed").await;
        assert_eq!(backend.requests().len(), 2);
        assert!(backend.requests()[1]
            .messages
            .iter()
            .any(|m| m.content.as_str() == Some("steering")));
        harness
            .send(op(
                "retry",
                serde_json::json!({"type":"submit_input","input_id":"steer1","text":"steering"}),
            ))
            .await;
        let receipt = harness.until("input_admitted").await;
        assert_eq!(
            receipt.last().unwrap()["event"]["receipt"],
            original_receipt
        );
        harness
            .send(op("inspect", serde_json::json!({"type":"inspect_inbox"})))
            .await;
        let inspected = harness.until("inbox").await;
        assert_eq!(
            inspected.last().unwrap()["event"]["inputs"][0]["status"],
            "completed"
        );
        assert_eq!(backend.requests().len(), 2);
        harness.shutdown("shutdown").await;
    }

    #[tokio::test]
    async fn orc290_persistent_inbox_reopens_held_without_provider_traffic() {
        let root = tempfile::tempdir().unwrap();
        let backend = FakeBackend::new(vec![FakeTurn::Pending]);
        let factory = factory(root.path(), backend.clone());
        let mut harness = Harness::start(factory.clone()).await;
        harness
            .send(op(
                "start",
                serde_json::json!({"type":"start_persistent_session"}),
            ))
            .await;
        let started = harness.until("session_started").await;
        let id = started.last().unwrap()["event"]["session_id"]
            .as_str()
            .unwrap()
            .to_string();
        harness
            .send(op(
                "work",
                serde_json::json!({"type":"submit_input","input_id":"active","text":"first"}),
            ))
            .await;
        harness.until("turn_started").await;
        harness.send(op("queue", serde_json::json!({"type":"submit_input","input_id":"later","text":"second","delivery":"queue"}))).await;
        harness.until("input_admitted").await;
        harness.shutdown("shutdown").await;
        let count = backend.requests().len();
        let mut reopened = Harness::start(factory).await;
        reopened
            .send(op(
                "load",
                serde_json::json!({"type":"load_persistent_session","session_id":id}),
            ))
            .await;
        reopened.until("session_started").await;
        reopened
            .send(op("inspect", serde_json::json!({"type":"inspect_inbox"})))
            .await;
        let values = reopened.until("inbox").await;
        let inbox = &values.last().unwrap()["event"];
        assert_eq!(inbox["held"], true);
        assert_eq!(inbox["inputs"][0]["status"], "interrupted");
        assert_eq!(inbox["inputs"][1]["status"], "pending");
        assert_eq!(backend.requests().len(), count);
        reopened.shutdown("done").await;
    }

    #[tokio::test]
    async fn multiple_turns_use_prepared_service_requests_and_keep_history() {
        let root = tempfile::tempdir().unwrap();
        let provider_state = crate::agent::stream::ProviderResponseItem::ChatReasoning {
            source: crate::agent::stream::ProviderStateSourceV1::for_test(
                "deepseek",
                crate::agent::ProviderProtocol::ChatCompletions,
            ),
            content: "service continuation sentinel".into(),
        };
        let first_turn = FakeTurn::Events(vec![
            Ok(ProviderEvent::Text("first".into())),
            Ok(ProviderEvent::ResponseItem(provider_state.clone())),
            Ok(ProviderEvent::Finish {
                reason: FinishReason::Stop,
                usage: None,
            }),
            Ok(ProviderEvent::Done),
        ]);
        let backend = FakeBackend::new(vec![first_turn, text_turn("second")]);
        let mut harness = Harness::start(factory(root.path(), backend.clone())).await;
        start_session(&mut harness).await;

        harness
            .send(op(
                "op_one",
                serde_json::json!({"type":"user_input","text":"one"}),
            ))
            .await;
        let first = harness.until("turn_completed").await;
        assert_eq!(event_type(&first[0]), "turn_started");
        harness
            .send(op(
                "op_two",
                serde_json::json!({"type":"user_input","text":"two"}),
            ))
            .await;
        harness.until("turn_completed").await;
        harness
            .send(op("op_status", serde_json::json!({"type":"status"})))
            .await;
        let status = harness.next().await;
        assert_eq!(status["event"]["session"]["turn"]["state"], "idle");
        harness.shutdown("op_shutdown").await;

        assert_eq!(
            backend.prepared_kinds(),
            [
                crate::request::RequestKind::Service,
                crate::request::RequestKind::Service
            ]
        );
        let requests = backend.requests();
        assert!(requests[1].messages.iter().any(|message| {
            message.role == "assistant" && message.content.as_str() == Some("first")
        }));
        assert_eq!(
            requests[1]
                .messages
                .iter()
                .find(|message| message.role == "assistant")
                .unwrap()
                .response_items,
            vec![provider_state]
        );
        let names = requests[0]
            .tools
            .iter()
            .map(|tool| tool.function.name.as_str())
            .collect::<HashSet<_>>();
        assert!(names.contains("request_user_input"));
        assert!(!names.contains("edit_plan"));
        assert!(!names.contains("spawn_agent"));
    }

    #[tokio::test]
    async fn service_reselects_recalled_memory_for_each_user_request() {
        let root = tempfile::tempdir().unwrap();
        let provider = crate::memory::MemoryRecord::new(
            crate::memory::MemoryScope::Workspace,
            crate::memory::MemoryKind::ExternalFact,
            "Provider retry fixtures use a hermetic loopback transport.",
            crate::memory::MemoryProvenance::Manual,
        )
        .unwrap();
        let garden = crate::memory::MemoryRecord::new(
            crate::memory::MemoryScope::Workspace,
            crate::memory::MemoryKind::ExternalFact,
            "The garden irrigation controller uses a rain sensor.",
            crate::memory::MemoryProvenance::Manual,
        )
        .unwrap();
        let backend = FakeBackend::new(vec![text_turn("first"), text_turn("second")]);
        let factory = Arc::new(FakeFactory {
            backend: backend.clone(),
            root: root.path().to_path_buf(),
            redaction_secrets: Vec::new(),
            memory_recall: crate::memory::MemoryRecall::from_records(
                false,
                true,
                &[],
                &[provider, garden],
            ),
            skills: Arc::new(Mutex::new(crate::skills::SkillRegistry::empty())),
        });
        let mut harness = Harness::start(factory).await;
        start_session(&mut harness).await;

        harness
            .send(op(
                "op_provider",
                serde_json::json!({"type":"user_input","text":"debug the provider loopback retry"}),
            ))
            .await;
        harness.until("turn_completed").await;
        harness
            .send(op(
                "op_garden",
                serde_json::json!({"type":"user_input","text":"inspect the garden rain sensor"}),
            ))
            .await;
        harness.until("turn_completed").await;
        harness.shutdown("op_shutdown").await;

        let requests = backend.requests();
        for request in &requests {
            assert_eq!(
                request
                    .prompt_cache
                    .expect("service requests have a conversation cache scope")
                    .stable_prefix_messages,
                1
            );
            let stable = request.messages[0].content.as_str().unwrap();
            assert!(!stable.contains("hermetic loopback transport"));
            assert!(!stable.contains("garden irrigation"));
        }
        fn logical_turn(request: &crate::request::PreparedModelRequest) -> &str {
            request
                .messages
                .iter()
                .skip(1)
                .find(|message| message.role == "system")
                .and_then(|message| message.content.as_str())
                .expect("recalled memory is a logical-turn segment after the cache boundary")
        }
        let first_memory = logical_turn(&requests[0]);
        assert!(first_memory.contains("hermetic loopback transport"));
        assert!(!first_memory.contains("garden irrigation"));
        let second_memory = logical_turn(&requests[1]);
        assert!(second_memory.contains("garden irrigation"));
        assert!(!second_memory.contains("hermetic loopback transport"));
    }

    #[tokio::test]
    async fn exact_client_approval_blocks_mutation_and_then_continues() {
        let root = tempfile::tempdir().unwrap();
        let output = root.path().join("approved.txt");
        let backend = FakeBackend::new(vec![
            tool_turn(
                "call-write",
                "write_file",
                serde_json::json!({"path":"approved.txt","content":"ok"}),
            ),
            text_turn("done"),
        ]);
        let mut harness = Harness::start(factory(root.path(), backend)).await;
        start_session(&mut harness).await;
        harness
            .send(op(
                "op_input",
                serde_json::json!({"type":"user_input","text":"write it"}),
            ))
            .await;
        let events = harness.until("approval_requested").await;
        let approval = events.last().unwrap();
        let request_id = approval["event"]["request_id"].as_str().unwrap();
        assert!(!output.exists());

        harness
            .send(op(
                "op_wrong",
                serde_json::json!({"type":"approval_response","request_id":"apr_wrong","decision":"allow_once"}),
            ))
            .await;
        assert_eq!(event_type(&harness.next().await), "error");
        assert!(!output.exists());
        harness
            .send(op(
                "op_allow",
                serde_json::json!({"type":"approval_response","request_id":request_id,"decision":"allow_once"}),
            ))
            .await;
        assert_eq!(event_type(&harness.next().await), "turn_resumed");
        harness.until("turn_completed").await;
        assert_eq!(std::fs::read_to_string(output).unwrap(), "ok");
        harness.shutdown("op_shutdown").await;
    }

    #[tokio::test]
    async fn exact_skill_activation_is_automatic_and_updates_only_the_next_service_turn() {
        let root = tempfile::tempdir().unwrap();
        let skill_dir = root.path().join("skills/deploy");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: deploy\ndescription: Deploy carefully\n---\nSERVICE-SKILL-NEXT-TURN",
        )
        .unwrap();
        let skills = Arc::new(Mutex::new(crate::skills::SkillRegistry::discover(
            crate::skills::SkillRoots {
                project: Some(root.path().join("skills")),
                project_boundary: Some(root.path().to_path_buf()),
                user: None,
                compatibility: Vec::new(),
            },
            &[],
        )));
        let backend = FakeBackend::new(vec![
            tool_turn(
                "call-skill",
                "activate_skill",
                serde_json::json!({"id":"deploy"}),
            ),
            text_turn("done"),
        ]);
        let factory = Arc::new(FakeFactory {
            backend: backend.clone(),
            root: root.path().to_path_buf(),
            redaction_secrets: Vec::new(),
            memory_recall: crate::memory::MemoryRecall::default(),
            skills: skills.clone(),
        });
        let mut harness = Harness::start(factory).await;
        start_session(&mut harness).await;
        harness
            .send(op(
                "op_input",
                serde_json::json!({"type":"user_input","text":"deploy carefully"}),
            ))
            .await;
        let terminal = loop {
            let value = harness.next().await;
            if matches!(event_type(&value), "approval_requested" | "turn_completed") {
                break value;
            }
        };
        assert_eq!(event_type(&terminal), "turn_completed");
        harness.shutdown("op_shutdown").await;

        let requests = backend.requests();
        assert_eq!(requests.len(), 2);
        assert!(!requests[0].messages[0]
            .content
            .as_str()
            .unwrap()
            .contains("SERVICE-SKILL-NEXT-TURN"));
        assert!(requests[1].messages[0]
            .content
            .as_str()
            .unwrap()
            .contains("SERVICE-SKILL-NEXT-TURN"));
        assert!(skills.lock().unwrap().is_active("deploy"));
    }

    #[tokio::test]
    async fn structured_interaction_rejects_bad_answer_without_resuming() {
        let root = tempfile::tempdir().unwrap();
        let backend = FakeBackend::new(vec![
            tool_turn(
                "call-input",
                "request_user_input",
                serde_json::json!({"question":"Pick one","kind":"single_choice","choices":[{"label":"Alpha","value":"a"},{"label":"Beta","value":"b"}]}),
            ),
            text_turn("accepted"),
        ]);
        let mut harness = Harness::start(factory(root.path(), backend.clone())).await;
        start_session(&mut harness).await;
        harness
            .send(op(
                "op_input",
                serde_json::json!({"type":"user_input","text":"ask me"}),
            ))
            .await;
        let events = harness.until("interaction_requested").await;
        let interaction = events.last().unwrap();
        let request_id = interaction["event"]["request_id"].as_str().unwrap();
        harness
            .send(op(
                "op_bad",
                serde_json::json!({"type":"interaction_response","request_id":request_id,"selected":["unknown"]}),
            ))
            .await;
        assert_eq!(event_type(&harness.next().await), "error");
        harness
            .send(op(
                "op_answer",
                serde_json::json!({"type":"interaction_response","request_id":request_id,"selected":["b"]}),
            ))
            .await;
        assert_eq!(event_type(&harness.next().await), "turn_resumed");
        harness.until("turn_completed").await;
        let requests = backend.requests();
        let result = requests[1]
            .messages
            .iter()
            .find(|message| message.tool_call_id.as_deref() == Some("call-input"))
            .and_then(|message| message.content.as_str())
            .unwrap();
        assert!(result.contains("\"b\""));
        harness.shutdown("op_shutdown").await;
    }

    #[tokio::test]
    async fn client_denial_becomes_one_tool_result_and_does_not_mutate() {
        let root = tempfile::tempdir().unwrap();
        let output = root.path().join("denied.txt");
        let backend = FakeBackend::new(vec![
            tool_turn(
                "call-denied",
                "write_file",
                serde_json::json!({"path":"denied.txt","content":"no"}),
            ),
            text_turn("continued"),
        ]);
        let mut harness = Harness::start(factory(root.path(), backend.clone())).await;
        start_session(&mut harness).await;
        harness
            .send(op(
                "op_input",
                serde_json::json!({"type":"user_input","text":"try"}),
            ))
            .await;
        let approval = harness.until("approval_requested").await;
        let request_id = approval.last().unwrap()["event"]["request_id"]
            .as_str()
            .unwrap();
        harness
            .send(op("op_status", serde_json::json!({"type":"status"})))
            .await;
        let status = harness.next().await;
        assert_eq!(
            status["event"]["session"]["turn"]["waiting_for"],
            "approval"
        );
        harness
            .send(op(
                "op_busy_input",
                serde_json::json!({"type":"user_input","text":"must not queue"}),
            ))
            .await;
        assert_eq!(event_type(&harness.next().await), "error");
        harness
            .send(op(
                "op_busy_start",
                serde_json::json!({"type":"start_session"}),
            ))
            .await;
        assert_eq!(event_type(&harness.next().await), "error");
        harness
            .send(op(
                "op_deny",
                serde_json::json!({"type":"approval_response","request_id":request_id,"decision":"deny"}),
            ))
            .await;
        harness.until("turn_completed").await;
        assert!(!output.exists());
        let requests = backend.requests();
        let denied = requests[1]
            .messages
            .iter()
            .find(|message| message.tool_call_id.as_deref() == Some("call-denied"))
            .and_then(|message| message.content.as_str())
            .unwrap();
        assert_eq!(denied, "Denied by client");
        harness.shutdown("op_shutdown").await;
    }

    #[tokio::test]
    async fn service_shutdown_kills_owned_background_process_groups() {
        let root = tempfile::tempdir().unwrap();
        let marker = root.path().join("late-marker.txt");
        let backend = FakeBackend::new(vec![
            tool_turn(
                "call-job",
                "run_command",
                serde_json::json!({
                    "command":"sleep 1; printf late > late-marker.txt",
                    "background":true
                }),
            ),
            text_turn("job started"),
        ]);
        let mut harness = Harness::start(factory(root.path(), backend)).await;
        start_session(&mut harness).await;
        harness
            .send(op(
                "op_input",
                serde_json::json!({"type":"user_input","text":"start it"}),
            ))
            .await;
        let approval = harness.until("approval_requested").await;
        let request_id = approval.last().unwrap()["event"]["request_id"]
            .as_str()
            .unwrap();
        harness
            .send(op(
                "op_allow",
                serde_json::json!({"type":"approval_response","request_id":request_id,"decision":"allow_once"}),
            ))
            .await;
        harness.until("turn_completed").await;
        harness.shutdown("op_shutdown").await;
        tokio::time::sleep(std::time::Duration::from_millis(1_200)).await;
        assert!(!marker.exists());
    }

    #[tokio::test]
    async fn start_session_cwd_cannot_escape_startup_workspace() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let backend = FakeBackend::new(Vec::new());
        let mut harness = Harness::start(factory(root.path(), backend.clone())).await;
        harness
            .send(op(
                "op_escape",
                serde_json::json!({
                    "type":"start_session",
                    "cwd":outside.path().to_string_lossy()
                }),
            ))
            .await;
        let error = harness.next().await;
        assert_eq!(event_type(&error), "error");
        assert_eq!(error["event"]["kind"], "setup");
        assert!(backend.requests().is_empty());
        harness.shutdown("op_shutdown").await;
    }

    #[tokio::test]
    async fn client_abort_ends_the_turn_without_provider_continuation() {
        let root = tempfile::tempdir().unwrap();
        let backend = FakeBackend::new(vec![tool_turn(
            "call-abort",
            "write_file",
            serde_json::json!({"path":"never.txt","content":"no"}),
        )]);
        let mut harness = Harness::start(factory(root.path(), backend.clone())).await;
        start_session(&mut harness).await;
        harness
            .send(op(
                "op_input",
                serde_json::json!({"type":"user_input","text":"try"}),
            ))
            .await;
        let approval = harness.until("approval_requested").await;
        let request_id = approval.last().unwrap()["event"]["request_id"]
            .as_str()
            .unwrap();
        harness
            .send(op(
                "op_abort",
                serde_json::json!({"type":"approval_response","request_id":request_id,"decision":"abort"}),
            ))
            .await;
        harness.until("turn_cancelled").await;
        assert_eq!(backend.requests().len(), 1);
        assert!(!root.path().join("never.txt").exists());
        harness.shutdown("op_shutdown").await;
    }

    #[tokio::test]
    async fn unsupported_or_unknown_selected_tools_fail_before_provider_traffic() {
        for tool in ["edit_plan", "does_not_exist"] {
            let root = tempfile::tempdir().unwrap();
            let backend = FakeBackend::new(Vec::new());
            let mut harness = Harness::start(factory(root.path(), backend.clone())).await;
            harness
                .send(op(
                    "op_start",
                    serde_json::json!({"type":"start_session","tools":[tool]}),
                ))
                .await;
            assert_eq!(event_type(&harness.next().await), "session_started");
            harness
                .send(op(
                    "op_input",
                    serde_json::json!({"type":"user_input","text":"go"}),
                ))
                .await;
            loop {
                let event = harness.next().await;
                if runner_type(&event) == Some("failure") {
                    break;
                }
            }
            harness.until("turn_failed").await;
            assert!(backend.requests().is_empty());
            harness.shutdown("op_shutdown").await;
        }
    }

    #[tokio::test]
    async fn provider_failure_is_typed_safe_and_returns_session_to_idle() {
        let root = tempfile::tempdir().unwrap();
        let backend = FakeBackend::new(vec![FakeTurn::Events(vec![Err(ProviderError::Http {
            status: 401,
            body_excerpt: None,
        })])]);
        let mut harness = Harness::start(factory(root.path(), backend)).await;
        start_session(&mut harness).await;
        harness
            .send(op(
                "op_input",
                serde_json::json!({"type":"user_input","text":"go"}),
            ))
            .await;
        loop {
            let event = harness.next().await;
            if runner_type(&event) == Some("failure") {
                assert_eq!(event["event"]["event"]["kind"], "provider_transport");
                assert_eq!(event["event"]["event"]["message"], "provider HTTP 401");
                break;
            }
        }
        harness.until("turn_failed").await;
        harness
            .send(op("op_status", serde_json::json!({"type":"status"})))
            .await;
        let status = loop {
            let event = harness.next().await;
            if event_type(&event) == "status" {
                break event;
            }
        };
        assert_eq!(status["event"]["session"]["turn"]["state"], "idle");
        harness.shutdown("op_shutdown").await;
    }

    #[tokio::test]
    async fn cancellation_returns_to_idle_and_discards_stale_worker_events() {
        let root = tempfile::tempdir().unwrap();
        let backend = FakeBackend::new(vec![FakeTurn::Pending]);
        let mut harness = Harness::start(factory(root.path(), backend)).await;
        start_session(&mut harness).await;
        harness
            .send(op(
                "op_input",
                serde_json::json!({"type":"user_input","text":"wait"}),
            ))
            .await;
        harness.until("runner").await;
        harness
            .send(op("op_cancel", serde_json::json!({"type":"cancel"})))
            .await;
        harness.until("turn_cancelled").await;
        harness
            .send(op("op_status", serde_json::json!({"type":"status"})))
            .await;
        assert_eq!(
            harness.next().await["event"]["session"]["turn"]["state"],
            "idle"
        );
        harness.shutdown("op_shutdown").await;
    }

    #[tokio::test]
    async fn malformed_duplicate_and_oversized_operations_are_recoverable() {
        let root = tempfile::tempdir().unwrap();
        let backend = FakeBackend::new(Vec::new());
        let mut harness = Harness::start(factory(root.path(), backend)).await;
        harness.input.write_all(b"not json\n").await.unwrap();
        assert_eq!(event_type(&harness.next().await), "error");
        let status = op("same", serde_json::json!({"type":"status"}));
        harness.send(status.clone()).await;
        assert_eq!(event_type(&harness.next().await), "status");
        harness.send(status).await;
        assert_eq!(harness.next().await["event"]["kind"], "duplicate_operation");
        let mut oversized = vec![b'x'; MAX_LINE_BYTES + 1];
        oversized.push(b'\n');
        harness.input.write_all(&oversized).await.unwrap();
        assert_eq!(event_type(&harness.next().await), "error");
        harness.shutdown("end").await;
    }

    #[tokio::test]
    async fn known_runtime_secrets_never_enter_tool_or_debug_output() {
        let root = tempfile::tempdir().unwrap();
        let backend = FakeBackend::new(vec![tool_turn(
            "call-secret",
            "run_command",
            serde_json::json!({"command":"printf sentinel-runtime-key"}),
        )]);
        let factory = Arc::new(FakeFactory {
            backend,
            root: root.path().to_path_buf(),
            redaction_secrets: vec!["sentinel-runtime-key".into()],
            memory_recall: crate::memory::MemoryRecall::default(),
            skills: Arc::new(Mutex::new(crate::skills::SkillRegistry::empty())),
        });
        let mut harness = Harness::start(factory).await;
        start_session(&mut harness).await;
        harness
            .send(op(
                "op_input",
                serde_json::json!({"type":"user_input","text":"run"}),
            ))
            .await;
        loop {
            let value = harness.next().await;
            let encoded = serde_json::to_string(&value).unwrap();
            assert!(!encoded.contains("sentinel-runtime-key"));
            if event_type(&value) == "approval_requested" {
                break;
            }
        }
        harness
            .send(op("op_cancel", serde_json::json!({"type":"cancel"})))
            .await;
        harness.until("turn_cancelled").await;
        harness.shutdown("op_shutdown").await;
    }

    #[tokio::test]
    async fn eof_gracefully_emits_goodbye() {
        let root = tempfile::tempdir().unwrap();
        let backend = FakeBackend::new(Vec::new());
        let mut harness = Harness::start(factory(root.path(), backend)).await;
        harness.input.shutdown().await.unwrap();
        assert_eq!(event_type(&harness.next().await), "goodbye");
        assert!(harness.task.await.unwrap().is_ok());
    }

    #[test]
    fn service_debug_redacts_runtime_values_and_url_arguments_are_sanitized() {
        let root = tempfile::tempdir().unwrap();
        let workspace = crate::workspace::WorkspaceContext::discover(root.path()).unwrap();
        let run = ServiceRunConfig {
            inbox: Arc::new(tokio::sync::Mutex::new(
                crate::service_inbox::Runtime::default(),
            )),
            workspace_activity_scope: None,
            outcome_sink: None,
            provider: "safe".into(),
            model: "model".into(),
            workspace,
            max_turns: 1,
            command_timeout_secs: 1,
            command_output_policy: crate::config::CommandOutputPolicy::Bounded,
            show_thinking: false,
            approval_rules: Vec::new(),
            permission_rules: Vec::new(),
            allow_network: false,
            chat_options: ChatRequestOptions::default(),
            sandbox_mode: crate::sandbox::SandboxMode::PolicyOnly,
            skills: Arc::new(Mutex::new(crate::skills::SkillRegistry::empty())),
            task_skill_prompt: String::new(),
            memory_recall: crate::memory::MemoryRecall::default(),
            mcp: None,
            lsp: None,
            email: None,
            session_identity: "debug-session".into(),
            discovery: crate::tool_discovery::ToolDiscoverySession::default(),
            tool_policy: crate::tool_selection::ToolSelectionPolicy::default(),
            tool_constraints: crate::tool_selection::ToolSelectionConstraints::default(),
            tool_profile: ToolProfile::Main,
            context_window_tokens: Some(8_192),
            job_owner: crate::jobs::JobOwner::new(None),
            redaction_secrets: vec!["never-debug-this".into()],
        };
        assert!(!format!("{run:?}").contains("never-debug-this"));
        let value = safe_tool_arguments(
            &serde_json::json!({
                "api_key":"never-debug-this",
                "url":"https://user:pass@example.invalid/a?token=secret#fragment"
            }),
            &run.redaction_refs(),
        );
        let encoded = value.to_string();
        assert!(!encoded.contains("never-debug-this"));
        assert!(!encoded.contains("user"));
        assert!(!encoded.contains("pass"));
        assert!(!encoded.contains("token"));
        assert!(!encoded.contains("fragment"));
    }

    #[tokio::test]
    async fn service_and_acp_active_turn_hold_workspace_exclusion_until_cancellation() {
        let root = tempfile::tempdir().unwrap();
        let profile = tempfile::tempdir().unwrap();
        let backend = FakeBackend::new(vec![FakeTurn::Pending]);
        let session =
            LocalAgentSession::for_test(Uuid::new_v4(), root.path(), backend.clone()).unwrap();
        let inner = session.inner.as_ref().unwrap();
        let mut run = inner.run.clone();
        run.workspace_activity_scope = Some(profile.path().to_path_buf());
        let (events, mut received) = mpsc::channel(16);
        let (_controls, control_rx) = mpsc::channel(16);
        let worker = tokio::spawn(run_turn(
            backend,
            inner.messages.clone(),
            run,
            Uuid::new_v4(),
            events,
            control_rx,
        ));
        tokio::time::timeout(std::time::Duration::from_secs(1), received.recv())
            .await
            .unwrap()
            .unwrap();
        let recovery = crate::recovery::RecoveryStore::new(profile.path());
        assert_eq!(
            recovery
                .try_workspace_restore(&[root.path().to_path_buf()])
                .err()
                .unwrap()
                .kind(),
            std::io::ErrorKind::WouldBlock
        );
        worker.abort();
        let _ = worker.await;
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            loop {
                if recovery
                    .try_workspace_restore(&[root.path().to_path_buf()])
                    .is_ok()
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn bounded_line_reader_drains_the_complete_oversized_record() {
        let mut input = vec![b'x'; MAX_LINE_BYTES + 20];
        input.extend_from_slice(b"\n{}\n");
        let mut reader = BufReader::new(std::io::Cursor::new(input));
        assert!(matches!(
            read_bounded_line(&mut reader).await.unwrap(),
            BoundedLine::TooLong
        ));
        let BoundedLine::Line(next) = read_bounded_line(&mut reader).await.unwrap() else {
            panic!("second record remains framed");
        };
        assert_eq!(next, b"{}");
        let mut rest = Vec::new();
        reader.read_to_end(&mut rest).await.unwrap();
        assert!(rest.is_empty());
    }

    #[test]
    fn loopback_addresses_and_attach_urls_are_closed() {
        assert_eq!(
            parse_loopback_listen("127.0.0.1:0").unwrap(),
            "127.0.0.1:0".parse().unwrap()
        );
        assert!(parse_loopback_listen("[::1]:9777").is_ok());
        for address in ["0.0.0.0:9777", "192.168.1.4:9777", "localhost:9777"] {
            assert!(parse_loopback_listen(address).is_err(), "{address}");
        }

        assert_eq!(
            validate_attach_endpoint("ws://127.0.0.1:9777/service").unwrap(),
            "ws://127.0.0.1:9777/service"
        );
        assert!(validate_attach_endpoint("ws://[::1]:9777/").is_ok());
        for endpoint in [
            "ws://example.com:9777/",
            "ws://0.0.0.0:9777/",
            "wss://127.0.0.1:9777/",
            "ws://user@127.0.0.1:9777/",
            "ws://127.0.0.1:9777/?token=no",
            "ws://127.0.0.1:9777/#fragment",
        ] {
            assert!(validate_attach_endpoint(endpoint).is_err(), "{endpoint}");
        }
    }

    #[test]
    fn token_sources_are_bounded_redacted_and_require_safe_names() {
        let source = auth_token_source(None, Some("CA_ATTACH_TOKEN_1".into())).unwrap();
        assert_eq!(
            format!("{source:?}"),
            "AuthTokenSource::Environment(\"CA_ATTACH_TOKEN_1\")"
        );
        assert!(auth_token_source(None, Some("lowercase".into())).is_err());
        assert!(auth_token_source(None, None).is_err());
        assert!(auth_token_source(Some(PathBuf::from("a")), Some("TOKEN".into())).is_err());
        assert!(normalize_token(vec![b'x'; TOKEN_MIN_BYTES - 1]).is_err());
        assert!(normalize_token(vec![b'x'; TOKEN_MAX_BYTES + 1]).is_err());
        assert!(normalize_token(vec![b' '; TOKEN_MIN_BYTES]).is_err());
        let token =
            normalize_token(format!("{}\n", "x".repeat(TOKEN_MIN_BYTES)).into_bytes()).unwrap();
        assert_eq!(format!("{token:?}"), "SecretToken(<redacted>)");
        assert!(constant_time_eq(&token.digest(), &token.digest()));
        assert!(!constant_time_eq(
            &token.digest(),
            &normalize_token(vec![b'y'; TOKEN_MIN_BYTES])
                .unwrap()
                .digest()
        ));

        let name = format!("CA_ATTACH_TEST_{}", std::process::id());
        // SAFETY: the test uses a process-unique name and removes it before returning.
        unsafe {
            std::env::set_var(&name, "z".repeat(TOKEN_MIN_BYTES));
        }
        let loaded = load_auth_token(&AuthTokenSource::Environment(name.clone())).unwrap();
        // SAFETY: no other test uses this process-unique variable name.
        unsafe {
            std::env::remove_var(name);
        }
        assert_eq!(loaded.0.len(), TOKEN_MIN_BYTES);
    }

    #[cfg(unix)]
    #[test]
    fn protected_token_files_reject_permissions_and_symlinks() {
        use std::os::unix::fs::{symlink, PermissionsExt};

        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("token");
        std::fs::write(&path, "a".repeat(TOKEN_MIN_BYTES)).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let source = AuthTokenSource::File(path.clone());
        assert!(load_auth_token(&source).is_ok());
        assert!(!format!("{source:?}").contains(path.to_string_lossy().as_ref()));

        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(load_auth_token(&source).is_err());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let link = root.path().join("link");
        symlink(&path, &link).unwrap();
        assert!(load_auth_token(&AuthTokenSource::File(link)).is_err());
    }

    #[tokio::test]
    async fn websocket_transport_rejects_binary_and_oversized_frames_and_cleans_up() {
        use tokio_tungstenite::tungstenite::client::IntoClientRequest;

        for message in [
            WebSocketMessage::Binary(vec![1, 2, 3].into()),
            WebSocketMessage::Text("x".repeat(MAX_LINE_BYTES + 1).into()),
        ] {
            let root = tempfile::tempdir().unwrap();
            let backend = FakeBackend::new(Vec::new());
            let factory = factory(root.path(), backend);
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let token = normalize_token(vec![b'q'; TOKEN_MIN_BYTES]).unwrap();
            let digest = token.digest();
            let (shutdown_tx, shutdown_rx) = watch::channel(false);
            let server = tokio::spawn(async move {
                let (stream, _) = listener.accept().await.unwrap();
                let socket = accept_authenticated_websocket(stream, digest, false)
                    .await
                    .unwrap();
                bridge_websocket(socket, factory, shutdown_rx).await;
            });

            let endpoint = format!("ws://{address}/");
            let mut request = endpoint.as_str().into_client_request().unwrap();
            request.headers_mut().insert(
                http::header::AUTHORIZATION,
                token.authorization_value().unwrap(),
            );
            let (mut client, _) = connect_async_with_config(request, None, false)
                .await
                .unwrap();
            let ready = client.next().await.unwrap().unwrap();
            let ready = ready.into_text().unwrap();
            assert_eq!(
                serde_json::from_str::<Value>(&ready).unwrap()["event"]["transport"],
                "websocket"
            );
            client.send(message).await.unwrap();
            let _ = tokio::time::timeout(std::time::Duration::from_secs(2), client.next()).await;
            drop(client);
            let _ = shutdown_tx.send(true);
            tokio::time::timeout(std::time::Duration::from_secs(2), server)
                .await
                .expect("connection task must stop")
                .unwrap();
        }
    }
}
