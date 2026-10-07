//! Session outcome telemetry — records whether the user's session achieved its goal.
//!
//! Stores task identities, counters and bounded redacted evidence excerpts.

use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Instant;

// ---------------------------------------------------------------------------
// Outcome types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutcomeKind {
    /// No verified result or explicit feedback is available.
    Unknown,
    /// User explicitly accepted the result.
    Accepted,
    /// User explicitly undid or rejected agent work before session end.
    Rejected,
    /// User abandoned the session without completing (Ctrl+C / quit mid-task).
    Abandoned,
    /// Session completed with no tool calls — informational only.
    Completed,
    /// Agent loop hit an unrecoverable error.
    Failed,
}

impl OutcomeKind {
    pub fn is_positive(self) -> bool {
        matches!(self, Self::Accepted)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionStatus {
    #[default]
    Running,
    Completed,
    Interrupted,
    Failed,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationStatus {
    #[default]
    Unverified,
    Passed,
    Failed,
    Stale,
}

/// A focused recipe or a model review does not establish task acceptance.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationScope {
    #[default]
    Unknown,
    FocusedChecks,
    TaskContract,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserFeedback {
    #[default]
    Unknown,
    Accepted,
    Rejected,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceEvent {
    pub id: String,
    pub sequence: u64,
    pub kind: String,
    pub tool: Option<String>,
    pub call_id: Option<String>,
    pub succeeded: Option<bool>,
    pub excerpt: String,
}

// ---------------------------------------------------------------------------
// Per-tool summary
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolSummary {
    pub total: u32,
    pub succeeded: u32,
    pub failed: u32,
    pub denied: u32,
    pub retried: u32,
}

impl ToolSummary {
    pub fn failure_rate(&self) -> f64 {
        if self.total == 0 {
            return 0.0;
        }
        self.failed as f64 / self.total as f64
    }
}

// ---------------------------------------------------------------------------
// User signals — implicit indicators of satisfaction or frustration
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UserSignals {
    /// Number of times the user used /undo or undo slash command.
    pub undo_count: u32,
    /// Number of times the loop guard detected repeated failures.
    pub loop_guard_trips: u32,
    /// Number of user messages that were corrections (contain "no", "wrong",
    /// "incorrect", "actually", "not what I meant", etc.).
    pub correction_count: u32,
    /// Number of times the user interrupted/stopped a turn in progress.
    pub interrupt_count: u32,
    /// Number of tool calls the user denied (n/a/d at approval prompt).
    pub denial_count: u32,
    /// Number of distinct user messages sent.
    pub user_message_count: u32,
    /// Number of distinct assistant turns completed.
    pub assistant_turn_count: u32,
    /// Number of tool calls executed across all turns.
    pub total_tool_calls: u32,
}

// ---------------------------------------------------------------------------
// Harness snapshot — content hashes of active harness components
// ---------------------------------------------------------------------------

/// SHA-256 hex digest of a harness component's content.
pub type ComponentHash = String;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessSnapshot {
    #[serde(default)]
    pub build: Option<ComponentHash>,
    #[serde(default)]
    pub model_route: Option<ComponentHash>,
    /// Hash of AGENTS.md (or CLAUDE.md) project instructions.
    pub agents_md: Option<ComponentHash>,
    /// Hash of the assembled system prompt.
    pub system_prompt: Option<ComponentHash>,
    /// Hash of the tool definition set (sorted names + descriptions).
    pub tool_definitions: Option<ComponentHash>,
    /// Hash of active skill names + their content digests.
    pub active_skills: Option<ComponentHash>,
    /// Hash of the approval policy configuration.
    pub approval_policy: Option<ComponentHash>,
    /// Hash of memory store contents.
    pub memory_store: Option<ComponentHash>,
    /// Hash of config.toml relevant fields.
    pub config: Option<ComponentHash>,
    /// Hash of QUALITY.md guardrails.
    pub quality_guardrails: Option<ComponentHash>,
    /// Hash of hooks configuration.
    pub hooks: Option<ComponentHash>,
}

// ---------------------------------------------------------------------------
// Session outcome record
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionOutcome {
    #[serde(default)]
    pub evidence_complete: bool,
    #[serde(default)]
    pub harness_epochs: Vec<HarnessSnapshot>,
    #[serde(default)]
    pub task_id: String,
    #[serde(default)]
    pub parent_task_id: Option<String>,
    #[serde(default)]
    pub root_task_id: Option<String>,
    #[serde(default)]
    pub execution: ExecutionStatus,
    #[serde(default)]
    pub verification: VerificationStatus,
    #[serde(default)]
    pub verification_scope: VerificationScope,
    #[serde(default)]
    pub feedback: UserFeedback,
    #[serde(default)]
    pub feedback_revision: u128,
    #[serde(default)]
    pub events: Vec<EvidenceEvent>,
    /// Schema version for forward-compatible storage.
    pub version: u32,
    /// Session UUID.
    pub session_id: String,
    /// When the session started (Unix timestamp seconds).
    pub started_at: u64,
    /// Duration of the session.
    pub duration_secs: u64,
    /// Final outcome.
    pub outcome: OutcomeKind,
    /// Per-tool summary, keyed by tool name.
    pub tools: BTreeMap<String, ToolSummary>,
    /// Aggregate user frustration/satisfaction signals.
    pub signals: UserSignals,
    /// Hashes of the harness components active during this session.
    pub harness: HarnessSnapshot,
    /// Workspace fingerprint — hash of the working directory path.
    pub workspace_hash: String,
    /// Model used for most of the session.
    pub model: Option<String>,
    /// Provider used for most of the session.
    pub provider: Option<String>,
}

const OUTCOME_VERSION: u32 = 2;

impl SessionOutcome {
    pub fn independent_task_id(&self) -> &str {
        self.root_task_id.as_deref().unwrap_or(&self.task_id)
    }

    pub fn verified_positive(&self) -> bool {
        self.version >= 2
            && self.evidence_complete
            && self.feedback != UserFeedback::Rejected
            && (self.feedback == UserFeedback::Accepted
                || (self.verification == VerificationStatus::Passed
                    && self.verification_scope == VerificationScope::TaskContract
                    && self.execution == ExecutionStatus::Completed))
    }
}

// ---------------------------------------------------------------------------
// In-process outcome tracker
// ---------------------------------------------------------------------------

/// Accumulates signals throughout a session. Call [`OutcomeTracker::finish`]
/// when the session ends to produce a [`SessionOutcome`].
#[derive(Debug, Clone)]
pub struct OutcomeTracker {
    sink: Option<OutcomeSink>,
    execution_hints: crate::loop_guard::ExecutionHints,
    task_id: String,
    parent_task_id: Option<String>,
    root_task_id: Option<String>,
    execution: ExecutionStatus,
    verification: VerificationStatus,
    verification_scope: VerificationScope,
    feedback: UserFeedback,
    feedback_revision: u128,
    events: Vec<EvidenceEvent>,
    evidence_complete: bool,
    session_id: String,
    workspace_hash: String,
    started_at: Instant,
    started_epoch: u64,
    outcome: OutcomeKind,
    model: Option<String>,
    provider: Option<String>,

    tools: BTreeMap<String, ToolSummary>,
    signals: UserSignals,
    harness: HarnessSnapshot,
    harness_epochs: Vec<HarnessSnapshot>,

    /// Track which (turn_generation, tool_id) pairs we've already counted.
    seen_tools: std::collections::HashSet<(u64, String)>,
}

#[derive(Debug, Clone)]
pub struct OutcomeSink {
    sender: std::sync::mpsc::SyncSender<OutcomeWrite>,
    failed: std::sync::Arc<std::sync::atomic::AtomicBool>,
}
#[derive(Debug)]
enum OutcomeWrite {
    Save(Box<SessionOutcome>),
    Snapshot(crate::harness_registry::ComponentSnapshot),
    Flush(std::sync::mpsc::Sender<()>),
}
impl OutcomeSink {
    pub fn new(data_dir: std::path::PathBuf) -> Self {
        let (sender, receiver) = std::sync::mpsc::sync_channel(256);
        let failed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let writer_failed = failed.clone();
        std::thread::spawn(move || {
            for message in receiver {
                match message {
                    OutcomeWrite::Save(mut outcome) => {
                        outcome.evidence_complete &=
                            !writer_failed.load(std::sync::atomic::Ordering::Acquire);
                        if let Err(error) = save_outcome(&outcome, &data_dir) {
                            writer_failed.store(true, std::sync::atomic::Ordering::Release);
                            tracing::warn!("harness outcome storage unavailable: {error}");
                        }
                    }
                    OutcomeWrite::Snapshot(snapshot) => {
                        if let Err(error) =
                            crate::harness_registry::save_snapshot(&snapshot, &data_dir)
                        {
                            writer_failed.store(true, std::sync::atomic::Ordering::Release);
                            tracing::warn!("harness snapshot storage unavailable: {error}");
                        }
                    }
                    OutcomeWrite::Flush(done) => {
                        let _ = done.send(());
                    }
                }
            }
        });
        Self { sender, failed }
    }
    /// Wait for all previously admitted writes, including when the queue is full.
    /// Call through `flush_async` from an async runtime.
    pub fn flush(&self) -> std::io::Result<()> {
        self.flush_until(Instant::now() + std::time::Duration::from_secs(5))
    }

    fn flush_until(&self, deadline: Instant) -> std::io::Result<()> {
        let (sender, receiver) = std::sync::mpsc::channel();
        let mut message = OutcomeWrite::Flush(sender);
        loop {
            match self.sender.try_send(message) {
                Ok(()) => break,
                Err(std::sync::mpsc::TrySendError::Full(pending)) => {
                    message = pending;
                    if Instant::now() >= deadline {
                        self.failed
                            .store(true, std::sync::atomic::Ordering::Release);
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::TimedOut,
                            "harness outcome flush queue deadline exceeded",
                        ));
                    }
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                    self.failed
                        .store(true, std::sync::atomic::Ordering::Release);
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::BrokenPipe,
                        "harness outcome writer stopped",
                    ));
                }
            }
        }
        receiver
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .map_err(|error| {
                self.failed
                    .store(true, std::sync::atomic::Ordering::Release);
                std::io::Error::other(format!("harness outcome flush incomplete: {error}"))
            })?;
        if self.failed.load(std::sync::atomic::Ordering::Acquire) {
            return Err(std::io::Error::other("harness evidence writes were lost"));
        }
        Ok(())
    }

