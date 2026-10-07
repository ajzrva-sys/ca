# Harness evidence and experiment controls

Implementation status: complete. Automatic experiments are enabled in the
selected dedicated macOS VM with $20 and 7,200 active seconds per cycle.
Evaluated guidance/source candidates can activate and roll back through durable
receipts. Promotion still requires passing engineering and matched-task checks.
The current checkout has nine existing integration failures and uncommitted work,
so it is not eligible for source promotion. Controlled tests demonstrate the
mechanism; real-provider improvement is not claimed.

VM controllers serialize on a private lock in the selected Tart home. Both the
parent and its independent supervisor retain ownership, so cancellation cannot
start another guest before cleanup ends. The lock file remains in place between
runs. Startup requires all earlier disposable clones to have been removed, even
if inventory says stopped: a surviving Tart runner could still be starting one.

`harness abandon CYCLE_ID [--json]` closes an interrupted VM experiment before
its promotion decision. It requires exclusive controller, worker and VM
ownership, unchanged frozen inputs, and no leftover guest or activation journal.
The rejected cycle retains every unknown money/time reservation, actual settled
usage, and existing evidence. Its closure is digest-bound and immutable; it
cannot resume, replay a request, or refund budget. Status distinguishes this
closure from ordinary rejection. A later cycle still obeys the 24-hour cooldown.

Idle execution applies this recovery only to its own profile/repository cycle.
User pauses remain paused; settled foreground pauses resume normally. Promotion
and rollback recovery retain their existing journal paths. Legacy host-only
plans cannot use this VM quiescence proof and remain available for inspection.

`harness discover [--json]` previews unused failure groups from the current build.
It requires at least three attributable failures across two independent tasks in
one runtime/tool cohort. Every cycle's frozen evidence is excluded, including
rejected cycles. Reads are bounded and reject links. Family labels are diagnostic
hypotheses from fixed error signatures, not established root causes; unknown or
ambiguous signatures remain unclassified. Referenced source paths are suggestions
that still need source-freezer checks. Discovery makes no model request and does
not create a cycle or start idle execution.

`harness create` freezes evidence, model route/pricing, source, executable and nine
coding cases before generation. Repeated `--source-file` options select a source
experiment. `run` generates and evaluates the candidate under durable spend/time
limits. Promotion requires the full fixed engineering matrix and matched trials
with no regressions, successful held-outs and a verified improvement.

`harness create --repository /path/to/ca --auto` selects unused evidence matching
the configured provider/model/route and a supported evaluation-family hypothesis.
It freezes six fixed regressions plus a distinct target and two undisclosed
same-family implementations from the host-owned catalog. Explicit case/evidence
and source-file options cannot be combined with `--auto`. Source edits are
selected only when the diagnostic names editable files in that same clean ca
checkout; otherwise the cycle evaluates guidance. Discovery and preparation time
are charged when the frozen cycle is recorded. This is automatic selection for
an explicit creation command.

`[harness_improvement]` accepts `enabled` (default false) and an optional absolute
`repository` path (default: the source directory embedded in the build). When
enabled, the TUI supervisor waits five uninterrupted quiet minutes, requires
network capability, and avoids overlap with learning maintenance. It selects the
configured default provider/model route, enforces one outstanding cycle and a
24-hour admission cooldown, then uses the same generation/evaluation/activation
runner as the CLI. Foreground activity pauses at settled operation boundaries;
a request or atomic adoption already underway can finish. Only its own
foreground-paused cycle can resume automatically, with the same profile, repository
and executable. User-paused cycles remain paused. Shutdown signals foreground
activity and waits up to five seconds for the async worker, then aborts it; an
already-started blocking preparation checks the same signal between file operations
and private Git polls, discarding unpublished snapshots when cancelled. An individual
filesystem call can still delay shutdown. Activity
detection covers this TUI, not all running ca instances. The configured user profile enables this for the next TUI launch.

For an eligible source plan with automatic promotion enabled and a frozen
executable destination, `run` adopts the candidate on the unchanged clean branch
and replaces that executable. It retains the baseline and a separately signed
candidate on the destination filesystem. Signature, hash, mode/owner and offline
help/version checks bind the installed result. Existing running processes are not
restarted. Legacy plans without a destination cannot activate an unspecified binary.

`rollback` restores the baseline executable first, then creates an additive source
revert. Later binary changes defer replacement. Dirty source or a later commit
defers the source revert without overwriting user work; repeating `rollback` can
continue once that condition is resolved. Failed post-install checks attempt this
same rollback automatically. Journals distinguish source adoption, binary activation,
binary restoration and completed rollback. `show --json` exposes the receipts.
Recovery charges interrupted host work at its reserved maximum; unrelated unknown
provider/activity charges remain blocked. Promotion retains a rollback time allowance.

