# TUI-130 — Optional Turn Metadata

Status: COMPLETE
Owner/session: Codex / current user-requested implementation
Last updated: 2026-08-16

## Goal

Add default-off transcript timestamps and compact final-assistant turn footers,
plus optional statusline generation-speed and sitting-cost fields, while
keeping old sessions and unknown usage/pricing states honest.

## Non-Goals

- Do not add a drawer, side panel, new provider estimates, or retroactively
  invent old-session metadata.
- Do not modify oracle/reference code or copy MiMo implementation source.
- Do not implement later roadmap tasks in this plan.

## Evidence and Current Behavior

- `src/app.rs` already records message timestamps plus optional reasoning timing
  annotations; `src/session.rs` owns backward-compatible session records.
- `src/usage.rs` and the read-only `/context` dashboard already distinguish
  incremental, subscription allocation, mixed, unknown cost, and foreground
  output speed.
- `src/ui_settings.rs` owns statusline fields and comment-preserving profile
  persistence; transcript rendering is centralized in `src/tui/render.rs`.

## Scope

Allowed files: config/UI settings, backward-compatible transcript annotations,
turn terminal metadata capture, transcript/statusline rendering, tests,
snapshots, and harness documents.

Forbidden: provider protocol changes, inferred historical values, cost
estimation without usage/pricing, drawer state, oracle edits, runtime-state
mutation during validation, unrelated refactors, and external agents.

## Design and Invariants

- Timestamp and assistant-footer display settings default off and affect
  presentation only.
- Exactly one footer is anchored to the final visible assistant message of a
  turn. Optional provider/model/duration/outcome fields remain absent when
  unknown; interrupted is explicit.
- Old session/config records deserialize unchanged and are not rewritten by
  reads. New fields are optional and bounded.
- Statusline speed and cost reuse the exact `/context` current-sitting
  calculations. Unknown, incremental, subscription-allocation, and mixed cost
  remain distinct and compact.

## Milestones

### M1 — Metadata model and lifecycle

- Characterize annotations, terminal turn paths, session compatibility, and
  current usage calculations.
- Add optional typed fields and one terminal recording boundary with legacy
  fixtures.

### M2 — Configuration and rendering

- Add default-off transcript settings and statusline field identities.
- Render timestamps and a single compact footer across wide/narrow transcript
  layouts without disturbing default snapshots.

### M3 — Validation and completion

- Prove all duration/outcome/cost/speed unknown and mixed states.
- Review deterministic snapshots, run full gates and a disposable native PTY,
  then reconcile harness documents and archive this plan before INP-131.

## Acceptance Criteria

- Existing default transcript/statusline output remains unchanged.
- Enabled timestamps and footers display only stored values, on exactly one
  final assistant message, including interrupted when recorded.
- Optional statusline speed/cost exactly agrees with `/context` and never
  converts missing timing, usage, limits, or pricing into a number.
- Old sessions/configs load without migration writes; no oracle, credential,
  installed binary, or real user state changes.

## Validation

Focused: PASS. Lifecycle annotation, session compatibility, duration, usage
summary, UI config/input, and transcript/statusline render tests pass.

Completion: PASS. `cargo test` passed 1,497 main plus 37 CLI tests with one
ignored. `cargo test --all-targets --all-features` passed 1,526 main, 118
experiment, and 37 CLI tests with two ignored. Format, strict all-feature
Clippy, all-feature release, release help/version, diff, and snapshot hygiene
pass.

Manual/visual: PASS. Reviewed wide/narrow deterministic snapshots cover
completed/interrupted footers and all cost states. A released disposable 80x24
no-session loopback PTY showed timestamps, one 705ms footer, 18.3 tokens/s, and
unknown cost. Fixture: `/Users/aji/.Trash/ca-tui130-pty.Tv20E5`.

Blocked checks: none currently.

## Progress

- [x] Existing annotation, lifecycle, UI config, and usage seams characterized.
- [x] Optional durable metadata and compatibility coverage complete.
- [x] Transcript/statusline configuration and rendering complete.
- [x] Snapshots, native walkthrough, completion gates, and harness complete.

## Decisions

- Decision: reuse current-sitting usage summaries rather than add a second cost
  or speed calculator.
  Consequence: `/context` and statusline cannot silently disagree.
- Decision: keep presentation settings outside session content.
  Consequence: enabling the feature can reveal stored metadata in old sessions
  only when that metadata genuinely exists.

## Surprises and Discoveries

- The central `TurnLifecycle::take_completion` boundary already observes every
  completed, failed, and cancelled interactive terminal path, so metadata did
  not need fragile per-branch writes.
- MiMo Token Plan classification requires explicit cache-hit and cache-write
  counters; the fixture now exercises that real fail-closed billing contract.

## Recovery and Rollback

All new durable fields are optional and display settings default off. A
targeted rollback can ignore them without rewriting sessions or configuration.
Never use destructive Git reset/checkout for recovery.

## Completion Record

Changed files: `src/app.rs`, `src/main.rs`, `src/session.rs`,
`src/ui_settings.rs`, `src/usage.rs`, `src/tui/input.rs`,
`src/tui/render.rs`, focused tests/snapshots, and harness documents.

Commands and results: all focused/default/all-feature/format/Clippy/release/
help/version/diff/snapshot checks above passed.

Remaining risks: values remain intentionally unknown until providers report
usage/timing and a configured pricing authority exists.

Harness updates: architecture, continuity, testing evidence, index, and queue
are reconciled; INP-131 is active next.
