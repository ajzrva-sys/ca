//! ca-owned automation execution engine and legacy orchestration services.
//!
//! The crate is organized so agents can inspect subsystem contracts instead of
//! loading the whole binary. `native` and `runtime` execute one admitted run;
//! they never schedule occurrences. Stable seams live here; `app` remains the legacy cycle
//! conductor for routing, accept/commit, and not-yet-extracted helpers.
//!
//! Module map:
//! - `cli` — Clap parsing and dispatch
//! - `app` — conductor (`step_once`, accept/commit, queue bookkeeping, live UI)
//! - `config` / `state` — TOML/JSON DTOs (load/save still in `app`)
//! - `queue` — pure NEXT_STEPS parsing
//! - `verification` — recipes, ValidatedCommand execution, evidence DTOs
//! - `guard` — snapshots and allowed-file reports
//! - `codex` — `codex exec` adapter and stream helpers
//! - `domain` — shared vocabulary (tasks, routes, acceptance, tool health)
//! - `context` — Worker file leases / repo-hint cache
//! - `lock` — `.orchestrator/codex-team.lock`
//! - `git` — git adapter: root discovery, status/diff queries, observation
//!   parsing, staging, and commit execution
//! - `repair` — retry briefs and failure fingerprints
//! - `prompts` — embedded prompt/schema assets
//! - `providers` — optional model backends (Ollama); Codex stays in `codex`
//!
//! Read when discovering crate layout. Do not treat this file as ownership for
//! routing policy; use `docs/AGENT_MAP.md` and subsystem READMEs.

pub mod app;
pub mod automation_setup;
pub mod automation_workflow;
pub mod broker;
pub mod ca;
pub mod cancellation;
pub mod cli;
pub mod codex;
pub mod config;
pub mod context;
pub mod domain;
pub mod fs_util;
pub mod git;
pub mod git_metadata;
pub mod guard;
mod json_identity;
pub mod learning;
pub mod lock;
pub mod machine_json;
pub mod mcp;
pub mod mechanistic;
pub mod native;
pub mod paths;
pub mod presentation;
pub mod prompts;
pub mod providers;
pub mod queue;
pub mod release_readiness;
pub mod remote;
pub mod repair;
pub mod revision;
pub mod roles;
pub mod runtime;
pub mod self_improvement;
pub mod state;
pub mod task_import;
pub mod telemetry;
pub mod text;
pub mod torrent;
pub mod verification;
pub mod worktree_manager;

pub mod repository;
