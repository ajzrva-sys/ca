# BUG-254 — Stale retry boundary and stuck tool status

Status: COMPLETE (2026-09-25)
Last updated: 2026-09-25

## Goal

1. `/retry` must never fail with `retry boundary is not a user request` when a user request exists. Stale indices after history prepend or session replacement fall back to the real user message.
2. Long `await_job` waits must not leave the status bar on generic `Executing tools…`. Waiting on a managed job shows `Waiting for managed job…`.

## Evidence

- Screenshot: `Retry failed: retry boundary is not a user request` after `/retry`.
- Screenshot: managed job running with progress while status stays `Executing tools…` for a long `await_job`.
- `apply_session_history_result` prepends older messages without shifting `retry_boundary` / `active_turn_start_message_count`.
- `apply_session_state` replaces `state.messages` without clearing those indices.
- `prepare_retry_branch` errors when the stored index is not a `Role::User` message instead of falling back.
- `start_completed_tool_batch` always sets `Executing tools…` even for a single `await_job`.

## Design

- Validate the stored retry boundary against the live transcript; if it is not a user message, replay the latest user message instead of erroring.
- Shift retry indices when older history is prepended.
- Clear retry indices whenever the session message list is replaced.
- Name the tool wait in status: `await_job` → `Waiting for managed job…`; single other tool → `Executing <name>…`.

## Allowed files

- `src/main.rs`
- `docs/exec-plans/active/BUG-254-*.md` (this file)
- `CONTINUITY.md`, `NEXT_STEPS.md` (status only)

## Milestones

1. Focused failing tests for stale-boundary retry, history-shift, session-clear, and await_job status.
2. Implement the four invariants above.
3. Focused tests, `cargo test`, `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, release build/install per standing rule.

## Validation

- Focused unit tests in `src/main.rs`.
- Full `cargo test --locked` and lint/format.
- No network or live provider checks claimed.