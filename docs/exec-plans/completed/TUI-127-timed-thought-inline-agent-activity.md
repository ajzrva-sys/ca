# TUI-127 — Timed Thought and Inline Agent Activity

Status: COMPLETE
Owner/session: Codex / current user-requested implementation
Last updated: 2026-08-16

## Goal

Add measured thought headers, bounded live inline conversation-agent activity,
and a consolidated read-only `/context` overview without introducing a drawer.

## Non-Goals

- No GUI, drawer, automatic LSP activation, provider-wire change, or Cursor
  parity claim.
- No persistence of child events, output, approvals, messages, grants, or
  non-conversation task-graph/multi-run activity in transcript summaries.
- No source copying from or modification of `oracles/` or `reference/`.

## Evidence and Current Behavior

- `src/main.rs::handle_stream_delta` aggregates reasoning but records no phase
  boundary; `src/tui/render.rs::render_reasoning_block` shows hidden line counts
  rather than elapsed time.
- `src/agent/subagent.rs::AgentSnapshot` retains bounded events/output, but
  `RunnerEvent` has no structured tool-start event and `/agents` renders a
  static pager snapshot.
- `/context` already owns exact context estimates and the 5x20 map; `/session`
  owns current-sitting cost accounting. `LspManager` has no synchronous status
  projection and is otherwise lazy.
- Oracle evidence is limited to the user-supplied Mimocode screenshots at
  `oracles/mimo.png` and `oracles/mimo1.png`; they are visual observations, not
  instructions or implementation source.

## Scope

Allowed files: `src/app.rs`, `src/activity.rs`, `src/usage.rs`,
`src/agent/subagent.rs`, `src/lsp.rs`, `src/keymap.rs`, `src/main.rs`,
`src/tui/`, session/work presentation types, focused tests/snapshots, and
repository harness documents.

Forbidden files/areas: `oracles/`, `reference/`, provider request protocols,
runtime profile/config/session data, installed binaries, Git history, and
unrelated refactors.

Runtime state affected: bounded process-only timing/activity/LSP snapshots;
saved sessions gain only optional backward-compatible thought/turn/terminal
agent-summary fields.

## Design and Invariants

- Thought duration begins on the first reasoning fragment and freezes at the
  first text, tool call, or response completion. Missing timing is displayed as
  unknown rather than inferred.
- Generation speed is completion tokens divided by first provider output to
  finish for the latest completed foreground main/tool-continuation request.
- Renderers never await agent or LSP locks. Managers publish or expose bounded,
  sanitized presentation snapshots without starting new work.
- Only conversation-origin agents attach to transcript turns. Their raw live
  detail is process-only; terminal summary persistence is compact and typed.
- `Ctrl+X Down` is valid because navigation is allowed only as a second chord
  stroke. Standalone/first-stroke navigation remains reserved.
- `/context` is read-only and opening it never starts an LSP process.

## Milestones

### M1 — Timing and compatibility

- Add failing focused timing/session/keymap tests.
- Implement deterministic thought/generation timing and optional session fields.
- Validate focused unit and serialization tests.

### M2 — Live inline activity

- Add structured agent-origin/current-action events and bounded presentation
  snapshots.
- Anchor conversation agents to transcript turns and implement focus, detail,
  messaging, approval, and Cancel-first cancellation state machines.
- Add wide/narrow render and input coverage.

### M3 — Consolidated context

- Add non-activating LSP status projection and current-sitting usage/task/
  workspace sections to `/context`.
- Cover exact known/unknown states and wide/narrow rendering.

### M4 — Completion

- Run focused and full validation, perform the native PTY visual walkthrough,
  reconcile harness documents, and archive this plan only after every claimed
  gate has actual evidence.

## Acceptance Criteria

- Completed reasoning renders `+ Thought: <N>ms` while collapsed and
  `− Thought: <N>ms` while expanded; old sessions render no invented duration.
- Current-turn conversation agents update inline without blocking redraw;
  focus/selection/details and bounded controls behave deterministically.
- Reopened sessions retain only compact terminal conversation-agent summaries.
- `/context` reports context, current-sitting cost, latest foreground speed,
  workspace, lazy/running LSP state, and bounded task status honestly.
