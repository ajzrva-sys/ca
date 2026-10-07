# REL-128 — Low-Yield Loop Guard

Status: COMPLETE
Owner/session: Codex / current user-requested implementation
Last updated: 2026-08-16

## Goal

Detect bounded, repeated no-progress tool activity and stop at a safe
post-tool boundary with explicit Replan, Continue, or Cancel choices instead
of silently continuing an unproductive loop.

## Non-Goals

- Do not parse rendered transcript text, launch an external coding harness, or
  modify an oracle.
- Do not infer success from prose or automatically choose a recovery action.
- Do not persist detector observations, command output, edit bodies, or
  suppression state.
- Do not implement later MiMo-informed roadmap tasks in this plan.

## Evidence and Current Behavior

- `src/agent/registry.rs::ToolExecutionResult` retains structured command
  observations and mutation receipts until the TUI coordinator converts them
  to presentation results.
- `src/main.rs::execute_pending_tools_async_with_email` owns completed tool
  batches, while provider continuation starts only after the batch is applied.
- `src/activity.rs::SoftPauseState` already models Tools, Provider, and
  Finalization boundaries, but only an explicit `/pause` request can enter it.
- Interactive approvals and cancellation are event-driven in `src/tui/input.rs`;
  headless and service runners must never invent an answer.
- MiMo files under `oracles/MiMo-Code-main/` are read-only behavioral evidence.
  This feature is a `ca` extension and not a Cursor parity claim.

## Scope

Allowed files: a focused detector module, tool-loop/service/headless
coordination, app/activity/TUI state and rendering, focused tests/snapshots,
and repository harness documents.

Forbidden files/areas: `oracles/`, `reference/`, user runtime data, installed
binaries, Git history, unrelated refactors, and external agent handoff.

Runtime state affected: bounded process-only detector observations and a
transient interaction. No new persistent state.

## Design and Invariants

- Observations are derived from typed tool names, arguments, execution status,
  command observations, and mutation receipts before transcript rendering.
- Edit comparison uses normalized three-token shingles and Jaccard similarity
  above 0.8 within the newest 12 edit observations. The third similar edit to
  one normalized target triggers an incident.
- A normalized failing command with unchanged normalized output triggers on
  its third occurrence. Four consecutive edit/verification actions without
  observable progress trigger a separate incident.
- Normalization removes temporary-path, timestamp, seed, duration, and large-
  number noise and bounds compared output to 2,000 characters.
- An incident can pause only after a completed batch and before the next
  provider continuation. Replan adds one process-only instruction, Continue
  resets and suppresses the guard for that turn, and Cancel remains
  Cancel-first.
- TUI lifecycle, route/session changes, completion, failure, and cancellation
  clear detector state. Service mode exposes a typed interaction; unattended
  modes terminate with a typed `low_yield_loop` failure.

## Milestones

### M1 — Detector characterization

- Add focused normalization, similarity, threshold, reset, and false-positive
  tests.
- Implement the bounded detector independently of transcript presentation.
- Validate the focused module tests.

### M2 — Tool-loop integration

- Produce typed observations from interactive and unattended tool execution.
- Gate the provider-continuation boundary and implement reset/suppression.
- Cover headless failure and service interaction behavior.

### M3 — TUI interaction

- Add deterministic evidence display and Replan/Continue/Cancel input states.
- Preserve Cancel-first behavior and exact-boundary resumption.
- Add wide and narrow snapshots plus focused input tests.

### M4 — Completion

- Run repository completion gates and a disposable native PTY walkthrough.
- Reconcile architecture, continuity, testing evidence, index, and queue.
- Archive this plan only after all claimed checks have actual results.

## Acceptance Criteria

- Each exact threshold triggers once with bounded sanitized evidence, while
  changed command output, meaningful verification progress, and reset
  boundaries avoid false incidents.
- Interactive work cannot continue past an incident until the user replans,
  continues, or confirms cancellation.
- Replan affects exactly the next provider continuation without becoming
  transcript/session content; Continue disables only the current turn.
- Headless and scheduled work returns `low_yield_loop`; service mode requests a
  structured decision and does not choose automatically.
- No oracle, reference, user profile/session, credential, or installed binary
  is modified.

## Validation

Focused: PASS. `cargo test loop_guard --no-fail-fast` ran 13 detector/input/
render tests; `cargo test low_yield --no-fail-fast` ran three headless/service/
orchestration tests.

Completion: PASS. Default: 1,479 main plus 37 CLI tests, one ignored.
All-target/all-feature: 1,508 main, 118 experiment, and 37 CLI tests, two
ignored. Format check, strict all-feature Clippy, all-feature release,
help/version, diff check, and snapshot hygiene all passed.

Manual/visual: PASS. Reviewed four wide/narrow warning/confirmation snapshots.
A released disposable 80x24 no-session TUI with a credentialless loopback
provider paused after three identical `false` failures, kept the turn paused by
default in cancellation confirmation, and completed a one-shot Replan with a
different response.

Blocked checks: none currently.

## Progress

- [x] Source boundaries and existing soft-pause behavior characterized.
- [x] Detector tests and bounded implementation complete.
- [x] Interactive, service, and unattended integrations complete.
- [x] TUI interaction and deterministic snapshots complete.
- [x] Completion gates and harness reconciliation complete.

## Decisions

- Decision: detector policy is fixed for V1 rather than exposed as mutable
  configuration.
  Evidence: the requested thresholds and normalization rules are explicit,
  while no configuration surface was requested.
  Consequence: behavior is deterministic and suppression is turn-local only.
- Decision: reuse the provider soft-pause boundary for interactive incidents.
  Evidence: it already prevents continuation without blocking the TUI loop.
  Consequence: loop recovery composes with existing cancellation and lifecycle
  cleanup instead of adding a competing execution lock.

## Surprises and Discoveries

- The first native PTY run exposed that changing `SoftPauseState` directly was
  insufficient to wake the coordinator: ordinary `/unpause` also carries an
  input action through the event loop. A typed `ResumeLoopGuard` action now
  performs that wake-up, and input tests assert it for both Replan and Continue.
- The existing `--yolo`/configuration state still presented the first
  run-command approval in the disposable walkthrough; selecting its explicit
  session yolo action allowed subsequent commands. This did not affect guard
  detection or user runtime state.

## Recovery and Rollback

The feature owns only process state. A targeted source rollback removes the
detector, dialog, and integration branches without data migration. Never use a
destructive Git reset or checkout to recover.

## Completion Record

Changed files: `src/loop_guard.rs`, `src/{activity,app,main,headless,service}.rs`,
`src/tui/{commands,input,input_tests,render,render_tests}.rs`, four loop-guard
snapshots, and repository harness/plan documents.

Commands and results: focused, default, all-feature, format, strict Clippy,
release, help/version, diff, snapshot hygiene, and native PTY checks all pass as
recorded above and in `TESTING.md`.

Remaining risks: similarity/progress policy is deliberately fixed and
conservative in V1. Detector observations are process-only, so a process crash
cannot explain a prior warning after restart; this is an intentional privacy
boundary.

Harness updates: `ARCHITECTURE.md`, `CONTINUITY.md`, `TESTING.md`,
`docs/.INDEX.md`, and `NEXT_STEPS.md` reflect the completed boundary and point
to TUI-129 as the next active task.