    pub async fn flush_async(&self) {
        let sink = self.clone();
        match tokio::task::spawn_blocking(move || sink.flush()).await {
            Ok(Ok(())) => {}
            result => tracing::warn!("harness evidence flush incomplete: {result:?}"),
        }
    }
}

impl OutcomeTracker {
    pub fn new(session_id: String, cwd: &str) -> Self {
        let workspace_hash = hex_sha256(cwd.as_bytes());
        Self {
            sink: None,
            execution_hints: crate::loop_guard::ExecutionHints::default(),
            task_id: uuid::Uuid::new_v4().to_string(),
            parent_task_id: None,
            root_task_id: None,
            execution: ExecutionStatus::Running,
            verification: VerificationStatus::Unverified,
            verification_scope: VerificationScope::Unknown,
            feedback: UserFeedback::Unknown,
            feedback_revision: 0,
            events: Vec::new(),
            evidence_complete: true,
            session_id,
            workspace_hash,
            started_at: Instant::now(),
            started_epoch: now_epoch(),
            outcome: OutcomeKind::Unknown,
            model: None,
            provider: None,
            tools: BTreeMap::new(),
            signals: UserSignals::default(),
            harness: HarnessSnapshot::default(),
            harness_epochs: Vec::new(),
            seen_tools: std::collections::HashSet::new(),
        }
    }

