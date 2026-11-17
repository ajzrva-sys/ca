# FLT-338 — Automatic filter setup with one editable preview

Status: COMPLETE. User approved the complete plan on 2026-11-09; signed ca 0.1.96
was installed and passed normal-entry acceptance on that date.

## Outcome

Every new interactive task receives one local, editable preview before any
planner/provider request. Private values is the default; Private values plus
suggestions adds editable phrase/identifier selections. Original, Provider copy
and Changes expose actual coverage and stop conditions. Send and run authorizes
automatic planning/execution once, preserving normal action permissions. Escape
restores the draft. Active steering/continuations reuse the accepted policy.

## Baseline and ownership

Installed/source baseline: ca 0.1.95; previous full gate
scratch/verify/20261109T085617Z-78373. No root Git metadata or .agent/PLANS.md
exists. Baseline source hashes are in scratch/FLT-338/baseline-source.json.
Root owns admission, async results, state integration, documentation and final
validation/install. Separate bounded owners implement task-filter runtime,
preview UI, and transactional automatic execution. Freeze all source before
final independent review and verification copies.

## Implementation contract

- Presets, saved explicit rules and task edits are separate. Existing stop
  rules remain active. Each task has an immutable effective filter and retains
  required historical alias mappings. Older workers cannot adopt new-task edits.
- Local preparation includes attachments/history and is asynchronous. Send is
  bound to the reviewed content and rule revision; stale analysis is discarded.
- Suggestions are editable and task-only unless Remember for this project is
  selected. No duplicate provider comparison request or automatic vocabulary
  expansion after Send.
- Automatic graph execution reuses the durable exact-revision start path and
  records Send authorization truthfully. Explicit Plan and restored work remain
  review/paused. Local commands and noninteractive entry retain existing behavior.
- Coverage means existing supported source data, filenames, URL hosts and
  explicit selections, not universal secret detection.

## Validation and release

Focused behavioral/state/render tests, before/after-save failure cases, queued
and ordinary admission, cancellation, stale input, two presets, source/path
restoration, resume and concurrent-task isolation. Native terminal acceptance
must show no pre-Send provider request, exact-once start and two visibly fresh
pending updates with at least 192 history messages in wide/narrow layouts.
Run formatting, strict Clippy and required full workspace gate after source
freeze and independent review. Bump once, build one release, install the same
signed artifact recoverably, then verify installed normal entry and protected
state. Record failed/missing checks without relabelling them as passes.

## Changed areas

- `src/task_filter.rs`, `src/filter_review.rs`, `src/app.rs`, `src/main.rs`:
  shared policy, local preparation, revision-bound Send, queue/hook admission.
- `src/provider_filter.rs`, `src/privacy/`, `src/agent/client.rs`:
  task snapshots, restored aliases, explicit rules, project choices and receipts.
- `src/tui/privacy.rs`, `src/tui/privacy/guide.rs`, `src/tui/commands.rs`:
  editable modes, live inspection, advanced controls and accurate help.
- `src/work.rs`, `src/plan.rs`, `src/plan_workflow_runtime.rs`,
  `src/automatic_tasks.rs`: truthful Send authorization and durable graph start
  and progress. `src/agent/subagent.rs` and `src/headless_contract.rs` restore
  structured results before admission while preserving raw-output bounds.
- `src/session.rs`, render/behavior tests and the affected command snapshot:
  failure injection, retry, lifetime, concurrency and layout regressions.
- `tools/filter_review_pty.py`, `tools/verify_runtime.py`: ordinary terminal
  acceptance and existing full-gate compatibility. The new policy/admission
  modules are registered in the existing harness-protected source inventory.
- User/configuration guides, architecture/parity notes and task indexes document
  the new flow; `Cargo.toml` and `Cargo.lock` advance only the root CA version.

## Progress

- [x] Read-only review and user decisions complete.
- [x] Current baseline inspected; implementation ownership assigned.
- [x] Shared task-filter runtime and local preview implemented and verified.
- [x] Interactive admission and editable preview UI implemented.
- [x] Automatic execution after Send implemented through existing transaction.
- [x] Independent combined review and required validation.
- [x] Recoverable installation and installed acceptance.

Qualification: `scratch/FLT-338/qualification.json`. Signed installed release:
`scratch/FLT-338/installation.json`. No required FLT-338 work remains.

## Retained attempts

- Focused attempts 20261109T194403Z-43161 and 20261109T194442Z-44632
  failed before compilation because the stable private cache lacked the pinned
  nono checkout and its newer registry dependencies. Dependency bytes were
  refreshed from the current local cache; no credentials/config were copied.
- Independent source review found remembered selections, analysis-error recovery,
  and unchanged inspector-copy issues. Repairs retain explicit selections, give
  failed analysis an editable retry/cancel state, and skip unchanged generations.
  Native imported history is included in the reviewed fingerprint.

- Full verifier self-test passed at scratch/FLT-338/verifier-selftest/result.json:
  existing baseline passes, real injected production defect fails the unchanged
  test, and restored source passes with exact restoration.

