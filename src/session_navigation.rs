//! Session-owned navigation and recovery UI requests. Filesystem inspection runs
//! on workers and restoration always consumes an exact, previously shown token.
use crate::app::AppState;
use crate::recovery::{RecoveryContext, RecoveryPreview, RecoveryStore};
use crate::session::SessionStore;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub enum SessionActionRequest {
    RecoveryList,
    RecoveryPreview(Uuid),
    RecoveryRestore(RecoveryPreview),
    History(crate::undo::UndoDirection),
    Family(usize),
    AcceptHandoff(Box<crate::branch_handoff::HandoffPreview>),
}

pub enum SessionActionResult {
    Page(String),
    Preview(RecoveryPreview),
    History {
        history: crate::undo::UndoStack,
        direction: crate::undo::UndoDirection,
    },
    Restored {
        rescue: Uuid,
        writer: crate::session_access::SessionWriterLease,
        warning: Option<String>,
    },
    Handoff(Box<crate::branch_handoff::HandoffPreview>),
    Switched(Box<crate::session::SessionV4>),
}

pub fn request(state: &mut AppState, recovery: bool, arguments: &str) {
    if !state.run_capabilities.session_persistence || state.current_session.is_none() {
        state.status_message = Some("This action requires a saved session.".into());
        return;
    }
    if state.session_action_busy
        || state.undo_busy
        || state.session_load_busy
        || state.checkpoint_in_flight
    {
        state.status_message = Some("Finish the current session operation first.".into());
        return;
    }
    let parts = arguments.split_whitespace().collect::<Vec<_>>();
    let parsed = if recovery {
        match parts.as_slice() {
            [] | ["list"] => Ok(SessionActionRequest::RecoveryList),
            ["preview", id] => Uuid::parse_str(id)
                .map(SessionActionRequest::RecoveryPreview)
                .map_err(|_| "Expected a recorded turn UUID."),
            ["restore", token] => state
                .recovery_preview
                .as_ref()
                .filter(|preview| preview.token == *token)
                .cloned()
                .map(SessionActionRequest::RecoveryRestore)
                .ok_or("Preview this recovery turn and use its exact confirmation token."),
            _ => Err("Usage: /recover [list|preview TURN_UUID|restore TOKEN]"),
        }
    } else {
        match parts.as_slice() {
            [] => Ok(SessionActionRequest::Family(0)),
            ["page", offset] => offset
                .parse::<usize>()
                .map(SessionActionRequest::Family)
                .map_err(|_| "Expected a page offset."),
            ["switch", id] => match Uuid::parse_str(id) {
                Ok(id) => {
                    // The established LoadSession path saves before switching and
                    // checks the loaded identity; the browser never switches itself.
                    state.pending_input_action = Some(
                        crate::tui::commands::InputAction::LoadSession(id.to_string()),
                    );
                    return;
                }
                Err(_) => Err("Expected a session UUID."),
            },
            ["summarize", id] => match Uuid::parse_str(id) {
                Ok(id) => {
                    if state
                        .current_session
                        .as_ref()
                        .is_none_or(|source| source.storage_binding.is_none())
                    {
                        state.status_message =
                            Some("Save the source session before summarizing a branch.".into());
                    } else {
                        state.branch_handoff_request = Some(id);
                    }
                    return;
                }
                Err(_) => Err("Expected a session UUID."),
            },
            ["accept", token] => state
                .branch_handoff_preview
                .as_ref()
                .filter(|preview| preview.token == *token)
                .filter(|preview| {
                    crate::branch_handoff::snapshot_digest(&crate::current_session_snapshot(state))
                        .ok()
                        == crate::branch_handoff::snapshot_digest(&preview.source).ok()
                })
                .cloned()
                .map(|preview| SessionActionRequest::AcceptHandoff(Box::new(preview)))
                .ok_or("Branch summary preview is missing or stale; summarize again."),
            ["cancel"] => {
                state.branch_handoff_preview = None;
                return;
            }
            _ => Err("Usage: /branches [page OFFSET|switch SESSION_UUID]"),
        }
    };
    match parsed {
        Ok(request) => {
            if matches!(request, SessionActionRequest::RecoveryRestore(_)) && relevant_work(state) {
                state.status_message =
                    Some("Stop work in this workspace before restoring recorded edits.".into());
                return;
            }
            state.session_action_request = Some(request);
            state.status_message = Some("Preparing session action…".into());
        }
        Err(error) => state.status_message = Some(error.into()),
    }
}

