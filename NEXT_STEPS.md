# Next Steps

## LIC-2026-10-17 dual licensing — implementation complete

Owner-selected MIT OR Apache-2.0 terms, workspace/fixture metadata, contribution
policy and dependency notices are implemented and package/notice checks pass.
See [the change record](docs/exec-plans/completed/LIC-2026-10-17-dual-licensing.md).
Before distributing a binary, review the recorded original-notice/provenance
gaps and verify that target's native linkage and bundled assets. This source
change does not certify release-specific third-party rights or binary contents.

## PUB-2026-10-06 public GitHub publication — COMPLETE

User-requested build, portable documentation, public source import and five TUI
captures delivered to `ajzrva-sys/ca` on `main`. Fresh gates: 3,958 all-feature
tests passed, 44 ignored; format, strict Clippy and release pass. See
`docs/PUBLICATION_VALIDATION.md` for evidence and separate historical gates.


## BUG-262/BUG-263 — Compaction and unavailable-tool repairs installed

Compaction reserves reasoning/output headroom and permits one explicit
output-limit retry shared with its single quality repair, at most three physical
requests. Unavailable-tool correction lists the exact advertised catalog and
shows visible Ask/Plan recovery guidance while preserving frozen retry and tool
authority. Normal Agent requests still advertise and execute run_command.

Full default/all-feature workspace tests pass: 3,809/3,958 cases, zero failures,
43/44 existing ignored, zero filtered, across 26/27 targets. Format, strict
all-target/all-feature workspace Clippy, release builds and all six signed
terminal cases pass. The first all-feature migration fixture failure is retained;
the unchanged fixture and full single-thread rerun pass. Its 25-ms identity-scan
deadline remains an inferred cause, not a confirmed diagnosis.

The signed release is installed at `/Users/aji/.local/bin/ca`; background session
1017 completed exit 0. Installed artifact and rollback copy are independently
verified in `scratch/compaction/installed-readback.json`. Restart ca to load both
fixes; running sessions and settings were left unchanged.

Completed plans: `docs/exec-plans/completed/BUG-262-compaction-output-budget.md`
and `docs/exec-plans/completed/BUG-263-unavailable-tool-correction.md`.
Evidence: `scratch/compaction/` and `scratch/unavailable-tool/`, including the
explicit six-input inventory completion and retained initial failure logs.

## BUG-261 — Recovery save repair installed

The source commits through the active writer before a new recovery branch is
saved; one identical automatic replacement succeeds, and genuine save failures
show the local cause in `/errors`. The final original-process owner explicitly
releases inherited-descriptor locks without weakening writer exclusion.
All six new recovery regressions, frozen HTTP request test, full default workspace
suite, format/strict lint/release checks and eight signed terminal scenarios pass.
Full all-feature workspace tests also pass: 3,945, zero failures, 44 existing
ignored, zero filtered (session 90557 exit 0).
Installed artifact and rollback copy are independently verified. Use a new ca
process to load both this repair and BUG-260's Enter fix.
Plan: `docs/exec-plans/completed/BUG-261-recovery-branch-save-ownership.md`.
Evidence: `scratch/recovery-branch/`.

## BUG-260 — Enter repair complete and installed

Leading-path drafts and empty path menus now interrupt and send on Enter.
127 input regressions, render/format/strict lint/release checks and five actual
signed terminal scenarios pass. Installed artifact and recoverable backup are
verified. New ca processes use the fix; Ctrl+S works in the current old process.
Plan: `docs/exec-plans/completed/BUG-260-leading-path-enter-interruption.md`.
Evidence: `scratch/enter-interrupt/`.

The previously open retention writer-lease follow-up is resolved by BUG-261.
Current default/all-feature main runs pass retention/archive; the final default
workspace is green. Original failure logs remain historical evidence in
`scratch/enter-interrupt/`.

## AUT-259 — Complete and installed

Full unfiltered default/all-feature workspace suites, strict lint, formatting,
real CLI/MCP, actual debug/signed-release terminal checks, release build and
recoverable paired installation pass. The two continuation screenshot failures
are repaired. See `docs/exec-plans/completed/AUT-259-ca-automation-absorption.md`
and `docs/CA-automation-runtime.md` for commands and evidence.

Runtime migration remains an explicit preview/copy/validate management operation.
No paused definition was enabled. Repository tasks require a reviewed real Git
binding; doctor reports this checkout's missing metadata and stale delegate path.
Use a new ca session for the installed changes; running sessions were left alone.

Older sections below retain their original task boundaries. AUT-259 supersedes
their dated workspace-failure and installation statements; separate unmeasured
real-model and eight-hour gates remain open.

## ORC-258 — Remaining release gates

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

1. Inspect the candidate's actual eight-hour receipt after its earliest finish;
   require actual elapsed duration and all post-idle checks, not only liveness.
2. Run explicitly selected registered-runtime/model recall and warm latency.
   Check JSON acceptance and fallback cases; exit zero is not acceptance.
3. Review final oracle/source/milestone receipts, then perform the already-authorized
   recoverable signed installation only after both remaining gates pass.

## BUG-257 — Long-session responsiveness and bounded failure recovery

Status: IMPLEMENTED; validation/install pending (2026-10-04)
Plan: `docs/exec-plans/active/BUG-257-long-session-responsiveness.md`

1. Inspect the eight-hour PTY result after 2026-10-05 01:23 America/New_York:
   `/private/tmp/ca-bug257-pty-uds9g6kg/result.json` (background session 89271).
   Require status passed, actual idle >= 28,800 seconds, p95 <100 ms and ordered
   input/theme/resize/terminal restoration; do not infer a soak pass from simulation.
2. Run the loopback replacement test and workspace gates in an environment that
   allows their required TCP/process/sandbox/fixture operations. Preserve/report
   unrelated failures; the current workspace suite is not green.
3. Install the verified signed release from a normal terminal using
   `bash tools/bug257_install.sh` after reviewing the remaining validation gaps.
   This turn's environment denied installation writes; current `ca` is unchanged.
4. Record the actual soak/network/install outcomes and only then close the plan.

## TUI-256 — Grok-style usage-limit graph

Status: COMPLETE (2026-09-30; bin suite + fmt/clippy)
Plan: `docs/exec-plans/completed/TUI-256-usage-limit-graph.md`

Usage limit tab shows a Grok-style percentage bar from CA provider/ledger
data. `/usage` still opens the three-tab modal.

## TUI-255 — Grok-style agent dashboard

Status: COMPLETE (2026-09-30; bin suite + fmt/clippy)
Plan: `docs/exec-plans/completed/TUI-255-grok-style-agent-dashboard.md`

