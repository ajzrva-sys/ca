# AUT-231 — Codex-Style Local Automations

Status: IN PROGRESS (resumed 2026-10-05 as part of the user-authorized oracle feature implementation)
Owner/session: current Codex collaboration; source implementation and temporary-fixture validation
Last updated: 2026-10-06 01:34 UTC

## Goal

Add safe local Codex-style automation UX over CA's existing durable scheduler:
conversational exact-preview creation, saved-session heartbeats, standalone
per-occurrence tasks, exact reasoning routes, rolling budgets, local
notifications, and a complete `/automation` management cockpit.

## Non-Goals

- Hosted execution, subscriptions, telemetry, remote marketplaces, or cloud
  delivery.
- RRULE, Responses WebSocket, automatic commit/push/merge, background
  approval, privileged tools, or arbitrary workflow runtimes.
- Migration, reinterpretation, or byte changes to Schedule V1/V2 definitions.
- Real provider traffic or real LaunchAgent installation during validation.

## Initial evidence (retained pre-implementation baseline)

- `src/schedule.rs` exposes profile-scoped list/add/update/pause/resume/remove,
  run/tick/history/doctor/service/daemon commands and a private worker protocol.
- `src/schedule_v2.rs` owns strict digest-bound V2 definitions, deterministic
  absolute/delay/interval/five-field cron triggers, IANA time zones, missed-run
  policy, immutable run journals, leases, pinned workspaces, recovery artifacts,
  and exact-confirmed LaunchAgent lifecycle.
- `src/main.rs::run_schedule_worker` currently runs isolated no-session
  headless prompts and resolves the route's default reasoning effort; it cannot
  append to saved sessions or create a resumable result task.
- `src/session.rs` has generation/message-count/history-digest integrity but no
  owner-bound cross-process turn/write lease or automation delivery journal.
- Existing `/sessions`, manager/cockpit, split-pane, notification, usage,
  pricing, approval, and exact-preview patterns provide the native UI and
  authority seams required here.
- Public OpenAI material is use-case evidence only; it does not define a
  complete desktop scheduler wire protocol. This plan makes no runtime or wire
  parity claim.

Historical dirty-tree baseline before AUT-231 edits (Git metadata is absent in
the current checkout; ORC-258 uses content manifests and source snapshots):

- HEAD: `25c936f37e545cb0b4ccfdbbcb397d216a497e83`
- `git status --porcelain=v2 -z` SHA-256:
  `6e90af2cbece9cbf25df5faca2ed7a8418acd888c4027a4936bd473b3c68b2b9`
- 893 status records, 565 tracked diff paths, and 343 untracked paths.
- Exact gzip/base64 status bytes:
  `docs/exec-plans/active/AUT-231-dirty-baseline.status.gz.b64`.

Active-plan overlaps:

- `SEC-158`: scheduled/headless/service surfaces remain closed to privileged
  execution and secret approval channels.
- `LRN-150` / `LRN-166`: preserve their separate learning state and
  LaunchAgent/supervisor ownership; reuse only the existing scheduler service.
- `REL-156`: preserve reproducible-release work and run its relevant gates.
- `SCH-099`: V1/V2 schedule bytes, journals, leases, recovery, and service
  confirmation remain stable.

## Scope

Allowed files: new automation modules and focused integration in scheduler,
session, CLI/headless, tools, TUI, runtime/config/pricing/usage, tests,
snapshots, and root harness documents.

Forbidden: `oracles/`, `reference/`, unrelated active-plan hunks, real provider
requests, real LaunchAgent changes, and user automation/session/config state
during tests. The installed binary changes only after every completion gate.

## Design and Invariants

- Persist additive `ca.automation.v1`, `ca.automation-run.v1`, and
  `ca.automation-session-delivery.v1` state separately from Schedule V1/V2.
- Use the existing trigger dialect and a deterministic cross-store due-order;
  enforce a combined profile cap of 256 schedules and automations.
- A model may list safe metadata and prepare a proposal. Only a host
  exact-confirmed action may create, enable, update, or delete automation.
- Heartbeats acquire a stable OS advisory session writer lease, load the latest
  committed generation, run one continuation, durably record the result, and
  compare-and-append once. Recovery settles recorded delivery without provider
  replay. Busy occurrences coalesce to one pending heartbeat.
