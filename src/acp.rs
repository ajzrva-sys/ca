use crate::headless::{HeadlessEventV1, HeadlessFailureKind, ToolResultStatus};
use crate::runtime::ActiveProfile;
use crate::service::{
    LocalAgentCancellation, LocalAgentEvent, LocalAgentFactory, LocalAgentMode, LocalAgentSession,
};
use agent_client_protocol::schema::v1::{
    AgentCapabilities, AvailableCommand, AvailableCommandsUpdate, CancelNotification, ContentBlock,
    ContentChunk, CurrentModeUpdate, EmbeddedResourceResource, Implementation, InitializeRequest,
    InitializeResponse, LoadSessionRequest, LoadSessionResponse, NewSessionRequest,
    NewSessionResponse, PermissionOption, PermissionOptionKind, Plan, PlanEntry, PlanEntryPriority,
    PlanEntryStatus, PromptCapabilities, PromptRequest, PromptResponse, RequestPermissionOutcome,
    RequestPermissionRequest, SessionAdditionalDirectoriesCapabilities, SessionCapabilities,
    SessionId, SessionMode, SessionModeState, SessionNotification, SessionUpdate,
    SetSessionModeRequest, SetSessionModeResponse, StopReason, TextContent, ToolCall,
    ToolCallLocation, ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields, ToolKind, UsageUpdate,
};
use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::{Agent, Client, ConnectTo, ConnectionTo, Error, Stdio};
use async_trait::async_trait;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

const MAX_PROMPT_BYTES: usize = 48 * 1024;
const MAX_RESOURCE_COUNT: usize = 32;
const MAX_RESOURCE_BYTES: usize = 64 * 1024;
const MAX_RESOURCE_AGGREGATE_BYTES: usize = 128 * 1024;
const MAX_RESOURCE_URI_BYTES: usize = 2 * 1024;

#[cfg(not(test))]
const PERMISSION_RESPONSE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
#[cfg(test)]
const PERMISSION_RESPONSE_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(100);

#[derive(Debug, Clone)]
pub struct AcpCliArgs {
    pub active_profile: ActiveProfile,
    pub allow_network: bool,
}

#[derive(Clone)]
struct AcpState {
    factory: Arc<dyn AcpSessionFactory>,
    initialized: Arc<AtomicBool>,
    sessions: Arc<Mutex<HashMap<String, Arc<Mutex<LocalAgentSession>>>>>,
    active: Arc<Mutex<HashMap<String, LocalAgentCancellation>>>,
}

#[async_trait]
trait AcpSessionFactory: Send + Sync {
    async fn new_session(
        &self,
        cwd: &Path,
        additional_directories: &[PathBuf],
    ) -> Result<LocalAgentSession, String>;

    async fn load_session(
        &self,
        id: Uuid,
        cwd: &Path,
        additional_directories: &[PathBuf],
    ) -> Result<LocalAgentSession, String>;
}

#[async_trait]
impl AcpSessionFactory for LocalAgentFactory {
    async fn new_session(
        &self,
        cwd: &Path,
        additional_directories: &[PathBuf],
    ) -> Result<LocalAgentSession, String> {
        LocalAgentFactory::new_session(self, cwd, additional_directories).await
    }

    async fn load_session(
        &self,
        id: Uuid,
        cwd: &Path,
        additional_directories: &[PathBuf],
    ) -> Result<LocalAgentSession, String> {
        LocalAgentFactory::load_session(self, id, cwd, additional_directories).await
    }
}

