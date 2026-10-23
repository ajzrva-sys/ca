//! Host-owned plan review and the boundary from planning to a new Agent turn.
use crate::app::{AgentMode, AppState, Message, Role};
use crate::work::{PlanReview, PlanStatus, WorkMutation};

fn explicit_route(value: &str) -> Result<crate::agent::subagent::AgentRoute, String> {
    let (provider, model) = value
        .split_once('/')
        .filter(|(provider, model)| !provider.is_empty() && !model.is_empty())
        .ok_or("Use a provider/model route.")?;
    Ok(crate::agent::subagent::AgentRoute {
        provider: provider.into(),
        model: model.into(),
    })
}

pub fn open_workflow_picker(state: &mut AppState, action: crate::app::PlanWorkflowPickerAction) {
    close_reader(state);
    crate::tui::input::open_model_picker(state);
    state.model_picker_target = crate::app::ModelPickerTarget::PlanWorkflow;
    state.plan_workflow_picker_action = Some(action);
}

fn open_workflow_graph(state: &mut AppState) {
    let selected_idx = {
        let work = state
            .work_state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        work.plan_workflow.as_ref().map_or(0, |workflow| {
            workflow.cursor.min(workflow.steps.len().saturating_sub(1))
        })
    };
    state.plan_workflow_graph = Some(crate::app::PlanWorkflowGraphState {
        selected_idx,
        participants: false,
    });
}

pub fn select_workflow_model(state: &mut AppState, model: crate::app::ModelInfo) {
    let Some(action) = state.plan_workflow_picker_action.take() else {
        return;
    };
    let route = crate::agent::subagent::AgentRoute {
        provider: model.provider,
        model: model.id,
    };
    match action {
        crate::app::PlanWorkflowPickerAction::Send { role, instructions } => {
            state.plan_workflow_graph = None;
            state.input = format!(
                "/plan send {} {}/{} {}",
                role.label().to_ascii_lowercase(),
                route.provider,
                route.model,
                instructions
            );
            state.input_cursor = state.input.len();
            state.slash_palette_open = false;
            state.status_message =
                Some("Add instructions or change review/revise, then press Enter to send.".into());
        }
        crate::app::PlanWorkflowPickerAction::Execute(review) => {
            state.plan_execute_route = Some(route);
            state.plan_execute_requested = Some(review);
            state.plan_workflow_graph = None;
            state.status_message = Some("Preparing plan execution…".into());
        }
    }
}

fn queue_handoff(
    state: &mut AppState,
    role: crate::plan_workflow::WorkflowRole,
    participant: Option<String>,
    route: Option<crate::agent::subagent::AgentRoute>,
    instructions: String,
) {
    let participant = participant.unwrap_or_else(|| {
        let work = state
            .work_state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let mut index = 1;
        while work.plan_workflow.as_ref().is_some_and(|workflow| {
            workflow
                .participants
                .iter()
                .any(|participant| participant.id == format!("agent-{index}"))
        }) {
            index += 1;
        }
        format!("agent-{index}")
    });
    state.plan_workflow_request = Some(crate::plan_workflow::PlanWorkflowArgs::Handoff {
        step: crate::plan_workflow::WorkflowStep {
            role,
            participant,
            route,
            instructions,
        },
    });
    close_reader(state);
    open_workflow_graph(state);
    state.status_message = Some("Sending plan…".into());
}

fn send_command(state: &mut AppState, value: &str) -> Result<(), String> {
    selected_review(state)?;
    if state.turn_lifecycle.is_active()
        || state.streaming.is_some()
        || state.plan_workflow_request.is_some()
    {
        return Err("Finish the active request before sending the plan.".into());
    }
    let mut remaining = value.trim();
    let first = remaining.split_whitespace().next().unwrap_or_default();
    let role = if first.eq_ignore_ascii_case("revise") {
        remaining = remaining[first.len()..].trim();
        crate::plan_workflow::WorkflowRole::Revise
    } else {
        if first.eq_ignore_ascii_case("review") {
            remaining = remaining[first.len()..].trim();
        }
        crate::plan_workflow::WorkflowRole::Review
    };
    let target = remaining.split_whitespace().next().unwrap_or_default();
    let participant = {
        let work = state
            .work_state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        work.plan_workflow.as_ref().and_then(|workflow| {
            workflow
                .participants
                .iter()
                .find(|participant| participant.id == target)
                .map(|participant| participant.id.clone())
        })
    };
    if participant.is_some() || target.contains('/') {
        let route = participant
            .is_none()
            .then(|| explicit_route(target))
            .transpose()?;
        let instructions = remaining[target.len()..].trim().to_string();
        queue_handoff(state, role, participant, route, instructions);
    } else {
        open_workflow_picker(
            state,
            crate::app::PlanWorkflowPickerAction::Send {
                role,
                instructions: remaining.into(),
            },
        );
    }
    Ok(())
}