- Standalone occurrences create a deterministic per-run saved task carrying safe automation
  provenance. Mutating runs retain the existing isolated-pinned/recovery proof
  boundary; no automatic transfer or merge occurs.
- Routes store exact provider/model/reasoning and price-card identity. No
  fallback or later default inheritance is allowed.
- Scheduled workers inherit no transient approvals, Yolo, AutoReview, plugin
  directories, privileged tools, MCP, hooks, or ambient shell state.
- New provider automations default to per-run 16 requests / 500k input / 64k
  output / 64 tool calls / 15 minutes / 2 MiB / USD 2 and rolling-30-day 256
  requests / 10M input / 1M output / 1,024 tool calls / USD 25.
- Remote calls reserve conservative worst-case exact-priced exposure before
  dispatch and refund only authoritative usage. Unknown-priced remote routes
  fail closed; recognized local routes retain non-monetary limits at zero cost.
- Every run enters private local history. Heartbeat structured reports keep
  unchanged checks and repeated stable states quiet by default; changed,
  completed, failed and attention results follow the saved notification policy.
  The decision is frozen with the result before delivery, and restart never
  recomputes it. Explicit AllResults, FailuresOnly and Muted remain available.
  Native metadata-only alerts and focus suppression are separate presentation
  boundaries; a quiet result does not create transcript messages.
- Archived heartbeat sessions continue; deleted/replaced/drifted targets pause.

## Milestones

### M1 — Contracts, stores, and failing characterization

- Freeze V1/V2 byte fixtures and add failure-first tests for strict automation
  schemas, atomic private stores, combined capacity, preview/apply drift, and
  deterministic cross-store due ordering.
- Add owner-only automation definition, run journal, delivery journal, budget
  ledger, exact digest, validation, and fake-clock primitives.

### M2 — Execution, session delivery, and budgets

- Add exact reasoning and budget fields to the internal worker envelope without
  changing public schedule definitions.
- Add session turn/write leases, CAS delivery, automation message provenance,
  heartbeat coalescing/recovery, and standalone task creation.
- Enforce route/pricing/context/authority drift, isolated mutation, request/tool
  limits, worst-case reservations, rolling accounting, refunds, and no replay.

### M3 — CLI, model tools, service, and notifications

- Add `ca automation` list/inspect/history/doctor/preview/create/update/pause/
  resume/run/delete/service commands with stable text/JSON output.
- Add interactive safe metadata listing and draft-only `propose_automation`
  previews; host exact confirmation owns every persisted mutation. The proposal
  path replaces the original `preview_local_automation_change` tool name.
- Reuse the scheduler LaunchAgent with a separate exact-confirmed network upper
  bound. Add safe native macOS notifications and focus-lease suppression.

### M4 — TUI cockpit and result navigation

- Add `/automation` search/list/detail/history, typed preview, confirmation,
  budget, service, notification, result-session, and heartbeat-session views.
- Preserve pane/session/generation event identity and add reviewed wide/narrow
  snapshots plus split-pane/background-event regressions.

### M5 — Reconciliation, release, and installation

- Run focused suites and both serial workspace matrices, formatting, strict
  all-feature Clippy, release, command/schema smokes, signature, diff/snapshot,
  protected-tree, orphan-process, and dirty-baseline audits.
- Reconcile architecture, continuity, parity, testing, index, and next steps;
  move this plan to completed only after all gates pass.
- Preserve the installed binary as a timestamped full-SHA recovery copy,
  atomically install the verified candidate, and restore on any integrity
  failure. Do not install the LaunchAgent.

## Acceptance Criteria

- Existing V1/V2 serialized fixtures and behavior remain unchanged.
- Exact-confirmed new automations execute each deterministic occurrence at most
  once, survive crashes, and cannot exceed or widen stored authority.
- Heartbeats and interactive turns cannot race session history; recorded output
  is appended exactly once or remains visibly pending without replay.
- Standalone runs create resumable, correctly attributed saved tasks.
- Reasoning, pricing, cost reservations, usage refunds, rolling limits, and
  blocked states are truthful and deterministic.
- `/automation`, CLI JSON, inbox/history, service preview, notifications, and
  result navigation pass focused and snapshot coverage.
