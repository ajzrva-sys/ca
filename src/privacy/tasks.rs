//! ca-owned private file broker. Workers receive a capability to local IPC,
//! never a mapping copy or an independently writable alias namespace.
use super::Context;
use ca_automation::private_files::{MoveBinding, PrivateFiles};
use ca_automation_protocol::private_files::{FileMoveProgressV1, MoveFilesV1, SelectFilesV1};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use uuid::Uuid;

const MAX_RPC: usize = 128 * 1024 * 1024;
const RPC_SCHEMA: &str = "ca.privacy-broker.v1";

/// Host-only worker admission. It is excluded from provider prompts and traces.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Binding {
    schema: String,
    pub session_id: Uuid,
    endpoint: PathBuf,
    capability: String,
    pub(crate) policy_fingerprint: String,
    pub(crate) replay_policy_digest: String,
    pub(crate) policy_revision: u64,
    #[serde(default)]
    pub(crate) trace: bool,
}
impl std::fmt::Debug for Binding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PrivacyBinding")
            .field("session_id", &self.session_id)
            .finish_non_exhaustive()
    }
}
impl Binding {
    pub(crate) fn directory(&self) -> &Path {
        self.endpoint.parent().expect("broker endpoint")
    }
    pub(crate) fn resource(&self) -> String {
        format!("privacy-context:{}", self.session_id)
    }
}

pub(super) struct Broker {
    binding: Binding,
    _directory: tempfile::TempDir,
    stop: Arc<AtomicBool>,
}
impl Drop for Broker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}

static REMOTE: OnceLock<Binding> = OnceLock::new();
static PROGRESS: OnceLock<Mutex<HashMap<String, FileMoveProgressV1>>> = OnceLock::new();
static CANCELS: OnceLock<Mutex<HashMap<String, Arc<AtomicBool>>>> = OnceLock::new();
fn cancel_flag(token: &str) -> Result<Arc<AtomicBool>, String> {
    if token.len() != 32 || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("private cancellation reference is invalid".into());
    }
    Ok(CANCELS
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| "private cancellation owner stopped")?
        .entry(token.into())
        .or_default()
        .clone())
}
pub(crate) struct MoveGuard {
    pub(crate) token: String,
    armed: bool,
    remote: Option<Binding>,
}
impl MoveGuard {
    pub(crate) fn new() -> Self {
        Self {
            token: Uuid::new_v4().simple().to_string(),
            armed: true,
            remote: remote_binding(),
        }
    }
    pub(crate) fn disarm(&mut self) {
        self.armed = false;
    }
}
impl Drop for MoveGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let token = self.token.clone();
        if let Some(binding) = self.remote.clone() {
            let _ = std::thread::spawn(move || rpc(&binding, "cancel", json!({"token":token})));
        } else if let Ok(flag) = cancel_flag(&token) {
            flag.store(true, Ordering::Release);
        }
    }
}
fn progress_map() -> &'static Mutex<HashMap<String, FileMoveProgressV1>> {
    PROGRESS.get_or_init(Default::default)
}
pub(crate) fn progress(selection: &str) -> Option<FileMoveProgressV1> {
    progress_map().lock().ok()?.get(selection).cloned()
}
fn report(selection: &str, value: &FileMoveProgressV1) {
    if let Ok(mut map) = progress_map().lock() {
        map.insert(selection.into(), value.clone());
    }
}
#[cfg(test)]
pub(crate) fn publish_test_progress(selection: &str, progress: &FileMoveProgressV1) {
    report(selection, progress);
}

pub(crate) fn remote_binding() -> Option<Binding> {
    REMOTE.get().cloned()
}
pub(crate) fn active_binding() -> Result<Option<Binding>, String> {
    if let Some(binding) = REMOTE.get() {
        return Ok(Some(binding.clone()));
    }
    super::current()
        .filter(|c| c.enabled())
        .map(|c| binding(&c))
        .transpose()
}
pub(crate) fn activate(binding: Binding) -> Result<(), String> {
    if binding.schema != RPC_SCHEMA
        || binding.session_id.is_nil()
        || binding.capability.len() != 64
        || !binding.endpoint.is_absolute()
    {
        return Err("private worker binding is invalid".into());
    }
    let response = rpc(&binding, "ping", Value::Null)?;
    if response != json!(binding.session_id) {
        return Err("private worker binding belongs to another context".into());
    }
    if let Some(old) = REMOTE.get() {
        if old != &binding {
            return Err("private worker context changed".into());
        }
        return Ok(());
    }
    REMOTE
        .set(binding)
        .map_err(|_| "private worker context was already bound".into())
}
pub(crate) fn adopt_worker(resources: &[String], binding: Option<&Binding>) -> Result<(), String> {
    let expected = resources
        .iter()
        .filter(|r| r.starts_with("privacy-context:"))
        .collect::<Vec<_>>();
    match binding {
        Some(binding) if expected.len() == 1 && *expected[0] == binding.resource() => {
            activate(binding.clone())
        }
        None if expected.is_empty() => Ok(()),
        _ => Err("worker lost its admitted private context".into()),
    }
}