    pub fn for_run(session_id: String, task_id: uuid::Uuid, cwd: &str) -> Self {
        let mut tracker = Self::new(session_id, cwd);
        tracker.task_id = task_id.to_string();
        tracker
    }

    pub fn child(&self, task_id: uuid::Uuid, cwd: &str) -> Self {
        let mut child = Self::for_run(self.session_id.clone(), task_id, cwd);
        child.parent_task_id = Some(self.task_id.clone());
        child.root_task_id = Some(
            self.root_task_id
                .clone()
                .unwrap_or_else(|| self.task_id.clone()),
        );
        child.sink = self.sink.clone();
        child
    }

    pub fn begin_execution(&mut self) {
        self.execution = ExecutionStatus::Running;
    }

    pub fn with_sink(mut self, sink: OutcomeSink) -> Self {
        self.sink = Some(sink);
        self
    }

    pub fn start_task(&mut self, session_id: String, cwd: &str) {
        self.persist();
        let sink = self.sink.clone();
        *self = Self::new(session_id, cwd);
        self.sink = sink;
    }

    pub fn persist(&self) {
        if self.has_activity() {
            if let Some(sink) = &self.sink {
                if sink
                    .sender
                    .try_send(OutcomeWrite::Save(Box::new(self.clone().finish())))
                    .is_err()
                {
                    sink.failed
                        .store(true, std::sync::atomic::Ordering::Release);
                    tracing::warn!("harness outcome writer full; evidence may be incomplete");
                }
            }
        }
    }

    pub fn sink(&self) -> Option<OutcomeSink> {
        self.sink.clone()
    }

    // -- setters for effective request identity --

    pub fn set_runtime_harness(&mut self, snapshot: crate::harness_registry::ComponentSnapshot) {
        self.set_harness(snapshot.outcome_snapshot());
        if let Some(sink) = &self.sink {
            if sink
                .sender
                .try_send(OutcomeWrite::Snapshot(snapshot))
                .is_err()
            {
                sink.failed
                    .store(true, std::sync::atomic::Ordering::Release);
                tracing::warn!("harness snapshot writer full; evidence may be incomplete");
            }
        }
    }

    pub fn set_harness(&mut self, snapshot: HarnessSnapshot) {
        if self.harness_epochs.last() != Some(&snapshot) {
            self.harness_epochs.push(snapshot.clone());
            self.event(
                "harness_epoch",
                None,
                None,
                None,
                "Effective runtime inputs changed",
            );
        }
        self.harness = snapshot;
    }

    pub fn set_model(&mut self, model: Option<String>, provider: Option<String>) {
        self.model = model;
        self.provider = provider;
    }

    pub fn set_outcome(&mut self, outcome: OutcomeKind) {
        self.feedback_revision = feedback_revision().max(self.feedback_revision.saturating_add(1));
        self.outcome = outcome;
        self.feedback = match outcome {
            OutcomeKind::Accepted => UserFeedback::Accepted,
            OutcomeKind::Rejected => UserFeedback::Rejected,
            _ => UserFeedback::Unknown,
        };
    }

    pub fn outcome(&self) -> OutcomeKind {
        self.outcome
    }

    // -- signal recording (called during the session) --

    /// Record a tool call result. Returns `true` if this is the first time
    /// we've seen this specific tool call (by turn generation + id).
    pub fn record_tool(
        &mut self,
        turn_generation: u64,
        tool_id: &str,
        tool_name: &str,
        succeeded: bool,
        was_denied: bool,
    ) -> bool {
        if !self.seen_tools.insert((turn_generation, tool_id.into())) {
            return false;
        }
        if !was_denied
            && crate::agent::registry::ToolRegistry::builtins()
                .get(tool_name)
                .is_none_or(|spec| spec.mutation == crate::agent::registry::ToolMutation::Mutating)
        {
            self.invalidate_verification();
        }
        let entry = self.tools.entry(tool_name.into()).or_default();
        entry.total += 1;
        if was_denied {
            entry.denied += 1;
        } else if succeeded {
            entry.succeeded += 1;
        } else {
            entry.failed += 1;
        }
        self.signals.total_tool_calls += 1;
        self.event(
            "tool_result",
            Some(tool_name),
            Some(tool_id),
            Some(succeeded && !was_denied),
            if was_denied {
                "Tool denied by user"
            } else if succeeded {
                "Tool completed"
            } else {
                "Tool failed"
            },
        );
        if was_denied {
            self.record_denial();
        }
        true
    }

    pub fn record_undo(&mut self) {
        self.invalidate_verification();
        self.signals.undo_count += 1;
    }

    pub fn record_loop_guard_trip(&mut self) {
        self.signals.loop_guard_trips += 1;
    }