impl AcpState {
    fn new(args: &AcpCliArgs) -> Self {
        Self {
            factory: Arc::new(LocalAgentFactory::new(
                args.active_profile.clone(),
                args.allow_network,
            )),
            initialized: Arc::new(AtomicBool::new(false)),
            sessions: Arc::new(Mutex::new(HashMap::new())),
            active: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    #[cfg(test)]
    fn with_factory(factory: Arc<dyn AcpSessionFactory>) -> Self {
        Self {
            factory,
            initialized: Arc::new(AtomicBool::new(false)),
            sessions: Arc::new(Mutex::new(HashMap::new())),
            active: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn require_initialized(&self) -> Result<(), Error> {
        if self.initialized.load(Ordering::Acquire) {
            Ok(())
        } else {
            Err(Error::invalid_request().data("initialize must complete first"))
        }
    }

    async fn session(&self, id: &SessionId) -> Result<Arc<Mutex<LocalAgentSession>>, Error> {
        self.require_initialized()?;
        self.sessions
            .lock()
            .await
            .get(&id.to_string())
            .cloned()
            .ok_or_else(|| Error::resource_not_found(Some(id.to_string())))
    }

    async fn insert_session(&self, session: LocalAgentSession) -> Result<SessionId, Error> {
        let id = session.id().to_string();
        let mut sessions = self.sessions.lock().await;
        if sessions.contains_key(&id) {
            return Err(Error::invalid_request().data("session is already loaded"));
        }
        sessions.insert(id.clone(), Arc::new(Mutex::new(session)));
        Ok(SessionId::new(id))
    }

    async fn set_active(
        &self,
        id: &SessionId,
        cancellation: LocalAgentCancellation,
    ) -> Result<(), Error> {
        let mut active = self.active.lock().await;
        if active.contains_key(&id.to_string()) {
            return Err(
                Error::invalid_request().data("a prompt is already active for this session")
            );
        }
        active.insert(id.to_string(), cancellation);
        Ok(())
    }

    async fn clear_active(&self, id: &SessionId) {
        self.active.lock().await.remove(&id.to_string());
    }

    async fn shutdown(&self) {
        let cancellations = self
            .active
            .lock()
            .await
            .drain()
            .map(|(_, cancellation)| cancellation)
            .collect::<Vec<_>>();
        for cancellation in cancellations {
            cancellation.cancel();
        }
        let sessions = self
            .sessions
            .lock()
            .await
            .drain()
            .map(|(_, session)| session)
            .collect::<Vec<_>>();
        for session in sessions {
            let mut session = session.lock().await;
            session.close().await;
        }
    }
}

fn mode_state(mode: LocalAgentMode) -> SessionModeState {
    SessionModeState::new(mode_id(mode), available_modes())
}

fn available_modes() -> Vec<SessionMode> {
    vec![
        SessionMode::new("agent", "Agent")
            .description("Use CA's normal tools under its local policy and approvals"),
        SessionMode::new("plan", "Plan")
            .description("Research and produce a plan without normal editing tools"),
        SessionMode::new("ask", "Ask").description("Answer without invoking tools"),
    ]
}

fn mode_id(mode: LocalAgentMode) -> &'static str {
    match mode {
        LocalAgentMode::Agent => "agent",
        LocalAgentMode::Plan => "plan",
        LocalAgentMode::Ask => "ask",
    }
}

fn parse_mode(value: &str) -> Result<LocalAgentMode, Error> {
    match value {
        "agent" => Ok(LocalAgentMode::Agent),
        "plan" => Ok(LocalAgentMode::Plan),
        "ask" => Ok(LocalAgentMode::Ask),
        _ => Err(Error::invalid_params().data("mode must be agent, plan, or ask")),
    }
}

fn initialize_response(request: &InitializeRequest) -> Result<InitializeResponse, Error> {
    if request.protocol_version != ProtocolVersion::V1 {
        return Err(Error::invalid_request().data("CA ACP supports stable protocol version 1 only"));
    }
    let session_capabilities = SessionCapabilities::new()
        .additional_directories(SessionAdditionalDirectoriesCapabilities::new());
    Ok(InitializeResponse::new(ProtocolVersion::V1)
        .agent_capabilities(
            AgentCapabilities::new()
                .load_session(true)
                .prompt_capabilities(PromptCapabilities::new().embedded_context(true))
                .session_capabilities(session_capabilities),
        )
        .agent_info(
            Implementation::new("ca", env!("CARGO_PKG_VERSION")).title("CA terminal coding agent"),
        ))
}

fn validate_session_request(cwd: &Path, mcp_count: usize) -> Result<(), Error> {
    if !cwd.is_absolute() {
        return Err(Error::invalid_params().data("cwd must be an absolute path"));
    }
    if mcp_count != 0 {
        return Err(Error::invalid_params().data(
            "client-supplied MCP servers are not supported; CA uses its configured local MCP registry",
        ));
    }
    Ok(())
}

fn prompt_text(prompt: &[ContentBlock], roots: &[PathBuf]) -> Result<String, Error> {
    let mut text = String::new();
    let mut text_bytes = 0_usize;
    let mut resource_count = 0_usize;
    let mut resource_bytes = 0_usize;
    for block in prompt {
        let fragment = match block {
            ContentBlock::Text(content) => {
                text_bytes = text_bytes.saturating_add(content.text.len());
                if text_bytes > MAX_PROMPT_BYTES {
                    return Err(
                        Error::invalid_params().data("prompt text exceeds the 48 KiB ACP bound")
                    );
                }
                content.text.clone()
            }
            ContentBlock::ResourceLink(resource) => {
                resource_count = resource_count.saturating_add(1);
                let (provenance, content) = read_linked_resource(resource, roots)?;
                resource_bytes = resource_bytes.saturating_add(content.len());
                bounded_resource_fragment(&provenance, &content, resource_count, resource_bytes)?
            }
            ContentBlock::Resource(resource) => {
                resource_count = resource_count.saturating_add(1);
                let EmbeddedResourceResource::TextResourceContents(content) = &resource.resource
                else {
                    return Err(Error::invalid_params()
                        .data("binary ACP embedded resources are not supported"));
                };
                validate_text_media_type(content.mime_type.as_deref())?;
                validate_resource_uri(&content.uri)?;
                resource_bytes = resource_bytes.saturating_add(content.text.len());
                bounded_resource_fragment(
                    &format!("embedded {}", content.uri),
                    &content.text,
                    resource_count,
                    resource_bytes,
                )?
            }
            ContentBlock::Image(_) | ContentBlock::Audio(_) => {
                return Err(Error::invalid_params()
                    .data("this CA ACP route supports text resources, not image or audio blocks"));
            }
            _ => {
                return Err(Error::invalid_params().data("unsupported ACP prompt content block"));
            }
        };
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&fragment);
    }
    if text.trim().is_empty() || text.contains('\0') {
        return Err(Error::invalid_params().data("prompt must contain safe non-empty text"));
    }
    Ok(text)
}

fn bounded_resource_fragment(
    provenance: &str,
    content: &str,
    count: usize,
    aggregate_bytes: usize,
) -> Result<String, Error> {
    if count > MAX_RESOURCE_COUNT {
        return Err(Error::invalid_params().data("ACP prompt exceeds the 32-resource bound"));
    }
    if content.len() > MAX_RESOURCE_BYTES {
        return Err(Error::invalid_params().data("ACP resource exceeds the 64 KiB bound"));
    }
    if aggregate_bytes > MAX_RESOURCE_AGGREGATE_BYTES {
        return Err(
            Error::invalid_params().data("ACP resources exceed the 128 KiB aggregate bound")
        );
    }
    if content.contains('\0') {
        return Err(Error::invalid_params().data("ACP text resource contains unsafe NUL bytes"));
    }
    Ok(format!(
        "[ACP explicit user context: {provenance}]\n{content}\n[End ACP explicit user context]"
    ))
}

fn validate_text_media_type(media_type: Option<&str>) -> Result<(), Error> {
    let supported = media_type.is_none_or(|value| {
        let value = value.split(';').next().unwrap_or_default().trim();
        value.starts_with("text/")
            || matches!(
                value,
                "application/json" | "application/xml" | "application/yaml"
            )
    });
    if supported {
        Ok(())
    } else {
        Err(Error::invalid_params().data("ACP resource media type is not supported text"))
    }
}

fn validate_resource_uri(uri: &str) -> Result<(), Error> {
    if uri.is_empty() || uri.len() > MAX_RESOURCE_URI_BYTES || uri.chars().any(char::is_control) {
        return Err(Error::invalid_params().data("ACP resource URI is malformed or oversized"));
    }
    Ok(())
}

fn read_linked_resource(
    resource: &agent_client_protocol::schema::v1::ResourceLink,
    roots: &[PathBuf],
) -> Result<(String, String), Error> {
    validate_text_media_type(resource.mime_type.as_deref())?;
    validate_resource_uri(&resource.uri)?;
    if resource.name.is_empty()
        || resource.name.len() > 256
        || resource.name.chars().any(char::is_control)
    {
        return Err(Error::invalid_params().data("ACP resource name is malformed or oversized"));
    }
    let mut uri = url::Url::parse(&resource.uri)
        .map_err(|_| Error::invalid_params().data("ACP resource link must be a valid file URI"))?;
    if uri.scheme() != "file"
        || uri.query().is_some()
        || uri
            .host_str()
            .is_some_and(|host| !host.is_empty() && host != "localhost")
    {
        return Err(Error::invalid_params()
            .data("ACP resource links must use a local file URI without a query"));
    }
    let range = parse_line_fragment(uri.fragment())?;
    uri.set_fragment(None);
    let path = uri
        .to_file_path()
        .map_err(|_| Error::invalid_params().data("ACP file resource path is malformed"))?;
    let root = roots
        .iter()
        .find(|root| path.starts_with(root.as_path()))
        .ok_or_else(|| {
            Error::invalid_params().data("ACP file resource is outside the workspace")
        })?;
    reject_resource_symlinks(root, &path)?;
    let metadata = std::fs::symlink_metadata(&path)
        .map_err(|_| Error::invalid_params().data("ACP file resource is unavailable"))?;
    if !metadata.file_type().is_file() {
        return Err(Error::invalid_params().data("ACP file resource is not a regular file"));
    }
    if metadata.len() > MAX_RESOURCE_BYTES as u64 {
        return Err(Error::invalid_params().data("ACP file resource exceeds the 64 KiB bound"));
    }
    let bytes = std::fs::read(&path)
        .map_err(|_| Error::invalid_params().data("ACP file resource could not be read"))?;
    let content = std::str::from_utf8(&bytes)
        .map_err(|_| Error::invalid_params().data("ACP file resource is not UTF-8 text"))?;
    let selected = select_resource_lines(content, range)?;
    let relative = path
        .strip_prefix(root)
        .map_err(|_| Error::invalid_params().data("ACP file resource containment changed"))?;
    Ok((
        format!("file {} ({})", resource.name, relative.display()),
        selected,
    ))
}

fn reject_resource_symlinks(root: &Path, target: &Path) -> Result<(), Error> {
    let relative = target
        .strip_prefix(root)
        .map_err(|_| Error::invalid_params().data("ACP file resource is outside the workspace"))?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        if !matches!(component, std::path::Component::Normal(_)) {
            return Err(Error::invalid_params().data("ACP file resource path is malformed"));
        }
        current.push(component);
        let metadata = std::fs::symlink_metadata(&current)
            .map_err(|_| Error::invalid_params().data("ACP file resource is unavailable"))?;
        if metadata.file_type().is_symlink() {
            return Err(Error::invalid_params().data("ACP file resource contains a symlink"));
        }
    }
    Ok(())
}

fn parse_line_fragment(fragment: Option<&str>) -> Result<Option<(usize, usize)>, Error> {
    let Some(fragment) = fragment else {
        return Ok(None);
    };
    let Some(rest) = fragment.strip_prefix('L') else {
        return Err(Error::invalid_params().data("ACP file resource line range is malformed"));
    };
    let (start, end) = rest.split_once("-L").map_or((rest, rest), |parts| parts);
    let start = start.parse::<usize>().ok().filter(|value| *value > 0);
    let end = end.parse::<usize>().ok().filter(|value| *value > 0);
    match (start, end) {
        (Some(start), Some(end)) if start <= end && end - start < 10_000 => Ok(Some((start, end))),
        _ => Err(Error::invalid_params().data("ACP file resource line range is malformed")),
    }
}

fn select_resource_lines(content: &str, range: Option<(usize, usize)>) -> Result<String, Error> {
    let Some((start, end)) = range else {
        return Ok(content.to_string());
    };
    let lines = content.lines().collect::<Vec<_>>();
    if start > lines.len() || end > lines.len() {
        return Err(Error::invalid_params().data("ACP file resource line range is unavailable"));
    }
    Ok(lines[start - 1..end].join("\n"))
}

fn available_commands() -> AvailableCommandsUpdate {
    AvailableCommandsUpdate::new(vec![
        AvailableCommand::new("agent", "Switch this session to agent mode"),
        AvailableCommand::new("plan", "Switch this session to plan mode"),
        AvailableCommand::new("ask", "Switch this session to ask mode"),
    ])
}

fn tool_kind(name: &str) -> ToolKind {
    if name.contains("read") || name.contains("view") {
        ToolKind::Read
    } else if name.contains("edit") || name.contains("write") || name.contains("patch") {
        ToolKind::Edit
    } else if name.contains("delete") || name.contains("remove") {
        ToolKind::Delete
    } else if name.contains("search") || name.contains("find") || name.contains("grep") {
        ToolKind::Search
    } else if name.contains("command") || name.contains("shell") || name.contains("job") {
        ToolKind::Execute
    } else if name.contains("fetch") || name.contains("browser") || name.contains("web") {
        ToolKind::Fetch
    } else if name.contains("plan") {
        ToolKind::Think
    } else {
        ToolKind::Other
    }
}

fn tool_locations(arguments: &serde_json::Value, roots: &[PathBuf]) -> Vec<ToolCallLocation> {
    let Some(object) = arguments.as_object() else {
        return Vec::new();
    };
    let line = object
        .get("line")
        .or_else(|| object.get("line_number"))
        .and_then(serde_json::Value::as_u64)
        .and_then(|line| u32::try_from(line).ok())
        .filter(|line| *line >= 1);
    let mut locations = Vec::new();
    for (key, value) in object {
        if !(key == "path" || key.ends_with("_path") || key == "file") {
            continue;
        }
        let Some(value) = value.as_str() else {
            continue;
        };
        let path = PathBuf::from(value);
        if let Some(path) = safe_tool_location(&path, roots) {
            locations.push(ToolCallLocation::new(path).line(line));
        }
    }
    locations.truncate(16);
    locations
}

fn safe_tool_location(path: &Path, roots: &[PathBuf]) -> Option<PathBuf> {
    use std::path::Component;

    if !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::CurDir))
    {
        return None;
    }
    let resolved = if path.exists() {
        path.canonicalize().ok()?
    } else {
        let parent = path.parent()?.canonicalize().ok()?;
        parent.join(path.file_name()?)
    };
    roots
        .iter()
        .any(|root| resolved.starts_with(root))
        .then_some(resolved)
}

