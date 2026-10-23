use crate::agent::stream::{ProviderEvent, ProviderResponseItem, UsageInfo};
use crate::request::{PreparedModelRequest, RequestKind};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use uuid::Uuid;

pub const TRACE_SCHEMA: &str = "ca.provider-trace.v1";
pub const MAX_TRACE_BYTES: u64 = 16 * 1024 * 1024;
/// Retain only the newest traces. Without a bound, every `--trace` run leaves a
/// multi-megabyte frame dump behind until someone remembers `/trace delete`.
pub const MAX_RETAINED_TRACES: usize = 4;
const WRITER_CHANNEL_CAPACITY: usize = 256;
const MAX_REQUEST_BODY_BYTES: usize = 512 * 1024;
const MAX_FRAME_BYTES: usize = 64 * 1024;
const MAX_EVENT_TEXT_BYTES: usize = 64 * 1024;
const MAX_OPEN_BYTES: usize = 512 * 1024;
const MAX_JSON_DEPTH: usize = 32;
const MAX_JSON_NODES: usize = 4_096;
const MAX_JSON_ITEMS: usize = 256;

#[derive(Debug, thiserror::Error)]
pub enum TraceError {
    #[error("provider tracing is unavailable with --profile bare")]
    BareProfile,
    #[error("trace storage path is a symlink")]
    Symlink,
    #[error("trace does not exist")]
    NotFound,
    #[error("trace storage is unavailable: {0}")]
    Storage(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceStatus {
    pub enabled: bool,
    pub trace_id: Option<Uuid>,
    pub started_at: Option<DateTime<Utc>>,
    pub bytes: u64,
    pub last_error: Option<String>,
    pub persistence_allowed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceSummary {
    pub id: Uuid,
    pub bytes: u64,
    pub modified_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceStart {
    Started(Uuid),
    AlreadyEnabled(Uuid),
}

#[derive(Clone)]
pub struct ProviderTraceHub {
    inner: Arc<HubInner>,
}

impl std::fmt::Debug for ProviderTraceHub {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let status = self.status();
        formatter
            .debug_struct("ProviderTraceHub")
            .field("enabled", &status.enabled)
            .field("trace_id", &status.trace_id)
            .field("persistence_allowed", &status.persistence_allowed)
            .finish_non_exhaustive()
    }
}

struct HubInner {
    directory: PathBuf,
    persistence_allowed: bool,
    max_trace_bytes: u64,
    state: Mutex<HubState>,
}

#[derive(Default)]
struct HubState {
    active: Option<ActiveTrace>,
    retired: Vec<Arc<WriterHealth>>,
    last_error: Option<String>,
    session_id: Option<Uuid>,
    configured_secrets: Vec<String>,
}

struct ActiveTrace {
    id: Uuid,
    started_at: DateTime<Utc>,
    next_sequence: u64,
    sender: std::sync::mpsc::SyncSender<WriterMessage>,
    health: Arc<WriterHealth>,
}

enum WriterMessage {
    Line(Vec<u8>),
    Stop(Option<std::sync::mpsc::SyncSender<()>>),
}

#[derive(Default)]
struct WriterHealth {
    accepting: AtomicBool,
    bytes: std::sync::atomic::AtomicU64,
    error: Mutex<Option<String>>,
    diagnostic_taken: AtomicBool,
}

impl WriterHealth {
    fn fail(&self, message: impl Into<String>) {
        self.accepting.store(false, Ordering::Release);
        let mut error = self
            .error
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if error.is_none() {
            *error = Some(crate::request::safe_text(&message.into(), &[]));
        }
    }

    fn error(&self) -> Option<String> {
        self.error
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .clone()
    }
}

#[derive(Clone)]
pub struct ProviderTraceRequest {
    hub: ProviderTraceHub,
    trace_id: Uuid,
    request_id: Uuid,
    kind: RequestKind,
    session_id: Option<Uuid>,
    started: Instant,
    secrets: Arc<Vec<String>>,
}

#[derive(Debug, Default)]
pub struct RawTraceFrameBuffer {
    bytes: Vec<u8>,
}

#[derive(Serialize)]
struct TraceEnvelope<'a> {
    schema: &'static str,
    sequence: u64,
    timestamp: DateTime<Utc>,
    trace_id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    session_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    request_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    request_kind: Option<&'a str>,
    event: &'a str,
    data: Value,
}

static PROCESS_HUB: OnceLock<Mutex<Option<ProviderTraceHub>>> = OnceLock::new();

fn process_slot() -> &'static Mutex<Option<ProviderTraceHub>> {
    PROCESS_HUB.get_or_init(|| Mutex::new(None))
}

pub fn install_process_hub(hub: ProviderTraceHub) {
    *process_slot()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner()) = Some(hub);
}

pub fn process_hub() -> Option<ProviderTraceHub> {
    process_slot()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .clone()
}

impl ProviderTraceHub {
    pub fn new(directory: PathBuf, persistence_allowed: bool) -> Self {
        Self::with_limit(directory, persistence_allowed, MAX_TRACE_BYTES)
    }

    fn with_limit(directory: PathBuf, persistence_allowed: bool, max_trace_bytes: u64) -> Self {
        Self {
            inner: Arc::new(HubInner {
                directory,
                persistence_allowed,
                max_trace_bytes,
                state: Mutex::new(HubState::default()),
            }),
        }
    }

    pub fn directory(&self) -> &Path {
        &self.inner.directory
    }

    pub fn set_session_id(&self, session_id: Option<Uuid>) {
        self.inner
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .session_id = session_id;
    }