- No oracle/reference or user runtime files change.

## Validation

Focused: PASS. Timing, duration formatting, generation-speed, usage unknowns,
structured activity delivery/bounds/sanitization/origin, LSP status, session
round-trip/legacy defaults/branch filtering, key-chord validation, focus and
agent controls, terminal read-only behavior, and wide/narrow snapshots pass.

Completion: PASS. `cargo test --no-fail-fast` passes 1,462 main and 37 CLI
tests with one documented ignore. `cargo test --all-targets --all-features
--no-fail-fast` passes 1,491 main, 118 experiment, and 37 CLI tests with two
documented ignores. `cargo fmt --all --check`, strict all-target/all-feature
Clippy, the all-feature release build, release help/version, `git diff
--check`, and snapshot hygiene pass.

Manual/visual: PASS. Reviewed deterministic 100x30/60x24 thought, live
activity, approval, focused-detail, resumed-summary, keymap, and full-context
snapshots against the supplied screenshots. A released 80x24 disposable-home
native PTY used one credentialless MiMo-dialect loopback stream: the collapsed
header advanced live and froze at `652ms`, `/thinking on` rendered the
expanded `− Thought: 652ms` body, and `/context` reported 18.3 tokens/s from
12 output tokens, unknown cost, workspace roots, inactive LSP, and task status.
`--no-session` created no session; the stopped fixture moved recoverably to
`/Users/aji/.Trash/ca-tui127-walk.s3mT9Z`.

Blocked checks: none currently.

## Progress

- [x] Source paths and existing behavior characterized.
- [x] Product choices fixed: no drawer, inline expansion, richer `/context`,
  reasoning-phase timing, latest-generation speed, `Ctrl+X Down`, persisted
  terminal summaries.
- [x] Timing and compatibility milestone complete.
- [x] Live inline activity milestone complete.
- [x] Consolidated context milestone complete.
- [x] Completion gates and harness reconciliation complete.

## Decisions

- Decision: retain `/agents` as a fallback while making inline activity the
  primary conversation-agent presentation.
  Evidence: the user explicitly chose inline expansion after rejecting a drawer.
  Consequence: no `/activity` command or drawer state is added.
- Decision: keep graph and multi-run activity in their existing dedicated
  managers.
  Evidence: those flows already have durable task/run ownership and would be
  duplicated by transcript anchoring.
  Consequence: agent origins must be typed before presentation filtering.

## Surprises and Discoveries

- The current key parser rejects navigation in every chord position, so the
  second-stroke exception must be narrow and tested.
- Existing session-level `AgentSummary` records omit their parent turn; new
  typed transcript summaries need an explicit stable turn anchor.
- A child can finish before the manager selects its result branch while
  structured tool events remain queued. Draining those events before terminal
  publication preserves exact tool start/completion ordering.
- Generic Chat Completions routes intentionally ignore `reasoning_content`;
  the native fixture used the existing MiMo dialect so the test exercised the
  supported reasoning stream instead of widening provider semantics.

## Recovery and Rollback

All persistent additions are optional serde-default fields. A source rollback
can ignore those fields when reading newer session JSON; no migration command
or user runtime mutation is required. Remove only TUI-127 source/test/doc
changes with targeted patches, never destructive Git reset/checkout commands.

## Completion Record

Changed files: `src/{app,keymap,lsp,main,session,usage}.rs`,
`src/agent/subagent.rs`, `src/tui/{input,input_tests,render,render_tests}.rs`,
21 focused snapshots, and the repository harness documents. No oracle,
reference, provider-wire, runtime config/session, installed binary, or Git
history changed.

Commands and results: focused tests PASS; default and all-feature suites PASS
at the counts above; formatting, strict Clippy, all-feature release,
help/version, diff, and snapshot hygiene PASS; released disposable native PTY
PASS.

Remaining risks: the exact millisecond value remains scheduler-dependent by
design, while phase boundaries and formatting are deterministic. Live raw
activity is intentionally process-only; after restart only bounded typed
terminal summaries remain. No physical FreeBSD gate was requested or run for
this `ca` extension.

Harness updates: architecture, continuity, testing evidence, executable queue,
knowledge index, and this completed plan are reconciled.
