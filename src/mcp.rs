use crate::agent::registry::{ToolRegistry, ToolSpec};
use crate::config::{CaConfig, McpServerConfig, McpTransport, ProjectMcpTrust};
use crate::sandbox::{SandboxBackend, SandboxMode, SandboxPolicy};
use crate::workspace::WorkspaceContext;
use futures::StreamExt as _;
use rmcp::handler::client::ClientHandler;
use rmcp::model::{
    CallToolRequestParams, ClientInfo, ClientJsonRpcMessage, ContentBlock,
    ReadResourceRequestParams, Resource, ResourceContents, ServerJsonRpcMessage,
};
use rmcp::service::{NotificationContext, RoleClient, RunningService};
use rmcp::transport::streamable_http_client::{
    AuthRequiredError, InsufficientScopeError, StreamableHttpClient,
    StreamableHttpClientTransportConfig, StreamableHttpError, StreamableHttpPostResponse,
};
use rmcp::transport::{StreamableHttpClientTransport, TokioChildProcess};
use rmcp::ServiceExt;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Mutex;

const MAX_CONFIG_BYTES: usize = 128 * 1024;
const MAX_TOOLS: usize = 128;
const MAX_RESOURCES: usize = 128;
const MAX_SCHEMA_BYTES: usize = 64 * 1024;
const MAX_CATALOG_BYTES: usize = 512 * 1024;
const MAX_RESOURCE_METADATA_BYTES: usize = 64 * 1024;
const MAX_OUTPUT_BYTES: usize = 256 * 1024;
const MAX_STDERR_BYTES: usize = 64 * 1024;
const MAX_HTTP_EVENT_BYTES: usize = 1024 * 1024;
const MAX_HTTP_JSON_BYTES: usize = 1024 * 1024;
const MAX_SEARCH_QUERY_BYTES: usize = 256;
const MAX_SEARCH_SERVERS: usize = 8;
const MAX_SEARCH_CONCURRENCY: usize = 4;
const MAX_SEARCH_RESULTS: usize = 10;
const MAX_SEARCH_OUTPUT_BYTES: usize = 8 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpConfigOrigin {
    User,
    Project,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpServerState {
    Stopped,
    Running,
    Untrusted,
    NetworkApprovalRequired,
    Failed,
    AuthorizationRequired,
}

#[derive(Debug, Clone)]
pub struct McpServerStatus {
    pub name: String,
    pub origin: McpConfigOrigin,
    pub state: McpServerState,
    pub tool_count: usize,
    pub active_tool_count: usize,
    pub resource_count: usize,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpResourceInfo {
    pub server: String,
    pub uri: String,
    pub name: String,
    pub description: Option<String>,
    pub mime_type: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct McpCatalog {
    pub registry: Option<ToolRegistry>,
    pub resources: Vec<McpResourceInfo>,
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum McpError {
    #[error("unknown MCP server: {0}")]
    UnknownServer(String),
    #[error("MCP server {0} is not trusted")]
    Untrusted(String),
    #[error("MCP server {0} requires a distinct network grant")]
    NetworkApprovalRequired(String),
    #[error("invalid MCP configuration: {0}")]
    InvalidConfig(String),
    #[error("MCP server {server} failed to start: {detail}")]
    Start { server: String, detail: String },
    #[error("MCP server {server} timed out during {operation}")]
    Timeout {
        server: String,
        operation: &'static str,
    },
    #[error("MCP server {server} protocol failure: {detail}")]
    Protocol { server: String, detail: String },
    #[error("MCP server {server} tool error {code}: {message}")]
    Tool {
        server: String,
        code: String,
        message: String,
        retryable: bool,
    },
    #[error("MCP output exceeded {0} bytes")]
    OutputLimit(usize),
    #[error("MCP binary content is not supported")]
    BinaryContent,
    #[error("MCP server {0} requires authorization; run /mcp connect {0}")]
    AuthorizationRequired(String),
    #[error("MCP server {server} authorization {operation} failed")]
    AuthorizationFailed {
        server: String,
        operation: &'static str,
    },
}

#[derive(Debug, Clone)]
struct ServerDefinition {
    name: String,
    config: McpServerConfig,
    origin: McpConfigOrigin,
    trusted: bool,
    network_trusted: bool,
}

#[derive(Clone)]
struct McpClientHandler {
    stale_tools: Arc<AtomicBool>,
    stale_resources: Arc<AtomicBool>,
}

impl ClientHandler for McpClientHandler {
    fn on_resource_list_changed(
        &self,
        _context: NotificationContext<RoleClient>,
    ) -> impl std::future::Future<Output = ()> + Send + '_ {
        self.stale_resources.store(true, Ordering::Release);
        std::future::ready(())
    }

    fn on_tool_list_changed(
        &self,
        _context: NotificationContext<RoleClient>,
    ) -> impl std::future::Future<Output = ()> + Send + '_ {
        self.stale_tools.store(true, Ordering::Release);
        std::future::ready(())
    }

    fn get_info(&self) -> ClientInfo {
        ClientInfo::default()
    }
}

type McpService = RunningService<RoleClient, McpClientHandler>;

struct RunningServer {
    service: McpService,
    tools: Vec<RemoteTool>,
    resources: Vec<Resource>,
    stale_tools: Arc<AtomicBool>,
    stale_resources: Arc<AtomicBool>,
    stderr: Arc<Mutex<String>>,
    temporary_directory: PathBuf,
    process_group: Option<i32>,
    transport: McpTransport,
}

#[derive(Debug, thiserror::Error)]
#[error("HTTP MCP request failed")]
struct SafeHttpClientError;

#[derive(Clone)]
struct SafeHttpClient {
    client: reqwest_13::Client,
    header_references: Arc<Vec<(http::HeaderName, String)>>,
}

#[derive(Debug, thiserror::Error)]
enum SafeHttpBodyError {
    #[error("HTTP MCP response failed")]
    Source,
    #[error("HTTP MCP SSE event exceeded its size limit")]
    EventTooLarge,
}

#[derive(Debug)]
struct SseEventLimiter {
    max_bytes: usize,
    event_bytes: usize,
    line_bytes: usize,
    previous_was_cr: bool,
}

impl SseEventLimiter {
    fn new(max_bytes: usize) -> Self {
        Self {
            max_bytes,
            event_bytes: 0,
            line_bytes: 0,
            previous_was_cr: false,
        }
    }

    fn observe(&mut self, chunk: &[u8]) -> Result<(), SafeHttpBodyError> {
        for byte in chunk.iter().copied() {
            if self.previous_was_cr {
                self.previous_was_cr = false;
                if byte == b'\n' {
                    continue;
                }
            }

            self.event_bytes = self.event_bytes.saturating_add(1);
            match byte {
                b'\r' | b'\n' => {
                    if self.line_bytes == 0 {
                        self.event_bytes = 0;
                    }
                    self.line_bytes = 0;
                    self.previous_was_cr = byte == b'\r';
                }
                _ => self.line_bytes = self.line_bytes.saturating_add(1),
            }
            if self.event_bytes > self.max_bytes {
                return Err(SafeHttpBodyError::EventTooLarge);
            }
        }
        Ok(())
    }
}

impl std::fmt::Debug for SafeHttpClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SafeHttpClient")
            .field("header_count", &self.header_references.len())
            .finish_non_exhaustive()
    }
}

impl SafeHttpClient {
    fn new(config: &McpServerConfig) -> Result<Self, McpError> {
        let timeout = Duration::from_secs(config.timeout_secs);
        let client = reqwest_13::Client::builder()
            .connect_timeout(timeout)
            .timeout(timeout)
            .redirect(reqwest_13::redirect::Policy::none())
            .no_proxy()
            .pool_max_idle_per_host(0)
            .build()
            .map_err(|_| McpError::InvalidConfig("invalid HTTP MCP client settings".into()))?;
        let mut header_references = Vec::with_capacity(config.headers.len());
        for (name, value) in &config.headers {
            let name = http::HeaderName::from_bytes(name.as_bytes()).map_err(|_| {
                McpError::InvalidConfig("invalid Streamable HTTP header name".into())
            })?;
            let environment = exact_environment_reference(value).ok_or_else(|| {
                McpError::InvalidConfig(
                    "Streamable HTTP header values must be exact ${ENV_VAR} references".into(),
                )
            })?;
            header_references.push((name, environment.into()));
        }
        let this = Self {
            client,
            header_references: Arc::new(header_references),
        };
        let _ = this.resolve_headers(HashMap::new())?;
        Ok(this)
    }

    fn resolve_headers(
        &self,
        mut generated: HashMap<http::HeaderName, http::HeaderValue>,
    ) -> Result<HashMap<http::HeaderName, http::HeaderValue>, McpError> {
        for (name, environment) in self.header_references.iter() {
            let value = crate::run_environment::var(environment).ok_or_else(|| {
                McpError::InvalidConfig(format!(
                    "Streamable HTTP credential environment variable {environment} is not set"
                ))
            })?;
            let value = http::HeaderValue::from_str(&value).map_err(|_| {
                McpError::InvalidConfig(format!(
                    "Streamable HTTP credential environment variable {environment} is not a valid header value"
                ))
            })?;
            generated.insert(name.clone(), value);
        }
        Ok(generated)
    }

    fn request_headers(
        &self,
        generated: HashMap<http::HeaderName, http::HeaderValue>,
    ) -> Result<
        HashMap<http::HeaderName, http::HeaderValue>,
        StreamableHttpError<SafeHttpClientError>,
    > {
        self.resolve_headers(generated)
            .map_err(|_| StreamableHttpError::Client(SafeHttpClientError))
    }

    async fn bounded_json_body(
        response: reqwest_13::Response,
    ) -> Result<Vec<u8>, StreamableHttpError<SafeHttpClientError>> {
        if response
            .content_length()
            .is_some_and(|length| length > MAX_HTTP_JSON_BYTES as u64)
        {
            return Err(StreamableHttpError::Client(SafeHttpClientError));
        }
        let mut bytes = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| StreamableHttpError::Client(SafeHttpClientError))?;
            if bytes.len().saturating_add(chunk.len()) > MAX_HTTP_JSON_BYTES {
                return Err(StreamableHttpError::Client(SafeHttpClientError));
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    }
}

fn map_http_transport_error(
    error: StreamableHttpError<reqwest_13::Error>,
) -> StreamableHttpError<SafeHttpClientError> {
    match error {
        StreamableHttpError::SessionExpired => StreamableHttpError::SessionExpired,
        StreamableHttpError::ServerDoesNotSupportSse => {
            StreamableHttpError::ServerDoesNotSupportSse
        }
        StreamableHttpError::ServerDoesNotSupportDeleteSession => {
            StreamableHttpError::ServerDoesNotSupportDeleteSession
        }
        StreamableHttpError::UnexpectedEndOfStream => StreamableHttpError::UnexpectedEndOfStream,
        StreamableHttpError::AuthRequired(error) => StreamableHttpError::AuthRequired(error),
        StreamableHttpError::InsufficientScope(error) => {
            StreamableHttpError::InsufficientScope(error)
        }
        StreamableHttpError::Auth(error) => StreamableHttpError::Auth(error),
        _ => StreamableHttpError::Client(SafeHttpClientError),
    }
}

impl StreamableHttpClient for SafeHttpClient {
    type Error = SafeHttpClientError;

    fn post_message(
        &self,
        uri: Arc<str>,
        message: ClientJsonRpcMessage,
        session_id: Option<Arc<str>>,
        auth_header: Option<String>,
        custom_headers: HashMap<http::HeaderName, http::HeaderValue>,
    ) -> impl std::future::Future<
        Output = Result<StreamableHttpPostResponse, StreamableHttpError<Self::Error>>,
    > + Send
           + '_ {
        self.post_message_with_max_sse_event_size(
            uri,
            message,
            session_id,
            auth_header,
            custom_headers,
            MAX_HTTP_EVENT_BYTES,
        )
    }

    async fn post_message_with_max_sse_event_size(
        &self,
        uri: Arc<str>,
        message: ClientJsonRpcMessage,
        session_id: Option<Arc<str>>,
        auth_header: Option<String>,
        custom_headers: HashMap<http::HeaderName, http::HeaderValue>,
        max_sse_event_size: usize,
    ) -> Result<StreamableHttpPostResponse, StreamableHttpError<Self::Error>> {
        let headers = self.request_headers(custom_headers)?;
        let mut request = self
            .client
            .post(uri.as_ref())
            .header(http::header::ACCEPT, "text/event-stream, application/json");
        if let Some(auth_header) = auth_header {
            request = request.bearer_auth(auth_header);
        }
        for (name, value) in headers {
            request = request.header(name, value);
        }
        let session_was_attached = session_id.is_some();
        if let Some(session_id) = session_id {
            request = request.header("mcp-session-id", session_id.as_ref());
        }
        let response = request
            .json(&message)
            .send()
            .await
            .map_err(|_| StreamableHttpError::Client(SafeHttpClientError))?;
        let status = response.status();
        if status == reqwest_13::StatusCode::UNAUTHORIZED {
            let challenge = bounded_auth_challenge(&response);
            return Err(StreamableHttpError::AuthRequired(AuthRequiredError::new(
                challenge,
            )));
        }
        if status == reqwest_13::StatusCode::FORBIDDEN {
            let challenge = bounded_auth_challenge(&response);
            return Err(StreamableHttpError::InsufficientScope(
                InsufficientScopeError::new(challenge, None),
            ));
        }
        if matches!(
            status,
            reqwest_13::StatusCode::ACCEPTED | reqwest_13::StatusCode::NO_CONTENT
        ) {
            return Ok(StreamableHttpPostResponse::Accepted);
        }
        if status == reqwest_13::StatusCode::NOT_FOUND && session_was_attached {
            return Err(StreamableHttpError::SessionExpired);
        }
        if !status.is_success() {
            return Err(StreamableHttpError::Client(SafeHttpClientError));
        }

        let content_type = response
            .headers()
            .get(http::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let content_length = response.content_length();
        let response_session = response
            .headers()
            .get("mcp-session-id")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        if content_length == Some(0)
            && matches!(
                message,
                ClientJsonRpcMessage::Notification(_)
                    | ClientJsonRpcMessage::Response(_)
                    | ClientJsonRpcMessage::Error(_)
            )
        {
            return Ok(StreamableHttpPostResponse::Accepted);
        }

        match content_type.as_deref() {
            Some(value) if value.as_bytes().starts_with(b"application/json") => {
                let bytes = Self::bounded_json_body(response).await?;
                match serde_json::from_slice::<ServerJsonRpcMessage>(&bytes) {
                    Ok(message) => Ok(StreamableHttpPostResponse::Json(message, response_session)),
                    Err(_)
                        if matches!(
                            message,
                            ClientJsonRpcMessage::Notification(_)
                                | ClientJsonRpcMessage::Response(_)
                                | ClientJsonRpcMessage::Error(_)
                        ) =>
                    {
                        Ok(StreamableHttpPostResponse::Accepted)
                    }
                    Err(_) => Err(StreamableHttpError::Client(SafeHttpClientError)),
                }
            }
            Some(value) if value.as_bytes().starts_with(b"text/event-stream") => {
                let mut limiter = SseEventLimiter::new(max_sse_event_size);
                let stream = response.bytes_stream().map(move |chunk| {
                    let chunk = chunk.map_err(|_| SafeHttpBodyError::Source)?;
                    limiter.observe(&chunk)?;
                    Ok::<_, SafeHttpBodyError>(chunk)
                });
                let stream = sse_stream::SseStream::from_bytes_stream(stream).boxed();
                Ok(StreamableHttpPostResponse::Sse(stream, response_session))
            }
            _ => Err(StreamableHttpError::Client(SafeHttpClientError)),
        }
    }

    async fn delete_session(
        &self,
        uri: Arc<str>,
        session_id: Arc<str>,
        auth_header: Option<String>,
        custom_headers: HashMap<http::HeaderName, http::HeaderValue>,
    ) -> Result<(), StreamableHttpError<Self::Error>> {
        let headers = self.request_headers(custom_headers)?;
        <reqwest_13::Client as StreamableHttpClient>::delete_session(
            &self.client,
            uri,
            session_id,
            auth_header,
            headers,
        )
        .await
        .map_err(map_http_transport_error)
    }

    fn get_stream(
        &self,
        uri: Arc<str>,
        session_id: Option<Arc<str>>,
        last_event_id: Option<String>,
        auth_header: Option<String>,
        custom_headers: HashMap<http::HeaderName, http::HeaderValue>,
    ) -> impl std::future::Future<
        Output = Result<
            futures::stream::BoxStream<'static, Result<sse_stream::Sse, sse_stream::Error>>,
            StreamableHttpError<Self::Error>,
        >,
    > + Send
           + '_ {
        self.get_stream_with_max_sse_event_size(
            uri,
            session_id,
            last_event_id,
            auth_header,
            custom_headers,
            MAX_HTTP_EVENT_BYTES,
        )
    }

    async fn get_stream_with_max_sse_event_size(
        &self,
        uri: Arc<str>,
        session_id: Option<Arc<str>>,
        last_event_id: Option<String>,
        auth_header: Option<String>,
        custom_headers: HashMap<http::HeaderName, http::HeaderValue>,
        max_sse_event_size: usize,
    ) -> Result<
        futures::stream::BoxStream<'static, Result<sse_stream::Sse, sse_stream::Error>>,
        StreamableHttpError<Self::Error>,
    > {
        let headers = self.request_headers(custom_headers)?;
        <reqwest_13::Client as StreamableHttpClient>::get_stream_with_max_sse_event_size(
            &self.client,
            uri,
            session_id,
            last_event_id,
            auth_header,
            headers,
            max_sse_event_size,
        )
        .await
        .map_err(map_http_transport_error)
    }
}

fn bounded_auth_challenge(response: &reqwest_13::Response) -> String {
    const MAX_CHALLENGE_BYTES: usize = 8 * 1024;
    response
        .headers()
        .get(http::header::WWW_AUTHENTICATE)
        .and_then(|value| value.to_str().ok())
        .filter(|value| value.len() <= MAX_CHALLENGE_BYTES)
        .unwrap_or("Bearer")
        .to_string()
}

#[derive(Debug, Clone)]
struct RemoteTool {
    exposed_name: String,
    kind: RemoteToolKind,
    description: String,
    schema: serde_json::Value,
}

#[derive(Debug, Clone)]
enum RemoteToolKind {
    Tool(String),
    ReadResource,
}

struct McpManagerInner {
    definitions: BTreeMap<String, ServerDefinition>,
    running: HashMap<String, RunningServer>,
    failures: HashMap<String, String>,
    authorization_required: HashSet<String>,
    activated_tools: BTreeSet<String>,
}

#[derive(Clone)]
pub struct McpManager {
    inner: Arc<Mutex<McpManagerInner>>,
    workspace: Arc<std::sync::RwLock<WorkspaceContext>>,
    sandbox_mode: SandboxMode,
    allow_all_network: bool,
    network_grants: Arc<std::sync::Mutex<HashSet<String>>>,
    base_environment: BTreeMap<String, String>,
    project_trust: Option<ProjectMcpTrust>,
    active_profile: crate::runtime::ActiveProfile,
    oauth_secret_store: Arc<dyn crate::mcp_oauth::McpOAuthSecretStore>,
    browser_opener: Arc<dyn crate::mcp_oauth::BrowserOpener>,
    mechanistic_server: Option<String>,
    mechanistic_max_wait_seconds: u64,
    mechanistic_config: crate::config::MechanisticMcpConfig,
    profile_config_path: Option<PathBuf>,
    delegation_activity: crate::mechanistic_delegation::DelegationActivity,
    workflow_process_groups: Arc<std::sync::Mutex<HashMap<String, i32>>>,
}

impl std::fmt::Debug for McpManager {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("McpManager")
            .field("sandbox_mode", &self.sandbox_mode)
            .field("allow_all_network", &self.allow_all_network)
            .field("active_profile", &self.active_profile.label())
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct ProjectMcpFile {
    #[serde(default)]
    servers: HashMap<String, McpServerConfig>,
}

impl McpManager {
    pub(crate) fn with_profile_config_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.profile_config_path = Some(path.into());
        self
    }

    pub(crate) fn active_profile_label(&self) -> &str {
        self.active_profile.label()
    }

    pub(crate) fn active_profile(&self) -> crate::runtime::ActiveProfile {
        self.active_profile.clone()
    }

    /// Returns only resource metadata that is already present in a running
    /// server catalog. This never starts, refreshes, or waits for a server.
    pub fn discovered_resource_snapshot(&self, limit: usize) -> Vec<McpResourceInfo> {
        let Ok(inner) = self.inner.try_lock() else {
            return Vec::new();
        };
        let mut resources = inner
            .running
            .iter()
            .flat_map(|(server_name, server)| {
                server.resources.iter().map(|resource| McpResourceInfo {
                    server: server_name.clone(),
                    uri: resource.uri.clone(),
                    name: resource.name.clone(),
                    description: resource.description.clone(),
                    mime_type: resource.mime_type.clone(),
                })
            })
            .collect::<Vec<_>>();
        resources.sort_by(|left, right| {
            left.server
                .cmp(&right.server)
                .then_with(|| left.name.cmp(&right.name))
                .then_with(|| left.uri.cmp(&right.uri))
        });
        resources.dedup_by(|left, right| left.server == right.server && left.uri == right.uri);
        resources.truncate(limit.min(256));
        resources
    }

    pub fn load(
        config: &CaConfig,
        workspace: WorkspaceContext,
        sandbox_mode: SandboxMode,
        network_allowed: bool,
    ) -> Result<Self, McpError> {
        Self::load_with_oauth_store(
            config,
            workspace,
            sandbox_mode,
            network_allowed,
            crate::runtime::ActiveProfile::Default,
            crate::mcp_oauth::system_mcp_oauth_secret_store(),
        )
    }

    pub fn load_with_oauth_store(
        config: &CaConfig,
        workspace: WorkspaceContext,
        sandbox_mode: SandboxMode,
        network_allowed: bool,
        active_profile: crate::runtime::ActiveProfile,
        oauth_secret_store: Arc<dyn crate::mcp_oauth::McpOAuthSecretStore>,
    ) -> Result<Self, McpError> {
        Self::load_with_oauth_dependencies(
            config,
            workspace,
            sandbox_mode,
            network_allowed,
            active_profile,
            oauth_secret_store,
            crate::mcp_oauth::system_browser_opener(),
        )
    }

    pub fn load_with_oauth_dependencies(
        config: &CaConfig,
        workspace: WorkspaceContext,
        sandbox_mode: SandboxMode,
        network_allowed: bool,
        active_profile: crate::runtime::ActiveProfile,
        oauth_secret_store: Arc<dyn crate::mcp_oauth::McpOAuthSecretStore>,
        browser_opener: Arc<dyn crate::mcp_oauth::BrowserOpener>,
    ) -> Result<Self, McpError> {
        let mut definitions = BTreeMap::new();
        for (name, server) in &config.mcp.servers {
            validate_server_name(name)?;
            validate_server_config(server)?;
            definitions.insert(
                name.clone(),
                ServerDefinition {
                    name: name.clone(),
                    config: server.clone(),
                    origin: McpConfigOrigin::User,
                    trusted: true,
                    network_trusted: true,
                },
            );
        }

        let mut project_trust = None;
        if let Some(primary) = workspace.primary() {
            let primary_path = primary.path.canonicalize().map_err(|error| {
                McpError::InvalidConfig(format!("{}: {error}", primary.path.display()))
            })?;
            let project_file = primary_path.join(".ca/mcp.toml");
            if project_file.exists() {
                let metadata = std::fs::symlink_metadata(&project_file).map_err(|error| {
                    McpError::InvalidConfig(format!("{}: {error}", project_file.display()))
                })?;
                if metadata.file_type().is_symlink() || metadata.len() as usize > MAX_CONFIG_BYTES {
                    return Err(McpError::InvalidConfig(format!(
                        "unsafe project MCP config {}",
                        project_file.display()
                    )));
                }
                let canonical = project_file.canonicalize().map_err(|error| {
                    McpError::InvalidConfig(format!("{}: {error}", project_file.display()))
                })?;
                if !canonical.starts_with(&primary_path) {
                    return Err(McpError::InvalidConfig(
                        "project MCP config escapes the workspace".into(),
                    ));
                }
                let raw = std::fs::read(&canonical).map_err(|error| {
                    McpError::InvalidConfig(format!("{}: {error}", canonical.display()))
                })?;
                let hash = hex_digest(&raw);
                project_trust = Some(ProjectMcpTrust {
                    workspace: primary_path.clone(),
                    sha256: hash.clone(),
                    network: false,
                });
                let trust = matching_trust(&config.mcp.trusted_projects, &primary_path, &hash);
                let text = std::str::from_utf8(&raw).map_err(|_| {
                    McpError::InvalidConfig("project MCP config is not UTF-8".into())
                })?;
                let mut project: ProjectMcpFile = toml::from_str(text)
                    .map_err(|error| McpError::InvalidConfig(error.to_string()))?;
                for server in project.servers.values_mut() {
                    for value in server.env.values_mut() {
                        *value = crate::config::expand_env_vars(value);
                    }
                }
                for (name, server) in project.servers {
                    validate_server_name(&name)?;
                    validate_server_config(&server)?;
                    if server.oauth.enabled {
                        return Err(McpError::InvalidConfig(
                            "project MCP files cannot configure OAuth; move that server to the active profile"
                                .into(),
                        ));
                    }
                    if definitions.contains_key(&name) {
                        continue;
                    }
                    definitions.insert(
                        name.clone(),
                        ServerDefinition {
                            name,
                            config: server,
                            origin: McpConfigOrigin::Project,
                            trusted: trust.is_some(),
                            network_trusted: trust.is_some_and(|trust| trust.network),
                        },
                    );
                }
            }
        }

        Ok(Self {
            inner: Arc::new(Mutex::new(McpManagerInner {
                definitions,
                running: HashMap::new(),
                failures: HashMap::new(),
                authorization_required: HashSet::new(),
                activated_tools: BTreeSet::new(),
            })),
            workspace: Arc::new(std::sync::RwLock::new(workspace)),
            sandbox_mode,
            allow_all_network: network_allowed,
            network_grants: Arc::new(std::sync::Mutex::new(HashSet::new())),
            base_environment: filtered_environment(),
            project_trust,
            active_profile,
            oauth_secret_store,
            browser_opener,
            mechanistic_server: config
                .mcp
                .mechanistic
                .enabled
                .then(|| config.mcp.mechanistic.server.clone()),
            mechanistic_max_wait_seconds: config.mcp.mechanistic.max_wait_seconds,
            mechanistic_config: config.mcp.mechanistic.clone(),
            profile_config_path: None,
            delegation_activity: crate::mechanistic_delegation::DelegationActivity::default(),
            workflow_process_groups: Arc::new(std::sync::Mutex::new(HashMap::new())),
        })
    }

    pub fn project_trust_record(&self, network: bool) -> Option<ProjectMcpTrust> {
        self.project_trust.clone().map(|mut trust| {
            trust.network = network;
            trust
        })
    }

    pub fn update_workspace(&self, workspace: WorkspaceContext) {
        *self
            .workspace
            .write()
            .unwrap_or_else(|poison| poison.into_inner()) = workspace;
    }

