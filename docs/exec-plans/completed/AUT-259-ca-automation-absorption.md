# AUT-259 — ca-owned automation and codex-team absorption

Status: COMPLETE (2026-10-06). Authorized by the user's full implementation request, including the two subsequently reported continuation failures.

## Goal and authority

Implement the approved absorption plan: ca owns triggers, admission, grants, budgets, run identity, worker supervision, history, recovery, session delivery and notifications. Extract executable services and runner-free contracts into ca-automation and ca-automation-protocol. Preserve legacy codex-team clients, V1 bytes, grants and recoverable state. Repository tasks implement in isolation and integrate verified candidates locally; self-improvement evaluates unattended but requires exact review before promotion.

## Baseline

The current source is a non-Git filesystem checkout. Offline workspace check passed during planning. Automation/workflow/mechanistic source exists but is disconnected from codex-team module/CLI registration; MCP exposes observe/operate rather than delegate. Nine associated baseline integration failures are recorded in ORC-258-baseline-test-exceptions.md. The delegate runtime profile names a missing historical path. Runtime state will only change through explicit migration/management commands, not implementation fixtures. The referenced .agent/PLANS.md is absent; this plan follows the existing active ExecPlan structure.

A recoverable content manifest and source archive are under scratch/automation-absorption; LATEST identifies the current baseline.

## Milestones

1. Restore executable typed surfaces and create native engine/protocol crates with legacy forwarding adapters and unchanged legacy wire identities.
2. Add typed preview/start/status/cancel/reconcile, correlated run/action journals, durable full receipts, cross-profile resource ownership and isolated ca workers.
3. Connect native workflows/mechanistic targets without MCP opt-in; unify due admission and expose durable lifecycle management and activity.
4. Add repository targets with isolated native workers, exact-tree host verification/review and journaled fast-forward integration; retain candidates on stale/dirty/conflicting targets.
5. Register harness discovery/evaluation targets and enforce exact review-required promotion in controller, CLI and resumed evaluation; preserve activation/rollback watchdogs.
6. Add previewed validated recoverable migration and retire duplicate scheduling; validate built CLI/MCP, actual TUI, default/all-feature unfiltered matrices, formatting, strict lint and release; install recoverably only after gates.

## Invariants

- Engine execution never independently admits occurrences or imports ca provider/session internals.
- Per-action states distinguish prepared/running/confirmed/unknown effects. Reconcile before replay; ambiguous effects need attention.
- Leases are ownership/process-bound, not time-only. Legacy and native callers share the namespace.
- Exact definition/resource/policy bindings grant unattended work. Compatible renaming preserves semantic authority; widening requires review.
- Scheduling and UI do not block on long synchronous execution.
- Integration requires fresh candidate-tree evidence and branch/resource ownership. No force integration, automatic rebase or automatic push/deploy.
- Evaluation alone never promotes. Promotion review binds exact candidate/evaluation/current baseline. Existing activation recovery remains intact.
- Existing disabled automations remain disabled, private output stays private, unchanged monitors remain quiet.

## Validation

Focused failure-first and hermetic process tests cover duplicate dispatch, independent occurrences, completion-before-parent-crash, ambiguous effects, owned cancellation, observation safety, cross-profile resources, native no-MCP workflows, verified/stale repository integration, promotion gates, migration, legacy wire compatibility and responsive terminal interaction. Run unfiltered default/all-feature workspace tests and distinguish every failure from ignored tests; no baseline skip list is a passing gate.

## Progress and evidence