Full-screen agent roster with Working/Idle/Inactive groups, dispatch prompt,
worktree toggle, and a tabbed `/usage` modal. Open with `/dashboard`,
`Ctrl+\`, or `ca dashboard`. `/sessions` remains the picker.

## BUG-254 — Stale retry boundary, tool status, bounded await_job

Status: COMPLETE (2026-09-25; two installs; full bin suite + fmt/clippy)
Plan: `docs/exec-plans/completed/BUG-254-stale-retry-boundary-and-tool-status.md`

`/retry` falls back to the latest user request when the stored boundary is
stale; history prepend shifts retry indices; session replace and new user turns
clear them. `await_job` waits are bounded (default 30s, cap 120s) and return a
running snapshot so the turn cannot freeze on a multi-hour job. Failure footers
fit the status bar and always show the next action.
Installed at `/Users/aji/.local/bin/ca`; see CONTINUITY.md for hashes.

## HRS-253 — Evidence-driven harness improvement

Status: COMPLETE — original six-stage implementation and authorized VM setup/install.
Plan: `docs/exec-plans/completed/HRS-253-evidence-driven-harness-improvement.md`

Evidence, isolated guidance/source candidates, matched evaluations, bounded idle
execution, dedicated VM verification, activation and rollback are connected.
The signed release is installed and VM-backed experiments are enabled with the
$20/two-active-hour limit. Current application tests, lint and formatting pass;
the complete 54-trial VM guidance lifecycle passes.

Nine existing workspace integration failures still block engineering admission.
The dirty original checkout is ineligible for source-changing experiments.
Existing edits and failed receipts are preserved. Controlled lifecycle tests
demonstrate the mechanism; real-provider improvement is not claimed. See the
completed plan for exact validation totals and the recovery/install receipt.

## HRS-252 — Apple Container execution backend

Status: DRAFT (2026-09-19; design note only — not activated; no `src/` changes)
Plan: `docs/HRS-252-apple-container-execution-backend.md`
Type: Optional isolation capability (CA extension; no Cursor parity claim)
PARITY: `HRS-252` section — `Unverified` until CA-owned tests exist

Contract-only note: keep `SandboxMode` as policy strength; add a separate
`ExecutionBackend::{HostSeatbelt,AppleContainer}` for jobs/`run_command` only.
Maps `SandboxPolicy` roots/RO roots/tmp/network onto container mounts and
`--network none`; fail-closed when CLI/host unsupported; owner-bound VM
lifetime. Evidence is Hermes PR #78035 behavior notes plus read-only
`oracles/hermes-agent-2026.9.14` patterns — no oracle source copied.
Non-goals: multi-backend registry, Desktop bridges, default-sandbox change.
Activate only on an explicit user turn; then open a full ExecPlan.

## HRS-251 — Hermes context/cache/session resilience

Status: COMPLETE (2026-09-17; M1–M2+M5 + FTS create-fallback; M4 ESTOP deferred)
Plan: `docs/exec-plans/completed/HRS-251-hermes-context-compaction-cache-resilience.md`
Related: `docs/exec-plans/completed/HRS-250-hermes-context-cache-compaction.md`

Checkpoint anchors + verbatim user quotes + recall pointer; never-summarize-user
prompt; usage-anchor cooldown 60/300/900s; cache-aware skill status; FTS5
create degrades to LIKE-style fallback. ESTOP sentinel deferred.

## IDX-250 — Index Cursor-informed draft plans

Status: COMPLETE (docs-only)
Plan: `docs/exec-plans/completed/IDX-250-cursor-informed-draft-plan-index.md`
Canonical index: [`docs/CURSOR_INFORMED_PLAN_INDEX.md`](docs/CURSOR_INFORMED_PLAN_INDEX.md)

Indexes DRAFT plans CUR-245, SEC-246, TUI-247, CMD-248, SES-249 (study bench:
Cursor `agent-cli-local` on MiMo/DeepSeek, no Cursor account). Child plans
stay DRAFT until explicitly activated; suggested order is in the canonical
index. Execution of IDX-250 was index/docs only — no `src/` changes.

## SES-249 — Headless session identity and resume fixtures

Status: COMPLETE (2026-09-17; store resume + CLI exclusivity; result-envelope echo deferred)
Plan: `docs/exec-plans/completed/SES-249-headless-session-identity-and-resume.md`

SessionStore resume by public id retains conversational tokens; unsaved
identities do not appear in the store. CLI rejects `--new-session-id` combined
with continue/resume/`--no-session` and rejects `--no-session` with resume
paths. No Cursor SQLite/persist import.

## CMD-248 — Slash/picker/keymap hygiene

Status: COMPLETE (2026-09-17; description inventory, model-as-slash alias, /tasks usage)
Plan: `docs/exec-plans/completed/CMD-248-slash-picker-keymap-hygiene.md`

Bare configured model ids resolve to `/model <id>`; explicit commands win;
106 visible commands have descriptions; /tasks usage documented.

## TUI-247 — Tool lifecycle events and tool-card clarity

Status: COMPLETE (2026-09-17; inventory + lifecycle event characterization; card timing deferred)
Plan: `docs/exec-plans/completed/TUI-247-tool-lifecycle-events-and-card-clarity.md`

Headless already emits ToolStart/ToolFinish; no duplicate schema. ToolCall has
no duration field yet so card timing was not invented.

## SEC-246 — Trust/approval/sandbox prompt clarity

Status: COMPLETE (2026-09-17; focused tests + fmt + clippy; not installed this turn)
Plan: `docs/exec-plans/completed/SEC-246-trust-approval-sandbox-prompt-clarity.md`

Locked `--yolo`/`--auto-review` exclusivity with tests; clarified workspace
trust prompt (path, blocked tools, y/n, headless `--trust` process-only note);
labeled auto-review as CA-local safe-tool evaluation in `/auto-review`, help,
and `ca doctor`; doctor sandbox copy states fail-closed when Required backend
is missing. PARITY Observed notes record Cursor CLI help only.

## CUR-245 — Cursor-informed plan/ask/composer follow-ups

Status: COMPLETE (2026-09-17; Ask/Plan characterization + launch status; M4 artifact skipped)
Plan: `docs/exec-plans/completed/CUR-245-cursor-informed-plan-ask-composer.md`

Ask resolves zero tools (empty allowed_names); Plan profile is proposal-only;
`--mode plan|ask` launch sets tool-policy status strings.

## PLN-244 — Limit the plan card to review

Status: COMPLETE (installed). Plan: `docs/exec-plans/completed/PLN-244-plan-card-review-lifetime.md`.

## PLN-243 — Show all plan tasks by default

Status: COMPLETE (installed). Plan: `docs/exec-plans/completed/PLN-243-expanded-plan-default.md`.

## BUG-242 — Astra reasoning

Status: COMPLETE (installed). Plan: `docs/exec-plans/completed/BUG-242-astra-reasoning.md`.

# PLN-241 — Plan Review and Execution

Status: COMPLETE (2026-09-04; installed)

Plan: `docs/exec-plans/completed/PLN-241-plan-review-execution.md`

Readable plan card/full reader, revision-bound actions, and a fresh current-agent
execution handoff. Failure-first regressions, focused/render tests, both terminal
sizes, strict lint/format, and release verification are complete. The broad
suite's single unrelated timeout passed unchanged in isolation; the known oracle
baseline discrepancy remains recorded separately. Background job 42112 installed
the verified release and preserved a recoverable prior executable.

---

# TUI-240 — Stop and Interrupt Controls

Status: COMPLETE (2026-09-03; built, not installed)

Plan: `docs/exec-plans/completed/TUI-240-stop-and-interrupt.md`

Enter interrupts/sends, Alt+Enter queues, Esc stops, and Ctrl+C works through
popups. Foreground commands stop with their waiter; drafts/images are retained
and old queues stay on hold. Focused/full tests, paired snapshots, debug/release
terminal walkthroughs, formatting, strict Clippy and release checks pass.
No installation ran. The separate oracle-baseline discrepancy remains.

---

# BUG-239 — Native Codex Model Catalog

Status: COMPLETE (2026-09-03; not installed at user's request)

Plan: `docs/exec-plans/completed/BUG-239-native-codex-catalog.md`

Native authenticated discovery now returns seven visible Codex models and
matches the picker. Both refresh paths retain last-success catalogs and label
configured-only entries. Focused tests, paired snapshots, full all-feature
workspace tests, formatting, strict Clippy and release checks pass. The user
explicitly canceled installation for this task; the release is built and the
current installed executable is unchanged. The existing oracle-baseline
discrepancy remains separately recorded.

---

# REL-238 — Post-Task Background Release Installation

Status: COMPLETE (2026-09-03)

Type: User-requested repository workflow and local installation

Plan: `docs/exec-plans/completed/REL-238-post-task-background-install.md`

The standing post-task background release/install rule is in `AGENTS.md`.
Managed background job 57224 built and installed the validated BUG-237 release
(`0336bd20…0192`) at `/Users/aji/.local/bin/ca`; the prior `1e91f114…8f72`
executable is recoverable. Hashes, signature, mode/owner, PATH, version/help,
and diff hygiene pass. The pre-existing oracle baseline discrepancy is unchanged.

---

# BUG-237 — OpenAI Codex Model Picker Fallback

Status: COMPLETE (2026-09-03; protected-tree gate separately blocked)

Priority: P1

Type: Provider model-catalog usability

The exact ChatGPT-backed `openai-codex` route now retains the supported GPT-5.6
Sol, Terra, and Luna fallback models when catalog discovery fails, instead of
only its configured starter model. Custom endpoints, live discovery, and
runtime configuration are unchanged. Failure-first catalog coverage, refresh
regression, paired render snapshot, full all-feature workspace tests, formatting,
strict Clippy, release build, and CLI smokes pass. The pre-existing protected-tree
baseline discrepancy remains; no installation ran. See `TESTING.md` and
`CONTINUITY.md` for validation and account-access limits.

Follow-up: REL-238 installed this exact validated candidate at the user's
request and established standing post-task installation authorization.

---

# SES-236 — Fresh Session Command

Status: COMPLETE (2026-09-03; protected-tree gate separately blocked)

Type: Session lifecycle and command discovery

Plan: `docs/exec-plans/completed/SES-236-new-session-command.md`

`/new` saves the outgoing session and starts an empty independent session,
retaining the model/workspace and leaving `/clear` unchanged. Ten focused tests,
all 218 render tests, the full workspace suite, formatting, strict Clippy, the
release build, CLI smokes, and a disposable-home terminal walkthrough pass.
The pre-existing protected-tree discrepancy remains. No installation ran.

---

# BUG-235 — Bounded Pre-Output Recovery and Clean Failed-Turn Follow-Ups

Status: COMPLETE (2026-09-03; protected-tree gate separately blocked)

Priority: P0

Type: Provider stream and failed-turn session reliability

Plan: `docs/exec-plans/completed/BUG-235-bounded-pre-output-recovery.md`

Share the two-request route cap across headers and pre-output body failures,
discard replayed metadata, and branch a validated replacement prompt before a
dangling failed user message. Focused and full tests, formatting, strict Clippy,
release, snapshots, and the disposable-home walkthrough pass. The unrelated
ignored `oracles/` digest mismatch is recorded in the completed plan; no
protected receipt/tree repair or installation was authorized.

---

# Reasoning-Effort Picker (TUI)

Status: COMPLETE (2026-09-03)

Type: Model reasoning-effort selection UX

`/reasoning` with no argument now opens a centered picker over the selected
model profile's configured `reasoning_efforts`; `/reasoning <effort>` still sets
directly, `Ctrl+T` cycles, and model switch applies the profile default. Enter
selects, Esc cancels without change. The chosen effort flows through the
existing provider wire translation unchanged. Focused picker tests, the full
render (216) and input (195) suites, the complete `ca` target (2,400/4 ignored),
formatting, strict Clippy, release, and snapshot hygiene all pass; the candidate
was recoverably installed.

---

# BUG-234 — DeepSeek DSML and Dream Queue Recovery

Status: COMPLETE (2026-09-03)

Priority: P0

Type: Provider protocol and durable learning-queue correctness

Plan: `docs/exec-plans/completed/BUG-234-deepseek-dsml-dream-queue-recovery.md`

Normalize exact DeepSeek V4 DSML wrappers into CA's structured tool-event path,
and narrowly recover authentic legacy multibyte supervised-learning intents so
one stale record cannot block later Dream runs. Both serial workspace matrices,
formatting, strict Clippy, release, smokes, signature, and hygiene checks pass,
and the candidate `60727a1c…ad566` was recoverably installed. The user-authorized
AUT-231 formatter/lint cleanup is complete.

---

# TUI-233 — Distinct Sub-Agent Worker Icons

Status: COMPLETE (2026-09-02)

Priority: P2

Type: Inline conversation-agent presentation polish

Replace the generic inline agent bullet with one of six compact colored worker
glyphs selected deterministically from the stable agent ID. Preserve status,
task, action, focus, approval, cancellation, and restored-summary behavior; the
same worker keeps the same icon across live updates and session restoration.

---

# BUG-232 — Bounded Unavailable-Tool Recovery

Status: COMPLETE (2026-09-03; AUT-231 blockers cleared under BUG-234)

Priority: P0

Type: Interactive provider/tool-loop reliability repair

Plan: `docs/exec-plans/completed/BUG-232-unavailable-tool-recovery.md`

Reject one provider-emitted tool call outside the exact advertised set without
executing it, return a bounded safe tool error to the same logical turn, and
allow one provider continuation before repetition fails closed. Preserve every
existing tool-selection, mode, workflow, approval, sandbox, and network bound.
The formatter and strict-Clippy blockers are cleared; the repair passes the
integrated matrices recorded under BUG-234.

---

# AUT-231 — Codex-Style Local Automations

Status: ACTIVE (resumed 2026-10-05 under ORC-258; the older partial-source
resume bundle remains historical evidence at `docs/exec-plans/active/AUT-231-resume/`)

Priority: P0

Type: Local scheduler product experience, session delivery, and bounded unattended execution

Plan: `docs/exec-plans/active/AUT-231-codex-style-local-automations.md`

Build conversational exact-preview automation creation, heartbeat continuations,
standalone per-run tasks, exact reasoning and rolling budgets, safe local
notifications, and the `/automation` cockpit over the existing frozen durable
scheduler. Preserve all active-plan and dirty-tree ownership; no real provider
or LaunchAgent operation runs during validation. Its former formatting and
dead-code gate blockers were cleared under BUG-234, but M1 remains incomplete
and M2–M5 remain paused.

---

# PRT-230 — Exclude Finder Metadata from Protected Trees

Status: COMPLETE (2026-08-29)

Priority: P0

Type: Protected-tree verification compatibility repair

Plan: `docs/exec-plans/completed/PRT-230-exclude-finder-metadata.md`

Exact `.DS_Store` components are excluded before protected-tree counts and
hashes under algorithm v2; similarly named files remain protected. Focused and
both integrated matrices pass. The separately authorized manifest-only commit
establishes HEAD trust, and protected verification passes.

---

# REL-229 — ORC-218 Integrated Audit and Installation

Status: COMPLETE (2026-08-29)

Priority: P0

Type: Integrated validation, reconciliation, and recoverable installation

Plan: `docs/exec-plans/completed/REL-229-orc218-audit-installation.md`

Every stable child and PRT-230's exact Finder-metadata exclusion are implemented.
Both replacement serial matrices, formatting, strict all-feature Clippy,
release, command/schema smokes, signature, diff/snapshot hygiene, and focused
regressions pass. The v2 manifest is committed and verified from `HEAD`; the
release is recoverably installed with matching candidate/installed SHA-256,
strict signature, ownership/mode, path, and command smokes.

---

# PRV-228 — Responses WebSocket Readiness Record

Status: COMPLETE — DEFERRED (2026-08-29)

Priority: P0

Type: Documentation-only stable-provider readiness deferral

Plan: `docs/exec-plans/completed/PRV-228-responses-websocket-readiness.md`

Official OpenAI documentation still places Responses WebSocket client/server
events under the Beta Responses resource while stable Responses documents
`POST /responses`. CA ships no experimental dialect or public surface; the
readiness record names the stable endpoint, authentication, ordering, tool,
cancellation, reconnect, usage, error, and no-replay evidence required to
reopen this work.

---

# IMP-227 — Safe Claude Setup Import

Status: COMPLETE (2026-08-29)

Priority: P0

Type: Digest-bound transactional local setup migration

Plan: `docs/exec-plans/completed/IMP-227-safe-claude-setup-import.md`

CA now previews and transactionally imports only Claude instructions, rules,
commands, and valid skills into native roots. Exact source/target digests bind
apply; bounds, sensitive exclusions, native validation, collision/drift,
symlink/hardlink, rollback, bare refusal, and Cancel-first TUI behavior are
covered. The complete CA target and child completion gates pass without a real
import, provider request, or installation.

---

# EDT-226 — ACP IDE Context and Hash-Anchored Editing

Status: COMPLETE (2026-08-29)

Priority: P0

Type: Stable editor interoperability and drift-safe file mutation

Plan: `docs/exec-plans/completed/EDT-226-acp-context-hash-anchored-editing.md`

Stable ACP v1 now accepts bounded explicit text resources and contained file
links with visible provenance. Optional eight-hex `read_file` anchors and the
separate `hashline_edit` tool reject stale/ambiguous ranges before using CA's
existing approval, sandbox, undo, receipt, loop-guard, LSP, activity, and
artifact paths. Focused and complete CA tests, formatting, strict Clippy, and
release pass without provider traffic or installation.

---

# PAN-225 — Bounded Split-Pane Sessions

Status: COMPLETE (2026-08-29)

Priority: P0

Type: Pane-local interactive sessions and bounded terminal layout

Plan: `docs/exec-plans/completed/PAN-225-bounded-split-pane-sessions.md`

The interactive TUI now supports at most four bounded pane-local sessions with
pane/generation-safe routing, configurable keyboard and mouse focus, explicit
background approvals, per-pane agents/jobs/goals/workflows, and Cancel-first
closure. Focused and complete CA tests, reviewed snapshots, formatting, strict
Clippy, and release pass; headless, service, ACP, and session formats are
unchanged.

---

# ART-224 — Session Artifact Ledger and Browser

Status: COMPLETE (2026-08-29)

Priority: P0

Type: Bounded session-local file provenance and safe artifact inspection

Plan: `docs/exec-plans/completed/ART-224-session-artifact-ledger.md`

Saved sessions now retain a 512-entry metadata-only artifact ledger. Mutations
are tracked by default, reads are opt-in, branches trim by message boundary,
portable exports omit the ledger, and `/artifacts` revalidates containment,
symlinks, size, and digest before viewing. Focused tests, reviewed snapshots,
the clean full CA target, formatting, strict Clippy, and release pass.

---

# SYS-223 — Active-Work Lifecycle

Status: COMPLETE (2026-08-29)

Priority: P0

Type: Bounded keep-awake lifecycle and graceful combined job stop/wait

Plan: `docs/exec-plans/completed/SYS-223-active-work-lifecycle.md`

Idle-only keep-awake now follows reference-counted interactive turns and
model-originated jobs, with fixed no-shell helpers, debounced release, and
shutdown/parent-death cleanup. `await_job` supports ownership-preserving
graceful stop, a 0–300 second grace, process-group escalation, final bounded
output, and exactly-once completion. Focused tests, formatting, strict Clippy,
release, and hygiene gates pass without provider traffic or installation.

---

# WFL-222 — Audited Built-in Workflows and Structure Inspection

Status: COMPLETE (2026-08-29)

Priority: P0

Type: Immutable local research workflows and safe DAG inspection

Plan: `docs/exec-plans/completed/WFL-222-audited-built-in-workflows.md`

Immutable `deep-research`, `fact-check`, and `research-experiment` definitions
now use the existing declarative engine. Network access remains subject to
ordinary approval; fact-check requires three distinct configured routes with
request-level juror isolation; experiments require a clean owned worktree,
bounded editable globs, and an exact digest-bound evaluation command. Safe
template/inspect/tree views expose structure and state without prompts or
outputs. Focused tests, reviewed snapshots, formatting, strict Clippy, release,
and hygiene gates pass without hosted traffic or installation.

---

# REV-221 — Convergent Review and Acceptance Packets

Status: COMPLETE (2026-08-29)

Priority: P0

Type: Dual-route isolated review, bounded repair, and host-evidence acceptance

Plan: `docs/exec-plans/completed/REV-221-convergent-review-acceptance.md`

Require two explicitly configured distinct provider/model reviewers to inspect
one identical objective in concurrent isolated read-only contexts. Both must
pass within strict request/output/cost and three-round limits. Add immutable,
private, revision-bound `ca.acceptance.v1` packets whose verified claims derive
only from host evidence. Headless repair uses fresh pairs for at most three
rounds; TUI repair remains an explicit ordinary agent turn followed by a fresh
one-pair invocation. Focused suites, formatting, strict all-feature Clippy,
release, and hygiene gates pass without provider traffic or installation.

---

# RES-220 — Streaming Anomaly Protection

Status: COMPLETE (2026-08-29)

Priority: P0

Type: Provider-neutral stream safety and protocol containment

Plan: `docs/exec-plans/completed/RES-220-streaming-anomaly-protection.md`

The bounded decoded-event guard now stops clear repetition loops and unfenced
pseudo-tool envelopes, preserves prior output, cancels the live response, and
returns typed secret-safe provider-protocol failures without fallback or
replay. Fences, ordinary code, valid tool events, malformed/non-advertised
envelopes, and short natural repetition remain unchanged. Focused suites,
formatting, strict all-feature Clippy, release, and hygiene checks pass.

---

# DIA-219 — Support Bundles and Configuration Provenance

Status: COMPLETE (2026-08-29)

Priority: P0

Type: Local diagnostics, provenance, privacy, and support export

Plan: `docs/exec-plans/completed/DIA-219-support-bundle-config-provenance.md`

Add read-only `ca config explain` and `/config explain` provenance plus private,
atomic, bounded `ca support-bundle` and Cancel-first `/support-bundle`. Records
remain typed, hash-bound, path-free, secret-free, and provider-free. Private
atomic publication, collision/drift and link/mode defenses, a Cancel-first TUI
preview, focused tests, the default workspace matrix, formatting, strict
all-feature Clippy, and the all-feature release build pass. Installation remains
owned by REL-229.

---

# ORC-218 — Complete Remaining Local Oracle Guidance

Status: COMPLETE (2026-08-29)

Priority: P0

Type: Local capability completion, verification, interoperability, and release

Plan: `docs/exec-plans/completed/ORC-218-complete-remaining-local-oracle-guidance.md`

Implement and validate every approved stable non-hosted capability from the
remaining oracle review while preserving CA's stricter privacy, permission,
provider, plugin, and session boundaries. Finish the existing AGT-157
prerequisite first; then execute DIA-219, RES-220, REV-221, WFL-222, SYS-223,
ART-224, PAN-225, EDT-226, IMP-227, the deferred PRV-228 readiness record, and
REL-229 sequentially. One recoverable installation is authorized only after all
stable work and completion gates pass. No paid/hosted request, oracle/reference
edit, beta Responses WebSocket surface, or unrelated-tree reset is authorized.

---

# TST-217 — Eliminate Broker Cache-Publication Collisions

Status: COMPLETE (2026-08-29)

Concurrent shared-query cache publication now uses private atomic temporary
files named with process ID, captured timestamp, and a monotonic sequence, with
16 bounded `AlreadyExists` retries and cleanup limited to writer-owned files.
Deterministic collision/exhaustion tests, all broker and MCP suites, three
repeated shared-query runs, both workspace matrices, formatting, strict Clippy,
release build, and protected-tree hygiene pass. Public schemas, cache semantics,
runtime state, provider traffic, and the installed binary are unchanged.

---

# TST-216 — Deterministic Scheduler Test Repair

Status: COMPLETE (2026-08-29)

The protected codex-team five-client evidence fixture now uses an ignored
marker barrier, verifies five broker consumers before release, and holds the
child deterministically through last-consumer cancellation. Production broker
behavior is unchanged. Focused repetitions, MCP unit tests, one clean 8/8 SDK
run, formatting, and strict package Clippy pass. The package matrix's separate
pre-existing shared-query cache-publication collision was resolved by TST-217.

---

# CUR-204 — Complete Missing Local Cursor Features

Status: COMPLETE (2026-08-29)

Priority: P0

Type: Local feature completion, interoperability, TUI, and release program

Plan: `docs/exec-plans/completed/CUR-204-complete-missing-local-cursor-features.md`

Implement and validate every identified non-subscription local Cursor feature
gap while preserving CA's stricter trust, privacy, permission, plugin, session,
and provider boundaries. Cursor `2026.07.09-a3815c0` remains source evidence,
not a runtime parity oracle. The exact 810-path pre-program dirty baseline and
all seven active-plan overlaps are recorded by the umbrella plan.

Execute sequentially, with one final installation only after all gates pass:

1. [WSP-205](docs/exec-plans/completed/WSP-205-named-multi-root-workspaces.md) —
   named multi-root workspaces (complete 2026-08-28).
2. [KNL-206](docs/exec-plans/completed/KNL-206-conditional-rules-command-management.md)
   — conditional rules and command management (complete 2026-08-28).
3. [JOB-207](docs/exec-plans/completed/JOB-207-background-job-lifecycle.md) —
   background-job lifecycle (complete 2026-08-28).
4. [PLG-208](docs/exec-plans/completed/PLG-208-plugin-ux-ephemeral-loading.md) —
   plugin discovery UX and ephemeral loading (complete 2026-08-28).
5. [ENV-209](docs/exec-plans/completed/ENV-209-direnv-shell-integration.md) —
   direnv and shell integration (complete 2026-08-28).
6. [HED-210](docs/exec-plans/completed/HED-210-rich-launch-worktree-controls.md) —
   rich launch and worktree controls (complete 2026-08-28).
7. [OBS-211](docs/exec-plans/completed/OBS-211-runtime-logs-watchdogs.md) — logs
   and runtime watchdogs (complete 2026-08-28).
8. [TUI-212](docs/exec-plans/completed/TUI-212-copy-mermaid-terminal-polish.md)
   — copy, Mermaid, and terminal polish (complete 2026-08-28).
9. [ACP-213](docs/exec-plans/completed/ACP-213-stable-acp-interoperability.md) —
   stable ACP interoperability (complete 2026-08-28).
10. [CLI-214](docs/exec-plans/completed/CLI-214-local-automation-flags.md) —
    lower-level local automation flags (complete 2026-08-29).
11. [REL-215](docs/exec-plans/completed/REL-215-cur204-audit-installation.md) —
    integrated audit, documentation, completion gates, and verified install
    (complete 2026-08-29).

Current result: WSP-205 through ACP-213 are complete. CA now has private named
multi-root workspaces plus bounded user/all-root conditional rules with
legacy/always/glob/manual activation, saved manual IDs, and host-explicit path
matching. `/rule` and `/command` expose guarded native management with digest
confirmation and drift rejection while foreign/generated sources stay
read-only and built-ins retain precedence. Model, direct-shell, and internal
jobs now have distinct origins; foreground work can be promoted without
restart, and explicit await and idle host completion share exactly-once
delivery. Headless runs wait for model jobs by default, while `--single-turn`
reports bounded shutdown. Repeatable `--plugin-dir` roots compose validated
plugins for one process without registry writes, and `/plugin` manages CA's
verified local bundles and signed catalogs without adding remote marketplaces.
The new redacted `RunEnvironment` consistently feeds provider credentials and
authorized child-process surfaces. Direnv remains explicit, trusted,
digest-bound, sandboxed, network-denied, and workspace-bound; `/direnv` previews
and confirms re-evaluation. CA-native zsh integration is exact-marker,
recoverable, and atomic. Focused tests, the default workspace matrix,
formatting, strict all-feature Clippy, release build, snapshots, and protected
trees pass. Fresh launch now supports plan/ask selection, process-effective
sandboxing, headless-only process trust, registered worktree creation, typed
native/compatibility setup with exact digest authority, an asynchronous
Cancel-first TUI preview, and local repository setup inspection. HED's full
child gates pass and setup failures retain their registered worktrees. CA also
has private rotating metadata-only runtime logs, a bounded `/logs` viewer,
central measured rendering, heartbeat/render/flicker/restoration/memory
watchdogs, and hidden `/debug render` diagnostics. The OBS child gates pass;
administrative no-state behavior, provider traces, and protected trees remain
unchanged. `/copy` now provides bounded message/part selection, supported
Mermaid blocks render off-loop with exact fallback, Markdown retains safe
clickable destinations, and terminal focus/theme/title handling is bounded and
restoration-safe. TUI-212's focused coverage, reviewed snapshots, both workspace
matrices, strict lint, release build, and hygiene checks pass. `ca acp` now
exposes stable ACP v1 newline JSON-RPC stdio through the pinned official SDK
with local sessions, agent/plan/ask modes, streamed updates, fail-closed
permissions, cancellation, and disconnect cleanup. The existing CA service
protocol remains independent, and an explicit sorted JSON compatibility
boundary preserves pre-ACP deterministic output bytes despite the SDK's
transitive object-order feature. ACP focused tests, both workspace matrices,
strict lint, release build, smokes, and hygiene checks pass. Strict pre-init
version gating, bounded ephemeral history, caller-owned UUIDs,
workspace-context exclusion, bounded Git context, key-only structured
environment deltas, and local versioned repository information are now also
complete. CLI-214 focused tests, the default matrix, and the final clean
all-feature matrix pass. REL-215 reconciled all plans and harness pages,
accounted for all 810 initial records plus exactly 58 CUR-owned additions,
passed the release and protected-tree gates, and installed SHA-256
`30660c4e21598363feb3c99d11f271716f84e6f3e95907d07f633faa0af83111`
recoverably. The prior binary remains at its timestamped full-SHA recovery
path.
The initial status fingerprint remains
`02fd90e56932bc57a3de507368c86d060628a48eb8ad40a3d3a83fbb400a2d29`;
no provider, hosted service, user configuration/session, oracle, reference, or
protected-tree source was changed.

---

# HRS-203 — Hermes Delegation and Learning Completion

Status: COMPLETE (2026-08-28)

Priority: P0

Type: Delegation lifecycle, isolation, learning curation, and graph visibility

Plan: `docs/exec-plans/completed/HRS-203-hermes-delegation-learning-completion.md`

Finish the ten Hermes-informed capabilities left outside HRS-202: strict child
JSON contracts with one repair, one-call parallel batches, origin-bound detached
completion, optional child worktrees, a capability-protected plugin lifecycle
facade, recoverable automatic skill curation, memory-aware visual learning
graphs, full-output spill and timeout diagnostics, surviving background-job
attribution, and a global future-spawn pause. Preserve existing grants,
approvals, session compatibility, default TUI behavior, and all unrelated dirty
tree changes.

Current result: all ten capabilities are implemented. Child agents support
strict one-repair JSON contracts, atomic ordered batches, origin-safe detached
completion, optional owned worktrees, capability-protected lifecycle handles,
private spills and detailed timeout receipts, durable background-job
attribution, and a global future-spawn pause. The opt-in curator has recoverable
archive/restore/consolidate/pin/sync/adopt transactions, and the learning graph
joins scoped memory metadata without exposing memory bodies. Focused suites,
both workspace matrices, strict lint, release build/smokes, snapshot hygiene,
and protected-tree verification were completed; two unchanged process/cache
stress cases each passed on their immediate isolated rerun. No user runtime
state, provider configuration, installed binary, or oracle was changed.

---

# HRS-202 — Hermes-Informed Learning and Agent Lifecycle

Status: COMPLETE (2026-08-28)

Priority: P1

Type: Skill relationships, learning visibility, and child lifecycle integrity

Plan: `docs/exec-plans/completed/HRS-202-hermes-learning-agent-lifecycle.md`

Add the pertinent read-only Hermes behaviors CA does not already provide:
declared skill relationships and a bounded metadata-only learning graph,
lossless child steering across approval/finalization boundaries, bounded waits,
and digest-verifiable terminal handoffs. Preserve CA's existing exact grants,
workspace snapshots, approval/cancellation ownership, learning ledger, session
schema, and suggestions-only asset maintenance.

Current result: portable skills now expose bounded descriptive categories and
relationships, `/learning graph` joins current-digest usage with declared and
successful co-use edges, and child steering is acceptance-tracked through
delivery or a terminal missed-steer receipt. Optional bounded waits return live
snapshots and terminal summaries carry stable SHA-256 identities. Focused
suites, both workspace matrices, strict lint, optimized release, executable
smokes, snapshot hygiene, and protected-tree verification pass without runtime
state, provider, installation, or oracle changes.

---

# PRV-201 — DeepSeek Cache-Usage Compatibility

Status: COMPLETE (2026-08-28)

Priority: P0

Type: Provider protocol reliability

Plan: `docs/exec-plans/completed/PRV-201-deepseek-cache-usage-compatibility.md`

Repair the live DeepSeek stream failure caused by supplemental OpenAI-shaped
cache details on a Chat hit/miss usage record. Keep DeepSeek Chat hit/miss
counters authoritative, select Responses cache usage by its documented shape,
preserve exact billing and the `deepseek_v1` identity, and leave custom
providers and public interfaces unchanged.

Acceptance:

- Supplemental Chat cache details cannot abort or alter a complete DeepSeek
  hit/miss record; incomplete or inconsistent hit/miss data still fails.
- Exact DeepSeek Responses routes decode cached-input details without sending
  explicit cache controls.
- Focused and repository release gates pass before the authorized backed-up
  installation and single capped default-profile live verifier.

Current result: DeepSeek Chat now treats its complete hit/miss pair as the sole
cache-usage authority even when supplemental OpenAI-shaped details are present,
while DeepSeek Responses resolves its documented cached-input details. Both
workspace matrices and every release gate pass. SHA-256
`82c72a53a0092c1a274a92b550d5790d530fef39515247bd525144ec1fd603ea`
was installed recoverably, and the single default-profile verifier passed
`deepseek/deepseek-v4-pro` with exactly two requests.

---

# INT-198 — Guided Typed Automation Setup

Status: COMPLETE (2026-08-27)

Plan: `docs/exec-plans/completed/INT-198-guided-automation-setup.md`

Let an ordinary CA model gather missing non-secret automation details, obtain a
typed codex-team preview, and create the private manifest plus active-profile
wiring after one exact approval. Keep the setup contract template-driven so
future typed automations reuse the same three tools.

Current result: normal Main/headless models can discover the generic setup
template, use typed user questions for missing non-secret fields, preview a
no-write codex-team transaction, and apply its exact digest through one explicit
approval. The generated private manifest and active profile load successfully;
excluded runners remain closed and MCP catalogs/resources are unchanged.

---

# INT-197 — Governed Remote Automation

Status: COMPLETE (2026-08-27)

Plan: `docs/exec-plans/completed/INT-197-governed-remote-automation.md`

Add typed named SSH/rsync and read-only Transmission RPC handoff from normal CA
agents to codex-team, with dry-run plan digests and approval-gated sync apply.
Keep arbitrary shell, model-supplied endpoints, credentials, runtime profile
mutation, and the existing MCP catalogs out of scope.

Current result: named remote discovery, bounded read-only Transmission RPC,
remote file comparison, digest-bound rsync planning/application, automatic
explicit-intent preflight, and normal CA approval are implemented. Focused,
MCP-compatibility, real hermetic handoff, formatting, strict Clippy, and release
checks pass without installation or live remote access.

---

# INT-194 — Dependency-Aware Verification and Automatic Failure Diagnosis

Status: COMPLETE (2026-08-27)

Plan: `docs/exec-plans/completed/INT-194-dependency-aware-verification.md`

Implement bounded dependency-graph verification, automatic terminal failure
diagnosis, exact mutation batches, an evidence ledger, privacy-safe quality
history, durable saved-session recovery, and a model-free event coordinator
without widening mechanistic authority or changing the MCP catalogs.

Current result:

- Dependency-aware graph selection, deterministic diagnosis, atomic mutation
  batches, `/evidence`, private quality history, saved-session recovery, and
  bounded event coordination are implemented.
- Focused tests and the one broad workspace matrix completed; its sole stale
  command-count snapshot was corrected and passed in isolation.
- Formatting, strict Clippy, all-feature release, release MCP compatibility,
  production isolation, and snapshot/protected-tree hygiene pass.

# INT-193 — Automatic Mechanistic Preflight and Governed Verification

Status: COMPLETE (2026-08-27)

Priority: P0

Type: Automatic CA/codex-team evidence and verification cooperation

Plan: `docs/exec-plans/completed/INT-193-automatic-mechanistic-preflight.md`

Acceptance:

- Enabled ordinary Main/headless runs receive bounded repository evidence
  before first provider I/O without requiring the model to construct a call.
- Exact edits trigger focused codex-team verification; unscoped changes use
  revision evidence and policy-controlled full verification.
- Failed evidence gates success and permits at most two verified repair passes.
- Only isolated input/revision/toolchain-bound successes enter the evidence
  cache; explicit unisolated passes remain advisory.
- Model-free Schedule V2 targets and bounded typed events make zero provider
  requests; every recursion exclusion and the 11/15/28 catalogs remain.

Current result:

- Protocol, executor, CA preflight/gating, conservative caches, schedule V2,
  event coordination, and learning integration are implemented.
- Focused protocol, composite, MCP, schedule, learning, role-run, exclusion,
  release-readiness, and schema-compatibility tests pass. The broad workspace
  run's one real schema failure was fixed; the failed codex-team target then
  passed 590 tests with one ignore.
- Formatting, strict lint, all-feature release, protected/snapshot hygiene,
  production Seatbelt, both real compatibility directions, installed
  `/delegate`, `/jobs`, help/version, and clean shutdown pass without live
  provider traffic.
- Validated CA and codex-team binaries are installed recoverably; the existing
  mode-0600 named profile is byte-identical.

---

# INT-192 — Live Mechanistic Delegation Hardening

Status: COMPLETE (2026-08-27)

Priority: P0

Type: CA/codex-team live delegation correctness and recovery

Plan: `docs/exec-plans/completed/INT-192-live-mechanistic-delegation-hardening.md`

Acceptance:

- Delegated search supports explicit literal/regex matching and tracked-file
  or directory-prefix scopes without absolute repository discovery.
- CA preserves actionable typed MCP errors and distinguishes invalid arguments
  from missing workspace paths.
- `/retry` continues from fully resolved tool receipts without replaying tools.
- Existing recursion exclusions and 11/15/28 zero-resource catalogs remain.

Current result:

- Literal/regex search and tracked exact-file/directory-prefix scopes work
  through the repository-pinned facade without absolute-root discovery.
- Typed invalid/stale/query failures survive CA transport and render with
  truthful success/failure and fallback semantics.
- `/retry` preserves resolved successful, failed, denied, and mutating tool
  batches while dropping only failed partial continuation text.
- Focused and completion evidence, exact release compatibility, protected-tree
  safety, recoverable installation, `/delegate`, `/jobs`, and clean shutdown
  pass. The post-install validation record names one unintended provider turn
  caused by using positional `doctor` instead of offline `--doctor`.

---

# INT-191 — Protected-Tree Baseline and Delegate Profile Activation

Status: COMPLETE (2026-08-26)

Priority: P0

Type: Protected evidence integrity and local mechanistic delegation activation

Plan: `docs/exec-plans/completed/INT-191-protected-tree-baseline-delegate-profile.md`

Acceptance:

- The ignored oracle/reference trees are verified against an exact tracked,
  HEAD-anchored digest baseline while malformed, stale, or changed state fails
  closed.
- Baseline preview/apply is revision- and digest-bound; the initial approved
  baseline is committed alone without absorbing unrelated dirty state.
- Validated CA and codex-team releases are recoverably installed and one
  private named profile exposes the exact local delegate facade without copying
  credentials or changing the default profile.
- Composer Left/Right navigation works while slash, file, or path completion is
  open, without regressing Unicode or native Option/Command movement.

Current result:

- Manifest-only commit `1530d660916ecb8d6f5befa68d1a850e9912c674`
  establishes the approved baseline and the real protected-tree receipt passes.
- The arrow regression is fixed and focused plus full input coverage passes.
- Full tests, lint, format, release build, hygiene, isolation, and both real
  compatibility directions pass.
- Validated releases are recoverably installed and the private mode-0600
  `codex-team-delegate` profile passes doctor, exact real-client compatibility,
  `/delegate`, `/jobs`, lazy startup, and clean shutdown without provider use.
- Local activation is guarded by exact artifact hashes, protected-tree
  verification, backups, and post-install checks; dirty Git state remains an
  audit fact, not an unrelated installation veto.

---

# INT-190 — CA ↔ Codex-Team Mechanistic Cooperation

Status: COMPLETE (2026-08-26)

Priority: P0

Type: Bidirectional CA/orchestrator mechanistic delegation

Plan: `docs/exec-plans/completed/INT-190-ca-codex-team-mechanistic-cooperation.md`

Acceptance:

- An opted-in normal TUI/headless CA agent sees one first-turn typed delegation
  facade without starting MCP during request preparation.
- CA supplies revision binding and host-polls codex-team queries/evidence/local
  observation without repeated model calls; excluded surfaces remain closed.
- `/delegate` and unified `/jobs` provide model-free status, cancellation, and
  categorical feedback while `ca role-run` remains non-recursive.
- Shared protocol, real-client, combined Worker-routing, focused UI, and full
  workspace release gates pass without live providers or runtime mutation.

Result:

- `codex-team-protocol` now supplies the shared closed wire contract while all
  execution and learning authority remains in codex-team.
- Opted-in ordinary CA TUI/headless runs expose one lazy facade; every excluded
  surface remains closed and raw delegate-server tools stay out of MCP search.
- `/delegate` and `/jobs` expose bounded readiness, durable recovery,
  cancellation, refresh, and fixed human feedback without provider calls.
- Exact 11/15/28 real-client compatibility, reverse Worker routing, production
  isolation, the serial workspace suite, strict lint, and release build pass.

---

# TUI-189 — Truthful Model Catalog Status

Status: COMPLETE (2026-08-26)

Priority: P0

Type: TUI provider/model discovery status

Plan: `docs/exec-plans/completed/TUI-189-truthful-model-catalog-status.md`

Result:

- Live, configured/documented fallback, and truly unavailable providers are
  mutually exclusive. A failed optional OpenAI Codex catalog lookup no longer
  claims the working Codex route is unavailable, and direct OpenAI fallback is
  no longer counted twice.
- The reported four-provider shape now reads `4 models · 2 live · 2 fallback`;
  genuinely model-less failed providers remain named as unavailable.
- Focused regressions, the complete serial workspace suite, formatting, strict
  all-feature Clippy, the all-feature release build, diff/snapshot/protected-tree
  hygiene, and release smoke checks pass. The verified binary was installed
  with a recoverable backup.

---

# TUI-188 — Collapsible Edit and Command Activity

Status: COMPLETE (2026-08-26)

Priority: P0

Type: TUI transcript grouping and mouse interaction

Plan: `docs/exec-plans/completed/TUI-188-collapsible-edit-command-activity.md`

Result:

- Runs of two or more direct file mutations and commands collapse into one
  status-aware activity header while exploration and standalone calls keep
  their prior presentation.
- Ctrl+O expands the latest block; clicking current, older, or scrolled visible
  headers toggles the exact block, while drag selection and overlays win.
- Expanded rows retain complete wrapped commands, existing diffs and adaptive
  output. Exact live receipts and conservative restored labels distinguish
  Created, Edited, Deleted, Created directory, and Changed without changing
  canonical output, sessions, approvals, provider behavior, or copy-all text.
- Targeted 60-, 100-, and 184-column snapshots, the complete serial workspace
  suite, formatting, strict all-feature Clippy, and the all-feature release
  build pass without network access or runtime-state mutation. After a separate
  explicit request, the validated release was installed with a recoverable
  backup and passed hash, signature, lookup, help, and version checks.

---

# ASY-187 — Dedicated `ca-watch` Utility and Token-Free Monitoring

Status: COMPLETE (2026-08-26)

Priority: P0

Type: Rust process observation, interactive tools, and unified activity TUI

Plan: `docs/exec-plans/completed/ASY-187-ca-watch-token-free-monitoring.md`

Result:

- A reusable `ca-watch` library and CLI observe typed local or fixed-SSH PID
  requests with versioned ordered events, identity protection, copy-marker
  semantics, bounded logs, safe cancellation, and degradation recovery.
- Interactive CA sessions can register or stop owner-scoped PID watchers once;
  Rust-only polling updates the activity strip, unified `/jobs`, and terminal
  toasts without provider calls, token accounting, persistence, or target-PID
  signaling.
- Plan, headless, service, scheduled, and child-agent catalogs omit the tools;
  remote registration receives one exact network-bearing approval.
- Focused crate/tool/approval/TUI coverage, the complete serial workspace suite,
  formatting, strict all-feature Clippy, the all-feature release build, both
  binaries' help/version checks, snapshot/diff hygiene, and protected-tree
  checks pass without live network/provider traffic, runtime-state mutation,
  target signaling, or binary installation.

---

# TUI-186 — Adaptive Tool-Output Layout

Status: COMPLETE (2026-08-26)

Priority: P0

Type: TUI output presentation and Unicode-safe tabular layout

Plan: `docs/exec-plans/completed/TUI-186-adaptive-tool-output-layout.md`

Result:

- Tool-output cards use the available transcript width, close every row, and
  cannot leak tabbed or wide-Unicode content into neighboring transcript text.
- Stable TSV blocks render as aligned tables with preserved columns and empty
  cells; collapsed previews remain bounded and Ctrl+O wraps the full capture.
- Canonical/model/session/native output remains unchanged. Five layout tests,
  all 182 render tests, the complete serial workspace suite, formatting,
  strict all-feature Clippy, and the all-feature release build pass without
  network traffic, runtime-state mutation, or binary installation.

---

# REL-185 — Live Network and Command UX Repair

Status: COMPLETE (2026-08-26)

Priority: P0

Type: Approval policy, composer routing, command presentation, and job polling

Plan: `docs/exec-plans/completed/REL-185-live-network-and-command-ux.md`

Result:

- Interactive shell work follows the Hermes/MiMo single-approval model: no
  redundant network-only prompt, while explicit Ask/Deny and non-TUI network
  authority boundaries remain intact.
- Approval prompts wrap and show complete commands, absolute paths in prose do
  not become slash commands, and bounded job polling reports running as running
  instead of failure.
- Focused approval/input/job coverage, the complete serial workspace suite,
  formatting, strict all-target/all-feature Clippy, and the workspace
  all-feature release build pass without live network traffic. After a
  separate explicit user request, that validated release was installed with a
  recoverable prior-binary backup and passed hash/signature/help/version checks.

---

# TUI-184 — Native macOS Composer Editing

Status: COMPLETE (2026-08-26)

Priority: P0

Type: Small TUI input and terminal-lifecycle correction

Result:

- Option+Backspace deletes the previous Unicode word and Command+Backspace
  deletes to the start of the current hard line without splitting atomic
  composer attachments.
- Option+Left/Right moves by word and Command+Left/Right moves to hard-line
  bounds, including while inline path completion is visible.
- Compatible terminals receive balanced enhanced-keyboard mode so Command can
  arrive as a distinct modifier. Remove Last Attachment moved to Ctrl+X A so
  it no longer intercepts Option+Backspace.
- All 105 composer-input tests, keymap and terminal lifecycle coverage, the
  complete serial workspace suite, formatting, strict all-target/all-feature
  Clippy, and the all-feature release build pass.

---

# REL-183 — Request-Scoped Read-Only SSH Authority

Status: COMPLETE (2026-08-26)

Priority: P0

Type: Interactive approval policy and SSH ergonomics

Plan: `docs/exec-plans/completed/REL-183-request-scoped-read-only-ssh.md`

Result:

- A live AutoReview or RunEverything request that names an SSH destination now
  runs closed read-only SSH inspections for that exact destination without a
  redundant network prompt; the reported two-call `root@bsd` batch is covered.
- Strict mode, explicit Ask/Deny, unmentioned hosts, attachment-only authority,
  remote mutation, forwarding, file transfer, and other network tools remain
  approval-gated.
- Focused permission/approval/SSH/AutoReview/workspace tests, the complete
  serial workspace suite, formatting, strict all-target/all-feature Clippy,
  and the all-feature release build pass without live SSH traffic, runtime-state
  mutation, or binary installation.

---

# TUI-182 — Prose-Safe Path Completion Editing

Status: COMPLETE (2026-08-26)

Priority: P0

Type: Small TUI input correction

Result:

- Ordinary slash-separated prose such as `red/ops` and `and/or` no longer
  opens the path-completion surface or starts filesystem discovery.
- Automatic inline path completion now requires `./`, `../`, `~/`, or an
  absolute `/` prefix; Tab completion still accepts bare relative paths.
- Left/Right move the composer cursor while a real path-completion surface is
  open without restarting unchanged discovery. Up/Down continue to select
  completion rows.
- Focused path/input tests, the complete serial workspace suite, formatting,
  strict all-target/all-feature Clippy, and the all-feature release build pass.

---

# SKL-181 — Session-Automatic Skill Activation

Status: COMPLETE (2026-08-25)

Priority: P0

Type: Model-selected session context and approval policy

Plan: `docs/exec-plans/completed/SKL-181-session-automatic-skill-activation.md`

Result:

- The bounded catalog tells the primary model to activate an exact relevant
  skill, and interactive, headless, and service sessions do so without generic
  approval interruption.
- Exact persistent Ask/Deny, digest, ambiguity, availability, budget, and
  session save/restore boundaries remain enforced.
- Skill activation grants no authority to later tools or declared
  dependencies; composer text remains free of passive suggestions.
- Focused skill/headless/service/session suites, formatting, strict workspace
  Clippy, and the release build pass. The one stale full-suite approval
  assertion was updated and passes exactly under the new contract; the final
  default serial workspace suite passes completely.

---

# TUI-180 — Quiet Skill Discovery

Status: COMPLETE (2026-08-25)

Priority: P1

Type: Small TUI presentation correction

Result:

- Ordinary composer text no longer replaces the status row with passive
  `Not activated` skill suggestions.
- Explicit `/skills <query>` ranking and exact `/skills activate <id>` behavior
  remain unchanged.
- Focused skill tests, formatting, strict workspace Clippy, and the all-feature
  release build pass. The full workspace test run had one unrelated
  process-cleanup timing failure that passed on an immediate exact rerun.

---

# LRN-179 — Cross-Session Recall and Skill Operations

Status: COMPLETE (2026-08-25)

Priority: P0

Type: Ordinary cross-session recall, conditional skills, bundles, and lifecycle audit

Plan: `docs/exec-plans/active/LRN-179-cross-session-recall-skill-operations.md`

Acceptance:

- Ordinary tasks can search bounded exact-after-redaction CA, Codex, Hermes,
  and MiMo/Xiao history without a provider sub-call or persisted transcript
  index.
- Skill availability/configuration and task-local bundles are exact,
  digest-pinned, fail-closed, and preserve CA's current authority boundaries.
- `/learning` provides metadata-only lifecycle history and deterministic
  suggestions without automatic asset mutation.

Result:

- Ordinary tasks and local `/recall` can search and page exact-after-redaction
  completed CA, Codex, Hermes, and MiMo/Xiao history through a lazy process-only
  FTS5 index. The searchable universe has no learning-window or corpus cap;
  every response remains independently bounded and foreign databases stay
  query-only.
- Hermes platform/tool/toolset conditions and non-secret settings are enforced,
  incompatible skills are hidden with exact diagnostics, and task-local
  two-to-five-skill bundles resolve exact digests atomically under the existing
  combined skill budget.
- The owner-only metadata ledger powers `/learning` and `/journey`, exact
  lifecycle drilldown, and deterministic refinement/staleness/bundle
  suggestions without storing transcript/tool content or mutating assets.
- Focused suites, all workspace tests, formatting, strict all-target/all-feature
  Clippy, the all-feature release build, diff hygiene, and eight new wide/narrow
  snapshots pass. No live provider, real-profile mutation, learning run, binary
  installation, or oracle/reference write occurred.

---

# LRN-178 — Efficient, Auditable Learning Results

Status: COMPLETE (2026-08-25)

Priority: P0

Type: Adaptive efficiency, signal-directed inspection, and compact audit UX

Plan: `docs/exec-plans/completed/LRN-178-efficient-auditable-learning-results.md`

Acceptance:

- A real-shaped 119-cluster zero-change fixture completes in two scripted turns
  with fitting catalog delivery and deep inspection of every high/medium cluster.
- Existing/proposed assets are distinguished, duplicate skips name exact paths,
  memory outcomes are explicit, and the normal transcript stays compact.
- `/learn status <run-id>` preserves the complete exact audit under a backward-
  compatible journal without reopening the completed real run.

Result:

- The complete fitting catalog, typed asset/memory indexes, deterministic
  signal, batched representative inspection, and exact reference validation
  make the real-shaped 190-session/1,596-message/119-cluster case complete in
  two scripted provider turns.
- Journal v5 stores the full decision audit while the normal receipt shows at
  most twelve prioritized rows, collapses singleton noise, resolves exact
  duplicate paths, and links to `/learn status <run-id>` for drilldown.
- Focused learning/TUI coverage and the complete workspace suite pass, as do
  formatting, strict all-target Clippy, and the release build. No live learning
  run, evidence reopening, profile mutation, or binary installation occurred.

---

# LRN-177 — Resilient Learning Finalization

Status: COMPLETE (2026-08-25)

Priority: P0

Type: Adaptive correction identity, truthful completion, and live acceptance

Plan: `docs/exec-plans/completed/LRN-177-resilient-learning-finalization.md`

Acceptance:

- A stale or hallucinated non-authorizing cluster reference cannot poison an
  otherwise complete additive correction, and never counts toward coverage.
- Unknown cluster authority, missing real clusters, conflicting outcomes, and
  incomplete publication continue to fail closed.
- Focused and completion gates pass before one repository-built bare `/learn`
  acceptance run.

Result:

- Non-authorizing stale cluster references are host-canonicalized away without
  losing valid accumulated decisions; unknown publication authority still fails
  closed, and correction schemas enumerate only current missing clusters when
  bounded.
- One repository-built bare `/learn` completed without retry as run
  `020c3875-ef34-4a0e-b034-51708fe22b0a`: 1,591/1,591 messages searchable, both
  catalog pages delivered, 118/118 clusters explicitly decided, and retirement
  committed atomically.
- The zero-change result correctly retained the existing MiMo rsync,
  Transmission, and seedbox commands. A live-report follow-up also fixed the
  missing Hermes source total.
- Focused suites, the final 2,599-test workspace run, formatting, strict Clippy,
  all-feature release build, release smokes, and diff/snapshot hygiene pass.
  After explicit user authorization, that exact validated release was installed
  at `/Users/aji/.local/bin/ca` with SHA-256 `5474464afa6dadf5c2e6497481ff31c72a62c2895d334bc35ec3a5d841c9d415`;
  the prior binary remains available as a timestamped recovery backup.

---

# LRN-176 — Foreign Knowledge Interoperability

Status: COMPLETE (2026-08-25)

Priority: P0

Type: Read-only portable skills and bounded cross-agent knowledge ingestion

Plan: `docs/exec-plans/completed/LRN-176-foreign-knowledge-interoperability.md`

Result:

- CA mounts compatible Codex, Hermes, and MiMo skill trees in place as
  immutable managed instances; the real registry exposes 81 Hermes, 22 MiMo,
  and one Codex skill without the former duplicate system instance.
- MiMo-compatible commands are ordinary slash prompts with literal arguments,
  and existing BSD rsync, Transmission, and seedbox commands are discoverable.
- Bounded sanitized foreign memory notes participate in normal recall and
  direct learning. Hermes completed main sessions are a standard query-only
  learning source with safe action/error/recovery evidence and no raw output,
  reasoning, email, secret, incomplete-turn, or subagent leakage.
- Focused suites, all 2,593 workspace tests, formatting, strict Clippy, and the
  release build pass. No `/learn` run, foreign write, provider call, installed
  binary change, or runtime publication occurred.

---

# LRN-175 — Closed-Loop MiMo/Hermes Learning

Status: COMPLETE (2026-08-25)

Priority: P0

Type: Procedural learning, complete skill lifecycle, and truthful cluster decisions

Plan: `docs/exec-plans/completed/LRN-175-closed-loop-mimo-hermes-learning.md`

Result:

- One direct `/learn` searches the complete archive, decides every workflow
  cluster, and can create or refine complete active skill trees without a
  review ceremony or publication-time execution sandbox.
- Procedural action outcomes, recovery, corrections, and stopping conditions
  are retained safely without exposing raw tool outputs or authority.
- Exact tree digests, atomic journals, normal runtime approvals, same-task
  feedback, and one-command undo preserve CA's stronger safety boundary.
- Complete skill trees with scripts, references, templates, assets, and
  descriptive dependencies activate after structural validation without
  publication-time execution or installation.
- Valid partial submissions now accumulate across correction turns instead of
  replacing one another. Host-compacted submissions also clear superseded
  canonical provider items, preventing mismatched OpenAI Responses replay.
- Real run `58f65db2-272f-464c-bac3-b610be749ef3` completed against 186
  sessions / 1,551 messages: 116/116 clusters decided, 69 sessions retrieved,
  11 inspected, 66 cited as support, and 1,551 messages retired only after
  commit. Its truthful zero-change result preserved the existing reusable
  agent and published no redundant memory or skill package.
- Focused learning/skill/command/TUI suites, the complete test suite,
  formatting, strict Clippy, and the release build pass. The release artifact
  remains uninstalled.

---

# LRN-171A — Full-Archive, Truthful Learning Coverage

Status: COMPLETE (2026-08-24)

Priority: P0

Type: Adaptive retrieval, coverage accounting, and targeted repair

Plan: `docs/exec-plans/completed/LRN-171A-full-archive-truthful-learning-coverage.md`

Result:

- Every eligible recent session/message remains searchable in a run-local
  sanitized index; 32 bounds one search result and eight bounds one inspection,
  rather than truncating the searchable universe.
- Deterministic workflow clusters, paginated catalog delivery, exact retrieval
  and inspection identifiers, truthful partial/full retirement, and versioned
  recoverable coverage journals are implemented and proven.
- The 184-session/1,515-message late-workflow fixture produces all three BSD
  commands in two provider turns, while dedup remains a candidate and one-off
  FreeBSD work is skipped.
- The named coverage-less legacy run was reopened exactly once without removing
  its memory or assets. Review authority is now bound to the selected sanitized
  archive-wide message projection, so movement between registered roots does
  not cause false source drift while selected-content changes remain guarded.
- The repository-built CA completed bare `/learn` as run
  `9c24e870-3ae0-4036-b95d-80879b283659`: 1,533/1,533 messages searchable and
  retired across 186 sessions, all 116 clusters delivered, and one immediately
  discoverable `.ca/commands/review-oracle-gaps.md` command published with a
  working run-scoped undo receipt.

---

# LRN-174 — MiMo End-to-End Learning and Dream Repair

Status: COMPLETE (2026-08-24)

Priority: P0

Type: Adaptive retrieval, first-run publication, and maintenance recovery

Result:

- Read-only MiMo source review is reflected in ephemeral session-title
  discovery, ranked broad-term search, bounded excerpts, and strict direct
  finalization without weakening CA's privacy or authority boundaries.
- A first bare `/learn` is proven end-to-end: automatic discovery, two provider
  turns, memory plus native command publication, compact receipt, and one-command
  undo, with no review-era command or second invocation.
- Dream evidence sanitization is UTF-8-byte-bounded and idempotent, eliminating
  the supplied unchanged 8,234-byte retry failure; a complete enrolled
  queue/reload/export regression covers the background-maintenance path.
- Focused learning tests, both workspace matrices (2,567 default; 2,714
  all-feature), formatting, strict Clippy, the all-feature release build, and
  release smokes pass.

---

# LRN-173 — First Direct Learning Run Repair

Status: COMPLETE (2026-08-24)

Priority: P0

Type: Direct learning control flow

Result:

- Automatic source discovery hands its freshly synchronized evidence directly
  to `/learn` or `/distill` instead of clearing it before route preparation.
- Existing registered sources with no pending review are synchronized inside
  the same direct invocation, so no review-era command is required.
- The exact first-run regression, all 50 archive-learning tests, the 2,562-test
  workspace suite, formatting, strict Clippy, release build, and smokes pass.

---

# LRN-172 — Full Command-Operator Examples

Status: COMPLETE (2026-08-24)

Priority: P0

Type: Slash-command readability

Result:

- Command usage strings are no longer clipped to a fixed 32-cell column.
- Wide rows expand their label column; constrained rows stack and wrap without
  replacing operators with an ellipsis.
- Exact `/learn` and `/distill` regressions, wide/narrow snapshots, all 177 TUI
  render tests, and the 2,561-test workspace suite pass.
- Formatting, strict Clippy, the all-feature release build, release smokes, and
  diff/snapshot/protected-tree hygiene pass.

---

# LRN-171 — Direct Adaptive MiMo-Style Learning

Status: COMPLETE (2026-08-24)

Priority: P0

Type: Direct adaptive learning, reusable commands, and transcript usability

Plan: `docs/exec-plans/completed/LRN-171-direct-adaptive-distillation.md`

Result:

- `/learn [focus]` and `/distill [focus]` run immediately with bounded adaptive
  retrieval, in-run repair, direct validated publication, cancellation, and a
  compact normal-transcript receipt.
- Commands, skills, and safe read-only agents use evidence-based scope, exact
  generated provenance, human conflict protection, registry refresh, aggregate
  journaling, and undo.
- The MiMo BSD fixture packages rsync, Transmission, and seedbox commands,
  defers variable deduplication, and skips one-off FreeBSD work.
- The completion audit added logical-turn resume, visible investigation cards,
  active-model preference, active-workspace authority, exact cancellation,
  prepared/applied/committed grouped publication recovery, and whole-run undo.
- Native CA session bundles use their native metadata/history contract; valid
  Codex rollouts above 64 MiB stream safely, and unusable siblings no longer
  stop valid evidence.
- The optimized uninstalled binary previewed the real 1.063 GB source set in
  3.91 seconds with zero provider calls and zero profile writes.
- Both workspace matrices (2,560 default; 2,707 all-feature), all 49 focused
  archive-learning tests, formatting, strict Clippy, the all-feature release
  build, release smokes, snapshots, and diff/protected-tree hygiene pass.

---

# LRN-170 — MiMoCode-Style Learning and Distillation

Status: COMPLETE (2026-08-24)

Priority: P0

Type: Trajectory fidelity, one-pass learning, and workflow packaging

Plan: `docs/exec-plans/completed/LRN-170-mimocode-distillation-parity.md`

Acceptance:

- Preserve bounded safe tool-action evidence from recent native and MiMoCode
  sessions so repeated workflows can actually be recognized.
- Inventory existing supported assets and complete reviewed learning in one
  resumable provider call with exact approval, validation, publication, and
  undo boundaries intact.
- Finish with a compact Created/Skipped/Needs-more-evidence summary; creating
  nothing remains a successful evidence-backed outcome.

Result:

- Native and MiMoCode histories retain bounded safe tool-action sequences,
  prioritize recent sessions under pressure, and keep older corroboration;
  outputs, email, secrets, subagents, and incomplete turns stay excluded.
- Reviewed learning inventories supported skills/agents and completes in one
  structured, checkpointed provider call with exact publication and undo
  boundaries unchanged; direct Distill uses the same asset inventory.
- Both workspace matrices, focused learning/TUI tests, formatting, strict
  Clippy, the all-feature release build, and hygiene checks pass.

---

# LRN-169 — In-TUI Learning Approval Progress

Status: COMPLETE (2026-08-24)

Priority: P0

Type: Manual learning approval and provider-call progress

Plan: `docs/exec-plans/completed/LRN-169-in-tui-learning-approval-progress.md`

Acceptance:

- `/learn approve` authorizes the currently priced evidence revision without
  digest retyping or another terminal.
- The responsive learning pager shows bounded Dream/Distill call progress until
  the inert proposal is ready, without exposing evidence text.
- Exact-digest drift, separate proposal publication approval, and strict
  supervisor authority parsing remain intact.

Result:

- `/learn approve` now has generation-tagged `0/N` through `N/N` progress and
  owner-only exact-plan checkpoints that resume only missing calls.
- One style-only final-line stream cursor eliminates the reported transcript
  jump; privacy-safe copy-all and bounded cross-scroll selection cover complete
  sessions and pagers.
- Both workspace matrices, formatting, strict Clippy, all-feature release
  build, reviewed snapshots, native disposable PTY, and hygiene checks pass.

---

# LRN-168 — Guided, Copyable Learning Review

Status: COMPLETE (2026-08-23)

Priority: P0

Type: Manual learning review usability and pager copying

Plan: `docs/exec-plans/completed/LRN-168-guided-copyable-learning-review.md`

Acceptance:

- `/learn` and `/learn review` provide a responsive two-step evidence/proposal
  flow without digest retyping.
- Source drift refreshes once and always requires fresh exact approval.
- Pager text supports visible drag-copy and complete off-screen Copy All.
- Existing CLI/storage/service boundaries remain unchanged.

Result:

- Exact evidence and proposal digests now stay inside typed TUI state, with
  dedicated asynchronous work and stale-generation rejection.
- Later-first actions cover approval, rejection, all/memory/skills application,
  retry, and bounded one-refresh drift recovery without digest retyping.
- Pager drag-copy, complete Copy All, deterministic failures, reviewed
  wide/narrow snapshots, 2,517 workspace tests, formatting, strict Clippy,
  release build, and hygiene checks pass.

---

# SES-165 — Understandable Session Names

Status: COMPLETE (2026-08-23)

Priority: P0

Type: Semantic session naming and browser clarity

Plan: `docs/exec-plans/completed/SES-165-understandable-session-names.md`

Acceptance:

- New unnamed persistent sessions receive concise generated names without
  delaying or risking the first turn.
- Explicit/resumed names remain authoritative, and old active/archive sessions
  can be repaired individually from `/sessions`.
- The browser explains its tabs, search, controls, and poor-name repair path in
  readable language at wide and narrow sizes.

Result:

- A separate tool-free, accounted request replaces only the matching local
  fallback; failure is silent and stale results are rejected.
- Ctrl+G loads and renames exactly the highlighted saved session through the
  UUID-checked active/archive persistence boundary.
- Focused fake-provider/session/input/render tests, 2,492 workspace tests,
  formatting, strict Clippy, release build, snapshots, and smokes pass.

---

# SES-164 — Named Session Browser

Status: COMPLETE (2026-08-23)

Priority: P0

Type: Session naming and history presentation

Plan: `docs/exec-plans/completed/SES-164-named-session-browser.md`

Acceptance:

- `/sessions` presents descriptive names grouped by date with updated times,
  while IDs/workspaces remain searchable rather than primary display rows.
- Saved active and archived sessions can be renamed safely from the browser.
- Attachment/path envelope text does not displace an explicit user request in
  automatic local titles.

Result:

- The browser is wide and title-first, with `Today`/date headings, one row per
  session, local-offset times, and preserved Active/Archive controls.
- Ctrl+R opens a bounded editor backed by exact UUID-checked atomic metadata
  persistence; empty, overlong, or control-bearing names are rejected.
- Focused suites, reviewed snapshots, 2,484 workspace tests, formatting,
  strict Clippy, release build, diff hygiene, and help/version smokes pass.

---

# SES-163 — Primary Sessions Browser

Status: COMPLETE (2026-08-23)

Priority: P0

Type: Session discovery and navigation

Plan: `docs/exec-plans/completed/SES-163-primary-sessions-browser.md`

Acceptance:

- `/sessions` is the canonical browser for former saved conversations, with
  `/resume` and `/continue` retained as aliases.
- `/session` remains available but no longer displaces session history in
  common-command discovery.
- The current saved session is excluded consistently from Active counts,
  filtering, navigation, and actions; Archive remains reachable.

Result:

- `/sessions [session-id]` is canonical; `/resume` and `/continue` are aliases,
  while `/session` remains available outside common-command priority.
- One shared picker projection removes the current UUID before Active display
  and interaction without changing storage or orchestration safety guards.
- Focused suites, reviewed snapshots, 2,478 workspace tests, formatting,
  strict Clippy, release build, diff hygiene, and help/version smokes pass.

---

# REL-162 — Low-Friction Bounded Autonomy

Status: COMPLETE (2026-08-23)

Priority: P0

Type: Approval fatigue, workspace authority, child reliability, and recovery

Plan: `docs/exec-plans/completed/REL-162-low-friction-bounded-autonomy.md`

Acceptance:

- Default AutoReview runs prepared local workspace work and owned-job lifecycle
  operations without repeated prompts; strict Ask and explicit policy remain.
- Typed reads and read-only children can inspect requested absolute paths while
  writes remain confined to workspace or conventional/private temp roots.
- Unknown named agents fail before approval, private temp usage is explicit,
  and provider progress timeouts tolerate long tool continuations.

Result:

- AutoReview now runs prepared local tools and owned job controls without
  re-prompting while explicit policy and external-authority boundaries remain.
- External reads, inherited child scopes, named-agent preflight, private-temp
  guidance, and provider progress windows are corrected with focused coverage.
- The final 2,472-test workspace suite, formatting, strict Clippy, release
  build, and diff hygiene pass; no live-provider or installed-binary check ran.

---

# TUI-161 — Readable Slash-Command Descriptions

Status: COMPLETE (2026-08-23)

Priority: P1

Type: TUI command discovery and responsive layout

Acceptance:

- The slash-command palette uses available wide-terminal space instead of a
  fixed 74-column ceiling.
- Common command descriptions remain fully readable at 100 columns while
  narrow terminals retain their stacked rows.
- Deterministic render coverage and the required Rust completion gates pass.

Result:

- Wide palettes now keep two-cell outer margins and cap command syntax at 32
  cells, prioritizing the explanatory description column.
- The focused readability regression, all 166 render tests, the full workspace
  suite, formatting, strict Clippy, release build, diff hygiene, and snapshot
  hygiene pass.

---

# REL-156 — Reproducible Release Baseline

Status: COMPLETE (2026-08-29)

Priority: P0

Type: Release reproducibility, root CI, and machine-readable evidence

Plan: `docs/exec-plans/active/REL-156-reproducible-release-baseline.md`

Acceptance:

- One pinned root workflow validates the complete `ca`/`codex-team` workspace
  from tracked source.
- `codex-team verify release-readiness --repo . --json` emits a stable bounded
  receipt and refuses readiness for dirty, incomplete, or drifting inputs.
- Clean-integration and release actions remain separately authorized; current
  working bytes, protected evidence, credentials, and runtime state are
  preserved.

Current result:

- The pinned root workflow, Rust 1.97.1 toolchain, and deterministic
  `codex-team.release-readiness.v1` audit are implemented and locally tested.
- The checkout is correctly not release-ready because the large pre-existing
  dirty/untracked integration is not a reproducible tracked source state; no
  cleanup, commit, remote run, or release action was authorized.

---

# AGT-157 — Host-Owned Agent Verification and Budgets

Status: IN PROGRESS (2026-08-23)

Priority: P0

Type: Agent completion integrity, evidence receipts, and autonomy budgets

Plan: `docs/exec-plans/completed/AGT-157-host-owned-agent-verification.md`

Acceptance:

- Assistant text alone cannot complete a graph task or release a dependent.
- Only a host-owned revision-bound pass receipt authorizes completion; legacy,
  failed, drifting, or inconclusive evidence pauses safely.
- Physical requests, tokens/cost, tool activity, retries, time, attempts,
  children, and retained output consume one monotonic finite budget.

Current result:

- Host-owned artifact and exact-command evidence bind source/workspace state,
  terminal events, and receipt digests; explicit human review is available
  only through a declared HumanReview gate.
- Writable primary/worktree lanes serialize, cross-lane edges require an
  explicit handoff, and all provider requests, usage/cost, tools, children,
  output, time, retries, and restored use consume one finite graph budget.
- Focused suites, both full workspace matrices, formatting, strict Clippy,
  release build, diff/snapshot hygiene, and oracle/reference inspection pass.

---

# SEC-158 — Privilege Supervisor and Authenticated Service V2

Status: IN PROGRESS (2026-08-23)

Priority: P0

Type: Privilege lifecycle, secret handling, and service protocol security

Plan: `docs/exec-plans/active/SEC-158-privilege-supervisor-and-service-v2.md`

Acceptance:

- The shared fake-tested supervisor clears environment, separates password and
  target stdin, executes one exact `sudo -A -p <private-fifo> -k -H -- ...`,
  invalidates before and after every command, and latches unavailable on
  cleanup failure.
- Main-TUI elevation is exact, one-use, system-wide, separately network-
  approved, and secret-free outside the host supervisor.
- Frozen v1 remains unchanged; authenticated WebSocket-only v2 carries bound
  encrypted privilege secrets while stdio and attach reject them.

Current result:

- The main interactive macOS Agent-mode handoff now uses a closed native
  utility set, exact one-use approval, masked keyboard input, a private
  askpass FIFO hidden from the target sandbox, cleared environment, bound cwd,
  descendant cleanup, and `sudo -k` before and after. Linux/FreeBSD and every
  unattended/non-Agent surface fail closed.
- Automated validation is fake-only. The `run_elevated_command` rename,
  separately approved network authority, and authenticated service v2 remain
  open; no real sudo/password acceptance run was performed.

---

# TUI-159 — Markdown Fidelity at Terminal Widths

Status: COMPLETE (2026-08-23)

Priority: P0

Type: TUI Markdown correctness and deterministic rendering

Acceptance:

- Markdown links render the label exactly once without injecting the
  destination into visible layout text.
- Tables preserve every cell's graphemes at wide, medium, and narrow terminal
  widths without exceeding the requested width.
- Focused link, Unicode, and width/content-preservation tests pass without
  oracle/reference changes or terminal escape injection.

Result:

- Links now lay out only their label, including linked inline code, and never
  inject the destination into terminal geometry. Tables preserve arbitrary
  column counts and all graphemes through bounded grid or stacked rendering.
- All 17 Markdown tests pass, including widths 100/60/40, more than eight
  columns, CJK/emoji, unsafe destinations, and malformed syntax.

---

# REL-160 — Stabilize Stateful Clean-Filter Regression

Status: IN PROGRESS (2026-08-29)

Priority: P1

Type: Test determinism and release-gate reliability

Acceptance:

- The clean-filter regression proves that only approved bytes reach a commit
  without assuming a Git-version/timing-specific exact filter invocation
  count.
- The focused test and repeated parallel `codex-team` library runs pass while
  retaining the content, index, and HEAD compare-and-swap assertions.

Evidence:

- The final all-feature matrix observed one invocation where the test expected
  two; the unchanged focused rerun passed. Two earlier complete matrices also
  passed, identifying a nondeterministic assertion rather than a product-path
  failure.
- ORC-218's post-PRT-230 serial workspace rerun reproduced the same one-versus-
  two assertion after an immediately preceding complete codex-team package
  pass. The repair now accepts Git's valid one-or-two pre-publication filter
  calls while still rejecting a third call and verifying exact committed
  content, an empty index, and HEAD compare-and-swap behavior.

---

# LRN-167 — Manual MiMoCode Session Learning

Status: COMPLETE (2026-08-23)

Priority: P0

Type: Local evidence acquisition

Plan: `docs/exec-plans/completed/LRN-167-manual-mimocode-session-learning.md`

Result:

- Manual `ca learn run` now auto-discovers bounded completed main-slice
  MiMoCode evidence through a query-only SQLite snapshot.
- Existing sanitation, secret rejection, workspace/fork deduplication, and
  memory/inert-skill-only boundaries are preserved; no MiMo transcript enters
  `/sessions` or persisted index text.
- Focused tests, 2,510 workspace tests, format, strict Clippy, all-feature
  release, help/version, live read-only preview, render snapshots, and diff
  hygiene pass. The background service remains disabled.

---

# LRN-166 — Complete and Activate the Autonomous Learning Loop

Status: ACTIVE — implementation validated; background activation deferred by user

Priority: P0

Type: Continuous local evidence learning and memory/inert-skill publication

Plan: `docs/exec-plans/active/LRN-166-autonomous-learning-loop-completion.md`

Completed implementation:

- Unified `ca learn status|run|pause|resume|service ...` and `/learn` status.
- Default native plus Codex active/archive autonomous harvesting with existing
  bounds, filtering, partitioning, and deduplication.
- Digest-bound export/completion, cross-provider replay, exact journaled
  memory/inert-skill publication, terminal acknowledgement, retry, and undo.
- One knowledge-only stable supervisor, isolated external-reference execution
  profile, legacy-scanner migration, health/foreground/disk fail-closed gates,
  and no recurring approval prompts.
- Focused and full validation pass: 2,500 workspace tests, format, strict lint,
  all-feature release, help/version, service preview, and diff hygiene.

Next acceptance slice:

- Keep the LaunchAgent disabled while learning is manual.
- If the user later requests background operation, generate a fresh service
  preview that includes the MiMoCode database when present; do not reuse the
  historical four-root activation digest.
- Then verify one loaded service, nonzero evidence/messages, one bounded
  terminal workflow, visible usage, and no recurring approval prompts.

---

# INT-151 — Autonomous Learning and Delegation Cockpit

Status: ACTIVE — exact learning activation, per-role settings, and production isolation host pending

Priority: P1

Type: Learning cockpit and delegation routing

Plan: `docs/exec-plans/active/INT-151-autonomous-learning-cockpit.md`

Next acceptance slice:

- Complete LRN-166's exact service activation and live terminal-workflow smoke.
- Add backward-compatible per-orchestrator-role `same`, `fixed`, and `learned`
  routing settings.
- Rerun the production release operate check on an isolation-capable host
  before any real enrollment or provider traffic.

---

# SEC-155 — Linux and FreeBSD Privilege Handoff

Status: COMPLETE (2026-08-23)

Priority: P0

Type: Privilege portability, native executable trust, and sandbox closure

Plan: `docs/exec-plans/completed/SEC-155-linux-freebsd-privilege-handoff.md`

Acceptance:

- The main interactive Linux and FreeBSD TUI uses SEC-154's exact approval,
  masked one-attempt authentication, private owner/nonce, and byte-redacted
  execution protocol with a compile-time fixed native sudo helper.
- Linux/FreeBSD mandatory sandbox mode fails before approval or credential
  entry; policy-only/off remain explicit and are never widened implicitly.
- Fake-helper and platform-contract tests require no real sudo, credential,
  runtime-state mutation, or protected-evidence change.

Result:

- The same exact approval, masked one-attempt authentication, private detached
  execution, and streaming redaction protocol now supports fixed native sudo
  helpers on macOS, Linux, and FreeBSD.
- Linux/FreeBSD native binaries require ELF identity. Required sandbox mode
  fails before consent because no mandatory backend exists there; explicit
  policy-only/off modes are preserved without silent widening.
- Local focused and full-workspace tests pass. An explicitly authorized
  FreeBSD probe verified the installed secure sudo identity; cached
  non-interactive validation was denied by policy before target execution. An
  unrelated existing `codex-team` compile error was not pursued.

---

# SEC-154 — Safe Interactive macOS Privilege Handoff

Status: COMPLETE (2026-08-23)

Priority: P0

Type: Privilege boundary, secret handling, process execution, and TUI consent

Plan: `docs/exec-plans/completed/SEC-154-safe-interactive-privilege-handoff.md`

Acceptance:

- The main macOS TUI can execute one exact root-secured argv only after a dedicated one-use approval and, when needed, masked host-only sudo authentication.
- Password bytes never enter generic stdin, model/session/hook/trace/history surfaces, or unredacted stdout/stderr capture.
- The target receives closed stdin and the existing sandbox with network denied; every unattended, remote, delegated, Plan-mode, and non-macOS surface fails closed.
- Hermetic fake-sudo coverage proves success, cache miss, wrong password, cancellation, timeout, stale/wrong-owner rejection, exact binding, and split-stream echo redaction without invoking real sudo.

Result:

- `run_privileged` is a closed, bounded, macOS-only main-TUI tool whose
  root-secured native argv, cwd, sandbox state, helper/program identities, and
  one-use nonce are bound to one exact digest and one explicit approval.
- Cached authorization, one-attempt masked authentication, target preparation,
  detached execution, streaming byte redaction, hooks, cancellation, and
  ordered mixed-tool continuation are host-owned; privilege processes never
  enter the generic job registry.
- Headless, service, scheduled, Plan-mode, subagent, non-macOS, failed-helper,
  and forged direct-call paths omit or reject the tool. Explicit denies remain
  authoritative while every allow/grant/Yolo/AutoReview path is ignored.
- Fake-sudo and deterministic TUI coverage plus the complete default and
  all-feature suites, formatting, strict lint, release, smoke, snapshot, diff,
  and protected-tree checks pass. No real sudo or credential was used.

---

# CON-153 — One-Project `ca` Monorepo Consolidation

Status: COMPLETE (2026-08-22)

Priority: P0

Type: Repository consolidation, cross-crate integration, and recoverable cleanup

Plan: `docs/exec-plans/completed/CON-153-one-project-monorepo.md`

Acceptance:

- `/Users/aji/project/ca` is the only working project folder for `ca` and
  `codex-team`, with both binaries represented in one Cargo workspace.
- Current `ca`, completed role-run integration, orchestrator history, and all
  unique dirty bytes are preserved before redundant folders are retired.
- Workspace tests, strict lint, release builds, cross-boundary checks, and
  source/snapshot hygiene pass without provider traffic or runtime mutation.

Result:

- The root workspace now contains `ca` and `crates/codex-team` with one lockfile.
- All side histories and unique pending source bytes are preserved by local
  archive refs and source-only recovery archives.
- Full workspace tests, strict lint, formatting, release builds, smokes, and
  hygiene checks passed; generated build trees were then removed.
- The four redundant project paths are absent. Recoverable source checkouts
  are in `/Users/aji/.Trash/ca-consolidation-20260822-145511`.

---

# OPT-152 — Hosted API Context Efficiency

Status: COMPLETE (2026-08-22)

Priority: P0

Type: Prompt efficiency, context compaction, command condensation, and metrics

Plan: `docs/exec-plans/completed/OPT-152-hosted-api-context-efficiency.md`

Acceptance:

- Stable prompt construction guides targeted batched retrieval without adding
  volatile prefix material.
- Checkpoints preserve facts, hypotheses, and open uncertainty as distinct
  canonical state.
- Explicit semantic command-output mode reduces eligible model-visible logs
  while bounded mode and raw process evidence remain intact.
- Request and usage surfaces expose stable-prefix estimates, exact-known cache
  efficiency, total/first-output latency, and provider tool round trips.

Result:

- Stable prompt guidance, uncertainty-preserving checkpoints, and explicit
  semantic model-result condensation are implemented across interactive,
  headless, and service continuations; bounded remains the default.
- `/request`, `/usage`, and `/session` now expose the truthful cache-boundary,
  cache/new-input, latency, and tool-round-trip observations in scope.
- Focused/default/all-feature, formatting, strict-lint, release, smoke,
  snapshot, diff, and protected-tree gates pass. Concurrent tool execution is
  intentionally deferred to a separate approval/order design.

---

# PRV-151 — Model-Scoped Prompt Cache Contracts

Status: COMPLETE (2026-08-22)

Priority: P0

Type: Provider cache contracts, prompt epochs, usage, pricing, and TUI

Plan: `docs/exec-plans/completed/PRV-151-model-scoped-prompt-cache-contracts.md`

Acceptance:

- Cache identity is stable through one saved conversation and its tool loops,
  while exact route/model/layout/prefix/tool changes rotate it.
- Exact first-party and custom model contracts control outbound fields, usage
  normalization, policy choices, and write-billing semantics independently.
- Checkpoint summaries reach the wire once and never activate mid-turn; the
  dedicated model-cache UI exposes only truthful choices.

Result:

- Exact built-in and custom contracts, scoped/frozen prompt epochs, checkpoint
  projection, canonical MCP hashing, cache usage provenance, and write billing
  are implemented.
- Direct OpenAI Responses receives only its declared explicit controls; Codex,
  MiMo, and DeepSeek remain provider-managed.
- `/cache provider/model` and Shift+C expose reviewed wide/narrow model-cache
  screens while legacy provider-wide settings remain read-only fallback.
- Focused/default/all-feature, formatting, strict lint, release, smoke,
  snapshot, diff, and oracle/reference gates pass.

---

# LRN-150 — Reviewed Codex Archive Learning

Status: ACTIVE — autonomous wiring validated; exact activation pending (2026-08-23)

Priority: P0

Type: Streaming foreign evidence, reviewed learning, and local background indexing

Plan: `docs/exec-plans/active/LRN-150-reviewed-codex-archive-learning.md`

Acceptance:

- Registered Codex archives are streamed, deduplicated, workspace-preserving,
  and indexed without raw transcript persistence or ordinary session import.
- Manual provider/application review remains available as a compatibility
  path. The enrolled autonomous path may publish only replay-approved memory
  and inert skills after one exact service activation.
- One secret-free knowledge-only supervisor owns scanning and evaluation; it
  never receives source, binary, routing, permission, executable, credential,
  plugin, dependency, or ordinary-session authority.

Current result:

- Streaming/index, exact evidence and proposal reviews, Dream/Distill journal
  integration, `/learn`/`ca learn`, and sandboxed LaunchAgent lifecycle are
  implemented with focused deterministic coverage.
- Focused/default/all-feature, format, strict-Clippy, release, smoke,
  wide/narrow snapshot, diff, and protected-tree gates pass.
- Final preview-only validation saw 93 live files / 504,570,420 bytes / 1,145
  completed candidates after four files appeared since the initial 89-file
  inventory. No profile state, provider traffic, installation, or archive
  mutation occurred.
- LRN-166 now supplies the missing archive-to-supervisor terminal completion
  and publication path; all pre-activation gates pass.
- Remaining: the one exact service activation and live terminal-workflow smoke.

---

# REL-149 — Reliable Tool Execution and Scoped Consent

Status: COMPLETE (2026-08-22)

Priority: P0

Type: Tool correctness, approvals, sandboxing, and optional capability UX

Plan: `docs/exec-plans/completed/REL-149-reliable-tools-scoped-consent.md`

Acceptance:

- Contextual patches, optional skill/LSP availability, and mutation diagnostics
  are correct and quiet by default.
- Scoped authority changes affect matching queued calls while write, process,
  network, deny, and external-communication boundaries remain fail closed.
- Required-sandbox Git and approved cached Cargo validation work with narrow
  read-only runtime roots and no credential/config exposure.

Result:

- Unified patches are atomic, contextual, multi-hunk, newline-faithful, and
  loop-guarded on repeated identical failures.
- Skill/LSP tools reflect actual runtime availability; ordinary LSP absence is
  quiet and invalid skill selectors fail before consent.
- Exact current-run/path grants reclassify matching queued calls in order while
  Ask, deny, network, outbound communication, and one-shot choices remain
  isolated.
- Required-sandbox Git and cached offline Cargo pass the exact reproduced
  commands; managed `read_lints`, trusted executable, and inferred-network
  boundaries are covered.
- Focused, default/all-feature, formatting, strict lint, release, smoke,
  snapshot, diff, and oracle/reference gates pass.

---

# TUI-148 — fx-Inspired Quiet Default

Status: COMPLETE (2026-08-22)

Priority: P0

Type: Terminal presentation and command discovery

Plan: `docs/exec-plans/completed/TUI-148-fx-quiet-default.md`

Acceptance:

- Idle startup shows one context line, one composer line, and one status line without duplicated model, workspace, shortcut, newline, or zero-message labels.
- Empty `/` discovery foregrounds common actions while retaining every visible command and all filtered matching behavior.
- Approvals, failures, queues, overlays, transcript expansion, command identities, and persistence contracts remain unchanged.

Result:

- Idle startup is a three-row quiet surface: compact context, borderless composer,
  and `/ commands`; configured title/status facts are not duplicated.
- Empty `/` discovery leads with ten common actions while preserving every visible
  registry command exactly once and leaving filtered search unchanged.
- Focused TUI, complete default/all-feature, formatting, lint, release, smoke,
  snapshot, diff, and oracle/reference checks pass.

---

# REL-147 — Route-Aware Token Estimates and Relevant Memory Recall

Status: COMPLETE (2026-08-19)

Priority: P0

Type: Context reliability and memory retrieval quality

Plan: `docs/exec-plans/completed/REL-147-route-token-memory-retrieval.md`

Acceptance:

- Automatic checkpointing uses an exact-route estimator identity and a
  conservative local fallback rather than raw UTF-8 bytes/4.
- Recalled user/workspace memory is selected deterministically for the current
  request within existing bounds, with a small broad-preference reserve.
- No tokenizer/network request, vector dependency, persisted index, schema
  rewrite, secret exposure, or change to unrelated providers occurs.

Current result:

- Exact official OpenAI/Codex, MiMo, and DeepSeek routes carry distinct
  conservative estimator identities; proxies remain generic and fallback
  attempts recompute the complete prepared breakdown.
- Interactive, headless, side-question, and service prompt construction now
  ranks recall per current request. Focused context, memory, service, fallback,
  side-question, mutation, and reviewed context-dashboard tests pass. Complete
  default/all-feature, formatting, strict Clippy, release, smoke, snapshot,
  diff, and oracle/reference gates pass.

---

# PRV-146 — First-Party OpenAI, MiMo, and DeepSeek Support

Status: COMPLETE (2026-08-19)

Priority: P0

Type: Provider protocols, capabilities, pricing, and verification

Plan: `docs/exec-plans/completed/PRV-146-first-party-provider-contracts.md`

Result:

- Exact official OpenAI/Codex, MiMo, and DeepSeek routes expose built-in model
  capabilities and dialect-specific reasoning, caching, continuation, and usage
  behavior; user overrides win and custom compatible endpoints remain generic.
- Catalog-v2 pricing is endpoint/billing scoped for Terra, MiMo PAYG/Token Plan,
  and DeepSeek without assigning API prices to Codex subscription routes.
- The explicit sanitized provider-contract verifier is capped at two no-retry
  requests per route. Focused and complete default/all-feature, lint, release,
  render, snapshot, diff, and oracle/reference gates pass without live traffic.

---

# TUI-145 — Simultaneous Trackpad Scroll and Text Selection

Status: COMPLETE (2026-08-19)

Priority: P0

Type: Terminal input, selection, clipboard, and presentation

Plan: `docs/exec-plans/completed/TUI-145-simultaneous-scroll-selection.md`

Result:

- Default Scroll mode retains wheel/trackpad navigation and supports visible
  left-drag transcript selection without a terminal modifier.
- Release copies the exact bounded visible selection through the in-process
  clipboard; failure is visible and non-destructive.
- Mouse Off and lifecycle cleanup remain compatible. Focused event/transcript/
  input/render suites and complete default/all-feature, formatting, strict
  Clippy, release, snapshot, diff, and oracle/reference gates pass.

---

# AGT-144 — Automatic Safe Agent Launches

Status: COMPLETE (2026-08-19)

Priority: P0

Type: Approval, agent routing/authentication, and TUI presentation

Plan: `docs/exec-plans/completed/AGT-144-automatic-safe-agent-launches.md`

Result:

- Read-only, child-network-free asynchronous spawns auto-run in AutoReview;
  unsafe grants and strict/persistent policy retain their prompts or denials.
- Omitted complexity inherits the active route, explicit complexity remains
  fail-closed, and transient Codex child clients resolve runtime OAuth before
  provider traffic or usage recording.
- Contiguous identical spawn failures render as one expandable block while all
  underlying calls and provider protocol results remain intact. Focused and
  complete default/all-feature, lint, release, snapshot, and hygiene gates pass.

---

# REL-143 — Low-Friction Reads and Credential Migration

Status: COMPLETE (2026-08-18)

Priority: P0

Type: Approval ergonomics, credential migration, and live delivery

Plan: `docs/exec-plans/completed/REL-143-low-friction-reads-credentials.md`

Result:

- Exact legacy `allowlist` values migrate atomically to comment-preserving
  `auto-review`; strict `ask`, bare process-only behavior, and all mutation,
  network, explicit-policy, unsafe-command, and workspace boundaries remain.
- macOS provider credentials use hardened profile-private files after a
  one-time legacy Keychain import; later loads avoid Keychain while legacy
  items remain for rollback.
- Focused and complete gates pass. The validated release is installed, live
  configuration/credential migration and offline doctor pass, the second
  secret-resolving load has no prompt, and timestamped rollback files exist.

---

# SES-142 — Exit Session Handoff

Status: COMPLETE (2026-08-18)

Priority: P0

Type: Session continuation usability

Plan: `docs/exec-plans/completed/SES-142-exit-session-handoff.md`

Result:

- A normal resumable interactive exit now prints the bounded session name and
  exact copyable `ca -s <ses-id>` continuation command after terminal restore.
- Empty, bare, and persistence-disabled sessions emit no unusable hint;
  headless/service contracts and exit codes remain unchanged.
- Focused characterization/session regressions, default and all-feature suites,
  formatting, strict Clippy, release, and hygiene checks pass.

---

# REL-141 — Practical Read-Only AutoReview

Status: COMPLETE (2026-08-18)

Priority: P0

Type: Approval ergonomics and local command classification

Plan: `docs/exec-plans/completed/REL-141-practical-read-only-autoreview.md`

Result:

- Ordinary read-only `find ... | sort` discovery, including the exact reported
  pipeline, now auto-runs under AutoReview.
- `find` execution/deletion/file-output modes and `sort` output/helper modes
  remain gated, as do unknown programs and explicit Ask/Deny policy matches.
- Focused characterization, interactive/headless routing, default and
  all-feature suites, formatting, strict Clippy, release, and hygiene checks
  pass.

---

# SES-140 — Automatic Session Names and Public Identifiers

Status: COMPLETE (2026-08-18)

Priority: P0

Type: Session identity and continuation usability

Plan: `docs/exec-plans/completed/SES-140-auto-name-public-session-id.md`

Result:

- Allocate the existing UUID-backed session identity before the first provider
  turn and derive a bounded Unicode-safe title locally from the first user
  request unless `--name` or `/name` supplied one.
- Expose the same UUID as a fixed-width reversible `ses_…` identifier without
  changing the V4 schema, storage key, or internal UUID authority boundaries.
- Accept `ca -s <ses-id>` while retaining UUID/legacy-key resume, show the full
  identifier in session search/pickers, and print a copyable continuation
  command in `/session`.
- Focused identity/title/store/CLI tests, all 155 render tests, default and
  all-feature suites, formatting, strict Clippy, release, help/version,
  diff/snapshot hygiene, and oracle/reference immutability checks pass.

---

# TUI-139 — White Composer Cursor Blink

Status: COMPLETE (2026-08-18)

Priority: P0

Type: Terminal input rendering

Result:

- Replace terminal-dependent composer cursor color/blinking with a white
  software block on the exact existing Unicode/display-cell insertion point.
- Alternate every 500 ms between the white block and the untouched underlying
  cell, reset to the visible phase after keyboard input or paste, and suppress
  the composer cursor while another surface has focus.
- Focused cursor/cadence tests, all 155 render tests, default and all-feature
  suites, formatting, strict Clippy, release, diff/snapshot hygiene, and
  oracle/reference immutability checks pass.

---

# TUI-138 — Composer-First Usability Overhaul

Status: COMPLETE (2026-08-17)

Priority: P0

Type: Terminal input, completion, scrolling, and lifecycle usability

Plan: `docs/exec-plans/completed/TUI-138-composer-first-usability.md`

Result:

- Add a seven-row Unicode/grapheme-aware multiline composer with exact
  cursor placement, visual-row/Vim motion, history draft restoration, atomic
  attachment/path/paste elements, and range-bound large-paste expansion.
- Replace slash, attachment, and path command windows with token-scoped,
  composer-anchored completion; share responsive navigation/row layout with
  the general model, session, provider, and permission pickers.
- Add compact adaptive chrome, explicit after-turn queue previews/editing,
  bottom-relative wrapped transcript scrolling, and configurable best-effort
  trackpad capture with complete terminal lifecycle cleanup.
- Focused/default/all-feature tests, reviewed wide/narrow snapshots,
  formatting, strict Clippy, release, help/version, diff/snapshot hygiene, and
  oracle/reference immutability checks pass. The disposable native PTY check
  was blocked by the repository rule forbidding an isolated `HOME` override;
  no real runtime state was changed.

---

# TUI-137 — Native Composer Cursor

Status: COMPLETE (2026-08-17); cursor presentation superseded by TUI-139

Priority: P0

Type: Terminal input rendering

Plan: `docs/exec-plans/completed/TUI-137-native-composer-cursor.md`

Result:

- Replace the permanent narrow software `▌` with the terminal's native cursor
  at the exact composer insertion cell.
- Let the user's terminal own cursor shape and blinking while preserving
  Unicode-safe placement and deterministic render coverage.
- Hide the native composer cursor while a modal has focus. Focused render,
  complete default/all-feature tests, formatting, strict Clippy, release,
  help/version, diff, and snapshot hygiene checks pass. No runtime state,
  installed binary, oracle/reference file, or Git history changed.

---

# TUI-136 — Bracketed Paste for Folder Drops

Status: COMPLETE (2026-08-17)

Priority: P0

Type: Terminal input reliability

Plan: `docs/exec-plans/completed/TUI-136-bracketed-paste-folder-drop.md`

Result:

- Treat a Finder-dropped path as one paste so an absolute folder path enters
  the composer instead of opening the slash-command palette.
- Preserve ordinary typed `/` commands and release bracketed-paste mode on
  every terminal suspend, recovery, panic, and exit path.
- Focused, default, all-feature, formatting, strict Clippy, release, help,
  version, diff, and snapshot checks pass. No runtime state, installed binary,
  oracle/reference evidence, or Git history changed.

---

# REL-135 — Sandboxed SSH and Read-Only Approval Friction

Status: COMPLETE (2026-08-17)

Priority: P0

Type: Reliability, sandbox compatibility, and approval ergonomics

Plan: `docs/exec-plans/completed/REL-135-ssh-and-read-only-approval.md`

Result:

- Restore macOS UID lookup needed by ordinary sandboxed SSH without exposing
  user SSH files outside existing workspace roots or weakening the closed
  experiment profile.
- Infer one-use network approval for obvious SSH-family commands even when the
  model omits `network = true`.
- Default new profiles to AutoReview and auto-run only a closed, regression-
  tested subset of harmless local read commands; every write-capable, unknown,
  network, or unsafe-shell command remains approval-gated.
- Focused, default, all-feature, formatting, strict Clippy, release, and native
  sandboxed-SSH checks pass. No runtime profile, SSH material, remote state,
  installed binary, oracle/reference evidence, or Git history changed.

---

# OBS-117 — Persistent Provider Exchange Tracing

Status: COMPLETE (2026-08-16)

Priority: P0

Type: Reliability and provider observability

Plan: `docs/exec-plans/completed/OBS-117-persistent-provider-exchange-tracing.md`

Result:

- Add explicit default-off `/trace` and `--trace` controls for persistent,
  versioned, owner-only, profile-isolated provider exchange traces.
- Capture every production model route at the common transport boundary,
  including retries, bounded raw frames, decoded events, and terminal outcome,
  without changing or failing provider work.
- Redact credentials, account identifiers, configured secrets, encrypted
  reasoning, binary/media bodies, query values, and terminal controls.
- Persist until exact cancel-first deletion; cap each trace at 16 MiB and add no
  automatic pruning or saved setting.

Oracle material was read-only design evidence only. This is not a Cursor parity
claim. Focused, local completion, synthetic loopback walkthrough, and exact
physical-FreeBSD gates pass. No binary was installed and no implementation
commit was created; later roadmap tasks remain in the same ordered uncommitted
delivery tree.

## Ordered reliability roadmap — unselected candidates

### INI-118 — Safe `/init`

Status: COMPLETE (2026-08-16)

Plan: `docs/exec-plans/completed/INI-118-safe-init.md`

Analyze bounded primary-workspace evidence, make one tool-free model request,
preview at most 32 KiB of `AGENTS.md`, and write only after confirmation. Never
overwrite or follow a symlink; integrate a successful write with `/undo`.

### LSP-119 — Post-Edit Diagnostics

Status: COMPLETE (2026-08-16)

Plan: `docs/exec-plans/completed/LSP-119-post-edit-diagnostics.md`

After successful text mutations, synchronize configured language servers and
append bounded matching-version diagnostics. Unavailable, stale, failed, or
timed-out LSP work must not fail a successful edit; never install servers.

### SEC-120 — Agent-Harness Audit

Status: COMPLETE (2026-08-16)

Plan: `docs/exec-plans/completed/SEC-120-agent-harness-audit.md`

Add a deterministic read-only local audit of instructions, skills, hooks, MCP,
permissions, credential references, and executable paths. Report severity,
evidence, and remediation without a model, network, execution, or mutation.

### TUI-121 — Native Raw Transcript

Status: COMPLETE (2026-08-16)

Plan: `docs/exec-plans/completed/TUI-121-native-raw-transcript.md`

Add argument-free `/raw`: suspend the alternate screen, print a sanitized
copy-friendly transcript to native scrollback, wait for Enter, and restore the
TUI without exposing hidden/private/unavailable content.

### USG-122 — Provider-Account Usage

Status: COMPLETE (2026-08-16)

Plan: `docs/exec-plans/completed/USG-122-provider-account-usage.md`

Add a typed quota-adapter boundary and a separate `/usage` provider-account
section. Query official-host OpenAI month-to-date usage/cost and DeepSeek live
balance explicitly, label MiMo plan allocation as a local estimate, and keep
all remote scopes distinct from local ledger totals.

### CTX-123 — Retained-Boundary Context Controls

Status: COMPLETE (2026-08-16)

Plan: `docs/exec-plans/completed/CTX-123-retained-boundary-context-controls.md`

Support `/compact here [N]` with a two-exchange default and 32-exchange cap,
preserving the retained exchanges verbatim. Add a deterministic 5×20 category
grid to `/context` while retaining exact totals and checkpoint coverage.

### CFG-124 — One-Turn Models and Route Presets

Status: COMPLETE (2026-08-16)

Plan: `docs/exec-plans/completed/CFG-124-one-turn-models-and-route-presets.md`

Add `/model <id> --once` with unconditional route/effort restoration and named
provider/model/effort presets through `/preset` and `/preset-save`; keep full
named profiles separate.

### OUT-125 — Semantic Command-Output Compression

Status: COMPLETE

Plan: `docs/exec-plans/completed/OUT-125-semantic-command-output-compression.md`

Add an opt-in `bounded|semantic` policy with internal Git status/diff and Cargo
test/Clippy parsers, labeled transformations, retained bounded raw expansion,
and a full-output rerun path. Do not invoke or integrate `rtk`.

### PRJ-126 — Persistent Project Scratch Space

Status: COMPLETE

Plan: `docs/exec-plans/completed/PRJ-126-persistent-project-scratch-space.md`

Add profile-isolated, canonical-workspace-keyed notes, todos, and saved-plan
references outside session serialization. Never commit them automatically or
add arbitrary saved shell-command execution; reuse declarative workflows.

### TUI-127 — Timed Thought and Inline Agent Activity

Status: COMPLETE

Plan: `docs/exec-plans/completed/TUI-127-timed-thought-inline-agent-activity.md`

Add measured thought headers, expandable live conversation-agent rows, and a
read-only `/context` overview for current-sitting usage, workspace, LSP, and
task status. Do not add a drawer or persist raw child activity.

### REL-128 — Low-Yield Loop Guard

Status: COMPLETE (2026-08-16)

Plan: `docs/exec-plans/completed/REL-128-low-yield-loop-guard.md`

Detect repeated near-identical edits, unchanged failing commands, and
no-progress edit/verification streaks from bounded typed tool outcomes. Pause
only between a completed tool batch and provider continuation, then require an
explicit Replan, Continue, or Cancel decision. Never launch another coding
agent automatically.

### TUI-129 — Asynchronous `@` Attachments

Status: COMPLETE (2026-08-16)

Plan: `docs/exec-plans/completed/TUI-129-asynchronous-attachments.md`

Replace the synchronous workspace walk with cancellable bounded indexing and
fuzzy/frecency ranking, add send-time validated line ranges, and show already-
discovered MCP resources without activating servers.

### TUI-130 — Optional Turn Metadata

Status: COMPLETE (2026-08-16)

Plan: `docs/exec-plans/completed/TUI-130-optional-turn-metadata.md`

Add default-off transcript timestamps and one final-assistant footer per turn,
plus optional generation-speed and sitting-cost statusline fields. Preserve
honest unknown, subscription-allocation, mixed, and interrupted states.

### INP-131 — Explicit Prompt Stash

Status: COMPLETE (2026-08-16)

Plan: `docs/exec-plans/completed/INP-131-explicit-prompt-stash.md`

Add explicit profile-scoped bounded draft save/restore/swap actions without
turning normal composer history into persistent history. Bare mode remains
process-only and restored attachments stay inert until send-time revalidation.

### CFG-132 — Recent and Favorite Models

Status: COMPLETE (2026-08-16)

Plan: `docs/exec-plans/completed/CFG-132-recent-favorite-models.md`

Add bounded profile-scoped recent/favorite model identity, unique picker
sections, an explicit favorite toggle, and configurable recent/favorite cycle
actions while retaining cycle-all. Bare mode keeps recency process-only and
does not persist favorites.

### SES-133 — Foreign Transcript Import

Status: COMPLETE (2026-08-16)

Plan: `docs/exec-plans/completed/SES-133-foreign-transcript-import.md`

Extend explicit `/import` with bounded inert Codex, Claude, and OpenCode
transcript adapters. Require a user-selected source and preview; discard all
foreign execution authority and exclude imports from learning by default.

### LRN-134 — Autonomous Dream and Distill

Status: COMPLETE (2026-08-17)

Plan: `docs/exec-plans/completed/LRN-134-autonomous-dream-distill.md`

Add bounded priced-route-only idle Dream memory maintenance and Distill
one-file skill generation with strict corpus filtering, provenance/CAS
protection, crash-safe journals, exact undo, explicit controls, and immediate
user-input cancellation. The seven-task MiMo-informed roadmap is complete.

Deferred and unscheduled: proprietary Cursor transport, shell integration based
only on the patched bundle, shadow-Git rollback, executable model-written
workflows beyond bounded `LRN-134`, GUI/mobile surfaces, messaging relays,
desktop control, automatic credential rotation, MCP sampling, and external
Codex/Claude subprocess handoff.

---

# GIT-111 — Approval-Safe `/commit` Workflow

Status: COMPLETE (2026-08-15)

Priority: P0

Type: Focused local Git workflow

Plan: `docs/exec-plans/completed/git-111-approval-safe-commit.md`

Result:

- `/commit` is visible, idle-only, and argument-free. It reuses the working-tree
  review surface for exact whole-file stage/unstage, tracked diffs, refresh,
  Unicode-safe manual subject entry, and a scrollable Cancel-first final
  confirmation.
- The Git service preserves raw paths separately from sanitized labels, refuses
  detached/conflicted/special-operation repositories, supports named unborn
  branches, and binds/revalidates branch, parent, index tree, and complete
  staged manifest before one ordinary hook-respecting `git commit -m`.
- Timeout and cancellation own the Git process group, output is bounded and
  sanitized, and unexpected post-command HEAD movement reports an uncertain
  outcome without retry, reset, push, or automatic recovery.
- Focused and complete local tests, 14 reviewed snapshots, the disposable
  no-network TUI walkthrough, and the exact-tree physical-FreeBSD completion
  gates pass. No binary was installed; the implementation is committed on
  `master` as `dd1dc647e3c52d073099c0c3cf51c3ca73203dfa`.

## CTX-112–MD-116 — Selected Oracle-Informed Enhancements

Status: COMPLETE (2026-08-16)

Priority: P1

Plan: `docs/exec-plans/completed/CTX-112-MD-116-selected-enhancements.md`

Result:

- `CTX-112` adds validated optional focus to manual compaction without changing
  automatic compaction or the required checkpoint structure.
- `TUI-115` adds default-off BEL/OSC 9 completion notifications, a searchable
  comment-preserving picker, and once-only post-redraw turn completion.
- `SES-113` adds a read-only `/session` pager with explicitly separated
  conversation and process-sitting scopes plus shared usage aggregation.
- `MD-116` adds conservative display-only terminal math normalization and a
  lossless bounded Unicode converter.
- `TUI-114` adds an explicit 18-tool exploration classification, collapsed
  cross-message groups, and stable first-call expansion keys.
- Focused, local completion, loopback-only manual TUI, and exact-source
  physical-FreeBSD gates pass. The native snapshot discovery was fixed with a
  platform-neutral render fixture; no production estimate was flattened.
- No user config/session, credential, installed binary, oracle, reference,
  provider history, session schema, or Cursor parity record changed.

These are intentional `ca` extensions informed by read-only oracle material;
they are not Cursor parity requirements. No subsequent implementation task is
selected.

---

# BSD-110 — FreeBSD Recovery Regression Repair

Status: COMPLETE (2026-08-15)

Priority: P0

Type: Focused cross-platform stabilization

Plan: `docs/exec-plans/completed/bsd-110-freebsd-recovery-regression.md`

Result:

- Recovery V2 records directories, empty directories, and directory modes;
  reconstruction restores directory modes deepest-first after file writes.
  Existing V1 regular-file bundles remain readable.
- FreeBSD `EMLINK` from `O_NOFOLLOW` is classified as the same safe symlink
  refusal as `ELOOP`. Unsupported macOS-only sandbox tests now assert the
  existing fail-closed boundary without changing production authority.
- The exact repair tree passes local and physical-FreeBSD default/all-feature
  suites, formatting, strict all-feature Clippy, release builds, and binary
  smokes. Native results include all 7 formerly mismatching recovery tests and
  the formerly mismatching symlink diagnostic.
- No provider request, runtime configuration, credential, installed binary,
  oracle, or reference content changed. Disposable validation trees were
  removed.

---

# Baseline Integration

The exact pre-review working tree at baseline
`ce787efc4dce99d9ddb48a831bae5e45629657bc` is preserved outside the checkout
at `/Users/aji/project/ca-working-tree-preservation-20260815T225026Z/`. Its
tracked binary patch and untracked archive reconstruct all changed files
byte-for-byte.

`CFG-109` is complete. On 2026-08-15 the repository owner designated the
complete current tree as the new local baseline on `master`, recorded as
`06015abc90e567de4a440f03516ab5de3487767d`. BSD-110 is also complete against
the exact repair tree derived from that baseline. GIT-111 is complete against
the later baseline `b426f5ca7a951f61ba1d78c20049fd0f80719e8f` and committed as
`dd1dc647e3c52d073099c0c3cf51c3ca73203dfa`. CTX-112–MD-116 is complete
against that commit and was committed as
`ae107301999744d490f1c134e9925f4d4c125011`. OBS-117, INI-118, and LSP-119 are
complete against that baseline and remain uncommitted as requested. The
preservation archive remains recovery evidence outside the repository. SEC-120
is the sole active implementation task.

# CFG-109 — Restore Agent Defaults for Partial TOML

Status: COMPLETE (2026-08-15)

Priority: P0

Type: Focused local compatibility task

Owner/session: Codex / current stabilization session

Evidence:

- `CaConfig.agent` uses `#[serde(default)]`, while `AgentConfig` derives Rust
  `Default`; omitting the entire `[agent]` table therefore creates zero/empty
  fields instead of the field-level serde defaults.
