//! Interactive host adapter for persistent plan exchanges. Provider work runs
//! in the existing agent manager; disk projections run off the terminal thread.
use crate::agent::subagent::{
    AgentContextMode, AgentGrant, AgentId, AgentIsolation, AgentRoute, AgentRunOrigin,
    AgentRunPurpose, AgentSpawnRequest, AgentStatus,
};
use crate::app::{AppState, Role};
use crate::plan_workflow::{self, ExecutionAuthority, PlanWorkflowArgs, WorkflowRole};
use sha2::{Digest, Sha256};
use std::path::Path;

fn initiating_authority(state: &AppState) -> Option<ExecutionAuthority> {
    let (index, message) = state
        .messages
        .iter()
        .enumerate()
        .rev()
        .find(|(_, message)| message.role == Role::User)?;
    if !implementation_requested(&message.content) {
        return None;
    }
    Some(ExecutionAuthority {
        source_user_message: index,
        source_request_sha256: format!("{:x}", Sha256::digest(message.content.as_bytes())),
    })
}

pub fn source_for_turn(state: &AppState) -> Option<plan_workflow::WorkflowRequestSource> {
    let (index, message) = state
        .messages
        .iter()
        .enumerate()
        .rev()
        .find(|(_, message)| message.role == Role::User)?;
    Some(plan_workflow::WorkflowRequestSource {
        turn_id: state.active_agent_turn_id?,
        source_user_message: index,
        source_request_sha256: format!("{:x}", Sha256::digest(message.content.as_bytes())),
        implementation_requested: implementation_requested(&message.content),
    })
}

fn implementation_requested(request: &str) -> bool {
    // /plan run wraps the user's text in host guidance. That guidance itself
    // must never supply implementation intent.
    let request = request
        .strip_prefix("Start a cross-provider plan workflow for this request")
        .and_then(|tail| tail.split_once("Request: ").map(|(_, request)| request))
        .unwrap_or(request);
    let normalized = request.to_lowercase().replace('’', "'");
    let text = normalized.split_whitespace().collect::<Vec<_>>().join(" ");
    if [
        "do not implement",
        "don't implement",
        "do not execute",
        "don't execute",
        "do not build",
        "don't build",
        "no implementation",
        "without implementing",
        "without implementation",
        "planning only",
        "plan only",
        "only plan",
        "just plan",
        "only make a plan",
        "only create a plan",
        "only write a plan",
    ]
    .iter()
    .any(|phrase| text.contains(phrase))
    {
        return false;
    }
    if [
        "plan to implement",
        "plan to build",
        "plan for implementing",
        "plan for building",
        "plan how to",
        "plan on how to",
    ]
    .iter()
    .any(|phrase| text.contains(phrase))
        && ![
            "then implement",
            "then execute",
            "then build",
            "and implement",
            "and execute",
            "and build",
        ]
        .iter()
        .any(|phrase| text.contains(phrase))
    {
        return false;
    }
    text.split(|character: char| !character.is_alphabetic())
        .any(|word| matches!(word, "implement" | "execute" | "build"))
        || text.contains("carry out")
        || text.contains("apply the plan")
        || text.contains("apply plan")
}

fn idle(state: &AppState) -> bool {
    let graph_running = {
        let work = state
            .work_state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        work.agent_graph.status == crate::agent_graph::AgentGraphStatus::Running
    };
    !state.turn_lifecycle.is_active()
        && state.streaming.is_none()
        && !state.hook_running
        && !state.review_in_flight
        && !state.session_load_busy
        && !state.session_action_busy
        && !state.attachment_resolution_busy
        && !state.agent_graph_busy
        && !graph_running
        && state.workflow_execution.is_none()
        && !state
            .goal_state
            .as_ref()
            .is_some_and(|goal| goal.status == crate::goal::GoalStatus::Active)
        && !state
            .workflow_run
            .as_ref()
            .is_some_and(|run| !run.status.is_terminal())
        && state.interaction.is_none()
        && !state.approval_queue.is_active()
        && state.plan_execute_requested.is_none()
}