/// Tool-less planning helpers use the existing contained native runner. Other
/// helpers retain their normal tool/approval path and share the parent Arc.
pub(crate) async fn contained_planner(
    context: Arc<Context>,
    run: &mut crate::agent::subagent::AgentRunContext,
    provider: &str,
    model: &str,
    reasoning_effort: Option<crate::config::ReasoningEffort>,
) -> Option<Result<String, String>> {
    // Filename-mode children retain the host's exact mapping Arc. The contained
    // worker protocol does not carry the structured filename namespace.
    if context.filenames_enabled()
        || crate::jobs::worker_contained()
        || !run.request.grants.tools.is_empty()
        || run.graph_budget.is_some()
        || run.request.output_contract.is_some()
    {
        return None;
    }
    let files = context.private_files().ok()?;
    if files.folders().ok()?.is_empty() {
        return None;
    }
    let paths = crate::runtime::RuntimePaths::discover().ok()?;
    let profile = if context.scope == paths.data_dir {
        crate::runtime::ActiveProfile::Default
    } else {
        let suffix = context
            .scope
            .strip_prefix(paths.data_dir.join("profiles"))
            .ok()?;
        crate::runtime::ActiveProfile::parse(suffix.to_str()).ok()?
    };
    Some(
        run_contained_planner(
            context,
            run,
            provider,
            model,
            paths,
            profile,
            reasoning_effort,
        )
        .await,
    )
}
async fn run_contained_planner(
    context: Arc<Context>,
    run: &mut crate::agent::subagent::AgentRunContext,
    provider: &str,
    model: &str,
    paths: crate::runtime::RuntimePaths,
    profile: crate::runtime::ActiveProfile,
    reasoning_effort: Option<crate::config::ReasoningEffort>,
) -> Result<String, String> {
    let workspace = run
        .request
        .workspace
        .cwd()
        .canonicalize()
        .map_err(|_| "private planner workspace is unavailable")?;
    let prompt=format!("Review or plan this private file task. Use only the anonymous references already provided; the host owns all paths and execution. Return a concise recommendation.\n\nContext:\n{}\n\nTask:\n{}",run.request.context_summary,run.request.task);
    let spec = crate::schedule::WorkerSpec {
        automation_budget: None,
        native_context: None,
        native_sandbox_binding: None,
        privacy_binding: Some(binding(&context)?),
        worker_max_turns: Some(1),
        automation_pricing: None,
        provider: provider.into(),
        model: model.into(),
        workspace: workspace.clone(),
        prompt,
        tools: Vec::new(),
        grants: Vec::new(),
        allow_network: true,
        runtime_secs: run.limits.runtime.as_secs().clamp(1, 86400),
        reasoning_effort,
        history: Vec::new(),
    };
    let mut execution = crate::schedule_compat_runtime::execution_context(
        &paths,
        &profile,
        run.id.0,
        &spec,
        &json!({"tools":[],"privacy":context.session_id}),
        &workspace,
        spec.runtime_secs,
        run.limits
            .retained_output_bytes
            .clamp(1024, 16 * 1024 * 1024) as u64,
    )?;
    execution.resources.push(binding(&context)?.resource());
    let native = crate::schedule_compat_runtime::LegacyRun::open(execution)?;
    native.begin_action("target")?;
    let _ = run
        .events
        .send(crate::agent::subagent::RunnerEvent::Activity(
            "Reviewing the private batch".into(),
        ));
    let result = native.scope(crate::schedule::run_worker(spec, &profile));
    tokio::pin!(result);
    let output = tokio::select! {
        result=&mut result=>result.map_err(|_|"contained private planner failed; its local run is retained".to_string())?,
        _=run.cancelled.changed()=>{native.store.cancel().map_err(|_|"private planner cancellation could not be saved")?;let _=result.await;return Err("private planner cancelled".into());}
    };
    let result = crate::automation_runtime::NativeResult {
        content: output.content,
        usage: crate::automation::AutomationUsageCountersV1 {
            provider_requests: 1,
            ..Default::default()
        },
    };
    native.finish_success(&result)?;
    let _ = run.events.send(crate::agent::subagent::RunnerEvent::Output(
        result.content.clone(),
    ));
    Ok(result.content)
}

pub(crate) fn binding(context: &Arc<Context>) -> Result<Binding, String> {
    let mut slot = context
        .task_broker
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    if let Some(broker) = slot
        .as_ref()
        .filter(|b| b.binding.policy_fingerprint == context.policy_fingerprint())
    {
        return Ok(broker.binding.clone());
    }
    *slot = None;
    #[cfg(unix)]
    {
        use std::os::unix::net::UnixListener;
        let directory = tempfile::Builder::new()
            .prefix("ca-privacy-")
            .tempdir_in("/private/tmp")
            .map_err(|_| "private worker IPC is unavailable")?;
        let endpoint = directory.path().join("broker");
        let listener =
            UnixListener::bind(&endpoint).map_err(|_| "private worker IPC could not be opened")?;
        listener
            .set_nonblocking(true)
            .map_err(|_| "private worker IPC could not be configured")?;
        let binding = Binding {
            schema: RPC_SCHEMA.into(),
            session_id: context.session_id,
            endpoint,
            capability: format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple()),
            policy_fingerprint: context.policy_fingerprint(),
            replay_policy_digest: context.replay_policy_digest(),
            policy_revision: context.policy().revision,
            trace: crate::provider_trace::process_hub().is_some_and(|hub| hub.status().enabled),
        };
        let stop = Arc::new(AtomicBool::new(false));
        let weak = Arc::downgrade(context);
        let expected = binding.clone();
        let stopping = stop.clone();
        std::thread::Builder::new()
            .name("ca-private-broker".into())
            .spawn(move || {
                while !stopping.load(Ordering::Acquire) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            let weak = weak.clone();
                            let expected = expected.clone();
                            let _ = std::thread::Builder::new()
                                .name("ca-private-rpc".into())
                                .spawn(move || serve(stream, weak, &expected));
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(std::time::Duration::from_millis(20))
                        }
                        Err(_) => break,
                    }
                }
            })
            .map_err(|_| "private worker IPC could not start")?;
        *slot = Some(Broker {
            binding: binding.clone(),
            _directory: directory,
            stop,
        });
        Ok(binding)
    }
    #[cfg(not(unix))]
    {
        let _ = context;
        Err("private workers are unavailable on this platform".into())
    }
}