    pub fn record_correction(&mut self) {
        self.signals.correction_count += 1;
    }

    pub fn record_interrupt(&mut self) {
        self.execution = ExecutionStatus::Interrupted;
        self.outcome = OutcomeKind::Abandoned;
        self.signals.interrupt_count += 1;
        self.persist();
    }

    pub fn record_denial(&mut self) {
        self.signals.denial_count += 1;
    }

    pub fn record_user_message(&mut self) {
        self.signals.user_message_count += 1;
    }

    pub fn record_assistant_turn(&mut self) {
        if self.execution != ExecutionStatus::Completed {
            self.execution = ExecutionStatus::Completed;
            self.signals.assistant_turn_count += 1;
            self.persist();
        }
    }

    /// Mark a tool call as retried (loop guard detected repetition for this tool).
    pub fn record_tool_retry(&mut self, tool_name: &str) {
        if let Some(entry) = self.tools.get_mut(tool_name) {
            entry.retried += 1;
        }
    }

    pub fn record_failure(&mut self) {
        self.execution = ExecutionStatus::Failed;
        self.outcome = OutcomeKind::Failed;
        self.event(
            "provider_failure",
            None,
            None,
            Some(false),
            "Provider or execution failed",
        );
        self.persist();
    }

    pub fn event(
        &mut self,
        kind: &str,
        tool: Option<&str>,
        call_id: Option<&str>,
        succeeded: Option<bool>,
        excerpt: &str,
    ) {
        if self.events.len() >= 256 {
            self.evidence_complete = false;
            return;
        }
        let sequence = self.events.len() as u64;
        let excerpt = crate::learning::sanitized_learning_text(excerpt)
            .unwrap_or_else(|| "[private evidence omitted]".into())
            .chars()
            .take(1024)
            .collect();
        self.events.push(EvidenceEvent {
            id: format!("{}:{sequence}", self.task_id),
            sequence,
            kind: kind.into(),
            tool: tool.map(str::to_string),
            call_id: call_id.map(str::to_string),
            succeeded,
            excerpt,
        });
    }

    pub fn observe_execution_hint(&mut self, command: &str) {
        if let Some((pattern, hint)) = self.execution_hints.observe(command) {
            // Observation only until a matched experiment authorizes activation.
            self.event("execution_hint_observed", Some(pattern), None, None, hint);
        }
    }

    pub fn record_host_verification(
        &mut self,
        outcome: &crate::mechanistic_delegation::AutomaticVerificationOutcome,
    ) {
        use crate::mechanistic_delegation::AutomaticVerificationStatus as Status;
        let receipt = outcome.receipt_digest.as_deref().filter(|digest| {
            digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
        });
        let stale = outcome
            .reason_codes
            .iter()
            .any(|reason| reason == "stale_revision" || reason.starts_with("checkpoint_"));
        let status = match outcome.status {
            _ if stale => Some(VerificationStatus::Stale),
            Status::Passed
                if receipt.is_some()
                    && outcome
                        .repository_revision
                        .as_deref()
                        .is_some_and(|revision| !revision.is_empty())
                    && !outcome.selected_recipes.is_empty() =>
            {
                Some(VerificationStatus::Passed)
            }
            Status::Passed => Some(VerificationStatus::Unverified),
            Status::Failed => Some(VerificationStatus::Failed),
            Status::Unchanged => None,
        };
        if let Some(status) = status {
            self.verification = status;
            self.verification_scope = VerificationScope::FocusedChecks;
        }
        let excerpt = serde_json::json!({
            "scope": "focused_checks", "status": status,
            "receipt_digest": receipt,
            "repository_revision": outcome.repository_revision.as_deref().map(|text| text.chars().take(128).collect::<String>()),
            "selected_recipes": outcome.selected_recipes.iter().take(8).map(|text| text.chars().take(48).collect::<String>()).collect::<Vec<_>>(),
            "reason_codes": outcome.reason_codes.iter().take(4).map(|text| text.chars().take(48).collect::<String>()).collect::<Vec<_>>(),
        }).to_string();
        self.event(
            "host_verification",
            None,
            None,
            status.map(|s| s == VerificationStatus::Passed),
            &excerpt,
        );
        self.persist();
    }

    pub fn record_model_verification(&mut self, passed: bool, summary: &str) {
        self.event(
            "model_verification_report",
            None,
            None,
            Some(passed),
            summary,
        );
        self.persist();
    }

    pub fn invalidate_verification(&mut self) {
        if matches!(
            self.verification,
            VerificationStatus::Passed | VerificationStatus::Failed
        ) {
            self.verification = VerificationStatus::Stale;
            self.event(
                "verification_invalidated",
                None,
                None,
                None,
                "Workspace may have changed after verification",
            );
            self.persist();
        }
    }

    pub fn has_activity(&self) -> bool {
        self.signals.user_message_count > 0 || !self.events.is_empty()
    }

    // -- finalize --

    pub fn finish(self) -> SessionOutcome {
        SessionOutcome {
            harness_epochs: self.harness_epochs,
            task_id: self.task_id,
            parent_task_id: self.parent_task_id,
            root_task_id: self.root_task_id,
            execution: self.execution,
            verification: self.verification,
            verification_scope: self.verification_scope,
            feedback: self.feedback,
            feedback_revision: self.feedback_revision,
            events: self.events,
            evidence_complete: self.evidence_complete
                && !self
                    .sink
                    .as_ref()
                    .is_some_and(|sink| sink.failed.load(std::sync::atomic::Ordering::Acquire)),
            version: OUTCOME_VERSION,
            session_id: self.session_id,
            started_at: self.started_epoch,
            duration_secs: self.started_at.elapsed().as_secs(),
            outcome: self.outcome,
            tools: self.tools,
            signals: self.signals,
            harness: self.harness,
            workspace_hash: self.workspace_hash,
            model: self.model,
            provider: self.provider,
        }
    }
}

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

