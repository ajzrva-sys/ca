# ORC-258 — Recovery and automation evidence

Status: ACTIVE — source and integrated/signed TUI gates pass; actual duration, selected model and install remain open
Updated: 2026-10-06 01:34 UTC
Companion plans: `ORC-258-oracle-informed-improvements.md` and
`AUT-231-codex-style-local-automations.md`.

## Current integrated status

All ten authorized contracts have source implementations, grouped into seven
review milestones. Final applicable default tests passed 3,558 cases
(42 ignored); all-feature tests passed 3,707 (43 ignored).
Both runs exclude exactly nine individually reproduced pre-existing codex-team
failures. Exclusions are not passes, and the unfiltered workspace remains failing.
See `docs/exec-plans/active/ORC-258-baseline-test-exceptions.md`.
Both full matrices ran at source `8d452a762e0e95a793e436e0b641476372a70747dcce76b0cfb052afb7f645b1`.
The final candidate changes only `len % 2 == 0` to `len.is_multiple_of(2)`
in benchmark median reporting. Exact reverse substitution recovers the tested
file hash; all other 1,016 inputs match. All 28 affected memory/CLI tests,
formatting, strict Clippy and release run after that equivalent lint change.
Formatting, strict workspace/all-target/all-feature Clippy and the all-feature
workspace release build pass. Code/build/test inventory: 1,017 files,
SHA-256 `a47efd5519ff39d8f0b30a8045fb2d995d8c730170e75f0a0b9b444cbc94b18c`.

Fresh signed candidate: `/Volumes/projects/ca/ca/scratch/oracle-improvements/final-validation/release-20261006T013149Z/ca`,
SHA-256 `fcae06c3a84fe284a18ed7bcfec6316dc4d6ae121f630b349bc35a3ca525db58`. Signature/version/help, exact render-buffer parity, all nine
actual TUI paths and short 4,096-message responsiveness checks pass. Short
keyboard p95: 19.785–20.584 ms. Read-only installer readiness is
separately recorded; no installed executable has been replaced.

The candidate's actual 28,800-second soak is running, not passed. Its first
minute process probe succeeded; the durable receipt is
`/Volumes/projects/ca/ca/scratch/oracle-improvements/final-validation/release-20261006T013149Z/eight-hour-soak.json`. Harness PID 93807, child PID
93811; earliest possible completion after actual idle is about
2026-10-06T05:32:11.490286-04:00 (America/New_York). Inspect final
status, actual elapsed >=28,800, post-idle p95 <100 ms, input/theme/resize and
terminal restoration before claiming a pass. The older 60.459-second denied
process-probe failure is retained as superseded evidence.

The additional Enter repair retires only a cancelled turn's owned response hook
before invalidating its generation. Actual fixed-debug and fresh signed hook
walkthroughs verify one cancellation followed by exactly one next submission,
physical cleanup and preserved draft; owner isolation/denial regressions pass.
Other bounded synthetic active reply/tool/child, 32-cycle and 4,096-message stream
probes passed sampled installed/current artifacts. The user's broader
intermittent installed-session failure has not been reproduced or fully closed.

Real-model hybrid quality and warm <=200 ms latency remain unmeasured pending
the user's explicit existing runtime/model selection. Synthetic fixture quality
and ranking timings cannot establish that gate. No model download, real service
installation, production provider request or installed binary replacement was
performed. Signed recoverable installation remains authorized after all gates
pass; resolve destination/candidate anew and leave running sessions alone.

Exact final build evidence: `/Volumes/projects/ca/ca/scratch/oracle-improvements/final-validation/build-20261006T013145Z`.
Audit failure-first and positive receipts remain separately preserved. The
timeout fixture repair changes only tests and passes 32 focused cases plus both
final matrices. Cold session proof fixtures pass 91 cases in 6.11 seconds.
The code inventory includes six formerly omitted build inputs whose hashes
match the initial immutable baseline; no source changed in that extension.

## Implemented boundaries

