//! Provider-neutral, sequential plan exchanges. Structured state owns the work;
//! Markdown files are reproducible projections, never execution authority.
use crate::agent::subagent::AgentRoute;
use crate::work::{PlanArtifact, PlanId, WorkMutation, WorkState};
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub const DEFAULT_REVIEW_PASSES: u8 = 3;
pub const MAX_REVIEW_PASSES: u8 = 10;
pub const MAX_WORKFLOW_STAGES: usize = 64;
const MAX_PARTICIPANTS: usize = 16;
const MAX_PLAN_BYTES: usize = 128 * 1024;
const MAX_WORKFLOW_BYTES: usize = 2 * 1024 * 1024;
const MAX_INSTRUCTIONS_BYTES: usize = 16 * 1024;

// AgentRoute deliberately has no schema dependency in the agent runtime.
#[derive(JsonSchema)]
#[allow(dead_code)]
struct RouteSchema {
    provider: String,
    model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WorkflowParticipantInput {
    pub id: String,
    #[serde(default)]
    #[schemars(with = "Option<RouteSchema>")]
    pub route: Option<AgentRoute>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowRole {
    Author,
    Review,
    Revise,
    Implement,
}

impl WorkflowRole {
    pub fn label(self) -> &'static str {
        match self {
            Self::Author => "Author",
            Self::Review => "Review",
            Self::Revise => "Revise",
            Self::Implement => "Implement",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WorkflowStep {
    pub role: WorkflowRole,
    pub participant: String,
    #[serde(default)]
    #[schemars(with = "Option<RouteSchema>")]
    pub route: Option<AgentRoute>,
    #[serde(default)]
    pub instructions: String,
}

fn default_review_passes() -> u8 {
    DEFAULT_REVIEW_PASSES
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WorkflowReviewLoop {
    pub reviewer: String,
    pub reviser: String,
    #[serde(default = "default_review_passes")]
    pub max_review_passes: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum PlanWorkflowArgs {
    Start {
        goal: String,
        #[serde(default)]
        participants: Vec<WorkflowParticipantInput>,
        steps: Vec<WorkflowStep>,
        #[serde(default)]
        review_loop: Option<WorkflowReviewLoop>,
        #[serde(default)]
        implementation: Option<WorkflowStep>,
    },
    Handoff {
        step: WorkflowStep,
    },
    Status,
    Pause,
    Resume,
    Cancel,
}

#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum WorkflowToolAction {
    Start,
    Handoff,
    Status,
    Pause,
    Resume,
    Cancel,
}

/// Provider tools require an object root. Internal tagged-enum deserialization
/// still enforces the fields required by each selected action.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct WorkflowToolSchema {
    action: WorkflowToolAction,
    goal: Option<String>,
    participants: Option<Vec<WorkflowParticipantInput>>,
    steps: Option<Vec<WorkflowStep>>,
    review_loop: Option<WorkflowReviewLoop>,
    implementation: Option<WorkflowStep>,
    step: Option<WorkflowStep>,
}

impl JsonSchema for PlanWorkflowArgs {
    fn schema_name() -> String {
        "PlanWorkflowArgs".into()
    }

    fn json_schema(generator: &mut schemars::gen::SchemaGenerator) -> schemars::schema::Schema {
        WorkflowToolSchema::json_schema(generator)
    }
}

/// Created by the host from the originating human turn. Child stage results
/// cannot create or replace this provenance.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExecutionAuthority {
    pub source_user_message: usize,
    pub source_request_sha256: String,
}

/// A model request is tied to the human input and execution turn that exposed
/// its tool schema. Queuing must never borrow authority from a later message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowRequestSource {
    pub turn_id: Uuid,
    pub source_user_message: usize,
    pub source_request_sha256: String,
    pub implementation_requested: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingPlanWorkflow {
    pub args: PlanWorkflowArgs,
    pub source: WorkflowRequestSource,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowStatus {
    Running,
    Paused,
    Completed,
    Failed,
    Cancelled,
}

impl WorkflowStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Running => "Running",
            Self::Paused => "Paused",
            Self::Completed => "Completed",
            Self::Failed => "Failed",
            Self::Cancelled => "Cancelled",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowStageStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
    Interrupted,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkflowParticipant {
    pub id: String,
    pub route: AgentRoute,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlanBinding {
    pub plan_id: PlanId,
    pub revision: u64,
    pub sha256: String,
}

impl PlanBinding {
    pub fn for_plan(plan: &PlanArtifact) -> Result<Self, String> {
        Ok(Self {
            plan_id: plan.id,
            revision: plan.revision,
            sha256: digest(&serde_json::to_vec(plan).map_err(|error| error.to_string())?),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlanSnapshot {
    pub plan: PlanArtifact,
    pub sha256: String,
    pub markdown: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkflowStage {
    pub id: Uuid,
    pub role: WorkflowRole,
    pub participant: String,
    pub route: AgentRoute,
    pub instructions: String,
    pub status: WorkflowStageStatus,
    pub agent_id: Option<Uuid>,
    pub source_plan: Option<PlanBinding>,
    pub result_snapshot: Option<usize>,
    pub result_summary: Option<String>,
    pub feedback: Option<String>,
    pub accepted: Option<bool>,
    pub loop_stage: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlanWorkflowState {
    pub id: Uuid,
    pub goal: String,
    pub status: WorkflowStatus,
    pub participants: Vec<WorkflowParticipant>,
    pub steps: Vec<WorkflowStage>,
    pub snapshots: Vec<PlanSnapshot>,
    pub cursor: usize,
    pub notice: Option<String>,
    pub review_loop: Option<WorkflowReviewLoop>,
    pub review_passes: u8,
    pub accepted_plan: Option<PlanBinding>,
    pub implementation: Option<WorkflowStep>,
    pub implementation_authority: Option<ExecutionAuthority>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct PlanWorkflowDispatch {
    pub workflow_id: Uuid,
    pub stage_id: Uuid,
    pub step_index: usize,
    pub role: WorkflowRole,
    pub participant: String,
    pub route: AgentRoute,
    pub prompt: String,
    pub output_schema: Option<serde_json::Value>,
    pub source_plan: Option<PlanBinding>,
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn validate_identity(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        return Err(
            "Participant and provider IDs need 1–64 letters, digits, dots, dashes or underscores"
                .into(),
        );
    }
    Ok(())
}

fn validate_route(route: &AgentRoute) -> Result<(), String> {
    validate_identity(&route.provider)?;
    if route.model.trim().is_empty()
        || route.model.trim() != route.model
        || route.model.len() > 256
        || route.model.chars().any(char::is_control)
    {
        return Err("Invalid plan participant model".into());
    }
    Ok(())
}

impl PlanWorkflowState {
    pub fn validate(&self) -> Result<(), String> {
        if self.goal.trim().is_empty()
            || self.goal.len() > MAX_INSTRUCTIONS_BYTES
            || self.participants.is_empty()
            || self.participants.len() > MAX_PARTICIPANTS
            || self.steps.is_empty()
            || self.steps.len() > MAX_WORKFLOW_STAGES
            || self.cursor > self.steps.len()
            || self.snapshots.len() > MAX_WORKFLOW_STAGES + 1
        {
            return Err("Plan workflow exceeds its bounds".into());
        }
        let mut identities = HashSet::new();
        for participant in &self.participants {
            validate_identity(&participant.id)?;
            validate_route(&participant.route)?;
            if !identities.insert(participant.id.as_str()) {
                return Err("Duplicate plan participant".into());
            }
        }
        let mut stage_ids = HashSet::new();
        for (index, step) in self.steps.iter().enumerate() {
            if !stage_ids.insert(step.id)
                || step.instructions.len() > MAX_INSTRUCTIONS_BYTES
                || !self.participants.iter().any(|participant| {
                    participant.id == step.participant && participant.route == step.route
                })
                || step
                    .result_snapshot
                    .is_some_and(|snapshot| snapshot >= self.snapshots.len())
                || (step.status == WorkflowStageStatus::Running
                    && (step.agent_id.is_none() || index != self.cursor))
                || step
                    .feedback
                    .as_ref()
                    .is_some_and(|feedback| feedback.len() > MAX_PLAN_BYTES)
                || step
                    .result_summary
                    .as_ref()
                    .is_some_and(|summary| summary.len() > MAX_PLAN_BYTES)
            {
                return Err("Invalid plan workflow stage".into());
            }
        }
        if let Some(loop_definition) = &self.review_loop {
            if loop_definition.max_review_passes == 0
                || loop_definition.max_review_passes > MAX_REVIEW_PASSES
                || self.review_passes > loop_definition.max_review_passes
                || !identities.contains(loop_definition.reviewer.as_str())
                || !identities.contains(loop_definition.reviser.as_str())
            {
                return Err("Invalid bounded plan review loop".into());
            }
        }
        if self
            .implementation_authority
            .as_ref()
            .is_some_and(|authority| !valid_digest(&authority.source_request_sha256))
        {
            return Err("Invalid originating user request provenance".into());
        }
        for snapshot in &self.snapshots {
            if snapshot.markdown.len() > MAX_PLAN_BYTES
                || snapshot.markdown != snapshot.plan.render_document()
                || snapshot.sha256
                    != digest(
                        &serde_json::to_vec(&snapshot.plan).map_err(|error| error.to_string())?,
                    )
            {
                return Err("Saved plan snapshot does not match its immutable contents".into());
            }
        }
        for binding in self
            .steps
            .iter()
            .filter_map(|stage| stage.source_plan.as_ref())
            .chain(self.accepted_plan.iter())
        {
            if !self.snapshots.iter().any(|snapshot| {
                snapshot.sha256 == binding.sha256
                    && snapshot.plan.id == binding.plan_id
                    && snapshot.plan.revision == binding.revision
            }) {
                return Err("Saved plan stage references an unknown source snapshot".into());
            }
        }
        if serde_json::to_vec(self)
            .map_err(|error| error.to_string())?
            .len()
            > MAX_WORKFLOW_BYTES
        {
            return Err("Plan workflow history exceeds 2 MiB; start another workflow".into());
        }
        Ok(())
    }

    pub fn pause_after_restore(&mut self) -> bool {
        if self.status != WorkflowStatus::Running {
            return false;
        }
        self.status = WorkflowStatus::Paused;
        self.notice = Some("Plan workflow paused after restore; resume explicitly".into());
        if let Some(stage) = self.steps.get_mut(self.cursor) {
            if stage.status == WorkflowStageStatus::Running {
                stage.status = WorkflowStageStatus::Interrupted;
                stage.result_summary = Some("Interrupted by session restore".into());
            }
        }
        self.updated_at = Utc::now();
        true
    }

    pub fn fork_full(&self) -> Self {
        let mut fork = self.clone();
        fork.id = Uuid::new_v4();
        fork.implementation_authority = None;
        fork.status = WorkflowStatus::Paused;
        fork.notice = Some("Forked plan history; execution needs a new user instruction".into());
        for stage in &mut fork.steps {
            stage.id = Uuid::new_v4();
            stage.agent_id = None;
            if stage.status == WorkflowStageStatus::Running {
                stage.status = WorkflowStageStatus::Interrupted;
            }
        }
        // Participant names are local handles. Remapping stage/workflow UUIDs
        // makes the fork independent while preserving readable return targets.
        fork.updated_at = Utc::now();
        fork
    }

    fn add_snapshot(&mut self, plan: &PlanArtifact) -> Result<usize, String> {
        let binding = PlanBinding::for_plan(plan)?;
        if let Some(index) = self
            .snapshots
            .iter()
            .position(|snapshot| snapshot.sha256 == binding.sha256)
        {
            return Ok(index);
        }
        let markdown = plan.render_document();
        if markdown.len() > MAX_PLAN_BYTES {
            return Err("Plan exceeds the 128 KiB publication limit".into());
        }
        let index = self.snapshots.len();
        self.snapshots.push(PlanSnapshot {
            plan: plan.clone(),
            sha256: binding.sha256,
            markdown,
            created_at: Utc::now(),
        });
        Ok(index)
    }

    fn resolve_participant(
        &mut self,
        id: &str,
        requested: Option<&AgentRoute>,
        default_route: &AgentRoute,
    ) -> Result<AgentRoute, String> {
        validate_identity(id)?;
        if let Some(participant) = self.participants.iter().find(|item| item.id == id) {
            if requested.is_some_and(|route| route != &participant.route) {
                return Err(
                    "A participant's route is frozen; use a new participant ID for another route"
                        .into(),
                );
            }
            return Ok(participant.route.clone());
        }
        let route = requested.unwrap_or(default_route).clone();
        validate_route(&route)?;
        self.participants.push(WorkflowParticipant {
            id: id.into(),
            route: route.clone(),
        });
        Ok(route)
    }

    fn append_step(
        &mut self,
        step: WorkflowStep,
        default_route: &AgentRoute,
        loop_stage: bool,
    ) -> Result<(), String> {
        if self.steps.len() >= MAX_WORKFLOW_STAGES {
            return Err("Plan workflow reached its 64-stage limit".into());
        }
        if step.role == WorkflowRole::Implement && self.implementation_authority.is_none() {
            return Err(
                "Implementation was not requested by the originating user instruction".into(),
            );
        }
        let route =
            self.resolve_participant(&step.participant, step.route.as_ref(), default_route)?;
        self.steps.push(WorkflowStage {
            id: Uuid::new_v4(),
            role: step.role,
            participant: step.participant,
            route,
            instructions: step.instructions,
            status: WorkflowStageStatus::Pending,
            agent_id: None,
            source_plan: None,
            result_snapshot: None,
            result_summary: None,
            feedback: None,
            accepted: None,
            loop_stage,
        });
        Ok(())
    }

    pub fn render_status(&self) -> String {
        let mut text = format!(
            "Plan workflow {} · {}\n{}\n",
            self.id,
            self.status.label(),
            self.goal
        );
        for (index, stage) in self.steps.iter().enumerate() {
            text.push_str(&format!(
                "{}. {} · {} · {}/{} · {:?}\n",
                index + 1,
                stage.role.label(),
                stage.participant,
                stage.route.provider,
                stage.route.model,
                stage.status
            ));
        }
        if let Some(notice) = &self.notice {
            text.push_str(notice);
        }
        text
    }
}

pub fn apply_action(
    work: &mut WorkState,
    args: PlanWorkflowArgs,
    default_route: &AgentRoute,
    authority: Option<ExecutionAuthority>,
) -> Result<String, String> {
    if matches!(
        args,
        PlanWorkflowArgs::Start { .. } | PlanWorkflowArgs::Handoff { .. }
    ) && task_graph_active(work)
    {
        return Err("Pause the task graph before changing or handing off this plan.".into());
    }
    if matches!(args, PlanWorkflowArgs::Status) {
        return work
            .plan_workflow
            .as_ref()
            .map(PlanWorkflowState::render_status)
            .ok_or_else(|| "No plan workflow; start one or send the current plan".into());
    }
    let mut candidate = work.plan_workflow.clone();
    match args {
        PlanWorkflowArgs::Start {
            goal,
            participants,
            steps,
            review_loop,
            implementation,
        } => {
            if candidate.as_ref().is_some_and(|workflow| {
                matches!(
                    workflow.status,
                    WorkflowStatus::Running | WorkflowStatus::Paused
                )
            }) {
                return Err(
                    "Finish or cancel the current plan workflow before starting another".into(),
                );
            }
            if steps.is_empty() {
                return Err("A plan workflow needs at least one stage".into());
            }
            if work.plan.is_none() && steps[0].role != WorkflowRole::Author {
                return Err("The first stage must author a plan when no plan exists".into());
            }
            let now = Utc::now();
            let mut workflow = PlanWorkflowState {
                id: Uuid::new_v4(),
                goal,
                status: WorkflowStatus::Running,
                participants: Vec::new(),
                steps: Vec::new(),
                snapshots: Vec::new(),
                cursor: 0,
                notice: None,
                review_loop,
                review_passes: 0,
                accepted_plan: None,
                implementation: implementation.clone(),
                implementation_authority: implementation.as_ref().and(authority),
                created_at: now,
                updated_at: now,
            };
            let mut ids = HashSet::new();
            for participant in participants {
                if !ids.insert(participant.id.clone()) {
                    return Err("Duplicate plan participant".into());
                }
                workflow.resolve_participant(
                    &participant.id,
                    participant.route.as_ref(),
                    default_route,
                )?;
            }
            if let Some(plan) = &work.plan {
                workflow.add_snapshot(plan)?;
            }
            for step in steps {
                if step.role == WorkflowRole::Implement {
                    return Err("Put implementation in the final implementation stage".into());
                }
                workflow.append_step(step, default_route, false)?;
            }
            if let Some(definition) = workflow.review_loop.clone() {
                workflow.resolve_participant(&definition.reviewer, None, default_route)?;
                workflow.resolve_participant(&definition.reviser, None, default_route)?;
                workflow.append_step(
                    WorkflowStep {
                        role: WorkflowRole::Review,
                        participant: definition.reviewer,
                        route: None,
                        instructions: String::new(),
                    },
                    default_route,
                    true,
                )?;
            }
            if let Some(implementation) = implementation {
                if implementation.role != WorkflowRole::Implement {
                    return Err("The final implementation stage must have role implement".into());
                }
                if workflow.implementation_authority.is_none() {
                    return Err(
                        "Implementation was not requested by the originating user instruction"
                            .into(),
                    );
                }
                let resolved = workflow.resolve_participant(
                    &implementation.participant,
                    implementation.route.as_ref(),
                    default_route,
                )?;
                workflow.implementation = Some(WorkflowStep {
                    route: Some(resolved),
                    ..implementation.clone()
                });
                if workflow.review_loop.is_none() {
                    workflow.append_step(implementation, default_route, false)?;
                }
            }
            candidate = Some(workflow);
        }
        PlanWorkflowArgs::Handoff { step } => {
            let workflow = if let Some(workflow) = candidate.as_mut() {
                if workflow.status == WorkflowStatus::Running
                    || workflow
                        .steps
                        .iter()
                        .any(|step| step.status == WorkflowStageStatus::Running)
                {
                    return Err("Finish or pause the active stage before sending the plan".into());
                }
                workflow
            } else {
                let plan = work
                    .plan
                    .as_ref()
                    .ok_or("Create a plan before sending it")?;
                let now = Utc::now();
                candidate = Some(PlanWorkflowState {
                    id: Uuid::new_v4(),
                    goal: plan.title.clone(),
                    status: WorkflowStatus::Running,
                    participants: Vec::new(),
                    steps: Vec::new(),
                    snapshots: Vec::new(),
                    cursor: 0,
                    notice: None,
                    review_loop: None,
                    review_passes: 0,
                    accepted_plan: None,
                    implementation: None,
                    implementation_authority: None,
                    created_at: now,
                    updated_at: now,
                });
                candidate.as_mut().expect("created workflow")
            };
            if step.role == WorkflowRole::Implement && authority.is_some() {
                workflow.implementation_authority = authority;
            }
            if let Some(plan) = &work.plan {
                workflow.add_snapshot(plan)?;
            }
            for abandoned in workflow.steps.iter_mut().skip(workflow.cursor) {
                if abandoned.status == WorkflowStageStatus::Pending {
                    abandoned.status = WorkflowStageStatus::Cancelled;
                }
            }
            workflow.cursor = workflow.steps.len();
            workflow.review_loop = None;
            workflow.implementation = None;
            workflow.append_step(step, default_route, false)?;
            workflow.status = WorkflowStatus::Running;
            workflow.notice = None;
        }
        PlanWorkflowArgs::Pause => {
            let workflow = candidate.as_mut().ok_or("No plan workflow")?;
            if workflow.status != WorkflowStatus::Running {
                return Err("The plan workflow is not running".into());
            }
            workflow.status = WorkflowStatus::Paused;
            workflow.notice =
                Some("Paused; an active stage may finish, but no next stage starts".into());
        }
        PlanWorkflowArgs::Resume => {
            let workflow = candidate.as_mut().ok_or("No plan workflow")?;
            if workflow.status != WorkflowStatus::Paused {
                return Err("Only a paused plan workflow can resume".into());
            }
            if workflow.review_loop.as_ref().is_some_and(|definition| {
                workflow.review_passes >= definition.max_review_passes
                    && workflow.accepted_plan.is_none()
            }) {
                return Err("The review pass limit was reached without acceptance; send a new handoff or start another workflow".into());
            }
            if let Some(stage) = workflow.steps.get_mut(workflow.cursor) {
                if stage.status == WorkflowStageStatus::Interrupted {
                    if stage.role == WorkflowRole::Implement {
                        return Err("Interrupted implementation may have changed files; send a fresh implementation instruction".into());
                    }
                    stage.status = WorkflowStageStatus::Pending;
                    stage.agent_id = None;
                }
            }
            workflow.status = if workflow.cursor == workflow.steps.len() {
                WorkflowStatus::Completed
            } else {
                WorkflowStatus::Running
            };
            workflow.notice = None;
        }
        PlanWorkflowArgs::Cancel => {
            let workflow = candidate.as_mut().ok_or("No plan workflow")?;
            workflow.status = WorkflowStatus::Cancelled;
            workflow.notice = Some("Plan workflow cancelled".into());
            for stage in &mut workflow.steps {
                if matches!(
                    stage.status,
                    WorkflowStageStatus::Pending | WorkflowStageStatus::Running
                ) {
                    stage.status = WorkflowStageStatus::Cancelled;
                }
            }
        }
        PlanWorkflowArgs::Status => unreachable!(),
    }
    let workflow = candidate.as_mut().ok_or("No plan workflow")?;
    workflow.updated_at = Utc::now();
    workflow.validate()?;
    let status = workflow.render_status();
    work.plan_workflow = candidate;
    Ok(status)
}

pub fn task_graph_active(work: &WorkState) -> bool {
    work.agent_graph.status == crate::agent_graph::AgentGraphStatus::Running
        || work.plan.as_ref().is_some_and(|plan| {
            plan.tasks
                .iter()
                .any(|task| task.agent_runtime.state == crate::agent_graph::AgentTaskState::Running)
        })
}

pub fn next_dispatch(work: &WorkState) -> Result<Option<PlanWorkflowDispatch>, String> {
    let Some(workflow) = &work.plan_workflow else {
        return Ok(None);
    };
    if workflow.status != WorkflowStatus::Running {
        return Ok(None);
    }
    workflow.validate()?;
    let Some(stage) = workflow.steps.get(workflow.cursor) else {
        return Ok(None);
    };
    if stage.status != WorkflowStageStatus::Pending {
        return Ok(None);
    }
    if stage.role != WorkflowRole::Author && work.plan.is_none() {
        return Err("This plan stage needs an existing plan".into());
    }
    if stage.role == WorkflowRole::Implement && workflow.implementation_authority.is_none() {
        return Err("This workflow has no originating user implementation instruction".into());
    }
    let source_plan = work.plan.as_ref().map(PlanBinding::for_plan).transpose()?;
    if stage.role == WorkflowRole::Implement
        && workflow.review_loop.is_some()
        && workflow.accepted_plan != source_plan
    {
        return Err(
            "The plan changed after acceptance; review the current version before implementation"
                .into(),
        );
    }
    let current = work.plan.as_ref().map_or_else(
        || "No plan has been authored yet.".into(),
        PlanArtifact::render_document,
    );
    let mut contributions = String::new();
    for previous in workflow.steps.iter().take(workflow.cursor) {
        if previous.participant == stage.participant {
            let prior_contribution = previous
                .result_snapshot
                .and_then(|index| workflow.snapshots.get(index))
                .map(|snapshot| snapshot.markdown.as_str())
                .or(previous.result_summary.as_deref())
                .unwrap_or("No committed output");
            contributions.push_str(&format!(
                "\nYour prior {} contribution:\n{}\n",
                previous.role.label(),
                prior_contribution
            ));
        }
        if let Some(feedback) = &previous.feedback {
            contributions.push_str(&format!(
                "\nFeedback from {} ({}/{}):\n{}\n",
                previous.participant, previous.route.provider, previous.route.model, feedback
            ));
        }
    }
    let task = match stage.role {
        WorkflowRole::Author => "Author a complete implementation plan. Return title, full Markdown body, and ordered task titles.",
        WorkflowRole::Review => "Review the exact current plan. Return verdict accepted or needs_changes and concrete feedback. Acceptance is a plan quality verdict, never user execution authorization.",
        WorkflowRole::Revise => "Revise the full plan using the feedback. Return title, complete replacement Markdown body, and ordered task titles.",
        WorkflowRole::Implement => "Implement the current plan using ordinary Agent tools and record validation evidence.",
    };
    Ok(Some(PlanWorkflowDispatch {
        workflow_id: workflow.id,
        stage_id: stage.id,
        step_index: workflow.cursor,
        role: stage.role,
        participant: stage.participant.clone(),
        route: stage.route.clone(),
        prompt: format!(
            "Plan workflow goal:\n{}\n\nYou are logical participant {} on {}/{}.\n{}\n\nStage instruction:\n{}\n\nCurrent plan:\n{}\n\nCommitted exchange context:\n{}",
            workflow.goal, stage.participant, stage.route.provider, stage.route.model,
            task, stage.instructions, current, contributions
        ),
        output_schema: output_schema(stage.role),
        source_plan,
    }))
}

pub fn active_dispatch(work: &WorkState) -> Option<(Uuid, WorkflowRole)> {
    let workflow = work.plan_workflow.as_ref()?;
    let stage = workflow.steps.get(workflow.cursor)?;
    if stage.status != WorkflowStageStatus::Running {
        return None;
    }
    stage.agent_id.map(|id| (id, stage.role))
}

pub fn needs_tick(work: &WorkState) -> bool {
    work.pending_plan_workflow.is_some()
        || work.plan_workflow.as_ref().is_some_and(|workflow| {
            workflow.status == WorkflowStatus::Running || active_dispatch(work).is_some()
        })
}

pub fn output_schema(role: WorkflowRole) -> Option<serde_json::Value> {
    Some(match role {
        WorkflowRole::Author | WorkflowRole::Revise => serde_json::json!({
            "type":"object", "additionalProperties":false,
            "properties":{
                "title":{"type":"string"},"body":{"type":"string"},
                "tasks":{"type":"array","items":{"type":"string"},"minItems":1,"maxItems":64}
            }, "required":["title","body","tasks"]
        }),
        WorkflowRole::Review => serde_json::json!({
            "type":"object", "additionalProperties":false,
            "properties":{
                "verdict":{"type":"string","enum":["accepted","needs_changes"]},
                "feedback":{"type":"string"}
            }, "required":["verdict","feedback"]
        }),
        WorkflowRole::Implement => return None,
    })
}

pub fn mark_dispatched(
    work: &mut WorkState,
    dispatch: &PlanWorkflowDispatch,
    agent_id: Uuid,
) -> Result<(), String> {
    let binding = work.plan.as_ref().map(PlanBinding::for_plan).transpose()?;
    let current_plan = work.plan.clone();
    let workflow = work.plan_workflow.as_mut().ok_or("No plan workflow")?;
    let stage = workflow
        .steps
        .get(workflow.cursor)
        .ok_or("No pending plan stage")?;
    if workflow.id != dispatch.workflow_id
        || workflow.status != WorkflowStatus::Running
        || workflow.cursor != dispatch.step_index
        || stage.id != dispatch.stage_id
        || stage.status != WorkflowStageStatus::Pending
        || binding != dispatch.source_plan
    {
        return Err("The plan workflow changed before dispatch".into());
    }
    if let Some(plan) = &current_plan {
        workflow.add_snapshot(plan)?;
    }
    let stage = &mut workflow.steps[workflow.cursor];
    stage.status = WorkflowStageStatus::Running;
    stage.agent_id = Some(agent_id);
    stage.source_plan = binding;
    workflow.updated_at = Utc::now();
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlanResult {
    title: String,
    body: String,
    tasks: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewResult {
    verdict: String,
    feedback: String,
}

pub fn complete_stage(
    work: &mut WorkState,
    agent_id: Uuid,
    result: serde_json::Value,
) -> Result<(), String> {
    let mut candidate = work.clone();
    complete_stage_inner(&mut candidate, agent_id, result)?;
    candidate
        .plan_workflow
        .as_ref()
        .ok_or("No plan workflow")?
        .validate()?;
    *work = candidate;
    Ok(())
}

fn complete_stage_inner(
    work: &mut WorkState,
    agent_id: Uuid,
    result: serde_json::Value,
) -> Result<(), String> {
    if serde_json::to_vec(&result)
        .map_err(|error| error.to_string())?
        .len()
        > MAX_PLAN_BYTES
    {
        return Err("Plan stage result exceeds 128 KiB".into());
    }
    let mut workflow = work.plan_workflow.take().ok_or("No plan workflow")?;
    let stage = workflow
        .steps
        .get(workflow.cursor)
        .cloned()
        .ok_or("No active plan stage")?;
    if matches!(stage.role, WorkflowRole::Author | WorkflowRole::Revise) && task_graph_active(work)
    {
        return Err("Pause the task graph before changing or handing off this plan.".into());
    }
    if stage.status != WorkflowStageStatus::Running || stage.agent_id != Some(agent_id) {
        return Err("The plan stage result has a stale owner".into());
    }
    if stage.role != WorkflowRole::Implement
        && stage.source_plan != work.plan.as_ref().map(PlanBinding::for_plan).transpose()?
    {
        return Err(
            "The plan changed while this stage was running; the result was not applied".into(),
        );
    }
    let summary: String;
    let mut feedback = None;
    let mut accepted = None;
    let mut result_snapshot = None;
    match stage.role {
        WorkflowRole::Author | WorkflowRole::Revise => {
            let proposal: PlanResult =
                serde_json::from_value(result).map_err(|error| error.to_string())?;
            if proposal.body.trim().is_empty() || proposal.tasks.len() > 64 {
                return Err("A plan needs a Markdown body and 1–64 tasks".into());
            }
            let source_id = work.plan.as_ref().map(|plan| plan.id);
            work.apply_model_mutation(WorkMutation::Propose {
                expected_revision: work.revision(),
                title: proposal.title,
                body: Some(proposal.body),
                tasks: proposal.tasks,
            })
            .map_err(|error| error.to_string())?;
            let plan = work.plan.as_mut().ok_or("Plan proposal disappeared")?;
            plan.source_plan_id = source_id;
            summary = plan.render_document();
            result_snapshot = Some(workflow.add_snapshot(plan)?);
            workflow.accepted_plan = None;
        }
        WorkflowRole::Review => {
            let review: ReviewResult =
                serde_json::from_value(result).map_err(|error| error.to_string())?;
            let verdict = match review.verdict.as_str() {
                "accepted" => true,
                "needs_changes" => false,
                _ => return Err("Review verdict must be accepted or needs_changes".into()),
            };
            summary = format!("{}\n{}", review.verdict, review.feedback);
            feedback = Some(review.feedback);
            accepted = Some(verdict);
            if verdict {
                workflow.accepted_plan = stage.source_plan.clone();
            } else {
                workflow.accepted_plan = None;
            }
        }
        WorkflowRole::Implement => {
            let succeeded = result
                .get("succeeded")
                .and_then(serde_json::Value::as_bool)
                .ok_or("Implementation result needs a host succeeded value")?;
            summary = result
                .get("summary")
                .and_then(serde_json::Value::as_str)
                .ok_or("Implementation result needs a host summary")?
                .into();
            if !succeeded {
                return Err(summary);
            }
            if let Some(plan) = &work.plan {
                result_snapshot = Some(workflow.add_snapshot(plan)?);
            }
        }
    }
    let completed = &mut workflow.steps[workflow.cursor];
    completed.status = WorkflowStageStatus::Completed;
    completed.result_summary = Some(summary);
    completed.feedback = feedback;
    completed.accepted = accepted;
    completed.result_snapshot = result_snapshot;
    workflow.cursor += 1;
    if stage.loop_stage && stage.role == WorkflowRole::Review {
        workflow.review_passes += 1;
        let definition = workflow
            .review_loop
            .clone()
            .ok_or("Missing plan review loop")?;
        if accepted == Some(true) {
            if let Some(implementation) = workflow.implementation.clone() {
                let default_route = stage.route.clone();
                workflow.append_step(implementation, &default_route, false)?;
            }
        } else if workflow.review_passes < definition.max_review_passes {
            let default_route = stage.route.clone();
            for (role, participant) in [
                (WorkflowRole::Revise, definition.reviser),
                (WorkflowRole::Review, definition.reviewer),
            ] {
                workflow.append_step(
                    WorkflowStep {
                        role,
                        participant,
                        route: None,
                        instructions: String::new(),
                    },
                    &default_route,
                    true,
                )?;
            }
        } else {
            workflow.status = WorkflowStatus::Paused;
            workflow.notice = Some(format!(
                "Plan was not accepted after {} review passes; implementation did not start",
                workflow.review_passes
            ));
        }
    }
    let review_limit_reached = workflow.review_loop.as_ref().is_some_and(|definition| {
        workflow.review_passes >= definition.max_review_passes && workflow.accepted_plan.is_none()
    });
    if !review_limit_reached
        && workflow.cursor == workflow.steps.len()
        && matches!(
            workflow.status,
            WorkflowStatus::Running | WorkflowStatus::Paused
        )
    {
        workflow.status = WorkflowStatus::Completed;
        workflow.notice = Some("Plan workflow completed".into());
    }
    workflow.updated_at = Utc::now();
    work.plan_workflow = Some(workflow);
    Ok(())
}

pub fn complete_implementation(
    work: &mut WorkState,
    agent_id: Uuid,
    succeeded: bool,
    summary: &str,
) -> Result<(), String> {
    complete_stage(
        work,
        agent_id,
        serde_json::json!({"succeeded":succeeded,"summary":summary}),
    )
}

pub fn fail_stage(work: &mut WorkState, agent_id: Option<Uuid>, reason: &str) {
    let Some(workflow) = work.plan_workflow.as_mut() else {
        return;
    };
    if workflow.status == WorkflowStatus::Cancelled {
        return;
    }
    if let Some(stage) = workflow.steps.get_mut(workflow.cursor) {
        if agent_id.is_some() && stage.agent_id != agent_id {
            return;
        }
        stage.status = WorkflowStageStatus::Failed;
        stage.result_summary = Some(reason.chars().take(4096).collect());
    }
    workflow.status = WorkflowStatus::Failed;
    workflow.notice = Some(reason.chars().take(4096).collect());
    workflow.updated_at = Utc::now();
}

/// Publish only beneath a host-selected profile data directory. Snapshot files
/// are create-once and verified on readback; the current projection is atomic.
pub fn write_projection(
    data_root: &Path,
    session_id: Uuid,
    workflow: &PlanWorkflowState,
) -> Result<PathBuf, String> {
    workflow.validate()?;
    std::fs::create_dir_all(data_root).map_err(|error| error.to_string())?;
    let mut root = std::fs::canonicalize(data_root).map_err(|error| error.to_string())?;
    for component in [
        "plans".to_string(),
        session_id.to_string(),
        workflow.id.to_string(),
    ] {
        root.push(component);
        private_directory(&root)?;
    }
    let versions = root.join("revisions");
    private_directory(&versions)?;
    for (index, snapshot) in workflow.snapshots.iter().enumerate() {
        let path = versions.join(format!("{:04}-{}.md", index + 1, snapshot.sha256));
        immutable_file(&path, snapshot.markdown.as_bytes())?;
    }
    let current = root.join("plan.md");
    if let Some(snapshot) = workflow.snapshots.last() {
        atomic_projection(&current, snapshot.markdown.as_bytes())?;
    }
    Ok(current)
}

fn private_directory(path: &Path) -> Result<(), String> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() => {}
        Ok(_) => return Err("Plan projection directory is not a safe directory".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let mut builder = std::fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            builder.create(path).map_err(|error| error.to_string())?;
        }
        Err(error) => return Err(error.to_string()),
    }
    Ok(())
}

fn immutable_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Ok(metadata) = std::fs::symlink_metadata(path) {
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || metadata.len() != bytes.len() as u64
        {
            return Err("An immutable plan projection changed".into());
        }
        let mut existing = Vec::new();
        std::fs::File::open(path)
            .and_then(|file| {
                file.take(MAX_PLAN_BYTES as u64 + 1)
                    .read_to_end(&mut existing)
            })
            .map_err(|error| error.to_string())?;
        if existing != bytes {
            return Err("An immutable plan projection changed".into());
        }
        return Ok(());
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|error| error.to_string())?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|error| error.to_string())
}

fn atomic_projection(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Ok(metadata) = std::fs::symlink_metadata(path) {
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err("Current plan projection is not a regular file".into());
        }
    }
    let temporary = path.with_file_name(format!(".plan-{}.tmp", Uuid::new_v4()));
    immutable_file(&temporary, bytes)?;
    if let Err(error) = std::fs::rename(&temporary, path) {
        let _ = std::fs::remove_file(&temporary);
        return Err(error.to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn route(provider: &str) -> AgentRoute {
        AgentRoute {
            provider: provider.into(),
            model: "model".into(),
        }
    }

    fn step(role: WorkflowRole, participant: &str) -> WorkflowStep {
        WorkflowStep {
            role,
            participant: participant.into(),
            route: None,
            instructions: String::new(),
        }
    }

    fn start(
        work: &mut WorkState,
        loop_definition: Option<WorkflowReviewLoop>,
        implementation: bool,
    ) {
        apply_action(
            work,
            PlanWorkflowArgs::Start {
                goal: "Build the requested feature".into(),
                participants: vec![
                    WorkflowParticipantInput {
                        id: "a".into(),
                        route: Some(route("provider-a")),
                    },
                    WorkflowParticipantInput {
                        id: "b".into(),
                        route: Some(route("provider-b")),
                    },
                ],
                steps: vec![step(WorkflowRole::Author, "a")],
                review_loop: loop_definition,
                implementation: implementation.then(|| step(WorkflowRole::Implement, "a")),
            },
            &route("default"),
            implementation.then(|| ExecutionAuthority {
                source_user_message: 0,
                source_request_sha256: "a".repeat(64),
            }),
        )
        .unwrap();
    }

    fn finish(work: &mut WorkState, value: serde_json::Value) -> PlanWorkflowDispatch {
        let dispatch = next_dispatch(work).unwrap().unwrap();
        let id = Uuid::new_v4();
        mark_dispatched(work, &dispatch, id).unwrap();
        assert!(next_dispatch(work).unwrap().is_none());
        complete_stage(work, id, value).unwrap();
        dispatch
    }

    fn proposal(body: &str) -> serde_json::Value {
        serde_json::json!({"title":"Feature plan","body":body,"tasks":["Implement","Validate"]})
    }

    #[test]
    fn workflow_tool_schema_has_an_object_root_and_keeps_action_validation() {
        let schema = serde_json::to_value(schemars::schema_for!(PlanWorkflowArgs)).unwrap();
        assert_eq!(schema["type"], "object");
        assert!(schema.get("oneOf").is_none());
        assert_eq!(schema["additionalProperties"], false);
        assert_eq!(schema["required"], serde_json::json!(["action"]));
        assert_eq!(
            schema["definitions"]["WorkflowToolAction"]["enum"],
            serde_json::json!(["start", "handoff", "status", "pause", "resume", "cancel"])
        );
        for field in [
            "action",
            "goal",
            "participants",
            "steps",
            "review_loop",
            "implementation",
            "step",
        ] {
            assert!(schema["properties"].get(field).is_some(), "Missing {field}");
        }
        assert!(
            serde_json::from_value::<PlanWorkflowArgs>(serde_json::json!({"action":"start"}))
                .is_err()
        );
        assert!(serde_json::from_value::<PlanWorkflowArgs>(
            serde_json::json!({"action":"handoff"})
        )
        .is_err());
        assert_eq!(
            serde_json::from_value::<PlanWorkflowArgs>(serde_json::json!({"action":"status"}))
                .unwrap(),
            PlanWorkflowArgs::Status
        );
    }

    #[test]
    fn cross_provider_return_preserves_contributions_and_immutable_versions() {
        let mut work = WorkState::default();
        start(&mut work, None, false);
        let author = finish(&mut work, proposal("Initial plan"));
        assert_eq!(author.route, route("provider-a"));
        let first = work.plan.clone().unwrap();
        apply_action(
            &mut work,
            PlanWorkflowArgs::Handoff {
                step: step(WorkflowRole::Review, "b"),
            },
            &route("changed-default"),
            None,
        )
        .unwrap();
        let review = finish(
            &mut work,
            serde_json::json!({"verdict":"needs_changes","feedback":"Add failure verification"}),
        );
        assert_eq!(review.route, route("provider-b"));
        assert_eq!(work.plan.as_ref(), Some(&first));
        apply_action(
            &mut work,
            PlanWorkflowArgs::Handoff {
                step: step(WorkflowRole::Revise, "a"),
            },
            &route("changed-default"),
            None,
        )
        .unwrap();
        let revision = next_dispatch(&work).unwrap().unwrap();
        assert_eq!(revision.route, route("provider-a"));
        assert!(revision.prompt.contains("Your prior Author contribution"));
        assert!(revision.prompt.contains("Add failure verification"));
        finish(
            &mut work,
            proposal("Revised plan with failure verification"),
        );
        let workflow = work.plan_workflow.as_ref().unwrap();
        assert_eq!(workflow.snapshots.len(), 2);
        assert_eq!(workflow.snapshots[0].plan, first);
        assert_eq!(work.plan.as_ref().unwrap().source_plan_id, Some(first.id));
    }

    #[test]
    fn review_acceptance_cannot_create_implementation_authority() {
        let mut work = WorkState::default();
        start(&mut work, None, false);
        finish(&mut work, proposal("Plan"));
        let before = work.clone();
        assert!(apply_action(
            &mut work,
            PlanWorkflowArgs::Handoff {
                step: step(WorkflowRole::Implement, "a")
            },
            &route("default"),
            None
        )
        .is_err());
        assert_eq!(work, before);
    }

    #[test]
    fn planning_only_and_review_handoffs_never_acquire_latent_authority() {
        let mut work = WorkState::default();
        let receipt = ExecutionAuthority {
            source_user_message: 4,
            source_request_sha256: "b".repeat(64),
        };
        apply_action(
            &mut work,
            PlanWorkflowArgs::Start {
                goal: "Plan only".into(),
                participants: Vec::new(),
                steps: vec![step(WorkflowRole::Author, "a")],
                review_loop: None,
                implementation: None,
            },
            &route("first"),
            Some(receipt.clone()),
        )
        .unwrap();
        assert!(work
            .plan_workflow
            .as_ref()
            .unwrap()
            .implementation_authority
            .is_none());
        finish(&mut work, proposal("Plan"));
        apply_action(
            &mut work,
            PlanWorkflowArgs::Handoff {
                step: step(WorkflowRole::Review, "b"),
            },
            &route("second"),
            Some(receipt),
        )
        .unwrap();
        finish(
            &mut work,
            serde_json::json!({"verdict":"accepted","feedback":"Accepted"}),
        );
        assert!(work
            .plan_workflow
            .as_ref()
            .unwrap()
            .implementation_authority
            .is_none());
        assert_eq!(
            work.plan.as_ref().unwrap().status,
            crate::work::PlanStatus::Proposed
        );
    }

    #[test]
    fn pause_finishes_current_stage_without_starting_the_next_and_cancel_is_final() {
        let mut work = WorkState::default();
        start(&mut work, None, true);
        let dispatch = next_dispatch(&work).unwrap().unwrap();
        let id = Uuid::new_v4();
        mark_dispatched(&mut work, &dispatch, id).unwrap();
        apply_action(&mut work, PlanWorkflowArgs::Pause, &route("default"), None).unwrap();
        complete_stage(&mut work, id, proposal("Plan")).unwrap();
        assert_eq!(
            work.plan_workflow.as_ref().unwrap().status,
            WorkflowStatus::Paused
        );
        assert!(next_dispatch(&work).unwrap().is_none());
        apply_action(&mut work, PlanWorkflowArgs::Resume, &route("default"), None).unwrap();
        assert_eq!(
            next_dispatch(&work).unwrap().unwrap().role,
            WorkflowRole::Implement
        );
        apply_action(&mut work, PlanWorkflowArgs::Cancel, &route("default"), None).unwrap();
        assert!(next_dispatch(&work).unwrap().is_none());
        assert_eq!(
            work.plan_workflow.as_ref().unwrap().status,
            WorkflowStatus::Cancelled
        );
    }

    #[test]
    fn malformed_and_duplicate_results_do_not_replace_the_current_plan() {
        let mut work = WorkState::default();
        start(&mut work, None, false);
        let dispatch = next_dispatch(&work).unwrap().unwrap();
        let id = Uuid::new_v4();
        mark_dispatched(&mut work, &dispatch, id).unwrap();
        let before = work.clone();
        assert!(complete_stage(
            &mut work,
            id,
            serde_json::json!({"title":"Bad","body":"Bad","tasks":[],"approved":true})
        )
        .is_err());
        assert_eq!(work, before);
        complete_stage(&mut work, id, proposal("Plan")).unwrap();
        let completed = work.clone();
        assert!(complete_stage(&mut work, id, proposal("Duplicate")).is_err());
        assert_eq!(work, completed);
    }

    #[test]
    fn bounded_review_revises_until_accepted_then_implements() {
        let mut work = WorkState::default();
        start(
            &mut work,
            Some(WorkflowReviewLoop {
                reviewer: "b".into(),
                reviser: "a".into(),
                max_review_passes: 3,
            }),
            true,
        );
        finish(&mut work, proposal("Initial"));
        finish(
            &mut work,
            serde_json::json!({"verdict":"needs_changes","feedback":"Repair"}),
        );
        assert_eq!(
            next_dispatch(&work).unwrap().unwrap().role,
            WorkflowRole::Revise
        );
        finish(&mut work, proposal("Repaired"));
        finish(
            &mut work,
            serde_json::json!({"verdict":"accepted","feedback":"Ready"}),
        );
        assert_eq!(
            next_dispatch(&work).unwrap().unwrap().role,
            WorkflowRole::Implement
        );
        finish(
            &mut work,
            serde_json::json!({"succeeded":true,"summary":"Implemented and verified"}),
        );
        assert_eq!(
            work.plan_workflow.as_ref().unwrap().status,
            WorkflowStatus::Completed
        );
    }

    #[test]
    fn exhausted_loop_retains_plan_and_does_not_implement() {
        let mut work = WorkState::default();
        start(
            &mut work,
            Some(WorkflowReviewLoop {
                reviewer: "b".into(),
                reviser: "a".into(),
                max_review_passes: 1,
            }),
            true,
        );
        finish(&mut work, proposal("Initial"));
        finish(
            &mut work,
            serde_json::json!({"verdict":"needs_changes","feedback":"Incomplete"}),
        );
        assert_eq!(
            work.plan_workflow.as_ref().unwrap().status,
            WorkflowStatus::Paused
        );
        assert!(next_dispatch(&work).unwrap().is_none());
        assert!(!work
            .plan_workflow
            .as_ref()
            .unwrap()
            .steps
            .iter()
            .any(|stage| stage.role == WorkflowRole::Implement));
        let before = work.clone();
        assert!(
            apply_action(&mut work, PlanWorkflowArgs::Resume, &route("default"), None,)
                .unwrap_err()
                .contains("review pass limit")
        );
        assert_eq!(work, before);
    }

    #[test]
    fn stale_result_is_transactional_and_restore_requires_explicit_resume() {
        let mut work = WorkState::default();
        start(&mut work, None, false);
        finish(&mut work, proposal("Initial"));
        apply_action(
            &mut work,
            PlanWorkflowArgs::Handoff {
                step: step(WorkflowRole::Revise, "a"),
            },
            &route("default"),
            None,
        )
        .unwrap();
        let dispatch = next_dispatch(&work).unwrap().unwrap();
        let id = Uuid::new_v4();
        mark_dispatched(&mut work, &dispatch, id).unwrap();
        work.apply_model_mutation(WorkMutation::Edit {
            expected_revision: work.revision(),
            title: None,
            body: Some("Human changed this".into()),
            decisions: Vec::new(),
        })
        .unwrap();
        let before = work.clone();
        assert!(complete_stage(&mut work, id, proposal("Stale revision")).is_err());
        assert_eq!(work, before);
        assert!(work.plan_workflow.as_mut().unwrap().pause_after_restore());
        assert!(next_dispatch(&work).unwrap().is_none());
        apply_action(&mut work, PlanWorkflowArgs::Resume, &route("default"), None).unwrap();
        assert!(next_dispatch(&work).unwrap().is_some());
    }

    #[test]
    fn projections_are_reproducible_immutable_and_reject_symlinks() {
        let root = tempfile::tempdir().unwrap();
        let mut work = WorkState::default();
        start(&mut work, None, false);
        finish(&mut work, proposal("# Unicode\n\n日本語 plan"));
        let workflow = work.plan_workflow.as_ref().unwrap();
        let session_id = Uuid::new_v4();
        let current = write_projection(root.path(), session_id, workflow).unwrap();
        assert_eq!(
            std::fs::read_to_string(&current).unwrap(),
            workflow.snapshots[0].markdown
        );
        assert_eq!(
            write_projection(root.path(), session_id, workflow).unwrap(),
            current
        );
        let immutable = current
            .parent()
            .unwrap()
            .join("revisions")
            .join(format!("0001-{}.md", workflow.snapshots[0].sha256));
        std::fs::write(immutable, "changed").unwrap();
        assert!(write_projection(root.path(), session_id, workflow).is_err());
        #[cfg(unix)]
        {
            let second = tempfile::tempdir().unwrap();
            std::os::unix::fs::symlink(root.path(), second.path().join("plans")).unwrap();
            assert!(write_projection(second.path(), session_id, workflow).is_err());
        }
    }

    #[test]
    fn saved_history_validates_and_fork_clears_execution_authority() {
        let mut work = WorkState::default();
        start(&mut work, None, true);
        finish(&mut work, proposal("Plan"));
        let workflow = work.plan_workflow.as_ref().unwrap();
        let restored: PlanWorkflowState =
            serde_json::from_slice(&serde_json::to_vec(workflow).unwrap()).unwrap();
        restored.validate().unwrap();
        let fork = restored.fork_full();
        assert_ne!(fork.id, restored.id);
        assert_eq!(fork.status, WorkflowStatus::Paused);
        assert!(fork.implementation_authority.is_none());
        let mut corrupt = restored;
        corrupt.snapshots[0].markdown.push('x');
        assert!(corrupt.validate().is_err());
    }

    #[test]
    fn native_session_round_trip_retains_history_but_not_pending_actions() {
        let root = tempfile::tempdir().unwrap();
        let store = crate::session::SessionStore::new(root.path());
        let mut session = crate::session::SessionV2::new(
            "Plan exchange",
            "/workspace",
            "provider-a",
            "model",
            Vec::new(),
        );
        start(&mut session.work_state, None, true);
        finish(&mut session.work_state, proposal("Persist this plan"));
        let dispatch = next_dispatch(&session.work_state).unwrap().unwrap();
        mark_dispatched(&mut session.work_state, &dispatch, Uuid::new_v4()).unwrap();
        session.work_state.pending_plan_workflow = Some(PendingPlanWorkflow {
            args: PlanWorkflowArgs::Cancel,
            source: WorkflowRequestSource {
                turn_id: Uuid::new_v4(),
                source_user_message: 0,
                source_request_sha256: "a".repeat(64),
                implementation_requested: false,
            },
        });
        store.save(&mut session).unwrap();
        let mut loaded = store.load(&session.id.to_string()).unwrap();
        assert_eq!(
            loaded.work_state.plan_workflow,
            session.work_state.plan_workflow
        );
        assert!(loaded.work_state.pending_plan_workflow.is_none());
        assert!(loaded
            .work_state
            .pause_agent_graph_after_restore(Utc::now()));
        assert_eq!(
            loaded.work_state.plan_workflow.as_ref().unwrap().status,
            WorkflowStatus::Paused
        );
        assert!(next_dispatch(&loaded.work_state).unwrap().is_none());
        let fork = loaded.work_state.fork_full();
        assert!(fork
            .plan_workflow
            .as_ref()
            .unwrap()
            .implementation_authority
            .is_none());
        assert_ne!(
            fork.plan_workflow.as_ref().unwrap().id,
            loaded.work_state.plan_workflow.as_ref().unwrap().id
        );
        assert!(WorkState::historical_omission().plan_workflow.is_none());
    }

    #[test]
    fn host_implementation_completion_requires_successful_task_evidence() {
        let mut work = WorkState::default();
        start(&mut work, None, true);
        finish(&mut work, proposal("Implement and verify"));
        let dispatch = next_dispatch(&work).unwrap().unwrap();
        mark_dispatched(&mut work, &dispatch, Uuid::new_v4()).unwrap();
        let mut state = crate::app::AppState::new(crate::context::ShellHistory::in_memory());
        *state.work_state.lock().unwrap() = work.clone();
        crate::plan_workflow_runtime::finish_implementation(&mut state, true, "Claimed completion");
        assert_eq!(
            state
                .work_state
                .lock()
                .unwrap()
                .plan_workflow
                .as_ref()
                .unwrap()
                .status,
            WorkflowStatus::Failed
        );

        let task_ids = work
            .plan
            .as_ref()
            .unwrap()
            .tasks
            .iter()
            .map(|task| task.id)
            .collect::<Vec<_>>();
        let event_id = work.record_event(
            crate::work::WorkEventKind::Test,
            "Focused checks passed",
            true,
        );
        for task_id in task_ids {
            work.apply_model_mutation(WorkMutation::RecordEvidence {
                expected_revision: work.revision(),
                task_id,
                event_id,
                note: "Host-recorded check".into(),
            })
            .unwrap();
            work.apply_model_mutation(WorkMutation::UpdateTask {
                expected_revision: work.revision(),
                task_id,
                status: crate::work::TaskStatus::Completed,
                blocked_reason: None,
            })
            .unwrap();
        }
        *state.work_state.lock().unwrap() = work;
        state.plan_workflow_request = None;
        crate::plan_workflow_runtime::finish_implementation(
            &mut state,
            true,
            "Recorded checks passed",
        );
        assert_eq!(
            state
                .work_state
                .lock()
                .unwrap()
                .plan_workflow
                .as_ref()
                .unwrap()
                .status,
            WorkflowStatus::Completed
        );
    }

    #[tokio::test]
    async fn queued_model_request_keeps_original_user_source_and_rejects_replaced_turn() {
        let mut state = crate::app::AppState::new(crate::context::ShellHistory::in_memory());
        let turn_id = Uuid::new_v4();
        state.active_agent_turn_id = Some(turn_id);
        state.messages.push(crate::app::Message {
            role: crate::app::Role::User,
            content: "Create a plan to implement the feature".into(),
            tool_calls: None,
            thinking: None,
            timestamp: Utc::now(),
            response_items: Vec::new(),
        });
        let source = crate::plan_workflow_runtime::source_for_turn(&state).unwrap();
        assert!(!source.implementation_requested);
        let owned = crate::owned_tools::OwnedToolContext {
            plan_workflow_source: Some(source.clone()),
            ..Default::default()
        };
        state.messages.push(crate::app::Message {
            role: crate::app::Role::User,
            content: "Implement another feature now".into(),
            tool_calls: None,
            thinking: None,
            timestamp: Utc::now(),
            response_items: Vec::new(),
        });
        let captured = crate::owned_tools::spawn_owned(owned, async {
            crate::owned_tools::current_plan_workflow_source().unwrap()
        })
        .await
        .unwrap();
        assert_eq!(captured, source);
        assert_ne!(
            captured,
            crate::plan_workflow_runtime::source_for_turn(&state).unwrap()
        );

        state.work_state.lock().unwrap().pending_plan_workflow = Some(PendingPlanWorkflow {
            args: PlanWorkflowArgs::Status,
            source: captured,
        });
        state.active_agent_turn_id = Some(Uuid::new_v4());
        let root = tempfile::tempdir().unwrap();
        let store = crate::session::SessionStore::new(root.path().join("sessions"));
        crate::plan_workflow_runtime::poll(
            &mut state,
            &crate::CaConfig::default(),
            &store,
            root.path(),
        )
        .await;
        assert!(state
            .status_message
            .as_deref()
            .unwrap()
            .contains("interrupted or replaced"));
        assert!(state
            .work_state
            .lock()
            .unwrap()
            .pending_plan_workflow
            .is_none());
        assert!(state.work_state.lock().unwrap().plan_workflow.is_none());
    }

    #[test]
    fn running_task_graph_blocks_new_exchanges_and_inflight_plan_replacement() {
        let mut work = WorkState::default();
        start(&mut work, None, false);
        let dispatch = next_dispatch(&work).unwrap().unwrap();
        let id = Uuid::new_v4();
        mark_dispatched(&mut work, &dispatch, id).unwrap();
        work.agent_graph.status = crate::agent_graph::AgentGraphStatus::Running;
        let before = work.clone();
        assert!(
            complete_stage(&mut work, id, proposal("Concurrent replacement"))
                .unwrap_err()
                .contains("Pause the task graph")
        );
        assert_eq!(work, before);
        work.agent_graph.status = crate::agent_graph::AgentGraphStatus::Idle;
        complete_stage(&mut work, id, proposal("Plan")).unwrap();
        work.plan.as_mut().unwrap().tasks[0].agent_runtime.state =
            crate::agent_graph::AgentTaskState::Running;
        let before = work.clone();
        assert!(apply_action(
            &mut work,
            PlanWorkflowArgs::Handoff {
                step: step(WorkflowRole::Review, "b")
            },
            &route("default"),
            None
        )
        .unwrap_err()
        .contains("Pause the task graph"));
        assert_eq!(work, before);
    }
}