Guidance plans default to the currently active repository-work paragraph in the
built-in system prompt. An explicit guidance `--instructions` file must match that
paragraph. Successful `run` publishes its evaluated replacement for the same
build, provider endpoint, model and options. Requests use it before recording
harness fingerprints; custom prompts without exactly one matching paragraph keep
their own instructions. Analysis and frozen trials do not inherit the active policy.
`rollback CYCLE_ID` restores the previous guidance without needing a repository
argument. A newer published version is preserved. `show --json` includes the
activation journal. Guidance does not change source or executable files.

The isolated coding worker, host verifier and process tests cover all six reasoning
families. The final dedicated-VM guidance lifecycle passes all 54 scheduled trials
with 51 verified successes and three expected baseline failures, then activates
and rolls back. The measured hint lifecycle and real-clock idle dispatch,
generation, foreground pause and shutdown also pass. A VM source transaction
covers build/adoption/activation/rollback with controlled trial receipts. These
composed checks do not make the nine failing full-workspace checks pass.

Experiment requests have independent input-size checks before budget reservation
and on the final protocol body before transport. The byte cap is the smaller of
the configured cap and the verified provider context ceiling. Monetary
reservations use the provider ceiling independently, so a smaller request cap
does not underreserve server-side context accounting. Unsupported routes fail
closed. These experiments accept text only; byte checks do not claim to reproduce
the provider tokenizer.

`ca pricing refresh --provider deepseek --json` refreshes only the selected
provider from its official source while retaining unrelated historical cards.
Current DeepSeek cards use peak-rate upper bounds, explicitly labeled in usage;
accounted amounts are not provider invoices. Unknown usage remains reserved.

Shutdown drains admitted evidence off the async event loop with a bounded deadline.
Queue or storage failure is reported and marks subsequent evidence incomplete;
incomplete evidence cannot establish verified success or enter stability cohorts.
Headless early exits retain interruption even when normal finalization is skipped.

Normal completion records execution completion, not user acceptance. Each task
has its own ID, verification state and explicit feedback. Delegated children record
their parent and root task IDs, actual request fingerprints and bounded tool
evidence. Analyze and cycle creation count independent root tasks, so siblings
cannot inflate failure frequency. Cancellation retains an interrupted outcome.
Host verification stores focused-check status, scope and receipt identity. Model
review reports are separate events and cannot establish host verification. A
focused pass does not establish whole-task success. Potentially mutating tools
(including shell and hashline edits), known file mutations and undo invalidate
earlier checks; denied tools do not. Legacy records remain
readable but cannot establish a verified positive outcome. TUI, headless and
service turns collect bounded privacy-filtered events; interrupted service tasks
retain an interrupted outcome. Effective prepared-request inputs have separate
fingerprints, and a task records changed input epochs. Build identity includes
source, embedded prompts/schemas, workspace manifests, verifier source, enabled
features, compiler settings and compiler version. Trial execution separately pins
the exact executable digest.

Use `/feedback accepted`, `/feedback rejected`, or `/feedback unknown` after a
TUI task. For a recorded task, use:

```text
ca harness create --help
ca harness outcomes 20
ca harness feedback TASK_ID accepted --json
ca harness stability
ca harness analyze
ca harness status --json
ca harness candidates --json
ca harness show CYCLE_ID --json
ca harness pause CYCLE_ID --json
ca harness resume CYCLE_ID --json
ca harness rollback CYCLE_ID --repository /path/to/ca --json
ca harness run CYCLE_ID --repository /path/to/ca --baseline-binary /path/to/frozen-ca --json
```

`create` takes a ca repository whose build inputs match the running build, optional baseline
instructions, a target fixture and two distinct held-out fixtures from the same
supported verifier family. Select at least three failed `tool_evidence` event IDs
across two independent root tasks in one build/route cohort. Each must link to a failed execution
receipt. Fresh verified pricing and the configured model's actual context limit
are required. Creation freezes inputs and makes no model request. It prints the
cycle ID for `run`, which starts budgeted generation and evaluation. Use `--auto` instead of manual case/evidence arguments for host-owned selection.

