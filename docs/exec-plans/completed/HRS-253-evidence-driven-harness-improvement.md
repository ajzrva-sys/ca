# HRS-253 — Evidence-driven harness improvement

Status: COMPLETE — original six-stage implementation and authorized setup
Owner/session: Codex, user-approved implementation on 2026-09-22

## Final completion receipt (2026-09-23)

The six-stage implementation and authorized dedicated-VM setup are complete.
The release is installed at `/Users/aji/.local/bin/ca` and automatic idle experiments
are enabled for `/Volumes/projects/ca/ca`, capped at $20 and 7,200 active seconds
per cycle. They require five quiet minutes and a 24-hour cooldown. Existing
running sessions were left alone; the new configuration applies on the next launch.

Final all-feature workspace tests: **3,461 passed, 9 failed, 42 ignored**. All 2,666 application
tests pass. Strict all-target/all-feature Clippy and formatting pass. The final
54-trial VM guidance lifecycle passes: 51 verified successes, three expected
baseline failures, no infrastructure failures or false completions, 106 local
provider requests, activation and rollback, and no outstanding reservations
(2,053.29 seconds). Final Tart inventory confirms both local templates stopped
and no disposable VM clones remain.
This used a deterministic local provider and real VM workers/verification; it
does not establish improvement on real-provider tasks.

The existing workspace integration failures remain in `mcp_sdk` (five),
`protected_baseline_cli` (three) and `remote_cli` (one). Engineering admission
continues to reject candidates that encounter them, including guidance candidates.
No gate was waived. The original checkout also contains pre-existing uncommitted
work, so source-changing experiments remain ineligible until it is clean.
Existing edits were preserved; no reset, stash or commit was performed.

The selected DeepSeek pricing refresh succeeded from its official page. New
cards explicitly account at the peak-rate upper bound; they are not invoice
amounts. Reservations use the verified provider context ceiling independently
of a smaller configured request-size cap. Historical pricing identities remain
unchanged. No paid-provider efficacy run is claimed.

The optimized workspace build passed. Candidate/staged/installed hashes,
ownership, executable mode, ad-hoc signature and version/help checks passed.
Installed SHA-256: `d0a6b0acf53bc33a7e30a8941bb54a6d8d558f100b6946b48e96a0b82349f4de`.
The previous executable and opaque configuration are recoverable in
`/Users/aji/.local/bin/.ca-recovery-HRS253-20260924T000247Z-27221`.
The staged configuration passed schema/settings validation in a private HOME;
saved credentials were resolved by the installed application at the original
config location, and its model-list check passed.

Final logs: `/tmp/ca-hrs253-final-workspace-tests.log`,
`/tmp/ca-hrs253-final-clippy.log`, `/tmp/ca-hrs253-final-format.log`,
`/tmp/ca-hrs253-vm-full-cycle.log` and `/tmp/ca-hrs253-final-install.log`.
Installation receipt: `/Users/aji/Documents/Codex/2026-09-22/https-github-com-china-qijizhifeng-agentic/work/hrs253-recovery/final-install-receipt.json`.

Coverage also includes the measured 54-trial hint lifecycle (124 local requests,
activation and rollback), the dedicated-VM source build/adoption/activation/
rollback transaction (287.10 seconds, with controlled trial receipts), and
real-clock idle dispatch/generation/foreground pause/accounting/shutdown.
Their logs are `/tmp/ca-hrs253-hint-full-cycle.log`,
`/tmp/ca-hrs253-vm-promotion-real.log` and
`/tmp/ca-hrs253-idle-pty-run3.log`.

Interrupted pre-decision cycles can close only after exclusive controller,
worker and VM ownership proves quiescence. Closure preserves unknown charges
and evidence; it cannot replay requests, refund reservations or authorize
promotion. User pauses and activation recovery retain their separate paths.

Earlier checkpoints below are historical. Their live handles, disabled setup,
unfinished status and proposed next steps are superseded by this receipt.
Three installer preparation attempts stopped before replacing the executable
or runtime configuration: Python 3.9 hashing compatibility, the expected
private-HOME credential lookup, and the staged executable's help basename.
The corrected installer passed. Their failed receipts remain in
`/tmp/ca-hrs253-final-install-attempt1.log` through `attempt3.log`.
Additional full-source, promoted-process restart, full real-clock idle and paid
efficacy runs were further validation ideas, not requirements of the original
six-stage request. They are not claimed as completed tests.

## Goal

Complete the existing AHE-inspired implementation: reliable task measurements,
effective runtime component identity, sourced analysis, matched coding-task
evaluation, automatic guidance/source promotion, and measured execution hints.

## User decisions

- 2026-09-23: user selected a dedicated macOS VM for automatic source tests.

- All six stages, including guidance and Rust candidates.
- Automatic activation after frozen evaluation checks.
- Automatic experiments while idle; $20 and two active hours per cycle.
- Successful source changes land on the current ca branch; do not absorb local
  edits or apply stale candidates.

## Evidence before implementation

The existing working tree contains 1,251 changed paths. Baseline source copies
are retained outside the repository in the task's work/hrs253-baseline directory.
The current telemetry equates exit with acceptance, fingerprints source files
under arbitrary customer workspaces, and verifies manifests by timestamp.
Analyze receives counters rather than events. codex-team already provides a
vault, source inspection/fast-forward, evaluation contracts, and A/B activation.
Its existing learning replay is text-only and does not generate source candidates.

## Scope and invariants

Allowed: ca outcomes, requests, learning, harness and loop-guard modules; their
runtime/TUI/CLI integration; codex-team self-improvement and related interfaces;
focused fixtures/tests and project documentation. Preserve pre-existing edits.
Oracle/reference trees remain read-only. No Cursor parity claim.
Source candidate experiments run in isolated clean worktrees. Host-owned
evaluation, spend, promotion, and rollback controls cannot be candidate edits.
Unknown evidence cannot become success or authorize deployment. Foreground work
must remain responsive. Existing sessions survive binary replacement.

## Milestones

- [x] M1: outcome v2, task/event identity, explicit feedback, runtime fingerprints,
  evidence validation, chronological cohort stability, legacy compatibility.
- [x] M2: typed manifests, frozen coding-task evaluations, guidance/source
  generation, host verification, repeated baseline/candidate promotion gate.
- [x] M3: idle orchestration, persistent budgets, controls, source adoption,
  signed atomic binary activation and recovery.
- [x] M4: independently evaluated, bounded execution hints and verification
  invalidation after edits.
- [x] M5: focused and full validation, terminal checks, bounded VM cycle,
  documentation reconciliation, background release and recoverable installation.

## Design

Separate execution, verification and explicit feedback; keep legacy records
readable but ineligible as promotion evidence. Fingerprint effective request
components without conversation content or credentials. Reports cite validated,
redacted event IDs. Freeze baseline, route, tasks, grants, pricing and verifier
before generation. Compare three trials per case; require no per-case regression,
held-out success and a measured predicted improvement (or >=10% median cost/time
improvement with equivalent verified output). Unknown/incomplete stays pending.

Automatic eligibility: three occurrences across two tasks, five idle minutes,
one cycle at a time, 24-hour cooldown. Budget covers all inference and retries;
resume does not reset spend/time. Dirty/drifted source queues or invalidates the
candidate. Journal source adoption and deployment separately. Roll back binaries
recoverably and use source revert only on an unchanged clean branch.

## Validation and acceptance

Failure-first focused tests for false acceptance, mixed identities, fabricated
evidence, reversed trends and source-attribution errors. Cover deterministic
fake-provider discovery→evaluation→promotion→rollback, drift, incomplete trials,
provider failures, cancellation, crash recovery, budget limits, and stale
verification. Full default/all-feature workspace tests, format, strict Clippy,
release-readiness and release builds; deterministic TUI and terminal checks.
Real VM lifecycle validation uses a deterministic local provider. Official
pricing is refreshed separately; no paid-provider efficacy claim is made.

## Recovery

Never reset/stash unrelated user work. Revert only this task's changes using the
recorded baseline if needed. Keep source/binary recovery receipts and restore the
previous installed artifact on installation verification failure.

## Historical progress and discoveries

- Read source, project controls, nested controls and verification contracts.
- Captured pre-edit source baseline; baseline all-feature check passed.
- Failure-first completion/acceptance and chronological-rate tests both failed,
  then passed after the fixes (`/tmp/ca-hrs253-before.log`,
  `/tmp/ca-hrs253-focused.log`).
- Implemented outcome v2 with task IDs, execution/feedback separation, input
  epochs, redacted event IDs, bounded async storage, and explicit TUI/CLI
  feedback. Headless and service paths now record execution evidence.
- Analyze validates exact supplied references and distinct-task counts; the
  timestamp manifest verifier was removed. Cohort stability uses failure rates.
- Added a codex-team frozen experiment core: alternating receipts, complete
  three-repeat evidence, regression/held-out/improvement decisions, durable
  reservations and pause/resume. Five engine tests pass, including persistence
  across pause/resume (`/tmp/ca-hrs253-engine-tests2.log`).
- Added status/candidates/show/pause/resume/feedback controls and observation-only
  execution hints. No automatic runner or activation has been enabled.
- All-feature workspace check passed (`/tmp/ca-hrs253-check4.log`). Initial full
  all-feature tests exposed two HRS-253 compatibility updates (the workflow
  event consumer and command-discovery count); both were repaired. Remaining
  failures include stale tool-count expectations, Git fixture publication and
  codex-team CLI/protocol mismatches. Preserve the failed run in
  `/tmp/ca-hrs253-all-tests.log`; do not label it passing.
- Strict Clippy remains blocked by existing renderer dead code, an unused
  learning test binding, and ui_settings' derivable default implementation
  (`/tmp/ca-hrs253-clippy2.log`). Whole-workspace formatting also has pre-existing
  worktree-manager/event/renderer differences. Task-owned code is formatted.

## 2026-09-23 continuation: source transaction

- Added `crates/codex-team/src/self_improvement/harness_source.rs`: host-protected
  source paths, evaluation recheck before adoption, repository-bound journal,
  existing clean-source inspection and fast-forward, crash resumption, and an
  additive Git revert that defers on dirty source or a later commit.
- SourceAdopting/SourceAdopted are separate from Promoted. A source revert
  requires separate binary recovery; neither operation claims binary activation.
  Pause arriving during an atomic source transition is preserved for the next step.
- Frozen plan creation now refuses to overwrite a different plan after a crash;
  evaluation decisions cannot reset a source-adoption or terminal stage.
- Final combined experiment/source tests: 9 passed, 0 failed
  (`/tmp/ca-hrs253-source-bound-tests.log`). Final all-feature workspace check:
  passed with the two unchanged renderer warnings
  (`/tmp/ca-hrs253-source-check.log`).
- Tests mutated disposable Git repositories only. The user's source branch,
  runtime configuration and installed binary were not changed by adoption APIs.

## 2026-09-23 continuation: isolated coding worker

- Added `src/harness_worker.rs`: bounded framed IPC, a credential-free internal
  launch path, fixed coding tool grants and the actual headless tool loop. The
  worker does not load runtime configuration or provider credentials.
- Added `src/harness_execution.rs`: required offline process sandbox, cleared
  environment, per-trial temporary HOME, explicit read-only executable grant,
  process-group cleanup and an overall deadline. Parent-side parsing rejects
  ungranted/duplicate tools and malformed framing. Only the injected host backend
  performs model requests. The paid budget broker is still a required integration.
- Eleven evidence/protocol tests passed (`/tmp/ca-hrs253-worker-tests.log`), and
  the debug worker binary built (`/tmp/ca-hrs253-worker-build.log`). Runtime
  runner dead-code warnings remain until the production broker/scheduler is wired.
- Added an explicitly invoked process test using a fake provider and a disposable
  workspace/private sentinel. Its first run caught the missing exact executable
  read grant before any worker launched (`/tmp/ca-hrs253-worker-process-test.log`).
  The grant was fixed. The next probe correctly hit the file-path gate before
  execution; the probe was changed to a shell `cat` to exercise the OS sandbox.
- The real worker process test then passed: three fake-provider calls, an actual
  workspace edit, and no private sentinel in model-visible output
  (`/tmp/ca-hrs253-worker-process-test4.log`). The strengthened final test also
  passed and verifies the explicit OS permission-denial output:
  `/tmp/ca-hrs253-worker-process-final.log` (1 passed, 0 failed).
- Worker completion now carries a bounded redacted failure reason for diagnosing
  failures without persisting request bodies. On host errors/deadline exhaustion,
  IPC closes and SIGTERM permits owned-job cleanup before forced termination.
  Forced descendant cleanup and cancellation tests remain required before shipping
  automatic execution. No paid provider call or runtime activation occurred.

## 2026-09-23 continuation: durable provider budget broker

- Added `src/harness_budget.rs`: frozen route/options/token bounds and verified
  pricing, conservative exact-rational maximum charge, durable reservations
  before provider I/O, single-attempt transport, bounded response collection,
  deadline enforcement and settlement from provider-reported usage only.
  Cache writes, reasoning output, long-context tariffs and subscription
  allocation are included in the bound. Missing/stale pricing fails closed.
- Active-time reservations now survive process interruption. Work IDs cannot be
  reused; outstanding work blocks decisions and source adoption. Completed work
  settles rounded-up elapsed time; cancellation retains both reservations.
  Request rejection records budget exhaustion without phantom reservations.
- AgentClient has opt-in output caps for Chat Completions, Responses and Anthropic
  requests. Existing tighter adapter caps are preserved; ordinary requests retain
  their prior defaults. No uncapped retry fallback is introduced.
- Request telemetry now includes the actual protocol/endpoint digest. Fixed the
  main fallback event using the primary prepared request instead of the fallback.
  Endpoints and credentials are not persisted in component snapshots.
- Eleven engine/source tests passed (`/tmp/ca-hrs253-budget-engine-tests.log`).
  Eighteen focused ca tests passed, one explicitly invoked process test skipped
  (`/tmp/ca-hrs253-budget-final-tests.log`), including an actual local HTTP server
  proving output caps on the wire and separate durable reservations for two
  physical retry attempts. These tests use synthetic credentials and no paid API.
- Final route/evidence/broker tests also passed: 18 passed, one explicitly invoked
  process test skipped (`/tmp/ca-hrs253-budget-route-tests.log`). All-feature
  workspace check passed (`/tmp/ca-hrs253-budget-final-check.log`), with unwired
  broker/runner and existing renderer warnings. Task-owned format and diff checks
  passed. The fallback regression suite also passed: 19 tests
  (`/tmp/ca-hrs253-route-fallback-tests.log`).
- The broker still needs production scheduler/fixture integration. Whole-cycle
  active time must also cover generation, tool execution and engineering checks;
  request timing alone is not the complete two-hour cycle limit. Source/binary
  promotion, live validation and installation remain incomplete.

## 2026-09-23 continuation: whole-activity time and frozen coding inputs

- Added `src/harness_activity.rs`: a durable time reservation spanning a complete
  asynchronous host operation, including tools and verification. Provider calls
  borrow that reservation rather than counting model time twice. Deadline expiry
  consumes the allowance; cancellation closes admission and retains unknown time.
- Added `src/harness_cases.rs`: bounded immutable fixture contents, exact
  materialization, allowed-edit checks, compiled verifier identity, and a nine-case
  production suite requirement (target, two held-out variants, six regressions).
  Held-out implementations must differ from disclosed inputs and match the target
  verifier family. This validates structure; selecting meaningful variants still
  belongs to the production freezer and must be demonstrated by execution.
- Added a descriptor-relative bounded snapshot reader to the existing workspace
  materializer. Candidate symlinks/special files/hard links and changes outside
  permitted source paths are rejected before host verification.
- Added `src/harness_verifier.rs` and an internal launch path. The baseline host
  binary runs public and hidden checks in a separate offline process, binds its
  receipt to the frozen verifier and candidate source, and enforces a deadline.
  The coding worker cannot provide a verification result through its protocol.
- Focused activity/case/evidence tests passed: 24 passed, one explicit process test
  skipped (`/tmp/ca-hrs253-cases-final-tests.log`). All-feature compilation passed
  (`/tmp/ca-hrs253-verifier-check.log`) with unwired integration warnings. The
  existing workspace-materialization suite passed (22 tests;
  `/tmp/ca-hrs253-materialization-tests.log`).
- The real verifier test initially failed before returning a receipt. A bounded
  infrastructure-failure frame identified that opening all ancestors of the trial
  directory exceeded sandbox grants (`/tmp/ca-hrs253-verifier-process-test2.log`).
  The snapshot reader now pins the host-owned root directly, retains no-follow
  descriptor-relative traversal within it, and does not widen sandbox grants.
