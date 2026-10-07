//! AI role orchestration subsystem.
//!
//! Owns role-specific runners, compact context packs, and decision schemas once
//! extracted. Rust routing decides whether a role deserves to be called; roles
//! may authorize typed work, but Rust owns queue mutation and commits.
//!
//! Read when changing CEO/Planner/Worker/Senior behavior or prompt assembly. Do
//! not read for low-level process execution or raw queue parsing.

pub mod ceo;
pub mod planner;
pub mod routing;
pub mod senior;
pub mod worker;
