# CTX-112–MD-116 — Selected Oracle-Informed Enhancements

Status: COMPLETE
Owner/session: Codex / current implementation session
Last updated: 2026-08-16

## Goal

Deliver focused manual compaction, opt-in completion notifications, a scoped
session report, conservative terminal math rendering, and collapsed exploration
groups as one tested CLI/TUI-only change.

## Non-Goals

- No Cursor parity claim, provider/wire/session schema change, GUI, push, binary
  installation, branch/worktree/stash creation, or automatic notification API.
- No hunk-level tool grouping semantics, external TeX renderer, persisted
  sitting metrics, or automatic-compaction focus.

## Evidence and Current Behavior

- `/compact` discards arguments in `src/main.rs`; the checkpoint prompt in
  `src/context.rs` has six fixed required sections.
- `/usage` and `/context` are separate; there is no `/session` command and the
  bounded usage ledger is process-local.
- UI configuration has theme/statusline settings but no notification setting;
  top-level turn timing already exists in `TurnLifecycle`.
- `src/markdown.rs` enables tables but not Pulldown-Cmark math events.
- `src/tui/render.rs` renders each tool call independently and expansion is
  keyed by message index.
- The named oracle documents are read-only design inspiration. This work is an
  intentional `ca` extension and not Cursor parity evidence.

## Scope

Allowed files: production Rust sources, existing Rust tests/snapshots, this
ExecPlan, `CONTINUITY.md`, `TESTING.md`, `NEXT_STEPS.md`, and `PARITY.md` only
if a real parity conflict is discovered.

Forbidden files/areas: `oracles/`, `reference/`, user runtime configuration,
credentials, saved sessions, installed binaries, and unrelated source.

Runtime state affected: only explicitly selected comment-preserving UI config
writes in disposable/manual testing; production default remains off.

## Design and Invariants

- Manual checkpoint focus is trimmed, single-line/control-free, bounded to
  2,048 Unicode scalar values, JSON-quoted in the prompt, and subordinate to
  the six required continuity sections. Automatic checkpoints have no focus.
- One top-level turn completion boundary supplies both process-only sitting
  duration and at-most-once notification decisions. Provider continuations,
  tools, and subagents do not create independent completions.
- `/session` labels conversation-history data separately from current-sitting
  data and visibly marks bounded-ledger truncation. Sitting state is never
  serialized and resets at start, clear, and successful resume.
- Math conversion is display-only, deterministic, and lossless for unsupported
  input. Code, URLs, escaped dollars, and currency remain literal.
- Exploration grouping is registry-driven and presentation-only. It cannot
  alter tool execution, canonical messages, exports, sessions, or provider
  history.

## Milestones

### M1 — CTX-112 focused compaction

- Add failing focus validation/prompt/orchestration characterizations.
- Carry optional manual focus through the checkpoint request while leaving
  automatic compaction unchanged.
- Validate with `cargo test compact_` and context-focused tests.

### M2 — TUI-115 completion notifications

- Add failing config, picker, lifecycle, and byte-output characterizations.
- Add the typed setting, comment-preserving save path, central turn completion,
  and post-redraw TTY emission.
- Validate with `cargo test notification_` and reviewed snapshots.

### M3 — SES-113 combined session report

- Add failing command, metric-scope, aggregation, reset, and rendering tests.
- Add process-only sitting metrics and the read-only pager report.
- Validate with `cargo test session_` and usage/context-focused tests.

### M4 — MD-116 conservative terminal math

- Add failing delimiter, currency, conversion, fallback, and snapshot tests.
- Enable math events, normalize safe alternate delimiters outside code, and
  render the bounded Unicode subset without mutating canonical content.
- Validate with `cargo test markdown::tests` and snapshots.

### M5 — TUI-114 collapsed exploration groups

- Add failing exact-classification, boundary, state, expansion, and snapshot
  tests.
- Add explicit presentation classification and stable group/block identities.
- Validate with `cargo test exploration_` and ordinary-tool regressions.

### M6 — Completion and integration

- Run default/all-feature suites, format check, strict Clippy, all-feature
  release build, release smokes, diff/snapshot hygiene, disposable manual TUI,
  and exact-source physical-FreeBSD gates.
- Reconcile harness documents, archive this plan, and create exactly one commit
  with subject `feat: add focused context and terminal UX workflows`.

## Acceptance Criteria

- Every behavior and boundary in the approved implementation plan has focused
  deterministic coverage; wide/narrow user-visible states are reviewed.
- Existing bare `/compact`, `/usage`, `/context`, ordinary tools, sessions,
  provider history, and default terminal output remain compatible.
- No oracle/reference/user-runtime/installed-binary mutation occurs.
- All available completion gates pass; any unavailable physical/manual check is
  recorded as blocked rather than inferred.

## Validation

Focused: PASS. `cargo test compact_` (4), `cargo test notification_` (5),
`cargo test session_` (88), `cargo test markdown::tests` (5), and
`cargo test exploration_` (6) pass locally and from the exact implementation
source on physical FreeBSD before final documentation reconciliation.

