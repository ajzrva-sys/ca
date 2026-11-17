//! Model-facing wrappers over the session work state, goal, plan mode, and schedule store.
use crate::goal::{GoalState, GoalStatus};
use crate::schedule_v2::ScheduleStoreV2;
use crate::work::{SessionTodo, WorkState};
use chrono::Utc;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::{Arc, Mutex};

tokio::task_local! {
    pub static WORK: Arc<Mutex<WorkState>>;
}

const TEN_HOURS_MS: u64 = 10 * 60 * 60 * 1000;

pub fn todo_write(todos: &mut Vec<SessionTodo>, arguments: &Value) -> Result<String, String> {
    let merge = arguments
        .get("merge")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let items = arguments
        .get("todos")
        .and_then(Value::as_array)
        .ok_or("todos must be a list")?;
    let mut incoming = Vec::new();
    for item in items {
        let id = item
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .ok_or("todo id is required")?
            .to_string();
        let status = item
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("pending");
        if !matches!(
            status,
            "pending" | "in_progress" | "completed" | "cancelled"
        ) {
            return Err(format!("invalid todo status {status}"));
        }
        let content = item
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        incoming.push(SessionTodo {
            id,
            content,
            status: status.to_string(),
        });
    }
    if !merge {
        *todos = incoming;
    } else {
        for item in incoming {
            if let Some(existing) = todos.iter_mut().find(|todo| todo.id == item.id) {
                if !item.content.is_empty() {
                    existing.content = item.content;
                }
                existing.status = item.status;
            } else {
                todos.push(item);
            }
        }
    }
    Ok(json!({"todos": todos}).to_string())
}

pub fn update_goal(goal: &mut GoalState, arguments: &Value) -> Result<String, String> {
    let now = Utc::now();
    if let Some(reason) = arguments
        .get("blocked_reason")
        .and_then(Value::as_str)
        .filter(|reason| !reason.is_empty())
    {
        if !goal.pause(crate::goal::GoalPauseReason::User, now) {
            return Err("goal is not active".into());
        }
        return Ok(format!("Goal paused: {reason}"));
    }
    if arguments
        .get("completed")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        goal.stop(GoalStatus::Complete, now)
            .map_err(|error| error.to_string())?;
        let message = arguments
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("");
        return Ok(if message.is_empty() {
            "Goal completed".into()
        } else {
            format!("Goal completed: {message}")
        });
    }
    if let Some(message) = arguments.get("message").and_then(Value::as_str) {
        goal.updated_at = now;
        return Ok(format!("Goal progress: {message}"));
    }
    Err("update_goal needs completed, message, or blocked_reason".into())
}

pub fn enter_plan_mode(work: &mut WorkState) -> String {
    work.session_mode = Some("plan".into());
    "[SWITCH_MODE:plan]".into()
}

pub fn exit_plan_mode(work: &mut WorkState) -> String {
    work.session_mode = Some("agent".into());
    "[SWITCH_MODE:agent]".into()
}

pub fn parse_interval(interval: &str) -> Result<u64, String> {
    let interval = interval.trim();
    let (number, unit) = interval.split_at(interval.len().saturating_sub(1));
    let count: u64 = number
        .parse()
        .map_err(|_| format!("invalid interval {interval}"))?;
    let seconds = match unit {
        "m" => count.saturating_mul(60),
        "h" => count.saturating_mul(60 * 60),
        "d" => count.saturating_mul(24 * 60 * 60),
        _ => return Err(format!("invalid interval {interval}")),
    };
    if seconds == 0 {
        return Err(format!("invalid interval {interval}"));
    }
    Ok(seconds)
}