#[derive(Serialize, Deserialize)]
struct Packet {
    capability: String,
    operation: String,
    value: Value,
}
#[cfg(unix)]
fn frame_write(
    stream: &mut std::os::unix::net::UnixStream,
    value: &impl Serialize,
) -> Result<(), String> {
    let bytes = serde_json::to_vec(value).map_err(|_| "private IPC encoding failed")?;
    if bytes.len() > MAX_RPC {
        return Err("private IPC exceeds its bound".into());
    }
    stream
        .write_all(&(bytes.len() as u64).to_be_bytes())
        .and_then(|_| stream.write_all(&bytes))
        .map_err(|_| "private IPC disconnected".into())
}
#[cfg(unix)]
fn frame_read<T: serde::de::DeserializeOwned>(
    stream: &mut std::os::unix::net::UnixStream,
) -> Result<T, String> {
    let mut n = [0u8; 8];
    stream
        .read_exact(&mut n)
        .map_err(|_| "private IPC disconnected")?;
    let length = u64::from_be_bytes(n);
    if length > MAX_RPC as u64 {
        return Err("private IPC exceeds its bound".into());
    }
    let mut bytes = vec![0; length as usize];
    stream
        .read_exact(&mut bytes)
        .map_err(|_| "private IPC disconnected")?;
    serde_json::from_slice(&bytes).map_err(|_| "private IPC response is invalid".into())
}
pub(crate) fn rpc(binding: &Binding, operation: &str, value: Value) -> Result<Value, String> {
    #[cfg(unix)]
    {
        let mut stream =
            std::os::unix::net::UnixStream::connect(&binding.endpoint).map_err(|_| {
                "The private task owner is unavailable; restore its context before continuing"
            })?;
        let timeout = std::time::Duration::from_secs(if operation == "move" { 86400 } else { 120 });
        stream
            .set_read_timeout(Some(timeout))
            .map_err(|_| "private IPC timeout is unavailable")?;
        stream
            .set_write_timeout(Some(timeout))
            .map_err(|_| "private IPC timeout is unavailable")?;
        frame_write(
            &mut stream,
            &Packet {
                capability: binding.capability.clone(),
                operation: operation.into(),
                value,
            },
        )?;
        let result: Result<Value, String> = frame_read(&mut stream)?;
        result
    }
    #[cfg(not(unix))]
    {
        let _ = (binding, operation, value);
        Err("private IPC is unavailable".into())
    }
}
#[cfg(unix)]
fn serve(mut stream: std::os::unix::net::UnixStream, context: Weak<Context>, binding: &Binding) {
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(120)));
    let result = (|| {
        // macOS accept inherits O_NONBLOCK from the listener. Framed reads must
        // wait for the client packet rather than treating EAGAIN as disconnect.
        stream
            .set_nonblocking(false)
            .map_err(|_| "private IPC blocking mode is unavailable")?;
        let packet: Packet = frame_read(&mut stream)?;
        if packet.capability != binding.capability {
            return Err("private IPC capability is invalid".into());
        }
        let context = context
            .upgrade()
            .ok_or("private task owner is unavailable")?;
        context.refresh_policy()?;
        if context.policy_fingerprint() != binding.policy_fingerprint {
            return Err("private worker policy changed; restore its binding".into());
        }
        let stopped = Arc::new(AtomicBool::new(false));
        if packet.operation == "move" {
            if let Some(token) = packet.value.get("cancel_token").and_then(Value::as_str) {
                let flag = cancel_flag(token)?;
                let socket = stream
                    .try_clone()
                    .map_err(|_| "private worker connection stopped")?;
                let finished = stopped.clone();
                std::thread::spawn(move || {
                    use std::os::fd::AsRawFd;
                    while !finished.load(Ordering::Acquire) {
                        let mut byte = 0u8;
                        if unsafe {
                            libc::recv(
                                socket.as_raw_fd(),
                                (&mut byte as *mut u8).cast(),
                                1,
                                libc::MSG_PEEK | libc::MSG_DONTWAIT,
                            )
                        } == 0
                        {
                            flag.store(true, Ordering::Release);
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(50));
                    }
                });
            }
        }
        let result = dispatch(&context, &packet.operation, packet.value);
        stopped.store(true, Ordering::Release);
        if context.policy_fingerprint() != binding.policy_fingerprint {
            return Err("private worker policy changed before transmission".into());
        }
        result
    })();
    let _ = frame_write(&mut stream, &result);
}

#[cfg(all(test, unix))]
pub(super) fn serve_test_stream(
    stream: std::os::unix::net::UnixStream,
    context: &Arc<Context>,
    binding: &Binding,
) {
    serve(stream, Arc::downgrade(context), binding);
}

fn dispatch(context: &Arc<Context>, operation: &str, value: Value) -> Result<Value, String> {
    let files = context.private_files()?;
    let text = |key: &str| {
        value
            .get(key)
            .and_then(Value::as_str)
            .ok_or_else(|| "private operation arguments are invalid".to_string())
    };
    match operation {
        "ping" => Ok(json!(context.session_id)),
        "wire" => {
            context.refresh_policy()?;
            context.discover_selected_sources()?;
            context.refresh_classification()?;
            context.refresh_task_names()?;
            let mut body = value;
            mask_json(context, &mut body)?;
            context.mask_task_wire_value(&mut body)?;
            if let Some(filter) = context.task_filter() {
                let bytes =
                    serde_json::to_vec(&body).map_err(|_| "private request cannot be encoded")?;
                body = serde_json::from_slice(
                    &filter
                        .prepare_request(&bytes)
                        .map_err(|error| error.to_string())?,
                )
                .map_err(|_| "filtered private request is invalid")?;
            }
            context.commit_registry()?;
            // No outbound value is trusted just because it came from a worker.
            let bytes =
                serde_json::to_vec(&body).map_err(|_| "private request cannot be encoded")?;
            context.validate_wire(
                &bytes,
                context.policy().revision,
                &context.policy_fingerprint(),
            )?;
            Ok(body)
        }
        "inspect" => {
            let bytes =
                serde_json::to_vec(&value).map_err(|_| "private provider request is invalid")?;
            context.validate_wire(
                &bytes,
                context.policy().revision,
                &context.policy_fingerprint(),
            )?;
            if context.project_task_wire(&bytes)? != bytes {
                return Err("A private task value appeared after replay projection; prepare this request again".into());
            }
            Ok(Value::Null)
        }
        "restore" => context.restore_arguments(&value),
        "prepare_edit" => context.prepare_edit_ticket(
            text("name")?,
            value.get("args").ok_or("private arguments missing")?,
        ),
        "read_file" => {
            let path = text("path")?;
            if Path::new(path).starts_with(context.private_metadata_root())
                || files.is_managed_path(Path::new(path))
            {
                return Err("host-owned task metadata is private".into());
            }
            let (output, source) = crate::agent::tools::read_private_file_range_blocking(
                context.clone(),
                path,
                path,
                value
                    .get("start_line")
                    .and_then(Value::as_u64)
                    .map(|v| v as usize),
                value
                    .get("end_line")
                    .and_then(Value::as_u64)
                    .map(|v| v as usize),
                value
                    .get("include_line_hashes")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            )?;
            Ok(json!({"text":output,"source":source}))
        }
        "consume_edit" => {
            context.consume_edit_ticket_if_required(
                text("name")?,
                value.get("args").ok_or("private arguments missing")?,
            )?;
            Ok(Value::Null)
        }
        "folder" => {
            let reference = text("reference")?;
            let (id, suffix) = reference.split_once('/').unwrap_or((reference, ""));
            let path = files
                .resolve_resource(id)
                .map_err(|_| "private resource is missing or changed")?;
            if !suffix.is_empty()
                && (!id.starts_with("folder_")
                    || Path::new(suffix)
                        .components()
                        .any(|c| !matches!(c, std::path::Component::Normal(_))))
            {
                return Err("private resource suffix is invalid".into());
            }
            Ok(json!({"path":if suffix.is_empty(){path}else{path.join(suffix)}}))
        }
        "protected_folders" => Ok(json!(files
            .folders()
            .map_err(|_| "private folders are unavailable")?
            .iter()
            .map(|r| r.path.path())
            .collect::<Vec<_>>())),
        "managed_path" => {
            let path = Path::new(text("path")?);
            Ok(json!(
                path.starts_with(context.private_metadata_root()) || files.is_managed_path(path)
            ))
        }
        "move_binding" => {
            let b = files
                .bind_move(text("selection")?, text("destination")?)
                .map_err(|_| "private selection or destination is stale; select again")?;
            let s = files
                .selection(&b.selection)
                .map_err(|_| "private selection is unavailable")?;
            let source = files
                .folder(&s.folder)
                .map_err(|_| "private source changed")?;
            let destination = files
                .folder(&b.destination)
                .map_err(|_| "private destination changed")?;
            let paths = s
                .members
                .iter()
                .flat_map(|member| {
                    let relative = member.relative.path();
                    [
                        ca_automation::private_files::NativePath::new(
                            &source.path.path().join(&relative),
                        ),
                        ca_automation::private_files::NativePath::new(
                            &destination.path.path().join(relative),
                        ),
                    ]
                })
                .collect::<Vec<_>>();
            Ok(
                json!({"binding":b,"source":source.path.path(),"destination":destination.path.path(),"private_root":files.root(),"count":s.members.len(),"paths":paths}),
            )
        }
        "select" => {
            let mut args: SelectFilesV1 = serde_json::from_value(value)
                .map_err(|_| "private selection arguments are invalid")?;
            let included = if context.filename_mode {
                if let Some(pattern) = args.criteria.name_glob.as_deref().filter(|_pattern| false) {
                    context.refresh_filenames()?;
                    let native = files
                        .resolve_resource(&args.folder)
                        .map_err(|_| "Selection source is unavailable")?;
                    let root = if native.is_file() {
                        native.parent().ok_or("Selection source has no parent")?
                    } else {
                        native.as_path()
                    };
                    let selected = context
                        .state
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .filenames
                        .as_mut()
                        .ok_or("Filename mappings are unavailable")?
                        .matching_files(root, pattern)?;
                    args.criteria.name_glob = None;
                    Some(selected)
                } else {
                    None
                }
            } else {
                None
            };
            let selection = files
                .select_including(
                    &args.folder,
                    args.destination.as_deref(),
                    args.criteria,
                    &[context.private_metadata_root()],
                    included.as_ref(),
                )
                .map_err(|_| {
                    "private selection failed; inspect the local folders and matching criteria"
                })?;
            context.refresh_task_names()?;
            context.commit_registry()?;
            let response = json!({"selection":selection.reference,"version":selection.version,"count":selection.members.len(),"types":selection.types,"folder":selection.folder,"destination":selection.destination});
            files
                .publish_control(&response)
                .map_err(|_| "private selection receipt could not be saved")?;
            Ok(response)
        }
        "move" => {
            let token = value
                .get("cancel_token")
                .and_then(Value::as_str)
                .map(str::to_owned);
            let args: MoveFilesV1 =
                serde_json::from_value(value.get("args").cloned().unwrap_or(value))
                    .map_err(|_| "private move arguments are invalid")?;
            execute_local(context, &args, token.as_deref())
        }
        "cancel" => {
            cancel_flag(text("token")?)?.store(true, Ordering::Release);
            Ok(Value::Null)
        }
        _ => Err("unknown private operation".into()),
    }
}

