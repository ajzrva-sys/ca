# ORCH-HEL-005: Preserve Disposition Suppresses Dirty Continuation

## Purpose / Big Picture
When a user chooses the `preserve` human disposition for a dirty adoption set, `codex-team` treats that dirty tree as human-owned context. It does not convert the preserved baseline into Rust-generated `DIRTY-*` Worker work on a later `step`.

## Progress
- [x] Confirmed the dirty continuation path ignored `preserve_quarantine_stop`.
- [x] Confirmed policy validation happened after task-card persistence in the Worker selection route.
- [x] Added preserve gates before dirty continuation generation and Senior-batch recovery.
- [x] Validated generated dirty candidates before Senior review and before durable task writes.
- [x] Added regression tests for preserved Helicon-like dirty state and policy-first dirty candidates.
- [x] Ran focused and full validation.
- [x] Moved this plan to `docs/exec-plans/completed/` with results.

## Surprises & Discoveries
The renderer/campaign preflight blocker prevented Worker execution, but only after a `DIRTY-*` task had been reviewed and persisted. The safety boundary worked for product files, but the orchestration bookkeeping boundary was too late.

## Decision Log
- Preserve means no automatic dirty-tree continuation. Rationale: the human explicitly chose to preserve/quarantine the dirty set as user-owned context.
- Existing stale `DIRTY-*` sections are ignored while preserve is active. Rationale: this makes already-written artifacts non-runnable without mutating the adopted repository.
- Dirty continuation policy checks run before Senior Dev. Rationale: Rust policy can reject invalid typed work cheaply and deterministically.
- Worker task cards are written only after task policy passes. Rationale: rejected tasks should not leave durable runnable artifacts.

## Context and Orientation
Relevant code is in `src/app.rs`: `try_queue_dirty_tree_technical_continuation`, `dirty_tree_continuation_candidate`, `enqueue_approved_next_steps_tasks`, `sync_state_queue_with_next_steps`, `queue_next_task_from_active_senior_batch`, and the Worker selection policy check. Regression tests live in `src/app_tests.rs`.

## Plan of Work
Added a helper that recognizes `human_disposition_decision.decision == "preserve_quarantine_stop"`. Used it to suppress generated dirty candidates and ignore stale approved `DIRTY-*` tasks during approved-task and Senior-batch recovery. Added a pre-Senior policy check for generated dirty candidates and moved Worker-selection persistence behind the existing policy validator.

## Concrete Steps
1. Patched `src/app.rs` with the preserve helper and dirty continuation gates.
2. Patched `src/app.rs` so dirty candidates are policy-validated before Senior Dev review.
3. Patched `src/app.rs` so `write_task_card` runs only after task policy passes.
4. Added focused regressions in `src/app_tests.rs`.
5. Updated source-of-truth docs with validation evidence.
6. Rebuilt `target/release/codex-team`.

## Validation and Acceptance
Acceptance:
- Preserved dirty baseline does not generate, review, persist, append, queue, or run `DIRTY-*` work.
- Stale approved `DIRTY-*` sections are ignored while preserve is active.
- Dirty candidates that fail policy do not call Senior Dev and do not write durable task artifacts.
- Normal dirty continuation behavior remains covered by existing approval tests.

Commands:
- `cargo test dirty_tree`: passed, 6 tests.
- `cargo test parity_preflight`: passed, 3 tests.
- `cargo test human_disposition`: passed, 7 tests.
- `cargo test preserve_disposition_ignores_stale_approved_dirty_next_steps_work`: passed.
- `cargo fmt -- --check`: passed.
- `cargo test`: passed, 150 tests.
- `cargo build --release`: passed.
- `git diff --check`: passed.

## Idempotence and Recovery
The patch is local to orchestrator behavior and tests. If validation fails later, revert or adjust only the ORCH-HEL-005 changes and rerun the focused failing command before full validation.

Existing stale Helicon `DIRTY-*` artifacts are made non-runnable by preserve-aware recovery. Physical cleanup of those Helicon files remains a separate explicit recovery action.

## Artifacts and Notes
No commands were run in `/Users/andrewzimmerman/Downloads/heliconfocus` during this implementation.

## Interfaces and Dependencies
No CLI or schema changes were made. The behavior change is internal Rust routing and validation order.

## Outcomes & Retrospective
`preserve_quarantine_stop` now suppresses dirty-tree continuation from preserved baselines, stale approved `DIRTY-*` sections are ignored under preserve, and generated dirty candidates must pass Rust policy before Senior Dev review or durable writes.
