//! Shared read, observe, and verify delegation boundary.
//!
//! MCP and the internal pre-Worker router both use these contracts and
//! executors. No action in this module edits product files, invokes a shell,
//! selects a repository, or makes a provider request.

mod contracts;
mod dependency_graph;
mod executor;
mod observation;
mod query;
mod rust_api;

pub use contracts::*;
pub(crate) use executor::{
    invalidate_evidence_cache_for_request, request_digest, validate_action_contract,
    validate_request, MechanisticExecutor,
};
pub(crate) use observation::LocalObservation;
pub(crate) use query::execute_query_action;
pub(crate) use rust_api::{compare as compare_rust_api, prime as prime_rust_api};