- `max_turns` becomes zero rather than 100. Production-generated configs
  include `[agent]`, but compatible handwritten and older partial configs do
  not necessarily do so.
- The defect is independent of provider choice and must not be folded into a
  provider, authentication, or routing change.

Allowed files:

- `src/config.rs`
- focused config tests in the existing config test module
- `CONTINUITY.md`, `TESTING.md`, `NEXT_STEPS.md`

Forbidden:

- `oracles/`, `reference/`, runtime config/session/credential files
- provider defaults, credential resolution, or config-path changes
- broad formatting, refactors, migrations, or automatic config rewrites

Steps:

1. Add a failing regression for TOML with no `[agent]` table.
2. Characterize a partial `[agent]` table and explicit saved values.
3. Make Rust `AgentConfig::default()` equal the semantic serde defaults.
4. Prove explicit values and serialized production config remain unchanged.
5. Run focused config tests and the standard Rust completion gates.

Acceptance:

- An omitted `[agent]` table yields `max_turns = 100`, `approval_mode =
  "allowlist"`, `command_timeout_secs = 30`, `sandbox_mode = "required"`, and
  the existing finite subagent defaults.
- A present partial `[agent]` table uses the same defaults for omitted fields.
- Explicit configured values remain byte/behavior compatible.
- Loading does not rewrite the config or touch runtime state.
- Focused tests, `cargo test`, `cargo fmt --check`, strict all-target Clippy,
  and `cargo build --release` pass.

