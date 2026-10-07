# TUI-145 — Simultaneous Trackpad Scroll and Text Selection

Status: COMPLETE
Owner/session: Codex / current implementation session
Last updated: 2026-08-19

## Goal

Keep the default trackpad/wheel scrolling behavior while making ordinary
left-button drag over the visible transcript produce a visible selection and
copy its exact text when released.

## Non-Goals

- Do not remove keyboard or trackpad scrolling, change transcript persistence,
  or alter provider/tool/session protocols.
- Do not depend on terminal-specific selection modifiers as the primary path.
- Do not add shell clipboard commands, inspect clipboard contents, install a
  binary, or mutate user runtime configuration.
- Do not modify `oracles/` or `reference/`.

## Evidence and Current Behavior

- Pre-fix, `src/tui/event.rs` used Crossterm `EnableMouseCapture`, which enables
  normal, drag, and any-motion reporting, but its event thread forwarded only
  wheel events and discarded left-button down/drag/up.
- Pre-fix, `src/tui/input.rs` therefore could not create a selection; its
  shortcut copy explicitly documented a terminal modifier or `mouse = "off"`
  workaround.
- `src/tui/render.rs` owns the exact final Ratatui cell buffer and conversation
  rectangle, so it can capture selectable visible text and render a selection
  without touching canonical messages.
- `arboard` is already a direct dependency and is already used by the TUI.

## Scope

Allowed files: TUI event/input/transcript/render code and tests, current
architecture/testing/continuity/queue documentation, and this plan.

Forbidden files/areas: provider/session schemas, runtime config contents,
installed binaries, `oracles/`, `reference/`, and Git history.

Runtime state affected: process-local visible selection and an explicit
clipboard write containing only the user-selected visible text.

## Design and Invariants

- Preserve wheel events and add typed left-button Start/Drag/End events with
  bounded terminal-cell coordinates.
- The selection model retains only a bounded snapshot of the visible
  conversation cells. It never stores hidden transcript history, provider
  state, credentials, or canonical messages.
- Selection is row-major, supports forward/reverse drags, clamps to the latest
  conversation rectangle, handles Unicode cell continuations, and omits
  unselected trailing padding.
- Rendering applies a reversible visual modifier after capturing the clean
  cells. Selection changes no text, transcript scroll position, or session.
- Mouse release copies through the existing in-process clipboard dependency;
  failure is visible and retains the selection. No shell process runs.
- `mouse = "off"` remains available for native terminal selection, but the
  default Scroll mode supports both scrolling and application-native selection.

## Milestones

### M1 — Characterize the discarded drag path

- Add event-mapping and bounded selection-model tests that fail before the new
  typed path exists.

### M2 — Implement selection and clipboard flow

- Forward left down/drag/up, capture/render visible conversation cells, and
  copy on release through an injectable helper.

### M3 — Validate and document

- Run focused event/input/transcript/render suites, full completion gates, and
  reconcile durable docs before moving this plan to completed.

## Acceptance Criteria

- In Scroll mode, wheel events retain existing behavior.
- A left drag across visible transcript cells visibly selects exact text and
  release copies it; reverse, multiline, Unicode, padding, and out-of-area
  cases are deterministic.
- Click/drag outside the conversation produces no copy and does not disturb
  composer/modal behavior.
- Clipboard failure is visible, non-destructive, and does not run a process.
- Mouse Off, suspend/resume, partial setup failure, and lifecycle cleanup remain
  unchanged.

## Validation

Focused: TUI event lifecycle/mapping, input selection/clipboard, transcript
selection model, and render tests.

Completion: `cargo test --no-fail-fast`, all-target/all-feature tests, `cargo
fmt --all --check`, strict Clippy, all-feature release, help/version, `git diff
--check`, snapshot hygiene, and oracle/reference diff checks.

Manual/visual: deterministic TestBackend selection-style inspection. A native
terminal walkthrough is optional and must not mutate runtime state.

Blocked checks: none. A native interactive terminal walkthrough was optional
and not run; deterministic event, injected clipboard, and TestBackend coverage
is the recorded acceptance evidence.

## Progress

- [x] Existing mouse capture, event discard, config workaround, and render boundary characterized.
- [x] Add failing event/selection characterizations.
- [x] Implement simultaneous scrolling and application-native selection.
- [x] Run completion gates and reconcile documentation.

## Decisions

- Decision: implement application-native selection rather than claiming the
  terminal can simultaneously own a drag while mouse reporting is enabled.
  Evidence: Crossterm enables DEC 1000/1002/1003 and the terminal sends the drag
  to `ca`; the current event loop discards it.
  Consequence: selection can coexist with wheel capture on supported terminals.
- Decision: copy automatically on release.
  Evidence: the terminal cannot receive its normal copy shortcut for a
  selection it does not own.
  Consequence: selected text is immediately available to paste and the status
  line reports success/failure.

## Surprises and Discoveries

- Crossterm mouse capture already supplied the required left down/drag/up
  coordinates; the event loop had discarded them. No terminal mode or
  persisted configuration change was required.
- The final Ratatui conversation buffer provides exact visible symbols and
  Unicode continuation cells, allowing selection without reconstructing
  Markdown or touching canonical transcript messages.

## Recovery and Rollback

Revert only TUI-145 hunks with a targeted patch. Do not reset the dirty
worktree. Process-local selection disappears on exit and has no durable
recovery requirement.

## Completion Record

Changed files: `src/tui/event.rs`, `src/tui/input.rs`,
`src/tui/transcript.rs`, `src/tui/render.rs`, `src/tui/render_tests.rs`,
`src/main.rs`, `ARCHITECTURE.md`, `CONTINUITY.md`, `TESTING.md`,
`NEXT_STEPS.md`, `docs/.INDEX.md`, and this plan.

Commands and results: event lifecycle PASS (9), transcript PASS (5), input
PASS (163), render PASS (158), default complete PASS (1,624 main + 37 CLI,
one ignored), all-target/all-feature complete PASS (1,653 main + 118
experiment + 37 CLI, two ignored), formatting PASS, strict Clippy PASS,
all-feature release PASS, local help/version PASS (`ca 0.1.0`), diff/snapshot
hygiene PASS, and oracle/reference immutability PASS.

Remaining risks: physical terminal mouse-reporting and clipboard integration
were not exercised interactively. The event translation, selection/copy
boundary, clipboard failure, and final rendered cells are deterministic and
covered without reading or mutating user runtime state.

Harness updates: `ARCHITECTURE.md` owns the selection/lifecycle invariants,
`TESTING.md` records actual gates, `CONTINUITY.md` records the durable outcome,
`NEXT_STEPS.md` links this completed plan, and `docs/.INDEX.md` indexes it.