Guidance creation accepts uncommitted source matching the running binary. It copies
bounded build and engineering inputs into a private Git snapshot and charges the
freezing time to the cycle. It preserves the original branch, index and working
files. Root runtime files and oracle/reference trees are excluded. Evaluation and
promotion use the private snapshot, so later edits in the original checkout do not
change the experiment. Actual snapshot bytes are rechecked alongside its Git
identity. Source-changing experiments still require the original clean checkout.

Without `--source-file`, creation selects guidance. To select Rust source, add
`--source-file src/loop_guard.rs` (or another unprotected repository-relative
source path), repeated for up to eight files. The total selected source is capped
at 256 KiB. Creation freezes those bytes and their digests; `run` checks them
again before its first model request, then generates, privately commits, builds
and evaluates the source candidate. Neither file selection nor candidate type
can change after creation.

Headless task IDs match their output run IDs. `show`, `pause` and `resume` require
an existing experiment; these commands do not generate or execute candidates.
Resume preserves trial receipts, unresolved spend reservations and elapsed time.
`run` loads the saved generation, recovers completed candidate admission, and
executes remaining frozen trials through the configured provider. It requires
the exact frozen binary and route and fresh pricing. It does not implicitly
resume a paused cycle. Infrastructure failures stop further trials; completed
guidance trials run the fixed release checks in a private clone with an offline
sandbox and cleared environment. Source candidates pass that matrix before
evaluation; their source and artifact are rechecked before the final decision. A changed private guidance snapshot or dirty source-experiment baseline cannot start these
checks. Failures are saved; interrupted checks retain reservations. Source and
artifact reinspection precedes reuse of passed checks. Eligible candidates activate
through the journaled promotion paths described above.
`show` includes the generation prediction and engineering receipts without
held-out implementation bytes.
The older `harness` subcommand is a disk inventory, not proof of active runtime
inputs. Runtime snapshots are recorded from actual prepared requests.

Analysis must cite supplied event IDs. Frequency is the number of distinct cited
tasks. Missing, repeated or invented references are rejected; causal explanations
are hypotheses. Truncated task evidence is not supplied as complete evidence.
Stability rates are chronological and restricted to comparable workspace,
model/provider and harness cohorts. Date-based manifest promotion is removed.

The experiment library freezes a content-addressed plan, three repetitions per
case, a predicted-fix case and at least two held-out cases. Receipt admission
follows alternating baseline/candidate order. Missing trials or infrastructure
failures produce no causal verdict. Promotion eligibility requires engineering
checks, no per-case regression, all held-out checks passing, and either two
additional successful target runs or a 10% median cost/latency reduction with
all outputs verified equivalent. This library verdict alone does not activate
anything.

Spend is represented in integer millionths of a dollar. Reservations and
settlements are written under the vault lock. An unresolved reservation remains
charged against the available budget after restart. The default policy is $20,
two active hours, five idle minutes and a 24-hour cooldown. The provider broker
enforces per-request spend and time. The shared activity clock covers surrounding
work for guidance/source generation, coding trials and engineering checks.
Automatic discovery and snapshot preparation are charged when the cycle is
frozen. Unknown usage and interrupted work retain their reservations. A fully settled cycle that
exhausted its budget before recording a promotion decision permits a later cycle;
its evidence and accounting remain saved. Cooldown and evidence exclusion still
apply to automatic selection. Exhausted cycles with a saved decision or unresolved
work continue to block admission pending recovery.

Execution observations identify repeated dependency probes, foreground waits
and build-only checks. A candidate can add a bounded intervention as described
below; runtime use requires that candidate to pass the matched evaluation and
activation gates.

Completion evidence and remaining workspace limitations are recorded in
`exec-plans/completed/HRS-253-evidence-driven-harness-improvement.md`.

Hint candidates may now add one host-owned intervention while leaving the guidance
text unchanged: dependency probes, foreground waits, or behavioral verification.
Frozen inputs and candidate hashes bind the baseline and candidate policies. The
isolated worker and runtime use the same completed-call transformation, with at
most two reminders per user turn and no repetition after the triggering batch.
Denied, unfinished or ambiguous calls do not qualify. Runtime fingerprints include
the active policy even before a reminder fires.

The host checks the worker's transformed system message and records delivered
hints in its verification artifact. Activation requires the added hint to have
been delivered in every candidate target repetition, in addition to the ordinary
matched-outcome and engineering gates. Missing exposure cannot become evidence
of improvement. Runtime consumption rechecks the evaluated candidate hash;
rollback restores the previous policy as well as its text. No policy is enabled
in the user vault during implementation tests. Real-worker trigger tests pass;
the complete 54-trial hint lifecycle also passes with a deterministic local
provider that repairs the target only after receiving the reminder (124 requests,
activation and rollback). This is a controlled integration result, not a claim
that the intervention helps real provider tasks.

