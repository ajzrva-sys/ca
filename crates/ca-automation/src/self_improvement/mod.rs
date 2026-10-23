//! Machine-local, opt-in autonomous learning boundary.
//!
//! The stable supervisor, vault, safety manifest, trace scrubber, evaluation
//! policy, and rollback controls are authority boundaries. Learned artifacts and
//! autonomous source candidates may consume these APIs but may not modify them.

mod artifacts;
mod contracts;
mod daily_budget;
mod evaluation;
mod harness;
mod harness_protected;
mod harness_source;
mod harness_vm;
mod learning_loop;
mod source;
mod supervisor;
mod vault;

pub use artifacts::*;
pub use contracts::*;
pub use daily_budget::*;
pub use evaluation::*;
pub use harness::*;
pub use harness_source::*;
pub use harness_vm::*;
pub use learning_loop::*;
pub use source::*;
pub use supervisor::*;
pub use vault::*;

mod delegation;
pub(crate) use delegation::active_manifest_digest_for_actions;
pub use delegation::*;
