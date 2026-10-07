# TUI-015 — Selection-Friendly Terminal Lifecycle

Status: COMPLETE
Owner/session: Codex `/root`
Last updated: 2026-08-09

## Goal

Leave mouse selection to the terminal until pointer handling exists, while guaranteeing acquired raw/alternate-screen state is released on all unwind-safe exits and partial initialization failures.

## Non-Goals

- Mouse hit testing, pointer interaction, or selection ownership.
- Visual layout changes or Cursor parity claims.
- Signal handlers beyond existing process behavior.

## Evidence and Current Behavior

- `src/tui/event.rs::Tui::new` executes `EnableMouseCapture`, but its event loop handles only key, resize, and paste events.
- `enable_raw_mode` runs before alternate-screen and terminal construction; a later `?` can return before `Tui::drop` exists.
- `Tui::drop` sends `DisableMouseCapture` despite no handled mouse state.

## Scope

Allowed files: `src/tui/event.rs`, focused tests, and harness docs.

Forbidden: oracle/reference trees, running TUI/processes, runtime config/session/history, accessibility work.

Runtime state affected: none; lifecycle tests use a fake terminal-control adapter.

## Design and Invariants

- A small RAII guard owns raw-mode and alternate-screen acquisition flags.
- Acquisition is ordered raw then alternate screen; cleanup is reverse ordered and best effort.
- The guard exists before any subsequent fallible terminal construction.
- No mouse capture command is issued until a future pointer-mode task adds hit testing.
- Rust unwinding drops the guard; process aborts remain outside Rust cleanup guarantees.

## Milestones

### M1 — Characterization
- Add fake-control tests for successful acquisition, partial failure, normal drop, and panic unwinding.
- Confirm no mouse operation exists in the lifecycle contract.

### M2 — Implementation
- Move terminal setup/teardown into the guard and store it in `Tui`.
- Remove mouse capture enable/disable commands.

### M3 — Validation
- Run focused tests, full suite, format, strict Clippy, release, and local help/version.
- Reconcile task/harness docs and move this plan to completed.

## Acceptance Criteria

- Fake lifecycle traces are exactly `raw-on, alt-on, alt-off, raw-off` on success/drop and clean up only acquired state on failures.
- Catch-unwind coverage observes teardown after a panic.
- Source contains neither `EnableMouseCapture` nor `DisableMouseCapture`.
- Existing tests and build gates retain their documented outcomes.

## Validation

Focused: PASS — 3 fake lifecycle tests cover success/drop, alternate-screen setup failure, and panic unwinding.

Completion: full tests (165 unit + 3 CLI), format, release, help, and version PASS; strict Clippy reports only the 3 pre-existing errors.

Manual/visual: not run against the live TUI; no visible layout change.

Blocked checks: none.

## Progress

- [x] Inspected lifecycle/event ownership and claimed TUI-015.
- [x] Added red lifecycle tests.
- [x] Implemented RAII lifecycle and removed mouse capture.
- [x] Completed validation and harness updates.

## Decisions

- Decision: model terminal acquisition behind an injected test adapter.
  Evidence: real raw-mode tests would mutate the controlling terminal and are not hermetic.
  Consequence: failure and unwind ordering can be tested without touching the running TUI.

## Surprises and Discoveries

- Storing the lifecycle guard after the terminal/event fields ensures it tears down after those owners are dropped during normal struct destruction.

## Recovery and Rollback

Apply an inverse patch only to `src/tui/event.rs` and task documentation.

## Completion Record

Changed files: `src/tui/event.rs`, `NEXT_STEPS.md`, `TESTING.md`, `CONTINUITY.md`, and this plan.

Commands and results: focused/full tests, format, release, and CLI smoke PASS; strict Clippy retains the known 3 unrelated findings.

Remaining risks: process aborts cannot run Rust destructors; future pointer support must explicitly reintroduce capture together with hit testing.

Harness updates: `NEXT_STEPS.md`, `TESTING.md`, and `CONTINUITY.md` reconciled.
