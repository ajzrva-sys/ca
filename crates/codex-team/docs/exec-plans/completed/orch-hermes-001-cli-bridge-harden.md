# ORCH-HERMES-001: Harden Hermes CLI Bridge

## Purpose / Big Picture
Make Nous Hermes Agent able to supervise `codex-team` via clean machine receipts: stdout is exactly one JSON object under `--json`, typed receipt contracts with schemas/tests, disposition/doctor JSON, skill draft + loop script, and Phase 2 cycle_id / observe-only status. MCP and Hermes-as-Worker stay deferred.

## Progress
- [x] ExecPlan + NEXT_STEPS task wired
- [x] Quiet `--json` stdout (progress → stderr)
- [x] Typed receipts + schemas + tests
- [x] `disposition --json` and `doctor --json`
- [x] Skill draft, supervisor script, HERMES/ROADMAP docs
- [x] `cycle_id` + `status --observe`
- [x] Validation recorded; plan moved to completed

## Surprises & Discoveries
- Shadowing module `println!` redirects ~288 progress sites without rewriting each call; receipts must use `::std::println!` via `emit_json_receipt`.
- `status --json` previously serialized the pause as the full object under `human_disposition_required`; it now uses the same `PauseReceipt` shape and adds `observe`.

## Decision Log
- Keep `protocol_version: 1` additive: `pause` + `cycle_id` on step receipts; preserve `human_disposition_required` bool.
- Shadow module `println!` under a thread-local so `--json` redirects progress without rewriting ~288 call sites.
- MCP remains deferred until disposable-repo POC checklist is checked off.

## Context and Orientation
- Contract: `docs/HERMES.md`, `docs/HERMES_IMPLEMENTATION_GUIDE.md`
- Receipts: `src/app/receipts.rs`
- Entry points: `src/app.rs` `step` / `status` / `disposition` / `doctor_command`
- CLI: `src/cli.rs`
- Tests: `src/app_tests.rs` (`hermes_*`)

## Plan of Work
Completed Milestones A–C as specified.

## Concrete Steps
Implemented in tree; validated with focused and full `cargo test`.

## Validation and Acceptance
- Passed: `cargo test hermes_` (7)
- Passed: `cargo test human_disposition` (10)
- Passed: `cargo fmt -- --check`
- Passed: `cargo test` (269 lib tests)
- Passed: `git diff --check`

## Idempotence and Recovery
Re-run validation after any partial edit; lock-held CLI failures leave repo lock released on process exit.

## Artifacts and Notes
- `schemas/cycle_receipt.schema.json`, `schemas/status_receipt.schema.json`
- `docs/hermes-skill-draft.md`
- `scripts/hermes_supervisor_loop.sh`

## Interfaces and Dependencies
- CLI: `--json` on step/status/disposition/doctor; `--observe` on status
- Hermes supervisor uses closed-stdin `step --json` + outcome table

## Outcomes & Retrospective
CLI Hermes bridge is parse-whole-stdout safe. MCP still deferred pending human POC checklist.