Result:

- Replaced the derived zero-valued Rust default with one semantic
  `AgentConfig::default()` shared by omitted-table serde loading and production
  config construction.
- Added regressions for an omitted `[agent]` table without file rewrite, a
  partial table with explicit-value preservation, serialized production
  defaults, and the provider-management snapshot's truthful `required`
  sandbox mode.
- The initial omitted-table test failed with `max_turns` 0 versus 100. After
  the repair, all 67 config-store tests pass.
- Completion gates pass: 1,308 default main tests plus 36 CLI tests; 1,337
  all-feature main tests plus 115 experiment tests plus 36 CLI tests; format,
  strict all-feature Clippy, release build, diff, and snapshot hygiene.
- No provider default, config path, runtime file, credential, oracle, or
  reference content changed.

---

# AGT-108 — DeepSeek Delegation Target

Status: COMPLETE (2026-08-15)

Priority: P0

Plan: `docs/exec-plans/completed/agt-108-deepseek-delegation-target.md`

Result:

- Add official DeepSeek V4 Flash/Pro price cards, exact cache-aware accounting,
  strict official-endpoint eligibility, and automatic simple/standard child
  routing without mutating the saved parent or making paid calls.
- Passed focused and complete all-feature validation, strict Clippy, and the
  arm64 release build; atomically installed SHA-256 `3c57a1ea4a…` with the
  prior `ede243f9de…` binary retained for recovery.