    pub async fn apply_project_trust(&self, network: bool) {
        let mut inner = self.inner.lock().await;
        for definition in inner.definitions.values_mut() {
            if definition.origin == McpConfigOrigin::Project {
                definition.trusted = true;
                definition.network_trusted = network;
                if network {
                    self.network_grants
                        .lock()
                        .unwrap_or_else(|poison| poison.into_inner())
                        .insert(definition.name.clone());
                }
            }
        }
    }

    pub async fn grant_network_for_server(&self, server: &str) -> Result<(), McpError> {
        let inner = self.inner.lock().await;
        let definition = inner
            .definitions
            .get(server)
            .ok_or_else(|| McpError::UnknownServer(server.into()))?;
        if !definition.trusted || !definition.network_trusted {
            return Err(McpError::NetworkApprovalRequired(server.into()));
        }
        self.network_grants
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .insert(server.into());
        Ok(())
    }

    fn network_allowed_for(&self, server: &str) -> bool {
        self.allow_all_network
            || self
                .network_grants
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .contains(server)
    }

    fn definition_search_eligibility(
        &self,
        definition: &ServerDefinition,
    ) -> Result<(), &'static str> {
        if self.mechanistic_server.as_deref() == Some(definition.name.as_str()) {
            return Err("server is reserved for the delegate_mechanistic facade");
        }
        if !definition.config.enabled {
            return Err("server is disabled");
        }
        if !definition.trusted {
            return Err("project trust is required");
        }
        if definition.config.network
            && (!definition.network_trusted || !self.network_allowed_for(&definition.name))
        {
            return Err("a distinct network grant is required");
        }
        Ok(())
    }

    async fn invalidate_server_activations(&self, server_name: &str) {
        let mut inner = self.inner.lock().await;
        let names = inner
            .running
            .get(server_name)
            .map(|server| {
                server
                    .tools
                    .iter()
                    .map(|tool| tool.exposed_name.clone())
                    .collect::<HashSet<_>>()
            })
            .unwrap_or_default();
        inner.activated_tools.retain(|name| !names.contains(name));
    }

    pub async fn statuses(&self) -> Vec<McpServerStatus> {
        let inner = self.inner.lock().await;
        inner
            .definitions
            .values()
            .map(|definition| {
                let running = inner.running.get(&definition.name);
                let state = if !definition.trusted {
                    McpServerState::Untrusted
                } else if definition.config.network
                    && (!definition.network_trusted || !self.network_allowed_for(&definition.name))
                {
                    McpServerState::NetworkApprovalRequired
                } else if running.is_some() {
                    McpServerState::Running
                } else if inner.authorization_required.contains(&definition.name) {
                    McpServerState::AuthorizationRequired
                } else if inner.failures.contains_key(&definition.name) {
                    McpServerState::Failed
                } else {
                    McpServerState::Stopped
                };
                McpServerStatus {
                    name: definition.name.clone(),
                    origin: definition.origin,
                    state,
                    tool_count: running.map_or(0, |server| server.tools.len()),
                    active_tool_count: running.map_or(0, |server| {
                        server
                            .tools
                            .iter()
                            .filter(|tool| inner.activated_tools.contains(&tool.exposed_name))
                            .count()
                    }),
                    resource_count: running.map_or(0, |server| server.resources.len()),
                    detail: inner.failures.get(&definition.name).cloned(),
                }
            })
            .collect()
    }

    pub async fn catalog(&self) -> McpCatalog {
        let names: Vec<String> = {
            let inner = self.inner.lock().await;
            inner
                .definitions
                .values()
                .filter(|server| server.config.enabled)
                .map(|server| server.name.clone())
                .collect()
        };
        let mut diagnostics = Vec::new();
        for name in names {
            if let Err(error) = self.ensure_started(&name).await {
                diagnostics.push(error.to_string());
                self.inner
                    .lock()
                    .await
                    .failures
                    .insert(name, error.to_string());
            }
        }
        let inner = self.inner.lock().await;
        let mut catalog = catalog_snapshot(&inner, false);
        catalog.diagnostics.splice(0..0, diagnostics);
        catalog
    }

    pub async fn active_catalog(&self) -> McpCatalog {
        let stale = {
            let inner = self.inner.lock().await;
            inner
                .running
                .iter()
                .filter(|(_, server)| {
                    server.stale_tools.load(Ordering::Acquire)
                        || server.stale_resources.load(Ordering::Acquire)
                })
                .map(|(name, _)| name.clone())
                .collect::<Vec<_>>()
        };
        let mut diagnostics = Vec::new();
        for name in stale {
            if let Err(error) = self.ensure_started(&name).await {
                self.invalidate_server_activations(&name).await;
                diagnostics.push(safe_search_failure(&name, &error));
            }
        }
        let inner = self.inner.lock().await;
        let mut catalog = catalog_snapshot(&inner, true);
        catalog.diagnostics.splice(0..0, diagnostics);
        catalog
    }

    pub async fn search_tools(
        &self,
        query: &str,
        server: Option<&str>,
        limit: usize,
    ) -> crate::agent::registry::ToolExecutionResult {
        use crate::agent::registry::ToolExecutionResult;

        let query = query.trim();
        if query.is_empty()
            || query.len() > MAX_SEARCH_QUERY_BYTES
            || query.chars().any(char::is_control)
        {
            return ToolExecutionResult::Failure(
                "MCP tool search requires a non-empty bounded text query".into(),
            );
        }
        if limit == 0 {
            return ToolExecutionResult::Failure("MCP tool search limit must be positive".into());
        }
        let limit = limit.min(MAX_SEARCH_RESULTS);
        let query_folded = query.to_ascii_lowercase();
        let query_terms = query_folded.split_whitespace().collect::<Vec<_>>();
        let (mut eligible, ineligible, exact_error) = {
            let inner = self.inner.lock().await;
            if let Some(server) = server {
                match inner.definitions.get(server) {
                    None => (Vec::new(), 0, Some("Unknown MCP server".to_string())),
                    Some(definition) => match self.definition_search_eligibility(definition) {
                        Ok(()) => (vec![server.to_string()], 0, None),
                        Err(reason) => (Vec::new(), 1, Some(reason.to_string())),
                    },
                }
            } else {
                let mut eligible = inner
                    .definitions
                    .values()
                    .filter(|definition| self.definition_search_eligibility(definition).is_ok())
                    .map(|definition| definition.name.clone())
                    .collect::<Vec<_>>();
                let ineligible = inner.definitions.len().saturating_sub(eligible.len());
                let matching_names = eligible
                    .iter()
                    .filter(|name| {
                        let folded = name.to_ascii_lowercase();
                        query_terms.iter().all(|term| folded.contains(term))
                    })
                    .cloned()
                    .collect::<Vec<_>>();
                if !matching_names.is_empty() {
                    eligible = matching_names;
                }
                (eligible, ineligible, None)
            }
        };
        if let Some(error) = exact_error {
            return ToolExecutionResult::Failure(format!(
                "MCP tool search could not inspect {}: {error}",
                safe_server_label(server.unwrap_or_default())
            ));
        }
        let omitted_servers = eligible.len().saturating_sub(MAX_SEARCH_SERVERS);
        eligible.truncate(MAX_SEARCH_SERVERS);

        let starts = futures::stream::iter(eligible.iter().cloned().map(|name| {
            let manager = self.clone();
            async move {
                let result = manager.ensure_started(&name).await;
                (name, result)
            }
        }))
        .buffer_unordered(MAX_SEARCH_CONCURRENCY)
        .collect::<Vec<_>>()
        .await;
        let mut failures = starts
            .iter()
            .filter_map(|(name, result)| {
                result
                    .as_ref()
                    .err()
                    .map(|error| (name.clone(), safe_search_failure(name, error)))
            })
            .collect::<Vec<_>>();
        failures.sort_by(|left, right| left.0.cmp(&right.0));

        let mut matches = {
            let inner = self.inner.lock().await;
            eligible
                .iter()
                .filter_map(|server_name| {
                    inner
                        .running
                        .get(server_name)
                        .map(|running| (server_name, running))
                })
                .flat_map(|(server_name, running)| {
                    running.tools.iter().filter_map(|tool| {
                        let haystack = format!(
                            "{} {} {}",
                            server_name.to_ascii_lowercase(),
                            tool.exposed_name.to_ascii_lowercase(),
                            tool.description.to_ascii_lowercase()
                        );
                        query_terms
                            .iter()
                            .all(|term| haystack.contains(term))
                            .then(|| (tool.exposed_name.clone(), server_name.clone()))
                    })
                })
                .collect::<Vec<_>>()
        };
        matches.sort();
        matches.dedup();
        let omitted_matches = matches.len().saturating_sub(limit);
        matches.truncate(limit);

        if !matches.is_empty() {
            let activation = matches
                .iter()
                .map(|(name, _)| name.clone())
                .collect::<BTreeSet<_>>();
            let mut inner = self.inner.lock().await;
            let mut proposed = inner.activated_tools.clone();
            proposed.extend(activation);
            if build_registry(&inner, Some(&proposed)).is_err() {
                return ToolExecutionResult::Failure(
                    "Matching MCP schemas could not be activated safely".into(),
                );
            }
            inner.activated_tools = proposed;
        }

        let mut lines = if matches.is_empty() {
            vec![format!("No MCP tools matched {query:?}.")]
        } else {
            vec![format!(
                "Activated {} MCP tool{} for subsequent turns:",
                matches.len(),
                if matches.len() == 1 { "" } else { "s" }
            )]
        };
        lines.extend(matches.iter().map(|(name, server_name)| {
            format!(
                "- {} [{}]",
                crate::terminal_text::sanitize_terminal_text(name),
                safe_server_label(server_name)
            )
        }));
        if omitted_matches > 0 {
            lines.push(format!(
                "{omitted_matches} additional match{} omitted; narrow the query.",
                if omitted_matches == 1 { "" } else { "es" }
            ));
        }
        if omitted_servers > 0 {
            lines.push(format!(
                "{omitted_servers} eligible server{} not inspected; retry with an exact server ID.",
                if omitted_servers == 1 {
                    " was"
                } else {
                    "s were"
                }
            ));
        }
        if ineligible > 0 {
            lines.push(format!(
                "{ineligible} configured server{} skipped because it is disabled or lacks trust, network, or authorization readiness.",
                if ineligible == 1 { " was" } else { "s were" }
            ));
        }
        lines.extend(failures.into_iter().map(|(_, failure)| failure));
        ToolExecutionResult::Success(bounded_search_output(&lines.join("\n")))
    }

    pub async fn call_tool(
        &self,
        exposed_name: &str,
        arguments: serde_json::Value,
    ) -> Result<crate::agent::registry::ToolExecutionResult, McpError> {
        let (server_name, kind, timeout, peer, transport, secret_values) = {
            let inner = self.inner.lock().await;
            let found = inner.running.iter().find_map(|(server_name, server)| {
                server
                    .tools
                    .iter()
                    .find(|tool| tool.exposed_name == exposed_name)
                    .map(|tool| {
                        (
                            server_name.clone(),
                            tool.kind.clone(),
                            inner.definitions[server_name].config.timeout_secs,
                            server.service.peer().clone(),
                            server.transport,
                            secret_values_for_config(&inner.definitions[server_name].config),
                        )
                    })
            });
            found.ok_or_else(|| McpError::UnknownServer(exposed_name.into()))?
        };
        if matches!(kind, RemoteToolKind::ReadResource) {
            let uri = arguments
                .get("uri")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| McpError::Protocol {
                    server: server_name.clone(),
                    detail: "resource uri is required".into(),
                })?;
            return self
                .read_resource(&server_name, uri)
                .await
                .map(crate::agent::registry::ToolExecutionResult::Success);
        }
        let RemoteToolKind::Tool(remote_name) = kind else {
            unreachable!("resource branch returned above")
        };
        let mut arguments = arguments
            .as_object()
            .cloned()
            .ok_or_else(|| McpError::Protocol {
                server: server_name.clone(),
                detail: "tool arguments must be an object".into(),
            })?;
        let request = peer.call_tool(
            CallToolRequestParams::new(remote_name).with_arguments(std::mem::take(&mut arguments)),
        );
        let result = tokio::time::timeout(Duration::from_secs(timeout), request)
            .await
            .map_err(|_| McpError::Timeout {
                server: server_name.clone(),
                operation: "tool call",
            })?
            .map_err(|error| McpError::Protocol {
                server: server_name.clone(),
                detail: safe_protocol_detail(transport, "tool call", &error),
            })?;
        let text = redact_values(
            &bounded_tool_text(&result.content, result.structured_content.as_ref())?,
            &secret_values,
        );
        if result.is_error == Some(true) {
            Ok(crate::agent::registry::ToolExecutionResult::Failure(text))
        } else {
            Ok(crate::agent::registry::ToolExecutionResult::Success(text))
        }
    }

    /// Call one exact tool on one exact configured server for a host-rendered
    /// workflow. This never adds the tool to the model-visible activation set.
    pub async fn call_host_tool_structured(
        &self,
        server_name: &str,
        remote_name: &str,
        arguments: serde_json::Value,
    ) -> Result<serde_json::Value, McpError> {
        validate_server_name(server_name)?;
        if remote_name.is_empty() || remote_name.len() > 128 {
            return Err(McpError::Protocol {
                server: server_name.to_string(),
                detail: "host tool name is invalid".into(),
            });
        }
        self.ensure_started(server_name).await?;
        let (timeout, peer, transport, secret_values) = {
            let inner = self.inner.lock().await;
            let definition = inner
                .definitions
                .get(server_name)
                .ok_or_else(|| McpError::UnknownServer(server_name.into()))?;
            let running = inner
                .running
                .get(server_name)
                .ok_or_else(|| McpError::Start {
                    server: server_name.into(),
                    detail: "server did not remain available".into(),
                })?;
            let exact = running.tools.iter().any(
                |tool| matches!(&tool.kind, RemoteToolKind::Tool(name) if name == remote_name),
            );
            if !exact {
                return Err(McpError::Protocol {
                    server: server_name.into(),
                    detail: "required host tool is unavailable".into(),
                });
            }
            (
                definition.config.timeout_secs,
                running.service.peer().clone(),
                running.transport,
                secret_values_for_config(&definition.config),
            )
        };
        let mut arguments = arguments
            .as_object()
            .cloned()
            .ok_or_else(|| McpError::Protocol {
                server: server_name.into(),
                detail: "host tool arguments must be an object".into(),
            })?;
        let request = peer.call_tool(
            CallToolRequestParams::new(remote_name.to_string())
                .with_arguments(std::mem::take(&mut arguments)),
        );
        let result = tokio::time::timeout(Duration::from_secs(timeout), request)
            .await
            .map_err(|_| McpError::Timeout {
                server: server_name.into(),
                operation: "host tool call",
            })?
            .map_err(|error| McpError::Protocol {
                server: server_name.into(),
                detail: safe_protocol_detail(transport, "host tool call", &error),
            })?;
        let error_text = (result.is_error == Some(true))
            .then(|| bounded_tool_text(&result.content, result.structured_content.as_ref()))
            .transpose()?
            .map(|text| redact_values(&text, &secret_values));
        let structured = result
            .structured_content
            .map(|structured| {
                sanitize_host_structured_content(server_name, &structured, &secret_values)
            })
            .transpose()?;
        if result.is_error == Some(true) {
            return Err(remote_tool_error(
                server_name,
                structured.as_ref(),
                error_text
                    .as_deref()
                    .unwrap_or("remote tool reported an error"),
            ));
        }
        structured.ok_or_else(|| McpError::Protocol {
            server: server_name.into(),
            detail: "host tool omitted structured content".into(),
        })
    }

    /// Lazily start and verify the exact, non-operating codex-team delegate
    /// catalog selected by the active profile. This never activates raw MCP
    /// tools for the model-visible catalog.
    pub async fn validate_mechanistic_delegate_server(
        &self,
        server_name: &str,
    ) -> Result<(), McpError> {
        if self.mechanistic_server.as_deref() != Some(server_name) {
            return Err(McpError::InvalidConfig(
                "requested MCP server is not the profile mechanistic server".into(),
            ));
        }
        self.ensure_started(server_name).await?;
        let inner = self.inner.lock().await;
        let definition = inner
            .definitions
            .get(server_name)
            .ok_or_else(|| McpError::UnknownServer(server_name.into()))?;
        if definition.origin != McpConfigOrigin::User
            || !definition.config.enabled
            || definition.config.transport != McpTransport::Stdio
            || definition.config.network
        {
            return Err(McpError::InvalidConfig(
                "mechanistic delegation requires enabled profile-local stdio with network = false"
                    .into(),
            ));
        }
        let running = inner
            .running
            .get(server_name)
            .ok_or_else(|| McpError::Start {
                server: server_name.into(),
                detail: "server did not remain available".into(),
            })?;
        if !running.resources.is_empty() {
            return Err(McpError::InvalidConfig(
                "mechanistic delegation server must advertise zero resources".into(),
            ));
        }
        const EXPECTED_DELEGATE_TOOLS: [&str; 15] = [
            "delegation_cancel",
            "delegation_feedback",
            "delegation_start",
            "delegation_status",
            "doctor",
            "recipe_list",
            "recipe_preview",
            "repo_query",
            "self_improve_deployments",
            "self_improve_evaluations",
            "self_improve_objectives",
            "self_improve_status",
            "self_improve_traces",
            "status",
            "step_status",
        ];
        let advertised = running
            .tools
            .iter()
            .filter_map(|tool| match &tool.kind {
                RemoteToolKind::Tool(name) => Some(name.as_str()),
                RemoteToolKind::ReadResource => None,
            })
            .collect::<BTreeSet<_>>();
        let expected = EXPECTED_DELEGATE_TOOLS.into_iter().collect::<BTreeSet<_>>();
        if advertised != expected {
            return Err(McpError::InvalidConfig(format!(
                "mechanistic delegation requires the exact 15-tool delegate catalog; server advertised {} tools",
                advertised.len()
            )));
        }
        Ok(())
    }

    /// Restart only the profile-selected local mechanistic delegate. This is
    /// host-owned recovery and never activates its raw MCP catalog for a model.
    pub async fn restart_mechanistic_delegate_server(
        &self,
        server_name: &str,
    ) -> Result<(), McpError> {
        if self.mechanistic_server.as_deref() != Some(server_name) {
            return Err(McpError::InvalidConfig(
                "requested MCP server is not the profile mechanistic server".into(),
            ));
        }
        if self.inner.lock().await.running.contains_key(server_name) {
            self.close_server(server_name).await?;
        }
        self.inner.lock().await.failures.remove(server_name);
        self.validate_mechanistic_delegate_server(server_name).await
    }

    pub fn configured_mechanistic_server(&self) -> Option<&str> {
        self.mechanistic_server.as_deref()
    }

    pub fn mechanistic_max_wait_seconds(&self) -> u64 {
        self.mechanistic_max_wait_seconds
    }

    pub fn mechanistic_config(&self) -> &crate::config::MechanisticMcpConfig {
        &self.mechanistic_config
    }

    /// Execute the separate typed codex-team remote protocol. The repository
    /// delegate remains network-disabled; this owned child receives a bounded
    /// network sandbox only after CA's ordinary tool approval has resolved.
    pub async fn execute_remote_automation(
        &self,
        request: &codex_team_protocol::RemoteAutomationRequestV1,
        network_allowed: bool,
    ) -> Result<codex_team_protocol::RemoteAutomationReceiptV1, McpError> {
        codex_team_protocol::validate_remote_automation_request(request)
            .map_err(|error| McpError::InvalidConfig(error.to_string()))?;
        if !self.mechanistic_config.remote.enabled {
            return Err(McpError::InvalidConfig(
                "remote mechanistic automation is disabled".into(),
            ));
        }
        let server_name = self
            .mechanistic_server
            .as_deref()
            .ok_or_else(|| McpError::InvalidConfig("mechanistic delegation is disabled".into()))?;
        self.validate_mechanistic_delegate_server(server_name)
            .await?;
        let executable = {
            let inner = self.inner.lock().await;
            let definition = inner
                .definitions
                .get(server_name)
                .cloned()
                .ok_or_else(|| McpError::UnknownServer(server_name.into()))?;
            resolve_executable(&definition.config.command, &self.base_environment).ok_or_else(
                || McpError::Start {
                    server: server_name.into(),
                    detail: "configured codex-team executable was not found".into(),
                },
            )?
        };
        let config_path = self.mechanistic_config.remote.config.clone();
        let temporary_directory = tempfile::tempdir().map_err(|error| McpError::Start {
            server: server_name.into(),
            detail: error.to_string(),
        })?;
        let workspace = self
            .workspace
            .read()
            .unwrap_or_else(|poison| poison.into_inner())
            .clone();
        let prepared = SandboxBackend::new(self.sandbox_mode)
            .prepare_process(
                &executable,
                &[
                    "remote".to_string(),
                    "execute".to_string(),
                    "--config".to_string(),
                    config_path.to_string_lossy().into_owned(),
                    "--json".to_string(),
                ],
                &SandboxPolicy {
                    roots: workspace
                        .roots()
                        .iter()
                        .map(|root| root.path.clone())
                        .collect(),
                    read_only_roots: vec![
                        executable
                            .canonicalize()
                            .unwrap_or_else(|_| executable.clone()),
                        config_path.clone(),
                    ],
                    temporary_directory: temporary_directory.path().to_path_buf(),
                    network_allowed,
                    hide_other_processes: false,
                },
            )
            .map_err(|error| McpError::Start {
                server: server_name.into(),
                detail: error.to_string(),
            })?;
        let mut environment = self.base_environment.clone();
        if network_allowed {
            environment.extend(filtered_server_environment(
                &self.mechanistic_config.remote.env,
            )?);
        }
        let mut command = tokio::process::Command::new(prepared.program);
        command
            .args(prepared.arguments)
            .current_dir(workspace.cwd())
            .env_clear()
            .envs(environment)
            .env("TMPDIR", temporary_directory.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        crate::run_environment::apply_tokio(&mut command);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.as_std_mut().process_group(0);
        }
        let mut child = command.spawn().map_err(|error| McpError::Start {
            server: server_name.into(),
            detail: error.to_string(),
        })?;
        let mut process_group_guard =
            OwnedProcessGroupGuard::new(child.id().and_then(|id| i32::try_from(id).ok()));
        let input = serde_json::to_vec(request).map_err(|error| McpError::Protocol {
            server: server_name.into(),
            detail: error.to_string(),
        })?;
        let mut stdin = child.stdin.take().ok_or_else(|| McpError::Start {
            server: server_name.into(),
            detail: "remote automation child omitted stdin".into(),
        })?;
        stdin
            .write_all(&input)
            .await
            .map_err(|error| McpError::Protocol {
                server: server_name.into(),
                detail: error.to_string(),
            })?;
        drop(stdin);
        let output = tokio::time::timeout(
            Duration::from_secs(self.mechanistic_config.remote.max_wait_seconds),
            child.wait_with_output(),
        )
        .await
        .map_err(|_| McpError::Timeout {
            server: server_name.into(),
            operation: "remote automation",
        })?
        .map_err(|error| McpError::Protocol {
            server: server_name.into(),
            detail: error.to_string(),
        })?;
        process_group_guard.disarm();
        if output.stdout.len() > MAX_OUTPUT_BYTES || output.stderr.len() > MAX_STDERR_BYTES {
            return Err(McpError::Protocol {
                server: server_name.into(),
                detail: "remote automation output exceeded its bound".into(),
            });
        }
        if !output.status.success() {
            return Err(McpError::Protocol {
                server: server_name.into(),
                detail: "typed remote automation child failed".into(),
            });
        }
        serde_json::from_slice(&output.stdout).map_err(|_| McpError::Protocol {
            server: server_name.into(),
            detail: "typed remote automation child returned a malformed receipt".into(),
        })
    }

    /// Preview or apply one typed automation setup. This subprocess has no
    /// network access and receives the active profile and repository paths from
    /// CA, never from model arguments.
    pub async fn execute_automation_setup(
        &self,
        request: &codex_team_protocol::AutomationSetupRequestV1,
    ) -> Result<codex_team_protocol::AutomationSetupReceiptV1, McpError> {
        codex_team_protocol::validate_automation_setup_request(request)
            .map_err(|error| McpError::InvalidConfig(error.to_string()))?;
        if matches!(self.active_profile, crate::runtime::ActiveProfile::Bare) {
            return Err(McpError::InvalidConfig(
                "automation setup is unavailable in bare mode".into(),
            ));
        }
        let profile_config = self.profile_config_path.clone().ok_or_else(|| {
            McpError::InvalidConfig(
                "active profile path is unavailable for automation setup".into(),
            )
        })?;
        let server_name = self
            .mechanistic_server
            .as_deref()
            .ok_or_else(|| McpError::InvalidConfig("mechanistic delegation is disabled".into()))?;
        let executable = {
            let inner = self.inner.lock().await;
            let definition = inner
                .definitions
                .get(server_name)
                .cloned()
                .ok_or_else(|| McpError::UnknownServer(server_name.into()))?;
            resolve_executable(&definition.config.command, &self.base_environment).ok_or_else(
                || McpError::Start {
                    server: server_name.into(),
                    detail: "configured codex-team executable was not found".into(),
                },
            )?
        };
        let workspace = self
            .workspace
            .read()
            .unwrap_or_else(|poison| poison.into_inner())
            .clone();
        let repository = workspace
            .primary()
            .map(|root| root.path.clone())
            .ok_or_else(|| {
                McpError::InvalidConfig("automation setup requires a pinned repository".into())
            })?;
        let repository = repository.canonicalize().map_err(|error| {
            McpError::InvalidConfig(format!("pinned repository is unavailable: {error}"))
        })?;
        let profile_parent = profile_config
            .parent()
            .ok_or_else(|| McpError::InvalidConfig("active profile path has no parent".into()))?
            .to_path_buf();
        let writable_root = if self.active_profile_label() == "default" {
            profile_parent.clone()
        } else {
            profile_parent
                .parent()
                .ok_or_else(|| {
                    McpError::InvalidConfig("named profile path has no config root".into())
                })?
                .to_path_buf()
        };
        let temporary_directory = tempfile::tempdir().map_err(|error| McpError::Start {
            server: server_name.into(),
            detail: error.to_string(),
        })?;
        let arguments = vec![
            "automation".to_string(),
            "setup".to_string(),
            "--profile-config".to_string(),
            profile_config.to_string_lossy().into_owned(),
            "--profile".to_string(),
            self.active_profile_label().to_string(),
            "--repo".to_string(),
            repository.to_string_lossy().into_owned(),
            "--json".to_string(),
        ];
        let prepared = SandboxBackend::new(self.sandbox_mode)
            .prepare_process(
                &executable,
                &arguments,
                &SandboxPolicy {
                    roots: vec![writable_root],
                    read_only_roots: vec![
                        executable
                            .canonicalize()
                            .unwrap_or_else(|_| executable.clone()),
                        repository.clone(),
                    ],
                    temporary_directory: temporary_directory.path().to_path_buf(),
                    network_allowed: false,
                    hide_other_processes: true,
                },
            )
            .map_err(|error| McpError::Start {
                server: server_name.into(),
                detail: error.to_string(),
            })?;
        let mut command = tokio::process::Command::new(prepared.program);
        command
            .args(prepared.arguments)
            .current_dir(&repository)
            .env_clear()
            .envs(self.base_environment.clone())
            .env("TMPDIR", temporary_directory.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        crate::run_environment::apply_tokio(&mut command);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.as_std_mut().process_group(0);
        }
        let mut child = command.spawn().map_err(|error| McpError::Start {
            server: server_name.into(),
            detail: error.to_string(),
        })?;
        let mut process_group_guard =
            OwnedProcessGroupGuard::new(child.id().and_then(|id| i32::try_from(id).ok()));
        let input = serde_json::to_vec(request).map_err(|error| McpError::Protocol {
            server: server_name.into(),
            detail: error.to_string(),
        })?;
        let mut stdin = child.stdin.take().ok_or_else(|| McpError::Start {
            server: server_name.into(),
            detail: "automation setup child omitted stdin".into(),
        })?;
        stdin
            .write_all(&input)
            .await
            .map_err(|error| McpError::Protocol {
                server: server_name.into(),
                detail: error.to_string(),
            })?;
        drop(stdin);
        let output = tokio::time::timeout(Duration::from_secs(30), child.wait_with_output())
            .await
            .map_err(|_| McpError::Timeout {
                server: server_name.into(),
                operation: "automation setup",
            })?
            .map_err(|error| McpError::Protocol {
                server: server_name.into(),
                detail: error.to_string(),
            })?;
        process_group_guard.disarm();
        if output.stdout.len() > MAX_OUTPUT_BYTES || output.stderr.len() > MAX_STDERR_BYTES {
            return Err(McpError::Protocol {
                server: server_name.into(),
                detail: "automation setup output exceeded its bound".into(),
            });
        }
        if !output.status.success() {
            let detail = crate::terminal_text::sanitize_terminal_text(&String::from_utf8_lossy(
                &output.stderr,
            ));
            return Err(McpError::Protocol {
                server: server_name.into(),
                detail: if detail.trim().is_empty() {
                    "typed automation setup child failed".into()
                } else {
                    detail.chars().take(1_024).collect()
                },
            });
        }
        serde_json::from_slice(&output.stdout).map_err(|_| McpError::Protocol {
            server: server_name.into(),
            detail: "typed automation setup child returned a malformed receipt".into(),
        })
    }

    /// Execute one typed workflow request through the separate remote
    /// codex-team child. The model never supplies the executable, config path,
    /// state root, environment, or sandbox policy.
    pub async fn execute_automation_workflow(
        &self,
        request: &codex_team_protocol::WorkflowRequestV1,
        network_allowed: bool,
    ) -> Result<codex_team_protocol::WorkflowResponseV1, McpError> {
        self.execute_automation_workflow_named(request, network_allowed, None)
            .await
    }

    pub async fn execute_automation_workflow_named(
        &self,
        request: &codex_team_protocol::WorkflowRequestV1,
        network_allowed: bool,
        job_id: Option<&str>,
    ) -> Result<codex_team_protocol::WorkflowResponseV1, McpError> {
        codex_team_protocol::validate_workflow_request(request)
            .map_err(|error| McpError::InvalidConfig(error.to_string()))?;
        if !self.mechanistic_config.remote.enabled {
            return Err(McpError::InvalidConfig(
                "remote mechanistic automation is disabled".into(),
            ));
        }
        let server_name = self
            .mechanistic_server
            .as_deref()
            .ok_or_else(|| McpError::InvalidConfig("mechanistic delegation is disabled".into()))?;
        self.validate_mechanistic_delegate_server(server_name)
            .await?;
        let executable = {
            let inner = self.inner.lock().await;
            let definition = inner
                .definitions
                .get(server_name)
                .cloned()
                .ok_or_else(|| McpError::UnknownServer(server_name.into()))?;
            resolve_executable(&definition.config.command, &self.base_environment).ok_or_else(
                || McpError::Start {
                    server: server_name.into(),
                    detail: "configured codex-team executable was not found".into(),
                },
            )?
        };
        let config_path = self.mechanistic_config.remote.config.clone();
        let home = self.base_environment.get("HOME").ok_or_else(|| {
            McpError::InvalidConfig("HOME is unavailable for the private workflow vault".into())
        })?;
        let state_root = PathBuf::from(home).join(".codex-team");
        let local_roots = if network_allowed {
            discover_workflow_local_roots(&executable, &config_path, &self.base_environment).await?
        } else {
            Vec::new()
        };
        let workflow_mutates = matches!(
            request.operation,
            codex_team_protocol::WorkflowOperationV1::Run { .. }
        );
        let temporary_directory = tempfile::tempdir().map_err(|error| McpError::Start {
            server: server_name.into(),
            detail: error.to_string(),
        })?;
        let workspace = self
            .workspace
            .read()
            .unwrap_or_else(|poison| poison.into_inner())
            .clone();
        let arguments = vec![
            "automation".to_string(),
            "workflow".to_string(),
            "--config".to_string(),
            config_path.to_string_lossy().into_owned(),
            "--state-root".to_string(),
            state_root.to_string_lossy().into_owned(),
            "--json".to_string(),
        ];
        let prepared = SandboxBackend::new(self.sandbox_mode)
            .prepare_process(
                &executable,
                &arguments,
                &SandboxPolicy {
                    roots: std::iter::once(state_root.clone())
                        .chain(std::iter::once(
                            PathBuf::from(home).join(".local/share/ca/automation-resources/v2"),
                        ))
                        .chain(
                            workflow_mutates
                                .then_some(local_roots.clone())
                                .into_iter()
                                .flatten(),
                        )
                        .collect(),
                    read_only_roots: vec![
                        executable
                            .canonicalize()
                            .unwrap_or_else(|_| executable.clone()),
                        config_path.clone(),
                    ]
                    .into_iter()
                    .chain(
                        (!workflow_mutates)
                            .then_some(local_roots)
                            .into_iter()
                            .flatten(),
                    )
                    .collect(),
                    temporary_directory: temporary_directory.path().to_path_buf(),
                    network_allowed,
                    hide_other_processes: true,
                },
            )
            .map_err(|error| McpError::Start {
                server: server_name.into(),
                detail: error.to_string(),
            })?;
        let mut environment = self.base_environment.clone();
        if network_allowed {
            environment.extend(filtered_server_environment(
                &self.mechanistic_config.remote.env,
            )?);
        }
        let mut command = tokio::process::Command::new(prepared.program);
        command
            .args(prepared.arguments)
            .current_dir(workspace.cwd())
            .env_clear()
            .envs(environment)
            .env("TMPDIR", temporary_directory.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        crate::run_environment::apply_tokio(&mut command);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.as_std_mut().process_group(0);
        }
        let mut child = command.spawn().map_err(|error| McpError::Start {
            server: server_name.into(),
            detail: error.to_string(),
        })?;
        let mut process_group_guard =
            OwnedProcessGroupGuard::new(child.id().and_then(|id| i32::try_from(id).ok()));
        let _workflow_registration = job_id.and_then(|job_id| {
            process_group_guard.process_group.map(|process_group| {
                WorkflowProcessRegistration::new(
                    self.workflow_process_groups.clone(),
                    job_id.to_string(),
                    process_group,
                )
            })
        });
        let input = serde_json::to_vec(request).map_err(|error| McpError::Protocol {
            server: server_name.into(),
            detail: error.to_string(),
        })?;
        let mut stdin = child.stdin.take().ok_or_else(|| McpError::Start {
            server: server_name.into(),
            detail: "automation workflow child omitted stdin".into(),
        })?;
        stdin
            .write_all(&input)
            .await
            .map_err(|error| McpError::Protocol {
                server: server_name.into(),
                detail: error.to_string(),
            })?;
        drop(stdin);
        let output = tokio::time::timeout(
            Duration::from_secs(self.mechanistic_config.remote.max_wait_seconds),
            child.wait_with_output(),
        )
        .await
        .map_err(|_| McpError::Timeout {
            server: server_name.into(),
            operation: "automation workflow",
        })?
        .map_err(|error| McpError::Protocol {
            server: server_name.into(),
            detail: error.to_string(),
        })?;
        process_group_guard.disarm();
        if output.stdout.len() > MAX_OUTPUT_BYTES || output.stderr.len() > MAX_STDERR_BYTES {
            return Err(McpError::Protocol {
                server: server_name.into(),
                detail: "automation workflow output exceeded its bound".into(),
            });
        }
        if !output.status.success() {
            let detail = crate::terminal_text::sanitize_terminal_text(&String::from_utf8_lossy(
                &output.stderr,
            ));
            return Err(McpError::Protocol {
                server: server_name.into(),
                detail: if detail.trim().is_empty() {
                    "typed automation workflow child failed".into()
                } else {
                    detail.chars().take(1_024).collect()
                },
            });
        }
        serde_json::from_slice(&output.stdout).map_err(|_| McpError::Protocol {
            server: server_name.into(),
            detail: "typed automation workflow child returned a malformed receipt".into(),
        })
    }

    pub fn cancel_automation_workflow(&self, job_id: &str) -> bool {
        let process_group = self
            .workflow_process_groups
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .get(job_id)
            .copied();
        if process_group.is_some() {
            kill_process_group(process_group);
            true
        } else {
            false
        }
    }

    pub fn delegation_activity(&self) -> &crate::mechanistic_delegation::DelegationActivity {
        &self.delegation_activity
    }

    pub async fn has_tool(&self, exposed_name: &str) -> bool {
        self.inner.lock().await.running.values().any(|server| {
            server
                .tools
                .iter()
                .any(|tool| tool.exposed_name == exposed_name)
        })
    }

    pub async fn has_active_tool(&self, exposed_name: &str) -> bool {
        let inner = self.inner.lock().await;
        inner.activated_tools.contains(exposed_name)
            && inner.running.values().any(|server| {
                server
                    .tools
                    .iter()
                    .any(|tool| tool.exposed_name == exposed_name)
            })
    }

    pub async fn tool_server(&self, exposed_name: &str) -> Option<String> {
        self.inner
            .lock()
            .await
            .running
            .iter()
            .find_map(|(server, runtime)| {
                runtime
                    .tools
                    .iter()
                    .any(|tool| tool.exposed_name == exposed_name)
                    .then(|| server.clone())
            })
    }

    pub async fn active_tool_server(&self, exposed_name: &str) -> Option<String> {
        let inner = self.inner.lock().await;
        if !inner.activated_tools.contains(exposed_name) {
            return None;
        }
        inner.running.iter().find_map(|(server, runtime)| {
            runtime
                .tools
                .iter()
                .any(|tool| tool.exposed_name == exposed_name)
                .then(|| server.clone())
        })
    }

    pub async fn validate_tool(
        &self,
        exposed_name: &str,
        arguments: &serde_json::Value,
    ) -> Result<(), String> {
        let inner = self.inner.lock().await;
        let (server_name, tool) = inner
            .running
            .iter()
            .find_map(|(server_name, server)| {
                server
                    .tools
                    .iter()
                    .find(|tool| tool.exposed_name == exposed_name)
                    .map(|tool| (server_name, tool))
            })
            .ok_or_else(|| format!("unknown MCP tool: {exposed_name}"))?;
        ToolSpec::mcp(
            &tool.exposed_name,
            server_name,
            tool.description.clone(),
            tool.schema.clone(),
        )
        .and_then(|spec| ToolRegistry::new(vec![spec]))
        .and_then(|registry| registry.validate(exposed_name, arguments))
        .map_err(|error| error.to_string())
    }

    pub async fn validate_active_tool(
        &self,
        exposed_name: &str,
        arguments: &serde_json::Value,
    ) -> Result<(), String> {
        if !self.has_active_tool(exposed_name).await {
            return Err(format!("unavailable MCP tool: {exposed_name}"));
        }
        self.validate_tool(exposed_name, arguments).await
    }

    pub async fn read_resource(&self, server_name: &str, uri: &str) -> Result<String, McpError> {
        self.ensure_started(server_name).await?;
        let (timeout_secs, peer, transport, secret_values) = {
            let inner = self.inner.lock().await;
            let definition = inner
                .definitions
                .get(server_name)
                .ok_or_else(|| McpError::UnknownServer(server_name.into()))?;
            let server = inner
                .running
                .get(server_name)
                .ok_or_else(|| McpError::UnknownServer(server_name.into()))?;
            if !server.resources.iter().any(|resource| resource.uri == uri) {
                return Err(McpError::Protocol {
                    server: server_name.into(),
                    detail: "resource was not advertised by the server".into(),
                });
            }
            (
                definition.config.timeout_secs,
                server.service.peer().clone(),
                server.transport,
                secret_values_for_config(&definition.config),
            )
        };
        let request = peer.read_resource(ReadResourceRequestParams::new(uri));
        let result = tokio::time::timeout(Duration::from_secs(timeout_secs), request)
            .await
            .map_err(|_| McpError::Timeout {
                server: server_name.into(),
                operation: "resource read",
            })?
            .map_err(|error| McpError::Protocol {
                server: server_name.into(),
                detail: safe_protocol_detail(transport, "resource read", &error),
            })?;
        let mut output = String::new();
        for content in result.contents {
            match content {
                ResourceContents::TextResourceContents { text, .. } => {
                    if let Err(error) = crate::provider_filter::scan_outbound(text.as_bytes()) {
                        return Ok(crate::provider_filter::withheld_message(&error));
                    }
                    append_bounded(&mut output, &text, MAX_OUTPUT_BYTES)?;
                }
                ResourceContents::BlobResourceContents { .. } => {
                    return Err(McpError::BinaryContent)
                }
                _ => return Err(McpError::BinaryContent),
            }
        }
        Ok(redact_values(
            &crate::terminal_text::sanitize_terminal_text(&output),
            &secret_values,
        ))
    }

    pub async fn reconnect(&self, server_name: &str) -> Result<(), McpError> {
        if self.inner.lock().await.running.contains_key(server_name) {
            self.close_server(server_name).await?;
        }
        self.ensure_started(server_name).await
    }

    pub async fn connect_authorization_with_progress(
        &self,
        server_name: &str,
        progress: Option<crate::mcp_oauth::ProgressSink>,
    ) -> Result<(), McpError> {
        let definition = {
            let inner = self.inner.lock().await;
            inner
                .definitions
                .get(server_name)
                .cloned()
                .ok_or_else(|| McpError::UnknownServer(server_name.into()))?
        };
        if !definition.trusted {
            return Err(McpError::Untrusted(server_name.into()));
        }
        if definition.config.network
            && (!definition.network_trusted || !self.network_allowed_for(server_name))
        {
            return Err(McpError::NetworkApprovalRequired(server_name.into()));
        }
        if !definition.config.oauth.enabled {
            return Err(McpError::InvalidConfig(format!(
                "MCP server {server_name} does not enable OAuth"
            )));
        }
        if self.inner.lock().await.running.contains_key(server_name) {
            self.close_server(server_name).await?;
        }
        match definition.config.oauth.flow {
            crate::config::McpOAuthFlow::Browser => {
                crate::mcp_oauth::authorize_browser(
                    &definition.config,
                    &self.active_profile,
                    server_name,
                    self.oauth_secret_store.clone(),
                    self.browser_opener.clone(),
                    progress,
                )
                .await
                .map_err(|_| McpError::AuthorizationFailed {
                    server: server_name.into(),
                    operation: "connect",
                })?;
            }
            crate::config::McpOAuthFlow::Device => {
                crate::mcp_oauth::authorize_device(
                    &definition.config,
                    &self.active_profile,
                    server_name,
                    self.oauth_secret_store.clone(),
                    self.browser_opener.clone(),
                    progress,
                )
                .await
                .map_err(|_| McpError::AuthorizationFailed {
                    server: server_name.into(),
                    operation: "connect",
                })?;
            }
        }
        {
            let mut inner = self.inner.lock().await;
            inner.authorization_required.remove(server_name);
            inner.failures.remove(server_name);
        }
        self.ensure_started(server_name).await
    }

    pub async fn refresh_authorization(&self, server_name: &str) -> Result<(), McpError> {
        let oauth_enabled = self
            .inner
            .lock()
            .await
            .definitions
            .get(server_name)
            .ok_or_else(|| McpError::UnknownServer(server_name.into()))?
            .config
            .oauth
            .enabled;
        if !oauth_enabled {
            return Err(McpError::InvalidConfig(format!(
                "MCP server {server_name} does not enable OAuth"
            )));
        }
        self.reconnect(server_name).await
    }

    pub async fn disconnect_authorization(&self, server_name: &str) -> Result<(), McpError> {
        let oauth_enabled = self
            .inner
            .lock()
            .await
            .definitions
            .get(server_name)
            .ok_or_else(|| McpError::UnknownServer(server_name.into()))?
            .config
            .oauth
            .enabled;
        if !oauth_enabled {
            return Err(McpError::InvalidConfig(format!(
                "MCP server {server_name} does not enable OAuth"
            )));
        }
        if self.inner.lock().await.running.contains_key(server_name) {
            self.close_server(server_name).await?;
        }
        let credentials = crate::mcp_oauth::KeychainCredentialStore::new(
            self.oauth_secret_store.clone(),
            &self.active_profile,
            server_name,
        );
        rmcp::transport::auth::CredentialStore::clear(&credentials)
            .await
            .map_err(|_| McpError::AuthorizationFailed {
                server: server_name.into(),
                operation: "disconnect",
            })?;
        let mut inner = self.inner.lock().await;
        inner.failures.remove(server_name);
        inner.authorization_required.insert(server_name.into());
        Ok(())
    }

    pub async fn shutdown(&self) {
        let names: Vec<String> = {
            let inner = self.inner.lock().await;
            inner.running.keys().cloned().collect()
        };
        for name in names {
            let _ = self.close_server(&name).await;
        }
    }

    async fn ensure_started(&self, server_name: &str) -> Result<(), McpError> {
        {
            let inner = self.inner.lock().await;
            if let Some(server) = inner.running.get(server_name) {
                if !server.stale_tools.swap(false, Ordering::AcqRel)
                    && !server.stale_resources.swap(false, Ordering::AcqRel)
                {
                    return Ok(());
                }
            }
        }
        if self.inner.lock().await.running.contains_key(server_name) {
            self.refresh(server_name).await?;
            return Ok(());
        }
        self.start_server(server_name).await
    }

    async fn start_server(&self, server_name: &str) -> Result<(), McpError> {
        let definition = {
            let inner = self.inner.lock().await;
            inner
                .definitions
                .get(server_name)
                .cloned()
                .ok_or_else(|| McpError::UnknownServer(server_name.into()))?
        };
        if !definition.trusted {
            return Err(McpError::Untrusted(server_name.into()));
        }
        if definition.config.network
            && (!definition.network_trusted || !self.network_allowed_for(server_name))
        {
            return Err(McpError::NetworkApprovalRequired(server_name.into()));
        }
        let temporary_directory = std::env::temp_dir().join(format!(
            "ca-mcp-{}-{}",
            normalize_component(server_name),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir(&temporary_directory).map_err(|error| McpError::Start {
            server: server_name.into(),
            detail: error.to_string(),
        })?;
        let start_result = self.spawn_service(&definition, &temporary_directory).await;
        let (mut service, stderr, stale_tools, stale_resources, process_group) = match start_result
        {
            Ok(value) => value,
            Err(error) => {
                let _ = std::fs::remove_dir_all(&temporary_directory);
                self.inner
                    .lock()
                    .await
                    .failures
                    .insert(server_name.into(), error.to_string());
                if matches!(&error, McpError::AuthorizationRequired(_)) {
                    self.inner
                        .lock()
                        .await
                        .authorization_required
                        .insert(server_name.into());
                }
                return Err(error);
            }
        };
        let timeout = Duration::from_secs(definition.config.timeout_secs);
        let tools = match tokio::time::timeout(timeout, service.list_all_tools()).await {
            Ok(Ok(tools)) => tools,
            Ok(Err(error)) => {
                let _ = service.close_with_timeout(Duration::from_secs(2)).await;
                kill_process_group(process_group);
                let _ = std::fs::remove_dir_all(&temporary_directory);
                return Err(McpError::Protocol {
                    server: server_name.into(),
                    detail: safe_protocol_detail(
                        definition.config.transport,
                        "tool discovery",
                        &error,
                    ),
                });
            }
            Err(_) => {
                let _ = service.close_with_timeout(Duration::from_secs(2)).await;
                kill_process_group(process_group);
                let _ = std::fs::remove_dir_all(&temporary_directory);
                return Err(McpError::Timeout {
                    server: server_name.into(),
                    operation: "tool discovery",
                });
            }
        };
        let mut resources = match tokio::time::timeout(timeout, service.list_all_resources()).await
        {
            Ok(Ok(resources)) => resources,
            Ok(Err(error)) => {
                let _ = service.close_with_timeout(Duration::from_secs(2)).await;
                kill_process_group(process_group);
                let _ = std::fs::remove_dir_all(&temporary_directory);
                return Err(McpError::Protocol {
                    server: server_name.into(),
                    detail: safe_protocol_detail(
                        definition.config.transport,
                        "resource discovery",
                        &error,
                    ),
                });
            }
            Err(_) => {
                let _ = service.close_with_timeout(Duration::from_secs(2)).await;
                kill_process_group(process_group);
                let _ = std::fs::remove_dir_all(&temporary_directory);
                return Err(McpError::Timeout {
                    server: server_name.into(),
                    operation: "resource discovery",
                });
            }
        };
        let mut remote_tools = match map_tools(server_name, tools) {
            Ok(tools) => tools,
            Err(error) => {
                let _ = service.close_with_timeout(Duration::from_secs(2)).await;
                kill_process_group(process_group);
                let _ = std::fs::remove_dir_all(&temporary_directory);
                return Err(error);
            }
        };
        if let Err(error) = validate_resources(server_name, &resources) {
            let _ = service.close_with_timeout(Duration::from_secs(2)).await;
            kill_process_group(process_group);
            let _ = std::fs::remove_dir_all(&temporary_directory);
            return Err(error);
        }
        sort_resources(&mut resources);
        add_resource_tool(server_name, &resources, &mut remote_tools);
        let mut inner = self.inner.lock().await;
        inner.failures.remove(server_name);
        inner.authorization_required.remove(server_name);
        inner.running.insert(
            server_name.into(),
            RunningServer {
                service,
                tools: remote_tools,
                resources,
                stale_tools,
                stale_resources,
                stderr,
                temporary_directory,
                process_group,
                transport: definition.config.transport,
            },
        );
        Ok(())
    }

    async fn spawn_service(
        &self,
        definition: &ServerDefinition,
        temporary_directory: &Path,
    ) -> Result<
        (
            McpService,
            Arc<Mutex<String>>,
            Arc<AtomicBool>,
            Arc<AtomicBool>,
            Option<i32>,
        ),
        McpError,
    > {
        match definition.config.transport {
            McpTransport::Stdio => {
                self.spawn_stdio_service(definition, temporary_directory)
                    .await
            }
            McpTransport::StreamableHttp => self.connect_http_service(definition).await,
        }
    }

    async fn spawn_stdio_service(
        &self,
        definition: &ServerDefinition,
        temporary_directory: &Path,
    ) -> Result<
        (
            McpService,
            Arc<Mutex<String>>,
            Arc<AtomicBool>,
            Arc<AtomicBool>,
            Option<i32>,
        ),
        McpError,
    > {
        let executable = resolve_executable(&definition.config.command, &self.base_environment)
            .ok_or_else(|| McpError::Start {
                server: definition.name.clone(),
                detail: "configured executable was not found".into(),
            })?;
        let workspace = self
            .workspace
            .read()
            .unwrap_or_else(|poison| poison.into_inner())
            .clone();
        let roots = workspace
            .roots()
            .iter()
            .map(|root| root.path.clone())
            .collect();
        let prepared = SandboxBackend::new(self.sandbox_mode)
            .prepare_process(
                &executable,
                &definition.config.args,
                &SandboxPolicy {
                    roots,
                    // A profile-local stdio server may be an installed binary
                    // outside the workspace (for example ~/.local/bin). Give
                    // the sandbox literal read/execute access to that one
                    // resolved executable, not its containing directory.
                    read_only_roots: vec![executable
                        .canonicalize()
                        .unwrap_or_else(|_| executable.clone())],
                    temporary_directory: temporary_directory.to_path_buf(),
                    network_allowed: definition.config.network,
                    hide_other_processes: false,
                },
            )
            .map_err(|error| McpError::Start {
                server: definition.name.clone(),
                detail: error.to_string(),
            })?;
        let mut command = tokio::process::Command::new(prepared.program);
        command
            .args(prepared.arguments)
            .current_dir(workspace.cwd())
            .env_clear()
            .envs(&self.base_environment)
            .envs(filtered_server_environment(&definition.config.env)?)
            .env("TMPDIR", temporary_directory)
            .kill_on_drop(true);
        crate::run_environment::apply_tokio(&mut command);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.as_std_mut().process_group(0);
        }
        let (transport, stderr) = TokioChildProcess::builder(command)
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| McpError::Start {
                server: definition.name.clone(),
                detail: error.to_string(),
            })?;
        let process_group = transport.id().and_then(|id| i32::try_from(id).ok());
        let stderr_text = Arc::new(Mutex::new(String::new()));
        if let Some(mut stderr) = stderr {
            let buffer = stderr_text.clone();
            tokio::spawn(async move {
                let mut chunk = [0u8; 4096];
                loop {
                    let count = match stderr.read(&mut chunk).await {
                        Ok(0) | Err(_) => break,
                        Ok(count) => count,
                    };
                    let mut text = buffer.lock().await;
                    let remaining = MAX_STDERR_BYTES.saturating_sub(text.len());
                    if remaining == 0 {
                        break;
                    }
                    let value = String::from_utf8_lossy(&chunk[..count.min(remaining)]);
                    text.push_str(&crate::terminal_text::sanitize_terminal_text(&value));
                }
            });
        }
        let stale_tools = Arc::new(AtomicBool::new(false));
        let stale_resources = Arc::new(AtomicBool::new(false));
        let handler = McpClientHandler {
            stale_tools: stale_tools.clone(),
            stale_resources: stale_resources.clone(),
        };
        let service_result = tokio::time::timeout(
            Duration::from_secs(definition.config.timeout_secs),
            handler.serve(transport),
        )
        .await;
        let service = match service_result {
            Err(_) => {
                kill_process_group(process_group);
                return Err(McpError::Timeout {
                    server: definition.name.clone(),
                    operation: "initialization",
                });
            }
            Ok(Err(error)) => {
                kill_process_group(process_group);
                tokio::time::sleep(Duration::from_millis(10)).await;
                let stderr = stderr_text.lock().await.clone();
                let detail = if stderr.trim().is_empty() {
                    error.to_string()
                } else {
                    format!("{}; stderr: {}", error, stderr.trim())
                };
                return Err(McpError::Start {
                    server: definition.name.clone(),
                    detail,
                });
            }
            Ok(Ok(service)) => service,
        };
        Ok((
            service,
            stderr_text,
            stale_tools,
            stale_resources,
            process_group,
        ))
    }

    async fn connect_http_service(
        &self,
        definition: &ServerDefinition,
    ) -> Result<
        (
            McpService,
            Arc<Mutex<String>>,
            Arc<AtomicBool>,
            Arc<AtomicBool>,
            Option<i32>,
        ),
        McpError,
    > {
        let client = SafeHttpClient::new(&definition.config)?;
        let url = definition
            .config
            .url
            .clone()
            .ok_or_else(|| McpError::InvalidConfig("Streamable HTTP URL is required".into()))?;
        let transport_config = StreamableHttpClientTransportConfig::with_uri(url)
            .max_sse_event_size(MAX_HTTP_EVENT_BYTES)
            .reinit_on_expired_session(true);
        let stale_tools = Arc::new(AtomicBool::new(false));
        let stale_resources = Arc::new(AtomicBool::new(false));
        let handler = McpClientHandler {
            stale_tools: stale_tools.clone(),
            stale_resources: stale_resources.clone(),
        };
        let service = if definition.config.oauth.enabled {
            let (manager, initialized) = crate::mcp_oauth::initialized_authorization_manager(
                &definition.config,
                &self.active_profile,
                &definition.name,
                self.oauth_secret_store.clone(),
            )
            .await
            .map_err(|_| McpError::AuthorizationFailed {
                server: definition.name.clone(),
                operation: "discovery",
            })?;
            if !initialized {
                return Err(McpError::AuthorizationRequired(definition.name.clone()));
            }
            let authenticated = rmcp::transport::auth::AuthClient::new(client, manager);
            let transport =
                StreamableHttpClientTransport::with_client(authenticated, transport_config.clone());
            tokio::time::timeout(
                Duration::from_secs(definition.config.timeout_secs),
                handler.serve(transport),
            )
            .await
            .map_err(|_| McpError::Timeout {
                server: definition.name.clone(),
                operation: "initialization",
            })?
            .map_err(|_| McpError::AuthorizationFailed {
                server: definition.name.clone(),
                operation: "initialization",
            })?
        } else {
            let transport = StreamableHttpClientTransport::with_client(client, transport_config);
            tokio::time::timeout(
                Duration::from_secs(definition.config.timeout_secs),
                handler.serve(transport),
            )
            .await
            .map_err(|_| McpError::Timeout {
                server: definition.name.clone(),
                operation: "initialization",
            })?
            .map_err(|_| McpError::Start {
                server: definition.name.clone(),
                detail: "HTTP transport initialization failed".into(),
            })?
        };
        Ok((
            service,
            Arc::new(Mutex::new(String::new())),
            stale_tools,
            stale_resources,
            None,
        ))
    }

    async fn refresh(&self, server_name: &str) -> Result<(), McpError> {
        let (timeout_secs, transport, peer) = {
            let inner = self.inner.lock().await;
            let timeout_secs = inner
                .definitions
                .get(server_name)
                .ok_or_else(|| McpError::UnknownServer(server_name.into()))?
                .config
                .timeout_secs;
            let server = inner
                .running
                .get(server_name)
                .ok_or_else(|| McpError::UnknownServer(server_name.into()))?;
            (
                timeout_secs,
                server.transport,
                server.service.peer().clone(),
            )
        };
        let timeout = Duration::from_secs(timeout_secs);
        let tools = tokio::time::timeout(timeout, peer.list_all_tools())
            .await
            .map_err(|_| McpError::Timeout {
                server: server_name.into(),
                operation: "tool refresh",
            })?
            .map_err(|error| McpError::Protocol {
                server: server_name.into(),
                detail: safe_protocol_detail(transport, "tool refresh", &error),
            })?;
        let mut resources = tokio::time::timeout(timeout, peer.list_all_resources())
            .await
            .map_err(|_| McpError::Timeout {
                server: server_name.into(),
                operation: "resource refresh",
            })?
            .map_err(|error| McpError::Protocol {
                server: server_name.into(),
                detail: safe_protocol_detail(transport, "resource refresh", &error),
            })?;
        let mut tools = map_tools(server_name, tools)?;
        validate_resources(server_name, &resources)?;
        sort_resources(&mut resources);
        add_resource_tool(server_name, &resources, &mut tools);
        let mut inner = self.inner.lock().await;
        let previous_names = inner
            .running
            .get(server_name)
            .map(|server| {
                server
                    .tools
                    .iter()
                    .map(|tool| tool.exposed_name.clone())
                    .collect::<HashSet<_>>()
            })
            .unwrap_or_default();
        inner
            .activated_tools
            .retain(|name| !previous_names.contains(name));
        let server = inner
            .running
            .get_mut(server_name)
            .ok_or_else(|| McpError::UnknownServer(server_name.into()))?;
        server.tools = tools;
        server.resources = resources;
        Ok(())
    }

    async fn close_server(&self, server_name: &str) -> Result<(), McpError> {
        let mut inner = self.inner.lock().await;
        let mut server = inner
            .running
            .remove(server_name)
            .ok_or_else(|| McpError::UnknownServer(server_name.into()))?;
        let names = server
            .tools
            .iter()
            .map(|tool| tool.exposed_name.clone())
            .collect::<HashSet<_>>();
        inner.activated_tools.retain(|name| !names.contains(name));
        drop(inner);
        let _ = server
            .service
            .close_with_timeout(Duration::from_secs(3))
            .await;
        kill_process_group(server.process_group);
        let _ = std::fs::remove_dir_all(&server.temporary_directory);
        Ok(())
    }

    pub async fn status_text(&self, verbose: bool) -> String {
        let statuses = self.statuses().await;
        let inner = self.inner.lock().await;
        let mut lines = vec!["MCP servers".to_string()];
        for status in statuses {
            let transport = inner.definitions[&status.name].config.transport;
            let definition = &inner.definitions[&status.name];
            let oauth = if !definition.config.oauth.enabled {
                "none"
            } else if matches!(status.state, McpServerState::Running) {
                "ready"
            } else if matches!(status.state, McpServerState::AuthorizationRequired) {
                "authorization_required"
            } else if matches!(status.state, McpServerState::Failed) {
                "error"
            } else {
                "unchecked"
            };
            lines.push(format!(
                "{} [{} {:?}/{:?}] oauth={} tools={} active={} resources={}",
                crate::terminal_text::sanitize_terminal_text(&status.name),
                transport_label(transport),
                status.origin,
                status.state,
                oauth,
                status.tool_count,
                status.active_tool_count,
                status.resource_count
            ));
            if let Some(server) = inner.running.get(&status.name) {
                if verbose {
                    for tool in &server.tools {
                        lines.push(format!(
                            "  tool: {}",
                            crate::terminal_text::sanitize_terminal_text(&tool.exposed_name)
                        ));
                    }
                    for resource in &server.resources {
                        lines.push(format!(
                            "  resource: {} ({})",
                            crate::terminal_text::sanitize_terminal_text(&resource.name),
                            crate::terminal_text::sanitize_terminal_text(&resource.uri)
                        ));
                    }
                }
                let stderr = server.stderr.lock().await;
                if verbose && !stderr.is_empty() {
                    let values = secret_values_for_config(&inner.definitions[&status.name].config);
                    lines.push(format!(
                        "  stderr: {}",
                        redact_values(stderr.lines().last().unwrap_or(""), &values)
                    ));
                }
            }
            if let Some(detail) = status.detail {
                let values = secret_values_for_config(&inner.definitions[&status.name].config);
                lines.push(format!(
                    "  error: {}",
                    redact_values(
                        &crate::terminal_text::sanitize_terminal_text(&detail),
                        &values
                    )
                ));
            }
        }
        lines.join("\n")
    }

    pub async fn verbose_status(&self) -> String {
        self.status_text(true).await
    }

    #[cfg(test)]
    async fn temporary_directories(&self) -> Vec<PathBuf> {
        self.inner
            .lock()
            .await
            .running
            .values()
            .map(|server| server.temporary_directory.clone())
            .collect()
    }
}