    pub fn add_configured_secrets<'a>(&self, secrets: impl IntoIterator<Item = &'a str>) {
        let mut state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        for secret in secrets {
            if !secret.is_empty()
                && !state
                    .configured_secrets
                    .iter()
                    .any(|existing| existing == secret)
            {
                state.configured_secrets.push(secret.to_string());
            }
        }
    }

    pub fn start(&self) -> Result<TraceStart, TraceError> {
        if !self.inner.persistence_allowed {
            return Err(TraceError::BareProfile);
        }
        let mut state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if let Some(active) = state
            .active
            .as_ref()
            .filter(|active| active.health.accepting.load(Ordering::Acquire))
        {
            return Ok(TraceStart::AlreadyEnabled(active.id));
        }
        if let Some(previous) = state.active.take() {
            state.retired.push(previous.health);
        }
        state.retired.retain(|health| {
            health.accepting.load(Ordering::Acquire)
                || (health.error().is_some() && !health.diagnostic_taken.load(Ordering::Acquire))
        });
        prepare_trace_directory(&self.inner.directory)?;
        let id = Uuid::new_v4();
        let path = self.inner.directory.join(format!("{id}.jsonl"));
        let file = create_trace_file(&path)?;
        let started_at = Utc::now();
        let health = Arc::new(WriterHealth::default());
        health.accepting.store(true, Ordering::Release);
        let (sender, receiver) =
            std::sync::mpsc::sync_channel::<WriterMessage>(WRITER_CHANNEL_CAPACITY);
        let trace_id = id;
        let max_bytes = self.inner.max_trace_bytes;
        let writer_health = health.clone();
        std::thread::Builder::new()
            .name(format!("ca-trace-{}", &id.to_string()[..8]))
            .spawn(move || writer_loop(file, receiver, writer_health, max_bytes))
            .map_err(|error| TraceError::Storage(safe_io_error(&error)))?;

        state.last_error = None;
        state.active = Some(ActiveTrace {
            id,
            started_at,
            next_sequence: 0,
            sender,
            health,
        });
        drop(state);
        // Trim older frame dumps once the new trace is in place, so the newest
        // (this one) is always within the retained window.
        self.prune_saved_traces(Some(id));
        self.record(
            None,
            None,
            "trace_started",
            serde_json::json!({
                "version": 1,
                "trace_id": trace_id,
                "retention": "newest_only",
                "max_bytes": self.inner.max_trace_bytes,
                "max_traces": MAX_RETAINED_TRACES,
            }),
        );
        Ok(TraceStart::Started(id))
    }

    /// Delete saved traces beyond the retention window, never touching `keep`.
    /// Pruning is best-effort: a busy or unreadable leftover is skipped rather
    /// than blocking a trace the user just asked for.
    fn prune_saved_traces(&self, keep: Option<Uuid>) {
        if !self.inner.persistence_allowed || !self.inner.directory.exists() {
            return;
        }
        let Ok(traces) = self.list() else {
            return;
        };
        for trace in traces.into_iter().skip(MAX_RETAINED_TRACES) {
            if Some(trace.id) == keep {
                continue;
            }
            let _ = self.delete(trace.id);
        }
    }

    pub fn stop(&self) -> Option<Uuid> {
        let mut state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let active = state.active.take()?;
        state.last_error = active.health.error();
        let _ = active.sender.try_send(WriterMessage::Stop(None));
        state.retired.push(active.health);
        Some(active.id)
    }

    pub fn stop_and_flush(&self, timeout: Duration) -> Option<Uuid> {
        let mut state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let active = state.active.take();
        let id = active.as_ref().map(|active| active.id);
        let mut health = state.retired.clone();
        let mut acknowledgement = None;
        if let Some(active) = active {
            state.last_error = active.health.error();
            health.push(active.health.clone());
            state.retired.push(active.health.clone());
            let (ack_tx, ack_rx) = std::sync::mpsc::sync_channel(1);
            if active
                .sender
                .try_send(WriterMessage::Stop(Some(ack_tx)))
                .is_ok()
            {
                acknowledgement = Some(ack_rx);
            }
        }
        drop(state);
        let deadline = Instant::now() + timeout;
        if let Some(acknowledgement) = acknowledgement {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let _ = acknowledgement.recv_timeout(remaining);
        }
        for trace in health {
            while trace.accepting.load(Ordering::Acquire) && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(2));
            }
        }
        id
    }

    pub fn status(&self) -> TraceStatus {
        let state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let active = state.active.as_ref();
        let accepting = active.is_some_and(|trace| trace.health.accepting.load(Ordering::Acquire));
        TraceStatus {
            enabled: accepting,
            trace_id: active.map(|trace| trace.id),
            started_at: active.map(|trace| trace.started_at),
            bytes: active.map_or(0, |trace| trace.health.bytes.load(Ordering::Acquire)),
            last_error: active
                .and_then(|trace| trace.health.error())
                .or_else(|| state.last_error.clone())
                .or_else(|| state.retired.iter().rev().find_map(|trace| trace.error())),
            persistence_allowed: self.inner.persistence_allowed,
        }
    }

    pub fn take_diagnostic(&self) -> Option<String> {
        let state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        for health in state
            .active
            .as_ref()
            .map(|active| &active.health)
            .into_iter()
            .chain(state.retired.iter())
        {
            let Some(error) = health.error() else {
                continue;
            };
            if !health.diagnostic_taken.swap(true, Ordering::AcqRel) {
                return Some(format!("Provider trace stopped: {error}"));
            }
        }
        None
    }

    pub fn begin_request_with_session(
        &self,
        request: &PreparedModelRequest,
        protocol: &str,
        secrets: &[&str],
        session_id: Option<Uuid>,
    ) -> Option<ProviderTraceRequest> {
        let state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let trace_id = state
            .active
            .as_ref()
            .filter(|active| active.health.accepting.load(Ordering::Acquire))?
            .id;
        let session_id = session_id.or(state.session_id);
        let mut all_secrets = state.configured_secrets.clone();
        drop(state);
        for secret in secrets.iter().copied().filter(|secret| !secret.is_empty()) {
            if !all_secrets.iter().any(|existing| existing == secret) {
                all_secrets.push(secret.to_string());
            }
        }
        let secret_refs = all_secrets.iter().map(String::as_str).collect::<Vec<_>>();
        let start_data = serde_json::json!({
            "provider": bounded_safe_text(&request.provider, &secret_refs, 256),
            "model": bounded_safe_text(&request.model, &secret_refs, 256),
            "protocol": protocol,
        });
        let trace = ProviderTraceRequest {
            hub: self.clone(),
            trace_id,
            request_id: Uuid::new_v4(),
            kind: request.kind,
            session_id,
            started: Instant::now(),
            secrets: Arc::new(all_secrets),
        };
        trace.hub.record_with_session(
            Some(trace.trace_id),
            Some(trace.session_id),
            Some(trace.request_id),
            Some(trace.kind),
            "request_started",
            start_data,
        );
        Some(trace)
    }

    pub fn list(&self) -> Result<Vec<TraceSummary>, TraceError> {
        if !self.inner.persistence_allowed {
            return Err(TraceError::BareProfile);
        }
        if !self.inner.directory.exists() {
            return Ok(Vec::new());
        }
        ensure_not_symlink(&self.inner.directory)?;
        let mut traces = Vec::new();
        let entries = std::fs::read_dir(&self.inner.directory)
            .map_err(|error| TraceError::Storage(safe_io_error(&error)))?;
        for entry in entries {
            let entry = entry.map_err(|error| TraceError::Storage(safe_io_error(&error)))?;
            let file_type = entry
                .file_type()
                .map_err(|error| TraceError::Storage(safe_io_error(&error)))?;
            if file_type.is_symlink() || !file_type.is_file() {
                continue;
            }
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let Some(stem) = name.strip_suffix(".jsonl") else {
                continue;
            };
            let Ok(id) = Uuid::parse_str(stem) else {
                continue;
            };
            let metadata = entry
                .metadata()
                .map_err(|error| TraceError::Storage(safe_io_error(&error)))?;
            let modified_at = metadata.modified().ok().map(DateTime::<Utc>::from);
            traces.push(TraceSummary {
                id,
                bytes: metadata.len(),
                modified_at,
            });
        }
        traces.sort_by(|left, right| {
            right
                .modified_at
                .cmp(&left.modified_at)
                .then_with(|| right.id.cmp(&left.id))
        });
        Ok(traces)
    }

    pub fn open_text(&self, id: Uuid) -> Result<String, TraceError> {
        let path = self.validated_trace_file(id)?;
        let file = open_trace_file(&path)?;
        let mut reader = BufReader::new(file);
        let mut output = format!("Provider trace {id}\n\n");
        let mut retained = 0usize;
        let mut line = Vec::new();
        loop {
            line.clear();
            let read = reader
                .read_until(b'\n', &mut line)
                .map_err(|error| TraceError::Storage(safe_io_error(&error)))?;
            if read == 0 {
                break;
            }
            if !line.ends_with(b"\n") {
                output.push_str("[incomplete trailing record ignored]\n");
                break;
            }
            if retained.saturating_add(line.len()) > MAX_OPEN_BYTES {
                output.push_str("[… trace display truncated at 512 KiB; file remains unchanged]\n");
                break;
            }
            let text = String::from_utf8_lossy(&line);
            output.push_str(&crate::terminal_text::sanitize_terminal_text(&text));
            retained += line.len();
        }
        Ok(output)
    }

    pub fn prepare_delete(&self, id: Uuid) -> Result<TraceSummary, TraceError> {
        let path = self.validated_trace_file(id)?;
        let metadata = std::fs::metadata(path).map_err(map_trace_io_error)?;
        Ok(TraceSummary {
            id,
            bytes: metadata.len(),
            modified_at: metadata.modified().ok().map(DateTime::<Utc>::from),
        })
    }

    pub fn delete(&self, id: Uuid) -> Result<(), TraceError> {
        let status = self.status();
        if status.trace_id == Some(id) {
            return Err(TraceError::Storage(
                "disable the active trace before deleting it".into(),
            ));
        }
        let path = self.validated_trace_file(id)?;
        std::fs::remove_file(path).map_err(map_trace_io_error)
    }

    pub fn render_status(&self) -> String {
        let status = self.status();
        let mut lines = vec!["Provider exchange tracing".to_string(), String::new()];
        if !status.persistence_allowed {
            lines.push("Status: unavailable in bare profile".into());
        } else if status.enabled {
            lines.push("Status: on".into());
        } else {
            lines.push("Status: off".into());
        }
        if let Some(id) = status.trace_id {
            lines.push(format!("Trace: {id}"));
        }
        if let Some(started) = status.started_at {
            lines.push(format!("Started: {}", started.to_rfc3339()));
        }
        lines.push(format!("Captured: {}", format_bytes(status.bytes)));
        lines.push(format!(
            "Per-trace cap: {}",
            format_bytes(self.inner.max_trace_bytes)
        ));
        lines.push(format!(
            "Retention: newest {MAX_RETAINED_TRACES} (pruned on /trace on)"
        ));
        lines.push(format!(
            "Directory: {}",
            crate::terminal_text::sanitize_terminal_text(
                &self.inner.directory.display().to_string()
            )
        ));
        if let Some(error) = status.last_error {
            lines.push(format!("Last error: {error}"));
        }
        lines.push(String::new());
        lines.push("/trace on · /trace off · /trace list".into());
        lines.push("/trace open <id> · /trace delete <id>".into());
        lines.join("\n")
    }

    pub fn render_list(&self) -> Result<String, TraceError> {
        let traces = self.list()?;
        let mut lines = vec!["Provider traces".to_string(), String::new()];
        if traces.is_empty() {
            lines.push("No saved traces".into());
        } else {
            for trace in traces {
                let modified = trace
                    .modified_at
                    .map_or_else(|| "unknown time".into(), |value| value.to_rfc3339());
                lines.push(format!(
                    "{}  {:>10}  {}",
                    trace.id,
                    format_bytes(trace.bytes),
                    modified
                ));
            }
        }
        lines.push(String::new());
        lines.push("Open with /trace open <id>".into());
        lines.push("Delete with /trace delete <id> (confirmation required)".into());
        Ok(lines.join("\n"))
    }

    fn validated_trace_file(&self, id: Uuid) -> Result<PathBuf, TraceError> {
        if !self.inner.persistence_allowed {
            return Err(TraceError::BareProfile);
        }
        ensure_not_symlink(&self.inner.directory)?;
        let path = self.inner.directory.join(format!("{id}.jsonl"));
        let metadata = std::fs::symlink_metadata(&path).map_err(map_trace_io_error)?;
        if metadata.file_type().is_symlink() {
            return Err(TraceError::Symlink);
        }
        if !metadata.file_type().is_file() {
            return Err(TraceError::NotFound);
        }
        Ok(path)
    }

    fn record(
        &self,
        request_id: Option<Uuid>,
        kind: Option<RequestKind>,
        event: &str,
        data: Value,
    ) {
        self.record_with_session(None, None, request_id, kind, event, data);
    }

    fn record_with_session(
        &self,
        expected_trace_id: Option<Uuid>,
        session_override: Option<Option<Uuid>>,
        request_id: Option<Uuid>,
        kind: Option<RequestKind>,
        event: &str,
        data: Value,
    ) {
        let mut state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let session_id = session_override.unwrap_or(state.session_id);
        let Some(active) = state.active.as_mut() else {
            return;
        };
        if expected_trace_id.is_some_and(|expected| expected != active.id) {
            return;
        }
        if !active.health.accepting.load(Ordering::Acquire) {
            return;
        }
        let sequence = active.next_sequence;
        active.next_sequence = active.next_sequence.saturating_add(1);
        let envelope = TraceEnvelope {
            schema: TRACE_SCHEMA,
            sequence,
            timestamp: Utc::now(),
            trace_id: active.id,
            session_id,
            request_id,
            request_kind: kind.map(RequestKind::trace_label),
            event,
            data,
        };
        let mut line = match serde_json::to_vec(&envelope) {
            Ok(line) => line,
            Err(_) => {
                active.health.fail("trace serialization failed");
                return;
            }
        };
        line.push(b'\n');
        if active.sender.try_send(WriterMessage::Line(line)).is_err() {
            active
                .health
                .fail("bounded trace writer could not accept more records");
        }
    }
}

