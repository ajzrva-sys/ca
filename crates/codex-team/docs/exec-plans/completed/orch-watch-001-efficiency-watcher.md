# ORCH-WATCH-001 Efficiency Watcher

## Purpose / Big Picture
Make `codex-team watch` useful as an operational watcher for the orchestrator. It should follow progress from local telemetry and surface inefficiencies like oversized prompts, repeated model hops, stale verification hints, and failed bookkeeping before the user has to notice them manually.

## Progress
- [x] Identify existing watch scaffold and telemetry sources.
- [x] Implement a read-only efficiency audit report.
- [x] Add focused regression coverage.
- [x] Run validation and rebuild the release binary.

## Surprises & Discoveries
`watch` currently records only a path watch plan. Prompt token estimates are already present in `.orchestrator/cost.jsonl`, and run prompt paths are written under `.orchestrator/runs/`, so a first useful watcher can be built without changing role execution.

## Decision Log
- Decision: Start with a read-only watcher that reports inefficiencies rather than auto-editing prompts.
  Rationale: Automatic prompt rewriting during a live orchestration loop is higher risk; surfacing exact bottlenecks is safe and immediately useful.

## Context and Orientation
- CLI `Watch` subcommand is in `src/cli.rs`.
- `pub fn watch` and `build_watch_plan` live in `src/app.rs`.
- Cost telemetry is read by `read_model_call_records`.
- Run artifacts live under `.orchestrator/runs/`.

## Plan of Work
Extend `watch` to build an `EfficiencyWatchReport` from cost records, state, and run artifacts. Print concise alerts and write `.orchestrator/observations/efficiency_watch.md`. Detect prompt bloat, adjacent PM/Worker large prompts, repeated failed tasks, and stale queue/state loops.

## Concrete Steps
1. Add report structs and rendering helpers in `src/app.rs`.
2. Wire the report into `watch`.
3. Add tests for high PM/Worker prompt alerts and repeated failure alerts.
4. Update `TESTING.md` with focused validation evidence.
5. Run focused tests, `cargo fmt -- --check`, `cargo test`, `cargo build --release`, and `git diff --check`.

## Validation and Acceptance
- `cargo test efficiency_watch`
- `cargo fmt -- --check`
- `cargo test`
- `cargo build --release`
- `git diff --check`

Acceptance: `codex-team watch` writes and prints actionable watcher findings without invoking roles or mutating task/product files.

## Idempotence and Recovery
The report is regenerated on each `watch` invocation. It is safe to rerun at any point during a live orchestration loop.

## Artifacts and Notes
- `cargo test efficiency_watch`: passed, 2 tests.
- `cargo fmt -- --check`: passed.
- `cargo test`: passed, 157 tests.
- `cargo build --release`: passed and refreshed `target/release/codex-team`.
- Live Helicon watcher smoke: `target/release/codex-team watch --repo /Users/andrewzimmerman/Downloads/heliconfocus` wrote `.orchestrator/observations/efficiency_watch.md` and flagged PM/Worker double-context bloat.

## Interfaces and Dependencies
Adds no new required config. Uses existing `watch` CLI and existing telemetry files.

## Outcomes & Retrospective
`codex-team watch` now emits an advisory efficiency report from local telemetry. It detects large prompts, back-to-back large Project Manager/Worker calls, repeated task failures, and compact-context opportunities. This is intentionally read-only: it writes `.orchestrator/observations/efficiency_watch.md` and `.json`, prints alerts, and does not approve, queue, run, stage, commit, or revert work.
