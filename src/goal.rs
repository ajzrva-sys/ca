use crate::agent::client::{ChatBackend, ChatMessage, ToolDef, ToolFunction};
use crate::agent::stream::{AssistantTurnAssembler, UsageInfo};
use crate::terminal_text::sanitize_terminal_text;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

pub const DEFAULT_MAX_CONTINUATIONS: u32 = 12;
pub const DEFAULT_MAX_TOKENS: u64 = 100_000;
pub const DEFAULT_MAX_WORKING_MINUTES: u64 = 30;
pub const ABSOLUTE_MAX_CONTINUATIONS: u32 = 100;
pub const ABSOLUTE_MAX_TOKENS: u64 = 10_000_000;
pub const ABSOLUTE_MAX_WORKING_MINUTES: u64 = 24 * 60;
const MAX_CONDITION_BYTES: usize = 4 * 1024;
const MAX_RATIONALE_BYTES: usize = 2 * 1024;
const MAX_FEEDBACK_BYTES: usize = 4 * 1024;
const MAX_EVIDENCE_ITEMS: usize = 16;
const MAX_EVIDENCE_ITEM_BYTES: usize = 512;
const MAX_LAST_RESPONSE_BYTES: usize = 4 * 1024;
const MAX_WORK_EVENTS: usize = 32;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GoalConfig {
    #[serde(default = "default_max_continuations")]
    pub max_continuations: u32,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u64,
    #[serde(default = "default_max_working_minutes")]
    pub max_working_minutes: u64,
}

impl Default for GoalConfig {
    fn default() -> Self {
        Self {
            max_continuations: DEFAULT_MAX_CONTINUATIONS,
            max_tokens: DEFAULT_MAX_TOKENS,
            max_working_minutes: DEFAULT_MAX_WORKING_MINUTES,
        }
    }
}

impl GoalConfig {
    pub fn validate(self) -> Result<Self, GoalError> {
        if self.max_continuations == 0
            || self.max_continuations > ABSOLUTE_MAX_CONTINUATIONS
            || self.max_tokens == 0
            || self.max_tokens > ABSOLUTE_MAX_TOKENS
            || self.max_working_minutes == 0
            || self.max_working_minutes > ABSOLUTE_MAX_WORKING_MINUTES
        {
            return Err(GoalError::InvalidLimits);
        }
        Ok(self)
    }
}

const fn default_max_continuations() -> u32 {
    DEFAULT_MAX_CONTINUATIONS
}

const fn default_max_tokens() -> u64 {
    DEFAULT_MAX_TOKENS
}

