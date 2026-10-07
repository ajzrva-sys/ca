# SES-039 — Saved Session Deletion

Status: COMPLETE
Owner/session: current Codex task
Last updated: 2026-08-10

## Goal

Let users permanently delete one inactive saved session from `/resume` through an explicit, failure-preserving confirmation.

## Non-Goals

- Do not delete the active session or cascade into branch descendants.
- Do not rewrite historical parent metadata or change the session format.
- Do not touch user session storage during validation.

## Evidence and Current Behavior

- `SessionStore` lists, loads, saves, and branches stable JSON files but has no deletion boundary.
- `/resume` supports filter/navigation/load only and does not retain an inline operation error.
- Ante documents session deletion as a workflow idea; this is a tested `ca` extension, not Cursor parity evidence.

## Scope

Allowed: `src/session.rs`, shared app/event state, `/resume` input/render paths, deterministic tests/snapshots, and harness documents.
Forbidden: oracle/reference edits, real session files, provider/config/credential changes.

## Design and Invariants

- `SessionStore::delete` accepts one validated storage-key component plus the expected UUID, re-reads the file, and unlinks exactly that JSON document.
- The current session is visibly marked and can never enter deletion confirmation.
- Confirmation defaults to Cancel, stays open on failure, and locks input while a blocking-worker deletion is pending.
- Success preserves the filter and maps selection to the row occupying the deleted row's previous visible position.
- Branch files and their parent metadata remain untouched.

## Milestones

- [x] Add the exact-file deletion API and filesystem tests.
- [x] Add confirmation/busy/result state and asynchronous orchestration.
- [x] Add protected-row rendering, navigation tests, and wide/narrow snapshots.
- [x] Run focused and completion validation, reconcile docs, and archive this plan.

## Acceptance Criteria

- Traversal, malformed keys, UUID mismatch, and active-session deletion fail without unlinking anything.
- Cancel is selected by default; deletion requires an explicit second choice and confirmation.
- Success refreshes predictably without clearing the filter or deleting descendants.
- Filesystem failure remains inline with the original row and confirmation intact.

## Validation

Focused session/input/render tests, then `cargo test --no-fail-fast`, format, strict Clippy, release build, and help/version smoke tests.

## Completion Record

Completed on 2026-08-10. Focused store, reducer, orchestration, and render tests passed, including active-session protection and failure recovery. Six wide/narrow picker/confirmation/error snapshots were reviewed. Repository gates passed with 406 unit tests and 23 CLI integration tests, formatting, strict Clippy, release build, and rebuilt help/version smoke tests.

## Recovery and Rollback

Revert only SES-039 files with targeted patches. There is no migration; deletion exists only behind explicit confirmation.
