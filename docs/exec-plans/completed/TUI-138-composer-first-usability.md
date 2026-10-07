# TUI-138 — Composer-First Usability Overhaul

Status: COMPLETE
Owner/session: Codex implementation session
Last updated: 2026-08-17

Post-completion note (2026-08-18): TUI-139 changed only cursor presentation
from terminal-owned blinking to a deterministic white software block blink;
this plan's shared Unicode/display-cell placement remains unchanged.

## Goal

Make the composer the stable center of the terminal UI: multiline and Unicode
editing remains spatially correct, completions stay anchored to the draft,
attachments and large pastes are explicit atomic elements, transcript scrolling
does not yank the viewport, and the surrounding chrome adapts to the current
task without changing provider, approval, queue-timing, or session semantics.

## Non-Goals

- No GUI, Cursor-hosted service, provider-protocol change, or approval weakening.
- No new delayed next-tool steering mode; preserve only after-turn queueing and
  immediate interrupt-and-steer.
- No automatic workspace authority for dropped folders.
- No session-format or `PendingInput` serialization change.
- No oracle modification, source copying, or Cursor parity claim.

## Evidence and Current Behavior

- `src/tui/render.rs::render_input` renders the entire draft as one Ratatui
  `Line`, while `src/main.rs` sizes it only from hard newline count. Native
  cursor placement therefore cannot represent hard or soft wrapped rows.
- `src/tui/input.rs` sends Up/Down directly to history and Home/End to whole
  draft bounds; Vim motion does not share a visual layout model.
- Slash and attachment results are centered modal windows, hiding conversation
  context instead of remaining attached to the composer.
- `src/tui/render.rs::render_conversation` computes offsets from logical lines,
  then gives them to a wrapping `Paragraph`, so offsets diverge from displayed
  rows. Incoming output and session-history prepends share ad-hoc
  `scroll_offset`/`auto_scroll` state.
- `src/tui/event.rs` owns terminal raw/alternate-screen/bracketed-paste
  acquisition and reverse-order release, but does not own mouse capture.
- `oracles/cursor-CLI-reverse/` is read-only parity evidence within `PARITY.md`.
  `oracles/codex/` is read-only engineering inspiration only.
- Inferred: a sidecar composer state can add layout, viewport, atomic-range,
  and completion state while retaining `AppState::input` as the text source of
  truth and preserving session compatibility.

## Scope

Allowed files:

- `Cargo.toml`, `Cargo.lock`
- `src/app.rs`, `src/config.rs`, `src/keymap.rs`, `src/main.rs`,
  `src/ui_settings.rs`
- `src/queue.rs`
- `src/tui/**` excluding unrelated snapshots
- focused/new TUI snapshots required by this plan
- `ARCHITECTURE.md`, `CONTINUITY.md`, `NEXT_STEPS.md`, `TESTING.md`,
  `docs/.INDEX.md`, and this ExecPlan

Forbidden files/areas:

- `oracles/**`, `reference/**`, runtime configuration/session/provider files,
  installed binaries, Git history, and unrelated dirty-worktree changes

Runtime state affected: none during implementation and validation. Manual work
uses disposable PTYs and loopback-only fixtures.

## Design and Invariants

- `AppState::input` remains the draft source of truth. `ComposerUiState` stores
  only process-local visual state, preferred column, registered atomic ranges,
  paste payloads, and inline-completion state.
- One grapheme/display-width layout engine drives drawing, native cursor
  placement, visual-row motion, viewport overflow, and atomic boundaries.
- Registered ranges, never placeholder text alone, control paste expansion and
  atomic deletion. Large payloads expand before every persistence or dispatch
  boundary. Images remain process-only and block queue/stash operations without
  discarding the draft.
- Completion workers are generation-tagged; stale path or attachment work
  cannot reopen or mutate a newer active token.
- Transcript position is process-only and bottom-relative. Offset zero means
  follow mode and clears unseen feedback.
- Mouse capture is best-effort and lifecycle-owned. Acquisition and release are
  ordered, partial failures recover, and `mouse = "off"` restores ordinary
  terminal selection behavior.