Source activation preserves the guidance text and hint policy used in both trial
lanes. Preparation reads the signed candidate’s compiled identity offline, and
binds the new build scope, provider route and evaluated policy into its journal.
Runtime consumption requires a settled, promoted source transaction. Rollback
invalidates the new build’s policy without changing the previous build’s pointer.
Publication refuses to overwrite a different policy already saved for that build.
Four disposable-project activation/recovery tests pass; they use synthetic trial
receipts and do not establish source-candidate coding performance.

The dated checkpoints below are historical; the completion status above and
completed plan supersede their pending work and installation state.

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
The user subsequently selected a dedicated macOS VM; its validation is recorded below.


### Dedicated macOS VM environment — 2026-09-23

The user selected a dedicated macOS VM. A separate Tart 2.32.1 store lives at
`/Volumes/vm/ca-harness-tart`, with prepared template `ca-harness-base` and a
separate validation clone. The guest is macOS 26.6.2 (25G83), with four CPUs,
12 GiB RAM, a 192 GB virtual disk, Rust 1.97.1, locked vendored dependencies,
and ripgrep 15.2.0. No original checkout, vault, or credential directories are
shared. Clipboard and audio are disabled. Tests run as the non-admin ca-runner
account. The root-owned guest packet filter permits loopback and blocks other
traffic; host networking settings were not changed.

Cold boot, restricted account, offline compilation, localhost listeners and
nested sandboxing pass. The base is stopped after provisioning. The first full
workspace run found missing ripgrep and Homebrew Python being denied by the sandbox. Installed the missing tool and selected system Python through the guest
PATH; test expectations and source were unchanged. The corrected default run
passes all 2,608 ca unit tests. Default workspace totals are 3,281 passed, 10 failed and 27 ignored. Corrected
all-feature totals are 3,429 passed, 11 failed and 28 ignored; these runs do not
constitute a passing engineering matrix. The active plan records setup failures,
corrections and the retained test failures.

A deliberately detached child (its own session, heartbeat nonce recorded outside
the VM) stopped when the whole VM stopped. After reboot its unique command was
absent and its heartbeat file remained unchanged. The first inspection matched
its own query text; corrected matching requires the child's separate final nonce
argument. This proves whole-VM shutdown cleanup, not the production harness's
current process-group cleanup. The production engineering/trial backends have
not yet been switched to Tart. Environment-probe receipts cannot authorize
promotion. VM provisioning does not close the remaining source lifecycle,
provider-context, recovery or release-validation work in the active plan.


The prepared template also uses a private guest-agent build that accepts only
kernel-identified host peers on its administrative RPC socket. Guest-local RPC
is rejected, attempts to bind the host CID fail, and host RPC remains usable
after rejection. The exact agent, patch, license and verification receipt are
retained under /Volumes/vm/ca-harness-tools/guest-agent-host-only-ab2159ca9ac4.
Cold-boot checks verify the installed binary and firewall. Source protection uses
root ownership with normal file modes, so copied fixtures remain writable without
granting the test account writes to frozen inputs. Both VMs are stopped. These
checks establish the prepared environment; production backend integration and
the overall HRS-253 plan remain unfinished.


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


The owned VM controller is now implemented in src/harness_vm/lease.rs and its
embedded supervisor. Seven focused checks pass (two opt-in probes skipped),
including nine Python lifecycle cases and Rust finish/Drop/drift coverage.
Wrong guest-agent admission failed before the startup identity check and now
passes. Strict all-feature workspace Clippy, format and diff checks pass. The
actual Rust clone/boot/agent-attestation/stop/delete probe passed in 10.81 seconds;
/tmp/ca-hrs253-vm-rust-lease-real.log. Candidate paths still reject VM plans until
bounded transfer/import and complete execution routing are wired. See the active
plan's Owned VM controller entry for exact receipts and remaining work.


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


VM source/workspace/worker transfer and named artifact import are implemented.
Nine focused checks pass, including 16 hostile-transfer and ten supervisor
cases. Actual guest source/worker protection, writable task/build output, exact
artifact bytes, artifact-symlink rejection and cleanup pass in 11.90 seconds.
Strict all-feature workspace Clippy and formatting pass. A poll/kill race found
during these checks is fixed with a failure-first regression. See the active
plan's Bounded VM transfers entry for limits, exact logs and the initial failed
fixtures. Candidate execution still fails closed pending fixed-gate routing,
private artifact publication, worker/provider framing and verifier migration.


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
