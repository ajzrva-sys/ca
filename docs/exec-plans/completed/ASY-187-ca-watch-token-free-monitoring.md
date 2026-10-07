# ASY-187 — Dedicated `ca-watch` Utility and Token-Free Monitoring

Status: COMPLETE
Owner/session: Codex / current user-requested implementation
Last updated: 2026-08-26

## Goal

Add a reusable Rust library and standalone `ca-watch` CLI for observing local
or SSH-hosted PIDs, then consume its typed events in interactive CA sessions so
background activity remains visible without provider polling or token use.

## Non-Goals

- No daemon, persistence, runtime-profile mutation, installed-binary change, or
  heuristic parsing of assistant prose.
- No signaling or termination of watched external processes.
- No process adapters beyond generic PID identity and copy/log/marker watches.
- No live SSH/BSD validation, password/private-key handling, forwarding, or
  arbitrary remotely supplied commands.
- No oracle/reference edits or copied implementation source.

## Evidence and Current Behavior

- `docs/exec-plans/completed/TUI-186-adaptive-tool-output-layout.md` is complete,
  satisfying this task's rendering prerequisite.
- `src/jobs.rs` owns process-scoped shell children, bounded stdout/stderr,
  cancellation, timeouts, and shutdown cleanup. It has no representation for
  externally owned local or remote PIDs.
- `src/main.rs` consumes `JobEvent` in the Tokio TUI loop and redraws without a
  provider call. This is the existing event-only integration boundary.
- `src/tui/render.rs::render_jobs_view` and related input handling implement a
  shell-job-only `/jobs` surface. There is no background activity strip.
- `src/agent/registry.rs`, `src/agent/tools.rs`, and `src/workspace.rs` jointly
  define tool exposure, execution, validation, and approval preparation.
- The root workspace currently contains `ca` and `crates/codex-team`; no
  `ca-watch` package exists.
- Inferred: the existing process-scoped manager and owner model is the safest CA
  integration pattern, while transport/process adapters belong in the sibling
  crate so TUI code remains independent of probe mechanics.

## Scope

Allowed files: root workspace metadata; a new `crates/ca-watch/` package; focused
CA watcher integration in app/event-loop/tool/workspace/TUI/job code; focused
tests and snapshots; model guidance; and reconciled harness documents.

Forbidden files/areas: `oracles/`, `reference/`, nested repositories, user
configuration/sessions, credentials, installed binaries, unrelated dirty-tree
changes, and live remote targets.

Runtime state affected: process-local watcher tasks and read-only local process,
log, and marker observations during explicit use. Tests use only short-lived
local fixtures and fake SSH transports.

## Design and Invariants

- `ca-watch` owns versioned request/event types, validation, per-watch sequence
  ordering, cancellation, snapshots, broadcasts, transport, and adapters.
- A watch captures an opaque process-identity fingerprint on its first
  successful probe. Identity material never appears in summaries or errors.
- Copy probes check the marker before process identity and expose only the last
  nonempty bounded log line. A missing PID is terminal only after a successful
  transport probe.
- SSH execution always starts `/usr/bin/ssh` with fixed batch/no-TTY/no-forward
  options, a bounded timeout, a validated destination/port, and one fixed probe
  program whose typed values are separately quoted. It never accepts shell
  source, password/key content, or forwarding configuration.
- Transport failure first reports `Degraded`, progresses to `Unreachable`, and
  retries with capped backoff. A later successful probe returns to adapter state.
- CA wraps the library service with session ownership. Stopping a watcher or
  shutting down CA cancels observation only; `JobManager` remains solely
  responsible for CA-owned child cleanup.
- `watch_process` and `stop_watcher` are registered only on the interactive main
  surface. Plan, headless, service, scheduled, and subagent catalogs omit them.
  Remote requests prepare an exact network-bearing approval identity.
- Watch events update process-local application state, activity presentation,
  and terminal toasts. They do not add conversation messages, begin model turns,
  call a provider, or modify token accounting.
- Foreground shell timeouts remain unchanged. Explicit background shell jobs do
  not inherit the foreground wall-clock timeout and remain visible until
  completion, cancellation, or shutdown.

## Milestones

### M1 — Crate contract and characterization

- Add the workspace package, versioned public types, bounded validation, adapter
  and transport seams, event service, and focused failing-then-passing tests.
- Add the standalone CLI with text/JSONL output and terminal exit behavior.
- Validate with `cargo test -p ca-watch -- --test-threads=1` and CLI fixtures.

### M2 — CA tools, ownership, and approval

- Add the process-scoped CA manager and exact TUI-only tools.
- Add remote network preparation and stable owner-scoped watcher IDs.
- Add catalog, validation, approval-identity, lifecycle, and token-isolation tests.

### M3 — Unified activity presentation

- Consume watch broadcasts in the Tokio event loop.
- Add the conditional activity strip, unified `/jobs` list/detail/control paths,
  and completion/failure toasts with deterministic wide/narrow coverage.
- Keep watcher history for the CA process lifetime and disable watcher stdin.

### M4 — Reconcile and complete