pub fn selected_review(state: &AppState) -> Result<PlanReview, String> {
    let work = state
        .work_state
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let plan = work
        .plan
        .as_ref()
        .ok_or("No plan to review. Use /plan first.")?;
    let selected = state
        .plan_reader
        .filter(|_| state.pager_open)
        .or(state.plan_proposal)
        .unwrap_or_else(|| plan.review());
    if selected != plan.review() {
        return Err("The plan changed. Use /plan show to review the latest version.".into());
    }
    Ok(selected)
}

pub fn open_reader(state: &mut AppState) -> Result<(), String> {
    let work = state
        .work_state
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let plan = work
        .plan
        .as_ref()
        .ok_or("No plan to review. Use /plan first.")?;
    state.plan_reader = Some(plan.review());
    state.plan_proposal = (plan.status != PlanStatus::Rejected).then(|| plan.review());
    state.pager_content = plan.render_document();
    state.pager_scroll = 0;
    state.pager_selection.clear_selection();
    state.learning_review_pager_open = false;
    state.pager_open = true;
    Ok(())
}

fn close_reader(state: &mut AppState) {
    if state.plan_reader.take().is_some() {
        state.pager_open = false;
        state.pager_selection.clear_selection();
    }
}

pub fn dismiss(state: &mut AppState) {
    close_reader(state);
    state.plan_proposal = None;
    state.plan_execute_requested = None;
    state.plan_execute_route = None;
    state.status_message = Some("Plan dismissed. Use /plan show to reopen.".into());
}

pub fn revise(state: &mut AppState) {
    close_reader(state);
    state.plan_proposal = None;
    state.plan_execute_requested = None;
    state.plan_execute_route = None;
    state.mode = AgentMode::Plan;
    state.status_message = Some("Describe the changes you want, then press Enter.".into());
}

pub fn comment_selection(state: &mut AppState) -> Result<(), String> {
    let review = selected_review(state)?;
    let quote = state
        .pager_selection
        .selection_text()
        .ok_or("Select plan text first, then press N.")?;
    let anchor = {
        let work = state
            .work_state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        crate::plan_feedback::FeedbackAnchor::selected_quote(
            work.plan.as_ref().ok_or("No plan to comment on.")?,
            &quote,
        )?
    };
    state.selected_plan_feedback = Some((review, anchor));
    close_reader(state);
    state.input = "/plan comment selected ".into();
    state.input_cursor = state.input.len();
    state.status_message =
        Some("Type the comment and press Enter; submit the draft ledger with /plan submit.".into());
    Ok(())
}

/// Finish the request, not the artifact: awaiting review is idle and editable.
pub fn finish_planning_epoch(state: &mut AppState) {
    state.turn_lifecycle.complete();
    crate::finish_active_prompt_epoch(state);
    state.active_turn_route = None;
    state.active_turn_start_message_count = None;
    state.active_attachment_context = None;
    state.turn_start = None;
    state.retry_boundary = None;
    state.soft_pause.clear();
    crate::reset_loop_guard_state(state);
    crate::record_active_skill_outcome(state, crate::learning_journey::SkillOutcome::Completed);
    state.task_skill_loads.clear();
    state.active_skill_invocation_group = None;
    state
        .task_skill_snapshot
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .clear();
}

pub fn finish_response(state: &mut AppState) {
    let start_revision = state.plan_turn_start_revision.take();
    state.plan_proposal = {
        let work = state
            .work_state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        work.plan
            .as_ref()
            .filter(|plan| {
                plan.status == PlanStatus::Proposed
                    && start_revision.is_some_and(|revision| plan.revision > revision)
            })
            .map(|plan| plan.review())
    };
    finish_planning_epoch(state);
    state.status_message = Some(if state.plan_proposal.is_some() {
        "Plan ready. Enter executes; type feedback to revise.".into()
    } else {
        "Planning reply complete".into()
    });
}