- Subsequent real execution exposed nested sandbox installation being denied by
  macOS. The trusted baseline verifier now applies the required closed profile
  once to each compiler/probe subprocess. The pre-existing reasoning verifier
  exposes a host-only profile callback and retains bounded compiler diagnostics;
  its default entry point keeps its existing profile behavior.
- Cargo then reported denied parallelism discovery. A standalone local C probe
  confirmed `sysconf(_SC_NPROCESSORS_ONLN)` requires exactly `hw.ncpu` (the other
  tested CPU sysctls did not satisfy it). Added that non-secret CPU-count read to
  the closed profile, retaining denial of other process inspection. The real
  verifier test is being rerun; no behavioral pass is inferred from compilation.
- The standalone Unicode process test passed after those fixes
  (`/tmp/ca-hrs253-verifier-process-test6.log`). Expanding to all six fixtures found
  that the process-cleanup golden repair depended on an out-of-scope main.rs edit.
  Fixed the golden library to own and terminate its child process group through
  the existing API, making the repair valid within the declared lib.rs-only grant.
- Allowed signals only to targets in the same sandbox for owned child cleanup.
  The host's PID-evidence parser now rejects broadcast/invalid/duplicate IDs,
  reads at most 4 KiB from a no-follow regular non-hardlinked file, and checks
  kernel process-group ownership before signaling a surviving descendant.
- Final combined evidence/activity/fixture/process run: 27 passed, 0 failed,
  0 ignored (`/tmp/ca-hrs253-final-process-and-focused.log`). This explicitly runs
  the private-file-denial coding worker test and all six buggy/repaired fixture
  pairs through the separate host verifier. No fixture helper processes remained
  afterward. Task-owned formatting and diff checks passed. Final all-feature
  workspace check passed (`/tmp/ca-hrs253-activity-cases-final-check.log`) with
  unwired integration and pre-existing renderer warnings.
- Next: connect trial materialization, the activity-scoped broker, worker and
  baseline verifier into receipt admission; then the candidate generator and
  idle lifecycle. Whole-activity accounting is now available but not yet applied
  across production cycles. No runtime activation or paid provider run occurred.

## 2026-09-23 continuation: journaled trial runner

- Added `src/harness_trials.rs`. It chooses the next frozen baseline/candidate
  trial, holds the worker lock, pins digest-checked executables, materializes the
  case, runs the budgeted coding worker, and verifies through the baseline host.
  Guidance trials use the baseline executable. Source trials bind their binary
  and commit identity to the cycle's candidate.
- Each trial has a durable attempt journal. Completed journals can be admitted
  after a crash without another provider call. Unfinished journals stop in
  RecoveryRequired and retain spend/time reservations. Recovery checks the saved
  verifier artifact and implementation bytes before admitting a successful result.
- Receipts arriving after pause or budget exhaustion preserve that state. A
  foreground pause cannot be erased by a completed receipt. Future orchestration
  still needs to define and enforce foreground handoff between trial phases.
- Worker Cargo output now stays in its private temporary directory. The cycle
  clock includes executable pinning and fixture setup; comparative latency starts
  after setup and includes model/tool execution and host verification.
- Final focused run: 30 passed, 0 failed, 0 ignored, including three explicitly
  enabled process tests (`/tmp/ca-hrs253-trial-final-tests.log`). The integrated
  test makes two synthetic HTTP calls, edits real code, passes host checks, saves
  source evidence, and recovers the same receipt without new calls. It also
  rejects a missing verification artifact. No paid API was used.
- Engine/source suite: 12 passed (`/tmp/ca-hrs253-trial-admission-tests.log`).
  All-feature workspace check passed (`/tmp/ca-hrs253-trials-final-check.log`) with
  unwired integration and existing renderer warnings. Task-owned formatting and
  diff checks passed.
- Next: production case selection and candidate generation, engineering gates,
  idle scheduling and CLI run/rollback, then activation/recovery and full checks.
  The trial runner is connected internally but has no production command caller.

## 2026-09-23 continuation: budgeted guidance generation

- Added `src/harness_generation.rs`: frozen provider/case/evidence inputs,
  one budgeted guidance request, bounded typed proposal, exact citation checks,
  root-cause hypothesis and prediction. Only the target is disclosed to generation;
  held-out implementations remain outside its prompt. Source generation remains
  unimplemented.
- A durable generation journal precedes provider I/O. The saved proposal, inputs
  and resulting trial harness have digest checks. Admission is repeatable after
  a crash without another request; unfinished attempts retain reservations and
  require recovery. Generation cannot grant engineering approval or activation.
- Candidate admission preserves foreground pause and settled spend/time. The
  existing `harness show` now exposes the generation prediction and a JSON report,
  omitting frozen held-out implementation bytes. The generation function still
  needs the production cycle controller as its caller.
- Four generation tests passed, including actual local HTTP with synthetic
  credentials and one physical request, durable pre-request reservations, and
  crash recovery after the proposal write without another POST:
  `/tmp/ca-hrs253-generation-final-tests.log`. No paid provider was called.
- The first compile caught a `ModelContent` construction mismatch; fixed with
  its existing text constructor. Failed compile evidence remains in
  `/tmp/ca-hrs253-generation-check.log`.
- Task-owned formatting and `git diff --check` passed. Broader focused run:
  31 passed, 0 failed, 3 explicitly enabled process tests skipped
  (`/tmp/ca-hrs253-generation-focused.log`). All-feature workspace compilation
  passed with unwired integration and existing warnings
  (`/tmp/ca-hrs253-generation-final-check.log`). Earlier process-test results
  remain recorded separately; they were not rerun for this addition.

## 2026-09-23 continuation: saved-candidate evaluation command

- Added `src/harness_controller.rs` and the asynchronous production command
  `ca harness run CYCLE_ID --repository PATH --baseline-binary PATH [--json]`.
  It loads the sealed guidance generation, uses the existing configured route
  through read-only configuration loading, and drives alternating frozen trials.
  It never creates a candidate from arbitrary CLI JSON or activates a candidate.
- The controller rechecks durable state between trials, stops on pause, budget
  exhaustion or infrastructure failure, and leaves completed evaluations waiting
  for engineering checks rather than rejecting them merely because checks have
  not run. Engineering approval still requires its production implementation.
- Added explicit recovery of a saved proposal before candidate admission, without
  provider I/O. Changed/unfinished generation cannot be run. The command refuses
  to implicitly resume paused or terminal cycles. `harness help` includes the
  exact command and required binary/repository arguments.
- Focused suite: 33 passed, 0 failed, 3 opt-in process tests skipped
  (`/tmp/ca-hrs253-controller-combined-tests.log`). This includes generation
  recovery followed by a paused controller with no additional request or trial,
  and controller/argument admission tests. All-feature compilation passed
  (`/tmp/ca-hrs253-controller-check.log`).
- Debug build passed (`/tmp/ca-hrs253-controller-build.log`). Actual CLI smoke
  confirmed help advertises the exact run arguments and incomplete arguments exit
  1 before configuration/provider I/O. Task-owned format, main.rs format without
  traversing unrelated modules, and `git diff --check` passed.
- Production source candidate generation, new-cycle evidence/fixture freezing,
  idle admission, engineering checks and activation remain incomplete. This
  command currently evaluates existing guidance cycles; no full lifecycle,
  automatic activation, paid provider call or release installation is claimed.

## 2026-09-23 continuation: complete build input fingerprint

- Engineering-gate inspection found that `build.rs` hashed only Rust files under
  src/crates and the root manifest/lock. Embedded prompt/schema changes, member
  manifests, hidden verifier code, feature flags and compiler settings could
  therefore retain an old Build component identity. Exact executable digests
  already protected trial pinning, but runtime cohort attribution was incomplete.
- Added `build_support/harness_identity.rs` and updated build.rs to include
  explicit source/assets/manifests, hidden verifier input, optional Cargo/toolchain
  configuration, selected Cargo build settings and `rustc -vV`. Field lengths and
  record types delimit inputs. Paths are relative, directory traversal is sorted,
  and symlinks/special inputs are rejected. Runtime state, credentials, repository
  history and generated build directories are outside the input inventory.
- The helper is a protected source path. Existing input-directory watches cover
  added/deleted assets; absent optional paths and build output directories are
  not watched, avoiding perpetual rebuilds.
- Three dedicated tests passed (`/tmp/ca-hrs253-build-identity-final-tests.log`).
  They exercise embedded assets, member manifests, hidden verifier, compiler,
  features/settings, path independence, ignored runtime/output changes, added
  files and symlink rejection. An unchanged repeat finished in 0.26 seconds
  without rebuilding (`/tmp/ca-hrs253-build-identity-unchanged.log`).
- All-feature workspace check passed (`/tmp/ca-hrs253-build-identity-check.log`),
  with the existing/unwired warnings. Task-owned formatting and diff checks pass.
  The full focused run passed 36 tests, 0 failed and 0 ignored, explicitly
  including the private-file-denial worker, six buggy/repaired verifier pairs and
  journaled trial recovery (`/tmp/ca-hrs253-build-identity-process-tests.log`).
- Production engineering gates remain next. Existing
  `crates/codex-team/src/release_readiness.rs` provides a fixed validation matrix
  and bounded process-group cleanup, but is synchronous and inherits environment;
  automatic candidate checks need a credential-free, budgeted, cancellable host
  adapter. Do not put its blocking audit directly inside an activity future.

## 2026-09-23 continuation: production guidance engineering gates

- Added `src/harness_engineering.rs` and exposed the existing fixed release
  matrix through `fixed_release_gates`. The guidance evaluation controller now
  runs that matrix before recording a decision, including when a prior boolean
  says checks passed. Engineering failure cannot authorize promotion.
- Checks use a clean, snapshot-matched private clone, required offline sandbox,
  cleared environment, private HOME/Cargo configuration, read-only dependency
  caches and source, and writable build/temporary directories. The source
  checkout is not modified. Candidate text never supplies commands or pass flags.
- Checkout, each gate, source reinspection and artifact hashing reserve active
  time before work. Git inspection uses bounded asynchronous processes with
  disabled hooks/fsmonitor and no inherited user Git configuration. Interrupted
  journals retain reservations and stop recovery; completed gates are reused
  after source/artifact reinspection, whose time is charged separately.
- Gate receipts retain command-specification and output digests, bounded filtered
  failure diagnostics, and the resulting release artifact digest. A changed or
  symlinked artifact is rejected. Before reinspection, prior engineering approval
  is cleared so an error cannot retain an old pass. Guidance trials continue
  using the separately pinned baseline executable.
- `harness show` now reports engineering counts and JSON gate receipts. Pause and
  budget states stop subsequent gates; the controller records rejection after a
  completed failing matrix and can record eligibility after passing checks and
  the full matched trial requirements. It still does not activate a candidate.
- Five engineering tests passed, explicitly including the full fixed matrix in
  a disposable two-binary Rust project, saved formatting failure without replay,
  changed/symlinked artifact rejection, durable exhaustion/pause preservation,
  and real timeout/cancellation termination of an owned child
  (`/tmp/ca-hrs253-engineering-admission-tests.log`). No paid provider was used.
- This is guidance integration. Source-candidate generation/engineering binding,
  forced cleanup of descendants that deliberately change process groups, new-cycle
  freezing, idle admission and activation still require completion. Passing the
  disposable fixture matrix is not a claim that the ca workspace release matrix
  passes. Final combined HRS-253 run: 41 passed, 0 failed, 0 ignored,
  including all five process tests (`/tmp/ca-hrs253-engineering-combined-tests.log`).
  All-feature workspace check passed with seven existing/unwired warnings
  (`/tmp/ca-hrs253-engineering-workspace-check.log`); debug build passed
  (`/tmp/ca-hrs253-engineering-build.log`). Task-owned format, main.rs format
  without unrelated module traversal, and diff checks passed.

## 2026-09-23 continuation: explicit evidenced cycle creation

- Added `src/harness_freeze.rs` and `ca harness create`. Creation accepts an
  explicit target family/directory, two held-out directories, baseline instruction
  file and evidence IDs. It freezes the six regressions plus target/held-outs,
  provider route/options/pricing, source snapshot and running executable digest.
  Creation makes no model request; `harness run` now generates the proposal for
  a newly frozen cycle and continues the existing evaluation/engineering path.
- Evidence selection requires 3–64 distinct failed tool outputs across at least
  two completed/interrupted/failed tasks in one runtime cohort. Each output must
  have a matching failed execution receipt and unique task/call identity. Legacy,
  incomplete, mixed-epoch, denied, duplicated and missing evidence is excluded.
  The selected provider route/options/build and verified price card must match
  the evidence. Failure excerpts are re-filtered and bounded to 1 KiB.
- Creation requires clean source and matching compiled source inputs. Build.rs
  now emits `CA_SOURCE_DIGEST` using the same build-input inventory without
  compiler settings, allowing the host to verify source separately from its exact
  executable. This prevents evaluating an unrelated clean repository as if it
  produced the running baseline. The freezer is a protected host source path.
- Frozen inputs are stored before publishing the cycle and checked on every
  load. Source case changes after creation cannot replace them. A second active
  cycle is refused. `run` checks the binary (and source before generation) before
  resolving the configured route, and preserves completed proposal recovery.
- Two freezer tests pass, including fixture materialization into a disposable
  Git repository, creation/load, held-out file mutation after freezing, concurrent
  cycle refusal, input tampering and evidence rejection cases
  (`/tmp/ca-hrs253-freeze-final-tests.log`). The initial test compile caught an
  incorrect `set_model` test call; fixed without changing its runtime API.
- This is explicit creation, not automatic discovery. Idle selection of targets
  and held-outs, guidance operation on a dirty development checkout, source
  candidates, and activation still need integration. No cycle was created in
  the user's machine vault and no paid provider call occurred during these tests.
  Final combined run: 43 passed, 0 failed, 0 ignored, including all five opt-in
  process tests (`/tmp/ca-hrs253-freeze-combined-tests.log`). All-feature workspace
  check passed (`/tmp/ca-hrs253-freeze-final-check.log`) with existing warnings;
  debug build passed (`/tmp/ca-hrs253-freeze-final-build.log`). Actual CLI smoke
  verified `harness create --help` and missing-input rejection before configuration
  or model I/O. Task-owned format, main.rs format and diff checks passed.

## Next implementation work

M1 has delegated ancestry, scoped host verification and asynchronous shutdown
handling. M2/M3 have explicit frozen cycles, guidance/source generation, private
commits/builds, matched coding trials and production source/binary promotion with
recovery and rollback. Automatic target/held-out/source selection is connected
through explicit `create --auto` and a default-disabled TUI idle supervisor. Complete
idle lifecycle/end-to-end validation and measured hint interventions. Dirty-checkout guidance
freezing and guidance activation/rollback are connected and tested separately. Finish escaped-descendant cleanup and audit the
wire input-size bound behind provider spend reservations before any live cycle.
Verify the real ca engineering matrix under its closed profile rather than
extrapolating from disposable fixtures. Run complete discovery-to-rollback tests,
full gates and terminal checks, then a bounded live cycle when source/route
prerequisites permit it. Preserve the dirty checkout. Release/install remains
pending validation.

## Validation checkpoint (not completion)

- Final all-feature workspace check passes with the two unchanged renderer warnings: `/tmp/ca-hrs253-check-final.log`.
- Nine HRS-253 focused tests pass on the final evidence changes:
  `/tmp/ca-hrs253-focused-final2.log`.
- Five experiment-engine tests pass:
  `/tmp/ca-hrs253-engine-tests2.log`.
- Final default ca suite: 2,520 passed, one failed, five ignored. The remaining
  test expects 46 tools but the current pre-existing registry advertises 47:
  `/tmp/ca-hrs253-ca-final2-tests.log`. The workflow event and menu snapshot
  regressions from this task are repaired and pass this run.
- Initial full all-feature workspace run: 3,306 passed, 28 failed, 7 ignored
  across all reported targets (`/tmp/ca-hrs253-all-tests.log`). This includes
  the subsequently repaired two HRS-253 compatibility failures. No broad pass
  is claimed and failures outside this task were not rewritten.
- `git diff --check` passed. Final formatting check reports only pre-existing
  worktree-manager, TUI event and renderer differences:
  `/tmp/ca-hrs253-fmt-final.log`.
- Built CLI smoke: `target/debug/ca harness status --json` returned schema
  `ca.harness_status.v1` and an empty cycle list, exit 0.
