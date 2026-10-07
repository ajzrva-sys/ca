# OBS-211 — Runtime Logs and Watchdogs

Status: COMPLETE (2026-08-28)
Owner/session: CUR-204 goal session
Last updated: 2026-08-28

## Goal

Provide private metadata-only rotating runtime diagnostics, a bounded `/logs`
viewer, and deterministic UI/process health watchdogs.

## Non-Goals

No prompt/body/tool/path/URL/environment/credential/provider-payload logging,
telemetry, or unsupported OOM claims.

## Evidence and Prior Behavior

CA had doctor/request/provider-trace surfaces but no consolidated safe runtime
log, log viewer, render watchdog, flicker detector, or corroborated memory exit
classification.

## Scope

Allowed: new diagnostics/log modules, runtime/main/TUI terminal/render code,
focused tests/snapshots/docs. Provider traces remain separate.

## Design and Invariants

Five profile-private mode-0600 files rotate at 1 MiB. Entries contain only
timestamp, level, component/event, safe counts/durations, run IDs, error class,
and process-local path hashes. `/logs` bounds and filters views. Fake-clock
watchdogs cover heartbeat stalls, slow/failed render, repaint/flicker rate,
restoration failure, memory thresholds, and corroborated high-memory unclean
exits. Hidden `/debug render` shows counters, dirty regions, frame time, and
non-persistent repaint highlighting.

Normal agent launches enable diagnostics by default. Bare profiles and local
administrative commands such as doctor, model listing, and tests leave runtime
state untouched. Provider traces remain a separate explicit facility.

## Implementation Record

- Added `ca.runtime-log.v1`, a typed metadata-only schema with no free-form
  content field, and a bounded background writer so render and event paths
  never perform log I/O.
- Added five-file private rotation, symlink/hardlink/owner/mode checks,
  process-local salted path hashing, a typed bounded viewer, and safe writer
  failure reporting.
- Added `ca.runtime-health.v1` clean/unclean lifecycle markers and conservative
  high-memory classification that requires persisted corroborating evidence.
- Routed every TUI draw through one measured wrapper and added heartbeat,
  slow/failed-render, repaint/flicker, restoration, and memory watchdogs.
- Added visible `/logs` and hidden `/debug render` controls with wide/narrow
  viewer and debug-overlay snapshots.

## Acceptance Criteria

Forbidden sentinel content never appears; rotation and modes are exact;
watchdogs are deterministic; uncorroborated exits are never classified as
memory-related; administrative no-state behavior remains unchanged.

## Validation

- Focused runtime-diagnostics, command, terminal-lifecycle, render, and
  snapshot tests: PASS.
- `cargo test --workspace --no-fail-fast -- --test-threads=1`: PASS. CA passed
  2,230 tests with four documented ignores; every workspace and integration
  target passed.
- `cargo fmt --all -- --check`: PASS.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: PASS.
- `cargo build --workspace --release --all-features`: PASS.
- Release help/version, `git diff --check`, zero `.snap.new`, exact protected
  status comparison, and protected-tree verifier: PASS.

The first full run exposed the expected command-count snapshot change and one
unchanged five-client broker file-creation race in the protected codex-team
subtree. The snapshot was reviewed and updated only from 97 to 98 commands;
the broker case passed alone and the complete serial matrix then passed.

## Progress

- [x] Failure-first safe-log tests.
- [x] Log store and viewer.
- [x] Watchdogs and debug render.
- [x] Focused and child gates.

## Decisions

Logs are safe operational metadata, not a second provider trace. Their writer
is asynchronous and bounded to preserve TUI responsiveness. Administrative
commands remain state-free even though ordinary agent launches log by default.

## Surprises and Discoveries

The TUI had 119 direct draw sites. Centralizing them behind one measured draw
boundary provided complete render accounting without changing individual
render call sites' behavior. The existing no-state CLI tests also made it
necessary to initialize diagnostics only for ordinary agent launches.

## Recovery and Rollback

Disposable profiles own logs. Reverse only OBS-211-labeled source/docs/snapshot
changes; do not alter provider traces, protected trees, or unrelated baseline
paths.

## Completion Record

Completed 2026-08-28. No runtime profile, installed binary, provider, oracle,
reference, or protected-tree path was changed.