pub fn scheduler_create(
    store: &ScheduleStoreV2,
    workspace: &Path,
    arguments: &Value,
) -> Result<String, String> {
    let task_id = arguments
        .get("task_id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty());
    let prompt = arguments.get("prompt").and_then(Value::as_str);
    let interval = arguments.get("interval").and_then(Value::as_str);
    let id = if let Some(task_id) = task_id {
        task_id.to_string()
    } else {
        format!("sched-{}", uuid::Uuid::new_v4().simple())
    };
    let existing = store
        .records()
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|record| record.definition.id == id);
    let prompt = match (prompt, existing.as_ref()) {
        (Some(prompt), _) if !prompt.is_empty() => prompt.to_string(),
        (_, Some(record)) => match &record.definition.target {
            crate::schedule_v2::ScheduleTargetV2::Prompt { prompt } => prompt.clone(),
            _ => return Err("schedule is not a prompt".into()),
        },
        _ => return Err("prompt is required".into()),
    };
    let seconds = match (interval, existing.as_ref()) {
        (Some(interval), _) => parse_interval(interval)?,
        (_, Some(record)) => match record.definition.trigger {
            crate::schedule_v2::ScheduleTriggerV2::Interval { seconds, .. } => seconds,
            _ => return Err("schedule is not an interval".into()),
        },
        _ => return Err("interval is required".into()),
    };
    let fire_immediately = arguments
        .get("fire_immediately")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let record = store
        .save_interval_prompt(
            &id,
            workspace,
            seconds,
            &prompt,
            fire_immediately,
            Utc::now(),
        )
        .map_err(|error| error.to_string())?;
    Ok(record.definition.id)
}

pub fn scheduler_list(store: &ScheduleStoreV2) -> Result<String, String> {
    let records = store.records().map_err(|error| error.to_string())?;
    let rows: Vec<_> = records
        .iter()
        .map(|record| {
            json!({
                "task_id": record.definition.id,
                "prompt": match &record.definition.target {
                    crate::schedule_v2::ScheduleTargetV2::Prompt { prompt } => prompt.as_str(),
                    _ => "",
                },
            })
        })
        .collect();
    Ok(json!({"schedules": rows}).to_string())
}