fn build_registry(
    inner: &McpManagerInner,
    active: Option<&BTreeSet<String>>,
) -> Result<ToolRegistry, Vec<String>> {
    let mut specs = ToolRegistry::builtins().specs().to_vec();
    let mut diagnostics = Vec::new();
    let mut remote_tools = inner
        .running
        .iter()
        .flat_map(|(server_name, server)| {
            server
                .tools
                .iter()
                .map(move |tool| (server_name.as_str(), tool))
        })
        .filter(|(_, tool)| active.is_none_or(|active| active.contains(&tool.exposed_name)))
        .collect::<Vec<_>>();
    sort_remote_tool_entries(&mut remote_tools);
    for (server_name, tool) in remote_tools {
        let description = safe_remote_description(inner, server_name, &tool.description);
        match ToolSpec::mcp(
            &tool.exposed_name,
            server_name,
            description,
            tool.schema.clone(),
        ) {
            Ok(spec) => specs.push(spec),
            Err(_) => diagnostics.push(format!(
                "MCP server {} advertised an invalid tool schema",
                safe_server_label(server_name)
            )),
        }
    }
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    ToolRegistry::new(specs)
        .map_err(|_| vec!["MCP tool namespace collision prevented catalog activation".to_string()])
}

fn sort_remote_tool_entries(entries: &mut [(&str, &RemoteTool)]) {
    entries.sort_by(|(left_server, left_tool), (right_server, right_tool)| {
        left_server
            .cmp(right_server)
            .then_with(|| left_tool.exposed_name.cmp(&right_tool.exposed_name))
    });
}

