# TUI-137 — Native Composer Cursor

Status: COMPLETE
Owner/session: current Codex session
Last updated: 2026-08-17

Post-completion note (2026-08-18): TUI-139 supersedes the terminal-owned
presentation decision with a deterministic white software block blink. Exact
Unicode/display-cell placement and modal-focus suppression remain in force.

## Goal

Render the idle composer cursor through the terminal's native cursor at the
exact insertion cell so it uses the user's full cursor shape and native blink,
matching the supplied Codex observations.

## Non-Goals

- Copy Codex oracle source or claim Cursor parity.
- Change text editing, cursor byte semantics, or slash/attachment behavior.
- Add configurable cursor shapes or a custom software blink timer.
- Modify user runtime state, install a binary, or edit oracle/reference files.

## Evidence and Current Behavior

- The supplied `ca` screenshot shows a permanently rendered narrow `▌` glyph
  after end-of-input text.
- The two supplied Codex screenshots show the same end insertion cell with the
  terminal cursor alternately hidden and visible as a full block.
- `src/tui/render.rs::render_input` draws `▌` into the ratatui buffer at the end
  and reverse-styles the next character in the middle; it never requests a
  native frame cursor position.
- Read-only secondary Codex evidence exposes a composer cursor position to its
  terminal frame and uses the terminal's default user cursor shape. This is an
  engineering lesson only; implementation will be derived independently from
  the observed behavior and local ratatui API/tests.
- Ratatui 0.29 shows and positions the terminal cursor when
  `Frame::set_cursor_position` is called, otherwise it hides it.

## Scope

Allowed files:

- `src/tui/render.rs`
- `src/tui/render_tests.rs`
- `src/tui/test_support.rs`
- `docs/exec-plans/{active,completed}/TUI-137-native-composer-cursor.md`
- `ARCHITECTURE.md`, `CONTINUITY.md`, `NEXT_STEPS.md`, `TESTING.md`,
  `docs/.INDEX.md`

Forbidden files/areas:

- `oracles/`, `reference/`, user runtime state, installed binaries, Git history

Runtime state affected: none beyond the process-owned terminal cursor already
managed by ratatui during each draw.

## Design and Invariants

Render only prompt/input/placeholder text into the buffer. Compute the cursor's
display column from the UTF-8-safe byte offset using terminal display width,
then request that cell through the ratatui frame. The terminal owns shape and
blink. Keep deterministic text snapshots readable by representing a native
end cursor as `▌` only inside test serialization; separately assert the real
buffer contains no software glyph and the backend cursor position is exact.

## Milestones

### M1 — Characterize native cursor behavior

- Add focused TestBackend coverage for empty, ASCII-end, and Unicode-middle
  cursor positions.
- Assert the rendered buffer contains no `▌` software glyph.
- Validation: focused render tests fail against the current software cursor.

### M2 — Implement native cursor placement

- Remove software cursor spans from the composer.
- Compute bounded display-cell coordinates and call the ratatui frame cursor
  API.
- Preserve deterministic snapshot cursor notation in test support.
- Validation: focused render and existing Unicode cursor snapshot tests.

### M3 — Complete validation and handoff

- Run the repository completion gates in `TESTING.md`.
- Record exact evidence and move this plan to completed.

## Acceptance Criteria

- With `over` and no trailing space, the native cursor position is the blank
  cell immediately after `r`.
- The composer buffer contains no cursor glyph or forced cursor colors.
- The terminal controls visible/hidden blink frames and uses its configured
  default cursor shape.
- Cursor placement remains correct for empty input and a Unicode cursor before
  a wide character.
- Focused tests, full tests, formatting, strict Clippy, and release build pass,
  or any blocked check is recorded accurately.

## Validation

Focused:

- `cargo test composer_uses_native_cursor_at_empty_ascii_end_and_unicode_middle
  -- --nocapture`: PASS. The pre-implementation characterization first failed
  at `(0,0)` versus expected `(3,27)`, then passed for empty, ASCII end,
  Unicode middle, and modal-hidden cases after implementation.
- `cargo test tui::render_tests --no-fail-fast`: PASS, 150 tests.

Completion:

- `cargo test --no-fail-fast`: PASS, 1,561 main and 37 CLI tests; one ignored.
- `cargo test --all-targets --all-features --no-fail-fast`: PASS, 1,590 main,
  118 experiment, and 37 CLI tests; two ignored.
- `cargo fmt --all --check`: PASS.
- `cargo clippy --all-targets --all-features -- -D warnings`: PASS.
- `cargo build --release --all-features`: PASS.
- released `--help`, released `--version` (`ca 0.1.0`), `git diff --check`,
  snapshot hygiene, and oracle/reference diff checks: PASS.

Manual/visual: not run. The supplied screenshots establish the target, and the
deterministic TestBackend proves runtime cursor cell/visibility. Blink cadence
and cursor shape are terminal-owned and are not reliably represented by a
static capture.

Blocked checks: none.

## Progress

- [x] Current software-cursor path and Codex native-cursor behavior identified.
- [x] Failing native cursor characterization added.
- [x] Implementation complete.
- [x] Completion checks recorded.

## Decisions

- Decision: use the terminal's native cursor rather than imitate blink with a
  second application timer.
  Evidence: the supplied visible/hidden frames and Codex's frame-level cursor
  position behavior both show terminal-owned blinking and shape.
  Consequence: the cursor follows the user's terminal settings and no extra
  redraw loop is introduced.

## Surprises and Discoveries

- A native cursor is composited by the terminal above the final frame, unlike
  the old buffer glyph which a later modal could overwrite. Composer cursor
  publication therefore requires an explicit modal-focus guard.
- Text snapshots do not encode backend cursor position. Test serialization now
  derives cursor notation separately while the focused backend assertion proves
  the runtime buffer itself has no software composer glyph.

## Recovery and Rollback

Revert only TUI-137 hunks and harness entries. Do not reset the dirty worktree,
which contains earlier user-owned changes.

## Completion Record

Changed files: `src/tui/render.rs`, `src/tui/render_tests.rs`,
`src/tui/test_support.rs`, this plan, `ARCHITECTURE.md`, `CONTINUITY.md`,
`NEXT_STEPS.md`, `TESTING.md`, and `docs/.INDEX.md`.

Commands and results: all focused, render, default, all-feature, formatting,
strict Clippy, release, help/version, diff, snapshot, and oracle/reference gates
listed above passed.

Remaining risks: the exact cursor shape and blink cadence follow the user's
terminal configuration by design. A terminal that is configured with a bar
cursor will display a bar rather than forcing Codex's photographed block.

Harness updates: task marked complete, durable architecture/continuity/testing
facts recorded, and the completed plan indexed. `PARITY.md` remains unchanged
because Codex was secondary engineering evidence rather than Cursor parity.
