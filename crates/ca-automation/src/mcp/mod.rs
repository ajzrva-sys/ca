//! Production-safe MCP stdio boundary for `codex-team`.
//!
//! The repository is canonicalized and pinned when the server starts. Tool
//! inputs therefore never select a repository, and the SDK owns JSON-RPC
//! framing, notification handling, and capability negotiation.

pub mod jobs;
pub mod server;