pub(crate) fn call(operation: &str, value: Value) -> Result<Value, String> {
    if let Some(binding) = REMOTE.get() {
        return rpc(binding, operation, value);
    }
    let context = super::current()
        .filter(|c| c.enabled())
        .ok_or("Private file tools require an active filtered conversation")?;
    dispatch(&context, operation, value)
}
pub(crate) fn call_for(
    context: &Arc<Context>,
    operation: &str,
    value: Value,
) -> Result<Value, String> {
    dispatch(context, operation, value)
}

pub(crate) fn prepare_invocation(
    workspace: &crate::workspace::WorkspaceContext,
    name: &str,
    args: &Value,
) -> Result<Value, crate::workspace::WorkspaceError> {
    use crate::workspace::{PathOperation, WorkspaceError};
    let invalid = WorkspaceError::InvalidArguments;
    let mut prepared = args.clone();
    if name == "select_files" {
        for field in ["folder", "destination"] {
            if let Some(id) = args.get(field).and_then(Value::as_str) {
                let value = call("folder", json!({"reference":id})).map_err(invalid)?;
                let path = value["path"]
                    .as_str()
                    .ok_or_else(|| invalid("private native folder cannot be resolved".into()))?;
                workspace.resolve(path, PathOperation::Read)?;
            }
        }
    } else if name == "move_files" {
        let binding = call("move_binding", args.clone()).map_err(invalid)?;
        for field in ["source", "destination"] {
            let path = binding[field]
                .as_str()
                .ok_or_else(|| invalid("private native folder cannot be resolved".into()))?;
            workspace.resolve(path, PathOperation::WriteExisting)?;
        }
        let paths: Vec<ca_automation::private_files::NativePath> =
            serde_json::from_value(binding["paths"].clone())
                .map_err(|_| invalid("private move membership is unavailable".into()))?;
        for path in paths {
            workspace.authorize_private_move_path(&path.path())?;
        }
        // Prepared arguments can enter an automatic approval review. Native
        // byte paths, fingerprints and journal bindings remain host-only.
        prepared["_ca_private_batch"] = json!({"operation":binding["binding"]["operation"],"source":binding["source"],"destination":binding["destination"],"count":binding["count"]});
    }
    Ok(prepared)
}

/// Ordinary names such as file_report.txt are not private references.
pub(crate) fn is_resource_reference(value: &str) -> bool {
    let id = value.split('/').next().unwrap_or_default();
    ["folder_", "file_"].iter().any(|prefix| {
        id.strip_prefix(*prefix)
            .is_some_and(|tail| tail.len() == 32 && tail.bytes().all(|b| b.is_ascii_hexdigit()))
    })
}

/// Bridge discovered filename directories to the existing private batch engine.
/// Capabilities and native paths stay host-owned; ordinary file tools continue
/// to require their canonical filename references.
pub(super) fn filename_batch_arguments(
    context: &Context,
    name: &str,
    args: &Value,
) -> Result<Value, String> {
    let files = context.private_files()?;
    let mut resolved = args.clone();
    let fields: &[&str] = if name == "select_files" {
        &["folder", "destination"]
    } else {
        &["destination"]
    };
    for field in fields {
        let Some(reference) = args.get(*field).and_then(Value::as_str) else {
            continue;
        };
        let resource = if is_resource_reference(reference) {
            files
                .folder(reference)
                .map_err(|_| "Private batch folder reference is unavailable")?
        } else {
            let path = {
                let state = context.state.lock().unwrap_or_else(|p| p.into_inner());
                state
                    .filenames
                    .as_ref()
                    .ok_or("Filename mappings are unavailable")?
                    .resolve_path(reference)?
            };
            let path = path
                .canonicalize()
                .map_err(|_| "Private batch folder is unavailable")?;
            if !context.filename_path_allowed(&path) {
                return Err("Private batch folder needs a current folder grant".into());
            }
            (if path.is_file() && *field == "folder" {
                files.bind_file(&path)
            } else {
                files.bind_folder(&path)
            })
            .map_err(|_| "Private batch source binding is unavailable")?
        };
        let path = resource
            .path
            .path()
            .canonicalize()
            .map_err(|_| "Private batch folder is unavailable")?;
        if !context.filename_path_allowed(&path) {
            return Err("Private batch folder needs a current folder grant".into());
        }
        resolved[*field] = Value::String(resource.reference);
    }
    Ok(resolved)
}