- No end-to-end generation/evaluation/promotion/rollback, live-provider cycle,
  manual terminal walkthrough, release build or installation is claimed.

## Completion record

Incomplete. Keep this plan ACTIVE and continue the integration work above.
No user configuration was enabled and no installed executable was replaced.


## 2026-09-23 continuation: delegated outcome ancestry

- Delegated executions now have child task IDs, immutable parent/root attribution,
  actual prepared-request fingerprints and bounded privacy-filtered tool evidence.
  Nested children inherit the original root even after a new foreground turn.
  Both TUI and headless runs bind the parent at task admission. Unknown-parent
  executions do not fabricate independent evidence.
- Analyze and explicit cycle freezing count independent root tasks. Siblings from
  one request cannot satisfy the two-task threshold. Legacy records without the
  optional ancestry fields retain their original task identity.
- Manager cancellation and timeout drain queued evidence after aborting the runner.
  Cancellation winning a race with the runner records interruption rather than
  failure. Normal child completion remains unverified with unknown user feedback.
- Delegated manager tests: 29 passed (`/tmp/ca-hrs253-ancestry-manager-tests.log`).
  Evidence suite: 39 passed, five opt-in process tests skipped
  (`/tmp/ca-hrs253-ancestry-combined-tests.log`). All-feature workspace check passed
  with existing warnings (`/tmp/ca-hrs253-ancestry-final-check.log`). Final focused
  reruns passed: 39 evidence tests and 29 manager tests, recorded in
  `/tmp/ca-hrs253-ancestry-final-tests.log` and
  `/tmp/ca-hrs253-ancestry-final-manager-tests.log`.
- No paid provider call, automatic activation, source adoption or installed-binary
  change. Host verification outcomes, complete shutdown flushing, automatic case
  selection, source generation, idle orchestration, promotion/recovery, execution
  hints, full validation and installation remain required.


## 2026-09-23 continuation: scoped host verification

- TUI and headless automatic host verification now record status, focused scope,
  receipt digest, revision and bounded recipe/reason references. Missing receipt,
  missing recipe selection or stale-revision evidence cannot record a valid pass.
  Unchanged results do not revive invalidated checks.
- Model-written headless check/convergence reports are separate evidence events;
  they cannot set host verification status. Focused checks and legacy unscoped
  passes do not establish whole-task success. Explicit user feedback remains
  independent. No task-contract coverage is invented for focused recipes.
- Central tool-result recording invalidates previous checks for potentially
  mutating tools, including failed shell commands and hashline edits. Denied and
  read-only calls preserve the status. Known mutation receipts and undo also
  invalidate checks. Removed narrower duplicate invalidation from callers.
- An initial focused run caught a test fixture using the wrong report field
  (`findings` instead of `issues`); retained at
  `/tmp/ca-hrs253-host-verification-tests.log`. The fixture was corrected before
  the final rerun. Final evidence suite: 41 passed, five opt-in process tests
  skipped (`/tmp/ca-hrs253-host-verification-final-tests.log`). Complete headless
  suite: 41 passed (`/tmp/ca-hrs253-host-verification-headless-tests.log`).
  Task-owned formatting and diff checks passed. All-feature workspace check passed
  with existing warnings (`/tmp/ca-hrs253-host-verification-final-check.log`).
- Full completion still requires shutdown flushing, automatic discovery/source
  generation, idle orchestration, source/binary activation and rollback, measured
  hints, full validation, a bounded live cycle and release installation.


## 2026-09-23 continuation: typed source proposal generation

- Added `src/harness_source_generation.rs`: a frozen source-input bundle, cited
  hypothesis/prediction, and exact substring edits tied to original file digests.
  The host rejects protected or unfrozen paths, ambiguous overlapping matches,
  no-op/net-zero edits, fabricated citations and oversized proposals. No model
  command is executed. Generation sees only the target, never held-out code.
- Source freezing checks the clean repository against the cycle before and after
  descriptor-relative reads of at most eight regular files (256 KiB total).
  Inspection consumes the durable activity budget. Symlinks, hard links, path
  escapes, dirty source and oversized reads fail closed.
- A journal is written before the single budgeted generation POST. Completed
  proposals recover without network I/O; incomplete proposals preserve charges
  and require recovery. Guidance/source attempts are mutually exclusive per
  cycle. A proposal never sets candidate identity or engineering approval.
- Read-only `harness show` exposes the proposed edits and prediction without the
  frozen held-out bundle. The generation API still needs the production source
  build/controller entry point; its new unused-code warnings are temporary,
  not evidence of a completed source workflow.
- Focused suite passed 45 tests with five opt-in process tests skipped
  (`/tmp/ca-hrs253-source-generation-final-tests.log`). Final rerun, including
  read-only report and overlapping-match assertions, also passed 45 tests
  (`/tmp/ca-hrs253-source-generation-final3-tests.log`). The intervening rerun
  caught a test-only digest argument type error; it was corrected. Task-owned
  formatting and diff checks passed. All-feature workspace check passed with
  existing and not-yet-connected source-runner warnings
  (`/tmp/ca-hrs253-source-generation-final-check.log`). Engine/source transaction
  tests also passed: 12 (`/tmp/ca-hrs253-source-generation-engine-tests.log`).
- Next source step: journal a private checkout/commit from the frozen proposal,
  build it with budgeted fixed host gates, bind the resulting executable and
  commit to TrialHarness, and pass that artifact through evaluation. Then connect
  automatic selection/scheduling and source/binary activation/recovery. No user
  repository commit, paid provider call or installation occurred.


## 2026-09-23 continuation: private source commit, build and evaluation binding

- Added `src/harness_source_checkout.rs`: budgeted private clone, exact preimage
  replacement through pinned regular-file descriptors, host-authored one-commit
  candidate and durable checkout journal. The original repository is checked
  before/after and remains untouched. Recovery rechecks ancestry, changed paths,
  exact proposed bytes and clean source; interrupted work is not replayed.
- Added `src/harness_source_build.rs`: the complete fixed engineering matrix in
  the candidate checkout, with per-gate start/finish journals and time charges.
  Only complete passing gates plus the exact release executable and commit can
  admit a TrialHarness. Failed checks reject the cycle; interruption/artifact
  drift requires recovery. Completed receipts are reused after source/artifact
  reinspection, without rerunning successful gates.
- `harness run` can now prepare/build a saved source proposal and evaluate its
  candidate executable. The controller chooses guidance/source artifacts and
  rechecks source engineering evidence before deciding. Read-only `show` includes
  source checkout and build reports. Source freezing/generation still needs its
  production create/automatic-selection entry point.
- Initial source tests: nine passed, including both explicitly enabled sandbox
  build tests (`/tmp/ca-hrs253-source-build-tests.log`). Those tests ran the real
  matrix in disposable projects, verified failure rejection, unchanged original
  source, exact artifact admission, receipt reuse and artifact-tamper rejection.
  Final combined process validation passed all 56 tests, with all seven process
  tests explicitly enabled (`/tmp/ca-hrs253-source-build-combined-final-tests.log`).
  Debug build passed (`/tmp/ca-hrs253-source-build-debug-final.log`). An earlier
  combined compile caught a stale test reference after the controller rename; it
  was corrected. Task-owned formatting and diff checks passed. All-feature
  workspace check passed with six existing/unwired-entry warnings
  (`/tmp/ca-hrs253-source-build-final-check.log`). No process-fixture helper
  remained after the combined run.
- Engineering process coverage uses minimal disposable Cargo projects. The full
  ca engineering matrix remains unverified under the closed profile, especially
  tests that use loopback servers or nested sandboxes; fixture success is not
  evidence that the production ca matrix passes.
- No user-branch adoption, paid provider request or installed executable change.
  Next: source input selection/create integration, automatic idle orchestration,
  signed activation/rollback, hint evaluation, full validation and installation.


## 2026-09-23 continuation: source creation and frozen candidate type

- `harness create --source-file PATH` now freezes a source experiment, with up to
  eight unprotected repository-relative files and 256 KiB total source. Omitting
  source files keeps guidance behavior. Creation stores exact source bytes with
  the frozen inputs and per-file digests in the sealed plan, before any model
  request. `run` rechecks source, generates the proposal, privately commits and
  builds it, and evaluates the bound candidate executable.
- Guidance/source types cannot be substituted after creation. Source generation
  requires exactly the frozen file identities, and TrialHarness validates the
  presence/format of a source commit against the plan type. Incomplete source
  generation enters recovery without replaying a provider request.
- Empty source selection is omitted from serialization, preserving legacy
  guidance plan digests. Tests cover that compatibility, sealed selection changes,
  protected paths, source-input tampering and repeated CLI file selections.
- Focused suite: 50 passed, seven process tests skipped
  (`/tmp/ca-hrs253-source-create-final-tests.log`). Engine/source suite: 13 passed
  (`/tmp/ca-hrs253-source-create-engine-tests.log`). Task-owned format and diff
  checks passed. Debug build and actual `harness create --help` smoke passed; the help exposes repeated source-file selection before configuration loading. All-feature workspace check passed with existing warnings (`/tmp/ca-hrs253-source-create-final-check.log`).
- No paid request, automatic scheduling, source adoption or installed-binary
  replacement. Automatic selection, dirty-source guidance support, idle lifecycle,
  activation/rollback, measured hints, full validation and installation remain.


## 2026-09-23 continuation: outcome drain and cancellation reliability

- Outcome flush now waits for capacity under a fixed deadline instead of silently
  skipping a full queue. Storage failures, dropped writes, disconnected writers
  and missed deadlines report failure. The sink retains a loss flag; subsequent
  evidence is incomplete and cannot establish verified success or enter stability
  cohorts. This reports loss honestly; it does not recover dropped events.
- Headless emitters own a scoped outcome, so broken pipes and cancelled futures
  that bypass normal finish retain interruption. Normal headless completion drains
  off the async runtime, and the production owner drains again after delegated
  agents stop. TUI shutdown uses the same asynchronous drain.
- Service terminal events await the turn's final destructor and drain its writer
  before acknowledging completion/failure/cancellation. Session shutdown also
  drains, covering EOF and transport-error cleanup.
- Regression coverage includes queue saturation, writer disconnect, deadline,
  storage failure, overflow, cancellation, headless early drop and a flush barrier
  whose acknowledgement requires the single-thread runtime to remain responsive.
- Final combined validation: 62 passed, zero failed or ignored, explicitly
  including all seven process tests (`/tmp/ca-hrs253-flush-process-tests.log`).
  This includes source creation/build tests against the sealed source-file plan.
  Fresh debug build passed (`/tmp/ca-hrs253-flush-debug.log`). Separate suites:
  41 headless, 25 service, three stability tests passed; the additional headless
  early-drop case also passes in the final combined run. All-feature workspace
  check passed with existing warnings (`/tmp/ca-hrs253-flush-workspace-check.log`).
  Task-owned formatting, diff checks and unchanged oracle/reference status pass.
  No paid request, user-branch mutation or installed-binary replacement occurred.
- Automatic discovery, idle orchestration, activation/rollback, measured hints,
  full production engineering validation and release installation remain open.


## 2026-09-23 continuation: verified source object import

- Production evaluation now imports a source candidate's exact commit objects
  after a promotion decision. `run` can recheck this stage without resolving a
  provider or making another request. The current branch, refs, index, working
  files, FETCH_HEAD and installed executable stay unchanged during import.
- Admission rechecks complete passing engineering receipts, the exact executable,
  frozen trial verdict, source proposal, private checkout identity and unchanged
  original baseline. Transfer uses an explicit local path, disables other Git
  protocols, hooks, recursive submodules, tags and automatic maintenance. Host
  Git inspection also ignores replacement objects.
- A receipt binds plan, checkout, candidate, trial evidence and repository.
  Recovery rechecks those bindings and the imported object without another fetch.
  Unknown activity charges prevent recovery from resetting the budget. Imports
  reserve and settle active time; provider calls remain forbidden after decision.
- Initial validation: one full-matrix disposable source/import test and 14 core
  experiment/source tests pass. The ordinary focused suite passes 55 tests with
  eight opt-in process tests skipped. Adding dirty-source and unresolved-charge
  checks exposed a stack overflow from repeatedly embedding the large import
  future (`/tmp/ca-hrs253-source-import-final-test.log`). The production import
  entry point now boxes its internal operation; the test stack limit is unchanged.
  The unchanged-stack rerun passed (`/tmp/ca-hrs253-source-import-boxed-test.log`).
  Final combined run: 63 passed, zero failed or ignored, including all eight
  process tests (`/tmp/ca-hrs253-source-import-combined-tests.log`). Fresh debug
  build and all-feature workspace check pass with existing warnings
  (`/tmp/ca-hrs253-source-import-final-debug.log`,
  `/tmp/ca-hrs253-source-import-final-check.log`). Task-owned formatting and
  diff checks pass. No user-branch adoption, live provider call or installation.
- This completes object preparation, not branch adoption or binary activation.
  Production activation must stage signed recovery artifacts, then call source
  adoption and verified binary replacement with a journal spanning both steps.
  Automatic selection/idle scheduling, rollback and measured hints remain open.


### Promotion integration constraint

The production transaction now uses the bounded asynchronous source-checkout
Git adapter. The older `harness_source.rs` adoption/revert adapter still calls synchronous
Git helpers from `self_improvement/source.rs`; they inherit environment and do
not have the asynchronous deadline/process owner used by the new importer.
Do not wrap those subprocess operations in an activity's `spawn_blocking` future
and assume cancellation stops them. Split transaction validation/journaling from
bounded asynchronous Git mutation, preserving clean unchanged-branch checks and
crash recovery. Also account for the promotion activity's own reservation without
allowing unrelated unknown charges to authorize a source/binary transition.


## 2026-09-23 continuation: frozen activation destination and signed recovery artifacts

- New plans seal the exact canonical baseline executable path before generation.
  The optional field is omitted for legacy plans, preserving their digests;
  legacy plans without a destination cannot replace an unspecified executable.
- Added protected `src/harness_activation.rs`. After verified source import,
  production evaluation prepares a private directory on the destination filesystem,
  preserves the exact baseline, copies the evaluated candidate, signs that copy
  ad hoc, verifies both signatures and runs offline credential-free version/help
  checks. The evaluated artifact remains unchanged. No branch or executable
  replacement happens during preparation.
- The journal binds plan, candidate, trial digest, destination/mode, evaluated and
  signed bytes, recovery copy and smoke output digests. Inspection/copy/signing/
  smoke work reserves active time. Completed artifacts are rechecked on recovery;
  incomplete journals, changed binaries or failed signatures require recovery.
  `harness show --json` exposes the activation artifacts; ready source `run`
  rechecks them without provider configuration or a model request.
- Tests exposed non-private default temporary-directory permissions; preparation
  now sets 0700 before writing any artifact. The corrected staging test passes
  with real signing and offline binary launches in a disposable project
  (`/tmp/ca-hrs253-activation-private-stage-test.log`). Tests also cover drift,
  symlink/hard-link and unsafe-mode rejection, frozen path identity, retained
  recovery bytes and unchanged active executable. Core suite: 15 passed
  (`/tmp/ca-hrs253-activation-engine-tests.log`). Combined run: 64 passed, zero
  failed or ignored (`/tmp/ca-hrs253-activation-combined-tests.log`), including all
  eight process tests. The final pre-launch artifact-digest refinement then
  passed its complete staging/import/signature/recovery test separately
  (`/tmp/ca-hrs253-activation-final-staging-test.log`). Final debug build and
  all-feature workspace check pass with existing warnings
  (`/tmp/ca-hrs253-activation-final-debug.log`,
  `/tmp/ca-hrs253-activation-final-check.log`). Task-owned formatting and diff
  checks pass; oracle/reference status is unchanged.
- This is activation preparation. The next step must add bounded asynchronous
  source adoption, atomic signed-executable replacement and post-install checks,
  then rollback of the binary and additive source revert with drift deferral.
  Preserve the baseline and staged artifacts through every crash boundary. Idle
  scheduling, guidance activation, measured hints and full release validation
  remain incomplete. No user installation, branch adoption or paid request ran.


## 2026-09-23 continuation: source and signed executable promotion/rollback

- Added protected `src/harness_promotion.rs`. The production source controller
  now invokes a journaled transaction after import and signed staging. It rechecks
  frozen trial/build/artifact/source bindings, fast-forwards only the unchanged
  clean baseline, copies to a private sibling on the destination filesystem and
  atomically renames after a final inode/ctime/mode identity check. Signature,
  exact hash, mode/owner and offline help/version postchecks precede `Promoted`.