pub(crate) fn relevant_work(state: &AppState) -> bool {
    let active = |pane: &AppState| {
        let overlaps = pane.workspace.roots().iter().any(|left| {
            state.workspace.roots().iter().any(|right| {
                left.path.starts_with(&right.path) || right.path.starts_with(&left.path)
            })
        });
        (overlaps || pane.pane_id == state.pane_id)
            && (pane.turn_lifecycle.is_active()
                || pane.foreground_job.is_some()
                || pane.undo_busy
                || pane.job_list.iter().any(|job| !job.status.is_terminal())
                || pane.agent_graph_busy
                || pane.agents.as_ref().is_some_and(|manager| {
                    manager
                        .subscribe_activity()
                        .borrow()
                        .iter()
                        .any(|child| !child.status.terminal())
                }))
    };
    state.pane_deck.as_ref().map_or_else(
        || active(state),
        |deck| {
            deck.ids()
                .into_iter()
                .filter_map(|id| deck.pane(state, id))
                .any(active)
        },
    )
}

pub fn start_pending(
    state: &mut AppState,
    store: &SessionStore,
    tx: &tokio::sync::mpsc::UnboundedSender<crate::StreamMsg>,
) {
    let Some(request) = state.session_action_request.take() else {
        return;
    };
    let Some(session_id) = state.current_session.as_ref().map(|session| session.id) else {
        return;
    };
    state.session_action_generation = state.session_action_generation.wrapping_add(1);
    let generation = state.session_action_generation;
    state.session_action_busy = true;
    let roots = state
        .workspace
        .roots()
        .iter()
        .map(|root| root.path.clone())
        .collect::<Vec<_>>();
    let expected = state
        .current_session
        .as_ref()
        .and_then(|session| session.storage_binding.clone());
    let store = store.clone();
    let tx = tx.clone();
    tokio::spawn(async move {
        let result =
            tokio::task::spawn_blocking(move || run(request, &store, session_id, roots, expected))
                .await
                .map_err(|_| "session action worker stopped".to_string())
                .and_then(|result| result);
        let _ = tx.send(crate::StreamMsg::SessionActionReady {
            session_id,
            generation,
            result: Box::new(result),
        });
    });
}

fn run(
    request: SessionActionRequest,
    store: &SessionStore,
    session_id: Uuid,
    roots: Vec<std::path::PathBuf>,
    expected: Option<crate::session::SessionBinding>,
) -> Result<SessionActionResult, String> {
    if let SessionActionRequest::AcceptHandoff(preview) = request {
        store
            .prepare_identity_inventory()
            .map_err(|error| format!("Branch ownership preparation failed: {error}"))?;
        return crate::branch_handoff::accept(store, *preview)
            .map(|target| SessionActionResult::Switched(Box::new(target)));
    }
    if let SessionActionRequest::Family(offset) = request {
        let metadata = store.family_metadata().map_err(|error| error.to_string())?;
        let page = crate::branches::family_page(&metadata, session_id, offset, 25);
        let mut lines = vec![
            format!(
                "Branches · {} sessions · offset {}",
                page.total, page.offset
            ),
            String::new(),
        ];
        for row in page.rows {
            lines.push(format!(
                "{}{:?} · {} · {}{}",
                "  ".repeat(row.depth.min(12)),
                row.relation,
                row.metadata.summary.id,
                row.metadata.summary.title,
                if row.metadata.archived {
                    " [archived]"
                } else {
                    ""
                }
            ));
            if let Some(parent) = row.metadata.parent {
                lines.push(format!(
                    "  parent {} · branch turn {}",
                    parent.session_id, parent.branch_turn
                ));
            }
        }
        for issue in page.issues {
            lines.push(format!("Unavailable ancestry: {issue:?}"));
        }
        if let Some(next) = page.next_offset {
            lines.push(format!("\nNext page: /branches page {next}"));
        }
        lines.push("\nSwitch: /branches switch SESSION_UUID · archived sessions can be restored in /resume".into());
        return Ok(SessionActionResult::Page(lines.join("\n")));
    }
    let recovery = Arc::new(RecoveryStore::new(store.access_coordinator().scope()));
    if matches!(request, SessionActionRequest::RecoveryList) {
        let turns = recovery
            .list(session_id)
            .map_err(|error| error.to_string())?;
        let mut lines = vec!["Recorded turn edits".into(), "Typed file edits only. Shell, MCP, foreign execution and unsaved sessions are outside coverage.".into(), String::new()];
        for turn in turns {
            lines.push(format!(
                "{} · {} files · {}{}{}",
                turn.turn_id,
                turn.file_count,
                if turn.completed {
                    "completed"
                } else {
                    "pending"
                },
                if turn.incomplete {
                    " · incomplete coverage"
                } else {
                    ""
                },
                if turn.conflicts { " · conflict" } else { "" }
            ));
        }
        lines.push("\nPreview: /recover preview TURN_UUID".into());
        return Ok(SessionActionResult::Page(lines.join("\n")));
    }
    store
        .prepare_identity_inventory()
        .map_err(|error| format!("Recovery ownership preparation failed: {error}"))?;
    let lease = store
        .try_writer_lease(session_id)
        .map_err(|error| error.to_string())?;
    if expected.as_ref()
        != Some(
            &store
                .committed_binding(session_id)
                .map_err(|error| error.to_string())?,
        )
    {
        return Err(
            "Saved session changed; reload before inspecting or restoring its recorded edits."
                .into(),
        );
    }
    let context = RecoveryContext::new(
        recovery.clone(),
        session_id,
        Uuid::new_v4(),
        roots,
        "human_recovery",
        lease,
    );
    match request {
        SessionActionRequest::History(direction) => {
            recovery
                .reconcile(&context)
                .map_err(|error| error.to_string())?;
            recovery
                .load_undo_stack(&context)
                .map(|history| SessionActionResult::History { history, direction })
                .map_err(|error| error.to_string())
        }
        SessionActionRequest::RecoveryPreview(turn) => {
            recovery
                .reconcile(&context)
                .map_err(|error| error.to_string())?;
            recovery
                .preview(&context, turn)
                .map(SessionActionResult::Preview)
                .map_err(|error| error.to_string())
        }
        SessionActionRequest::RecoveryRestore(preview) => recovery
            .restore_with_outcome(&context, &preview)
            .map(|restored| SessionActionResult::Restored {
                rescue: restored.rescue_turn,
                writer: context.writer.clone(),
                warning: restored.warning,
            })
            .map_err(|error| error.to_string()),
        _ => unreachable!(),
    }
}