fn validate_routes(args: &PlanWorkflowArgs, available: &[AgentRoute]) -> Result<(), String> {
    let mut routes = Vec::new();
    match args {
        PlanWorkflowArgs::Start {
            participants,
            steps,
            implementation,
            ..
        } => {
            routes.extend(
                participants
                    .iter()
                    .filter_map(|participant| participant.route.as_ref()),
            );
            routes.extend(steps.iter().filter_map(|step| step.route.as_ref()));
            routes.extend(implementation.iter().filter_map(|step| step.route.as_ref()));
        }
        PlanWorkflowArgs::Handoff { step } => routes.extend(step.route.as_ref()),
        _ => {}
    }
    for route in routes {
        if !available.contains(route) {
            return Err(format!(
                "Unknown configured plan route: {}/{}",
                route.provider, route.model
            ));
        }
    }
    Ok(())
}

async fn persist(
    state: &mut AppState,
    sessions: &crate::session::SessionStore,
    data_root: &Path,
) -> Result<(), String> {
    let session_id = crate::save_current_session(state, sessions)
        .map_err(|error| format!("Plan workflow save failed: {error}"))?;
    let workflow = state
        .work_state
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .plan_workflow
        .clone();
    if let Some(workflow) = workflow {
        let root = data_root.to_path_buf();
        let path = tokio::task::spawn_blocking(move || {
            plan_workflow::write_projection(&root, session_id, &workflow)
        })
        .await
        .map_err(|_| "Plan Markdown worker stopped".to_string())??;
        if state.plan_workflow_graph.is_some() {
            state.status_message = Some(format!("Plan files: {}", path.display()));
        }
    }
    Ok(())
}

fn stop_on_error(state: &mut AppState, error: &str) {
    let mut work = state
        .work_state
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    plan_workflow::fail_stage(&mut work, None, error);
    state.status_message = Some(error.into());
}