fn tool_status(status: ToolResultStatus) -> ToolCallStatus {
    match status {
        ToolResultStatus::Success => ToolCallStatus::Completed,
        ToolResultStatus::Failure | ToolResultStatus::Denied | ToolResultStatus::Cancelled => {
            ToolCallStatus::Failed
        }
    }
}

fn failure_stop_reason(kind: HeadlessFailureKind) -> StopReason {
    match kind {
        HeadlessFailureKind::MaximumTurns => StopReason::MaxTurnRequests,
        HeadlessFailureKind::Interrupted | HeadlessFailureKind::ProviderFilter => {
            StopReason::Cancelled
        }
        _ => StopReason::Refusal,
    }
}

fn permission_request(
    session_id: &SessionId,
    call_id: &str,
    name: &str,
    reason: &str,
    network: bool,
) -> RequestPermissionRequest {
    let title = if network {
        format!("{name} (network): {reason}")
    } else {
        format!("{name}: {reason}")
    };
    RequestPermissionRequest::new(
        session_id.clone(),
        ToolCallUpdate::new(
            call_id.to_string(),
            ToolCallUpdateFields::new()
                .title(title)
                .status(ToolCallStatus::Pending),
        ),
        vec![
            PermissionOption::new("allow_once", "Allow once", PermissionOptionKind::AllowOnce),
            PermissionOption::new("deny", "Deny", PermissionOptionKind::RejectOnce),
        ],
    )
}