/// Returns whether an artifact decision needs saving. Starting is handled once
/// by the event loop so keyboard and slash commands share the same boundary.
pub fn manage_command(state: &mut AppState, arguments: &str) -> Result<bool, String> {
    let arguments = arguments.trim();
    if arguments.is_empty() {
        state.mode = AgentMode::Plan;
        state.status_message = Some("Plan mode. Describe what you want to plan.".into());
        return Ok(false);
    }
    let (command, value) = arguments
        .split_once(char::is_whitespace)
        .map_or((arguments, ""), |(command, value)| (command, value.trim()));
    match command.to_ascii_lowercase().as_str() {
        "run" => {
            if value.is_empty() {
                return Err("Use /plan run <goal and the agents or providers you want>.".into());
            }
            if state.turn_lifecycle.is_active() || state.streaming.is_some() {
                return Err("Finish the active request before starting a plan workflow.".into());
            }
            close_reader(state);
            state.plan_proposal = None;
            state.mode = AgentMode::Plan;
            state.input_queue.enqueue_front_with_id(uuid::Uuid::new_v4(), format!(
                "Start a cross-provider plan workflow for this request using the plan_workflow tool. Use list_agent_routes to discover configured routes, create named participants and ordered author/review/revise stages, and include implementation only if requested. Preserve the requested provider choices. Request: {value}"
            ));
            state.plan_workflow_graph = Some(Default::default());
            state.status_message = Some("Preparing plan workflow…".into());
        }
        "send" => send_command(state, value)?,
        "graph" => {
            close_reader(state);
            open_workflow_graph(state);
        }
        "pause" | "resume" | "cancel" | "status" => {
            state.plan_workflow_request = Some(match command.to_ascii_lowercase().as_str() {
                "pause" => crate::plan_workflow::PlanWorkflowArgs::Pause,
                "resume" => crate::plan_workflow::PlanWorkflowArgs::Resume,
                "cancel" => crate::plan_workflow::PlanWorkflowArgs::Cancel,
                _ => crate::plan_workflow::PlanWorkflowArgs::Status,
            });
        }
        "comments" => {
            let mut work = state
                .work_state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            let plan = work.plan.clone().ok_or("No plan to comment on.")?;
            work.plan_feedback.refresh(&plan);
            state.plan_reader = Some(plan.review());
            state.pager_content = work.plan_feedback.render(&plan);
            state.pager_scroll = 0;
            state.pager_selection.clear_selection();
            state.pager_open = true;
        }
        "comment" => {
            let review = selected_review(state)?;
            let (action, rest) = value
                .split_once(char::is_whitespace)
                .ok_or("Use /plan comments to see comment actions.")?;
            let mut work = state
                .work_state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            let plan = work.plan.clone().ok_or("No plan to comment on.")?;
            match action {
                "selected" => {
                    let (selected, anchor) = state
                        .selected_plan_feedback
                        .clone()
                        .ok_or("Select plan text in /plan show and press N first.")?;
                    if selected != review {
                        return Err(
                            "The selected plan revision changed; select the text again.".into()
                        );
                    }
                    let id = work
                        .plan_feedback
                        .add(&plan, review, anchor, rest.trim().into())?;
                    state.selected_plan_feedback = None;
                    state.status_message = Some(format!(
                        "Comment {id} saved as a draft. Use /plan submit to request revision."
                    ));
                }
                "task" | "text" => {
                    let (target, comment) = rest
                        .trim()
                        .split_once(char::is_whitespace)
                        .ok_or("A target and comment are required.")?;
                    let anchor = if action == "task" {
                        let task_id = if let Ok(index) = target.parse::<usize>() {
                            plan.tasks
                                .get(index.checked_sub(1).ok_or("Tasks start at 1.")?)
                                .ok_or("Unknown task number.")?
                                .id
                        } else {
                            crate::work::TaskId(
                                uuid::Uuid::parse_str(target)
                                    .map_err(|_| "Invalid task identity.")?,
                            )
                        };
                        crate::plan_feedback::FeedbackAnchor::task(&plan, task_id)?
                    } else {
                        let (start, end) = target
                            .split_once(':')
                            .ok_or("Use a logical source range start:end.")?;
                        crate::plan_feedback::FeedbackAnchor::text(
                            &plan,
                            start.parse().map_err(|_| "Invalid source start.")?,
                            end.parse().map_err(|_| "Invalid source end.")?,
                        )?
                    };
                    let id =
                        work.plan_feedback
                            .add(&plan, review, anchor, comment.trim().into())?;
                    state.status_message = Some(format!(
                        "Comment {id} saved as a draft. Use /plan submit to request revision."
                    ));
                }
                "edit" => {
                    let (id, comment) = rest
                        .trim()
                        .split_once(char::is_whitespace)
                        .ok_or("A comment identity and text are required.")?;
                    work.plan_feedback.edit(
                        uuid::Uuid::parse_str(id).map_err(|_| "Invalid comment identity.")?,
                        comment.trim().into(),
                    )?;
                    state.status_message = Some("Draft comment edited.".into());
                }
                "delete" => {
                    work.plan_feedback.delete(
                        uuid::Uuid::parse_str(rest.trim())
                            .map_err(|_| "Invalid comment identity.")?,
                    )?;
                    state.status_message = Some("Draft comment deleted.".into());
                }
                _ => return Err("Use /plan comments to see comment actions.".into()),
            }
            return Ok(true);
        }
        "submit" => {
            if state.turn_lifecycle.is_active() || state.streaming.is_some() || state.hook_running {
                return Err("Finish the active request before submitting plan comments.".into());
            }
            let review = selected_review(state)?;
            let (id, text) = {
                let mut work = state
                    .work_state
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner());
                let plan = work.plan.clone().ok_or("No plan to revise.")?;
                work.plan_feedback.submit(&plan, review)?
            };
            close_reader(state);
            state.mode = AgentMode::Plan;
            state.plan_execute_requested = None;
            state.input_queue.enqueue_front_with_id(id, text);
            state.plan_feedback_dispatch = Some(id);
            state.status_message = Some("Submitting comments for a plan revision…".into());
            return Ok(true);
        }
        "show" => {
            open_reader(state)?;
        }
        "dismiss" => dismiss(state),
        "revise" => revise(state),
        "execute" => {
            if state.turn_lifecycle.is_active()
                || state.streaming.is_some()
                || state.plan_execute_requested.is_some()
                || state.hook_running
                || state.review_in_flight
                || state.attachment_resolution_busy
            {
                return Err("A request is already active. Stop it before executing a plan.".into());
            }
            let review = selected_review(state)?;
            if value.eq_ignore_ascii_case("choose") {
                open_workflow_picker(state, crate::app::PlanWorkflowPickerAction::Execute(review));
                return Ok(false);
            }
            state.plan_execute_route = if value.is_empty() { None } else { Some(explicit_route(value)?) };
            if let Some(history) = state.session_history.as_mut() {
                // A fresh explicit action may retry an earlier history-load failure.
                history.error = None;
            }
            state.plan_execute_requested = Some(review);
            state.status_message = Some("Preparing plan execution…".into());
        }
        "approve" | "reject" => {
            if state.streaming.is_some() {
                return Err("Wait for the planning reply to finish before deciding.".into());
            }
            let review = selected_review(state)?;
            let approve = command.eq_ignore_ascii_case("approve");
            {
                let mut work = state
                    .work_state
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner());
                work.user_decide(review.revision, approve)
                    .map_err(|error| error.to_string())?;
            }
            close_reader(state);
            state.plan_proposal = None;
            state.plan_execute_requested = None;
            finish_planning_epoch(state);
            state.mode = AgentMode::Agent;
            state.status_message = Some(if approve {
                "Plan approved. Use /plan execute to start.".into()
            } else {
                "Plan rejected".into()
            });
            return Ok(true);
        }
        "edit" if !value.is_empty() => {
            let review = selected_review(state)?;
            {
                let mut work = state
                    .work_state
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner());
                work.apply_model_mutation(WorkMutation::Edit {
                    expected_revision: review.revision,
                    title: Some(value.into()),
                    body: None,
                    decisions: Vec::new(),
                })
                .map_err(|error| error.to_string())?;
            }
            open_reader(state)?;
            state.status_message = Some("Plan updated. Review it before execution.".into());
            return Ok(true);
        }
        _ => {
            return Err(
                "Usage: /plan [show|run <request>|send [review|revise] [participant|provider/model] [instructions]|graph|pause|resume|cancel|execute [provider/model|choose]|approve|revise|dismiss|reject|edit <title>]".into(),
            )
        }
    }
    Ok(false)
}

