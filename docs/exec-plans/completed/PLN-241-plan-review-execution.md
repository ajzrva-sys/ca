# PLN-241 — Readable Plan Review and Reliable Execution

Status: COMPLETE (2026-09-04)
Owner/session: current Codex task
Last updated: 2026-09-04

## Goal

A completed plan is readable in the terminal, and Execute starts a fresh
current-agent request with normal tools in the existing workspace.

## Evidence and Current Behavior

- User screenshots show truncated diagnostic plan output, joined list/paragraph
  text, and Agent-labeled requests that still report planning-only authority.
- `finalize_completed_plain_response` leaves the plan turn in Interaction;
  approval reuses `active_turn_prompt` and its frozen canonical tools.
- `markdown.rs` does not flush at list/item endings.
- The existing approval test checks work state only and passes (1 test).
- This is a user-requested CA repair, not an oracle parity claim.

## Scope

Allowed: plan/work artifacts, main orchestration, app state, tool schemas,
Markdown and TUI rendering/input, session compatibility fixtures, focused tests,
snapshots, and harness documentation. Preserve existing unrelated changes.
Oracle/reference content is read-only. User runtime config/sessions are not
changed by validation. The standing release rule authorizes a recoverable
installation after validation; leave running sessions alone.

## Design and Invariants

- Pending review is distinct from an active provider turn. Execute validates
  the displayed plan ID/revision, saves approval, and starts once with fresh
  Agent instructions/tools. Graph grants are not main-agent authority.
- Inline card and full reader expose Execute/View/Revise/Dismiss; typed feedback
  works normally. Dismiss is not rejection. `/plan approve` approves only.
- Optional Markdown body is backward compatible. Material edits invalidate
  approval; progress does not. Only typed proposals offer execution.
- Preserve request/tool-result integrity, existing approval/workspace policy,
  evidence requirements, cancellation, and no automatic execution on restore.

## Milestones

1. Characterize stale approval and Markdown boundaries with failing tests.
2. Implement durable content, review controls, fresh execution handoff, and tests.
3. Review wide/narrow snapshots and exercise a disposable-home fake-provider PTY.
4. Run completion gates, background release/install, and reconcile documentation.

## Acceptance Criteria

- Proposal → Execute produces a fresh Agent request and a successful temporary
  edit/command, without the Plan restriction or stale tool list.
- Approval-only, revision, dismissal, stale/duplicate actions, save failures,
  cancellation, restoration, and incomplete graph cases have focused coverage.
- Markdown lists remain separate from following paragraphs; long/Unicode plans
  wrap, scroll, select, and copy with actions visible at 100x30 and 60x24.

## Validation

Focused: plan/work/input/request tests and Markdown boundary tests.
Completion: cargo test, cargo fmt --check, strict Clippy, release build/smokes;
all-feature workspace gates for final installation.
Manual: local fake-provider terminal walkthrough with disposable state only.
Known separate failure: recorded protected-tree oracle baseline mismatch.

## Progress

- [x] Failure-first characterization
- [x] Artifact and execution implementation
- [x] Presentation and interaction implementation
- [x] Focused, render, and terminal validation
- [x] Completion gates and verified background installation
- [x] Harness reconciliation

## Decisions

- Plan orchestration lives in `src/plan.rs`; main retains the event-loop entrypoints. Full saved history is loaded before an explicitly requested restored-plan execution. Explicit mode changes dismiss pending review.

- User selected inline card plus full reader and current-agent execution.
- Preserve graph execution as an explicit `/tasks` operation.
- Reuse ordinary terminal reader navigation/copy and tool policy boundaries.

## Surprises and Discoveries

Both regressions failed before changes: approval retained an active turn (21 other matching tests passed); the Markdown fixture rendered `• testsApprove the plan.`. Logs: `/tmp/ca-pln241-characterization.log` and `/tmp/ca-pln241-markdown-before.log`.

The final normal focused suite passes 37 tests; Markdown passes 20; render
coverage passes 224 with one unrelated manual test ignored. All five paired
plan snapshots and three intentional list-spacing updates were reviewed.
Disposable-home loopback PTY walkthroughs pass at 100×30 and 60×24, including
proposal, full reader, typed revision, dismissal/reopening, and file write/read
via fresh Agent tools. Each made exactly seven physical fixture requests.
Early PTY attempts exposed a fixture issue: stripping ANSI sequences does not
reconstruct terminal redraws. The corrected fixture uses a screen emulator and
checks visible screens; both dimensions pass. Logs: `/tmp/ca-pln241-manual-wide-final.log` and `/tmp/ca-pln241-manual-narrow-final.log`.

## Recovery and Rollback

Task-local before-images live in the private temporary validation directory.
Revert only this task's hunks, never the pre-existing working tree. Installation
must keep a timestamped SHA-256-labelled prior executable, stage on destination
filesystem, replace atomically, verify hashes/signature/mode/owner/help/version,
and restore the prior artifact if post-install checks fail.

## Completion Record

Implemented in `src/plan.rs`, `src/main.rs`, `src/work.rs`, `src/app.rs`,
`src/agent/registry.rs`, `src/markdown.rs`, and TUI input/render/command files.
Session fixtures, focused tests, paired snapshots, and harness pages were
updated. Existing unrelated changes and oracle/reference evidence were preserved.

- Final focused plan tests: 37 passed (`/tmp/ca-pln241-focused-final3.log`).
  The last assertion-only refinement verifies cancellation and restoration of
  actual pending review/start state; it was rerun after the broad build.
- Markdown: 20 passed. Render: 224 passed, one unrelated manual check ignored.
- Full all-feature workspace: 3,373 passed, one failed, seven ignored. The
  sole failure was an unrelated status-line sandbox timeout; its unchanged
  compiled test passed immediately in isolation. The broad command exited 101
  and remains recorded as such. Every other target passed, including MCP SDK
  8/8. Logs: `/tmp/ca-pln241-workspace-tests.log` and
  `/tmp/ca-pln241-status-sandbox-rerun.log`.
- Final formatting, strict all-target/all-feature workspace Clippy, diff
  whitespace, and zero pending snapshots: passed. The debug test build emitted
  the existing non-fatal oversized macOS unwind-section linker warning.
- Debug terminal walkthroughs at 100×30 and 60×24 and release walkthrough at
  60×24: passed using disposable homes and a loopback fake provider. Each
  produced exactly seven requests and a verified two-line proof file. No live
  provider inference or user runtime config/session mutation was used.
- Protected-tree check: existing committed oracle baseline mismatch remains.
  Direct Git status for oracle/reference content is empty; no receipt changed.
- Background release/install job 42112: succeeded. Workspace all-feature release
  build, candidate/staged/installed hash agreement, executable mode/ownership,
  strict platform signature, and version/help checks passed. The prior binary
  was copied recoverably and replacement was atomic; running sessions were left
  alone. Installed `/Users/aji/.local/bin/ca` SHA-256: `d9cbe4aac3947fb79eba4510ecafba0f53513f526045fba5c91ce41c6efa88e6`.
  Recovery copy: `/Users/aji/.local/bin/.ca-recovery-20260904T042329Z.jRN6nS/ca.previous-0336bd2050b525d71959f885af40ccc603873f534a0821b85e8bdd3c53fd0192`.
  Job/script/log: `/tmp/ca-pln241-release.12xj4sgt/`.

PLN-241 has no remaining implementation work. The separate protected-tree
baseline discrepancy is retained in the repository's existing validation record.