impl ProviderTraceRequest {
    pub fn attempt_started(
        &self,
        attempt: usize,
        method: &str,
        url: &str,
        headers: &reqwest::header::HeaderMap,
        body: &[u8],
    ) {
        let secrets = self.secret_refs();
        let Some(url) = sanitize_url(url, &secrets) else {
            self.disable("trace URL redaction failed");
            return;
        };
        let Some(body) = sanitize_json_bytes(body, &secrets, MAX_REQUEST_BODY_BYTES) else {
            self.disable("trace request-body redaction failed");
            return;
        };
        let data = serde_json::json!({
            "attempt": attempt,
            "elapsed_ms": self.started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
            "method": method,
            "url": url,
            "headers": sanitize_headers(headers, &secrets),
            "body": body,
        });
        self.emit("attempt_started", data);
    }

    pub fn attempt_response(
        &self,
        attempt: usize,
        status: u16,
        headers: &reqwest::header::HeaderMap,
        retry: bool,
        retry_delay: Option<Duration>,
        elapsed: Duration,
    ) {
        let secrets = self.secret_refs();
        self.emit(
            "attempt_response",
            serde_json::json!({
                "attempt": attempt,
                "elapsed_ms": elapsed.as_millis().min(u128::from(u64::MAX)) as u64,
                "status": status,
                "retry": retry,
                "retry_delay_ms": retry_delay.map(|value| value.as_millis().min(u128::from(u64::MAX)) as u64),
                "headers": sanitize_headers(headers, &secrets),
            }),
        );
    }

    pub fn attempt_error(
        &self,
        attempt: usize,
        error: &str,
        retry: bool,
        retry_delay: Option<Duration>,
        elapsed: Duration,
    ) {
        let secrets = self.secret_refs();
        self.emit(
            "attempt_error",
            serde_json::json!({
                "attempt": attempt,
                "elapsed_ms": elapsed.as_millis().min(u128::from(u64::MAX)) as u64,
                "error": bounded_safe_text(error, &secrets, MAX_EVENT_TEXT_BYTES),
                "retry": retry,
                "retry_delay_ms": retry_delay.map(|value| value.as_millis().min(u128::from(u64::MAX)) as u64),
            }),
        );
    }