- First compiled focused run: scratch/check-change/runs/20261109T194753Z-52397.
  30 tests passed, 9 failed (eight invalid task-metadata alias-kind failures and
  one automatic-execution save-fault test). These are non-passing intermediate
  results. Source review also identified concurrent old-task registry commit
  lineage and edited-identifier alias protection; bounded repairs are in progress.

- Runtime verifier contract tests passed: python3 tools/test_verify_runtime.py
  (16 tests). Python syntax checks passed for the new native fixture and the
  updated verifier fixture.
- Admission retains attachment source evidence on the detached task preview;
  accepted queued tasks reuse the reviewed expansion rather than resolving or
  fetching their attachments a second time.

- Focused final repair run `scratch/check-change/runs/20261109T200409Z-90634`
  passed 46 tests, formatting and strict affected-bin lint; source digest
  `7a8f88bbdb7172c30c6e35172c45ddc0e192748756e1d3cb7d9b71c129a6b9c8`.
  Intermediate attempts `20261109T200023Z-81616` (test-only Arc move) and
  `20261109T200128Z-84006` (large-enum/needless-borrow lint) failed and were repaired.
- First native debug attempt `scratch/FLT-338/native-debug` failed after Send:
  preview/cancellation made zero provider requests, but the planner lacked
  saved-session ownership for private aliases. The saved workflow reports the
  exact failure. This is an in-scope integration blocker, not passing acceptance.
- Writer ownership now travels with the saved preview into AppState. Independent
  combined review confirmed that fix. The added regression's first two attempts
  (`20261109T201056Z-6556`, `20261109T201150Z-8816`) failed because the disposable
  test's privacy scope did not match its session store; fixture setup was repaired.
- Native attempt `scratch/FLT-338/native-debug-ownership` proves Send starts one
  planner and automatically starts the graph, but failed the native file read:
  structured plan grants retained filename aliases. Exact restoration before
  plan admission is being repaired; the fixture/permission checks remain intact.
- Coverage review identified missing direct queued/hook admission and new-Send
  save-failure tests, plus same-length attachment changes. Bounded regressions
  are being added at those existing seams before final validation.
- Structured planner results now restore through their exact privacy context
  before schema/task-map admission. Raw JSON envelope/resource limits remain
  enforced before restoration, and restored values are revalidated. Independent
  review found no remaining issue in that repair.
- Save-failure review identified a fresh-owner retry issue: a detached candidate
  retained its temporary writer after failed persistence. Error cleanup now
  releases that temporary owner; fresh-owner before/after fault coverage is
  required before qualification. Queued/hook tests drive the actual local
  preview worker and accepted submission entry, including double Enter.
- Expanded focused run `scratch/check-change/runs/20261109T202103Z-29535`
  passed 60 tests. Its new structured-result regression failed because it
  incorrectly expected two spellings of one workspace file to become different
  paths. The test now preserves that canonical equivalence and separately checks
  rejection of a path outside the workspace. Production restoration was unchanged.
- Final combined independent review found no unresolved blocker in the frozen
  admission, persistence, restoration and automatic-execution changes. Full gate
  must use the revised runtime fixture's own criteria, without a cross-criteria
  performance comparison to the older baseline gate.
- Corrected structured-result test, format and strict lint passed in
  `scratch/check-change/runs/20261109T202342Z-36077`, source digest
  `3915767fa6719138393a327b3d60c3fa17ecdc2c32769f9989a72c27d97712a0`.
- `scratch/FLT-338/native-debug-paths` showed restored native reads and two
  visibly fresh pending updates in both widths. It failed because the fixture
  could mistake the first completed graph for the second. The predicate now
  binds the expected task title and graph plan ID; an independent reviewer
  confirmed the race against the retained seven-request trace.
- `scratch/FLT-338/native-debug-final` then exposed a real second-task durability
  failure: UI completed and seven requests succeeded, but the saved graph stayed
  running after a session revision conflict. Privacy receipt publication and
  graph saves could race using cloned ownership. The required repair orders
  graph saves with the existing privacy commit gate on the blocking worker,
  persists changed graph state before publishing the tick, and avoids redundant
  TUI metadata saves. This attempt remains failed pending the permanent repair.
- The durability repair is frozen and independently reviewed. New regressions
  force a committed-but-unpublished privacy receipt to overlap a real graph save,
  preserve rejection of unrelated stale edits, and save two workers' terminal
  graph state before publishing completion without dispatching them again.
- Narrow preview layout reserves the actual wrapped coverage height. The existing
  render test now verifies the complete coverage/gap note at 140×40, 70×30 and
  60×24 while retaining the second pending update and usable controls.
- Frozen repair validation `scratch/check-change/runs/20261109T203500Z-61529`
  passed all 78 selected tests, formatting and strict lint.
- Native debug acceptance `scratch/FLT-338/native-debug-durable/receipt.json`
  passed with 196 retained messages, seven requests for two tasks, two planner
  calls, no pre-Send request, cancellation, both modes, edited suggestion,
  automatic graph execution, exact source restoration and unchanged fixture
  settings/source. Both pending updates were visibly fresh in wide/narrow panes.
  Native terminal restoration and clean exit passed. The corrected fixture keeps
  exact task/plan identity for completion.