pub fn scheduler_delete(store: &ScheduleStoreV2, arguments: &Value) -> Result<String, String> {
    let id = arguments
        .get("task_id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .ok_or("task_id is required")?;
    let removed = store.delete(id).map_err(|error| error.to_string())?;
    if removed {
        Ok(format!("Deleted schedule {id}"))
    } else {
        Err(format!("Unknown schedule {id}"))
    }
}

pub fn monitor_timeout_ms(arguments: &Value) -> Result<u64, String> {
    let persistent = arguments
        .get("persistent")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if persistent {
        return Ok(TEN_HOURS_MS);
    }
    let timeout = arguments
        .get("timeout_ms")
        .and_then(Value::as_u64)
        .unwrap_or(TEN_HOURS_MS);
    if timeout == 0 || timeout > TEN_HOURS_MS {
        return Err("timeout_ms must be from 1 through 36000000".into());
    }
    Ok(timeout)
}

pub fn schedule_store_dir() -> std::path::PathBuf {
    if let Ok(dir) = std::env::var("CA_SCHEDULE_DIR") {
        if !dir.is_empty() {
            return std::path::PathBuf::from(dir);
        }
    }
    std::env::var_os("HOME")
        .map(|home| {
            crate::runtime::RuntimePaths::from_home(home)
                .schedules_dir(&crate::runtime::ActiveProfile::Default)
        })
        .unwrap_or_else(|| std::path::PathBuf::from(".ca-schedules"))
}

pub async fn monitor_command(
    command: &str,
    cwd: &Path,
    timeout_ms: u64,
    owner: crate::jobs::JobOwner,
) -> Result<crate::jobs::JobId, String> {
    crate::jobs::JobManager::process()
        .start_sandboxed_owned(
            command,
            cwd.to_string_lossy(),
            true,
            std::time::Duration::from_millis(timeout_ms),
            vec![cwd.to_path_buf()],
            false,
            crate::sandbox::SandboxMode::PolicyOnly,
            owner,
        )
        .await
}

/// Copies the `/goal` goal into the work state the tool handler mutates.
pub fn publish_goal(state: &crate::app::AppState) {
    install_goal(&state.work_state, state.goal_state.clone());
}

/// Assigns one goal snapshot into the work state `update_goal` reads.
/// `None` leaves the current goal in place so a later tool in the same batch
/// does not reset an earlier mutation.
pub fn install_goal(work_state: &Arc<Mutex<WorkState>>, goal: Option<crate::goal::GoalState>) {
    if let Some(goal) = goal {
        lock_work(work_state).session_goal = Some(goal);
    }
}

pub fn apply_session_mirror(state: &mut crate::app::AppState) {
    let Ok(work) = state.work_state.lock() else {
        return;
    };
    if let Some(goal) = work.session_goal.clone() {
        state.goal_state = Some(goal);
    }
    if let Some(mode) = work.session_mode.as_deref() {
        state.mode = match mode {
            "plan" => crate::app::AgentMode::Plan,
            "ask" => crate::app::AgentMode::Ask,
            "debug" => crate::app::AgentMode::Debug,
            "agent" => crate::app::AgentMode::Agent,
            other => crate::app::AgentMode::Custom(other.to_string()),
        };
    }
}

pub fn lock_work(work_state: &Arc<Mutex<WorkState>>) -> std::sync::MutexGuard<'_, WorkState> {
    work_state
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::tools::execute_tool_with_context;
    use crate::goal::GoalLimits;
    use crate::work::PlanStatus;
    use serde_json::json;

    fn context(cwd: &Path) -> crate::agent::registry::ToolContext {
        crate::agent::registry::ToolContext {
            recovery: None,
            cwd: cwd.to_path_buf(),
            timeout_secs: 2,
            profile: crate::agent::registry::ToolProfile::Main,
            workspace_roots: vec![cwd.to_path_buf()],
            sandbox_mode: crate::sandbox::SandboxMode::PolicyOnly,
            network_allowed: false,
            job_owner: crate::jobs::JobOwner::process(),
            job_execution_policy: crate::jobs::JobExecutionPolicy::default(),
        }
    }

    #[test]
    fn todo_write_merge_updates_one_id_and_replace_drops_the_rest() {
        let mut todos = vec![
            SessionTodo {
                id: "a".into(),
                content: "keep".into(),
                status: "pending".into(),
            },
            SessionTodo {
                id: "b".into(),
                content: "old".into(),
                status: "pending".into(),
            },
        ];
        todo_write(
            &mut todos,
            &json!({"todos":[{"id":"b","status":"completed"}]}),
        )
        .unwrap();
        assert_eq!(todos[0].content, "keep");
        assert_eq!(todos[1].status, "completed");
        assert_eq!(todos[1].content, "old");
        todo_write(
            &mut todos,
            &json!({"merge": false, "todos":[{"id":"c","content":"only","status":"pending"}]}),
        )
        .unwrap();
        assert_eq!(todos.len(), 1);
        assert_eq!(todos[0].id, "c");
    }

    fn started_goal(state: &mut crate::app::AppState, condition: &str) -> uuid::Uuid {
        let limits = GoalLimits {
            max_continuations: 4,
            max_tokens: 1000,
            max_working_seconds: 600,
        };
        let goal = GoalState::start(condition.into(), limits, Utc::now()).unwrap();
        let id = goal.id;
        state.goal_state = Some(goal);
        id
    }

    async fn dispatch_update_goal(
        state: &mut crate::app::AppState,
        arguments: Value,
    ) -> crate::agent::registry::ToolExecutionResult {
        let workspace = crate::workspace::WorkspaceContext::discover(std::env::temp_dir()).unwrap();
        let skills =
            std::sync::Arc::new(std::sync::Mutex::new(crate::skills::SkillRegistry::empty()));
        let hooks =
            crate::hooks::HookRunner::new(Vec::new(), crate::sandbox::SandboxMode::PolicyOnly);
        // The active goal stays on AppState. Dispatch publishes it; this test does not.
        crate::execute_tool_dispatch(
            "update_goal",
            &arguments,
            &workspace,
            5,
            crate::sandbox::SandboxMode::PolicyOnly,
            false,
            crate::jobs::JobOwner::process(),
            &skills,
            &state.work_state,
            None,
            None,
            None,
            &hooks,
            None,
            state.goal_state.clone(),
        )
        .await
    }

    #[tokio::test]
    async fn update_goal_completes_pauses_and_records_progress() {
        let mut state = crate::app::AppState::new(crate::context::ShellHistory::in_memory());
        let id = started_goal(&mut state, "finish the work");
        assert!(state.work_state.lock().unwrap().session_goal.is_none());
        let progress = dispatch_update_goal(&mut state, json!({"message": "halfway"})).await;
        assert!(progress.is_success(), "{}", progress.model_text());
        assert!(
            !progress.model_text().contains("no active goal"),
            "{}",
            progress.model_text()
        );
        assert!(
            progress.model_text().contains("halfway"),
            "{}",
            progress.model_text()
        );
        apply_session_mirror(&mut state);
        let goal = state.goal_state.as_ref().unwrap();
        assert_eq!(goal.id, id);
        assert_eq!(goal.status, GoalStatus::Active);

        let paused = dispatch_update_goal(&mut state, json!({"blocked_reason": "stuck"})).await;
        assert!(paused.is_success(), "{}", paused.model_text());
        assert!(
            paused.model_text().contains("paused"),
            "{}",
            paused.model_text()
        );
        apply_session_mirror(&mut state);
        let goal = state.goal_state.as_ref().unwrap();
        assert_eq!(goal.id, id);
        assert_eq!(goal.status, GoalStatus::Paused);

        let id = started_goal(&mut state, "finish the work");
        let completed =
            dispatch_update_goal(&mut state, json!({"completed": true, "message": "done"})).await;
        assert!(completed.is_success(), "{}", completed.model_text());
        assert!(
            completed.model_text().contains("done"),
            "{}",
            completed.model_text()
        );
        apply_session_mirror(&mut state);
        let goal = state.goal_state.as_ref().unwrap();
        assert_eq!(goal.id, id);
        assert_eq!(goal.status, GoalStatus::Complete);
    }

    #[tokio::test]
    async fn completed_goal_survives_a_later_approval_in_the_same_batch() {
        let mut state = crate::app::AppState::new(crate::context::ShellHistory::in_memory());
        let id = started_goal(&mut state, "finish the work");
        publish_goal(&state);
        let root = tempfile::tempdir().unwrap();
        let workspace = crate::workspace::WorkspaceContext::discover(root.path()).unwrap();
        let skills =
            std::sync::Arc::new(std::sync::Mutex::new(crate::skills::SkillRegistry::empty()));
        let hooks =
            crate::hooks::HookRunner::new(Vec::new(), crate::sandbox::SandboxMode::PolicyOnly);
        let calls = [
            crate::app::ToolCall {
                observation: None,
                output_source: None,
                id: "goal".into(),
                name: "update_goal".into(),
                arguments: json!({"completed": true, "message": "done"}).to_string(),
                arguments_retrieved_spans: Vec::new(),
                status: crate::app::ToolStatus::Running,
                output: None,
                started_at: None,
            },
            crate::app::ToolCall {
                observation: None,
                output_source: None,
                id: "write".into(),
                name: "write_file".into(),
                arguments: json!({"path": root.path().join("out.txt"), "content": "x"}).to_string(),
                arguments_retrieved_spans: Vec::new(),
                status: crate::app::ToolStatus::Running,
                output: None,
                started_at: None,
            },
        ];
        let outcome = crate::execute_pending_tools_async(
            &calls,
            &crate::app::ApprovalMode::Allowlist,
            &workspace,
            5,
            &[crate::config::ApprovalRule::AllowTool("update_goal".into())],
            &[],
            &[],
            crate::sandbox::SandboxMode::PolicyOnly,
            crate::jobs::JobOwner::process(),
            &skills,
            &state.work_state,
            None,
            None,
            None,
            &hooks,
            None,
        )
        .await;
        assert!(
            outcome.pending.iter().any(|pending| matches!(
                pending,
                crate::interaction::PendingManualResolution::Approval(_)
            )),
            "the later tool should wait for approval: {:?}",
            outcome
                .pending
                .iter()
                .map(|pending| match pending {
                    crate::interaction::PendingManualResolution::Approval(approval) => {
                        approval.tool_name.as_str()
                    }
                    crate::interaction::PendingManualResolution::Execution(execution) => {
                        execution.tool_name.as_str()
                    }
                    crate::interaction::PendingManualResolution::Privilege(_) => "privilege",
                    crate::interaction::PendingManualResolution::Interaction(_) => "interaction",
                })
                .collect::<Vec<_>>()
        );
        let completed = outcome
            .results
            .iter()
            .find(|result| result.tool_id == "goal");
        assert!(
            completed.is_some_and(|result| result.output.contains("done")),
            "{:?}",
            outcome
                .results
                .iter()
                .map(|result| &result.output)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            state.goal_state.as_ref().map(|goal| goal.status),
            Some(GoalStatus::Active)
        );
        let paused = crate::settle_goal_after_tool_batch(&mut state, &outcome.pending);
        assert!(!paused);
        let goal = state.goal_state.as_ref().unwrap();
        assert_eq!(goal.id, id);
        assert_eq!(goal.status, GoalStatus::Complete);
        assert_eq!(
            state
                .work_state
                .lock()
                .unwrap()
                .session_goal
                .as_ref()
                .map(|goal| goal.status),
            Some(GoalStatus::Complete)
        );
    }

    #[tokio::test]
    async fn enter_and_exit_plan_mode_follow_switch_mode_and_do_not_approve() {
        let work = Arc::new(Mutex::new(WorkState {
            plan: Some(crate::work::PlanArtifact {
                id: crate::work::PlanId::new(),
                revision: 1,
                status: PlanStatus::Proposed,
                title: "plan".into(),
                body: None,
                created_at: Utc::now(),
                updated_at: Utc::now(),
                approved_at: None,
                approved_by: None,
                tasks: Vec::new(),
                decisions: Vec::new(),
                source_plan_id: None,
            }),
            ..WorkState::default()
        }));
        let cwd = std::env::temp_dir();
        let entered = WORK
            .scope(
                work.clone(),
                execute_tool_with_context("enter_plan_mode", &json!({}), &context(&cwd)),
            )
            .await;
        assert!(entered.model_text().contains("[SWITCH_MODE:plan]"));
        assert_eq!(work.lock().unwrap().session_mode.as_deref(), Some("plan"));
        let exited = WORK
            .scope(
                work.clone(),
                execute_tool_with_context("exit_plan_mode", &json!({}), &context(&cwd)),
            )
            .await;
        assert!(exited.model_text().contains("[SWITCH_MODE:agent]"));
        assert_eq!(
            work.lock().unwrap().plan.as_ref().unwrap().status,
            PlanStatus::Proposed
        );
    }

    #[test]
    fn scheduler_create_list_and_delete_use_the_schedule_store() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let store = ScheduleStoreV2::new(root.path().join("schedules"));
        let id = scheduler_create(
            &store,
            workspace.path(),
            &json!({"interval": "5m", "prompt": "check"}),
        )
        .unwrap();
        let listed = scheduler_list(&store).unwrap();
        assert!(listed.contains(&id), "{listed}");
        assert!(listed.contains("check"), "{listed}");
        scheduler_delete(&store, &json!({"task_id": id})).unwrap();
        let listed = scheduler_list(&store).unwrap();
        assert!(!listed.contains("check"), "{listed}");
    }

    #[test]
    fn scheduler_create_enables_a_record_claim_due_runs() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let store = ScheduleStoreV2::new(root.path().join("schedules"));
        let journal = crate::schedule_v2::RunJournal::new(root.path().join("journal"));
        let later = scheduler_create(
            &store,
            workspace.path(),
            &json!({"interval": "5m", "prompt": "later", "fire_immediately": false}),
        )
        .unwrap();
        let record = store
            .records()
            .unwrap()
            .into_iter()
            .find(|record| record.definition.id == later)
            .unwrap();
        assert!(record.enabled);
        let due_at = record.next_occurrence.unwrap().at;
        assert!(due_at > Utc::now());
        assert!(store
            .claim_due("default", &journal, Utc::now())
            .unwrap()
            .is_none());

        let immediate = scheduler_create(
            &store,
            workspace.path(),
            &json!({"interval": "5m", "prompt": "now", "fire_immediately": true}),
        )
        .unwrap();
        let claimed = store
            .claim_due(
                "default",
                &journal,
                Utc::now() + chrono::TimeDelta::seconds(2),
            )
            .unwrap()
            .unwrap();
        assert_eq!(claimed.schedule_id, immediate);
        store.settle(&claimed).unwrap();

        let claimed = store
            .claim_due("default", &journal, due_at)
            .unwrap()
            .unwrap();
        assert_eq!(claimed.schedule_id, later);
    }

    #[tokio::test]
    async fn monitor_returns_a_task_id_kill_job_can_cancel() {
        let temp = tempfile::tempdir().unwrap();
        let cwd = temp.path();
        let started = execute_tool_with_context(
            "monitor",
            &json!({"command": "sleep 30", "description": "wait"}),
            &context(cwd),
        )
        .await;
        assert!(started.is_success(), "{}", started.model_text());
        let id = started
            .model_text()
            .split("task_id: ")
            .nth(1)
            .unwrap_or("")
            .trim()
            .to_string();
        assert!(!id.is_empty(), "{}", started.model_text());
        let killed =
            execute_tool_with_context("kill_job", &json!({"job_id": id}), &context(cwd)).await;
        assert!(
            killed.model_text().contains("Cancellation requested"),
            "{}",
            killed.model_text()
        );
    }
}