| Boundary | Source | Contract |
| --- | --- | --- |
| Saved-session ownership | `src/session_access.rs:73`, `src/session.rs::SessionStore::save_owned`, `SessionStore::unique_identity_location` | Canonical profile storage + UUID, advisory writer lease and in-process non-reentrancy. Participating lock files never unlinked. Loaded UUID/history generation, digest and metadata revision must match before save. Matching malformed metadata or duplicate locations across active/archive refuse writes, including identical interrupted-move hard links. Read-only family navigation retains duplicate warnings. |
| Normal typed edits | `src/recovery.rs::RecoveryStore::prepare`, `RecoveryContext::apply_admitted`, `src/undo.rs` | Private blobs and intent synced before mutation when capture succeeds. Capture/retention failure warns and continues an otherwise valid edit. Existing file and batch bounds apply. |
| Native undo/redo | `src/recovery.rs::RecoveryStore::apply_history`, `RecoveryStore::load_undo_stack_locked` | Refresh durable stack, validate stable batch identity, acquire workspace exclusion, sync rescue intent, apply and commit history transition. Crash derivation settles applied unrecorded transitions once. |
| Recorded turn restore | `src/recovery.rs::RecoveryStore::restore_with_outcome` | Exact reviewed revision/token and current file states, mandatory durable rescue intent. Persisted history-reset marker prevents old native history resurfacing after a crash. Rescue turn provides redo. |
| Workspace admission | `src/recovery.rs::RecoveryStore::try_workspace_activity` | Private per-user temporary registry covers all profiles, HOME roots and Bare. Shared active work conflicts with exclusive restoration for overlapping canonical roots; unrelated siblings proceed. |
| Blocking mutation drain | `src/recovery.rs::RecoveryContext::admit_mutation` | Parent/child admission acquired before blocking-worker scheduling. Cancellation of the async waiter cannot release its running worker's guard. Retirement closes admissions and drains guards before sealing; open idle waits preserve live-child continuation. |
| Family/bookmarks | `src/branches.rs:51`, `src/session.rs` | Bundle metadata includes archived parents/siblings. Bounded pagination reports cycles, duplicates and missing parents. Bookmark anchors bind complete boundaries and exact prefixes; handoffs are separate inert provenance. |
| Automation delivery | `src/automation.rs:780`, `src/automation.rs:1434`, `src/automation.rs:1582` | Stable run ownership, exact immutable claim/binding/provenance, conservative reservation, private result + frozen decision before delivery, deterministic standalone UUID, heartbeat lease through commit. Restart settles recorded results without provider replay. |
| Native alert | `src/automation_notifications.rs:169`, `src/automation_notifications.rs:200` | Metadata-only name/state/navigation; verified PID/start/profile/session focus suppresses presentation. Durable admission precedes at-most-one best-effort transport attempt. Unverifiable live focus fails quiet. |
| Reviewed deletion | `src/automation.rs::automation_record_digest`, `AutomationStore::remove_confirmed_snapshot` | Full definition + run-state snapshot checked under mutation lease. Live run Busy; idle/dead-owned removal retains all immutable run/budget receipts. |
| Rolling budget rotation | `src/automation.rs::AutomationBudgetLedger` | Exact bounded per-run reserve/refund receipt is synced before ledger evidence is dropped. Only settled pairs older than 30 days expire; unresolved reservations remain charged. Admission reserves storage for future refunds. Historical checkpoint inspection uses its immutable receipt; legacy ledger proof remains supported. |

## Recovery crash/fault matrix

These simulate process interruption at durable boundaries, not power loss or
fault injection for every filesystem system call. Tests below are in
`src/recovery.rs::tests`.

