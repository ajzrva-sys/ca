# ORC-258 — Oracle-informed improvements

Status: ACTIVE — source and integrated checks pass; duration, model and installation gates open

## Objective and delivery boundaries

Implement the user's approved ten workstreams as separately reviewable milestones.
The current checkout has no usable Git metadata. Recoverable source archives and
SHA-256 manifests in `scratch/oracle-improvements/` establish the content baseline;
oracle and reference manifests establish their read-only boundary. The approved
feature contracts are recorded in the 2026-10-05 implementation request and the
review evidence is at `../../../../scratch/oracle-feature-review-2026-10-05.md`.

Default behavior: lexical recall, eager tools, automatic typed-edit recovery in
saved sessions, one compaction quality repair, external dashboard observation
only. Recovery storage errors continue edits with visibly incomplete coverage.

## Milestones

Checked M1–M7 mark implemented source and focused/integrated fixture coverage.
Actual duration and real-model acceptance remain M8 release gates; overall status
is ACTIVE and installation has not occurred.

- [x] M1: current baseline, fresh-path BUG-257 evidence, checkpoint validation and repair.
- [x] M2: stable advisory session ownership, optimistic storage validation, owner-scoped hook outcomes.
- [x] M3: durable recovery, plan feedback, family navigation and bookmarks/handoffs.
- [x] M4: local/external dashboard snapshots and compaction/child lifecycle hooks.
- [x] M5: ordered native read concurrency and optional authorized tool discovery.
- [x] M6: deterministic recall evaluation and optional registered local embeddings.
- [x] M7: audit and complete AUT-231 heartbeat execution, dispatch budgets, cockpit and navigation.
- [ ] M8: integrated tests, formatting, strict Clippy, release, actual TUI paths,
  responsiveness/soak and performance gates, oracle byte verification, recoverable signed installation.

## Shared invariants

Session writer leases cover active turns through their final commit, using stable
lock files that are never unlinked. Stored writes validate UUID, history
generation/digest, and metadata revision. Read-only observation acquires no
writer ownership. Hook results are consumed only by their exact owner. Recovery
intents reach durable private storage before mutations; pending/conflicting
states remain visible after restart. Providers and tools retain existing mode,
profile, approval, sandbox, child and workflow authority.

## Validation and evidence

Every milestone adds focused failure-first fixtures and records actual results.
Use disposable profiles and fake providers/clocks; do not mutate real runtime
configuration, sessions, schedules, or oracle bytes. The historical BUG-257
eight-hour receipt is still `running` and references an absent nested checkout;
it is not a duration pass. `target/release/ca` is absent at baseline and must be
rebuilt. The historical installer has stale hard-coded paths and hashes and is
not a usable fresh installer. Resolve binaries and source paths from this checkout.

Release requires the user's full gates, including ordered/cancellable read
overlap, at least 30% lower lazy schema overhead, bounded hybrid retrieval with
identifier parity and improved paraphrase/multilingual recall, full applicable
workspace checks and long-session evidence. Blocked/pre-existing failures are
reported individually. Installation follows AGENTS.md standing authorization
with a freshly resolved candidate, rollback copy, signing and hash/version/help
verification, leaving running sessions alone.

## Progress

Latest 2026-10-06 01:34 UTC:

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

2026-10-05: audited current paths and missing Git/plan-template metadata; baseline
capture started. Checkpoint, ownership/recovery, and hook/concurrency work are
assigned to separate implementation owners. Root owns integration, plan feedback,
branch/dashboard orchestration and automation completion.

2026-10-05 21:40 UTC integration: baseline source snapshot and separate oracle
manifest captured at `scratch/oracle-improvements/20261005T211220Z/` (1,276
source files; 43,046 oracle/reference files). Initial integrated `cargo check`
passed; subsequent checks sampled edits in progress and remain queued for a
consolidated stable-source run. Focused results reported by implementation
owners: 38 context, 17 memory, 6 discovery, 23 hooks, and 14 recovery/undo cases
passed. Local supervised embedding fixture initially could not bind localhost
inside the default sandbox and passed with the approved localhost environment.
These focused runs do not establish a green workspace suite or performance gate.

Connected user paths: plan comment ledger (including selected Unicode/wrapped
quotations with explicit ambiguity refusal), once-only Submit revision queue,
`/recover` list/preview/exact-token restore and durable rescue turn, `/branches`
(`/tree`) family pages and save-before-switch, `/bookmark` completed-boundary
anchors, and inert branch summary previews with explicit accept/cancel. Local
all-pane/child dashboard snapshots and observation-only verified external
presence now refresh through a process worker. Typed tools receive parent-owned
recovery contexts. Read batches and lazy schemas are integrated on interactive,
headless, service and child runners; actual timing and schema-overhead gates
remain unmeasured. Optional local recall prepares off the input loop and freezes
selection before provider execution; real-model uplift is not established.

