# QUE-012 — Visible Queue, Editing, Steering, and Interruption

Status: COMPLETE (2026-08-09)
Owner/session: current Codex roadmap implementation
Last updated: 2026-08-09

## Goal

Implement a deterministic queue/steer/interrupt state machine with stable pending-input IDs, explicit editing state, visible previews, and safe provider-turn transitions.

## Non-Goals

- Do not add persistent queues or background jobs.
- Do not change approval policy or structured-question semantics.
- Do not redesign the provider protocol beyond removing unresolved call metadata during steering.

## Scope

Allowed: new queue state module, app/input/render/main command orchestration, focused tests/snapshots, harness docs.
Forbidden: oracle/reference edits, runtime user state, unrelated shell/job implementation owned by ASY-006.

## Design and Invariants

- `InputQueue` owns ordered `PendingInput` values, selection, and editing ID.
- `TurnState` explicitly distinguishes active, idle/success, cancelled, and failed outcomes.
- Only successful, fully resolved turns may pop one queued item.
- Steering aborts active stream/tool work, sanitizes partial assistant state, removes an edited queued item by ID, appends the steering message, and starts a fresh provider generation.
- Question, approval, plan, and overlay handlers keep precedence over queue shortcuts.

## Completed Milestones

### M1 — Pure queue state

- Added stable items, ordering/edit/delete/reorder APIs, explicit turn outcome policy, and five unit tests.

### M2 — Event-loop transitions

- Busy Enter queues; Ctrl+S steers; Ctrl+C aborts tracked stream/tool/approval work; draining occurs only after normal terminal completion.

### M3 — Queue UI

- Added `/queue`, full-screen interaction, three-item composer preview, and four wide/narrow snapshots.

### M4 — Validation and handoff

- Focused/full tests, targeted formatting, release build, and local smoke pass.
- Global formatting and strict clippy remain red only on the recorded pre-existing baseline.

## Acceptance Results

- Ordering, edit/delete/reorder, steering cancellation, partial text preservation, approval/question precedence, failed/cancelled no-drain behavior, and previews have deterministic coverage.
- Provider conversion succeeds after steering because unresolved assistant tool-call metadata is removed.
- Existing live TUI PID 51440 was not restarted or terminated.

## Validation Evidence

- Pure queue suite: PASS, 5 tests.
- Production turn-transition tests: PASS, 4 tests.
- Focused input queue/steering tests: PASS, 3 tests.
- Queue preview/view snapshots: PASS, 4 reviewed snapshots.
- `cargo test`: PASS, 128 tests.
- `rustfmt --edition 2021 --check src/queue.rs`: PASS.
- `cargo fmt --check`: FAIL on pre-existing repository-wide drift.
- `cargo clippy --all-targets -- -D warnings`: FAIL on the same three pre-existing diagnostics; no QUE diagnostic remains.
- `cargo build --release`: PASS.
- `./target/release/ca --version`, `--help`, and live PID check: PASS.

## Recovery and Rollback

Revert only QUE-012 changes. Do not drain or mutate any user queue/session as part of validation.