- All completion gates and installed-binary integrity checks pass.

## Validation

Focused: automation stores/coordinator, scheduler compatibility, session lease
and delivery crash matrix, fake provider/pricing/budget cases, safe
notifications, CLI, tools, and TUI render/input suites.

Completion:

- `cargo test --workspace --no-fail-fast -- --test-threads=1`
- `cargo test --workspace --all-targets --all-features --no-fail-fast -- --test-threads=1`
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo build --workspace --release --all-features`
- release help/version/new-command smokes, strict signature, `git diff --check`,
  zero `.snap.new`, protected-tree verifier, orphan checks, and exact baseline
  comparison.

Manual/visual: review wide/narrow automation manager, run detail, preview,
service, notification, and split-pane snapshots.

Blocked checks: real provider requests, real OS notifications, real LaunchAgent
installation, and user profile/session mutation are intentionally forbidden.

## Progress

- [x] Goal created and initial dirty-tree/overlap baseline recorded.
- [x] M1 contracts/stores drafted in `src/automation.rs`; the user-authorized
  BUG-234 gate cleanup formatted the paused files, connected the existing
  due-claim path to schedule tick/daemon execution, removed the redundant
  public successful-tool count, and added a no-state read-only regression.
  This is the historical draft checkpoint; the resumed implementation below
  now completes the bounded store/claim/preview/receipt contracts.
- [x] M2 shared session/run advisory ownership, exact committed metadata/history
  CAS, one-pending busy heartbeat, immutable claim/binding/result checkpoint,
  deterministic standalone result sessions and restart settlement without
  provider replay implemented. Frozen worker dispatch usage is authoritative
  for accounting; budget dispatch gates are implemented by the companion
  scheduler work. Completion depends on integrated gates below.
- [x] M3 CLI/proposal/service surfaces and metadata-only native alerts are
  implemented. Durable admission, verified focus suppression and fake transport
  cases passed 3/3; no real OS alerts were sent. Reviewed deletion permits
  idle/dead-owned removal and refuses live owners, retaining immutable receipts.
  Current applicable integrated tests and fake-alert coverage pass; release
  walkthrough and installation remain tracked below.
- [x] M4 cockpit, frozen preview/exact confirmation, private quiet-result
  inspector and saved-result navigation are implemented by companion UI work.
  Focused input/render and actual debug interaction gates passed. The fresh signed-release `/automation` walkthrough passes.
- [ ] M5 gates, reconciliation, release, and installation complete.

Current resumed validation (2026-10-06 01:34 UTC): all AUT-231 source contracts
are implemented; the fresh signed `/automation` walkthrough and both applicable
workspace matrices pass. Default 3,558, all-feature 3,707, with the same
nine documented baseline exclusions. Strict Clippy, formatting and release pass.
Recorded results settle without provider replay, heartbeat writers use shared
ownership and physical dispatch budgets preserve exact reasoning/pricing.
Fixtures use fake providers/clocks and no real notifications or service changes.
The shared actual eight-hour and real-model release gates remain open; no
installed binary has been replaced.

## Decisions

- Validation receipt (2026-10-05): shared compile 95668 and exact test-binary
  SHA-256 are recorded in `ORC-258-recovery-automation-evidence.md`.
  `automation::tests` passed 23/23 including safe budget rotation, immutable
  receipt/crash/cap cases, deletion and human-edit acknowledgement; native fake
  alert/focus cases passed 3/3. Full workspace/UI/release gates remain separate.

- Decision (2026-10-05): replace PID/create_new ownership with stable advisory
  files plus in-process capabilities. Never unlink participating lock files.
  Shared session ownership is also used by interactive turns and dashboard
  controls, so heartbeats hold one capability from load through delivery.
- Decision (2026-10-05): checkpoint exact active claim, target binding,
  provenance, result digest, settled budget and notification decision before
  delivery. Interrupted execution without a checkpoint pauses for review;
  restart can only settle an existing receipt or publish a recorded result.
- Decision (2026-10-05): structured heartbeat status is explicit. Malformed or
  absent provider status is attention-worthy; authoritative host execution
  failures remain failures. Stable state fingerprints suppress repeated
  changes, while explicit unchanged reports remain in private history.

- Decision: keep CA trigger formats instead of adding RRULE.
  Evidence: existing V2 already covers requested recurrence and DST behavior.
  Consequence: one recurrence engine and no migration.
- Decision: execute heartbeats while the TUI is closed and create one saved
  task per standalone occurrence.
  Consequence: session CAS/lease and durable delivery are mandatory.
- Decision: use rolling 30-day aggregate budgets and worst-case reservation.
  Consequence: unknown-priced remote routes cannot run unattended.
- Decision: creation activates immediately after exact confirmation, while
  scheduler-service installation remains separately confirmed.
  Consequence: definitions may be active but visibly blocked until service
  readiness and authority are satisfied.

## Surprises and Discoveries

- BUG-234's release gates exposed the two frozen M1 dead-code warnings and
  formatter drift. At the user's explicit request, those blockers were fixed
  without activating a service or advancing the remaining AUT milestones.

## Recovery and Rollback

Reverse only AUT-231-owned hunks using `apply_patch`; never reset or clean the
dirty repository. New private formats are additive and may be ignored by older
binaries. Installation keeps a verified recovery binary and restores it on any
failed post-install check.

## Historical paused state and current resumption

Frozen 2026-08-29: the driving agent was shut down before finishing M1.
Resume bundle (file copy + wiring map + steps):
`docs/exec-plans/active/AUT-231-resume/`.

Updated checkpoint 2026-09-02: automation tests pass 8/8, schedule tests pass
11/11, both serial workspace matrices pass, repository formatting and strict
all-target/all-feature Clippy pass, and the all-feature release builds cleanly.
No real automation state, provider, or LaunchAgent was used. AUT-231 remains
paused because M1 characterization and M2–M5 product work are still incomplete.

Resumed checkpoint 2026-10-05: shared ownership/session CAS and five crash/busy
automation cases passed in the focused binary suite (13 tests selected by
`automation::tests`, including companion local-automation tests). New
notification-decision cases and integrated dispatch/cockpit gates are under
active validation; this historical receipt does not claim the current full
workspace matrices, native alerts, release or installation have passed. Tests
use temporary profiles only; no real provider, user automation state or
LaunchAgent installation was used.

## Historical partial completion record

Changed files: `src/automation.rs` (new), `src/main.rs`, `src/schedule.rs`,
`src/session.rs`, `src/app.rs` (app.rs edit unverified — see resume bundle).
Commands and results: original `cargo check --bin ca` exit 0; the later narrow
gate-cleanup and integrated results are recorded in `TESTING.md` under BUG-234.
Remaining risks at the historical pause: M1 was incomplete; M2–M5 untouched.
Harness updates: plan PAUSED; resume bundle created; NEXT_STEPS.md updated.

## Final audit repair evidence — provisional

The failure-first receipt `/private/tmp/ca-orc258-final-audit-negative.log`
observed workflow reasoning `null` versus the reviewed `high`, and two dispatches
versus one for a one-request budget with one configured provider-error retry.
The regression filters are:

- `schedule::tests::automation_workflow_preserves_frozen_reasoning_in_worker_payload_and_legacy_omits_it`
- `schedule::tests::automation_workflow_unknown_failed_usage_stops_before_retry_and_legacy_can_retry`

The repair adds `WorkflowAutomationContext`, forwards its frozen effort in
`prepare_workflow_worker_spec` (`src/schedule.rs:2530`), and uses
`record_workflow_worker_failure` (`src/schedule.rs:2552`) to stop automation
replay without trustworthy usage/side-effect evidence. Unknown usage retains the
existing conservative reservation. Legacy scheduled workflows keep their omitted
reasoning field and retry policy. Exact source/fixture hashes are in
`/private/tmp/ca-orc258-workflow-audit-source.json`; passing repair validation
is not yet claimed.

Root is checking the companion frozen-absence and unsupported-after-reload
regressions,
`tests::automation_headless_reasoning_keeps_frozen_absence_after_default_drift`
and
`tests::automation_headless_reasoning_refuses_unsupported_frozen_effort_after_reload`.
Prior artifacts and gates remain historical/provisional for this repaired source.
The first fresh soak failed after 60.459 seconds at `/bin/ps`, so the actual
eight-hour gate remains pending. Real-model quality and recoverable signed
installation also remain pending; this note does not complete AUT-231 or claim
an installation.
