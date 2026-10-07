use crate::agent::registry::ToolExecutionResult;
use crate::config::{LspConfig, LspServerConfig};
use crate::sandbox::{SandboxBackend, SandboxMode, SandboxPolicy};
use crate::workspace::{PathOperation, WorkspaceContext};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::sync::Mutex;

const MAX_DOCUMENT_BYTES: u64 = 1024 * 1024;
const MAX_HEADER_BYTES: usize = 8 * 1024;
const MAX_FRAME_BYTES: usize = 1024 * 1024;
const MAX_RESULT_BYTES: usize = 64 * 1024;
const MAX_RESULT_ITEMS: usize = 100;
const MAX_DIAGNOSTIC_ITEMS: usize = 50;
const MAX_DIAGNOSTIC_BYTES: usize = 32 * 1024;
const MAX_SERVER_NAME_BYTES: usize = 64;
const MAX_ARGUMENTS: usize = 64;
const MAX_ARGUMENT_BYTES: usize = 4096;

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum LspError {
    #[error("invalid LSP configuration for {server}: {detail}")]
    InvalidConfig {
        server: String,
        detail: &'static str,
    },
    #[error("LSP server {0} is not configured or enabled")]
    UnknownServer(String),
    #[error("configured executable for LSP server {0} was not found")]
    ExecutableMissing(String),
    #[error("LSP server {0} could not be started safely")]
    Start(String),
    #[error("LSP server {0} timed out")]
    Timeout(String),
    #[error("LSP server {0} returned an invalid or incomplete response")]
    Protocol(String),
    #[error("LSP position is outside the requested document")]
    InvalidPosition,
    #[error("LSP document is not a bounded UTF-8 workspace file")]
    InvalidDocument,
    #[error("LSP response referenced a location outside workspace authority")]
    ExternalLocation,
}

#[derive(Debug, Clone)]
struct ServerDefinition {
    id: String,
    config: LspServerConfig,
}

struct RunningServer {
    definition: ServerDefinition,
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    process_group: Option<i32>,
    temporary_directory: PathBuf,
    next_id: u64,
    documents: HashMap<PathBuf, OpenDocument>,
    diagnostics: HashMap<PathBuf, PublishedDiagnostics>,
}

struct OpenDocument {
    version: i64,
    text: String,
}

#[derive(Debug, Clone)]
struct PublishedDiagnostics {
    version: Option<i64>,
    items: Vec<LspDiagnostic>,
}

#[derive(Debug, Clone)]
struct LspDiagnostic {
    severity: Option<u64>,
    source: Option<String>,
    code: Option<String>,
    start_line: u64,
    start_character: u64,
    end_line: u64,
    end_character: u64,
    message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MutationDiagnostics {
    NotApplicable,
    Report(String),
}

impl MutationDiagnostics {
    pub fn into_report(self) -> Option<String> {
        match self {
            Self::NotApplicable => None,
            Self::Report(text) => Some(text),
        }
    }

