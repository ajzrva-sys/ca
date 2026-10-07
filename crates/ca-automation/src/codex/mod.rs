//! Codex adapter subsystem.
//!
//! Owns `codex exec` invocation, JSONL stream parsing, live terminal rendering,
//! prompt-size utilities, and model-call IO boundaries once extracted. It should
//! not decide task policy or mutate queue state.
//!
//! Read when changing model execution, streaming output, or prompt accounting.
//! Do not read for task parsing, guard scope checks, or verification recipes.

pub mod live;
pub mod prompt_pack;
pub mod run;
pub mod stream;
