# SCH-099 — Durable Automation and LaunchAgent Support

Status: COMPLETE
Owner/session: Codex `/root`
Last updated: 2026-08-13

## Goal

Preserve schedule V1 bytes and behavior while adding strict V2 definitions/time semantics, durable run receipts and leases, isolated script execution, offline recovery diagnostics, and explicit secret-free per-profile LaunchAgent lifecycle.

## Non-Goals

- Rewriting V1 definitions or `schedules.json` merely because they were read.
- Ambient skills, MCP, hooks, sessions, memory, or schedule-creation tools in workers.
- Real provider/webhook traffic, real LaunchAgent mutation, installed-binary mutation, source-checkout writes, or automatic ambiguous replay.
- Activating `EmailDraft` before EML-101.

## Evidence and Current Behavior

- `src/schedule.rs` owns public V1 definition/trigger/target/delivery, private V1 records/store, path/PID lock, worker, workflow runner, delivery, and CLI. Store and definition versions are exactly 1.
- V1 store save currently uses a wrapping revision increment and tick executes while its short database lock remains in scope.
- V1 scheduled capabilities already disable skills, MCP, LSP, hooks, persistence, workflows, worktrees, agents, and multi-run through `RunCapabilities::scheduled`.
- `workspace_materialization::materialize_scheduled_workspace` already accepts a strict tracked allowlist and produces a disposable workspace/receipt boundary.
- Hermes documents absolute/delay/interval/cron scheduling, fresh isolated sessions, recursion denial, script-only no-model jobs, local delivery suppression for empty output, and atomic job storage. ca follows the stricter frozen journal, lease, WSP, and confirmation contract.

## Scope

Allowed files: `src/schedule.rs`, new `src/schedule_v2.rs`, `src/runtime.rs`, `src/main.rs`, `src/workspace_materialization.rs` only if an existing public seam is insufficient, Cargo dependency files, schedule/CLI fixtures/tests, `ARCHITECTURE.md`, `CONTINUITY.md`, `ENVIRONMENT.md`, `NEXT_STEPS.md`, `TESTING.md`, `docs/.INDEX.md`, and this plan.

Touched-file manifest: this plan, `NEXT_STEPS.md`, `Cargo.toml`, `Cargo.lock`, `src/main.rs`, `src/runtime.rs`, `src/schedule.rs`, `src/schedule_v2.rs`, `src/workspace_materialization.rs`, `ARCHITECTURE.md`, `CONTINUITY.md`, `ENVIRONMENT.md`, `TESTING.md`, and `docs/.INDEX.md`. Add each later path before its first edit.

Forbidden: `oracles/`, `reference/`, real provider/webhook/launchctl operations, user runtime state, source-checkout mutation, broad Git cleanup/reset, SKL-098 behavior except exact pin consumption.

Runtime state affected: only explicit schedule V2 confirmation, tick/run execution, and explicitly confirmed service lifecycle under injected/profile paths. Automated validation uses disposable homes, repositories, fake executables, and fake launchctl.

## Design and Invariants

- V1 decoders and public serialization stay byte-compatible; V2 definitions live separately from `schedules.json` and never cause read-time rewrite.
- Every V2 run has a stable ID and append-only hash-chained owner-only receipt events. Legal transitions and occurrence identity fail closed.
- Claims are durable before execution; short definition locks are not held across workers. Settlement uses exact definition digest/run identity and checked revisions.
- Leases are owner-only, no-follow, root/profile/run/workspace bound, capacity two globally and one per workspace, and never recover corrupt or ambiguous ownership.
- Script-only execution uses an absolute digest-pinned executable and exact argv with no shell, provider construction, inherited provider secrets, or automatic retry.
- Write-capable work requires an isolated pinned Git revision and WSP materialization. Existing read-only mode is accepted only for provably read-only tools and enforced sandbox availability.
- LaunchAgent preview is deterministic and secret-free. Install/uninstall requires the exact preview digest and exact label/path and uses only a fake `launchctl` in automated validation.

## Milestones

### M1 — V1 freeze and V2 model/time engine
- Add V1 golden characterization and checked revision increment.
- Add strict V2 public types, digest confirmation, absolute/delay/interval/five-field IANA cron with DST and missed-run tests.

### M2 — Journal and leases
- Add owner-only append-only hash-chained receipts, legal transitions, history/doctor, failpoints, fair claims, capacities, and conservative dead-owner recovery.

### M3 — Execution and WSP
- Add exact script-only worker and no-provider test.
- Add pinned Git tracked allowlist, detached-source validation, WSP materialization, source immutability, exact skill pins, and artifact metadata/recovery boundaries.