    pub fn raw_frame(&self, attempt: usize, bytes: &[u8]) {
        let secrets = self.secret_refs();
        let Some(frame) = sanitize_raw_frame(bytes, &secrets) else {
            self.disable("trace raw-frame redaction was uncertain");
            return;
        };
        self.emit(
            "raw_stream_frame",
            serde_json::json!({
                "attempt": attempt,
                "frame": frame,
            }),
        );
    }

    pub fn decoded_event(&self, attempt: usize, event: &ProviderEvent) {
        let secrets = self.secret_refs();
        self.emit(
            "decoded_provider_event",
            serde_json::json!({
                "attempt": attempt,
                "provider_event": sanitize_provider_event(event, &secrets),
            }),
        );
    }

    pub(crate) fn diagnostics(&self, diagnostics: &[String]) {
        if !diagnostics.is_empty() {
            self.emit("adapter_diagnostics", serde_json::json!({"diagnostics": diagnostics.iter().take(32).map(|value| bounded_safe_text(value, &self.secret_refs(), 256)).collect::<Vec<_>>() }));
        }
    }

    pub fn outcome(&self, attempt: usize, outcome: &str, error: Option<&str>) {
        let secrets = self.secret_refs();
        self.emit(
            "request_finished",
            serde_json::json!({
                "attempt": attempt,
                "elapsed_ms": self.started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
                "outcome": bounded_safe_text(outcome, &secrets, 128),
                "error": error.map(|value| bounded_safe_text(value, &secrets, MAX_EVENT_TEXT_BYTES)),
            }),
        );
    }

    fn emit(&self, event: &str, data: Value) {
        self.hub.record_with_session(
            Some(self.trace_id),
            Some(self.session_id),
            Some(self.request_id),
            Some(self.kind),
            event,
            data,
        );
    }

    fn disable(&self, message: &str) {
        let state = self
            .hub
            .inner
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if let Some(active) = &state.active {
            if active.id == self.trace_id {
                active.health.fail(message);
            }
        }
    }

    fn secret_refs(&self) -> Vec<&str> {
        self.secrets.iter().map(String::as_str).collect()
    }
}

impl RawTraceFrameBuffer {
    const LIMIT: usize = 512 * 1024;

    pub fn push(&mut self, chunk: &[u8]) -> Result<Vec<Vec<u8>>, ()> {
        if self.bytes.len().saturating_add(chunk.len()) > Self::LIMIT {
            self.bytes.clear();
            return Err(());
        }
        self.bytes.extend_from_slice(chunk);
        let mut frames = Vec::new();
        loop {
            let lf = self
                .bytes
                .windows(2)
                .position(|window| window == b"\n\n")
                .map(|index| (index, 2));
            let crlf = self
                .bytes
                .windows(4)
                .position(|window| window == b"\r\n\r\n")
                .map(|index| (index, 4));
            let Some((index, delimiter)) = (match (lf, crlf) {
                (Some(left), Some(right)) => Some(if left.0 <= right.0 { left } else { right }),
                (Some(found), None) | (None, Some(found)) => Some(found),
                (None, None) => None,
            }) else {
                break;
            };
            frames.push(self.bytes.drain(..index + delimiter).collect());
        }
        Ok(frames)
    }

    pub fn finish(&mut self) -> Option<Vec<u8>> {
        (!self.bytes.is_empty()).then(|| std::mem::take(&mut self.bytes))
    }
}

fn writer_loop(
    file: File,
    receiver: std::sync::mpsc::Receiver<WriterMessage>,
    health: Arc<WriterHealth>,
    max_bytes: u64,
) {
    let mut writer = BufWriter::new(file);
    while let Ok(message) = receiver.recv() {
        match message {
            WriterMessage::Stop(acknowledge) => {
                let _ = writer.flush();
                if let Some(acknowledge) = acknowledge {
                    let _ = acknowledge.try_send(());
                }
                break;
            }
            WriterMessage::Line(line) => {
                let current = health.bytes.load(Ordering::Acquire);
                if current.saturating_add(line.len() as u64) > max_bytes {
                    health.fail(format!("trace reached its {} cap", format_bytes(max_bytes)));
                    break;
                }
                if let Err(error) = writer.write_all(&line).and_then(|()| writer.flush()) {
                    health.fail(format!("trace write failed: {}", safe_io_error(&error)));
                    break;
                }
                health
                    .bytes
                    .store(current + line.len() as u64, Ordering::Release);
            }
        }
    }
    let _ = writer.flush();
    health.accepting.store(false, Ordering::Release);
}

fn prepare_trace_directory(directory: &Path) -> Result<(), TraceError> {
    reject_existing_symlink_components(directory)?;
    if directory.exists() {
        ensure_not_symlink(directory)?;
        let metadata = std::fs::metadata(directory)
            .map_err(|error| TraceError::Storage(safe_io_error(&error)))?;
        if !metadata.is_dir() {
            return Err(TraceError::Storage(
                "trace storage path is not a directory".into(),
            ));
        }
    } else {
        std::fs::create_dir_all(directory)
            .map_err(|error| TraceError::Storage(safe_io_error(&error)))?;
        reject_existing_symlink_components(directory)?;
    }
    set_owner_only_directory(directory)?;
    Ok(())
}

fn reject_existing_symlink_components(path: &Path) -> Result<(), TraceError> {
    let mut candidate = Some(path);
    while let Some(component) = candidate {
        match std::fs::symlink_metadata(component) {
            Ok(metadata) if metadata.file_type().is_symlink() => return Err(TraceError::Symlink),
            Ok(_) => return Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                candidate = component.parent();
            }
            Err(error) => return Err(TraceError::Storage(safe_io_error(&error))),
        }
    }
    Ok(())
}

fn ensure_not_symlink(path: &Path) -> Result<(), TraceError> {
    let metadata = std::fs::symlink_metadata(path).map_err(map_trace_io_error)?;
    if metadata.file_type().is_symlink() {
        Err(TraceError::Symlink)
    } else {
        Ok(())
    }
}

fn create_trace_file(path: &Path) -> Result<File, TraceError> {
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path).map_err(map_trace_io_error)?;
    set_owner_only_file(path)?;
    Ok(file)
}

fn open_trace_file(path: &Path) -> Result<File, TraceError> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let file = options.open(path).map_err(map_trace_io_error)?;
    if !file
        .metadata()
        .map_err(map_trace_io_error)?
        .file_type()
        .is_file()
    {
        return Err(TraceError::NotFound);
    }
    Ok(file)
}

#[cfg(unix)]
fn set_owner_only_directory(path: &Path) -> Result<(), TraceError> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
        .map_err(|error| TraceError::Storage(safe_io_error(&error)))
}

#[cfg(not(unix))]
fn set_owner_only_directory(_path: &Path) -> Result<(), TraceError> {
    Ok(())
}

#[cfg(unix)]
fn set_owner_only_file(path: &Path) -> Result<(), TraceError> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .map_err(|error| TraceError::Storage(safe_io_error(&error)))
}

#[cfg(not(unix))]
fn set_owner_only_file(_path: &Path) -> Result<(), TraceError> {
    Ok(())
}

fn map_trace_io_error(error: std::io::Error) -> TraceError {
    if error.kind() == std::io::ErrorKind::NotFound {
        TraceError::NotFound
    } else {
        TraceError::Storage(safe_io_error(&error))
    }
}

