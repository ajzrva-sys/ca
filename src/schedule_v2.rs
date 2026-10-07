//! Durable V2 scheduling primitives.
//!
//! V2 state intentionally lives outside the V1 `schedules.json` file.  The
//! definition store is a small manager index; execution truth is the immutable,
//! hash-chained run journal.

use base64::Engine as _;
use chrono::{DateTime, LocalResult, NaiveDateTime, TimeDelta, TimeZone, Utc};
use chrono_tz::Tz;
use cron::Schedule;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::str::FromStr;
use std::sync::OnceLock;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use uuid::Uuid;

const DEFINITION_VERSION: u32 = 2;
const STORE_VERSION: u32 = 2;
const JOURNAL_VERSION: u32 = 2;
const LEASE_VERSION: u32 = 1;
const MAX_DEFINITION_BYTES: u64 = 256 * 1024;
const MAX_STORE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_EVENT_BYTES: u64 = 128 * 1024;
const MAX_SCHEDULES: usize = 256;
const MAX_OUTPUT_BYTES: u64 = 16 * 1024 * 1024;
const MAX_RUNTIME_SECS: u64 = 86_400;
const MAX_LEASES: usize = 2;
const MAX_OUTCOME_BYTES: usize = 4096;

#[derive(Debug, thiserror::Error)]
pub enum ScheduleV2Error {
    #[error("invalid V2 schedule definition: {0}")]
    InvalidDefinition(String),
    #[error("unsupported schedule definition version: {0}")]
    UnsupportedVersion(u32),
    #[error("exact confirmation digest required: {0}")]
    ConfirmationRequired(String),
    #[error("schedule already exists: {0}")]
    Duplicate(String),
    #[error("schedule was not found: {0}")]
    Missing(String),
    #[error("schedule state is busy")]
    Busy,
    #[error("unsafe or corrupt schedule state: {0}")]
    State(String),
    #[error("schedule execution failed: {0}")]
    Execution(String),
    #[error("schedule I/O failed: {0}")]
    Io(String),
}

impl From<std::io::Error> for ScheduleV2Error {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ScheduleDefinitionV2 {
    #[serde(default = "definition_version")]
    pub version: u32,
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub workspace: PathBuf,
    #[serde(default = "default_runtime_secs")]
    pub runtime_secs: u64,
    #[serde(default = "default_output_bytes")]
    pub output_bytes: u64,
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default)]
    pub grants: Vec<crate::permission::PermissionMatcher>,
    #[serde(default)]
    pub allow_network: bool,
    #[serde(default)]
    pub skills: Vec<ScheduleSkillPin>,
    pub workspace_policy: WorkspacePolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pinned_revision: Option<String>,
    pub trigger: ScheduleTriggerV2,
    #[serde(default)]
    pub missed_run_policy: MissedRunPolicy,
    pub target: ScheduleTargetV2,
    pub delivery: ScheduleDeliveryV2,
}

fn definition_version() -> u32 {
    DEFINITION_VERSION
}

fn default_runtime_secs() -> u64 {
    300
}