- Existing provider requests, approvals, aliases, `/raw`, `/history`, `/queue`,
  `/help`, suspend behavior, and queue timing remain compatible.

## Milestones

### M1 — Characterization and shared state

- Record the dirty-tree baseline and focused render/input baseline.
- Add direct `unicode-segmentation`, the active plan, and focused failing tests
  for wrapped cursor, visual motion, and seven-row viewport behavior.
- Validation: focused composer/layout tests.

### M2 — Unicode composer and atomic elements

- Add shared grapheme-aware layout and process-local composer UI state.
- Route native cursor, visual movement, line-local boundaries, history draft
  restoration, Vim motion, and seven-row viewport through the shared layout.
- Render and edit path/reference/paste/image elements atomically; expand large
  paste payloads at dispatch/persistence boundaries and reject image queueing.
- Validation: composer, input, queue, stash, history, and render tests.

### M3 — Inline completion and shared list behavior

- Add composer-anchored slash, attachment, and path surfaces with token-scoped
  dismissal and generation-safe asynchronous results.
- Add explicit slash Enter behavior metadata and shared list navigation/row
  layout for inline results and the named general pickers.
- Validation: completion, stale-result, narrow-layout, and picker tests.

### M4 — Adaptive chrome, queue clarity, transcript, and mouse

- Replace startup/logo and permanent shortcut chrome with compact adaptive
  title/footer content and an empty-composer `?` shortcut browser.
- Clarify after-turn queue previews, restored-review state, and last-queued edit.
- Replace transcript scroll fields with bottom-relative wrapped-row state.
- Add UI mouse configuration and best-effort terminal lifecycle capture/routing.
- Validation: chrome, queue, scroll, event lifecycle, and 100x30/60x24 snapshots.

### M5 — Completion gates and durable handoff

- Run focused, default, all-target/all-feature, formatting, strict Clippy,
  release, released help/version, diff, snapshot, and oracle/reference hygiene
  checks.
- Perform a disposable-PTY loopback walkthrough where supported and record any
  unavailable visual check honestly.
- Reconcile harness documents and archive this plan to `completed/`.

## Acceptance Criteria

- Hard newlines, word wrapping, exact-width wrapping, combining sequences, and
  wide graphemes produce the correct native cursor row/cell and visual motion.
- The composer shows at most seven text rows with `↑N`/`↓N` overflow feedback;
  history begins only beyond visual top/bottom and restores the saved draft.
- Home/End are hard-line local; Ctrl+A/Ctrl+E remain whole-draft; Vim motions
  use the same boundaries.
- Reference/path/paste/image elements render compactly and delete atomically;
  large paste payloads survive every send/persistence route exactly; images
  cannot be queued or stashed and do not disappear on refusal.
- `/`, `@`, and path completions stay composer-anchored and token-scoped. Tab
  completes only; Enter runs no-argument slash commands and completes the five
  argument-requiring commands. Stale worker results are ignored.
- Startup uses two compact working-context lines. Footer content changes for
  typing, active turn, completion, and scrolled transcript states. `?` opens
  shortcuts only for an empty composer.
- Queue previews say `After this turn`, omit UUIDs, show three plus an overflow
  count, distinguish restored review-required entries, and Alt+Up edits the
  last queued item only into an empty composer.
- Page movement, top/bottom jumps, unseen feedback, lazy-history prepend, and
  return-to-follow use displayed wrapped rows without viewport yanking.
- Mouse scroll routing honors focused popup/picker/pager/transcript, ignores
  clicks and horizontal movement, and survives unsupported capture, toggling,
  suspend/resume, panic cleanup, and Drop cleanup.

## Validation

Focused baseline: `cargo test tui:: --no-fail-fast` — PASS (353 TUI tests;
1,209 filtered, 0 failed).

Completion: PASS. Focused composer/input/completion/queue/stash/config,
terminal-lifecycle, transcript, and render coverage passed. The default suite
passed 1,594 main and 37 CLI tests with one documented ignore. The complete
all-target/all-feature rerun passed 1,623 main, 118 experiment, and 37 CLI
tests with two documented ignores. Formatting, strict Clippy, all-feature
release, released help/version, diff, snapshot, and oracle/reference hygiene
checks passed.