/// Called between terminal events. Each call applies at most one admission or
/// completion; no provider request is awaited by the terminal event loop.
pub async fn poll(
    state: &mut AppState,
    config: &crate::CaConfig,
    sessions: &crate::session::SessionStore,
    data_root: &Path,
) {
    let pending_action = state
        .plan_workflow_request
        .as_ref()
        .is_some_and(|request| !matches!(request, PlanWorkflowArgs::Status))
        || state
            .work_state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .pending_plan_workflow
            .is_some();
    if pending_action {
        if let Some(history) = state
            .session_history
            .as_mut()
            .filter(|history| !history.is_complete())
        {
            if let Some(error) = &history.error {
                state.status_message = Some(format!("Plan workflow action kept; saved history could not load: {error}. Reload the session to retry."));
            } else {
                history.request_full = true;
                state.status_message = Some(
                    "Loading complete saved history before continuing the plan workflow…".into(),
                );
            }
            return;
        }
    }
    if state.plan_workflow_projection_pending {
        state.plan_workflow_projection_pending = false;
        if let Err(error) = persist(state, sessions, data_root).await {
            stop_on_error(state, &error);
            return;
        }
    }
    if matches!(state.plan_workflow_request, Some(PlanWorkflowArgs::Status)) {
        state.plan_workflow_request = None;
        let work = state
            .work_state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        state.status_message = Some(work.plan_workflow.as_ref().map_or_else(
            || "No plan workflow. Use /plan run or /plan send to start.".into(),
            plan_workflow::PlanWorkflowState::render_status,
        ));
        return;
    }
    let (request, authority) = if let Some(request) = state.plan_workflow_request.take() {
        if matches!(request, PlanWorkflowArgs::Cancel) {
            state
                .work_state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .pending_plan_workflow = None;
        }
        (Some(request), initiating_authority(state))
    } else {
        let pending = state
            .work_state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .pending_plan_workflow
            .take();
        match pending {
            Some(pending) => {
                let source = pending.source;
                let matches = state.active_agent_turn_id == Some(source.turn_id)
                    && state
                        .messages
                        .get(source.source_user_message)
                        .is_some_and(|message| {
                            message.role == Role::User
                                && format!("{:x}", Sha256::digest(message.content.as_bytes()))
                                    == source.source_request_sha256
                        });
                if !matches {
                    state.status_message = Some("Discarded a plan workflow action from an interrupted or replaced user turn.".into());
                    return;
                }
                let authority = source
                    .implementation_requested
                    .then_some(ExecutionAuthority {
                        source_user_message: source.source_user_message,
                        source_request_sha256: source.source_request_sha256,
                    });
                (Some(pending.args), authority)
            }
            None => (None, None),
        }
    };
    if request.is_none()
        && !{
            let work = state
                .work_state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            plan_workflow::needs_tick(&work)
        }
    {
        return;
    }
    let Some(manager) = state.agents.clone() else {
        if request.is_some() {
            state.status_message = Some("Plan workflows require an active agent manager.".into());
        }
        return;
    };
    if let Some(request) = request {
        if !state.run_capabilities.session_persistence {
            state.status_message = Some("Plan workflows require a persistent ca profile.".into());
            return;
        }
        // Resolve the inherited/configured delegation default once. Explicit
        // participant routes are frozen by the controller and never fall back.
        let (provider, _, model) = crate::resolve_provider_route(state, config);
        let default = manager
            .default_delegation_route_for_turn(manager.current_parent_turn())
            .unwrap_or(AgentRoute { provider, model });
        let old_agent = {
            let work = state
                .work_state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            plan_workflow::active_dispatch(&work)
        };
        let stopping = matches!(request, PlanWorkflowArgs::Cancel);
        let changes_bindings = matches!(
            request,
            PlanWorkflowArgs::Start { .. } | PlanWorkflowArgs::Handoff { .. }
        );
        let result = validate_routes(&request, &manager.available_routes()).and_then(|()| {
            let mut work = state
                .work_state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            plan_workflow::apply_action(&mut work, request, &default, authority)
        });
        match result {
            Ok(notice) => state.status_message = Some(notice),
            Err(error) => {
                state.status_message = Some(error);
                return;
            }
        }
        if stopping {
            if let Some((agent_id, role)) = old_agent {
                if role != WorkflowRole::Implement {
                    let _ = manager.cancel(AgentId(agent_id)).await;
                }
            }
        }
        if changes_bindings {
            state.plan_proposal = None;
        }
        if let Err(error) = persist(state, sessions, data_root).await {
            stop_on_error(state, &error);
            return;
        }
    }
    if crate::provider_filter::is_tripped() {
        let active = {
            let work = state
                .work_state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            plan_workflow::active_dispatch(&work)
        };
        if let Some((id, role)) = active {
            if role != WorkflowRole::Implement {
                let _ = manager.cancel(AgentId(id)).await;
            }
            stop_on_error(
                state,
                "Plan workflow stopped by the provider filter; no further stage started.",
            );
            let _ = persist(state, sessions, data_root).await;
        }
        return;
    }
    let active = {
        let work = state
            .work_state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        plan_workflow::active_dispatch(&work)
    };
    if let Some((agent_id, role)) = active {
        if role == WorkflowRole::Implement {
            return;
        }
        let snapshot = match manager.snapshot(AgentId(agent_id)).await {
            Ok(snapshot) if snapshot.status.terminal() => snapshot,
            Ok(_) => return,
            Err(error) => {
                stop_on_error(state, &format!("Plan stage could not be read: {error}"));
                let _ = persist(state, sessions, data_root).await;
                return;
            }
        };
        let result = if snapshot.status == AgentStatus::Completed {
            match snapshot.structured_result {
                Some(value) => {
                    let mut work = state
                        .work_state
                        .lock()
                        .unwrap_or_else(|poison| poison.into_inner());
                    plan_workflow::complete_stage(&mut work, agent_id, value)
                }
                None => Err("Plan stage returned no validated structured result".into()),
            }
        } else {
            Err(format!(
                "Plan stage ended {:?}: {}",
                snapshot.status,
                snapshot.result_summary.as_deref().unwrap_or("No result")
            ))
        };
        if let Err(error) = result {
            stop_on_error(state, &error);
        }
        state.plan_proposal = None;
        // A reader already open on an earlier version retains its stale binding;
        // all new graph actions select the freshly committed current plan.
        if let Err(error) = persist(state, sessions, data_root).await {
            stop_on_error(state, &error);
        }
        return;
    }
    if !idle(state) || !state.input_queue.is_empty() {
        return;
    }
    let dispatch = {
        let work = state
            .work_state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        plan_workflow::next_dispatch(&work)
    };
    let dispatch = match dispatch {
        Ok(Some(dispatch)) => dispatch,
        Ok(None) => return,
        Err(error) => {
            stop_on_error(state, &error);
            let _ = persist(state, sessions, data_root).await;
            return;
        }
    };
    if !manager.available_routes().contains(&dispatch.route) {
        stop_on_error(
            state,
            "The frozen plan provider/model route is no longer configured.",
        );
        let _ = persist(state, sessions, data_root).await;
        return;
    }
    // Re-read the saved owner and publish Markdown before any provider traffic.
    if let Err(error) = persist(state, sessions, data_root).await {
        stop_on_error(state, &error);
        return;
    }
    if dispatch.role == WorkflowRole::Implement {
        let id = uuid::Uuid::new_v4();
        let review = {
            let mut work = state
                .work_state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            if let Err(error) = plan_workflow::mark_dispatched(&mut work, &dispatch, id) {
                drop(work);
                stop_on_error(state, &error);
                return;
            }
            work.plan.as_ref().map(crate::work::PlanArtifact::review)
        };
        if let Err(error) = persist(state, sessions, data_root).await {
            stop_on_error(state, &error);
            return;
        }
        state.plan_execute_route = Some(dispatch.route);
        state.plan_execute_requested = review;
        state.status_message =
            Some("Planning complete; starting the requested implementation…".into());
        return;
    }
    let available_tools = manager.available_tool_names().unwrap_or_default();
    let tools = [
        "read_file",
        "list_dir",
        "glob",
        "search_files",
        "read_todos",
    ]
    .into_iter()
    .filter(|tool| available_tools.iter().any(|available| available == tool))
    .map(str::to_string)
    .collect();
    let output_contract = dispatch
        .output_schema
        .clone()
        .map(crate::headless_contract::StructuredOutputContract::from_value)
        .transpose();
    let output_contract = match output_contract {
        Ok(contract) => contract,
        Err(error) => {
            stop_on_error(state, &error);
            return;
        }
    };
    let request = AgentSpawnRequest {
        parent: None,
        parent_turn: manager.current_parent_turn(),
        task_id: None,
        task: dispatch.prompt.clone(),
        context_mode: AgentContextMode::Summary,
        context_summary: crate::load_project_instructions_for_workspace(&state.workspace),
        grants: AgentGrant {
            tools,
            workspace_root_ids: state
                .workspace
                .roots()
                .iter()
                .map(|root| root.id.to_string())
                .collect(),
            path_scopes: state
                .workspace
                .roots()
                .iter()
                .map(|root| root.path.to_string_lossy().into_owned())
                .collect(),
            command_scopes: Vec::new(),
            network: false,
        },
        workspace: state.workspace.clone(),
        depth: 1,
        route: Some(dispatch.route.clone()),
        purpose: AgentRunPurpose::Subagent,
        output_contract,
        isolation: AgentIsolation::Shared,
    };
    // Retain the exact input and an admission nonce before the manager can
    // start network traffic. A crash here restores paused, never replayed.
    let admission = uuid::Uuid::new_v4();
    let reserved = {
        let mut work = state
            .work_state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        plan_workflow::mark_dispatched(&mut work, &dispatch, admission)
    };
    if let Err(error) = reserved {
        stop_on_error(state, &error);
        return;
    }
    if let Err(error) = persist(state, sessions, data_root).await {
        stop_on_error(state, &error);
        return;
    }
    match manager
        .spawn_with_origin(request, AgentRunOrigin::PlanWorkflow)
        .await
    {
        Ok(agent_id) => {
            let result = {
                let mut work = state
                    .work_state
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner());
                match work
                    .plan_workflow
                    .as_mut()
                    .and_then(|workflow| workflow.steps.get_mut(dispatch.step_index))
                {
                    Some(stage)
                        if stage.id == dispatch.stage_id && stage.agent_id == Some(admission) =>
                    {
                        stage.agent_id = Some(agent_id.0);
                        Ok(())
                    }
                    _ => Err("Plan stage changed during child admission".to_string()),
                }
            };
            if let Err(error) = result {
                let _ = manager.cancel(agent_id).await;
                stop_on_error(state, &error);
            } else {
                state.status_message = Some(format!(
                    "{}: {} on {}/{}",
                    dispatch.participant,
                    dispatch.role.label(),
                    dispatch.route.provider,
                    dispatch.route.model
                ));
            }
            if let Err(error) = persist(state, sessions, data_root).await {
                let _ = manager.cancel(agent_id).await;
                stop_on_error(state, &error);
            }
        }
        Err(error) => {
            stop_on_error(state, &format!("Plan stage could not start: {error}"));
            let _ = persist(state, sessions, data_root).await;
        }
    }
}

