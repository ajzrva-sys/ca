//! Typed, token-free observation of local and SSH-hosted processes.
//!
//! A [`WatchService`] polls read-only process adapters and publishes ordered
//! [`WatchEventV1`] values. Stopping a watch cancels observation only; this
//! crate never sends a signal to the observed PID.

mod adapter;
mod service;
mod transport;
mod types;

pub use service::WatchService;
pub use transport::{ProbeError, ProbeObservation, ProbeTransport, SystemProbeTransport};
pub use types::{
    HostSpec, ProcessSpec, ValidationError, WatchEventV1, WatchId, WatchRequestV1, WatchSnapshotV1,
    WatchState, DEFAULT_INTERVAL_SECS, MAX_INTERVAL_SECS, MIN_INTERVAL_SECS,
};