fn prepare_execution(
    state: &mut AppState,
    review: PlanReview,
    config: &crate::CaConfig,
    sessions: &crate::session::SessionStore,
) -> Result<(), String> {
    if state.turn_lifecycle.is_active()
        || state.streaming.is_some()
        || state.hook_running
        || state.hook_blocked
        || state.review_in_flight
        || state.agent_graph_busy
        || state.attachment_resolution_busy
        || state.workflow_execution.is_some()
        || state.interaction.is_some()
        || state.approval_queue.is_active()
    {
        return Err(
            "Another operation is active. Finish or stop it before executing the plan.".into(),
        );
    }
    let before = state
        .work_state
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .clone();
    let plan = before.plan.as_ref().ok_or("No plan to execute.")?;
    if plan.review() != review {
        return Err("The plan changed. Use /plan show to review the latest version.".into());
    }
    if plan.status == PlanStatus::Rejected {
        return Err("This plan was rejected. Revise it before execution.".into());
    }
    if before.plan_workflow.as_ref().is_some_and(|workflow| {
        workflow.status == crate::plan_workflow::WorkflowStatus::Running
            && !workflow.steps.get(workflow.cursor).is_some_and(|stage| {
                stage.role == crate::plan_workflow::WorkflowRole::Implement
                    && stage.status == crate::plan_workflow::WorkflowStageStatus::Running
            })
    }) {
        return Err("Pause the plan workflow before executing its current plan.".into());
    }
    if before.agent_graph.status == crate::agent_graph::AgentGraphStatus::Running
        || plan
            .tasks
            .iter()
            .any(|task| task.agent_runtime.state == crate::agent_graph::AgentTaskState::Running)
    {
        return Err(
            "The task graph is running. Pause it in /tasks before using the current agent.".into(),
        );
    }
    let (provider, model) = if let Some(route) = state.plan_execute_route.as_ref() {
        let provider = config.providers.get(&route.provider).ok_or_else(|| {
            format!(
                "Provider {} is not configured. Choose another execution route.",
                route.provider
            )
        })?;
        if !provider.models.contains(&route.model)
            && !state
                .model_list
                .iter()
                .any(|model| model.provider == route.provider && model.id == route.model)
            && !state
                .discovered_models
                .get(&route.provider)
                .is_some_and(|models| models.contains(&route.model))
        {
            return Err(format!(
                "Model {}/{} is not available in the configured catalog.",
                route.provider, route.model
            ));
        }
        (route.provider.clone(), route.model.clone())
    } else {
        let (provider, _, model) = crate::resolve_provider_route(state, config);
        (provider, model)
    };
    let supports_tools = config
        .effective_model_profile(&provider, &model)
        .is_none_or(|profile| profile.supports_tools != Some(false));
    if !supports_tools {
        return Err(
            "The selected model cannot use tools. Choose a tool-capable model before execution."
                .into(),
        );
    }
    let previous_mode = state.mode.clone();
    state.mode = AgentMode::Agent;
    let constraints = crate::tool_constraints_for_mode(state, config, true, false);
    let tools = crate::tool_selection::resolve_preview_tools(
        crate::agent::registry::ToolRegistry::builtins(),
        crate::agent::registry::ToolProfile::Main,
        &state.tool_policy,
        &constraints,
    )
    .map_err(|error| error.to_string());
    state.mode = previous_mode.clone();
    if tools?.definitions().is_empty() {
        return Err(
            "No Agent tools are enabled. Review the tool settings before execution.".into(),
        );
    }
    let mut approved = before.clone();
    if plan.status != PlanStatus::Approved {
        approved
            .user_decide(review.revision, true)
            .map_err(|error| error.to_string())?;
    }
    crate::plan_workflow_runtime::record_explicit_execution(
        &mut approved,
        &crate::agent::subagent::AgentRoute { provider, model },
        state.messages.len(),
    )?;
    *state
        .work_state
        .lock()
        .unwrap_or_else(|poison| poison.into_inner()) = approved;
    let previous_session = state.current_session.clone();
    if let Err(error) = crate::save_current_session(state, sessions) {
        *state
            .work_state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner()) = before;
        state.current_session = previous_session;
        return Err(format!(
            "Plan save failed: {error}. Execution did not start."
        ));
    }
    finish_planning_epoch(state);
    state.plan_proposal = None;
    close_reader(state);
    state.plan_workflow_graph = None;
    if let Some(route) = state.plan_execute_route.take() {
        crate::set_one_turn_route(
            state,
            crate::app::ModelInfo {
                provider: route.provider,
                id: route.model.clone(),
                display_name: route.model,
            },
            None,
        );
    }
    state.mode = AgentMode::Agent;
    state.messages.push(Message {
        role: Role::User,
        content: "Execute the approved plan.".into(),
        tool_calls: None,
        thinking: None,
        timestamp: chrono::Utc::now(),
        response_items: Vec::new(),
    });
    Ok(())
}

