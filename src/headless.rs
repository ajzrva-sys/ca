#[cfg(test)]
use crate::agent::client::ToolDef;
use crate::agent::client::{ChatBackend, ChatMessage};
use crate::agent::registry::{ToolCapability, ToolExecutionResult, ToolRegistry};
use crate::agent::stream::{
    AssembledToolCall, AssistantTurnAssembler, AssistantTurnLimits, ProviderError, ProviderEvent,
    UsageInfo,
};
use crate::app::{Message, Role, ToolCall, ToolStatus};
use crate::config::ApprovalRule;
use clap::ValueEnum;
use serde::Serialize;
use std::io::Write;
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Clone)]
pub struct HeadlessConvergenceConfig {
    pub plan: crate::review_convergence::ReviewConvergencePlan,
    pub clients: [std::sync::Arc<dyn ChatBackend>; 2],
    pub acceptance_store: Option<crate::acceptance::AcceptanceStore>,
}

impl std::fmt::Debug for HeadlessConvergenceConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HeadlessConvergenceConfig")
            .field("plan", &self.plan)
            .field("clients", &"[REDACTED; 2]")
            .field("acceptance_store", &self.acceptance_store)
            .finish()
    }
}

pub const SCHEMA: &str = "ca.headless.v1";

const ROLE_RUN_MAX_ASSISTANT_TEXT_BYTES: usize = 512 * 1024;
const ROLE_RUN_MAX_REASONING_BYTES: usize = 512 * 1024;
const ROLE_RUN_MAX_TOOL_ARGUMENT_BYTES: usize = 512 * 1024;
const ROLE_RUN_MAX_TOOL_CALLS_PER_TURN: usize = 64;
const ROLE_RUN_MAX_PROVIDER_TURN_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    #[default]
    Text,
    Json,
    #[value(alias = "stream-json")]
    Jsonl,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeadlessApprovalMode {
    RequireApproval,
    AutoReview,
    Yolo,
}

#[derive(Debug, Clone)]
pub struct HeadlessRunConfig {
    pub workspace_activity_scope: Option<PathBuf>,
    pub automation_policy: Option<crate::automation_dispatch::AutomationDispatchPolicy>,
    pub outcome_sink: Option<crate::session_outcome::OutcomeSink>,
    pub provider: String,
    pub model: String,
    pub cwd: PathBuf,
    pub workspace: crate::workspace::WorkspaceContext,
    pub max_turns: usize,
    pub single_turn: bool,
    pub command_timeout_secs: u64,
    pub command_output_policy: crate::config::CommandOutputPolicy,
    pub output_format: OutputFormat,
    pub show_thinking: bool,
    pub printenv: bool,
    pub approval_mode: HeadlessApprovalMode,
    pub approval_rules: Vec<ApprovalRule>,
    pub permission_rules: Vec<crate::permission::PermissionRule>,
    pub session_grants: Vec<crate::permission::PermissionMatcher>,
    pub allow_network: bool,
    pub output_last_message: Option<PathBuf>,
    pub check_mode: crate::headless_contract::CheckMode,
    pub convergence: Option<HeadlessConvergenceConfig>,
    pub structured_output: Option<crate::headless_contract::StructuredOutputContract>,
    pub original_request: String,
    pub verification_instructions: String,
    pub chat_options: crate::agent::client::ChatRequestOptions,
    pub sandbox_mode: crate::sandbox::SandboxMode,
    pub run_id: Uuid,
    pub job_owner: crate::jobs::JobOwner,
    pub skills: std::sync::Arc<std::sync::Mutex<crate::skills::SkillRegistry>>,
    pub mcp: Option<std::sync::Arc<crate::mcp::McpManager>>,
    pub mechanistic: crate::config::MechanisticMcpConfig,
    pub lsp: Option<std::sync::Arc<crate::lsp::LspManager>>,
    pub email: Option<std::sync::Arc<crate::email::EmailManager>>,
    pub tool_policy: crate::tool_selection::ToolSelectionPolicy,
    pub tool_constraints: crate::tool_selection::ToolSelectionConstraints,
    pub tool_profile: crate::agent::registry::ToolProfile,
    pub startup_warnings: Vec<String>,
    pub request_kind: crate::request::RequestKind,
    pub agents: Option<std::sync::Arc<crate::agent::subagent::AgentManager>>,
    pub work_state: std::sync::Arc<std::sync::Mutex<crate::work::WorkState>>,
    /// The workspace directory inode captured while validating a role-run
    /// request. Role-only file boundaries reject any later pathname reopen
    /// that does not resolve to this exact directory.
    pub role_run_workspace_identity: Option<crate::role_run::RoleRunWorkspaceIdentity>,
    /// Exact, resolved secret values that must never cross the headless output
    /// boundary. Ordinary headless runs leave this empty; role-run supplies
    /// only the selected route's resolved credential.
    pub output_redactions: HeadlessOutputRedactions,
}

pub(crate) struct WorkspaceActivityGuard {
    lease: Option<crate::recovery::WorkspaceActivityLease>,
    agents: Option<std::sync::Arc<crate::agent::subagent::AgentManager>>,
    owner: crate::jobs::JobOwner,
}
impl WorkspaceActivityGuard {
    pub(crate) fn new(
        lease: Option<crate::recovery::WorkspaceActivityLease>,
        agents: Option<std::sync::Arc<crate::agent::subagent::AgentManager>>,
        owner: crate::jobs::JobOwner,
    ) -> Self {
        Self {
            lease,
            agents,
            owner,
        }
    }
}
impl Drop for WorkspaceActivityGuard {
    fn drop(&mut self) {
        let Some(lease) = self.lease.take() else {
            return;
        };
        let manager = self.agents.take();
        let mut activity = manager.as_ref().map(|manager| manager.subscribe_activity());
        let owner = self.owner;
        // Managed jobs and background children can outlive a provider loop.
        // Preserve workspace exclusion until all owned work is terminal.
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                let _lease = lease;
                let jobs = crate::jobs::JobManager::process();
                let mut job_events = jobs.subscribe();
                loop {
                    let children_finished = activity.as_ref().is_none_or(|activity| {
                        activity
                            .borrow()
                            .iter()
                            .all(|child| child.status.terminal())
                    });
                    if children_finished
                        && jobs
                            .list_owned(owner)
                            .await
                            .iter()
                            .all(|job| job.status.is_terminal())
                    {
                        break;
                    }
                    tokio::select! {
                        _ = job_events.recv() => {},
                        _ = async {
                            match activity.as_mut() {
                                Some(activity) => { let _ = activity.changed().await; },
                                None => std::future::pending::<()>().await,
                            }
                        } => {},
                    }
                }
                drop(manager);
            });
        }
    }
}

impl HeadlessRunConfig {
    fn uses_v2(&self) -> bool {
        self.check_mode.enabled() || self.structured_output.is_some()
    }
}

/// A deliberately opaque collection of exact values removed from externally
/// visible headless fields. Debug output never reveals values or lengths.
#[derive(Clone, Default)]
pub struct HeadlessOutputRedactions {
    values: Vec<String>,
}

impl std::fmt::Debug for HeadlessOutputRedactions {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("HeadlessOutputRedactions([REDACTED])")
    }
}

impl HeadlessOutputRedactions {
    pub fn exact(values: impl IntoIterator<Item = String>) -> Self {
        let mut values = values
            .into_iter()
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>();
        values.sort_by_key(|value| std::cmp::Reverse(value.len()));
        values.dedup();
        Self { values }
    }

    fn text(&self, value: &str) -> String {
        if self.values.iter().any(|secret| value.contains(secret)) {
            self.safe_marker("[REDACTED]")
        } else {
            value.to_string()
        }
    }

    fn label(&self, value: &str) -> String {
        if self.values.iter().any(|secret| value.contains(secret)) {
            self.safe_marker("redacted")
        } else {
            value.to_string()
        }
    }

    fn safe_marker(&self, preferred: &str) -> String {
        if self.values.iter().all(|secret| !preferred.contains(secret)) {
            return preferred.to_string();
        }
        "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_-"
            .chars()
            .map(|candidate| candidate.to_string())
            .find(|candidate| self.values.iter().all(|secret| !candidate.contains(secret)))
            .unwrap_or_default()
    }

    pub(crate) fn text_contains_secret(&self, value: &str) -> bool {
        self.values.iter().any(|secret| value.contains(secret))
    }

    fn value_contains_secret(&self, value: &serde_json::Value) -> bool {
        match value {
            serde_json::Value::String(text) => self.text_contains_secret(text),
            serde_json::Value::Array(values) => {
                values.iter().any(|value| self.value_contains_secret(value))
            }
            serde_json::Value::Object(object) => object.iter().any(|(key, value)| {
                self.text_contains_secret(key) || self.value_contains_secret(value)
            }),
            serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {
                false
            }
        }
    }

    fn value(&self, value: &mut serde_json::Value) {
        match value {
            serde_json::Value::String(text) => *text = self.text(text),
            serde_json::Value::Array(values) => {
                for value in values {
                    self.value(value);
                }
            }
            serde_json::Value::Object(object) => {
                let fields = std::mem::take(object);
                for (key, mut value) in fields {
                    self.value(&mut value);
                    object.insert(self.text(&key), value);
                }
            }
            serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {
            }
        }
    }

    fn usage(&self, usage: &UsageInfo) -> UsageInfo {
        let response_metadata = usage.response_metadata.as_ref().and_then(|metadata| {
            crate::agent::stream::ProviderResponseMetadata::new(
                self.label(metadata.request_id_sha256()),
                self.label(metadata.model()),
                metadata.service_tier().map(|value| self.label(value)),
            )
            .ok()
        });
        UsageInfo {
            prompt_tokens: usage.prompt_tokens,
            completion_tokens: usage.completion_tokens,
            total_tokens: usage.total_tokens,
            cached_prompt_tokens: usage.cached_prompt_tokens,
            cache_write_tokens: usage.cache_write_tokens,
            reasoning_tokens: usage.reasoning_tokens,
            response_metadata,
            cache_usage_override: usage.cache_usage_override,
        }
    }

    fn event_v1(&self, event: HeadlessEventV1) -> HeadlessEventV1 {
        match event {
            HeadlessEventV1::RunStart { model } => HeadlessEventV1::RunStart {
                model: self.text(&model),
            },
            HeadlessEventV1::Warning { message } => HeadlessEventV1::Warning {
                message: self.text(&message),
            },
            HeadlessEventV1::TurnStart { turn } => HeadlessEventV1::TurnStart { turn },
            HeadlessEventV1::AssistantOutput { delta } => HeadlessEventV1::AssistantOutput {
                delta: self.text(&delta),
            },
            HeadlessEventV1::ReasoningOutput { delta } => HeadlessEventV1::ReasoningOutput {
                delta: self.text(&delta),
            },
            HeadlessEventV1::ToolStart {
                call_id,
                name,
                mut arguments,
            } => {
                self.value(&mut arguments);
                HeadlessEventV1::ToolStart {
                    call_id: self.text(&call_id),
                    name: self.text(&name),
                    arguments,
                }
            }
            HeadlessEventV1::ToolFinish {
                call_id,
                name,
                status,
                output,
            } => HeadlessEventV1::ToolFinish {
                call_id: self.text(&call_id),
                name: self.text(&name),
                status,
                output: self.text(&output),
            },
            HeadlessEventV1::EnvironmentSnapshot { snapshot } => {
                HeadlessEventV1::EnvironmentSnapshot { snapshot }
            }
            HeadlessEventV1::Usage { usage } => HeadlessEventV1::Usage {
                usage: self.usage(&usage),
            },
            HeadlessEventV1::ApprovalRequired {
                call_id,
                name,
                reason,
            } => HeadlessEventV1::ApprovalRequired {
                call_id: self.text(&call_id),
                name: self.text(&name),
                reason: self.text(&reason),
            },
            HeadlessEventV1::Completion { message } => HeadlessEventV1::Completion {
                message: self.text(&message),
            },
            HeadlessEventV1::Failure {
                kind,
                message,
                exit_code,
            } => HeadlessEventV1::Failure {
                kind,
                message: self.text(&message),
                exit_code,
            },
        }
    }