- Restored `openai-codex/gpt-5.6-sol` as the persisted parent. Authorized live
  run `745e6c33-25d9-4164-afe5-ce9f88707218` made one simple task call, routed
  to `deepseek/deepseek-v4-flash`, returned `AGT108_DEEPSEEK_OK`, and used no
  fallback or retry.

---

# AGT-107 — Browser-Authorized MiMo Token Plan Repair

Status: COMPLETE (2026-08-15)

Priority: P0

Plan: `docs/exec-plans/completed/agt-107-browser-authorized-mimo-token-plan-repair.md`

Result:

- Browser and manual MiMo setup now classify the credential together with the
  exact endpoint, fail closed on mismatches, and keep new browser Token Plan
  secrets process-only until confirmed metadata commits transactionally.
- Repaired the existing saved Singapore credential as Pro annual at $528 using
  one successful isolated probe and a billing-only atomic update. OpenAI Codex
  remained the active `gpt-5.6-sol` parent; doctor reports MiMo eligible.
- Passed formatting, strict all-feature Clippy, the complete all-feature suite,
  and the release build. Installed SHA-256 `ede243f9…`; the prior binary remains
  recoverable. One authorized live simple task routed exactly once to
  `mimo/mimo-v2.5-pro` and returned `AGT107_MIMO_OK` with `Subagent` accounting.

---

# AGT-106 — macOS Inter-Provider Delegation

Status: COMPLETE (2026-08-15)

Priority: P0

Plan: `docs/exec-plans/completed/agt-106-macos-inter-provider-delegation.md`

Result:

- Repaired the Firefox-sensitive macOS MiMo callback, added masked regional
  Token Plan setup with transactional credential/billing storage, and added
  exact official pricing status/refresh/accounting with 30-day fail-closed
  freshness.
- Added explicit complexity routing across eligible Sol, Luna, and MiMo route
  clients while preserving omitted-hint compatibility and `Subagent`
  accounting; headless children remain a closed read-only subset.
- Passed strict formatting/Clippy, 1,321 main + 115 experiment + 36 CLI tests,
  both release builds, and offline installed-binary smokes. Atomically installed
  SHA-256 `e3d79cef…` with the prior `32a4f164…` binary retained as a backup.
  Live paid delegation remains intentionally deferred.

---

# AUTH-105 — MiMo Authorization Handoff Repair

Status: COMPLETE (2026-08-15)

Priority: P0

Plan: `docs/exec-plans/completed/auth-105-mimo-authorization-handoff.md`

Result:

- Matched MiMoCode's direct Xiaomi `/authorize` contract and OSC 52
  tmux/screen passthrough.
- Replaced risky bordered long-URL selection with an automatic borderless
  FreeBSD handoff; Ctrl+Y repeats the exact handoff on every platform.
- Passed complete macOS and physical-FreeBSD gates, verified the native PTY
  behavior, and installed both platform-native release binaries.

---

# AUTH-104 — Automated Provider Authentication

Status: COMPLETE (2026-08-14)

Priority: P0

Plan: `docs/exec-plans/completed/auth-104-automated-provider-authentication.md`

Result:

- Made `/provider` a four-route connection picker with supported MiMo and
  OpenAI Codex browser/headless flows plus masked DeepSeek/OpenAI bearer keys.
- Added canonical cross-platform credential resolution, hardened FreeBSD
  profile storage, bare process-only behavior, isolated probes with rollback,
  starter-model selection, and matching doctor/normal Codex authentication.
- Passed full macOS and physical-FreeBSD tests, strict all-target Clippy, and
  release builds. Existing Codex accounts on both systems passed doctor-route
  and ordinary marker requests; absent MiMo, DeepSeek, and direct OpenAI
  accounts remain unclaimed.

---

# BSD-103 — FreeBSD Regression Audit

Status: COMPLETE (2026-08-14)

Priority: P0

Plan: `docs/exec-plans/completed/bsd-103-freebsd-regression-audit.md`

Result:

- Revalidated the exact current tree on physical FreeBSD after the auth,
  scheduling, skills, and email changes.
- Added manifest-safe x86-64 target identity, packaged FreeBSD Git discovery,
  portable Python fixtures, and explicit non-macOS policy-only test setup.
- Passed 1,230 applicable native unit tests, all 36 CLI tests, formatting,
  strict Clippy, release build, and released help/version/ELF smokes without
  weakening the macOS-only required sandbox boundary.

---

# BSD-102 — FreeBSD Native Support

Status: COMPLETE (2026-08-14)

Priority: P0

Result:

- Added FreeBSD `O_PATH` symlink-descriptor handling to the existing no-follow workspace materialization and cleanup boundary.
- Made system prompts report the actual operating system and removed non-macOS warning failures from platform-specific Keychain and voice code.
- Verified the native FreeBSD 15 amd64 build and focused workspace/system-prompt tests on the `bsd` host; macOS-only sandbox, Keychain, voice-capture, and LaunchAgent boundaries remain explicit rather than being silently weakened.

---

# RND-097 — Capped OpenAI Planner/Executor Pilot

Status: COMPLETE (2026-08-12)

Priority: P0

Plan: `docs/exec-plans/completed/rnd-097-openai-planner-executor-pilot.md`

Execution handoff: `docs/CAPABILITY_EXPANSION_HANDOFF.md`

Result:

- Added the complete feature-gated 72-slot controller, strict one-call broker/tool IPC, exact $50 reservation ledger, WSP-isolated fixtures and recovery, safe resume, calibration, SC-G recovery, SP rendering, and underpowered paired report.
- Final gates passed 1,166 default unit tests plus 36 CLI tests, 112 feature experiment tests plus 47 reasoning integration tests, formatting, default/all-feature strict Clippy, both release builds, released 12-fixture verification, help/version, and snapshot hygiene.
- A fresh authorized pilot stopped at canonical OpenAI route preflight because no configured Sol/Luna route exists. It made zero provider requests and recorded zero settled, outstanding, or unknown spend; the economic hypothesis remains unmeasured rather than misreported.

---

# SKL-098 — Agent Skills Compatibility

Status: COMPLETE (2026-08-13; plan archived after validation)

Priority: P0

Plan: `docs/exec-plans/completed/skl-098-agent-skills-compatibility.md`

Contract: `docs/CAPABILITY_EXPANSION_HANDOFF.md#skl-098--agent-skills-compatibility`

Goal:

- Add portable Agent Skills discovery, distinct digest-pinned installations, progressive task-local loading, safe resources, signed disabled installs/imports, and compatible CLI/TUI management without widening authority.

---

# SCH-099 — Durable Automation and LaunchAgent Support

Status: COMPLETE (2026-08-13; plan archived after validation)

Priority: P0

Contract: `docs/CAPABILITY_EXPANSION_HANDOFF.md#sch-099--durable-automation-and-launchagent-support`

Plan: `docs/exec-plans/completed/sch-099-durable-automation-launchagent.md`

Goal:

- Add V1-compatible durable ScheduleDefinitionV2 triggers/runs, cross-process leases, WSP-isolated mutations, exact script-only jobs, crash recovery, and explicit secret-free LaunchAgent lifecycle.

Result:

- Preserved frozen V1 bytes while adding separate exact-confirmed V2 definitions, deterministic time/DST handling, immutable run journals, checked claim CAS, bounded leases, and conservative no-replay dead-owner recovery.
- Added exact pinned Git/WSP execution, SKL-098 pins, direct clean-environment script jobs, independently reconstructed recovery archives, offline history/doctor, manager aliases/drain/daemon, and deterministic exact-confirmation LaunchAgent lifecycle.
- Full gates passed with disposable/fake-only validation; no provider/webhook traffic or real LaunchAgent/user-runtime mutation occurred.

---

# EML-100 — Typed Himalaya Mailbox Tools

Status: COMPLETE (2026-08-13; plan archived after validation)

Priority: P0

Contract: `docs/CAPABILITY_EXPANSION_HANDOFF.md#eml-100--typed-himalaya-mailbox-tools`

Plan: `docs/exec-plans/completed/eml-100-typed-himalaya-mailbox-tools.md`

Goal:

- Add opt-in typed mailbox read/draft/send/reply operations through exact-argv Himalaya, immutable drafts, private-content controls, and one-use exact-recipient external-communication approval.

Result:

- Added exactly six disabled-by-default Himalaya v2.0.x tools with bounded account/folder/message identity, immutable owner-only drafts and attachments, Bcc-safe SMTP envelopes, and safe delivery receipts.
- Interactive and service sends require exact single-use digest-bound approval; Yolo, AutoReview, generic rules, network permission, and broad/session grants cannot send. Headless sending stops with approval required.
- Full gates passed with fake executables and private-data sentinels only; no live mailbox, credential, provider request, user runtime, or installed binary was accessed or changed.

---

# EML-101 — Dedicated Draft-First Email Gateway

Status: COMPLETE (2026-08-13; plan archived after validation)

Priority: P0

Contract: `docs/CAPABILITY_EXPANSION_HANDOFF.md#eml-101--dedicated-draft-first-email-gateway`

Plan: `docs/exec-plans/completed/eml-101-dedicated-draft-first-email-gateway.md`

Goal:

- Add a dedicated allowlisted IMAP gateway with UID idempotency, isolated no-tool requests, attachment omission, and human-review Drafts only; it never sends or auto-replies.

Result:

- Added a dedicated empty-allowlist account with exact profile/account Keychain scope, verified TLS transport, bounded MIME/attachment omission, UIDVALIDITY bootstrap and hash-chained idempotency state.
- Each accepted message receives one fresh zero-tool request; host code appends a deterministic threaded plain-text `\Draft` only. Ambiguous APPEND recovery searches exact Message-ID and permits at most one later byte-identical retry.
- Added offline/connect doctor, once/daemon polling, exact-confirmation LaunchAgent lifecycle, and canonical confirmed schedule EmailDraft delivery. Full gates passed without a live mailbox, credential, external provider, SMTP send, or real LaunchAgent mutation.

---

# WSP-096 — Copy-on-Write Workspace Materialization

Status: COMPLETE (2026-08-12)

Priority: P0

Plan: `docs/exec-plans/completed/wsp-096-cow-workspace-materialization.md`

Result:

- Added a Unix descriptor-relative, manifest-verified disposable workspace primitive with actual APFS per-file cloning and bounded create-new copy fallback.
- Exact benchmark/scheduled policies enforce entry/depth/byte/space bounds, topology rules, and scheduled tracked-path allowlists; source drift and path/inode substitution fail before publication or deletion.
- Durable owner-only receipts, crash reopen, workspace-bound external recovery proof, and retryable partial cleanup passed 22 focused tests, 1152 unit tests plus 36 CLI tests, strict formatting/Clippy, release build/help/version, and snapshot hygiene.

---

# RND-095 — Strong-Planner / Cheap-Executor Cost Experiment

Status: COMPLETE (2026-08-12)

Priority: P0

Plan: `docs/exec-plans/completed/rnd-095-planner-executor-cost-experiment.md`

Result:

- Current provider evidence supports exact route continuation and a Google-specific compatible Gemini model-switch experiment, but not arbitrary cross-provider or OpenAI tier-to-tier opaque reasoning transfer. The recommended handoff is a visible, validated `reasoning_artifact.v1` plus pre-execution routing and deterministic verification.
- A non-default `ca-reasoning-artifact-experiment` binary now emits the schema, validates bounded caller-supplied artifacts and selected repository dependencies, and aggregates date-stamped declared-rate cost fixtures exactly. It performs no provider traffic, tool execution, runtime-state mutation, or normal `ca` integration.
- Validation passed 16 focused tests, 1130 unit tests plus 36 CLI tests, formatting, default and all-feature strict Clippy, default and experiment release builds, released schema/validate/cost smokes, unchanged default help/version, and zero pending snapshots. Live dollars-per-success remains an unproven benchmark question.

---

# PRV-094 — Reasoning-State Tool Continuation

Status: COMPLETE (2026-08-12)

Priority: P0

Plan: `docs/exec-plans/completed/prv-094-reasoning-state-continuation.md`

Result:

- Canonical OpenAI Responses now requests encrypted reasoning content, retains and validates the bounded completed output sequence, and replays it in order before tool output without `previous_response_id`; canonical DeepSeek and Xiaomi MiMo retain and replay exact `reasoning_content`.
- Opaque state is bound to the actual provider/protocol/normalized-endpoint digest, persists in direct sessions, and stays absent from portable exports, checkpoints, request snapshots, Debug, UI, and failed/cancelled turns. The catalog is now 14 ready / 26 requiring integration.
- Validation passed 1130 unit tests plus 36 CLI tests, focused two-request/route/persistence/privacy/service/subagent/cancellation suites, strict formatting and Clippy, release build/help/version, 114 render tests with reviewed wide/narrow snapshots, zero pending snapshots, and a released disposable three-provider loopback walkthrough.

---

# CFG-093 — Provider Template Catalog

Status: COMPLETE (2026-08-12)

Priority: P0

Plan: `docs/exec-plans/completed/cfg-093-provider-template-catalog.md`

Result:

- At CFG-093 completion, `/provider` and `/provider setup` exposed `Shift+P`, and `/provider templates [filter]` opened a local searchable catalog covering all 40 entries in the current Hermes provider guide. Its initial 11-ready, one-limited, 28-inert classification is historical and was superseded by PRV-094.
- The direct OpenAI and custom-provider paths remain intact. Opening/filtering/canceling is zero-I/O, duplicate IDs never overwrite, ordinary saves select the new model, setup saves preserve defaults, and all template saves retain an explicit typed protocol.
- Validation passed 1106 unit tests plus 36 CLI tests, formatting, strict Clippy, release build/help/version, 121 focused provider tests, 65 input tests, 114 render tests, zero pending snapshots, and a released disposable TUI walkthrough showing ready/inert rows and cancel-without-template-write.

---

# CFG-092 — OpenAI Provider Template

Status: COMPLETE (2026-08-12)

Priority: P0

Plan: `docs/exec-plans/completed/cfg-092-openai-provider-template.md`

Result:

- `/provider` and `/provider setup` now visibly expose `⇧O OpenAI template` beside the unchanged custom-provider action. The editable masked form prefills `openai`, `https://api.openai.com/v1`, `OPENAI_API_KEY`, `gpt-5.6-sol`, and the existing `openai_responses` adapter.
- Opening is process-only; explicit save atomically adds the provider without switching setup defaults, generic Add Provider remains protocol-free Chat Completions, duplicate `openai` refuses overwrite, and bare mode remains read-only.
- Validation passed 1091 unit tests plus 36 CLI tests, formatting, strict Clippy, release build/help/version, reviewed wide/narrow snapshots, isolated Responses adapter/probe tests, and a released disposable TUI save with no live provider traffic or user runtime mutation.

---

# AUD-091 — Full Program Completion Audit

Status: COMPLETE (2026-08-12)

Priority: P0

Plan: `docs/exec-plans/completed/aud-091-full-program-completion-audit.md`

Result:

- `docs/FULL_CAPABILITY_AUDIT.md` maps all 49 accepted roadmap IDs to current source, focused tests, and isolated walkthrough or accepted AUTH-060 stop-condition evidence; every row and archived plan resolves exactly once with no unchecked milestone.
- The audit corrected the exact inspectable public `ServeOp`, `ServeEvent`, `ScheduleDefinition`, `PluginManifest`, and `AgentDependencyEdge` contracts without changing released JSON or runtime authority, and added closed-default/public-boundary regressions.
- Final validation passed 1087 unit tests plus 36 CLI tests, formatting, strict Clippy, release build, zero-pending-snapshot inspection, and released default/named/bare plus every leading-manager smoke with no runtime-state creation.

---

# UPD-090 — Explicit Self-Update

Status: COMPLETE (2026-08-12)

Priority: P0

Plan: `docs/exec-plans/completed/upd-090-explicit-self-update.md`

Result:

- Leading `ca update` now pins one exact Ed25519 authority, explicitly verifies a closed current-target manifest, persists channel/pin choices, stages exact streamed release bytes, replaces only the invoking executable after exact confirmation, and retains one verified rollback copy.
- Trust and status perform no network; check never downloads binaries; download never executes; install and rollback reject symlinks, widened ownership/modes, drift, manifest/staging mismatches, and changed authorities. No automatic check, download, install, or cleanup exists.
- Validation passed 9 focused updater tests, parser/CLI coverage, 1085 unit tests plus 36 CLI tests, formatting, strict Clippy, release/help/version/update smokes, and a released copied-binary lifecycle with exactly two loopback requests, byte-for-byte rollback, no config/session creation, and an unchanged repository release binary.

---

# LOC-089 — Managed llama.cpp and GGUF

Status: COMPLETE (2026-08-12)

Priority: P0

Plan: `docs/exec-plans/completed/loc-089-managed-llama-cpp-and-gguf.md`

Result:

- Leading `ca local` now imports or exact-downloads validated GGUFs into private profile storage, inspects and revalidates exact bytes, and removes only confirmed manager-owned model trees while preserving source files.
- Explicit `--local-model` plus `--local-runtime` resolves the exact EXT-managed `llama-server`, uses a fixed sandboxed loopback launch, waits for valid health/model responses, injects a credentialless provider only in memory, disables session persistence, and kills the full process group on exit.
- Validation passed 9 focused local-model/supervisor tests, parser and released CLI lifecycle coverage, 1075 unit tests plus 35 CLI tests, formatting, strict Clippy, release/help/version/profile/local smokes, and a released signed-runtime/GGUF/credentialless request lifecycle with no config/session creation.

---

# EXT-088 — Signed External Runtime Manager

Status: COMPLETE (2026-08-12)

Priority: P0

Plan: `docs/exec-plans/completed/ext-088-signed-external-runtime-manager.md`

Result:

- Leading `ca runtime` now pins exact catalog sources and Ed25519 roots, verifies monotonic closed multi-file manifests, previews exact disk changes, streams confirmed installs/updates into private retained versions, revalidates rollback/entrypoints, and removes one exact manager-owned runtime.
- Validation passed 10 focused runtime tests, 1065 unit tests plus 34 CLI tests, formatting, strict Clippy, release/help/version/profile/runtime smokes, and a released two-version signed-loopback lifecycle with exactly four requests and no config/session creation.

---

# VOC-087 — Voice Input

Status: COMPLETE (2026-08-12)

Priority: P0

Plan: `docs/exec-plans/completed/voc-087-voice-input.md`

Result:

- `/voice` now records only after an explicit Space press, owns one bounded process-local mono WAV, transcribes through a configured sandboxed local or exact-provider backend, and inserts only an editable unsent draft.
- Validation passed 22 focused voice tests, 1054 unit tests plus 33 CLI tests, formatting, strict Clippy, release/help/version/profile smokes, and a released disposable Ready-modal walkthrough without microphone or network access.

---

# BRW-086 — Browser Automation

Status: COMPLETE (2026-08-12)

Priority: P0

Plan: `docs/exec-plans/completed/brw-086-browser-automation.md`

Result:

- The explicitly selected `browser` tool now drives an existing local Chrome/Chromium through one disposable literal-loopback CDP profile, with separate network authority, bounded DOM/screenshot/download results, owner isolation, cancellation-safe reblocking, and exact cleanup.
- Validation passed 1032 unit tests plus 33 CLI tests, formatting, strict Clippy, release/help/version/tool-selection smokes, fake-CDP lifecycle coverage, and an ignored-by-default real local Chromium walkthrough proving disposable-profile cleanup and no surviving browser process.

---

# PLG-085 — Signed Curated Catalog

Status: COMPLETE (2026-08-12)

Priority: P0

Plan: `docs/exec-plans/completed/plg-085-signed-curated-catalog.md`

Result:

- `ca plugin catalog` now pins local Ed25519 roots, verifies monotonic signed snapshots and exact package/file hashes, stages private disabled installs/updates, retains verified rollback versions, and performs no automatic network or activation.
- Validation passed 1019 unit tests plus 33 CLI tests, formatting, strict Clippy, release/help/version/catalog smokes, and a disposable signed-catalog lifecycle proving inert installation, exact activation, rollback, profile isolation, and zero sessions.

---

# PLG-084 — Local Plugin Bundles

Status: COMPLETE (2026-08-12)

Priority: P0

Plan: `docs/exec-plans/completed/plg-084-local-plugin-bundles.md`

Result:

- `ca plugin` now registers strict local declarative bundles disabled, derives safe capabilities, requires exact-confirm unchanged collision-free enablement, and composes only through existing skill/MCP/hook/workflow/theme boundaries without native code.
- Validation passed 1013 unit tests plus 32 CLI tests, formatting, strict Clippy, release/help/version/plugin smokes, and a released disposable-home walkthrough proving inert add, safe preview, drift refusal before traffic, exact activation, source-preserving removal, and zero sessions.

---

# SCH-083 — Scheduled Tasks

Status: COMPLETE (2026-08-12)

Priority: P0

Plan: `docs/exec-plans/completed/sch-083-scheduled-tasks.md`

Result:

- `ca schedule` now stores new one-shot/cron definitions disabled per profile, claims at most one due run under an owner-checked lock, skips late occurrences without catch-up, and executes exact prompt/workflow requests through an isolated no-session worker.
- Validation passed 1005 unit tests plus 31 CLI tests, formatting, strict Clippy, release/help/version/schedule smokes, and a released loopback CLI path proving default inactivity, exact request routing, bounded local delivery, and zero sessions.

---

# SRV-082 — Loopback WebSocket and Attach

Status: COMPLETE (2026-08-12)

Priority: P0

Plan: `docs/exec-plans/completed/srv-082-loopback-websocket-and-attach.md`

Result:

- `ca serve --listen` now exposes the unchanged `ca.serve.v1` lifecycle to one authenticated literal-loopback WebSocket client, while `ca attach` safely bridges JSONL using an owner-only file or named environment token source and stdio remains the default.
- Validation passed 997 unit tests plus 30 CLI tests, formatting, strict Clippy, release/help/version/profile/address smokes, and a released disposable walkthrough proving wrong-token refusal, exact approval/interaction/cancellation, environment reconnect, secret-free transport, and zero session persistence.

---

# SRV-081 — JSONL Stdio Service

Status: COMPLETE (2026-08-12)

Priority: P0

Plan: `docs/exec-plans/completed/srv-081-jsonl-stdio-service.md`

Result:

- `ca serve` and `ca serve --stdio` now expose one strict bounded `ca.serve.v1` JSONL process with process-only sessions, canonical prepared requests/runner events, exact client approvals/interactions, cancellation/status, and owner-scoped cleanup without saved-session writes.
- Validation passed 993 unit tests plus 29 CLI tests, formatting, strict Clippy, release/help/version/profile smokes, and a released loopback walkthrough proving ordered output, approval-before-mutation, structured interaction, cancellation-to-idle, empty stderr, and zero session persistence.

---

# TUR-078 — Soft Pause

Status: COMPLETE (2026-08-12)

Priority: P0

Plan: `docs/exec-plans/completed/tur-078-soft-pause.md`

Result:

- `/pause` and `/unpause` now retain exact main-turn state at completed provider, tool, and plain-finalization boundaries without interrupting work in flight or conflating Ctrl+C, goal/workflow pause, queues, or sessions.
- Validation passed 940 unit tests plus 28 CLI tests, formatting, strict Clippy, release/profile smokes, six reviewed snapshots, and a released disposable TUI walkthrough proving held side effects/finalization and terminal cancellation.

---

# TRM-079 — Durable Terminals

Status: COMPLETE (2026-08-12)

Priority: P0

Plan: `docs/exec-plans/completed/trm-079-durable-terminals.md`

Result:

- `/term` now safely lists, creates, attaches, detaches, and Cancel-first kills exact user-owned tmux sessions while restoring ratatui around attach and never cleaning tmux during normal shutdown.
- Validation passed 957 unit tests plus 28 CLI tests, formatting, strict Clippy, release/help/version smokes, 14 reviewed snapshots, and a released fake-tmux walkthrough proving an external survivor remained untouched.

---

# TUI-080 — Themes and Status Surfaces

Status: COMPLETE (2026-08-12)

Priority: P0

Plan: `docs/exec-plans/completed/tui-080-themes-and-status-surfaces.md`

Result:

- `/theme` and `/statusline` now persist closed profile-scoped display choices while missing config remains visually compatible; optional custom output is process-only, bounded, one-at-a-time, and requires an enforceable network-denied sandbox.
- Validation passed 977 unit tests plus 28 CLI tests, formatting, strict Clippy, release/profile smokes, 22 reviewed snapshots, and a released disposable walkthrough proving save/restart plus bare no-write behavior.

---

# SES-077 — Redo

Status: COMPLETE (2026-08-12)

Priority: P0

Plan: `docs/exec-plans/completed/ses-077-redo.md`

Result:

- `/undo` now actually records successful built-in UTF-8 file mutations, `/redo` restores undone mutations, and both run exact live-state comparisons plus restoration off the TUI thread.
- Drift and restoration failures preserve both stacks; a new mutation clears redo; receipts remain process-only and absent from model, hooks, Debug, requests, sessions, and other runners.
- Validation passed 933 unit tests plus 28 CLI tests, formatting, strict Clippy, release/profile smokes, and a released disposable TUI walkthrough proving direct/approved recording, exact cycles, invalidation, and drift refusal.

---

# SES-076 — Lazy History Storage

Status: COMPLETE (2026-08-12)

Priority: P0

Plan: `docs/exec-plans/completed/ses-076-lazy-history-storage.md`

Result:

- Session V4 semantics now persist through a stable metadata commit point plus one generation-scoped append-only JSONL history, while legacy V1-V4 files remain read-compatible until a successful migration save.
- `/resume` loads bounded recent/older pages on blocking workers, preserves the visible transcript anchor, and requires exact full hydration before provider submission or complete-history mutation.
- Validation passed 923 unit tests plus 28 CLI tests, formatting, strict Clippy, release/profile smokes, eight reviewed snapshots, and a released disposable TUI walkthrough proving migration, paging, full hydration, append reuse, and exact-pair archive/restore.

---

# CHG-075 — Guided Changes Walkthrough

Status: COMPLETE (2026-08-12)

Priority: P0

Plan: `docs/exec-plans/completed/chg-075-guided-changes-walkthrough.md`

Result:

- `/walkthrough` and `/explain-changes` now explain only complete bounded working/base/commit Git snapshots through one isolated ordered-report schema, refuse partial/untracked input before traffic, and retain an exact source hash that visibly becomes stale until explicit recapture.
- Validation passed 909 unit tests plus 28 CLI tests, formatting, strict Clippy, release/profile smokes, 14 reviewed snapshots, and a released disposable TUI walkthrough proving two distinct exact hashes/requests, stale detection, explicit refresh, and zero-traffic untracked refusal.

---

# SKL-074 — Semantic Skill Suggestions

Status: COMPLETE (2026-08-12)

Priority: P0

Plan: `docs/exec-plans/completed/skl-074-semantic-skill-suggestions.md`

Result:

- Safe discovered metadata now produces deterministic exact-ID, exact-alias, and bounded BM25 suggestions in the idle composer and `/skills`; every surface explicitly requires exact canonical activation and neither reads a body nor changes prompt/session/authority state.
- Validation passed 893 unit tests plus 28 CLI tests, formatting, strict Clippy, release/profile smokes, eight reviewed snapshots, and a released disposable TUI walkthrough proving zero traffic before activation and exact active-body-only request injection afterward.

---

# RUN-073 — Multi-Run and Best-of-N

Status: COMPLETE (2026-08-12)

Priority: P0

Plan: `docs/exec-plans/completed/run-073-multi-run-best-of-n.md`

Result:

- `/multi-run` and `/best-of` now freeze one request, run two to five exact-route candidates through `AgentManager` in distinct profile-owned worktrees, collect bounded hashed local evidence, and gate read-only judgment plus a non-applying fusion proposal behind separate Cancel-first confirmations.
- Validation passed 886 unit tests plus 28 CLI tests, formatting, strict Clippy, release/profile smokes, 14 reviewed snapshots, and a released disposable TUI walkthrough proving two distinct diffs, one isolated peer failure, sole-schema judge/fusion, unchanged main/config/session/default state, and retained worktrees/branches.

---

# AGT-072 — Dependency-Aware Agent Graph

Status: COMPLETE (2026-08-12)

Priority: P0

Plan: `docs/exec-plans/completed/agt-072-dependency-aware-agent-graph.md`

Result:

- Approved plan tasks now carry bounded acyclic dependencies, exact narrowed grants, optional existing owned-worktree assignments, append-only attempts, and local evidence gates; `/tasks` schedules only ready nodes through the canonical `AgentManager` and requires explicit retry after failure.
- Restore pauses interrupted graphs without provider traffic, descendants remain dependency-blocked until proven success, and full forks remap graph identity while clearing runtime ownership.
- Validation passed 869 unit tests plus 28 CLI tests, formatting, strict Clippy, release/profile smokes, fourteen reviewed snapshots, and a released disposable-home TUI walkthrough proving parallel isolated roots, evidence failure, explicit retry, and dependent release.

---

# WT-071 — Worktree Lifecycle

Status: COMPLETE (2026-08-11)

Priority: P0

Plan: `docs/exec-plans/completed/wt-071-worktree-lifecycle.md`

Result:

- `/worktree` now owns profile-scoped create/list/status/diff/attach/cleanup around derived Git paths and preserved user branches; identity mismatches and every dirty state fail closed, and cleanup is Cancel-first without force.
- Attached workflows and later child agents receive the exact isolated workspace while earlier children retain immutable snapshots; Session V4 stores only the owned attachment ID and fails safely when it cannot be restored.
- Validation passed 855 unit tests plus 28 CLI tests, formatting, strict Clippy, release/profile smoke gates, fourteen reviewed snapshots, real-Git safety regressions, and a released disposable-home TUI walkthrough.

---

# WFL-070 — Declarative Workflow Engine

Status: COMPLETE (2026-08-11)

Priority: P0

Plan: `docs/exec-plans/completed/wfl-070-declarative-workflow-engine.md`

Result:

- `/workflow run <path>` now validates bounded TOML/JSON DAGs before provider traffic, previews exact routes/tools/scoped grants Cancel-first, and schedules ready steps deterministically through the canonical provider/tool/approval loop.
- Session V4 carries bounded typed run state, named dependency outputs, evidence/retry decisions, and explicit pause/resume without persisting workflow prompts, tool arguments/results, endpoints, or credentials.
- Validation passed 834 unit tests plus 28 CLI tests, formatting, strict Clippy, release/profile smoke gates, 16 reviewed workflow snapshots, and an isolated released-TUI branching/tool-evidence walkthrough.

---

# GOA-069 — Goal Sessions

Status: COMPLETE (2026-08-11)

Priority: P0

Plan: `docs/exec-plans/completed/goa-069-goal-sessions.md`

Result:

- `/goal` now persists finite Session V4 state, evaluates bounded recorded evidence in a fresh context with only a non-executable report schema, and uses request-only feedback for autonomous continuation without synthetic canonical messages.
- Approvals, interactions, user steering, explicit pause, failures, cancellation, restoration, and finite iteration/token/time limits pause or terminate with typed reasons; in-flight requests retain correct goal-token ownership.
- Validation passed 810 unit tests plus 28 CLI tests, formatting, strict Clippy, release/smoke gates, 14 reviewed goal snapshots, and isolated released-TUI continuation/resume/pause walkthroughs.

---

# MEM-068 — Project and User Memory

Status: COMPLETE (2026-08-11)

Priority: P0

Plan: `docs/exec-plans/completed/mem-068-project-user-memory.md`

Result:

- Separate profile/workspace-scoped stores now remain inert by default, validate and bound all records, inject explicitly enabled stale-labelled recall, and expose local add/edit/export/delete through `/memory`.
- Model memory generation is an opt-in tool that produces only a process-local proposal; Cancel-first acceptance is the sole persistence route and sessions/structured output receive no proposal content.
- Validation passed 791 unit tests plus 28 CLI tests, formatting, strict Clippy, release/smoke gates, eight reviewed snapshots, and an isolated released-TUI proposal/accept/export/recall walkthrough.

---

# LSP-065 — Semantic Navigation

Status: COMPLETE (2026-08-11)

Priority: P0

Plan: `docs/exec-plans/completed/lsp-065-semantic-navigation.md`

Result:

- Explicit default-empty LSP configuration now exposes six Main-only read-only tools for symbols, definition, references, hover, implementation, and incoming/outgoing call hierarchy; bare and internal profiles receive none.
- The lazy JSON-RPC stdio manager converts 1-based Unicode positions to UTF-16, synchronizes bounded workspace documents, rejects external URIs, sanitizes/bounds results, denies network in the effective sandbox, and terminates/reaps exact process groups on failure, timeout, cancellation completion, and shutdown.
- Validation passed 748 unit tests plus 28 CLI tests, formatting, strict Clippy, release/smoke gates, four reviewed snapshots, and an isolated released-headless provider/tool continuation against a fake LSP executable.

---

# DOC-066 — PDF Text Reading

Status: COMPLETE (2026-08-11)

Priority: P0

Plan: `docs/exec-plans/completed/doc-066-pdf-text-reading.md`

Result:

- `read_pdf` now performs exact bounded page extraction on a blocking worker, preserves workspace authority, reports total/omitted/next-range metadata, and safely rejects encrypted, malformed, panic-prone, oversized, or out-of-range input without traversing embedded content.
- Validation passed 754 unit tests plus 28 CLI tests, formatting, strict Clippy, release/smoke gates, four reviewed snapshots, and an isolated released-headless provider/tool continuation against a generated local PDF.

---

# MM-067 — Typed Multimodal Content and Image Inspection

Status: COMPLETE (2026-08-11)

Priority: P0

Plan: `docs/exec-plans/completed/mm-067-typed-multimodal-image-inspection.md`

Result:

- Typed content now serializes bounded normalized images through Chat Completions, Responses, and Anthropic adapters while preserving exact text-only request fixtures and rejecting incompatible models before network I/O.
- `view_image` is a centrally selected read-only workspace tool whose process-only payload reaches exactly the next provider continuation; every durable or observable surface receives only safe MIME/dimension markers.
- Validation passed 768 unit tests plus 28 CLI tests, formatting, strict Clippy, release/smoke gates, six reviewed snapshots, and an isolated released-headless two-request walkthrough proving one image block and no base64 output.

---

# MCP-064 — Deferred MCP Tool Discovery

Status: COMPLETE (2026-08-11)

Priority: P0

Plan: `docs/exec-plans/completed/mcp-064-deferred-mcp-tool-discovery.md`

Result:

- Ordinary interactive/headless requests use a no-start active catalog; explicit bounded search starts only eligible servers and atomically activates exact schemas for the following continuation.
- Central selection, approval, safe errors, list-change invalidation, lifecycle cleanup, and `/context`/`/request` estimates operate on the process-only active set; bare and restrictive tool profiles cannot discover or start MCP.
- Validation passed 739 unit tests plus 28 CLI tests, formatting, snapshot hygiene, strict Clippy, release, smoke, six reviewed snapshots, and an isolated three-request released-headless walkthrough.

---

# MCP-063 — MCP OAuth

Status: COMPLETE (2026-08-11)

Priority: P0

Plan: `docs/exec-plans/completed/mcp-063-mcp-oauth.md`

Result:

- Streamable HTTP OAuth is explicit and generation-bound: browser PKCE and optional advertised device authorization pin all authority, store tokens by exact profile/server, refresh safely, and disconnect idempotently.
- Ordinary catalog loading never launches authorization; missing credentials become a typed actionable state, while challenges, endpoints, bodies, parameters, secrets, tokens, and credential sizes never enter safe state or output.
- Validation passed 733 unit tests plus 28 CLI tests, formatting, snapshot hygiene, strict Clippy, release, smoke, six reviewed OAuth snapshots, and an isolated released-TUI authorization-required walkthrough without browser or Keychain access.

---

# MCP-062 — Streamable HTTP MCP

Status: COMPLETE (2026-08-11)

Priority: P0

Plan: `docs/exec-plans/completed/mcp-062-streamable-http-mcp.md`

Result:

- Omitted transport remains lazy stdio; explicit `streamable_http` shares catalogs, namespaced tools/resources, approvals, project trust, distinct network grants, refresh, reconnect, and shutdown over HTTPS or literal-loopback HTTP.
- URL credentials/query/fragment data, process fields, unsafe schemes, reserved/literal headers, missing references, redirects, proxies, response bodies, and auth challenges fail closed without leaking authority; JSON and SSE are capped independently.
- Validation passed 717 unit tests plus 28 CLI tests, formatting, snapshot hygiene, strict Clippy, release, smoke, two reviewed status snapshots, and an isolated released-headless HTTP MCP tool-loop walkthrough.

---

# FAL-061 — Explicit Provider Fallback

Status: COMPLETE (2026-08-11)

Priority: P0

Plan: `docs/exec-plans/completed/fal-061-explicit-provider-fallback.md`

Result:

- Exact configured primary/backup pairs activate at most once, only after an allowed exhausted failure and before any usable text/reasoning/tool event; auth/not-found/protocol/content failures and partial output never switch.
- The active route stays pinned through tool continuations, failures never chain, per-attempt usage remains distinct, and the unchanged current/default primary is restored for the next user turn.
- Session V4 persists only sanitized per-turn provenance; provider deletion/model rename keep rules valid while preserving unrelated TOML/comments.
- Validation passed 707 unit tests plus 28 CLI tests, formatting, snapshot hygiene, strict Clippy, release, smoke, four reviewed wide/narrow marker snapshots, and an isolated released-TUI loopback walkthrough.

---

# AUTH-060 — Codex/ChatGPT Authentication

Status: BLOCKED WITHOUT IMPLEMENTATION (2026-08-11)

Priority: P0

Plan: `docs/exec-plans/completed/auth-060-codex-chatgpt-authentication.md`

Result:

- Official OpenAI documentation limits ChatGPT browser/device login to Codex-owned surfaces and limits Codex access tokens to Codex CLI or app-server clients; it directs general API traffic to Platform API keys.
- No supported third-party OAuth client registration, scopes, audience, refresh, or direct provider-host contract is published for `ca`, so `/connect` and token storage were deliberately not implemented.
- Reopen only for an official third-party contract or a separately approved Codex app-server adapter; never import Codex auth caches or private protocol details.

Superseded on 2026-08-14 by the user's explicit approval of the narrower,
documented shared-cache integration in `AUTH-061`.

---

# AUTH-061 — Codex Auth Cache Integration

Status: COMPLETE

Priority: P0

Plan: `docs/exec-plans/completed/auth-061-codex-auth-cache.md`

Goal: use the documented owner-only Codex `auth.json` cache for the exact
`openai-codex` provider on macOS and FreeBSD, including native authenticated
verification on the trusted `bsd` host.

---

# PRV-059 — Anthropic Messages

Status: COMPLETE (2026-08-11)

Priority: P0

Plan: `docs/exec-plans/completed/prv-059-anthropic-messages.md`

Result:

- `protocol = "anthropic_messages"` maps leading system text, local text, assistant client-tool calls, and coalesced tool results into exact `/messages` request structures with conditional `x-api-key` plus the stable version header.
- The bounded decoder translates thinking/text/multiple-tool/cache-aware usage events, tolerates documented future event extensions, rejects malformed known lifecycles, and discards payload messages, IDs, signatures, endpoints, and credentials.
- Validation passed 697 unit tests plus 28 CLI tests, formatting, strict Clippy, release, smoke, and an isolated credentialless loopback walkthrough.

---

# PRV-058 — OpenAI Responses

Status: COMPLETE (2026-08-11)

Priority: P0

Plan: `docs/exec-plans/completed/prv-058-openai-responses.md`

Result:

- `protocol = "openai_responses"` maps local text/function history to typed `/responses` Items, flat function schemas, optional effort, and exact function-result continuation while omitted protocol remains frozen Chat Completions.
- The bounded decoder translates reasoning/text/tool/usage events, requires typed successful completion, and drops response IDs, error messages, signatures, endpoints, credentials, and bodies.
- Validation passed 687 unit tests plus 28 CLI tests, formatting, strict Clippy, release, smoke, and an isolated credentialless loopback walkthrough.

---

# PRV-057 — Protocol-Neutral Provider Boundary

Status: COMPLETE (2026-08-11)

Priority: P0

Plan: `docs/exec-plans/completed/prv-057-protocol-neutral-provider-boundary.md`

Result:

- Missing protocol configuration remains exact Chat Completions without rewriting TOML; unsupported names fail before secret resolution or traffic.
- One adapter owns endpoint suffix, conditional authentication, stable request bytes, bounded SSE decoding, canonical tool/usage events, and safe body-free failures.
- Validation passed 676 unit tests plus 28 CLI tests, formatting, strict Clippy, release, smoke, and an isolated credentialless loopback walkthrough.

---

# HED-056 — Stronger Headless Contracts

Status: COMPLETE (2026-08-11)

Priority: P0

Plan: `docs/exec-plans/completed/hed-056-stronger-headless-contracts.md`

Result:

- Ordinary headless text/JSON/JSONL remains `ca.headless.v1`; opt-in check/schema runs emit explicit `ca.headless.v2` lifecycle events.
- `--check` uses a fresh bounded read-only context and exit 9 for a valid failed report; `--check-repair` remains under normal execution authority.
- `--output-schema` performs bounded no-follow local preflight, sole-final report enforcement, and local Draft 2020-12 validation before emitting/writing exact compact JSON.
- Validation passed 667 unit tests plus 28 CLI tests, formatting, strict Clippy, release, smoke, and an isolated loopback walkthrough.

---

# APR-055 — Rich Permission Policy

Status: COMPLETE (2026-08-11)

Priority: P0

Plan: `docs/exec-plans/completed/apr-055-rich-permission-policy.md`

Result:

- One typed evaluator applies Deny, process-only session grant, Ask, Allow, then the existing fallback after canonical preparation across interactive, headless, MCP, and provider-subagent execution.
- Unsafe shell syntax never auto-runs, network remains separate, legacy TOML is preserved, and `/permissions` safely filters and removes persistent or session rules through Cancel-first flows.
- Validation passed 657 unit tests plus 26 CLI tests, formatting, strict Clippy, release, help/version/profile smoke, ten reviewed snapshots, and an isolated loopback walkthrough proving no denied side effects or persisted session grant.

---

# TOL-054 — Central Tool Selection

Status: COMPLETE (2026-08-11)

Priority: P0

Plan: `docs/exec-plans/completed/tol-054-central-tool-selection.md`

Centralize tool resolution for every runner and add process-only `--tools`, `--include-tools`, and `--exclude-tools` controls without widening profile, approval, workspace, sandbox, or network authority.

Result:

- One process-only resolver now applies replacement, inclusion, exclusion, profile/capability/model/grant restrictions, and final built-in/MCP validation for every tool-capable runner before provider traffic.
- Hidden or unadvertised provider calls fail closed; selection does not widen approvals, workspace roots, sandboxing, network authority, or subagent grants. Bare requests omit `activate_skill` after final catalog assembly.
- Validation passed 635 unit tests plus 26 CLI tests, formatting, strict Clippy, release, help/version/profile/flag-order smoke, reviewed wide/narrow failure snapshots, and a disposable loopback TUI walkthrough proving exact order, unknown-name zero traffic, and unchanged approval behavior.

---

# SAN-042 — Sandbox Hub

Status: COMPLETE (2026-08-10)

Priority: P0

Plan: `docs/exec-plans/completed/san-042-sandbox-hub.md`

Add a truthful idle-only `/sandbox` manager and `/config` row without hot-switching already-constructed runtime boundaries.

Result:

- The manager shows saved next-launch and effective current modes, macOS backend readiness, sanitized writable roots, and mode-specific network enforcement.
- Required saves directly; policy-only/off require a Cancel-first warning. Saving runs on a blocking worker, preserves TOML comments/unrelated data, reloads config, and leaves the current process on its startup mode.
- Bare is view-only with actionable guidance. Validation passed 446 unit tests plus 25 CLI tests, formatting, strict Clippy, release, local help/version/profile smoke, ten reviewed snapshots, and isolated default/bare TUI walkthroughs.

---

# CTX-043 — Context-Limit Editor

Status: COMPLETE (2026-08-10)

Priority: P0

Plan: `docs/exec-plans/completed/ctx-043-context-limit-editor.md`

Add exact `provider/model` context-window and checkpoint-threshold editing with independent inheritance and between-turn application.

Result:

- `/context-limit` targets the current model and accepts an exact configured `provider/model`; `/config` delegates through the same command.
- Empty fields inherit independently through exact and legacy settings. Positive values are validated before I/O, and explicit/effective checkpoint thresholds must remain below a known window.
- Blocking atomic saves preserve comments, unrelated TOML, profile capabilities, providers, and defaults; successful saves reload config and invalidate only process context/checkpoint caches. Bare is view-only.
- Validation passed 461 unit tests plus 25 CLI tests, formatting, strict Clippy, release, help/version/profile smoke, ten reviewed snapshots, and an isolated cancel/save TUI walkthrough.

---

# USG-044 — Usage and Cost Ledger

Status: COMPLETE (2026-08-10)

Priority: P0

Plan: `docs/exec-plans/completed/usg-044-usage-cost-ledger.md`

Add a process-only `/usage` ledger covering every current TUI provider request with safe provenance, typed outcomes, detailed token subsets, and optional exact-decimal prices.

Result:

- Main turns, tool continuations, checkpoints, reviews, provider-backed subagents, and explicit provider probes create one provider/model-attributed record per request; completion, failure, and cancellation close records exactly once.
- `/usage` is read-only and available while busy. It shows process totals, request categories, durations, outcomes, cached/reasoning subsets, and newest records without retaining prompts, output, endpoints, or credentials.
- Exact `pricing."provider/model"` entries accept quoted decimal USD-per-million values; missing pricing remains `unknown` and there is no bundled table.
- Validation passed 474 unit tests plus 25 CLI tests, formatting, strict Clippy, release, help/version/profile smoke, four reviewed snapshots, focused loopback runner tests, and an isolated success/failure TUI walkthrough.

---

# PRV-045 — Local Provider Discovery

Status: COMPLETE (2026-08-10)

Priority: P0

Plan: `docs/exec-plans/completed/prv-045-local-provider-discovery.md`

Add explicit, credentialless `/provider discover` scanning only the approved loopback ports with short deadlines, bounded concurrency, no proxy/redirects/persistence, and selection into the existing Add Provider form.

Result:

- The explicit command scans exactly 103 approved `127.0.0.1` ports with 100-ms connect and 400-ms request deadlines, concurrency 32, no proxy, no redirects, no credentials/cookies, and bounded decoded catalogs/model IDs.
- A process-only stale-safe overlay supports scan/rescan/navigation/cancel. Enter opens the existing Add Provider form with editable `local-<port>`, exact loopback endpoint, and initial model; only Save persists and then refreshes that provider's full catalog.
- Validation passed 488 unit tests plus 25 CLI tests, formatting, strict Clippy, release, help/version/profile smoke, ten reviewed/updated wide/narrow snapshots, and an isolated loopback TUI walkthrough whose temporary config hash remained unchanged.

---

# BTW-046 — Ephemeral Side Questions

Status: COMPLETE (2026-08-10)

Priority: P0

Plan: `docs/exec-plans/completed/btw-046-ephemeral-side-questions.md`

Add `/btw [question]` during an active main turn using a frozen completed-context snapshot, independent generation/cancellation/display/usage state, no tools/hooks/MCP, and no canonical/session persistence.

Result:

- `/btw` is visible and available during a main turn; an optional argument starts immediately and the empty form provides an isolated Unicode-safe editor. Esc cancels only the side request.
- The request freezes the largest protocol-complete canonical prefix, supplies no tools, and excludes live output, unresolved tool suffixes, hooks, MCP, images, queued drafts, and persistent session state.
- Validation passed 498 unit tests plus 25 CLI tests, formatting, strict Clippy, release, help/version/profile smoke, ten reviewed wide/narrow snapshots, and an isolated loopback walkthrough proving only the main exchange was saved.

---

# KEY-047 — Keymaps and History Search

Status: COMPLETE (2026-08-10)

Priority: P0

Plan: `docs/exec-plans/completed/key-047-keymaps-history-search.md`

Add typed one- and two-stroke bindings with validation, `/keymap` inspection/reset/capture, dynamic help labels, and `/history` reverse search while keeping Ctrl+R bound to Diff and safety controls non-configurable.

Result:

- Eight global composer actions use validated one- or two-stroke chords. Malformed, duplicate, prefix-conflicting, unknown, character-only, Ctrl+R-first, emergency, editing, navigation, and modal-safety bindings fail before activation or persistence.
- `/keymap` filters, captures, resets, saves atomically, reloads immediately, and renders effective/custom/fixed controls; bare is view-only. `/history` is newest-first, duplicate-preserving, paste-isolated, process-only, and restores an editable draft without submission.
- Validation passed 515 unit tests plus 25 CLI tests, formatting, strict Clippy, release, help/version/profile smoke, 12 reviewed wide/narrow snapshots, and an isolated loopback TUI walkthrough covering one/two-stroke capture, restart persistence, reset, fixed Ctrl+R, and bare refusal.

---

# SES-048 — Session V4, No-Session, and Queue Persistence

Status: COMPLETE (2026-08-10)

Priority: P0

Plan: `docs/exec-plans/completed/ses-048-session-v4-no-session-queue-persistence.md`

Add defaulted pin/queued-draft fields with lazy V1–V4 compatibility, persist queue mutations safely, and add `--no-session` without disabling MCP, skills, hooks, retry, rewind, or in-memory forks.

Result:

- Session V4 stores defaulted pin state and ordered queue IDs/text/timestamps. V1-V3 reads remain non-mutating and upgrade the same file only on the next successful save; new branches deliberately start queue-empty.
- Every queue mutation updates the session snapshot, continuation pops persist before provider I/O, save failure restores the draft, and restored drafts remain visible without becoming auto-drain eligible.
- `--no-session` conflicts with startup resume options, displays an unmistakable title and command guidance, leaves config/MCP/skills/hooks enabled, and keeps retry/rewind/forks in memory without reading or creating session storage.
- Validation passed 524 unit tests plus 25 CLI tests, formatting, strict Clippy, release, help/version/conflict smoke, two reviewed wide/narrow title snapshots, and an isolated V4 resume/mutation/no-session retry/fork/rewind walkthrough.

---

# SES-049 — Archive and Pinning

Status: COMPLETE (2026-08-10)

Priority: P0

Plan: `docs/exec-plans/completed/ses-049-archive-and-pinning.md`

Add pin/unpin plus exact inactive-session archive/unarchive flows to `/resume`, using profile-scoped storage and no descendant cascade.

Result:

- `/resume` has searchable Active and Archive views, pinned/current markers, pinned-first active ordering, current-session protection, Cancel-first archive confirmation, busy locking, deterministic refresh, and inline failures.
- Pinning validates the storage key and UUID without changing recency. Archive/restore moves one exact file through a no-overwrite hard-link/unlink transfer, preserving bytes, branches, and historical parent metadata.
- Default and named profiles have separate archive roots; bare and `--no-session` retain their no-persistence boundary.
- Validation passed 539 unit tests plus 25 CLI tests, formatting, strict Clippy, release, help/version/profile smoke, reviewed wide/narrow snapshots, and an isolated default/named-profile walkthrough.

---

# SES-050 — Session Import and Export

Status: COMPLETE (2026-08-10)

Priority: P0

Plan: `docs/exec-plans/completed/ses-050-session-import-export.md`

Add bounded Markdown and normalized JSON export plus sanitized inert-history import with new UUIDs, unresolved-tool rejection, and confirmation before overwrite.

Result:

- `/export` emits bounded Markdown or deny-unknown-fields `ca.session-export.v1` JSON without source UUID, workspace, lineage, extensions, queue, checkpoint, runtime agent state, endpoint, or credentials. Existing regular files require Cancel-first confirmation and are replaced atomically; symlink/non-regular destinations fail closed.
- `/import` accepts only bounded local regular portable JSON, terminal-sanitizes text, rejects unresolved/duplicate/invalid tool histories, assigns a fresh UUID and current safe runtime defaults, and creates an inert saved session without replacing the current task.
- Default and named profiles store imports separately; bare and `--no-session` block import before I/O. All portability work is asynchronous, generation-bound, and failure-safe.
- Validation passed 557 unit tests plus 25 CLI tests, formatting, strict Clippy, release, help/version/profile smoke checks, reviewed wide/narrow snapshots, and an isolated default/named-profile walkthrough.

---

# SES-051 — Retention

Status: COMPLETE (2026-08-11)

Priority: P0

Plan: `docs/exec-plans/completed/ses-051-session-retention.md`

Add optional age/count retention, disabled by default, with an exact preview, Cancel-first confirmation, and current/pinned/archived-session exclusions.

Result:

- `/retention` and `/config` expose disabled-by-default age/count rules; saving is comment-preserving, atomic, asynchronous, and never deletes sessions.
- Exact previews use one captured time, exclude current/pinned/archived sessions, show Age/Count/Both reasons, default to Cancel, and fail closed above 10,000 candidates.
- Confirmed batches validate every storage key, UUID, current/pin state, and regular-file boundary before the first unlink; deletion never follows branch relationships, and partial filesystem failures report truthful removed/remaining counts.
- Validation passed 576 unit tests plus 25 CLI tests, formatting, strict Clippy, release, help/version/profile smoke, 14 reviewed wide/narrow retention snapshots, updated command-palette snapshots, and an isolated Cancel/confirm TUI walkthrough.

---

# HOK-052 — Complete Hook Lifecycle

Status: COMPLETE (2026-08-11)

Priority: P0

Plan: `docs/exec-plans/completed/hok-052-complete-hook-lifecycle.md`

Move hook execution off the TUI loop, complete the required lifecycle events and fail-closed repair path, and add a safe `/hooks` manager.

Result:

- Interactive main-session hooks now run through one bounded asynchronous runner with process-group cleanup, truthful sandbox/network status, exact SessionStart/BeforeSubmit/Pre/Post/Thought/Response boundaries, and no command/output/payload leakage into UI state.
- `/hooks` safely filters, inspects, toggles, reloads, tests, cancels, and repairs fail-closed startup; bare mode remains inspect-only and config changes preserve comments and unrelated TOML.
- Validation passed 606 unit tests plus 25 CLI tests, formatting, strict Clippy, release, isolated smoke, 16 reviewed wide/narrow snapshots, and a disposable live walkthrough including descendant cleanup.

---

# REQ-053 — Prepared Request and `/request` Inspector

Status: COMPLETE (2026-08-11)

Priority: P0

Plan: `docs/exec-plans/completed/req-053-prepared-request-inspector.md`

Route every provider runner through one prepared-request boundary and retain a single bounded, secret-free semantic snapshot for `/request`, without endpoints, headers, credentials, or raw binary content.

Result:

- Main/continuation, BTW, checkpoint, review, subagent, headless, scripted, and provider-probe paths construct an owned `PreparedModelRequest`; the current adapter preserves characterized Chat Completions model/message/tool/reasoning serialization.
- One last-prepared-wins process store exposes safe request kind, provider/model/protocol, estimates, instructions, messages/tool calls, and schemas. Runtime credentials are redaction inputs only; structured secrets, binary payloads, terminal controls, deep/wide JSON, Unicode fragments, and item/render totals are bounded before storage.
- `/request` is visible and read-only while busy, has a deterministic no-I/O empty state, and never enters canonical messages, sessions, usage, headless output, or debug credential surfaces.
- Validation passed 617 unit tests plus 25 CLI tests, formatting, strict Clippy, release, help/version/profile smoke, ten reviewed wide/narrow snapshots, and a four-request disposable loopback walkthrough covering main, tool continuation, `/btw`, and provider check.

---

# CFG-041 — Named Run Profiles

Status: COMPLETE (2026-08-10)

Priority: P0

Plan: `docs/exec-plans/completed/cfg-041-named-run-profiles.md`

Select one immutable whole-file profile before runtime construction, isolate named config/credentials/sessions, and provide a non-persistent `bare` run.

Result:

- `--profile NAME` preserves positional prompt and `@file` parsing; `default` keeps existing paths, named profiles use separate config/session roots, and missing profiles stay in memory until their first successful mutation.
- Named Keychain accounts and references include profile plus provider. Active non-default profiles appear in the title, `/config`, and doctor text/JSON.
- `bare` reads normal provider/safety settings while disabling MCP, skills/activation, hooks, config writes, and session persistence; provider checks, process-only model choices, retry, rewind, and forks remain available.
- Validation passed 431 unit tests plus 25 CLI tests, formatting, strict Clippy, release, help/version/profile smoke checks, and four reviewed wide/narrow profile snapshots.

---

# OUT-040 — Truthful Bounded Output

Status: COMPLETE (2026-08-10)

Priority: P0

Plan: `docs/exec-plans/completed/out-040-truthful-bounded-output.md`

Replace silent/double truncation with explicit ranged file reads, independent job-stream captures, capped web decoding, typed job outcomes, and labelled previews.

Result:

- `read_file` now streams 1-based inclusive ranges (200 default, 500 maximum) and reports total lines/bytes, omissions, and a next-range call within a 64-KiB result.
- Jobs retain independent stdout/stderr head and tail, report exact omitted bytes, share model space fairly, keep the originating owner, and classify status without parsing text.
- WebFetch applies timeout, content-type/status checks and a decoded 1-MiB cap before a truthful 4-KiB result; generic and text-headless clipping is UTF-8 safe and explicit.
- Validation passed 417 unit tests plus 23 CLI tests, formatting, strict Clippy, release, help/version, and four reviewed wide/narrow output snapshots.

---

# SES-039 — Saved Session Deletion

Status: COMPLETE (2026-08-10)

Priority: P0

Plan: `docs/exec-plans/completed/ses-039-saved-session-deletion.md`

Add confirmed deletion of one inactive saved session from `/resume` without cascading to branches or rewriting history.

Result:

- `Shift+D` opens a Cancel-first confirmation; the current session is visibly marked and protected in both input and orchestration.
- `SessionStore::delete` validates one filename component, re-reads the expected UUID, and unlinks exactly one JSON file. The TUI mutation runs on a blocking worker and keeps failures inline.
- Validation passed 406 unit tests plus 23 CLI tests, formatting, strict Clippy, release, help/version, and six reviewed wide/narrow session snapshots.

---

# CTX-038 — Context Transparency

Status: COMPLETE (2026-08-10)

Priority: P0

