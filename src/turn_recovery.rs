//! Process-local, bounded recovery policy. Never serialize requests or credentials.
use crate::agent::stream::{ProviderError, ProviderTransportFailure};
use std::collections::VecDeque;

const MAX_FAILURES: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Category {
    Timeout,
    Connect,
    Transport,
    Http(u16),
    PrematureEof,
    Authentication,
    Protocol,
    Incomplete,
    Refused,
}

impl Category {
    pub(crate) fn of(error: &ProviderError) -> Self {
        match error {
            ProviderError::Transport(ProviderTransportFailure::Timeout) => Self::Timeout,
            ProviderError::Transport(ProviderTransportFailure::Connect) => Self::Connect,
            ProviderError::Transport(_) => Self::Transport,
            ProviderError::Http { status } => Self::Http(*status),
            ProviderError::PrematureEof => Self::PrematureEof,
            ProviderError::MissingEnvironment(_) => Self::Authentication,
            ProviderError::ResponseRefused => Self::Refused,
            ProviderError::IncompleteFinish(_)
            | ProviderError::ResponseIncomplete
            | ProviderError::OutputTokenLimit
            | ProviderError::ResponseFailed => Self::Incomplete,
            _ => Self::Protocol,
        }
    }

    pub(crate) fn retryable(self) -> bool {
        matches!(
            self,
            Self::Timeout
                | Self::Connect
                | Self::Transport
                | Self::PrematureEof
                | Self::Http(429 | 500 | 502 | 503 | 504)
        )
    }

