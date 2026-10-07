# ORCH-WATCH-007: Watcher-Driven Mechanistic Handoff To Rust

## Purpose / Big Picture
Turn watcher findings like "Worker ran a deterministic benchmark/test/build" into Rust-owned evidence work. The operator should not have to paste watcher output back into a Codex thread for known safe commands; Rust should materialize the handoff and route future command-only tasks away from Worker.

## Progress
- [x] State the intended contract and safe boundaries.
- [x] Add mechanistic handoff observations and watcher self-heal materialization.
- [x] Drain command-only tasks through Rust evidence before Worker.
- [x] Add focused regression tests.
- [x] Run validation and move this plan to completed.

## Surprises & Discoveries
- Existing watcher alerts already detect Worker-run deterministic commands and suggest `rust_evidence_job`, but self-heal only handles compact planner mode and stale verification hints.
- Existing Senior progress actions already execute Rust evidence jobs and persist receipts; this slice can reuse that executor instead of adding another command runner.
- `step_once` syncs the in-memory queue from control docs before routing, so routing tests must use real `NEXT_STEPS.md` task cards instead of manually seeding `state.queue`.

## Decision Log
- Use target `.orchestrator/observations/` for self-heal artifacts; do not edit target product files from watcher.
- Keep command-only handoff separate from edit-task verification. Source/doc edit tasks with validation commands remain Worker tasks, with Rust final verification afterward.
- Keep high-intelligence orchestrator repair behind existing `watch-loop --orchestrator-repo` feedback rather than letting watcher patch source code directly.

## Context and Orientation
- `src/app.rs` owns watcher reports, self-heal, step routing, Senior progress actions, and evidence receipt persistence.
- `src/verification/recipe.rs` and app helper parsing own typed safe command validation.
- `src/app_tests.rs` contains watcher, routing, and fake-runner tests.
- Target artifacts are `.orchestrator/observations/mechanistic_handoffs.md` and `.json`.

## Plan of Work
Add a small mechanistic handoff candidate model. Watcher will build candidates from recent Worker logs and include structured handoff metadata in alerts. `watch --self-heal` will persist safe/unsafe/unsupported decisions as observation data. The step router will check a selected task before Worker launch; if the task is command-only and all commands are safe typed recipes, Rust will run it through the existing evidence-job executor, persist a receipt, update queue bookkeeping, and record Worker as skipped.

## Concrete Steps
1. Add mechanistic handoff structs, Markdown/JSON persistence, and worker-log candidate extraction.
2. Extend watcher alerts and self-heal reports with handoff actions.
3. Add a pre-Worker Rust evidence drain for command-only tasks.
4. Add tests for watcher materialization, safe Rust-before-Worker execution, source-edit non-bypass, unsafe command handling, and feedback rendering.
5. Update continuity/testing docs, run validation, rebuild release binary.

## Validation and Acceptance
- `cargo test efficiency_watch`
- `cargo test foreman_progress`
- `cargo test verification`
- Focused routing tests for mechanistic handoff behavior.
- `cargo fmt -- --check`
- `cargo test`
- `cargo build --release`
- `git diff --check`

## Idempotence and Recovery
Mechanistic handoff observations are overwriteable summaries keyed by command fingerprint and source log/task. Re-running watcher self-heal updates `.orchestrator/observations/mechanistic_handoffs.*` but does not run commands or edit product files. If a direct Rust evidence task fails, queue bookkeeping records it as blocked and leaves subsequent routing to existing planner/Senior paths.

## Artifacts and Notes
- Target observations: `.orchestrator/observations/mechanistic_handoffs.md` and `.json`.
- Evidence receipts remain under `.orchestrator/observations/evidence_jobs/`.

## Interfaces and Dependencies
- No new CLI flags.
- Watch reports gain optional structured `mechanistic_handoff` alert data.
- Self-heal reports gain a `mechanistic_handoff` action when candidates are persisted.

## Outcomes & Retrospective
Implemented. Watcher self-heal now writes mechanistic handoff observations, watch reports carry structured handoff metadata, unsupported mechanistic alerts are described as recipe/router repair feedback for the orchestrator repo, and command-only evidence tasks run in Rust before Worker. Source/doc edit tasks with validation commands remain Worker-owned. Validation passed with `cargo test efficiency_watch`, `cargo test foreman_progress`, `cargo test verification`, `cargo test mechanistic`, `cargo test command_only`, `cargo test source_edit_task_with_validation_commands_still_calls_worker`, `cargo test watch_loop`, `cargo fmt -- --check`, `cargo test`, and `cargo build --release`.
