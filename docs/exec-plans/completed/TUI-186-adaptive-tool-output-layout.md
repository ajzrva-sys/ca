# TUI-186 — Adaptive Tool-Output Layout

Status: COMPLETE
Owner/session: Codex / current user-requested implementation
Last updated: 2026-08-26

## Goal

Render bounded in-TUI tool output at the available transcript width, keep every
visual row inside a complete card border, and present stable tab-separated
blocks as readable Unicode-safe tables with wrapped Ctrl+O expansion.

## Non-Goals

- No change to captured stdout/stderr, canonical `ToolCall.output`, provider
  continuation, sessions, exports, saved TSV files, or `/output full` streaming.
- No CSV/space-delimited guessing, command-specific parser, new setting,
  shortcut, tool schema, provider protocol, runtime-state mutation, binary
  installation, or oracle/reference change.

## Evidence and Current Behavior

- `src/tui/render.rs::render_tool_output` caps cards at 60 columns, truncates by
  Unicode scalar count, omits right borders on content rows, and passes tabs to
  Ratatui unchanged.
- Ratatui treats tabs as zero-width control graphemes, so TSV fields collapse;
  wide graphemes can also exceed scalar-count bounds.
- The supplied screenshots show collapsed fields and content outside the card;
  the attached TSV has nine stable tab-separated columns and long Unicode names.
- `src/jobs.rs::JobInfo::formatted_output` and `src/command_output.rs` establish
  that render-only transformation must not replace canonical bounded output.

## Scope

Allowed files: a focused crate-private TUI output-layout module, its module
registration, tool-output rendering, focused tests/snapshots, this plan, and
reconciled harness documents.

Forbidden files/areas: `oracles/`, `reference/`, provider/session/tool schemas,
runtime configuration, credentials, installed binaries, branches/worktrees,
and unrelated snapshots.

Runtime state affected: none.

## Design and Invariants

- Sanitize first, then segment stable runs of at least two rows with the same
  tab-separated arity of at least two columns. Preserve empty cells. Isolated
  or inconsistent tabs remain ordinary text expanded at eight-cell tab stops.
- Use the available transcript width with the existing four-cell indent and
  right margin. Every content row is padded and closed by a right border.
- Table columns retain source order. Consistently numeric data columns align
  right; others align left. Identifier-like first rows receive header styling.
  Width allocation is deterministic, caps one column's desired width, and
  wraps cells at grapheme/display-cell boundaries. Mathematically impossible
  tables fall back to safe wrapped text.
- Collapsed output stops before exceeding 20 complete visual rows and shows an
  expansion marker. Expanded output renders the full bounded capture.
- The formatting result is process-only; canonical output bytes remain intact.

## Milestones

### M1 — Characterize layout failures

- Add failing pure-layout tests for mixed TSV/text, Unicode, empty cells,
  alignment, tab fallback, visual bounds, narrow fallback, and preview limits.
- Add deterministic render regressions for wide/narrow collapsed/expanded and
  expanded-to-collapsed redraw behavior.

### M2 — Implement and integrate

- Implement the pure output layout and connect it only to ordinary tool-output
  cards after semantic/raw display selection.
- Preserve special send-message and diff render paths.

### M3 — Validate and reconcile

- Review targeted snapshots and compare the user-shaped fixture at 60, 100,
  and 184 columns.
- Run focused tests and all completion gates from `TESTING.md`, then reconcile
  harness documents and move this plan to `completed/`.

## Acceptance Criteria

- Stable TSV blocks are aligned, numeric columns are right-aligned, long cells
  wrap, and all captured cell content appears exactly once when expanded.
- Ordinary output and inconsistent tabs remain readable without false table
  detection; control sequences remain sanitized.
- Every card row fits the transcript width and has a closing border; redraws
  leave no output fragments beside later assistant Markdown.
- Canonical/model/session/native output remains unchanged.

## Validation

Focused:

- Initial four-test characterization: expected FAIL before implementation.
- `cargo test -p ca tui::output_layout::tests -- --test-threads=1`: PASS,
  five tests.
- `cargo test -p ca tui::render_tests -- --test-threads=1`: PASS, 182 tests.
- `cargo test -p ca command_output -- --test-threads=1`: PASS, six tests.
- `cargo test -p ca terminal_text -- --test-threads=1`: PASS, two tests.

Completion:

- `cargo test --workspace --no-fail-fast -- --test-threads=1`: PASS, 2,030 CA
  tests, 38 CLI tests, 17 role-run tests, 556 embedded scheduler tests, and all
  integration/doc targets; three documented manual/external tests ignored.
- `cargo fmt --all -- --check`: PASS.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: PASS.
- `cargo build --workspace --release --all-features`: PASS.
- Focused `git diff --check` and `.snap.new` hygiene: PASS.

Manual/visual: new and affected 60-, 100-, and 184-column snapshots reviewed
against the supplied failure shape. Cards use the available width, every row
has a right edge, tabular fields align, and later Markdown remains clean.

Blocked checks: none known.

## Progress

- [x] Repository behavior, screenshots, and representative TSV shape inspected.
- [x] Failing focused regressions recorded.
- [x] Adaptive layout implemented and integrated.
- [x] Focused and completion validation recorded.
- [x] Harness reconciled and plan archived.

## Decisions

- Decision: recognize only stable tab-delimited runs.
  Evidence: the reported corruption is caused by literal tabs; broader delimiter
  guessing would create avoidable false positives.
  Consequence: ordinary prose and command logs keep the generic layout.
- Decision: wrap wide cells inside the current Ctrl+O interaction.
  Evidence: the user selected wrapping over a new horizontal viewer.
  Consequence: no new state or key handling is required.

## Surprises and Discoveries

- Ratatui retains invisible backing cells under double-width glyphs in its test
  buffer. The redraw regression therefore compares the user-visible exterior
  of the collapsed card and the later transcript, while direct layout tests
  enforce display-cell bounds for every produced row.

## Recovery and Rollback

Remove the focused module registration, restore the prior ordinary card branch,
and delete only TUI-186 tests/snapshots/docs. No persisted data migration or
runtime rollback is required.

## Completion Record

Changed files: `src/tui/output_layout.rs`, focused registration/render/tests,
18 new or directly affected output-card snapshots, and reconciled harness docs.

Commands and results: all focused and completion commands above passed.

Remaining risks: intentionally conservative detection ignores non-tabular CSV
and inconsistent rows; mathematically impossible table widths fall back to
wrapped plain text.

Harness updates: `CONTINUITY.md`, `ARCHITECTURE.md`, `NEXT_STEPS.md`,
`TESTING.md`, and `docs/.INDEX.md` reconciled. No binary was installed.
