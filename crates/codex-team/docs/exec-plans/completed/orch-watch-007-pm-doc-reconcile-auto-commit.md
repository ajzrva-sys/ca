# ORCH-WATCH-007: PM Control-Doc Reconciliation Auto-Commit

## Purpose / Big Picture
Avoid spending Senior Dev and Worker prompts when Project Manager already made a bounded control-doc reconciliation and emitted no executable tasks. Rust should verify and commit the named tracking-doc edits directly when the change is mechanically safe.

## Progress
- [x] Reproduce the inefficiency from Helicon output.
- [x] Add Rust auto-commit path for PM zero-task control-doc reconciliation.
- [x] Keep existing Senior/Worker fallback for unsafe or uncommittable cases.
- [x] Add regression tests.
- [x] Validate and rebuild release binary.

## Surprises & Discoveries
- `planner_zero_work_reconciliation_candidate` currently turns a zero-task PM docs reconciliation into a synthetic `RECONCILE-*` task, then asks Senior and Worker to approve/execute it. This caused a 10.4k-token Worker call for docs PM had already reconciled.

## Decision Log
- Rust may commit only dirty files named in the PM reconciliation candidate and already accepted as tracking/control docs.
- Rust must run `git diff --check -- <named files>` before committing.
- If no named dirty control docs exist, or the commit/verification fails, preserve the current Senior-gated fallback.

## Context and Orientation
- PM fallback branch lives in `src/app.rs` after `run_project_manager`.
- Candidate construction lives in `planner_zero_work_reconciliation_candidate`.
- Control-doc commit helpers live near `commit_control_doc_edits`.
- Tests live in `src/app_tests.rs`.

## Plan of Work
Extract selected-control-doc commit support, add a `try_auto_commit_pm_control_doc_reconciliation` helper, call it before Senior batch approval, and test both the auto-commit and fallback cases.

## Concrete Steps
1. Refactor control-doc commit helper to accept an explicit file list.
2. Add PM reconciliation auto-commit helper with conservative tracking-doc checks.
3. Use it before the existing Senior approval branch.
4. Add tests for auto-commit without Senior/Worker and fallback when no named dirty file exists.

## Validation and Acceptance
- `cargo test pm_control_doc`
- `cargo test human_disposition`
- `cargo fmt -- --check`
- `cargo test`
- `cargo build --release`
- `git diff --check`

## Idempotence and Recovery
The auto-commit helper is no-op when there are no matching dirty control docs. Failed verification/commit returns to the existing Senior-gated path.

## Artifacts and Notes
- Triggering Helicon run: PM prompt ~10.4k, Worker prompt ~10.4k for `RECONCILE-1782761514616`.

## Interfaces and Dependencies
- No new CLI surface.
- Uses existing no-shell `git diff --check`/`git add`/`git commit` process boundaries.

## Outcomes & Retrospective
Implemented. PM zero-task control-doc reconciliation now auto-commits when all tracked dirty files are the PM-named control docs and `git diff --check -- <files>` passes. Rust records Senior and Worker skips with `pm_control_doc_reconciliation_auto_committed`. If there are no named dirty docs or any tracked dirty file is outside the named control-doc set, the existing Senior-gated fallback remains available.

Validation passed:
- `cargo test pm_control_doc`
- `cargo test human_disposition`
- `cargo fmt -- --check`
- `cargo test`
- `cargo build --release`