fn catalog_snapshot(inner: &McpManagerInner, active_only: bool) -> McpCatalog {
    let active = active_only.then_some(&inner.activated_tools);
    let (registry, diagnostics) = match build_registry(inner, active) {
        Ok(registry) => (Some(registry), Vec::new()),
        Err(diagnostics) => (None, diagnostics),
    };
    let active_servers = active.map(|active| {
        inner
            .running
            .iter()
            .filter(|(_, server)| {
                server
                    .tools
                    .iter()
                    .any(|tool| active.contains(&tool.exposed_name))
            })
            .map(|(name, _)| name.as_str())
            .collect::<HashSet<_>>()
    });
    let mut resources = inner
        .running
        .iter()
        .filter(|(name, _)| {
            active_servers
                .as_ref()
                .is_none_or(|servers| servers.contains(name.as_str()))
        })
        .flat_map(|(server_name, server)| {
            server.resources.iter().map(|resource| McpResourceInfo {
                server: server_name.clone(),
                uri: resource.uri.clone(),
                name: resource.name.clone(),
                description: resource.description.clone(),
                mime_type: resource.mime_type.clone(),
            })
        })
        .collect::<Vec<_>>();
    resources.sort_by(|left, right| {
        left.server
            .cmp(&right.server)
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.uri.cmp(&right.uri))
    });
    McpCatalog {
        registry,
        resources,
        diagnostics,
    }
}

fn safe_remote_description(
    inner: &McpManagerInner,
    server_name: &str,
    description: &str,
) -> String {
    let mut sensitive = inner
        .definitions
        .get(server_name)
        .map(|definition| secret_values_for_config(&definition.config))
        .unwrap_or_default();
    if let Some(url) = inner
        .definitions
        .get(server_name)
        .and_then(|definition| definition.config.url.clone())
    {
        sensitive.push(url);
    }
    redact_values(
        &crate::terminal_text::sanitize_terminal_text(description),
        &sensitive,
    )
}

fn safe_server_label(server: &str) -> String {
    let sanitized = crate::terminal_text::sanitize_terminal_text(server);
    if sanitized.is_empty() {
        "<unknown>".into()
    } else {
        sanitized.chars().take(64).collect()
    }
}

fn safe_search_failure(server: &str, error: &McpError) -> String {
    let reason = match error {
        McpError::Untrusted(_) => "project trust is required",
        McpError::NetworkApprovalRequired(_) => "a distinct network grant is required",
        McpError::AuthorizationRequired(_) => "authorization is required",
        McpError::AuthorizationFailed { .. } => "authorization failed",
        McpError::Timeout { .. } => "the server timed out",
        McpError::UnknownServer(_) => "the server is unknown",
        McpError::InvalidConfig(_) => "the configuration is invalid",
        McpError::Tool { .. } => "the remote tool rejected the request",
        McpError::Start { .. }
        | McpError::Protocol { .. }
        | McpError::OutputLimit(_)
        | McpError::BinaryContent => "the server was unavailable",
    };
    format!(
        "MCP server {} was not searched: {reason}",
        safe_server_label(server)
    )
}

fn bounded_search_output(value: &str) -> String {
    if value.len() <= MAX_SEARCH_OUTPUT_BYTES {
        return value.to_string();
    }
    let mut boundary = MAX_SEARCH_OUTPUT_BYTES.saturating_sub(64).min(value.len());
    while boundary > 0 && !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    let omitted = value.len().saturating_sub(boundary);
    format!("{}\n[… {omitted} bytes omitted]", &value[..boundary])
}

fn transport_label(transport: McpTransport) -> &'static str {
    match transport {
        McpTransport::Stdio => "stdio",
        McpTransport::StreamableHttp => "streamable_http",
    }
}

fn safe_protocol_detail(
    transport: McpTransport,
    operation: &'static str,
    error: &impl std::fmt::Display,
) -> String {
    match transport {
        McpTransport::Stdio => error.to_string(),
        McpTransport::StreamableHttp => format!("HTTP {operation} failed"),
    }
}

fn secret_values_for_config(config: &McpServerConfig) -> Vec<String> {
    let mut values = config.env.values().cloned().collect::<Vec<_>>();
    values.extend(config.headers.values().filter_map(|reference| {
        exact_environment_reference(reference).and_then(crate::run_environment::var)
    }));
    values
}

async fn discover_workflow_local_roots(
    executable: &Path,
    config_path: &Path,
    base_environment: &BTreeMap<String, String>,
) -> Result<Vec<PathBuf>, McpError> {
    let mut command = tokio::process::Command::new(executable);
    command
        .args([
            "automation",
            "workflow-roots",
            "--config",
            &config_path.to_string_lossy(),
            "--json",
        ])
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    for name in ["HOME", "PATH", "TMPDIR"] {
        if let Some(value) = base_environment.get(name) {
            command.env(name, value);
        }
    }
    crate::run_environment::apply_tokio(&mut command);
    let output = tokio::time::timeout(Duration::from_secs(5), command.output())
        .await
        .map_err(|_| McpError::Timeout {
            server: "codex-team-workflow".into(),
            operation: "local root discovery",
        })?
        .map_err(|error| McpError::Start {
            server: "codex-team-workflow".into(),
            detail: error.to_string(),
        })?;
    if !output.status.success() || output.stdout.len() > 64 * 1024 {
        return Err(McpError::InvalidConfig(
            "codex-team rejected the configured local workflow roots".into(),
        ));
    }
    let value: serde_json::Value =
        serde_json::from_slice(&output.stdout).map_err(|_| McpError::Protocol {
            server: "codex-team-workflow".into(),
            detail: "local workflow root receipt was malformed".into(),
        })?;
    if value.get("schema").and_then(serde_json::Value::as_str)
        != Some("codex-team.workflow_local_roots.v1")
    {
        return Err(McpError::Protocol {
            server: "codex-team-workflow".into(),
            detail: "local workflow root receipt schema was invalid".into(),
        });
    }
    let roots = value
        .get("roots")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| McpError::Protocol {
            server: "codex-team-workflow".into(),
            detail: "local workflow root receipt omitted roots".into(),
        })?;
    if roots.len() > 64 {
        return Err(McpError::InvalidConfig(
            "too many configured local workflow roots".into(),
        ));
    }
    roots
        .iter()
        .map(|root| {
            let root = root.as_str().ok_or_else(|| McpError::Protocol {
                server: "codex-team-workflow".into(),
                detail: "local workflow root was not a string".into(),
            })?;
            let path = PathBuf::from(root);
            if !path.is_absolute()
                || path == Path::new("/")
                || path.canonicalize().ok() != Some(path.clone())
            {
                return Err(McpError::InvalidConfig(
                    "local workflow root was not an exact canonical directory".into(),
                ));
            }
            Ok(path)
        })
        .collect()
}

fn kill_process_group(process_group: Option<i32>) {
    #[cfg(unix)]
    if let Some(process_group) = process_group {
        let _ = nix::sys::signal::killpg(
            nix::unistd::Pid::from_raw(process_group),
            nix::sys::signal::Signal::SIGKILL,
        );
    }
    #[cfg(not(unix))]
    let _ = process_group;
}

struct OwnedProcessGroupGuard {
    process_group: Option<i32>,
}

struct WorkflowProcessRegistration {
    registry: Arc<std::sync::Mutex<HashMap<String, i32>>>,
    job_id: String,
}

impl WorkflowProcessRegistration {
    fn new(
        registry: Arc<std::sync::Mutex<HashMap<String, i32>>>,
        job_id: String,
        process_group: i32,
    ) -> Self {
        registry
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .insert(job_id.clone(), process_group);
        Self { registry, job_id }
    }
}

impl Drop for WorkflowProcessRegistration {
    fn drop(&mut self) {
        self.registry
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .remove(&self.job_id);
    }
}

impl OwnedProcessGroupGuard {
    fn new(process_group: Option<i32>) -> Self {
        Self { process_group }
    }

    fn disarm(&mut self) {
        self.process_group = None;
    }
}

impl Drop for OwnedProcessGroupGuard {
    fn drop(&mut self) {
        kill_process_group(self.process_group);
    }
}

fn matching_trust<'a>(
    trust: &'a [ProjectMcpTrust],
    workspace: &Path,
    sha256: &str,
) -> Option<&'a ProjectMcpTrust> {
    trust
        .iter()
        .find(|record| record.workspace == workspace && record.sha256 == sha256)
}

fn validate_server_name(name: &str) -> Result<(), McpError> {
    if name.is_empty()
        || name.len() > 64
        || !name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
    {
        return Err(McpError::InvalidConfig(format!(
            "invalid server name {name:?}"
        )));
    }
    Ok(())
}

pub(crate) fn validate_server_config(config: &McpServerConfig) -> Result<(), McpError> {
    if config.timeout_secs == 0 || config.timeout_secs > 600 {
        return Err(McpError::InvalidConfig(
            "MCP timeout must be between 1 and 600 seconds".into(),
        ));
    }
    match config.transport {
        McpTransport::Stdio => {
            if config.command.trim().is_empty() || config.command.contains('\0') {
                return Err(McpError::InvalidConfig("empty MCP command".into()));
            }
            if config.args.iter().any(|argument| argument.contains('\0')) {
                return Err(McpError::InvalidConfig("MCP argument contains NUL".into()));
            }
            if config.url.is_some() || !config.headers.is_empty() || config.oauth.enabled {
                return Err(McpError::InvalidConfig(
                    "stdio MCP servers cannot configure an HTTP URL, headers, or OAuth".into(),
                ));
            }
            let _ = filtered_server_environment(&config.env)?;
        }
        McpTransport::StreamableHttp => {
            if !config.command.is_empty() || !config.args.is_empty() || !config.env.is_empty() {
                return Err(McpError::InvalidConfig(
                    "Streamable HTTP MCP servers cannot configure command, args, or env".into(),
                ));
            }
            if !config.network {
                return Err(McpError::InvalidConfig(
                    "Streamable HTTP MCP servers require network = true".into(),
                ));
            }
            let url = config
                .url
                .as_deref()
                .ok_or_else(|| McpError::InvalidConfig("Streamable HTTP URL is required".into()))?;
            validate_http_url(url)?;
            validate_http_headers(&config.headers)?;
            validate_oauth_config(config, url)?;
        }
    }
    Ok(())
}

fn validate_oauth_config(config: &McpServerConfig, resource: &str) -> Result<(), McpError> {
    let oauth = &config.oauth;
    if !oauth.enabled {
        if oauth != &crate::config::McpOAuthConfig::default() {
            return Err(McpError::InvalidConfig(
                "MCP OAuth settings require enabled = true".into(),
            ));
        }
        return Ok(());
    }
    if config
        .headers
        .keys()
        .any(|name| name.eq_ignore_ascii_case("authorization"))
    {
        return Err(McpError::InvalidConfig(
            "MCP OAuth cannot be combined with an Authorization header".into(),
        ));
    }
    if oauth.client_id.as_deref().is_some_and(|value| {
        value.is_empty() || value.len() > 1024 || value.chars().any(char::is_control)
    }) {
        return Err(McpError::InvalidConfig(
            "invalid MCP OAuth client ID".into(),
        ));
    }
    if oauth.client_id.is_some() && oauth.client_metadata_url.is_some() {
        return Err(McpError::InvalidConfig(
            "MCP OAuth client ID and client metadata URL are mutually exclusive".into(),
        ));
    }
    if let Some(secret) = oauth.client_secret.as_deref() {
        if oauth.client_id.is_none() || exact_environment_reference(secret).is_none() {
            return Err(McpError::InvalidConfig(
                "MCP OAuth client secrets require a client ID and an exact ${ENV_VAR} reference"
                    .into(),
            ));
        }
    }
    if matches!(oauth.flow, crate::config::McpOAuthFlow::Device)
        && (oauth.client_id.is_none() || oauth.client_secret.is_some())
    {
        return Err(McpError::InvalidConfig(
            "MCP device authorization requires a pre-registered public client ID".into(),
        ));
    }
    if oauth.scopes.len() > 32
        || oauth.scopes.iter().any(|scope| {
            scope.is_empty()
                || scope.len() > 256
                || scope.chars().any(char::is_control)
                || scope.chars().any(char::is_whitespace)
        })
    {
        return Err(McpError::InvalidConfig("invalid MCP OAuth scopes".into()));
    }

    let resource = reqwest_13::Url::parse(resource)
        .map_err(|_| McpError::InvalidConfig("invalid Streamable HTTP URL".into()))?;
    if let Some(issuer) = oauth.issuer.as_deref() {
        let issuer = validate_oauth_endpoint(issuer, &resource, "issuer")?;
        if issuer.query().is_some() || issuer.fragment().is_some() {
            return Err(McpError::InvalidConfig(
                "MCP OAuth issuer cannot contain a query or fragment".into(),
            ));
        }
    }
    if let Some(metadata) = oauth.client_metadata_url.as_deref() {
        let metadata = validate_oauth_endpoint(metadata, &resource, "client metadata URL")?;
        if metadata.scheme() != "https" || metadata.path() == "/" {
            return Err(McpError::InvalidConfig(
                "MCP OAuth client metadata URL must be a non-root HTTPS URL".into(),
            ));
        }
    }
    Ok(())
}

fn validate_oauth_endpoint(
    value: &str,
    resource: &reqwest_13::Url,
    label: &str,
) -> Result<reqwest_13::Url, McpError> {
    let url = reqwest_13::Url::parse(value)
        .map_err(|_| McpError::InvalidConfig(format!("invalid MCP OAuth {label}")))?;
    if !url.username().is_empty() || url.password().is_some() || url.host_str().is_none() {
        return Err(McpError::InvalidConfig(format!(
            "invalid MCP OAuth {label}"
        )));
    }
    let loopback_http = url.scheme() == "http"
        && resource.scheme() == "http"
        && resource
            .host_str()
            .and_then(|host| host.parse::<std::net::IpAddr>().ok())
            .is_some_and(|address| address.is_loopback())
        && url
            .host_str()
            .and_then(|host| host.parse::<std::net::IpAddr>().ok())
            .is_some_and(|address| address.is_loopback());
    if url.scheme() != "https" && !loopback_http {
        return Err(McpError::InvalidConfig(format!(
            "MCP OAuth {label} must use HTTPS or literal-loopback HTTP"
        )));
    }
    Ok(url)
}

fn validate_http_url(value: &str) -> Result<(), McpError> {
    let url = reqwest::Url::parse(value)
        .map_err(|_| McpError::InvalidConfig("invalid Streamable HTTP URL".into()))?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err(McpError::InvalidConfig(
            "Streamable HTTP URL cannot contain credentials".into(),
        ));
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(McpError::InvalidConfig(
            "Streamable HTTP URL cannot contain a query or fragment".into(),
        ));
    }
    let host = url
        .host_str()
        .ok_or_else(|| McpError::InvalidConfig("Streamable HTTP URL requires a host".into()))?;
    match url.scheme() {
        "https" => Ok(()),
        "http" => {
            let loopback = host
                .parse::<std::net::IpAddr>()
                .is_ok_and(|address| address.is_loopback());
            if loopback {
                Ok(())
            } else {
                Err(McpError::InvalidConfig(
                    "plain HTTP MCP is allowed only for a literal loopback address".into(),
                ))
            }
        }
        _ => Err(McpError::InvalidConfig(
            "Streamable HTTP MCP URL must use https or loopback http".into(),
        )),
    }
}

fn validate_http_headers(headers: &HashMap<String, String>) -> Result<(), McpError> {
    if headers.len() > 32 {
        return Err(McpError::InvalidConfig(
            "Streamable HTTP MCP accepts at most 32 headers".into(),
        ));
    }
    for (name, value) in headers {
        let parsed = reqwest::header::HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| McpError::InvalidConfig("invalid Streamable HTTP header name".into()))?;
        let normalized = parsed.as_str();
        if is_reserved_http_header(normalized) {
            return Err(McpError::InvalidConfig(
                "reserved Streamable HTTP header is not configurable".into(),
            ));
        }
        if exact_environment_reference(value).is_none() {
            return Err(McpError::InvalidConfig(
                "Streamable HTTP header values must be exact ${ENV_VAR} references".into(),
            ));
        }
    }
    Ok(())
}

fn is_reserved_http_header(name: &str) -> bool {
    matches!(
        name,
        "accept"
            | "content-type"
            | "content-length"
            | "host"
            | "cookie"
            | "set-cookie"
            | "proxy-authorization"
            | "proxy-authenticate"
            | "connection"
            | "transfer-encoding"
            | "upgrade"
            | "te"
            | "trailer"
            | "mcp-session-id"
            | "mcp-protocol-version"
            | "last-event-id"
            | "forwarded"
            | "via"
    ) || name.starts_with("x-forwarded-")
}

fn exact_environment_reference(value: &str) -> Option<&str> {
    let name = value.strip_prefix("${")?.strip_suffix('}')?;
    if name.is_empty()
        || name.len() > 128
        || !name.chars().all(|character| {
            character.is_ascii_uppercase() || character.is_ascii_digit() || character == '_'
        })
        || name.as_bytes()[0].is_ascii_digit()
    {
        return None;
    }
    Some(name)
}

fn filtered_environment() -> BTreeMap<String, String> {
    ["PATH", "HOME", "LANG", "LC_ALL", "TERM"]
        .into_iter()
        .filter_map(|name| crate::run_environment::var(name).map(|value| (name.into(), value)))
        .collect()
}

fn filtered_server_environment(
    configured: &HashMap<String, String>,
) -> Result<BTreeMap<String, String>, McpError> {
    let mut result = BTreeMap::new();
    for (name, value) in configured {
        if name.is_empty()
            || !name
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '_')
            || name.starts_with("DYLD_")
            || name.starts_with("LD_")
            || matches!(name.as_str(), "PATH" | "HOME" | "TMPDIR")
        {
            return Err(McpError::InvalidConfig(format!(
                "unsafe MCP environment variable {name:?}"
            )));
        }
        result.insert(name.clone(), value.clone());
    }
    Ok(result)
}

fn resolve_executable(command: &str, environment: &BTreeMap<String, String>) -> Option<PathBuf> {
    let path = Path::new(command);
    if path.is_absolute() {
        return path.is_file().then(|| path.to_path_buf());
    }
    if command.contains('/') {
        return None;
    }
    environment.get("PATH").and_then(|path_value| {
        std::env::split_paths(path_value)
            .map(|directory| directory.join(command))
            .find(|candidate| candidate.is_file())
    })
}

