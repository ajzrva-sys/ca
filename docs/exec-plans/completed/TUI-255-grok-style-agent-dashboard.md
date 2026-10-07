# TUI-255 — Grok-style agent dashboard

Status: COMPLETE
Owner/session: TUI-255
Last updated: 2026-09-30

## Goal

Give `ca` a full-screen agent dashboard that matches the Grok Build dashboard
the user attached: location header, `+ New Agent` / worktree / previous
sessions, Working and Inactive groups, dispatch prompt, and a tabbed usage
modal (`Context usage` / `Usage limit` / `Session info`).

## Non-Goals

- Concurrent live agents in one process (CA remains one attached conversation).
- Peek/reply panel, pin reorder, search-mode `Ctrl+/`, or Grok source copies.
- SuperGrok weekly-quota API. Usage-limit bars use CA provider/context data.
- Changing default `ca` launch into the dashboard (use `ca dashboard`).

## Evidence and Current Behavior

- Source: session picker is a modal (`session_picker_open`); `/usage`,
  `/context`, and `/session` open the pager. One attached conversation.
- User screenshot (`ca-dashboard.jpeg`) and Grok user-guide
  `~/.grok/docs/user-guide/23-dashboard.md`: roster grouped by state, usage
  modal over the dashboard, `Ctrl+\`, `/dashboard`, `grok dashboard`.
- Oracle `oracles/grok-build-main` is read-only evidence. No source copied.
- Inferred: Inactive rows are other saved sessions; Working/Idle is the
  current conversation.

## Scope

Allowed files: `src/tui/dashboard.rs`, `src/tui/{mod,render,input,commands}.rs`,
`src/tui/render_tests.rs`, `src/tui/input_tests.rs`, `src/app.rs`,
`src/main.rs`, harness docs (`CONTINUITY`, `NEXT_STEPS`, `PARITY`,
`ARCHITECTURE`, `docs/.INDEX.md`, this plan).
Forbidden: `oracles/`, `reference/`, runtime config/session mutation.
Runtime state: TUI only; `ca dashboard` / `--dashboard` launch flag.

## Design and Invariants

- Dashboard is an alternate full-screen view, not a conversation overlay.
- `/sessions` stays the saved-session picker (Open Previous).
- `/dashboard` (aliases `/agents-dashboard`) toggles the roster.
- `/usage` opens the tabbed modal over dashboard or conversation.
- Dispatch with a prompt starts a new session when the current one has
  content, then submits. Empty `+ New Agent` is `/new`.
- Inactive starts collapsed. Enter on a section title toggles it.
- Deterministic ages in tests through `now_override`.

## Milestones

### M1 — characterization
- Failing snapshots and command parse tests.

### M2 — implementation
- Roster, usage modal, keys, CLI, dispatch.

### M3 — validation
- Focused tests, fmt, clippy, `cargo test --locked --bin ca`, release install.

## Acceptance Criteria

- Wide/narrow snapshots: empty roster, working+inactive, usage-limit modal.
- `/dashboard` and `ca dashboard` / `--dashboard` open the view.
- `Ctrl+\` toggles it. Esc steps to New Agent, then closes.
- Enter on a saved row loads that session. Enter on `+ New Agent` starts `/new`.
- `/usage` shows Context / Usage limit / Session info tabs.

## Validation

Focused: dashboard/usage/CLI tests PASS.
Completion: `cargo test --locked --bin ca` **2,653 passed, 0 failed, 40 ignored**;
`cargo fmt --check` PASS; `cargo clippy --all-targets -- -D warnings` PASS.
Manual/visual: reviewed `agent_dashboard_{empty,roster,usage_limit}` vs screenshot.
Install: `/Users/aji/.local/bin/ca` SHA-256
`864eae83f2861daa425648405210102c003cade408e2a2bcb9379562e54ae5c2`.
Blocked checks: none.

## Progress

- [x] Evidence from screenshot, user-guide, CA TUI
- [x] Implementation
- [x] Tests and snapshots
- [x] Harness docs
- [x] Completion checks (`cargo test --locked --bin ca` 2,653 passed)

## Decisions

- Decision: CA-owned Grok-informed dashboard; no oracle source copy.
- Decision: `/sessions` remains the picker; dashboard has Open Previous.
- Decision: `/usage` becomes the tabbed modal; `/context` and `/session`
  keep their pagers and also appear as modal tabs.