- Promotion retains 180 seconds for rollback in addition to its own allowance.
  Transition work has a separate host-only reservation admission path; it does
  not reopen provider calls in terminal states. Crash recovery can settle only
  this journal's host reservation at its full maximum. Unknown unrelated charges
  remain blocked. Pauses stop the next step rather than restarting work.
- `ca harness rollback ID --repository PATH` restores the verified baseline
  binary before attempting an additive source revert. Dirty/later source is left
  unchanged with the binary restored and source recovery deferred. Changed user
  executable bytes/mode are not overwritten. A failed activation attempts rollback
  automatically. Fully verified recovery records the terminal `RolledBack` stage.
- `run` resumes recorded transitions without provider configuration or model I/O.
  `show --json` includes the promotion journal. Legacy source plans lacking a
  frozen executable destination remain ineligible for binary activation.
- Initial compile caught the vault writer's PathBuf return type; corrected the
  local adapter. Initial transaction tests passed two cases and caught a test-only
  `/var` versus `/private/var` recovery journal mismatch; the fixture now uses the
  same canonical repository identity as production. Four transaction tests then
  passed (`/tmp/ca-hrs253-promotion-final-tests.log`), including real post-install
  failure rollback. Core suite: 16 passed
  (`/tmp/ca-hrs253-promotion-final-engine-tests.log`). Expanded crash coverage
  passed all four transaction tests (`/tmp/ca-hrs253-promotion-recovery-tests.log`),
  including a crash after binary rename with no duplicate replacement. Final
  combined suite: 68 passed, zero failed or ignored, including all 12 process
  tests (`/tmp/ca-hrs253-promotion-combined-tests.log`). Debug build and all-feature
  workspace check passed with existing warnings (`/tmp/ca-hrs253-promotion-debug.log`,
  `/tmp/ca-hrs253-promotion-workspace-check.log`). Actual `harness rollback --help`
  exits zero before vault/provider loading. Task-owned format and diff checks pass;
  oracle/reference status is unchanged.
- These tests build/sign/activate disposable tiny projects and use synthetic
  matched trial receipts to test the host transaction. They are not a claim of
  a complete real-provider discover-to-promote cycle or full ca engineering pass.
  No original-repository adoption, user installation or paid request occurred.
- Remaining: idle discovery/orchestration, dirty-source guidance support,
  guidance activation/rollback, measured hints, escaped-descendant cleanup,
  full validation, bounded live cycle and the standing release/install workflow.


### Continuation — independent provider input-size checks

- Host experiments now reject oversized serialized message/tool/options inputs
  before reserving spend. A bounded writer counts UTF-8 bytes and JSON escaping
  without allocating a second unbounded input copy. Image inputs are rejected:
  the experiment tariff and coding cases currently cover text only.
- The production client also checks the final adapter body before transport for
  Chat Completions, Responses and Anthropic Messages. The byte ceiling equals
  the frozen configured context ceiling; normal interactive clients are unchanged.
  This is a strict resource bound, not proof of a tokenizer or server context limit.
  Provider-specific accounting and opaque continuation-state expansion still need
  validation before a live automatic cycle. A final adapter rejection after host
  reservation conservatively retains that reservation.
- First regression compile caught test construction using unsupported ModelContent
  conversion; corrected to its text constructor. Initial suite: 57 passed, two
  generation tests failed, 12 opt-in tests ignored. The new limit correctly rejected
  their 1,000-byte fixtures, which carry target code and evidence. Generation and
  real-worker fixtures now declare 32,768-byte context ceilings before sealing;
  the small budget arithmetic fixtures and their exact-cost checks are unchanged.
- Final validation: all 71 focused/process tests passed, including all 12 opt-in
  process tests (`/tmp/ca-hrs253-input-cap-process-tests.log`). All 60 provider
  client tests passed (`/tmp/ca-hrs253-input-cap-client-tests.log`). Fresh debug
  build and all-feature workspace check passed with existing warnings
  (`/tmp/ca-hrs253-input-cap-build.log`,
  `/tmp/ca-hrs253-input-cap-workspace-check.log`). Task-owned formatting and diff
  checks passed; oracle/reference status is unchanged. No paid requests or
  installed executable changes. Broader completion gates remain open.

### Continuation — guidance activation and runtime application

- Added a protected guidance transaction and an existing runtime prompt slot: the
  repository-work paragraph in the built-in system prefix. Candidates replace
  that paragraph only. Custom prompts without exactly one matching slot retain
  their own instructions. Provider route, model, options and build scope the
  active pointer; source replacement cannot silently carry an old policy forward.
- `create` now defaults to the active runtime paragraph. Explicit guidance
  baselines must match it; source experiments can still supply their own frozen
  instructions. Eligible guidance `run` now activates after rechecking verdict,
  the actual baseline executable and completed private engineering artifacts.
  `show --json` exposes the journal. `rollback` restores the previous pointer,
  without changing source files or executable bytes.
- Publication, pause checks and core state share the vault lock. A durable
  activity record accounts interrupted host work at its full reservation.
  Recovery reuses a published pointer; unknown charges cannot be cleared by
  activation. Rollback refuses newer guidance and remains available for historical
  builds. Incomplete journal/core state is not consumed by runtime requests.
- Runtime preparation happens before telemetry in main/fallback, headless,
  service and subagent paths. Client transport also prepares direct callers.
  Prepared requests pin their guidance, retain message order and cache boundaries,
  and adjust context estimates by the actual text delta. Analysis, verification
  and frozen experiment broker requests do not load an active policy. Reads run
  off the async executor with a two-second timeout; unavailable guidance falls
  back to the built-in paragraph with a warning. Unit tests use injected vaults.
- Initial transaction test exposed excessive nested future stack use; removed
  the nested engineering reservation and boxed the public transaction boundary.
  The first corrected three-test run passed; five expanded tests then passed
  (`/tmp/ca-hrs253-guidance-wire-tests.log`). They include real disposable-project
  engineering gates, crash recovery, exact fake-provider wire content, protected
  user text, route/build scoping and context estimates. Matched trial receipts in
  the transaction fixture are synthetic; this is not the full automatic live loop.
- Further validation in progress: prior-version rollback, broader runtime tests,
  a fresh debug binary and the combined opt-in process suite. No user-vault policy,
  runtime configuration, source branch or installed executable was activated.

- The previous-version regression initially failed: refusing rollback of a
  superseded policy changed its core stage to RecoveryRequired, preventing a newer
  deployment from restoring it. Ownership is now checked before mutation or time
  reservation, preserving that historical receipt. The regression passes
  (`/tmp/ca-hrs253-guidance-previous-fixed.log`).

- The combined suite passed all 77 tests, including all 13 opt-in process tests
  (`/tmp/ca-hrs253-guidance-combined-tests.log`). The runtime suite passed 136
  client/request/headless/service tests; the separate subagent suite passed 29.
  Fresh debug build, all-feature workspace check and actual create/rollback CLI
  help checks passed with existing warnings.
- Final accounting audit found that terminal-looking cycles with unsettled work
  could permit a new cycle, and a published guidance pointer could be read before
  activity settlement. Both new regressions failed first
  (`/tmp/ca-hrs253-guidance-settlement-red.log`). Admission now checks outstanding
  provider and host reservations regardless of terminal stage; runtime requires
  settled publication. Final revalidation passed all nine targeted tests
  (`/tmp/ca-hrs253-guidance-final-accounting-tests.log`).
- Prepared requests now retain the client's resolved provider source, so plain
  outcome capture in headless/service paths includes protocol/endpoint identity,
  matching explicit on-route capture. Previously those paths hashed only the
  provider label/model/options. A new regression verifies the equivalence.

- Final runtime regression suite: 166 passed, zero failed or ignored
  (`/tmp/ca-hrs253-guidance-final-runtime-tests.log`). Fresh debug build and
  all-feature workspace check passed with existing warnings
  (`/tmp/ca-hrs253-guidance-final-debug.log`,
  `/tmp/ca-hrs253-guidance-final-workspace-check.log`). Task-owned formatting and
  diff checks pass. The installed executable, runtime configuration, user vault,
  original source branch and oracle/reference trees were not changed by tests.


## Private guidance source snapshots (2026-09-23)

- Guidance creation now captures uncommitted source matching the running build in
  a private repository. The original HEAD, index and files are never mutated.
  Source experiments retain the clean-checkout requirement.
- The sealed plan binds original/private paths, source digest and freezing time;
  initial cycle accounting charges that time. Private snapshots remain owned by
  temporary-directory cleanup until all inputs validate, then survive journal
  persistence. Trials, engineering and promotion resolve the private source and
  verify its actual build-input bytes alongside Git identity.
- Build and runtime hashing share the same v2 encoding. Runtime inventory and
  descriptor-relative reads impose file/count/aggregate bounds and reject links
  and special files. The copy excludes root runtime files and oracle/reference
  trees, checks original inventory/bytes again, and isolates Git configuration.
- The initial four-test run had two failures because the scheduled-workspace
  helper rejects unlisted files rather than selecting a subset. Kept its contract
  unchanged and implemented a selective bounded copier. The final four tests
  pass (`/tmp/ca-hrs253-dirty-guidance-bounded-tests.log`), including build/runtime
  digest parity, unchanged original index/HEAD, excluded trees, source drift and
  invalid source/guidance plan combinations.
- All 12 core lifecycle tests pass (`/tmp/ca-hrs253-dirty-guidance-core.log`). The
  opt-in guidance transaction test now starts from dirty source, changes the
  original after freezing, runs actual disposable-project engineering gates,
  promotes, verifies fake-provider wire content, rejects changed snapshot bytes,
  recovers publication and rolls back while preserving user work. It passes
  (`/tmp/ca-hrs253-dirty-guidance-activation.log`). Its matched trial receipts remain
  synthetic. This is not a full automatic live cycle.
- Fresh debug build and all-feature workspace check pass with existing warnings
  (`/tmp/ca-hrs253-dirty-guidance-debug.log`,
  `/tmp/ca-hrs253-dirty-guidance-workspace.log`). Task-owned formatting, main.rs
  formatting and diff checks pass. Combined process regressions are running.

- The combined suite exposed a pause regression: the controller canonicalized the
  repository before observing a paused cycle, failing when the checkout was
  unavailable. The isolated test reproduced it
  (`/tmp/ca-hrs253-dirty-guidance-pause-red.log`). Snapshot resolution now happens
  only in work-admitting trial/engineering/promotion paths; CLI resolution follows
  its stage check. The unchanged regression passes
  (`/tmp/ca-hrs253-dirty-guidance-pause-fixed.log`). Final rebuild is underway.

- The first combined run ended 69 passed/3 failed. Besides the reproduced pause
  regression, two worker/verifier tests rejected frozen verifier identities after
  the pause-fix rebuild replaced their referenced debug executable while the old
  test process was still running. The verifier binds `CA_BUILD_DIGEST`; this run
  mixed builds. Preserve `/tmp/ca-hrs253-dirty-guidance-combined.log` as a failed
  run. A new combined run began only after the final debug rebuild completed,
  with no further source changes or executable rebuilds during the run.

- Final combined suite passed: 72 tests, zero failed or ignored, including all
  selected opt-in worker/verifier and transaction checks
  (`/tmp/ca-hrs253-dirty-guidance-final-combined.log`, 177.73 seconds). The final
  debug rebuild and all-feature workspace check passed with existing warnings
  (`/tmp/ca-hrs253-dirty-guidance-final-{debug,workspace}.log`). Task-owned/main
  formatting and diff checks pass. Installed executable and user vault remain
  unchanged. This checkpoint completes dirty-guidance freezing, not HRS-253.


## Failure discovery foundation (2026-09-23)

- Added protected `harness_discovery` and read-only `harness discover [--json]`.
  It loads bounded regular outcome files off the async executor, groups current-
  build failures by exact cohort/tool and tentative family, and excludes evidence
  referenced by any saved cycle. It does not load provider credentials, create a
  vault, generate candidates or mutate source.
- Extracted the existing freezer's single-event validation for shared use.
  Duplicate output evidence for one execution is now rejected, avoiding a later
  cycle spending the same failure under another event ID. At least three unique
  executions across two independent roots must remain. Capping at 64 reserves a
  slot for another root; ordering is independent of directory/input order.
- Fixed diagnostic signatures produce an evaluation-family hypothesis only.
  Ambiguous/unknown signatures stay unclassified. Exact unprotected relative
  source paths mentioned in the output are suggestions pending preimage and
  clean-source validation. Case discovery and idle execution remain unfinished;
  this preview is a reusable admission input, not a completed automatic loop.
- Initial five focused tests passed. Expanded tests first failed compilation
  because the new fixture omitted `start_harness_cycle`'s timestamp; supplied its
  deterministic fixture time. Final focused suite passed all 10 discovery,
  freezer and source-generation tests
  (`/tmp/ca-hrs253-discovery-final-tests2.log`). It covers consumed evidence from a
  real saved plan, independent-root capping, links/oversize, ambiguous IDs and
  execution receipts, denied calls, incomplete records, build/route/workspace
  separation and deterministic ordering. Fresh debug build and all-feature
  workspace check pass with existing warnings
  (`/tmp/ca-hrs253-discovery-{debug,workspace}.log`). Actual discovery CLI help,
  task-owned formatting and diff checks pass. No user-vault or installed-binary
  changes occurred. The broader process suite has not been rerun for this change.


## Automatic case and source selection (2026-09-23)

- `harness create --auto` now selects an unused supported failure group matching
  the configured provider, model and runtime route. It uses the existing freezer
  for immutable evidence/preimage/case validation. Manual case/evidence/source
  flags conflict with `--auto`. Idle dispatch remains unfinished.
- Added 18 embedded host-owned implementations: a target and two held-out
  variants for each existing verifier family, alongside the six fixed regressions.
  Candidate generation cannot edit the catalog/variant directory. Variants keep
  the public APIs and tasks but introduce different faulty implementations.
- Existing hidden probes missed several contract failures. Extended them for a
  retained valid UTF-8 prefix, relative scope rejection, duplicate call starts,
  a partial non-UTF-8 journal tail and unknown configuration keys. The opt-in
  catalog test requires every variant to pass public tests and fail hidden checks,
  then requires the existing golden repair to pass both. Process validation pending.
- Automatic source selection requires the failure workspace fingerprint to match
  the canonical ca checkout, all mentioned selected files to pass the source
  reader, and the original checkout to remain clean. Dirty/unrelated workspaces
  select guidance. Git inspection disables optional index updates and fsmonitor/
  hooks. The regression checks original index bytes remain unchanged.
- Added optional sealed preparation seconds, charged by initial cycle creation
  for both source and guidance. Automatic discovery/materialization passes its
  start time through the freezer. Legacy plans omit the field and preserve their
  previous accounting; invalid/over-budget preparation time cannot seal.
- The 10-test selection/freezer/catalog/parser suite and all 13 core lifecycle
  tests passed (`/tmp/ca-hrs253-auto-selection-final-tests.log`,
  `/tmp/ca-hrs253-catalog-core.log`). Final index-preservation refinement is being
  rechecked; fresh debug and all-feature workspace checks are running. No live
  model request, user-vault cycle, source adoption or installation occurred.

- The first real catalog run found a verifier false positive: the process target
  left its helper running, but `run_exact` waited for inherited stdout/stderr pipes
  until the helper naturally exited 30 seconds later. The timeout cleanup probe
  then observed an exited descendant and incorrectly passed it. Preserve the
  failing catalog assertion in `/tmp/ca-hrs253-catalog-process.log`; the original
  six baseline/repair checks still passed in that run.
- Verifier capture now uses nonblocking pipes with the command deadline. A held
  pipe marks the command timed out rather than extending verification and hiding
  missing cleanup. A real socket-pair regression retains its writer, proves the
  bounded/truncated capture returns at the deadline, and passes
  (`/tmp/ca-hrs253-catalog-pipe-tests.log`). Fresh build and catalog rerun pending.

- Final verifier run passed all seven selected tests, including 18 catalog bug/
  repair pairs and the original six baseline/repair pairs, with no ignored tests
  (`/tmp/ca-hrs253-catalog-final-process.log`, 69.05 seconds). All variants now fail
  hidden verification before repair and pass public/hidden checks after repair.
  This proves verifier sensitivity for those fixtures, not a full automatic
  generation-to-promotion cycle or full ca engineering-profile compatibility.