fn normalize_component(value: &str) -> String {
    let mut output = String::new();
    let mut underscore = false;
    for character in value.chars() {
        if character.is_ascii_alphanumeric() {
            output.push(character.to_ascii_lowercase());
            underscore = false;
        } else if !underscore {
            output.push('_');
            underscore = true;
        }
    }
    output.trim_matches('_').chars().take(48).collect()
}

fn map_tools(server: &str, mut tools: Vec<rmcp::model::Tool>) -> Result<Vec<RemoteTool>, McpError> {
    if tools.len() > MAX_TOOLS {
        return Err(McpError::Protocol {
            server: server.into(),
            detail: format!("advertised more than {MAX_TOOLS} tools"),
        });
    }
    tools.sort_by(|left, right| left.name.as_ref().cmp(right.name.as_ref()));
    let builtins: HashSet<String> = ToolRegistry::builtins()
        .specs()
        .iter()
        .map(|spec| spec.name.to_string())
        .collect();
    let mut used = builtins;
    let mut mapped = Vec::with_capacity(tools.len());
    let mut catalog_bytes = 0usize;
    for tool in tools {
        let remote_name = tool.name.into_owned();
        let server_component = normalize_component(server);
        let tool_component = normalize_component(&remote_name);
        if server_component.is_empty() || tool_component.is_empty() {
            return Err(McpError::Protocol {
                server: server.into(),
                detail: format!("invalid tool name {remote_name:?}"),
            });
        }
        let base = format!("mcp__{server_component}__{tool_component}");
        let lossy = server_component != server.to_ascii_lowercase()
            || tool_component != remote_name.to_ascii_lowercase();
        let exposed_name = if !lossy && used.insert(base.clone()) {
            base
        } else {
            let digest = hex_digest(format!("{server}\0{remote_name}").as_bytes());
            let value = format!("{base}__{}", &digest[..8]);
            if !used.insert(value.clone()) {
                return Err(McpError::Protocol {
                    server: server.into(),
                    detail: "tool namespace collision".into(),
                });
            }
            value
        };
        let schema = serde_json::Value::Object((*tool.input_schema).clone());
        let schema_bytes = serde_json::to_vec(&schema).map_or(usize::MAX, |bytes| bytes.len());
        if schema_bytes > MAX_SCHEMA_BYTES {
            return Err(McpError::Protocol {
                server: server.into(),
                detail: format!("schema for {remote_name} is too large"),
            });
        }
        let description: String = tool
            .description
            .map(|value| value.into_owned())
            .unwrap_or_else(|| format!("Tool provided by MCP server {server}"))
            .chars()
            .take(4096)
            .collect();
        catalog_bytes = catalog_bytes
            .saturating_add(exposed_name.len())
            .saturating_add(description.len())
            .saturating_add(schema_bytes);
        if catalog_bytes > MAX_CATALOG_BYTES {
            return Err(McpError::Protocol {
                server: server.into(),
                detail: format!("tool catalog exceeded {MAX_CATALOG_BYTES} bytes"),
            });
        }
        mapped.push(RemoteTool {
            exposed_name,
            kind: RemoteToolKind::Tool(remote_name),
            description,
            schema,
        });
    }
    Ok(mapped)
}

fn sort_resources(resources: &mut [Resource]) {
    resources.sort_by(|left, right| {
        left.uri
            .cmp(&right.uri)
            .then_with(|| left.name.cmp(&right.name))
    });
}

fn validate_resources(server: &str, resources: &[Resource]) -> Result<(), McpError> {
    if resources.len() > MAX_RESOURCES {
        return Err(McpError::Protocol {
            server: server.into(),
            detail: format!("advertised more than {MAX_RESOURCES} resources"),
        });
    }
    let bytes = resources.iter().fold(0usize, |total, resource| {
        total
            .saturating_add(resource.uri.len())
            .saturating_add(resource.name.len())
            .saturating_add(resource.title.as_ref().map_or(0, String::len))
            .saturating_add(resource.description.as_ref().map_or(0, String::len))
            .saturating_add(resource.mime_type.as_ref().map_or(0, String::len))
    });
    if bytes > MAX_RESOURCE_METADATA_BYTES {
        return Err(McpError::Protocol {
            server: server.into(),
            detail: format!("resource metadata exceeded {MAX_RESOURCE_METADATA_BYTES} bytes"),
        });
    }
    Ok(())
}

