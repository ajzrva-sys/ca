use crate::config::HookConfig;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};

const MAX_ADDITIONAL_CONTEXT: usize = 10_000;
const MAX_HOOK_PAYLOAD_BYTES: usize = 64 * 1024;
const MAX_HOOK_OUTPUT_BYTES: usize = 64 * 1024;
const MAX_DENIAL_BYTES: usize = 1_024;
const MAX_PENDING_OWNERS: usize = 128;
const MAX_OWNER_CONTEXT_BYTES: usize = 64 * 1024;

/// Exact authority for a hook invocation and its retained continuation effects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub struct HookOwner {
    pub session_id: Option<uuid::Uuid>,
    pub pane_id: u8,
    pub turn_id: Option<uuid::Uuid>,
    pub generation: u64,
    pub child_id: Option<uuid::Uuid>,
}

impl HookOwner {
    pub fn for_child(self, child_id: uuid::Uuid, parent_turn: uuid::Uuid) -> Self {
        Self {
            turn_id: Some(parent_turn),
            child_id: Some(child_id),
            ..self
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HookInvocation {
    pub owner: HookOwner,
    pub id: uuid::Uuid,
}

impl HookInvocation {
    pub fn new(owner: HookOwner) -> Self {
        Self {
            owner,
            id: uuid::Uuid::new_v4(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct HookScope {
    owner: HookOwner,
    invocation: Option<uuid::Uuid>,
}

#[derive(Default)]
struct OwnerOutcome {
    contexts: Vec<String>,
    blocking_failure: Option<(HookEvent, String)>,
}

impl OwnerOutcome {
    fn is_empty(&self) -> bool {
        self.contexts.is_empty() && self.blocking_failure.is_none()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(dead_code)]
pub enum HookEvent {
    SessionStart,
    BeforeSubmitPrompt,
    PreToolUse,
    PostToolUse,
    PostToolUseFailure,
    AfterAgentThought,
    AfterAgentResponse,
    PreCompact,
    PostCompact,
    SubagentStart,
    SubagentStop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookSafeStatus {
    Running,
    Succeeded,
    Denied,
    Failed,
    TimedOut,
    Cancelled,
}

impl HookSafeStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Running => "testing",
            Self::Succeeded => "working",
            Self::Denied => "denied",
            Self::Failed => "failed",
            Self::TimedOut => "timed out",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookSafeResult {
    pub status: HookSafeStatus,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookManagementEntry {
    pub index: usize,
    pub event: HookEvent,
    pub enabled: bool,
    pub timeout_secs: u64,
    pub fail_closed: bool,
    pub last_result: Option<HookSafeResult>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookManagementSnapshot {
    pub entries: Vec<HookManagementEntry>,
    pub network_enforcement: String,
}

#[derive(Clone, Default, PartialEq, Eq)]
pub struct HookBatchResult {
    pub denied: Option<String>,
    pub contexts: Vec<String>,
    pub fail_closed: bool,
}

#[derive(Clone)]
pub struct HookRunner {
    inner: Arc<HookRunnerInner>,
    owner: HookOwner,
    invocation: Option<uuid::Uuid>,
}

struct HookRunnerInner {
    hooks: RwLock<Vec<HookConfig>>,
    results: Arc<Mutex<HashMap<(usize, HookEvent), HookSafeResult>>>,
    owner_outcomes: Mutex<HashMap<HookScope, OwnerOutcome>>,
    invocation_gate: tokio::sync::Mutex<()>,
    sandbox_mode: crate::sandbox::SandboxMode,
    runtime_enabled: bool,
}

impl std::fmt::Debug for HookRunner {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HookRunner")
            .field("hooks", &"[REDACTED]")
            .field("sandbox_mode", &self.inner.sandbox_mode)
            .finish()
    }
}

impl HookRunner {
    #[cfg(test)]
    pub fn new(hooks: Vec<HookConfig>, sandbox_mode: crate::sandbox::SandboxMode) -> Self {
        Self::new_with_runtime(hooks, sandbox_mode, true)
    }

    pub fn new_with_runtime(
        hooks: Vec<HookConfig>,
        sandbox_mode: crate::sandbox::SandboxMode,
        runtime_enabled: bool,
    ) -> Self {
        Self {
            invocation: None,
            owner: HookOwner {
                session_id: None,
                pane_id: 0,
                turn_id: Some(uuid::Uuid::new_v4()),
                generation: 0,
                child_id: None,
            },
            inner: Arc::new(HookRunnerInner {
                hooks: RwLock::new(hooks),
                results: Arc::new(Mutex::new(HashMap::new())),
                owner_outcomes: Mutex::new(HashMap::new()),
                invocation_gate: tokio::sync::Mutex::new(()),
                sandbox_mode,
                runtime_enabled,
            }),
        }
    }

    /// Capture an immutable invocation owner while sharing configuration and safe status.
    pub fn for_owner(&self, owner: HookOwner) -> Self {
        Self {
            inner: self.inner.clone(),
            owner,
            invocation: None,
        }
    }

    pub fn for_invocation(&self, invocation: HookInvocation) -> Self {
        Self {
            inner: self.inner.clone(),
            owner: invocation.owner,
            invocation: Some(invocation.id),
        }
    }

    fn scope(&self, owner: &HookOwner) -> HookScope {
        HookScope {
            owner: *owner,
            invocation: self.invocation,
        }
    }

    pub fn invocation_id(&self) -> Option<uuid::Uuid> {
        self.invocation
    }

    pub fn owner(&self) -> HookOwner {
        self.owner
    }

    pub fn reload(&self, hooks: Vec<HookConfig>) {
        let previous = self
            .inner
            .hooks
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .enumerate()
            .filter_map(|(index, hook)| {
                HookEvent::parse(&hook.event).map(|event| ((index, event), hook_fingerprint(hook)))
            })
            .collect::<HashMap<_, _>>();
        let valid_keys = hooks
            .iter()
            .enumerate()
            .filter_map(|(index, hook)| {
                HookEvent::parse(&hook.event).and_then(|event| {
                    let key = (index, event);
                    (previous.get(&key) == Some(&hook_fingerprint(hook))).then_some(key)
                })
            })
            .collect::<std::collections::HashSet<_>>();
        self.inner
            .results
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .retain(|key, _| valid_keys.contains(key));
        *self
            .inner
            .hooks
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = hooks;
    }

    pub fn snapshot(&self) -> HookManagementSnapshot {
        let hooks = self
            .inner
            .hooks
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let results = self
            .inner
            .results
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let entries = hooks
            .iter()
            .enumerate()
            .filter_map(|(index, hook)| {
                let event = HookEvent::parse(&hook.event)?;
                Some(HookManagementEntry {
                    index,
                    event,
                    enabled: hook.enabled,
                    timeout_secs: hook.timeout_secs,
                    fail_closed: hook.fail_closed,
                    last_result: results.get(&(index, event)).cloned(),
                })
            })
            .collect();
        HookManagementSnapshot {
            entries,
            network_enforcement: hook_network_enforcement(self.inner.sandbox_mode).into(),
        }
    }

    pub fn has_enabled(&self, event: HookEvent) -> bool {
        self.inner.runtime_enabled
            && self
                .inner
                .hooks
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .iter()
                .any(|hook| hook.enabled && HookEvent::parse(&hook.event) == Some(event))
    }

    pub fn take_contexts(&self, owner: &HookOwner) -> Vec<String> {
        let scope = self.scope(owner);
        let mut outcomes = self
            .inner
            .owner_outcomes
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let contexts = outcomes
            .get_mut(&scope)
            .map(|pending| std::mem::take(&mut pending.contexts))
            .unwrap_or_default();
        if outcomes.get(&scope).is_some_and(OwnerOutcome::is_empty) {
            outcomes.remove(&scope);
        }
        contexts
    }

    pub fn take_blocking_failure(&self, owner: &HookOwner) -> Option<(HookEvent, String)> {
        let scope = self.scope(owner);
        let mut outcomes = self
            .inner
            .owner_outcomes
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let failure = outcomes
            .get_mut(&scope)
            .and_then(|pending| pending.blocking_failure.take());
        if outcomes.get(&scope).is_some_and(OwnerOutcome::is_empty) {
            outcomes.remove(&scope);
        }
        failure
    }

    pub fn discard_owner(&self, owner: &HookOwner) {
        self.inner
            .owner_outcomes
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&self.scope(owner));
    }

    pub async fn run_event(
        &self,
        owner: &HookOwner,
        event: HookEvent,
        payload: &HookPayload,
        roots: &[PathBuf],
    ) -> HookBatchResult {
        if !self.inner.runtime_enabled {
            return HookBatchResult::default();
        }
        let _invocation = self.inner.invocation_gate.lock().await;
        let payload = payload.with_owner(owner);
        let hooks = self
            .inner
            .hooks
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .enumerate()
            .filter(|(_, hook)| hook.enabled && HookEvent::parse(&hook.event) == Some(event))
            .map(|(index, hook)| (index, hook.clone()))
            .collect::<Vec<_>>();
        if hooks.is_empty() {
            return HookBatchResult::default();
        }
        {
            let outcomes = self
                .inner
                .owner_outcomes
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            if !outcomes.contains_key(&self.scope(owner)) && outcomes.len() >= MAX_PENDING_OWNERS {
                return HookBatchResult {
                    denied: Some("Hook owner retention limit reached".into()),
                    fail_closed: true,
                    ..Default::default()
                };
            }
        }
        let mut batch = HookBatchResult::default();
        for (index, hook) in hooks {
            let outcome = execute_single_hook_async(
                index,
                event,
                &hook,
                &payload,
                roots,
                self.inner.sandbox_mode,
                self.inner.results.clone(),
            )
            .await;
            match outcome {
                HookOutcome::Allowed { context } => {
                    if let Some(context) = context {
                        batch.contexts.push(truncate_context(&context));
                    }
                }
                HookOutcome::Denied { reason, context } => {
                    if batch.denied.is_none() {
                        batch.denied = Some(reason);
                    }
                    if let Some(context) = context {
                        batch.contexts.push(truncate_context(&context));
                    }
                }
                HookOutcome::Failed { fail_closed, .. } => {
                    if fail_closed && batch.denied.is_none() {
                        batch.denied = Some("Hook failed (fail-closed)".into());
                        batch.fail_closed = true;
                    }
                }
                HookOutcome::Timeout { fail_closed } => {
                    if fail_closed && batch.denied.is_none() {
                        batch.denied = Some("Hook timed out (fail-closed)".into());
                        batch.fail_closed = true;
                    }
                }
            }
        }
        let blocks_continuation = batch.fail_closed
            || (batch.denied.is_some()
                && matches!(
                    event,
                    HookEvent::PostToolUse | HookEvent::PostToolUseFailure
                ));
        let failure = (blocks_continuation
            && matches!(
                event,
                HookEvent::PreToolUse | HookEvent::PostToolUse | HookEvent::PostToolUseFailure
            ))
        .then(|| {
            (
                event,
                batch
                    .denied
                    .clone()
                    .unwrap_or_else(|| "Hook failed (fail-closed)".into()),
            )
        });
        if !batch.contexts.is_empty() || failure.is_some() {
            let mut outcomes = self
                .inner
                .owner_outcomes
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            let pending = outcomes.entry(self.scope(owner)).or_default();
            let mut remaining = MAX_OWNER_CONTEXT_BYTES
                .saturating_sub(pending.contexts.iter().map(String::len).sum::<usize>());
            for context in &batch.contexts {
                if remaining == 0 {
                    break;
                }
                let mut end = remaining.min(context.len());
                while !context.is_char_boundary(end) {
                    end -= 1;
                }
                if end > 0 {
                    pending.contexts.push(context[..end].to_string());
                    remaining -= end;
                }
            }
            if failure.is_some() {
                pending.blocking_failure = failure;
            }
        }
        batch
    }

    pub async fn test_hook(
        &self,
        owner: &HookOwner,
        index: usize,
        expected_event: HookEvent,
        payload: &HookPayload,
        roots: &[PathBuf],
    ) -> Result<HookBatchResult, String> {
        if !self.inner.runtime_enabled {
            return Err("Hooks are disabled in the bare profile".into());
        }
        let _invocation = self.inner.invocation_gate.lock().await;
        let hook = self
            .inner
            .hooks
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(index)
            .cloned()
            .ok_or_else(|| "Hook entry changed; reload /hooks".to_string())?;
        if HookEvent::parse(&hook.event) != Some(expected_event) {
            return Err("Hook entry changed; reload /hooks".into());
        }
        let payload = payload.with_owner(owner);
        let outcome = execute_single_hook_async(
            index,
            expected_event,
            &hook,
            &payload,
            roots,
            self.inner.sandbox_mode,
            self.inner.results.clone(),
        )
        .await;
        let mut batch = HookBatchResult::default();
        match outcome {
            HookOutcome::Allowed { context } => {
                if let Some(context) = context {
                    batch.contexts.push(truncate_context(&context));
                }
            }
            HookOutcome::Denied { reason, context } => {
                batch.denied = Some(reason);
                if let Some(context) = context {
                    batch.contexts.push(truncate_context(&context));
                }
            }
            HookOutcome::Failed { fail_closed, .. } => {
                batch.fail_closed = fail_closed;
                if fail_closed {
                    batch.denied = Some("Hook failed (fail-closed)".into());
                }
            }
            HookOutcome::Timeout { fail_closed } => {
                batch.fail_closed = fail_closed;
                if fail_closed {
                    batch.denied = Some("Hook timed out (fail-closed)".into());
                }
            }
        }
        Ok(batch)
    }
}

fn hook_fingerprint(hook: &HookConfig) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    hook.event.hash(&mut hasher);
    hook.command.hash(&mut hasher);
    hook.enabled.hash(&mut hasher);
    hook.timeout_secs.hash(&mut hasher);
    hook.fail_closed.hash(&mut hasher);
    hasher.finish()
}

impl HookEvent {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::SessionStart => "session_start",
            Self::BeforeSubmitPrompt => "before_submit_prompt",
            Self::PreToolUse => "pre_tool_use",
            Self::PostToolUse => "post_tool_use",
            Self::PostToolUseFailure => "post_tool_use_failure",
            Self::AfterAgentThought => "after_agent_thought",
            Self::AfterAgentResponse => "after_agent_response",
            Self::PreCompact => "pre_compact",
            Self::PostCompact => "post_compact",
            Self::SubagentStart => "subagent_start",
            Self::SubagentStop => "subagent_stop",
        }
    }

    #[allow(dead_code)]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "session_start" => Some(Self::SessionStart),
            "before_submit_prompt" => Some(Self::BeforeSubmitPrompt),
            "pre_tool_use" => Some(Self::PreToolUse),
            "post_tool_use" => Some(Self::PostToolUse),
            "post_tool_use_failure" => Some(Self::PostToolUseFailure),
            "after_agent_thought" => Some(Self::AfterAgentThought),
            "after_agent_response" => Some(Self::AfterAgentResponse),
            "pre_compact" => Some(Self::PreCompact),
            "post_compact" => Some(Self::PostCompact),
            "subagent_start" => Some(Self::SubagentStart),
            "subagent_stop" => Some(Self::SubagentStop),
            _ => None,
        }
    }
}

#[derive(Clone, Serialize)]
pub struct HookPayload {
    pub hook_event_name: String,
    pub session_id: Option<String>,
    pub cwd: String,
    #[serde(flatten)]
    pub extra: serde_json::Value,
}

impl HookPayload {
    fn with_owner(&self, owner: &HookOwner) -> Self {
        let mut payload = self.clone();
        payload.session_id = owner.session_id.map(|id| id.to_string());
        let mut extra = payload.extra.as_object().cloned().unwrap_or_default();
        for reserved in ["session_id", "hook_event_name", "cwd"] {
            extra.remove(reserved);
        }
        extra.insert(
            "hook_owner".into(),
            serde_json::to_value(owner).expect("hook owner serializes"),
        );
        payload.extra = serde_json::Value::Object(extra);
        payload
    }

    /// Metadata-only lifecycle payload. No prompt, transcript, tool input, or output.
    pub fn lifecycle(
        event: HookEvent,
        owner: &HookOwner,
        cwd: &std::path::Path,
        model: Option<&str>,
        trigger: &str,
        status: &str,
    ) -> Self {
        let bounded = |value: &str| {
            crate::terminal_text::sanitize_terminal_text(value)
                .chars()
                .take(256)
                .collect::<String>()
        };
        Self {
            hook_event_name: event.as_str().into(),
            session_id: owner.session_id.map(|id| id.to_string()),
            cwd: cwd.to_string_lossy().into_owned(),
            extra: serde_json::json!({
                "schema": "ca.hook_lifecycle.v1", "event_id": uuid::Uuid::new_v4(),
                "model": model.map(bounded), "trigger": bounded(trigger), "status": bounded(status)
            }),
        }
    }
}

#[derive(Clone, Deserialize, Default)]
pub struct HookResponse {
    pub permission: Option<String>,
    pub user_message: Option<String>,
    pub additional_context: Option<String>,
    #[allow(dead_code)]
    pub updated_input: Option<serde_json::Value>,
}

#[derive(Clone)]
pub enum HookOutcome {
    Allowed {
        context: Option<String>,
    },
    Denied {
        reason: String,
        context: Option<String>,
    },
    Failed {
        fail_closed: bool,
    },
    Timeout {
        fail_closed: bool,
    },
}

async fn execute_single_hook_async(
    index: usize,
    event: HookEvent,
    config: &HookConfig,
    payload: &HookPayload,
    roots: &[PathBuf],
    sandbox_mode: crate::sandbox::SandboxMode,
    results: Arc<Mutex<HashMap<(usize, HookEvent), HookSafeResult>>>,
) -> HookOutcome {
    let key = (index, event);
    set_safe_result(
        &results,
        key,
        HookSafeResult {
            status: HookSafeStatus::Running,
            duration_ms: 0,
        },
    );
    let started = std::time::Instant::now();
    let payload_json = match serde_json::to_vec(payload) {
        Ok(payload) if payload.len() <= MAX_HOOK_PAYLOAD_BYTES => payload,
        Ok(_) | Err(_) => {
            finish_without_process(&results, key, HookSafeStatus::Failed, started);
            return HookOutcome::Failed {
                fail_closed: config.fail_closed,
            };
        }
    };

    let temporary_directory =
        std::env::temp_dir().join(format!("ca-hook-{}", uuid::Uuid::new_v4()));
    if std::fs::create_dir(&temporary_directory).is_err() {
        finish_without_process(&results, key, HookSafeStatus::Failed, started);
        return HookOutcome::Failed {
            fail_closed: config.fail_closed,
        };
    }
    let mut sandbox_roots = roots.to_vec();
    let cwd = PathBuf::from(&payload.cwd);
    if !sandbox_roots.contains(&cwd) {
        sandbox_roots.push(cwd.clone());
    }
    let prepared = match crate::sandbox::SandboxBackend::new(sandbox_mode).prepare_shell(
        &config.command,
        &crate::sandbox::SandboxPolicy {
            roots: sandbox_roots,
            read_only_roots: Vec::new(),
            temporary_directory: temporary_directory.clone(),
            network_allowed: false,
            hide_other_processes: false,
        },
    ) {
        Ok(prepared) => prepared,
        Err(_) => {
            let _ = std::fs::remove_dir(&temporary_directory);
            finish_without_process(&results, key, HookSafeStatus::Failed, started);
            return HookOutcome::Failed {
                fail_closed: config.fail_closed,
            };
        }
    };

    let mut command = tokio::process::Command::new(&prepared.program);
    command
        .args(&prepared.arguments)
        .current_dir(&cwd)
        .env_clear()
        .envs(hook_environment())
        .env("TMPDIR", &temporary_directory)
        .env("CA_HOOK_EVENT", event.as_str())
        .env("CA_PROJECT_DIR", &payload.cwd)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    if let Some(session_id) = &payload.session_id {
        command.env("CA_SESSION_ID", session_id);
    }
    crate::run_environment::apply_tokio(&mut command);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.as_std_mut().process_group(0);
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(_) => {
            let _ = std::fs::remove_dir(&temporary_directory);
            finish_without_process(&results, key, HookSafeStatus::Failed, started);
            return HookOutcome::Failed {
                fail_closed: config.fail_closed,
            };
        }
    };
    let pid = child.id();
    let stdin = child.stdin.take();
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let mut guard = HookProcessGuard::new(pid, temporary_directory, results.clone(), key, started);

    let writer = tokio::spawn(async move {
        let Some(mut stdin) = stdin else {
            return;
        };
        let _ = stdin.write_all(&payload_json).await;
        let _ = stdin.shutdown().await;
    });
    let stdout_reader = tokio::spawn(read_bounded(stdout, MAX_HOOK_OUTPUT_BYTES));
    let stderr_reader = tokio::spawn(read_bounded(stderr, MAX_HOOK_OUTPUT_BYTES));
    let timeout = tokio::time::sleep(Duration::from_secs(config.timeout_secs));
    tokio::pin!(timeout);
    let status = tokio::select! {
        status = child.wait() => status.ok(),
        () = &mut timeout => {
            guard.terminate();
            let _ = child.start_kill();
            let _ = child.wait().await;
            None
        }
    };
    // A hook command may exit after leaving descendants that inherited its pipes. Always
    // terminate the dedicated process group before awaiting bounded readers so those
    // descendants cannot keep the worker alive past the hook boundary.
    if status.is_some() {
        guard.terminate();
    }
    let _ = writer.await;
    let stdout = stdout_reader
        .await
        .ok()
        .and_then(Result::ok)
        .unwrap_or_default();
    let _stderr = stderr_reader.await;

    let Some(status) = status else {
        guard.complete(HookSafeStatus::TimedOut);
        return HookOutcome::Timeout {
            fail_closed: config.fail_closed,
        };
    };
    if !status.success() {
        guard.complete(HookSafeStatus::Failed);
        return HookOutcome::Failed {
            fail_closed: config.fail_closed,
        };
    }
    if stdout.1 {
        guard.complete(HookSafeStatus::Failed);
        return HookOutcome::Failed {
            fail_closed: config.fail_closed,
        };
    }
    let stdout = match String::from_utf8(stdout.0) {
        Ok(stdout) => stdout,
        Err(_) => {
            guard.complete(HookSafeStatus::Failed);
            return HookOutcome::Failed {
                fail_closed: config.fail_closed,
            };
        }
    };
    let stdout = stdout.trim();
    if stdout.is_empty() {
        guard.complete(HookSafeStatus::Succeeded);
        return HookOutcome::Allowed { context: None };
    }
    let response: HookResponse = match serde_json::from_str(stdout) {
        Ok(response) => response,
        Err(_) if !stdout.starts_with('{') && !stdout.starts_with('[') => {
            guard.complete(HookSafeStatus::Succeeded);
            return HookOutcome::Allowed { context: None };
        }
        Err(_) => {
            guard.complete(HookSafeStatus::Failed);
            return HookOutcome::Failed {
                fail_closed: config.fail_closed,
            };
        }
    };
    let context = response
        .additional_context
        .map(|context| truncate_context(&crate::terminal_text::sanitize_terminal_text(&context)));
    if response.permission.as_deref() == Some("deny") {
        guard.complete(HookSafeStatus::Denied);
        HookOutcome::Denied {
            reason: bounded_safe_text(
                response
                    .user_message
                    .as_deref()
                    .unwrap_or("Blocked by hook"),
                MAX_DENIAL_BYTES,
            ),
            context,
        }
    } else {
        guard.complete(HookSafeStatus::Succeeded);
        HookOutcome::Allowed { context }
    }
}

fn hook_environment() -> impl Iterator<Item = (String, String)> {
    ["PATH", "HOME", "LANG", "LC_ALL", "TERM"]
        .into_iter()
        .filter_map(|name| crate::run_environment::var(name).map(|value| (name.to_string(), value)))
}

fn hook_network_enforcement(mode: crate::sandbox::SandboxMode) -> &'static str {
    match mode {
        crate::sandbox::SandboxMode::Required => "network denied by OS sandbox",
        crate::sandbox::SandboxMode::PolicyOnly => "network denied by policy only",
        crate::sandbox::SandboxMode::Off => "network denial is not enforced in sandbox-off mode",
    }
}

async fn read_bounded<R>(reader: Option<R>, limit: usize) -> std::io::Result<(Vec<u8>, bool)>
where
    R: AsyncRead + Unpin,
{
    let Some(reader) = reader else {
        return Ok((Vec::new(), false));
    };
    let mut bytes = Vec::new();
    reader
        .take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .await?;
    let oversized = bytes.len() > limit;
    bytes.truncate(limit);
    Ok((bytes, oversized))
}

fn set_safe_result(
    results: &Arc<Mutex<HashMap<(usize, HookEvent), HookSafeResult>>>,
    key: (usize, HookEvent),
    result: HookSafeResult,
) {
    results
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(key, result);
}

fn finish_without_process(
    results: &Arc<Mutex<HashMap<(usize, HookEvent), HookSafeResult>>>,
    key: (usize, HookEvent),
    status: HookSafeStatus,
    started: std::time::Instant,
) {
    set_safe_result(
        results,
        key,
        HookSafeResult {
            status,
            duration_ms: duration_millis(started.elapsed()),
        },
    );
}

struct HookProcessGuard {
    pid: Option<u32>,
    temporary_directory: PathBuf,
    results: Arc<Mutex<HashMap<(usize, HookEvent), HookSafeResult>>>,
    key: (usize, HookEvent),
    started: std::time::Instant,
    armed: bool,
}

impl HookProcessGuard {
    fn new(
        pid: Option<u32>,
        temporary_directory: PathBuf,
        results: Arc<Mutex<HashMap<(usize, HookEvent), HookSafeResult>>>,
        key: (usize, HookEvent),
        started: std::time::Instant,
    ) -> Self {
        Self {
            pid,
            temporary_directory,
            results,
            key,
            started,
            armed: true,
        }
    }

    fn terminate(&self) {
        #[cfg(unix)]
        if let Some(pid) = self.pid {
            let _ = nix::sys::signal::killpg(
                nix::unistd::Pid::from_raw(pid as i32),
                nix::sys::signal::Signal::SIGKILL,
            );
        }
    }

    fn complete(&mut self, status: HookSafeStatus) {
        self.armed = false;
        set_safe_result(
            &self.results,
            self.key,
            HookSafeResult {
                status,
                duration_ms: duration_millis(self.started.elapsed()),
            },
        );
        let _ = std::fs::remove_dir_all(&self.temporary_directory);
    }
}

impl Drop for HookProcessGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        self.terminate();
        set_safe_result(
            &self.results,
            self.key,
            HookSafeResult {
                status: HookSafeStatus::Cancelled,
                duration_ms: duration_millis(self.started.elapsed()),
            },
        );
        let _ = std::fs::remove_dir_all(&self.temporary_directory);
    }
}

