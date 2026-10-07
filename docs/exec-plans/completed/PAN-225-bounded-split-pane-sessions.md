# PAN-225 — Bounded Split-Pane Sessions

Status: COMPLETE
Owner/session: ORC-218 goal session
Last updated: 2026-08-29

## Goal

Support at most four simultaneously visible interactive session panes while
preserving pane-local model, workspace, conversation, approval, workflow, job,
and generation ownership and retaining shared process services only where
authority cannot cross panes.

## Non-Goals

- No pane layout persistence, headless/service/ACP behavior change, detached
  pane jobs, automatic background approval, cross-pane transcript delivery, or
  deletion of a saved session when a pane closes.
- No oracle/reference edit, provider traffic, user runtime mutation, or binary
  installation.

## Evidence and Baseline

- `AppState` currently owns one interactive conversation plus process services;
  TUI event variants use generation IDs but not pane IDs.
- Session loading, approval queues, jobs, goals, workflows, worktrees, and
  terminal restoration already have explicit ownership seams that must remain
  authoritative after the split.
- ORC-218 baseline: 868 records, SHA-256
  `2dcaf84dc86d3ae27bfea593bc811ce16f46f9782382780901fdcf890ff39978`.

## Design

- Introduce a bounded layout tree and stable pane IDs. At most four leaf panes
  exist; horizontal/vertical splits fail when either child would violate the
  transcript/composer minimum.
- Extract pane-local conversation/session state behind explicit switch/save
  boundaries while shared configuration, provider catalog, diagnostics, and
  terminal lifecycle remain process-owned. Every asynchronous conversation
  event carries pane plus generation identity and stale/cross-pane events are
  ignored.
- `/split horizontal|vertical [new|SESSION]`, `/pane next|prev|close|list`, and
  configurable `Ctrl+X H/V/N/P/Q` actions operate only on interactive TUI
  state. Mouse focus changes the active pane; only that pane accepts composer
  input.
- Background panes may receive provider text, but mutation/network approvals
  remain pending there. Closing an active pane is Cancel-first and cancels only
  that pane's turn/jobs; process quit retains all-session cancellation.

## Validation

Add failure-first layout, dimension, four-pane, event-routing, session switch,
approval/job/goal/workflow isolation, stale-event, close/quit, focus badge,
configuration, input, wide/narrow snapshot, and disposable PTY tests. Run
focused suites, formatting, strict all-feature Clippy, release, and hygiene
checks without provider traffic or installation.

## Progress

- [x] Child plan and queue entry created after ART-224 completion.
- [x] Pane-local state and bounded layout tree implemented.
- [x] Commands, key actions, focus, close, and badges implemented.
- [x] Async events and owned work proven cross-pane safe.
- [x] Focused suites and child completion gates pass.
- [x] Harness reconciled and plan completed.

## Completion Record

- `src/panes.rs` owns a four-leaf bounded horizontal/vertical layout and swaps
  complete isolated `AppState` values while synchronizing only declared
  process services. New panes receive distinct session grants, work state, job
  owners, agent managers, MCP/LSP managers, queues, approvals, goals, workflows,
  and stream generations.
- Every provider/tool/goal stream is routed through an originating pane ID;
  materialization plus the existing generation check rejects stale and
  cross-pane delivery. Background mutation/network work is converted to an
  explicit pending approval until that pane is focused.
- `/split`, `/pane`, mouse focus, configurable `Ctrl+X H/V/N/P/Q`, focused
  badges, per-pane rendering, Cancel-first pane closure, and all-pane shutdown
  cleanup are implemented. Loading an existing session for a split runs on a
  blocking worker rather than the terminal event loop.
- The 11 pane-focused tests, the background-approval regression, reviewed
  vertical/horizontal/four-pane snapshots, and the complete serial CA binary
  suite pass: 2,353 passed with four documented ignores. Formatting, strict
  all-feature CA Clippy, and the all-feature release build pass. The existing
  macOS oversized `__eh_frame` linker warning remains non-fatal.
- No provider traffic, runtime profile/session mutation, oracle/reference or
  protected-tree edit, or binary installation occurred.

## Recovery

Reverse only PAN-225-labelled hunks and delete only PAN-created paths with
`apply_patch`. Never reset, clean, mutate oracle/reference or protected trees,
contact a provider, alter user runtime state, or install a binary.
