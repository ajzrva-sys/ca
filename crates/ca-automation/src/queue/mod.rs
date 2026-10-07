//! Queue and tracking document subsystem.
//!
//! Owns NEXT_STEPS parsing/rendering, dependency gates, non-AI task queues, and
//! Rust-owned tracking-document patches. It may depend on `domain` but should
//! not call Codex, run verification commands, or own top-level app routing.
//!
//! Read when changing task card parsing, queue promotion, or completion notes.
//! Do not read for process execution, stream rendering, or role prompts.

pub mod gates;
pub mod next_steps;
pub mod non_ai;
pub mod tracking_patch;
