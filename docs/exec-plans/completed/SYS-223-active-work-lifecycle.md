# SYS-223 — Active-Work Lifecycle

Status: COMPLETE
Owner/session: ORC-218 goal session
Last updated: 2026-08-29

## Goal

Keep the machine awake only while qualifying interactive model work is active,
and extend `await_job` with an ownership-preserving graceful stop followed by a
bounded wait and process-group escalation.

## Non-Goals

- No display-sleep inhibition, daemon, shell-evaluated helper, detached job,
  widened job ownership, or persistent change from process flags.
- No keep-awake activity for idle TUI state, direct-shell-only jobs, internal
  maintenance, service/ACP/headless work not explicitly covered by the user
  contract, or unsupported platforms.
- No replacement of `kill_job`; `await_job` remains the combined observation
  and exactly-once completion boundary.

## Evidence and Baseline

- `src/activity.rs`, `src/app.rs`, and `src/main.rs` own turn lifecycle and
  termination paths.
- `src/agent/jobs.rs`, `src/agent/tools.rs`, and `src/agent/registry.rs` own job
  origin, process-group cancellation, awaiting, and tool schema validation.
- CUR-204 already distinguishes model/direct/internal jobs and exactly-once
  completion claims. SYS-223 composes with those fields rather than adding a
  second job authority model.
- ORC-218 baseline: 868 status records, SHA-256
  `2dcaf84dc86d3ae27bfea593bc811ce16f46f9782382780901fdcf890ff39978`.

## Design

- Add saved `agent.keep_awake` defaulting to true and fresh-launch
  `--keep-awake`/`--no-keep-awake` process overrides. Older configurations use
  the default without eager rewrite.
- A shared reference-counted manager owns one fixed absolute helper child:
  macOS `/usr/bin/caffeinate -i -w <pid>` or the validated absolute Linux
  `systemd-inhibit --what=idle` form around a fixed sleep child. It uses no
  shell, reports unsupported platforms as a safe no-op, debounces idle release,
  and makes cleanup idempotent across cancellation, pane closure, restoration,
  signals, and normal shutdown.
- Qualifying leases are interactive model turns and model-originated jobs only.
  Acquisition/release follows existing lifecycle edges and origin transitions;
  duplicate events cannot underflow the count or leak helpers.
- `await_job` accepts optional `stop` and `grace_secs`. `stop:true` conflicts
  with `timeout_secs`, validates grace in 0–300 seconds, sends graceful
  process-group termination, waits, escalates if necessary, and returns final
  bounded status/output through the existing ownership and exactly-once claim.

## Validation

Add deterministic fake-helper/reference-count/unsupported/cleanup tests and
job fixtures covering graceful exit, grace-zero escalation, timeout conflict,
ownership, output preservation, and completion deduplication. Run focused
activity/job/tool/CLI/config/shutdown tests, formatting, strict all-feature
Clippy, all-feature release, diff/snapshot hygiene, and direct protected and
oracle/reference inspection. No real provider request or installation.

## Progress

- [x] Child plan and queue entry created after WFL-222 completion.
- [x] Failure-first keep-awake and stop/wait coverage added.
- [x] Keep-awake configuration, flags, and lifecycle manager implemented.
- [x] Combined job stop/wait implemented without ownership widening.
- [x] Focused suites and child completion gates pass.
- [x] Harness reconciled and plan completed.

## Completion Record

`src/keep_awake.rs` owns the no-shell helper actor, lease tracker, debounce,
platform specifications, process-group cleanup, and Linux parent-death guard.
`src/app.rs`/`src/main.rs` bind leases to interactive turn lifecycle and
process-only configuration. `src/jobs.rs` binds independent leases to
model-originated jobs and owns graceful termination/escalation. The typed tool
schema/handler and configuration/provenance paths were extended additively.

Focused results: keep-awake 3/3, jobs 29/29, job tools 30/30, configuration
101/101, CLI and provenance tests pass. Formatting, strict workspace
all-target/all-feature Clippy, workspace all-feature release, diff/snapshot
hygiene, and direct oracle/reference inspection pass. Only the existing macOS
oversized `__eh_frame` warning remains. No provider, runtime profile/session,
protected subtree, or installed binary was touched.

## Recovery

Reverse only SYS-223-labelled hunks and delete only SYS-created paths with
`apply_patch`. Never reset, clean, mutate oracle/reference trees, contact a
provider, alter a user profile/session, or install a binary.
