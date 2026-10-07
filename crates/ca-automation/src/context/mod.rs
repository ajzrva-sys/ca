//! Worker context leasing and repo-hint caches.
//!
//! Owns which file bodies may enter Worker prompts. Workers may request a
//! bounded `expand_lease`; they must not embed global repo maps by default.
//!
//! Read when changing lease policy or hint-cache TTL. Do not read for queue
//! parsing or Codex spawning. See `src/context/README.md`.

pub mod lease;