pub fn implementation_active(work: &crate::work::WorkState) -> Option<uuid::Uuid> {
    plan_workflow::active_dispatch(work)
        .and_then(|(id, role)| (role == WorkflowRole::Implement).then_some(id))
}

pub fn record_explicit_execution(
    work: &mut crate::work::WorkState,
    route: &AgentRoute,
    source_user_message: usize,
) -> Result<(), String> {
    if work.plan_workflow.is_none() || implementation_active(work).is_some() {
        return Ok(());
    }
    let participant = format!(
        "executor-{}",
        work.plan_workflow
            .as_ref()
            .map_or(1, |workflow| workflow.steps.len() + 1)
    );
    let request = format!("/plan execute {}/{}", route.provider, route.model);
    plan_workflow::apply_action(
        work,
        PlanWorkflowArgs::Handoff {
            step: plan_workflow::WorkflowStep {
                role: WorkflowRole::Implement,
                participant,
                route: Some(route.clone()),
                instructions: "Execute the explicitly selected plan and record task evidence."
                    .into(),
            },
        },
        route,
        Some(ExecutionAuthority {
            source_user_message,
            source_request_sha256: format!("{:x}", Sha256::digest(request.as_bytes())),
        }),
    )?;
    let dispatch =
        plan_workflow::next_dispatch(work)?.ok_or("No implementation stage was prepared")?;
    plan_workflow::mark_dispatched(work, &dispatch, uuid::Uuid::new_v4())
}