Plan: `docs/exec-plans/completed/ctx-038-context-transparency.md`

Report the latest complete prepared provider request by estimated category, use that complete estimate for checkpoint policy, and render persisted checkpoint boundaries without changing canonical messages.

Result:

- `/context` distinguishes an MCP-free preview from the latest prepared request and reports instructions, skills, built-in tools, MCP tools, checkpoint, conversation, attachments, total, and sanitized checkpoint details.
- Request preparation stores estimates only; exact prompt bytes, canonical messages, and Session V3 remain unchanged. Transcript checkpoint markers are derived during rendering.
- Validation passed 397 unit tests plus 23 CLI tests, formatting, strict Clippy, release, help/version, and four reviewed wide/narrow snapshots.

---

# REL-037 — Ordinary Provider Stream Resilience

Status: COMPLETE (2026-08-10)

Priority: P0

Plan: `docs/exec-plans/completed/rel-037-provider-stream-resilience.md`

Add shared connect/first-event/no-progress deadlines and bounded pre-output recovery without replaying any partial provider output.

---

# CFG-036 — Searchable Config Hub

Status: COMPLETE (2026-08-10)

Priority: P0

Plan: `docs/exec-plans/completed/cfg-036-searchable-config-hub.md`

Replace path-only `/config` with a searchable safe settings hub that delegates provider/model/MCP/skills editing to existing managers and exposes typed scalar settings without resolving secrets.

Result:

- `/config [filter]` opens a searchable overlay with current model, provider setup, reasoning, review model, MCP, skills, and config path rows; paste/filter/navigation stay isolated from the composer.
- Existing managers own complex work. Review-model selection has a dedicated picker target and atomic raw-TOML patch that persists discovered models while preserving chat defaults, credentials, comments, and unrelated settings.
- Validation passed 387 unit tests plus 23 CLI tests, formatting, strict Clippy, release, focused command/input/orchestration/config tests, and four reviewed snapshots. A live TUI walkthrough was not run.

---

# DIA-035 — Provider-Aware Doctor

Status: COMPLETE (2026-08-10)

Priority: P0

Plan: `docs/exec-plans/completed/dia-035-provider-aware-doctor.md`

Add secret-safe offline diagnostics through `ca --doctor` and `/doctor`. Network-backed provider checks remain explicit through `--connect` and reuse the existing typed provider-probe boundary.

Acceptance:

- Offline diagnostics cover runtime/config/default/catalog/sandbox/render readiness without provider traffic or runtime mutation.
- Text and aggregate JSON output use meaningful exit status and never expose credentials, URLs, or response bodies.
- `/doctor` is an idle-only searchable command that opens the same safe offline report.

Result:

- `ca --doctor` now checks runtime/config/default/provider/catalog/sandbox/first-frame readiness without contacting providers or creating config. Text and aggregate JSON use exit 0 for ready and exit 1 for failed checks; positional prompts remain compatible.
- `--connect` explicitly reuses the bounded isolated provider probe, orders the current provider first, and reports only typed safe outcomes. `/doctor` opens the offline report in the existing pager.
- Validation passed 382 unit tests plus 23 CLI tests, formatting, strict Clippy, release, help/version, two reviewed snapshots, and an isolated rebuilt-binary missing-config check with no runtime mutation.

---

# CFG-034 — Provider Setup and Credential Checks

Status: COMPLETE (2026-08-10)

Priority: P0

Plan: `docs/exec-plans/completed/cfg-034-provider-setup-credential-checks.md`

Add an explicit `/provider setup [filter]` walkthrough over every configured provider. Each check uses one isolated representative-model request, returns only typed secret-free status, helps repair rejected/missing credentials, and lets the user explicitly select a passing provider before `/retry`.

Acceptance:

- Opening setup performs no network request; Test is explicit, one-at-a-time, cancellable, bounded, and process-local.
- Provider rows distinguish working, missing environment, rejected/access-denied credentials, timeout/connect/route/rate-limit/provider/protocol failures, and missing models without showing response bodies, URLs, or secrets.
- Successful checks feed a finish chooser that atomically selects the tested provider/model; auth failures direct users through setup and then immutable `/retry` branching.

Result:

- `/provider setup [filter]` and `/providers setup [filter]` open a current-first checklist without network activity. Explicit tests use one deterministic representative model, one fixed user-only prompt, a 20-second timeout, bounded complete-stream validation, and process-only status.
- Authenticated and credentialless providers are supported; unresolved environment references fail locally. Typed outcomes cover working, missing environment/model, 401/403/404/429/5xx, timeout/connect, and invalid/incomplete streams without retaining provider bodies, URLs, generated text, or credentials.
- Successful checks advance to the next unchecked provider. Failures remain on a repair/retest detail; setup-specific add/edit returns invalidate only that provider. Finish offers only passing pairs and persists selection atomically, after which existing immutable `/retry` uses the updated model.
- Normal 401/403 chat failures now name only the provider/status and direct the user to `/provider setup <provider>`, then `/retry`. Provider client debug/error output is redacted and credentialless requests omit Authorization.
- Validation passed 376 unit tests plus 20 CLI tests, 57 focused provider tests, 45 render tests, 47 input tests, formatting, strict Clippy, release, help/version, and an isolated rebuilt-TUI walkthrough covering authenticated success, credentialless success, missing environment, and rejected credentials. The temporary fake home was moved to Trash; user config, Keychain, sessions, oracle files, and installed binary were unchanged.

---

# CFG-033 — `/provider` Management Hub

Status: COMPLETE (2026-08-10)
Priority: P1
Complexity: L
Depends on: CFG-032
Plan: `docs/exec-plans/completed/cfg-033-provider-management-hub.md`

## Outcome

Add an oracle-informed `/provider` manager for provider settings, all available provider models, secure add/edit flows, and confirmed provider deletion with deterministic replacement and Keychain cleanup. Provider IDs remain fixed and individual model deletion is excluded.

## Result

- `/provider [filter]` and `/providers [filter]` open a searchable provider-first list with the current provider first, safe credential labels, zero-model providers, and provider detail containing configured, documented-fallback, and live models.
- Provider settings can be edited without changing models/defaults; add-provider rejects duplicates; editor cancel/save returns to its originating manager surface. Direct keys remain masked and Keychain-backed, while environment-variable names remain the portable alternative.
- Provider deletion defaults to Cancel, removes exact provider-scoped config and the exact saved Keychain item, rolls raw config back on cleanup failure, and persists a deterministic replacement (including discovered-only models). No replacement means no confirmation or mutation.
- Catalog refresh after provider changes is scoped to the affected provider, preserving unrelated provider caches and selection. Endpoint summaries strip embedded credentials/query data before entering manager state, and new endpoints reject those forms.
- Validation passed 361 unit tests plus 20 CLI tests, formatting, strict Clippy, release, help/version, 43 render tests with 92 accepted snapshots and zero pending, and an isolated rebuilt-TUI flow covering fallback/live models, edit/add cancellation, targeted refresh, confirmed deletion, and automatic replacement. The temporary home was moved to Trash; user config, sessions, Keychain, oracle files, and installed binary were unchanged.

---

# CFG-032 — Secure API-Key Entry

Status: COMPLETE (2026-08-10)
Priority: P1
Complexity: L
Depends on: CFG-031
Plan: `docs/exec-plans/completed/cfg-032-secure-api-key-entry.md`

## Outcome

Let users paste a masked provider API key directly in the provider/model editor, save it in macOS Keychain, and keep environment-variable configuration as a portable alternative. Raw config stores only a provider-scoped Keychain reference; secret work stays off the TUI event loop and is covered through an injected fake store.

## Result

- Add Provider/Add Model/Edit Model now include a masked direct `API key` field plus the existing `API key env` alternative; picker copy makes the edit/key path explicit.
- Direct input is redacted, zeroized, excluded from actions/debug/status/snapshots, and rendered with a fixed mask that reveals neither content nor length. Entering one credential source clears the other unsaved source.
- macOS saves use a generic-password Keychain item keyed by provider and persist only `keychain://ca/provider/<provider>`. Exact references resolve at runtime through an injected store; missing/malformed entries fail before provider setup.
- Keychain/config work runs on a blocking worker. Storage failure restores the original raw config and leaves the form/key ready to retry; successful save reloads and selects the model.
- Validation passed 343 unit tests plus 20 CLI tests, 17 config-store tests, 2 provider-secret tests, 42 input tests, 39 render tests/74 reviewed snapshots, formatting, strict Clippy, release, help/version, and an isolated real-TUI masked-paste/cancel flow. No user Keychain/config/session state or installed binary changed.

---

## CFG-031 — Provider/Model Editor Ergonomics

Status: COMPLETE (2026-08-10)  
Priority: P1  
Type: ExecPlan  
Plan: `docs/exec-plans/completed/cfg-031-provider-model-editor-ergonomics.md`

Context:

CFG-030 made the populated picker operable, but Add still discards the selected provider and opens a blank combined form. The editor only appends or removes the last character, has no visible insertion point, hides even safe environment-reference names, and does not distinguish Add Model from Add Provider.

Acceptance:

- `Shift+A` adds a model to the selected provider with safe provider details prefilled; `Shift+P` opens a distinct new-provider form.
- Existing provider environment references are visible without ever exposing literal or resolved keys.
- Every editable field supports visible, Unicode-safe cursor movement, insertion, and deletion, with action-specific wide/narrow snapshots and completion gates.

Result:

- `Shift+A` now opens “Add model to <provider>” with the selected/current provider, base URL, and safe environment-variable name prefilled; Model ID is empty and focused. `Shift+P` opens the distinct blank “Add provider” flow.
- Add Provider, Add Model, and Edit Model are explicit modes. Existing provider IDs stay fixed, safe `${ENV_VAR}` names are visible, and literal/composite/resolved key material is never returned to the editor.
- Fields now support a visible insertion cursor, Left/Right, Home/End, Backspace/Delete, Unicode-safe insertion, Tab/BackTab, and cursor-aware long-value truncation. Paste targets the visible picker filter or editor field instead of the hidden composer.
- A rebuilt-binary check with an isolated temporary home selected MiMo, opened Add Model, showed `MIMO_API_KEY`, moved from Model ID into the existing URL, inserted text in its middle, and canceled without saving. The temporary home was removed; no user runtime state or installed binary changed.
- Validation passed 336 unit tests plus 20 CLI tests, format, strict Clippy, release, local help/version, 14 config-store tests, 41 input tests, and 39 render tests with 74 accepted snapshots and zero pending.

## CFG-030 — Model Picker Usability and Visibility

Status: COMPLETE (2026-08-10)  
Priority: P1  
Type: ExecPlan  
Plan: `docs/exec-plans/completed/cfg-030-model-picker-usability.md`

Context:

Catalog population is now provider-wide, but the picker remains hard to operate: its advertised shortcut is wired to another action, long lists do not scroll, selection can move out of bounds, the current model and provider health are unclear, and discovered-only edits fail.

Acceptance:

- `Ctrl+K` opens on the current model and filtered navigation is bounded, paged, and visibly scrolled.
- Current model and selected-provider catalog health are obvious; no-match and failure text tells the user what to do next.
- Configured and discovered-only edit paths both work, with deterministic wide/narrow coverage and completion gates.

Result:

- The advertised `Ctrl+K` path now opens on the active model without changing the composer; bounded Up/Down plus Home/End and PageUp/PageDown keep long filtered catalogs selectable and visible.
- Rows distinguish the active model from the navigation cursor, show match position and selected-provider live/fallback/unavailable state, and give an actionable no-match path. Catalog failure text is classified so request URLs and secrets cannot reach the picker.
- `Shift+E` on a discovered-only row now opens a valid prefilled add-to-provider draft, while selecting a discovered row immediately updates runtime config after atomic persistence so it can be edited in the same run.
- A rebuilt binary was exercised with an isolated temporary home: `Ctrl+K` showed six fallback/configured choices including `mimo-v2.5-pro` and `deepseek-v4-pro`, and selecting MiMo Pro updated the header and status line. No user runtime state or installed binary was changed.
- Validation passed 329 unit tests plus 20 CLI tests, format, strict Clippy, release, local help/version, 38 input tests, and 37 render tests with 70 accepted snapshots and zero pending.

## CFG-029 — Provider-Wide Model Catalog Completion

Status: COMPLETE (2026-08-10)  
Priority: P1  
Type: ExecPlan  
Plan: `docs/exec-plans/completed/cfg-029-provider-wide-model-catalog.md`

Context:

CFG-028 corrected MiMo only, leaving DeepSeek's discontinued one-model fallback and unnecessarily blocking Ofox's public live catalog when its chat key is unset. Complete the original model-picker outcome across all built-in providers.

Acceptance:

- DeepSeek exposes its two current V4 models when discovery is unavailable.
- Ofox's official public catalog refreshes without an API key; local/custom providers remain evidence-driven.
- MiMo remains fixed, obsolete defaults are not seeded in new configs, and all completion gates pass.

Result:

- DeepSeek now shows only `deepseek-v4-flash` and `deepseek-v4-pro` for official configs; discontinued aliases are upgraded in memory without rewriting raw user TOML.
- Ofox now loads its public live catalog without requiring or sending `OFOX_API_KEY`, then retains only text-output models advertising tool support. The live check returned 129 total and 100 agent-compatible models.
- MiMo retains its two current chat choices; Ollama/custom providers remain driven by their configured and actual live models rather than guesses.
- Validation passed 320 unit tests plus 20 CLI tests, format, strict Clippy, release, local help/version, 13 config-store tests, 7 catalog tests, 3 focused orchestration paths, and 35 render tests with 66 accepted snapshots and zero pending.

## CFG-028 — MiMo Model Catalog Recovery

Status: COMPLETE (2026-08-10)  
Priority: P1  
Type: ExecPlan  
Plan: `docs/exec-plans/completed/cfg-028-mimo-model-catalog-recovery.md`

Context:

The live catalog picker correctly retains configured models when providers fail, but the observed zero-live state leaves MiMo with only `mimo-v2.5`. Restore official environment-name compatibility and provide a documented chat-model fallback so `mimo-v2.5-pro` remains selectable without presenting incompatible speech endpoints.

Acceptance:

- MiMo shows `mimo-v2.5` and `mimo-v2.5-pro` when live discovery is unavailable.
- Existing `${XIAOMI_API_KEY}` references can resolve through official `MIMO_API_KEY` without rewriting raw config.
- Status distinguishes documented fallback from live discovery, and hermetic focused/completion checks pass.

Result:

- The picker now merges the documented MiMo chat catalog, so `mimo-v2.5` and `mimo-v2.5-pro` remain separate selectable rows even when `/models` is unavailable.
- New configs use the official `${MIMO_API_KEY}` name and include both chat models; legacy `${XIAOMI_API_KEY}` references can resolve from `MIMO_API_KEY` at runtime without rewriting or exposing the raw placeholder.
- MiMo ASR/TTS IDs are intentionally omitted because this coding agent uses chat-completion requests.
- Validation passed 315 unit tests plus 20 CLI tests, format, strict Clippy, release, local help/version, 12 config-store tests, 5 catalog tests, 2 focused orchestration tests, and 34 render tests with 64 accepted snapshots and zero pending. No credential-dependent Xiaomi request or user-runtime mutation was performed.

## CFG-027 — Live Provider Model Catalog

Status: COMPLETE (2026-08-10)  
Priority: P1  
Type: ExecPlan  
Plan: `docs/exec-plans/completed/cfg-027-live-model-catalog.md`

Context:

The provider/model editor made configured entries manageable, but the picker still shows only those static IDs. Query each configured OpenAI-compatible provider's authenticated model-list endpoint asynchronously and merge the returned IDs with offline-safe configured fallbacks.

Acceptance:

- Multiple provider-reported model IDs appear as distinct selectable rows.
- First open refreshes asynchronously; `Shift+R` refreshes explicitly and visible state explains loading/partial failure.
- Provider failures retain configured models and never expose credentials or response bodies.
- Selecting a discovered-only model atomically persists it as a fallback/default.
- Loopback, input, render, persistence, and completion gates pass without real credentials or user-state mutation.

Result:

- First picker open now asynchronously queries every configured OpenAI-compatible provider's authenticated `/models` endpoint; `Shift+R` refreshes explicitly.
- Provider-reported IDs merge with configured fallbacks, the current provider sorts first, loading/partial-failure state is visible, and external text is sanitized.
- Discovery is bounded to 8 concurrent providers, 10 seconds, 1 MiB, 2,048 IDs, and 256 bytes per ID; errors never display response bodies or credentials.
- Selecting a discovered-only ID atomically adds it to that provider's fallback list and makes it the default while preserving unresolved key placeholders.
- Validation passed 310 unit tests plus 20 CLI tests, format, strict Clippy, release, local help/version, 35 input tests, and 33 render tests with 62 accepted snapshots and zero pending. The Xiaomi live check was unrun because `XIAOMI_API_KEY` was unavailable to this process.

## CFG-026 — Provider and Model Manager

Status: COMPLETE (2026-08-10)  
Priority: P1  
Type: ExecPlan  
Plan: `docs/exec-plans/completed/cfg-026-provider-model-manager.md`

Context:

The model picker can only filter and select entries already present in `config.toml`. Add a keyboard-first, secret-safe form for adding a provider/model or editing the highlighted provider/model, then refresh the usable list immediately.

Acceptance:

- `A` and `E` are discoverable from the model picker and open add/edit forms.
- Literal/resolved API secrets never enter the form; only environment-variable names are persisted as placeholders.
- Atomic config patches preserve unrelated content and update selected/default references coherently.
- Focused input, render, config, and orchestration coverage is hermetic.

Result:

- Added discoverable `Shift+A` and `Shift+E` actions to the model picker plus a keyboard-only provider/model editor with inline validation feedback.
- API credentials are accepted only as environment-variable names and persisted as `${ENV_VAR}`; editing with a blank field preserves the existing unresolved reference.
- Atomic raw-TOML updates preserve unrelated comments/config, support new or existing providers, migrate exact model profile/context keys, and select the saved model as the runtime/default choice.
- Validation passed 302 unit tests plus 20 CLI tests, format, strict Clippy, release, local help/version, 33 input tests, and 32 render tests with 60 accepted snapshots and zero pending.

## Queue Rules

- Claim one task at a time by changing `Status` to `IN PROGRESS` and adding an owner/session note.
- Verify every named symbol/path before editing.
- Tasks marked `ExecPlan` require a plan under `docs/exec-plans/active/`.
- Do not broaden scope to historical `TODO.md` or `TASKS_FOR_AGENTS.md`.
- On completion, record actual validation, update the relevant harness docs, then remove or archive the task.

## TST-001 — Hermetic TUI and Protocol Test Foundation

Status: COMPLETE (2026-08-01)  
Priority: P0  
Type: ExecPlan

Context:

`src/test.rs` already renders `.ca` scripts with ratatui `TestBackend` at 120×40, but runs them through a live provider client. The project lacks hermetic multi-width snapshots and full tool-protocol fixtures. The Codex oracle demonstrates broader `TestBackend`/`insta` coverage but is a pattern source, not code to copy.

Allowed files:

- `Cargo.toml`, `Cargo.lock`
- `src/test.rs`
- new focused test-support modules under `src/` or `tests/`
- `tests/` fixtures/snapshots
- harness docs

Forbidden:

- `oracles/`, `reference/`
- runtime config/session files
- broad production refactors

Steps:

1. Characterize `tui::render` dependencies, the existing `src/test.rs::render_state` helper, and provider seams.
2. Extract or extend the existing render helper into deterministic coverage at no fewer than two widths.
3. Add fixtures for startup, streamed assistant content, one completed tool call, one approval prompt, and one picker.
4. Add a fake provider/transport seam capable of asserting outbound message order without network access.
5. Document snapshot review/update commands.

Acceptance:

- New tests run with network disabled and no credentials or user state.
- Snapshots include narrow/wide output and normalize volatile paths/time.
- Existing `cargo test` remains green.
- `TESTING.md` contains exact focused commands.

Validation:

- focused new test target;
- `cargo test`;
- format, clippy, and release build gates.

Stop if:

A test requires restructuring `main.rs` across multiple concerns; write the extraction as a separate ExecPlan milestone first.

Result:

- Added an injectable `ChatBackend` while retaining `AgentClient::chat_stream` for production HTTP/SSE behavior.
- Extracted the `.ca` command loop so Cargo tests can drive it with scripted deltas and explicit cwd/dimensions.
- Added in-memory shell history for user-state-free `AppState` fixtures.
- Added 10 reviewed `insta` snapshots for five states at 100×30 and 60×24.
- Characterized one-call and multiple-call assistant/tool message ordering with matching `tool_call_id` values.
- Added one fake-backed `.ca` script test requiring no provider credentials.
- Exact validation results are recorded in `TESTING.md` and `CONTINUITY.md`.

## PRO-002 — Verify and Fix Interactive Tool-Result Serialization

Status: COMPLETE (2026-08-01)  
Priority: P0  
Type: ExecPlan

Context:

Interactive messages keep tool output inside `ToolCall.output`, while `AgentClient::build_messages` visibly emits assistant tool calls. Confirm whether the next request emits matching `role: "tool"` messages; headless mode uses a separate path.

Allowed files:

- `src/agent/client.rs`, `src/main.rs`, `src/app.rs`
- focused protocol tests and docs

Steps:

1. Add a failing fake-transport test for assistant call → tool result → next assistant turn.
2. Cover one call, multiple calls, denied call, and tool execution error.
3. Define one canonical conversion from application conversation state to provider messages.
4. Reuse it in interactive/headless paths where practical without unrelated refactoring.

Acceptance:

- Exactly one correctly ordered tool message exists for every call ID.
- No request contains an unresolved assistant call unless the provider stream is still active.
- Multi-call order is deterministic.
- Headless and interactive tests use the same invariant.

Stop if:

Provider behavior differs by configured endpoint; capture the endpoint-specific requirement in an adapter decision before implementation.

Result:

- `AgentClient::build_messages` now emits each assistant tool call followed immediately by one matching `role: "tool"` message.
- Completed outputs preserve original call order, IDs, denial/error strings, and empty strings.
- Request construction fails before provider I/O for unresolved outputs or duplicate tool-result IDs.
- Interactive, headless, and scripted fake-backend paths reuse `append_provider_messages`; interrupted streams no longer persist incomplete tool-call metadata.
- Legacy `Role::Tool` state and serde-restored assistant turns have deterministic compatibility coverage.
- Validation passed 27 Cargo tests, including 11 protocol tests and 5 fake-backend tests; detailed evidence is in `TESTING.md`.

## APR-003 — Deterministic Multi-Tool Approval State Machine

Status: COMPLETE (2026-08-01)  
Priority: P0  
Type: ExecPlan  
Plan: `docs/exec-plans/active/apr-003-approval-state-machine.md`

Context:

Approval state is shared while one assistant response may contain several calls. Characterize current y/n/a/d progression and continuation before adding features.

Allowed files:

- `src/app.rs`, `src/main.rs`, `src/approval.rs`
- relevant input/render code only when the visible contract changes
- focused tests and docs

Steps:

1. Model pending work as an ordered queue with an explicit current decision.
2. Test mixed read-only/mutating calls, all four decisions, cancellation, and execution error.
3. Ensure approved calls execute once; denied calls produce coherent tool results.
4. Continue the provider loop once after the entire batch resolves.
5. Snapshot the prompt states if visible output changes.

Acceptance:

- No skipped, duplicated, or silently empty result.
- Persisted allow/deny policy is applied before prompting.
- Cancellation leaves state internally consistent.
- Approval tests require no shell/network/user files.

Result:

- Replaced the single approval slot with an invariant-preserving FIFO batch queue and explicit current/in-flight state.
- Classified persistent allow/deny rules before prompting and omitted unresolved calls from tool results until a real decision exists.
- Moved approved execution off the TUI event loop; duplicate keypresses and stale completions cannot resolve twice.
- Added coherent waiting/in-flight cancellation, typed denial status, and exactly-once continuation after batch exhaustion.
- Added reviewed `[1/1]`, `[1/3]`, and `[2/3]` approval snapshots at wide and narrow sizes.
- Validation passed 71 Cargo tests, including 45 focused approval tests and a production duplicate-denial regression; release/help passed, with only the same 4 pre-existing strict-clippy diagnostics remaining.
- Fresh spec-compliance and code-quality reviews passed. No commits, oracle edits, runtime-config writes, or live-provider claims.

## UNI-004 — UTF-8-Safe Composer Editing

Status: COMPLETE (2026-08-01)  
Priority: P1  
Type: Small if isolated

Context:

Input cursor logic mixes byte offsets with single-byte increments/decrements. Rust string editing requires UTF-8 boundaries.

Allowed files:

- `src/tui/input.rs`
- `src/app.rs` only if cursor semantics must be documented centrally
- focused tests

Steps:

1. Add failing tests for ASCII, emoji, combining characters, CJK, multiline paste, vim h/l/x/w/b, backspace, delete, and insertion.
2. Centralize previous/next character-boundary movement.
3. Keep cursor representation explicit and renderer-compatible.

Acceptance:

- No panic and no invalid boundary after any tested action.
- Existing ASCII behavior is unchanged.
- Visual cursor position is tested at a representative width.

Result:

- Added scalar byte-boundary helpers (`previous_char_boundary`, `next_char_boundary`) used by the affected movement/edit paths.
- Normal movement, backspace, delete, insertion, and paste now operate on UTF-8 character boundaries; no byte-offset panics on multi-byte content.
- Unicode word-boundary and Ctrl+W/@ shortcuts delegate to the same scalar helpers; selection and delete-word work correctly across emoji, combining marks, and CJK.
- Vim a/h/l/x/w/b modes respect UTF-8 boundaries; no intermediate invalid-state exposure.
- Renderer cursor-clamping added; new `insta` snapshots at 100×30 and 60×24 cover Unicode scalar cursor positions. Grapheme-cluster navigation remains explicitly out of scope.
- `cargo test`: PASS 91/91.
- `cargo test tui::input_tests -- --nocapture`: PASS 18/18 input tests.
- `cargo test tui::render_tests -- --nocapture`: PASS 11/11 render tests.
- 16 accepted `insta` snapshots, 0 `.snap.new` pending.
- Stage-1 spec-compliance review: PASS, no blockers.
- Stage-2 code-quality review: PASS, no blockers.
- `cargo fmt --check`: FAIL on pre-existing repository-wide drift (not a UNI-004 regression; no broad formatter run).
- `cargo clippy --all-targets -- -D warnings`: FAIL on 3 pre-existing diagnostics outside UNI scope — `src/agent/tools.rs` manual `Option::filter`, `src/approval.rs` two collapsible `if` statements. No `input.rs`/UNI diagnostics remain; old identical-branch lint was minimally fixed.
- No commits, dependency changes, oracle edits, runtime-config writes, or live-provider claims.

## CLI-007 — Make CLI Flag Parsing Side-Effect Safe

Status: COMPLETE (2026-08-09)  
Priority: P1  
Type: Small with subprocess tests

Context:

`./target/release/ca --version` was executed during harness verification. The flag was treated as prompt text, attempted a live provider request, and failed with HTTP 401. Unknown option-like arguments must not silently become prompts.

Allowed files:

- `src/main.rs`
- focused CLI subprocess tests and docs

Steps:

1. Add subprocess tests for `--help`, `--version`, an unknown flag, explicit one-shot prompt syntax, `--test <script-file>`, and positional file attachments.
2. Parse `--version` locally and return success without loading config or contacting a provider.
3. Reject unknown option-like arguments with usage and a nonzero exit.
4. Correct the current help label from `--test <url>` to the script-file contract used by `src/test.rs`.
5. Preserve documented prompt and attachment behavior.

Acceptance:

- `ca --version` prints the package version, exits 0, and performs no network/runtime-state access.
- `ca --unknown` exits nonzero with a concise error and usage.
- `ca --help` remains local and successful.
- Tests prove no provider call is made.

Result:

- Replaced the permissive manual parser with `clap` parsing before config, session, or provider initialization.
- `--help`, `--version`, and unknown-option errors are local; subprocess tests verify they create no config state.
- Preserved prompt, attachment, session, resume-list, and scripted-test arguments; corrected the script-file help contract.
- Focused CLI tests pass (3 parser units and 3 subprocess cases); full `cargo test` passes 97 tests.
- Strict clippy remains blocked only by the same 3 pre-existing diagnostics recorded in `TESTING.md`.

## CMD-010 — Unified Searchable Command Registry

Status: COMPLETE (2026-08-09)  
Priority: P1  
Type: ExecPlan  
Plan: `docs/exec-plans/completed/cmd-010-command-registry.md`

Context:

Slash dispatch and palette metadata are separate hard-coded lists. Centralize command identity, aliases, discovery, availability, and typed orchestration dispatch without changing unrelated command behavior.

Acceptance:

- One registry drives lookup, help, search, palette navigation, and dispatch.
- Hidden legacy commands do not appear in discovery.
- Input handling performs no command side effects.
- Focused command and render coverage is deterministic.

Result:

- Added one registry for command IDs, names, aliases, descriptions, usage, category, busy availability, mutation class, and visibility.
- Palette search now matches names, aliases, and descriptions, supports Up/Down selection, and uses the registry at wide and narrow widths.
- Input emits typed actions; Git, filesystem, config, session, clipboard, editor, and process effects are handled by orchestration.
- Registered every prior slash command and kept the legacy `/commit` command exact-name-only and disabled pending an approval-safe redesign.
- Focused command/input/render tests and two reviewed snapshots pass; full `cargo test` passes 105 tests.
- Release build and CLI smoke pass. Global format and strict-clippy gates remain blocked by the previously recorded repository baseline.

## SES-011 — Stable Sessions, Branching, and Timeline

Status: COMPLETE (2026-08-09)  
Priority: P1  
Type: ExecPlan  
Plan: `docs/exec-plans/completed/ses-011-stable-sessions.md`

Context:

Version-1 saves create timestamped files, `/fork` does not create a true branch, and `/rewind` destructively removes messages. Introduce stable atomic persistence and non-destructive history operations while retaining JSON and version-1 readability.

