# TUI-149 — Mimocode-Inspired Presentation Polish

Status: COMPLETE
Owner/session: Codex `/root`
Last updated: 2026-08-21

## Goal

Make `ca` easier to read and more satisfying to operate by adding a closed
Mimocode-inspired dark palette, a centered clipboard confirmation, a visibly
distinct Plan-mode composer, and stronger Markdown hierarchy for long agent
responses.

## Non-Goals

- No GUI, terminal-font control, downloadable/free-form theme, animation
  framework, provider behavior change, or new plan protocol.
- No source copied from Mimocode, Codex, Cursor, or repository oracles.
- No change to Plan-mode authority: it remains the existing read-only tool
  profile and still requires explicit plan approval before Agent mode.
- No oracle/reference edits, real profile mutation, binary installation, or
  live provider request.

## Evidence and Current Behavior

- `src/ui_settings.rs::UiTheme` already defines a closed persisted theme enum;
  `src/tui/render.rs::palette` applies it to principal chrome.
- Transcript selection and `/copy` already reach the local clipboard, but
  success is shown only as small status-bar text.
- `AgentMode::Plan` already selects `ToolProfile::Plan`, builds a structured
  proposal, and is reachable through `/plan` and Tab cycling. Its composer is
  visually almost identical to Agent mode.
- `src/markdown.rs` hard-codes colors, renders every list item as a bullet, and
  draws tables without an outer grid. Those choices flatten long structured
  answers even when another UI theme is active.
- The supplied screenshots are visual direction only: warm orange accents,
  periwinkle headings, pale-green Plan identity, dark surfaces, strong text
  hierarchy, and a centered copy acknowledgement.

## Scope

Allowed files:

- `src/ui_settings.rs`, `src/app.rs`, `src/main.rs`
- `src/markdown.rs`, `src/tui/{input,render,render_tests}.rs`
- focused snapshots and harness documentation

Forbidden files/areas:

- `oracles/`, `reference/`, provider/tool/session protocols, credentials,
  runtime configuration outside disposable tests, and installed binaries

Runtime state affected: none. Theme persistence remains opt-in through the
existing `/theme` manager; tests use in-memory state.

## Design and Invariants

- `mimocode` is one reviewed built-in theme identifier. Existing `default`,
  `ocean`, `amber`, and `mono` values deserialize and render unchanged.
- Clipboard success/failure uses one bounded host-only toast. It never contains
  copied text and naturally expires; status and model context remain untouched.
- Plan mode gets a pale-green mode accent, dark composer surface, explicit
  read-only title, and planning-specific placeholder. Authority and request
  construction do not change.
- Markdown receives semantic colors from the active UI theme. Ordered lists
  retain their numbers, tables fit the available width and draw a full grid,
  headings and strong text remain visually distinct, and terminal sanitation
  stays before Markdown rendering.

## Milestones

### M1 — Characterization and typed palette

- Add focused theme serialization and Markdown structure tests.
- Add the closed `Mimocode` palette without changing defaults.

### M2 — Clipboard and Plan presentation

- Add bounded toast state and render it last as a small centered overlay.
- Route transcript selection and `/copy` success/failure through the toast.
- Give Plan mode distinct title/composer/placeholder copy.

### M3 — Readable structured output

- Make assistant Markdown theme-aware.
- Preserve ordered-list numbering and render bounded full-grid tables.
- Add reviewed wide/narrow deterministic render snapshots.

### M4 — Validation and harness

- Run focused Markdown/input/render/config tests.
- Run formatting, strict Clippy, release build, snapshot hygiene, diff, and
  protected-tree checks; record any environment-blocked broad checks honestly.

## Acceptance Criteria

- Selecting `Mimocode` previews/persists a warm-orange, periwinkle, pale-green,
  dark-surface palette without affecting users who keep another theme.
- Successful selection or `/copy` renders `Copied to clipboard` in a centered
  accent-sided toast containing no copied bytes; failure is equally visible.
- Plan mode is unmistakable at title and composer while retaining read-only
  tools and existing approval flow.
- A response containing headings, bold text, inline code, ordered lists, and a
  table has deterministic, width-bounded visual hierarchy at 60 and 100 cells.
- Existing theme values, clipboard failure semantics, transcript selection,
  Plan authority, and terminal-text sanitation remain compatible.

## Validation

Focused: PASS — nine Markdown tests, focused clipboard/theme tests, and all
160 TUI render tests.

Completion: PASS — `cargo test --no-fail-fast` (1,710 unit tests, two
documented ignores, 37 CLI tests, 17 role-run CLI tests), strict all-feature
Clippy, the offline all-feature release build, formatting, snapshot hygiene,
and diff checks.

Manual/visual: reviewed deterministic 100×30 and 60×24 TestBackend snapshots;
native TUI walkthrough was not required.

Blocked checks: none.

## Progress

- [x] Existing theme, clipboard, Plan-mode, and Markdown boundaries inspected.
- [x] Mimocode palette and semantic Markdown colors implemented.
- [x] Clipboard toast and distinct Plan composer implemented.
- [x] Focused snapshots reviewed.
- [x] Completion gates recorded and plan reconciled.

## Decisions

- Decision: add a built-in theme instead of changing the default palette.
  Evidence: the existing theme contract promises missing `[ui]` preserves the
  established appearance.
  Consequence: users opt in with `/theme mimo`, while compatibility is exact.
- Decision: polish the existing Plan mode rather than create another mode.
  Evidence: `AgentMode::Plan` already owns the correct read-only authority and
  structured proposal lifecycle.
  Consequence: this task changes discoverability and presentation only.

## Surprises and Discoveries

- Plan mode was already complete functionally; its muted presentation made it
  appear less substantial than it is.
- Markdown ignored ordered-list starts, so structured procedures lost their
  original numbering before Ratatui wrapping.

## Recovery and Rollback

Remove the additive `Mimocode` enum variant and toast field, then restore the
theme-neutral Markdown entrypoint and composer styles. Existing saved themes
remain valid throughout; no migration or runtime data cleanup is needed.

## Completion Record

Changed files: UI settings/state, command and input copy surfaces, Markdown and
TUI renderers/tests/snapshots, plus the architecture/continuity/testing harness.

Commands and results: all focused and completion commands above pass.

Remaining risks: terminal color rendering depends on the user's emulator;
deterministic tests verify Ratatui cell colors and text at both target widths.

Harness updates: `NEXT_STEPS.md`, `ARCHITECTURE.md`, `CONTINUITY.md`,
`TESTING.md`, `docs/.INDEX.md`, and this completed ExecPlan.