- Final selection/index-preservation suite passed all 10 tests
  (`/tmp/ca-hrs253-auto-selection-checked-tests.log`). All 13 core lifecycle tests
  pass. Fresh debug build and all-feature workspace check pass with existing
  warnings (`/tmp/ca-hrs253-catalog-pipe-debug.log`,
  `/tmp/ca-hrs253-catalog-final-workspace.log`). Actual `create --help` includes
  automatic selection; scoped formatting and diff checks pass. No model request,
  original-branch mutation, user-vault activation or installation occurred.

### Idle supervisor checkpoint (2026-09-23)

- Added default-disabled `harness_improvement` configuration and TUI supervisor.
  Dispatch requires five quiet minutes, network capability, no learning job,
  matching configured provider evidence and a 24-hour cooldown. The freezer
  rechecks cooldown under its worker lock to close concurrent-admission races.
- Shared CLI/background runner now checks cooperative boundaries between settled
  operations. Durable pause cause distinguishes foreground pauses from user pauses.
  Owned resume binds profile, canonical repository, plan digest and executable.
- Shutdown sets foreground and waits five seconds before aborting the async task.
  Blocking preparation may still finish; this is an explicit remaining lifecycle
  limitation. No claim of host-wide foreground detection or full idle integration.
- Focused config/idle/control/generation/freezer suite: 116 passed, zero failures
  (`/tmp/ca-hrs253-idle-owned-tests.log`). Core lifecycle suite: 13 passed
  (`/tmp/ca-hrs253-idle-core.log`). Fresh debug build and all-feature workspace
  check pass with existing warnings (`/tmp/ca-hrs253-idle-debug.log`,
  `/tmp/ca-hrs253-idle-workspace.log`). Scoped formatting and diff checks pass.
  Combined harness suite, including all opt-in process tests: 87 passed, zero
  failed or ignored (`/tmp/ca-hrs253-idle-combined.log`, 233.63 seconds).
- No runtime config activation, paid requests, original-branch adoption or install.
  Remaining work retains the active goal and includes idle end-to-end/shutdown
  coverage, measured hints, process cleanup, token/context proof and full gates.

### Cooperative preparation cancellation (2026-09-23)

- The idle worker now carries its foreground signal through blocking preparation.
  Discovery reads, source inventory/hash/copy/recheck operations and private Git
  waits check the signal. The freezer checks again before retaining the snapshot
  and publishing its cycle; publication then finishes so its owner can be paused.
- A scoped guard restores prior thread-local state on normal return and unwind.
  Explicit commands and unrelated threads do not inherit automatic cancellation.
- Three new tests cover worker cancellation/nested scopes/unwind, supervisor
  shutdown joining a blocking worker, and cancelled snapshot/discovery leaving no
  cycle or snapshot while preserving the original index. All 18 focused tests
  passed (`/tmp/ca-hrs253-idle-cancellation-final-tests.log`). Scoped format passes.
- Individual filesystem calls and other bounded inspections cannot be interrupted
  mid-call. Full discovery-to-rollback idle integration is still the next major
  validation task. The prior 87-test process pass predates this refinement.
- All-feature workspace check passed with existing warnings
  (`/tmp/ca-hrs253-idle-cancellation-check.log`); scoped format and diff checks pass.
  Debug executable has not been rebuilt after this refinement.

### Full guidance lifecycle integration (in progress)

- Added `src/harness_integration.rs` under the freezer's test module and protected
  it from candidate edits. It uses discovered evidence, all nine automatic cases,
  the shared background runner, a local fake provider and real worker/verifier
  processes. It pauses after generation and checks resume without another
  generation request; intended assertions cover 54 trials, measured promotion,
  consumed-evidence exclusion and rollback without synthetic trial receipts.
- The only substituted freezer authority is the expected source identity for a
  disposable tiny engineering project. This fixture cannot prove the real ca
  engineering matrix or actual TUI five-minute admission. Those remain separate.
- Initial run failed during fixture setup because the materialization parent was
  absent (`/tmp/ca-hrs253-integration-full-cycle.log`); created the parent and
  started fresh build/run at `/tmp/ca-hrs253-integration-{debug2,full-cycle2}.log`.
  No full lifecycle pass is claimed yet.
- Second run correctly rejected malformed synthetic discovery evidence: the
  fixture changed its tool-result excerpt and failed to update its single runtime
  epoch. Corrected those fixture fields, preserving production attribution checks.
  Evidence: `/tmp/ca-hrs253-integration-full-cycle2.log`. Third fresh build/run:
  `/tmp/ca-hrs253-integration-{debug3,full-cycle3}.log`.
- Third run is live and progressing: inspected its disposable cycle at 12/54
  trials, 25 requests and 205 active seconds. A process sample shows repeated
  `pin_binary` hashing/copying of the large debug worker, not a deadlock.
  Live combined build/test session: 43017; test process 49249; disposable vault
  `/private/var/folders/nx/1xjt7xc96zg_d9w7glkcyvhr0000gn/T/.tmpv9tqdo/vault`.
  Keep that run; do not restart or rebuild its executable while it is active.
- All-feature workspace check passed with existing warnings
  (`/tmp/ca-hrs253-integration-check.log`).

### Measured hint policy foundation (in progress)

- Added protected `src/harness_hints.rs`: typed interventions, explicit policy
  selection and a one-intervention addition check for matched experiments.
  Pattern recognition uses completed `run_command` calls from the current user
  turn, rejects denied/unfinished/duplicate evidence, handles parallel batches,
  and emits at most two reminders on the batch containing the third occurrence.
  Reconstructing from bounded history avoids mutable session state and repeated
  reminders on subsequent tool batches.
- This is not wired into trial inputs or runtime activation yet. No intervention
  is enabled or claimed beneficial. Next: bind baseline/candidate policies into
  generation/trial/activation receipts and apply the same transformation in the
  isolated worker and prepared runtime request before fingerprinting/transport.
- Initial focused tests: 3 passed (`/tmp/ca-hrs253-hints-policy-tests.log`).
  Final parallel-batch/duplicate-result tests are running in session 27323 at
  `/tmp/ca-hrs253-hints-policy-final-tests.log`. This only rebuilds the test target;
  the live guidance integration's referenced debug executable remains unchanged.
- The live full-cycle run reached 40/54 trials and 79 charged requests. It runs
  the build preceding the new hint module; do not claim it validates hint logic.
- Final hint-policy run: 4 passed, zero failures
  (`/tmp/ca-hrs253-hints-policy-final-tests.log`). Scoped format/diff checks pass.
- Full-cycle run 43017 ended FAILED after 898.78 seconds. It completed all 54
  trials, 106 charged requests, $0.004240 synthetic spend and the tiny engineering
  matrix. Promotion correctly rejected the test executable as different from the
  evaluated debug ca executable. This is a fixture authority mismatch, not a
  reason to relax production identity checks.
- Corrected the fixture to resume through the evaluated ca CLI in a disposable
  HOME/config/machine-vault, retaining the generation/foreground-pause check in
  the test process. Fresh build session 29036: integration-debug4.log.
- CLI handoff attempt 4 failed before trials due to the fixture's `--binary`
  spelling; corrected to the documented `--baseline-binary`. No production
  interface or identity check changed. Log: integration-full-cycle4.log.
- Current session 11583 builds a fresh worker, strips debug symbols into the
  task-owned work/hrs253-worker/ca, ad-hoc signs/verifies it, and runs attempt 5
  (`/tmp/ca-hrs253-integration-{debug5,full-cycle5}.log`). Do not overwrite the
  referenced worker until this session terminates. This is not an installation.
- The hint foundation is intentionally not connected yet and adds non-test
  dead-code warnings. Resolve those by wiring it, not by claiming a clean lint.

### Guidance integration pass and hint wiring (2026-09-23)

- Full-cycle attempt 5 passed in 590.09 seconds
  (`/tmp/ca-hrs253-integration-full-cycle5.log`): all 54 trials, 106 accounted
  local provider requests, measured target improvement, tiny engineering gates,
  CLI-host activation and rollback. No synthetic trial receipts. This build
  predates hint wiring and does not establish real-ca engineering validity.
- Added baseline/candidate hint policies with legacy-compatible omitted empty
  fields. Generation permits one new hint with unchanged prompt text. Source
  trials retain the baseline policy. Worker and runtime use the same reminder
  transformation; runtime fingerprints bind the policy before any cue fires.
- Activation and rollback bind policies. Runtime consumption reconstructs the
  evaluated candidate digest from the receipt, including instructions/policies,
  so matching but altered pointer/journal fields cannot enable an unmeasured policy.
- Twenty focused tests passed, one process test ignored
  (`/tmp/ca-hrs253-hints-wiring-final-tests.log`). Latest exposure refinement adds
  host-checked system transformation, applied-hint evidence in verification
  artifacts, and a gate requiring delivery in every candidate target repetition.
  Exposure tests are running; actual hint-trigger process tests still remain.
- Inferred integration gap to resolve: source trials inherit active guidance/hints,
  but their new build has a different runtime guidance scope. Ensure source
  activation carries the evaluated baseline policy into that build, or the deployed
  behavior can differ from the evaluated behavior. Do not treat this as a user
  prohibition; fix the lifecycle before final automatic activation validation.
- Exposure-gate suite passed: 19 tests, one process test ignored
  (`/tmp/ca-hrs253-hints-exposure-tests.log`). Synthetic artifact tests distinguish
  this gate from coding verification and reject missing/undelivered exposure.
- Real isolated-worker hint test passed in four seconds with policy disabled and
  enabled: five requests each, reminder only after the third completed probe,
  absent on the following request, and host-recorded exposure
  (`/tmp/ca-hrs253-hints-worker-process2.log`). Initial process-test compile failed
  on a fixture JSON macro; corrected without a production-interface change.
- Trial artifacts now retain bounded redacted worker failure details and request
  counts, preserving legacy artifacts by omitting absent optional fields. This
  uses the previously unused WorkerResult fields in production diagnostics.
- Final focused and workspace checks are running at
  `/tmp/ca-hrs253-hints-final-{focused,workspace}.log`. The full real-worker
  guidance-cycle pass predates hint wiring; a complete measured hint-cycle test
  and source-build policy carry-forward remain required.
- Final checks passed: 20 focused tests (one separate process test ignored),
  all-feature workspace check with two existing renderer warnings, scoped format
  and diff checks. Logs: `/tmp/ca-hrs253-hints-final-{focused,workspace,format,diff}.log`.
  Sessions 11583, 13968, 96667 and 16955 are terminal; no worker remains live.

- Measured-hint full lifecycle passed in 583.47 seconds: 54 trials and 124
  accounted local requests, cue-driven target repair, exposure gating, activation
  and rollback (`/tmp/ca-hrs253-hint-full-cycle.log`). Session 6914 is terminal.
  This predates the source-policy carry changes and uses tiny-project gates.
- Source policy carry-forward is implemented: signed-artifact compiled identity,
  frozen route and policy binding, replayable scoped publication, settled source
  activation required by readers, rollback invalidation, and conflict rejection.
  Synthetic binding test passed (`/tmp/ca-hrs253-source-guidance-tests.log`).
  Source-process coverage and final checks are still running; no install.

- Source policy validation passed: four disposable-project activation/recovery
  process tests (47.43 seconds), real signed ca identity inside the offline
  sandbox (1.47 seconds), 92 focused tests with 18 opt-in process tests ignored,
  all-feature workspace check, and scoped formatting/diff checks. Logs:
  `/tmp/ca-hrs253-source-guidance-{process,identity,focused,workspace,format,diff}.log`.
  Source-process tests retain synthetic coding receipts; real-ca source evaluation
  remains unproven. Initial compile failure returned Result instead of main’s
  integer exit code; fixed and the failed check log retained. All current sessions
  are terminal. No installed binary, runtime configuration or user vault changed.

- Started the first real-ca engineering-profile probe, using the production
  snapshot and fixed gate runner in a private retained checkout. Opt-in test
  `hrs253_real_ca_engineering_profile` selects only an existing fixed gate via
  `CA_HARNESS_ENGINEERING_GATE` (default `test_default`); it does not alter the
  production gate specification. Session 51642 runs the focused diagnostic test
  and then the real default test gate. Logs:
  `/tmp/ca-hrs253-engineering-tail-test.log` and
  `/tmp/ca-hrs253-real-ca-engineering-test.log`. Results pending.
- Engineering diagnostics now retain the final 8 KiB, while hashing the entire
  bounded stream. Previously long compiler output could hide the final failure
  behind its initial build messages. Small identity/Git output is unaffected.

- Real default test gate failed after 208.99 seconds; session 51642 is terminal.
  The diagnostic exposed repeated denied `/etc/gitattributes` lookups, but stderr
  swallowed stdout’s result. Set `GIT_ATTR_NOSYSTEM=1` in the clean environment,
  retain separate bounded tails for both streams, and preserve whole-stream hashes.
  The regression caught secondary truncation by the learning sanitizer (session
  28391 failed); switched to its same privacy filtering without early truncation.
  Session 23145 runs the corrected diagnostic test then a new real default gate;
  logs are `/tmp/ca-hrs253-engineering-tail-test3.log` and
  `/tmp/ca-hrs253-real-ca-engineering-test2.log`. No broad pass is claimed.

- Corrected diagnostic regression passed. The second real default gate failed
  in nine test targets after 205.72 seconds (session 23145 terminal). Its retained
  stderr identifies ca bin/cli/role_run_cli, ca-watch lib, codex-team lib and
  manual_commit_cli/mcp_sdk/protected_baseline_cli/remote_cli. Full causes still
  need inspection. Added opt-in test-only bounded stream tracing; session 83451
  runs a fresh unchanged default gate with trace in
  `/tmp/ca-hrs253-real-ca-engineering-test3.log`. No production gate skipped or
  sandbox permission widened.

- Traced real default gate finished failed in 212.95 seconds (session 83451
  terminal). It proves production-profile incompatibility: loopback listener
  binds fail with EPERM, nested sandbox-exec reports sandbox_apply EPERM, and
  ca-watch process observation degrades. ca bin alone reports 2383 passed,
  224 failed, 24 ignored. This is not a promotion-ready engineering environment.
  Asked the user to choose a dedicated macOS VM versus host execution with
  isolated HOME/workspace; no execution-policy change made while pending.
  Apple Container/Colima installed but stopped; existing Lima VM is unrelated
  and untouched.
- Independently repaired a stale prepared-request test count: the existing
  registry includes install_binary (also asserted by its registry test), so the
  request test now expects 46 plus the conditional privilege tool and explicitly
  asserts install_binary. Focused check: session 57089,
  `/tmp/ca-hrs253-request-schema-regression.log`. Normal host workspace validation
  (not automatic candidate execution) is running with system/global Git config
  disabled: `/tmp/ca-hrs253-workspace-current-default.log`.

- Current default workspace validation: ca 2607 passed/0 failed/24 ignored,
  ca CLI 38 passed, role-run CLI 17 passed, codex-team lib 575 passed/0 failed/
  1 ignored. Remaining unrelated red targets: mcp_sdk, protected_baseline_cli,
  remote_cli. Log `/tmp/ca-hrs253-workspace-current-default.log`; session 20670
  terminal. Prepared-request regression passed, session 57089.
- Strict default Clippy passes (`/tmp/ca-hrs253-current-clippy2.log`, session
  65989 terminal). Minimal lint repairs remove dead spinner code, derive the
  identical BlockStyleConfig default, rename an unused binding, share the test
  build-identity module, initialize enabled settings directly, and alias the
  verifier capture result type. All-feature Clippy/tests are running next.

- All-feature workspace result: 3430 passed, 9 failed, 26 ignored; the same
  mcp_sdk/protected_baseline_cli/remote_cli targets remain red. Default totals
  were 3281 passed, 9 failed, 25 ignored. No test expectations weakened for these
  missing/contradictory pre-existing behaviors. Both strict Clippy configurations,
  whole-repository format, and diff checks now pass. Only formatter-reported
  event.rs/render.rs/worktree_manager.rs were formatted. The standalone experiment
  expects unused portions of its shared materialization module; main remains
  strictly linted. Logs `/tmp/ca-hrs253-current-{clippy2,clippy-all2,format2,diff}.log`.
- Session 20543 builds a fresh default debug worker for a real PTY idle test;
  `/tmp/ca-hrs253-idle-pty-build.log`. Intended test: isolated HOME/config/vault,
  local fake provider, synthetic same-build failure evidence, real 300-second
  TUI idle dispatch, automatic discovery/generation, and foreground pause at a
  settled boundary. No PTY process started yet. Automatic engineering environment
  question remains pending; no release/install or live user-vault mutation.

