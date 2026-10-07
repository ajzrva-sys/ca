#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityPhase {
    Idle,
    Sending,
    Waiting,
    Reasoning,
    Responding,
    Tool,
    Interaction,
    Paused,
}

impl std::fmt::Display for ActivityPhase {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Idle => "Idle",
            Self::Sending => "Sending",
            Self::Waiting => "Waiting",
            Self::Reasoning => "Reasoning",
            Self::Responding => "Responding",
            Self::Tool => "Tool",
            Self::Interaction => "Interaction",
            Self::Paused => "Paused",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoftPauseBoundary {
    Tools,
    Provider,
    Finalization,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SoftPauseState {
    #[default]
    Inactive,
    Requested,
    Paused(SoftPauseBoundary),
    ResumeRequested(SoftPauseBoundary),
}

impl SoftPauseState {
    pub fn request(&mut self, active: bool) -> SoftPauseRequestOutcome {
        if !active {
            return SoftPauseRequestOutcome::Idle;
        }
        match *self {
            Self::Inactive => {
                *self = Self::Requested;
                SoftPauseRequestOutcome::Requested
            }
            Self::Requested => SoftPauseRequestOutcome::AlreadyRequested,
            Self::Paused(boundary) | Self::ResumeRequested(boundary) => {
                SoftPauseRequestOutcome::AlreadyPaused(boundary)
            }
        }
    }

    pub fn reach(&mut self, boundary: SoftPauseBoundary) -> bool {
        if *self != Self::Requested {
            return false;
        }
        *self = Self::Paused(boundary);
        true
    }

    pub fn pause_at(&mut self, boundary: SoftPauseBoundary) {
        *self = Self::Paused(boundary);
    }

    pub fn request_resume(&mut self) -> SoftPauseResumeOutcome {
        match *self {
            Self::Inactive => SoftPauseResumeOutcome::NotPaused,
            Self::Requested => SoftPauseResumeOutcome::StillRunning,
            Self::Paused(boundary) => {
                *self = Self::ResumeRequested(boundary);
                SoftPauseResumeOutcome::Requested(boundary)
            }
            Self::ResumeRequested(boundary) => SoftPauseResumeOutcome::AlreadyRequested(boundary),
        }
    }

    pub fn take_resume(&mut self) -> Option<SoftPauseBoundary> {
        let Self::ResumeRequested(boundary) = *self else {
            return None;
        };
        *self = Self::Inactive;
        Some(boundary)
    }

    pub fn clear(&mut self) {
        *self = Self::Inactive;
    }

    pub fn is_paused(self) -> bool {
        matches!(self, Self::Paused(_) | Self::ResumeRequested(_))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoftPauseRequestOutcome {
    Idle,
    Requested,
    AlreadyRequested,
    AlreadyPaused(SoftPauseBoundary),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoftPauseResumeOutcome {
    NotPaused,
    StillRunning,
    Requested(SoftPauseBoundary),
    AlreadyRequested(SoftPauseBoundary),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnOutcome {
    None,
    InProgress,
    Completed,
    Failed,
    Cancelled,
}

pub const NOTIFICATION_THRESHOLD: std::time::Duration = std::time::Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TurnCompletion {
    pub outcome: TurnOutcome,
    pub duration: std::time::Duration,
    pub was_paused: bool,
}

impl TurnCompletion {
    pub fn should_notify(self) -> bool {
        matches!(self.outcome, TurnOutcome::Completed | TurnOutcome::Failed)
            && self.duration >= NOTIFICATION_THRESHOLD
            && !self.was_paused
    }
}

#[derive(Debug, Clone)]
pub struct TurnLifecycle {
    pub generation: u64,
    pub phase: ActivityPhase,
    pub outcome: TurnOutcome,
    pub turn_started_at: Option<std::time::Instant>,
    pub phase_started_at: Option<std::time::Instant>,
    completion: Option<TurnCompletion>,
    notification_suppressed: bool,
}

impl Default for TurnLifecycle {
    fn default() -> Self {
        Self {
            generation: 0,
            phase: ActivityPhase::Idle,
            outcome: TurnOutcome::None,
            turn_started_at: None,
            phase_started_at: None,
            completion: None,
            notification_suppressed: false,
        }
    }
}

impl TurnLifecycle {
    pub fn begin(&mut self) {
        let now = std::time::Instant::now();
        self.generation = self.generation.wrapping_add(1).max(1);
        self.phase = ActivityPhase::Sending;
        self.outcome = TurnOutcome::InProgress;
        self.turn_started_at = Some(now);
        self.phase_started_at = Some(now);
        self.completion = None;
        self.notification_suppressed = false;
    }

    pub fn set_phase(&mut self, phase: ActivityPhase) {
        if self.outcome != TurnOutcome::InProgress || self.phase == phase {
            return;
        }
        self.phase = phase;
        self.phase_started_at = Some(std::time::Instant::now());
        if phase == ActivityPhase::Paused {
            self.notification_suppressed = true;
        }
    }

    pub fn complete(&mut self) {
        self.finish(TurnOutcome::Completed);
    }

    pub fn fail(&mut self) {
        self.finish(TurnOutcome::Failed);
    }

    pub fn cancel(&mut self) {
        self.finish(TurnOutcome::Cancelled);
    }

    pub fn reset(&mut self) {
        let generation = self.generation;
        *self = Self::default();
        self.generation = generation;
    }

    pub fn is_active(&self) -> bool {
        self.outcome == TurnOutcome::InProgress
    }

    pub fn phase_elapsed(&self) -> Option<std::time::Duration> {
        self.phase_started_at.map(|started| started.elapsed())
    }

    pub fn take_completion(&mut self) -> Option<TurnCompletion> {
        self.completion.take()
    }

    fn finish(&mut self, outcome: TurnOutcome) {
        if self.outcome != TurnOutcome::InProgress {
            return;
        }
        let was_paused = self.notification_suppressed;
        let duration = self
            .turn_started_at
            .take()
            .map_or(std::time::Duration::ZERO, |started| started.elapsed());
        self.phase = ActivityPhase::Idle;
        self.outcome = outcome;
        self.phase_started_at = None;
        self.completion = Some(TurnCompletion {
            outcome,
            duration,
            was_paused,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_keeps_phase_and_terminal_outcome_distinct() {
        let mut lifecycle = TurnLifecycle::default();
        assert_eq!(lifecycle.outcome, TurnOutcome::None);
        lifecycle.begin();
        assert!(lifecycle.is_active());
        assert_eq!(lifecycle.phase, ActivityPhase::Sending);
        for phase in [
            ActivityPhase::Waiting,
            ActivityPhase::Reasoning,
            ActivityPhase::Responding,
            ActivityPhase::Tool,
            ActivityPhase::Interaction,
            ActivityPhase::Paused,
        ] {
            lifecycle.set_phase(phase);
            assert_eq!(lifecycle.phase, phase);
        }
        lifecycle.complete();
        assert!(!lifecycle.is_active());
        assert_eq!(lifecycle.phase, ActivityPhase::Idle);
        assert_eq!(lifecycle.outcome, TurnOutcome::Completed);
        let completion = lifecycle.take_completion().unwrap();
        assert_eq!(completion.outcome, TurnOutcome::Completed);
        assert!(completion.was_paused);
        lifecycle.reset();
        assert_eq!(lifecycle.outcome, TurnOutcome::None);
    }

    #[test]
    fn notification_threshold_is_inclusive_and_suppresses_cancel_or_pause() {
        let completed = TurnCompletion {
            outcome: TurnOutcome::Completed,
            duration: std::time::Duration::from_secs(30),
            was_paused: false,
        };
        assert!(completed.should_notify());
        assert!(!TurnCompletion {
            duration: std::time::Duration::from_millis(29_999),
            ..completed
        }
        .should_notify());
        assert!(!TurnCompletion {
            outcome: TurnOutcome::Cancelled,
            ..completed
        }
        .should_notify());
        assert!(!TurnCompletion {
            was_paused: true,
            ..completed
        }
        .should_notify());

        let mut lifecycle = TurnLifecycle::default();
        lifecycle.begin();
        lifecycle.turn_started_at =
            std::time::Instant::now().checked_sub(std::time::Duration::from_secs(31));
        lifecycle.set_phase(ActivityPhase::Paused);
        lifecycle.set_phase(ActivityPhase::Responding);
        lifecycle.complete();
        assert!(lifecycle.take_completion().unwrap().was_paused);
    }

    #[test]
    fn soft_pause_reaches_and_resumes_one_typed_boundary() {
        let mut pause = SoftPauseState::default();
        assert_eq!(pause.request(false), SoftPauseRequestOutcome::Idle);
        assert_eq!(pause.request(true), SoftPauseRequestOutcome::Requested);
        assert_eq!(
            pause.request(true),
            SoftPauseRequestOutcome::AlreadyRequested
        );
        assert!(pause.reach(SoftPauseBoundary::Tools));
        assert!(pause.is_paused());
        assert_eq!(
            pause.request_resume(),
            SoftPauseResumeOutcome::Requested(SoftPauseBoundary::Tools)
        );
        assert_eq!(
            pause.request_resume(),
            SoftPauseResumeOutcome::AlreadyRequested(SoftPauseBoundary::Tools)
        );
        assert_eq!(pause.take_resume(), Some(SoftPauseBoundary::Tools));
        assert_eq!(pause.take_resume(), None);
        assert_eq!(pause, SoftPauseState::Inactive);
    }

    #[test]
    fn forced_pause_uses_exact_boundary() {
        let mut pause = SoftPauseState::Requested;
        pause.pause_at(SoftPauseBoundary::Provider);
        assert_eq!(pause, SoftPauseState::Paused(SoftPauseBoundary::Provider));
        assert_eq!(
            pause.request_resume(),
            SoftPauseResumeOutcome::Requested(SoftPauseBoundary::Provider)
        );
        assert_eq!(pause.take_resume(), Some(SoftPauseBoundary::Provider));
    }
}