| Boundary | Expected behavior | Regression |
| --- | --- | --- |
| Storage/quota unavailable before ordinary edit | Continue with coverage warning; retain protected active captures. | `capture_storage_failure_warns_and_continues_exact_edit`, `active_capture_is_preserved_when_quota_is_full` |
| Intent synced, files unchanged | Observation marks Aborted; no startup writes. | `interrupted_intents_reconcile_by_observation_without_writes` |
| All files committed, completion/retention absent | Mark Applied and derive receipt once; seal inactive group with incomplete coverage. | `startup_seals_applied_turn_and_derives_receipt_after_crash_before_retention` |
| Mixed batch commit or external drift | Visible Conflict, no auto restoration or invented undo receipt. | `interrupted_partial_batch_remains_conflicted_without_restoring_or_deriving_undo`, `external_drift_and_stale_preview_never_overwrite_files` |
| Native inverse applied, stack commit absent | Reconstruct stable undo/redo transition once. | `interrupted_native_undo_reconstructs_transition_once_before_stack_commit` |
| Older parent group's child completes after a newer turn | Unseen receipts/transitions follow monotonic durable completion revisions across groups, with deterministic capture/legacy fallback. | `interleaved_parent_and_child_turns_reconstruct_undo_by_monotonic_completion` |
| Turn restore applied, stack reset absent | Applied history-reset intent clears old native stack. | `interrupted_turn_restore_resets_native_history_before_stack_reset_commit` |
| Storage unavailable before explicit restore | Refuse before file changes. | `restoration_refuses_storage_failure_and_active_workspace_ownership` |
| Live overlapping workspace, including another process/profile | Restore refuses; shared activity admitted; process death releases ownership. | `workspace_activity_shares_profiles_and_handles_overlapping_roots_without_blocking_siblings`, `workspace_shared_and_exclusive_ownership_is_cross_process` |
| Cancelled async waiter, admitted child worker still paused before capture | Turn cannot seal; closed admission refuses new work, while already admitted worker captures/applies/retains before drain completes. | `aborted_tool_waiter_still_drains_admitted_child_blocking_mutation_before_sealing`, `mutation_idle_wait_preserves_child_admission_and_rejects_foreign_turn_guard` |
| Stale UI stack or preview | CAS/token/preflight refuses without replacing fresh receipts/files. | `stale_ui_stack_cannot_overwrite_automatic_durable_receipts`, `external_drift_and_stale_preview_never_overwrite_files` |
| Same-turn create then delete | Missing → Missing restores as exact no-op after safe-parent validation. | `create_then_delete_turn_restores_the_exact_net_zero_missing_state` |
| Tampered blob, replaced root, symlink parent/storage | Read/restore refuses; cleanup never follows symlinks. | `blobs_are_private_validated_and_not_embedded_in_journal`, `root_replacement_and_symlink_parent_reject_capture_or_restore`, `private_storage_symlinks_do_not_redirect_cleanup` |

After successful file changes, completion/retention faults return success plus a
warning. Startup preparation also surfaces bounded reconciliation errors,
conflicting/incomplete-group counts and unreadable undo history in `src/main.rs`.

## Automation crash/fault matrix

Tests below are in `src/automation.rs::tests` unless explicitly noted.

| Boundary | Expected behavior | Regression |
| --- | --- | --- |
| Live claim owner | Recovery skips; deletion Busy. | `busy_heartbeat_coalesces_pending_occurrences_and_live_run_owner_blocks_recovery`, `confirmed_deletion_retains_recorded_receipts_and_refuses_a_live_run` |
| Reservation, result or settled accounting persisted before checkpoint | Preserve private bytes/accounting, pause ambiguous claim without replay/refund. | `interrupted_reservation_or_uncheckpointed_result_never_replays_or_refunds` exercises all three boundaries. |
| Checkpoint present, session commit absent/present | Publish frozen result once or acknowledge exact existing provenance, then settle. | `restart_delivers_standalone_result_once_before_or_after_session_commit`, archived heartbeat crash coverage |
| Existing result later human edited | Acknowledge committed provenance, preserve human edits. | `delivered_standalone_provenance_acknowledges_later_human_edits` |
| Target drift or result/checkpoint tamper | Preserve exact pending claim/private result, visibly block delivery. | `delivery_tamper_or_target_drift_preserves_result_and_exact_pending_claim` |
| Frozen quiet decision, later baseline change | Use frozen decision; unchanged/repeated states have no transcript delivery. | Quiet-decision and terminal baseline acknowledgement cases. |
| Missing/malformed report or host failure | Missing status requires attention; host failure remains authoritative. | Structured heartbeat policy and host-failure cases. |
| Alert admission present, transport outcome ambiguous | Never repeat OS attempt; committed provider/child/session outcome unchanged. | `automation::notifications::tests::durable_alert_admission_suppresses_focus_and_never_replays_ambiguous_delivery` |
| Alert admission cannot sync | No transport call; result unaffected. | `automation::notifications::tests::failed_durable_alert_admission_never_calls_transport` |
| Reviewed deletion races claim/state drift | Reject old token; current review deletes idle/dead-owned definition only. | `confirmed_deletion_refuses_reviewed_definition_or_claim_drift` and companion UI confirmation case. |
| Receipt synced, ledger rebase absent | Retry preserves the exact immutable receipt and old result remains readable. | `expired_budget_rotation_archives_legacy_evidence_before_rebase_and_keeps_results_readable` |
| Old unresolved reserve and late refund | Preserve full outstanding charge; eligibility follows reservation time and cannot underflow an isolated refund window. | `old_unsettled_budget_remains_charged_and_late_refund_never_underflows_window` |
| Receipt archive failure or live-window capacity | Keep ledger bytes/evidence, refuse new admission; never discard unresolved charges. | `budget_rotation_refuses_receipt_failure_without_discarding_legacy_accounting`, `high_frequency_live_budget_evidence_reserves_future_refunds_and_fails_closed_at_cap` |