fn safe_io_error(error: &std::io::Error) -> String {
    format!("I/O error ({:?})", error.kind())
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

fn bounded_safe_text(text: &str, secrets: &[&str], limit: usize) -> String {
    let safe = crate::request::safe_text(text, secrets);
    if safe.len() <= limit {
        return safe;
    }
    let mut end = limit.min(safe.len());
    while end > 0 && !safe.is_char_boundary(end) {
        end -= 1;
    }
    format!(
        "{}[…{} bytes omitted]",
        &safe[..end],
        safe.len().saturating_sub(end)
    )
}

fn sanitize_url(raw: &str, secrets: &[&str]) -> Option<String> {
    let mut url = reqwest::Url::parse(raw).ok()?;
    if !url.username().is_empty() {
        url.set_username("").ok()?;
    }
    if url.password().is_some() {
        url.set_password(None).ok()?;
    }
    let query = url
        .query_pairs()
        .map(|(key, _)| key.into_owned())
        .collect::<Vec<_>>();
    if !query.is_empty() {
        url.set_query(None);
        let mut pairs = url.query_pairs_mut();
        for key in query {
            pairs.append_pair(&bounded_safe_text(&key, secrets, 256), "[REDACTED]");
        }
    }
    url.set_fragment(None);
    Some(bounded_safe_text(url.as_str(), secrets, 4 * 1024))
}

fn sanitize_headers(
    headers: &reqwest::header::HeaderMap,
    secrets: &[&str],
) -> BTreeMap<String, Vec<String>> {
    let mut result = BTreeMap::<String, Vec<String>>::new();
    for (name, value) in headers {
        let name = name.as_str().to_ascii_lowercase();
        let safe = if sensitive_header(&name) {
            "[REDACTED]".into()
        } else if name == "x-request-id" || name == "x-oai-request-id" {
            let mut hasher = Sha256::new();
            hasher.update(value.as_bytes());
            format!("[sha256:{:x}]", hasher.finalize())
        } else {
            value.to_str().map_or_else(
                |_| "[non-UTF-8 header redacted]".into(),
                |text| bounded_safe_text(text, secrets, 4 * 1024),
            )
        };
        result.entry(name).or_default().push(safe);
    }
    result
}

fn sensitive_header(name: &str) -> bool {
    let normalized = name.replace(['-', '_'], "");
    normalized.contains("authorization")
        || normalized.contains("cookie")
        || normalized.contains("apikey")
        || normalized.contains("secret")
        || normalized.contains("password")
        || normalized.contains("credential")
        || normalized.contains("token")
        || normalized.contains("accesstoken")
        || normalized.contains("refreshtoken")
        || normalized.contains("accountid")
        || normalized.contains("organization")
        || normalized.contains("projectid")
        || normalized.contains("tenantid")
        || normalized.contains("userid")
        || normalized.contains("clientrequestid")
}

fn sanitize_json_bytes(bytes: &[u8], secrets: &[&str], limit: usize) -> Option<Value> {
    let value = serde_json::from_slice::<Value>(bytes).ok()?;
    let mut nodes = MAX_JSON_NODES;
    let safe = sanitize_value(&value, secrets, 0, &mut nodes);
    let encoded = serde_json::to_vec(&safe).ok()?;
    if encoded.len() <= limit {
        Some(safe)
    } else {
        Some(serde_json::json!({
            "marker": "request body omitted by trace bound",
            "bytes": bytes.len(),
            "sanitized_sha256": format!("{:x}", Sha256::digest(&encoded)),
        }))
    }
}

fn sanitize_value(value: &Value, secrets: &[&str], depth: usize, nodes: &mut usize) -> Value {
    if depth >= MAX_JSON_DEPTH || *nodes == 0 {
        return Value::String("[content omitted by trace bound]".into());
    }
    *nodes -= 1;
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) => value.clone(),
        Value::String(text) => Value::String(sanitize_provider_text(text, secrets)),
        Value::Array(values) => {
            let retained = values.len().min(MAX_JSON_ITEMS);
            let mut safe = values
                .iter()
                .take(retained)
                .map(|value| sanitize_value(value, secrets, depth + 1, nodes))
                .collect::<Vec<_>>();
            if retained < values.len() {
                safe.push(Value::String(format!(
                    "[…{} items omitted]",
                    values.len() - retained
                )));
            }
            Value::Array(safe)
        }
        Value::Object(values) => {
            let mut safe = Map::new();
            let opaque_block = values
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|kind| matches!(kind, "redacted_thinking" | "thinking"));
            let media_object = values
                .get("media_type")
                .and_then(Value::as_str)
                .is_some_and(|value| value.starts_with("image/") || value.starts_with("audio/"))
                || values
                    .get("type")
                    .and_then(Value::as_str)
                    .is_some_and(|value| matches!(value, "base64" | "image" | "input_audio"));
            for (key, value) in values.iter().take(MAX_JSON_ITEMS) {
                let value = if (opaque_block && matches!(key.as_str(), "data" | "signature"))
                    || sensitive_json_field(key)
                    || (media_object && matches!(key.as_str(), "data" | "payload" | "url"))
                {
                    Value::String(redaction_marker(key).into())
                } else {
                    sanitize_value(value, secrets, depth + 1, nodes)
                };
                safe.insert(bounded_safe_text(key, secrets, 256), value);
            }
            if values.len() > MAX_JSON_ITEMS {
                safe.insert(
                    "[ca omitted fields]".into(),
                    Value::Number((values.len() - MAX_JSON_ITEMS).into()),
                );
            }
            Value::Object(safe)
        }
    }
}

pub(crate) fn native_diagnostic_body(body: &[u8], secrets: &[&str]) -> Vec<u8> {
    sanitize_json_bytes(body, secrets, MAX_REQUEST_BODY_BYTES)
        .and_then(|value| serde_json::to_vec(&value).ok())
        .unwrap_or_else(|| b"[native provider body omitted by diagnostic bounds]".to_vec())
}

fn sensitive_json_field(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase().replace(['-', '_', ' '], "");
    normalized.contains("authorization")
        || normalized.contains("cookie")
        || normalized.contains("apikey")
        || normalized.contains("secret")
        || normalized.contains("password")
        || normalized.contains("credential")
        || normalized.contains("accesstoken")
        || normalized.contains("refreshtoken")
        || normalized.contains("accountid")
        || normalized == "promptcachekey"
        || matches!(
            normalized.as_str(),
            "user"
                | "userid"
                | "organization"
                | "organizationid"
                | "project"
                | "projectid"
                | "tenant"
                | "tenantid"
        )
        || normalized.contains("encrypted")
        || normalized == "reasoningdetails"
        || normalized == "signature"
        || normalized == "thoughtsignature"
        || normalized == "inlinedata"
        || normalized == "imageurl"
        || normalized == "inputaudio"
        || normalized == "audio"
        || normalized == "filedata"
}

fn redaction_marker(key: &str) -> &'static str {
    let normalized = key.to_ascii_lowercase().replace(['-', '_', ' '], "");
    if normalized.contains("encrypted") || normalized == "reasoningdetails" {
        "[encrypted reasoning payload redacted]"
    } else if normalized == "imageurl"
        || normalized == "inputaudio"
        || normalized == "audio"
        || normalized == "filedata"
    {
        "[binary/media payload redacted]"
    } else {
        "[REDACTED]"
    }
}