/// Only schema-declared resource fields are resolved. A reference appearing in
/// source text, an explanation or an arbitrary shell string remains opaque.
pub(crate) fn resolve_typed_arguments(
    name: &str,
    args: &Value,
    context: Option<&Arc<Context>>,
) -> Result<Value, String> {
    let registry = crate::agent::registry::ToolRegistry::builtins();
    let Some(spec) = registry.get(name) else {
        return Ok(args.clone());
    };
    let mut resolved = args.clone();
    for resource in &spec.resources {
        if !matches!(
            resource.kind,
            crate::agent::registry::ResourceKind::Path
                | crate::agent::registry::ResourceKind::WorkingDirectory
        ) {
            continue;
        }
        if let Some(value) = resolved
            .get(resource.field)
            .and_then(Value::as_str)
            .filter(|v| is_resource_reference(v))
        {
            let args = json!({"reference":value});
            let result = if let Some(context) = context {
                call_for(context, "folder", args)
            } else {
                call("folder", args)
            }?;
            resolved[resource.field] = result["path"].clone();
        }
    }
    Ok(resolved)
}

pub(crate) fn move_authority_paths(args: &Value) -> Result<Vec<PathBuf>, String> {
    let value = call(
        "move_binding",
        json!({"selection":args["selection"],"destination":args["destination"]}),
    )?;
    let paths: Vec<ca_automation::private_files::NativePath> =
        serde_json::from_value(value["paths"].clone())
            .map_err(|_| "private permission membership is unavailable")?;
    Ok(paths.into_iter().map(|p| p.path()).collect())
}

#[cfg(test)]
pub(crate) async fn project_remote_wire(body: Vec<u8>) -> Result<Vec<u8>, String> {
    let Some(binding) = REMOTE.get().cloned() else {
        return Ok(body);
    };
    tokio::task::spawn_blocking(move || {
        let value: Value =
            serde_json::from_slice(&body).map_err(|_| "private provider request is invalid")?;
        serde_json::to_vec(&rpc(&binding, "wire", value)?)
            .map_err(|_| "private provider request could not be encoded".into())
    })
    .await
    .map_err(|_| "private request broker stopped")?
}
pub(crate) fn project_remote_wire_blocking(body: &[u8]) -> Result<Vec<u8>, String> {
    let Some(binding) = REMOTE.get() else {
        return Ok(body.to_vec());
    };
    let value: Value =
        serde_json::from_slice(body).map_err(|_| "private provider request is invalid")?;
    serde_json::to_vec(&rpc(binding, "wire", value)?)
        .map_err(|_| "private provider request could not be encoded".into())
}
pub(crate) async fn validate_remote_wire(body: &[u8]) -> Result<(), String> {
    let Some(binding) = REMOTE.get().cloned() else {
        return Ok(());
    };
    let body = body.to_vec();
    tokio::task::spawn_blocking(move || {
        let value: Value =
            serde_json::from_slice(&body).map_err(|_| "private provider request is invalid")?;
        rpc(&binding, "inspect", value).map(|_| ())
    })
    .await
    .map_err(|_| "private replay inspection stopped")?
}
fn mask_json(context: &Context, value: &mut Value) -> Result<(), String> {
    match value {
        Value::String(s) => *s = context.mask_known(s)?,
        Value::Array(v) => {
            for item in v {
                mask_json(context, item)?;
            }
        }
        Value::Object(v) => {
            let old = std::mem::take(v);
            for (key, mut item) in old {
                mask_json(context, &mut item)?;
                let key = context.mask_known(&key)?;
                if v.insert(key, item).is_some() {
                    return Err("private object keys collide after projection".into());
                }
            }
        }
        _ => {}
    }
    Ok(())
}

