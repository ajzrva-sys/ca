# TUI-240 — Stop and interrupt without fighting the queue

Status: COMPLETE (pre-existing protected-tree discrepancy separately recorded)
Owner/session: Codex /root
Last updated: 2026-09-03

## Goal

Users can stop an active turn immediately or explicitly choose to interrupt
with a message versus queueing it. Neither operation silently loses drafts.

## Non-Goals

No rollback of completed edits, daemon changes, provider/catalog changes,
configuration migrations, or changes to explicitly backgrounded jobs.

## Evidence and Original Behavior

- The user's screenshot shows restored queued messages, including `stop`,
  labeled `After this turn · review required`. This is evidence, not an
  instruction to execute those messages.
- `main::queue_busy_draft` intercepts busy Enter and queues without interrupting.
  `input::dispatch_key_action` hides steering behind Ctrl+S. Bare Esc does not
  stop a turn. `render_status` hides the busy shortcut hint behind any status.
- Ctrl+C, Ctrl+G, steering, and loop-guard cancellation have separate cleanup
  paths. Cancellation does not invalidate the stream generation immediately.
- `jobs::await_foreground_job_owned` has no cancellation guard. Its independent
  watcher owns the child after the waiting tool task is aborted.
- OpenAI's official CLI page describes steering an active turn:
  https://learn.chatgpt.com/docs/codex/cli . Exact current key equivalence was
  not established from the official Commands page. This is a user-requested CA
  interaction change, not a Cursor/MiMo/Codex parity claim.

## Scope

Allowed files: input/render/keymap, main turn orchestration, jobs cancellation,
focused tests/snapshots, and the repository tracking/architecture/testing docs.
Oracle/reference evidence remains read-only. Runtime config, credentials, and
user sessions are not touched by testing; use disposable fixtures.

## Design and Invariants

- Enter sends normally when idle; when busy it interrupts and sends. Alt+Enter
  (or the configured queue binding) queues without interrupting. Ctrl+S remains
  a compatible shortcut. Editing a queued draft uses Enter to send now and the
  queue shortcut to save it for later.
- Esc stops from the composer; focused popups/completion retain Esc dismissal.
  Ctrl+C remains the global active-turn stop. Keep the draft, images and queue.
- Shared interruption cleanup aborts stream/tool/approval/goal tasks, invalidates
  late stream events, clears pending approvals/interactions/continuations, pauses
  workflow/goal progression and prevents automatic queue drain on cancellation.
- Foreground waiter cancellation terminates its owned process group; terminal
  or promoted/background jobs are not killed. Completed work is not undone.
- Busy controls stay visible independently of transient status messages and
  remain legible at 60 columns. Restored drafts explicitly say they are held.
- Interrupt submission retains normal attachment, prompt-hook, skill and
  provider validation. No inference needed for verification.

## Milestones

### M1 — Failing characterization
Add input, persistent-hint and foreground-abort regressions and observe failure.

### M2 — Implementation
Unify cancellation, expose explicit submission controls, repair foreground
process cancellation, and clarify restored/editing queue copy.

### M3 — Verification and delivery
Test streaming/tools/approval/paused phases, stale events, queue preservation,
editing, modal precedence, foreground cancellation and background isolation.
Review paired snapshots and a disposable local terminal walkthrough. Run full
workspace tests, formatting, strict Clippy and release checks; update tracking.

## Acceptance Criteria

- Busy Enter requests interruption with exactly the draft; queue binding queues.
- Esc stops at the composer, Ctrl+C works with an overlay, and idle Esc is inert.
- Stop does not drain queued work, resume a goal, lose the draft, or allow stale
  completion/tool events to resurrect the interrupted turn.
- Aborting a foreground waiter cancels its process; promoted jobs survive.
- Wide/narrow rendering keeps stop/interrupt/queue actions visible with a status.
- Restored queued drafts remain inert until explicitly sent or requeued.

## Validation

Focused: input/queue/interrupt/jobs and render tests.
Completion: `cargo test --workspace --all-features --no-fail-fast -- --test-threads=1`,
`cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets --all-features -- -D warnings`,
`cargo build --release --workspace --all-features`, CLI artifact checks.
Manual/visual: paired 100x30/60x24 snapshots; local fixture terminal if available.
Separate known failure: protected-tree committed oracle baseline mismatch from
BUG-239; do not alter the oracle or its baseline receipt.