- Update model guidance and durable architecture/testing/continuity/task docs.
- Run all focused checks, workspace completion gates, snapshot/diff/protected-tree
  hygiene, and release help/version checks for `ca` and `ca-watch`.
- Move this plan to `completed/` only after recording actual results.

## Acceptance Criteria

- Valid local generic and copy watches emit strictly increasing versioned events
  and reach the specified terminal states without signaling the target PID.
- Identity drift fails; marker-first copy completion wins; PID disappearance
  produces `Exited` for generic and `Failed` for incomplete copy work.
- Transport outages never become target failure, recover after a successful
  fake probe, and obey bounded retry backoff.
- SSH argument/script tests prove fixed construction, hostile input rejection,
  redacted diagnostics, and zero network traffic.
- Both documented CLI command shapes validate; JSONL round-trips the public
  event form; invalid input and Ctrl+C have deterministic exit behavior.
- Only interactive CA agent catalogs expose watcher tools; remote approvals bind
  destination, PID, kind, paths, cadence, and network capability exactly.
- Repeated watcher polling/event delivery leaves provider-call and token counts
  unchanged.
- The activity strip and `/jobs` distinguish managed jobs from local/remote PID
  watches, bound progress, expose health/cadence, prohibit watcher stdin, and
  describe stop-watching as non-terminating.

## Validation

Focused:

- `cargo test -p ca-watch --no-fail-fast -- --test-threads=1`: PASS, 15
  library tests and three CLI integration tests.
- `cargo test -p ca tui::render_tests -- --test-threads=1`: PASS, 185 tests.
- `cargo test -p ca tui::input_tests -- --test-threads=1`: PASS, 107 tests.
- Focused registry, owner, tool execution, approval identity, workspace network,
  schedule rejection, background timeout, and token-isolation tests: PASS.

Completion:

- `cargo test --workspace --no-fail-fast -- --test-threads=1`: PASS, 2,040 CA
  tests, 38 CA CLI tests, 17 role-run tests, 15 watcher library tests, three
  watcher CLI tests, 556 embedded scheduler tests, and all integration/doc
  targets; three documented manual/external tests ignored.
- `cargo fmt --all -- --check`: PASS.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: PASS.
- `cargo build --workspace --release --all-features`: PASS.
- Release help/version checks: PASS for `ca 0.1.0` and `ca-watch 0.1.0`.
- Diff, snapshot, and protected-tree hygiene: PASS.

Manual/visual: wide/narrow activity-strip, unified `/jobs`, watcher detail, and
completion/failure toast snapshots reviewed and accepted.

Blocked checks: live SSH/BSD execution is intentionally out of scope; fake
transport coverage must establish the remote contract without network access.

## Progress

- [x] TUI-186 prerequisite and current repository boundaries verified.
- [x] `ca-watch` library and CLI implemented with focused tests.
- [x] CA tools, ownership, approval, and lifecycle integrated.
- [x] Unified activity strip, `/jobs`, and toasts implemented and reviewed.
- [x] Guidance and durable harness documents reconciled.
- [x] Completion gates pass and the plan is archived.

## Decisions

- Decision: link CA to the `ca-watch` library instead of spawning its CLI.
  Evidence: the requested event stream must feed the existing Tokio loop without
  serialization overhead or sibling-process lifecycle ambiguity.
  Consequence: CLI and CA share validation/adapters while remaining independently
  runnable binaries.
- Decision: use explicit typed registration only.
  Evidence: the user excludes prose heuristics and requires stable approval
  identity.
  Consequence: the model makes one authorized tool call after learning a PID;
  all subsequent observation is Rust-only.

## Surprises and Discoveries

- The working tree already contains a very large set of user changes. ASY-187
  edits must remain narrowly scoped and be reviewed with path-focused diffs.
- The pre-existing `JobManager` applied the configured wall-clock timeout to
  background jobs as well as foreground jobs. ASY-187 now characterizes and
  preserves foreground timeout behavior while letting explicit background jobs
  run until completion, cancellation, or shutdown.

## Recovery and Rollback

Remove only the `crates/ca-watch` workspace member and ASY-187-specific CA
integration/tests/docs, then restore the prior root workspace metadata with an
inverse patch. No persisted state migration or external-process cleanup is
required because watchers are process-local and never signal observed PIDs.

## Completion Record

Changed files: root workspace metadata; the new `crates/ca-watch/` library/CLI
and tests; CA process-watch manager, registry/tool/workspace/event-loop/job and
surface filters; activity app/input/render state, tests, and snapshots; and the
ASY-187 architecture/parity/testing/continuity/task/index records.

Commands and results: all focused and exact completion commands recorded above
pass. The first full workspace run exposed only an obsolete interactive schema
count after the two intentional tools were added; the assertion was updated to
name both schemas, its focused rerun passed, and the clean full rerun passed.

Remaining risks: live SSH/BSD execution was intentionally not performed. Remote
OS compatibility is covered by fixed argument/script and fake-transport tests
but remains the only manual environment-specific acceptance item.

Harness updates: `CONTINUITY.md`, `ARCHITECTURE.md`, `PARITY.md`, `TESTING.md`,
`NEXT_STEPS.md`, and `docs/.INDEX.md` now record the implementation, extension
boundary, validation, and completed plan location.