Manual/visual: PASS for reviewed deterministic 100×30/60×24 startup,
multiline/overflow, chip, slash/attachment/path, migrated-picker, footer,
queue, and transcript snapshots. The native disposable PTY walkthrough is
BLOCKED by the binding rule that forbids overriding `HOME`; a bare no-session
attempt stopped on the existing credential startup check without mutation.
Finder drag and physical trackpad behavior are not claimed.

Blocked checks: native isolated PTY only, for the safety conflict above.

## Progress

- [x] Active ExecPlan created with locked product decisions and safety bounds.
- [x] Baseline input/render behavior characterized.
- [x] Unicode composer and atomic-element milestone complete.
- [x] Inline completion and shared list milestone complete.
- [x] Adaptive chrome, queue, transcript, and mouse milestone complete.
- [x] Completion gates pass and harness is reconciled.

## Decisions

- Decision: keep `AppState::input` and `input_cursor` as compatibility anchors,
  adding sidecar UI state rather than replacing all composer call sites.
  Evidence: input dispatch, command handling, history, queueing, external editor,
  and session paths currently use the string directly.
  Consequence: behavior can be improved incrementally without a repository-wide
  state migration.
- Decision: slash Enter behavior is explicit command metadata, never inferred
  from human-readable usage text.
  Evidence: required-completion command set is a locked product decision.
  Consequence: aliases and help formatting cannot accidentally change execution.
- Decision: absent `[ui] mouse` configuration defaults to `scroll`.
  Evidence: the approved plan selects trackpad-default behavior and an explicit
  `off` escape hatch.
  Consequence: terminal selection may require the terminal modifier until users
  choose `mouse = "off"`.

## Surprises and Discoveries

- The implementation starts from a large pre-existing dirty tree containing
  earlier completed TUI work and snapshots. TUI-138 must not normalize or revert
  those changes.
- Exact transcript row measurement is available through Ratatui's
  `unstable-rendered-line-info` feature, avoiding a second approximation of the
  paragraph wrapper.
- The first concurrent all-feature completion run hit one unrelated
  timing-sensitive voice descendant-cleanup assertion. The exact isolated test
  and the complete all-feature rerun both passed.
- `bare --no-session` still reads the ordinary provider configuration by
  design, so a truly isolated native walkthrough would require a disposable
  `HOME`. Binding repository rules forbid that override; the check is recorded
  as blocked rather than weakening the safety boundary.

## Recovery and Rollback

Use targeted inverse `apply_patch` edits only. Never use reset, checkout,
recursive deletion, or broad formatting. If an overlap with pre-existing user
work cannot be isolated, stop and report the exact file/symbol conflict.

## Completion Record

Changed files: `Cargo.toml`/`Cargo.lock`; composer, selection, transcript,
event/input/render state and tests under `src/tui/`; orchestration/config/UI/
keymap/queue integration in `src/{app,config,keymap,main,queue,ui_settings}.rs`;
reviewed TUI snapshots; and the repository harness documents named in Scope.
Pre-existing unrelated dirty-tree changes were preserved.

Commands and results: focused checks PASS; `cargo test --no-fail-fast` PASS
(1,594 main, 37 CLI, one ignored); complete `cargo test --all-targets
--all-features --no-fail-fast` rerun PASS (1,623 main, 118 experiment, 37 CLI,
two ignored); `cargo fmt --all --check`, strict all-target/all-feature Clippy,
and `cargo build --release --all-features` PASS; released help/version PASS as
`ca 0.1.0`; render/snapshot, diff, and oracle/reference hygiene PASS.

Remaining risks: physical Finder drag, terminal-specific selection modifier,
and trackpad behavior were not manually exercised because the isolated native
PTY prerequisite conflicts with the binding `HOME` guard. Deterministic event,
lifecycle, native-cursor, and render tests cover the implementation contract.

Harness updates: `ARCHITECTURE.md`, `CONTINUITY.md`, `NEXT_STEPS.md`,
`TESTING.md`, and `docs/.INDEX.md` describe the completed state and validation;
this plan is archived in `docs/exec-plans/completed/`.