Acceptance:

- A test-injected `SessionStore` round-trips version 2 and updates one stable file atomically.
- Version-1 files remain readable and upgrade only when saved.
- Picker search and sorting include title, stable ID, workspace, and updated time.
- Timeline, fork, and rewind create independent child sessions without rewriting the source.
- Completed turns preserve canonical messages and resolved tool-call pairs.

Result:

- Added UUID-backed version-2 JSON sessions with title/workspace/timestamps/provider/model/parent/messages/checkpoint metadata.
- Added an injected `SessionStore`, atomic same-directory replacement, stable one-file upserts, updated-time sorting, and title/ID/workspace search.
- Version-1 files load without mutation, derive stable IDs from their storage names, and rewrite as version 2 only on save.
- Normal completed turns now autosave the current stable session; `/timeline`, `/fork`, and `/rewind` create independent child sessions.
- Six store/timeline tests, one input-state test, and four reviewed wide/narrow snapshots pass; full `cargo test` passes 114 tests.
- Release build and CLI smoke pass; the existing live TUI remained untouched.

## QUE-012 — Visible Queue, Editing, Steering, and Interruption

Status: COMPLETE (2026-08-09)  
Priority: P1  
Type: ExecPlan  
Plan: `docs/exec-plans/completed/que-012-queue-steering.md`

Context:

Queued input is currently a raw string vector drained after a normal answer, while ordinary Enter during streaming interrupts the active response. Replace it with stable queue items and explicit turn outcomes so busy Enter queues by default, Ctrl+S steers, Ctrl+C interrupts, and failed/cancelled turns never auto-drain.

Acceptance:

- `PendingInput` retains stable identity through edit/reorder operations.
- Busy Enter and Alt+Enter queue; busy Ctrl+S steers; Ctrl+C cancels without draining.
- Queue view supports selection, delete, reorder, edit, and steering an edited item.
- Up to three queued drafts render above the composer.
- Approval/question/picker modes keep precedence and protocol requests never retain unresolved tool-call metadata after steering.

Result:

- Added stable `PendingInput` IDs, explicit queue selection/edit state, reorder/delete/replace operations, and turn outcome gating.
- Busy Enter queues by default, Alt+Enter explicitly queues, Ctrl+S steers, and Ctrl+C cancels stream/tool/approval work without draining.
- Steering preserves assistant text while removing unresolved tool-call metadata; edited queue items are removed by ID when steered.
- `/queue` opens a full-screen editor and up to three deterministic previews render above the composer.
- Fourteen queue/orchestration/input/render tests and four new reviewed snapshots pass; full `cargo test` passes 128 tests.
- Release build and CLI smoke pass; the live TUI remained untouched and no user queue state was mutated.

## CTX-013 — Context Usage and Model-Generated Checkpoints

Status: COMPLETE (2026-08-09)  
Priority: P1  
Type: ExecPlan  
Plan: `docs/exec-plans/completed/ctx-013-context-checkpoints.md`

Context:

Current automatic compression destructively replaces canonical messages with a local lossy summary, and `/compact` only reports an estimate. Preserve full history, generate structured checkpoints through a no-tools model request, and build provider context from checkpoint plus post-checkpoint messages.

Acceptance:

- Canonical application/session messages are never replaced by compaction.
- Optional per-model limits remain backward compatible with string model lists and default automatic threshold to 80% of a known window.
- Manual/automatic checkpoints cut only at complete turn boundaries and preserve tool-call/result integrity.
- `/context` reports request estimate, model limit/threshold, checkpoint coverage, and cumulative session usage.
- Generation failure rolls back cleanly and does not enter an automatic retry loop.

Result:

- Removed destructive transcript replacement; canonical application/session messages remain complete.
- Added optional model-context settings, current request estimates, 80% known-window thresholds, and separate cumulative session usage.
- Added structured no-tools checkpoint generation, complete-turn boundaries, suffix reconstruction, persistence/restoration, stale-result rejection, and failure/no-retry state.
- `/compact` forces generation and `/context` opens a wrapped full-screen dashboard.
- Seven context/fake-model tests, restored-checkpoint coverage, and two reviewed dashboard snapshots pass; full `cargo test` passes 136 tests.
- Release build and CLI smoke pass; no live provider compaction request was made.

## DIF-014 — Full Working-Tree Change Review

Status: COMPLETE (2026-08-09)  
Priority: P1  
Type: ExecPlan  
Plan: `docs/exec-plans/completed/dif-014-working-tree-review.md`

Context:

`/diff` and Ctrl+R currently show only `git diff --stat`. Add a read-only full-screen review surface that distinguishes staged, unstaged, renamed, deleted, and untracked paths without attributing all changes to the agent.

Acceptance:

- `/diff`, `/changes`, and Ctrl+R open one “Working tree changes” view.
- Status and line counts cover staged/unstaged/deleted/renamed/untracked files in temporary repositories.
- Enter opens wrapped sanitized unified diffs for tracked files; Esc returns to the list.
- Untracked contents are never read automatically and the surface has no stage/revert/delete actions.
- Non-repository and Git errors render deterministically.

Result:

- Added an argument-safe asynchronous Git service for porcelain status, staged/unstaged numstat, and sanitized tracked-file unified diffs.
- `/diff`, `/changes`, and Ctrl+R open one full-screen “Working tree changes” surface with list, loading, error, and per-file diff states.
- The list distinguishes staged, unstaged, deleted, renamed, and untracked paths; untracked contents are never opened automatically.
- Temporary-repository tests cover all statuses, Unicode, long lines, sanitization, and non-repository failures; seven input/service tests and four reviewed wide/narrow snapshots pass.
- Full `cargo test` passes 145 tests; release and CLI smoke pass, and the live TUI remained untouched.

## PAR-005 — Capture One Clean Cursor Parity Slice

Status: COMPLETE — BLOCKED OBSERVATION RECORDED (2026-08-09)  
Priority: P1  
Type: ExecPlan  
Plan: `docs/exec-plans/completed/par-005-clean-cursor-evidence.md`

Context:

Current parity docs are largely inferred from minified/patched artifacts. Capture one clean, reproducible user-visible slice before changing broad UI behavior.

Allowed files:

- new textual fixtures/notes under `tests/fixtures/parity/`
- `PARITY.md`, `TESTING.md`, plan docs
- production code only after a failing characterization test

Steps:

1. Choose startup/composer, tool card, approval prompt, or resume picker.
2. Record Cursor version, terminal width/height, input/keystrokes, and output.
3. Cross-check the clean backup and extracted module names without copying source.
4. Add a deterministic local characterization test.
5. Update one parity-matrix row.

Acceptance:

- Fixture is reproducible and contains no secret, token, cert, or bundled code.
- The row identifies exact evidence and any intentional difference.

Result:

- Recorded `tests/fixtures/parity/cursor-runtime-blocked-2026-08-09.md` with the attempted version/terminal/input/capture conditions and exact blockers.
- The dated wrapper is locally modified and cannot establish stock behavior; the clean bundle is not a standalone CLI and failed under a network-denied empty-home launch before rendering user-visible output.
- No Cursor version, dimensions, keystrokes, or output fixture can be truthfully claimed from the available runtime. All matrix rows remain `Unverified`, and roadmap behavior remains labeled `ca extension`.
- No oracle/runtime file, credential, production source, installed binary, or live TUI was changed.

## ASY-006 — Remove Blocking Work from TUI/Input Paths

Status: COMPLETE (2026-08-09)  
Priority: P2  
Type: ExecPlan
Plan: `docs/exec-plans/completed/asy-006-async-shell-jobs.md`

Context:

Search for nested runtime calls and synchronous process/file/network work reachable from input and the event loop. Do not refactor based on name alone; first demonstrate UI starvation or an unsafe path.

Acceptance:

- Focused tests prove ticks/cancellation continue during slow work.
- Side effects run through asynchronous orchestration, not rendering/input reducers.
- No behavior or approval regression.

Result:

- Added a process-scoped `JobManager` with stable IDs, bounded stdout/stderr, channel events, stdin, timeout, cancellation, and process-group shutdown.
- Removed the direct-shell nested runtime/blocking path; `!` is responsive foreground execution, `!!` is silent, and `!&` starts a process-bound background job.
- Extended tools with background `run_command`, `await_job`, `write_shell_stdin`, and `kill_job` without changing approval classification.
- Added `/jobs` list/detail views with live status, elapsed time, recent output, scrolling, cancellation, and composer-based stdin.
- Fourteen lifecycle/tool/input/parser/approval tests and four reviewed wide/narrow snapshots pass; full `cargo test` passes 160 tests.
- Format and release gates pass; strict Clippy reports only the same 3 pre-existing diagnostics, and live PID 51440 remained untouched.

## SSE-008 — Harden OpenAI-Compatible Stream Framing

Status: COMPLETE (2026-08-09)  
Priority: P0  
Type: ExecPlan  
Plan: `docs/exec-plans/completed/sse-008-truthful-streams-retry.md`

Context:

`src/agent/client.rs` parses newline-delimited `data:` records from streamed byte chunks. Add fake-transport characterization for records split across reads, multiple events per chunk, comments, blank delimiters, `[DONE]`, malformed JSON, and multi-line data before changing parsing.

Acceptance:

- Arbitrary chunk boundaries do not lose or duplicate a delta.
- Stream termination and malformed input produce deterministic outcomes.
- Tests require no network or provider credentials.

## TOL-016 — Typed Tool Registry

Status: COMPLETE (2026-08-09)  
Priority: P0  
Type: ExecPlan  
Plan: `docs/exec-plans/completed/tol-016-typed-tool-registry.md`

Context:

Tool schemas, string dispatch, approval classification, and runner profiles are separate lists. Invalid/missing arguments often become empty strings, unknown tools return ordinary text, and identical name/argument calls are deduplicated despite distinct call IDs.

Acceptance:

- One registry owns built-in names, schemas, handlers, origin, mutation/capability/resource metadata, and profile membership.
- Typed `Deserialize + JsonSchema` adapters reject invalid arguments before side effects.
- Unknown tools fail closed; dynamic names cannot shadow built-ins.
- Distinct call IDs execute independently.
- Execution returns typed success/failure/denial/cancellation while preserving ordinary model-visible text.
- Strict Clippy passes, including the existing tool/approval baseline.

Result:

- Added one `ToolRegistry` for all 27 built-ins, with typed names/origins/profiles, generated schemas, serde validation, enum dispatch, mutation/capability/resource metadata, and typed execution outcomes.
- Main, plan, checkpoint, review, and subagent consumers select registry profiles; dynamic collisions and unknown tools fail closed.
- Missing required arguments fail before side effects, ordinary model-visible result text is preserved, and denial/cancellation are represented separately from success.
- Removed name/argument deduplication: identical invocations with distinct call IDs execute independently and preserve order.
- Auto-review approval classification now uses registry mutation metadata; the prior tool/approval strict-Clippy baseline is cleared.
- Validation passed 180 unit tests plus 3 CLI tests, format, strict Clippy, release, help, and version. No visible TUI output changed.

## HED-017 — Structured Headless Automation

Status: COMPLETE (2026-08-09)  
Priority: P0  
Type: ExecPlan  
Plan: `docs/exec-plans/completed/hed-017-structured-headless.md`

Context:

The current headless loop mixes provider setup, streaming, unsafe tool execution, diagnostics, and termination in `main.rs`. Automation needs injectable execution, stable machine-readable output, fail-closed approvals, and deterministic exit semantics.

Acceptance:

- `text`, aggregate `json`, and streaming `jsonl`/`stream-json` formats preserve strict stdout/stderr separation.
- `ca.headless.v1` emits monotonic lifecycle, assistant, tool, usage, approval, completion, and failure events.
- Headless approvals fail closed unless persistently allowed or explicitly bypassed with `--yolo`.
- Empty/setup/approval/transport/protocol/max-turn/internal outcomes use exit codes 2–8 as specified.
- Loopback fake-provider subprocess tests require no credentials or external network.

Result:

- Added an injectable headless runner and stable `ca.headless.v1` envelopes with monotonic sequence/run IDs and run, turn, assistant, usage, tool, approval, completion, and failure events.
- Added text, aggregate JSON, JSONL, and `stream-json` alias output with strict structured stdout and atomic last-message writes.
- Default/auto-review modes fail closed; persistent allow/deny rules remain effective, yolo cannot bypass interaction or network gating, and unknown tools fail as protocol errors.
- Added explicit exits 2–8, broken-pipe success, and SIGINT 130 with process job cleanup retained in `main`.
- Fifteen subprocess tests use temporary homes and a loopback fake SSE provider; nine runner tests cover policy, event order, typed results, and output failures.
- Full validation passed 189 unit tests plus 15 CLI tests, format, strict Clippy, release, help, and version. No TUI layout changed.

## INT-018 — Structured Interaction Engine

Status: COMPLETE (2026-08-09)  
Priority: P0  
Type: ExecPlan  
Plan: `docs/exec-plans/completed/int-018-structured-interactions.md`

Context:

Questions currently use a model-visible marker string, reuse the composer, and append a synthetic user message. Replace this with exact tool-call continuation, dedicated modal state, and ordered coexistence with approvals.

Acceptance:

- Canonical `request_user_input` supports free text, single/multi choice, confirmation, descriptions, numbered shortcuts, Other, and cancellation.
- `ask_question` remains an unadvertised restored-transcript alias.
- Answers resolve the original call exactly once without adding a user message or touching composer/queue drafts.
- Mixed approval/interaction batches retain call order and continue only when fully resolved.
- Host-only masked input is ephemeral, zeroized, nonserializable, and never model-visible.

Result:

- Added typed IDs, free-text/single/multi/confirmation requests, descriptions, 1–9 shortcuts, Other fallback, cancellation, and a dedicated draft/cursor/selection state.
- `request_user_input` is the advertised canonical tool; `ask_question` remains registered but unadvertised for compatibility. Marker scanning and synthetic user replies were removed.
- One ordered coordinator now sequences interactions, approvals, and later automatic tool execution by original call order; provider continuation waits for all calls.
- Answers update the exact originating tool call once. Stale IDs cannot overwrite results, and composer, queue, history, kill ring, and canonical user messages stay untouched.
- Added host-only masked state and `SecretHandle`; drafts/bytes zeroize, debug is redacted, types are nonserializable, and model/headless paths cannot receive secrets.
- Validation passed 199 unit tests plus 15 CLI tests, format, strict Clippy, release, help/version, and four reviewed wide/narrow modal snapshots.

## MOD-019 — Model Capabilities, Reasoning, and Activity

Status: COMPLETE (2026-08-09)  
Priority: P0  
Type: ExecPlan  
Plan: `docs/exec-plans/completed/mod-019-model-reasoning-activity.md`

Context:

Model context is partly configurable, but capabilities and reasoning formats are not. Provider reasoning cannot cross the shared stream boundary, and queue success is inferred from an idle state rather than a terminal outcome.

Acceptance:

- Exact optional model profiles retain legacy provider model lists and context configuration without name inference.
- Configured reasoning effort/request/stream formats work end to end; unknown models send and expose none.
- Authoritative phase/outcome state drives truthful activity and completed-only queue draining.
- Reasoning is persisted separately, collapsed by default, and controlled through `/reasoning`, `/thinking`, and Ctrl+T.
- One sanitizer removes terminal control families from externally supplied display text.

Result:

- Added exact optional model profiles for context, tools/images, configured reasoning efforts/defaults, request format, and stream field while retaining legacy string models and `model_context`.
- Unknown models send no reasoning option; configured flat/nested request formats and byte-split reasoning deltas share the provider boundary. Reasoning remains separate from answer text and structured headless output honors `--show-thinking`.
- Added persisted selected effort, visible model-default fallback, `/reasoning`, `/thinking`, and Ctrl+T configured-order cycling. Explicit `supports_tools = false` and `supports_images = false` are enforced.
- Added authoritative phase/outcome state for Sending, Waiting, Reasoning, Responding, Tool, and Interaction. Only `Completed` drains one queued draft; failed/cancelled/merely idle states do not.
- Centralized display sanitization for CSI, OSC, DCS, APC, BEL, C0, ST termination, and malformed/truncated escape input.
- Validation passed 209 unit tests plus 15 CLI tests, format, strict Clippy, release, help/version, and four new reviewed reasoning snapshots at wide/narrow sizes (46 accepted total, zero pending).

## WSP-020 — Workspace Roots, Approval Broker, and macOS Sandbox

Status: COMPLETE (2026-08-09)  
Priority: P0  
Type: ExecPlan  
Plan: `docs/exec-plans/completed/wsp-020-workspace-policy-sandbox.md`

Context:

Model paths currently accept arbitrary absolute/traversal/symlink targets, approvals and handlers resolve independently, and shell jobs inherit unrestricted filesystem/network access. Typed registry resource metadata now makes a single preparation boundary possible.

Acceptance:

- Stable primary/additional roots contain existing and created paths, including symlink-aware checks.
- Approval and execution consume the same prepared invocation with origin context.
- Process sandbox and network policy fail closed; yolo never widens roots, sandbox, or network.
- Direct shell and model tools share the policy, and job control is owner-scoped.
- `/add-dir` and `/workspace` expose session-local root management without changing user runtime state during tests.

Result:

- Added stable Git-root/cwd discovery, confirmed session-local additional roots, root-relative resolution, creation-ancestor validation, read-only enforcement, symlink/traversal containment, session persistence, and missing-root restore warnings.
- Registry path/cwd fields are normalized once into prepared arguments before policy. The origin-aware broker applies persistent deny first, never lets allow/yolo widen roots or network, and execution consumes the prepared JSON without a second workspace resolution.
- Added required-by-default `SandboxBackend`; macOS commands run through capability-checked `sandbox-exec` with scoped system reads, workspace/temp writes, and network denied unless explicitly requested and granted. Unsupported required mode fails closed; policy-only/off are explicit.
- Direct `!`, `!!`, and `!&` accept explicit `--network`; model/headless network remains separately gated. Stable run/session ownership scopes job list/await/stdin/kill and shutdown cleanup.
- Added `/add-dir`, `/workspace`, and `/roots`, plus reviewed wide/narrow workspace and palette snapshots. Validation passed 226 unit tests plus 15 CLI tests, format, strict Clippy, release, help/version, and macOS filesystem/loopback sandbox checks.

## SKL-021 — Filesystem `SKILL.md` Registry

Status: COMPLETE (2026-08-09)  
Priority: P0  
Type: ExecPlan  
Plan: `docs/exec-plans/completed/skl-021-filesystem-skills.md`

Context:

`ca` has no validated filesystem skill boundary. Project/user skill metadata should be discoverable without injecting full instructions, and activated bodies must be exact, bounded, hash-pinned, and unable to widen workspace/tool authority.

Acceptance:

- Strict project/user discovery rejects malformed, oversize, duplicate, and symlink-escaping documents; compatibility roots are opt-in.
- The model sees a bounded catalog only; full bodies load solely through exact user or approved tool activation within a 12,000-token budget.
- `/skills`, repeatable `--skill`, and an exact-ID tool support activation/deactivation/view/reload without fuzzy auto-activation.
- Sessions persist ID/path/SHA only and visibly reject missing or hash-changed references on restore.
- Focused and completion gates use injected temporary roots/state only.

Result:

- Added strict project/user/opt-in compatibility discovery with project precedence, same-tier duplicate rejection, disabled IDs, 128 KiB limits, canonical SHA-256, and symlink/root-boundary containment.
- Provider prompts expose a 64-entry/16 KiB metadata-only catalog. Bodies load only after exact ID activation, remain within a 12,000-token aggregate budget, and update the next turn after an approved model tool call.
- Added searchable `/skills` list/activate/deactivate/view/reload, repeatable headless `--skill`, and mutating `activate_skill`; aliases are searchable but cannot substitute for an exact activation ID.
- Sessions store only ID/path/SHA references, forks retain them, restored workspaces rediscover their own project catalog, and missing/changed refs remain inactive with visible reconfirmation warnings.
- Validation passed 243 unit tests plus 16 CLI tests, format, strict Clippy, release, help/version, and two reviewed wide/narrow skill snapshots (50 accepted total, zero pending).

## MCP-022 — Sandboxed Local-stdio MCP

Status: COMPLETE (2026-08-09)  
Priority: P0  
Type: ExecPlan  
Plan: `docs/exec-plans/completed/mcp-022-local-stdio-mcp.md`

Context:

The typed registry reserves MCP origins and profiles, but `ca` does not yet connect to external tool providers. Local stdio servers require explicit project trust, collision-safe discovery, bounded protocol surfaces, prompt-default approvals, and the existing workspace/sandbox policy.

Acceptance:

- Pin the requested client/child-process-only Rust SDK and start configured servers lazily.
- Require exact path-and-hash host trust for project configuration and filter/redact child environments.
- Route namespaced dynamic tools and bounded text resources through typed validation, approval, workspace, sandbox, and network policy.
- Add status/reconnect/trust controls, stale-catalog refresh, cancellation, and complete child cleanup.
- Validate with a hermetic fake stdio server and reviewed wide/narrow snapshots without touching live state.

Result:

- Added exact `rmcp` 3.1.2 client/child-process stdio support, lazy user/project configuration, canonical path-and-SHA project trust, atomic host trust patching, strict parsing, filtered environments, and secret-preserving runtime resolution.
- Added collision-safe namespaced dynamic tools and bounded text-resource list/read, compiled Draft 2020-12 argument validation, prompt-default MCP origins, stale-list refresh, status/verbose/reconnect/trust/network commands, and headless integration.
- Direct executable/argument sandbox preparation preserves argument boundaries; required macOS filesystem/network policy applies to servers, and process groups plus dedicated temp directories are cleaned on start failure, timeout, reconnect, cancellation, and shutdown.
- Validation passed 257 unit tests plus 18 CLI tests, format, strict Clippy, release, help/version, 10 hermetic MCP tests, 2 loopback subprocess tests, and two reviewed wide/narrow MCP snapshots (52 accepted total, zero pending).

## PLN-023 — Persistent Plans, Tasks, and Evidence

Status: COMPLETE (2026-08-09)  
Priority: P0  
Type: ExecPlan  
Plan: `docs/exec-plans/completed/pln-023-persistent-plans-tasks-evidence.md`

Context:

Session V2 cannot retain revisioned work artifacts, and plan mode stores one transient Markdown string. Durable plans need stable IDs, user-only approval, CAS mutations, recorded evidence, and explicit branch behavior before asynchronous agents can own tasks safely.

Acceptance:

- Read V1/V2 unchanged and atomically upgrade to V3 in the same file/UUID on normal save.
- Persist typed plans/tasks/decisions/evidence with stable IDs, serial revision checks, and recorded-event integrity.
- Preserve `/plan` mode switching while adding artifact show/approve/reject/edit and `/tasks` surfaces.
- Regenerate plan/task IDs with source lineage on full forks; omit unavailable historical work state visibly; never inherit running agents.

Result:

- Added Session V3 optional work/reasoning/workspace/skill/review/agent fields with defaulted V2 reads, stable-file upgrades, and unchanged V1 compatibility.
- Added typed `PlanId`, `TaskId`, revisioned `WorkState`, statuses, ownership/block reasons, decisions, recorded command/test/review events, evidence references, and CAS-safe model mutations. Completion requires successful recorded evidence, while only user orchestration can approve/reject.
- Added `propose_plan`, `edit_plan`, `update_plan_task`, and `record_plan_evidence`; plan mode advertises only its typed plan tools. `/plan` retains mode behavior and adds management actions; `/tasks` renders the artifact.
- Full forks regenerate plan/task IDs with source links and copy eligible records; historical forks omit work state with a restore notice; running agent summaries never inherit.
- Validation passed 264 unit tests plus 18 CLI tests, format, strict Clippy, release, help/version, 7 session migration/branch tests, 3 reducer tests, typed orchestration tests, and two reviewed plan/task snapshots (54 accepted total, zero pending).

## AGT-024 — Asynchronous Subagent Manager

Status: COMPLETE (2026-08-09)  
Priority: P0  
Type: ExecPlan  
Plan: `docs/exec-plans/completed/agt-024-asynchronous-subagents.md`

Context:

The legacy `task` handler blocks for one string, reloads global config, and invokes child tools outside the shared workspace/sandbox/approval/job boundaries. Durable task ownership now provides the prerequisite for stable asynchronous children.

Acceptance:

- Stable IDs and bounded status/events/output support spawn/list/await/message/cancel and foreground `task` compatibility.
- Configurable concurrency, depth, children-per-turn, provider-turn, runtime, and retained-output limits fail deterministically.
- Child tools use exact inherited tool/path/command/root/network grants and the shared registry, preparation, approval, sandbox, and job ownership boundaries.
- `/agents` exposes activity and approval waits; shutdown cancels children/jobs and persisted active summaries restore interrupted without resuming.

Result:

- Added a process-scoped `AgentManager`, stable IDs/status/context/grants/events, configurable defaults (3 concurrent, depth 2, 8 children per parent turn, 30 turns, 30 minutes, 256 KiB), bounded output, messages, cancellation, timeouts, per-child approval waits, and shutdown cleanup.
- Added `spawn_agent`, `await_agent`, `send_agent_message`, `cancel_agent`, and `list_agents`; `task` now routes through foreground spawn-plus-await and the raw handler fails closed without manager context.
- Provider children receive summary context, active skills, assigned-file and exact-command scopes, isolated job ownership, registry validation, prepared workspace resources, origin-tagged broker policy, sandboxing, and non-widening network rules. Headless child control fails closed.
- Added `/agents` list/cancel/message/approve/deny, session summaries that restore active states as interrupted, and no branch inheritance of running children.
- Validation passed 271 unit tests plus 18 CLI tests, format, strict Clippy, release, help/version, 6 manager/policy/nested-inheritance tests, and two reviewed agent snapshots (56 accepted total, zero pending).

## REV-025 — Fresh-context Model Review

Status: COMPLETE (2026-08-09)  
Priority: P0  
Type: ExecPlan  
Plan: `docs/exec-plans/completed/rev-025-fresh-context-review.md`

Context:

Working-tree inspection existed only as a non-model `/diff` viewer. Model review needed an argument-safe bounded Git boundary, a context isolated from the active conversation and extensibility state, a read-only tool profile, structured findings, and durable follow-up records.

Acceptance:

- `/review [working|base <branch>|commit <sha>|custom <instructions>]` builds at most 1 MiB of complete per-file diff sections and lists untracked names without reading their contents.
- The provider receives only project instructions and the delimited review input, with read-only file/Git tools plus one structured findings reporter.
- Unsafe refs, malformed/incomplete reports, mutating tools, and provider failures fail deterministically without adding a successful record.
- Successful reviews persist typed findings/evidence and append only a concise canonical exchange; `/diff` remains unchanged.

Result:

- Added the async `/review` workflow with working/base/commit/custom targets, strict ref/SHA validation, argument-array Git calls, per-file 1 MiB capping, omitted-file metadata, and untracked-name-only privacy.
- Added an injectable fresh-context reviewer using the shared stream assembler, an exact read-only review profile, sole `report_review_findings` terminal tool, typed severity/file/line/confidence/validation output, and centralized terminal sanitization.
- Added optional explicit review provider/model config, cancellation/idle-command gating, pager output, recorded review evidence, Session V3 review persistence, concise canonical follow-up, and reviewed wide/narrow snapshots.
- Validation passed 281 unit tests plus 18 CLI tests, format, strict Clippy, release, help/version, temporary-Git and fake-provider privacy/protocol tests, and 31 render tests with 58 accepted snapshots and zero pending.

## TUI-015 — Selection-Friendly Terminal Lifecycle

Status: COMPLETE (2026-08-09)  
Priority: P1  
Type: ExecPlan  
Plan: `docs/exec-plans/completed/tui-015-terminal-lifecycle.md`

Context:

`src/tui/event.rs` enables mouse capture but discards every mouse event, preventing ordinary terminal selection without providing pointer interaction. Setup also enables raw mode before fallible alternate-screen/terminal initialization.

Acceptance:

- TUI startup never enables mouse capture while mouse input is unhandled.
- Partial setup failure restores raw/alternate-screen state already acquired.
- Normal return, error, cancellation, and unwinding drop the lifecycle guard and restore the terminal.
- Tests exercise lifecycle ordering without a real terminal.

Result:

- Removed unused mouse capture so normal terminal selection is no longer intercepted by `ca`.
- Added a testable RAII terminal guard that cleans acquired state in reverse order on drop, partial setup failure, and panic unwinding.
- Validation passed 165 unit tests plus 3 CLI tests, format, release, help, and version; strict Clippy retains only the 3 documented pre-existing findings.

## ENV-009 — Remove Developer-Specific HOME Fallbacks

Status: COMPLETE (2026-08-09)  
Priority: P2  
Type: ExecPlan  
Plan: `docs/exec-plans/completed/env-009-runtime-paths-config.md`

Context:

Production paths in `src/main.rs`, `src/agent/tools.rs`, `src/config.rs`, and `src/session.rs` fall back to `/Users/andrewzimmerman` when `HOME` is absent. Approval tests also contain that path as fixture data; test-only examples are harmless but should use neutral temporary paths when touched.

Acceptance:

- Missing `HOME` never reads or writes another developer's path.
- Config, sessions, helper tools, and startup agree on one explicit path-resolution policy.
- Tests isolate environment mutation and use temporary roots.

Result:

- Added injected `RuntimePaths` for config, data, sessions, and shell history; missing `HOME` now fails deterministically.
- Added `ConfigStore`, which resolves provider-key placeholders only in the runtime copy and atomically patches the raw TOML for approvals/model defaults.
- Removed implicit Hermes env loading and all developer-specific production fallbacks; production startup injects session/history/config locations.
- Added temporary-root coverage for path derivation, config creation, raw-placeholder preservation, and shell history persistence.
- Validation passed 162 unit tests plus 3 CLI tests, format, release, help, and version. Strict Clippy retains only the documented 3 pre-existing findings owned by `TOL-016`.
