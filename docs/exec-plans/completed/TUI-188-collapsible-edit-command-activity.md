# TUI-188 — Collapsible Edit and Command Activity

Status: COMPLETE
Owner/session: current local implementation session
Last updated: 2026-08-26

## Goal

Render consecutive direct file mutations and shell commands as one compact,
status-aware TUI activity block that expands with Ctrl+O or a mouse click while
preserving the existing detailed outputs.

## Non-Goals

- No provider, tool-schema, approval, session-format, or canonical-output change.
- No keyboard row focus, clickable file rows, configuration flag, or install.
- No grouping of exploration, MCP, mail, browser, subagent, or other ordinary tools.

## Evidence and Current Behavior

- `src/tui/render.rs` groups registry-classified exploration calls, but renders
  direct file tools and `run_command` independently.
- `TranscriptBlockKey` and `expanded_blocks` already provide stable Ctrl+O
  state keyed by the first call ID.
- `ToolExecutionResult::SuccessMutation` carries an exact process-only
  `FileMutationReceipt`; only the undo stack currently retains it.
- `TranscriptScrollState` captures the rendered transcript for bounded
  drag-selection, and left-click without movement currently has no action.
- The user-supplied screenshot is visual guidance for a compact activity
  header and expanded file/command rows. No Cursor parity claim is made.

## Scope

Allowed files: focused registry, app/process state, main result retention,
transcript input/rendering, targeted tests/snapshots, and harness documents.

Forbidden files/areas: `oracles/`, `reference/`, provider/session protocols,
runtime configuration, user sessions, installed binaries, and unrelated
already-modified snapshots.

Runtime state affected: process-only activity metadata, expansion state, and
render-time mouse hit regions.

## Design and Invariants

- Registry presentation distinguishes file activity, command activity,
  exploration, and ordinary calls. Two or more consecutive activity calls form
  a group across tool-only assistant messages; visible content, another tool
  class, or a user boundary closes it.
- A group uses its first call ID as the stable expansion key. Standalone calls
  retain their current rendering.
- Live file labels come from retained mutation receipts. Restored history uses
  canonical tool names/arguments/output conservatively and says `Changed` when
  create-versus-edit cannot be proven.
- Collapsed groups show one bounded status-aware line. Expanded groups show
  chronological rows and reuse existing diff, semantic-command, and adaptive
  output rendering.
- Render-time hit targets bind only the visible group header to its stable key.
  A release without drag toggles that key; selection, pagers, and overlays win.
- Canonical `ToolCall.output`, model results, saved sessions, `/output full`,
  and privacy-safe whole-transcript copy remain unchanged.

## Milestones

### M1 — Characterization

- Add failing registry/grouping/render/mouse tests for mixed activity,
  boundaries, restored fallback, Unicode width, Ctrl+O, and click-versus-drag.
- Validation: focused TUI and registry test filters fail for the missing feature.

### M2 — Activity state and grouping

- Add internal activity classification and process-only mutation presentation.
- Build deterministic activity groups for completed and partial calls.
- Validation: focused pure grouping and retention tests pass.

### M3 — Rendering and interaction

- Add compact/expanded rendering and exact visible mouse hit targeting.
- Preserve existing standalone, exploration, diff, semantic-output, and copy behavior.
- Validation: focused input, transcript, and render suites pass.

### M4 — Completion

- Review only TUI-188 snapshots and reconcile harness documents.
- Run the serial workspace suite, formatting, strict all-feature Clippy,
  release build, and diff/snapshot/protected-tree hygiene.

## Acceptance Criteria

- Two or more consecutive file/command calls collapse under one grammatical
  header and expand to complete ordered details.
- Ctrl+O targets the latest group; clicking any visible group header toggles
  that exact group; dragging text never toggles.
- Created, Edited, Deleted, and Created directory labels are exact for live
  runs and conservative after restore.
- Exploration and standalone calls retain their existing behavior.
- All rendered rows remain within terminal-cell bounds at 60, 100, and 184 columns.

## Validation

Focused:

- `cargo test -p ca activity_ -- --test-threads=1`: PASS, 19 matching tests.
- `cargo test -p ca tui::render_tests -- --test-threads=1`: PASS, 197 tests.
- `cargo test -p ca tui::input_tests -- --test-threads=1`: PASS, 107 tests.
- `cargo test -p ca tui::transcript::tests -- --test-threads=1`: PASS, eight tests.

Completion:

- `cargo test --workspace --no-fail-fast -- --test-threads=1`: PASS, 2,055 CA
  tests, 38 CLI tests, 17 role-run tests, 15 watcher tests, three watcher CLI
  tests, 556 embedded scheduler tests, and all integration/doc targets; three
  documented manual/external checks ignored.
- `cargo fmt --all -- --check`: PASS.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: PASS.
- `cargo build --workspace --release --all-features`: PASS.
- Release help/version, `git diff --check`, zero `.snap.new` files, and clean
  protected-tree status: PASS.

Manual/visual: nine targeted collapsed, expanded, running, failed, and
file-only snapshots at 60, 100, and 184 columns were reviewed individually.
They preserve the screenshot's compact header/expanded-row direction while
remaining terminal-native and width-safe.

Blocked checks: none. The three workspace ignores are documented manual or
external integration checks unrelated to TUI-188.

## Progress

- [x] Repository harness and current grouping/output/mouse paths inspected.
- [x] Failing focused characterization tests recorded: `cargo test -p ca
  activity_ -- --test-threads=1` failed because `ToolPresentation::Activity`,
  `ToolActivityKind`, and `activity_groups` do not yet exist.
- [x] Activity classification and exact process-only labels implemented.
- [x] Compact/expanded rendering and mouse hit targeting implemented.
- [x] Targeted snapshots reviewed.
- [x] Completion gates recorded and harness reconciled.

## Decisions

- Decision: group only runs of at least two eligible calls.
  Evidence: preserves the existing useful standalone-command output while
  matching the multi-operation screenshot.
  Consequence: a lone edit or command is rendered exactly as before.
- Decision: only the header is clickable; file rows are styled but inert.
  Evidence: the user requested mouse compact/expand, not file navigation.
  Consequence: no new focus or external-editor surface is required.

## Surprises and Discoveries

- Automatic pre-resolved mutation batches originally recorded undo receipts
  without retaining the new process-only display receipt. The shared retention
  boundary now records both atomically for every successful mutation path.
- Existing privacy-safe copy-all text intentionally retains ordinary tool-call
  argument truncation; grouping is disabled for that projection so TUI-188 does
  not alter its established behavior.
- Strict Clippy required the grouping toggle to use an explicit `if` branch;
  the readability correction has no behavior impact.

## Recovery and Rollback

Remove only TUI-188 additions and targeted snapshots with an inverse patch.
Do not reset or overwrite the dirty worktree.

## Completion Record

Changed files: focused presentation-registry, app/process state, result
retention, output-wrap visibility, transcript grouping/render/input/mouse
state, render/transcript tests, nine TUI-188 snapshots, and the linked harness
documents.

Commands and results: all focused and completion commands listed in Validation
passed after the expected initial characterization failure and one corrected
strict-Clippy readability finding. After plan completion, a separate explicit
user request authorized installing the validated release at
`/Users/aji/.local/bin/ca`; source and installed SHA-256 match at
`0f457421a0fd1ca1bf5a2d4bcc8bfb091f02f97fe606f00b9f3ec8c7bd886b22`,
strict signature/help/version/lookup checks pass, and the prior binary is
recoverable at `/Users/aji/.local/bin/ca.backup-tui188-20260826`.

Remaining risks: restored sessions without canonical create-versus-edit
markers deliberately show Changed. Styling is terminal/theme dependent, so the
review establishes layout and interaction behavior rather than pixel parity.

Harness updates: `ARCHITECTURE.md`, `CONTINUITY.md`, `NEXT_STEPS.md`,
`TESTING.md`, and `docs/.INDEX.md` record the completed boundary and evidence.