AUT-231 audit found missing heartbeat ownership through provider execution,
crash result settlement and pre-dispatch budgets. Shared leases and private,
content-digested delivery checkpoints now underpin those repairs; delivery and
budget fixtures and the cockpit are being completed. The installer now resolves
its repository, candidate, installed destination, hashes, version and mode anew,
and retains the existing rollback/signing lifecycle. It has not been executed.

2026-10-05 22:30 UTC consolidation: typed mutation workers now retain their
admission through cancellation and drain before turn retirement/replacement.
Recovery reconstructs interleaved parent/child receipts in durable completion
order. Compaction cancellation is pane-owned. Shared asynchronous/synchronous
session navigation now saves the source before loading the target, and a
successful source save refreshes the cached binding even when target loading
fails. New-session transitions reuse held writer leases. Focused failure-first
regressions cover each boundary; the latest consolidated compile sampled
budget edits in progress and failed, so no new passing binary is claimed.

Measured native reads show 1.048x median improvement on the bounded warmed
four-file debug fixture, with identical output. Lazy schema fixtures measured
76.38% Main and 65.97% broad-child byte reductions before the final bounded
metadata-list schema addition; final receipts are pending. A fresh oracle
inventory/hash check at 22:21 UTC verified all 43,046 evidence files unchanged.
Actual TUI fixtures cover seven workflows and await a fresh debug build;
previous walkthrough failures are retained as failure-first evidence.

AUT-231 now has draft-only conversational proposals, one-use confirmations,
reviewed definition removal, crash settlement, meaningful-change reporting,
focus-aware best-effort metadata notifications, and process/workspace cleanup.
The audit is finishing pre-dispatch physical output and deadline admission for
all allowed execution targets, safe conversational listing and rolling-ledger
longevity. No real alerts, service installation or user configuration changes
have been performed.

Real-model hybrid quality/latency still requires the user's explicitly selected
registered embedding runtime/model IDs. Synthetic vector/HTTP fixtures do not
satisfy that gate. Full current-source checks, release, eight-hour soak and
recoverable signed installation remain open.

2026-10-05 23:02 UTC terminal consolidation: all seven actual TUI paths passed
in one 46.89-second disposable-profile run, including drag/release plus `N`,
clipboard restoration, restarted undo/redo and recovery, failed save-before-switch,
real session-list archive confirmation, local child/pane dashboard controls,
external observation, automation proposals/deletion/results, compaction repair
and cancellation, and later-request discovery. Receipt:
`/private/tmp/ca-orc258-pty-8_2kj8oz/result.json`; preserved debug artifact SHA-256
`0f0bd8191fef960beddb79f6f72530997dc081aef9c65496fe099bf2b3285305`.
Verification invalidation is covered by state fixtures; the recovery PTY checks
transcript preservation without claiming a previously passed verification receipt.

The first broad workspace run was not green: stale registry/Plan catalog
expectations, a workflow fixture missing the frozen-request event, command-count
snapshots, an oversized terminal snapshot fixture, relocated package artifacts,
and disposable Git commit fixtures without local identity were reproduced.
Fixtures now use current artifact paths, local test repository identities,
runtime Plan resolution, and boxed terminal states. Production permission and
Git behavior were not weakened. Fresh package rebuilds and the default/all-feature
workspace matrices are running sequentially. A test-only assertion introduced
during the workflow fixture repair initially failed compilation and was corrected;
it is not counted as a passing test.

Final schema fixtures measured Main 30,938 to 7,150 bytes (76.89%) and broad child
21,013 to 7,150 (65.97%); explicitly pinned variants also exceed 30%. The learning
receipt records 92 focused passes across its documented artifacts, separating
the latest two physical-output cases from earlier runs. Real registered-model
quality remains unmeasured because no registered runtimes/models exist and no
selection has been supplied. Final release, render/PTY/soak, snapshots and
installation remain pending.

2026-10-05 23:33 UTC build gates: applicable default workspace matrix passed
3,533 tests (42 ignored, nine explicitly filtered baseline exceptions); the
all-feature matrix passed 3,682 (43 ignored, the same nine exceptions).
The complete ca target is green, including the corrected workflow, Plan,
terminal and usage fixtures. The unfiltered workspace remains failing the nine
existing codex-team contracts documented individually in
`ORC-258-baseline-test-exceptions.md`; this is not an unconditional green
workspace claim. Formatting and strict workspace/all-target/all-feature
Clippy passed on the frozen source. The all-feature workspace release is
building.

Frozen implementation/test/tool fingerprint: 1,011 files,
`987511d3fd52e98212b5a046c2deabfe4eb3caec67663218d0c9271015371f9f`.
Focused/debug evidence is collected with individual artifact hashes in
`scratch/oracle-improvements/final-validation/focused-evidence-index.json`.
The extended actual branch path also passed preview, cancellation, invalid
summary, explicit acceptance, unchanged source/target transcript, provenance
and separate usage. Signed-release walkthroughs, render/PTY checks and the
real eight-hour duration remain pending; real-model hybrid validation still
requires an explicitly selected runtime/model.
