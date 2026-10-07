use crate::app::{
    AppState, Message, ModelInfo, PartialToolCall, PendingApproval, Role, StreamingState, ToolCall,
    ToolStatus,
};
use crate::context::ShellHistory;
use crate::tui::test_support::{render_to_text, test_state};
use ratatui::backend::{Backend, TestBackend};
use ratatui::buffer::Buffer;
use ratatui::layout::Position;
use ratatui::style::{Color, Modifier};
use ratatui::Terminal;

/// Render one state at both the wide and narrow test sizes and join them, so a
/// single snapshot covers both bounded layouts.
fn wide_narrow(wide: String, narrow: String) -> String {
    format!("{wide}\n-- narrow --\n{narrow}")
}

#[test]
fn unavailable_tool_mode_guidance_is_visible_in_narrow_terminals() {
    for mode in [crate::app::AgentMode::Ask, crate::app::AgentMode::Plan] {
        let mut state = test_state();
        state.mode = mode.clone();
        state.status_message = Some(crate::unavailable_tool_failure_message(
            &mode,
            "run_command",
        ));
        for (width, height) in [(100, 30), (60, 24)] {
            let screen = render_to_text(&state, width, height);
            let footer = screen
                .lines()
                .find(|line| line.contains("resend"))
                .expect("mode recovery action remains visible");
            assert!(footer.contains("Tab"), "{screen}");
            assert!(footer.contains("Agent"), "{screen}");
            assert!(!footer.contains("/retry"), "{screen}");
        }
    }
}

fn readable_plan_state() -> AppState {
    let mut state = test_state();
    state.mode = crate::app::AgentMode::Plan;
    let body = "Make completed work visible and keep the next action clear.\n\n## Approach\n\n- Preserve Unicode: café, 界面, and 👩🏽‍💻.\n- Keep each list item separate.\n\nApprove the plan only after reviewing the full implementation and validation details.\n\n## Validation\n\nExercise narrow terminals, session restoration, interrupted execution, and a successful real file edit in a temporary workspace.\n\n## Additional details\n\n".to_string() + &"This is a deliberately long readable paragraph that wraps across several terminal rows without losing any of its text.\n\n".repeat(8);
    state.work_state.lock().unwrap().apply_model_mutation(crate::work::WorkMutation::Propose {
        expected_revision: 0,
        title: "Make plan review readable and execution reliable".into(), body: Some(body),
        tasks: vec![
            "Finish the planning request before starting execution with the current agent and its normal workspace tools".into(),
            "Render long tasks and Unicode cleanly: café, 界面, 👩🏽‍💻".into(),
            "Verify approval, revision, dismissal, saved-state recovery, and failure messages".into(),
            "Run completion checks".into(), "Install the verified release".into(),
        ],
    }).unwrap();
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
fn plan_card_leaves_the_transcript_after_review() {
    for command in ["approve", "dismiss", "revise", "reject"] {
        let mut state = readable_plan_state();
        crate::plan::manage_command(&mut state, command).unwrap();
        for (width, height) in [(100, 30), (60, 24)] {
            let screen = render_to_text(&state, width, height);
            assert!(
                !screen.contains("Make plan review readable"),
                "{command}: {screen}"
            );
            assert!(!screen.contains("Enter Execute"), "{command}: {screen}");
        }
        assert!(state.work_state.lock().unwrap().plan.is_some());
        crate::plan::manage_command(&mut state, "show").unwrap();
        let screen = render_to_text(&state, 100, 30);
        assert!(screen.contains("Make plan review readable"), "{screen}");
        crate::tui::input::handle_event(
            &mut state,
            &crate::tui::event::Event::Key(
                crossterm::event::KeyCode::Esc,
                crossterm::event::KeyModifiers::NONE,
            ),
            "/workspace/ca",
        );
        assert!(state.plan_proposal.is_none());
        assert!(!render_to_text(&state, 100, 30).contains("Make plan review readable"));
    }
}

#[test]
fn plan_card_stays_hidden_for_running_and_restored_work() {
    let mut state = readable_plan_state();
    state.mode = crate::app::AgentMode::Agent;
    state.turn_lifecycle.begin();
    for (width, height) in [(100, 30), (60, 24)] {
        let screen = render_to_text(&state, width, height);
        assert!(!screen.contains("Make plan review readable"), "{screen}");
        assert!(!screen.contains("Enter Execute"), "{screen}");
    }
    insta::assert_snapshot!(
        "plan_hidden_during_execution",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24),
        )
    );
    state.turn_lifecycle.complete();
    state.plan_proposal = None;
    for status in [
        crate::work::PlanStatus::Proposed,
        crate::work::PlanStatus::Approved,
        crate::work::PlanStatus::Rejected,
    ] {
        {
            let mut work = state.work_state.lock().unwrap();
            work.plan.as_mut().unwrap().status = status;
        }
        for (width, height) in [(100, 30), (60, 24)] {
            let screen = render_to_text(&state, width, height);
            assert!(!screen.contains("Make plan review readable"), "{screen}");
        }
    }
}

#[test]
fn plan_card_shows_all_tasks_and_complete_titles_by_default() {
    let mut state = readable_plan_state();
    {
        let mut work = state.work_state.lock().unwrap();
        let plan = work.plan.as_mut().unwrap();
        plan.status = crate::work::PlanStatus::Approved;
        let mut task = plan.tasks[0].clone();
        task.id = crate::work::TaskId(uuid::Uuid::new_v4());
        task.title = "Report the final result".into();
        plan.tasks.push(task);
        plan.tasks[0].title = "Inspect repository history, processing symbols, datasets, and buildability; reproduce the current application baseline, compare reproducible reference outputs, and preserve the complete validation evidence through the final marker".into();
        plan.tasks[0].status = crate::work::TaskStatus::InProgress;
        plan.tasks[3].status = crate::work::TaskStatus::Completed;
        plan.tasks[4].status = crate::work::TaskStatus::Blocked;
    }
    for width in [100, 60] {
        let screen = render_to_text(&state, width, 60);
        let text: String = screen
            .chars()
            .filter(|ch| !ch.is_whitespace() && *ch != '│')
            .collect();
        for expected in [
            "Plan · Blocked",
            "final marker [in progress]",
            "4. Run completion checks [done]",
            "5. Install the verified release [blocked]",
            "6. Report the final result",
        ] {
            let expected: String = expected.chars().filter(|ch| !ch.is_whitespace()).collect();
            assert!(text.contains(&expected), "{screen}");
        }
        assert!(!screen.contains("more tasks"), "{screen}");
    }
    assert!(state.plan_reader.is_none());
    insta::assert_snapshot!(
        "plan_review_all_tasks",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24),
        )
    );
    state.transcript_scroll.oldest();
    let top = render_to_text(&state, 60, 24);
    assert!(top.contains("Plan · Blocked"), "{top}");
    assert!(top.contains("1. Inspect repository history"), "{top}");
}

#[test]
fn plan_review_card_snapshots_keep_actions_and_summary_visible() {
    let state = readable_plan_state();
    let wide = render_to_text(&state, 100, 30);
    let narrow = render_to_text(&state, 60, 24);
    for screen in [&wide, &narrow] {
        assert!(screen.contains("Ready for review"), "{screen}");
        assert!(screen.contains("Enter Execute"), "{screen}");
        assert!(screen.contains("Type to Revise"), "{screen}");
        assert!(screen.contains("4. Run completion checks"), "{screen}");
        assert!(
            screen.contains("5. Install the verified release"),
            "{screen}"
        );
        assert!(!screen.contains("more tasks"));
        assert!(!screen.contains("Grants:"));
        assert!(!screen.contains(&state.plan_proposal.unwrap().plan_id.to_string()));
    }
    insta::assert_snapshot!("plan_review_card", wide_narrow(wide, narrow));
}

#[test]
fn plan_full_reader_snapshots_wrap_scroll_and_report_stale_actions() {
    let mut state = readable_plan_state();
    crate::plan::open_reader(&mut state).unwrap();
    insta::assert_snapshot!(
        "plan_full_reader",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24),
        )
    );
    assert!(state.plan_reader_max_scroll.get() > 10);
    state.pager_scroll = u16::MAX;
    let wide = render_to_text(&state, 100, 30);
    let narrow = render_to_text(&state, 60, 24);
    assert!(narrow.contains("Install the verified release"));
    assert!(narrow.contains("C copy all"));
    insta::assert_snapshot!("plan_full_reader_end", wide_narrow(wide, narrow));
    let revision = state.plan_reader.unwrap().revision;
    state
        .work_state
        .lock()
        .unwrap()
        .apply_model_mutation(crate::work::WorkMutation::Edit {
            expected_revision: revision,
            title: Some("Updated plan".into()),
            body: None,
            decisions: Vec::new(),
        })
        .unwrap();
    insta::assert_snapshot!(
        "plan_full_reader_stale",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24),
        )
    );
}

#[test]
fn plan_start_failure_snapshots_preserve_review_actions() {
    let mut state = readable_plan_state();
    state.status_message =
        Some("Plan save failed: storage unavailable. Execution did not start.".into());
    let wide = render_to_text(&state, 100, 30);
    let narrow = render_to_text(&state, 60, 24);
    assert!(narrow.contains("Plan save failed"));
    assert!(narrow.contains("Enter Execute"));
    insta::assert_snapshot!("plan_start_failure", wide_narrow(wide, narrow));
}

fn rendered_frame(state: &AppState, width: u16, height: u16) -> (Position, Buffer) {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| crate::tui::render(frame, state))
        .unwrap();
    let cursor = terminal.backend_mut().get_cursor_position().unwrap();
    let buffer = terminal.backend().buffer().clone();
    (cursor, buffer)
}

#[test]
fn test_state_uses_in_memory_shell_history() {
    let mut state = AppState::new(ShellHistory::in_memory());
    state.shell_history.push("echo test-only");

    assert_eq!(
        state.shell_history.iter().next().map(String::as_str),
        Some("echo test-only")
    );
}

#[test]
fn startup_renders_without_a_real_terminal() {
    let state = test_state();
    let rendered = render_to_text(&state, 100, 30);

    let visible = rendered
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    assert_eq!(visible.len(), 3, "idle chrome:\n{rendered}");
    assert!(visible[0].starts_with("ca · agent · auto"));
    assert!(visible[1].starts_with("→ "));
    assert_eq!(visible[2], "/ commands");
    for duplicate in [
        "Workspace ·",
        "drop a folder or type a request",
        "Shift+Enter",
        "? shortcuts",
        "0 messages",
    ] {
        assert!(!rendered.contains(duplicate), "unexpected {duplicate:?}");
    }

    let (_, buffer) = rendered_frame(&state, 100, 30);
    for x in 0..100 {
        assert_ne!(buffer[(x, 0)].bg, Color::DarkGray);
    }
}

#[test]
fn new_session_command_and_empty_session_snapshots() {
    let mut state = test_state();
    state.input = "/new".into();
    state.input_cursor = 4;
    state.slash_palette_open = true;
    state.slash_filter = "new".into();
    insta::assert_snapshot!(
        "new_session_command",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
    let _request = crate::prepare_new_session(&mut state).unwrap();
    let generation = state.session_load_generation;
    assert!(crate::apply_new_session_result(
        &mut state,
        Ok(()),
        generation
    ));
    insta::assert_snapshot!(
        "new_session_empty",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

fn pane_state(label: &str) -> AppState {
    let mut state = test_state();
    state.session_name = label.into();
    state.messages.push(Message {
        role: Role::Assistant,
        content: format!("Transcript owned by {label}."),
        tool_calls: None,
        thinking: None,
        timestamp: chrono::DateTime::from_timestamp(0, 0).unwrap(),
        response_items: Vec::new(),
    });
    state
}

#[test]
fn split_pane_layout_snapshots() {
    let mut vertical = pane_state("primary");
    let mut deck = vertical.pane_deck.take().unwrap();
    let second = deck
        .split(
            &vertical,
            crate::panes::SplitAxis::Vertical,
            pane_state("secondary"),
            ratatui::layout::Rect::new(0, 0, 120, 32),
        )
        .unwrap();
    vertical.pane_deck = Some(deck);
    crate::panes::focus(&mut vertical, second).unwrap();
    vertical.input = "Focused draft".into();
    vertical.input_cursor = vertical.input.len();
    insta::assert_snapshot!(
        "split_panes_vertical_wide",
        render_to_text(&vertical, 120, 32)
    );

    let mut horizontal = pane_state("top");
    let mut deck = horizontal.pane_deck.take().unwrap();
    let bottom = deck
        .split(
            &horizontal,
            crate::panes::SplitAxis::Horizontal,
            pane_state("bottom"),
            ratatui::layout::Rect::new(0, 0, 60, 32),
        )
        .unwrap();
    horizontal.pane_deck = Some(deck);
    crate::panes::focus(&mut horizontal, bottom).unwrap();
    insta::assert_snapshot!(
        "split_panes_horizontal_narrow",
        render_to_text(&horizontal, 60, 32)
    );
}

#[test]
fn four_pane_nested_layout_snapshot() {
    let mut state = pane_state("one");
    let area = ratatui::layout::Rect::new(0, 0, 144, 48);
    for (axis, name) in [
        (crate::panes::SplitAxis::Vertical, "two"),
        (crate::panes::SplitAxis::Horizontal, "three"),
        (crate::panes::SplitAxis::Horizontal, "four"),
    ] {
        let mut deck = state.pane_deck.take().unwrap();
        let added = deck.split(&state, axis, pane_state(name), area).unwrap();
        state.pane_deck = Some(deck);
        crate::panes::focus(&mut state, added).unwrap();
    }
    state.interaction = Some(crate::interaction::InteractionState::host(
        crate::interaction::InteractionRequest::host_masked("Pane-local approval", None),
    ));
    insta::assert_snapshot!(
        "split_panes_four_nested_wide",
        render_to_text(&state, 144, 48)
    );
}

#[test]
fn osc8_links_are_sanitized_without_consuming_layout_cells() {
    let root = tempfile::tempdir().unwrap();
    let workspace = crate::workspace::WorkspaceContext::from_roots(root.path(), &[]).unwrap();
    let area = ratatui::layout::Rect::new(0, 0, 12, 1);
    let mut buffer = ratatui::buffer::Buffer::empty(area);
    for (index, symbol) in ["s", "a", "f", "e"].into_iter().enumerate() {
        buffer[(index as u16, 0)].set_symbol(symbol);
    }
    super::render::apply_hyperlinks(
        &mut buffer,
        area,
        &[
            crate::markdown::MarkdownLink {
                label: "safe".into(),
                destination: "https://example.test/path".into(),
            },
            crate::markdown::MarkdownLink {
                label: "other".into(),
                destination: "javascript:alert(1)".into(),
            },
        ],
        &workspace,
    );
    assert!(buffer[(0, 0)]
        .symbol()
        .starts_with("\x1b]8;;https://example.test/path"));
    assert_eq!(buffer.area, area);
    assert_eq!(buffer[(4, 0)].symbol(), " ");
}

fn assert_lrn179_pager_snapshots(name: &str, content: &str) {
    let mut state = test_state();
    state.pager_open = true;
    state.pager_content = content.into();
    insta::assert_snapshot!(
        format!("{name}"),
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

#[test]
fn runtime_log_viewer_snapshots_are_bounded_and_readable() {
    assert_lrn179_pager_snapshots(
        "runtime_logs",
        "Runtime logs\n\n2026-08-28T12:00:00+00:00 info  process    run_started run=00000000-0000-0000-0000-000000000001\n2026-08-28T12:00:02+00:00 warn  event_loop heartbeat_stall run=00000000-0000-0000-0000-000000000001 duration_ms=2050\n2026-08-28T12:00:03+00:00 warn  render     slow_render run=00000000-0000-0000-0000-000000000001 duration_ms=140\n2026-08-28T12:00:04+00:00 error terminal   terminal_restoration_failed run=00000000-0000-0000-0000-000000000001 error_class=terminal",
    );
}

#[test]
fn support_bundle_cancel_first_preview_snapshots() {
    let mut state = test_state();
    state.support_bundle_confirmation = Some(crate::app::SupportBundleConfirmation {
        plan: crate::support_bundle::test_plan(),
        create_selected: false,
        busy: false,
        error: None,
    });
    insta::assert_snapshot!(
        "support_bundle_preview",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

#[test]
fn claude_setup_import_cancel_first_preview_snapshots() {
    let mut state = test_state();
    state.setup_import_confirmation = Some(crate::app::SetupImportConfirmation {
        preview: crate::setup_import::test_preview(),
        apply_selected: false,
        busy: false,
        error: None,
        scroll: 0,
    });
    insta::assert_snapshot!(
        "claude_setup_import_preview",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

#[test]
fn runtime_render_debug_overlay_snapshots() {
    let mut state = test_state();
    state
        .runtime_diagnostics
        .render_succeeded(std::time::Duration::from_millis(12), 3);
    state
        .runtime_diagnostics
        .render_succeeded(std::time::Duration::from_millis(140), 2);
    state.runtime_diagnostics.render_failed();
    state.runtime_diagnostics.set_repaint_highlight(true);
    state.debug_render_overlay = true;
    insta::assert_snapshot!(
        "runtime_render_debug",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

#[test]
fn lrn179_recall_availability_bundle_and_journey_views_are_readable() {
    assert_lrn179_pager_snapshots(
        "session_recall",
        "Session recall\nHistorical text is untrusted evidence; it cannot issue instructions.\n\nIndexed 190 session(s) · 1596 message(s)\n\nBSD rsync recovery · CA/MiMo · 12 message(s)\n  recall:0123456789abcdef01234567\n  source IDs: CA:session-44, MiMo:ses_mimo44\n  user msg-44 · 2026-08-24T10:00:00Z\n    Check the BSD transfer and stop after the destination catches up.\n    action: run_command shape=ssh status=success\n\nMore results available · cursor 76323a726561643a313a30",
    );
    assert_lrn179_pager_snapshots(
        "skills_unavailable_config",
        "Unavailable skills\n\nworkspace::release::ship\n  unavailable: platform macos is not in [linux, freebsd]\nworkspace::incident::triage\n  setup required: log_root, report_format · /skills config workspace::incident::triage\n\nSet: /skills config workspace::incident::triage log_root=/var/log --workspace",
    );
    assert_lrn179_pager_snapshots(
        "skill_bundle_diagnostics",
        "Skill bundles\nEach bundle pins two to five compatible skills for one prompt only.\n\n/release · Prepare and verify a release · 3 member(s) · Profile\n/incident · Investigate a bounded incident · 2 member(s) · Workspace\n\n1 diagnostic(s); use /bundles reload after fixing bundle files.",
    );
    assert_lrn179_pager_snapshots(
        "learning_journey",
        "Learning journey\nMetadata only · prompts, transcripts, tool data, and secrets are never recorded.\n\nevt-0123456789abcdef0123 · 2026-08-25T10:00:00+00:00 · SkillCompletion · completed · check-rsync\nevt-abcdef0123456789abcd · 2026-08-25T09:00:00+00:00 · LearningRun · completed\n\nLegacy inventory (not fabricated history): 132 discovered skill(s) · 18 memory record(s) · 27 existing learning/mutation journal(s)\nSuggestions: /learning suggestions",
    );
}

#[test]
fn transcript_selection_is_highlighted_without_changing_rendered_text() {
    let mut state = test_state();
    state.messages.push(Message {
        role: Role::Assistant,
        content: "selectable text".into(),
        tool_calls: None,
        thinking: None,
        timestamp: chrono::DateTime::from_timestamp(0, 0).unwrap(),
        response_items: Vec::new(),
    });

    let (_, initial) = rendered_frame(&state, 60, 24);
    assert_eq!(initial[(0, 1)].symbol(), "s");
    assert!(state.transcript_scroll.begin_selection(0, 1));
    assert!(state.transcript_scroll.update_selection(5, 1));

    let (_, selected) = rendered_frame(&state, 60, 24);
    assert_eq!(selected[(0, 1)].symbol(), "s");
    for x in 0..=5 {
        assert!(selected[(x, 1)].modifier.contains(Modifier::REVERSED));
    }
    assert!(!selected[(6, 1)].modifier.contains(Modifier::REVERSED));
}

#[test]
fn pager_selection_is_highlighted_without_exposing_hidden_conversation() {
    let mut state = test_state();
    state.messages.push(Message {
        role: Role::Assistant,
        content: "hidden conversation".into(),
        tool_calls: None,
        thinking: None,
        timestamp: chrono::DateTime::from_timestamp(0, 0).unwrap(),
        response_items: Vec::new(),
    });
    state.pager_content = "pager evidence".into();
    state.pager_open = true;

    let (_, initial) = rendered_frame(&state, 60, 24);
    assert_eq!(initial[(1, 1)].symbol(), "p");
    assert!(state.pager_selection.begin_selection(1, 1));
    assert!(state.pager_selection.update_selection(5, 1));

    let (_, selected) = rendered_frame(&state, 60, 24);
    assert_eq!(selected[(1, 1)].symbol(), "p");
    for x in 1..=5 {
        assert!(selected[(x, 1)].modifier.contains(Modifier::REVERSED));
    }
    assert!(!selected[(6, 1)].modifier.contains(Modifier::REVERSED));
    assert!(!render_to_text(&state, 60, 24).contains("hidden conversation"));
}

#[test]
fn composer_cursor_blinks_white_then_restores_the_underlying_cell() {
    let empty = test_state();
    let (native_cursor, buffer) = rendered_frame(&empty, 100, 30);
    assert_eq!(native_cursor, Position::ORIGIN);
    assert_eq!(buffer[(3, 27)].symbol(), "A");
    assert_eq!(buffer[(3, 27)].fg, Color::Black);
    assert_eq!(buffer[(3, 27)].bg, Color::White);

    let mut ascii = test_state();
    ascii.input = "over".into();
    ascii.input_cursor = ascii.input.len();
    let (native_cursor, buffer) = rendered_frame(&ascii, 100, 30);
    assert_eq!(native_cursor, Position::ORIGIN);
    assert_eq!(buffer[(7, 27)].symbol(), " ");
    assert_eq!(buffer[(7, 27)].fg, Color::Black);
    assert_eq!(buffer[(7, 27)].bg, Color::White);

    ascii.cursor_blink_visible = false;
    let (native_cursor, buffer) = rendered_frame(&ascii, 100, 30);
    assert_eq!(native_cursor, Position::ORIGIN);
    assert_eq!(buffer[(7, 27)].symbol(), " ");
    assert_ne!(buffer[(7, 27)].bg, Color::White);

    let mut unicode = test_state();
    unicode.input = "A🙂界".into();
    unicode.input_cursor = "A🙂".len();
    let (native_cursor, buffer) = rendered_frame(&unicode, 100, 30);
    assert_eq!(native_cursor, Position::ORIGIN);
    assert_eq!(buffer[(6, 27)].symbol(), "界");
    assert_eq!(buffer[(6, 27)].fg, Color::Black);
    assert_eq!(buffer[(6, 27)].bg, Color::White);

    let mut overlay = test_state();
    overlay.model_picker_open = true;
    let (native_cursor, buffer) = rendered_frame(&overlay, 100, 30);
    assert_eq!(native_cursor, Position::ORIGIN);
    assert_ne!(buffer[(3, 27)].bg, Color::White);
}

#[test]
fn cursor_blink_uses_a_stable_half_second_phase_and_resets_after_input() {
    let mut state = test_state();
    for _ in 0..9 {
        state.advance_cursor_blink();
        assert!(state.cursor_blink_visible);
    }
    state.advance_cursor_blink();
    assert!(!state.cursor_blink_visible);

    state.reset_cursor_blink();
    assert!(state.cursor_blink_visible);
    assert_eq!(state.cursor_blink_ticks, 0);
}

fn streaming_jump_regression_state() -> AppState {
    let mut state = test_state();
    state.streaming = Some(StreamingState {
        content: "The repository already has most of the safe async TUI machinery from LRN-168, so I’ll extend that rather than create a second path. The working tree is heavily pre-existing dirty; I’ll constrain edits to the learning/TUI orchestration, focused tests/snapshots, and required plan/harness files, and won’t touch unrelated changes.".into(),
        tool_calls: Vec::new(),
        thinking: None,
        response_items: Vec::new(),
        last_usage: None,
        usage_handle: crate::usage::UsageHandle::test_handle(),
        reasoning_started_at: None,
        reasoning_duration: None,
        generation_started_at: None,
    });
    state.turn_lifecycle.begin();
    state
}

fn assert_streaming_blink_preserves_geometry(state: &mut AppState, width: u16, height: u16) {
    state.cursor_blink_visible = true;
    let (_, visible) = rendered_frame(state, width, height);
    let visible_offset = state.transcript_scroll.bottom_offset();
    let visible_cursor_cells = (0..height)
        .flat_map(|y| (0..width).map(move |x| (x, y)))
        .filter(|&(x, y)| visible[(x, y)].symbol() == "▌")
        .count();
    assert!(
        visible_cursor_cells <= 1,
        "streaming response must have at most one visible cursor cell"
    );

    for tick in 0..24 {
        state.cursor_blink_visible = tick % 2 != 0;
        let (_, blink) = rendered_frame(state, width, height);
        assert_eq!(
            state.transcript_scroll.bottom_offset(),
            visible_offset,
            "blink tick {tick} changed the bottom-relative transcript offset"
        );
        for y in 0..height {
            for x in 0..width {
                assert_eq!(
                    visible[(x, y)].symbol(),
                    blink[(x, y)].symbol(),
                    "blink tick {tick} moved text at ({x}, {y})"
                );
            }
        }
        let blink_cursor_cells = (0..height)
            .flat_map(|y| (0..width).map(move |x| (x, y)))
            .filter(|&(x, y)| blink[(x, y)].symbol() == "▌")
            .count();
        assert_eq!(blink_cursor_cells, visible_cursor_cells);
    }
}

#[test]
fn streaming_cursor_blink_never_moves_screenshot_response() {
    for (width, height) in [(190, 24), (100, 24), (52, 20)] {
        let mut state = streaming_jump_regression_state();
        assert_streaming_blink_preserves_geometry(&mut state, width, height);
    }
}

#[test]
fn streaming_cursor_blink_preserves_scrolled_up_anchor_across_reflow() {
    for (width, height) in [(100, 12), (48, 12), (72, 18)] {
        let mut state = streaming_jump_regression_state();
        state.messages = (0..12)
            .map(|index| Message {
                role: Role::Assistant,
                content: format!(
                    "Saved transcript row {index}: enough stable text to wrap on narrow terminals."
                ),
                tool_calls: None,
                thinking: None,
                timestamp: fixed_time(),
                response_items: Vec::new(),
            })
            .collect();
        let _ = rendered_frame(&state, width, height);
        state.transcript_scroll.page_up(5);
        assert!(state.transcript_scroll.bottom_offset() > 0);
        assert_streaming_blink_preserves_geometry(&mut state, width, height);
    }
}

#[test]
fn streaming_new_deltas_follow_bottom_or_preserve_scrolled_visible_rows() {
    let mut following = streaming_jump_regression_state();
    let _ = rendered_frame(&following, 72, 16);
    following
        .streaming
        .as_mut()
        .unwrap()
        .content
        .push_str(" A newly arrived provider delta extends the final line.");
    let _ = rendered_frame(&following, 72, 16);
    assert!(following.transcript_scroll.following());
    assert_eq!(following.transcript_scroll.bottom_offset(), 0);

    let mut scrolled = streaming_jump_regression_state();
    scrolled.messages = (0..16)
        .map(|index| Message {
            role: Role::Assistant,
            content: format!("Anchored saved row {index} with deterministic wrapped content."),
            tool_calls: None,
            thinking: None,
            timestamp: fixed_time(),
            response_items: Vec::new(),
        })
        .collect();
    let _ = rendered_frame(&scrolled, 64, 14);
    scrolled.transcript_scroll.page_up(7);
    let (_, before) = rendered_frame(&scrolled, 64, 14);
    scrolled
        .streaming
        .as_mut()
        .unwrap()
        .content
        .push_str(" **Markdown** and `code` arrive below the viewport.");
    let (_, after) = rendered_frame(&scrolled, 64, 14);
    for y in 1..10 {
        for x in 0..64 {
            assert_eq!(
                before[(x, y)].symbol(),
                after[(x, y)].symbol(),
                "new tail delta moved scrolled transcript text at ({x}, {y})"
            );
        }
    }
    assert!(scrolled.transcript_scroll.bottom_offset() > 7);
    assert_streaming_blink_preserves_geometry(&mut scrolled, 64, 14);
}

#[test]
fn streaming_markdown_cursor_geometry_survives_terminal_reflow() {
    let mut state = streaming_jump_regression_state();
    state.streaming.as_mut().unwrap().content.push_str(
        "\n\n## Next step\n\nKeep **bold text**, `inline code`, and [links](https://example.invalid) stable.",
    );
    for (width, height) in [(120, 22), (46, 18), (88, 20)] {
        assert_streaming_blink_preserves_geometry(&mut state, width, height);
    }
}

#[test]
fn composer_white_cursor_tracks_hard_lines_soft_wraps_and_exact_width_wraps() {
    let mut hard_line = test_state();
    hard_line.input = "one\ntwo".into();
    hard_line.input_cursor = hard_line.input.len();
    let (native_cursor, buffer) = rendered_frame(&hard_line, 100, 30);
    assert_eq!(native_cursor, Position::ORIGIN);
    assert_eq!(buffer[(6, 27)].bg, Color::White);

    // At width 20, the first visual row has sixteen cells after the border and prompt.
    let mut exact_width = test_state();
    exact_width.input = "1234567890123456".into();
    exact_width.input_cursor = exact_width.input.len();
    let (native_cursor, buffer) = rendered_frame(&exact_width, 20, 30);
    assert_eq!(native_cursor, Position::ORIGIN);
    assert_eq!(buffer[(3, 27)].bg, Color::White);

    let mut soft_wrap = exact_width;
    soft_wrap.input.push('x');
    soft_wrap.input_cursor = soft_wrap.input.len();
    let (native_cursor, buffer) = rendered_frame(&soft_wrap, 20, 30);
    assert_eq!(native_cursor, Position::ORIGIN);
    assert_eq!(buffer[(4, 27)].bg, Color::White);
}

#[test]
fn rendering_changes_with_terminal_dimensions() {
    let state = test_state();
    let wide = render_to_text(&state, 100, 30);
    let narrow = render_to_text(&state, 60, 24);

    assert_ne!(wide, narrow);
    assert!(wide.contains("/ commands"));
    assert!(narrow.contains("/ commands"));
}

#[test]
fn startup_snapshots() {
    let state = test_state();
    insta::assert_snapshot!(
        "startup",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

fn soft_paused_state(boundary: crate::activity::SoftPauseBoundary) -> AppState {
    let mut state = test_state();
    state.turn_lifecycle.begin();
    state
        .turn_lifecycle
        .set_phase(crate::activity::ActivityPhase::Paused);
    state.soft_pause = crate::activity::SoftPauseState::Paused(boundary);
    state.status_message = Some(match boundary {
        crate::activity::SoftPauseBoundary::Tools => {
            "Paused before tools · /unpause · Ctrl+C cancel".into()
        }
        crate::activity::SoftPauseBoundary::Provider => {
            "Paused before provider · /unpause · Ctrl+C cancel".into()
        }
        crate::activity::SoftPauseBoundary::Finalization => {
            "Paused after provider · /unpause · Ctrl+C cancel".into()
        }
    });
    state.messages = vec![Message {
        role: Role::Assistant,
        content: "I retained the completed boundary.".into(),
        tool_calls: None,
        thinking: None,
        timestamp: fixed_time(),
        response_items: Vec::new(),
    }];
    state
}

#[test]
fn soft_pause_boundary_snapshots() {
    for (name, boundary) in [
        (
            "soft_pause_before_tools",
            crate::activity::SoftPauseBoundary::Tools,
        ),
        (
            "soft_pause_before_provider",
            crate::activity::SoftPauseBoundary::Provider,
        ),
        (
            "soft_pause_after_provider",
            crate::activity::SoftPauseBoundary::Finalization,
        ),
    ] {
        let state = soft_paused_state(boundary);
        insta::assert_snapshot!(
            format!("{name}"),
            wide_narrow(
                render_to_text(&state, 100, 30),
                render_to_text(&state, 60, 24)
            )
        );
    }
}

fn loop_guard_render_state(cancel_confirmation: bool) -> AppState {
    let mut state = soft_paused_state(crate::activity::SoftPauseBoundary::Provider);
    let mut interaction =
        crate::loop_guard::LoopGuardInteraction::new(crate::loop_guard::LoopIncident {
            kind: crate::loop_guard::LoopIncidentKind::RepeatedFailedCommand,
            evidence:
                "Failing command `cargo test --all-targets` returned unchanged output 3 times"
                    .into(),
            occurrences: 3,
        });
    interaction.cancel_confirmation = cancel_confirmation;
    state.loop_guard_interaction = Some(interaction);
    state.status_message =
        Some("Low-yield loop paused · choose Replan, Continue, or Cancel".into());
    state
}

#[test]
fn loop_guard_dialog_snapshots() {
    let state = loop_guard_render_state(false);
    insta::assert_snapshot!(
        "loop_guard",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );

    let confirm = loop_guard_render_state(true);
    insta::assert_snapshot!(
        "loop_guard_cancel",
        wide_narrow(
            render_to_text(&confirm, 100, 30),
            render_to_text(&confirm, 60, 24)
        )
    );
}

fn btw_render_state(status: crate::app::BtwStatus) -> AppState {
    let mut state = test_state();
    state.btw = Some(crate::app::BtwState {
        question: "Why is the current request taking a while?".into(),
        answer: "The provider is still working. Your main turn continues independently, and this side answer is not saved.".into(),
        status,
        provider: "mimo".into(),
        model: "mimo-v2.5-pro".into(),
        cursor: 12,
        scroll: 0,
        generation: 1,
    });
    state
}

#[test]
fn btw_editing_running_and_complete_snapshots() {
    for (name, status) in [
        ("btw_editing", crate::app::BtwStatus::Editing),
        ("btw_running", crate::app::BtwStatus::Running),
        ("btw_complete", crate::app::BtwStatus::Complete),
    ] {
        let state = btw_render_state(status);
        insta::assert_snapshot!(
            format!("{name}"),
            wide_narrow(
                render_to_text(&state, 100, 30),
                render_to_text(&state, 60, 24)
            )
        );
    }
}

#[test]
fn btw_failure_and_cancelled_snapshots() {
    let failed = btw_render_state(crate::app::BtwStatus::Failed(
        "Authentication rejected for mimo (HTTP 401). Run /provider setup mimo".into(),
    ));
    insta::assert_snapshot!(
        "btw_failure",
        wide_narrow(
            render_to_text(&failed, 100, 30),
            render_to_text(&failed, 60, 24)
        )
    );

    let mut cancelled = btw_render_state(crate::app::BtwStatus::Cancelled);
    cancelled.btw.as_mut().unwrap().answer.clear();
    insta::assert_snapshot!(
        "btw_cancelled",
        wide_narrow(
            render_to_text(&cancelled, 100, 30),
            render_to_text(&cancelled, 60, 24)
        )
    );
}

fn voice_render_state(phase: crate::voice::VoicePhase) -> AppState {
    let mut state = test_state();
    state.input = "Existing editable draft".into();
    state.input_cursor = state.input.len();
    state.voice = Some(crate::voice::VoiceState {
        phase,
        generation: 1,
        error: None,
    });
    state
}

#[test]
fn voice_ready_recording_and_transcribing_snapshots() {
    for (name, phase) in [
        ("voice_ready", crate::voice::VoicePhase::Ready),
        ("voice_recording", crate::voice::VoicePhase::Recording),
        ("voice_transcribing", crate::voice::VoicePhase::Transcribing),
    ] {
        let state = voice_render_state(phase);
        insta::assert_snapshot!(
            format!("{name}"),
            wide_narrow(
                render_to_text(&state, 100, 30),
                render_to_text(&state, 60, 24)
            )
        );
    }
}

#[test]
fn voice_failure_snapshots_are_safe() {
    let mut state = voice_render_state(crate::voice::VoicePhase::Failed);
    state.voice.as_mut().unwrap().error =
        Some("Authentication rejected by the transcription provider (HTTP 401)".into());
    insta::assert_snapshot!(
        "voice_failure",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

fn keymap_render_state() -> AppState {
    let mut state = test_state();
    state.keymap_hub = Some(crate::app::KeymapHubState {
        filter: String::new(),
        selected_idx: 0,
        capture: None,
        saving: false,
        writes_enabled: true,
        error: None,
        notice: Some("Saved Switch model".into()),
    });
    state
        .keymap_customized
        .insert(crate::keymap::KeyAction::ModelPicker);
    state
}

#[test]
fn keymap_manager_and_capture_snapshots() {
    let state = keymap_render_state();
    insta::assert_snapshot!(
        "keymap_manager",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );

    let mut capture = keymap_render_state();
    capture.keymap_hub.as_mut().unwrap().capture = Some(crate::app::KeymapCaptureState {
        action: crate::keymap::KeyAction::ModelPicker,
        first: Some(crate::keymap::KeyStroke::from_event(
            crossterm::event::KeyCode::Char('x'),
            crossterm::event::KeyModifiers::CONTROL,
        )),
    });
    capture.keymap_hub.as_mut().unwrap().notice = None;
    insta::assert_snapshot!(
        "keymap_capture",
        wide_narrow(
            render_to_text(&capture, 100, 30),
            render_to_text(&capture, 60, 24)
        )
    );
}

#[test]
fn keymap_bare_and_error_snapshots() {
    let mut bare = keymap_render_state();
    bare.active_profile = crate::runtime::ActiveProfile::Bare;
    bare.run_capabilities = crate::runtime::RunCapabilities::for_profile(&bare.active_profile);
    let hub = bare.keymap_hub.as_mut().unwrap();
    hub.writes_enabled = false;
    hub.notice = None;
    insta::assert_snapshot!(
        "keymap_bare",
        wide_narrow(
            render_to_text(&bare, 100, 30),
            render_to_text(&bare, 60, 24)
        )
    );

    let mut failed = keymap_render_state();
    let hub = failed.keymap_hub.as_mut().unwrap();
    hub.notice = None;
    hub.error = Some("Could not save keymap: simulated filesystem failure".into());
    insta::assert_snapshot!(
        "keymap_error",
        wide_narrow(
            render_to_text(&failed, 100, 30),
            render_to_text(&failed, 60, 24)
        )
    );
}

#[test]
fn history_search_populated_and_empty_snapshots() {
    let mut state = test_state();
    state.input_history = vec![
        "cargo test keymap".into(),
        "explain the provider failure\nwithout retrying".into(),
        "cargo test keymap".into(),
    ];
    state.history_search = Some(crate::app::HistorySearchState {
        filter: "cargo".into(),
        selected_idx: 1,
    });
    insta::assert_snapshot!(
        "history_search",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );

    state.history_search.as_mut().unwrap().filter = "no-match".into();
    state.history_search.as_mut().unwrap().selected_idx = 0;
    insta::assert_snapshot!(
        "history_search_empty",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

fn hook_manager_render_state() -> AppState {
    let mut state = test_state();
    state.hook_manager = Some(crate::app::HookManagerState {
        filter: String::new(),
        selected_idx: 0,
        entries: vec![
            crate::hooks::HookManagementEntry {
                index: 0,
                event: crate::hooks::HookEvent::SessionStart,
                enabled: true,
                timeout_secs: 5,
                fail_closed: true,
                last_result: Some(crate::hooks::HookSafeResult {
                    status: crate::hooks::HookSafeStatus::Succeeded,
                    duration_ms: 42,
                }),
            },
            crate::hooks::HookManagementEntry {
                index: 1,
                event: crate::hooks::HookEvent::PreToolUse,
                enabled: false,
                timeout_secs: 12,
                fail_closed: false,
                last_result: None,
            },
            crate::hooks::HookManagementEntry {
                index: 2,
                event: crate::hooks::HookEvent::AfterAgentResponse,
                enabled: true,
                timeout_secs: 8,
                fail_closed: false,
                last_result: Some(crate::hooks::HookSafeResult {
                    status: crate::hooks::HookSafeStatus::Denied,
                    duration_ms: 17,
                }),
            },
        ],
        network_enforcement: "network denied by OS sandbox".into(),
        config_path: "/tmp/ca-test/config.toml".into(),
        busy: false,
        testing: false,
        writes_enabled: true,
        execution_enabled: true,
        error: None,
        notice: Some("session_start enabled".into()),
    });
    state
}

#[test]
fn hook_manager_populated_and_empty_snapshots() {
    let populated = hook_manager_render_state();
    insta::assert_snapshot!(
        "hook_manager",
        wide_narrow(
            render_to_text(&populated, 100, 30),
            render_to_text(&populated, 60, 24)
        )
    );

    let mut empty = hook_manager_render_state();
    empty.hook_manager.as_mut().unwrap().entries.clear();
    empty.hook_manager.as_mut().unwrap().notice = None;
    insta::assert_snapshot!(
        "hook_manager_empty",
        wide_narrow(
            render_to_text(&empty, 100, 30),
            render_to_text(&empty, 60, 24)
        )
    );

    let mut no_match = hook_manager_render_state();
    no_match.hook_manager.as_mut().unwrap().filter = "missing".into();
    no_match.hook_manager.as_mut().unwrap().notice = None;
    insta::assert_snapshot!(
        "hook_manager_no_match",
        wide_narrow(
            render_to_text(&no_match, 100, 30),
            render_to_text(&no_match, 60, 24)
        )
    );
}

#[test]
fn hook_manager_busy_failure_and_bare_snapshots() {
    let mut busy = hook_manager_render_state();
    busy.hook_manager.as_mut().unwrap().busy = true;
    busy.hook_manager.as_mut().unwrap().testing = true;
    busy.hook_manager.as_mut().unwrap().notice = None;
    insta::assert_snapshot!(
        "hook_manager_busy",
        wide_narrow(
            render_to_text(&busy, 100, 30),
            render_to_text(&busy, 60, 24)
        )
    );

    let mut failed = hook_manager_render_state();
    failed.hook_manager.as_mut().unwrap().notice = None;
    failed.hook_manager.as_mut().unwrap().error =
        Some("Could not reload hooks; fix config.toml and press Shift+R".into());
    insta::assert_snapshot!(
        "hook_manager_failure",
        wide_narrow(
            render_to_text(&failed, 100, 30),
            render_to_text(&failed, 60, 24)
        )
    );

    let mut cancelled = hook_manager_render_state();
    cancelled.hook_manager.as_mut().unwrap().notice = None;
    cancelled.hook_manager.as_mut().unwrap().error = Some("Hook test cancelled".into());
    insta::assert_snapshot!(
        "hook_manager_cancelled",
        wide_narrow(
            render_to_text(&cancelled, 100, 30),
            render_to_text(&cancelled, 60, 24)
        )
    );

    let mut bare = hook_manager_render_state();
    bare.active_profile = crate::runtime::ActiveProfile::Bare;
    bare.run_capabilities = crate::runtime::RunCapabilities::for_profile(&bare.active_profile);
    let manager = bare.hook_manager.as_mut().unwrap();
    manager.writes_enabled = false;
    manager.execution_enabled = false;
    manager.notice = None;
    insta::assert_snapshot!(
        "hook_manager_bare",
        wide_narrow(
            render_to_text(&bare, 100, 30),
            render_to_text(&bare, 60, 24)
        )
    );
}

#[test]
fn fail_closed_session_start_block_is_repairable_snapshot() {
    let mut state = hook_manager_render_state();
    state.hook_blocked = true;
    state.hook_blocked_event = Some(crate::hooks::HookEvent::SessionStart);
    state.status_message = Some(
        "Session-start hook blocked: Hook failed (fail-closed). Run /hooks to repair or disable it"
            .into(),
    );
    let manager = state.hook_manager.as_mut().unwrap();
    manager.notice = None;
    manager.error = state.status_message.clone();
    insta::assert_snapshot!(
        "hook_session_blocked",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

fn permission_manager_render_state() -> AppState {
    let mut state = test_state();
    state.permission_manager = Some(crate::app::PermissionManagerState {
        filter: String::new(),
        selected_idx: 1,
        entries: vec![
            crate::app::PermissionManagerEntry {
                source: crate::app::PermissionEntrySource::Legacy(0),
                decision: crate::permission::PermissionDecision::Allow,
                matcher: "read_file".into(),
            },
            crate::app::PermissionManagerEntry {
                source: crate::app::PermissionEntrySource::Rich(0),
                decision: crate::permission::PermissionDecision::Ask,
                matcher: "run_command · cargo test".into(),
            },
            crate::app::PermissionManagerEntry {
                source: crate::app::PermissionEntrySource::Session(0),
                decision: crate::permission::PermissionDecision::Allow,
                matcher: "write_file · /tmp/project/src/lib.rs".into(),
            },
        ],
        confirmation: None,
        busy: false,
        writes_enabled: true,
        generation: 4,
        error: None,
        notice: Some("Permission removed".into()),
    });
    state
}

fn memory_manager_render_state() -> AppState {
    let mut state = test_state();
    state.user_memory_records = vec![
        crate::memory::MemoryRecord::new(
            crate::memory::MemoryScope::User,
            crate::memory::MemoryKind::UserPreference,
            "Prefer concise completion notes with the actual validation result.",
            crate::memory::MemoryProvenance::Manual,
        )
        .unwrap(),
        crate::memory::MemoryRecord::new(
            crate::memory::MemoryScope::User,
            crate::memory::MemoryKind::UserWorkflow,
            "Run focused checks before repository-wide gates.",
            crate::memory::MemoryProvenance::Manual,
        )
        .unwrap(),
    ];
    state.workspace_memory_records = vec![crate::memory::MemoryRecord::new(
        crate::memory::MemoryScope::Workspace,
        crate::memory::MemoryKind::WorkspaceConvention,
        "Use hermetic loopback fixtures for provider checks.",
        crate::memory::MemoryProvenance::Manual,
    )
    .unwrap()];
    state.memory_manager = Some(crate::app::MemoryManagerState {
        filter: String::new(),
        selected_idx: 0,
        view: crate::app::MemoryManagerView::User,
        editor: None,
        confirmation: None,
        busy: false,
        writes_enabled: true,
        user_recall: true,
        user_generation: false,
        workspace_recall: false,
        workspace_generation: false,
        generation: 1,
        error: None,
        notice: None,
    });
    state
}

#[test]
fn memory_manager_snapshots() {
    let populated = memory_manager_render_state();
    insta::assert_snapshot!(
        "memory_manager",
        wide_narrow(
            render_to_text(&populated, 100, 30),
            render_to_text(&populated, 60, 24)
        )
    );

    let mut proposal = memory_manager_render_state();
    proposal.memory_proposals.push(
        crate::memory::MemoryProposal::new(
            crate::memory::MemoryScope::Workspace,
            crate::memory::MemoryKind::WorkspaceDecision,
            "Provider checks use a fixed deterministic message.",
        )
        .unwrap(),
    );
    let manager = proposal.memory_manager.as_mut().unwrap();
    manager.view = crate::app::MemoryManagerView::Proposals;
    manager.confirmation = Some(crate::app::MemoryConfirmation {
        action: crate::app::MemoryConfirmationAction::AcceptProposal {
            id: proposal.memory_proposals[0].id,
        },
        confirm_selected: false,
    });
    insta::assert_snapshot!(
        "memory_proposal",
        wide_narrow(
            render_to_text(&proposal, 100, 30),
            render_to_text(&proposal, 60, 24)
        )
    );

    let mut bare = memory_manager_render_state();
    let manager = bare.memory_manager.as_mut().unwrap();
    manager.writes_enabled = false;
    manager.user_recall = false;
    manager.error =
        Some("Memory changes are disabled in bare mode; rerun without --profile bare".into());
    insta::assert_snapshot!(
        "memory_bare",
        wide_narrow(
            render_to_text(&bare, 100, 30),
            render_to_text(&bare, 60, 24)
        )
    );

    let mut editor = memory_manager_render_state();
    editor.memory_manager.as_mut().unwrap().editor = Some(crate::app::MemoryEditorState {
        id: None,
        scope: crate::memory::MemoryScope::User,
        kind: crate::memory::MemoryKind::UserPreference,
        text: "Prefer deterministic output".into(),
        cursor: "Prefer deterministic output".len(),
    });
    insta::assert_snapshot!(
        "memory_editor",
        wide_narrow(
            render_to_text(&editor, 100, 30),
            render_to_text(&editor, 60, 24)
        )
    );
}

fn goal_render_state(status: crate::goal::GoalStatus) -> AppState {
    let mut state = test_state();
    let now = chrono::DateTime::parse_from_rfc3339("2026-08-11T12:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    let mut goal = crate::goal::GoalState::start(
        "Finish the bounded implementation and prove it with recorded tests.".into(),
        crate::goal::GoalLimits::from_config(crate::goal::GoalConfig::default()).unwrap(),
        now,
    )
    .unwrap();
    goal.continuations = 3;
    goal.combined_provider_tokens = 12_345;
    goal.accumulated_working_seconds = 421;
    goal.active_since = None;
    goal.status = status;
    if status == crate::goal::GoalStatus::Paused {
        goal.pause_reason = Some(crate::goal::GoalPauseReason::Approval);
    }
    state.goal_evaluating = status == crate::goal::GoalStatus::Active;
    state.pager_content = goal.render_status_with_activity(now, state.goal_evaluating);
    state.pager_open = true;
    state.goal_state = Some(goal);
    state
}

macro_rules! goal_snapshots {
    ($name:literal, $status:expr) => {{
        let state = goal_render_state($status);
        insta::assert_snapshot!(
            concat!($name),
            wide_narrow(
                render_to_text(&state, 100, 30),
                render_to_text(&state, 60, 24)
            )
        );
    }};
}

#[test]
fn goal_lifecycle_snapshots() {
    goal_snapshots!("goal_evaluating", crate::goal::GoalStatus::Active);
    goal_snapshots!("goal_paused", crate::goal::GoalStatus::Paused);
    goal_snapshots!("goal_complete", crate::goal::GoalStatus::Complete);
    goal_snapshots!("goal_unreachable", crate::goal::GoalStatus::Unreachable);
    goal_snapshots!("goal_blocked", crate::goal::GoalStatus::Blocked);
    goal_snapshots!("goal_usage_limited", crate::goal::GoalStatus::UsageLimited);
    goal_snapshots!(
        "goal_budget_limited",
        crate::goal::GoalStatus::BudgetLimited
    );
}

#[test]
fn dream_and_distill_status_snapshots() {
    use crate::learning::{
        MaintenanceKind, MaintenanceRunStatus, MaintenanceRunSummary, MaintenanceSnapshot,
        MaintenanceTrigger,
    };
    let completed = MaintenanceRunSummary {
        id: uuid::Uuid::parse_str("00000000-0000-0000-0000-000000000134").unwrap(),
        status: MaintenanceRunStatus::Completed,
        trigger: MaintenanceTrigger::Automatic,
        finished_at: Some(
            chrono::DateTime::parse_from_rfc3339("2026-08-17T01:34:00Z")
                .unwrap()
                .with_timezone(&chrono::Utc),
        ),
        route: Some("mimo/mimo-v2.5-pro".into()),
        evidence_count: 12,
        automatic_changes: 3,
        conflicts: 1,
        note: Some("applied 3 Dream memory changes; 1 conflict".into()),
    };
    let recent_status = |suffix: u128, status, note: &str| MaintenanceRunSummary {
        id: uuid::Uuid::from_u128(0x130 + suffix),
        status,
        trigger: MaintenanceTrigger::Manual,
        finished_at: None,
        route: None,
        evidence_count: 0,
        automatic_changes: 0,
        conflicts: 0,
        note: Some(note.into()),
    };
    let dream = MaintenanceSnapshot {
        kind: MaintenanceKind::Dream,
        enabled: true,
        interval_days: 7,
        due: false,
        latest: Some(completed.clone()),
        recent: vec![
            completed,
            recent_status(1, MaintenanceRunStatus::Skipped, "no eligible priced route"),
            recent_status(2, MaintenanceRunStatus::Failed, "invalid structured batch"),
            recent_status(
                3,
                MaintenanceRunStatus::Cancelled,
                "cancelled for user input",
            ),
            recent_status(4, MaintenanceRunStatus::Undone, "digest-verified undo"),
        ],
        active_skill_generations: 0,
    };
    let mut state = test_state();
    state.pager_content = dream.render();
    state.pager_open = true;
    insta::assert_snapshot!(
        "dream_status",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );

    let distill_run = MaintenanceRunSummary {
        id: uuid::Uuid::parse_str("00000000-0000-0000-0000-000000000135").unwrap(),
        status: MaintenanceRunStatus::Completed,
        trigger: MaintenanceTrigger::Manual,
        finished_at: None,
        route: Some("openai/gpt-5.6-terra".into()),
        evidence_count: 8,
        automatic_changes: 2,
        conflicts: 0,
        note: Some("published 2 auto-distilled skill generations".into()),
    };
    let distill = MaintenanceSnapshot {
        kind: MaintenanceKind::Distill,
        enabled: false,
        interval_days: 30,
        due: false,
        latest: Some(distill_run.clone()),
        recent: vec![distill_run],
        active_skill_generations: 2,
    };
    state.pager_content = distill.render();
    insta::assert_snapshot!(
        "distill_status",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

#[test]
fn permission_manager_and_confirmation_snapshots() {
    let state = permission_manager_render_state();
    insta::assert_snapshot!(
        "permission_manager",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );

    let mut confirm = permission_manager_render_state();
    let manager = confirm.permission_manager.as_mut().unwrap();
    manager.notice = None;
    manager.confirmation = Some(crate::app::PermissionDeleteConfirmation {
        source: crate::app::PermissionEntrySource::Rich(0),
        label: "Ask · run_command · cargo test".into(),
        delete_selected: false,
    });
    insta::assert_snapshot!(
        "permission_confirmation",
        wide_narrow(
            render_to_text(&confirm, 100, 30),
            render_to_text(&confirm, 60, 24)
        )
    );
}

#[test]
fn permission_manager_busy_failure_and_empty_snapshots() {
    let mut busy = permission_manager_render_state();
    let manager = busy.permission_manager.as_mut().unwrap();
    manager.busy = true;
    manager.notice = None;
    insta::assert_snapshot!(
        "permission_busy",
        wide_narrow(
            render_to_text(&busy, 100, 30),
            render_to_text(&busy, 60, 24)
        )
    );

    let mut failed = permission_manager_render_state();
    let manager = failed.permission_manager.as_mut().unwrap();
    manager.notice = None;
    manager.error = Some("Could not remove permission: simulated filesystem failure".into());
    manager.confirmation = Some(crate::app::PermissionDeleteConfirmation {
        source: crate::app::PermissionEntrySource::Rich(0),
        label: "Ask · run_command · cargo test".into(),
        delete_selected: true,
    });
    insta::assert_snapshot!(
        "permission_failure",
        wide_narrow(
            render_to_text(&failed, 100, 30),
            render_to_text(&failed, 60, 24)
        )
    );

    let mut empty = permission_manager_render_state();
    let manager = empty.permission_manager.as_mut().unwrap();
    manager.entries.clear();
    manager.selected_idx = 0;
    manager.notice = None;
    insta::assert_snapshot!(
        "permission_empty",
        wide_narrow(
            render_to_text(&empty, 100, 30),
            render_to_text(&empty, 60, 24)
        )
    );
}

#[test]
fn dynamic_keymap_help_labels_follow_the_effective_bindings() {
    let mut state = test_state();
    let mut configured = crate::keymap::KeymapConfig::new();
    configured.insert(
        "model_picker".into(),
        crate::keymap::KeyBindingConfig::One("ctrl+m".into()),
    );
    configured.insert(
        "cycle_reasoning".into(),
        crate::keymap::KeyBindingConfig::One("ctrl+shift+t".into()),
    );
    configured.insert(
        "save_or_steer".into(),
        crate::keymap::KeyBindingConfig::One("ctrl+shift+s".into()),
    );
    configured.insert(
        "queue_input".into(),
        crate::keymap::KeyBindingConfig::One("ctrl+q".into()),
    );
    state.keymap = crate::keymap::Keymap::from_config(&configured).unwrap();
    assert!(crate::tui::input::handle_event(
        &mut state,
        &crate::tui::event::Event::Key(
            crossterm::event::KeyCode::Char('?'),
            crossterm::event::KeyModifiers::NONE,
        ),
        "/workspace/ca",
    ));
    let shortcuts = render_to_text(&state, 100, 30);
    assert!(shortcuts.contains("Ctrl+M"));
    assert!(shortcuts.contains("Switch model"));
    assert!(shortcuts.contains("Ctrl+Shift+T"));
    assert!(!shortcuts.contains("Ctrl+K"));

    state.pager_open = false;
    state.turn_lifecycle.begin();
    let busy = render_to_text(&state, 100, 30);
    assert!(busy.contains("Enter interrupt · Ctrl+Q queue"));
}

#[test]
fn named_and_bare_profile_snapshots() {
    let mut named = test_state();
    named.active_profile = crate::runtime::ActiveProfile::Named("work".into());
    named.run_capabilities = crate::runtime::RunCapabilities::for_profile(&named.active_profile);
    insta::assert_snapshot!(
        "named_profile",
        wide_narrow(
            render_to_text(&named, 100, 30),
            render_to_text(&named, 60, 24)
        )
    );

    let mut bare = test_state();
    bare.active_profile = crate::runtime::ActiveProfile::Bare;
    bare.run_capabilities = crate::runtime::RunCapabilities::for_profile(&bare.active_profile);
    bare.config_hub = Some(crate::app::ConfigHubState {
        filter: String::new(),
        selected_idx: 0,
        items: vec![crate::app::ConfigHubItem {
            label: "Active profile".into(),
            value: "bare · persistent writes, sessions, MCP, skills, and hooks disabled".into(),
            keywords: "profile configuration runtime bare".into(),
            action: crate::app::ConfigHubAction::Profile("bare".into()),
        }],
    });
    insta::assert_snapshot!(
        "bare_profile_config",
        wide_narrow(
            render_to_text(&bare, 100, 30),
            render_to_text(&bare, 60, 24)
        )
    );

    let mut no_session = test_state();
    no_session.run_capabilities =
        crate::runtime::RunCapabilities::for_run(&crate::runtime::ActiveProfile::Default, true);
    insta::assert_snapshot!(
        "no_session_title",
        wide_narrow(
            render_to_text(&no_session, 100, 30),
            render_to_text(&no_session, 60, 24)
        )
    );
}

fn sandbox_render_state() -> AppState {
    let mut state = test_state();
    state.effective_sandbox_mode = crate::sandbox::SandboxMode::Required;
    state.sandbox_hub = Some(crate::app::SandboxHubState {
        saved_mode: crate::sandbox::SandboxMode::Required,
        effective_mode: crate::sandbox::SandboxMode::Required,
        backend_available: true,
        writable_roots: vec![
            "/tmp/project".into(),
            "/tmp/shared source".into(),
            "/tmp/generated".into(),
            "/tmp/fourth-root".into(),
        ],
        selected_idx: 0,
        confirmation: None,
        busy: false,
        writes_enabled: true,
        notice: None,
        error: None,
    });
    state
}

#[test]
fn sandbox_hub_snapshots() {
    let state = sandbox_render_state();
    insta::assert_snapshot!(
        "sandbox_hub",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

#[test]
fn sandbox_downgrade_confirmation_snapshots() {
    let mut state = sandbox_render_state();
    state.sandbox_hub.as_mut().unwrap().confirmation = Some(crate::app::SandboxConfirmationState {
        target: crate::sandbox::SandboxMode::Off,
        confirm_selected: false,
    });
    insta::assert_snapshot!(
        "sandbox_confirmation",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

#[test]
fn sandbox_bare_empty_snapshots() {
    let mut state = sandbox_render_state();
    let hub = state.sandbox_hub.as_mut().unwrap();
    hub.writable_roots.clear();
    hub.writes_enabled = false;
    hub.backend_available = false;
    hub.error = Some("View-only; rerun without --profile bare".into());
    insta::assert_snapshot!(
        "sandbox_bare_empty",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

#[test]
fn sandbox_busy_and_failure_snapshots() {
    let mut busy = sandbox_render_state();
    let hub = busy.sandbox_hub.as_mut().unwrap();
    hub.busy = true;
    hub.notice = Some("Saving policy-only for the next launch…".into());
    insta::assert_snapshot!(
        "sandbox_busy",
        wide_narrow(
            render_to_text(&busy, 100, 30),
            render_to_text(&busy, 60, 24)
        )
    );

    let mut failed = sandbox_render_state();
    let hub = failed.sandbox_hub.as_mut().unwrap();
    hub.selected_idx = 1;
    hub.error = Some("Could not save sandbox mode: permission denied".into());
    insta::assert_snapshot!(
        "sandbox_failure",
        wide_narrow(
            render_to_text(&failed, 100, 30),
            render_to_text(&failed, 60, 24)
        )
    );
}

fn context_limit_render_state() -> AppState {
    let mut state = test_state();
    state.context_limit_editor = Some(crate::app::ContextLimitEditorState {
        provider: "mimo".into(),
        model: "mimo-v2.5-pro".into(),
        window_input: "131072".into(),
        threshold_input: "98304".into(),
        fallback_window: Some(65536),
        fallback_threshold: None,
        editing_profile: true,
        field: crate::app::ContextLimitField::Threshold,
        cursor: 5,
        saving: false,
        writes_enabled: true,
        error: None,
    });
    state
}

#[test]
fn context_limit_populated_and_inherited_snapshots() {
    let populated = context_limit_render_state();
    insta::assert_snapshot!(
        "context_limit_populated",
        wide_narrow(
            render_to_text(&populated, 100, 30),
            render_to_text(&populated, 60, 24)
        )
    );

    let mut inherited = context_limit_render_state();
    let editor = inherited.context_limit_editor.as_mut().unwrap();
    editor.window_input.clear();
    editor.threshold_input.clear();
    editor.fallback_window = Some(65536);
    editor.fallback_threshold = None;
    editor.editing_profile = false;
    editor.field = crate::app::ContextLimitField::Window;
    editor.cursor = 0;
    insta::assert_snapshot!(
        "context_limit_inherited",
        wide_narrow(
            render_to_text(&inherited, 100, 30),
            render_to_text(&inherited, 60, 24)
        )
    );
}

#[test]
fn context_limit_bare_busy_and_failure_snapshots() {
    let mut bare = context_limit_render_state();
    let editor = bare.context_limit_editor.as_mut().unwrap();
    editor.writes_enabled = false;
    editor.error = Some("View-only; rerun without --profile bare".into());
    insta::assert_snapshot!(
        "context_limit_bare",
        wide_narrow(
            render_to_text(&bare, 100, 30),
            render_to_text(&bare, 60, 24)
        )
    );

    let mut busy = context_limit_render_state();
    busy.context_limit_editor.as_mut().unwrap().saving = true;
    insta::assert_snapshot!(
        "context_limit_busy",
        wide_narrow(
            render_to_text(&busy, 100, 30),
            render_to_text(&busy, 60, 24)
        )
    );

    let mut failed = context_limit_render_state();
    failed.context_limit_editor.as_mut().unwrap().error =
        Some("Could not save context limits: permission denied".into());
    insta::assert_snapshot!(
        "context_limit_failure",
        wide_narrow(
            render_to_text(&failed, 100, 30),
            render_to_text(&failed, 60, 24)
        )
    );
}

#[test]
fn model_prompt_cache_editor_wide_and_narrow_snapshots() {
    let mut state = test_state();
    state.model_prompt_cache_editor = Some(crate::app::ModelPromptCacheEditorState {
        provider: "custom".into(),
        model: "model-a".into(),
        protocol: crate::agent::ProviderProtocol::OpenAiResponses,
        request_format: Some(crate::config::PromptCacheRequestFormat::OpenAiResponsesExplicit),
        usage_format: Some(crate::config::PromptCacheUsageFormat::Auto),
        policy_override: Some(crate::config::PromptCachePolicy::StablePrefix),
        inherited_policy: Some(crate::config::PromptCachePolicy::Automatic),
        contract_id: "custom_v1:openai_responses_explicit:auto".into(),
        contract_source: crate::provider_contract::PromptCacheContractSource::ModelConfig,
        policy_source: crate::provider_contract::PromptCachePolicySource::ModelConfig,
        formats_locked: false,
        field: crate::app::ModelPromptCacheField::UsageFormat,
        saving: false,
        writes_enabled: true,
        error: None,
    });

    insta::assert_snapshot!(
        "model_prompt_cache_editor",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

fn usage_report_state(populated: bool) -> AppState {
    let mut state = test_state();
    if populated {
        let mut ledger = state
            .usage_ledger
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        ledger.push_test_record(
            crate::usage::UsageKind::MainTurn,
            "mimo",
            "mimo-v2.5-pro",
            std::time::Duration::from_millis(842),
            Some(crate::agent::stream::UsageInfo {
                prompt_tokens: 1_200,
                completion_tokens: 300,
                total_tokens: 1_500,
                cached_prompt_tokens: Some(400),
                cache_write_tokens: None,
                reasoning_tokens: Some(120),
                response_metadata: None,
                cache_usage_override: None,
            }),
            crate::usage::UsageOutcome::Completed,
        );
        ledger.push_test_record(
            crate::usage::UsageKind::Review,
            "deepseek",
            "deepseek-chat",
            std::time::Duration::from_millis(1_450),
            None,
            crate::usage::UsageOutcome::Failed,
        );
        ledger.begin(
            crate::usage::UsageKind::ProviderProbe,
            "local",
            "test-model",
        );
    }
    let snapshot = state
        .usage_ledger
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .snapshot();
    state.pager_content = crate::usage::render_report(&snapshot, &std::collections::HashMap::new());
    state.pager_open = true;
    state
}

#[test]
fn usage_empty_and_populated_snapshots() {
    let empty = usage_report_state(false);
    insta::assert_snapshot!(
        "usage_empty",
        wide_narrow(
            render_to_text(&empty, 100, 30),
            render_to_text(&empty, 60, 24)
        )
    );

    let populated = usage_report_state(true);
    insta::assert_snapshot!(
        "usage_populated",
        wide_narrow(
            render_to_text(&populated, 100, 30),
            render_to_text(&populated, 60, 24)
        )
    );
}

fn provider_account_usage_state(account: String) -> AppState {
    let mut state = usage_report_state(true);
    let snapshot = state
        .usage_ledger
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .snapshot();
    state.pager_content = crate::usage::render_report_with_provider_account(
        &snapshot,
        &std::collections::HashMap::new(),
        None,
        &std::collections::HashMap::new(),
        &std::collections::HashMap::new(),
        fixed_time(),
        &account,
    );
    state
}

#[test]
fn provider_account_usage_snapshots() {
    let loading = provider_account_usage_state("  Loading configured provider accounts…".into());
    insta::assert_snapshot!(
        "provider_account_usage_loading",
        wide_narrow(
            render_to_text(&loading, 100, 30),
            render_to_text(&loading, 60, 24)
        )
    );

    let empty_report = crate::provider_usage::ProviderAccountReport {
        queried_at: fixed_time(),
        entries: Vec::new(),
    };
    let empty = provider_account_usage_state(crate::provider_usage::render(&empty_report));
    insta::assert_snapshot!(
        "provider_account_usage_empty",
        wide_narrow(
            render_to_text(&empty, 100, 30),
            render_to_text(&empty, 60, 24)
        )
    );

    let unavailable_report = crate::provider_usage::ProviderAccountReport {
        queried_at: fixed_time(),
        entries: vec![crate::provider_usage::ProviderAccountEntry::Unavailable {
            provider: "openai".into(),
            service: crate::provider_usage::ProviderAccountService::OpenAi,
            reason: "organization-admin access unavailable".into(),
        }],
    };
    let unavailable =
        provider_account_usage_state(crate::provider_usage::render(&unavailable_report));
    insta::assert_snapshot!(
        "provider_account_usage_unavailable",
        wide_narrow(
            render_to_text(&unavailable, 100, 30),
            render_to_text(&unavailable, 60, 24)
        )
    );

    let populated_report = crate::provider_usage::ProviderAccountReport {
        queried_at: fixed_time(),
        entries: vec![
            crate::provider_usage::ProviderAccountEntry::OpenAi {
                provider: "openai".into(),
                month: "2026-08".into(),
                requests: 4,
                input_tokens: 120,
                output_tokens: 30,
                cached_input_tokens: 20,
                costs: vec![crate::provider_usage::CurrencyAmount {
                    currency: "USD".into(),
                    value: "1.25".parse().unwrap(),
                }],
            },
            crate::provider_usage::ProviderAccountEntry::DeepSeek {
                provider: "deepseek".into(),
                is_available: true,
                balances: vec![crate::provider_usage::DeepSeekBalance {
                    currency: "USD".into(),
                    total: "12.5".parse().unwrap(),
                    granted: "2.5".parse().unwrap(),
                    topped_up: "10".parse().unwrap(),
                }],
            },
            crate::provider_usage::ProviderAccountEntry::Mimo {
                provider: "mimo".into(),
                allocation_credits: Some(8_000_000),
                allocation_usd: Some("50".parse().unwrap()),
                observed_credits: None,
                observed_requests: 0,
                observed_at_least: false,
            },
        ],
    };
    let populated = provider_account_usage_state(format!(
        "  Cached for up to 60 seconds\n{}",
        crate::provider_usage::render(&populated_report)
    ));
    insta::assert_snapshot!(
        "provider_account_usage_populated",
        wide_narrow(
            render_to_text(&populated, 100, 30),
            render_to_text(&populated, 60, 24)
        )
    );
}

fn request_pager_state(
    name: &str,
    messages: Vec<crate::agent::client::ChatMessage>,
    tools: Vec<crate::agent::client::ToolDef>,
    secrets: &[&str],
) -> AppState {
    let mut state = test_state();
    let prepared = crate::request::PreparedModelRequest::new(
        crate::request::RequestKind::MainTurn,
        "mimo",
        "mimo-v2.5-pro",
        messages,
        tools,
        crate::agent::client::ChatRequestOptions::default(),
    )
    .with_context_breakdown(crate::context::ContextBreakdown {
        source: crate::context::ContextBreakdownSource::Prepared,
        instructions: 120,
        skills: 30,
        built_in_tools: 210,
        mcp_tools: 45,
        checkpoint: 18,
        conversation: 24,
        attachments: 12,
        total: 459,
        mcp_prepared: true,
    });
    state.request_store.record(&prepared, secrets);
    state.pager_content = crate::request::render_latest_request(&state.request_store);
    state.pager_open = true;
    state.status_message = Some(format!("Request fixture: {name}"));
    state
}

fn scroll_request_to(state: &mut AppState, needle: &str) {
    state.pager_scroll = state
        .pager_content
        .lines()
        .position(|line| line.starts_with(needle))
        .unwrap_or_default()
        .saturating_sub(2) as u16;
}

#[test]
fn request_inspector_snapshots() {
    let mut empty = test_state();
    empty.pager_content = crate::request::render_latest_request(&empty.request_store);
    empty.pager_open = true;
    insta::assert_snapshot!(
        "request_empty",
        wide_narrow(
            render_to_text(&empty, 100, 30),
            render_to_text(&empty, 60, 24)
        )
    );

    let mut populated = request_pager_state(
        "populated",
        vec![
            crate::agent::client::ChatMessage {
                role: "system".into(),
                content: crate::image_content::ModelContent::text(
                    "You are a coding agent. Keep changes focused.",
                ),
                tool_calls: None,
                tool_call_id: None,
                response_items: Vec::new(),
            },
            crate::agent::client::ChatMessage {
                role: "user".into(),
                content: crate::image_content::ModelContent::text("Inspect src/request.rs"),
                tool_calls: None,
                tool_call_id: None,
                response_items: Vec::new(),
            },
        ],
        vec![crate::agent::client::ToolDef {
            def_type: "function".into(),
            function: crate::agent::client::ToolFunction {
                name: "read_file".into(),
                description: "Read a bounded line range".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {"path": {"type": "string"}}
                }),
            },
        }],
        &[],
    );
    scroll_request_to(&mut populated, "Messages (");
    insta::assert_snapshot!(
        "request_populated",
        wide_narrow(
            render_to_text(&populated, 100, 30),
            render_to_text(&populated, 60, 24)
        )
    );

    let mut omitted = request_pager_state(
        "omitted",
        (0..70)
            .map(|index| crate::agent::client::ChatMessage {
                role: "user".into(),
                content: crate::image_content::ModelContent::text(format!(
                    "message {index}: {}",
                    "bounded content ".repeat(700)
                )),
                tool_calls: None,
                tool_call_id: None,
                response_items: Vec::new(),
            })
            .collect(),
        Vec::new(),
        &[],
    );
    omitted.pager_scroll = omitted.pager_content.lines().count().saturating_sub(18) as u16;
    insta::assert_snapshot!(
        "request_omitted",
        wide_narrow(
            render_to_text(&omitted, 100, 30),
            render_to_text(&omitted, 60, 24)
        )
    );

    let mut binary = request_pager_state(
        "binary metadata",
        vec![crate::agent::client::ChatMessage {
            role: "user".into(),
            content: crate::image_content::ModelContent::multimodal(
                "Inspect this image",
                &[crate::image_content::normalize_rgba(1, 1, vec![7, 8, 9, 255]).unwrap()],
            ),
            tool_calls: None,
            tool_call_id: None,
            response_items: Vec::new(),
        }],
        Vec::new(),
        &[],
    );
    scroll_request_to(&mut binary, "Messages (");
    insta::assert_snapshot!(
        "request_binary",
        wide_narrow(
            render_to_text(&binary, 100, 30),
            render_to_text(&binary, 60, 24)
        )
    );

    let mut redacted = request_pager_state(
        "redacted",
        vec![crate::agent::client::ChatMessage {
            role: "user".into(),
            content: crate::image_content::ModelContent::text(
                "Authorization: Bearer snapshot-secret; runtime snapshot-secret",
            ),
            tool_calls: None,
            tool_call_id: None,
            response_items: Vec::new(),
        }],
        Vec::new(),
        &["snapshot-secret"],
    );
    scroll_request_to(&mut redacted, "Messages (");
    insta::assert_snapshot!(
        "request_redacted",
        wide_narrow(
            render_to_text(&redacted, 100, 30),
            render_to_text(&redacted, 60, 24)
        )
    );
}

#[test]
fn tool_selection_failure_snapshots() {
    let mut state = test_state();
    state.messages.push(crate::app::Message {
        role: crate::app::Role::User,
        content: "Inspect the selected files".into(),
        tool_calls: None,
        thinking: None,
        timestamp: fixed_time(),
        response_items: Vec::new(),
    });
    state.status_message = Some("Tool selection failed: unknown tool: missing_tool".into());
    state.turn_lifecycle.fail();

    insta::assert_snapshot!(
        "tool_selection_failure",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

fn fixed_time() -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::from_timestamp(0, 0).expect("Unix epoch is valid")
}

#[test]
fn streaming_snapshots() {
    let mut state = test_state();
    state.messages.push(Message {
        role: Role::User,
        content: "Inspect the project".to_string(),
        tool_calls: None,
        thinking: None,
        timestamp: fixed_time(),
        response_items: Vec::new(),
    });
    state.streaming = Some(StreamingState {
        usage_handle: crate::usage::UsageHandle::test_handle(),
        content: "I am checking the relevant files.".to_string(),
        tool_calls: vec![PartialToolCall {
            index: 0,
            id: Some("call-stream".to_string()),
            name: Some("read_file".to_string()),
            arguments: r#"{"path":"src/main.rs"}"#.to_string(),
        }],
        thinking: None,
        last_usage: None,
        response_items: Vec::new(),
        reasoning_started_at: None,
        reasoning_duration: None,
        generation_started_at: None,
    });
    state.turn_lifecycle.begin();
    insta::assert_snapshot!(
        "streaming",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

#[test]
fn failed_stream_retry_snapshots() {
    let mut state = test_state();
    state.messages = vec![
        Message {
            role: Role::User,
            content: "Inspect the provider stream".into(),
            tool_calls: None,
            thinking: None,
            timestamp: fixed_time(),
            response_items: Vec::new(),
        },
        Message {
            role: Role::Assistant,
            content: "I decoded part of the response before the connection failed.".into(),
            tool_calls: None,
            thinking: None,
            timestamp: fixed_time(),
            response_items: Vec::new(),
        },
    ];
    state.turn_lifecycle.begin();
    state.turn_lifecycle.fail();
    state.retry_boundary = Some(crate::app::RetryBoundary::replay(0));
    state.status_message = Some(
        "Authentication rejected for mimo (HTTP 401). Run /provider setup mimo, then /retry".into(),
    );

    insta::assert_snapshot!(
        "failed_stream_retry",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

#[test]
fn recovery_save_failure_keeps_the_local_cause_visible() {
    let mut state = test_state();
    state.turn_lifecycle.begin();
    state.turn_lifecycle.fail();
    state.status_message =
        Some("Output interrupted (test · stream ended early). /errors · /retry".into());
    state.turn_recovery.record(
        "test",
        "model",
        crate::turn_recovery::Category::PrematureEof,
    );
    state.turn_recovery.pending = true;
    state
        .turn_recovery
        .save_failed("source save: permission denied");

    for (width, height) in [(100, 30), (60, 24)] {
        let rendered = render_to_text(&state, width, height);
        assert!(
            rendered.contains("Recovery stopped: source save: permission denied"),
            "{rendered}"
        );
        assert!(!rendered.contains("Output interrupted"), "{rendered}");
        if width == 100 {
            assert!(rendered.contains("No replacement sent"), "{rendered}");
            assert!(rendered.contains("/errors · /retry"), "{rendered}");
        }
    }
}

#[test]
fn pre_output_recovery_snapshots() {
    for (name, error) in [
        (
            "failed_turn_http400_recovery",
            crate::agent::stream::ProviderError::Http { status: 400 },
        ),
        (
            "failed_turn_exhausted_recovery",
            crate::agent::stream::ProviderError::Transport(
                crate::agent::stream::ProviderTransportFailure::Request,
            ),
        ),
    ] {
        let mut state = test_state();
        state.messages = vec![Message {
            role: Role::User,
            content: "Inspect the provider stream".into(),
            tool_calls: None,
            thinking: None,
            timestamp: fixed_time(),
            response_items: Vec::new(),
        }];
        state.active_turn_start_message_count = Some(0);
        state.turn_lifecycle.begin();
        crate::apply_stream_failure(&mut state, "test", &error);
        let wide = render_to_text(&state, 100, 30);
        let narrow = render_to_text(&state, 60, 24);
        for rendered in [&wide, &narrow] {
            assert_eq!(rendered.matches("Inspect the provider stream").count(), 1);
            assert!(rendered.contains("/retry") || rendered.contains("clean branch"));
            assert!(!rendered.contains("Use /retry to branch. Stream failed"));
            assert!(!rendered.contains("provider HTTP 400"));
        }
        insta::assert_snapshot!(name, wide_narrow(wide, narrow));
    }
}

#[test]
fn reasoning_picker_wide_and_narrow_snapshots() {
    let mut state = test_state();
    state.current_model = Some(crate::app::ModelInfo {
        id: "coder".into(),
        provider: "local".into(),
        display_name: "Coder Pro".into(),
    });
    state.reasoning_effort = Some(crate::config::ReasoningEffort::Medium);
    state.reasoning_picker = Some(crate::app::ReasoningPickerState {
        efforts: vec![
            crate::config::ReasoningEffort::None,
            crate::config::ReasoningEffort::Low,
            crate::config::ReasoningEffort::Medium,
            crate::config::ReasoningEffort::High,
            crate::config::ReasoningEffort::XHigh,
            crate::config::ReasoningEffort::Max,
        ],
        selected_idx: 3,
    });

    insta::assert_snapshot!(
        "reasoning_picker",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

#[test]
fn reasoning_collapsed_and_expanded_snapshots() {
    let mut state = test_state();
    let turn_id = uuid::Uuid::parse_str("11111111-1111-4111-8111-111111111111").unwrap();
    state.messages.push(Message {
        role: Role::Assistant,
        content: "The fix is ready.".into(),
        tool_calls: None,
        thinking: Some("First inspect the state.\nThen preserve the invariant.".into()),
        timestamp: fixed_time(),
        response_items: Vec::new(),
    });
    state.transcript_annotations = vec![crate::app::TranscriptMessageAnnotation {
        message_timestamp: fixed_time(),
        turn_id,
        reasoning_duration_ms: Some(651),
        provider: None,
        model: None,
        total_turn_duration_ms: None,
        terminal_outcome: None,
    }];
    insta::assert_snapshot!(
        "reasoning_collapsed",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
    state.show_thinking = true;
    insta::assert_snapshot!(
        "reasoning_expanded",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

fn turn_metadata_render_state(outcome: crate::app::TranscriptTurnOutcome) -> AppState {
    let mut state = test_state();
    let turn_id = uuid::Uuid::parse_str("31313131-3131-4131-8131-313131313131").unwrap();
    state.show_transcript_timestamps = true;
    state.show_assistant_footers = true;
    state.messages = vec![
        Message {
            role: Role::User,
            content: "Check the release path.".into(),
            tool_calls: None,
            thinking: None,
            timestamp: fixed_time(),
            response_items: Vec::new(),
        },
        Message {
            role: Role::Assistant,
            content: "I checked the first boundary.".into(),
            tool_calls: None,
            thinking: None,
            timestamp: fixed_time() + chrono::Duration::seconds(1),
            response_items: Vec::new(),
        },
        Message {
            role: Role::Assistant,
            content: "The release path is ready.".into(),
            tool_calls: None,
            thinking: None,
            timestamp: fixed_time() + chrono::Duration::seconds(2),
            response_items: Vec::new(),
        },
    ];
    state.transcript_annotations = vec![
        crate::app::TranscriptMessageAnnotation {
            message_timestamp: state.messages[1].timestamp,
            turn_id,
            reasoning_duration_ms: None,
            provider: Some("mimo".into()),
            model: Some("mimo-v2.5-pro".into()),
            total_turn_duration_ms: None,
            terminal_outcome: None,
        },
        crate::app::TranscriptMessageAnnotation {
            message_timestamp: state.messages[2].timestamp,
            turn_id,
            reasoning_duration_ms: None,
            provider: Some("mimo".into()),
            model: Some("mimo-v2.5-pro".into()),
            total_turn_duration_ms: Some(1_400),
            terminal_outcome: Some(outcome),
        },
    ];
    state
}

#[test]
fn transcript_metadata_completed_and_interrupted_snapshots() {
    let completed = turn_metadata_render_state(crate::app::TranscriptTurnOutcome::Completed);
    let completed_wide = render_to_text(&completed, 100, 30);
    assert_eq!(completed_wide.matches("mimo/mimo-v2.5-pro").count(), 1);
    insta::assert_snapshot!(
        "turn_metadata_completed",
        wide_narrow(completed_wide, render_to_text(&completed, 60, 24))
    );

    let interrupted = turn_metadata_render_state(crate::app::TranscriptTurnOutcome::Cancelled);
    insta::assert_snapshot!(
        "turn_metadata_interrupted",
        wide_narrow(
            render_to_text(&interrupted, 100, 30),
            render_to_text(&interrupted, 60, 24)
        )
    );
}

#[test]
fn transcript_metadata_defaults_off_and_durations_are_deterministic() {
    let mut state = turn_metadata_render_state(crate::app::TranscriptTurnOutcome::Completed);
    state.show_transcript_timestamps = false;
    state.show_assistant_footers = false;
    let rendered = render_to_text(&state, 100, 30);
    assert!(!rendered.contains("1970-01-01"));
    assert!(!rendered.contains("mimo/mimo-v2.5-pro"));
    assert_eq!(super::render::format_turn_duration(651), "651ms");
    assert_eq!(super::render::format_turn_duration(1_400), "1.4s");
    assert_eq!(super::render::format_turn_duration(62_000), "1m 02s");
}

#[test]
fn inline_agent_activity_collapsed_and_focused_snapshots() {
    let mut state = test_state();
    let turn_id = uuid::Uuid::parse_str("22222222-2222-4222-8222-222222222222").unwrap();
    let first = crate::agent::subagent::AgentId(
        uuid::Uuid::parse_str("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa").unwrap(),
    );
    let second = crate::agent::subagent::AgentId(
        uuid::Uuid::parse_str("bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb").unwrap(),
    );
    state.messages.push(Message {
        role: Role::Assistant,
        content: "I launched two focused explorations.".into(),
        tool_calls: None,
        thinking: None,
        timestamp: fixed_time(),
        response_items: Vec::new(),
    });
    state.transcript_annotations = vec![crate::app::TranscriptMessageAnnotation {
        message_timestamp: fixed_time(),
        turn_id,
        reasoning_duration_ms: None,
        provider: None,
        model: None,
        total_turn_duration_ms: None,
        terminal_outcome: None,
    }];
    state.live_agent_activity = vec![
        crate::agent::subagent::AgentActivitySnapshot {
            id: first,
            parent_turn: turn_id,
            parent: None,
            origin: crate::agent::subagent::AgentRunOrigin::Conversation,
            purpose: crate::agent::subagent::AgentRunPurpose::Subagent,
            status: crate::agent::subagent::AgentStatus::Running,
            task: "Audit learning hooks and tools".into(),
            current_action: Some("read_file".into()),
            created_at: fixed_time(),
            updated_at: fixed_time(),
            result_summary: None,
            recent_output: "read_file: inspected src/main.rs".into(),
            events: vec![crate::agent::subagent::AgentEvent {
                sequence: 1,
                at: fixed_time(),
                message: "started read_file".into(),
            }],
        },
        crate::agent::subagent::AgentActivitySnapshot {
            id: second,
            parent_turn: turn_id,
            parent: None,
            origin: crate::agent::subagent::AgentRunOrigin::Conversation,
            purpose: crate::agent::subagent::AgentRunPurpose::Subagent,
            status: crate::agent::subagent::AgentStatus::WaitingApproval,
            task: "Audit session structure".into(),
            current_action: Some("run_command".into()),
            created_at: fixed_time() + chrono::Duration::seconds(1),
            updated_at: fixed_time() + chrono::Duration::seconds(1),
            result_summary: None,
            recent_output: String::new(),
            events: vec![crate::agent::subagent::AgentEvent {
                sequence: 1,
                at: fixed_time(),
                message: "approval required for run_command".into(),
            }],
        },
    ];

    let collapsed = render_to_text(&state, 100, 30);
    assert!(
        collapsed.contains("✿ Background Agent — Audit learning hooks and tools"),
        "first worker should have a stable flower icon:\n{collapsed}"
    );
    assert!(
        collapsed.contains("◆ Waiting for approval — Audit session structure"),
        "second worker should have a distinct stable icon:\n{collapsed}"
    );
    let (_, collapsed_buffer) = rendered_frame(&state, 100, 30);
    assert_eq!(collapsed_buffer[(2, 4)].symbol(), "✿");
    assert_eq!(collapsed_buffer[(2, 4)].fg, Color::Rgb(244, 107, 147));
    assert_eq!(collapsed_buffer[(2, 6)].symbol(), "◆");
    assert_eq!(collapsed_buffer[(2, 6)].fg, Color::Rgb(173, 127, 255));

    insta::assert_snapshot!(
        "inline_agent_activity_collapsed",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );

    state.agent_activity_focus = Some(crate::app::AgentActivityFocusState {
        turn_id,
        selected_agent: Some(second),
        detail_open: true,
        ..crate::app::AgentActivityFocusState::default()
    });
    insta::assert_snapshot!(
        "inline_agent_activity_focused",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );

    state.live_agent_activity.clear();
    state.agent_activity_focus = None;
    state.transcript_agent_summaries = vec![crate::app::TranscriptAgentSummary {
        agent_id: second,
        turn_id,
        purpose: crate::agent::subagent::AgentRunPurpose::Subagent,
        task: "Audit session structure".into(),
        status: crate::agent::subagent::AgentStatus::Completed,
        summary: "Session boundaries verified".into(),
        updated_at: fixed_time() + chrono::Duration::seconds(2),
    }];
    insta::assert_snapshot!(
        "inline_agent_activity_resumed",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

#[test]
fn workspace_roots_snapshots() {
    let mut state = test_state();
    state.workspace = crate::workspace::WorkspaceContext::from_primary(
        "/workspace/ca".into(),
        "/workspace/ca".into(),
    );
    state.cwd = "/workspace/ca".into();
    crate::manage_workspace_command(&mut state, "");
    insta::assert_snapshot!(
        "workspace_roots",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

#[test]
fn named_workspace_catalog_snapshots() {
    let temp = tempfile::tempdir().unwrap();
    let first = temp.path().join("first");
    let second = temp.path().join("second");
    std::fs::create_dir_all(&first).unwrap();
    std::fs::create_dir_all(&second).unwrap();
    let stored =
        crate::workspace::WorkspaceContext::from_roots(&first, std::slice::from_ref(&second))
            .unwrap();
    let catalog = crate::workspace_catalog::WorkspaceCatalogStore::new(
        temp.path().join("profile/workspaces/catalog-v1.json"),
    );
    catalog.save("client", &stored).unwrap();

    let mut state = test_state();
    state.workspace_catalog = Some(catalog);
    crate::manage_workspace_command(&mut state, "list");
    let temporary_root = std::fs::canonicalize(temp.path())
        .unwrap()
        .to_string_lossy()
        .into_owned();
    state.pager_content = state
        .pager_content
        .replace(&temporary_root, "/catalog-fixture");
    insta::assert_snapshot!(
        "named_workspace_catalog",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

#[test]
fn rule_manager_snapshots() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let project = temp.path().join("project");
    std::fs::create_dir_all(home.join(".ca/rules")).unwrap();
    std::fs::create_dir_all(project.join(".ca/rules")).unwrap();
    std::fs::write(
        home.join(".ca/rules/manual.md"),
        "---\ndescription: Opt-in review guidance\nactivation: manual\n---\nReview carefully.\n",
    )
    .unwrap();
    std::fs::write(
        project.join(".ca/rules/legacy.md"),
        "Keep changes focused.\n",
    )
    .unwrap();
    std::fs::write(
        project.join(".ca/rules/rust.md"),
        "---\ndescription: Rust source guidance\nactivation: glob\nglobs: [\"src/**/*.rs\"]\n---\nUse Rust conventions.\n",
    )
    .unwrap();

    let mut state = test_state();
    state.rules_home = Some(std::fs::canonicalize(&home).unwrap());
    state.workspace = crate::workspace::WorkspaceContext::discover(&project).unwrap();
    state.cwd = state.workspace.cwd().to_string_lossy().into_owned();
    crate::refresh_rules(&mut state);
    state.manual_rule_ids.insert("user/manual".into());
    let sessions = crate::session::SessionStore::new(temp.path().join("sessions"));
    crate::manage_rule_command(&mut state, "", &sessions);
    insta::assert_snapshot!(
        "rule_manager",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

#[test]
fn custom_command_manager_snapshots() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let project = temp.path().join("project");
    std::fs::create_dir_all(home.join(".claude/commands")).unwrap();
    std::fs::create_dir_all(project.join(".ca/commands")).unwrap();
    std::fs::write(
        home.join(".claude/commands/foreign.md"),
        "---\ndescription: Imported compatibility command\n---\n\nRead-only guidance.\n",
    )
    .unwrap();
    std::fs::write(
        project.join(".ca/commands/local.md"),
        "---\ndescription: Native workspace command\n---\n\nInspect local changes.\n",
    )
    .unwrap();

    let paths = crate::runtime::RuntimePaths::from_home(home);
    let mut state = test_state();
    state.workspace = crate::workspace::WorkspaceContext::discover(&project).unwrap();
    state.cwd = state.workspace.cwd().to_string_lossy().into_owned();
    state.prompt_commands = crate::prompt_commands::PromptCommandRegistry::discover_for_workspace(
        &paths,
        &state.active_profile,
        &state.workspace,
    );
    crate::manage_prompt_command(&mut state, "");
    insta::assert_snapshot!(
        "custom_command_manager",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

#[test]
fn plugin_manager_snapshots() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let bundle = temp.path().join("bundle");
    std::fs::create_dir_all(&bundle).unwrap();
    let manifest = bundle.join("plugin.toml");
    std::fs::write(
        &manifest,
        "version = 1\nid = \"review-helper\"\nname = \"Review Helper\"\ndescription = \"Local review workflow\"\nworkflows = {}\n",
    )
    .unwrap();
    let paths = crate::runtime::RuntimePaths::from_home(&home);
    let profile = crate::runtime::ActiveProfile::Default;
    crate::plugin::run_tui_action(&paths, &profile, &format!("add {}", manifest.display()))
        .unwrap();
    let content = crate::plugin::run_tui_action(&paths, &profile, "enable review-helper").unwrap();

    let mut state = test_state();
    state.pager_open = true;
    state.pager_content = content;
    insta::assert_snapshot!(
        "plugin_manager",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

#[test]
fn direnv_approval_snapshots() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join(".envrc"), "export PROJECT_TOKEN=private\n").unwrap();
    let mut state = test_state();
    state.workspace = crate::workspace::WorkspaceContext::discover(temp.path()).unwrap();
    state.cwd = state.workspace.cwd().to_string_lossy().into_owned();
    state.workspace_trust_prompt = false;
    crate::manage_direnv_command(&mut state, "");
    let temporary_root = std::fs::canonicalize(temp.path())
        .unwrap()
        .to_string_lossy()
        .into_owned();
    state.pager_content = state
        .pager_content
        .replace(&temporary_root, "/workspace/direnv-fixture");
    insta::assert_snapshot!(
        "direnv_approval",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

#[test]
fn skills_catalog_snapshots() {
    let temp = tempfile::tempdir().unwrap();
    let deploy = temp.path().join("deploy");
    let review = temp.path().join("review-rust");
    std::fs::create_dir_all(&deploy).unwrap();
    std::fs::create_dir_all(&review).unwrap();
    std::fs::write(
        deploy.join("SKILL.md"),
        "---\nname: deploy\ndescription: Deploy releases safely\naliases: [ship]\n---\nbody",
    )
    .unwrap();
    std::fs::write(
        review.join("SKILL.md"),
        "---\nname: review-rust\ndescription: Review Rust changes\n---\nbody",
    )
    .unwrap();
    let mut registry = crate::skills::SkillRegistry::discover(
        crate::skills::SkillRoots {
            project: Some(temp.path().to_path_buf()),
            project_boundary: Some(temp.path().to_path_buf()),
            user: None,
            compatibility: Vec::new(),
        },
        &[],
    );
    registry.activate_exact("deploy").unwrap();
    let mut state = test_state();
    state.skills = std::sync::Arc::new(std::sync::Mutex::new(registry));
    crate::manage_skills_command(&mut state, "");
    insta::assert_snapshot!(
        "skills_catalog",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

fn skill_suggestion_state(activate_release: bool) -> AppState {
    let temp = tempfile::tempdir().unwrap();
    for (directory, name, alias, description) in [
        (
            "release",
            "release-manager",
            "ship-production",
            "Coordinate a safe production deployment and rollback",
        ),
        (
            "review",
            "rust-review",
            "audit-rust",
            "Review Rust code for correctness and maintainability",
        ),
        (
            "reports",
            "evidence-report",
            "write-report",
            "Create a concise report from verified project evidence",
        ),
    ] {
        let skill = temp.path().join(directory);
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::write(
            skill.join("SKILL.md"),
            format!(
                "---\nname: {name}\ndescription: {description}\naliases: [{alias}]\n---\n{name} private body"
            ),
        )
        .unwrap();
    }
    let mut registry = crate::skills::SkillRegistry::discover(
        crate::skills::SkillRoots {
            project: Some(temp.path().to_path_buf()),
            project_boundary: Some(temp.path().to_path_buf()),
            user: None,
            compatibility: Vec::new(),
        },
        &[],
    );
    if activate_release {
        registry.activate_exact("release-manager").unwrap();
    }
    let mut state = test_state();
    state.skills = std::sync::Arc::new(std::sync::Mutex::new(registry));
    state
}

#[test]
fn skill_suggestion_composer_and_ranked_pager_snapshots() {
    let mut composer = skill_suggestion_state(false);
    composer.input = "ship-production and review Rust correctness".into();
    composer.input_cursor = composer.input.len();
    insta::assert_snapshot!(
        "skill_suggestions_composer",
        wide_narrow(
            render_to_text(&composer, 100, 30),
            render_to_text(&composer, 60, 24)
        )
    );

    let mut ranked = skill_suggestion_state(true);
    crate::manage_skills_command(
        &mut ranked,
        "ship-production rust correctness maintainability",
    );
    insta::assert_snapshot!(
        "skill_suggestions_ranked",
        wide_narrow(
            render_to_text(&ranked, 100, 30),
            render_to_text(&ranked, 60, 24)
        )
    );

    let mut exact = skill_suggestion_state(false);
    crate::manage_skills_command(&mut exact, "release-manager");
    insta::assert_snapshot!(
        "skill_suggestions_exact_id",
        wide_narrow(
            render_to_text(&exact, 100, 30),
            render_to_text(&exact, 60, 24)
        )
    );

    let mut no_match = skill_suggestion_state(false);
    crate::manage_skills_command(&mut no_match, "unrelated-zebra-quantum");
    insta::assert_snapshot!(
        "skill_suggestions_no_match",
        wide_narrow(
            render_to_text(&no_match, 100, 30),
            render_to_text(&no_match, 60, 24)
        )
    );
}

#[test]
fn skill_suggestions_never_replace_the_idle_status_row() {
    let mut state = skill_suggestion_state(false);
    state.input = "review Rust correctness".into();
    state.input_cursor = state.input.len();
    assert!(!render_to_text(&state, 100, 30).contains("Not activated:"));

    state.slash_palette_open = true;
    assert!(!render_to_text(&state, 100, 30).contains("Not activated:"));
    state.slash_palette_open = false;
    state.pager_open = true;
    assert!(!render_to_text(&state, 100, 30).contains("Not activated:"));
    state.pager_open = false;
    state.run_capabilities.skills = false;
    assert!(!render_to_text(&state, 100, 30).contains("Not activated:"));
    state.run_capabilities.skills = true;
    state.input = "/skills rust".into();
    assert!(!render_to_text(&state, 100, 30).contains("Not activated:"));
    state.input = "zebra quantum unrelated".into();
    assert!(!render_to_text(&state, 100, 30).contains("Not activated:"));
}

#[test]
fn mcp_status_snapshots() {
    let mut state = test_state();
    state.pager_open = true;
    state.pager_content = concat!(
        "MCP servers\n",
        "filesystem [stdio User/Running] oauth=none tools=4 active=2 resources=2\n",
        "project-db [stdio Project/Untrusted] oauth=none tools=0 active=0 resources=0\n",
        "network-api [streamable_http User/AuthorizationRequired] oauth=authorization_required tools=0 active=0 resources=0\n",
        "  error: MCP server network-api requires authorization; run /mcp connect network-api"
    )
    .into();
    insta::assert_snapshot!(
        "mcp_status",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );

    state.mcp_task_active = true;
    state.pager_content =
        "MCP authorization\n\nOpen login.example.test and enter code ABCD-EFGH\n\nWaiting for MCP authorization…"
            .into();
    insta::assert_snapshot!(
        "mcp_oauth_device",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );

    state.mcp_task_active = true;
    state.pager_content =
        "MCP authorization\n\nOpening the MCP authorization page…\n\nWaiting for MCP authorization… (Esc cancels)"
            .into();
    insta::assert_snapshot!(
        "mcp_oauth_browser",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );

    state.mcp_task_active = false;
    state.pager_content = concat!(
        "MCP authorization\n\n",
        "MCP server docs authorization connect failed\n\n",
        "Edit the server configuration or retry /mcp connect."
    )
    .into();
    insta::assert_snapshot!(
        "mcp_oauth_failure",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

#[test]
fn persistent_plan_tasks_snapshots() {
    let mut state = test_state();
    let timestamp = fixed_time();
    let plan_id =
        crate::work::PlanId(uuid::Uuid::parse_str("00000000-0000-0000-0000-000000000123").unwrap());
    let task_id =
        crate::work::TaskId(uuid::Uuid::parse_str("00000000-0000-0000-0000-000000000456").unwrap());
    let event_id = crate::work::WorkEventId(
        uuid::Uuid::parse_str("00000000-0000-0000-0000-000000000789").unwrap(),
    );
    *state.work_state.lock().unwrap() = crate::work::WorkState {
        pending_automation_proposal: None,
        plan_feedback: crate::plan_feedback::PlanFeedbackLedger::default(),
        plan: Some(crate::work::PlanArtifact {
            body: None,
            id: plan_id,
            revision: 4,
            status: crate::work::PlanStatus::Approved,
            title: "Persistent release plan".into(),
            created_at: timestamp,
            updated_at: timestamp,
            approved_at: Some(timestamp),
            approved_by: Some("user".into()),
            tasks: vec![crate::work::PlanTask {
                id: task_id,
                title: "Run repository validation".into(),
                status: crate::work::TaskStatus::Completed,
                owner: Some("main".into()),
                blocked_reason: None,
                created_at: timestamp,
                updated_at: timestamp,
                evidence: vec![crate::work::EvidenceRef {
                    event_id,
                    note: "cargo test passed".into(),
                }],
                agent: None,
                agent_runtime: crate::agent_graph::AgentTaskRuntime::default(),
                source_task_id: None,
            }],
            decisions: vec!["Keep JSON session storage".into()],
            source_plan_id: None,
        }),
        events: vec![crate::work::RecordedWorkEvent {
            id: event_id,
            kind: crate::work::WorkEventKind::Test,
            summary: "cargo test".into(),
            succeeded: true,
            created_at: timestamp,
        }],
        agent_graph: crate::agent_graph::AgentGraphState::default(),
        historical_notice: None,
    };
    crate::open_tasks(&mut state);
    insta::assert_snapshot!(
        "persistent_plan_tasks",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

#[test]
fn asynchronous_agents_snapshots() {
    let mut state = test_state();
    state.pager_open = true;
    state.pager_content = concat!(
        "Asynchronous child agents\n",
        "00000000-0000-0000-0000-000000000101 [Running] task=Inspect session migrations output=reading src/session.rs\n",
        "00000000-0000-0000-0000-000000000202 [WaitingApproval] task=Validate changes output=approval required for run_command\n",
        "00000000-0000-0000-0000-000000000303 [Completed] task=Review policy output=no findings"
    )
    .into();
    insta::assert_snapshot!(
        "asynchronous_agents",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

#[test]
fn fresh_context_review_snapshots() {
    let mut state = test_state();
    let result = crate::review::ReviewResult {
        summary: "Two correctness issues were found in the bounded working-tree diff.".into(),
        findings: vec![crate::review::ReviewFinding {
            severity: crate::review::ReviewSeverity::High,
            title: "Failed stream is marked complete".into(),
            rationale: "The failure path can drain queued input after an incomplete response."
                .into(),
            file: "src/main.rs".into(),
            line_start: 1840,
            line_end: 1848,
            confidence: 96,
            suggested_validation: "Add a fake-provider premature EOF test.".into(),
        }],
    };
    state.pager_open = true;
    state.pager_content = result.render("working tree");
    insta::assert_snapshot!(
        "fresh_context_review",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

#[test]
fn completed_tool_snapshots() {
    let mut state = test_state();
    state.messages.push(Message {
        role: Role::Assistant,
        content: "Read the requested file.".to_string(),
        tool_calls: Some(vec![ToolCall {
            id: "call-read".to_string(),
            name: "read_file".to_string(),
            arguments: r#"{"path":"src/main.rs"}"#.to_string(),
            status: ToolStatus::Done,
            output: Some("fn main() {\n    println!(\"hello\");\n}".to_string()),
        }]),
        thinking: None,
        timestamp: fixed_time(),
        response_items: Vec::new(),
    });
    insta::assert_snapshot!(
        "completed_tool",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

fn adaptive_tool_output_state(expanded: bool) -> AppState {
    let mut state = test_state();
    let id = "call-adaptive-output";
    let output = concat!(
        "exit code: 0\n",
        "stdout:\n",
        "Highest upload ratios\n",
        "rank\tsite\tid\tname\tratio\tuploaded_bytes\tdata_size_bytes\tcumulative_size_bytes\tcurrent_upload_Bps\n",
        "1\tOPS\t11952\tAstral Projection – Power Gen EP [FLAC]\t2.001586952\t737125708\t368270640\t1004897068\t0\n",
        "2\tRED\t31873\t佐井好子 – 1976.6.29 ライブ\t1.520707435\t395325056\t259961283\t3779522457\t\n",
    );
    state.messages = vec![
        Message {
            role: Role::Assistant,
            content: "I calculated the ratio-sorted selection.".into(),
            tool_calls: Some(vec![ToolCall {
                id: id.into(),
                name: "run_command".into(),
                arguments: r#"{"command":"python3 ratio_report.py"}"#.into(),
                status: ToolStatus::Done,
                output: Some(output.into()),
            }]),
            thinking: None,
            timestamp: fixed_time(),
            response_items: Vec::new(),
        },
        Message {
            role: Role::Assistant,
            content: "## Results\n\nThe saved TSV remains unchanged.".into(),
            tool_calls: None,
            thinking: None,
            timestamp: fixed_time(),
            response_items: Vec::new(),
        },
    ];
    if expanded {
        state
            .expanded_blocks
            .push(crate::app::TranscriptBlockKey::tool_calls(id).unwrap());
    }
    state
}

#[test]
fn adaptive_tool_output_is_bounded_at_wide_and_narrow_widths() {
    for width in [60, 100, 184] {
        let rendered = render_to_text(&adaptive_tool_output_state(false), width, 80);
        assert!(!rendered.contains('\t'));
        let content_rows = rendered
            .lines()
            .filter(|line| line.trim_start().starts_with("│ "))
            .collect::<Vec<_>>();
        assert!(
            !content_rows.is_empty(),
            "missing output card at width {width}"
        );
        assert!(
            content_rows
                .iter()
                .all(|line| line.trim_end().ends_with('│')),
            "open output card row at width {width}:\n{rendered}"
        );
    }
}

#[test]
fn adaptive_tool_output_snapshots() {
    insta::assert_snapshot!(
        "adaptive_tool_output_collapsed",
        wide_narrow(
            render_to_text(&adaptive_tool_output_state(false), 184, 34),
            render_to_text(&adaptive_tool_output_state(false), 60, 34)
        )
    );
    insta::assert_snapshot!(
        "adaptive_tool_output_expanded",
        wide_narrow(
            render_to_text(&adaptive_tool_output_state(true), 184, 38),
            render_to_text(&adaptive_tool_output_state(true), 60, 52)
        )
    );
}

#[test]
fn collapsing_adaptive_output_clears_the_expanded_frame() {
    let width = 100;
    let height = 50;
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut state = adaptive_tool_output_state(true);
    terminal
        .draw(|frame| crate::tui::render(frame, &state))
        .unwrap();
    state.expanded_blocks.clear();
    terminal
        .draw(|frame| crate::tui::render(frame, &state))
        .unwrap();
    let redrawn = terminal.backend().buffer().clone();
    let (_, fresh) = rendered_frame(&adaptive_tool_output_state(false), width, height);
    for y in 0..height {
        for x in 0..width {
            if x >= width - 2 || y >= 13 {
                assert_eq!(
                    redrawn[(x, y)],
                    fresh[(x, y)],
                    "stale cell outside the collapsed card at ({x}, {y})"
                );
            }
        }
    }
}

fn identical_spawn_failure_state() -> AppState {
    let mut state = test_state();
    let output = "no automatic delegation route qualifies for complex complexity; verify credentials, supported billing metadata, capability, and pricing freshness";
    state.messages.push(Message {
        role: Role::Assistant,
        content: "Splitting the review by feature domain.".into(),
        tool_calls: Some(
            [
                "Compare core agent and TUI behavior",
                "Compare multimodal and document support",
                "Compare provider and authentication support",
                "Compare memory and planning support",
            ]
            .into_iter()
            .enumerate()
            .map(|(index, task)| ToolCall {
                id: format!("spawn-{index}"),
                name: "spawn_agent".into(),
                arguments: serde_json::json!({
                    "task":task,
                    "tools":["read_file", "list_dir"],
                    "complexity":"complex"
                })
                .to_string(),
                status: ToolStatus::Error,
                output: Some(output.into()),
            })
            .collect(),
        ),
        thinking: None,
        timestamp: fixed_time(),
        response_items: Vec::new(),
    });
    state
}

#[test]
fn identical_spawn_failures_grouped_and_expanded_snapshots() {
    let collapsed = identical_spawn_failure_state();
    insta::assert_snapshot!(
        "spawn_failure_group_collapsed",
        wide_narrow(
            render_to_text(&collapsed, 100, 30),
            render_to_text(&collapsed, 60, 24)
        )
    );

    let mut expanded = collapsed;
    expanded
        .expanded_blocks
        .push(crate::app::TranscriptBlockKey::tool_calls("spawn-0").unwrap());
    insta::assert_snapshot!(
        "spawn_failure_group_expanded",
        wide_narrow(
            render_to_text(&expanded, 100, 34),
            render_to_text(&expanded, 60, 30)
        )
    );
}

#[test]
fn spawn_failure_grouping_preserves_calls_and_exact_contiguous_boundaries() {
    let state = identical_spawn_failure_state();
    let calls = state.messages[0].tool_calls.as_ref().unwrap();
    assert_eq!(calls.len(), 4);
    assert!(calls.iter().all(|call| call.status == ToolStatus::Error));
    assert!(calls
        .windows(2)
        .all(|pair| pair[0].output.as_deref() == pair[1].output.as_deref()));
    let rendered = render_to_text(&state, 100, 30);
    assert_eq!(rendered.matches("spawn_agent").count(), 1);
    assert!(rendered.contains("×4 identical failures"));
    assert_eq!(
        crate::tui::render::latest_expandable_block_key(&state),
        crate::app::TranscriptBlockKey::tool_calls("spawn-0")
    );

    let mut distinct = identical_spawn_failure_state();
    let distinct_calls = distinct.messages[0].tool_calls.as_mut().unwrap();
    distinct_calls[1].output = Some("different routing failure".into());
    assert_eq!(
        render_to_text(&distinct, 100, 30)
            .matches("spawn_agent")
            .count(),
        3
    );

    let mut noncontiguous = identical_spawn_failure_state();
    let noncontiguous_calls = noncontiguous.messages[0].tool_calls.as_mut().unwrap();
    noncontiguous_calls.truncate(2);
    noncontiguous_calls.insert(
        1,
        ToolCall {
            id: "list-between".into(),
            name: "list_agents".into(),
            arguments: "{}".into(),
            status: ToolStatus::Error,
            output: noncontiguous_calls[0].output.clone(),
        },
    );
    assert_eq!(
        render_to_text(&noncontiguous, 100, 30)
            .matches("spawn_agent")
            .count(),
        2
    );

    let mut active = identical_spawn_failure_state();
    let active_calls = active.messages[0].tool_calls.as_mut().unwrap();
    active_calls.truncate(2);
    active_calls[0].status = ToolStatus::Running;
    active_calls[1].status = ToolStatus::Done;
    assert_eq!(
        render_to_text(&active, 100, 30)
            .matches("spawn_agent")
            .count(),
        2
    );
}

#[test]
fn deferred_mcp_search_success_and_failure_snapshots() {
    let mut success = test_state();
    success.messages.push(Message {
        role: Role::Assistant,
        content: "I found the relevant MCP tools. Their exact schemas will be available on the next turn.".into(),
        tool_calls: Some(vec![ToolCall {
            id: "call-mcp-search".into(),
            name: "search_mcp_tools".into(),
            arguments: r#"{"query":"issues","server":"linear","limit":2}"#.into(),
            status: ToolStatus::Done,
            output: Some(
                "Activated 2 MCP tools for subsequent turns:\n- mcp__linear__list_issues [linear]\n- mcp__linear__read_issue [linear]"
                    .into(),
            ),
        }]),
        thinking: None,
        timestamp: fixed_time(),
        response_items: Vec::new(),
    });
    insta::assert_snapshot!(
        "deferred_mcp_search_success",
        wide_narrow(
            render_to_text(&success, 100, 30),
            render_to_text(&success, 60, 24)
        )
    );

    let mut failed = test_state();
    failed.messages.push(Message {
        role: Role::Assistant,
        content: "The server needs explicit network authority before discovery can run.".into(),
        tool_calls: Some(vec![ToolCall {
            id: "call-mcp-search-failed".into(),
            name: "search_mcp_tools".into(),
            arguments: r#"{"query":"issues","server":"remote"}"#.into(),
            status: ToolStatus::Error,
            output: Some(
                "MCP tool search could not inspect remote: a distinct network grant is required"
                    .into(),
            ),
        }]),
        thinking: None,
        timestamp: fixed_time(),
        response_items: Vec::new(),
    });
    insta::assert_snapshot!(
        "deferred_mcp_search_failure",
        wide_narrow(
            render_to_text(&failed, 100, 30),
            render_to_text(&failed, 60, 24)
        )
    );
}

#[test]
fn semantic_navigation_success_and_failure_snapshots() {
    let mut success = test_state();
    success.messages.push(Message {
        role: Role::Assistant,
        content: "The definition is in the current workspace.".into(),
        tool_calls: Some(vec![ToolCall {
            id: "call-lsp-definition".into(),
            name: "lsp_definition".into(),
            arguments: r#"{"server":"rust","path":"src/main.rs","line":42,"column":9}"#.into(),
            status: ToolStatus::Done,
            output: Some("Locations (1 shown)\nsrc/config.rs:17:12".into()),
        }]),
        thinking: None,
        timestamp: fixed_time(),
        response_items: Vec::new(),
    });
    insta::assert_snapshot!(
        "semantic_navigation_success",
        wide_narrow(
            render_to_text(&success, 100, 30),
            render_to_text(&success, 60, 24)
        )
    );

    let mut failed = test_state();
    failed.messages.push(Message {
        role: Role::Assistant,
        content: "The configured language server returned an unsafe location.".into(),
        tool_calls: Some(vec![ToolCall {
            id: "call-lsp-external".into(),
            name: "lsp_references".into(),
            arguments: r#"{"server":"rust","path":"src/main.rs","line":42,"column":9}"#.into(),
            status: ToolStatus::Error,
            output: Some("LSP response referenced a location outside workspace authority".into()),
        }]),
        thinking: None,
        timestamp: fixed_time(),
        response_items: Vec::new(),
    });
    insta::assert_snapshot!(
        "semantic_navigation_failure",
        wide_narrow(
            render_to_text(&failed, 100, 30),
            render_to_text(&failed, 60, 24)
        )
    );
}

#[test]
fn pdf_text_reading_success_and_failure_snapshots() {
    let mut success = test_state();
    success.messages.push(Message {
        role: Role::Assistant,
        content: "I extracted only the requested PDF pages within the configured bounds.".into(),
        tool_calls: Some(vec![ToolCall {
            id: "call-read-pdf".into(),
            name: "read_pdf".into(),
            arguments: r#"{"path":"docs/guide.pdf","start_page":2,"end_page":3}"#.into(),
            status: ToolStatus::Done,
            output: Some(
                "--- Page 2 ---\nSetup instructions\n\n--- Page 3 ---\nConfiguration details\n\nShowing pages 2-3 of 9 · 48120 bytes total\nOmitted pages: 1 before, 6 after\nNext range: {\"path\":\"docs/guide.pdf\",\"start_page\":4,\"end_page\":9}"
                    .into(),
            ),
        }]),
        thinking: None,
        timestamp: fixed_time(),
        response_items: Vec::new(),
    });
    insta::assert_snapshot!(
        "pdf_text_reading_success",
        wide_narrow(
            render_to_text(&success, 100, 30),
            render_to_text(&success, 60, 24)
        )
    );

    let mut failed = test_state();
    failed.messages.push(Message {
        role: Role::Assistant,
        content: "The PDF needs to be decrypted before its text can be read.".into(),
        tool_calls: Some(vec![ToolCall {
            id: "call-read-pdf-encrypted".into(),
            name: "read_pdf".into(),
            arguments: r#"{"path":"docs/private.pdf"}"#.into(),
            status: ToolStatus::Error,
            output: Some("Error reading PDF: encrypted PDFs are not supported".into()),
        }]),
        thinking: None,
        timestamp: fixed_time(),
        response_items: Vec::new(),
    });
    insta::assert_snapshot!(
        "pdf_text_reading_failure",
        wide_narrow(
            render_to_text(&failed, 100, 30),
            render_to_text(&failed, 60, 24)
        )
    );
}

#[test]
fn multimodal_attachment_and_image_tool_snapshots() {
    let mut processing = test_state();
    processing.image_attachment_busy = true;
    processing.status_message = Some("Processing clipboard image…".into());
    insta::assert_snapshot!(
        "multimodal_clipboard_processing",
        wide_narrow(
            render_to_text(&processing, 100, 30),
            render_to_text(&processing, 60, 24)
        )
    );

    let mut success = test_state();
    success.messages.push(Message {
        role: Role::Assistant,
        content: "I inspected the normalized image without exposing its bytes.".into(),
        tool_calls: Some(vec![ToolCall {
            id: "call-view-image".into(),
            name: "view_image".into(),
            arguments: r#"{"path":"screenshots/example.png"}"#.into(),
            status: ToolStatus::Done,
            output: Some("[image/png 1280×720; image available only in this process]".into()),
        }]),
        thinking: None,
        timestamp: fixed_time(),
        response_items: Vec::new(),
    });
    insta::assert_snapshot!(
        "multimodal_image_tool_success",
        wide_narrow(
            render_to_text(&success, 100, 30),
            render_to_text(&success, 60, 24)
        )
    );

    let mut failed = test_state();
    failed.status_message = Some("Selected model profile does not support images".into());
    insta::assert_snapshot!(
        "multimodal_incompatible_model",
        wide_narrow(
            render_to_text(&failed, 100, 30),
            render_to_text(&failed, 60, 24)
        )
    );
}

#[test]
fn bounded_tool_output_snapshots() {
    let mut state = test_state();
    state.messages.push(Message {
        role: Role::Assistant,
        content: "Read a bounded section and report what remains.".into(),
        tool_calls: Some(vec![ToolCall {
            id: "call-bounded-read".into(),
            name: "read_file".into(),
            arguments: r#"{"path":"src/main.rs","start_line":1,"end_line":200}"#.into(),
            status: ToolStatus::Done,
            output: Some(
                "fn main() {\n    // beginning retained\n[… 48120 bytes omitted …]\n}\n\nShowing lines 1-200 of 8240 · 285100 bytes total\nOmitted lines: 0 before, 8040 after\nNext range: {\"path\":\"src/main.rs\",\"start_line\":201,\"end_line\":400}"
                    .into(),
            ),
        }]),
        thinking: None,
        timestamp: fixed_time(),
        response_items: Vec::new(),
    });

    insta::assert_snapshot!(
        "bounded_tool_output",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

#[test]
fn approval_prompt_snapshots() {
    let mut state = test_state();
    state.messages.push(Message {
        role: Role::User,
        content: "Run the test suite".to_string(),
        tool_calls: None,
        thinking: None,
        timestamp: fixed_time(),
        response_items: Vec::new(),
    });
    state.approval_queue.begin_batch(vec![PendingApproval {
        tool_name: "run_command".to_string(),
        tool_args: r#"{"command":"cargo test"}"#.to_string(),
        tool_id: "call-approval".to_string(),
        network_requested: false,
        email_preview: None,
        email_draft_ref: None,
    }]);
    insta::assert_snapshot!(
        "approval_prompt",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

#[test]
fn approval_prompt_shows_the_complete_command_at_every_width() {
    let mut state = test_state();
    let command = concat!(
        "ssh bsd 'pgrep -af \"do_copy|cp -a\" || true; ",
        "printf command-tail-must-remain-visible'"
    );
    state.approval_queue.begin_batch(vec![PendingApproval {
        tool_name: "run_command".into(),
        tool_args: serde_json::json!({
            "command": command,
            "cwd": "/Users/aji/project/ca",
            "network": true,
        })
        .to_string(),
        tool_id: "call-long-approval".into(),
        network_requested: true,
        email_preview: None,
        email_draft_ref: None,
    }]);

    for width in [100, 60] {
        let rendered = render_to_text(&state, width, 30);
        assert!(rendered.contains("command-tail-must-remain-visible"));
        assert!(rendered.contains("ssh bsd"));
    }
}

fn privilege_approval_state() -> AppState {
    let mut state = test_state();
    state.messages.push(Message {
        role: Role::User,
        content: "Apply the approved system change".into(),
        tool_calls: None,
        thinking: None,
        timestamp: fixed_time(),
        response_items: Vec::new(),
    });
    let invocation = crate::privilege::PreparedPrivilegeInvocation::display_fixture_for_test(
        "/usr/bin/id",
        vec!["space value".into(), "$(literal);&|".into()],
        "/Users/example/project",
        crate::sandbox::SandboxMode::Required,
        &"a".repeat(64),
    );
    state.manual_resolutions.begin(vec![
        crate::interaction::PendingManualResolution::Privilege(Box::new(
            crate::privilege::PendingPrivilegeApproval {
                tool_id: "call-privilege".into(),
                tool_args: r#"{"program":"/usr/bin/id","args":["space value","$(literal);&|"]}"#
                    .into(),
                invocation,
            },
        )),
    ]);
    state.approval_queue.begin_batch(vec![PendingApproval {
        tool_name: crate::privilege::TOOL_NAME.into(),
        tool_args: r#"{"program":"/usr/bin/id","args":["space value","$(literal);&|"]}"#.into(),
        tool_id: "call-privilege".into(),
        network_requested: false,
        email_preview: None,
        email_draft_ref: None,
    }]);
    state
}

#[test]
fn privilege_approval_and_handoff_phase_snapshots() {
    let approval = privilege_approval_state();
    insta::assert_snapshot!(
        "privilege_approval",
        wide_narrow(
            render_to_text(&approval, 100, 34),
            render_to_text(&approval, 60, 30)
        )
    );

    let render_phases = |width, height| {
        let mut states = Vec::new();

        let mut checking = privilege_approval_state();
        checking.approval_queue.mark_in_flight();
        checking.status_message = Some("Checking administrator authorization…".into());
        states.push(("CACHE CHECKING", checking));

        let mut masked = privilege_approval_state();
        masked.approval_queue.mark_in_flight();
        let mut interaction = crate::interaction::InteractionState::host(
            crate::interaction::InteractionRequest::host_masked(
                "Administrator password",
                Some(format!(
                    "One keyboard-only attempt for invocation {}",
                    "a".repeat(64)
                )),
            ),
        );
        for character in "never-render-this-password".chars() {
            interaction.handle_key(
                crossterm::event::KeyCode::Char(character),
                crossterm::event::KeyModifiers::NONE,
            );
        }
        masked.interaction = Some(interaction);
        masked.status_message = Some("Administrator authentication required (Esc cancels)".into());
        states.push(("MASKED INPUT", masked));

        for (label, status) in [
            ("CANCELLATION", "Cancelling administrator authentication…"),
            ("AUTH FAILURE", "Administrator authentication failed"),
            ("EXECUTION", "Executing approved administrator command…"),
            ("COMPLETION", "Administrator command completed"),
        ] {
            let mut state = privilege_approval_state();
            state.approval_queue.mark_in_flight();
            state.status_message = Some(status.into());
            states.push((label, state));
        }

        states
            .into_iter()
            .map(|(label, state)| {
                format!("=== {label} ===\n{}", render_to_text(&state, width, height))
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let wide = render_phases(100, 34);
    let narrow = render_phases(60, 30);
    assert!(!wide.contains("never-render-this-password"));
    assert!(!narrow.contains("never-render-this-password"));
    insta::assert_snapshot!("privilege_handoff_phases", wide_narrow(wide, narrow));
}

#[test]
fn outbound_email_approval_snapshots() {
    let mut state = test_state();
    state.messages.push(Message {
        role: Role::User,
        content: "Send the frozen draft".into(),
        tool_calls: None,
        thinking: None,
        timestamp: fixed_time(),
        response_items: Vec::new(),
    });
    state.approval_queue.begin_batch(vec![PendingApproval {
        tool_name: "email_send".into(),
        tool_args: crate::email::private_arguments_marker("email_send"),
        tool_id: "call-email-approval".into(),
        network_requested: false,
        email_preview: Some(Box::new(
            crate::email::types::ExternalCommunicationPreview {
                account_id: "work".into(),
                to: vec!["alice@example.com".into(), "bob@example.com".into()],
                cc: vec!["copy@example.com".into()],
                bcc: vec!["audit@example.com".into()],
                subject: "Quarterly follow-up".into(),
                body_bytes: 248,
                body_sha256: "a".repeat(64),
                draft_sha256: "b".repeat(64),
                attachments: vec![crate::email::types::AttachmentPreview {
                    basename: "report.pdf".into(),
                    size: 4096,
                    sha256: "c".repeat(64),
                }],
            },
        )),
        email_draft_ref: Some("draft_00000000000000000000000000000000".into()),
    }]);
    insta::assert_snapshot!(
        "outbound_email_approval",
        wide_narrow(
            render_to_text(&state, 100, 34),
            render_to_text(&state, 60, 28)
        )
    );
}

#[test]
fn multi_tool_approval_progress_snapshots() {
    let mut state = test_state();
    state.messages.push(Message {
        role: Role::User,
        content: "Edit multiple files".to_string(),
        tool_calls: None,
        thinking: None,
        timestamp: fixed_time(),
        response_items: Vec::new(),
    });
    state.approval_queue.begin_batch(vec![
        PendingApproval {
            tool_name: "write_file".to_string(),
            tool_args: r#"{"path":"src/foo.rs"}"#.to_string(),
            tool_id: "call-1".to_string(),
            network_requested: false,
            email_preview: None,
            email_draft_ref: None,
        },
        PendingApproval {
            tool_name: "run_command".to_string(),
            tool_args: r#"{"command":"cargo check"}"#.to_string(),
            tool_id: "call-2".to_string(),
            network_requested: false,
            email_preview: None,
            email_draft_ref: None,
        },
        PendingApproval {
            tool_name: "edit_file".to_string(),
            tool_args: r#"{"path":"src/bar.rs"}"#.to_string(),
            tool_id: "call-3".to_string(),
            network_requested: false,
            email_preview: None,
            email_draft_ref: None,
        },
    ]);
    // Should show [1/3]
    insta::assert_snapshot!(
        "approval_multi_tool_1_of_3",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );

    // Resolve first → advance to [2/3]
    state.approval_queue.resolve();
    insta::assert_snapshot!(
        "approval_multi_tool_2_of_3",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

#[test]
fn model_picker_snapshots() {
    let mut state = test_state();
    state.model_picker_open = true;
    state.model_catalog_loaded = true;
    state.model_catalog_status = Some("4 models · 2 provider(s) live".into());
    state.model_list = vec![
        ModelInfo {
            id: "mimo-v2.5".to_string(),
            provider: "mimo".to_string(),
            display_name: "MiMo v2.5".to_string(),
        },
        ModelInfo {
            id: "mimo-v2.5-pro".to_string(),
            provider: "mimo".to_string(),
            display_name: "mimo-v2.5-pro".to_string(),
        },
        ModelInfo {
            id: "mimo-v2.5-omni".to_string(),
            provider: "mimo".to_string(),
            display_name: "mimo-v2.5-omni".to_string(),
        },
        ModelInfo {
            id: "local-coder".to_string(),
            provider: "ollama".to_string(),
            display_name: "Local Coder".to_string(),
        },
    ];
    state.current_model = Some(state.model_list[1].clone());
    state
        .model_catalog_provider_status
        .insert("mimo".into(), "live · 3 models".into());
    state
        .model_catalog_provider_status
        .insert("ollama".into(), "live · 1 model".into());
    insta::assert_snapshot!(
        "model_picker",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

#[test]
fn model_picker_favorites_and_recent_sections_snapshots() {
    let mut state = test_state();
    state.model_picker_open = true;
    state.model_catalog_loaded = true;
    state.model_catalog_status = Some("4 models · 2 provider(s) configured".into());
    state.model_list = vec![
        ModelInfo {
            id: "one".into(),
            provider: "alpha".into(),
            display_name: "Alpha One".into(),
        },
        ModelInfo {
            id: "two".into(),
            provider: "alpha".into(),
            display_name: "Alpha Two".into(),
        },
        ModelInfo {
            id: "spare".into(),
            provider: "alpha".into(),
            display_name: "Alpha Spare".into(),
        },
        ModelInfo {
            id: "three".into(),
            provider: "beta".into(),
            display_name: "Beta Three".into(),
        },
    ];
    state
        .model_preferences
        .toggle_favorite(&state.model_list[3])
        .unwrap();
    state
        .model_preferences
        .record_selection(&state.model_list[0], chrono::Utc::now())
        .unwrap();
    state
        .model_preferences
        .record_selection(&state.model_list[1], chrono::Utc::now())
        .unwrap();
    state.current_model = Some(state.model_list[1].clone());

    insta::assert_snapshot!(
        "model_picker_preferences",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

#[test]
fn model_picker_loading_snapshots() {
    let mut state = test_state();
    state.model_picker_open = true;
    state.model_catalog_refreshing = true;
    state.model_catalog_status = Some("Refreshing provider models…".into());
    state.model_list = vec![ModelInfo {
        id: "mimo-v2.5".into(),
        provider: "mimo".into(),
        display_name: "mimo-v2.5".into(),
    }];

    insta::assert_snapshot!(
        "model_picker_loading",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

#[test]
fn model_picker_documented_mimo_fallback_snapshots() {
    let mut state = test_state();
    state.model_picker_open = true;
    state.model_catalog_loaded = true;
    state.model_filter = "mimo".into();
    state.model_catalog_status = Some("5 models · 0 live · 1 fallback · 4 unavailable".into());
    state.model_list = vec![
        ModelInfo {
            id: "mimo-v2.5".into(),
            provider: "mimo".into(),
            display_name: "mimo-v2.5".into(),
        },
        ModelInfo {
            id: "mimo-v2.5-pro".into(),
            provider: "mimo".into(),
            display_name: "mimo-v2.5-pro".into(),
        },
    ];
    state.current_model = Some(state.model_list[0].clone());
    state
        .model_catalog_provider_status
        .insert("mimo".into(), "fallback · API key env is not set".into());

    insta::assert_snapshot!(
        "model_picker_documented_mimo_fallback",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

#[test]
fn model_picker_documented_codex_fallback_snapshots() {
    let mut state = test_state();
    state.model_picker_open = true;
    state.model_catalog_loaded = true;
    state.model_filter = "openai".into();
    state.model_catalog_status = Some("3 models · 0 live · 1 fallback".into());
    state.model_list = ["gpt-5.6-luna", "gpt-5.6-sol", "gpt-5.6-terra"]
        .into_iter()
        .map(|model| ModelInfo {
            id: model.into(),
            provider: "openai-codex".into(),
            display_name: model.into(),
        })
        .collect();
    state.current_model = Some(state.model_list[1].clone());
    state.selected_model_idx = 1;
    state.model_catalog_provider_status.insert(
        "openai-codex".into(),
        "fallback · models request failed".into(),
    );

    insta::assert_snapshot!(
        "model_picker_documented_codex_fallback",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

fn native_codex_picker_fixture() -> (AppState, crate::config::CaConfig) {
    let mut state = test_state();
    let mut config = crate::config::CaConfig::default();
    let mut provider =
        crate::provider_connect::FirstClassProvider::OpenAiCodex.candidate(String::new(), None);
    provider.models.push("z-configured-only".into());
    config.providers = std::collections::HashMap::from([("openai-codex".into(), provider)]);
    state.current_model = Some(ModelInfo {
        id: "gpt-5.6-sol".into(),
        display_name: "gpt-5.6-sol".into(),
        provider: "openai-codex".into(),
    });
    state
        .model_preferences
        .record_selection(state.current_model.as_ref().unwrap(), chrono::Utc::now())
        .unwrap();
    state.model_picker_open = true;
    (state, config)
}

#[test]
fn model_picker_native_codex_live_cached_and_configured_snapshots() {
    let (mut state, config) = native_codex_picker_fixture();
    let models = crate::model_catalog::codex_fixture_models();
    crate::apply_model_catalog_refresh(
        &mut state,
        &config,
        crate::model_catalog::CatalogRefresh {
            catalogs: std::collections::HashMap::from([("openai-codex".into(), models.clone())]),
            documented: std::collections::HashMap::from([(
                "openai-codex".into(),
                vec!["not-live-fallback".into()],
            )]),
            failures: vec![],
        },
    );
    let mut wide_pages = String::new();
    let mut narrow_pages = String::new();
    for (page, index) in [("recent", 0), ("middle", 4), ("configured", 7)] {
        state.selected_model_idx = index;
        let wide = render_to_text(&state, 100, 30);
        let narrow = render_to_text(&state, 60, 24);
        wide_pages.push_str(&wide);
        narrow_pages.push_str(&narrow);
        insta::assert_snapshot!(
            format!("model_picker_native_codex_{page}"),
            wide_narrow(wide, narrow)
        );
    }
    for model in &models {
        assert!(wide_pages.contains(model), "wide missing {model}");
        assert!(narrow_pages.contains(model), "narrow missing {model}");
    }
    assert!(narrow_pages.contains("configured only"));
    assert!(!wide_pages.contains("not-live-fallback"));
    state.selected_model_idx = 4;
    crate::apply_model_catalog_refresh(
        &mut state,
        &config,
        crate::model_catalog::CatalogRefresh {
            failures: vec![(
                "openai-codex".into(),
                "models request failed (HTTP 503)".into(),
            )],
            ..Default::default()
        },
    );
    insta::assert_snapshot!(
        "model_picker_native_codex_cached",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );

    state.model_picker_open = false;
    let mut manager = provider_manager_state();
    manager.providers = vec![crate::config::ProviderManagementEntry {
        id: "openai-codex".into(),
        base_url: config.providers["openai-codex"].base_url.clone(),
        api_key_source: crate::config::ProviderApiKeySource::None,
        configured_models: config.providers["openai-codex"].models.clone(),
        configured: true,
        connected: true,
    }];
    manager.view = crate::app::ProviderManagerView::Models {
        provider: "openai-codex".into(),
    };
    manager.selected_model_idx = 7;
    state.provider_manager = Some(manager);
    crate::apply_provider_catalog_refresh(
        &mut state,
        &config,
        crate::model_catalog::CatalogRefresh {
            catalogs: std::collections::HashMap::from([("openai-codex".into(), models)]),
            ..Default::default()
        },
        "openai-codex",
    );
    assert_eq!(
        state.provider_manager.as_ref().unwrap().selected_model_idx,
        7
    );
    insta::assert_snapshot!(
        "provider_models_native_codex_live",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

#[tokio::test]
#[ignore = "manual authenticated catalog-only verification; no inference or config/session writes"]
async fn native_codex_live_catalog_matches_picker() {
    let (mut state, mut config) = native_codex_picker_fixture();
    config
        .providers
        .get_mut("openai-codex")
        .unwrap()
        .models
        .retain(|model| model != "z-configured-only");
    let refresh = crate::model_catalog::discover_provider_models(config.providers.clone()).await;
    // Only sanitized errors and model identifiers may leave this verification.
    assert!(
        refresh.failures.is_empty(),
        "catalog failed: {:?}",
        refresh.failures
    );
    let live = refresh
        .catalogs
        .get("openai-codex")
        .expect("native live response")
        .clone();
    assert!(!live.is_empty(), "account returned no visible models");
    crate::apply_model_catalog_refresh(&mut state, &config, refresh);
    assert_eq!(state.discovered_models["openai-codex"], live);
    assert_eq!(state.codex_catalogs["openai-codex"], live);
    let picker_live: Vec<_> = state
        .model_list
        .iter()
        .filter(|model| !state.model_is_configured_only(model))
        .map(|model| model.id.clone())
        .collect();
    assert_eq!(picker_live, live);
    println!(
        "LIVE_CODEX_CATALOG {} models: {}",
        live.len(),
        live.join(", ")
    );
    let mut pages = String::new();
    for index in 0..state.model_list.len() {
        state.selected_model_idx = index;
        pages.push_str(&wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24),
        ));
        pages.push('\n');
    }
    for model in live {
        assert!(pages.contains(&model), "picker did not render {model}");
    }
    println!("{pages}");
}

#[test]
fn model_picker_current_deepseek_fallback_snapshots() {
    let mut state = test_state();
    state.model_picker_open = true;
    state.model_catalog_loaded = true;
    state.model_filter = "deepseek".into();
    state.model_catalog_status = Some("6 models · 0 live · 2 fallback · 4 unavailable".into());
    state.model_list = vec![
        ModelInfo {
            id: "deepseek-v4-flash".into(),
            provider: "deepseek".into(),
            display_name: "deepseek-v4-flash".into(),
        },
        ModelInfo {
            id: "deepseek-v4-pro".into(),
            provider: "deepseek".into(),
            display_name: "deepseek-v4-pro".into(),
        },
    ];
    state.current_model = Some(state.model_list[0].clone());
    state.model_catalog_provider_status.insert(
        "deepseek".into(),
        "fallback · API key env is not set".into(),
    );

    insta::assert_snapshot!(
        "model_picker_current_deepseek_fallback",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

#[test]
fn model_picker_long_catalog_scroll_snapshots() {
    let mut state = test_state();
    state.model_picker_open = true;
    state.model_catalog_loaded = true;
    state.model_catalog_status = Some("100 models · 1 provider live".into());
    state.model_list = (0..100)
        .map(|index| ModelInfo {
            id: format!("model-{index:03}"),
            provider: "ofox".into(),
            display_name: format!("model-{index:03}"),
        })
        .collect();
    state.current_model = Some(state.model_list[0].clone());
    state.selected_model_idx = 99;
    state
        .model_catalog_provider_status
        .insert("ofox".into(), "live · 100 models".into());

    insta::assert_snapshot!(
        "model_picker_long_catalog_scroll",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

#[test]
fn model_picker_no_matches_snapshots() {
    let mut state = test_state();
    state.model_picker_open = true;
    state.model_catalog_loaded = true;
    state.model_filter = "not-there".into();
    state.model_catalog_status = Some("2 configured models".into());
    state.model_list = vec![ModelInfo {
        id: "available".into(),
        provider: "local".into(),
        display_name: "available".into(),
    }];

    insta::assert_snapshot!(
        "model_picker_no_matches",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

#[test]
fn provider_editor_snapshots() {
    let mut state = test_state();
    state.provider_editor = Some(crate::app::ProviderEditorState {
        origin: crate::app::ProviderEditorOrigin::ModelPicker,
        mode: crate::app::ProviderEditorMode::EditModel,
        draft: crate::app::ProviderModelDraft {
            original_model: Some("mimo-v2.5".into()),
            provider: "mimo".into(),
            base_url: "https://token-plan-sgp.xiaomimimo.com/v1".into(),
            api_key_env: "MIMO_API_KEY".into(),
            model: "mimo-v2.5".into(),
        },
        api_key: crate::provider_secrets::SecretValue::default(),
        credential_configured: true,
        credential_in_keychain: false,
        protocol_override: None,
        prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
        field: crate::app::ProviderEditorField::BaseUrl,
        cursor: "https://token-plan-sgp.xiaomimimo.com/v1".len(),
        error: None,
        saving: false,
    });

    insta::assert_snapshot!(
        "provider_editor",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

#[test]
fn provider_editor_add_model_snapshots() {
    let mut state = test_state();
    state.provider_editor = Some(crate::app::ProviderEditorState {
        origin: crate::app::ProviderEditorOrigin::ModelPicker,
        mode: crate::app::ProviderEditorMode::AddModel,
        draft: crate::app::ProviderModelDraft {
            original_model: None,
            provider: "mimo".into(),
            base_url: "https://token-plan-sgp.xiaomimimo.com/v1".into(),
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
        error: None,
        saving: false,
    });

    insta::assert_snapshot!(
        "provider_editor_add_model",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

#[test]
fn direct_openai_provider_editor_legacy_prompt_cache_snapshots() {
    let mut state = test_state();
    state.provider_editor = Some(crate::app::ProviderEditorState {
        origin: crate::app::ProviderEditorOrigin::ProviderManager,
        mode: crate::app::ProviderEditorMode::EditProvider,
        draft: crate::app::ProviderModelDraft {
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

    insta::assert_snapshot!(
        "provider_editor_legacy_prompt_cache",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

#[test]
fn provider_editor_add_provider_snapshots() {
    let mut state = test_state();
    state.provider_editor = Some(crate::app::ProviderEditorState {
        origin: crate::app::ProviderEditorOrigin::ModelPicker,
        mode: crate::app::ProviderEditorMode::AddProvider,
        draft: crate::app::ProviderModelDraft {
            original_model: None,
            provider: String::new(),
            base_url: String::new(),
            api_key_env: String::new(),
            model: String::new(),
        },
        api_key: crate::provider_secrets::SecretValue::from_text("not-a-real-key"),
        credential_configured: false,
        credential_in_keychain: false,
        protocol_override: None,
        prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
        field: crate::app::ProviderEditorField::ApiKey,
        cursor: "not-a-real-key".len(),
        error: None,
        saving: false,
    });

    insta::assert_snapshot!(
        "provider_editor_add_provider",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

#[test]
fn provider_editor_openai_template_snapshots() {
    let mut state = test_state();
    state.provider_editor = Some(crate::app::ProviderEditorState {
        origin: crate::app::ProviderEditorOrigin::ProviderSetup,
        mode: crate::app::ProviderEditorMode::AddProvider,
        draft: crate::app::ProviderModelDraft {
            original_model: None,
            provider: crate::config::OPENAI_PROVIDER_TEMPLATE_ID.into(),
            base_url: crate::config::OPENAI_PROVIDER_TEMPLATE_BASE_URL.into(),
            api_key_env: crate::config::OPENAI_PROVIDER_TEMPLATE_API_KEY_ENV.into(),
            model: crate::config::OPENAI_PROVIDER_TEMPLATE_MODEL.into(),
        },
        api_key: crate::provider_secrets::SecretValue::default(),
        credential_configured: false,
        credential_in_keychain: false,
        protocol_override: Some(crate::agent::ProviderProtocol::OpenAiResponses),
        prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
        field: crate::app::ProviderEditorField::ApiKey,
        cursor: 0,
        error: None,
        saving: false,
    });

    insta::assert_snapshot!(
        "provider_editor_openai_template",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

#[test]
fn provider_editor_anthropic_template_snapshots() {
    let mut state = test_state();
    let template = crate::provider_templates::get("anthropic").unwrap();
    state.provider_editor = Some(crate::app::ProviderEditorState {
        mode: crate::app::ProviderEditorMode::AddProvider,
        origin: crate::app::ProviderEditorOrigin::ProviderSetup,
        draft: crate::app::ProviderModelDraft {
            original_model: None,
            provider: template.provider_id.into(),
            base_url: template.base_url.unwrap().into(),
            api_key_env: template.api_key_env.unwrap().into(),
            model: template.starter_model.unwrap().into(),
        },
        api_key: crate::provider_secrets::SecretValue::default(),
        credential_configured: false,
        credential_in_keychain: false,
        protocol_override: template.protocol,
        prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
        field: crate::app::ProviderEditorField::ApiKey,
        cursor: 0,
        error: None,
        saving: false,
    });

    insta::assert_snapshot!(
        "provider_editor_anthropic_template",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

fn provider_manager_state() -> crate::app::ProviderManagerState {
    crate::app::ProviderManagerState {
        view: crate::app::ProviderManagerView::Providers,
        providers: vec![
            crate::config::ProviderManagementEntry {
                id: "mimo".into(),
                base_url: "https://token-plan-sgp.xiaomimimo.com/v1".into(),
                api_key_source: crate::config::ProviderApiKeySource::Environment(
                    "MIMO_API_KEY".into(),
                ),
                configured_models: vec!["mimo-v2.5".into(), "mimo-v2.5-pro".into()],
                configured: true,
                connected: true,
            },
            crate::config::ProviderManagementEntry {
                id: "empty-local".into(),
                base_url: "http://127.0.0.1:4000/v1".into(),
                api_key_source: crate::config::ProviderApiKeySource::None,
                configured_models: Vec::new(),
                configured: true,
                connected: false,
            },
            crate::config::ProviderManagementEntry {
                id: "saved".into(),
                base_url: "https://saved.invalid/v1".into(),
                api_key_source: crate::config::ProviderApiKeySource::Keychain,
                configured_models: vec!["saved-model".into()],
                configured: true,
                connected: true,
            },
        ],
        provider_filter: String::new(),
        selected_provider_idx: 0,
        model_filter: String::new(),
        selected_model_idx: 1,
        template_filter: String::new(),
        selected_template_idx: 0,
        delete_confirmation: None,
        busy: false,
        error: None,
    }
}

#[test]
fn provider_manager_list_empty_and_no_match_snapshots() {
    let mut state = test_state();
    state.current_model = Some(ModelInfo {
        id: "mimo-v2.5-pro".into(),
        provider: "mimo".into(),
        display_name: "mimo-v2.5-pro".into(),
    });
    state.provider_manager = Some(provider_manager_state());
    insta::assert_snapshot!(
        "provider_manager_list",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );

    state.provider_manager.as_mut().unwrap().error =
        Some("openai exists; template made no changes".into());
    insta::assert_snapshot!(
        "provider_manager_openai_duplicate_narrow",
        render_to_text(&state, 60, 24)
    );
    state.provider_manager.as_mut().unwrap().error = None;

    state.provider_manager.as_mut().unwrap().providers.clear();
    insta::assert_snapshot!(
        "provider_manager_empty",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );

    state.provider_manager = Some(provider_manager_state());
    state.provider_manager.as_mut().unwrap().provider_filter = "not-there".into();
    insta::assert_snapshot!(
        "provider_manager_no_match",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

#[test]
fn provider_auth_remote_device_and_bare_key_snapshots() {
    let mut state = test_state();
    state.provider_auth = Some(crate::app::ProviderAuthState {
        provider: crate::provider_connect::FirstClassProvider::OpenAiCodex,
        stage: crate::app::ProviderAuthStage::WaitingDevice,
        url: Some("https://auth.openai.com/codex/device".into()),
        code: Some("ABCD-EFGH".into()),
        prompt: "Open the URL on any browser, enter the one-time code, and leave ca running."
            .into(),
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
        generation: 7,
    });
    insta::assert_snapshot!(
        "provider_auth_device",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 52, 24)
        )
    );

    state.provider_auth = Some(crate::app::ProviderAuthState {
        provider: crate::provider_connect::FirstClassProvider::Mimo,
        stage: crate::app::ProviderAuthStage::SecretInput,
        url: Some(
            concat!(
                "https://platform.xiaomimimo.com/authorize?",
                "pk=MCowBQYDK2VuAyEA0123456789012345678901234567890123456789012&",
                "redirect_uri=https%3A%2F%2Fplatform.xiaomimimo.com%2Fauthorize%2Fcode%2Fcallback&",
                "kn=mimocode&key_name=mimo-code-cli-key-01234567"
            )
            .into(),
        ),
        code: None,
        prompt: "Open the URL on any device, sign in, then paste the encrypted one-time result"
            .into(),
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
        generation: 8,
    });
    let wide = render_to_text(&state, 100, 30);
    assert!(!wide.contains("pk=MCow"));
    assert!(wide.contains("borderless selectable handoff"));
    insta::assert_snapshot!(
        "provider_auth_mimo_manual",
        wide_narrow(wide, render_to_text(&state, 52, 24))
    );

    state.provider_auth = Some(crate::app::ProviderAuthState {
        provider: crate::provider_connect::FirstClassProvider::DeepSeek,
        stage: crate::app::ProviderAuthStage::SecretInput,
        url: None,
        code: None,
        prompt: "Paste your DeepSeek API key".into(),
        input: crate::provider_secrets::SecretValue::from_text("never-render-this-secret"),
        cursor: 24,
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
        bare: true,
        generation: 8,
    });
    let wide = render_to_text(&state, 100, 30);
    assert!(!wide.contains("never-render-this-secret"));
    insta::assert_snapshot!(
        "provider_auth_bare_key",
        wide_narrow(wide, render_to_text(&state, 52, 24))
    );
}

#[test]
fn provider_auth_mimo_token_plan_setup_snapshots_are_secret_free() {
    let mut state = test_state();
    crate::tui::input::begin_provider_auth(
        &mut state,
        crate::provider_connect::FirstClassProvider::Mimo,
    );
    insta::assert_snapshot!(
        "provider_auth_mimo_method_picker",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 52, 24)
        )
    );

    let mut saved = test_state();
    saved.provider_manager = Some(provider_manager_state());
    crate::tui::input::begin_provider_auth(
        &mut saved,
        crate::provider_connect::FirstClassProvider::Mimo,
    );
    let saved_wide = render_to_text(&saved, 100, 30);
    assert!(saved_wide.contains("Use saved Token Plan credential (Singapore; no sign-in)"));
    assert!(saved_wide.contains("Browser authorization (API or Token Plan)"));
    assert!(saved_wide.contains("20% off during off-peak hours"));
    assert!(saved_wide.contains("9:00 AM–5:00 PM PDT"));
    insta::assert_snapshot!(
        "provider_auth_mimo_saved_method",
        wide_narrow(saved_wide, render_to_text(&saved, 52, 24))
    );

    let auth = state.provider_auth.as_mut().unwrap();
    auth.mimo_method = Some(crate::app::MimoSetupMethod::ManualTokenPlan);
    auth.stage = crate::app::ProviderAuthStage::RegionPicker;
    auth.selected_index = 1;
    auth.prompt = "Select the region shown on your Xiaomi Token Plan page".into();
    insta::assert_snapshot!(
        "provider_auth_mimo_region_picker_wide",
        render_to_text(&state, 100, 30)
    );

    let auth = state.provider_auth.as_mut().unwrap();
    auth.stage = crate::app::ProviderAuthStage::SecretInput;
    auth.mimo_region = Some(crate::pricing::MimoRegion::Singapore);
    auth.mimo_tier = Some(crate::pricing::MimoPlanTier::Lite);
    auth.mimo_term = Some(crate::pricing::MimoPlanTerm::Monthly);
    auth.input = crate::provider_secrets::SecretValue::from_text("tp-never-render-this-key");
    auth.cursor = auth.input.len();
    auth.prompt = "Paste the Token Plan API key (tp-…)".into();
    let masked = render_to_text(&state, 100, 30);
    assert!(!masked.contains("never-render-this-key"));
    insta::assert_snapshot!("provider_auth_mimo_token_key_masked_wide", masked);

    let auth = state.provider_auth.as_mut().unwrap();
    auth.stage = crate::app::ProviderAuthStage::PriceConfirmation;
    auth.metadata_input = "5".into();
    auth.metadata_cursor = 1;
    auth.selected_index = 0;
    auth.prompt =
        "Actual $5 differs from official list $6. Confirm this subscription allocation?".into();
    let wide = render_to_text(&state, 100, 30);
    let narrow = render_to_text(&state, 52, 24);
    assert!(!wide.contains("never-render-this-key"));
    assert!(!narrow.contains("never-render-this-key"));
    insta::assert_snapshot!(
        "provider_auth_mimo_price_confirmation",
        wide_narrow(wide, narrow)
    );
}

#[test]
fn provider_picker_shows_all_first_class_routes_snapshots() {
    let mut state = test_state();
    let mut manager = provider_manager_state();
    manager.providers = [
        ("mimo", "https://api.xiaomimimo.com/v1"),
        ("openai-codex", "https://chatgpt.com/backend-api/codex"),
        ("deepseek", "https://api.deepseek.com/v1"),
        ("openai", "https://api.openai.com/v1"),
    ]
    .into_iter()
    .map(|(id, base_url)| crate::config::ProviderManagementEntry {
        id: id.into(),
        base_url: base_url.into(),
        api_key_source: crate::config::ProviderApiKeySource::None,
        configured_models: Vec::new(),
        configured: false,
        connected: false,
    })
    .collect();
    state.provider_manager = Some(manager);
    insta::assert_snapshot!(
        "provider_picker_first_class",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

#[test]
fn provider_template_catalog_mixed_filter_and_no_match_snapshots() {
    let mut state = test_state();
    let mut manager = provider_manager_state();
    manager.view = crate::app::ProviderManagerView::Templates {
        return_to: crate::app::ProviderTemplateReturn::Providers,
    };
    state.provider_manager = Some(manager);
    insta::assert_snapshot!(
        "provider_template_catalog",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );

    let manager = state.provider_manager.as_mut().unwrap();
    manager.template_filter = "openai".into();
    manager.selected_template_idx = 1;
    insta::assert_snapshot!(
        "provider_template_catalog_filtered",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );

    let manager = state.provider_manager.as_mut().unwrap();
    manager.template_filter = "openai codex".into();
    manager.selected_template_idx = 0;
    manager.error = Some(
        crate::provider_templates::get("openai-codex")
            .unwrap()
            .availability
            .reason()
            .into(),
    );
    insta::assert_snapshot!(
        "provider_template_catalog_unavailable_narrow",
        render_to_text(&state, 60, 24)
    );

    let manager = state.provider_manager.as_mut().unwrap();
    manager.template_filter = "not-there".into();
    manager.selected_template_idx = 0;
    manager.error = None;
    insta::assert_snapshot!(
        "provider_template_catalog_no_match",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

#[test]
fn provider_manager_detail_delete_busy_and_failure_snapshots() {
    let mut state = test_state();
    state.current_model = Some(ModelInfo {
        id: "mimo-v2.5-pro".into(),
        provider: "mimo".into(),
        display_name: "mimo-v2.5-pro".into(),
    });
    state.model_list = vec![
        ModelInfo {
            id: "mimo-v2.5".into(),
            provider: "mimo".into(),
            display_name: "mimo-v2.5".into(),
        },
        ModelInfo {
            id: "mimo-v2.5-pro".into(),
            provider: "mimo".into(),
            display_name: "mimo-v2.5-pro".into(),
        },
        ModelInfo {
            id: "mimo-v2.5-omni".into(),
            provider: "mimo".into(),
            display_name: "mimo-v2.5-omni".into(),
        },
    ];
    state
        .model_catalog_provider_status
        .insert("mimo".into(), "live · 3 models".into());
    let mut manager = provider_manager_state();
    manager.view = crate::app::ProviderManagerView::Models {
        provider: "mimo".into(),
    };
    state.provider_manager = Some(manager);
    insta::assert_snapshot!(
        "provider_manager_detail",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );

    state.provider_manager.as_mut().unwrap().delete_confirmation =
        Some(crate::app::ProviderDeleteConfirmation {
            provider: "mimo".into(),
            configured_model_count: 2,
            removes_keychain: false,
            replacement: Some(ModelInfo {
                id: "saved-model".into(),
                provider: "saved".into(),
                display_name: "saved-model".into(),
            }),
            confirm_selected: false,
        });
    insta::assert_snapshot!(
        "provider_manager_delete",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );

    let manager = state.provider_manager.as_mut().unwrap();
    manager.delete_confirmation = None;
    manager.busy = true;
    insta::assert_snapshot!(
        "provider_manager_busy",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
    let manager = state.provider_manager.as_mut().unwrap();
    manager.busy = false;
    manager.error = Some("Could not delete provider; config was restored".into());
    insta::assert_snapshot!(
        "provider_manager_failure",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

#[test]
fn provider_settings_editor_masks_saved_credentials_snapshots() {
    let mut state = test_state();
    state.provider_manager = Some(provider_manager_state());
    state.provider_editor = Some(crate::app::ProviderEditorState {
        origin: crate::app::ProviderEditorOrigin::ProviderManager,
        mode: crate::app::ProviderEditorMode::EditProvider,
        draft: crate::app::ProviderModelDraft {
            original_model: None,
            provider: "saved".into(),
            base_url: "https://saved.invalid/v1".into(),
            api_key_env: String::new(),
            model: String::new(),
        },
        api_key: crate::provider_secrets::SecretValue::default(),
        credential_configured: true,
        credential_in_keychain: true,
        protocol_override: None,
        prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
        field: crate::app::ProviderEditorField::ApiKey,
        cursor: 0,
        error: None,
        saving: false,
    });
    insta::assert_snapshot!(
        "provider_settings_editor",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

#[test]
fn provider_discovery_snapshots() {
    let mut scanning = test_state();
    scanning.provider_discovery = Some(crate::app::ProviderDiscoveryState {
        candidates: Vec::new(),
        selected_idx: 0,
        scanning: true,
        error: None,
    });
    insta::assert_snapshot!(
        "provider_discovery_scanning",
        wide_narrow(
            render_to_text(&scanning, 100, 30),
            render_to_text(&scanning, 60, 24)
        )
    );

    let mut populated = test_state();
    populated.provider_discovery = Some(crate::app::ProviderDiscoveryState {
        candidates: vec![
            crate::provider_discovery::LocalProviderCandidate {
                provider_id: "local-8000".into(),
                port: 8000,
                base_url: "http://127.0.0.1:8000/v1".into(),
                models: vec!["tiny-coder".into()],
            },
            crate::provider_discovery::LocalProviderCandidate {
                provider_id: "local-11434".into(),
                port: 11434,
                base_url: "http://127.0.0.1:11434/v1".into(),
                models: vec!["llama3.2".into(), "qwen-coder".into(), "mistral".into()],
            },
        ],
        selected_idx: 1,
        scanning: false,
        error: None,
    });
    insta::assert_snapshot!(
        "provider_discovery_populated",
        wide_narrow(
            render_to_text(&populated, 100, 30),
            render_to_text(&populated, 60, 24)
        )
    );

    let mut empty = test_state();
    empty.provider_discovery = Some(crate::app::ProviderDiscoveryState {
        candidates: Vec::new(),
        selected_idx: 0,
        scanning: false,
        error: None,
    });
    insta::assert_snapshot!(
        "provider_discovery_empty",
        wide_narrow(
            render_to_text(&empty, 100, 30),
            render_to_text(&empty, 60, 24)
        )
    );

    empty.provider_discovery.as_mut().unwrap().error =
        Some("could not prepare local provider discovery".into());
    insta::assert_snapshot!(
        "provider_discovery_failure",
        wide_narrow(
            render_to_text(&empty, 100, 30),
            render_to_text(&empty, 60, 24)
        )
    );
}

#[test]
fn provider_setup_checklist_and_detail_snapshots() {
    let mut state = test_state();
    state.current_model = Some(ModelInfo {
        id: "mimo-v2.5-pro".into(),
        provider: "mimo".into(),
        display_name: "mimo-v2.5-pro".into(),
    });
    let mut manager = provider_manager_state();
    manager.view = crate::app::ProviderManagerView::SetupProviders;
    state.provider_manager = Some(manager);
    state.provider_checks.insert(
        "mimo".into(),
        crate::provider_probe::ProviderCheckStatus::Working {
            model: "mimo-v2.5-pro".into(),
        },
    );
    state.provider_checks.insert(
        "empty-local".into(),
        crate::provider_probe::ProviderCheckStatus::CannotConnect,
    );
    state.provider_checks.insert(
        "saved".into(),
        crate::provider_probe::ProviderCheckStatus::CredentialRejected,
    );
    insta::assert_snapshot!(
        "provider_setup_checklist",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );

    state.provider_manager.as_mut().unwrap().view =
        crate::app::ProviderManagerView::SetupProvider {
            provider: "mimo".into(),
        };
    state.provider_check_running = Some("mimo".into());
    state.provider_checks.insert(
        "mimo".into(),
        crate::provider_probe::ProviderCheckStatus::Checking,
    );
    insta::assert_snapshot!(
        "provider_setup_checking",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );

    state.provider_check_running = None;
    state.provider_checks.insert(
        "mimo".into(),
        crate::provider_probe::ProviderCheckStatus::MissingEnvironment {
            variable: "MIMO_API_KEY".into(),
        },
    );
    insta::assert_snapshot!(
        "provider_setup_missing_environment",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );

    state.provider_checks.insert(
        "mimo".into(),
        crate::provider_probe::ProviderCheckStatus::AccessDenied,
    );
    insta::assert_snapshot!(
        "provider_setup_access_denied",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );

    state.provider_manager.as_mut().unwrap().view =
        crate::app::ProviderManagerView::SetupProvider {
            provider: "empty-local".into(),
        };
    state.provider_checks.insert(
        "empty-local".into(),
        crate::provider_probe::ProviderCheckStatus::NeedsModel,
    );
    insta::assert_snapshot!(
        "provider_setup_needs_model",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

#[test]
fn provider_setup_finish_and_no_passing_snapshots() {
    let mut state = test_state();
    let mut manager = provider_manager_state();
    manager.view = crate::app::ProviderManagerView::SetupFinish;
    manager.selected_model_idx = 0;
    state.provider_manager = Some(manager);
    state.provider_checks.insert(
        "mimo".into(),
        crate::provider_probe::ProviderCheckStatus::Working {
            model: "mimo-v2.5-pro".into(),
        },
    );
    state.provider_checks.insert(
        "saved".into(),
        crate::provider_probe::ProviderCheckStatus::Working {
            model: "saved-model".into(),
        },
    );
    insta::assert_snapshot!(
        "provider_setup_finish",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );

    state.provider_checks.clear();
    let manager = state.provider_manager.as_mut().unwrap();
    manager.view = crate::app::ProviderManagerView::SetupProviders;
    manager.error = Some("Test at least one working provider before finishing".into());
    insta::assert_snapshot!(
        "provider_setup_no_passing",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

#[test]
fn provider_command_palette_snapshots() {
    let mut state = test_state();
    state.input = "/provider".into();
    state.input_cursor = state.input.len();
    state.slash_palette_open = true;
    state.slash_filter = "provider".into();
    insta::assert_snapshot!(
        "provider_command_palette",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

#[test]
fn command_palette_snapshots() {
    let mut discovery = test_state();
    discovery.input = "/".into();
    discovery.input_cursor = discovery.input.len();
    discovery.slash_palette_open = true;
    insta::assert_snapshot!(
        "command_discovery",
        wide_narrow(
            render_to_text(&discovery, 100, 30),
            render_to_text(&discovery, 60, 24)
        )
    );

    let mut state = test_state();
    state.input = "/session".into();
    state.input_cursor = state.input.len();
    state.slash_palette_open = true;
    state.slash_filter = "session".to_string();
    state.selected_slash_idx = 1;

    insta::assert_snapshot!(
        "command_palette",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

#[test]
fn wide_command_palette_keeps_common_descriptions_readable() {
    let mut state = test_state();
    state.input = "/".into();
    state.input_cursor = state.input.len();
    state.slash_palette_open = true;

    let rendered = render_to_text(&state, 100, 30);
    assert!(rendered.contains("Browse and continue previous sessions"));
    assert!(rendered.contains("Show context, account, and session usage in a tabbed dashboard"));
    assert!(rendered.contains("Open the agent dashboard"));
}

#[test]
fn learning_command_palette_keeps_full_operator_examples_readable() {
    let mut state = test_state();
    state.input = "/learn".into();
    state.input_cursor = state.input.len();
    state.slash_palette_open = true;
    state.slash_filter = "learn".into();

    let learn = render_to_text(&state, 100, 30);
    assert!(learn.contains("/learn [focus text|status [run-id]|sync|undo <run-id>]"));

    state.input = "/distill".into();
    state.input_cursor = state.input.len();
    state.slash_filter = "distill".into();
    let distill = render_to_text(&state, 100, 30);
    assert!(distill.contains("/distill [focus text|run [focus]|status|on|off|undo <run-id>]"));
}

#[test]
fn config_hub_populated_and_no_match_snapshots() {
    let mut state = test_state();
    state.config_hub = Some(crate::app::ConfigHubState {
        filter: String::new(),
        selected_idx: 1,
        items: vec![
            crate::app::ConfigHubItem {
                label: "Current model".into(),
                value: "mimo/mimo-v2.5-pro".into(),
                keywords: "provider default switch".into(),
                action: crate::app::ConfigHubAction::CurrentModel,
            },
            crate::app::ConfigHubItem {
                label: "Delegated agent".into(),
                value: "same as current agent".into(),
                keywords: "subagent child provider model".into(),
                action: crate::app::ConfigHubAction::Delegation {
                    selected: None,
                    configured: Vec::new(),
                },
            },
            crate::app::ConfigHubItem {
                label: "Provider setup".into(),
                value: "4 configured".into(),
                keywords: "api key endpoint".into(),
                action: crate::app::ConfigHubAction::ProviderSetup,
            },
            crate::app::ConfigHubItem {
                label: "Review model".into(),
                value: "deepseek/deepseek-v4-pro".into(),
                keywords: "reviewer".into(),
                action: crate::app::ConfigHubAction::ReviewModel,
            },
            crate::app::ConfigHubItem {
                label: "Config file".into(),
                value: "/tmp/ca/config.toml".into(),
                keywords: "toml path".into(),
                action: crate::app::ConfigHubAction::ConfigFile("/tmp/ca/config.toml".into()),
            },
        ],
    });
    insta::assert_snapshot!(
        "config_hub",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );

    let hub = state.config_hub.as_mut().unwrap();
    hub.filter = "no-such-setting".into();
    hub.selected_idx = 0;
    insta::assert_snapshot!(
        "config_hub_no_match",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

#[test]
fn doctor_report_snapshots() {
    let report = crate::doctor::DoctorReport {
        schema: crate::doctor::SCHEMA,
        profile: "default".into(),
        ready: false,
        checks: vec![
            crate::doctor::DoctorCheck {
                name: "Config".into(),
                level: crate::doctor::CheckLevel::Pass,
                message: "config.toml parsed successfully".into(),
            },
            crate::doctor::DoctorCheck {
                name: "Default model".into(),
                level: crate::doctor::CheckLevel::Pass,
                message: "mimo / mimo-v2.5-pro".into(),
            },
            crate::doctor::DoctorCheck {
                name: "Provider connections".into(),
                level: crate::doctor::CheckLevel::Fail,
                message: "1 provider(s) did not pass".into(),
            },
        ],
        providers: vec![
            crate::doctor::DoctorProvider {
                id: "mimo".into(),
                configured_models: 2,
                available_models: 2,
                credential: "credential environment MIMO_API_KEY".into(),
                connection: "credential rejected (HTTP 401)".into(),
                price_freshness: "2/2 fresh".into(),
                billing_readiness: "USD Token Plan ready".into(),
                capability: "standard".into(),
                route_eligibility: "eligible".into(),
                tested_model: Some("mimo-v2.5-pro".into()),
            },
            crate::doctor::DoctorProvider {
                id: "local".into(),
                configured_models: 1,
                available_models: 1,
                credential: "credential none".into(),
                connection: "working".into(),
                price_freshness: "unknown".into(),
                billing_readiness: "unsupported billing mode".into(),
                capability: "unknown".into(),
                route_eligibility: "ineligible".into(),
                tested_model: Some("local-model".into()),
            },
        ],
    };
    let mut state = test_state();
    state.pager_content = report.render_text();
    state.pager_open = true;

    insta::assert_snapshot!(
        "doctor_report",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

#[test]
fn agent_harness_audit_pager_snapshots() {
    let mut loading = test_state();
    loading.pager_content =
        "Agent-harness audit\n\nStatus: SCANNING\n\nBounded local inspection is in progress…"
            .into();
    loading.pager_open = true;
    insta::assert_snapshot!(
        "agent_harness_audit_loading",
        wide_narrow(
            render_to_text(&loading, 100, 30),
            render_to_text(&loading, 60, 24)
        )
    );

    let report = crate::security_audit::AuditReport {
        workspace: "workspace".into(),
        profile: "work".into(),
        scanned_files: 17,
        scanned_bytes: 4_208,
        partial: true,
        findings: vec![
            crate::security_audit::AuditFinding {
                severity: crate::security_audit::AuditSeverity::Critical,
                rule_id: "AUD-SYMLINK-001",
                path: "workspace/.agents/skills/escaped".into(),
                evidence: "skill symlink escapes its declared authority; target withheld".into(),
                remediation: "Replace the symlink with an owned regular file or directory inside its authority.",
            },
            crate::security_audit::AuditFinding {
                severity: crate::security_audit::AuditSeverity::Medium,
                rule_id: "AUD-LIMIT-001",
                path: "workspace/.ca/rules".into(),
                evidence: "directory depth limit reached".into(),
                remediation: "Reduce the harness surface or repair read access, then rerun the audit.",
            },
        ],
    };
    let mut populated = test_state();
    populated.pager_content = report.render_text();
    populated.pager_open = true;
    insta::assert_snapshot!(
        "agent_harness_audit_report",
        wide_narrow(
            render_to_text(&populated, 100, 30),
            render_to_text(&populated, 60, 24)
        )
    );

    let mut empty = test_state();
    empty.pager_content = crate::security_audit::AuditReport {
        workspace: "workspace".into(),
        profile: "default".into(),
        scanned_files: 0,
        scanned_bytes: 0,
        partial: false,
        findings: Vec::new(),
    }
    .render_text();
    empty.pager_open = true;
    insta::assert_snapshot!(
        "agent_harness_audit_empty",
        wide_narrow(
            render_to_text(&empty, 100, 30),
            render_to_text(&empty, 60, 24)
        )
    );

    let mut failed = test_state();
    failed.pager_content =
        "Agent-harness audit\n\nStatus: ERROR\n\nThe primary workspace could not be inspected safely."
            .into();
    failed.pager_open = true;
    insta::assert_snapshot!(
        "agent_harness_audit_error",
        wide_narrow(
            render_to_text(&failed, 100, 30),
            render_to_text(&failed, 60, 24)
        )
    );
}

#[test]
fn reviewed_archive_learning_pager_snapshots() {
    let mut status = test_state();
    status.pager_content = "Session evidence learning\n\nSources: 2\nIndexed files: 89\nIndexed: 1515 messages\nSearchable: 1515 · Retrieved: 96 · Inspected: 24 · Used as support: 48 · Retired: 1515\nIndexed sources: CA 12 · Codex 1003 · MiMo 500\nSearchable sources: CA 12 · Codex 1003 · MiMo 500\nWorkflow clusters: 18 · catalog pages 1/1 · complete\nPending evidence: none\nLast scanner failure: 2026-08-22T12:00:00Z · archive source changed during revision check\nSkipped records: 3 malformed · 7 unknown · 1 oversized · 2 sensitive messages · 4 incomplete messages".into();
    status.pager_open = true;
    status.learning_review_pager_open = true;
    insta::assert_snapshot!(
        "reviewed_archive_learning_status",
        wide_narrow(
            render_to_text(&status, 100, 30),
            render_to_text(&status, 60, 24)
        )
    );

    let mut evidence = test_state();
    evidence.pager_content = "Pending evidence: 1129 messages · 842311 bytes · 6 workspaces · 27 root threads · 18 planned bounded calls\nRoute: openai/gpt-5.6-terra · estimated cost USD 1.42 maximum for 18 bounded calls\nSensitive messages skipped: 2\nExact digest: 8c675308b4ab93d41d21c2762b02be7b7deca092bd8f46cb26d1f70a2caf8d26\n\nSource revisions\n- source 14b92072a0bd · revision 7818d12e274d\n\nSanitized excerpts\n- workspace 42f5d194034e · thread bc8ba8ecac09 · user title/excerpt: Keep local changes narrowly scoped.\n\nEnter opens in-app actions.\nDrag copies selected text; C copies the full page.\nCLI fallback: ca learn review approve 8c675308b4ab93d41d21c2762b02be7b7deca092bd8f46cb26d1f70a2caf8d26".into();
    evidence.pager_open = true;
    evidence.learning_review_pager_open = true;
    insta::assert_snapshot!(
        "reviewed_archive_learning_evidence",
        wide_narrow(
            render_to_text(&evidence, 100, 30),
            render_to_text(&evidence, 60, 24)
        )
    );

    let mut proposal = test_state();
    proposal.pager_content = "Generated archive-learning proposal (inert)\nEvidence digest: 8c675308b4ab93d41d21c2762b02be7b7deca092bd8f46cb26d1f70a2caf8d26\nRoute: openai/gpt-5.6-terra\nProposal runs: 2\nExact proposal digest: 93114028fd7be3278956f3f7eb9ac6cfebd1e1b69a495209eef83413784c8b1c\n\n- 9f596ff0-d920-4dd1-a21e-91b60b33a4c0 · workspace 42f5d194034e · 1 memory operation(s) · 1 skill package(s)\n  memory item bbb3a430c14d1b2e793330543706592f30c78bfef16e39716669704ebcb9e0a8 · Workspace Add: Keep diffs local.\n  skill item 0d722f881bcd53a69c05aab6c86f5095378672193540854350113949147eea29: Build focused validation fixtures.\n\nEnter opens in-app actions.\nDrag copies selected text; C copies the full page.\nCLI fallback: ca learn proposal apply 93114028fd7be3278956f3f7eb9ac6cfebd1e1b69a495209eef83413784c8b1c --item bbb3a430c14d1b2e793330543706592f30c78bfef16e39716669704ebcb9e0a8".into();
    proposal.pager_open = true;
    proposal.learning_review_pager_open = true;
    insta::assert_snapshot!(
        "reviewed_archive_learning_proposal",
        wide_narrow(
            render_to_text(&proposal, 100, 30),
            render_to_text(&proposal, 60, 24)
        )
    );
}

#[test]
fn direct_adaptive_learning_transcript_snapshots() {
    let mut state = test_state();
    state.messages.push(Message {
        role: Role::Assistant,
        content: "# Learning pass complete\n\n## Inventory\n- Indexed 1596 message(s) · Searchable 1596 message(s) · Retired 1596 message(s).\n- Searchable sessions 190 · Retrieved 12 message(s) / 6 session(s) · Inspected 12 message(s) / 6 session(s) · Used as support 6 session(s).\n- Sources: CA 18 · Codex 1000 · MiMo 578.\n- Workflow clusters: 2/2 page(s) delivered · 119 searchable · 119 explicitly decided.\n- Existing assets: 132 — 3 commands · 128 skills · 1 subagent.\n- Proposed this run: 0 commands · 0 skills · 0 subagents.\n\n## Memory review\n- Changed 0 cluster decision(s) · already covered 3 · non-durable 116 · unresolved 0.\n\n## Shortlist\n| Workflow | Evidence | Confidence | Outcome | Reason / existing assets |\n| --- | --- | --- | --- | --- |\n| BSD rsync check | 25 sessions | high | skipped | Existing: /check-rsync — `/Users/aji/.mimocode/command/check-rsync.md` |\n| Transmission status | 25 sessions | high | skipped | Existing: /check-transmission — `/Users/aji/.mimocode/command/check-transmission.md` |\n| Seedbox status | 24 sessions | high | skipped | Existing: /check-seedbox — `/Users/aji/.mimocode/command/check-seedbox.md` |\n\nCollapsed low-signal singletons: 116 clusters / 116 sessions. Examples: FreeBSD QEMU check, one-off cleanup, isolated migration.\n\n## Created or extended\n- Nothing new; existing knowledge already covers the validated evidence.\n\n## Skipped\n- Existing BSD commands are shown above with exact paths.\n- Collapsed 116 low-signal singleton clusters across 116 sessions.\n\n## Needs more evidence\n- No unresolved recurring workflow.\n\n## Receipt\nRun: 11111111-1111-4111-8111-111111111111 · model mimo/mimo-v2.5-pro · 2 adaptive turns · 1 bounded retrieval step · 1842 input / 419 output tokens · undo `/learn undo 11111111-1111-4111-8111-111111111111`\nStructural validation: passed.\nDetails: `/learn status 11111111-1111-4111-8111-111111111111`".into(),
        tool_calls: None,
        thinking: None,
        response_items: Vec::new(),
        timestamp: chrono::Utc::now(),
    });
    assert!(!state.pager_open);
    assert!(state.interaction.is_none());
    insta::assert_snapshot!(
        "direct_adaptive_learning_transcript",
        wide_narrow(
            render_to_text(&state, 120, 44),
            render_to_text(&state, 64, 80)
        )
    );
}

#[test]
fn direct_adaptive_learning_running_cards_snapshots() {
    let mut state = test_state();
    state.messages.push(Message {
        role: Role::Assistant,
        content: "# Learning\n\nModel: `mimo/mimo-v2.5-pro` · up to 8 turns\n\n## Investigation\n- ▸ Inspect · 3 signal clusters · 6 representative sessions (collapsed)\n- ▸ Inventory · 132 reusable asset entries (collapsed)\n\n## Current stage\n⏳ Continuing from bounded evidence · adaptive turn 2/8".into(),
        tool_calls: None,
        thinking: None,
        response_items: Vec::new(),
        timestamp: chrono::Utc::now(),
    });
    insta::assert_snapshot!(
        "direct_adaptive_learning_running_cards",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 26)
        )
    );
}

fn paged_history_render_state() -> AppState {
    let mut state = test_state();
    state.transcript_scroll.request_offset(3);
    state.messages = (64..68)
        .map(|index| Message {
            role: if index % 2 == 0 {
                Role::User
            } else {
                Role::Assistant
            },
            content: format!("saved history message {index}"),
            tool_calls: None,
            thinking: None,
            timestamp: chrono::DateTime::from_timestamp(index, 0).unwrap(),
            response_items: Vec::new(),
        })
        .collect();
    state.session_history = Some(crate::app::SessionHistoryState {
        storage_key: "session-one".into(),
        session_id: uuid::Uuid::nil(),
        storage_generation: uuid::Uuid::nil(),
        start_index: 64,
        total_messages: 68,
        busy: false,
        request_older: false,
        request_full: false,
        resume_submit: false,
        generation: 1,
        error: None,
    });
    state
}

#[test]
fn lazy_session_history_recent_loading_complete_and_failure_snapshots() {
    let recent = paged_history_render_state();
    insta::assert_snapshot!(
        "session_history_recent",
        wide_narrow(
            render_to_text(&recent, 100, 30),
            render_to_text(&recent, 60, 24)
        )
    );

    let mut loading = paged_history_render_state();
    loading.session_history.as_mut().unwrap().busy = true;
    insta::assert_snapshot!(
        "session_history_loading",
        wide_narrow(
            render_to_text(&loading, 100, 30),
            render_to_text(&loading, 60, 24)
        )
    );

    let mut complete = paged_history_render_state();
    complete.session_history.as_mut().unwrap().start_index = 0;
    complete.session_history.as_mut().unwrap().total_messages = 4;
    for (index, message) in complete.messages.iter_mut().enumerate() {
        message.content = format!("saved history message {index}");
    }
    insta::assert_snapshot!(
        "session_history_complete",
        wide_narrow(
            render_to_text(&complete, 100, 30),
            render_to_text(&complete, 60, 24)
        )
    );

    let mut failed = paged_history_render_state();
    failed.session_history.as_mut().unwrap().error =
        Some("saved history changed; resume the session again".into());
    insta::assert_snapshot!(
        "session_history_failed",
        wide_narrow(
            render_to_text(&failed, 100, 30),
            render_to_text(&failed, 60, 24)
        )
    );
}

#[test]
fn prepending_lazy_history_preserves_the_visible_bottom_relative_anchor() {
    let mut state = paged_history_render_state();
    state.messages = (70..100)
        .map(|index| Message {
            role: Role::User,
            content: format!("saved history message {index}"),
            tool_calls: None,
            thinking: None,
            timestamp: chrono::DateTime::from_timestamp(index, 0).unwrap(),
            response_items: Vec::new(),
        })
        .collect();
    let history = state.session_history.as_mut().unwrap();
    history.start_index = 70;
    history.total_messages = 100;
    state.transcript_scroll.request_offset(8);
    let before = render_to_text(&state, 100, 20);

    let mut older = (60..70)
        .map(|index| Message {
            role: Role::User,
            content: format!("saved history message {index}"),
            tool_calls: None,
            thinking: None,
            timestamp: chrono::DateTime::from_timestamp(index, 0).unwrap(),
            response_items: Vec::new(),
        })
        .collect::<Vec<_>>();
    older.append(&mut state.messages);
    state.messages = older;
    state.transcript_scroll.note_history_prepend(10);
    state.session_history.as_mut().unwrap().start_index = 60;
    let after = render_to_text(&state, 100, 20);

    let visible_messages = |rendered: &str| {
        rendered
            .lines()
            .filter(|line| line.contains("saved history message"))
            .map(|line| line.trim().to_string())
            .collect::<Vec<_>>()
    };
    let before_messages = visible_messages(&before);
    assert!(!before_messages.is_empty());
    assert_eq!(before_messages, visible_messages(&after));
}

#[test]
fn session_picker_snapshots() {
    let mut state = test_state();
    state.session_picker_open = true;
    state.session_filter = "workspace".into();
    let current_id = uuid::Uuid::parse_str("11111111-1111-4111-8111-111111111111").unwrap();
    state.session_list = vec![
        crate::session::SessionSummary {
            storage_key: "session-current".into(),
            id: current_id,
            title: "Current conversation".into(),
            pinned: true,
            workspace: "/workspace/ca".into(),
            updated_at: fixed_time(),
        },
        crate::session::SessionSummary {
            storage_key: "session-former".into(),
            id: uuid::Uuid::parse_str("33333333-3333-4333-8333-333333333333").unwrap(),
            title: "Repository review".into(),
            pinned: false,
            workspace: "/workspace/ca".into(),
            updated_at: fixed_time() + chrono::Duration::days(1),
        },
        crate::session::SessionSummary {
            storage_key: "session-newer".into(),
            id: uuid::Uuid::parse_str("44444444-4444-4444-8444-444444444444").unwrap(),
            title: "Named session browser".into(),
            pinned: false,
            workspace: "/workspace/ca".into(),
            updated_at: fixed_time(),
        },
    ];
    state.session_archive_list = vec![crate::session::SessionSummary {
        storage_key: "session-archive".into(),
        id: uuid::Uuid::parse_str("22222222-2222-4222-8222-222222222222").unwrap(),
        title: "Older investigation".into(),
        pinned: false,
        workspace: "/workspace/ca".into(),
        updated_at: fixed_time(),
    }];
    let mut current = crate::session::SessionV2::new(
        "Repository review",
        "/workspace/ca",
        "local",
        "model",
        Vec::new(),
    );
    current.id = current_id;
    state.current_session = Some(current);

    insta::assert_snapshot!(
        "session_picker",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

#[test]
fn session_picker_hides_current_from_rows_and_active_count() {
    let mut state = test_state();
    state.session_picker_open = true;
    let current_id = uuid::Uuid::parse_str("11111111-1111-4111-8111-111111111111").unwrap();
    state.session_list = vec![
        crate::session::SessionSummary {
            storage_key: "current-session".into(),
            id: current_id,
            title: "CURRENT ROW MUST BE HIDDEN".into(),
            pinned: false,
            workspace: "/workspace/current".into(),
            updated_at: fixed_time(),
        },
        crate::session::SessionSummary {
            storage_key: "former-session".into(),
            id: uuid::Uuid::parse_str("22222222-2222-4222-8222-222222222222").unwrap(),
            title: "Former conversation".into(),
            pinned: false,
            workspace: "/workspace/former".into(),
            updated_at: fixed_time(),
        },
    ];
    let mut current = crate::session::SessionV2::new(
        "Current",
        "/workspace/current",
        "local",
        "model",
        Vec::new(),
    );
    current.id = current_id;
    state.current_session = Some(current);

    let rendered = render_to_text(&state, 100, 30);
    assert!(rendered.contains("Recent 1"));
    assert!(rendered.contains("Former conversation"));
    assert!(!rendered.contains("CURRENT ROW MUST BE HIDDEN"));
    assert!(!rendered.contains("/workspace/former"));
    assert!(!rendered.contains("ses_"));
}

#[test]
fn session_picker_explains_how_to_repair_prompt_like_names() {
    let mut state = test_state();
    state.session_picker_open = true;
    state.session_list = vec![crate::session::SessionSummary {
        storage_key: "legacy-session".into(),
        id: uuid::Uuid::parse_str("22222222-2222-4222-8222-222222222222").unwrap(),
        title: "/Users/aji/project/ca review…".into(),
        pinned: false,
        workspace: "/workspace/ca".into(),
        updated_at: fixed_time(),
    }];

    let rendered = render_to_text(&state, 60, 24);
    assert!(rendered.contains("Prompt-like name"));
    assert!(rendered.contains("Ctrl+G generate name"));
}

#[test]
fn session_rename_snapshots() {
    let mut state = test_state();
    state.session_picker_open = true;
    let id = uuid::Uuid::parse_str("22222222-2222-4222-8222-222222222222").unwrap();
    state.session_list = vec![crate::session::SessionSummary {
        storage_key: "session-former".into(),
        id,
        title: "Repository review".into(),
        pinned: false,
        workspace: "/workspace/ca".into(),
        updated_at: fixed_time(),
    }];
    state.session_rename = Some(crate::app::SessionRenameState {
        storage_key: "session-former".into(),
        session_id: id,
        title: "Repository review".into(),
        cursor: "Repository".len(),
        archived: false,
        busy: false,
        error: None,
    });

    insta::assert_snapshot!(
        "session_rename",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

#[test]
fn session_archive_picker_snapshots() {
    let mut state = test_state();
    state.session_picker_open = true;
    state.session_picker_view = crate::app::SessionPickerView::Archive;
    state.session_archive_list = vec![crate::session::SessionSummary {
        storage_key: "session-archive".into(),
        id: uuid::Uuid::parse_str("22222222-2222-4222-8222-222222222222").unwrap(),
        title: "Older investigation".into(),
        pinned: true,
        workspace: "/workspace/ca".into(),
        updated_at: fixed_time(),
    }];

    insta::assert_snapshot!(
        "session_archive",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

#[test]
fn session_picker_empty_and_busy_snapshots() {
    let mut empty = test_state();
    empty.session_picker_open = true;
    empty.session_picker_view = crate::app::SessionPickerView::Archive;
    insta::assert_snapshot!(
        "session_archive_empty",
        wide_narrow(
            render_to_text(&empty, 100, 30),
            render_to_text(&empty, 60, 24)
        )
    );

    let mut busy = empty;
    busy.session_picker_view = crate::app::SessionPickerView::Active;
    busy.session_picker_busy = true;
    busy.session_list = vec![crate::session::SessionSummary {
        storage_key: "session-one".into(),
        id: uuid::Uuid::parse_str("11111111-1111-4111-8111-111111111111").unwrap(),
        title: "Repository review".into(),
        pinned: false,
        workspace: "/workspace/ca".into(),
        updated_at: fixed_time(),
    }];
    insta::assert_snapshot!(
        "session_management_busy",
        wide_narrow(
            render_to_text(&busy, 100, 30),
            render_to_text(&busy, 60, 24)
        )
    );
}

#[test]
fn session_archive_confirmation_snapshots() {
    let mut state = test_state();
    state.session_picker_open = true;
    state.session_archive_confirmation = Some(crate::app::SessionArchiveConfirmation {
        storage_key: "session-one".into(),
        session_id: uuid::Uuid::parse_str("11111111-1111-4111-8111-111111111111").unwrap(),
        title: "Repository review".into(),
        archive_selected: false,
        busy: false,
        error: None,
    });

    insta::assert_snapshot!(
        "session_archive_confirmation",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );

    let confirmation = state.session_archive_confirmation.as_mut().unwrap();
    confirmation.busy = true;
    state.session_picker_busy = true;
    insta::assert_snapshot!(
        "session_archive_confirmation_busy",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );

    let confirmation = state.session_archive_confirmation.as_mut().unwrap();
    confirmation.busy = false;
    confirmation.error = Some("Session archive failed: permission denied".into());
    state.session_picker_busy = false;
    insta::assert_snapshot!(
        "session_archive_confirmation_error",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

#[test]
fn session_delete_confirmation_snapshots() {
    let mut state = test_state();
    state.session_picker_open = true;
    state.session_delete_confirmation = Some(crate::app::SessionDeleteConfirmation {
        storage_key: "session-one".into(),
        session_id: uuid::Uuid::parse_str("11111111-1111-4111-8111-111111111111").unwrap(),
        title: "Repository review".into(),
        delete_selected: false,
        busy: false,
        error: None,
    });

    insta::assert_snapshot!(
        "session_delete_confirmation",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );

    state.session_delete_confirmation.as_mut().unwrap().error =
        Some("Session deletion failed: permission denied".into());
    insta::assert_snapshot!(
        "session_delete_error",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

#[test]
fn provider_trace_status_and_delete_confirmation_snapshots() {
    let hub = crate::provider_trace::ProviderTraceHub::new(
        std::path::PathBuf::from("/tmp/ca-profile/traces"),
        true,
    );
    let mut status = test_state();
    status.pager_content = hub.render_status();
    status.pager_open = true;
    insta::assert_snapshot!(
        "provider_trace_status",
        wide_narrow(
            render_to_text(&status, 100, 30),
            render_to_text(&status, 60, 24)
        )
    );

    let mut list = test_state();
    list.pager_content = concat!(
        "Provider traces\n\n",
        "00000000-0000-0000-0000-000000000117    12.1 KiB  2026-08-16T12:00:00+00:00\n",
        "00000000-0000-0000-0000-000000000116       512 B  2026-08-16T11:00:00+00:00\n\n",
        "Open with /trace open <id>\n",
        "Delete with /trace delete <id> (confirmation required)",
    )
    .into();
    list.pager_open = true;
    insta::assert_snapshot!(
        "provider_trace_list",
        wide_narrow(
            render_to_text(&list, 100, 30),
            render_to_text(&list, 60, 24)
        )
    );

    let mut open = test_state();
    open.pager_content = concat!(
        "Provider trace 00000000-0000-0000-0000-000000000117\n\n",
        "{\"schema\":\"ca.provider-trace.v1\",\"sequence\":0,\"event\":\"trace_started\"}\n",
        "{\"schema\":\"ca.provider-trace.v1\",\"sequence\":1,\"request_kind\":\"main_turn\",\"event\":\"request_started\"}\n",
    )
    .into();
    open.pager_open = true;
    insta::assert_snapshot!(
        "provider_trace_open",
        wide_narrow(
            render_to_text(&open, 100, 30),
            render_to_text(&open, 60, 24)
        )
    );

    let mut confirmation = test_state();
    confirmation.trace_delete_confirmation = Some(crate::app::TraceDeleteConfirmation {
        trace_id: uuid::Uuid::parse_str("00000000-0000-0000-0000-000000000117").unwrap(),
        bytes: 12_345,
        delete_selected: false,
        error: None,
    });
    insta::assert_snapshot!(
        "provider_trace_delete_confirmation",
        wide_narrow(
            render_to_text(&confirmation, 100, 30),
            render_to_text(&confirmation, 60, 24)
        )
    );

    let delete = confirmation.trace_delete_confirmation.as_mut().unwrap();
    delete.delete_selected = true;
    delete.error = Some("trace changed before deletion".into());
    insta::assert_snapshot!(
        "provider_trace_delete_error",
        wide_narrow(
            render_to_text(&confirmation, 100, 30),
            render_to_text(&confirmation, 60, 24)
        )
    );
}

#[test]
fn lsp_post_edit_diagnostic_report_snapshots() {
    let mut state = test_state();
    state.pager_content = concat!(
        "Modified src/lib.rs\n\n",
        "LSP diagnostics [rust-analyzer] for document version 7: 2\n",
        "- error [rustc/E0597] 12:9-12:14 — borrowed value does not live long enough\n",
        "- warning [clippy/needless_borrow] 18:5-18:17 — this expression creates a reference which is immediately dereferenced",
    )
    .into();
    state.pager_open = true;
    insta::assert_snapshot!(
        "lsp_post_edit_report",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

#[test]
fn session_artifact_browser_snapshots() {
    let mut state = test_state();
    state.pager_content = concat!(
        "Session artifacts\n\n",
        "1. src/session.rs · modified · tracked · 3 accesses\n",
        "2. docs/report.md · created · tracked · 1 access\n",
        "3. tmp/removed.txt · deleted · deleted · 1 access\n",
        "\nUse /artifacts inspect|view|copy|untrack <number-or-path>.\n",
    )
    .into();
    state.pager_open = true;
    insta::assert_snapshot!(
        "session_artifact_browser",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

#[test]
fn project_init_surface_snapshots() {
    let mut preparing = test_state();
    preparing.project_init = Some(crate::app::ProjectInitState {
        root: "/workspace/demo".into(),
        draft: None,
        phase: crate::app::ProjectInitPhase::Preparing,
        scroll: 0,
        create_selected: false,
        error: None,
    });
    insta::assert_snapshot!(
        "project_init_preparing",
        wide_narrow(
            render_to_text(&preparing, 100, 30),
            render_to_text(&preparing, 60, 24)
        )
    );

    let mut preview = preparing;
    let init = preview.project_init.as_mut().unwrap();
    init.phase = crate::app::ProjectInitPhase::Preview;
    init.draft = Some(crate::project_init::test_draft(
        "/workspace/demo",
        "# Repository instructions\n\n- Run `cargo test`.\n- Keep credentials external.\n",
    ));
    insta::assert_snapshot!(
        "project_init_preview",
        wide_narrow(
            render_to_text(&preview, 100, 30),
            render_to_text(&preview, 60, 24)
        )
    );

    let init = preview.project_init.as_mut().unwrap();
    init.create_selected = true;
    init.error = Some("workspace evidence changed after preview; run /init again".into());
    insta::assert_snapshot!(
        "project_init_error",
        wide_narrow(
            render_to_text(&preview, 100, 30),
            render_to_text(&preview, 60, 24)
        )
    );
}

#[test]
fn timeline_snapshots() {
    let mut state = test_state();
    state.timeline_open = true;
    state.timeline_mode = crate::app::TimelineMode::Rewind;
    state.selected_timeline_idx = 1;
    state.timeline_entries = vec![
        crate::session::TimelineEntry {
            turn_index: 1,
            through_message_count: 2,
            title: "Inspect the project".into(),
            completed_at: fixed_time(),
        },
        crate::session::TimelineEntry {
            turn_index: 2,
            through_message_count: 5,
            title: "Implement stable sessions".into(),
            completed_at: chrono::DateTime::from_timestamp(60, 0).unwrap(),
        },
    ];

    insta::assert_snapshot!(
        "timeline",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

#[test]
fn queue_preview_snapshots() {
    let mut state = test_state();
    state.turn_lifecycle.begin();
    for (id, text) in [
        (
            "11111111-1111-4111-8111-111111111111",
            "First queued follow-up",
        ),
        (
            "22222222-2222-4222-8222-222222222222",
            "Second queued follow-up with a longer explanation",
        ),
        (
            "33333333-3333-4333-8333-333333333333",
            "Third queued follow-up",
        ),
        (
            "44444444-4444-4444-8444-444444444444",
            "Hidden fourth follow-up",
        ),
    ] {
        state
            .input_queue
            .enqueue_with_id(uuid::Uuid::parse_str(id).unwrap(), text);
    }
    state.input_height = 6;

    insta::assert_snapshot!(
        "queue_preview",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

#[test]
fn busy_interrupt_controls_survive_status_messages() {
    let mut state = test_state();
    state.turn_lifecycle.begin();
    state.status_message = Some("Working on your request…".into());
    for width in [100, 60] {
        let rendered = render_to_text(&state, width, 24);
        for control in ["Esc stop", "Enter interrupt", "Alt+Enter queue"] {
            assert!(rendered.contains(control), "missing {control}:\n{rendered}");
        }
        assert!(rendered.contains("Working on your request"));
    }
    insta::assert_snapshot!(
        "interrupt_controls_busy",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
    let id = uuid::Uuid::parse_str("55555555-5555-4555-8555-555555555555").unwrap();
    state.input_queue.enqueue_with_id(id, "stop");
    state.restored_queue_ids.insert(id);
    state.turn_lifecycle.cancel();
    state.status_message = Some("Stopped · queued messages on hold".into());
    insta::assert_snapshot!(
        "interrupt_controls_stopped",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
    state.input = state.input_queue.begin_edit_last().unwrap();
    state.input_cursor = state.input.len();
    insta::assert_snapshot!(
        "interrupt_controls_queued_edit",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

#[test]
fn restored_and_editing_queue_preview_snapshots() {
    let mut state = test_state();
    state.turn_lifecycle.begin();
    let restored = uuid::Uuid::parse_str("55555555-5555-4555-8555-555555555555").unwrap();
    state
        .input_queue
        .enqueue_with_id(restored, "Review this restored message before sending");
    state.input_queue.enqueue_with_id(
        uuid::Uuid::parse_str("66666666-6666-4666-8666-666666666666").unwrap(),
        "Edit this newest queued message",
    );
    state.restored_queue_ids.insert(restored);
    assert!(state.input_queue.begin_edit_last().is_some());
    insta::assert_snapshot!(
        "queue_restored_editing",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

#[test]
fn queue_view_snapshots() {
    let mut state = test_state();
    state.input_queue.enqueue_with_id(
        uuid::Uuid::parse_str("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa").unwrap(),
        "Run the focused tests",
    );
    state.input_queue.enqueue_with_id(
        uuid::Uuid::parse_str("bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb").unwrap(),
        "Then review the narrow layout",
    );
    state.input_queue.select_next();
    state.queue_view_open = true;

    insta::assert_snapshot!(
        "queue_view",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

#[test]
fn context_dashboard_snapshots() {
    let mut state = test_state();
    state.current_model = Some(ModelInfo {
        id: "coder".into(),
        provider: "local".into(),
        display_name: "Coder".into(),
    });
    state.messages = vec![
        Message {
            role: Role::User,
            content: "Implement checkpoints".into(),
            tool_calls: None,
            thinking: None,
            timestamp: fixed_time(),
            response_items: Vec::new(),
        },
        Message {
            role: Role::Assistant,
            content: "Implemented".into(),
            tool_calls: None,
            thinking: None,
            timestamp: fixed_time(),
            response_items: Vec::new(),
        },
    ];
    state.context_checkpoint = Some(crate::context::ContextCheckpoint {
        format_version: 1,
        through_message_count: 2,
        summary: "## Goal\nImplement checkpoints".into(),
        created_at: fixed_time(),
        source_tokens: 42,
        source_sha256: None,
        anchors: Vec::new(),
        user_quotes: Vec::new(),
        recall_pointer: None,
    });
    state.context_breakdown = Some(crate::context::ContextBreakdown {
        source: crate::context::ContextBreakdownSource::Prepared,
        instructions: 120,
        skills: 30,
        built_in_tools: 210,
        mcp_tools: 45,
        checkpoint: 18,
        conversation: 24,
        attachments: 12,
        total: 459,
        mcp_prepared: true,
    });
    state.token_count = (120, 30);
    let mut config = crate::config::CaConfig::default();
    config.model_context.insert(
        "local/coder".into(),
        crate::config::ModelContextConfig {
            context_window_tokens: Some(100_000),
            compact_at_tokens: None,
        },
    );
    state.pager_content = crate::context_dashboard(&state, &config);
    state.pager_open = true;

    insta::assert_snapshot!(
        "context_dashboard",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
    state.pager_scroll = 22;
    insta::assert_snapshot!(
        "context_dashboard_status",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
    state.pager_scroll = 28;
    insta::assert_snapshot!(
        "context_dashboard_tasks_narrow",
        render_to_text(&state, 60, 24)
    );
}

#[test]
fn context_grid_has_stable_category_colors() {
    use ratatui::style::Color;

    for (glyph, color) in [
        ('I', Color::Cyan),
        ('S', Color::Magenta),
        ('B', Color::Yellow),
        ('M', Color::Green),
        ('K', Color::LightBlue),
        ('C', Color::White),
        ('A', Color::LightRed),
        ('·', Color::DarkGray),
    ] {
        assert_eq!(
            crate::tui::render::context_grid_style(glyph).fg,
            Some(color)
        );
    }
}

#[test]
fn checkpoint_marker_snapshots() {
    let mut state = test_state();
    state.messages = vec![
        Message {
            role: Role::User,
            content: "Original request".into(),
            tool_calls: None,
            thinking: None,
            timestamp: fixed_time(),
            response_items: Vec::new(),
        },
        Message {
            role: Role::Assistant,
            content: "Original answer".into(),
            tool_calls: None,
            thinking: None,
            timestamp: fixed_time(),
            response_items: Vec::new(),
        },
        Message {
            role: Role::User,
            content: "Continue after compaction".into(),
            tool_calls: None,
            thinking: None,
            timestamp: fixed_time(),
            response_items: Vec::new(),
        },
    ];
    state.context_checkpoint = Some(crate::context::ContextCheckpoint {
        format_version: 1,
        through_message_count: 2,
        summary: "## Goal\nContinue safely".into(),
        created_at: fixed_time(),
        source_tokens: 42,
        source_sha256: None,
        anchors: Vec::new(),
        user_quotes: Vec::new(),
        recall_pointer: None,
    });

    insta::assert_snapshot!(
        "checkpoint_marker",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

fn fallback_marker_state(streaming: bool) -> AppState {
    let mut state = test_state();
    state.messages = vec![Message {
        role: Role::User,
        content: "Continue even if the primary is temporarily unavailable".into(),
        tool_calls: None,
        thinking: None,
        timestamp: fixed_time(),
        response_items: Vec::new(),
    }];
    state
        .fallback_provenance
        .push(crate::fallback::TurnProviderProvenance {
            user_message_index: 0,
            primary_provider: "mimo".into(),
            primary_model: "mimo-v2.5-pro".into(),
            provider: "deepseek".into(),
            model: "deepseek-v4-pro".into(),
            cause: crate::fallback::ProviderFallbackCause::RateLimited,
        });
    if streaming {
        state.streaming = Some(StreamingState {
            content: "The backup provider is answering now.".into(),
            tool_calls: Vec::new(),
            thinking: None,
            last_usage: None,
            usage_handle: crate::usage::UsageHandle::test_handle(),
            response_items: Vec::new(),
            reasoning_started_at: None,
            reasoning_duration: None,
            generation_started_at: None,
        });
    } else {
        state.messages.push(Message {
            role: Role::Assistant,
            content: "The request completed through the configured backup.".into(),
            tool_calls: None,
            thinking: None,
            timestamp: fixed_time(),
            response_items: Vec::new(),
        });
    }
    state
}

#[test]
fn provider_fallback_provenance_snapshots() {
    let persisted = fallback_marker_state(false);
    insta::assert_snapshot!(
        "provider_fallback_persisted",
        wide_narrow(
            render_to_text(&persisted, 100, 30),
            render_to_text(&persisted, 60, 24)
        )
    );
    let streaming = fallback_marker_state(true);
    insta::assert_snapshot!(
        "provider_fallback_streaming",
        wide_narrow(
            render_to_text(&streaming, 100, 30),
            render_to_text(&streaming, 60, 24)
        )
    );
}

fn populated_change_review_state() -> AppState {
    let mut state = test_state();
    state.change_review.open = true;
    state.change_review.files = vec![
        crate::changes::ChangeFile {
            path: "src/main.rs".into(),
            old_path: None,
            kind: crate::changes::ChangeKind::Modified,
            conflicted: false,
            staged: true,
            unstaged: true,
            additions: Some(14),
            deletions: Some(3),
            state_fingerprint: "main-state".into(),
        },
        crate::changes::ChangeFile {
            path: "src/new_name_with_a_long_descriptive_suffix.rs".into(),
            old_path: Some("src/old_name.rs".into()),
            kind: crate::changes::ChangeKind::Renamed,
            conflicted: false,
            staged: true,
            unstaged: false,
            additions: Some(2),
            deletions: Some(1),
            state_fingerprint: "rename-state".into(),
        },
        crate::changes::ChangeFile {
            path: "notes/新規-untracked.txt".into(),
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
    state
}

#[test]
fn working_tree_change_list_snapshots() {
    let state = populated_change_review_state();

    insta::assert_snapshot!(
        "working_tree_changes",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

#[test]
fn working_tree_diff_snapshots() {
    let mut state = populated_change_review_state();
    state.change_review.mode = crate::changes::ChangeViewMode::Diff;
    state.change_review.diff_path = Some("src/main.rs".into());
    state.change_review.diff_content = "Unstaged changes\ndiff --git a/src/main.rs b/src/main.rs\n--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1,2 +1,3 @@\n-old value\n+new value with a deliberately long explanation that wraps cleanly on narrow terminal widths\n context".into();

    insta::assert_snapshot!(
        "working_tree_diff",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

fn populated_commit_review_state() -> AppState {
    let mut state = populated_change_review_state();
    state.change_review.purpose = crate::changes::ChangeReviewPurpose::Commit;
    state.change_review.loading = false;
    state.change_review.operation = None;
    state.change_review.repository = Some(crate::changes::GitRepositoryState {
        root: "/tmp/ca-git-111-review".into(),
        branch: "codex/git-111".into(),
        head: Some("0123456789abcdef0123456789abcdef01234567".into()),
    });
    state
}

fn commit_snapshot() -> crate::changes::GitCommitSnapshot {
    crate::changes::GitCommitSnapshot {
        branch: "codex/git-111".into(),
        head: Some("0123456789abcdef0123456789abcdef01234567".into()),
        index_tree: "fedcba9876543210fedcba9876543210fedcba98".into(),
        files: vec![
            crate::changes::StagedChange {
                path: "src/main.rs".into(),
                old_path: None,
                kind: crate::changes::ChangeKind::Modified,
                additions: Some(14),
                deletions: Some(3),
            },
            crate::changes::StagedChange {
                path: "assets/terminal-preview.bin".into(),
                old_path: None,
                kind: crate::changes::ChangeKind::Added,
                additions: None,
                deletions: None,
            },
            crate::changes::StagedChange {
                path: "notes/新規 release checklist with a long name.md".into(),
                old_path: None,
                kind: crate::changes::ChangeKind::Added,
                additions: Some(8),
                deletions: Some(0),
            },
        ],
    }
}

#[test]
fn commit_review_list_and_diff_snapshots() {
    let list = populated_commit_review_state();
    insta::assert_snapshot!(
        "commit_review_list",
        wide_narrow(
            render_to_text(&list, 100, 30),
            render_to_text(&list, 60, 24)
        )
    );

    let mut diff = populated_commit_review_state();
    diff.change_review.mode = crate::changes::ChangeViewMode::Diff;
    diff.change_review.diff_path = Some("src/main.rs".into());
    diff.change_review.diff_content = "Staged changes\ndiff --git a/src/main.rs b/src/main.rs\n--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1,2 +1,3 @@\n-old value\n+new staged value\n context\n\nUnstaged changes\n@@ -8,1 +8,2 @@\n keep\n+leave this line unstaged".into();
    insta::assert_snapshot!(
        "commit_review_diff",
        wide_narrow(
            render_to_text(&diff, 100, 30),
            render_to_text(&diff, 60, 24)
        )
    );
}

#[test]
fn commit_subject_and_confirmation_snapshots() {
    let mut subject = populated_commit_review_state();
    subject.change_review.mode = crate::changes::ChangeViewMode::Subject;
    subject.change_review.subject = "Stabilize Unicode café workflow".into();
    subject.change_review.subject_cursor = "Stabilize Unicode café".len();
    insta::assert_snapshot!(
        "commit_review_subject",
        wide_narrow(
            render_to_text(&subject, 100, 30),
            render_to_text(&subject, 60, 24)
        )
    );

    let mut confirmation = populated_commit_review_state();
    confirmation.change_review.mode = crate::changes::ChangeViewMode::Confirm;
    confirmation.change_review.subject = "Stabilize Unicode café workflow".into();
    confirmation.change_review.commit_snapshot = Some(commit_snapshot());
    confirmation.change_review.commit_selected = false;
    insta::assert_snapshot!(
        "commit_review_confirmation",
        wide_narrow(
            render_to_text(&confirmation, 100, 30),
            render_to_text(&confirmation, 60, 24)
        )
    );
}

#[test]
fn commit_busy_and_repository_refusal_snapshots() {
    let mut busy = populated_commit_review_state();
    busy.change_review.mode = crate::changes::ChangeViewMode::Confirm;
    busy.change_review.subject = "Keep the exact staged tree".into();
    busy.change_review.commit_snapshot = Some(commit_snapshot());
    busy.change_review.loading = true;
    busy.change_review.operation = Some(crate::changes::GitReviewOperation::Committing);
    insta::assert_snapshot!(
        "commit_review_busy",
        wide_narrow(
            render_to_text(&busy, 100, 30),
            render_to_text(&busy, 60, 24)
        )
    );

    for (name, message) in [
        (
            "commit_review_conflict",
            "Resolve conflicted paths before using /commit",
        ),
        (
            "commit_review_detached",
            "Detached HEAD is not supported by /commit; check out a named branch first",
        ),
    ] {
        let mut refused = populated_commit_review_state();
        refused.change_review.repository = None;
        refused.change_review.files.clear();
        refused.change_review.error = Some(message.into());
        insta::assert_snapshot!(
            format!("{name}"),
            wide_narrow(
                render_to_text(&refused, 100, 30),
                render_to_text(&refused, 60, 24)
            )
        );
    }
}

fn change_walkthrough_state(
    status: crate::change_walkthrough::ChangeWalkthroughStatus,
) -> AppState {
    let mut state = test_state();
    state.change_walkthrough = Some(crate::change_walkthrough::ChangeWalkthroughState {
        target: crate::changes::ReviewTarget::Working,
        provider: "local".into(),
        model: "walkthrough-model".into(),
        status,
        source_hash: Some(
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into(),
        ),
        included_files: vec!["src/changes.rs".into(), "src/tui/render.rs".into()],
        refusal: None,
        result: Some(crate::change_walkthrough::ChangeWalkthroughResult {
            summary: "The change adds a complete, hash-bound explanation flow without altering the existing diff or review commands.".into(),
            steps: vec![
                crate::change_walkthrough::WalkthroughStep {
                    order: 1,
                    title: "Capture an exact snapshot".into(),
                    files: vec!["src/changes.rs".into()],
                    explanation: "Git changes are bounded and hashed before any provider request begins.".into(),
                },
                crate::change_walkthrough::WalkthroughStep {
                    order: 2,
                    title: "Render the ordered explanation".into(),
                    files: vec!["src/tui/render.rs".into()],
                    explanation: "The overlay keeps the source identity visible and labels stale output instead of silently regenerating it.".into(),
                },
            ],
            notes: vec!["Suggestions are explanatory only; no edits are applied.".into()],
        }),
        error: None,
        freshness_error: None,
        scroll: 0,
        generation: 7,
    });
    state
}

#[test]
fn change_walkthrough_capturing_complete_and_stale_snapshots() {
    let mut capturing =
        change_walkthrough_state(crate::change_walkthrough::ChangeWalkthroughStatus::Capturing);
    let walkthrough = capturing.change_walkthrough.as_mut().unwrap();
    walkthrough.source_hash = None;
    walkthrough.included_files.clear();
    walkthrough.result = None;
    insta::assert_snapshot!(
        "change_walkthrough_capturing",
        wide_narrow(
            render_to_text(&capturing, 100, 30),
            render_to_text(&capturing, 60, 24)
        )
    );

    let mut generating =
        change_walkthrough_state(crate::change_walkthrough::ChangeWalkthroughStatus::Generating);
    generating.change_walkthrough.as_mut().unwrap().result = None;
    insta::assert_snapshot!(
        "change_walkthrough_generating",
        wide_narrow(
            render_to_text(&generating, 100, 30),
            render_to_text(&generating, 60, 24)
        )
    );

    let complete =
        change_walkthrough_state(crate::change_walkthrough::ChangeWalkthroughStatus::Complete);
    insta::assert_snapshot!(
        "change_walkthrough_complete",
        wide_narrow(
            render_to_text(&complete, 100, 30),
            render_to_text(&complete, 60, 24)
        )
    );

    let stale = change_walkthrough_state(crate::change_walkthrough::ChangeWalkthroughStatus::Stale);
    insta::assert_snapshot!(
        "change_walkthrough_stale",
        wide_narrow(
            render_to_text(&stale, 100, 30),
            render_to_text(&stale, 60, 24)
        )
    );
}

#[test]
fn change_walkthrough_refused_and_failed_snapshots() {
    let mut refused =
        change_walkthrough_state(crate::change_walkthrough::ChangeWalkthroughStatus::Refused);
    let walkthrough = refused.change_walkthrough.as_mut().unwrap();
    walkthrough.source_hash = None;
    walkthrough.included_files.clear();
    walkthrough.result = None;
    walkthrough.refusal = Some(crate::change_walkthrough::WalkthroughRefusal {
        reason: "Untracked file contents are not captured; stage them or remove them before generating a complete walkthrough".into(),
        omitted_files: vec!["src/oversized.rs".into()],
        untracked_files: vec!["notes/private-draft.txt".into()],
    });
    insta::assert_snapshot!(
        "change_walkthrough_refused",
        wide_narrow(
            render_to_text(&refused, 100, 30),
            render_to_text(&refused, 60, 24)
        )
    );

    let mut empty =
        change_walkthrough_state(crate::change_walkthrough::ChangeWalkthroughStatus::Refused);
    let walkthrough = empty.change_walkthrough.as_mut().unwrap();
    walkthrough.source_hash = None;
    walkthrough.included_files.clear();
    walkthrough.result = None;
    walkthrough.refusal = Some(crate::change_walkthrough::WalkthroughRefusal {
        reason: "No textual tracked changes are available for this target".into(),
        omitted_files: Vec::new(),
        untracked_files: Vec::new(),
    });
    insta::assert_snapshot!(
        "change_walkthrough_empty",
        wide_narrow(
            render_to_text(&empty, 100, 30),
            render_to_text(&empty, 60, 24)
        )
    );

    let mut failed =
        change_walkthrough_state(crate::change_walkthrough::ChangeWalkthroughStatus::Failed);
    failed.change_walkthrough.as_mut().unwrap().result = None;
    failed.change_walkthrough.as_mut().unwrap().error =
        Some("Walkthrough failed for local: provider timed out before usable output".into());
    insta::assert_snapshot!(
        "change_walkthrough_failed",
        wide_narrow(
            render_to_text(&failed, 100, 30),
            render_to_text(&failed, 60, 24)
        )
    );
}

fn jobs_state() -> AppState {
    let mut state = test_state();
    state.jobs_view_open = true;
    state.job_list = vec![
        crate::jobs::JobInfo {
            id: "00000000-0000-4000-8000-000000000001".parse().unwrap(),
            command: "cargo test --workspace".into(),
            cwd: "/tmp/project".into(),
            status: crate::jobs::JobStatus::Running,
            elapsed: std::time::Duration::from_millis(1234),
            stdout: "test output began\n[… stdout: 2048 bytes omitted …]\nrunning 12 tests\n"
                .into(),
            stderr: String::new(),
            stdout_omitted_bytes: 2048,
            stderr_omitted_bytes: 0,
            background: true,
        },
        crate::jobs::JobInfo {
            id: "00000000-0000-4000-8000-000000000002".parse().unwrap(),
            command: "printf complete".into(),
            cwd: "/tmp/project".into(),
            status: crate::jobs::JobStatus::Completed { exit_code: 0 },
            elapsed: std::time::Duration::from_millis(250),
            stdout: "complete\n".into(),
            stderr: "one warning\n".into(),
            stdout_omitted_bytes: 0,
            stderr_omitted_bytes: 0,
            background: false,
        },
    ];
    state
}

fn watcher_snapshot(
    id: &str,
    label: &str,
    host: ca_watch::HostSpec,
    pid: u32,
    process: ca_watch::ProcessSpec,
    status: (ca_watch::WatchState, &str),
    elapsed_secs: i64,
) -> ca_watch::WatchSnapshotV1 {
    let (state, summary) = status;
    let started_at = chrono::DateTime::parse_from_rfc3339("2026-08-26T17:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    ca_watch::WatchSnapshotV1 {
        version: 1,
        watch_id: id.parse().unwrap(),
        request: ca_watch::WatchRequestV1 {
            label: label.into(),
            host,
            pid,
            process,
            interval_secs: 10,
        },
        started_at,
        event: ca_watch::WatchEventV1 {
            version: 1,
            watch_id: id.parse().unwrap(),
            sequence: 4,
            observed_at: started_at + chrono::Duration::seconds(elapsed_secs),
            state,
            summary: summary.into(),
        },
        consecutive_transport_failures: if state == ca_watch::WatchState::Unreachable {
            3
        } else {
            0
        },
    }
}

fn watchers_state() -> AppState {
    let mut state = jobs_state();
    state.watch_list = vec![
        watcher_snapshot(
            "00000000-0000-4000-8000-000000000011",
            "Seedbox copy",
            ca_watch::HostSpec::Ssh {
                destination: "root@bsd".into(),
                port: None,
            },
            55_347,
            ca_watch::ProcessSpec::Copy {
                log_path: Some("/home/aji/copy-seedbox1-ratio.log".into()),
                completion_marker: "/bigmac/seed-ro/seedbox1/.copy-complete".into(),
            },
            (
                ca_watch::WatchState::Running,
                "item 53 of 993, approximately 15 GB copied",
            ),
            15,
        ),
        watcher_snapshot(
            "00000000-0000-4000-8000-000000000012",
            "Local index",
            ca_watch::HostSpec::Local,
            98_765,
            ca_watch::ProcessSpec::Generic,
            (
                ca_watch::WatchState::Unreachable,
                "Target remains unreachable; observation will continue",
            ),
            30,
        ),
    ];
    state.latest_background_activity = Some(crate::app::BackgroundActivityRef::Watch(
        state.watch_list[0].watch_id,
    ));
    state
}

#[test]
fn jobs_list_snapshots() {
    let state = jobs_state();
    insta::assert_snapshot!(
        "jobs_list",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

#[test]
fn job_detail_snapshots() {
    let mut state = jobs_state();
    state.job_detail_open = true;
    insta::assert_snapshot!(
        "job_detail",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

#[test]
fn token_free_activity_strip_snapshots() {
    let mut state = watchers_state();
    state.jobs_view_open = false;
    insta::assert_snapshot!(
        "background_activity_strip",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

#[test]
fn unified_jobs_and_watchers_snapshots() {
    let state = watchers_state();
    insta::assert_snapshot!(
        "jobs_and_watchers",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );

    let mut detail = watchers_state();
    detail.selected_job_idx = detail.job_list.len();
    detail.job_detail_open = true;
    insta::assert_snapshot!(
        "watcher_detail",
        wide_narrow(
            render_to_text(&detail, 100, 30),
            render_to_text(&detail, 60, 24)
        )
    );
}

fn delegations_state() -> AppState {
    let mut state = test_state();
    state.jobs_view_open = true;
    state.delegation_list = vec![
        crate::mechanistic_delegation::DelegationActivitySnapshot {
            job_id: "delegation-running-190".into(),
            server: "codex_team_delegate".into(),
            action_categories: vec!["evidence".into()],
            status: crate::mechanistic_delegation::DelegationActivityStatus::Running,
            started_at: "2026-08-26T18:00:00Z".into(),
            updated_at: "2026-08-26T18:00:20Z".into(),
            repository_revision: "a".repeat(64),
            receipt_digest: None,
            broker_disposition: Some("executed".into()),
            source_receipt_digest: None,
            consumer_count: Some(2),
            avoided_execution_millis: 0,
        },
        crate::mechanistic_delegation::DelegationActivitySnapshot {
            job_id: "delegation-complete-189".into(),
            server: "codex_team_delegate".into(),
            action_categories: vec!["observation".into()],
            status: crate::mechanistic_delegation::DelegationActivityStatus::Completed,
            started_at: "2026-08-26T17:58:00Z".into(),
            updated_at: "2026-08-26T17:59:00Z".into(),
            repository_revision: "b".repeat(64),
            receipt_digest: Some("c".repeat(64)),
            broker_disposition: Some("cache_hit".into()),
            source_receipt_digest: Some("d".repeat(64)),
            consumer_count: Some(1),
            avoided_execution_millis: 420,
        },
    ];
    state
}

#[test]
fn codex_team_delegation_jobs_snapshots() {
    let state = delegations_state();
    insta::assert_snapshot!(
        "delegation_jobs",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );

    let mut detail = delegations_state();
    detail.job_detail_open = true;
    insta::assert_snapshot!(
        "delegation_detail",
        wide_narrow(
            render_to_text(&detail, 100, 30),
            render_to_text(&detail, 60, 24)
        )
    );
}

#[test]
fn watcher_completion_and_failure_toasts_stay_in_window() {
    let mut completed = test_state();
    completed.show_toast(
        "Seedbox copy: Completion marker detected",
        crate::app::ToastKind::Success,
    );
    insta::assert_snapshot!(
        "watcher_completed_toast",
        render_to_text(&completed, 80, 20)
    );

    let mut failed = test_state();
    failed.show_toast(
        "Seedbox copy: Process exited before the completion marker appeared",
        crate::app::ToastKind::Error,
    );
    insta::assert_snapshot!("watcher_failed_toast", render_to_text(&failed, 80, 20));
}

fn terminal_manager_state() -> Box<AppState> {
    let mut state = Box::new(test_state());
    state.terminal_manager = Some(crate::app::TerminalManagerState {
        sessions: vec![
            crate::terminal_sessions::TerminalSession {
                name: "dev-api".into(),
                windows: 3,
                attached_clients: 1,
                created_unix: 1,
            },
            crate::terminal_sessions::TerminalSession {
                name: "local-repl".into(),
                windows: 1,
                attached_clients: 0,
                created_unix: 2,
            },
        ],
        selected_idx: 0,
        filter: String::new(),
        version: Some("tmux 3.5a".into()),
        unavailable: false,
        busy: false,
        mode: crate::app::TerminalManagerMode::List,
        kill_confirmation: None,
        notice: None,
        error: None,
    });
    state
}

#[test]
fn durable_terminal_manager_snapshots() {
    let populated = terminal_manager_state();
    let mut empty = terminal_manager_state();
    empty.terminal_manager.as_mut().unwrap().sessions.clear();
    let mut unavailable = terminal_manager_state();
    let unavailable_manager = unavailable.terminal_manager.as_mut().unwrap();
    unavailable_manager.sessions.clear();
    unavailable_manager.unavailable = true;
    unavailable_manager.version = None;
    unavailable_manager.error = Some("tmux is not installed or is not available on PATH".into());
    let mut creating = terminal_manager_state();
    creating.terminal_manager.as_mut().unwrap().mode = crate::app::TerminalManagerMode::Create {
        name: "new-dev".into(),
        cursor: 7,
    };
    let mut busy = terminal_manager_state();
    busy.terminal_manager.as_mut().unwrap().busy = true;
    let mut confirmation = terminal_manager_state();
    confirmation
        .terminal_manager
        .as_mut()
        .unwrap()
        .kill_confirmation = Some(crate::app::TerminalKillConfirmation {
        name: "dev-api".into(),
        kill_selected: false,
    });
    let mut failed = terminal_manager_state();
    failed.terminal_manager.as_mut().unwrap().error = Some("tmux command failed (exit 1)".into());

    for (name, state) in [
        ("term_populated", populated),
        ("term_empty", empty),
        ("term_unavailable", unavailable),
        ("term_create", creating),
        ("term_busy", busy),
        ("term_kill_confirmation", confirmation),
        ("term_failure", failed),
    ] {
        insta::assert_snapshot!(
            format!("{name}"),
            wide_narrow(
                render_to_text(&state, 100, 30),
                render_to_text(&state, 60, 24)
            )
        );
    }
}

#[test]
fn unicode_input_render_clamps_non_boundary_cursor_without_panicking() {
    // "A🙂界e\u{301}Z" — 🙂 is 4 bytes (indices 1..5), 界 starts at 5.
    // Place cursor at byte 3 (interior of 🙂). split_at(3) would panic.
    let mut state = test_state();
    state.input = "A🙂界e\u{301}Z".to_string();
    state.input_cursor = 3; // interior of the 4-byte 🙂 emoji
                            // Must not panic — render should floor cursor to a valid boundary.
    let _ = render_to_text(&state, 100, 30);
}

#[test]
fn unicode_cursor_snapshots() {
    let mut state = test_state();
    state.input = "A🙂界e\u{301}Z".to_string();
    // Cursor after A🙂 → byte 5, which is a valid boundary at start of 界
    state.input_cursor = 5;
    insta::assert_snapshot!(
        "uni004_unicode_cursor",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

#[test]
fn multiline_overflow_and_atomic_chip_snapshots() {
    let mut overflow = test_state();
    overflow.input = (0..10)
        .map(|index| format!("visual row {index} 你e\u{301}"))
        .collect::<Vec<_>>()
        .join("\n");
    overflow.input_cursor = overflow.input.find("visual row 8").unwrap();
    insta::assert_snapshot!(
        "composer_multiline_overflow",
        wide_narrow(
            render_to_text(&overflow, 100, 30),
            render_to_text(&overflow, 60, 24)
        )
    );

    let mut chips = test_state();
    let reference = "@src/main.rs";
    let paste = "[Pasted Content 12438 chars]";
    chips.input = format!("Review {reference} and {paste}");
    chips.input_cursor = chips.input.len();
    let reference_start = chips.input.find(reference).unwrap();
    let paste_start = chips.input.find(paste).unwrap();
    chips
        .composer_ui
        .atomic_elements
        .push(crate::tui::composer::AtomicElement {
            range: reference_start..reference_start + reference.len(),
            kind: crate::tui::composer::AtomicKind::File,
        });
    chips
        .composer_ui
        .register_large_paste(paste_start..paste_start + paste.len(), "x".repeat(12_438));
    chips.pasted_images.push(
        crate::image_content::RawClipboardImage::new(1, 1, vec![0, 0, 0, 255])
            .unwrap()
            .normalize()
            .unwrap(),
    );
    insta::assert_snapshot!(
        "composer_chips",
        wide_narrow(
            render_to_text(&chips, 100, 30),
            render_to_text(&chips, 60, 24)
        )
    );
}

#[test]
fn structured_choice_interaction_snapshots() {
    let mut state = test_state();
    let request = crate::interaction::InteractionRequest::from_model(
        crate::interaction::RequestUserInputArgs {
            question: "Which validation should run?".into(),
            description: Some("Choose one option; the composer draft stays untouched.".into()),
            kind: crate::interaction::ModelInteractionKind::SingleChoice,
            choices: vec![
                crate::interaction::InteractionChoice {
                    label: "Focused tests".into(),
                    value: Some("focused".into()),
                    description: Some("Run only the affected module tests.".into()),
                },
                crate::interaction::InteractionChoice {
                    label: "Full validation".into(),
                    value: Some("full".into()),
                    description: Some("Run tests, format, Clippy, and release build.".into()),
                },
            ],
            allow_other: true,
        },
    )
    .unwrap();
    state.interaction = Some(crate::interaction::InteractionState::host(request));
    state.input = "preserved composer draft".into();
    state.input_cursor = state.input.len();

    insta::assert_snapshot!(
        "structured_interaction_choice",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

#[test]
fn learned_recipe_review_uses_plan_cockpit_theme_at_wide_and_narrow_widths() {
    let mut state = test_state();
    state.ui_theme = crate::ui_settings::UiTheme::Mimocode;
    state.mode = crate::app::AgentMode::Plan;
    state.learned_recipe_review = Some(crate::app::LearnedRecipeReview {
        server: "codex-team".into(),
        recipe_id: "cargo-check".into(),
        digest: "a".repeat(64),
        repository_revision: "b".repeat(64),
        markdown: "# Learned Rust Recipe".into(),
    });
    state.interaction = Some(crate::interaction::InteractionState::host(
        crate::interaction::InteractionRequest {
            question: "Review learned Rust recipe cargo-check".into(),
            description: Some(
                "Global approval automatically activates exact local matches.".into(),
            ),
            mode: crate::interaction::InteractionMode::SingleChoice,
            choices: vec![
                crate::interaction::InteractionChoice {
                    label: "Approve globally".into(),
                    value: Some("approve".into()),
                    description: Some(
                        "Publish this exact digest after isolated validation.".into(),
                    ),
                },
                crate::interaction::InteractionChoice {
                    label: "Reject".into(),
                    value: Some("reject".into()),
                    description: None,
                },
                crate::interaction::InteractionChoice {
                    label: "Later".into(),
                    value: Some("later".into()),
                    description: None,
                },
            ],
            allow_other: false,
        },
    ));

    insta::assert_snapshot!(
        "learned_recipe_review",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
    let rendered = render_to_text(&state, 100, 30);
    assert!(rendered.contains("Learned Rust Recipe"));
    assert!(rendered.contains("Approve globally"));
}

#[test]
fn autonomous_learning_controls_are_readable_at_wide_and_narrow_widths() {
    let mut state = test_state();
    state.ui_theme = crate::ui_settings::UiTheme::Mimocode;
    state.mode = crate::app::AgentMode::Plan;
    state.self_improve_review = Some(crate::app::SelfImproveReview {
        server: "codex-team".into(),
        revision: "c".repeat(64),
        paused: false,
        markdown: "# Autonomous Learning Fabric".into(),
    });
    state.interaction = Some(crate::interaction::InteractionState::host(
        crate::interaction::InteractionRequest {
            question: "Autonomous learning controls".into(),
            description: Some("Host-owned and revision-bound; no model request.".into()),
            mode: crate::interaction::InteractionMode::SingleChoice,
            choices: vec![
                crate::interaction::InteractionChoice {
                    label: "Pause".into(),
                    value: Some("pause".into()),
                    description: Some("Stop new experiments and deployments.".into()),
                },
                crate::interaction::InteractionChoice {
                    label: "Rollback ca".into(),
                    value: Some("rollback-ca".into()),
                    description: Some("Restore the prior verified slot.".into()),
                },
                crate::interaction::InteractionChoice {
                    label: "Later".into(),
                    value: Some("later".into()),
                    description: None,
                },
            ],
            allow_other: false,
        },
    ));

    for (width, height) in [(100, 30), (60, 24)] {
        let rendered = render_to_text(&state, width, height);
        assert!(rendered.contains("Autonomous Learning"));
        assert!(rendered.contains("Pause"));
        assert!(rendered.contains("Rollback ca"));
        assert!(rendered.contains("no model request"));
    }
}

#[test]
fn structured_free_text_interaction_snapshots() {
    let mut state = test_state();
    let request = crate::interaction::InteractionRequest::from_legacy_question(
        "What should the branch be called?".into(),
    )
    .unwrap();
    let mut interaction = crate::interaction::InteractionState::host(request);
    for character in "codex/interaction".chars() {
        interaction.handle_key(
            crossterm::event::KeyCode::Char(character),
            crossterm::event::KeyModifiers::NONE,
        );
    }
    state.interaction = Some(interaction);

    insta::assert_snapshot!(
        "structured_interaction_text",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

fn portability_state(operation: crate::app::SessionPortabilityOperation, path: &str) -> AppState {
    let mut state = test_state();
    state.session_portability_generation = 4;
    state.session_portability = Some(crate::app::SessionPortabilityState {
        operation,
        path: std::path::PathBuf::from(path),
        generation: 4,
        busy: true,
        awaiting_overwrite: false,
        overwrite_selected: false,
        import_selected: false,
        import_learning_eligible: false,
        import_preview: None,
        error: None,
    });
    state
}

#[test]
fn session_export_overwrite_and_busy_snapshots() {
    let mut overwrite = portability_state(
        crate::app::SessionPortabilityOperation::Export {
            format: crate::session_portability::ExportFormat::Json,
            overwrite: false,
        },
        "/private/tmp/a very long workspace path/exports/current conversation.json",
    );
    let overlay = overwrite.session_portability.as_mut().unwrap();
    overlay.busy = false;
    overlay.awaiting_overwrite = true;
    insta::assert_snapshot!(
        "session_export_overwrite",
        wide_narrow(
            render_to_text(&overwrite, 100, 30),
            render_to_text(&overwrite, 60, 24)
        )
    );

    let busy = portability_state(
        crate::app::SessionPortabilityOperation::Export {
            format: crate::session_portability::ExportFormat::Markdown,
            overwrite: false,
        },
        "/private/tmp/a very long workspace path/exports/current conversation.md",
    );
    insta::assert_snapshot!(
        "session_export_busy",
        wide_narrow(
            render_to_text(&busy, 100, 30),
            render_to_text(&busy, 60, 24)
        )
    );
}

#[test]
fn session_import_busy_and_failure_snapshots() {
    let busy = portability_state(
        crate::app::SessionPortabilityOperation::Import,
        "/private/tmp/imports/portable history.json",
    );
    insta::assert_snapshot!(
        "session_import_busy",
        wide_narrow(
            render_to_text(&busy, 100, 30),
            render_to_text(&busy, 60, 24)
        )
    );

    let mut failed = portability_state(
        crate::app::SessionPortabilityOperation::Import,
        "/private/tmp/imports/portable history.json",
    );
    let overlay = failed.session_portability.as_mut().unwrap();
    overlay.busy = false;
    overlay.error =
        Some("Session import failed: unresolved tool call call-1 cannot be imported".into());
    insta::assert_snapshot!(
        "session_import_failure",
        wide_narrow(
            render_to_text(&failed, 100, 30),
            render_to_text(&failed, 60, 24)
        )
    );
}

#[test]
fn foreign_session_import_preview_snapshots() {
    let mut preview = portability_state(
        crate::app::SessionPortabilityOperation::ForeignImport {
            source: crate::session::ForeignTranscriptSource::Claude,
            commit: false,
            learning_eligible: false,
        },
        "/private/tmp/imports/claude histories",
    );
    preview.active_profile = crate::runtime::ActiveProfile::Named("work".into());
    let overlay = preview.session_portability.as_mut().unwrap();
    overlay.busy = false;
    overlay.import_preview = Some(crate::foreign_import::ForeignImportPreview {
        source: crate::session::ForeignTranscriptSource::Claude,
        source_label: "claude histories".into(),
        source_files: 4,
        source_bytes: 12_345,
        sessions: 3,
        messages: 42,
        skipped_items: 9,
        errors: vec!["old.jsonl: malformed JSONL record".into()],
        learning_eligible: false,
    });
    insta::assert_snapshot!(
        "foreign_session_import_preview",
        wide_narrow(
            render_to_text(&preview, 100, 30),
            render_to_text(&preview, 60, 24)
        )
    );
}

fn retention_render_state() -> AppState {
    let mut state = test_state();
    state.session_retention_generation = 5;
    state.session_retention = Some(crate::app::SessionRetentionState {
        settings: crate::session_retention::RetentionSettings::default(),
        mode: crate::app::SessionRetentionMode::Overview,
        field: crate::app::SessionRetentionField::Days,
        days_input: String::new(),
        count_input: String::new(),
        cursor: 0,
        preview: None,
        preview_selected_idx: 0,
        delete_selected: false,
        busy: None,
        writes_enabled: true,
        sessions_enabled: true,
        active_count: 7,
        archived_count: 2,
        generation: 5,
        notice: None,
        error: None,
    });
    state
}

fn retention_preview(candidates: usize) -> crate::session_retention::RetentionPreview {
    let generated_at = chrono::DateTime::parse_from_rfc3339("2026-08-10T12:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    crate::session_retention::RetentionPreview {
        settings: crate::session_retention::RetentionSettings {
            days: Some(30),
            count: Some(3),
        },
        generated_at,
        active_count: 7,
        protected_current: 1,
        protected_pinned: 1,
        candidates: (0..candidates)
            .map(|index| crate::session_retention::RetentionCandidate {
                storage_key: format!("saved-{index}"),
                id: uuid::Uuid::from_u128(index as u128 + 1),
                title: if index == 1 {
                    "A very long saved session title that is safely clipped".into()
                } else {
                    format!("Expired session {}", index + 1)
                },
                updated_at: generated_at - chrono::Duration::days(90 - index as i64),
                reason: match index % 3 {
                    0 => crate::session_retention::RetentionReason::Both,
                    1 => crate::session_retention::RetentionReason::Age,
                    _ => crate::session_retention::RetentionReason::Count,
                },
            })
            .collect(),
    }
}

#[test]
fn session_retention_overview_and_editor_snapshots() {
    let disabled = retention_render_state();
    insta::assert_snapshot!(
        "session_retention_disabled",
        wide_narrow(
            render_to_text(&disabled, 100, 30),
            render_to_text(&disabled, 60, 24)
        )
    );

    let mut editor = retention_render_state();
    let manager = editor.session_retention.as_mut().unwrap();
    manager.mode = crate::app::SessionRetentionMode::Edit;
    manager.days_input = "30".into();
    manager.count_input = "100".into();
    manager.field = crate::app::SessionRetentionField::Count;
    manager.cursor = 3;
    insta::assert_snapshot!(
        "session_retention_editor",
        wide_narrow(
            render_to_text(&editor, 100, 30),
            render_to_text(&editor, 60, 24)
        )
    );
}

#[test]
fn session_retention_empty_and_populated_preview_snapshots() {
    let mut empty = retention_render_state();
    let manager = empty.session_retention.as_mut().unwrap();
    manager.settings = crate::session_retention::RetentionSettings {
        days: Some(30),
        count: Some(3),
    };
    manager.mode = crate::app::SessionRetentionMode::Preview;
    manager.preview = Some(retention_preview(0));
    insta::assert_snapshot!(
        "session_retention_empty_preview",
        wide_narrow(
            render_to_text(&empty, 100, 30),
            render_to_text(&empty, 60, 24)
        )
    );

    let mut populated = retention_render_state();
    let manager = populated.session_retention.as_mut().unwrap();
    manager.settings = crate::session_retention::RetentionSettings {
        days: Some(30),
        count: Some(3),
    };
    manager.mode = crate::app::SessionRetentionMode::Preview;
    manager.preview = Some(retention_preview(3));
    insta::assert_snapshot!(
        "session_retention_preview",
        wide_narrow(
            render_to_text(&populated, 100, 30),
            render_to_text(&populated, 60, 24)
        )
    );
}

#[test]
fn session_retention_busy_bare_and_failure_snapshots() {
    let mut busy = retention_render_state();
    let manager = busy.session_retention.as_mut().unwrap();
    manager.settings = crate::session_retention::RetentionSettings {
        days: Some(30),
        count: Some(3),
    };
    manager.mode = crate::app::SessionRetentionMode::Preview;
    manager.preview = Some(retention_preview(3));
    manager.delete_selected = true;
    manager.busy = Some(crate::app::SessionRetentionBusy::Deleting);
    insta::assert_snapshot!(
        "session_retention_deleting",
        wide_narrow(
            render_to_text(&busy, 100, 30),
            render_to_text(&busy, 60, 24)
        )
    );

    let mut bare = retention_render_state();
    bare.active_profile = crate::runtime::ActiveProfile::Bare;
    bare.run_capabilities = crate::runtime::RunCapabilities::for_profile(&bare.active_profile);
    let manager = bare.session_retention.as_mut().unwrap();
    manager.writes_enabled = false;
    manager.sessions_enabled = false;
    manager.active_count = 0;
    manager.archived_count = 0;
    manager.notice = Some(
        "This run has no persistent session store; settings may be edited for a future run".into(),
    );
    insta::assert_snapshot!(
        "session_retention_bare",
        wide_narrow(
            render_to_text(&bare, 100, 30),
            render_to_text(&bare, 60, 24)
        )
    );

    let mut failed = retention_render_state();
    let manager = failed.session_retention.as_mut().unwrap();
    manager.settings = crate::session_retention::RetentionSettings {
        days: Some(30),
        count: Some(3),
    };
    manager.mode = crate::app::SessionRetentionMode::Preview;
    manager.preview = Some(retention_preview(2));
    manager.error = Some(
        "retention preflight failed: a session became pinned (2 remaining from the confirmed batch)"
            .into(),
    );
    insta::assert_snapshot!(
        "session_retention_failure",
        wide_narrow(
            render_to_text(&failed, 100, 30),
            render_to_text(&failed, 60, 24)
        )
    );
}

fn workflow_loaded() -> crate::workflow::LoadedWorkflow {
    crate::workflow::LoadedWorkflow {
        definition: crate::workflow::WorkflowDefinition {
            version: 1,
            id: "release-check".into(),
            title: "Release readiness".into(),
            steps: vec![
                crate::workflow::WorkflowStep {
                    id: "inspect".into(),
                    depends_on: Vec::new(),
                    prompt: "Inspect the change".into(),
                    model: Some("mimo/mimo-v2.5".into()),
                    tools: Some(vec!["read_file".into()]),
                    grants: Vec::new(),
                    retry: crate::workflow::WorkflowRetryPolicy {
                        max_retries: 1,
                        causes: vec![crate::workflow::WorkflowRetryCause::ProviderError],
                    },
                    output: Some("inspection".into()),
                    evidence: vec![crate::workflow::WorkflowEvidenceGate::AssistantOutput],
                },
                crate::workflow::WorkflowStep {
                    id: "verify".into(),
                    depends_on: vec!["inspect".into()],
                    prompt: "Verify the result".into(),
                    model: None,
                    tools: Some(Vec::new()),
                    grants: Vec::new(),
                    retry: crate::workflow::WorkflowRetryPolicy::default(),
                    output: None,
                    evidence: vec![crate::workflow::WorkflowEvidenceGate::AssistantOutput],
                },
            ],
        }
        .validate()
        .unwrap(),
        relative_path: ".ca/workflows/release.toml".into(),
        digest: "0123456789abcdef0123456789abcdef".into(),
    }
}

#[test]
fn workflow_preview_and_clear_snapshots() {
    let mut preview = test_state();
    preview.workflow_preview = Some(crate::workflow::WorkflowPreview {
        loaded: workflow_loaded(),
        resume_existing: false,
        run_selected: false,
        error: None,
    });
    insta::assert_snapshot!(
        "workflow_preview",
        wide_narrow(
            render_to_text(&preview, 100, 30),
            render_to_text(&preview, 60, 24)
        )
    );

    let loaded = workflow_loaded();
    let mut clear = test_state();
    clear.workflow_run = Some(crate::workflow::WorkflowRun::start(
        &loaded,
        "mimo".into(),
        "mimo-v2.5".into(),
        chrono::DateTime::parse_from_rfc3339("2026-08-11T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc),
    ));
    clear.workflow_clear_confirmation = Some(false);
    insta::assert_snapshot!(
        "workflow_clear",
        wide_narrow(
            render_to_text(&clear, 100, 30),
            render_to_text(&clear, 60, 24)
        )
    );
}

#[test]
fn builtin_workflow_tree_snapshots() {
    let loaded = crate::workflow::builtin_template("deep-research").unwrap();
    let mut definition = test_state();
    definition.pager_content = crate::workflow::render_definition_tree(&loaded);
    definition.pager_open = true;
    insta::assert_snapshot!(
        "builtin_workflow_tree",
        wide_narrow(
            render_to_text(&definition, 100, 30),
            render_to_text(&definition, 60, 24)
        )
    );

    let mut run = crate::workflow::WorkflowRun::start(
        &loaded,
        "provider".into(),
        "model".into(),
        chrono::DateTime::parse_from_rfc3339("2026-08-29T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc),
    );
    run.id = uuid::Uuid::parse_str("11111111-2222-4333-8444-555555555555").unwrap();
    run.begin_step(
        &loaded.definition,
        "brief",
        "provider".into(),
        "model".into(),
        chrono::DateTime::parse_from_rfc3339("2026-08-29T12:00:01Z")
            .unwrap()
            .with_timezone(&chrono::Utc),
    )
    .unwrap();
    let mut live = test_state();
    live.pager_content = crate::workflow::render_run_tree(&run, Some(&loaded));
    live.pager_open = true;
    insta::assert_snapshot!(
        "builtin_workflow_run_tree",
        wide_narrow(
            render_to_text(&live, 100, 30),
            render_to_text(&live, 60, 24)
        )
    );
}

fn workflow_status_state(status: crate::workflow::WorkflowRunStatus) -> AppState {
    let loaded = workflow_loaded();
    let now = chrono::DateTime::parse_from_rfc3339("2026-08-11T12:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    let mut run =
        crate::workflow::WorkflowRun::start(&loaded, "mimo".into(), "mimo-v2.5".into(), now);
    match status {
        crate::workflow::WorkflowRunStatus::Running => {
            run.begin_step(
                &loaded.definition,
                "inspect",
                "mimo".into(),
                "mimo-v2.5".into(),
                now,
            )
            .unwrap();
        }
        crate::workflow::WorkflowRunStatus::Paused => {
            run.pause(crate::workflow::WorkflowPauseReason::User, now);
        }
        crate::workflow::WorkflowRunStatus::Complete => {
            run.steps.iter_mut().for_each(|step| {
                step.status = crate::workflow::WorkflowStepStatus::Succeeded;
            });
            run.status = status;
        }
        crate::workflow::WorkflowRunStatus::Failed => {
            run.steps[0].status = crate::workflow::WorkflowStepStatus::Failed;
            run.steps[0].error = Some("provider request failed safely".into());
            run.status = status;
        }
        crate::workflow::WorkflowRunStatus::Blocked => {
            run.steps[0].status = crate::workflow::WorkflowStepStatus::Failed;
            run.steps[1].status = crate::workflow::WorkflowStepStatus::Blocked;
            run.steps[1].error = Some("dependency did not succeed".into());
            run.status = status;
        }
        crate::workflow::WorkflowRunStatus::Cleared => run.status = status,
    }
    let mut state = test_state();
    state.pager_content = crate::workflow::render_status(&run);
    state.pager_open = true;
    state.workflow_run = Some(run);
    state
}

#[test]
fn workflow_lifecycle_snapshots() {
    for (name, status) in [
        (
            "workflow_running",
            crate::workflow::WorkflowRunStatus::Running,
        ),
        (
            "workflow_paused",
            crate::workflow::WorkflowRunStatus::Paused,
        ),
        (
            "workflow_failed",
            crate::workflow::WorkflowRunStatus::Failed,
        ),
        (
            "workflow_blocked",
            crate::workflow::WorkflowRunStatus::Blocked,
        ),
        (
            "workflow_complete",
            crate::workflow::WorkflowRunStatus::Complete,
        ),
    ] {
        let state = workflow_status_state(status);
        insta::assert_snapshot!(
            format!("{name}"),
            wide_narrow(
                render_to_text(&state, 100, 30),
                render_to_text(&state, 60, 24)
            )
        );
    }
}

#[test]
fn workflow_waiting_approval_snapshots() {
    let mut state = workflow_status_state(crate::workflow::WorkflowRunStatus::Running);
    state.pager_open = false;
    state.messages.push(Message {
        role: Role::Assistant,
        content: "The verification needs a command.".into(),
        tool_calls: Some(vec![ToolCall {
            id: "workflow-call".into(),
            name: "run_command".into(),
            arguments: r#"{"command":"cargo test workflow"}"#.into(),
            status: ToolStatus::Running,
            output: None,
        }]),
        thinking: None,
        timestamp: chrono::Utc::now(),
        response_items: Vec::new(),
    });
    state.approval_queue.begin_batch(vec![PendingApproval {
        tool_name: "run_command".into(),
        tool_args: r#"{"command":"cargo test workflow"}"#.into(),
        tool_id: "workflow-call".into(),
        network_requested: false,
        email_preview: None,
        email_draft_ref: None,
    }]);
    state.status_message = Some("Tool needs approval [1/1] (y/n/a/g/s/x/p)".into());
    insta::assert_snapshot!(
        "workflow_waiting_approval",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

fn task_graph_render_state() -> AppState {
    let now = chrono::DateTime::parse_from_rfc3339("2026-08-11T12:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    let first = crate::work::TaskId(uuid::Uuid::from_u128(1));
    let second = crate::work::TaskId(uuid::Uuid::from_u128(2));
    let plan_id = crate::work::PlanId(uuid::Uuid::from_u128(9));
    let definition = |depends_on| {
        let evidence = vec![crate::agent_graph::AgentEvidenceGate::AssistantOutput];
        crate::agent_graph::AgentTaskDefinition {
            depends_on,
            grant: crate::agent_graph::AgentTaskGrant {
                tools: vec!["read_file".into()],
                paths: vec!["src".into()],
                commands: Vec::new(),
                network: false,
            },
            worktree_id: None,
            accept_cross_lane_handoff: false,
            max_attempts: 2,
            evidence: evidence.clone(),
            verification_contract: Some(crate::agent_graph::EvidenceContractV1 {
                version: crate::agent_graph::EVIDENCE_CONTRACT_VERSION,
                plan_id: Some(plan_id),
                configuration_revision: Some(2),
                gates: evidence,
            }),
        }
    };
    let task = |id, title: &str, agent| crate::work::PlanTask {
        id,
        title: title.into(),
        status: crate::work::TaskStatus::Pending,
        owner: None,
        blocked_reason: None,
        created_at: now,
        updated_at: now,
        evidence: Vec::new(),
        agent: Some(agent),
        agent_runtime: crate::agent_graph::AgentTaskRuntime::default(),
        source_task_id: None,
    };
    let mut work = crate::work::WorkState {
        pending_automation_proposal: None,
        plan_feedback: crate::plan_feedback::PlanFeedbackLedger::default(),
        plan: Some(crate::work::PlanArtifact {
            body: None,
            id: plan_id,
            revision: 3,
            status: crate::work::PlanStatus::Approved,
            title: "Release graph".into(),
            created_at: now,
            updated_at: now,
            approved_at: Some(now),
            approved_by: Some("user".into()),
            tasks: vec![
                task(first, "Inspect provider boundary", definition(Vec::new())),
                task(second, "Verify isolated changes", definition(vec![first])),
            ],
            decisions: Vec::new(),
            source_plan_id: None,
        }),
        events: Vec::new(),
        agent_graph: crate::agent_graph::AgentGraphState {
            status: crate::agent_graph::AgentGraphStatus::Running,
            updated_at: Some(now),
            notice: None,
            plan_id: Some(plan_id),
            configuration_revision: Some(2),
            approved_revision: Some(3),
            budget_limits: Some(
                crate::agent_graph::AgentGraphBudgetLimitsV1::from_input(
                    crate::agent_graph::AgentGraphBudgetInput {
                        max_cost_usd: None,
                        allow_unpriced: true,
                        max_physical_requests: crate::agent_graph::DEFAULT_GRAPH_PHYSICAL_REQUESTS,
                    },
                )
                .unwrap(),
            ),
            budget_usage: crate::agent_graph::AgentGraphBudgetUsageV1::default(),
        },
        historical_notice: None,
    };
    crate::agent_graph::synchronize(
        &mut work.plan.as_mut().unwrap().tasks,
        &mut work.agent_graph,
    );
    let mut state = test_state();
    *state.work_state.lock().unwrap() = work;
    state.task_graph_manager = Some(crate::app::TaskGraphManagerState {
        filter: String::new(),
        selected_idx: 0,
        confirmation: None,
        error: None,
        notice: None,
    });
    state
}

#[test]
fn task_graph_empty_ready_and_busy_snapshots() {
    let mut empty = test_state();
    empty.task_graph_manager = Some(crate::app::TaskGraphManagerState {
        filter: String::new(),
        selected_idx: 0,
        confirmation: None,
        error: None,
        notice: None,
    });
    insta::assert_snapshot!(
        "task_graph_empty",
        wide_narrow(
            render_to_text(&empty, 100, 30),
            render_to_text(&empty, 60, 24)
        )
    );

    let ready = task_graph_render_state();
    insta::assert_snapshot!(
        "task_graph_ready",
        wide_narrow(
            render_to_text(&ready, 100, 30),
            render_to_text(&ready, 60, 24)
        )
    );

    let mut busy = task_graph_render_state();
    busy.agent_graph_busy = true;
    insta::assert_snapshot!(
        "task_graph_busy",
        wide_narrow(
            render_to_text(&busy, 100, 30),
            render_to_text(&busy, 60, 24)
        )
    );
}

#[test]
fn task_graph_running_failure_retry_and_complete_snapshots() {
    let now = chrono::DateTime::parse_from_rfc3339("2026-08-11T12:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    let agent_id = uuid::Uuid::from_u128(20);
    let running = task_graph_render_state();
    {
        let mut work = running.work_state.lock().unwrap();
        let task = &mut work.plan.as_mut().unwrap().tasks[0];
        task.status = crate::work::TaskStatus::InProgress;
        task.agent_runtime.state = crate::agent_graph::AgentTaskState::Running;
        task.agent_runtime
            .attempts
            .push(crate::agent_graph::AgentTaskAttempt {
                number: 1,
                agent_id: Some(agent_id),
                status: crate::agent_graph::AgentAttemptStatus::WaitingApproval,
                started_at: now,
                finished_at: None,
                successful_tools: Vec::new(),
                summary: None,
                terminal_event_sequence: None,
                verification_receipt: None,
                workspace_snapshot: None,
                host_verification: None,
                budget_usage: crate::agent_graph::AgentGraphBudgetUsageV1::default(),
            });
    }
    insta::assert_snapshot!(
        "task_graph_running",
        wide_narrow(
            render_to_text(&running, 100, 30),
            render_to_text(&running, 60, 24)
        )
    );

    let mut failed = running;
    let first = {
        let mut work = failed.work_state.lock().unwrap();
        let plan = work.plan.as_mut().unwrap();
        let first = plan.tasks[0].id;
        plan.tasks[0].status = crate::work::TaskStatus::Blocked;
        plan.tasks[0].blocked_reason = Some("evidence gate was not satisfied".into());
        plan.tasks[0].agent_runtime.state = crate::agent_graph::AgentTaskState::Failed;
        plan.tasks[0].agent_runtime.failure = Some("evidence gate was not satisfied".into());
        plan.tasks[0].agent_runtime.attempts[0].status =
            crate::agent_graph::AgentAttemptStatus::EvidenceFailed;
        plan.tasks[0].agent_runtime.attempts[0].finished_at = Some(now);
        plan.tasks[1].agent_runtime.state = crate::agent_graph::AgentTaskState::DependencyBlocked;
        work.agent_graph.status = crate::agent_graph::AgentGraphStatus::Paused;
        first
    };
    failed.task_graph_manager.as_mut().unwrap().confirmation =
        Some(crate::app::TaskGraphConfirmation {
            action: crate::app::TaskGraphConfirmationAction::Retry(first),
            confirm_selected: false,
        });
    insta::assert_snapshot!(
        "task_graph_retry",
        wide_narrow(
            render_to_text(&failed, 100, 30),
            render_to_text(&failed, 60, 24)
        )
    );

    let complete = task_graph_render_state();
    {
        let mut work = complete.work_state.lock().unwrap();
        work.agent_graph.status = crate::agent_graph::AgentGraphStatus::Completed;
        for task in &mut work.plan.as_mut().unwrap().tasks {
            task.status = crate::work::TaskStatus::Completed;
            task.agent_runtime.state = crate::agent_graph::AgentTaskState::Completed;
        }
    }
    insta::assert_snapshot!(
        "task_graph_complete",
        wide_narrow(
            render_to_text(&complete, 100, 30),
            render_to_text(&complete, 60, 24)
        )
    );
}

#[test]
fn task_graph_labels_a_stale_pass_receipt_as_invalid() {
    let state = task_graph_render_state();
    {
        let mut work = state.work_state.lock().unwrap();
        let binding = work.agent_graph.verification_binding().unwrap();
        let task = &mut work.plan.as_mut().unwrap().tasks[0];
        let agent_id = uuid::Uuid::from_u128(21);
        let gate = crate::agent_graph::AgentEvidenceGate::ToolSuccess {
            tool: "read_file".into(),
        };
        let definition = task.agent.as_mut().unwrap();
        definition.evidence = vec![gate.clone()];
        definition.verification_contract = Some(crate::agent_graph::EvidenceContractV1 {
            version: crate::agent_graph::EVIDENCE_CONTRACT_VERSION,
            plan_id: Some(binding.plan_id),
            configuration_revision: Some(binding.configuration_revision),
            gates: vec![gate],
        });
        let host_snapshot = crate::agent_graph::AgentWorkspaceSnapshotV1 {
            workspace_identity_sha256: "1".repeat(64),
            source_revision_sha256: "2".repeat(64),
            artifact_observations: Vec::new(),
        };
        let host_verification = crate::agent_graph::AgentHostVerificationV1 {
            started: host_snapshot.clone(),
            finished: host_snapshot.clone(),
            observations: Vec::new(),
        };
        let receipt = crate::agent_graph::verify_agent_attempt_with_host(
            Some(binding),
            task.id,
            task.agent.as_ref().unwrap(),
            1,
            agent_id,
            crate::agent_graph::AgentAttemptStatus::Completed,
            Some(25),
            Some("done"),
            &["read_file".into()],
            Some(&host_verification),
        );
        assert_eq!(
            receipt.disposition,
            crate::agent_graph::AgentVerificationDisposition::Pass
        );
        task.status = crate::work::TaskStatus::Completed;
        task.agent_runtime.state = crate::agent_graph::AgentTaskState::Completed;
        task.agent_runtime
            .attempts
            .push(crate::agent_graph::AgentTaskAttempt {
                number: 1,
                agent_id: Some(agent_id),
                status: crate::agent_graph::AgentAttemptStatus::Completed,
                started_at: chrono::Utc::now(),
                finished_at: Some(chrono::Utc::now()),
                successful_tools: vec!["read_file".into()],
                summary: Some("done".into()),
                terminal_event_sequence: Some(25),
                verification_receipt: Some(receipt),
                workspace_snapshot: Some(host_snapshot),
                host_verification: Some(host_verification),
                budget_usage: crate::agent_graph::AgentGraphBudgetUsageV1::default(),
            });
        work.agent_graph.approved_revision = Some(binding.approved_revision + 1);
    }

    let rendered = render_to_text(&state, 100, 30);
    assert!(rendered.contains("Verification: stale/invalid receipt"));
    assert!(!rendered.contains("Verification: Pass"));
}

fn worktree_record(label: &str, id: uuid::Uuid) -> crate::worktree::WorktreeRecord {
    crate::worktree::WorktreeRecord {
        id,
        label: label.into(),
        repository_root: "/workspace/ca".into(),
        path: format!("/profile/worktrees/0123456789ab/{id}/{label}").into(),
        branch: format!("ca/{label}-{}", &id.simple().to_string()[..8]),
        base_commit: "0123456789abcdef0123456789abcdef01234567".into(),
        git_common_dir: "/workspace/ca/.git".into(),
        created_at: chrono::DateTime::parse_from_rfc3339("2026-08-11T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc),
    }
}

fn worktree_status(label: &str, id: uuid::Uuid, dirty: bool) -> crate::worktree::WorktreeStatus {
    crate::worktree::WorktreeStatus {
        record: worktree_record(label, id),
        health: crate::worktree::WorktreeHealth::Ready,
        head: Some("89abcdef0123456789abcdef0123456789abcdef".into()),
        branch: Some(format!("ca/{label}-{}", &id.simple().to_string()[..8])),
        dirty,
        changed_entries: usize::from(dirty) * 2,
        ahead: Some(1),
        behind: Some(0),
    }
}

#[test]
fn worktree_list_status_diff_and_attached_title_snapshots() {
    let review_id = uuid::Uuid::parse_str("11111111-1111-4111-8111-111111111111").unwrap();
    let verify_id = uuid::Uuid::parse_str("22222222-2222-4222-8222-222222222222").unwrap();
    let review = worktree_status("review", review_id, false);
    let verify = worktree_status("verify", verify_id, true);

    let mut list = test_state();
    list.attached_worktree = Some(review.record.clone());
    list.pager_content = crate::worktree::render_list(&[review.clone(), verify], Some(review_id));
    list.pager_open = true;
    insta::assert_snapshot!(
        "worktree_list",
        wide_narrow(
            render_to_text(&list, 100, 30),
            render_to_text(&list, 60, 24)
        )
    );

    let mut status = test_state();
    status.attached_worktree = Some(review.record.clone());
    status.pager_content = crate::worktree::render_status(&review, true);
    status.pager_open = true;
    insta::assert_snapshot!(
        "worktree_status",
        wide_narrow(
            render_to_text(&status, 100, 30),
            render_to_text(&status, 60, 24)
        )
    );

    let mut diff = test_state();
    diff.attached_worktree = Some(review.record);
    diff.pager_content = "Worktree diff · review · ca/review-11111111\n\nUnstaged changes\ndiff --git a/src/lib.rs b/src/lib.rs\n@@ -1 +1 @@\n-old\n+new\n\nUntracked files (names only)\nnotes.txt".into();
    diff.pager_open = true;
    insta::assert_snapshot!(
        "worktree_diff",
        wide_narrow(
            render_to_text(&diff, 100, 30),
            render_to_text(&diff, 60, 24)
        )
    );

    let mut attached_title = test_state();
    attached_title.attached_worktree = Some(worktree_record("review", review_id));
    insta::assert_snapshot!(
        "worktree_attached_title",
        wide_narrow(
            render_to_text(&attached_title, 100, 30),
            render_to_text(&attached_title, 60, 24)
        )
    );
}

#[test]
fn worktree_cleanup_confirmation_busy_and_failure_snapshots() {
    let id = uuid::Uuid::parse_str("11111111-1111-4111-8111-111111111111").unwrap();
    for (name, busy, error) in [
        ("worktree_cleanup", false, None),
        ("worktree_cleanup_busy", true, None),
        (
            "worktree_cleanup_failure",
            false,
            Some("Git refused cleanup because the worktree changed".to_string()),
        ),
    ] {
        let mut state = test_state();
        state.worktree_cleanup_confirmation = Some(crate::worktree::WorktreeCleanupConfirmation {
            id,
            label: "review".into(),
            branch: "ca/review-11111111".into(),
            remove_selected: busy,
            busy,
            error,
        });
        insta::assert_snapshot!(
            format!("{name}"),
            wide_narrow(
                render_to_text(&state, 100, 30),
                render_to_text(&state, 60, 24)
            )
        );
    }
}

#[test]
fn worktree_setup_preview_wide_narrow_busy_and_failure_snapshots() {
    let id = uuid::Uuid::parse_str("33333333-3333-4333-8333-333333333333").unwrap();
    let preview = crate::worktree_setup::SetupPreview {
        source_kind: crate::worktree_setup::SetupSourceKind::Native,
        source_path: "/workspace/ca/.ca/worktrees.toml".into(),
        source_sha256: "0123456789abcdef".repeat(4),
        repository_root: "/workspace/ca".into(),
        label: "feature-review".into(),
        base_ref: "main".into(),
        base_commit: "89abcdef0123456789abcdef0123456789abcdef".into(),
        steps: vec![
            crate::worktree_setup::SetupStep {
                name: "fetch dependencies".into(),
                program: "cargo".into(),
                args: vec!["fetch".into(), "--locked".into()],
                cwd: ".".into(),
                sandbox: crate::sandbox::SandboxMode::Required,
                network: false,
            },
            crate::worktree_setup::SetupStep {
                name: "prepare fixtures".into(),
                program: "just".into(),
                args: vec!["fixtures".into()],
                cwd: "tests".into(),
                sandbox: crate::sandbox::SandboxMode::PolicyOnly,
                network: true,
            },
        ],
        digest: "fedcba9876543210".repeat(4),
    };
    for (name, run_selected, busy, error) in [
        ("worktree_setup", false, false, None),
        ("worktree_setup_busy", true, true, None),
        (
            "worktree_setup_failure",
            true,
            false,
            Some("configuration changed after preview".to_string()),
        ),
    ] {
        let mut state = test_state();
        let mut confirmation = crate::worktree_setup::SetupConfirmation::new(
            worktree_record("feature-review", id),
            preview.clone(),
        );
        confirmation.run_selected = run_selected;
        confirmation.busy = busy;
        confirmation.error = error;
        state.worktree_setup_confirmation = Some(confirmation);
        insta::assert_snapshot!(
            format!("{name}"),
            wide_narrow(
                render_to_text(&state, 100, 30),
                render_to_text(&state, 60, 24)
            )
        );
    }
}

fn multi_run_render_state() -> AppState {
    let current = ModelInfo {
        provider: "mimo".into(),
        id: "mimo-v2.5-pro".into(),
        display_name: "mimo-v2.5-pro".into(),
    };
    let other = ModelInfo {
        provider: "deepseek".into(),
        id: "deepseek-chat".into(),
        display_name: "deepseek-chat".into(),
    };
    let definition = crate::multi_run::MultiRunDefinition::build(
        "Implement the parser fix and prove the behavior",
        2,
        Some(&current),
        &[current.clone(), other],
    )
    .unwrap();
    let run = crate::multi_run::MultiRunState::preview(
        definition,
        "User: fix the parser\nAssistant: I will inspect it".into(),
    );
    let mut state = test_state();
    state.current_model = Some(current);
    state.multi_run_manager = Some(crate::app::MultiRunManagerState {
        draft_request: "Implement the parser fix and prove the behavior".into(),
        context_snapshot: "User: fix the parser\nAssistant: I will inspect it".into(),
        pricing: std::collections::HashMap::new(),
        cursor: 47,
        candidate_count: 2,
        editing: false,
        run: Some(run),
        confirmation: None,
        busy: false,
        error: None,
        notice: Some("Review routes, then press Shift+S".into()),
    });
    state
}

#[test]
fn multi_run_setup_preview_and_start_confirmation_snapshots() {
    let mut setup = multi_run_render_state();
    let manager = setup.multi_run_manager.as_mut().unwrap();
    manager.editing = true;
    manager.run = None;
    manager.draft_request.clear();
    manager.cursor = 0;
    manager.notice = None;
    insta::assert_snapshot!(
        "multi_run_setup",
        wide_narrow(
            render_to_text(&setup, 100, 30),
            render_to_text(&setup, 60, 24)
        )
    );

    let mut preview = multi_run_render_state();
    preview.multi_run_manager.as_mut().unwrap().confirmation =
        Some(crate::app::MultiRunConfirmation {
            action: crate::app::MultiRunConfirmationAction::Start,
            confirm_selected: false,
        });
    insta::assert_snapshot!(
        "multi_run_start_confirmation",
        wide_narrow(
            render_to_text(&preview, 100, 30),
            render_to_text(&preview, 60, 24)
        )
    );
}

#[test]
fn multi_run_running_failure_cancelled_and_ready_snapshots() {
    let mut running = multi_run_render_state();
    {
        let run = running
            .multi_run_manager
            .as_mut()
            .unwrap()
            .run
            .as_mut()
            .unwrap();
        run.status = crate::multi_run::MultiRunStatus::Running;
        run.candidates[0].status = crate::multi_run::CandidateStatus::Running;
        run.candidates[0].worktree_id =
            Some(uuid::Uuid::parse_str("11111111-1111-4111-8111-111111111111").unwrap());
        run.candidates[0].estimated_cost = "~$0.004".into();
        run.candidates[1].status = crate::multi_run::CandidateStatus::WaitingApproval;
        run.candidates[1].worktree_id =
            Some(uuid::Uuid::parse_str("22222222-2222-4222-8222-222222222222").unwrap());
        run.candidates[1].estimated_cost = "unknown".into();
    }
    running.multi_run_manager.as_mut().unwrap().notice =
        Some("Candidates are running in isolated worktrees".into());
    insta::assert_snapshot!(
        "multi_run_running",
        wide_narrow(
            render_to_text(&running, 100, 30),
            render_to_text(&running, 60, 24)
        )
    );

    let mut failed = multi_run_render_state();
    let failed_manager = failed.multi_run_manager.as_mut().unwrap();
    failed_manager.run.as_mut().unwrap().status = crate::multi_run::MultiRunStatus::Failed;
    failed_manager.error =
        Some("Candidate worktree identity changed; no provider request sent".into());
    failed_manager.notice = None;
    insta::assert_snapshot!(
        "multi_run_failure",
        wide_narrow(
            render_to_text(&failed, 100, 30),
            render_to_text(&failed, 60, 24)
        )
    );

    let mut cancelled = multi_run_render_state();
    let cancelled_manager = cancelled.multi_run_manager.as_mut().unwrap();
    cancelled_manager.run.as_mut().unwrap().status = crate::multi_run::MultiRunStatus::Cancelled;
    for candidate in &mut cancelled_manager.run.as_mut().unwrap().candidates {
        candidate.status = crate::multi_run::CandidateStatus::Cancelled;
    }
    cancelled_manager.notice = Some("Multi-run cancelled; worktrees preserved".into());
    insta::assert_snapshot!(
        "multi_run_cancelled",
        wide_narrow(
            render_to_text(&cancelled, 100, 30),
            render_to_text(&cancelled, 60, 24)
        )
    );

    let mut ready = multi_run_render_state();
    let ready_manager = ready.multi_run_manager.as_mut().unwrap();
    let run = ready_manager.run.as_mut().unwrap();
    run.status = crate::multi_run::MultiRunStatus::ReadyToJudge;
    for (index, candidate) in run.candidates.iter_mut().enumerate() {
        candidate.status = crate::multi_run::CandidateStatus::Completed;
        candidate.worktree_id = Some(if index == 0 {
            uuid::Uuid::parse_str("11111111-1111-4111-8111-111111111111").unwrap()
        } else {
            uuid::Uuid::parse_str("22222222-2222-4222-8222-222222222222").unwrap()
        });
        candidate.summary = Some(format!("Candidate {} completed", index + 1));
        candidate.successful_tools = vec!["read_file".into(), "apply_patch".into()];
        candidate.evidence = Some(crate::multi_run::CandidateEvidence::from_bounded_diff(
            &format!("diff {index}"),
            index + 1,
            0,
        ));
    }
    ready_manager.notice = Some("Candidate evidence is ready; press Shift+J to judge".into());
    insta::assert_snapshot!(
        "multi_run_ready",
        wide_narrow(
            render_to_text(&ready, 100, 30),
            render_to_text(&ready, 60, 24)
        )
    );
}

#[test]
fn multi_run_judged_and_fusion_snapshots() {
    let mut state = multi_run_render_state();
    let manager = state.multi_run_manager.as_mut().unwrap();
    let run = manager.run.as_mut().unwrap();
    for (index, candidate) in run.candidates.iter_mut().enumerate() {
        candidate.status = crate::multi_run::CandidateStatus::Completed;
        candidate.worktree_id = Some(if index == 0 {
            uuid::Uuid::parse_str("11111111-1111-4111-8111-111111111111").unwrap()
        } else {
            uuid::Uuid::parse_str("22222222-2222-4222-8222-222222222222").unwrap()
        });
        candidate.evidence = Some(crate::multi_run::CandidateEvidence::from_bounded_diff(
            &format!("candidate {index} diff"),
            index + 1,
            0,
        ));
    }
    run.apply_judgment(crate::multi_run::JudgmentReport {
        winner: 2,
        rationale: "Candidate 2 has the stronger focused diff and test evidence".into(),
    })
    .unwrap();
    manager.notice = Some("Multi-run judgment ready".into());
    insta::assert_snapshot!(
        "multi_run_judged",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );

    state
        .multi_run_manager
        .as_mut()
        .unwrap()
        .run
        .as_mut()
        .unwrap()
        .apply_fusion(crate::multi_run::FusionReport {
            source: 2,
            plan: "Use candidate 2 as the proposed source; inspect its branch, then integrate explicitly."
                .into(),
        })
        .unwrap();
    state.multi_run_manager.as_mut().unwrap().notice = Some("Fusion proposal ready".into());
    insta::assert_snapshot!(
        "multi_run_fusion",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

fn theme_render_state(writes: bool) -> AppState {
    let mut state = test_state();
    state.run_capabilities.config_writes = writes;
    state.theme_manager = Some(crate::app::ThemeManagerState {
        saved_theme: crate::ui_settings::UiTheme::Default,
        selected_idx: 2,
        filter: String::new(),
        busy: false,
        notice: Some("Highlighting Amber previews the palette".into()),
        error: None,
    });
    state
}

#[test]
fn theme_preview_and_bare_snapshots() {
    let state = theme_render_state(true);
    insta::assert_snapshot!(
        "theme_preview",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );

    let mut bare = theme_render_state(false);
    bare.active_profile = crate::runtime::ActiveProfile::Bare;
    bare.theme_manager.as_mut().unwrap().error = Some(
        "Theme preview is available in bare mode, but persistent changes require rerunning without --profile bare".into(),
    );
    insta::assert_snapshot!(
        "theme_bare",
        wide_narrow(
            render_to_text(&bare, 100, 30),
            render_to_text(&bare, 60, 24)
        )
    );
}

#[test]
fn theme_no_match_busy_and_failure_snapshots() {
    let mut no_match = theme_render_state(true);
    no_match.theme_manager.as_mut().unwrap().filter = "violet".into();
    no_match.theme_manager.as_mut().unwrap().selected_idx = 0;
    no_match.theme_manager.as_mut().unwrap().notice = None;
    insta::assert_snapshot!(
        "theme_no_match",
        wide_narrow(
            render_to_text(&no_match, 100, 30),
            render_to_text(&no_match, 60, 24)
        )
    );

    let mut busy = theme_render_state(true);
    busy.theme_manager.as_mut().unwrap().busy = true;
    busy.theme_manager.as_mut().unwrap().notice = None;
    insta::assert_snapshot!(
        "theme_busy",
        wide_narrow(
            render_to_text(&busy, 100, 30),
            render_to_text(&busy, 60, 24)
        )
    );

    let mut failure = theme_render_state(true);
    failure.theme_manager.as_mut().unwrap().notice = None;
    failure.theme_manager.as_mut().unwrap().error =
        Some("Could not save theme: read-only profile file".into());
    insta::assert_snapshot!(
        "theme_failure",
        wide_narrow(
            render_to_text(&failure, 100, 30),
            render_to_text(&failure, 60, 24)
        )
    );
}

fn copy_picker_render_state(parts: bool) -> AppState {
    let mut state = test_state();
    state.copy_picker = Some(crate::app::CopyPickerState {
        messages: vec![
            crate::app::CopyPickerMessage {
                role: Role::User,
                summary: "User · Review the provider contract before editing".into(),
                parts: vec![crate::app::CopyPickerPart {
                    label: "Content".into(),
                    value: "Review the provider contract before editing.".into(),
                }],
            },
            crate::app::CopyPickerMessage {
                role: Role::Assistant,
                summary: "Assistant · I found the failing cache-usage branch".into(),
                parts: vec![
                    crate::app::CopyPickerPart {
                        label: "Content".into(),
                        value: "I found the failing cache-usage branch.".into(),
                    },
                    crate::app::CopyPickerPart {
                        label: "Visible reasoning".into(),
                        value: "The hit/miss pair is authoritative.".into(),
                    },
                    crate::app::CopyPickerPart {
                        label: "Tool result · run_command".into(),
                        value: "focused tests passed".into(),
                    },
                ],
            },
        ],
        level: if parts {
            crate::app::CopyPickerLevel::Parts { message_index: 1 }
        } else {
            crate::app::CopyPickerLevel::Messages
        },
        filter: String::new(),
        selected_idx: 1,
    });
    state
}

#[test]
fn copy_message_and_part_picker_snapshots() {
    let messages = copy_picker_render_state(false);
    insta::assert_snapshot!(
        "copy_picker_messages",
        wide_narrow(
            render_to_text(&messages, 100, 30),
            render_to_text(&messages, 60, 24)
        )
    );

    let parts = copy_picker_render_state(true);
    insta::assert_snapshot!(
        "copy_picker_parts",
        wide_narrow(
            render_to_text(&parts, 100, 30),
            render_to_text(&parts, 60, 24)
        )
    );
}

#[test]
fn mermaid_terminal_diagram_snapshots_are_readable_wide_and_narrow() {
    let source = "flowchart TD\nA[One] --> B[Two]\n";
    assert!(matches!(
        crate::mermaid::wait_for_render(source, 94),
        crate::mermaid::MermaidRender::Ready(_)
    ));
    assert!(matches!(
        crate::mermaid::wait_for_render(source, 54),
        crate::mermaid::MermaidRender::Ready(_)
    ));
    let mut state = test_state();
    state.messages.push(Message {
        role: Role::Assistant,
        content: format!("```mermaid\n{source}```"),
        tool_calls: None,
        thinking: None,
        response_items: Vec::new(),
        timestamp: chrono::Utc::now(),
    });
    insta::assert_snapshot!(
        "mermaid_diagram",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

fn statusline_render_state() -> AppState {
    let mut state = test_state();
    state.current_model = Some(ModelInfo {
        id: "mimo-v2.5-pro".into(),
        provider: "mimo".into(),
        display_name: "mimo-v2.5-pro".into(),
    });
    state.statusline_fields = vec![
        crate::ui_settings::StatusField::Model,
        crate::ui_settings::StatusField::Messages,
        crate::ui_settings::StatusField::Custom,
        crate::ui_settings::StatusField::CommandsHint,
    ];
    state.status_command.output = Some("tests green".into());
    state.statusline_manager = Some(crate::app::StatuslineManagerState {
        selected_idx: crate::ui_settings::StatusField::ALL
            .iter()
            .position(|field| *field == crate::ui_settings::StatusField::Custom)
            .unwrap(),
        fields: state.statusline_fields.clone(),
        custom_configured: true,
        custom_enabled: true,
        refresh_secs: 5,
        busy: false,
        testing: false,
        notice: Some("Custom status succeeded".into()),
        error: None,
    });
    state
}

#[test]
fn statusline_fields_testing_and_failure_snapshots() {
    let state = statusline_render_state();
    let rendered = render_to_text(&state, 100, 30);
    assert_eq!(rendered.matches("mimo-v2.5-pro").count(), 1);
    insta::assert_snapshot!(
        "statusline_fields",
        wide_narrow(rendered, render_to_text(&state, 60, 24))
    );

    let mut testing = statusline_render_state();
    testing.status_command.running = true;
    testing.statusline_manager.as_mut().unwrap().testing = true;
    testing.statusline_manager.as_mut().unwrap().notice = None;
    insta::assert_snapshot!(
        "statusline_testing",
        wide_narrow(
            render_to_text(&testing, 100, 30),
            render_to_text(&testing, 60, 24)
        )
    );

    let mut failure = statusline_render_state();
    failure.status_command.output = None;
    failure.status_command.error = Some("custom status command timed out".into());
    failure.statusline_manager.as_mut().unwrap().notice = None;
    failure.statusline_manager.as_mut().unwrap().error =
        Some("custom status command timed out".into());
    insta::assert_snapshot!(
        "statusline_failure",
        wide_narrow(
            render_to_text(&failure, 100, 30),
            render_to_text(&failure, 60, 24)
        )
    );
}

fn usage_statusline_state(cost: crate::usage::SittingCostPresentation) -> AppState {
    let mut state = test_state();
    state.statusline_fields = vec![
        crate::ui_settings::StatusField::GenerationSpeed,
        crate::ui_settings::StatusField::SittingCost,
    ];
    state.latest_generation_speed_tenths = Some(125);
    state.sitting_cost = cost;
    state
}

#[test]
fn usage_statusline_cost_state_snapshots() {
    for (name, cost) in [
        (
            "statusline_usage_unknown",
            crate::usage::SittingCostPresentation::Unknown,
        ),
        (
            "statusline_usage_incremental",
            crate::usage::SittingCostPresentation::Incremental {
                usd: "0.0042".into(),
            },
        ),
        (
            "statusline_usage_allocation",
            crate::usage::SittingCostPresentation::SubscriptionAllocation {
                credits: "0.75".into(),
            },
        ),
        (
            "statusline_usage_mixed",
            crate::usage::SittingCostPresentation::Mixed {
                incremental_usd: Some("0.0042".into()),
                allocation_credits: Some("0.75".into()),
                unknown_requests: 1,
            },
        ),
    ] {
        let mut state = usage_statusline_state(cost);
        if name == "statusline_usage_unknown" {
            state.latest_generation_speed_tenths = None;
        }
        insta::assert_snapshot!(
            format!("{name}"),
            wide_narrow(
                render_to_text(&state, 100, 30),
                render_to_text(&state, 60, 24)
            )
        );
    }
}

#[test]
fn statusline_unconfigured_busy_and_bare_snapshots() {
    let mut unconfigured = statusline_render_state();
    let manager = unconfigured.statusline_manager.as_mut().unwrap();
    manager.custom_configured = false;
    manager.custom_enabled = false;
    manager.notice = None;
    unconfigured.status_command.output = None;
    insta::assert_snapshot!(
        "statusline_unconfigured",
        wide_narrow(
            render_to_text(&unconfigured, 100, 30),
            render_to_text(&unconfigured, 60, 24)
        )
    );

    let mut busy = statusline_render_state();
    busy.statusline_manager.as_mut().unwrap().busy = true;
    busy.statusline_manager.as_mut().unwrap().notice = None;
    insta::assert_snapshot!(
        "statusline_busy",
        wide_narrow(
            render_to_text(&busy, 100, 30),
            render_to_text(&busy, 60, 24)
        )
    );

    let mut bare = statusline_render_state();
    bare.active_profile = crate::runtime::ActiveProfile::Bare;
    bare.run_capabilities = crate::runtime::RunCapabilities::for_profile(&bare.active_profile);
    bare.statusline_manager.as_mut().unwrap().custom_enabled = false;
    bare.statusline_manager.as_mut().unwrap().notice = None;
    bare.status_command.output = None;
    insta::assert_snapshot!(
        "statusline_bare",
        wide_narrow(
            render_to_text(&bare, 100, 30),
            render_to_text(&bare, 60, 24)
        )
    );
}

fn notification_picker_render_state() -> AppState {
    let mut state = test_state();
    state.notification_picker = Some(crate::app::NotificationPickerState {
        saved: crate::ui_settings::NotificationStyle::Off,
        selected_idx: 2,
        busy: false,
        error: None,
    });
    state
}

#[test]
fn notification_picker_wide_narrow_busy_and_bare_snapshots() {
    let state = notification_picker_render_state();
    insta::assert_snapshot!(
        "notification_picker",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );

    let mut busy = notification_picker_render_state();
    busy.notification_picker.as_mut().unwrap().busy = true;
    insta::assert_snapshot!("notification_picker_busy", render_to_text(&busy, 80, 24));

    let mut bare = notification_picker_render_state();
    bare.active_profile = crate::runtime::ActiveProfile::Bare;
    bare.run_capabilities = crate::runtime::RunCapabilities::for_profile(&bare.active_profile);
    bare.notification_picker.as_mut().unwrap().error = Some(
        "Notification settings cannot be saved in bare mode; rerun without --profile bare".into(),
    );
    insta::assert_snapshot!("notification_picker_bare", render_to_text(&bare, 80, 24));
}

#[test]
fn command_output_picker_and_confirmation_snapshots() {
    let mut picker = test_state();
    picker.command_output_picker = Some(crate::app::CommandOutputPickerState {
        saved: crate::config::CommandOutputPolicy::Bounded,
        selected_idx: 1,
        busy: false,
        error: None,
    });
    insta::assert_snapshot!(
        "command_output_picker",
        wide_narrow(
            render_to_text(&picker, 100, 30),
            render_to_text(&picker, 60, 24)
        )
    );

    let mut confirmation = test_state();
    confirmation.output_full_confirmation = Some(crate::app::OutputFullConfirmation {
        rerun: crate::command_output::FullOutputRerun {
            command: "cargo test --all-targets".into(),
            cwd: "/workspace/demo".into(),
            network_requested: false,
        },
        run_selected: false,
        busy: false,
        error: None,
    });
    insta::assert_snapshot!(
        "command_output_full_confirmation",
        wide_narrow(
            render_to_text(&confirmation, 100, 30),
            render_to_text(&confirmation, 60, 24)
        )
    );
}

#[test]
fn semantic_command_collapsed_and_raw_expansion_snapshots() {
    let mut state = test_state();
    let id = "command-semantic";
    state.messages = vec![assistant_calls(vec![ToolCall {
        id: id.into(),
        name: "run_command".into(),
        arguments: r#"{"command":"git status --short"}"#.into(),
        status: ToolStatus::Done,
        output: Some("exit code: 0\nstdout:\n M src/main.rs\n?? notes.txt".into()),
    }])];
    let observation = crate::command_output::CommandObservation {
        command: "git status --short".into(),
        cwd: "/workspace/demo".into(),
        network_requested: false,
        status: crate::jobs::JobStatus::Completed { exit_code: 0 },
        stdout: " M src/main.rs\n?? notes.txt\n".into(),
        stderr: String::new(),
        stdout_omitted_bytes: 0,
        stderr_omitted_bytes: 0,
        bounded_model_text: "exit code: 0\nstdout:\n M src/main.rs\n?? notes.txt".into(),
    };
    state.command_presentations.insert(
        id.into(),
        crate::command_output::summarize(&observation).unwrap(),
    );
    insta::assert_snapshot!(
        "semantic_command_collapsed",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
    state
        .expanded_blocks
        .push(crate::app::TranscriptBlockKey::tool_calls(id).unwrap());
    insta::assert_snapshot!(
        "semantic_command_raw",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );
}

#[test]
fn scratch_empty_and_delete_confirmation_snapshots() {
    let mut empty = test_state();
    empty.pager_open = true;
    empty.pager_content = "Project scratch\n(empty)\n".into();
    insta::assert_snapshot!(
        "scratch_empty",
        wide_narrow(
            render_to_text(&empty, 100, 30),
            render_to_text(&empty, 60, 24)
        )
    );

    let mut confirmation = test_state();
    confirmation.scratch_delete_confirmation = Some(crate::app::ScratchDeleteConfirmation {
        id: uuid::Uuid::nil(),
        delete_selected: false,
        error: None,
    });
    insta::assert_snapshot!(
        "scratch_delete",
        wide_narrow(
            render_to_text(&confirmation, 100, 30),
            render_to_text(&confirmation, 60, 24)
        )
    );
}

fn route_preset_picker_render_state() -> AppState {
    let mut state = test_state();
    state.route_preset_picker = Some(crate::app::RoutePresetPickerState {
        rows: vec![
            crate::app::RoutePresetRow {
                name: "daily".into(),
                preset: crate::config::RoutePreset {
                    provider: "local".into(),
                    model: "coder".into(),
                    reasoning_effort: Some(crate::config::ReasoningEffort::High),
                },
            },
            crate::app::RoutePresetRow {
                name: "quick_review".into(),
                preset: crate::config::RoutePreset {
                    provider: "backup".into(),
                    model: "reviewer".into(),
                    reasoning_effort: None,
                },
            },
        ],
        selected_idx: 1,
        error: None,
    });
    state
}

#[test]
fn route_preset_picker_and_overwrite_snapshots() {
    let state = route_preset_picker_render_state();
    insta::assert_snapshot!(
        "route_preset_picker",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );

    let mut empty = test_state();
    empty.route_preset_picker = Some(crate::app::RoutePresetPickerState::default());
    insta::assert_snapshot!("route_preset_picker_empty", render_to_text(&empty, 80, 24));
    empty.route_preset_picker.as_mut().unwrap().error =
        Some("No route presets are configured; use /preset-save <name>".into());
    insta::assert_snapshot!("route_preset_picker_error", render_to_text(&empty, 80, 24));

    let preset = crate::config::RoutePreset {
        provider: "local".into(),
        model: "coder".into(),
        reasoning_effort: Some(crate::config::ReasoningEffort::High),
    };
    let mut overwrite = test_state();
    overwrite.route_preset_overwrite = Some(crate::app::RoutePresetOverwriteConfirmation {
        name: "daily".into(),
        preset,
        save_selected: false,
        busy: false,
        error: None,
    });
    insta::assert_snapshot!(
        "route_preset_overwrite",
        wide_narrow(
            render_to_text(&overwrite, 100, 30),
            render_to_text(&overwrite, 60, 24)
        )
    );
    overwrite.route_preset_overwrite.as_mut().unwrap().busy = true;
    insta::assert_snapshot!(
        "route_preset_overwrite_busy",
        render_to_text(&overwrite, 80, 24)
    );
    let confirmation = overwrite.route_preset_overwrite.as_mut().unwrap();
    confirmation.busy = false;
    confirmation.error = Some("Could not save route preset: injected failure".into());
    insta::assert_snapshot!(
        "route_preset_overwrite_error",
        render_to_text(&overwrite, 80, 24)
    );
}

fn session_report_render_state() -> AppState {
    let mut state = test_state();
    state.session_name = "Focused stabilization".into();
    state.status_git_branch = Some("master".into());
    state.current_model = Some(ModelInfo {
        provider: "local".into(),
        id: "model".into(),
        display_name: "model".into(),
    });
    state.messages.push(crate::app::Message {
        role: crate::app::Role::User,
        content: "implement it".into(),
        tool_calls: None,
        thinking: None,
        timestamp: chrono::Utc::now(),
        response_items: Vec::new(),
    });
    let mut session = crate::session::SessionV2::new(
        state.session_name.clone(),
        state.cwd.clone(),
        "local",
        "model",
        state.messages.clone(),
    );
    session.id = uuid::Uuid::from_u128(0xfeed_beef_dead_cafe_0123_4567_89ab_cdef);
    state.current_session = Some(session);
    state
        .session_sitting
        .record_tool(1, "call-1", "read_file", crate::app::ToolStatus::Done);
    state
        .session_sitting
        .record_completion(crate::activity::TurnCompletion {
            outcome: crate::activity::TurnOutcome::Completed,
            duration: std::time::Duration::from_secs(42),
            was_paused: false,
        });
    state.usage_ledger.lock().unwrap().push_test_record(
        crate::usage::UsageKind::MainTurn,
        "local",
        "model",
        std::time::Duration::from_millis(15),
        Some(crate::agent::stream::UsageInfo {
            prompt_tokens: 20,
            completion_tokens: 6,
            total_tokens: 26,
            cached_prompt_tokens: Some(5),
            cache_write_tokens: Some(3),
            reasoning_tokens: Some(2),
            response_metadata: None,
            cache_usage_override: None,
        }),
        crate::usage::UsageOutcome::Completed,
    );
    let config = crate::config::CaConfig::default();
    let native_occupancy = crate::current_context_breakdown(&state, &config).total;
    state.pager_content = crate::session_report(&state, &config).replace(
        &format!("Occupancy: {native_occupancy} / unknown"),
        "Occupancy: 3400 / unknown",
    );
    state.pager_open = true;
    state
}

#[test]
fn session_report_wide_and_narrow_snapshots() {
    let state = session_report_render_state();
    insta::assert_snapshot!(
        "session_report",
        wide_narrow(
            render_to_text(&state, 100, 40),
            render_to_text(&state, 60, 36)
        )
    );
}

#[test]
fn terminal_math_prose_table_and_list_snapshots() {
    let mut state = test_state();
    state.messages = vec![Message {
        role: Role::Assistant,
        content: concat!(
            "The inline result is \\(x_2 + \\alpha \\leq \\infty\\), while $5.00$ remains currency.\n\n",
            "\\[\\sum_{i=1}^n i = \\frac{n}{2}\\]\n\n",
            "- Gradient: $\\nabla f$\n",
            "- Literal code: `\\(x_2\\)`\n\n",
            "| Symbol | Meaning |\n",
            "| --- | --- |\n",
            "| $\\beta$ | coefficient |\n",
            "| $x^2$ | square |"
        )
        .into(),
        tool_calls: None,
        thinking: None,
        timestamp: fixed_time(),
        response_items: Vec::new(),
    }];

    insta::assert_snapshot!(
        "terminal_math",
        wide_narrow(
            render_to_text(&state, 100, 38),
            render_to_text(&state, 60, 34)
        )
    );
}

fn exploration_call(id: &str, name: &str, status: ToolStatus) -> ToolCall {
    ToolCall {
        id: id.into(),
        name: name.into(),
        arguments: format!(r#"{{"path":"src/{id}.rs"}}"#),
        status,
        output: Some(format!("result for {id}\nsecond line")),
    }
}

fn assistant_calls(calls: Vec<ToolCall>) -> Message {
    Message {
        role: Role::Assistant,
        content: String::new(),
        tool_calls: Some(calls),
        thinking: None,
        timestamp: fixed_time(),
        response_items: Vec::new(),
    }
}

#[test]
fn exploration_grouping_crosses_empty_assistant_messages_and_honors_boundaries() {
    let mut state = test_state();
    state.messages = vec![
        assistant_calls(vec![exploration_call(
            "read-a",
            "read_file",
            ToolStatus::Done,
        )]),
        assistant_calls(vec![exploration_call(
            "search-b",
            "search_files",
            ToolStatus::Done,
        )]),
        Message {
            role: Role::Assistant,
            content: "Visible boundary".into(),
            tool_calls: Some(vec![exploration_call("glob-c", "glob", ToolStatus::Done)]),
            thinking: None,
            timestamp: fixed_time(),
            response_items: Vec::new(),
        },
        assistant_calls(vec![
            exploration_call("read-d", "read_pdf", ToolStatus::Done),
            exploration_call("ordinary", "run_command", ToolStatus::Done),
            exploration_call("image-e", "view_image", ToolStatus::Done),
        ]),
        Message {
            role: Role::User,
            content: "Another boundary".into(),
            tool_calls: None,
            thinking: None,
            timestamp: fixed_time(),
            response_items: Vec::new(),
        },
        assistant_calls(vec![exploration_call(
            "lsp-f",
            "lsp_hover",
            ToolStatus::Done,
        )]),
    ];

    let groups = super::render::exploration_groups(&state);
    assert_eq!(
        groups.iter().map(|group| group.len()).collect::<Vec<_>>(),
        [2, 2, 1, 1]
    );
    assert_eq!(
        groups.iter().map(|group| &group.key).collect::<Vec<_>>(),
        [
            &crate::app::TranscriptBlockKey::tool_calls("read-a").unwrap(),
            &crate::app::TranscriptBlockKey::tool_calls("glob-c").unwrap(),
            &crate::app::TranscriptBlockKey::tool_calls("image-e").unwrap(),
            &crate::app::TranscriptBlockKey::tool_calls("lsp-f").unwrap(),
        ]
    );
    assert_eq!(
        super::render::latest_expandable_block_key(&state),
        crate::app::TranscriptBlockKey::tool_calls("lsp-f")
    );
}

fn exploration_snapshot_state(status: ToolStatus) -> AppState {
    let mut state = test_state();
    state.messages = vec![assistant_calls(vec![
        exploration_call("read-one", "read_file", ToolStatus::Done),
        exploration_call("search-two", "search_files", status),
        exploration_call("symbols-three", "lsp_symbols", ToolStatus::Done),
    ])];
    state
}

#[test]
fn exploration_collapsed_and_expanded_snapshots() {
    let state = exploration_snapshot_state(ToolStatus::Done);
    insta::assert_snapshot!(
        "exploration_collapsed",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );

    let mut expanded = state;
    expanded
        .expanded_blocks
        .push(crate::app::TranscriptBlockKey::tool_calls("read-one").expect("stable test id"));
    insta::assert_snapshot!(
        "exploration_expanded",
        wide_narrow(
            render_to_text(&expanded, 100, 34),
            render_to_text(&expanded, 60, 30)
        )
    );
}

#[test]
fn exploration_running_and_error_snapshots() {
    let running = exploration_snapshot_state(ToolStatus::Running);
    insta::assert_snapshot!(
        "exploration_running",
        wide_narrow(
            render_to_text(&running, 100, 30),
            render_to_text(&running, 60, 24)
        )
    );

    let failed = exploration_snapshot_state(ToolStatus::Error);
    insta::assert_snapshot!(
        "exploration_error",
        wide_narrow(
            render_to_text(&failed, 100, 30),
            render_to_text(&failed, 60, 24)
        )
    );
}

#[test]
fn exploration_collapsed_summary_is_capped_at_twelve_actions() {
    let mut state = test_state();
    state.messages = vec![assistant_calls(
        (0..14)
            .map(|index| exploration_call(&format!("call-{index}"), "read_file", ToolStatus::Done))
            .collect(),
    )];
    let rendered = render_to_text(&state, 100, 40);
    assert!(rendered.contains("+2 more"));
    assert!(rendered.contains("src/call-11.rs"));
    assert!(!rendered.contains("src/call-12.rs"));
    assert!(!rendered.contains("result for call-0"));
}

#[test]
fn exploration_ctrl_o_uses_the_stable_first_call_key() {
    let mut state = exploration_snapshot_state(ToolStatus::Done);
    assert!(crate::tui::input::handle_event(
        &mut state,
        &crate::tui::Event::Key(
            crossterm::event::KeyCode::Char('o'),
            crossterm::event::KeyModifiers::CONTROL,
        ),
        "/workspace/ca",
    ));
    let key = crate::app::TranscriptBlockKey::tool_calls("read-one").unwrap();
    assert_eq!(state.expanded_blocks.as_slice(), std::slice::from_ref(&key));
    assert!(render_to_text(&state, 100, 30).contains("result for read-one"));

    state.messages.insert(
        0,
        Message {
            role: Role::User,
            content: "Earlier history loaded".into(),
            tool_calls: None,
            thinking: None,
            timestamp: fixed_time(),
            response_items: Vec::new(),
        },
    );
    assert!(state.expanded_blocks.contains(&key));
    assert!(render_to_text(&state, 100, 34).contains("result for read-one"));

    crate::tui::input::handle_event(
        &mut state,
        &crate::tui::Event::Key(
            crossterm::event::KeyCode::Char('o'),
            crossterm::event::KeyModifiers::CONTROL,
        ),
        "/workspace/ca",
    );
    assert!(state.expanded_blocks.is_empty());
}

fn activity_call(
    id: &str,
    name: &str,
    arguments: serde_json::Value,
    status: ToolStatus,
    output: &str,
) -> ToolCall {
    ToolCall {
        id: id.into(),
        name: name.into(),
        arguments: arguments.to_string(),
        status,
        output: Some(output.into()),
    }
}

#[test]
fn activity_grouping_requires_two_calls_and_honors_visible_and_tool_boundaries() {
    let mut state = test_state();
    state.messages = vec![
        assistant_calls(vec![activity_call(
            "write-a",
            "write_file",
            serde_json::json!({"path":"src/a.rs","content":"a"}),
            ToolStatus::Done,
            "@@ /workspace/ca/src/a.rs (Created, 1 bytes) @@\n+a",
        )]),
        assistant_calls(vec![activity_call(
            "command-b",
            "run_command",
            serde_json::json!({"command":"cargo test -p ca activity"}),
            ToolStatus::Done,
            "exit code: 0",
        )]),
        Message {
            role: Role::Assistant,
            content: "Visible boundary".into(),
            tool_calls: Some(vec![
                activity_call(
                    "edit-c",
                    "edit_file",
                    serde_json::json!({"path":"src/c.rs"}),
                    ToolStatus::Done,
                    "@@ /workspace/ca/src/c.rs @@\n-old\n+new",
                ),
                activity_call(
                    "command-d",
                    "run_command",
                    serde_json::json!({"command":"cargo check"}),
                    ToolStatus::Done,
                    "exit code: 0",
                ),
            ]),
            thinking: None,
            timestamp: fixed_time(),
            response_items: Vec::new(),
        },
        Message {
            role: Role::Assistant,
            content: "Second visible boundary".into(),
            tool_calls: None,
            thinking: None,
            timestamp: fixed_time(),
            response_items: Vec::new(),
        },
        assistant_calls(vec![
            activity_call(
                "write-e",
                "write_file",
                serde_json::json!({"path":"src/e.rs","content":"e"}),
                ToolStatus::Done,
                "@@ /workspace/ca/src/e.rs (Created, 1 bytes) @@\n+e",
            ),
            exploration_call("read-boundary", "read_file", ToolStatus::Done),
            activity_call(
                "command-f",
                "run_command",
                serde_json::json!({"command":"cargo fmt --check"}),
                ToolStatus::Done,
                "exit code: 0",
            ),
        ]),
    ];

    let groups = super::render::activity_groups(&state);
    assert_eq!(
        groups.iter().map(|group| group.len()).collect::<Vec<_>>(),
        [2, 2]
    );
    assert_eq!(
        groups.iter().map(|group| &group.key).collect::<Vec<_>>(),
        [
            &crate::app::TranscriptBlockKey::tool_calls("write-a").unwrap(),
            &crate::app::TranscriptBlockKey::tool_calls("edit-c").unwrap(),
        ]
    );
}

#[test]
fn activity_grouping_stops_at_recorded_turn_boundaries() {
    let mut state = test_state();
    let first_timestamp = fixed_time();
    let second_timestamp = first_timestamp + chrono::Duration::seconds(1);
    state.messages = vec![
        assistant_calls(vec![activity_call(
            "turn-one-edit",
            "edit_file",
            serde_json::json!({"path":"src/one.rs","old_string":"a","new_string":"b"}),
            ToolStatus::Done,
            "@@ src/one.rs @@\n-a\n+b",
        )]),
        assistant_calls(vec![activity_call(
            "turn-two-command",
            "run_command",
            serde_json::json!({"command":"cargo check"}),
            ToolStatus::Done,
            "exit code: 0",
        )]),
    ];
    state.messages[0].timestamp = first_timestamp;
    state.messages[1].timestamp = second_timestamp;
    let first_turn = uuid::Uuid::parse_str("11111111-1111-4111-8111-111111111111").unwrap();
    let second_turn = uuid::Uuid::parse_str("22222222-2222-4222-8222-222222222222").unwrap();
    state.transcript_annotations = vec![
        crate::app::TranscriptMessageAnnotation {
            message_timestamp: first_timestamp,
            turn_id: first_turn,
            reasoning_duration_ms: None,
            provider: None,
            model: None,
            total_turn_duration_ms: None,
            terminal_outcome: None,
        },
        crate::app::TranscriptMessageAnnotation {
            message_timestamp: second_timestamp,
            turn_id: first_turn,
            reasoning_duration_ms: None,
            provider: None,
            model: None,
            total_turn_duration_ms: None,
            terminal_outcome: None,
        },
    ];
    assert_eq!(super::render::activity_groups(&state).len(), 1);

    state.transcript_annotations[1].turn_id = second_turn;
    assert!(super::render::activity_groups(&state).is_empty());
}

#[test]
fn activity_ctrl_o_expands_complete_commands_and_restored_file_labels() {
    let mut state = test_state();
    let command = "cargo test --workspace --all-features -- --test-threads=1";
    state.messages = vec![assistant_calls(vec![
        activity_call(
            "write-one",
            "write_file",
            serde_json::json!({"path":"src/佐井.rs","content":"new"}),
            ToolStatus::Done,
            "@@ /workspace/ca/src/佐井.rs (Created, 3 bytes) @@\n+new",
        ),
        activity_call(
            "command-two",
            "run_command",
            serde_json::json!({"command":command}),
            ToolStatus::Done,
            "exit code: 0\nstdout:\nall tests passed",
        ),
    ])];

    let collapsed = render_to_text(&state, 100, 30);
    assert!(collapsed.contains("Edited 1 file, ran a command"));
    assert!(!collapsed.contains(command));
    assert!(!collapsed.contains("all tests passed"));
    let copied = crate::tui::render::privacy_safe_transcript_text(&state, 100);
    assert!(copied.contains("write_file"));
    assert!(copied.contains("all tests passed"));

    assert!(crate::tui::input::handle_event(
        &mut state,
        &crate::tui::Event::Key(
            crossterm::event::KeyCode::Char('o'),
            crossterm::event::KeyModifiers::CONTROL,
        ),
        "/workspace/ca",
    ));
    let expanded = render_to_text(&state, 60, 80);
    assert!(expanded.contains("Created"));
    assert!(expanded.contains("src/佐"), "{expanded}");
    assert!(expanded.contains("井 .rs"), "{expanded}");
    assert!(expanded.contains("cargo test --workspace --all-features"));
    let compact = expanded.lines().map(str::trim).collect::<String>();
    assert!(compact.contains("--test-threads=1"), "{expanded}");
    assert!(expanded.contains("all tests passed"));
}

fn activity_header_rows(buffer: &Buffer, width: u16, height: u16) -> Vec<u16> {
    (0..height)
        .filter(|y| {
            let row = (0..width)
                .map(|x| buffer[(x, *y)].symbol())
                .collect::<String>();
            row.contains("Edited 1 file, ran a command")
        })
        .collect()
}

#[test]
fn clicking_an_older_visible_activity_header_toggles_that_exact_group() {
    let mut state = test_state();
    let group = |prefix: &str| {
        assistant_calls(vec![
            activity_call(
                &format!("{prefix}-edit"),
                "edit_file",
                serde_json::json!({"path":format!("src/{prefix}.rs")}),
                ToolStatus::Done,
                &format!("@@ /workspace/ca/src/{prefix}.rs @@\n-old\n+new"),
            ),
            activity_call(
                &format!("{prefix}-command"),
                "run_command",
                serde_json::json!({"command":format!("cargo check -p {prefix}")}),
                ToolStatus::Done,
                "exit code: 0",
            ),
        ])
    };
    state.messages = vec![
        group("older"),
        Message {
            role: Role::Assistant,
            content: "Boundary".into(),
            tool_calls: None,
            thinking: None,
            timestamp: fixed_time(),
            response_items: Vec::new(),
        },
        group("latest"),
    ];

    let (_, buffer) = rendered_frame(&state, 100, 40);
    let rows = activity_header_rows(&buffer, 100, 40);
    assert_eq!(rows.len(), 2);
    for phase in [
        crate::tui::event::MouseSelectionPhase::Start,
        crate::tui::event::MouseSelectionPhase::End,
    ] {
        assert!(crate::tui::input::handle_event(
            &mut state,
            &crate::tui::Event::MouseSelection(crate::tui::event::MouseSelectionEvent {
                phase,
                column: 3,
                row: rows[0],
            }),
            "/workspace/ca",
        ));
    }
    assert_eq!(
        state.expanded_blocks,
        vec![crate::app::TranscriptBlockKey::tool_calls("older-edit").unwrap()]
    );
    assert_eq!(
        state.status_message.as_deref(),
        Some("Expanded activity block")
    );
}

#[test]
fn clicking_a_scrolled_activity_header_uses_document_coordinates() {
    let mut state = test_state();
    state.messages = vec![
        assistant_calls(vec![
            activity_call(
                "scrolled-edit",
                "edit_file",
                serde_json::json!({"path":"src/scrolled.rs"}),
                ToolStatus::Done,
                "@@ /workspace/ca/src/scrolled.rs @@\n-old\n+new",
            ),
            activity_call(
                "scrolled-command",
                "run_command",
                serde_json::json!({"command":"cargo check -p scrolled"}),
                ToolStatus::Done,
                "exit code: 0",
            ),
        ]),
        Message {
            role: Role::Assistant,
            content: (0..24)
                .map(|index| format!("intervening line {index}"))
                .collect::<Vec<_>>()
                .join("\n"),
            tool_calls: None,
            thinking: None,
            timestamp: fixed_time(),
            response_items: Vec::new(),
        },
        assistant_calls(vec![
            activity_call(
                "latest-edit-one",
                "edit_file",
                serde_json::json!({"path":"src/latest-one.rs"}),
                ToolStatus::Done,
                "@@ src/latest-one.rs @@",
            ),
            activity_call(
                "latest-edit-two",
                "edit_file",
                serde_json::json!({"path":"src/latest-two.rs"}),
                ToolStatus::Done,
                "@@ src/latest-two.rs @@",
            ),
        ]),
    ];

    let _ = rendered_frame(&state, 100, 12);
    let mut scrolled_header_row = None;
    for offset in 1..=40 {
        state.transcript_scroll.request_offset(offset);
        let (_, buffer) = rendered_frame(&state, 100, 12);
        if let Some(row) = activity_header_rows(&buffer, 100, 12).into_iter().next() {
            scrolled_header_row = Some(row);
            break;
        }
    }
    let row =
        scrolled_header_row.expect("older activity header should become visible when scrolled");
    assert!(state.transcript_scroll.bottom_offset() > 0);
    for phase in [
        crate::tui::event::MouseSelectionPhase::Start,
        crate::tui::event::MouseSelectionPhase::End,
    ] {
        assert!(crate::tui::input::handle_event(
            &mut state,
            &crate::tui::Event::MouseSelection(crate::tui::event::MouseSelectionEvent {
                phase,
                column: 3,
                row,
            }),
            "/workspace/ca",
        ));
    }
    assert_eq!(
        state.expanded_blocks,
        vec![crate::app::TranscriptBlockKey::tool_calls("scrolled-edit").unwrap()]
    );
}

#[test]
fn pager_overlay_disables_activity_header_clicks() {
    let mut state = test_state();
    state.messages = vec![assistant_calls(vec![
        activity_call(
            "overlay-edit",
            "edit_file",
            serde_json::json!({"path":"src/overlay.rs"}),
            ToolStatus::Done,
            "@@ /workspace/ca/src/overlay.rs @@\n-old\n+new",
        ),
        activity_call(
            "overlay-command",
            "run_command",
            serde_json::json!({"command":"cargo check"}),
            ToolStatus::Done,
            "exit code: 0",
        ),
    ])];
    let (_, transcript) = rendered_frame(&state, 100, 30);
    let header_row = activity_header_rows(&transcript, 100, 30)[0];

    state.pager_open = true;
    state.pager_content = "Pager content\nNo transcript clicks".into();
    let _ = rendered_frame(&state, 100, 30);
    for phase in [
        crate::tui::event::MouseSelectionPhase::Start,
        crate::tui::event::MouseSelectionPhase::End,
    ] {
        crate::tui::input::handle_event(
            &mut state,
            &crate::tui::Event::MouseSelection(crate::tui::event::MouseSelectionEvent {
                phase,
                column: 3,
                row: header_row,
            }),
            "/workspace/ca",
        );
    }
    assert!(state.expanded_blocks.is_empty());
}

#[test]
fn live_mutation_presentation_overrides_restored_write_marker_guessing() {
    let mut state = test_state();
    state.messages = vec![assistant_calls(vec![
        activity_call(
            "write-live",
            "write_file",
            serde_json::json!({"path":"src/live.rs","content":"new"}),
            ToolStatus::Done,
            "@@ /workspace/ca/src/live.rs (Created, 3 bytes) @@\n+new",
        ),
        activity_call(
            "check-live",
            "run_command",
            serde_json::json!({"command":"cargo check"}),
            ToolStatus::Done,
            "exit code: 0",
        ),
    ])];
    state.file_activity_presentations.insert(
        "write-live".into(),
        crate::app::FileActivityPresentation {
            path: "/workspace/ca/src/live.rs".into(),
            kind: crate::app::FileActivityKind::Edited,
        },
    );
    state
        .expanded_blocks
        .push(crate::app::TranscriptBlockKey::tool_calls("write-live").unwrap());

    let rendered = render_to_text(&state, 100, 40);
    assert!(rendered.contains("Edited src/live.rs"), "{rendered}");
    assert!(!rendered.contains("Created src/live.rs"), "{rendered}");
}

#[test]
fn activity_grouping_includes_partial_streaming_calls_without_losing_status() {
    let mut state = test_state();
    state.messages = vec![assistant_calls(vec![activity_call(
        "write-complete",
        "write_file",
        serde_json::json!({"path":"src/complete.rs","content":"done"}),
        ToolStatus::Done,
        "@@ /workspace/ca/src/complete.rs (Created, 4 bytes) @@\n+done",
    )])];
    state.streaming = Some(StreamingState {
        content: String::new(),
        tool_calls: vec![PartialToolCall {
            index: 0,
            id: Some("command-partial".into()),
            name: Some("run_command".into()),
            arguments: serde_json::json!({"command":"cargo test activity"}).to_string(),
        }],
        thinking: None,
        response_items: Vec::new(),
        last_usage: None,
        usage_handle: crate::usage::UsageHandle::test_handle(),
        reasoning_started_at: None,
        reasoning_duration: None,
        generation_started_at: None,
    });

    let groups = super::render::activity_groups(&state);
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].len(), 2);
    let rendered = render_to_text(&state, 100, 30);
    assert!(
        rendered.contains("Edited 1 file, running a command…"),
        "{rendered}"
    );
}

#[test]
fn expanded_activity_rows_report_failed_file_actions_and_sanitize_commands() {
    let mut state = test_state();
    state.messages = vec![assistant_calls(vec![
        activity_call(
            "edit-failed",
            "edit_file",
            serde_json::json!({"path":"src/fail.rs"}),
            ToolStatus::Error,
            "Error: old text not found",
        ),
        activity_call(
            "command-failed",
            "run_command",
            serde_json::json!({"command":"printf '\u{1b}[31mbad'"}),
            ToolStatus::Error,
            "exit code: 1",
        ),
    ])];
    state
        .expanded_blocks
        .push(crate::app::TranscriptBlockKey::tool_calls("edit-failed").unwrap());

    let rendered = render_to_text(&state, 100, 40);
    assert!(
        rendered.contains("Failed to edit src/fail.rs"),
        "{rendered}"
    );
    assert!(rendered.contains("Command failed"), "{rendered}");
    assert!(!rendered.contains('\u{1b}'));
}

fn activity_snapshot_state(command_status: ToolStatus, expanded: bool) -> AppState {
    let mut state = test_state();
    state.messages = vec![assistant_calls(vec![
        activity_call(
            "activity-create",
            "write_file",
            serde_json::json!({"path":"src/new.rs","content":"new"}),
            ToolStatus::Done,
            "@@ /workspace/ca/src/new.rs (Created, 3 bytes) @@\n+new",
        ),
        activity_call(
            "activity-edit",
            "edit_file",
            serde_json::json!({"path":"src/長い名前.rs"}),
            ToolStatus::Done,
            "@@ /workspace/ca/src/長い名前.rs @@\n-old\n+new",
        ),
        activity_call(
            "activity-delete",
            "delete_file",
            serde_json::json!({"path":"src/obsolete.rs"}),
            ToolStatus::Done,
            "@@ /workspace/ca/src/obsolete.rs (deleted) @@\n-src/obsolete.rs",
        ),
        activity_call(
            "activity-directory",
            "create_directory",
            serde_json::json!({"path":"src/generated"}),
            ToolStatus::Done,
            "Created directory /workspace/ca/src/generated",
        ),
        activity_call(
            "activity-command",
            "run_command",
            serde_json::json!({
                "command":"cargo test --workspace --all-features -- --test-threads=1"
            }),
            command_status.clone(),
            if command_status == ToolStatus::Error {
                "exit code: 101\nstderr:\nactivity verification failed"
            } else {
                "exit code: 0\nstdout:\nactivity verification passed"
            },
        ),
    ])];
    if expanded {
        state
            .expanded_blocks
            .push(crate::app::TranscriptBlockKey::tool_calls("activity-create").unwrap());
    }
    state
}

#[test]
fn activity_block_collapsed_and_expanded_snapshots() {
    insta::assert_snapshot!(
        "activity_block_collapsed_184",
        render_to_text(&activity_snapshot_state(ToolStatus::Done, false), 184, 34)
    );
    insta::assert_snapshot!(
        "activity_block_collapsed_100",
        render_to_text(&activity_snapshot_state(ToolStatus::Done, false), 100, 30)
    );
    insta::assert_snapshot!(
        "activity_block_collapsed_60",
        render_to_text(&activity_snapshot_state(ToolStatus::Done, false), 60, 24)
    );
    insta::assert_snapshot!(
        "activity_block_expanded_184",
        render_to_text(&activity_snapshot_state(ToolStatus::Done, true), 184, 52)
    );
    insta::assert_snapshot!(
        "activity_block_expanded_100",
        render_to_text(&activity_snapshot_state(ToolStatus::Done, true), 100, 52)
    );
    insta::assert_snapshot!(
        "activity_block_expanded_60",
        render_to_text(&activity_snapshot_state(ToolStatus::Done, true), 60, 58)
    );
}

#[test]
fn activity_block_running_error_and_file_only_snapshots() {
    insta::assert_snapshot!(
        "activity_block_running_100",
        render_to_text(
            &activity_snapshot_state(ToolStatus::Running, false),
            100,
            30
        )
    );
    insta::assert_snapshot!(
        "activity_block_error_60",
        render_to_text(&activity_snapshot_state(ToolStatus::Error, false), 60, 24)
    );

    let mut file_only = test_state();
    file_only.messages = vec![assistant_calls(vec![
        activity_call(
            "file-only-create",
            "write_file",
            serde_json::json!({"path":"src/one.rs","content":"one"}),
            ToolStatus::Done,
            "@@ /workspace/ca/src/one.rs (Created, 3 bytes) @@\n+one",
        ),
        activity_call(
            "file-only-edit",
            "edit_file",
            serde_json::json!({"path":"src/two.rs"}),
            ToolStatus::Done,
            "@@ /workspace/ca/src/two.rs @@\n-old\n+two",
        ),
    ])];
    insta::assert_snapshot!(
        "activity_block_file_only_100",
        render_to_text(&file_only, 100, 30)
    );
}

#[test]
fn collapsing_activity_block_clears_every_expanded_cell() {
    let width = 100;
    let height = 60;
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut state = activity_snapshot_state(ToolStatus::Done, true);
    terminal
        .draw(|frame| crate::tui::render(frame, &state))
        .unwrap();
    state.expanded_blocks.clear();
    terminal
        .draw(|frame| crate::tui::render(frame, &state))
        .unwrap();
    let redrawn = terminal.backend().buffer().clone();
    let (_, fresh) = rendered_frame(
        &activity_snapshot_state(ToolStatus::Done, false),
        width,
        height,
    );
    assert_eq!(redrawn, fresh);
}

fn attachment_picker_state(phase: crate::attachments::PickerPhase) -> AppState {
    let mut state = test_state();
    state.input = "Summarize @".into();
    state.input_cursor = state.input.len();
    state.file_picker_open = true;
    state.file_picker_phase = phase;
    state.file_picker_generation = 7;
    state
}

#[test]
fn path_completion_loading_results_and_error_snapshots() {
    let mut loading = test_state();
    loading.input = "Open ./s".into();
    loading.input_cursor = loading.input.len();
    loading.composer_ui.path_completion.open = true;
    loading.composer_ui.path_completion.loading = true;
    loading.composer_ui.path_completion.token = "./s".into();
    insta::assert_snapshot!(
        "path_completion_loading",
        wide_narrow(
            render_to_text(&loading, 100, 30),
            render_to_text(&loading, 60, 24)
        )
    );

    let mut results = loading;
    results.composer_ui.path_completion.loading = false;
    results.composer_ui.path_completion.selected = 1;
    results.composer_ui.path_completion.entries = vec![
        crate::tui::composer::PathCompletionEntry {
            display: "src/".into(),
            replacement: "./src/".into(),
            is_dir: true,
        },
        crate::tui::composer::PathCompletionEntry {
            display: "specification-with-a-long-unicode-name-你.md".into(),
            replacement: "./specification-with-a-long-unicode-name-你.md".into(),
            is_dir: false,
        },
    ];
    insta::assert_snapshot!(
        "path_completion_results",
        wide_narrow(
            render_to_text(&results, 100, 30),
            render_to_text(&results, 60, 24)
        )
    );

    let mut failed = test_state();
    failed.input = "Open ./s".into();
    failed.input_cursor = failed.input.len();
    failed.composer_ui.path_completion.open = true;
    failed.composer_ui.path_completion.token = "./s".into();
    failed.composer_ui.path_completion.loading = false;
    failed.composer_ui.path_completion.error =
        Some("Path completion failed: permission denied".into());
    insta::assert_snapshot!(
        "path_completion_error",
        wide_narrow(
            render_to_text(&failed, 100, 30),
            render_to_text(&failed, 60, 24)
        )
    );
}

#[test]
fn attachment_picker_snapshots() {
    let loading = attachment_picker_state(crate::attachments::PickerPhase::Indexing);
    insta::assert_snapshot!(
        "attachment_picker_loading",
        wide_narrow(
            render_to_text(&loading, 100, 30),
            render_to_text(&loading, 60, 24)
        )
    );

    let mut populated = attachment_picker_state(crate::attachments::PickerPhase::Ready);
    populated.file_picker_filter = "main#12-40".into();
    populated.file_picker_discovered = 50_000;
    populated.file_picker_matches = 3;
    populated.file_picker_truncated = true;
    populated.file_picker_entries = vec![
        crate::attachments::PickerEntry::File(crate::attachments::IndexedFile {
            root_id: "primary".into(),
            root_label: "workspace".into(),
            relative: "src/main.rs".into(),
            reference: "src/main.rs".into(),
            read_only: false,
        }),
        crate::attachments::PickerEntry::File(crate::attachments::IndexedFile {
            root_id: "docs".into(),
            root_label: "docs".into(),
            relative: "guide/main.md".into(),
            reference: "/workspace/docs/guide/main.md".into(),
            read_only: true,
        }),
        crate::attachments::PickerEntry::Mcp(crate::mcp::McpResourceInfo {
            server: "reference".into(),
            uri: "docs://main".into(),
            name: "Main guide".into(),
            description: None,
            mime_type: Some("text/plain".into()),
        }),
    ];
    populated.selected_file_idx = 1;
    insta::assert_snapshot!(
        "attachment_picker_results",
        wide_narrow(
            render_to_text(&populated, 100, 30),
            render_to_text(&populated, 60, 24)
        )
    );

    let mut no_match = attachment_picker_state(crate::attachments::PickerPhase::Ready);
    no_match.file_picker_filter = "does-not-exist".into();
    no_match.file_picker_discovered = 82;
    insta::assert_snapshot!(
        "attachment_picker_no_match",
        wide_narrow(
            render_to_text(&no_match, 100, 30),
            render_to_text(&no_match, 60, 24)
        )
    );

    let mut failed = attachment_picker_state(crate::attachments::PickerPhase::Failed);
    failed.file_picker_error = Some("Workspace paths could not be indexed".into());
    insta::assert_snapshot!(
        "attachment_picker_error",
        wide_narrow(
            render_to_text(&failed, 100, 30),
            render_to_text(&failed, 60, 24)
        )
    );
}

fn prompt_stash_render_state() -> AppState {
    let mut state = test_state();
    state.input = "current composer stays visible behind the picker".into();
    state.input_cursor = state.input.len();
    state.prompt_stash = Some(crate::app::PromptStashPickerState {
        entries: vec![
            crate::prompt_stash::PromptDraft {
                id: uuid::Uuid::parse_str("00000000-0000-0000-0000-000000000131").unwrap(),
                text: "Review @src/main.rs#12-40 before changing the parser.".into(),
                created_at: "2026-08-16T12:03:00Z".parse().unwrap(),
            },
            crate::prompt_stash::PromptDraft {
                id: uuid::Uuid::parse_str("00000000-0000-0000-0000-000000000132").unwrap(),
                text: "First line\nSecond line\nThird line with a longer explanation".into(),
                created_at: "2026-08-14T11:05:00Z".parse().unwrap(),
            },
        ],
        selected_idx: 1,
        snapshot_at: "2026-08-16T12:05:00Z".parse().unwrap(),
        busy: false,
        writes_enabled: true,
        delete_confirmation: Some(
            uuid::Uuid::parse_str("00000000-0000-0000-0000-000000000132").unwrap(),
        ),
        notice: Some("Press d again to delete this draft".into()),
        error: None,
    });
    state
}

#[test]
fn prompt_stash_picker_snapshots() {
    let state = prompt_stash_render_state();
    insta::assert_snapshot!(
        "prompt_stash_populated",
        wide_narrow(
            render_to_text(&state, 100, 30),
            render_to_text(&state, 60, 24)
        )
    );

    let mut busy = prompt_stash_render_state();
    let picker = busy.prompt_stash.as_mut().unwrap();
    picker.busy = true;
    picker.delete_confirmation = None;
    picker.notice = Some("Swapping composer and stashed draft…".into());
    insta::assert_snapshot!(
        "prompt_stash_busy",
        wide_narrow(
            render_to_text(&busy, 100, 30),
            render_to_text(&busy, 60, 24)
        )
    );

    let mut bare = prompt_stash_render_state();
    let picker = bare.prompt_stash.as_mut().unwrap();
    picker.entries.clear();
    picker.selected_idx = 0;
    picker.writes_enabled = false;
    picker.delete_confirmation = None;
    picker.notice =
        Some("Bare mode can inspect no persistent drafts and cannot save or restore them".into());
    picker.error = Some("Prompt stash persistence is unavailable in bare mode".into());
    insta::assert_snapshot!(
        "prompt_stash_bare",
        wide_narrow(
            render_to_text(&bare, 100, 30),
            render_to_text(&bare, 60, 24)
        )
    );
}

#[test]
fn workspace_trust_prompt_names_path_and_recovery() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("README.md"), "trust fixture\n").unwrap();
    let mut state = test_state();
    state.workspace = crate::workspace::WorkspaceContext::discover(temp.path()).unwrap();
    state.cwd = state.workspace.cwd().to_string_lossy().into_owned();
    state.workspace_trust_prompt = true;
    let wide = render_to_text(&state, 100, 30);
    let narrow = render_to_text(&state, 60, 24);
    for frame in [&wide, &narrow] {
        assert!(frame.contains("Workspace Trust"), "{frame}");
        assert!(frame.contains("Trust this workspace?"), "{frame}");
        assert!(
            frame.contains("Until trusted") || frame.contains("stay blocked"),
            "{frame}"
        );
        assert!(frame.contains("trusted_workspaces"), "{frame}");
        assert!(frame.contains("--trust"), "{frame}");
        assert!(frame.contains("process-only"), "{frame}");
        assert!(frame.contains("y") && frame.contains("n"), "{frame}");
    }
}

fn dashboard_now() -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::from_timestamp(1_700_000_000, 0).expect("fixed dashboard clock")
}

fn dashboard_state() -> AppState {
    let mut state = test_state();
    state.dashboard.now_override = Some(dashboard_now());
    state.status_git_branch = Some("main".into());
    state.current_model = Some(ModelInfo {
        provider: "local".into(),
        id: "coder".into(),
        display_name: "Coder".into(),
    });
    crate::tui::dashboard::open(&mut state);
    state
}

#[test]
fn agent_dashboard_snapshots() {
    let empty = dashboard_state();
    insta::assert_snapshot!(
        "agent_dashboard_empty",
        wide_narrow(
            render_to_text(&empty, 100, 30),
            render_to_text(&empty, 60, 24)
        )
    );

    let mut populated = dashboard_state();
    populated.session_name = "bsd2 MiMo Flash official Q2_K setup".into();
    populated.turn_lifecycle.begin();
    populated
        .turn_lifecycle
        .set_phase(crate::activity::ActivityPhase::Waiting);
    populated.session_list = vec![
        crate::session::SessionSummary {
            storage_key: "qwen".into(),
            id: uuid::Uuid::from_u128(2),
            title: "bsd2 local Qwen Coder 30B A3B".into(),
            pinned: false,
            workspace: "/Volumes/projects/freebsd-localagent".into(),
            updated_at: dashboard_now() - chrono::Duration::hours(3),
        },
        crate::session::SessionSummary {
            storage_key: "review".into(),
            id: uuid::Uuid::from_u128(3),
            title: "Review FreeBSD fusion branch".into(),
            pinned: false,
            workspace: "/Volumes/projects/freebsd-localagent".into(),
            updated_at: dashboard_now() - chrono::Duration::days(2),
        },
    ];
    populated.dashboard.collapsed_inactive = false;
    populated.dashboard.cursor = crate::tui::dashboard::DashboardCursor::Section(
        crate::tui::dashboard::DashboardGroup::Inactive,
    );
    insta::assert_snapshot!(
        "agent_dashboard_roster",
        wide_narrow(
            render_to_text(&populated, 100, 30),
            render_to_text(&populated, 60, 24)
        )
    );

    let mut usage = populated;
    usage.usage_modal = Some(crate::tui::dashboard::UsageModalState {
        tab: crate::tui::dashboard::UsageTab::Limit,
        scroll: 0,
        context_body: "Context\n\nRequest context: 1200 / 32000 estimated tokens (3%)".into(),
        limit_body: "MiMo  8000000 credits · USD 50\nDeepSeek  CNY 110 remaining\nThis process  0 requests · 0 in · 0 out".into(),
        session_body: "Session\nTitle: bsd2 MiMo Flash official Q2_K setup".into(),
        context_percent: Some(3),
        limit_percent: Some(47),
        limit_title: "Token plan (MiMo)".into(),
        limit_reset: Some("3760000 of 8000000 credits used".into()),
    });
    insta::assert_snapshot!(
        "agent_dashboard_usage_limit",
        wide_narrow(
            render_to_text(&usage, 100, 30),
            render_to_text(&usage, 60, 24)
        )
    );
}