Fresh default debug build/signature verification passed; session 20543 is
terminal (`/tmp/ca-hrs253-idle-pty-build.log`). The private worker is current,
but no PTY test has started and no installed executable was changed. All named
sessions in this checkpoint are terminal. The next independent test should use
a disposable profile and the unchanged real five-minute idle interval.

- Real TUI idle probe running in session 20678; current fixture
  ca-hrs253-idle-pty-7v4lzhg7, TUI PID 19622, log
  `/tmp/ca-hrs253-idle-pty-run2.log`. Typed disposable fixture seeder verifies
  current-build discovery and localhost route; no real task-efficacy claim.
  Uses a separate empty workspace, actual source snapshot repository, isolated
  home and the unchanged 300-second interval. First attempt was deliberately
  stopped to isolate the workspace; its brief TUI PID 19609 is verified exited.
  Fresh default worker and seeder match. Do not rebuild the worker while live.

### HRS-253 real-clock PTY result and terminal probe fix

The previous PTY attempt (session 20678, run2 log) failed after 440 seconds:
no generation request, no cycle, forced shutdown. A one-second sample of its
owned TUI process showed ca-terminal-theme-probe blocked in File::read while
input polling was suspended. The 400ms deadline bounded poll but not the read.
terminal_awareness.rs now opens /dev/tty nonblocking, checks POLLIN, and retries
WouldBlock/Interrupted only within that deadline. The two existing terminal
awareness tests pass; the previously failing real PTY scenario now passes.

Session 10860 is terminal, exit 0. Fresh matching worker/seeder, log
`/tmp/ca-hrs253-idle-pty-run3.log`, fixture ca-hrs253-idle-pty-jn_sd424:
one local generation request at 334.210 seconds, foreground pause at 336.404
seconds, one settled charge, zero trials, no outstanding reservations, matching
idle ownership, unchanged original Git index, graceful TUI exit status 0 and no
forced shutdown. Initial background maintenance and preparation account for
elapsed time beyond the unchanged 300-second inactivity threshold. This proves
idle discovery/generation and foreground pause, not a complete idle evaluation
or real-provider improvement. No installed binary or user profile was changed.
The automatic engineering-environment question remains pending.

Post-fix validation is complete: ca binary tests 2607 passed, 0 failed,
25 ignored (`/tmp/ca-hrs253-terminal-final-tests.log`, session 47541 terminal);
all-target/all-feature strict Clippy passed (`/tmp/ca-hrs253-terminal-final-clippy.log`,
session 37768 terminal), whole-repository formatting passed
(`/tmp/ca-hrs253-terminal-final-format.log`), and diff check passed. No test or
build sessions remain running from this checkpoint. The previous workspace-wide
nine unrelated integration failures remain recorded; this focused rerun does not
supersede those failures. No release/install has run. Next work remains automatic
engineering environment, escaped-descendant containment, provider context proof,
recovery completeness and full source/idle validation.

### HRS-253 settled budget exhaustion and admission

A real admission regression is being repaired: a cycle with no promotion decision
and no unresolved provider/time reservations still blocked every later cycle when
its stage was BudgetExhausted. A focused idle-admission regression failed against
the previous compiled implementation (`/tmp/ca-hrs253-exhausted-before-actual.log`).
The initial --exact invocation matched zero tests and is not evidence of a pass.
Both explicit freezing and idle admission now share the host-owned
harness_control::blocks_new_cycle predicate. A settled pre-decision exhausted
cycle releases admission; its stage, charges, evidence and history remain intact.
Idle cooldown and unused-evidence selection are unchanged. Unknown reservations,
recovery states, and exhausted cycles with a saved decision remain blocking,
because a decision-bearing cycle could have interrupted deployment.
Focused HRS-253 checks are running in session 69490,
`/tmp/ca-hrs253-exhausted-after.log`; no worker executable was replaced.

Lint rerun 3550 exposed the integration-test sibling's implicit dependency on
its parent's HarnessStage import; that test module now imports the enum explicitly.
Session 3550 terminal failure; final strict lint/format/diff rerun is 45428,
`/tmp/ca-hrs253-exhausted-clippy3.log`. These are import-only follow-ups to the
passing 2608-test run. The private signed PTY worker still contains the previous
source identity: rebuild it before creating any new current-source process
experiment, and never overwrite it while an older experiment is running.

Final admission-fix checks passed: all-target/all-feature strict Clippy, whole
repository format, and diff check (`/tmp/ca-hrs253-exhausted-{clippy3,format,diff}.log`).
Session 45428 is terminal, exit 0; all sessions launched for this fix are terminal.
Behavioral coverage: 94 focused tests and 2608 ca tests passed; final follow-ups
only made test enum imports explicit and were checked in all-target Clippy.
No release/install, user-vault mutation, paid request, or isolation-policy change.
The broader recorded workspace integration failures and automatic engineering
sandbox incompatibility remain unresolved. Goal remains active.

### HRS-253 completion audit after release build

The original six-part request remains the completion scope. Current evidence:

| Requirement | Current implementation/evidence | Completion limit |
| --- | --- | --- |
| Trustworthy success measurements | session_outcome separates completion, scoped verification and explicit feedback; interruption, mutation invalidation, legacy and storage-failure regressions pass in the 2608-test ca run. | Runtime mechanism verified; does not label model claims or ordinary exit as acceptance. |
| Actual runtime fingerprints | harness_registry hashes prepared instructions/tools, effective route/options and compiled identity; runtime wiring covers TUI/headless/service/delegation. Current-build identity regression passes. | The older disk-inventory CLI remains explicitly separate. |
| Inspectable Analyze evidence | learning validates event IDs, independent task counts and supplied session references; invented-reference regression passes. | Causal explanations remain hypotheses; no real-provider efficacy claim. |
| Matched experiments and trends | Frozen 9-case/54-trial design, held-outs, host verification, regressions/cost/time gates and chronological comparable-cohort rates. Controlled guidance/hint end-to-end runs and current focused checks pass. | Actual ca source-candidate engineering matrix does not pass its current execution environment. |
| Candidate lifecycle | Discovery, private source/guidance candidates, budgets, trials, signed activation, journaled rollback and controls exist. Guidance/hint controlled lifecycle tests pass. Real-clock TUI creation/generation/pause/clean exit passes. | Full automatic source and idle evaluation still unverified; unknown-charge recovery remains conservative. |
| Measured execution guidance | Candidate policy/exposure binding, completed-call thresholds and bounded reminders implemented; controlled 54-trial hint activation/rollback passed. | A deterministic fake provider proves the mechanism, not benefit on real tasks. |
| User-approved idle/budget/source adoption | Five quiet minutes, cooldown, one outstanding cycle, $20/7200 active seconds, clean branch adoption and preserved source/binary receipts. Settled pre-decision exhaustion now permits later admission. | Escaped process-group descendants and provider-specific context proof remain unresolved; no paid live cycle yet. |
| Validation and installation | Current ca tests, strict all-feature lint, formatting and diff checks pass. Full release workspace build and signed artifact smoke checks now pass. | Previously recorded nine workspace integration failures remain; installed executable is unchanged. |

Release command `cargo build --release --workspace --all-features` passed in
2m19s, session 21770 terminal, `/tmp/ca-hrs253-release-validation.log`.
An independently copied/ad-hoc-signed artifact passed strict signature verification,
mode 0755/uid 501, --version, --help and --internal-harness-build-identity under
a new temporary home with a cleared environment. Its hash remained unchanged
through the smoke checks. Session 25114 terminal; `/tmp/ca-hrs253-release-smoke.log`.
Private candidate SHA-256: 7bbfdf18bb787595945ebddf6df0851c3f3037635bc21522b99967d17ce73fed.
Compiled identity: 7f4084a6dd1909010d68bf839f673ea952d5322de7baaaf8ba8ba45081c3634d.
Artifact: Codex task work/hrs253-release-validation/ca; machine-readable smoke
receipt: work/hrs253-release-validation/result.json. No installation performed.

Reinspection confirms engineering and worker cleanup currently signal only the
original process group. The passing engineering timeout/cancellation regression
spawns a direct child that execs sleep, so it does not prove cleanup after setsid.
This unresolved containment requirement belongs in the engineering-environment
choice, not in a claim based on the direct-child test. The requested macOS VM vs
isolated-home host choice is still unanswered. No environment policy was changed.
All processes launched in this continuation are terminal. Goal is not complete.

### HRS-253 blocked audit — execution environment decision

The same automatic engineering-environment blocker has persisted through more
than three consecutive goal turns. Independent terminal, admission, lint and
release validation progressed in those turns and is now finished. Reinspection
of the production profile still shows network_allowed=false and
hide_other_processes=true; the retained real-workspace log still demonstrates
localhost bind EPERM and nested sandbox_apply EPERM. The signed private release
still matches its recorded SHA-256. No validation handle remains live.

Completion remains unproven. Proceeding with full automatic source evaluation
requires resolving the outstanding dedicated macOS VM versus isolated-home host
choice. This is an implementation/isolation decision requested from the user,
not a claimed user prohibition on host execution. It also determines the process
containment work; direct-process-group tests do not cover escaped descendants.
Provider-specific context proof, conservative unknown-charge recovery, the
recorded workspace test failures, full source/idle lifecycle and installation
remain open and are not waived. No deployment, configuration change or broader
execution grant was inferred from the unanswered question. The goal is being
marked blocked pending that decision; do not mark the plan complete.


### Dedicated macOS VM implementation — user decision received

The user explicitly chose a dedicated macOS VM. The previous environment-choice
blocker is resolved; implementation resumes. Use a separate Tart store under
/Volumes/vm/ca-harness-tart, with no reuse of existing Lima or VMware machines.
Host is arm64 macOS 26.6.2, 36 GiB RAM, about 622 GiB available on the VM volume.
Tart's official docs support macOS guest automation and read-only folder shares.
The official Homebrew formula currently fails on deprecated macOS dependency
syntax. Its verified 2.32.1 release archive is being installed privately instead;
SHA-256 comes from the official tapped formula. No host global network/DHCP
settings are changed. Plan: provision a credential-free macOS base, verify guest
local listeners/nested sandboxing, then integrate fresh disposable VM execution
with fixed gates, bounded receipts and host-owned shutdown. Do not infer that
VM boot alone proves the complete harness lifecycle or eliminates other test
failures. Source snapshots are copied into guest storage; host credentials,
original checkout and vault must not be mounted writable into a candidate VM.

VM provisioning progress: Tart 2.32.1 installed privately under
/Volumes/vm/ca-harness-tools; official archive SHA and Developer ID signature
verified. Official Homebrew formula failed on obsolete dependency syntax, so
no formula was edited. Separate TART_HOME /Volumes/vm/ca-harness-tart contains
ca-harness-base cloned from ghcr.io/cirruslabs/macos-tahoe-base at OCI digest
sha256:1b093499716409d29e8b5336844528e1cae375db97d2ad8e5aeff78cf0da201e.
Image download session 96775 completed. Guest macOS matches host 26.6.2/25G83.
VM configured 4 CPUs, 12288 MB, 192 GB disk; guest agent grew APFS to use it.
Toolchain 1.97.1 and its dynamic dependencies were copied from installed host
package artifacts, not user configuration. Cargo vendor completed using the
locked manifest; missing cross-platform crates required a registry download.

Guest ca-runner uid 550 is non-admin and cannot sudo. The public base-image
admin password was replaced without retaining its value; SSH/screen sharing
and GUI autologin disabled. Guest root-owned packet filter exempts loopback and
blocks other traffic. Clipboard/audio disabled; bootstrap share was read-only.
The guest probe passed localhost sockets, nested sandboxing, offline Rust/serde
compile/test, denied sudo, blocked network probes and an actual rejected write
to the host share (/tmp/ca-hrs253-vm-probe2.log). Mode-bit-only write checks were
insufficient on the read-only share; the original probe failed before replacement
with an actual write attempt. These are environment checks, not ca suite results.

Cold boot currently needs repair: the added admin-user RPC daemon did not
reconnect without GUI login. Guest filesystem was inspected only after stopping
the owned VM; its isolated disk was attached, inspected and detached. No RPC log
was created in /var/log. Its log path was moved to /tmp and HOME set explicitly;
second cold boot session 32214 is running. All previous VM boot/exec handles
are terminal. Cold-boot success remains unproven. No candidates executed yet.

Added ignored hrs253_export_current_source_for_vm test in harness_engineering.rs.
It uses the production bounded snapshot copier and emits an input receipt only;
the export test passed (session 86424). Current archive/source receipt lives in
Codex task work/hrs253-vm-bootstrap/current-source.json. This is not a gate pass.
Read-only virtual shares cached edits to bootstrap scripts during setup; use
unique immutable paths for published inputs. No user runtime config/install or
original Git branch mutation occurred.


### VM validation update — 2026-09-23

Cold-boot repair and template provisioning are complete. The final base probe
passed restricted-account checks, no host shares, Rust/ripgrep/system-Python
versions and PF status/rules, then stopped the base. Receipt:
Codex task work/hrs253-vm-bootstrap/base-ready.json.

The first full VM run completed in 595 seconds with 3,266 passes, 25 failures,
and 27 ignored tests. Thirteen additional failures were missing ripgrep and two
were Homebrew Python being denied by the sandbox. Installed root-owned ripgrep/PCRE2
from the host package artifacts into base and validation clone, then changed
only guest PATH order to prefer system Python. The corrected run records
3,281 passes, 10 failures, 27 ignored; all 2,608 ca unit tests pass. Nine failures
match the recorded host integration failures. The additional MCP asynchronous
step fixture emits its result immediately without consuming the launch barrier;
it races the controller's write and fails with Broken pipe. No production gate
or test expectation was relaxed. Logs: /tmp/ca-hrs253-vm-workspace{2,3}.log;
receipts: work/hrs253-vm-bootstrap/workspace-result{2,3}.json.

Detached-process cleanup passed: stop returned zero, Tart listed the VM stopped,
and after reboot the unique child command was absent and its heartbeat unchanged.
Receipt: work/hrs253-vm-bootstrap/detached-probe-verified.json. Initial inspection
falsely matched its own query text; the corrected predicate matches only the
child's final separate nonce argument. All earlier VM handles are terminal.

All-feature protected-source probe is running: VM handle 72856, driver 82701,
/tmp/ca-hrs253-vm-workspace-all.log, /tmp/ca-hrs253-vm-all-driver.log. The source
was moved beneath root-owned /opt/ca-harness/project, with 2,948 input entries
read-only to ca-runner; only target and the separate test home are writable.
The source parent is protected too, preventing rename-and-replace of the frozen
root. A narrowly scoped Git safe.directory option permits its status check.
The driver stops the whole VM on finish/error/1800-second timeout. This remains
an environment probe, not production promotion evidence. Tart has not yet been
wired into harness_engineering or harness_execution; config, installed binary,
original Git branch and real-provider execution remain unchanged.


#### Remaining VM integration work

1. Add an explicit, digest-bound execution environment to new frozen plans.
   Bind Tart/tool identity, prepared template identity, guest toolchain/vendor
   inputs, execution policy and fixed gate versions. Changing the environment
   must invalidate existing gate/trial receipts. Preserve legacy plan decoding.
2. Give each disposable VM an owned lifetime outside guest control. Cancellation,
   timeout and host-worker failure must terminate the entire VM, including child
   sessions. Verify the stopped state before accepting completion or cleanup.
3. Transfer bounded inputs into root-owned guest storage, with a protected parent
   directory and writable build/test output directories. Do not mount the host
   checkout, vault or credentials. Keep the system-first PATH proven here.
4. Route engineering builds, artifact identity/smoke execution, trial workers and
   executable verification through the selected VM boundary. Preserve host-owned
   provider requests, spend reservations and promotion decisions. Candidate
   output remains untrusted; import only bounded expected artifacts with digest,
   regular-file and path checks. A build-only VM leaves candidate execution on
   the host and does not satisfy this integration.
5. Exercise the actual source and idle lifecycles through that backend. Retain
   the known workspace gate failures and reject promotion while they remain;
   do not reuse manual environment-probe receipts as engineering admission.


### VM environment validation settled — 2026-09-23

The prepared base and validation VM are both stopped. No live VM/test/build
handle remains. Current evidence is in the Codex task's
work/hrs253-vm-bootstrap directory; bootstrap.json records the prepared state
and explicitly says production_backend_wired=false.