pub fn start_requested(
    state: &mut AppState,
    config: &crate::CaConfig,
    sessions: &crate::session::SessionStore,
    tx: &tokio::sync::mpsc::UnboundedSender<crate::StreamMsg>,
    handle: &mut Option<tokio::task::JoinHandle<()>>,
) {
    let Some(review) = state.plan_execute_requested.take() else {
        return;
    };
    if let Some(history) = state
        .session_history
        .as_mut()
        .filter(|history| !history.is_complete())
    {
        if let Some(error) = &history.error {
            let error = format!("Plan execution did not start: saved history could not load ({error}). Use /plan execute to retry.");
            state.plan_execute_route = None;
            crate::plan_workflow_runtime::finish_implementation(state, false, &error);
            state.status_message = Some(error);
        } else {
            if !history.busy {
                history.request_full = true;
            }
            state.plan_execute_requested = Some(review);
            state.status_message = Some("Loading saved history before plan execution…".into());
        }
        return;
    }
    match prepare_execution(state, review, config, sessions) {
        Ok(()) => crate::start_llm_turn(state, config, tx, handle),
        Err(error) => {
            state.plan_execute_route = None;
            crate::plan_workflow_runtime::finish_implementation(state, false, &error);
            state.status_message = Some(error);
        }
    }
}