pub fn implementation_context(work: &crate::work::WorkState) -> String {
    if implementation_active(work).is_none() {
        return String::new();
    }
    let Some(workflow) = work.plan_workflow.as_ref() else {
        return String::new();
    };
    let Some(stage) = workflow.steps.get(workflow.cursor) else {
        return String::new();
    };
    format!("\n\n## Plan workflow implementation\nParticipant: {} on {}/{}.\nOriginal goal:\n{}\n\nImplementation instructions:\n{}\nComplete every stable plan task with successful recorded evidence before finishing. Successful run_command results include recorded_event_id for record_plan_evidence; task mutations return the next revision.\n",
        stage.participant, stage.route.provider, stage.route.model, workflow.goal, stage.instructions)
}

pub fn foreground_stop_requested(state: &AppState) -> bool {
    let work = state
        .work_state
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    implementation_active(&work).is_some()
        && matches!(
            state.plan_workflow_request.as_ref().or(work
                .pending_plan_workflow
                .as_ref()
                .map(|pending| &pending.args)),
            Some(PlanWorkflowArgs::Cancel)
        )
}

pub fn finish_implementation(state: &mut AppState, succeeded: bool, summary: &str) {
    let mut work = state
        .work_state
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let Some(id) = implementation_active(&work) else {
        return;
    };
    let all_tasks_complete = work.plan.as_ref().is_some_and(|plan| {
        !plan.tasks.is_empty()
            && plan.tasks.iter().all(|task| {
                task.status == crate::work::TaskStatus::Completed && !task.evidence.is_empty()
            })
    });
    if succeeded && all_tasks_complete {
        if let Err(error) = plan_workflow::complete_implementation(&mut work, id, true, summary) {
            plan_workflow::fail_stage(&mut work, Some(id), &error);
        }
    } else {
        let reason = if succeeded {
            "Implementation turn ended with incomplete plan tasks or missing recorded evidence. The work is retained; continue implementation explicitly."
        } else {
            summary
        };
        plan_workflow::fail_stage(&mut work, Some(id), reason);
    }
    drop(work);
    // Finalization saves the native session immediately. The next host poll
    // publishes the final immutable Markdown snapshot without a provider call.
    state.plan_workflow_projection_pending = true;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn workflow_controls_wait_for_complete_saved_history() {
        let temporary = tempfile::tempdir().unwrap();
        let sessions = crate::session::SessionStore::new(temporary.path().join("sessions"));
        let mut state = AppState::new(crate::context::ShellHistory::in_memory());
        let session_id = uuid::Uuid::new_v4();
        state.session_history = Some(crate::app::SessionHistoryState {
            storage_key: session_id.to_string(),
            session_id,
            storage_generation: uuid::Uuid::nil(),
            start_index: 2,
            total_messages: 3,
            busy: false,
            request_older: false,
            request_full: false,
            resume_submit: false,
            generation: 1,
            error: None,
        });
        state.plan_workflow_request = Some(PlanWorkflowArgs::Resume);
        poll(
            &mut state,
            &crate::CaConfig::default(),
            &sessions,
            temporary.path(),
        )
        .await;
        assert!(state.session_history.as_ref().unwrap().request_full);
        assert!(matches!(
            state.plan_workflow_request,
            Some(PlanWorkflowArgs::Resume)
        ));
        assert!(!temporary.path().join("sessions").exists());
    }

    #[test]
    fn implementation_intent_comes_from_the_human_request() {
        assert!(implementation_requested(
            "A plans, B reviews, A revises, then implement on C."
        ));
        assert!(implementation_requested("Plan and build the feature."));
        for request in [
            "Make a plan",
            "Only make a plan; do not implement it",
            "Just plan the change",
            "Review this plan only",
            "Create a plan without implementing anything",
            "Create a plan to implement the feature",
            "Do not build anything; make a plan",
        ] {
            assert!(!implementation_requested(request), "{request}");
        }
        assert!(!implementation_requested("Start a cross-provider plan workflow for this request. Include implementation only if requested. Request: Make a plan"));
    }

    #[test]
    fn route_validation_rejects_unknown_destinations_before_admission() {
        let args = PlanWorkflowArgs::Handoff {
            step: crate::plan_workflow::WorkflowStep {
                role: WorkflowRole::Review,
                participant: "reviewer".into(),
                route: Some(AgentRoute {
                    provider: "missing".into(),
                    model: "model".into(),
                }),
                instructions: String::new(),
            },
        };
        assert!(validate_routes(&args, &[]).is_err());
    }
}
