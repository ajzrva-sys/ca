# ORCH-ROUTER-001 Route Cost Controls

## Purpose / Big Picture
Adapt the useful cost-control lessons from `opencode-model-router-master` into `codex-team` without copying OpenCode plugin mechanics. The first slice is observability-only: Rust records redundant Worker exploration after a run and makes route-regret/cost reports show where future routing could save tokens.

## Progress
- [x] Compare `codex-team` with `opencode-model-router-master`.
- [x] Decide to reuse `ExecutionLane` instead of adding a parallel tier enum.
- [x] Add redundant exploration route-regret detection.
- [x] Extend route-regret and cost reports with lane savings.
- [x] Validate focused route/watch tests and full suite.

## Surprises & Discoveries
- `codex-team` already has route attempts, route regret reports, cost output, prompt receipts, and Worker log harvesting. The first slice can extend those instead of adding a new reporting system.
- Redundant exploration should be recorded as observed Worker spend, not as a cancelled call, so `calls_avoided` remains truthful.

## Decision Log
- Use existing `ExecutionLane` values for cost lanes; do not introduce `fast`/`medium`/`heavy` terminology.
- Detect redundant exploration only from completed Worker command executions in JSONL logs.
- Keep this slice advisory-only. It does not hard-block live `codex exec` or alter Worker prompts.
- Treat follow-on file leases, acceptance normalization, and escalation policy as future tasks.

## Context and Orientation
- Route domain types live in `src/domain/route.rs`.
- Route attempt persistence, route-regret reports, watcher alerts, and cost output currently live in `src/app.rs`.
- Existing tests for this area live in `src/app_tests.rs` under route-regret and efficiency-watch tests.
- Related completed work: `docs/exec-plans/completed/orch-route-001-route-attempt-memory.md`.

## Plan of Work
Add a new `RedundantExploration` route regret kind, scan recent Worker stdout JSONL logs for repeated read/search command fingerprints, and record compact route-regret observations. Extend the route report with a redundant exploration count and savings grouped by suggested better lane. Update watcher/cost output and tests.

## Concrete Steps
1. Add `RouteRegretKind::RedundantExploration`.
2. Add a Worker-log scanner for repeated completed `sed`, `cat`, `head`, `tail`, and `rg` commands.
3. Record at most one redundant-exploration route regret per task/fingerprint.
4. Extend `RouteRegretReport` and markdown rendering.
5. Print redundant exploration count and lane savings in `codex-team cost`.
6. Add focused tests.
7. Update `NEXT_STEPS.md`, `CONTINUITY.md`, and `TESTING.md` with final validation evidence.

## Validation and Acceptance
- `cargo test route_regret`
- `cargo test efficiency_watch`
- `cargo fmt -- --check`
- `cargo test`
- `cargo build --release`
- Acceptance: reports show `redundant_exploration`, redundant count, lane savings, and fingerprint evidence without changing live Worker execution.

## Idempotence and Recovery
The scanner deduplicates by task plus redundant command fingerprint before recording. Re-running `watch` or `cost` should not create duplicate route-regret rows for the same task/fingerprint.

## Artifacts and Notes
- Source comparison target: `opencode-model-router-master/`.
- First slice intentionally excludes OpenCode hooks, provider fallback chains, and `Task(subagent_type=...)`.
- Validation evidence:
  - `cargo test route_regret`: 6 tests passed.
  - `cargo test efficiency_watch`: 11 tests passed.
  - `cargo fmt -- --check`: passed.
  - `cargo test`: 245 unit tests passed, plus zero-test main/doc targets.
  - `cargo build --release`: passed.

## Interfaces and Dependencies
- Public CLI surface remains unchanged.
- Existing JSONL route attempt/outcome files remain backward-compatible; new report fields are additive.

## Outcomes & Retrospective
Implemented the observability-only first slice. `codex-team` now records `redundant_exploration` route-regret observations from recent completed Worker stdout JSONL command logs, dedupes them by task plus command fingerprint, keeps `calls_avoided` limited to cancelled pre-model calls, and renders redundant counts, suggested-lane token savings, and fingerprint evidence in route-regret/cost reporting. Follow-on Worker leases, acceptance-contract normalization, and bounded escalation policy remain future work.