## Actual validation receipts

Historical focused receipt: shared compile 95668 succeeded in 1m22s. It produced
`target/debug/deps/ca-78208114319849d8` with SHA-256
`2b03f8c0c0e600fed3b59e18302f5b16cf4358db0e0b20c93eb9c02dc734465e`.
The linker reported its known unwind-size warning; there were no Rust source
diagnostics. Direct focused runs against this exact binary all passed:

| Filter | Passed | Log |
| --- | --- | --- |
| `recovery::tests` | 32/32 | `/private/tmp/ca-orc258-owned-95668-0.log` |
| `automation::tests` | 23/23 | `/private/tmp/ca-orc258-owned-95668-1.log` |
| `session::tests` | 61/61 | `/private/tmp/ca-orc258-owned-95668-2.log` |
| `session_access::tests` | 5/5 | `/private/tmp/ca-orc258-owned-95668-3.log` |
| `branches::tests` | 4/4 | `/private/tmp/ca-orc258-owned-95668-4.log` |
| `undo::tests` | 7/7 | `/private/tmp/ca-orc258-owned-95668-5.log` |
| `automation::notifications::tests` | 3/3 | `/private/tmp/ca-orc258-owned-95668-6.log` |

Total 135 selected cases, zero failures. The session-access log also contains
the child probe's separate 1/1 subprocess result; it is not double-counted.
At that checkpoint, net-zero, history-reset, partial-batch, mutation-drain, interleaved ordering,
deletion, uncheckpointed accounting, human-edit and four budget-rotation cases
were included in those passing receipts. Native transport remained fake.

The final read-only audit identified an ownership gap after an interrupted
archive move: identical metadata/history could survive in two locations and a
writer could select one copy. The approved follow-up adds a shared identity
location guard for all mutations, owned load/binding and heartbeat
snapshot/delivery, while canonical directory aliases count once. Four new
session regressions cover partial-move hard links, identical legacy UUIDs
under different storage keys, malformed outer/inner identity, and canonical
root aliases plus archived identity recreation. All four passed directly from
the fresh artifact compiled by default workspace run 39000 (2026-10-05
18:50:23; 2,899 available cases), with zero failures. These four are a separate
receipt from the 135 selected cases on build 95668; the full workspace result
remains owned by the root validation pass.

Artifact: `target/debug/deps/ca-78208114319849d8`.
SHA-256: `8c24b52e3ff1fcba8dd4937424a727ec381656098744a01b7fc816d2e5bc1a5c`.

| Exact new session regression | Result | Receipt |
| --- | --- | --- |
| `interrupted_archive_hard_links_refuse_every_identity_write_without_changing_either_copy` | 1/1 | `/private/tmp/ca-orc258-identity-39000-0.log` |
| `identical_legacy_identity_in_different_storage_keys_refuses_writes` | 1/1 | `/private/tmp/ca-orc258-identity-39000-1.log` |
| `malformed_duplicate_inner_identity_cannot_hide_behind_another_storage_key` | 1/1 | `/private/tmp/ca-orc258-identity-39000-2.log` |
| `canonical_root_alias_is_one_location_and_archived_identity_cannot_be_recreated` | 1/1 | `/private/tmp/ca-orc258-identity-39000-3.log` |

The restoration rescue/history-reset and receipt-before-ledger-rebase audit
found no further actionable contract gap.

The first full default workspace run 39000 failed; its complete result remains
in `/private/tmp/ca-orc258-workspace-default.log`. The owned baseline follow-up
isolated two unrelated fixture/artifact causes:

- `codex-team --lib`: all 16 failed cases reached `git commit-tree` without a
  persisted identity in their disposable repository. The initial fixture commit
  supplied `user.name`/`user.email` only with one-process `-c` overrides. Direct
  control runs on `codex_team-ae017b6e440b343f` passed all 16 when a fixed test-only
  author/committer identity was supplied; receipts are
  `/private/tmp/ca-orc258-codex-team-identity-control-0.log` through `-15.log`.
  `crates/codex-team/src/app_tests.rs` now configures identity with `git config
  --local` in the shared fixture and the one manually initialized fixture.
  Production Git behavior and real user/global configuration are unchanged.
  The repaired fixture still requires the fresh root-owned workspace rerun.
- `checked_in_schemas_match_the_protocol_types`: the old protocol test artifact
  contains `/Volumes/projects/ca/ca/ca/crates/codex-team-protocol` as its compiled
  manifest directory. That location does not exist; both checked-in schema files
  exist under the current checkout. The direct failure is recorded in
  `/private/tmp/ca-orc258-protocol-path-repro.log`. No schema source was changed;
  root is cleaning/rebuilding the affected workspace packages to discard
  relocated artifacts. Integration subprocess paths have the same extra `ca`
  artifact issue and belong to the companion fixture validation pass.

Fresh default workspace run 26390 passed all 16 repaired codex-team library
cases and all 15 protocol cases. Three protected-baseline CLI integrations
still fail individually on the fresh artifact because their required CLI
surface is absent in the saved pre-change source too. This is a separate
pre-existing contract gap, not a skipped product regression:

- `protected_baseline_cli_excludes_only_exact_finder_metadata_names`: preview
  exits unsuccessfully because `protected-baseline` is an unrecognized command.
- `protected_baseline_cli_is_digest_bound_head_anchored_and_mutation_free`:
  the legacy `verify protected-trees` handler reports ignored roots without
  a trusted content baseline; it does not emit the expected `baseline is missing`
  receipt. The test's later preview/apply command is also absent.
- `protected_baseline_cli_rejects_stale_revision_without_writing`: the absent
  preview command emits no JSON receipt, so its JSON parse fails at EOF.

The exact direct failures are
`/private/tmp/ca-orc258-protected-baseline-exact-0.log` through `-2.log`; parser
reproduction is `/private/tmp/ca-orc258-protected-baseline-help-repro.log`.
Artifact SHA-256: codex-team
`51b8064b0873c20bc77a4165bd5bb277ea7ffba5106febfd00f53d6e936ddaf3`;
integration test
`3c8b677949474fb19badd67f357b311042fcd8d8ec5637fc96753a0ef7f4cd04`.

Archive comparison against `scratch/oracle-improvements/20261005T211220Z/`
proves unchanged content for `crates/codex-team/src/cli.rs` (SHA
`c2dac1bae51d680cd0066f08b2aca88fff22358be5e56c5fd6e1f4d65e68f5bd`),
`src/main.rs` (`7630ef61ebf46ed233486192e3e96ca947d9936894f184db6c639719766c82c5`),
`src/verification/protected_tree.rs`
(`32e8cdfc1748240a74e79a52a0d12453e2661116c9d60e60c33acbd70a3fe030`),
and `src/verification/ca.rs`
(`c6d94d78f1648da8e5b756d03a084912364a4eda85009206ccef1b4bf52b3842`).
The unchanged `src/verification/mod.rs` does not register `protected_tree.rs`
at all, so the separate baseline implementation is not compiled into this CLI.
The integration test diff only adds the relocated-binary resolver; every CLI
argument and assertion is unchanged. Exact comparison and diff are recorded
in `/private/tmp/ca-orc258-protected-baseline-source-comparison.json`.
No production protected-baseline enhancement or expectation weakening was made.

Historical failure-first/build receipts below establish the repair sequence;
the current table supersedes their pending focused-test status.

- Shared binary `target/debug/deps/ca-78208114319849d8`, build 63793:
  `recovery::tests` selected 26 tests, 26 passed; `automation::tests` selected
  16 tests, 16 passed. Filters include companion recovery/local-automation
  cases. Session cases passed 60/60 after legacy oversized-read fix; a new
  bounded committed-preview case subsequently passed independently.