fn duration_millis(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}

fn bounded_safe_text(text: &str, max_bytes: usize) -> String {
    let sanitized = crate::terminal_text::sanitize_terminal_text(text);
    if sanitized.len() <= max_bytes {
        return sanitized;
    }
    let mut end = max_bytes.min(sanitized.len());
    while end > 0 && !sanitized.is_char_boundary(end) {
        end -= 1;
    }
    sanitized[..end].to_string()
}

fn truncate_context(context: &str) -> String {
    bounded_safe_text(context, MAX_ADDITIONAL_CONTEXT)
}

#[cfg(test)]
fn run_hooks(
    event: HookEvent,
    payload: &HookPayload,
    hooks_config: &[HookConfig],
    _session_id: Option<&str>,
) -> (Option<String>, Vec<String>) {
    let runner = HookRunner::new(
        hooks_config.to_vec(),
        crate::sandbox::SandboxMode::PolicyOnly,
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let batch = runtime.block_on(runner.run_event(&test_owner(), event, payload, &[]));
    (batch.denied, batch.contexts)
}

#[cfg(test)]
fn test_owner() -> HookOwner {
    HookOwner {
        session_id: Some(uuid::Uuid::from_u128(1)),
        pane_id: 1,
        turn_id: Some(uuid::Uuid::from_u128(2)),
        generation: 1,
        child_id: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn owner_scopes_do_not_drain_other_panes_turns_generations_or_children() {
        let runner = HookRunner::new(vec![HookConfig {
            event: "post_tool_use".into(),
            command: r#"printf '%s' '{"permission":"deny","user_message":"blocked","additional_context":"private"}'"#.into(),
            enabled: true, timeout_secs: 5, fail_closed: false,
        }], crate::sandbox::SandboxMode::PolicyOnly);
        let base = test_owner();
        let owners = [
            base,
            HookOwner { pane_id: 2, ..base },
            HookOwner {
                session_id: Some(uuid::Uuid::new_v4()),
                ..base
            },
            HookOwner {
                turn_id: Some(uuid::Uuid::new_v4()),
                ..base
            },
            HookOwner {
                generation: 2,
                ..base
            },
            base.for_child(uuid::Uuid::new_v4(), base.turn_id.unwrap()),
        ];
        for owner in &owners {
            runner
                .run_event(
                    owner,
                    HookEvent::PostToolUse,
                    &async_payload(HookEvent::PostToolUse),
                    &[],
                )
                .await;
        }
        let unrelated = HookOwner {
            generation: 3,
            ..base
        };
        assert!(runner.take_contexts(&unrelated).is_empty());
        assert!(runner.take_blocking_failure(&unrelated).is_none());
        for owner in &owners {
            assert_eq!(runner.take_contexts(owner), ["private"]);
            assert_eq!(
                runner.take_blocking_failure(owner),
                Some((HookEvent::PostToolUse, "blocked".into()))
            );
            assert!(runner.take_contexts(owner).is_empty());
        }
        assert!(runner.inner.owner_outcomes.lock().unwrap().is_empty());
    }

    #[test]
    fn bound_clones_keep_owner_and_lifecycle_metadata_is_bounded_authoritative() {
        let runner = HookRunner::new(Vec::new(), crate::sandbox::SandboxMode::PolicyOnly);
        let owner = test_owner();
        let bound = runner.for_owner(owner);
        assert_eq!(bound.clone().owner(), owner);
        assert_ne!(
            runner.owner(),
            HookRunner::new(Vec::new(), crate::sandbox::SandboxMode::PolicyOnly).owner()
        );
        let payload = HookPayload::lifecycle(
            HookEvent::PreCompact,
            &owner,
            std::path::Path::new("/tmp"),
            Some(&"m".repeat(1000)),
            "manual",
            "started",
        );
        let value = serde_json::to_value(payload.with_owner(&owner)).unwrap();
        assert_eq!(value["hook_owner"]["pane_id"], owner.pane_id);
        assert_eq!(value["model"].as_str().unwrap().len(), 256);
        assert!(value.get("prompt").is_none());
        assert!(value.get("transcript").is_none());
    }

    #[tokio::test]
    async fn discarding_retired_owner_preserves_other_outcomes() {
        let runner = HookRunner::new(Vec::new(), crate::sandbox::SandboxMode::PolicyOnly);
        let old = test_owner();
        let current = HookOwner {
            generation: old.generation + 1,
            ..old
        };
        for owner in [old, current] {
            runner.inner.owner_outcomes.lock().unwrap().insert(
                runner.scope(&owner),
                OwnerOutcome {
                    contexts: vec!["context".into()],
                    ..Default::default()
                },
            );
        }
        runner.discard_owner(&old);
        assert!(runner.take_contexts(&old).is_empty());
        assert_eq!(runner.take_contexts(&current), ["context"]);
    }

    #[test]
    fn hook_event_parse_round_trips() {
        for event in &[
            HookEvent::SessionStart,
            HookEvent::BeforeSubmitPrompt,
            HookEvent::PreToolUse,
            HookEvent::PostToolUse,
            HookEvent::PostToolUseFailure,
            HookEvent::AfterAgentThought,
            HookEvent::AfterAgentResponse,
            HookEvent::PreCompact,
            HookEvent::PostCompact,
            HookEvent::SubagentStart,
            HookEvent::SubagentStop,
        ] {
            let s = event.as_str();
            assert_eq!(HookEvent::parse(s), Some(*event));
        }
        assert_eq!(HookEvent::parse("unknown"), None);
    }

    #[test]
    fn run_hooks_with_no_matching_hooks_returns_empty() {
        let payload = HookPayload {
            hook_event_name: "pre_tool_use".into(),
            session_id: None,
            cwd: "/tmp".into(),
            extra: serde_json::json!({}),
        };
        let (deny, ctxs) = run_hooks(HookEvent::PreToolUse, &payload, &[], None);
        assert!(deny.is_none());
        assert!(ctxs.is_empty());
    }

    #[test]
    fn run_hooks_allows_when_script_exits_zero() {
        let hook = HookConfig {
            event: "pre_tool_use".into(),
            command: "cat > /dev/null".into(),
            enabled: true,
            timeout_secs: 5,
            fail_closed: false,
        };
        let payload = HookPayload {
            hook_event_name: "pre_tool_use".into(),
            session_id: None,
            cwd: "/tmp".into(),
            extra: serde_json::json!({"tool_name": "read_file"}),
        };
        let (deny, ctxs) = run_hooks(HookEvent::PreToolUse, &payload, &[hook], None);
        assert!(deny.is_none());
        assert!(ctxs.is_empty());
    }

    #[test]
    fn run_hooks_denies_when_script_returns_permission_deny() {
        let hook = HookConfig {
            event: "pre_tool_use".into(),
            command: r#"echo '{"permission":"deny","user_message":"blocked by test"}'"#.into(),
            enabled: true,
            timeout_secs: 5,
            fail_closed: false,
        };
        let payload = HookPayload {
            hook_event_name: "pre_tool_use".into(),
            session_id: None,
            cwd: "/tmp".into(),
            extra: serde_json::json!({"tool_name": "run_command"}),
        };
        let (deny, ctxs) = run_hooks(HookEvent::PreToolUse, &payload, &[hook], None);
        assert_eq!(deny.as_deref(), Some("blocked by test"));
        assert!(ctxs.is_empty());
    }

    #[test]
    fn run_hooks_collects_additional_context() {
        let hook = HookConfig {
            event: "post_tool_use".into(),
            command: r#"echo '{"additional_context":"extra info"}'"#.into(),
            enabled: true,
            timeout_secs: 5,
            fail_closed: false,
        };
        let payload = HookPayload {
            hook_event_name: "post_tool_use".into(),
            session_id: None,
            cwd: "/tmp".into(),
            extra: serde_json::json!({}),
        };
        let (deny, ctxs) = run_hooks(HookEvent::PostToolUse, &payload, &[hook], None);
        assert!(deny.is_none());
        assert_eq!(ctxs, vec!["extra info".to_string()]);
    }

    #[test]
    fn run_hooks_fail_closed_blocks_on_error() {
        let hook = HookConfig {
            event: "pre_tool_use".into(),
            command: "false".into(), // exits non-zero
            enabled: true,
            timeout_secs: 5,
            fail_closed: true,
        };
        let payload = HookPayload {
            hook_event_name: "pre_tool_use".into(),
            session_id: None,
            cwd: "/tmp".into(),
            extra: serde_json::json!({}),
        };
        let (deny, _) = run_hooks(HookEvent::PreToolUse, &payload, &[hook], None);
        assert!(deny.is_some());
        assert!(deny.unwrap().contains("fail-closed"));
    }

    #[test]
    fn run_hooks_non_fail_closed_allows_on_error() {
        let hook = HookConfig {
            event: "pre_tool_use".into(),
            command: "false".into(),
            enabled: true,
            timeout_secs: 5,
            fail_closed: false,
        };
        let payload = HookPayload {
            hook_event_name: "pre_tool_use".into(),
            session_id: None,
            cwd: "/tmp".into(),
            extra: serde_json::json!({}),
        };
        let (deny, _) = run_hooks(HookEvent::PreToolUse, &payload, &[hook], None);
        assert!(deny.is_none());
    }

    #[test]
    fn truncate_context_limits_to_10k_chars() {
        let long = "x".repeat(15_000);
        let truncated = truncate_context(&long);
        assert_eq!(truncated.len(), 10_000);
    }

    fn async_payload(event: HookEvent) -> HookPayload {
        HookPayload {
            hook_event_name: event.as_str().into(),
            session_id: Some("session-safe-id".into()),
            cwd: std::env::temp_dir().to_string_lossy().to_string(),
            extra: serde_json::json!({"safe_test": true}),
        }
    }

    #[tokio::test]
    async fn async_runner_skips_disabled_hooks_and_keeps_snapshot_secret_free() {
        let secret_marker = "HIDDEN-HOOK-COMMAND-MARKER";
        let runner = HookRunner::new(
            vec![HookConfig {
                event: "pre_tool_use".into(),
                command: format!("printf '{secret_marker}'; exit 1"),
                enabled: false,
                timeout_secs: 5,
                fail_closed: true,
            }],
            crate::sandbox::SandboxMode::PolicyOnly,
        );
        let batch = runner
            .run_event(
                &test_owner(),
                HookEvent::PreToolUse,
                &async_payload(HookEvent::PreToolUse),
                &[],
            )
            .await;
        assert!(batch.denied.is_none());
        let snapshot = runner.snapshot();
        assert_eq!(snapshot.entries.len(), 1);
        assert!(!snapshot.entries[0].enabled);
        assert!(snapshot.entries[0].last_result.is_none());
        assert!(!format!("{runner:?}{snapshot:?}").contains(secret_marker));
    }

    #[tokio::test]
    async fn async_runner_reports_denial_context_and_safe_status() {
        let runner = HookRunner::new(
            vec![HookConfig {
                event: "pre_tool_use".into(),
                command: r#"printf '%s' '{"permission":"deny","user_message":"blocked","additional_context":"use safe path"}'"#.into(),
                enabled: true,
                timeout_secs: 5,
                fail_closed: false,
            }],
            crate::sandbox::SandboxMode::PolicyOnly,
        );
        let batch = runner
            .run_event(
                &test_owner(),
                HookEvent::PreToolUse,
                &async_payload(HookEvent::PreToolUse),
                &[],
            )
            .await;
        assert_eq!(batch.denied.as_deref(), Some("blocked"));
        assert_eq!(batch.contexts, vec!["use safe path"]);
        assert_eq!(
            runner.snapshot().entries[0]
                .last_result
                .as_ref()
                .unwrap()
                .status,
            HookSafeStatus::Denied
        );
    }

    #[tokio::test]
    async fn async_runner_passes_only_safe_hook_identity_environment() {
        let runner = HookRunner::new(
            vec![HookConfig {
                event: "session_start".into(),
                command:
                    r#"printf '{"additional_context":"%s|%s"}' "$CA_HOOK_EVENT" "$CA_SESSION_ID""#
                        .into(),
                enabled: true,
                timeout_secs: 5,
                fail_closed: true,
            }],
            crate::sandbox::SandboxMode::PolicyOnly,
        );
        let batch = runner
            .run_event(
                &test_owner(),
                HookEvent::SessionStart,
                &async_payload(HookEvent::SessionStart),
                &[],
            )
            .await;
        assert_eq!(
            batch.contexts,
            ["session_start|00000000-0000-0000-0000-000000000001"]
        );
    }

    #[tokio::test]
    async fn reload_invalidates_only_changed_hook_results() {
        let hook = |event: &str, command: &str| HookConfig {
            event: event.into(),
            command: command.into(),
            enabled: true,
            timeout_secs: 5,
            fail_closed: false,
        };
        let runner = HookRunner::new(
            vec![hook("pre_tool_use", "true"), hook("post_tool_use", "true")],
            crate::sandbox::SandboxMode::PolicyOnly,
        );
        runner
            .run_event(
                &test_owner(),
                HookEvent::PreToolUse,
                &async_payload(HookEvent::PreToolUse),
                &[],
            )
            .await;
        runner
            .run_event(
                &test_owner(),
                HookEvent::PostToolUse,
                &async_payload(HookEvent::PostToolUse),
                &[],
            )
            .await;
        runner.reload(vec![
            hook("pre_tool_use", "false"),
            hook("post_tool_use", "true"),
        ]);
        let snapshot = runner.snapshot();
        assert!(snapshot.entries[0].last_result.is_none());
        assert_eq!(
            snapshot.entries[1].last_result.as_ref().unwrap().status,
            HookSafeStatus::Succeeded
        );
    }

    #[tokio::test]
    async fn async_runner_times_out_and_fails_closed() {
        let runner = HookRunner::new(
            vec![HookConfig {
                event: "session_start".into(),
                command: "sleep 5".into(),
                enabled: true,
                timeout_secs: 1,
                fail_closed: true,
            }],
            crate::sandbox::SandboxMode::PolicyOnly,
        );
        let batch = runner
            .run_event(
                &test_owner(),
                HookEvent::SessionStart,
                &async_payload(HookEvent::SessionStart),
                &[],
            )
            .await;
        assert!(batch.fail_closed);
        assert_eq!(
            batch.denied.as_deref(),
            Some("Hook timed out (fail-closed)")
        );
        assert_eq!(
            runner.snapshot().entries[0]
                .last_result
                .as_ref()
                .unwrap()
                .status,
            HookSafeStatus::TimedOut
        );
    }

    #[tokio::test]
    async fn async_runner_rejects_oversized_output_without_retaining_it() {
        let runner = HookRunner::new(
            vec![HookConfig {
                event: "after_agent_response".into(),
                command: "head -c 70000 /dev/zero".into(),
                enabled: true,
                timeout_secs: 5,
                fail_closed: true,
            }],
            crate::sandbox::SandboxMode::PolicyOnly,
        );
        let batch = runner
            .run_event(
                &test_owner(),
                HookEvent::AfterAgentResponse,
                &async_payload(HookEvent::AfterAgentResponse),
                &[],
            )
            .await;
        assert!(batch.fail_closed);
        assert!(batch.contexts.is_empty());
        assert_eq!(runner.take_contexts(&test_owner()), Vec::<String>::new());
        assert!(format!("{:?}", runner.snapshot()).len() < 2_000);
    }

    #[tokio::test]
    async fn malformed_structured_output_fails_closed_without_retaining_output() {
        let output_marker = "MALFORMED-HOOK-OUTPUT-MARKER";
        let runner = HookRunner::new(
            vec![HookConfig {
                event: "pre_tool_use".into(),
                command: format!("printf '%s' '{{{output_marker}'"),
                enabled: true,
                timeout_secs: 5,
                fail_closed: true,
            }],
            crate::sandbox::SandboxMode::PolicyOnly,
        );
        let batch = runner
            .run_event(
                &test_owner(),
                HookEvent::PreToolUse,
                &async_payload(HookEvent::PreToolUse),
                &[],
            )
            .await;
        assert!(batch.fail_closed);
        assert_eq!(batch.denied.as_deref(), Some("Hook failed (fail-closed)"));
        assert_eq!(
            runner.snapshot().entries[0]
                .last_result
                .as_ref()
                .unwrap()
                .status,
            HookSafeStatus::Failed
        );
        assert!(!format!("{:?}{:?}", runner, runner.snapshot()).contains(output_marker));
    }

    #[tokio::test]
    async fn bare_runner_exposes_safe_snapshot_but_never_executes() {
        let runner = HookRunner::new_with_runtime(
            vec![HookConfig {
                event: "session_start".into(),
                command: "exit 99".into(),
                enabled: true,
                timeout_secs: 5,
                fail_closed: true,
            }],
            crate::sandbox::SandboxMode::PolicyOnly,
            false,
        );
        assert_eq!(runner.snapshot().entries.len(), 1);
        assert!(!runner.has_enabled(HookEvent::SessionStart));
        let batch = runner
            .run_event(
                &test_owner(),
                HookEvent::SessionStart,
                &async_payload(HookEvent::SessionStart),
                &[],
            )
            .await;
        assert!(batch.denied.is_none());
        assert!(batch.contexts.is_empty());
        assert!(!batch.fail_closed);
        assert!(runner.snapshot().entries[0].last_result.is_none());
        let error = match runner
            .test_hook(
                &test_owner(),
                0,
                HookEvent::SessionStart,
                &async_payload(HookEvent::SessionStart),
                &[],
            )
            .await
        {
            Ok(_) => panic!("bare hook unexpectedly executed"),
            Err(error) => error,
        };
        assert_eq!(error, "Hooks are disabled in the bare profile");
    }

    #[tokio::test]
    async fn explicit_test_context_does_not_enter_next_model_request() {
        let runner = HookRunner::new(
            vec![HookConfig {
                event: "pre_tool_use".into(),
                command: r#"printf '%s' '{"additional_context":"test-only context"}'"#.into(),
                enabled: true,
                timeout_secs: 5,
                fail_closed: false,
            }],
            crate::sandbox::SandboxMode::PolicyOnly,
        );
        let batch = runner
            .test_hook(
                &test_owner(),
                0,
                HookEvent::PreToolUse,
                &async_payload(HookEvent::PreToolUse),
                &[],
            )
            .await
            .unwrap();
        assert_eq!(batch.contexts, ["test-only context"]);
        assert!(runner.take_contexts(&test_owner()).is_empty());
    }

    #[tokio::test]
    async fn failing_post_hook_records_exact_blocking_event() {
        let runner = HookRunner::new(
            vec![HookConfig {
                event: "post_tool_use_failure".into(),
                command: "false".into(),
                enabled: true,
                timeout_secs: 5,
                fail_closed: true,
            }],
            crate::sandbox::SandboxMode::PolicyOnly,
        );
        let batch = runner
            .run_event(
                &test_owner(),
                HookEvent::PostToolUseFailure,
                &async_payload(HookEvent::PostToolUseFailure),
                &[],
            )
            .await;
        assert!(batch.fail_closed);
        assert_eq!(
            runner.take_blocking_failure(&test_owner()),
            Some((
                HookEvent::PostToolUseFailure,
                "Hook failed (fail-closed)".into()
            ))
        );
    }

    #[tokio::test]
    async fn aborting_hook_marks_it_cancelled_and_kills_the_process_group() {
        let runner = HookRunner::new(
            vec![HookConfig {
                event: "before_submit_prompt".into(),
                command: "sleep 30 & wait".into(),
                enabled: true,
                timeout_secs: 60,
                fail_closed: false,
            }],
            crate::sandbox::SandboxMode::PolicyOnly,
        );
        let task_runner = runner.clone();
        let task = tokio::spawn(async move {
            task_runner
                .run_event(
                    &test_owner(),
                    HookEvent::BeforeSubmitPrompt,
                    &async_payload(HookEvent::BeforeSubmitPrompt),
                    &[],
                )
                .await
        });
        tokio::time::sleep(Duration::from_millis(100)).await;
        task.abort();
        let _ = task.await;
        tokio::task::yield_now().await;
        assert_eq!(
            runner.snapshot().entries[0]
                .last_result
                .as_ref()
                .unwrap()
                .status,
            HookSafeStatus::Cancelled
        );
    }

    #[tokio::test]
    async fn successful_hook_cleans_up_descendants_that_inherit_output_pipes() {
        let runner = HookRunner::new(
            vec![HookConfig {
                event: "post_tool_use".into(),
                command: "sleep 30 & printf '{}'".into(),
                enabled: true,
                timeout_secs: 5,
                fail_closed: true,
            }],
            crate::sandbox::SandboxMode::PolicyOnly,
        );
        let batch = tokio::time::timeout(
            Duration::from_secs(2),
            runner.run_event(
                &test_owner(),
                HookEvent::PostToolUse,
                &async_payload(HookEvent::PostToolUse),
                &[],
            ),
        )
        .await
        .expect("hook descendants kept the worker alive");
        assert!(batch.denied.is_none());
        assert_eq!(
            runner.snapshot().entries[0]
                .last_result
                .as_ref()
                .unwrap()
                .status,
            HookSafeStatus::Succeeded
        );
    }
}