pub fn apply(state: &mut AppState, result: SessionActionResult) {
    match result {
        SessionActionResult::History { history, direction } => {
            state.undo_stack = history;
            if state.undo_stack.prepare(direction).is_some() {
                state.undo_request = Some(direction);
                state.status_message = Some("Restoring recorded file history…".into());
            } else {
                state.status_message = Some(
                    match direction {
                        crate::undo::UndoDirection::Undo => "Nothing to undo",
                        crate::undo::UndoDirection::Redo => "Nothing to redo",
                    }
                    .into(),
                );
            }
            return;
        }
        SessionActionResult::Page(text) => state.pager_content = text,
        SessionActionResult::Preview(preview) => {
            state.pager_content = format!("Restore recorded turn {}\n\n{}\n\n{}\n\n{}{}\nConfirmation: /recover restore {}\nThe token binds the exact file hashes, workspace identity and journal revision. Restoration preserves the transcript and records a rescue turn for redo.",
                preview.turn_id, preview.files.join("\n"), if preview.incomplete { "Coverage is incomplete; only these captured edits can be restored." } else { "These files return to their first state before this turn." },
                preview.changes_text, if preview.changes_truncated { "\nPreview truncated; the displayed hashes bind the complete saved contents.\n" } else { "" }, preview.token);
            state.recovery_preview = Some(preview);
        }
        SessionActionResult::Restored {
            rescue,
            writer,
            warning,
        } => {
            state.session_writer = Some(writer);
            state.recovery_preview = None;
            state.outcome_tracker.invalidate_verification();
            state.mechanistic_verification_context = None;
            state
                .work_state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .invalidate_verification_after_recovery();
            state.pager_content = format!("Recorded edits restored. Transcript preserved. Verification evidence invalidated.\n\nRedo preview: /recover preview {rescue}");
            state.status_message = Some(warning.map_or_else(
                || "Recorded edits restored; verification needs to run again.".into(),
                |warning| format!("Recorded edits restored. {warning}"),
            ));
        }
        SessionActionResult::Handoff(preview) => {
            state.pager_content = format!("Summarize and switch preview\nSource {} → target {}\n\n{}\n\nAccept and switch: /branches accept {}\nCancel: /branches cancel", preview.source.id, preview.target.id, preview.summary, preview.token);
            state.branch_handoff_preview = Some(*preview);
        }
        SessionActionResult::Switched(_) => {
            unreachable!("switches use the established main session lifecycle")
        }
    }
    state.pager_scroll = 0;
    state.pager_open = true;
}

pub fn bookmark(state: &mut AppState, store: &SessionStore, arguments: &str) {
    let result = bookmark_inner(state, store, arguments);
    if let Err(error) = result {
        state.status_message = Some(format!("Bookmark action failed: {error}"));
    }
}

