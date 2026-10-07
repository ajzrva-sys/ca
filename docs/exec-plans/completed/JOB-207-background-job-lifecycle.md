# JOB-207 — Background-Job Lifecycle

Status: COMPLETE (2026-08-28)
Owner/session: CUR-204 goal session
Last updated: 2026-08-28

## Goal

Support safe foreground promotion and exactly-once automatic completion turns
for model-owned background jobs in TUI and headless runs.

## Non-Goals

No detached OS daemons, automatic turns for direct `!&`, ownership transfer,
or continuation outside existing turn budgets.

## Evidence and Current Behavior

Before this plan, jobs distinguished foreground/background but not origin or
delivery. Model tools had to call `await_job`, and headless shutdown cancelled
remaining work. CA's existing host-authored detached-agent completion boundary
was reused rather than creating user-authored turns.

## Scope

Allowed: jobs/tool/headless/main/app/TUI event/input/render code and focused
tests. Preserve active agent-lifecycle plans and cancellation authority.

## Design and Invariants

Persist process-local origin and delivery state. Double Ctrl+B for the same
owned foreground job within 1.5 seconds promotes without restart, timeout, lost
output, or changed cancellation owner. `await_job` and auto-delivery share one
atomic exactly-once marker. Idle model-origin completions batch into one bounded
host system message and one continuation. Headless waits up to 1800 seconds by
default; `--single-turn` records explicit shutdown outcomes. Each follow-up
spends `max_turns`.

## Milestones

### M1 — Origin/delivery state and races

Failure-first completion, await/delivery, cancellation, and timeout-boundary
tests; add atomic lifecycle state.

### M2 — Promotion and TUI

Implement double-key hint/promotion for model tool and direct shell jobs with
PTY timing coverage and no process restart.

### M3 — Automatic and headless continuation

Implement batching, direct-job exclusion, turn limits, default wait, ceiling,
and single-turn behavior; validate.

## Acceptance Criteria

All CUR-204 race/promotion/batching cases pass; every completion is delivered at
most once; quit still cancels; direct background commands never spend a turn.

## Validation

Focused jobs/tools/headless/agent-continuation/input/render/PTY tests plus child
gates.

## Progress

- [x] Failure-first lifecycle tests.
- [x] Origin/delivery and promotion.
- [x] Automatic/headless continuation.
- [x] Focused and child gates.

## Decisions

Reuse host-authored continuation boundaries rather than inventing user messages.

## Surprises and Discoveries

- Headless tests often use fixed run IDs. Reusing those IDs as job owners let a
  completed job from one test appear in another test. Every headless and role
  run now receives an independent process-local job owner.
- Automatic completion must claim only while the conversation is idle. Claiming
  at completion time could race a model's explicit `await_job`; delaying the
  atomic claim until idle preserves exactly-once delivery.
- Two full-workspace attempts exposed different nondeterministic tests in the
  already-dirty protected `codex-team` subtree. Both exact tests passed on their
  immediate isolated reruns; a final clean workspace rerun is recorded below.

## Recovery and Rollback

Cancel disposable jobs; reverse only JOB-207 state/event fields and tests.

## Completion Record

Implemented explicit model-tool, direct-shell, and internal job origins plus an
atomic pending/reserved/explicit/automatic delivery state. Owned foreground
jobs can be promoted without restart, output loss, timeout carryover, or owner
change. TUI Ctrl+B uses a same-job 1.5-second two-press confirmation. Idle model
completions batch into one bounded host system message and one continuation;
direct `!&` jobs never do. `await_job` shares the marker. Headless runs wait up
to 1,800 seconds by default and count follow-ups against `max_turns`;
`--single-turn` cancels remaining model jobs and emits a typed outcome.

Focused results: 27 job tests, model-completion/headless continuation,
single-turn, max-turn, batching, parser, and promotion-timing tests pass. The
final serial workspace run passes: CA 2,175 tests with four documented ignores,
CLI 38, role-run 17, ca-watch 15 library plus three CLI, codex-team 647 library
with one documented ignore plus 16 integration, and protocol 12. Formatting,
strict all-feature Clippy, all-feature release build, release help, diff and
snapshot hygiene, baseline-protected status comparison, and protected-tree
verification pass. No runtime profile, installed binary, oracle, or reference
state was changed.