fn default_output_bytes() -> u64 {
    1024 * 1024
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct ScheduleSkillPin {
    pub instance_id: String,
    pub full_digest: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkspacePolicy {
    ExistingReadOnly,
    IsolatedPinned,
    /// The repository target owns its per-occurrence isolated worktree.
    ManagedRepository,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ScheduleTriggerV2 {
    Absolute {
        at: DateTime<Utc>,
    },
    Delay {
        seconds: u64,
    },
    Interval {
        seconds: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        anchor: Option<DateTime<Utc>>,
    },
    Cron {
        expression: String,
        timezone: String,
    },
}

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MissedRunPolicy {
    #[default]
    Skip,
    OneCatchUp,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ScheduleTargetV2 {
    Prompt {
        prompt: String,
    },
    Workflow {
        path: String,
        workflow_id: String,
        digest: String,
    },
    ScriptOnly {
        executable: PathBuf,
        #[serde(default)]
        args: Vec<String>,
        executable_sha256: String,
        #[serde(default = "true_value")]
        scrub_environment: bool,
    },
    Mechanistic {
        actions: Vec<codex_team_protocol::MechanisticActionV1>,
    },
    AutomationWorkflow {
        workflow_id: String,
        workflow_digest: String,
        resource_binding_digest: String,
    },
    RepositoryTask {
        task: ca_automation::repository::RepositoryTask,
    },
    HarnessDiscovery {
        vault_root: PathBuf,
    },
    HarnessEvaluation {
        vault_root: PathBuf,
        cycle_id: String,
        expected_plan_digest: String,
        repository: PathBuf,
        baseline_binary: PathBuf,
    },
    HarnessCycle {
        vault_root: PathBuf,
    },
    Monitor {
        request: ca_watch::WatchRequestV1,
    },
}

impl ScheduleTargetV2 {
    pub fn native(&self) -> bool {
        matches!(
            self,
            Self::Mechanistic { .. }
                | Self::AutomationWorkflow { .. }
                | Self::RepositoryTask { .. }
                | Self::HarnessDiscovery { .. }
                | Self::HarnessEvaluation { .. }
                | Self::HarnessCycle { .. }
                | Self::Monitor { .. }
        )
    }
}

fn true_value() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ScheduleDeliveryV2 {
    None,
    LocalFile {
        path: String,
    },
    Webhook {
        url: String,
        secret_env: String,
    },
    EmailDraft {
        account: String,
        recipients: Vec<String>,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleRunState {
    Claimed,
    Running,
    Delivering,
    Succeeded,
    Failed,
    TimedOut,
    Interrupted,
}

impl ScheduleRunState {
    pub(crate) fn terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::TimedOut | Self::Interrupted
        )
    }

    fn may_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Claimed, Self::Running)
                | (Self::Claimed, Self::Interrupted)
                | (Self::Running, Self::Running)
                | (Self::Running, Self::Delivering)
                | (Self::Running, Self::Succeeded)
                | (Self::Running, Self::Failed)
                | (Self::Running, Self::TimedOut)
                | (Self::Running, Self::Interrupted)
                | (Self::Delivering, Self::Succeeded)
                | (Self::Delivering, Self::Failed)
                | (Self::Delivering, Self::Interrupted)
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Occurrence {
    pub id: String,
    pub at: DateTime<Utc>,
}

pub fn definition_digest(definition: &ScheduleDefinitionV2) -> Result<String, ScheduleV2Error> {
    let bytes = serde_json::to_vec(definition)
        .map_err(|error| ScheduleV2Error::InvalidDefinition(error.to_string()))?;
    Ok(hex_digest(&Sha256::digest(bytes)))
}

pub fn normalize_at_confirmation(
    mut definition: ScheduleDefinitionV2,
    now: DateTime<Utc>,
) -> Result<ScheduleDefinitionV2, ScheduleV2Error> {
    if definition.version != DEFINITION_VERSION {
        return Err(ScheduleV2Error::UnsupportedVersion(definition.version));
    }
    validate_id(&definition.id)?;
    if !(1..=MAX_RUNTIME_SECS).contains(&definition.runtime_secs)
        || !(1..=MAX_OUTPUT_BYTES).contains(&definition.output_bytes)
    {
        return Err(ScheduleV2Error::InvalidDefinition(
            "runtime/output limits are outside the supported bounds".into(),
        ));
    }
    let workspace = definition.workspace.canonicalize().map_err(|_| {
        ScheduleV2Error::InvalidDefinition("workspace must be an existing directory".into())
    })?;
    if !workspace.is_dir() {
        return Err(ScheduleV2Error::InvalidDefinition(
            "workspace must be an existing directory".into(),
        ));
    }
    definition.workspace = workspace;

    definition.trigger = match definition.trigger {
        ScheduleTriggerV2::Delay { seconds } => {
            let seconds = i64::try_from(seconds)
                .map_err(|_| ScheduleV2Error::InvalidDefinition("delay is too large".into()))?;
            let at = now
                .checked_add_signed(TimeDelta::seconds(seconds))
                .ok_or_else(|| {
                    ScheduleV2Error::InvalidDefinition("delay overflows UTC time".into())
                })?;
            ScheduleTriggerV2::Absolute { at }
        }
        ScheduleTriggerV2::Interval { seconds, anchor } => {
            if seconds == 0 || seconds > MAX_RUNTIME_SECS * 366 {
                return Err(ScheduleV2Error::InvalidDefinition(
                    "interval is outside the supported bounds".into(),
                ));
            }
            ScheduleTriggerV2::Interval {
                seconds,
                anchor: Some(anchor.unwrap_or(now)),
            }
        }
        trigger => trigger,
    };
    validate_trigger(&definition.trigger)?;
    validate_skill_pins(&definition.skills)?;
    validate_tools_and_grants(&definition)?;
    validate_delivery(&definition.delivery)?;
    validate_target_and_policy(&mut definition)?;
    Ok(definition)
}

fn validate_tools_and_grants(definition: &ScheduleDefinitionV2) -> Result<(), ScheduleV2Error> {
    if definition.tools.len() > 64 || definition.grants.len() > 64 {
        return Err(ScheduleV2Error::InvalidDefinition(
            "too many tools or grants".into(),
        ));
    }
    let registry = crate::agent::registry::ToolRegistry::builtins();
    let mut seen = BTreeSet::new();
    for tool in &definition.tools {
        let spec = registry
            .get(tool)
            .ok_or_else(|| ScheduleV2Error::InvalidDefinition(format!("unknown tool {tool}")))?;
        if !seen.insert(tool)
            || spec.capabilities.iter().any(|capability| {
                matches!(
                    capability,
                    crate::agent::registry::ToolCapability::Interaction
                        | crate::agent::registry::ToolCapability::Subagent
                        | crate::agent::registry::ToolCapability::Privilege
                )
            })
            || matches!(
                spec.handler,
                crate::agent::registry::ToolHandler::WatchProcess
                    | crate::agent::registry::ToolHandler::StopWatcher
            )
            || (spec
                .capabilities
                .contains(&crate::agent::registry::ToolCapability::Network)
                && !definition.allow_network)
        {
            return Err(ScheduleV2Error::InvalidDefinition(format!(
                "unavailable or unsafe tool {tool}"
            )));
        }
    }
    for grant in &definition.grants {
        grant
            .validate()
            .map_err(|error| ScheduleV2Error::InvalidDefinition(error.to_string()))?;
        let tool = match grant {
            crate::permission::PermissionMatcher::Tool { tool }
            | crate::permission::PermissionMatcher::Path { tool, .. }
            | crate::permission::PermissionMatcher::Invocation { tool, .. } => tool.as_str(),
            crate::permission::PermissionMatcher::CommandPrefix { .. } => "run_command",
        };
        if !definition.tools.iter().any(|candidate| candidate == tool) {
            return Err(ScheduleV2Error::InvalidDefinition(format!(
                "grant for {tool} is outside the schedule tool set"
            )));
        }
    }
    Ok(())
}

fn validate_target_and_policy(
    definition: &mut ScheduleDefinitionV2,
) -> Result<(), ScheduleV2Error> {
    let has_route = definition
        .provider
        .as_deref()
        .zip(definition.model.as_deref())
        .is_some_and(|(provider, model)| valid_atom(provider) && valid_atom(model));
    match &mut definition.target {
        ScheduleTargetV2::Prompt { prompt } => {
            *prompt = prompt.trim().to_string();
            if prompt.is_empty() || prompt.len() > 64 * 1024 || !has_route {
                return Err(ScheduleV2Error::InvalidDefinition(
                    "prompt targets require a bounded prompt and exact provider/model".into(),
                ));
            }
        }
        ScheduleTargetV2::Workflow {
            path,
            workflow_id,
            digest,
        } => {
            if !has_route
                || !valid_relative(path)
                || !valid_atom(workflow_id)
                || !is_sha256(digest)
                || definition.workspace_policy != WorkspacePolicy::IsolatedPinned
            {
                return Err(ScheduleV2Error::InvalidDefinition(
                    "workflow targets require an isolated pinned workspace and exact identity"
                        .into(),
                ));
            }
        }
        ScheduleTargetV2::ScriptOnly {
            executable,
            args,
            executable_sha256,
            scrub_environment,
        } => {
            if definition.provider.is_some()
                || definition.model.is_some()
                || !definition.tools.is_empty()
                || !definition.skills.is_empty()
            {
                return Err(ScheduleV2Error::InvalidDefinition(
                    "script-only targets cannot select a provider, model, or provider tools".into(),
                ));
            }
            if definition.workspace_policy != WorkspacePolicy::IsolatedPinned
                || !*scrub_environment
                || args.len() > 256
                || args
                    .iter()
                    .any(|arg| arg.len() > 16 * 1024 || arg.contains('\0'))
            {
                return Err(ScheduleV2Error::InvalidDefinition(
                    "script-only targets require isolated pinned execution and scrubbed exact argv"
                        .into(),
                ));
            }
            let canonical = executable.canonicalize().map_err(|_| {
                ScheduleV2Error::InvalidDefinition("script executable does not exist".into())
            })?;
            if !canonical.is_absolute() || !canonical.is_file() || &canonical != executable {
                return Err(ScheduleV2Error::InvalidDefinition(
                    "script executable must be an absolute canonical regular file".into(),
                ));
            }
            if hash_file(&canonical, 128 * 1024 * 1024)? != *executable_sha256 {
                return Err(ScheduleV2Error::InvalidDefinition(
                    "script executable digest does not match".into(),
                ));
            }
        }
        ScheduleTargetV2::Mechanistic { actions } => {
            if definition.provider.is_some()
                || definition.model.is_some()
                || !definition.tools.is_empty()
                || !definition.grants.is_empty()
                || !definition.skills.is_empty()
                || definition.allow_network
            {
                return Err(ScheduleV2Error::InvalidDefinition(
                    "mechanistic targets cannot select a provider, model, provider tools, grants, skills, or network"
                        .into(),
                ));
            }
            codex_team_protocol::validate_actions_structure(actions)
                .map_err(|error| ScheduleV2Error::InvalidDefinition(error.to_string()))?;
            if actions.is_empty()
                || actions
                    .iter()
                    .any(|action| !action.is_query() && !action.is_evidence())
            {
                return Err(ScheduleV2Error::InvalidDefinition(
                    "mechanistic schedules require bounded queries or evidence; observation is not persisted"
                        .into(),
                ));
            }
        }
        ScheduleTargetV2::AutomationWorkflow {
            workflow_id,
            workflow_digest,
            resource_binding_digest,
        } => {
            if definition.provider.is_some()
                || definition.model.is_some()
                || !definition.tools.is_empty()
                || !definition.grants.is_empty()
                || !definition.skills.is_empty()
                || !definition.allow_network
            {
                return Err(ScheduleV2Error::InvalidDefinition(
                    "automation workflow targets require explicit network authority and cannot select a provider, model, provider tools, grants, or skills"
                        .into(),
                ));
            }
            if workflow_id.is_empty()
                || !workflow_id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
                || !is_lower_sha256(workflow_digest)
                || !is_lower_sha256(resource_binding_digest)
            {
                return Err(ScheduleV2Error::InvalidDefinition(
                    "automation workflow target identifiers are invalid".into(),
                ));
            }
        }
        ScheduleTargetV2::RepositoryTask { task } => {
            task.validate()
                .map_err(|e| ScheduleV2Error::InvalidDefinition(e.to_string()))?;
            if definition.workspace_policy != WorkspacePolicy::ManagedRepository
                || definition.workspace != task.repository
                || definition.provider.as_deref() != Some(&task.worker_route.provider)
                || definition.model.as_deref() != Some(&task.worker_route.model)
            {
                return Err(ScheduleV2Error::InvalidDefinition("repository tasks require their named workspace, worker route and managed_repository isolation".into()));
            }
        }
        ScheduleTargetV2::HarnessDiscovery { vault_root } => {
            if !vault_root.is_absolute()
                || definition.provider.is_some()
                || definition.model.is_some()
                || !definition.tools.is_empty()
                || !definition.grants.is_empty()
                || definition.allow_network
            {
                return Err(ScheduleV2Error::InvalidDefinition(
                    "harness discovery requires an absolute vault and model-free authority".into(),
                ));
            }
        }
        ScheduleTargetV2::HarnessEvaluation {
            vault_root,
            cycle_id,
            expected_plan_digest,
            repository,
            baseline_binary,
        } => {
            if !vault_root.is_absolute()
                || !repository.is_absolute()
                || repository.canonicalize().ok().as_ref() != Some(&definition.workspace)
                || !baseline_binary.is_absolute()
                || !ca_automation_protocol::execution::valid_id(cycle_id)
                || !is_lower_sha256(expected_plan_digest)
                || definition.provider.is_none()
                || definition.model.is_none()
                || !definition.allow_network
                || !definition.tools.is_empty()
                || !definition.grants.is_empty()
            {
                return Err(ScheduleV2Error::InvalidDefinition("harness evaluation requires exact plan/resource bindings and its frozen provider route".into()));
            }
        }
        ScheduleTargetV2::HarnessCycle { vault_root } => {
            if !vault_root.is_absolute()
                || definition.provider.is_none()
                || definition.model.is_none()
                || !definition.allow_network
                || !definition.tools.is_empty()
                || !definition.grants.is_empty()
            {
                return Err(ScheduleV2Error::InvalidDefinition(
                    "harness cycle requires an absolute vault and authorized exact provider route"
                        .into(),
                ));
            }
        }
        ScheduleTargetV2::Monitor { request } => {
            request
                .validate()
                .map_err(|e| ScheduleV2Error::InvalidDefinition(e.to_string()))?;
            if definition.provider.is_some()
                || definition.model.is_some()
                || !definition.tools.is_empty()
                || !definition.grants.is_empty()
                || (request.host.is_remote() && !definition.allow_network)
            {
                return Err(ScheduleV2Error::InvalidDefinition(
                    "monitors are model-free and remote probes require network authority".into(),
                ));
            }
        }
    }
    if definition.workspace_policy == WorkspacePolicy::ManagedRepository
        && !matches!(definition.target, ScheduleTargetV2::RepositoryTask { .. })
    {
        return Err(ScheduleV2Error::InvalidDefinition(
            "managed_repository is reserved for repository tasks".into(),
        ));
    }
    if definition.workspace_policy == WorkspacePolicy::IsolatedPinned {
        let revision = definition.pinned_revision.as_deref().ok_or_else(|| {
            ScheduleV2Error::InvalidDefinition(
                "isolated workspaces require a pinned revision".into(),
            )
        })?;
        if !is_git_oid(revision) {
            return Err(ScheduleV2Error::InvalidDefinition(
                "pinned revision must be an exact 40 or 64 digit Git object ID".into(),
            ));
        }
    } else if definition.pinned_revision.is_some() {
        return Err(ScheduleV2Error::InvalidDefinition(
            "read-only existing workspaces cannot carry a pinned revision".into(),
        ));
    }
    if definition.workspace_policy == WorkspacePolicy::ExistingReadOnly {
        let registry = crate::agent::registry::ToolRegistry::builtins();
        for tool in &definition.tools {
            let spec = registry.get(tool).ok_or_else(|| {
                ScheduleV2Error::InvalidDefinition(format!("unknown tool {tool}"))
            })?;
            if spec.mutation == crate::agent::registry::ToolMutation::Mutating {
                return Err(ScheduleV2Error::InvalidDefinition(format!(
                    "mutating tool {tool} requires an isolated pinned workspace"
                )));
            }
        }
    }
    Ok(())
}

fn is_lower_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn validate_trigger(trigger: &ScheduleTriggerV2) -> Result<(), ScheduleV2Error> {
    match trigger {
        ScheduleTriggerV2::Absolute { .. } => Ok(()),
        ScheduleTriggerV2::Delay { .. } => Err(ScheduleV2Error::InvalidDefinition(
            "delay must be normalized during confirmation".into(),
        )),
        ScheduleTriggerV2::Interval { seconds, anchor } => {
            if *seconds == 0 || anchor.is_none() {
                Err(ScheduleV2Error::InvalidDefinition(
                    "interval must have a positive duration and durable anchor".into(),
                ))
            } else {
                Ok(())
            }
        }
        ScheduleTriggerV2::Cron {
            expression,
            timezone,
        } => {
            if expression.split_whitespace().count() != 5 {
                return Err(ScheduleV2Error::InvalidDefinition(
                    "cron requires exactly five fields".into(),
                ));
            }
            let _: Tz = timezone.parse().map_err(|_| {
                ScheduleV2Error::InvalidDefinition("cron requires an IANA time zone".into())
            })?;
            Schedule::from_str(&format!("0 {expression}")).map_err(|_| {
                ScheduleV2Error::InvalidDefinition("invalid cron expression".into())
            })?;
            Ok(())
        }
    }
}

pub fn next_occurrence(
    trigger: &ScheduleTriggerV2,
    after: DateTime<Utc>,
) -> Result<Option<Occurrence>, ScheduleV2Error> {
    validate_trigger(trigger)?;
    let at = match trigger {
        ScheduleTriggerV2::Absolute { at } => (*at > after).then_some(*at),
        ScheduleTriggerV2::Delay { .. } => unreachable!("validated above"),
        ScheduleTriggerV2::Interval { seconds, anchor } => {
            let anchor = anchor.expect("validated interval anchor");
            if after < anchor {
                Some(anchor)
            } else {
                let elapsed = (after - anchor).num_seconds();
                let interval = i64::try_from(*seconds).map_err(|_| {
                    ScheduleV2Error::InvalidDefinition("interval is too large".into())
                })?;
                let count = elapsed
                    .checked_div(interval)
                    .and_then(|value| value.checked_add(1))
                    .ok_or_else(|| ScheduleV2Error::State("interval arithmetic overflow".into()))?;
                anchor.checked_add_signed(TimeDelta::seconds(
                    interval.checked_mul(count).ok_or_else(|| {
                        ScheduleV2Error::State("interval arithmetic overflow".into())
                    })?,
                ))
            }
        }
        ScheduleTriggerV2::Cron {
            expression,
            timezone,
        } => next_cron(expression, timezone, after)?,
    };
    Ok(at.map(|at| Occurrence {
        id: occurrence_id(trigger, at),
        at,
    }))
}

/// Resolves one due occurrence without replaying an unbounded backlog.
pub fn due_occurrence(
    trigger: &ScheduleTriggerV2,
    durable_next: &Occurrence,
    now: DateTime<Utc>,
    policy: MissedRunPolicy,
) -> Result<Option<Occurrence>, ScheduleV2Error> {
    if durable_next.at > now {
        return Ok(None);
    }
    if durable_next.at == now {
        return Ok(Some(durable_next.clone()));
    }
    if policy == MissedRunPolicy::OneCatchUp {
        return Ok(Some(durable_next.clone()));
    }
    let _ = trigger;
    Ok(None)
}

fn next_cron(
    expression: &str,
    timezone: &str,
    after: DateTime<Utc>,
) -> Result<Option<DateTime<Utc>>, ScheduleV2Error> {
    let timezone: Tz = timezone.parse().map_err(|_| {
        ScheduleV2Error::InvalidDefinition("cron requires an IANA time zone".into())
    })?;
    let schedule = Schedule::from_str(&format!("0 {expression}"))
        .map_err(|_| ScheduleV2Error::InvalidDefinition("invalid cron expression".into()))?;
    // The cron crate iterates UTC instants. Treat those fields as a local wall
    // clock, then resolve explicitly through chrono-tz so gaps and folds have
    // the frozen SCH-099 semantics.
    let local_after = after.with_timezone(&timezone).naive_local();
    let pseudo_after = DateTime::<Utc>::from_naive_utc_and_offset(local_after, Utc);
    for pseudo in schedule.after(&pseudo_after).take(10_000) {
        let local: NaiveDateTime = pseudo.naive_utc();
        let candidate = match timezone.from_local_datetime(&local) {
            LocalResult::None => continue,
            LocalResult::Single(value) => value.with_timezone(&Utc),
            LocalResult::Ambiguous(first, second) => {
                first.with_timezone(&Utc).min(second.with_timezone(&Utc))
            }
        };
        if candidate > after {
            return Ok(Some(candidate));
        }
    }
    Err(ScheduleV2Error::InvalidDefinition(
        "cron has no bounded future occurrence".into(),
    ))
}

fn occurrence_id(trigger: &ScheduleTriggerV2, at: DateTime<Utc>) -> String {
    let mut hasher = Sha256::new();
    hasher.update(serde_json::to_vec(trigger).expect("trigger serializes"));
    hasher.update([0]);
    hasher.update(at.to_rfc3339_opts(chrono::SecondsFormat::Nanos, true));
    hex_digest(&hasher.finalize())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ScheduleRecordV2 {
    pub definition: ScheduleDefinitionV2,
    pub definition_digest: String,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_occurrence: Option<Occurrence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_run_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claim_revision: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScheduleStoreDataV2 {
    version: u32,
    revision: u64,
    records: Vec<ScheduleRecordV2>,
}

impl Default for ScheduleStoreDataV2 {
    fn default() -> Self {
        Self {
            version: STORE_VERSION,
            revision: 0,
            records: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ScheduleStoreV2 {
    dir: PathBuf,
    path: PathBuf,
}

impl ScheduleStoreV2 {
    pub fn new(schedule_dir: PathBuf) -> Self {
        let schedule_dir = normalize_missing_path(&schedule_dir).unwrap_or(schedule_dir);
        Self {
            path: schedule_dir.join("schedules-v2.json"),
            dir: schedule_dir,
        }
    }

    fn load(&self) -> Result<ScheduleStoreDataV2, ScheduleV2Error> {
        if !self.path.exists() {
            return Ok(ScheduleStoreDataV2::default());
        }
        let bytes = read_owner_file(&self.path, MAX_STORE_BYTES)?;
        let data: ScheduleStoreDataV2 = serde_json::from_slice(&bytes)
            .map_err(|error| ScheduleV2Error::State(error.to_string()))?;
        if data.version != STORE_VERSION || data.records.len() > MAX_SCHEDULES {
            return Err(ScheduleV2Error::State(
                "unsupported or oversized V2 schedule store".into(),
            ));
        }
        for record in &data.records {
            if definition_digest(&record.definition)? != record.definition_digest {
                return Err(ScheduleV2Error::State(format!(
                    "definition digest mismatch for {}",
                    record.definition.id
                )));
            }
        }
        Ok(data)
    }

    fn save(&self, data: &mut ScheduleStoreDataV2) -> Result<(), ScheduleV2Error> {
        data.revision = data
            .revision
            .checked_add(1)
            .ok_or_else(|| ScheduleV2Error::State("schedule revision overflow".into()))?;
        let bytes = serde_json::to_vec_pretty(data)
            .map_err(|error| ScheduleV2Error::State(error.to_string()))?;
        if bytes.len() as u64 > MAX_STORE_BYTES {
            return Err(ScheduleV2Error::State(
                "V2 schedule store is too large".into(),
            ));
        }
        atomic_owner_write(&self.dir, &self.path, &bytes, true)
    }

    pub fn records(&self) -> Result<Vec<ScheduleRecordV2>, ScheduleV2Error> {
        Ok(self.load()?.records)
    }

    pub fn add_confirmed(
        &self,
        definition: ScheduleDefinitionV2,
        expected_digest: &str,
        now: DateTime<Utc>,
    ) -> Result<ScheduleRecordV2, ScheduleV2Error> {
        let definition = normalize_at_confirmation(definition, now)?;
        let digest = definition_digest(&definition)?;
        if !constant_time_eq(digest.as_bytes(), expected_digest.as_bytes()) {
            return Err(ScheduleV2Error::ConfirmationRequired(digest));
        }
        let _lock = OwnerLock::acquire(self.dir.join("schedules-v2.lock"))?;
        let mut data = self.load()?;
        if data
            .records
            .iter()
            .any(|record| record.definition.id == definition.id)
        {
            return Err(ScheduleV2Error::Duplicate(definition.id));
        }
        if data.records.len() >= MAX_SCHEDULES {
            return Err(ScheduleV2Error::State("schedule limit reached".into()));
        }
        let record = ScheduleRecordV2 {
            definition,
            definition_digest: digest,
            enabled: false,
            next_occurrence: None,
            active_run_id: None,
            claim_revision: None,
        };
        data.records.push(record.clone());
        data.records
            .sort_by(|left, right| left.definition.id.cmp(&right.definition.id));
        self.save(&mut data)?;
        Ok(record)
    }

    pub fn update_confirmed(
        &self,
        id: &str,
        definition: ScheduleDefinitionV2,
        expected_digest: &str,
        now: DateTime<Utc>,
    ) -> Result<ScheduleRecordV2, ScheduleV2Error> {
        if definition.id != id {
            return Err(ScheduleV2Error::InvalidDefinition(
                "update cannot change a schedule ID".into(),
            ));
        }
        let definition = normalize_at_confirmation(definition, now)?;
        let digest = definition_digest(&definition)?;
        if !constant_time_eq(digest.as_bytes(), expected_digest.as_bytes()) {
            return Err(ScheduleV2Error::ConfirmationRequired(digest));
        }
        let _lock = OwnerLock::acquire(self.dir.join("schedules-v2.lock"))?;
        let mut data = self.load()?;
        let record = data
            .records
            .iter_mut()
            .find(|record| record.definition.id == id)
            .ok_or_else(|| ScheduleV2Error::Missing(id.into()))?;
        if record.active_run_id.is_some() {
            return Err(ScheduleV2Error::State(
                "cannot update a schedule with an active run".into(),
            ));
        }
        record.definition = definition;
        record.definition_digest = digest;
        record.enabled = false;
        record.next_occurrence = None;
        let updated = record.clone();
        self.save(&mut data)?;
        Ok(updated)
    }

    pub fn set_enabled(
        &self,
        id: &str,
        enabled: bool,
        now: DateTime<Utc>,
    ) -> Result<(), ScheduleV2Error> {
        validate_id(id)?;
        let _lock = OwnerLock::acquire(self.dir.join("schedules-v2.lock"))?;
        let mut data = self.load()?;
        let record = data
            .records
            .iter_mut()
            .find(|record| record.definition.id == id)
            .ok_or_else(|| ScheduleV2Error::Missing(id.into()))?;
        if record.active_run_id.is_some() {
            return Err(ScheduleV2Error::State("schedule has an active run".into()));
        }
        record.enabled = enabled;
        record.next_occurrence = if enabled {
            next_occurrence(&record.definition.trigger, now)?
        } else {
            None
        };
        self.save(&mut data)
    }

    pub fn claim_due(
        &self,
        profile: &str,
        journal: &RunJournal,
        now: DateTime<Utc>,
    ) -> Result<Option<RunClaim>, ScheduleV2Error> {
        let _lock = OwnerLock::acquire(self.dir.join("schedules-v2.lock"))?;
        let mut data = self.load()?;
        loop {
            let candidate = data
                .records
                .iter()
                .enumerate()
                .filter(|(_, record)| {
                    record.enabled
                        && record.active_run_id.is_none()
                        && record
                            .next_occurrence
                            .as_ref()
                            .is_some_and(|item| item.at <= now)
                })
                .min_by_key(|(_, record)| {
                    (
                        record.next_occurrence.as_ref().map(|value| value.at),
                        &record.definition.id,
                    )
                })
                .map(|(index, _)| index);
            let Some(index) = candidate else {
                return Ok(None);
            };
            let record = data.records[index].clone();
            let occurrence = due_occurrence(
                &record.definition.trigger,
                record.next_occurrence.as_ref().expect("due occurrence"),
                now,
                record.definition.missed_run_policy,
            )?;
            let Some(occurrence) = occurrence else {
                data.records[index].next_occurrence =
                    next_occurrence(&record.definition.trigger, now)?;
                if matches!(
                    record.definition.trigger,
                    ScheduleTriggerV2::Absolute { .. }
                ) {
                    data.records[index].enabled = false;
                }
                self.save(&mut data)?;
                continue;
            };
            let run_id = stable_run_id(&record.definition_digest, &occurrence.id);
            let claim_revision = data
                .revision
                .checked_add(1)
                .ok_or_else(|| ScheduleV2Error::State("claim revision overflow".into()))?;
            let claim = RunClaim {
                run_id,
                profile: profile.into(),
                schedule_id: record.definition.id.clone(),
                definition_digest: record.definition_digest.clone(),
                occurrence: occurrence.clone(),
                claim_revision,
                definition: record.definition.clone(),
            };
            journal.create_claim(&claim, now)?;
            data.records[index].active_run_id = Some(run_id);
            data.records[index].claim_revision = Some(claim_revision);
            data.records[index].next_occurrence =
                next_occurrence(&record.definition.trigger, occurrence.at)?;
            if matches!(
                record.definition.trigger,
                ScheduleTriggerV2::Absolute { .. }
            ) {
                data.records[index].enabled = false;
            }
            self.save(&mut data)?;
            return Ok(Some(claim));
        }
    }

    pub fn claim_immediate(
        &self,
        profile: &str,
        journal: &RunJournal,
        id: &str,
        now: DateTime<Utc>,
    ) -> Result<RunClaim, ScheduleV2Error> {
        let _lock = OwnerLock::acquire(self.dir.join("schedules-v2.lock"))?;
        let mut data = self.load()?;
        let index = data
            .records
            .iter()
            .position(|record| record.definition.id == id)
            .ok_or_else(|| ScheduleV2Error::Missing(id.into()))?;
        let record = data.records[index].clone();
        if record.active_run_id.is_some() {
            return Err(ScheduleV2Error::State(
                "schedule already has an active run".into(),
            ));
        }
        let occurrence = Occurrence {
            id: occurrence_id(&record.definition.trigger, now),
            at: now,
        };
        let run_id = Uuid::new_v4();
        let claim_revision = data
            .revision
            .checked_add(1)
            .ok_or_else(|| ScheduleV2Error::State("claim revision overflow".into()))?;
        let claim = RunClaim {
            run_id,
            profile: profile.into(),
            schedule_id: id.into(),
            definition_digest: record.definition_digest,
            occurrence,
            claim_revision,
            definition: record.definition,
        };
        journal.create_claim(&claim, now)?;
        data.records[index].active_run_id = Some(run_id);
        data.records[index].claim_revision = Some(claim_revision);
        self.save(&mut data)?;
        Ok(claim)
    }

    pub fn settle(&self, claim: &RunClaim) -> Result<(), ScheduleV2Error> {
        let _lock = OwnerLock::acquire(self.dir.join("schedules-v2.lock"))?;
        let mut data = self.load()?;
        let record = data
            .records
            .iter_mut()
            .find(|record| record.definition.id == claim.schedule_id)
            .ok_or_else(|| ScheduleV2Error::Missing(claim.schedule_id.clone()))?;
        if record.definition_digest != claim.definition_digest
            || record.active_run_id != Some(claim.run_id)
            || record.claim_revision != Some(claim.claim_revision)
        {
            return Err(ScheduleV2Error::State(
                "claim settlement compare-and-swap failed".into(),
            ));
        }
        record.active_run_id = None;
        record.claim_revision = None;
        self.save(&mut data)
    }

    /// Reconciles only runs whose recorded owner and native executor are gone.
    /// A contained child can outlive its dispatching parent; its OS lease keeps
    /// the exact occurrence claim active without a time-based expiry.
    pub fn recover_dead_owners(
        &self,
        journal: &RunJournal,
        now: DateTime<Utc>,
    ) -> Result<usize, ScheduleV2Error> {
        let _lock = OwnerLock::acquire(self.dir.join("schedules-v2.lock"))?;
        let mut data = self.load()?;
        let mut recovered = 0_usize;
        for record in &mut data.records {
            let Some(run_id) = record.active_run_id else {
                continue;
            };
            let latest = journal.latest(run_id)?.ok_or_else(|| {
                ScheduleV2Error::State(format!("active run {run_id} has no journal"))
            })?;
            if latest.schedule_id != record.definition.id
                || latest.definition_digest != record.definition_digest
                || record.claim_revision != Some(latest.claim_revision)
            {
                return Err(ScheduleV2Error::State(format!(
                    "active run {run_id} does not match its definition"
                )));
            }
            if self.has_live_native_executor(&latest)? {
                continue;
            }
            if latest.state.terminal() {
                record.active_run_id = None;
                record.claim_revision = None;
                recovered = recovered.saturating_add(1);
            } else if !process_is_alive(latest.owner_pid)? {
                journal.transition(
                    run_id,
                    ScheduleRunState::Interrupted,
                    ReceiptUpdate {
                        phase: "owner_dead_recovery".into(),
                        outcome: Some(
                            "recorded owner is dead; execution was not replayed and retained evidence requires inspection"
                                .into(),
                        ),
                        ..Default::default()
                    },
                    now,
                )?;
                record.active_run_id = None;
                record.claim_revision = None;
                recovered = recovered.saturating_add(1);
            }
        }
        if recovered > 0 {
            self.save(&mut data)?;
        }
        Ok(recovered)
    }

    pub(crate) fn has_live_native_executor(
        &self,
        receipt: &RunReceipt,
    ) -> Result<bool, ScheduleV2Error> {
        native_executor_is_alive(&self.dir, receipt)
    }
}

fn native_executor_is_alive(
    schedule_dir: &Path,
    receipt: &RunReceipt,
) -> Result<bool, ScheduleV2Error> {
    let profile_root = schedule_dir.parent().ok_or_else(|| {
        ScheduleV2Error::State("schedule directory has no profile storage owner".into())
    })?;
    let native_root =
        crate::automation_runtime::storage_path(&profile_root.join("automation-engine/v2"));
    let run_id = receipt.run_id.to_string();
    let context_path = native_root.join("runs").join(&run_id).join("context.json");
    match std::fs::symlink_metadata(&context_path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {}
        Ok(_) => {
            return Err(ScheduleV2Error::State(
                "native run context is not a regular owned file".into(),
            ))
        }
    }
    let context =
        ca_automation::runtime::RunStore::load_context(&native_root, &run_id).map_err(|error| {
            ScheduleV2Error::State(format!(
                "native executor ownership could not be inspected: {error}"
            ))
        })?;
    if context.profile != receipt.profile {
        return Err(ScheduleV2Error::State(
            "native executor profile does not match the occurrence claim".into(),
        ));
    }
    let store = ca_automation::runtime::RunStore::open(&context).map_err(|error| {
        ScheduleV2Error::State(format!(
            "native executor ownership could not be inspected: {error}"
        ))
    })?;
    match store.execution_lease() {
        Ok(_lease) => Ok(false),
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|error| error.kind() == std::io::ErrorKind::WouldBlock) =>
        {
            Ok(true)
        }
        Err(error) => Err(ScheduleV2Error::State(format!(
            "native executor ownership could not be inspected: {error}"
        ))),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunClaim {
    pub run_id: Uuid,
    pub profile: String,
    pub schedule_id: String,
    pub definition_digest: String,
    pub occurrence: Occurrence,
    pub claim_revision: u64,
    pub definition: ScheduleDefinitionV2,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RunReceipt {
    pub version: u32,
    pub sequence: u64,
    pub profile: String,
    pub schedule_id: String,
    pub definition_digest: String,
    pub run_id: Uuid,
    pub occurrence_id: String,
    pub occurrence_at: DateTime<Utc>,
    pub claim_revision: u64,
    pub owner_pid: u32,
    pub owner_process_start: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_repo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pinned_revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tracked_manifest_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<Uuid>,
    pub state: ScheduleRunState,
    pub phase: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_size: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_manifest_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivery_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,
    pub created_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_event_hash: Option<String>,
    pub event_hash: String,
}

#[derive(Debug, Clone, Default)]
pub struct ReceiptUpdate {
    pub phase: String,
    pub source_repo: Option<String>,
    pub pinned_revision: Option<String>,
    pub tracked_manifest_digest: Option<String>,
    pub workspace_id: Option<Uuid>,
    pub artifact_digest: Option<String>,
    pub artifact_size: Option<u64>,
    pub final_manifest_digest: Option<String>,
    pub delivery_status: Option<String>,
    pub outcome: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RunJournal {
    root: PathBuf,
}

impl RunJournal {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root: normalize_missing_path(&root).unwrap_or(root),
        }
    }

    pub fn create_claim(
        &self,
        claim: &RunClaim,
        now: DateTime<Utc>,
    ) -> Result<RunReceipt, ScheduleV2Error> {
        ensure_private_dir(&self.root)?;
        let run_dir = self.root.join(claim.run_id.to_string());
        ensure_private_dir(&run_dir)?;
        let mut receipt = RunReceipt {
            version: JOURNAL_VERSION,
            sequence: 0,
            profile: claim.profile.clone(),
            schedule_id: claim.schedule_id.clone(),
            definition_digest: claim.definition_digest.clone(),
            run_id: claim.run_id,
            occurrence_id: claim.occurrence.id.clone(),
            occurrence_at: claim.occurrence.at,
            claim_revision: claim.claim_revision,
            owner_pid: std::process::id(),
            owner_process_start: process_identity(),
            source_repo: None,
            pinned_revision: claim.definition.pinned_revision.clone(),
            tracked_manifest_digest: None,
            workspace_id: None,
            state: ScheduleRunState::Claimed,
            phase: "claimed".into(),
            artifact_digest: None,
            artifact_size: None,
            final_manifest_digest: None,
            delivery_status: None,
            outcome: None,
            created_at: now,
            previous_event_hash: None,
            event_hash: String::new(),
        };
        receipt.event_hash = receipt_hash(&receipt)?;
        self.write_event(&receipt)?;
        Ok(receipt)
    }

    pub fn transition(
        &self,
        run_id: Uuid,
        state: ScheduleRunState,
        update: ReceiptUpdate,
        now: DateTime<Utc>,
    ) -> Result<RunReceipt, ScheduleV2Error> {
        let previous = self
            .latest(run_id)?
            .ok_or_else(|| ScheduleV2Error::State(format!("run journal is missing {run_id}")))?;
        if previous.state.terminal() || !previous.state.may_transition_to(state) {
            return Err(ScheduleV2Error::State(format!(
                "illegal run transition {:?} -> {:?}",
                previous.state, state
            )));
        }
        let sequence = previous
            .sequence
            .checked_add(1)
            .ok_or_else(|| ScheduleV2Error::State("journal sequence overflow".into()))?;
        let mut receipt = RunReceipt {
            version: JOURNAL_VERSION,
            sequence,
            profile: previous.profile,
            schedule_id: previous.schedule_id,
            definition_digest: previous.definition_digest,
            run_id,
            occurrence_id: previous.occurrence_id,
            occurrence_at: previous.occurrence_at,
            claim_revision: previous.claim_revision,
            owner_pid: previous.owner_pid,
            owner_process_start: previous.owner_process_start,
            source_repo: update.source_repo.or(previous.source_repo),
            pinned_revision: update.pinned_revision.or(previous.pinned_revision),
            tracked_manifest_digest: update
                .tracked_manifest_digest
                .or(previous.tracked_manifest_digest),
            workspace_id: update.workspace_id.or(previous.workspace_id),
            state,
            phase: update.phase,
            artifact_digest: update.artifact_digest.or(previous.artifact_digest),
            artifact_size: update.artifact_size.or(previous.artifact_size),
            final_manifest_digest: update
                .final_manifest_digest
                .or(previous.final_manifest_digest),
            delivery_status: update.delivery_status.or(previous.delivery_status),
            outcome: update
                .outcome
                .map(|value| bounded(value, MAX_OUTCOME_BYTES))
                .or(previous.outcome),
            created_at: now,
            previous_event_hash: Some(previous.event_hash),
            event_hash: String::new(),
        };
        receipt.event_hash = receipt_hash(&receipt)?;
        self.write_event(&receipt)?;
        Ok(receipt)
    }

    pub fn history(&self, schedule_id: Option<&str>) -> Result<Vec<RunReceipt>, ScheduleV2Error> {
        if !self.root.exists() {
            return Ok(Vec::new());
        }
        check_private_dir(&self.root)?;
        let mut receipts = Vec::new();
        for entry in std::fs::read_dir(&self.root)? {
            let entry = entry?;
            let metadata = entry.metadata()?;
            if !metadata.is_dir() || entry.file_type()?.is_symlink() {
                return Err(ScheduleV2Error::State("invalid run journal entry".into()));
            }
            let run_id = entry
                .file_name()
                .to_string_lossy()
                .parse::<Uuid>()
                .map_err(|_| ScheduleV2Error::State("invalid run journal ID".into()))?;
            let history = self.read_run(run_id)?;
            if schedule_id
                .is_none_or(|id| history.first().is_some_and(|event| event.schedule_id == id))
            {
                receipts.extend(history);
            }
        }
        receipts.sort_by_key(|receipt| (receipt.created_at, receipt.run_id, receipt.sequence));
        Ok(receipts)
    }

    pub fn latest(&self, run_id: Uuid) -> Result<Option<RunReceipt>, ScheduleV2Error> {
        Ok(self.read_run(run_id)?.pop())
    }

    pub fn doctor(&self) -> Result<DoctorReport, ScheduleV2Error> {
        let events = self.history(None)?;
        let mut latest = BTreeMap::new();
        for event in events {
            latest.insert(event.run_id, event);
        }
        let interrupted = latest
            .values()
            .filter(|event| event.state == ScheduleRunState::Interrupted)
            .count();
        let active = latest
            .values()
            .filter(|event| !event.state.terminal())
            .count();
        Ok(DoctorReport {
            runs: latest.len(),
            active,
            interrupted,
            healthy: active == 0,
        })
    }

    fn read_run(&self, run_id: Uuid) -> Result<Vec<RunReceipt>, ScheduleV2Error> {
        let run_dir = self.root.join(run_id.to_string());
        if !run_dir.exists() {
            return Ok(Vec::new());
        }
        check_private_dir(&run_dir)?;
        let mut paths = std::fs::read_dir(&run_dir)?
            .map(|entry| entry.map(|value| value.path()))
            .collect::<Result<Vec<_>, _>>()?;
        paths.sort();
        let mut events = Vec::with_capacity(paths.len());
        let mut prior: Option<String> = None;
        for (index, path) in paths.into_iter().enumerate() {
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                return Err(ScheduleV2Error::State(
                    "invalid journal event filename".into(),
                ));
            }
            let bytes = read_owner_file(&path, MAX_EVENT_BYTES)?;
            let event: RunReceipt = serde_json::from_slice(&bytes)
                .map_err(|error| ScheduleV2Error::State(error.to_string()))?;
            if event.version != JOURNAL_VERSION
                || event.run_id != run_id
                || event.sequence != index as u64
                || event.previous_event_hash != prior
                || event.event_hash != receipt_hash(&event)?
            {
                return Err(ScheduleV2Error::State(format!(
                    "corrupt run journal {run_id}"
                )));
            }
            if let Some(previous) = events.last() {
                let previous: &RunReceipt = previous;
                if !previous.state.may_transition_to(event.state) {
                    return Err(ScheduleV2Error::State(format!(
                        "illegal run journal {run_id}"
                    )));
                }
            } else if event.state != ScheduleRunState::Claimed {
                return Err(ScheduleV2Error::State(format!(
                    "run {run_id} does not start claimed"
                )));
            }
            prior = Some(event.event_hash.clone());
            events.push(event);
        }
        Ok(events)
    }

    fn write_event(&self, receipt: &RunReceipt) -> Result<(), ScheduleV2Error> {
        let run_dir = self.root.join(receipt.run_id.to_string());
        let path = run_dir.join(
            format!("{:020}-{:?}.json", receipt.sequence, receipt.state).to_ascii_lowercase(),
        );
        let bytes = serde_json::to_vec_pretty(receipt)
            .map_err(|error| ScheduleV2Error::State(error.to_string()))?;
        if bytes.len() as u64 > MAX_EVENT_BYTES {
            return Err(ScheduleV2Error::State("journal event is too large".into()));
        }
        atomic_owner_write(&run_dir, &path, &bytes, false)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DoctorReport {
    pub runs: usize,
    pub active: usize,
    pub interrupted: usize,
    pub healthy: bool,
}

fn receipt_hash(receipt: &RunReceipt) -> Result<String, ScheduleV2Error> {
    let mut unhashed = receipt.clone();
    unhashed.event_hash.clear();
    let bytes =
        serde_json::to_vec(&unhashed).map_err(|error| ScheduleV2Error::State(error.to_string()))?;
    Ok(hex_digest(&Sha256::digest(bytes)))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LeaseReceipt {
    version: u32,
    token: Uuid,
    run_id: Uuid,
    workspace_digest: String,
    reserved_bytes: u64,
    deadline: DateTime<Utc>,
    pid: u32,
    process_start: String,
    root_device: u64,
    root_inode: u64,
}

#[derive(Debug, Clone)]
pub struct LeaseManager {
    root: PathBuf,
    byte_capacity: u64,
}

pub struct ScheduleLease {
    path: PathBuf,
    token: Uuid,
}

impl LeaseManager {
    pub fn new(private_root: PathBuf, byte_capacity: u64) -> Self {
        let private_root = normalize_missing_path(&private_root).unwrap_or(private_root);
        Self {
            root: private_root.join("leases"),
            byte_capacity,
        }
    }

    pub fn acquire(
        &self,
        run_id: Uuid,
        workspace: &Path,
        reserved_bytes: u64,
        deadline: DateTime<Utc>,
    ) -> Result<ScheduleLease, ScheduleV2Error> {
        if reserved_bytes == 0 || reserved_bytes > self.byte_capacity {
            return Err(ScheduleV2Error::State("invalid byte reservation".into()));
        }
        ensure_private_dir(&self.root)?;
        let _lock = OwnerLock::acquire(self.root.join("lease.lock"))?;
        let identity = root_identity(&self.root)?;
        let workspace_digest =
            hex_digest(&Sha256::digest(workspace.as_os_str().as_encoded_bytes()));
        let mut active = Vec::new();
        for entry in std::fs::read_dir(&self.root)? {
            let path = entry?.path();
            if path.file_name().and_then(|value| value.to_str()) == Some("lease.lock") {
                continue;
            }
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                return Err(ScheduleV2Error::State("unknown lease entry".into()));
            }
            let bytes = read_owner_file(&path, 16 * 1024)?;
            let lease: LeaseReceipt = serde_json::from_slice(&bytes)
                .map_err(|_| ScheduleV2Error::State("corrupt lease receipt".into()))?;
            if lease.version != LEASE_VERSION {
                return Err(ScheduleV2Error::State("unsupported lease receipt".into()));
            }
            // Automatic deletion is intentionally conservative. Even a dead
            // PID requires journal/workspace reconciliation by `doctor`.
            active.push(lease);
        }
        if active.len() >= MAX_LEASES
            || active
                .iter()
                .any(|lease| lease.workspace_digest == workspace_digest)
        {
            return Err(ScheduleV2Error::Busy);
        }
        let used = active
            .iter()
            .try_fold(0_u64, |sum, lease| sum.checked_add(lease.reserved_bytes))
            .ok_or_else(|| ScheduleV2Error::State("lease byte accounting overflow".into()))?;
        if used
            .checked_add(reserved_bytes)
            .is_none_or(|sum| sum > self.byte_capacity)
        {
            return Err(ScheduleV2Error::Busy);
        }
        let token = Uuid::new_v4();
        let receipt = LeaseReceipt {
            version: LEASE_VERSION,
            token,
            run_id,
            workspace_digest,
            reserved_bytes,
            deadline,
            pid: std::process::id(),
            process_start: process_identity(),
            root_device: identity.0,
            root_inode: identity.1,
        };
        let path = self.root.join(format!("{token}.json"));
        let bytes = serde_json::to_vec_pretty(&receipt)
            .map_err(|error| ScheduleV2Error::State(error.to_string()))?;
        atomic_owner_write(&self.root, &path, &bytes, false)?;
        Ok(ScheduleLease { path, token })
    }

    pub fn recover_terminal_dead(&self, journal: &RunJournal) -> Result<usize, ScheduleV2Error> {
        if !self.root.exists() {
            return Ok(0);
        }
        check_private_dir(&self.root)?;
        let _lock = OwnerLock::acquire(self.root.join("lease.lock"))?;
        let mut recovered = 0_usize;
        for entry in std::fs::read_dir(&self.root)? {
            let path = entry?.path();
            if path.file_name().and_then(|value| value.to_str()) == Some("lease.lock") {
                continue;
            }
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                return Err(ScheduleV2Error::State("unknown lease entry".into()));
            }
            let bytes = read_owner_file(&path, 16 * 1024)?;
            let lease: LeaseReceipt = serde_json::from_slice(&bytes)
                .map_err(|_| ScheduleV2Error::State("corrupt lease receipt".into()))?;
            let latest = journal.latest(lease.run_id)?.ok_or_else(|| {
                ScheduleV2Error::State("lease has no matching run journal".into())
            })?;
            if latest.owner_pid != lease.pid || latest.owner_process_start != lease.process_start {
                return Err(ScheduleV2Error::State(
                    "lease owner identity does not match its run journal".into(),
                ));
            }
            let private_root = self.root.parent().ok_or_else(|| {
                ScheduleV2Error::State("lease directory has no profile storage owner".into())
            })?;
            if latest.state.terminal()
                && !process_is_alive(lease.pid)?
                && !native_executor_is_alive(private_root, &latest)?
            {
                let current = read_owner_file(&path, 16 * 1024)?;
                let current: LeaseReceipt = serde_json::from_slice(&current)
                    .map_err(|_| ScheduleV2Error::State("corrupt lease receipt".into()))?;
                if current.token != lease.token || current.run_id != lease.run_id {
                    return Err(ScheduleV2Error::State(
                        "lease changed during recovery".into(),
                    ));
                }
                std::fs::remove_file(&path)?;
                sync_directory(&self.root)?;
                recovered = recovered.saturating_add(1);
            }
        }
        Ok(recovered)
    }
}

impl Drop for ScheduleLease {
    fn drop(&mut self) {
        let owned = read_owner_file(&self.path, 16 * 1024)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<LeaseReceipt>(&bytes).ok())
            .is_some_and(|receipt| {
                receipt.token == self.token && receipt.pid == std::process::id()
            });
        if owned {
            let _ = std::fs::remove_file(&self.path);
            if let Some(parent) = self.path.parent() {
                let _ = sync_directory(parent);
            }
        }
    }
}

#[derive(Debug)]
pub struct ScriptOutput {
    pub stdout: Vec<u8>,
}

pub async fn execute_script_only(
    definition: &ScheduleDefinitionV2,
    workspace: &Path,
    sandbox_mode: crate::sandbox::SandboxMode,
) -> Result<ScriptOutput, ScheduleV2Error> {
    let started = std::time::Instant::now();
    let output_limit = usize::try_from(definition.output_bytes)
        .ok()
        .filter(|limit| *limit > 0)
        .ok_or_else(|| ScheduleV2Error::Execution("output bound is unsupported".into()))?;
    if definition.runtime_secs == 0 {
        return Err(ScheduleV2Error::Execution(
            "script runtime capacity exhausted before dispatch".into(),
        ));
    }
    let ScheduleTargetV2::ScriptOnly {
        executable,
        args,
        executable_sha256,
        scrub_environment,
    } = &definition.target
    else {
        return Err(ScheduleV2Error::Execution(
            "target is not script-only".into(),
        ));
    };
    if !scrub_environment || hash_file(executable, 128 * 1024 * 1024)? != *executable_sha256 {
        return Err(ScheduleV2Error::Execution("script identity changed".into()));
    }
    let admission = crate::schedule::admit_native_worker(workspace)
        .await
        .map_err(ScheduleV2Error::Execution)?;
    let remaining = Duration::from_secs(definition.runtime_secs).saturating_sub(started.elapsed());
    if remaining.is_zero() {
        return Err(ScheduleV2Error::Execution(
            "script runtime capacity exhausted before dispatch".into(),
        ));
    }
    let temporary = tempfile::tempdir()?;
    let prepared = crate::sandbox::SandboxBackend::new(sandbox_mode)
        .prepare_process_with_denied_paths(
            executable,
            args,
            &crate::sandbox::SandboxPolicy {
                roots: vec![workspace.to_path_buf()],
                read_only_roots: vec![executable.clone()],
                temporary_directory: temporary.path().to_path_buf(),
                network_allowed: definition.allow_network,
                hide_other_processes: false,
            },
            &crate::jobs::automation_private_paths(),
        )
        .map_err(|error| ScheduleV2Error::Execution(error.to_string()))?;
    let contained =
        ca_automation::runtime::current_run_store().is_some() && crate::jobs::worker_contained();
    let mut command = tokio::process::Command::new(prepared.program);
    command
        .args(prepared.arguments)
        .current_dir(workspace)
        .env_clear()
        .env("TMPDIR", temporary.path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    if !contained {
        command.process_group(0);
    }
    let child = command
        .spawn()
        .map_err(|error| ScheduleV2Error::Execution(format!("could not start script: {error}")))?;
    let mut child = if contained {
        admission.attach_contained(child)
    } else {
        admission.attach(child)
    };
    let stdout = child.child_mut().stdout.take().expect("piped stdout");
    let stderr = child.child_mut().stderr.take().expect("piped stderr");
    let stdout_task = tokio::spawn(read_bounded(stdout, output_limit));
    let stderr_task = tokio::spawn(read_bounded(stderr, (64 * 1024).min(output_limit)));
    let status = match tokio::time::timeout(remaining, child.child_mut().wait()).await {
        Ok(Ok(status)) => status,
        Ok(Err(error)) => {
            return Err(ScheduleV2Error::Execution(format!(
                "script wait failed: {error}"
            )))
        }
        Err(_) => {
            let _ = child.retire(true).await;
            return Err(ScheduleV2Error::Execution("script timed out".into()));
        }
    };
    if child
        .retire(false)
        .await
        .map_err(|_| ScheduleV2Error::Execution("script cleanup stopped".into()))?
    {
        return Err(ScheduleV2Error::Execution(
            "script left native commands active; containment cleanup stopped them".into(),
        ));
    }
    let (stdout, stdout_overflow) = stdout_task
        .await
        .map_err(|_| ScheduleV2Error::Execution("stdout reader failed".into()))??;
    let (_stderr, stderr_overflow) = stderr_task
        .await
        .map_err(|_| ScheduleV2Error::Execution("stderr reader failed".into()))??;
    if stdout_overflow || stderr_overflow {
        return Err(ScheduleV2Error::Execution(
            "script output exceeded its bound".into(),
        ));
    }
    if !status.success() {
        return Err(ScheduleV2Error::Execution(format!(
            "script exited unsuccessfully ({:?})",
            status.code()
        )));
    }
    Ok(ScriptOutput { stdout })
}

async fn read_bounded<R: tokio::io::AsyncRead + Unpin>(
    mut reader: R,
    limit: usize,
) -> std::io::Result<(Vec<u8>, bool)> {
    let mut bytes = Vec::with_capacity(limit.min(8192));
    let mut buffer = [0_u8; 8192];
    let mut overflow = false;
    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        let keep = read.min(limit.saturating_sub(bytes.len()));
        bytes.extend_from_slice(&buffer[..keep]);
        overflow |= keep < read;
    }
    Ok((bytes, overflow))
}

#[derive(Debug)]
pub struct PreparedWorkspace {
    pub workspace: crate::workspace_materialization::DisposableWorkspace,
    pub pinned_revision: String,
    pub tracked_manifest_digest: String,
    pub source_repo_digest: String,
}

#[derive(Debug, Clone)]
pub struct ArtifactProof {
    pub digest: String,
    pub size: u64,
    pub final_manifest_digest: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RecoveryArchive {
    version: u32,
    entries: Vec<RecoveryEntry>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RecoveryEntry {
    path_base64: String,
    kind: RecoveryKind,
    mode: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    content_base64: Option<String>,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum RecoveryKind {
    Directory,
    Regular,
    Symlink,
}

/// Exports every final-tree entry, reconstructs it independently, compares the
/// reconstruction with WSP's own manifest algorithm, then exercises the
/// artifact-bound cleanup authority. Any failure preserves the disposable WSP.
pub fn export_verify_and_cleanup(
    prepared: PreparedWorkspace,
    artifact_root: &Path,
    run_id: Uuid,
) -> Result<ArtifactProof, ScheduleV2Error> {
    let artifact_root =
        normalize_missing_path(artifact_root).unwrap_or_else(|| artifact_root.to_path_buf());
    ensure_private_dir(&artifact_root)?;
    let archive = snapshot_recovery_tree(prepared.workspace.path())?;
    let bytes =
        serde_json::to_vec(&archive).map_err(|error| ScheduleV2Error::State(error.to_string()))?;
    if bytes.is_empty() || bytes.len() as u64 > 2 * 1024 * 1024 * 1024 {
        return Err(ScheduleV2Error::State(
            "recovery archive is empty or oversized".into(),
        ));
    }
    let artifact_path = artifact_root.join(format!("{run_id}.recovery.json"));
    atomic_owner_write(&artifact_root, &artifact_path, &bytes, false)?;
    let verification = prepared
        .workspace
        .verify_recovery_artifact(&artifact_path)
        .map_err(|error| ScheduleV2Error::State(format!("artifact binding failed: {error}")))?;

    let reconstruction = tempfile::Builder::new()
        .prefix("ca-schedule-reconstruct-")
        .tempdir()
        .map_err(|error| ScheduleV2Error::Io(error.to_string()))?;
    reconstruct_recovery_tree(&archive, reconstruction.path())?;
    let reconstruction_root = reconstruction
        .path()
        .canonicalize()
        .map_err(|error| ScheduleV2Error::Io(error.to_string()))?;
    let reconstructed_manifest = crate::workspace_materialization::inspect_workspace_manifest(
        &reconstruction_root,
        &crate::workspace_materialization::MaterializationPolicy::scheduled(),
    )
    .map_err(|error| ScheduleV2Error::State(format!("reconstruction scan failed: {error}")))?;
    if reconstructed_manifest != verification.final_manifest_sha256() {
        return Err(ScheduleV2Error::State(
            "recovery archive reconstruction does not match the final workspace".into(),
        ));
    }
    let proof = ArtifactProof {
        digest: verification.artifact_sha256(),
        size: verification.artifact_size(),
        final_manifest_digest: verification.final_manifest_sha256().to_string(),
    };
    prepared
        .workspace
        .cleanup_after_export(verification)
        .map_err(|error| ScheduleV2Error::State(format!("workspace cleanup failed: {error}")))?;
    Ok(proof)
}

fn snapshot_recovery_tree(root: &Path) -> Result<RecoveryArchive, ScheduleV2Error> {
    let mut entries = Vec::new();
    let mut hardlinks = BTreeSet::new();
    let mut logical_bytes = 0_u64;
    snapshot_directory(
        root,
        root,
        &mut entries,
        &mut hardlinks,
        0,
        &mut logical_bytes,
    )?;
    entries.sort_by(|left, right| left.path_base64.cmp(&right.path_base64));
    Ok(RecoveryArchive {
        version: 1,
        entries,
    })
}

fn snapshot_directory(
    root: &Path,
    directory: &Path,
    entries: &mut Vec<RecoveryEntry>,
    hardlinks: &mut BTreeSet<(u64, u64)>,
    depth: usize,
    logical_bytes: &mut u64,
) -> Result<(), ScheduleV2Error> {
    if depth > crate::workspace_materialization::DEFAULT_MAX_DEPTH {
        return Err(ScheduleV2Error::State("recovery tree is too deep".into()));
    }
    let mut children = std::fs::read_dir(directory)?.collect::<Result<Vec<_>, _>>()?;
    children.sort_by_key(std::fs::DirEntry::file_name);
    for child in children {
        if entries.len() >= crate::workspace_materialization::DEFAULT_MAX_ENTRIES {
            return Err(ScheduleV2Error::State(
                "recovery tree has too many entries".into(),
            ));
        }
        let path = child.path();
        let relative = path
            .strip_prefix(root)
            .map_err(|_| ScheduleV2Error::State("recovery path escaped its root".into()))?;
        if !valid_relative_path(relative) {
            return Err(ScheduleV2Error::State("unsafe recovery path".into()));
        }
        let metadata = std::fs::symlink_metadata(&path)?;
        #[cfg(unix)]
        let mode = {
            use std::os::unix::fs::MetadataExt;
            metadata.mode() & 0o7777
        };
        #[cfg(not(unix))]
        let mode = if metadata.permissions().readonly() {
            0o444
        } else {
            0o644
        };
        let path_base64 = encode_os_path(relative);
        if metadata.is_dir() {
            entries.push(RecoveryEntry {
                path_base64,
                kind: RecoveryKind::Directory,
                mode,
                content_base64: None,
            });
            snapshot_directory(root, &path, entries, hardlinks, depth + 1, logical_bytes)?;
        } else if metadata.is_file() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                if metadata.nlink() != 1 || !hardlinks.insert((metadata.dev(), metadata.ino())) {
                    return Err(ScheduleV2Error::State(
                        "hardlinks are unavailable in recovery archives".into(),
                    ));
                }
            }
            *logical_bytes = logical_bytes
                .checked_add(metadata.len())
                .ok_or_else(|| ScheduleV2Error::State("recovery byte count overflow".into()))?;
            if *logical_bytes > crate::workspace_materialization::DEFAULT_MAX_LOGICAL_BYTES {
                return Err(ScheduleV2Error::State("recovery tree is oversized".into()));
            }
            let content = std::fs::read(&path)?;
            entries.push(RecoveryEntry {
                path_base64,
                kind: RecoveryKind::Regular,
                mode,
                content_base64: Some(base64::engine::general_purpose::STANDARD.encode(content)),
            });
        } else if metadata.file_type().is_symlink() {
            let target = std::fs::read_link(&path)?;
            entries.push(RecoveryEntry {
                path_base64,
                kind: RecoveryKind::Symlink,
                mode,
                content_base64: Some(encode_os_path(&target)),
            });
        } else {
            return Err(ScheduleV2Error::State(
                "special recovery entry is unsupported".into(),
            ));
        }
    }
    Ok(())
}

fn reconstruct_recovery_tree(
    archive: &RecoveryArchive,
    root: &Path,
) -> Result<(), ScheduleV2Error> {
    if archive.version != 1 || archive.entries.is_empty() {
        return Err(ScheduleV2Error::State("invalid recovery archive".into()));
    }
    let mut decoded = archive
        .entries
        .iter()
        .map(|entry| Ok((decode_os_path(&entry.path_base64)?, entry)))
        .collect::<Result<Vec<_>, ScheduleV2Error>>()?;
    decoded.sort_by_key(|(path, _)| path.components().count());
    for (relative, entry) in &decoded {
        if !valid_relative_path(relative) {
            return Err(ScheduleV2Error::State("unsafe reconstructed path".into()));
        }
        let destination = root.join(relative);
        match entry.kind {
            RecoveryKind::Directory => std::fs::create_dir(&destination)?,
            RecoveryKind::Regular => {
                let content = entry.content_base64.as_deref().ok_or_else(|| {
                    ScheduleV2Error::State("recovery file is missing bytes".into())
                })?;
                let content = base64::engine::general_purpose::STANDARD
                    .decode(content)
                    .map_err(|_| {
                        ScheduleV2Error::State("recovery file encoding is invalid".into())
                    })?;
                let mut options = OpenOptions::new();
                options.write(true).create_new(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    options.mode(entry.mode);
                }
                let mut file = options.open(&destination)?;
                file.write_all(&content)?;
                file.sync_all()?;
            }
            RecoveryKind::Symlink => {
                let target = entry.content_base64.as_deref().ok_or_else(|| {
                    ScheduleV2Error::State("recovery link is missing a target".into())
                })?;
                let target = decode_os_path(target)?;
                #[cfg(unix)]
                std::os::unix::fs::symlink(target, &destination)?;
                #[cfg(not(unix))]
                return Err(ScheduleV2Error::State("recovery links require Unix".into()));
            }
        }
    }
    #[cfg(unix)]
    for (relative, entry) in decoded.iter().rev() {
        if matches!(entry.kind, RecoveryKind::Directory | RecoveryKind::Regular) {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                root.join(relative),
                std::fs::Permissions::from_mode(entry.mode),
            )?;
        }
    }
    Ok(())
}

#[cfg(unix)]
fn encode_os_path(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    base64::engine::general_purpose::STANDARD.encode(path.as_os_str().as_bytes())
}

#[cfg(not(unix))]
fn encode_os_path(path: &Path) -> String {
    base64::engine::general_purpose::STANDARD.encode(path.to_string_lossy().as_bytes())
}

#[cfg(unix)]
fn decode_os_path(value: &str) -> Result<PathBuf, ScheduleV2Error> {
    use std::os::unix::ffi::OsStringExt;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(value)
        .map_err(|_| ScheduleV2Error::State("recovery path encoding is invalid".into()))?;
    Ok(PathBuf::from(std::ffi::OsString::from_vec(bytes)))
}

#[cfg(not(unix))]
fn decode_os_path(value: &str) -> Result<PathBuf, ScheduleV2Error> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(value)
        .map_err(|_| ScheduleV2Error::State("recovery path encoding is invalid".into()))?;
    let value = String::from_utf8(bytes)
        .map_err(|_| ScheduleV2Error::State("recovery path is not UTF-8".into()))?;
    Ok(PathBuf::from(value))
}

pub fn materialize_isolated_workspace(
    definition: &ScheduleDefinitionV2,
    private_root: &Path,
) -> Result<PreparedWorkspace, ScheduleV2Error> {
    if definition.workspace_policy != WorkspacePolicy::IsolatedPinned {
        return Err(ScheduleV2Error::InvalidDefinition(
            "workspace is not isolated pinned".into(),
        ));
    }
    let revision = definition
        .pinned_revision
        .as_deref()
        .ok_or_else(|| ScheduleV2Error::InvalidDefinition("missing pinned revision".into()))?;
    let normalized_private_root =
        normalize_missing_path(private_root).unwrap_or_else(|| private_root.to_path_buf());
    let private_root = normalized_private_root.as_path();
    ensure_private_dir(private_root)?;
    ensure_git_source_clean(&definition.workspace)?;
    let resolved = git_text(
        &definition.workspace,
        &["rev-parse", "--verify", &format!("{revision}^{{commit}}")],
    )?;
    if resolved != revision {
        return Err(ScheduleV2Error::State(
            "pinned revision did not resolve exactly".into(),
        ));
    }
    let staging_root = private_root.join("git-staging");
    let wsp_root = private_root.join("workspaces");
    ensure_private_dir(&staging_root)?;
    ensure_private_dir(&wsp_root)?;
    let staging = staging_root.join(Uuid::new_v4().to_string());
    git_status(
        &definition.workspace,
        &[
            "worktree",
            "add",
            "--detach",
            path_text(&staging)?,
            revision,
        ],
    )?;
    // `?` preserves the detached worktree as recovery evidence on validation
    // failure because removal occurs only after successful WSP publication.
    let (tracked, manifest_digest) = tracked_tree(&staging, revision)?;
    let allowlist = crate::workspace_materialization::ScheduledTrackedAllowlist::new(tracked)
        .map_err(|error| ScheduleV2Error::State(error.to_string()))?;
    let workspace = crate::workspace_materialization::materialize_scheduled_workspace(
        &staging,
        &wsp_root,
        &format!("schedule-{}", definition.id),
        &crate::workspace_materialization::MaterializationPolicy::scheduled(),
        crate::workspace_materialization::ClonePreference::CopyOnly,
        &allowlist,
    )
    .map_err(|error| ScheduleV2Error::State(error.to_string()))?;
    git_status(
        &definition.workspace,
        &["worktree", "remove", path_text(&staging)?],
    )?;
    let source_repo_digest = hex_digest(&Sha256::digest(
        definition.workspace.as_os_str().as_encoded_bytes(),
    ));
    Ok(PreparedWorkspace {
        workspace,
        pinned_revision: resolved,
        tracked_manifest_digest: manifest_digest,
        source_repo_digest,
    })
}

fn ensure_git_source_clean(repo: &Path) -> Result<(), ScheduleV2Error> {
    let inside = git_text(repo, &["rev-parse", "--is-inside-work-tree"])?;
    if inside != "true" {
        return Err(ScheduleV2Error::State(
            "workspace is not a Git worktree".into(),
        ));
    }
    let output = git_bytes(
        repo,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
    )?;
    if !output.is_empty() {
        return Err(ScheduleV2Error::State(
            "source worktree has dirty tracked or untracked files".into(),
        ));
    }
    Ok(())
}

fn tracked_tree(repo: &Path, revision: &str) -> Result<(Vec<PathBuf>, String), ScheduleV2Error> {
    let bytes = git_bytes(repo, &["ls-tree", "-rz", "--full-tree", revision])?;
    let mut entries = Vec::new();
    let mut hasher = Sha256::new();
    for raw in bytes
        .split(|byte| *byte == 0)
        .filter(|value| !value.is_empty())
    {
        let tab = raw
            .iter()
            .position(|byte| *byte == b'\t')
            .ok_or_else(|| ScheduleV2Error::State("invalid Git tree record".into()))?;
        let header = std::str::from_utf8(&raw[..tab])
            .map_err(|_| ScheduleV2Error::State("invalid Git tree header".into()))?;
        let mut parts = header.split_whitespace();
        let mode = parts.next().unwrap_or_default();
        let kind = parts.next().unwrap_or_default();
        let oid = parts.next().unwrap_or_default();
        if kind != "blob"
            || mode == "160000"
            || !matches!(mode, "100644" | "100755" | "120000")
            || !is_git_oid(oid)
        {
            return Err(ScheduleV2Error::State("unsupported Git tree entry".into()));
        }
        #[cfg(unix)]
        let path = {
            use std::os::unix::ffi::OsStringExt;
            PathBuf::from(std::ffi::OsString::from_vec(raw[tab + 1..].to_vec()))
        };
        #[cfg(not(unix))]
        let path = PathBuf::from(
            std::str::from_utf8(&raw[tab + 1..])
                .map_err(|_| ScheduleV2Error::State("non-UTF8 Git path unsupported".into()))?,
        );
        if !valid_relative_path(&path) || path.components().any(|part| part.as_os_str() == ".git") {
            return Err(ScheduleV2Error::State("unsafe Git tree path".into()));
        }
        hasher.update(raw);
        hasher.update([0]);
        entries.push(path);
    }
    if entries.is_empty() {
        return Err(ScheduleV2Error::State("Git tree is empty".into()));
    }
    Ok((entries, hex_digest(&hasher.finalize())))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceAction {
    Preview,
    Install,
    Uninstall,
    Status,
}

#[derive(Debug, Clone)]
pub struct LaunchAgentPreview {
    pub label: String,
    pub path: PathBuf,
    pub xml: Vec<u8>,
    pub digest: String,
}

pub fn launch_agent_preview(
    paths: &crate::runtime::RuntimePaths,
    profile: &crate::runtime::ActiveProfile,
    executable: &Path,
) -> Result<LaunchAgentPreview, ScheduleV2Error> {
    let normalized_home = paths
        .home
        .canonicalize()
        .map_err(|_| ScheduleV2Error::State("HOME is unavailable".into()))?;
    let normalized_paths = crate::runtime::RuntimePaths::from_home(&normalized_home);
    let executable = executable
        .canonicalize()
        .map_err(|_| ScheduleV2Error::State("ca executable is unavailable".into()))?;
    let metadata = std::fs::metadata(&executable)?;
    if !metadata.is_file() {
        return Err(ScheduleV2Error::State(
            "ca executable is not a regular file".into(),
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o022 != 0 {
            return Err(ScheduleV2Error::State(
                "ca executable ownership or mode is unsafe".into(),
            ));
        }
    }
    let profile_label = profile.label();
    let label_suffix = hex_digest(&Sha256::digest(profile_label.as_bytes()))[..16].to_string();
    let label = format!("com.ca.schedule.{label_suffix}");
    let log_dir = normalized_paths.profile_data_dir(profile).join("logs");
    let path = normalized_home
        .join("Library/LaunchAgents")
        .join(format!("{label}.plist"));
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n  <key>Label</key><string>{}</string>\n  <key>ProgramArguments</key>\n  <array>\n    <string>{}</string>\n    <string>schedule</string>\n    <string>--profile</string>\n    <string>{}</string>\n    <string>tick</string>\n    <string>--drain</string>\n  </array>\n  <key>EnvironmentVariables</key>\n  <dict><key>HOME</key><string>{}</string></dict>\n  <key>RunAtLoad</key><false/>\n  <key>StartInterval</key><integer>60</integer>\n  <key>StandardOutPath</key><string>{}</string>\n  <key>StandardErrorPath</key><string>{}</string>\n</dict>\n</plist>\n",
        xml_escape(&label),
        xml_escape(&executable.to_string_lossy()),
        xml_escape(profile_label),
        xml_escape(&normalized_home.to_string_lossy()),
        xml_escape(&log_dir.join("schedule.out.log").to_string_lossy()),
        xml_escape(&log_dir.join("schedule.err.log").to_string_lossy()),
    ).into_bytes();
    let digest = hex_digest(&Sha256::digest(&xml));
    Ok(LaunchAgentPreview {
        label,
        path,
        xml,
        digest,
    })
}

pub fn service_mutation(
    action: ServiceAction,
    preview: &LaunchAgentPreview,
    confirmation: Option<&str>,
    launchctl: &Path,
) -> Result<String, ScheduleV2Error> {
    if action == ServiceAction::Preview {
        return Ok(String::from_utf8_lossy(&preview.xml).into_owned());
    }
    let launchctl = trusted_launchctl(launchctl)?;
    match action {
        ServiceAction::Preview => unreachable!("preview returned before launchctl validation"),
        ServiceAction::Status => {
            let status = std::process::Command::new(&launchctl)
                .args([
                    "print",
                    &format!("gui/{}/{}", unsafe { libc::geteuid() }, preview.label),
                ])
                .env_clear()
                .status()?;
            Ok(if status.success() {
                "loaded"
            } else {
                "not loaded"
            }
            .into())
        }
        ServiceAction::Install => {
            require_confirmation(confirmation, &preview.digest)?;
            let parent = preview
                .path
                .parent()
                .ok_or_else(|| ScheduleV2Error::State("plist has no parent".into()))?;
            ensure_private_dir(parent)?;
            if preview.path.exists() {
                let current = read_owner_file(&preview.path, MAX_EVENT_BYTES)?;
                if current != preview.xml {
                    return Err(ScheduleV2Error::State(
                        "refusing to replace a different plist".into(),
                    ));
                }
            } else {
                atomic_owner_write(parent, &preview.path, &preview.xml, false)?;
            }
            let status = std::process::Command::new(&launchctl)
                .args([
                    "bootstrap",
                    &format!("gui/{}", unsafe { libc::geteuid() }),
                    path_text(&preview.path)?,
                ])
                .env_clear()
                .status()?;
            if !status.success() {
                return Err(ScheduleV2Error::Execution(
                    "launchctl bootstrap failed".into(),
                ));
            }
            Ok("installed".into())
        }
        ServiceAction::Uninstall => {
            require_confirmation(confirmation, &preview.digest)?;
            let current = read_owner_file(&preview.path, MAX_EVENT_BYTES)?;
            if current != preview.xml {
                return Err(ScheduleV2Error::State(
                    "installed plist does not match preview".into(),
                ));
            }
            let status = std::process::Command::new(&launchctl)
                .args([
                    "bootout",
                    &format!("gui/{}/{}", unsafe { libc::geteuid() }, preview.label),
                ])
                .env_clear()
                .status()?;
            if !status.success() {
                return Err(ScheduleV2Error::Execution(
                    "launchctl bootout failed".into(),
                ));
            }
            std::fs::remove_file(&preview.path)?;
            sync_directory(preview.path.parent().expect("plist parent"))?;
            Ok("uninstalled".into())
        }
    }
}

fn trusted_launchctl(path: &Path) -> Result<PathBuf, ScheduleV2Error> {
    if !path.is_absolute() {
        return Err(ScheduleV2Error::State(
            "launchctl path must be absolute".into(),
        ));
    }
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(ScheduleV2Error::State(
            "launchctl must be a regular non-symlink file".into(),
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.mode() & 0o022 != 0 {
            return Err(ScheduleV2Error::State(
                "launchctl is writable by group or other".into(),
            ));
        }
    }
    path.canonicalize().map_err(ScheduleV2Error::from)
}

fn require_confirmation(value: Option<&str>, expected: &str) -> Result<(), ScheduleV2Error> {
    if value.is_some_and(|value| constant_time_eq(value.as_bytes(), expected.as_bytes())) {
        Ok(())
    } else {
        Err(ScheduleV2Error::ConfirmationRequired(expected.into()))
    }
}

pub fn load_definition_file(path: &Path) -> Result<ScheduleDefinitionV2, ScheduleV2Error> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|_| ScheduleV2Error::InvalidDefinition("definition file is unavailable".into()))?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() > MAX_DEFINITION_BYTES
    {
        return Err(ScheduleV2Error::InvalidDefinition(
            "definition must be a bounded regular file".into(),
        ));
    }
    let extension = path.extension().and_then(|value| value.to_str());
    if !matches!(extension, Some("json" | "toml")) {
        return Err(ScheduleV2Error::InvalidDefinition(
            "definition must be JSON or TOML".into(),
        ));
    }
    let mut bytes = Vec::new();
    File::open(path)?
        .take(MAX_DEFINITION_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_DEFINITION_BYTES {
        return Err(ScheduleV2Error::InvalidDefinition(
            "definition is too large".into(),
        ));
    }
    if extension == Some("json") {
        serde_json::from_slice(&bytes)
            .map_err(|error| ScheduleV2Error::InvalidDefinition(error.to_string()))
    } else {
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| ScheduleV2Error::InvalidDefinition("definition is not UTF-8".into()))?;
        toml::from_str(text).map_err(|error| ScheduleV2Error::InvalidDefinition(error.to_string()))
    }
}

pub fn is_v2_definition_file(path: &Path) -> bool {
    let Ok(bytes) = std::fs::read(path) else {
        return false;
    };
    if bytes.len() as u64 > MAX_DEFINITION_BYTES {
        return false;
    }
    let version = if path.extension().and_then(|value| value.to_str()) == Some("json") {
        serde_json::from_slice::<serde_json::Value>(&bytes)
            .ok()
            .and_then(|value| value.get("version").and_then(serde_json::Value::as_u64))
    } else {
        std::str::from_utf8(&bytes)
            .ok()
            .and_then(|text| toml::from_str::<toml::Value>(text).ok())
            .and_then(|value| {
                value
                    .get("version")
                    .and_then(toml::Value::as_integer)
                    .and_then(|value| u64::try_from(value).ok())
            })
    };
    version == Some(2)
}

fn validate_delivery(delivery: &ScheduleDeliveryV2) -> Result<(), ScheduleV2Error> {
    match delivery {
        ScheduleDeliveryV2::None => Ok(()),
        ScheduleDeliveryV2::LocalFile { path } if valid_relative(path) => Ok(()),
        ScheduleDeliveryV2::LocalFile { .. } => Err(ScheduleV2Error::InvalidDefinition(
            "invalid local delivery path".into(),
        )),
        ScheduleDeliveryV2::Webhook { url, secret_env } => {
            let parsed = reqwest::Url::parse(url)
                .map_err(|_| ScheduleV2Error::InvalidDefinition("invalid webhook URL".into()))?;
            let safe_env = !secret_env.is_empty()
                && secret_env.len() <= 128
                && secret_env.as_bytes()[0].is_ascii_uppercase()
                && secret_env.bytes().all(|value| {
                    value.is_ascii_uppercase() || value.is_ascii_digit() || value == b'_'
                });
            let loopback = parsed
                .host_str()
                .and_then(|host| host.parse::<std::net::IpAddr>().ok())
                .is_some_and(|address| address.is_loopback());
            if !safe_env
                || (!loopback && parsed.scheme() != "https")
                || !parsed.username().is_empty()
                || parsed.password().is_some()
                || parsed.query().is_some()
                || parsed.fragment().is_some()
            {
                Err(ScheduleV2Error::InvalidDefinition(
                    "unsafe webhook delivery".into(),
                ))
            } else {
                Ok(())
            }
        }
        ScheduleDeliveryV2::EmailDraft {
            account,
            recipients,
        } => {
            if crate::email::types::AccountId::parse(account.clone()).is_err()
                || recipients.is_empty()
                || recipients.len() > 32
            {
                return Err(ScheduleV2Error::InvalidDefinition(
                    "invalid email_draft delivery".into(),
                ));
            }
            let mut canonical = recipients
                .iter()
                .map(|value| {
                    crate::email::types::EmailAddress::parse(value)
                        .map(|address| address.as_str().to_owned())
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(|_| {
                    ScheduleV2Error::InvalidDefinition("invalid email_draft recipient".into())
                })?;
            canonical.sort();
            canonical.dedup();
            if &canonical != recipients {
                return Err(ScheduleV2Error::InvalidDefinition(
                    "email_draft recipients must be sorted, unique, and canonical".into(),
                ));
            }
            Ok(())
        }
    }
}

fn validate_skill_pins(skills: &[ScheduleSkillPin]) -> Result<(), ScheduleV2Error> {
    let mut seen = BTreeSet::new();
    for skill in skills {
        if !valid_atom(&skill.instance_id)
            || !is_sha256(&skill.full_digest)
            || !seen.insert(&skill.instance_id)
        {
            return Err(ScheduleV2Error::InvalidDefinition(
                "skills require unique exact instance IDs and full digests".into(),
            ));
        }
    }
    Ok(())
}

#[allow(dead_code)] // Public seam consumed by scheduled skill activation once a registry is injected.
pub fn resolve_skill_pins<'a>(
    pins: &[ScheduleSkillPin],
    instances: impl IntoIterator<Item = &'a crate::skills::SkillInstance> + Clone,
) -> Result<Vec<&'a crate::skills::SkillInstance>, ScheduleV2Error> {
    pins.iter()
        .map(|pin| {
            crate::skills::resolve_pinned_instance(
                instances.clone(),
                &pin.instance_id,
                &pin.full_digest,
            )
            .map_err(|error| ScheduleV2Error::State(error.to_string()))
        })
        .collect()
}

fn validate_id(value: &str) -> Result<(), ScheduleV2Error> {
    if !value.is_empty()
        && value.len() <= 64
        && value.as_bytes()[0].is_ascii_lowercase()
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
        })
    {
        Ok(())
    } else {
        Err(ScheduleV2Error::InvalidDefinition(
            "invalid schedule ID".into(),
        ))
    }
}

fn valid_atom(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && !value
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
}

fn valid_relative(value: &str) -> bool {
    !value.is_empty() && value.len() <= 4096 && valid_relative_path(Path::new(value))
}

fn valid_relative_path(path: &Path) -> bool {
    !path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn is_git_oid(value: &str) -> bool {
    matches!(value.len(), 40 | 64)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn hash_file(path: &Path, max_bytes: u64) -> Result<String, ScheduleV2Error> {
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > max_bytes {
        return Err(ScheduleV2Error::InvalidDefinition(
            "unsafe executable file".into(),
        ));
    }
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut total = 0_u64;
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        total = total
            .checked_add(read as u64)
            .ok_or_else(|| ScheduleV2Error::State("file size overflow".into()))?;
        if total > max_bytes {
            return Err(ScheduleV2Error::InvalidDefinition(
                "executable is too large".into(),
            ));
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex_digest(&hasher.finalize()))
}

fn stable_run_id(definition_digest: &str, occurrence_id: &str) -> Uuid {
    let mut hasher = Sha256::new();
    hasher.update(definition_digest);
    hasher.update([0]);
    hasher.update(occurrence_id);
    let digest = hasher.finalize();
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

fn hex_digest(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        })
        == 0
}

fn bounded(mut value: String, limit: usize) -> String {
    if value.len() <= limit {
        return value;
    }
    while !value.is_char_boundary(limit) {
        value.pop();
    }
    value.truncate(limit);
    value
}

fn ensure_private_dir(path: &Path) -> Result<(), ScheduleV2Error> {
    reject_symlink_components(path)?;
    std::fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    check_private_dir(path)
}

fn check_private_dir(path: &Path) -> Result<(), ScheduleV2Error> {
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(ScheduleV2Error::State(
            "private path is not a directory".into(),
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 {
            return Err(ScheduleV2Error::State(
                "private directory ownership or mode is unsafe".into(),
            ));
        }
    }
    Ok(())
}

fn read_owner_file(path: &Path, limit: u64) -> Result<Vec<u8>, ScheduleV2Error> {
    reject_symlink_components(path)?;
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > limit {
        return Err(ScheduleV2Error::State(
            "state file is unsafe or oversized".into(),
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 {
            return Err(ScheduleV2Error::State(
                "state file ownership or mode is unsafe".into(),
            ));
        }
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    File::open(path)?.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(ScheduleV2Error::State("state file is oversized".into()));
    }
    Ok(bytes)
}

fn atomic_owner_write(
    dir: &Path,
    path: &Path,
    bytes: &[u8],
    replace: bool,
) -> Result<(), ScheduleV2Error> {
    ensure_private_dir(dir)?;
    let temp = dir.join(format!(".schedule-v2.{}.tmp", Uuid::new_v4()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| {
        let mut file = options.open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        if !replace && path.exists() {
            return Err(ScheduleV2Error::State(
                "immutable state event already exists".into(),
            ));
        }
        std::fs::rename(&temp, path)?;
        sync_directory(dir)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

fn sync_directory(path: &Path) -> Result<(), ScheduleV2Error> {
    File::open(path)?.sync_all()?;
    Ok(())
}

fn reject_symlink_components(path: &Path) -> Result<(), ScheduleV2Error> {
    for ancestor in path.ancestors() {
        if ancestor.as_os_str().is_empty() {
            continue;
        }
        if std::fs::symlink_metadata(ancestor)
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
        {
            return Err(ScheduleV2Error::State(
                "path contains a symbolic link".into(),
            ));
        }
    }
    Ok(())
}

fn normalize_missing_path(path: &Path) -> Option<PathBuf> {
    let existing = path.ancestors().find(|ancestor| ancestor.exists())?;
    let canonical = existing.canonicalize().ok()?;
    let suffix = path.strip_prefix(existing).ok()?;
    Some(canonical.join(suffix))
}

struct OwnerLock {
    path: PathBuf,
    token: Uuid,
}

#[derive(Serialize, Deserialize)]
struct OwnerLockReceipt {
    pid: u32,
    token: Uuid,
}

impl OwnerLock {
    fn acquire(path: PathBuf) -> Result<Self, ScheduleV2Error> {
        let parent = path
            .parent()
            .ok_or_else(|| ScheduleV2Error::State("lock has no parent".into()))?;
        ensure_private_dir(parent)?;
        let token = Uuid::new_v4();
        let bytes = serde_json::to_vec(&OwnerLockReceipt {
            pid: std::process::id(),
            token,
        })
        .map_err(|error| ScheduleV2Error::State(error.to_string()))?;
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                ScheduleV2Error::Busy
            } else {
                ScheduleV2Error::Io(error.to_string())
            }
        })?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        Ok(Self { path, token })
    }
}

impl Drop for OwnerLock {
    fn drop(&mut self) {
        let owned = std::fs::read(&self.path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<OwnerLockReceipt>(&bytes).ok())
            .is_some_and(|receipt| {
                receipt.pid == std::process::id() && receipt.token == self.token
            });
        if owned {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

#[cfg(unix)]
fn root_identity(path: &Path) -> Result<(u64, u64), ScheduleV2Error> {
    use std::os::unix::fs::MetadataExt;
    let metadata = std::fs::metadata(path)?;
    Ok((metadata.dev(), metadata.ino()))
}

#[cfg(not(unix))]
fn root_identity(_path: &Path) -> Result<(u64, u64), ScheduleV2Error> {
    Err(ScheduleV2Error::State(
        "leases require Unix identity".into(),
    ))
}

fn process_identity() -> String {
    static IDENTITY: OnceLock<String> = OnceLock::new();
    IDENTITY.get_or_init(process_identity_inner).clone()
}

fn process_identity_inner() -> String {
    #[cfg(target_os = "linux")]
    if let Ok(stat) = std::fs::read_to_string(format!("/proc/{}/stat", std::process::id())) {
        if let Some(start) = stat.split_whitespace().nth(21) {
            return start.into();
        }
    }
    // PID plus executable identity is deliberately not enough for stale lease
    // recovery; this token only binds a live lease owned by this process.
    format!("pid:{}:session:{}", std::process::id(), Uuid::new_v4())
}

pub(crate) fn owner_is_alive(pid: u32) -> Result<bool, ScheduleV2Error> {
    process_is_alive(pid)
}
fn process_is_alive(pid: u32) -> Result<bool, ScheduleV2Error> {
    #[cfg(unix)]
    {
        let pid =
            i32::try_from(pid).map_err(|_| ScheduleV2Error::State("invalid owner PID".into()))?;
        match nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None) {
            Ok(()) | Err(nix::errno::Errno::EPERM) => Ok(true),
            Err(nix::errno::Errno::ESRCH) => Ok(false),
            Err(error) => Err(ScheduleV2Error::State(format!(
                "could not inspect owner PID: {error}"
            ))),
        }
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        Err(ScheduleV2Error::State(
            "owner recovery requires Unix process identity".into(),
        ))
    }
}

fn git_text(repo: &Path, args: &[&str]) -> Result<String, ScheduleV2Error> {
    let bytes = git_bytes(repo, args)?;
    String::from_utf8(bytes)
        .map(|value| value.trim().to_string())
        .map_err(|_| ScheduleV2Error::State("Git returned non-UTF8 identity".into()))
}

fn git_bytes(repo: &Path, args: &[&str]) -> Result<Vec<u8>, ScheduleV2Error> {
    let output = std::process::Command::new(git_executable()?)
        .args(args)
        .current_dir(repo)
        .env_clear()
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("PATH", "/usr/local/bin:/usr/bin:/bin")
        .output()?;
    if !output.status.success() {
        return Err(ScheduleV2Error::State(
            "Git validation command failed".into(),
        ));
    }
    Ok(output.stdout)
}

fn git_status(repo: &Path, args: &[&str]) -> Result<(), ScheduleV2Error> {
    let output = std::process::Command::new(git_executable()?)
        .args(args)
        .current_dir(repo)
        .env_clear()
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("PATH", "/usr/local/bin:/usr/bin:/bin")
        .output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(ScheduleV2Error::State("Git worktree command failed".into()))
    }
}

fn git_executable() -> Result<&'static Path, ScheduleV2Error> {
    [Path::new("/usr/bin/git"), Path::new("/usr/local/bin/git")]
        .into_iter()
        .find(|path| path.is_file())
        .ok_or_else(|| ScheduleV2Error::State("Git executable was not found".into()))
}

fn path_text(path: &Path) -> Result<&str, ScheduleV2Error> {
    path.to_str()
        .ok_or_else(|| ScheduleV2Error::State("non-UTF8 service/Git path is unsupported".into()))
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn absolute_definition(root: &Path) -> ScheduleDefinitionV2 {
        ScheduleDefinitionV2 {
            version: 2,
            id: "nightly".into(),
            provider: Some("fake".into()),
            model: Some("model".into()),
            workspace: root.canonicalize().unwrap(),
            runtime_secs: 10,
            output_bytes: 1024,
            tools: Vec::new(),
            grants: Vec::new(),
            allow_network: false,
            skills: Vec::new(),
            workspace_policy: WorkspacePolicy::ExistingReadOnly,
            pinned_revision: None,
            trigger: ScheduleTriggerV2::Absolute {
                at: "2026-08-14T00:00:00Z".parse().unwrap(),
            },
            missed_run_policy: MissedRunPolicy::Skip,
            target: ScheduleTargetV2::Prompt {
                prompt: "hello".into(),
            },
            delivery: ScheduleDeliveryV2::None,
        }
    }

    #[test]
    fn mechanistic_target_admits_model_free_query_evidence_batches_and_excludes_observation() {
        let root = tempfile::tempdir().unwrap();
        let mut definition = absolute_definition(root.path());
        definition.provider = None;
        definition.model = None;
        definition.target = ScheduleTargetV2::Mechanistic {
            actions: vec![codex_team_protocol::MechanisticActionV1::GitHistory { max_entries: 4 }],
        };
        normalize_at_confirmation(definition.clone(), Utc::now()).expect("pure query schedule");

        if let ScheduleTargetV2::Mechanistic { actions } = &mut definition.target {
            actions.push(codex_team_protocol::MechanisticActionV1::Evidence {
                recipe: codex_team_protocol::EvidenceRecipeV1::CargoTest { filter: None },
                cwd: None,
                timeout_seconds: Some(10),
            });
        }
        normalize_at_confirmation(definition.clone(), Utc::now())
            .expect("bounded mixed query/evidence schedule");

        definition.target = ScheduleTargetV2::Mechanistic {
            actions: vec![codex_team_protocol::MechanisticActionV1::ObserveProcess {
                label: "unsafe-persisted-pid".into(),
                pid: 42,
                interval_seconds: 10,
            }],
        };
        assert!(normalize_at_confirmation(definition, Utc::now()).is_err());
    }

    #[test]
    fn approved_automation_workflow_schedule_is_model_free_and_digest_bound() {
        let root = tempfile::tempdir().unwrap();
        let mut definition = absolute_definition(root.path());
        definition.provider = None;
        definition.model = None;
        definition.allow_network = true;
        definition.target = ScheduleTargetV2::AutomationWorkflow {
            workflow_id: "seedbox_archive".into(),
            workflow_digest: "a".repeat(64),
            resource_binding_digest: "b".repeat(64),
        };
        normalize_at_confirmation(definition.clone(), Utc::now())
            .expect("approved workflow schedule");

        definition.allow_network = false;
        assert!(normalize_at_confirmation(definition.clone(), Utc::now()).is_err());
        definition.allow_network = true;
        definition.provider = Some("fake".into());
        assert!(normalize_at_confirmation(definition.clone(), Utc::now()).is_err());
        definition.provider = None;
        definition.target = ScheduleTargetV2::AutomationWorkflow {
            workflow_id: "seedbox_archive".into(),
            workflow_digest: "not-a-digest".into(),
            resource_binding_digest: "b".repeat(64),
        };
        assert!(normalize_at_confirmation(definition, Utc::now()).is_err());
    }

    #[test]
    fn interactive_only_tools_are_rejected_by_v2_schedules() {
        let root = tempfile::tempdir().unwrap();
        for tool in [
            crate::privilege::TOOL_NAME,
            crate::process_watch::WATCH_PROCESS_TOOL_NAME,
            crate::process_watch::STOP_WATCHER_TOOL_NAME,
        ] {
            let mut definition = absolute_definition(root.path());
            definition.tools.push(tool.into());
            assert!(validate_tools_and_grants(&definition).is_err());
        }
    }

    #[test]
    fn delay_and_interval_are_normalized_at_confirmation() {
        let root = tempfile::tempdir().unwrap();
        let now = "2026-08-13T12:00:00Z".parse().unwrap();
        let mut delay = absolute_definition(root.path());
        delay.trigger = ScheduleTriggerV2::Delay { seconds: 90 };
        let normalized = normalize_at_confirmation(delay, now).unwrap();
        assert_eq!(
            normalized.trigger,
            ScheduleTriggerV2::Absolute {
                at: "2026-08-13T12:01:30Z".parse().unwrap()
            }
        );
        let mut interval = absolute_definition(root.path());
        interval.trigger = ScheduleTriggerV2::Interval {
            seconds: 60,
            anchor: None,
        };
        let normalized = normalize_at_confirmation(interval, now).unwrap();
        assert_eq!(
            normalized.trigger,
            ScheduleTriggerV2::Interval {
                seconds: 60,
                anchor: Some(now)
            }
        );
    }

    #[test]
    fn cron_skips_dst_gap_and_chooses_earlier_fold_once() {
        let spring = ScheduleTriggerV2::Cron {
            expression: "30 2 * * *".into(),
            timezone: "America/New_York".into(),
        };
        let next = next_occurrence(&spring, "2026-03-08T06:00:00Z".parse().unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(next.at.to_rfc3339(), "2026-03-09T06:30:00+00:00");

        let fall = ScheduleTriggerV2::Cron {
            expression: "30 1 * * *".into(),
            timezone: "America/New_York".into(),
        };
        let first = next_occurrence(&fall, "2026-11-01T04:00:00Z".parse().unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(first.at.to_rfc3339(), "2026-11-01T05:30:00+00:00");
        let second = next_occurrence(&fall, first.at).unwrap().unwrap();
        assert_eq!(second.at.to_rfc3339(), "2026-11-02T06:30:00+00:00");
    }

    #[test]
    fn missed_interval_is_skipped_or_caught_up_at_most_once() {
        let anchor: DateTime<Utc> = "2026-08-13T12:00:00Z".parse().unwrap();
        let trigger = ScheduleTriggerV2::Interval {
            seconds: 60,
            anchor: Some(anchor),
        };
        let durable = next_occurrence(
            &trigger,
            anchor.checked_sub_signed(TimeDelta::seconds(1)).unwrap(),
        )
        .unwrap()
        .unwrap();
        let now = anchor + TimeDelta::minutes(10);
        assert!(
            due_occurrence(&trigger, &durable, now, MissedRunPolicy::Skip)
                .unwrap()
                .is_none()
        );
        assert_eq!(
            due_occurrence(&trigger, &durable, now, MissedRunPolicy::OneCatchUp)
                .unwrap()
                .unwrap(),
            durable
        );
        assert_eq!(
            next_occurrence(&trigger, now).unwrap().unwrap().at,
            now + TimeDelta::minutes(1)
        );
    }

    #[test]
    fn store_requires_exact_digest_and_starts_disabled() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let now = "2026-08-13T12:00:00Z".parse().unwrap();
        let definition =
            normalize_at_confirmation(absolute_definition(workspace.path()), now).unwrap();
        let digest = definition_digest(&definition).unwrap();
        let store = ScheduleStoreV2::new(root.path().join("schedules"));
        assert!(matches!(
            store.add_confirmed(definition.clone(), "bad", now),
            Err(ScheduleV2Error::ConfirmationRequired(_))
        ));
        let record = store.add_confirmed(definition, &digest, now).unwrap();
        assert!(!record.enabled);
        assert!(record.next_occurrence.is_none());
    }

    #[test]
    fn journal_is_hash_chained_and_rejects_illegal_transition() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let definition = absolute_definition(workspace.path());
        let claim = RunClaim {
            run_id: Uuid::new_v4(),
            profile: "default".into(),
            schedule_id: definition.id.clone(),
            definition_digest: definition_digest(&definition).unwrap(),
            occurrence: Occurrence {
                id: "occurrence".into(),
                at: Utc::now(),
            },
            claim_revision: 1,
            definition,
        };
        let journal = RunJournal::new(root.path().join("journal"));
        journal.create_claim(&claim, Utc::now()).unwrap();
        let running = journal
            .transition(
                claim.run_id,
                ScheduleRunState::Running,
                ReceiptUpdate {
                    phase: "running".into(),
                    ..Default::default()
                },
                Utc::now(),
            )
            .unwrap();
        assert_eq!(
            running.previous_event_hash.as_deref(),
            journal
                .history(None)
                .unwrap()
                .first()
                .map(|event| event.event_hash.as_str())
        );
        assert!(journal
            .transition(
                claim.run_id,
                ScheduleRunState::Claimed,
                ReceiptUpdate::default(),
                Utc::now()
            )
            .is_err());
    }

    #[test]
    fn dead_parent_recovery_preserves_claim_while_native_child_holds_executor_lease() {
        struct FixtureWorker(std::process::Child);
        impl Drop for FixtureWorker {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().canonicalize().unwrap();
        let now = Utc::now();
        let definition = absolute_definition(&root);
        let digest = definition_digest(&definition).unwrap();
        let store = ScheduleStoreV2::new(root.join("schedules"));
        store.add_confirmed(definition, &digest, now).unwrap();
        let journal = RunJournal::new(root.join("schedules/schedule-runs-v2"));
        let claim = store
            .claim_immediate("default", &journal, "nightly", now)
            .unwrap();
        let mut dead_parent = std::process::Command::new("/usr/bin/true").spawn().unwrap();
        let dead_pid = dead_parent.id();
        assert!(dead_parent.wait().unwrap().success());
        assert!(!process_is_alive(dead_pid).unwrap());
        // Rewrite only the temporary fixture's first event to represent a
        // genuinely exited dispatching parent, retaining the valid hash chain.
        let mut first = journal.latest(claim.run_id).unwrap().unwrap();
        first.owner_pid = dead_pid;
        first.event_hash = receipt_hash(&first).unwrap();
        let event = journal
            .root
            .join(claim.run_id.to_string())
            .join("00000000000000000000-claimed.json");
        std::fs::write(event, serde_json::to_vec(&first).unwrap()).unwrap();
        journal
            .transition(
                claim.run_id,
                ScheduleRunState::Running,
                ReceiptUpdate {
                    phase: "contained_worker".into(),
                    ..Default::default()
                },
                now,
            )
            .unwrap();
        let native_root = root.join("automation-engine/v2");
        let context = ca_automation::runtime::ExecutionContext {
            schema: ca_automation::runtime::EXECUTION_SCHEMA.into(),
            run_id: claim.run_id.to_string(),
            profile: "default".into(),
            state_root: native_root.clone(),
            lease_root: root.join("resources"),
            definition_digest: digest,
            resource_binding_digest: "b".repeat(64),
            policy_digest: "c".repeat(64),
            resources: Vec::new(),
            runtime_seconds: 1,
            output_bytes: 4096,
        };
        let native = ca_automation::runtime::RunStore::open(&context).unwrap();
        native.start().unwrap();
        native.prepare_action("target").unwrap();
        assert!(native.begin_action("target").unwrap());
        drop(native.execution_lease().unwrap());
        let ready = root.join("worker-ready");
        let mut worker = FixtureWorker(
            std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "schedule_v2::tests::native_executor_fixture_worker",
                    "--ignored",
                    "--nocapture",
                ])
                .env("CA_V2_EXECUTOR_FIXTURE_ROOT", &native_root)
                .env("CA_V2_EXECUTOR_FIXTURE_RUN", claim.run_id.to_string())
                .env("CA_V2_EXECUTOR_FIXTURE_READY", &ready)
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !ready.exists() {
            assert!(
                worker.0.try_wait().unwrap().is_none(),
                "fixture executor exited before owning its lease"
            );
            assert!(
                std::time::Instant::now() < deadline,
                "fixture executor did not start"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            native.reconcile().unwrap().state,
            ca_automation::runtime::ExecutionState::Running
        );
        #[cfg(unix)]
        {
            let alias = root.join("profile-alias");
            std::os::unix::fs::symlink(&root, &alias).unwrap();
            assert!(native_executor_is_alive(
                &alias.join("schedules"),
                &journal.latest(claim.run_id).unwrap().unwrap()
            )
            .unwrap());
        }
        let future = now + TimeDelta::days(365);
        // Terminal workspace metadata must also retain ownership while the
        // same contained executor survives its dispatching parent. Use a
        // separate fixture journal so the active occurrence still exercises
        // interrupted recovery after the child exits.
        let lease_journal = RunJournal::new(root.join("terminal-lease-journal"));
        lease_journal.create_claim(&claim, now).unwrap();
        let mut terminal_first = lease_journal.latest(claim.run_id).unwrap().unwrap();
        terminal_first.owner_pid = dead_pid;
        terminal_first.event_hash = receipt_hash(&terminal_first).unwrap();
        let terminal_event = lease_journal
            .root
            .join(claim.run_id.to_string())
            .join("00000000000000000000-claimed.json");
        std::fs::write(terminal_event, serde_json::to_vec(&terminal_first).unwrap()).unwrap();
        for state in [ScheduleRunState::Running, ScheduleRunState::Succeeded] {
            lease_journal
                .transition(claim.run_id, state, ReceiptUpdate::default(), now)
                .unwrap();
        }
        let lease_manager = LeaseManager::new(root.join("schedule-private-v2"), 4096);
        let workspace_lease = lease_manager
            .acquire(claim.run_id, &root, 4096, now)
            .unwrap();
        let mut lease_receipt: LeaseReceipt =
            serde_json::from_slice(&std::fs::read(&workspace_lease.path).unwrap()).unwrap();
        lease_receipt.pid = dead_pid;
        lease_receipt.process_start = terminal_first.owner_process_start.clone();
        std::fs::write(
            &workspace_lease.path,
            serde_json::to_vec(&lease_receipt).unwrap(),
        )
        .unwrap();
        let before = store.load().unwrap().revision;
        for _ in 0..2 {
            assert_eq!(
                lease_manager.recover_terminal_dead(&lease_journal).unwrap(),
                0
            );
            assert!(workspace_lease.path.exists());
            assert_eq!(store.recover_dead_owners(&journal, future).unwrap(), 0);
            let record = store.records().unwrap().remove(0);
            assert_eq!(record.active_run_id, Some(claim.run_id));
            assert_eq!(record.claim_revision, Some(claim.claim_revision));
            assert_eq!(
                journal.latest(claim.run_id).unwrap().unwrap().state,
                ScheduleRunState::Running
            );
            assert_eq!(store.load().unwrap().revision, before);
            assert!(store
                .claim_immediate("default", &journal, "nightly", future)
                .is_err());
        }
        let context_file = native_root
            .join("runs")
            .join(claim.run_id.to_string())
            .join("context.json");
        let original = std::fs::read(&context_file).unwrap();
        std::fs::write(&context_file, b"corrupt fixture context").unwrap();
        assert!(store.recover_dead_owners(&journal, future).is_err());
        assert!(lease_manager.recover_terminal_dead(&lease_journal).is_err());
        assert!(workspace_lease.path.exists());
        assert_eq!(
            store.records().unwrap()[0].active_run_id,
            Some(claim.run_id)
        );
        std::fs::write(&context_file, original).unwrap();
        drop(worker.0.stdin.take());
        assert!(worker.0.wait().unwrap().success());
        assert_eq!(
            lease_manager.recover_terminal_dead(&lease_journal).unwrap(),
            1
        );
        assert!(!workspace_lease.path.exists());
        assert_eq!(store.recover_dead_owners(&journal, future).unwrap(), 1);
        assert_eq!(store.records().unwrap()[0].active_run_id, None);
        assert_eq!(
            journal.latest(claim.run_id).unwrap().unwrap().state,
            ScheduleRunState::Interrupted
        );
    }

    #[test]
    #[ignore = "subprocess fixture invoked only by the native executor ownership test"]
    fn native_executor_fixture_worker() {
        let root =
            PathBuf::from(std::env::var_os("CA_V2_EXECUTOR_FIXTURE_ROOT").expect("fixture root"));
        let run_id = std::env::var("CA_V2_EXECUTOR_FIXTURE_RUN").expect("fixture run");
        let ready =
            PathBuf::from(std::env::var_os("CA_V2_EXECUTOR_FIXTURE_READY").expect("fixture ready"));
        let context = ca_automation::runtime::RunStore::load_context(&root, &run_id).unwrap();
        let store = ca_automation::runtime::RunStore::open(&context).unwrap();
        let _owner = store.execution_lease().unwrap();
        std::fs::write(ready, b"ready").unwrap();
        std::io::stdin().read_to_end(&mut Vec::new()).unwrap();
    }

    #[test]
    fn leases_enforce_global_workspace_and_byte_caps() {
        let root = tempfile::tempdir().unwrap();
        let first_workspace = tempfile::tempdir().unwrap();
        let second_workspace = tempfile::tempdir().unwrap();
        let manager = LeaseManager::new(root.path().to_path_buf(), 100);
        let deadline = Utc::now() + TimeDelta::minutes(1);
        let first = manager
            .acquire(Uuid::new_v4(), first_workspace.path(), 40, deadline)
            .unwrap();
        assert!(matches!(
            manager.acquire(Uuid::new_v4(), first_workspace.path(), 10, deadline),
            Err(ScheduleV2Error::Busy)
        ));
        let second = manager
            .acquire(Uuid::new_v4(), second_workspace.path(), 60, deadline)
            .unwrap();
        let third_workspace = tempfile::tempdir().unwrap();
        assert!(matches!(
            manager.acquire(Uuid::new_v4(), third_workspace.path(), 1, deadline),
            Err(ScheduleV2Error::Busy)
        ));
        drop((first, second));
        assert!(manager
            .acquire(Uuid::new_v4(), third_workspace.path(), 100, deadline)
            .is_ok());
    }

    #[test]
    fn email_draft_requires_canonical_frozen_recipients() {
        let delivery = ScheduleDeliveryV2::EmailDraft {
            account: "mail".into(),
            recipients: vec!["a@example.com".into()],
        };
        assert!(serde_json::to_string(&delivery)
            .unwrap()
            .contains("email_draft"));
        assert!(validate_delivery(&delivery).is_ok());
        assert!(validate_delivery(&ScheduleDeliveryV2::EmailDraft {
            account: "mail".into(),
            recipients: vec!["z@example.com".into(), "a@example.com".into()]
        })
        .is_err());
    }

    #[test]
    fn launch_agent_preview_is_deterministic_and_secret_free() {
        let home = tempfile::tempdir().unwrap();
        let executable = std::env::current_exe().unwrap().canonicalize().unwrap();
        let paths = crate::runtime::RuntimePaths::from_home(home.path());
        let one =
            launch_agent_preview(&paths, &crate::runtime::ActiveProfile::Default, &executable)
                .unwrap();
        let two =
            launch_agent_preview(&paths, &crate::runtime::ActiveProfile::Default, &executable)
                .unwrap();
        assert_eq!(one.digest, two.digest);
        let text = String::from_utf8(one.xml).unwrap();
        assert!(text.contains("<string>--drain</string>"));
        assert!(!text.to_ascii_lowercase().contains("secret"));
        assert!(!text.contains("RunAtLoad</key><true"));
    }

    #[test]
    fn recovery_archive_reconstructs_exactly_before_workspace_cleanup() {
        let source = tempfile::tempdir().unwrap();
        std::fs::write(source.path().join("original.txt"), "baseline").unwrap();
        let private = tempfile::tempdir().unwrap();
        let artifacts = tempfile::tempdir().unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(private.path(), std::fs::Permissions::from_mode(0o700))
                .unwrap();
            std::fs::set_permissions(artifacts.path(), std::fs::Permissions::from_mode(0o700))
                .unwrap();
        }
        let allowlist =
            crate::workspace_materialization::ScheduledTrackedAllowlist::new([PathBuf::from(
                "original.txt",
            )])
            .unwrap();
        let workspace = crate::workspace_materialization::materialize_scheduled_workspace(
            &source.path().canonicalize().unwrap(),
            &private.path().canonicalize().unwrap(),
            "schedule-test",
            &crate::workspace_materialization::MaterializationPolicy::scheduled(),
            crate::workspace_materialization::ClonePreference::CopyOnly,
            &allowlist,
        )
        .unwrap();
        let published = workspace.path().to_path_buf();
        std::fs::write(workspace.path().join("original.txt"), "changed").unwrap();
        std::fs::create_dir(workspace.path().join("new-dir")).unwrap();
        std::fs::write(workspace.path().join("new-dir/new.bin"), [0_u8, 255, 3]).unwrap();
        let prepared = PreparedWorkspace {
            workspace,
            pinned_revision: "a".repeat(40),
            tracked_manifest_digest: "b".repeat(64),
            source_repo_digest: "c".repeat(64),
        };
        let run_id = Uuid::new_v4();
        let proof = export_verify_and_cleanup(prepared, artifacts.path(), run_id).unwrap();
        assert!(!published.exists());
        assert_eq!(proof.digest.len(), 64);
        assert!(proof.size > 0);
        assert!(artifacts
            .path()
            .join(format!("{run_id}.recovery.json"))
            .is_file());
    }

    #[test]
    fn pinned_git_materialization_uses_exact_tracked_tree_and_leaves_source_bytes_unchanged() {
        let source = tempfile::tempdir().unwrap();
        std::fs::write(source.path().join("tracked.txt"), "tracked bytes").unwrap();
        let git = |args: &[&str]| {
            let status = std::process::Command::new(git_executable().unwrap())
                .args(args)
                .current_dir(source.path())
                .env_clear()
                .env("PATH", "/usr/local/bin:/usr/bin:/bin")
                .status()
                .unwrap();
            assert!(status.success(), "git command failed: {args:?}");
        };
        git(&["init", "-q"]);
        git(&["add", "tracked.txt"]);
        git(&[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "-qm",
            "baseline",
        ]);
        let revision = git_text(source.path(), &["rev-parse", "HEAD"]).unwrap();
        let private = tempfile::tempdir().unwrap();
        let artifacts = tempfile::tempdir().unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(private.path(), std::fs::Permissions::from_mode(0o700))
                .unwrap();
            std::fs::set_permissions(artifacts.path(), std::fs::Permissions::from_mode(0o700))
                .unwrap();
        }
        let mut definition = absolute_definition(source.path());
        definition.workspace_policy = WorkspacePolicy::IsolatedPinned;
        definition.pinned_revision = Some(revision.clone());
        let prepared = materialize_isolated_workspace(&definition, private.path()).unwrap();
        assert_eq!(prepared.pinned_revision, revision);
        assert_eq!(
            std::fs::read_to_string(prepared.workspace.path().join("tracked.txt")).unwrap(),
            "tracked bytes"
        );
        assert!(!prepared.workspace.path().join(".git").exists());
        assert_eq!(
            std::fs::read_to_string(source.path().join("tracked.txt")).unwrap(),
            "tracked bytes"
        );
        export_verify_and_cleanup(prepared, artifacts.path(), Uuid::new_v4()).unwrap();
        assert_eq!(
            git_text(source.path(), &["status", "--porcelain=v1"]).unwrap(),
            ""
        );
    }

    #[tokio::test]
    async fn script_only_uses_exact_argv_and_a_clean_environment() {
        let root = tempfile::tempdir().unwrap();
        let executable = root.path().join("fake-script");
        std::fs::write(
            &executable,
            "#!/bin/sh\nprintf '%s|%s' \"$1\" \"${HOME-unset}\"\n",
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let executable = executable.canonicalize().unwrap();
        let definition = ScheduleDefinitionV2 {
            version: 2,
            id: "script".into(),
            provider: None,
            model: None,
            workspace: root.path().canonicalize().unwrap(),
            runtime_secs: 10,
            output_bytes: 1024,
            tools: Vec::new(),
            grants: Vec::new(),
            allow_network: false,
            skills: Vec::new(),
            workspace_policy: WorkspacePolicy::IsolatedPinned,
            pinned_revision: Some("a".repeat(40)),
            trigger: ScheduleTriggerV2::Absolute { at: Utc::now() },
            missed_run_policy: MissedRunPolicy::Skip,
            target: ScheduleTargetV2::ScriptOnly {
                executable: executable.clone(),
                args: vec!["literal argument".into()],
                executable_sha256: hash_file(&executable, 1024 * 1024).unwrap(),
                scrub_environment: true,
            },
            delivery: ScheduleDeliveryV2::None,
        };
        let output = execute_script_only(
            &definition,
            root.path(),
            crate::sandbox::SandboxMode::PolicyOnly,
        )
        .await
        .unwrap();
        assert_eq!(output.stdout, b"literal argument|unset");
    }

    #[cfg(unix)]
    fn bounded_script_fixture(root: &Path, source: &str) -> ScheduleDefinitionV2 {
        use std::os::unix::fs::PermissionsExt;
        let executable = root.join("bounded-script");
        std::fs::write(&executable, source).unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let executable = executable.canonicalize().unwrap();
        serde_json::from_value(serde_json::json!({
            "version": 2, "id": "bounded-script", "workspace": root.canonicalize().unwrap(),
            "runtime_secs": 10, "output_bytes": 1024, "workspace_policy": "isolated_pinned",
            "pinned_revision": "a".repeat(40), "trigger": {"kind": "absolute", "at": Utc::now()},
            "target": {"kind": "script_only", "executable": executable,
                "args": [], "executable_sha256": hash_file(&executable, 1024 * 1024).unwrap(), "scrub_environment": true},
            "delivery": {"kind": "none"}
        })).unwrap()
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn required_script_keeps_private_receipts_inaccessible() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().canonicalize().unwrap();
        let workspace = root.join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        let private = root.join("private-receipt");
        std::fs::write(&private, b"original-authority").unwrap();
        let definition = bounded_script_fixture(
            &workspace,
            &format!(
                "#!/bin/sh\nif /bin/cat '{}' >/dev/null 2>&1; then exit 70; fi\nif printf forged >'{}' 2>/dev/null; then exit 71; fi\nprintf permitted > allowed\nprintf '%s' \"${{HOME-unset}}\"\n",
                private.display(),
                private.display()
            ),
        );
        let output = crate::jobs::AUTOMATION_PRIVATE_PATHS
            .scope(
                vec![private.clone()],
                execute_script_only(
                    &definition,
                    &workspace,
                    crate::sandbox::SandboxMode::Required,
                ),
            )
            .await
            .unwrap();
        assert_eq!(output.stdout, b"unset");
        assert_eq!(std::fs::read(private).unwrap(), b"original-authority");
        assert_eq!(
            std::fs::read(workspace.join("allowed")).unwrap(),
            b"permitted"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn script_rejects_exhausted_output_before_launch_and_bounds_oversized_capture() {
        let root = tempfile::tempdir().unwrap();
        let mut definition = bounded_script_fixture(
            root.path(),
            "#!/bin/sh\nprintf launched > launched\nprintf '%04096d' 0\n",
        );
        definition.output_bytes = 0;
        assert!(execute_script_only(
            &definition,
            root.path(),
            crate::sandbox::SandboxMode::PolicyOnly
        )
        .await
        .is_err());
        assert!(!root.path().join("launched").exists());
        definition.output_bytes = 16;
        let error = execute_script_only(
            &definition,
            root.path(),
            crate::sandbox::SandboxMode::PolicyOnly,
        )
        .await
        .unwrap_err();
        assert!(root.path().join("launched").exists());
        assert!(error.to_string().contains("output exceeded its bound"));
        assert!(!error.to_string().contains("00000000"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn dropped_script_drains_native_group_before_workspace_restore() {
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().canonicalize().unwrap();
        let definition = bounded_script_fixture(&workspace,
            "#!/bin/sh\ntrap 'exit 130' INT\n/bin/sleep 30 &\nprintf '%s' \"$$\" > script-pid\nprintf '%s' \"$!\" > native-pid\nwait\n");
        let execution_workspace = workspace.clone();
        let execution = tokio::spawn(async move {
            execute_script_only(
                &definition,
                &execution_workspace,
                crate::sandbox::SandboxMode::PolicyOnly,
            )
            .await
        });
        tokio::time::timeout(Duration::from_secs(2), async {
            while !workspace.join("native-pid").exists() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        let group: i32 = std::fs::read_to_string(workspace.join("script-pid"))
            .unwrap()
            .parse()
            .unwrap();
        let native: i32 = std::fs::read_to_string(workspace.join("native-pid"))
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(unsafe { libc::getpgid(native) }, group);
        let store = crate::recovery::RecoveryStore::new(workspace.join("unused-profile"));
        assert!(store
            .try_workspace_restore(std::slice::from_ref(&workspace))
            .is_err());
        execution.abort();
        assert!(execution.await.unwrap_err().is_cancelled());
        tokio::time::timeout(Duration::from_secs(8), async {
            loop {
                if unsafe { libc::kill(-group, 0) } != 0 {
                    match std::io::Error::last_os_error().raw_os_error() {
                        // Darwin can report EPERM while a zombie-only owned
                        // group still exists. Ownership must remain held until
                        // the kernel confirms that the group is absent.
                        Some(libc::EPERM) => {}
                        Some(libc::ESRCH) => {
                            if let Ok(lease) =
                                store.try_workspace_restore(std::slice::from_ref(&workspace))
                            {
                                drop(lease);
                                break;
                            }
                        }
                        error => panic!("unexpected owned process-group probe error: {error:?}"),
                    }
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert!(!workspace.join("unused-profile").exists());
    }

    #[test]
    fn fake_launchctl_lifecycle_requires_exact_preview_and_removes_only_matching_plist() {
        let home = tempfile::tempdir().unwrap();
        let log = home.path().join("launchctl.log");
        let fake = home.path().join("fake-launchctl");
        std::fs::write(
            &fake,
            format!("#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\n", log.display()),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let executable = std::env::current_exe().unwrap().canonicalize().unwrap();
        let paths = crate::runtime::RuntimePaths::from_home(home.path());
        let preview = launch_agent_preview(
            &paths,
            &crate::runtime::ActiveProfile::Named("work".into()),
            &executable,
        )
        .unwrap();
        assert!(matches!(
            service_mutation(ServiceAction::Install, &preview, Some("wrong"), &fake),
            Err(ScheduleV2Error::ConfirmationRequired(_))
        ));
        assert_eq!(
            service_mutation(
                ServiceAction::Install,
                &preview,
                Some(&preview.digest),
                &fake
            )
            .unwrap(),
            "installed"
        );
        assert!(preview.path.is_file());
        assert_eq!(
            service_mutation(ServiceAction::Status, &preview, None, &fake).unwrap(),
            "loaded"
        );
        assert_eq!(
            service_mutation(
                ServiceAction::Uninstall,
                &preview,
                Some(&preview.digest),
                &fake
            )
            .unwrap(),
            "uninstalled"
        );
        assert!(!preview.path.exists());
        let calls = std::fs::read_to_string(log).unwrap();
        assert!(calls.contains("bootstrap"));
        assert!(calls.contains("bootout"));
    }
}