fn bookmark_inner(
    state: &mut AppState,
    store: &SessionStore,
    arguments: &str,
) -> Result<(), String> {
    if state.turn_lifecycle.is_active() || state.session_action_busy {
        return Err("Finish current work first.".into());
    }
    if state
        .session_history
        .as_ref()
        .is_some_and(|history| !history.is_complete())
    {
        return Err("Load the complete history before editing bookmark anchors.".into());
    }
    crate::ensure_session_identity(state);
    let current = state
        .current_session
        .as_ref()
        .ok_or("No session is available.")?;
    let (command, rest) = arguments
        .trim()
        .split_once(char::is_whitespace)
        .unwrap_or((arguments.trim(), ""));
    if command.is_empty() || command == "list" {
        state.pager_content = format!(
            "Bookmarks\n\n{}\n\n/bookmark add LABEL · /bookmark branch UUID",
            current
                .bookmarks
                .entries
                .iter()
                .map(|entry| format!(
                    "{} · {} · message boundary {}{}",
                    entry.id,
                    entry.label,
                    entry.through_message_count,
                    if entry.matches_history(&state.messages) {
                        ""
                    } else {
                        " [stale]"
                    }
                ))
                .collect::<Vec<_>>()
                .join("\n")
        );
        state.pager_scroll = 0;
        state.pager_open = true;
        return Ok(());
    }
    if command == "branch" {
        let id = Uuid::parse_str(rest).map_err(|_| "Expected a bookmark UUID.")?;
        let entry = current
            .bookmarks
            .entries
            .iter()
            .find(|entry| entry.id == id)
            .ok_or("Unknown bookmark.")?;
        if !entry.matches_history(&state.messages) {
            return Err("Bookmark history changed; this anchor is stale.".into());
        }
        let boundary = entry.through_message_count;
        let turn = crate::session::completed_turns(&state.messages[..boundary]).len();
        return crate::branch_current_session(state, store, boundary, turn, None)
            .map_err(|error| error.to_string());
    }
    let previous = current.bookmarks.clone();
    let current = state
        .current_session
        .as_mut()
        .ok_or("No session is available.")?;
    let result = match command {
        "add" => current
            .bookmarks
            .add(&state.messages, state.messages.len(), rest)
            .map(|_| ()),
        "rename" => {
            let (id, label) = rest
                .split_once(char::is_whitespace)
                .ok_or("Expected UUID and label.")?;
            let id = Uuid::parse_str(id).map_err(|_| "Expected a bookmark UUID.")?;
            current.bookmarks.rename(id, label)
        }
        "delete" => current
            .bookmarks
            .remove(Uuid::parse_str(rest).map_err(|_| "Expected a bookmark UUID.")?),
        _ => Err(
            "Usage: /bookmark [list|add LABEL|rename UUID LABEL|delete UUID|branch UUID]".into(),
        ),
    };
    result?;
    if let Err(error) = crate::save_current_session(state, store) {
        if let Some(current) = state.current_session.as_mut() {
            current.bookmarks = previous;
        }
        return Err(error.to_string());
    }
    state.status_message = Some("Bookmark saved.".into());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state() -> AppState {
        let mut state = AppState::new(crate::context::ShellHistory::in_memory());
        state.current_session = Some(crate::session::SessionV4::new(
            "Fixture",
            "/workspace",
            "local",
            "fixture",
            Vec::new(),
        ));
        state
    }
    #[test]
    fn restore_requires_the_exact_shown_token_and_keeps_transcript() {
        let mut state = state();
        let before = state.messages.len();
        request(&mut state, true, "restore guessed-token");
        assert!(state.session_action_request.is_none());
        assert_eq!(state.messages.len(), before);
        assert!(state.status_message.unwrap().contains("Preview"));
    }
    #[test]
    fn tree_selection_uses_uuid_and_preview_does_not_switch() {
        let mut state = state();
        let source = state.current_session.as_ref().unwrap().id;
        request(&mut state, false, "");
        assert!(matches!(
            state.session_action_request,
            Some(SessionActionRequest::Family(0))
        ));
        assert_eq!(state.current_session.as_ref().unwrap().id, source);
        let target = Uuid::new_v4();
        request(&mut state, false, &format!("switch {target}"));
        assert_eq!(
            state.pending_input_action,
            Some(crate::tui::commands::InputAction::LoadSession(
                target.to_string()
            ))
        );
        assert_eq!(state.current_session.as_ref().unwrap().id, source);
    }
    #[test]
    fn relevant_work_prevents_restore_in_a_materialized_pane() {
        let mut state = state();
        state.turn_lifecycle.begin();
        assert!(relevant_work(&state));
        state.turn_lifecycle.complete();
        assert!(!relevant_work(&state));
    }
}
