# ORCH-OLLAMA-001: Rust-Owned Ollama Role Runner

## Purpose / Big Picture
Let Rust invoke local Ollama for selected roles via a per-role `provider` switch, while Hermes continues to supervise only through `codex-team` CLI (no Hermes `delegate_task` / free Ollama orchestration).

## Progress
- [x] ExecPlan + NEXT_STEPS wired
- [x] RoleProvider + OllamaConfig
- [x] providers/ollama.rs adapter
- [x] run_codex_role dispatch + doctor probe
- [x] Tests + Hermes/docs updates
- [x] Validation; plan moved to completed

## Surprises & Discoveries
- `ureq` needs the `json` feature for `send_json` / `into_json`.
- Doctor Codex model-list checks must skip roles with `provider = ollama` (local tags are not Codex model names).

## Decision Log
- Default provider remains `codex`; Ollama is opt-in per role and requires `ollama.enabled = true`.
- No silent Codex fallback when Ollama is selected but disabled/unreachable.
- Use sync `ureq` for `/v1/chat/completions` and `/api/tags`.

## Context and Orientation
- Dispatch: `src/app.rs` `run_codex_role`
- Config: `src/config.rs`
- Adapter: `src/providers/ollama.rs`
- Codex stays in `src/codex/run.rs`

## Plan of Work
Implemented as specified.

## Concrete Steps
Done in tree.

## Validation and Acceptance
- Passed: `cargo test ollama` (9)
- Passed: `cargo test codex` (12)
- Passed: `cargo fmt -- --check`
- Passed: `cargo test` (278)
- Passed: `git diff --check`

## Idempotence and Recovery
Disable `ollama.enabled` or set `provider = "codex"` to restore Codex-only calls.

## Artifacts and Notes
- `src/providers/ollama.rs`, `src/providers/mod.rs`
- Config keys `[ollama]` + `roles.*.provider`

## Interfaces and Dependencies
- Config: `[ollama]`, `roles.*.provider`
- HTTP: `ureq` with `json` feature

## Outcomes & Retrospective
Rust owns optional Ollama role execution. Hermes skill/docs forbid free Ollama/`delegate_task` for supervised repos.
