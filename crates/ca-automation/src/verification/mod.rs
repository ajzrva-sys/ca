//! Deterministic verification subsystem.
//!
//! Owns typed recipes, legacy command migration, allowlist validation, process
//! execution, evidence reports, diagnostics, and result classification. It may
//! depend on `domain`, `repair`, and `telemetry`, but not on role runners or CLI.
//!
//! Read when changing verification commands, evidence persistence, or failure
//! summaries. Do not read for queue Markdown, prompt prose, or terminal UI.

pub mod ca;
pub mod classify;
pub mod diagnostics;
pub mod evidence;
pub mod isolation;
pub mod legacy;
mod library_reads;
pub mod plan;
pub mod protected_tree;
pub mod recipe;
pub mod registry;
pub mod runner;
pub mod validate;

pub(crate) use evidence::{CommandEvidence, EvidenceReport};
pub(crate) use plan::VerificationPlan;