/// Persist a session outcome to disk under the given data directory.
pub fn save_outcome(outcome: &SessionOutcome, data_dir: &Path) -> std::io::Result<()> {
    let dir = outcomes_dir(data_dir);
    std::fs::create_dir_all(&dir)?;
    ensure_outcome_directory(&dir)?;
    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    use std::os::unix::fs::OpenOptionsExt;
    options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    let lock = options.open(dir.join(".lock"))?;
    lock.lock()?;
    let mut outcome = outcome.clone();
    let id = if outcome.task_id.is_empty() {
        &outcome.session_id
    } else {
        &outcome.task_id
    };
    validate_id(id)?;
    let path = dir.join(format!("{id}.json"));
    if let Some(previous) = load_outcome(id, data_dir)? {
        if previous.feedback_revision > outcome.feedback_revision {
            outcome.feedback = previous.feedback;
            outcome.feedback_revision = previous.feedback_revision;
            outcome.outcome = match previous.feedback {
                UserFeedback::Accepted => OutcomeKind::Accepted,
                UserFeedback::Rejected => OutcomeKind::Rejected,
                UserFeedback::Unknown => OutcomeKind::Unknown,
            };
        }
    }
    let json = serde_json::to_string_pretty(&outcome)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    if path
        .symlink_metadata()
        .is_ok_and(|m| m.file_type().is_symlink())
    {
        return Err(std::io::Error::other("outcome path is a symlink"));
    }
    let mut temp = tempfile::NamedTempFile::new_in(&dir)?;
    use std::io::Write;
    temp.write_all(json.as_bytes())?;
    temp.as_file().sync_all()?;
    temp.persist(&path).map_err(|e| e.error)?;
    Ok(())
}

/// Load a specific session outcome.
#[allow(dead_code)]
pub fn load_outcome(session_id: &str, data_dir: &Path) -> std::io::Result<Option<SessionOutcome>> {
    validate_id(session_id)?;
    let path = outcomes_dir(data_dir).join(format!("{session_id}.json"));
    if !path.exists() {
        return Ok(None);
    }
    let metadata = std::fs::symlink_metadata(&path)?;
    if !metadata.is_file() || metadata.len() > 2 * 1024 * 1024 {
        return Err(std::io::Error::other(
            "outcome must be a bounded regular file",
        ));
    }
    let mut options = std::fs::OpenOptions::new();
    use std::os::unix::fs::OpenOptionsExt;
    options.read(true).custom_flags(libc::O_NOFOLLOW);
    let mut file = options.open(&path)?;
    let mut data = String::new();
    use std::io::Read;
    file.read_to_string(&mut data)?;
    let outcome: SessionOutcome = serde_json::from_str(&data)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    Ok(Some(outcome))
}

/// List the N most recent outcomes, newest first.
pub fn recent_outcomes(limit: usize, data_dir: &Path) -> std::io::Result<Vec<SessionOutcome>> {
    let dir = outcomes_dir(data_dir);
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let entries: Vec<_> = std::fs::read_dir(&dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "json"))
        .collect();
    let mut outcomes = Vec::with_capacity(limit.min(entries.len()));
    for entry in entries {
        if !entry.file_type().is_ok_and(|t| t.is_file())
            || entry.metadata().is_ok_and(|m| m.len() > 2 * 1024 * 1024)
        {
            continue;
        }
        match std::fs::read_to_string(entry.path()) {
            Ok(data) => {
                if let Ok(outcome) = serde_json::from_str::<SessionOutcome>(&data) {
                    outcomes.push(outcome);
                }
            }
            Err(_) => continue,
        }
    }
    outcomes.sort_by(|a, b| {
        b.started_at
            .cmp(&a.started_at)
            .then_with(|| b.task_id.cmp(&a.task_id))
    });
    outcomes.truncate(limit);
    Ok(outcomes)
}

fn validate_id(id: &str) -> std::io::Result<()> {
    if id.is_empty()
        || id.len() > 128
        || !id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
    {
        return Err(std::io::Error::other("invalid outcome ID"));
    }
    Ok(())
}

pub fn set_feedback(id: &str, feedback: UserFeedback, data_dir: &Path) -> std::io::Result<()> {
    let mut outcome =
        load_outcome(id, data_dir)?.ok_or_else(|| std::io::Error::other("task not found"))?;
    if outcome.version < 2 {
        return Err(std::io::Error::other(
            "legacy outcomes cannot be upgraded to verified evidence",
        ));
    }
    outcome.feedback_revision =
        feedback_revision().max(outcome.feedback_revision.saturating_add(1));
    outcome.feedback = feedback;
    outcome.outcome = match feedback {
        UserFeedback::Accepted => OutcomeKind::Accepted,
        UserFeedback::Rejected => OutcomeKind::Rejected,
        UserFeedback::Unknown => OutcomeKind::Unknown,
    };
    save_outcome(&outcome, data_dir)
}

fn outcomes_dir(data_dir: &Path) -> std::path::PathBuf {
    data_dir.join("outcomes")
}

fn now_epoch() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn hex_sha256(data: &[u8]) -> String {
    let hash = sha2::Sha256::digest(data);
    format!("{hash:x}")
}