fn add_resource_tool(server: &str, resources: &[Resource], tools: &mut Vec<RemoteTool>) {
    if resources.is_empty() {
        return;
    }
    let base = format!("mcp__{}__read_resource", normalize_component(server));
    let exposed_name = if tools.iter().any(|tool| tool.exposed_name == base) {
        let digest = hex_digest(format!("{server}\0__ca_read_resource").as_bytes());
        format!("{base}__{}", &digest[..8])
    } else {
        base
    };
    let advertised = resources
        .iter()
        .take(16)
        .map(|resource| resource.uri.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    tools.push(RemoteTool {
        exposed_name,
        kind: RemoteToolKind::ReadResource,
        description: format!(
            "Read one advertised text resource from MCP server {server}. Advertised URIs: {advertised}"
        ),
        schema: serde_json::json!({
            "type":"object",
            "properties":{"uri":{"type":"string"}},
            "required":["uri"],
            "additionalProperties":false
        }),
    });
}

fn bounded_tool_text(
    content: &[ContentBlock],
    structured: Option<&serde_json::Value>,
) -> Result<String, McpError> {
    let mut output = String::new();
    for block in content {
        match block {
            ContentBlock::Text(text) => {
                if let Err(error) = crate::provider_filter::scan_outbound(text.text.as_bytes()) {
                    return Ok(crate::provider_filter::withheld_message(&error));
                }
                append_bounded(&mut output, &text.text, MAX_OUTPUT_BYTES)?
            }
            _ => return Err(McpError::BinaryContent),
        }
    }
    if output.is_empty() {
        if let Some(structured) = structured {
            let value = serde_json::to_string(structured).map_err(|error| McpError::Protocol {
                server: "unknown".into(),
                detail: error.to_string(),
            })?;
            if let Err(error) = crate::provider_filter::scan_outbound(value.as_bytes()) {
                return Ok(crate::provider_filter::withheld_message(&error));
            }
            append_bounded(&mut output, &value, MAX_OUTPUT_BYTES)?;
        }
    }
    Ok(crate::terminal_text::sanitize_terminal_text(&output))
}

fn sanitize_host_structured_content(
    server: &str,
    structured: &serde_json::Value,
    secrets: &[String],
) -> Result<serde_json::Value, McpError> {
    let encoded = serde_json::to_string(structured).map_err(|_| McpError::Protocol {
        server: server.into(),
        detail: "host tool structured content was invalid".into(),
    })?;
    if encoded.len() > MAX_OUTPUT_BYTES {
        return Err(McpError::OutputLimit(MAX_OUTPUT_BYTES));
    }
    serde_json::from_str(&redact_values(&encoded, secrets)).map_err(|_| McpError::Protocol {
        server: server.into(),
        detail: "host tool structured content could not be sanitized".into(),
    })
}

fn remote_tool_error(
    server: &str,
    structured: Option<&serde_json::Value>,
    fallback_text: &str,
) -> McpError {
    let typed = structured
        .filter(|value| {
            value.get("schema").and_then(serde_json::Value::as_str)
                == Some("codex-team.mcp_error.v1")
        })
        .and_then(|value| value.get("error"))
        .filter(|value| value.is_object());
    let code = typed
        .and_then(|error| error.get("code"))
        .and_then(serde_json::Value::as_str)
        .filter(|code| {
            !code.is_empty()
                && code.len() <= 128
                && code
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        })
        .unwrap_or("REMOTE_TOOL_ERROR")
        .to_string();
    let message = typed
        .and_then(|error| error.get("message"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or(fallback_text);
    let message = crate::terminal_text::sanitize_terminal_text(message)
        .chars()
        .take(4_096)
        .collect();
    let retryable = typed
        .and_then(|error| error.get("retryable"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    McpError::Tool {
        server: server.into(),
        code,
        message,
        retryable,
    }
}

fn append_bounded(output: &mut String, value: &str, limit: usize) -> Result<(), McpError> {
    if output.len().saturating_add(value.len()) > limit {
        return Err(McpError::OutputLimit(limit));
    }
    output.push_str(value);
    Ok(())
}

fn redact_values(value: &str, secrets: &[String]) -> String {
    secrets.iter().fold(value.to_string(), |redacted, secret| {
        if secret.is_empty() {
            redacted
        } else {
            redacted.replace(secret, "[REDACTED]")
        }
    })
}

fn hex_digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::transport::auth::StoredCredentials;
    use std::sync::Mutex as StdMutex;
    use tempfile::TempDir;

    #[derive(Debug, Clone)]
    struct HttpFixtureRequest {
        method: String,
        path: String,
        headers: HashMap<String, String>,
        body: Option<serde_json::Value>,
    }

    #[test]
    fn remote_tool_order_is_independent_of_server_and_discovery_insertion() {
        fn tool(name: &str) -> RemoteTool {
            RemoteTool {
                exposed_name: name.into(),
                kind: RemoteToolKind::Tool(name.into()),
                description: name.into(),
                schema: serde_json::json!({"type":"object"}),
            }
        }
        fn order(catalog: &HashMap<String, Vec<RemoteTool>>) -> Vec<(String, String)> {
            let mut entries = catalog
                .iter()
                .flat_map(|(server, tools)| tools.iter().map(move |tool| (server.as_str(), tool)))
                .collect::<Vec<_>>();
            sort_remote_tool_entries(&mut entries);
            entries
                .into_iter()
                .map(|(server, tool)| (server.into(), tool.exposed_name.clone()))
                .collect()
        }
        fn serialized(catalog: &HashMap<String, Vec<RemoteTool>>) -> Vec<u8> {
            let mut entries = catalog
                .iter()
                .flat_map(|(server, tools)| tools.iter().map(move |tool| (server.as_str(), tool)))
                .collect::<Vec<_>>();
            sort_remote_tool_entries(&mut entries);
            let specs = entries
                .into_iter()
                .map(|(server, tool)| {
                    crate::agent::registry::ToolSpec::mcp(
                        &tool.exposed_name,
                        server,
                        tool.description.clone(),
                        tool.schema.clone(),
                    )
                    .unwrap()
                })
                .collect::<Vec<_>>();
            let definitions = crate::agent::registry::ToolRegistry::new(specs)
                .unwrap()
                .definitions(crate::agent::registry::ToolProfile::Mcp);
            serde_json::to_vec(&definitions).unwrap()
        }

        let alpha = vec![tool("mcp__alpha__zebra"), tool("mcp__alpha__apple")];
        let beta = vec![tool("mcp__beta__apple")];
        let mut first = HashMap::new();
        first.insert("beta".into(), beta.clone());
        first.insert("alpha".into(), alpha.clone());
        let mut second = HashMap::new();
        second.insert("alpha".into(), alpha);
        second.insert("beta".into(), beta);

        assert_eq!(order(&first), order(&second));
        assert_eq!(
            serialized(&first),
            serialized(&second),
            "equivalent MCP catalogs must produce byte-identical tool declarations"
        );
        assert_eq!(
            order(&first),
            [
                ("alpha".into(), "mcp__alpha__apple".into()),
                ("alpha".into(), "mcp__alpha__zebra".into()),
                ("beta".into(), "mcp__beta__apple".into()),
            ]
        );
    }

    #[test]
    fn advertised_tools_are_mapped_in_canonical_name_order() {
        let tools: Vec<rmcp::model::Tool> = serde_json::from_value(serde_json::json!([
            {"name":"zebra","inputSchema":{"type":"object"}},
            {"name":"apple","inputSchema":{"type":"object"}}
        ]))
        .unwrap();

        let mapped = map_tools("server", tools).unwrap();

        assert_eq!(
            mapped
                .iter()
                .map(|tool| tool.exposed_name.as_str())
                .collect::<Vec<_>>(),
            ["mcp__server__apple", "mcp__server__zebra"]
        );
    }

    fn workspace(temp: &TempDir) -> WorkspaceContext {
        WorkspaceContext::from_primary(temp.path().to_path_buf(), temp.path().to_path_buf())
    }

    fn run_test_command(
        executable: &Path,
        current_dir: &Path,
        args: &[&str],
    ) -> anyhow::Result<std::process::Output> {
        let output = std::process::Command::new(executable)
            .args(args)
            .current_dir(current_dir)
            .output()?;
        anyhow::ensure!(
            output.status.success(),
            "test command failed with status {:?}",
            output.status.code()
        );
        Ok(output)
    }

    fn prepare_real_codex_team_test_repo(
        codex_team: &Path,
        repo: &Path,
        mission: &str,
        no_work: bool,
    ) -> anyhow::Result<()> {
        run_test_command(Path::new("git"), repo, &["init", "--quiet"])?;
        run_test_command(
            Path::new("git"),
            repo,
            &["config", "user.email", "ca-mcp-test@example.invalid"],
        )?;
        run_test_command(
            Path::new("git"),
            repo,
            &["config", "user.name", "ca MCP Test"],
        )?;
        run_test_command(
            codex_team,
            repo,
            &[
                "init",
                "--repo",
                repo.to_str()
                    .ok_or_else(|| anyhow::anyhow!("temporary repository path is not UTF-8"))?,
                "--mission",
                mission,
                "--commit-mode",
                "manual",
                "--require-clean-start",
            ],
        )?;
        run_test_command(
            Path::new("git"),
            repo,
            &[
                "add",
                "ROADMAP.md",
                "NEXT_STEPS.md",
                "NEXT_STEPS_non-ai.md",
                "AGENTS.md",
            ],
        )?;
        run_test_command(
            Path::new("git"),
            repo,
            &["commit", "--quiet", "-m", "initialize ca MCP target"],
        )?;
        if no_work {
            let state_path = repo.join(".orchestrator/state.json");
            let mut state: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&state_path)?)?;
            state["project_complete"] = serde_json::Value::Bool(true);
            state["master_due"] = serde_json::Value::Bool(false);
            state["planner_due"] = serde_json::Value::Bool(false);
            state["needs_reconciliation"] = serde_json::Value::Bool(false);
            std::fs::write(&state_path, serde_json::to_vec_pretty(&state)?)?;
        }
        Ok(())
    }

    fn real_codex_team_manager(codex_team: &Path, repo: &Path, access: &str) -> McpManager {
        real_codex_team_manager_with_workspace(codex_team, repo, access, repo)
    }

    fn real_codex_team_manager_with_workspace(
        codex_team: &Path,
        repo: &Path,
        access: &str,
        workspace_root: &Path,
    ) -> McpManager {
        let mut config = CaConfig::default();
        config.mcp.servers.insert(
            "codex_team".into(),
            McpServerConfig {
                transport: McpTransport::Stdio,
                command: "/usr/bin/env".into(),
                args: vec![
                    "CODEX_TEAM_TEST_ISOLATION_READY=1".into(),
                    codex_team.to_string_lossy().into_owned(),
                    "mcp".into(),
                    "--repo".into(),
                    repo.to_string_lossy().into_owned(),
                    "--access".into(),
                    access.into(),
                    "--job-timeout-seconds".into(),
                    "60".into(),
                ],
                env: HashMap::new(),
                url: None,
                headers: HashMap::new(),
                enabled: true,
                timeout_secs: 60,
                network: false,
                oauth: Default::default(),
            },
        );
        if access == "delegate" {
            config.mcp.mechanistic = crate::config::MechanisticMcpConfig {
                enabled: true,
                server: "codex_team".into(),
                max_wait_seconds: 900,
                ..crate::config::MechanisticMcpConfig::default()
            };
        }
        McpManager::load(
            &config,
            WorkspaceContext::from_primary(
                workspace_root.to_path_buf(),
                workspace_root.to_path_buf(),
            ),
            SandboxMode::PolicyOnly,
            false,
        )
        .expect("load real codex-team MCP manager")
    }

    struct ScopedTestHome(Option<std::ffi::OsString>);

    impl ScopedTestHome {
        fn set(path: &Path) -> Self {
            let previous = std::env::var_os("HOME");
            std::env::set_var("HOME", path);
            Self(previous)
        }
    }

    impl Drop for ScopedTestHome {
        fn drop(&mut self) {
            if let Some(previous) = self.0.take() {
                std::env::set_var("HOME", previous);
            } else {
                std::env::remove_var("HOME");
            }
        }
    }

    fn successful_mcp_json(
        result: crate::agent::registry::ToolExecutionResult,
    ) -> anyhow::Result<serde_json::Value> {
        match result {
            crate::agent::registry::ToolExecutionResult::Success(text) => {
                Ok(serde_json::from_str(&text)?)
            }
            _ => anyhow::bail!("real codex-team MCP tool did not return JSON success text"),
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    #[ignore = "requires CODEX_TEAM_BIN pointing to the built integration binary"]
    async fn real_codex_team_mcp_manager_11_15_28_catalog_compatibility() -> anyhow::Result<()> {
        let Some(codex_team) = std::env::var_os("CODEX_TEAM_BIN") else {
            anyhow::bail!("set CODEX_TEAM_BIN to run the real compatibility test");
        };
        let codex_team = std::fs::canonicalize(PathBuf::from(codex_team))?;
        let metadata = std::fs::symlink_metadata(&codex_team)?;
        anyhow::ensure!(
            metadata.is_file() && !metadata.file_type().is_symlink(),
            "CODEX_TEAM_BIN must be a regular, non-symlink file"
        );

        let import_temp = tempfile::tempdir()?;
        let import_repo = std::fs::canonicalize(import_temp.path())?;
        let test_home = import_repo
            .join(".orchestrator")
            .join("codex-team-test-home");
        std::fs::create_dir_all(&test_home)?;
        let _home = ScopedTestHome::set(&test_home);
        prepare_real_codex_team_test_repo(
            &codex_team,
            &import_repo,
            "Real ca MCP typed import compatibility",
            false,
        )?;
        std::fs::create_dir(import_repo.join("docs"))?;
        std::fs::write(
            import_repo.join("docs").join("delegation.txt"),
            "orchestration_available\ndelegate_mechanistic\n",
        )?;
        run_test_command(
            Path::new("git"),
            &import_repo,
            &["add", "docs/delegation.txt"],
        )?;
        run_test_command(
            Path::new("git"),
            &import_repo,
            &["commit", "--quiet", "-m", "add delegation search fixture"],
        )?;

        let observe = real_codex_team_manager(&codex_team, &import_repo, "observe");
        let observe_catalog = observe.catalog().await;
        anyhow::ensure!(
            observe_catalog.diagnostics.is_empty(),
            "real observe server reported diagnostics"
        );
        anyhow::ensure!(
            observe_catalog.resources.is_empty(),
            "observe resources were not empty"
        );
        let mut observe_names = observe_catalog
            .registry
            .as_ref()
            .expect("observe registry")
            .specs()
            .iter()
            .map(|spec| spec.name.to_string())
            .filter(|name| name.starts_with("mcp__codex_team__"))
            .collect::<Vec<_>>();
        observe_names.sort();
        assert_eq!(
            observe_names,
            [
                "mcp__codex_team__doctor",
                "mcp__codex_team__recipe_list",
                "mcp__codex_team__recipe_preview",
                "mcp__codex_team__repo_query",
                "mcp__codex_team__self_improve_deployments",
                "mcp__codex_team__self_improve_evaluations",
                "mcp__codex_team__self_improve_objectives",
                "mcp__codex_team__self_improve_status",
                "mcp__codex_team__self_improve_traces",
                "mcp__codex_team__status",
                "mcp__codex_team__step_status",
            ]
        );
        let recipes = observe
            .call_host_tool_structured(
                "codex_team",
                "recipe_list",
                serde_json::json!({"global": false}),
            )
            .await?;
        assert_eq!(recipes["schema"], "codex-team.recipe_list.v1");
        assert!(
            !observe
                .has_active_tool("mcp__codex_team__recipe_list")
                .await
        );
        let observed_status = successful_mcp_json(
            observe
                .call_tool("mcp__codex_team__status", serde_json::json!({}))
                .await?,
        )?;
        assert_eq!(observed_status["observe"], true);
        assert_eq!(observed_status["commit_mode"], "manual");
        observe.shutdown().await;

        let launch_temp = tempfile::tempdir()?;
        let launch_workspace = std::fs::canonicalize(launch_temp.path())?;
        let delegate = std::sync::Arc::new(real_codex_team_manager_with_workspace(
            &codex_team,
            &import_repo,
            "delegate",
            &launch_workspace,
        ));
        delegate
            .validate_mechanistic_delegate_server("codex_team")
            .await?;
        let delegate_catalog = delegate.catalog().await;
        anyhow::ensure!(
            delegate_catalog.resources.is_empty(),
            "delegate resources were not empty"
        );
        let mut delegate_names = delegate_catalog
            .registry
            .as_ref()
            .expect("delegate registry")
            .specs()
            .iter()
            .map(|spec| spec.name.to_string())
            .filter(|name| name.starts_with("mcp__codex_team__"))
            .collect::<Vec<_>>();
        delegate_names.sort();
        assert_eq!(
            delegate_names,
            [
                "mcp__codex_team__delegation_cancel",
                "mcp__codex_team__delegation_feedback",
                "mcp__codex_team__delegation_start",
                "mcp__codex_team__delegation_status",
                "mcp__codex_team__doctor",
                "mcp__codex_team__recipe_list",
                "mcp__codex_team__recipe_preview",
                "mcp__codex_team__repo_query",
                "mcp__codex_team__self_improve_deployments",
                "mcp__codex_team__self_improve_evaluations",
                "mcp__codex_team__self_improve_objectives",
                "mcp__codex_team__self_improve_status",
                "mcp__codex_team__self_improve_traces",
                "mcp__codex_team__status",
                "mcp__codex_team__step_status",
            ]
        );
        let binding = crate::mechanistic_delegation::repository_binding(&delegate).await?;
        assert_eq!(
            std::path::Path::new(&binding.canonical_root),
            import_repo.as_path()
        );
        assert_ne!(
            std::path::Path::new(&binding.canonical_root),
            launch_workspace.as_path()
        );
        let transparent = crate::mechanistic_delegation::execute_transparent_calls(
            delegate.clone(),
            &[
                crate::mechanistic_delegation::TransparentToolRequest {
                    name: "list_dir".into(),
                    arguments: serde_json::json!({"path":"docs"}),
                },
                crate::mechanistic_delegation::TransparentToolRequest {
                    name: "search_files".into(),
                    arguments: serde_json::json!({
                        "pattern":"orchestration_available|delegate_mechanistic",
                        "path":"docs"
                    }),
                },
            ],
            false,
        )
        .await?;
        assert_eq!(transparent.executions.len(), 2);
        assert!(transparent
            .executions
            .iter()
            .all(|result| result.is_success()));
        assert!(transparent.executions[0]
            .model_text()
            .contains("delegation.txt"));
        assert!(transparent.executions[1]
            .model_text()
            .contains("orchestration_available"));
        let capsule = transparent.capsule.expect("transparent evidence capsule");
        assert_eq!(capsule.revision, binding.revision);
        assert_eq!(capsule.digest.len(), 64);
        let delegated = crate::mechanistic_delegation::execute(
            delegate.clone(),
            &serde_json::json!({
                "actions": [{
                    "kind": "search_text",
                    "pattern": "orchestration_available|delegate_mechanistic",
                    "match_mode": "regex",
                    "paths": ["docs"],
                    "max_matches": 8
                }]
            }),
        )
        .await;
        let delegated = successful_mcp_json(delegated)?;
        assert_eq!(delegated["status"], "completed");
        assert_eq!(delegated["fallback_allowed"], false);
        assert_eq!(
            delegated["receipt"]["action_results"][0]["entries"]
                .as_array()
                .expect("search entries")
                .len(),
            2
        );

        let rejected = crate::mechanistic_delegation::execute(
            delegate.clone(),
            &serde_json::json!({
                "actions": [{
                    "kind": "search_text",
                    "pattern": "(",
                    "match_mode": "regex",
                    "paths": ["docs"],
                    "max_matches": 8
                }]
            }),
        )
        .await;
        let rejected = match rejected {
            crate::agent::registry::ToolExecutionResult::Failure(text) => {
                serde_json::from_str::<serde_json::Value>(&text)?
            }
            other => anyhow::bail!("invalid delegated regex was not rejected: {other:?}"),
        };
        assert_eq!(rejected["status"], "failed");
        assert_eq!(rejected["fallback_allowed"], false);
        assert_eq!(rejected["error"]["code"], "INVALID_ARGUMENT");
        assert_eq!(rejected["error"]["retryable"], true);
        assert!(!rejected["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("protocol failure"));
        delegate.shutdown().await;

        let operate = real_codex_team_manager(&codex_team, &import_repo, "operate");
        let operate_catalog = operate.catalog().await;
        anyhow::ensure!(
            operate_catalog.diagnostics.is_empty(),
            "real operate server reported diagnostics: {:?}",
            operate_catalog.diagnostics
        );
        anyhow::ensure!(
            operate_catalog.resources.is_empty(),
            "operate resources were not empty"
        );
        let mut operate_names = operate_catalog
            .registry
            .as_ref()
            .expect("operate registry")
            .specs()
            .iter()
            .map(|spec| spec.name.to_string())
            .filter(|name| name.starts_with("mcp__codex_team__"))
            .collect::<Vec<_>>();
        operate_names.sort();
        assert_eq!(
            operate_names,
            [
                "mcp__codex_team__commit_pending",
                "mcp__codex_team__delegation_cancel",
                "mcp__codex_team__delegation_feedback",
                "mcp__codex_team__delegation_start",
                "mcp__codex_team__delegation_status",
                "mcp__codex_team__disposition",
                "mcp__codex_team__doctor",
                "mcp__codex_team__recipe_approve",
                "mcp__codex_team__recipe_list",
                "mcp__codex_team__recipe_preview",
                "mcp__codex_team__recipe_reactivate",
                "mcp__codex_team__recipe_reject",
                "mcp__codex_team__recipe_retire",
                "mcp__codex_team__repo_query",
                "mcp__codex_team__self_improve_deployments",
                "mcp__codex_team__self_improve_evaluations",
                "mcp__codex_team__self_improve_objectives",
                "mcp__codex_team__self_improve_pause",
                "mcp__codex_team__self_improve_resume",
                "mcp__codex_team__self_improve_rollback",
                "mcp__codex_team__self_improve_status",
                "mcp__codex_team__self_improve_traces",
                "mcp__codex_team__status",
                "mcp__codex_team__step_cancel",
                "mcp__codex_team__step_start",
                "mcp__codex_team__step_status",
                "mcp__codex_team__task_import_apply",
                "mcp__codex_team__task_import_preview",
            ]
        );

        let head = String::from_utf8(
            run_test_command(Path::new("git"), &import_repo, &["rev-parse", "HEAD"])?.stdout,
        )?;
        let task = serde_json::json!({
            "schema": "codex-team.task_import.v1",
            "source": "ca",
            "base_head": head.trim(),
            "task": {
                "task_id": "INT-148",
                "title": "Real ca MCP import",
                "kind": "protocol",
                "objective": "Prove real ca McpManager typed task adoption",
                "why_unblocks_progress": "It closes the actual-client compatibility gate",
                "allowed_files": ["src/role_run.rs"],
                "read_only_files": ["src/headless.rs"],
                "forbidden_actions": ["No runtime configuration mutation"],
                "steps": ["Preview", "Reject stale apply", "Apply exact digest"],
                "acceptance_criteria": ["Imported task is present in state"],
                "verification_commands": ["cargo fmt --all --check"],
                "stop_conditions": ["Stop on workspace escape"],
                "discovery_budget": "Read only named files",
                "estimated_minutes": 30
            }
        });
        let preview = successful_mcp_json(
            operate
                .call_tool("mcp__codex_team__task_import_preview", task.clone())
                .await?,
        )?;
        assert_eq!(preview["valid"], true);
        let digest = preview["import_digest"]
            .as_str()
            .expect("preview digest")
            .to_string();
        let revision = preview["repository_revision"]
            .as_str()
            .expect("preview revision")
            .to_string();
        let state_path = import_repo.join(".orchestrator/state.json");
        let state_before = std::fs::read(&state_path)?;
        let stale = operate
            .call_tool(
                "mcp__codex_team__task_import_apply",
                serde_json::json!({
                    "task": task.clone(),
                    "preview_digest": digest.clone(),
                    "expected_revision": "0".repeat(64)
                }),
            )
            .await?;
        assert!(matches!(
            stale,
            crate::agent::registry::ToolExecutionResult::Failure(_)
        ));
        assert_eq!(std::fs::read(&state_path)?, state_before);
        let applied = successful_mcp_json(
            operate
                .call_tool(
                    "mcp__codex_team__task_import_apply",
                    serde_json::json!({
                        "task": task,
                        "preview_digest": digest,
                        "expected_revision": revision
                    }),
                )
                .await?,
        )?;
        assert_eq!(applied["outcome"], "applied");
        let state_after: serde_json::Value = serde_json::from_slice(&std::fs::read(&state_path)?)?;
        assert_eq!(state_after["queue"][0]["task_id"], "INT-148");
        operate.shutdown().await;

        let no_work_temp = tempfile::tempdir()?;
        let no_work_repo = std::fs::canonicalize(no_work_temp.path())?;
        prepare_real_codex_team_test_repo(
            &codex_team,
            &no_work_repo,
            "Real ca MCP no-work compatibility",
            true,
        )?;
        let head_before = String::from_utf8(
            run_test_command(Path::new("git"), &no_work_repo, &["rev-parse", "HEAD"])?.stdout,
        )?;
        let no_work = real_codex_team_manager(&codex_team, &no_work_repo, "operate");
        let catalog = no_work.catalog().await;
        anyhow::ensure!(
            catalog.diagnostics.is_empty(),
            "no-work server reported diagnostics"
        );
        let status = successful_mcp_json(
            no_work
                .call_tool("mcp__codex_team__status", serde_json::json!({}))
                .await?,
        )?;
        let started = successful_mcp_json(
            no_work
                .call_tool(
                    "mcp__codex_team__step_start",
                    serde_json::json!({"expected_revision": status["revision"]}),
                )
                .await?,
        )?;
        let terminal = successful_mcp_json(
            no_work
                .call_tool(
                    "mcp__codex_team__step_status",
                    serde_json::json!({
                        "job_id": started["job_id"],
                        "wait_seconds": 10
                    }),
                )
                .await?,
        )?;
        assert_eq!(terminal["status"], "completed");
        assert_eq!(terminal["cycle_receipt"]["outcome"], "no_work");
        assert_eq!(terminal["repository_may_be_partial"], false);
        no_work.shutdown().await;

        let head_after = String::from_utf8(
            run_test_command(Path::new("git"), &no_work_repo, &["rev-parse", "HEAD"])?.stdout,
        )?;
        assert_eq!(head_after, head_before);
        run_test_command(
            Path::new("git"),
            &no_work_repo,
            &["diff", "--cached", "--quiet"],
        )?;
        Ok(())
    }

    #[cfg(unix)]
    #[tokio::test]
    #[ignore = "requires CODEX_TEAM_BIN pointing to the built integration binary"]
    async fn real_codex_team_remote_automation_handoff() -> anyhow::Result<()> {
        use std::io::{Read, Write};
        use std::os::unix::fs::PermissionsExt;

        let Some(codex_team) = std::env::var_os("CODEX_TEAM_BIN") else {
            anyhow::bail!("set CODEX_TEAM_BIN to run the real remote handoff test");
        };
        let codex_team = std::fs::canonicalize(PathBuf::from(codex_team))?;
        let temp = tempfile::tempdir()?;
        let repo = std::fs::canonicalize(temp.path())?;
        prepare_real_codex_team_test_repo(
            &codex_team,
            &repo,
            "Real CA to codex-team remote automation handoff",
            false,
        )?;
        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        let server = std::thread::spawn(move || -> anyhow::Result<()> {
            for index in 0..2 {
                let (mut stream, _) = listener.accept()?;
                let mut request = Vec::new();
                let mut chunk = [0_u8; 4_096];
                loop {
                    let read = stream.read(&mut chunk)?;
                    request.extend_from_slice(&chunk[..read]);
                    let Some(header_end) =
                        request.windows(4).position(|window| window == b"\r\n\r\n")
                    else {
                        continue;
                    };
                    let headers = String::from_utf8_lossy(&request[..header_end + 4]);
                    let length = headers
                        .lines()
                        .find_map(|line| line.strip_prefix("Content-Length: "))
                        .and_then(|value| value.parse::<usize>().ok())
                        .unwrap_or(0);
                    if request.len() >= header_end + 4 + length {
                        break;
                    }
                }
                let response = if index == 0 {
                    "HTTP/1.1 409 Conflict\r\nX-Transmission-Session-Id: ca-handoff\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string()
                } else {
                    let body =
                        r#"{"result":"success","arguments":{"version":"4.0.6","rpc-version":17}}"#;
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                };
                stream.write_all(response.as_bytes())?;
            }
            Ok(())
        });
        let remote_config = repo.join("remote.toml");
        std::fs::write(
            &remote_config,
            format!(
                "schema = 'codex-team.remote_config.v1'\n[transmission.seedbox]\nurl = 'http://{address}/transmission/rpc'\ntimeout_seconds = 10\n"
            ),
        )?;
        std::fs::set_permissions(&remote_config, std::fs::Permissions::from_mode(0o600))?;

        let mut config = CaConfig::default();
        config.mcp.servers.insert(
            "codex_team".into(),
            McpServerConfig {
                transport: McpTransport::Stdio,
                command: codex_team.to_string_lossy().into_owned(),
                args: vec![
                    "mcp".into(),
                    "--repo".into(),
                    repo.to_string_lossy().into_owned(),
                    "--access".into(),
                    "delegate".into(),
                ],
                env: HashMap::from([("CODEX_TEAM_TEST_ISOLATION_READY".into(), "1".into())]),
                url: None,
                headers: HashMap::new(),
                enabled: true,
                timeout_secs: 60,
                network: false,
                oauth: Default::default(),
            },
        );
        config.mcp.mechanistic = crate::config::MechanisticMcpConfig {
            enabled: true,
            server: "codex_team".into(),
            remote: crate::config::RemoteMechanisticConfig {
                enabled: true,
                config: remote_config,
                max_wait_seconds: 60,
                env: HashMap::new(),
            },
            ..Default::default()
        };
        let manager = Arc::new(McpManager::load(
            &config,
            WorkspaceContext::from_primary(repo.clone(), repo),
            SandboxMode::PolicyOnly,
            false,
        )?);
        let results = crate::mechanistic_delegation::execute_transparent_calls(
            manager.clone(),
            &[
                crate::mechanistic_delegation::TransparentToolRequest {
                    name: "list_remote_automation".into(),
                    arguments: serde_json::json!({}),
                },
                crate::mechanistic_delegation::TransparentToolRequest {
                    name: "query_remote_rpc".into(),
                    arguments: serde_json::json!({
                        "endpoint":"seedbox",
                        "operation":"session"
                    }),
                },
            ],
            false,
        )
        .await?;
        assert_eq!(results.executions.len(), 2);
        assert!(results.executions.iter().all(|result| result.is_success()));
        assert!(results.executions[0].model_text().contains("seedbox"));
        assert!(results.executions[1].model_text().contains("RPC 17"));
        manager.shutdown().await;
        server.join().unwrap()?;
        Ok(())
    }

    #[cfg(unix)]
    #[tokio::test]
    #[ignore = "requires CODEX_TEAM_BIN pointing to the built integration binary"]
    async fn real_codex_team_guided_automation_setup_handoff() -> anyhow::Result<()> {
        use std::os::unix::fs::PermissionsExt;

        let Some(codex_team) = std::env::var_os("CODEX_TEAM_BIN") else {
            anyhow::bail!("set CODEX_TEAM_BIN to run the real setup handoff test");
        };
        let codex_team = std::fs::canonicalize(PathBuf::from(codex_team))?;
        let temp = tempfile::tempdir()?;
        let repo_path = temp.path().join("repo");
        std::fs::create_dir(&repo_path)?;
        let repo = std::fs::canonicalize(&repo_path)?;
        prepare_real_codex_team_test_repo(
            &codex_team,
            &repo,
            "Real CA to codex-team guided automation setup handoff",
            false,
        )?;
        let profiles = temp.path().join("config/profiles");
        std::fs::create_dir_all(&profiles)?;
        let profile_path = profiles.join("work.toml");
        std::fs::write(
            &profile_path,
            format!(
                "[mcp.servers.codex_team]\ntransport = 'stdio'\ncommand = '{}'\nargs = ['mcp', '--repo', '{}', '--access', 'delegate']\nenabled = true\nnetwork = false\ntimeout_secs = 60\n\n[mcp.mechanistic]\nenabled = true\nserver = 'codex_team'\n",
                codex_team.display(),
                repo.display()
            ),
        )?;
        std::fs::set_permissions(&profile_path, std::fs::Permissions::from_mode(0o600))?;

        let mut config = CaConfig::default();
        config.mcp.servers.insert(
            "codex_team".into(),
            McpServerConfig {
                transport: McpTransport::Stdio,
                command: codex_team.to_string_lossy().into_owned(),
                args: vec![
                    "mcp".into(),
                    "--repo".into(),
                    repo.to_string_lossy().into_owned(),
                    "--access".into(),
                    "delegate".into(),
                ],
                env: HashMap::new(),
                url: None,
                headers: HashMap::new(),
                enabled: true,
                timeout_secs: 60,
                network: false,
                oauth: Default::default(),
            },
        );
        config.mcp.mechanistic = crate::config::MechanisticMcpConfig {
            enabled: true,
            server: "codex_team".into(),
            ..Default::default()
        };
        let manager = Arc::new(
            McpManager::load_with_oauth_store(
                &config,
                WorkspaceContext::from_primary(repo.clone(), repo),
                SandboxMode::PolicyOnly,
                false,
                crate::runtime::ActiveProfile::Named("work".into()),
                crate::mcp_oauth::system_mcp_oauth_secret_store(),
            )?
            .with_profile_config_path(profile_path.clone()),
        );
        let draft = serde_json::json!({
            "template":"remote_automation",
            "manifest_name":"seedbox",
            "ssh_hosts":[{"name":"server","destination":"ops@example.test"}],
            "transmission":[{
                "name":"seedbox",
                "url":"https://seedbox.example/transmission/rpc",
                "username_env":"SEEDBOX_USER",
                "password_env":"SEEDBOX_PASSWORD"
            }],
            "syncs":[{
                "name":"server_to_seedbox",
                "controller":"server",
                "source":"/data/completed/",
                "destination":"seedbox:/data/incoming/",
                "delete":false
            }]
        });
        let preview = crate::mechanistic_delegation::execute_transparent_calls(
            manager.clone(),
            &[crate::mechanistic_delegation::TransparentToolRequest {
                name: "preview_automation_setup".into(),
                arguments: serde_json::json!({"draft":draft.clone()}),
            }],
            false,
        )
        .await?;
        let preview_text = preview.executions[0].model_text();
        assert!(preview_text.contains("no files were written"));
        let digest = preview_text
            .lines()
            .find_map(|line| line.strip_prefix("plan digest: "))
            .ok_or_else(|| anyhow::anyhow!("preview omitted plan digest"))?;
        let applied = crate::mechanistic_delegation::execute_transparent_calls(
            manager.clone(),
            &[crate::mechanistic_delegation::TransparentToolRequest {
                name: "apply_automation_setup".into(),
                arguments: serde_json::json!({
                    "draft":draft,
                    "accepted_plan_digest":digest
                }),
            }],
            false,
        )
        .await?;
        assert!(applied.executions[0].is_success());
        assert!(applied.executions[0]
            .model_text()
            .contains("applied to the active profile"));
        let manifest = temp.path().join("config/automation/work/seedbox.toml");
        assert!(manifest.is_file());
        let profile = std::fs::read_to_string(&profile_path)?;
        assert!(profile.contains("[mcp.mechanistic.remote]"));
        assert!(profile.contains("SEEDBOX_PASSWORD = \"${SEEDBOX_PASSWORD}\""));
        assert!(!profile.contains("ops@example.test") && !profile.contains("/data/completed/"));
        let loaded = crate::config::ConfigStore::for_profile(
            profile_path,
            crate::runtime::ActiveProfile::Named("work".into()),
        )
        .load_with_env(|_| Some("test-only-credential".into()))?;
        assert!(loaded.mcp.mechanistic.remote.enabled);
        assert_eq!(loaded.mcp.mechanistic.remote.env.len(), 2);
        manager.shutdown().await;
        Ok(())
    }

    #[derive(Default)]
    struct FakeOAuthStore(StdMutex<HashMap<String, Vec<u8>>>);

    impl crate::mcp_oauth::McpOAuthSecretStore for FakeOAuthStore {
        fn load(&self, account: &str) -> anyhow::Result<Option<Vec<u8>>> {
            Ok(self.0.lock().unwrap().get(account).cloned())
        }

        fn store(&self, account: &str, secret: &[u8]) -> anyhow::Result<()> {
            self.0.lock().unwrap().insert(account.into(), secret.into());
            Ok(())
        }

        fn delete(&self, account: &str) -> anyhow::Result<()> {
            self.0.lock().unwrap().remove(account);
            Ok(())
        }
    }

    struct NoopBrowser;

    impl crate::mcp_oauth::BrowserOpener for NoopBrowser {
        fn open(&self, _authorization_url: &str) -> anyhow::Result<()> {
            Ok(())
        }
    }

    fn stored_oauth_credentials(issuer: &str, token: &str) -> Vec<u8> {
        let response = serde_json::from_value(serde_json::json!({
            "access_token": token,
            "token_type": "Bearer",
            "expires_in": 3600,
            "refresh_token": "refresh-private-sentinel",
            "scope": "tools"
        }))
        .unwrap();
        serde_json::to_vec(
            &StoredCredentials::new(
                "fixture-client".into(),
                Some(response),
                vec!["tools".into()],
                Some(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs(),
                ),
            )
            .with_issuer(Some(issuer.into())),
        )
        .unwrap()
    }

    async fn read_http_fixture_request(stream: &mut tokio::net::TcpStream) -> HttpFixtureRequest {
        let mut bytes = Vec::new();
        let header_end = loop {
            let mut chunk = [0_u8; 4096];
            let count = stream.read(&mut chunk).await.unwrap();
            assert!(count > 0, "request closed before headers");
            bytes.extend_from_slice(&chunk[..count]);
            if let Some(index) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
                break index + 4;
            }
            assert!(
                bytes.len() <= 64 * 1024,
                "request headers exceeded fixture cap"
            );
        };
        let headers_text = std::str::from_utf8(&bytes[..header_end]).unwrap();
        let mut lines = headers_text.split("\r\n");
        let request_line = lines.next().unwrap();
        let mut request_parts = request_line.split_whitespace();
        let method = request_parts.next().unwrap().to_string();
        let path = request_parts.next().unwrap().to_string();
        let headers = lines
            .filter_map(|line| line.split_once(':'))
            .map(|(name, value)| (name.to_ascii_lowercase(), value.trim().to_string()))
            .collect::<HashMap<_, _>>();
        let content_length = headers
            .get("content-length")
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(0);
        while bytes.len() - header_end < content_length {
            let mut chunk = [0_u8; 4096];
            let count = stream.read(&mut chunk).await.unwrap();
            assert!(count > 0, "request closed before body");
            bytes.extend_from_slice(&chunk[..count]);
        }
        let body = (content_length > 0).then(|| {
            serde_json::from_slice(&bytes[header_end..header_end + content_length]).unwrap()
        });
        HttpFixtureRequest {
            method,
            path,
            headers,
            body,
        }
    }

    async fn write_http_fixture_response(
        stream: &mut tokio::net::TcpStream,
        status: &str,
        session: bool,
        body: Option<serde_json::Value>,
    ) {
        let body = body
            .map(|value| serde_json::to_vec(&value).unwrap())
            .unwrap_or_default();
        let mut headers = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n",
            body.len()
        );
        if !body.is_empty() {
            headers.push_str("Content-Type: application/json\r\n");
        }
        if session {
            headers.push_str("Mcp-Session-Id: ca-http-fixture\r\n");
        }
        headers.push_str("\r\n");
        stream.write_all(headers.as_bytes()).await.unwrap();
        stream.write_all(&body).await.unwrap();
    }

    async fn start_http_mcp_fixture() -> (
        String,
        tokio::sync::mpsc::UnboundedReceiver<HttpFixtureRequest>,
        tokio::sync::oneshot::Sender<()>,
        tokio::task::JoinHandle<()>,
    ) {
        start_http_mcp_fixture_with_initial_sse(false).await
    }

    async fn start_oauth_mcp_fixture() -> (
        String,
        tokio::sync::mpsc::UnboundedReceiver<HttpFixtureRequest>,
        tokio::sync::oneshot::Sender<()>,
        tokio::task::JoinHandle<()>,
    ) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let url = format!("{base}/mcp");
        let fixture_base = base.clone();
        let (requests_tx, requests_rx) = tokio::sync::mpsc::unbounded_channel();
        let (stop_tx, mut stop_rx) = tokio::sync::oneshot::channel();
        let handle = tokio::spawn(async move {
            loop {
                let accepted = tokio::select! {
                    _ = &mut stop_rx => break,
                    accepted = listener.accept() => accepted,
                };
                let (mut stream, _) = accepted.unwrap();
                let request = read_http_fixture_request(&mut stream).await;
                requests_tx.send(request.clone()).unwrap();
                if request.method == "GET" && request.path.contains("oauth-protected-resource") {
                    write_http_fixture_response(
                        &mut stream,
                        "200 OK",
                        false,
                        Some(serde_json::json!({
                            "resource": format!("{fixture_base}/mcp"),
                            "authorization_servers": [fixture_base]
                        })),
                    )
                    .await;
                    continue;
                }
                if request.method == "GET"
                    && (request.path.contains("oauth-authorization-server")
                        || request.path.contains("openid-configuration"))
                {
                    write_http_fixture_response(
                        &mut stream,
                        "200 OK",
                        false,
                        Some(serde_json::json!({
                            "issuer": fixture_base,
                            "authorization_endpoint": format!("{fixture_base}/authorize"),
                            "token_endpoint": format!("{fixture_base}/token"),
                            "code_challenge_methods_supported": ["S256"]
                        })),
                    )
                    .await;
                    continue;
                }
                if request.method == "DELETE" {
                    write_http_fixture_response(&mut stream, "204 No Content", false, None).await;
                    continue;
                }
                if request.headers.get("authorization").map(String::as_str)
                    != Some("Bearer access-private-sentinel")
                {
                    let body = b"never-display-auth-body";
                    let response = format!(
                        "HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Bearer resource_metadata=\"{fixture_base}/.well-known/oauth-protected-resource\"\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    stream.write_all(response.as_bytes()).await.unwrap();
                    stream.write_all(body).await.unwrap();
                    continue;
                }
                let body = request.body.as_ref();
                let method = body
                    .and_then(|body| body.get("method"))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default();
                let id = body
                    .and_then(|body| body.get("id"))
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                let result = match method {
                    "initialize" => serde_json::json!({
                        "protocolVersion":"2025-11-25",
                        "capabilities":{"tools":{},"resources":{}},
                        "serverInfo":{"name":"oauth-fixture","version":"1"}
                    }),
                    "tools/list" => serde_json::json!({"tools":[{
                        "name":"secure_echo",
                        "inputSchema":{"type":"object","properties":{}}
                    }]}),
                    "resources/list" => serde_json::json!({"resources":[]}),
                    _ => {
                        write_http_fixture_response(&mut stream, "202 Accepted", false, None).await;
                        continue;
                    }
                };
                write_http_fixture_response(
                    &mut stream,
                    "200 OK",
                    method == "initialize",
                    Some(serde_json::json!({"jsonrpc":"2.0","id":id,"result":result})),
                )
                .await;
            }
        });
        (url, requests_rx, stop_tx, handle)
    }

    async fn start_http_mcp_fixture_with_initial_sse(
        initial_sse: bool,
    ) -> (
        String,
        tokio::sync::mpsc::UnboundedReceiver<HttpFixtureRequest>,
        tokio::sync::oneshot::Sender<()>,
        tokio::task::JoinHandle<()>,
    ) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/mcp", listener.local_addr().unwrap());
        let (requests_tx, requests_rx) = tokio::sync::mpsc::unbounded_channel();
        let (stop_tx, mut stop_rx) = tokio::sync::oneshot::channel();
        let handle = tokio::spawn(async move {
            loop {
                let accepted = tokio::select! {
                    _ = &mut stop_rx => break,
                    accepted = listener.accept() => accepted,
                };
                let (mut stream, _) = accepted.unwrap();
                let request = read_http_fixture_request(&mut stream).await;
                let body = request.body.clone();
                let method = body
                    .as_ref()
                    .and_then(|body| body.get("method"))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default();
                let id = body
                    .as_ref()
                    .and_then(|body| body.get("id"))
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                requests_tx.send(request.clone()).unwrap();
                match (request.method.as_str(), method) {
                    ("GET", _) => {
                        write_http_fixture_response(
                            &mut stream,
                            "405 Method Not Allowed",
                            false,
                            None,
                        )
                        .await;
                    }
                    ("DELETE", _) => {
                        write_http_fixture_response(&mut stream, "204 No Content", false, None)
                            .await;
                    }
                    ("POST", "initialize") => {
                        let response = serde_json::json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": {
                                "protocolVersion": "2025-11-25",
                                "capabilities": {
                                    "tools": {"listChanged": true},
                                    "resources": {"subscribe": false, "listChanged": true}
                                },
                                "serverInfo": {"name": "ca-http-fixture", "version": "1.0"}
                            }
                        });
                        if initial_sse {
                            let body = format!("data: {response}\n\n");
                            let headers = format!(
                                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nMcp-Session-Id: ca-http-fixture\r\nConnection: close\r\n\r\n",
                                body.len()
                            );
                            stream.write_all(headers.as_bytes()).await.unwrap();
                            stream.write_all(body.as_bytes()).await.unwrap();
                        } else {
                            write_http_fixture_response(
                                &mut stream,
                                "200 OK",
                                true,
                                Some(response),
                            )
                            .await;
                        }
                    }
                    ("POST", "tools/list") => {
                        write_http_fixture_response(
                            &mut stream,
                            "200 OK",
                            false,
                            Some(serde_json::json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "result": {"tools": [{
                                    "name": "echo",
                                    "description": "Echo safe text",
                                    "inputSchema": {
                                        "type": "object",
                                        "properties": {"text": {"type": "string"}},
                                        "required": ["text"]
                                    }
                                }]}
                            })),
                        )
                        .await;
                    }
                    ("POST", "resources/list") => {
                        write_http_fixture_response(
                            &mut stream,
                            "200 OK",
                            false,
                            Some(serde_json::json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "result": {"resources": [{
                                    "uri": "test://http-resource",
                                    "name": "HTTP resource",
                                    "mimeType": "text/plain"
                                }]}
                            })),
                        )
                        .await;
                    }
                    ("POST", "tools/call") => {
                        let text = body
                            .as_ref()
                            .and_then(|body| body.pointer("/params/arguments/text"))
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or_default();
                        write_http_fixture_response(
                            &mut stream,
                            "200 OK",
                            false,
                            Some(serde_json::json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "result": {"content": [{"type": "text", "text": text}]}
                            })),
                        )
                        .await;
                    }
                    ("POST", "resources/read") => {
                        write_http_fixture_response(
                            &mut stream,
                            "200 OK",
                            false,
                            Some(serde_json::json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "result": {"contents": [{
                                    "uri": "test://http-resource",
                                    "mimeType": "text/plain",
                                    "text": "HTTP resource body"
                                }]}
                            })),
                        )
                        .await;
                    }
                    ("POST", _) => {
                        write_http_fixture_response(&mut stream, "202 Accepted", false, None).await;
                    }
                    _ => unreachable!(),
                }
            }
        });
        (url, requests_rx, stop_tx, handle)
    }

    async fn start_single_http_response(
        status: &str,
        content_type: Option<&str>,
        extra_headers: &str,
        body: Vec<u8>,
        chunked: bool,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/mcp", listener.local_addr().unwrap());
        let status = status.to_owned();
        let content_type = content_type.map(str::to_owned);
        let extra_headers = extra_headers.to_owned();
        let handle = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let _ = read_http_fixture_request(&mut stream).await;
            let mut headers = format!("HTTP/1.1 {status}\r\nConnection: close\r\n");
            if let Some(content_type) = content_type {
                headers.push_str(&format!("Content-Type: {content_type}\r\n"));
            }
            if chunked {
                headers.push_str("Transfer-Encoding: chunked\r\n");
            } else {
                headers.push_str(&format!("Content-Length: {}\r\n", body.len()));
            }
            headers.push_str(&extra_headers);
            headers.push_str("\r\n");
            let _ = stream.write_all(headers.as_bytes()).await;
            if chunked {
                let _ = stream
                    .write_all(format!("{:x}\r\n", body.len()).as_bytes())
                    .await;
                let _ = stream.write_all(&body).await;
                let _ = stream.write_all(b"\r\n0\r\n\r\n").await;
            } else {
                let _ = stream.write_all(&body).await;
            }
        });
        (url, handle)
    }

    fn http_manager(url: String, timeout_secs: u64) -> (TempDir, McpManager) {
        let temp = tempfile::tempdir().unwrap();
        let mut config = CaConfig::default();
        config.mcp.servers.insert(
            "http".into(),
            McpServerConfig {
                transport: McpTransport::StreamableHttp,
                command: String::new(),
                args: Vec::new(),
                env: HashMap::new(),
                url: Some(url),
                headers: HashMap::new(),
                enabled: true,
                timeout_secs,
                network: true,
                oauth: Default::default(),
            },
        );
        let manager =
            McpManager::load(&config, workspace(&temp), SandboxMode::PolicyOnly, true).unwrap();
        (temp, manager)
    }

    fn write_fake_server(temp: &TempDir) -> PathBuf {
        let script = temp.path().join("fake_mcp.py");
        std::fs::write(
            &script,
            r#"import json, os, sys, time

marker = os.environ.get("EXIT_MARKER")
mode = os.environ.get("MODE", "normal")
changed = False
sys.stderr.write("\x1b[31mfake stderr\x1b[0m\n")
sys.stderr.flush()

def send(value):
    sys.stdout.write(json.dumps(value, separators=(",", ":")) + "\n")
    sys.stdout.flush()

try:
    for line in sys.stdin:
        request = json.loads(line)
        method = request.get("method")
        request_id = request.get("id")
        if method == "initialize":
            if mode == "malformed":
                sys.stdout.write("not-json\n")
                sys.stdout.flush()
                continue
            send({"jsonrpc":"2.0","id":request_id,"result":{
                "protocolVersion":"2025-11-25",
                "capabilities":{"tools":{"listChanged":True},"resources":{"subscribe":False,"listChanged":True}},
                "serverInfo":{"name":"ca-fake","version":"1.0"}
            }})
        elif method == "tools/list":
            if mode == "timeout":
                time.sleep(5)
                continue
            cursor = (request.get("params") or {}).get("cursor")
            if cursor is None:
                send({"jsonrpc":"2.0","id":request_id,"result":{"tools":[
                    {"name":"echo","description":"Echo input","inputSchema":{"type":"object","properties":{"text":{"type":"string"}},"required":["text"],"additionalProperties":False}},
                    {"name":"inspect_env","description":"Inspect filtered environment","inputSchema":{"type":"object"}}
                ],"nextCursor":"tools-2"}})
            else:
                second_page = [
                    {"name":"binary","inputSchema":{"type":"object"}},
                    {"name":"huge","inputSchema":{"type":"object"}},
                    {"name":"network_probe","inputSchema":{"type":"object","properties":{"host":{"type":"string"},"port":{"type":"integer"}},"required":["host","port"]}}
                ]
                if changed:
                    second_page.append({"name":"refreshed","inputSchema":{"type":"object"}})
                send({"jsonrpc":"2.0","id":request_id,"result":{"tools":second_page}})
        elif method == "resources/list":
            cursor = (request.get("params") or {}).get("cursor")
            if cursor is None:
                send({"jsonrpc":"2.0","id":request_id,"result":{"resources":[
                    {"uri":"test://one","name":"One","description":"First","mimeType":"text/plain"}
                ],"nextCursor":"resources-2"}})
            else:
                send({"jsonrpc":"2.0","id":request_id,"result":{"resources":[
                    {"uri":"test://two","name":"Two","mimeType":"text/plain"}
                ]}})
        elif method == "tools/call":
            if mode == "call_timeout":
                time.sleep(5)
                continue
            name = request["params"]["name"]
            args = request["params"].get("arguments") or {}
            if name == "echo":
                content = [{"type":"text","text":args["text"]}]
            elif name == "inspect_env":
                content = [{"type":"text","text":json.dumps(sorted(os.environ.keys()))}]
            elif name == "binary":
                content = [{"type":"image","data":"AA==","mimeType":"image/png"}]
            elif name == "network_probe":
                import socket
                try:
                    connection = socket.create_connection((args["host"], args["port"]), timeout=0.5)
                    connection.close()
                    value = "connected"
                except Exception:
                    value = "denied"
                content = [{"type":"text","text":value}]
            else:
                content = [{"type":"text","text":"x" * 300000}]
            typed_error = name == "echo" and args.get("text") == "typed-error"
            result = {"content":content,"isError":typed_error}
            if typed_error:
                result["structuredContent"] = {
                    "schema":"codex-team.mcp_error.v1",
                    "error":{"code":"INVALID_ARGUMENT","message":"relative scope required; resolved-test-secret","retryable":True}
                }
            elif name == "echo":
                result["structuredContent"] = {"text": args["text"]}
            send({"jsonrpc":"2.0","id":request_id,"result":result})
            if name == "echo" and not typed_error:
                changed = True
                send({"jsonrpc":"2.0","method":"notifications/tools/list_changed"})
        elif method == "resources/read":
            uri = request["params"]["uri"]
            send({"jsonrpc":"2.0","id":request_id,"result":{"contents":[
                {"uri":uri,"mimeType":"text/plain","text":"resource:" + uri}
            ]}})
finally:
    if marker:
        with open(marker, "w") as handle:
            handle.write("closed")
"#,
        )
        .unwrap();
        script
    }

    fn fake_config(script: &Path, marker: &Path, mode: &str) -> CaConfig {
        let mut config = CaConfig::default();
        config.mcp.servers.insert(
            "fake".into(),
            McpServerConfig {
                transport: crate::config::McpTransport::Stdio,
                command: "python3".into(),
                args: vec![script.to_string_lossy().into_owned()],
                env: HashMap::from([
                    ("EXIT_MARKER".into(), marker.to_string_lossy().into_owned()),
                    ("MODE".into(), mode.into()),
                    ("TEST_TOKEN".into(), "resolved-test-secret".into()),
                ]),
                url: None,
                headers: HashMap::new(),
                enabled: true,
                timeout_secs: 1,
                network: false,
                oauth: Default::default(),
            },
        );
        config
    }

    #[tokio::test]
    async fn mechanistic_facade_preparation_is_lazy_hidden_and_rejects_wrong_catalog() {
        let temp = tempfile::tempdir().unwrap();
        let marker = temp.path().join("closed.marker");
        let script = write_fake_server(&temp);
        let mut config = fake_config(&script, &marker, "normal");
        config.mcp.mechanistic = crate::config::MechanisticMcpConfig {
            enabled: true,
            server: "fake".into(),
            max_wait_seconds: 900,
            ..crate::config::MechanisticMcpConfig::default()
        };
        let manager =
            McpManager::load(&config, workspace(&temp), SandboxMode::PolicyOnly, false).unwrap();

        let tools = crate::tool_selection::resolve_tools(
            crate::agent::registry::ToolRegistry::builtins(),
            crate::agent::registry::ToolProfile::Main,
            &crate::tool_selection::ToolSelectionPolicy::default(),
            &crate::tool_selection::ToolSelectionConstraints {
                mechanistic_delegation_enabled: true,
                ..crate::tool_selection::ToolSelectionConstraints::default()
            },
        )
        .unwrap();
        assert!(tools
            .definitions()
            .iter()
            .any(|tool| tool.function.name == "delegate_mechanistic"));
        assert_eq!(manager.statuses().await[0].state, McpServerState::Stopped);

        let search = manager.search_tools("echo", None, 5).await;
        assert!(search.model_text().contains("No MCP tools matched"));
        assert_eq!(manager.statuses().await[0].state, McpServerState::Stopped);

        let rejected = manager.validate_mechanistic_delegate_server("fake").await;
        assert!(matches!(rejected, Err(McpError::InvalidConfig(_))));
        manager.shutdown().await;
    }

    #[tokio::test]
    async fn mechanistic_activity_restores_only_schema_valid_tool_receipts() {
        let temp = tempfile::tempdir().unwrap();
        let marker = temp.path().join("closed.marker");
        let script = write_fake_server(&temp);
        let mut config = fake_config(&script, &marker, "normal");
        config.mcp.mechanistic = crate::config::MechanisticMcpConfig {
            enabled: true,
            server: "fake".into(),
            max_wait_seconds: 900,
            ..crate::config::MechanisticMcpConfig::default()
        };
        let manager =
            McpManager::load(&config, workspace(&temp), SandboxMode::PolicyOnly, false).unwrap();
        let revision = "a".repeat(64);
        let valid_output = serde_json::json!({
            "schema": "ca.mechanistic_delegation_receipt.v1",
            "job_id": "durable-job-190",
            "status": "completed",
            "fallback_allowed": false,
            "receipt": {
                "repository_revision": revision,
                "started_at": "2026-08-26T18:00:00Z",
                "finished_at": "2026-08-26T18:00:01Z"
            }
        })
        .to_string();
        let messages = vec![
            crate::app::Message {
                role: crate::app::Role::User,
                content: format!("arbitrary transcript text: {valid_output}"),
                tool_calls: None,
                thinking: None,
                response_items: Vec::new(),
                timestamp: chrono::Utc::now(),
            },
            crate::app::Message {
                role: crate::app::Role::Assistant,
                content: String::new(),
                tool_calls: Some(vec![crate::app::ToolCall {
                    id: "call-190".into(),
                    name: "delegate_mechanistic".into(),
                    arguments: "{}".into(),
                    status: crate::app::ToolStatus::Done,
                    output: Some(valid_output),
                }]),
                thinking: None,
                response_items: Vec::new(),
                timestamp: chrono::Utc::now(),
            },
        ];

        crate::mechanistic_delegation::restore_from_tool_receipts(&manager, &messages);

        let snapshots = manager.delegation_activity().snapshots();
        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].job_id, "durable-job-190");
        assert_eq!(snapshots[0].action_categories, ["restored"]);
        assert_eq!(manager.statuses().await[0].state, McpServerState::Stopped);
    }

    #[tokio::test]
    async fn project_config_requires_exact_workspace_and_hash_trust() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join(".ca")).unwrap();
        let raw = b"[servers.project]\ncommand = '/bin/echo'\n";
        std::fs::write(temp.path().join(".ca/mcp.toml"), raw).unwrap();
        let mut config = CaConfig::default();
        let manager =
            McpManager::load(&config, workspace(&temp), SandboxMode::PolicyOnly, false).unwrap();
        assert_eq!(manager.statuses().await[0].state, McpServerState::Untrusted);

        config.mcp.trusted_projects.push(ProjectMcpTrust {
            workspace: temp.path().canonicalize().unwrap(),
            sha256: hex_digest(raw),
            network: false,
        });
        let trusted =
            McpManager::load(&config, workspace(&temp), SandboxMode::PolicyOnly, false).unwrap();
        assert_eq!(trusted.statuses().await[0].state, McpServerState::Stopped);

        std::fs::write(
            temp.path().join(".ca/mcp.toml"),
            b"[servers.project]\ncommand = '/bin/echo'\nargs=['changed']\n",
        )
        .unwrap();
        let changed =
            McpManager::load(&config, workspace(&temp), SandboxMode::PolicyOnly, false).unwrap();
        assert_eq!(changed.statuses().await[0].state, McpServerState::Untrusted);
    }

    #[tokio::test]
    async fn user_precedence_and_network_grants_are_fail_closed() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join(".ca")).unwrap();
        std::fs::write(
            temp.path().join(".ca/mcp.toml"),
            "[servers.same]\ncommand='/bin/false'\n",
        )
        .unwrap();
        let mut config = CaConfig::default();
        config.mcp.servers.insert(
            "same".into(),
            McpServerConfig {
                transport: crate::config::McpTransport::Stdio,
                command: "/bin/true".into(),
                args: Vec::new(),
                env: HashMap::new(),
                url: None,
                headers: HashMap::new(),
                enabled: true,
                timeout_secs: 1,
                network: true,
                oauth: Default::default(),
            },
        );
        let manager =
            McpManager::load(&config, workspace(&temp), SandboxMode::PolicyOnly, false).unwrap();
        let statuses = manager.statuses().await;
        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].state, McpServerState::NetworkApprovalRequired);
        manager.grant_network_for_server("same").await.unwrap();
        assert_eq!(manager.statuses().await[0].state, McpServerState::Stopped);
    }

    #[test]
    fn environment_filter_and_name_mapping_reject_unsafe_inputs() {
        assert!(filtered_server_environment(&HashMap::from([(
            "DYLD_INSERT_LIBRARIES".into(),
            "x".into()
        )]))
        .is_err());
        assert!(
            filtered_server_environment(&HashMap::from([("TOKEN".into(), "secret".into())]))
                .is_ok()
        );
        assert_eq!(normalize_component("Server Name"), "server_name");
        assert!(validate_server_name("../bad").is_err());
    }

    #[test]
    fn streamable_http_config_is_explicit_and_rejects_unsafe_authority() {
        let safe = McpServerConfig {
            transport: McpTransport::StreamableHttp,
            command: String::new(),
            args: Vec::new(),
            env: HashMap::new(),
            url: Some("http://127.0.0.1:8765/mcp".into()),
            headers: HashMap::from([
                ("Authorization".into(), "${MCP_TEST_TOKEN}".into()),
                ("X-Api-Key".into(), "${MCP_TEST_KEY}".into()),
            ]),
            enabled: true,
            timeout_secs: 5,
            network: true,
            oauth: Default::default(),
        };
        validate_server_config(&safe).unwrap();
        let parsed: ProjectMcpFile =
            toml::from_str("[servers.legacy]\ncommand = '/bin/echo'\nargs = ['ok']\n").unwrap();
        assert_eq!(parsed.servers["legacy"].transport, McpTransport::Stdio);
        assert_eq!(parsed.servers["legacy"].command, "/bin/echo");

        let invalid = [
            ("non-loopback plaintext", "http://example.com/mcp"),
            ("hostname plaintext", "http://localhost/mcp"),
            ("URL credentials", "https://user:password@example.com/mcp"),
            ("URL query", "https://example.com/mcp?token=secret"),
            ("URL fragment", "https://example.com/mcp#secret"),
            ("unsupported scheme", "ftp://127.0.0.1/mcp"),
        ];
        for (label, url) in invalid {
            let mut config = safe.clone();
            config.url = Some(url.into());
            assert!(
                validate_server_config(&config).is_err(),
                "{label} must fail"
            );
        }

        let mut no_network = safe.clone();
        no_network.network = false;
        assert!(validate_server_config(&no_network).is_err());
        let mut process_fields = safe.clone();
        process_fields.command = "/bin/echo".into();
        assert!(validate_server_config(&process_fields).is_err());
        let mut stdio_fields = parsed.servers["legacy"].clone();
        stdio_fields.url = Some("https://example.com/mcp".into());
        assert!(validate_server_config(&stdio_fields).is_err());
    }

    #[test]
    fn streamable_http_headers_are_reference_only_reserved_and_debug_safe() {
        let base = McpServerConfig {
            transport: McpTransport::StreamableHttp,
            command: String::new(),
            args: Vec::new(),
            env: HashMap::new(),
            url: Some("https://sensitive-host.invalid/private".into()),
            headers: HashMap::new(),
            enabled: true,
            timeout_secs: 5,
            network: true,
            oauth: Default::default(),
        };
        for (name, value) in [
            ("X-Api-Key", "literal-secret"),
            ("X-Api-Key", "prefix-${MCP_TOKEN}"),
            ("X-Api-Key", "${lowercase}"),
            ("X-Api-Key", "${1STARTS_WITH_DIGIT}"),
            ("Accept", "${MCP_TOKEN}"),
            ("Cookie", "${MCP_TOKEN}"),
            ("Mcp-Session-Id", "${MCP_TOKEN}"),
            ("X-Forwarded-Host", "${MCP_TOKEN}"),
        ] {
            let mut config = base.clone();
            config.headers.insert(name.into(), value.into());
            assert!(
                validate_server_config(&config).is_err(),
                "{name}={value} must fail"
            );
        }

        let mut safe = base;
        safe.headers
            .insert("X-Api-Key".into(), "${MCP_TOKEN}".into());
        validate_server_config(&safe).unwrap();
        let debug = format!("{safe:?}");
        assert!(debug.contains("StreamableHttp"));
        assert!(debug.contains("header_count: 1"));
        for forbidden in ["sensitive-host", "private", "X-Api-Key", "MCP_TOKEN"] {
            assert!(!debug.contains(forbidden));
        }
    }

    #[test]
    fn oauth_configuration_is_opt_in_transport_bound_and_debug_safe() {
        let mut config = McpServerConfig {
            transport: McpTransport::StreamableHttp,
            command: String::new(),
            args: Vec::new(),
            env: HashMap::new(),
            url: Some("https://resource.invalid/mcp".into()),
            headers: HashMap::new(),
            enabled: true,
            timeout_secs: 5,
            network: true,
            oauth: crate::config::McpOAuthConfig {
                enabled: true,
                flow: crate::config::McpOAuthFlow::Browser,
                issuer: Some("https://login.invalid/tenant".into()),
                client_id: Some("public-client".into()),
                client_secret: Some("${MCP_CLIENT_SECRET}".into()),
                client_metadata_url: None,
                scopes: vec!["tools.read".into()],
            },
        };
        validate_server_config(&config).unwrap();
        let debug = format!("{config:?}");
        for secret in [
            "resource.invalid",
            "login.invalid",
            "public-client",
            "MCP_CLIENT_SECRET",
            "tools.read",
        ] {
            assert!(!debug.contains(secret));
        }

        config
            .headers
            .insert("Authorization".into(), "${MCP_ACCESS_TOKEN}".into());
        assert!(validate_server_config(&config)
            .unwrap_err()
            .to_string()
            .contains("cannot be combined"));
        config.headers.clear();

        config.oauth.client_secret = Some("literal-secret".into());
        assert!(validate_server_config(&config).is_err());
        config.oauth.client_secret = None;
        config.oauth.client_metadata_url = Some("https://client.invalid/metadata.json".into());
        assert!(validate_server_config(&config).is_err());
        config.oauth.client_id = None;
        validate_server_config(&config).unwrap();

        config.oauth.flow = crate::config::McpOAuthFlow::Device;
        assert!(validate_server_config(&config).is_err());
        config.oauth.client_id = Some("device-client".into());
        config.oauth.client_metadata_url = None;
        validate_server_config(&config).unwrap();

        config.transport = McpTransport::Stdio;
        config.command = "/bin/true".into();
        config.url = None;
        config.network = false;
        assert!(validate_server_config(&config)
            .unwrap_err()
            .to_string()
            .contains("OAuth"));
    }

    #[test]
    fn oauth_endpoint_validation_rejects_unsafe_authority_before_io() {
        let base = McpServerConfig {
            transport: McpTransport::StreamableHttp,
            command: String::new(),
            args: Vec::new(),
            env: HashMap::new(),
            url: Some("https://resource.invalid/mcp".into()),
            headers: HashMap::new(),
            enabled: true,
            timeout_secs: 5,
            network: true,
            oauth: crate::config::McpOAuthConfig {
                enabled: true,
                client_id: Some("client".into()),
                ..Default::default()
            },
        };
        for issuer in [
            "http://login.invalid",
            "https://user:secret@login.invalid",
            "file:///tmp/token",
            "https://login.invalid/path?secret=value",
        ] {
            let mut config = base.clone();
            config.oauth.issuer = Some(issuer.into());
            assert!(validate_server_config(&config).is_err(), "{issuer}");
        }

        let mut loopback = base;
        loopback.url = Some("http://127.0.0.1:8765/mcp".into());
        loopback.oauth.issuer = Some("http://127.0.0.1:8766/issuer".into());
        validate_server_config(&loopback).unwrap();
        loopback.oauth.issuer = Some("http://localhost:8766/issuer".into());
        assert!(validate_server_config(&loopback).is_err());
    }

    #[tokio::test]
    async fn streamable_http_reuses_network_catalog_call_resource_and_shutdown_boundaries() {
        const ENVIRONMENT: &str = "CA_MCP062_HTTP_FIXTURE_TOKEN";
        const SECRET: &str = "mcp062-fixture-secret";
        std::env::set_var(ENVIRONMENT, SECRET);
        let (url, mut requests, stop, server) = start_http_mcp_fixture().await;
        let temp = tempfile::tempdir().unwrap();
        let mut config = CaConfig::default();
        config.mcp.servers.insert(
            "http".into(),
            McpServerConfig {
                transport: McpTransport::StreamableHttp,
                command: String::new(),
                args: Vec::new(),
                env: HashMap::new(),
                url: Some(url.clone()),
                headers: HashMap::from([("X-Test-Token".into(), format!("${{{ENVIRONMENT}}}"))]),
                enabled: true,
                timeout_secs: 2,
                network: true,
                oauth: Default::default(),
            },
        );
        let manager =
            McpManager::load(&config, workspace(&temp), SandboxMode::PolicyOnly, false).unwrap();

        assert_eq!(
            manager.statuses().await[0].state,
            McpServerState::NetworkApprovalRequired
        );
        assert!(!manager.catalog().await.diagnostics.is_empty());
        assert!(
            requests.try_recv().is_err(),
            "denied catalog must make no request"
        );
        manager.grant_network_for_server("http").await.unwrap();

        let catalog = manager.catalog().await;
        assert!(catalog.diagnostics.is_empty(), "{:?}", catalog.diagnostics);
        assert!(catalog
            .registry
            .as_ref()
            .unwrap()
            .get("mcp__http__echo")
            .is_some());
        assert_eq!(catalog.resources.len(), 1);
        assert_eq!(catalog.resources[0].uri, "test://http-resource");
        let echo = manager
            .call_tool("mcp__http__echo", serde_json::json!({"text": SECRET}))
            .await
            .unwrap();
        assert_eq!(echo.model_text(), "[REDACTED]");
        assert_eq!(
            manager
                .read_resource("http", "test://http-resource")
                .await
                .unwrap(),
            "HTTP resource body"
        );
        let status = manager.verbose_status().await;
        assert!(status.contains("http [streamable_http User/Running]"));
        for forbidden in [&url, "X-Test-Token", ENVIRONMENT, SECRET] {
            assert!(!status.contains(forbidden));
        }

        manager.reconnect("http").await.unwrap();
        assert_eq!(manager.statuses().await[0].state, McpServerState::Running);
        manager.shutdown().await;
        let _ = stop.send(());
        server.await.unwrap();
        std::env::remove_var(ENVIRONMENT);

        let requests = std::iter::from_fn(|| requests.try_recv().ok()).collect::<Vec<_>>();
        assert!(requests.len() >= 9, "requests: {requests:#?}");
        assert!(requests.iter().all(|request| request.path == "/mcp"));
        assert!(requests.iter().all(|request| {
            request.headers.get("x-test-token").map(String::as_str) == Some(SECRET)
        }));
        assert!(requests.iter().any(|request| {
            request.body.as_ref().and_then(|body| body.get("method"))
                == Some(&serde_json::Value::String("tools/call".into()))
        }));
        assert!(requests.iter().any(|request| request.method == "DELETE"));
    }

    #[tokio::test]
    async fn oauth_http_catalog_uses_only_profile_scoped_bearer_and_disconnects_exactly() {
        let (url, mut requests, stop, server) = start_oauth_mcp_fixture().await;
        let issuer = url.trim_end_matches("/mcp").to_string();
        let store = Arc::new(FakeOAuthStore::default());
        store.0.lock().unwrap().insert(
            "profile/work/server/secure".into(),
            stored_oauth_credentials(&issuer, "access-private-sentinel"),
        );
        store.0.lock().unwrap().insert(
            "profile/personal/server/secure".into(),
            stored_oauth_credentials(&issuer, "other-private-sentinel"),
        );
        let temp = tempfile::tempdir().unwrap();
        let mut config = CaConfig::default();
        config.mcp.servers.insert(
            "secure".into(),
            McpServerConfig {
                transport: McpTransport::StreamableHttp,
                command: String::new(),
                args: Vec::new(),
                env: HashMap::new(),
                url: Some(url.clone()),
                headers: HashMap::new(),
                enabled: true,
                timeout_secs: 2,
                network: true,
                oauth: crate::config::McpOAuthConfig {
                    enabled: true,
                    issuer: Some(issuer.clone()),
                    client_id: Some("fixture-client".into()),
                    scopes: vec!["tools".into()],
                    ..Default::default()
                },
            },
        );
        let manager = McpManager::load_with_oauth_dependencies(
            &config,
            workspace(&temp),
            SandboxMode::PolicyOnly,
            true,
            crate::runtime::ActiveProfile::Named("work".into()),
            store.clone(),
            Arc::new(NoopBrowser),
        )
        .unwrap();
        let catalog = manager.catalog().await;
        assert!(catalog.diagnostics.is_empty(), "{:?}", catalog.diagnostics);
        assert!(catalog
            .registry
            .unwrap()
            .get("mcp__secure__secure_echo")
            .is_some());
        assert_eq!(manager.statuses().await[0].state, McpServerState::Running);
        let status = manager.status_text(false).await;
        assert!(status.contains("oauth=ready"));
        for hidden in [
            "access-private-sentinel",
            "refresh-private-sentinel",
            "never-display-auth-body",
            &url,
        ] {
            assert!(!status.contains(hidden));
        }
        manager.disconnect_authorization("secure").await.unwrap();
        assert_eq!(
            manager.statuses().await[0].state,
            McpServerState::AuthorizationRequired
        );
        let (work_exists, personal_exists) = {
            let values = store.0.lock().unwrap();
            (
                values.contains_key("profile/work/server/secure"),
                values.contains_key("profile/personal/server/secure"),
            )
        };
        assert!(!work_exists);
        assert!(personal_exists);
        while let Ok(request) = requests.try_recv() {
            if request.method == "POST" {
                assert_eq!(
                    request.headers.get("authorization").map(String::as_str),
                    Some("Bearer access-private-sentinel")
                );
            }
        }
        let _ = stop.send(());
        server.await.unwrap();
    }

    #[tokio::test]
    async fn missing_oauth_credentials_becomes_safe_actionable_state_before_mcp_post() {
        let (url, mut requests, stop, server) = start_oauth_mcp_fixture().await;
        let issuer = url.trim_end_matches("/mcp").to_string();
        let store = Arc::new(FakeOAuthStore::default());
        let temp = tempfile::tempdir().unwrap();
        let mut config = CaConfig::default();
        config.mcp.servers.insert(
            "secure".into(),
            McpServerConfig {
                transport: McpTransport::StreamableHttp,
                command: String::new(),
                args: Vec::new(),
                env: HashMap::new(),
                url: Some(url.clone()),
                headers: HashMap::new(),
                enabled: true,
                timeout_secs: 2,
                network: true,
                oauth: crate::config::McpOAuthConfig {
                    enabled: true,
                    issuer: Some(issuer),
                    client_id: Some("fixture-client".into()),
                    ..Default::default()
                },
            },
        );
        let manager = McpManager::load_with_oauth_dependencies(
            &config,
            workspace(&temp),
            SandboxMode::PolicyOnly,
            true,
            crate::runtime::ActiveProfile::Default,
            store,
            Arc::new(NoopBrowser),
        )
        .unwrap();
        let catalog = manager.catalog().await;
        assert_eq!(catalog.diagnostics.len(), 1);
        assert_eq!(
            manager.statuses().await[0].state,
            McpServerState::AuthorizationRequired
        );
        let status = manager.status_text(false).await;
        assert!(status.contains("oauth=authorization_required"));
        assert!(status.contains("run /mcp connect secure"));
        assert!(!status.contains(&url));
        assert!(!status.contains("never-display-auth-body"));
        while let Ok(request) = requests.try_recv() {
            assert_ne!(request.method, "POST");
        }
        let _ = stop.send(());
        server.await.unwrap();
    }

    #[tokio::test]
    async fn streamable_http_accepts_a_bounded_sse_initialization_response() {
        let (url, _requests, stop, server) = start_http_mcp_fixture_with_initial_sse(true).await;
        let (_temp, manager) = http_manager(url, 2);
        let catalog = manager.catalog().await;
        assert!(catalog.diagnostics.is_empty(), "{:?}", catalog.diagnostics);
        assert!(catalog.registry.unwrap().get("mcp__http__echo").is_some());
        manager.shutdown().await;
        let _ = stop.send(());
        server.await.unwrap();
    }

    #[tokio::test]
    async fn project_streamable_http_requires_exact_trust_and_network_authority() {
        let (url, mut requests, stop, server) = start_http_mcp_fixture().await;
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join(".ca")).unwrap();
        std::fs::write(
            temp.path().join(".ca/mcp.toml"),
            format!(
                "[servers.remote]\ntransport = 'streamable_http'\nurl = '{url}'\nnetwork = true\n"
            ),
        )
        .unwrap();

        let config = CaConfig::default();
        let untrusted =
            McpManager::load(&config, workspace(&temp), SandboxMode::PolicyOnly, true).unwrap();
        assert!(matches!(
            untrusted.statuses().await[0].state,
            McpServerState::Untrusted
        ));
        assert_eq!(untrusted.catalog().await.diagnostics.len(), 1);
        assert!(requests.try_recv().is_err());

        let mut trust = untrusted.project_trust_record(false).unwrap();
        let mut config = CaConfig::default();
        config.mcp.trusted_projects.push(trust.clone());
        let no_network =
            McpManager::load(&config, workspace(&temp), SandboxMode::PolicyOnly, true).unwrap();
        assert!(matches!(
            no_network.statuses().await[0].state,
            McpServerState::NetworkApprovalRequired
        ));
        assert_eq!(no_network.catalog().await.diagnostics.len(), 1);
        assert!(requests.try_recv().is_err());

        trust.network = true;
        config.mcp.trusted_projects = vec![trust];
        let trusted =
            McpManager::load(&config, workspace(&temp), SandboxMode::PolicyOnly, true).unwrap();
        let catalog = trusted.catalog().await;
        assert!(catalog.diagnostics.is_empty(), "{:?}", catalog.diagnostics);
        assert!(catalog.registry.unwrap().get("mcp__remote__echo").is_some());
        trusted.shutdown().await;
        let _ = stop.send(());
        server.await.unwrap();
    }

    #[tokio::test]
    async fn missing_streamable_http_header_reference_fails_before_network_io() {
        const ENVIRONMENT: &str = "CA_MCP062_INTENTIONALLY_MISSING_TOKEN";
        std::env::remove_var(ENVIRONMENT);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/mcp", listener.local_addr().unwrap());
        let temp = tempfile::tempdir().unwrap();
        let mut config = CaConfig::default();
        config.mcp.servers.insert(
            "missing".into(),
            McpServerConfig {
                transport: McpTransport::StreamableHttp,
                command: String::new(),
                args: Vec::new(),
                env: HashMap::new(),
                url: Some(url.clone()),
                headers: HashMap::from([("Authorization".into(), format!("${{{ENVIRONMENT}}}"))]),
                enabled: true,
                timeout_secs: 1,
                network: true,
                oauth: Default::default(),
            },
        );
        let manager =
            McpManager::load(&config, workspace(&temp), SandboxMode::PolicyOnly, true).unwrap();
        let catalog = manager.catalog().await;
        assert_eq!(catalog.diagnostics.len(), 1);
        assert!(catalog.diagnostics[0].contains(ENVIRONMENT));
        assert!(!catalog.diagnostics[0].contains(&url));
        assert!(
            tokio::time::timeout(Duration::from_millis(100), listener.accept())
                .await
                .is_err()
        );
    }

    #[test]
    fn streamable_http_sse_event_limit_survives_chunk_boundaries() {
        let mut limiter = SseEventLimiter::new(5);
        limiter.observe(b"data").unwrap();
        limiter.observe(b"\r").unwrap();
        limiter.observe(b"\n\r\n").unwrap();
        limiter.observe(b"next\n\n").unwrap();

        let mut oversized = SseEventLimiter::new(5);
        oversized.observe(b"abc").unwrap();
        assert!(matches!(
            oversized.observe(b"def"),
            Err(SafeHttpBodyError::EventTooLarge)
        ));
    }

    #[tokio::test]
    async fn streamable_http_rejects_declared_and_chunked_oversized_json_safely() {
        const MARKER: &str = "mcp062-oversized-body-marker";
        for chunked in [false, true] {
            let mut body = vec![b'x'; MAX_HTTP_JSON_BYTES + 1];
            body[..MARKER.len()].copy_from_slice(MARKER.as_bytes());
            let (url, server) =
                start_single_http_response("200 OK", Some("application/json"), "", body, chunked)
                    .await;
            let (_temp, manager) = http_manager(url.clone(), 2);
            let catalog = manager.catalog().await;
            assert_eq!(catalog.diagnostics.len(), 1);
            assert!(!catalog.diagnostics[0].contains(MARKER));
            assert!(!catalog.diagnostics[0].contains(&url));
            manager.shutdown().await;
            server.await.unwrap();
        }
    }

    #[tokio::test]
    async fn streamable_http_refuses_redirects_and_redacts_http_failures() {
        const MARKER: &str = "mcp062-private-http-error-body";
        const CHALLENGE: &str = "Bearer realm=\"mcp062-secret-realm\"";
        let redirect_target = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let target_url = format!("http://{}/stolen", redirect_target.local_addr().unwrap());
        let (url, server) = start_single_http_response(
            "307 Temporary Redirect",
            None,
            &format!("Location: {target_url}\r\n"),
            MARKER.as_bytes().to_vec(),
            false,
        )
        .await;
        let (_temp, manager) = http_manager(url.clone(), 1);
        let catalog = manager.catalog().await;
        assert_eq!(catalog.diagnostics.len(), 1);
        for forbidden in [MARKER, target_url.as_str(), url.as_str()] {
            assert!(!catalog.diagnostics[0].contains(forbidden));
        }
        assert!(
            tokio::time::timeout(Duration::from_millis(100), redirect_target.accept())
                .await
                .is_err()
        );
        manager.shutdown().await;
        server.await.unwrap();

        let (url, server) = start_single_http_response(
            "401 Unauthorized",
            Some("application/json"),
            &format!("WWW-Authenticate: {CHALLENGE}\r\n"),
            format!(r#"{{"error":"{MARKER}"}}"#).into_bytes(),
            false,
        )
        .await;
        let (_temp, manager) = http_manager(url.clone(), 1);
        let catalog = manager.catalog().await;
        assert_eq!(catalog.diagnostics.len(), 1);
        for forbidden in [MARKER, CHALLENGE, url.as_str()] {
            assert!(!catalog.diagnostics[0].contains(forbidden));
        }
        manager.shutdown().await;
        server.await.unwrap();
    }

    #[tokio::test]
    async fn streamable_http_bounds_sse_events_and_operation_time() {
        const MARKER: &str = "mcp062-oversized-sse-marker";
        let mut body = format!("data: {MARKER}").into_bytes();
        body.extend(std::iter::repeat_n(b'x', MAX_HTTP_EVENT_BYTES));
        body.extend_from_slice(b"\n\n");
        let (url, server) =
            start_single_http_response("200 OK", Some("text/event-stream"), "", body, true).await;
        let (_temp, manager) = http_manager(url.clone(), 2);
        let catalog = manager.catalog().await;
        assert_eq!(catalog.diagnostics.len(), 1);
        assert!(!catalog.diagnostics[0].contains(MARKER));
        assert!(!catalog.diagnostics[0].contains(&url));
        manager.shutdown().await;
        server.await.unwrap();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/mcp", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let _ = read_http_fixture_request(&mut stream).await;
            std::future::pending::<()>().await;
        });
        let (_temp, manager) = http_manager(url.clone(), 1);
        let catalog = tokio::time::timeout(Duration::from_secs(2), manager.catalog())
            .await
            .expect("the configured MCP timeout must bound initialization");
        assert_eq!(catalog.diagnostics.len(), 1);
        assert!(!catalog.diagnostics[0].contains(&url));
        manager.shutdown().await;
        server.abort();
    }

    #[test]
    fn manager_is_lazy_until_catalog_or_call_is_requested() {
        let temp = tempfile::tempdir().unwrap();
        let marker = temp.path().join("started");
        let script = temp.path().join("server.sh");
        std::fs::write(
            &script,
            format!("#!/bin/sh\ntouch '{}'\nexit 0\n", marker.display()),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let mut config = CaConfig::default();
        config.mcp.servers.insert(
            "lazy".into(),
            McpServerConfig {
                transport: crate::config::McpTransport::Stdio,
                command: script.to_string_lossy().into(),
                args: Vec::new(),
                env: HashMap::new(),
                url: None,
                headers: HashMap::new(),
                enabled: true,
                timeout_secs: 1,
                network: false,
                oauth: Default::default(),
            },
        );
        let _manager =
            McpManager::load(&config, workspace(&temp), SandboxMode::PolicyOnly, false).unwrap();
        assert!(!marker.exists());
    }

    #[tokio::test]
    async fn active_catalog_defers_startup_until_bounded_search_activates_exact_tools() {
        let temp = tempfile::tempdir().unwrap();
        let script = write_fake_server(&temp);
        let marker = temp.path().join("deferred-closed");
        let manager = McpManager::load(
            &fake_config(&script, &marker, "normal"),
            workspace(&temp),
            SandboxMode::PolicyOnly,
            false,
        )
        .unwrap();

        let initial = manager.active_catalog().await;
        assert!(initial.diagnostics.is_empty());
        assert_eq!(manager.statuses().await[0].state, McpServerState::Stopped);
        assert!(initial.registry.as_ref().is_some_and(|registry| {
            registry.get("search_mcp_tools").is_some() && registry.get("mcp__fake__echo").is_none()
        }));
        let initial_resolved = crate::tool_selection::resolve_tools(
            initial.registry.as_ref().unwrap(),
            crate::agent::registry::ToolProfile::Main,
            &crate::tool_selection::ToolSelectionPolicy::default(),
            &crate::tool_selection::ToolSelectionConstraints::default(),
        )
        .unwrap();
        assert!(initial_resolved.mcp_definitions().is_empty());

        let result = manager.search_tools("echo", Some("fake"), 1).await;
        assert!(result.is_success(), "{}", result.model_text());
        assert!(result.model_text().contains("mcp__fake__echo"));

        let activated = manager.active_catalog().await;
        let registry = activated.registry.unwrap();
        assert!(registry.get("mcp__fake__echo").is_some());
        assert!(registry.get("mcp__fake__inspect_env").is_none());
        let activated_resolved = crate::tool_selection::resolve_tools(
            &registry,
            crate::agent::registry::ToolProfile::Main,
            &crate::tool_selection::ToolSelectionPolicy::default(),
            &crate::tool_selection::ToolSelectionConstraints::default(),
        )
        .unwrap();
        assert_eq!(activated_resolved.mcp_definitions().len(), 1);
        assert!(
            crate::context::estimate_serialized_tokens(
                &activated_resolved.mcp_definitions().to_vec()
            ) > 0
        );
        manager.shutdown().await;
    }

    #[tokio::test]
    async fn host_rendered_tool_calls_do_not_activate_model_visible_tools() {
        let temp = tempfile::tempdir().unwrap();
        let script = write_fake_server(&temp);
        let marker = temp.path().join("host-only-closed");
        let manager = McpManager::load(
            &fake_config(&script, &marker, "normal"),
            workspace(&temp),
            SandboxMode::PolicyOnly,
            false,
        )
        .unwrap();

        let value = manager
            .call_host_tool_structured("fake", "echo", serde_json::json!({"text": "review"}))
            .await
            .unwrap();

        assert_eq!(value, serde_json::json!({"text": "review"}));
        assert!(!manager.has_active_tool("mcp__fake__echo").await);
        assert!(manager
            .active_catalog()
            .await
            .registry
            .unwrap()
            .get("mcp__fake__echo")
            .is_none());
        manager.shutdown().await;
    }

    #[tokio::test]
    async fn host_rendered_tool_calls_preserve_bounded_redacted_typed_errors() {
        let temp = tempfile::tempdir().unwrap();
        let script = write_fake_server(&temp);
        let marker = temp.path().join("host-error-closed");
        let manager = McpManager::load(
            &fake_config(&script, &marker, "normal"),
            workspace(&temp),
            SandboxMode::PolicyOnly,
            false,
        )
        .unwrap();

        let error = manager
            .call_host_tool_structured("fake", "echo", serde_json::json!({"text": "typed-error"}))
            .await
            .unwrap_err();
        assert_eq!(
            error,
            McpError::Tool {
                server: "fake".into(),
                code: "INVALID_ARGUMENT".into(),
                message: "relative scope required; [REDACTED]".into(),
                retryable: true,
            }
        );
        assert!(!error.to_string().contains("protocol failure"));
        manager.shutdown().await;
    }

    #[tokio::test]
    async fn catalog_change_invalidates_only_affected_active_schemas() {
        let temp = tempfile::tempdir().unwrap();
        let script = write_fake_server(&temp);
        let marker = temp.path().join("changed-closed");
        let manager = McpManager::load(
            &fake_config(&script, &marker, "normal"),
            workspace(&temp),
            SandboxMode::PolicyOnly,
            false,
        )
        .unwrap();
        assert!(manager
            .search_tools("echo", Some("fake"), 1)
            .await
            .is_success());
        manager
            .call_tool("mcp__fake__echo", serde_json::json!({"text":"change"}))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(25)).await;

        let refreshed = manager.active_catalog().await;
        let registry = refreshed.registry.unwrap();
        assert!(registry.get("mcp__fake__echo").is_none());
        assert!(registry.get("mcp__fake__refreshed").is_none());
        assert_eq!(manager.statuses().await[0].active_tool_count, 0);

        let search = manager.search_tools("refreshed", Some("fake"), 1).await;
        assert!(search.is_success(), "{}", search.model_text());
        assert!(manager
            .active_catalog()
            .await
            .registry
            .unwrap()
            .get("mcp__fake__refreshed")
            .is_some());
        manager.shutdown().await;
    }

    #[tokio::test]
    async fn search_never_starts_disabled_or_network_ungranted_servers() {
        let temp = tempfile::tempdir().unwrap();
        let script = write_fake_server(&temp);
        let marker = temp.path().join("ineligible-closed");
        let mut config = fake_config(&script, &marker, "normal");
        config.mcp.servers.get_mut("fake").unwrap().enabled = false;
        let disabled =
            McpManager::load(&config, workspace(&temp), SandboxMode::PolicyOnly, false).unwrap();
        let result = disabled.search_tools("echo", Some("fake"), 5).await;
        assert!(!result.is_success());
        assert!(result.model_text().contains("server is disabled"));
        assert_eq!(disabled.statuses().await[0].state, McpServerState::Stopped);
        assert!(!marker.exists());

        config.mcp.servers.get_mut("fake").unwrap().enabled = true;
        config.mcp.servers.get_mut("fake").unwrap().network = true;
        let network =
            McpManager::load(&config, workspace(&temp), SandboxMode::PolicyOnly, false).unwrap();
        let result = network.search_tools("echo", Some("fake"), 5).await;
        assert!(!result.is_success());
        assert!(result.model_text().contains("network grant is required"));
        assert_eq!(
            network.statuses().await[0].state,
            McpServerState::NetworkApprovalRequired
        );
        assert!(!marker.exists());
    }

    #[tokio::test]
    async fn search_caps_unhinted_server_startup_and_reports_omission() {
        let temp = tempfile::tempdir().unwrap();
        let script = write_fake_server(&temp);
        let template_marker = temp.path().join("template-closed");
        let template = fake_config(&script, &template_marker, "normal")
            .mcp
            .servers
            .remove("fake")
            .unwrap();
        let mut config = CaConfig::default();
        config.mcp.servers.clear();
        let markers = (0..=MAX_SEARCH_SERVERS)
            .map(|index| {
                let marker = temp.path().join(format!("server-{index}-closed"));
                let mut definition = template.clone();
                definition
                    .env
                    .insert("EXIT_MARKER".into(), marker.to_string_lossy().into());
                config
                    .mcp
                    .servers
                    .insert(format!("server{index}"), definition);
                marker
            })
            .collect::<Vec<_>>();
        let manager =
            McpManager::load(&config, workspace(&temp), SandboxMode::PolicyOnly, false).unwrap();

        let result = manager.search_tools("echo", None, 10).await;
        assert!(result.is_success(), "{}", result.model_text());
        assert!(result
            .model_text()
            .contains("1 eligible server was not inspected"));
        assert_eq!(
            manager
                .statuses()
                .await
                .iter()
                .filter(|status| status.state == McpServerState::Running)
                .count(),
            MAX_SEARCH_SERVERS
        );
        manager.shutdown().await;
        for marker in markers.iter().take(MAX_SEARCH_SERVERS) {
            assert!(marker.exists(), "started server must close");
        }
        assert!(!markers[MAX_SEARCH_SERVERS].exists());
    }

    #[tokio::test]
    async fn fake_stdio_server_covers_pagination_calls_resources_filters_and_cleanup() {
        let temp = tempfile::tempdir().unwrap();
        let script = write_fake_server(&temp);
        let marker = temp.path().join("closed");
        let manager = McpManager::load(
            &fake_config(&script, &marker, "normal"),
            workspace(&temp),
            SandboxMode::PolicyOnly,
            false,
        )
        .unwrap();
        assert!(!marker.exists(), "construction must stay lazy");
        assert!(manager.discovered_resource_snapshot(256).is_empty());
        assert!(
            !marker.exists(),
            "metadata snapshot must not activate a server"
        );

        let catalog = manager.catalog().await;
        assert!(catalog.diagnostics.is_empty(), "{:?}", catalog.diagnostics);
        assert_eq!(manager.discovered_resource_snapshot(256).len(), 2);
        let registry = catalog.registry.unwrap();
        for name in [
            "mcp__fake__echo",
            "mcp__fake__inspect_env",
            "mcp__fake__binary",
            "mcp__fake__huge",
            "mcp__fake__network_probe",
            "mcp__fake__read_resource",
        ] {
            assert!(registry.get(name).is_some(), "missing {name}");
        }
        assert_eq!(catalog.resources.len(), 2);
        let selected = vec![crate::attachments::ComposerAttachment {
            reference: "mcp:fake:test".into(),
            kind: crate::attachments::ComposerAttachmentKind::Mcp {
                server: "fake".into(),
                uri: "test://one".into(),
                name: "one".into(),
            },
        }];
        let resolution = crate::attachments::resolve_mcp_attachments(
            "inspect @mcp:fake:test",
            &selected,
            Some(&Arc::new(manager.clone())),
        )
        .await;
        assert!(resolution.unavailable_markers.is_empty());
        assert!(resolution
            .process_context
            .as_deref()
            .is_some_and(|context| context.contains("resource:test://one")));
        assert!(registry
            .validate("mcp__fake__echo", &serde_json::json!({}))
            .is_err());
        registry
            .validate("mcp__fake__echo", &serde_json::json!({"text":"hello"}))
            .unwrap();

        let echo = manager
            .call_tool("mcp__fake__echo", serde_json::json!({"text":"hello"}))
            .await
            .unwrap();
        assert_eq!(echo.model_text(), "hello");
        let redacted = manager
            .call_tool(
                "mcp__fake__echo",
                serde_json::json!({"text":"resolved-test-secret"}),
            )
            .await
            .unwrap();
        assert_eq!(redacted.model_text(), "[REDACTED]");
        tokio::time::sleep(Duration::from_millis(25)).await;
        let refreshed = manager.catalog().await.registry.unwrap();
        assert!(refreshed.get("mcp__fake__refreshed").is_some());
        let environment = manager
            .call_tool("mcp__fake__inspect_env", serde_json::json!({}))
            .await
            .unwrap();
        assert!(environment.model_text().contains("TEST_TOKEN"));
        assert!(!environment.model_text().contains("CARGO_HOME"));
        assert_eq!(
            manager.read_resource("fake", "test://two").await.unwrap(),
            "resource:test://two"
        );
        assert_eq!(
            manager
                .call_tool(
                    "mcp__fake__read_resource",
                    serde_json::json!({"uri":"test://one"}),
                )
                .await
                .unwrap()
                .model_text(),
            "resource:test://one"
        );
        assert_eq!(
            manager
                .call_tool("mcp__fake__binary", serde_json::json!({}))
                .await
                .unwrap_err(),
            McpError::BinaryContent
        );
        assert_eq!(
            manager
                .call_tool("mcp__fake__huge", serde_json::json!({}))
                .await
                .unwrap_err(),
            McpError::OutputLimit(MAX_OUTPUT_BYTES)
        );
        let verbose = manager.verbose_status().await;
        assert!(verbose.contains("fake stderr"));
        assert!(!verbose.contains("\u{1b}"));

        manager.reconnect("fake").await.unwrap();
        assert_eq!(manager.statuses().await[0].state, McpServerState::Running);
        let temporary_directories = manager.temporary_directories().await;
        assert!(temporary_directories.iter().all(|path| path.is_dir()));
        manager.shutdown().await;
        for _ in 0..20 {
            if marker.exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        assert!(
            marker.exists(),
            "server must observe stdin close on shutdown"
        );
        assert!(temporary_directories.iter().all(|path| !path.exists()));
    }

    #[tokio::test]
    async fn malformed_and_timeout_servers_fail_without_hanging_or_leaking() {
        for mode in ["malformed", "timeout"] {
            let temp = tempfile::tempdir().unwrap();
            let script = write_fake_server(&temp);
            let marker = temp.path().join(format!("closed-{mode}"));
            let manager = McpManager::load(
                &fake_config(&script, &marker, mode),
                workspace(&temp),
                SandboxMode::PolicyOnly,
                false,
            )
            .unwrap();
            let catalog = manager.catalog().await;
            assert!(!catalog.diagnostics.is_empty(), "mode {mode} must fail");
            assert!(!manager.has_tool("mcp__fake__echo").await);
            manager.shutdown().await;
        }
    }

    #[tokio::test]
    async fn shutdown_cancels_an_in_flight_tool_call() {
        let temp = tempfile::tempdir().unwrap();
        let script = write_fake_server(&temp);
        let marker = temp.path().join("cancelled-closed");
        let mut config = fake_config(&script, &marker, "call_timeout");
        config.mcp.servers.get_mut("fake").unwrap().timeout_secs = 5;
        let manager = Arc::new(
            McpManager::load(&config, workspace(&temp), SandboxMode::PolicyOnly, false).unwrap(),
        );
        assert!(manager.catalog().await.diagnostics.is_empty());
        let caller = {
            let manager = manager.clone();
            tokio::spawn(async move {
                manager
                    .call_tool("mcp__fake__echo", serde_json::json!({"text":"never"}))
                    .await
            })
        };
        tokio::time::sleep(Duration::from_millis(50)).await;
        manager.shutdown().await;
        let result = tokio::time::timeout(Duration::from_secs(1), caller)
            .await
            .expect("call cancellation must be prompt")
            .unwrap();
        assert!(result.is_err());
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn fake_stdio_server_runs_inside_required_macos_sandbox() {
        let temp = tempfile::tempdir().unwrap();
        let script = write_fake_server(&temp);
        let marker = temp.path().join("sandbox-closed");
        let manager = McpManager::load(
            &fake_config(&script, &marker, "normal"),
            workspace(&temp),
            SandboxMode::Required,
            false,
        )
        .unwrap();
        let catalog = manager.catalog().await;
        assert!(catalog.diagnostics.is_empty(), "{:?}", catalog.diagnostics);
        assert!(catalog.registry.unwrap().get("mcp__fake__echo").is_some());
        manager.shutdown().await;
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn installed_stdio_executable_outside_workspace_is_literal_read_only() {
        use std::os::unix::fs::PermissionsExt;

        let executable_temp = tempfile::tempdir().unwrap();
        let script = write_fake_server(&executable_temp);
        let original = std::fs::read_to_string(&script).unwrap();
        std::fs::write(&script, format!("#!/usr/bin/env python3\n{original}")).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();

        let workspace_temp = tempfile::tempdir().unwrap();
        let marker = workspace_temp.path().join("installed-sandbox-closed");
        let mut config = fake_config(&script, &marker, "normal");
        let server = config.mcp.servers.get_mut("fake").unwrap();
        server.command = script.to_string_lossy().into_owned();
        server.args.clear();

        let manager = McpManager::load(
            &config,
            workspace(&workspace_temp),
            SandboxMode::Required,
            false,
        )
        .unwrap();
        let catalog = manager.catalog().await;
        assert!(catalog.diagnostics.is_empty(), "{:?}", catalog.diagnostics);
        assert!(catalog.registry.unwrap().get("mcp__fake__echo").is_some());
        manager.shutdown().await;
        assert!(marker.is_file());
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn mcp_network_is_denied_by_default_and_granted_only_when_config_and_run_agree() {
        let temp = tempfile::tempdir().unwrap();
        let script = write_fake_server(&temp);
        let denied_marker = temp.path().join("denied-closed");
        let denied = McpManager::load(
            &fake_config(&script, &denied_marker, "normal"),
            workspace(&temp),
            SandboxMode::Required,
            false,
        )
        .unwrap();
        let _ = denied.catalog().await;
        let denied_result = denied
            .call_tool(
                "mcp__fake__network_probe",
                serde_json::json!({"host":"127.0.0.1","port":9}),
            )
            .await
            .unwrap();
        assert_eq!(denied_result.model_text(), "denied");
        denied.shutdown().await;

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let accept = tokio::spawn(async move { listener.accept().await.unwrap() });
        let allowed_marker = temp.path().join("allowed-closed");
        let mut config = fake_config(&script, &allowed_marker, "normal");
        config.mcp.servers.get_mut("fake").unwrap().network = true;
        let allowed =
            McpManager::load(&config, workspace(&temp), SandboxMode::Required, true).unwrap();
        let catalog = allowed.catalog().await;
        assert!(catalog.diagnostics.is_empty(), "{:?}", catalog.diagnostics);
        let result = allowed
            .call_tool(
                "mcp__fake__network_probe",
                serde_json::json!({"host":"127.0.0.1","port":port}),
            )
            .await
            .unwrap();
        assert_eq!(result.model_text(), "connected");
        accept.await.unwrap();
        allowed.shutdown().await;
    }

    #[test]
    fn output_bounds_and_dynamic_schema_validation_fail_closed() {
        let mut output = String::new();
        assert_eq!(
            append_bounded(&mut output, &"x".repeat(5), 4).unwrap_err(),
            McpError::OutputLimit(4)
        );
        let spec = ToolSpec::mcp(
            "mcp__s__t",
            "s",
            "test".into(),
            serde_json::json!({
                "type":"object",
                "properties":{"count":{"type":"integer"}},
                "required":["count"],
                "additionalProperties":false
            }),
        )
        .unwrap();
        let registry = ToolRegistry::new(vec![spec]).unwrap();
        assert!(registry
            .validate("mcp__s__t", &serde_json::json!({"count":1}))
            .is_ok());
        assert!(registry
            .validate("mcp__s__t", &serde_json::json!({"count":"1"}))
            .is_err());
        assert!(registry
            .validate("mcp__s__t", &serde_json::json!({"count":1,"extra":true}))
            .is_err());
        let union = ToolSpec::mcp(
            "mcp__s__union",
            "s",
            "union".into(),
            serde_json::json!({
                "type":"object",
                "properties":{"value":{"oneOf":[{"type":"string"},{"type":"integer"}]}},
                "required":["value"]
            }),
        )
        .unwrap();
        let union = ToolRegistry::new(vec![union]).unwrap();
        assert!(union
            .validate("mcp__s__union", &serde_json::json!({"value":true}))
            .is_err());
        assert!(ToolSpec::mcp(
            "mcp__s__external",
            "s",
            "external".into(),
            serde_json::json!({"$ref":"https://example.invalid/schema.json"}),
        )
        .is_err());
    }
}