    fn event_v2(
        &self,
        event: crate::headless_contract::HeadlessEventV2,
    ) -> crate::headless_contract::HeadlessEventV2 {
        use crate::headless_contract::HeadlessEventV2;
        match event {
            HeadlessEventV2::StructuredOutput { mut value } => {
                self.value(&mut value);
                HeadlessEventV2::StructuredOutput { value }
            }
            HeadlessEventV2::MainCompletion { message } => HeadlessEventV2::MainCompletion {
                message: self.text(&message),
            },
            HeadlessEventV2::CheckReport { mut outcome } => {
                outcome.report.summary = self.text(&outcome.report.summary);
                for item in outcome
                    .report
                    .issues
                    .iter_mut()
                    .chain(&mut outcome.report.evidence)
                {
                    *item = self.text(item);
                }
                HeadlessEventV2::CheckReport { outcome }
            }
            HeadlessEventV2::VerificationFailed { summary, exit_code } => {
                HeadlessEventV2::VerificationFailed {
                    summary: self.text(&summary),
                    exit_code,
                }
            }
            HeadlessEventV2::CheckStart { repair_requested } => {
                HeadlessEventV2::CheckStart { repair_requested }
            }
            HeadlessEventV2::ConvergenceStart {
                round,
                mut routes,
                max_cost_usd,
                unpriced_acknowledged,
            } => {
                for route in &mut routes {
                    *route = self.text(route);
                }
                HeadlessEventV2::ConvergenceStart {
                    round,
                    routes,
                    max_cost_usd,
                    unpriced_acknowledged,
                }
            }
            HeadlessEventV2::ConvergenceReport { mut outcome } => {
                outcome.scope = self.text(&outcome.scope);
                for reviewer in &mut outcome.reviewers {
                    reviewer.provider = self.text(&reviewer.provider);
                    reviewer.model = self.text(&reviewer.model);
                    reviewer.summary = self.text(&reviewer.summary);
                    for finding in &mut reviewer.findings {
                        finding.title = self.text(&finding.title);
                        finding.rationale = self.text(&finding.rationale);
                        finding.file = self.text(&finding.file);
                        finding.suggested_validation = self.text(&finding.suggested_validation);
                    }
                }
                HeadlessEventV2::ConvergenceReport { outcome }
            }
            HeadlessEventV2::AcceptancePacket {
                packet_sha256,
                stale,
            } => HeadlessEventV2::AcceptancePacket {
                packet_sha256,
                stale,
            },
            other => map_v1_event(
                &self.event_v1(v2_event_as_v1(&other).expect("covered non-v1 v2 event")),
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeadlessExit {
    pub code: i32,
}

impl HeadlessExit {
    pub const SUCCESS: Self = Self { code: 0 };
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct HeadlessEnvelopeV1 {
    pub schema: &'static str,
    pub sequence: u64,
    pub run_id: Uuid,
    pub event: HeadlessEventV1,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct HeadlessOutputV1 {
    pub schema: &'static str,
    pub run_id: Uuid,
    pub success: bool,
    pub final_message: Option<String>,
    pub events: Vec<HeadlessEnvelopeV1>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub automation_usage: Option<crate::automation::AutomationUsageCountersV1>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HeadlessEventV1 {
    RunStart {
        model: String,
    },
    Warning {
        message: String,
    },
    TurnStart {
        turn: usize,
    },
    AssistantOutput {
        delta: String,
    },
    #[allow(dead_code)]
    ReasoningOutput {
        delta: String,
    },
    ToolStart {
        call_id: String,
        name: String,
        arguments: serde_json::Value,
    },
    ToolFinish {
        call_id: String,
        name: String,
        status: ToolResultStatus,
        output: String,
    },
    EnvironmentSnapshot {
        snapshot: crate::run_environment::EnvironmentSnapshotV1,
    },
    Usage {
        usage: UsageInfo,
    },
    ApprovalRequired {
        call_id: String,
        name: String,
        reason: String,
    },
    Completion {
        message: String,
    },
    Failure {
        kind: HeadlessFailureKind,
        message: String,
        exit_code: i32,
    },
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolResultStatus {
    Success,
    Failure,
    Denied,
    Cancelled,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HeadlessFailureKind {
    Input,
    Setup,
    ApprovalRequired,
    ProviderTransport,
    ProviderProtocol,
    MaximumTurns,
    Internal,
    Interrupted,
    IncompleteSubagents,
    LowYieldLoop,
    Verification,
}

pub struct HeadlessEmitter<'a> {
    automation_dispatch:
        Option<std::sync::Arc<std::sync::Mutex<crate::automation_dispatch::AutomationDispatch>>>,
    outcome: Option<crate::session_outcome::ScopedOutcome>,
    format: OutputFormat,
    run_id: Uuid,
    sequence: u64,
    events: Vec<HeadlessEnvelopeV1>,
    contract_v2: bool,
    v2_events: Vec<crate::headless_contract::HeadlessEnvelopeV2>,
    stdout: &'a mut dyn Write,
    stderr: &'a mut dyn Write,
    final_message: Option<String>,
    success: bool,
    structured_output: Option<serde_json::Value>,
    verification: Option<crate::headless_contract::VerificationOutcome>,
    v2_main_completed: bool,
    redactions: HeadlessOutputRedactions,
}

#[derive(Debug, thiserror::Error)]
enum EmitError {
    #[error("downstream output closed")]
    BrokenPipe,
    #[error("output failed: {0}")]
    Other(String),
}

impl<'a> HeadlessEmitter<'a> {
    fn new_for_contract(
        format: OutputFormat,
        run_id: Uuid,
        stdout: &'a mut dyn Write,
        stderr: &'a mut dyn Write,
        contract_v2: bool,
        redactions: HeadlessOutputRedactions,
    ) -> Self {
        Self {
            automation_dispatch: None,
            outcome: None,
            format,
            run_id,
            sequence: 0,
            events: Vec::new(),
            contract_v2,
            v2_events: Vec::new(),
            stdout,
            stderr,
            final_message: None,
            success: false,
            structured_output: None,
            verification: None,
            v2_main_completed: false,
            redactions,
        }
    }

    fn emit(&mut self, event: HeadlessEventV1) -> Result<(), EmitError> {
        let event = self.redactions.event_v1(event);
        if let Some(tracker) = &mut self.outcome {
            match &event {
                HeadlessEventV1::ToolFinish {
                    call_id,
                    name,
                    status,
                    output,
                } => {
                    let success = matches!(status, ToolResultStatus::Success);
                    if tracker.record_tool(
                        0,
                        call_id,
                        name,
                        success,
                        matches!(status, ToolResultStatus::Denied),
                    ) {
                        tracker.event(
                            "tool_evidence",
                            Some(name),
                            Some(call_id),
                            Some(success),
                            output,
                        );
                    }
                }
                HeadlessEventV1::TurnStart { .. } => tracker.begin_execution(),
                HeadlessEventV1::Completion { .. } => tracker.record_assistant_turn(),
                HeadlessEventV1::Failure {
                    kind: HeadlessFailureKind::Interrupted,
                    ..
                } => tracker.record_interrupt(),
                HeadlessEventV1::Failure { .. } => tracker.record_failure(),
                _ => {}
            }
            tracker.persist();
        }
        if self.contract_v2 {
            return self.emit_v2(map_v1_event(&event));
        }
        let envelope = HeadlessEnvelopeV1 {
            schema: SCHEMA,
            sequence: self.sequence,
            run_id: self.run_id,
            event,
        };
        self.sequence += 1;
        match self.format {
            OutputFormat::Text => self.emit_text(&envelope.event)?,
            OutputFormat::Json => {}
            OutputFormat::Jsonl => {
                serde_json::to_writer(&mut self.stdout, &envelope)
                    .map_err(|error| map_io_error(error.into()))?;
                write_checked(&mut self.stdout, b"\n")?;
                self.stdout.flush().map_err(map_io_error)?;
            }
        }
        self.events.push(envelope);
        Ok(())
    }

    fn emit_v2(
        &mut self,
        event: crate::headless_contract::HeadlessEventV2,
    ) -> Result<(), EmitError> {
        debug_assert!(self.contract_v2);
        let event = self.redactions.event_v2(event);
        let envelope = crate::headless_contract::HeadlessEnvelopeV2 {
            schema: crate::headless_contract::SCHEMA_V2,
            sequence: self.sequence,
            run_id: self.run_id,
            event,
        };
        self.sequence += 1;
        match self.format {
            OutputFormat::Text => self.emit_v2_text(&envelope.event)?,
            OutputFormat::Json => {}
            OutputFormat::Jsonl => {
                serde_json::to_writer(&mut self.stdout, &envelope)
                    .map_err(|error| map_io_error(error.into()))?;
                write_checked(&mut self.stdout, b"\n")?;
                self.stdout.flush().map_err(map_io_error)?;
            }
        }
        self.v2_events.push(envelope);
        Ok(())
    }

    fn emit_v2_text(
        &mut self,
        event: &crate::headless_contract::HeadlessEventV2,
    ) -> Result<(), EmitError> {
        use crate::headless_contract::HeadlessEventV2;
        match event {
            HeadlessEventV2::MainCompletion { .. } => {
                self.v2_main_completed = true;
                write_checked(&mut self.stdout, b"\n")?;
            }
            HeadlessEventV2::CheckStart { repair_requested } => write_checked(
                &mut self.stderr,
                if *repair_requested {
                    b"ca: verification started (repair enabled)\n"
                } else {
                    b"ca: verification started (read-only)\n"
                },
            )?,
            HeadlessEventV2::CheckReport { outcome } => {
                let label = if outcome.report.passed {
                    "passed"
                } else {
                    "failed"
                };
                write_checked(
                    &mut self.stderr,
                    format!(
                        "ca: verification {label}: {}\n",
                        crate::terminal_text::sanitize_terminal_text(&outcome.report.summary)
                    )
                    .as_bytes(),
                )?;
            }
            HeadlessEventV2::ConvergenceStart { round, routes, .. } => write_checked(
                &mut self.stderr,
                format!(
                    "ca: convergent review round {round} started ({} and {})\n",
                    crate::terminal_text::sanitize_terminal_text(&routes[0]),
                    crate::terminal_text::sanitize_terminal_text(&routes[1])
                )
                .as_bytes(),
            )?,
            HeadlessEventV2::ConvergenceReport { outcome } => write_checked(
                &mut self.stderr,
                format!(
                    "ca: convergent review round {} {}\n",
                    outcome.round,
                    if outcome.passed { "passed" } else { "blocked" }
                )
                .as_bytes(),
            )?,
            HeadlessEventV2::AcceptancePacket { packet_sha256, .. } => write_checked(
                &mut self.stderr,
                format!("ca: acceptance packet {}\n", &packet_sha256[..16]).as_bytes(),
            )?,
            HeadlessEventV2::VerificationFailed { .. } => {}
            HeadlessEventV2::StructuredOutput { value } => {
                let encoded = serde_json::to_string(value)
                    .map_err(|error| EmitError::Other(error.to_string()))?;
                write_checked(&mut self.stdout, encoded.as_bytes())?;
            }
            HeadlessEventV2::Completion { .. } if self.v2_main_completed => {}
            _ => {
                if let Some(v1) = v2_event_as_v1(event) {
                    self.emit_text(&v1)?;
                }
            }
        }
        Ok(())
    }

    fn emit_text(&mut self, event: &HeadlessEventV1) -> Result<(), EmitError> {
        match event {
            HeadlessEventV1::AssistantOutput { delta } => {
                let safe = crate::terminal_text::sanitize_terminal_text(delta);
                write_checked(&mut self.stdout, safe.as_bytes())?
            }
            HeadlessEventV1::ReasoningOutput { delta } => {
                let safe = crate::terminal_text::sanitize_terminal_text(delta);
                write_checked(
                    &mut self.stderr,
                    format!("\n[reasoning]\n{safe}\n").as_bytes(),
                )?
            }
            HeadlessEventV1::ToolFinish { name, output, .. } => {
                let name = crate::terminal_text::sanitize_terminal_text(name);
                let output = crate::terminal_text::sanitize_terminal_text(output);
                write_checked(
                    &mut self.stderr,
                    format!("\n[tool: {name}]\n{}\n", clipped_preview(&output, 200)).as_bytes(),
                )?;
            }
            HeadlessEventV1::Completion { .. } => write_checked(&mut self.stdout, b"\n")?,
            HeadlessEventV1::Failure { message, .. } => {
                let message = crate::terminal_text::sanitize_terminal_text(message);
                write_checked(&mut self.stderr, format!("ca: {message}\n").as_bytes())?
            }
            HeadlessEventV1::ApprovalRequired { name, reason, .. } => write_checked(
                &mut self.stderr,
                format!(
                    "ca: approval required for {}: {}\n",
                    crate::terminal_text::sanitize_terminal_text(name),
                    crate::terminal_text::sanitize_terminal_text(reason),
                )
                .as_bytes(),
            )?,
            HeadlessEventV1::Warning { message } => {
                let message = crate::terminal_text::sanitize_terminal_text(message);
                write_checked(
                    &mut self.stderr,
                    format!("ca: warning: {message}\n").as_bytes(),
                )?
            }
            _ => {}
        }
        Ok(())
    }

    fn complete(&mut self, message: String) -> Result<(), EmitError> {
        let message = self.redactions.text(&message);
        self.final_message = Some(message.clone());
        self.success = true;
        self.emit(HeadlessEventV1::Completion { message })
    }

    fn record_structured_output(&mut self, value: serde_json::Value) {
        let mut value = value;
        self.redactions.value(&mut value);
        self.structured_output = Some(value);
    }

    fn record_verification(&mut self, outcome: crate::headless_contract::VerificationOutcome) {
        if let Some(tracker) = &mut self.outcome {
            tracker.record_model_verification(outcome.report.passed, &outcome.report.summary);
        }
        self.verification = Some(outcome);
    }

    fn finish(mut self) -> Result<(), EmitError> {
        // Drop records interruption on early exits. The async owner drains the
        // writer after this emitter and any delegated executions have finished.
        drop(self.outcome.take());
        if self.format == OutputFormat::Json {
            if self.contract_v2 {
                serde_json::to_writer(
                    &mut self.stdout,
                    &crate::headless_contract::HeadlessOutputV2 {
                        schema: crate::headless_contract::SCHEMA_V2,
                        run_id: self.run_id,
                        success: self.success,
                        final_message: self.final_message,
                        structured_output: self.structured_output,
                        verification: self.verification,
                        events: self.v2_events,
                    },
                )
                .map_err(|error| map_io_error(error.into()))?;
            } else {
                serde_json::to_writer(
                    &mut self.stdout,
                    &HeadlessOutputV1 {
                        schema: SCHEMA,
                        run_id: self.run_id,
                        success: self.success,
                        final_message: self.final_message,
                        events: self.events,
                        automation_usage: self.automation_dispatch.as_ref().map(|dispatch| {
                            dispatch
                                .lock()
                                .unwrap_or_else(|poison| poison.into_inner())
                                .snapshot()
                        }),
                    },
                )
                .map_err(|error| map_io_error(error.into()))?;
            }
            write_checked(&mut self.stdout, b"\n")?;
        }
        self.stdout.flush().map_err(map_io_error)
    }
}

fn map_v1_event(event: &HeadlessEventV1) -> crate::headless_contract::HeadlessEventV2 {
    use crate::headless_contract::HeadlessEventV2;
    match event {
        HeadlessEventV1::RunStart { model } => HeadlessEventV2::RunStart {
            model: model.clone(),
        },
        HeadlessEventV1::Warning { message } => HeadlessEventV2::Warning {
            message: message.clone(),
        },
        HeadlessEventV1::TurnStart { turn } => HeadlessEventV2::TurnStart { turn: *turn },
        HeadlessEventV1::AssistantOutput { delta } => HeadlessEventV2::AssistantOutput {
            delta: delta.clone(),
        },
        HeadlessEventV1::ReasoningOutput { delta } => HeadlessEventV2::ReasoningOutput {
            delta: delta.clone(),
        },
        HeadlessEventV1::ToolStart {
            call_id,
            name,
            arguments,
        } => HeadlessEventV2::ToolStart {
            call_id: call_id.clone(),
            name: name.clone(),
            arguments: arguments.clone(),
        },
        HeadlessEventV1::ToolFinish {
            call_id,
            name,
            status,
            output,
        } => HeadlessEventV2::ToolFinish {
            call_id: call_id.clone(),
            name: name.clone(),
            status: *status,
            output: output.clone(),
        },
        HeadlessEventV1::EnvironmentSnapshot { snapshot } => HeadlessEventV2::EnvironmentSnapshot {
            snapshot: snapshot.clone(),
        },
        HeadlessEventV1::Usage { usage } => HeadlessEventV2::Usage {
            usage: usage.clone(),
        },
        HeadlessEventV1::ApprovalRequired {
            call_id,
            name,
            reason,
        } => HeadlessEventV2::ApprovalRequired {
            call_id: call_id.clone(),
            name: name.clone(),
            reason: reason.clone(),
        },
        HeadlessEventV1::Completion { message } => HeadlessEventV2::Completion {
            message: message.clone(),
        },
        HeadlessEventV1::Failure {
            kind,
            message,
            exit_code,
        } => HeadlessEventV2::Failure {
            kind: *kind,
            message: message.clone(),
            exit_code: *exit_code,
        },
    }
}

fn v2_event_as_v1(event: &crate::headless_contract::HeadlessEventV2) -> Option<HeadlessEventV1> {
    use crate::headless_contract::HeadlessEventV2;
    match event {
        HeadlessEventV2::RunStart { model } => Some(HeadlessEventV1::RunStart {
            model: model.clone(),
        }),
        HeadlessEventV2::Warning { message } => Some(HeadlessEventV1::Warning {
            message: message.clone(),
        }),
        HeadlessEventV2::TurnStart { turn } => Some(HeadlessEventV1::TurnStart { turn: *turn }),
        HeadlessEventV2::AssistantOutput { delta } => Some(HeadlessEventV1::AssistantOutput {
            delta: delta.clone(),
        }),
        HeadlessEventV2::ReasoningOutput { delta } => Some(HeadlessEventV1::ReasoningOutput {
            delta: delta.clone(),
        }),
        HeadlessEventV2::ToolStart {
            call_id,
            name,
            arguments,
        } => Some(HeadlessEventV1::ToolStart {
            call_id: call_id.clone(),
            name: name.clone(),
            arguments: arguments.clone(),
        }),
        HeadlessEventV2::ToolFinish {
            call_id,
            name,
            status,
            output,
        } => Some(HeadlessEventV1::ToolFinish {
            call_id: call_id.clone(),
            name: name.clone(),
            status: *status,
            output: output.clone(),
        }),
        HeadlessEventV2::EnvironmentSnapshot { snapshot } => {
            Some(HeadlessEventV1::EnvironmentSnapshot {
                snapshot: snapshot.clone(),
            })
        }
        HeadlessEventV2::Usage { usage } => Some(HeadlessEventV1::Usage {
            usage: usage.clone(),
        }),
        HeadlessEventV2::ApprovalRequired {
            call_id,
            name,
            reason,
        } => Some(HeadlessEventV1::ApprovalRequired {
            call_id: call_id.clone(),
            name: name.clone(),
            reason: reason.clone(),
        }),
        HeadlessEventV2::Completion { message } => Some(HeadlessEventV1::Completion {
            message: message.clone(),
        }),
        HeadlessEventV2::Failure {
            kind,
            message,
            exit_code,
        } => Some(HeadlessEventV1::Failure {
            kind: *kind,
            message: message.clone(),
            exit_code: *exit_code,
        }),
        HeadlessEventV2::MainCompletion { .. }
        | HeadlessEventV2::CheckStart { .. }
        | HeadlessEventV2::CheckReport { .. }
        | HeadlessEventV2::ConvergenceStart { .. }
        | HeadlessEventV2::ConvergenceReport { .. }
        | HeadlessEventV2::AcceptancePacket { .. }
        | HeadlessEventV2::VerificationFailed { .. }
        | HeadlessEventV2::StructuredOutput { .. } => None,
    }
}

fn finish_with_code(emitter: HeadlessEmitter<'_>, code: i32) -> HeadlessExit {
    match emitter.finish() {
        Ok(()) => HeadlessExit { code },
        Err(EmitError::BrokenPipe) => HeadlessExit::SUCCESS,
        Err(EmitError::Other(_)) => HeadlessExit { code: 8 },
    }
}

fn map_io_error(error: std::io::Error) -> EmitError {
    if error.kind() == std::io::ErrorKind::BrokenPipe {
        EmitError::BrokenPipe
    } else {
        EmitError::Other(error.to_string())
    }
}

fn write_checked(writer: &mut dyn Write, bytes: &[u8]) -> Result<(), EmitError> {
    writer.write_all(bytes).map_err(map_io_error)
}

fn clipped_preview(value: &str, limit: usize) -> String {
    let total = value.chars().count();
    if total <= limit {
        return value.into();
    }
    let omitted_label = " characters omitted from tool output preview …]";
    let marker_reserve = omitted_label.chars().count() + 16;
    let retained = limit.saturating_sub(marker_reserve);
    let head_count = retained.div_ceil(2);
    let tail_count = retained / 2;
    let head = value.chars().take(head_count).collect::<String>();
    let tail = value
        .chars()
        .rev()
        .take(tail_count)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>();
    let omitted = total.saturating_sub(head.chars().count() + tail.chars().count());
    format!("{head}\n[… {omitted}{omitted_label}\n{tail}")
}

fn provider_failure(error: &ProviderError) -> (i32, HeadlessFailureKind) {
    match error {
        ProviderError::Http { .. }
        | ProviderError::Transport(_)
        | ProviderError::MissingEnvironment(_) => (5, HeadlessFailureKind::ProviderTransport),
        _ => (6, HeadlessFailureKind::ProviderProtocol),
    }
}

fn is_role_run(config: &HeadlessRunConfig) -> bool {
    config.request_kind == crate::request::RequestKind::RoleRun
}

fn assistant_turn_assembler(config: &HeadlessRunConfig) -> AssistantTurnAssembler {
    if is_role_run(config) {
        AssistantTurnAssembler::with_limits(AssistantTurnLimits::new(
            ROLE_RUN_MAX_ASSISTANT_TEXT_BYTES,
            ROLE_RUN_MAX_REASONING_BYTES,
            ROLE_RUN_MAX_TOOL_ARGUMENT_BYTES,
            ROLE_RUN_MAX_TOOL_CALLS_PER_TURN,
            ROLE_RUN_MAX_PROVIDER_TURN_BYTES,
        ))
    } else {
        AssistantTurnAssembler::new()
    }
}

fn public_call_id(
    config: &HeadlessRunConfig,
    call_id: &str,
    turn_number: usize,
    call_index: usize,
) -> String {
    if is_role_run(config) {
        format!("role-call-{turn_number}-{call_index}")
    } else {
        call_id.to_string()
    }
}

fn execution_workspace(
    config: &HeadlessRunConfig,
) -> Result<crate::workspace::WorkspaceContext, crate::workspace::WorkspaceError> {
    Ok(config.workspace.clone())
}

async fn interruption_signal() {
    #[cfg(unix)]
    {
        if let Ok(mut terminate) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = terminate.recv() => {}
            }
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}

#[derive(Clone)]
struct RoleRunCancellation {
    cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    notify: std::sync::Arc<tokio::sync::Notify>,
}

impl RoleRunCancellation {
    fn new() -> Self {
        Self {
            cancelled: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            notify: std::sync::Arc::new(tokio::sync::Notify::new()),
        }
    }

    fn cancel(&self) {
        self.cancelled
            .store(true, std::sync::atomic::Ordering::SeqCst);
        self.notify.notify_waiters();
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(std::sync::atomic::Ordering::SeqCst)
    }

    fn flag(&self) -> std::sync::Arc<std::sync::atomic::AtomicBool> {
        self.cancelled.clone()
    }

    async fn wait(&self) {
        loop {
            let notified = self.notify.notified();
            if self.is_cancelled() {
                return;
            }
            notified.await;
        }
    }
}

struct RoleRunInterruptListener {
    cancellation: RoleRunCancellation,
    task: tokio::task::JoinHandle<()>,
}

impl RoleRunInterruptListener {
    async fn start() -> Self {
        let cancellation = RoleRunCancellation::new();
        let listener = cancellation.clone();
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            #[cfg(unix)]
            {
                let mut interrupt =
                    match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
                    {
                        Ok(signal) => signal,
                        Err(_) => {
                            let _ = ready_tx.send(());
                            listener.cancel();
                            return;
                        }
                    };
                let mut terminate =
                    match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                    {
                        Ok(signal) => signal,
                        Err(_) => {
                            let _ = ready_tx.send(());
                            listener.cancel();
                            return;
                        }
                    };
                let _ = ready_tx.send(());
                tokio::select! {
                    _ = interrupt.recv() => {}
                    _ = terminate.recv() => {}
                }
                listener.cancel();
            }
            #[cfg(not(unix))]
            {
                let _ = ready_tx.send(());
                let _ = tokio::signal::ctrl_c().await;
                listener.cancel();
            }
        });
        let _ = ready_rx.await;
        Self { cancellation, task }
    }
}

impl Drop for RoleRunInterruptListener {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn wait_for_interruption(role_run: Option<&RoleRunCancellation>) {
    if let Some(role_run) = role_run {
        role_run.wait().await;
    } else {
        interruption_signal().await;
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

async fn execute_call(
    config: &HeadlessRunConfig,
    call: &AssembledToolCall,
    role_run_cancellation: Option<&RoleRunCancellation>,
) -> Result<ToolExecutionResult, CallDecisionError> {
    execute_call_with_discovery(config, call, role_run_cancellation, None, None).await
}

async fn prepare_headless_native_read(
    config: &HeadlessRunConfig,
    call: &AssembledToolCall,
    workspace: &crate::workspace::WorkspaceContext,
) -> Option<crate::agent::tools::PreparedNativeRead> {
    if is_role_run(config)
        || !ToolRegistry::builtins().supports_native_read(&call.name, config.tool_profile)
        || (config.tool_constraints.transparent_mechanistic_enabled
            && crate::mechanistic_delegation::is_transparent_tool(&call.name))
        || (config.tool_constraints.automation_setup_enabled
            && crate::mechanistic_delegation::is_automation_setup_tool(&call.name))
    {
        return None;
    }
    if let Some(manager) = &config.mcp {
        if manager.has_active_tool(&call.name).await {
            return None;
        }
    }
    let arguments: serde_json::Value = serde_json::from_str(&call.arguments).ok()?;
    let broker = crate::approval::ApprovalBroker::with_policy(
        workspace,
        &config.approval_rules,
        &config.permission_rules,
        &config.session_grants,
        crate::approval::ApprovalOrigin::Main,
    );
    let (prepared, decision) = broker
        .prepare(
            &call.name,
            &arguments,
            crate::agent::registry::ToolOrigin::BuiltIn,
        )
        .ok()?;
    let allowed = decision == crate::approval::BrokerDecision::AllowedByRule
        || (decision == crate::approval::BrokerDecision::Prompt
            && (config.approval_mode == HeadlessApprovalMode::Yolo
                || (config.approval_mode == HeadlessApprovalMode::AutoReview
                    && crate::permission::auto_review_safe_invocation(
                        &call.name,
                        &prepared.arguments,
                    ))));
    (allowed && !prepared.network_requested).then_some(crate::agent::tools::PreparedNativeRead {
        name: call.name.clone(),
        args: prepared.arguments,
    })
}

async fn execute_call_with_discovery(
    config: &HeadlessRunConfig,
    call: &AssembledToolCall,
    role_run_cancellation: Option<&RoleRunCancellation>,
    discovery: Option<&crate::tool_discovery::ToolDiscoverySession>,
    retained_output_cap: Option<usize>,
) -> Result<ToolExecutionResult, CallDecisionError> {
    if role_run_cancellation.is_some_and(RoleRunCancellation::is_cancelled) {
        return Err(CallDecisionError::Interrupted);
    }
    let raw_arguments: serde_json::Value = serde_json::from_str(&call.arguments)
        .map_err(|error| CallDecisionError::Protocol(error.to_string()))?;
    if is_role_run(config)
        && config
            .output_redactions
            .value_contains_secret(&raw_arguments)
    {
        return Err(CallDecisionError::Protocol(
            "role-run tool arguments contain protected route material".into(),
        ));
    }
    if call.name == "delegate_mechanistic" {
        if !config.tool_constraints.mechanistic_delegation_enabled {
            return Ok(ToolExecutionResult::Failure(
                "Mechanistic delegation is unavailable for this run".into(),
            ));
        }
        let Some(manager) = &config.mcp else {
            return Ok(ToolExecutionResult::Failure(
                "Mechanistic delegation has no MCP manager".into(),
            ));
        };
        return Ok(crate::mechanistic_delegation::execute(manager.clone(), &raw_arguments).await);
    }
    if let Some(manager) = &config.mcp {
        if manager.has_active_tool(&call.name).await {
            manager
                .validate_active_tool(&call.name, &raw_arguments)
                .await
                .map_err(CallDecisionError::Protocol)?;
            let workspace = execution_workspace(config)
                .map_err(|error| CallDecisionError::Protocol(error.to_string()))?;
            let server = manager
                .active_tool_server(&call.name)
                .await
                .unwrap_or_else(|| "unknown".into());
            let broker = crate::approval::ApprovalBroker::with_policy(
                &workspace,
                &config.approval_rules,
                &config.permission_rules,
                &config.session_grants,
                crate::approval::ApprovalOrigin::Mcp(server.clone()),
            );
            let (_, decision) = broker.prepare_dynamic(
                &call.name,
                &raw_arguments,
                crate::agent::registry::ToolOrigin::Mcp(server),
                false,
            );
            if decision == crate::approval::BrokerDecision::DeniedByRule {
                return Ok(ToolExecutionResult::Denied(
                    "Denied by persistent approval rule".into(),
                ));
            }
            match decision {
                crate::approval::BrokerDecision::AllowedByRule => {}
                crate::approval::BrokerDecision::DeniedByRule => unreachable!(),
                crate::approval::BrokerDecision::ExplicitPrompt => {
                    return Err(CallDecisionError::Approval(
                        "an explicit Ask rule requires interactive approval".into(),
                    ));
                }
                crate::approval::BrokerDecision::Prompt
                    if config.approval_mode == HeadlessApprovalMode::Yolo => {}
                crate::approval::BrokerDecision::Prompt => {
                    return Err(CallDecisionError::Approval(
                        "no persistent allow rule matched for MCP tool".into(),
                    ));
                }
                crate::approval::BrokerDecision::NetworkPrompt => {
                    return Err(CallDecisionError::Approval(
                        "MCP network access requires a distinct network grant".into(),
                    ));
                }
            }
            return manager
                .call_tool(&call.name, raw_arguments)
                .await
                .map_err(|error| CallDecisionError::Protocol(error.to_string()));
        }
    }
    let registry = ToolRegistry::builtins();
    let spec = registry
        .get(&call.name)
        .ok_or_else(|| CallDecisionError::Protocol(format!("unknown tool: {}", call.name)))?;
    if (config.tool_constraints.transparent_mechanistic_enabled
        && crate::mechanistic_delegation::is_transparent_tool(&call.name))
        || (config.tool_constraints.automation_setup_enabled
            && crate::mechanistic_delegation::is_automation_setup_tool(&call.name))
    {
        registry
            .validate(&call.name, &raw_arguments)
            .map_err(|error| CallDecisionError::Protocol(error.to_string()))?;
        let manager = config.mcp.as_ref().ok_or_else(|| {
            CallDecisionError::Protocol("mechanistic routing has no MCP manager".into())
        })?;
        let binding = crate::mechanistic_delegation::repository_binding(manager)
            .await
            .map_err(|error| CallDecisionError::Protocol(error.to_string()))?;
        let pinned_workspace = crate::mechanistic_delegation::binding_workspace(&binding)
            .map_err(|error| CallDecisionError::Protocol(error.to_string()))?;
        let route_broker = crate::approval::ApprovalBroker::with_policy(
            &pinned_workspace,
            &config.approval_rules,
            &config.permission_rules,
            &config.session_grants,
            crate::approval::ApprovalOrigin::Main,
        );
        let (_, decision) = route_broker
            .prepare_with_combined_network_decision(
                &call.name,
                &raw_arguments,
                crate::agent::registry::ToolOrigin::BuiltIn,
                true,
            )
            .map_err(CallDecisionError::Protocol)?;
        match decision {
            crate::approval::BrokerDecision::AllowedByRule => {}
            crate::approval::BrokerDecision::DeniedByRule => {
                return Ok(ToolExecutionResult::Denied(
                    "Denied by persistent approval rule".into(),
                ));
            }
            crate::approval::BrokerDecision::Prompt
                if config.approval_mode == HeadlessApprovalMode::Yolo => {}
            crate::approval::BrokerDecision::Prompt
            | crate::approval::BrokerDecision::ExplicitPrompt => {
                return Err(CallDecisionError::Approval(
                    "transparent repository tool requires interactive approval".into(),
                ));
            }
            crate::approval::BrokerDecision::NetworkPrompt => {
                return Err(CallDecisionError::Approval(
                    "transparent repository tools cannot request network access".into(),
                ));
            }
        }
        let request = crate::mechanistic_delegation::TransparentToolRequest {
            name: call.name.clone(),
            arguments: raw_arguments.clone(),
        };
        match crate::mechanistic_delegation::execute_transparent_batch(
            manager.clone(),
            std::slice::from_ref(&request),
            config.mechanistic.allow_unisolated_evidence,
        )
        .await
        {
            Ok(mut batch) => {
                if let Some(capsule) = batch.capsule.as_ref() {
                    let _ = crate::mechanistic_delegation::record_process_event(
                        crate::mechanistic_delegation::MechanisticProcessEvent {
                            kind: crate::mechanistic_delegation::MechanisticEventKind::DelegationCompletion,
                            origin: &capsule.digest,
                            revision: Some(&capsule.revision),
                            action_digest: Some(&capsule.action_digest),
                        },
                    );
                }
                if let Some(capsule) = batch.capsule.take() {
                    crate::mechanistic_delegation::publish_process_capsule(capsule);
                }
                return Ok(batch.executions.pop().unwrap_or_else(|| {
                    ToolExecutionResult::Failure(
                        "Mechanistic routing returned no tool result".into(),
                    )
                }));
            }
            Err(crate::mechanistic_delegation::TransparentRoutingError::Unavailable {
                message,
                binding,
            }) => {
                let workspace = execution_workspace(config)
                    .map_err(|error| CallDecisionError::Protocol(error.to_string()))?;
                let fallback = matches!(
                    call.name.as_str(),
                    "read_file" | "search_files" | "list_dir" | "glob"
                ) && binding.as_ref().is_some_and(|binding| {
                    crate::mechanistic_delegation::binding_matches_workspace(binding, &workspace)
                });
                if !fallback {
                    return Ok(ToolExecutionResult::Failure(message));
                }
                crate::mechanistic_delegation::mark_transparent_routing_degraded(&message);
            }
            Err(error) => return Ok(ToolExecutionResult::Failure(error.to_string())),
        }
    }
    if spec.capabilities.contains(&ToolCapability::Privilege) {
        return Err(CallDecisionError::Approval(
            "privilege handoff is unavailable outside the interactive TUI".into(),
        ));
    }
    let workspace = execution_workspace(config)
        .map_err(|error| CallDecisionError::Protocol(error.to_string()))?;
    let raw_arguments = if is_role_run(config) {
        crate::role_run::validate_role_run_tool_invocation(
            workspace.cwd(),
            &call.name,
            &raw_arguments,
        )
        .map_err(|_| CallDecisionError::Protocol("role-run tool invocation was rejected".into()))?
    } else {
        raw_arguments
    };
    let broker = crate::approval::ApprovalBroker::with_policy(
        &workspace,
        &config.approval_rules,
        &config.permission_rules,
        &config.session_grants,
        crate::approval::ApprovalOrigin::Main,
    );
    let (prepared, broker_decision) = broker
        .prepare(
            &call.name,
            &raw_arguments,
            crate::agent::registry::ToolOrigin::BuiltIn,
        )
        .map_err(CallDecisionError::Protocol)?;
    let arguments = prepared.arguments;

    if broker_decision == crate::approval::BrokerDecision::DeniedByRule {
        return Ok(ToolExecutionResult::Denied(
            "Denied by persistent approval rule".into(),
        ));
    }

    if is_role_run(config)
        && !crate::role_run::role_run_process_grants_authorize(
            workspace.cwd(),
            &config.session_grants,
            &call.name,
            &arguments,
        )
    {
        return Err(CallDecisionError::Approval(
            "role-run process grant did not authorize this invocation".into(),
        ));
    }

    if crate::email::is_outbound_tool(&call.name) {
        return Err(CallDecisionError::Approval(
            "external communication requires an exact interactive one-use approval".into(),
        ));
    }

    if spec.capabilities.contains(&ToolCapability::Subagent) {
        let Some(manager) = &config.agents else {
            return Ok(ToolExecutionResult::Failure(
                "headless subagent manager is unavailable".into(),
            ));
        };
        return crate::agent::subagent::execute_manager_tool(
            &call.name,
            &arguments,
            manager,
            &config.work_state,
            &workspace,
        )
        .await
        .ok_or_else(|| {
            CallDecisionError::Protocol(format!("unknown subagent tool: {}", call.name))
        });
    }

    if spec.capabilities.contains(&ToolCapability::Interaction) {
        return Err(CallDecisionError::Approval(
            "headless interaction is unavailable".into(),
        ));
    }
    if prepared.network_requested && !config.allow_network {
        return Err(CallDecisionError::Approval(
            "network access was not enabled with --allow-network".into(),
        ));
    }
    match broker_decision {
        crate::approval::BrokerDecision::AllowedByRule
        | crate::approval::BrokerDecision::NetworkPrompt => {}
        crate::approval::BrokerDecision::DeniedByRule => {
            unreachable!("persistent denial returned above")
        }
        crate::approval::BrokerDecision::ExplicitPrompt => {
            return Err(CallDecisionError::Approval(
                "an explicit Ask rule requires interactive approval".into(),
            ));
        }
        crate::approval::BrokerDecision::Prompt
            if call.name == crate::skills::ACTIVATE_SKILL_TOOL_NAME => {}
        crate::approval::BrokerDecision::Prompt => match config.approval_mode {
            HeadlessApprovalMode::Yolo => {}
            HeadlessApprovalMode::AutoReview
                if crate::permission::auto_review_safe_invocation(&call.name, &arguments) => {}
            HeadlessApprovalMode::RequireApproval | HeadlessApprovalMode::AutoReview => {
                return Err(CallDecisionError::Approval(
                    "no persistent allow rule matched".into(),
                ));
            }
        },
    }

    if is_role_run(config) {
        if role_run_cancellation.is_some_and(RoleRunCancellation::is_cancelled) {
            return Err(CallDecisionError::Interrupted);
        }
        let cancellation = role_run_cancellation
            .map(RoleRunCancellation::flag)
            .unwrap_or_else(|| std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)));
        if matches!(call.name.as_str(), "write_file" | "apply_patch") {
            let workspace_identity = config.role_run_workspace_identity.ok_or_else(|| {
                CallDecisionError::Protocol("role-run workspace identity is unavailable".into())
            })?;
            return Ok(crate::role_run::execute_role_run_mutation(
                workspace.cwd(),
                workspace_identity,
                cancellation,
                config.output_redactions.clone(),
                &call.name,
                &arguments,
            )
            .await);
        }
        if call.name == "blame" {
            let workspace_identity = config.role_run_workspace_identity.ok_or_else(|| {
                CallDecisionError::Protocol("role-run workspace identity is unavailable".into())
            })?;
            return Ok(crate::role_run::execute_role_run_blame(
                workspace.cwd(),
                workspace_identity,
                cancellation,
                &arguments,
                config.command_timeout_secs,
            )
            .await);
        }
        if matches!(
            call.name.as_str(),
            "read_file" | "list_dir" | "search_files" | "glob" | "read_todos"
        ) {
            let workspace_identity = config.role_run_workspace_identity.ok_or_else(|| {
                CallDecisionError::Protocol("role-run workspace identity is unavailable".into())
            })?;
            return Ok(crate::role_run::execute_role_run_read(
                workspace.cwd(),
                workspace_identity,
                cancellation,
                &call.name,
                &arguments,
            )
            .await);
        }
    }

    if call.name == crate::tool_discovery::SEARCH_TOOLS_NAME {
        return Ok(match discovery {
            Some(discovery) => discovery.search(&arguments),
            None => ToolExecutionResult::Failure(
                "Built-in discovery is unavailable for this run".into(),
            ),
        });
    }

    if call.name == "search_mcp_tools" {
        let Some(manager) = &config.mcp else {
            return Ok(ToolExecutionResult::Failure(
                "MCP discovery is unavailable for this run".into(),
            ));
        };
        let query = arguments
            .get("query")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        let server = arguments.get("server").and_then(serde_json::Value::as_str);
        let limit = arguments
            .get("limit")
            .and_then(serde_json::Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .unwrap_or(5);
        return Ok(manager.search_tools(query, server, limit).await);
    }

    if crate::lsp::is_lsp_tool(&call.name) {
        let Some(manager) = &config.lsp else {
            return Ok(ToolExecutionResult::Failure(
                "LSP navigation is unavailable for this run".into(),
            ));
        };
        manager.update_workspace(workspace.clone()).await;
        return Ok(manager.execute(&call.name, &arguments).await);
    }

    if crate::email::is_email_tool(&call.name) {
        let Some(manager) = &config.email else {
            return Ok(ToolExecutionResult::Failure(
                "email tools are unavailable for this run".into(),
            ));
        };
        return Ok(manager.execute_read_or_draft(&call.name, &arguments).await);
    }

    if call.name == "run_command"
        || ToolRegistry::builtins().supports_native_read(&call.name, config.tool_profile)
    {
        if let Some(cap) = retained_output_cap {
            return Ok(crate::agent::tools::execute_prepared_tool_in_workspace_policy_owned_with_job_policy(
                &call.name, &arguments, &workspace, config.command_timeout_secs,
                config.sandbox_mode, config.allow_network && prepared.network_requested,
                config.job_owner, crate::jobs::JobExecutionPolicy::default().with_output_limit(cap),
            ).await);
        }
    }
    Ok(
        crate::agent::tools::execute_prepared_tool_in_workspace_policy_owned_with_skills(
            &call.name,
            &arguments,
            &workspace,
            config.command_timeout_secs,
            config.sandbox_mode,
            config.allow_network && prepared.network_requested,
            config.job_owner,
            &config.skills,
        )
        .await,
    )
}

#[derive(Debug)]
enum CallDecisionError {
    Approval(String),
    Protocol(String),
    Interrupted,
}

pub async fn run_with_backend<B: ChatBackend + ?Sized>(
    backend: &B,
    messages: Vec<ChatMessage>,
    config: HeadlessRunConfig,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> HeadlessExit {
    let sink = config.outcome_sink.clone();
    let exit = run_with_backend_inner(backend, messages, config, stdout, stderr).await;
    if let Some(sink) = sink {
        sink.flush_async().await;
    }
    exit
}

async fn run_with_backend_inner<B: ChatBackend + ?Sized>(
    backend: &B,
    mut messages: Vec<ChatMessage>,
    config: HeadlessRunConfig,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> HeadlessExit {
    // Role-run installs one process-signal receiver for the entire run and
    // latches its state. Recreating receivers around individual awaits leaves
    // synchronous parsing/validation gaps in which Tokio can consume a signal
    // without any live receiver.
    let role_run_interrupt = if is_role_run(&config) {
        Some(RoleRunInterruptListener::start().await)
    } else {
        None
    };
    let role_run_cancellation = role_run_interrupt
        .as_ref()
        .map(|listener| &listener.cancellation);
    let mut emitter = HeadlessEmitter::new_for_contract(
        config.output_format,
        config.run_id,
        stdout,
        stderr,
        config.uses_v2(),
        config.output_redactions.clone(),
    );
    let automation_dispatch = config
        .automation_policy
        .as_ref()
        .map(|policy| std::sync::Arc::new(std::sync::Mutex::new(policy.start())));
    emitter.automation_dispatch = automation_dispatch.clone();
    if let Some(sink) = config.outcome_sink.clone() {
        let mut tracker = crate::session_outcome::OutcomeTracker::for_run(
            config.run_id.to_string(),
            config.run_id,
            &config.cwd.to_string_lossy(),
        )
        .with_sink(sink);
        tracker.set_model(Some(config.model.clone()), Some(config.provider.clone()));
        tracker.record_user_message();
        if let Some(manager) = &config.agents {
            manager.begin_parent_task(&tracker);
        }
        emitter.outcome = Some(crate::session_outcome::ScopedOutcome(tracker));
    }
    macro_rules! emit_or_pipe {
        ($event:expr) => {
            if let Err(error) = emitter.emit($event) {
                return match error {
                    EmitError::BrokenPipe => HeadlessExit::SUCCESS,
                    EmitError::Other(_) => HeadlessExit { code: 8 },
                };
            }
        };
    }
    macro_rules! automation_limit {
        ($method:ident $(, $argument:expr)*) => {
            if let Some(dispatch) = &automation_dispatch {
                let result = dispatch.lock().unwrap_or_else(|poison|poison.into_inner()).$method($($argument),*);
                if let Err(message) = result {
                    let _ = crate::jobs::JobManager::process()
                        .shutdown_model_background_jobs_owned(config.job_owner)
                        .await;
                    emit_or_pipe!(HeadlessEventV1::Failure {kind:HeadlessFailureKind::MaximumTurns,message,exit_code:7});
                    return finish_with_code(emitter,7);
                }
            }
        };
    }
    let _workspace_activity = if let Some(scope) = config.workspace_activity_scope.clone() {
        let roots = config
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
                WorkspaceActivityGuard::new(Some(lease), config.agents.clone(), config.job_owner)
            }
            Err(message) => {
                emit_or_pipe!(HeadlessEventV1::Failure {
                    kind: HeadlessFailureKind::Setup,
                    message,
                    exit_code: 3
                });
                return finish_with_code(emitter, 3);
            }
        }
    } else {
        WorkspaceActivityGuard::new(None, None, config.job_owner)
    };
    macro_rules! exit_if_role_run_interrupted {
        () => {
            if role_run_cancellation.is_some_and(RoleRunCancellation::is_cancelled) {
                emit_or_pipe!(HeadlessEventV1::Failure {
                    kind: HeadlessFailureKind::Interrupted,
                    message: "interrupted".into(),
                    exit_code: 130,
                });
                return finish_with_code(emitter, 130);
            }
        };
    }
    if config.uses_v2() && !config.tool_constraints.tools_supported {
        emit_or_pipe!(HeadlessEventV1::Failure {
            kind: HeadlessFailureKind::Input,
            message: "--check and --output-schema require a model with tool support".into(),
            exit_code: 2,
        });
        return finish_with_code(emitter, 2);
    }
    if config.automation_policy.is_some()
        && (config.check_mode.enabled() || config.convergence.is_some())
    {
        emit_or_pipe!(HeadlessEventV1::Failure {
            kind: HeadlessFailureKind::Input,
            message: "Budgeted automation does not support verification or convergent review; use an explicitly budgeted workflow step".into(),
            exit_code: 2,
        });
        return finish_with_code(emitter, 2);
    }
    emit_or_pipe!(HeadlessEventV1::RunStart {
        model: config.model.clone()
    });
    for warning in &config.startup_warnings {
        emit_or_pipe!(HeadlessEventV1::Warning {
            message: warning.clone(),
        });
    }
    exit_if_role_run_interrupted!();
    let mut prompted_skills: std::collections::HashSet<String> = config
        .skills
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .active_refs()
        .into_iter()
        .map(|reference| reference.id)
        .collect();
    let mut loop_guard = crate::loop_guard::LoopGuard::default();
    let automatic_mechanistic_enabled = config.mechanistic.enabled
        && config.tool_constraints.mechanistic_delegation_enabled
        && !is_role_run(&config);
    let automatic_preflight_context = if automatic_mechanistic_enabled
        && config.mechanistic.automatic_preflight
    {
        match &config.mcp {
            Some(manager) => {
                crate::mechanistic_delegation::automatic_preflight_context(
                    manager.clone(),
                    &config.original_request,
                    std::time::Duration::from_secs(config.mechanistic.preflight_timeout_seconds),
                )
                .await
            }
            None => None,
        }
    } else {
        None
    };
    let verification_suppressed =
        crate::mechanistic_delegation::automatic_verification_suppressed(&config.original_request);
    let allow_unscoped_full =
        crate::mechanistic_delegation::automatic_release_requested(&config.original_request)
            || config.mechanistic.full_suite
                == crate::config::MechanisticFullSuitePolicy::ReleaseOrUnscoped;
    let mut pending_verification_context: Option<String> = None;
    let mut pending_capsule_context: Option<String> = None;
    let mut verification_failed = false;
    let mut repair_passes = 0_u8;
    let discovery = crate::tool_discovery::ToolDiscoverySession::default();

    for turn_number in 1..=config.max_turns {
        exit_if_role_run_interrupted!();
        emit_or_pipe!(HeadlessEventV1::TurnStart { turn: turn_number });
        let catalog_registry = if let Some(manager) = &config.mcp {
            manager.active_catalog().await.registry
        } else {
            None
        };
        let registry = catalog_registry
            .as_ref()
            .unwrap_or_else(|| ToolRegistry::builtins());
        let mut tool_constraints = config.tool_constraints.clone();
        if config.automation_policy.is_some() {
            tool_constraints.images_supported = false;
        }
        let mut turn_tools = match crate::tool_selection::resolve_tools(
            registry,
            config.tool_profile,
            &config.tool_policy,
            &tool_constraints,
        ) {
            Ok(tools) => tools.into_definitions(),
            Err(error) => {
                emit_or_pipe!(HeadlessEventV1::Failure {
                    kind: HeadlessFailureKind::Input,
                    message: format!("tool selection failed: {error}"),
                    exit_code: 2,
                });
                return finish_with_code(emitter, 2);
            }
        };
        turn_tools.retain(|tool| {
            headless_surface_tool_allowed(&tool.function.name)
                && (!matches!(tool.function.name.as_str(), "task" | "spawn_agent")
                    || !subagent_tool_hard_denied(&config, &tool.function.name))
        });
        turn_tools = discovery.advertise(turn_tools, &config.tool_policy);
        if turn_tools.iter().any(|tool| {
            matches!(
                tool.function.name.as_str(),
                crate::headless_contract::STRUCTURED_OUTPUT_TOOL
                    | crate::headless_contract::VERIFICATION_TOOL
            )
        }) {
            emit_or_pipe!(HeadlessEventV1::Failure {
                kind: HeadlessFailureKind::Input,
                message: "selected tools collide with a reserved headless contract tool".into(),
                exit_code: 2,
            });
            return finish_with_code(emitter, 2);
        }
        if let Some(contract) = &config.structured_output {
            turn_tools.push(contract.tool_definition());
        }
        let advertised_tool_names = turn_tools
            .iter()
            .map(|tool| tool.function.name.clone())
            .collect::<std::collections::HashSet<_>>();
        let stable_prefix_messages = messages
            .iter()
            .take_while(|message| matches!(message.role.as_str(), "system" | "developer"))
            .count();
        let mut request_messages = messages.clone();
        if turn_number == 1 {
            if let Some(context) = automatic_preflight_context.as_ref() {
                request_messages.push(ChatMessage {
                    role: "user".into(),
                    content: crate::image_content::ModelContent::text(context.clone()),
                    tool_calls: None,
                    tool_call_id: None,
                    response_items: Vec::new(),
                });
            }
        }
        if let Some(context) = pending_verification_context.take() {
            request_messages.push(ChatMessage {
                role: "user".into(),
                content: crate::image_content::ModelContent::text(context),
                tool_calls: None,
                tool_call_id: None,
                response_items: Vec::new(),
            });
        }
        if let Some(context) = pending_capsule_context.take() {
            request_messages.push(ChatMessage {
                role: "user".into(),
                content: crate::image_content::ModelContent::text(context),
                tool_calls: None,
                tool_call_id: None,
                response_items: Vec::new(),
            });
        }
        let prepared = crate::request::PreparedModelRequest::new(
            config.request_kind,
            config.provider.clone(),
            config.model.clone(),
            request_messages,
            turn_tools,
            config.chat_options.clone(),
        )
        .with_prompt_cache(
            crate::request::PromptCacheScope::conversation(config.run_id),
            stable_prefix_messages,
        );
        let prepared = backend.prepare_harness_request(prepared).await;
        let reservation = if let Some(dispatch) = &automation_dispatch {
            let result = dispatch
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .reserve_request(&prepared);
            match result {
                Ok(reservation) => Some(reservation),
                Err(message) => {
                    emit_or_pipe!(HeadlessEventV1::Failure {
                        kind: HeadlessFailureKind::MaximumTurns,
                        message,
                        exit_code: 7
                    });
                    return finish_with_code(emitter, 7);
                }
            }
        } else {
            None
        };
        let request_deadline = automation_dispatch.as_ref().map(|dispatch| {
            let remaining = dispatch
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .remaining();
            tokio::time::Instant::now()
                + remaining.min(std::time::Duration::from_secs(
                    config
                        .automation_policy
                        .as_ref()
                        .expect("owned automation policy")
                        .request_seconds(),
                ))
        });
        if let Some(tracker) = &mut emitter.outcome {
            tracker.set_runtime_harness(crate::harness_registry::capture_prepared(&prepared));
        }
        let stream_result = tokio::select! {
            result = backend.stream_prepared(&prepared) => Some(result),
            _ = wait_for_interruption(role_run_cancellation) => None,
            _ = async {if let Some(deadline)=request_deadline {tokio::time::sleep_until(deadline).await;} else {std::future::pending::<()>().await;}} => {
                emit_or_pipe!(HeadlessEventV1::Failure {kind:HeadlessFailureKind::MaximumTurns,message:"Automation provider deadline exhausted".into(),exit_code:7}); return finish_with_code(emitter,7);
            },
        };
        let mut stream = match stream_result {
            None => {
                emit_or_pipe!(HeadlessEventV1::Failure {
                    kind: HeadlessFailureKind::Interrupted,
                    message: "interrupted".into(),
                    exit_code: 130,
                });
                return finish_with_code(emitter, 130);
            }
            Some(Ok(stream)) => stream,
            Some(Err(error)) => {
                let (code, kind) = provider_failure(&error);
                emit_or_pipe!(HeadlessEventV1::Failure {
                    kind,
                    message: if is_role_run(&config) {
                        "role-run provider request failed".into()
                    } else {
                        error.to_string()
                    },
                    exit_code: code,
                });
                return finish_with_code(emitter, code);
            }
        };
        let mut assembler = assistant_turn_assembler(&config);
        let mut stream_error = None;
        loop {
            let item = tokio::select! {
                item = stream.recv() => item,
                _ = async {if let Some(deadline)=request_deadline {tokio::time::sleep_until(deadline).await;} else {std::future::pending::<()>().await;}} => {
                    emit_or_pipe!(HeadlessEventV1::Failure {kind:HeadlessFailureKind::MaximumTurns,message:"Automation provider deadline exhausted".into(),exit_code:7}); return finish_with_code(emitter,7);
                },
                _ = wait_for_interruption(role_run_cancellation) => {
                    emit_or_pipe!(HeadlessEventV1::Failure {
                        kind: HeadlessFailureKind::Interrupted,
                        message: "interrupted".into(),
                        exit_code: 130,
                    });
                    return finish_with_code(emitter, 130);
                }
            };
            let Some(item) = item else {
                break;
            };
            match item {
                Ok(event) => {
                    automation_limit!(account_provider_event, &event);
                    if config.structured_output.is_none() {
                        if let ProviderEvent::Text(delta) = &event {
                            emit_or_pipe!(HeadlessEventV1::AssistantOutput {
                                delta: delta.clone()
                            });
                        }
                    }
                    if config.show_thinking {
                        if let ProviderEvent::Reasoning(delta) = &event {
                            emit_or_pipe!(HeadlessEventV1::ReasoningOutput {
                                delta: delta.clone()
                            });
                        }
                    }
                    if let ProviderEvent::Usage(usage)
                    | ProviderEvent::Finish {
                        usage: Some(usage), ..
                    } = &event
                    {
                        emit_or_pipe!(HeadlessEventV1::Usage {
                            usage: usage.clone()
                        });
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
                emit_or_pipe!(HeadlessEventV1::Failure {
                    kind,
                    message: if is_role_run(&config) {
                        "role-run provider response failed".into()
                    } else {
                        error.to_string()
                    },
                    exit_code: code,
                });
                return finish_with_code(emitter, code);
            }
        };
        if let (Some(dispatch), Some(reservation)) = (&automation_dispatch, reservation) {
            let result = dispatch
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .settle_request(reservation, turn.usage.as_ref());
            if let Err(message) = result {
                emit_or_pipe!(HeadlessEventV1::Failure {
                    kind: HeadlessFailureKind::MaximumTurns,
                    message,
                    exit_code: 7
                });
                return finish_with_code(emitter, 7);
            }
        }
        automation_limit!(reserve_tools, turn.tool_calls.len());
        let automation_tool_output_cap = automation_dispatch.as_ref().map(|dispatch| {
            dispatch
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .tool_output_cap()
        });
        exit_if_role_run_interrupted!();

        if let Some(contract) = &config.structured_output {
            let reports = turn
                .tool_calls
                .iter()
                .filter(|call| call.name == crate::headless_contract::STRUCTURED_OUTPUT_TOOL)
                .collect::<Vec<_>>();
            if !reports.is_empty() {
                exit_if_role_run_interrupted!();
                if reports.len() != 1 || turn.tool_calls.len() != 1 {
                    emit_or_pipe!(HeadlessEventV1::Failure {
                        kind: HeadlessFailureKind::ProviderProtocol,
                        message: "report_structured_output must be the sole final tool call".into(),
                        exit_code: 6,
                    });
                    return finish_with_code(emitter, 6);
                }
                let value = match contract.validate_arguments(&reports[0].arguments) {
                    Ok(value) => value,
                    Err(message) => {
                        emit_or_pipe!(HeadlessEventV1::Failure {
                            kind: HeadlessFailureKind::ProviderProtocol,
                            message: if is_role_run(&config) {
                                "role-run structured output failed local validation".into()
                            } else {
                                message
                            },
                            exit_code: 6,
                        });
                        return finish_with_code(emitter, 6);
                    }
                };
                let encoded = match serde_json::to_string(&value) {
                    Ok(encoded) => encoded,
                    Err(error) => {
                        emit_or_pipe!(HeadlessEventV1::Failure {
                            kind: HeadlessFailureKind::Internal,
                            message: format!("could not encode structured output: {error}"),
                            exit_code: 8,
                        });
                        return finish_with_code(emitter, 8);
                    }
                };
                if let Some(path) = config.output_last_message.as_deref() {
                    if let Err(error) = atomic_write(path, &encoded) {
                        emit_or_pipe!(HeadlessEventV1::Failure {
                            kind: HeadlessFailureKind::Internal,
                            message: format!("failed to write last message: {error}"),
                            exit_code: 8,
                        });
                        return finish_with_code(emitter, 8);
                    }
                }
                emitter.record_structured_output(value.clone());
                if let Err(error) = emitter
                    .emit_v2(crate::headless_contract::HeadlessEventV2::StructuredOutput { value })
                {
                    return match error {
                        EmitError::BrokenPipe => HeadlessExit::SUCCESS,
                        EmitError::Other(_) => HeadlessExit { code: 8 },
                    };
                }
                if config.check_mode.enabled() {
                    return finish_after_check(backend, &config, encoded, emitter, prompted_skills)
                        .await;
                }
                exit_if_role_run_interrupted!();
                let active = reap_incomplete_subagents(&config).await;
                if active > 0 {
                    emit_or_pipe!(HeadlessEventV1::Failure {
                        kind: HeadlessFailureKind::IncompleteSubagents,
                        message: format!(
                            "headless run completed with {active} active subagent(s); they were cancelled and reaped"
                        ),
                        exit_code: 8,
                    });
                    return finish_with_code(emitter, 8);
                }
                exit_if_role_run_interrupted!();
                if let Err(error) = emitter.complete(encoded) {
                    return match error {
                        EmitError::BrokenPipe => HeadlessExit::SUCCESS,
                        EmitError::Other(_) => HeadlessExit { code: 8 },
                    };
                }
                return match emitter.finish() {
                    Ok(()) | Err(EmitError::BrokenPipe) => HeadlessExit::SUCCESS,
                    Err(EmitError::Other(_)) => HeadlessExit { code: 8 },
                };
            }
        }

        if turn.tool_calls.is_empty() {
            if verification_failed {
                emit_or_pipe!(HeadlessEventV1::Failure {
                    kind: HeadlessFailureKind::Verification,
                    message: "codex-team verification remains unresolved".into(),
                    exit_code: 10,
                });
                return finish_with_code(emitter, 10);
            }
            if config.structured_output.is_some() {
                emit_or_pipe!(HeadlessEventV1::Failure {
                    kind: HeadlessFailureKind::ProviderProtocol,
                    message: "structured output response did not call report_structured_output"
                        .into(),
                    exit_code: 6,
                });
                return finish_with_code(emitter, 6);
            }
            if config.check_mode.enabled() {
                return finish_after_check(
                    backend,
                    &config,
                    turn.content,
                    emitter,
                    prompted_skills,
                )
                .await;
            }
            let job_manager = crate::jobs::JobManager::process();
            let job_owner = config.job_owner;
            let mut completed_jobs = job_manager
                .claim_completed_model_jobs_owned(job_owner)
                .await;
            let active_jobs = job_manager
                .active_model_background_jobs_owned(job_owner)
                .await;
            if config.single_turn && (!completed_jobs.is_empty() || !active_jobs.is_empty()) {
                let active_count = active_jobs.len();
                let completed_count = completed_jobs.len();
                let stopped = job_manager
                    .shutdown_model_background_jobs_owned(job_owner)
                    .await;
                emit_or_pipe!(HeadlessEventV1::ToolFinish {
                    call_id: "host-background-jobs".into(),
                    name: "background_jobs".into(),
                    status: if active_count > 0 {
                        ToolResultStatus::Cancelled
                    } else {
                        ToolResultStatus::Success
                    },
                    output: format!(
                        "single-turn skipped automatic delivery for {completed_count} completed job(s) and shut down {}/{} active job(s)",
                        stopped.len(),
                        active_count
                    ),
                });
                completed_jobs.clear();
            } else if !config.single_turn && (!completed_jobs.is_empty() || !active_jobs.is_empty())
            {
                if completed_jobs.is_empty() {
                    let background_limit = automation_dispatch.as_ref().map_or(
                        std::time::Duration::from_secs(1_800),
                        |dispatch| {
                            dispatch
                                .lock()
                                .unwrap_or_else(|poison| poison.into_inner())
                                .remaining()
                        },
                    );
                    let waited = tokio::select! {
                        waited = tokio::time::timeout(background_limit, async {
                            loop {
                                if job_manager
                                    .active_model_background_jobs_owned(job_owner)
                                    .await
                                    .is_empty()
                                {
                                    break;
                                }
                                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                            }
                        }) => Some(waited),
                        _ = wait_for_interruption(role_run_cancellation) => None,
                    };
                    if !matches!(waited, Some(Ok(()))) {
                        let stopped = job_manager
                            .shutdown_model_background_jobs_owned(job_owner)
                            .await
                            .len();
                        let (kind, reason, code) = if waited.is_none() {
                            (HeadlessFailureKind::Interrupted, "interrupted", 130)
                        } else if automation_dispatch.is_some() {
                            (
                                HeadlessFailureKind::MaximumTurns,
                                "Automation background-job runtime budget exhausted",
                                7,
                            )
                        } else {
                            (
                                HeadlessFailureKind::Internal,
                                "background-job wait exceeded 1800 seconds",
                                8,
                            )
                        };
                        emit_or_pipe!(HeadlessEventV1::Failure {
                            kind,
                            message: format!("{reason}; {stopped} job(s) were shut down"),
                            exit_code: code,
                        });
                        return finish_with_code(emitter, code);
                    }
                    completed_jobs = job_manager
                        .claim_completed_model_jobs_owned(job_owner)
                        .await;
                }
                if !completed_jobs.is_empty() {
                    let assistant = Message {
                        role: Role::Assistant,
                        content: turn.content.clone(),
                        tool_calls: None,
                        thinking: turn.reasoning.clone(),
                        response_items: turn.response_items.clone(),
                        timestamp: chrono::Utc::now(),
                    };
                    if let Err(error) =
                        crate::agent::client::append_provider_messages(&mut messages, &assistant)
                    {
                        emit_or_pipe!(HeadlessEventV1::Failure {
                            kind: HeadlessFailureKind::Internal,
                            message: error.to_string(),
                            exit_code: 8,
                        });
                        return finish_with_code(emitter, 8);
                    }
                    messages.push(ChatMessage {
                        role: "system".into(),
                        content: crate::image_content::ModelContent::text(
                            model_job_completion_message(&completed_jobs),
                        ),
                        tool_calls: None,
                        tool_call_id: None,
                        response_items: Vec::new(),
                    });
                    continue;
                }
            }
            let active = reap_incomplete_subagents(&config).await;
            if active > 0 {
                emit_or_pipe!(HeadlessEventV1::Failure {
                    kind: HeadlessFailureKind::IncompleteSubagents,
                    message: format!(
                        "headless run completed with {active} active subagent(s); they were cancelled and reaped"
                    ),
                    exit_code: 8,
                });
                return finish_with_code(emitter, 8);
            }
            if let Some(path) = config.output_last_message.as_deref() {
                if let Err(error) = atomic_write(path, &turn.content) {
                    emit_or_pipe!(HeadlessEventV1::Failure {
                        kind: HeadlessFailureKind::Internal,
                        message: format!("failed to write last message: {error}"),
                        exit_code: 8,
                    });
                    return finish_with_code(emitter, 8);
                }
            }
            if let Err(error) = emitter.complete(turn.content) {
                return match error {
                    EmitError::BrokenPipe => HeadlessExit::SUCCESS,
                    EmitError::Other(_) => HeadlessExit { code: 8 },
                };
            }
            return match emitter.finish() {
                Ok(()) | Err(EmitError::BrokenPipe) => HeadlessExit::SUCCESS,
                Err(EmitError::Other(_)) => HeadlessExit { code: 8 },
            };
        }

        let mut unscoped_mutation = automatic_mechanistic_enabled
            && config.mechanistic.automatic_verification
            && turn.tool_calls.iter().any(|call| {
                !matches!(
                    call.name.as_str(),
                    "write_file" | "edit_file" | "hashline_edit" | "apply_patch" | "delete_file"
                ) && ToolRegistry::builtins().get(&call.name).is_none_or(|spec| {
                    spec.mutation == crate::agent::registry::ToolMutation::Mutating
                })
            });
        let repository_revision_before = if unscoped_mutation {
            match &config.mcp {
                Some(manager) => crate::mechanistic_delegation::repository_revision(manager)
                    .await
                    .ok(),
                None => None,
            }
        } else {
            None
        };
        let mut changed_paths = std::collections::BTreeSet::new();
        let mut completed_calls = Vec::new();
        let mut completed_images = std::collections::HashMap::new();
        let mut loop_incident = None;
        let discovery_batch = discovery.begin_batch();
        let mut native_results = std::collections::BTreeMap::new();
        let mut native_started = std::collections::HashSet::new();
        for (call_index, call) in turn.tool_calls.iter().enumerate() {
            if !advertised_tool_names.contains(&call.name) {
                emit_or_pipe!(HeadlessEventV1::Failure {
                    kind: HeadlessFailureKind::ProviderProtocol,
                    message: if is_role_run(&config) {
                        "role-run provider attempted an unavailable tool".into()
                    } else {
                        format!("provider attempted unavailable tool: {}", call.name)
                    },
                    exit_code: 6,
                });
                return finish_with_code(emitter, 6);
            }
            let arguments: serde_json::Value = match serde_json::from_str(&call.arguments) {
                Ok(arguments) => arguments,
                Err(error) => {
                    emit_or_pipe!(HeadlessEventV1::Failure {
                        kind: HeadlessFailureKind::ProviderProtocol,
                        message: if is_role_run(&config) {
                            "role-run provider supplied invalid tool arguments".into()
                        } else {
                            error.to_string()
                        },
                        exit_code: 6,
                    });
                    return finish_with_code(emitter, 6);
                }
            };
            let activation_id = (call.name == "activate_skill")
                .then(|| arguments.get("id").and_then(serde_json::Value::as_str))
                .flatten()
                .map(str::to_string);
            let event_arguments = if is_role_run(&config) {
                serde_json::json!({"redacted": true})
            } else if crate::email::is_email_tool(&call.name) {
                crate::email::metadata_only(&call.name, &arguments, None)
            } else {
                arguments
            };
            let event_call_id = public_call_id(&config, &call.id, turn_number, call_index);
            if !native_started.remove(&call_index) {
                emit_or_pipe!(HeadlessEventV1::ToolStart {
                    call_id: event_call_id.clone(),
                    name: call.name.clone(),
                    arguments: event_arguments,
                });
            }
            if !native_results.contains_key(&call_index) {
                let mut prepared = Vec::new();
                let batch_workspace = execution_workspace(&config).ok();
                for candidate in turn
                    .tool_calls
                    .iter()
                    .skip(call_index)
                    .take(crate::agent::tools::MAX_NATIVE_READ_CONCURRENCY)
                {
                    if !advertised_tool_names.contains(&candidate.name) {
                        break;
                    }
                    let Some(workspace) = batch_workspace.as_ref() else {
                        break;
                    };
                    let Some(read) =
                        prepare_headless_native_read(&config, candidate, workspace).await
                    else {
                        break;
                    };
                    prepared.push(read);
                }
                if prepared.len() > 1 {
                    if let Some(workspace) = batch_workspace {
                        for offset in 1..prepared.len() {
                            let candidate = &turn.tool_calls[call_index + offset];
                            emit_or_pipe!(HeadlessEventV1::ToolStart {
                                call_id: public_call_id(
                                    &config,
                                    &candidate.id,
                                    turn_number,
                                    call_index + offset
                                ),
                                name: candidate.name.clone(),
                                arguments: serde_json::from_str(&candidate.arguments)
                                    .expect("admitted read arguments parsed")
                            });
                            native_started.insert(call_index + offset);
                        }
                        let mut context = crate::agent::registry::ToolContext::native_read(
                            &workspace,
                            config.command_timeout_secs,
                            config.tool_profile,
                            config.sandbox_mode,
                            config.job_owner,
                        );
                        if let Some(cap) = automation_tool_output_cap {
                            context.job_execution_policy =
                                crate::jobs::JobExecutionPolicy::default().with_output_limit(cap);
                        }
                        let results = tokio::select! {
                            results = crate::agent::tools::execute_native_read_run(&prepared, &context) => results,
                            _ = async {if let Some(dispatch)=&automation_dispatch {let remaining=dispatch.lock().unwrap_or_else(|poison|poison.into_inner()).remaining();tokio::time::sleep(remaining).await;} else {std::future::pending::<()>().await;}} => {
                                emit_or_pipe!(HeadlessEventV1::Failure {kind:HeadlessFailureKind::MaximumTurns,message:"Automation tool runtime budget exhausted".into(),exit_code:7});return finish_with_code(emitter,7);
                            },
                            _ = interruption_signal() => {emit_or_pipe!(HeadlessEventV1::Failure {kind: HeadlessFailureKind::Interrupted, message: "interrupted".into(), exit_code: 130}); return finish_with_code(emitter, 130);}
                        };
                        for (offset, result) in results.into_iter().enumerate() {
                            native_results.insert(call_index + offset, result);
                        }
                    }
                }
            }
            let execution_result = if let Some(result) = native_results.remove(&call_index) {
                Some(Ok(result))
            } else if is_role_run(&config) {
                let mut execution = Box::pin(execute_call(&config, call, role_run_cancellation));
                tokio::select! {
                    result = &mut execution => Some(result),
                    _ = wait_for_interruption(role_run_cancellation) => {
                        // A running spawn_blocking closure is not cancelled by
                        // dropping its JoinHandle. Keep the role alive until
                        // the fd-bound tool observes the latched token and
                        // returns, so no mutation can outlive Interrupted.
                        let _ = execution.await;
                        None
                    },
                }
            } else {
                let operation = execute_call_with_discovery(
                    &config,
                    call,
                    None,
                    Some(&discovery),
                    automation_tool_output_cap,
                );
                let operation = async {
                    if call.name == "run_command" {
                        tokio::select! {
                            result = operation => Some(result),
                            _ = wait_for_interruption(role_run_cancellation) => None,
                        }
                    } else {
                        Some(operation.await)
                    }
                };
                if let Some(dispatch) = &automation_dispatch {
                    let remaining = dispatch
                        .lock()
                        .unwrap_or_else(|poison| poison.into_inner())
                        .remaining();
                    match tokio::time::timeout(remaining, operation).await {
                        Ok(result) => result,
                        Err(_) => {
                            let _ = crate::jobs::JobManager::process()
                                .shutdown_model_background_jobs_owned(config.job_owner)
                                .await;
                            emit_or_pipe!(HeadlessEventV1::Failure {
                                kind: HeadlessFailureKind::MaximumTurns,
                                message: "Automation tool runtime budget exhausted".into(),
                                exit_code: 7
                            });
                            return finish_with_code(emitter, 7);
                        }
                    }
                } else {
                    operation.await
                }
            };
            let Some(execution_result) = execution_result else {
                let _ = crate::jobs::JobManager::process()
                    .shutdown_model_background_jobs_owned(config.job_owner)
                    .await;
                emit_or_pipe!(HeadlessEventV1::Failure {
                    kind: HeadlessFailureKind::Interrupted,
                    message: "interrupted".into(),
                    exit_code: 130,
                });
                return finish_with_code(emitter, 130);
            };
            exit_if_role_run_interrupted!();
            let mut execution = match execution_result {
                Ok(result) => result,
                Err(CallDecisionError::Approval(reason)) => {
                    emit_or_pipe!(HeadlessEventV1::ApprovalRequired {
                        call_id: event_call_id,
                        name: call.name.clone(),
                        reason,
                    });
                    emit_or_pipe!(HeadlessEventV1::Failure {
                        kind: HeadlessFailureKind::ApprovalRequired,
                        message: format!("approval or interaction required for {}", call.name),
                        exit_code: 4,
                    });
                    return finish_with_code(emitter, 4);
                }
                Err(CallDecisionError::Protocol(message)) => {
                    emit_or_pipe!(HeadlessEventV1::Failure {
                        kind: HeadlessFailureKind::ProviderProtocol,
                        message: if is_role_run(&config) {
                            "role-run tool request was invalid".into()
                        } else {
                            message
                        },
                        exit_code: 6,
                    });
                    return finish_with_code(emitter, 6);
                }
                Err(CallDecisionError::Interrupted) => {
                    emit_or_pipe!(HeadlessEventV1::Failure {
                        kind: HeadlessFailureKind::Interrupted,
                        message: "interrupted".into(),
                        exit_code: 130,
                    });
                    return finish_with_code(emitter, 130);
                }
            };
            if let Some(cap) = automation_tool_output_cap {
                execution = crate::automation_dispatch::bound_tool_result(execution, cap);
            }
            discovery.touch(&call.name);
            if let Some(capsule) = crate::mechanistic_delegation::take_process_capsule() {
                pending_capsule_context =
                    Some(crate::mechanistic_delegation::capsule_request_context(
                        &capsule,
                        config.mechanistic.capsule_bytes,
                    ));
            }
            if execution.is_success() {
                if let Some(batch) = execution.mutation_batch() {
                    for mutation in batch.receipts() {
                        if matches!(mutation.before, crate::undo::FileState::Missing) {
                            unscoped_mutation = true;
                        }
                        let relative = mutation
                            .path
                            .strip_prefix(&config.cwd)
                            .ok()
                            .map(|path| path.to_string_lossy().replace('\\', "/"))
                            .filter(|path| !path.is_empty())
                            .unwrap_or_else(|| "__ca_outside_pinned_workspace__".into());
                        changed_paths.insert(relative);
                    }
                }
                if let Some(id) = activation_id.filter(|id| prompted_skills.insert(id.clone())) {
                    let block = config
                        .skills
                        .lock()
                        .unwrap_or_else(|poison| poison.into_inner())
                        .active_prompt_for(&id);
                    if let Some(block) = block {
                        if let Some(message) = messages
                            .first_mut()
                            .filter(|message| message.role == "system")
                        {
                            if let Some(system) = message.content.as_text_mut() {
                                system.push_str(&block);
                            }
                        }
                    }
                }
            }
            let emit_environment = config.printenv && call.name == "run_command";
            emit_or_pipe!(HeadlessEventV1::ToolFinish {
                call_id: event_call_id,
                name: call.name.clone(),
                status: result_status(&execution),
                output: if is_role_run(&config) {
                    "[role-run tool output redacted]".into()
                } else {
                    execution.persistent_text().to_string()
                },
            });
            if emit_environment {
                emit_or_pipe!(HeadlessEventV1::EnvironmentSnapshot {
                    snapshot: crate::run_environment::current().structured_snapshot(),
                });
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
            let mut retained_model_text = crate::command_output::model_text_for_policy(
                &execution,
                config.command_output_policy,
            );
            if let Some(cap) = automation_tool_output_cap {
                retained_model_text =
                    crate::automation_dispatch::bound_retained_text(retained_model_text, cap);
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
                output: Some(retained_model_text),
            });
        }
        if let Some(incident) = loop_incident {
            emit_or_pipe!(HeadlessEventV1::Failure {
                kind: HeadlessFailureKind::LowYieldLoop,
                message: incident.evidence,
                exit_code: 9,
            });
            return finish_with_code(emitter, 9);
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
            emit_or_pipe!(HeadlessEventV1::Failure {
                kind: HeadlessFailureKind::Internal,
                message: if is_role_run(&config) {
                    "role-run provider messages were internally inconsistent".into()
                } else {
                    error.to_string()
                },
                exit_code: 8,
            });
            return finish_with_code(emitter, 8);
        }
        crate::agent::client::append_transient_tool_images(&mut messages, &completed_images);
        discovery_batch.commit();
        if let Some(reason) = crate::mechanistic_delegation::transparent_routing_degraded_reason() {
            emit_or_pipe!(HeadlessEventV1::Failure {
                kind: HeadlessFailureKind::Internal,
                message: format!(
                    "A safe local tool result was preserved, but codex-team routing must recover before provider continuation: {reason}"
                ),
                exit_code: 8,
            });
            return finish_with_code(emitter, 8);
        }
        if automatic_mechanistic_enabled
            && config.mechanistic.automatic_verification
            && (!changed_paths.is_empty() || unscoped_mutation)
        {
            if verification_suppressed {
                pending_verification_context =
                    Some(crate::mechanistic_delegation::skipped_verification_context(
                        "the user explicitly suppressed tests or codex-team automation",
                    ));
                verification_failed = false;
            } else if let Some(manager) = &config.mcp {
                let paths = if unscoped_mutation {
                    Vec::new()
                } else {
                    changed_paths.into_iter().collect()
                };
                let outcome = crate::mechanistic_delegation::automatic_verification(
                    manager.clone(),
                    paths,
                    repository_revision_before,
                    std::time::Duration::from_secs(config.mechanistic.max_wait_seconds),
                    config.mechanistic.allow_unisolated_evidence,
                    allow_unscoped_full,
                )
                .await;
                if let Some(tracker) = &mut emitter.outcome {
                    tracker.record_host_verification(&outcome);
                }
                pending_verification_context = outcome.context;
                match outcome.status {
                    crate::mechanistic_delegation::AutomaticVerificationStatus::Passed => {
                        verification_failed = false;
                    }
                    crate::mechanistic_delegation::AutomaticVerificationStatus::Unchanged => {}
                    crate::mechanistic_delegation::AutomaticVerificationStatus::Failed => {
                        verification_failed = true;
                        if let Some(job_id) = outcome.job_id.as_deref() {
                            let diagnosis =
                                crate::mechanistic_delegation::automatic_failure_diagnosis(
                                    manager.clone(),
                                    job_id,
                                    std::time::Duration::from_secs(10),
                                )
                                .await;
                            let original = pending_verification_context.take().unwrap_or_default();
                            pending_verification_context = Some(if original.is_empty() {
                                diagnosis.context
                            } else {
                                format!("{original}\n\n{}", diagnosis.context)
                            });
                        }
                        if repair_passes < config.mechanistic.max_repair_passes {
                            repair_passes += 1;
                        } else {
                            emit_or_pipe!(HeadlessEventV1::Failure {
                                kind: HeadlessFailureKind::Verification,
                                message: format!(
                                    "codex-team verification failed after {repair_passes} repair continuations"
                                ),
                                exit_code: 10,
                            });
                            return finish_with_code(emitter, 10);
                        }
                    }
                }
            }
        }
    }

    emit_or_pipe!(HeadlessEventV1::Failure {
        kind: HeadlessFailureKind::MaximumTurns,
        message: "maximum turns reached without a terminal response".into(),
        exit_code: 7,
    });
    finish_with_code(emitter, 7)
}

fn model_job_completion_message(completions: &[crate::jobs::JobInfo]) -> String {
    let mut content = String::from(
        "Background model-tool completion event. Incorporate these results into the conversation. This is host-owned lifecycle data, not user input.",
    );
    const MAX_MESSAGE_BYTES: usize = 64 * 1024;
    for completion in completions {
        let mut output =
            crate::terminal_text::sanitize_terminal_text(&completion.formatted_output());
        if output.len() > 8 * 1024 {
            let mut end = 8 * 1024;
            while !output.is_char_boundary(end) {
                end -= 1;
            }
            output.truncate(end);
            output.push_str("\n[… background job output truncated …]");
        }
        let item = format!(
            "\n\njob={} status={} command={}\n{}",
            completion.id,
            completion.status.label(),
            crate::terminal_text::sanitize_terminal_text(&completion.command),
            output
        );
        if content.len().saturating_add(item.len()) > MAX_MESSAGE_BYTES {
            content.push_str("\n\n[… additional completed jobs omitted from this bounded batch …]");
            break;
        }
        content.push_str(&item);
    }
    content
}

const CHECK_READ_TOOLS: &[&str] = &[
    "read_file",
    "list_dir",
    "search_files",
    "glob",
    "read_todos",
    "blame",
];
const MAX_CHECK_CONTEXT_CHARS: usize = 64 * 1024;
const MAX_CHECK_TURNS: usize = 8;

async fn reap_incomplete_subagents(config: &HeadlessRunConfig) -> usize {
    let Some(manager) = config.agents.as_ref() else {
        return 0;
    };
    let active = manager.active_count().await;
    if active > 0 {
        manager.shutdown().await;
    }
    active
}

fn subagent_tool_hard_denied(config: &HeadlessRunConfig, tool: &str) -> bool {
    config
        .approval_rules
        .iter()
        .any(|rule| matches!(rule, ApprovalRule::DenyTool(name) if name == tool))
        || config.permission_rules.iter().any(|rule| {
            rule.decision == crate::permission::PermissionDecision::Deny
                && matches!(
                    &rule.matcher,
                    crate::permission::PermissionMatcher::Tool { tool: name } if name == tool
                )
        })
}

fn headless_surface_tool_allowed(tool: &str) -> bool {
    tool != crate::privilege::TOOL_NAME
        && tool != crate::process_watch::WATCH_PROCESS_TOOL_NAME
        && tool != crate::process_watch::STOP_WATCHER_TOOL_NAME
}

struct CheckFailure {
    kind: HeadlessFailureKind,
    message: String,
    code: i32,
}

async fn finish_after_check<B: ChatBackend + ?Sized>(
    backend: &B,
    config: &HeadlessRunConfig,
    main_message: String,
    mut emitter: HeadlessEmitter<'_>,
    mut prompted_skills: std::collections::HashSet<String>,
) -> HeadlessExit {
    if config.structured_output.is_none() {
        if let Some(path) = config.output_last_message.as_deref() {
            if let Err(error) = atomic_write(path, &main_message) {
                let _ = emitter.emit(HeadlessEventV1::Failure {
                    kind: HeadlessFailureKind::Internal,
                    message: format!("failed to write last message: {error}"),
                    exit_code: 8,
                });
                return finish_with_code(emitter, 8);
            }
        }
    }
    emitter.final_message = Some(main_message.clone());
    if let Err(error) = emitter.emit_v2(crate::headless_contract::HeadlessEventV2::MainCompletion {
        message: main_message.clone(),
    }) {
        return match error {
            EmitError::BrokenPipe => HeadlessExit::SUCCESS,
            EmitError::Other(_) => HeadlessExit { code: 8 },
        };
    }
    if config.convergence.is_some() {
        return finish_after_convergent_check(
            backend,
            config,
            main_message,
            emitter,
            prompted_skills,
        )
        .await;
    }
    let outcome = match run_verification(
        backend,
        config,
        &main_message,
        &mut emitter,
        &mut prompted_skills,
    )
    .await
    {
        Ok(outcome) => outcome,
        Err(failure) => {
            let _ = emitter.emit(HeadlessEventV1::Failure {
                kind: failure.kind,
                message: failure.message,
                exit_code: failure.code,
            });
            return finish_with_code(emitter, failure.code);
        }
    };
    emitter.record_verification(outcome.clone());
    if let Err(error) = emitter.emit_v2(crate::headless_contract::HeadlessEventV2::CheckReport {
        outcome: outcome.clone(),
    }) {
        return match error {
            EmitError::BrokenPipe => HeadlessExit::SUCCESS,
            EmitError::Other(_) => HeadlessExit { code: 8 },
        };
    }
    if !outcome.report.passed {
        let _ = emitter.emit_v2(
            crate::headless_contract::HeadlessEventV2::VerificationFailed {
                summary: outcome.report.summary,
                exit_code: 9,
            },
        );
        return finish_with_code(emitter, 9);
    }
    let active = reap_incomplete_subagents(config).await;
    if active > 0 {
        let _ = emitter.emit(HeadlessEventV1::Failure {
            kind: HeadlessFailureKind::IncompleteSubagents,
            message: format!(
                "headless run completed with {active} active subagent(s); they were cancelled and reaped"
            ),
            exit_code: 8,
        });
        return finish_with_code(emitter, 8);
    }
    if let Err(error) = emitter.complete(main_message) {
        return match error {
            EmitError::BrokenPipe => HeadlessExit::SUCCESS,
            EmitError::Other(_) => HeadlessExit { code: 8 },
        };
    }
    match emitter.finish() {
        Ok(()) | Err(EmitError::BrokenPipe) => HeadlessExit::SUCCESS,
        Err(EmitError::Other(_)) => HeadlessExit { code: 8 },
    }
}

async fn finish_after_convergent_check<B: ChatBackend + ?Sized>(
    repair_backend: &B,
    config: &HeadlessRunConfig,
    main_message: String,
    mut emitter: HeadlessEmitter<'_>,
    mut prompted_skills: std::collections::HashSet<String>,
) -> HeadlessExit {
    let convergence = config
        .convergence
        .as_ref()
        .expect("caller checks convergent review configuration");
    let budget = match convergence.plan.budget() {
        Ok(budget) => budget,
        Err(message) => {
            let _ = emitter.emit(HeadlessEventV1::Failure {
                kind: HeadlessFailureKind::Input,
                message,
                exit_code: 2,
            });
            return finish_with_code(emitter, 2);
        }
    };
    let objective = bounded_chars(
        &format!(
            "{}\n\n<objective_rubric>Independently verify the completed result against the original request and concrete workspace evidence. Report every material defect; an empty findings list is a pass.</objective_rubric>\n<original_request>\n{}\n</original_request>\n<completed_result>\n{}\n</completed_result>",
            config.verification_instructions, config.original_request, main_message
        ),
        MAX_CHECK_CONTEXT_CHARS,
    );
    let usage_ledger =
        std::sync::Arc::new(std::sync::Mutex::new(crate::usage::UsageLedger::default()));
    let messages = vec![
        crate::app::Message {
            role: crate::app::Role::User,
            content: config.original_request.clone(),
            tool_calls: None,
            thinking: None,
            response_items: Vec::new(),
            timestamp: chrono::Utc::now(),
        },
        crate::app::Message {
            role: crate::app::Role::Assistant,
            content: main_message.clone(),
            tool_calls: None,
            thinking: None,
            response_items: Vec::new(),
            timestamp: chrono::Utc::now(),
        },
    ];
    let mut repair_tools = 0usize;

    for round_number in 1..=convergence.plan.max_rounds {
        let routes = [
            convergence.plan.routes[0].label(),
            convergence.plan.routes[1].label(),
        ];
        if let Err(error) = emitter.emit_v2(
            crate::headless_contract::HeadlessEventV2::ConvergenceStart {
                round: round_number,
                routes,
                max_cost_usd: convergence.plan.max_cost_usd.to_string(),
                unpriced_acknowledged: convergence.plan.unpriced_acknowledged,
            },
        ) {
            return match error {
                EmitError::BrokenPipe => HeadlessExit::SUCCESS,
                EmitError::Other(_) => HeadlessExit { code: 8 },
            };
        }
        let input = match crate::changes::GitChangeService::new(&config.cwd)
            .review_input(crate::changes::ReviewTarget::Working)
            .await
        {
            Ok(input) => input,
            Err(message) => {
                let _ = emitter.emit(HeadlessEventV1::Failure {
                    kind: HeadlessFailureKind::Input,
                    message: format!("convergent review could not capture source: {message}"),
                    exit_code: 2,
                });
                return finish_with_code(emitter, 2);
            }
        };
        let outcome = crate::review_convergence::run_review_pair(
            convergence.clients[0].as_ref(),
            convergence.clients[1].as_ref(),
            &convergence.plan,
            crate::review_convergence::ReviewPairRun {
                input: &input,
                project_instructions: &objective,
                workspace: &config.workspace,
                timeout_secs: config.command_timeout_secs,
                usage_ledger: usage_ledger.clone(),
                tool_policy: &config.tool_policy,
                budget: &budget,
                round: round_number,
            },
        )
        .await;
        if let Err(error) = emitter.emit_v2(
            crate::headless_contract::HeadlessEventV2::ConvergenceReport {
                outcome: outcome.clone(),
            },
        ) {
            return match error {
                EmitError::BrokenPipe => HeadlessExit::SUCCESS,
                EmitError::Other(_) => HeadlessExit { code: 8 },
            };
        }
        let verification = convergence_verification(&outcome, config.check_mode, repair_tools);
        emitter.record_verification(verification.clone());
        if let Err(error) =
            emitter.emit_v2(crate::headless_contract::HeadlessEventV2::CheckReport {
                outcome: verification,
            })
        {
            return match error {
                EmitError::BrokenPipe => HeadlessExit::SUCCESS,
                EmitError::Other(_) => HeadlessExit { code: 8 },
            };
        }
        let work = config
            .work_state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .clone();
        let revision = match crate::acceptance::capture_revision(
            &input,
            &config.workspace,
            None,
            &messages,
            &work,
        ) {
            Ok(revision) => revision,
            Err(message) => {
                let _ = emitter.emit(HeadlessEventV1::Failure {
                    kind: HeadlessFailureKind::Internal,
                    message,
                    exit_code: 8,
                });
                return finish_with_code(emitter, 8);
            }
        };
        let packet = match crate::acceptance::build_packet(
            &config.original_request,
            revision,
            &input,
            &work,
            &outcome,
            chrono::Utc::now(),
        ) {
            Ok(packet) => packet,
            Err(message) => {
                let _ = emitter.emit(HeadlessEventV1::Failure {
                    kind: HeadlessFailureKind::Internal,
                    message,
                    exit_code: 8,
                });
                return finish_with_code(emitter, 8);
            }
        };
        let packet_sha256 = packet.packet_sha256.clone();
        let Some(store) = convergence.acceptance_store.as_ref() else {
            let _ = emitter.emit(HeadlessEventV1::Failure {
                kind: HeadlessFailureKind::Setup,
                message: "convergent review requires a persistent acceptance store".into(),
                exit_code: 3,
            });
            return finish_with_code(emitter, 3);
        };
        if let Err(message) = store.create(packet) {
            let _ = emitter.emit(HeadlessEventV1::Failure {
                kind: HeadlessFailureKind::Internal,
                message: format!("acceptance packet publication failed: {message}"),
                exit_code: 8,
            });
            return finish_with_code(emitter, 8);
        }
        if let Err(error) = emitter.emit_v2(
            crate::headless_contract::HeadlessEventV2::AcceptancePacket {
                packet_sha256,
                stale: false,
            },
        ) {
            return match error {
                EmitError::BrokenPipe => HeadlessExit::SUCCESS,
                EmitError::Other(_) => HeadlessExit { code: 8 },
            };
        }
        if outcome.passed {
            let active = reap_incomplete_subagents(config).await;
            if active > 0 {
                let _ = emitter.emit(HeadlessEventV1::Failure {
                    kind: HeadlessFailureKind::IncompleteSubagents,
                    message: format!(
                        "headless run completed with {active} active subagent(s); they were cancelled and reaped"
                    ),
                    exit_code: 8,
                });
                return finish_with_code(emitter, 8);
            }
            if let Err(error) = emitter.complete(main_message) {
                return match error {
                    EmitError::BrokenPipe => HeadlessExit::SUCCESS,
                    EmitError::Other(_) => HeadlessExit { code: 8 },
                };
            }
            return match emitter.finish() {
                Ok(()) | Err(EmitError::BrokenPipe) => HeadlessExit::SUCCESS,
                Err(EmitError::Other(_)) => HeadlessExit { code: 8 },
            };
        }
        if !config.check_mode.repair_requested() || round_number == convergence.plan.max_rounds {
            let summary = format!(
                "convergent review remained blocked after round {round_number}; both distinct reviewers must pass"
            );
            let _ = emitter.emit_v2(
                crate::headless_contract::HeadlessEventV2::VerificationFailed {
                    summary,
                    exit_code: 9,
                },
            );
            return finish_with_code(emitter, 9);
        }
        match run_verification(
            repair_backend,
            config,
            &main_message,
            &mut emitter,
            &mut prompted_skills,
        )
        .await
        {
            Ok(repair) => {
                repair_tools = repair_tools.saturating_add(repair.tools_executed);
                emitter.record_verification(repair.clone());
                if let Err(error) =
                    emitter.emit_v2(crate::headless_contract::HeadlessEventV2::CheckReport {
                        outcome: repair,
                    })
                {
                    return match error {
                        EmitError::BrokenPipe => HeadlessExit::SUCCESS,
                        EmitError::Other(_) => HeadlessExit { code: 8 },
                    };
                }
            }
            Err(failure) => {
                let _ = emitter.emit(HeadlessEventV1::Failure {
                    kind: failure.kind,
                    message: failure.message,
                    exit_code: failure.code,
                });
                return finish_with_code(emitter, failure.code);
            }
        }
    }
    unreachable!("convergence loop has at least one validated round")
}

fn convergence_verification(
    outcome: &crate::review_convergence::ReviewConvergenceRound,
    mode: crate::headless_contract::CheckMode,
    tools_executed: usize,
) -> crate::headless_contract::VerificationOutcome {
    let issues = outcome
        .reviewers
        .iter()
        .flat_map(|reviewer| {
            reviewer
                .findings
                .iter()
                .map(|finding| {
                    format!(
                        "{}/{}: {}",
                        reviewer.provider, reviewer.model, finding.title
                    )
                })
                .chain(
                    (reviewer.verdict == crate::review_convergence::ReviewerVerdict::Error).then(
                        || format!("{}/{}: reviewer error", reviewer.provider, reviewer.model),
                    ),
                )
        })
        .take(32)
        .collect::<Vec<_>>();
    let evidence = outcome
        .reviewers
        .iter()
        .map(|reviewer| {
            format!(
                "{}/{}: {:?}",
                reviewer.provider, reviewer.model, reviewer.verdict
            )
        })
        .collect();
    crate::headless_contract::VerificationOutcome {
        report: crate::headless_contract::VerificationReport {
            passed: outcome.passed,
            summary: format!(
                "convergent review round {} {}",
                outcome.round,
                if outcome.passed { "passed" } else { "blocked" }
            ),
            issues,
            evidence,
        },
        repair_requested: mode.repair_requested(),
        tools_executed,
    }
}

async fn run_verification<B: ChatBackend + ?Sized>(
    backend: &B,
    config: &HeadlessRunConfig,
    main_message: &str,
    emitter: &mut HeadlessEmitter<'_>,
    prompted_skills: &mut std::collections::HashSet<String>,
) -> Result<crate::headless_contract::VerificationOutcome, CheckFailure> {
    let repair_requested = config.check_mode.repair_requested();
    emitter
        .emit_v2(crate::headless_contract::HeadlessEventV2::CheckStart { repair_requested })
        .map_err(check_emit_failure)?;
    let instructions = bounded_chars(&config.verification_instructions, MAX_CHECK_CONTEXT_CHARS);
    let original = bounded_chars(&config.original_request, MAX_CHECK_CONTEXT_CHARS);
    let result = bounded_chars(main_message, MAX_CHECK_CONTEXT_CHARS);
    let mode = if repair_requested {
        "Repairs are explicitly enabled, but every tool still follows the configured approval, permission, workspace, sandbox, and network policy."
    } else {
        "This is verify-only. Use only the advertised read-only tools. Do not modify files, run commands, use network access, or propose tool calls outside the advertised set."
    };
    let mut messages = vec![
        ChatMessage {
            role: "system".into(),
            content: crate::image_content::ModelContent::text(format!(
                "You are a fresh-context verification pass. Check the completed result against the original request using concrete workspace evidence. {mode} Finish with exactly one report_verification tool call and no other tool call in that final turn.\n\n<project_instructions>\n{instructions}\n</project_instructions>"
            )),
            tool_calls: None,
            tool_call_id: None,
            response_items: Vec::new(),
        },
        ChatMessage {
            role: "user".into(),
            content: crate::image_content::ModelContent::text(format!(
                "<original_request>\n{original}\n</original_request>\n<completed_result>\n{result}\n</completed_result>"
            )),
            tool_calls: None,
            tool_call_id: None,
            response_items: Vec::new(),
        },
    ];
    let mut tools_run = 0usize;
    let mut loop_guard = crate::loop_guard::LoopGuard::default();
    let mut execution_config = config.clone();
    if !repair_requested {
        execution_config.approval_mode = HeadlessApprovalMode::AutoReview;
        execution_config.allow_network = false;
        execution_config.mcp = None;
    }

    for turn_number in 1..=MAX_CHECK_TURNS {
        emitter
            .emit_v2(crate::headless_contract::HeadlessEventV2::TurnStart { turn: turn_number })
            .map_err(check_emit_failure)?;
        let catalog_registry = if repair_requested {
            if let Some(manager) = &config.mcp {
                manager.active_catalog().await.registry
            } else {
                None
            }
        } else {
            None
        };
        let registry = catalog_registry
            .as_ref()
            .unwrap_or_else(|| ToolRegistry::builtins());
        let mut tools = if repair_requested {
            crate::tool_selection::resolve_tools(
                registry,
                crate::agent::registry::ToolProfile::Main,
                &config.tool_policy,
                &config.tool_constraints,
            )
        } else {
            crate::tool_selection::resolve_internal_tools(
                registry,
                crate::agent::registry::ToolProfile::Review,
                &config.tool_policy,
                &crate::tool_selection::ToolSelectionConstraints::default()
                    .restricted_to(CHECK_READ_TOOLS.iter().copied()),
            )
        }
        .map_err(|error| CheckFailure {
            kind: HeadlessFailureKind::Input,
            message: format!("verification tool selection failed: {error}"),
            code: 2,
        })?
        .into_definitions();
        tools.retain(|tool| headless_surface_tool_allowed(&tool.function.name));
        if tools.iter().any(|tool| {
            matches!(
                tool.function.name.as_str(),
                crate::headless_contract::STRUCTURED_OUTPUT_TOOL
                    | crate::headless_contract::VERIFICATION_TOOL
            )
        }) {
            return Err(CheckFailure {
                kind: HeadlessFailureKind::Input,
                message: "selected tools collide with a reserved headless contract tool".into(),
                code: 2,
            });
        }
        tools.push(crate::headless_contract::verification_tool_definition());
        let advertised = tools
            .iter()
            .map(|tool| tool.function.name.clone())
            .collect::<std::collections::HashSet<_>>();
        let stable_prefix_messages = messages
            .iter()
            .take_while(|message| matches!(message.role.as_str(), "system" | "developer"))
            .count();
        let prepared = crate::request::PreparedModelRequest::new(
            crate::request::RequestKind::HeadlessCheck,
            config.provider.clone(),
            config.model.clone(),
            messages.clone(),
            tools,
            config.chat_options.clone(),
        )
        .with_prompt_cache(
            crate::request::PromptCacheScope::conversation(config.run_id),
            stable_prefix_messages,
        );
        let stream_result = tokio::select! {
            result = backend.stream_prepared(&prepared) => result.map(Some),
            _ = interruption_signal() => Ok(None),
        };
        let mut stream = match stream_result {
            Ok(Some(stream)) => stream,
            Ok(None) => {
                return Err(CheckFailure {
                    kind: HeadlessFailureKind::Interrupted,
                    message: "interrupted".into(),
                    code: 130,
                });
            }
            Err(error) => return Err(check_provider_failure(error)),
        };
        let mut assembler = AssistantTurnAssembler::new();
        while let Some(item) = tokio::select! {
            item = stream.recv() => item,
            _ = interruption_signal() => {
                return Err(CheckFailure {
                    kind: HeadlessFailureKind::Interrupted,
                    message: "interrupted".into(),
                    code: 130,
                });
            }
        } {
            let event = item.map_err(check_provider_failure)?;
            if let ProviderEvent::Usage(usage)
            | ProviderEvent::Finish {
                usage: Some(usage), ..
            } = &event
            {
                emitter
                    .emit_v2(crate::headless_contract::HeadlessEventV2::Usage {
                        usage: usage.clone(),
                    })
                    .map_err(check_emit_failure)?;
            }
            assembler.push(&event).map_err(check_provider_failure)?;
        }
        let turn = assembler.finish().map_err(check_provider_failure)?;
        let report_calls = turn
            .tool_calls
            .iter()
            .filter(|call| call.name == crate::headless_contract::VERIFICATION_TOOL)
            .collect::<Vec<_>>();
        if !report_calls.is_empty() {
            if report_calls.len() != 1 || turn.tool_calls.len() != 1 {
                return Err(CheckFailure {
                    kind: HeadlessFailureKind::ProviderProtocol,
                    message: "report_verification must be the sole final tool call".into(),
                    code: 6,
                });
            }
            let report =
                crate::headless_contract::VerificationReport::parse(&report_calls[0].arguments)
                    .map_err(|message| CheckFailure {
                        kind: HeadlessFailureKind::ProviderProtocol,
                        message,
                        code: 6,
                    })?;
            return Ok(crate::headless_contract::VerificationOutcome {
                report,
                repair_requested,
                tools_executed: tools_run,
            });
        }
        if turn.tool_calls.is_empty() {
            return Err(CheckFailure {
                kind: HeadlessFailureKind::ProviderProtocol,
                message: "verification response did not call report_verification".into(),
                code: 6,
            });
        }
        let mut completed_calls = Vec::new();
        let mut loop_incident = None;
        for call in turn.tool_calls {
            if !advertised.contains(&call.name) {
                return Err(CheckFailure {
                    kind: HeadlessFailureKind::ProviderProtocol,
                    message: format!(
                        "verification provider attempted unavailable tool: {}",
                        call.name
                    ),
                    code: 6,
                });
            }
            let arguments: serde_json::Value =
                serde_json::from_str(&call.arguments).map_err(|error| CheckFailure {
                    kind: HeadlessFailureKind::ProviderProtocol,
                    message: format!("invalid verification tool arguments: {error}"),
                    code: 6,
                })?;
            emitter
                .emit_v2(crate::headless_contract::HeadlessEventV2::ToolStart {
                    call_id: call.id.clone(),
                    name: call.name.clone(),
                    arguments,
                })
                .map_err(check_emit_failure)?;
            let execution = match execute_call(&execution_config, &call, None).await {
                Ok(execution) => execution,
                Err(CallDecisionError::Approval(reason)) => {
                    emitter
                        .emit_v2(
                            crate::headless_contract::HeadlessEventV2::ApprovalRequired {
                                call_id: call.id.clone(),
                                name: call.name.clone(),
                                reason: reason.clone(),
                            },
                        )
                        .map_err(check_emit_failure)?;
                    return Err(CheckFailure {
                        kind: HeadlessFailureKind::ApprovalRequired,
                        message: reason,
                        code: 4,
                    });
                }
                Err(CallDecisionError::Protocol(message)) => {
                    return Err(CheckFailure {
                        kind: HeadlessFailureKind::ProviderProtocol,
                        message,
                        code: 6,
                    });
                }
                Err(CallDecisionError::Interrupted) => {
                    return Err(CheckFailure {
                        kind: HeadlessFailureKind::Interrupted,
                        message: "interrupted".into(),
                        code: 130,
                    });
                }
            };
            if repair_requested && !matches!(execution, ToolExecutionResult::Denied(_)) {
                tools_run = tools_run.saturating_add(1);
            }
            let activation_id = (call.name == "activate_skill")
                .then(|| {
                    serde_json::from_str::<serde_json::Value>(&call.arguments)
                        .ok()
                        .and_then(|value| {
                            value
                                .get("id")
                                .and_then(serde_json::Value::as_str)
                                .map(str::to_string)
                        })
                })
                .flatten();
            if execution.is_success() {
                if let Some(id) = activation_id.filter(|id| prompted_skills.insert(id.clone())) {
                    let block = config
                        .skills
                        .lock()
                        .unwrap_or_else(|poison| poison.into_inner())
                        .active_prompt_for(&id);
                    if let Some(block) = block {
                        if let Some(message) = messages
                            .first_mut()
                            .filter(|message| message.role == "system")
                        {
                            if let Some(system) = message.content.as_text_mut() {
                                system.push_str(&block);
                            }
                        }
                    }
                }
            }
            emitter
                .emit_v2(crate::headless_contract::HeadlessEventV2::ToolFinish {
                    call_id: call.id.clone(),
                    name: call.name.clone(),
                    status: result_status(&execution),
                    output: execution.model_text().to_string(),
                })
                .map_err(check_emit_failure)?;
            if loop_incident.is_none() {
                loop_incident =
                    loop_guard.observe_batch([crate::loop_guard::LoopObservation::from_execution(
                        &call.name,
                        &call.arguments,
                        &execution,
                    )]);
            }
            completed_calls.push(ToolCall {
                id: call.id,
                name: call.name,
                arguments: call.arguments,
                status: if execution.is_success() {
                    ToolStatus::Done
                } else {
                    ToolStatus::Error
                },
                output: Some(crate::command_output::model_text_for_policy(
                    &execution,
                    execution_config.command_output_policy,
                )),
            });
        }
        if let Some(incident) = loop_incident {
            return Err(CheckFailure {
                kind: HeadlessFailureKind::LowYieldLoop,
                message: incident.evidence,
                code: 9,
            });
        }
        if let Some(incident) = loop_incident {
            return Err(CheckFailure {
                kind: HeadlessFailureKind::LowYieldLoop,
                message: incident.evidence,
                code: 9,
            });
        }
        let message = Message {
            role: Role::Assistant,
            content: turn.content,
            tool_calls: Some(completed_calls),
            thinking: turn.reasoning,
            response_items: turn.response_items,
            timestamp: chrono::Utc::now(),
        };
        crate::agent::client::append_provider_messages(&mut messages, &message).map_err(
            |error| CheckFailure {
                kind: HeadlessFailureKind::Internal,
                message: error.to_string(),
                code: 8,
            },
        )?;
    }
    Err(CheckFailure {
        kind: HeadlessFailureKind::MaximumTurns,
        message: "verification exceeded the maximum number of turns".into(),
        code: 7,
    })
}

fn check_emit_failure(error: EmitError) -> CheckFailure {
    match error {
        EmitError::BrokenPipe => CheckFailure {
            kind: HeadlessFailureKind::Internal,
            message: "downstream output closed".into(),
            code: 0,
        },
        EmitError::Other(message) => CheckFailure {
            kind: HeadlessFailureKind::Internal,
            message,
            code: 8,
        },
    }
}

fn check_provider_failure(error: ProviderError) -> CheckFailure {
    let (code, kind) = provider_failure(&error);
    CheckFailure {
        kind,
        message: error.to_string(),
        code,
    }
}

fn bounded_chars(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.to_string();
    }
    let omitted = value.chars().count().saturating_sub(max);
    let mut output = value.chars().take(max).collect::<String>();
    output.push_str(&format!("\n[… {omitted} characters omitted …]"));
    output
}

pub fn emit_standalone_failure_for_contract(
    format: OutputFormat,
    run_id: Uuid,
    contract_v2: bool,
    kind: HeadlessFailureKind,
    message: String,
    code: i32,
    writers: (&mut dyn Write, &mut dyn Write),
) -> HeadlessExit {
    let (stdout, stderr) = writers;
    let mut emitter = HeadlessEmitter::new_for_contract(
        format,
        run_id,
        stdout,
        stderr,
        contract_v2,
        HeadlessOutputRedactions::default(),
    );
    if matches!(
        emitter.emit(HeadlessEventV1::Failure {
            kind,
            message,
            exit_code: code,
        }),
        Err(EmitError::BrokenPipe)
    ) {
        return HeadlessExit::SUCCESS;
    }
    match emitter.finish() {
        Ok(()) | Err(EmitError::BrokenPipe) => HeadlessExit { code },
        Err(EmitError::Other(_)) => HeadlessExit { code: 8 },
    }
}

fn atomic_write(path: &Path, content: &str) -> std::io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let temp = parent.join(format!(".ca-last-message-{}.tmp", Uuid::new_v4()));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
        std::fs::rename(&temp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::stream::FinishReason;
    use std::collections::VecDeque;
    use std::sync::Mutex;
    use std::time::Duration;

    #[test]
    fn interactive_only_tools_are_never_advertised_headlessly() {
        assert!(!headless_surface_tool_allowed(crate::privilege::TOOL_NAME));
        assert!(!headless_surface_tool_allowed(
            crate::process_watch::WATCH_PROCESS_TOOL_NAME
        ));
        assert!(!headless_surface_tool_allowed(
            crate::process_watch::STOP_WATCHER_TOOL_NAME
        ));
    }

    type FakeTurn = Result<Vec<Result<ProviderEvent, ProviderError>>, ProviderError>;

    struct FakeBackend {
        turns: Mutex<VecDeque<FakeTurn>>,
        requests: Mutex<Vec<Vec<ChatMessage>>>,
        prepared: Mutex<Vec<(crate::request::RequestKind, String, String)>>,
        tool_sets: Mutex<Vec<Vec<String>>>,
    }

    impl FakeBackend {
        fn new(turns: Vec<FakeTurn>) -> Self {
            Self {
                turns: Mutex::new(turns.into()),
                requests: Mutex::new(Vec::new()),
                prepared: Mutex::new(Vec::new()),
                tool_sets: Mutex::new(Vec::new()),
            }
        }

        fn requests(&self) -> Vec<Vec<ChatMessage>> {
            self.requests.lock().unwrap().clone()
        }

        fn prepared(&self) -> Vec<(crate::request::RequestKind, String, String)> {
            self.prepared.lock().unwrap().clone()
        }

        fn tool_sets(&self) -> Vec<Vec<String>> {
            self.tool_sets.lock().unwrap().clone()
        }
    }

    #[async_trait::async_trait]
    impl ChatBackend for FakeBackend {
        async fn stream_chat(
            &self,
            messages: &[ChatMessage],
            _tools: &[ToolDef],
        ) -> Result<
            tokio::sync::mpsc::UnboundedReceiver<crate::agent::client::ChatStreamItem>,
            ProviderError,
        > {
            self.requests.lock().unwrap().push(messages.to_vec());
            let turn = self.turns.lock().unwrap().pop_front().expect("fake turn")?;
            let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
            for event in turn {
                sender.send(event).unwrap();
            }
            drop(sender);
            Ok(receiver)
        }

        async fn stream_prepared(
            &self,
            request: &crate::request::PreparedModelRequest,
        ) -> Result<
            tokio::sync::mpsc::UnboundedReceiver<crate::agent::client::ChatStreamItem>,
            ProviderError,
        > {
            self.prepared.lock().unwrap().push((
                request.kind,
                request.provider.clone(),
                request.model.clone(),
            ));
            self.tool_sets.lock().unwrap().push(
                request
                    .tools
                    .iter()
                    .map(|tool| tool.function.name.clone())
                    .collect(),
            );
            self.stream_chat(&request.messages, &request.tools).await
        }
    }

    fn text_turn(text: &str) -> Result<Vec<Result<ProviderEvent, ProviderError>>, ProviderError> {
        Ok(vec![
            Ok(ProviderEvent::Text(text.into())),
            Ok(ProviderEvent::Finish {
                reason: FinishReason::Stop,
                usage: Some(UsageInfo {
                    prompt_tokens: 3,
                    completion_tokens: 2,
                    total_tokens: 5,
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

    fn tool_turn(
        id: &str,
        name: &str,
        arguments: serde_json::Value,
    ) -> Result<Vec<Result<ProviderEvent, ProviderError>>, ProviderError> {
        Ok(vec![
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

    fn two_tool_turn(
        first: (&str, &str, serde_json::Value),
        second: (&str, &str, serde_json::Value),
    ) -> Result<Vec<Result<ProviderEvent, ProviderError>>, ProviderError> {
        let mut events = Vec::new();
        for (index, (id, name, arguments)) in [first, second].into_iter().enumerate() {
            events.push(Ok(ProviderEvent::ToolCallStart {
                index,
                id: id.into(),
                name: name.into(),
            }));
            events.push(Ok(ProviderEvent::ToolCallArg {
                index,
                arg: arguments.to_string(),
            }));
        }
        events.push(Ok(ProviderEvent::Finish {
            reason: FinishReason::ToolCalls,
            usage: None,
        }));
        events.push(Ok(ProviderEvent::Done));
        Ok(events)
    }

    fn messages() -> Vec<ChatMessage> {
        vec![ChatMessage {
            role: "user".into(),
            content: crate::image_content::ModelContent::text("hello"),
            tool_calls: None,
            tool_call_id: None,
            response_items: Vec::new(),
        }]
    }

    #[tokio::test]
    async fn workspace_restore_ownership_blocks_headless_before_provider_dispatch() {
        let root = tempfile::tempdir().unwrap();
        let profile = tempfile::tempdir().unwrap();
        let recovery = crate::recovery::RecoveryStore::new(profile.path());
        let restore = recovery
            .try_workspace_restore(&[root.path().to_path_buf()])
            .unwrap();
        let backend = FakeBackend::new(vec![text_turn("must remain unconsumed")]);
        let mut run = config(root.path(), OutputFormat::Json);
        run.workspace_activity_scope = Some(profile.path().to_path_buf());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let exit =
            run_with_backend(&backend, messages(), run.clone(), &mut stdout, &mut stderr).await;
        assert_eq!(exit.code, 3);
        assert!(backend.requests().is_empty());
        drop(restore);
        let exit =
            run_with_backend(&backend, messages(), run, &mut Vec::new(), &mut Vec::new()).await;
        assert_eq!(exit.code, 0);
        assert_eq!(backend.requests().len(), 1);
        tokio::time::timeout(Duration::from_secs(1), async {
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
    async fn background_native_job_retains_workspace_activity_after_provider_failure() {
        let root = tempfile::tempdir().unwrap();
        let profile = tempfile::tempdir().unwrap();
        let recovery = crate::recovery::RecoveryStore::new(profile.path());
        let backend = FakeBackend::new(vec![
            tool_turn(
                "background",
                "run_command",
                serde_json::json!({"command":"while [ ! -e release-job ]; do sleep 0.05; done", "background":true}),
            ),
            Err(ProviderError::MalformedData(
                "fixture provider failure".into(),
            )),
        ]);
        let mut run = config(root.path(), OutputFormat::Json);
        run.command_timeout_secs = 30;
        run.approval_mode = HeadlessApprovalMode::Yolo;
        run.workspace_activity_scope = Some(profile.path().to_path_buf());
        run.tool_policy = crate::tool_selection::ToolSelectionPolicy::replacing(&["run_command"]);
        let exit =
            run_with_backend(&backend, messages(), run, &mut Vec::new(), &mut Vec::new()).await;
        assert_eq!(exit.code, 6);
        assert_eq!(
            recovery
                .try_workspace_restore(&[root.path().to_path_buf()])
                .err()
                .unwrap()
                .kind(),
            std::io::ErrorKind::WouldBlock
        );
        std::fs::write(root.path().join("release-job"), b"done").unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
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

    fn config(root: &Path, format: OutputFormat) -> HeadlessRunConfig {
        HeadlessRunConfig {
            workspace_activity_scope: None,
            automation_policy: None,
            outcome_sink: None,
            provider: "fake".into(),
            model: "fake-model".into(),
            cwd: root.into(),
            workspace: crate::workspace::WorkspaceContext::discover(root).unwrap(),
            max_turns: 3,
            single_turn: false,
            command_timeout_secs: 2,
            command_output_policy: crate::config::CommandOutputPolicy::Bounded,
            output_format: format,
            show_thinking: false,
            printenv: false,
            approval_mode: HeadlessApprovalMode::RequireApproval,
            approval_rules: Vec::new(),
            permission_rules: Vec::new(),
            session_grants: Vec::new(),
            allow_network: false,
            output_last_message: None,
            check_mode: crate::headless_contract::CheckMode::Disabled,
            convergence: None,
            structured_output: None,
            original_request: "hello".into(),
            verification_instructions: "test instructions".into(),
            chat_options: crate::agent::client::ChatRequestOptions::default(),
            sandbox_mode: crate::sandbox::SandboxMode::PolicyOnly,
            run_id: Uuid::parse_str("00000000-0000-4000-8000-000000000017").unwrap(),
            job_owner: crate::jobs::JobOwner::new(None),
            skills: std::sync::Arc::new(std::sync::Mutex::new(
                crate::skills::SkillRegistry::empty(),
            )),
            mcp: None,
            mechanistic: crate::config::MechanisticMcpConfig::default(),
            lsp: None,
            email: None,
            tool_policy: crate::tool_selection::ToolSelectionPolicy::default(),
            tool_constraints: crate::tool_selection::ToolSelectionConstraints::default(),
            tool_profile: crate::agent::registry::ToolProfile::Main,
            startup_warnings: Vec::new(),
            request_kind: crate::request::RequestKind::Headless,
            agents: None,
            work_state: std::sync::Arc::new(std::sync::Mutex::new(
                crate::work::WorkState::default(),
            )),
            role_run_workspace_identity: None,
            output_redactions: HeadlessOutputRedactions::default(),
        }
    }

    #[tokio::test]
    async fn native_read_provider_batch_starts_together_and_returns_results_in_call_order() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("first.txt"), "first-marker").unwrap();
        std::fs::write(temp.path().join("second.txt"), "second-marker").unwrap();
        let backend = FakeBackend::new(vec![
            two_tool_turn(
                (
                    "read-first",
                    "read_file",
                    serde_json::json!({"path":"first.txt"}),
                ),
                (
                    "read-second",
                    "read_file",
                    serde_json::json!({"path":"second.txt"}),
                ),
            ),
            text_turn("done"),
        ]);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut run_config = config(temp.path(), OutputFormat::Json);
        run_config.approval_mode = HeadlessApprovalMode::Yolo;
        let exit =
            run_with_backend(&backend, messages(), run_config, &mut stdout, &mut stderr).await;
        assert_eq!(exit.code, 0, "{}", String::from_utf8_lossy(&stderr));
        let output: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
        let events = output["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|event| {
                let event = &event["event"];
                matches!(event["type"].as_str(), Some("tool_start" | "tool_finish")).then(|| {
                    (
                        event["type"].as_str().unwrap().to_string(),
                        event["call_id"].as_str().unwrap().to_string(),
                    )
                })
            })
            .collect::<Vec<_>>();
        assert_eq!(
            events,
            [
                ("tool_start".into(), "read-first".into()),
                ("tool_start".into(), "read-second".into()),
                ("tool_finish".into(), "read-first".into()),
                ("tool_finish".into(), "read-second".into())
            ]
        );
        let requests = backend.requests();
        let results = requests[1]
            .iter()
            .filter(|message| message.role == "tool")
            .collect::<Vec<_>>();
        assert_eq!(results[0].tool_call_id.as_deref(), Some("read-first"));
        assert_eq!(results[1].tool_call_id.as_deref(), Some("read-second"));
    }

    #[tokio::test]
    async fn native_headless_admission_keeps_ask_denial_and_routing_as_serial_barriers() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("read.txt"), "data").unwrap();
        let mut run = config(temp.path(), OutputFormat::Json);
        run.approval_mode = HeadlessApprovalMode::Yolo;
        let call = AssembledToolCall {
            index: 0,
            id: "read".into(),
            name: "read_file".into(),
            arguments: serde_json::json!({"path":"read.txt"}).to_string(),
        };
        assert!(prepare_headless_native_read(&run, &call, &run.workspace)
            .await
            .is_some());
        run.permission_rules
            .push(crate::permission::PermissionRule {
                decision: crate::permission::PermissionDecision::Ask,
                matcher: crate::permission::PermissionMatcher::Tool {
                    tool: "read_file".into(),
                },
            });
        assert!(prepare_headless_native_read(&run, &call, &run.workspace)
            .await
            .is_none());
        assert!(matches!(
            execute_call(&run, &call, None).await,
            Err(CallDecisionError::Approval(_))
        ));
        run.permission_rules.clear();
        run.approval_rules
            .push(crate::config::ApprovalRule::DenyTool("read_file".into()));
        assert!(prepare_headless_native_read(&run, &call, &run.workspace)
            .await
            .is_none());
        assert!(matches!(
            execute_call(&run, &call, None).await,
            Ok(ToolExecutionResult::Denied(_))
        ));
        run.approval_rules.clear();
        run.tool_constraints.transparent_mechanistic_enabled = true;
        assert!(prepare_headless_native_read(&run, &call, &run.workspace)
            .await
            .is_none());
    }

    #[tokio::test]
    async fn automation_background_wait_obeys_runtime_and_cancels_owned_job() {
        let root = tempfile::tempdir().unwrap();
        let mut waiting = text_turn("waiting").unwrap();
        for event in &mut waiting {
            if let Ok(ProviderEvent::Finish { usage, .. }) = event {
                *usage = None;
            }
        }
        let backend = FakeBackend::new(vec![
            tool_turn(
                "background-budget",
                "run_command",
                serde_json::json!({"command":"sleep 10", "background":true}),
            ),
            Ok(waiting),
        ]);
        let mut run = config(root.path(), OutputFormat::Json);
        run.approval_mode = HeadlessApprovalMode::Yolo;
        run.tool_policy = crate::tool_selection::ToolSelectionPolicy::replacing(&["run_command"]);
        let owner = run.job_owner;
        let mut frozen = crate::harness_budget::tests::frozen();
        frozen.context_tokens = 131_072;
        frozen.input_byte_limit = Some(131_072);
        let mut budget = crate::automation::AutomationBudgetV1::per_run_default();
        budget.runtime_secs = 1;
        run.automation_policy = Some(
            crate::automation_dispatch::AutomationDispatchPolicy::new(budget, frozen).unwrap(),
        );
        let result = tokio::time::timeout(
            Duration::from_secs(2),
            run_with_backend(&backend, messages(), run, &mut Vec::new(), &mut Vec::new()),
        )
        .await;
        // A failing pre-fix test must still stop its temporary native command.
        let jobs = crate::jobs::JobManager::process();
        let remaining = jobs.active_model_background_jobs_owned(owner).await;
        let _ = jobs.shutdown_model_background_jobs_owned(owner).await;
        let exit = result.expect("automation background wait exceeded its runtime budget");
        assert_eq!(exit.code, 7);
        assert!(
            remaining.is_empty(),
            "runtime exhaustion left a native job active"
        );
    }

    #[tokio::test]
    async fn automation_budget_rejects_first_request_before_fake_provider_dispatch() {
        let temporary = tempfile::tempdir().unwrap();
        let backend = FakeBackend::new(vec![text_turn("must not dispatch")]);
        let mut run = config(temporary.path(), OutputFormat::Json);
        let mut frozen = crate::harness_budget::tests::frozen();
        frozen.context_tokens = 131_072;
        frozen.input_byte_limit = Some(131_072);
        let mut budget = crate::automation::AutomationBudgetV1::per_run_default();
        budget.input_tokens = 1;
        run.automation_policy = Some(
            crate::automation_dispatch::AutomationDispatchPolicy::new(budget, frozen).unwrap(),
        );
        let mut stdout = Vec::new();
        assert_eq!(
            run_with_backend(&backend, messages(), run, &mut stdout, &mut Vec::new())
                .await
                .code,
            7
        );
        assert!(backend.requests().is_empty());
        let result: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
        assert_eq!(result["automation_usage"]["provider_requests"], 0);
    }

    #[tokio::test]
    async fn budgeted_verification_is_rejected_before_any_provider_request() {
        let temporary = tempfile::tempdir().unwrap();
        let backend = FakeBackend::new(vec![text_turn("must not dispatch")]);
        let mut run = config(temporary.path(), OutputFormat::Json);
        run.check_mode = crate::headless_contract::CheckMode::VerifyOnly;
        run.automation_policy = Some(
            crate::automation_dispatch::AutomationDispatchPolicy::new(
                crate::automation::AutomationBudgetV1::per_run_default(),
                crate::harness_budget::tests::frozen(),
            )
            .unwrap(),
        );
        assert_eq!(
            run_with_backend(&backend, messages(), run, &mut Vec::new(), &mut Vec::new())
                .await
                .code,
            2
        );
        assert!(backend.requests().is_empty());
    }

    #[tokio::test]
    async fn automation_tool_batch_budget_prevents_all_mutations_and_output_budget_drops_large_delta(
    ) {
        let temporary = tempfile::tempdir().unwrap();
        let backend = FakeBackend::new(vec![two_tool_turn(
            (
                "one",
                "write_file",
                serde_json::json!({"path":"one.txt","content":"one"}),
            ),
            (
                "two",
                "write_file",
                serde_json::json!({"path":"two.txt","content":"two"}),
            ),
        )]);
        let mut run = config(temporary.path(), OutputFormat::Json);
        run.approval_mode = HeadlessApprovalMode::Yolo;
        run.tool_policy = crate::tool_selection::ToolSelectionPolicy::replacing(&["write_file"]);
        let mut frozen = crate::harness_budget::tests::frozen();
        frozen.context_tokens = 131_072;
        frozen.input_byte_limit = Some(131_072);
        let mut budget = crate::automation::AutomationBudgetV1::per_run_default();
        budget.tool_calls = 1;
        run.automation_policy = Some(
            crate::automation_dispatch::AutomationDispatchPolicy::new(budget, frozen.clone())
                .unwrap(),
        );
        assert_eq!(
            run_with_backend(
                &backend,
                messages(),
                run.clone(),
                &mut Vec::new(),
                &mut Vec::new()
            )
            .await
            .code,
            7
        );
        assert!(
            !temporary.path().join("one.txt").exists()
                && !temporary.path().join("two.txt").exists()
        );
        let mut budget = crate::automation::AutomationBudgetV1::per_run_default();
        budget.output_bytes = 1;
        run.automation_policy = Some(
            crate::automation_dispatch::AutomationDispatchPolicy::new(budget, frozen).unwrap(),
        );
        let backend = FakeBackend::new(vec![text_turn("oversized delta")]);
        let mut stdout = Vec::new();
        assert_eq!(
            run_with_backend(&backend, messages(), run, &mut stdout, &mut Vec::new())
                .await
                .code,
            7
        );
        assert!(!String::from_utf8(stdout)
            .unwrap()
            .contains("oversized delta"));
        assert!(
            backend.requests().is_empty(),
            "insufficient retained-output budget dispatched a provider request"
        );
    }

    #[tokio::test]
    async fn automation_missing_usage_preserves_reservation_and_blocks_second_request() {
        let temporary = tempfile::tempdir().unwrap();
        let backend = FakeBackend::new(vec![
            tool_turn("read", "read_todos", serde_json::json!({})),
            text_turn("must not dispatch"),
        ]);
        let mut run = config(temporary.path(), OutputFormat::Json);
        run.approval_mode = HeadlessApprovalMode::Yolo;
        run.tool_policy = crate::tool_selection::ToolSelectionPolicy::replacing(&["read_todos"]);
        let mut frozen = crate::harness_budget::tests::frozen();
        frozen.context_tokens = 131_072;
        frozen.input_byte_limit = Some(131_072);
        let mut budget = crate::automation::AutomationBudgetV1::per_run_default();
        budget.provider_requests = 1;
        run.automation_policy = Some(
            crate::automation_dispatch::AutomationDispatchPolicy::new(budget, frozen).unwrap(),
        );
        let mut stdout = Vec::new();
        assert_eq!(
            run_with_backend(&backend, messages(), run, &mut stdout, &mut Vec::new())
                .await
                .code,
            7
        );
        assert_eq!(backend.requests().len(), 1);
        let result: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
        assert_eq!(result["automation_usage"]["provider_requests"], 1);
        assert!(result["automation_usage"]["input_tokens"].as_u64().unwrap() > 0);
        assert_eq!(result["automation_usage"]["output_tokens"], 100);
    }

    #[tokio::test]
    async fn lazy_builtin_discovery_activates_after_the_batch_and_rejects_current_batch_calls() {
        let temporary = tempfile::tempdir().unwrap();
        let mut run = config(temporary.path(), OutputFormat::Jsonl);
        run.approval_mode = HeadlessApprovalMode::Yolo;
        run.tool_policy = run
            .tool_policy
            .with_discovery_mode(crate::config::ToolDiscoveryMode::Lazy);
        let backend = FakeBackend::new(vec![
            tool_turn(
                "search",
                "search_tools",
                serde_json::json!({"query":"read_lints","limit":1}),
            ),
            text_turn("done"),
        ]);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let exit =
            run_with_backend(&backend, messages(), run.clone(), &mut stdout, &mut stderr).await;
        assert_eq!(exit.code, 0, "{}", String::from_utf8_lossy(&stderr));
        let sets = backend.tool_sets();
        assert!(sets[0].contains(&"search_tools".into()));
        assert!(!sets[0].contains(&"read_lints".into()));
        assert!(sets[1].contains(&"read_lints".into()));
        let backend = FakeBackend::new(vec![two_tool_turn(
            (
                "search",
                "search_tools",
                serde_json::json!({"query":"read_lints","limit":1}),
            ),
            ("early", "read_lints", serde_json::json!({})),
        )]);
        let exit =
            run_with_backend(&backend, messages(), run, &mut Vec::new(), &mut Vec::new()).await;
        assert_eq!(exit.code, 6);
        assert_eq!(backend.tool_sets().len(), 1);
    }

    #[tokio::test]
    async fn lazy_discovery_preserves_explicit_tool_replacements_and_hidden_authority() {
        let temporary = tempfile::tempdir().unwrap();
        let mut run = config(temporary.path(), OutputFormat::Jsonl);
        run.tool_policy = crate::tool_selection::ToolSelectionPolicy::replacing(&["read_lints"])
            .with_discovery_mode(crate::config::ToolDiscoveryMode::Lazy);
        let backend = FakeBackend::new(vec![text_turn("done")]);
        assert_eq!(
            run_with_backend(&backend, messages(), run, &mut Vec::new(), &mut Vec::new())
                .await
                .code,
            0
        );
        assert_eq!(backend.tool_sets(), vec![vec!["read_lints".to_string()]]);
    }

    #[tokio::test]
    async fn startup_warnings_are_structured_and_mode_profiles_constrain_tools() {
        let temp = tempfile::tempdir().unwrap();
        let backend = FakeBackend::new(vec![text_turn("planned")]);
        let mut run = config(temp.path(), OutputFormat::Json);
        run.tool_profile = crate::agent::registry::ToolProfile::Plan;
        run.startup_warnings = vec!["setup skipped safely".into()];
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let exit = run_with_backend(&backend, messages(), run, &mut stdout, &mut stderr).await;
        assert_eq!(exit.code, 0);
        let output: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
        assert!(output["events"].as_array().unwrap().iter().any(|event| {
            event["event"]["type"] == "warning"
                && event["event"]["message"] == "setup skipped safely"
        }));
        assert_eq!(
            backend.tool_sets()[0],
            ["propose_plan", "configure_agent_graph", "edit_plan"]
        );

        let backend = FakeBackend::new(vec![text_turn("answer")]);
        let mut run = config(temp.path(), OutputFormat::Json);
        run.tool_constraints.allowed_names = Some(std::collections::HashSet::new());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        assert_eq!(
            run_with_backend(&backend, messages(), run, &mut stdout, &mut stderr)
                .await
                .code,
            0
        );
        assert!(backend.tool_sets()[0].is_empty());
    }

    #[tokio::test]
    async fn model_background_completion_waits_and_reenters_once() {
        let temp = tempfile::tempdir().unwrap();
        let backend = FakeBackend::new(vec![
            tool_turn(
                "background-1",
                "run_command",
                serde_json::json!({
                    "command": "sleep 0.05; printf done",
                    "background": true
                }),
            ),
            text_turn("Waiting for the background work."),
            text_turn("Integrated the completed job."),
        ]);
        let mut run = config(temp.path(), OutputFormat::Json);
        run.approval_mode = HeadlessApprovalMode::Yolo;
        run.max_turns = 3;
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let exit = run_with_backend(&backend, messages(), run, &mut stdout, &mut stderr).await;
        assert_eq!(exit.code, 0, "{}", String::from_utf8_lossy(&stderr));
        let requests = backend.requests();
        assert_eq!(requests.len(), 3);
        let completion_context = requests[2]
            .iter()
            .filter(|message| message.role == "system")
            .filter_map(|message| message.content.as_str())
            .find(|content| content.contains("Background model-tool completion event"))
            .expect("host completion context");
        assert!(completion_context.contains("done"));
        assert_eq!(
            requests[2]
                .iter()
                .filter(|message| message.role == "system")
                .filter_map(|message| message.content.as_str())
                .filter(|content| content.contains("Background model-tool completion event"))
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn single_turn_shuts_down_background_jobs_with_structured_outcome() {
        let temp = tempfile::tempdir().unwrap();
        let backend = FakeBackend::new(vec![
            tool_turn(
                "background-1",
                "run_command",
                serde_json::json!({"command": "sleep 10", "background": true}),
            ),
            text_turn("Started the work."),
        ]);
        let mut run = config(temp.path(), OutputFormat::Json);
        run.approval_mode = HeadlessApprovalMode::Yolo;
        run.single_turn = true;
        run.max_turns = 3;
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let exit = run_with_backend(&backend, messages(), run, &mut stdout, &mut stderr).await;
        assert_eq!(exit.code, 0, "{}", String::from_utf8_lossy(&stderr));
        assert_eq!(backend.requests().len(), 2);
        let output: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
        let event = output["events"]
            .as_array()
            .unwrap()
            .iter()
            .find(|event| event["event"]["name"] == "background_jobs")
            .expect("structured background shutdown outcome");
        assert_eq!(event["event"]["type"], "tool_finish");
        assert!(event["event"]["output"]
            .as_str()
            .unwrap()
            .contains("shut down 1/1 active job"));
    }

    #[tokio::test]
    async fn printenv_emits_only_opt_in_structured_post_command_snapshots() {
        let temp = tempfile::tempdir().unwrap();
        let backend = FakeBackend::new(vec![
            tool_turn(
                "command-1",
                "run_command",
                serde_json::json!({"command":"printf ok"}),
            ),
            text_turn("done"),
        ]);
        let mut run = config(temp.path(), OutputFormat::Json);
        run.approval_mode = HeadlessApprovalMode::Yolo;
        run.printenv = true;
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let exit = run_with_backend(&backend, messages(), run, &mut stdout, &mut stderr).await;
        assert_eq!(exit.code, 0, "{}", String::from_utf8_lossy(&stderr));
        let output: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
        let snapshot = output["events"]
            .as_array()
            .unwrap()
            .iter()
            .find(|event| event["event"]["type"] == "environment_snapshot")
            .expect("opt-in environment snapshot");
        assert_eq!(
            snapshot["event"]["snapshot"]["schema"],
            "ca.environment-snapshot.v1"
        );
        assert_eq!(snapshot["event"]["snapshot"]["values_redacted"], true);
    }

    #[tokio::test]
    async fn background_follow_up_consumes_the_normal_max_turn_budget() {
        let temp = tempfile::tempdir().unwrap();
        let backend = FakeBackend::new(vec![
            tool_turn(
                "background-1",
                "run_command",
                serde_json::json!({
                    "command": "sleep 0.03; printf done",
                    "background": true
                }),
            ),
            text_turn("Waiting for the background work."),
        ]);
        let mut run = config(temp.path(), OutputFormat::Json);
        run.approval_mode = HeadlessApprovalMode::Yolo;
        run.max_turns = 2;
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let exit = run_with_backend(&backend, messages(), run, &mut stdout, &mut stderr).await;
        assert_eq!(exit.code, 7);
        assert_eq!(backend.requests().len(), 2);
        let output: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
        assert!(output["events"].as_array().unwrap().iter().any(|event| {
            event["event"]["type"] == "failure" && event["event"]["kind"] == "maximum_turns"
        }));
    }

    #[tokio::test]
    async fn repeated_headless_failure_terminates_with_typed_low_yield_loop() {
        let temp = tempfile::tempdir().unwrap();
        let backend = FakeBackend::new(vec![
            tool_turn(
                "fail-1",
                "run_command",
                serde_json::json!({"command":"false"}),
            ),
            tool_turn(
                "fail-2",
                "run_command",
                serde_json::json!({"command":"false"}),
            ),
            tool_turn(
                "fail-3",
                "run_command",
                serde_json::json!({"command":"false"}),
            ),
        ]);
        let mut run = config(temp.path(), OutputFormat::Jsonl);
        run.approval_mode = HeadlessApprovalMode::Yolo;
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let exit = run_with_backend(&backend, messages(), run, &mut stdout, &mut stderr).await;

        assert_eq!(exit.code, 9, "{}", String::from_utf8_lossy(&stderr));
        let output = String::from_utf8(stdout).unwrap();
        assert!(output.contains(r#""kind":"low_yield_loop""#));
        assert!(output.contains("unchanged output 3 times"));
        assert_eq!(backend.requests().len(), 3);
    }

    #[derive(Default)]
    struct RecordingRunner {
        requests: Mutex<Vec<crate::agent::subagent::AgentSpawnRequest>>,
        block_until_cancelled: bool,
    }

    #[async_trait::async_trait]
    impl crate::agent::subagent::AgentRunner for RecordingRunner {
        async fn run(
            &self,
            mut context: crate::agent::subagent::AgentRunContext,
        ) -> Result<String, String> {
            self.requests.lock().unwrap().push(context.request.clone());
            if self.block_until_cancelled {
                if !*context.cancelled.borrow() {
                    let _ = context.cancelled.changed().await;
                }
                return Err("cancelled".into());
            }
            Ok("mock child result".into())
        }
    }

    fn delegation_config(
        include_luna: bool,
        include_mimo: bool,
        include_deepseek: bool,
    ) -> crate::config::CaConfig {
        let mut config = crate::config::CaConfig::default();
        config.providers.clear();
        let mut openai_models = vec!["gpt-5.6-sol".into()];
        if include_luna {
            openai_models.push("gpt-5.6-luna".into());
        }
        config.providers.insert(
            "openai".into(),
            crate::config::ProviderConfig {
                base_url: "https://api.openai.com/v1".into(),
                api_key: "sk-mock-never-sent".into(),
                protocol: crate::agent::ProviderProtocol::default(),
                prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
                models: openai_models,
                timeout: crate::config::ProviderTimeoutConfig::default(),
            },
        );
        if include_mimo {
            config.providers.insert(
                "mimo".into(),
                crate::config::ProviderConfig {
                    base_url: crate::pricing::MimoRegion::Singapore
                        .token_plan_url()
                        .into(),
                    api_key: "tp-mock-never-sent".into(),
                    protocol: crate::agent::ProviderProtocol::default(),
                    prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
                    models: vec!["mimo-v2.5".into()],
                    timeout: crate::config::ProviderTimeoutConfig::default(),
                },
            );
            config.provider_billing.insert(
                "mimo".into(),
                crate::pricing::ProviderBillingConfig::mimo_token_plan(
                    crate::pricing::MimoRegion::Singapore,
                    crate::pricing::MimoPlanTier::Lite,
                    crate::pricing::MimoPlanTerm::Monthly,
                    "6".parse().unwrap(),
                    true,
                )
                .unwrap(),
            );
        }
        if include_deepseek {
            config.providers.insert(
                "deepseek".into(),
                crate::config::ProviderConfig {
                    base_url: crate::pricing::DEEPSEEK_API_V1_BASE_URL.into(),
                    api_key: "sk-deepseek-mock-never-sent".into(),
                    protocol: crate::agent::ProviderProtocol::ChatCompletions,
                    prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
                    models: vec!["deepseek-v4-flash".into(), "deepseek-v4-pro".into()],
                    timeout: crate::config::ProviderTimeoutConfig::default(),
                },
            );
        }
        config
    }

    fn test_agent_manager(
        runner: std::sync::Arc<RecordingRunner>,
        delegation: crate::config::CaConfig,
    ) -> std::sync::Arc<crate::agent::subagent::AgentManager> {
        const READ_TOOLS: &[&str] = &[
            "read_file",
            "list_dir",
            "search_files",
            "glob",
            "read_todos",
            "read_lints",
            "blame",
            "skill_view",
        ];
        let manager = crate::agent::subagent::AgentManager::new_with_tool_policy(
            runner,
            crate::agent::subagent::AgentLimits::default(),
            crate::tool_selection::ToolSelectionPolicy::replacing(READ_TOOLS),
            false,
            false,
        );
        manager.enforce_headless_read_only();
        manager.replace_parent_route(Some(crate::agent::subagent::AgentRoute {
            provider: "openai".into(),
            model: "gpt-5.6-sol".into(),
        }));
        manager.replace_delegation_router(Some(crate::pricing::DelegationRouter::from_config(
            &delegation,
            &crate::pricing::OfficialPriceCatalog::seeded(),
            chrono::DateTime::parse_from_rfc3339("2026-08-15T12:00:00Z")
                .unwrap()
                .to_utc(),
        )));
        manager
    }

    #[tokio::test]
    async fn sol_parent_routes_mocked_simple_tasks_to_luna_mimo_and_deepseek() {
        for (delegation, expected) in [
            (
                delegation_config(true, false, false),
                ("openai", "gpt-5.6-luna"),
            ),
            (delegation_config(false, true, false), ("mimo", "mimo-v2.5")),
            (
                delegation_config(false, false, true),
                ("deepseek", "deepseek-v4-flash"),
            ),
        ] {
            let temp = tempfile::tempdir().unwrap();
            let runner = std::sync::Arc::new(RecordingRunner::default());
            let manager = test_agent_manager(runner.clone(), delegation);
            let backend = FakeBackend::new(vec![
                tool_turn(
                    "delegate",
                    "task",
                    serde_json::json!({
                        "subagent_name": "plan",
                        "prompt": "summarize the small fixture",
                        "complexity": "simple"
                    }),
                ),
                text_turn("parent complete"),
            ]);
            let mut run = config(temp.path(), OutputFormat::Jsonl);
            run.agents = Some(manager.clone());
            let mut stdout = Vec::new();
            let mut stderr = Vec::new();

            let exit = run_with_backend(&backend, messages(), run, &mut stdout, &mut stderr).await;

            assert_eq!(exit.code, 0, "{}", String::from_utf8_lossy(&stderr));
            {
                let requests = runner.requests.lock().unwrap();
                assert_eq!(requests.len(), 1);
                let route = requests[0].route.as_ref().unwrap();
                assert_eq!((route.provider.as_str(), route.model.as_str()), expected);
                assert_eq!(
                    requests[0].purpose,
                    crate::agent::subagent::AgentRunPurpose::Subagent
                );
            }
            assert!(
                String::from_utf8(stdout)
                    .unwrap()
                    .contains("subscription allocation")
                    || matches!(expected.0, "openai" | "deepseek")
            );
            manager.shutdown().await;
        }
    }

    #[tokio::test]
    async fn headless_children_reject_shell_write_network_and_unbounded_named_agents() {
        let temp = tempfile::tempdir().unwrap();
        let runner = std::sync::Arc::new(RecordingRunner::default());
        let manager = test_agent_manager(runner.clone(), delegation_config(true, false, false));
        let mut run = config(temp.path(), OutputFormat::Text);
        run.approval_mode = HeadlessApprovalMode::Yolo;
        run.allow_network = true;
        run.agents = Some(manager);

        for arguments in [
            serde_json::json!({
                "task":"write",
                "tools":["write_file"]
            }),
            serde_json::json!({
                "task":"browse",
                "tools":["web_fetch"]
            }),
            serde_json::json!({
                "task":"shell",
                "tools":["read_file"],
                "commands":["git status"]
            }),
        ] {
            let call = AssembledToolCall {
                index: 0,
                id: "restricted".into(),
                name: "spawn_agent".into(),
                arguments: arguments.to_string(),
            };
            let result = execute_call(&run, &call, None).await.unwrap();
            assert!(!result.is_success());
        }
        let general = AssembledToolCall {
            index: 0,
            id: "general".into(),
            name: "task".into(),
            arguments: serde_json::json!({
                "subagent_name":"general",
                "prompt":"change a file"
            })
            .to_string(),
        };
        assert!(!execute_call(&run, &general, None)
            .await
            .unwrap()
            .is_success());
        assert!(runner.requests.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn exact_hard_denies_remove_headless_subagent_tools_from_advertisement() {
        let temp = tempfile::tempdir().unwrap();
        let backend = FakeBackend::new(vec![text_turn("done")]);
        let mut run = config(temp.path(), OutputFormat::Jsonl);
        run.approval_rules = vec![ApprovalRule::DenyTool("task".into())];
        run.permission_rules = vec![crate::permission::PermissionRule {
            decision: crate::permission::PermissionDecision::Deny,
            matcher: crate::permission::PermissionMatcher::Tool {
                tool: "spawn_agent".into(),
            },
        }];
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let exit = run_with_backend(&backend, messages(), run, &mut stdout, &mut stderr).await;

        assert_eq!(exit.code, 0);
        assert!(!backend.tool_sets()[0].iter().any(|tool| tool == "task"));
        assert!(!backend.tool_sets()[0]
            .iter()
            .any(|tool| tool == "spawn_agent"));
    }

    #[tokio::test]
    async fn headless_completion_cancels_active_async_children_and_fails_closed() {
        let temp = tempfile::tempdir().unwrap();
        let runner = std::sync::Arc::new(RecordingRunner {
            requests: Mutex::new(Vec::new()),
            block_until_cancelled: true,
        });
        let manager = test_agent_manager(runner, delegation_config(true, false, false));
        let backend = FakeBackend::new(vec![
            tool_turn(
                "spawn",
                "spawn_agent",
                serde_json::json!({
                    "task":"read slowly",
                    "tools":["read_file"],
                    "complexity":"simple"
                }),
            ),
            text_turn("parent returned early"),
        ]);
        let mut run = config(temp.path(), OutputFormat::Jsonl);
        run.agents = Some(manager.clone());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let exit = run_with_backend(&backend, messages(), run, &mut stdout, &mut stderr).await;

        assert_eq!(exit.code, 8);
        assert_eq!(manager.active_count().await, 0);
        let output = String::from_utf8(stdout).unwrap();
        assert!(output.contains("incomplete_subagents"));
        assert!(output.contains("cancelled and reaped"));
    }

    fn structured_contract(root: &Path) -> crate::headless_contract::StructuredOutputContract {
        let path = root.join("schema.json");
        std::fs::write(
            &path,
            serde_json::to_vec(&serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["answer"],
                "properties": { "answer": { "type": "integer" } }
            }))
            .unwrap(),
        )
        .unwrap();
        crate::headless_contract::StructuredOutputContract::load(&path).unwrap()
    }

    #[tokio::test]
    async fn yolo_and_generic_allow_cannot_authorize_outbound_email() {
        let temp = tempfile::tempdir().unwrap();
        let mut run = config(temp.path(), OutputFormat::Text);
        run.approval_mode = HeadlessApprovalMode::Yolo;
        run.approval_rules = vec![crate::config::ApprovalRule::AllowTool("email_send".into())];
        run.permission_rules = vec![crate::permission::PermissionRule {
            decision: crate::permission::PermissionDecision::Allow,
            matcher: crate::permission::PermissionMatcher::Tool {
                tool: "email_send".into(),
            },
        }];
        let call = AssembledToolCall {
            index: 0,
            id: "email-call".into(),
            name: "email_send".into(),
            arguments: serde_json::json!({
                "draft_ref":"draft_00000000000000000000000000000000"
            })
            .to_string(),
        };
        assert!(matches!(
            execute_call(&run, &call, None).await,
            Err(CallDecisionError::Approval(reason))
                if reason.contains("exact interactive one-use approval")
        ));
    }

    #[tokio::test]
    async fn view_image_is_process_only_and_reaches_exactly_the_next_continuation() {
        let temp = tempfile::tempdir().unwrap();
        let image_path = temp.path().join("fixture.png");
        image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            2,
            1,
            image::Rgba([20, 40, 60, 255]),
        ))
        .save(&image_path)
        .unwrap();
        let backend = FakeBackend::new(vec![
            tool_turn(
                "view-1",
                "view_image",
                serde_json::json!({"path":"fixture.png"}),
            ),
            text_turn("done"),
        ]);
        let mut run_config = config(temp.path(), OutputFormat::Jsonl);
        run_config.approval_mode = HeadlessApprovalMode::Yolo;
        run_config.tool_constraints.images_supported = true;
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let exit =
            run_with_backend(&backend, messages(), run_config, &mut stdout, &mut stderr).await;
        assert_eq!(exit, HeadlessExit::SUCCESS);
        let requests = backend.requests();
        assert_eq!(requests.len(), 2);
        assert!(!requests[0]
            .iter()
            .any(|message| message.content.has_images()));
        let multimodal = requests[1]
            .iter()
            .filter(|message| message.content.has_images())
            .collect::<Vec<_>>();
        assert_eq!(multimodal.len(), 1);
        assert_eq!(multimodal[0].role, "user");
        assert!(requests[1].iter().any(|message| {
            message.role == "tool"
                && message.tool_call_id.as_deref() == Some("view-1")
                && message
                    .content
                    .as_str()
                    .is_some_and(|text| text.contains("image/png 2×1"))
        }));
        let output = String::from_utf8(stdout).unwrap();
        assert!(output.contains("image/png 2×1"));
        assert!(!output.contains("data:image"));
        assert!(stderr.is_empty());
    }

    #[tokio::test]
    async fn structured_output_uses_v2_and_revalidates_the_sole_final_value() {
        let temp = tempfile::tempdir().unwrap();
        let backend = FakeBackend::new(vec![tool_turn(
            "report-1",
            crate::headless_contract::STRUCTURED_OUTPUT_TOOL,
            serde_json::json!({"value": {"answer": 7}}),
        )]);
        let mut run_config = config(temp.path(), OutputFormat::Json);
        run_config.structured_output = Some(structured_contract(temp.path()));
        let last = temp.path().join("last.json");
        run_config.output_last_message = Some(last.clone());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let exit =
            run_with_backend(&backend, messages(), run_config, &mut stdout, &mut stderr).await;

        assert_eq!(exit.code, 0);
        assert!(stderr.is_empty());
        let output: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
        assert_eq!(output["schema"], crate::headless_contract::SCHEMA_V2);
        assert_eq!(output["success"], true);
        assert_eq!(
            output["structured_output"],
            serde_json::json!({"answer": 7})
        );
        assert_eq!(output["final_message"], r#"{"answer":7}"#);
        assert_eq!(std::fs::read_to_string(last).unwrap(), r#"{"answer":7}"#);
        assert!(backend.tool_sets()[0]
            .iter()
            .any(|name| name == crate::headless_contract::STRUCTURED_OUTPUT_TOOL));

        let backend = FakeBackend::new(vec![tool_turn(
            "report-2",
            crate::headless_contract::STRUCTURED_OUTPUT_TOOL,
            serde_json::json!({"value": {"answer": "wrong"}}),
        )]);
        let mut run_config = config(temp.path(), OutputFormat::Json);
        run_config.structured_output = Some(structured_contract(temp.path()));
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let exit =
            run_with_backend(&backend, messages(), run_config, &mut stdout, &mut stderr).await;
        assert_eq!(exit.code, 6);
        let output: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
        assert_eq!(output["schema"], crate::headless_contract::SCHEMA_V2);
        assert_eq!(output["success"], false);
        assert!(output["events"].to_string().contains("local validation"));

        let backend = FakeBackend::new(vec![two_tool_turn(
            (
                "report-3",
                crate::headless_contract::STRUCTURED_OUTPUT_TOOL,
                serde_json::json!({"value": {"answer": 7}}),
            ),
            (
                "read-1",
                "read_file",
                serde_json::json!({"path": "Cargo.toml"}),
            ),
        )]);
        let mut run_config = config(temp.path(), OutputFormat::Json);
        run_config.structured_output = Some(structured_contract(temp.path()));
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let exit =
            run_with_backend(&backend, messages(), run_config, &mut stdout, &mut stderr).await;
        assert_eq!(exit.code, 6);
        assert!(
            serde_json::from_slice::<serde_json::Value>(&stdout).unwrap()["events"]
                .to_string()
                .contains("sole final")
        );
    }

    #[tokio::test]
    async fn verify_only_check_is_fresh_read_only_and_has_distinct_failed_exit() {
        let temp = tempfile::tempdir().unwrap();
        let pass = FakeBackend::new(vec![
            text_turn("main result"),
            tool_turn(
                "check-1",
                crate::headless_contract::VERIFICATION_TOOL,
                serde_json::json!({
                    "passed": true,
                    "summary": "request satisfied",
                    "issues": [],
                    "evidence": ["main result"]
                }),
            ),
        ]);
        let mut run_config = config(temp.path(), OutputFormat::Json);
        run_config.check_mode = crate::headless_contract::CheckMode::VerifyOnly;
        run_config.original_request = "original task".into();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let exit = run_with_backend(&pass, messages(), run_config, &mut stdout, &mut stderr).await;
        assert_eq!(exit.code, 0);
        let output: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
        assert_eq!(output["schema"], crate::headless_contract::SCHEMA_V2);
        assert_eq!(output["verification"]["report"]["passed"], true);
        assert_eq!(
            pass.prepared(),
            vec![
                (
                    crate::request::RequestKind::Headless,
                    "fake".into(),
                    "fake-model".into()
                ),
                (
                    crate::request::RequestKind::HeadlessCheck,
                    "fake".into(),
                    "fake-model".into()
                )
            ]
        );
        let requests = pass.requests();
        assert_eq!(requests[1].len(), 2, "check context must be fresh");
        let check_text = requests[1]
            .iter()
            .map(|message| {
                message
                    .content
                    .as_str()
                    .unwrap_or("[multimodal content]")
                    .to_string()
            })
            .collect::<String>();
        assert!(check_text.contains("original task"));
        assert!(check_text.contains("main result"));
        assert!(!pass.tool_sets()[1]
            .iter()
            .any(|name| { matches!(name.as_str(), "write_file" | "run_command" | "web_fetch") }));

        let fail = FakeBackend::new(vec![
            text_turn("main result"),
            tool_turn(
                "check-2",
                crate::headless_contract::VERIFICATION_TOOL,
                serde_json::json!({
                    "passed": false,
                    "summary": "missing required evidence",
                    "issues": ["no test result"],
                    "evidence": []
                }),
            ),
        ]);
        let mut run_config = config(temp.path(), OutputFormat::Json);
        run_config.check_mode = crate::headless_contract::CheckMode::VerifyOnly;
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let exit = run_with_backend(&fail, messages(), run_config, &mut stdout, &mut stderr).await;
        assert_eq!(exit.code, 9);
        let output: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
        assert_eq!(output["success"], false);
        assert_eq!(output["final_message"], "main result");
        assert!(output["events"].to_string().contains("verification_failed"));
    }

    fn convergent_config(
        root: &Path,
        reviewers: [std::sync::Arc<dyn ChatBackend>; 2],
    ) -> HeadlessConvergenceConfig {
        let mut ca = crate::config::CaConfig::default();
        for (provider, model, port) in [
            ("review-a", "model-a", 1_u16),
            ("review-b", "model-b", 2_u16),
        ] {
            ca.providers.insert(
                provider.into(),
                crate::config::ProviderConfig {
                    base_url: format!("http://127.0.0.1:{port}/v1"),
                    api_key: "test".into(),
                    protocol: crate::agent::ProviderProtocol::default(),
                    prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
                    models: vec![model.into()],
                    timeout: crate::config::ProviderTimeoutConfig::default(),
                },
            );
        }
        ca.review.convergence.routes = vec![
            crate::config::ReviewRouteConfig {
                provider: "review-a".into(),
                model: "model-a".into(),
            },
            crate::config::ReviewRouteConfig {
                provider: "review-b".into(),
                model: "model-b".into(),
            },
        ];
        let plan = crate::review_convergence::resolve_review_convergence(
            &ca,
            None,
            true,
            chrono::Utc::now(),
        )
        .unwrap();
        HeadlessConvergenceConfig {
            plan,
            clients: reviewers,
            acceptance_store: Some(crate::acceptance::AcceptanceStore::new(
                root.join("acceptance"),
            )),
        }
    }

    fn review_report(summary: &str, finding: bool) -> FakeTurn {
        let findings = if finding {
            vec![serde_json::json!({
                "severity":"high", "title":"regression", "rationale":"broken",
                "file":"src/lib.rs", "line_start":1, "line_end":1,
                "confidence":99, "suggested_validation":"run the test"
            })]
        } else {
            Vec::new()
        };
        tool_turn(
            "review-report",
            "report_review_findings",
            serde_json::json!({"summary":summary,"findings":findings}),
        )
    }

    fn init_review_repo(root: &Path) {
        let status = std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(root)
            .status()
            .unwrap();
        assert!(status.success());
        std::fs::create_dir(root.join("src")).unwrap();
        std::fs::write(root.join("src/lib.rs"), "change\n").unwrap();
    }

    #[tokio::test]
    async fn convergent_check_runs_two_fresh_reviewers_and_publishes_acceptance() {
        let temp = tempfile::tempdir().unwrap();
        init_review_repo(temp.path());
        let first =
            std::sync::Arc::new(FakeBackend::new(vec![review_report("first clean", false)]));
        let second =
            std::sync::Arc::new(FakeBackend::new(vec![review_report("second clean", false)]));
        let mut run_config = config(temp.path(), OutputFormat::Json);
        run_config.check_mode = crate::headless_contract::CheckMode::VerifyOnly;
        run_config.convergence = Some(convergent_config(
            temp.path(),
            [first.clone(), second.clone()],
        ));
        let main = FakeBackend::new(vec![text_turn("main result")]);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let exit = run_with_backend(&main, messages(), run_config, &mut stdout, &mut stderr).await;
        assert_eq!(exit.code, 0, "{}", String::from_utf8_lossy(&stderr));
        assert_eq!(first.prepared().len(), 1);
        assert_eq!(second.prepared().len(), 1);
        assert_eq!(first.requests()[0].len(), 2);
        assert_eq!(second.requests()[0].len(), 2);
        assert_eq!(
            serde_json::to_vec(&first.requests()[0]).unwrap(),
            serde_json::to_vec(&second.requests()[0]).unwrap()
        );
        let output: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
        let events = output["events"].to_string();
        assert!(events.contains("convergence_start"));
        assert!(events.contains("convergence_report"));
        assert!(events.contains("acceptance_packet"));
        assert_eq!(output["verification"]["report"]["passed"], true);
        let store = crate::acceptance::AcceptanceStore::new(temp.path().join("acceptance"));
        assert_eq!(store.list(None).unwrap().len(), 1);
    }

    #[tokio::test]
    async fn convergent_repair_uses_fresh_pairs_and_stops_after_three_rounds() {
        let temp = tempfile::tempdir().unwrap();
        init_review_repo(temp.path());
        let first = std::sync::Arc::new(FakeBackend::new(vec![
            review_report("first clean", false),
            review_report("first still clean", false),
            review_report("first remains clean", false),
        ]));
        let second = std::sync::Arc::new(FakeBackend::new(vec![
            review_report("second blocked", true),
            review_report("second still blocked", true),
            review_report("second remains blocked", true),
        ]));
        let mut run_config = config(temp.path(), OutputFormat::Json);
        run_config.check_mode = crate::headless_contract::CheckMode::Repair;
        run_config.convergence = Some(convergent_config(
            temp.path(),
            [first.clone(), second.clone()],
        ));
        let main = FakeBackend::new(vec![
            text_turn("main result"),
            tool_turn(
                "repair-one",
                crate::headless_contract::VERIFICATION_TOOL,
                serde_json::json!({
                    "passed":true,"summary":"repair round one","issues":[],"evidence":[]
                }),
            ),
            tool_turn(
                "repair-two",
                crate::headless_contract::VERIFICATION_TOOL,
                serde_json::json!({
                    "passed":true,"summary":"repair round two","issues":[],"evidence":[]
                }),
            ),
        ]);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let exit = run_with_backend(&main, messages(), run_config, &mut stdout, &mut stderr).await;
        assert_eq!(exit.code, 9, "{}", String::from_utf8_lossy(&stderr));
        assert_eq!(first.prepared().len(), 3);
        assert_eq!(second.prepared().len(), 3);
        assert_eq!(main.prepared().len(), 3);
        let output: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
        assert_eq!(output["verification"]["report"]["passed"], false);
        let store = crate::acceptance::AcceptanceStore::new(temp.path().join("acceptance"));
        assert_eq!(store.list(None).unwrap().len(), 3);
    }

    #[tokio::test]
    async fn headless_contracts_fail_before_traffic_for_a_toolless_model() {
        let temp = tempfile::tempdir().unwrap();
        for structured in [false, true] {
            let backend = FakeBackend::new(Vec::new());
            let mut run_config = config(temp.path(), OutputFormat::Json);
            run_config.tool_constraints.tools_supported = false;
            if structured {
                run_config.structured_output = Some(structured_contract(temp.path()));
            } else {
                run_config.check_mode = crate::headless_contract::CheckMode::VerifyOnly;
            }
            let mut stdout = Vec::new();
            let mut stderr = Vec::new();
            let exit =
                run_with_backend(&backend, messages(), run_config, &mut stdout, &mut stderr).await;
            assert_eq!(exit.code, 2);
            assert!(backend.prepared().is_empty());
            let output: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
            assert_eq!(output["schema"], crate::headless_contract::SCHEMA_V2);
        }
    }

    #[tokio::test]
    async fn verify_only_cannot_mutate_and_repair_remains_under_normal_approval() {
        let temp = tempfile::tempdir().unwrap();
        let attempted = temp.path().join("attempted.txt");
        let verify = FakeBackend::new(vec![
            text_turn("main result"),
            tool_turn(
                "write-1",
                "write_file",
                serde_json::json!({"path": attempted, "content": "no"}),
            ),
        ]);
        let mut run_config = config(temp.path(), OutputFormat::Json);
        run_config.check_mode = crate::headless_contract::CheckMode::VerifyOnly;
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let exit =
            run_with_backend(&verify, messages(), run_config, &mut stdout, &mut stderr).await;
        assert_eq!(exit.code, 6);
        assert!(!attempted.exists());

        let repaired = temp.path().join("repaired.txt");
        let approval_required = FakeBackend::new(vec![
            text_turn("main result"),
            tool_turn(
                "write-approval",
                "write_file",
                serde_json::json!({"path": repaired, "content": "fixed"}),
            ),
        ]);
        let mut run_config = config(temp.path(), OutputFormat::Json);
        run_config.check_mode = crate::headless_contract::CheckMode::Repair;
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let exit = run_with_backend(
            &approval_required,
            messages(),
            run_config,
            &mut stdout,
            &mut stderr,
        )
        .await;
        assert_eq!(exit.code, 4);
        assert!(!repaired.exists());

        let repair = FakeBackend::new(vec![
            text_turn("main result"),
            tool_turn(
                "write-2",
                "write_file",
                serde_json::json!({"path": repaired, "content": "fixed"}),
            ),
            tool_turn(
                "check-3",
                crate::headless_contract::VERIFICATION_TOOL,
                serde_json::json!({
                    "passed": true,
                    "summary": "repaired",
                    "issues": [],
                    "evidence": ["file written"]
                }),
            ),
        ]);
        let mut run_config = config(temp.path(), OutputFormat::Json);
        run_config.check_mode = crate::headless_contract::CheckMode::Repair;
        run_config.approval_mode = HeadlessApprovalMode::Yolo;
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let exit =
            run_with_backend(&repair, messages(), run_config, &mut stdout, &mut stderr).await;
        assert_eq!(exit.code, 0);
        assert_eq!(std::fs::read_to_string(repaired).unwrap(), "fixed");
        let output: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
        assert_eq!(output["verification"]["repair_requested"], true);
        assert_eq!(output["verification"]["tools_executed"], 1);
    }

    #[tokio::test]
    async fn jsonl_is_monotonic_parseable_and_has_no_diagnostics() {
        let temp = tempfile::tempdir().unwrap();
        let backend = FakeBackend::new(vec![text_turn("hello")]);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let exit = run_with_backend(
            &backend,
            messages(),
            config(temp.path(), OutputFormat::Jsonl),
            &mut stdout,
            &mut stderr,
        )
        .await;

        assert_eq!(exit.code, 0);
        assert!(stderr.is_empty());
        let lines: Vec<serde_json::Value> = String::from_utf8(stdout)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(lines.len(), 5);
        for (sequence, line) in lines.iter().enumerate() {
            assert_eq!(line["schema"], SCHEMA);
            assert_eq!(line["sequence"], sequence);
            assert_eq!(
                line["run_id"],
                config(temp.path(), OutputFormat::Jsonl).run_id.to_string()
            );
        }
        assert_eq!(lines[0]["event"]["type"], "run_start");
        assert_eq!(lines[2]["event"]["type"], "assistant_output");
        assert_eq!(lines[4]["event"]["type"], "completion");
        assert_eq!(
            backend.prepared(),
            vec![(
                crate::request::RequestKind::Headless,
                "fake".into(),
                "fake-model".into()
            )]
        );
    }

    #[tokio::test]
    async fn reasoning_output_is_emitted_only_when_requested() {
        let temp = tempfile::tempdir().unwrap();
        let turn = || {
            Ok(vec![
                Ok(ProviderEvent::Reasoning("private chain".into())),
                Ok(ProviderEvent::Text("answer".into())),
                Ok(ProviderEvent::Finish {
                    reason: FinishReason::Stop,
                    usage: None,
                }),
                Ok(ProviderEvent::Done),
            ])
        };
        for (show, expected) in [(false, false), (true, true)] {
            let backend = FakeBackend::new(vec![turn()]);
            let mut run_config = config(temp.path(), OutputFormat::Jsonl);
            run_config.show_thinking = show;
            let mut stdout = Vec::new();
            let mut stderr = Vec::new();
            assert_eq!(
                run_with_backend(&backend, messages(), run_config, &mut stdout, &mut stderr,)
                    .await
                    .code,
                0
            );
            assert_eq!(
                String::from_utf8(stdout)
                    .unwrap()
                    .contains("reasoning_output"),
                expected
            );
        }
    }

    #[tokio::test]
    async fn aggregate_json_and_last_message_are_deterministic() {
        let temp = tempfile::tempdir().unwrap();
        let last = temp.path().join("last.txt");
        let backend = FakeBackend::new(vec![text_turn("final answer")]);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut run_config = config(temp.path(), OutputFormat::Json);
        run_config.output_last_message = Some(last.clone());

        let exit =
            run_with_backend(&backend, messages(), run_config, &mut stdout, &mut stderr).await;

        assert_eq!(exit.code, 0);
        assert!(stderr.is_empty());
        let output: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
        assert_eq!(output["schema"], SCHEMA);
        assert_eq!(output["success"], true);
        assert_eq!(output["final_message"], "final answer");
        assert_eq!(std::fs::read_to_string(last).unwrap(), "final answer");
    }

    #[tokio::test]
    async fn unapproved_mutation_fails_closed_without_side_effect() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("blocked.txt");
        let backend = FakeBackend::new(vec![tool_turn(
            "call-1",
            "write_file",
            serde_json::json!({"path": target, "content":"secret"}),
        )]);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let exit = run_with_backend(
            &backend,
            messages(),
            config(temp.path(), OutputFormat::Jsonl),
            &mut stdout,
            &mut stderr,
        )
        .await;

        assert_eq!(exit.code, 4);
        assert!(!target.exists());
        let text = String::from_utf8(stdout).unwrap();
        assert!(text.contains("approval_required"));
        assert!(text.contains("\"exit_code\":4"));
        assert!(stderr.is_empty());
    }

    #[tokio::test]
    async fn yolo_executes_but_persistent_deny_still_wins() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("allowed.txt");
        let backend = FakeBackend::new(vec![
            tool_turn(
                "call-1",
                "write_file",
                serde_json::json!({"path": target, "content":"ok"}),
            ),
            text_turn("done"),
        ]);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut run_config = config(temp.path(), OutputFormat::Jsonl);
        run_config.approval_mode = HeadlessApprovalMode::Yolo;
        let exit =
            run_with_backend(&backend, messages(), run_config, &mut stdout, &mut stderr).await;
        assert_eq!(exit.code, 0);
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "ok");

        std::fs::remove_file(&target).unwrap();
        let backend = FakeBackend::new(vec![
            tool_turn(
                "call-2",
                "write_file",
                serde_json::json!({"path": target, "content":"no"}),
            ),
            text_turn("denied handled"),
        ]);
        let mut run_config = config(temp.path(), OutputFormat::Jsonl);
        run_config.approval_mode = HeadlessApprovalMode::Yolo;
        run_config.approval_rules = vec![ApprovalRule::DenyTool("write_file".into())];
        stdout.clear();
        let exit =
            run_with_backend(&backend, messages(), run_config, &mut stdout, &mut stderr).await;
        assert_eq!(exit.code, 0);
        assert!(!target.exists());
        assert!(String::from_utf8(stdout)
            .unwrap()
            .contains("\"status\":\"denied\""));
    }

    #[tokio::test]
    async fn persistent_allow_executes_without_yolo() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("allowed-by-rule.txt");
        let backend = FakeBackend::new(vec![
            tool_turn(
                "call-1",
                "write_file",
                serde_json::json!({"path": target, "content":"allowed"}),
            ),
            text_turn("done"),
        ]);
        let mut run_config = config(temp.path(), OutputFormat::Jsonl);
        run_config.approval_rules = vec![ApprovalRule::AllowTool("write_file".into())];
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let exit =
            run_with_backend(&backend, messages(), run_config, &mut stdout, &mut stderr).await;

        assert_eq!(exit.code, 0);
        assert_eq!(std::fs::read_to_string(target).unwrap(), "allowed");
    }

    #[tokio::test]
    async fn auto_review_executes_closed_read_only_shell_pipeline() {
        let temp = tempfile::tempdir().unwrap();
        let backend = FakeBackend::new(vec![
            tool_turn(
                "call-read",
                "run_command",
                serde_json::json!({"command":"printf safe | wc -c"}),
            ),
            text_turn("done"),
        ]);
        let mut run_config = config(temp.path(), OutputFormat::Jsonl);
        run_config.approval_mode = HeadlessApprovalMode::AutoReview;
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let exit =
            run_with_backend(&backend, messages(), run_config, &mut stdout, &mut stderr).await;

        assert_eq!(exit.code, 0, "{}", String::from_utf8_lossy(&stdout));
        let output = String::from_utf8(stdout).unwrap();
        assert!(output.contains("exit code: 0"), "{output}");
        assert!(output.contains("done"), "{output}");
    }

    #[tokio::test]
    async fn auto_review_executes_read_only_find_sort_pipeline() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir(temp.path().join("src")).unwrap();
        std::fs::write(temp.path().join("src/lib.rs"), "// fixture\n").unwrap();
        let backend = FakeBackend::new(vec![
            tool_turn(
                "call-read",
                "run_command",
                serde_json::json!({"command":"find src -maxdepth 3 -type f | sort"}),
            ),
            text_turn("done"),
        ]);
        let mut run_config = config(temp.path(), OutputFormat::Jsonl);
        run_config.approval_mode = HeadlessApprovalMode::AutoReview;
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let exit =
            run_with_backend(&backend, messages(), run_config, &mut stdout, &mut stderr).await;

        assert_eq!(exit.code, 0, "{}", String::from_utf8_lossy(&stdout));
        let output = String::from_utf8(stdout).unwrap();
        assert!(output.contains("src/lib.rs"), "{output}");
        assert!(output.contains("done"), "{output}");
    }

    #[tokio::test]
    async fn rich_ask_rule_blocks_yolo_without_side_effect() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("asked.txt");
        let backend = FakeBackend::new(vec![tool_turn(
            "call-ask",
            "write_file",
            serde_json::json!({"path": target, "content":"blocked"}),
        )]);
        let mut run_config = config(temp.path(), OutputFormat::Jsonl);
        run_config.approval_mode = HeadlessApprovalMode::Yolo;
        run_config.permission_rules = vec![crate::permission::PermissionRule {
            decision: crate::permission::PermissionDecision::Ask,
            matcher: crate::permission::PermissionMatcher::Tool {
                tool: "write_file".into(),
            },
        }];
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let exit =
            run_with_backend(&backend, messages(), run_config, &mut stdout, &mut stderr).await;

        assert_eq!(exit.code, 4);
        assert!(!target.exists());
        assert!(String::from_utf8(stdout).unwrap().contains("explicit Ask"));
    }

    #[tokio::test]
    async fn yolo_executes_prepared_compound_shell_without_an_extra_prompt() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("unsafe.txt");
        let backend = FakeBackend::new(vec![
            tool_turn(
                "call-shell",
                "run_command",
                serde_json::json!({"command":format!("printf allowed > {}", target.display())}),
            ),
            text_turn("done"),
        ]);
        let mut run_config = config(temp.path(), OutputFormat::Jsonl);
        run_config.approval_mode = HeadlessApprovalMode::Yolo;
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let exit =
            run_with_backend(&backend, messages(), run_config, &mut stdout, &mut stderr).await;

        assert_eq!(exit.code, 0);
        assert_eq!(std::fs::read_to_string(target).unwrap(), "allowed");
        assert!(String::from_utf8(stdout).unwrap().contains("done"));
    }

    #[tokio::test]
    async fn rich_allow_rule_executes_without_yolo() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("rich-allowed.txt");
        let backend = FakeBackend::new(vec![
            tool_turn(
                "call-rich",
                "write_file",
                serde_json::json!({"path": target, "content":"allowed"}),
            ),
            text_turn("done"),
        ]);
        let mut run_config = config(temp.path(), OutputFormat::Jsonl);
        run_config.permission_rules = vec![crate::permission::PermissionRule {
            decision: crate::permission::PermissionDecision::Allow,
            matcher: crate::permission::PermissionMatcher::Tool {
                tool: "write_file".into(),
            },
        }];
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let exit =
            run_with_backend(&backend, messages(), run_config, &mut stdout, &mut stderr).await;

        assert_eq!(exit.code, 0);
        assert_eq!(std::fs::read_to_string(target).unwrap(), "allowed");
    }

    #[tokio::test]
    async fn automatic_exact_skill_activation_updates_only_the_next_provider_prompt() {
        let temp = tempfile::tempdir().unwrap();
        let skill_dir = temp.path().join("skills/deploy");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: deploy\ndescription: Deploy carefully\n---\nSKILL-BODY-NEXT-TURN",
        )
        .unwrap();
        let registry = crate::skills::SkillRegistry::discover(
            crate::skills::SkillRoots {
                project: Some(temp.path().join("skills")),
                project_boundary: Some(temp.path().to_path_buf()),
                user: None,
                compatibility: Vec::new(),
            },
            &[],
        );
        let backend = FakeBackend::new(vec![
            tool_turn(
                "call-skill",
                "activate_skill",
                serde_json::json!({"id":"deploy"}),
            ),
            text_turn("done"),
        ]);
        let mut run_config = config(temp.path(), OutputFormat::Jsonl);
        run_config.approval_mode = HeadlessApprovalMode::RequireApproval;
        run_config.skills = std::sync::Arc::new(std::sync::Mutex::new(registry));
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut skill_messages = messages();
        skill_messages.insert(
            0,
            ChatMessage {
                role: "system".into(),
                content: crate::image_content::ModelContent::text("base prompt"),
                tool_calls: None,
                tool_call_id: None,
                response_items: Vec::new(),
            },
        );

        let exit = run_with_backend(
            &backend,
            skill_messages,
            run_config.clone(),
            &mut stdout,
            &mut stderr,
        )
        .await;

        assert_eq!(exit.code, 0);
        let requests = backend.requests();
        assert_eq!(requests.len(), 2);
        assert!(!requests[0][0]
            .content
            .as_str()
            .unwrap()
            .contains("SKILL-BODY-NEXT-TURN"));
        assert!(requests[1][0]
            .content
            .as_str()
            .unwrap()
            .contains("SKILL-BODY-NEXT-TURN"));
        assert!(run_config.skills.lock().unwrap().is_active("deploy"));
    }

    #[tokio::test]
    async fn interactions_and_network_boundary_fail_even_with_yolo() {
        let temp = tempfile::tempdir().unwrap();
        for (name, arguments, reason, code) in [
            (
                "ask_question",
                serde_json::json!({"question":"choose"}),
                "unavailable tool",
                6,
            ),
            (
                "request_user_input",
                serde_json::json!({"question":"choose", "kind":"free_text"}),
                "headless interaction",
                4,
            ),
            (
                "web_fetch",
                serde_json::json!({"url":"https://example.com"}),
                "--allow-network",
                4,
            ),
        ] {
            let backend = FakeBackend::new(vec![tool_turn("call-1", name, arguments)]);
            let mut run_config = config(temp.path(), OutputFormat::Jsonl);
            run_config.approval_mode = HeadlessApprovalMode::Yolo;
            let mut stdout = Vec::new();
            let mut stderr = Vec::new();
            let exit =
                run_with_backend(&backend, messages(), run_config, &mut stdout, &mut stderr).await;
            assert_eq!(exit.code, code);
            assert!(String::from_utf8(stdout).unwrap().contains(reason));
        }
    }

    #[tokio::test]
    async fn last_message_write_failure_is_internal_exit_eight() {
        let temp = tempfile::tempdir().unwrap();
        let backend = FakeBackend::new(vec![text_turn("answer")]);
        let mut run_config = config(temp.path(), OutputFormat::Jsonl);
        run_config.output_last_message = Some(temp.path().join("missing/last.txt"));
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let exit =
            run_with_backend(&backend, messages(), run_config, &mut stdout, &mut stderr).await;

        assert_eq!(exit.code, 8);
        assert!(String::from_utf8(stdout)
            .unwrap()
            .contains("\"exit_code\":8"));
    }

    #[test]
    fn text_tool_preview_keeps_utf8_head_tail_and_labels_omission() {
        let source = format!("HEAD{}TAIL", "界".repeat(300));
        let preview = clipped_preview(&source, 200);
        assert!(preview.starts_with("HEAD"));
        assert!(preview.ends_with("TAIL"));
        assert!(preview.contains("characters omitted from tool output preview"));
        let lines = preview.lines().collect::<Vec<_>>();
        let retained =
            lines.first().unwrap().chars().count() + lines.last().unwrap().chars().count();
        assert!(preview.contains(&format!(
            "{} characters omitted",
            source.chars().count() - retained
        )));
    }

    #[tokio::test]
    async fn provider_and_max_turn_failures_map_to_stable_codes() {
        let temp = tempfile::tempdir().unwrap();
        for (error, expected) in [
            (
                ProviderError::Transport(crate::agent::stream::ProviderTransportFailure::Connect),
                5,
            ),
            (ProviderError::MalformedData("bad".into()), 6),
        ] {
            let backend = FakeBackend::new(vec![Err(error)]);
            let mut stdout = Vec::new();
            let mut stderr = Vec::new();
            let exit = run_with_backend(
                &backend,
                messages(),
                config(temp.path(), OutputFormat::Jsonl),
                &mut stdout,
                &mut stderr,
            )
            .await;
            assert_eq!(exit.code, expected);
            assert!(String::from_utf8(stdout)
                .unwrap()
                .contains(&format!("\"exit_code\":{expected}")));
        }

        std::fs::write(temp.path().join("existing"), "content").unwrap();
        let backend = FakeBackend::new(vec![tool_turn(
            "call-1",
            "read_file",
            serde_json::json!({"path":"existing"}),
        )]);
        let mut run_config = config(temp.path(), OutputFormat::Jsonl);
        run_config.max_turns = 1;
        run_config.approval_mode = HeadlessApprovalMode::AutoReview;
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let exit =
            run_with_backend(&backend, messages(), run_config, &mut stdout, &mut stderr).await;
        assert_eq!(exit.code, 7);
        assert!(String::from_utf8(stdout).unwrap().contains("maximum_turns"));
    }

    struct BrokenWriter;

    impl Write for BrokenWriter {
        fn write(&mut self, _buffer: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "closed",
            ))
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn downstream_broken_pipe_is_success() {
        let temp = tempfile::tempdir().unwrap();
        let backend = FakeBackend::new(vec![text_turn("ignored")]);
        let mut stdout = BrokenWriter;
        let mut stderr = Vec::new();
        let exit = run_with_backend(
            &backend,
            messages(),
            config(temp.path(), OutputFormat::Jsonl),
            &mut stdout,
            &mut stderr,
        )
        .await;
        assert_eq!(exit.code, 0);
    }

    #[tokio::test]
    async fn unknown_tool_selection_fails_before_backend_traffic() {
        let temp = tempfile::tempdir().unwrap();
        let backend = FakeBackend::new(vec![text_turn("must not be consumed")]);
        let mut run_config = config(temp.path(), OutputFormat::Json);
        run_config.tool_policy =
            crate::tool_selection::ToolSelectionPolicy::replacing(&["missing_tool"]);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let exit =
            run_with_backend(&backend, messages(), run_config, &mut stdout, &mut stderr).await;

        assert_eq!(exit.code, 2);
        assert!(backend.requests().is_empty());
        assert!(backend.prepared().is_empty());
        let output = String::from_utf8(stdout).unwrap();
        assert!(output.contains("tool selection failed"));
        assert!(output.contains("missing_tool"));
        assert!(stderr.is_empty());
    }

    #[tokio::test]
    async fn explicit_tools_fail_before_traffic_for_a_toolless_model() {
        let temp = tempfile::tempdir().unwrap();
        let backend = FakeBackend::new(vec![text_turn("must not be consumed")]);
        let mut run_config = config(temp.path(), OutputFormat::Json);
        run_config.tool_policy =
            crate::tool_selection::ToolSelectionPolicy::replacing(&["read_file"]);
        run_config.tool_constraints.tools_supported = false;
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let exit =
            run_with_backend(&backend, messages(), run_config, &mut stdout, &mut stderr).await;

        assert_eq!(exit.code, 2);
        assert!(backend.requests().is_empty());
        assert!(String::from_utf8(stdout)
            .unwrap()
            .contains("does not support tools"));
    }

    #[tokio::test]
    async fn headless_advertises_the_resolved_registry_order() {
        let temp = tempfile::tempdir().unwrap();
        let backend = FakeBackend::new(vec![text_turn("done")]);
        let mut run_config = config(temp.path(), OutputFormat::Json);
        run_config.tool_policy = crate::tool_selection::ToolSelectionPolicy::from_cli(
            Some(vec!["write_file,read_file".into()]),
            vec!["search_files".into()],
            vec!["write_file".into()],
        )
        .unwrap();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let exit =
            run_with_backend(&backend, messages(), run_config, &mut stdout, &mut stderr).await;

        assert_eq!(exit.code, 0);
        assert_eq!(backend.tool_sets(), [vec!["read_file", "search_files"]]);
    }

    #[tokio::test]
    async fn provider_cannot_execute_a_tool_removed_by_selection() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("must-not-exist");
        let backend = FakeBackend::new(vec![tool_turn(
            "call-hidden",
            "write_file",
            serde_json::json!({"path":target, "content":"no"}),
        )]);
        let mut run_config = config(temp.path(), OutputFormat::Json);
        run_config.approval_mode = HeadlessApprovalMode::Yolo;
        run_config.tool_policy =
            crate::tool_selection::ToolSelectionPolicy::replacing(&["read_file"]);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let exit =
            run_with_backend(&backend, messages(), run_config, &mut stdout, &mut stderr).await;

        assert_eq!(exit.code, 6);
        assert!(!target.exists());
        assert!(String::from_utf8(stdout)
            .unwrap()
            .contains("unavailable tool"));
        assert_eq!(backend.tool_sets(), [vec!["read_file"]]);
    }
}

#[cfg(test)]
mod harness_tracking_tests {
    use super::*;
    #[tokio::test]
    async fn hrs253_headless_early_drop_persists_interruption() {
        let root = tempfile::tempdir().unwrap();
        let sink = crate::session_outcome::OutcomeSink::new(root.path().into());
        let id = Uuid::new_v4();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut emitter = HeadlessEmitter::new_for_contract(
            OutputFormat::Json,
            id,
            &mut stdout,
            &mut stderr,
            false,
            HeadlessOutputRedactions::default(),
        );
        let mut tracker =
            crate::session_outcome::OutcomeTracker::for_run(id.to_string(), id, "/tmp/test")
                .with_sink(sink.clone());
        tracker.record_user_message();
        emitter.outcome = Some(crate::session_outcome::ScopedOutcome(tracker));
        // A broken output pipe or cancelled future can skip finish().
        drop(emitter);
        sink.flush_async().await;
        let outcome = crate::session_outcome::load_outcome(&id.to_string(), root.path())
            .unwrap()
            .unwrap();
        assert_eq!(
            outcome.execution,
            crate::session_outcome::ExecutionStatus::Interrupted
        );
        assert!(!outcome.verified_positive());
    }

    #[test]
    fn hrs253_headless_completion_persists_without_acceptance() {
        let root = tempfile::tempdir().unwrap();
        let sink = crate::session_outcome::OutcomeSink::new(root.path().into());
        let id = Uuid::new_v4();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut emitter = HeadlessEmitter::new_for_contract(
            OutputFormat::Json,
            id,
            &mut stdout,
            &mut stderr,
            false,
            HeadlessOutputRedactions::default(),
        );
        let mut tracker =
            crate::session_outcome::OutcomeTracker::for_run(id.to_string(), id, "/tmp/test")
                .with_sink(sink.clone());
        tracker.record_user_message();
        emitter.outcome = Some(crate::session_outcome::ScopedOutcome(tracker));
        emitter.record_verification(crate::headless_contract::VerificationOutcome {
            report: crate::headless_contract::VerificationReport::parse(
                r#"{"passed":true,"summary":"Model says checks passed","issues":[],"evidence":[]}"#,
            )
            .unwrap(),
            repair_requested: false,
            tools_executed: 0,
        });
        emitter.complete("done".into()).unwrap();
        emitter.finish().unwrap();
        sink.flush().unwrap();
        let outcome = crate::session_outcome::load_outcome(&id.to_string(), root.path())
            .unwrap()
            .unwrap();
        assert_eq!(
            outcome.execution,
            crate::session_outcome::ExecutionStatus::Completed
        );
        assert!(!outcome.verified_positive());
        assert_eq!(
            outcome.verification,
            crate::session_outcome::VerificationStatus::Unverified
        );
        assert!(outcome.events.iter().any(
            |event| event.kind == "model_verification_report" && event.succeeded == Some(true)
        ));
    }
}