- 2026-10-06: task authorized; source snapshot captured; implementation work delegated by subsystem.
- Native crates and compatibility reexports created. Dormant typed surfaces restored; real stdio MCP compatibility passes 8/8.
- Native unfiltered engine library rerun passes 681 cases, zero failures, one existing ignored. Restored-module failures were repaired, not excluded.
- ca-owned durable worker, action journal, parent-held shared ownership, completed receipt recovery, common due admission and management are connected.
- Repository engine tests cover local integration, stale/dirty targets, exact checks/review, and legacy publication ownership. Native worker/reviewer/host-check fixture passes on the default debug stack without MCP.
- Harness discovery/evaluation/idle cycles are registered behind common admission, physical request budgets and exact promotion review. Legacy supervisor keeps health/rollback only. Vault migration retains absolute recovery identities.
- User additionally reported continuation transport and missing lifecycle parent ownership failures. Exact parent ownership repair and one bounded same-request continuation recovery are implemented with focused regressions.
- Runtime/config migrations are implemented as previewed, validated copies. Production profiles and sessions have not been modified.
- Both screenshot regressions pass focused tests (12 continuation cases): completed tool results and exact parent ownership survive one bounded same-request transport recovery.
- Real inherited resource/executor descriptors survive parent closure and retain claims through live descendants (10 native runtime cases pass). Legacy schedule sidecars now use the same run identity and journal; V2 recovery preserves active children independent of elapsed time.
- First complete default matrix: ca 2,906 passed, 41 existing ignored, one migration alias failure (repaired); native engine 684 passed, one existing ignored; compatibility/integration/doc targets complete. These earlier matrices were diagnostic; final unfiltered default/all-feature matrices and strict lint follow the completed terminal checks.
- Actual terminal automatic continuation recovery and explicit `/retry` both preserve the byte-identical frozen request, completed file identity and parent lifecycle owner. A native ordinary prompt also executes its real command in the required sandbox and completes successfully.
- Actual terminal typed workflow input responsiveness passes with 32 ordered keystrokes, about 20 ms p95 and no provider requests. Offline verification exposed a macOS nested-sandbox startup failure; command execution now receives one physical sandbox per untrusted command, with private config/journal/lease paths denied. Compiler-loader restrictions are repaired and the real fresh-target compiler acceptance test passes; matching terminal verification is still pending.
- Native storage confirmation now remains byte-identical before and after directory creation; strict worker binding checks are retained. Trusted Git metadata/staging/commit operations suppress repository-supplied filters, hooks, monitor and signing helpers.
- Actual fresh-target compilation, linking and test execution pass under the required command sandbox. Its trusted runtime libraries, validated public OS TLS configuration, SDK and compiler selection stay bounded; source/private state/network and non-scratch writes remain denied.
- Final review found and repaired exact command cwd rebinding and failed-worker receipt acceptance. Confirmed effects remain recoverable without replay. Checked-out publication now uses expected-old Git transactions and owned index guards; a real ignored-file overwrite probe led to explicit collision checks. Native V2 isolation is frozen at admission, and ordinary physical commands do not inherit authority descriptors.
- Current focused ca acceptance passes 22/22, including actual required-isolation repository checks, malformed/nonzero worker recovery, frozen admission policy and descendant cleanup. Git commit/abort now close their owned input pipe before waiting for exit. Native combined tests exposed local journal starvation and a raw-fork fixture inheriting unrelated parallel-test locks; both are repaired. Process-local journal coordination retains the existing OS-lock bound, and the inherited-lock probe runs in its own exact subprocess.
- Current focused acceptance passes 43 ca cases (one existing ignored), 69 native engine cases, and two built CLI integration cases. A real compatibility observer crash preserves the live ca-owned worker claim; its completed receipt is reconciled and duplicate dispatch does not repeat the effect.
- Actual terminal mixed query/verification exposed and repaired a final native dispatcher gap after admission. Ordered execution retains the original V1 request digest, checkpoints each action and refuses to replay an unconfirmed effect. Four native regressions pass. The matching debug terminal workflow and mixed Git-query/Required-CargoTest both complete: 32 ordered keys each, p95 21.722/20.108 ms, zero provider requests and restored terminal state. Earlier stopped runs remain diagnostic evidence.
- The complete final-source default diagnostic matrix ran every target (no exclusions) and found four failed targets: a startup stopwatch, transient Darwin group state, false global workspace-registry contention, compatibility workflow claims and an atomic cache publication race. It is retained under `final-validation/diagnostic-default-20261006T1337Z`; the stalled isolated CLI fixture was stopped to expose its failure and allow the remaining targets to finish. Its bounded standalone reproduction passes. The repairs are frozen: all 14 focused root checks, 39 native workflow/cache checks and 57 built CLI integration cases pass. The complete default/all-feature matrices will now be repeated against those repairs.
- The final inherited-authority fixture was isolated in its exact subprocess to eliminate unrelated parallel-test fork state; every original authority/noninheritance assertion remains intact. No production policy was weakened. Final unfiltered suites, lint, formatting and release build now pass at one unchanged compiled-input digest. Earlier failed matrices remain diagnostic evidence.