    pub(crate) fn label(self) -> String {
        match self {
            Self::Timeout => "request timeout".into(),
            Self::Connect => "connection failed".into(),
            Self::Transport => "transport failed".into(),
            Self::Http(status) => format!("HTTP {status}"),
            Self::PrematureEof => "stream ended early".into(),
            Self::Authentication => "authentication unavailable".into(),
            Self::Protocol => "invalid provider stream".into(),
            Self::Incomplete => "incomplete provider response".into(),
            Self::Refused => "provider refused response".into(),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct FrozenRequest {
    pub client: crate::agent::AgentClient,
    pub prepared: crate::request::PreparedModelRequest,
}

#[derive(Debug, Clone)]
pub(crate) struct ContinuationRequest {
    pub frozen: FrozenRequest,
    pub generation: u64,
    pub parent_turn: uuid::Uuid,
    pub message_count: usize,
    pub batch_timestamp: chrono::DateTime<chrono::Utc>,
}

pub(crate) struct Context {
    pub user_index: usize,
    pub user_timestamp: Option<chrono::DateTime<chrono::Utc>>,
    pub generation: u64,
    pub attempt: u8,
    pub effects_started: bool,
    pub frozen: Option<FrozenRequest>,
    pub prompt: Option<crate::app::ActiveTurnPromptState>,
    pub attachments: Option<String>,
}

impl std::fmt::Debug for Context {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RecoveryContext")
            .field("user_index", &self.user_index)
            .field("generation", &self.generation)
            .field("attempt", &self.attempt)
            .field("effects_started", &self.effects_started)
            .field("request_prepared", &self.frozen.is_some())
            .finish_non_exhaustive()
    }
}

impl Context {
    pub(crate) fn reserve_replacement(&mut self, category: Category) -> bool {
        if !category.retryable()
            || self.effects_started
            || self.attempt != 0
            || self.frozen.is_none()
        {
            return false;
        }
        // Reserve before the delay/branch. Even a persistence failure cannot re-arm this turn.
        self.attempt = 1;
        true
    }
}

#[derive(Debug)]
struct Failure {
    time: chrono::DateTime<chrono::Utc>,
    route: String,
    category: Category,
    resolution: &'static str,
    save_error: Option<String>,
}

#[derive(Debug, Default)]
pub(crate) struct State {
    pub context: Option<Context>,
    pub pending: bool,
    pub before_submit_effects: bool,
    pub notice: Option<String>,
    pub continuation: Option<ContinuationRequest>,
    pub continuation_retry_used: bool,
    failures: VecDeque<Failure>,
}

fn safe_label(value: &str) -> String {
    crate::terminal_text::sanitize_terminal_text(value)
        .chars()
        .map(|char| if char.is_control() { ' ' } else { char })
        .take(96)
        .collect()
}

impl State {
    pub(crate) fn begin(&mut self, user_index: Option<usize>, generation: u64) {
        self.pending = false;
        self.notice = None;
        self.continuation = None;
        self.continuation_retry_used = false;
        self.context = user_index.map(|user_index| Context {
            user_index,
            user_timestamp: None,
            generation,
            attempt: 0,
            effects_started: std::mem::take(&mut self.before_submit_effects),
            frozen: None,
            prompt: None,
            attachments: None,
        });
        self.before_submit_effects = false;
    }

    pub(crate) fn effects_started(&mut self) {
        if let Some(context) = &mut self.context {
            context.effects_started = true;
        }
    }

    pub(crate) fn cancel(&mut self) {
        self.context = None;
        self.pending = false;
        self.before_submit_effects = false;
        self.continuation = None;
        self.resolve("cancelled");
    }

    pub(crate) fn record(&mut self, provider: &str, model: &str, category: Category) {
        if self.failures.len() == MAX_FAILURES {
            self.failures.pop_front();
        }
        self.failures.push_back(Failure {
            time: chrono::Utc::now(),
            route: format!("{}/{}", safe_label(provider), safe_label(model)),
            category,
            resolution: "manual retry available",
            save_error: None,
        });
    }

    pub(crate) fn resolve(&mut self, resolution: &'static str) {
        if let Some(last) = self.failures.back_mut() {
            last.resolution = resolution;
        }
    }

    /// Only local persistence errors enter here, never provider response bodies.
    pub(crate) fn save_failed(&mut self, error: &str) {
        let error = safe_label(error);
        self.pending = false;
        self.resolve("save failed; no replacement sent");
        if let Some(last) = self.failures.back_mut() {
            last.save_error = Some(error.clone());
        }
        self.notice = Some(format!(
            "Recovery stopped: {error}. No replacement sent. /errors · /retry"
        ));
    }

    pub(crate) fn view(&self) -> String {
        if self.failures.is_empty() {
            return "No provider failures in this pane.\n".into();
        }
        let mut output = "Recent provider failures (this pane, newest first)\n\n".to_string();
        for failure in self.failures.iter().rev() {
            output.push_str(&format!(
                "{} · {} · {} · {}\n",
                failure.time.format("%H:%M:%S UTC"),
                failure.route,
                failure.category.label(),
                failure.resolution
            ));
            if let Some(error) = &failure.save_error {
                output.push_str(&format!("  {error}\n"));
            }
        }
        output.push_str("\nProvider bodies, prompts and credentials are not retained here.\n/errors dismiss clears the footer; /retry creates a manual branch.\n");
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn permanent_and_ambiguous_failures_do_not_retry() {
        assert_eq!(
            Category::of(&ProviderError::OutputTokenLimit),
            Category::of(&ProviderError::ResponseIncomplete)
        );
        for error in [
            ProviderError::Http { status: 400 },
            ProviderError::Http { status: 401 },
            ProviderError::Http { status: 403 },
            ProviderError::Protocol("secret".into()),
            ProviderError::ResponseIncomplete,
            ProviderError::OutputTokenLimit,
            ProviderError::ResponseRefused,
        ] {
            assert!(!Category::of(&error).retryable());
            assert!(!Category::of(&error).label().contains("secret"));
        }
        assert!(Category::of(&ProviderError::PrematureEof).retryable());
        assert!(Category::Http(503).retryable());
    }

    #[test]
    fn recent_failures_are_bounded_and_body_free() {
        let mut state = State::default();
        for _ in 0..100 {
            state.record("test", "model", Category::Protocol);
        }
        assert_eq!(state.failures.len(), MAX_FAILURES);
        state.resolve("recovered");
        assert!(state.view().contains("recovered"));
    }

    #[test]
    fn save_diagnostics_are_bounded_and_terminal_safe() {
        let mut state = State::default();
        state.record("test", "model", Category::PrematureEof);
        state.pending = true;
        state.save_failed(&format!(
            "branch save: permission denied\x1b[31m\n{}",
            "x".repeat(500)
        ));
        assert!(!state.pending);
        let detail = state.failures.back().unwrap().save_error.as_ref().unwrap();
        assert!(detail.chars().count() <= 96);
        assert!(!detail.chars().any(char::is_control));
        assert!(state.view().contains("branch save: permission denied"));
        state.resolve("cancelled");
        assert!(state.view().contains("branch save: permission denied"));
    }
}
