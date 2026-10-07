# TUI-148 — fx-Inspired Quiet Default

Status: COMPLETE (2026-08-22)
Owner/session: Codex / current implementation session
Last updated: 2026-08-22

## Goal

Make the ordinary `ca` terminal surface quieter through progressive disclosure: idle startup contains one context line, one composer line, and one status line; common commands lead discovery; and existing safety, queue, overlay, and transcript behavior remains available.

## Non-Goals

- Do not replace the alternate-screen TUI, remove commands, change provider/session/approval protocols, or reduce safety detail.
- Do not optimize binary size or startup time.
- Do not modify `oracles/`, `reference/`, runtime configuration, sessions, credentials, or installed binaries.

## Evidence and Current Behavior

- `fx.plan` records the requested behavior and scope.
- `src/tui/render.rs` always renders a filled title bar, two-line empty-state welcome, bordered composer, and shortcut/message footer.
- `src/tui/commands.rs::matching_commands` preserves registry order for an empty query, placing many management commands ahead of common conversation actions.
- Existing TUI-138 composer, contextual status, inline completion, expandable exploration, and overlay behavior already supplies the required progressive-disclosure mechanisms.
- Inferred: presentation can be simplified without changing `AppState`, command identity, input actions, or persistence.

## Scope

Allowed files: `fx.plan`; TUI render/command code, focused tests, and snapshots; this plan; `ARCHITECTURE.md`, `CONTINUITY.md`, `NEXT_STEPS.md`, `TESTING.md`, and `docs/.INDEX.md`.

Forbidden files/areas: providers, approvals, sessions, runtime state, `oracles/`, `reference/`, installed binaries, and unrelated dirty-tree changes.

Runtime state affected: none; validation uses deterministic TestBackend rendering.

## Design and Invariants

- The title remains the one persistent context line and retains mode/model/safety states, but loses full-row background fill.
- The empty idle transcript contributes no duplicate workspace/model/shortcut text.
- The idle empty composer removes its border and instructional duplication; the status line advertises `/ commands` only.
- Active, queued, completion, scrolled, approval, error, and overlay states keep contextual status precedence and complete safety content.
- Empty-query discovery places ten common actions first and retains every visible command exactly once; non-empty search ranking and command IDs remain unchanged.

## Milestones

### M1 — Characterization

- Add failing startup assertions for three visible chrome rows, no duplicated labels, and no filled title background.
- Add a failing command-order/completeness test for empty discovery.

### M2 — Implementation

- Simplify title, empty transcript, idle composer, and idle status rendering.
- Add stable empty-query common-command ordering without affecting filtered search.
- Update only affected deterministic snapshots and navigation expectations.

### M3 — Validation and handoff

- Run focused TUI checks, default/all-feature tests, format, strict Clippy, release build, help/version, diff/snapshot hygiene, and oracle/reference immutability checks.
- Reconcile harness documents and move this plan to `completed/`.

## Acceptance Criteria

- Wide and narrow startup render exactly three non-empty chrome rows with no repeated model, workspace, shortcut, newline, or zero-message label.
- The title row has no full-width background fill; active safety text remains visible.
- `/` discovery begins with the locked ten common commands and still contains every visible registry command exactly once.
- Filtered matching, slash execution/completion, approvals, queues, failures, overlays, and transcript expansion remain compatible.

## Validation

Focused: startup/render, command registry, slash input/navigation, and reviewed wide/narrow snapshots.

Completion: commands in `TESTING.md`, including default/all-feature tests, formatting, strict Clippy, release, local help/version, diff/snapshot hygiene, and oracle/reference checks.

Manual/visual: reviewed deterministic 100x30 and 60x24 TestBackend snapshots.

Blocked checks: none anticipated; no native PTY is required for this presentation-only slice.

## Progress

- [x] Current rendering and command discovery characterized.
- [x] Focused failing tests added.
- [x] Quiet default rendering implemented.
- [x] Common-command discovery implemented.
- [x] Validation and harness reconciliation complete.

## Decisions

- Decision: reuse the title as the single context line instead of adding a new workspace banner.
  Evidence: the title already owns mode, model, profile, safety, goal, workflow, and worktree state.
  Consequence: no state/schema change and no duplicated welcome context.
- Decision: prioritize common commands only for an empty query.
  Evidence: filtered matching already has stable exact/prefix/substring ranking relied on by tests and users.
  Consequence: search behavior remains compatible while initial discovery becomes task-shaped.

## Surprises and Discoveries

- The surrounding composer/status behavior is already adaptive from TUI-138; this task is primarily removal of duplicated presentation.
- Removing globally duplicated chrome intentionally changed many render snapshots; representative startup, discovery, streaming, queue, approval, failure, completion, and status-line states were reviewed before the full render suite passed.

## Recovery and Rollback

Use targeted inverse patches for TUI-148 hunks only. Do not reset, checkout, or rewrite unrelated dirty-tree work.

## Completion Record

Changed files: `fx.plan`; TUI rendering, command ordering, focused tests, test support, and affected snapshots; this plan and the project harness documents.

Commands and results: baseline startup and command suites passed before characterization failed as intended. Focused TUI passed 391 tests. Complete default passed 1,652 main plus 37 CLI tests with one ignore. Complete all-feature passed 1,681 main, 118 experiment, and 37 CLI tests with two ignores. Formatting, strict Clippy, all-feature release, help/version, diff/snapshot hygiene, and oracle/reference checks passed.

Remaining risks: terminal styling was accepted through deterministic wide/narrow buffers rather than a native PTY session; no behavior or safety protocol changed.

Harness updates: `ARCHITECTURE.md`, `CONTINUITY.md`, `NEXT_STEPS.md`, `TESTING.md`, and `docs/.INDEX.md` reconciled on 2026-08-22.