- Root version bumped once from 0.1.95 to 0.1.96. Required full gate and the
  intended release build are running in `scratch/FLT-338/full-gate`; installed
  CA remains 0.1.95 until the candidate passes and is replaced recoverably.
- First full gate `scratch/FLT-338/full-gate` passed preflight, workflow checks,
  formatting and strict workspace/all-target lint. Its default CA tests passed
  4,053 with 44 opt-in tests ignored and two help-related failures: `/filter`
  was missing from provider searches, and the usage assertion still expected
  only `[on]`. The run was deliberately interrupted before any release build;
  the unfinished workspace matrix is recorded as infrastructure_failed, not a
  pass. Preserve this report/log. Repair restores provider discoverability and
  updates only intentional help syntax/snapshot expectations, then reruns the gate.
- Help repair passed both focused tests, formatting and strict lint in
  `scratch/check-change/runs/20261109T205258Z-58006`. Provider search keeps its
  established five-command assertion; only the intentional `/filter` help rows
  and usage expectation changed. Final full gate is running at
  `scratch/FLT-338/full-gate-final` (tool-managed job 36179). Version remains
  0.1.96; no second bump and no release build from the interrupted first gate.

## Final qualification and installation

- Final full gate job 36179 completed formatting, strict workspace lint, default
  matrix (4,952 passed / 45 ignored), all-features matrix (5,108 passed / 46
  ignored), the one intended release build and candidate signing. Source digest
  remained `9a16eb283c4530ce53a60357c8ab18fd02f08472c79ea73775b257bfa87bf2dc`.
  These counts exclude nested child summaries. The gate's runtime failed because
  the old fixture wrote a raw filename under the new default alias policy;
  downstream review/resize did not run. The original report remains failed and
  untouched; it is not described as a passing monolithic gate.
- Dedicated release acceptance passed before the fixture correction. The runtime
  repair changed only `tools/verify_runtime.py`, `tools/verify_evaluate.py` and
  their two test files. It uses the advertised workspace reference for creation,
  binds the successful native result to the issued file reference, and uses it
  in reviewer inventory, Changed path and reads. Failed tool results are rejected.
  Disk bytes, exact turn-owned candidate digest, six requests, sole report and
  visible completion requirements remain intact. Parsed tool-argument equality
  tolerates JSON whitespace while rejecting malformed and duplicate-key input.
- Preserve `runtime-repair/result.json`: its write passed but reviewer context
  still expected the old Changed path. Preserve
  `runtime-repair-final-evaluation-first-failure.txt`: a serialized-JSON
  whitespace comparison failed before the exact parsed-object correction.
- Final corrected runtime passed all nine cases on the unchanged signed
  candidate in `runtime-repair-final/result.json`. Independent evaluation passed
  in `runtime-repair-final-evaluation.json`; 82 Python contracts passed in
  `runtime-repair-contracts.log`. Combined source review and final Python delta
  review found no remaining blocker. Verifier self-test job 74638 passed baseline,
  detected the real intentional defect and passed exact restoration in
  `verifier-selftest-qualified/result.json`.
- `qualification.json` separately records the original failed gate, immutable
  passing Rust-stage logs, four changed Python hashes and fresh runtime evidence.
  No Rust source changed after those matrices/build, so their evidence and the
  same release artifact were reused; no second release build/version bump ran.
- Candidate and installed ordinary-entry acceptance both passed using
  `tools/filter_review_pty.py`: `native-release/receipt.json` and
  `native-installed/receipt.json`. Each retained 196 history messages, seven
  provider requests for two tasks and two planner calls; no task provider request
  preceded Send. Cancellation, both modes, edited suggestions, automatic graph
  execution, exact native source restoration, two fresh pending preview updates,
  clean exit and terminal restoration passed. Wide/narrow captured frames were
  manually compared, including complete coverage text and the second update
  above the first. Fixture source/config remained unchanged.
- Signed release SHA-256:
  `eb4c81af7585ab0154465c29116b1c73aa9198d6a8dd0aadb5d95a9987774c48`.
  `tools/bug257_install.sh --check` passed; background install job 14459 completed.
  Candidate/staged/installed hashes, executable owner/mode, destination ad-hoc
  signature, version/help and installed native entry passed. Install log:
  `scratch/FLT-338/install.log`; readback: `scratch/FLT-338/installation.json`.
  Recovery of 0.1.95:
  `/Users/aji/.local/bin/.ca-release-stage.Zaf8Kx/ca.recovery-ORC258-20261109T215004Z-b9162136f9fb3c19ac28d768edf5a22738e048c08dd816a4d5ed30ad487707de`.
- All 4,459 production config/session metadata entries matched the baseline
  after installation (`protected-runtime-after.json`). Running sessions were
  left alone. Real external/paid providers, ignored opt-in cases and other host
  platforms were not exercised; no such coverage is claimed.