Corrected default workspace run: 3,281 passed, 10 failed, 27 ignored. All 2,608
ca unit tests passed. First all-feature protected-source run: 3,413 passed,
27 failed, 28 ignored. That run exposed two setup mistakes: removing owner write
bits made copied fixture workspaces read-only, and moving the source while
reusing build output left embedded CARGO_MANIFEST_DIR paths pointing to its old
location. Kept that output under /Users/ca-runner/target-before-relocation and
rebuilt from fresh output at the final /opt/ca-harness/project path. Input files
are root-owned with normal 0644/0755 modes; the non-admin runner cannot open them
for writes, chmod them or replace their root-owned parent. Private copies remain
writable to their new owner. Build output is owned by ca-runner.

Corrected all-feature workspace run: 3,429 passed, 11 failed, 28 ignored, in
521.56 seconds (two build jobs, four test threads). All 120 experimental-workflow
tests now pass. Failures are the nine previously recorded integration failures,
the asynchronous-step launch-barrier fixture, and
voice::tests::cancelling_local_transcription_kills_descendants under parallel
load. The voice test passes three later isolated runs without code changes;
that does not erase its broad failure. Earlier serialized all-feature ca suite
passed all 2,637 tests. Logs: /tmp/ca-hrs253-vm-workspace-all{,-corrected}.log and
/tmp/ca-hrs253-vm-voice-focused.log. These are environment probes; the manual
all-feature command did not include the fixed matrix's --all-targets option.
No full engineering admission or source-efficacy result is claimed.

A direct guest-local AF_VSOCK probe reached the administrative guest-agent port.
Prepared a private host-only agent from upstream commit
26268c6d77d1c1059cd4a0fb88ae170c64cd2d9a: reject connections unless the kernel's
peer CID is VMADDR_CID_HOST. The upstream reference clone remains unchanged.
The changed agent's vsock and RPC tests pass, its binary is ad-hoc signed and
verified, and the original license is retained. Direct Go dependency fetching
timed out; fetched pinned module artifacts over official-proxy HTTPS with curl,
then built offline with Go checking the existing go.sum hashes. The first
installation did not survive immediate VM shutdown; explicitly fsynced the
installed files and ran guest sync before stopping. The next cold boot verified
the exact binary and daemon path. Keep that durability step in provisioning.

Both VM images now reject guest-local RPC before returning application data.
Host RPC still works after rejection. The non-admin guest also cannot bind the
host CID (errno 49). PF remains enabled with the block-all/non-loopback policy;
no host directories are shared. This closes the observed administrative control
path for these probes, not a blanket claim about all VM escape possibilities.
Verified receipt: work/hrs253-vm-bootstrap/host-only-agent-verified.json;
validation repeat: final-validation-probe.json. Binary SHA-256:
ab2159ca9ac4bd5ab3f49b0c0715561c8688549fde6e7cdf8418b23d1f25a3cd.
Managed binary, source patch, test, license and build/verification receipts:
/Volumes/vm/ca-harness-tools/guest-agent-host-only-ab2159ca9ac4.

Production harness_engineering/harness_execution still use their existing
backend. Next HRS-253 work is to freeze the VM environment in plans, route every
candidate execution and verification boundary through owned disposable VMs,
and validate bounded artifact transfer/cancellation and the complete source/idle
lifecycle. Do not turn the environment checks into promotion receipts. No user
provider settings, installed ca binary, original Git branch or real-provider
calls were changed. The overall plan remains ACTIVE and incomplete.


### Frozen VM environment binding — 2026-09-23

New optional `harness_improvement.vm` configuration freezes the selected Tart
binary, stopped template config/disk/NVRAM, guest-agent digest and host adapter
source digest into each new plan. Legacy plans omit the optional field and keep
their prior digest. Input reads reject links, unsafe ownership/permissions,
replaceable parent directories and concurrent changes. Subsequent checks detect
same-length edits even when mtime is restored. Both explicit and idle creation
charge preparation time, including image hashing.

VM-bound evaluation, engineering, source-build and trial entry points currently
fail explicitly before host execution. These are transitional guards, not a
finished VM backend; remove each only when its actual execution path is migrated.
User runtime configuration remains unchanged.

Validation: four focused VM tests and one shared plan-binding test pass. The
replaceable-parent regression failed before its fix. Strict workspace Clippy
(all targets/all features), full formatting and diff checks pass. Logs:
/tmp/ca-hrs253-vm-binding-tests4.log,
/tmp/ca-hrs253-vm-binding-core-tests3.log,
/tmp/ca-hrs253-vm-parent-before.log,
/tmp/ca-hrs253-vm-binding-{clippy,format,diff}.log.

Full all-feature workspace run: 3,420 passed, 25 failed, 29 ignored, including
all 2,641 ca unit tests passing. Nine failures match the previously recorded
integration failures. Sixteen shared-library commit tests fail because the host
fixture environment has no Git identity: one repeats in isolation and passes
with explicit fixture-only author/committer identity. The entire unchanged
shared-library binary then passes 576 tests (one ignored) with that identity.
No user Git config or production code was changed to obtain this result. Keep
the original failed run. Logs: /tmp/ca-hrs253-vm-binding-workspace.log,
/tmp/ca-hrs253-vm-binding-commit-{isolated,identity}.log and
/tmp/ca-hrs253-vm-binding-core-identity.log.

The unoptimized real-image freeze probe exceeded its explicit 600-second bound;
it failed, without booting or changing the VM. Optimized verification passed in 427.36 seconds under the same 600-second
bound. Logs: /tmp/ca-hrs253-vm-real-binding{,-release}.log. The frozen receipt is
retained at work/hrs253-vm-bootstrap/frozen-vm-verified.json in the Codex task;
its adapter source digest predates the controller added below. Disk SHA-256:
0050fcb56c7b9d0a12d02bd243ed25a01fd5d8a6de7acd68bb29fef70be920d4.

An independently sessioned host supervisor prototype lives in the Codex task's
work/hrs253-vm-lifecycle directory. Nine mock lifecycle tests pass: owner EOF,
abrupt owner exit, hard deadline, termination, template drift, cancellation
during clone, failed boot, failed cleanup and bounded inventory output. A real fresh
clone also passed: non-admin guest child PID/SID 370 escaped its parent's
session, then the owner was SIGKILLed. Pipe EOF caused the surviving supervisor
to stop the whole VM, confirm stopped state and remove only its disposable
clone. Independent inventory confirms the clone is gone and both prepared VMs
remain stopped. Receipt: work/hrs253-vm-lifecycle/
probe-22149b228c4448ab86e7e4519c1883e1/result.json. This is a controller prototype
probe, not production harness wiring or an engineering/promotion receipt.


### Owned VM controller — 2026-09-23

Added src/harness_vm/lease.rs and embedded supervisor.py. Rust starts an
independent supervisor with a clean host environment and a one-owner pipe.
The supervisor checks the frozen template before and after cloning, disables
Tart auto-pruning, starts a fresh named VM without host shares/clipboard, and
stops the entire VM on EOF, deadline, termination or failure. It confirms the
stopped state before deleting only its own clone. Failed cleanup retains the
clone and reports failure. Bounded receipts survive owner loss. Rust explicit
finish checks the cleanup receipt and template identity; Drop releases the
pipe without killing the supervisor needed for cleanup.

Startup now waits for guest RPC and checks the exact frozen host-only guest
agent SHA before returning a usable lease. A failure-first regression showed
that wrong-agent admission was possible before this check; it now passes.
The controller is compiled and tested, but candidate execution entry points
still reject VM plans pending transfer/backend wiring. The module's temporary
dead-code allowance is limited to this transitional controller; remove it when
execution uses the API. No existing plan is silently moved to a new backend.

Current focused checks: seven passed, two explicit real-environment probes
ignored in the ordinary suite. One test runs nine Python supervisor cases;
Rust tests cover explicit finish, Drop cleanup and rejecting agent drift.
Logs: /tmp/ca-hrs253-vm-lease-tests4.log and
/tmp/ca-hrs253-vm-agent-attestation-before.log. Strict all-target/all-feature
workspace Clippy, formatting and diff checks pass:
/tmp/ca-hrs253-vm-lease-{clippy2,format,diff}.log.

The Rust controller real-VM test passed in 10.81 seconds, including clone,
boot, actual guest-agent identity, guest command, explicit stop and deletion.
Log: /tmp/ca-hrs253-vm-rust-lease-real.log; retained receipt directory:
work/hrs253-vm-lifecycle/rust-lease-verified. Its fixture reused the already
hashed image inventory after checking exact metadata, with a fresh controller
source identity; it did not edit a saved experiment plan. The earlier owner-
SIGKILL/detached-child probe used the same supervisor implementation. These
are controller checks, not source-efficacy or promotion receipts.

Next: bounded host-to-guest source/worker transfer and expected-artifact import;
then connect all engineering, artifact identity/smoke, worker/provider framing
and host verifier boundaries. Provider calls, secrets, accounting and promotion
remain host-owned. Keep transitional no-host-fallback guards until every relevant
execution path supports the VM. Finish complete source/idle lifecycle and the
remaining accounting/context/release checks before declaring HRS-253 complete.


Controller validation settled: all-feature ca suite recorded 2,643 passes,
one failure and 28 ignored tests. The privilege fake-sudo test expected exit
code 7; it passes unchanged in isolation (1.73 seconds). Preserve both logs:
/tmp/ca-hrs253-vm-lease-ca-all.log and
/tmp/ca-hrs253-vm-lease-privilege-isolated.log. The same fixture failure appears
in earlier repository validation records; no expectation or timeout was relaxed.
The current focused VM checks and strict Clippy/format/diff checks pass. The
prior full-workspace failures remain recorded separately. Both prepared VMs
are stopped, the real controller clone is deleted, and no build/test/VM handle
from this checkpoint remains live. No installed executable or user runtime
configuration was changed. HRS-253 remains ACTIVE and incomplete.


### Bounded VM transfers — 2026-09-23

Added src/harness_vm/transfer.rs, embedded transfer.py and hostile-input tests.
Input packaging reads only an already-audited private checkout or frozen task
workspace. It rejects links, hard links, special files, unsafe/oversize paths,
more than 50,000 entries, files over 16 MiB and source data over 512 MiB. The
uncompressed archive is capped at 640 MiB including headers. Root target/cache/
oracle/reference trees are excluded. The private checkout's Git metadata is
included; callers must never pass the original user checkout. The final
inventory and file bytes are checked again before freezing the archive SHA.
Run this synchronous packaging on a blocking worker with the activity deadline.

The host sends the archive through Tart's stdin transport. A fixed guest helper
verifies the entire stream hash and manually extracts only regular files into
fixed destinations. It rejects traversal, absolute paths, duplicate paths,
links/devices, conflicting parents and source entries under target. Modes are
normalized to 0644/0755 without special bits. Source and its parent stay root-
owned; target is writable by ca-runner. Task workspace contents are writable by
ca-runner beneath a protected parent. A root-owned marker prevents reinstallation.
Raw workers have a separate SHA-bound, exclusive root-owned destination.

Artifact export accepts only the host enum ca/codex-team and uses descriptor-
relative no-follow lookup for every guest path component. It rejects links,
hard links, special files, zero/oversize files and changes during reading. The
host streams at most 512 MiB into a private temporary file, hashes the actual
bytes and requires successful remote completion. This API returns opaque bytes;
it neither runs the artifact on the host nor publishes a source candidate.
Named artifact publication into the private host checkout and complete gate/
worker/verifier routing remain to be implemented.

The existing command-capture helper now has an explicit stdin variant; ordinary
callers still receive null stdin. Both paths retain the same timeout, output
caps and process-group cleanup. VmLease adds an explicit stdin-enabled guest
command constructor. Production VM-plan entry guards remain closed until all
candidate execution routes are supported; no runtime config was activated.

Validation: nine focused Rust tests pass, three real-environment probes remain
opt-in. Embedded suites cover 16 transfer cases and ten supervisor cases.
The first transfer test attempt correctly rejected a fixture path through
macOS /var's symlink; canonicalizing the fixture fixed the two positive cases
without relaxing no-follow lookup. The combined run then exposed poll/killpg's
ESRCH race during cleanup. A deterministic regression fails with the prior
supervisor and passes with the fix: an already-exited process is reaped and
inventory still decides cleanup. Logs: /tmp/ca-hrs253-vm-transfer-python{,2}.log,
/tmp/ca-hrs253-vm-transfer-tests{,2}.log and
/tmp/ca-hrs253-vm-supervisor-race{,-before}.log.

The real Rust transfer test passed in 11.90 seconds. It verified exact input
bytes, source/worker write denial and source chmod denial as ca-runner, writable
task/build output, binary artifact round-trip including NUL/non-UTF-8 bytes,
rejection of a substituted artifact symlink, and whole-VM cleanup. Log:
/tmp/ca-hrs253-vm-transfer-real.log; retained receipt directory:
work/hrs253-vm-lifecycle/rust-transfer-verified in the Codex task. It uses a
small synthetic artifact, not a compiled source candidate or promotion receipt.
Strict all-target/all-feature Clippy and formatting pass:
/tmp/ca-hrs253-vm-transfer-{clippy,format}.log. Broader ca checks are recorded
below when settled. Next connect fixed engineering gates and bounded artifact
publication, followed by worker/provider framing and executable verification.
Do not remove the overall VM execution guard before those boundaries are ready.


Transfer checkpoint settled: the current all-feature ca suite passes all 2,646
tests with four test threads; 29 opt-in tests are ignored. Log:
/tmp/ca-hrs253-vm-transfer-ca-all.log (44.67 seconds after compilation).
The prior default-parallel privilege-fixture failure remains recorded; this
run does not erase that observation. Strict all-feature workspace Clippy,
format and final diff checks pass. All checkpoint process handles are terminal,
both prepared VMs are stopped and the disposable transfer VM was deleted.
No user runtime config, installed executable or original source branch was
changed. Continue with the fixed engineering matrix and artifact publication;
HRS-253 is not complete.


## VM engineering dispatch and receipt binding (2026-09-23)

Guidance engineering and source builds now dispatch fixed gates through the
frozen VM backend. Each gate has a fresh lease and root-owned source; ca-runner
executes the exact fixed command with a clean offline environment. Successful
release gates import only ca/codex-team as bounded opaque bytes and publish them
atomically in the private host checkout without execution. Receipt hashes bind
smoke inputs to the latest release gate. Admitted artifacts require a complete
successful matrix and matching hash; legacy host receipts remain compatible.
Overall evaluation/trial VM entry guards remain closed while worker/verifier
migration is unfinished. No runtime config or installed binary changed.

The tiny-project real 12-gate matrix passes in 148.51s, including guest-only UID,
home and source-protection assertions and fresh-VM artifact smokes. Real current
ca formatting passes in 30.74s. Logs: /tmp/ca-hrs253-vm-engineering-matrix.log and
/tmp/ca-hrs253-vm-engineering-ca-format.log. Retained receipts are in the Codex
task's work/hrs253-vm-engineering. These are not promotion/efficacy receipts.
Focused VM checks pass 11 tests (five ignored), including 16 Python transfer
and 11 supervisor cases. A combined supervisor run exposed a permission error
while terminating an owned child. The deterministic regression fails before
and passes after the fix: PermissionError is acceptable only if waiting confirms
that exact child exited; timeout preserves the clone and fails cleanup.
Logs: /tmp/ca-hrs253-vm-supervisor-permission-{before,after}.log and
/tmp/ca-hrs253-vm-engineering-tests3.log. Strict workspace Clippy and formatting
pass; host ca all-feature checks pass 2,648 tests, 31 ignored (four threads).

The actual current-ca test_all_features gate completed in 523.39 seconds with
3,442 passes, ten failures and 33 ignored tests. Exact fixed command includes
--workspace --all-targets --all-features --locked --offline --no-fail-fast.
Nine failures remain in mcp_sdk/protected_baseline_cli/remote_cli. The additional
jobs::tests::ordinary_required_sandbox_supports_git_and_approved_cached_cargo
failure shows the nested Cargo command inherited CARGO_TARGET_DIR pointing to
/opt/ca-harness/project/target outside its sandbox grant. Remove that unnecessary
gate environment override: Cargo defaults to the same target for the gate itself
while fixture Cargo commands select their own workspace. Correction awaits a
real rerun. Full output: /tmp/ca-hrs253-vm-engineering-ca-all.log. The gate returned
passed:false, cleaned up the owned clone, and left both templates stopped.


## VM worker transport checkpoint (2026-09-23)

The existing worker broker is extracted as harness_execution::run_worker_command.
Host Seatbelt execution still uses it; the VM adapter uses the same bounded
frames, frozen provider/model/options, request counts, hint checks, completion/
exit agreement and timeout cleanup. Only host-owned launcher arguments enter
administrative VM RPC. The guest launcher creates a fresh private home/temp/
build root, clears the environment, drops to ca-runner, applies the closed
Seatbelt profile, and executes the fixed hash-installed worker. Protected source
is unreadable; task and private build directories are writable. Provider access
and credentials remain host-side. The caller retains the lease for bounded
result export and must finish it before admitting evidence. Production trials
are not yet connected; result export and independent fresh-VM verification remain.