// ---------------------------------------------------------------------------
// Correction detection heuristic
// ---------------------------------------------------------------------------

/// Returns `true` if the user message looks like a correction.
pub fn looks_like_correction(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    let patterns = [
        "no,",
        "no.",
        "no ",
        "wrong",
        "incorrect",
        "actually",
        "not what i meant",
        "that's not right",
        "that is not right",
        "try again",
        "do it again",
        "fix this",
        "this is broken",
        "you broke",
        "revert",
        "undo that",
        "that's wrong",
        "that is wrong",
    ];
    let trimmed = lower.trim();
    patterns
        .iter()
        .any(|p| trimmed.starts_with(p) || trimmed.contains(p))
}

// ---------------------------------------------------------------------------
// CLI helpers
// ---------------------------------------------------------------------------

/// Format a summary table of recent outcomes for display.
pub fn format_outcomes_table(outcomes: &[SessionOutcome]) -> String {
    if outcomes.is_empty() {
        return "No session outcomes recorded.".into();
    }
    let mut out = String::new();
    out.push_str(&format!(
        "{:<12} {:<12} {:<8} {:<8} {:<8} {}\n",
        "Task ID", "Outcome", "Tools", "Failed", "Undos", "Duration"
    ));
    out.push_str(&"-".repeat(72));
    out.push('\n');
    for o in outcomes {
        let total_tools: u32 = o.tools.values().map(|t| t.total).sum();
        let total_failed: u32 = o.tools.values().map(|t| t.failed).sum();
        let id_short = if o.task_id.is_empty() {
            &o.session_id
        } else {
            &o.task_id
        };
        let dur = format_duration(o.duration_secs);
        out.push_str(&format!(
            "{:<12} {:<12} {:<8} {:<8} {:<8} {}\n",
            id_short,
            format!(
                "{:?}{}",
                o.outcome,
                if o.version < 2 { " (legacy)" } else { "" }
            ),
            total_tools,
            total_failed,
            o.signals.undo_count,
            dur,
        ));
    }
    out
}