fn sanitize_raw_frame(bytes: &[u8], secrets: &[&str]) -> Option<Value> {
    let retained = bytes.len().min(MAX_FRAME_BYTES);
    let slice = &bytes[..retained];
    let omitted = bytes.len().saturating_sub(retained);
    let Ok(text) = std::str::from_utf8(slice) else {
        return Some(serde_json::json!({
            "marker": "binary provider frame redacted",
            "bytes": bytes.len(),
            "sha256": format!("{:x}", Sha256::digest(bytes)),
        }));
    };
    let mut rendered = Vec::<Value>::new();
    for line in text.lines() {
        let trimmed = line.trim();
        let payload = trimmed
            .strip_prefix("data:")
            .map(str::trim)
            .unwrap_or(trimmed);
        if payload == "[DONE]" {
            rendered.push(Value::String("data: [DONE]".into()));
        } else if let Ok(value) = serde_json::from_str::<Value>(payload) {
            let mut nodes = MAX_JSON_NODES;
            rendered.push(sanitize_value(&value, secrets, 0, &mut nodes));
        } else if trimmed.starts_with("data:") || contains_sensitive_json_shape(line) {
            return None;
        } else {
            rendered.push(Value::String(bounded_safe_text(
                line,
                secrets,
                MAX_EVENT_TEXT_BYTES,
            )));
        }
    }
    Some(serde_json::json!({
        "lines": rendered,
        "bytes": bytes.len(),
        "omitted_bytes": omitted,
    }))
}

fn contains_sensitive_json_shape(text: &str) -> bool {
    let normalized = text.to_ascii_lowercase().replace(['-', '_', ' ', '\t'], "");
    [
        "\"authorization\":",
        "\"apikey\":",
        "\"accountid\":",
        "\"accesstoken\":",
        "\"refreshtoken\":",
        "\"cookie\":",
        "\"secret\":",
        "\"password\":",
        "\"credential\":",
        "\"encryptedcontent\":",
        "\"reasoningdetails\":",
    ]
    .iter()
    .any(|needle| normalized.contains(needle))
}

fn sanitize_provider_text(text: &str, secrets: &[&str]) -> String {
    if contains_sensitive_json_shape(text) {
        "[decoded provider text redacted: secret-shaped JSON]".into()
    } else {
        bounded_safe_text(text, secrets, MAX_EVENT_TEXT_BYTES)
    }
}

fn sanitize_provider_event(event: &ProviderEvent, secrets: &[&str]) -> Value {
    match event {
        ProviderEvent::Text(text) => serde_json::json!({
            "type": "text",
            "text": sanitize_provider_text(text, secrets),
        }),
        ProviderEvent::Reasoning(text) => serde_json::json!({
            "type": "reasoning",
            "text": sanitize_provider_text(text, secrets),
        }),
        ProviderEvent::ResponseItem(item) => match item {
            ProviderResponseItem::ResponsesOutput { output_index, .. } => serde_json::json!({
                "type": "response_item",
                "output_index": output_index,
                "item": "[encrypted/opaque provider continuation redacted]",
            }),
            ProviderResponseItem::NativeContent { .. }
            | ProviderResponseItem::ChatReasoning { .. } => serde_json::json!({
                "type": "reasoning_continuation",
                "content": "[encrypted/opaque provider continuation redacted]",
            }),
        },
        ProviderEvent::ToolCallStart { index, id, name } => serde_json::json!({
            "type": "tool_call_start",
            "index": index,
            "id": bounded_safe_text(id, secrets, 256),
            "name": bounded_safe_text(name, secrets, 256),
        }),
        ProviderEvent::ToolCallArg { index, arg } => {
            let value = serde_json::from_str::<Value>(arg).map_or_else(
                |_| Value::String(sanitize_provider_text(arg, secrets)),
                |value| {
                    let mut nodes = MAX_JSON_NODES;
                    sanitize_value(&value, secrets, 0, &mut nodes)
                },
            );
            serde_json::json!({ "type": "tool_call_arg", "index": index, "arg": value })
        }
        ProviderEvent::Finish { reason, usage } => serde_json::json!({
            "type": "finish",
            "reason": bounded_safe_text(&reason.to_string(), secrets, 128),
            "usage": usage.as_ref().map(|usage| sanitize_usage(usage, secrets)),
        }),
        ProviderEvent::Usage(usage) => {
            serde_json::json!({ "type": "usage", "usage": sanitize_usage(usage, secrets) })
        }
        ProviderEvent::Done => serde_json::json!({ "type": "done" }),
    }
}

fn sanitize_usage(usage: &UsageInfo, secrets: &[&str]) -> Value {
    serde_json::json!({
        "prompt_tokens": usage.prompt_tokens,
        "completion_tokens": usage.completion_tokens,
        "total_tokens": usage.total_tokens,
        "cached_prompt_tokens": usage.cached_prompt_tokens,
        "cache_write_tokens": usage.cache_write_tokens,
        "reasoning_tokens": usage.reasoning_tokens,
        "response_metadata": usage.response_metadata.as_ref().map(|metadata| serde_json::json!({
            "request_id_sha256": metadata.request_id_sha256(),
            "model": bounded_safe_text(metadata.model(), secrets, 256),
            "service_tier": metadata.service_tier().map(|tier| bounded_safe_text(tier, secrets, 128)),
        })),
    })
}