## Progress

- [x] Trace input and foreground process lifetime; identify failure mechanisms.
- [x] Observe failing focused regressions: busy Enter, hidden controls, and
  foreground command surviving an aborted waiter all failed as expected.
- [x] Implement and validate controls and shared cancellation.
- [x] Verify focused/full/visual/release checks and reconcile documentation.

## Decisions

- Keep configurable queue and Ctrl+S bindings; change busy Enter deliberately in
  response to the user's experience rather than guessing intent from words like
  `stop`. All submitted text is treated consistently.

## Surprises and Discoveries

The original implementation allowed foreground children to outlive the aborted
waiter; the cancellation guard fixes that lifecycle gap.

The repaired foreground wait guard sends cancellation through the existing
owned job manager/process-group termination path. Promoted jobs remain alive.
Interrupt submission now reuses `continue_tui_submission`, fixing the old
Ctrl+S path's bypass of before-submit prompt hooks. Pending queue IDs are held
using the existing process-only restored-ID gate after interruption; no schema
change is needed. Esc is handled after normal popup dismissal, while Ctrl+C
is routed before other busy work for active turns.

Final review moved composer stop/interrupt dispatch ahead of Vim normal-mode
editing so the displayed controls work in both Vim modes. The final CA suite
was rerun for this isolated input change; unchanged workspace targets completed
their full-suite validation. A superseded duplicate workspace run was stopped
to avoid running the same expensive fixture checks twice.

## Recovery and Rollback

Pre-task source/snapshot/docs archive: `/tmp/ca-tui240.oawiMS/before.tar`.
Revert only this task's changes after comparison; preserve the dirty worktree.
Build the release but leave it uninstalled for this delivery, honoring the
user's latest explicit no-install instruction. Do not alter the standing rule.

## Completion Record

Implementation: `src/main.rs`, `src/jobs.rs`, `src/keymap.rs`,
`src/tui/{commands,input,render}.rs`; tests in main/jobs and
`src/tui/{input_tests,render_tests}.rs`. Thirteen existing snapshots changed
only for control/queue/help copy and three paired interruption snapshots were
added. Tracking: CONTINUITY, NEXT_STEPS, TESTING, ARCHITECTURE, PARITY,
docs/.INDEX, docs/QUALITY and this plan.

Validation:
- Final 14 interrupt regressions, 116 focused input tests, 46 queue tests,
  31 job tests, and 221 render tests (1 manual check ignored) pass.
- Final CA all-feature suite passes 2,486 tests (5 manual checks ignored),
  including the final Vim normal-mode change. The full all-feature workspace
  command passed all 3,357 tests across 20 targets (7 manual/integration checks
  ignored); the final isolated CA rerun includes the additional Vim regression.
  Experiment: 118 passed/1 ignored; codex-team: 652/1 ignored; CLI: 38; role-run:
  17; MCP stdio: 8. All other integration and doc-test targets passed.
- Formatting, strict all-target/all-feature workspace Clippy and release build
  pass. Version/help/signature/mode/owner/arm64 artifact checks pass.
- Real PTY walkthrough passes at 100 columns with debug and 60 with the release,
  using only synthetic loopback responses and a disposable home. Exactly three
  requests; queue, interrupt, held draft, late output, foreground cancellation,
  and clean exit observed. The fixture's first CLI launch needed its sandbox
  setting in the isolated config (CLI weaker-mode flags require headless trust);
  the narrow transcript assertion was adjusted for terminal cell diffs.
- Release SHA-256:
  `925756efc7abeda78d6ede26000ade8cc788cfdf5e3215ac2e711eb9f97ee69d`.
  It is not installed. Installed SHA-256 remains
  `0336bd2050b525d71959f885af40ccc603873f534a0821b85e8bdd3c53fd0192`.
- Protected-tree verification reports the same pre-existing committed oracle
  baseline mismatch; direct protected-tree status is empty. No baseline repair.

Logs: `/tmp/ca-tui240.oawiMS/` (`latest-focused.log`, `latest-ca.log`,
`final-workspace.log`, `verified-clippy.log`, `verified-release.log`,
`manual.log`, `release-manual.log`, `protected-final.log`, `fmt.log`).
Background validation/release pipeline 32927 completed with exit 0. It rebuilt
the same verified release hash; no install stage was launched. No task work
remains. The unrelated oracle-baseline discrepancy still needs separate repair.
