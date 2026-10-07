# TUI-256 — Grok-style usage-limit graph

Status: COMPLETE
Owner/session: TUI-256
Last updated: 2026-09-30

## Goal

The Usage limit tab of the dashboard usage modal shows a compact Grok-style
percentage bar (title, filled/empty meter, percent, caption) sourced from CA
provider/ledger data. The verbose provider dump is no longer the primary view.

## Non-Goals

- SuperGrok weekly-quota API or copied Grok source.
- Changing Context usage or Session info tab content beyond shared meter paint.
- Changing the pager snapshots that still render the full provider-account report.

## Evidence and Current Behavior

- Source: `render_usage_modal` already paints `meter(percent)` when
  `limit_percent` is `Some`. `open_usage_modal` sets `limit_percent: None`,
  `limit_title: "Provider account"`, and `limit_body` to
  `usage::render_report_with_provider_account` (the dump in the user screenshot).
- Snapshot `agent_dashboard_usage_limit.snap` already shows the bar when the
  fixture supplies percent/title/reset, then still dumps "Provider account".
- User screenshot (session Image #2): Usage limit tab is the dump only.
- Grok screenshot (session Image #1 / `ca-dashboard.jpeg`): title
  "Weekly limit (SuperGrok)", block bar, percent, reset line. Empty space below.
- Inferred: CA has no weekly SuperGrok quota. Graph from MiMo token-plan
  observed/allocation, else DeepSeek remaining as caption, else OpenAI
  month-to-date caption, else 0% with an honest status line.

## Scope

Allowed files: `src/provider_usage.rs`, `src/main.rs`, `src/tui/dashboard.rs`,
`src/tui/render_tests.rs`, `src/tui/snapshots/ca__tui__render_tests__agent_dashboard_usage_limit.snap`,
harness docs (`CONTINUITY`, `NEXT_STEPS`, `PARITY`, `ARCHITECTURE`,
`TESTING`, `docs/.INDEX.md`, this plan).
Forbidden: `oracles/`, `reference/`, runtime config/session mutation.
Runtime state: TUI display only.

## Design and Invariants

- `provider_usage::usage_limit_graph` derives title/percent/caption/compact
  details. No oracle copy.
- Limit tab always draws a bar (0% when used/limit is unknown).
- Unsupported providers are omitted from compact details.
- `/usage` still opens the three-tab modal. Full dump remains available to the
  leftover pager path (`pager_content` starting with `Usage\n`).
- Prefer the current model’s provider when that entry is graphable.

## Milestones

### M1 — characterization
- Graph unit tests for MiMo ratio, DeepSeek remaining, loading, prefer-current.
- Snapshot fixture without the dump as hero content.

### M2 — implementation
- Derive graph, fill modal on open and refresh, two-tone meter, muted details.

### M3 — validation
- Focused tests, fmt, clippy, `cargo test --locked --bin ca`, release install.

## Acceptance Criteria

- Usage limit tab renders title + `█`/`░` bar + percent + caption.
- Live `/usage` sets `limit_percent` (Some, including 0) rather than None.
- MiMo with observed 4_000_000 / allocation 8_000_000 → 50%.
- Loading state shows empty bar and "Loading configured provider accounts…".
- Wide/narrow `agent_dashboard_usage_limit` snapshot matches the compact graph.
- No oracle source copied.

## Validation

Focused: `cargo test --locked --bin ca -- usage_limit_graph meter_fills limit_tab_always usage_command_opens agent_dashboard_snapshots -- --test-threads=1` PASS.
Completion: `cargo test --locked --bin ca --no-fail-fast` **2,661 passed, 0 failed, 40 ignored**;
`cargo fmt --check` PASS; `cargo clippy --all-targets -- -D warnings` PASS.
Manual/visual: reviewed `agent_dashboard_usage_limit` wide/narrow vs Grok weekly-limit screenshot.
Install: `/Users/aji/.local/bin/ca` SHA-256 (post codesign)
`46d60ee15c7a4893c357e75eccd19b8f05eaaeacc1fc52c25cf122e913c07b41`.
Recovery: `/Users/aji/.local/bin/ca.recovery-TUI256-20260930T155545Z-864eae83`.
Blocked checks: none.

## Progress

- [x] ExecPlan written
- [x] Graph derivation + tests
- [x] Modal fill + render restyle
- [x] Snapshot and `/usage` tests
- [x] Completion checks and install

## Decisions

- Decision: Honest CA titles (`Token plan (MiMo)`, `Account balance (DeepSeek)`)
  rather than labeling a SuperGrok weekly limit that CA does not have.
- Decision: Compact supporting lines stay under the graph in muted style;
  unsupported providers are omitted.