fn format_duration(secs: u64) -> String {
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m{}s", secs / 60, secs % 60)
    } else {
        format!("{}h{}m", secs / 3600, (secs % 3600) / 60)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Persists an interrupted execution when an async task is dropped or aborted.
pub struct ScopedOutcome(pub OutcomeTracker);
impl std::ops::Deref for ScopedOutcome {
    type Target = OutcomeTracker;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for ScopedOutcome {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
impl Drop for ScopedOutcome {
    fn drop(&mut self) {
        if self.0.execution == ExecutionStatus::Running {
            self.0.record_interrupt();
        } else {
            self.0.persist();
        }
    }
}

fn feedback_revision() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}
fn ensure_outcome_directory(dir: &Path) -> std::io::Result<()> {
    let metadata = std::fs::symlink_metadata(dir)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(std::io::Error::other(
            "outcomes directory must be a real directory",
        ));
    }
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hrs253_flush_waits_for_full_queue_and_reports_writer_loss() {
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let sink = OutcomeSink {
            sender,
            failed: Default::default(),
        };
        let (first_done, _) = std::sync::mpsc::channel();
        sink.sender.send(OutcomeWrite::Flush(first_done)).unwrap();
        let draining = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(30));
            let _ = receiver.recv().unwrap();
            match receiver
                .recv_timeout(std::time::Duration::from_secs(2))
                .unwrap()
            {
                OutcomeWrite::Flush(done) => done.send(()).unwrap(),
                _ => panic!("expected flush barrier"),
            }
        });
        sink.flush().unwrap();
        draining.join().unwrap();
        assert_eq!(
            sink.flush().unwrap_err().kind(),
            std::io::ErrorKind::BrokenPipe
        );

        let (sender, _blocked) = std::sync::mpsc::sync_channel(1);
        let sink = OutcomeSink {
            sender,
            failed: Default::default(),
        };
        let (done, _) = std::sync::mpsc::channel();
        sink.sender.send(OutcomeWrite::Flush(done)).unwrap();
        assert_eq!(
            sink.flush_until(Instant::now()).unwrap_err().kind(),
            std::io::ErrorKind::TimedOut,
        );
        let mut tracker = OutcomeTracker::new("lost".into(), "/tmp").with_sink(sink);
        tracker.record_user_message();
        assert!(!tracker.finish().evidence_complete);
    }

    #[test]
    fn hrs253_failed_storage_and_queue_overflow_cannot_report_complete_evidence() {
        let temp = tempfile::tempdir().unwrap();
        // A regular file cannot serve as the storage root.
        let root = temp.path().join("blocked");
        std::fs::write(&root, b"file").unwrap();
        let sink = OutcomeSink::new(root);
        let mut tracker = OutcomeTracker::new("write-error".into(), "/tmp").with_sink(sink.clone());
        tracker.record_user_message();
        tracker.persist();
        assert!(sink.flush().is_err());
        assert!(!tracker.finish().evidence_complete);

        let (sender, _blocked) = std::sync::mpsc::sync_channel(1);
        let sink = OutcomeSink {
            sender,
            failed: Default::default(),
        };
        let mut tracker = OutcomeTracker::new("overflow".into(), "/tmp").with_sink(sink);
        tracker.record_user_message();
        tracker.persist();
        tracker.persist();
        tracker.set_outcome(OutcomeKind::Accepted);
        assert!(!tracker.clone().finish().verified_positive());
        assert!(!tracker.finish().evidence_complete);
    }

    #[tokio::test]
    async fn hrs253_flush_barrier_yields_to_current_thread_runtime() {
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let sink = OutcomeSink {
            sender,
            failed: Default::default(),
        };
        let (barrier_tx, barrier_rx) = tokio::sync::oneshot::channel();
        let worker = std::thread::spawn(move || {
            let OutcomeWrite::Flush(done) = receiver.recv().unwrap() else {
                panic!("expected flush");
            };
            barrier_tx.send(done).unwrap();
        });
        let acknowledge = async {
            // The writer needs this runtime task to run before it can finish.
            barrier_rx.await.unwrap().send(()).unwrap();
        };
        tokio::join!(sink.flush_async(), acknowledge);
        worker.join().unwrap();
        assert!(!sink.failed.load(std::sync::atomic::Ordering::Acquire));
    }

    #[tokio::test]
    async fn hrs253_cancelled_scope_drains_interruption_without_blocking_runtime() {
        let temp = tempfile::tempdir().unwrap();
        let sink = OutcomeSink::new(temp.path().into());
        let mut tracker = OutcomeTracker::new("cancelled".into(), "/tmp").with_sink(sink.clone());
        let id = tracker.task_id.clone();
        tracker.record_user_message();
        let (ready, started) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            let _scope = ScopedOutcome(tracker);
            ready.send(()).unwrap();
            std::future::pending::<()>().await;
        });
        started.await.unwrap();
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        sink.flush_async().await;
        let outcome = load_outcome(&id, temp.path()).unwrap().unwrap();
        assert_eq!(outcome.execution, ExecutionStatus::Interrupted);
        assert_eq!(outcome.feedback, UserFeedback::Unknown);
        assert!(outcome.evidence_complete);
    }

    #[test]
    fn hrs253_normal_completion_is_not_user_acceptance() {
        let mut tracker = OutcomeTracker::new("normal-exit".into(), "/tmp/project");
        tracker.record_user_message();
        tracker.record_assistant_turn();
        assert!(!tracker.finish().outcome.is_positive());
    }

    #[test]
    fn outcome_tracker_records_tools() {
        let mut tracker = OutcomeTracker::new("test-1".into(), "/tmp/project");
        assert!(tracker.record_tool(1, "c1", "read_file", true, false));
        assert!(!tracker.record_tool(1, "c1", "read_file", true, false)); // dedup
        assert!(tracker.record_tool(1, "c2", "read_file", false, false));
        assert!(tracker.record_tool(1, "c3", "write_file", true, false));
        tracker.record_undo();
        tracker.record_correction();

        let outcome = tracker.finish();
        assert_eq!(outcome.outcome, OutcomeKind::Unknown);
        assert_eq!(outcome.tools["read_file"].total, 2);
        assert_eq!(outcome.tools["read_file"].succeeded, 1);
        assert_eq!(outcome.tools["read_file"].failed, 1);
        assert_eq!(outcome.tools["write_file"].total, 1);
        assert_eq!(outcome.signals.undo_count, 1);
        assert_eq!(outcome.signals.correction_count, 1);
        assert!(!outcome.workspace_hash.is_empty());
    }

    #[test]
    fn correction_detection() {
        assert!(looks_like_correction("no, that's wrong"));
        assert!(looks_like_correction("actually, try again"));
        assert!(looks_like_correction("this is broken"));
        assert!(!looks_like_correction("great, now add a feature"));
        assert!(!looks_like_correction("read the file"));
    }

    #[test]
    fn tool_summary_failure_rate() {
        let mut ts = ToolSummary::default();
        assert_eq!(ts.failure_rate(), 0.0);
        ts.total = 10;
        ts.failed = 3;
        assert!((ts.failure_rate() - 0.3).abs() < f64::EPSILON);
    }

    #[test]
    fn outcome_serialization_roundtrip() {
        let mut tracker = OutcomeTracker::new("test-2".into(), "/tmp/project");
        tracker.record_tool(1, "c1", "read_file", true, false);
        tracker.set_outcome(OutcomeKind::Accepted);
        let outcome = tracker.finish();
        let json = serde_json::to_string(&outcome).unwrap();
        let parsed: SessionOutcome = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.session_id, "test-2");
        assert_eq!(parsed.outcome, OutcomeKind::Accepted);
        assert_eq!(parsed.version, OUTCOME_VERSION);
    }
}

#[cfg(test)]
mod task_evidence_tests {
    use super::*;

