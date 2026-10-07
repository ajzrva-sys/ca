use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

macro_rules! stable_id {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
        #[serde(transparent)]
        pub struct $name(pub Uuid);

        impl $name {
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(formatter)
            }
        }
    };
}

stable_id!(PlanId);
stable_id!(TaskId);
stable_id!(WorkEventId);
stable_id!(ReviewId);
stable_id!(AgentSummaryId);

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlanStatus {
    Proposed,
    Approved,
    Rejected,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    InProgress,
    Completed,
    Blocked,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkEventKind {
    Command,
    Test,
    Review,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecordedWorkEvent {
    pub id: WorkEventId,
    pub kind: WorkEventKind,
    pub summary: String,
    pub succeeded: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EvidenceRef {
    pub event_id: WorkEventId,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlanTask {
    pub id: TaskId,
    pub title: String,
    pub status: TaskStatus,
    pub owner: Option<String>,
    pub blocked_reason: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub evidence: Vec<EvidenceRef>,
    #[serde(default)]
    pub agent: Option<crate::agent_graph::AgentTaskDefinition>,
    #[serde(default)]
    pub agent_runtime: crate::agent_graph::AgentTaskRuntime,
    pub source_task_id: Option<TaskId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlanArtifact {
    pub id: PlanId,
    pub revision: u64,
    pub status: PlanStatus,
    pub title: String,
    /// Full user-facing plan. Older artifacts retain their task-list fallback.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub approved_at: Option<DateTime<Utc>>,
    pub approved_by: Option<String>,
    #[serde(default)]
    pub tasks: Vec<PlanTask>,
    #[serde(default)]
    pub decisions: Vec<String>,
    pub source_plan_id: Option<PlanId>,
}

/// Process-local binding for an action on the exact artifact the user reviewed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlanReview {
    pub plan_id: PlanId,
    pub revision: u64,
}

impl PlanArtifact {
    pub fn review(&self) -> PlanReview {
        PlanReview {
            plan_id: self.id,
            revision: self.revision,
        }
    }

    pub fn display_status(&self) -> &'static str {
        match self.status {
            PlanStatus::Rejected => "Rejected",
            PlanStatus::Proposed => "Ready for review",
            PlanStatus::Approved
                if !self.tasks.is_empty()
                    && self
                        .tasks
                        .iter()
                        .all(|task| task.status == TaskStatus::Completed) =>
            {
                "Completed"
            }
            PlanStatus::Approved
                if self
                    .tasks
                    .iter()
                    .any(|task| task.status == TaskStatus::Blocked) =>
            {
                "Blocked"
            }
            PlanStatus::Approved => "Approved",
        }
    }

    pub fn render_document(&self) -> String {
        let mut text = format!("# {}\n\n{}\n\n", self.title, self.display_status());
        if let Some(body) = self.body.as_deref().filter(|body| !body.trim().is_empty()) {
            text.push_str(body);
            text.push_str("\n\n");
        }
        text.push_str("## Tasks\n\n");
        for (index, task) in self.tasks.iter().enumerate() {
            text.push_str(&format!(
                "{}. {} [{:?}]\n",
                index + 1,
                task.title,
                task.status
            ));
            if let Some(reason) = &task.blocked_reason {
                text.push_str(&format!("\n   Blocked: {reason}\n"));
            }
        }
        if !self.decisions.is_empty() {
            text.push_str("\n## Decisions\n\n");
            for decision in &self.decisions {
                text.push_str(&format!("- {decision}\n"));
            }
        }
        text
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkState {
    pub plan: Option<PlanArtifact>,
    #[serde(default)]
    pub plan_feedback: crate::plan_feedback::PlanFeedbackLedger,
    #[serde(default)]
    pub events: Vec<RecordedWorkEvent>,
    #[serde(default)]
    pub agent_graph: crate::agent_graph::AgentGraphState,
    pub historical_notice: Option<String>,
    /// Process-only host preview handoff. A model proposal is never persisted
    /// as automation authority or carried into a resumed/branched session.
    #[serde(skip)]
    pub pending_automation_proposal: Option<crate::automation::AutomationDefinitionV1>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReviewRecord {
    pub id: ReviewId,
    pub scope: String,
    pub summary: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub findings: Vec<crate::review::ReviewFinding>,
    #[serde(default)]
    pub truncated: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentSummary {
    pub id: AgentSummaryId,
    pub status: String,
    pub summary: String,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkMutation {
    Propose {
        expected_revision: u64,
        title: String,
        body: Option<String>,
        tasks: Vec<String>,
    },
    Edit {
        expected_revision: u64,
        title: Option<String>,
        body: Option<String>,
        decisions: Vec<String>,
    },
    UpdateTask {
        expected_revision: u64,
        task_id: TaskId,
        status: TaskStatus,
        blocked_reason: Option<String>,
    },
    RecordEvidence {
        expected_revision: u64,
        task_id: TaskId,
        event_id: WorkEventId,
        note: String,
    },
    ConfigureAgentGraph {
        expected_revision: u64,
        budget: crate::agent_graph::AgentGraphBudgetInput,
        tasks: Vec<crate::agent_graph::AgentTaskInput>,
    },
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum WorkError {
    #[error("stale plan revision: expected {expected}, current {current}")]
    StaleRevision { expected: u64, current: u64 },
    #[error("no plan exists")]
    NoPlan,
    #[error("plan title and tasks must not be empty")]
    EmptyPlan,
    #[error("unknown task: {0}")]
    UnknownTask(TaskId),
    #[error("unknown validation event: {0}")]
    UnknownEvent(WorkEventId),
    #[error("completed tasks require successful recorded evidence")]
    CompletionWithoutEvidence,
    #[error("blocked tasks require a blocked reason")]
    MissingBlockedReason,
    #[error("only the user may approve or reject a plan")]
    UserAuthorityRequired,
    #[error("invalid work tool arguments: {0}")]
    InvalidArguments(String),
    #[error(transparent)]
    AgentGraph(#[from] crate::agent_graph::AgentGraphError),
}

impl WorkState {
    pub fn revision(&self) -> u64 {
        self.plan.as_ref().map_or(0, |plan| plan.revision)
    }

    pub fn record_event(
        &mut self,
        kind: WorkEventKind,
        summary: impl Into<String>,
        succeeded: bool,
    ) -> WorkEventId {
        let id = WorkEventId::new();
        self.events.push(RecordedWorkEvent {
            id,
            kind,
            summary: summary.into(),
            succeeded,
            created_at: Utc::now(),
        });
        id
    }

    pub fn apply_model_mutation(&mut self, mutation: WorkMutation) -> Result<u64, WorkError> {
        let invalidates_existing_graph = matches!(
            &mutation,
            WorkMutation::Edit { .. }
                | WorkMutation::UpdateTask { .. }
                | WorkMutation::RecordEvidence { .. }
        );
        let expected_revision = match &mutation {
            WorkMutation::Propose {
                expected_revision, ..
            }
            | WorkMutation::Edit {
                expected_revision, ..
            }
            | WorkMutation::UpdateTask {
                expected_revision, ..
            }
            | WorkMutation::RecordEvidence {
                expected_revision, ..
            }
            | WorkMutation::ConfigureAgentGraph {
                expected_revision, ..
            } => *expected_revision,
        };
        let current = self.revision();
        if expected_revision != current {
            return Err(WorkError::StaleRevision {
                expected: expected_revision,
                current,
            });
        }
        match mutation {
            WorkMutation::Propose {
                title, body, tasks, ..
            } => {
                if title.trim().is_empty()
                    || tasks.is_empty()
                    || tasks.iter().any(|task| task.trim().is_empty())
                {
                    return Err(WorkError::EmptyPlan);
                }
                let now = Utc::now();
                self.plan = Some(PlanArtifact {
                    id: PlanId::new(),
                    revision: current + 1,
                    status: PlanStatus::Proposed,
                    title,
                    body,
                    created_at: now,
                    updated_at: now,
                    approved_at: None,
                    approved_by: None,
                    tasks: tasks
                        .into_iter()
                        .map(|title| PlanTask {
                            id: TaskId::new(),
                            title,
                            status: TaskStatus::Pending,
                            owner: None,
                            blocked_reason: None,
                            created_at: now,
                            updated_at: now,
                            evidence: Vec::new(),
                            agent: None,
                            agent_runtime: crate::agent_graph::AgentTaskRuntime::default(),
                            source_task_id: None,
                        })
                        .collect(),
                    decisions: Vec::new(),
                    source_plan_id: None,
                });
                self.agent_graph = crate::agent_graph::AgentGraphState::default();
            }
            WorkMutation::Edit {
                title,
                body,
                decisions,
                ..
            } => {
                let plan = self.plan.as_mut().ok_or(WorkError::NoPlan)?;
                let content_changed = title.as_ref().is_some_and(|title| *title != plan.title)
                    || body
                        .as_ref()
                        .is_some_and(|body| Some(body) != plan.body.as_ref())
                    || !decisions.is_empty();
                if let Some(title) = title {
                    if title.trim().is_empty() {
                        return Err(WorkError::EmptyPlan);
                    }
                    plan.title = title;
                }
                if let Some(body) = body {
                    plan.body = Some(body);
                }
                plan.decisions.extend(decisions);
                if content_changed {
                    plan.status = PlanStatus::Proposed;
                    plan.approved_at = None;
                    plan.approved_by = None;
                }
                bump(plan);
            }
            WorkMutation::UpdateTask {
                task_id,
                status,
                blocked_reason,
                ..
            } => {
                if status == TaskStatus::Blocked
                    && blocked_reason.as_deref().is_none_or(str::is_empty)
                {
                    return Err(WorkError::MissingBlockedReason);
                }
                let event_success = |event_id| {
                    self.events
                        .iter()
                        .any(|event| event.id == event_id && event.succeeded)
                };
                let plan = self.plan.as_mut().ok_or(WorkError::NoPlan)?;
                let task = plan
                    .tasks
                    .iter_mut()
                    .find(|task| task.id == task_id)
                    .ok_or(WorkError::UnknownTask(task_id))?;
                if status == TaskStatus::Completed
                    && !task
                        .evidence
                        .iter()
                        .any(|item| event_success(item.event_id))
                {
                    return Err(WorkError::CompletionWithoutEvidence);
                }
                task.status = status;
                task.blocked_reason = blocked_reason;
                task.updated_at = Utc::now();
                bump(plan);
            }
            WorkMutation::RecordEvidence {
                task_id,
                event_id,
                note,
                ..
            } => {
                if !self.events.iter().any(|event| event.id == event_id) {
                    return Err(WorkError::UnknownEvent(event_id));
                }
                let plan = self.plan.as_mut().ok_or(WorkError::NoPlan)?;
                let task = plan
                    .tasks
                    .iter_mut()
                    .find(|task| task.id == task_id)
                    .ok_or(WorkError::UnknownTask(task_id))?;
                task.evidence.push(EvidenceRef { event_id, note });
                task.updated_at = Utc::now();
                bump(plan);
            }
            WorkMutation::ConfigureAgentGraph { budget, tasks, .. } => {
                let plan = self.plan.as_mut().ok_or(WorkError::NoPlan)?;
                let task_ids = plan.tasks.iter().map(|task| task.id).collect::<Vec<_>>();
                let configuration_revision = plan.revision + 1;
                let configured = crate::agent_graph::parse_configuration(
                    plan.id,
                    configuration_revision,
                    &task_ids,
                    tasks,
                )?;
                let budget_limits =
                    crate::agent_graph::AgentGraphBudgetLimitsV1::from_input(budget)?;
                for task in &mut plan.tasks {
                    task.agent = configured.get(&task.id).cloned();
                    task.agent_runtime = crate::agent_graph::AgentTaskRuntime::default();
                }
                self.agent_graph = crate::agent_graph::AgentGraphState::configured(
                    plan.id,
                    configuration_revision,
                    budget_limits,
                );
                bump(plan);
            }
        }
        if invalidates_existing_graph {
            self.invalidate_agent_graph_after_plan_mutation();
        }
        if let Some(plan) = &self.plan {
            self.plan_feedback.refresh(plan);
        }
        Ok(self.revision())
    }

    pub fn invalidate_verification_after_recovery(&mut self) {
        self.invalidate_agent_graph_after_plan_mutation();
    }

    fn invalidate_agent_graph_after_plan_mutation(&mut self) {
        if self.agent_graph.plan_id.is_none() {
            return;
        }
        self.agent_graph.approved_revision = None;
        self.agent_graph.status = crate::agent_graph::AgentGraphStatus::Paused;
        self.agent_graph.updated_at = Some(Utc::now());
        self.agent_graph.notice = Some(
            "Graph verification invalidated by a later plan mutation; reapprove or reconfigure"
                .into(),
        );
    }

    pub fn user_decide(&mut self, expected_revision: u64, approve: bool) -> Result<u64, WorkError> {
        let current = self.revision();
        if expected_revision != current {
            return Err(WorkError::StaleRevision {
                expected: expected_revision,
                current,
            });
        }
        if self.agent_graph.status == crate::agent_graph::AgentGraphStatus::Running
            || self.plan.as_ref().is_some_and(|plan| {
                plan.tasks.iter().any(|task| {
                    task.agent_runtime.state == crate::agent_graph::AgentTaskState::Running
                })
            })
        {
            return Err(crate::agent_graph::AgentGraphError::InvalidTransition.into());
        }
        let plan = self.plan.as_mut().ok_or(WorkError::NoPlan)?;
        plan.status = if approve {
            PlanStatus::Approved
        } else {
            PlanStatus::Rejected
        };
        plan.approved_at = approve.then(Utc::now);
        plan.approved_by = approve.then(|| "user".to_string());
        bump(plan);
        let configuration_matches = self.agent_graph.plan_id == Some(plan.id)
            && self.agent_graph.configuration_revision.is_some();
        self.agent_graph.approved_revision =
            (approve && configuration_matches).then_some(plan.revision);
        Ok(plan.revision)
    }

    pub fn user_start_agent_graph(&mut self) -> Result<u64, WorkError> {
        let plan = self.plan.as_mut().ok_or(WorkError::NoPlan)?;
        if plan.status != PlanStatus::Approved {
            return Err(crate::agent_graph::AgentGraphError::PlanNotApproved.into());
        }
        if plan.tasks.is_empty() || plan.tasks.iter().any(|task| task.agent.is_none()) {
            return Err(crate::agent_graph::AgentGraphError::NotConfigured.into());
        }
        if !matches!(
            self.agent_graph.status,
            crate::agent_graph::AgentGraphStatus::Idle
                | crate::agent_graph::AgentGraphStatus::Paused
        ) {
            return Err(crate::agent_graph::AgentGraphError::InvalidTransition.into());
        }
        if !agent_graph_verification_ready(plan, &self.agent_graph) {
            return Err(crate::agent_graph::AgentGraphError::VerificationContractRequired.into());
        }
        self.agent_graph.status = crate::agent_graph::AgentGraphStatus::Running;
        self.agent_graph.notice = None;
        crate::agent_graph::synchronize(&mut plan.tasks, &mut self.agent_graph);
        bump(plan);
        Ok(plan.revision)
    }

    pub fn user_pause_agent_graph(&mut self) -> Result<u64, WorkError> {
        let plan = self.plan.as_mut().ok_or(WorkError::NoPlan)?;
        if self.agent_graph.status != crate::agent_graph::AgentGraphStatus::Running {
            return Err(crate::agent_graph::AgentGraphError::InvalidTransition.into());
        }
        self.agent_graph.status = crate::agent_graph::AgentGraphStatus::Paused;
        self.agent_graph.updated_at = Some(Utc::now());
        self.agent_graph.notice =
            Some("Paused by user; running children may be cancelled separately".into());
        bump(plan);
        Ok(plan.revision)
    }

    pub fn user_cancel_agent_graph(&mut self) -> Result<u64, WorkError> {
        let plan = self.plan.as_mut().ok_or(WorkError::NoPlan)?;
        if matches!(
            self.agent_graph.status,
            crate::agent_graph::AgentGraphStatus::Completed
                | crate::agent_graph::AgentGraphStatus::Cancelled
        ) {
            return Err(crate::agent_graph::AgentGraphError::InvalidTransition.into());
        }
        for task in plan.tasks.iter_mut().filter(|task| task.agent.is_some()) {
            if task.agent_runtime.state != crate::agent_graph::AgentTaskState::Completed {
                if let Some(attempt) = task.agent_runtime.attempts.last_mut() {
                    if !attempt.status.terminal() {
                        attempt.status = crate::agent_graph::AgentAttemptStatus::Cancelled;
                        attempt.finished_at = Some(Utc::now());
                        attempt.summary = Some("graph cancelled by user".into());
                    }
                }
                task.agent_runtime.state = crate::agent_graph::AgentTaskState::Cancelled;
                task.agent_runtime.failure = Some("graph cancelled by user".into());
                task.status = TaskStatus::Blocked;
                task.blocked_reason = Some("graph cancelled by user".into());
            }
        }
        self.agent_graph.status = crate::agent_graph::AgentGraphStatus::Cancelled;
        self.agent_graph.updated_at = Some(Utc::now());
        self.agent_graph.notice =
            Some("Graph cancelled; child changes were not rolled back".into());
        bump(plan);
        Ok(plan.revision)
    }

    pub fn user_retry_agent_task(&mut self, task_id: TaskId) -> Result<u64, WorkError> {
        let plan = self.plan.as_mut().ok_or(WorkError::NoPlan)?;
        if !agent_graph_verification_ready(plan, &self.agent_graph) {
            return Err(crate::agent_graph::AgentGraphError::VerificationContractRequired.into());
        }
        let task = plan
            .tasks
            .iter_mut()
            .find(|task| task.id == task_id)
            .ok_or(WorkError::UnknownTask(task_id))?;
        let definition = task
            .agent
            .as_ref()
            .ok_or(crate::agent_graph::AgentGraphError::NotConfigured)?;
        if task.agent_runtime.state != crate::agent_graph::AgentTaskState::Failed {
            return Err(crate::agent_graph::AgentGraphError::InvalidTransition.into());
        }
        if task.agent_runtime.attempts.len() >= usize::from(definition.max_attempts) {
            return Err(crate::agent_graph::AgentGraphError::AttemptLimit.into());
        }
        task.agent_runtime.state = crate::agent_graph::AgentTaskState::Pending;
        task.agent_runtime.failure = None;
        task.status = TaskStatus::Pending;
        task.blocked_reason = None;
        task.owner = None;
        self.agent_graph.status = crate::agent_graph::AgentGraphStatus::Running;
        self.agent_graph.notice = None;
        crate::agent_graph::synchronize(&mut plan.tasks, &mut self.agent_graph);
        bump(plan);
        Ok(plan.revision)
    }

    pub fn user_review_agent_task(&mut self, task_id: TaskId) -> Result<u64, WorkError> {
        let binding = self
            .agent_graph
            .verification_binding()
            .ok_or(crate::agent_graph::AgentGraphError::VerificationContractRequired)?;
        let plan = self.plan.as_mut().ok_or(WorkError::NoPlan)?;
        let index = plan
            .tasks
            .iter()
            .position(|task| task.id == task_id)
            .ok_or(WorkError::UnknownTask(task_id))?;
        let task = &mut plan.tasks[index];
        let definition = task
            .agent
            .clone()
            .ok_or(crate::agent_graph::AgentGraphError::NotConfigured)?;
        if !definition
            .evidence
            .contains(&crate::agent_graph::AgentEvidenceGate::HumanReview)
        {
            return Err(crate::agent_graph::AgentGraphError::InvalidTransition.into());
        }
        let attempt = task
            .agent_runtime
            .attempts
            .last_mut()
            .filter(|attempt| {
                attempt.status == crate::agent_graph::AgentAttemptStatus::EvidenceFailed
            })
            .ok_or(crate::agent_graph::AgentGraphError::InvalidTransition)?;
        let agent_id = attempt
            .agent_id
            .ok_or(crate::agent_graph::AgentGraphError::InvalidTransition)?;
        let mut verification = attempt
            .host_verification
            .clone()
            .ok_or(crate::agent_graph::AgentGraphError::VerificationContractRequired)?;
        for gate in definition
            .evidence
            .iter()
            .filter(|gate| matches!(gate, crate::agent_graph::AgentEvidenceGate::HumanReview))
        {
            let gate_sha256 = crate::agent_graph::evidence_gate_sha256(gate);
            verification
                .observations
                .retain(|observation| observation.gate_sha256 != gate_sha256);
            verification
                .observations
                .push(crate::agent_graph::AgentHostEvidenceObservationV1 {
                    gate_sha256,
                    authority: crate::agent_graph::AgentVerificationAuthority::HumanReview,
                    satisfied: true,
                    observation_sha256: None,
                });
        }
        let receipt = crate::agent_graph::verify_agent_attempt_with_host(
            Some(binding),
            task_id,
            &definition,
            attempt.number,
            agent_id,
            crate::agent_graph::AgentAttemptStatus::Completed,
            attempt.terminal_event_sequence,
            attempt.summary.as_deref(),
            &attempt.successful_tools,
            Some(&verification),
        );
        if receipt.disposition != crate::agent_graph::AgentVerificationDisposition::Pass {
            return Err(crate::agent_graph::AgentGraphError::VerificationContractRequired.into());
        }
        attempt.status = crate::agent_graph::AgentAttemptStatus::Completed;
        attempt.host_verification = Some(verification);
        attempt.verification_receipt = Some(receipt);
        task.agent_runtime.state = crate::agent_graph::AgentTaskState::Completed;
        task.agent_runtime.failure = None;
        task.status = TaskStatus::Completed;
        task.blocked_reason = None;
        task.owner = None;
        task.updated_at = Utc::now();
        self.agent_graph.status = crate::agent_graph::AgentGraphStatus::Running;
        self.agent_graph.notice = Some("Human review recorded with a host receipt".into());
        crate::agent_graph::synchronize(&mut plan.tasks, &mut self.agent_graph);
        bump(plan);
        Ok(plan.revision)
    }

    pub fn mark_agent_task_spawned(
        &mut self,
        task_id: TaskId,
        agent_id: crate::agent::subagent::AgentId,
        now: DateTime<Utc>,
    ) -> Result<u64, WorkError> {
        self.mark_agent_task_spawned_with_snapshot(task_id, agent_id, None, now)
    }

    pub fn mark_agent_task_spawned_with_snapshot(
        &mut self,
        task_id: TaskId,
        agent_id: crate::agent::subagent::AgentId,
        workspace_snapshot: Option<crate::agent_graph::AgentWorkspaceSnapshotV1>,
        now: DateTime<Utc>,
    ) -> Result<u64, WorkError> {
        let plan = self.plan.as_mut().ok_or(WorkError::NoPlan)?;
        if self.agent_graph.status != crate::agent_graph::AgentGraphStatus::Running {
            return Err(crate::agent_graph::AgentGraphError::InvalidTransition.into());
        }
        if !agent_graph_verification_ready(plan, &self.agent_graph) {
            return Err(crate::agent_graph::AgentGraphError::VerificationContractRequired.into());
        }
        let task = plan
            .tasks
            .iter_mut()
            .find(|task| task.id == task_id)
            .ok_or(WorkError::UnknownTask(task_id))?;
        let definition = task
            .agent
            .as_ref()
            .ok_or(crate::agent_graph::AgentGraphError::NotConfigured)?;
        if task.agent_runtime.state != crate::agent_graph::AgentTaskState::Ready {
            return Err(crate::agent_graph::AgentGraphError::InvalidTransition.into());
        }
        if task.agent_runtime.attempts.len() >= usize::from(definition.max_attempts) {
            return Err(crate::agent_graph::AgentGraphError::AttemptLimit.into());
        }
        task.agent_runtime
            .attempts
            .push(crate::agent_graph::AgentTaskAttempt {
                number: u8::try_from(task.agent_runtime.attempts.len() + 1)
                    .map_err(|_| crate::agent_graph::AgentGraphError::AttemptLimit)?,
                agent_id: Some(agent_id.0),
                status: crate::agent_graph::AgentAttemptStatus::Queued,
                started_at: now,
                finished_at: None,
                successful_tools: Vec::new(),
                summary: None,
                terminal_event_sequence: None,
                verification_receipt: None,
                workspace_snapshot,
                host_verification: None,
                budget_usage: crate::agent_graph::AgentGraphBudgetUsageV1::default(),
            });
        task.agent_runtime.state = crate::agent_graph::AgentTaskState::Running;
        task.status = TaskStatus::InProgress;
        task.owner = Some(format!("agent:{agent_id}"));
        task.updated_at = now;
        bump(plan);
        Ok(plan.revision)
    }

    pub fn record_agent_task_failure(
        &mut self,
        task_id: TaskId,
        reason: &str,
        now: DateTime<Utc>,
    ) -> Result<u64, WorkError> {
        let plan = self.plan.as_mut().ok_or(WorkError::NoPlan)?;
        if self.agent_graph.status != crate::agent_graph::AgentGraphStatus::Running {
            return Err(crate::agent_graph::AgentGraphError::InvalidTransition.into());
        }
        if !agent_graph_verification_ready(plan, &self.agent_graph) {
            return Err(crate::agent_graph::AgentGraphError::VerificationContractRequired.into());
        }
        let task = plan
            .tasks
            .iter_mut()
            .find(|task| task.id == task_id)
            .ok_or(WorkError::UnknownTask(task_id))?;
        let definition = task
            .agent
            .as_ref()
            .ok_or(crate::agent_graph::AgentGraphError::NotConfigured)?;
        if task.agent_runtime.state != crate::agent_graph::AgentTaskState::Ready {
            return Err(crate::agent_graph::AgentGraphError::InvalidTransition.into());
        }
        if task.agent_runtime.attempts.len() >= usize::from(definition.max_attempts) {
            return Err(crate::agent_graph::AgentGraphError::AttemptLimit.into());
        }
        let reason = crate::agent_graph::bounded_summary(reason);
        task.agent_runtime
            .attempts
            .push(crate::agent_graph::AgentTaskAttempt {
                number: u8::try_from(task.agent_runtime.attempts.len() + 1)
                    .map_err(|_| crate::agent_graph::AgentGraphError::AttemptLimit)?,
                agent_id: None,
                status: crate::agent_graph::AgentAttemptStatus::Failed,
                started_at: now,
                finished_at: Some(now),
                successful_tools: Vec::new(),
                summary: Some(reason.clone()),
                terminal_event_sequence: None,
                verification_receipt: None,
                workspace_snapshot: None,
                host_verification: None,
                budget_usage: crate::agent_graph::AgentGraphBudgetUsageV1::default(),
            });
        task.agent_runtime.state = crate::agent_graph::AgentTaskState::Failed;
        task.agent_runtime.failure = Some(reason.clone());
        task.status = TaskStatus::Blocked;
        task.blocked_reason = Some(reason);
        task.owner = None;
        task.updated_at = now;
        crate::agent_graph::synchronize(&mut plan.tasks, &mut self.agent_graph);
        bump(plan);
        Ok(plan.revision)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn observe_agent_task(
        &mut self,
        task_id: TaskId,
        agent_id: crate::agent::subagent::AgentId,
        status: crate::agent_graph::AgentAttemptStatus,
        successful_tools: Vec<String>,
        summary: Option<&str>,
        terminal_event_sequence: Option<u64>,
        now: DateTime<Utc>,
    ) -> Result<bool, WorkError> {
        self.observe_agent_task_with_host(
            task_id,
            agent_id,
            status,
            successful_tools,
            summary,
            terminal_event_sequence,
            None,
            crate::agent_graph::AgentGraphBudgetUsageV1::default(),
            now,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn observe_agent_task_with_host(
        &mut self,
        task_id: TaskId,
        agent_id: crate::agent::subagent::AgentId,
        status: crate::agent_graph::AgentAttemptStatus,
        successful_tools: Vec<String>,
        summary: Option<&str>,
        terminal_event_sequence: Option<u64>,
        host_verification: Option<crate::agent_graph::AgentHostVerificationV1>,
        budget_usage: crate::agent_graph::AgentGraphBudgetUsageV1,
        now: DateTime<Utc>,
    ) -> Result<bool, WorkError> {
        let verification_binding = self.agent_graph.verification_binding();
        let plan = self.plan.as_mut().ok_or(WorkError::NoPlan)?;
        let index = plan
            .tasks
            .iter()
            .position(|task| task.id == task_id)
            .ok_or(WorkError::UnknownTask(task_id))?;
        let (definition, became_terminal, receipt, failure_reason) = {
            let task = &mut plan.tasks[index];
            let definition = task
                .agent
                .clone()
                .ok_or(crate::agent_graph::AgentGraphError::NotConfigured)?;
            let attempt = task
                .agent_runtime
                .attempts
                .last_mut()
                .filter(|attempt| attempt.agent_id == Some(agent_id.0))
                .ok_or(crate::agent_graph::AgentGraphError::InvalidTransition)?;
            if attempt.status.terminal() {
                return Ok(false);
            }
            let next_summary = summary.map(crate::agent_graph::bounded_summary);
            let next_terminal_event_sequence = status
                .terminal()
                .then_some(terminal_event_sequence)
                .flatten();
            if attempt.status == status
                && attempt.successful_tools == successful_tools
                && attempt.summary == next_summary
                && attempt.terminal_event_sequence == next_terminal_event_sequence
            {
                return Ok(false);
            }
            attempt.status = status;
            attempt.successful_tools = successful_tools;
            attempt.summary = next_summary;
            attempt.terminal_event_sequence = next_terminal_event_sequence;
            if host_verification.is_some() {
                attempt.host_verification = host_verification;
            }
            attempt.budget_usage = budget_usage;
            let became_terminal = status.terminal();
            if became_terminal {
                attempt.finished_at = Some(now);
            }
            let receipt = became_terminal.then(|| {
                crate::agent_graph::verify_agent_attempt_with_host(
                    verification_binding,
                    task_id,
                    &definition,
                    attempt.number,
                    agent_id.0,
                    status,
                    attempt.terminal_event_sequence,
                    attempt.summary.as_deref(),
                    &attempt.successful_tools,
                    attempt.host_verification.as_ref(),
                )
            });
            attempt.verification_receipt = receipt.clone();
            let disposition = receipt.as_ref().map(|receipt| receipt.disposition);
            let failure_reason = if status == crate::agent_graph::AgentAttemptStatus::Completed
                && disposition != Some(crate::agent_graph::AgentVerificationDisposition::Pass)
            {
                if let Some(receipt) = receipt.as_ref() {
                    attempt.status = crate::agent_graph::AgentAttemptStatus::EvidenceFailed;
                    Some(match receipt.disposition {
                        crate::agent_graph::AgentVerificationDisposition::Inconclusive => {
                            "agent verification was inconclusive because no host-authoritative evidence passed"
                                .to_string()
                        }
                        crate::agent_graph::AgentVerificationDisposition::Fail => {
                            "agent completed without satisfying every evidence gate".to_string()
                        }
                        crate::agent_graph::AgentVerificationDisposition::Pass => {
                            unreachable!("passing verification is handled above")
                        }
                    })
                } else {
                    Some("agent completed without a host verification receipt".to_string())
                }
            } else if became_terminal
                && disposition != Some(crate::agent_graph::AgentVerificationDisposition::Pass)
            {
                Some(
                    attempt
                        .summary
                        .clone()
                        .unwrap_or_else(|| format!("agent ended with {:?}", attempt.status)),
                )
            } else {
                None
            };
            (definition, became_terminal, receipt, failure_reason)
        };
        if !became_terminal {
            bump(plan);
            return Ok(true);
        }
        if receipt.as_ref().is_some_and(|receipt| {
            receipt.disposition == crate::agent_graph::AgentVerificationDisposition::Pass
        }) {
            let receipt = receipt.as_ref().expect("passing receipt exists");
            let event_id = WorkEventId::new();
            self.events.push(RecordedWorkEvent {
                id: event_id,
                kind: WorkEventKind::Review,
                summary: format!("agent task {task_id} passed host verification"),
                succeeded: true,
                created_at: now,
            });
            let task = &mut plan.tasks[index];
            task.evidence.push(EvidenceRef {
                event_id,
                note: format!(
                    "agent attempt {} passed {} host-bound gate(s) ({})",
                    receipt.attempt_number,
                    definition.evidence.len(),
                    &receipt.contract_sha256[..12]
                ),
            });
            task.agent_runtime.state = crate::agent_graph::AgentTaskState::Completed;
            task.agent_runtime.failure = None;
            task.status = TaskStatus::Completed;
            task.blocked_reason = None;
        } else {
            let failure = crate::agent_graph::bounded_summary(
                failure_reason.as_deref().unwrap_or("agent task failed"),
            );
            let task = &mut plan.tasks[index];
            task.agent_runtime.state = crate::agent_graph::AgentTaskState::Failed;
            task.agent_runtime.failure = Some(failure.clone());
            task.status = TaskStatus::Blocked;
            task.blocked_reason = Some(failure);
        }
        plan.tasks[index].updated_at = now;
        crate::agent_graph::synchronize(&mut plan.tasks, &mut self.agent_graph);
        bump(plan);
        Ok(true)
    }

    pub fn pause_agent_graph_after_restore(&mut self, now: DateTime<Utc>) -> bool {
        let verification_binding = self.agent_graph.verification_binding();
        let budget_requires_pause = !self
            .agent_graph
            .budget_limits
            .as_ref()
            .is_some_and(crate::agent_graph::AgentGraphBudgetLimitsV1::valid);
        let Some(plan) = self.plan.as_mut() else {
            return false;
        };
        let mut interrupted = false;
        let mut invalid_verification = false;
        let mut legacy_requires_pause = false;
        for task in plan.tasks.iter_mut().filter(|task| task.agent.is_some()) {
            let definition = task.agent.as_ref().expect("filtered agent task").clone();
            let legacy_contract = definition.verification_contract.is_none();
            legacy_requires_pause |= legacy_contract
                && matches!(
                    task.agent_runtime.state,
                    crate::agent_graph::AgentTaskState::Pending
                        | crate::agent_graph::AgentTaskState::Ready
                        | crate::agent_graph::AgentTaskState::Running
                        | crate::agent_graph::AgentTaskState::DependencyBlocked
                );
            if task.agent_runtime.state == crate::agent_graph::AgentTaskState::Completed
                && !legacy_contract
                && !task.agent_runtime.attempts.last().is_some_and(|attempt| {
                    attempt
                        .verification_receipt
                        .as_ref()
                        .is_some_and(|receipt| {
                            receipt.disposition
                                == crate::agent_graph::AgentVerificationDisposition::Pass
                        })
                        && crate::agent_graph::verification_receipt_valid(
                            verification_binding,
                            task.id,
                            &definition,
                            attempt,
                        )
                })
            {
                let reason = "saved agent completion lacks a valid host verification receipt";
                task.agent_runtime.state = crate::agent_graph::AgentTaskState::Failed;
                task.agent_runtime.failure = Some(reason.into());
                task.status = TaskStatus::Blocked;
                task.blocked_reason = Some(reason.into());
                task.updated_at = now;
                invalid_verification = true;
                continue;
            }
            if task.agent_runtime.state != crate::agent_graph::AgentTaskState::Running {
                continue;
            }
            if let Some(attempt) = task.agent_runtime.attempts.last_mut() {
                if !attempt.status.terminal() {
                    attempt.status = crate::agent_graph::AgentAttemptStatus::Interrupted;
                    attempt.finished_at = Some(now);
                    attempt.summary = Some("agent interrupted by session restore".into());
                    attempt.terminal_event_sequence = None;
                    attempt.verification_receipt = attempt.agent_id.map(|agent_id| {
                        crate::agent_graph::verify_agent_attempt_with_host(
                            verification_binding,
                            task.id,
                            &definition,
                            attempt.number,
                            agent_id,
                            crate::agent_graph::AgentAttemptStatus::Interrupted,
                            None,
                            attempt.summary.as_deref(),
                            &attempt.successful_tools,
                            attempt.host_verification.as_ref(),
                        )
                    });
                }
            }
            task.agent_runtime.state = crate::agent_graph::AgentTaskState::Failed;
            task.agent_runtime.failure = Some("agent interrupted by session restore".into());
            task.status = TaskStatus::Blocked;
            task.blocked_reason = Some("agent interrupted by session restore".into());
            task.updated_at = now;
            interrupted = true;
        }
        if interrupted
            || invalid_verification
            || legacy_requires_pause
            || budget_requires_pause
            || self.agent_graph.status == crate::agent_graph::AgentGraphStatus::Running
        {
            self.agent_graph.status = crate::agent_graph::AgentGraphStatus::Paused;
            self.agent_graph.updated_at = Some(now);
            self.agent_graph.notice = Some(if budget_requires_pause {
                "Legacy graph paused without a host-owned autonomy budget; reconfigure it before execution"
                    .into()
            } else if invalid_verification {
                "Graph paused because saved completion verification is missing or invalid".into()
            } else if legacy_requires_pause {
                "Legacy graph paused without a host verification contract; reconfigure it before execution"
                    .into()
            } else {
                "Graph paused after session restore; retry is explicit".into()
            });
            bump(plan);
            true
        } else {
            false
        }
    }

    pub fn fork_full(&self) -> Self {
        let mut clone = self.clone();
        clone.historical_notice = None;
        clone.pending_automation_proposal = None;
        clone.plan_feedback.make_historical();
        if let Some(plan) = clone.plan.as_mut() {
            plan.source_plan_id = Some(plan.id);
            plan.id = PlanId::new();
            let remapped = plan
                .tasks
                .iter()
                .map(|task| (task.id, TaskId::new()))
                .collect::<std::collections::HashMap<_, _>>();
            for task in &mut plan.tasks {
                task.source_task_id = Some(task.id);
                task.id = remapped[&task.id];
                if let Some(agent) = task.agent.as_mut() {
                    for dependency in &mut agent.depends_on {
                        *dependency = remapped[dependency];
                    }
                }
                task.agent_runtime = crate::agent_graph::AgentTaskRuntime::default();
            }
        }
        clone.agent_graph = crate::agent_graph::AgentGraphState::default();
        clone
    }

    pub fn historical_omission() -> Self {
        Self {
            pending_automation_proposal: None,
            plan: None,
            plan_feedback: crate::plan_feedback::PlanFeedbackLedger::default(),
            events: Vec::new(),
            agent_graph: crate::agent_graph::AgentGraphState::default(),
            historical_notice: Some(
                "Work state was omitted because exact historical state is unavailable".into(),
            ),
        }
    }

    pub fn render_plan(&self) -> String {
        let Some(plan) = &self.plan else {
            return self
                .historical_notice
                .clone()
                .unwrap_or_else(|| "No plan artifact".into());
        };
        let mut lines = vec![
            format!("{} [{:?}]", plan.title, plan.status),
            format!("Plan {} · revision {}", plan.id, plan.revision),
            String::new(),
        ];
        if let Some(body) = plan.body.as_ref() {
            lines.push(body.clone());
            lines.push(String::new());
        }
        for task in &plan.tasks {
            lines.push(format!("[{:?}] {}  ({})", task.status, task.title, task.id));
            if let Some(reason) = &task.blocked_reason {
                lines.push(format!("  Blocked: {reason}"));
            }
            if let Some(agent) = &task.agent {
                let dependencies = if agent.depends_on.is_empty() {
                    "none".into()
                } else {
                    agent
                        .depends_on
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(", ")
                };
                lines.push(format!(
                    "  Agent: {:?} · dependencies: {} · attempts: {}/{} · worktree: {}",
                    task.agent_runtime.state,
                    dependencies,
                    task.agent_runtime.attempts.len(),
                    agent.max_attempts,
                    agent
                        .worktree_id
                        .map(|id| id.to_string())
                        .unwrap_or_else(|| "current workspace".into())
                ));
                lines.push(format!(
                    "  Grants: {} tool(s), {} path(s), {} command(s), network {}",
                    agent.grant.tools.len(),
                    agent.grant.paths.len(),
                    agent.grant.commands.len(),
                    if agent.grant.network {
                        "requested"
                    } else {
                        "denied"
                    }
                ));
                if let Some(failure) = &task.agent_runtime.failure {
                    lines.push(format!("  Agent failure: {failure}"));
                }
            }
            for evidence in &task.evidence {
                lines.push(format!(
                    "  Evidence {}: {}",
                    evidence.event_id, evidence.note
                ));
            }
        }
        if !plan.decisions.is_empty() {
            lines.push(String::new());
            lines.push("Decisions".into());
            lines.extend(
                plan.decisions
                    .iter()
                    .map(|decision| format!("- {decision}")),
            );
        }
        lines.join("\n")
    }
}

fn agent_graph_verification_ready(
    plan: &PlanArtifact,
    graph: &crate::agent_graph::AgentGraphState,
) -> bool {
    let Some(binding) = graph.verification_binding() else {
        return false;
    };
    if binding.plan_id != plan.id {
        return false;
    }
    if !graph
        .budget_limits
        .as_ref()
        .is_some_and(crate::agent_graph::AgentGraphBudgetLimitsV1::valid)
        || graph.budget_usage.blocked.is_some()
    {
        return false;
    }
    plan.tasks.iter().all(|task| {
        let Some(definition) = task.agent.as_ref() else {
            return false;
        };
        let contract_valid = definition
            .verification_contract
            .as_ref()
            .is_some_and(|contract| {
                contract.matches(
                    binding.plan_id,
                    binding.configuration_revision,
                    &definition.evidence,
                )
            });
        contract_valid
            && (task.agent_runtime.state != crate::agent_graph::AgentTaskState::Completed
                || crate::agent_graph::task_completion_verified(Some(binding), task))
    })
}

fn bump(plan: &mut PlanArtifact) {
    plan.revision += 1;
    plan.updated_at = Utc::now();
}

pub fn execute_model_tool(
    name: &str,
    arguments: &serde_json::Value,
    state: &Arc<Mutex<WorkState>>,
) -> Option<Result<String, WorkError>> {
    if name == "propose_automation" {
        let invalid = || WorkError::InvalidArguments(name.into());
        return Some((|| {
            if arguments.as_object().is_none_or(|object| object.len() != 1)
                || serde_json::to_vec(arguments).map_err(|_| invalid())?.len() > 128 * 1024
            {
                return Err(invalid());
            }
            let definition =
                serde_json::from_value(arguments.get("definition").cloned().ok_or_else(invalid)?)
                    .map_err(|_| invalid())?;
            let mut work = state.lock().unwrap_or_else(|poison| poison.into_inner());
            if work.pending_automation_proposal.is_some() {
                return Err(invalid());
            }
            work.pending_automation_proposal = Some(definition);
            Ok("Automation draft queued for host validation and preview. No automation was created or enabled. The user must review and type the exact confirmation token.".into())
        })());
    }
    if !matches!(
        name,
        "propose_plan"
            | "edit_plan"
            | "update_plan_task"
            | "record_plan_evidence"
            | "configure_agent_graph"
    ) {
        return None;
    }
    let invalid = || WorkError::InvalidArguments(name.to_string());
    let mutation = (|| -> Result<WorkMutation, WorkError> {
        let expected_revision = arguments
            .get("expected_revision")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(invalid)?;
        Ok(match name {
            "propose_plan" => WorkMutation::Propose {
                expected_revision,
                body: arguments
                    .get("body")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string),
                title: arguments
                    .get("title")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(invalid)?
                    .to_string(),
                tasks: arguments
                    .get("tasks")
                    .and_then(serde_json::Value::as_array)
                    .ok_or_else(invalid)?
                    .iter()
                    .filter_map(|value| value.as_str().map(str::to_string))
                    .collect(),
            },
            "edit_plan" => WorkMutation::Edit {
                expected_revision,
                body: arguments
                    .get("body")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string),
                title: arguments
                    .get("title")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string),
                decisions: arguments
                    .get("decisions")
                    .and_then(serde_json::Value::as_array)
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(|value| value.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default(),
            },
            "update_plan_task" => WorkMutation::UpdateTask {
                expected_revision,
                task_id: TaskId(
                    Uuid::parse_str(
                        arguments
                            .get("task_id")
                            .and_then(serde_json::Value::as_str)
                            .ok_or_else(invalid)?,
                    )
                    .map_err(|_| invalid())?,
                ),
                status: serde_json::from_value(
                    arguments.get("status").cloned().ok_or_else(invalid)?,
                )
                .map_err(|_| invalid())?,
                blocked_reason: arguments
                    .get("blocked_reason")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string),
            },
            "record_plan_evidence" => WorkMutation::RecordEvidence {
                expected_revision,
                task_id: TaskId(
                    Uuid::parse_str(
                        arguments
                            .get("task_id")
                            .and_then(serde_json::Value::as_str)
                            .ok_or_else(invalid)?,
                    )
                    .map_err(|_| invalid())?,
                ),
                event_id: WorkEventId(
                    Uuid::parse_str(
                        arguments
                            .get("event_id")
                            .and_then(serde_json::Value::as_str)
                            .ok_or_else(invalid)?,
                    )
                    .map_err(|_| invalid())?,
                ),
                note: arguments
                    .get("note")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(invalid)?
                    .to_string(),
            },
            "configure_agent_graph" => {
                let parsed: crate::agent_graph::ConfigureAgentGraphArgs =
                    serde_json::from_value(arguments.clone()).map_err(|_| invalid())?;
                WorkMutation::ConfigureAgentGraph {
                    expected_revision: parsed.expected_revision,
                    budget: parsed.budget,
                    tasks: parsed.tasks,
                }
            }
            _ => unreachable!(),
        })
    })();
    let mutation = match mutation {
        Ok(mutation) => mutation,
        Err(error) => return Some(Err(error)),
    };
    let mut state = state.lock().unwrap_or_else(|poison| poison.into_inner());
    Some(state.apply_model_mutation(mutation).map(|revision| {
        if matches!(name, "propose_plan" | "configure_agent_graph") {
            format!(
                "Plan artifact updated to revision {revision}. Stable task IDs and graph state:\n{}",
                state.render_plan()
            )
        } else {
            format!("Plan artifact updated to revision {revision}")
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unpriced_budget() -> crate::agent_graph::AgentGraphBudgetInput {
        crate::agent_graph::AgentGraphBudgetInput {
            max_cost_usd: None,
            allow_unpriced: true,
            max_physical_requests: crate::agent_graph::DEFAULT_GRAPH_PHYSICAL_REQUESTS,
        }
    }

    fn proposed() -> WorkState {
        let mut work = WorkState::default();
        work.apply_model_mutation(WorkMutation::Propose {
            body: None,
            expected_revision: 0,
            title: "Ship".into(),
            tasks: vec!["Test".into()],
        })
        .unwrap();
        work
    }

    fn configure_two_task_graph_with_evidence(
        work: &mut WorkState,
        evidence: Vec<crate::agent_graph::AgentEvidenceInput>,
    ) -> (TaskId, TaskId) {
        let tasks = work.plan.as_ref().unwrap().tasks.clone();
        let first = tasks[0].id;
        let second = tasks[1].id;
        let input = |task_id: TaskId, depends_on: Vec<TaskId>| crate::agent_graph::AgentTaskInput {
            task_id: task_id.to_string(),
            depends_on: depends_on.into_iter().map(|id| id.to_string()).collect(),
            tools: vec!["read_file".into()],
            paths: vec!["src".into()],
            commands: Vec::new(),
            network: false,
            worktree_id: None,
            accept_cross_lane_handoff: false,
            max_attempts: 2,
            evidence: evidence.clone(),
        };
        work.apply_model_mutation(WorkMutation::ConfigureAgentGraph {
            expected_revision: work.revision(),
            budget: unpriced_budget(),
            tasks: vec![input(first, Vec::new()), input(second, vec![first])],
        })
        .unwrap();
        (first, second)
    }

    fn configure_two_task_graph(work: &mut WorkState) -> (TaskId, TaskId) {
        configure_two_task_graph_with_evidence(
            work,
            vec![crate::agent_graph::AgentEvidenceInput::AssistantOutput],
        )
    }

    fn stable_host_verification() -> crate::agent_graph::AgentHostVerificationV1 {
        let snapshot = crate::agent_graph::AgentWorkspaceSnapshotV1 {
            workspace_identity_sha256: "1".repeat(64),
            source_revision_sha256: "2".repeat(64),
            artifact_observations: Vec::new(),
        };
        crate::agent_graph::AgentHostVerificationV1 {
            started: snapshot.clone(),
            finished: snapshot,
            observations: Vec::new(),
        }
    }

    #[test]
    fn revision_cas_rejects_stale_mutation_without_partial_change() {
        let mut work = proposed();
        let original = work.clone();
        let result = work.apply_model_mutation(WorkMutation::Edit {
            body: None,
            expected_revision: 0,
            title: Some("stale".into()),
            decisions: Vec::new(),
        });
        assert_eq!(
            result,
            Err(WorkError::StaleRevision {
                expected: 0,
                current: 1
            })
        );
        assert_eq!(work, original);
    }

    #[test]
    fn evidence_must_reference_a_recorded_event_before_completion() {
        let mut work = proposed();
        let task_id = work.plan.as_ref().unwrap().tasks[0].id;
        assert_eq!(
            work.apply_model_mutation(WorkMutation::UpdateTask {
                expected_revision: 1,
                task_id,
                status: TaskStatus::Completed,
                blocked_reason: None,
            }),
            Err(WorkError::CompletionWithoutEvidence)
        );
        let event_id = work.record_event(WorkEventKind::Test, "cargo test", true);
        work.apply_model_mutation(WorkMutation::RecordEvidence {
            expected_revision: 1,
            task_id,
            event_id,
            note: "passed".into(),
        })
        .unwrap();
        work.apply_model_mutation(WorkMutation::UpdateTask {
            expected_revision: 2,
            task_id,
            status: TaskStatus::Completed,
            blocked_reason: None,
        })
        .unwrap();
        assert_eq!(work.revision(), 3);
    }

    #[test]
    fn user_decision_and_forks_preserve_authority_and_lineage() {
        let mut work = proposed();
        work.user_decide(1, true).unwrap();
        assert_eq!(
            work.plan.as_ref().unwrap().approved_by.as_deref(),
            Some("user")
        );
        let fork = work.fork_full();
        assert_ne!(
            fork.plan.as_ref().unwrap().id,
            work.plan.as_ref().unwrap().id
        );
        assert_ne!(
            fork.plan.as_ref().unwrap().tasks[0].id,
            work.plan.as_ref().unwrap().tasks[0].id
        );
        assert!(WorkState::historical_omission().plan.is_none());
    }

    #[test]
    fn pre_graph_work_state_deserializes_with_inert_defaults() {
        let json = serde_json::json!({
            "plan": null,
            "events": [],
            "historical_notice": null
        });
        let work: WorkState = serde_json::from_value(json).unwrap();
        assert_eq!(
            work.agent_graph.status,
            crate::agent_graph::AgentGraphStatus::Idle
        );
        assert!(work.agent_graph.updated_at.is_none());
    }

    #[test]
    fn agent_graph_releases_dependency_after_host_tool_observation() {
        let mut work = WorkState::default();
        work.apply_model_mutation(WorkMutation::Propose {
            body: None,
            expected_revision: 0,
            title: "Graph".into(),
            tasks: vec!["First".into(), "Second".into()],
        })
        .unwrap();
        let (first, second) = configure_two_task_graph_with_evidence(
            &mut work,
            vec![
                crate::agent_graph::AgentEvidenceInput::AssistantOutput,
                crate::agent_graph::AgentEvidenceInput::ToolSuccess {
                    tool: "read_file".into(),
                },
            ],
        );
        assert!(matches!(
            work.user_start_agent_graph(),
            Err(WorkError::AgentGraph(
                crate::agent_graph::AgentGraphError::PlanNotApproved
            ))
        ));
        work.user_decide(work.revision(), true).unwrap();
        work.user_start_agent_graph().unwrap();
        let plan = work.plan.as_ref().unwrap();
        assert_eq!(
            plan.tasks[0].agent_runtime.state,
            crate::agent_graph::AgentTaskState::Ready
        );
        assert_eq!(
            plan.tasks[1].agent_runtime.state,
            crate::agent_graph::AgentTaskState::Pending
        );
        let agent = crate::agent::subagent::AgentId(Uuid::from_u128(90));
        let host = stable_host_verification();
        work.mark_agent_task_spawned_with_snapshot(
            first,
            agent,
            Some(host.started.clone()),
            Utc::now(),
        )
        .unwrap();
        work.observe_agent_task_with_host(
            first,
            agent,
            crate::agent_graph::AgentAttemptStatus::Completed,
            vec!["read_file".into()],
            Some("proved"),
            Some(17),
            Some(host),
            crate::agent_graph::AgentGraphBudgetUsageV1::default(),
            Utc::now(),
        )
        .unwrap();
        let plan = work.plan.as_ref().unwrap();
        assert_eq!(plan.tasks[0].status, TaskStatus::Completed);
        let receipt = plan.tasks[0].agent_runtime.attempts[0]
            .verification_receipt
            .as_ref()
            .unwrap();
        assert_eq!(
            receipt.disposition,
            crate::agent_graph::AgentVerificationDisposition::Pass
        );
        assert_eq!(receipt.terminal_event_sequence, Some(17));
        assert_eq!(
            plan.tasks[1].agent_runtime.state,
            crate::agent_graph::AgentTaskState::Ready
        );
        assert_eq!(plan.tasks[1].id, second);
    }

    #[test]
    fn explicit_human_review_creates_a_host_receipt_before_releasing_dependency() {
        let mut work = WorkState::default();
        work.apply_model_mutation(WorkMutation::Propose {
            body: None,
            expected_revision: 0,
            title: "Human review graph".into(),
            tasks: vec!["Review me".into(), "Dependent".into()],
        })
        .unwrap();
        let (first, second) = configure_two_task_graph_with_evidence(
            &mut work,
            vec![crate::agent_graph::AgentEvidenceInput::HumanReview],
        );
        work.user_decide(work.revision(), true).unwrap();
        work.user_start_agent_graph().unwrap();
        let agent = crate::agent::subagent::AgentId(Uuid::from_u128(91));
        let host = stable_host_verification();
        work.mark_agent_task_spawned_with_snapshot(
            first,
            agent,
            Some(host.started.clone()),
            Utc::now(),
        )
        .unwrap();
        work.observe_agent_task_with_host(
            first,
            agent,
            crate::agent_graph::AgentAttemptStatus::Completed,
            Vec::new(),
            Some("ready for review"),
            Some(18),
            Some(host),
            crate::agent_graph::AgentGraphBudgetUsageV1::default(),
            Utc::now(),
        )
        .unwrap();
        assert_eq!(
            work.plan.as_ref().unwrap().tasks[0].agent_runtime.state,
            crate::agent_graph::AgentTaskState::Failed
        );
        assert_ne!(
            work.plan.as_ref().unwrap().tasks[1].agent_runtime.state,
            crate::agent_graph::AgentTaskState::Ready
        );

        work.user_review_agent_task(first).unwrap();

        let plan = work.plan.as_ref().unwrap();
        assert_eq!(plan.tasks[0].status, TaskStatus::Completed);
        assert_eq!(
            plan.tasks[0].agent_runtime.attempts[0]
                .verification_receipt
                .as_ref()
                .unwrap()
                .disposition,
            crate::agent_graph::AgentVerificationDisposition::Pass
        );
        assert_eq!(plan.tasks[1].id, second);
        assert_eq!(
            plan.tasks[1].agent_runtime.state,
            crate::agent_graph::AgentTaskState::Ready
        );
    }

    #[test]
    fn assistant_output_only_completion_is_inconclusive_and_does_not_release_dependency() {
        let mut work = WorkState::default();
        work.apply_model_mutation(WorkMutation::Propose {
            body: None,
            expected_revision: 0,
            title: "Graph".into(),
            tasks: vec!["First".into(), "Second".into()],
        })
        .unwrap();
        let (first, _) = configure_two_task_graph(&mut work);
        work.user_decide(work.revision(), true).unwrap();
        work.user_start_agent_graph().unwrap();
        let agent = crate::agent::subagent::AgentId(Uuid::from_u128(93));
        work.mark_agent_task_spawned(first, agent, Utc::now())
            .unwrap();

        work.observe_agent_task(
            first,
            agent,
            crate::agent_graph::AgentAttemptStatus::Completed,
            Vec::new(),
            Some("proved"),
            None,
            Utc::now(),
        )
        .unwrap();

        let plan = work.plan.as_ref().unwrap();
        assert_ne!(plan.tasks[0].status, TaskStatus::Completed);
        assert_eq!(
            plan.tasks[0].agent_runtime.attempts[0]
                .verification_receipt
                .as_ref()
                .unwrap()
                .disposition,
            crate::agent_graph::AgentVerificationDisposition::Inconclusive
        );
        assert_eq!(
            plan.tasks[1].agent_runtime.state,
            crate::agent_graph::AgentTaskState::DependencyBlocked
        );
        assert_eq!(
            work.agent_graph.status,
            crate::agent_graph::AgentGraphStatus::Paused
        );
        assert!(plan.tasks[0].evidence.is_empty());
    }

    #[test]
    fn failed_agent_blocks_descendants_until_explicit_retry() {
        let mut work = WorkState::default();
        work.apply_model_mutation(WorkMutation::Propose {
            body: None,
            expected_revision: 0,
            title: "Graph".into(),
            tasks: vec!["First".into(), "Second".into()],
        })
        .unwrap();
        let (first, _) = configure_two_task_graph(&mut work);
        work.user_decide(work.revision(), true).unwrap();
        work.user_start_agent_graph().unwrap();
        let agent = crate::agent::subagent::AgentId(Uuid::from_u128(91));
        work.mark_agent_task_spawned(first, agent, Utc::now())
            .unwrap();
        work.observe_agent_task(
            first,
            agent,
            crate::agent_graph::AgentAttemptStatus::Failed,
            Vec::new(),
            Some("safe failure"),
            None,
            Utc::now(),
        )
        .unwrap();
        let plan = work.plan.as_ref().unwrap();
        assert_eq!(
            plan.tasks[1].agent_runtime.state,
            crate::agent_graph::AgentTaskState::DependencyBlocked
        );
        assert_eq!(
            work.agent_graph.status,
            crate::agent_graph::AgentGraphStatus::Paused
        );
        work.user_retry_agent_task(first).unwrap();
        assert_eq!(
            work.plan.as_ref().unwrap().tasks[0].agent_runtime.state,
            crate::agent_graph::AgentTaskState::Ready
        );
    }

    #[test]
    fn graph_fork_remaps_edges_and_restore_interrupts_attempts() {
        let mut work = WorkState::default();
        work.apply_model_mutation(WorkMutation::Propose {
            body: None,
            expected_revision: 0,
            title: "Graph".into(),
            tasks: vec!["First".into(), "Second".into()],
        })
        .unwrap();
        let (first, _) = configure_two_task_graph(&mut work);
        work.user_decide(work.revision(), true).unwrap();
        work.user_start_agent_graph().unwrap();
        let agent = crate::agent::subagent::AgentId(Uuid::from_u128(92));
        work.mark_agent_task_spawned(first, agent, Utc::now())
            .unwrap();
        let fork = work.fork_full();
        let fork_plan = fork.plan.as_ref().unwrap();
        assert!(fork_plan
            .tasks
            .iter()
            .all(|task| task.agent_runtime.attempts.is_empty()));
        assert_eq!(
            fork_plan.tasks[1].agent.as_ref().unwrap().depends_on,
            [fork_plan.tasks[0].id]
        );
        assert!(work.pause_agent_graph_after_restore(Utc::now()));
        assert_eq!(
            work.plan.as_ref().unwrap().tasks[0].agent_runtime.attempts[0].status,
            crate::agent_graph::AgentAttemptStatus::Interrupted
        );
    }

    #[test]
    fn restored_graph_rejects_tampered_verification_receipt() {
        let mut work = proposed();
        let task_id = work.plan.as_ref().unwrap().tasks[0].id;
        work.apply_model_mutation(WorkMutation::ConfigureAgentGraph {
            expected_revision: work.revision(),
            budget: unpriced_budget(),
            tasks: vec![crate::agent_graph::AgentTaskInput {
                task_id: task_id.to_string(),
                depends_on: Vec::new(),
                tools: vec!["read_file".into()],
                paths: vec!["src".into()],
                commands: Vec::new(),
                network: false,
                worktree_id: None,
                accept_cross_lane_handoff: false,
                max_attempts: 1,
                evidence: vec![crate::agent_graph::AgentEvidenceInput::ToolSuccess {
                    tool: "read_file".into(),
                }],
            }],
        })
        .unwrap();
        work.user_decide(work.revision(), true).unwrap();
        work.user_start_agent_graph().unwrap();
        let agent = crate::agent::subagent::AgentId(Uuid::from_u128(94));
        let host = stable_host_verification();
        work.mark_agent_task_spawned_with_snapshot(
            task_id,
            agent,
            Some(host.started.clone()),
            Utc::now(),
        )
        .unwrap();
        work.observe_agent_task_with_host(
            task_id,
            agent,
            crate::agent_graph::AgentAttemptStatus::Completed,
            vec!["read_file".into()],
            Some("done"),
            Some(21),
            Some(host),
            crate::agent_graph::AgentGraphBudgetUsageV1::default(),
            Utc::now(),
        )
        .unwrap();
        assert_eq!(
            work.agent_graph.status,
            crate::agent_graph::AgentGraphStatus::Completed
        );

        let mut restored: WorkState =
            serde_json::from_value(serde_json::to_value(&work).unwrap()).unwrap();
        assert!(!restored.pause_agent_graph_after_restore(Utc::now()));
        let mut historical_legacy = restored.clone();
        historical_legacy.plan.as_mut().unwrap().tasks[0]
            .agent
            .as_mut()
            .unwrap()
            .verification_contract = None;
        historical_legacy.agent_graph.plan_id = None;
        historical_legacy.agent_graph.configuration_revision = None;
        historical_legacy.agent_graph.approved_revision = None;
        assert!(!historical_legacy.pause_agent_graph_after_restore(Utc::now()));
        assert_eq!(
            historical_legacy.plan.as_ref().unwrap().tasks[0].status,
            TaskStatus::Completed
        );
        let attempt = &mut restored.plan.as_mut().unwrap().tasks[0]
            .agent_runtime
            .attempts[0];
        let receipt = attempt.verification_receipt.as_mut().unwrap();
        receipt.checks[0].satisfied = false;
        let tampered_sha256 = crate::agent_graph::verification_receipt_sha256(receipt);
        receipt.receipt_sha256 = tampered_sha256;

        assert!(restored.pause_agent_graph_after_restore(Utc::now()));
        assert_eq!(
            restored.agent_graph.status,
            crate::agent_graph::AgentGraphStatus::Paused
        );
        assert_eq!(
            restored.plan.as_ref().unwrap().tasks[0].status,
            TaskStatus::Blocked
        );
    }

    #[test]
    fn restored_legacy_graph_cannot_restart_without_reconfiguration() {
        let mut work = proposed();
        let task_id = work.plan.as_ref().unwrap().tasks[0].id;
        work.apply_model_mutation(WorkMutation::ConfigureAgentGraph {
            expected_revision: work.revision(),
            budget: unpriced_budget(),
            tasks: vec![crate::agent_graph::AgentTaskInput {
                task_id: task_id.to_string(),
                depends_on: Vec::new(),
                tools: vec!["read_file".into()],
                paths: vec!["src".into()],
                commands: Vec::new(),
                network: false,
                worktree_id: None,
                accept_cross_lane_handoff: false,
                max_attempts: 1,
                evidence: vec![crate::agent_graph::AgentEvidenceInput::ToolSuccess {
                    tool: "read_file".into(),
                }],
            }],
        })
        .unwrap();
        work.user_decide(work.revision(), true).unwrap();
        work.user_start_agent_graph().unwrap();
        work.user_pause_agent_graph().unwrap();
        work.plan.as_mut().unwrap().tasks[0]
            .agent
            .as_mut()
            .unwrap()
            .verification_contract = None;

        assert_eq!(
            work.user_start_agent_graph(),
            Err(WorkError::AgentGraph(
                crate::agent_graph::AgentGraphError::VerificationContractRequired
            ))
        );
    }

    #[test]
    fn legacy_completion_cannot_release_a_current_dependency() {
        let mut work = WorkState::default();
        work.apply_model_mutation(WorkMutation::Propose {
            body: None,
            expected_revision: 0,
            title: "Graph".into(),
            tasks: vec!["First".into(), "Second".into()],
        })
        .unwrap();
        let (first, _) = configure_two_task_graph_with_evidence(
            &mut work,
            vec![crate::agent_graph::AgentEvidenceInput::ToolSuccess {
                tool: "read_file".into(),
            }],
        );
        work.user_decide(work.revision(), true).unwrap();
        work.user_start_agent_graph().unwrap();
        let agent = crate::agent::subagent::AgentId(Uuid::from_u128(98));
        let host = stable_host_verification();
        work.mark_agent_task_spawned_with_snapshot(
            first,
            agent,
            Some(host.started.clone()),
            Utc::now(),
        )
        .unwrap();
        work.observe_agent_task_with_host(
            first,
            agent,
            crate::agent_graph::AgentAttemptStatus::Completed,
            vec!["read_file".into()],
            Some("done"),
            Some(23),
            Some(host),
            crate::agent_graph::AgentGraphBudgetUsageV1::default(),
            Utc::now(),
        )
        .unwrap();

        let plan = work.plan.as_mut().unwrap();
        plan.tasks[0].agent.as_mut().unwrap().verification_contract = None;
        plan.tasks[1].agent_runtime.state = crate::agent_graph::AgentTaskState::Pending;
        work.agent_graph.status = crate::agent_graph::AgentGraphStatus::Running;
        crate::agent_graph::synchronize(&mut plan.tasks, &mut work.agent_graph);

        assert_eq!(
            work.plan.as_ref().unwrap().tasks[1].agent_runtime.state,
            crate::agent_graph::AgentTaskState::Pending
        );
        assert_eq!(
            work.agent_graph.status,
            crate::agent_graph::AgentGraphStatus::Paused
        );
        assert_eq!(
            work.user_start_agent_graph(),
            Err(WorkError::AgentGraph(
                crate::agent_graph::AgentGraphError::VerificationContractRequired
            ))
        );
    }

    #[test]
    fn valid_non_pass_receipt_cannot_release_a_dependency() {
        let mut work = WorkState::default();
        work.apply_model_mutation(WorkMutation::Propose {
            body: None,
            expected_revision: 0,
            title: "Graph".into(),
            tasks: vec!["First".into(), "Second".into()],
        })
        .unwrap();
        let (first, _) = configure_two_task_graph(&mut work);
        work.user_decide(work.revision(), true).unwrap();
        work.user_start_agent_graph().unwrap();
        let agent = crate::agent::subagent::AgentId(Uuid::from_u128(99));
        work.mark_agent_task_spawned(first, agent, Utc::now())
            .unwrap();
        work.observe_agent_task(
            first,
            agent,
            crate::agent_graph::AgentAttemptStatus::Completed,
            Vec::new(),
            Some("assistant claim only"),
            Some(24),
            Utc::now(),
        )
        .unwrap();

        let binding = work.agent_graph.verification_binding();
        let plan = work.plan.as_mut().unwrap();
        let first_task = &mut plan.tasks[0];
        assert!(crate::agent_graph::verification_receipt_valid(
            binding,
            first_task.id,
            first_task.agent.as_ref().unwrap(),
            first_task.agent_runtime.attempts.last().unwrap(),
        ));
        first_task.status = TaskStatus::Completed;
        first_task.agent_runtime.state = crate::agent_graph::AgentTaskState::Completed;
        first_task.agent_runtime.failure = None;
        plan.tasks[1].agent_runtime.state = crate::agent_graph::AgentTaskState::Pending;
        work.agent_graph.status = crate::agent_graph::AgentGraphStatus::Running;
        crate::agent_graph::synchronize(&mut plan.tasks, &mut work.agent_graph);

        assert_eq!(
            work.plan.as_ref().unwrap().tasks[1].agent_runtime.state,
            crate::agent_graph::AgentTaskState::Pending
        );
        assert_eq!(
            work.agent_graph.status,
            crate::agent_graph::AgentGraphStatus::Paused
        );
        assert_eq!(
            work.user_start_agent_graph(),
            Err(WorkError::AgentGraph(
                crate::agent_graph::AgentGraphError::VerificationContractRequired
            ))
        );
    }

    #[test]
    fn plan_mutation_invalidates_an_existing_pass_receipt() {
        let mut work = proposed();
        let task_id = work.plan.as_ref().unwrap().tasks[0].id;
        work.apply_model_mutation(WorkMutation::ConfigureAgentGraph {
            expected_revision: work.revision(),
            budget: unpriced_budget(),
            tasks: vec![crate::agent_graph::AgentTaskInput {
                task_id: task_id.to_string(),
                depends_on: Vec::new(),
                tools: vec!["read_file".into()],
                paths: vec!["src".into()],
                commands: Vec::new(),
                network: false,
                worktree_id: None,
                accept_cross_lane_handoff: false,
                max_attempts: 1,
                evidence: vec![crate::agent_graph::AgentEvidenceInput::ToolSuccess {
                    tool: "read_file".into(),
                }],
            }],
        })
        .unwrap();
        work.user_decide(work.revision(), true).unwrap();
        work.user_start_agent_graph().unwrap();
        let agent = crate::agent::subagent::AgentId(Uuid::from_u128(95));
        let host = stable_host_verification();
        work.mark_agent_task_spawned_with_snapshot(
            task_id,
            agent,
            Some(host.started.clone()),
            Utc::now(),
        )
        .unwrap();
        work.observe_agent_task_with_host(
            task_id,
            agent,
            crate::agent_graph::AgentAttemptStatus::Completed,
            vec!["read_file".into()],
            Some("done"),
            Some(22),
            Some(host),
            crate::agent_graph::AgentGraphBudgetUsageV1::default(),
            Utc::now(),
        )
        .unwrap();
        assert_eq!(
            work.plan.as_ref().unwrap().tasks[0].status,
            TaskStatus::Completed
        );

        work.apply_model_mutation(WorkMutation::Edit {
            body: None,
            expected_revision: work.revision(),
            title: Some("Changed after verification".into()),
            decisions: Vec::new(),
        })
        .unwrap();

        assert!(work.agent_graph.approved_revision.is_none());
        work.user_decide(work.revision(), true).unwrap();
        assert!(work.agent_graph.approved_revision.is_some());
        assert!(work.pause_agent_graph_after_restore(Utc::now()));
        assert_eq!(
            work.plan.as_ref().unwrap().tasks[0].status,
            TaskStatus::Blocked
        );
    }

    #[test]
    fn approval_binding_cannot_change_while_an_agent_is_running() {
        let mut work = proposed();
        let task_id = work.plan.as_ref().unwrap().tasks[0].id;
        work.apply_model_mutation(WorkMutation::ConfigureAgentGraph {
            expected_revision: work.revision(),
            budget: unpriced_budget(),
            tasks: vec![crate::agent_graph::AgentTaskInput {
                task_id: task_id.to_string(),
                depends_on: Vec::new(),
                tools: vec!["read_file".into()],
                paths: vec!["src".into()],
                commands: Vec::new(),
                network: false,
                worktree_id: None,
                accept_cross_lane_handoff: false,
                max_attempts: 1,
                evidence: vec![crate::agent_graph::AgentEvidenceInput::ToolSuccess {
                    tool: "read_file".into(),
                }],
            }],
        })
        .unwrap();
        work.user_decide(work.revision(), true).unwrap();
        work.user_start_agent_graph().unwrap();
        let agent = crate::agent::subagent::AgentId(Uuid::from_u128(97));
        work.mark_agent_task_spawned(task_id, agent, Utc::now())
            .unwrap();
        let before = work.clone();

        assert_eq!(
            work.user_decide(work.revision(), false),
            Err(WorkError::AgentGraph(
                crate::agent_graph::AgentGraphError::InvalidTransition
            ))
        );
        assert_eq!(work, before);
    }

    #[test]
    fn host_tool_observation_without_terminal_event_is_inconclusive() {
        let mut work = proposed();
        let task_id = work.plan.as_ref().unwrap().tasks[0].id;
        work.apply_model_mutation(WorkMutation::ConfigureAgentGraph {
            expected_revision: work.revision(),
            budget: unpriced_budget(),
            tasks: vec![crate::agent_graph::AgentTaskInput {
                task_id: task_id.to_string(),
                depends_on: Vec::new(),
                tools: vec!["read_file".into()],
                paths: vec!["src".into()],
                commands: Vec::new(),
                network: false,
                worktree_id: None,
                accept_cross_lane_handoff: false,
                max_attempts: 1,
                evidence: vec![crate::agent_graph::AgentEvidenceInput::ToolSuccess {
                    tool: "read_file".into(),
                }],
            }],
        })
        .unwrap();
        work.user_decide(work.revision(), true).unwrap();
        work.user_start_agent_graph().unwrap();
        let agent = crate::agent::subagent::AgentId(Uuid::from_u128(96));
        work.mark_agent_task_spawned(task_id, agent, Utc::now())
            .unwrap();

        work.observe_agent_task(
            task_id,
            agent,
            crate::agent_graph::AgentAttemptStatus::Completed,
            vec!["read_file".into()],
            Some("done"),
            None,
            Utc::now(),
        )
        .unwrap();

        let task = &work.plan.as_ref().unwrap().tasks[0];
        assert_ne!(task.status, TaskStatus::Completed);
        assert_eq!(
            task.agent_runtime.attempts[0]
                .verification_receipt
                .as_ref()
                .unwrap()
                .disposition,
            crate::agent_graph::AgentVerificationDisposition::Inconclusive
        );
    }
}