- Native metadata/focus/transport cases on the binary from build 87992:
  3 selected, 3 passed, with no real OS alerts. The net-zero case failed first
  with ENOENT before the Missing restoration fix.
- At that checkpoint, new net-zero fix, history-reset, partial-batch, deletion, uncheckpointed
  accounting, human-edit, blocking mutation-drain and interleaved-turn ordering
  cases awaited the next consolidated binary; the current table records their
  subsequent passing run.
- Budget receipt/rotation, reservation-time accounting and refund-capacity
  cases were subsequently added; passing compile 95668 is logged at
  `/private/tmp/ca-orc258-stable-build.log` and focused results appear above.
- `session_access` cases previously passed 5/5 including actual subprocess
  exclusion; branches/bookmarks/handoff cases passed 4/4. Owned source was
  formatted with the repository's Rust edition.
- Compile 33741 stopped on parent-owned fixture ambiguity at
  `src/main.rs:55101`; no owned diagnostic was reported. It is not a passing
  build receipt. Root coordinates the next stable-source run and final gates.

## Limits and remaining gates

Recorded typed UTF-8 edits only: 64 files/batch, 1 MiB/file, 20 native history
entries, bounded private journal and default 256 MiB quota. Pruning removes only
completed safe groups; native-history blobs remain protected, so exhaustion may
reduce new coverage with a warning. Preview text caps at 64 KiB while tokens bind
complete states/digests. Restoration restores recorded file states, not
transcripts, Git history or whole workspace snapshots.

Workspace admission is cooperative: older binaries, foreign programs, shell
and MCP writes do not automatically participate. Exact observed-state/workspace
identity checks remain necessary. Native presentation provides at-most-one
attempt, so a crash after admission may lose an alert. No real macOS alert,
provider request, user profile mutation or service installation was validated.

Budget storage remains bounded at 4 MiB/16,384 live events plus bounded immutable
per-run receipts. Settled evidence older than 30 days rotates safely; in-window or
unsettled evidence can still exhaust capacity and blocks new dispatch rather
than dropping exact accounting. Unresolved reservations remain conservative
until settlement. This is an explicit operational bound, not an unbounded log.

Applicable workspace matrices pass with the tested-source identity recorded
above. The final equivalent lint spelling passes affected tests, formatting,
strict Clippy and release; the signed candidate passes version/help, render
parity and all nine actual interaction paths. The actual eight-hour soak,
explicitly selected real-model quality/warm latency and recoverable signed
installation remain open. Oracle/source review evidence is recorded separately.
Focused or short successes do not satisfy the remaining release gates.

## Review groups and shared integration appendix

Review these foundation groups independently; the shared appendix identifies
files whose integration spans several groups. Root's source snapshot tooling
produces the concrete patches after stable validation.

| Group | Primary source and seams | Focused review/tests |
| --- | --- | --- |
| M2 ownership/persistence | `src/session_access.rs`; `src/session.rs` writer capability, unique active/archive identity location, committed binding, metadata revision and owned save/delivery methods | `session_access::tests`; `session::tests` stale-save, wrong-lease, malformed/duplicate identity, interrupted archive move, metadata revision and legacy migration cases |
| M3 typed recovery/native history | `src/recovery.rs`; `src/undo.rs` prepared mutation and stable batch identity | `recovery::tests`, `undo::tests`; private storage/fault matrix, turn/order/drain crash cases, preview/token and workspace exclusion |
| M3 family/bookmarks/handoff | `src/branches.rs`; additive SessionV4 bookmark/handoff fields and cheap family metadata in `src/session.rs` | `branches::tests`, session family roundtrip/fork cases; archived/cycle/duplicate/missing identity and prefix pruning |
| M7 automation storage/execution | `src/automation.rs` run/definition leases, immutable delivery and budget receipts, quiet decisions, rotation and reviewed deletion | `automation::tests`; no replay, drift, coalescing, provenance acknowledgement, exact accounting and deletion invariants |
| M7 native presentation | `src/automation_notifications.rs`, checkpoint native-alert field | `automation::notifications::tests`; fake transport only, durable admission, identity-verified focus and failure isolation |

