# TUI-136 — Bracketed Paste for Folder Drops

Status: COMPLETE
Owner/session: current Codex session
Last updated: 2026-08-17

## Goal

A path dropped from Finder into the idle composer is received as one paste and
inserted into the prompt instead of opening the slash-command palette.

## Non-Goals

- Add a GUI or a platform-specific drag-and-drop API.
- Automatically grant an outside directory workspace authority.
- Change the existing `/` command shortcut or `@` file attachment picker.
- Modify user configuration, sessions, installed binaries, or oracle evidence.

## Evidence and Current Behavior

- `src/tui/event.rs` converts Crossterm paste events into the internal
  `Event::Paste`, but terminal setup never enables bracketed-paste mode.
- `src/tui/input.rs` already inserts `Event::Paste` text directly into the
  composer without applying the single-key `/` palette shortcut.
- The supplied screenshot shows a dropped absolute path opening Commands and
  placing the path minus its leading slash in the palette filter. This is the
  exact result of receiving the drop as individual key events.
- Inferred: Finder/terminal path insertion will be reported as a single
  Crossterm paste event once bracketed-paste mode is active.

## Scope

Allowed files:

- `src/tui/event.rs`
- `src/tui/input_tests.rs` only if additional composer characterization is needed
- `docs/exec-plans/{active,completed}/TUI-136-bracketed-paste-folder-drop.md`
- `ARCHITECTURE.md`, `CONTINUITY.md`, `NEXT_STEPS.md`, `TESTING.md`,
  `docs/.INDEX.md`

Forbidden files/areas:

- `oracles/`, `reference/`, user runtime state, installed binaries, Git history

Runtime state affected: terminal modes are process-scoped and restored on
suspend, failure, panic, and normal exit.

## Design and Invariants

Enable bracketed paste only while the TUI owns raw alternate-screen input.
Track that mode independently in the terminal lifecycle so every partial setup,
suspend, resume, panic, and drop path releases exactly the state acquired. Keep
the existing input state machine unchanged: an ordinary typed `/` still opens
Commands, while `Event::Paste("/path")` inserts literal prompt text.

## Milestones

### M1 — Characterize lifecycle and composer behavior

- Extend lifecycle tests to require bracketed paste acquisition and reverse-
  order release.
- Preserve or add a focused composer test proving a pasted leading slash does
  not open Commands.
- Validation: focused TUI event/input tests.

### M2 — Implement terminal-mode ownership

- Enable/disable Crossterm bracketed paste through `TerminalLifecycle`.
- Preserve cleanup under partial failures, suspend/resume, panic, and Drop.
- Validation: focused lifecycle tests and TUI input tests.

### M3 — Complete local validation and handoff

- Run repository completion gates required by `TESTING.md`.
- Record exact results in the harness and move this plan to completed.

## Acceptance Criteria

- A paste containing `/Users/example/project/folder ` in an empty composer
  leaves the slash palette closed and inserts the exact path.
- Typing `/` in an empty composer still opens the slash-command palette.
- Bracketed paste is disabled before leaving the alternate screen or disabling
  raw mode on suspend, setup recovery, panic, and normal exit.
- Focused tests, `cargo test`, formatting, strict Clippy, and release build pass,
  or any blocked check is recorded accurately.

## Validation

Focused:

- `cargo test tui::event::lifecycle_tests --no-fail-fast`: PASS, seven tests.
- `cargo test pasted_absolute_folder_path_stays_in_composer_without_opening_commands --no-fail-fast`:
  PASS, one focused input regression.

Completion:

- `cargo test --no-fail-fast`: PASS, 1,560 main and 37 CLI tests; one ignored.
- `cargo test --all-targets --all-features --no-fail-fast`: PASS, 1,589
  main, 118 experiment, and 37 CLI tests; two ignored.
- `cargo fmt --all --check`: PASS.
- `cargo clippy --all-targets --all-features -- -D warnings`: PASS.
- `cargo build --release --all-features`: PASS.
- Release help/version (`ca 0.1.0`), `git diff --check`, snapshot hygiene,
  and oracle/reference diff check: PASS.

Manual/visual: not run. A native Finder drop requires an interactive terminal;
the deterministic lifecycle and composer regressions cover the terminal-mode
handshake and exact resulting input event without mutating runtime state.

Blocked checks: none.

## Progress

- [x] Current failure traced to missing bracketed-paste terminal mode.
- [x] Lifecycle and composer regressions added.
- [x] Implementation complete.
- [x] Completion checks recorded.

## Decisions

- Decision: fix the terminal event boundary rather than weakening slash-command
  recognition.
  Evidence: paste handling already has the desired literal insertion behavior;
  only the terminal-mode handshake is absent.
  Consequence: typed command UX is unchanged and all paste sources benefit.

## Surprises and Discoveries

- The composer already handled an internal `Event::Paste` correctly; only the
  terminal's bracketed-paste mode was missing.

## Recovery and Rollback

Revert only the TUI-136 hunks in `src/tui/event.rs` and the harness entries.
Do not reset the dirty worktree because it contains earlier user-owned work.

## Completion Record

Changed files: `src/tui/event.rs`, `src/tui/input_tests.rs`, `ARCHITECTURE.md`,
`CONTINUITY.md`, `NEXT_STEPS.md`, `TESTING.md`, `docs/.INDEX.md`, and this plan.

Commands and results: all focused and local completion checks listed above pass.

Remaining risks: native Finder drag was not manually exercised in this session;
terminal emulator support for standard bracketed paste remains an external
compatibility dependency.

Harness updates: terminal lifecycle ownership and validation evidence recorded;
TUI-136 is complete and no follow-up task is required.