    #[test]
    fn hrs253_host_verification_preserves_scope_and_invalidates_mutations() {
        use crate::mechanistic_delegation::{
            AutomaticVerificationOutcome, AutomaticVerificationStatus,
        };
        let mut host = AutomaticVerificationOutcome {
            status: AutomaticVerificationStatus::Passed,
            context: Some("untrusted diagnostic".into()),
            job_id: Some("job".into()),
            repository_revision: Some("a".repeat(64)),
            receipt_digest: Some("b".repeat(64)),
            selected_paths: vec!["src/example.rs".into()],
            selected_recipes: vec!["cargo-test".into()],
            reason_codes: vec![],
            cache_state: None,
            duration_millis: 10,
        };
        let mut tracker = OutcomeTracker::new("session".into(), "/tmp/project");
        tracker.record_host_verification(&host);
        tracker.record_assistant_turn();
        let checked = tracker.clone().finish();
        assert_eq!(checked.verification, VerificationStatus::Passed);
        assert_eq!(checked.verification_scope, VerificationScope::FocusedChecks);
        assert!(!checked.verified_positive());
        assert!(checked.events[0].excerpt.contains(&"b".repeat(64)));
        assert!(!checked.events[0].excerpt.contains("untrusted diagnostic"));
        tracker.record_tool(0, "read", "read_file", true, false);
        tracker.record_tool(0, "denied", "write_file", false, true);
        assert_eq!(
            tracker.clone().finish().verification,
            VerificationStatus::Passed
        );
        // A shell may mutate even when it exits with an error.
        tracker.record_tool(0, "shell", "run_command", false, false);
        assert_eq!(
            tracker.clone().finish().verification,
            VerificationStatus::Stale
        );
        assert_eq!(
            tracker
                .events
                .iter()
                .filter(|e| e.kind == "verification_invalidated")
                .count(),
            1
        );
        tracker.record_host_verification(&host);
        tracker.record_tool(0, "hashline", "hashline_edit", true, false);
        assert_eq!(
            tracker.clone().finish().verification,
            VerificationStatus::Stale
        );
        tracker.record_host_verification(&host);
        tracker.record_undo();
        assert_eq!(
            tracker.clone().finish().verification,
            VerificationStatus::Stale
        );
        host.status = AutomaticVerificationStatus::Unchanged;
        tracker.record_host_verification(&host);
        assert_eq!(
            tracker.clone().finish().verification,
            VerificationStatus::Stale
        );
        host.status = AutomaticVerificationStatus::Passed;
        host.reason_codes = vec!["stale_revision".into()];
        tracker.record_host_verification(&host);
        assert_eq!(
            tracker.clone().finish().verification,
            VerificationStatus::Stale
        );
        host.reason_codes.clear();
        host.receipt_digest = None;
        tracker.record_host_verification(&host);
        assert_eq!(
            tracker.clone().finish().verification,
            VerificationStatus::Unverified
        );
        host.status = AutomaticVerificationStatus::Failed;
        tracker.record_host_verification(&host);
        assert_eq!(
            tracker.clone().finish().verification,
            VerificationStatus::Failed
        );
        tracker.record_model_verification(true, "I checked everything");
        assert_eq!(
            tracker.clone().finish().verification,
            VerificationStatus::Failed
        );
        assert!(!tracker.finish().verified_positive());
    }

    #[test]
    fn hrs253_legacy_verification_without_scope_cannot_claim_task_success() {
        let mut tracker = OutcomeTracker::new("session".into(), "/tmp/project");
        tracker.record_assistant_turn();
        tracker.verification = VerificationStatus::Passed;
        let mut json = serde_json::to_value(tracker.finish()).unwrap();
        json.as_object_mut().unwrap().remove("verification_scope");
        let outcome: SessionOutcome = serde_json::from_value(json).unwrap();
        assert_eq!(outcome.verification_scope, VerificationScope::Unknown);
        assert!(!outcome.verified_positive());
    }

    #[test]
    fn hrs253_tasks_epochs_and_feedback_are_separate() {
        let mut tracker = OutcomeTracker::new("same-session".into(), "/tmp/project");
        tracker.record_user_message();
        tracker.set_harness(HarnessSnapshot::default());
        tracker.set_harness(HarnessSnapshot::default());
        let first = tracker.clone().finish();
        assert_eq!(first.harness_epochs.len(), 1);
        tracker.start_task("same-session".into(), "/tmp/project");
        tracker.record_assistant_turn();
        let second = tracker.finish();
        assert_ne!(first.task_id, second.task_id);
        assert!(!second.verified_positive());
        let root = tempfile::tempdir().unwrap();
        save_outcome(&second, root.path()).unwrap();
        set_feedback(&second.task_id, UserFeedback::Accepted, root.path()).unwrap();
        assert!(load_outcome(&second.task_id, root.path())
            .unwrap()
            .unwrap()
            .verified_positive());
        save_outcome(&second, root.path()).unwrap();
        assert!(load_outcome(&second.task_id, root.path())
            .unwrap()
            .unwrap()
            .verified_positive());
        set_feedback(&second.task_id, UserFeedback::Unknown, root.path()).unwrap();
        assert!(!load_outcome(&second.task_id, root.path())
            .unwrap()
            .unwrap()
            .verified_positive());
        save_outcome(&second, root.path()).unwrap();
        assert_eq!(
            load_outcome(&second.task_id, root.path())
                .unwrap()
                .unwrap()
                .feedback,
            UserFeedback::Unknown
        );
        assert!(load_outcome("../outside", root.path()).is_err());
    }
    #[test]
    fn hrs253_feedback_updates_do_not_reorder_task_history() {
        let root = tempfile::tempdir().unwrap();
        let mut old = OutcomeTracker::new("old".into(), "/tmp/project").finish();
        old.started_at = 1;
        let mut new = OutcomeTracker::new("new".into(), "/tmp/project").finish();
        new.started_at = 2;
        save_outcome(&new, root.path()).unwrap();
        save_outcome(&old, root.path()).unwrap();
        assert_eq!(
            recent_outcomes(1, root.path()).unwrap()[0].task_id,
            new.task_id
        );
    }
    #[test]
    fn hrs253_edits_invalidate_verification_and_denials_are_not_failures() {
        let mut tracker = OutcomeTracker::new("session".into(), "/tmp/project");
        tracker.verification = VerificationStatus::Passed;
        tracker.invalidate_verification();
        tracker.record_tool(1, "call", "shell", false, true);
        assert!(!tracker.record_tool(1, "call", "shell", false, true));
        let outcome = tracker.finish();
        assert_eq!(outcome.verification, VerificationStatus::Stale);
        assert_eq!(outcome.tools["shell"].failed, 0);
        assert_eq!(outcome.tools["shell"].denied, 1);
    }
}