async fn send_update(
    connection: &ConnectionTo<Client>,
    session_id: &SessionId,
    update: SessionUpdate,
) -> Result<(), Error> {
    connection.send_notification(SessionNotification::new(session_id.clone(), update))
}

async fn run_prompt(
    state: AcpState,
    connection: ConnectionTo<Client>,
    request: PromptRequest,
) -> Result<PromptResponse, Error> {
    let session = state.session(&request.session_id).await?;
    let (roots, mode) = {
        let session = session.lock().await;
        (session.workspace_roots(), session.mode())
    };
    let prompt = request.prompt.clone();
    let prompt_roots = roots.clone();
    let text = tokio::task::spawn_blocking(move || prompt_text(&prompt, &prompt_roots))
        .await
        .map_err(|_| Error::internal_error().data("ACP resource worker failed"))??;
    let mut turn = {
        let mut session = session.lock().await;
        session.start_turn(text).map_err(input_error)?
    };
    state
        .set_active(&request.session_id, turn.cancellation())
        .await?;
    let outcome = async {
        send_update(
            &connection,
            &request.session_id,
            SessionUpdate::AvailableCommandsUpdate(available_commands()),
        )
        .await?;

        let mut response = PromptResponse::new(StopReason::EndTurn);
        while let Some(event) = turn.next_event().await {
            match event {
                LocalAgentEvent::Runner(HeadlessEventV1::AssistantOutput { delta }) => {
                    send_update(
                        &connection,
                        &request.session_id,
                        SessionUpdate::AgentMessageChunk(ContentChunk::new(ContentBlock::Text(
                            TextContent::new(delta),
                        ))),
                    )
                    .await?;
                }
                LocalAgentEvent::Runner(HeadlessEventV1::ReasoningOutput { delta }) => {
                    send_update(
                        &connection,
                        &request.session_id,
                        SessionUpdate::AgentThoughtChunk(ContentChunk::new(ContentBlock::Text(
                            TextContent::new(delta),
                        ))),
                    )
                    .await?;
                }
                LocalAgentEvent::Runner(HeadlessEventV1::ToolStart {
                    call_id,
                    name,
                    arguments,
                }) => {
                    let call = ToolCall::new(call_id, name.clone())
                        .kind(tool_kind(&name))
                        .status(ToolCallStatus::InProgress)
                        .locations(tool_locations(&arguments, &roots))
                        .raw_input(arguments);
                    send_update(
                        &connection,
                        &request.session_id,
                        SessionUpdate::ToolCall(call),
                    )
                    .await?;
                }
                LocalAgentEvent::Runner(HeadlessEventV1::ToolFinish {
                    call_id,
                    status,
                    output,
                    ..
                }) => {
                    let update = ToolCallUpdate::new(
                        call_id,
                        ToolCallUpdateFields::new()
                            .status(tool_status(status))
                            .content(vec![output.clone().into()])
                            .raw_output(serde_json::Value::String(output)),
                    );
                    send_update(
                        &connection,
                        &request.session_id,
                        SessionUpdate::ToolCallUpdate(update),
                    )
                    .await?;
                }
                LocalAgentEvent::Runner(HeadlessEventV1::Usage { usage }) => {
                    let used = u64::from(usage.total_tokens);
                    let size = turn.context_window_tokens().unwrap_or(used).max(used);
                    send_update(
                        &connection,
                        &request.session_id,
                        SessionUpdate::UsageUpdate(UsageUpdate::new(used, size)),
                    )
                    .await?;
                }
                LocalAgentEvent::Runner(HeadlessEventV1::Failure { kind, message, .. }) => {
                    response = PromptResponse::new(failure_stop_reason(kind));
                    send_update(
                        &connection,
                        &request.session_id,
                        SessionUpdate::AgentThoughtChunk(ContentChunk::new(ContentBlock::Text(
                            TextContent::new(format!("CA stopped the turn: {message}")),
                        ))),
                    )
                    .await?;
                }
                LocalAgentEvent::Runner(HeadlessEventV1::Completion { message })
                    if mode == LocalAgentMode::Plan =>
                {
                    send_update(
                        &connection,
                        &request.session_id,
                        SessionUpdate::Plan(Plan::new(vec![PlanEntry::new(
                            message,
                            PlanEntryPriority::High,
                            PlanEntryStatus::Completed,
                        )])),
                    )
                    .await?;
                }
                LocalAgentEvent::Runner(_) => {}
                LocalAgentEvent::ApprovalRequested {
                    call_id,
                    name,
                    reason,
                    network,
                } => {
                    let cancellation = turn.cancellation();
                    let decision = connection
                        .send_request(permission_request(
                            &request.session_id,
                            &call_id,
                            &name,
                            &reason,
                            network,
                        ))
                        .block_task();
                    let decision = tokio::select! {
                        result = tokio::time::timeout(PERMISSION_RESPONSE_TIMEOUT, decision) => {
                            result.ok().and_then(Result::ok)
                        }
                        () = cancellation.cancelled() => {
                            response = PromptResponse::new(StopReason::Cancelled);
                            break;
                        }
                    };
                    let allowed = decision.is_some_and(|decision| {
                        matches!(
                            decision.outcome,
                            RequestPermissionOutcome::Selected(selected)
                                if selected.option_id.to_string() == "allow_once"
                        )
                    });
                    if allowed {
                        turn.allow_once().await.map_err(internal_error)?;
                    } else {
                        // Missing permission support, disconnects, malformed answers and
                        // explicit rejection all fail closed through the native tool gate.
                        turn.deny().await.map_err(internal_error)?;
                    }
                }
                LocalAgentEvent::InteractionRequested => {
                    turn.cancellation().cancel();
                    response = PromptResponse::new(StopReason::Refusal);
                    break;
                }
                LocalAgentEvent::Completed => break,
                LocalAgentEvent::Failed => break,
                LocalAgentEvent::Cancelled => {
                    response = PromptResponse::new(StopReason::Cancelled);
                    break;
                }
            }
        }
        Ok(response)
    }
    .await;
    if outcome.is_err() {
        // A broken ACP transport must not leave a native turn running or the
        // session marked busy. Cancellation still flows through CA's normal
        // job, MCP and LSP cleanup path below.
        turn.cancellation().cancel();
    }
    turn.join().await;
    {
        let mut session = session.lock().await;
        session.apply_turn(&mut turn);
        if outcome.is_err()
            || matches!(
                outcome,
                Ok(ref response) if response.stop_reason == StopReason::Cancelled
            )
        {
            session.cleanup_after_cancel().await;
        }
    }
    state.clear_active(&request.session_id).await;
    outcome
}