const fn default_max_working_minutes() -> u64 {
    DEFAULT_MAX_WORKING_MINUTES
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct GoalLimits {
    pub max_continuations: u32,
    pub max_tokens: u64,
    pub max_working_seconds: u64,
}

impl GoalLimits {
    pub fn from_config(config: GoalConfig) -> Result<Self, GoalError> {
        let config = config.validate()?;
        Ok(Self {
            max_continuations: config.max_continuations,
            max_tokens: config.max_tokens,
            max_working_seconds: config.max_working_minutes.saturating_mul(60),
        })
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GoalStatus {
    Active,
    Paused,
    Complete,
    Unreachable,
    Blocked,
    Cleared,
    Interrupted,
    UsageLimited,
    BudgetLimited,
}

impl GoalStatus {
    pub fn is_terminal(self) -> bool {
        !matches!(self, Self::Active | Self::Paused)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Paused => "paused",
            Self::Complete => "complete",
            Self::Unreachable => "unreachable",
            Self::Blocked => "blocked",
            Self::Cleared => "cleared",
            Self::Interrupted => "interrupted",
            Self::UsageLimited => "usage limited",
            Self::BudgetLimited => "budget limited",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GoalPauseReason {
    User,
    Approval,
    Interaction,
    UserInput,
    EvaluationFailed,
}

impl GoalPauseReason {
    pub fn label(self) -> &'static str {
        match self {
            Self::User => "paused by user",
            Self::Approval => "approval required",
            Self::Interaction => "user input required",
            Self::UserInput => "user input received",
            Self::EvaluationFailed => "evaluation failed",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GoalDecision {
    Continue,
    Complete,
    Unreachable,
    Blocked,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GoalEvaluation {
    pub decision: GoalDecision,
    pub rationale: String,
    #[serde(default)]
    pub feedback: String,
    #[serde(default)]
    pub evidence: Vec<String>,
    pub created_at: DateTime<Utc>,
}

impl GoalEvaluation {
    fn validate(mut self) -> Result<Self, GoalError> {
        self.rationale = validate_bounded_text(self.rationale, MAX_RATIONALE_BYTES)?;
        self.feedback = validate_optional_bounded_text(self.feedback, MAX_FEEDBACK_BYTES)?;
        if self.evidence.len() > MAX_EVIDENCE_ITEMS {
            return Err(GoalError::InvalidEvaluation);
        }
        self.evidence = self
            .evidence
            .into_iter()
            .map(|item| validate_bounded_text(item, MAX_EVIDENCE_ITEM_BYTES))
            .collect::<Result<Vec<_>, _>>()?;
        if self.decision == GoalDecision::Continue && self.feedback.is_empty() {
            return Err(GoalError::InvalidEvaluation);
        }
        Ok(self)
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GoalState {
    pub id: Uuid,
    #[serde(default)]
    pub source_goal_id: Option<Uuid>,
    pub condition: String,
    pub status: GoalStatus,
    #[serde(default)]
    pub pause_reason: Option<GoalPauseReason>,
    pub limits: GoalLimits,
    #[serde(default)]
    pub continuations: u32,
    #[serde(default)]
    pub combined_provider_tokens: u64,
    #[serde(default)]
    pub unknown_usage_requests: u32,
    #[serde(default)]
    pub accumulated_working_seconds: u64,
    pub active_since: Option<DateTime<Utc>>,
    pub started_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub last_evaluation: Option<GoalEvaluation>,
}

impl std::fmt::Debug for GoalState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("GoalState")
            .field("id", &self.id)
            .field("source_goal_id", &self.source_goal_id)
            .field("condition", &"[REDACTED]")
            .field("status", &self.status)
            .field("pause_reason", &self.pause_reason)
            .field("limits", &self.limits)
            .field("continuations", &self.continuations)
            .field("combined_provider_tokens", &self.combined_provider_tokens)
            .field("unknown_usage_requests", &self.unknown_usage_requests)
            .field(
                "accumulated_working_seconds",
                &self.accumulated_working_seconds,
            )
            .finish_non_exhaustive()
    }
}

impl GoalState {
    pub fn start(
        condition: String,
        limits: GoalLimits,
        now: DateTime<Utc>,
    ) -> Result<Self, GoalError> {
        let condition = validate_condition(condition)?;
        Ok(Self {
            id: Uuid::new_v4(),
            source_goal_id: None,
            condition,
            status: GoalStatus::Active,
            pause_reason: None,
            limits,
            continuations: 0,
            combined_provider_tokens: 0,
            unknown_usage_requests: 0,
            accumulated_working_seconds: 0,
            active_since: Some(now),
            started_at: now,
            updated_at: now,
            last_evaluation: None,
        })
    }

    pub fn working_seconds_at(&self, now: DateTime<Utc>) -> u64 {
        let active = self
            .active_since
            .and_then(|since| (now - since).to_std().ok())
            .map_or(0, |duration| duration.as_secs());
        self.accumulated_working_seconds.saturating_add(active)
    }

    pub fn pause(&mut self, reason: GoalPauseReason, now: DateTime<Utc>) -> bool {
        if self.status != GoalStatus::Active {
            return false;
        }
        self.accumulated_working_seconds = self.working_seconds_at(now);
        self.active_since = None;
        self.status = GoalStatus::Paused;
        self.pause_reason = Some(reason);
        self.updated_at = now;
        true
    }

    pub fn resume(&mut self, now: DateTime<Utc>) -> Result<(), GoalError> {
        if self.status != GoalStatus::Paused {
            return Err(GoalError::NotPaused);
        }
        self.status = GoalStatus::Active;
        self.pause_reason = None;
        self.active_since = Some(now);
        self.updated_at = now;
        Ok(())
    }

    pub fn pause_after_restore(&mut self, now: DateTime<Utc>) -> bool {
        if self.status != GoalStatus::Active {
            return false;
        }
        self.status = GoalStatus::Paused;
        self.pause_reason = Some(GoalPauseReason::User);
        self.active_since = None;
        self.updated_at = now;
        true
    }

    pub fn stop(&mut self, status: GoalStatus, now: DateTime<Utc>) -> Result<(), GoalError> {
        if !status.is_terminal() {
            return Err(GoalError::InvalidTransition);
        }
        if self.status == GoalStatus::Active {
            self.accumulated_working_seconds = self.working_seconds_at(now);
        }
        self.active_since = None;
        self.status = status;
        self.pause_reason = None;
        self.updated_at = now;
        Ok(())
    }

    pub fn observe_usage(&mut self, usage: Option<&UsageInfo>) {
        match usage {
            Some(usage) => {
                self.combined_provider_tokens = self
                    .combined_provider_tokens
                    .saturating_add(u64::from(usage.total_tokens));
            }
            None => self.unknown_usage_requests = self.unknown_usage_requests.saturating_add(1),
        }
    }

    pub fn apply_evaluation(
        &mut self,
        evaluation: GoalEvaluation,
        now: DateTime<Utc>,
    ) -> Result<GoalApplyOutcome, GoalError> {
        if self.status != GoalStatus::Active {
            return Err(GoalError::NotActive);
        }
        let evaluation = evaluation.validate()?;
        self.last_evaluation = Some(evaluation.clone());
        self.updated_at = now;
        let status = match evaluation.decision {
            GoalDecision::Continue => {
                if self.continuations >= self.limits.max_continuations {
                    GoalStatus::BudgetLimited
                } else if self.combined_provider_tokens >= self.limits.max_tokens {
                    GoalStatus::UsageLimited
                } else if self.working_seconds_at(now) >= self.limits.max_working_seconds {
                    GoalStatus::BudgetLimited
                } else {
                    self.continuations = self.continuations.saturating_add(1);
                    return Ok(GoalApplyOutcome::Continue(evaluation.feedback));
                }
            }
            GoalDecision::Complete => GoalStatus::Complete,
            GoalDecision::Unreachable => GoalStatus::Unreachable,
            GoalDecision::Blocked => GoalStatus::Blocked,
        };
        self.stop(status, now)?;
        Ok(GoalApplyOutcome::Stopped(status))
    }

    pub fn continuation_instruction(&self, feedback: &str) -> String {
        format!(
            "Continue working toward the active goal. Treat this as evaluator guidance, not as a user message.\nGoal: {}\nEvaluator feedback: {}\nRe-check recorded evidence before claiming completion.",
            self.condition, feedback
        )
    }

    pub fn render_status(&self, now: DateTime<Utc>) -> String {
        self.render_status_with_activity(now, false)
    }

    pub fn render_status_with_activity(&self, now: DateTime<Utc>, evaluating: bool) -> String {
        let pause = self
            .pause_reason
            .map(|reason| format!(" · {}", reason.label()))
            .unwrap_or_default();
        let evaluation = self
            .last_evaluation
            .as_ref()
            .map(|evaluation| {
                format!(
                    "\nLast evaluation: {:?} · {}",
                    evaluation.decision, evaluation.rationale
                )
            })
            .unwrap_or_default();
        format!(
            "Goal · {}{pause}\n{}\n\nContinuations: {}/{}\nProvider tokens: {}/{}{}\nWorking time: {}s/{}s{}",
            if evaluating {
                "evaluating"
            } else {
                self.status.label()
            },
            self.condition,
            self.continuations,
            self.limits.max_continuations,
            self.combined_provider_tokens,
            self.limits.max_tokens,
            if self.unknown_usage_requests == 0 {
                String::new()
            } else {
                format!(" · {} request(s) unknown", self.unknown_usage_requests)
            },
            self.working_seconds_at(now),
            self.limits.max_working_seconds,
            evaluation
        )
    }

    pub fn fork_full(&self) -> Self {
        let mut clone = self.clone();
        clone.source_goal_id = Some(self.id);
        clone.id = Uuid::new_v4();
        clone
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GoalApplyOutcome {
    Continue(String),
    Stopped(GoalStatus),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoalStartRequest {
    pub condition: String,
    pub limits: GoalLimits,
}

impl GoalStartRequest {
    pub fn parse(arguments: &str, config: GoalConfig) -> Result<Self, GoalError> {
        let defaults = GoalLimits::from_config(config)?;
        let mut condition = Vec::new();
        let mut continuations = None;
        let mut tokens = None;
        let mut minutes = None;
        let mut parts = arguments.split_whitespace();
        while let Some(part) = parts.next() {
            match part {
                "--iterations" => continuations = Some(parse_u32(parts.next())?),
                "--tokens" => tokens = Some(parse_u64(parts.next())?),
                "--minutes" => minutes = Some(parse_u64(parts.next())?),
                flag if flag.starts_with("--") => return Err(GoalError::InvalidArguments),
                text => condition.push(text),
            }
        }
        let limits = GoalLimits {
            max_continuations: continuations.unwrap_or(defaults.max_continuations),
            max_tokens: tokens.unwrap_or(defaults.max_tokens),
            max_working_seconds: minutes
                .unwrap_or(defaults.max_working_seconds / 60)
                .saturating_mul(60),
        };
        if limits.max_continuations == 0
            || limits.max_continuations > defaults.max_continuations
            || limits.max_tokens == 0
            || limits.max_tokens > defaults.max_tokens
            || limits.max_working_seconds == 0
            || limits.max_working_seconds > defaults.max_working_seconds
        {
            return Err(GoalError::InvalidLimits);
        }
        let condition = condition.join(" ");
        validate_condition(condition.clone())?;
        Ok(Self { condition, limits })
    }
}

fn parse_u32(value: Option<&str>) -> Result<u32, GoalError> {
    value
        .and_then(|value| value.parse().ok())
        .ok_or(GoalError::InvalidArguments)
}

fn parse_u64(value: Option<&str>) -> Result<u64, GoalError> {
    value
        .and_then(|value| value.parse().ok())
        .ok_or(GoalError::InvalidArguments)
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum GoalError {
    #[error("goal condition is invalid or contains sensitive/task-state material")]
    InvalidCondition,
    #[error("goal limits are invalid")]
    InvalidLimits,
    #[error("goal arguments are invalid")]
    InvalidArguments,
    #[error("goal evaluation is invalid")]
    InvalidEvaluation,
    #[error("goal is not active")]
    NotActive,
    #[error("goal is not paused")]
    NotPaused,
    #[error("goal transition is invalid")]
    InvalidTransition,
}

fn validate_condition(text: String) -> Result<String, GoalError> {
    let text = text.trim();
    let lower = text.to_ascii_lowercase();
    if text.is_empty()
        || text.len() > MAX_CONDITION_BYTES
        || text.chars().any(char::is_control)
        || [
            "api_key",
            "api key",
            "authorization:",
            "bearer ",
            "password=",
            "secret=",
        ]
        .iter()
        .any(|marker| lower.contains(marker))
    {
        return Err(GoalError::InvalidCondition);
    }
    Ok(sanitize_terminal_text(text))
}

fn validate_bounded_text(text: String, limit: usize) -> Result<String, GoalError> {
    let text = text.trim();
    if text.is_empty() || text.len() > limit || text.chars().any(char::is_control) {
        return Err(GoalError::InvalidEvaluation);
    }
    Ok(sanitize_terminal_text(text))
}

fn validate_optional_bounded_text(text: String, limit: usize) -> Result<String, GoalError> {
    if text.is_empty() {
        return Ok(text);
    }
    validate_bounded_text(text, limit)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoalEvidenceSnapshot {
    pub goal_id: Uuid,
    pub condition: String,
    pub workspace_label: String,
    pub last_response: String,
    pub plan: Vec<String>,
    pub work_events: Vec<String>,
    pub continuations: u32,
    pub combined_provider_tokens: u64,
    pub working_seconds: u64,
}

impl GoalEvidenceSnapshot {
    pub fn from_state(
        goal: &GoalState,
        work: &crate::work::WorkState,
        last_response: &str,
        workspace_label: &str,
        now: DateTime<Utc>,
        runtime_secrets: &[&str],
    ) -> Self {
        let plan = work
            .plan
            .as_ref()
            .map(|plan| {
                plan.tasks
                    .iter()
                    .take(MAX_EVIDENCE_ITEMS)
                    .map(|task| {
                        format!(
                            "{:?}: {} · {} evidence reference(s)",
                            task.status,
                            safe_bounded(&task.title, MAX_EVIDENCE_ITEM_BYTES, runtime_secrets),
                            task.evidence.len()
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        let work_events = work
            .events
            .iter()
            .rev()
            .take(MAX_WORK_EVENTS)
            .rev()
            .map(|event| {
                format!(
                    "{} {:?} {}",
                    event.id,
                    event.kind,
                    if event.succeeded { "passed" } else { "failed" }
                )
            })
            .collect();
        Self {
            goal_id: goal.id,
            condition: safe_bounded(&goal.condition, MAX_CONDITION_BYTES, runtime_secrets),
            workspace_label: safe_bounded(
                workspace_label,
                MAX_EVIDENCE_ITEM_BYTES,
                runtime_secrets,
            ),
            last_response: safe_bounded(last_response, MAX_LAST_RESPONSE_BYTES, runtime_secrets),
            plan,
            work_events,
            continuations: goal.continuations,
            combined_provider_tokens: goal.combined_provider_tokens,
            working_seconds: goal.working_seconds_at(now),
        }
    }

    fn prompt(&self) -> String {
        format!(
            "<goal id=\"{}\">{}</goal>\n<workspace>{}</workspace>\n<counters continuations=\"{}\" provider_tokens=\"{}\" working_seconds=\"{}\"/>\n<plan_tasks>\n{}\n</plan_tasks>\n<recorded_work_events>\n{}\n</recorded_work_events>\n<last_agent_response>\n{}\n</last_agent_response>",
            self.goal_id,
            self.condition,
            self.workspace_label,
            self.continuations,
            self.combined_provider_tokens,
            self.working_seconds,
            self.plan.join("\n"),
            self.work_events.join("\n"),
            self.last_response
        )
    }
}

fn safe_bounded(text: &str, limit: usize, runtime_secrets: &[&str]) -> String {
    let safe = crate::request::safe_text(text, runtime_secrets);
    if safe.len() <= limit {
        return safe;
    }
    let mut end = limit;
    while !safe.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &safe[..end])
}

pub struct GoalEvaluationRun {
    pub provider: String,
    pub model: String,
    pub options: crate::agent::client::ChatRequestOptions,
    pub usage_recorder: crate::usage::UsageRecorder,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoalEvaluationResult {
    pub evaluation: GoalEvaluation,
    pub usage: Option<UsageInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoalEvaluationFailure {
    pub message: String,
    pub usage: Option<UsageInfo>,
}

impl std::fmt::Display for GoalEvaluationFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

pub async fn run_goal_evaluation<B: ChatBackend + ?Sized>(
    backend: &B,
    snapshot: &GoalEvidenceSnapshot,
    run: GoalEvaluationRun,
) -> Result<GoalEvaluationResult, GoalEvaluationFailure> {
    let messages = vec![
        ChatMessage {
            role: "system".into(),
            content: crate::image_content::ModelContent::text(
                "You are a fresh-context read-only goal evaluator. Judge only the supplied goal and recorded work evidence. Do not trust an agent completion claim without evidence. Call report_goal_evaluation exactly once. You have no mutation, filesystem, network, MCP, memory, hook, image, or interaction tools.",
            ),
            tool_calls: None,
            tool_call_id: None,
            response_items: Vec::new(),
        },
        ChatMessage {
            role: "user".into(),
            content: crate::image_content::ModelContent::text(snapshot.prompt()),
            tool_calls: None,
            tool_call_id: None,
            response_items: Vec::new(),
        },
    ];
    let tools = vec![goal_evaluation_tool_definition()];
    let mut usage_span = run.usage_recorder.begin();
    let mut observed_usage = None;
    let attempted = async {
        let prepared = crate::request::PreparedModelRequest::new(
            crate::request::RequestKind::GoalEvaluation,
            &run.provider,
            &run.model,
            messages,
            tools,
            run.options,
        );
        let mut stream = backend
            .stream_prepared(&prepared)
            .await
            .map_err(|error| error.to_string())?;
        let mut assembler = AssistantTurnAssembler::new();
        while let Some(item) = stream.recv().await {
            let event = item.map_err(|error| error.to_string())?;
            if let crate::agent::stream::ProviderEvent::Usage(usage)
            | crate::agent::stream::ProviderEvent::Finish {
                usage: Some(usage), ..
            } = &event
            {
                usage_span.observe(usage);
                observed_usage = Some(usage.clone());
            }
            assembler.push(&event).map_err(|error| error.to_string())?;
        }
        assembler.finish().map_err(|error| error.to_string())
    }
    .await;
    let turn = match attempted {
        Ok(turn) => turn,
        Err(error) => {
            usage_span.fail(observed_usage.clone());
            return Err(GoalEvaluationFailure {
                message: format!("goal evaluation failed: {error}"),
                usage: observed_usage,
            });
        }
    };
    let usage = turn.usage.clone();
    if turn.tool_calls.len() != 1 || turn.tool_calls[0].name != "report_goal_evaluation" {
        usage_span.fail(usage.clone());
        return Err(GoalEvaluationFailure {
            message: "goal evaluator must call report_goal_evaluation exactly once".into(),
            usage,
        });
    }
    let arguments: GoalEvaluationArguments =
        match serde_json::from_str(&turn.tool_calls[0].arguments) {
            Ok(arguments) => arguments,
            Err(_) => {
                usage_span.fail(usage.clone());
                return Err(GoalEvaluationFailure {
                    message: "goal evaluator returned invalid report arguments".into(),
                    usage,
                });
            }
        };
    let evaluation = GoalEvaluation {
        decision: arguments.decision,
        rationale: arguments.rationale,
        feedback: arguments.feedback,
        evidence: arguments.evidence,
        created_at: Utc::now(),
    }
    .validate();
    let evaluation = match evaluation {
        Ok(evaluation) => evaluation,
        Err(_) => {
            usage_span.fail(usage.clone());
            return Err(GoalEvaluationFailure {
                message: "goal evaluator returned an invalid bounded report".into(),
                usage,
            });
        }
    };
    usage_span.complete(usage.clone());
    Ok(GoalEvaluationResult { evaluation, usage })
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GoalEvaluationArguments {
    decision: GoalDecision,
    rationale: String,
    #[serde(default)]
    feedback: String,
    #[serde(default)]
    evidence: Vec<String>,
}

pub fn goal_evaluation_tool_definition() -> ToolDef {
    ToolDef {
        def_type: "function".into(),
        function: ToolFunction {
            name: "report_goal_evaluation".into(),
            description: "Report the evidence-grounded goal disposition exactly once.".into(),
            parameters: json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["decision", "rationale"],
                "properties": {
                    "decision": { "type": "string", "enum": ["continue", "complete", "unreachable", "blocked"] },
                    "rationale": { "type": "string", "minLength": 1, "maxLength": MAX_RATIONALE_BYTES },
                    "feedback": { "type": "string", "maxLength": MAX_FEEDBACK_BYTES },
                    "evidence": {
                        "type": "array", "maxItems": MAX_EVIDENCE_ITEMS,
                        "items": { "type": "string", "minLength": 1, "maxLength": MAX_EVIDENCE_ITEM_BYTES }
                    }
                }
            }),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::client::ChatStreamItem;
    use crate::agent::stream::{FinishReason, ProviderError, ProviderEvent};
    use chrono::TimeZone;
    use std::sync::Mutex;
    use tokio::sync::mpsc;

    fn now(seconds: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(seconds, 0).unwrap()
    }

    #[test]
    fn defaults_and_per_run_limits_are_finite_and_only_lower() {
        let defaults = GoalLimits::from_config(GoalConfig::default()).unwrap();
        assert_eq!(defaults.max_continuations, 12);
        assert_eq!(defaults.max_tokens, 100_000);
        assert_eq!(defaults.max_working_seconds, 1_800);
        let request = GoalStartRequest::parse(
            "finish safely --iterations 3 --tokens 900 --minutes 5",
            GoalConfig::default(),
        )
        .unwrap();
        assert_eq!(request.condition, "finish safely");
        assert_eq!(request.limits.max_continuations, 3);
        assert!(GoalStartRequest::parse("finish --iterations 13", GoalConfig::default()).is_err());
        assert!(GoalStartRequest::parse("finish --minutes 0", GoalConfig::default()).is_err());
    }

    #[test]
    fn condition_validation_does_not_echo_sensitive_input() {
        let error = GoalStartRequest::parse("use api_key=hunter2", GoalConfig::default())
            .unwrap_err()
            .to_string();
        assert!(!error.contains("hunter2"));
    }

    #[test]
    fn pause_resume_and_terminal_transitions_preserve_working_time() {
        let mut goal = GoalState::start(
            "finish the task".into(),
            GoalLimits::from_config(GoalConfig::default()).unwrap(),
            now(100),
        )
        .unwrap();
        assert!(goal.pause(GoalPauseReason::Approval, now(110)));
        assert_eq!(goal.working_seconds_at(now(500)), 10);
        goal.resume(now(500)).unwrap();
        goal.stop(GoalStatus::Interrupted, now(507)).unwrap();
        assert_eq!(goal.accumulated_working_seconds, 17);
    }

    #[test]
    fn limits_stop_before_an_extra_continuation() {
        let mut goal = GoalState::start(
            "finish".into(),
            GoalLimits {
                max_continuations: 1,
                max_tokens: 10,
                max_working_seconds: 60,
            },
            now(100),
        )
        .unwrap();
        let evaluation = |second| GoalEvaluation {
            decision: GoalDecision::Continue,
            rationale: "more evidence is needed".into(),
            feedback: format!("continue {second}"),
            evidence: vec![],
            created_at: now(101),
        };
        assert!(matches!(
            goal.apply_evaluation(evaluation(false), now(101)).unwrap(),
            GoalApplyOutcome::Continue(_)
        ));
        assert!(matches!(
            goal.apply_evaluation(evaluation(true), now(102)).unwrap(),
            GoalApplyOutcome::Stopped(GoalStatus::BudgetLimited)
        ));
    }

    #[test]
    fn evidence_uses_typed_events_without_raw_command_text() {
        let goal = GoalState::start(
            "finish".into(),
            GoalLimits::from_config(GoalConfig::default()).unwrap(),
            now(100),
        )
        .unwrap();
        let mut work = crate::work::WorkState::default();
        work.record_event(
            crate::work::WorkEventKind::Command,
            "echo super-secret",
            true,
        );
        let snapshot = GoalEvidenceSnapshot::from_state(
            &goal,
            &work,
            "I finished",
            "workspace",
            now(101),
            &[],
        );
        let prompt = snapshot.prompt();
        assert!(!prompt.contains("super-secret"));
        assert!(prompt.contains("Command passed"));
    }

    #[test]
    fn evidence_redacts_runtime_secrets_before_building_the_prompt() {
        let goal = GoalState::start(
            "finish".into(),
            GoalLimits::from_config(GoalConfig::default()).unwrap(),
            now(100),
        )
        .unwrap();
        let marker = "runtime-secret-marker";
        let snapshot = GoalEvidenceSnapshot::from_state(
            &goal,
            &crate::work::WorkState::default(),
            &format!("provider returned {marker}"),
            "workspace",
            now(101),
            &[marker],
        );

        let prompt = snapshot.prompt();
        assert!(!prompt.contains(marker));
        assert!(prompt.contains("[REDACTED]"));
    }

    #[test]
    fn token_and_elapsed_limits_have_distinct_terminal_outcomes() {
        let evaluation = || GoalEvaluation {
            decision: GoalDecision::Continue,
            rationale: "more work is needed".into(),
            feedback: "continue safely".into(),
            evidence: Vec::new(),
            created_at: now(101),
        };
        let limits = GoalLimits {
            max_continuations: 12,
            max_tokens: 10,
            max_working_seconds: 60,
        };
        let mut token_limited = GoalState::start("finish".into(), limits, now(100)).unwrap();
        token_limited.observe_usage(Some(&UsageInfo {
            prompt_tokens: 7,
            completion_tokens: 3,
            total_tokens: 10,
            cached_prompt_tokens: None,
            cache_write_tokens: None,
            reasoning_tokens: None,
            response_metadata: None,
            cache_usage_override: None,
        }));
        assert_eq!(
            token_limited
                .apply_evaluation(evaluation(), now(101))
                .unwrap(),
            GoalApplyOutcome::Stopped(GoalStatus::UsageLimited)
        );

        let mut elapsed = GoalState::start("finish".into(), limits, now(100)).unwrap();
        assert_eq!(
            elapsed.apply_evaluation(evaluation(), now(160)).unwrap(),
            GoalApplyOutcome::Stopped(GoalStatus::BudgetLimited)
        );
    }

    #[test]
    fn restored_active_goal_pauses_without_counting_offline_time() {
        let mut goal = GoalState::start(
            "finish".into(),
            GoalLimits::from_config(GoalConfig::default()).unwrap(),
            now(100),
        )
        .unwrap();
        goal.accumulated_working_seconds = 9;
        assert!(goal.pause_after_restore(now(10_000)));
        assert_eq!(goal.status, GoalStatus::Paused);
        assert_eq!(goal.pause_reason, Some(GoalPauseReason::User));
        assert_eq!(goal.working_seconds_at(now(20_000)), 9);
    }

    struct FakeBackend {
        turns: Mutex<Vec<Result<Vec<ChatStreamItem>, ProviderError>>>,
        requests: Mutex<Vec<(Vec<ChatMessage>, Vec<ToolDef>)>>,
    }

    #[async_trait::async_trait]
    impl ChatBackend for FakeBackend {
        async fn stream_chat(
            &self,
            messages: &[ChatMessage],
            tools: &[ToolDef],
        ) -> Result<mpsc::UnboundedReceiver<ChatStreamItem>, ProviderError> {
            self.requests
                .lock()
                .unwrap()
                .push((messages.to_vec(), tools.to_vec()));
            let turn = self.turns.lock().unwrap().remove(0)?;
            let (tx, rx) = mpsc::unbounded_channel();
            for item in turn {
                tx.send(item).unwrap();
            }
            Ok(rx)
        }
    }

    fn evaluation_events(arguments: &str) -> Vec<ChatStreamItem> {
        vec![
            Ok(ProviderEvent::ToolCallStart {
                index: 0,
                id: "goal-report".into(),
                name: "report_goal_evaluation".into(),
            }),
            Ok(ProviderEvent::ToolCallArg {
                index: 0,
                arg: arguments.into(),
            }),
            Ok(ProviderEvent::Finish {
                reason: FinishReason::ToolCalls,
                usage: Some(UsageInfo {
                    prompt_tokens: 10,
                    completion_tokens: 5,
                    total_tokens: 15,
                    cached_prompt_tokens: None,
                    cache_write_tokens: None,
                    reasoning_tokens: None,
                    response_metadata: None,
                    cache_usage_override: None,
                }),
            }),
            Ok(ProviderEvent::Done),
        ]
    }

    #[tokio::test]
    async fn evaluator_is_fresh_and_advertises_only_the_report_schema() {
        let backend = FakeBackend {
            turns: Mutex::new(vec![Ok(evaluation_events(
                r#"{"decision":"complete","rationale":"recorded test passed","evidence":["event passed"]}"#,
            ))]),
            requests: Mutex::new(Vec::new()),
        };
        let goal = GoalState::start(
            "finish".into(),
            GoalLimits::from_config(GoalConfig::default()).unwrap(),
            now(100),
        )
        .unwrap();
        let snapshot = GoalEvidenceSnapshot::from_state(
            &goal,
            &crate::work::WorkState::default(),
            "done",
            "workspace",
            now(101),
            &[],
        );
        let ledger =
            std::sync::Arc::new(std::sync::Mutex::new(crate::usage::UsageLedger::default()));
        let result = run_goal_evaluation(
            &backend,
            &snapshot,
            GoalEvaluationRun {
                provider: "local".into(),
                model: "model".into(),
                options: Default::default(),
                usage_recorder: crate::usage::UsageRecorder::new(
                    ledger,
                    crate::usage::UsageKind::GoalEvaluation,
                    "local",
                    "model",
                ),
            },
        )
        .await
        .unwrap();
        assert_eq!(result.evaluation.decision, GoalDecision::Complete);
        let requests = backend.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].0.len(), 2);
        assert_eq!(requests[0].1.len(), 1);
        assert_eq!(requests[0].1[0].function.name, "report_goal_evaluation");
    }

    #[tokio::test]
    async fn malformed_evaluator_report_is_failed_usage_with_safe_bounded_error() {
        let backend = FakeBackend {
            turns: Mutex::new(vec![Ok(evaluation_events("not-json"))]),
            requests: Mutex::new(Vec::new()),
        };
        let goal = GoalState::start(
            "finish".into(),
            GoalLimits::from_config(GoalConfig::default()).unwrap(),
            now(100),
        )
        .unwrap();
        let snapshot = GoalEvidenceSnapshot::from_state(
            &goal,
            &crate::work::WorkState::default(),
            "done",
            "workspace",
            now(101),
            &[],
        );
        let ledger =
            std::sync::Arc::new(std::sync::Mutex::new(crate::usage::UsageLedger::default()));
        let error = run_goal_evaluation(
            &backend,
            &snapshot,
            GoalEvaluationRun {
                provider: "local".into(),
                model: "model".into(),
                options: Default::default(),
                usage_recorder: crate::usage::UsageRecorder::new(
                    ledger.clone(),
                    crate::usage::UsageKind::GoalEvaluation,
                    "local",
                    "model",
                ),
            },
        )
        .await
        .unwrap_err();

        assert!(error.message.starts_with("goal evaluation failed:"));
        assert!(!error.message.contains("not-json"));
        assert_eq!(
            error.usage.as_ref().map(|usage| usage.total_tokens),
            Some(15)
        );
        let usage = ledger.lock().unwrap().snapshot();
        assert_eq!(usage.records.len(), 1);
        assert_eq!(usage.records[0].outcome, crate::usage::UsageOutcome::Failed);
        assert_eq!(
            usage.records[0]
                .usage
                .as_ref()
                .map(|usage| usage.total_tokens),
            Some(15)
        );
    }
}