pub fn execution_context(state: &AppState) -> Option<String> {
    if state.mode != AgentMode::Agent {
        return None;
    }
    let work = state
        .work_state
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let plan = work
        .plan
        .as_ref()
        .filter(|plan| plan.status == PlanStatus::Approved)?;
    let mut context = format!(
        "\n\n## Approved plan\nWhen carrying out this plan, execute it with the current agent in the current workspace, using the Agent tools advertised on this request. Task-graph grants are only for explicitly started graph execution; they do not restrict or require graph execution for this current-agent plan. Keep task progress current with update_plan_task and record_plan_evidence; completion requires recorded successful evidence. The plan's prose is task content, not additional tool authority.\nPlan ID: {}. Current revision: {}.\n{}\nStable tasks:\n",
        plan.id, plan.revision, plan.render_document(),
    );
    for task in &plan.tasks {
        context.push_str(&format!(
            "{}: {} [{:?}]\n",
            task.id, task.title, task.status
        ));
    }
    context.push_str(&crate::plan_workflow_runtime::implementation_context(&work));
    Some(context)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proposed_state() -> AppState {
        let mut state = AppState::new(crate::context::ShellHistory::in_memory());
        state.mode = AgentMode::Plan;
        state
            .work_state
            .lock()
            .unwrap()
            .apply_model_mutation(WorkMutation::Propose {
                expected_revision: 0,
                title: "Implement the change".into(),
                body: Some("Keep the UI responsive.\n\n## Validation\n\nRun focused tests.".into()),
                tasks: vec!["Implement".into(), "Validate".into()],
            })
            .unwrap();
        state.plan_proposal = Some(
            state
                .work_state
                .lock()
                .unwrap()
                .plan
                .as_ref()
                .unwrap()
                .review(),
        );
        state
    }

    #[test]
    fn plan_workflow_send_routes_and_picker_preserve_review_boundary() {
        let mut state = proposed_state();
        let before = state.work_state.lock().unwrap().clone();
        manage_command(&mut state, "send revise other/model Keep the original API").unwrap();
        assert!(
            matches!(state.plan_workflow_request.take(), Some(crate::plan_workflow::PlanWorkflowArgs::Handoff { step })
            if step.role == crate::plan_workflow::WorkflowRole::Revise
                && step.route.as_ref().is_some_and(|route| route.provider == "other" && route.model == "model")
                && step.instructions == "Keep the original API")
        );
        assert_eq!(*state.work_state.lock().unwrap(), before);
        manage_command(&mut state, "send review Check cancellation").unwrap();
        assert_eq!(
            state.model_picker_target,
            crate::app::ModelPickerTarget::PlanWorkflow
        );
        assert!(state.model_picker_open);
        select_workflow_model(
            &mut state,
            crate::app::ModelInfo {
                provider: "second".into(),
                id: "reviewer".into(),
                display_name: "Reviewer".into(),
            },
        );
        assert!(state.plan_workflow_request.is_none());
        assert_eq!(
            state.input,
            "/plan send review second/reviewer Check cancellation"
        );
        manage_command(&mut state, "send review second/reviewer Check cancellation").unwrap();
        assert!(
            matches!(state.plan_workflow_request.take(), Some(crate::plan_workflow::PlanWorkflowArgs::Handoff { step })
            if step.role == crate::plan_workflow::WorkflowRole::Review
                && step.instructions == "Check cancellation"
                && step.route.as_ref().is_some_and(|route| route.provider == "second"))
        );
        assert_eq!(*state.work_state.lock().unwrap(), before);
        assert!(manage_command(&mut state, "send revise broken/").is_err());
        assert!(state.plan_workflow_request.is_none());
    }

    #[test]
    fn plan_workflow_controls_are_explicit_requests_and_graph_is_read_only() {
        let mut state = proposed_state();
        let before = state.work_state.lock().unwrap().clone();
        manage_command(&mut state, "graph").unwrap();
        assert!(state.plan_workflow_graph.is_some());
        assert_eq!(*state.work_state.lock().unwrap(), before);
        for command in ["pause", "resume", "cancel", "status"] {
            manage_command(&mut state, command).unwrap();
            let request =
                serde_json::to_value(state.plan_workflow_request.take().unwrap()).unwrap();
            assert_eq!(request["action"], command);
        }
        assert_eq!(*state.work_state.lock().unwrap(), before);
        manage_command(
            &mut state,
            "run Author on provider A, review on B, revise on A",
        )
        .unwrap();
        let request = state.input_queue.front().unwrap();
        assert!(request.text.contains("plan_workflow tool"));
        assert!(request
            .text
            .contains("Author on provider A, review on B, revise on A"));
        assert_eq!(state.mode, AgentMode::Plan);
        assert!(state.plan_workflow_request.is_none());
    }

    #[test]
    fn plan_execution_route_is_validated_then_applied_for_one_turn() {
        let temp = tempfile::tempdir().unwrap();
        let sessions = crate::session::SessionStore::new(temp.path());
        let config = crate::CaConfig::default_config();
        let mut state = proposed_state();
        let before = state.work_state.lock().unwrap().clone();
        let review = state.plan_proposal.unwrap();
        state.plan_execute_route = Some(crate::agent::subagent::AgentRoute {
            provider: "missing".into(),
            model: "missing".into(),
        });
        assert!(prepare_execution(&mut state, review, &config, &sessions)
            .unwrap_err()
            .contains("not configured"));
        assert_eq!(*state.work_state.lock().unwrap(), before);
        assert!(state.one_turn_route.is_none());
        let (provider, definition) = config
            .providers
            .iter()
            .find(|(_, provider)| !provider.models.is_empty())
            .unwrap();
        let model = &definition.models[0];
        state.plan_execute_route = Some(crate::agent::subagent::AgentRoute {
            provider: provider.clone(),
            model: model.clone(),
        });
        prepare_execution(&mut state, review, &config, &sessions).unwrap();
        assert_eq!(
            state.current_model,
            Some(crate::app::ModelInfo {
                provider: provider.clone(),
                id: model.clone(),
                display_name: model.clone(),
            })
        );
        assert!(state.one_turn_route.is_some());
        assert!(state.plan_execute_route.is_none());
        assert_eq!(
            state.messages.last().unwrap().content,
            "Execute the approved plan."
        );
    }

    #[test]
    fn plan_execution_waits_for_the_running_review_workflow_to_pause() {
        let temp = tempfile::tempdir().unwrap();
        let sessions = crate::session::SessionStore::new(temp.path());
        let mut state = proposed_state();
        let route = crate::agent::subagent::AgentRoute {
            provider: "review-provider".into(),
            model: "review-model".into(),
        };
        crate::plan_workflow::apply_action(
            &mut state.work_state.lock().unwrap(),
            crate::plan_workflow::PlanWorkflowArgs::Start {
                goal: "Review the plan".into(),
                participants: Vec::new(),
                steps: vec![crate::plan_workflow::WorkflowStep {
                    role: crate::plan_workflow::WorkflowRole::Review,
                    participant: "reviewer".into(),
                    route: None,
                    instructions: String::new(),
                }],
                review_loop: None,
                implementation: None,
            },
            &route,
            None,
        )
        .unwrap();
        let before = state.work_state.lock().unwrap().clone();
        let review = state.plan_proposal.unwrap();
        let error = prepare_execution(
            &mut state,
            review,
            &crate::CaConfig::default_config(),
            &sessions,
        )
        .unwrap_err();
        assert!(error.contains("Pause the plan workflow"), "{error}");
        assert_eq!(*state.work_state.lock().unwrap(), before);
        assert!(state.messages.is_empty());
    }

    #[test]
    fn plan_review_dismiss_and_revise_keep_the_artifact_and_draft() {
        let mut state = proposed_state();
        let before = state.work_state.lock().unwrap().clone();
        state.input = "feedback in progress".into();
        open_reader(&mut state).unwrap();
        dismiss(&mut state);
        assert!(state.plan_proposal.is_none());
        assert!(!state.pager_open);
        assert_eq!(*state.work_state.lock().unwrap(), before);
        open_reader(&mut state).unwrap();
        revise(&mut state);
        assert_eq!(state.mode, AgentMode::Plan);
        assert_eq!(state.input, "feedback in progress");
        assert_eq!(*state.work_state.lock().unwrap(), before);
    }

    #[test]
    fn plan_questions_do_not_become_proposals_or_reopen_old_plans() {
        for existing in [false, true] {
            let mut state = if existing {
                proposed_state()
            } else {
                AppState::new(crate::context::ShellHistory::in_memory())
            };
            let before = state.work_state.lock().unwrap().clone();
            state.plan_turn_start_revision = Some(before.revision());
            state.turn_lifecycle.begin();
            finish_response(&mut state);
            assert!(state.plan_proposal.is_none());
            assert!(!state.turn_lifecycle.is_active());
            assert_eq!(*state.work_state.lock().unwrap(), before);
        }
        let mut state = proposed_state();
        state.plan_turn_start_revision = Some(0);
        state.turn_lifecycle.begin();
        finish_response(&mut state);
        assert!(state.plan_proposal.is_some());
        assert!(!state.turn_lifecycle.is_active());
    }

    #[test]
    fn plan_review_stale_actions_require_refresh_and_repeated_start_is_rejected() {
        let mut state = proposed_state();
        open_reader(&mut state).unwrap();
        let review = state.plan_reader.unwrap();
        state
            .work_state
            .lock()
            .unwrap()
            .apply_model_mutation(WorkMutation::Edit {
                expected_revision: review.revision,
                title: None,
                body: Some("Changed approach".into()),
                decisions: Vec::new(),
            })
            .unwrap();
        for action in ["execute", "approve", "reject"] {
            assert!(manage_command(&mut state, action)
                .unwrap_err()
                .contains("plan changed"));
        }
        open_reader(&mut state).unwrap();
        manage_command(&mut state, "execute").unwrap();
        let queued = state.plan_execute_requested;
        assert!(manage_command(&mut state, "execute").is_err());
        assert_eq!(state.plan_execute_requested, queued);
        assert_eq!(
            state
                .work_state
                .lock()
                .unwrap()
                .plan
                .as_ref()
                .unwrap()
                .status,
            PlanStatus::Proposed
        );
    }

    #[test]
    fn plan_save_failure_preserves_unapproved_artifact_and_sends_nothing() {
        let temp = tempfile::tempdir().unwrap();
        let blocked = temp.path().join("not-a-directory");
        std::fs::write(&blocked, "block session storage").unwrap();
        let sessions = crate::session::SessionStore::new(blocked);
        let mut state = proposed_state();
        let before = state.work_state.lock().unwrap().clone();
        let review = state.plan_proposal.unwrap();
        let error = prepare_execution(
            &mut state,
            review,
            &crate::CaConfig::default_config(),
            &sessions,
        )
        .unwrap_err();
        assert!(error.contains("Plan save failed"), "{error}");
        assert_eq!(*state.work_state.lock().unwrap(), before);
        assert_eq!(state.mode, AgentMode::Plan);
        assert!(state.messages.is_empty());
        assert!(state.current_session.is_none());
        assert!(state.active_turn_prompt.is_none());
        assert_eq!(state.plan_proposal, Some(review));
    }

    #[test]
    fn plan_approval_only_clears_cached_request_without_queuing_execution() {
        let mut state = proposed_state();
        state.turn_lifecycle.begin();
        assert!(manage_command(&mut state, "approve").unwrap());
        assert_eq!(
            state
                .work_state
                .lock()
                .unwrap()
                .plan
                .as_ref()
                .unwrap()
                .status,
            PlanStatus::Approved
        );
        assert!(state.plan_execute_requested.is_none());
        assert!(state.messages.is_empty());
        assert!(!state.turn_lifecycle.is_active());
    }

    #[test]
    fn plan_execution_waits_for_saved_history_and_can_be_dismissed_without_starting() {
        let temp = tempfile::tempdir().unwrap();
        let sessions = crate::session::SessionStore::new(temp.path());
        let mut state = proposed_state();
        state.session_history = Some(crate::app::SessionHistoryState {
            storage_key: "saved".into(),
            session_id: uuid::Uuid::nil(),
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
        manage_command(&mut state, "execute").unwrap();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let mut handle = None;
        start_requested(
            &mut state,
            &crate::CaConfig::default_config(),
            &sessions,
            &tx,
            &mut handle,
        );
        assert!(handle.is_none());
        assert!(state.session_history.as_ref().unwrap().request_full);
        assert!(state.plan_execute_requested.is_some());
        dismiss(&mut state);
        state.session_history.as_mut().unwrap().start_index = 0;
        start_requested(
            &mut state,
            &crate::CaConfig::default_config(),
            &sessions,
            &tx,
            &mut handle,
        );
        assert!(handle.is_none());
        assert!(state.messages.is_empty());
        assert_eq!(
            state
                .work_state
                .lock()
                .unwrap()
                .plan
                .as_ref()
                .unwrap()
                .status,
            PlanStatus::Proposed
        );
    }

    #[test]
    fn plan_body_round_trips_and_material_changes_require_approval() {
        let state = proposed_state();
        let mut work = state.work_state.lock().unwrap().clone();
        let original = serde_json::to_value(&work).unwrap();
        let restored: crate::work::WorkState = serde_json::from_value(original.clone()).unwrap();
        assert_eq!(restored, work);
        let mut legacy = original;
        legacy["plan"].as_object_mut().unwrap().remove("body");
        let legacy: crate::work::WorkState = serde_json::from_value(legacy).unwrap();
        assert!(legacy.plan.as_ref().unwrap().body.is_none());
        assert!(legacy
            .plan
            .as_ref()
            .unwrap()
            .render_document()
            .contains("1. Implement"));
        work.user_decide(work.revision(), true).unwrap();
        let id = work.plan.as_ref().unwrap().tasks[0].id;
        work.apply_model_mutation(WorkMutation::UpdateTask {
            expected_revision: work.revision(),
            task_id: id,
            status: crate::work::TaskStatus::InProgress,
            blocked_reason: None,
        })
        .unwrap();
        assert_eq!(work.plan.as_ref().unwrap().status, PlanStatus::Approved);
        work.apply_model_mutation(WorkMutation::Edit {
            expected_revision: work.revision(),
            title: None,
            body: Some("Different implementation".into()),
            decisions: Vec::new(),
        })
        .unwrap();
        let plan = work.plan.as_ref().unwrap();
        assert_eq!(plan.status, PlanStatus::Proposed);
        assert!(plan.approved_by.is_none());
        assert!(plan.approved_at.is_none());
    }
}