fn input_error(message: String) -> Error {
    Error::invalid_params().data(crate::terminal_text::sanitize_terminal_text(&message))
}

fn internal_error(message: String) -> Error {
    Error::internal_error().data(crate::terminal_text::sanitize_terminal_text(&message))
}

fn agent_component(state: AcpState) -> impl ConnectTo<Client> {
    Agent
        .builder()
        .name("ca-acp-v1")
        .on_receive_request(
            {
                let state = state.clone();
                async move |request: InitializeRequest, responder, _connection| {
                    let result = initialize_response(&request);
                    if result.is_ok() {
                        state.initialized.store(true, Ordering::Release);
                    }
                    responder.respond_with_result(result)
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let state = state.clone();
                async move |request: NewSessionRequest, responder, connection| {
                    state.require_initialized()?;
                    validate_session_request(&request.cwd, request.mcp_servers.len())?;
                    connection.spawn({
                        let state = state.clone();
                        async move {
                            let result = state
                                .factory
                                .new_session(&request.cwd, &request.additional_directories)
                                .await
                                .map_err(internal_error);
                            let response = match result {
                                Ok(session) => state.insert_session(session).await.map(|id| {
                                    NewSessionResponse::new(id)
                                        .modes(mode_state(LocalAgentMode::Agent))
                                }),
                                Err(error) => Err(error),
                            };
                            responder.respond_with_result(response)
                        }
                    })?;
                    Ok(())
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let state = state.clone();
                async move |request: LoadSessionRequest, responder, connection| {
                    state.require_initialized()?;
                    validate_session_request(&request.cwd, request.mcp_servers.len())?;
                    let id = Uuid::parse_str(&request.session_id.to_string())
                        .map_err(|_| Error::invalid_params().data("sessionId must be a CA UUID"))?;
                    connection.spawn({
                        let state = state.clone();
                        async move {
                            let result = state
                                .factory
                                .load_session(id, &request.cwd, &request.additional_directories)
                                .await
                                .map_err(internal_error);
                            let response = match result {
                                Ok(session) => state.insert_session(session).await.map(|_| {
                                    LoadSessionResponse::new()
                                        .modes(mode_state(LocalAgentMode::Agent))
                                }),
                                Err(error) => Err(error),
                            };
                            responder.respond_with_result(response)
                        }
                    })?;
                    Ok(())
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let state = state.clone();
                async move |request: SetSessionModeRequest, responder, connection| {
                    let session = state.session(&request.session_id).await?;
                    if state
                        .active
                        .lock()
                        .await
                        .contains_key(&request.session_id.to_string())
                    {
                        return responder.respond_with_error(
                            Error::invalid_request()
                                .data("cannot change mode during an active prompt"),
                        );
                    }
                    let mode = parse_mode(&request.mode_id.to_string())?;
                    session.lock().await.set_mode(mode);
                    connection.send_notification(SessionNotification::new(
                        request.session_id,
                        SessionUpdate::CurrentModeUpdate(CurrentModeUpdate::new(mode_id(mode))),
                    ))?;
                    responder.respond(SetSessionModeResponse::new())
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let state = state.clone();
                async move |request: PromptRequest, responder, connection| {
                    state.require_initialized()?;
                    connection.spawn({
                        let state = state.clone();
                        let connection = connection.clone();
                        async move {
                            responder
                                .respond_with_result(run_prompt(state, connection, request).await)
                        }
                    })?;
                    Ok(())
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_notification(
            {
                let state = state.clone();
                async move |request: CancelNotification, _connection| {
                    if let Some(cancellation) = state
                        .active
                        .lock()
                        .await
                        .get(&request.session_id.to_string())
                        .cloned()
                    {
                        cancellation.cancel();
                    }
                    Ok(())
                }
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_close({
            let state = state.clone();
            async move |_connection| {
                state.shutdown().await;
                Ok(())
            }
        })
}

pub async fn run_production(args: AcpCliArgs) -> i32 {
    let state = AcpState::new(&args);
    let result = agent_component(state).connect_to(Stdio::new()).await;
    match result {
        Ok(()) => 0,
        Err(error) => {
            eprintln!(
                "ca acp: {}",
                crate::terminal_text::sanitize_terminal_text(&error.to_string())
            );
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::client::{ChatBackend, ChatMessage, ChatStreamItem, ToolDef};
    use crate::agent::stream::{FinishReason, ProviderError, ProviderEvent, UsageInfo};
    use agent_client_protocol::schema::v1::{RequestPermissionResponse, SelectedPermissionOutcome};
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
    use std::sync::Mutex as StdMutex;

    #[derive(Clone, Copy)]
    enum FixtureScenario {
        Text,
        Mutation,
        Pending,
    }

    struct FixtureBackend {
        calls: AtomicUsize,
        scenario: FixtureScenario,
        root: PathBuf,
        held: StdMutex<Vec<tokio::sync::mpsc::UnboundedSender<ChatStreamItem>>>,
        started: Arc<tokio::sync::Notify>,
    }

    #[async_trait]
    impl ChatBackend for FixtureBackend {
        async fn stream_chat(
            &self,
            _messages: &[ChatMessage],
            _tools: &[ToolDef],
        ) -> Result<tokio::sync::mpsc::UnboundedReceiver<ChatStreamItem>, ProviderError> {
            unreachable!("ACP test sessions use prepared requests")
        }

        async fn stream_prepared(
            &self,
            _request: &crate::request::PreparedModelRequest,
        ) -> Result<tokio::sync::mpsc::UnboundedReceiver<ChatStreamItem>, ProviderError> {
            let call = self.calls.fetch_add(1, AtomicOrdering::Relaxed);
            let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
            if matches!(self.scenario, FixtureScenario::Pending) {
                self.started.notify_waiters();
                self.held.lock().unwrap().push(sender);
                return Ok(receiver);
            }
            if matches!(self.scenario, FixtureScenario::Mutation) && call == 0 {
                sender
                    .send(Ok(ProviderEvent::ToolCallStart {
                        index: 0,
                        id: "fixture-write".into(),
                        name: "write_file".into(),
                    }))
                    .unwrap();
                sender
                    .send(Ok(ProviderEvent::ToolCallArg {
                        index: 0,
                        arg: serde_json::json!({
                            "path": self.root.join("approved.txt"),
                            "content": "approved"
                        })
                        .to_string(),
                    }))
                    .unwrap();
                sender
                    .send(Ok(ProviderEvent::Finish {
                        reason: FinishReason::ToolCalls,
                        usage: None,
                    }))
                    .unwrap();
                sender.send(Ok(ProviderEvent::Done)).unwrap();
                return Ok(receiver);
            }
            sender
                .send(Ok(ProviderEvent::Text("fixture reply".into())))
                .unwrap();
            sender
                .send(Ok(ProviderEvent::Usage(UsageInfo {
                    prompt_tokens: 3,
                    completion_tokens: 2,
                    total_tokens: 5,
                    cached_prompt_tokens: None,
                    cache_write_tokens: None,
                    reasoning_tokens: None,
                    response_metadata: None,
                    cache_usage_override: None,
                })))
                .unwrap();
            sender
                .send(Ok(ProviderEvent::Finish {
                    reason: FinishReason::Stop,
                    usage: None,
                }))
                .unwrap();
            sender.send(Ok(ProviderEvent::Done)).unwrap();
            Ok(receiver)
        }
    }

    struct TestSessionFactory {
        root: PathBuf,
        scenario: FixtureScenario,
        started: Arc<tokio::sync::Notify>,
    }

    #[async_trait]
    impl AcpSessionFactory for TestSessionFactory {
        async fn new_session(
            &self,
            cwd: &Path,
            _additional_directories: &[PathBuf],
        ) -> Result<LocalAgentSession, String> {
            if cwd != self.root {
                return Err("unexpected fixture cwd".into());
            }
            LocalAgentSession::for_test(
                Uuid::new_v4(),
                &self.root,
                Arc::new(FixtureBackend {
                    calls: AtomicUsize::new(0),
                    scenario: self.scenario,
                    root: self.root.clone(),
                    held: StdMutex::new(Vec::new()),
                    started: self.started.clone(),
                }),
            )
        }

        async fn load_session(
            &self,
            id: Uuid,
            cwd: &Path,
            _additional_directories: &[PathBuf],
        ) -> Result<LocalAgentSession, String> {
            if cwd != self.root {
                return Err("unexpected fixture cwd".into());
            }
            LocalAgentSession::for_test(
                id,
                &self.root,
                Arc::new(FixtureBackend {
                    calls: AtomicUsize::new(0),
                    scenario: self.scenario,
                    root: self.root.clone(),
                    held: StdMutex::new(Vec::new()),
                    started: self.started.clone(),
                }),
            )
        }
    }

    #[test]
    fn stable_initialize_advertises_only_local_v1_capabilities() {
        let request = InitializeRequest::new(ProtocolVersion::V1);
        let response = initialize_response(&request).unwrap();
        assert_eq!(response.protocol_version, ProtocolVersion::V1);
        assert!(response.agent_capabilities.load_session);
        assert!(response.auth_methods.is_empty());
        assert!(response
            .agent_capabilities
            .session_capabilities
            .additional_directories
            .is_some());
        assert!(!response.agent_capabilities.prompt_capabilities.image);
        assert!(
            response
                .agent_capabilities
                .prompt_capabilities
                .embedded_context
        );
    }

    #[test]
    fn v2_and_non_absolute_workspace_are_rejected() {
        let request = InitializeRequest::new(2.into());
        assert!(initialize_response(&request).is_err());
        assert!(validate_session_request(Path::new("relative"), 0).is_err());
        assert!(validate_session_request(Path::new("/tmp"), 1).is_err());
    }

    #[test]
    fn prompt_blocks_are_bounded_and_resource_links_are_data() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().canonicalize().unwrap();
        std::fs::write(root.join("notes.md"), "linked text").unwrap();
        let text = prompt_text(
            &[
                ContentBlock::Text(TextContent::new("hello")),
                ContentBlock::ResourceLink(agent_client_protocol::schema::v1::ResourceLink::new(
                    "notes",
                    url::Url::from_file_path(root.join("notes.md"))
                        .unwrap()
                        .to_string(),
                )),
            ],
            std::slice::from_ref(&root),
        )
        .unwrap();
        assert!(text.contains("hello"));
        assert!(text.contains("ACP explicit user context"));
        assert!(text.contains("linked text"));
    }

    #[test]
    fn embedded_and_linked_editor_context_is_bounded_contained_and_range_checked() {
        use agent_client_protocol::schema::v1::{
            EmbeddedResource, ResourceLink, TextResourceContents,
        };

        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().canonicalize().unwrap();
        let path = root.join("source.rs");
        std::fs::write(&path, "one\ntwo\nthree\n").unwrap();
        let mut ranged_uri = url::Url::from_file_path(&path).unwrap();
        ranged_uri.set_fragment(Some("L2-L3"));
        let prompt = vec![
            ContentBlock::Text(TextContent::new("inspect these selections")),
            ContentBlock::Resource(EmbeddedResource::new(
                EmbeddedResourceResource::TextResourceContents(
                    TextResourceContents::new("selected α", "editor://selection")
                        .mime_type("text/plain"),
                ),
            )),
            ContentBlock::ResourceLink(
                ResourceLink::new("open file", ranged_uri.to_string()).mime_type("text/x-rust"),
            ),
        ];
        let rendered = prompt_text(&prompt, std::slice::from_ref(&root)).unwrap();
        assert!(rendered.contains("embedded editor://selection"));
        assert!(rendered.contains("selected α"));
        assert!(rendered.contains("file open file (source.rs)"));
        assert!(rendered.contains("two\nthree"));
        assert!(!rendered.contains("\none\n"));

        let mut malformed = url::Url::from_file_path(&path).unwrap();
        malformed.set_fragment(Some("line=2"));
        assert!(prompt_text(
            &[ContentBlock::ResourceLink(ResourceLink::new(
                "bad range",
                malformed.to_string()
            ))],
            std::slice::from_ref(&root)
        )
        .is_err());
        assert!(prompt_text(
            &[ContentBlock::ResourceLink(ResourceLink::new(
                "remote",
                "https://example.invalid/context"
            ))],
            std::slice::from_ref(&root)
        )
        .is_err());

        let outside = tempfile::NamedTempFile::new().unwrap();
        let outside_uri = url::Url::from_file_path(outside.path()).unwrap();
        assert!(prompt_text(
            &[ContentBlock::ResourceLink(ResourceLink::new(
                "outside",
                outside_uri.to_string()
            ))],
            std::slice::from_ref(&root)
        )
        .is_err());
    }

    #[test]
    fn editor_context_rejects_binary_count_item_and_aggregate_overflow() {
        use agent_client_protocol::schema::v1::{
            BlobResourceContents, EmbeddedResource, TextResourceContents,
        };
        let root = tempfile::tempdir().unwrap();
        let embedded = |text: String, index: usize| {
            ContentBlock::Resource(EmbeddedResource::new(
                EmbeddedResourceResource::TextResourceContents(TextResourceContents::new(
                    text,
                    format!("editor://selection/{index}"),
                )),
            ))
        };
        assert!(prompt_text(
            &[embedded("x".repeat(MAX_RESOURCE_BYTES + 1), 0)],
            &[root.path().to_path_buf()]
        )
        .is_err());
        let too_many = (0..=MAX_RESOURCE_COUNT)
            .map(|index| embedded("x".into(), index))
            .collect::<Vec<_>>();
        assert!(prompt_text(&too_many, &[root.path().to_path_buf()]).is_err());
        let aggregate = (0..3)
            .map(|index| embedded("x".repeat(48 * 1024), index))
            .collect::<Vec<_>>();
        assert!(prompt_text(&aggregate, &[root.path().to_path_buf()]).is_err());
        let binary = ContentBlock::Resource(EmbeddedResource::new(
            EmbeddedResourceResource::BlobResourceContents(BlobResourceContents::new(
                "AA==",
                "editor://binary",
            )),
        ));
        assert!(prompt_text(&[binary], &[root.path().to_path_buf()]).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn linked_editor_context_rejects_symlink_components_and_binary_text() {
        use agent_client_protocol::schema::v1::ResourceLink;
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().canonicalize().unwrap();
        let real = root.join("real.txt");
        std::fs::write(&real, "context").unwrap();
        let linked = root.join("linked.txt");
        std::os::unix::fs::symlink(&real, &linked).unwrap();
        assert!(prompt_text(
            &[ContentBlock::ResourceLink(ResourceLink::new(
                "linked",
                url::Url::from_file_path(&linked).unwrap().to_string()
            ))],
            std::slice::from_ref(&root)
        )
        .is_err());

        let binary = root.join("binary.txt");
        std::fs::write(&binary, [0xff, 0xfe]).unwrap();
        assert!(prompt_text(
            &[ContentBlock::ResourceLink(ResourceLink::new(
                "binary",
                url::Url::from_file_path(&binary).unwrap().to_string()
            ))],
            std::slice::from_ref(&root)
        )
        .is_err());
    }

    #[test]
    fn tool_locations_are_absolute_contained_and_one_based() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().canonicalize().unwrap();
        std::fs::create_dir(root.join("src")).unwrap();
        let path = root.join("src/main.rs");
        let locations = tool_locations(
            &serde_json::json!({"path":path, "line":7}),
            std::slice::from_ref(&root),
        );
        assert_eq!(locations.len(), 1);
        assert_eq!(locations[0].path, root.join("src/main.rs"));
        assert_eq!(locations[0].line, Some(7));
        assert!(tool_locations(
            &serde_json::json!({"path":"relative.rs", "line":0}),
            std::slice::from_ref(&root),
        )
        .is_empty());
        assert!(tool_locations(
            &serde_json::json!({"path":root.join("../escaped.rs"), "line":1}),
            std::slice::from_ref(&root),
        )
        .is_empty());
    }

    #[tokio::test]
    async fn official_sdk_client_exercises_v1_new_load_prompt_and_mode_updates() {
        use agent_client_protocol::schema::v1::{EmbeddedResource, TextResourceContents};
        let root = tempfile::tempdir().unwrap();
        let root = root.path().canonicalize().unwrap();
        std::fs::write(root.join("open.rs"), "fn fixture() {}\n").unwrap();
        let state = AcpState::with_factory(Arc::new(TestSessionFactory {
            root: root.clone(),
            scenario: FixtureScenario::Text,
            started: Arc::new(tokio::sync::Notify::new()),
        }));
        let updates = Arc::new(Mutex::new(Vec::<SessionUpdate>::new()));
        let observed = updates.clone();
        agent_client_protocol::Client
            .builder()
            .on_receive_notification(
                async move |notification: SessionNotification, _connection| {
                    observed.lock().await.push(notification.update);
                    Ok(())
                },
                agent_client_protocol::on_receive_notification!(),
            )
            .connect_with(agent_component(state), async move |connection| {
                let initialized = connection
                    .send_request(InitializeRequest::new(ProtocolVersion::V1))
                    .block_task()
                    .await?;
                assert_eq!(initialized.protocol_version, ProtocolVersion::V1);

                let created = connection
                    .send_request(NewSessionRequest::new(root.clone()))
                    .block_task()
                    .await?;
                assert_eq!(
                    created.modes.as_ref().unwrap().current_mode_id.to_string(),
                    "agent"
                );
                connection
                    .send_request(SetSessionModeRequest::new(
                        created.session_id.clone(),
                        "plan",
                    ))
                    .block_task()
                    .await?;
                let completed = connection
                    .send_request(PromptRequest::new(
                        created.session_id,
                        vec![
                            ContentBlock::Text(TextContent::new("make a plan")),
                            ContentBlock::Resource(EmbeddedResource::new(
                                EmbeddedResourceResource::TextResourceContents(
                                    TextResourceContents::new(
                                        "selected fixture",
                                        "editor://selection",
                                    ),
                                ),
                            )),
                            ContentBlock::ResourceLink(
                                agent_client_protocol::schema::v1::ResourceLink::new(
                                    "open file",
                                    url::Url::from_file_path(root.join("open.rs"))
                                        .unwrap()
                                        .to_string(),
                                ),
                            ),
                        ],
                    ))
                    .block_task()
                    .await?;
                assert_eq!(completed.stop_reason, StopReason::EndTurn);

                let load_id = Uuid::new_v4();
                let loaded = connection
                    .send_request(LoadSessionRequest::new(load_id.to_string(), root))
                    .block_task()
                    .await?;
                assert_eq!(
                    loaded.modes.as_ref().unwrap().current_mode_id.to_string(),
                    "agent"
                );
                Ok(())
            })
            .await
            .unwrap();

        let updates = updates.lock().await;
        assert!(updates.iter().any(|update| matches!(
            update,
            SessionUpdate::CurrentModeUpdate(mode) if mode.current_mode_id.to_string() == "plan"
        )));
        assert!(updates.iter().any(|update| matches!(
            update,
            SessionUpdate::AgentMessageChunk(chunk)
                if matches!(&chunk.content, ContentBlock::Text(text) if text.text == "fixture reply")
        )));
        assert!(updates
            .iter()
            .any(|update| matches!(update, SessionUpdate::UsageUpdate(usage) if usage.used == 5)));
        assert!(updates
            .iter()
            .any(|update| matches!(update, SessionUpdate::Plan(_))));
    }

    async fn run_permission_fixture(allow: bool) {
        let root = tempfile::tempdir().unwrap();
        let root = root.path().canonicalize().unwrap();
        let state = AcpState::with_factory(Arc::new(TestSessionFactory {
            root: root.clone(),
            scenario: FixtureScenario::Mutation,
            started: Arc::new(tokio::sync::Notify::new()),
        }));
        let requests = Arc::new(AtomicUsize::new(0));
        let observed = requests.clone();
        let request_root = root.clone();
        agent_client_protocol::Client
            .builder()
            .on_receive_request(
                async move |request: RequestPermissionRequest, responder, _connection| {
                    observed.fetch_add(1, AtomicOrdering::Relaxed);
                    let option = if allow { "allow_once" } else { "deny" };
                    assert!(request
                        .options
                        .iter()
                        .any(|candidate| candidate.option_id.to_string() == option));
                    responder.respond(RequestPermissionResponse::new(
                        RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(option)),
                    ))
                },
                agent_client_protocol::on_receive_request!(),
            )
            .connect_with(agent_component(state), async move |connection| {
                connection
                    .send_request(InitializeRequest::new(ProtocolVersion::V1))
                    .block_task()
                    .await?;
                let created = connection
                    .send_request(NewSessionRequest::new(request_root))
                    .block_task()
                    .await?;
                let completed = connection
                    .send_request(PromptRequest::new(
                        created.session_id,
                        vec![ContentBlock::Text(TextContent::new("write the fixture"))],
                    ))
                    .block_task()
                    .await?;
                assert_eq!(completed.stop_reason, StopReason::EndTurn);
                Ok(())
            })
            .await
            .unwrap();
        assert_eq!(requests.load(AtomicOrdering::Relaxed), 1);
        assert_eq!(root.join("approved.txt").exists(), allow);
    }

    #[tokio::test]
    async fn official_sdk_permission_allow_executes_and_deny_fails_closed() {
        run_permission_fixture(false).await;
        run_permission_fixture(true).await;
    }

    #[tokio::test]
    async fn client_without_permission_handler_cannot_mutate() {
        let root = tempfile::tempdir().unwrap();
        let root = root.path().canonicalize().unwrap();
        let state = AcpState::with_factory(Arc::new(TestSessionFactory {
            root: root.clone(),
            scenario: FixtureScenario::Mutation,
            started: Arc::new(tokio::sync::Notify::new()),
        }));
        let request_root = root.clone();
        agent_client_protocol::Client
            .builder()
            .connect_with(agent_component(state), async move |connection| {
                connection
                    .send_request(InitializeRequest::new(ProtocolVersion::V1))
                    .block_task()
                    .await?;
                let created = connection
                    .send_request(NewSessionRequest::new(request_root))
                    .block_task()
                    .await?;
                let completed = connection
                    .send_request(PromptRequest::new(
                        created.session_id,
                        vec![ContentBlock::Text(TextContent::new(
                            "write without handler",
                        ))],
                    ))
                    .block_task()
                    .await?;
                assert_eq!(completed.stop_reason, StopReason::EndTurn);
                Ok(())
            })
            .await
            .unwrap();
        assert!(!root.join("approved.txt").exists());
    }

    #[tokio::test]
    async fn official_sdk_cancel_stops_prompt_and_disconnect_cleans_sessions() {
        let root = tempfile::tempdir().unwrap();
        let root = root.path().canonicalize().unwrap();
        let started = Arc::new(tokio::sync::Notify::new());
        let state = AcpState::with_factory(Arc::new(TestSessionFactory {
            root: root.clone(),
            scenario: FixtureScenario::Pending,
            started: started.clone(),
        }));
        let state_after = state.clone();
        agent_client_protocol::Client
            .builder()
            .connect_with(agent_component(state), async move |connection| {
                connection
                    .send_request(InitializeRequest::new(ProtocolVersion::V1))
                    .block_task()
                    .await?;
                let created = connection
                    .send_request(NewSessionRequest::new(root))
                    .block_task()
                    .await?;
                let waiting = started.notified();
                let prompt = connection
                    .send_request(PromptRequest::new(
                        created.session_id.clone(),
                        vec![ContentBlock::Text(TextContent::new("wait"))],
                    ))
                    .block_task();
                waiting.await;
                connection.send_notification(CancelNotification::new(created.session_id))?;
                let completed = tokio::time::timeout(std::time::Duration::from_secs(2), prompt)
                    .await
                    .map_err(|_| Error::internal_error().data("cancel fixture timed out"))??;
                assert_eq!(completed.stop_reason, StopReason::Cancelled);
                Ok(())
            })
            .await
            .unwrap();
        assert!(state_after.active.lock().await.is_empty());
        // Direct in-memory component cancellation is not transport EOF; invoke
        // the same cleanup used by the stdio on-close callback before checking
        // resource ownership.
        state_after.shutdown().await;
        assert!(state_after.sessions.lock().await.is_empty());
    }

    #[tokio::test]
    async fn newline_transport_reports_parse_errors_and_clean_eof_runs_disconnect_cleanup() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

        let root = tempfile::tempdir().unwrap();
        let root = root.path().canonicalize().unwrap();
        let state = AcpState::with_factory(Arc::new(TestSessionFactory {
            root: root.clone(),
            scenario: FixtureScenario::Text,
            started: Arc::new(tokio::sync::Notify::new()),
        }));
        let state_after = state.clone();
        let (mut client_input, agent_input) = tokio::io::duplex(64 * 1024);
        let (agent_output, client_output) = tokio::io::duplex(64 * 1024);
        let task = tokio::spawn(agent_component(state).connect_to(
            agent_client_protocol::ByteStreams::new(
                agent_output.compat_write(),
                agent_input.compat(),
            ),
        ));
        let mut output = BufReader::new(client_output);
        client_input.write_all(b"{not-json}\n").await.unwrap();
        let mut line = String::new();
        output.read_line(&mut line).await.unwrap();
        let parse_error: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(parse_error["error"]["code"], -32700);

        line.clear();
        client_input
            .write_all(
                format!(
                    "{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{{\"protocolVersion\":1,\"clientCapabilities\":{{}}}}}}\n{{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"session/new\",\"params\":{{\"cwd\":{},\"mcpServers\":[]}}}}\n",
                    serde_json::to_string(&root).unwrap()
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        output.read_line(&mut line).await.unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&line).unwrap()["id"],
            1
        );
        line.clear();
        output.read_line(&mut line).await.unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&line).unwrap()["id"],
            2
        );
        assert_eq!(state_after.sessions.lock().await.len(), 1);

        client_input.shutdown().await.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(2), task)
            .await
            .expect("ACP transport did not close after EOF")
            .unwrap()
            .unwrap();
        assert!(state_after.sessions.lock().await.is_empty());
        assert!(state_after.active.lock().await.is_empty());
    }
}
