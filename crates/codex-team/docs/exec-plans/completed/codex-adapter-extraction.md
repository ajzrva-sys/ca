# Codex Adapter Extraction And Reference Hardening

## Purpose / Big Picture
Move Codex process execution and pure JSONL stream parsing out of the app conductor so the adapter owns `codex exec` IO while Rust keeps orchestration authority. Use `claude-codex-orchestrator-main.zip` only as a reference for separation and test shapes; do not import TypeScript/MCP behavior.

## Progress
- [x] Link this plan from `NEXT_STEPS.md`.
- [x] Move the Codex runner seam and production spawn path into `src/codex/run.rs`.
- [x] Add pure JSONL parser helpers in `src/codex/stream.rs`.
- [x] Add structured JSON response and path canonicalization tests.
- [x] Run focused tests, formatting check, and full suite.

## Surprises & Discoveries
- `src/app.rs` still owns the live renderer and telemetry record type. The extraction should keep the public behavior stable while moving only adapter-shaped code.
- Live terminal rendering stayed in `src/app.rs` for this pass and is called through narrow runner hooks. This avoided coupling the adapter to conductor policy while preserving current output behavior.
- The adapter writes AI escalation events and cost records with the same persisted JSON shape; app-side skip telemetry remains in `src/app.rs`.

## Decision Log
- Keep role routing, queue mutation, guard checks, verification acceptance, and commits in `src/app.rs`.
- Use a small crate-private adapter settings/request shape instead of exposing broad app state to `src/codex/run.rs`.
- Add path/security coverage only for existing `Paths::new` canonicalization; do not add an unused allowed-directory runtime policy.

## Context and Orientation
- Current runner seam: `CodexRunRequest`, `CodexRunner`, `RealCodexRunner`, and `run_codex` in `src/app.rs`.
- Target modules: `src/codex/run.rs`, `src/codex/stream.rs`, and existing live-rendering target `src/codex/live.rs`.
- Existing fake-runner coverage: `step_once_routes_planner_through_fake_codex_runner`.
- Existing JSON fallback helper: `parse_json_response` in `src/app.rs`.
- Existing path root canonicalization: `Paths::new` in `src/paths.rs`.

## Plan of Work
First move the runner trait/request and production process execution behind `crate::codex::run`. Keep the conductor responsible for prompt/schema construction and typed response parsing. Then add pure JSONL helpers/tests inspired by the zip. Finally add focused tests for JSON response fallback and path canonicalization.

## Concrete Steps
1. Update `NEXT_STEPS.md` so MOD-002 points to this active plan and includes the broader parser/test scope.
2. Implement crate-private runner/request/settings types in `src/codex/run.rs`.
3. Replace the app-local runner trait/request with imports from `crate::codex::run`.
4. Ensure no production Codex spawn remains in `src/app.rs`.
5. Implement `parse_jsonl_line`, `parse_jsonl_stream`, `extract_final_agent_message`, `extract_completed_commands`, and `extract_changed_files` in `src/codex/stream.rs`.
6. Add module tests for stream parsing, response fallback, and `Paths::new`.

## Validation and Acceptance
- `cargo test step_once_routes_planner_through_fake_codex_runner`
- `cargo test codex`
- `cargo test parse_json_response`
- `cargo test paths`
- `cargo fmt -- --check`
- `cargo test`

Acceptance requires unchanged role prompt/schema behavior, unchanged telemetry JSON shape, fake-runner tests still avoiding real Codex, parser tests covering malformed JSONL, and no production `codex exec` spawn outside `src/codex/run.rs`.

## Idempotence and Recovery
All changes are source/doc refactors. If extraction becomes too broad, keep only the runner seam and pure parser helpers, record the live-render/security follow-up in this plan, and leave behavior in place.

## Artifacts and Notes
- Reference archive: `claude-codex-orchestrator-main.zip`.
- The archive is a TypeScript MCP server; it is not a source of architecture truth for this Rust CLI.
- Validation passed:
  - `cargo test step_once_routes_planner_through_fake_codex_runner`
  - `cargo test codex`
  - `cargo test parse_json_response`
  - `cargo test paths`
  - `cargo fmt -- --check`
  - `cargo test` with 131 tests passing

## Interfaces and Dependencies
- No public CLI/config/schema changes.
- `src/codex/run.rs` exposes crate-private adapter types only.
- `src/codex/stream.rs` exposes crate-private pure helpers only.

## Outcomes & Retrospective
- `src/codex/run.rs` now owns the `codex exec` process boundary for role calls.
- `src/codex/stream.rs` now owns pure JSONL parsing helpers and tests.
- `src/app.rs` remains the conductor and still owns role prompt construction, typed response parsing, queue/state mutation, live rendering policy, and Rust-skip telemetry.
- Future work: move live rendering into `src/codex/live.rs` when that can be done without mixing terminal presentation with process execution.
