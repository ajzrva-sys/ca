# ORCH-ROUTE-001: Route Attempt Memory And Active Regret Routing

## Purpose / Big Picture
The orchestrator should learn from its own routing choices instead of repeatedly spending PM, Senior Dev, or Worker calls on work Rust can classify or run deterministically. This slice adds target-local route memory under `.orchestrator/observations/`, records route attempts at existing role/Rust seams, and surfaces high-confidence regret patterns in watcher and cost output.

## Progress
- [x] Root cause and intended contract recorded.
- [x] Route domain types and pure transition helpers added.
- [x] Route attempt/outcome persistence added under `.orchestrator/observations/`.
- [x] Existing PM/Senior/Worker/Rust-evidence seams record route memory.
- [x] Watcher and cost output include route regret summaries.
- [x] Focused tests and full validation completed.
- [x] Release binary rebuilt.

## Surprises & Discoveries
- The repo already has prompt receipts, a cost ledger, mechanistic handoff detection, and Rust evidence job receipts. Route memory can link those instead of storing raw logs or transcripts.
- Existing `RunRecord` is command/recipe oriented, not role-routing oriented, so a separate target-local route memory file is clearer.

## Decision Log
- Keep route memory target-local for v1 because the current watcher and state artifacts are target-local.
- Store compact attempts/outcomes and receipts, not raw prompts, full logs, or full diffs.
- Use conservative active routing: safe command-only work goes to Rust; repeated identical failures stop Worker retry churn by routing to Senior repair; stale planner churn and PM/Worker doc bounce become regret alerts before deeper automation.

## Context and Orientation
- Domain vocabulary lives in `src/domain/`.
- Main routing seams live in `src/app.rs`.
- Watcher reporting lives around `build_efficiency_watch_report` and `write_efficiency_watch_report`.
- Cost reporting lives in `print_cost_report`.
- Existing deterministic/evidence receipts live under `.orchestrator/observations/evidence_jobs/`.

## Plan of Work
Add a route domain module with lifecycle, lanes, regret kinds, token usage, and pure decision helpers. Add app-level helpers to append `route_attempts.jsonl`, append compact `route_outcomes.jsonl`, and render aggregate `route_regret.json` / `.md`. Wire the helpers into Codex role calls, Rust model skips, Rust evidence jobs, mechanistic handoff, and repeated-fingerprint retry decisions. Extend watcher/cost reports to read route outcomes and show avoidable-spend signals.

## Concrete Steps
1. Add `src/domain/route.rs` and export it from `src/domain/mod.rs`.
2. Add `RouteAttemptRecorded` and `RouteRegretSummarized` event kinds.
3. Add app helpers for route path calculation, append/read, regret aggregation, and Markdown rendering.
4. Record attempts from `run_codex_role`, `record_rust_model_skip`, `run_senior_rust_evidence_job`, and policy/skip paths where already explicit.
5. Update Worker retry routing to use the pure repeated-fingerprint regret helper.
6. Extend watch and cost output.
7. Add focused tests and run validation commands.

## Validation and Acceptance
- `cargo test route_attempt`
- `cargo test route_regret`
- `cargo test efficiency_watch`
- `cargo test foreman_progress`
- `cargo test command_only`
- `cargo test worker`
- `cargo fmt -- --check`
- `cargo test`
- `cargo build --release`
- `git diff --check`

Acceptance: route attempt/outcome files are appended without raw transcripts, watcher/cost output surfaces route regret, command-only safe work records Rust substitution, repeated identical failures do not trigger another full Worker retry, and existing deterministic evidence behavior remains intact.

## Idempotence and Recovery
Route memory files are append-only observations. If a run is interrupted, the next cycle can append new attempts/outcomes without rewriting prior rows. Malformed JSONL lines are ignored by existing readers. No product files are edited by watcher/report generation.

## Artifacts and Notes
- Planned target artifacts:
  - `.orchestrator/observations/route_attempts.jsonl`
  - `.orchestrator/observations/route_outcomes.jsonl`
  - `.orchestrator/observations/route_regret.json`
  - `.orchestrator/observations/route_regret.md`
- Focused validation started:
  - `cargo test route_attempt` passed.
  - `cargo test route_regret` passed.
  - `cargo test efficiency_watch` passed.
  - `cargo test foreman_progress` passed.
  - `cargo test command_only` passed.
  - `cargo test worker` passed.
- Full validation completed:
  - `cargo fmt -- --check` initially reported formatting drift, then passed after `cargo fmt`.
  - `cargo test` passed 213 unit tests, plus zero-test main/doc targets.
  - `cargo build --release` passed and refreshed `target/release/codex-team`.
  - `git diff --check` passed.

## Interfaces and Dependencies
- Existing cost ledger remains `.orchestrator/cost.jsonl`.
- Existing event stream remains `.orchestrator/events.jsonl`.
- `future_worktree_worker` is metadata only in this slice.

## Outcomes & Retrospective
Route memory now records compact target-local attempts/outcomes for role calls, Rust skips, and Rust evidence jobs. Watcher/cost output can surface route regret without requiring transcript inspection. Active repeated-fingerprint routing now prevents another full Worker retry when the same failure repeats without verification improvement, while command-only deterministic work remains in the Rust evidence lane. Follow-up work should add richer prompt-receipt IDs to non-Worker role calls and, later, cross-repo route memory once target-local behavior has run for a while.
