# ORCH-LESSONS-001: Others Reliability And Routing Lessons

## Purpose / Big Picture
Adopt the useful lessons from the remaining `others/` repositories as Rust-owned routing intelligence, health telemetry, derived status, and acceptance policy. This work keeps live routing conservative: scorecards and cooldowns are advisory, while Rust blocks only risky Worker-bound tasks whose task contract lacks characterization evidence.

## Progress
- [x] Add pure domain vocabulary for tool health, route learning, derived status, and acceptance contracts.
- [x] Record compact Codex/tool health evidence and advisory cooldown summaries.
- [x] Add route scorecards, positive route learning, and `route-bench`.
- [x] Render derived status in status/watch reports.
- [x] Add pre-Worker acceptance-contract blocking for risky migration/refactor work without characterization evidence.
- [x] Run focused and full validation.

## Surprises & Discoveries
- `TaskSpec` already carries the fields needed for an acceptance contract: success criteria, verification commands, review tier, automation level, allowed/read-only files, and stop conditions. No task schema change is needed.
- The app-side cost reader can ignore extra Codex-runner cost fields, so the health ledger can be additive without changing old cost tests.
- The support-brief golden contract already had a default success criterion; the new acceptance contract correctly carries it as definition of done.

## Decision Log
- Reuse existing `ExecutionLane` for scorecards and route-bench results; no cost-tier enum was added.
- Keep tool cooldowns advisory until `codex-team` has an explicit Rust-owned fallback model/provider policy.
- Store route scorecards in a sidecar JSONL rather than changing route-attempt schema.
- Block Worker only for risky migration/refactor/port/parity work without characterization evidence.

## Context and Orientation
- Domain types live under `src/domain/`.
- `src/codex/run.rs` owns real `codex exec` process status and writes tool-health records.
- `src/app.rs` remains the adapter for report IO, status/watch/cost rendering, route-bench, and Worker-bound policy checks.
- `src/app/support.rs` adds the derived acceptance contract to the Worker support brief.

## Plan of Work
Add pure type modules first, wire compact tool-health evidence in the Codex adapter, then add app adapters for status/watch/cost/route-bench and the pre-Worker bad-contract guard. Finish with docs and focused tests for each new behavior.

## Concrete Steps
1. Added domain modules: acceptance, derived status, route learning, and tool health.
2. Appended `.orchestrator/observations/tool_health.jsonl` records for Codex success/failure/timeout.
3. Wrote advisory summaries to `tool_health.{json,md}` and rendered them in status/watch/cost.
4. Wrote route scorecards and positive route learnings under `.orchestrator/observations/`.
5. Added `codex-team route-bench --repo <path> [--json]`.
6. Derived `AcceptanceContract` before Worker launch and blocked risky uncharacterized tasks.
7. Added operations runbook docs and validation evidence.

## Validation and Acceptance
- Passed: `cargo test tool_health` (3 tests).
- Passed: `cargo test route_learning` (3 tests).
- Passed: `cargo test derived_status` (2 tests).
- Passed: `cargo test acceptance_contract` (4 tests).
- Passed: `cargo test route_bench` (1 test).
- Passed: `cargo test route_regret` (6 tests).
- Passed: `cargo test efficiency_watch` (12 tests).
- Passed after `cargo fmt`: `cargo fmt -- --check`.
- Passed after updating one support-brief golden expectation: `cargo test` (258 unit tests, zero-test main/doc targets).
- Passed: `cargo build --release`.
- Passed: `git diff --check`.

## Idempotence and Recovery
All observation files are append-only or overwriteable summaries under `.orchestrator/observations/`. Re-running watch/cost/route-bench refreshes summary files without touching product files. If validation fails later, keep the domain modules and narrow the failing adapter/test rather than changing task schema.

## Artifacts and Notes
- New observation files: `tool_health.jsonl`, `tool_health.{json,md}`, `route_learnings.jsonl`, `route_scorecards.jsonl`, `route_benchmark.{json,md}`.

## Interfaces and Dependencies
- New CLI: `codex-team route-bench --repo <path> [--json]`.
- No external dependencies were added.
- `TaskSpec` schema is unchanged.

## Outcomes & Retrospective
Completed. `codex-team` now has Rust-owned tool failure taxonomy and advisory cooldown summaries, route scorecards, positive route learning, local route benchmarking, derived operator status, and a normalized Worker acceptance contract. Live routing remains conservative: cooldowns and scorecards are advisory, while risky uncharacterized migration/refactor tasks are stopped before Worker and routed back to Project Manager.
