use crate::app::{AgentMode, ModelInfo};
use crate::tui::event::{Event, MouseSelectionEvent, MouseSelectionPhase};
use crossterm::event::{KeyCode, KeyModifiers};

pub fn handle_event(state: &mut crate::app::AppState, event: &Event, cwd: &str) -> bool {
    // Reset status fade timer on any keypress — new status messages will restart the clock
    state.status_set_at = None;
    if let Event::MouseSelection(selection) = event {
        return handle_mouse_selection(state, *selection, copy_text_to_clipboard);
    }
    if let Event::MouseScroll(scroll) = event {
        let dragging = if state.pager_open {
            state.pager_selection.is_dragging()
        } else {
            state.transcript_scroll.is_dragging()
        };
        if dragging {
            let selection = if state.pager_open {
                &mut state.pager_selection
            } else {
                &mut state.transcript_scroll
            };
            selection.update_selection(scroll.column, scroll.row);
        } else {
            state.transcript_scroll.clear_selection();
            state.pager_selection.clear_selection();
        }
        return handle_mouse_scroll(state, scroll.direction, cwd, if dragging { 1 } else { 3 });
    }
    if matches!(event, Event::Tick) {
        let edge = if state.pager_open {
            state.pager_selection.drag_edge()
        } else {
            state.transcript_scroll.drag_edge()
        };
        if let Some(edge) = edge {
            let direction = match edge {
                crate::tui::transcript::SelectionEdge::Up => crate::tui::event::ScrollDirection::Up,
                crate::tui::transcript::SelectionEdge::Down => {
                    crate::tui::event::ScrollDirection::Down
                }
            };
            return handle_mouse_scroll(state, direction, cwd, 1);
        }
    }
    if !matches!(event, Event::Tick) {
        state.transcript_scroll.clear_selection();
        let copy_all = state.pager_open && matches!(event, Event::Key(KeyCode::Char('c' | 'C'), _));
        let comment_selection = state.pager_open
            && state.plan_reader.is_some()
            && matches!(event, Event::Key(KeyCode::Char('n' | 'N'), _));
        if !copy_all && !comment_selection {
            state.pager_selection.clear_selection();
        }
    }
    if let Event::Paste(pasted) = event {
        if let Some(focus) = state.agent_activity_focus.as_mut() {
            if let Some(draft) = focus.message_draft.as_mut() {
                let pasted = crate::terminal_text::sanitize_terminal_text(pasted);
                let remaining = 4096_usize.saturating_sub(draft.len());
                let mut end = pasted.len().min(remaining);
                while !pasted.is_char_boundary(end) {
                    end = end.saturating_sub(1);
                }
                draft.insert_str(focus.message_cursor, &pasted[..end]);
                focus.message_cursor += end;
            }
            return true;
        }
        if state.prompt_stash.is_some() {
            return true;
        }
    }
    match event {
        Event::Key(code, mods) => {
            if state.loop_guard_interaction.is_some() {
                return handle_loop_guard_interaction(state, *code);
            }
            if state.mcp_task_active && *code == KeyCode::Esc {
                state.mcp_cancel_requested = true;
                state.pager_open = false;
                state.status_message = Some("Cancelling MCP authorization…".into());
                return true;
            }
            if state.workspace_trust_prompt {
                return handle_workspace_trust_prompt(state, *code);
            }
            if state.worktree_setup_confirmation.is_some() {
                return handle_worktree_setup_confirmation(state, *code);
            }
            if state.support_bundle_confirmation.is_some() {
                return handle_support_bundle_confirmation(state, *code);
            }
            if state.setup_import_confirmation.is_some() {
                return handle_setup_import_confirmation(state, *code);
            }
            if state.trace_delete_confirmation.is_some() {
                return handle_trace_delete_confirmation(state, *code);
            }
            if state.project_init.is_some() {
                return handle_project_init(state, *code, *mods);
            }
            if state.output_full_confirmation.is_some() {
                return handle_output_full_confirmation(state, *code);
            }
            if state.scratch_delete_confirmation.is_some() {
                return handle_scratch_delete_confirmation(state, *code);
            }
            if state.route_preset_overwrite.is_some() {
                return handle_route_preset_overwrite(state, *code);
            }
            if state.route_preset_picker.is_some() {
                return handle_route_preset_picker(state, *code);
            }
            if state.reasoning_picker.is_some() {
                return handle_reasoning_picker(state, *code);
            }
            if state.worktree_cleanup_confirmation.is_some() {
                return handle_worktree_cleanup_confirmation(state, *code);
            }
            if state.model_picker_open {
                return handle_model_picker(state, *code, *mods);
            }
            if state.multi_run_manager.is_some() {
                return handle_multi_run_manager(state, *code, *mods);
            }
            if state.task_graph_manager.is_some() {
                return handle_task_graph_manager(state, *code, *mods);
            }
            if state.workflow_clear_confirmation.is_some() {
                return handle_workflow_clear_confirmation(state, *code);
            }
            if state.workflow_preview.is_some() {
                return handle_workflow_preview(state, *code);
            }
            if state.btw.is_some() {
                return handle_btw(state, *code);
            }
            if state.voice.is_some() {
                return handle_voice(state, *code, *mods);
            }
            if state.keymap_hub.is_some() {
                return handle_keymap_hub(state, *code, *mods);
            }
            if state.history_search.is_some() {
                return handle_history_search(state, *code);
            }
            if state.prompt_stash.is_some() {
                return handle_prompt_stash(state, *code);
            }
            if state.hook_manager.is_some() {
                return handle_hook_manager(state, *code, *mods);
            }
            if state.permission_manager.is_some() {
                return handle_permission_manager(state, *code, *mods);
            }
            if state.memory_manager.is_some() {
                return handle_memory_manager(state, *code, *mods);
            }
            if state.context_limit_editor.is_some() {
                return handle_context_limit_editor(state, *code);
            }
            if state.model_prompt_cache_editor.is_some() {
                return handle_model_prompt_cache_editor(state, *code);
            }
            if state.theme_manager.is_some() {
                return handle_theme_manager(state, *code, *mods);
            }
            if state.copy_picker.is_some() {
                return handle_copy_picker(state, *code, *mods);
            }
            if state.notification_picker.is_some() {
                return handle_notification_picker(state, *code);
            }
            if state.command_output_picker.is_some() {
                return handle_command_output_picker(state, *code);
            }
            if state.statusline_manager.is_some() {
                return handle_statusline_manager(state, *code, *mods);
            }
            if state.sandbox_hub.is_some() {
                return handle_sandbox_hub(state, *code);
            }
            if state.config_hub.is_some() {
                return handle_config_hub(state, *code);
            }
            if state.provider_editor.is_some() {
                return handle_provider_editor(state, *code, *mods);
            }
            if state.provider_auth.is_some() {
                return handle_provider_auth(state, *code, *mods);
            }
            if state.provider_discovery.is_some() {
                return handle_provider_discovery(state, *code, *mods);
            }
            if state.provider_manager.is_some() {
                return handle_provider_manager(state, *code, *mods);
            }
            if state.composer_ui.path_completion.open && handle_path_completion(state, *code, *mods)
            {
                return true;
            }
            if state.slash_palette_open {
                return handle_slash_palette(state, *code, *mods);
            }
            if state.session_portability.is_some() {
                return handle_session_portability(state, *code);
            }
            if state.session_retention.is_some() {
                return handle_session_retention(state, *code, *mods);
            }
            if state.usage_modal.is_some() {
                return crate::tui::dashboard::handle_usage_modal(state, *code, *mods);
            }
            if state.dashboard.open {
                if state.session_picker_open {
                    return handle_session_picker(state, *code, *mods);
                }
                if state.pager_open {
                    return handle_pager(state, *code);
                }
                if *mods == KeyModifiers::CONTROL
                    && matches!(*code, KeyCode::Char('.') | KeyCode::Char('?'))
                    || (*mods == KeyModifiers::NONE
                        && *code == KeyCode::Char('?')
                        && state.input.is_empty())
                {
                    state.pager_content = shortcut_help(state);
                    state.pager_scroll = 0;
                    state.pager_open = true;
                    return true;
                }
                return crate::tui::dashboard::handle(state, *code, *mods);
            }
            if state.session_picker_open {
                return handle_session_picker(state, *code, *mods);
            }
            if state.timeline_open {
                return handle_timeline(state, *code);
            }
            if state.queue_view_open {
                return handle_queue_view(state, *code, *mods);
            }
            if state.change_review.open {
                return handle_change_review(state, *code, *mods);
            }
            if state.change_walkthrough.is_some() {
                return handle_change_walkthrough(state, *code, *mods);
            }
            if state.terminal_manager.is_some() {
                return handle_terminal_manager(state, *code, *mods);
            }
            if state.jobs_view_open {
                return handle_jobs_view(state, *code);
            }
            if state.pager_open {
                return handle_pager(state, *code);
            }
            if state.file_picker_open {
                return handle_file_picker(state, *code, *mods, cwd);
            }
            if state.plan_proposal.is_some() && !state.turn_lifecycle.is_active() {
                if *code == KeyCode::Enter
                    && mods.is_empty()
                    && state.input.is_empty()
                    && state.pasted_images.is_empty()
                {
                    state.pending_input_action = Some(crate::tui::commands::InputAction::Command(
                        crate::tui::commands::CommandAction {
                            id: crate::tui::commands::CommandId::Plan,
                            args: "execute".into(),
                        },
                    ));
                    return true;
                }
                if *code == KeyCode::Esc && mods.is_empty() {
                    crate::plan::dismiss(state);
                    return true;
                }
            }
            // Emergency process controls remain authoritative even while a configurable
            // two-stroke chord is pending. They are deliberately outside the keymap.
            if *mods == KeyModifiers::CONTROL && *code == KeyCode::Char('\\') {
                state.pending_key_chord = None;
                state.pending_input_action = Some(crate::tui::commands::InputAction::Command(
                    crate::tui::commands::CommandAction {
                        id: crate::tui::commands::CommandId::Dashboard,
                        args: String::new(),
                    },
                ));
                return true;
            }
            if *mods == KeyModifiers::CONTROL && *code == KeyCode::Char('c') {
                state.pending_key_chord = None;
                state.should_quit = true;
                return true;
            }
            if *mods == KeyModifiers::CONTROL && *code == KeyCode::Char('d') {
                state.pending_key_chord = None;
                if state.input.is_empty() {
                    state.should_quit = true;
                }
                return true;
            }
            if *code == KeyCode::Esc
                && state.turn_lifecycle.is_active()
                && state.pending_key_chord.is_some()
            {
                state.pending_key_chord = None;
                state.pending_input_action = Some(crate::tui::commands::InputAction::StopTurn);
                return true;
            }
            if state.agent_activity_focus.is_some() {
                return handle_agent_activity_focus(state, *code, *mods);
            }
            if let Some(pending) = state.pending_key_chord.take() {
                let second = crate::keymap::KeyStroke::from_event(*code, *mods);
                return match state.keymap.resolve(pending.first, Some(second)) {
                    crate::keymap::KeyResolution::Action(action) => {
                        state.status_message = None;
                        dispatch_key_action(state, action);
                        true
                    }
                    _ if second.modifiers == KeyModifiers::CONTROL
                        && second.code == KeyCode::Char('r') =>
                    {
                        let busy = state.turn_lifecycle.is_active() || state.review_in_flight;
                        queue_command(
                            state,
                            crate::tui::commands::CommandAction {
                                id: crate::tui::commands::CommandId::Diff,
                                args: String::new(),
                            },
                            busy,
                        );
                        true
                    }
                    _ => {
                        state.status_message =
                            Some(format!("No binding for {} {}", pending.first, second));
                        true
                    }
                };
            }
            if *mods == KeyModifiers::CONTROL && *code == KeyCode::Char('r') {
                let busy = state.turn_lifecycle.is_active() || state.review_in_flight;
                queue_command(
                    state,
                    crate::tui::commands::CommandAction {
                        id: crate::tui::commands::CommandId::Diff,
                        args: String::new(),
                    },
                    busy,
                );
                return true;
            }
            let stroke = crate::keymap::KeyStroke::from_event(*code, *mods);
            match state.keymap.resolve(stroke, None) {
                crate::keymap::KeyResolution::Action(action) => {
                    dispatch_key_action(state, action);
                    return true;
                }
                crate::keymap::KeyResolution::Prefix => {
                    state.pending_key_chord = Some(crate::app::PendingKeyChord {
                        first: stroke,
                        started_at: std::time::Instant::now(),
                    });
                    state.status_message = Some(format!("{} …", stroke));
                    return true;
                }
                crate::keymap::KeyResolution::NoMatch => {}
            }
            // Turn controls work in both Vim modes; modal/completion handling
            // above still owns its keys before the composer gets them.
            if *mods == KeyModifiers::NONE {
                if *code == KeyCode::Esc && state.turn_lifecycle.is_active() {
                    state.pending_input_action = Some(crate::tui::commands::InputAction::StopTurn);
                    return true;
                }
                if *code == KeyCode::Enter
                    && (state.input_queue.editing_id.is_some()
                        || (state.turn_lifecycle.is_active() && !composer_is_command(state)))
                {
                    request_interrupt_submission(state);
                    return true;
                }
            }
            // Vim mode: normal mode intercepts keypresses
            if state.vim_mode && state.vim_state == crate::app::VimMode::Normal {
                return handle_vim_normal(state, *code);
            }
            match (*mods, *code) {
                (KeyModifiers::NONE, KeyCode::Char('?')) if state.input.is_empty() => {
                    state.pager_content = shortcut_help(state);
                    state.pager_scroll = 0;
                    state.pager_open = true;
                    true
                }
                // Slash completion is an inline composer token.
                (KeyModifiers::NONE, KeyCode::Char('/')) if state.input.is_empty() => {
                    insert_char(state, '/');
                    state.slash_palette_open = true;
                    state.slash_filter.clear();
                    state.selected_slash_idx = 0;
                    true
                }
                (KeyModifiers::CONTROL, KeyCode::Char('y')) => {
                    // Yank from kill ring
                    if let Some(text) = state.kill_ring.first().cloned() {
                        state.kill_ring_idx = Some(0);
                        state
                            .composer_ui
                            .apply_edit(state.input_cursor..state.input_cursor, text.len());
                        state.input.insert_str(state.input_cursor, &text);
                        state.input_cursor += text.len();
                    }
                    true
                }
                (KeyModifiers::ALT, KeyCode::Char('Y'))
                | (KeyModifiers::ALT, KeyCode::Char('y')) => {
                    // Cycle kill ring
                    if let Some(idx) = state.kill_ring_idx {
                        // Remove old yanked text before cursor
                        if let Some(old) = state.kill_ring.get(idx).cloned() {
                            let start = state.input_cursor.saturating_sub(old.len());
                            state.composer_ui.apply_edit(start..state.input_cursor, 0);
                            state.input.replace_range(start..state.input_cursor, "");
                            state.input_cursor = start;
                        }
                        let next = (idx + 1) % state.kill_ring.len();
                        if let Some(text) = state.kill_ring.get(next).cloned() {
                            state.kill_ring_idx = Some(next);
                            state
                                .composer_ui
                                .apply_edit(state.input_cursor..state.input_cursor, text.len());
                            state.input.insert_str(state.input_cursor, &text);
                            state.input_cursor += text.len();
                        }
                    }
                    true
                }
                (KeyModifiers::SHIFT, KeyCode::Enter) => {
                    insert_char(state, '\n');
                    true
                }
                (KeyModifiers::NONE, KeyCode::Enter) if state.input.starts_with('/') => {
                    let Some(action) = crate::tui::commands::parse_command_line_with_model_alias(
                        &state.input,
                        state
                            .model_list
                            .iter()
                            .map(|model| model.id.as_str())
                            .collect::<Vec<_>>(),
                    ) else {
                        return false;
                    };
                    if action.id.selection_behavior()
                        == crate::tui::commands::CommandSelectionBehavior::CompleteArguments
                        && action.args.trim().is_empty()
                    {
                        let name = crate::tui::commands::COMMANDS
                            .iter()
                            .find(|spec| spec.id == action.id)
                            .map_or("command", |spec| spec.name);
                        state.status_message = Some(format!("/{} needs an argument", name));
                        return true;
                    }
                    let busy = state.turn_lifecycle.is_active() || state.review_in_flight;
                    state.take_composer_text();
                    queue_command(state, action, busy);
                    true
                }
                // Normal typing
                (KeyModifiers::NONE, KeyCode::Char(c)) => {
                    // @ trigger for file picker
                    if c == '@' && should_open_file_picker(&state.input, state.input_cursor) {
                        insert_char(state, '@');
                        open_inline_file_picker(state, true);
                        return true;
                    }
                    insert_char(state, c);
                    refresh_inline_completion(state);
                    update_ghost(state);
                    true
                }
                (KeyModifiers::SHIFT, KeyCode::Char(c)) => {
                    insert_char(state, c.to_ascii_uppercase());
                    refresh_inline_completion(state);
                    update_ghost(state);
                    true
                }
                (KeyModifiers::NONE, KeyCode::Backspace) => {
                    delete_previous_char(state);
                    refresh_inline_completion(state);
                    true
                }
                (KeyModifiers::ALT, KeyCode::Backspace) => {
                    delete_previous_word(state);
                    refresh_inline_completion(state);
                    true
                }
                (modifiers, KeyCode::Backspace) if is_command_modifier(modifiers) => {
                    delete_to_hard_line_start(state);
                    refresh_inline_completion(state);
                    true
                }
                (KeyModifiers::NONE, KeyCode::Delete) => {
                    delete_current_char(state);
                    refresh_inline_completion(state);
                    true
                }
                (KeyModifiers::CONTROL, KeyCode::Char('w')) => {
                    delete_previous_word(state);
                    true
                }
                (KeyModifiers::CONTROL, KeyCode::Char('u')) => {
                    state.input.clear();
                    state.input_cursor = 0;
                    state.composer_ui.clear_for_new_draft();
                    state.composer_attachments.clear();
                    true
                }
                (KeyModifiers::CONTROL, KeyCode::Char('v')) => {
                    if state.image_attachment_busy {
                        state.status_message = Some("Clipboard image is still processing".into());
                        return true;
                    }
                    // Try to get image from clipboard first
                    if let Ok(mut clipboard) = arboard::Clipboard::new() {
                        if let Ok(img) = clipboard.get_image() {
                            if state.pasted_images.len()
                                >= crate::image_content::MAX_IMAGES_PER_REQUEST
                            {
                                state.status_message = Some(format!(
                                    "At most {} images can be attached",
                                    crate::image_content::MAX_IMAGES_PER_REQUEST
                                ));
                                return true;
                            }
                            match crate::image_content::RawClipboardImage::new(
                                img.width as u32,
                                img.height as u32,
                                img.bytes.into_owned(),
                            ) {
                                Ok(image) => {
                                    state.image_attachment_generation =
                                        state.image_attachment_generation.wrapping_add(1);
                                    state.image_attachment_busy = true;
                                    state.status_message =
                                        Some("Processing clipboard image…".into());
                                    state.pending_input_action = Some(
                                        crate::tui::commands::InputAction::AttachClipboardImage {
                                            image,
                                            generation: state.image_attachment_generation,
                                        },
                                    );
                                }
                                Err(error) => {
                                    state.status_message =
                                        Some(format!("Could not attach image: {error}"));
                                }
                            }
                            return true;
                        }
                        // Fall back to text paste
                        if let Ok(text) = clipboard.get_text() {
                            for ch in text.chars() {
                                if ch == '\n' || ch == '\r' {
                                    insert_char(state, '\n');
                                } else if !ch.is_control() {
                                    insert_char(state, ch);
                                } else if ch == '\t' {
                                    insert_char(state, ' ');
                                }
                            }
                            return true;
                        }
                    }
                    true
                }
                (KeyModifiers::NONE, KeyCode::Up) => composer_move_vertical(state, -1),
                (KeyModifiers::NONE, KeyCode::Down) => composer_move_vertical(state, 1),
                (KeyModifiers::ALT, KeyCode::Left) => {
                    move_to_previous_word(state);
                    true
                }
                (KeyModifiers::ALT, KeyCode::Right) => {
                    move_to_next_word_end(state);
                    true
                }
                (modifiers, KeyCode::Left) if is_command_modifier(modifiers) => {
                    move_to_hard_line_start(state);
                    true
                }
                (modifiers, KeyCode::Right) if is_command_modifier(modifiers) => {
                    move_to_hard_line_end(state);
                    true
                }
                (KeyModifiers::NONE, KeyCode::Left) => {
                    if state.input_cursor > 0 {
                        state.input_cursor = state
                            .composer_ui
                            .previous_boundary(&state.input, state.input_cursor);
                    }
                    state.composer_ui.preferred_column = None;
                    true
                }
                (KeyModifiers::NONE, KeyCode::Right) => {
                    if state.input_cursor < state.input.len() {
                        state.input_cursor = state
                            .composer_ui
                            .next_boundary(&state.input, state.input_cursor);
                    }
                    state.composer_ui.preferred_column = None;
                    true
                }
                (KeyModifiers::NONE, KeyCode::Home) => {
                    state.input_cursor =
                        crate::tui::composer::hard_line_bounds(&state.input, state.input_cursor)
                            .start;
                    state.composer_ui.preferred_column = None;
                    true
                }
                (KeyModifiers::CONTROL, KeyCode::Char('a')) => {
                    state.input_cursor = 0;
                    state.composer_ui.preferred_column = None;
                    true
                }
                (KeyModifiers::NONE, KeyCode::End) => {
                    state.input_cursor =
                        crate::tui::composer::hard_line_bounds(&state.input, state.input_cursor)
                            .end;
                    state.composer_ui.preferred_column = None;
                    true
                }
                (KeyModifiers::CONTROL, KeyCode::Char('e')) => {
                    state.input_cursor = state.input.len();
                    state.composer_ui.preferred_column = None;
                    true
                }
                (KeyModifiers::NONE, KeyCode::Esc) => {
                    if state.vim_mode && state.vim_state == crate::app::VimMode::Insert {
                        state.vim_state = crate::app::VimMode::Normal;
                        state.status_message = Some("-- NORMAL --".into());
                        true
                    } else {
                        state.model_picker_open = false;
                        state.slash_palette_open = false;
                        state.status_message = None;
                        true
                    }
                }
                (KeyModifiers::NONE, KeyCode::Tab) => {
                    // Accept ghost text if present
                    if let Some(ref ghost) = state.ghost_text.clone() {
                        state
                            .composer_ui
                            .apply_edit(state.input_cursor..state.input_cursor, ghost.len());
                        state.input.push_str(ghost);
                        state.input_cursor = state.input.len();
                        state.ghost_text = None;
                        state.status_message = Some("Accepted".into());
                        true
                    } else {
                        // Try path completion
                        if let Some((suffix, matches)) =
                            complete_path(&state.input, state.input_cursor, cwd)
                        {
                            if !suffix.is_empty() {
                                state.composer_ui.apply_edit(
                                    state.input_cursor..state.input_cursor,
                                    suffix.len(),
                                );
                                state.input.insert_str(state.input_cursor, &suffix);
                                state.input_cursor += suffix.len();
                            }
                            if matches.len() > 1 {
                                state.status_message =
                                    Some(format!("Matches: {}", matches.join(", ")));
                            } else {
                                state.status_message = Some("Completed".into());
                            }
                            true
                        } else {
                            // Fall back to mode cycling
                            if state.plan_proposal.is_some() {
                                crate::plan::dismiss(state);
                            }
                            state.mode = match state.mode {
                                AgentMode::Agent => AgentMode::Plan,
                                AgentMode::Plan => AgentMode::Ask,
                                AgentMode::Ask => AgentMode::Debug,
                                AgentMode::Debug | AgentMode::Custom(_) => AgentMode::Agent,
                            };
                            state.status_message = Some(match state.mode {
                                AgentMode::Plan => {
                                    "Plan mode · read-only tools · propose before editing".into()
                                }
                                AgentMode::Ask => "Ask mode · answer without editing".into(),
                                AgentMode::Debug => "Debug mode".into(),
                                AgentMode::Agent | AgentMode::Custom(_) => "Agent mode".into(),
                            });
                            true
                        }
                    }
                }
                (KeyModifiers::NONE, KeyCode::PageUp) => {
                    let page = state
                        .viewport_height
                        .saturating_sub(state.input_height)
                        .saturating_sub(2)
                        .max(1);
                    state.transcript_scroll.page_up(page);
                    if let Some(history) = state
                        .session_history
                        .as_mut()
                        .filter(|history| !history.is_complete() && !history.busy)
                    {
                        history.request_older = true;
                        history.error = None;
                    }
                    true
                }
                (KeyModifiers::NONE, KeyCode::PageDown) => {
                    let page = state
                        .viewport_height
                        .saturating_sub(state.input_height)
                        .saturating_sub(2)
                        .max(1);
                    state.transcript_scroll.page_down(page);
                    true
                }
                _ => false,
            }
        }
        Event::Resize(w, h) => {
            state.viewport_width = *w;
            state.viewport_height = *h;
            true
        }
        Event::Paste(text) => {
            if state.voice.is_some() {
                return true;
            }
            if state.file_picker_open {
                let pasted = crate::terminal_text::sanitize_terminal_text(text);
                for character in pasted.chars().filter(|character| !character.is_control()) {
                    if state.file_picker_filter.len() + character.len_utf8()
                        > crate::attachments::MAX_PICKER_QUERY_BYTES
                    {
                        break;
                    }
                    insert_char(state, character);
                }
                sync_file_picker_filter(state);
                state.selected_file_idx = 0;
                state.file_picker_generation = state.file_picker_generation.wrapping_add(1);
                state.file_picker_phase = crate::attachments::PickerPhase::Matching;
                state.pending_input_action =
                    Some(crate::tui::commands::InputAction::RefreshFilePicker);
                return true;
            }
            if state.workflow_preview.is_some()
                || state.workflow_clear_confirmation.is_some()
                || state.worktree_cleanup_confirmation.is_some()
                || state.worktree_setup_confirmation.is_some()
                || state.support_bundle_confirmation.is_some()
                || state.setup_import_confirmation.is_some()
            {
                return true;
            }
            if let Some(manager) = state.multi_run_manager.as_mut() {
                if manager.editing && !manager.busy && manager.confirmation.is_none() {
                    for character in text.chars().filter(|character| !character.is_control()) {
                        manager.draft_request.insert(manager.cursor, character);
                        manager.cursor += character.len_utf8();
                    }
                    manager.error = None;
                }
                return true;
            }
            if state.session_portability.is_some() {
                return true;
            }
            if let Some(retention) = state.session_retention.as_mut() {
                if retention.busy.is_none()
                    && retention.mode == crate::app::SessionRetentionMode::Edit
                {
                    let had_invalid = text
                        .chars()
                        .any(|character| !character.is_ascii_digit() && !character.is_control());
                    for character in text.chars().filter(char::is_ascii_digit) {
                        insert_retention_char(retention, character);
                    }
                    retention.error = had_invalid.then(|| "Use decimal digits only".into());
                }
                return true;
            }
            if let Some(btw) = state.btw.as_mut() {
                if btw.status == crate::app::BtwStatus::Editing {
                    for character in text.chars().filter(|character| !character.is_control()) {
                        btw.question.insert(btw.cursor, character);
                        btw.cursor += character.len_utf8();
                    }
                }
                return true;
            }
            if let Some(hub) = state.keymap_hub.as_mut() {
                if hub.capture.is_none() && !hub.saving {
                    hub.filter
                        .extend(text.chars().filter(|character| !character.is_control()));
                    hub.selected_idx = 0;
                    hub.error = None;
                }
                return true;
            }
            if let Some(history) = state.history_search.as_mut() {
                history
                    .filter
                    .extend(text.chars().filter(|character| !character.is_control()));
                history.selected_idx = 0;
                return true;
            }
            if let Some(manager) = state.hook_manager.as_mut() {
                if !manager.busy {
                    manager
                        .filter
                        .extend(text.chars().filter(|character| !character.is_control()));
                    manager.selected_idx = 0;
                    manager.error = None;
                }
                return true;
            }
            if let Some(manager) = state.permission_manager.as_mut() {
                if !manager.busy && manager.confirmation.is_none() {
                    manager
                        .filter
                        .extend(text.chars().filter(|character| !character.is_control()));
                    manager.selected_idx = 0;
                    manager.error = None;
                }
                return true;
            }
            if let Some(manager) = state.task_graph_manager.as_mut() {
                if manager.confirmation.is_none() && !state.agent_graph_busy {
                    manager
                        .filter
                        .extend(text.chars().filter(|character| !character.is_control()));
                    manager.selected_idx = 0;
                    manager.error = None;
                }
                return true;
            }
            if let Some(editor) = state.context_limit_editor.as_mut() {
                if editor.saving {
                    return true;
                }
                let had_invalid = text
                    .chars()
                    .any(|character| !character.is_ascii_digit() && !character.is_control());
                for character in text.chars().filter(char::is_ascii_digit) {
                    insert_context_limit_char(editor, character);
                }
                editor.error = had_invalid.then(|| "Use decimal digits only".into());
                return true;
            }
            if let Some(manager) = state.theme_manager.as_mut() {
                if !manager.busy {
                    manager
                        .filter
                        .extend(text.chars().filter(|character| !character.is_control()));
                    manager.selected_idx = 0;
                    manager.error = None;
                }
                return true;
            }
            if state.notification_picker.is_some() {
                return true;
            }
            if state.reasoning_picker.is_some()
                || state.route_preset_picker.is_some()
                || state.route_preset_overwrite.is_some()
            {
                return true;
            }
            if state.statusline_manager.is_some() {
                return true;
            }
            if state.sandbox_hub.is_some() {
                return true;
            }
            if let Some(hub) = state.config_hub.as_mut() {
                hub.filter
                    .extend(text.chars().filter(|character| !character.is_control()));
                hub.selected_idx = 0;
                return true;
            }
            if let Some(editor) = state.provider_editor.as_mut() {
                if editor.saving {
                    return true;
                }
                editor.error = None;
                for character in text.chars().filter(|character| !character.is_control()) {
                    insert_provider_editor_char(editor, character);
                }
                return true;
            }
            if let Some(auth) = state.provider_auth.as_mut() {
                if auth.stage == crate::app::ProviderAuthStage::SecretInput {
                    for character in text.chars().filter(|character| !character.is_control()) {
                        if auth.input.len() >= 96 * 1024 {
                            break;
                        }
                        auth.cursor = auth.input.insert_char(auth.cursor, character);
                    }
                    auth.error = None;
                }
                return true;
            }
            if state.provider_discovery.is_some() {
                return true;
            }
            if let Some(manager) = state.provider_manager.as_mut() {
                if manager.busy || manager.delete_confirmation.is_some() {
                    return true;
                }
                manager.error = None;
                let filter = match manager.view {
                    crate::app::ProviderManagerView::Providers
                    | crate::app::ProviderManagerView::SetupProviders => {
                        &mut manager.provider_filter
                    }
                    crate::app::ProviderManagerView::Models { .. } => &mut manager.model_filter,
                    crate::app::ProviderManagerView::Templates { .. } => {
                        &mut manager.template_filter
                    }
                    crate::app::ProviderManagerView::SetupProvider { .. }
                    | crate::app::ProviderManagerView::SetupFinish => return true,
                };
                filter.extend(text.chars().filter(|character| !character.is_control()));
                match manager.view {
                    crate::app::ProviderManagerView::Providers
                    | crate::app::ProviderManagerView::SetupProviders => {
                        manager.selected_provider_idx = 0
                    }
                    crate::app::ProviderManagerView::Models { .. } => {
                        manager.selected_model_idx = 0
                    }
                    crate::app::ProviderManagerView::Templates { .. } => {
                        manager.selected_template_idx = 0
                    }
                    crate::app::ProviderManagerView::SetupProvider { .. }
                    | crate::app::ProviderManagerView::SetupFinish => {}
                }
                return true;
            }
            if state.model_picker_open {
                state
                    .model_filter
                    .extend(text.chars().filter(|character| !character.is_control()));
                state.selected_model_idx = 0;
                return true;
            }
            if let Some(manager) = state.terminal_manager.as_mut() {
                if manager.busy || manager.kill_confirmation.is_some() {
                    return true;
                }
                match &mut manager.mode {
                    crate::app::TerminalManagerMode::List => {
                        manager
                            .filter
                            .extend(text.chars().filter(|character| !character.is_control()));
                        manager.selected_idx = 0;
                    }
                    crate::app::TerminalManagerMode::Create { name, cursor } => {
                        for character in text.chars().filter(|character| !character.is_control()) {
                            name.insert(*cursor, character);
                            *cursor += character.len_utf8();
                        }
                    }
                }
                manager.error = None;
                return true;
            }
            if state.session_picker_open {
                if let Some(rename) = state.session_rename.as_mut() {
                    if !rename.busy {
                        insert_session_rename_text(rename, text);
                    }
                } else if state.session_delete_confirmation.is_none()
                    && state.session_archive_confirmation.is_none()
                    && !state.session_picker_busy
                {
                    state
                        .session_filter
                        .extend(text.chars().filter(|character| !character.is_control()));
                    state.selected_session_idx = 0;
                    state.session_picker_error = None;
                }
                return true;
            }
            if text.chars().count() > crate::tui::composer::LARGE_PASTE_THRESHOLD {
                insert_large_paste(state, text);
            } else {
                let start = state.input_cursor;
                for ch in text.chars() {
                    if ch == '\n' || ch == '\r' {
                        insert_char(state, '\n');
                    } else if !ch.is_control() {
                        insert_char(state, ch);
                    } else if ch == '\t' {
                        insert_char(state, ' ');
                    }
                }
                register_pasted_path(state, start, state.input_cursor, cwd);
            }
            true
        }
        Event::Tick | Event::Redraw => {
            if state.pending_key_chord.as_ref().is_some_and(|pending| {
                pending.started_at.elapsed() >= std::time::Duration::from_millis(1500)
            }) {
                state.pending_key_chord = None;
                state.status_message = Some("Key chord timed out".into());
                true
            } else {
                false
            }
        }
        Event::MouseScroll(_) => unreachable!("mouse scroll is routed before key handling"),
        Event::MouseSelection(_) => {
            unreachable!("mouse selection is routed before key handling")
        }
        Event::Focus(_) | Event::ThemeDetected(_) => {
            unreachable!("terminal-awareness events are routed by the TUI loop")
        }
    }
}

fn shortcut_help(state: &crate::app::AppState) -> String {
    let mut lines = vec![
        "Keyboard shortcuts".to_string(),
        String::new(),
        "Enter                 send / interrupt and send while working".into(),
        "Shift+Enter           insert newline".into(),
        "Up / Down             move visual rows, then browse history".into(),
        "Home / End            current hard-line bounds".into(),
        "Ctrl+A / Ctrl+E       whole draft bounds".into(),
        "PageUp / PageDown     transcript page".into(),
        "Ctrl+Home / Ctrl+End  oldest / latest transcript".into(),
        "/ · @ · path          inline completion".into(),
        "Esc                   stop current turn / dismiss active popup".into(),
        "Ctrl+C                stop current turn, even with a popup open".into(),
        String::new(),
        "Configured actions".into(),
    ];
    lines.extend(crate::keymap::KeyAction::ALL.into_iter().map(|action| {
        format!(
            "{:<24} {}",
            state.keymap.display_binding(action),
            action.label()
        )
    }));
    lines.push(String::new());
    lines.push(
        "Mouse: wheel/trackpad scrolls; left-drag selects visible transcript text and copies it on release."
            .into(),
    );
    lines.join("\n")
}

pub(crate) fn copy_text_to_clipboard(text: &str) -> Result<(), ()> {
    let mut clipboard = arboard::Clipboard::new().map_err(|_| ())?;
    clipboard.set_text(text.to_owned()).map_err(|_| ())
}

fn handle_mouse_selection<F>(
    state: &mut crate::app::AppState,
    event: MouseSelectionEvent,
    copy: F,
) -> bool
where
    F: FnOnce(&str) -> Result<(), ()>,
{
    let selection = if state.pager_open {
        &mut state.pager_selection
    } else {
        &mut state.transcript_scroll
    };
    match event.phase {
        MouseSelectionPhase::Start => {
            selection.begin_selection(event.column, event.row);
        }
        MouseSelectionPhase::Drag => {
            selection.update_selection(event.column, event.row);
        }
        MouseSelectionPhase::End => {
            let clicked_activity = if state.pager_open {
                None
            } else {
                selection.activity_key_for_click_release(event.column, event.row)
            };
            if let Some(text) = selection.finish_selection(event.column, event.row) {
                let characters = text.chars().count();
                state.status_message = Some(if copy(&text).is_ok() {
                    format!("Copied selection · {characters} chars")
                } else {
                    "Clipboard unavailable; selection retained".into()
                });
            } else if selection.selection_limit_exceeded() {
                state.status_message = Some(
                    "Selection exceeds the 200,000-cell limit; use /copy all for the complete transcript"
                        .into(),
                );
            } else if let Some(key) = clicked_activity {
                toggle_expanded_block(state, key, "activity block");
            }
        }
    }
    true
}

fn toggle_expanded_block(
    state: &mut crate::app::AppState,
    key: crate::app::TranscriptBlockKey,
    label: &str,
) {
    if let Some(position) = state
        .expanded_blocks
        .iter()
        .position(|expanded| expanded == &key)
    {
        state.expanded_blocks.remove(position);
        state.status_message = Some(format!("Collapsed {label}"));
    } else {
        state.expanded_blocks.push(key);
        state.status_message = Some(format!("Expanded {label}"));
    }
}

fn handle_mouse_scroll(
    state: &mut crate::app::AppState,
    direction: crate::tui::event::ScrollDirection,
    cwd: &str,
    rows: u16,
) -> bool {
    let key = match direction {
        crate::tui::event::ScrollDirection::Up => KeyCode::Up,
        crate::tui::event::ScrollDirection::Down => KeyCode::Down,
    };
    if state.slash_palette_open {
        return handle_slash_palette(state, key, KeyModifiers::NONE);
    }
    if state.file_picker_open {
        return handle_file_picker(state, key, KeyModifiers::NONE, cwd);
    }
    if state.composer_ui.path_completion.open {
        return handle_path_completion(state, key, KeyModifiers::NONE);
    }
    if state.model_picker_open {
        return handle_model_picker(state, key, KeyModifiers::NONE);
    }
    if state.reasoning_picker.is_some() {
        return handle_reasoning_picker(state, key);
    }
    if state.session_picker_open {
        return handle_session_picker(state, key, KeyModifiers::NONE);
    }
    if state.provider_manager.is_some() {
        return handle_provider_manager(state, key, KeyModifiers::NONE);
    }
    if state.permission_manager.is_some() {
        return handle_permission_manager(state, key, KeyModifiers::NONE);
    }
    if state.pager_open {
        return handle_pager(state, key);
    }
    match direction {
        crate::tui::event::ScrollDirection::Up => state.transcript_scroll.page_up(rows),
        crate::tui::event::ScrollDirection::Down => state.transcript_scroll.page_down(rows),
    }
    true
}

fn handle_loop_guard_interaction(state: &mut crate::app::AppState, code: KeyCode) -> bool {
    let Some(snapshot) = state.loop_guard_interaction.as_ref().cloned() else {
        return false;
    };
    if snapshot.cancel_confirmation {
        match code {
            KeyCode::Esc => {
                if let Some(interaction) = state.loop_guard_interaction.as_mut() {
                    interaction.cancel_confirmation = false;
                    interaction.cancel_confirm_selected = false;
                }
            }
            KeyCode::Left | KeyCode::Right | KeyCode::Tab | KeyCode::BackTab => {
                if let Some(interaction) = state.loop_guard_interaction.as_mut() {
                    interaction.cancel_confirm_selected = !interaction.cancel_confirm_selected;
                }
            }
            KeyCode::Enter if snapshot.cancel_confirm_selected => {
                state.loop_guard_interaction = None;
                state.pending_input_action = Some(crate::tui::commands::InputAction::StopTurn);
            }
            KeyCode::Enter => {
                if let Some(interaction) = state.loop_guard_interaction.as_mut() {
                    interaction.cancel_confirmation = false;
                    interaction.cancel_confirm_selected = false;
                }
            }
            _ => {}
        }
        return true;
    }

    match code {
        KeyCode::Left | KeyCode::Up | KeyCode::BackTab => {
            if let Some(interaction) = state.loop_guard_interaction.as_mut() {
                interaction.selected = interaction.selected.previous();
            }
        }
        KeyCode::Right | KeyCode::Down | KeyCode::Tab => {
            if let Some(interaction) = state.loop_guard_interaction.as_mut() {
                interaction.selected = interaction.selected.next();
            }
        }
        KeyCode::Char('r') => {
            resolve_loop_guard_decision(state, crate::loop_guard::LoopGuardDecision::Replan)
        }
        KeyCode::Char('o') => {
            resolve_loop_guard_decision(state, crate::loop_guard::LoopGuardDecision::Continue)
        }
        KeyCode::Char('c') => {
            resolve_loop_guard_decision(state, crate::loop_guard::LoopGuardDecision::Cancel)
        }
        KeyCode::Enter => resolve_loop_guard_decision(state, snapshot.selected),
        KeyCode::Esc => {
            state.status_message =
                Some("Low-yield loop remains paused; choose Replan, Continue, or Cancel".into());
        }
        _ => {}
    }
    true
}

fn resolve_loop_guard_decision(
    state: &mut crate::app::AppState,
    decision: crate::loop_guard::LoopGuardDecision,
) {
    match decision {
        crate::loop_guard::LoopGuardDecision::Replan => {
            state.loop_guard.replan();
            state.loop_guard_replan_prompt = Some(
                "A low-yield loop was detected. Replan now and use a materially different approach; do not repeat the same edit or command sequence."
                    .into(),
            );
            state.loop_guard_interaction = None;
            let _ = state.soft_pause.request_resume();
            state.pending_input_action = Some(crate::tui::commands::InputAction::ResumeLoopGuard);
            state.status_message = Some("Replanning with a different approach…".into());
        }
        crate::loop_guard::LoopGuardDecision::Continue => {
            state.loop_guard.suppress_for_turn();
            state.loop_guard_interaction = None;
            let _ = state.soft_pause.request_resume();
            state.pending_input_action = Some(crate::tui::commands::InputAction::ResumeLoopGuard);
            state.status_message = Some("Loop guard suppressed for this turn".into());
        }
        crate::loop_guard::LoopGuardDecision::Cancel => {
            if let Some(interaction) = state.loop_guard_interaction.as_mut() {
                interaction.cancel_confirmation = true;
                interaction.cancel_confirm_selected = false;
            }
        }
    }
}

fn handle_voice(state: &mut crate::app::AppState, code: KeyCode, modifiers: KeyModifiers) -> bool {
    let Some(voice) = state.voice.as_mut() else {
        return false;
    };
    match (voice.phase, code) {
        (
            crate::voice::VoicePhase::Ready | crate::voice::VoicePhase::Failed,
            KeyCode::Char(' '),
        ) => {
            voice.phase = crate::voice::VoicePhase::Recording;
            voice.error = None;
            state.pending_input_action = Some(crate::tui::commands::InputAction::StartVoice);
        }
        (crate::voice::VoicePhase::Recording, KeyCode::Char(' ') | KeyCode::Enter) => {
            state.pending_input_action = Some(crate::tui::commands::InputAction::StopVoice);
        }
        (_, KeyCode::Esc) => {
            state.voice_generation = state.voice_generation.wrapping_add(1);
            state.pending_input_action = Some(crate::tui::commands::InputAction::CancelVoice);
            state.voice = None;
        }
        (_, KeyCode::Char('c')) if modifiers == KeyModifiers::CONTROL => {
            state.voice_generation = state.voice_generation.wrapping_add(1);
            state.pending_input_action = Some(crate::tui::commands::InputAction::CancelVoice);
            state.voice = None;
        }
        _ => {}
    }
    true
}

#[cfg(test)]
mod lazy_session_history_input_tests {
    use super::*;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::style::Style;
    use std::cell::RefCell;

    fn selection_event(
        phase: crate::tui::event::MouseSelectionPhase,
        column: u16,
        row: u16,
    ) -> crate::tui::event::MouseSelectionEvent {
        crate::tui::event::MouseSelectionEvent { phase, column, row }
    }

    #[test]
    fn mouse_drag_copies_visible_text_and_retains_selection_when_clipboard_fails() {
        let mut buffer = Buffer::empty(Rect::new(0, 1, 12, 1));
        buffer.set_string(0, 1, "select me", Style::default());
        let mut state = crate::app::AppState::new(crate::context::ShellHistory::in_memory());
        state
            .transcript_scroll
            .capture_selectable_frame(&buffer, Rect::new(0, 1, 12, 1));

        assert!(handle_mouse_selection(
            &mut state,
            selection_event(MouseSelectionPhase::Start, 0, 1),
            |_| Ok(()),
        ));
        assert!(handle_mouse_selection(
            &mut state,
            selection_event(MouseSelectionPhase::Drag, 5, 1),
            |_| Ok(()),
        ));
        let copied = RefCell::new(None);
        assert!(handle_mouse_selection(
            &mut state,
            selection_event(MouseSelectionPhase::End, 5, 1),
            |text| {
                *copied.borrow_mut() = Some(text.to_string());
                Ok(())
            },
        ));
        assert_eq!(copied.into_inner().as_deref(), Some("select"));
        assert_eq!(
            state.status_message.as_deref(),
            Some("Copied selection · 6 chars")
        );
        assert!(state.transcript_scroll.has_selection());

        assert!(handle_mouse_selection(
            &mut state,
            selection_event(MouseSelectionPhase::Start, 7, 1),
            |_| Ok(()),
        ));
        assert!(handle_mouse_selection(
            &mut state,
            selection_event(MouseSelectionPhase::End, 8, 1),
            |_| Err(()),
        ));
        assert_eq!(
            state.status_message.as_deref(),
            Some("Clipboard unavailable; selection retained")
        );
        assert!(state.transcript_scroll.has_selection());
    }

    #[test]
    fn pager_drag_targets_visible_pager_not_hidden_conversation() {
        let mut conversation = Buffer::empty(Rect::new(0, 1, 16, 2));
        conversation.set_string(0, 1, "hidden transcript", Style::default());
        let mut pager = Buffer::empty(Rect::new(0, 1, 16, 2));
        pager.set_string(0, 1, "pager evidence", Style::default());
        pager.set_string(0, 2, "界 second", Style::default());
        let mut state = crate::app::AppState::new(crate::context::ShellHistory::in_memory());
        state
            .transcript_scroll
            .capture_selectable_frame(&conversation, Rect::new(0, 1, 16, 2));
        state
            .pager_selection
            .capture_selectable_frame(&pager, Rect::new(0, 1, 16, 2));
        state.pager_open = true;

        assert!(handle_mouse_selection(
            &mut state,
            selection_event(MouseSelectionPhase::Start, 0, 1),
            |_| Ok(()),
        ));
        assert!(handle_mouse_selection(
            &mut state,
            selection_event(MouseSelectionPhase::Drag, 1, 2),
            |_| Ok(()),
        ));
        let copied = RefCell::new(None);
        assert!(handle_mouse_selection(
            &mut state,
            selection_event(MouseSelectionPhase::End, 1, 2),
            |text| {
                *copied.borrow_mut() = Some(text.to_string());
                Ok(())
            },
        ));

        assert_eq!(copied.into_inner().as_deref(), Some("pager evidence\n界"));
        assert!(state.pager_selection.has_selection());
        assert!(!state.transcript_scroll.has_selection());

        assert!(handle_mouse_selection(
            &mut state,
            selection_event(MouseSelectionPhase::Start, 1, 2),
            |_| Ok(()),
        ));
        assert!(handle_mouse_selection(
            &mut state,
            selection_event(MouseSelectionPhase::Drag, 0, 1),
            |_| Ok(()),
        ));
        let reverse = RefCell::new(None);
        assert!(handle_mouse_selection(
            &mut state,
            selection_event(MouseSelectionPhase::End, 0, 1),
            |text| {
                *reverse.borrow_mut() = Some(text.to_string());
                Ok(())
            },
        ));
        assert_eq!(reverse.into_inner().as_deref(), Some("pager evidence\n界"));
    }

    #[test]
    fn pager_copy_all_includes_offscreen_content_and_reports_failure() {
        let mut state = crate::app::AppState::new(crate::context::ShellHistory::in_memory());
        state.pager_content = "visible\nfar below\n界".into();
        state.pager_scroll = 2;
        let mut pager = Buffer::empty(Rect::new(0, 0, 16, 1));
        pager.set_string(0, 0, "visible", Style::default());
        state
            .pager_selection
            .capture_selectable_frame(&pager, Rect::new(0, 0, 16, 1));
        state.pager_selection.begin_selection(0, 0);
        assert!(state.pager_selection.finish_selection(6, 0).is_some());
        let copied = RefCell::new(None);

        copy_pager_content(&mut state, |text| {
            *copied.borrow_mut() = Some(text.to_string());
            Ok(())
        });
        assert_eq!(
            copied.into_inner().as_deref(),
            Some("visible\nfar below\n界")
        );
        assert!(state.status_message.is_none());
        assert!(!state.pager_selection.has_selection());

        state.pager_selection.begin_selection(0, 0);
        assert!(state.pager_selection.finish_selection(6, 0).is_some());
        copy_pager_content(&mut state, |_| Err(()));
        assert_eq!(
            state.status_message.as_deref(),
            Some("Clipboard unavailable; page was not copied")
        );
        assert!(state.pager_selection.has_selection());
    }

    #[test]
    fn learning_pager_enter_and_refresh_emit_contextual_actions() {
        let mut state = crate::app::AppState::new(crate::context::ShellHistory::in_memory());
        state.pager_open = true;
        state.learning_review_pager_open = true;

        assert!(handle_pager(&mut state, KeyCode::Enter));
        assert_eq!(
            state.pending_input_action,
            Some(crate::tui::commands::InputAction::OpenLearningReviewActions)
        );
        state.pending_input_action = None;
        assert!(handle_pager(&mut state, KeyCode::Char('R')));
        assert_eq!(
            state.pending_input_action,
            Some(crate::tui::commands::InputAction::RefreshLearningReview)
        );
        assert!(handle_pager(&mut state, KeyCode::Esc));
        assert!(!state.pager_open);
        assert!(!state.learning_review_pager_open);
    }

    #[test]
    fn page_up_requests_one_older_page_without_touching_the_composer() {
        let mut state = crate::app::AppState::new(crate::context::ShellHistory::in_memory());
        state.input = "draft remains".into();
        state.input_cursor = state.input.len();
        state.session_history = Some(crate::app::SessionHistoryState {
            storage_key: "saved".into(),
            session_id: uuid::Uuid::nil(),
            storage_generation: uuid::Uuid::nil(),
            start_index: 64,
            total_messages: 128,
            busy: false,
            request_older: false,
            request_full: false,
            resume_submit: false,
            generation: 1,
            error: Some("old error".into()),
        });

        state.transcript_scroll.observe_rendered_maximum(20);
        assert!(handle_event(
            &mut state,
            &Event::Key(KeyCode::PageUp, KeyModifiers::NONE),
            "/workspace"
        ));

        let history = state.session_history.unwrap();
        assert!(history.request_older);
        assert!(history.error.is_none());
        assert_eq!(state.transcript_scroll.bottom_offset(), 1);
        assert!(!state.transcript_scroll.following());
        assert_eq!(state.input, "draft remains");
    }

    #[test]
    fn mouse_wheel_routes_to_popup_before_transcript() {
        let mut state = crate::app::AppState::new(crate::context::ShellHistory::in_memory());
        let mut buffer = Buffer::empty(Rect::new(0, 1, 8, 1));
        buffer.set_string(0, 1, "selected", Style::default());
        state
            .transcript_scroll
            .capture_selectable_frame(&buffer, Rect::new(0, 1, 8, 1));
        assert!(state.transcript_scroll.begin_selection(0, 1));
        assert!(state.transcript_scroll.update_selection(3, 1));
        assert!(state.transcript_scroll.has_selection());
        state.transcript_scroll.observe_rendered_maximum(30);
        assert!(handle_event(
            &mut state,
            &Event::MouseScroll(crate::tui::event::MouseScrollEvent {
                direction: crate::tui::event::ScrollDirection::Up,
                column: 3,
                row: 1,
            }),
            "/workspace",
        ));
        assert_eq!(state.transcript_scroll.bottom_offset(), 1);
        assert!(state.transcript_scroll.has_selection());
        assert!(state.transcript_scroll.finish_selection(3, 1).is_some());

        state.input = "/".into();
        state.input_cursor = 1;
        state.slash_palette_open = true;
        state.slash_filter.clear();
        state.selected_slash_idx = 0;
        assert!(handle_event(
            &mut state,
            &Event::MouseScroll(crate::tui::event::MouseScrollEvent {
                direction: crate::tui::event::ScrollDirection::Down,
                column: 3,
                row: 1,
            }),
            "/workspace",
        ));
        assert_eq!(state.selected_slash_idx, 1);
        assert_eq!(state.transcript_scroll.bottom_offset(), 1);
        assert!(!state.transcript_scroll.has_selection());
    }

    #[test]
    fn copy_transcript_chord_remains_available_during_active_turn() {
        let mut state = crate::app::AppState::new(crate::context::ShellHistory::in_memory());
        state.turn_lifecycle.begin();
        assert!(handle_event(
            &mut state,
            &Event::Key(KeyCode::Char('x'), KeyModifiers::CONTROL),
            "/workspace",
        ));
        assert!(handle_event(
            &mut state,
            &Event::Key(KeyCode::Char('c'), KeyModifiers::NONE),
            "/workspace",
        ));
        assert_eq!(
            state.pending_input_action,
            Some(crate::tui::commands::InputAction::Command(
                crate::tui::commands::CommandAction {
                    id: crate::tui::commands::CommandId::Copy,
                    args: "all".into(),
                }
            ))
        );
    }
}

fn handle_workflow_preview(state: &mut crate::app::AppState, code: KeyCode) -> bool {
    match code {
        KeyCode::Esc => {
            state.workflow_preview = None;
            state.status_message = Some("Workflow cancelled".into());
        }
        KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down | KeyCode::Tab => {
            if let Some(preview) = state.workflow_preview.as_mut() {
                preview.run_selected = !preview.run_selected;
            }
        }
        KeyCode::Enter => {
            let Some(preview) = state.workflow_preview.take() else {
                return true;
            };
            if preview.run_selected {
                state.workflow_loaded = Some(preview.loaded);
                state.workflow_start_requested = true;
                state.status_message = Some(if preview.resume_existing {
                    "Resuming workflow…".into()
                } else {
                    "Starting workflow…".into()
                });
            } else {
                state.status_message = Some("Workflow cancelled".into());
            }
        }
        _ => {}
    }
    true
}

fn handle_workflow_clear_confirmation(state: &mut crate::app::AppState, code: KeyCode) -> bool {
    match code {
        KeyCode::Esc => {
            state.workflow_clear_confirmation = None;
            state.status_message = Some("Workflow clear cancelled".into());
        }
        KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down | KeyCode::Tab => {
            if let Some(confirm) = state.workflow_clear_confirmation.as_mut() {
                *confirm = !*confirm;
            }
        }
        KeyCode::Enter => {
            let confirmed = state.workflow_clear_confirmation.take().unwrap_or(false);
            if confirmed {
                state.workflow_clear_requested = true;
                state.status_message = Some("Clearing workflow…".into());
            } else {
                state.status_message = Some("Workflow clear cancelled".into());
            }
        }
        _ => {}
    }
    true
}

fn handle_worktree_cleanup_confirmation(state: &mut crate::app::AppState, code: KeyCode) -> bool {
    let Some(confirmation) = state.worktree_cleanup_confirmation.as_mut() else {
        return true;
    };
    if confirmation.busy {
        return true;
    }
    match code {
        KeyCode::Esc => {
            state.worktree_cleanup_confirmation = None;
            state.status_message = Some("Worktree cleanup cancelled".into());
        }
        KeyCode::Left
        | KeyCode::Right
        | KeyCode::Up
        | KeyCode::Down
        | KeyCode::Tab
        | KeyCode::BackTab => {
            confirmation.remove_selected = !confirmation.remove_selected;
        }
        KeyCode::Enter if confirmation.remove_selected => {
            confirmation.busy = true;
            confirmation.error = None;
            state.worktree_busy = true;
            state.worktree_generation = state.worktree_generation.wrapping_add(1);
            state.worktree_request = Some(crate::worktree::WorktreeActionRequest {
                generation: state.worktree_generation,
                action: crate::worktree::WorktreeAction::ConfirmCleanup {
                    id: confirmation.id.to_string(),
                },
            });
            state.status_message = Some("Cleaning up worktree…".into());
        }
        KeyCode::Enter => {
            state.worktree_cleanup_confirmation = None;
            state.status_message = Some("Worktree cleanup cancelled".into());
        }
        _ => {}
    }
    true
}

fn handle_worktree_setup_confirmation(state: &mut crate::app::AppState, code: KeyCode) -> bool {
    let Some(confirmation) = state.worktree_setup_confirmation.as_mut() else {
        return true;
    };
    if confirmation.busy {
        return true;
    }
    match code {
        KeyCode::Esc => {
            state.worktree_setup_confirmation = None;
            state.status_message =
                Some("Worktree setup skipped; the created worktree was retained".into());
        }
        KeyCode::Left
        | KeyCode::Right
        | KeyCode::Up
        | KeyCode::Down
        | KeyCode::Tab
        | KeyCode::BackTab => {
            confirmation.run_selected = !confirmation.run_selected;
        }
        KeyCode::Enter if confirmation.run_selected => {
            confirmation.busy = true;
            confirmation.error = None;
            state.pending_input_action =
                Some(crate::tui::commands::InputAction::RunLaunchWorktreeSetup);
            state.status_message = Some("Running worktree setup…".into());
        }
        KeyCode::Enter => {
            state.worktree_setup_confirmation = None;
            state.status_message =
                Some("Worktree setup skipped; the created worktree was retained".into());
        }
        _ => {}
    }
    true
}

fn handle_support_bundle_confirmation(state: &mut crate::app::AppState, code: KeyCode) -> bool {
    let Some(confirmation) = state.support_bundle_confirmation.as_mut() else {
        return true;
    };
    if confirmation.busy {
        return true;
    }
    match code {
        KeyCode::Esc => {
            state.support_bundle_confirmation = None;
            state.status_message = Some("Support bundle cancelled; no archive was written".into());
        }
        KeyCode::Left
        | KeyCode::Right
        | KeyCode::Up
        | KeyCode::Down
        | KeyCode::Tab
        | KeyCode::BackTab => {
            confirmation.create_selected = !confirmation.create_selected;
        }
        KeyCode::Enter if confirmation.create_selected => {
            confirmation.busy = true;
            confirmation.error = None;
            state.pending_input_action =
                Some(crate::tui::commands::InputAction::CreateSupportBundle);
            state.status_message = Some("Creating support bundle…".into());
        }
        KeyCode::Enter => {
            state.support_bundle_confirmation = None;
            state.status_message = Some("Support bundle cancelled; no archive was written".into());
        }
        _ => {}
    }
    true
}

fn handle_setup_import_confirmation(state: &mut crate::app::AppState, code: KeyCode) -> bool {
    let Some(confirmation) = state.setup_import_confirmation.as_mut() else {
        return true;
    };
    if confirmation.busy {
        return true;
    }
    match code {
        KeyCode::Esc => {
            state.setup_import_confirmation = None;
            state.status_message =
                Some("Claude setup import cancelled; no files were written".into());
        }
        KeyCode::Left | KeyCode::Right | KeyCode::Tab | KeyCode::BackTab => {
            confirmation.apply_selected = !confirmation.apply_selected;
        }
        KeyCode::Up | KeyCode::PageUp => {
            confirmation.scroll = confirmation.scroll.saturating_sub(1);
        }
        KeyCode::Down | KeyCode::PageDown => {
            let maximum = confirmation.preview.files.len().saturating_sub(1);
            confirmation.scroll = confirmation.scroll.saturating_add(1).min(maximum);
        }
        KeyCode::Enter
            if confirmation.apply_selected && !confirmation.preview.collisions.is_empty() =>
        {
            confirmation.error = Some("resolve every target collision and preview again".into());
            confirmation.apply_selected = false;
        }
        KeyCode::Enter if confirmation.apply_selected => {
            confirmation.busy = true;
            confirmation.error = None;
            state.pending_input_action = Some(crate::tui::commands::InputAction::ApplySetupImport);
            state.status_message = Some("Applying exact Claude setup preview…".into());
        }
        KeyCode::Enter => {
            state.setup_import_confirmation = None;
            state.status_message =
                Some("Claude setup import cancelled; no files were written".into());
        }
        _ => {}
    }
    true
}

fn handle_task_graph_manager(
    state: &mut crate::app::AppState,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> bool {
    let confirmation = state
        .task_graph_manager
        .as_ref()
        .and_then(|manager| manager.confirmation.clone());
    if let Some(confirmation) = confirmation {
        match code {
            KeyCode::Esc => {
                if let Some(manager) = state.task_graph_manager.as_mut() {
                    manager.confirmation = None;
                    manager.notice = Some("Graph action cancelled".into());
                }
            }
            KeyCode::Left
            | KeyCode::Right
            | KeyCode::Up
            | KeyCode::Down
            | KeyCode::Tab
            | KeyCode::BackTab => {
                if let Some(manager) = state.task_graph_manager.as_mut() {
                    if let Some(confirmation) = manager.confirmation.as_mut() {
                        confirmation.confirm_selected = !confirmation.confirm_selected;
                    }
                }
            }
            KeyCode::Enter => {
                if confirmation.confirm_selected {
                    if state.agent_graph_busy {
                        if let Some(manager) = state.task_graph_manager.as_mut() {
                            manager.error = Some(
                                "Agent graph update in progress; wait for it to finish".into(),
                            );
                        }
                        return true;
                    }
                    state.pending_input_action = Some(match confirmation.action {
                        crate::app::TaskGraphConfirmationAction::Start => {
                            crate::tui::commands::InputAction::StartAgentGraph
                        }
                        crate::app::TaskGraphConfirmationAction::Cancel => {
                            crate::tui::commands::InputAction::CancelAgentGraph
                        }
                        crate::app::TaskGraphConfirmationAction::Retry(task_id) => {
                            crate::tui::commands::InputAction::RetryAgentTask(task_id)
                        }
                        crate::app::TaskGraphConfirmationAction::Review(task_id) => {
                            crate::tui::commands::InputAction::ReviewAgentTask(task_id)
                        }
                    });
                } else if let Some(manager) = state.task_graph_manager.as_mut() {
                    manager.confirmation = None;
                    manager.notice = Some("Graph action cancelled".into());
                }
            }
            _ => {}
        }
        return true;
    }

    let visible = visible_task_graph_ids(state);
    match code {
        KeyCode::Esc => state.task_graph_manager = None,
        KeyCode::Up => {
            if let Some(manager) = state.task_graph_manager.as_mut() {
                manager.selected_idx = manager.selected_idx.saturating_sub(1);
            }
        }
        KeyCode::Down => {
            if let Some(manager) = state.task_graph_manager.as_mut() {
                manager.selected_idx =
                    (manager.selected_idx + 1).min(visible.len().saturating_sub(1));
            }
        }
        KeyCode::Home => {
            if let Some(manager) = state.task_graph_manager.as_mut() {
                manager.selected_idx = 0;
            }
        }
        KeyCode::End => {
            if let Some(manager) = state.task_graph_manager.as_mut() {
                manager.selected_idx = visible.len().saturating_sub(1);
            }
        }
        KeyCode::PageUp => {
            if let Some(manager) = state.task_graph_manager.as_mut() {
                manager.selected_idx = manager.selected_idx.saturating_sub(8);
            }
        }
        KeyCode::PageDown => {
            if let Some(manager) = state.task_graph_manager.as_mut() {
                manager.selected_idx =
                    (manager.selected_idx + 8).min(visible.len().saturating_sub(1));
            }
        }
        KeyCode::Char('S' | 's') if modifiers.contains(KeyModifiers::SHIFT) => {
            if state.agent_graph_busy {
                return true;
            }
            if let Some(manager) = state.task_graph_manager.as_mut() {
                manager.confirmation = Some(crate::app::TaskGraphConfirmation {
                    action: crate::app::TaskGraphConfirmationAction::Start,
                    confirm_selected: false,
                });
                manager.error = None;
            }
        }
        KeyCode::Char('P' | 'p') if modifiers.contains(KeyModifiers::SHIFT) => {
            if !state.agent_graph_busy {
                state.pending_input_action =
                    Some(crate::tui::commands::InputAction::PauseAgentGraph);
            }
        }
        KeyCode::Char('C' | 'c') if modifiers.contains(KeyModifiers::SHIFT) => {
            if state.agent_graph_busy {
                return true;
            }
            if let Some(manager) = state.task_graph_manager.as_mut() {
                manager.confirmation = Some(crate::app::TaskGraphConfirmation {
                    action: crate::app::TaskGraphConfirmationAction::Cancel,
                    confirm_selected: false,
                });
                manager.error = None;
            }
        }
        KeyCode::Char('R' | 'r') if modifiers.contains(KeyModifiers::SHIFT) => {
            if state.agent_graph_busy {
                return true;
            }
            if let Some(task_id) = visible.get(
                state
                    .task_graph_manager
                    .as_ref()
                    .map_or(0, |manager| manager.selected_idx),
            ) {
                if let Some(manager) = state.task_graph_manager.as_mut() {
                    manager.confirmation = Some(crate::app::TaskGraphConfirmation {
                        action: crate::app::TaskGraphConfirmationAction::Retry(*task_id),
                        confirm_selected: false,
                    });
                    manager.error = None;
                }
            }
        }
        KeyCode::Char('H' | 'h') if modifiers.contains(KeyModifiers::SHIFT) => {
            if state.agent_graph_busy {
                return true;
            }
            if let Some(task_id) = visible.get(
                state
                    .task_graph_manager
                    .as_ref()
                    .map_or(0, |manager| manager.selected_idx),
            ) {
                if let Some(manager) = state.task_graph_manager.as_mut() {
                    manager.confirmation = Some(crate::app::TaskGraphConfirmation {
                        action: crate::app::TaskGraphConfirmationAction::Review(*task_id),
                        confirm_selected: false,
                    });
                    manager.error = None;
                }
            }
        }
        KeyCode::Backspace => {
            if let Some(manager) = state.task_graph_manager.as_mut() {
                manager.filter.pop();
                manager.selected_idx = 0;
            }
        }
        KeyCode::Char(character)
            if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            if let Some(manager) = state.task_graph_manager.as_mut() {
                manager.filter.push(character);
                manager.selected_idx = 0;
            }
        }
        _ => {}
    }
    true
}

fn visible_task_graph_ids(state: &crate::app::AppState) -> Vec<crate::work::TaskId> {
    let filter = state
        .task_graph_manager
        .as_ref()
        .map(|manager| manager.filter.trim().to_ascii_lowercase())
        .unwrap_or_default();
    state
        .work_state
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .plan
        .as_ref()
        .map(|plan| {
            plan.tasks
                .iter()
                .filter(|task| {
                    filter.is_empty()
                        || task.title.to_ascii_lowercase().contains(&filter)
                        || task.id.to_string().contains(&filter)
                })
                .map(|task| task.id)
                .collect()
        })
        .unwrap_or_default()
}

fn handle_session_portability(state: &mut crate::app::AppState, code: KeyCode) -> bool {
    let Some(portability) = state.session_portability.as_mut() else {
        return true;
    };
    if portability.busy {
        return true;
    }
    if portability.awaiting_overwrite {
        match code {
            KeyCode::Left | KeyCode::Right | KeyCode::Tab | KeyCode::BackTab => {
                portability.overwrite_selected = !portability.overwrite_selected;
            }
            KeyCode::Esc => {
                state.session_portability = None;
                state.status_message = Some("Session export cancelled".into());
            }
            KeyCode::Enter if !portability.overwrite_selected => {
                state.session_portability = None;
                state.status_message = Some("Session export cancelled".into());
            }
            KeyCode::Enter => {
                let format = match portability.operation {
                    crate::app::SessionPortabilityOperation::Export { format, .. } => format,
                    crate::app::SessionPortabilityOperation::Import
                    | crate::app::SessionPortabilityOperation::ForeignImport { .. } => return true,
                };
                portability.busy = true;
                portability.awaiting_overwrite = false;
                portability.error = None;
                portability.operation = crate::app::SessionPortabilityOperation::Export {
                    format,
                    overwrite: true,
                };
                state.session_portability_request = Some(crate::app::SessionPortabilityRequest {
                    operation: portability.operation,
                    path: portability.path.clone(),
                    generation: portability.generation,
                });
            }
            _ => {}
        }
        return true;
    }
    if portability.error.is_some() {
        if matches!(code, KeyCode::Esc | KeyCode::Enter) {
            state.session_portability = None;
        }
        return true;
    }
    if let Some(preview) = portability.import_preview.as_mut() {
        match code {
            KeyCode::Left | KeyCode::Right | KeyCode::Tab | KeyCode::BackTab
                if preview.sessions > 0 =>
            {
                portability.import_selected = !portability.import_selected;
            }
            KeyCode::Char('l') | KeyCode::Char('L') => {
                portability.import_learning_eligible = !portability.import_learning_eligible;
                preview.learning_eligible = portability.import_learning_eligible;
            }
            KeyCode::Esc => {
                state.session_portability = None;
                state.status_message = Some("Foreign session import cancelled".into());
            }
            KeyCode::Enter if !portability.import_selected || preview.sessions == 0 => {
                state.session_portability = None;
                state.status_message = Some("Foreign session import cancelled".into());
            }
            KeyCode::Enter => {
                let source = match portability.operation {
                    crate::app::SessionPortabilityOperation::ForeignImport { source, .. } => source,
                    _ => return true,
                };
                let operation = crate::app::SessionPortabilityOperation::ForeignImport {
                    source,
                    commit: true,
                    learning_eligible: portability.import_learning_eligible,
                };
                portability.busy = true;
                portability.operation = operation;
                portability.import_preview = None;
                portability.error = None;
                state.session_portability_request = Some(crate::app::SessionPortabilityRequest {
                    operation,
                    path: portability.path.clone(),
                    generation: portability.generation,
                });
            }
            _ => {}
        }
        return true;
    }
    if code == KeyCode::Esc {
        state.session_portability = None;
    }
    true
}

fn handle_session_retention(
    state: &mut crate::app::AppState,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> bool {
    let Some(retention) = state.session_retention.as_mut() else {
        return true;
    };
    if retention.busy.is_some() {
        return true;
    }
    match retention.mode {
        crate::app::SessionRetentionMode::Overview => match code {
            KeyCode::Esc => state.session_retention = None,
            KeyCode::Char('E') | KeyCode::Char('e') if modifiers.contains(KeyModifiers::SHIFT) => {
                if retention.writes_enabled {
                    retention.mode = crate::app::SessionRetentionMode::Edit;
                    retention.field = crate::app::SessionRetentionField::Days;
                    retention.days_input = retention
                        .settings
                        .days
                        .map_or_else(String::new, |value| value.to_string());
                    retention.count_input = retention
                        .settings
                        .count
                        .map_or_else(String::new, |value| value.to_string());
                    retention.cursor = retention.days_input.len();
                    retention.error = None;
                    retention.notice = None;
                } else {
                    retention.error = Some(
                        "Persistent retention settings are unavailable in bare mode; rerun without --profile bare"
                            .into(),
                    );
                }
            }
            KeyCode::Enter => {
                if !retention.sessions_enabled {
                    retention.error = Some(
                        "This run has no persistent session store; rerun without --no-session"
                            .into(),
                    );
                } else {
                    match crate::session_retention::build_preview(
                        &state.session_list,
                        state.current_session.as_ref().map(|session| session.id),
                        retention.settings,
                        chrono::Utc::now(),
                    ) {
                        Ok(preview) => {
                            retention.preview = Some(preview);
                            retention.preview_selected_idx = 0;
                            retention.delete_selected = false;
                            retention.mode = crate::app::SessionRetentionMode::Preview;
                            retention.error = None;
                            retention.notice = None;
                        }
                        Err(error) => retention.error = Some(error),
                    }
                }
            }
            _ => {}
        },
        crate::app::SessionRetentionMode::Edit => match code {
            KeyCode::Esc => {
                retention.mode = crate::app::SessionRetentionMode::Overview;
                retention.error = None;
                retention.days_input = retention
                    .settings
                    .days
                    .map_or_else(String::new, |value| value.to_string());
                retention.count_input = retention
                    .settings
                    .count
                    .map_or_else(String::new, |value| value.to_string());
            }
            KeyCode::Tab | KeyCode::BackTab | KeyCode::Up | KeyCode::Down => {
                retention.field = match retention.field {
                    crate::app::SessionRetentionField::Days => {
                        crate::app::SessionRetentionField::Count
                    }
                    crate::app::SessionRetentionField::Count => {
                        crate::app::SessionRetentionField::Days
                    }
                };
                retention.cursor = retention_input(retention).len();
                retention.error = None;
            }
            KeyCode::Left => {
                retention.cursor = retention.cursor.saturating_sub(1);
            }
            KeyCode::Right => {
                retention.cursor = retention
                    .cursor
                    .saturating_add(1)
                    .min(retention_input(retention).len());
            }
            KeyCode::Home => retention.cursor = 0,
            KeyCode::End => retention.cursor = retention_input(retention).len(),
            KeyCode::Backspace => {
                if retention.cursor > 0 {
                    retention.cursor -= 1;
                    let cursor = retention.cursor;
                    retention_input_mut(retention).remove(cursor);
                }
                retention.error = None;
            }
            KeyCode::Delete => {
                let cursor = retention.cursor;
                if cursor < retention_input(retention).len() {
                    retention_input_mut(retention).remove(cursor);
                }
                retention.error = None;
            }
            KeyCode::Enter => match retention_settings_from_editor(retention) {
                Ok(settings) => {
                    retention.generation = retention.generation.wrapping_add(1);
                    let generation = retention.generation;
                    retention.busy = Some(crate::app::SessionRetentionBusy::Saving);
                    retention.error = None;
                    state.session_retention_generation = generation;
                    state.session_retention_request = Some(crate::app::SessionRetentionRequest {
                        generation,
                        operation: crate::app::SessionRetentionOperation::Save(settings),
                    });
                }
                Err(error) => retention.error = Some(error),
            },
            KeyCode::Char(character) if character.is_ascii_digit() => {
                insert_retention_char(retention, character);
                retention.error = None;
            }
            KeyCode::Char(_) => retention.error = Some("Use decimal digits only".into()),
            _ => {}
        },
        crate::app::SessionRetentionMode::Preview => match code {
            KeyCode::Esc => {
                retention.mode = crate::app::SessionRetentionMode::Overview;
                retention.preview = None;
                retention.delete_selected = false;
                retention.notice = Some("Retention deletion cancelled".into());
                retention.error = None;
            }
            KeyCode::Left | KeyCode::Right | KeyCode::Tab | KeyCode::BackTab => {
                if retention
                    .preview
                    .as_ref()
                    .is_some_and(|preview| !preview.candidates.is_empty())
                {
                    retention.delete_selected = !retention.delete_selected;
                }
                retention.error = None;
            }
            KeyCode::Up => {
                retention.preview_selected_idx = retention.preview_selected_idx.saturating_sub(1)
            }
            KeyCode::Down => {
                if let Some(preview) = &retention.preview {
                    retention.preview_selected_idx = retention
                        .preview_selected_idx
                        .saturating_add(1)
                        .min(preview.candidates.len().saturating_sub(1));
                }
            }
            KeyCode::PageUp => {
                retention.preview_selected_idx = retention.preview_selected_idx.saturating_sub(8)
            }
            KeyCode::PageDown => {
                if let Some(preview) = &retention.preview {
                    retention.preview_selected_idx = retention
                        .preview_selected_idx
                        .saturating_add(8)
                        .min(preview.candidates.len().saturating_sub(1));
                }
            }
            KeyCode::Enter => {
                let Some(preview) = retention.preview.clone() else {
                    return true;
                };
                if !retention.delete_selected || preview.candidates.is_empty() {
                    retention.mode = crate::app::SessionRetentionMode::Overview;
                    retention.preview = None;
                    retention.delete_selected = false;
                    retention.notice = Some("Retention deletion cancelled".into());
                    retention.error = None;
                } else {
                    retention.generation = retention.generation.wrapping_add(1);
                    let generation = retention.generation;
                    retention.busy = Some(crate::app::SessionRetentionBusy::Deleting);
                    retention.error = None;
                    state.session_retention_generation = generation;
                    state.session_retention_request = Some(crate::app::SessionRetentionRequest {
                        generation,
                        operation: crate::app::SessionRetentionOperation::Delete {
                            preview,
                            current_id: state.current_session.as_ref().map(|session| session.id),
                        },
                    });
                }
            }
            _ => {}
        },
    }
    true
}

fn retention_input(retention: &crate::app::SessionRetentionState) -> &String {
    match retention.field {
        crate::app::SessionRetentionField::Days => &retention.days_input,
        crate::app::SessionRetentionField::Count => &retention.count_input,
    }
}

fn retention_input_mut(retention: &mut crate::app::SessionRetentionState) -> &mut String {
    match retention.field {
        crate::app::SessionRetentionField::Days => &mut retention.days_input,
        crate::app::SessionRetentionField::Count => &mut retention.count_input,
    }
}

fn insert_retention_char(retention: &mut crate::app::SessionRetentionState, character: char) {
    let cursor = retention.cursor.min(retention_input(retention).len());
    retention_input_mut(retention).insert(cursor, character);
    retention.cursor = cursor + character.len_utf8();
}

fn retention_settings_from_editor(
    retention: &crate::app::SessionRetentionState,
) -> Result<crate::session_retention::RetentionSettings, String> {
    fn parse_optional(value: &str, label: &str) -> Result<Option<u64>, String> {
        if value.is_empty() {
            Ok(None)
        } else {
            value
                .parse::<u64>()
                .map(Some)
                .map_err(|_| format!("{label} must be a positive decimal integer or blank"))
        }
    }
    let count = parse_optional(&retention.count_input, "Retention count")?
        .map(usize::try_from)
        .transpose()
        .map_err(|_| "Retention count is too large".to_string())?;
    crate::session_retention::RetentionSettings {
        days: parse_optional(&retention.days_input, "Retention days")?,
        count,
    }
    .validate()
}

fn composer_is_command(state: &crate::app::AppState) -> bool {
    state.input.starts_with('/')
        && (crate::tui::commands::parse_command_line_with_model_alias(
            &state.input,
            state.model_list.iter().map(|model| model.id.as_str()),
        )
        .is_some()
            || state
                .input
                .trim()
                .trim_start_matches('/')
                .split_whitespace()
                .next()
                .is_some_and(|name| state.prompt_commands.find(name).is_some()))
}

fn request_interrupt_submission(state: &mut crate::app::AppState) {
    if state.input.trim().is_empty() || !crate::queue_composer_available(state) {
        return;
    }
    if state.hook_blocked {
        state.status_message = Some("Resolve the blocked hook in /hooks before sending".into());
        return;
    }
    if state.hook_running || state.review_in_flight || state.attachment_resolution_busy {
        state.status_message = Some("Press Esc to stop the current preparation first".into());
        return;
    }
    let text = state.take_composer_text();
    state.pending_input_action = Some(crate::tui::commands::InputAction::Steer {
        text,
        queued_id: state.input_queue.editing_id,
    });
}

fn dispatch_key_action(state: &mut crate::app::AppState, action: crate::keymap::KeyAction) {
    use crate::keymap::KeyAction;
    match action {
        KeyAction::ModelPicker => open_model_picker(state),
        KeyAction::OpenEditor => {
            state.pending_input_action = Some(crate::tui::commands::InputAction::OpenEditor)
        }
        KeyAction::CycleModel => {
            if !state.model_list.is_empty() {
                let current_idx = state
                    .model_list
                    .iter()
                    .position(|model| Some(model) == state.current_model.as_ref())
                    .unwrap_or(0);
                let next = (current_idx + 1) % state.model_list.len();
                state.pending_input_action = Some(crate::tui::commands::InputAction::SelectModel(
                    state.model_list[next].clone(),
                ));
            }
        }
        KeyAction::CycleRecentModel => {
            let models = crate::model_preferences::available_recent_models(
                &state.model_list,
                &state.model_preferences,
            );
            queue_scoped_model_cycle(state, &models, "No recent models are available");
        }
        KeyAction::CycleFavoriteModel => {
            let models = crate::model_preferences::available_favorite_models(
                &state.model_list,
                &state.model_preferences,
            );
            queue_scoped_model_cycle(state, &models, "No favorite models are available");
        }
        KeyAction::CycleReasoning => {
            state.pending_input_action = Some(crate::tui::commands::InputAction::CycleReasoning)
        }
        KeyAction::QueueInput => {
            if crate::queue_composer_draft(state) {
                state.pending_input_action =
                    Some(crate::tui::commands::InputAction::PersistSessionQueue);
            }
        }
        KeyAction::SaveOrSteer => {
            if state.turn_lifecycle.is_active() || state.input_queue.editing_id.is_some() {
                request_interrupt_submission(state);
            } else {
                let busy = state.turn_lifecycle.is_active() || state.review_in_flight;
                queue_command(
                    state,
                    crate::tui::commands::CommandAction {
                        id: crate::tui::commands::CommandId::Save,
                        args: String::new(),
                    },
                    busy,
                );
            }
        }
        KeyAction::ToggleExpand => {
            if state.plan_proposal.is_some() && !state.turn_lifecycle.is_active() {
                if let Err(error) = crate::plan::open_reader(state) {
                    state.status_message = Some(error);
                }
            } else if let Some(key) = crate::tui::render::latest_expandable_block_key(state) {
                toggle_expanded_block(state, key, "latest block");
            } else {
                state.status_message = Some("Nothing to expand".into());
            }
        }
        KeyAction::HistorySearch => open_history_search(state, String::new()),
        KeyAction::StashDraft => {
            if state.input.trim().is_empty() {
                state.status_message = Some("Nothing to stash".into());
            } else if !state.pasted_images.is_empty() {
                state.status_message =
                    Some("Remove pasted images before stashing this draft".into());
            } else {
                state.pending_input_action =
                    Some(crate::tui::commands::InputAction::SavePromptDraft);
                state.status_message = Some("Saving prompt draft…".into());
            }
        }
        KeyAction::FocusAgentActivity => focus_latest_agent_activity(state),
        KeyAction::RemoveLastAttachment => {
            if state.pasted_images.pop().is_some() {
                state.status_message = Some("Removed last image attachment".into());
            } else if let Some(element) = state.composer_ui.atomic_elements.last().cloned() {
                if element.range.end <= state.input.len() {
                    state.input.replace_range(element.range.clone(), "");
                    state.input_cursor = element.range.start.min(state.input.len());
                    state.composer_ui.apply_edit(element.range, 0);
                    state
                        .composer_attachments
                        .retain(|attachment| attachment.is_referenced_by(&state.input));
                    state.status_message = Some("Removed last composer attachment".into());
                }
            } else {
                state.status_message = Some("No attachment to remove".into());
            }
        }
        KeyAction::EditLastQueued => {
            if !state.input.is_empty() {
                state.status_message =
                    Some("Clear the composer before editing a queued input".into());
            } else if let Some(text) = state.input_queue.begin_edit_last() {
                state.input = text;
                state.input_cursor = state.input.len();
                state.composer_ui.clear_for_new_draft();
                state.composer_attachments.clear();
                state.pasted_images.clear();
                state.status_message = Some("Editing last queued input".into());
            } else {
                state.status_message = Some("No queued input to edit".into());
            }
        }
        KeyAction::TranscriptTop => {
            state.transcript_scroll.oldest();
        }
        KeyAction::TranscriptBottom => {
            state.transcript_scroll.latest();
        }
        KeyAction::CopyTranscript => {
            let busy = state.turn_lifecycle.is_active() || state.review_in_flight;
            queue_command(
                state,
                crate::tui::commands::CommandAction {
                    id: crate::tui::commands::CommandId::Copy,
                    args: "all".into(),
                },
                busy,
            );
        }
        KeyAction::SplitHorizontal => {
            state.pane_request = Some(crate::panes::PaneRequest::Split {
                axis: crate::panes::SplitAxis::Horizontal,
                session: None,
            });
        }
        KeyAction::SplitVertical => {
            state.pane_request = Some(crate::panes::PaneRequest::Split {
                axis: crate::panes::SplitAxis::Vertical,
                session: None,
            });
        }
        KeyAction::NextPane => {
            state.pane_request = Some(crate::panes::PaneRequest::FocusNext);
        }
        KeyAction::PreviousPane => {
            state.pane_request = Some(crate::panes::PaneRequest::FocusPrevious);
        }
        KeyAction::ClosePane => {
            state.pane_request = Some(crate::panes::PaneRequest::Close);
        }
    }
}

fn handle_prompt_stash(state: &mut crate::app::AppState, code: KeyCode) -> bool {
    let Some(picker) = state.prompt_stash.as_mut() else {
        return false;
    };
    if picker.busy {
        return true;
    }
    match code {
        KeyCode::Esc => state.prompt_stash = None,
        KeyCode::Up => {
            picker.selected_idx = picker.selected_idx.saturating_sub(1);
            picker.delete_confirmation = None;
        }
        KeyCode::Down => {
            picker.selected_idx =
                (picker.selected_idx + 1).min(picker.entries.len().saturating_sub(1));
            picker.delete_confirmation = None;
        }
        KeyCode::Home => {
            picker.selected_idx = 0;
            picker.delete_confirmation = None;
        }
        KeyCode::End => {
            picker.selected_idx = picker.entries.len().saturating_sub(1);
            picker.delete_confirmation = None;
        }
        KeyCode::Enter => {
            if !picker.writes_enabled {
                picker.error = Some("Prompt stash persistence is unavailable in bare mode".into());
            } else if !state.pasted_images.is_empty() {
                picker.error = Some("Remove pasted images before restoring a draft".into());
            } else if let Some(id) = picker.selected().map(|entry| entry.id) {
                picker.busy = true;
                picker.error = None;
                picker.notice = Some(if state.input.trim().is_empty() {
                    "Restoring draft…".into()
                } else {
                    "Swapping composer and stashed draft…".into()
                });
                state.pending_input_action =
                    Some(crate::tui::commands::InputAction::RestorePromptDraft(id));
            }
        }
        KeyCode::Char('d') => {
            if !picker.writes_enabled {
                picker.error = Some("Prompt stash persistence is unavailable in bare mode".into());
            } else if let Some(id) = picker.selected().map(|entry| entry.id) {
                if picker.delete_confirmation == Some(id) {
                    picker.busy = true;
                    picker.error = None;
                    picker.notice = Some("Deleting draft…".into());
                    state.pending_input_action =
                        Some(crate::tui::commands::InputAction::DeletePromptDraft(id));
                } else {
                    picker.delete_confirmation = Some(id);
                    picker.notice = Some("Press d again to delete this draft".into());
                    picker.error = None;
                }
            }
        }
        _ => {
            picker.delete_confirmation = None;
        }
    }
    true
}

fn focus_latest_agent_activity(state: &mut crate::app::AppState) {
    let live = state
        .live_agent_activity
        .iter()
        .filter(|agent| agent.origin == crate::agent::subagent::AgentRunOrigin::Conversation)
        .max_by_key(|agent| agent.updated_at);
    let persisted = state
        .transcript_agent_summaries
        .iter()
        .max_by_key(|summary| summary.updated_at);
    let (turn_id, selected_agent) = match (live, persisted) {
        (Some(agent), Some(summary)) if summary.updated_at > agent.updated_at => {
            (summary.turn_id, Some(summary.agent_id))
        }
        (Some(agent), _) => (agent.parent_turn, Some(agent.id)),
        (None, Some(summary)) => (summary.turn_id, Some(summary.agent_id)),
        (None, None) => {
            state.status_message = Some("No conversation-agent activity".into());
            return;
        }
    };
    state.agent_activity_focus = Some(crate::app::AgentActivityFocusState {
        turn_id,
        selected_agent,
        detail_open: true,
        ..crate::app::AgentActivityFocusState::default()
    });
    state.transcript_scroll.latest();
    state.status_message = Some("Agent activity focused · Esc to return".into());
}

#[derive(Clone)]
struct FocusAgentRow {
    id: crate::agent::subagent::AgentId,
    status: crate::agent::subagent::AgentStatus,
    live: bool,
    updated_at: chrono::DateTime<chrono::Utc>,
}

fn focused_agent_rows(state: &crate::app::AppState, turn_id: uuid::Uuid) -> Vec<FocusAgentRow> {
    let mut rows = state
        .live_agent_activity
        .iter()
        .filter(|agent| {
            agent.origin == crate::agent::subagent::AgentRunOrigin::Conversation
                && agent.parent_turn == turn_id
        })
        .map(|agent| FocusAgentRow {
            id: agent.id,
            status: agent.status.clone(),
            live: true,
            updated_at: agent.updated_at,
        })
        .collect::<Vec<_>>();
    let live_ids = rows
        .iter()
        .map(|row| row.id)
        .collect::<std::collections::HashSet<_>>();
    rows.extend(
        state
            .transcript_agent_summaries
            .iter()
            .filter(|summary| summary.turn_id == turn_id && !live_ids.contains(&summary.agent_id))
            .map(|summary| FocusAgentRow {
                id: summary.agent_id,
                status: summary.status.clone(),
                live: false,
                updated_at: summary.updated_at,
            }),
    );
    rows.sort_by_key(|row| row.updated_at);
    rows
}

fn queue_inline_agent_action(state: &mut crate::app::AppState, request: String) {
    state.agents_request = Some(request);
    state.agent_activity_action_pending = true;
    state.status_message = Some("Applying agent action…".into());
}

fn handle_agent_activity_focus(
    state: &mut crate::app::AppState,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> bool {
    let Some(snapshot) = state.agent_activity_focus.as_ref().cloned() else {
        return false;
    };
    let rows = focused_agent_rows(state, snapshot.turn_id);
    let selected = snapshot
        .selected_agent
        .and_then(|id| rows.iter().position(|row| row.id == id))
        .unwrap_or_else(|| rows.len().saturating_sub(1));
    let selected_row = rows.get(selected).cloned();

    if snapshot.message_draft.is_some() {
        match (modifiers, code) {
            (_, KeyCode::Esc) => {
                if let Some(focus) = state.agent_activity_focus.as_mut() {
                    focus.message_draft = None;
                    focus.message_cursor = 0;
                }
            }
            (_, KeyCode::Enter) => {
                let submission = state.agent_activity_focus.as_ref().and_then(|focus| {
                    focus
                        .message_draft
                        .as_deref()
                        .map(str::trim)
                        .filter(|message| !message.is_empty())
                        .zip(focus.selected_agent)
                        .map(|(message, id)| (message.to_string(), id))
                });
                if let Some((message, id)) = submission {
                    queue_inline_agent_action(state, format!("message {id} {message}"));
                }
                if let Some(focus) = state.agent_activity_focus.as_mut() {
                    focus.message_draft = None;
                    focus.message_cursor = 0;
                }
            }
            (_, KeyCode::Backspace) => {
                if let Some(focus) = state.agent_activity_focus.as_mut() {
                    if let Some(draft) = focus.message_draft.as_mut() {
                        let previous = previous_char_boundary(draft, focus.message_cursor);
                        draft.drain(previous..focus.message_cursor);
                        focus.message_cursor = previous;
                    }
                }
            }
            (_, KeyCode::Delete) => {
                if let Some(focus) = state.agent_activity_focus.as_mut() {
                    if let Some(draft) = focus.message_draft.as_mut() {
                        let next = next_char_boundary(draft, focus.message_cursor);
                        draft.drain(focus.message_cursor..next);
                    }
                }
            }
            (_, KeyCode::Left) => {
                if let Some(focus) = state.agent_activity_focus.as_mut() {
                    if let Some(draft) = focus.message_draft.as_ref() {
                        focus.message_cursor = previous_char_boundary(draft, focus.message_cursor);
                    }
                }
            }
            (_, KeyCode::Right) => {
                if let Some(focus) = state.agent_activity_focus.as_mut() {
                    if let Some(draft) = focus.message_draft.as_ref() {
                        focus.message_cursor = next_char_boundary(draft, focus.message_cursor);
                    }
                }
            }
            (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(character)) => {
                if let Some(focus) = state.agent_activity_focus.as_mut() {
                    if let Some(draft) = focus.message_draft.as_mut() {
                        if draft.len().saturating_add(character.len_utf8()) <= 4096 {
                            draft.insert(focus.message_cursor, character);
                            focus.message_cursor += character.len_utf8();
                        }
                    }
                }
            }
            _ => {}
        }
        return true;
    }

    if snapshot.cancel_confirmation {
        match code {
            KeyCode::Esc => {
                if let Some(focus) = state.agent_activity_focus.as_mut() {
                    focus.cancel_confirmation = false;
                    focus.cancel_confirm_selected = false;
                }
            }
            KeyCode::Left | KeyCode::Right | KeyCode::Tab | KeyCode::BackTab => {
                if let Some(focus) = state.agent_activity_focus.as_mut() {
                    focus.cancel_confirm_selected = !focus.cancel_confirm_selected;
                }
            }
            KeyCode::Enter => {
                if snapshot.cancel_confirm_selected {
                    if let Some(id) = snapshot.selected_agent {
                        queue_inline_agent_action(state, format!("cancel {id}"));
                    }
                }
                if let Some(focus) = state.agent_activity_focus.as_mut() {
                    focus.cancel_confirmation = false;
                    focus.cancel_confirm_selected = false;
                }
            }
            _ => {}
        }
        return true;
    }

    match (modifiers, code) {
        (_, KeyCode::Esc) => {
            state.agent_activity_focus = None;
            state.status_message = Some("Agent activity unfocused".into());
        }
        (_, KeyCode::Up | KeyCode::Down) if !rows.is_empty() => {
            let next = if code == KeyCode::Up {
                selected.checked_sub(1).unwrap_or(rows.len() - 1)
            } else {
                (selected + 1) % rows.len()
            };
            if let Some(focus) = state.agent_activity_focus.as_mut() {
                focus.selected_agent = Some(rows[next].id);
                focus.detail_scroll = 0;
            }
        }
        (_, KeyCode::Enter) => {
            if let Some(focus) = state.agent_activity_focus.as_mut() {
                focus.detail_open = !focus.detail_open;
            }
        }
        (_, KeyCode::PageUp) => {
            if let Some(focus) = state.agent_activity_focus.as_mut() {
                focus.detail_scroll = focus.detail_scroll.saturating_add(8);
            }
        }
        (_, KeyCode::PageDown) => {
            if let Some(focus) = state.agent_activity_focus.as_mut() {
                focus.detail_scroll = focus.detail_scroll.saturating_sub(8);
            }
        }
        (KeyModifiers::NONE, KeyCode::Char('m'))
            if selected_row
                .as_ref()
                .is_some_and(|row| row.live && !row.status.terminal()) =>
        {
            if let Some(focus) = state.agent_activity_focus.as_mut() {
                focus.message_draft = Some(String::new());
                focus.message_cursor = 0;
            }
        }
        (KeyModifiers::NONE, KeyCode::Char('y'))
            if selected_row.as_ref().is_some_and(|row| {
                row.live && row.status == crate::agent::subagent::AgentStatus::WaitingApproval
            }) =>
        {
            if let Some(id) = snapshot.selected_agent {
                queue_inline_agent_action(state, format!("approve {id}"));
            }
        }
        (KeyModifiers::NONE, KeyCode::Char('n'))
            if selected_row.as_ref().is_some_and(|row| {
                row.live && row.status == crate::agent::subagent::AgentStatus::WaitingApproval
            }) =>
        {
            if let Some(id) = snapshot.selected_agent {
                queue_inline_agent_action(state, format!("deny {id}"));
            }
        }
        (KeyModifiers::NONE, KeyCode::Char('c'))
            if selected_row
                .as_ref()
                .is_some_and(|row| row.live && !row.status.terminal()) =>
        {
            if let Some(focus) = state.agent_activity_focus.as_mut() {
                focus.cancel_confirmation = true;
                focus.cancel_confirm_selected = false;
            }
        }
        _ => {}
    }
    true
}

pub fn open_keymap_hub(state: &mut crate::app::AppState, filter: String) {
    state.pending_key_chord = None;
    state.keymap_hub = Some(crate::app::KeymapHubState {
        filter,
        selected_idx: 0,
        capture: None,
        saving: false,
        writes_enabled: state.run_capabilities.config_writes,
        error: None,
        notice: None,
    });
}

pub fn open_history_search(state: &mut crate::app::AppState, filter: String) {
    state.pending_key_chord = None;
    state.history_search = Some(crate::app::HistorySearchState {
        filter,
        selected_idx: 0,
    });
}

fn visible_keymap_actions(state: &crate::app::AppState) -> Vec<crate::keymap::KeyAction> {
    let filter = state
        .keymap_hub
        .as_ref()
        .map(|hub| hub.filter.trim().to_ascii_lowercase())
        .unwrap_or_default();
    crate::keymap::KeyAction::ALL
        .into_iter()
        .filter(|action| {
            filter.is_empty()
                || action.label().to_ascii_lowercase().contains(&filter)
                || action.config_key().contains(&filter)
                || state
                    .keymap
                    .display_binding(*action)
                    .to_ascii_lowercase()
                    .contains(&filter)
        })
        .collect()
}

fn handle_keymap_hub(
    state: &mut crate::app::AppState,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> bool {
    if state.keymap_hub.as_ref().is_some_and(|hub| hub.saving) {
        return true;
    }
    if state
        .keymap_hub
        .as_ref()
        .and_then(|hub| hub.capture.as_ref())
        .is_some()
    {
        if code == KeyCode::Esc {
            if let Some(hub) = state.keymap_hub.as_mut() {
                hub.capture = None;
                hub.error = None;
            }
            return true;
        }
        let capture = state
            .keymap_hub
            .as_ref()
            .and_then(|hub| hub.capture.clone())
            .expect("capture exists");
        if code == KeyCode::Enter && modifiers.is_empty() {
            if let Some(first) = capture.first {
                match crate::keymap::KeyChord::new(vec![first]) {
                    Ok(chord) => {
                        if let Some(hub) = state.keymap_hub.as_mut() {
                            hub.saving = true;
                            hub.error = None;
                        }
                        state.pending_input_action =
                            Some(crate::tui::commands::InputAction::SaveKeymapBinding {
                                action: capture.action,
                                chord: Some(chord),
                            });
                    }
                    Err(error) => {
                        if let Some(hub) = state.keymap_hub.as_mut() {
                            hub.error = Some(error.to_string());
                        }
                    }
                }
            }
            return true;
        }
        let stroke = crate::keymap::KeyStroke::from_event(code, modifiers);
        if let Some(first) = capture.first {
            match crate::keymap::KeyChord::new(vec![first, stroke]) {
                Ok(chord) => {
                    if let Some(hub) = state.keymap_hub.as_mut() {
                        hub.saving = true;
                        hub.error = None;
                    }
                    state.pending_input_action =
                        Some(crate::tui::commands::InputAction::SaveKeymapBinding {
                            action: capture.action,
                            chord: Some(chord),
                        });
                }
                Err(error) => {
                    if let Some(hub) = state.keymap_hub.as_mut() {
                        hub.error = Some(error.to_string());
                    }
                }
            }
        } else {
            match crate::keymap::KeyChord::new(vec![stroke]) {
                Ok(_) => {
                    if let Some(hub) = state.keymap_hub.as_mut() {
                        if let Some(capture) = hub.capture.as_mut() {
                            capture.first = Some(stroke);
                        }
                        hub.error = None;
                    }
                }
                Err(error) => {
                    if let Some(hub) = state.keymap_hub.as_mut() {
                        hub.error = Some(error.to_string());
                    }
                }
            }
        }
        return true;
    }

    let visible = visible_keymap_actions(state);
    match (modifiers, code) {
        (_, KeyCode::Esc) => state.keymap_hub = None,
        (_, KeyCode::Up) => {
            if let Some(hub) = state.keymap_hub.as_mut() {
                hub.selected_idx = hub.selected_idx.saturating_sub(1);
            }
        }
        (_, KeyCode::Down) => {
            if let Some(hub) = state.keymap_hub.as_mut() {
                hub.selected_idx = hub
                    .selected_idx
                    .saturating_add(1)
                    .min(visible.len().saturating_sub(1));
            }
        }
        (_, KeyCode::Home) => {
            if let Some(hub) = state.keymap_hub.as_mut() {
                hub.selected_idx = 0;
            }
        }
        (_, KeyCode::End) => {
            if let Some(hub) = state.keymap_hub.as_mut() {
                hub.selected_idx = visible.len().saturating_sub(1);
            }
        }
        (_, KeyCode::PageUp) => {
            if let Some(hub) = state.keymap_hub.as_mut() {
                hub.selected_idx = hub.selected_idx.saturating_sub(6);
            }
        }
        (_, KeyCode::PageDown) => {
            if let Some(hub) = state.keymap_hub.as_mut() {
                hub.selected_idx = hub
                    .selected_idx
                    .saturating_add(6)
                    .min(visible.len().saturating_sub(1));
            }
        }
        (_, KeyCode::Backspace) => {
            if let Some(hub) = state.keymap_hub.as_mut() {
                hub.filter.pop();
                hub.selected_idx = 0;
                hub.error = None;
            }
        }
        (mods, KeyCode::Char('r' | 'R')) if mods.contains(KeyModifiers::SHIFT) => {
            let writes_enabled = state
                .keymap_hub
                .as_ref()
                .is_some_and(|hub| hub.writes_enabled);
            if !writes_enabled {
                if let Some(hub) = state.keymap_hub.as_mut() {
                    hub.error = Some(
                        "Keymap changes are unavailable in bare mode; rerun without --profile bare"
                            .into(),
                    );
                }
            } else if let Some(action) = visible
                .get(state.keymap_hub.as_ref().map_or(0, |hub| hub.selected_idx))
                .copied()
            {
                if let Some(hub) = state.keymap_hub.as_mut() {
                    hub.saving = true;
                    hub.error = None;
                }
                state.pending_input_action =
                    Some(crate::tui::commands::InputAction::SaveKeymapBinding {
                        action,
                        chord: None,
                    });
            }
        }
        (_, KeyCode::Enter) => {
            let writes_enabled = state
                .keymap_hub
                .as_ref()
                .is_some_and(|hub| hub.writes_enabled);
            if !writes_enabled {
                if let Some(hub) = state.keymap_hub.as_mut() {
                    hub.error = Some(
                        "Keymap changes are unavailable in bare mode; rerun without --profile bare"
                            .into(),
                    );
                }
            } else if let Some(action) = visible
                .get(state.keymap_hub.as_ref().map_or(0, |hub| hub.selected_idx))
                .copied()
            {
                if let Some(hub) = state.keymap_hub.as_mut() {
                    hub.capture = Some(crate::app::KeymapCaptureState {
                        action,
                        first: None,
                    });
                    hub.error = None;
                    hub.notice = None;
                }
            }
        }
        (mods, KeyCode::Char(character))
            if !mods.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            if let Some(hub) = state.keymap_hub.as_mut() {
                hub.filter.push(character);
                hub.selected_idx = 0;
                hub.error = None;
            }
        }
        _ => {}
    }
    true
}

fn visible_history_indices(state: &crate::app::AppState) -> Vec<usize> {
    let filter = state
        .history_search
        .as_ref()
        .map(|history| history.filter.to_ascii_lowercase())
        .unwrap_or_default();
    state
        .input_history
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, entry)| filter.is_empty() || entry.to_ascii_lowercase().contains(&filter))
        .map(|(index, _)| index)
        .collect()
}

fn handle_history_search(state: &mut crate::app::AppState, code: KeyCode) -> bool {
    let visible = visible_history_indices(state);
    match code {
        KeyCode::Esc => state.history_search = None,
        KeyCode::Enter => {
            let selected = state
                .history_search
                .as_ref()
                .map_or(0, |history| history.selected_idx);
            if let Some(index) = visible.get(selected).copied() {
                state.input = state.input_history[index].clone();
                state.input_cursor = state.input.len();
                state.composer_ui.clear_for_new_draft();
                state.composer_attachments.clear();
                state.pasted_images.clear();
                state.history_idx = Some(index);
                state.history_search = None;
                state.status_message = Some("Restored input history draft".into());
            }
        }
        KeyCode::Up => {
            if let Some(history) = state.history_search.as_mut() {
                history.selected_idx = history.selected_idx.saturating_sub(1);
            }
        }
        KeyCode::Down => {
            if let Some(history) = state.history_search.as_mut() {
                history.selected_idx = history
                    .selected_idx
                    .saturating_add(1)
                    .min(visible.len().saturating_sub(1));
            }
        }
        KeyCode::Home => {
            if let Some(history) = state.history_search.as_mut() {
                history.selected_idx = 0;
            }
        }
        KeyCode::End => {
            if let Some(history) = state.history_search.as_mut() {
                history.selected_idx = visible.len().saturating_sub(1);
            }
        }
        KeyCode::PageUp => {
            if let Some(history) = state.history_search.as_mut() {
                history.selected_idx = history.selected_idx.saturating_sub(8);
            }
        }
        KeyCode::PageDown => {
            if let Some(history) = state.history_search.as_mut() {
                history.selected_idx = history
                    .selected_idx
                    .saturating_add(8)
                    .min(visible.len().saturating_sub(1));
            }
        }
        KeyCode::Backspace => {
            if let Some(history) = state.history_search.as_mut() {
                history.filter.pop();
                history.selected_idx = 0;
            }
        }
        KeyCode::Char(character) if !character.is_control() => {
            if let Some(history) = state.history_search.as_mut() {
                history.filter.push(character);
                history.selected_idx = 0;
            }
        }
        _ => {}
    }
    true
}

pub fn open_btw(
    state: &mut crate::app::AppState,
    question: String,
    provider: String,
    model: String,
) {
    if state.btw.is_some() {
        state.status_message = Some("A side question is already open".into());
        return;
    }
    let question = question.trim().to_string();
    state.btw_generation = state.btw_generation.wrapping_add(1);
    let generation = state.btw_generation;
    state.btw_start_requested = !question.is_empty();
    state.btw = Some(crate::app::BtwState {
        cursor: question.len(),
        question,
        answer: String::new(),
        status: if state.btw_start_requested {
            crate::app::BtwStatus::Running
        } else {
            crate::app::BtwStatus::Editing
        },
        provider,
        model,
        scroll: 0,
        generation,
    });
}

fn handle_btw(state: &mut crate::app::AppState, code: KeyCode) -> bool {
    let Some(status) = state.btw.as_ref().map(|btw| btw.status.clone()) else {
        return false;
    };
    match status {
        crate::app::BtwStatus::Editing => match code {
            KeyCode::Esc => {
                state.btw = None;
                state.btw_generation = state.btw_generation.wrapping_add(1);
            }
            KeyCode::Enter => {
                let btw = state.btw.as_mut().expect("btw exists");
                if btw.question.trim().is_empty() {
                    state.status_message = Some("Enter a side question first".into());
                } else {
                    btw.question = btw.question.trim().to_string();
                    btw.cursor = btw.question.len();
                    btw.status = crate::app::BtwStatus::Running;
                    state.btw_start_requested = true;
                }
            }
            KeyCode::Char(character) => {
                let btw = state.btw.as_mut().expect("btw exists");
                btw.question.insert(btw.cursor, character);
                btw.cursor += character.len_utf8();
            }
            KeyCode::Backspace => {
                let btw = state.btw.as_mut().expect("btw exists");
                if btw.cursor > 0 {
                    let previous = previous_char_boundary(&btw.question, btw.cursor);
                    btw.question.replace_range(previous..btw.cursor, "");
                    btw.cursor = previous;
                }
            }
            KeyCode::Delete => {
                let btw = state.btw.as_mut().expect("btw exists");
                if btw.cursor < btw.question.len() {
                    let next = next_char_boundary(&btw.question, btw.cursor);
                    btw.question.replace_range(btw.cursor..next, "");
                }
            }
            KeyCode::Left => {
                let btw = state.btw.as_mut().expect("btw exists");
                btw.cursor = previous_char_boundary(&btw.question, btw.cursor);
            }
            KeyCode::Right => {
                let btw = state.btw.as_mut().expect("btw exists");
                btw.cursor = next_char_boundary(&btw.question, btw.cursor);
            }
            KeyCode::Home => state.btw.as_mut().expect("btw exists").cursor = 0,
            KeyCode::End => {
                let btw = state.btw.as_mut().expect("btw exists");
                btw.cursor = btw.question.len();
            }
            _ => {}
        },
        crate::app::BtwStatus::Running => match code {
            KeyCode::Esc => {
                state.btw_generation = state.btw_generation.wrapping_add(1);
                let generation = state.btw_generation;
                let btw = state.btw.as_mut().expect("btw exists");
                btw.status = crate::app::BtwStatus::Cancelled;
                btw.generation = generation;
                state.pending_input_action = Some(crate::tui::commands::InputAction::CancelBtw);
            }
            KeyCode::PageUp | KeyCode::Up => {
                let btw = state.btw.as_mut().expect("btw exists");
                btw.scroll =
                    btw.scroll
                        .saturating_sub(if code == KeyCode::PageUp { 10 } else { 1 });
            }
            KeyCode::PageDown | KeyCode::Down => {
                let btw = state.btw.as_mut().expect("btw exists");
                btw.scroll =
                    btw.scroll
                        .saturating_add(if code == KeyCode::PageDown { 10 } else { 1 });
            }
            _ => {}
        },
        crate::app::BtwStatus::Complete
        | crate::app::BtwStatus::Failed(_)
        | crate::app::BtwStatus::Cancelled => match code {
            KeyCode::Esc | KeyCode::Enter => {
                state.btw = None;
                state.btw_generation = state.btw_generation.wrapping_add(1);
            }
            KeyCode::PageUp | KeyCode::Up => {
                let btw = state.btw.as_mut().expect("btw exists");
                btw.scroll =
                    btw.scroll
                        .saturating_sub(if code == KeyCode::PageUp { 10 } else { 1 });
            }
            KeyCode::PageDown | KeyCode::Down => {
                let btw = state.btw.as_mut().expect("btw exists");
                btw.scroll =
                    btw.scroll
                        .saturating_add(if code == KeyCode::PageDown { 10 } else { 1 });
            }
            _ => {}
        },
    }
    true
}

fn floor_char_boundary(text: &str, cursor: usize) -> usize {
    let mut cursor = cursor.min(text.len());
    while !text.is_char_boundary(cursor) {
        cursor -= 1;
    }
    cursor
}

fn previous_char_boundary(text: &str, cursor: usize) -> usize {
    let cursor = floor_char_boundary(text, cursor);
    text[..cursor]
        .char_indices()
        .next_back()
        .map(|(index, _)| index)
        .unwrap_or(0)
}

/// Find the byte index of the start of the next word from `cursor`.
/// Skips current non-whitespace word, then skips whitespace.
/// Uses Unicode `char::is_whitespace` classification and scalar boundaries.
fn next_word_boundary(text: &str, cursor: usize) -> usize {
    let cursor = floor_char_boundary(text, cursor);
    // Phase 1: skip current non-whitespace word
    let after_word = {
        let mut pos = cursor;
        while pos < text.len() {
            let next = next_char_boundary(text, pos);
            if text[pos..next]
                .chars()
                .next()
                .is_none_or(|c| c.is_whitespace())
            {
                break;
            }
            pos = next;
        }
        pos
    };
    // Phase 2: skip whitespace
    let mut pos = after_word;
    while pos < text.len() {
        let next = next_char_boundary(text, pos);
        if !text[pos..next]
            .chars()
            .next()
            .is_some_and(|c| c.is_whitespace())
        {
            break;
        }
        pos = next;
    }
    pos
}

/// Find the byte index of the start of the previous word from `cursor`.
/// Skips trailing Unicode whitespace, then skips back over a non-whitespace word.
fn previous_word_boundary(text: &str, cursor: usize) -> usize {
    let cursor = floor_char_boundary(text, cursor);
    // Phase 1: skip trailing whitespace
    let after_ws = {
        let mut pos = cursor;
        while pos > 0 {
            let prev = previous_char_boundary(text, pos);
            if !text[prev..pos]
                .chars()
                .next()
                .is_some_and(|c| c.is_whitespace())
            {
                break;
            }
            pos = prev;
        }
        pos
    };
    // Phase 2: skip back over the word (non-whitespace scalars)
    let mut pos = after_ws;
    while pos > 0 {
        let prev = previous_char_boundary(text, pos);
        if text[prev..pos]
            .chars()
            .next()
            .is_some_and(|c| c.is_whitespace())
        {
            break;
        }
        pos = prev;
    }
    pos
}

/// Find the byte index at the end of the next word from `cursor`.
/// This follows native Option+Right behavior: skip leading Unicode whitespace,
/// then advance across one non-whitespace word.
fn next_word_end_boundary(text: &str, cursor: usize) -> usize {
    let cursor = floor_char_boundary(text, cursor);
    let mut pos = cursor;
    while pos < text.len() {
        let next = next_char_boundary(text, pos);
        if !text[pos..next]
            .chars()
            .next()
            .is_some_and(char::is_whitespace)
        {
            break;
        }
        pos = next;
    }
    while pos < text.len() {
        let next = next_char_boundary(text, pos);
        if text[pos..next]
            .chars()
            .next()
            .is_none_or(char::is_whitespace)
        {
            break;
        }
        pos = next;
    }
    pos
}

fn is_command_modifier(modifiers: KeyModifiers) -> bool {
    modifiers == KeyModifiers::SUPER || modifiers == KeyModifiers::META
}

fn atomic_motion_target(
    state: &crate::app::AppState,
    target: usize,
    moving_forward: bool,
) -> usize {
    state
        .composer_ui
        .element_containing(target)
        .map_or(target, |range| {
            if moving_forward {
                range.end
            } else {
                range.start
            }
        })
}

fn move_to_previous_word(state: &mut crate::app::AppState) {
    let cursor = crate::tui::composer::normalize_boundary(&state.input, state.input_cursor);
    let target = previous_word_boundary(&state.input, cursor);
    state.input_cursor = atomic_motion_target(state, target, false);
    state.composer_ui.preferred_column = None;
}

fn move_to_next_word_end(state: &mut crate::app::AppState) {
    let cursor = crate::tui::composer::normalize_boundary(&state.input, state.input_cursor);
    let target = next_word_end_boundary(&state.input, cursor);
    state.input_cursor = atomic_motion_target(state, target, true);
    state.composer_ui.preferred_column = None;
}

fn move_to_hard_line_start(state: &mut crate::app::AppState) {
    let cursor = crate::tui::composer::normalize_boundary(&state.input, state.input_cursor);
    let target = crate::tui::composer::hard_line_bounds(&state.input, cursor).start;
    state.input_cursor = atomic_motion_target(state, target, false);
    state.composer_ui.preferred_column = None;
}

fn move_to_hard_line_end(state: &mut crate::app::AppState) {
    let cursor = crate::tui::composer::normalize_boundary(&state.input, state.input_cursor);
    let target = crate::tui::composer::hard_line_bounds(&state.input, cursor).end;
    state.input_cursor = atomic_motion_target(state, target, true);
    state.composer_ui.preferred_column = None;
}

fn expand_deletion_to_atomic_elements(
    state: &crate::app::AppState,
    mut range: std::ops::Range<usize>,
) -> std::ops::Range<usize> {
    loop {
        let previous = range.clone();
        for element in &state.composer_ui.atomic_elements {
            if element.range.start < range.end && range.start < element.range.end {
                range.start = range.start.min(element.range.start);
                range.end = range.end.max(element.range.end);
            }
        }
        if range == previous {
            return range;
        }
    }
}

fn delete_composer_range(state: &mut crate::app::AppState, range: std::ops::Range<usize>) {
    let range = expand_deletion_to_atomic_elements(state, range);
    if range.start >= range.end {
        state.input_cursor = range.start;
        state.composer_ui.preferred_column = None;
        return;
    }
    let deleted = state.input[range.clone()].to_string();
    state.composer_ui.apply_edit(range.clone(), 0);
    state.input.replace_range(range.clone(), "");
    state.input_cursor = range.start;
    state
        .composer_attachments
        .retain(|attachment| attachment.is_referenced_by(&state.input));
    state.kill_ring.insert(0, deleted);
    state.kill_ring.truncate(10);
    state.kill_ring_idx = None;
}

fn delete_previous_word(state: &mut crate::app::AppState) {
    let cursor = crate::tui::composer::normalize_boundary(&state.input, state.input_cursor);
    let boundary = previous_word_boundary(&state.input, cursor);
    delete_composer_range(state, boundary..cursor);
}

fn delete_to_hard_line_start(state: &mut crate::app::AppState) {
    let cursor = crate::tui::composer::normalize_boundary(&state.input, state.input_cursor);
    let boundary = crate::tui::composer::hard_line_bounds(&state.input, cursor).start;
    delete_composer_range(state, boundary..cursor);
}

/// Pure predicate: should `@` open the file picker?
/// Returns true when input is empty, cursor is at start, or the Unicode
/// scalar immediately before cursor is whitespace.
pub(crate) fn should_open_file_picker(input: &str, cursor: usize) -> bool {
    if input.is_empty() || cursor == 0 {
        return true;
    }
    let cursor = floor_char_boundary(input, cursor);
    let prev = previous_char_boundary(input, cursor);
    input[prev..cursor]
        .chars()
        .next()
        .is_none_or(|c| c.is_whitespace())
}

fn next_char_boundary(text: &str, cursor: usize) -> usize {
    let cursor = floor_char_boundary(text, cursor);
    match text[cursor..].chars().next() {
        Some(ch) => cursor + ch.len_utf8(),
        None => text.len(),
    }
}

fn delete_previous_char(state: &mut crate::app::AppState) {
    let cursor = crate::tui::composer::normalize_boundary(&state.input, state.input_cursor);
    let range = state
        .composer_ui
        .element_adjacent_to_backspace(cursor)
        .unwrap_or_else(|| state.composer_ui.previous_boundary(&state.input, cursor)..cursor);
    if range.start < range.end {
        state.input.replace_range(range.clone(), "");
        state.composer_ui.apply_edit(range.clone(), 0);
    }
    state.input_cursor = range.start;
    state
        .composer_attachments
        .retain(|attachment| attachment.is_referenced_by(&state.input));
}

fn delete_current_char(state: &mut crate::app::AppState) {
    let cursor = crate::tui::composer::normalize_boundary(&state.input, state.input_cursor);
    let range = state
        .composer_ui
        .element_adjacent_to_delete(cursor)
        .unwrap_or_else(|| cursor..state.composer_ui.next_boundary(&state.input, cursor));
    if range.start < range.end {
        state.input.replace_range(range.clone(), "");
        state.composer_ui.apply_edit(range, 0);
    }
    state.input_cursor = cursor;
    state
        .composer_attachments
        .retain(|attachment| attachment.is_referenced_by(&state.input));
}

fn insert_char(state: &mut crate::app::AppState, c: char) {
    state.input_cursor = crate::tui::composer::normalize_boundary(&state.input, state.input_cursor);
    let insertion = state.input_cursor..state.input_cursor;
    state.composer_ui.apply_edit(insertion, c.len_utf8());
    if state.input_cursor >= state.input.len() {
        state.input.push(c);
    } else {
        state.input.insert(state.input_cursor, c);
    }
    state.input_cursor += c.len_utf8();
}

fn insert_large_paste(state: &mut crate::app::AppState, payload: &str) {
    state.input_cursor = crate::tui::composer::normalize_boundary(&state.input, state.input_cursor);
    let marker = crate::tui::composer::large_paste_placeholder(payload.chars().count());
    let start = state.input_cursor;
    state.composer_ui.apply_edit(start..start, marker.len());
    state.input.insert_str(start, &marker);
    state.input_cursor += marker.len();
    state
        .composer_ui
        .register_large_paste(start..state.input_cursor, payload.to_string());
    state.status_message = Some(format!(
        "Collapsed pasted content ({} chars)",
        payload.chars().count()
    ));
}

fn register_pasted_path(state: &mut crate::app::AppState, start: usize, end: usize, cwd: &str) {
    if start >= end || end > state.input.len() {
        return;
    }
    let value = &state.input[start..end];
    if value.contains('\n') {
        return;
    }
    let candidates = [value, value.trim_end_matches(char::is_whitespace)];
    let Some((text, path, metadata)) = candidates.into_iter().find_map(|candidate| {
        if candidate.is_empty() {
            return None;
        }
        let decoded = decode_dropped_path(candidate);
        let path = std::path::Path::new(&decoded);
        let resolved = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::path::Path::new(cwd).join(path)
        };
        std::fs::symlink_metadata(&resolved)
            .ok()
            .map(|metadata| (candidate, resolved, metadata))
    }) else {
        return;
    };
    let chip_end = start + text.len();
    let kind = if metadata.file_type().is_file() {
        crate::tui::composer::AtomicKind::File
    } else if metadata.file_type().is_dir() {
        let workspace =
            std::fs::canonicalize(cwd).unwrap_or_else(|_| std::path::PathBuf::from(cwd));
        let resolved = std::fs::canonicalize(&path).unwrap_or(path);
        let outside_workspace = !resolved.starts_with(workspace);
        if outside_workspace {
            state.status_message =
                Some("Folder mentioned outside the workspace; use /add-dir to grant access".into());
        }
        crate::tui::composer::AtomicKind::Folder { outside_workspace }
    } else {
        return;
    };
    state
        .composer_ui
        .register_atomic(crate::tui::composer::AtomicElement {
            range: start..chip_end,
            kind,
        });
}

/// Finder and terminal emulators commonly paste a quoted or backslash-escaped
/// path. Decode only for metadata inspection; the composer keeps the exact
/// pasted spelling as its source text.
fn decode_dropped_path(value: &str) -> String {
    let value = if value.len() >= 2
        && ((value.starts_with('\'') && value.ends_with('\''))
            || (value.starts_with('"') && value.ends_with('"')))
    {
        &value[1..value.len() - 1]
    } else {
        value
    };
    let mut decoded = String::with_capacity(value.len());
    let mut characters = value.chars();
    while let Some(character) = characters.next() {
        if character == '\\' {
            if let Some(escaped) = characters.next() {
                decoded.push(escaped);
            } else {
                decoded.push(character);
            }
        } else {
            decoded.push(character);
        }
    }
    decoded
}

fn composer_move_vertical(state: &mut crate::app::AppState, direction: i8) -> bool {
    let layout = crate::tui::composer::ComposerLayout::new(
        &state.input,
        state.input_cursor,
        state.viewport_width.saturating_sub(2) as usize,
        2,
    );
    if let Some(cursor) = layout.move_vertical(direction, &mut state.composer_ui.preferred_column) {
        state.input_cursor = cursor;
        return true;
    }
    state.composer_ui.preferred_column = None;
    if direction < 0 {
        history_up(state)
    } else {
        history_down(state)
    }
}

fn history_up(state: &mut crate::app::AppState) -> bool {
    if state.input_history.is_empty() {
        return false;
    }
    if state.history_idx.is_none() {
        state.composer_ui.history_draft = Some(crate::tui::composer::HistoryDraft {
            text: state.input.clone(),
            cursor: state.input_cursor,
            atomic_elements: state.composer_ui.atomic_elements.clone(),
            attachments: state.composer_attachments.clone(),
            images: state.pasted_images.clone(),
        });
    }
    let idx = match state.history_idx {
        None => state.input_history.len().saturating_sub(1),
        Some(0) => 0,
        Some(i) => i - 1,
    };
    state.input = state.input_history[idx].clone();
    state.input_cursor = state.input.len();
    state.composer_ui.atomic_elements.clear();
    state.composer_attachments.clear();
    state.pasted_images.clear();
    state.history_idx = Some(idx);
    true
}

fn history_down(state: &mut crate::app::AppState) -> bool {
    match state.history_idx {
        Some(i) if i + 1 < state.input_history.len() => {
            state.input = state.input_history[i + 1].clone();
            state.input_cursor = state.input.len();
            state.composer_ui.atomic_elements.clear();
            state.composer_attachments.clear();
            state.pasted_images.clear();
            state.history_idx = Some(i + 1);
        }
        Some(_) => {
            if let Some(draft) = state.composer_ui.history_draft.take() {
                state.input = draft.text;
                state.input_cursor = draft.cursor.min(state.input.len());
                state.composer_ui.atomic_elements = draft.atomic_elements;
                state.composer_attachments = draft.attachments;
                state.pasted_images = draft.images;
            } else {
                state.input.clear();
                state.input_cursor = 0;
                state.composer_ui.atomic_elements.clear();
                state.composer_attachments.clear();
                state.pasted_images.clear();
            }
            state.history_idx = None;
        }
        None => {}
    }
    true
}

fn handle_workspace_trust_prompt(state: &mut crate::app::AppState, code: KeyCode) -> bool {
    match code {
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            state.workspace_trust_prompt = false;
            state.pending_input_action = Some(crate::tui::commands::InputAction::TrustWorkspace);
            true
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            state.should_quit = true;
            true
        }
        _ => false,
    }
}

fn handle_model_picker(
    state: &mut crate::app::AppState,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> bool {
    match code {
        KeyCode::Esc => {
            state.model_picker_open = false;
            state.model_picker_target = crate::app::ModelPickerTarget::Current;
        }
        KeyCode::Enter => {
            if matches!(
                state.model_picker_target,
                crate::app::ModelPickerTarget::Delegation
            ) && delegation_inherit_visible(state)
                && state.selected_model_idx == 0
            {
                state.pending_input_action =
                    Some(crate::tui::commands::InputAction::SaveDelegation(None));
                state.model_picker_open = false;
                state.model_picker_target = crate::app::ModelPickerTarget::Current;
                return true;
            }
            let visible = visible_model_rows(state);
            let model_index = state
                .selected_model_idx
                .saturating_sub(usize::from(delegation_inherit_visible(state)));
            if let Some(row) = visible.get(model_index) {
                let model = row.model.clone();
                state.pending_input_action = Some(match state.model_picker_target {
                    crate::app::ModelPickerTarget::Current => {
                        crate::tui::commands::InputAction::SelectModel(model)
                    }
                    crate::app::ModelPickerTarget::Review => {
                        crate::tui::commands::InputAction::SelectReviewModel(model)
                    }
                    crate::app::ModelPickerTarget::Delegation => {
                        crate::tui::commands::InputAction::SaveDelegation(Some(
                            crate::config::DelegationRoute {
                                provider: model.provider,
                                model: model.id,
                            },
                        ))
                    }
                    crate::app::ModelPickerTarget::MultiRun(candidate) => {
                        crate::tui::commands::InputAction::SelectMultiRunRoute { candidate, model }
                    }
                });
                state.model_picker_open = false;
                state.model_picker_target = crate::app::ModelPickerTarget::Current;
            } else {
                state.status_message = Some("No matching models".into());
            }
        }
        KeyCode::Up => {
            state.selected_model_idx = crate::tui::selection::previous_wrap(
                state.selected_model_idx,
                visible_model_count(state),
            );
        }
        KeyCode::Down => {
            state.selected_model_idx = crate::tui::selection::next_wrap(
                state.selected_model_idx,
                visible_model_count(state),
            );
        }
        KeyCode::Home => {
            state.selected_model_idx = 0;
        }
        KeyCode::End => {
            state.selected_model_idx = visible_model_count(state).saturating_sub(1);
        }
        KeyCode::PageUp => {
            state.selected_model_idx = crate::tui::selection::page_up(state.selected_model_idx, 8);
        }
        KeyCode::PageDown => {
            state.selected_model_idx = crate::tui::selection::page_down(
                state.selected_model_idx,
                visible_model_count(state),
                8,
            );
        }
        KeyCode::Char('F') | KeyCode::Char('f') if modifiers.contains(KeyModifiers::SHIFT) => {
            if !matches!(
                state.model_picker_target,
                crate::app::ModelPickerTarget::Current
            ) {
                state.status_message =
                    Some("Favorites can be edited only in the current-model picker".into());
            } else if !state.model_preferences_store.writable() {
                state.status_message = Some("Model favorites are unavailable in bare mode".into());
            } else if let Some(model) = selected_picker_model(state) {
                state.pending_input_action = Some(
                    crate::tui::commands::InputAction::ToggleModelFavorite(model),
                );
            }
        }
        KeyCode::Char('A') | KeyCode::Char('a') if modifiers.contains(KeyModifiers::SHIFT) => {
            if matches!(
                state.model_picker_target,
                crate::app::ModelPickerTarget::Delegation
            ) {
                state.status_message = Some(
                    "Delegated routes use configured models. Use /provider to add one.".into(),
                );
                return true;
            }
            if matches!(
                state.model_picker_target,
                crate::app::ModelPickerTarget::MultiRun(_)
            ) {
                state.status_message = Some(
                    "Candidate routes are selection-only. Use /provider to edit models.".into(),
                );
                return true;
            }
            let request = selected_picker_model(state)
                .or_else(|| state.current_model.clone())
                .map_or(
                    crate::app::ProviderEditorRequest::AddProvider,
                    crate::app::ProviderEditorRequest::AddModel,
                );
            state.model_picker_open = false;
            state.pending_input_action = Some(
                crate::tui::commands::InputAction::OpenProviderEditor(request),
            );
        }
        KeyCode::Char('P') | KeyCode::Char('p') if modifiers.contains(KeyModifiers::SHIFT) => {
            if matches!(
                state.model_picker_target,
                crate::app::ModelPickerTarget::Delegation
            ) {
                state.status_message = Some(
                    "Delegated routes use configured providers. Use /provider to add one.".into(),
                );
                return true;
            }
            if matches!(
                state.model_picker_target,
                crate::app::ModelPickerTarget::MultiRun(_)
            ) {
                state.status_message = Some(
                    "Candidate routes are selection-only. Use /provider to edit providers.".into(),
                );
                return true;
            }
            state.model_picker_open = false;
            state.pending_input_action =
                Some(crate::tui::commands::InputAction::OpenProviderEditor(
                    crate::app::ProviderEditorRequest::AddProvider,
                ));
        }
        KeyCode::Char('E') | KeyCode::Char('e') if modifiers.contains(KeyModifiers::SHIFT) => {
            if matches!(
                state.model_picker_target,
                crate::app::ModelPickerTarget::Delegation
            ) {
                state.status_message =
                    Some("Use /provider to edit a delegated provider or model.".into());
                return true;
            }
            if matches!(
                state.model_picker_target,
                crate::app::ModelPickerTarget::MultiRun(_)
            ) {
                state.status_message = Some(
                    "Candidate routes are selection-only. Use /provider to edit models.".into(),
                );
                return true;
            }
            let selected = selected_picker_model(state);
            if let Some(model) = selected {
                state.model_picker_open = false;
                state.pending_input_action =
                    Some(crate::tui::commands::InputAction::OpenProviderEditor(
                        crate::app::ProviderEditorRequest::EditModel(model),
                    ));
            }
        }
        KeyCode::Char('R') | KeyCode::Char('r') if modifiers.contains(KeyModifiers::SHIFT) => {
            if !state.model_catalog_refreshing {
                state.model_catalog_refresh_provider = None;
                state.model_catalog_refresh_requested = true;
            }
        }
        KeyCode::Char(c) => {
            state.model_filter.push(c);
            state.selected_model_idx = 0;
        }
        KeyCode::Backspace => {
            state.model_filter.pop();
            state.selected_model_idx = 0;
        }
        _ => {}
    }
    true
}

fn handle_multi_run_manager(
    state: &mut crate::app::AppState,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> bool {
    let confirmation = state
        .multi_run_manager
        .as_ref()
        .and_then(|manager| manager.confirmation.clone());
    if let Some(confirmation) = confirmation {
        match code {
            KeyCode::Esc => {
                if let Some(manager) = state.multi_run_manager.as_mut() {
                    manager.confirmation = None;
                    manager.notice = Some("Multi-run action cancelled".into());
                }
            }
            KeyCode::Left
            | KeyCode::Right
            | KeyCode::Up
            | KeyCode::Down
            | KeyCode::Tab
            | KeyCode::BackTab => {
                if let Some(manager) = state.multi_run_manager.as_mut() {
                    if let Some(confirmation) = manager.confirmation.as_mut() {
                        confirmation.confirm_selected = !confirmation.confirm_selected;
                    }
                }
            }
            KeyCode::Enter if confirmation.confirm_selected => {
                let action = match confirmation.action {
                    crate::app::MultiRunConfirmationAction::Start => {
                        crate::tui::commands::InputAction::StartMultiRun
                    }
                    crate::app::MultiRunConfirmationAction::Cancel => {
                        crate::tui::commands::InputAction::CancelMultiRun
                    }
                    crate::app::MultiRunConfirmationAction::Judge => {
                        crate::tui::commands::InputAction::JudgeMultiRun
                    }
                    crate::app::MultiRunConfirmationAction::Fuse => {
                        crate::tui::commands::InputAction::FuseMultiRun
                    }
                };
                if let Some(manager) = state.multi_run_manager.as_mut() {
                    manager.confirmation = None;
                    manager.busy = true;
                    manager.error = None;
                }
                state.pending_input_action = Some(action);
            }
            KeyCode::Enter => {
                if let Some(manager) = state.multi_run_manager.as_mut() {
                    manager.confirmation = None;
                    manager.notice = Some("Multi-run action cancelled".into());
                }
            }
            _ => {}
        }
        return true;
    }
    let Some(manager) = state.multi_run_manager.as_mut() else {
        return true;
    };
    if manager.busy {
        return true;
    }
    manager.error = None;
    if manager.editing {
        match code {
            KeyCode::Esc => state.multi_run_manager = None,
            KeyCode::Up => {
                manager.candidate_count =
                    (manager.candidate_count + 1).min(crate::multi_run::MAX_CANDIDATES)
            }
            KeyCode::Down => {
                manager.candidate_count = manager
                    .candidate_count
                    .saturating_sub(1)
                    .max(crate::multi_run::DEFAULT_CANDIDATES)
            }
            KeyCode::Left if manager.cursor > 0 => {
                manager.cursor = previous_char_boundary(&manager.draft_request, manager.cursor)
            }
            KeyCode::Right if manager.cursor < manager.draft_request.len() => {
                manager.cursor = next_char_boundary(&manager.draft_request, manager.cursor)
            }
            KeyCode::Backspace if manager.cursor > 0 => {
                let previous = previous_char_boundary(&manager.draft_request, manager.cursor);
                manager
                    .draft_request
                    .replace_range(previous..manager.cursor, "");
                manager.cursor = previous;
            }
            KeyCode::Enter => {
                let request = manager.draft_request.clone();
                let count = manager.candidate_count;
                match crate::multi_run::MultiRunDefinition::build(
                    request,
                    count,
                    state.current_model.as_ref(),
                    &state.model_list,
                ) {
                    Ok(definition) => {
                        let mut run = crate::multi_run::MultiRunState::preview(
                            definition,
                            manager.context_snapshot.clone(),
                        );
                        run.apply_cost_estimates(&manager.pricing);
                        manager.run = Some(run);
                        manager.editing = false;
                        manager.notice = Some("Review routes, then press Shift+S".into());
                    }
                    Err(error) => manager.error = Some(error.to_string()),
                }
            }
            KeyCode::Char(character)
                if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                manager.draft_request.insert(manager.cursor, character);
                manager.cursor += character.len_utf8();
            }
            _ => {}
        }
        return true;
    }
    let Some(run) = manager.run.as_mut() else {
        manager.editing = true;
        return true;
    };
    match code {
        KeyCode::Esc => {
            if let Some(mut closed) = state.multi_run_manager.take() {
                state.multi_run_stashed = closed.run.take();
                state.status_message =
                    Some("Multi-run hidden; use /multi-run to reopen it in this process".into());
            }
        }
        KeyCode::Up => run.selected_idx = run.selected_idx.saturating_sub(1),
        KeyCode::Down => {
            run.selected_idx = run
                .selected_idx
                .saturating_add(1)
                .min(run.candidates.len().saturating_sub(1))
        }
        KeyCode::Enter if run.status == crate::multi_run::MultiRunStatus::Preview => {
            if let Some(candidate) = run.candidates.get(run.selected_idx) {
                state.model_picker_target =
                    crate::app::ModelPickerTarget::MultiRun(candidate.definition.id);
                state.model_picker_open = true;
                state.model_filter.clear();
                state.selected_model_idx = state
                    .model_list
                    .iter()
                    .position(|model| {
                        model.provider == candidate.definition.route.provider
                            && model.id == candidate.definition.route.model
                    })
                    .unwrap_or(0);
            }
        }
        KeyCode::Char('S' | 's')
            if modifiers.contains(KeyModifiers::SHIFT)
                && run.status == crate::multi_run::MultiRunStatus::Preview =>
        {
            manager.confirmation = Some(crate::app::MultiRunConfirmation {
                action: crate::app::MultiRunConfirmationAction::Start,
                confirm_selected: false,
            });
        }
        KeyCode::Char('C' | 'c')
            if modifiers.contains(KeyModifiers::SHIFT)
                && matches!(
                    run.status,
                    crate::multi_run::MultiRunStatus::CreatingWorktrees
                        | crate::multi_run::MultiRunStatus::Running
                ) =>
        {
            manager.confirmation = Some(crate::app::MultiRunConfirmation {
                action: crate::app::MultiRunConfirmationAction::Cancel,
                confirm_selected: false,
            });
        }
        KeyCode::Char('R' | 'r') if modifiers.contains(KeyModifiers::SHIFT) => {
            manager.busy = true;
            state.pending_input_action =
                Some(crate::tui::commands::InputAction::RefreshMultiRunEvidence);
        }
        KeyCode::Char('J' | 'j')
            if modifiers.contains(KeyModifiers::SHIFT)
                && run.status == crate::multi_run::MultiRunStatus::ReadyToJudge =>
        {
            manager.confirmation = Some(crate::app::MultiRunConfirmation {
                action: crate::app::MultiRunConfirmationAction::Judge,
                confirm_selected: false,
            });
        }
        KeyCode::Char('F' | 'f')
            if modifiers.contains(KeyModifiers::SHIFT)
                && run.status == crate::multi_run::MultiRunStatus::Judged
                && run.judgment_is_fresh() =>
        {
            manager.confirmation = Some(crate::app::MultiRunConfirmation {
                action: crate::app::MultiRunConfirmationAction::Fuse,
                confirm_selected: false,
            });
        }
        KeyCode::Char('A' | 'a')
            if modifiers.contains(KeyModifiers::SHIFT)
                && run
                    .candidates
                    .get(run.selected_idx)
                    .is_some_and(|candidate| {
                        candidate.status == crate::multi_run::CandidateStatus::WaitingApproval
                    }) =>
        {
            let candidate = run.candidates[run.selected_idx].definition.id;
            state.pending_input_action =
                Some(crate::tui::commands::InputAction::ResolveMultiRunApproval {
                    candidate,
                    approved: true,
                });
        }
        KeyCode::Char('D' | 'd')
            if modifiers.contains(KeyModifiers::SHIFT)
                && run
                    .candidates
                    .get(run.selected_idx)
                    .is_some_and(|candidate| {
                        candidate.status == crate::multi_run::CandidateStatus::WaitingApproval
                    }) =>
        {
            let candidate = run.candidates[run.selected_idx].definition.id;
            state.pending_input_action =
                Some(crate::tui::commands::InputAction::ResolveMultiRunApproval {
                    candidate,
                    approved: false,
                });
        }
        _ => {}
    }
    true
}

pub fn open_model_picker(state: &mut crate::app::AppState) {
    state.model_picker_target = crate::app::ModelPickerTarget::Current;
    state.model_picker_open = true;
    state.model_filter.clear();
    state.selected_model_idx = state
        .current_model
        .as_ref()
        .and_then(|current| {
            visible_model_rows(state)
                .iter()
                .position(|row| &row.model == current)
        })
        .unwrap_or(0);
    if !state.model_catalog_loaded && !state.model_catalog_refreshing {
        state.model_catalog_refresh_provider = None;
        state.model_catalog_refresh_requested = true;
    }
}

pub fn open_review_model_picker(state: &mut crate::app::AppState, selected: Option<&ModelInfo>) {
    state.model_picker_target = crate::app::ModelPickerTarget::Review;
    state.model_picker_open = true;
    state.model_filter.clear();
    state.selected_model_idx = selected
        .and_then(|selected| {
            visible_model_rows(state)
                .iter()
                .position(|row| &row.model == selected)
        })
        .unwrap_or(0);
    if !state.model_catalog_loaded && !state.model_catalog_refreshing {
        state.model_catalog_refresh_provider = None;
        state.model_catalog_refresh_requested = true;
    }
}

pub fn open_delegation_model_picker(
    state: &mut crate::app::AppState,
    selected: Option<&crate::config::DelegationRoute>,
    configured: &[crate::config::DelegationRoute],
) {
    state.model_picker_target = crate::app::ModelPickerTarget::Delegation;
    state.model_picker_open = true;
    state.model_filter.clear();
    state.delegation_model_routes = configured
        .iter()
        .map(|route| (route.provider.clone(), route.model.clone()))
        .collect();
    state.selected_model_idx = selected
        .and_then(|selected| {
            visible_model_rows(state).iter().position(|row| {
                row.model.provider == selected.provider && row.model.id == selected.model
            })
        })
        .map_or(0, |index| index + 1);
    if !state.model_catalog_loaded && !state.model_catalog_refreshing {
        state.model_catalog_refresh_provider = None;
        state.model_catalog_refresh_requested = true;
    }
}

pub fn model_matches_filter(model: &ModelInfo, filter_lower: &str) -> bool {
    crate::model_preferences::model_matches_filter(model, filter_lower)
}

pub fn visible_model_rows(
    state: &crate::app::AppState,
) -> Vec<crate::model_preferences::ModelPickerRow> {
    let mut rows = crate::model_preferences::picker_rows(
        &state.model_list,
        &state.model_preferences,
        &state.model_filter,
    );
    if matches!(
        state.model_picker_target,
        crate::app::ModelPickerTarget::Delegation
    ) {
        rows.retain(|row| {
            state
                .delegation_model_routes
                .contains(&(row.model.provider.clone(), row.model.id.clone()))
        });
    }
    rows
}

fn visible_model_count(state: &crate::app::AppState) -> usize {
    visible_model_rows(state).len() + usize::from(delegation_inherit_visible(state))
}

pub(crate) fn selected_picker_model(state: &crate::app::AppState) -> Option<ModelInfo> {
    let index = state
        .selected_model_idx
        .checked_sub(usize::from(delegation_inherit_visible(state)))?;
    visible_model_rows(state)
        .get(index)
        .map(|row| row.model.clone())
}

pub fn delegation_inherit_visible(state: &crate::app::AppState) -> bool {
    if !matches!(
        state.model_picker_target,
        crate::app::ModelPickerTarget::Delegation
    ) {
        return false;
    }
    let filter = state.model_filter.trim().to_ascii_lowercase();
    filter.is_empty() || "same as current agent inherit parent default".contains(filter.as_str())
}

fn queue_scoped_model_cycle(
    state: &mut crate::app::AppState,
    models: &[ModelInfo],
    empty_message: &str,
) {
    if models.is_empty() {
        state.status_message = Some(empty_message.into());
        return;
    }
    let next = state
        .current_model
        .as_ref()
        .and_then(|current| models.iter().position(|model| model == current))
        .map_or(0, |index| (index + 1) % models.len());
    state.pending_input_action = Some(crate::tui::commands::InputAction::SelectModel(
        models[next].clone(),
    ));
}

pub fn open_provider_manager(
    state: &mut crate::app::AppState,
    entries: Vec<crate::config::ProviderManagementEntry>,
    filter: String,
) {
    let current_provider = state
        .current_model
        .as_ref()
        .map(|model| model.provider.as_str());
    let mut entries = entries;
    entries.sort_by(|left, right| {
        let left_current = current_provider == Some(left.id.as_str());
        let right_current = current_provider == Some(right.id.as_str());
        right_current
            .cmp(&left_current)
            .then_with(|| provider_display_rank(&left.id).cmp(&provider_display_rank(&right.id)))
            .then_with(|| left.id.cmp(&right.id))
    });
    let selected_provider_idx = entries
        .iter()
        .filter(|entry| provider_matches_filter(entry, &filter.to_ascii_lowercase()))
        .position(|entry| current_provider == Some(entry.id.as_str()))
        .unwrap_or(0);
    state.provider_manager = Some(crate::app::ProviderManagerState {
        view: crate::app::ProviderManagerView::Providers,
        providers: entries,
        provider_filter: filter,
        selected_provider_idx,
        model_filter: String::new(),
        selected_model_idx: 0,
        template_filter: String::new(),
        selected_template_idx: 0,
        delete_confirmation: None,
        busy: false,
        error: None,
    });
}

pub fn open_provider_setup(
    state: &mut crate::app::AppState,
    entries: Vec<crate::config::ProviderManagementEntry>,
    filter: String,
) {
    open_provider_manager(state, entries, filter);
    if let Some(manager) = state.provider_manager.as_mut() {
        manager.view = crate::app::ProviderManagerView::SetupProviders;
    }
}

pub fn open_provider_discovery(state: &mut crate::app::AppState) {
    state.provider_manager = None;
    state.model_picker_open = false;
    state.provider_discovery = Some(crate::app::ProviderDiscoveryState {
        candidates: Vec::new(),
        selected_idx: 0,
        scanning: true,
        error: None,
    });
    state.provider_discovery_requested = true;
}

fn handle_provider_discovery(
    state: &mut crate::app::AppState,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> bool {
    match code {
        KeyCode::Esc => {
            state.provider_discovery = None;
            state.provider_discovery_requested = false;
            state.provider_discovery_generation =
                state.provider_discovery_generation.wrapping_add(1);
        }
        KeyCode::Char('R') | KeyCode::Char('r') if modifiers.contains(KeyModifiers::SHIFT) => {
            if let Some(discovery) = state.provider_discovery.as_mut() {
                discovery.candidates.clear();
                discovery.selected_idx = 0;
                discovery.scanning = true;
                discovery.error = None;
                state.provider_discovery_requested = true;
            }
        }
        KeyCode::Enter => {
            if let Some(candidate) = state
                .provider_discovery
                .as_ref()
                .filter(|discovery| !discovery.scanning)
                .and_then(|discovery| discovery.candidates.get(discovery.selected_idx))
                .cloned()
            {
                state.pending_input_action =
                    Some(crate::tui::commands::InputAction::OpenProviderEditor(
                        crate::app::ProviderEditorRequest::AddDiscoveredProvider(candidate),
                    ));
            }
        }
        KeyCode::Up => {
            if let Some(discovery) = state.provider_discovery.as_mut() {
                discovery.selected_idx = discovery.selected_idx.saturating_sub(1);
            }
        }
        KeyCode::Down => {
            if let Some(discovery) = state.provider_discovery.as_mut() {
                discovery.selected_idx = discovery
                    .selected_idx
                    .saturating_add(1)
                    .min(discovery.candidates.len().saturating_sub(1));
            }
        }
        KeyCode::Home => {
            if let Some(discovery) = state.provider_discovery.as_mut() {
                discovery.selected_idx = 0;
            }
        }
        KeyCode::End => {
            if let Some(discovery) = state.provider_discovery.as_mut() {
                discovery.selected_idx = discovery.candidates.len().saturating_sub(1);
            }
        }
        KeyCode::PageUp => {
            if let Some(discovery) = state.provider_discovery.as_mut() {
                discovery.selected_idx = discovery.selected_idx.saturating_sub(8);
            }
        }
        KeyCode::PageDown => {
            if let Some(discovery) = state.provider_discovery.as_mut() {
                discovery.selected_idx = discovery
                    .selected_idx
                    .saturating_add(8)
                    .min(discovery.candidates.len().saturating_sub(1));
            }
        }
        _ => {}
    }
    true
}

pub fn refresh_provider_manager_entries(
    state: &mut crate::app::AppState,
    entries: Vec<crate::config::ProviderManagementEntry>,
    selected_provider: Option<&str>,
) {
    let Some(manager) = state.provider_manager.as_mut() else {
        return;
    };
    let current_provider = state
        .current_model
        .as_ref()
        .map(|model| model.provider.as_str());
    let mut entries = entries;
    entries.sort_by(|left, right| {
        let left_current = current_provider == Some(left.id.as_str());
        let right_current = current_provider == Some(right.id.as_str());
        right_current
            .cmp(&left_current)
            .then_with(|| provider_display_rank(&left.id).cmp(&provider_display_rank(&right.id)))
            .then_with(|| left.id.cmp(&right.id))
    });
    manager.providers = entries;
    manager.busy = false;
    manager.delete_confirmation = None;
    manager.error = None;
    let target = selected_provider.or(current_provider);
    let filter = manager.provider_filter.to_ascii_lowercase();
    manager.selected_provider_idx = manager
        .providers
        .iter()
        .filter(|entry| provider_matches_filter(entry, &filter))
        .position(|entry| target == Some(entry.id.as_str()))
        .unwrap_or(0);
}

fn handle_provider_manager(
    state: &mut crate::app::AppState,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> bool {
    if state.provider_check_running.is_some() {
        if code == KeyCode::Esc {
            state.pending_input_action =
                Some(crate::tui::commands::InputAction::CancelProviderCheck);
        }
        return true;
    }
    if state
        .provider_manager
        .as_ref()
        .is_some_and(|manager| manager.busy)
    {
        return true;
    }
    if state
        .provider_manager
        .as_ref()
        .and_then(|manager| manager.delete_confirmation.as_ref())
        .is_some()
    {
        return handle_provider_delete_confirmation(state, code);
    }
    let view = state
        .provider_manager
        .as_ref()
        .map(|manager| manager.view.clone());
    match view {
        Some(crate::app::ProviderManagerView::Providers) => {
            handle_provider_list(state, code, modifiers)
        }
        Some(crate::app::ProviderManagerView::Models { provider }) => {
            handle_provider_models(state, &provider, code, modifiers)
        }
        Some(crate::app::ProviderManagerView::Templates { return_to }) => {
            handle_provider_templates(state, return_to, code, modifiers)
        }
        Some(crate::app::ProviderManagerView::SetupProviders) => {
            handle_provider_setup_list(state, code, modifiers)
        }
        Some(crate::app::ProviderManagerView::SetupProvider { provider }) => {
            handle_provider_setup_provider(state, &provider, code, modifiers)
        }
        Some(crate::app::ProviderManagerView::SetupFinish) => {
            handle_provider_setup_finish(state, code)
        }
        None => false,
    }
}

fn handle_provider_setup_list(
    state: &mut crate::app::AppState,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> bool {
    let selected = selected_provider_entry(state).cloned();
    let has_passing_provider = !checked_provider_models(state).is_empty();
    let Some(manager) = state.provider_manager.as_mut() else {
        return false;
    };
    manager.error = None;
    match code {
        KeyCode::Esc => state.provider_manager = None,
        KeyCode::Enter => {
            if let Some(entry) = selected {
                manager.view =
                    crate::app::ProviderManagerView::SetupProvider { provider: entry.id };
            } else {
                manager.error = Some("No matching providers".into());
            }
        }
        KeyCode::Up => {
            manager.selected_provider_idx = crate::tui::selection::previous_wrap(
                manager.selected_provider_idx,
                visible_provider_count(manager),
            )
        }
        KeyCode::Down => {
            manager.selected_provider_idx = crate::tui::selection::next_wrap(
                manager.selected_provider_idx,
                visible_provider_count(manager),
            );
        }
        KeyCode::Home => manager.selected_provider_idx = 0,
        KeyCode::End => {
            manager.selected_provider_idx = visible_provider_count(manager).saturating_sub(1)
        }
        KeyCode::PageUp => {
            manager.selected_provider_idx =
                crate::tui::selection::page_up(manager.selected_provider_idx, 8)
        }
        KeyCode::PageDown => {
            manager.selected_provider_idx = crate::tui::selection::page_down(
                manager.selected_provider_idx,
                visible_provider_count(manager),
                8,
            );
        }
        KeyCode::Char('T') | KeyCode::Char('t') if modifiers.contains(KeyModifiers::SHIFT) => {
            if let Some(entry) = selected {
                state.pending_input_action =
                    Some(crate::tui::commands::InputAction::CheckProvider(entry.id));
            }
        }
        KeyCode::Char('E') | KeyCode::Char('e') if modifiers.contains(KeyModifiers::SHIFT) => {
            if let Some(entry) = selected {
                state.pending_input_action =
                    Some(crate::tui::commands::InputAction::OpenProviderEditor(
                        crate::app::ProviderEditorRequest::EditProvider(entry.id),
                    ));
            }
        }
        KeyCode::Char('A') | KeyCode::Char('a') if modifiers.contains(KeyModifiers::SHIFT) => {
            state.pending_input_action =
                Some(crate::tui::commands::InputAction::OpenProviderEditor(
                    crate::app::ProviderEditorRequest::AddProvider,
                ));
        }
        KeyCode::Char('O') | KeyCode::Char('o') if modifiers.contains(KeyModifiers::SHIFT) => {
            state.pending_input_action =
                Some(crate::tui::commands::InputAction::OpenProviderEditor(
                    crate::app::ProviderEditorRequest::AddOpenAiProvider,
                ));
        }
        KeyCode::Char('P') | KeyCode::Char('p') if modifiers.contains(KeyModifiers::SHIFT) => {
            manager.view = crate::app::ProviderManagerView::Templates {
                return_to: crate::app::ProviderTemplateReturn::SetupProviders,
            };
            manager.template_filter.clear();
            manager.selected_template_idx = 0;
        }
        KeyCode::Char('D') | KeyCode::Char('d') if modifiers.contains(KeyModifiers::SHIFT) => {
            if let Some(entry) = selected {
                state.pending_input_action = Some(
                    crate::tui::commands::InputAction::RequestDeleteProvider(entry.id),
                );
            }
        }
        KeyCode::Char('F') | KeyCode::Char('f') if modifiers.contains(KeyModifiers::SHIFT) => {
            if !has_passing_provider {
                manager.error = Some("Test at least one working provider before finishing".into());
            } else {
                manager.view = crate::app::ProviderManagerView::SetupFinish;
                manager.selected_model_idx = 0;
            }
        }
        KeyCode::Char(character)
            if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            manager.provider_filter.push(character);
            manager.selected_provider_idx = 0;
        }
        KeyCode::Backspace => {
            manager.provider_filter.pop();
            manager.selected_provider_idx = 0;
        }
        _ => {}
    }
    true
}

fn handle_provider_setup_provider(
    state: &mut crate::app::AppState,
    provider: &str,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> bool {
    let fallback_context = provider_models(state, provider).first().copied().cloned();
    let Some(manager) = state.provider_manager.as_mut() else {
        return false;
    };
    manager.error = None;
    match code {
        KeyCode::Esc => manager.view = crate::app::ProviderManagerView::SetupProviders,
        KeyCode::Char('T') | KeyCode::Char('t') if modifiers.contains(KeyModifiers::SHIFT) => {
            state.pending_input_action = Some(crate::tui::commands::InputAction::CheckProvider(
                provider.to_string(),
            ));
        }
        KeyCode::Char('E') | KeyCode::Char('e') if modifiers.contains(KeyModifiers::SHIFT) => {
            state.pending_input_action =
                Some(crate::tui::commands::InputAction::OpenProviderEditor(
                    crate::app::ProviderEditorRequest::EditProvider(provider.to_string()),
                ));
        }
        KeyCode::Char('A') | KeyCode::Char('a') if modifiers.contains(KeyModifiers::SHIFT) => {
            let context = fallback_context.unwrap_or_else(|| ModelInfo {
                id: String::new(),
                provider: provider.to_string(),
                display_name: String::new(),
            });
            state.pending_input_action =
                Some(crate::tui::commands::InputAction::OpenProviderEditor(
                    crate::app::ProviderEditorRequest::AddModel(context),
                ));
        }
        KeyCode::Char('R') | KeyCode::Char('r')
            if modifiers.contains(KeyModifiers::SHIFT) && !state.model_catalog_refreshing =>
        {
            state.model_catalog_refresh_provider = Some(provider.to_string());
            state.model_catalog_refresh_requested = true;
        }
        _ => {}
    }
    true
}

fn handle_provider_setup_finish(state: &mut crate::app::AppState, code: KeyCode) -> bool {
    let passing = checked_provider_models(state);
    let Some(manager) = state.provider_manager.as_mut() else {
        return false;
    };
    manager.error = None;
    match code {
        KeyCode::Esc => manager.view = crate::app::ProviderManagerView::SetupProviders,
        KeyCode::Up => manager.selected_model_idx = manager.selected_model_idx.saturating_sub(1),
        KeyCode::Down => {
            if manager.selected_model_idx.saturating_add(1) < passing.len() {
                manager.selected_model_idx += 1;
            }
        }
        KeyCode::Home => manager.selected_model_idx = 0,
        KeyCode::End => manager.selected_model_idx = passing.len().saturating_sub(1),
        KeyCode::Enter => {
            if let Some(model) = passing.get(manager.selected_model_idx).cloned() {
                manager.busy = true;
                state.pending_input_action = Some(
                    crate::tui::commands::InputAction::SelectCheckedProvider(model),
                );
            } else {
                manager.error = Some("No working providers are available".into());
            }
        }
        _ => {}
    }
    true
}

fn checked_provider_models(state: &crate::app::AppState) -> Vec<ModelInfo> {
    let Some(manager) = state.provider_manager.as_ref() else {
        return Vec::new();
    };
    manager
        .providers
        .iter()
        .filter_map(|entry| {
            state
                .provider_checks
                .get(&entry.id)
                .and_then(crate::provider_probe::ProviderCheckStatus::working_model)
                .map(|model| ModelInfo {
                    id: model.to_string(),
                    provider: entry.id.clone(),
                    display_name: model.to_string(),
                })
        })
        .collect()
}

fn handle_provider_list(
    state: &mut crate::app::AppState,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> bool {
    let selected = selected_provider_entry(state).cloned();
    let selected_model_idx = selected.as_ref().map_or(0, |entry| {
        provider_models(state, &entry.id)
            .iter()
            .position(|model| state.current_model.as_ref() == Some(*model))
            .unwrap_or(0)
    });
    let Some(manager) = state.provider_manager.as_mut() else {
        return false;
    };
    manager.error = None;
    match code {
        KeyCode::Esc => state.provider_manager = None,
        KeyCode::Enter => {
            if let Some(entry) = selected {
                if let Some(provider) =
                    crate::provider_connect::FirstClassProvider::for_id(&entry.id)
                {
                    if !entry.connected {
                        begin_provider_auth(state, provider);
                        return true;
                    }
                }
                manager.view = crate::app::ProviderManagerView::Models {
                    provider: entry.id.clone(),
                };
                manager.model_filter.clear();
                manager.selected_model_idx = selected_model_idx;
                if !state.model_catalog_loaded && !state.model_catalog_refreshing {
                    state.model_catalog_refresh_requested = true;
                }
            } else {
                manager.error = Some("No matching providers".into());
            }
        }
        KeyCode::Up => {
            manager.selected_provider_idx = crate::tui::selection::previous_wrap(
                manager.selected_provider_idx,
                visible_provider_count(manager),
            )
        }
        KeyCode::Down => {
            manager.selected_provider_idx = crate::tui::selection::next_wrap(
                manager.selected_provider_idx,
                visible_provider_count(manager),
            );
        }
        KeyCode::Home => manager.selected_provider_idx = 0,
        KeyCode::End => {
            manager.selected_provider_idx = visible_provider_count(manager).saturating_sub(1)
        }
        KeyCode::PageUp => {
            manager.selected_provider_idx =
                crate::tui::selection::page_up(manager.selected_provider_idx, 8)
        }
        KeyCode::PageDown => {
            manager.selected_provider_idx = crate::tui::selection::page_down(
                manager.selected_provider_idx,
                visible_provider_count(manager),
                8,
            );
        }
        KeyCode::Char('A') | KeyCode::Char('a') if modifiers.contains(KeyModifiers::SHIFT) => {
            state.pending_input_action =
                Some(crate::tui::commands::InputAction::OpenProviderEditor(
                    crate::app::ProviderEditorRequest::AddProvider,
                ));
        }
        KeyCode::Char('O') | KeyCode::Char('o') if modifiers.contains(KeyModifiers::SHIFT) => {
            state.pending_input_action =
                Some(crate::tui::commands::InputAction::OpenProviderEditor(
                    crate::app::ProviderEditorRequest::AddOpenAiProvider,
                ));
        }
        KeyCode::Char('P') | KeyCode::Char('p') if modifiers.contains(KeyModifiers::SHIFT) => {
            manager.view = crate::app::ProviderManagerView::Templates {
                return_to: crate::app::ProviderTemplateReturn::Providers,
            };
            manager.template_filter.clear();
            manager.selected_template_idx = 0;
        }
        KeyCode::Char('E') | KeyCode::Char('e') if modifiers.contains(KeyModifiers::SHIFT) => {
            if let Some(entry) = selected {
                state.pending_input_action =
                    Some(crate::tui::commands::InputAction::OpenProviderEditor(
                        crate::app::ProviderEditorRequest::EditProvider(entry.id),
                    ));
            }
        }
        KeyCode::Char('D') | KeyCode::Char('d') if modifiers.contains(KeyModifiers::SHIFT) => {
            if let Some(entry) = selected {
                state.pending_input_action = Some(
                    crate::tui::commands::InputAction::RequestDeleteProvider(entry.id),
                );
            }
        }
        KeyCode::Char('L') | KeyCode::Char('l') if modifiers.contains(KeyModifiers::SHIFT) => {
            if let Some(provider) = selected
                .as_ref()
                .and_then(|entry| crate::provider_connect::FirstClassProvider::for_id(&entry.id))
            {
                begin_provider_auth(state, provider);
                return true;
            }
        }
        KeyCode::Char(character)
            if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            manager.provider_filter.push(character);
            manager.selected_provider_idx = 0;
        }
        KeyCode::Backspace => {
            manager.provider_filter.pop();
            manager.selected_provider_idx = 0;
        }
        _ => {}
    }
    true
}

pub(crate) fn begin_provider_auth(
    state: &mut crate::app::AppState,
    provider: crate::provider_connect::FirstClassProvider,
) {
    state.provider_auth_generation = state.provider_auth_generation.wrapping_add(1);
    let saved_mimo_region = (provider == crate::provider_connect::FirstClassProvider::Mimo)
        .then(|| {
            state.provider_manager.as_ref().and_then(|manager| {
                manager
                    .providers
                    .iter()
                    .find(|entry| entry.id == provider.id() && entry.connected)
                    .and_then(|entry| {
                        crate::pricing::MimoRegion::from_token_plan_url(&entry.base_url)
                    })
            })
        })
        .flatten();
    let direct = matches!(
        provider,
        crate::provider_connect::FirstClassProvider::DeepSeek
            | crate::provider_connect::FirstClassProvider::OpenAi
    );
    let prompt = match provider {
        crate::provider_connect::FirstClassProvider::DeepSeek => "Paste your DeepSeek API key",
        crate::provider_connect::FirstClassProvider::OpenAi => "Paste your OpenAI API key",
        crate::provider_connect::FirstClassProvider::Mimo if saved_mimo_region.is_some() => {
            "Reuse the saved Token Plan credential, authorize in a browser, or enter a key"
        }
        crate::provider_connect::FirstClassProvider::Mimo => {
            "Choose browser authorization or a manual Token Plan key"
        }
        crate::provider_connect::FirstClassProvider::OpenAiCodex => {
            "Preparing OpenAI authorization…"
        }
    };
    let bare = !state.run_capabilities.config_writes;
    state.provider_auth = Some(crate::app::ProviderAuthState {
        provider,
        stage: if provider == crate::provider_connect::FirstClassProvider::Mimo {
            crate::app::ProviderAuthStage::MethodPicker
        } else if direct {
            crate::app::ProviderAuthStage::SecretInput
        } else {
            crate::app::ProviderAuthStage::Starting
        },
        url: None,
        code: None,
        prompt: prompt.into(),
        input: crate::provider_secrets::SecretValue::default(),
        cursor: 0,
        metadata_input: String::new(),
        metadata_cursor: 0,
        selected_index: 0,
        mimo_method: None,
        saved_mimo_region,
        mimo_region: None,
        mimo_tier: None,
        mimo_term: None,
        billing: None,
        error: None,
        bare,
        generation: state.provider_auth_generation,
    });
    if !direct && provider != crate::provider_connect::FirstClassProvider::Mimo && !bare {
        state.pending_input_action = Some(crate::tui::commands::InputAction::StartProviderAuth(
            provider,
        ));
    }
}

fn provider_display_rank(provider: &str) -> usize {
    ["mimo", "openai-codex", "deepseek", "openai"]
        .iter()
        .position(|candidate| *candidate == provider)
        .unwrap_or(usize::MAX)
}

fn handle_provider_auth(
    state: &mut crate::app::AppState,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> bool {
    let Some(auth) = state.provider_auth.as_mut() else {
        return false;
    };
    match code {
        KeyCode::Esc => {
            state.pending_input_action =
                Some(crate::tui::commands::InputAction::CancelProviderAuth);
            state.provider_auth = None;
        }
        KeyCode::Up
            if matches!(
                auth.stage,
                crate::app::ProviderAuthStage::MethodPicker
                    | crate::app::ProviderAuthStage::RegionPicker
                    | crate::app::ProviderAuthStage::PlanPicker
                    | crate::app::ProviderAuthStage::TermPicker
            ) =>
        {
            auth.selected_index = auth.selected_index.saturating_sub(1);
        }
        KeyCode::Down
            if matches!(
                auth.stage,
                crate::app::ProviderAuthStage::MethodPicker
                    | crate::app::ProviderAuthStage::RegionPicker
                    | crate::app::ProviderAuthStage::PlanPicker
                    | crate::app::ProviderAuthStage::TermPicker
            ) =>
        {
            let count = match auth.stage {
                crate::app::ProviderAuthStage::MethodPicker => auth.mimo_method_options().len(),
                crate::app::ProviderAuthStage::RegionPicker => {
                    crate::pricing::MimoRegion::ALL.len()
                }
                crate::app::ProviderAuthStage::PlanPicker => {
                    crate::pricing::MimoPlanTier::ALL.len()
                }
                crate::app::ProviderAuthStage::TermPicker => {
                    crate::pricing::MimoPlanTerm::ALL.len()
                }
                _ => 0,
            };
            auth.selected_index = auth
                .selected_index
                .saturating_add(1)
                .min(count.saturating_sub(1));
        }
        KeyCode::Enter if auth.stage == crate::app::ProviderAuthStage::MethodPicker => {
            if let Some(method) = auth.mimo_method_options().get(auth.selected_index).copied() {
                auth.mimo_method = Some(method);
                match method {
                    crate::app::MimoSetupMethod::SavedTokenPlan => {
                        auth.mimo_region = auth.saved_mimo_region;
                        auth.stage = crate::app::ProviderAuthStage::PlanPicker;
                        auth.selected_index = 0;
                        auth.prompt = "Select the tier for the saved Token Plan credential".into();
                    }
                    crate::app::MimoSetupMethod::Browser => {
                        auth.mimo_region = None;
                        auth.stage = crate::app::ProviderAuthStage::Starting;
                        auth.prompt = if auth.bare {
                            "Browser authorization will remain memory-only; press Enter to continue"
                                .into()
                        } else {
                            "Preparing Xiaomi browser authorization…".into()
                        };
                        if !auth.bare {
                            state.pending_input_action = Some(
                                crate::tui::commands::InputAction::StartProviderAuth(auth.provider),
                            );
                        }
                    }
                    crate::app::MimoSetupMethod::ManualTokenPlan => {
                        auth.mimo_region = None;
                        auth.stage = crate::app::ProviderAuthStage::RegionPicker;
                        auth.selected_index = 0;
                        auth.prompt =
                            "Select the region shown on your Xiaomi Token Plan page".into();
                    }
                }
            }
        }
        KeyCode::Enter if auth.stage == crate::app::ProviderAuthStage::RegionPicker => {
            auth.mimo_region = crate::pricing::MimoRegion::ALL
                .get(auth.selected_index)
                .copied();
            auth.stage = crate::app::ProviderAuthStage::PlanPicker;
            auth.selected_index = 0;
            auth.prompt = "Select your Token Plan tier".into();
        }
        KeyCode::Enter if auth.stage == crate::app::ProviderAuthStage::PlanPicker => {
            auth.mimo_tier = crate::pricing::MimoPlanTier::ALL
                .get(auth.selected_index)
                .copied();
            auth.stage = crate::app::ProviderAuthStage::TermPicker;
            auth.selected_index = 0;
            auth.prompt = "Select your Token Plan billing term".into();
        }
        KeyCode::Enter if auth.stage == crate::app::ProviderAuthStage::TermPicker => {
            auth.mimo_term = crate::pricing::MimoPlanTerm::ALL
                .get(auth.selected_index)
                .copied();
            if auth.mimo_method == Some(crate::app::MimoSetupMethod::ManualTokenPlan) {
                auth.stage = crate::app::ProviderAuthStage::SecretInput;
                auth.prompt = "Paste the Token Plan API key (tp-…)".into();
            } else {
                begin_mimo_usd_input(auth);
            }
        }
        KeyCode::Enter if auth.stage == crate::app::ProviderAuthStage::SecretInput => {
            if auth.input.is_empty() {
                auth.error = Some("A credential or authorization result is required".into());
            } else if auth.mimo_method == Some(crate::app::MimoSetupMethod::ManualTokenPlan)
                && !auth.input.with_secret(crate::pricing::valid_token_plan_key)
            {
                auth.error = Some("Token Plan keys must use the tp-… format".into());
            } else if auth.mimo_method == Some(crate::app::MimoSetupMethod::ManualTokenPlan) {
                begin_mimo_usd_input(auth);
            } else {
                auth.stage = crate::app::ProviderAuthStage::Probing;
                auth.error = None;
                state.pending_input_action =
                    Some(crate::tui::commands::InputAction::SubmitProviderAuth);
            }
        }
        KeyCode::Enter if auth.stage == crate::app::ProviderAuthStage::PlanUsdInput => {
            let parsed = auth.metadata_input.parse::<crate::usage::DecimalUsd>();
            match parsed {
                Ok(actual) if actual != crate::usage::DecimalUsd::ZERO => {
                    let tier = auth.mimo_tier.expect("Token Plan tier was selected");
                    let term = auth.mimo_term.expect("Token Plan term was selected");
                    let official = crate::pricing::mimo_plan_list_price(tier, term);
                    if actual == official {
                        auth.billing = crate::pricing::ProviderBillingConfig::mimo_token_plan(
                            auth.mimo_region.expect("Token Plan region was selected"),
                            tier,
                            term,
                            actual,
                            false,
                        )
                        .ok();
                        auth.stage = crate::app::ProviderAuthStage::Probing;
                        auth.error = None;
                        state.pending_input_action =
                            Some(crate::tui::commands::InputAction::SubmitProviderAuth);
                    } else {
                        auth.stage = crate::app::ProviderAuthStage::PriceConfirmation;
                        auth.selected_index = 0;
                        auth.prompt = format!(
                            "Actual ${actual} differs from official list ${official}. Confirm this subscription allocation?"
                        );
                        auth.error = None;
                    }
                }
                _ => auth.error = Some("Enter a positive USD amount using decimal digits".into()),
            }
        }
        KeyCode::Left | KeyCode::Right
            if auth.stage == crate::app::ProviderAuthStage::PriceConfirmation =>
        {
            auth.selected_index = usize::from(auth.selected_index == 0);
        }
        KeyCode::Enter if auth.stage == crate::app::ProviderAuthStage::PriceConfirmation => {
            if auth.selected_index == 0 {
                auth.stage = crate::app::ProviderAuthStage::PlanUsdInput;
                auth.error = Some("Price difference was not confirmed".into());
            } else if let Ok(actual) = auth.metadata_input.parse::<crate::usage::DecimalUsd>() {
                auth.billing = crate::pricing::ProviderBillingConfig::mimo_token_plan(
                    auth.mimo_region.expect("Token Plan region was selected"),
                    auth.mimo_tier.expect("Token Plan tier was selected"),
                    auth.mimo_term.expect("Token Plan term was selected"),
                    actual,
                    true,
                )
                .ok();
                auth.stage = crate::app::ProviderAuthStage::Probing;
                auth.error = None;
                state.pending_input_action =
                    Some(crate::tui::commands::InputAction::SubmitProviderAuth);
            }
        }
        KeyCode::Enter if auth.stage == crate::app::ProviderAuthStage::Starting && auth.bare => {
            auth.stage = crate::app::ProviderAuthStage::WaitingBrowser;
            auth.prompt = "Preparing memory-only authorization…".into();
            state.pending_input_action = Some(
                crate::tui::commands::InputAction::StartProviderAuth(auth.provider),
            );
        }
        KeyCode::Enter if auth.stage == crate::app::ProviderAuthStage::Failed => {
            let provider = auth.provider;
            begin_provider_auth(state, provider);
        }
        KeyCode::Char('y') if modifiers == KeyModifiers::CONTROL && auth.url.is_some() => {
            state.pending_input_action =
                Some(crate::tui::commands::InputAction::CopyProviderAuthUrl(
                    auth.url.clone().unwrap_or_default(),
                ));
            auth.prompt = "Opening a borderless authorization URL handoff…".into();
            auth.error = None;
        }
        KeyCode::Left if auth.stage == crate::app::ProviderAuthStage::SecretInput => {
            auth.cursor = auth.input.previous_char_boundary(auth.cursor);
        }
        KeyCode::Right if auth.stage == crate::app::ProviderAuthStage::SecretInput => {
            auth.cursor = auth.input.next_char_boundary(auth.cursor);
        }
        KeyCode::Home if auth.stage == crate::app::ProviderAuthStage::SecretInput => {
            auth.cursor = 0;
        }
        KeyCode::End if auth.stage == crate::app::ProviderAuthStage::SecretInput => {
            auth.cursor = auth.input.len();
        }
        KeyCode::Backspace if auth.stage == crate::app::ProviderAuthStage::SecretInput => {
            let previous = auth.input.previous_char_boundary(auth.cursor);
            auth.input.remove(previous..auth.cursor);
            auth.cursor = previous;
            auth.error = None;
        }
        KeyCode::Delete if auth.stage == crate::app::ProviderAuthStage::SecretInput => {
            let next = auth.input.next_char_boundary(auth.cursor);
            auth.input.remove(auth.cursor..next);
            auth.error = None;
        }
        KeyCode::Char(character)
            if auth.stage == crate::app::ProviderAuthStage::SecretInput
                && !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                && auth.input.len() < 96 * 1024 =>
        {
            auth.cursor = auth.input.insert_char(auth.cursor, character);
            auth.error = None;
        }
        KeyCode::Left if auth.stage == crate::app::ProviderAuthStage::PlanUsdInput => {
            auth.metadata_cursor =
                previous_char_boundary(&auth.metadata_input, auth.metadata_cursor);
        }
        KeyCode::Right if auth.stage == crate::app::ProviderAuthStage::PlanUsdInput => {
            auth.metadata_cursor = next_char_boundary(&auth.metadata_input, auth.metadata_cursor);
        }
        KeyCode::Backspace if auth.stage == crate::app::ProviderAuthStage::PlanUsdInput => {
            let previous = previous_char_boundary(&auth.metadata_input, auth.metadata_cursor);
            auth.metadata_input
                .replace_range(previous..auth.metadata_cursor, "");
            auth.metadata_cursor = previous;
            auth.error = None;
        }
        KeyCode::Char(character)
            if auth.stage == crate::app::ProviderAuthStage::PlanUsdInput
                && !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                && (character.is_ascii_digit() || character == '.')
                && auth.metadata_input.len() < 32 =>
        {
            auth.metadata_input.insert(auth.metadata_cursor, character);
            auth.metadata_cursor += character.len_utf8();
            auth.error = None;
        }
        _ => {}
    }
    true
}

pub struct ProviderAuthSubmission {
    pub provider: crate::provider_connect::FirstClassProvider,
    pub secret: crate::provider_secrets::SecretValue,
    pub generation: u64,
    pub mimo_method: Option<crate::app::MimoSetupMethod>,
    pub billing: Option<crate::pricing::ProviderBillingConfig>,
    pub mimo_region: Option<crate::pricing::MimoRegion>,
}

pub fn take_provider_auth_secret(
    state: &mut crate::app::AppState,
) -> Option<ProviderAuthSubmission> {
    let auth = state.provider_auth.as_mut()?;
    Some(ProviderAuthSubmission {
        provider: auth.provider,
        secret: std::mem::take(&mut auth.input),
        generation: auth.generation,
        mimo_method: auth.mimo_method,
        billing: auth.billing.clone(),
        mimo_region: auth.mimo_region,
    })
}

fn begin_mimo_usd_input(auth: &mut crate::app::ProviderAuthState) {
    auth.stage = crate::app::ProviderAuthStage::PlanUsdInput;
    auth.metadata_input.clear();
    auth.metadata_cursor = 0;
    auth.error = None;
    let tier = auth.mimo_tier.expect("Token Plan tier was selected");
    let term = auth.mimo_term.expect("Token Plan term was selected");
    let official = crate::pricing::mimo_plan_list_price(tier, term);
    auth.prompt = format!(
        "Enter actual USD paid (official {} {} list: ${official})",
        tier.label(),
        term.label()
    );
}

pub fn filtered_provider_templates(
    manager: &crate::app::ProviderManagerState,
) -> Vec<&'static crate::provider_templates::ProviderTemplate> {
    crate::provider_templates::ALL
        .iter()
        .filter(|template| template.matches(&manager.template_filter))
        .collect()
}

fn handle_provider_templates(
    state: &mut crate::app::AppState,
    return_to: crate::app::ProviderTemplateReturn,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> bool {
    let selected = state.provider_manager.as_ref().and_then(|manager| {
        let visible = filtered_provider_templates(manager);
        visible
            .get(
                manager
                    .selected_template_idx
                    .min(visible.len().saturating_sub(1)),
            )
            .copied()
    });
    let Some(manager) = state.provider_manager.as_mut() else {
        return false;
    };
    manager.error = None;
    match code {
        KeyCode::Esc => {
            manager.view = match return_to {
                crate::app::ProviderTemplateReturn::Providers => {
                    crate::app::ProviderManagerView::Providers
                }
                crate::app::ProviderTemplateReturn::SetupProviders => {
                    crate::app::ProviderManagerView::SetupProviders
                }
            };
            manager.template_filter.clear();
            manager.selected_template_idx = 0;
        }
        KeyCode::Enter => match selected {
            Some(template)
                if manager
                    .providers
                    .iter()
                    .any(|entry| entry.id == template.provider_id) =>
            {
                manager.error = Some(format!(
                    "{} exists; template made no changes",
                    template.provider_id
                ));
            }
            Some(template) if !template.is_installable() => {
                manager.error = Some(template.availability.reason().to_string());
            }
            Some(template) => {
                state.pending_input_action =
                    Some(crate::tui::commands::InputAction::OpenProviderEditor(
                        crate::app::ProviderEditorRequest::AddProviderTemplate(
                            template.id.to_string(),
                        ),
                    ));
            }
            None => manager.error = Some("No matching provider templates".into()),
        },
        KeyCode::Up => {
            manager.selected_template_idx = manager.selected_template_idx.saturating_sub(1)
        }
        KeyCode::Down => {
            let count = filtered_provider_templates(manager).len();
            if manager.selected_template_idx.saturating_add(1) < count {
                manager.selected_template_idx += 1;
            }
        }
        KeyCode::Home => manager.selected_template_idx = 0,
        KeyCode::End => {
            manager.selected_template_idx =
                filtered_provider_templates(manager).len().saturating_sub(1)
        }
        KeyCode::PageUp => {
            manager.selected_template_idx = manager.selected_template_idx.saturating_sub(8)
        }
        KeyCode::PageDown => {
            let last = filtered_provider_templates(manager).len().saturating_sub(1);
            manager.selected_template_idx =
                manager.selected_template_idx.saturating_add(8).min(last);
        }
        KeyCode::Char(character)
            if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            manager.template_filter.push(character);
            manager.selected_template_idx = 0;
        }
        KeyCode::Backspace => {
            manager.template_filter.pop();
            manager.selected_template_idx = 0;
        }
        _ => {}
    }
    true
}

fn handle_provider_models(
    state: &mut crate::app::AppState,
    provider: &str,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> bool {
    let selected = selected_provider_model(state, provider).cloned();
    let visible_model_count = visible_provider_model_count(state, provider);
    let fallback_context = provider_models(state, provider).first().copied().cloned();
    let Some(manager) = state.provider_manager.as_mut() else {
        return false;
    };
    manager.error = None;
    match code {
        KeyCode::Esc => {
            manager.view = crate::app::ProviderManagerView::Providers;
            manager.model_filter.clear();
        }
        KeyCode::Enter => {
            if let Some(model) = selected {
                state.provider_manager = None;
                state.pending_input_action =
                    Some(crate::tui::commands::InputAction::SelectModel(model));
            } else {
                manager.error = Some("No matching models".into());
            }
        }
        KeyCode::Up => manager.selected_model_idx = manager.selected_model_idx.saturating_sub(1),
        KeyCode::Down => {
            if manager.selected_model_idx.saturating_add(1) < visible_model_count {
                manager.selected_model_idx += 1;
            }
        }
        KeyCode::Home => manager.selected_model_idx = 0,
        KeyCode::End => manager.selected_model_idx = visible_model_count.saturating_sub(1),
        KeyCode::PageUp => {
            manager.selected_model_idx = manager.selected_model_idx.saturating_sub(8)
        }
        KeyCode::PageDown => {
            let last = visible_model_count.saturating_sub(1);
            manager.selected_model_idx = manager.selected_model_idx.saturating_add(8).min(last);
        }
        KeyCode::Char('A') | KeyCode::Char('a') if modifiers.contains(KeyModifiers::SHIFT) => {
            let context = selected.or(fallback_context).unwrap_or_else(|| ModelInfo {
                id: String::new(),
                provider: provider.to_string(),
                display_name: String::new(),
            });
            state.pending_input_action =
                Some(crate::tui::commands::InputAction::OpenProviderEditor(
                    crate::app::ProviderEditorRequest::AddModel(context),
                ));
        }
        KeyCode::Char('E') | KeyCode::Char('e') if modifiers.contains(KeyModifiers::SHIFT) => {
            if let Some(model) = selected {
                state.pending_input_action =
                    Some(crate::tui::commands::InputAction::OpenProviderEditor(
                        crate::app::ProviderEditorRequest::EditModel(model),
                    ));
            }
        }
        KeyCode::Char('S') | KeyCode::Char('s') if modifiers.contains(KeyModifiers::SHIFT) => {
            state.pending_input_action =
                Some(crate::tui::commands::InputAction::OpenProviderEditor(
                    crate::app::ProviderEditorRequest::EditProvider(provider.to_string()),
                ));
        }
        KeyCode::Char('C') | KeyCode::Char('c') if modifiers.contains(KeyModifiers::SHIFT) => {
            if let Some(model) = selected {
                state.pending_input_action = Some(crate::tui::commands::InputAction::Command(
                    crate::tui::commands::CommandAction {
                        id: crate::tui::commands::CommandId::PromptCache,
                        args: format!("{}/{}", model.provider, model.id),
                    },
                ));
            } else {
                manager.error = Some("No matching models".into());
            }
        }
        KeyCode::Char('R') | KeyCode::Char('r') if modifiers.contains(KeyModifiers::SHIFT) => {
            if !state.model_catalog_refreshing {
                state.model_catalog_refresh_provider = Some(provider.to_string());
                state.model_catalog_refresh_requested = true;
            }
        }
        KeyCode::Char('D') | KeyCode::Char('d') if modifiers.contains(KeyModifiers::SHIFT) => {
            state.pending_input_action = Some(
                crate::tui::commands::InputAction::RequestDeleteProvider(provider.to_string()),
            );
        }
        KeyCode::Char(character)
            if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            manager.model_filter.push(character);
            manager.selected_model_idx = 0;
        }
        KeyCode::Backspace => {
            manager.model_filter.pop();
            manager.selected_model_idx = 0;
        }
        _ => {}
    }
    true
}

fn handle_provider_delete_confirmation(state: &mut crate::app::AppState, code: KeyCode) -> bool {
    let Some(manager) = state.provider_manager.as_mut() else {
        return false;
    };
    let Some(confirmation) = manager.delete_confirmation.as_mut() else {
        return false;
    };
    match code {
        KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
            manager.delete_confirmation = None;
        }
        KeyCode::Left | KeyCode::Right | KeyCode::Tab | KeyCode::BackTab => {
            confirmation.confirm_selected = !confirmation.confirm_selected;
        }
        KeyCode::Char('y') | KeyCode::Char('Y') => confirmation.confirm_selected = true,
        KeyCode::Enter => {
            if confirmation.confirm_selected {
                let provider = confirmation.provider.clone();
                let replacement = confirmation.replacement.clone();
                manager.busy = true;
                state.pending_input_action =
                    Some(crate::tui::commands::InputAction::DeleteProvider {
                        provider,
                        replacement,
                    });
            } else {
                manager.delete_confirmation = None;
            }
        }
        _ => {}
    }
    true
}

pub(crate) fn provider_matches_filter(
    entry: &crate::config::ProviderManagementEntry,
    filter_lower: &str,
) -> bool {
    filter_lower.is_empty()
        || entry.id.to_ascii_lowercase().contains(filter_lower)
        || crate::provider_connect::FirstClassProvider::for_id(&entry.id).is_some_and(|provider| {
            provider
                .display_name()
                .to_ascii_lowercase()
                .contains(filter_lower)
        })
}

fn visible_provider_count(manager: &crate::app::ProviderManagerState) -> usize {
    let filter = manager.provider_filter.to_ascii_lowercase();
    manager
        .providers
        .iter()
        .filter(|entry| provider_matches_filter(entry, &filter))
        .count()
}

fn selected_provider_entry(
    state: &crate::app::AppState,
) -> Option<&crate::config::ProviderManagementEntry> {
    let manager = state.provider_manager.as_ref()?;
    let filter = manager.provider_filter.to_ascii_lowercase();
    manager
        .providers
        .iter()
        .filter(|entry| provider_matches_filter(entry, &filter))
        .nth(manager.selected_provider_idx)
}

fn provider_models<'a>(state: &'a crate::app::AppState, provider: &str) -> Vec<&'a ModelInfo> {
    state
        .model_list
        .iter()
        .filter(|model| model.provider == provider)
        .collect()
}

fn visible_provider_model_count(state: &crate::app::AppState, provider: &str) -> usize {
    let Some(manager) = state.provider_manager.as_ref() else {
        return 0;
    };
    let filter = manager.model_filter.to_ascii_lowercase();
    provider_models(state, provider)
        .into_iter()
        .filter(|model| model_matches_filter(model, &filter))
        .count()
}

fn selected_provider_model<'a>(
    state: &'a crate::app::AppState,
    provider: &str,
) -> Option<&'a ModelInfo> {
    let manager = state.provider_manager.as_ref()?;
    let filter = manager.model_filter.to_ascii_lowercase();
    provider_models(state, provider)
        .into_iter()
        .filter(|model| model_matches_filter(model, &filter))
        .nth(manager.selected_model_idx)
}

fn handle_provider_editor(
    state: &mut crate::app::AppState,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> bool {
    use crate::app::ProviderEditorField;

    let Some(editor) = state.provider_editor.as_mut() else {
        return false;
    };
    if editor.saving {
        return true;
    }
    editor.error = None;
    match code {
        KeyCode::Esc => {
            let origin = editor.origin;
            state.provider_editor = None;
            if origin == crate::app::ProviderEditorOrigin::ModelPicker {
                state.model_picker_open = true;
            }
        }
        KeyCode::Enter => {
            state.pending_input_action = Some(
                if editor.mode == crate::app::ProviderEditorMode::EditProvider {
                    crate::tui::commands::InputAction::SaveProvider(editor.draft.clone())
                } else {
                    crate::tui::commands::InputAction::SaveProviderModel(editor.draft.clone())
                },
            );
        }
        KeyCode::Tab | KeyCode::BackTab => {
            let backwards = code == KeyCode::BackTab || modifiers.contains(KeyModifiers::SHIFT);
            let editing_existing = editor.mode != crate::app::ProviderEditorMode::AddProvider;
            let editing_provider = editor.mode == crate::app::ProviderEditorMode::EditProvider;
            editor.field = match (editing_existing, backwards, editor.field) {
                (_, false, ProviderEditorField::BaseUrl) if editing_provider => {
                    ProviderEditorField::ApiKey
                }
                (_, false, ProviderEditorField::ApiKey) if editing_provider => {
                    ProviderEditorField::ApiKeyEnv
                }
                (_, false, ProviderEditorField::ApiKeyEnv) if editing_provider => {
                    ProviderEditorField::BaseUrl
                }
                (_, false, ProviderEditorField::PromptCache) if editing_provider => {
                    ProviderEditorField::BaseUrl
                }
                (_, true, ProviderEditorField::BaseUrl) if editing_provider => {
                    ProviderEditorField::ApiKeyEnv
                }
                (_, true, ProviderEditorField::PromptCache) if editing_provider => {
                    ProviderEditorField::ApiKeyEnv
                }
                (_, true, ProviderEditorField::ApiKey) if editing_provider => {
                    ProviderEditorField::BaseUrl
                }
                (_, true, ProviderEditorField::ApiKeyEnv) if editing_provider => {
                    ProviderEditorField::ApiKey
                }
                (true, false, ProviderEditorField::BaseUrl) => ProviderEditorField::ApiKey,
                (true, false, ProviderEditorField::ApiKey) => ProviderEditorField::ApiKeyEnv,
                (true, false, ProviderEditorField::ApiKeyEnv) => ProviderEditorField::Model,
                (true, false, ProviderEditorField::Model) => ProviderEditorField::BaseUrl,
                (true, true, ProviderEditorField::BaseUrl) => ProviderEditorField::Model,
                (true, true, ProviderEditorField::ApiKey) => ProviderEditorField::BaseUrl,
                (true, true, ProviderEditorField::ApiKeyEnv) => ProviderEditorField::ApiKey,
                (true, true, ProviderEditorField::Model) => ProviderEditorField::ApiKeyEnv,
                (true, _, ProviderEditorField::PromptCache) => ProviderEditorField::BaseUrl,
                (false, false, ProviderEditorField::Provider) => ProviderEditorField::BaseUrl,
                (false, false, ProviderEditorField::BaseUrl) => ProviderEditorField::ApiKey,
                (false, false, ProviderEditorField::ApiKey) => ProviderEditorField::ApiKeyEnv,
                (false, false, ProviderEditorField::ApiKeyEnv) => ProviderEditorField::Model,
                (false, false, ProviderEditorField::Model) => ProviderEditorField::Provider,
                (false, true, ProviderEditorField::Provider) => ProviderEditorField::Model,
                (false, true, ProviderEditorField::BaseUrl) => ProviderEditorField::Provider,
                (false, true, ProviderEditorField::ApiKey) => ProviderEditorField::BaseUrl,
                (false, true, ProviderEditorField::ApiKeyEnv) => ProviderEditorField::ApiKey,
                (false, true, ProviderEditorField::Model) => ProviderEditorField::ApiKeyEnv,
                (false, _, ProviderEditorField::PromptCache) => ProviderEditorField::Provider,
                (true, _, ProviderEditorField::Provider) => ProviderEditorField::BaseUrl,
            };
            editor.cursor = provider_editor_value_len(editor);
        }
        KeyCode::Backspace => {
            let cursor = editor.cursor;
            let previous = provider_editor_previous_char_boundary(editor, cursor);
            if previous < cursor {
                remove_provider_editor_range(editor, previous..cursor);
            }
            editor.cursor = previous;
        }
        KeyCode::Delete => {
            let cursor = editor.cursor;
            let cursor = provider_editor_floor_char_boundary(editor, cursor);
            let next = provider_editor_next_char_boundary(editor, cursor);
            if cursor < next {
                remove_provider_editor_range(editor, cursor..next);
            }
            editor.cursor = cursor;
        }
        KeyCode::Left => {
            editor.cursor = provider_editor_previous_char_boundary(editor, editor.cursor);
        }
        KeyCode::Right => {
            editor.cursor = provider_editor_next_char_boundary(editor, editor.cursor);
        }
        KeyCode::Home => {
            editor.cursor = 0;
        }
        KeyCode::End => {
            editor.cursor = provider_editor_value_len(editor);
        }
        KeyCode::Char('u') if modifiers == KeyModifiers::CONTROL => {
            clear_provider_editor_value(editor);
            editor.cursor = 0;
        }
        KeyCode::Char(character)
            if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            insert_provider_editor_char(editor, character);
        }
        _ => {}
    }
    true
}

fn provider_editor_string_value(
    editor: &mut crate::app::ProviderEditorState,
) -> Option<&mut String> {
    match editor.field {
        crate::app::ProviderEditorField::Provider => Some(&mut editor.draft.provider),
        crate::app::ProviderEditorField::BaseUrl => Some(&mut editor.draft.base_url),
        crate::app::ProviderEditorField::ApiKey => None,
        crate::app::ProviderEditorField::ApiKeyEnv => Some(&mut editor.draft.api_key_env),
        crate::app::ProviderEditorField::PromptCache => None,
        crate::app::ProviderEditorField::Model => Some(&mut editor.draft.model),
    }
}

fn insert_provider_editor_char(editor: &mut crate::app::ProviderEditorState, character: char) {
    let cursor = editor.cursor;
    if editor.field == crate::app::ProviderEditorField::PromptCache {
        return;
    }
    if editor.field == crate::app::ProviderEditorField::ApiKey {
        editor.draft.api_key_env.clear();
        editor.cursor = editor.api_key.insert_char(cursor, character);
    } else {
        if editor.field == crate::app::ProviderEditorField::ApiKeyEnv {
            editor.api_key.clear();
        }
        editor.cursor = {
            let value = provider_editor_string_value(editor)
                .expect("non-secret provider editor fields have string values");
            let cursor = floor_char_boundary(value, cursor);
            value.insert(cursor, character);
            cursor + character.len_utf8()
        };
    }
}

fn provider_editor_value_len(editor: &mut crate::app::ProviderEditorState) -> usize {
    if editor.field == crate::app::ProviderEditorField::PromptCache {
        return 0;
    }
    if editor.field == crate::app::ProviderEditorField::ApiKey {
        editor.api_key.len()
    } else {
        provider_editor_string_value(editor)
            .expect("non-secret provider editor fields have string values")
            .len()
    }
}

fn provider_editor_floor_char_boundary(
    editor: &mut crate::app::ProviderEditorState,
    cursor: usize,
) -> usize {
    if editor.field == crate::app::ProviderEditorField::PromptCache {
        return 0;
    }
    if editor.field == crate::app::ProviderEditorField::ApiKey {
        editor.api_key.floor_char_boundary(cursor)
    } else {
        let value = provider_editor_string_value(editor)
            .expect("non-secret provider editor fields have string values");
        floor_char_boundary(value, cursor)
    }
}

fn provider_editor_previous_char_boundary(
    editor: &mut crate::app::ProviderEditorState,
    cursor: usize,
) -> usize {
    if editor.field == crate::app::ProviderEditorField::PromptCache {
        return 0;
    }
    if editor.field == crate::app::ProviderEditorField::ApiKey {
        editor.api_key.previous_char_boundary(cursor)
    } else {
        let value = provider_editor_string_value(editor)
            .expect("non-secret provider editor fields have string values");
        previous_char_boundary(value, cursor)
    }
}

fn provider_editor_next_char_boundary(
    editor: &mut crate::app::ProviderEditorState,
    cursor: usize,
) -> usize {
    if editor.field == crate::app::ProviderEditorField::PromptCache {
        return 0;
    }
    if editor.field == crate::app::ProviderEditorField::ApiKey {
        editor.api_key.next_char_boundary(cursor)
    } else {
        let value = provider_editor_string_value(editor)
            .expect("non-secret provider editor fields have string values");
        next_char_boundary(value, cursor)
    }
}

fn remove_provider_editor_range(
    editor: &mut crate::app::ProviderEditorState,
    range: std::ops::Range<usize>,
) {
    if editor.field == crate::app::ProviderEditorField::PromptCache {
        return;
    }
    if editor.field == crate::app::ProviderEditorField::ApiKey {
        editor.api_key.remove(range);
    } else {
        provider_editor_string_value(editor)
            .expect("non-secret provider editor fields have string values")
            .replace_range(range, "");
    }
}

fn clear_provider_editor_value(editor: &mut crate::app::ProviderEditorState) {
    if editor.field == crate::app::ProviderEditorField::PromptCache {
        return;
    }
    if editor.field == crate::app::ProviderEditorField::ApiKey {
        editor.api_key.clear();
    } else {
        provider_editor_string_value(editor)
            .expect("non-secret provider editor fields have string values")
            .clear();
    }
}

fn handle_slash_palette(
    state: &mut crate::app::AppState,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> bool {
    if handle_inline_overlay_horizontal_motion(state, code, modifiers) {
        return true;
    }
    let busy = state.turn_lifecycle.is_active() || state.review_in_flight;
    match code {
        KeyCode::Esc => {
            state.composer_ui.dismissed_completion_token =
                active_composer_token(&state.input, state.input_cursor)
                    .map(|(_, token)| token.to_string());
            state.slash_palette_open = false;
        }
        KeyCode::Enter => {
            if let Some(action) = crate::tui::commands::parse_command_line_with_model_alias(
                &state.input,
                state
                    .model_list
                    .iter()
                    .map(|model| model.id.as_str())
                    .collect::<Vec<_>>(),
            ) {
                let executable = !action.args.trim().is_empty()
                    || action.id.selection_behavior()
                        == crate::tui::commands::CommandSelectionBehavior::Execute;
                if executable {
                    state.take_composer_text();
                    state.slash_palette_open = false;
                    queue_command(state, action, busy);
                    return true;
                }
            }
            let builtins = crate::tui::commands::matching_commands(&state.slash_filter, busy);
            let custom = state.prompt_commands.matching(&state.slash_filter, busy);
            let selected = builtins.get(state.selected_slash_idx).copied();
            if let Some(spec) = selected {
                match spec.id.selection_behavior() {
                    crate::tui::commands::CommandSelectionBehavior::CompleteArguments => {
                        let _ = replace_active_composer_token(state, &format!("/{} ", spec.name));
                        state.slash_palette_open = false;
                        state.status_message = Some(format!("/{} needs an argument", spec.name));
                    }
                    crate::tui::commands::CommandSelectionBehavior::Execute => {
                        state.take_composer_text();
                        state.slash_palette_open = false;
                        queue_command(
                            state,
                            crate::tui::commands::CommandAction {
                                id: spec.id,
                                args: String::new(),
                            },
                            busy,
                        );
                    }
                }
            } else if let Some(command) = custom.get(state.selected_slash_idx - builtins.len()) {
                let command_name = command.name.clone();
                let invocation = state.input.clone();
                match state.prompt_commands.expand_line(&invocation) {
                    Ok(Some(expanded)) => {
                        state.input = expanded;
                        state.input_cursor = state.input.len();
                        state.slash_palette_open = false;
                        state.status_message =
                            Some(format!("Expanded /{command_name}; press Enter to run"));
                    }
                    Ok(None) => {
                        let _ = replace_active_composer_token(state, &format!("/{command_name} "));
                        state.slash_palette_open = false;
                    }
                    Err(error) => {
                        state.slash_palette_open = false;
                        state.status_message = Some(error);
                    }
                }
            } else {
                state.slash_palette_open = false;
                state.status_message = Some(format!("Unknown command: /{}", state.slash_filter));
            }
        }
        KeyCode::Char(c) => {
            insert_char(state, c);
            sync_slash_filter(state);
            state.selected_slash_idx = 0;
            if c.is_whitespace() {
                state.slash_palette_open = false;
                refresh_inline_completion(state);
            }
        }
        KeyCode::Backspace => {
            delete_previous_char(state);
            sync_slash_filter(state);
            if !active_slash_command_token(&state.input, state.input_cursor).is_some() {
                state.slash_palette_open = false;
            }
            state.selected_slash_idx = 0;
        }
        KeyCode::Up => {
            let count = crate::tui::commands::matching_commands(&state.slash_filter, busy).len()
                + state
                    .prompt_commands
                    .matching(&state.slash_filter, busy)
                    .len();
            if count > 0 {
                state.selected_slash_idx = if state.selected_slash_idx == 0 {
                    count - 1
                } else {
                    state.selected_slash_idx - 1
                };
            }
        }
        KeyCode::Down => {
            let count = crate::tui::commands::matching_commands(&state.slash_filter, busy).len()
                + state
                    .prompt_commands
                    .matching(&state.slash_filter, busy)
                    .len();
            if count > 0 {
                state.selected_slash_idx = (state.selected_slash_idx + 1) % count;
            }
        }
        KeyCode::Home => state.selected_slash_idx = 0,
        KeyCode::End => {
            state.selected_slash_idx =
                (crate::tui::commands::matching_commands(&state.slash_filter, busy).len()
                    + state
                        .prompt_commands
                        .matching(&state.slash_filter, busy)
                        .len())
                .saturating_sub(1);
        }
        KeyCode::Tab => {
            let builtins = crate::tui::commands::matching_commands(&state.slash_filter, busy);
            let custom = state.prompt_commands.matching(&state.slash_filter, busy);
            if let Some(spec) = builtins.get(state.selected_slash_idx) {
                let suffix = if spec.id.selection_behavior()
                    == crate::tui::commands::CommandSelectionBehavior::CompleteArguments
                {
                    " "
                } else {
                    ""
                };
                let _ = replace_active_composer_token(state, &format!("/{}{suffix}", spec.name));
                state.slash_palette_open = false;
                state.status_message = Some(format!("Completed /{}", spec.name));
            } else if let Some(command) = custom.get(state.selected_slash_idx - builtins.len()) {
                let command_name = command.name.clone();
                let _ = replace_active_composer_token(state, &format!("/{command_name} "));
                state.slash_palette_open = false;
                state.status_message = Some(format!("Completed /{command_name}"));
            }
        }
        _ => {}
    }
    true
}

fn active_composer_token(input: &str, cursor: usize) -> Option<(std::ops::Range<usize>, &str)> {
    let cursor = crate::tui::composer::normalize_boundary(input, cursor);
    let start = input[..cursor]
        .rfind(char::is_whitespace)
        .map_or(0, |index| index + 1);
    let end = input[cursor..]
        .find(char::is_whitespace)
        .map_or(input.len(), |offset| cursor + offset);
    (start < end).then(|| (start..end, &input[start..end]))
}

fn replace_active_composer_token(
    state: &mut crate::app::AppState,
    replacement: &str,
) -> Option<std::ops::Range<usize>> {
    let (range, _) = active_composer_token(&state.input, state.input_cursor)?;
    state
        .composer_ui
        .apply_edit(range.clone(), replacement.len());
    state.input.replace_range(range.clone(), replacement);
    state.input_cursor = range.start + replacement.len();
    state.composer_ui.dismissed_completion_token = None;
    Some(range.start..state.input_cursor)
}

fn sync_slash_filter(state: &mut crate::app::AppState) {
    state.slash_filter = active_slash_command_token(&state.input, state.input_cursor)
        .and_then(|token| token.strip_prefix('/'))
        .unwrap_or_default()
        .to_string();
}

fn active_slash_command_token(input: &str, cursor: usize) -> Option<&str> {
    active_composer_token(input, cursor)
        .filter(|(range, token)| range.start == 0 && token.starts_with('/'))
        .map(|(_, token)| token)
}

fn refresh_inline_completion(state: &mut crate::app::AppState) {
    let Some((range, token)) = active_composer_token(&state.input, state.input_cursor) else {
        state.slash_palette_open = false;
        return;
    };
    let token = token.to_string();
    if range.start == 0 && token.starts_with('/') {
        let query = token.trim_start_matches('/');
        let busy = state.turn_lifecycle.is_active() || state.review_in_flight;
        if !crate::tui::commands::matching_commands(query, busy).is_empty()
            || !state.prompt_commands.matching(query, busy).is_empty()
        {
            if state.composer_ui.dismissed_completion_token.as_deref() != Some(token.as_str()) {
                sync_slash_filter(state);
                state.selected_slash_idx = 0;
                state.slash_palette_open = true;
                state.composer_ui.path_completion.open = false;
            }
        } else if crate::tui::composer::path_shaped(&token)
            && state.composer_ui.dismissed_completion_token.as_deref() != Some(token.as_str())
        {
            state.slash_palette_open = false;
            open_path_completion(state, token);
        }
    } else if token.starts_with('@')
        && state.composer_ui.dismissed_completion_token.as_deref() != Some(token.as_str())
    {
        open_inline_file_picker(state, false);
    } else if crate::tui::composer::path_shaped(&token)
        && state.composer_ui.dismissed_completion_token.as_deref() != Some(token.as_str())
    {
        open_path_completion(state, token);
    } else {
        state.composer_ui.path_completion.open = false;
    }
}

fn open_path_completion(state: &mut crate::app::AppState, token: String) {
    let completion = &mut state.composer_ui.path_completion;
    if completion.open && completion.token == token {
        return;
    }
    completion.open = true;
    completion.generation = completion.generation.wrapping_add(1);
    completion.token = token;
    completion.loading = true;
    completion.selected = 0;
    completion.entries.clear();
    completion.error = None;
    state.pending_input_action = Some(crate::tui::commands::InputAction::RefreshPathCompletion);
}

fn handle_path_completion(
    state: &mut crate::app::AppState,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> bool {
    match (modifiers, code) {
        (_, KeyCode::Esc) => {
            state.composer_ui.dismissed_completion_token =
                Some(state.composer_ui.path_completion.token.clone());
            state.composer_ui.path_completion.open = false;
            state.composer_ui.path_completion.generation =
                state.composer_ui.path_completion.generation.wrapping_add(1);
        }
        (KeyModifiers::NONE, KeyCode::Up) => {
            let completion = &mut state.composer_ui.path_completion;
            completion.selected =
                crate::tui::selection::previous_wrap(completion.selected, completion.entries.len());
        }
        (KeyModifiers::NONE, KeyCode::Down) => {
            let completion = &mut state.composer_ui.path_completion;
            completion.selected =
                crate::tui::selection::next_wrap(completion.selected, completion.entries.len());
        }
        (KeyModifiers::ALT, KeyCode::Left) => {
            move_to_previous_word(state);
            refresh_inline_completion(state);
        }
        (KeyModifiers::ALT, KeyCode::Right) => {
            move_to_next_word_end(state);
            refresh_inline_completion(state);
        }
        (command, KeyCode::Left) if is_command_modifier(command) => {
            move_to_hard_line_start(state);
            refresh_inline_completion(state);
        }
        (command, KeyCode::Right) if is_command_modifier(command) => {
            move_to_hard_line_end(state);
            refresh_inline_completion(state);
        }
        (KeyModifiers::NONE, KeyCode::Left) => {
            if state.input_cursor > 0 {
                state.input_cursor = state
                    .composer_ui
                    .previous_boundary(&state.input, state.input_cursor);
            }
            state.composer_ui.preferred_column = None;
            refresh_inline_completion(state);
        }
        (KeyModifiers::NONE, KeyCode::Right) => {
            if state.input_cursor < state.input.len() {
                state.input_cursor = state
                    .composer_ui
                    .next_boundary(&state.input, state.input_cursor);
            }
            state.composer_ui.preferred_column = None;
            refresh_inline_completion(state);
        }
        (KeyModifiers::NONE, KeyCode::Home) => state.composer_ui.path_completion.selected = 0,
        (KeyModifiers::NONE, KeyCode::End) => {
            state.composer_ui.path_completion.selected = state
                .composer_ui
                .path_completion
                .entries
                .len()
                .saturating_sub(1)
        }
        (KeyModifiers::NONE, KeyCode::PageUp) => {
            let selected = state.composer_ui.path_completion.selected;
            state.composer_ui.path_completion.selected =
                crate::tui::selection::page_up(selected, 8);
        }
        (KeyModifiers::NONE, KeyCode::PageDown) => {
            let completion = &mut state.composer_ui.path_completion;
            completion.selected =
                crate::tui::selection::page_down(completion.selected, completion.entries.len(), 8);
        }
        (KeyModifiers::NONE, KeyCode::Enter | KeyCode::Tab) => {
            let Some(entry) = state
                .composer_ui
                .path_completion
                .entries
                .get(state.composer_ui.path_completion.selected)
                .cloned()
            else {
                if code == KeyCode::Enter {
                    state.composer_ui.path_completion.open = false;
                    return false;
                }
                return true;
            };
            let replacement = if entry.is_dir {
                entry.replacement.clone()
            } else {
                format!("{} ", entry.replacement)
            };
            let inserted = replace_active_composer_token(state, &replacement);
            if entry.is_dir {
                open_path_completion(state, entry.replacement);
            } else {
                state.composer_ui.path_completion.open = false;
                if let Some(range) = inserted {
                    state
                        .composer_ui
                        .register_atomic(crate::tui::composer::AtomicElement {
                            range: range.start..range.end.saturating_sub(1),
                            kind: crate::tui::composer::AtomicKind::File,
                        });
                }
            }
        }
        (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(character)) => {
            insert_char(state, character);
            refresh_inline_completion(state);
        }
        (KeyModifiers::ALT, KeyCode::Backspace) => {
            delete_previous_word(state);
            refresh_inline_completion(state);
        }
        (command, KeyCode::Backspace) if is_command_modifier(command) => {
            delete_to_hard_line_start(state);
            refresh_inline_completion(state);
        }
        (KeyModifiers::NONE, KeyCode::Backspace) => {
            delete_previous_char(state);
            refresh_inline_completion(state);
        }
        _ => {}
    }
    true
}

fn open_inline_file_picker(state: &mut crate::app::AppState, rebuild_index: bool) {
    state.file_picker_open = true;
    sync_file_picker_filter(state);
    state.selected_file_idx = 0;
    state.file_picker_generation = state.file_picker_generation.wrapping_add(1);
    state.file_picker_discovered = 0;
    state.file_picker_truncated = false;
    state.file_picker_error = None;
    if rebuild_index || !state.file_picker_index_ready {
        state.file_picker_phase = crate::attachments::PickerPhase::Indexing;
        state.file_picker_index = std::sync::Arc::new(Vec::new());
        state.file_picker_index_ready = false;
        state.file_picker_resources.clear();
        state.file_picker_entries.clear();
        state.file_picker_matches = 0;
    } else {
        state.file_picker_phase = crate::attachments::PickerPhase::Matching;
    }
    state.pending_input_action = Some(crate::tui::commands::InputAction::RefreshFilePicker);
}

fn handle_queue_view(
    state: &mut crate::app::AppState,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> bool {
    match (modifiers, code) {
        (_, KeyCode::Esc) => state.queue_view_open = false,
        (KeyModifiers::SHIFT, KeyCode::Up) => {
            if state.input_queue.move_selected_up() {
                state.pending_input_action =
                    Some(crate::tui::commands::InputAction::PersistSessionQueue);
            }
        }
        (KeyModifiers::SHIFT, KeyCode::Down) => {
            if state.input_queue.move_selected_down() {
                state.pending_input_action =
                    Some(crate::tui::commands::InputAction::PersistSessionQueue);
            }
        }
        (_, KeyCode::Up) | (_, KeyCode::Char('k')) => state.input_queue.select_previous(),
        (_, KeyCode::Down) | (_, KeyCode::Char('j')) => state.input_queue.select_next(),
        (_, KeyCode::Delete) | (_, KeyCode::Char('d')) => {
            if state.input_queue.delete_selected().is_some() {
                state.pending_input_action =
                    Some(crate::tui::commands::InputAction::PersistSessionQueue);
            }
            if state.input_queue.is_empty() {
                state.queue_view_open = false;
            }
        }
        (_, KeyCode::Enter) | (_, KeyCode::Char('e')) => {
            if let Some(text) = state.input_queue.begin_edit_selected() {
                state.input = text;
                state.input_cursor = state.input.len();
                state.composer_ui.clear_for_new_draft();
                state.composer_attachments.clear();
                state.pasted_images.clear();
                state.queue_view_open = false;
                state.status_message = Some(format!(
                    "Editing queued message · Enter sends now · {} saves to queue",
                    state
                        .keymap
                        .display_binding(crate::keymap::KeyAction::QueueInput)
                ));
            }
        }
        _ => {}
    }
    true
}

fn handle_change_review(
    state: &mut crate::app::AppState,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> bool {
    use crate::changes::{ChangeViewMode, GitReviewOperation};

    if state.change_review.operation.is_some() {
        if modifiers.contains(KeyModifiers::CONTROL) && code == KeyCode::Char('c') {
            state.pending_input_action =
                Some(crate::tui::commands::InputAction::CancelChangeOperation);
        }
        return true;
    }

    let selected_file = |state: &crate::app::AppState| {
        state
            .change_review
            .files
            .get(state.change_review.selected_index)
            .cloned()
    };

    match state.change_review.mode {
        ChangeViewMode::List => match code {
            KeyCode::Esc | KeyCode::Char('q') => state.change_review.open = false,
            KeyCode::Up | KeyCode::Char('k') => {
                state.change_review.selected_index =
                    state.change_review.selected_index.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if state.change_review.selected_index + 1 < state.change_review.files.len() {
                    state.change_review.selected_index += 1;
                }
            }
            KeyCode::Enter => {
                if let Some(file) = selected_file(state) {
                    if file.is_tracked() {
                        state.pending_input_action =
                            Some(crate::tui::commands::InputAction::OpenChangeDiff(file));
                    } else {
                        state.status_message =
                            Some("Untracked file contents are not opened automatically".into());
                    }
                }
            }
            KeyCode::Char('r') if state.change_review.is_commit() => {
                state.pending_input_action =
                    Some(crate::tui::commands::InputAction::RefreshChangeReview);
            }
            KeyCode::Char('s') if state.change_review.is_commit() => {
                if let Some(file) = selected_file(state) {
                    state.pending_input_action =
                        Some(crate::tui::commands::InputAction::StageChange(file));
                }
            }
            KeyCode::Char('u') if state.change_review.is_commit() => {
                if let Some(file) = selected_file(state) {
                    state.pending_input_action =
                        Some(crate::tui::commands::InputAction::UnstageChange(file));
                }
            }
            KeyCode::Char('c') if state.change_review.is_commit() => {
                if state.change_review.files.iter().any(|file| file.staged) {
                    state.change_review.mode = ChangeViewMode::Subject;
                    state.change_review.subject_cursor = state.change_review.subject.len();
                    state.change_review.error = None;
                    state.change_review.notice = None;
                } else {
                    state.change_review.error = Some("No staged changes to commit".into());
                }
            }
            _ => {}
        },
        ChangeViewMode::Diff => match code {
            KeyCode::Esc | KeyCode::Char('q') => {
                state.change_review.mode = ChangeViewMode::List;
                state.change_review.scroll = 0;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                state.change_review.scroll = state.change_review.scroll.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                state.change_review.scroll = state.change_review.scroll.saturating_add(1);
            }
            KeyCode::PageUp | KeyCode::Char('b') => {
                state.change_review.scroll = state.change_review.scroll.saturating_sub(15);
            }
            KeyCode::PageDown | KeyCode::Char(' ') => {
                state.change_review.scroll = state.change_review.scroll.saturating_add(15);
            }
            KeyCode::Home | KeyCode::Char('g') => state.change_review.scroll = 0,
            KeyCode::Char('r') if state.change_review.is_commit() => {
                state.change_review.mode = ChangeViewMode::List;
                state.pending_input_action =
                    Some(crate::tui::commands::InputAction::RefreshChangeReview);
            }
            KeyCode::Char('s') if state.change_review.is_commit() => {
                if let Some(file) = selected_file(state) {
                    state.change_review.mode = ChangeViewMode::List;
                    state.pending_input_action =
                        Some(crate::tui::commands::InputAction::StageChange(file));
                }
            }
            KeyCode::Char('u') if state.change_review.is_commit() => {
                if let Some(file) = selected_file(state) {
                    state.change_review.mode = ChangeViewMode::List;
                    state.pending_input_action =
                        Some(crate::tui::commands::InputAction::UnstageChange(file));
                }
            }
            KeyCode::Char('c') if state.change_review.is_commit() => {
                if state.change_review.files.iter().any(|file| file.staged) {
                    state.change_review.mode = ChangeViewMode::Subject;
                    state.change_review.subject_cursor = state.change_review.subject.len();
                    state.change_review.error = None;
                } else {
                    state.change_review.error = Some("No staged changes to commit".into());
                }
            }
            _ => {}
        },
        ChangeViewMode::Subject => match code {
            KeyCode::Esc => {
                state.change_review.mode = ChangeViewMode::List;
                state.change_review.error = None;
            }
            KeyCode::Enter => {
                match crate::changes::validate_commit_subject(&state.change_review.subject) {
                    Ok(subject) => {
                        state.pending_input_action =
                            Some(crate::tui::commands::InputAction::PrepareCommit(subject));
                        state.change_review.operation = Some(GitReviewOperation::Preparing);
                        state.change_review.loading = true;
                        state.change_review.error = None;
                    }
                    Err(error) => state.change_review.error = Some(error),
                }
            }
            KeyCode::Left => {
                state.change_review.subject_cursor = previous_char_boundary(
                    &state.change_review.subject,
                    state.change_review.subject_cursor,
                );
            }
            KeyCode::Right => {
                state.change_review.subject_cursor = next_char_boundary(
                    &state.change_review.subject,
                    state.change_review.subject_cursor,
                );
            }
            KeyCode::Home => state.change_review.subject_cursor = 0,
            KeyCode::End => {
                state.change_review.subject_cursor = state.change_review.subject.len();
            }
            KeyCode::Backspace if state.change_review.subject_cursor > 0 => {
                let previous = previous_char_boundary(
                    &state.change_review.subject,
                    state.change_review.subject_cursor,
                );
                state
                    .change_review
                    .subject
                    .replace_range(previous..state.change_review.subject_cursor, "");
                state.change_review.subject_cursor = previous;
                state.change_review.error = None;
            }
            KeyCode::Delete
                if state.change_review.subject_cursor < state.change_review.subject.len() =>
            {
                let next = next_char_boundary(
                    &state.change_review.subject,
                    state.change_review.subject_cursor,
                );
                state
                    .change_review
                    .subject
                    .replace_range(state.change_review.subject_cursor..next, "");
                state.change_review.error = None;
            }
            KeyCode::Char(character)
                if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                    && !character.is_control() =>
            {
                if state.change_review.subject.chars().count()
                    < crate::changes::MAX_COMMIT_SUBJECT_CHARS
                {
                    state
                        .change_review
                        .subject
                        .insert(state.change_review.subject_cursor, character);
                    state.change_review.subject_cursor += character.len_utf8();
                    state.change_review.error = None;
                } else {
                    state.change_review.error = Some(format!(
                        "Commit subject must be at most {} characters",
                        crate::changes::MAX_COMMIT_SUBJECT_CHARS
                    ));
                }
            }
            _ => {}
        },
        ChangeViewMode::Confirm => match code {
            KeyCode::Esc => {
                state.change_review.mode = ChangeViewMode::Subject;
                state.change_review.commit_snapshot = None;
                state.change_review.commit_selected = false;
            }
            KeyCode::Left | KeyCode::Right | KeyCode::Tab => {
                state.change_review.commit_selected = !state.change_review.commit_selected;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                state.change_review.confirm_scroll =
                    state.change_review.confirm_scroll.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                state.change_review.confirm_scroll =
                    state.change_review.confirm_scroll.saturating_add(1);
            }
            KeyCode::PageUp | KeyCode::Char('b') => {
                state.change_review.confirm_scroll =
                    state.change_review.confirm_scroll.saturating_sub(15);
            }
            KeyCode::PageDown | KeyCode::Char(' ') => {
                state.change_review.confirm_scroll =
                    state.change_review.confirm_scroll.saturating_add(15);
            }
            KeyCode::Home | KeyCode::Char('g') => state.change_review.confirm_scroll = 0,
            KeyCode::Enter if state.change_review.commit_selected => {
                if let Some(snapshot) = state.change_review.commit_snapshot.clone() {
                    state.pending_input_action =
                        Some(crate::tui::commands::InputAction::CommitChanges {
                            snapshot,
                            subject: state.change_review.subject.clone(),
                        });
                    state.change_review.operation = Some(GitReviewOperation::Committing);
                    state.change_review.loading = true;
                    state.change_review.error = None;
                } else {
                    state.change_review.mode = ChangeViewMode::Subject;
                    state.change_review.error = Some("Commit snapshot is unavailable".into());
                }
            }
            KeyCode::Enter => {
                state.change_review.mode = ChangeViewMode::Subject;
                state.change_review.commit_snapshot = None;
                state.change_review.commit_selected = false;
            }
            _ => {}
        },
    }
    true
}

fn handle_change_walkthrough(
    state: &mut crate::app::AppState,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> bool {
    let Some(status) = state
        .change_walkthrough
        .as_ref()
        .map(|walkthrough| walkthrough.status.clone())
    else {
        return false;
    };
    let busy = matches!(
        status,
        crate::change_walkthrough::ChangeWalkthroughStatus::Capturing
            | crate::change_walkthrough::ChangeWalkthroughStatus::Generating
    );
    match (modifiers, code) {
        (_, KeyCode::Esc) => {
            state.change_walkthrough = None;
            state.change_walkthrough_requested = false;
            state.change_walkthrough_generation =
                state.change_walkthrough_generation.wrapping_add(1);
            state.pending_input_action =
                Some(crate::tui::commands::InputAction::CancelChangeWalkthrough);
        }
        (KeyModifiers::SHIFT, KeyCode::Char('R')) if !busy => {
            state.change_walkthrough_generation =
                state.change_walkthrough_generation.wrapping_add(1);
            let generation = state.change_walkthrough_generation;
            let walkthrough = state
                .change_walkthrough
                .as_mut()
                .expect("walkthrough exists");
            walkthrough.status = crate::change_walkthrough::ChangeWalkthroughStatus::Capturing;
            walkthrough.source_hash = None;
            walkthrough.included_files.clear();
            walkthrough.refusal = None;
            walkthrough.result = None;
            walkthrough.error = None;
            walkthrough.freshness_error = None;
            walkthrough.scroll = 0;
            walkthrough.generation = generation;
            state.change_walkthrough_requested = true;
            state.pending_input_action =
                Some(crate::tui::commands::InputAction::CancelChangeWalkthrough);
        }
        (_, KeyCode::Up) | (_, KeyCode::Char('k')) if !busy => {
            let walkthrough = state
                .change_walkthrough
                .as_mut()
                .expect("walkthrough exists");
            walkthrough.scroll = walkthrough.scroll.saturating_sub(1);
        }
        (_, KeyCode::Down) | (_, KeyCode::Char('j')) if !busy => {
            let walkthrough = state
                .change_walkthrough
                .as_mut()
                .expect("walkthrough exists");
            walkthrough.scroll = walkthrough.scroll.saturating_add(1);
        }
        (_, KeyCode::PageUp) if !busy => {
            let walkthrough = state
                .change_walkthrough
                .as_mut()
                .expect("walkthrough exists");
            walkthrough.scroll = walkthrough.scroll.saturating_sub(15);
        }
        (_, KeyCode::PageDown) | (_, KeyCode::Char(' ')) if !busy => {
            let walkthrough = state
                .change_walkthrough
                .as_mut()
                .expect("walkthrough exists");
            walkthrough.scroll = walkthrough.scroll.saturating_add(15);
        }
        (_, KeyCode::Home) | (_, KeyCode::Char('g')) if !busy => {
            state
                .change_walkthrough
                .as_mut()
                .expect("walkthrough exists")
                .scroll = 0;
        }
        _ => {}
    }
    true
}

fn handle_jobs_view(state: &mut crate::app::AppState, code: KeyCode) -> bool {
    let activity_count =
        state.job_list.len() + state.watch_list.len() + state.delegation_list.len();
    let delegation_offset = state.job_list.len() + state.watch_list.len();
    let selected_delegation = state
        .selected_job_idx
        .checked_sub(delegation_offset)
        .and_then(|index| state.delegation_list.get(index));
    match code {
        KeyCode::Esc | KeyCode::Char('q') => {
            if state.delegation_correction_confirmation.take().is_some() {
                state.status_message = Some("Delegation correction cancelled".into());
            } else if state.job_detail_open {
                state.job_detail_open = false;
            } else {
                state.jobs_view_open = false;
            }
        }
        KeyCode::Up | KeyCode::Char('k') => {
            if state.job_detail_open {
                state.job_scroll = state.job_scroll.saturating_sub(1);
            } else {
                state.selected_job_idx = state.selected_job_idx.saturating_sub(1);
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if state.job_detail_open {
                state.job_scroll = state.job_scroll.saturating_add(1);
            } else if state.selected_job_idx + 1 < activity_count {
                state.selected_job_idx += 1;
            }
        }
        KeyCode::PageUp if state.job_detail_open => {
            state.job_scroll = state.job_scroll.saturating_sub(15);
        }
        KeyCode::PageDown if state.job_detail_open => {
            state.job_scroll = state.job_scroll.saturating_add(15);
        }
        KeyCode::Enter => {
            if activity_count > 0 {
                state.job_detail_open = !state.job_detail_open;
                state.job_scroll = 0;
            }
        }
        KeyCode::Char('x') => {
            if let Some(job) = state.job_list.get(state.selected_job_idx) {
                if !job.status.is_terminal() {
                    state.pending_input_action =
                        Some(crate::tui::commands::InputAction::KillJob(job.id));
                }
            } else if let Some(watcher) = state
                .watch_list
                .get(state.selected_job_idx.saturating_sub(state.job_list.len()))
            {
                if !watcher.event.state.is_terminal() {
                    state.pending_input_action = Some(
                        crate::tui::commands::InputAction::StopWatcher(watcher.watch_id),
                    );
                }
            } else if let Some(delegation) = selected_delegation {
                if !delegation.status.is_terminal() {
                    state.pending_input_action =
                        Some(crate::tui::commands::InputAction::CancelDelegation(
                            delegation.job_id.clone(),
                        ));
                }
            }
        }
        KeyCode::Char('i') => {
            if let Some(job) = state.job_list.get(state.selected_job_idx) {
                if job.status == crate::jobs::JobStatus::Running {
                    state.job_stdin_target = Some(job.id);
                    state.jobs_view_open = false;
                    state.input.clear();
                    state.input_cursor = 0;
                    state.composer_ui.clear_for_new_draft();
                    state.composer_attachments.clear();
                    state.pasted_images.clear();
                    state.status_message =
                        Some(format!("Type stdin for job {} and press Enter", job.id));
                }
            } else if selected_delegation.is_some() {
                state.status_message = Some("Codex-team delegations do not accept stdin".into());
            } else if state.selected_job_idx >= state.job_list.len() && !state.watch_list.is_empty()
            {
                state.status_message =
                    Some("PID watchers do not accept stdin; the target process is external".into());
            }
        }
        KeyCode::Char('r') => {
            state.jobs_refresh_requested = true;
            if let Some(delegation) = selected_delegation {
                state.mechanistic_request = Some(crate::app::MechanisticUiRequest::Recover(
                    delegation.job_id.clone(),
                ));
            }
        }
        KeyCode::Char('a') if selected_delegation.is_some_and(|job| job.status.is_terminal()) => {
            let job = selected_delegation.expect("checked");
            state.pending_input_action =
                Some(crate::tui::commands::InputAction::FeedbackDelegation {
                    job_id: job.job_id.clone(),
                    category: crate::mechanistic_delegation::DelegationFeedbackCategory::Accepted,
                    correction_confirmed: false,
                });
        }
        KeyCode::Char(key @ ('1' | '2' | '3' | '4' | '5'))
            if selected_delegation.is_some_and(|job| job.status.is_terminal()) =>
        {
            let category = match key {
                '1' => crate::mechanistic_delegation::DelegationFeedbackCategory::IncorrectResult,
                '2' => crate::mechanistic_delegation::DelegationFeedbackCategory::Incomplete,
                '3' => crate::mechanistic_delegation::DelegationFeedbackCategory::WrongMatch,
                '4' => crate::mechanistic_delegation::DelegationFeedbackCategory::Unsafe,
                _ => crate::mechanistic_delegation::DelegationFeedbackCategory::Stale,
            };
            let job = selected_delegation.expect("checked");
            state.pending_input_action =
                Some(crate::tui::commands::InputAction::FeedbackDelegation {
                    job_id: job.job_id.clone(),
                    category,
                    correction_confirmed: false,
                });
        }
        KeyCode::Char('C') if selected_delegation.is_some_and(|job| job.status.is_terminal()) => {
            let job = selected_delegation.expect("checked");
            state.delegation_correction_confirmation = Some(job.job_id.clone());
            state.status_message =
                Some("Press y to confirm human correction feedback, or Esc to cancel".into());
        }
        KeyCode::Char('y') => {
            if let Some(job_id) = state.delegation_correction_confirmation.take() {
                state.pending_input_action = Some(
                    crate::tui::commands::InputAction::FeedbackDelegation {
                        job_id,
                        category: crate::mechanistic_delegation::DelegationFeedbackCategory::IncorrectResult,
                        correction_confirmed: true,
                    },
                );
            }
        }
        _ => {}
    }
    true
}

fn handle_terminal_manager(
    state: &mut crate::app::AppState,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> bool {
    let Some(manager) = state.terminal_manager.as_mut() else {
        return false;
    };
    if manager.busy {
        return true;
    }
    if let Some(confirmation) = manager.kill_confirmation.as_mut() {
        match code {
            KeyCode::Esc => manager.kill_confirmation = None,
            KeyCode::Left | KeyCode::Right | KeyCode::Tab => {
                confirmation.kill_selected = !confirmation.kill_selected;
            }
            KeyCode::Enter if confirmation.kill_selected => {
                let name = confirmation.name.clone();
                manager.busy = true;
                manager.error = None;
                state.pending_input_action =
                    Some(crate::tui::commands::InputAction::KillTerminal(name));
            }
            KeyCode::Enter => manager.kill_confirmation = None,
            _ => {}
        }
        return true;
    }
    if let crate::app::TerminalManagerMode::Create { name, cursor } = &mut manager.mode {
        match code {
            KeyCode::Esc => manager.mode = crate::app::TerminalManagerMode::List,
            KeyCode::Enter => {
                if crate::terminal_sessions::valid_session_name(name) {
                    let name = name.clone();
                    manager.busy = true;
                    manager.error = None;
                    state.pending_input_action =
                        Some(crate::tui::commands::InputAction::CreateTerminal(name));
                } else {
                    manager.error = Some("Name must start with a letter or digit and use at most 64 letters, digits, '.', '_' or '-'".into());
                }
            }
            KeyCode::Char(character)
                if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                name.insert(*cursor, character);
                *cursor += character.len_utf8();
            }
            KeyCode::Backspace if *cursor > 0 => {
                let previous = previous_char_boundary(name, *cursor);
                name.replace_range(previous..*cursor, "");
                *cursor = previous;
            }
            KeyCode::Delete if *cursor < name.len() => {
                let next = next_char_boundary(name, *cursor);
                name.replace_range(*cursor..next, "");
            }
            KeyCode::Left => *cursor = previous_char_boundary(name, *cursor),
            KeyCode::Right => *cursor = next_char_boundary(name, *cursor),
            KeyCode::Home => *cursor = 0,
            KeyCode::End => *cursor = name.len(),
            _ => {}
        }
        return true;
    }

    let filtered_len = manager.filtered_indices().len();
    match (modifiers, code) {
        (_, KeyCode::Esc) => state.terminal_manager = None,
        (mods, KeyCode::Char('a' | 'A')) if mods.contains(KeyModifiers::SHIFT) => {
            manager.mode = crate::app::TerminalManagerMode::Create {
                name: String::new(),
                cursor: 0,
            };
            manager.error = None;
        }
        (mods, KeyCode::Char('d' | 'D')) if mods.contains(KeyModifiers::SHIFT) => {
            if let Some(session) = manager.selected_session() {
                let name = session.name.clone();
                manager.busy = true;
                manager.error = None;
                state.pending_input_action =
                    Some(crate::tui::commands::InputAction::DetachTerminal(name));
            }
        }
        (mods, KeyCode::Char('k' | 'K')) if mods.contains(KeyModifiers::SHIFT) => {
            if let Some(session) = manager.selected_session() {
                manager.kill_confirmation = Some(crate::app::TerminalKillConfirmation {
                    name: session.name.clone(),
                    kill_selected: false,
                });
            }
        }
        (_, KeyCode::Enter) => {
            if let Some(session) = manager.selected_session() {
                let name = session.name.clone();
                manager.busy = true;
                manager.error = None;
                state.pending_input_action =
                    Some(crate::tui::commands::InputAction::AttachTerminal(name));
            }
        }
        (_, KeyCode::Up) | (_, KeyCode::Char('k')) => {
            manager.selected_idx = manager.selected_idx.saturating_sub(1);
        }
        (_, KeyCode::Down) | (_, KeyCode::Char('j')) => {
            manager.selected_idx = (manager.selected_idx + 1).min(filtered_len.saturating_sub(1));
        }
        (_, KeyCode::Home) => manager.selected_idx = 0,
        (_, KeyCode::End) => manager.selected_idx = filtered_len.saturating_sub(1),
        (_, KeyCode::PageUp) => {
            manager.selected_idx = manager.selected_idx.saturating_sub(10);
        }
        (_, KeyCode::PageDown) => {
            manager.selected_idx = (manager.selected_idx + 10).min(filtered_len.saturating_sub(1));
        }
        (_, KeyCode::Backspace) => {
            manager.filter.pop();
            manager.selected_idx = 0;
        }
        (_, KeyCode::Char('r')) => {
            manager.busy = true;
            manager.error = None;
            state.terminal_refresh_requested = true;
        }
        (mods, KeyCode::Char(character))
            if !mods.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            manager.filter.push(character);
            manager.selected_idx = 0;
        }
        _ => {}
    }
    true
}

fn handle_theme_manager(
    state: &mut crate::app::AppState,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> bool {
    let Some(manager) = state.theme_manager.as_mut() else {
        return false;
    };
    if manager.busy {
        return true;
    }
    let filtered_len = manager.filtered_themes().len();
    match (modifiers, code) {
        (_, KeyCode::Esc) => state.theme_manager = None,
        (_, KeyCode::Enter) => {
            if let Some(theme) = manager.selected_theme() {
                if state.run_capabilities.config_writes {
                    manager.busy = true;
                    manager.error = None;
                    state.pending_input_action =
                        Some(crate::tui::commands::InputAction::SaveTheme(theme));
                } else {
                    manager.error = Some("Theme preview is available in bare mode, but persistent changes require rerunning without --profile bare".into());
                }
            }
        }
        (_, KeyCode::Up) | (_, KeyCode::Char('k')) => {
            manager.selected_idx = manager.selected_idx.saturating_sub(1);
        }
        (_, KeyCode::Down) | (_, KeyCode::Char('j')) => {
            manager.selected_idx = (manager.selected_idx + 1).min(filtered_len.saturating_sub(1));
        }
        (_, KeyCode::Home) => manager.selected_idx = 0,
        (_, KeyCode::End) => manager.selected_idx = filtered_len.saturating_sub(1),
        (_, KeyCode::Backspace) => {
            manager.filter.pop();
            manager.selected_idx = 0;
        }
        (mods, KeyCode::Char(character))
            if !mods.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            manager.filter.push(character);
            manager.selected_idx = 0;
        }
        _ => {}
    }
    true
}

fn handle_statusline_manager(
    state: &mut crate::app::AppState,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> bool {
    let Some(manager) = state.statusline_manager.as_mut() else {
        return false;
    };
    if manager.busy {
        return true;
    }
    let selected = crate::ui_settings::StatusField::ALL[manager
        .selected_idx
        .min(crate::ui_settings::StatusField::ALL.len() - 1)];
    match (modifiers, code) {
        (_, KeyCode::Esc) => {
            if manager.testing {
                state.pending_input_action =
                    Some(crate::tui::commands::InputAction::CancelStatusCommand);
            }
            state.statusline_manager = None;
        }
        (_, KeyCode::Enter) => {
            if state.run_capabilities.config_writes {
                manager.busy = true;
                manager.error = None;
                state.pending_input_action =
                    Some(crate::tui::commands::InputAction::SaveStatusline(
                        crate::ui_settings::StatuslineUpdate {
                            fields: manager.fields.clone(),
                            custom_enabled: manager.custom_enabled,
                        },
                    ));
            } else {
                manager.error = Some("Statusline preview is available in bare mode, but persistent changes require rerunning without --profile bare".into());
            }
        }
        (mods, KeyCode::Up) if mods.contains(KeyModifiers::SHIFT) => {
            if let Some(index) = manager.fields.iter().position(|field| *field == selected) {
                if index > 0 {
                    manager.fields.swap(index, index - 1);
                }
            }
        }
        (mods, KeyCode::Down) if mods.contains(KeyModifiers::SHIFT) => {
            if let Some(index) = manager.fields.iter().position(|field| *field == selected) {
                if index + 1 < manager.fields.len() {
                    manager.fields.swap(index, index + 1);
                }
            }
        }
        (_, KeyCode::Up) | (_, KeyCode::Char('k')) => {
            manager.selected_idx = manager.selected_idx.saturating_sub(1);
        }
        (_, KeyCode::Down) | (_, KeyCode::Char('j')) => {
            manager.selected_idx =
                (manager.selected_idx + 1).min(crate::ui_settings::StatusField::ALL.len() - 1);
        }
        (_, KeyCode::Home) => manager.selected_idx = 0,
        (_, KeyCode::End) => {
            manager.selected_idx = crate::ui_settings::StatusField::ALL.len() - 1;
        }
        (_, KeyCode::Char(' ')) => {
            if let Some(index) = manager.fields.iter().position(|field| *field == selected) {
                manager.fields.remove(index);
            } else {
                manager.fields.push(selected);
            }
            manager.error = None;
        }
        (_, KeyCode::Char('e')) => {
            if manager.custom_configured {
                manager.custom_enabled = !manager.custom_enabled;
                manager.error = None;
            } else {
                manager.error = Some("Configure ui.statusline.custom_command in the active profile before enabling it".into());
            }
        }
        (mods, KeyCode::Char('t' | 'T')) if mods.contains(KeyModifiers::SHIFT) => {
            if manager.custom_configured {
                manager.testing = true;
                manager.error = None;
                state.pending_input_action =
                    Some(crate::tui::commands::InputAction::TestStatusCommand);
            } else {
                manager.error = Some("No custom status command is configured".into());
            }
        }
        _ => {}
    }
    true
}

fn handle_notification_picker(state: &mut crate::app::AppState, code: KeyCode) -> bool {
    let Some(picker) = state.notification_picker.as_mut() else {
        return false;
    };
    if picker.busy {
        return true;
    }
    match code {
        KeyCode::Esc => state.notification_picker = None,
        KeyCode::Up | KeyCode::Char('k') => {
            picker.selected_idx = picker.selected_idx.saturating_sub(1);
            picker.error = None;
        }
        KeyCode::Down | KeyCode::Char('j') => {
            picker.selected_idx =
                (picker.selected_idx + 1).min(crate::ui_settings::NotificationStyle::ALL.len() - 1);
            picker.error = None;
        }
        KeyCode::Home => picker.selected_idx = 0,
        KeyCode::End => {
            picker.selected_idx = crate::ui_settings::NotificationStyle::ALL.len() - 1;
        }
        KeyCode::Enter => {
            if state.run_capabilities.config_writes {
                let selected = picker.selected();
                picker.busy = true;
                picker.error = None;
                state.pending_input_action = Some(
                    crate::tui::commands::InputAction::SaveNotification(selected),
                );
            } else {
                picker.error = Some(
                    "Notification settings cannot be saved in bare mode; rerun without --profile bare"
                        .into(),
                );
            }
        }
        _ => {}
    }
    true
}

fn handle_copy_picker(
    state: &mut crate::app::AppState,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> bool {
    use crate::app::CopyPickerLevel;

    let Some(picker) = state.copy_picker.as_mut() else {
        return false;
    };
    match code {
        KeyCode::Esc => match picker.level {
            CopyPickerLevel::Messages => state.copy_picker = None,
            CopyPickerLevel::Parts { .. } => {
                picker.level = CopyPickerLevel::Messages;
                picker.filter.clear();
                picker.selected_idx = 0;
            }
        },
        KeyCode::Up => picker.selected_idx = picker.selected_idx.saturating_sub(1),
        KeyCode::Down => {
            picker.selected_idx =
                (picker.selected_idx + 1).min(picker.visible_len().saturating_sub(1));
        }
        KeyCode::Home => picker.selected_idx = 0,
        KeyCode::End => picker.selected_idx = picker.visible_len().saturating_sub(1),
        KeyCode::Backspace => {
            if picker.filter.pop().is_none()
                && matches!(picker.level, CopyPickerLevel::Parts { .. })
            {
                picker.level = CopyPickerLevel::Messages;
            }
            picker.selected_idx = 0;
        }
        KeyCode::Char(character)
            if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            picker.filter.push(character);
            picker.selected_idx = 0;
        }
        KeyCode::Enter => match picker.level {
            CopyPickerLevel::Messages => {
                if let Some(message_index) = picker
                    .filtered_message_indices()
                    .get(picker.selected_idx)
                    .copied()
                {
                    picker.level = CopyPickerLevel::Parts { message_index };
                    picker.filter.clear();
                    picker.selected_idx = 0;
                }
            }
            CopyPickerLevel::Parts { message_index } => {
                let selected = picker
                    .filtered_part_indices(message_index)
                    .get(picker.selected_idx)
                    .and_then(|part_index| picker.messages[message_index].parts.get(*part_index))
                    .cloned();
                if let Some(part) = selected {
                    state.copy_picker = None;
                    if copy_text_to_clipboard(&part.value).is_ok() {
                        state.status_message = None;
                        state.show_toast(
                            format!("Copied {}", part.label.to_ascii_lowercase()),
                            crate::app::ToastKind::Success,
                        );
                    } else {
                        state.status_message =
                            Some("Clipboard unavailable; nothing was copied".into());
                        state.show_toast("Clipboard unavailable", crate::app::ToastKind::Error);
                    }
                }
            }
        },
        _ => {}
    }
    true
}

fn handle_command_output_picker(state: &mut crate::app::AppState, code: KeyCode) -> bool {
    let Some(picker) = state.command_output_picker.as_mut() else {
        return false;
    };
    if picker.busy {
        return true;
    }
    match code {
        KeyCode::Esc => state.command_output_picker = None,
        KeyCode::Up | KeyCode::Char('k') => {
            picker.selected_idx = picker.selected_idx.saturating_sub(1);
            picker.error = None;
        }
        KeyCode::Down | KeyCode::Char('j') => {
            picker.selected_idx =
                (picker.selected_idx + 1).min(crate::config::CommandOutputPolicy::ALL.len() - 1);
            picker.error = None;
        }
        KeyCode::Home => picker.selected_idx = 0,
        KeyCode::End => picker.selected_idx = crate::config::CommandOutputPolicy::ALL.len() - 1,
        KeyCode::Enter => {
            if state.run_capabilities.config_writes {
                let selected = picker.selected();
                picker.busy = true;
                picker.error = None;
                state.pending_input_action = Some(
                    crate::tui::commands::InputAction::SaveCommandOutput(selected),
                );
            } else {
                picker.error = Some(
                    "Command output settings cannot be saved in bare mode; rerun without --profile bare"
                        .into(),
                );
            }
        }
        _ => {}
    }
    true
}

fn handle_output_full_confirmation(state: &mut crate::app::AppState, code: KeyCode) -> bool {
    let Some(confirmation) = state.output_full_confirmation.as_mut() else {
        return false;
    };
    if confirmation.busy {
        return true;
    }
    match code {
        KeyCode::Esc => state.output_full_confirmation = None,
        KeyCode::Left | KeyCode::Char('h') => confirmation.run_selected = false,
        KeyCode::Right | KeyCode::Char('l') => confirmation.run_selected = true,
        KeyCode::Enter if confirmation.run_selected => {
            confirmation.busy = true;
            confirmation.error = None;
            state.pending_input_action = Some(crate::tui::commands::InputAction::RerunFullCommand(
                confirmation.rerun.clone(),
            ));
        }
        KeyCode::Enter => state.output_full_confirmation = None,
        _ => {}
    }
    true
}

fn handle_scratch_delete_confirmation(state: &mut crate::app::AppState, code: KeyCode) -> bool {
    let Some(confirmation) = state.scratch_delete_confirmation.as_mut() else {
        return false;
    };
    match code {
        KeyCode::Esc => state.scratch_delete_confirmation = None,
        KeyCode::Left | KeyCode::Char('h') => confirmation.delete_selected = false,
        KeyCode::Right | KeyCode::Char('l') => confirmation.delete_selected = true,
        KeyCode::Enter if confirmation.delete_selected => {
            state.pending_input_action = Some(crate::tui::commands::InputAction::DeleteScratch(
                confirmation.id,
            ));
        }
        KeyCode::Enter => state.scratch_delete_confirmation = None,
        _ => {}
    }
    true
}

fn handle_route_preset_picker(state: &mut crate::app::AppState, code: KeyCode) -> bool {
    let Some(picker) = state.route_preset_picker.as_mut() else {
        return false;
    };
    match code {
        KeyCode::Esc => state.route_preset_picker = None,
        KeyCode::Up | KeyCode::Char('k') => {
            picker.selected_idx = picker.selected_idx.saturating_sub(1);
            picker.error = None;
        }
        KeyCode::Down | KeyCode::Char('j') => {
            picker.selected_idx =
                (picker.selected_idx + 1).min(picker.rows.len().saturating_sub(1));
            picker.error = None;
        }
        KeyCode::Home => picker.selected_idx = 0,
        KeyCode::End => picker.selected_idx = picker.rows.len().saturating_sub(1),
        KeyCode::Enter => {
            if let Some(name) = picker.selected().map(|row| row.name.clone()) {
                state.pending_input_action =
                    Some(crate::tui::commands::InputAction::ApplyRoutePreset(name));
            } else {
                picker.error =
                    Some("No route presets are configured; use /preset-save <name>".into());
            }
        }
        _ => {}
    }
    true
}

fn handle_reasoning_picker(state: &mut crate::app::AppState, code: KeyCode) -> bool {
    let Some(picker) = state.reasoning_picker.as_mut() else {
        return false;
    };
    match code {
        KeyCode::Esc => state.reasoning_picker = None,
        KeyCode::Up | KeyCode::Char('k') => {
            picker.selected_idx = picker.selected_idx.saturating_sub(1);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            picker.selected_idx =
                (picker.selected_idx + 1).min(picker.efforts.len().saturating_sub(1));
        }
        KeyCode::Home => picker.selected_idx = 0,
        KeyCode::End => picker.selected_idx = picker.efforts.len().saturating_sub(1),
        KeyCode::Enter => {
            if let Some(effort) = picker.selected() {
                state.reasoning_picker = None;
                state.pending_input_action =
                    Some(crate::tui::commands::InputAction::SelectReasoning(effort));
            }
        }
        _ => {}
    }
    true
}

fn handle_route_preset_overwrite(state: &mut crate::app::AppState, code: KeyCode) -> bool {
    let Some(confirmation) = state.route_preset_overwrite.as_mut() else {
        return false;
    };
    if confirmation.busy {
        return true;
    }
    match code {
        KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
            state.route_preset_overwrite = None;
        }
        KeyCode::Left | KeyCode::Right | KeyCode::Tab | KeyCode::Char('h') | KeyCode::Char('l') => {
            confirmation.save_selected = !confirmation.save_selected;
            confirmation.error = None;
        }
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            confirmation.save_selected = true;
            confirmation.error = None;
        }
        KeyCode::Enter if confirmation.save_selected => {
            confirmation.busy = true;
            confirmation.error = None;
            state.pending_input_action = Some(crate::tui::commands::InputAction::SaveRoutePreset {
                name: confirmation.name.clone(),
                preset: confirmation.preset.clone(),
            });
        }
        KeyCode::Enter => state.route_preset_overwrite = None,
        _ => {}
    }
    true
}

fn queue_command(
    state: &mut crate::app::AppState,
    action: crate::tui::commands::CommandAction,
    busy: bool,
) {
    let Some(spec) = crate::tui::commands::COMMANDS
        .iter()
        .find(|spec| spec.id == action.id)
    else {
        return;
    };
    if busy
        && matches!(
            spec.availability,
            crate::tui::commands::CommandAvailability::IdleOnly
        )
    {
        state.status_message = Some(format!(
            "/{} is unavailable while a turn is running",
            spec.name
        ));
    } else {
        state.pending_input_action = Some(crate::tui::commands::InputAction::Command(action));
    }
}

pub fn build_model_list(
    providers: &std::collections::HashMap<String, crate::config::ProviderConfig>,
) -> Vec<ModelInfo> {
    build_model_list_with_catalog(providers, &std::collections::HashMap::new(), None)
}

pub fn build_model_list_with_catalog(
    providers: &std::collections::HashMap<String, crate::config::ProviderConfig>,
    discovered: &std::collections::HashMap<String, Vec<String>>,
    preferred_provider: Option<&str>,
) -> Vec<ModelInfo> {
    let mut provider_names: Vec<&String> = providers.keys().collect();
    provider_names.sort_by(|left, right| {
        let left_preferred = preferred_provider.is_some_and(|preferred| preferred == left.as_str());
        let right_preferred =
            preferred_provider.is_some_and(|preferred| preferred == right.as_str());
        right_preferred
            .cmp(&left_preferred)
            .then_with(|| left.cmp(right))
    });
    let mut models = Vec::new();
    for name in provider_names {
        let mut model_ids = std::collections::BTreeSet::new();
        model_ids.extend(providers[name].models.iter().cloned());
        if let Some(live) = discovered.get(name) {
            model_ids.extend(live.iter().cloned());
        }
        for mid in model_ids {
            models.push(ModelInfo {
                id: mid.clone(),
                provider: name.clone(),
                display_name: mid,
            });
        }
    }
    if models.is_empty() {
        models.push(ModelInfo {
            id: "auto".into(),
            provider: "default".into(),
            display_name: "auto".into(),
        });
    }
    models
}

fn handle_config_hub(state: &mut crate::app::AppState, code: KeyCode) -> bool {
    let visible_count = state.config_hub.as_ref().map_or(0, |hub| {
        hub.items
            .iter()
            .filter(|item| item.matches(&hub.filter))
            .count()
    });
    match code {
        KeyCode::Esc => state.config_hub = None,
        KeyCode::Up => {
            if let Some(hub) = state.config_hub.as_mut() {
                hub.selected_idx = hub.selected_idx.saturating_sub(1);
            }
        }
        KeyCode::Down => {
            if let Some(hub) = state.config_hub.as_mut() {
                if hub.selected_idx + 1 < visible_count {
                    hub.selected_idx += 1;
                }
            }
        }
        KeyCode::Home => {
            if let Some(hub) = state.config_hub.as_mut() {
                hub.selected_idx = 0;
            }
        }
        KeyCode::End => {
            if let Some(hub) = state.config_hub.as_mut() {
                hub.selected_idx = visible_count.saturating_sub(1);
            }
        }
        KeyCode::PageUp => {
            if let Some(hub) = state.config_hub.as_mut() {
                hub.selected_idx = hub.selected_idx.saturating_sub(6);
            }
        }
        KeyCode::PageDown => {
            if let Some(hub) = state.config_hub.as_mut() {
                hub.selected_idx = hub
                    .selected_idx
                    .saturating_add(6)
                    .min(visible_count.saturating_sub(1));
            }
        }
        KeyCode::Backspace => {
            if let Some(hub) = state.config_hub.as_mut() {
                hub.filter.pop();
                hub.selected_idx = 0;
            }
        }
        KeyCode::Enter => {
            let action = state.config_hub.as_ref().and_then(|hub| {
                hub.items
                    .iter()
                    .filter(|item| item.matches(&hub.filter))
                    .nth(hub.selected_idx)
                    .map(|item| item.action.clone())
            });
            state.config_hub = None;
            match action {
                Some(crate::app::ConfigHubAction::Profile(profile)) => {
                    state.status_message = Some(format!("Active profile: {profile}"))
                }
                Some(crate::app::ConfigHubAction::CurrentModel) => open_model_picker(state),
                Some(crate::app::ConfigHubAction::Delegation {
                    selected,
                    configured,
                }) => open_delegation_model_picker(state, selected.as_ref(), &configured),
                Some(crate::app::ConfigHubAction::DelegationMode(mode)) => {
                    state.pending_input_action =
                        Some(crate::tui::commands::InputAction::SaveDelegationMode(mode));
                }
                Some(crate::app::ConfigHubAction::ReviewModel) => {
                    open_review_model_picker(state, None)
                }
                Some(crate::app::ConfigHubAction::ProviderSetup) => {
                    state.pending_input_action = Some(crate::tui::commands::InputAction::Command(
                        crate::tui::commands::CommandAction {
                            id: crate::tui::commands::CommandId::Provider,
                            args: "setup".into(),
                        },
                    ));
                }
                Some(crate::app::ConfigHubAction::Reasoning) => {
                    state.pending_input_action =
                        Some(crate::tui::commands::InputAction::CycleReasoning);
                }
                Some(crate::app::ConfigHubAction::Mcp) => {
                    state.pending_input_action = Some(crate::tui::commands::InputAction::Command(
                        crate::tui::commands::CommandAction {
                            id: crate::tui::commands::CommandId::Mcp,
                            args: String::new(),
                        },
                    ));
                }
                Some(crate::app::ConfigHubAction::SelfImprovement) => {
                    state.pending_input_action = Some(crate::tui::commands::InputAction::Command(
                        crate::tui::commands::CommandAction {
                            id: crate::tui::commands::CommandId::SelfImprove,
                            args: String::new(),
                        },
                    ));
                }
                Some(crate::app::ConfigHubAction::Skills) => {
                    state.pending_input_action = Some(crate::tui::commands::InputAction::Command(
                        crate::tui::commands::CommandAction {
                            id: crate::tui::commands::CommandId::Skills,
                            args: String::new(),
                        },
                    ));
                }
                Some(crate::app::ConfigHubAction::Hooks) => {
                    state.pending_input_action = Some(crate::tui::commands::InputAction::Command(
                        crate::tui::commands::CommandAction {
                            id: crate::tui::commands::CommandId::Hooks,
                            args: String::new(),
                        },
                    ));
                }
                Some(crate::app::ConfigHubAction::Sandbox) => {
                    state.pending_input_action = Some(crate::tui::commands::InputAction::Command(
                        crate::tui::commands::CommandAction {
                            id: crate::tui::commands::CommandId::Sandbox,
                            args: String::new(),
                        },
                    ));
                }
                Some(crate::app::ConfigHubAction::ContextLimits) => {
                    state.pending_input_action = Some(crate::tui::commands::InputAction::Command(
                        crate::tui::commands::CommandAction {
                            id: crate::tui::commands::CommandId::ContextLimit,
                            args: String::new(),
                        },
                    ));
                }
                Some(crate::app::ConfigHubAction::SessionRetention) => {
                    state.pending_input_action = Some(crate::tui::commands::InputAction::Command(
                        crate::tui::commands::CommandAction {
                            id: crate::tui::commands::CommandId::Retention,
                            args: String::new(),
                        },
                    ));
                }
                Some(crate::app::ConfigHubAction::Notification) => {
                    let saved = state.notification_style;
                    let selected_idx = crate::ui_settings::NotificationStyle::ALL
                        .iter()
                        .position(|style| *style == saved)
                        .unwrap_or(0);
                    state.notification_picker = Some(crate::app::NotificationPickerState {
                        saved,
                        selected_idx,
                        busy: false,
                        error: None,
                    });
                }
                Some(crate::app::ConfigHubAction::Mouse) => {
                    if state.run_capabilities.config_writes {
                        state.pending_input_action =
                            Some(crate::tui::commands::InputAction::SaveMouse(
                                state.mouse_mode.toggled(),
                            ));
                        state.status_message = Some("Saving trackpad scrolling preference…".into());
                    } else {
                        state.status_message =
                            Some("Bare mode cannot save trackpad scrolling preferences".into());
                    }
                }
                Some(crate::app::ConfigHubAction::CommandOutput) => {
                    let saved = state.command_output_policy;
                    let selected_idx = crate::config::CommandOutputPolicy::ALL
                        .iter()
                        .position(|policy| *policy == saved)
                        .unwrap_or(0);
                    state.command_output_picker = Some(crate::app::CommandOutputPickerState {
                        saved,
                        selected_idx,
                        busy: false,
                        error: None,
                    });
                }
                Some(crate::app::ConfigHubAction::ConfigFile(path)) => {
                    state.status_message = Some(path)
                }
                None => state.status_message = Some("No matching settings".into()),
            }
        }
        KeyCode::Char(character) if !character.is_control() => {
            if let Some(hub) = state.config_hub.as_mut() {
                hub.filter.push(character);
                hub.selected_idx = 0;
            }
        }
        _ => {}
    }
    true
}

fn visible_hook_indices(manager: &crate::app::HookManagerState) -> Vec<usize> {
    let filter = manager.filter.trim().to_ascii_lowercase();
    manager
        .entries
        .iter()
        .enumerate()
        .filter_map(|(position, entry)| {
            let searchable = format!(
                "{} {} {}",
                entry.event.as_str(),
                if entry.enabled { "enabled" } else { "disabled" },
                if entry.fail_closed {
                    "fail-closed"
                } else {
                    "fail-open"
                }
            );
            (filter.is_empty() || searchable.contains(&filter)).then_some(position)
        })
        .collect()
}

fn handle_hook_manager(
    state: &mut crate::app::AppState,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> bool {
    if code == KeyCode::Esc {
        if let Some(manager) = state.hook_manager.as_mut() {
            if manager.busy {
                if manager.testing {
                    state.hook_manager_generation = state.hook_manager_generation.wrapping_add(1);
                    manager.busy = false;
                    manager.testing = false;
                    manager.notice = None;
                    manager.error = Some("Hook test cancelled".into());
                }
                return true;
            }
            if state.hook_blocked {
                manager.error = Some(
                    "Session start remains blocked; disable or repair the failing hook, then reload"
                        .into(),
                );
                return true;
            }
        }
        state.hook_manager_generation = state.hook_manager_generation.wrapping_add(1);
        state.hook_manager_request = None;
        state.hook_manager = None;
        return true;
    }
    let Some(manager) = state.hook_manager.as_mut() else {
        return true;
    };
    if manager.busy {
        return true;
    }
    let visible = visible_hook_indices(manager);
    let visible_count = visible.len();
    match (modifiers, code) {
        (KeyModifiers::NONE, KeyCode::Up) => {
            manager.selected_idx = manager.selected_idx.saturating_sub(1);
        }
        (KeyModifiers::NONE, KeyCode::Down) => {
            manager.selected_idx = manager
                .selected_idx
                .saturating_add(1)
                .min(visible_count.saturating_sub(1));
        }
        (KeyModifiers::NONE, KeyCode::Home) => {
            manager.selected_idx = 0;
        }
        (KeyModifiers::NONE, KeyCode::End) => {
            manager.selected_idx = visible_count.saturating_sub(1);
        }
        (KeyModifiers::NONE, KeyCode::PageUp) => {
            manager.selected_idx = manager.selected_idx.saturating_sub(6);
        }
        (KeyModifiers::NONE, KeyCode::PageDown) => {
            manager.selected_idx = manager
                .selected_idx
                .saturating_add(6)
                .min(visible_count.saturating_sub(1));
        }
        (KeyModifiers::NONE, KeyCode::Backspace) => {
            manager.filter.pop();
            manager.selected_idx = 0;
            manager.error = None;
        }
        (KeyModifiers::SHIFT, KeyCode::Char('r' | 'R')) => {
            request_hook_manager_operation(state, crate::app::HookManagerOperation::Reload);
        }
        (KeyModifiers::SHIFT, KeyCode::Char('t' | 'T')) => {
            if !manager.execution_enabled {
                manager.error = Some(
                    "Hook execution is disabled in the bare profile; rerun without --profile bare"
                        .into(),
                );
            } else if let Some(entry) = visible
                .get(manager.selected_idx)
                .and_then(|position| manager.entries.get(*position))
                .cloned()
            {
                request_hook_manager_operation(
                    state,
                    crate::app::HookManagerOperation::Test {
                        index: entry.index,
                        event: entry.event,
                    },
                );
            }
        }
        (KeyModifiers::NONE, KeyCode::Enter)
        | (KeyModifiers::NONE, KeyCode::Char(' '))
        | (KeyModifiers::SHIFT, KeyCode::Char('e' | 'E')) => {
            if !manager.writes_enabled {
                manager.error = Some(
                    "Hook changes are disabled in the bare profile; edit the normal config file and restart"
                        .into(),
                );
            } else if let Some(entry) = visible
                .get(manager.selected_idx)
                .and_then(|position| manager.entries.get(*position))
                .cloned()
            {
                request_hook_manager_operation(
                    state,
                    crate::app::HookManagerOperation::Toggle {
                        index: entry.index,
                        event: entry.event,
                        enabled: !entry.enabled,
                    },
                );
            }
        }
        (KeyModifiers::NONE, KeyCode::Char(character)) if !character.is_control() => {
            manager.filter.push(character);
            manager.selected_idx = 0;
            manager.error = None;
        }
        _ => {}
    }
    true
}

fn visible_permission_indices(manager: &crate::app::PermissionManagerState) -> Vec<usize> {
    let filter = manager.filter.trim().to_ascii_lowercase();
    manager
        .entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| {
            let searchable = format!("{:?} {}", entry.decision, entry.matcher).to_ascii_lowercase();
            (filter.is_empty() || searchable.contains(&filter)).then_some(index)
        })
        .collect()
}

fn handle_permission_manager(
    state: &mut crate::app::AppState,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> bool {
    let Some(manager) = state.permission_manager.as_mut() else {
        return true;
    };
    if let Some(confirmation) = manager.confirmation.as_mut() {
        if manager.busy {
            return true;
        }
        match (modifiers, code) {
            (_, KeyCode::Esc) => manager.confirmation = None,
            (KeyModifiers::NONE, KeyCode::Left | KeyCode::Right | KeyCode::Tab)
            | (KeyModifiers::SHIFT, KeyCode::BackTab) => {
                confirmation.delete_selected = !confirmation.delete_selected;
            }
            (KeyModifiers::NONE, KeyCode::Char('c' | 'C')) => manager.confirmation = None,
            (KeyModifiers::NONE, KeyCode::Enter) if !confirmation.delete_selected => {
                manager.confirmation = None;
            }
            (KeyModifiers::NONE, KeyCode::Enter) => {
                let source = confirmation.source;
                if !manager.writes_enabled
                    && !matches!(source, crate::app::PermissionEntrySource::Session(_))
                {
                    manager.error = Some(
                        "Persistent permission changes are disabled in bare mode; rerun without --profile bare"
                            .into(),
                    );
                    manager.confirmation = None;
                } else {
                    manager.busy = true;
                    manager.error = None;
                    state.pending_input_action =
                        Some(crate::tui::commands::InputAction::DeletePermission(source));
                }
            }
            _ => {}
        }
        return true;
    }
    if manager.busy {
        return true;
    }
    if code == KeyCode::Esc {
        state.permission_manager_generation = state.permission_manager_generation.wrapping_add(1);
        state.permission_manager = None;
        return true;
    }
    let visible = visible_permission_indices(manager);
    let visible_count = visible.len();
    match (modifiers, code) {
        (KeyModifiers::NONE, KeyCode::Up) => {
            manager.selected_idx =
                crate::tui::selection::previous_wrap(manager.selected_idx, visible_count);
        }
        (KeyModifiers::NONE, KeyCode::Down) => {
            manager.selected_idx =
                crate::tui::selection::next_wrap(manager.selected_idx, visible_count);
        }
        (KeyModifiers::NONE, KeyCode::Home) => manager.selected_idx = 0,
        (KeyModifiers::NONE, KeyCode::End) => {
            manager.selected_idx = visible_count.saturating_sub(1);
        }
        (KeyModifiers::NONE, KeyCode::PageUp) => {
            manager.selected_idx = crate::tui::selection::page_up(manager.selected_idx, 6);
        }
        (KeyModifiers::NONE, KeyCode::PageDown) => {
            manager.selected_idx =
                crate::tui::selection::page_down(manager.selected_idx, visible_count, 6);
        }
        (KeyModifiers::NONE, KeyCode::Backspace) => {
            manager.filter.pop();
            manager.selected_idx = 0;
            manager.error = None;
        }
        (KeyModifiers::SHIFT, KeyCode::Char('d' | 'D')) => {
            if let Some(entry) = visible
                .get(manager.selected_idx)
                .and_then(|index| manager.entries.get(*index))
                .cloned()
            {
                manager.confirmation = Some(crate::app::PermissionDeleteConfirmation {
                    source: entry.source,
                    label: format!("{:?} · {}", entry.decision, entry.matcher),
                    delete_selected: false,
                });
                manager.error = None;
            }
        }
        (KeyModifiers::NONE, KeyCode::Char(character)) if !character.is_control() => {
            manager.filter.push(character);
            manager.selected_idx = 0;
            manager.error = None;
        }
        _ => {}
    }
    true
}

fn visible_memory_record_indices(state: &crate::app::AppState) -> Vec<usize> {
    let Some(manager) = state.memory_manager.as_ref() else {
        return Vec::new();
    };
    let filter = manager.filter.trim().to_ascii_lowercase();
    let records = match manager.view {
        crate::app::MemoryManagerView::User => &state.user_memory_records,
        crate::app::MemoryManagerView::Workspace => &state.workspace_memory_records,
        crate::app::MemoryManagerView::Proposals => return Vec::new(),
    };
    records
        .iter()
        .enumerate()
        .filter_map(|(index, record)| {
            let searchable = format!("{:?} {}", record.kind, record.text).to_ascii_lowercase();
            (filter.is_empty() || searchable.contains(&filter)).then_some(index)
        })
        .collect()
}

fn visible_memory_proposal_indices(state: &crate::app::AppState) -> Vec<usize> {
    let Some(manager) = state.memory_manager.as_ref() else {
        return Vec::new();
    };
    if manager.view != crate::app::MemoryManagerView::Proposals {
        return Vec::new();
    }
    let filter = manager.filter.trim().to_ascii_lowercase();
    state
        .memory_proposals
        .iter()
        .enumerate()
        .filter_map(|(index, proposal)| {
            let searchable = format!("{:?} {}", proposal.kind, proposal.text).to_ascii_lowercase();
            (filter.is_empty() || searchable.contains(&filter)).then_some(index)
        })
        .collect()
}

fn next_memory_kind(
    scope: crate::memory::MemoryScope,
    kind: crate::memory::MemoryKind,
) -> crate::memory::MemoryKind {
    use crate::memory::{MemoryKind, MemoryScope};
    match (scope, kind) {
        (MemoryScope::User, MemoryKind::UserPreference) => MemoryKind::UserWorkflow,
        (MemoryScope::User, MemoryKind::UserWorkflow) => MemoryKind::ExternalFact,
        (MemoryScope::User, _) => MemoryKind::UserPreference,
        (MemoryScope::Workspace, MemoryKind::WorkspaceConvention) => MemoryKind::WorkspaceDecision,
        (MemoryScope::Workspace, MemoryKind::WorkspaceDecision) => MemoryKind::ExternalFact,
        (MemoryScope::Workspace, _) => MemoryKind::WorkspaceConvention,
    }
}

fn handle_memory_manager(
    state: &mut crate::app::AppState,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> bool {
    let Some(manager) = state.memory_manager.as_mut() else {
        return true;
    };
    if manager.busy {
        return true;
    }
    if let Some(editor) = manager.editor.as_mut() {
        match (modifiers, code) {
            (_, KeyCode::Esc) => manager.editor = None,
            (KeyModifiers::NONE, KeyCode::Tab) => {
                editor.kind = next_memory_kind(editor.scope, editor.kind)
            }
            (KeyModifiers::NONE, KeyCode::Backspace) => {
                if editor.cursor > 0 {
                    let previous = previous_char_boundary(&editor.text, editor.cursor);
                    editor.text.drain(previous..editor.cursor);
                    editor.cursor = previous;
                }
            }
            (KeyModifiers::NONE, KeyCode::Left) => {
                editor.cursor = previous_char_boundary(&editor.text, editor.cursor)
            }
            (KeyModifiers::NONE, KeyCode::Right) => {
                editor.cursor = next_char_boundary(&editor.text, editor.cursor)
            }
            (KeyModifiers::NONE, KeyCode::Enter) => {
                manager.busy = true;
                manager.error = None;
                state.pending_input_action =
                    Some(crate::tui::commands::InputAction::SaveMemoryRecord {
                        id: editor.id,
                        scope: editor.scope,
                        kind: editor.kind,
                        text: editor.text.clone(),
                    });
            }
            (KeyModifiers::NONE, KeyCode::Char(character)) if !character.is_control() => {
                editor.text.insert(editor.cursor, character);
                editor.cursor += character.len_utf8();
            }
            _ => {}
        }
        return true;
    }
    if let Some(confirmation) = manager.confirmation.as_mut() {
        match (modifiers, code) {
            (_, KeyCode::Esc) | (KeyModifiers::NONE, KeyCode::Char('c' | 'C')) => {
                manager.confirmation = None
            }
            (KeyModifiers::NONE, KeyCode::Left | KeyCode::Right | KeyCode::Tab)
            | (KeyModifiers::SHIFT, KeyCode::BackTab) => {
                confirmation.confirm_selected = !confirmation.confirm_selected
            }
            (KeyModifiers::NONE, KeyCode::Enter) if !confirmation.confirm_selected => {
                manager.confirmation = None
            }
            (KeyModifiers::NONE, KeyCode::Enter) => {
                if !manager.writes_enabled {
                    manager.error = Some(
                        "Memory changes are disabled in bare mode; rerun without --profile bare"
                            .into(),
                    );
                    manager.confirmation = None;
                } else {
                    manager.busy = true;
                    state.pending_input_action = Some(match confirmation.action {
                        crate::app::MemoryConfirmationAction::Delete { scope, id } => {
                            crate::tui::commands::InputAction::DeleteMemoryRecord { scope, id }
                        }
                        crate::app::MemoryConfirmationAction::AcceptProposal { id } => {
                            crate::tui::commands::InputAction::ApproveMemoryProposal(id)
                        }
                    });
                }
            }
            _ => {}
        }
        return true;
    }
    if code == KeyCode::Esc {
        state.memory_manager_generation = state.memory_manager_generation.wrapping_add(1);
        state.memory_manager = None;
        return true;
    }
    let _ = manager;
    let record_indices = visible_memory_record_indices(state);
    let proposal_indices = visible_memory_proposal_indices(state);
    let manager = state
        .memory_manager
        .as_mut()
        .expect("memory manager is open");
    let count = if manager.view == crate::app::MemoryManagerView::Proposals {
        proposal_indices.len()
    } else {
        record_indices.len()
    };
    match (modifiers, code) {
        (KeyModifiers::NONE, KeyCode::Tab) | (KeyModifiers::SHIFT, KeyCode::BackTab) => {
            manager.view = manager.view.next();
            manager.selected_idx = 0;
            manager.filter.clear();
            manager.error = None;
        }
        (KeyModifiers::NONE, KeyCode::Up) => {
            manager.selected_idx = manager.selected_idx.saturating_sub(1)
        }
        (KeyModifiers::NONE, KeyCode::Down) => {
            manager.selected_idx = manager
                .selected_idx
                .saturating_add(1)
                .min(count.saturating_sub(1))
        }
        (KeyModifiers::NONE, KeyCode::Home) => manager.selected_idx = 0,
        (KeyModifiers::NONE, KeyCode::End) => manager.selected_idx = count.saturating_sub(1),
        (KeyModifiers::NONE, KeyCode::Backspace) => {
            manager.filter.pop();
            manager.selected_idx = 0;
            manager.error = None;
        }
        (mods, KeyCode::Char('a' | 'A')) if mods.contains(KeyModifiers::SHIFT) => {
            if !manager.writes_enabled {
                manager.error = Some(
                    "Memory changes are disabled in bare mode; rerun without --profile bare".into(),
                );
            } else if manager.view != crate::app::MemoryManagerView::Proposals {
                let (scope, kind) = match manager.view {
                    crate::app::MemoryManagerView::User => (
                        crate::memory::MemoryScope::User,
                        crate::memory::MemoryKind::UserPreference,
                    ),
                    crate::app::MemoryManagerView::Workspace => (
                        crate::memory::MemoryScope::Workspace,
                        crate::memory::MemoryKind::WorkspaceConvention,
                    ),
                    crate::app::MemoryManagerView::Proposals => unreachable!(),
                };
                manager.editor = Some(crate::app::MemoryEditorState {
                    id: None,
                    scope,
                    kind,
                    text: String::new(),
                    cursor: 0,
                });
            }
        }
        (mods, KeyCode::Char('e' | 'E')) if mods.contains(KeyModifiers::SHIFT) => {
            if let Some(index) = record_indices.get(manager.selected_idx).copied() {
                let record = match manager.view {
                    crate::app::MemoryManagerView::User => &state.user_memory_records[index],
                    crate::app::MemoryManagerView::Workspace => {
                        &state.workspace_memory_records[index]
                    }
                    crate::app::MemoryManagerView::Proposals => return true,
                };
                manager.editor = Some(crate::app::MemoryEditorState {
                    id: Some(record.id),
                    scope: record.scope,
                    kind: record.kind,
                    text: record.text.clone(),
                    cursor: record.text.len(),
                });
            }
        }
        (mods, KeyCode::Char('d' | 'D')) if mods.contains(KeyModifiers::SHIFT) => {
            if manager.view == crate::app::MemoryManagerView::Proposals {
                if let Some(index) = proposal_indices.get(manager.selected_idx).copied() {
                    state.memory_proposals.remove(index);
                    manager.selected_idx = manager
                        .selected_idx
                        .min(state.memory_proposals.len().saturating_sub(1));
                    manager.notice = Some("Proposal rejected; nothing was written".into());
                }
            } else if let Some(index) = record_indices.get(manager.selected_idx).copied() {
                let record = match manager.view {
                    crate::app::MemoryManagerView::User => &state.user_memory_records[index],
                    crate::app::MemoryManagerView::Workspace => {
                        &state.workspace_memory_records[index]
                    }
                    crate::app::MemoryManagerView::Proposals => return true,
                };
                manager.confirmation = Some(crate::app::MemoryConfirmation {
                    action: crate::app::MemoryConfirmationAction::Delete {
                        scope: record.scope,
                        id: record.id,
                    },
                    confirm_selected: false,
                });
            }
        }
        (KeyModifiers::NONE, KeyCode::Enter)
            if manager.view == crate::app::MemoryManagerView::Proposals =>
        {
            if let Some(index) = proposal_indices.get(manager.selected_idx).copied() {
                manager.confirmation = Some(crate::app::MemoryConfirmation {
                    action: crate::app::MemoryConfirmationAction::AcceptProposal {
                        id: state.memory_proposals[index].id,
                    },
                    confirm_selected: false,
                });
            }
        }
        (mods, KeyCode::Char('r' | 'R')) if mods.contains(KeyModifiers::SHIFT) => {
            let (scope, recall, generation) = match manager.view {
                crate::app::MemoryManagerView::User => (
                    crate::memory::MemoryScope::User,
                    !manager.user_recall,
                    manager.user_generation,
                ),
                crate::app::MemoryManagerView::Workspace => (
                    crate::memory::MemoryScope::Workspace,
                    !manager.workspace_recall,
                    manager.workspace_generation,
                ),
                crate::app::MemoryManagerView::Proposals => return true,
            };
            manager.busy = true;
            state.pending_input_action =
                Some(crate::tui::commands::InputAction::SaveMemorySettings {
                    scope,
                    recall,
                    generation,
                });
        }
        (mods, KeyCode::Char('g' | 'G')) if mods.contains(KeyModifiers::SHIFT) => {
            let (scope, recall, generation) = match manager.view {
                crate::app::MemoryManagerView::User => (
                    crate::memory::MemoryScope::User,
                    manager.user_recall,
                    !manager.user_generation,
                ),
                crate::app::MemoryManagerView::Workspace => (
                    crate::memory::MemoryScope::Workspace,
                    manager.workspace_recall,
                    !manager.workspace_generation,
                ),
                crate::app::MemoryManagerView::Proposals => return true,
            };
            manager.busy = true;
            state.pending_input_action =
                Some(crate::tui::commands::InputAction::SaveMemorySettings {
                    scope,
                    recall,
                    generation,
                });
        }
        (mods, KeyCode::Char('x' | 'X')) if mods.contains(KeyModifiers::SHIFT) => {
            manager.busy = true;
            state.pending_input_action = Some(crate::tui::commands::InputAction::ExportMemory(
                std::path::Path::new(&state.cwd).join("ca-memory-export.md"),
            ));
        }
        (KeyModifiers::NONE, KeyCode::Char(character)) if !character.is_control() => {
            manager.filter.push(character);
            manager.selected_idx = 0;
            manager.error = None;
        }
        _ => {}
    }
    true
}

fn request_hook_manager_operation(
    state: &mut crate::app::AppState,
    operation: crate::app::HookManagerOperation,
) {
    state.hook_manager_generation = state.hook_manager_generation.wrapping_add(1);
    let generation = state.hook_manager_generation;
    if let Some(manager) = state.hook_manager.as_mut() {
        manager.busy = true;
        manager.testing = matches!(&operation, crate::app::HookManagerOperation::Test { .. });
        manager.error = None;
        manager.notice = None;
    }
    state.hook_manager_request = Some(crate::app::HookManagerRequest {
        generation,
        operation,
    });
}

fn handle_context_limit_editor(state: &mut crate::app::AppState, code: KeyCode) -> bool {
    if state
        .context_limit_editor
        .as_ref()
        .is_some_and(|editor| editor.saving)
    {
        return true;
    }
    if code == KeyCode::Esc {
        state.context_limit_editor = None;
        return true;
    }
    let Some(editor) = state.context_limit_editor.as_mut() else {
        return true;
    };
    match code {
        KeyCode::Up | KeyCode::Down | KeyCode::Tab | KeyCode::BackTab => {
            editor.field = match editor.field {
                crate::app::ContextLimitField::Window => crate::app::ContextLimitField::Threshold,
                crate::app::ContextLimitField::Threshold => crate::app::ContextLimitField::Window,
            };
            editor.cursor = editor.active_input().len();
            editor.error = None;
        }
        KeyCode::Home => editor.cursor = 0,
        KeyCode::End => editor.cursor = editor.active_input().len(),
        KeyCode::Left => {
            editor.cursor = previous_char_boundary(editor.active_input(), editor.cursor);
        }
        KeyCode::Right => {
            editor.cursor = next_char_boundary(editor.active_input(), editor.cursor);
        }
        KeyCode::Backspace => {
            let previous = previous_char_boundary(editor.active_input(), editor.cursor);
            if previous < editor.cursor {
                let cursor = editor.cursor;
                editor
                    .active_input_mut()
                    .replace_range(previous..cursor, "");
                editor.cursor = previous;
            }
            editor.error = None;
        }
        KeyCode::Delete => {
            let cursor = editor.cursor;
            let next = next_char_boundary(editor.active_input(), cursor);
            if next > cursor {
                editor.active_input_mut().replace_range(cursor..next, "");
            }
            editor.error = None;
        }
        KeyCode::Char(character) if character.is_ascii_digit() => {
            insert_context_limit_char(editor, character);
            editor.error = None;
        }
        KeyCode::Char(character) if !character.is_control() => {
            editor.error = Some("Use decimal digits only; empty inherits".into());
        }
        KeyCode::Enter => {
            if !editor.writes_enabled {
                editor.error = Some("View-only; rerun without --profile bare".into());
                return true;
            }
            match context_limit_update_from_editor(editor) {
                Ok(update) => {
                    editor.saving = true;
                    editor.error = None;
                    state.pending_input_action =
                        Some(crate::tui::commands::InputAction::SaveContextLimits(update));
                }
                Err(error) => editor.error = Some(error),
            }
        }
        _ => {}
    }
    true
}

fn handle_model_prompt_cache_editor(state: &mut crate::app::AppState, code: KeyCode) -> bool {
    if state
        .model_prompt_cache_editor
        .as_ref()
        .is_some_and(|editor| editor.saving)
    {
        return true;
    }
    if code == KeyCode::Esc {
        state.model_prompt_cache_editor = None;
        return true;
    }
    let Some(editor) = state.model_prompt_cache_editor.as_mut() else {
        return true;
    };
    use crate::app::ModelPromptCacheField;
    match code {
        KeyCode::Tab | KeyCode::Down => {
            editor.field = if editor.formats_locked {
                ModelPromptCacheField::Policy
            } else {
                match editor.field {
                    ModelPromptCacheField::RequestFormat => ModelPromptCacheField::UsageFormat,
                    ModelPromptCacheField::UsageFormat => ModelPromptCacheField::Policy,
                    ModelPromptCacheField::Policy => ModelPromptCacheField::RequestFormat,
                }
            };
            editor.error = None;
        }
        KeyCode::BackTab | KeyCode::Up => {
            editor.field = if editor.formats_locked {
                ModelPromptCacheField::Policy
            } else {
                match editor.field {
                    ModelPromptCacheField::RequestFormat => ModelPromptCacheField::Policy,
                    ModelPromptCacheField::UsageFormat => ModelPromptCacheField::RequestFormat,
                    ModelPromptCacheField::Policy => ModelPromptCacheField::UsageFormat,
                }
            };
            editor.error = None;
        }
        KeyCode::Left => {
            cycle_model_prompt_cache_value(editor, true);
            editor.error = None;
        }
        KeyCode::Right | KeyCode::Char(' ') => {
            cycle_model_prompt_cache_value(editor, false);
            editor.error = None;
        }
        KeyCode::Backspace | KeyCode::Delete => {
            if !editor.formats_locked {
                match editor.field {
                    ModelPromptCacheField::RequestFormat => {
                        editor.request_format = None;
                        editor.policy_override = None;
                    }
                    ModelPromptCacheField::UsageFormat => editor.usage_format = None,
                    ModelPromptCacheField::Policy => editor.policy_override = None,
                }
            } else if editor.field == ModelPromptCacheField::Policy {
                editor.policy_override = None;
            }
            editor.error = None;
        }
        KeyCode::Enter => {
            if !editor.writes_enabled {
                editor.error = Some("View-only; rerun without --profile bare".into());
                return true;
            }
            if !editor.formats_locked
                && editor.policy_override.is_some()
                && editor.request_format.is_none()
            {
                editor.error = Some("Choose a request format before a policy".into());
                return true;
            }
            if editor.policy_override.is_some_and(|policy| {
                editor
                    .request_format
                    .is_none_or(|format| !format.supports_policy(policy))
            }) {
                editor.error = Some("That policy is unavailable for this request format".into());
                return true;
            }
            editor.saving = true;
            editor.error = None;
            state.pending_input_action = Some(
                crate::tui::commands::InputAction::SaveModelPromptCache(editor.update()),
            );
        }
        _ => {}
    }
    true
}

fn cycle_model_prompt_cache_value(
    editor: &mut crate::app::ModelPromptCacheEditorState,
    backwards: bool,
) {
    use crate::app::ModelPromptCacheField;
    match editor.field {
        ModelPromptCacheField::RequestFormat if !editor.formats_locked => {
            let mut values = vec![
                None,
                Some(crate::config::PromptCacheRequestFormat::ProviderManaged),
            ];
            if editor.protocol == crate::agent::ProviderProtocol::OpenAiResponses {
                values.push(Some(
                    crate::config::PromptCacheRequestFormat::OpenAiResponsesExplicit,
                ));
            }
            editor.request_format = cycle_optional_value(&values, editor.request_format, backwards);
            if editor.policy_override.is_some_and(|policy| {
                editor
                    .request_format
                    .is_none_or(|format| !format.supports_policy(policy))
            }) {
                editor.policy_override = None;
            }
        }
        ModelPromptCacheField::UsageFormat if !editor.formats_locked => {
            use crate::config::PromptCacheUsageFormat;
            let values = match editor.protocol {
                crate::agent::ProviderProtocol::ChatCompletions => {
                    vec![
                        None,
                        Some(PromptCacheUsageFormat::Auto),
                        Some(PromptCacheUsageFormat::OpenAiDetails),
                        Some(PromptCacheUsageFormat::HitMiss),
                    ]
                }
                crate::agent::ProviderProtocol::OpenAiResponses => {
                    vec![
                        None,
                        Some(PromptCacheUsageFormat::Auto),
                        Some(PromptCacheUsageFormat::OpenAiDetails),
                    ]
                }
                crate::agent::ProviderProtocol::AnthropicMessages => {
                    vec![
                        None,
                        Some(PromptCacheUsageFormat::Auto),
                        Some(PromptCacheUsageFormat::ReadWrite),
                    ]
                }
            };
            editor.usage_format = cycle_optional_value(&values, editor.usage_format, backwards);
        }
        ModelPromptCacheField::Policy => {
            let values = std::iter::once(None)
                .chain(editor.supported_policies().iter().copied().map(Some))
                .collect::<Vec<_>>();
            editor.policy_override =
                cycle_optional_value(&values, editor.policy_override, backwards);
        }
        ModelPromptCacheField::RequestFormat | ModelPromptCacheField::UsageFormat => {}
    }
}

fn cycle_optional_value<T: Copy + PartialEq>(
    values: &[Option<T>],
    current: Option<T>,
    backwards: bool,
) -> Option<T> {
    let current = values
        .iter()
        .position(|value| *value == current)
        .unwrap_or(0);
    let next = if backwards {
        current
            .checked_sub(1)
            .unwrap_or(values.len().saturating_sub(1))
    } else {
        (current + 1) % values.len().max(1)
    };
    values.get(next).copied().flatten()
}

fn insert_context_limit_char(editor: &mut crate::app::ContextLimitEditorState, character: char) {
    let cursor = editor.cursor.min(editor.active_input().len());
    editor.active_input_mut().insert(cursor, character);
    editor.cursor = cursor + character.len_utf8();
}

fn context_limit_update_from_editor(
    editor: &crate::app::ContextLimitEditorState,
) -> Result<crate::config::ContextLimitUpdate, String> {
    fn parse(label: &str, value: &str) -> Result<Option<u64>, String> {
        if value.is_empty() {
            return Ok(None);
        }
        let parsed = value
            .parse::<u64>()
            .map_err(|_| format!("{label} must be a positive whole number"))?;
        if parsed == 0 {
            return Err(format!("{label} must be greater than zero"));
        }
        Ok(Some(parsed))
    }

    let context_window_tokens = parse("Context window", &editor.window_input)?;
    let compact_at_tokens = parse("Checkpoint threshold", &editor.threshold_input)?;
    let effective_window = context_window_tokens.or(editor.fallback_window);
    let effective_threshold = compact_at_tokens.or(editor.fallback_threshold);
    if let Some(threshold) = effective_threshold {
        let Some(window) = effective_window else {
            return Err("Checkpoint threshold needs a known context window".into());
        };
        if threshold >= window {
            return Err("Checkpoint threshold must be below the context window".into());
        }
    }
    Ok(crate::config::ContextLimitUpdate {
        provider: editor.provider.clone(),
        model: editor.model.clone(),
        context_window_tokens,
        compact_at_tokens,
    })
}

fn handle_sandbox_hub(state: &mut crate::app::AppState, code: KeyCode) -> bool {
    let Some(hub) = state.sandbox_hub.as_mut() else {
        return true;
    };
    if hub.busy {
        return true;
    }
    if let Some(confirmation) = hub.confirmation.as_mut() {
        match code {
            KeyCode::Esc => hub.confirmation = None,
            KeyCode::Left | KeyCode::Right | KeyCode::Tab | KeyCode::BackTab => {
                confirmation.confirm_selected = !confirmation.confirm_selected;
            }
            KeyCode::Char('c' | 'C') => hub.confirmation = None,
            KeyCode::Char('s' | 'S') => {
                confirmation.confirm_selected = true;
                let target = confirmation.target;
                hub.confirmation = None;
                hub.busy = true;
                hub.error = None;
                hub.notice = Some(format!("Saving {target} for the next launch…"));
                state.pending_input_action =
                    Some(crate::tui::commands::InputAction::SaveSandboxMode(target));
            }
            KeyCode::Enter if confirmation.confirm_selected => {
                let target = confirmation.target;
                hub.confirmation = None;
                hub.busy = true;
                hub.error = None;
                hub.notice = Some(format!("Saving {target} for the next launch…"));
                state.pending_input_action =
                    Some(crate::tui::commands::InputAction::SaveSandboxMode(target));
            }
            KeyCode::Enter => hub.confirmation = None,
            _ => {}
        }
        return true;
    }

    match code {
        KeyCode::Esc => state.sandbox_hub = None,
        KeyCode::Up => hub.selected_idx = hub.selected_idx.saturating_sub(1),
        KeyCode::Down => {
            hub.selected_idx =
                (hub.selected_idx + 1).min(crate::app::SandboxHubState::MODES.len() - 1)
        }
        KeyCode::Home | KeyCode::PageUp => hub.selected_idx = 0,
        KeyCode::End | KeyCode::PageDown => {
            hub.selected_idx = crate::app::SandboxHubState::MODES.len() - 1
        }
        KeyCode::Enter => {
            let target = hub.selected_mode();
            hub.error = None;
            if !hub.writes_enabled {
                hub.error = Some("View-only; rerun without --profile bare".into());
            } else if target == hub.saved_mode {
                hub.notice = Some(format!("{target} is already saved"));
            } else if target.is_weaker_than(crate::sandbox::SandboxMode::Required) {
                hub.confirmation = Some(crate::app::SandboxConfirmationState {
                    target,
                    confirm_selected: false,
                });
            } else {
                hub.busy = true;
                hub.notice = Some(format!("Saving {target} for the next launch…"));
                state.pending_input_action =
                    Some(crate::tui::commands::InputAction::SaveSandboxMode(target));
            }
        }
        _ => {}
    }
    true
}

fn handle_session_picker(
    state: &mut crate::app::AppState,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> bool {
    if state.session_rename.is_some() {
        return handle_session_rename(state, code);
    }
    if state.session_archive_confirmation.is_some() {
        return handle_session_archive_confirmation(state, code);
    }
    if state.session_delete_confirmation.is_some() {
        return handle_session_delete_confirmation(state, code);
    }
    if state.session_picker_busy {
        return true;
    }
    match code {
        KeyCode::Esc => {
            state.session_picker_open = false;
            state.session_picker_error = None;
        }
        KeyCode::Tab | KeyCode::BackTab => {
            state.session_picker_view = match state.session_picker_view {
                crate::app::SessionPickerView::Active => crate::app::SessionPickerView::Archive,
                crate::app::SessionPickerView::Archive => crate::app::SessionPickerView::Active,
            };
            let count = visible_session_count(state);
            state.selected_session_idx = state.selected_session_idx.min(count.saturating_sub(1));
            state.session_picker_error = None;
        }
        KeyCode::Enter => {
            if state.session_picker_view == crate::app::SessionPickerView::Archive {
                state.session_picker_error = Some("Restore this session before resuming".into());
            } else {
                if let Some(summary) = selected_session_summary(state) {
                    state.pending_input_action = Some(
                        crate::tui::commands::InputAction::LoadSession(summary.storage_key),
                    );
                }
                state.session_picker_open = false;
            }
        }
        KeyCode::Char('P') | KeyCode::Char('p')
            if modifiers.contains(KeyModifiers::SHIFT)
                && state.session_picker_view == crate::app::SessionPickerView::Active =>
        {
            if let Some(summary) = selected_session_summary(state) {
                state.session_management_generation =
                    state.session_management_generation.wrapping_add(1);
                state.session_picker_busy = true;
                state.session_picker_error = None;
                state.pending_input_action =
                    Some(crate::tui::commands::InputAction::SetSessionPinned {
                        storage_key: summary.storage_key,
                        expected_id: summary.id,
                        pinned: !summary.pinned,
                        generation: state.session_management_generation,
                    });
            }
        }
        KeyCode::Char('A') | KeyCode::Char('a')
            if modifiers.contains(KeyModifiers::SHIFT)
                && state.session_picker_view == crate::app::SessionPickerView::Active =>
        {
            if let Some(summary) = selected_session_summary(state) {
                if state.current_session.as_ref().map(|session| session.id) == Some(summary.id) {
                    state.session_picker_error =
                        Some("The current session cannot be archived".into());
                } else {
                    state.session_picker_error = None;
                    state.session_archive_confirmation =
                        Some(crate::app::SessionArchiveConfirmation {
                            storage_key: summary.storage_key,
                            session_id: summary.id,
                            title: summary.title,
                            archive_selected: false,
                            busy: false,
                            error: None,
                        });
                }
            }
        }
        KeyCode::Char('U') | KeyCode::Char('u')
            if modifiers.contains(KeyModifiers::SHIFT)
                && state.session_picker_view == crate::app::SessionPickerView::Archive =>
        {
            if let Some(summary) = selected_session_summary(state) {
                state.session_management_generation =
                    state.session_management_generation.wrapping_add(1);
                state.session_picker_busy = true;
                state.session_picker_error = None;
                state.pending_input_action =
                    Some(crate::tui::commands::InputAction::TransferSession {
                        storage_key: summary.storage_key,
                        expected_id: summary.id,
                        direction: crate::app::SessionTransferDirection::Restore,
                        generation: state.session_management_generation,
                    });
            }
        }
        KeyCode::Char('D') | KeyCode::Char('d') if modifiers.contains(KeyModifiers::SHIFT) => {
            if state.session_picker_view == crate::app::SessionPickerView::Archive {
                return true;
            }
            if let Some(summary) = selected_session_summary(state) {
                if state.current_session.as_ref().map(|session| session.id) == Some(summary.id) {
                    state.session_picker_error =
                        Some("The current session cannot be deleted".into());
                } else {
                    state.session_picker_error = None;
                    state.session_delete_confirmation =
                        Some(crate::app::SessionDeleteConfirmation {
                            storage_key: summary.storage_key.clone(),
                            session_id: summary.id,
                            title: summary.title.clone(),
                            delete_selected: false,
                            busy: false,
                            error: None,
                        });
                }
            }
        }
        KeyCode::Char('R') | KeyCode::Char('r')
            if modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::SHIFT) =>
        {
            if let Some(summary) = selected_session_summary(state) {
                let title = if summary.title.trim().is_empty() {
                    "Untitled session".to_string()
                } else {
                    crate::terminal_text::sanitize_terminal_text(&summary.title)
                };
                state.session_rename = Some(crate::app::SessionRenameState {
                    storage_key: summary.storage_key,
                    session_id: summary.id,
                    cursor: title.len(),
                    title,
                    archived: state.session_picker_view == crate::app::SessionPickerView::Archive,
                    busy: false,
                    error: None,
                });
                state.session_picker_error = None;
            }
        }
        KeyCode::Char('G') | KeyCode::Char('g') if modifiers.contains(KeyModifiers::CONTROL) => {
            if state.session_title_running {
                state.session_picker_error =
                    Some("Another session name is still being generated".into());
            } else if let Some(summary) = selected_session_summary(state) {
                state.session_title_generation = state.session_title_generation.wrapping_add(1);
                state.session_picker_busy = true;
                state.session_picker_error = None;
                state.pending_input_action =
                    Some(crate::tui::commands::InputAction::GenerateSessionTitle {
                        storage_key: summary.storage_key,
                        expected_id: summary.id,
                        archived: state.session_picker_view
                            == crate::app::SessionPickerView::Archive,
                        generation: state.session_title_generation,
                    });
            }
        }
        KeyCode::Up => {
            state.selected_session_idx = crate::tui::selection::previous_wrap(
                state.selected_session_idx,
                visible_session_count(state),
            );
        }
        KeyCode::Down => {
            state.selected_session_idx = crate::tui::selection::next_wrap(
                state.selected_session_idx,
                visible_session_count(state),
            );
        }
        KeyCode::Home => state.selected_session_idx = 0,
        KeyCode::End => state.selected_session_idx = visible_session_count(state).saturating_sub(1),
        KeyCode::PageUp => {
            state.selected_session_idx =
                crate::tui::selection::page_up(state.selected_session_idx, 8)
        }
        KeyCode::PageDown => {
            state.selected_session_idx = crate::tui::selection::page_down(
                state.selected_session_idx,
                visible_session_count(state),
                8,
            )
        }
        KeyCode::Char(c) => {
            state.session_filter.push(c);
            state.selected_session_idx = 0;
            state.session_picker_error = None;
        }
        KeyCode::Backspace => {
            state.session_filter.pop();
            state.selected_session_idx = 0;
            state.session_picker_error = None;
        }
        _ => {}
    }
    true
}

fn handle_session_rename(state: &mut crate::app::AppState, code: KeyCode) -> bool {
    let Some(rename) = state.session_rename.as_mut() else {
        return true;
    };
    if rename.busy {
        return true;
    }
    match code {
        KeyCode::Esc => state.session_rename = None,
        KeyCode::Enter => {
            let title = rename
                .title
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            if title.is_empty() {
                rename.error = Some("Session name cannot be empty".into());
            } else {
                state.session_management_generation =
                    state.session_management_generation.wrapping_add(1);
                rename.busy = true;
                rename.error = None;
                state.session_picker_busy = true;
                state.pending_input_action =
                    Some(crate::tui::commands::InputAction::RenameSession {
                        storage_key: rename.storage_key.clone(),
                        expected_id: rename.session_id,
                        title,
                        archived: rename.archived,
                        generation: state.session_management_generation,
                    });
            }
        }
        KeyCode::Home => rename.cursor = 0,
        KeyCode::End => rename.cursor = rename.title.len(),
        KeyCode::Left => rename.cursor = previous_char_boundary(&rename.title, rename.cursor),
        KeyCode::Right => rename.cursor = next_char_boundary(&rename.title, rename.cursor),
        KeyCode::Backspace => {
            let previous = previous_char_boundary(&rename.title, rename.cursor);
            if previous < rename.cursor {
                rename.title.replace_range(previous..rename.cursor, "");
                rename.cursor = previous;
            }
            rename.error = None;
        }
        KeyCode::Delete => {
            let next = next_char_boundary(&rename.title, rename.cursor);
            if next > rename.cursor {
                rename.title.replace_range(rename.cursor..next, "");
            }
            rename.error = None;
        }
        KeyCode::Char(character) if !character.is_control() => {
            insert_session_rename_text(rename, &character.to_string());
        }
        _ => {}
    }
    true
}

fn insert_session_rename_text(rename: &mut crate::app::SessionRenameState, text: &str) {
    for character in text.chars().filter(|character| !character.is_control()) {
        let mut candidate = rename.title.clone();
        candidate.insert(rename.cursor, character);
        if unicode_width::UnicodeWidthStr::width(candidate.as_str()) > 100 {
            break;
        }
        rename.title = candidate;
        rename.cursor += character.len_utf8();
    }
    rename.error = None;
}

fn visible_session_count(state: &crate::app::AppState) -> usize {
    state.session_picker_visible_summaries().len()
}

fn selected_session_summary(
    state: &crate::app::AppState,
) -> Option<crate::session::SessionSummary> {
    state
        .session_picker_visible_summaries()
        .get(state.selected_session_idx)
        .map(|summary| (*summary).clone())
}

fn handle_session_archive_confirmation(state: &mut crate::app::AppState, code: KeyCode) -> bool {
    let Some(confirmation) = state.session_archive_confirmation.as_mut() else {
        return true;
    };
    if confirmation.busy {
        return true;
    }
    match code {
        KeyCode::Esc => state.session_archive_confirmation = None,
        KeyCode::Left | KeyCode::Right | KeyCode::Tab | KeyCode::BackTab => {
            confirmation.archive_selected = !confirmation.archive_selected;
            confirmation.error = None;
        }
        KeyCode::Char('c') | KeyCode::Char('C') => {
            state.session_archive_confirmation = None;
        }
        KeyCode::Enter if confirmation.archive_selected => {
            state.session_management_generation =
                state.session_management_generation.wrapping_add(1);
            confirmation.busy = true;
            confirmation.error = None;
            state.session_picker_busy = true;
            state.pending_input_action = Some(crate::tui::commands::InputAction::TransferSession {
                storage_key: confirmation.storage_key.clone(),
                expected_id: confirmation.session_id,
                direction: crate::app::SessionTransferDirection::Archive,
                generation: state.session_management_generation,
            });
        }
        KeyCode::Enter => state.session_archive_confirmation = None,
        _ => {}
    }
    true
}

fn handle_session_delete_confirmation(state: &mut crate::app::AppState, code: KeyCode) -> bool {
    let Some(confirmation) = state.session_delete_confirmation.as_mut() else {
        return true;
    };
    if confirmation.busy {
        return true;
    }
    match code {
        KeyCode::Esc => state.session_delete_confirmation = None,
        KeyCode::Left | KeyCode::Right | KeyCode::Tab | KeyCode::BackTab => {
            confirmation.delete_selected = !confirmation.delete_selected;
            confirmation.error = None;
        }
        KeyCode::Char('c') | KeyCode::Char('C') => {
            state.session_delete_confirmation = None;
        }
        KeyCode::Enter if confirmation.delete_selected => {
            confirmation.busy = true;
            confirmation.error = None;
            state.pending_input_action = Some(crate::tui::commands::InputAction::DeleteSession {
                storage_key: confirmation.storage_key.clone(),
                expected_id: confirmation.session_id,
            });
        }
        KeyCode::Enter => state.session_delete_confirmation = None,
        _ => {}
    }
    true
}

fn handle_timeline(state: &mut crate::app::AppState, code: KeyCode) -> bool {
    match code {
        KeyCode::Esc => state.timeline_open = false,
        KeyCode::Up => {
            state.selected_timeline_idx = state.selected_timeline_idx.saturating_sub(1);
        }
        KeyCode::Down => {
            if state.selected_timeline_idx + 1 < state.timeline_entries.len() {
                state.selected_timeline_idx += 1;
            }
        }
        KeyCode::Enter => {
            if let Some(entry) = state.timeline_entries.get(state.selected_timeline_idx) {
                state.pending_input_action = Some(crate::tui::commands::InputAction::BranchAt {
                    through_message_count: entry.through_message_count,
                    branch_turn: entry.turn_index,
                });
            }
            state.timeline_open = false;
        }
        _ => {}
    }
    true
}

fn handle_pager(state: &mut crate::app::AppState, code: KeyCode) -> bool {
    if state.plan_reader.is_some() {
        let maximum = state.plan_reader_max_scroll.get();
        state.pager_scroll = state.pager_scroll.min(maximum);
        match code {
            KeyCode::Enter => {
                state.pending_input_action = Some(crate::tui::commands::InputAction::Command(
                    crate::tui::commands::CommandAction {
                        id: crate::tui::commands::CommandId::Plan,
                        args: "execute".into(),
                    },
                ));
            }
            KeyCode::Char('r' | 'R') => crate::plan::revise(state),
            KeyCode::Char('n' | 'N') => {
                if let Err(error) = crate::plan::comment_selection(state) {
                    state.status_message = Some(error);
                }
            }
            KeyCode::Char('c' | 'C') => copy_pager_content(state, copy_text_to_clipboard),
            KeyCode::Char('j') | KeyCode::Down => {
                state.pager_scroll = state.pager_scroll.saturating_add(1).min(maximum)
            }
            KeyCode::Char('k') | KeyCode::Up => {
                state.pager_scroll = state.pager_scroll.saturating_sub(1)
            }
            KeyCode::Char('g') | KeyCode::Home => state.pager_scroll = 0,
            KeyCode::Char('G') | KeyCode::End => state.pager_scroll = maximum,
            KeyCode::Char(' ' | 'f') | KeyCode::PageDown => {
                state.pager_scroll = state.pager_scroll.saturating_add(15).min(maximum)
            }
            KeyCode::Char('b') | KeyCode::PageUp => {
                state.pager_scroll = state.pager_scroll.saturating_sub(15)
            }
            KeyCode::Char('q') | KeyCode::Esc => {
                crate::plan::dismiss(state);
            }
            _ => return false,
        }
        return true;
    }
    match code {
        KeyCode::Enter if state.learning_review_pager_open => {
            state.pending_input_action =
                Some(crate::tui::commands::InputAction::OpenLearningReviewActions);
            true
        }
        KeyCode::Char('r') | KeyCode::Char('R') if state.learning_review_pager_open => {
            state.pending_input_action =
                Some(crate::tui::commands::InputAction::RefreshLearningReview);
            true
        }
        KeyCode::Char('c') | KeyCode::Char('C') => {
            copy_pager_content(state, copy_text_to_clipboard);
            true
        }
        KeyCode::Char('j') | KeyCode::Down => {
            state.pager_scroll += 1;
            true
        }
        KeyCode::Char('k') | KeyCode::Up => {
            state.pager_scroll = state.pager_scroll.saturating_sub(1);
            true
        }
        KeyCode::Char('g') => {
            state.pager_scroll = 0;
            true
        }
        KeyCode::Char('G') => {
            let total = state.pager_content.lines().count().saturating_sub(1);
            state.pager_scroll = total as u16;
            true
        }
        KeyCode::Char(' ') | KeyCode::Char('f') => {
            state.pager_scroll += 15;
            true
        }
        KeyCode::Char('b') => {
            state.pager_scroll = state.pager_scroll.saturating_sub(15);
            true
        }
        KeyCode::Char('q') | KeyCode::Esc => {
            state.pager_open = false;
            state.learning_review_pager_open = false;
            true
        }
        _ => false,
    }
}

fn copy_pager_content<F>(state: &mut crate::app::AppState, copy: F)
where
    F: FnOnce(&str) -> Result<(), ()>,
{
    let characters = state.pager_content.chars().count();
    if copy(&state.pager_content).is_ok() {
        state.pager_selection.clear_selection();
        state.status_message = None;
        state.show_toast(
            format!("Copied page · {characters} chars"),
            crate::app::ToastKind::Success,
        );
    } else {
        state.status_message = Some("Clipboard unavailable; page was not copied".into());
        state.show_toast("Clipboard unavailable", crate::app::ToastKind::Error);
    }
}

#[cfg(test)]
mod plan_reader_copy_tests {
    #[test]
    fn real_plan_comment_key_keeps_the_committed_selection_until_annotation() {
        use ratatui::{buffer::Buffer, layout::Rect, style::Style};
        let mut state = crate::app::AppState::new(crate::context::ShellHistory::in_memory());
        state
            .work_state
            .lock()
            .unwrap()
            .apply_model_mutation(crate::work::WorkMutation::Propose {
                expected_revision: 0,
                title: "Fixture plan".into(),
                body: Some("Unique annotation phrase".into()),
                tasks: vec!["Verify fixture".into()],
            })
            .unwrap();
        crate::plan::open_reader(&mut state).unwrap();
        let mut buffer = Buffer::empty(Rect::new(0, 0, 40, 1));
        buffer.set_string(0, 0, "Unique annotation phrase", Style::default());
        state
            .pager_selection
            .capture_selectable_frame(&buffer, Rect::new(0, 0, 40, 1));
        state.pager_selection.begin_selection(0, 0);
        assert!(state.pager_selection.finish_selection(23, 0).is_some());
        assert!(super::handle_event(
            &mut state,
            &crate::tui::event::Event::Key(
                crossterm::event::KeyCode::Char('n'),
                crossterm::event::KeyModifiers::NONE,
            ),
            "/fixture"
        ));
        assert!(state.selected_plan_feedback.is_some());
        assert_eq!(state.input, "/plan comment selected ");
        assert!(!state.pager_open);
    }
    #[test]
    fn plan_reader_copy_keeps_the_entire_markdown_document() {
        let mut state = crate::app::AppState::new(crate::context::ShellHistory::in_memory());
        state
            .work_state
            .lock()
            .unwrap()
            .apply_model_mutation(crate::work::WorkMutation::Propose {
                expected_revision: 0,
                title: "Unicode plan".into(),
                body: Some("## Details\n\ncafé 界面 👩🏽‍💻\n\n- test\n\nFollowing paragraph".into()),
                tasks: vec!["Last task".into()],
            })
            .unwrap();
        crate::plan::open_reader(&mut state).unwrap();
        state.pager_scroll = 20;
        let expected = state.pager_content.clone();
        let mut copied = String::new();
        super::copy_pager_content(&mut state, |content| {
            copied = content.to_string();
            Ok(())
        });
        assert_eq!(copied, expected);
        assert!(copied.contains("café 界面 👩🏽‍💻"));
        assert!(copied.contains("Last task"));
        assert!(!copied.contains("Grants:"));
    }
}

fn handle_project_init(
    state: &mut crate::app::AppState,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> bool {
    let Some(init) = state.project_init.as_mut() else {
        return true;
    };
    match init.phase {
        crate::app::ProjectInitPhase::Preparing => match (modifiers, code) {
            (KeyModifiers::NONE, KeyCode::Esc) | (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                state.pending_input_action =
                    Some(crate::tui::commands::InputAction::CancelProjectInit);
            }
            _ => {}
        },
        crate::app::ProjectInitPhase::Writing => {}
        crate::app::ProjectInitPhase::Preview => match (modifiers, code) {
            (KeyModifiers::NONE, KeyCode::Esc | KeyCode::Char('c' | 'C')) => {
                state.pending_input_action =
                    Some(crate::tui::commands::InputAction::CancelProjectInit);
            }
            (KeyModifiers::NONE, KeyCode::Left | KeyCode::Right | KeyCode::Tab)
            | (KeyModifiers::SHIFT, KeyCode::BackTab) => {
                init.create_selected = !init.create_selected;
                init.error = None;
            }
            (KeyModifiers::NONE, KeyCode::Up | KeyCode::Char('k')) => {
                init.scroll = init.scroll.saturating_sub(1);
            }
            (KeyModifiers::NONE, KeyCode::Down | KeyCode::Char('j')) => {
                init.scroll = init.scroll.saturating_add(1);
            }
            (KeyModifiers::NONE, KeyCode::PageUp) => {
                init.scroll = init.scroll.saturating_sub(15);
            }
            (KeyModifiers::NONE, KeyCode::PageDown | KeyCode::Char(' ')) => {
                init.scroll = init.scroll.saturating_add(15);
            }
            (KeyModifiers::NONE, KeyCode::Home | KeyCode::Char('g')) => init.scroll = 0,
            (KeyModifiers::NONE, KeyCode::Enter) if init.create_selected => {
                if let Some(draft) = init.draft.clone() {
                    state.pending_input_action =
                        Some(crate::tui::commands::InputAction::CreateProjectInstructions(draft));
                }
            }
            (KeyModifiers::NONE, KeyCode::Enter) => {
                state.pending_input_action =
                    Some(crate::tui::commands::InputAction::CancelProjectInit);
            }
            _ => {}
        },
    }
    true
}

fn handle_trace_delete_confirmation(state: &mut crate::app::AppState, code: KeyCode) -> bool {
    let Some(confirmation) = state.trace_delete_confirmation.as_mut() else {
        return true;
    };
    match code {
        KeyCode::Esc | KeyCode::Char('c' | 'C') => state.trace_delete_confirmation = None,
        KeyCode::Left | KeyCode::Right | KeyCode::Tab | KeyCode::BackTab => {
            confirmation.delete_selected = !confirmation.delete_selected;
            confirmation.error = None;
        }
        KeyCode::Enter if confirmation.delete_selected => {
            state.pending_input_action = Some(crate::tui::commands::InputAction::DeleteTrace(
                confirmation.trace_id,
            ));
        }
        KeyCode::Enter => state.trace_delete_confirmation = None,
        _ => {}
    }
    true
}

fn handle_vim_normal(state: &mut crate::app::AppState, code: KeyCode) -> bool {
    use crate::app::VimMode;
    match code {
        KeyCode::Char('i') => {
            state.vim_state = VimMode::Insert;
            state.status_message = Some("-- INSERT --".into());
            true
        }
        KeyCode::Char('a') => {
            state.vim_state = VimMode::Insert;
            state.input_cursor = state
                .composer_ui
                .next_boundary(&state.input, state.input_cursor);
            state.composer_ui.preferred_column = None;
            state.status_message = Some("-- INSERT --".into());
            true
        }
        KeyCode::Char('h') => {
            if state.input_cursor > 0 {
                state.input_cursor = state
                    .composer_ui
                    .previous_boundary(&state.input, state.input_cursor);
            }
            state.composer_ui.preferred_column = None;
            true
        }
        KeyCode::Char('l') => {
            if state.input_cursor < state.input.len() {
                state.input_cursor = state
                    .composer_ui
                    .next_boundary(&state.input, state.input_cursor);
            }
            state.composer_ui.preferred_column = None;
            true
        }
        KeyCode::Char('j') => composer_move_vertical(state, 1),
        KeyCode::Char('k') => composer_move_vertical(state, -1),
        KeyCode::Char('0') => {
            state.input_cursor =
                crate::tui::composer::hard_line_bounds(&state.input, state.input_cursor).start;
            state.composer_ui.preferred_column = None;
            true
        }
        KeyCode::Char('$') => {
            state.input_cursor =
                crate::tui::composer::hard_line_bounds(&state.input, state.input_cursor).end;
            state.composer_ui.preferred_column = None;
            true
        }
        KeyCode::Char('x') => {
            delete_current_char(state);
            true
        }
        KeyCode::Char('d') => {
            state.input.clear();
            state.input_cursor = 0;
            state.composer_ui.clear_for_new_draft();
            true
        }
        KeyCode::Char('w') => {
            state.input_cursor = next_word_boundary(&state.input, state.input_cursor);
            true
        }
        KeyCode::Char('b') => {
            state.input_cursor = previous_word_boundary(&state.input, state.input_cursor);
            true
        }
        KeyCode::Char('p') => {
            if !state.yank_buffer.is_empty() {
                state.input_cursor =
                    crate::tui::composer::normalize_boundary(&state.input, state.input_cursor);
                state.composer_ui.apply_edit(
                    state.input_cursor..state.input_cursor,
                    state.yank_buffer.len(),
                );
                state
                    .input
                    .insert_str(state.input_cursor, &state.yank_buffer.clone());
                state.input_cursor += state.yank_buffer.len();
            }
            true
        }
        KeyCode::Char('u') => {
            // Simple undo: revert last insert_char if we tracked it
            // For now, restore from history if available
            if let Some(prev) = state.input_history.last() {
                state.input = prev.clone();
                state.input_cursor = state.input.len();
                state.composer_ui.clear_for_new_draft();
                state.composer_attachments.clear();
                state.pasted_images.clear();
            }
            true
        }
        KeyCode::Char('y') => {
            state.yank_buffer = state.input.clone();
            state.status_message = Some("Yanked".into());
            true
        }
        _ => false,
    }
}

fn update_ghost(state: &mut crate::app::AppState) {
    if state.input.len() < 2 {
        state.ghost_text = None;
        return;
    }
    for entry in state.input_history.iter().rev() {
        if entry.starts_with(&state.input) && entry != &state.input {
            state.ghost_text = Some(entry[state.input.len()..].to_string());
            return;
        }
    }
    state.ghost_text = None;
}

fn complete_path(input: &str, cursor: usize, cwd: &str) -> Option<(String, Vec<String>)> {
    // Extract the token before cursor
    let before = &input[..cursor];
    let token_start = before
        .rfind(char::is_whitespace)
        .map(|i| i + 1)
        .unwrap_or(0);
    let token = &before[token_start..];

    // Only complete if this looks like a path
    if !token.contains('/') && !token.starts_with('.') && !token.starts_with('~') {
        return None;
    }

    let resolved_prefix = crate::agent::tools::resolve_path(token, cwd);

    let glob_pattern = format!("{}*", resolved_prefix);

    let mut matches: Vec<String> = glob::glob(&glob_pattern)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|p| {
            !p.file_name()
                .map(|n| n.to_string_lossy().starts_with('.'))
                .unwrap_or(false)
        })
        .map(|p| {
            let p_str = p.to_string_lossy().to_string();
            // Add trailing slash for directories
            if p.is_dir() {
                format!("{}/", p_str)
            } else {
                p_str
            }
        })
        .filter(|p| p.starts_with(&resolved_prefix))
        .collect();

    if matches.is_empty() {
        return None;
    }

    matches.sort();

    // Find common prefix
    let common = if matches.len() == 1 {
        matches[0].clone()
    } else {
        matches.iter().skip(1).fold(matches[0].clone(), |acc, m| {
            acc.chars()
                .zip(m.chars())
                .take_while(|(a, b)| a == b)
                .map(|(a, _)| a)
                .collect()
        })
    };

    // Calculate the suffix relative to original token (keep input as-is, just add missing part)
    let display_matches: Vec<String> = matches
        .iter()
        .map(|m| {
            let p = std::path::Path::new(m);
            p.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| m.clone())
        })
        .collect();

    // Get suffix relative to what user already typed (resolved path)
    let suffix = if common.starts_with(&resolved_prefix) {
        common[resolved_prefix.len()..].to_string()
    } else {
        String::new()
    };

    Some((suffix, display_matches))
}

fn handle_file_picker(
    state: &mut crate::app::AppState,
    code: KeyCode,
    modifiers: KeyModifiers,
    _cwd: &str,
) -> bool {
    if handle_inline_overlay_horizontal_motion(state, code, modifiers) {
        return true;
    }
    match code {
        KeyCode::Esc => {
            state.composer_ui.dismissed_completion_token =
                active_composer_token(&state.input, state.input_cursor)
                    .map(|(_, token)| token.to_string());
            state.file_picker_open = false;
            state.file_picker_generation = state.file_picker_generation.wrapping_add(1);
            state.file_picker_phase = crate::attachments::PickerPhase::Idle;
        }
        KeyCode::Enter | KeyCode::Tab => {
            let Some(entry) = state
                .file_picker_entries
                .get(state.selected_file_idx)
                .cloned()
            else {
                return true;
            };
            let suffix = match crate::attachments::range_suffix(&state.file_picker_filter) {
                Ok(suffix) => suffix,
                Err(error) => {
                    state.file_picker_error = Some(error);
                    return true;
                }
            };
            if matches!(entry, crate::attachments::PickerEntry::Mcp(_)) && !suffix.is_empty() {
                state.file_picker_error = Some("line ranges apply only to workspace files".into());
                return true;
            }
            state
                .composer_attachments
                .retain(|attachment| attachment.is_referenced_by(&state.input));
            let attachment = crate::attachments::ComposerAttachment::from_picker(&entry);
            if state.composer_attachments.len() >= crate::attachments::MAX_COMPOSER_ATTACHMENTS {
                state.file_picker_error = Some("composer attachment limit reached".into());
                return true;
            }
            let insert_str = format!("@{}{} ", attachment.reference, suffix);
            let Some(inserted) = replace_active_composer_token(state, &insert_str) else {
                state.file_picker_error = Some("attachment token is no longer active".into());
                return true;
            };
            let chip_range = inserted.start..inserted.end.saturating_sub(1);
            state
                .composer_ui
                .register_atomic(crate::tui::composer::AtomicElement {
                    range: chip_range,
                    kind: if matches!(entry, crate::attachments::PickerEntry::File(_)) {
                        crate::tui::composer::AtomicKind::File
                    } else {
                        crate::tui::composer::AtomicKind::Reference
                    },
                });
            if let crate::attachments::PickerEntry::File(file) = &entry {
                state.pending_input_action =
                    Some(crate::tui::commands::InputAction::RecordAttachmentFrecency(
                        file.frecency_key(),
                    ));
            }
            state.composer_attachments.push(attachment);
            state.file_picker_open = false;
            state.file_picker_generation = state.file_picker_generation.wrapping_add(1);
            state.file_picker_phase = crate::attachments::PickerPhase::Idle;
        }
        KeyCode::Up => {
            state.selected_file_idx = crate::tui::selection::previous_wrap(
                state.selected_file_idx,
                state.file_picker_entries.len(),
            );
        }
        KeyCode::Down => {
            state.selected_file_idx = crate::tui::selection::next_wrap(
                state.selected_file_idx,
                state.file_picker_entries.len(),
            );
        }
        KeyCode::Char(c) => {
            if state.file_picker_filter.len() + c.len_utf8()
                > crate::attachments::MAX_PICKER_QUERY_BYTES
            {
                state.file_picker_error = Some("attachment query limit reached".into());
                return true;
            }
            insert_char(state, c);
            sync_file_picker_filter(state);
            state.selected_file_idx = 0;
            state.file_picker_generation = state.file_picker_generation.wrapping_add(1);
            state.file_picker_phase = crate::attachments::PickerPhase::Matching;
            state.pending_input_action = Some(crate::tui::commands::InputAction::RefreshFilePicker);
        }
        KeyCode::Backspace => {
            delete_previous_char(state);
            sync_file_picker_filter(state);
            if !active_composer_token(&state.input, state.input_cursor)
                .is_some_and(|(_, token)| token.starts_with('@'))
            {
                state.file_picker_open = false;
            }
            state.selected_file_idx = 0;
            state.file_picker_generation = state.file_picker_generation.wrapping_add(1);
            state.file_picker_phase = crate::attachments::PickerPhase::Matching;
            state.pending_input_action = Some(crate::tui::commands::InputAction::RefreshFilePicker);
        }
        KeyCode::Home => state.selected_file_idx = 0,
        KeyCode::End => state.selected_file_idx = state.file_picker_entries.len().saturating_sub(1),
        KeyCode::PageUp => {
            state.selected_file_idx = crate::tui::selection::page_up(state.selected_file_idx, 8)
        }
        KeyCode::PageDown => {
            state.selected_file_idx = crate::tui::selection::page_down(
                state.selected_file_idx,
                state.file_picker_entries.len(),
                8,
            )
        }
        _ => {}
    }
    true
}

fn handle_inline_overlay_horizontal_motion(
    state: &mut crate::app::AppState,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> bool {
    match (modifiers, code) {
        (KeyModifiers::ALT, KeyCode::Left) => move_to_previous_word(state),
        (KeyModifiers::ALT, KeyCode::Right) => move_to_next_word_end(state),
        (command, KeyCode::Left) if is_command_modifier(command) => move_to_hard_line_start(state),
        (command, KeyCode::Right) if is_command_modifier(command) => move_to_hard_line_end(state),
        (KeyModifiers::NONE, KeyCode::Left) => {
            if state.input_cursor > 0 {
                state.input_cursor = state
                    .composer_ui
                    .previous_boundary(&state.input, state.input_cursor);
            }
            state.composer_ui.preferred_column = None;
        }
        (KeyModifiers::NONE, KeyCode::Right) => {
            if state.input_cursor < state.input.len() {
                state.input_cursor = state
                    .composer_ui
                    .next_boundary(&state.input, state.input_cursor);
            }
            state.composer_ui.preferred_column = None;
        }
        _ => return false,
    }
    true
}

fn sync_file_picker_filter(state: &mut crate::app::AppState) {
    state.file_picker_filter = active_composer_token(&state.input, state.input_cursor)
        .and_then(|(_, token)| token.strip_prefix('@'))
        .unwrap_or_default()
        .to_string();
}

#[cfg(test)]
mod session_picker_tests {
    use super::*;
    use crate::context::ShellHistory;
    use crate::session::{SessionSummary, SessionV2};
    use chrono::{TimeZone, Utc};
    use uuid::Uuid;

    fn summary(key: &str, id: Uuid, title: &str) -> SessionSummary {
        SessionSummary {
            storage_key: key.into(),
            id,
            title: title.into(),
            pinned: false,
            workspace: "/workspace/ca".into(),
            updated_at: Utc.timestamp_opt(1, 0).unwrap(),
        }
    }

    fn picker_state() -> crate::app::AppState {
        let mut state = crate::app::AppState::new(ShellHistory::in_memory());
        state.session_picker_open = true;
        state
    }

    #[test]
    fn current_session_is_protected_from_delete_confirmation() {
        let mut state = picker_state();
        let id = Uuid::new_v4();
        state.session_list = vec![summary("current", id, "Current")];
        let mut current = SessionV2::new("Current", "/workspace/ca", "p", "m", Vec::new());
        current.id = id;
        state.current_session = Some(current);

        handle_event(
            &mut state,
            &Event::Key(KeyCode::Char('D'), KeyModifiers::SHIFT),
            "/workspace/ca",
        );

        assert!(state.session_delete_confirmation.is_none());
        assert!(state.session_picker_error.is_none());
    }

    #[test]
    fn delete_confirmation_defaults_to_cancel_and_locks_while_busy() {
        let mut state = picker_state();
        let id = Uuid::new_v4();
        state.session_list = vec![summary("saved", id, "Saved")];

        handle_event(
            &mut state,
            &Event::Key(KeyCode::Char('D'), KeyModifiers::SHIFT),
            "/workspace/ca",
        );
        assert!(
            !state
                .session_delete_confirmation
                .as_ref()
                .unwrap()
                .delete_selected
        );
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Right, KeyModifiers::NONE),
            "/workspace/ca",
        );
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
            "/workspace/ca",
        );
        assert!(state.session_delete_confirmation.as_ref().unwrap().busy);
        assert_eq!(
            state.pending_input_action,
            Some(crate::tui::commands::InputAction::DeleteSession {
                storage_key: "saved".into(),
                expected_id: id,
            })
        );
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Esc, KeyModifiers::NONE),
            "/workspace/ca",
        );
        assert!(state.session_delete_confirmation.is_some());
    }

    #[test]
    fn filtering_maps_delete_to_visible_row_and_paste_stays_in_picker() {
        let mut state = picker_state();
        state.input = "composer".into();
        let hidden = Uuid::new_v4();
        let visible = Uuid::new_v4();
        state.session_list = vec![
            summary("hidden", hidden, "Alpha"),
            summary("visible", visible, "Keep me"),
        ];

        handle_event(&mut state, &Event::Paste("Keep".into()), "/workspace/ca");
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Char('D'), KeyModifiers::SHIFT),
            "/workspace/ca",
        );

        assert_eq!(state.input, "composer");
        assert_eq!(state.session_filter, "Keep");
        assert_eq!(
            state
                .session_delete_confirmation
                .as_ref()
                .unwrap()
                .session_id,
            visible
        );
    }

    #[test]
    fn tab_switches_active_and_archive_views_and_archive_enter_never_resumes() {
        let mut state = picker_state();
        let archived = Uuid::new_v4();
        state.session_archive_list = vec![summary("archived", archived, "Archived")];

        handle_event(
            &mut state,
            &Event::Key(KeyCode::Tab, KeyModifiers::NONE),
            "/workspace/ca",
        );
        assert_eq!(
            state.session_picker_view,
            crate::app::SessionPickerView::Archive
        );

        handle_event(
            &mut state,
            &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
            "/workspace/ca",
        );
        assert!(state.pending_input_action.is_none());
        assert!(state.session_picker_open);
        assert_eq!(
            state.session_picker_error.as_deref(),
            Some("Restore this session before resuming")
        );
    }

    #[test]
    fn current_session_is_excluded_from_pinning() {
        let mut state = picker_state();
        let id = Uuid::new_v4();
        state.session_list = vec![summary("current", id, "Current")];
        let mut current = SessionV2::new("Current", "/workspace/ca", "p", "m", Vec::new());
        current.id = id;
        state.current_session = Some(current);

        handle_event(
            &mut state,
            &Event::Key(KeyCode::Char('P'), KeyModifiers::SHIFT),
            "/workspace/ca",
        );

        assert!(!state.session_picker_busy);
        assert!(state.pending_input_action.is_none());
    }

    #[test]
    fn current_session_is_protected_from_archive_confirmation() {
        let mut state = picker_state();
        let id = Uuid::new_v4();
        state.session_list = vec![summary("current", id, "Current")];
        let mut current = SessionV2::new("Current", "/workspace/ca", "p", "m", Vec::new());
        current.id = id;
        state.current_session = Some(current);

        handle_event(
            &mut state,
            &Event::Key(KeyCode::Char('A'), KeyModifiers::SHIFT),
            "/workspace/ca",
        );

        assert!(state.session_archive_confirmation.is_none());
        assert!(state.session_picker_error.is_none());
    }

    #[test]
    fn current_session_is_excluded_before_filtering_and_selection() {
        let mut state = picker_state();
        let current_id = Uuid::new_v4();
        let former_id = Uuid::new_v4();
        state.session_list = vec![
            summary("current", current_id, "Current matching"),
            summary("former", former_id, "Former matching"),
        ];
        state.session_filter = "matching".into();
        let mut current = SessionV2::new("Current", "/workspace/ca", "p", "m", Vec::new());
        current.id = current_id;
        state.current_session = Some(current);

        handle_event(
            &mut state,
            &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
            "/workspace/ca",
        );

        assert_eq!(
            state.pending_input_action,
            Some(crate::tui::commands::InputAction::LoadSession(
                "former".into()
            ))
        );
    }

    #[test]
    fn active_actions_and_navigation_map_around_the_hidden_current_session() {
        let current_id = Uuid::new_v4();
        let former_one = Uuid::new_v4();
        let former_two = Uuid::new_v4();
        let build_state = || {
            let mut state = picker_state();
            state.session_list = vec![
                summary("current", current_id, "Current"),
                summary("former-one", former_one, "Former one"),
                summary("former-two", former_two, "Former two"),
            ];
            let mut current = SessionV2::new("Current", "/workspace/ca", "p", "m", Vec::new());
            current.id = current_id;
            state.current_session = Some(current);
            state
        };

        let mut pin = build_state();
        handle_event(
            &mut pin,
            &Event::Key(KeyCode::Char('P'), KeyModifiers::SHIFT),
            "/workspace/ca",
        );
        assert!(matches!(
            pin.pending_input_action,
            Some(crate::tui::commands::InputAction::SetSessionPinned {
                expected_id,
                ..
            }) if expected_id == former_one
        ));

        let mut archive = build_state();
        handle_event(
            &mut archive,
            &Event::Key(KeyCode::Char('A'), KeyModifiers::SHIFT),
            "/workspace/ca",
        );
        assert_eq!(
            archive
                .session_archive_confirmation
                .as_ref()
                .map(|confirmation| confirmation.session_id),
            Some(former_one)
        );

        let mut delete = build_state();
        handle_event(
            &mut delete,
            &Event::Key(KeyCode::Char('D'), KeyModifiers::SHIFT),
            "/workspace/ca",
        );
        assert_eq!(
            delete
                .session_delete_confirmation
                .as_ref()
                .map(|confirmation| confirmation.session_id),
            Some(former_one)
        );

        let mut navigate = build_state();
        handle_event(
            &mut navigate,
            &Event::Key(KeyCode::Up, KeyModifiers::NONE),
            "/workspace/ca",
        );
        handle_event(
            &mut navigate,
            &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
            "/workspace/ca",
        );
        assert_eq!(
            navigate.pending_input_action,
            Some(crate::tui::commands::InputAction::LoadSession(
                "former-two".into()
            ))
        );
    }

    #[test]
    fn archive_confirmation_defaults_to_cancel_and_locks_while_busy() {
        let mut state = picker_state();
        let id = Uuid::new_v4();
        state.session_list = vec![summary("saved", id, "Saved")];

        handle_event(
            &mut state,
            &Event::Key(KeyCode::Char('A'), KeyModifiers::SHIFT),
            "/workspace/ca",
        );
        assert!(
            !state
                .session_archive_confirmation
                .as_ref()
                .unwrap()
                .archive_selected
        );
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Right, KeyModifiers::NONE),
            "/workspace/ca",
        );
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
            "/workspace/ca",
        );
        assert!(state.session_archive_confirmation.as_ref().unwrap().busy);
        assert_eq!(
            state.pending_input_action,
            Some(crate::tui::commands::InputAction::TransferSession {
                storage_key: "saved".into(),
                expected_id: id,
                direction: crate::app::SessionTransferDirection::Archive,
                generation: 1,
            })
        );
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Esc, KeyModifiers::NONE),
            "/workspace/ca",
        );
        assert!(state.session_archive_confirmation.is_some());
    }

    #[test]
    fn restore_maps_the_filtered_archived_row_and_locks_the_picker() {
        let mut state = picker_state();
        state.session_picker_view = crate::app::SessionPickerView::Archive;
        state.session_filter = "keep".into();
        let hidden = Uuid::new_v4();
        let visible = Uuid::new_v4();
        state.session_archive_list = vec![
            summary("hidden", hidden, "Other"),
            summary("visible", visible, "Keep archived"),
        ];

        handle_event(
            &mut state,
            &Event::Key(KeyCode::Char('U'), KeyModifiers::SHIFT),
            "/workspace/ca",
        );

        assert!(state.session_picker_busy);
        assert_eq!(
            state.pending_input_action,
            Some(crate::tui::commands::InputAction::TransferSession {
                storage_key: "visible".into(),
                expected_id: visible,
                direction: crate::app::SessionTransferDirection::Restore,
                generation: 1,
            })
        );
    }

    #[test]
    fn rename_edits_and_submits_the_highlighted_projected_session() {
        let mut state = picker_state();
        state.session_filter = "keep".into();
        let hidden = Uuid::new_v4();
        let visible = Uuid::new_v4();
        state.session_list = vec![
            summary("hidden", hidden, "Other"),
            summary("visible", visible, "Keep this"),
        ];

        handle_event(
            &mut state,
            &Event::Key(KeyCode::Char('r'), KeyModifiers::CONTROL),
            "/workspace/ca",
        );
        assert_eq!(
            state
                .session_rename
                .as_ref()
                .map(|rename| rename.title.as_str()),
            Some("Keep this")
        );
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Char('!'), KeyModifiers::NONE),
            "/workspace/ca",
        );
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
            "/workspace/ca",
        );

        assert_eq!(
            state.pending_input_action,
            Some(crate::tui::commands::InputAction::RenameSession {
                storage_key: "visible".into(),
                expected_id: visible,
                title: "Keep this!".into(),
                archived: false,
                generation: 1,
            })
        );
        assert!(state.session_rename.as_ref().unwrap().busy);
    }

    #[test]
    fn generate_name_targets_the_highlighted_projected_session() {
        let mut state = picker_state();
        state.session_picker_view = crate::app::SessionPickerView::Archive;
        state.session_filter = "keep".into();
        let hidden = Uuid::new_v4();
        let visible = Uuid::new_v4();
        state.session_archive_list = vec![
            summary("hidden", hidden, "Other"),
            summary("visible", visible, "/workspace/keep prompt…"),
        ];

        handle_event(
            &mut state,
            &Event::Key(KeyCode::Char('g'), KeyModifiers::CONTROL),
            "/workspace/ca",
        );

        assert!(state.session_picker_busy);
        assert_eq!(state.session_title_generation, 1);
        assert_eq!(
            state.pending_input_action,
            Some(crate::tui::commands::InputAction::GenerateSessionTitle {
                storage_key: "visible".into(),
                expected_id: visible,
                archived: true,
                generation: 1,
            })
        );
    }

    #[test]
    fn rename_rejects_an_empty_name_and_escape_cancels() {
        let mut state = picker_state();
        let id = Uuid::new_v4();
        state.session_list = vec![summary("saved", id, "A")];

        handle_event(
            &mut state,
            &Event::Key(KeyCode::Char('r'), KeyModifiers::CONTROL),
            "/workspace/ca",
        );
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Backspace, KeyModifiers::NONE),
            "/workspace/ca",
        );
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
            "/workspace/ca",
        );

        assert_eq!(
            state
                .session_rename
                .as_ref()
                .and_then(|rename| rename.error.as_deref()),
            Some("Session name cannot be empty")
        );
        assert!(state.pending_input_action.is_none());
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Esc, KeyModifiers::NONE),
            "/workspace/ca",
        );
        assert!(state.session_rename.is_none());
    }
}

#[cfg(test)]
mod session_portability_input_tests {
    use super::*;
    use crate::context::ShellHistory;
    use std::path::PathBuf;

    fn overwrite_state() -> crate::app::AppState {
        let mut state = crate::app::AppState::new(ShellHistory::in_memory());
        state.input = "composer stays".into();
        state.session_portability_generation = 7;
        state.session_portability = Some(crate::app::SessionPortabilityState {
            operation: crate::app::SessionPortabilityOperation::Export {
                format: crate::session_portability::ExportFormat::Json,
                overwrite: false,
            },
            path: PathBuf::from("/tmp/existing.json"),
            generation: 7,
            busy: false,
            awaiting_overwrite: true,
            overwrite_selected: false,
            import_selected: false,
            import_learning_eligible: false,
            import_preview: None,
            error: None,
        });
        state
    }

    #[test]
    fn overwrite_confirmation_is_cancel_first_and_paste_is_isolated() {
        let mut state = overwrite_state();
        handle_event(&mut state, &Event::Paste("hidden".into()), "/tmp");
        assert_eq!(state.input, "composer stays");
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
            "/tmp",
        );
        assert!(state.session_portability.is_none());
        assert!(state.session_portability_request.is_none());

        let mut state = overwrite_state();
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Right, KeyModifiers::NONE),
            "/tmp",
        );
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
            "/tmp",
        );
        let overlay = state.session_portability.as_ref().unwrap();
        assert!(overlay.busy);
        assert!(!overlay.awaiting_overwrite);
        assert_eq!(
            state.session_portability_request,
            Some(crate::app::SessionPortabilityRequest {
                operation: crate::app::SessionPortabilityOperation::Export {
                    format: crate::session_portability::ExportFormat::Json,
                    overwrite: true,
                },
                path: PathBuf::from("/tmp/existing.json"),
                generation: 7,
            })
        );
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Esc, KeyModifiers::NONE),
            "/tmp",
        );
        assert!(state.session_portability.is_some());
    }

    #[test]
    fn portability_failure_closes_without_changing_the_composer() {
        let mut state = overwrite_state();
        let overlay = state.session_portability.as_mut().unwrap();
        overlay.awaiting_overwrite = false;
        overlay.error = Some("invalid portable session".into());
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
            "/tmp",
        );
        assert!(state.session_portability.is_none());
        assert_eq!(state.input, "composer stays");
    }

    fn foreign_preview_state() -> crate::app::AppState {
        let mut state = crate::app::AppState::new(ShellHistory::in_memory());
        state.input = "composer stays".into();
        state.session_portability_generation = 9;
        state.session_portability = Some(crate::app::SessionPortabilityState {
            operation: crate::app::SessionPortabilityOperation::ForeignImport {
                source: crate::session::ForeignTranscriptSource::Claude,
                commit: false,
                learning_eligible: false,
            },
            path: PathBuf::from("/selected/claude"),
            generation: 9,
            busy: false,
            awaiting_overwrite: false,
            overwrite_selected: false,
            import_selected: false,
            import_learning_eligible: false,
            import_preview: Some(crate::foreign_import::ForeignImportPreview {
                source: crate::session::ForeignTranscriptSource::Claude,
                source_label: "claude".into(),
                source_files: 2,
                source_bytes: 2048,
                sessions: 2,
                messages: 7,
                skipped_items: 3,
                errors: vec!["one malformed record".into()],
                learning_eligible: false,
            }),
            error: None,
        });
        state
    }

    #[test]
    fn foreign_import_confirmation_is_cancel_first_and_learning_is_explicit() {
        let mut cancelled = foreign_preview_state();
        handle_event(
            &mut cancelled,
            &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
            "/tmp",
        );
        assert!(cancelled.session_portability.is_none());
        assert!(cancelled.session_portability_request.is_none());
        assert_eq!(cancelled.input, "composer stays");

        let mut confirmed = foreign_preview_state();
        handle_event(
            &mut confirmed,
            &Event::Key(KeyCode::Char('l'), KeyModifiers::NONE),
            "/tmp",
        );
        handle_event(
            &mut confirmed,
            &Event::Key(KeyCode::Right, KeyModifiers::NONE),
            "/tmp",
        );
        handle_event(
            &mut confirmed,
            &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
            "/tmp",
        );
        assert!(confirmed.session_portability.as_ref().unwrap().busy);
        assert_eq!(
            confirmed.session_portability_request,
            Some(crate::app::SessionPortabilityRequest {
                operation: crate::app::SessionPortabilityOperation::ForeignImport {
                    source: crate::session::ForeignTranscriptSource::Claude,
                    commit: true,
                    learning_eligible: true,
                },
                path: PathBuf::from("/selected/claude"),
                generation: 9,
            })
        );
        assert_eq!(confirmed.input, "composer stays");
    }
}

#[cfg(test)]
mod session_retention_input_tests {
    use super::*;
    use crate::context::ShellHistory;
    use chrono::{Duration, TimeZone, Utc};
    use uuid::Uuid;

    fn summary(title: &str, age_days: i64, pinned: bool) -> crate::session::SessionSummary {
        crate::session::SessionSummary {
            storage_key: title.to_ascii_lowercase(),
            id: Uuid::new_v4(),
            title: title.into(),
            pinned,
            workspace: "/workspace".into(),
            updated_at: Utc.timestamp_opt(2_000_000, 0).unwrap() - Duration::days(age_days),
        }
    }

    fn retention_state(
        settings: crate::session_retention::RetentionSettings,
    ) -> crate::app::AppState {
        let mut state = crate::app::AppState::new(ShellHistory::in_memory());
        state.session_retention_generation = 1;
        state.session_retention = Some(crate::app::SessionRetentionState {
            settings,
            mode: crate::app::SessionRetentionMode::Overview,
            field: crate::app::SessionRetentionField::Days,
            days_input: settings
                .days
                .map_or_else(String::new, |value| value.to_string()),
            count_input: settings
                .count
                .map_or_else(String::new, |value| value.to_string()),
            cursor: 0,
            preview: None,
            preview_selected_idx: 0,
            delete_selected: false,
            busy: None,
            writes_enabled: true,
            sessions_enabled: true,
            active_count: 0,
            archived_count: 0,
            generation: 1,
            notice: None,
            error: None,
        });
        state
    }

    #[test]
    fn preview_is_complete_protects_rows_and_defaults_to_cancel() {
        let settings = crate::session_retention::RetentionSettings {
            days: Some(30),
            count: None,
        };
        let mut state = retention_state(settings);
        let expired = summary("Expired", 60, false);
        let pinned = summary("Pinned", 60, true);
        let current_summary = summary("Current", 60, false);
        let mut current =
            crate::session::SessionV2::new("Current", "/workspace", "local", "coder", Vec::new());
        current.id = current_summary.id;
        state.current_session = Some(current);
        state.session_list = vec![expired.clone(), pinned, current_summary];

        handle_event(
            &mut state,
            &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
            "/workspace",
        );
        let retention = state.session_retention.as_ref().unwrap();
        assert_eq!(retention.mode, crate::app::SessionRetentionMode::Preview);
        assert!(!retention.delete_selected);
        let preview = retention.preview.as_ref().unwrap();
        assert_eq!(preview.candidates.len(), 1);
        assert_eq!(preview.candidates[0].id, expired.id);
        assert_eq!(preview.protected_current, 1);
        assert_eq!(preview.protected_pinned, 1);

        handle_event(
            &mut state,
            &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
            "/workspace",
        );
        assert_eq!(
            state.session_retention.as_ref().unwrap().mode,
            crate::app::SessionRetentionMode::Overview
        );
        assert!(state.session_retention_request.is_none());
    }

    #[test]
    fn confirmed_preview_queues_exact_generation_and_busy_locks_controls() {
        let mut state = retention_state(crate::session_retention::RetentionSettings {
            days: Some(30),
            count: None,
        });
        state.session_list = vec![summary("Expired", 60, false)];
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
            "/workspace",
        );
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Right, KeyModifiers::NONE),
            "/workspace",
        );
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
            "/workspace",
        );

        let retention = state.session_retention.as_ref().unwrap();
        assert_eq!(
            retention.busy,
            Some(crate::app::SessionRetentionBusy::Deleting)
        );
        assert!(matches!(
            state.session_retention_request,
            Some(crate::app::SessionRetentionRequest {
                operation: crate::app::SessionRetentionOperation::Delete { .. },
                ..
            })
        ));
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Esc, KeyModifiers::NONE),
            "/workspace",
        );
        assert!(state.session_retention.is_some());
    }

    #[test]
    fn editor_is_digit_only_paste_isolated_and_save_is_not_delete() {
        let mut state = retention_state(crate::session_retention::RetentionSettings::default());
        state.input = "composer".into();
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Char('E'), KeyModifiers::SHIFT),
            "/workspace",
        );
        handle_event(&mut state, &Event::Paste("30x".into()), "/workspace");
        assert_eq!(state.input, "composer");
        assert_eq!(state.session_retention.as_ref().unwrap().days_input, "30");
        assert_eq!(
            state.session_retention.as_ref().unwrap().error.as_deref(),
            Some("Use decimal digits only")
        );
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Tab, KeyModifiers::NONE),
            "/workspace",
        );
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Char('2'), KeyModifiers::NONE),
            "/workspace",
        );
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
            "/workspace",
        );

        assert!(matches!(
            state.session_retention_request,
            Some(crate::app::SessionRetentionRequest {
                operation: crate::app::SessionRetentionOperation::Save(
                    crate::session_retention::RetentionSettings {
                        days: Some(30),
                        count: Some(2),
                    }
                ),
                ..
            })
        ));
        assert!(state.session_retention.as_ref().unwrap().preview.is_none());
    }
}

#[cfg(test)]
mod context_limit_editor_tests {
    use super::*;
    use crate::context::ShellHistory;

    fn editor_state() -> crate::app::AppState {
        let mut state = crate::app::AppState::new(ShellHistory::in_memory());
        state.input = "composer stays".into();
        state.context_limit_editor = Some(crate::app::ContextLimitEditorState {
            provider: "mimo".into(),
            model: "mimo-v2.5-pro".into(),
            window_input: String::new(),
            threshold_input: String::new(),
            fallback_window: Some(32_000),
            fallback_threshold: None,
            editing_profile: false,
            field: crate::app::ContextLimitField::Window,
            cursor: 0,
            saving: false,
            writes_enabled: true,
            error: None,
        });
        state
    }

    #[test]
    fn paste_is_digits_only_and_never_reaches_composer() {
        let mut state = editor_state();

        handle_event(&mut state, &Event::Paste("12x34\n".into()), "/workspace/ca");

        let editor = state.context_limit_editor.unwrap();
        assert_eq!(editor.window_input, "1234");
        assert_eq!(editor.error.as_deref(), Some("Use decimal digits only"));
        assert_eq!(state.input, "composer stays");
    }

    #[test]
    fn fields_validate_and_emit_an_exact_update() {
        let mut state = editor_state();
        let editor = state.context_limit_editor.as_mut().unwrap();
        editor.window_input = "1000".into();
        editor.cursor = 4;
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Tab, KeyModifiers::NONE),
            "/workspace/ca",
        );
        for character in ['9', '0', '0'] {
            handle_event(
                &mut state,
                &Event::Key(KeyCode::Char(character), KeyModifiers::NONE),
                "/workspace/ca",
            );
        }
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
            "/workspace/ca",
        );

        assert_eq!(
            state.pending_input_action,
            Some(crate::tui::commands::InputAction::SaveContextLimits(
                crate::config::ContextLimitUpdate {
                    provider: "mimo".into(),
                    model: "mimo-v2.5-pro".into(),
                    context_window_tokens: Some(1000),
                    compact_at_tokens: Some(900),
                }
            ))
        );
        assert!(state.context_limit_editor.unwrap().saving);
    }

    #[test]
    fn invalid_threshold_stays_editable_without_a_write() {
        let mut state = editor_state();
        let editor = state.context_limit_editor.as_mut().unwrap();
        editor.window_input = "1000".into();
        editor.threshold_input = "1000".into();
        editor.field = crate::app::ContextLimitField::Threshold;
        editor.cursor = 4;

        handle_event(
            &mut state,
            &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
            "/workspace/ca",
        );

        assert!(state.pending_input_action.is_none());
        let editor = state.context_limit_editor.unwrap();
        assert!(!editor.saving);
        assert!(editor.error.unwrap().contains("below"));
    }

    #[test]
    fn bare_and_busy_states_lock_persistence_and_cancellation() {
        let mut bare = editor_state();
        bare.context_limit_editor.as_mut().unwrap().writes_enabled = false;
        handle_event(
            &mut bare,
            &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
            "/workspace/ca",
        );
        assert!(bare.pending_input_action.is_none());
        assert!(bare
            .context_limit_editor
            .as_ref()
            .unwrap()
            .error
            .as_deref()
            .unwrap()
            .contains("View-only"));

        let mut busy = editor_state();
        busy.context_limit_editor.as_mut().unwrap().saving = true;
        handle_event(
            &mut busy,
            &Event::Key(KeyCode::Esc, KeyModifiers::NONE),
            "/workspace/ca",
        );
        assert!(busy.context_limit_editor.is_some());
    }
}

#[cfg(test)]
mod model_prompt_cache_editor_tests {
    use super::*;

    fn editor_state() -> crate::app::AppState {
        let mut state = crate::app::AppState::new(crate::context::ShellHistory::in_memory());
        state.input = "composer stays".into();
        state.model_prompt_cache_editor = Some(crate::app::ModelPromptCacheEditorState {
            provider: "custom".into(),
            model: "model-a".into(),
            protocol: crate::agent::ProviderProtocol::OpenAiResponses,
            request_format: Some(crate::config::PromptCacheRequestFormat::OpenAiResponsesExplicit),
            usage_format: None,
            policy_override: None,
            inherited_policy: Some(crate::config::PromptCachePolicy::Automatic),
            contract_id: "custom_v1:openai_responses_explicit:unspecified".into(),
            contract_source: crate::provider_contract::PromptCacheContractSource::ModelConfig,
            policy_source: crate::provider_contract::PromptCachePolicySource::ContractDefault,
            formats_locked: false,
            field: crate::app::ModelPromptCacheField::UsageFormat,
            saving: false,
            writes_enabled: true,
            error: None,
        });
        state
    }

    #[test]
    fn explicit_auto_usage_cycles_and_save_emits_exact_model_update() {
        let mut state = editor_state();
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Right, KeyModifiers::NONE),
            "/workspace/ca",
        );
        assert_eq!(
            state
                .model_prompt_cache_editor
                .as_ref()
                .unwrap()
                .usage_format,
            Some(crate::config::PromptCacheUsageFormat::Auto)
        );
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Tab, KeyModifiers::NONE),
            "/workspace/ca",
        );
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Right, KeyModifiers::NONE),
            "/workspace/ca",
        );
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
            "/workspace/ca",
        );

        assert_eq!(
            state.pending_input_action,
            Some(crate::tui::commands::InputAction::SaveModelPromptCache(
                crate::config::ModelPromptCacheUpdate {
                    provider: "custom".into(),
                    model: "model-a".into(),
                    request_format: Some(
                        crate::config::PromptCacheRequestFormat::OpenAiResponsesExplicit,
                    ),
                    usage_format: Some(crate::config::PromptCacheUsageFormat::Auto),
                    policy: Some(crate::config::PromptCachePolicy::Automatic),
                }
            ))
        );
        assert_eq!(state.input, "composer stays");
    }

    #[test]
    fn provider_managed_contract_exposes_automatic_policy_only() {
        let mut state = editor_state();
        let editor = state.model_prompt_cache_editor.as_mut().unwrap();
        editor.request_format = Some(crate::config::PromptCacheRequestFormat::ProviderManaged);
        editor.usage_format = Some(crate::config::PromptCacheUsageFormat::OpenAiDetails);
        editor.formats_locked = true;
        editor.field = crate::app::ModelPromptCacheField::Policy;

        handle_event(
            &mut state,
            &Event::Key(KeyCode::Right, KeyModifiers::NONE),
            "/workspace/ca",
        );
        assert_eq!(
            state
                .model_prompt_cache_editor
                .as_ref()
                .unwrap()
                .policy_override,
            Some(crate::config::PromptCachePolicy::Automatic)
        );
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Right, KeyModifiers::NONE),
            "/workspace/ca",
        );
        assert_eq!(
            state
                .model_prompt_cache_editor
                .as_ref()
                .unwrap()
                .policy_override,
            None
        );
    }
}

#[cfg(test)]
mod change_walkthrough_input_tests {
    use super::*;

    fn walkthrough_state(
        status: crate::change_walkthrough::ChangeWalkthroughStatus,
    ) -> crate::app::AppState {
        let mut state = crate::app::AppState::new(crate::context::ShellHistory::in_memory());
        state.change_walkthrough = Some(crate::change_walkthrough::ChangeWalkthroughState {
            target: crate::changes::ReviewTarget::Working,
            provider: "local".into(),
            model: "model".into(),
            status,
            source_hash: Some("a".repeat(64)),
            included_files: vec!["src/lib.rs".into()],
            refusal: None,
            result: None,
            error: None,
            freshness_error: None,
            scroll: 0,
            generation: 1,
        });
        state.change_walkthrough_generation = 1;
        state
    }

    #[test]
    fn escape_closes_and_invalidates_busy_walkthrough() {
        let mut state =
            walkthrough_state(crate::change_walkthrough::ChangeWalkthroughStatus::Generating);
        assert!(handle_change_walkthrough(
            &mut state,
            KeyCode::Esc,
            KeyModifiers::NONE
        ));
        assert!(state.change_walkthrough.is_none());
        assert!(state.change_walkthrough_generation > 1);
        assert_eq!(
            state.pending_input_action,
            Some(crate::tui::commands::InputAction::CancelChangeWalkthrough)
        );
    }

    #[test]
    fn shift_r_recaptures_only_terminal_state_and_clears_old_result() {
        let mut state =
            walkthrough_state(crate::change_walkthrough::ChangeWalkthroughStatus::Stale);
        state.change_walkthrough.as_mut().unwrap().error = Some("old".into());
        assert!(handle_change_walkthrough(
            &mut state,
            KeyCode::Char('R'),
            KeyModifiers::SHIFT
        ));
        let walkthrough = state.change_walkthrough.as_ref().unwrap();
        assert_eq!(
            walkthrough.status,
            crate::change_walkthrough::ChangeWalkthroughStatus::Capturing
        );
        assert!(walkthrough.source_hash.is_none());
        assert!(walkthrough.error.is_none());
        assert!(state.change_walkthrough_requested);
        assert_eq!(
            state.pending_input_action,
            Some(crate::tui::commands::InputAction::CancelChangeWalkthrough)
        );

        let mut busy =
            walkthrough_state(crate::change_walkthrough::ChangeWalkthroughStatus::Generating);
        let generation = busy.change_walkthrough_generation;
        handle_change_walkthrough(&mut busy, KeyCode::Char('R'), KeyModifiers::SHIFT);
        assert_eq!(busy.change_walkthrough_generation, generation);
        assert!(!busy.change_walkthrough_requested);
    }

    #[test]
    fn completed_walkthrough_scrolls_without_touching_composer() {
        let mut state =
            walkthrough_state(crate::change_walkthrough::ChangeWalkthroughStatus::Complete);
        state.input = "draft stays".into();
        handle_change_walkthrough(&mut state, KeyCode::PageDown, KeyModifiers::NONE);
        assert_eq!(state.change_walkthrough.as_ref().unwrap().scroll, 15);
        handle_change_walkthrough(&mut state, KeyCode::Home, KeyModifiers::NONE);
        assert_eq!(state.change_walkthrough.as_ref().unwrap().scroll, 0);
        assert_eq!(state.input, "draft stays");
    }
}

#[cfg(test)]
mod btw_tests {
    use super::*;
    use crate::context::ShellHistory;

    fn state() -> crate::app::AppState {
        let mut state = crate::app::AppState::new(ShellHistory::in_memory());
        state.input = "composer stays".into();
        open_btw(&mut state, String::new(), "mimo".into(), "mimo-v2.5".into());
        state
    }

    #[test]
    fn editor_is_unicode_safe_and_paste_never_reaches_composer() {
        let mut state = state();
        handle_event(&mut state, &Event::Paste("why 🙂\n".into()), "/tmp");
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Left, KeyModifiers::NONE),
            "/tmp",
        );
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Backspace, KeyModifiers::NONE),
            "/tmp",
        );

        let btw = state.btw.as_ref().unwrap();
        assert_eq!(btw.question, "why🙂");
        assert_eq!(state.input, "composer stays");

        handle_event(
            &mut state,
            &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
            "/tmp",
        );
        assert!(state.btw_start_requested);
        assert_eq!(state.btw.unwrap().status, crate::app::BtwStatus::Running);
    }

    #[test]
    fn running_escape_cancels_only_btw_and_stale_events_get_a_new_generation() {
        let mut state = state();
        state.btw.as_mut().unwrap().question = "question".into();
        state.btw.as_mut().unwrap().status = crate::app::BtwStatus::Running;
        let old_generation = state.btw_generation;

        handle_event(
            &mut state,
            &Event::Key(KeyCode::Esc, KeyModifiers::NONE),
            "/tmp",
        );

        assert_eq!(
            state.pending_input_action,
            Some(crate::tui::commands::InputAction::CancelBtw)
        );
        assert_eq!(state.btw.unwrap().status, crate::app::BtwStatus::Cancelled);
        assert!(state.btw_generation > old_generation);
        assert!(state.messages.is_empty());
    }

    #[test]
    fn empty_editor_stays_open_and_completed_overlay_closes_without_composer_changes() {
        let mut state = state();
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
            "/tmp",
        );
        assert_eq!(
            state.btw.as_ref().unwrap().status,
            crate::app::BtwStatus::Editing
        );
        assert!(state.status_message.as_deref().unwrap().contains("Enter"));

        state.btw.as_mut().unwrap().status = crate::app::BtwStatus::Complete;
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Esc, KeyModifiers::NONE),
            "/tmp",
        );
        assert!(state.btw.is_none());
        assert_eq!(state.input, "composer stays");
    }
}

#[cfg(test)]
mod hook_manager_tests {
    use super::*;

    fn state(writes_enabled: bool, execution_enabled: bool) -> crate::app::AppState {
        let mut state = crate::app::AppState::default();
        state.input = "composer stays".into();
        state.input_cursor = state.input.len();
        state.hook_manager = Some(crate::app::HookManagerState {
            filter: String::new(),
            selected_idx: 0,
            entries: vec![
                crate::hooks::HookManagementEntry {
                    index: 0,
                    event: crate::hooks::HookEvent::PreToolUse,
                    enabled: true,
                    timeout_secs: 5,
                    fail_closed: true,
                    last_result: None,
                },
                crate::hooks::HookManagementEntry {
                    index: 1,
                    event: crate::hooks::HookEvent::AfterAgentResponse,
                    enabled: false,
                    timeout_secs: 7,
                    fail_closed: false,
                    last_result: None,
                },
            ],
            network_enforcement: "network denied by policy only".into(),
            config_path: "/tmp/config.toml".into(),
            busy: false,
            testing: false,
            writes_enabled,
            execution_enabled,
            error: None,
            notice: None,
        });
        state
    }

    #[test]
    fn hook_manager_filters_without_touching_composer_and_requests_exact_test() {
        let mut state = state(true, true);
        handle_event(&mut state, &Event::Paste("response".into()), "/tmp");
        assert_eq!(state.hook_manager.as_ref().unwrap().filter, "response");
        assert_eq!(state.input, "composer stays");

        handle_event(
            &mut state,
            &Event::Key(KeyCode::Char('T'), KeyModifiers::SHIFT),
            "/tmp",
        );
        assert!(state.hook_manager.as_ref().unwrap().busy);
        assert!(matches!(
            state.hook_manager_request,
            Some(crate::app::HookManagerRequest {
                operation: crate::app::HookManagerOperation::Test {
                    index: 1,
                    event: crate::hooks::HookEvent::AfterAgentResponse,
                },
                ..
            })
        ));
        let generation = state.hook_manager_generation;
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Esc, KeyModifiers::NONE),
            "/tmp",
        );
        assert!(state.hook_manager.is_some());
        assert!(!state.hook_manager.as_ref().unwrap().busy);
        assert_eq!(
            state.hook_manager.as_ref().unwrap().error.as_deref(),
            Some("Hook test cancelled")
        );
        assert!(state.hook_manager_generation > generation);
    }

    #[test]
    fn hook_manager_toggle_is_exact_and_busy_state_locks_actions() {
        let mut state = state(true, true);
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Down, KeyModifiers::NONE),
            "/tmp",
        );
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
            "/tmp",
        );
        assert!(matches!(
            state.hook_manager_request,
            Some(crate::app::HookManagerRequest {
                operation: crate::app::HookManagerOperation::Toggle {
                    index: 1,
                    event: crate::hooks::HookEvent::AfterAgentResponse,
                    enabled: true,
                },
                ..
            })
        ));
        let generation = state.hook_manager_generation;
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Char('r'), KeyModifiers::SHIFT),
            "/tmp",
        );
        assert_eq!(state.hook_manager_generation, generation);
    }

    #[test]
    fn bare_hook_manager_is_view_only_and_escape_invalidates_pending_results() {
        let mut bare = state(false, false);
        handle_event(
            &mut bare,
            &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
            "/tmp",
        );
        assert!(bare.hook_manager_request.is_none());
        assert!(bare
            .hook_manager
            .as_ref()
            .unwrap()
            .error
            .as_deref()
            .unwrap()
            .contains("bare"));
        handle_event(
            &mut bare,
            &Event::Key(KeyCode::Char('t'), KeyModifiers::SHIFT),
            "/tmp",
        );
        assert!(bare.hook_manager_request.is_none());
        let generation = bare.hook_manager_generation;
        handle_event(
            &mut bare,
            &Event::Key(KeyCode::Esc, KeyModifiers::NONE),
            "/tmp",
        );
        assert!(bare.hook_manager.is_none());
        assert!(bare.hook_manager_generation > generation);
        assert_eq!(bare.input, "composer stays");

        let mut blocked = state(true, true);
        blocked.hook_blocked = true;
        blocked.hook_blocked_event = Some(crate::hooks::HookEvent::SessionStart);
        handle_event(
            &mut blocked,
            &Event::Key(KeyCode::Esc, KeyModifiers::NONE),
            "/tmp",
        );
        assert!(blocked.hook_manager.is_some());
        assert!(blocked
            .hook_manager
            .as_ref()
            .unwrap()
            .error
            .as_deref()
            .unwrap()
            .contains("remains blocked"));
    }
}

#[cfg(test)]
mod memory_manager_tests {
    use super::*;

    fn state(writes_enabled: bool) -> crate::app::AppState {
        let mut state = crate::app::AppState {
            input: "composer stays".into(),
            cwd: "/workspace/ca".into(),
            ..crate::app::AppState::default()
        };
        state.user_memory_records.push(
            crate::memory::MemoryRecord::new(
                crate::memory::MemoryScope::User,
                crate::memory::MemoryKind::UserPreference,
                "Prefer concise status notes.",
                crate::memory::MemoryProvenance::Manual,
            )
            .unwrap(),
        );
        state.memory_manager = Some(crate::app::MemoryManagerState {
            filter: String::new(),
            selected_idx: 0,
            view: crate::app::MemoryManagerView::User,
            editor: None,
            confirmation: None,
            busy: false,
            writes_enabled,
            user_recall: false,
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
    fn filter_and_scope_navigation_do_not_touch_composer() {
        let mut state = state(true);
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Char('c'), KeyModifiers::NONE),
            "/workspace/ca",
        );
        assert_eq!(state.memory_manager.as_ref().unwrap().filter, "c");
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Tab, KeyModifiers::NONE),
            "/workspace/ca",
        );
        let manager = state.memory_manager.as_ref().unwrap();
        assert_eq!(manager.view, crate::app::MemoryManagerView::Workspace);
        assert!(manager.filter.is_empty());
        assert_eq!(state.input, "composer stays");
    }

    #[test]
    fn add_editor_is_unicode_safe_and_emits_validated_fields() {
        let mut state = state(true);
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Char('A'), KeyModifiers::SHIFT),
            "/workspace/ca",
        );
        for character in "Keep 🙂 concise".chars() {
            handle_event(
                &mut state,
                &Event::Key(KeyCode::Char(character), KeyModifiers::NONE),
                "/workspace/ca",
            );
        }
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
            "/workspace/ca",
        );
        assert_eq!(
            state.pending_input_action,
            Some(crate::tui::commands::InputAction::SaveMemoryRecord {
                id: None,
                scope: crate::memory::MemoryScope::User,
                kind: crate::memory::MemoryKind::UserPreference,
                text: "Keep 🙂 concise".into(),
            })
        );
        assert!(state.memory_manager.as_ref().unwrap().busy);
    }

    #[test]
    fn delete_confirmation_defaults_to_cancel_and_busy_locks_escape() {
        let mut state = state(true);
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Char('D'), KeyModifiers::SHIFT),
            "/workspace/ca",
        );
        assert!(
            !state
                .memory_manager
                .as_ref()
                .unwrap()
                .confirmation
                .as_ref()
                .unwrap()
                .confirm_selected
        );
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
            "/workspace/ca",
        );
        assert!(state
            .memory_manager
            .as_ref()
            .unwrap()
            .confirmation
            .is_none());

        state.memory_manager.as_mut().unwrap().busy = true;
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Esc, KeyModifiers::NONE),
            "/workspace/ca",
        );
        assert!(state.memory_manager.is_some());
    }

    #[test]
    fn bare_mode_rejects_mutation_with_guidance() {
        let mut state = state(false);
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Char('A'), KeyModifiers::SHIFT),
            "/workspace/ca",
        );
        assert!(state.pending_input_action.is_none());
        assert!(state
            .memory_manager
            .as_ref()
            .unwrap()
            .error
            .as_deref()
            .unwrap()
            .contains("bare"));
    }
}

#[cfg(test)]
mod permission_manager_tests {
    use super::*;

    fn state(writes_enabled: bool) -> crate::app::AppState {
        let mut state = crate::app::AppState::default();
        state.input = "composer stays".into();
        state.input_cursor = state.input.len();
        state.permission_manager = Some(crate::app::PermissionManagerState {
            filter: String::new(),
            selected_idx: 0,
            entries: vec![
                crate::app::PermissionManagerEntry {
                    source: crate::app::PermissionEntrySource::Rich(0),
                    decision: crate::permission::PermissionDecision::Ask,
                    matcher: "run_command · cargo test".into(),
                },
                crate::app::PermissionManagerEntry {
                    source: crate::app::PermissionEntrySource::Session(0),
                    decision: crate::permission::PermissionDecision::Allow,
                    matcher: "read_file · /tmp/**".into(),
                },
            ],
            confirmation: None,
            busy: false,
            writes_enabled,
            generation: 1,
            error: None,
            notice: None,
        });
        state
    }

    #[test]
    fn permissions_filter_isolated_and_delete_confirmation_defaults_to_cancel() {
        let mut state = state(true);
        handle_event(&mut state, &Event::Paste("cargo".into()), "/tmp");
        assert_eq!(state.input, "composer stays");
        assert_eq!(state.permission_manager.as_ref().unwrap().filter, "cargo");
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Char('D'), KeyModifiers::SHIFT),
            "/tmp",
        );
        let confirmation = state
            .permission_manager
            .as_ref()
            .unwrap()
            .confirmation
            .as_ref()
            .unwrap();
        assert!(!confirmation.delete_selected);
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
            "/tmp",
        );
        assert!(state
            .permission_manager
            .as_ref()
            .unwrap()
            .confirmation
            .is_none());
        assert!(state.pending_input_action.is_none());
    }

    #[test]
    fn permissions_confirm_exact_source_and_lock_while_busy() {
        let mut state = state(true);
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Down, KeyModifiers::NONE),
            "/tmp",
        );
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Char('D'), KeyModifiers::SHIFT),
            "/tmp",
        );
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Right, KeyModifiers::NONE),
            "/tmp",
        );
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
            "/tmp",
        );
        assert!(state.permission_manager.as_ref().unwrap().busy);
        assert_eq!(
            state.pending_input_action,
            Some(crate::tui::commands::InputAction::DeletePermission(
                crate::app::PermissionEntrySource::Session(0)
            ))
        );
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Esc, KeyModifiers::NONE),
            "/tmp",
        );
        assert!(state.permission_manager.is_some());
    }

    #[test]
    fn bare_blocks_persistent_removal_but_allows_session_removal() {
        let mut state = state(false);
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Char('D'), KeyModifiers::SHIFT),
            "/tmp",
        );
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Right, KeyModifiers::NONE),
            "/tmp",
        );
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
            "/tmp",
        );
        assert!(state.pending_input_action.is_none());
        assert!(state
            .permission_manager
            .as_ref()
            .unwrap()
            .error
            .as_deref()
            .unwrap()
            .contains("bare"));

        state.permission_manager.as_mut().unwrap().selected_idx = 1;
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Char('D'), KeyModifiers::SHIFT),
            "/tmp",
        );
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Right, KeyModifiers::NONE),
            "/tmp",
        );
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
            "/tmp",
        );
        assert_eq!(
            state.pending_input_action,
            Some(crate::tui::commands::InputAction::DeletePermission(
                crate::app::PermissionEntrySource::Session(0)
            ))
        );
    }

    #[test]
    fn escape_cancels_an_active_mcp_authorization_before_other_modal_handling() {
        let mut state = state(false);
        state.mcp_task_active = true;
        state.pager_open = true;
        state.pager_content = "MCP authorization waiting".into();
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Esc, KeyModifiers::NONE),
            "/tmp",
        );
        assert!(state.mcp_cancel_requested);
        assert!(!state.pager_open);
        assert_eq!(
            state.status_message.as_deref(),
            Some("Cancelling MCP authorization…")
        );
    }
}

#[cfg(test)]
mod ui_manager_input_tests {
    use super::*;

    fn theme_state(writes: bool) -> crate::app::AppState {
        let mut state = crate::app::AppState::default();
        state.run_capabilities.config_writes = writes;
        state.theme_manager = Some(crate::app::ThemeManagerState {
            saved_theme: crate::ui_settings::UiTheme::Default,
            selected_idx: 0,
            filter: String::new(),
            busy: false,
            notice: None,
            error: None,
        });
        state
    }

    fn statusline_state(writes: bool) -> crate::app::AppState {
        let mut state = crate::app::AppState::default();
        state.run_capabilities.config_writes = writes;
        state.statusline_manager = Some(crate::app::StatuslineManagerState {
            selected_idx: 0,
            fields: vec![
                crate::ui_settings::StatusField::Messages,
                crate::ui_settings::StatusField::Tokens,
                crate::ui_settings::StatusField::SendHint,
            ],
            custom_configured: true,
            custom_enabled: false,
            refresh_secs: 5,
            busy: false,
            testing: false,
            notice: None,
            error: None,
        });
        state
    }

    fn notification_state(writes: bool) -> crate::app::AppState {
        let mut state = crate::app::AppState::default();
        state.run_capabilities.config_writes = writes;
        state.notification_picker = Some(crate::app::NotificationPickerState {
            saved: crate::ui_settings::NotificationStyle::Off,
            selected_idx: 0,
            busy: false,
            error: None,
        });
        state
    }

    #[test]
    fn theme_filter_isolated_preview_is_reversible_and_enter_saves() {
        let mut state = theme_state(true);
        state.input = "composer stays".into();
        state.input_cursor = state.input.len();
        handle_event(&mut state, &Event::Paste("amb".into()), "/tmp");
        assert_eq!(state.input, "composer stays");
        assert_eq!(
            state.theme_manager.as_ref().unwrap().selected_theme(),
            Some(crate::ui_settings::UiTheme::Amber)
        );
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Esc, KeyModifiers::NONE),
            "/tmp",
        );
        assert!(state.theme_manager.is_none());
        assert_eq!(state.ui_theme, crate::ui_settings::UiTheme::Default);

        state = theme_state(true);
        state.theme_manager.as_mut().unwrap().selected_idx = 2;
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
            "/tmp",
        );
        assert_eq!(
            state.pending_input_action,
            Some(crate::tui::commands::InputAction::SaveTheme(
                crate::ui_settings::UiTheme::Amber
            ))
        );
        assert!(state.theme_manager.as_ref().unwrap().busy);
    }

    #[test]
    fn theme_bare_mode_previews_but_refuses_save() {
        let mut state = theme_state(false);
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
            "/tmp",
        );
        assert!(state.pending_input_action.is_none());
        assert!(state
            .theme_manager
            .as_ref()
            .unwrap()
            .error
            .as_deref()
            .unwrap()
            .contains("bare"));
    }

    #[test]
    fn notification_picker_saves_exact_choice_and_bare_mode_refuses() {
        let mut state = notification_state(true);
        handle_event(
            &mut state,
            &Event::Key(KeyCode::End, KeyModifiers::NONE),
            "/tmp",
        );
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
            "/tmp",
        );
        assert_eq!(
            state.pending_input_action,
            Some(crate::tui::commands::InputAction::SaveNotification(
                crate::ui_settings::NotificationStyle::Osc9
            ))
        );
        assert!(state.notification_picker.as_ref().unwrap().busy);

        let mut bare = notification_state(false);
        handle_event(
            &mut bare,
            &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
            "/tmp",
        );
        assert!(bare.pending_input_action.is_none());
        assert!(bare
            .notification_picker
            .as_ref()
            .unwrap()
            .error
            .as_deref()
            .unwrap()
            .contains("bare"));
    }

    #[test]
    fn reasoning_picker_navigates_selects_and_preserves_composer() {
        let mut state = crate::app::AppState {
            input: "keep this draft".into(),
            reasoning_effort: Some(crate::config::ReasoningEffort::Low),
            reasoning_picker: Some(crate::app::ReasoningPickerState {
                efforts: vec![
                    crate::config::ReasoningEffort::Low,
                    crate::config::ReasoningEffort::Medium,
                    crate::config::ReasoningEffort::High,
                ],
                selected_idx: 0,
            }),
            ..crate::app::AppState::default()
        };
        state.input_cursor = state.input.len();

        handle_event(
            &mut state,
            &Event::Key(KeyCode::Down, KeyModifiers::NONE),
            "/tmp",
        );
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Char('j'), KeyModifiers::NONE),
            "/tmp",
        );
        assert_eq!(state.reasoning_picker.as_ref().unwrap().selected_idx, 2);
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Home, KeyModifiers::NONE),
            "/tmp",
        );
        handle_event(
            &mut state,
            &Event::Key(KeyCode::End, KeyModifiers::NONE),
            "/tmp",
        );
        handle_event(&mut state, &Event::Paste("ignored".into()), "/tmp");
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
            "/tmp",
        );

        assert!(state.reasoning_picker.is_none());
        assert_eq!(
            state.pending_input_action,
            Some(crate::tui::commands::InputAction::SelectReasoning(
                crate::config::ReasoningEffort::High
            ))
        );
        assert_eq!(state.input, "keep this draft");
        assert_eq!(
            state.reasoning_effort,
            Some(crate::config::ReasoningEffort::Low)
        );
    }

    #[test]
    fn reasoning_picker_escape_cancels_without_changing_selection() {
        let mut state = crate::app::AppState {
            input: "keep this draft".into(),
            reasoning_effort: Some(crate::config::ReasoningEffort::Medium),
            reasoning_picker: Some(crate::app::ReasoningPickerState {
                efforts: vec![
                    crate::config::ReasoningEffort::Low,
                    crate::config::ReasoningEffort::Medium,
                ],
                selected_idx: 1,
            }),
            ..crate::app::AppState::default()
        };
        state.input_cursor = state.input.len();

        handle_event(
            &mut state,
            &Event::Key(KeyCode::Esc, KeyModifiers::NONE),
            "/tmp",
        );

        assert!(state.reasoning_picker.is_none());
        assert!(state.pending_input_action.is_none());
        assert_eq!(state.input, "keep this draft");
        assert_eq!(
            state.reasoning_effort,
            Some(crate::config::ReasoningEffort::Medium)
        );
    }

    #[test]
    fn route_preset_picker_applies_and_overwrite_defaults_to_cancel() {
        let preset = crate::config::RoutePreset {
            provider: "local".into(),
            model: "coder".into(),
            reasoning_effort: Some(crate::config::ReasoningEffort::High),
        };
        let mut state = crate::app::AppState {
            route_preset_picker: Some(crate::app::RoutePresetPickerState {
                rows: vec![crate::app::RoutePresetRow {
                    name: "daily".into(),
                    preset: preset.clone(),
                }],
                selected_idx: 0,
                error: None,
            }),
            ..crate::app::AppState::default()
        };
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
            "/tmp",
        );
        assert_eq!(
            state.pending_input_action,
            Some(crate::tui::commands::InputAction::ApplyRoutePreset(
                "daily".into()
            ))
        );

        state.pending_input_action = None;
        state.route_preset_picker = None;
        state.route_preset_overwrite = Some(crate::app::RoutePresetOverwriteConfirmation {
            name: "daily".into(),
            preset: preset.clone(),
            save_selected: false,
            busy: false,
            error: None,
        });
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
            "/tmp",
        );
        assert!(state.route_preset_overwrite.is_none());
        assert!(state.pending_input_action.is_none());

        state.route_preset_overwrite = Some(crate::app::RoutePresetOverwriteConfirmation {
            name: "daily".into(),
            preset: preset.clone(),
            save_selected: false,
            busy: false,
            error: None,
        });
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Right, KeyModifiers::NONE),
            "/tmp",
        );
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
            "/tmp",
        );
        assert_eq!(
            state.pending_input_action,
            Some(crate::tui::commands::InputAction::SaveRoutePreset {
                name: "daily".into(),
                preset,
            })
        );
        assert!(state.route_preset_overwrite.as_ref().unwrap().busy);
    }

    #[test]
    fn statusline_toggles_reorders_tests_and_saves_one_draft() {
        let mut state = statusline_state(true);
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Char(' '), KeyModifiers::NONE),
            "/tmp",
        );
        assert!(!state
            .statusline_manager
            .as_ref()
            .unwrap()
            .fields
            .contains(&crate::ui_settings::StatusField::Messages));
        state.statusline_manager.as_mut().unwrap().selected_idx =
            crate::ui_settings::StatusField::ALL
                .iter()
                .position(|field| *field == crate::ui_settings::StatusField::SendHint)
                .unwrap();
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Up, KeyModifiers::SHIFT),
            "/tmp",
        );
        assert_eq!(
            state.statusline_manager.as_ref().unwrap().fields,
            vec![
                crate::ui_settings::StatusField::SendHint,
                crate::ui_settings::StatusField::Tokens,
            ]
        );
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Char('T'), KeyModifiers::SHIFT),
            "/tmp",
        );
        assert_eq!(
            state.pending_input_action,
            Some(crate::tui::commands::InputAction::TestStatusCommand)
        );
        state.pending_input_action = None;
        state.statusline_manager.as_mut().unwrap().testing = false;
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
            "/tmp",
        );
        assert_eq!(
            state.pending_input_action,
            Some(crate::tui::commands::InputAction::SaveStatusline(
                crate::ui_settings::StatuslineUpdate {
                    fields: vec![
                        crate::ui_settings::StatusField::SendHint,
                        crate::ui_settings::StatusField::Tokens,
                    ],
                    custom_enabled: false,
                }
            ))
        );
    }

    #[test]
    fn closing_statusline_while_testing_requests_cancellation() {
        let mut state = statusline_state(true);
        state.statusline_manager.as_mut().unwrap().testing = true;
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Esc, KeyModifiers::NONE),
            "/tmp",
        );
        assert!(state.statusline_manager.is_none());
        assert_eq!(
            state.pending_input_action,
            Some(crate::tui::commands::InputAction::CancelStatusCommand)
        );
    }

    #[test]
    fn statusline_paste_and_busy_inputs_never_reach_composer() {
        let mut state = statusline_state(true);
        state.input = "composer".into();
        handle_event(&mut state, &Event::Paste("ignored".into()), "/tmp");
        assert_eq!(state.input, "composer");
        state.statusline_manager.as_mut().unwrap().busy = true;
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Esc, KeyModifiers::NONE),
            "/tmp",
        );
        assert!(state.statusline_manager.is_some());
    }
}

#[cfg(test)]
mod terminal_manager_input_tests {
    use super::*;

    fn state() -> crate::app::AppState {
        crate::app::AppState {
            terminal_manager: Some(crate::app::TerminalManagerState {
                sessions: vec![
                    crate::terminal_sessions::TerminalSession {
                        name: "alpha".into(),
                        windows: 1,
                        attached_clients: 0,
                        created_unix: 1,
                    },
                    crate::terminal_sessions::TerminalSession {
                        name: "beta".into(),
                        windows: 2,
                        attached_clients: 1,
                        created_unix: 2,
                    },
                ],
                selected_idx: 0,
                filter: String::new(),
                version: Some("tmux 3.5".into()),
                unavailable: false,
                busy: false,
                mode: crate::app::TerminalManagerMode::List,
                kill_confirmation: None,
                notice: None,
                error: None,
            }),
            ..crate::app::AppState::default()
        }
    }

    #[test]
    fn filters_navigates_and_attaches_selected_session() {
        let mut state = state();
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Char('b'), KeyModifiers::NONE),
            "/workspace/ca",
        );
        assert_eq!(state.terminal_manager.as_ref().unwrap().filter, "b");
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
            "/workspace/ca",
        );
        assert!(matches!(
            state.pending_input_action,
            Some(crate::tui::commands::InputAction::AttachTerminal(ref name)) if name == "beta"
        ));
        assert!(state.terminal_manager.as_ref().unwrap().busy);
    }

    #[test]
    fn paste_updates_only_the_visible_terminal_field() {
        let mut state = state();
        state.input = "composer stays".into();
        state.input_cursor = state.input.len();
        handle_event(
            &mut state,
            &Event::Paste("dev\nignored".into()),
            "/workspace/ca",
        );
        assert_eq!(state.input, "composer stays");
        assert_eq!(
            state.terminal_manager.as_ref().unwrap().filter,
            "devignored"
        );
    }

    #[test]
    fn create_validates_name_and_emits_one_action() {
        let mut state = state();
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Char('A'), KeyModifiers::SHIFT),
            "/workspace/ca",
        );
        for character in "dev-1".chars() {
            handle_event(
                &mut state,
                &Event::Key(KeyCode::Char(character), KeyModifiers::NONE),
                "/workspace/ca",
            );
        }
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
            "/workspace/ca",
        );
        assert!(matches!(
            state.pending_input_action,
            Some(crate::tui::commands::InputAction::CreateTerminal(ref name)) if name == "dev-1"
        ));
    }

    #[test]
    fn kill_confirmation_defaults_to_cancel_and_busy_locks_it() {
        let mut state = state();
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Char('K'), KeyModifiers::SHIFT),
            "/workspace/ca",
        );
        assert!(
            !state
                .terminal_manager
                .as_ref()
                .unwrap()
                .kill_confirmation
                .as_ref()
                .unwrap()
                .kill_selected
        );
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
            "/workspace/ca",
        );
        assert!(state
            .terminal_manager
            .as_ref()
            .unwrap()
            .kill_confirmation
            .is_none());

        state.terminal_manager.as_mut().unwrap().busy = true;
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Esc, KeyModifiers::NONE),
            "/workspace/ca",
        );
        assert!(state.terminal_manager.is_some());
    }

    #[test]
    fn confirmed_kill_targets_only_the_selected_session() {
        let mut state = state();
        state.terminal_manager.as_mut().unwrap().selected_idx = 1;
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Char('K'), KeyModifiers::SHIFT),
            "/workspace/ca",
        );
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Right, KeyModifiers::NONE),
            "/workspace/ca",
        );
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
            "/workspace/ca",
        );
        assert!(matches!(
            state.pending_input_action,
            Some(crate::tui::commands::InputAction::KillTerminal(ref name)) if name == "beta"
        ));
    }
}

#[cfg(test)]
mod task_graph_input_tests {
    use super::*;

    fn state() -> crate::app::AppState {
        let mut state = crate::app::AppState {
            input: "composer stays".into(),
            task_graph_manager: Some(crate::app::TaskGraphManagerState {
                filter: String::new(),
                selected_idx: 0,
                confirmation: None,
                error: None,
                notice: None,
            }),
            ..crate::app::AppState::default()
        };
        state.input_cursor = state.input.len();
        let mut work = crate::work::WorkState::default();
        work.apply_model_mutation(crate::work::WorkMutation::Propose {
            body: None,
            expected_revision: 0,
            title: "Graph".into(),
            tasks: vec!["First task".into(), "Second task".into()],
        })
        .unwrap();
        *state.work_state.lock().unwrap() = work;
        state
    }

    #[test]
    fn graph_filter_and_paste_are_isolated_from_the_composer() {
        let mut state = state();
        handle_event(
            &mut state,
            &Event::Paste("Second\nignored".into()),
            "/workspace/ca",
        );
        assert_eq!(state.input, "composer stays");
        assert_eq!(
            state.task_graph_manager.as_ref().unwrap().filter,
            "Secondignored"
        );
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Home, KeyModifiers::NONE),
            "/workspace/ca",
        );
        assert_eq!(state.task_graph_manager.as_ref().unwrap().selected_idx, 0);
    }

    #[test]
    fn graph_start_confirmation_is_cancel_first_and_busy_locked() {
        let mut state = state();
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Char('S'), KeyModifiers::SHIFT),
            "/workspace/ca",
        );
        assert!(
            !state
                .task_graph_manager
                .as_ref()
                .unwrap()
                .confirmation
                .as_ref()
                .unwrap()
                .confirm_selected
        );
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
            "/workspace/ca",
        );
        assert!(state
            .task_graph_manager
            .as_ref()
            .unwrap()
            .confirmation
            .is_none());
        assert!(state.pending_input_action.is_none());

        handle_event(
            &mut state,
            &Event::Key(KeyCode::Char('S'), KeyModifiers::SHIFT),
            "/workspace/ca",
        );
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Right, KeyModifiers::NONE),
            "/workspace/ca",
        );
        state.agent_graph_busy = true;
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
            "/workspace/ca",
        );
        assert!(state.pending_input_action.is_none());
        assert!(state
            .task_graph_manager
            .as_ref()
            .unwrap()
            .error
            .as_deref()
            .unwrap()
            .contains("in progress"));
        state.agent_graph_busy = false;
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
            "/workspace/ca",
        );
        assert!(matches!(
            state.pending_input_action,
            Some(crate::tui::commands::InputAction::StartAgentGraph)
        ));
    }
}

#[cfg(test)]
mod workflow_input_tests {
    use super::*;

    fn loaded() -> crate::workflow::LoadedWorkflow {
        crate::workflow::LoadedWorkflow {
            definition: crate::workflow::WorkflowDefinition {
                version: 1,
                id: "daily-check".into(),
                title: "Daily check".into(),
                steps: vec![crate::workflow::WorkflowStep {
                    id: "verify".into(),
                    depends_on: Vec::new(),
                    prompt: "Verify the workspace".into(),
                    model: None,
                    tools: Some(Vec::new()),
                    grants: Vec::new(),
                    retry: crate::workflow::WorkflowRetryPolicy::default(),
                    output: None,
                    evidence: vec![crate::workflow::WorkflowEvidenceGate::AssistantOutput],
                }],
            }
            .validate()
            .unwrap(),
            relative_path: ".ca/workflows/daily.toml".into(),
            digest: "0123456789abcdef".into(),
        }
    }

    fn preview_state() -> crate::app::AppState {
        let mut state = crate::app::AppState::default();
        state.input = "composer stays".into();
        state.input_cursor = state.input.len();
        state.workflow_preview = Some(crate::workflow::WorkflowPreview {
            loaded: loaded(),
            resume_existing: false,
            run_selected: false,
            error: None,
        });
        state
    }

    #[test]
    fn workflow_preview_is_cancel_first_and_paste_isolated() {
        let mut state = preview_state();
        handle_event(
            &mut state,
            &Event::Paste("do not inject".into()),
            "/workspace/ca",
        );
        assert_eq!(state.input, "composer stays");
        assert!(state.workflow_preview.is_some());

        handle_event(
            &mut state,
            &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
            "/workspace/ca",
        );
        assert!(state.workflow_preview.is_none());
        assert!(!state.workflow_start_requested);
        assert!(state.workflow_loaded.is_none());
    }

    #[test]
    fn workflow_preview_requires_explicit_run_selection() {
        let mut state = preview_state();
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Right, KeyModifiers::NONE),
            "/workspace/ca",
        );
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
            "/workspace/ca",
        );
        assert!(state.workflow_start_requested);
        assert_eq!(
            state.workflow_loaded.as_ref().unwrap().definition.id,
            "daily-check"
        );
    }

    #[test]
    fn workflow_clear_confirmation_is_cancel_first() {
        let mut state = crate::app::AppState {
            workflow_clear_confirmation: Some(false),
            ..crate::app::AppState::default()
        };
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
            "/workspace/ca",
        );
        assert!(!state.workflow_clear_requested);

        state.workflow_clear_confirmation = Some(false);
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Right, KeyModifiers::NONE),
            "/workspace/ca",
        );
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
            "/workspace/ca",
        );
        assert!(state.workflow_clear_requested);
    }
}

#[cfg(test)]
mod worktree_input_tests {
    use super::*;

    fn confirmation() -> crate::worktree::WorktreeCleanupConfirmation {
        crate::worktree::WorktreeCleanupConfirmation {
            id: uuid::Uuid::nil(),
            label: "review".into(),
            branch: "ca/review-00000000".into(),
            remove_selected: false,
            busy: false,
            error: None,
        }
    }

    fn setup_confirmation() -> crate::worktree_setup::SetupConfirmation {
        let record = crate::worktree::WorktreeRecord {
            id: uuid::Uuid::nil(),
            label: "launch".into(),
            repository_root: "/workspace/ca".into(),
            path: "/profile/worktrees/launch".into(),
            branch: "ca/launch-00000000".into(),
            base_commit: "0123456789abcdef0123456789abcdef01234567".into(),
            git_common_dir: "/workspace/ca/.git".into(),
            created_at: chrono::Utc::now(),
        };
        crate::worktree_setup::SetupConfirmation::new(
            record,
            crate::worktree_setup::SetupPreview {
                source_kind: crate::worktree_setup::SetupSourceKind::Native,
                source_path: "/workspace/ca/.ca/worktrees.toml".into(),
                source_sha256: "a".repeat(64),
                repository_root: "/workspace/ca".into(),
                label: "launch".into(),
                base_ref: "main".into(),
                base_commit: "0123456789abcdef0123456789abcdef01234567".into(),
                steps: vec![crate::worktree_setup::SetupStep {
                    name: "bootstrap".into(),
                    program: "cargo".into(),
                    args: vec!["fetch".into()],
                    cwd: ".".into(),
                    sandbox: crate::sandbox::SandboxMode::Required,
                    network: false,
                }],
                digest: "b".repeat(64),
            },
        )
    }

    #[test]
    fn worktree_cleanup_is_cancel_first_and_paste_isolated() {
        let mut state = crate::app::AppState {
            input: "composer stays".into(),
            worktree_cleanup_confirmation: Some(confirmation()),
            ..crate::app::AppState::default()
        };
        state.input_cursor = state.input.len();
        handle_event(
            &mut state,
            &Event::Paste("must not enter composer".into()),
            "/workspace/ca",
        );
        assert_eq!(state.input, "composer stays");
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
            "/workspace/ca",
        );
        assert!(state.worktree_cleanup_confirmation.is_none());
        assert!(state.worktree_request.is_none());
    }

    #[test]
    fn worktree_cleanup_requires_explicit_selection_and_locks_while_busy() {
        let mut state = crate::app::AppState {
            worktree_cleanup_confirmation: Some(confirmation()),
            ..crate::app::AppState::default()
        };
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Right, KeyModifiers::NONE),
            "/workspace/ca",
        );
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
            "/workspace/ca",
        );
        assert!(state.worktree_busy);
        assert!(state.worktree_cleanup_confirmation.as_ref().unwrap().busy);
        assert!(matches!(
            state
                .worktree_request
                .as_ref()
                .map(|request| &request.action),
            Some(crate::worktree::WorktreeAction::ConfirmCleanup { .. })
        ));

        handle_event(
            &mut state,
            &Event::Key(KeyCode::Esc, KeyModifiers::NONE),
            "/workspace/ca",
        );
        assert!(state.worktree_cleanup_confirmation.is_some());
        assert!(state.worktree_request.is_some());
    }

    #[test]
    fn launch_setup_is_skip_first_paste_isolated_and_run_locks() {
        let mut state = crate::app::AppState {
            input: "composer stays".into(),
            worktree_setup_confirmation: Some(setup_confirmation()),
            ..crate::app::AppState::default()
        };
        state.input_cursor = state.input.len();
        handle_event(
            &mut state,
            &Event::Paste("must not enter composer".into()),
            "/workspace/ca",
        );
        assert_eq!(state.input, "composer stays");
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
            "/workspace/ca",
        );
        assert!(state.worktree_setup_confirmation.is_none());

        state.worktree_setup_confirmation = Some(setup_confirmation());
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Right, KeyModifiers::NONE),
            "/workspace/ca",
        );
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
            "/workspace/ca",
        );
        assert!(state.worktree_setup_confirmation.as_ref().unwrap().busy);
        assert!(matches!(
            state.pending_input_action,
            Some(crate::tui::commands::InputAction::RunLaunchWorktreeSetup)
        ));
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Esc, KeyModifiers::NONE),
            "/workspace/ca",
        );
        assert!(state.worktree_setup_confirmation.is_some());
    }

    #[test]
    fn support_bundle_is_cancel_first_paste_isolated_and_create_locks() {
        let mut state = crate::app::AppState {
            input: "composer stays".into(),
            support_bundle_confirmation: Some(crate::app::SupportBundleConfirmation {
                plan: crate::support_bundle::test_plan(),
                create_selected: false,
                busy: false,
                error: None,
            }),
            ..crate::app::AppState::default()
        };
        state.input_cursor = state.input.len();
        handle_event(
            &mut state,
            &Event::Paste("must not enter composer".into()),
            "/workspace/ca",
        );
        assert_eq!(state.input, "composer stays");
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
            "/workspace/ca",
        );
        assert!(state.support_bundle_confirmation.is_none());

        state.support_bundle_confirmation = Some(crate::app::SupportBundleConfirmation {
            plan: crate::support_bundle::test_plan(),
            create_selected: false,
            busy: false,
            error: None,
        });
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Right, KeyModifiers::NONE),
            "/workspace/ca",
        );
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
            "/workspace/ca",
        );
        assert!(state.support_bundle_confirmation.as_ref().unwrap().busy);
        assert!(matches!(
            state.pending_input_action,
            Some(crate::tui::commands::InputAction::CreateSupportBundle)
        ));
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Esc, KeyModifiers::NONE),
            "/workspace/ca",
        );
        assert!(state.support_bundle_confirmation.is_some());
    }

    #[test]
    fn setup_import_is_cancel_first_paste_isolated_and_apply_locks() {
        let confirmation = || crate::app::SetupImportConfirmation {
            preview: crate::setup_import::test_preview(),
            apply_selected: false,
            busy: false,
            error: None,
            scroll: 0,
        };
        let mut state = crate::app::AppState {
            input: "composer stays".into(),
            setup_import_confirmation: Some(confirmation()),
            ..crate::app::AppState::default()
        };
        state.input_cursor = state.input.len();
        handle_event(
            &mut state,
            &Event::Paste("must not enter composer".into()),
            "/workspace/ca",
        );
        assert_eq!(state.input, "composer stays");
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
            "/workspace/ca",
        );
        assert!(state.setup_import_confirmation.is_none());

        state.setup_import_confirmation = Some(confirmation());
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Down, KeyModifiers::NONE),
            "/workspace/ca",
        );
        assert_eq!(state.setup_import_confirmation.as_ref().unwrap().scroll, 1);
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Right, KeyModifiers::NONE),
            "/workspace/ca",
        );
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
            "/workspace/ca",
        );
        assert!(state.setup_import_confirmation.as_ref().unwrap().busy);
        assert!(matches!(
            state.pending_input_action,
            Some(crate::tui::commands::InputAction::ApplySetupImport)
        ));
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Esc, KeyModifiers::NONE),
            "/workspace/ca",
        );
        assert!(state.setup_import_confirmation.is_some());
    }
}

#[cfg(test)]
mod multi_run_input_tests {
    use super::*;

    fn model(provider: &str, id: &str) -> crate::app::ModelInfo {
        crate::app::ModelInfo {
            provider: provider.into(),
            id: id.into(),
            display_name: id.into(),
        }
    }

    fn editing_state() -> crate::app::AppState {
        let current = model("alpha", "one");
        let mut state = crate::app::AppState::default();
        state.input = "composer stays".into();
        state.input_cursor = state.input.len();
        state.current_model = Some(current.clone());
        state.model_list = vec![current, model("beta", "two")];
        state.multi_run_manager = Some(crate::app::MultiRunManagerState {
            draft_request: String::new(),
            context_snapshot: "frozen context".into(),
            pricing: std::collections::HashMap::new(),
            cursor: 0,
            candidate_count: 2,
            editing: true,
            run: None,
            confirmation: None,
            busy: false,
            error: None,
            notice: None,
        });
        state
    }

    fn preview_state() -> crate::app::AppState {
        let mut state = editing_state();
        let definition = crate::multi_run::MultiRunDefinition::build(
            "same request",
            2,
            state.current_model.as_ref(),
            &state.model_list,
        )
        .unwrap();
        let manager = state.multi_run_manager.as_mut().unwrap();
        manager.editing = false;
        manager.run = Some(crate::multi_run::MultiRunState::preview(
            definition,
            manager.context_snapshot.clone(),
        ));
        state
    }

    #[test]
    fn setup_paste_isolated_from_composer_and_builds_bounded_preview() {
        let mut state = editing_state();
        handle_event(
            &mut state,
            &Event::Paste("implement this carefully".into()),
            "/workspace/ca",
        );
        assert_eq!(state.input, "composer stays");
        assert_eq!(
            state.multi_run_manager.as_ref().unwrap().draft_request,
            "implement this carefully"
        );
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
            "/workspace/ca",
        );
        let manager = state.multi_run_manager.as_ref().unwrap();
        assert!(!manager.editing);
        assert_eq!(manager.run.as_ref().unwrap().candidates.len(), 2);
        assert_eq!(
            manager.run.as_ref().unwrap().context_summary,
            "frozen context"
        );
    }

    #[test]
    fn start_is_cancel_first_then_emits_explicit_action_and_locks() {
        let mut state = preview_state();
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Char('S'), KeyModifiers::SHIFT),
            "/workspace/ca",
        );
        assert!(
            !state
                .multi_run_manager
                .as_ref()
                .unwrap()
                .confirmation
                .as_ref()
                .unwrap()
                .confirm_selected
        );
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
            "/workspace/ca",
        );
        assert!(state.pending_input_action.is_none());

        handle_event(
            &mut state,
            &Event::Key(KeyCode::Char('S'), KeyModifiers::SHIFT),
            "/workspace/ca",
        );
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Right, KeyModifiers::NONE),
            "/workspace/ca",
        );
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
            "/workspace/ca",
        );
        assert_eq!(
            state.pending_input_action,
            Some(crate::tui::commands::InputAction::StartMultiRun)
        );
        assert!(state.multi_run_manager.as_ref().unwrap().busy);
    }

    #[test]
    fn candidate_route_picker_does_not_select_the_global_model() {
        let mut state = preview_state();
        let original = state.current_model.clone();
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
            "/workspace/ca",
        );
        assert!(state.model_picker_open);
        state.selected_model_idx = 1;
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
            "/workspace/ca",
        );
        assert_eq!(state.current_model, original);
        assert!(matches!(
            state.pending_input_action,
            Some(crate::tui::commands::InputAction::SelectMultiRunRoute {
                candidate: crate::multi_run::CandidateId(1),
                model: crate::app::ModelInfo { ref provider, ref id, .. },
            }) if provider == "beta" && id == "two"
        ));
    }

    #[test]
    fn candidate_route_picker_blocks_provider_mutation_shortcuts() {
        let mut state = preview_state();
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
            "/workspace/ca",
        );
        assert!(state.model_picker_open);

        for key in ['A', 'P', 'E'] {
            state.pending_input_action = None;
            handle_event(
                &mut state,
                &Event::Key(KeyCode::Char(key), KeyModifiers::SHIFT),
                "/workspace/ca",
            );
            assert!(state.model_picker_open);
            assert!(state.pending_input_action.is_none());
            assert!(state
                .status_message
                .as_deref()
                .is_some_and(|message| message.contains("selection-only")));
        }
    }

    #[test]
    fn hiding_and_reopening_can_preserve_process_only_run_state() {
        let mut state = preview_state();
        state
            .multi_run_manager
            .as_mut()
            .unwrap()
            .run
            .as_mut()
            .unwrap()
            .status = crate::multi_run::MultiRunStatus::Running;
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Esc, KeyModifiers::NONE),
            "/workspace/ca",
        );
        assert!(state.multi_run_manager.is_none());
        assert_eq!(
            state.multi_run_stashed.as_ref().unwrap().status,
            crate::multi_run::MultiRunStatus::Running
        );
    }
}

#[cfg(test)]
mod delegation_picker_input_tests {
    use super::*;

    fn state() -> crate::app::AppState {
        crate::app::AppState {
            model_list: vec![
                crate::app::ModelInfo {
                    provider: "openai".into(),
                    id: "gpt-5.6-sol".into(),
                    display_name: "GPT-5.6 Sol".into(),
                },
                crate::app::ModelInfo {
                    provider: "mimo".into(),
                    id: "mimo-v2.5-pro".into(),
                    display_name: "MiMo V2.5 Pro".into(),
                },
                crate::app::ModelInfo {
                    provider: "mimo".into(),
                    id: "discovered-only".into(),
                    display_name: "Discovered only".into(),
                },
            ],
            ..crate::app::AppState::default()
        }
    }

    #[test]
    fn delegated_agent_picker_saves_inherit_or_an_exact_cross_provider_route() {
        let mut state = state();
        let configured = [
            crate::config::DelegationRoute {
                provider: "openai".into(),
                model: "gpt-5.6-sol".into(),
            },
            crate::config::DelegationRoute {
                provider: "mimo".into(),
                model: "mimo-v2.5-pro".into(),
            },
        ];
        open_delegation_model_picker(&mut state, None, &configured);
        assert_eq!(visible_model_rows(&state).len(), 2);
        assert!(visible_model_rows(&state)
            .iter()
            .all(|row| row.model.id != "discovered-only"));
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
            "/workspace/ca",
        );
        assert_eq!(
            state.pending_input_action,
            Some(crate::tui::commands::InputAction::SaveDelegation(None))
        );

        state.pending_input_action = None;
        let route = crate::config::DelegationRoute {
            provider: "mimo".into(),
            model: "mimo-v2.5-pro".into(),
        };
        open_delegation_model_picker(&mut state, Some(&route), &configured);
        assert_eq!(
            selected_picker_model(&state)
                .as_ref()
                .map(|model| model.id.as_str()),
            Some("mimo-v2.5-pro")
        );
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
            "/workspace/ca",
        );
        assert_eq!(
            state.pending_input_action,
            Some(crate::tui::commands::InputAction::SaveDelegation(Some(
                route
            )))
        );
    }
}

#[cfg(test)]
mod provider_auth_input_tests {
    use super::*;

    fn key(state: &mut crate::app::AppState, code: KeyCode) {
        handle_event(
            state,
            &Event::Key(code, KeyModifiers::NONE),
            "/workspace/ca",
        );
    }

    fn type_text(state: &mut crate::app::AppState, value: &str) {
        for character in value.chars() {
            key(state, KeyCode::Char(character));
        }
    }

    #[test]
    fn mimo_token_plan_flow_masks_key_and_confirms_actual_usd_difference() {
        let mut state = crate::app::AppState::default();
        begin_provider_auth(
            &mut state,
            crate::provider_connect::FirstClassProvider::Mimo,
        );
        state.pending_input_action = None;
        assert_eq!(
            state.provider_auth.as_ref().unwrap().stage,
            crate::app::ProviderAuthStage::MethodPicker
        );

        key(&mut state, KeyCode::Down);
        key(&mut state, KeyCode::Enter);
        key(&mut state, KeyCode::Down);
        key(&mut state, KeyCode::Enter);
        key(&mut state, KeyCode::Enter);
        key(&mut state, KeyCode::Enter);
        assert_eq!(
            state.provider_auth.as_ref().unwrap().stage,
            crate::app::ProviderAuthStage::SecretInput
        );

        type_text(&mut state, "sk-no");
        key(&mut state, KeyCode::Enter);
        assert!(state
            .provider_auth
            .as_ref()
            .unwrap()
            .error
            .as_deref()
            .unwrap()
            .contains("tp-"));
        for _ in 0..5 {
            key(&mut state, KeyCode::Backspace);
        }
        type_text(&mut state, "tp-private-never-render");
        assert!(!format!("{:?}", state.provider_auth).contains("private-never-render"));
        key(&mut state, KeyCode::Enter);
        assert_eq!(
            state.provider_auth.as_ref().unwrap().stage,
            crate::app::ProviderAuthStage::PlanUsdInput
        );
        type_text(&mut state, "5");
        key(&mut state, KeyCode::Enter);
        assert_eq!(
            state.provider_auth.as_ref().unwrap().stage,
            crate::app::ProviderAuthStage::PriceConfirmation
        );
        key(&mut state, KeyCode::Enter);
        assert_eq!(
            state.provider_auth.as_ref().unwrap().stage,
            crate::app::ProviderAuthStage::PlanUsdInput
        );
        key(&mut state, KeyCode::Enter);
        key(&mut state, KeyCode::Right);
        key(&mut state, KeyCode::Enter);

        assert!(matches!(
            state.pending_input_action,
            Some(crate::tui::commands::InputAction::SubmitProviderAuth)
        ));
        let submission = take_provider_auth_secret(&mut state).unwrap();
        assert_eq!(
            submission.mimo_region,
            Some(crate::pricing::MimoRegion::Singapore)
        );
        assert!(submission
            .secret
            .with_secret(crate::pricing::valid_token_plan_key));
        assert!(submission
            .billing
            .as_ref()
            .is_some_and(crate::pricing::ProviderBillingConfig::delegation_ready));
    }

    #[test]
    fn saved_mimo_token_plan_flow_skips_sign_in_and_credential_entry() {
        let mut state = crate::app::AppState::default();
        open_provider_manager(
            &mut state,
            vec![crate::config::ProviderManagementEntry {
                id: "mimo".into(),
                base_url: crate::pricing::MimoRegion::Singapore
                    .token_plan_url()
                    .into(),
                api_key_source: crate::config::ProviderApiKeySource::Keychain,
                configured_models: vec!["mimo-v2.5-pro".into()],
                configured: true,
                connected: true,
            }],
            String::new(),
        );
        begin_provider_auth(
            &mut state,
            crate::provider_connect::FirstClassProvider::Mimo,
        );
        assert_eq!(
            state.provider_auth.as_ref().unwrap().mimo_method_options(),
            [
                crate::app::MimoSetupMethod::SavedTokenPlan,
                crate::app::MimoSetupMethod::Browser,
                crate::app::MimoSetupMethod::ManualTokenPlan,
            ]
        );

        key(&mut state, KeyCode::Enter);
        assert_eq!(
            state.provider_auth.as_ref().unwrap().stage,
            crate::app::ProviderAuthStage::PlanPicker
        );
        assert!(state.pending_input_action.is_none());
        key(&mut state, KeyCode::Down);
        key(&mut state, KeyCode::Down);
        key(&mut state, KeyCode::Enter);
        key(&mut state, KeyCode::Down);
        key(&mut state, KeyCode::Enter);
        assert_eq!(
            state.provider_auth.as_ref().unwrap().stage,
            crate::app::ProviderAuthStage::PlanUsdInput
        );
        assert!(state.provider_auth.as_ref().unwrap().input.is_empty());
        type_text(&mut state, "528");
        key(&mut state, KeyCode::Enter);

        let submission = take_provider_auth_secret(&mut state).unwrap();
        assert_eq!(
            submission.mimo_method,
            Some(crate::app::MimoSetupMethod::SavedTokenPlan)
        );
        assert!(submission.secret.is_empty());
        assert_eq!(
            submission.mimo_region,
            Some(crate::pricing::MimoRegion::Singapore)
        );
        assert!(submission
            .billing
            .as_ref()
            .is_some_and(crate::pricing::ProviderBillingConfig::delegation_ready));
        assert!(matches!(
            state.pending_input_action,
            Some(crate::tui::commands::InputAction::SubmitProviderAuth)
        ));
    }

    #[test]
    fn mimo_setup_cancel_and_failed_retry_return_to_safe_picker_state() {
        let mut state = crate::app::AppState::default();
        begin_provider_auth(
            &mut state,
            crate::provider_connect::FirstClassProvider::Mimo,
        );
        key(&mut state, KeyCode::Esc);
        assert!(state.provider_auth.is_none());
        assert!(matches!(
            state.pending_input_action,
            Some(crate::tui::commands::InputAction::CancelProviderAuth)
        ));

        begin_provider_auth(
            &mut state,
            crate::provider_connect::FirstClassProvider::Mimo,
        );
        state.provider_auth.as_mut().unwrap().stage = crate::app::ProviderAuthStage::Failed;
        key(&mut state, KeyCode::Enter);
        assert_eq!(
            state.provider_auth.as_ref().unwrap().stage,
            crate::app::ProviderAuthStage::MethodPicker
        );
    }

    #[test]
    fn ctrl_y_requests_a_borderless_url_handoff_without_claiming_copy_success() {
        let mut state = crate::app::AppState::default();
        begin_provider_auth(
            &mut state,
            crate::provider_connect::FirstClassProvider::Mimo,
        );
        state.pending_input_action = None;
        let auth = state.provider_auth.as_mut().unwrap();
        auth.stage = crate::app::ProviderAuthStage::SecretInput;
        auth.url = Some("https://platform.xiaomimimo.com/authorize?pk=fixture".into());

        handle_event(
            &mut state,
            &Event::Key(KeyCode::Char('y'), KeyModifiers::CONTROL),
            "/workspace/ca",
        );

        assert!(matches!(
            state.pending_input_action,
            Some(crate::tui::commands::InputAction::CopyProviderAuthUrl(ref url))
                if url == "https://platform.xiaomimimo.com/authorize?pk=fixture"
        ));
        let auth = state.provider_auth.as_ref().unwrap();
        assert_eq!(
            auth.prompt,
            "Opening a borderless authorization URL handoff…"
        );
        assert!(!auth.prompt.contains("copied"));
    }
}

#[cfg(test)]
mod copy_picker_input_tests {
    use super::*;

    fn picker() -> crate::app::CopyPickerState {
        crate::app::CopyPickerState {
            messages: vec![
                crate::app::CopyPickerMessage {
                    role: crate::app::Role::User,
                    summary: "User · first prompt".into(),
                    parts: vec![crate::app::CopyPickerPart {
                        label: "Content".into(),
                        value: "first prompt".into(),
                    }],
                },
                crate::app::CopyPickerMessage {
                    role: crate::app::Role::Assistant,
                    summary: "Assistant · final answer".into(),
                    parts: vec![
                        crate::app::CopyPickerPart {
                            label: "Content".into(),
                            value: "final answer".into(),
                        },
                        crate::app::CopyPickerPart {
                            label: "Visible reasoning".into(),
                            value: "bounded reasoning".into(),
                        },
                    ],
                },
            ],
            level: crate::app::CopyPickerLevel::Messages,
            filter: String::new(),
            selected_idx: 0,
        }
    }

    #[test]
    fn copy_picker_filters_drills_into_parts_and_returns_without_mutating_content() {
        let mut state = crate::app::AppState {
            copy_picker: Some(picker()),
            ..crate::app::AppState::default()
        };
        for character in "assistant".chars() {
            handle_event(
                &mut state,
                &Event::Key(KeyCode::Char(character), KeyModifiers::NONE),
                "/workspace/ca",
            );
        }
        assert_eq!(state.copy_picker.as_ref().unwrap().visible_len(), 1);
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Enter, KeyModifiers::NONE),
            "/workspace/ca",
        );
        let picker = state.copy_picker.as_ref().unwrap();
        assert_eq!(
            picker.level,
            crate::app::CopyPickerLevel::Parts { message_index: 1 }
        );
        assert_eq!(picker.messages[1].parts[1].value, "bounded reasoning");
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Esc, KeyModifiers::NONE),
            "/workspace/ca",
        );
        assert_eq!(
            state.copy_picker.as_ref().unwrap().level,
            crate::app::CopyPickerLevel::Messages
        );
        handle_event(
            &mut state,
            &Event::Key(KeyCode::Esc, KeyModifiers::NONE),
            "/workspace/ca",
        );
        assert!(state.copy_picker.is_none());
    }
}