## Final acceptance and release evidence

All required gates pass at compiled-input SHA-256
`130cc08ec3d36ff2c8dd4874a5368dc5f5d28298419267693dc87a7febc8fdfa`
(1,073 inputs). Default workspace: 26 targets, 3,783 passed, zero failed,
43 existing ignored, zero filtered. All features: 27 targets, 3,932 passed,
zero failed, 44 existing ignored, zero filtered. Both matrices use the full
workspace without exclusions, with four test threads. Strict default and
all-feature workspace/all-target Clippy, formatting and the all-feature
release build pass. Exact commands, log hashes, completion times and per-target
results are in `scratch/automation-absorption/final-validation/`; `TESTING.md`
records the repeatable commands. Release build completed in managed session
54352; `release-workspace.log` and `release-gate.json` retain its result.

Actual signed release terminal workflows and ordered mixed Git query/required
Cargo verification succeed with zero provider requests and 32 ordered keys each
(p95 19.572/20.568 ms). The continuation fixture proves automatic frozen retry
and explicit `/retry` preserve completed write bytes/inode/mtime and lifecycle
ownership; an ordinary native prompt succeeds through required command isolation.
Both terminal fixtures restore terminal state. A SIGKILLed legacy observer leaves
the live ca worker/resource claim intact; its completed V1 receipt reconciles,
and duplicate dispatch performs no second effect. Matching debug fixtures also
pass. Relevant receipt names: `pty-release-mixed.json`,
`pty-continuation-release.json`, `compatibility-parent-crash-release.json`.

The paired installer initially refused renamed-backup help text before replacing
any executable. The exact byte/signature-preserving fix supplies the logical CLI
name while executing the explicitly selected absolute file. Seven focused
installer tests pass, including real renamed signed copies, lost signing
acknowledgement, unrelated replacement retention and inherited installation
claims. No Rust/build input changed for this repair.

Recoverable paired installation completed through managed session 9381. Both
existing PATH destinations `/Users/aji/.local/bin/ca` and
`/Users/aji/.local/bin/codex-team` were staged on their destination filesystem,
atomically replaced, force ad-hoc signed at the installed paths and verified.
Signed ca SHA-256:
`f750a7a07dbda3e500f55d177830d58d4d062389d0bb547a637d7a187d23c1e6`.
Signed compatibility CLI SHA-256:
`8dd5300589bfac73b5ed4be3d80f0aa09fc6951a0e0bc59d60ed6c3194c6ad1f`.
Independent read-back checks PATH, bytes, mode 0755, owner, all-architecture strict
signatures, version/help and both preserved before-images. Evidence copies are
`install-receipt.json` and `installed-readback.json`. Recovery receipt:
`/Users/aji/.local/bin/.ca-automation-stage-7ck13j2t/paired-install.json`;
run `python3 tools/automation_install.py --recover RECEIPT` for guarded paired
rollback. Timestamped SHA-labelled before-images remain in the receipt's two
private adjacent stage directories. The earlier refused stage remains diagnostic.

## Completion

The executable baseline, native crates/services, common lifecycle, target/UI
connections and compatibility forwarding migration are complete and installed.
No AUT-259 release blocker remains. Production profiles, paused automations,
vault/session records and running sessions were not migrated or changed; use the
explicit preview/copy/validate commands for state migration. The checkout lacks
Git metadata and the old delegate binding remains unavailable; doctor surfaces
both, and repository execution requires an actual reviewed Git binding. Hermetic
Git fixtures establish repository publication acceptance without inventing live
Git state. Changed resource bindings still require review.

Source baseline/candidate archives, manifest and reviewable patch preserve this
non-Git delivery. Documentation and installer-only follow-ups retain the compiled
input digest above. Prior real-model recall and eight-hour soak requirements from
other plans remain separately unresolved; short terminal evidence is not a soak.
