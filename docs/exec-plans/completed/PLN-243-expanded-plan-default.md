# PLN-243 — Show all plan tasks by default

Status: COMPLETE
Last updated: 2026-09-05

## Goal
The inline plan shows every task and wraps complete task titles by default.

## Evidence and Current Behavior
The user's screenshot is UI evidence, not instructions to perform its plan.
`src/tui/render.rs::render_plan_card` takes three tasks and caps each entry at
two rows. The user requests the plan be shown by default. This is CA-specific
requested behavior, with no Cursor parity claim.

## Scope
Change the plan card, focused render tests/snapshots, and task documentation.
Preserve existing working-tree changes. No oracle/reference edits or user
config/session mutation. Install the validated release under standing authority.

## Design and Invariants
Remove task and row truncation. Preserve terminal sanitization, cell wrapping,
task status, existing transcript scrolling, review actions, and full reader.
Approval and execution behavior remain controlled by existing actions.

## Milestones and Progress
- [x] Reproduce hidden tasks and truncated long titles with a focused test.
- [x] Expand task rendering and review wide/narrow fixtures.
- [x] Run completion checks and background recoverable release/install.

## Acceptance Criteria
An approved six-task plan displays all six tasks without opening the reader;
long task titles retain their ending and status at wide and narrow widths.
Review actions remain accessible, including when the card exceeds the viewport.

## Validation
Focused plan tests; render suite; cargo test; format; strict Clippy; workspace
all-feature release and candidate/staged/installed verification. Record actual
results and any manual/visual limitation below.

## Recovery and Rollback
Undo only this task's edits. The installer preserves a timestamped hash-labelled
prior executable and restores it if post-install checks fail.

## Completion Record
Changed files: `src/tui/render.rs`, `src/tui/render_tests.rs`, three plan
snapshots, and continuity/queue/architecture/parity/testing/index documentation.

Failure-first regression reproduced the hidden content. Focused tests: 38 pass.
Full suite: 3,231 pass, zero failures, six ignored. Formatting, strict Clippy,
and diff whitespace pass. See `TESTING.md` for exact commands and logs.

Release PTY checks pass at 100x30 and 60x24, covering full task visibility,
approval-only, scrolling, graceful exit, and terminal restoration. Initial
fixture launch/trust timing/wrapped-text/exit assumptions were corrected;
production code was unchanged after the Rust checks.

Managed release/install job 85952 succeeded. Candidate/staged/installed SHA-256,
strict signatures, executable mode/owner and version/help match. Installed hash:
`9ed19d138e6f214722fe9209752a78bf507077f3a31341b426ae793a564180ac`.
Log: `/tmp/ca-pln243-release.log`; recovery path is recorded in `TESTING.md`.
Running sessions and user config/session data remain untouched.

No PLN-243 work remains. The separate known protected-tree baseline discrepancy
is not repaired; direct oracle/reference working-tree status is empty.
