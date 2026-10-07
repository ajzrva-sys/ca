//! Event domain types.
//!
//! Owns lightweight serializable events shared by orchestration state and the
//! append-only event stream. Persistence belongs in `telemetry`/`app`; these
//! types are just the durable vocabulary.
//!
//! Read when changing event names or persisted event shape. Do not read for
//! state transitions, queue parsing, or file IO.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Event {
    pub(crate) ts: String,
    pub(crate) role: String,
    pub(crate) kind: String,
    pub(crate) summary: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum OrchestratorEventKind {
    TaskSelected,
    AffectedWorkComputed,
    RecipePlanned,
    RecipeStarted,
    RecipeFinished,
    DiagnosticsParsed,
    FindingsFiltered,
    EvidenceSummarized,
    GuardChecked,
    AutoAccepted,
    AiEscalated,
    SeniorReviewRequested,
    StateTransitionApplied,
    CommitCreated,
    CacheHit,
    CacheMiss,
    WatchDebounced,
    PolicyViolation,
    DependencyPolicyPlanned,
    SnapshotCompared,
    StagedVerificationPlanned,
    BatchCandidateFormed,
    SeniorBatchApproved,
    SeniorBatchExpired,
    GlobalMemoryApplied,
    RegressionSearchPlanned,
    RepoMapRanked,
    HookManifestLoaded,
    PromptReceiptLogged,
    RouteAttemptRecorded,
    RouteRegretSummarized,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct OrchestratorEvent {
    pub(crate) timestamp: String,
    pub(crate) kind: OrchestratorEventKind,
    pub(crate) task_id: Option<String>,
    pub(crate) payload: Value,
}