impl Context {
    pub(super) fn discover_filename_grant_requests(
        &self,
        request: &crate::request::PreparedModelRequest,
    ) -> Result<(), String> {
        if !self.filename_mode {
            return Ok(());
        }
        for message in request
            .messages
            .iter()
            .filter(|message| message.role == "user")
        {
            let Some(text) = message.content.as_str() else {
                continue;
            };
            for reference in path_references(text) {
                let path = if let Some(relative) = reference.value.strip_prefix("~/") {
                    crate::runtime::RuntimePaths::discover()
                        .map_err(|_| "Local home is unavailable")?
                        .home
                        .join(relative)
                } else {
                    PathBuf::from(&reference.value)
                };
                if !path.is_absolute()
                    || !path.exists()
                    || path.starts_with(&self.workspace)
                    || path.starts_with(self.private_metadata_root())
                {
                    continue;
                }
                let root = if path.is_dir() {
                    path.as_path()
                } else {
                    path.parent().ok_or("Requested file has no parent")?
                };
                self.state
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .filenames
                    .as_mut()
                    .ok_or("Filename mappings are unavailable")?
                    .register_grant_request(root)?;
            }
        }
        Ok(())
    }
    pub(crate) fn private_files(&self) -> Result<PrivateFiles, String> {
        let mut slot = self.task_context.lock().unwrap_or_else(|p| p.into_inner());
        let id = slot.as_ref().map_or(self.session_id, |r| r.context_id);
        let root = self.scope.join("privacy/tasks/v1").join(id.to_string());
        let marker = json!({"schema":"ca.private-task-context.v1","context_id":id});
        let marker_hash = super::digest(
            &serde_json::to_vec(&marker)
                .map_err(|_| "private context identity cannot be encoded")?,
        );
        if let Some(saved) = slot.as_ref() {
            saved.validate()?;
            let actual:Value=ca_automation::runtime::read(&root.join("context.json"),4096).map_err(|_|"Saved private task context is missing; restore its local journal before continuing")?;
            if actual != marker || saved.marker_sha256 != marker_hash {
                return Err("private task context identity changed".into());
            }
        }
        let files = PrivateFiles::open(&root).map_err(|_| "private task storage is unavailable")?;
        if slot.is_none() {
            if root.join("context.json").exists() {
                let old: Value = ca_automation::runtime::read(&root.join("context.json"), 4096)
                    .map_err(|_| "private task identity is unavailable")?;
                if old != marker {
                    return Err("private task context identity changed".into());
                }
            } else {
                ca_automation::runtime::atomic_write(&root.join("context.json"), &marker)
                    .map_err(|_| "private task identity could not be saved")?;
            }
            *slot = Some(super::store::PrivateTaskContextRefV1 {
                schema: "ca.private-task-context.v1".into(),
                context_id: id,
                marker_sha256: marker_hash,
            });
        }
        Ok(files)
    }
    pub(crate) fn private_metadata_root(&self) -> PathBuf {
        self.scope.join("privacy")
    }
    pub(super) fn bind_task_paths(
        &self,
        request: &mut crate::request::PreparedModelRequest,
    ) -> Result<(), String> {
        if !self.selected_only {
            return Ok(());
        }
        use ca_automation::private_files::{AuthoredBindingsV1, AuthoredPathBindingV1};
        use ca_automation_protocol::private_files::PrivateResourceCatalogV1;
        let files = self.private_files()?;
        let mut messages = Vec::new();
        for (ordinal, mut message) in std::mem::take(&mut request.messages)
            .into_iter()
            .enumerate()
        {
            if message.role != "user" {
                messages.push(message);
                continue;
            }
            let authored = message.content.authored_text()?;
            let hash = super::digest(authored.as_bytes());
            let key = message
                .content
                .authored_id()
                .map(str::to_owned)
                .unwrap_or_else(|| super::digest(format!("{ordinal}:{authored}").as_bytes()));
            let saved = files
                .authored_bindings(&key)
                .map_err(|_| "private authored binding is unavailable")?;
            let bindings = if let Some(saved) = saved {
                if hash != saved.authored_sha256 && hash != saved.projected_sha256 {
                    return Err(
                        "Private message identity changed; submit the updated task again".into(),
                    );
                }
                saved
            } else {
                let mut paths = Vec::new();
                let mut resources = Vec::new();
                for reference in path_references(&authored) {
                    let expanded = if let Some(tail) = reference.value.strip_prefix("~/") {
                        crate::runtime::RuntimePaths::discover()
                            .map_err(|_| "local home folder is unavailable")?
                            .home
                            .join(tail)
                    } else {
                        let p = PathBuf::from(&reference.value);
                        if p.is_absolute() {
                            p
                        } else {
                            self.workspace.join(p)
                        }
                    };
                    let metadata = match std::fs::symlink_metadata(&expanded) {
                        Ok(metadata) => metadata,
                        Err(_) if !reference.explicit => continue,
                        Err(_) => return Err(format!("Local path reference is unavailable: {}. Choose an existing resource before continuing.", reference.value)),
                    };
                    // Canonicalize the parent, preserving a symlink leaf as a link.
                    let path = if metadata.is_dir() {
                        expanded.canonicalize()
                    } else {
                        expanded
                            .parent()
                            .and_then(|parent| parent.canonicalize().ok())
                            .zip(expanded.file_name())
                            .map(|(parent, name)| parent.join(name))
                            .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))
                    }
                    .map_err(|_| "local path reference cannot be resolved")?;
                    if path.starts_with(self.private_metadata_root())
                        || files.is_managed_path(&path)
                    {
                        return Err(
                            "Host-owned privacy metadata is not a file-task resource".into()
                        );
                    }
                    let bound = if metadata.is_dir() {
                        files.bind_folder(&path)
                    } else {
                        files.bind_file(&path)
                    }
                    .map_err(|_| "local resource binding is stale or unsupported")?;
                    if !resources.iter().any(
                        |r: &ca_automation_protocol::private_files::PrivateResourceSummaryV1| {
                            r.reference == bound.reference
                        },
                    ) {
                        resources.push(
                            files
                                .summarize(&bound.reference, &[self.private_metadata_root()])
                                .map_err(|_| "private resource inventory is unavailable")?,
                        );
                    }
                    paths.push(AuthoredPathBindingV1 {
                        spelling: reference.spelling,
                        reference: bound.reference,
                    });
                }
                if paths.is_empty() {
                    messages.push(message);
                    continue;
                }
                let lower = authored.to_ascii_lowercase();
                let file_task = ["move", "organize", "relocate"].iter().any(|word| {
                    lower
                        .split(|c: char| !c.is_alphabetic())
                        .any(|token| token == *word)
                }) && !["function", "method", "class"].iter().any(|word| {
                    lower
                        .split(|c: char| !c.is_alphabetic())
                        .any(|token| token == *word)
                });
                let projected = replace_references(&authored, &paths);
                files
                    .save_authored_bindings(&AuthoredBindingsV1 {
                        schema: "ca.authored-private-paths.v1".into(),
                        message_key: key,
                        authored_sha256: hash.clone(),
                        projected_sha256: super::digest(projected.as_bytes()),
                        paths,
                        catalog: PrivateResourceCatalogV1 {
                            schema: "ca.private-resource-catalog.v1".into(),
                            resources,
                        },
                        file_task,
                    })
                    .map_err(|_| "private authored binding could not be saved")?
            };
            message.content = message
                .content
                .project_authored(bindings.file_task && !bindings.paths.is_empty(), |text| {
                    replace_references(text, &bindings.paths)
                })?;
            messages.push(message);
            if !bindings.paths.is_empty() {
                files
                    .publish_control(
                        &serde_json::to_value(&bindings.catalog)
                            .map_err(|_| "private catalog could not be encoded")?,
                    )
                    .map_err(|_| "private catalog receipt could not be saved")?;
                messages.push(crate::agent::client::ChatMessage {
                    role: "system".into(),
                    content: crate::image_content::ModelContent::text(
                        serde_json::to_string(&bindings.catalog)
                            .map_err(|_| "private catalog could not be encoded")?,
                    ),
                    tool_calls: None,
                    tool_call_id: None,
                    response_items: Vec::new(),
                });
            }
        }
        request.messages = messages;
        self.refresh_task_names()?;
        Ok(())
    }
    fn refresh_task_names(&self) -> Result<(), String> {
        if self.filename_mode {
            return Ok(());
        }
        use base64::Engine;
        let files = self.private_files()?;
        let folders = files
            .folders()
            .map_err(|_| "private folder registry is unavailable")?;
        if !folders.is_empty() {
            let roots = std::iter::once(self.workspace.clone())
                .chain(folders.iter().map(|f| f.path.path()))
                .collect::<Vec<_>>();
            for path in &roots {
                for ancestor in path.ancestors().filter(|p| p.parent().is_some()) {
                    if let Some(value) = ancestor.to_str() {
                        self.register_task_value(value, None)?;
                        self.register_task_value(
                            &base64::engine::general_purpose::STANDARD.encode(value),
                            None,
                        )?;
                        self.register_task_value(
                            &percent_encoding::percent_encode(
                                value.as_bytes(),
                                percent_encoding::NON_ALPHANUMERIC,
                            )
                            .to_string(),
                            None,
                        )?;
                    }
                }
            }
        }
        for folder in folders {
            let root = folder.path.path();
            if let Some(s) = root.to_str() {
                self.register_task_value(s, Some(&folder.reference))?;
            }
            // Enumeration and local filtering run on the blocking request worker.
            // Nothing is read through a symlink or sent as an inventory of names.
            for entry in walkdir::WalkDir::new(&root)
                .follow_links(false)
                .into_iter()
                .filter_entry(|e| {
                    !files.is_managed_path(e.path())
                        && !e.path().starts_with(self.private_metadata_root())
                })
            {
                let entry = entry.map_err(|_| "private task enumeration failed")?;
                let path = entry.path();
                let spellings = [
                    path.to_str(),
                    path.strip_prefix(&root).ok().and_then(Path::to_str),
                    entry.file_name().to_str(),
                ];
                for spelling in spellings.into_iter().flatten().filter(|s| !s.is_empty()) {
                    self.register_task_value(
                        spelling,
                        if root.to_str() == Some(spelling) {
                            Some(&folder.reference)
                        } else {
                            None
                        },
                    )?;
                    let bytes = spelling.as_bytes();
                    self.register_task_value(
                        &base64::engine::general_purpose::STANDARD.encode(bytes),
                        None,
                    )?;
                    let hex = bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
                    self.register_task_value(&hex, None)?;
                    let percent =
                        percent_encoding::percent_encode(bytes, percent_encoding::NON_ALPHANUMERIC)
                            .to_string();
                    if percent != spelling {
                        self.register_task_value(&percent, None)?;
                    }
                }
                // Non-UTF-8 paths stay local. Mask deterministic serialized native
                // byte spellings as well, without ever exposing a lossy pathname.
                if path.to_str().is_none() {
                    let bytes = path.as_os_str().as_encoded_bytes();
                    self.register_task_value(
                        &base64::engine::general_purpose::STANDARD.encode(bytes),
                        None,
                    )?;
                    self.register_task_value(
                        &bytes.iter().map(|b| format!("{b:02x}")).collect::<String>(),
                        None,
                    )?;
                }
            }
        }
        Ok(())
    }
    fn register_task_value(&self, value: &str, replacement: Option<&str>) -> Result<(), String> {
        if value.is_empty() {
            return Ok(());
        }
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let token = match replacement {
            Some(token) => token.into(),
            None => state.registry.alias("task_path", value)?,
        };
        let names = if replacement.is_some() {
            &mut state.active
        } else {
            &mut state.task_names
        };
        if names.get(value) != Some(&token) {
            names.insert(value.into(), token);
            state.matcher_dirty = true;
            self.private_revision.fetch_add(1, Ordering::AcqRel);
        }
        Ok(())
    }

    pub(crate) fn project_task_wire(&self, body: &[u8]) -> Result<Vec<u8>, String> {
        if !self.has_task_names() {
            return Ok(body.to_vec());
        }
        let mut value: Value =
            serde_json::from_slice(body).map_err(|_| "private provider envelope is invalid")?;
        self.mask_task_wire_value(&mut value)?;
        serde_json::to_vec(&value).map_err(|_| "private provider envelope cannot be encoded".into())
    }
    pub(crate) fn has_task_names(&self) -> bool {
        !self
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .task_names
            .is_empty()
    }
    fn mask_task_wire_value(&self, value: &mut Value) -> Result<(), String> {
        let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let replacements = state
            .task_names
            .iter()
            .map(|(s, t)| (s.clone(), t.clone()))
            .collect::<Vec<_>>();
        drop(state);
        if replacements.is_empty() {
            return Ok(());
        }
        let originals = replacements
            .iter()
            .map(|(s, _)| s.as_str())
            .collect::<Vec<_>>();
        let matcher = aho_corasick::AhoCorasickBuilder::new()
            .match_kind(aho_corasick::MatchKind::LeftmostLongest)
            .build(&originals)
            .map_err(|_| "private task names cannot be indexed")?;
        fn mask(
            text: &str,
            matcher: &aho_corasick::AhoCorasick,
            replacements: &[(String, String)],
        ) -> String {
            let mut out = String::new();
            let mut offset = 0;
            for found in matcher.find_iter(text) {
                let (original, replacement) = &replacements[found.pattern().as_usize()];
                if super::is_word(original)
                    && (!super::boundary(text, found.start())
                        || !super::boundary(text, found.end()))
                {
                    continue;
                }
                // Public syntax/type words can coincide with a native filename.
                // Preserve their structural/prose role; a complete data value,
                // quoted name, listing row or path component still stays private.
                let lower = original.to_ascii_lowercase();
                let keyword = matches!(
                    lower.as_str(),
                    "fn" | "def"
                        | "if"
                        | "else"
                        | "for"
                        | "while"
                        | "return"
                        | "let"
                        | "const"
                        | "true"
                        | "false"
                        | "null"
                        | "none"
                        | "class"
                        | "struct"
                        | "enum"
                        | "impl"
                        | "use"
                        | "import"
                        | "from"
                        | "pub"
                        | "async"
                        | "await"
                        | "match"
                        | "in"
                        | "and"
                        | "or"
                        | "not"
                );
                let label = matches!(
                    lower.as_str(),
                    "pdf"
                        | "pdfs"
                        | "txt"
                        | "text"
                        | "json"
                        | "csv"
                        | "jpg"
                        | "jpeg"
                        | "png"
                        | "zip"
                        | "source"
                        | "destination"
                        | "folder"
                        | "path"
                        | "count"
                        | "total"
                        | "moved"
                        | "skipped"
                        | "errors"
                        | "remaining"
                        | "status"
                        | "type"
                );
                if keyword || label {
                    let before = &text[..found.start()];
                    let after = &text[found.end()..];
                    let line_start = before.rfind('\n').map_or(0, |p| p + 1);
                    let line_end = after.find('\n').map_or(text.len(), |p| found.end() + p);
                    let named = text[line_start..line_end].trim() == original
                        || before.ends_with(['\'', '"', '/', '\\'])
                        || after.starts_with(['\'', '"', '/', '\\'])
                        || after.starts_with(':')
                        || (label && before.trim_end().ends_with(':'));
                    if !named {
                        continue;
                    }
                }
                // Host tokens are already public references, never mask inside one.
                if text[..found.start()]
                    .rfind(super::store::TOKEN_PREFIX)
                    .is_some_and(|start| {
                        let tail = start + super::store::TOKEN_PREFIX.len();
                        text[tail..]
                            .find(super::store::TOKEN_SUFFIX)
                            .is_some_and(|end| {
                                found.start() < tail + end + super::store::TOKEN_SUFFIX.len()
                            })
                    })
                {
                    continue;
                }
                out.push_str(&text[offset..found.start()]);
                out.push_str(replacement);
                offset = found.end();
            }
            out.push_str(&text[offset..]);
            out
        }
        fn visit(
            value: &mut Value,
            context: &Context,
            matcher: &aho_corasick::AhoCorasick,
            replacements: &[(String, String)],
            data: bool,
            arguments: bool,
        ) -> Result<(), String> {
            match value {
                Value::String(text) => {
                    // Tool arguments are JSON inside a protocol string. Mask their
                    // data fields, preserving their typed interface and syntax.
                    if let Ok(mut nested) = serde_json::from_str::<Value>(text) {
                        if nested.is_object() || nested.is_array() {
                            if public_file_control(context, &nested) {
                                return Ok(());
                            }
                            let before = nested.to_string();
                            visit(
                                &mut nested,
                                context,
                                matcher,
                                replacements,
                                !arguments,
                                false,
                            )?;
                            let after = nested.to_string();
                            if before != after {
                                *text = after;
                            }
                            return Ok(());
                        }
                    }
                    *text = mask(text, matcher, replacements);
                }
                Value::Array(values) => {
                    for v in values {
                        visit(v, context, matcher, replacements, data, false)?;
                    }
                }
                Value::Object(values) => {
                    let old = std::mem::take(values);
                    for (key, mut v) in old {
                        if !data
                            && key == "extensions"
                            && v.as_array().is_some_and(|a| {
                                a.iter().all(|v| {
                                    v.as_str().is_some_and(|s| {
                                        matches!(
                                            s.to_ascii_lowercase().as_str(),
                                            "pdf"
                                                | "txt"
                                                | "md"
                                                | "json"
                                                | "csv"
                                                | "jpg"
                                                | "jpeg"
                                                | "png"
                                                | "zip"
                                        )
                                    })
                                })
                            })
                        {
                            values.insert(key, v);
                            continue;
                        }
                        if !data
                            && matches!(
                                key.as_str(),
                                "model"
                                    | "role"
                                    | "type"
                                    | "name"
                                    | "id"
                                    | "call_id"
                                    | "tool_call_id"
                                    | "previous_response_id"
                                    | "parameters"
                                    | "input_schema"
                                    | "required"
                                    | "enum"
                            )
                        {
                            values.insert(key, v);
                            continue;
                        }
                        let arguments = key == "arguments";
                        visit(&mut v, context, matcher, replacements, data, arguments)?;
                        let key = if data {
                            mask(&key, matcher, replacements)
                        } else {
                            key
                        };
                        if values.insert(key, v).is_some() {
                            return Err("private data keys collide after projection".into());
                        }
                    }
                }
                _ => {}
            }
            Ok(())
        }
        visit(value, self, &matcher, &replacements, false, false)
    }
}