Shared integration: `src/main.rs` owns active/retired turn admission, session
reload/final commit, startup warnings, worker events and user commands;
`src/app.rs` stores pane-owned state; `src/agent/tools.rs` moves recovery
admission and transient workspace activity into blocking mutation workers;
`src/agent/subagent.rs` attributes children to parent turns; `src/agent/registry.rs`
contains proposal/metadata schemas. `src/session_navigation.rs`,
`src/branch_handoff.rs`, `src/automation_ui.rs`, `src/dashboard_presence.rs` and
TUI rendering/input files are companion integration review groups.

Dispatch/runtime companion seams: `src/automation_dispatch.rs`, `src/schedule.rs`,
`src/schedule_v2.rs`, `src/headless.rs`, `src/service.rs` and `src/jobs.rs` own
pre-dispatch bounds, exact frozen counters, native output/runtime containment,
and activity lifetime. Their separate focused receipts and root-wide gates
must accompany the foundations; this note does not claim those source paths
are covered by recovery/automation unit filters alone.

## Final audit repair checkpoint — provisional

The failure-first run in `/private/tmp/ca-orc258-final-audit-negative.log`
observed both workflow contract defects: the serialized reasoning field was
`null` instead of `high`, and a one-request automation dispatched twice
instead of once after an unaccounted failed worker. The exact regression filters
are:

- `schedule::tests::automation_workflow_preserves_frozen_reasoning_in_worker_payload_and_legacy_omits_it`
- `schedule::tests::automation_workflow_unknown_failed_usage_stops_before_retry_and_legacy_can_retry`

The repair threads frozen reasoning through `WorkflowAutomationContext` and
`prepare_workflow_worker_spec` (`src/schedule.rs:2530`).
`record_workflow_worker_failure` (`src/schedule.rs:2552`) stops automation
replay when authoritative usage/side-effect evidence is absent, preserving the
conservative reservation; legacy schedule retries remain unchanged. Source and
fixture hashes are recorded in `/private/tmp/ca-orc258-workflow-audit-source.json`.
No passing repair receipt is claimed here.

Root is also checking
`tests::automation_headless_reasoning_keeps_frozen_absence_after_default_drift`
and
`tests::automation_headless_reasoning_refuses_unsupported_frozen_effort_after_reload`
for frozen absence and worker-reload capability drift. Earlier source fingerprints,
binaries and passing gates remain historical evidence for their own artifacts;
fresh validation is required for these repairs. The first fresh soak failed
after 60.459 seconds at `/bin/ps`; it is not an eight-hour duration receipt.
Real-model quality, the actual eight-hour soak and recoverable signed installation
remain pending. This checkpoint makes no completion or installation claim.

## Ownership inventory audit repair — focused positives passed

The failure-first run in `/private/tmp/ca-orc258-ownership-negative.log`
observed **0 passed / 5 failed** in 1.20 seconds. The existing identity proof
accepted unprepared large legacy history and exceeded candidate/aggregate
probe bounds; the integration cases also confirmed that worker preparation
was absent from turn ownership and recorded automation delivery. Exact filters:

- `session::tests::cold_unrelated_legacy_history_refuses_unprepared_short_identity_scan_without_writing`
- `session::tests::identity_inventory_candidate_bound_refuses_metadata_write_without_hiding_family_rows`
- `session::tests::identity_inventory_aggregate_probe_bytes_refuse_write_before_partial_proof_is_trusted`
- `oracle_integration_tests::oracle_turn_preparation_warms_cold_legacy_inventory_off_loop`
- `automation::tests::recorded_delivery_prepares_cold_identity_inventory_without_repeating_execution`

`SessionStore::prepare_identity_inventory()` (`src/session.rs:938`) now warms
compact identity probes on existing blocking workers. Worker preparation caps
at 16,384 directory entries, 256 MiB of uncached probe bytes and a cooperative
2-second deadline. The final owned inventory proof retains a 16,384-entry cap,
8 MiB of uncached bytes and a cooperative 25-millisecond deadline. Counting
all directory entries also bounds pathological non-session directories. An
incomplete proof returns `WouldBlock` with prepare/retry guidance; it does not
ignore a budget failure as an unrelated malformed session.