    pub fn report(&self) -> Option<&str> {
        match self {
            Self::NotApplicable => None,
            Self::Report(text) => Some(text),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LspServerPhase {
    Lazy,
    Busy,
    Running,
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LspServerStatus {
    pub id: String,
    pub phase: LspServerPhase,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LspStatusSnapshot {
    pub servers: Vec<LspServerStatus>,
}

struct ManagerInner {
    definitions: HashMap<String, ServerDefinition>,
    slots: HashMap<String, Arc<Mutex<Option<RunningServer>>>>,
    workspace: RwLock<WorkspaceContext>,
    sandbox_mode: SandboxMode,
    environment: BTreeMap<String, String>,
    failures: RwLock<HashMap<String, String>>,
}

#[derive(Clone)]
pub struct LspManager {
    inner: Arc<ManagerInner>,
}

impl std::fmt::Debug for LspManager {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LspManager")
            .field("configured_server_count", &self.inner.definitions.len())
            .finish()
    }
}

impl LspManager {
    pub fn load(
        config: &LspConfig,
        workspace: WorkspaceContext,
        sandbox_mode: SandboxMode,
    ) -> Result<Option<Self>, LspError> {
        let mut definitions = HashMap::new();
        let mut slots = HashMap::new();
        for (id, server) in &config.servers {
            validate_definition(id, server)?;
            if !server.enabled {
                continue;
            }
            definitions.insert(
                id.clone(),
                ServerDefinition {
                    id: id.clone(),
                    config: server.clone(),
                },
            );
            slots.insert(id.clone(), Arc::new(Mutex::new(None)));
        }
        if definitions.is_empty() {
            return Ok(None);
        }
        Ok(Some(Self {
            inner: Arc::new(ManagerInner {
                definitions,
                slots,
                workspace: RwLock::new(workspace),
                sandbox_mode,
                environment: filtered_environment(),
                failures: RwLock::new(HashMap::new()),
            }),
        }))
    }

    pub fn configured_server_count(&self) -> usize {
        self.inner.definitions.len()
    }

    pub fn status_snapshot(&self) -> LspStatusSnapshot {
        let failures = self
            .inner
            .failures
            .read()
            .unwrap_or_else(|poison| poison.into_inner());
        let mut servers = self
            .inner
            .definitions
            .keys()
            .map(|id| {
                let phase = match self
                    .inner
                    .slots
                    .get(id)
                    .and_then(|slot| slot.try_lock().ok())
                {
                    Some(guard) if guard.is_some() => LspServerPhase::Running,
                    Some(_) => failures.get(id).map_or(LspServerPhase::Lazy, |failure| {
                        LspServerPhase::Failed(failure.clone())
                    }),
                    None => LspServerPhase::Busy,
                };
                LspServerStatus {
                    id: safe_id(id),
                    phase,
                }
            })
            .collect::<Vec<_>>();
        servers.sort_by(|left, right| left.id.cmp(&right.id));
        LspStatusSnapshot { servers }
    }

    fn record_start_result(&self, id: &str, result: &Result<RunningServer, LspError>) {
        let mut failures = self
            .inner
            .failures
            .write()
            .unwrap_or_else(|poison| poison.into_inner());
        match result {
            Ok(_) => {
                failures.remove(id);
            }
            Err(error) => {
                failures.insert(id.to_string(), safe_diagnostic_error(error).into());
            }
        }
    }

    fn record_failure(&self, id: &str, error: &LspError) {
        self.inner
            .failures
            .write()
            .unwrap_or_else(|poison| poison.into_inner())
            .insert(id.to_string(), safe_diagnostic_error(error).into());
    }

    fn clear_failure(&self, id: &str) {
        self.inner
            .failures
            .write()
            .unwrap_or_else(|poison| poison.into_inner())
            .remove(id);
    }

    pub async fn diagnostics_after_mutation(
        &self,
        receipt: &crate::undo::FileMutationReceipt,
    ) -> MutationDiagnostics {
        self.diagnostics_for_state(&receipt.path, &receipt.after)
            .await
    }

    pub async fn diagnostics_after_undo_operation(
        &self,
        operation: &crate::undo::UndoOperation,
    ) -> MutationDiagnostics {
        let receipt = operation.batch.primary();
        let state = match operation.direction {
            crate::undo::UndoDirection::Undo => &receipt.before,
            crate::undo::UndoDirection::Redo => &receipt.after,
        };
        self.diagnostics_for_state(&receipt.path, state).await
    }

    async fn diagnostics_for_state(
        &self,
        path: &Path,
        state: &crate::undo::FileState,
    ) -> MutationDiagnostics {
        let workspace = self
            .inner
            .workspace
            .read()
            .unwrap_or_else(|poison| poison.into_inner())
            .clone();
        let canonical_path =
            match state {
                crate::undo::FileState::Utf8(_) => match path.canonicalize() {
                    Ok(path)
                        if workspace
                            .roots()
                            .iter()
                            .any(|root| path.starts_with(&root.path)) =>
                    {
                        path
                    }
                    _ => return MutationDiagnostics::Report(
                        "LSP diagnostics: unavailable (document is outside workspace authority)"
                            .into(),
                    ),
                },
                crate::undo::FileState::Missing => {
                    let Some(name) = path.file_name() else {
                        return MutationDiagnostics::Report(
                            "LSP diagnostics: unavailable (document path is invalid)".into(),
                        );
                    };
                    match path.parent().and_then(|parent| parent.canonicalize().ok()) {
                    Some(parent)
                        if workspace
                            .roots()
                            .iter()
                            .any(|root| parent.starts_with(&root.path)) =>
                    {
                        parent.join(name)
                    }
                    _ => return MutationDiagnostics::Report(
                        "LSP diagnostics: unavailable (document is outside workspace authority)"
                            .into(),
                    ),
                }
                }
            };
        let matching = self
            .inner
            .definitions
            .values()
            .filter(|definition| validate_extension(definition, &canonical_path).is_ok())
            .cloned()
            .collect::<Vec<_>>();
        if matching.is_empty() {
            return MutationDiagnostics::NotApplicable;
        }

        let mut sections = Vec::new();
        for definition in matching {
            sections.push(
                self.diagnostics_from_server(definition, &workspace, &canonical_path, state)
                    .await,
            );
        }
        MutationDiagnostics::Report(bound_diagnostics(sections))
    }

    async fn diagnostics_from_server(
        &self,
        definition: ServerDefinition,
        workspace: &WorkspaceContext,
        path: &Path,
        state: &crate::undo::FileState,
    ) -> String {
        let Some(slot) = self.inner.slots.get(&definition.id).cloned() else {
            return format!(
                "LSP diagnostics [{}]: unavailable (server slot is absent)",
                safe_id(&definition.id)
            );
        };
        let timeout = Duration::from_secs(definition.config.timeout_secs);
        let operation = async {
            let mut guard = slot.lock().await;
            if guard.is_none() {
                let started = self.start_server(definition.clone(), workspace).await;
                self.record_start_result(&definition.id, &started);
                *guard = Some(started?);
            }
            let running = guard.as_mut().expect("server inserted");
            match state {
                crate::undo::FileState::Missing => {
                    running.close_document(path).await?;
                    Ok(format!(
                        "LSP diagnostics [{}]: document closed; retained diagnostics cleared",
                        safe_id(&definition.id)
                    ))
                }
                crate::undo::FileState::Utf8(text) => {
                    if text.len() as u64 > MAX_DOCUMENT_BYTES {
                        return Ok(format!(
                            "LSP diagnostics [{}]: unavailable (document exceeds the LSP size limit)",
                            safe_id(&definition.id)
                        ));
                    }
                    let uri = file_uri(path)?;
                    let version = running.sync_document(&uri, path, text.clone()).await?;
                    running.wait_for_diagnostics(path, version).await?;
                    let published = running
                        .diagnostics
                        .get(path)
                        .cloned()
                        .ok_or_else(|| LspError::Protocol(definition.id.clone()))?;
                    Ok(render_diagnostics(
                        &definition.id,
                        version,
                        &published.items,
                    ))
                }
            }
        };
        match tokio::time::timeout(timeout, operation).await {
            Ok(Ok(report)) => {
                self.clear_failure(&definition.id);
                report
            }
            Ok(Err(error)) => {
                self.record_failure(&definition.id, &error);
                format!(
                    "LSP diagnostics [{}]: unavailable ({})",
                    safe_id(&definition.id),
                    safe_diagnostic_error(&error)
                )
            }
            Err(_) => {
                self.record_failure(&definition.id, &LspError::Timeout(definition.id.clone()));
                let stale = {
                    let guard = slot.lock().await;
                    guard
                        .as_ref()
                        .and_then(|running| running.diagnostics.get(path))
                        .map(|diagnostics| diagnostics.version)
                };
                match stale {
                    Some(Some(version)) => format!(
                        "LSP diagnostics [{}]: stale (latest published document version {version})",
                        safe_id(&definition.id)
                    ),
                    Some(None) => format!(
                        "LSP diagnostics [{}]: stale (published diagnostics had no document version)",
                        safe_id(&definition.id)
                    ),
                    None => format!(
                        "LSP diagnostics [{}]: timed out waiting for the new document version",
                        safe_id(&definition.id)
                    ),
                }
            }
        }
    }

    pub async fn execute(&self, tool: &str, arguments: &Value) -> ToolExecutionResult {
        let manager = self.clone();
        let tool = tool.to_string();
        let arguments = arguments.clone();
        let result =
            tokio::spawn(async move { manager.execute_inner(&tool, &arguments).await }).await;
        match result.unwrap_or_else(|_| Err(LspError::Protocol("worker".into()))) {
            Ok(output) => ToolExecutionResult::Success(bound_output(&output)),
            Err(error) => ToolExecutionResult::Failure(error.to_string()),
        }
    }

    pub async fn update_workspace(&self, workspace: WorkspaceContext) {
        let changed = {
            let current = self
                .inner
                .workspace
                .read()
                .unwrap_or_else(|poison| poison.into_inner());
            current.cwd() != workspace.cwd()
                || current.roots().len() != workspace.roots().len()
                || current
                    .roots()
                    .iter()
                    .zip(workspace.roots())
                    .any(|(left, right)| {
                        left.path != right.path
                            || left.read_only != right.read_only
                            || left.primary != right.primary
                    })
        };
        if changed {
            self.shutdown().await;
            *self
                .inner
                .workspace
                .write()
                .unwrap_or_else(|poison| poison.into_inner()) = workspace;
        }
    }

    async fn execute_inner(&self, tool: &str, arguments: &Value) -> Result<String, LspError> {
        let server_id = arguments
            .get("server")
            .and_then(Value::as_str)
            .ok_or_else(|| LspError::UnknownServer("<missing>".into()))?;
        let definition = self
            .inner
            .definitions
            .get(server_id)
            .cloned()
            .ok_or_else(|| LspError::UnknownServer(safe_id(server_id)))?;
        let slot = self
            .inner
            .slots
            .get(server_id)
            .cloned()
            .ok_or_else(|| LspError::UnknownServer(safe_id(server_id)))?;
        let timeout = Duration::from_secs(definition.config.timeout_secs);
        let operation = async {
            let workspace = self
                .inner
                .workspace
                .read()
                .unwrap_or_else(|poison| poison.into_inner())
                .clone();
            let path = arguments
                .get("path")
                .and_then(Value::as_str)
                .ok_or(LspError::InvalidDocument)?;
            let path = workspace
                .resolve(path, PathOperation::Read)
                .map_err(|_| LspError::InvalidDocument)?;
            let text = read_bounded_document(&path).await?;
            validate_extension(&definition, &path)?;
            let uri = file_uri(&path)?;
            let position = if tool == "lsp_symbols" {
                None
            } else {
                let line = argument_usize(arguments, "line")?;
                let column = argument_usize(arguments, "column")?;
                let (line, character) = utf16_position(&text, line, column)?;
                Some(json!({"line": line, "character": character}))
            };
            let mut guard = slot.lock().await;
            if guard.is_none() {
                let started = self.start_server(definition.clone(), &workspace).await;
                self.record_start_result(&definition.id, &started);
                *guard = Some(started?);
            }
            let running = guard.as_mut().expect("server inserted");
            running.sync_document(&uri, &path, text).await?;
            let result = match tool {
                "lsp_symbols" => {
                    running
                        .request(
                            "textDocument/documentSymbol",
                            json!({"textDocument":{"uri":uri}}),
                        )
                        .await?
                }
                "lsp_definition" => {
                    running
                        .request("textDocument/definition", position_params(&uri, position))
                        .await?
                }
                "lsp_references" => {
                    let include_declaration = arguments
                        .get("include_declaration")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    let mut params = position_params(&uri, position);
                    params["context"] = json!({"includeDeclaration":include_declaration});
                    running.request("textDocument/references", params).await?
                }
                "lsp_hover" => {
                    running
                        .request("textDocument/hover", position_params(&uri, position))
                        .await?
                }
                "lsp_implementation" => {
                    running
                        .request(
                            "textDocument/implementation",
                            position_params(&uri, position),
                        )
                        .await?
                }
                "lsp_call_hierarchy" => {
                    let prepared = running
                        .request(
                            "textDocument/prepareCallHierarchy",
                            position_params(&uri, position),
                        )
                        .await?;
                    let item = prepared
                        .as_array()
                        .and_then(|items| items.first())
                        .cloned()
                        .ok_or_else(|| LspError::Protocol(definition.id.clone()))?;
                    validate_item_uri(&workspace, &item)?;
                    let direction = arguments
                        .get("direction")
                        .and_then(Value::as_str)
                        .unwrap_or("incoming");
                    let method = match direction {
                        "incoming" => "callHierarchy/incomingCalls",
                        "outgoing" => "callHierarchy/outgoingCalls",
                        _ => {
                            return Err(LspError::InvalidConfig {
                                server: definition.id.clone(),
                                detail: "direction must be incoming or outgoing",
                            })
                        }
                    };
                    running.request(method, json!({"item":item})).await?
                }
                _ => return Err(LspError::Protocol(definition.id.clone())),
            };
            normalize_result(tool, &result, &workspace, &path)
        };
        match tokio::time::timeout(timeout, operation).await {
            Ok(Ok(result)) => {
                self.clear_failure(&definition.id);
                Ok(result)
            }
            Ok(Err(error)) => {
                self.record_failure(&definition.id, &error);
                if matches!(error, LspError::Protocol(_) | LspError::ExternalLocation) {
                    let mut guard = slot.lock().await;
                    if let Some(mut running) = guard.take() {
                        running.terminate();
                        let _ = running.child.wait().await;
                    }
                }
                Err(error)
            }
            Err(_) => {
                let error = LspError::Timeout(definition.id.clone());
                self.record_failure(&definition.id, &error);
                let mut guard = slot.lock().await;
                if let Some(mut running) = guard.take() {
                    running.terminate();
                    let _ = running.child.wait().await;
                }
                Err(error)
            }
        }
    }

    async fn start_server(
        &self,
        definition: ServerDefinition,
        workspace: &WorkspaceContext,
    ) -> Result<RunningServer, LspError> {
        let executable = resolve_executable(&definition.config.command, &self.inner.environment)
            .ok_or_else(|| LspError::ExecutableMissing(definition.id.clone()))?;
        let temporary_directory = std::env::temp_dir().join(format!(
            "ca-lsp-{}-{}",
            safe_component(&definition.id),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir(&temporary_directory)
            .map_err(|_| LspError::Start(definition.id.clone()))?;
        let roots = workspace
            .roots()
            .iter()
            .map(|root| root.path.clone())
            .collect::<Vec<_>>();
        let prepared = SandboxBackend::new(self.inner.sandbox_mode)
            .prepare_process(
                &executable,
                &definition.config.args,
                &SandboxPolicy {
                    roots,
                    read_only_roots: Vec::new(),
                    temporary_directory: temporary_directory.clone(),
                    network_allowed: false,
                    hide_other_processes: false,
                },
            )
            .map_err(|_| LspError::Start(definition.id.clone()))?;
        let mut command = tokio::process::Command::new(prepared.program);
        command
            .args(prepared.arguments)
            .current_dir(workspace.cwd())
            .env_clear()
            .envs(&self.inner.environment)
            .env("TMPDIR", &temporary_directory)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        crate::run_environment::apply_tokio(&mut command);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.as_std_mut().process_group(0);
        }
        let mut child = command
            .spawn()
            .map_err(|_| LspError::Start(definition.id.clone()))?;
        let process_group = child.id().and_then(|id| i32::try_from(id).ok());
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| LspError::Start(definition.id.clone()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| LspError::Start(definition.id.clone()))?;
        let mut running = RunningServer {
            definition: definition.clone(),
            child,
            stdin,
            stdout: BufReader::new(stdout),
            process_group,
            temporary_directory,
            next_id: 1,
            documents: HashMap::new(),
            diagnostics: HashMap::new(),
        };
        let root_uri = workspace
            .primary()
            .and_then(|root| file_uri(&root.path).ok());
        let workspace_folders = workspace
            .roots()
            .iter()
            .filter_map(|root| {
                file_uri(&root.path).ok().map(|uri| {
                    json!({
                        "uri": uri,
                        "name": root.path.file_name().and_then(|value| value.to_str()).unwrap_or("workspace")
                    })
                })
            })
            .collect::<Vec<_>>();
        let initialized = running
            .request(
                "initialize",
                json!({
                    "processId": std::process::id(),
                    "rootUri": root_uri,
                    "workspaceFolders": workspace_folders,
                    "capabilities": {
                        "textDocument": {
                            "documentSymbol": {},
                            "definition": {"linkSupport": true},
                            "references": {},
                            "hover": {"contentFormat":["plaintext","markdown"]},
                            "implementation": {"linkSupport": true},
                            "callHierarchy": {}
                        }
                    },
                    "clientInfo": {"name":"ca","version":env!("CARGO_PKG_VERSION")}
                }),
            )
            .await;
        if initialized.is_err() {
            running.terminate();
            return Err(LspError::Protocol(definition.id));
        }
        running
            .notify("initialized", json!({}))
            .await
            .map_err(|_| LspError::Protocol(definition.id.clone()))?;
        Ok(running)
    }

    pub async fn shutdown(&self) {
        for slot in self.inner.slots.values() {
            let mut guard = slot.lock().await;
            if let Some(mut running) = guard.take() {
                let _ = tokio::time::timeout(Duration::from_millis(500), running.shutdown()).await;
                running.terminate();
                let _ = running.child.wait().await;
            }
        }
        self.inner
            .failures
            .write()
            .unwrap_or_else(|poison| poison.into_inner())
            .clear();
    }
}

pub fn is_lsp_tool(name: &str) -> bool {
    matches!(
        name,
        "lsp_symbols"
            | "lsp_definition"
            | "lsp_references"
            | "lsp_hover"
            | "lsp_implementation"
            | "lsp_call_hierarchy"
    )
}

impl Drop for ManagerInner {
    fn drop(&mut self) {
        for slot in self.slots.values() {
            if let Ok(mut guard) = slot.try_lock() {
                if let Some(mut running) = guard.take() {
                    running.terminate();
                }
            }
        }
    }
}

impl RunningServer {
    async fn request(&mut self, method: &str, params: Value) -> Result<Value, LspError> {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.write_message(&json!({
            "jsonrpc":"2.0",
            "id":id,
            "method":method,
            "params":params
        }))
        .await?;
        for _ in 0..128 {
            let message = self.read_message().await?;
            if message.get("id").and_then(Value::as_u64) == Some(id) {
                if message.get("error").is_some() {
                    return Err(LspError::Protocol(self.definition.id.clone()));
                }
                return message
                    .get("result")
                    .cloned()
                    .ok_or_else(|| LspError::Protocol(self.definition.id.clone()));
            }
            if message.get("method").and_then(Value::as_str)
                == Some("textDocument/publishDiagnostics")
            {
                self.retain_diagnostics(&message)?;
                continue;
            }
            if message.get("method").is_some() && message.get("id").is_some() {
                let response_id = message.get("id").cloned().unwrap_or(Value::Null);
                self.write_message(&json!({
                    "jsonrpc":"2.0",
                    "id":response_id,
                    "result":null
                }))
                .await?;
            }
        }
        Err(LspError::Protocol(self.definition.id.clone()))
    }

    async fn notify(&mut self, method: &str, params: Value) -> Result<(), LspError> {
        self.write_message(&json!({
            "jsonrpc":"2.0",
            "method":method,
            "params":params
        }))
        .await
    }

    async fn sync_document(
        &mut self,
        uri: &str,
        path: &Path,
        text: String,
    ) -> Result<i64, LspError> {
        match self.documents.get_mut(path) {
            None => {
                self.notify(
                    "textDocument/didOpen",
                    json!({"textDocument":{
                        "uri":uri,
                        "languageId":self.definition.config.language_id,
                        "version":1,
                        "text":text
                    }}),
                )
                .await?;
                self.documents
                    .insert(path.to_path_buf(), OpenDocument { version: 1, text });
            }
            Some(document) if document.text != text => {
                document.version = document.version.saturating_add(1);
                document.text = text.clone();
                let version = document.version;
                self.notify(
                    "textDocument/didChange",
                    json!({
                        "textDocument":{"uri":uri,"version":version},
                        "contentChanges":[{"text":text}]
                    }),
                )
                .await?;
            }
            Some(_) => {}
        }
        self.documents
            .get(path)
            .map(|document| document.version)
            .ok_or_else(|| LspError::Protocol(self.definition.id.clone()))
    }

    async fn close_document(&mut self, path: &Path) -> Result<(), LspError> {
        if self.documents.remove(path).is_some() {
            let uri = file_uri(path)?;
            self.notify("textDocument/didClose", json!({"textDocument":{"uri":uri}}))
                .await?;
        }
        self.diagnostics.remove(path);
        Ok(())
    }

    async fn wait_for_diagnostics(
        &mut self,
        path: &Path,
        expected_version: i64,
    ) -> Result<(), LspError> {
        if self
            .diagnostics
            .get(path)
            .is_some_and(|published| published.version == Some(expected_version))
        {
            return Ok(());
        }
        loop {
            let message = self.read_message().await?;
            if message.get("method").and_then(Value::as_str)
                == Some("textDocument/publishDiagnostics")
            {
                self.retain_diagnostics(&message)?;
                if self
                    .diagnostics
                    .get(path)
                    .is_some_and(|published| published.version == Some(expected_version))
                {
                    return Ok(());
                }
                continue;
            }
            if message.get("method").is_some() && message.get("id").is_some() {
                let response_id = message.get("id").cloned().unwrap_or(Value::Null);
                self.write_message(&json!({
                    "jsonrpc":"2.0",
                    "id":response_id,
                    "result":null
                }))
                .await?;
            }
        }
    }

    fn retain_diagnostics(&mut self, message: &Value) -> Result<(), LspError> {
        let params = message
            .get("params")
            .and_then(Value::as_object)
            .ok_or_else(|| LspError::Protocol(self.definition.id.clone()))?;
        let uri = params
            .get("uri")
            .and_then(Value::as_str)
            .ok_or_else(|| LspError::Protocol(self.definition.id.clone()))?;
        let path = self
            .documents
            .keys()
            .find(|path| file_uri(path).ok().as_deref() == Some(uri))
            .cloned();
        let Some(path) = path else {
            return Ok(());
        };
        let version = params.get("version").and_then(Value::as_i64);
        let values = params
            .get("diagnostics")
            .and_then(Value::as_array)
            .ok_or_else(|| LspError::Protocol(self.definition.id.clone()))?;
        let items = values
            .iter()
            .filter_map(parse_diagnostic)
            .take(MAX_DIAGNOSTIC_ITEMS.saturating_add(1))
            .collect();
        self.diagnostics
            .insert(path, PublishedDiagnostics { version, items });
        Ok(())
    }

    async fn write_message(&mut self, message: &Value) -> Result<(), LspError> {
        let body = serde_json::to_vec(message)
            .map_err(|_| LspError::Protocol(self.definition.id.clone()))?;
        if body.len() > MAX_FRAME_BYTES {
            return Err(LspError::Protocol(self.definition.id.clone()));
        }
        let header = format!("Content-Length: {}\r\n\r\n", body.len());
        self.stdin
            .write_all(header.as_bytes())
            .await
            .map_err(|_| LspError::Protocol(self.definition.id.clone()))?;
        self.stdin
            .write_all(&body)
            .await
            .map_err(|_| LspError::Protocol(self.definition.id.clone()))?;
        self.stdin
            .flush()
            .await
            .map_err(|_| LspError::Protocol(self.definition.id.clone()))
    }

    async fn read_message(&mut self) -> Result<Value, LspError> {
        let mut header_bytes = 0usize;
        let mut content_length = None;
        loop {
            let mut line = Vec::new();
            let count = self
                .stdout
                .read_until(b'\n', &mut line)
                .await
                .map_err(|_| LspError::Protocol(self.definition.id.clone()))?;
            if count == 0 {
                return Err(LspError::Protocol(self.definition.id.clone()));
            }
            header_bytes = header_bytes.saturating_add(count);
            if header_bytes > MAX_HEADER_BYTES {
                return Err(LspError::Protocol(self.definition.id.clone()));
            }
            if line == b"\r\n" || line == b"\n" {
                break;
            }
            let line = std::str::from_utf8(&line)
                .map_err(|_| LspError::Protocol(self.definition.id.clone()))?;
            if let Some(value) = line
                .trim()
                .strip_prefix("Content-Length:")
                .or_else(|| line.trim().strip_prefix("content-length:"))
            {
                let parsed = value
                    .trim()
                    .parse::<usize>()
                    .ok()
                    .filter(|length| *length <= MAX_FRAME_BYTES)
                    .ok_or_else(|| LspError::Protocol(self.definition.id.clone()))?;
                content_length = Some(parsed);
            }
        }
        let length =
            content_length.ok_or_else(|| LspError::Protocol(self.definition.id.clone()))?;
        let mut body = vec![0; length];
        self.stdout
            .read_exact(&mut body)
            .await
            .map_err(|_| LspError::Protocol(self.definition.id.clone()))?;
        serde_json::from_slice(&body).map_err(|_| LspError::Protocol(self.definition.id.clone()))
    }

    async fn shutdown(&mut self) -> Result<(), LspError> {
        let _ = self.request("shutdown", Value::Null).await;
        let _ = self.notify("exit", Value::Null).await;
        let _ = self.child.wait().await;
        Ok(())
    }

    fn terminate(&mut self) {
        kill_process_group(self.process_group);
        let _ = self.child.start_kill();
        let _ = std::fs::remove_dir_all(&self.temporary_directory);
    }
}

fn position_params(uri: &str, position: Option<Value>) -> Value {
    json!({
        "textDocument":{"uri":uri},
        "position":position.unwrap_or_else(|| json!({"line":0,"character":0}))
    })
}

async fn read_bounded_document(path: &Path) -> Result<String, LspError> {
    let metadata = tokio::fs::metadata(path)
        .await
        .map_err(|_| LspError::InvalidDocument)?;
    if !metadata.is_file() || metadata.len() > MAX_DOCUMENT_BYTES {
        return Err(LspError::InvalidDocument);
    }
    let bytes = tokio::fs::read(path)
        .await
        .map_err(|_| LspError::InvalidDocument)?;
    String::from_utf8(bytes).map_err(|_| LspError::InvalidDocument)
}

pub(crate) fn utf16_position(
    source: &str,
    line: usize,
    column: usize,
) -> Result<(u32, u32), LspError> {
    if line == 0 || column == 0 {
        return Err(LspError::InvalidPosition);
    }
    let line_text = source
        .split_inclusive('\n')
        .nth(line - 1)
        .or_else(|| (line == source.lines().count() + 1 && source.ends_with('\n')).then_some(""))
        .ok_or(LspError::InvalidPosition)?
        .trim_end_matches(['\r', '\n']);
    let scalar_count = line_text.chars().count();
    if column > scalar_count.saturating_add(1) {
        return Err(LspError::InvalidPosition);
    }
    let prefix = line_text.chars().take(column - 1).collect::<String>();
    let line = u32::try_from(line - 1).map_err(|_| LspError::InvalidPosition)?;
    let character =
        u32::try_from(prefix.encode_utf16().count()).map_err(|_| LspError::InvalidPosition)?;
    Ok((line, character))
}

fn argument_usize(arguments: &Value, name: &str) -> Result<usize, LspError> {
    arguments
        .get(name)
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .ok_or(LspError::InvalidPosition)
}

fn file_uri(path: &Path) -> Result<String, LspError> {
    reqwest::Url::from_file_path(path)
        .map(|url| url.to_string())
        .map_err(|_| LspError::InvalidDocument)
}

fn validate_item_uri(workspace: &WorkspaceContext, item: &Value) -> Result<(), LspError> {
    let uri = item
        .get("uri")
        .and_then(Value::as_str)
        .ok_or(LspError::ExternalLocation)?;
    workspace_location(workspace, uri).map(|_| ())
}

fn workspace_location(workspace: &WorkspaceContext, uri: &str) -> Result<String, LspError> {
    let url = reqwest::Url::parse(uri).map_err(|_| LspError::ExternalLocation)?;
    if url.scheme() != "file" || url.query().is_some() || url.fragment().is_some() {
        return Err(LspError::ExternalLocation);
    }
    let path = url
        .to_file_path()
        .map_err(|_| LspError::ExternalLocation)?
        .canonicalize()
        .map_err(|_| LspError::ExternalLocation)?;
    let root = workspace
        .roots()
        .iter()
        .find(|root| path.starts_with(&root.path))
        .ok_or(LspError::ExternalLocation)?;
    let relative = path
        .strip_prefix(&root.path)
        .map_err(|_| LspError::ExternalLocation)?;
    let value = if root.primary {
        relative.to_path_buf()
    } else {
        PathBuf::from(root.path.file_name().unwrap_or_default()).join(relative)
    };
    Ok(value.to_string_lossy().replace('\\', "/"))
}

fn normalize_result(
    tool: &str,
    result: &Value,
    workspace: &WorkspaceContext,
    document: &Path,
) -> Result<String, LspError> {
    match tool {
        "lsp_symbols" => normalize_symbols(result, workspace, document),
        "lsp_definition" | "lsp_references" | "lsp_implementation" => {
            normalize_locations(result, workspace)
        }
        "lsp_hover" => normalize_hover(result),
        "lsp_call_hierarchy" => normalize_calls(result, workspace),
        _ => Err(LspError::Protocol("unknown".into())),
    }
}

fn normalize_symbols(
    result: &Value,
    workspace: &WorkspaceContext,
    document: &Path,
) -> Result<String, LspError> {
    let items = result
        .as_array()
        .ok_or_else(|| LspError::Protocol("symbols".into()))?;
    let document_uri = file_uri(document)?;
    let mut rows = Vec::new();
    for item in items {
        collect_symbol(item, workspace, &document_uri, 0, &mut rows)?;
        if rows.len() >= MAX_RESULT_ITEMS {
            break;
        }
    }
    Ok(render_rows("Symbols", rows, items.len()))
}

fn collect_symbol(
    item: &Value,
    workspace: &WorkspaceContext,
    default_uri: &str,
    depth: usize,
    rows: &mut Vec<String>,
) -> Result<(), LspError> {
    if rows.len() >= MAX_RESULT_ITEMS {
        return Ok(());
    }
    let name = safe_text(
        item.get("name")
            .and_then(Value::as_str)
            .unwrap_or("<unnamed>"),
    );
    let kind = item.get("kind").and_then(Value::as_u64).unwrap_or(0);
    let (uri, range) = if let Some(location) = item.get("location") {
        (
            location
                .get("uri")
                .and_then(Value::as_str)
                .ok_or(LspError::ExternalLocation)?,
            location.get("range"),
        )
    } else {
        (
            default_uri,
            item.get("selectionRange").or_else(|| item.get("range")),
        )
    };
    let path = workspace_location(workspace, uri)?;
    let position = range_position(range)?;
    rows.push(format!(
        "{}{} (kind {}) — {}:{}:{}",
        "  ".repeat(depth.min(8)),
        name,
        kind,
        path,
        position.0,
        position.1
    ));
    if let Some(children) = item.get("children").and_then(Value::as_array) {
        for child in children {
            collect_symbol(child, workspace, default_uri, depth + 1, rows)?;
            if rows.len() >= MAX_RESULT_ITEMS {
                break;
            }
        }
    }
    Ok(())
}

fn normalize_locations(result: &Value, workspace: &WorkspaceContext) -> Result<String, LspError> {
    let values = match result {
        Value::Null => Vec::new(),
        Value::Array(values) => values.clone(),
        Value::Object(_) => vec![result.clone()],
        _ => return Err(LspError::Protocol("locations".into())),
    };
    let mut rows = Vec::new();
    for value in values.iter().take(MAX_RESULT_ITEMS) {
        let uri = value
            .get("uri")
            .or_else(|| value.get("targetUri"))
            .and_then(Value::as_str)
            .ok_or(LspError::ExternalLocation)?;
        let range = value
            .get("range")
            .or_else(|| value.get("targetSelectionRange"))
            .or_else(|| value.get("targetRange"));
        let path = workspace_location(workspace, uri)?;
        let (line, column) = range_position(range)?;
        rows.push(format!("{path}:{line}:{column}"));
    }
    Ok(render_rows("Locations", rows, values.len()))
}

fn normalize_hover(result: &Value) -> Result<String, LspError> {
    if result.is_null() {
        return Ok("Hover\n(no result)".into());
    }
    let contents = result
        .get("contents")
        .ok_or_else(|| LspError::Protocol("hover".into()))?;
    let mut parts = Vec::new();
    collect_hover_text(contents, &mut parts);
    let text = safe_text(&parts.join("\n"));
    Ok(if text.trim().is_empty() {
        "Hover\n(no result)".into()
    } else {
        format!("Hover\n{text}")
    })
}

fn collect_hover_text(value: &Value, output: &mut Vec<String>) {
    match value {
        Value::String(text) => output.push(text.clone()),
        Value::Array(values) => {
            for value in values.iter().take(MAX_RESULT_ITEMS) {
                collect_hover_text(value, output);
            }
        }
        Value::Object(object) => {
            if let Some(text) = object.get("value").and_then(Value::as_str) {
                output.push(text.to_string());
            }
        }
        _ => {}
    }
}

fn normalize_calls(result: &Value, workspace: &WorkspaceContext) -> Result<String, LspError> {
    let values = result
        .as_array()
        .ok_or_else(|| LspError::Protocol("call hierarchy".into()))?;
    let mut rows = Vec::new();
    for value in values.iter().take(MAX_RESULT_ITEMS) {
        let item = value
            .get("from")
            .or_else(|| value.get("to"))
            .ok_or_else(|| LspError::Protocol("call hierarchy".into()))?;
        let name = safe_text(
            item.get("name")
                .and_then(Value::as_str)
                .unwrap_or("<unnamed>"),
        );
        let uri = item
            .get("uri")
            .and_then(Value::as_str)
            .ok_or(LspError::ExternalLocation)?;
        let path = workspace_location(workspace, uri)?;
        let (line, column) =
            range_position(item.get("selectionRange").or_else(|| item.get("range")))?;
        rows.push(format!("{name} — {path}:{line}:{column}"));
    }
    Ok(render_rows("Calls", rows, values.len()))
}

fn range_position(range: Option<&Value>) -> Result<(u64, u64), LspError> {
    let start = range
        .and_then(|range| range.get("start"))
        .ok_or_else(|| LspError::Protocol("range".into()))?;
    let line = start
        .get("line")
        .and_then(Value::as_u64)
        .ok_or_else(|| LspError::Protocol("range".into()))?
        .saturating_add(1);
    let column = start
        .get("character")
        .and_then(Value::as_u64)
        .ok_or_else(|| LspError::Protocol("range".into()))?
        .saturating_add(1);
    Ok((line, column))
}

fn render_rows(label: &str, rows: Vec<String>, total: usize) -> String {
    let shown = rows.len();
    let mut output = format!("{label} ({shown} shown)");
    if rows.is_empty() {
        output.push_str("\n(no results)");
    } else {
        output.push('\n');
        output.push_str(&rows.join("\n"));
    }
    if total > shown {
        output.push_str(&format!("\n[… {} results omitted …]", total - shown));
    }
    output
}

fn bound_output(value: &str) -> String {
    let value = crate::terminal_text::sanitize_terminal_text(value);
    if value.len() <= MAX_RESULT_BYTES {
        return value;
    }
    let mut end = MAX_RESULT_BYTES.saturating_sub(48).min(value.len());
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    format!(
        "{}\n[… {} bytes omitted …]",
        &value[..end],
        value.len().saturating_sub(end)
    )
}

fn safe_text(value: &str) -> String {
    crate::terminal_text::sanitize_terminal_text(value)
        .chars()
        .take(4096)
        .collect()
}

fn parse_diagnostic(value: &Value) -> Option<LspDiagnostic> {
    let range = value.get("range")?;
    let start = range.get("start")?;
    let end = range.get("end")?;
    let code = value.get("code").and_then(|code| match code {
        Value::String(code) => Some(safe_text(code)),
        Value::Number(code) => Some(code.to_string()),
        _ => None,
    });
    Some(LspDiagnostic {
        severity: value.get("severity").and_then(Value::as_u64),
        source: value.get("source").and_then(Value::as_str).map(safe_text),
        code,
        start_line: start.get("line")?.as_u64()?,
        start_character: start.get("character")?.as_u64()?,
        end_line: end.get("line")?.as_u64()?,
        end_character: end.get("character")?.as_u64()?,
        message: safe_text(value.get("message")?.as_str()?),
    })
}

fn render_diagnostics(server: &str, version: i64, items: &[LspDiagnostic]) -> String {
    let mut output = format!(
        "LSP diagnostics [{}] for document version {version}: {}",
        safe_id(server),
        items.len()
    );
    if items.is_empty() {
        output.push_str(" (no diagnostics)");
        return output;
    }
    for diagnostic in items {
        let severity = match diagnostic.severity {
            Some(1) => "error",
            Some(2) => "warning",
            Some(3) => "information",
            Some(4) => "hint",
            _ => "unknown",
        };
        let mut origin = String::new();
        if let Some(source) = diagnostic.source.as_deref() {
            origin.push_str(source);
        }
        if let Some(code) = diagnostic.code.as_deref() {
            if !origin.is_empty() {
                origin.push('/');
            }
            origin.push_str(code);
        }
        let origin = if origin.is_empty() {
            String::new()
        } else {
            format!(" [{origin}]")
        };
        output.push_str(&format!(
            "\n- {severity}{origin} {}:{}-{}:{} — {}",
            diagnostic.start_line.saturating_add(1),
            diagnostic.start_character.saturating_add(1),
            diagnostic.end_line.saturating_add(1),
            diagnostic.end_character.saturating_add(1),
            diagnostic.message
        ));
    }
    output
}

fn bound_diagnostics(sections: Vec<String>) -> String {
    let joined = sections.join("\n");
    let mut output = String::new();
    let mut shown = 0usize;
    let mut omitted = 0usize;
    for line in joined.lines() {
        if line.starts_with("- ") {
            if shown >= MAX_DIAGNOSTIC_ITEMS {
                omitted = omitted.saturating_add(1);
                continue;
            }
            shown = shown.saturating_add(1);
        }
        if !output.is_empty() {
            output.push('\n');
        }
        output.push_str(line);
    }
    if omitted > 0 {
        output.push_str(&format!(
            "\n[… {omitted} diagnostics omitted at the 50-item limit …]"
        ));
    }
    let output = crate::terminal_text::sanitize_terminal_text(&output);
    if output.len() <= MAX_DIAGNOSTIC_BYTES {
        return output;
    }
    const TRUNCATION_MARKER: &str = "\n[… diagnostic output truncated at the 32 KiB limit …]";
    let mut end = MAX_DIAGNOSTIC_BYTES
        .saturating_sub(TRUNCATION_MARKER.len())
        .min(output.len());
    while end > 0 && !output.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{TRUNCATION_MARKER}", &output[..end])
}

fn safe_diagnostic_error(error: &LspError) -> &'static str {
    match error {
        LspError::ExecutableMissing(_) => "configured server executable is unavailable",
        LspError::Start(_) => "configured server could not start",
        LspError::Timeout(_) => "configured server timed out",
        LspError::InvalidDocument => "document is unsupported or unavailable",
        LspError::Protocol(_) | LspError::ExternalLocation => {
            "server returned invalid or unsafe diagnostic data"
        }
        LspError::InvalidConfig { .. } | LspError::UnknownServer(_) => {
            "server configuration is unavailable"
        }
        LspError::InvalidPosition => "server returned an invalid position",
    }
}

fn validate_definition(id: &str, config: &LspServerConfig) -> Result<(), LspError> {
    let invalid = |detail| LspError::InvalidConfig {
        server: safe_id(id),
        detail,
    };
    if id.is_empty()
        || id.len() > MAX_SERVER_NAME_BYTES
        || !id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        return Err(invalid(
            "server ID must contain only ASCII letters, digits, '-' or '_'",
        ));
    }
    if config.command.is_empty() || config.command.len() > 1024 {
        return Err(invalid("command is required and must be bounded"));
    }
    if config.args.len() > MAX_ARGUMENTS
        || config
            .args
            .iter()
            .any(|argument| argument.len() > MAX_ARGUMENT_BYTES)
    {
        return Err(invalid("arguments exceed the configured bounds"));
    }
    if config.language_id.is_empty()
        || config.language_id.len() > 64
        || !config
            .language_id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        return Err(invalid("language_id is invalid"));
    }
    if !(1..=60).contains(&config.timeout_secs) {
        return Err(invalid("timeout_secs must be between 1 and 60"));
    }
    if config.extensions.len() > 64
        || config.extensions.iter().any(|extension| {
            extension.is_empty()
                || extension.len() > 32
                || !extension.chars().all(|character| {
                    character.is_ascii_alphanumeric() || matches!(character, '-' | '_')
                })
        })
    {
        return Err(invalid("extensions are invalid"));
    }
    Ok(())
}

fn validate_extension(definition: &ServerDefinition, path: &Path) -> Result<(), LspError> {
    if definition.config.extensions.is_empty() {
        return Ok(());
    }
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    definition
        .config
        .extensions
        .iter()
        .any(|candidate| candidate == extension)
        .then_some(())
        .ok_or(LspError::InvalidDocument)
}

fn filtered_environment() -> BTreeMap<String, String> {
    ["PATH", "HOME", "LANG", "LC_ALL", "TERM"]
        .into_iter()
        .filter_map(|name| crate::run_environment::var(name).map(|value| (name.into(), value)))
        .collect()
}

fn resolve_executable(command: &str, environment: &BTreeMap<String, String>) -> Option<PathBuf> {
    let path = Path::new(command);
    if path.is_absolute() {
        return path.is_file().then(|| path.to_path_buf());
    }
    if command.contains('/') {
        return None;
    }
    environment.get("PATH").and_then(|value| {
        std::env::split_paths(value)
            .map(|directory| directory.join(command))
            .find(|candidate| candidate.is_file())
    })
}

fn safe_id(value: &str) -> String {
    let value = crate::terminal_text::sanitize_terminal_text(value);
    let bounded = value
        .chars()
        .take(MAX_SERVER_NAME_BYTES)
        .collect::<String>();
    if bounded.is_empty() {
        "<unknown>".into()
    } else {
        bounded
    }
}

fn safe_component(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .take(32)
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

#[cfg(test)]
mod tests {
    use super::*;

    const FAKE_SERVER: &str = r#"
import json, os, sys, time

log_path, pid_path, mode, external_uri = sys.argv[1:5]
with open(pid_path, "w", encoding="utf-8") as f:
    f.write(str(os.getpid()))
current_uri = None

def read_message():
    length = None
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            return None
        if line in (b"\r\n", b"\n"):
            break
        if line.lower().startswith(b"content-length:"):
            length = int(line.split(b":", 1)[1].strip())
    if length is None:
        return None
    return json.loads(sys.stdin.buffer.read(length))

def send(value):
    body = json.dumps(value, separators=(",", ":")).encode("utf-8")
    sys.stdout.buffer.write(("Content-Length: %d\r\n\r\n" % len(body)).encode("ascii"))
    sys.stdout.buffer.write(body)
    sys.stdout.buffer.flush()

def position_range(line=0, character=0):
    return {"start":{"line":line,"character":character},"end":{"line":line,"character":character+1}}

while True:
    message = read_message()
    if message is None:
        break
    with open(log_path, "a", encoding="utf-8") as f:
        f.write(json.dumps(message, separators=(",", ":")) + "\n")
    method = message.get("method")
    if method == "textDocument/didOpen":
        current_uri = message["params"]["textDocument"]["uri"]
    if method in ("textDocument/didOpen", "textDocument/didChange") and mode in ("diagnostics", "stale", "unversioned", "controls"):
        document = message["params"]["textDocument"]
        version = document["version"]
        published_version = version - 1 if mode == "stale" else version
        diagnostic_message = "unsafe\u001b[31m message" if mode == "controls" else "borrowed value does not live long enough"
        params = {
            "uri":document["uri"],
            "diagnostics":[{
                "range":position_range(0, 1),
                "severity":1,
                "source":"rustc",
                "code":"E0597",
                "message":diagnostic_message
            }]
        }
        if mode != "unversioned":
            params["version"] = published_version
        send({"jsonrpc":"2.0","method":"textDocument/publishDiagnostics","params":params})
    if "id" not in message:
        if method == "exit":
            break
        continue
    request_id = message["id"]
    if method == "initialize":
        result = {"capabilities":{}}
    elif method == "shutdown":
        result = None
    elif method == "textDocument/documentSymbol":
        result = [{"name":"symbol_%03d" % i,"kind":12,"selectionRange":position_range(i % 3, 0),"range":position_range(i % 3, 0)} for i in range(120)]
    elif method == "textDocument/definition":
        if mode == "malformed":
            sys.stdout.buffer.write(b"Content-Length: 1048577\r\n\r\n")
            sys.stdout.buffer.flush()
            time.sleep(60)
            continue
        uri = external_uri if mode == "external" else current_uri
        result = [{"uri":uri,"range":position_range(0, 1)}]
    elif method == "textDocument/references":
        result = [{"uri":current_uri,"range":position_range(0, 2)}]
    elif method == "textDocument/hover":
        if mode == "hang":
            time.sleep(60)
            continue
        result = {"contents":{"kind":"markdown","value":"**safe hover**"}}
    elif method == "textDocument/implementation":
        result = {"targetUri":current_uri,"targetSelectionRange":position_range(0, 3),"targetRange":position_range(0, 3),"originSelectionRange":position_range(0, 0)}
    elif method == "textDocument/prepareCallHierarchy":
        result = [{"name":"root_call","kind":12,"uri":current_uri,"range":position_range(),"selectionRange":position_range()}]
    elif method == "callHierarchy/incomingCalls":
        result = [{"from":{"name":"caller","kind":12,"uri":current_uri,"range":position_range(0,4),"selectionRange":position_range(0,4)},"fromRanges":[position_range()]}]
    elif method == "callHierarchy/outgoingCalls":
        result = [{"to":{"name":"callee","kind":12,"uri":current_uri,"range":position_range(0,5),"selectionRange":position_range(0,5)},"fromRanges":[position_range()]}]
    else:
        result = None
    send({"jsonrpc":"2.0","id":request_id,"result":result})
"#;

    struct FakeFixture {
        root: tempfile::TempDir,
        source: PathBuf,
        log: PathBuf,
        pid: PathBuf,
        script: PathBuf,
    }

    impl FakeFixture {
        fn new() -> Self {
            let root = tempfile::tempdir().unwrap();
            let source = root.path().join("sample.rs");
            let log = root.path().join("requests.jsonl");
            let pid = root.path().join("server.pid");
            let script = root.path().join("fake_lsp.py");
            std::fs::write(&source, "a😀b\n").unwrap();
            std::fs::write(&script, FAKE_SERVER).unwrap();
            Self {
                root,
                source,
                log,
                pid,
                script,
            }
        }

        fn manager(&self, mode: &str, external_uri: &str, timeout_secs: u64) -> LspManager {
            let mut config = LspConfig::default();
            config.servers.insert(
                "fake".into(),
                LspServerConfig {
                    command: python3_executable().display().to_string(),
                    args: vec![
                        self.script.display().to_string(),
                        self.log.display().to_string(),
                        self.pid.display().to_string(),
                        mode.into(),
                        external_uri.into(),
                    ],
                    language_id: "rust".into(),
                    extensions: vec!["rs".into()],
                    enabled: true,
                    timeout_secs,
                },
            );
            LspManager::load(
                &config,
                WorkspaceContext::discover(self.root.path()).unwrap(),
                SandboxMode::PolicyOnly,
            )
            .unwrap()
            .unwrap()
        }

        fn args(&self, line: usize, column: usize) -> Value {
            json!({
                "server":"fake",
                "path":self.source,
                "line":line,
                "column":column
            })
        }

        fn pid(&self) -> i32 {
            std::fs::read_to_string(&self.pid).unwrap().parse().unwrap()
        }
    }

    fn python3_executable() -> PathBuf {
        ["/usr/bin/python3", "/usr/local/bin/python3"]
            .into_iter()
            .map(PathBuf::from)
            .find(|path| path.is_file())
            .expect("Python 3 test fixture executable")
    }

    fn process_is_gone(pid: i32) -> bool {
        let status = std::process::Command::new("/bin/kill")
            .args(["-0", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
        !status.success()
    }

    #[test]
    fn converts_one_based_unicode_columns_to_zero_based_utf16() {
        let source = "a😀b\nsecond\n";
        assert_eq!(utf16_position(source, 1, 3).unwrap(), (0, 3));
        assert_eq!(utf16_position(source, 2, 1).unwrap(), (1, 0));
        assert_eq!(utf16_position(source, 0, 1), Err(LspError::InvalidPosition));
        assert_eq!(utf16_position(source, 1, 5), Err(LspError::InvalidPosition));
    }

    #[test]
    fn default_config_has_no_language_servers() {
        assert!(crate::config::CaConfig::default().lsp.servers.is_empty());
    }

    #[tokio::test]
    async fn configured_server_with_no_matching_extension_is_not_applicable() {
        let fixture = FakeFixture::new();
        let manager = fixture.manager("diagnostics", "unused", 2);
        let text = fixture.root.path().join("notes.txt");
        std::fs::write(&text, "notes\n").unwrap();
        let diagnostics = manager
            .diagnostics_after_mutation(&crate::undo::FileMutationReceipt::new(
                text,
                "write_file",
                crate::undo::FileState::Missing,
                crate::undo::FileState::Utf8("notes\n".into()),
            ))
            .await;
        assert_eq!(diagnostics, MutationDiagnostics::NotApplicable);
        assert!(manager
            .status_snapshot()
            .servers
            .iter()
            .all(|server| matches!(server.phase, LspServerPhase::Lazy)));
        manager.shutdown().await;
    }

    #[test]
    fn configuration_validation_is_bounded_and_secret_free() {
        let mut config = LspConfig::default();
        config.servers.insert(
            "bad/id".into(),
            LspServerConfig {
                command: "/secret/path/server".into(),
                args: vec![],
                language_id: "rust".into(),
                extensions: vec!["rs".into()],
                enabled: true,
                timeout_secs: 10,
            },
        );
        let workspace = WorkspaceContext::discover(".").unwrap();
        let error = LspManager::load(&config, workspace, SandboxMode::PolicyOnly).unwrap_err();
        let text = error.to_string();
        assert!(!text.contains("/secret/path/server"));
        assert!(text.contains("invalid LSP configuration"));
    }

    #[test]
    fn external_locations_are_rejected_without_echoing_the_uri() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let outside_file = outside.path().join("secret.rs");
        std::fs::write(&outside_file, "secret").unwrap();
        let workspace =
            WorkspaceContext::from_primary(root.path().to_path_buf(), root.path().to_path_buf());
        let uri = file_uri(&outside_file).unwrap();
        let error = workspace_location(&workspace, &uri).unwrap_err();
        assert_eq!(error, LspError::ExternalLocation);
        assert!(!error.to_string().contains("secret.rs"));
    }

    #[tokio::test]
    async fn one_fake_server_supports_all_six_bounded_request_families() {
        let fixture = FakeFixture::new();
        let manager = fixture.manager("normal", "unused", 5);
        assert_eq!(manager.configured_server_count(), 1);
        assert_eq!(
            manager.status_snapshot().servers,
            vec![LspServerStatus {
                id: "fake".into(),
                phase: LspServerPhase::Lazy,
            }]
        );

        let symbols = manager
            .execute(
                "lsp_symbols",
                &json!({"server":"fake","path":fixture.source}),
            )
            .await;
        assert!(symbols.is_success());
        assert_eq!(
            manager.status_snapshot().servers[0].phase,
            LspServerPhase::Running
        );
        assert!(symbols.model_text().contains("symbol_000"));
        assert!(symbols.model_text().contains("20 results omitted"));

        for (tool, expected) in [
            ("lsp_definition", "sample.rs:1:2"),
            ("lsp_references", "sample.rs:1:3"),
            ("lsp_hover", "safe hover"),
            ("lsp_implementation", "sample.rs:1:4"),
        ] {
            let result = manager.execute(tool, &fixture.args(1, 3)).await;
            assert!(result.is_success(), "{tool}: {}", result.model_text());
            assert!(
                result.model_text().contains(expected),
                "{tool}: {}",
                result.model_text()
            );
        }
        let incoming = manager
            .execute(
                "lsp_call_hierarchy",
                &json!({"server":"fake","path":fixture.source,"line":1,"column":3,"direction":"incoming"}),
            )
            .await;
        assert!(incoming.is_success());
        assert!(incoming.model_text().contains("caller"));
        let outgoing = manager
            .execute(
                "lsp_call_hierarchy",
                &json!({"server":"fake","path":fixture.source,"line":1,"column":3,"direction":"outgoing"}),
            )
            .await;
        assert!(outgoing.is_success());
        assert!(outgoing.model_text().contains("callee"));

        let log = std::fs::read_to_string(&fixture.log).unwrap();
        assert_eq!(log.matches("\"method\":\"initialize\"").count(), 1);
        let definition = log
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .find(|message| {
                message.get("method") == Some(&Value::String("textDocument/definition".into()))
            })
            .unwrap();
        assert_eq!(
            definition["params"]["position"],
            json!({"line":0,"character":3})
        );

        let pid = fixture.pid();
        manager.shutdown().await;
        assert_eq!(
            manager.status_snapshot().servers[0].phase,
            LspServerPhase::Lazy
        );
        assert!(process_is_gone(pid));
    }

    #[tokio::test]
    async fn external_protocol_locations_fail_safely_and_kill_the_server() {
        let fixture = FakeFixture::new();
        let outside = tempfile::tempdir().unwrap();
        let outside_file = outside.path().join("do-not-leak.rs");
        std::fs::write(&outside_file, "secret").unwrap();
        let outside_uri = file_uri(&outside_file).unwrap();
        let manager = fixture.manager("external", &outside_uri, 5);
        let result = manager.execute("lsp_definition", &fixture.args(1, 1)).await;
        assert!(!result.is_success());
        assert_eq!(result.model_text(), LspError::ExternalLocation.to_string());
        assert!(!result.model_text().contains("do-not-leak"));
        assert!(process_is_gone(fixture.pid()));
    }

    #[tokio::test]
    async fn timeout_and_oversized_frame_terminate_the_owned_process_group() {
        for (mode, expected) in [
            ("hang", "timed out"),
            ("malformed", "invalid or incomplete"),
        ] {
            let fixture = FakeFixture::new();
            let manager = fixture.manager(mode, "unused", 1);
            let tool = if mode == "hang" {
                "lsp_hover"
            } else {
                "lsp_definition"
            };
            let result = manager.execute(tool, &fixture.args(1, 1)).await;
            assert!(!result.is_success());
            assert!(
                result.model_text().contains(expected),
                "{}",
                result.model_text()
            );
            assert!(matches!(
                manager.status_snapshot().servers[0].phase,
                LspServerPhase::Failed(_)
            ));
            assert!(process_is_gone(fixture.pid()));
        }
    }

    #[tokio::test]
    async fn post_edit_diagnostics_require_the_new_document_version_and_are_sanitized() {
        let fixture = FakeFixture::new();
        let manager = fixture.manager("diagnostics", "unused", 2);
        std::fs::write(
            &fixture.source,
            "fn main() { let value = &String::new(); }\n",
        )
        .unwrap();
        let receipt = crate::undo::FileMutationReceipt::new(
            fixture.source.clone(),
            "write_file",
            crate::undo::FileState::Utf8("a😀b\n".into()),
            crate::undo::FileState::Utf8("fn main() { let value = &String::new(); }\n".into()),
        );
        let first = manager.diagnostics_after_mutation(&receipt).await;
        let first = first.report().unwrap();
        assert!(first.contains("document version 1"), "{first}");
        assert!(first.contains("error [rustc/E0597] 1:2-1:3"));
        assert!(first.contains("borrowed value"));

        std::fs::write(&fixture.source, "fn main() {}\n").unwrap();
        let second = manager
            .diagnostics_after_mutation(&crate::undo::FileMutationReceipt::new(
                fixture.source.clone(),
                "edit_file",
                receipt.after,
                crate::undo::FileState::Utf8("fn main() {}\n".into()),
            ))
            .await;
        let second = second.report().unwrap();
        assert!(second.contains("document version 2"), "{second}");
        manager.shutdown().await;

        let controls = FakeFixture::new();
        let manager = controls.manager("controls", "unused", 2);
        let report = manager
            .diagnostics_after_mutation(&crate::undo::FileMutationReceipt::new(
                controls.source.clone(),
                "write_file",
                crate::undo::FileState::Missing,
                crate::undo::FileState::Utf8("a😀b\n".into()),
            ))
            .await;
        let report = report.report().unwrap();
        assert!(!report.contains('\u{1b}'));
        assert!(report.contains("unsafe message"));
        manager.shutdown().await;
    }

    #[tokio::test]
    async fn stale_timeout_and_deletion_are_best_effort_and_explicit() {
        let fixture = FakeFixture::new();
        let manager = fixture.manager("stale", "unused", 1);
        let receipt = crate::undo::FileMutationReceipt::new(
            fixture.source.clone(),
            "write_file",
            crate::undo::FileState::Missing,
            crate::undo::FileState::Utf8("a😀b\n".into()),
        );
        let stale = manager.diagnostics_after_mutation(&receipt).await;
        let stale = stale.report().unwrap();
        assert!(stale.contains("stale"), "{stale}");
        assert!(stale.contains("version 0"), "{stale}");

        std::fs::remove_file(&fixture.source).unwrap();
        let closed = manager
            .diagnostics_after_mutation(&crate::undo::FileMutationReceipt::new(
                fixture.source.clone(),
                "delete_file",
                receipt.after,
                crate::undo::FileState::Missing,
            ))
            .await;
        let closed = closed.report().unwrap();
        assert!(closed.contains("document closed"), "{closed}");
        manager.shutdown().await;
        let log = std::fs::read_to_string(&fixture.log).unwrap();
        assert!(log.contains("textDocument/didClose"));

        let unversioned = FakeFixture::new();
        let manager = unversioned.manager("unversioned", "unused", 1);
        let report = manager
            .diagnostics_after_mutation(&crate::undo::FileMutationReceipt::new(
                unversioned.source.clone(),
                "write_file",
                crate::undo::FileState::Missing,
                crate::undo::FileState::Utf8("a😀b\n".into()),
            ))
            .await;
        let report = report.report().unwrap();
        assert!(report.contains("no document version"), "{report}");
        manager.shutdown().await;
    }

    #[test]
    fn post_edit_diagnostic_rendering_enforces_item_byte_and_control_bounds() {
        let items = (0..80)
            .map(|index| LspDiagnostic {
                severity: Some(2),
                source: Some("fixture".into()),
                code: Some(index.to_string()),
                start_line: index,
                start_character: 0,
                end_line: index,
                end_character: 1,
                message: format!("{}\u{1b}[31m", "x".repeat(900)),
            })
            .collect::<Vec<_>>();
        let bounded = bound_diagnostics(vec![render_diagnostics("fake", 7, &items)]);
        assert!(bounded.len() <= MAX_DIAGNOSTIC_BYTES);
        let shown = bounded
            .lines()
            .filter(|line| line.starts_with("- "))
            .count();
        assert!((1..=50).contains(&shown));
        assert!(
            bounded.contains("diagnostics omitted")
                || bounded.contains("diagnostic output truncated")
        );
        assert!(!bounded.contains('\u{1b}'));
    }
}