### M4 — Manager and service
- Add update/pause/resume/run/history/doctor/tick-drain/daemon commands.
- Add deterministic plist preview plus explicit fake-launchctl install/status/uninstall and signal shutdown.

### M5 — Completion
- Run focused and full Rust gates, released disposable smokes, reconcile docs, and archive.

## Acceptance Criteria

- V1 definition/store/list/result/worker goldens remain unchanged.
- V2 time engine is deterministic across UTC, DST gaps/folds, skip, and one catch-up.
- Journal/lease corruption, illegal transitions, PID ambiguity, over-capacity, and replay attempts fail closed.
- Script-only makes no provider request, uses exact argv/clean environment, bounds output/time, and suppresses empty delivery.
- Write-capable jobs cannot execute in the source checkout; pinned tracked materialization and exact skills are required.
- History/doctor are offline; service preview is deterministic/secret-free and `plutil -lint` valid; mutation requires exact confirmation.
- Full tests, format, both strict Clippy modes, both release builds, help/version, and snapshot hygiene pass.

## Validation

Focused: `cargo test schedule -- --nocapture` passed V1 goldens plus V2 time, journal, lease, Git/WSP, recovery, script-only, and fake-launchctl tests; pinned Git and interval policy cases also passed independently.
Completion: `cargo test --no-fail-fast` passed 1,195 unit tests with one accepted manual browser ignore and 36 CLI tests. `cargo fmt --check`, default/all-feature strict Clippy, the default release build, and the feature experiment release build passed.
Manual/visual: released `--help`, `--version`, and `schedule --help` exited zero from a disposable HOME; released service preview piped directly to `plutil -lint -` and passed. Snapshot hygiene found zero `.snap.new` files.
Blocked checks: real provider/webhook/LaunchAgent mutation intentionally prohibited and unnecessary.

## Progress

- [x] Frozen contract, V1 source, WSP seam, and primary Hermes references reviewed.
- [x] Active plan and initial touched-file manifest created.
- [x] M1 V1 freeze and V2 time engine.
- [x] M2 journal and leases.
- [x] M3 execution and WSP.
- [x] M4 manager and LaunchAgent.
- [x] M5 completion gates/docs/archive.

## Decisions

- Decision: implement V2 in a separate module/store and keep V1 decoding/summary semantics intact.
  Evidence: the handoff forbids adding in-flight fields to `schedules.json` and calling it compatible.
  Consequence: V1 files remain readable without rewrite and V2 failure recovery is independently inspectable.
- Decision: use exact local confirmation for V2 definitions and service plist bytes.
  Evidence: automation creation and host service mutation both require explicit digest confirmation.
  Consequence: previews are mutation-free and stale sources cannot be confirmed accidentally.

## Surprises and Discoveries

- V1 `ScheduleStore::save` used wrapping arithmetic and `tick_once` retained its lock across execution; both were corrected without schema changes.
- A recovery artifact hash alone was insufficient for SCH-099. WSP gained one narrow read-only manifest-inspection seam so the complete archive could be reconstructed independently before cleanup.
- V2 outputs/artifacts must be siblings of `schedule-runs-v2/`; placing them inside the journal root would make offline history correctly reject them as corrupt journal entries.

## Recovery and Rollback

Reverse only manifest-listed changes with `apply_patch`. Do not reset/clean the repository. V2 tests use create-new disposable state. A failed run retains journal/workspace evidence; cleanup is explicit only after verification.

## Completion Record

Changed files: `Cargo.toml`, `Cargo.lock`, `src/main.rs`, `src/runtime.rs`, `src/schedule.rs`, new `src/schedule_v2.rs`, narrow `src/workspace_materialization.rs`, the six project knowledge/status documents, and this archived plan. Pre-existing SKL-098 files remain part of the dirty worktree and were not reset or broadly cleaned.
Commands and results: focused schedule tests passed; full tests passed 1,195 + 36; formatting and both strict Clippy modes passed; default and feature release builds passed; released local help/version/plist and snapshot checks passed.
Remaining risks: real LaunchAgent, provider, and webhook operations were intentionally not exercised. Dead/live/PID-reused ambiguity fails closed; interrupted mutating work is never replayed and retained evidence can require manual inspection.
Harness updates: SCH-099 is complete in `NEXT_STEPS.md`, architecture/environment/continuity/testing/index records are current, and EML-100 is next. RND-097 remains closed with zero requests and zero spend.