impl RequestKind {
    fn trace_label(self) -> &'static str {
        match self {
            Self::MainTurn => "main_turn",
            Self::ToolContinuation => "tool_continuation",
            Self::SideQuestion => "side_question",
            Self::SessionTitle => "session_title",
            Self::ContextCheckpoint => "context_checkpoint",
            Self::BranchHandoff => "branch_handoff",
            Self::ProjectInit => "project_init",
            Self::Review => "review",
            Self::Subagent => "subagent",
            Self::Headless => "headless",
            Self::HeadlessCheck => "headless_check",
            Self::Scripted => "scripted",
            Self::ProviderProbe => "provider_probe",
            Self::GoalEvaluation => "goal_evaluation",
            Self::WorkflowStep => "workflow_step",
            Self::MultiRunCandidate => "multi_run_candidate",
            Self::MultiRunJudge => "multi_run_judge",
            Self::MultiRunFusion => "multi_run_fusion",
            Self::ChangeWalkthrough => "change_walkthrough",
            Self::Service => "service",
            Self::Scheduled => "scheduled",
            Self::RoleRun => "role_run",
            Self::EmailGateway => "email_gateway",
            Self::Dream => "dream",
            Self::Distill => "distill",
            Self::Analyze => "analyze",
            Self::Compatibility => "compatibility",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait_for_writer(hub: &ProviderTraceHub) {
        for _ in 0..100 {
            if hub.status().bytes > 0 || hub.status().last_error.is_some() {
                return;
            }
            std::thread::yield_now();
        }
    }

    #[test]
    fn bare_profile_refuses_persistence() {
        let temp = tempfile::tempdir().unwrap();
        let hub = ProviderTraceHub::new(temp.path().join("traces"), false);
        assert!(matches!(hub.start(), Err(TraceError::BareProfile)));
        assert!(!temp.path().join("traces").exists());
    }

    #[test]
    fn creates_owner_only_trace_and_lists_opens_and_deletes_exact_id() {
        let temp = tempfile::tempdir().unwrap();
        let hub = ProviderTraceHub::new(temp.path().join("traces"), true);
        let TraceStart::Started(id) = hub.start().unwrap() else {
            panic!("first activation must create a trace");
        };
        wait_for_writer(&hub);
        hub.stop_and_flush(Duration::from_secs(1));
        let list = hub.list().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, id);
        assert!(hub.open_text(id).unwrap().contains(TRACE_SCHEMA));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(hub.directory())
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
            assert_eq!(
                std::fs::metadata(hub.directory().join(format!("{id}.jsonl")))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        hub.delete(id).unwrap();
        assert!(hub.list().unwrap().is_empty());
    }

    #[test]
    fn repeated_activations_keep_prior_traces_within_the_retention_window() {
        let temp = tempfile::tempdir().unwrap();
        let hub = ProviderTraceHub::new(temp.path().join("traces"), true);
        let TraceStart::Started(first) = hub.start().unwrap() else {
            panic!("first activation must create a trace");
        };
        hub.stop_and_flush(Duration::from_secs(1));
        let TraceStart::Started(second) = hub.start().unwrap() else {
            panic!("second activation must create a new trace");
        };
        hub.stop_and_flush(Duration::from_secs(1));
        assert_ne!(first, second);
        let ids = hub
            .list()
            .unwrap()
            .into_iter()
            .map(|trace| trace.id)
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(ids, std::collections::HashSet::from([first, second]));
    }

    #[test]
    fn starting_a_new_trace_prunes_beyond_the_retention_window() {
        let temp = tempfile::tempdir().unwrap();
        let hub = ProviderTraceHub::new(temp.path().join("traces"), true);
        let mut ids = Vec::new();
        for index in 0..(MAX_RETAINED_TRACES + 3) {
            let TraceStart::Started(id) = hub.start().unwrap() else {
                panic!("each activation must create a trace");
            };
            ids.push(id);
            hub.stop_and_flush(Duration::from_secs(1));
            // Pin mtimes so newest-first ordering is deterministic even on
            // filesystems with coarse timestamp granularity.
            let path = hub.directory().join(format!("{id}.jsonl"));
            let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
            let when = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000 + index as u64);
            file.set_modified(when).unwrap();
        }
        let kept = hub.list().unwrap();
        assert_eq!(kept.len(), MAX_RETAINED_TRACES);
        // The newest traces survive; the earliest ones are gone.
        for id in &ids[..3] {
            assert!(
                !kept.iter().any(|trace| trace.id == *id),
                "oldest trace {id} should have been pruned"
            );
        }
        for id in &ids[3..] {
            assert!(
                kept.iter().any(|trace| trace.id == *id),
                "recent trace {id} should be retained"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn refuses_symlink_directory_and_trace_entries() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("target");
        std::fs::create_dir(&target).unwrap();
        let linked = temp.path().join("traces");
        symlink(&target, &linked).unwrap();
        let hub = ProviderTraceHub::new(linked, true);
        assert!(matches!(hub.start(), Err(TraceError::Symlink)));

        let real = temp.path().join("real");
        let hub = ProviderTraceHub::new(real.clone(), true);
        std::fs::create_dir(&real).unwrap();
        let id = Uuid::new_v4();
        let victim = temp.path().join("victim");
        std::fs::write(&victim, b"keep").unwrap();
        symlink(&victim, real.join(format!("{id}.jsonl"))).unwrap();
        assert!(matches!(hub.open_text(id), Err(TraceError::Symlink)));
        assert!(matches!(hub.delete(id), Err(TraceError::Symlink)));
        assert_eq!(std::fs::read(&victim).unwrap(), b"keep");
    }

    #[test]
    fn open_ignores_a_crash_truncated_final_record() {
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("traces");
        std::fs::create_dir(&directory).unwrap();
        let id = Uuid::new_v4();
        std::fs::write(
            directory.join(format!("{id}.jsonl")),
            b"{\"complete\":true}\n{\"partial\":",
        )
        .unwrap();
        let hub = ProviderTraceHub::new(directory, true);
        let text = hub.open_text(id).unwrap();
        assert!(text.contains("complete"));
        assert!(!text.contains("partial"));
        assert!(text.contains("incomplete trailing record ignored"));
    }

    #[test]
    fn sanitizes_url_headers_json_binary_controls_and_opaque_reasoning() {
        let url = sanitize_url(
            "https://user:pass@example.test/v1?q=secret&two=2#frag",
            &["secret"],
        )
        .unwrap();
        assert_eq!(
            url,
            "https://example.test/v1?q=%5BREDACTED%5D&two=%5BREDACTED%5D"
        );
        let body = br#"{"api_key":"trace-secret","account_id":"acct","prompt_cache_key":"private-cache-key","input":[{"image_url":"data:image/png;base64,AAAA"}],"text":"ok\u001b[31m"}"#;
        let safe = sanitize_json_bytes(body, &["trace-secret"], MAX_REQUEST_BODY_BYTES).unwrap();
        let rendered = serde_json::to_string(&safe).unwrap();
        assert!(!rendered.contains("trace-secret"));
        assert!(!rendered.contains("acct"));
        assert!(!rendered.contains("AAAA"));
        assert!(!rendered.contains("\\u001b"));
        assert!(rendered.contains("binary/media payload redacted"));
        assert!(!rendered.contains("private-cache-key"));

        let pasted = br#"{"input":"pasted {\"password\":\"unknown-secret\"}"}"#;
        let rendered = sanitize_json_bytes(pasted, &[], MAX_REQUEST_BODY_BYTES)
            .unwrap()
            .to_string();
        assert!(!rendered.contains("unknown-secret"));
        assert!(rendered.contains("secret-shaped JSON"));

        let item = ProviderEvent::ResponseItem(ProviderResponseItem::ResponsesOutput {
            source: crate::agent::stream::ProviderStateSourceV1::for_test(
                "openai",
                crate::agent::provider::ProviderProtocol::OpenAiResponses,
            ),
            output_index: 0,
            item: serde_json::json!({"encrypted_content":"do-not-store"}),
        });
        let rendered = sanitize_provider_event(&item, &[]).to_string();
        assert!(!rendered.contains("do-not-store"));
        assert!(rendered.contains("redacted"));

        let text = ProviderEvent::Text(r#"{"password":"unknown-secret"}"#.into());
        let rendered = sanitize_provider_event(&text, &[]).to_string();
        assert!(!rendered.contains("unknown-secret"));
        assert!(rendered.contains("secret-shaped JSON"));

        let argument = ProviderEvent::ToolCallArg {
            index: 0,
            arg: r#"{"api-key":"partial-secret"#.into(),
        };
        let rendered = sanitize_provider_event(&argument, &[]).to_string();
        assert!(!rendered.contains("partial-secret"));
        assert!(rendered.contains("secret-shaped JSON"));
    }

    #[test]
    fn split_raw_frames_are_buffered_before_secret_shaped_json_redaction() {
        let mut frames = RawTraceFrameBuffer::default();
        assert!(frames
            .push(b"data: {\"account_id\":\"must-")
            .unwrap()
            .is_empty());
        let frames = frames.push(b"not-leak\",\"text\":\"ok\"}\n\n").unwrap();
        assert_eq!(frames.len(), 1);
        let safe = sanitize_raw_frame(&frames[0], &[]).unwrap().to_string();
        assert!(!safe.contains("must-not-leak"));
        assert!(safe.contains("REDACTED"));
        assert!(sanitize_raw_frame(b"data: {\"account_id\":\"partial", &[]).is_none());
    }

    #[test]
    fn every_production_request_kind_has_a_stable_trace_label() {
        let kinds = [
            RequestKind::MainTurn,
            RequestKind::ToolContinuation,
            RequestKind::SideQuestion,
            RequestKind::ContextCheckpoint,
            RequestKind::Review,
            RequestKind::Subagent,
            RequestKind::Headless,
            RequestKind::HeadlessCheck,
            RequestKind::Scripted,
            RequestKind::ProviderProbe,
            RequestKind::GoalEvaluation,
            RequestKind::WorkflowStep,
            RequestKind::MultiRunCandidate,
            RequestKind::MultiRunJudge,
            RequestKind::MultiRunFusion,
            RequestKind::ChangeWalkthrough,
            RequestKind::Service,
            RequestKind::Scheduled,
            RequestKind::RoleRun,
            RequestKind::EmailGateway,
            RequestKind::Compatibility,
        ];
        let labels = kinds
            .iter()
            .map(|kind| kind.trace_label())
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(labels.len(), kinds.len());
        assert!(labels.iter().all(|label| !label.contains(' ')));
    }

    #[test]
    fn cap_disables_only_the_trace_and_reports_once() {
        let temp = tempfile::tempdir().unwrap();
        let hub = ProviderTraceHub::with_limit(temp.path().join("traces"), true, 256);
        hub.start().unwrap();
        for _ in 0..20 {
            hub.record(
                None,
                None,
                "large",
                serde_json::json!({"text": "x".repeat(128)}),
            );
        }
        for _ in 0..1000 {
            if hub.status().last_error.is_some() {
                break;
            }
            std::thread::yield_now();
        }
        assert!(!hub.status().enabled);
        assert!(hub.status().last_error.unwrap().contains("cap"));
        assert!(hub
            .take_diagnostic()
            .unwrap()
            .contains("Provider trace stopped"));
        assert!(hub.take_diagnostic().is_none());
    }

    #[test]
    fn stopped_trace_diagnostic_remains_visible_after_retirement() {
        let temp = tempfile::tempdir().unwrap();
        let hub = ProviderTraceHub::new(temp.path().join("traces"), true);
        hub.start().unwrap();
        let health = hub
            .inner
            .state
            .lock()
            .unwrap()
            .active
            .as_ref()
            .unwrap()
            .health
            .clone();
        health.fail("late writer failure");
        hub.stop_and_flush(Duration::from_secs(1));
        assert!(hub
            .take_diagnostic()
            .unwrap()
            .contains("late writer failure"));
        assert!(hub.take_diagnostic().is_none());
    }

    #[test]
    fn request_events_never_cross_trace_activations() {
        let temp = tempfile::tempdir().unwrap();
        let hub = ProviderTraceHub::new(temp.path().join("traces"), true);
        let TraceStart::Started(first) = hub.start().unwrap() else {
            panic!("first activation must start");
        };
        let request = PreparedModelRequest::new(
            RequestKind::MainTurn,
            "provider",
            "model",
            Vec::new(),
            Vec::new(),
            crate::agent::client::ChatRequestOptions::default(),
        );
        let trace_request = hub
            .begin_request_with_session(&request, "test", &[], None)
            .unwrap();
        hub.stop_and_flush(Duration::from_secs(1));

        let TraceStart::Started(second) = hub.start().unwrap() else {
            panic!("second activation must start");
        };
        trace_request.decoded_event(1, &ProviderEvent::Text("must-not-cross-activation".into()));
        trace_request.raw_frame(1, b"data: {\"password\":\"must-not-disable-new-trace");
        assert!(hub.status().enabled);
        hub.stop_and_flush(Duration::from_secs(1));

        assert!(!hub
            .open_text(first)
            .unwrap()
            .contains("must-not-cross-activation"));
        let second_text = hub.open_text(second).unwrap();
        assert!(!second_text.contains("must-not-cross-activation"));
        assert!(!second_text.contains("must-not-disable-new-trace"));
    }

    #[test]
    fn concurrent_requests_keep_identity_and_one_monotonic_sequence() {
        let temp = tempfile::tempdir().unwrap();
        let hub = ProviderTraceHub::new(temp.path().join("traces"), true);
        let TraceStart::Started(id) = hub.start().unwrap() else {
            panic!("first activation must start");
        };
        let workers = (0..4)
            .map(|worker| {
                let hub = hub.clone();
                std::thread::spawn(move || {
                    for item in 0..10 {
                        let request = PreparedModelRequest::new(
                            RequestKind::MainTurn,
                            "provider",
                            format!("model-{worker}"),
                            Vec::new(),
                            Vec::new(),
                            crate::agent::client::ChatRequestOptions::default(),
                        );
                        let trace = hub
                            .begin_request_with_session(&request, "test", &[], None)
                            .unwrap();
                        trace.attempt_started(
                            1,
                            "POST",
                            "https://example.test/v1?q=private",
                            &reqwest::header::HeaderMap::new(),
                            b"{}",
                        );
                        trace.decoded_event(
                            1,
                            &ProviderEvent::Text(format!("worker {worker} item {item}")),
                        );
                        trace.outcome(1, "completed", None);
                    }
                })
            })
            .collect::<Vec<_>>();
        for worker in workers {
            worker.join().unwrap();
        }
        hub.stop_and_flush(Duration::from_secs(1));
        let file = File::open(hub.directory().join(format!("{id}.jsonl"))).unwrap();
        let records = BufReader::new(file)
            .lines()
            .map(|line| serde_json::from_str::<Value>(&line.unwrap()).unwrap())
            .collect::<Vec<_>>();
        let sequences = records
            .iter()
            .map(|record| record["sequence"].as_u64().unwrap())
            .collect::<Vec<_>>();
        assert!(sequences
            .windows(2)
            .all(|window| window[1] == window[0] + 1));

        let mut events_by_request = std::collections::HashMap::<String, Vec<&str>>::new();
        for record in &records {
            let Some(request_id) = record["request_id"].as_str() else {
                continue;
            };
            events_by_request
                .entry(request_id.into())
                .or_default()
                .push(record["event"].as_str().unwrap());
        }
        assert_eq!(events_by_request.len(), 40);
        assert!(events_by_request.values().all(|events| {
            events.as_slice()
                == [
                    "request_started",
                    "attempt_started",
                    "decoded_provider_event",
                    "request_finished",
                ]
        }));
    }
}

#[cfg(test)]
mod native_content_privacy_tests {
    use super::*;
    #[test]
    fn native_signature_and_redacted_payload_never_enter_trace_frames() {
        let frame = br#"{"type":"content_block_start","content_block":{"type":"redacted_thinking","data":"opaque-private-state","signature":"signed-private-state"}}"#;
        let safe = sanitize_json_bytes(frame, &[], MAX_FRAME_BYTES)
            .unwrap()
            .to_string();
        assert!(!safe.contains("opaque-private-state"));
        assert!(!safe.contains("signed-private-state"));
    }
    #[test]
    fn google_signature_and_inline_media_never_enter_trace_frames() {
        let frame=br#"{"candidates":[{"content":{"parts":[{"text":"","thoughtSignature":"opaque-google-signature"},{"inlineData":{"mimeType":"image/png","data":"opaque-google-image"}}]}}]}"#;
        let safe = sanitize_json_bytes(frame, &[], MAX_FRAME_BYTES)
            .unwrap()
            .to_string();
        assert!(!safe.contains("opaque-google-signature"));
        assert!(!safe.contains("opaque-google-image"));
    }
}
