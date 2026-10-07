use sha2::{Digest, Sha256};

use crate::{ProbeObservation, ProcessSpec, WatchRequestV1, WatchState};

#[derive(Default)]
pub(crate) struct AdapterState {
    expected_identity: Option<[u8; 32]>,
}

pub(crate) struct AdapterEvent {
    pub state: WatchState,
    pub summary: String,
}

pub(crate) trait ProcessAdapter {
    fn classify(&self, observation: ProbeObservation, state: &mut AdapterState) -> AdapterEvent;
}

pub(crate) fn classify(
    request: &WatchRequestV1,
    observation: ProbeObservation,
    state: &mut AdapterState,
) -> AdapterEvent {
    match &request.process {
        ProcessSpec::Generic => GenericAdapter.classify(observation, state),
        ProcessSpec::Copy { .. } => CopyAdapter.classify(observation, state),
    }
}

struct GenericAdapter;

impl ProcessAdapter for GenericAdapter {
    fn classify(&self, observation: ProbeObservation, state: &mut AdapterState) -> AdapterEvent {
        match verify_identity(observation.identity.as_deref(), state) {
            IdentityResult::Alive => AdapterEvent {
                state: WatchState::Running,
                summary: "Process is running".to_string(),
            },
            IdentityResult::Missing => AdapterEvent {
                state: WatchState::Exited,
                summary: "Process exited".to_string(),
            },
            IdentityResult::Drifted => drift_event(),
        }
    }
}

struct CopyAdapter;

impl ProcessAdapter for CopyAdapter {
    fn classify(&self, observation: ProbeObservation, state: &mut AdapterState) -> AdapterEvent {
        if observation.marker_present {
            return AdapterEvent {
                state: WatchState::Completed,
                summary: "Completion marker detected".to_string(),
            };
        }
        match verify_identity(observation.identity.as_deref(), state) {
            IdentityResult::Alive => AdapterEvent {
                state: WatchState::Running,
                summary: observation
                    .log_line
                    .unwrap_or_else(|| "Copy process is running".to_string()),
            },
            IdentityResult::Missing => AdapterEvent {
                state: WatchState::Failed,
                summary: "Process exited before the completion marker appeared".to_string(),
            },
            IdentityResult::Drifted => drift_event(),
        }
    }
}

enum IdentityResult {
    Alive,
    Missing,
    Drifted,
}

fn verify_identity(identity: Option<&str>, state: &mut AdapterState) -> IdentityResult {
    let Some(identity) = identity else {
        return IdentityResult::Missing;
    };
    let fingerprint: [u8; 32] = Sha256::digest(identity.as_bytes()).into();
    match state.expected_identity {
        Some(expected) if expected != fingerprint => IdentityResult::Drifted,
        Some(_) => IdentityResult::Alive,
        None => {
            state.expected_identity = Some(fingerprint);
            IdentityResult::Alive
        }
    }
}

fn drift_event() -> AdapterEvent {
    AdapterEvent {
        state: WatchState::Failed,
        summary: "PID identity changed; refusing to follow a reused PID".to_string(),
    }
}