Completion: PASS locally and on physical FreeBSD 15.1-RELEASE-p2 amd64.
Local default/all-feature suites pass 1,359/1,388 main tests, 118 experiment
tests, and 36 CLI tests. Native locked suites pass 1,346/1,375 main tests, 116
experiment tests, and 36 CLI tests. Documented manual-browser and
external-fixture cases remain ignored. Formatting, strict all-feature Clippy,
all-feature release builds, help/version, snapshot hygiene, and platform-native
binary/linkage checks pass. Local release SHA-256 is
`47a997081ae94906a2fff82420a5fe378cb526fdf3695bf96c87d4168724b86e`;
FreeBSD release SHA-256 is
`8ff0208e7a7f2a9c9ecab60ff5792d6e41d98685e4c0bd99fda7f411f69ad1b0`.

Manual/visual: PASS. A released 100x34 TUI used a disposable home/repository
and loopback-only deterministic provider. It showed collapsed exploration,
converted inline/display math, `/session` metrics, bare-mode notification
refusal, focused checkpoint completion, and 3/3 checkpoint coverage. The
fixture received three requests; no session file or persistent setting change
occurred and sample bytes were unchanged.

Physical source: PASS. Final archive SHA-256
`5e640069a28e45e0df7eaeec8eb75a07fe3b4ba1fb47e0bc953d7fdd515fd96e`.
Both disposable FreeBSD trees, their Cargo cache, the local walkthrough tree,
and transfer bundle were removed.

Blocked checks: none.

## Progress

- [x] Confirmed clean `master` and current harness state.
- [x] Created and linked the active combined ExecPlan.
- [x] M1 CTX-112: optional validated focus is isolated to manual checkpoint
  prompts; `cargo test compact_` (4 tests) and `cargo test context::tests` (17
  tests) pass.
- [x] M2 TUI-115: typed off/BEL/OSC9 setting, searchable picker,
  comment-preserving save, top-level threshold/suppression, TTY writer, and
  four reviewed snapshots; focused notification/activity/UI tests pass.
- [x] M3 SES-113: argument-free read-only report with separated history/sitting
  scopes, top-level worked time, deduplicated terminal tools, typed usage/cost
  aggregation, bounded-ledger lower bounds, reset semantics, and two reviewed
  pager snapshots; focused session and all usage tests pass.
- [x] M4 MD-116: native math events, code-aware alternate-delimiter
  normalization, conservative Unicode conversion, literal fallback/currency
  handling, Unicode-width-safe tables, and reviewed prose/table/list snapshots;
  all Markdown-focused tests pass.
- [x] M5 TUI-114: explicit 18-tool registry classification, cross-message
  grouping with exact boundaries and terminal states, 12-action collapsed cap,
  original-order expansion, stable first-call keys, and reviewed wide/narrow
  snapshots; exploration, full render, and full input suites pass.
- [x] M6 completion, documentation, and FreeBSD validation. The final commit
  is the last action after archiving this plan.

## Decisions

- Decision: implement in the approved order and create one final commit only.
  Evidence: explicit user instruction.
  Consequence: milestone progress remains uncommitted until every gate and
  document is reconciled.
- Decision: use one top-level completion seam for time and notifications.
  Evidence: both features require the same at-most-once terminal transition.
  Consequence: retries and continuations cannot double-count or double-notify.

## Surprises and Discoveries

- The `/session` snapshot's context occupancy included the native system
  prompt and differed by one estimated token: 3403 on macOS, 3404 on FreeBSD.
  The rendering fixture now normalizes only this one display value to 3400;
  production continues to compute its native occupancy.
- macOS archive xattrs caused FreeBSD `tar` to return a nonzero status in the
  first partial disposable extraction. A new `--no-xattrs` archive produced
  the exact implementation source tree used for the completed native gates;
  the partial tree was never tested and was removed.
- The unprivileged host had no usable cached registry for this dependency set.
  Dependencies were fetched into a task-local Cargo home, all later commands
  ran offline, and the cache was removed with the validation tree.

## Recovery and Rollback

Do not use destructive Git operations. Focused changes remain separable by
milestone in the working tree. If unrelated concurrent edits appear, preserve
them in place and work around non-overlapping files; stop only for an actual
overlap whose intent cannot be derived.

## Completion Record

Changed files: focused production changes in `src/activity.rs`,
`src/agent/registry.rs`, `src/app.rs`, `src/config.rs`, `src/context.rs`,
`src/main.rs`, `src/markdown.rs`, `src/session_metrics.rs`,
`src/tui/{commands,event,input,render}.rs`, `src/ui_settings.rs`, and
`src/usage.rs`; focused tests in those modules plus `src/tui/render_tests.rs`;
reviewed TUI snapshots; and the required continuity/testing/queue/plan records.

Commands and results: all focused and completion commands listed under
Validation passed. `git diff --check`, released CLI smokes, Mach-O/ELF identity,
native linkage, and zero pending snapshots also passed. Detailed counts and
walkthrough evidence are recorded in `TESTING.md`.

Remaining risks: notification escape sequences still depend on terminal
support and remain opt-in/off by default; math deliberately preserves rather
than interprets unsupported TeX; exploration grouping is limited to the
explicit registry allowlist. These are designed boundaries, not blocked work.

Harness updates: `CONTINUITY.md`, `TESTING.md`, and `NEXT_STEPS.md` reconciled.
`PARITY.md` unchanged because no real Cursor-parity conflict was found. No
later task is selected.