/// Host provenance is required even for a syntactically valid anonymous DTO.
fn public_file_control(context: &Context, value: &Value) -> bool {
    context
        .private_files()
        .is_ok_and(|files| files.is_public_control(value))
}

struct PathReference {
    value: String,
    spelling: String,
    explicit: bool,
    start: usize,
    end: usize,
}
fn path_references(text: &str) -> Vec<PathReference> {
    static PATHS: OnceLock<regex::Regex> = OnceLock::new();
    let regex = PATHS.get_or_init(|| {
        regex::Regex::new(r#""((?:\\.|[^"\\])*)"|'([^']*)'|(?:^|[\s(])@([^\s"'<>]+)"#)
            .expect("path reference grammar")
    });
    regex
        .captures_iter(text)
        .filter_map(|c| {
            let (raw, at) = if let Some(v) = c.get(1) {
                (v, false)
            } else if let Some(v) = c.get(2) {
                (v, false)
            } else {
                (c.get(3)?, true)
            };
            let value = if c.get(1).is_some() {
                serde_json::from_str::<String>(&format!("\"{}\"", raw.as_str()))
                    .unwrap_or_else(|_| raw.as_str().into())
            } else {
                raw.as_str().into()
            };
            if value.is_empty() {
                return None;
            }
            let explicit = at
                || ["/", "./", "../", "~/"]
                    .iter()
                    .any(|prefix| value.starts_with(prefix));
            Some(PathReference {
                value,
                spelling: raw.as_str().into(),
                explicit,
                start: raw.start(),
                end: raw.end(),
            })
        })
        .collect()
}
fn replace_references(
    text: &str,
    paths: &[ca_automation::private_files::AuthoredPathBindingV1],
) -> String {
    let mut output = String::new();
    let mut offset = 0;
    for reference in path_references(text) {
        if let Some(binding) = paths.iter().find(|p| p.spelling == reference.spelling) {
            output.push_str(&text[offset..reference.start]);
            output.push_str(&binding.reference);
            offset = reference.end;
        }
    }
    output.push_str(&text[offset..]);
    output
}

fn execute_local(
    context: &Arc<Context>,
    args: &MoveFilesV1,
    token: Option<&str>,
) -> Result<Value, String> {
    let files = context.private_files()?;
    let binding = files
        .bind_move(&args.selection, &args.destination)
        .map_err(|_| "private move binding is stale")?;
    let root = files.root().to_path_buf();
    let execution = execution_context(context, &binding)?;
    let selection = args.selection.clone();
    let callback = move |p: &FileMoveProgressV1| report(&selection, p);
    // Both direct calls and IPC handlers are owned blocking workers.
    let receipt = ca_automation::native::AutomationEngine::execute_private_move(
        &execution,
        root,
        binding,
        ca_automation::native::FileMoveHooks {
            progress: Some(Arc::new(callback)),
            cancelled: token.map(cancel_flag).transpose()?,
        },
    )
    .map_err(|_| "private batch needs local reconciliation")?;
    let response = receipt
        .result
        .ok_or("private batch parked; inspect its local journal")?;
    files
        .publish_control(&response)
        .map_err(|_| "private batch receipt could not be saved; retry the same batch")?;
    Ok(response)
}
fn execution_context(
    context: &Context,
    binding: &MoveBinding,
) -> Result<ca_automation::runtime::ExecutionContext, String> {
    let digest = super::digest(
        &serde_json::to_vec(binding).map_err(|_| "private move identity cannot be encoded")?,
    );
    let root = context.private_files()?.root().to_path_buf();
    Ok(ca_automation::runtime::ExecutionContext {
        schema: ca_automation::runtime::EXECUTION_SCHEMA.into(),
        run_id: binding.operation.clone(),
        profile: "private-task".into(),
        state_root: root.join("engine"),
        lease_root: root.join("leases"),
        definition_digest: digest.clone(),
        resource_binding_digest: digest,
        policy_digest:super::digest(b"ca.private-move-policy.v1: preserve-relative-subfolders; skip-collisions; no-follow-links; verified-cross-filesystem-copy"),
        resources: vec![format!(
            "private-files:{}",
            context
                .task_context
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .as_ref()
                .expect("admitted private context")
                .context_id
        )],
        runtime_seconds: 86400,
        output_bytes: 64 * 1024,
    })
}
