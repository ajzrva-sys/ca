# Senior Foreman Progress And Rust Evidence Jobs

## Purpose / Big Picture
Move the hot path away from repeated Project Manager and Worker calls for deterministic work. Project Manager should be read-only and rare, Senior Dev should be able to classify the next progress actions, and Rust should run command-only evidence jobs without spending Worker tokens.

## Progress
- [x] State intended contract and minimal slice.
- [x] Update PM prompt/schema contract and add mutation guard.
- [x] Add Senior progress-action schema support and Rust execution lane.
- [x] Persist compact Rust evidence-job receipts and surface them to roles.
- [x] Extend efficiency watch alerts for PM edits and Worker-run deterministic commands.
- [x] Add focused regression tests.
- [x] Run validation and move this plan to completed.

## Surprises & Discoveries
- The SwiftStacker release benchmark recipes and no-shell execution already exist in Rust verification; this slice reused them instead of adding another command runner.
- PM zero-task control-doc auto-commit already existed; the missing guard was stopping any other PM file mutation before Worker.
- Recovering approved `NEXT_STEPS.md` work already skipped PM, but recovery cleared `planner_due` before skip telemetry was recorded. Rust now records that planner skip at recovery time.

## Decision Log
- Keep the first slice single-worktree and sequential. Parallel Worker worktrees and background evidence jobs remain follow-up work.
- Reuse `VerificationPlan`/`EvidenceReport` for Rust evidence jobs so receipts share the existing evidence model.
- Do not call Senior merely because the queue is empty in this slice. Progress actions ride through the existing Senior gate first, avoiding a new role-call loop while the action contract stabilizes.

## Context and Orientation
- `src/app.rs` owns the current routing loop, role prompts, Senior batch gate, verification plans, evidence persistence, watcher reports, and commits.
- `prompts/planner.md` and `schemas/planner.schema.json` define the Project Manager contract.
- `prompts/foreman_batch.md` and `schemas/foreman_batch.schema.json` define the current Senior batch gate.
- `src/verification/recipe.rs` validates typed command recipes including SwiftStacker release build and benchmark report roots.
- `src/app_tests.rs` holds fake-runner routing and watcher tests.

## Plan of Work
Update the PM prompt to say PM must not edit files. Snapshot the repo around PM calls, auto-commit only the existing zero-task control-doc reconciliation case, and otherwise stop with a policy event. Extend Senior batch output with typed progress actions. Rust validates evidence/bookkeeping commands, executes them through the existing verification runner, persists a compact receipt, and queues at most one Worker edit task. Watch mode flags PM mutations, PM/Worker doc bounces, and long deterministic commands run by Worker.

## Concrete Steps
1. Patched prompts/schema/structs for PM and Senior progress actions.
2. Added PM mutation guard helpers and wired them into both PM call branches.
3. Added Rust evidence receipt helpers and Senior progress-action application.
4. Added watcher event/log scans.
5. Added focused tests and ran validation commands.

## Validation and Acceptance
- Passed: `cargo test foreman_progress` with 3 tests.
- Passed: `cargo test pm_control_doc` with 3 tests.
- Passed: `cargo test pm_mutated` with 1 test.
- Passed: `cargo test pm_source_edits` with 1 test.
- Passed: `cargo test approved_backlog_skips_pm` with 1 test.
- Passed: `cargo test efficiency_watch` with 9 tests.
- Passed: `cargo test verification` with 22 tests.
- Passed: `cargo fmt -- --check`.
- Passed: `cargo test` with 182 tests.
- Passed: `cargo build --release`.

## Idempotence and Recovery
The patch only changes orchestrator source, prompts, schemas, and control docs. Rust evidence receipts are append-only runtime artifacts in target repos; tests use temp repos. If a future progress action is rejected, Rust records a policy event and routes to PM rather than running unsafe commands or Worker edits.

## Artifacts and Notes
- Rust evidence job receipts are written under `.orchestrator/observations/evidence_jobs/` in target repos.
- No runtime target repo was intentionally modified while implementing this slice.

## Interfaces and Dependencies
- Project Manager output schema remains compatible and is now documented as read-only.
- Senior batch schema gains required `progress_actions`; callers should emit an empty array when no progress actions are needed.
- Evidence jobs use existing typed verification command parsing and `std::process::Command` execution via the verification runner.

## Outcomes & Retrospective
The orchestrator now has the first Foreman/Rust evidence-job slice: PM cannot silently act like Worker, Senior can hand deterministic command work to Rust, Worker receives receipts instead of long-running benchmark commands, and watch mode can flag the inefficiencies observed during Helicon runs. The next useful slice is background evidence-job scheduling or isolated Worker worktrees, after this single-worktree contract has been exercised.