The process-local Unix cache stores only compact identities/version or bounded
parse failures, with at most 16,384 entries and nonblocking cache mutex access.
Every probe checks an opened regular file and its path using device, inode,
length, mtime and ctime (including nanoseconds), then repeats those checks after
reading or before returning a cache hit. Filename and mtime alone never prove
identity. Symlinks/nonregular files are rejected before opening; Unix opens
also use `O_NOFOLLOW | O_CLOEXEC | O_NONBLOCK` to prevent a FIFO replacement
from blocking admission. Exact target schema, history validity and committed
binding remain independently validated and uncached. Active/archive duplicate
UUID locations, malformed matching identity and interrupted-move hard links
still refuse unsafe writes.

Worker seams cover turn ownership/startup, source saves and navigation,
metadata operations, branch/handoff preview, restoration and compaction
verification in `src/main.rs`, `src/session_navigation.rs` and
`src/branch_handoff.rs`. `src/automation.rs` prepares before heartbeat ownership
and recorded-result delivery, retaining the no-provider-replay path. Warming
does not grant a writer capability: the final inventory proof runs again under
the existing lease, and target CAS semantics are unchanged.

Six additional session cases cover warm-cache reuse without history rewrites,
same-length edits with restored mtime, atomic replacements, uncached target
schema/sidecar validity, deadline/oversize refusal and a named-pipe fixture in
a timed child process. The consolidated run at
`/private/tmp/ca-orc258-ownership-positive.log` passed **91/91** selected cases
in **6.11 seconds**, including all nine new guard cases, the actual turn worker
integration and recorded automation delivery without replay. Cargo session
2236 exited zero; compilation took 1m21s with only the known linker unwind-size
warning. This receipt covers that artifact and precedes the later timeout
fixture repair. These cooperative inventory deadlines are **not** wall-clock
bounds on whole persistence:
filesystem calls, exact target I/O and durable commits are separate. Previous
gates remain artifact-specific; no whole-workspace, release, completion or
installation claim is made here. Real-model quality and a fresh signed-candidate
eight-hour soak remain pending.

## Background timeout fixture repair — focused positives passed

The revised default workspace log
`/private/tmp/ca-orc258-final-20261006T002818Z-default_workspace_tests.log`
recorded ca at **2,880 passed / 1 failed / 41 ignored** in 218.81 seconds. The
failure was
`agent::subagent::manager_tests::timeout_diagnostics_include_surviving_background_job_attribution`:
expected one attributed job and observed zero. Three independent runs of the
same frozen artifact reproduced that failure, recorded in
`/private/tmp/ca-orc258-child-timeout-triage.json` and
`/private/tmp/ca-orc258-child-timeout-triage-1.log` through `-3.log`. That test
binary had SHA-256
`0ea5dff2656d8271a9a04ad1f1c9231d61ec855c9c39a93502987de068a07376`.

The fixture's 30 ms child runtime began before real native-job registration.
Job startup first awaits a blocking workspace activity lease, then prepares
and spawns the process before inserting its attributed record. Expiring during
that admission correctly leaves no background job to attribute. The approved
test-only repair in `src/agent/subagent.rs` reports the actual registered JobId
through a rendezvous, leaves its runner pending, and uses a bounded two-second
runtime. Registration plus terminal observation must finish within four
seconds; cleanup cancels the child and kills/reaps all owned jobs within five
seconds before attribution assertions. Exact registered ID, background status,
survival at timeout and diagnostic attribution remain asserted.

Production code before the test module is byte-identical, SHA-256
`133b4b5f56c15f79b82a934774f86431fe6926d635475713966f70a66df8c6cf`.
The repaired file SHA-256 is
`217b7eed6f80f426741377257e1a5703a0fc017a29390c515036068071bfa42f`;
test-module SHA-256 is
`1598e231ff6712df9f10068c7397a5116bddb8ef5c5285e6d9f57c4a14f7faa8`.
No production timeout, attribution, cleanup or exclusion was changed. The
fresh `manager_tests` run (Cargo session 32834) passed **32/32**, zero failures,
in **3.64 seconds**, including the repaired timeout case and completed fixture
cleanup. Receipt: `/private/tmp/ca-orc258-child-timeout-positive.log`. The
restarted pipeline 17551 was later superseded after its PID-publication fixture
failure. This historical focused result did not establish final workspace,
release, interaction or eight-hour timing gates; the current integrated summary
above records the subsequent passing checks and remaining release gates.
