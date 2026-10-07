# PLN-244 — Limit the plan card to review

Status: COMPLETE
Last updated: 2026-09-05

## Goal
Show the expanded plan automatically for review, without a persistent card
during execution or after approval/dismissal. `/plan show` remains available.

## Evidence and Scope
The screenshot is UI evidence only. `render_plan_card` is appended whenever a
stored artifact exists, ignoring the existing `plan_proposal` review lifetime.
`plan.rs` already clears that binding on approval, dismissal, revision, and
successful execution. Change only the renderer, reader-close input handler, focused tests/snapshots, and
task documentation. Preserve unrelated changes and oracle/reference content.

## Design
Render the inline card only for a pending review while no foreground turn is
active. Keep every task and full wrapping from PLN-243. Keep the artifact and
existing reader, approval, and execution transitions intact.

## Acceptance and Progress
- [x] Failure-first coverage: approving/dismissing/revising/rejecting clears the
  card, busy work and restored artifacts have no card, explicit reader works.
- [x] Review still shows every task; deterministic wide/narrow fixtures pass.
- [x] Focused/full tests, format, strict lint, release PTY and recoverable install.

## Validation and Recovery
Use TESTING.md commands, record actual failures, preserve a recoverable installed
binary under standing authorization, and verify replacement. No user config or
session mutation; manual fixture state stays in temporary directories.

## Discovery
The release PTY caught a real remaining path: closing the full reader retained
`plan_proposal`, restoring the inline card. The input handler previously needed
a second Esc. Route reader Esc/q through `plan::dismiss` and exercise real input
in the regression. Pre-fix terminal evidence is in
`/tmp/ca-pln244-release-reader-before.log`; no installation occurred.

## Completion
Changed source: `src/tui/render.rs`, `src/tui/input.rs`; tests:
`src/tui/render_tests.rs`, `src/tui/input_tests.rs`, paired active/review snapshots.
Updated continuity, queue, architecture, parity, testing and knowledge index.

Failure-first renderer and real-reader regressions passed after repair. Final
focused tests: 40 pass; final CA suite: 2,534 pass, five ignored. Broad workspace
suite before the reader repair: 3,232 pass, one unrelated codex-team commit test
failure, six ignored; the failed test passed unchanged in isolation. Preserve
that failure rather than claiming a clean broad pass. Exact commands/logs are
in `TESTING.md`. Formatting and strict all-target Clippy pass.

Debug and release PTY walkthroughs pass at 100x30 and 60x24: full review,
dismissal, explicit reader, reader closure, approval-only, execution, completion,
and terminal restoration. Background job 32794 installed the verified release;
log `/tmp/ca-pln244-release.log`. Installed SHA-256:
`77a071bc3abb82dd9cd367b5c76e8aa4617e26a38df2826f3303e6091a76e5ad`.
Recovery copy and full verification are recorded in `TESTING.md`.
No remaining PLN-244 work. User runtime sessions/config and protected content
were untouched; existing protected-tree baseline discrepancy remains separate.
