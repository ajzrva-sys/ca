use crate::app::AppState;
use crate::context::ShellHistory;
use crate::tui::event::Event;
use crate::tui::input::handle_event;
use crossterm::event::{KeyCode, KeyModifiers};

fn state_with_input(input: &str, cursor: usize) -> AppState {
    let mut state = AppState::new(ShellHistory::in_memory());
    state.input = input.to_string();
    state.input_cursor = cursor;
    state
}

fn plan_review_state() -> AppState {
    let mut state = state_with_input("", 0);
    state.mode = crate::app::AgentMode::Plan;
    state
        .work_state
        .lock()
        .unwrap()
        .apply_model_mutation(crate::work::WorkMutation::Propose {
            expected_revision: 0,
            title: "Reviewed plan".into(),
            body: Some("Readable body".into()),
            tasks: vec!["Test the change".into()],
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
fn plan_review_empty_enter_executes_but_typing_and_paste_remain_editable() {
    let mut state = plan_review_state();
    press(&mut state, KeyCode::Enter);
    assert!(
        matches!(state.pending_input_action.take(), Some(crate::tui::commands::InputAction::Command(action)) if action.args == "execute")
    );
    press(&mut state, KeyCode::Char('r'));
    assert_eq!(state.input, "r");
    handle_event(&mut state, &Event::Paste("evise this".into()), ".");
    assert_eq!(state.input, "revise this");
    // Ordinary nonempty submission belongs to the event loop, not this handler.
    assert!(!handle_event(
        &mut state,
        &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
        "."
    ));
    assert!(state.pending_input_action.is_none());
    assert!(state.plan_proposal.is_some());
}

#[test]
fn plan_reader_view_revision_dismissal_and_scrolling_are_independent_of_execution() {
    let mut state = plan_review_state();
    let before = state.work_state.lock().unwrap().clone();
    handle_event(
        &mut state,
        &Event::Key(KeyCode::Char('o'), KeyModifiers::CONTROL),
        ".",
    );
    assert!(state.pager_open && state.plan_reader.is_some());
    assert!(state.pager_content.contains("Readable body"));
    state.plan_reader_max_scroll.set(40);
    press(&mut state, KeyCode::End);
    assert_eq!(state.pager_scroll, 40);
    press(&mut state, KeyCode::Up);
    assert_eq!(state.pager_scroll, 39);
    press(&mut state, KeyCode::PageUp);
    assert_eq!(state.pager_scroll, 24);
    press(&mut state, KeyCode::Char('r'));
    assert!(!state.pager_open);
    assert!(state.plan_proposal.is_none());
    assert_eq!(state.mode, crate::app::AgentMode::Plan);
    for key in [KeyCode::Esc, KeyCode::Char('q')] {
        crate::plan::open_reader(&mut state).unwrap();
        press(&mut state, key);
        assert!(!state.pager_open);
        assert!(state.plan_reader.is_none());
        assert!(state.plan_proposal.is_none());
    }
    assert_eq!(*state.work_state.lock().unwrap(), before);
}

#[test]
fn plan_workflow_picker_cancellation_and_selection_do_not_switch_the_main_agent() {
    let mut state = plan_review_state();
    state.model_catalog_loaded = true;
    state.model_list = vec![crate::app::ModelInfo {
        provider: "other".into(),
        id: "planner".into(),
        display_name: "Planner".into(),
    }];
    let before = state.work_state.lock().unwrap().clone();
    crate::plan::manage_command(&mut state, "send review Check the API").unwrap();
    press(&mut state, KeyCode::Esc);
    assert!(state.plan_workflow_request.is_none());
    assert!(state.plan_workflow_picker_action.is_none());
    assert!(state.current_model.is_none());
    crate::plan::manage_command(&mut state, "send review Check the API").unwrap();
    press(&mut state, KeyCode::Enter);
    assert!(state.plan_workflow_request.is_none());
    assert_eq!(state.input, "/plan send review other/planner Check the API");
    crate::plan::manage_command(&mut state, "send revise other/planner Check the API").unwrap();
    assert!(
        matches!(state.plan_workflow_request.take(), Some(crate::plan_workflow::PlanWorkflowArgs::Handoff { step })
        if step.route.as_ref().is_some_and(|route| route.provider == "other" && route.model == "planner") && step.instructions == "Check the API" && step.role == crate::plan_workflow::WorkflowRole::Revise)
    );
    assert!(state.current_model.is_none());
    assert_eq!(*state.work_state.lock().unwrap(), before);
    crate::plan::manage_command(&mut state, "execute choose").unwrap();
    press(&mut state, KeyCode::Enter);
    assert!(state.plan_execute_requested.is_some());
    assert!(state
        .plan_execute_route
        .as_ref()
        .is_some_and(|route| route.provider == "other"));
    assert!(state.current_model.is_none());
    assert_eq!(*state.work_state.lock().unwrap(), before);
}

#[test]
fn plan_workflow_graph_inspection_and_return_to_existing_agent_are_distinct_actions() {
    use crate::plan_workflow::{PlanWorkflowArgs, WorkflowRole, WorkflowStep};
    let mut state = plan_review_state();
    let route = crate::agent::subagent::AgentRoute {
        provider: "first".into(),
        model: "author-model".into(),
    };
    {
        let mut work = state.work_state.lock().unwrap();
        crate::plan_workflow::apply_action(
            &mut work,
            PlanWorkflowArgs::Start {
                goal: "Review and return the same plan".into(),
                participants: Vec::new(),
                steps: vec![WorkflowStep {
                    role: WorkflowRole::Review,
                    participant: "original-author".into(),
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
        let workflow = work.plan_workflow.as_mut().unwrap();
        workflow.status = crate::plan_workflow::WorkflowStatus::Completed;
        workflow.steps[0].status = crate::plan_workflow::WorkflowStageStatus::Completed;
        workflow.steps[0].result_snapshot = Some(0);
        workflow.steps[0].feedback = Some("Check cancellation before implementation".into());
    }
    let before = state.work_state.lock().unwrap().clone();
    crate::plan::manage_command(&mut state, "graph").unwrap();
    press(&mut state, KeyCode::End);
    assert_eq!(state.plan_workflow_graph.as_ref().unwrap().selected_idx, 0);
    press(&mut state, KeyCode::Enter);
    assert!(state.pager_open);
    assert!(state.plan_reader.is_none());
    assert!(state.pager_content.contains("Readable body"));
    assert!(state.pager_content.contains("Check cancellation"));
    press(&mut state, KeyCode::Enter);
    assert!(state.plan_execute_requested.is_none());
    press(&mut state, KeyCode::Esc);
    assert!(!state.pager_open);
    assert!(state.plan_workflow_graph.is_some());
    assert_eq!(*state.work_state.lock().unwrap(), before);
    press(&mut state, KeyCode::Tab);
    assert!(state.plan_workflow_graph.as_ref().unwrap().participants);
    press(&mut state, KeyCode::Enter);
    assert!(state.plan_workflow_request.is_none());
    assert!(state.plan_workflow_graph.is_none());
    assert_eq!(state.input, "/plan send revise original-author ");
    crate::plan::manage_command(
        &mut state,
        "send review original-author Verify the updated plan",
    )
    .unwrap();
    assert!(
        matches!(state.plan_workflow_request.take(), Some(PlanWorkflowArgs::Handoff { step })
        if step.participant == "original-author" && step.route.is_none() && step.role == WorkflowRole::Review && step.instructions == "Verify the updated plan")
    );
    assert_eq!(*state.work_state.lock().unwrap(), before);
}

#[test]
fn new_session_command_executes_exactly_and_closes_the_palette() {
    let mut state = state_with_input("/new", 4);
    state.slash_palette_open = true;
    state.slash_filter = "new".into();
    state.selected_slash_idx = 99;
    press(&mut state, KeyCode::Enter);
    assert_eq!(
        state.pending_input_action,
        Some(crate::tui::commands::InputAction::Command(
            crate::tui::commands::CommandAction {
                id: crate::tui::commands::CommandId::New,
                args: String::new(),
            }
        ))
    );
    assert!(state.input.is_empty());
    assert!(!state.slash_palette_open);
}

fn config_hub_items() -> Vec<crate::app::ConfigHubItem> {
    vec![
        crate::app::ConfigHubItem {
            label: "Current model".into(),
            value: "mimo/mimo-v2.5-pro".into(),
            keywords: "provider switch".into(),
            action: crate::app::ConfigHubAction::CurrentModel,
        },
        crate::app::ConfigHubItem {
            label: "Review model".into(),
            value: "use current default".into(),
            keywords: "reviewer".into(),
            action: crate::app::ConfigHubAction::ReviewModel,
        },
    ]
}

fn sandbox_hub(writes_enabled: bool) -> crate::app::SandboxHubState {
    crate::app::SandboxHubState {
        saved_mode: crate::sandbox::SandboxMode::Required,
        effective_mode: crate::sandbox::SandboxMode::Required,
        backend_available: true,
        writable_roots: vec!["/tmp/project".into()],
        selected_idx: 0,
        confirmation: None,
        busy: false,
        writes_enabled,
        notice: None,
        error: None,
    }
}

#[test]
fn sandbox_downgrade_confirmation_is_cancel_first_and_paste_isolated() {
    let mut state = state_with_input("keep this draft", 15);
    state.sandbox_hub = Some(sandbox_hub(true));

    assert!(handle_event(
        &mut state,
        &Event::Paste("off".into()),
        "/tmp/sandbox-hub"
    ));
    assert_eq!(state.input, "keep this draft");
    press(&mut state, KeyCode::End);
    press(&mut state, KeyCode::Enter);
    let confirmation = state.sandbox_hub.as_ref().unwrap().confirmation.unwrap();
    assert_eq!(confirmation.target, crate::sandbox::SandboxMode::Off);
    assert!(!confirmation.confirm_selected);

    press(&mut state, KeyCode::Enter);
    assert!(state.sandbox_hub.as_ref().unwrap().confirmation.is_none());
    assert!(state.pending_input_action.is_none());
}

#[test]
fn trace_delete_confirmation_defaults_to_cancel_and_emits_one_exact_action() {
    let id = uuid::Uuid::parse_str("00000000-0000-0000-0000-000000000117").unwrap();
    let mut state = state_with_input("keep draft", 10);
    state.trace_delete_confirmation = Some(crate::app::TraceDeleteConfirmation {
        trace_id: id,
        bytes: 2048,
        delete_selected: false,
        error: None,
    });

    press(&mut state, KeyCode::Enter);
    assert!(state.trace_delete_confirmation.is_none());
    assert!(state.pending_input_action.is_none());
    assert_eq!(state.input, "keep draft");

    state.trace_delete_confirmation = Some(crate::app::TraceDeleteConfirmation {
        trace_id: id,
        bytes: 2048,
        delete_selected: false,
        error: None,
    });
    press(&mut state, KeyCode::Right);
    press(&mut state, KeyCode::Enter);
    assert_eq!(
        state.pending_input_action.take(),
        Some(crate::tui::commands::InputAction::DeleteTrace(id))
    );
    assert_eq!(state.input, "keep draft");
}

#[test]
fn project_init_preview_defaults_to_cancel_and_preserves_composer() {
    let mut state = state_with_input("keep draft", 10);
    state.project_init = Some(crate::app::ProjectInitState {
        root: "/workspace/demo".into(),
        draft: Some(crate::project_init::test_draft(
            "/workspace/demo",
            "# Instructions\n\nRun tests.\n",
        )),
        phase: crate::app::ProjectInitPhase::Preview,
        scroll: 0,
        create_selected: false,
        error: None,
    });

    press(&mut state, KeyCode::Enter);
    assert_eq!(
        state.pending_input_action.take(),
        Some(crate::tui::commands::InputAction::CancelProjectInit)
    );
    assert_eq!(state.input, "keep draft");

    press(&mut state, KeyCode::Right);
    press(&mut state, KeyCode::Enter);
    assert!(matches!(
        state.pending_input_action.take(),
        Some(crate::tui::commands::InputAction::CreateProjectInstructions(_))
    ));
    assert_eq!(state.input, "keep draft");
}

#[test]
fn project_init_preparation_can_cancel_but_writing_is_locked() {
    let mut state = state_with_input("draft", 5);
    state.project_init = Some(crate::app::ProjectInitState {
        root: "/workspace/demo".into(),
        draft: None,
        phase: crate::app::ProjectInitPhase::Preparing,
        scroll: 0,
        create_selected: false,
        error: None,
    });
    press_ctrl(&mut state, 'c');
    assert_eq!(
        state.pending_input_action.take(),
        Some(crate::tui::commands::InputAction::CancelProjectInit)
    );
    state.project_init.as_mut().unwrap().phase = crate::app::ProjectInitPhase::Writing;
    press(&mut state, KeyCode::Esc);
    assert!(state.pending_input_action.is_none());
    assert_eq!(state.input, "draft");
}

#[test]
fn sandbox_confirmation_emits_one_locked_save_action() {
    let mut state = state_with_input("", 0);
    state.sandbox_hub = Some(sandbox_hub(true));
    press(&mut state, KeyCode::End);
    press(&mut state, KeyCode::Enter);
    press(&mut state, KeyCode::Right);
    press(&mut state, KeyCode::Enter);

    assert_eq!(
        state.pending_input_action.take(),
        Some(crate::tui::commands::InputAction::SaveSandboxMode(
            crate::sandbox::SandboxMode::Off
        ))
    );
    assert!(state.sandbox_hub.as_ref().unwrap().busy);
    press(&mut state, KeyCode::Esc);
    assert!(state.sandbox_hub.is_some());
}

#[test]
fn sandbox_required_saves_directly_and_bare_is_view_only() {
    let mut state = state_with_input("", 0);
    let mut hub = sandbox_hub(true);
    hub.saved_mode = crate::sandbox::SandboxMode::Off;
    state.sandbox_hub = Some(hub);
    press(&mut state, KeyCode::Enter);
    assert_eq!(
        state.pending_input_action.take(),
        Some(crate::tui::commands::InputAction::SaveSandboxMode(
            crate::sandbox::SandboxMode::Required
        ))
    );
    assert!(state.sandbox_hub.as_ref().unwrap().confirmation.is_none());

    let mut bare = state_with_input("", 0);
    bare.sandbox_hub = Some(sandbox_hub(false));
    press(&mut bare, KeyCode::End);
    press(&mut bare, KeyCode::Enter);
    let hub = bare.sandbox_hub.as_ref().unwrap();
    assert!(hub.confirmation.is_none());
    assert!(hub.error.as_deref().unwrap().contains("--profile bare"));
    assert!(bare.pending_input_action.is_none());
}

#[test]
fn config_hub_filters_pastes_navigates_and_routes_without_touching_composer() {
    let mut state = state_with_input("draft", 5);
    state.config_hub = Some(crate::app::ConfigHubState {
        filter: String::new(),
        selected_idx: 0,
        items: config_hub_items(),
    });

    assert!(handle_event(
        &mut state,
        &Event::Paste("review".into()),
        "/tmp/config-hub"
    ));
    assert_eq!(state.input, "draft");
    assert_eq!(state.config_hub.as_ref().unwrap().filter, "review");
    press(&mut state, KeyCode::Enter);
    assert!(state.config_hub.is_none());
    assert!(state.model_picker_open);
    assert_eq!(
        state.model_picker_target,
        crate::app::ModelPickerTarget::Review
    );

    state.model_list = vec![crate::app::ModelInfo {
        id: "reviewer".into(),
        provider: "safe".into(),
        display_name: "reviewer".into(),
    }];
    press(&mut state, KeyCode::Enter);
    assert_eq!(
        state.pending_input_action.take(),
        Some(crate::tui::commands::InputAction::SelectReviewModel(
            state.model_list[0].clone()
        ))
    );
    assert_eq!(state.input, "draft");
}

#[test]
fn config_hub_routes_sandbox_to_the_shared_manager_command() {
    let mut state = state_with_input("preserved", 9);
    state.config_hub = Some(crate::app::ConfigHubState {
        filter: String::new(),
        selected_idx: 0,
        items: vec![crate::app::ConfigHubItem {
            label: "Sandbox".into(),
            value: "required saved · required active".into(),
            keywords: "process isolation".into(),
            action: crate::app::ConfigHubAction::Sandbox,
        }],
    });

    press(&mut state, KeyCode::Enter);

    assert_eq!(
        state.pending_input_action.take(),
        Some(crate::tui::commands::InputAction::Command(
            crate::tui::commands::CommandAction {
                id: crate::tui::commands::CommandId::Sandbox,
                args: String::new(),
            }
        ))
    );
    assert_eq!(state.input, "preserved");
}

fn press(state: &mut AppState, code: KeyCode) {
    assert!(handle_event(
        state,
        &Event::Key(code, KeyModifiers::NONE),
        "/tmp/ca-uni004-test",
    ));
    assert!(state.input.is_char_boundary(state.input_cursor));
    assert!(state.input_cursor <= state.input.len());
}

fn press_ctrl(state: &mut AppState, c: char) {
    assert!(handle_event(
        state,
        &Event::Key(KeyCode::Char(c), KeyModifiers::CONTROL),
        "/tmp/ca-uni004-test",
    ));
    assert!(state.input.is_char_boundary(state.input_cursor));
    assert!(state.input_cursor <= state.input.len());
}

fn press_with_modifiers(state: &mut AppState, code: KeyCode, modifiers: KeyModifiers) {
    assert!(handle_event(
        state,
        &Event::Key(code, modifiers),
        "/tmp/ca-uni004-test",
    ));
}

fn loop_guard_state() -> AppState {
    let mut state = state_with_input("preserved draft", 15);
    state.turn_lifecycle.begin();
    state
        .turn_lifecycle
        .set_phase(crate::activity::ActivityPhase::Paused);
    state.soft_pause =
        crate::activity::SoftPauseState::Paused(crate::activity::SoftPauseBoundary::Provider);
    state.loop_guard_interaction = Some(crate::loop_guard::LoopGuardInteraction::new(
        crate::loop_guard::LoopIncident {
            kind: crate::loop_guard::LoopIncidentKind::RepeatedFailedCommand,
            evidence: "Failing command `cargo test` returned unchanged output 3 times".into(),
            occurrences: 3,
        },
    ));
    state
}

#[test]
fn loop_guard_replan_is_one_shot_process_only_resume() {
    let mut state = loop_guard_state();
    press(&mut state, KeyCode::Enter);
    assert!(state.loop_guard_interaction.is_none());
    assert!(state
        .loop_guard_replan_prompt
        .as_deref()
        .is_some_and(|prompt| { prompt.contains("materially different approach") }));
    assert_eq!(
        state.soft_pause,
        crate::activity::SoftPauseState::ResumeRequested(
            crate::activity::SoftPauseBoundary::Provider
        )
    );
    assert_eq!(
        state.pending_input_action,
        Some(crate::tui::commands::InputAction::ResumeLoopGuard)
    );
    assert!(state.messages.is_empty());
    assert_eq!(state.input, "preserved draft");
}

#[test]
fn loop_guard_continue_suppresses_current_turn() {
    let mut state = loop_guard_state();
    press(&mut state, KeyCode::Right);
    press(&mut state, KeyCode::Enter);
    assert!(state.loop_guard_interaction.is_none());
    assert!(state.loop_guard.is_suppressed());
    assert!(state.loop_guard_replan_prompt.is_none());
    assert_eq!(
        state.soft_pause,
        crate::activity::SoftPauseState::ResumeRequested(
            crate::activity::SoftPauseBoundary::Provider
        )
    );
    assert_eq!(
        state.pending_input_action,
        Some(crate::tui::commands::InputAction::ResumeLoopGuard)
    );
}

#[test]
fn loop_guard_cancel_confirmation_defaults_to_keep_paused() {
    let mut state = loop_guard_state();
    press(&mut state, KeyCode::Char('c'));
    assert!(state
        .loop_guard_interaction
        .as_ref()
        .is_some_and(
            |interaction| interaction.cancel_confirmation && !interaction.cancel_confirm_selected
        ));
    press(&mut state, KeyCode::Enter);
    assert!(state.loop_guard_interaction.is_some());
    assert!(state.pending_input_action.is_none());

    press(&mut state, KeyCode::Char('c'));
    press(&mut state, KeyCode::Right);
    press(&mut state, KeyCode::Enter);
    assert!(state.loop_guard_interaction.is_none());
    assert_eq!(
        state.pending_input_action.take(),
        Some(crate::tui::commands::InputAction::StopTurn)
    );
}

#[test]
fn model_picker_opens_contextual_add_provider_and_edit_actions() {
    let mut state = state_with_input("", 0);
    state.model_picker_open = true;
    state.model_list = vec![crate::app::ModelInfo {
        id: "coder-v1".into(),
        provider: "local".into(),
        display_name: "coder-v1 (local)".into(),
    }];

    let selected = state.model_list[0].clone();
    press_with_modifiers(&mut state, KeyCode::Char('A'), KeyModifiers::SHIFT);
    assert!(!state.model_picker_open);
    assert_eq!(
        state.pending_input_action.take(),
        Some(crate::tui::commands::InputAction::OpenProviderEditor(
            crate::app::ProviderEditorRequest::AddModel(selected.clone())
        ))
    );

    state.model_picker_open = true;
    press_with_modifiers(&mut state, KeyCode::Char('P'), KeyModifiers::SHIFT);
    assert_eq!(
        state.pending_input_action.take(),
        Some(crate::tui::commands::InputAction::OpenProviderEditor(
            crate::app::ProviderEditorRequest::AddProvider
        ))
    );

    state.model_picker_open = true;
    press_with_modifiers(&mut state, KeyCode::Char('E'), KeyModifiers::SHIFT);
    assert_eq!(
        state.pending_input_action,
        Some(crate::tui::commands::InputAction::OpenProviderEditor(
            crate::app::ProviderEditorRequest::EditModel(selected)
        ))
    );
}

#[test]
fn advertised_ctrl_k_opens_picker_on_current_model_without_changing_composer() {
    let mut state = state_with_input("keep this draft", "keep this draft".len());
    state.model_list = vec![
        crate::app::ModelInfo {
            id: "first".into(),
            provider: "one".into(),
            display_name: "first".into(),
        },
        crate::app::ModelInfo {
            id: "current".into(),
            provider: "two".into(),
            display_name: "current".into(),
        },
    ];
    state.current_model = Some(state.model_list[1].clone());

    press_ctrl(&mut state, 'k');

    assert!(state.model_picker_open);
    assert_eq!(state.selected_model_idx, 1);
    assert_eq!(state.input, "keep this draft");
    assert_eq!(state.input_cursor, "keep this draft".len());
    assert!(state.model_catalog_refresh_requested);

    state.model_picker_open = false;
    state.vim_mode = true;
    state.vim_state = crate::app::VimMode::Normal;
    press_ctrl(&mut state, 'k');
    assert!(state.model_picker_open);
    assert_eq!(state.selected_model_idx, 1);
}

#[test]
fn model_picker_favorite_toggle_is_explicit_and_bare_refuses_it() {
    let home = tempfile::tempdir().unwrap();
    let mut state = state_with_input("untouched", 9);
    state.model_picker_open = true;
    state.model_list = vec![crate::app::ModelInfo {
        id: "coder".into(),
        provider: "local".into(),
        display_name: "coder".into(),
    }];
    state.model_preferences_store = crate::model_preferences::ModelPreferencesStore::for_profile(
        home.path(),
        &home.path().join("profile"),
    )
    .unwrap();

    press_with_modifiers(&mut state, KeyCode::Char('F'), KeyModifiers::SHIFT);
    assert_eq!(
        state.pending_input_action.take(),
        Some(crate::tui::commands::InputAction::ToggleModelFavorite(
            state.model_list[0].clone()
        ))
    );
    assert!(state.model_picker_open);
    assert_eq!(state.input, "untouched");

    state.active_profile = crate::runtime::ActiveProfile::Bare;
    state.model_preferences_store = crate::model_preferences::ModelPreferencesStore::unavailable();
    press_with_modifiers(&mut state, KeyCode::Char('F'), KeyModifiers::SHIFT);
    assert!(state.pending_input_action.is_none());
    assert_eq!(
        state.status_message.as_deref(),
        Some("Model favorites are unavailable in bare mode")
    );
}

#[test]
fn recent_and_favorite_chords_cycle_only_available_scoped_models() {
    let mut state = state_with_input("", 0);
    state.model_list = vec![
        crate::app::ModelInfo {
            id: "one".into(),
            provider: "p".into(),
            display_name: "one".into(),
        },
        crate::app::ModelInfo {
            id: "two".into(),
            provider: "p".into(),
            display_name: "two".into(),
        },
        crate::app::ModelInfo {
            id: "three".into(),
            provider: "p".into(),
            display_name: "three".into(),
        },
    ];
    state
        .model_preferences
        .record_selection(&state.model_list[0], chrono::Utc::now())
        .unwrap();
    state
        .model_preferences
        .record_selection(&state.model_list[1], chrono::Utc::now())
        .unwrap();
    state
        .model_preferences
        .toggle_favorite(&state.model_list[0])
        .unwrap();
    state
        .model_preferences
        .toggle_favorite(&state.model_list[2])
        .unwrap();

    state.current_model = Some(state.model_list[1].clone());
    press_ctrl(&mut state, 'x');
    press(&mut state, KeyCode::Char('r'));
    assert_eq!(
        state.pending_input_action.take(),
        Some(crate::tui::commands::InputAction::SelectModel(
            state.model_list[0].clone()
        ))
    );

    state.current_model = Some(state.model_list[0].clone());
    press_ctrl(&mut state, 'x');
    press(&mut state, KeyCode::Char('f'));
    assert_eq!(
        state.pending_input_action.take(),
        Some(crate::tui::commands::InputAction::SelectModel(
            state.model_list[2].clone()
        ))
    );

    press_ctrl(&mut state, 'p');
    assert_eq!(
        state.pending_input_action,
        Some(crate::tui::commands::InputAction::SelectModel(
            state.model_list[1].clone()
        ))
    );
}

#[test]
fn model_picker_long_list_navigation_wraps_and_pages() {
    let mut state = state_with_input("", 0);
    state.model_picker_open = true;
    state.model_list = (0..100)
        .map(|index| crate::app::ModelInfo {
            id: format!("model-{index:03}"),
            provider: "large".into(),
            display_name: format!("model-{index:03}"),
        })
        .collect();

    press(&mut state, KeyCode::End);
    assert_eq!(state.selected_model_idx, 99);
    press(&mut state, KeyCode::Down);
    assert_eq!(state.selected_model_idx, 0);
    press(&mut state, KeyCode::Up);
    assert_eq!(state.selected_model_idx, 99);
    press(&mut state, KeyCode::PageUp);
    assert!(state.selected_model_idx < 99);
    press(&mut state, KeyCode::Home);
    assert_eq!(state.selected_model_idx, 0);
    press(&mut state, KeyCode::PageDown);
    assert!(state.selected_model_idx > 0);
    press(&mut state, KeyCode::End);
    press(&mut state, KeyCode::Enter);
    assert!(!state.model_picker_open);
    assert_eq!(
        state.pending_input_action,
        Some(crate::tui::commands::InputAction::SelectModel(
            state.model_list[99].clone()
        ))
    );
}

#[test]
fn no_matching_models_keeps_picker_open_for_filter_or_add() {
    let mut state = state_with_input("", 0);
    state.model_picker_open = true;
    state.model_filter = "missing".into();
    state.model_list = vec![crate::app::ModelInfo {
        id: "available".into(),
        provider: "provider".into(),
        display_name: "available".into(),
    }];

    press(&mut state, KeyCode::Enter);

    assert!(state.model_picker_open);
    assert!(state.pending_input_action.is_none());
    assert_eq!(state.status_message.as_deref(), Some("No matching models"));
}

#[test]
fn model_picker_requests_first_and_explicit_catalog_refreshes() {
    let mut state = state_with_input("", 0);
    press_ctrl(&mut state, 'l');
    assert!(state.model_picker_open);
    assert!(state.model_catalog_refresh_requested);

    state.model_catalog_refresh_requested = false;
    state.model_catalog_loaded = true;
    state.model_picker_open = false;
    press_ctrl(&mut state, 'l');
    assert!(!state.model_catalog_refresh_requested);

    press_with_modifiers(&mut state, KeyCode::Char('R'), KeyModifiers::SHIFT);
    assert!(state.model_catalog_refresh_requested);
    assert!(state.model_catalog_refresh_provider.is_none());
}

#[test]
fn first_model_picker_open_queues_full_refresh_while_provider_catalog_is_refreshing() {
    for target in [
        crate::app::ModelPickerTarget::Current,
        crate::app::ModelPickerTarget::Review,
        crate::app::ModelPickerTarget::Delegation,
        crate::app::ModelPickerTarget::PlanWorkflow,
    ] {
        let mut state = state_with_input("", 0);
        state.model_catalog_refreshing = true;
        state.model_catalog_refresh_provider = Some("mimo".into());
        match target {
            crate::app::ModelPickerTarget::Current => {
                crate::tui::input::open_model_picker(&mut state);
            }
            crate::app::ModelPickerTarget::Review => {
                crate::tui::input::open_review_model_picker(&mut state, None);
            }
            crate::app::ModelPickerTarget::Delegation => {
                crate::tui::input::open_delegation_model_picker(&mut state, None, &[]);
            }
            crate::app::ModelPickerTarget::PlanWorkflow => {
                crate::plan::open_workflow_picker(
                    &mut state,
                    crate::app::PlanWorkflowPickerAction::Send {
                        role: crate::plan_workflow::WorkflowRole::Review,
                        instructions: String::new(),
                    },
                );
            }
            crate::app::ModelPickerTarget::MultiRun(_) => unreachable!(),
        }
        assert!(state.model_catalog_refresh_requested, "{target:?}");
        assert!(state.model_catalog_refresh_provider.is_none(), "{target:?}");
        assert!(state.model_catalog_refreshing);
    }
}

#[test]
fn model_picker_refresh_accepts_legacy_uppercase_and_preserves_filter() {
    let mut state = state_with_input("", 0);
    state.model_picker_open = true;
    state.model_filter = "sol".into();
    press(&mut state, KeyCode::Char('R'));
    assert!(state.model_catalog_refresh_requested);
    assert_eq!(state.model_filter, "sol");

    state.model_catalog_refresh_requested = false;
    press_ctrl(&mut state, 'r');
    assert!(!state.model_catalog_refresh_requested);
    state.model_catalog_refreshing = true;
    press(&mut state, KeyCode::Char('R'));
    assert!(!state.model_catalog_refresh_requested);
}

#[test]
fn model_picker_enter_selects_the_displayed_clamped_row() {
    let mut state = state_with_input("", 0);
    state.model_picker_open = true;
    state.model_list = vec![crate::app::ModelInfo {
        id: "gpt-6.1-sol".into(),
        provider: "openai-codex".into(),
        display_name: "gpt-6.1-sol".into(),
    }];
    state.selected_model_idx = 50;
    press(&mut state, KeyCode::Enter);
    assert_eq!(
        state.pending_input_action,
        Some(crate::tui::commands::InputAction::SelectModel(
            state.model_list[0].clone()
        ))
    );
    assert!(!state.model_picker_open);
}

#[test]
fn live_catalog_merges_with_configured_models_and_sorts_current_provider_first() {
    let providers = std::collections::HashMap::from([
        (
            "other".to_string(),
            crate::config::ProviderConfig {
                google: None,
                tool_schema_policy: Default::default(),
                base_url: "https://other.invalid/v1".into(),
                api_key: String::new(),
                protocol: crate::agent::ProviderProtocol::default(),
                prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
                models: vec!["configured-other".into()],
                timeout: crate::config::ProviderTimeoutConfig::default(),
            },
        ),
        (
            "mimo".to_string(),
            crate::config::ProviderConfig {
                google: None,
                tool_schema_policy: Default::default(),
                base_url: "https://mimo.invalid/v1".into(),
                api_key: String::new(),
                protocol: crate::agent::ProviderProtocol::default(),
                prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
                models: vec!["mimo-v2.5".into()],
                timeout: crate::config::ProviderTimeoutConfig::default(),
            },
        ),
    ]);
    let discovered = std::collections::HashMap::from([(
        "mimo".to_string(),
        vec!["mimo-v2.5-pro".into(), "mimo-v2.5".into()],
    )]);

    let models =
        crate::tui::input::build_model_list_with_catalog(&providers, &discovered, Some("mimo"));

    assert_eq!(
        models
            .iter()
            .map(|model| (model.provider.as_str(), model.id.as_str()))
            .collect::<Vec<_>>(),
        [
            ("mimo", "mimo-v2.5"),
            ("mimo", "mimo-v2.5-pro"),
            ("other", "configured-other"),
        ]
    );
    assert_eq!(models[1].display_name, "mimo-v2.5-pro");
}

fn provider_entry(id: &str, configured_models: &[&str]) -> crate::config::ProviderManagementEntry {
    crate::config::ProviderManagementEntry {
        id: id.into(),
        base_url: format!("https://{id}.invalid/v1"),
        api_key_source: crate::config::ProviderApiKeySource::Environment(format!(
            "{}_KEY",
            id.to_ascii_uppercase()
        )),
        configured_models: configured_models
            .iter()
            .map(|model| (*model).into())
            .collect(),
        configured: true,
        connected: true,
    }
}

fn disconnected_provider_entry(id: &str) -> crate::config::ProviderManagementEntry {
    crate::config::ProviderManagementEntry {
        id: id.into(),
        base_url: format!("https://{id}.invalid/v1"),
        api_key_source: crate::config::ProviderApiKeySource::None,
        configured_models: Vec::new(),
        configured: false,
        connected: false,
    }
}

#[test]
fn provider_picker_starts_oauth_for_disconnected_first_class_provider() {
    let mut state = state_with_input("composer stays", "composer stays".len());
    crate::tui::input::open_provider_manager(
        &mut state,
        vec![disconnected_provider_entry("mimo")],
        String::new(),
    );

    press(&mut state, KeyCode::Enter);

    let auth = state.provider_auth.as_ref().unwrap();
    assert_eq!(
        auth.provider,
        crate::provider_connect::FirstClassProvider::Mimo
    );
    assert_eq!(auth.stage, crate::app::ProviderAuthStage::MethodPicker);
    assert_eq!(state.pending_input_action.take(), None);
    press(&mut state, KeyCode::Enter);
    assert_eq!(
        state.provider_auth.as_ref().unwrap().stage,
        crate::app::ProviderAuthStage::Starting
    );
    assert_eq!(
        state.pending_input_action.take(),
        Some(crate::tui::commands::InputAction::StartProviderAuth(
            crate::provider_connect::FirstClassProvider::Mimo
        ))
    );
    assert_eq!(state.input, "composer stays");

    press(&mut state, KeyCode::Esc);
    assert!(state.provider_auth.is_none());
    assert_eq!(
        state.pending_input_action.take(),
        Some(crate::tui::commands::InputAction::CancelProviderAuth)
    );
}

#[test]
fn bare_profile_requires_confirmation_before_starting_oauth() {
    let mut state = state_with_input("", 0);
    state.run_capabilities =
        crate::runtime::RunCapabilities::for_profile(&crate::runtime::ActiveProfile::Bare);
    crate::tui::input::open_provider_manager(
        &mut state,
        vec![disconnected_provider_entry("openai-codex")],
        String::new(),
    );

    press(&mut state, KeyCode::Enter);
    assert!(state.pending_input_action.is_none());
    let auth = state.provider_auth.as_ref().unwrap();
    assert!(auth.bare);
    assert_eq!(auth.stage, crate::app::ProviderAuthStage::Starting);

    press(&mut state, KeyCode::Enter);
    assert_eq!(
        state.pending_input_action.take(),
        Some(crate::tui::commands::InputAction::StartProviderAuth(
            crate::provider_connect::FirstClassProvider::OpenAiCodex
        ))
    );
}

#[test]
fn provider_picker_masks_direct_key_and_submits_without_touching_composer() {
    let mut state = state_with_input("composer stays", "composer stays".len());
    crate::tui::input::open_provider_manager(
        &mut state,
        vec![disconnected_provider_entry("deepseek")],
        String::new(),
    );

    press(&mut state, KeyCode::Enter);
    assert_eq!(
        state.provider_auth.as_ref().unwrap().stage,
        crate::app::ProviderAuthStage::SecretInput
    );
    for character in "private-key".chars() {
        press(&mut state, KeyCode::Char(character));
    }
    assert!(!format!("{:?}", state.provider_auth).contains("private-key"));
    press(&mut state, KeyCode::Enter);

    assert_eq!(
        state.pending_input_action.take(),
        Some(crate::tui::commands::InputAction::SubmitProviderAuth)
    );
    assert_eq!(
        state.provider_auth.as_ref().unwrap().stage,
        crate::app::ProviderAuthStage::Probing
    );
    assert_eq!(state.input, "composer stays");
}

#[test]
fn provider_auth_copies_full_url_and_routes_paste_into_masked_input() {
    let mut state = state_with_input("composer stays", "composer stays".len());
    state.provider_auth = Some(crate::app::ProviderAuthState {
        provider: crate::provider_connect::FirstClassProvider::Mimo,
        stage: crate::app::ProviderAuthStage::SecretInput,
        url: Some(
            "https://platform.xiaomimimo.com/authorize?pk=full-value&redirect_uri=callback".into(),
        ),
        code: None,
        prompt: "Open the URL and paste the encrypted result".into(),
        input: crate::provider_secrets::SecretValue::default(),
        cursor: 0,
        metadata_input: String::new(),
        metadata_cursor: 0,
        selected_index: 0,
        mimo_method: None,
        saved_mimo_region: None,
        mimo_region: None,
        mimo_tier: None,
        mimo_term: None,
        billing: None,
        error: None,
        bare: false,
        generation: 9,
    });

    press_with_modifiers(&mut state, KeyCode::Char('y'), KeyModifiers::CONTROL);
    assert_eq!(
        state.pending_input_action.take(),
        Some(crate::tui::commands::InputAction::CopyProviderAuthUrl(
            "https://platform.xiaomimimo.com/authorize?pk=full-value&redirect_uri=callback".into()
        ))
    );

    crate::tui::input::handle_event(
        &mut state,
        &Event::Paste("encrypted-result\n".into()),
        "/workspace/ca",
    );
    let auth = state.provider_auth.as_ref().unwrap();
    assert_eq!(
        auth.input.with_secret(|bytes| bytes.to_vec()),
        b"encrypted-result"
    );
    assert_eq!(state.input, "composer stays");
    assert!(!format!("{auth:?}").contains("encrypted-result"));
}

#[test]
fn connected_first_class_provider_opens_models_and_shift_l_reconnects() {
    let mut state = state_with_input("", 0);
    crate::tui::input::open_provider_manager(
        &mut state,
        vec![provider_entry("openai-codex", &["gpt-5.6-sol"])],
        String::new(),
    );

    press(&mut state, KeyCode::Enter);
    assert_eq!(
        state.provider_manager.as_ref().unwrap().view,
        crate::app::ProviderManagerView::Models {
            provider: "openai-codex".into()
        }
    );

    press(&mut state, KeyCode::Esc);
    press_with_modifiers(&mut state, KeyCode::Char('L'), KeyModifiers::SHIFT);
    assert_eq!(
        state.provider_auth.as_ref().unwrap().provider,
        crate::provider_connect::FirstClassProvider::OpenAiCodex
    );
    assert_eq!(
        state.pending_input_action.take(),
        Some(crate::tui::commands::InputAction::StartProviderAuth(
            crate::provider_connect::FirstClassProvider::OpenAiCodex
        ))
    );
}

#[test]
fn provider_manager_first_visit_refreshes_the_selected_provider_after_other_catalog_loaded() {
    let mut state = state_with_input("", 0);
    state.model_catalog_loaded = true;
    state
        .model_catalog_provider_status
        .insert("mimo".into(), "live · 1 model".into());
    crate::tui::input::open_provider_manager(
        &mut state,
        vec![provider_entry("openai-codex", &["gpt-5.6-sol"])],
        String::new(),
    );

    press(&mut state, KeyCode::Enter);
    assert!(state.model_catalog_refresh_requested);
    assert_eq!(
        state.model_catalog_refresh_provider.as_deref(),
        Some("openai-codex")
    );

    state.model_catalog_refresh_requested = false;
    state
        .model_catalog_provider_status
        .insert("openai-codex".into(), "live · 1 model".into());
    press(&mut state, KeyCode::Esc);
    press(&mut state, KeyCode::Enter);
    assert!(!state.model_catalog_refresh_requested);
}

#[test]
fn provider_manager_first_visit_queues_refresh_while_another_provider_is_refreshing() {
    let mut state = state_with_input("", 0);
    state.model_catalog_refreshing = true;
    state
        .model_catalog_provider_status
        .insert("mimo".into(), "refreshing".into());
    crate::tui::input::open_provider_manager(
        &mut state,
        vec![provider_entry("openai-codex", &["gpt-5.6-sol"])],
        String::new(),
    );

    press(&mut state, KeyCode::Enter);
    assert!(state.model_catalog_refresh_requested);
    assert_eq!(
        state.model_catalog_refresh_provider.as_deref(),
        Some("openai-codex")
    );
    assert!(state.model_catalog_refreshing);
}

#[test]
fn provider_manager_enter_selects_the_displayed_clamped_row() {
    let mut state = state_with_input("", 0);
    state.model_list = vec![crate::app::ModelInfo {
        id: "gpt-6.1-sol".into(),
        provider: "openai-codex".into(),
        display_name: "gpt-6.1-sol".into(),
    }];
    crate::tui::input::open_provider_manager(
        &mut state,
        vec![provider_entry("openai-codex", &["gpt-6.1-sol"])],
        String::new(),
    );
    press(&mut state, KeyCode::Enter);
    state.provider_manager.as_mut().unwrap().selected_model_idx = 50;
    press(&mut state, KeyCode::Enter);
    assert_eq!(
        state.pending_input_action,
        Some(crate::tui::commands::InputAction::SelectModel(
            state.model_list[0].clone()
        ))
    );
    assert!(state.provider_manager.is_none());
}

#[test]
fn provider_manager_refresh_accepts_legacy_uppercase_and_preserves_filter() {
    let mut state = state_with_input("", 0);
    crate::tui::input::open_provider_manager(
        &mut state,
        vec![provider_entry("openai-codex", &["gpt-6.1-sol"])],
        "codex".into(),
    );
    press(&mut state, KeyCode::Char('R'));
    assert!(state.model_catalog_refresh_requested);
    assert_eq!(
        state.model_catalog_refresh_provider.as_deref(),
        Some("openai-codex")
    );
    assert_eq!(
        state.provider_manager.as_ref().unwrap().provider_filter,
        "codex"
    );

    state.model_catalog_refresh_requested = false;
    state
        .model_catalog_provider_status
        .insert("openai-codex".into(), "live · 1 model".into());
    press(&mut state, KeyCode::Enter);
    state.provider_manager.as_mut().unwrap().model_filter = "sol".into();
    press(&mut state, KeyCode::Char('R'));
    assert!(state.model_catalog_refresh_requested);
    assert_eq!(state.provider_manager.as_ref().unwrap().model_filter, "sol");

    state.model_catalog_refresh_requested = false;
    state.model_catalog_refreshing = true;
    press(&mut state, KeyCode::Char('R'));
    assert!(!state.model_catalog_refresh_requested);

    state.model_catalog_refreshing = false;
    state.provider_manager.as_mut().unwrap().view =
        crate::app::ProviderManagerView::SetupProvider {
            provider: "openai-codex".into(),
        };
    press(&mut state, KeyCode::Char('R'));
    assert!(state.model_catalog_refresh_requested);
    assert_eq!(
        state.model_catalog_refresh_provider.as_deref(),
        Some("openai-codex")
    );
}

#[test]
fn provider_manager_orders_current_first_filters_and_browses_zero_model_provider() {
    let mut state = state_with_input("composer stays", "composer stays".len());
    state.current_model = Some(crate::app::ModelInfo {
        id: "current".into(),
        provider: "zeta".into(),
        display_name: "current".into(),
    });
    crate::tui::input::open_provider_manager(
        &mut state,
        vec![
            provider_entry("zeta", &["current"]),
            provider_entry("empty", &[]),
            provider_entry("alpha", &["one"]),
        ],
        String::new(),
    );

    let manager = state.provider_manager.as_ref().unwrap();
    assert_eq!(
        manager
            .providers
            .iter()
            .map(|provider| provider.id.as_str())
            .collect::<Vec<_>>(),
        ["zeta", "alpha", "empty"]
    );
    press(&mut state, KeyCode::End);
    assert_eq!(
        state
            .provider_manager
            .as_ref()
            .unwrap()
            .selected_provider_idx,
        2
    );
    press(&mut state, KeyCode::Enter);
    assert_eq!(
        state.provider_manager.as_ref().unwrap().view,
        crate::app::ProviderManagerView::Models {
            provider: "empty".into()
        }
    );

    press_with_modifiers(&mut state, KeyCode::Char('A'), KeyModifiers::SHIFT);
    assert_eq!(
        state.pending_input_action.take(),
        Some(crate::tui::commands::InputAction::OpenProviderEditor(
            crate::app::ProviderEditorRequest::AddModel(crate::app::ModelInfo {
                id: String::new(),
                provider: "empty".into(),
                display_name: String::new(),
            })
        ))
    );
    assert_eq!(state.input, "composer stays");

    press(&mut state, KeyCode::Esc);
    press(&mut state, KeyCode::Char('a'));
    let manager = state.provider_manager.as_ref().unwrap();
    assert_eq!(manager.provider_filter, "a");
    assert_eq!(manager.selected_provider_idx, 0);
    assert_eq!(state.input, "composer stays");
}

#[test]
fn provider_manager_detail_selects_models_and_routes_edit_settings_refresh_and_delete() {
    let mut state = state_with_input("", 0);
    state.model_catalog_loaded = true;
    state.model_list = vec![
        crate::app::ModelInfo {
            id: "configured".into(),
            provider: "mimo".into(),
            display_name: "configured".into(),
        },
        crate::app::ModelInfo {
            id: "live-only".into(),
            provider: "mimo".into(),
            display_name: "live-only".into(),
        },
    ];
    crate::tui::input::open_provider_manager(
        &mut state,
        vec![provider_entry("mimo", &["configured"])],
        String::new(),
    );
    press(&mut state, KeyCode::Enter);
    press(&mut state, KeyCode::Down);
    press_with_modifiers(&mut state, KeyCode::Char('E'), KeyModifiers::SHIFT);
    assert_eq!(
        state.pending_input_action.take(),
        Some(crate::tui::commands::InputAction::OpenProviderEditor(
            crate::app::ProviderEditorRequest::EditModel(state.model_list[1].clone())
        ))
    );

    press_with_modifiers(&mut state, KeyCode::Char('C'), KeyModifiers::SHIFT);
    assert_eq!(
        state.pending_input_action.take(),
        Some(crate::tui::commands::InputAction::Command(
            crate::tui::commands::CommandAction {
                id: crate::tui::commands::CommandId::PromptCache,
                args: "mimo/live-only".into(),
            }
        ))
    );

    press_with_modifiers(&mut state, KeyCode::Char('S'), KeyModifiers::SHIFT);
    assert_eq!(
        state.pending_input_action.take(),
        Some(crate::tui::commands::InputAction::OpenProviderEditor(
            crate::app::ProviderEditorRequest::EditProvider("mimo".into())
        ))
    );
    press_with_modifiers(&mut state, KeyCode::Char('R'), KeyModifiers::SHIFT);
    assert!(state.model_catalog_refresh_requested);
    assert_eq!(
        state.model_catalog_refresh_provider.as_deref(),
        Some("mimo")
    );
    press_with_modifiers(&mut state, KeyCode::Char('D'), KeyModifiers::SHIFT);
    assert_eq!(
        state.pending_input_action.take(),
        Some(crate::tui::commands::InputAction::RequestDeleteProvider(
            "mimo".into()
        ))
    );

    press(&mut state, KeyCode::Enter);
    assert!(state.provider_manager.is_none());
    assert_eq!(
        state.pending_input_action,
        Some(crate::tui::commands::InputAction::SelectModel(
            state.model_list[1].clone()
        ))
    );
}

#[test]
fn provider_manager_paste_isolated_editor_returns_and_delete_confirmation_defaults_to_cancel() {
    let mut state = state_with_input("composer stays", "composer stays".len());
    crate::tui::input::open_provider_manager(
        &mut state,
        vec![provider_entry("mimo", &["configured"])],
        String::new(),
    );
    assert!(handle_event(
        &mut state,
        &Event::Paste("mimo\n".into()),
        "/tmp/ca-cfg033-test",
    ));
    assert_eq!(
        state.provider_manager.as_ref().unwrap().provider_filter,
        "mimo"
    );
    assert_eq!(state.input, "composer stays");

    press_with_modifiers(&mut state, KeyCode::Char('P'), KeyModifiers::SHIFT);
    state.provider_manager.as_mut().unwrap().error = Some("stale template error".into());
    assert!(handle_event(
        &mut state,
        &Event::Paste("OPENAI_API_KEY\n".into()),
        "/tmp/ca-cfg093-test",
    ));
    assert_eq!(
        state.provider_manager.as_ref().unwrap().template_filter,
        "OPENAI_API_KEY"
    );
    assert!(state.provider_manager.as_ref().unwrap().error.is_none());
    assert_eq!(state.input, "composer stays");
    press(&mut state, KeyCode::Esc);

    state.provider_editor = Some(crate::app::ProviderEditorState {
        origin: crate::app::ProviderEditorOrigin::ProviderManager,
        mode: crate::app::ProviderEditorMode::EditProvider,
        draft: crate::app::ProviderModelDraft {
            google: None,
            original_model: None,
            provider: "mimo".into(),
            base_url: "https://mimo.invalid/v1".into(),
            api_key_env: "MIMO_KEY".into(),
            model: String::new(),
        },
        api_key: crate::provider_secrets::SecretValue::default(),
        credential_configured: true,
        credential_in_keychain: false,
        protocol_override: None,
        prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
        field: crate::app::ProviderEditorField::BaseUrl,
        cursor: 0,
        error: None,
        saving: false,
    });
    press(&mut state, KeyCode::Esc);
    assert!(state.provider_editor.is_none());
    assert!(state.provider_manager.is_some());
    assert!(!state.model_picker_open);

    state.provider_manager.as_mut().unwrap().delete_confirmation =
        Some(crate::app::ProviderDeleteConfirmation {
            provider: "mimo".into(),
            configured_model_count: 1,
            removes_keychain: true,
            replacement: Some(crate::app::ModelInfo {
                id: "backup".into(),
                provider: "other".into(),
                display_name: "backup".into(),
            }),
            confirm_selected: false,
        });
    press(&mut state, KeyCode::Enter);
    assert!(state
        .provider_manager
        .as_ref()
        .unwrap()
        .delete_confirmation
        .is_none());
    assert!(state.pending_input_action.is_none());

    state.provider_manager.as_mut().unwrap().busy = true;
    press(&mut state, KeyCode::Esc);
    assert!(state.provider_manager.is_some());
}

#[test]
fn provider_setup_routes_explicit_checks_and_cancellation_without_touching_composer() {
    let mut state = state_with_input("composer stays", "composer stays".len());
    state.current_model = Some(crate::app::ModelInfo {
        id: "mimo-v2.5".into(),
        provider: "mimo".into(),
        display_name: "mimo-v2.5".into(),
    });
    crate::tui::input::open_provider_setup(
        &mut state,
        vec![
            provider_entry("other", &["other-model"]),
            provider_entry("mimo", &["mimo-v2.5"]),
        ],
        String::new(),
    );
    assert_eq!(
        state.provider_manager.as_ref().unwrap().view,
        crate::app::ProviderManagerView::SetupProviders
    );
    assert_eq!(
        state.provider_manager.as_ref().unwrap().providers[0].id,
        "mimo"
    );

    press_with_modifiers(&mut state, KeyCode::Char('T'), KeyModifiers::SHIFT);
    assert_eq!(
        state.pending_input_action.take(),
        Some(crate::tui::commands::InputAction::CheckProvider(
            "mimo".into()
        ))
    );
    state.provider_check_running = Some("mimo".into());
    state.provider_checks.insert(
        "mimo".into(),
        crate::provider_probe::ProviderCheckStatus::Checking,
    );
    press(&mut state, KeyCode::Esc);
    assert_eq!(
        state.pending_input_action.take(),
        Some(crate::tui::commands::InputAction::CancelProviderCheck)
    );
    assert!(state.provider_manager.is_some());
    assert_eq!(state.input, "composer stays");
}

fn discovered_candidate(
    port: u16,
    models: &[&str],
) -> crate::provider_discovery::LocalProviderCandidate {
    crate::provider_discovery::LocalProviderCandidate {
        provider_id: format!("local-{port}"),
        port,
        base_url: format!("http://127.0.0.1:{port}/v1"),
        models: models.iter().map(|model| (*model).to_string()).collect(),
    }
}

#[test]
fn provider_discovery_is_paste_isolated_navigable_and_prefills_add_action() {
    let mut state = state_with_input("composer stays", "composer stays".len());
    crate::tui::input::open_provider_discovery(&mut state);
    assert!(state.provider_discovery_requested);
    assert!(handle_event(
        &mut state,
        &Event::Paste("must not leak".into()),
        "/tmp/ca-prv045-test",
    ));
    assert_eq!(state.input, "composer stays");

    let discovery = state.provider_discovery.as_mut().unwrap();
    discovery.scanning = false;
    discovery.candidates = vec![
        discovered_candidate(8000, &["first"]),
        discovered_candidate(11434, &["llama", "coder"]),
    ];
    press(&mut state, KeyCode::End);
    assert_eq!(state.provider_discovery.as_ref().unwrap().selected_idx, 1);
    press(&mut state, KeyCode::Enter);
    assert_eq!(
        state.pending_input_action.take(),
        Some(crate::tui::commands::InputAction::OpenProviderEditor(
            crate::app::ProviderEditorRequest::AddDiscoveredProvider(discovered_candidate(
                11434,
                &["llama", "coder"],
            ))
        ))
    );
}

#[test]
fn provider_discovery_rescan_and_close_invalidate_previous_work() {
    let mut state = state_with_input("", 0);
    crate::tui::input::open_provider_discovery(&mut state);
    state.provider_discovery_requested = false;
    state.provider_discovery.as_mut().unwrap().scanning = false;
    state.provider_discovery.as_mut().unwrap().candidates =
        vec![discovered_candidate(1234, &["old"])];

    press_with_modifiers(&mut state, KeyCode::Char('R'), KeyModifiers::SHIFT);
    let discovery = state.provider_discovery.as_ref().unwrap();
    assert!(discovery.scanning);
    assert!(discovery.candidates.is_empty());
    assert!(state.provider_discovery_requested);

    let generation = state.provider_discovery_generation;
    press(&mut state, KeyCode::Esc);
    assert!(state.provider_discovery.is_none());
    assert!(!state.provider_discovery_requested);
    assert_eq!(
        state.provider_discovery_generation,
        generation.wrapping_add(1)
    );
}

#[test]
fn provider_setup_details_and_finish_only_offer_working_providers() {
    let mut state = state_with_input("", 0);
    crate::tui::input::open_provider_setup(
        &mut state,
        vec![
            provider_entry("mimo", &["mimo-v2.5-pro"]),
            provider_entry("offline", &["offline-model"]),
        ],
        String::new(),
    );

    press_with_modifiers(&mut state, KeyCode::Char('F'), KeyModifiers::SHIFT);
    assert!(state
        .provider_manager
        .as_ref()
        .unwrap()
        .error
        .as_deref()
        .unwrap()
        .contains("Test at least one"));

    state.provider_checks.insert(
        "mimo".into(),
        crate::provider_probe::ProviderCheckStatus::Working {
            model: "mimo-v2.5-pro".into(),
        },
    );
    press_with_modifiers(&mut state, KeyCode::Char('F'), KeyModifiers::SHIFT);
    assert_eq!(
        state.provider_manager.as_ref().unwrap().view,
        crate::app::ProviderManagerView::SetupFinish
    );
    press(&mut state, KeyCode::Enter);
    assert_eq!(
        state.pending_input_action.take(),
        Some(crate::tui::commands::InputAction::SelectCheckedProvider(
            crate::app::ModelInfo {
                id: "mimo-v2.5-pro".into(),
                provider: "mimo".into(),
                display_name: "mimo-v2.5-pro".into(),
            }
        ))
    );
    assert!(state.provider_manager.as_ref().unwrap().busy);

    state.provider_manager.as_mut().unwrap().busy = false;
    state.provider_manager.as_mut().unwrap().view = crate::app::ProviderManagerView::SetupProviders;
    press(&mut state, KeyCode::Enter);
    assert_eq!(
        state.provider_manager.as_ref().unwrap().view,
        crate::app::ProviderManagerView::SetupProvider {
            provider: "mimo".into()
        }
    );
    press_with_modifiers(&mut state, KeyCode::Char('E'), KeyModifiers::SHIFT);
    assert_eq!(
        state.pending_input_action.take(),
        Some(crate::tui::commands::InputAction::OpenProviderEditor(
            crate::app::ProviderEditorRequest::EditProvider("mimo".into())
        ))
    );
}

#[test]
fn provider_lists_offer_an_openai_template_without_replacing_custom_add() {
    let mut state = state_with_input("composer stays", "composer stays".len());
    crate::tui::input::open_provider_manager(
        &mut state,
        vec![provider_entry("mimo", &["mimo-v2.5-pro"])],
        String::new(),
    );

    press_with_modifiers(&mut state, KeyCode::Char('O'), KeyModifiers::SHIFT);
    assert_eq!(
        state.pending_input_action.take(),
        Some(crate::tui::commands::InputAction::OpenProviderEditor(
            crate::app::ProviderEditorRequest::AddOpenAiProvider
        ))
    );
    assert_eq!(state.input, "composer stays");

    press_with_modifiers(&mut state, KeyCode::Char('A'), KeyModifiers::SHIFT);
    assert_eq!(
        state.pending_input_action.take(),
        Some(crate::tui::commands::InputAction::OpenProviderEditor(
            crate::app::ProviderEditorRequest::AddProvider
        ))
    );

    state.provider_manager.as_mut().unwrap().view = crate::app::ProviderManagerView::SetupProviders;
    press_with_modifiers(&mut state, KeyCode::Char('O'), KeyModifiers::SHIFT);
    assert_eq!(
        state.pending_input_action.take(),
        Some(crate::tui::commands::InputAction::OpenProviderEditor(
            crate::app::ProviderEditorRequest::AddOpenAiProvider
        ))
    );
    assert_eq!(state.input, "composer stays");
}

#[test]
fn provider_template_catalog_filters_routes_ready_rows_and_preserves_return_target() {
    let mut state = state_with_input("composer stays", "composer stays".len());
    crate::tui::input::open_provider_manager(
        &mut state,
        vec![provider_entry("mimo", &["mimo-v2.5-pro"])],
        String::new(),
    );

    press_with_modifiers(&mut state, KeyCode::Char('P'), KeyModifiers::SHIFT);
    assert_eq!(
        state.provider_manager.as_ref().unwrap().view,
        crate::app::ProviderManagerView::Templates {
            return_to: crate::app::ProviderTemplateReturn::Providers,
        }
    );
    press(&mut state, KeyCode::End);
    assert_eq!(
        state
            .provider_manager
            .as_ref()
            .unwrap()
            .selected_template_idx,
        crate::provider_templates::ALL.len() - 1
    );
    press(&mut state, KeyCode::PageUp);
    assert_eq!(
        state
            .provider_manager
            .as_ref()
            .unwrap()
            .selected_template_idx,
        crate::provider_templates::ALL.len() - 9
    );
    press(&mut state, KeyCode::Home);
    for character in "anthropic".chars() {
        press(&mut state, KeyCode::Char(character));
    }
    press(&mut state, KeyCode::Enter);
    assert_eq!(
        state.pending_input_action.take(),
        Some(crate::tui::commands::InputAction::OpenProviderEditor(
            crate::app::ProviderEditorRequest::AddProviderTemplate("anthropic".into())
        ))
    );
    assert_eq!(state.input, "composer stays");

    let manager = state.provider_manager.as_mut().unwrap();
    manager.view = crate::app::ProviderManagerView::SetupProviders;
    manager.template_filter.clear();
    manager.selected_template_idx = 0;
    press_with_modifiers(&mut state, KeyCode::Char('P'), KeyModifiers::SHIFT);
    assert_eq!(
        state.provider_manager.as_ref().unwrap().view,
        crate::app::ProviderManagerView::Templates {
            return_to: crate::app::ProviderTemplateReturn::SetupProviders,
        }
    );
    press(&mut state, KeyCode::Esc);
    assert_eq!(
        state.provider_manager.as_ref().unwrap().view,
        crate::app::ProviderManagerView::SetupProviders
    );
    assert_eq!(state.input, "composer stays");
}

#[test]
fn provider_template_catalog_keeps_integration_rows_inert_and_refuses_duplicates() {
    let mut state = state_with_input("", 0);
    crate::tui::input::open_provider_manager(
        &mut state,
        vec![provider_entry("openai", &["gpt-5.6-sol"])],
        String::new(),
    );
    press_with_modifiers(&mut state, KeyCode::Char('P'), KeyModifiers::SHIFT);

    for character in "openai codex".chars() {
        press(&mut state, KeyCode::Char(character));
    }
    press(&mut state, KeyCode::Enter);
    // openai-codex is now Ready (OAuth resolved via /connect), so selecting it opens the editor.
    assert!(state.pending_input_action.is_some());
    state.pending_input_action = None;

    let manager = state.provider_manager.as_mut().unwrap();
    manager.template_filter = "OPENAI_API_KEY".into();
    manager.selected_template_idx = 0;
    manager.error = None;
    assert_eq!(
        crate::tui::input::filtered_provider_templates(manager)
            .iter()
            .map(|template| template.id)
            .collect::<Vec<_>>(),
        ["openai-api"]
    );
    press(&mut state, KeyCode::Enter);
    assert!(state.pending_input_action.is_none());
    let error = state.provider_manager.as_ref().unwrap().error.as_deref();
    assert!(
        error.is_some_and(|error| error.contains("no changes")),
        "unexpected template error: {error:?}"
    );

    let manager = state.provider_manager.as_mut().unwrap();
    manager.template_filter = "XIAOMI_API_KEY".into();
    manager.selected_template_idx = 0;
    manager.error = None;
    manager
        .providers
        .push(provider_entry("mimo", &["mimo-v2.5-pro"]));
    press(&mut state, KeyCode::Enter);
    assert!(state.pending_input_action.is_none());
    assert!(state
        .provider_manager
        .as_ref()
        .unwrap()
        .error
        .as_deref()
        .is_some_and(|error| error.contains("mimo exists")));
}

#[test]
fn provider_editor_navigates_edits_saves_and_cancels_without_touching_composer() {
    let mut state = state_with_input("composer stays", "composer stays".len());
    state.provider_editor = Some(crate::app::ProviderEditorState {
        origin: crate::app::ProviderEditorOrigin::ModelPicker,
        mode: crate::app::ProviderEditorMode::AddProvider,
        draft: crate::app::ProviderModelDraft {
            google: None,
            original_model: None,
            provider: String::new(),
            base_url: String::new(),
            api_key_env: String::new(),
            model: String::new(),
        },
        api_key: crate::provider_secrets::SecretValue::default(),
        credential_configured: false,
        credential_in_keychain: false,
        protocol_override: None,
        prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
        field: crate::app::ProviderEditorField::Provider,
        cursor: 0,
        error: Some("old error".into()),
        saving: false,
    });

    for character in "local".chars() {
        press(&mut state, KeyCode::Char(character));
    }
    press(&mut state, KeyCode::Tab);
    for character in "http://127.0.0.1:11434/v1".chars() {
        press(&mut state, KeyCode::Char(character));
    }
    press(&mut state, KeyCode::Tab);
    press(&mut state, KeyCode::Tab);
    press(&mut state, KeyCode::Tab);
    for character in "coder-v1".chars() {
        press(&mut state, KeyCode::Char(character));
    }
    press(&mut state, KeyCode::Enter);

    let draft = state.provider_editor.as_ref().unwrap().draft.clone();
    assert_eq!(
        state.pending_input_action,
        Some(crate::tui::commands::InputAction::SaveProviderModel(
            draft.clone()
        ))
    );
    assert_eq!(draft.provider, "local");
    assert_eq!(draft.model, "coder-v1");
    assert_eq!(state.input, "composer stays");
    assert!(state.provider_editor.as_ref().unwrap().error.is_none());

    state.pending_input_action = None;
    press(&mut state, KeyCode::Esc);
    assert!(state.provider_editor.is_none());
    assert!(state.model_picker_open);
    assert_eq!(state.input, "composer stays");
}

#[test]
fn provider_editor_supports_cursor_insertion_deletion_and_unicode_boundaries() {
    let mut state = state_with_input("composer stays", "composer stays".len());
    state.provider_editor = Some(crate::app::ProviderEditorState {
        origin: crate::app::ProviderEditorOrigin::ModelPicker,
        mode: crate::app::ProviderEditorMode::EditModel,
        draft: crate::app::ProviderModelDraft {
            google: None,
            original_model: Some("模型".into()),
            provider: "local".into(),
            base_url: "https://example.invalid/v1".into(),
            api_key_env: "LOCAL_API_KEY".into(),
            model: "模型".into(),
        },
        api_key: crate::provider_secrets::SecretValue::default(),
        credential_configured: true,
        credential_in_keychain: false,
        protocol_override: None,
        prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
        field: crate::app::ProviderEditorField::BaseUrl,
        cursor: "https://example.invalid/v1".len(),
        error: None,
        saving: false,
    });

    press(&mut state, KeyCode::Home);
    press(&mut state, KeyCode::Right);
    press(&mut state, KeyCode::Char('X'));
    assert_eq!(
        state.provider_editor.as_ref().unwrap().draft.base_url,
        "hXttps://example.invalid/v1"
    );
    press(&mut state, KeyCode::End);
    press(&mut state, KeyCode::Backspace);
    assert!(state
        .provider_editor
        .as_ref()
        .unwrap()
        .draft
        .base_url
        .ends_with("/v"));

    press(&mut state, KeyCode::Tab);
    assert_eq!(state.provider_editor.as_ref().unwrap().cursor, 0);
    press(&mut state, KeyCode::Tab);
    assert_eq!(
        state.provider_editor.as_ref().unwrap().cursor,
        "LOCAL_API_KEY".len()
    );
    press(&mut state, KeyCode::Tab);
    press(&mut state, KeyCode::Left);
    press(&mut state, KeyCode::Backspace);
    let editor = state.provider_editor.as_ref().unwrap();
    assert_eq!(editor.draft.model, "型");
    assert_eq!(editor.cursor, 0);
    assert!(editor.draft.model.is_char_boundary(editor.cursor));
    assert_eq!(state.input, "composer stays");
}

#[test]
fn provider_editor_paste_targets_the_active_field_not_the_composer() {
    let mut state = state_with_input("composer stays", "composer stays".len());
    state.provider_editor = Some(crate::app::ProviderEditorState {
        origin: crate::app::ProviderEditorOrigin::ModelPicker,
        mode: crate::app::ProviderEditorMode::AddModel,
        draft: crate::app::ProviderModelDraft {
            google: None,
            original_model: None,
            provider: "mimo".into(),
            base_url: "https://mimo.invalid/v1".into(),
            api_key_env: "MIMO_API_KEY".into(),
            model: String::new(),
        },
        api_key: crate::provider_secrets::SecretValue::default(),
        credential_configured: true,
        credential_in_keychain: false,
        protocol_override: None,
        prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
        field: crate::app::ProviderEditorField::Model,
        cursor: 0,
        error: Some("old error".into()),
        saving: false,
    });

    assert!(handle_event(
        &mut state,
        &Event::Paste("mimo-v2.5-pro\n".into()),
        "/tmp/ca-cfg031-test",
    ));

    let editor = state.provider_editor.as_ref().unwrap();
    assert_eq!(editor.draft.model, "mimo-v2.5-pro");
    assert_eq!(editor.cursor, "mimo-v2.5-pro".len());
    assert!(editor.error.is_none());
    assert_eq!(state.input, "composer stays");
}

#[test]
fn provider_editor_masks_direct_key_input_and_switches_away_from_environment_source() {
    let mut state = state_with_input("composer stays", "composer stays".len());
    state.provider_editor = Some(crate::app::ProviderEditorState {
        origin: crate::app::ProviderEditorOrigin::ModelPicker,
        mode: crate::app::ProviderEditorMode::AddModel,
        draft: crate::app::ProviderModelDraft {
            google: None,
            original_model: None,
            provider: "mimo".into(),
            base_url: "https://mimo.invalid/v1".into(),
            api_key_env: "MIMO_API_KEY".into(),
            model: "mimo-v2.5-pro".into(),
        },
        api_key: crate::provider_secrets::SecretValue::default(),
        credential_configured: true,
        credential_in_keychain: false,
        protocol_override: None,
        prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
        field: crate::app::ProviderEditorField::ApiKey,
        cursor: 0,
        error: None,
        saving: false,
    });

    assert!(handle_event(
        &mut state,
        &Event::Paste("not-a-real-key\n".into()),
        "/tmp/ca-cfg032-test",
    ));

    let editor = state.provider_editor.as_ref().unwrap();
    assert!(editor.draft.api_key_env.is_empty());
    assert_eq!(editor.cursor, "not-a-real-key".len());
    assert!(!format!("{editor:?}").contains("not-a-real-key"));
    assert_eq!(state.input, "composer stays");

    press(&mut state, KeyCode::Enter);
    assert!(!format!("{:?}", state.pending_input_action).contains("not-a-real-key"));

    state.pending_input_action = None;
    state.provider_editor.as_mut().unwrap().saving = true;
    press(&mut state, KeyCode::Backspace);
    assert_eq!(
        state.provider_editor.as_ref().unwrap().cursor,
        "not-a-real-key".len()
    );

    let editor = state.provider_editor.as_mut().unwrap();
    editor.saving = false;
    editor.field = crate::app::ProviderEditorField::ApiKeyEnv;
    editor.cursor = 0;
    press(&mut state, KeyCode::Char('N'));
    let editor = state.provider_editor.as_ref().unwrap();
    assert!(editor.api_key.is_empty());
    assert_eq!(editor.draft.api_key_env, "N");
}

#[test]
fn direct_openai_provider_editor_skips_and_preserves_legacy_prompt_cache_mode() {
    let mut state = state_with_input("composer stays", "composer stays".len());
    state.provider_editor = Some(crate::app::ProviderEditorState {
        origin: crate::app::ProviderEditorOrigin::ProviderManager,
        mode: crate::app::ProviderEditorMode::EditProvider,
        draft: crate::app::ProviderModelDraft {
            google: None,
            original_model: None,
            provider: "openai".into(),
            base_url: crate::pricing::OPENAI_API_BASE_URL.into(),
            api_key_env: "OPENAI_API_KEY".into(),
            model: String::new(),
        },
        api_key: crate::provider_secrets::SecretValue::default(),
        credential_configured: true,
        credential_in_keychain: false,
        protocol_override: None,
        prompt_cache_mode: crate::config::PromptCacheMode::StablePrefix,
        field: crate::app::ProviderEditorField::ApiKeyEnv,
        cursor: "OPENAI_API_KEY".len(),
        error: None,
        saving: false,
    });

    press(&mut state, KeyCode::Tab);
    assert_eq!(
        state.provider_editor.as_ref().unwrap().field,
        crate::app::ProviderEditorField::BaseUrl
    );
    press(&mut state, KeyCode::BackTab);
    assert_eq!(
        state.provider_editor.as_ref().unwrap().field,
        crate::app::ProviderEditorField::ApiKeyEnv
    );

    state.provider_editor.as_mut().unwrap().field = crate::app::ProviderEditorField::PromptCache;
    press(&mut state, KeyCode::Right);
    press(&mut state, KeyCode::Char(' '));
    press(&mut state, KeyCode::Left);
    assert_eq!(
        state.provider_editor.as_ref().unwrap().prompt_cache_mode,
        crate::config::PromptCacheMode::StablePrefix
    );
    press(&mut state, KeyCode::Tab);
    assert_eq!(
        state.provider_editor.as_ref().unwrap().field,
        crate::app::ProviderEditorField::BaseUrl
    );
    assert_eq!(state.input, "composer stays");
}

#[test]
fn model_picker_paste_targets_the_filter_not_the_composer() {
    let mut state = state_with_input("composer stays", "composer stays".len());
    state.model_picker_open = true;
    state.selected_model_idx = 4;

    assert!(handle_event(
        &mut state,
        &Event::Paste("mimo-v2.5-pro\n".into()),
        "/tmp/ca-cfg031-test",
    ));

    assert_eq!(state.model_filter, "mimo-v2.5-pro");
    assert_eq!(state.selected_model_idx, 0);
    assert_eq!(state.input, "composer stays");
}

#[test]
fn pasted_absolute_folder_path_stays_in_composer_without_opening_commands() {
    let mut state = state_with_input("", 0);

    assert!(handle_event(
        &mut state,
        &Event::Paste("/Users/example/project/folder ".into()),
        "/Users/example/project",
    ));

    assert_eq!(state.input, "/Users/example/project/folder ");
    assert_eq!(state.input_cursor, state.input.len());
    assert!(!state.slash_palette_open);

    let mut typed = state_with_input("", 0);
    press(&mut typed, KeyCode::Char('/'));
    assert!(typed.slash_palette_open);
    assert_eq!(typed.input, "/");
}

#[test]
fn remote_absolute_path_in_prose_never_becomes_a_slash_command_or_blocks_submit() {
    let input =
        "great, copy the data files for the missing RED, OPS ones into /bigmac/sed-ro/seedbox/";
    let mut state = state_with_input("", 0);

    for character in input.chars() {
        press(&mut state, KeyCode::Char(character));
    }

    assert_eq!(state.input, input);
    assert!(!state.slash_palette_open);
    assert!(state.composer_ui.path_completion.open);
    assert!(state.composer_ui.path_completion.entries.is_empty());

    assert!(!handle_event(
        &mut state,
        &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
        "/tmp/ca-remote-path-test",
    ));
    assert_eq!(state.input, input);
    assert!(!state.slash_palette_open);
    assert!(!state.composer_ui.path_completion.open);
    assert!(!state
        .status_message
        .as_deref()
        .is_some_and(|message| message.starts_with("Unknown command:")));
}

#[test]
fn slash_palette_resolves_aliases_to_typed_actions() {
    let mut state = state_with_input("", 0);
    press(&mut state, KeyCode::Char('/'));
    press(&mut state, KeyCode::Char('m'));
    press(&mut state, KeyCode::Char(' '));
    for character in "local-coder".chars() {
        press(&mut state, KeyCode::Char(character));
    }
    press(&mut state, KeyCode::Enter);

    assert_eq!(
        state.pending_input_action,
        Some(crate::tui::commands::InputAction::Command(
            crate::tui::commands::CommandAction {
                id: crate::tui::commands::CommandId::Model,
                args: "local-coder".into(),
            }
        ))
    );
}

#[test]
fn slash_palette_navigation_selects_the_visible_registry_entry() {
    let mut state = state_with_input("", 0);
    press(&mut state, KeyCode::Char('/'));
    press(&mut state, KeyCode::Down);
    press(&mut state, KeyCode::Enter);

    assert_eq!(
        state
            .pending_input_action
            .as_ref()
            .and_then(|action| match action {
                crate::tui::commands::InputAction::Command(action) => Some(action.id),
                _ => None,
            }),
        Some(crate::tui::commands::CommandId::Resume)
    );
}

#[test]
fn idle_only_palette_commands_are_rejected_while_busy() {
    let mut state = state_with_input("", 0);
    state.streaming = Some(crate::app::StreamingState {
        provider_text: Default::default(),
        usage_handle: crate::usage::UsageHandle::test_handle(),
        content: String::new(),
        tool_calls: Vec::new(),
        thinking: None,
        last_usage: None,
        response_items: Vec::new(),
        reasoning_started_at: None,
        reasoning_duration: None,
        generation_started_at: None,
    });
    state.turn_lifecycle.begin();
    press(&mut state, KeyCode::Char('/'));
    for character in "diff".chars() {
        press(&mut state, KeyCode::Char(character));
    }
    press(&mut state, KeyCode::Enter);

    assert!(state.pending_input_action.is_none());
    assert_eq!(
        state.status_message.as_deref(),
        Some("/diff is unavailable while a turn is running")
    );
}

#[test]
fn goal_status_and_pause_commands_remain_available_while_busy() {
    for arguments in ["goal status", "goal pause"] {
        let mut state = state_with_input("", 0);
        state.turn_lifecycle.begin();
        press(&mut state, KeyCode::Char('/'));
        for character in arguments.chars() {
            press(&mut state, KeyCode::Char(character));
        }
        press(&mut state, KeyCode::Enter);

        assert_eq!(
            state.pending_input_action,
            Some(crate::tui::commands::InputAction::Command(
                crate::tui::commands::CommandAction {
                    id: crate::tui::commands::CommandId::Goal,
                    args: arguments.trim_start_matches("goal ").into(),
                }
            ))
        );
    }
}

#[test]
fn timeline_confirmation_emits_a_non_destructive_branch_action() {
    let mut state = state_with_input("", 0);
    state.timeline_open = true;
    state.timeline_entries = vec![crate::session::TimelineEntry {
        turn_index: 2,
        through_message_count: 5,
        title: "Inspect the session".into(),
        completed_at: chrono::DateTime::from_timestamp(5, 0).unwrap(),
    }];
    press(&mut state, KeyCode::Enter);

    assert!(!state.timeline_open);
    assert_eq!(
        state.pending_input_action,
        Some(crate::tui::commands::InputAction::BranchAt {
            through_message_count: 5,
            branch_turn: 2,
        })
    );
}

#[test]
fn alt_enter_explicitly_queues_the_current_draft() {
    let mut state = state_with_input("follow up", "follow up".len());
    press_with_modifiers(&mut state, KeyCode::Enter, KeyModifiers::ALT);

    assert!(state.input.is_empty());
    assert_eq!(state.input_queue.len(), 1);
    assert_eq!(state.input_queue.selected().unwrap().text, "follow up");
}

#[test]
fn ctrl_s_emits_steering_action_while_turn_is_active() {
    let mut state = state_with_input("change direction", "change direction".len());
    state.turn_lifecycle.begin();
    press_ctrl(&mut state, 's');

    assert_eq!(
        state.pending_input_action,
        Some(crate::tui::commands::InputAction::Steer {
            text: "change direction".into(),
            queued_id: None,
        })
    );
}

#[test]
fn busy_enter_interrupts_instead_of_silently_queueing() {
    let mut state = state_with_input("", 0);
    state.turn_lifecycle.begin();
    state.input = "Stop that and inspect the diff".into();
    state.input_cursor = state.input.len();
    press(&mut state, KeyCode::Enter);
    assert_eq!(
        state.pending_input_action,
        Some(crate::tui::commands::InputAction::Steer {
            text: "Stop that and inspect the diff".into(),
            queued_id: None,
        })
    );
    assert!(state.input_queue.is_empty());
}

#[test]
fn busy_enter_interrupts_leading_path_prose() {
    for input in [
        "/Volumes/projects/qui you can use this source",
        "/tmp/project inspect this directory",
    ] {
        let mut state = state_with_input(input, input.len());
        state.turn_lifecycle.begin();
        state
            .turn_lifecycle
            .set_phase(crate::activity::ActivityPhase::Tool);
        press(&mut state, KeyCode::Enter);
        assert_eq!(
            state.pending_input_action,
            Some(crate::tui::commands::InputAction::Steer {
                text: input.into(),
                queued_id: None,
            })
        );
        assert!(state.input.is_empty());
        assert!(state.input_queue.is_empty());
    }
}

#[test]
fn busy_enter_interrupts_an_unmatched_path_completion() {
    let input = "/missing/project";
    let mut state = state_with_input(input, input.len());
    state.turn_lifecycle.begin();
    state.composer_ui.path_completion.open = true;
    state.composer_ui.path_completion.token = input.into();
    assert!(state.composer_ui.path_completion.entries.is_empty());
    press(&mut state, KeyCode::Enter);
    assert!(!state.composer_ui.path_completion.open);
    assert_eq!(
        state.pending_input_action,
        Some(crate::tui::commands::InputAction::Steer {
            text: input.into(),
            queued_id: None,
        })
    );
}

#[test]
fn busy_enter_preserves_recognized_slash_command_routing() {
    for (input, id, args) in [
        (
            "/goal status",
            crate::tui::commands::CommandId::Goal,
            "status",
        ),
        ("/help", crate::tui::commands::CommandId::Help, ""),
    ] {
        let mut state = state_with_input(input, input.len());
        state.turn_lifecycle.begin();
        press(&mut state, KeyCode::Enter);
        assert_eq!(
            state.pending_input_action,
            Some(crate::tui::commands::InputAction::Command(
                crate::tui::commands::CommandAction {
                    id,
                    args: args.into()
                }
            ))
        );
    }

    let mut state = state_with_input("/local-coder", 12);
    state.turn_lifecycle.begin();
    state.model_list = vec![crate::app::ModelInfo {
        id: "local-coder".into(),
        provider: "local".into(),
        display_name: "Local coder".into(),
    }];
    press(&mut state, KeyCode::Enter);
    assert_eq!(
        state.pending_input_action,
        Some(crate::tui::commands::InputAction::Command(
            crate::tui::commands::CommandAction {
                id: crate::tui::commands::CommandId::Model,
                args: "local-coder".into()
            }
        ))
    );

    let mut state = state_with_input("/diff", 5);
    state.turn_lifecycle.begin();
    press(&mut state, KeyCode::Enter);
    assert!(state.pending_input_action.is_none());
    assert_eq!(
        state.status_message.as_deref(),
        Some("/diff is unavailable while a turn is running")
    );
}

#[test]
fn busy_enter_keeps_reusable_commands_out_of_steering() {
    let temp = tempfile::tempdir().unwrap();
    let commands = temp.path().join(".ca/commands");
    std::fs::create_dir_all(&commands).unwrap();
    std::fs::write(
        commands.join("inspect.md"),
        "---\ndescription: Inspect the source\n---\nInspect $ARGUMENTS",
    )
    .unwrap();
    let mut state = state_with_input("/inspect source", 15);
    state.prompt_commands = crate::prompt_commands::PromptCommandRegistry::discover(
        &crate::runtime::RuntimePaths::from_home(temp.path()),
        &crate::runtime::ActiveProfile::Default,
        temp.path(),
    );
    assert!(state.prompt_commands.find("inspect").is_some());
    state.turn_lifecycle.begin();
    assert!(!handle_event(
        &mut state,
        &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
        "/tmp/ca-custom-enter-test"
    ));
    assert_eq!(state.input, "/inspect source");
    assert!(state.pending_input_action.is_none());
}

#[test]
fn esc_stops_busy_composer_but_dismisses_popups_first() {
    let mut state = state_with_input("keep my draft", 13);
    state.turn_lifecycle.begin();
    state.input_queue.enqueue("later");
    state.model_picker_open = true;
    press(&mut state, KeyCode::Esc);
    assert!(!state.model_picker_open);
    assert!(state.pending_input_action.is_none());
    press(&mut state, KeyCode::Esc);
    assert_eq!(
        state.pending_input_action,
        Some(crate::tui::commands::InputAction::StopTurn)
    );
    assert_eq!(state.input, "keep my draft");
    assert_eq!(state.input_queue.len(), 1);

    state.pending_input_action = None;
    state.turn_lifecycle.cancel();
    press(&mut state, KeyCode::Esc);
    assert!(state.pending_input_action.is_none());
    assert!(!state.should_quit);
}

#[test]
fn edited_restored_queue_can_send_now_or_be_requeued() {
    for active in [false, true] {
        let mut state = state_with_input("", 0);
        let id = state.input_queue.enqueue("old draft").unwrap();
        state.restored_queue_ids.insert(id);
        state.input_queue.begin_edit_last();
        state.input = "edited draft".into();
        state.input_cursor = state.input.len();
        if active {
            state.turn_lifecycle.begin();
        }
        press_with_modifiers(&mut state, KeyCode::Enter, KeyModifiers::ALT);
        assert_eq!(state.input_queue.front().unwrap().id, id);
        assert_eq!(state.input_queue.front().unwrap().text, "edited draft");
        assert!(!state.restored_queue_ids.contains(&id));
        assert!(state.input_queue.editing_id.is_none());

        state.pending_input_action = None;
        state.input = state.input_queue.begin_edit_last().unwrap();
        state.input_cursor = state.input.len();
        press(&mut state, KeyCode::Enter);
        assert_eq!(
            state.pending_input_action,
            Some(crate::tui::commands::InputAction::Steer {
                text: "edited draft".into(),
                queued_id: Some(id),
            })
        );
    }
}

#[test]
fn interrupt_submission_cannot_bypass_a_blocked_prompt_hook() {
    let mut state = state_with_input("correction", 10);
    state.turn_lifecycle.begin();
    state.hook_blocked = true;
    press(&mut state, KeyCode::Enter);
    assert_eq!(state.input, "correction");
    assert!(state.pending_input_action.is_none());
    assert!(state.status_message.as_deref().unwrap().contains("/hooks"));
    press(&mut state, KeyCode::Esc);
    assert_eq!(
        state.pending_input_action,
        Some(crate::tui::commands::InputAction::StopTurn)
    );
}

#[test]
fn interrupt_controls_work_in_vim_normal_mode() {
    let mut state = state_with_input("correction", 10);
    state.turn_lifecycle.begin();
    state.vim_mode = true;
    state.vim_state = crate::app::VimMode::Normal;
    press(&mut state, KeyCode::Esc);
    assert_eq!(
        state.pending_input_action,
        Some(crate::tui::commands::InputAction::StopTurn)
    );
    assert_eq!(state.input, "correction");
    state.pending_input_action = None;
    press(&mut state, KeyCode::Enter);
    assert!(matches!(
        state.pending_input_action,
        Some(crate::tui::commands::InputAction::Steer { .. })
    ));
}

#[test]
fn busy_submit_keeps_newline_completion_and_empty_draft_behavior() {
    let mut state = state_with_input("hello", 5);
    state.turn_lifecycle.begin();
    press_with_modifiers(&mut state, KeyCode::Enter, KeyModifiers::SHIFT);
    assert_eq!(state.input, "hello\n");
    assert!(state.pending_input_action.is_none());
    state.input = "  \n".into();
    state.input_cursor = state.input.len();
    press(&mut state, KeyCode::Enter);
    assert!(state.pending_input_action.is_none());
    assert_eq!(state.input, "  \n");
}

#[test]
fn queue_view_edit_retains_item_identity_for_steering() {
    let mut state = state_with_input("", 0);
    let id = state.input_queue.enqueue("queued draft").unwrap();
    state.queue_view_open = true;
    press(&mut state, KeyCode::Enter);

    assert_eq!(state.input, "queued draft");
    assert_eq!(state.input_queue.editing_id, Some(id));
    state.turn_lifecycle.begin();
    state.input = "edited steering draft".into();
    state.input_cursor = state.input.len();
    press_ctrl(&mut state, 's');
    assert_eq!(
        state.pending_input_action,
        Some(crate::tui::commands::InputAction::Steer {
            text: "edited steering draft".into(),
            queued_id: Some(id),
        })
    );
}

#[test]
fn ctrl_r_emits_the_shared_change_review_command() {
    let mut state = state_with_input("", 0);
    press_ctrl(&mut state, 'r');

    assert_eq!(
        state.pending_input_action,
        Some(crate::tui::commands::InputAction::Command(
            crate::tui::commands::CommandAction {
                id: crate::tui::commands::CommandId::Diff,
                args: String::new(),
            }
        ))
    );
}

#[test]
fn ctrl_t_emits_reasoning_cycle_without_touching_composer() {
    let mut state = state_with_input("draft", "draft".len());
    press_ctrl(&mut state, 't');
    assert_eq!(
        state.pending_input_action,
        Some(crate::tui::commands::InputAction::CycleReasoning)
    );
    assert_eq!(state.input, "draft");
}

#[test]
fn change_review_opens_only_tracked_file_diffs() {
    let mut state = state_with_input("", 0);
    state.change_review.open = true;
    state.change_review.files = vec![
        crate::changes::ChangeFile {
            path: "src/main.rs".into(),
            old_path: None,
            kind: crate::changes::ChangeKind::Modified,
            conflicted: false,
            staged: false,
            unstaged: true,
            additions: Some(1),
            deletions: Some(0),
            state_fingerprint: "tracked-state".into(),
        },
        crate::changes::ChangeFile {
            path: "secret-untracked.txt".into(),
            old_path: None,
            kind: crate::changes::ChangeKind::Untracked,
            conflicted: false,
            staged: false,
            unstaged: true,
            additions: None,
            deletions: None,
            state_fingerprint: "untracked-state".into(),
        },
    ];

    press(&mut state, KeyCode::Enter);
    assert!(matches!(
        state.pending_input_action,
        Some(crate::tui::commands::InputAction::OpenChangeDiff(_))
    ));

    state.pending_input_action = None;
    state.change_review.selected_index = 1;
    press(&mut state, KeyCode::Enter);
    assert!(state.pending_input_action.is_none());
    assert_eq!(
        state.status_message.as_deref(),
        Some("Untracked file contents are not opened automatically")
    );
}

#[test]
fn escape_returns_from_diff_before_closing_change_review() {
    let mut state = state_with_input("", 0);
    state.change_review.open = true;
    state.change_review.mode = crate::changes::ChangeViewMode::Diff;

    press(&mut state, KeyCode::Esc);
    assert!(state.change_review.open);
    assert_eq!(
        state.change_review.mode,
        crate::changes::ChangeViewMode::List
    );
    press(&mut state, KeyCode::Esc);
    assert!(!state.change_review.open);
}

fn commit_review_state() -> AppState {
    let mut state = state_with_input("composer stays", "composer stays".len());
    state.change_review =
        crate::changes::ChangeReviewState::open_for(crate::changes::ChangeReviewPurpose::Commit);
    state.change_review.loading = false;
    state.change_review.operation = None;
    state.change_review.repository = Some(crate::changes::GitRepositoryState {
        root: "/tmp/repo".into(),
        branch: "master".into(),
        head: Some("1".repeat(40)),
    });
    state.change_review.files = vec![crate::changes::ChangeFile {
        path: "src/main.rs".into(),
        old_path: None,
        kind: crate::changes::ChangeKind::Modified,
        conflicted: false,
        staged: true,
        unstaged: true,
        additions: Some(2),
        deletions: Some(1),
        state_fingerprint: "commit-state".into(),
    }];
    state
}

#[test]
fn commit_review_emits_exact_stage_unstage_and_refresh_actions() {
    let mut state = commit_review_state();
    press(&mut state, KeyCode::Char('s'));
    assert!(matches!(
        state.pending_input_action.take(),
        Some(crate::tui::commands::InputAction::StageChange(file))
            if file.path == std::path::Path::new("src/main.rs")
    ));
    press(&mut state, KeyCode::Char('u'));
    assert!(matches!(
        state.pending_input_action.take(),
        Some(crate::tui::commands::InputAction::UnstageChange(file))
            if file.path == std::path::Path::new("src/main.rs")
    ));
    press(&mut state, KeyCode::Char('r'));
    assert_eq!(
        state.pending_input_action.take(),
        Some(crate::tui::commands::InputAction::RefreshChangeReview)
    );
    press(&mut state, KeyCode::Esc);
    assert!(!state.change_review.open);
    assert_eq!(state.input, "composer stays");
}

#[test]
fn commit_subject_editor_is_unicode_safe_and_prepares_trimmed_subject() {
    let mut state = commit_review_state();
    press(&mut state, KeyCode::Char('c'));
    assert_eq!(
        state.change_review.mode,
        crate::changes::ChangeViewMode::Subject
    );
    for character in "  Fix 界  ".chars() {
        press(&mut state, KeyCode::Char(character));
    }
    press(&mut state, KeyCode::Left);
    press(&mut state, KeyCode::Backspace);
    assert!(state
        .change_review
        .subject
        .is_char_boundary(state.change_review.subject_cursor));
    press(&mut state, KeyCode::End);
    press(&mut state, KeyCode::Enter);
    assert_eq!(
        state.pending_input_action.take(),
        Some(crate::tui::commands::InputAction::PrepareCommit(
            "Fix 界".into()
        ))
    );
    assert_eq!(
        state.change_review.operation,
        Some(crate::changes::GitReviewOperation::Preparing)
    );
    assert_eq!(state.input, "composer stays");
}

#[test]
fn commit_subject_editor_handles_boundaries_delete_and_the_character_limit() {
    let mut state = commit_review_state();
    state.change_review.mode = crate::changes::ChangeViewMode::Subject;
    state.change_review.subject = "a🙂界b".into();
    state.change_review.subject_cursor = "a🙂".len();

    press(&mut state, KeyCode::Delete);
    assert_eq!(state.change_review.subject, "a🙂b");
    assert!(state
        .change_review
        .subject
        .is_char_boundary(state.change_review.subject_cursor));
    press(&mut state, KeyCode::Home);
    press(&mut state, KeyCode::Delete);
    assert_eq!(state.change_review.subject, "🙂b");
    press(&mut state, KeyCode::End);
    press(&mut state, KeyCode::Backspace);
    assert_eq!(state.change_review.subject, "🙂");

    state.change_review.subject = "界".repeat(crate::changes::MAX_COMMIT_SUBJECT_CHARS);
    state.change_review.subject_cursor = state.change_review.subject.len();
    press(&mut state, KeyCode::Char('x'));
    assert_eq!(
        state.change_review.subject.chars().count(),
        crate::changes::MAX_COMMIT_SUBJECT_CHARS
    );
    assert!(state
        .change_review
        .error
        .as_deref()
        .is_some_and(|error| error.contains("at most 256")));
    press_ctrl(&mut state, 'x');
    assert_eq!(
        state.change_review.subject.chars().count(),
        crate::changes::MAX_COMMIT_SUBJECT_CHARS
    );
}

#[test]
fn commit_confirmation_defaults_to_cancel_and_emits_exact_snapshot_only_after_selection() {
    let mut state = commit_review_state();
    let snapshot = crate::changes::GitCommitSnapshot {
        branch: "master".into(),
        head: Some("1".repeat(40)),
        index_tree: "2".repeat(40),
        files: vec![crate::changes::StagedChange {
            path: "src/main.rs".into(),
            old_path: None,
            kind: crate::changes::ChangeKind::Modified,
            additions: Some(2),
            deletions: Some(1),
        }],
    };
    state.change_review.mode = crate::changes::ChangeViewMode::Confirm;
    state.change_review.subject = "Focused commit".into();
    state.change_review.subject_cursor = state.change_review.subject.len();
    state.change_review.commit_snapshot = Some(snapshot.clone());

    assert!(!state.change_review.commit_selected);
    press(&mut state, KeyCode::Enter);
    assert_eq!(
        state.change_review.mode,
        crate::changes::ChangeViewMode::Subject
    );
    assert!(state.pending_input_action.is_none());

    state.change_review.mode = crate::changes::ChangeViewMode::Confirm;
    state.change_review.commit_snapshot = Some(snapshot.clone());
    press(&mut state, KeyCode::Right);
    press(&mut state, KeyCode::Enter);
    assert_eq!(
        state.pending_input_action.take(),
        Some(crate::tui::commands::InputAction::CommitChanges {
            snapshot,
            subject: "Focused commit".into(),
        })
    );
    assert_eq!(
        state.change_review.operation,
        Some(crate::changes::GitReviewOperation::Committing)
    );
}

#[test]
fn busy_commit_review_accepts_only_emergency_cancellation() {
    let mut state = commit_review_state();
    state.change_review.loading = true;
    state.change_review.operation = Some(crate::changes::GitReviewOperation::Committing);
    press(&mut state, KeyCode::Char('s'));
    assert!(state.pending_input_action.is_none());
    press_ctrl(&mut state, 'c');
    assert_eq!(
        state.pending_input_action,
        Some(crate::tui::commands::InputAction::CancelChangeOperation)
    );
}

fn running_job() -> crate::jobs::JobInfo {
    crate::jobs::JobInfo {
        id: "00000000-0000-4000-8000-000000000001".parse().unwrap(),
        command: "read line; echo $line".into(),
        cwd: "/tmp/project".into(),
        status: crate::jobs::JobStatus::Running,
        elapsed: std::time::Duration::from_millis(1250),
        stdout: "ready\n".into(),
        stderr: String::new(),
        stdout_omitted_bytes: 0,
        stderr_omitted_bytes: 0,
        background: true,
    }
}

#[test]
fn jobs_view_emits_cancel_and_enters_stdin_composer() {
    let mut state = state_with_input("", 0);
    let job = running_job();
    state.jobs_view_open = true;
    state.job_list = vec![job.clone()];

    press(&mut state, KeyCode::Char('x'));
    assert_eq!(
        state.pending_input_action,
        Some(crate::tui::commands::InputAction::KillJob(job.id))
    );

    state.pending_input_action = None;
    press(&mut state, KeyCode::Char('i'));
    assert!(!state.jobs_view_open);
    assert_eq!(state.job_stdin_target, Some(job.id));
}

#[test]
fn jobs_detail_scrolls_before_escape_returns_to_list() {
    let mut state = state_with_input("", 0);
    state.jobs_view_open = true;
    state.job_detail_open = true;
    state.job_list = vec![running_job()];
    press(&mut state, KeyCode::Down);
    assert_eq!(state.job_scroll, 1);
    press(&mut state, KeyCode::Esc);
    assert!(state.jobs_view_open);
    assert!(!state.job_detail_open);
}

#[test]
fn watcher_rows_stop_observation_and_never_open_stdin() {
    let started_at = chrono::DateTime::parse_from_rfc3339("2026-08-26T17:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    let id: ca_watch::WatchId = "00000000-0000-4000-8000-000000000011".parse().unwrap();
    let mut state = state_with_input("", 0);
    state.jobs_view_open = true;
    state.watch_list.push(ca_watch::WatchSnapshotV1 {
        version: 1,
        watch_id: id,
        request: ca_watch::WatchRequestV1 {
            label: "copy".into(),
            host: ca_watch::HostSpec::Local,
            pid: 42,
            process: ca_watch::ProcessSpec::Generic,
            interval_secs: 10,
        },
        started_at,
        event: ca_watch::WatchEventV1 {
            version: 1,
            watch_id: id,
            sequence: 2,
            observed_at: started_at,
            state: ca_watch::WatchState::Running,
            summary: "Process is running".into(),
        },
        consecutive_transport_failures: 0,
    });

    press(&mut state, KeyCode::Char('i'));
    assert!(state.jobs_view_open);
    assert!(state.job_stdin_target.is_none());
    assert!(state
        .status_message
        .as_deref()
        .is_some_and(|message| message.contains("do not accept stdin")));

    press(&mut state, KeyCode::Char('x'));
    assert_eq!(
        state.pending_input_action,
        Some(crate::tui::commands::InputAction::StopWatcher(id))
    );
}

#[test]
fn delegation_rows_cancel_and_submit_model_free_feedback_with_correction_confirmation() {
    let mut state = state_with_input("", 0);
    state.jobs_view_open = true;
    state
        .delegation_list
        .push(crate::mechanistic_delegation::DelegationActivitySnapshot {
            job_id: "delegation-190".into(),
            server: "codex_team_delegate".into(),
            action_categories: vec!["evidence".into()],
            status: crate::mechanistic_delegation::DelegationActivityStatus::Running,
            started_at: String::new(),
            updated_at: String::new(),
            repository_revision: "a".repeat(64),
            receipt_digest: None,
            broker_disposition: None,
            source_receipt_digest: None,
            consumer_count: None,
            avoided_execution_millis: 0,
        });
    press(&mut state, KeyCode::Char('x'));
    assert_eq!(
        state.pending_input_action,
        Some(crate::tui::commands::InputAction::CancelDelegation(
            "delegation-190".into()
        ))
    );

    state.pending_input_action = None;
    state.delegation_list[0].status =
        crate::mechanistic_delegation::DelegationActivityStatus::Completed;
    press(&mut state, KeyCode::Char('a'));
    assert_eq!(
        state.pending_input_action,
        Some(crate::tui::commands::InputAction::FeedbackDelegation {
            job_id: "delegation-190".into(),
            category: crate::mechanistic_delegation::DelegationFeedbackCategory::Accepted,
            correction_confirmed: false,
        })
    );

    state.pending_input_action = None;
    press(&mut state, KeyCode::Char('C'));
    assert_eq!(
        state.delegation_correction_confirmation.as_deref(),
        Some("delegation-190")
    );
    assert!(state.pending_input_action.is_none());
    press(&mut state, KeyCode::Char('y'));
    assert_eq!(
        state.pending_input_action,
        Some(crate::tui::commands::InputAction::FeedbackDelegation {
            job_id: "delegation-190".into(),
            category: crate::mechanistic_delegation::DelegationFeedbackCategory::IncorrectResult,
            correction_confirmed: true,
        })
    );
}

#[test]
fn ascii_left_right_backspace_remain_byte_compatible() {
    let mut state = state_with_input("abc", 3);
    press(&mut state, KeyCode::Left);
    assert_eq!(state.input_cursor, 2);
    press(&mut state, KeyCode::Right);
    assert_eq!(state.input_cursor, 3);
    press(&mut state, KeyCode::Backspace);
    assert_eq!(state.input, "ab");
    assert_eq!(state.input_cursor, 2);
}

#[test]
fn ascii_home_end_and_insert_remain_unchanged() {
    let mut state = state_with_input("ac", 1);
    press(&mut state, KeyCode::Char('b'));
    assert_eq!((state.input.as_str(), state.input_cursor), ("abc", 2));
    press(&mut state, KeyCode::Home);
    assert_eq!(state.input_cursor, 0);
    press(&mut state, KeyCode::End);
    assert_eq!(state.input_cursor, 3);
}

#[test]
fn left_and_right_follow_grapheme_boundaries() {
    let text = "A🙂界e\u{301}Z";
    let mut state = state_with_input(text, text.len());
    // The combining mark remains attached to e as one visible grapheme.
    let expected_left = [11, 8, 5, 1, 0];
    for cursor in expected_left {
        press(&mut state, KeyCode::Left);
        assert_eq!(state.input_cursor, cursor);
    }
    assert_eq!(state.input_cursor, 0);
    let expected_right = [1, 5, 8, 11, 12];
    for cursor in expected_right {
        press(&mut state, KeyCode::Right);
        assert_eq!(state.input_cursor, cursor);
    }
    assert_eq!(state.input_cursor, text.len());
}

#[test]
fn backspace_and_delete_remove_one_unicode_scalar() {
    let mut state = state_with_input("A🙂界", "A🙂".len());
    press(&mut state, KeyCode::Backspace);
    assert_eq!((state.input.as_str(), state.input_cursor), ("A界", 1));
    press(&mut state, KeyCode::Delete);
    assert_eq!((state.input.as_str(), state.input_cursor), ("A", 1));
}

#[test]
fn unicode_insertion_at_middle_preserves_boundary() {
    let mut state = state_with_input("A界", 1);
    press(&mut state, KeyCode::Char('🙂'));
    assert_eq!(
        (state.input.as_str(), state.input_cursor),
        ("A🙂界", "A🙂".len())
    );
}

#[test]
fn multiline_unicode_paste_preserves_text_and_boundary() {
    let mut state = state_with_input("AZ", 1);
    assert!(handle_event(
        &mut state,
        &Event::Paste("🙂\n界e\u{301}".to_string()),
        "/tmp/ca-uni004-test",
    ));
    assert_eq!(state.input, "A🙂\n界e\u{301}Z");
    assert_eq!(state.input_cursor, "A🙂\n界e\u{301}".len());
    assert!(state.input.is_char_boundary(state.input_cursor));
}

#[test]
fn ctrl_w_deletes_unicode_word_and_unicode_whitespace() {
    // Unicode non-breaking space (U+00A0) acts as Unicode whitespace
    let text = "hello\u{00a0}world";
    let mut state = state_with_input(text, text.len());
    press_ctrl(&mut state, 'w');
    assert_eq!(state.input, "hello\u{00a0}");
    assert_eq!(state.input_cursor, "hello\u{00a0}".len());
    assert!(state.input.is_char_boundary(state.input_cursor));
    // Kill ring contains the exact deleted slice
    assert_eq!(state.kill_ring.first().unwrap().as_str(), "world");

    // Now delete the Unicode whitespace + word
    press_ctrl(&mut state, 'w');
    assert_eq!(state.input, "");
    assert_eq!(state.input_cursor, 0);
    assert_eq!(state.kill_ring.first().unwrap().as_str(), "hello\u{00a0}");

    // Unicode word with multi-byte scalars: "café ☕ end"
    let text2 = "café ☕ end";
    let mut state2 = state_with_input(text2, text2.len());
    press_ctrl(&mut state2, 'w');
    assert_eq!(state2.input, "café ☕ ");
    assert_eq!(state2.input_cursor, "café ☕ ".len());
    assert_eq!(state2.kill_ring.first().unwrap().as_str(), "end");
}

#[test]
fn option_backspace_deletes_the_previous_unicode_word_atomically() {
    let text = "hello\u{00a0}café 世界";
    let mut state = state_with_input(text, text.len());

    press_with_modifiers(&mut state, KeyCode::Backspace, KeyModifiers::ALT);
    assert_eq!(state.input, "hello\u{00a0}café ");
    assert_eq!(state.input_cursor, "hello\u{00a0}café ".len());
    assert_eq!(state.kill_ring.first().map(String::as_str), Some("世界"));

    let marker = crate::tui::composer::large_paste_placeholder(1_001);
    let input = format!("before {marker}");
    let start = "before ".len();
    let mut atomic = state_with_input(&input, input.len());
    atomic
        .composer_ui
        .register_large_paste(start..input.len(), "x".repeat(1_001));

    press_with_modifiers(&mut atomic, KeyCode::Backspace, KeyModifiers::ALT);
    assert_eq!(atomic.input, "before ");
    assert_eq!(atomic.input_cursor, start);
    assert!(atomic.composer_ui.atomic_elements.is_empty());
}

#[test]
fn command_backspace_deletes_to_the_start_of_the_current_hard_line() {
    let text = "first\nsecond café tail\nthird";
    let cursor = "first\nsecond café".len();
    let mut state = state_with_input(text, cursor);

    press_with_modifiers(&mut state, KeyCode::Backspace, KeyModifiers::SUPER);

    assert_eq!(state.input, "first\n tail\nthird");
    assert_eq!(state.input_cursor, "first\n".len());
    assert_eq!(
        state.kill_ring.first().map(String::as_str),
        Some("second café")
    );
}

#[test]
fn option_and_command_horizontal_arrows_use_native_mac_boundaries() {
    let text = "first line\nhello\u{00a0}café 世界";
    let line_start = "first line\n".len();
    let cafe_start = line_start + "hello\u{00a0}".len();
    let cafe_end = cafe_start + "café".len();
    let world_start = cafe_end + 1;
    let mut state = state_with_input(text, text.len());

    press_with_modifiers(&mut state, KeyCode::Left, KeyModifiers::ALT);
    assert_eq!(state.input_cursor, world_start);
    press_with_modifiers(&mut state, KeyCode::Left, KeyModifiers::ALT);
    assert_eq!(state.input_cursor, cafe_start);
    press_with_modifiers(&mut state, KeyCode::Right, KeyModifiers::ALT);
    assert_eq!(state.input_cursor, cafe_end);

    press_with_modifiers(&mut state, KeyCode::Left, KeyModifiers::SUPER);
    assert_eq!(state.input_cursor, line_start);
    press_with_modifiers(&mut state, KeyCode::Right, KeyModifiers::SUPER);
    assert_eq!(state.input_cursor, text.len());
}

#[test]
fn file_picker_trigger_opens_on_empty_input() {
    use crate::tui::input::should_open_file_picker;
    assert!(should_open_file_picker("", 0));
}

#[test]
fn file_picker_trigger_opens_at_cursor_zero() {
    use crate::tui::input::should_open_file_picker;
    assert!(should_open_file_picker("abc", 0));
}

#[test]
fn file_picker_trigger_opens_after_ascii_space() {
    use crate::tui::input::should_open_file_picker;
    assert!(should_open_file_picker("hello ", 6));
}

#[test]
fn file_picker_trigger_opens_after_unicode_whitespace() {
    use crate::tui::input::should_open_file_picker;
    // U+00A0 non-breaking space
    assert!(should_open_file_picker(
        "hello\u{00a0}",
        "hello\u{00a0}".len()
    ));
    // U+3000 ideographic space
    assert!(should_open_file_picker("x\u{3000}", "x\u{3000}".len()));
    // U+2003 em space
    assert!(should_open_file_picker("\u{2003}", "\u{2003}".len()));
}

#[test]
fn file_picker_trigger_blocked_after_non_whitespace_unicode() {
    use crate::tui::input::should_open_file_picker;
    // Multi-byte non-whitespace Unicode scalar
    assert!(!should_open_file_picker("café@", "café".len()));
    // Emoji directly before cursor
    assert!(!should_open_file_picker("🙂", "🙂".len()));
    // CJK character
    assert!(!should_open_file_picker("界", "界".len()));
    // Combining character sequence
    let text = "e\u{301}";
    assert!(!should_open_file_picker(text, text.len()));
}

fn enable_vim_normal(state: &mut AppState) {
    state.vim_mode = true;
    state.vim_state = crate::app::VimMode::Normal;
}

#[test]
fn vim_h_l_and_a_follow_utf8_boundaries() {
    // "A🙂界" = A(1B) + 🙃(4B) + 界(3B) = 8 bytes
    let text = "A🙂界";
    let mut state = state_with_input(text, 0);
    enable_vim_normal(&mut state);

    // l: 0 → 1 (after A), 1 → 5 (after 🙂), 5 → 8 (after 界)
    press(&mut state, KeyCode::Char('l'));
    assert_eq!(state.input_cursor, 1);
    press(&mut state, KeyCode::Char('l'));
    assert_eq!(state.input_cursor, 5);
    press(&mut state, KeyCode::Char('l'));
    assert_eq!(state.input_cursor, text.len());
    // l at end stays
    press(&mut state, KeyCode::Char('l'));
    assert_eq!(state.input_cursor, text.len());

    // h: 8 → 5 → 1 → 0, h at 0 stays
    press(&mut state, KeyCode::Char('h'));
    assert_eq!(state.input_cursor, 5);
    press(&mut state, KeyCode::Char('h'));
    assert_eq!(state.input_cursor, 1);
    press(&mut state, KeyCode::Char('h'));
    assert_eq!(state.input_cursor, 0);
    press(&mut state, KeyCode::Char('h'));
    assert_eq!(state.input_cursor, 0);

    // a: moves cursor forward by one scalar then enters insert mode
    press(&mut state, KeyCode::Char('a'));
    assert_eq!(state.input_cursor, 1); // after A
    assert_eq!(state.vim_state, crate::app::VimMode::Insert);
}

#[test]
fn vim_x_deletes_current_unicode_scalar() {
    let text = "A🙂界";
    // Cursor at start of 🙂 (byte index 1)
    let mut state = state_with_input(text, 1);
    enable_vim_normal(&mut state);

    // x should delete the 4-byte 🙂 scalar, not one byte
    press(&mut state, KeyCode::Char('x'));
    assert_eq!(state.input, "A界");
    assert_eq!(state.input_cursor, 1);
    assert!(state.input.is_char_boundary(state.input_cursor));

    // x again deletes 界 (3-byte CJK scalar)
    press(&mut state, KeyCode::Char('x'));
    assert_eq!(state.input, "A");
    assert_eq!(state.input_cursor, 1);
    assert!(state.input.is_char_boundary(state.input_cursor));

    // x at end of string is a no-op
    press(&mut state, KeyCode::Char('x'));
    assert_eq!(state.input, "A");
    assert_eq!(state.input_cursor, 1);
}

#[test]
fn vim_w_and_b_cross_unicode_words_and_whitespace() {
    // U+3000 ideographic space as Unicode whitespace, CJK characters as non-whitespace word
    // "hello 世界　café" where 　 is U+3000 (3-byte ideographic space)
    let text = "hello\u{3000}世界 café";
    let mut state = state_with_input(text, 0);
    enable_vim_normal(&mut state);

    // w from 0: skip over "hello" (non-whitespace), land at start of "世界" after U+3000
    let after_hello = "hello".len(); // byte 5 — start of U+3000
    let after_3000 = after_hello + '\u{3000}'.len_utf8(); // byte 8 — start of "世界"
    press(&mut state, KeyCode::Char('w'));
    assert_eq!(
        state.input_cursor, after_3000,
        "w should land at start of next word after Unicode whitespace U+3000"
    );

    // w again: skip over "世界", land at space (ASCII ws), then at "café"
    let world_end = after_3000 + "世界".len(); // byte 14 — start of " café"
    let after_space = world_end + 1; // byte 15 — start of "café"
    press(&mut state, KeyCode::Char('w'));
    assert_eq!(
        state.input_cursor, after_space,
        "w should skip ASCII space and land at start of next word"
    );

    // w at end: stays at end
    let text_end = text.len();
    press(&mut state, KeyCode::Char('w'));
    assert_eq!(
        state.input_cursor, text_end,
        "w at end of text should stay at end"
    );
    press(&mut state, KeyCode::Char('w'));
    assert_eq!(state.input_cursor, text_end);

    // b from end: retreat to start of "café"
    press(&mut state, KeyCode::Char('b'));
    assert_eq!(
        state.input_cursor, after_space,
        "b should retreat to start of current/previous word 'café'"
    );

    // b again: retreat to start of "世界" (skipping ASCII space and U+3000)
    press(&mut state, KeyCode::Char('b'));
    assert_eq!(
        state.input_cursor, after_3000,
        "b should retreat to start of '世界' crossing both ASCII and Unicode whitespace"
    );

    // b again: retreat to start of "hello"
    press(&mut state, KeyCode::Char('b'));
    assert_eq!(
        state.input_cursor, 0,
        "b should retreat to start of first word 'hello'"
    );

    // b at start: stays at 0
    press(&mut state, KeyCode::Char('b'));
    assert_eq!(state.input_cursor, 0, "b at start of text should stay at 0");

    // Cursor always on valid char boundary
    assert!(text.is_char_boundary(state.input_cursor));
}

#[test]
fn vim_ascii_word_motion_is_unchanged() {
    // ASCII-only test: "hello world  foo" preserves prior w/b semantics
    let text = "hello world  foo";
    let mut state = state_with_input(text, 0);
    enable_vim_normal(&mut state);

    // w: 0→6 (start of "world"), 6→13 (start of "foo"), 13→end stays
    press(&mut state, KeyCode::Char('w'));
    assert_eq!(state.input_cursor, 6, "w should land at start of 'world'");
    press(&mut state, KeyCode::Char('w'));
    assert_eq!(state.input_cursor, 13, "w should land at start of 'foo'");
    press(&mut state, KeyCode::Char('w'));
    assert_eq!(state.input_cursor, text.len(), "w at end stays at end");
    press(&mut state, KeyCode::Char('w'));
    assert_eq!(state.input_cursor, text.len(), "w at end stays at end");

    // b: end→13 (start of "foo"), 13→6 (start of "world"), 6→0 (start of "hello"), 0 stays
    press(&mut state, KeyCode::Char('b'));
    assert_eq!(state.input_cursor, 13, "b should retreat to start of 'foo'");
    press(&mut state, KeyCode::Char('b'));
    assert_eq!(
        state.input_cursor, 6,
        "b should retreat to start of 'world'"
    );
    press(&mut state, KeyCode::Char('b'));
    assert_eq!(
        state.input_cursor, 0,
        "b should retreat to start of 'hello'"
    );
    press(&mut state, KeyCode::Char('b'));
    assert_eq!(state.input_cursor, 0, "b at start stays at 0");

    // Cursor always on valid char boundary
    assert!(text.is_char_boundary(state.input_cursor));
}

/// Characterization: a mixed Unicode editing sequence through production
/// handle_event never leaves input_cursor on an invalid UTF-8 boundary.
#[test]
fn mixed_unicode_edit_sequence_never_leaves_invalid_cursor() {
    let mut state = state_with_input("", 0);

    // Type emoji and CJK
    press(&mut state, KeyCode::Char('🙂'));
    press(&mut state, KeyCode::Char('界'));
    assert_eq!(state.input, "🙂界");

    // Move left past both multi-byte scalars
    press(&mut state, KeyCode::Left);
    assert_eq!(state.input_cursor, "🙂".len());

    // Insert combining char sequence
    press(&mut state, KeyCode::Char('e'));
    press(&mut state, KeyCode::Char('\u{301}'));
    assert_eq!(state.input, "🙂e\u{301}界");

    // Backspace removes the complete visible grapheme (e + combining accent).
    press(&mut state, KeyCode::Backspace);
    assert_eq!(state.input, "🙂界");

    // Delete removes 界 (cursor is already at start of 界)
    press(&mut state, KeyCode::Delete);
    assert_eq!(state.input, "🙂");

    // Ctrl+U clears everything
    press_ctrl(&mut state, 'u');
    assert_eq!(state.input, "");
    assert_eq!(state.input_cursor, 0);

    // Paste multi-Unicode, navigate, delete word
    assert!(handle_event(
        &mut state,
        &Event::Paste("hello café ☕ 界".to_string()),
        "/tmp/ca-uni004-test",
    ));
    assert!(state.input.is_char_boundary(state.input_cursor));

    // Ctrl+W from end
    press_ctrl(&mut state, 'w');
    assert!(state.input.is_char_boundary(state.input_cursor));
    assert!(state.input_cursor <= state.input.len());

    // Ctrl+W again
    press_ctrl(&mut state, 'w');
    assert!(state.input.is_char_boundary(state.input_cursor));
    assert!(state.input_cursor <= state.input.len());
}

#[test]
fn multiline_vertical_motion_enters_history_only_at_boundaries_and_restores_draft() {
    let mut state = state_with_input("abc\ndef", "abc\ndef".len());
    state.viewport_width = 40;
    state.input_history = vec!["older request".into()];
    state
        .composer_ui
        .register_atomic(crate::tui::composer::AtomicElement {
            range: 4..7,
            kind: crate::tui::composer::AtomicKind::File,
        });
    state.pasted_images.push(
        crate::image_content::RawClipboardImage::new(1, 1, vec![0, 0, 0, 255])
            .unwrap()
            .normalize()
            .unwrap(),
    );

    press(&mut state, KeyCode::Up);
    assert_eq!(state.input, "abc\ndef");
    assert_eq!(state.input_cursor, 3);
    press(&mut state, KeyCode::Up);
    assert_eq!(state.input, "older request");
    assert!(state.composer_ui.atomic_elements.is_empty());
    assert!(state.pasted_images.is_empty());
    press(&mut state, KeyCode::Down);
    assert_eq!(state.input, "abc\ndef");
    assert_eq!(state.input_cursor, 3);
    assert_eq!(state.composer_ui.atomic_elements.len(), 1);
    assert_eq!(state.pasted_images.len(), 1);
}

#[test]
fn home_end_are_hard_line_local_while_ctrl_variants_span_the_draft() {
    let mut state = state_with_input("first\nsecond\nthird", "first\nsec".len());
    press(&mut state, KeyCode::Home);
    assert_eq!(state.input_cursor, "first\n".len());
    press(&mut state, KeyCode::End);
    assert_eq!(state.input_cursor, "first\nsecond".len());
    press_ctrl(&mut state, 'a');
    assert_eq!(state.input_cursor, 0);
    press_ctrl(&mut state, 'e');
    assert_eq!(state.input_cursor, state.input.len());
}

#[test]
fn large_paste_is_registered_and_expands_exactly_at_dispatch() {
    let payload = "é".repeat(1_001);
    let mut state = state_with_input("before ", "before ".len());
    assert!(handle_event(
        &mut state,
        &Event::Paste(payload.clone()),
        "/tmp/ca-large-paste-test",
    ));
    assert!(state.input.contains("[Pasted Content 1001 chars]"));
    assert_eq!(state.composer_ui.atomic_elements.len(), 1);
    assert_eq!(state.take_composer_text(), format!("before {payload}"));
    assert!(state.composer_ui.atomic_elements.is_empty());
}

#[test]
fn slash_completion_has_hybrid_enter_tab_and_token_scoped_escape() {
    let mut required = state_with_input("", 0);
    for character in "/preset-save".chars() {
        press(&mut required, KeyCode::Char(character));
    }
    press(&mut required, KeyCode::Enter);
    assert_eq!(required.input, "/preset-save ");
    assert!(required.pending_input_action.is_none());

    let mut tab = state_with_input("", 0);
    for character in "/he".chars() {
        press(&mut tab, KeyCode::Char(character));
    }
    press(&mut tab, KeyCode::Tab);
    assert_eq!(tab.input, "/help");
    assert!(tab.pending_input_action.is_none());

    let mut dismissed = state_with_input("", 0);
    for character in "/he".chars() {
        press(&mut dismissed, KeyCode::Char(character));
    }
    press(&mut dismissed, KeyCode::Esc);
    assert_eq!(dismissed.input, "/he");
    assert!(!dismissed.slash_palette_open);
    press(&mut dismissed, KeyCode::Char('l'));
    assert!(dismissed.slash_palette_open);
}

#[test]
fn path_directory_completion_stays_active_and_file_completion_closes() {
    let mut directory = state_with_input("./s", 3);
    directory.composer_ui.path_completion.open = true;
    directory.composer_ui.path_completion.token = "./s".into();
    directory.composer_ui.path_completion.entries =
        vec![crate::tui::composer::PathCompletionEntry {
            display: "src/".into(),
            replacement: "./src/".into(),
            is_dir: true,
        }];
    press(&mut directory, KeyCode::Enter);
    assert_eq!(directory.input, "./src/");
    assert!(directory.composer_ui.path_completion.open);
    assert!(matches!(
        directory.pending_input_action,
        Some(crate::tui::commands::InputAction::RefreshPathCompletion)
    ));

    let mut file = state_with_input("./C", 3);
    file.composer_ui.path_completion.open = true;
    file.composer_ui.path_completion.token = "./C".into();
    file.composer_ui.path_completion.entries = vec![crate::tui::composer::PathCompletionEntry {
        display: "Cargo.toml".into(),
        replacement: "./Cargo.toml".into(),
        is_dir: false,
    }];
    press(&mut file, KeyCode::Tab);
    assert_eq!(file.input, "./Cargo.toml ");
    assert!(!file.composer_ui.path_completion.open);
    assert!(matches!(
        file.composer_ui.atomic_elements[0].kind,
        crate::tui::composer::AtomicKind::File
    ));
}

#[test]
fn prose_slashes_do_not_open_path_completion_or_block_cursor_motion() {
    let mut state = state_with_input("", 0);
    for character in "separated by red/ops".chars() {
        press(&mut state, KeyCode::Char(character));
    }

    assert!(!state.composer_ui.path_completion.open);
    assert!(state.pending_input_action.is_none());
    let end = state.input.len();
    press(&mut state, KeyCode::Left);
    assert_eq!(state.input_cursor, end - 1);
}

#[test]
fn horizontal_arrows_move_the_cursor_while_path_completion_is_open() {
    let mut state = state_with_input("./src", 5);
    state.composer_ui.path_completion.open = true;
    state.composer_ui.path_completion.token = "./src".into();

    press(&mut state, KeyCode::Left);
    assert_eq!(state.input_cursor, 4);
    press(&mut state, KeyCode::Right);
    assert_eq!(state.input_cursor, 5);
    assert!(state.pending_input_action.is_none());
}

#[test]
fn horizontal_arrows_move_the_cursor_while_slash_palette_is_open() {
    let mut state = state_with_input("/delegate", "/delegate".len());
    state.slash_palette_open = true;
    state.slash_filter = "delegate".into();

    press(&mut state, KeyCode::Left);
    assert_eq!(state.input_cursor, "/delegat".len());
    press(&mut state, KeyCode::Right);
    assert_eq!(state.input_cursor, state.input.len());
    assert!(state.slash_palette_open);
}

#[test]
fn horizontal_arrows_move_the_cursor_while_file_picker_is_open() {
    let mut state = state_with_input("attach @src", "attach @src".len());
    state.file_picker_open = true;
    state.file_picker_filter = "src".into();

    press(&mut state, KeyCode::Left);
    assert_eq!(state.input_cursor, "attach @sr".len());
    press(&mut state, KeyCode::Right);
    assert_eq!(state.input_cursor, state.input.len());
    assert!(state.file_picker_open);
}

#[test]
fn native_mac_editing_keys_keep_working_while_path_completion_is_open() {
    let mut option_left = state_with_input("open ./some/path", "open ./some/path".len());
    option_left.composer_ui.path_completion.open = true;
    option_left.composer_ui.path_completion.token = "./some/path".into();
    press_with_modifiers(&mut option_left, KeyCode::Left, KeyModifiers::ALT);
    assert_eq!(option_left.input_cursor, "open ".len());

    let mut command_left = state_with_input("open ./some/path", "open ./some/path".len());
    command_left.composer_ui.path_completion.open = true;
    command_left.composer_ui.path_completion.token = "./some/path".into();
    press_with_modifiers(&mut command_left, KeyCode::Left, KeyModifiers::SUPER);
    assert_eq!(command_left.input_cursor, 0);

    let mut option_delete = state_with_input("open ./some/path", "open ./some/path".len());
    option_delete.composer_ui.path_completion.open = true;
    option_delete.composer_ui.path_completion.token = "./some/path".into();
    press_with_modifiers(&mut option_delete, KeyCode::Backspace, KeyModifiers::ALT);
    assert_eq!(option_delete.input, "open ");
}

#[test]
fn tab_path_completion_still_accepts_bare_relative_paths() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("src")).unwrap();
    std::fs::write(root.path().join("src/main.rs"), "fn main() {}\n").unwrap();
    let mut state = state_with_input("src/ma", 6);

    assert!(handle_event(
        &mut state,
        &Event::Key(KeyCode::Tab, KeyModifiers::NONE),
        root.path().to_string_lossy().as_ref(),
    ));
    assert_eq!(state.input, "src/main.rs");
    assert_eq!(state.input_cursor, state.input.len());
}

#[test]
fn finder_style_folder_paste_registers_an_exact_chip_without_granting_access() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let folder = outside.path().join("Folder With Space");
    std::fs::create_dir(&folder).unwrap();
    let escaped = format!("{} ", folder.to_string_lossy().replace(' ', "\\ "));
    let mut state = state_with_input("", 0);
    assert!(handle_event(
        &mut state,
        &Event::Paste(escaped.clone()),
        root.path().to_string_lossy().as_ref(),
    ));
    assert_eq!(state.input, escaped);
    assert!(matches!(
        state.composer_ui.atomic_elements[0].kind,
        crate::tui::composer::AtomicKind::Folder {
            outside_workspace: true
        }
    ));
    assert!(state
        .status_message
        .as_deref()
        .unwrap()
        .contains("/add-dir"));
}

/// Adversarial: Ctrl+W with cursor at an interior emoji byte.
/// previous_word_boundary normalizes via floor_char_boundary before slicing.
#[test]
fn ctrl_w_with_interior_emoji_byte_cursor_normalizes_and_does_not_panic() {
    // "hi 🙂world": 🙂 is 4 bytes at byte index 3..7
    let text = "hi \u{1f642}world";
    let mut state = state_with_input(text, text.len());
    state.input_cursor = 5; // interior of 🙂, not a char boundary
    assert!(!state.input.is_char_boundary(state.input_cursor));

    // Ctrl+W should normalize cursor via floor_char_boundary internally and not panic
    press_ctrl(&mut state, 'w');
    assert!(state.input.is_char_boundary(state.input_cursor));
    assert!(state.input_cursor <= state.input.len());
    assert!(!state.kill_ring.is_empty() || state.input.is_empty());
}

#[test]
fn typed_keymap_dispatches_custom_one_and_default_two_stroke_without_shadowing_diff() {
    let mut state = state_with_input("draft", 5);
    let mut configured = crate::keymap::KeymapConfig::new();
    configured.insert(
        "model_picker".into(),
        crate::keymap::KeyBindingConfig::One("ctrl+m".into()),
    );
    state.keymap = crate::keymap::Keymap::from_config(&configured).unwrap();

    assert!(!handle_event(
        &mut state,
        &Event::Key(KeyCode::Char('k'), KeyModifiers::CONTROL),
        "/tmp/ca-keymap-test"
    ));
    assert!(!state.model_picker_open);
    press_ctrl(&mut state, 'm');
    assert!(state.model_picker_open);
    press(&mut state, KeyCode::Esc);

    press_ctrl(&mut state, 'x');
    assert!(state.pending_key_chord.is_some());
    press_ctrl(&mut state, 'r');
    assert!(state.history_search.is_some());
    assert!(state.status_message.is_none());
    assert_eq!(state.input, "draft");
    press(&mut state, KeyCode::Esc);

    press_ctrl(&mut state, 'r');
    assert_eq!(
        state.pending_input_action.take(),
        Some(crate::tui::commands::InputAction::Command(
            crate::tui::commands::CommandAction {
                id: crate::tui::commands::CommandId::Diff,
                args: String::new(),
            }
        ))
    );
}

#[test]
fn pane_key_chords_queue_only_typed_pane_actions() {
    let cases = [
        (
            'h',
            crate::panes::PaneRequest::Split {
                axis: crate::panes::SplitAxis::Horizontal,
                session: None,
            },
        ),
        (
            'v',
            crate::panes::PaneRequest::Split {
                axis: crate::panes::SplitAxis::Vertical,
                session: None,
            },
        ),
        ('n', crate::panes::PaneRequest::FocusNext),
        ('p', crate::panes::PaneRequest::FocusPrevious),
        ('q', crate::panes::PaneRequest::Close),
    ];
    for (second, expected) in cases {
        let mut state = state_with_input("pane-local draft", 16);
        press_ctrl(&mut state, 'x');
        press(&mut state, KeyCode::Char(second));
        assert_eq!(state.pane_request.take(), Some(expected));
        assert_eq!(state.input, "pane-local draft");
        assert!(state.pending_input_action.is_none());
    }
}

#[test]
fn keymap_capture_reset_bare_and_paste_are_isolated_from_composer() {
    let mut state = state_with_input("keep composer", 13);
    crate::tui::input::open_keymap_hub(&mut state, String::new());
    assert!(handle_event(
        &mut state,
        &Event::Paste("history".into()),
        "/tmp/keymap"
    ));
    assert_eq!(state.input, "keep composer");
    assert_eq!(state.keymap_hub.as_ref().unwrap().filter, "history");

    press(&mut state, KeyCode::Enter);
    press_ctrl(&mut state, 'h');
    press(&mut state, KeyCode::Enter);
    assert_eq!(
        state.pending_input_action.take(),
        Some(crate::tui::commands::InputAction::SaveKeymapBinding {
            action: crate::keymap::KeyAction::HistorySearch,
            chord: Some("ctrl+h".parse().unwrap()),
        })
    );
    assert!(state.keymap_hub.as_ref().unwrap().saving);

    let mut two = state_with_input("untouched", 9);
    crate::tui::input::open_keymap_hub(&mut two, "history".into());
    press(&mut two, KeyCode::Enter);
    press_ctrl(&mut two, 'x');
    press_ctrl(&mut two, 'h');
    assert_eq!(
        two.pending_input_action.take(),
        Some(crate::tui::commands::InputAction::SaveKeymapBinding {
            action: crate::keymap::KeyAction::HistorySearch,
            chord: Some("ctrl+x ctrl+h".parse().unwrap()),
        })
    );

    let mut reset = state_with_input("", 0);
    crate::tui::input::open_keymap_hub(&mut reset, "history".into());
    press_with_modifiers(&mut reset, KeyCode::Char('R'), KeyModifiers::SHIFT);
    assert_eq!(
        reset.pending_input_action.take(),
        Some(crate::tui::commands::InputAction::SaveKeymapBinding {
            action: crate::keymap::KeyAction::HistorySearch,
            chord: None,
        })
    );

    let mut bare = state_with_input("", 0);
    bare.run_capabilities =
        crate::runtime::RunCapabilities::for_profile(&crate::runtime::ActiveProfile::Bare);
    crate::tui::input::open_keymap_hub(&mut bare, String::new());
    press(&mut bare, KeyCode::Enter);
    assert!(bare.pending_input_action.is_none());
    assert!(bare
        .keymap_hub
        .as_ref()
        .unwrap()
        .error
        .as_deref()
        .unwrap()
        .contains("bare mode"));
}

#[test]
fn history_search_is_newest_first_filterable_duplicate_preserving_and_draft_only() {
    let mut state = state_with_input("existing draft", 14);
    state.input_history = vec![
        "first alpha".into(),
        "middle beta".into(),
        "first alpha".into(),
    ];
    crate::tui::input::open_history_search(&mut state, "first".into());
    assert!(handle_event(
        &mut state,
        &Event::Paste(" alpha".into()),
        "/tmp/history"
    ));
    assert_eq!(state.input, "existing draft");
    assert_eq!(state.history_search.as_ref().unwrap().filter, "first alpha");
    press(&mut state, KeyCode::Down);
    press(&mut state, KeyCode::Enter);
    assert!(state.history_search.is_none());
    assert_eq!(state.input, "first alpha");
    assert_eq!(state.history_idx, Some(0));
    assert_eq!(state.pending_input_action, None);
}

#[test]
fn modifier_key_events_do_not_consume_a_pending_chord() {
    let mut state = state_with_input("draft", 5);
    press_ctrl(&mut state, 'x');
    assert!(handle_event(
        &mut state,
        &Event::Key(
            KeyCode::Modifier(crossterm::event::ModifierKeyCode::LeftControl),
            KeyModifiers::CONTROL,
        ),
        "/tmp/keymap",
    ));
    assert!(state.pending_key_chord.is_some());
    press(&mut state, KeyCode::Char('j'));
    assert!(state.pending_key_chord.is_none());
    assert_eq!(state.input, "draft");
}

#[test]
fn two_stroke_prefix_times_out_and_unmatched_ctrl_r_still_opens_diff() {
    let mut state = state_with_input("draft", 5);
    let mut configured = crate::keymap::KeymapConfig::new();
    configured.insert(
        "history_search".into(),
        crate::keymap::KeyBindingConfig::One("ctrl+x ctrl+h".into()),
    );
    state.keymap = crate::keymap::Keymap::from_config(&configured).unwrap();

    press_ctrl(&mut state, 'x');
    state.pending_key_chord.as_mut().unwrap().started_at -= std::time::Duration::from_secs(2);
    assert!(handle_event(&mut state, &Event::Tick, "/tmp/keymap"));
    assert!(state.pending_key_chord.is_none());
    assert_eq!(state.status_message.as_deref(), Some("Key chord timed out"));

    press_ctrl(&mut state, 'x');
    press_ctrl(&mut state, 'r');
    assert_eq!(
        state.pending_input_action.take(),
        Some(crate::tui::commands::InputAction::Command(
            crate::tui::commands::CommandAction {
                id: crate::tui::commands::CommandId::Diff,
                args: String::new(),
            }
        ))
    );
    assert!(state.history_search.is_none());
}

#[test]
fn two_stroke_prefix_cannot_shadow_emergency_process_controls() {
    let mut state = state_with_input("draft", 5);

    press_ctrl(&mut state, 'x');
    assert!(state.pending_key_chord.is_some());
    press_ctrl(&mut state, 'c');
    assert!(state.should_quit);
    assert!(state.pending_key_chord.is_none());

    let mut empty = state_with_input("", 0);
    press_ctrl(&mut empty, 'x');
    press_ctrl(&mut empty, 'd');
    assert!(empty.should_quit);
    assert!(empty.pending_key_chord.is_none());

    let mut draft = state_with_input("keep", 4);
    press_ctrl(&mut draft, 'x');
    press_ctrl(&mut draft, 'd');
    assert!(!draft.should_quit);
    assert!(draft.pending_key_chord.is_none());
    assert_eq!(draft.input, "keep");
}

fn live_agent(
    id: crate::agent::subagent::AgentId,
    turn: uuid::Uuid,
    status: crate::agent::subagent::AgentStatus,
) -> crate::agent::subagent::AgentActivitySnapshot {
    crate::agent::subagent::AgentActivitySnapshot {
        id,
        parent_turn: turn,
        parent: None,
        origin: crate::agent::subagent::AgentRunOrigin::Conversation,
        purpose: crate::agent::subagent::AgentRunPurpose::Subagent,
        status,
        task: "audit activity UI".into(),
        role: Some("Explorer".into()),
        model: Some("fixture-model".into()),
        provider: Some("fixture".into()),
        phase: crate::agent::subagent::AgentExecutionPhase::Tool,
        current_action: Some("read_file".into()),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        result_summary: None,
        recent_output: "bounded output".into(),
        events: Vec::new(),
    }
}

#[test]
fn agent_activity_chord_focuses_and_controls_are_cancel_first() {
    let mut state = state_with_input("keep draft", 10);
    let turn = uuid::Uuid::new_v4();
    let id = crate::agent::subagent::AgentId::new();
    state.live_agent_activity = vec![live_agent(
        id,
        turn,
        crate::agent::subagent::AgentStatus::Running,
    )];

    press_ctrl(&mut state, 'x');
    press(&mut state, KeyCode::Down);
    assert_eq!(
        state
            .agent_activity_focus
            .as_ref()
            .and_then(|focus| focus.selected_agent),
        Some(id)
    );
    assert_eq!(state.input, "keep draft");

    state.agent_activity_focus = None;
    press_ctrl(&mut state, 'x');
    press_with_modifiers(&mut state, KeyCode::Down, KeyModifiers::CONTROL);
    assert_eq!(
        state
            .agent_activity_focus
            .as_ref()
            .and_then(|focus| focus.selected_agent),
        Some(id)
    );

    state.agent_activity_focus = None;
    press_ctrl(&mut state, 'x');
    press_ctrl(&mut state, 'j');
    assert_eq!(
        state
            .agent_activity_focus
            .as_ref()
            .and_then(|focus| focus.selected_agent),
        Some(id)
    );
    assert_eq!(state.input, "keep draft");

    press(&mut state, KeyCode::Char('c'));
    assert!(
        state
            .agent_activity_focus
            .as_ref()
            .unwrap()
            .cancel_confirmation
    );
    press(&mut state, KeyCode::Enter);
    assert!(state.agents_request.is_none());

    press(&mut state, KeyCode::Char('c'));
    press(&mut state, KeyCode::Right);
    press(&mut state, KeyCode::Enter);
    assert_eq!(state.agents_request, Some(format!("cancel {id}")));
    assert!(state.agent_activity_action_pending);
}

#[test]
fn focused_agent_message_and_approval_actions_are_exact() {
    let mut state = state_with_input("composer", 8);
    let turn = uuid::Uuid::new_v4();
    let id = crate::agent::subagent::AgentId::new();
    state.live_agent_activity = vec![live_agent(
        id,
        turn,
        crate::agent::subagent::AgentStatus::Running,
    )];
    press_ctrl(&mut state, 'x');
    press(&mut state, KeyCode::Down);

    press(&mut state, KeyCode::Char('m'));
    for character in "check this".chars() {
        press(&mut state, KeyCode::Char(character));
    }
    press(&mut state, KeyCode::Enter);
    assert_eq!(
        state.agents_request,
        Some(format!("message {id} check this"))
    );
    assert_eq!(state.input, "composer");

    state.agents_request = None;
    state.live_agent_activity[0].status = crate::agent::subagent::AgentStatus::WaitingApproval;
    press(&mut state, KeyCode::Char('y'));
    assert_eq!(state.agents_request, Some(format!("approve {id}")));
    state.agents_request = None;
    press(&mut state, KeyCode::Char('n'));
    assert_eq!(state.agents_request, Some(format!("deny {id}")));
}

#[test]
fn focused_agent_navigation_scrolls_and_terminal_summaries_are_read_only() {
    let mut state = state_with_input("composer", 8);
    let turn = uuid::Uuid::new_v4();
    let first = crate::agent::subagent::AgentId::new();
    let second = crate::agent::subagent::AgentId::new();
    let mut first_snapshot = live_agent(first, turn, crate::agent::subagent::AgentStatus::Running);
    first_snapshot.updated_at = chrono::Utc::now() - chrono::Duration::seconds(1);
    state.live_agent_activity = vec![
        first_snapshot,
        live_agent(second, turn, crate::agent::subagent::AgentStatus::Running),
    ];

    press_ctrl(&mut state, 'x');
    press(&mut state, KeyCode::Down);
    assert_eq!(
        state.agent_activity_focus.as_ref().unwrap().selected_agent,
        Some(second)
    );
    press(&mut state, KeyCode::Down);
    assert_eq!(
        state.agent_activity_focus.as_ref().unwrap().selected_agent,
        Some(first)
    );
    press(&mut state, KeyCode::PageUp);
    assert_eq!(
        state.agent_activity_focus.as_ref().unwrap().detail_scroll,
        8
    );
    press(&mut state, KeyCode::PageDown);
    assert_eq!(
        state.agent_activity_focus.as_ref().unwrap().detail_scroll,
        0
    );
    press(&mut state, KeyCode::Enter);
    assert!(!state.agent_activity_focus.as_ref().unwrap().detail_open);

    state.live_agent_activity.clear();
    state.agent_activity_focus = None;
    state.transcript_agent_summaries = vec![crate::app::TranscriptAgentSummary {
        agent_id: second,
        turn_id: turn,
        purpose: crate::agent::subagent::AgentRunPurpose::Subagent,
        task: "completed audit".into(),
        role: None,
        model: None,
        provider: None,
        created_at: None,
        status: crate::agent::subagent::AgentStatus::Completed,
        summary: "audit complete".into(),
        updated_at: chrono::Utc::now(),
    }];
    press_ctrl(&mut state, 'x');
    press(&mut state, KeyCode::Down);
    for key in ['m', 'y', 'n', 'c'] {
        press(&mut state, KeyCode::Char(key));
    }

    assert!(state.agents_request.is_none());
    let focus = state.agent_activity_focus.as_ref().unwrap();
    assert!(focus.message_draft.is_none());
    assert!(!focus.cancel_confirmation);
    assert_eq!(state.input, "composer");
}

#[test]
fn focused_agent_navigation_keeps_roster_order_when_activity_updates() {
    let mut state = state_with_input("composer", 8);
    let turn = uuid::Uuid::new_v4();
    let at = chrono::DateTime::<chrono::Utc>::from_timestamp(0, 0).unwrap();
    let ids = [
        crate::agent::subagent::AgentId::new(),
        crate::agent::subagent::AgentId::new(),
        crate::agent::subagent::AgentId::new(),
    ];
    state.live_agent_activity = ids
        .iter()
        .enumerate()
        .map(|(index, id)| {
            let mut agent = live_agent(*id, turn, crate::agent::subagent::AgentStatus::Running);
            agent.created_at = at + chrono::Duration::seconds(index as i64);
            agent.updated_at = at + chrono::Duration::seconds(10 - index as i64);
            agent
        })
        .collect();
    state.agent_activity_focus = Some(crate::app::AgentActivityFocusState {
        turn_id: turn,
        selected_agent: Some(ids[1]),
        ..crate::app::AgentActivityFocusState::default()
    });
    press(&mut state, KeyCode::Down);
    assert_eq!(
        state.agent_activity_focus.as_ref().unwrap().selected_agent,
        Some(ids[2])
    );
    state.live_agent_activity[2].updated_at = at + chrono::Duration::seconds(20);
    press(&mut state, KeyCode::Up);
    assert_eq!(
        state.agent_activity_focus.as_ref().unwrap().selected_agent,
        Some(ids[1])
    );
}

#[test]
fn voice_controls_are_explicit_and_isolate_composer_and_paste() {
    let mut state = state_with_input("draft🙂tail", "draft🙂".len());
    state.voice = Some(crate::voice::VoiceState::ready(3));

    assert!(handle_event(
        &mut state,
        &Event::Paste("must not leak".into()),
        "/tmp/voice"
    ));
    assert_eq!(state.input, "draft🙂tail");

    press(&mut state, KeyCode::Char(' '));
    assert_eq!(
        state.pending_input_action.take(),
        Some(crate::tui::commands::InputAction::StartVoice)
    );
    assert_eq!(
        state.voice.as_ref().unwrap().phase,
        crate::voice::VoicePhase::Recording
    );

    press(&mut state, KeyCode::Enter);
    assert_eq!(
        state.pending_input_action.take(),
        Some(crate::tui::commands::InputAction::StopVoice)
    );
    assert_eq!(state.input, "draft🙂tail");

    state.voice.as_mut().unwrap().phase = crate::voice::VoicePhase::Transcribing;
    press(&mut state, KeyCode::Esc);
    assert_eq!(
        state.pending_input_action.take(),
        Some(crate::tui::commands::InputAction::CancelVoice)
    );
    assert!(state.voice.is_none());
    assert!(state.messages.is_empty());

    state.voice_generation = 5;
    state.voice = Some(crate::voice::VoiceState::ready(5));
    press_ctrl(&mut state, 'c');
    assert_eq!(
        state.pending_input_action.take(),
        Some(crate::tui::commands::InputAction::CancelVoice)
    );
    assert!(state.voice.is_none());
    assert!(!state.should_quit);
}

#[test]
fn failed_voice_can_retry_without_touching_draft() {
    let mut state = state_with_input("keep", 4);
    state.voice = Some(crate::voice::VoiceState {
        phase: crate::voice::VoicePhase::Failed,
        generation: 8,
        error: Some("safe failure".into()),
    });
    press(&mut state, KeyCode::Char(' '));
    assert_eq!(
        state.voice.as_ref().unwrap().phase,
        crate::voice::VoicePhase::Recording
    );
    assert!(state.voice.as_ref().unwrap().error.is_none());
    assert_eq!(state.input, "keep");
}

#[test]
fn attachment_picker_opens_asynchronously_and_tracks_query_generations() {
    let mut state = state_with_input("attach ", 7);
    press(&mut state, KeyCode::Char('@'));
    assert!(state.file_picker_open);
    assert_eq!(state.input, "attach @");
    assert_eq!(
        state.file_picker_phase,
        crate::attachments::PickerPhase::Indexing
    );
    assert!(!state.file_picker_index_ready);
    assert!(matches!(
        state.pending_input_action.take(),
        Some(crate::tui::commands::InputAction::RefreshFilePicker)
    ));

    let generation = state.file_picker_generation;
    press(&mut state, KeyCode::Char('m'));
    assert_eq!(state.file_picker_filter, "m");
    assert_eq!(state.file_picker_generation, generation + 1);
    assert_eq!(
        state.file_picker_phase,
        crate::attachments::PickerPhase::Matching
    );
    assert!(matches!(
        state.pending_input_action.take(),
        Some(crate::tui::commands::InputAction::RefreshFilePicker)
    ));

    assert!(handle_event(
        &mut state,
        &Event::Paste("ain\u{1b}[31m".into()),
        "/tmp/ca-uni004-test",
    ));
    assert_eq!(state.file_picker_filter, "main");
    assert_eq!(state.input, "attach @main");
}

#[test]
fn attachment_picker_inserts_typed_file_ranges_and_rejects_mcp_ranges() {
    let mut state = state_with_input("see @main#12-40", "see @main#12-40".len());
    state.file_picker_open = true;
    state.file_picker_phase = crate::attachments::PickerPhase::Ready;
    state.file_picker_filter = "main#12-40".into();
    state.file_picker_entries = vec![crate::attachments::PickerEntry::File(
        crate::attachments::IndexedFile {
            root_id: "root".into(),
            root_label: "workspace".into(),
            relative: "src/main.rs".into(),
            reference: "src/main.rs".into(),
            read_only: false,
        },
    )];
    press(&mut state, KeyCode::Enter);
    assert_eq!(state.input, "see @src/main.rs#12-40 ");
    assert!(matches!(
        state.pending_input_action.take(),
        Some(crate::tui::commands::InputAction::RecordAttachmentFrecency(
            _
        ))
    ));
    assert!(matches!(
        &state.composer_attachments[0].kind,
        crate::attachments::ComposerAttachmentKind::File { relative, .. }
            if relative == "src/main.rs"
    ));

    state.file_picker_open = true;
    state.file_picker_filter = "resource#2".into();
    state.file_picker_entries = vec![crate::attachments::PickerEntry::Mcp(
        crate::mcp::McpResourceInfo {
            server: "docs".into(),
            uri: "docs://guide".into(),
            name: "Guide".into(),
            description: None,
            mime_type: Some("text/plain".into()),
        },
    )];
    state.selected_file_idx = 0;
    let before = state.input.clone();
    press(&mut state, KeyCode::Enter);
    assert_eq!(state.input, before);
    assert_eq!(
        state.file_picker_error.as_deref(),
        Some("line ranges apply only to workspace files")
    );
}

fn prompt_stash_picker(writes_enabled: bool) -> crate::app::PromptStashPickerState {
    crate::app::PromptStashPickerState {
        entries: vec![
            crate::prompt_stash::PromptDraft {
                id: uuid::Uuid::parse_str("00000000-0000-0000-0000-000000000131").unwrap(),
                text: "newest draft".into(),
                created_at: "2026-08-16T12:00:00Z".parse().unwrap(),
            },
            crate::prompt_stash::PromptDraft {
                id: uuid::Uuid::parse_str("00000000-0000-0000-0000-000000000132").unwrap(),
                text: "older\nmultiline draft".into(),
                created_at: "2026-08-15T12:00:00Z".parse().unwrap(),
            },
        ],
        selected_idx: 0,
        snapshot_at: "2026-08-16T12:05:00Z".parse().unwrap(),
        busy: false,
        writes_enabled,
        delete_confirmation: None,
        notice: None,
        error: None,
    }
}

#[test]
fn prompt_stash_chord_and_picker_emit_explicit_actions_without_leaking_paste() {
    let mut state = state_with_input("draft @src/main.rs#12", 21);
    press_ctrl(&mut state, 'x');
    press(&mut state, KeyCode::Char('s'));
    assert_eq!(
        state.pending_input_action.take(),
        Some(crate::tui::commands::InputAction::SavePromptDraft)
    );
    assert_eq!(state.input, "draft @src/main.rs#12");

    state.prompt_stash = Some(prompt_stash_picker(true));
    assert!(handle_event(
        &mut state,
        &Event::Paste("must not enter composer".into()),
        "/tmp/ca-uni004-test",
    ));
    assert_eq!(state.input, "draft @src/main.rs#12");
    press(&mut state, KeyCode::Down);
    let selected = state.prompt_stash.as_ref().unwrap().selected().unwrap().id;
    press(&mut state, KeyCode::Char('d'));
    assert!(state.pending_input_action.is_none());
    assert_eq!(
        state.prompt_stash.as_ref().unwrap().delete_confirmation,
        Some(selected)
    );
    press(&mut state, KeyCode::Char('d'));
    assert_eq!(
        state.pending_input_action.take(),
        Some(crate::tui::commands::InputAction::DeletePromptDraft(
            selected
        ))
    );
    assert!(state.prompt_stash.as_ref().unwrap().busy);

    state.prompt_stash = Some(prompt_stash_picker(true));
    let restore = state.prompt_stash.as_ref().unwrap().selected().unwrap().id;
    press(&mut state, KeyCode::Enter);
    assert_eq!(
        state.pending_input_action.take(),
        Some(crate::tui::commands::InputAction::RestorePromptDraft(
            restore
        ))
    );
}

#[test]
fn prompt_stash_delete_confirmation_resets_and_bare_picker_is_read_only() {
    let mut state = state_with_input("preserved", 9);
    state.prompt_stash = Some(prompt_stash_picker(true));
    press(&mut state, KeyCode::Char('d'));
    press(&mut state, KeyCode::Down);
    assert!(state
        .prompt_stash
        .as_ref()
        .unwrap()
        .delete_confirmation
        .is_none());

    state.prompt_stash = Some(prompt_stash_picker(false));
    press(&mut state, KeyCode::Enter);
    assert!(state.pending_input_action.is_none());
    assert!(state
        .prompt_stash
        .as_ref()
        .unwrap()
        .error
        .as_deref()
        .unwrap()
        .contains("bare mode"));
    assert_eq!(state.input, "preserved");
}

#[test]
fn dashboard_ctrl_backslash_requests_the_command_and_esc_closes() {
    let mut state = state_with_input("", 0);
    press_ctrl(&mut state, '\\');
    assert_eq!(
        state.pending_input_action,
        Some(crate::tui::commands::InputAction::Command(
            crate::tui::commands::CommandAction {
                id: crate::tui::commands::CommandId::Dashboard,
                args: String::new(),
            }
        ))
    );

    state.pending_input_action = None;
    crate::tui::dashboard::open(&mut state);
    press(&mut state, KeyCode::Esc);
    assert!(!state.dashboard.open);
}

#[test]
fn dashboard_enter_on_saved_row_loads_the_session() {
    let mut state = state_with_input("", 0);
    state.session_list = vec![crate::session::SessionSummary {
        storage_key: "saved".into(),
        id: uuid::Uuid::from_u128(9),
        title: "Saved work".into(),
        pinned: false,
        workspace: "/workspace/ca".into(),
        updated_at: chrono::Utc::now(),
    }];
    crate::tui::dashboard::open(&mut state);
    state.dashboard.collapsed_inactive = false;
    state.dashboard.cursor = crate::tui::dashboard::DashboardCursor::Row {
        group: crate::tui::dashboard::DashboardGroup::Inactive,
        index: 0,
    };
    press(&mut state, KeyCode::Enter);
    assert!(!state.dashboard.open);
    assert_eq!(
        state.pending_input_action,
        Some(crate::tui::commands::InputAction::LoadSession(
            "saved".into()
        ))
    );
}

#[test]
fn dashboard_typed_prompt_stays_on_an_empty_session_for_submit() {
    let mut state = state_with_input("", 0);
    crate::tui::dashboard::open(&mut state);
    press(&mut state, KeyCode::Char('f'));
    press(&mut state, KeyCode::Char('i'));
    press(&mut state, KeyCode::Char('x'));
    press(&mut state, KeyCode::Enter);
    assert!(!state.dashboard.open);
    assert_eq!(state.input, "fix");
    assert!(state.pending_composer_submit);
}
