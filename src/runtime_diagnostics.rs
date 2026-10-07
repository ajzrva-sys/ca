use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use uuid::Uuid;

pub const RUNTIME_LOG_SCHEMA: &str = "ca.runtime-log.v1";
pub const RUNTIME_HEALTH_SCHEMA: &str = "ca.runtime-health.v1";
pub const MAX_LOG_BYTES: u64 = 1024 * 1024;
pub const LOG_FILE_COUNT: usize = 5;
const MAX_VIEW_BYTES: usize = 256 * 1024;
const MAX_VIEW_ENTRIES: usize = 500;
const WRITER_CHANNEL_CAPACITY: usize = 256;
pub const HEARTBEAT_STALL_MS: u64 = 2_000;
pub const SLOW_RENDER_MS: u64 = 100;
pub const FLICKER_WINDOW_MS: u64 = 1_000;
pub const FLICKER_FRAME_LIMIT: usize = 60;
pub const MEMORY_PRESSURE_BYTES: u64 = 1024 * 1024 * 1024;
pub const HIGH_MEMORY_BYTES: u64 = 1536 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeLogLevel {
    Info,
    Warn,
    Error,
}

impl RuntimeLogLevel {
    fn label(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeComponent {
    Process,
    EventLoop,
    Render,
    Terminal,
    Memory,
}

impl RuntimeComponent {
    fn label(self) -> &'static str {
        match self {
            Self::Process => "process",
            Self::EventLoop => "event_loop",
            Self::Render => "render",
            Self::Terminal => "terminal",
            Self::Memory => "memory",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeEvent {
    RunStarted,
    RunFinished,
    PreviousRunUnclean,
    PreviousRunHighMemory,
    HeartbeatStall,
    SlowRender,
    RenderFailed,
    ExcessiveRepaint,
    TerminalRestorationFailed,
    MemoryPressure,
    ProviderFailure,
    RecoveryScheduled,
    RecoverySucceeded,
    RecoveryCancelled,
    InputDelayed,
}

impl RuntimeEvent {
    fn label(self) -> &'static str {
        match self {
            Self::RunStarted => "run_started",
            Self::RunFinished => "run_finished",
            Self::PreviousRunUnclean => "previous_run_unclean",
            Self::PreviousRunHighMemory => "previous_run_high_memory",
            Self::HeartbeatStall => "heartbeat_stall",
            Self::SlowRender => "slow_render",
            Self::RenderFailed => "render_failed",
            Self::ExcessiveRepaint => "excessive_repaint",
            Self::TerminalRestorationFailed => "terminal_restoration_failed",
            Self::MemoryPressure => "memory_pressure",
            Self::ProviderFailure => "provider_failure",
            Self::RecoveryScheduled => "recovery_scheduled",
            Self::RecoverySucceeded => "recovery_succeeded",
            Self::RecoveryCancelled => "recovery_cancelled",
            Self::InputDelayed => "input_delayed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeErrorClass {
    Io,
    Render,
    Terminal,
    UnexpectedExit,
    HighMemoryExit,
    ProviderTimeout,
    ProviderConnect,
    ProviderTransport,
    ProviderHttp,
    ProviderEof,
    ProviderAuthentication,
    ProviderProtocol,
    ProviderIncomplete,
    ProviderRefused,
}

impl RuntimeErrorClass {
    fn label(self) -> &'static str {
        match self {
            Self::Io => "io",
            Self::Render => "render",
            Self::Terminal => "terminal",
            Self::UnexpectedExit => "unexpected_exit",
            Self::HighMemoryExit => "high_memory_exit",
            Self::ProviderTimeout => "provider_timeout",
            Self::ProviderConnect => "provider_connect",
            Self::ProviderTransport => "provider_transport",
            Self::ProviderHttp => "provider_http",
            Self::ProviderEof => "provider_eof",
            Self::ProviderAuthentication => "provider_authentication",
            Self::ProviderProtocol => "provider_protocol",
            Self::ProviderIncomplete => "provider_incomplete",
            Self::ProviderRefused => "provider_refused",
        }
    }
}

fn classify_unclean_exit(peak_bytes: u64) -> RuntimeErrorClass {
    if peak_bytes >= HIGH_MEMORY_BYTES {
        RuntimeErrorClass::HighMemoryExit
    } else {
        RuntimeErrorClass::UnexpectedExit
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeLogEntry {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
    pub component: RuntimeComponent,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_class: Option<RuntimeErrorClass>,
    pub event: RuntimeEvent,
    pub level: RuntimeLogLevel,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path_hash: Option<String>,
    pub run_id: Uuid,
    pub schema: String,
    pub timestamp: DateTime<Utc>,
}

impl RuntimeLogEntry {
    fn new(
        level: RuntimeLogLevel,
        component: RuntimeComponent,
        event: RuntimeEvent,
        run_id: Uuid,
    ) -> Self {
        Self {
            bytes: None,
            component,
            count: None,
            duration_ms: None,
            error_class: None,
            event,
            level,
            path_hash: None,
            run_id,
            schema: RUNTIME_LOG_SCHEMA.into(),
            timestamp: Utc::now(),
        }
    }

    fn render(&self) -> String {
        let mut line = format!(
            "{} {:<5} {:<10} {} run={}",
            self.timestamp.to_rfc3339(),
            self.level.label(),
            self.component.label(),
            self.event.label(),
            self.run_id
        );
        if let Some(count) = self.count {
            line.push_str(&format!(" count={count}"));
        }
        if let Some(duration_ms) = self.duration_ms {
            line.push_str(&format!(" duration_ms={duration_ms}"));
        }
        if let Some(bytes) = self.bytes {
            line.push_str(&format!(" bytes={bytes}"));
        }
        if let Some(error_class) = self.error_class {
            line.push_str(&format!(" error_class={}", error_class.label()));
        }
        if let Some(path_hash) = self.path_hash.as_deref() {
            line.push_str(&format!(" path_hash={path_hash}"));
        }
        line
    }

    fn with_path_hash(mut self, path: Option<&Path>) -> Self {
        if let Some(path) = path {
            let mut digest = Sha256::new();
            digest.update(self.run_id.as_bytes());
            digest.update(path.as_os_str().as_encoded_bytes());
            self.path_hash = Some(
                digest.finalize()[..16]
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect(),
            );
        }
        self
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RuntimeLogError {
    #[error("runtime logs are unavailable in a bare profile")]
    BareProfile,
    #[error("runtime log storage is unsafe")]
    UnsafeStorage,
    #[error("runtime log storage is unavailable")]
    Storage,
    #[error("invalid log filter; use level=<info|warn|error>, component=<name>, or event=<name>")]
    InvalidFilter,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RuntimeLogFilter {
    level: Option<RuntimeLogLevel>,
    component: Option<String>,
    event: Option<String>,
}

impl RuntimeLogFilter {
    pub fn parse(input: &str) -> Result<Self, RuntimeLogError> {
        let mut filter = Self::default();
        for token in input.split_whitespace() {
            let Some((key, value)) = token.split_once('=') else {
                return Err(RuntimeLogError::InvalidFilter);
            };
            if value.is_empty()
                || value.len() > 64
                || !value
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte == b'_')
            {
                return Err(RuntimeLogError::InvalidFilter);
            }
            match key {
                "level" => {
                    filter.level = Some(match value {
                        "info" => RuntimeLogLevel::Info,
                        "warn" => RuntimeLogLevel::Warn,
                        "error" => RuntimeLogLevel::Error,
                        _ => return Err(RuntimeLogError::InvalidFilter),
                    });
                }
                "component" => filter.component = Some(value.into()),
                "event" => filter.event = Some(value.into()),
                _ => return Err(RuntimeLogError::InvalidFilter),
            }
        }
        if filter.component.as_deref().is_some_and(|component| {
            !["process", "event_loop", "render", "terminal", "memory"].contains(&component)
        }) || filter.event.as_deref().is_some_and(|event| {
            ![
                "run_started",
                "run_finished",
                "previous_run_unclean",
                "previous_run_high_memory",
                "heartbeat_stall",
                "slow_render",
                "render_failed",
                "excessive_repaint",
                "terminal_restoration_failed",
                "memory_pressure",
                "provider_failure",
                "recovery_scheduled",
                "recovery_succeeded",
                "recovery_cancelled",
                "input_delayed",
            ]
            .contains(&event)
        }) {
            return Err(RuntimeLogError::InvalidFilter);
        }
        Ok(filter)
    }

    fn matches(&self, entry: &RuntimeLogEntry) -> bool {
        self.level.is_none_or(|level| level == entry.level)
            && self
                .component
                .as_deref()
                .is_none_or(|component| component == entry.component.label())
            && self
                .event
                .as_deref()
                .is_none_or(|event| event == entry.event.label())
    }
}

#[derive(Clone)]
pub struct RuntimeLogHub {
    inner: Arc<RuntimeLogHubInner>,
}

impl std::fmt::Debug for RuntimeLogHub {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RuntimeLogHub")
            .field("enabled", &self.inner.enabled)
            .field("run_id", &self.inner.run_id)
            .finish_non_exhaustive()
    }
}

struct RuntimeLogHubInner {
    directory: PathBuf,
    marker: PathBuf,
    enabled: bool,
    run_id: Uuid,
    max_log_bytes: u64,
    state: Mutex<RuntimeLogHubState>,
    writer_health: Arc<RuntimeLogWriterHealth>,
}

#[derive(Default)]
struct RuntimeLogHubState {
    started: bool,
    finished: bool,
    sender: Option<std::sync::mpsc::SyncSender<RuntimeLogWriterMessage>>,
}

#[derive(Default)]
struct RuntimeLogWriterHealth {
    last_error: Mutex<Option<RuntimeLogError>>,
    dropped: AtomicU64,
    failed: AtomicBool,
}

enum RuntimeLogWriterMessage {
    Entry(RuntimeLogEntry),
    Marker(RuntimeHealthMarker),
    Flush(std::sync::mpsc::SyncSender<()>),
    Finish {
        entry: RuntimeLogEntry,
        marker: RuntimeHealthMarker,
        acknowledgement: std::sync::mpsc::SyncSender<()>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RuntimeHealthMarker {
    schema: String,
    run_id: Uuid,
    clean: bool,
    high_memory: bool,
    peak_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeHealthSnapshot {
    pub schema: String,
    pub run_id: Uuid,
    pub clean: bool,
    pub high_memory: bool,
    pub peak_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeSupportSnapshot {
    pub schema: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health: Option<RuntimeHealthSnapshot>,
    pub entries: Vec<RuntimeLogEntry>,
}

/// Read the already metadata-only runtime store without creating or modifying it.
/// Every source is opened without following symlinks and revalidated against the
/// inspected inode before its typed entries are retained.
pub fn support_snapshot(
    directory: &Path,
    since: Option<DateTime<Utc>>,
) -> Result<RuntimeSupportSnapshot, RuntimeLogError> {
    if !directory.exists() {
        return Ok(RuntimeSupportSnapshot {
            schema: RUNTIME_HEALTH_SCHEMA.into(),
            health: None,
            entries: Vec::new(),
        });
    }
    prepare_directory(directory)?;
    validate_existing_logs(directory)?;
    let marker =
        read_marker(&directory.join("health-v1.json"))?.map(|marker| RuntimeHealthSnapshot {
            schema: marker.schema,
            run_id: marker.run_id,
            clean: marker.clean,
            high_memory: marker.high_memory,
            peak_bytes: marker.peak_bytes,
        });
    let mut entries = Vec::new();
    let mut retained_bytes = 0usize;
    for index in (0..LOG_FILE_COUNT).rev() {
        let path = log_path(directory, index);
        if !path.exists() {
            continue;
        }
        let file = open_readonly_secure(&path)?;
        for line in BufReader::new(file).lines() {
            let line = line.map_err(|_| RuntimeLogError::Storage)?;
            let Ok(entry) = serde_json::from_str::<RuntimeLogEntry>(&line) else {
                continue;
            };
            if entry.schema != RUNTIME_LOG_SCHEMA
                || since.is_some_and(|since| entry.timestamp < since)
            {
                continue;
            }
            retained_bytes = retained_bytes.saturating_add(line.len()).saturating_add(1);
            if retained_bytes > LOG_FILE_COUNT.saturating_mul(MAX_LOG_BYTES as usize) {
                return Err(RuntimeLogError::Storage);
            }
            entries.push(entry);
        }
    }
    Ok(RuntimeSupportSnapshot {
        schema: RUNTIME_HEALTH_SCHEMA.into(),
        health: marker,
        entries,
    })
}

impl RuntimeLogHub {
    pub fn disabled() -> Self {
        Self::new(PathBuf::new(), false)
    }

    pub fn new(directory: PathBuf, enabled: bool) -> Self {
        let marker = directory.join("health-v1.json");
        Self {
            inner: Arc::new(RuntimeLogHubInner {
                directory,
                marker,
                enabled,
                run_id: Uuid::new_v4(),
                max_log_bytes: MAX_LOG_BYTES,
                state: Mutex::new(RuntimeLogHubState::default()),
                writer_health: Arc::new(RuntimeLogWriterHealth::default()),
            }),
        }
    }

    #[cfg(test)]
    fn with_limit(directory: PathBuf, max_log_bytes: u64) -> Self {
        let mut hub = Self::new(directory, true);
        Arc::get_mut(&mut hub.inner).unwrap().max_log_bytes = max_log_bytes;
        hub
    }

    pub fn enabled(&self) -> bool {
        self.inner.enabled
    }

    pub fn run_id(&self) -> Uuid {
        self.inner.run_id
    }

    pub fn start(&self) -> Result<(), RuntimeLogError> {
        if !self.inner.enabled {
            return Ok(());
        }
        let mut state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if state.started {
            return Ok(());
        }
        prepare_directory(&self.inner.directory)?;
        validate_existing_logs(&self.inner.directory)?;
        let mut previous_entry = None;
        if let Ok(Some(marker)) = read_marker(&self.inner.marker) {
            if !marker.clean {
                let event = if marker.high_memory && marker.peak_bytes >= HIGH_MEMORY_BYTES {
                    RuntimeEvent::PreviousRunHighMemory
                } else {
                    RuntimeEvent::PreviousRunUnclean
                };
                let mut entry = RuntimeLogEntry::new(
                    RuntimeLogLevel::Warn,
                    RuntimeComponent::Process,
                    event,
                    self.inner.run_id,
                );
                entry.bytes = Some(marker.peak_bytes);
                entry.error_class = Some(classify_unclean_exit(marker.peak_bytes));
                previous_entry = Some(entry);
            }
        }
        let initial_marker = RuntimeHealthMarker {
            schema: RUNTIME_HEALTH_SCHEMA.into(),
            run_id: self.inner.run_id,
            clean: false,
            high_memory: false,
            peak_bytes: 0,
        };
        write_marker(&self.inner.marker, &initial_marker)?;
        let (sender, receiver) =
            std::sync::mpsc::sync_channel::<RuntimeLogWriterMessage>(WRITER_CHANNEL_CAPACITY);
        let directory = self.inner.directory.clone();
        let marker_path = self.inner.marker.clone();
        let max_log_bytes = self.inner.max_log_bytes;
        let writer_health = self.inner.writer_health.clone();
        if std::thread::Builder::new()
            .name(format!(
                "ca-runtime-log-{}",
                &self.inner.run_id.to_string()[..8]
            ))
            .spawn(move || {
                runtime_log_writer_loop(
                    &directory,
                    &marker_path,
                    max_log_bytes,
                    receiver,
                    &writer_health,
                )
            })
            .is_err()
        {
            let _ = write_marker(
                &self.inner.marker,
                &RuntimeHealthMarker {
                    clean: true,
                    ..initial_marker
                },
            );
            return Err(RuntimeLogError::Storage);
        }
        state.sender = Some(sender);
        state.started = true;
        drop(state);
        if let Some(entry) = previous_entry {
            self.record(entry);
        }
        self.record(
            RuntimeLogEntry::new(
                RuntimeLogLevel::Info,
                RuntimeComponent::Process,
                RuntimeEvent::RunStarted,
                self.inner.run_id,
            )
            .with_path_hash(std::env::current_dir().ok().as_deref()),
        );
        Ok(())
    }

    pub fn finish(&self, clean: bool, peak_bytes: u64) {
        if !self.inner.enabled {
            return;
        }
        let sender = {
            let mut state = self
                .inner
                .state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            if !state.started || state.finished {
                return;
            }
            state.finished = true;
            state.sender.take()
        };
        let Some(sender) = sender else {
            return;
        };
        let mut entry = RuntimeLogEntry::new(
            if clean {
                RuntimeLogLevel::Info
            } else {
                RuntimeLogLevel::Warn
            },
            RuntimeComponent::Process,
            RuntimeEvent::RunFinished,
            self.inner.run_id,
        );
        entry.bytes = Some(peak_bytes);
        if !clean {
            entry.error_class = Some(classify_unclean_exit(peak_bytes));
        }
        let marker = RuntimeHealthMarker {
            schema: RUNTIME_HEALTH_SCHEMA.into(),
            run_id: self.inner.run_id,
            clean,
            high_memory: peak_bytes >= HIGH_MEMORY_BYTES,
            peak_bytes,
        };
        let (acknowledgement, receiver) = std::sync::mpsc::sync_channel(1);
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut message = RuntimeLogWriterMessage::Finish {
            entry,
            marker,
            acknowledgement,
        };
        loop {
            match sender.try_send(message) {
                Ok(()) => break,
                Err(std::sync::mpsc::TrySendError::Full(returned)) if Instant::now() < deadline => {
                    message = returned;
                    std::thread::sleep(Duration::from_millis(2));
                }
                Err(_) => {
                    self.writer_failed(RuntimeLogError::Storage);
                    return;
                }
            }
        }
        if receiver
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .is_err()
        {
            self.writer_failed(RuntimeLogError::Storage);
        }
    }

    pub fn record(&self, mut entry: RuntimeLogEntry) {
        if !self.inner.enabled {
            return;
        }
        entry.schema = RUNTIME_LOG_SCHEMA.into();
        entry.run_id = self.inner.run_id;
        self.try_send(RuntimeLogWriterMessage::Entry(entry));
    }

    pub fn update_memory_marker(&self, peak_bytes: u64) {
        if !self.inner.enabled {
            return;
        }
        self.try_send(RuntimeLogWriterMessage::Marker(RuntimeHealthMarker {
            schema: RUNTIME_HEALTH_SCHEMA.into(),
            run_id: self.inner.run_id,
            clean: false,
            high_memory: peak_bytes >= HIGH_MEMORY_BYTES,
            peak_bytes,
        }));
    }

    fn try_send(&self, message: RuntimeLogWriterMessage) {
        if self.inner.writer_health.failed.load(Ordering::Acquire) {
            return;
        }
        let sender = self
            .inner
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .sender
            .clone();
        let Some(sender) = sender else {
            return;
        };
        match sender.try_send(message) {
            Ok(()) => {}
            Err(std::sync::mpsc::TrySendError::Full(_)) => {
                self.inner
                    .writer_health
                    .dropped
                    .fetch_add(1, Ordering::Relaxed);
            }
            Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                self.writer_failed(RuntimeLogError::Storage);
            }
        }
    }

    fn flush(&self, timeout: Duration) {
        let sender = self
            .inner
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .sender
            .clone();
        let Some(sender) = sender else {
            return;
        };
        let (acknowledgement, receiver) = std::sync::mpsc::sync_channel(1);
        if sender
            .try_send(RuntimeLogWriterMessage::Flush(acknowledgement))
            .is_ok()
        {
            let _ = receiver.recv_timeout(timeout);
        }
    }

    fn writer_failed(&self, error: RuntimeLogError) {
        self.inner
            .writer_health
            .failed
            .store(true, Ordering::Release);
        let mut last_error = self
            .inner
            .writer_health
            .last_error
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if last_error.is_none() {
            *last_error = Some(error);
        }
    }

    pub fn view(&self, filter: &RuntimeLogFilter) -> Result<String, RuntimeLogError> {
        if !self.inner.enabled {
            return Err(RuntimeLogError::BareProfile);
        }
        self.flush(Duration::from_millis(250));
        prepare_directory(&self.inner.directory)?;
        validate_existing_logs(&self.inner.directory)?;
        let mut entries = VecDeque::new();
        let mut retained_bytes = 0usize;
        let mut truncated = false;
        for index in (0..LOG_FILE_COUNT).rev() {
            let path = log_path(&self.inner.directory, index);
            if !path.exists() {
                continue;
            }
            let file = open_readonly_secure(&path)?;
            for line in BufReader::new(file).lines() {
                let line = line.map_err(|_| RuntimeLogError::Storage)?;
                let Ok(entry) = serde_json::from_str::<RuntimeLogEntry>(&line) else {
                    continue;
                };
                if entry.schema == RUNTIME_LOG_SCHEMA && filter.matches(&entry) {
                    let rendered_bytes = entry.render().len().saturating_add(1);
                    entries.push_back((entry, rendered_bytes));
                    retained_bytes = retained_bytes.saturating_add(rendered_bytes);
                    while retained_bytes > MAX_VIEW_BYTES || entries.len() > MAX_VIEW_ENTRIES {
                        if let Some((_, removed_bytes)) = entries.pop_front() {
                            retained_bytes = retained_bytes.saturating_sub(removed_bytes);
                            truncated = true;
                        }
                    }
                }
            }
        }
        let mut output = String::from("Runtime logs\n\n");
        let dropped = self.inner.writer_health.dropped.load(Ordering::Relaxed);
        if dropped > 0 {
            output.push_str(&format!(
                "Dropped metadata events in this process: {dropped}\n\n"
            ));
        }
        if entries.is_empty() {
            output.push_str("No matching entries.");
        } else {
            for (entry, _) in entries {
                output.push_str(&entry.render());
                output.push('\n');
            }
        }
        if truncated {
            output.push_str("[… older matching entries omitted by display bounds]\n");
        }
        Ok(output)
    }

    pub fn take_error(&self) -> Option<RuntimeLogError> {
        self.inner
            .writer_health
            .last_error
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .take()
    }
}

fn runtime_log_writer_loop(
    directory: &Path,
    marker_path: &Path,
    max_log_bytes: u64,
    receiver: std::sync::mpsc::Receiver<RuntimeLogWriterMessage>,
    health: &RuntimeLogWriterHealth,
) {
    while let Ok(message) = receiver.recv() {
        let result = match message {
            RuntimeLogWriterMessage::Entry(entry) => {
                write_log_entry(directory, max_log_bytes, &entry)
            }
            RuntimeLogWriterMessage::Marker(marker) => write_marker(marker_path, &marker),
            RuntimeLogWriterMessage::Flush(acknowledgement) => {
                let _ = acknowledgement.try_send(());
                continue;
            }
            RuntimeLogWriterMessage::Finish {
                entry,
                marker,
                acknowledgement,
            } => {
                let result = write_log_entry(directory, max_log_bytes, &entry)
                    .and_then(|_| write_marker(marker_path, &marker));
                let _ = acknowledgement.try_send(());
                if let Err(error) = result {
                    record_writer_error(health, error);
                }
                break;
            }
        };
        if let Err(error) = result {
            record_writer_error(health, error);
        }
    }
}

fn record_writer_error(health: &RuntimeLogWriterHealth, error: RuntimeLogError) {
    health.failed.store(true, Ordering::Release);
    let mut last_error = health
        .last_error
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    if last_error.is_none() {
        *last_error = Some(error);
    }
}

fn write_log_entry(
    directory: &Path,
    max_log_bytes: u64,
    entry: &RuntimeLogEntry,
) -> Result<(), RuntimeLogError> {
    let mut encoded = serde_json::to_vec(entry).map_err(|_| RuntimeLogError::Storage)?;
    encoded.push(b'\n');
    if encoded.len() as u64 > max_log_bytes {
        return Err(RuntimeLogError::Storage);
    }
    let current = log_path(directory, 0);
    if current.exists() {
        let metadata = secure_metadata(&current)?;
        if metadata.len().saturating_add(encoded.len() as u64) > max_log_bytes {
            rotate(directory)?;
        }
    }
    let mut file = open_append_secure(&current)?;
    file.write_all(&encoded)
        .and_then(|_| file.flush())
        .map_err(|_| RuntimeLogError::Storage)
}

fn validate_existing_logs(directory: &Path) -> Result<(), RuntimeLogError> {
    for index in 0..LOG_FILE_COUNT {
        let path = log_path(directory, index);
        if path.exists() {
            secure_metadata(&path)?;
        }
    }
    Ok(())
}

fn log_path(directory: &Path, index: usize) -> PathBuf {
    if index == 0 {
        directory.join("runtime.jsonl")
    } else {
        directory.join(format!("runtime.jsonl.{index}"))
    }
}

fn prepare_directory(directory: &Path) -> Result<(), RuntimeLogError> {
    if directory.as_os_str().is_empty() {
        return Err(RuntimeLogError::BareProfile);
    }
    if let Ok(metadata) = std::fs::symlink_metadata(directory) {
        if !metadata.file_type().is_dir()
            || metadata.file_type().is_symlink()
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.mode() & 0o077 != 0
        {
            return Err(RuntimeLogError::UnsafeStorage);
        }
        return Ok(());
    }
    std::fs::create_dir_all(directory).map_err(|_| RuntimeLogError::Storage)?;
    std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))
        .map_err(|_| RuntimeLogError::Storage)?;
    let metadata = std::fs::symlink_metadata(directory).map_err(|_| RuntimeLogError::Storage)?;
    if !metadata.file_type().is_dir()
        || metadata.file_type().is_symlink()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o077 != 0
    {
        return Err(RuntimeLogError::UnsafeStorage);
    }
    Ok(())
}

fn secure_metadata(path: &Path) -> Result<std::fs::Metadata, RuntimeLogError> {
    let metadata = std::fs::symlink_metadata(path).map_err(|_| RuntimeLogError::Storage)?;
    if !metadata.file_type().is_file()
        || metadata.file_type().is_symlink()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o077 != 0
        || metadata.nlink() != 1
    {
        return Err(RuntimeLogError::UnsafeStorage);
    }
    Ok(metadata)
}

fn open_append_secure(path: &Path) -> Result<File, RuntimeLogError> {
    if path.exists() {
        secure_metadata(path)?;
    }
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(path)
        .map_err(|_| RuntimeLogError::Storage)?;
    let metadata = file.metadata().map_err(|_| RuntimeLogError::Storage)?;
    if !metadata.file_type().is_file()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o077 != 0
        || metadata.nlink() != 1
    {
        return Err(RuntimeLogError::UnsafeStorage);
    }
    Ok(file)
}

fn open_readonly_secure(path: &Path) -> Result<File, RuntimeLogError> {
    secure_metadata(path)?;
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(path)
        .map_err(|_| RuntimeLogError::Storage)
}

fn rotate(directory: &Path) -> Result<(), RuntimeLogError> {
    let oldest = log_path(directory, LOG_FILE_COUNT - 1);
    if oldest.exists() {
        secure_metadata(&oldest)?;
        std::fs::remove_file(&oldest).map_err(|_| RuntimeLogError::Storage)?;
    }
    for index in (0..LOG_FILE_COUNT - 1).rev() {
        let source = log_path(directory, index);
        if !source.exists() {
            continue;
        }
        secure_metadata(&source)?;
        let destination = log_path(directory, index + 1);
        if destination.exists() {
            secure_metadata(&destination)?;
        }
        std::fs::rename(source, destination).map_err(|_| RuntimeLogError::Storage)?;
    }
    Ok(())
}

fn write_marker(path: &Path, marker: &RuntimeHealthMarker) -> Result<(), RuntimeLogError> {
    let directory = path.parent().ok_or(RuntimeLogError::Storage)?;
    prepare_directory(directory)?;
    if path.exists() {
        secure_metadata(path)?;
    }
    let bytes = serde_json::to_vec(marker).map_err(|_| RuntimeLogError::Storage)?;
    let temporary = directory.join(format!(".health-{}.tmp", Uuid::new_v4()));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(&temporary)
        .map_err(|_| RuntimeLogError::Storage)?;
    if file
        .write_all(&bytes)
        .and_then(|_| file.sync_all())
        .is_err()
    {
        let _ = std::fs::remove_file(&temporary);
        return Err(RuntimeLogError::Storage);
    }
    if std::fs::rename(&temporary, path).is_err() {
        let _ = std::fs::remove_file(&temporary);
        return Err(RuntimeLogError::Storage);
    }
    Ok(())
}

fn read_marker(path: &Path) -> Result<Option<RuntimeHealthMarker>, RuntimeLogError> {
    if !path.exists() {
        return Ok(None);
    }
    let file = open_readonly_secure(path)?;
    let marker: RuntimeHealthMarker =
        serde_json::from_reader(file).map_err(|_| RuntimeLogError::Storage)?;
    if marker.schema != RUNTIME_HEALTH_SCHEMA {
        return Err(RuntimeLogError::Storage);
    }
    Ok(Some(marker))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeSignal {
    HeartbeatStall { duration_ms: u64 },
    SlowRender { duration_ms: u64 },
    RenderFailed,
    ExcessiveRepaint { count: u64 },
    TerminalRestorationFailed,
    MemoryPressure { bytes: u64 },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RenderDebugSnapshot {
    pub redraw_count: u64,
    pub dirty_regions: u64,
    pub last_frame_ms: u64,
    pub max_frame_ms: u64,
    pub render_failures: u64,
    pub repaint_rate: u64,
    pub repaint_highlight: bool,
}

impl RenderDebugSnapshot {
    pub fn render(self) -> String {
        format!(
            "Render diagnostics\n\nRedraws: {}\nDirty regions: {}\nLast frame: {} ms\nSlowest frame: {} ms\nRender failures: {}\nRepaint rate: {}/s\nRepaint highlighting: {}",
            self.redraw_count,
            self.dirty_regions,
            self.last_frame_ms,
            self.max_frame_ms,
            self.render_failures,
            self.repaint_rate,
            if self.repaint_highlight { "on" } else { "off" }
        )
    }
}

#[derive(Debug, Default)]
pub struct RuntimeHealth {
    last_heartbeat_ms: Option<u64>,
    last_stall_signal_ms: Option<u64>,
    redraw_count: u64,
    dirty_regions: u64,
    last_frame_ms: u64,
    max_frame_ms: u64,
    render_failures: u64,
    repaint_times: VecDeque<u64>,
    last_flicker_signal_ms: Option<u64>,
    terminal_restoration_failures: u64,
    peak_memory_bytes: u64,
    memory_pressure_reported: bool,
    repaint_highlight: bool,
}

impl RuntimeHealth {
    pub fn heartbeat(&mut self, now_ms: u64) -> Option<RuntimeSignal> {
        let signal = self.last_heartbeat_ms.and_then(|previous| {
            let duration_ms = now_ms.saturating_sub(previous);
            let rate_limited = self
                .last_stall_signal_ms
                .is_some_and(|last| now_ms.saturating_sub(last) < HEARTBEAT_STALL_MS);
            (duration_ms >= HEARTBEAT_STALL_MS && !rate_limited)
                .then_some(RuntimeSignal::HeartbeatStall { duration_ms })
        });
        if signal.is_some() {
            self.last_stall_signal_ms = Some(now_ms);
        }
        self.last_heartbeat_ms = Some(now_ms);
        signal
    }

    pub fn render_succeeded(
        &mut self,
        now_ms: u64,
        duration_ms: u64,
        dirty_regions: u64,
    ) -> Vec<RuntimeSignal> {
        self.redraw_count = self.redraw_count.saturating_add(1);
        self.dirty_regions = self.dirty_regions.saturating_add(dirty_regions);
        self.last_frame_ms = duration_ms;
        self.max_frame_ms = self.max_frame_ms.max(duration_ms);
        self.repaint_times.push_back(now_ms);
        while self
            .repaint_times
            .front()
            .is_some_and(|oldest| now_ms.saturating_sub(*oldest) > FLICKER_WINDOW_MS)
        {
            self.repaint_times.pop_front();
        }
        let mut signals = Vec::new();
        if duration_ms >= SLOW_RENDER_MS {
            signals.push(RuntimeSignal::SlowRender { duration_ms });
        }
        if self.repaint_times.len() > FLICKER_FRAME_LIMIT
            && self
                .last_flicker_signal_ms
                .is_none_or(|last| now_ms.saturating_sub(last) >= FLICKER_WINDOW_MS)
        {
            self.last_flicker_signal_ms = Some(now_ms);
            signals.push(RuntimeSignal::ExcessiveRepaint {
                count: self.repaint_times.len() as u64,
            });
        }
        signals
    }

    pub fn render_failed(&mut self) -> RuntimeSignal {
        self.render_failures = self.render_failures.saturating_add(1);
        RuntimeSignal::RenderFailed
    }

    pub fn terminal_restoration_failed(&mut self) -> RuntimeSignal {
        self.terminal_restoration_failures = self.terminal_restoration_failures.saturating_add(1);
        RuntimeSignal::TerminalRestorationFailed
    }

    pub fn observe_memory(&mut self, bytes: u64) -> Option<RuntimeSignal> {
        self.peak_memory_bytes = self.peak_memory_bytes.max(bytes);
        if bytes >= MEMORY_PRESSURE_BYTES && !self.memory_pressure_reported {
            self.memory_pressure_reported = true;
            Some(RuntimeSignal::MemoryPressure { bytes })
        } else {
            None
        }
    }

    pub fn peak_memory_bytes(&self) -> u64 {
        self.peak_memory_bytes
    }

    pub fn set_repaint_highlight(&mut self, enabled: bool) {
        self.repaint_highlight = enabled;
    }

    pub fn reset_render(&mut self) {
        self.redraw_count = 0;
        self.dirty_regions = 0;
        self.last_frame_ms = 0;
        self.max_frame_ms = 0;
        self.render_failures = 0;
        self.repaint_times.clear();
        self.last_flicker_signal_ms = None;
    }

    pub fn render_snapshot(&self) -> RenderDebugSnapshot {
        RenderDebugSnapshot {
            redraw_count: self.redraw_count,
            dirty_regions: self.dirty_regions,
            last_frame_ms: self.last_frame_ms,
            max_frame_ms: self.max_frame_ms,
            render_failures: self.render_failures,
            repaint_rate: self.repaint_times.len() as u64,
            repaint_highlight: self.repaint_highlight,
        }
    }
}

#[derive(Clone)]
pub struct RuntimeDiagnostics {
    hub: RuntimeLogHub,
    health: Arc<Mutex<RuntimeHealth>>,
    started: Instant,
}

impl std::fmt::Debug for RuntimeDiagnostics {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RuntimeDiagnostics")
            .field("enabled", &self.hub.enabled())
            .finish_non_exhaustive()
    }
}

impl Default for RuntimeDiagnostics {
    fn default() -> Self {
        Self::disabled()
    }
}

impl RuntimeDiagnostics {
    pub fn disabled() -> Self {
        Self {
            hub: RuntimeLogHub::disabled(),
            health: Arc::new(Mutex::new(RuntimeHealth::default())),
            started: Instant::now(),
        }
    }

    pub fn new(hub: RuntimeLogHub) -> Self {
        Self {
            hub,
            health: Arc::new(Mutex::new(RuntimeHealth::default())),
            started: Instant::now(),
        }
    }

    pub fn hub(&self) -> RuntimeLogHub {
        self.hub.clone()
    }

    pub fn heartbeat(&self) {
        let now_ms = self.started.elapsed().as_millis() as u64;
        let signal = self
            .health
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .heartbeat(now_ms);
        if let Some(signal) = signal {
            self.record_signal(signal);
        }
    }

    pub fn render_succeeded(&self, duration: Duration, dirty_regions: u64) {
        let now_ms = self.started.elapsed().as_millis() as u64;
        let signals = self
            .health
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .render_succeeded(now_ms, duration.as_millis() as u64, dirty_regions);
        for signal in signals {
            self.record_signal(signal);
        }
    }

    pub fn render_failed(&self) {
        let signal = self
            .health
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .render_failed();
        self.record_signal(signal);
    }

    pub fn terminal_restoration_failed(&self) {
        let signal = self
            .health
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .terminal_restoration_failed();
        self.record_signal(signal);
    }

    pub fn sample_memory(&self, bytes: u64) {
        let signal = {
            let mut health = self
                .health
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            let previous_peak = health.peak_memory_bytes();
            let signal = health.observe_memory(bytes);
            if health.peak_memory_bytes() > previous_peak {
                self.hub.update_memory_marker(health.peak_memory_bytes());
            }
            signal
        };
        if let Some(signal) = signal {
            self.record_signal(signal);
        }
    }

    pub fn peak_memory_bytes(&self) -> u64 {
        self.health
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .peak_memory_bytes()
    }

    pub fn render_snapshot(&self) -> RenderDebugSnapshot {
        self.health
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .render_snapshot()
    }

    pub fn set_repaint_highlight(&self, enabled: bool) {
        self.health
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .set_repaint_highlight(enabled);
    }

    pub fn reset_render(&self) {
        self.health
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .reset_render();
    }

    pub(crate) fn input_dequeued(&self, age: Duration, remaining: usize) {
        // Metadata only, at most one queue warning per second across panes.
        static LAST_WARNING: AtomicU64 = AtomicU64::new(0);
        if age.as_millis() < 100 {
            return;
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let previous = LAST_WARNING.load(Ordering::Relaxed);
        if now.saturating_sub(previous) < 1_000
            || LAST_WARNING
                .compare_exchange(previous, now, Ordering::Relaxed, Ordering::Relaxed)
                .is_err()
        {
            return;
        }
        let mut entry = RuntimeLogEntry::new(
            RuntimeLogLevel::Warn,
            RuntimeComponent::EventLoop,
            RuntimeEvent::InputDelayed,
            self.hub.run_id(),
        );
        entry.duration_ms = Some(age.as_millis().min(u128::from(u64::MAX)) as u64);
        entry.count = Some(remaining as u64);
        self.hub.record(entry);
    }

    pub(crate) fn provider_event(&self, event: RuntimeEvent, count: Option<u64>) {
        let mut entry = RuntimeLogEntry::new(
            RuntimeLogLevel::Info,
            RuntimeComponent::EventLoop,
            event,
            self.hub.run_id(),
        );
        entry.count = count;
        self.hub.record(entry);
    }

    pub(crate) fn provider_failure(&self, category: crate::turn_recovery::Category) {
        use crate::turn_recovery::Category;
        let mut entry = RuntimeLogEntry::new(
            RuntimeLogLevel::Warn,
            RuntimeComponent::EventLoop,
            RuntimeEvent::ProviderFailure,
            self.hub.run_id(),
        );
        entry.error_class = Some(match category {
            Category::Timeout => RuntimeErrorClass::ProviderTimeout,
            Category::Connect => RuntimeErrorClass::ProviderConnect,
            Category::Transport => RuntimeErrorClass::ProviderTransport,
            Category::Http(status) => {
                entry.count = Some(u64::from(status));
                RuntimeErrorClass::ProviderHttp
            }
            Category::PrematureEof => RuntimeErrorClass::ProviderEof,
            Category::Authentication => RuntimeErrorClass::ProviderAuthentication,
            Category::Protocol => RuntimeErrorClass::ProviderProtocol,
            Category::Incomplete => RuntimeErrorClass::ProviderIncomplete,
            Category::Refused => RuntimeErrorClass::ProviderRefused,
        });
        self.hub.record(entry);
    }

    fn record_signal(&self, signal: RuntimeSignal) {
        let (level, component, event, count, duration_ms, bytes, error_class) = match signal {
            RuntimeSignal::HeartbeatStall { duration_ms } => (
                RuntimeLogLevel::Warn,
                RuntimeComponent::EventLoop,
                RuntimeEvent::HeartbeatStall,
                None,
                Some(duration_ms),
                None,
                None,
            ),
            RuntimeSignal::SlowRender { duration_ms } => (
                RuntimeLogLevel::Warn,
                RuntimeComponent::Render,
                RuntimeEvent::SlowRender,
                None,
                Some(duration_ms),
                None,
                None,
            ),
            RuntimeSignal::RenderFailed => (
                RuntimeLogLevel::Error,
                RuntimeComponent::Render,
                RuntimeEvent::RenderFailed,
                None,
                None,
                None,
                Some(RuntimeErrorClass::Render),
            ),
            RuntimeSignal::ExcessiveRepaint { count } => (
                RuntimeLogLevel::Warn,
                RuntimeComponent::Render,
                RuntimeEvent::ExcessiveRepaint,
                Some(count),
                None,
                None,
                None,
            ),
            RuntimeSignal::TerminalRestorationFailed => (
                RuntimeLogLevel::Error,
                RuntimeComponent::Terminal,
                RuntimeEvent::TerminalRestorationFailed,
                None,
                None,
                None,
                Some(RuntimeErrorClass::Terminal),
            ),
            RuntimeSignal::MemoryPressure { bytes } => (
                RuntimeLogLevel::Warn,
                RuntimeComponent::Memory,
                RuntimeEvent::MemoryPressure,
                None,
                None,
                Some(bytes),
                None,
            ),
        };
        let mut entry = RuntimeLogEntry::new(level, component, event, self.hub.run_id());
        entry.count = count;
        entry.duration_ms = duration_ms;
        entry.bytes = bytes;
        entry.error_class = error_class;
        self.hub.record(entry);
    }
}

static PROCESS_DIAGNOSTICS: OnceLock<Mutex<RuntimeDiagnostics>> = OnceLock::new();

fn process_slot() -> &'static Mutex<RuntimeDiagnostics> {
    PROCESS_DIAGNOSTICS.get_or_init(|| Mutex::new(RuntimeDiagnostics::disabled()))
}

pub fn install_process(diagnostics: RuntimeDiagnostics) {
    *process_slot()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner()) = diagnostics;
}

pub fn process() -> RuntimeDiagnostics {
    process_slot()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .clone()
}

pub fn current_resident_bytes() -> Option<u64> {
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) } != 0 {
        return None;
    }
    #[cfg(target_os = "macos")]
    let bytes = usage.ru_maxrss.max(0) as u64;
    #[cfg(not(target_os = "macos"))]
    let bytes = (usage.ru_maxrss.max(0) as u64).saturating_mul(1024);
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    fn entry(event: RuntimeEvent) -> RuntimeLogEntry {
        RuntimeLogEntry::new(
            RuntimeLogLevel::Info,
            RuntimeComponent::Process,
            event,
            Uuid::nil(),
        )
    }

    #[test]
    fn log_schema_has_no_free_form_content_fields() {
        let value = serde_json::to_value(entry(RuntimeEvent::RunStarted)).unwrap();
        let object = value.as_object().unwrap();
        assert_eq!(
            object.keys().map(String::as_str).collect::<Vec<_>>(),
            [
                "component",
                "event",
                "level",
                "run_id",
                "schema",
                "timestamp"
            ]
        );
        for forbidden in [
            "message",
            "prompt",
            "path",
            "url",
            "arguments",
            "output",
            "environment",
            "payload",
        ] {
            assert!(!object.contains_key(forbidden));
        }
    }

    #[test]
    fn rotation_is_bounded_and_files_are_private() {
        let root = tempfile::tempdir().unwrap();
        let hub = RuntimeLogHub::with_limit(root.path().join("logs"), 320);
        hub.start().unwrap();
        for _ in 0..30 {
            hub.record(entry(RuntimeEvent::RunStarted));
        }
        hub.flush(Duration::from_secs(1));
        let files = std::fs::read_dir(root.path().join("logs"))
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("runtime.jsonl")
            })
            .collect::<Vec<_>>();
        assert!(files.len() <= LOG_FILE_COUNT);
        assert!(!files.is_empty());
        for file in files {
            let metadata = file.metadata().unwrap();
            assert_eq!(metadata.mode() & 0o077, 0);
            assert!(metadata.len() <= 320);
        }
    }

    #[test]
    fn unsafe_symlink_and_hardlink_targets_are_rejected() {
        let root = tempfile::tempdir().unwrap();
        let outside = root.path().join("outside");
        std::fs::create_dir(&outside).unwrap();
        let link = root.path().join("linked");
        symlink(&outside, &link).unwrap();
        assert_eq!(
            RuntimeLogHub::new(link, true).start().unwrap_err(),
            RuntimeLogError::UnsafeStorage
        );

        let logs = root.path().join("logs");
        std::fs::create_dir(&logs).unwrap();
        std::fs::set_permissions(&logs, std::fs::Permissions::from_mode(0o700)).unwrap();
        let target = logs.join("runtime.jsonl");
        std::fs::write(&target, "").unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::fs::hard_link(&target, root.path().join("alias")).unwrap();
        assert_eq!(
            RuntimeLogHub::new(logs, true).start().unwrap_err(),
            RuntimeLogError::UnsafeStorage
        );
    }

    #[test]
    fn viewer_filters_typed_metadata_and_ignores_malformed_lines() {
        let root = tempfile::tempdir().unwrap();
        let hub = RuntimeLogHub::new(root.path().join("logs"), true);
        hub.start().unwrap();
        let mut warning = entry(RuntimeEvent::SlowRender);
        warning.level = RuntimeLogLevel::Warn;
        warning.component = RuntimeComponent::Render;
        warning.duration_ms = Some(140);
        hub.record(warning);
        let mut file = open_append_secure(&log_path(&root.path().join("logs"), 0)).unwrap();
        writeln!(file, "not-json").unwrap();

        let filter = RuntimeLogFilter::parse("level=warn component=render").unwrap();
        let view = hub.view(&filter).unwrap();
        assert!(view.contains("slow_render"));
        assert!(view.contains("duration_ms=140"));
        assert!(!view.contains("run_started"));
        assert!(!view.contains("not-json"));
        assert_eq!(
            RuntimeLogFilter::parse("query=secret").unwrap_err(),
            RuntimeLogError::InvalidFilter
        );
        assert_eq!(
            RuntimeLogFilter::parse("component=provider").unwrap_err(),
            RuntimeLogError::InvalidFilter
        );
    }

    #[test]
    fn current_directory_is_logged_only_as_a_process_local_hash() {
        let root = tempfile::tempdir().unwrap();
        let hub = RuntimeLogHub::new(root.path().join("logs"), true);
        hub.start().unwrap();
        let view = hub.view(&RuntimeLogFilter::default()).unwrap();
        assert!(view.contains("path_hash="));
        let cwd = std::env::current_dir()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert!(!view.contains(&cwd));
        let hash = view
            .split("path_hash=")
            .nth(1)
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap();
        assert_eq!(hash.len(), 32);
        assert!(hash.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }

    #[test]
    fn fake_clock_detects_stalls_slow_frames_and_flicker() {
        let mut health = RuntimeHealth::default();
        assert_eq!(health.heartbeat(0), None);
        assert_eq!(
            health.heartbeat(HEARTBEAT_STALL_MS + 5),
            Some(RuntimeSignal::HeartbeatStall {
                duration_ms: HEARTBEAT_STALL_MS + 5
            })
        );
        let slow = health.render_succeeded(2_010, SLOW_RENDER_MS, 2);
        assert!(slow.contains(&RuntimeSignal::SlowRender {
            duration_ms: SLOW_RENDER_MS
        }));
        let mut flicker = Vec::new();
        for frame in 0..=FLICKER_FRAME_LIMIT {
            flicker.extend(health.render_succeeded(3_000 + frame as u64, 1, 1));
        }
        assert!(flicker.iter().any(|signal| matches!(
            signal,
            RuntimeSignal::ExcessiveRepaint { count } if *count > FLICKER_FRAME_LIMIT as u64
        )));
        let snapshot = health.render_snapshot();
        assert_eq!(snapshot.dirty_regions, 2 + FLICKER_FRAME_LIMIT as u64 + 1);
        assert!(snapshot.redraw_count > FLICKER_FRAME_LIMIT as u64);
    }

    #[test]
    fn render_failure_restoration_and_debug_reset_are_deterministic() {
        let mut health = RuntimeHealth::default();
        assert_eq!(health.render_failed(), RuntimeSignal::RenderFailed);
        assert_eq!(
            health.terminal_restoration_failed(),
            RuntimeSignal::TerminalRestorationFailed
        );
        health.render_succeeded(10, 7, 3);
        health.set_repaint_highlight(true);
        let snapshot = health.render_snapshot();
        assert_eq!(snapshot.render_failures, 1);
        assert_eq!(snapshot.dirty_regions, 3);
        assert!(snapshot.repaint_highlight);
        health.reset_render();
        let reset = health.render_snapshot();
        assert_eq!(reset.redraw_count, 0);
        assert_eq!(reset.render_failures, 0);
        assert!(reset.repaint_highlight);
    }

    #[test]
    fn unclean_exit_is_not_called_high_memory_without_evidence() {
        let mut health = RuntimeHealth::default();
        assert_eq!(
            classify_unclean_exit(health.peak_memory_bytes()),
            RuntimeErrorClass::UnexpectedExit
        );
        assert_eq!(
            health.observe_memory(MEMORY_PRESSURE_BYTES),
            Some(RuntimeSignal::MemoryPressure {
                bytes: MEMORY_PRESSURE_BYTES
            })
        );
        assert_eq!(
            classify_unclean_exit(health.peak_memory_bytes()),
            RuntimeErrorClass::UnexpectedExit
        );
        health.observe_memory(HIGH_MEMORY_BYTES);
        assert_eq!(
            classify_unclean_exit(health.peak_memory_bytes()),
            RuntimeErrorClass::HighMemoryExit
        );
    }

    #[test]
    fn marker_classifies_unclean_exit_only_with_persisted_high_memory() {
        let root = tempfile::tempdir().unwrap();
        let logs = root.path().join("logs");
        let first = RuntimeLogHub::new(logs.clone(), true);
        first.start().unwrap();
        first.finish(false, MEMORY_PRESSURE_BYTES);
        let second = RuntimeLogHub::new(logs.clone(), true);
        second.start().unwrap();
        let view = second.view(&RuntimeLogFilter::default()).unwrap();
        assert!(view.contains("previous_run_unclean"));
        assert!(!view.contains("previous_run_high_memory"));
        second.finish(false, HIGH_MEMORY_BYTES);
        let third = RuntimeLogHub::new(logs, true);
        third.start().unwrap();
        let view = third.view(&RuntimeLogFilter::default()).unwrap();
        assert!(view.contains("previous_run_high_memory"));
    }

    #[test]
    fn support_snapshot_filters_by_time_and_refuses_hardlinked_logs() {
        let root = tempfile::tempdir().unwrap();
        let logs = root.path().join("logs");
        let hub = RuntimeLogHub::new(logs.clone(), true);
        hub.start().unwrap();
        hub.record(RuntimeLogEntry::new(
            RuntimeLogLevel::Warn,
            RuntimeComponent::Render,
            RuntimeEvent::SlowRender,
            hub.run_id(),
        ));
        hub.finish(true, 42);

        let retained = support_snapshot(
            &logs,
            Some(chrono::Utc::now() - chrono::Duration::minutes(1)),
        )
        .unwrap();
        assert!(retained.health.is_some());
        assert!(retained
            .entries
            .iter()
            .any(|entry| entry.event == RuntimeEvent::SlowRender));
        let excluded = support_snapshot(
            &logs,
            Some(chrono::Utc::now() + chrono::Duration::minutes(1)),
        )
        .unwrap();
        assert!(excluded.entries.is_empty());

        let current = log_path(&logs, 0);
        let hardlink = logs.join("runtime-hardlink.jsonl");
        std::fs::hard_link(&current, &hardlink).unwrap();
        assert_eq!(
            support_snapshot(&logs, None),
            Err(RuntimeLogError::UnsafeStorage)
        );
    }
}