The nine-scenario shared broker regression passes: valid duplex exchange,
oversized/truncated frames, invalid sequence, unknown fields, ungranted tools,
excess requests, inconsistent exit and deadline. Log:
/tmp/ca-hrs253-worker-transport3.log. Two prior test attempts failed because the
synthetic reply expectation used the wrong ProviderEvent serialization and the
EOF diagnostic assertion used the wrong wording; production checks were not
relaxed. The synthetic real VM worker now passes in 10.99s, including two
provider exchanges, non-root identity, fixed guest workspace/home, task writes,
worker/source write denial, protected-source read denial and whole-VM cleanup.
Log: /tmp/ca-hrs253-vm-worker-real3.log. Initial VM attempts failed because the
positive fixture assumed a source file existed in the base template. The test
now installs its synthetic protected source explicitly before testing denial.
It does not reinterpret a missing file as a successful protection check.

A real freshly built ca worker probe and current broad validation are pending.
These synthetic transport results are not a complete VM trial or promotion.


## Corrected VM gate and bounded results (2026-09-23)

Removing the inherited gate CARGO_TARGET_DIR corrected the nested Cargo sandbox
failure. The actual fixed all-target/all-feature workspace gate now reports
3,445 passes, nine existing integration failures and 35 ignored tests in
555.73s. All 2,650 ca unit tests passed, including the original failing fixture.
The gate still returns passed:false because mcp_sdk (five), protected_baseline_cli
(three) and remote_cli (one) fail. No failed gate was admitted. Log:
/tmp/ca-hrs253-vm-engineering-ca-all2.log; retained ca-all-gate-corrected.json
under the task's work/hrs253-vm-engineering. The owned clone was cleaned up.

The real current ca worker passes editing, three host provider exchanges,
protected-source read denial and cleanup in 36.68s:
/tmp/ca-hrs253-vm-worker-ca-real.log. Shared protocol regression and the current
host suite passed 2,650 tests (33 ignored). Subsequent bounded result work adds
nine Python snapshot cases and Rust import/grant checks. results.py walks
pinned directory descriptors with no-follow opens; rejects links, hard links,
special files, changed identities and limits of 80 entries, eight levels and
1MiB total. The wire representation is capped at 5MiB. Host decoding rejects
unknown/duplicate paths and applies the same FrozenCodingCase edit grants as
local snapshots before materializing only into a new private directory.

The real VM result import passes in 13.15s, including exact edited bytes and
rejection of a guest file replaced by a symlink. Log:
/tmp/ca-hrs253-vm-results-real.log. The shared byte exporter retains remote exit,
timeout and byte bounds. No imported byte is executed on the host. Current
focused VM checks pass 14 tests (eight ignored). Host ca checks pass 2,652 tests
(35 ignored), strict workspace Clippy and format pass:
/tmp/ca-hrs253-vm-results-{ca-tests,clippy,format}.log.

The independent verifier now starts a separate fresh VM, installs only the
pinned baseline binary and checked task bytes, drops to ca-runner, and uses the
existing trusted verifier's per-compiler/probe sandbox. The baseline owns hidden
checks; candidate worker children cannot survive into this VM. The host retains
bounded receipt decoding, clean exit and exact case/verifier/implementation
bindings. Both worker and verifier require whole-VM cleanup before returning.
The trial runner now selects these paths and reserves 60 seconds for VM
verification (legacy host reserve remains eight). Overall evaluation/trial VM
entry guards remain until identity/activation routing and combined validation
are complete. The six-bug/twelve-guest verifier probe is running, followed by a
complete worker/export/fresh-verifier probe. No runtime activation or install.


Verifier fixture matrix settled: all six buggy inputs and their six repairs
passed the expected independent checks in twelve disposable VMs (323.94s).
Each buggy fixture passed public checks and failed hidden checks; each repaired
fixture passed both. Log: /tmp/ca-hrs253-vm-verifier-real.log. The baseline was a
fresh all-feature debug build, copied privately, stripped of debug symbols and
ad-hoc signed; its exact hash was verified on each guest transfer. It was not
installed. Current trial-selection code compiles; its full host ca suite passes
2,652 tests, 36 ignored. The combined real VM trial probe is running separately.

Follow-up inference to test before source activation: VM admission currently
binds adapter_digest to the whole CA_SOURCE_DIGEST. A promoted source binary
necessarily changes that identity even when all protected VM/controller inputs
are identical. New-binary recovery/rollback of an older frozen plan must be
explicitly tested when routing activation smoke checks through VMs. Preserve
saved-plan immutability and reject actual adapter changes; do not silently
rewrite a saved plan's adapter digest. This is an implementation compatibility
question, not a new user restriction. Main VM entry guards remain until that
boundary and the remaining activation paths are handled.


VM trial checkpoint settled: the complete real worker edit/export/cleanup and
independent fresh-VM verifier probe passes in 62.17s, with two local fake-provider
requests. The original host task remains unchanged; only validated imported
bytes are verified, and both public/hidden checks pass. Log:
/tmp/ca-hrs253-vm-trial-real.log. This tests one frozen fixture composition; it is
not a full 54-trial cycle, paid-provider efficacy result or activation receipt.
The independent verifier matrix separately passed all six bugs and six repairs
in 323.94s (/tmp/ca-hrs253-vm-verifier-real.log).

Current focused VM suite: 14 pass, nine opt-in probes ignored. Current all-feature
ca unit suite: 2,652 pass, 36 ignored (45.98s, four threads). Strict locked
workspace all-target/all-feature Clippy, formatting and diff checks pass. Logs:
/tmp/ca-hrs253-vm-trial-routing-focused2.log,
/tmp/ca-hrs253-vm-trial-ca-tests.log,
/tmp/ca-hrs253-vm-trial-{clippy,format}.log.
The known nine full-workspace integration failures remain as recorded above.
The current debug baseline builds successfully; private test copies are signed,
never installed. No real-provider call, user runtime configuration change,
original-branch adoption or installed-executable replacement occurred.

Next migrate harness_activation identity/smoke and harness_promotion installed
postchecks to the selected VM path; explicitly cover adapter identity across
source upgrade/recovery/rollback. Only then remove evaluation/trial entry guards
and run complete source/idle lifecycle checks. Finish provider-specific context
proof, unknown-charge recovery and the user-authorized configuration/release
installation. HRS-253 remains ACTIVE; no user input currently blocks progress.


## VM activation probes and recovery identity (2026-09-23)

A failure-first test proved that binding VM recovery to the entire source digest
rejects ordinary application repairs. The adapter now uses a compiled digest of
protected control inputs plus manifests, lockfile and build configuration.
The protection list lives in one pure module shared by build.rs and runtime
source grants; it has not been broadened or relaxed. Application build identity
remains separate and continues to hash all build inputs. Saved VM plans are not
rewritten: changed control/dependency inputs still reject admission. Only new
test fixtures rebind the already-hashed image to the currently tested adapter.
The regression passes after ordinary application edits and rejects adapter,
build-helper, manifest and lockfile edits. Logs:
/tmp/ca-hrs253-vm-authority-{before,after}.log. Four shared source-adoption and
protected-input tests pass (/tmp/ca-hrs253-vm-authority-core-tests.log).

Fixed version/help/compiled-identity probes now upload exact signed bytes and
run as ca-runner under Seatbelt in a fresh VM for each invocation. Each probe
requires whole-VM cleanup before returning. Activation preparation and installed
promotion/rollback postchecks dispatch to this backend when the frozen plan
selects it; legacy no-VM plans retain their prior host sandbox. A real synthetic
probe passes non-root, fixed private home and fresh-state assertions across all
three flags in 33.63s (/tmp/ca-hrs253-vm-probes-real.log). Unknown flags are rejected
before startup. No imported candidate executable is run on the host by VM-plan
preparation/postchecks.

The VM source build/adoption/activation/rollback transaction is currently being
validated with a tiny disposable project. Its trial receipts are controlled test
inputs, not measured improvement evidence. Main evaluation/trial entry guards
remain until this transaction settles and backend admission is wired. Current
focused VM checks: 15 pass, 11 ignored. Host all-feature ca: 2,653 pass, 38 ignored
(57.29s). Strict Clippy initially found the shared build-only helper unused in
an integration test; a scoped allowance fixes that intentional shared-module
case. The rerun, format and diff checks pass. Logs:
/tmp/ca-hrs253-vm-activation-{focused2,ca-tests,clippy,clippy2,format}.log.


Activation/admission checkpoint: the tiny-project VM source transaction passes
in 287.10 seconds. It runs baseline/candidate builds and the fixed engineering
matrix through VMs, signs/imports the exact artifacts, adopts the private source,
activates, probes the installed copy in VMs, and restores executable/source with
additive rollback. The ledger records 275 active seconds and no unsettled work.
Log: /tmp/ca-hrs253-vm-promotion-real.log. Trial receipts in this transaction are
test-controlled; do not call it a complete measured 54-trial cycle. Only its
private repository/install path changed.

The temporary require_host_backend guards are removed. Main evaluation and trial
entry points call validate_backend, which checks the immutable plan's authority
and frozen assets on a blocking worker with a bounded deadline. Wrong authority
or changed assets fail before candidate execution; there is no host fallback.
Each subsequent execution independently rechecks its lease. Focused admission
suite passes 15 tests, 11 opt-in tests ignored:
/tmp/ca-hrs253-vm-admission-focused.log. The actual authority compatibility test
compares ordinary application edits with protected/dependency edits; a promoted
real-ca process recovery remains part of the full source lifecycle validation.
Both templates are stopped and no disposable clone remains. Final admission
unit/lint/format checks are running; no user runtime setting or installed binary
has changed. Continue provider context, unknown-charge recovery, full automatic
source/idle lifecycle, bounded live validation and authorized release/install.


Final admission checks settled: 2,653 ca tests pass, 38 ignored (45.48s, four
threads); strict locked workspace all-target/all-feature Clippy, format and diff
pass. Logs: /tmp/ca-hrs253-vm-admission-{ca-tests,clippy,format}.log. All checkpoint
handles are terminal. The nine previously recorded integration failures remain;
no current-source full workspace release/install has been claimed. VM execution
migration is connected through activation/rollback, while full automatic source/
idle evaluation, recovery/accounting and live-provider validation remain open.


### VM ownership through cleanup — 2026-09-23

Implemented a private per-Tart-home flock, held before VM validation/startup.
A close-on-exec duplicate at descriptor >=64 is inherited only by the supervisor
through its pre-exec hook; the parent descriptor remains close-on-exec. Tart
subprocesses do not inherit it. Parent and supervisor release by closing, never
LOCK_UN or unlink. Parent ownership lasts through final verification, while
supervisor ownership survives caller cancellation and lasts through cleanup.
This avoids changing the shared LearningVault lock semantics or threading its
lock through every activity. Startup inventory also rejects running or uncertain
harness leftovers after a supervisor failure. Stopped retained clones are inert.

Failure-first inheritance check failed before the pre-exec implementation:
/tmp/ca-hrs253-vm-ownership-before.log. It now proves the lock survives parent
release, excludes another owner, is absent in the supervisor's subprocess, and
releases on supervisor exit. Linked/shared lock checks and leftover-inventory
checks pass. Focused VM suite: 18 pass, 11 opt-in ignored:
/tmp/ca-hrs253-vm-ownership-focused.log.

Final source all-feature ca run: 2,655 pass, one failure, 38 ignored (49.07s):
/tmp/ca-hrs253-vm-ownership-ca-tests.log. The unrelated
external_runtime::tests::cancelled_download_drops_staging_and_never_registers_the_runtime
fixture reported an early request disconnect; it passes unchanged in isolation:
/tmp/ca-hrs253-vm-ownership-runtime-isolated.log. Preserve both outcomes.
Strict locked workspace/all-target/all-feature Clippy, format and diff pass:
/tmp/ca-hrs253-vm-ownership-clippy.log,
/tmp/ca-hrs253-vm-ownership-format.log. Test linking reports the existing large
unwind-section performance warning, not a test or lint failure.

The actual dedicated VM startup, exclusive ownership, guest-agent attestation,
stop/delete and ownership release pass in 16.67s:
/tmp/ca-hrs253-vm-ownership-real.log. Both templates are stopped; no disposable
clone remains. All checkpoint processes are finished. No runtime config,
installed executable or original source adoption changed.

Next: acquire the artifact-worker lock plus VmOwnership and inspect frozen Tart
inventory before recovering a dead pre-decision cycle. Preserve unknown money
and time reservations as conservative unknown bounds in an immutable recovery
record; do not call them actual spend, refund them, or resume a rejected cycle.
User pauses and decision-bearing promotion/recovery states must remain separate.
Full source/idle/live-provider validation and final configuration/install remain.


### Interrupted-cycle closure and full VM lifecycle launch — 2026-09-23

Added a distinct whole-driver HarnessControllerLock, retained across idle
preparation and worker boundaries. Recovery acquires controller, artifact-worker
and VM ownership before verifying the frozen backend and absence of activation
journals. Startup now rejects even stopped leftover disposable clones: a
surviving Tart runner could still start one after supervisor death. Successful
cleanup removes the clone. This corrects the previous stopped-leftover assumption.

HarnessAbandonment binds both the prior and closed cycle digests. It changes the
stage to Rejected while retaining all money/time reservations, settled usage,
trials, prior reasons and pause metadata. Subsequent mutation is rejected; loaded
records validate the binding. No request replay, settlement, refund or promotion
is inferred. `ca harness abandon` exposes explicit closure; idle ownership scopes
automatic closure to its own profile/repository. User pauses, settled foreground
resumption, decision-bearing states, activation journals and cooldown are kept.
Legacy host-only plans cannot use this VM proof.

Validation: four recovery tests pass, covering ownership exclusion, orphan
inventory, preserved spending/time, immutable/tampered records, user pauses,
decision states, idle scope and 24-hour cooldown:
/tmp/ca-hrs253-recovery-focused2.log. All-feature ca: 2,660 pass, 38 ignored in
50.26s (/tmp/ca-hrs253-recovery-ca-tests.log). Shared harness ledger: 14 pass
(/tmp/ca-hrs253-recovery-core-tests.log). Strict locked workspace all-target/
all-feature Clippy, format and diff pass (/tmp/ca-hrs253-recovery-{clippy,format}.log).
The previously failing external-runtime cancellation fixture passes in this run;
the earlier failed receipt remains. Nine known workspace integration failures
have not been relabeled as passing.

Added opt-in hrs253_vm_guidance_cycle_evaluates_activates_and_rolls_back. It passes
the selected real VM profile through production image rehash/freezing, then runs
all 54 measured local-provider trials, independent guest verification, CLI
activation and rollback. No saved plan/digest is replaced. The fresh optimized
workspace build is running as session 37868 with log
/tmp/ca-hrs253-final-release-build.log. The release-mode test command is queued
behind it as session 86980 with log /tmp/ca-hrs253-vm-full-cycle.log. Its exact
module is harness_freeze::integration_tests; an earlier command with the wrong
exact module filter was terminated while waiting for the build lock, before any
test ran. Do not restart either live command based only on a quiet log. No final
installation or user configuration change has occurred.


Release/live-pricing update: session 37868 completed successfully in 2m32s.
Fresh target/release/ca is 84,022,608 bytes, --version succeeds and codesign
reports valid on disk. It is not installed. Session 86980 compiled the optimized
VM lifecycle test in 4m27s and is now running that one test; PID 83003 was observed
actively hashing the selected image. Do not restart or replace its baseline.
Follow-up formatting passes (/tmp/ca-hrs253-recovery-format2.log).

The default live route is deepseek/deepseek-v4-pro (--list-models). All nine
saved price cards are stale. Official refresh failed without sending inference:
/tmp/ca-hrs253-pricing-refresh.err. The official DeepSeek page now publishes
Pro peak cache/input/output rates $0.044/$1.32/$3.96 per million (half off-peak),
where the existing seed/validator expects old rates. Official receipt:
task work/hrs253-recovery/deepseek-pricing.html, fetched from
https://api-docs.deepseek.com/quick_start/pricing/. This is a concrete accounting
completion issue, not a reason to bypass price validation. Resolve it and the
configured-versus-official context reservation bound while the VM test runs.
No live provider request, successful price refresh, runtime config change or
installed executable replacement has occurred.
