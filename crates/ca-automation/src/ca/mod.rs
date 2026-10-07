//! Restricted `ca role-run` process adapter.
//!
//! This module owns the versioned machine request, exact process-scoped
//! authority, bounded JSONL parsing, and child-process supervision. The app
//! conductor remains responsible for choosing roles and for the authoritative
//! post-run guard, verification, acceptance, staging, and commit flow.

mod run;

#[cfg(test)]
pub(crate) use run::CaPreflightReceipt;
pub(crate) use run::{CaProcessConfig, CaRoleAuthority, CaRunRequest, CaRunner, RealCaRunner};
