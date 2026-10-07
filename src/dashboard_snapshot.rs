//! Immutable, bounded dashboard observations. Capture never changes focus or authority.
use crate::agent::subagent::AgentStatus;
use crate::app::{AppState, Role};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const MAX_PREVIEW_BYTES: usize = 2048;
pub const MAX_LIVE_ENTRIES: usize = 64;

pub fn instance_id() -> Uuid {
    static INSTANCE: std::sync::LazyLock<Uuid> = std::sync::LazyLock::new(Uuid::new_v4);
    *INSTANCE
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DashboardIdentity {
    pub instance_id: Uuid,
    pub pane_id: u8,
    pub session_id: Option<Uuid>,
    pub child_id: Option<Uuid>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LiveStatus {
    NeedsInput,
    Working,
    Idle,
    Completed,
    Failed,
    Cancelled,
    Stale,
    Unavailable,
}

impl LiveStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::NeedsInput => "Needs input",
            Self::Working => "Working",
            Self::Idle => "Idle",
            Self::Completed => "Completed",
            Self::Failed => "Failed",
            Self::Cancelled => "Cancelled",
            Self::Stale => "Stale",
            Self::Unavailable => "Unavailable",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DashboardEntry {
    pub identity: DashboardIdentity,
    pub status: LiveStatus,
    pub title: String,
    pub workspace: String,
    pub activity: String,
    pub preview: String,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub parent_turn: Option<Uuid>,
    pub external: bool,
    pub counters: ActivityCounters,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActivityCounters {
    pub messages: u32,
    pub active_children: u16,
    pub pending_inputs: u16,
}

pub fn bounded_text(text: &str, max_bytes: usize) -> String {
    // Bound the sanitizer's input too: previews may refer to very long live messages.
    let mut input_end = text
        .len()
        .min(max_bytes.saturating_mul(4).saturating_add(64));
    while !text.is_char_boundary(input_end) {
        input_end -= 1;
    }
    let text = crate::terminal_text::sanitize_terminal_text(&text[..input_end]);
    if text.len() <= max_bytes {
        return text;
    }
    let mut end = max_bytes;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

pub fn capture_local(state: &AppState) -> Vec<DashboardEntry> {
    let mut rows = Vec::new();
    if let Some(deck) = &state.pane_deck {
        for id in deck.ids() {
            if let Some(pane) = deck.pane(state, id) {
                capture_pane(pane, &mut rows);
            }
        }
    } else {
        capture_pane(state, &mut rows);
    }
    rows.truncate(MAX_LIVE_ENTRIES);
    rows
}

fn capture_pane(state: &AppState, rows: &mut Vec<DashboardEntry>) {
    let session_id = state.current_session.as_ref().map(|session| session.id);
    let identity = DashboardIdentity {
        instance_id: instance_id(),
        pane_id: state.pane_id.get(),
        session_id,
        child_id: None,
    };
    let status = if state.approval_queue.current().is_some() || state.interaction.is_some() {
        LiveStatus::NeedsInput
    } else if state.turn_lifecycle.is_active()
        || state.streaming.is_some()
        || state.approval_queue.is_active()
        || state.session_access_in_flight
        || state.memory_turn_in_flight
        || state.pending_composer_submit
        || state.session_load_busy
        || state.session_action_busy
        || state.hook_running
        || state.review_in_flight
        || state.attachment_resolution_busy
        || state.agent_graph_busy
        || state.worktree_busy
        || state.checkpoint_in_flight
        || state.undo_busy
        || state.foreground_job.is_some()
        || state.job_list.iter().any(|job| !job.status.is_terminal())
    {
        LiveStatus::Working
    } else {
        match state.turn_lifecycle.outcome {
            crate::activity::TurnOutcome::Completed => LiveStatus::Completed,
            crate::activity::TurnOutcome::Failed => LiveStatus::Failed,
            crate::activity::TurnOutcome::Cancelled => LiveStatus::Cancelled,
            _ => LiveStatus::Idle,
        }
    };
    let preview = state
        .streaming
        .as_ref()
        .map(|stream| stream.content.as_str())
        .filter(|text| !text.is_empty())
        .or_else(|| {
            state
                .messages
                .iter()
                .rev()
                .find(|message| {
                    matches!(message.role, Role::Assistant | Role::User)
                        && !message.content.trim().is_empty()
                })
                .map(|message| message.content.as_str())
        })
        .unwrap_or("");
    let title = if !state.session_name.trim().is_empty() {
        state.session_name.clone()
    } else {
        format!("Pane {}", state.pane_id)
    };
    rows.push(DashboardEntry {
        identity: identity.clone(),
        status,
        title: bounded_text(&title, 128),
        workspace: bounded_text(&state.cwd, 1024),
        activity: status.label().into(),
        preview: bounded_text(preview, MAX_PREVIEW_BYTES),
        updated_at: state
            .current_session
            .as_ref()
            .map(|session| session.updated_at)
            .unwrap_or_else(chrono::Utc::now),
        parent_turn: state.active_agent_turn_id,
        external: false,
        counters: ActivityCounters {
            messages: state.messages.len().min(u32::MAX as usize) as u32,
            active_children: state
                .live_agent_activity
                .iter()
                .take(512)
                .filter(|child| !child.status.terminal())
                .count() as u16,
            pending_inputs: u16::from(status == LiveStatus::NeedsInput),
        },
    });
    for child in &state.live_agent_activity {
        if rows.len() >= MAX_LIVE_ENTRIES {
            break;
        }
        let status = match child.status {
            AgentStatus::WaitingApproval => LiveStatus::NeedsInput,
            AgentStatus::Queued | AgentStatus::Running => LiveStatus::Working,
            AgentStatus::Completed => LiveStatus::Completed,
            AgentStatus::Failed | AgentStatus::TimedOut => LiveStatus::Failed,
            AgentStatus::Cancelled | AgentStatus::Interrupted => LiveStatus::Cancelled,
        };
        rows.push(DashboardEntry {
            identity: DashboardIdentity {
                child_id: Some(child.id.0),
                ..identity.clone()
            },
            status,
            title: bounded_text(&child.task, 128),
            workspace: bounded_text(&state.cwd, 1024),
            activity: bounded_text(
                child.current_action.as_deref().unwrap_or(status.label()),
                128,
            ),
            preview: bounded_text(
                child
                    .result_summary
                    .as_deref()
                    .unwrap_or(&child.recent_output),
                MAX_PREVIEW_BYTES,
            ),
            updated_at: child.updated_at,
            parent_turn: Some(child.parent_turn),
            external: false,
            counters: ActivityCounters {
                pending_inputs: u16::from(status == LiveStatus::NeedsInput),
                ..Default::default()
            },
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preview_is_utf8_bounded_and_capture_never_changes_owner_focus() {
        let mut state = AppState::new(crate::context::ShellHistory::new());
        state.session_name = "session".into();
        state.messages.push(crate::app::Message {
            role: Role::Assistant,
            content: "é".repeat(4096),
            tool_calls: None,
            thinking: None,
            response_items: Vec::new(),
            timestamp: chrono::Utc::now(),
        });
        let owner = state.pane_id;
        let rows = capture_local(&state);
        assert_eq!(state.pane_id, owner);
        assert_eq!(rows[0].identity.pane_id, owner.get());
        assert!(rows[0].preview.len() <= MAX_PREVIEW_BYTES);
        assert_eq!(rows[0].status, LiveStatus::Idle);
    }

    #[test]
    fn parent_terminal_states_and_preparation_keep_needs_input_precedence() {
        let mut state = AppState::new(crate::context::ShellHistory::new());
        for (outcome, expected) in [
            (
                crate::activity::TurnOutcome::Completed,
                LiveStatus::Completed,
            ),
            (crate::activity::TurnOutcome::Failed, LiveStatus::Failed),
            (
                crate::activity::TurnOutcome::Cancelled,
                LiveStatus::Cancelled,
            ),
        ] {
            state.turn_lifecycle.outcome = outcome;
            assert_eq!(capture_local(&state)[0].status, expected);
        }
        state.session_access_in_flight = true;
        assert_eq!(capture_local(&state)[0].status, LiveStatus::Working);
        state.session_access_in_flight = false;
        state.memory_turn_in_flight = true;
        assert_eq!(capture_local(&state)[0].status, LiveStatus::Working);
        state.memory_turn_in_flight = false;
        for busy in 0..5 {
            state.hook_running = busy == 0;
            state.review_in_flight = busy == 1;
            state.attachment_resolution_busy = busy == 2;
            state.agent_graph_busy = busy == 3;
            state.worktree_busy = busy == 4;
            assert_eq!(capture_local(&state)[0].status, LiveStatus::Working);
        }
        state.worktree_busy = false;
        state.foreground_job = Some(crate::jobs::JobId::new());
        assert_eq!(capture_local(&state)[0].status, LiveStatus::Working);
    }
}
