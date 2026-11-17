# PLN-289 — Cross-provider plan handoffs and TUI graph

Status: COMPLETED (2026-10-15)
Owner/session: current Codex task
Last updated: 2026-10-15

## Goal

Provide persistent author → review → revise → review workflows across selected
provider/model routes, including a return to an earlier participant, and allow
implementation on any selected provider. Expose manual handoffs, automatic
sequences and bounded review loops through one controller and a TUI graph.

## Evidence and Current Behavior

- Native plans, review cards, the plan reader, task graph, routed subagents and
  fresh Agent execution already exist and will be reused.
- The current checkout is `/Volumes/projects/ca`; it has no `.git` directory or
  `.agent/PLANS.md`. This plan follows the existing PLN-241 ExecPlan format.
- The established FLT-287 gate remains failed: 157 default / 158 all-feature
  test failures, 60 displayed existing lint diagnostics, and an existing TUI
  Reviewer timeout. Raw evidence is `scratch/verify/FLT-287-final-v3/`;
  classification evidence is `scratch/filter-review/20261015T014453Z/`.
- This is a user-requested CA feature, not an oracle parity assertion.

## Scope

Implement the approved workflow/controller, typed tool, native persistence,
Markdown publication, route selection and TUI graph. Preserve unrelated source
changes, runtime configuration/sessions, existing assertions and all oracle and
reference evidence. Verification uses only disposable state and credentialless
loopback providers. The standing release rule authorizes the final recoverable
signed executable installation after applicable validation.

## Design and Invariants

- `plan_workflow` supports start, handoff, status, pause, resume and cancel.
  Participants retain identity and frozen actual provider/model routes. Each
  visit is a separate ordered ledger entry with predecessor and plan versions.
- Author/revise stages publish complete proposals; review stages preserve the
  plan and record structured feedback. Acceptance binds to the exact reviewed
  version. Replacing a proposal resets stale approval/task evidence.
- Planning stages are read-only; implementation starts a fresh Agent request
  with current workspace/approval rules. The initial request fixes whether
  execution is authorized; reviewers cannot broaden that authorization.
- Fixed sequences and loops share a controller. Review loops default to three
  passes and at most two intervening revisions, pausing at exhaustion before
  implementation. Planning-only workflows stop after planning.
- Durable structured state is authoritative. Save state and publish `plan.md`
  plus immutable revision Markdown under profile plans/session/workflow before
  advancing. Stale/malformed/provider/save/publication failures halt dependent
  stages and preserve the last committed plan.
- Restore pauses workflows. Full forks retain history with new identities and
  cleared execution authority; historical fork omission and portable session
  format remain unchanged. Existing run/storage limits remain enforced.
- `/plan send`, `/plan run`, `/plan graph` and routed `/plan execute` share the
  tool controller. The handoff graph is separate from `/tasks`; every visit
  shows participant, route, role, version and status and opens plan/feedback.

## Milestones

1. Inspect the existing plan, provider, runner and persistence boundaries.
2. Implement native workflow state/controller, route/tool integration and tests.
3. Implement manual handoff controls, graph rendering/navigation and coverage.
4. Exercise actual Chat Completions/Responses and TUI handoffs in local fixtures.
5. Run focused checks and the required gate; classify baseline failures without
   weakening existing criteria, then perform standing release/install readback.
6. Reconcile project records and complete this plan with actual evidence.

## Acceptance Criteria and Validation

- Credentialless actual binary proof: A authors, B reviews, A revises, C reviews
  and D implements on a separately selected provider, spanning Chat Completions
  and Responses. Assert route/model attribution, returning participant context,
  committed versions/history, read-only planning tools and fresh execution tools.
- Focused tests cover defaults and exact routes, sequences/loops, exact-version
  acceptance, planning-only versus authorized automatic implementation, errors,
  cancellation, stale results, save/publication boundaries, restore and forks.
- Wide/narrow deterministic render and control tests cover graph navigation and
  pause/resume/cancel. Real terminal checks cover graph scroll, visit inspection,
  handoff/execute, clean exits and terminal restoration. Retain provider requests,
  terminal frames, receipt and binary hash.
- Run focused tests while iterating, then `./scripts/verify.sh`. Inspect raw
  report/evaluation/failing logs, preserving each failure and distinguishing new
  regressions from the independently recorded baseline. No broad baseline repair.
- After applicable validation, bump once, run background workspace/all-feature
  release, preserve the prior installed binary recoverably, stage atomically,
  sign, verify candidate/staged/installed hashes, mode/owner, signature, version
  and help. Record the actual background job/log and installed outcome.

## Progress

- [x] Approved plan captured; checkout and verification baseline inspected.
- [x] Native workflow and persistence implementation.
- [x] Tool/runtime and routed execution integration.
- [x] TUI handoff graph and navigation.
- [x] Focused, render and credentialless actual-binary verification.
- [x] Required gate and known-baseline reconciliation; raw gate remains failed.
- [x] Background release/install and installed readback.
- [x] Harness reconciliation and completion record.

## Verification Evidence

- Added `tools/plan_workflow_pty.py`, a standard-library actual-binary fixture
  spanning Chat Completions and OpenAI Responses. Python compilation passes.
- Failure-first prior installed ca 0.1.22 (signed SHA-256 `4f01a503e8a32be194bc6cf154b43c1446506231ada04e61d78bf4b356019cbf`)
  lacks `plan_workflow` in its actual advertised tool catalog; the fixture fails
  before any A/B/C/D request, exits cleanly and restores terminal settings.
  Receipt: `scratch/plan-workflow/before-installed-corrected/receipt.json`.
- An earlier fixture configuration attempt used `responses` instead of the
  established `openai_responses` enum and is preserved separately at
  `scratch/plan-workflow/before-installed/`. It is fixture error evidence, not
  evidence about workflow behavior.
- First integrated debug candidate SHA-256
  `29b22b4e596474e53728a01f5f7d0cfa343b94a8ad75f1affbc7531dcf2c86f8`
  completes A → B → A → C and D implementation in the actual terminal fixture.
  Saved native work proves five distinct visits, the frozen routes, exact C
  acceptance, successful D command evidence and completed typed task. Three
  immutable Markdown files match the author, revision and implementation
  snapshots. Wide/narrow graph, selected revision reader and provider picker
  frames are retained at `scratch/plan-workflow/debug-third/`.
- This debug replay remains **failed**, because its older artifact lacks the
  subsequently implemented editable handoff UI, and failure cleanup exited by
  SIGTERM without terminal restoration. The fixture now closes modal views and
  uses bounded interrupts; final-source replay is still required. Earlier
  Responses metadata-header and frozen-revision fixture mistakes are retained
  in `debug-first/` and `debug-second/`, without source repairs or weakened
  acceptance assertions.
- The corrected actual terminal replay passes at
  `scratch/plan-workflow/debug-final-02/receipt.json`: debug binary SHA-256
  `25798cb73bc7748ccbc1564be4416c1b928d49275711d94317009cc1a8e6c941`,
  18 actual requests across Chat Completions and Responses, five automatic
  visits plus manual review by the same B participant, route reuse, preserved
  plan, graph/reader/picker navigation at 120×36 and 60×24, child exit zero and
  exact terminal restoration. Post-exit native readback remains completed with
  six visits. No fixture error or production configuration change occurred.
- Initial feature-focused checks pass 37/37. The broader focused selection has 73
  passed and seven independently retained existing header-padding snapshot
  failures; formatting passes. Root logs are
  `/tmp/ca-plan-workflow-feature-focused.log`,
  `/tmp/ca-plan-workflow-focused-final.log` and
  `/tmp/ca-plan-workflow-format-final-02.log`.
- Required gate attempt `scratch/verify/PLN-289-final/` was intentionally
  interrupted (exit 143) after a concrete restored-history hydration issue was
  found before workflow control saves. Its raw state and
  `scratch/plan-workflow/interrupted-gate.json` are retained. A narrow
  failure-first preflight repair and a new complete gate at
  `scratch/verify/PLN-289-final-v2/` remain necessary; the interrupted gate is
  not a completed verification result.
- The restored-history regression was executed before repair and failed 0/1
  at `/tmp/ca-plan-workflow-history-before.log`. The narrow preflight repair
  keeps the workflow action queued until full history is hydrated; a separate
  process-only publication flag keeps public Status read-only. The updated
  feature selection passes 38/38 at
  `/tmp/ca-plan-workflow-feature-focused-v2.log`.
- Gate v2 ran on frozen source
  `33e11a0bb7248506959a5ab24e566c82194de493844375f2fe7b6749dde06337`.
  Its verification criteria match the retained FLT-287 baseline exactly.
  `scratch/plan-workflow/focused-evidence-v2.json` binds the focused logs and
  explicitly identifies the debug terminal receipt as prior-source evidence;
  actual v2 release-candidate and installed replays remain required.
- Gate v2 was intentionally interrupted (exit 143) after four new main-target
  failures were identified. Three enforce the intentional additional built-in,
  Main/Plan route-discovery profiles and prepared-request tool count. The
  fourth correctly detects that the production `prompts/plan_mode.md` still
  omitted the new workflow guidance even though the strict expected text
  already required it. The production prompt is corrected; exact assertions
  remain strict and gain explicit workflow-tool presence coverage.
- `scratch/plan-workflow/partial-default-classification-v2.json` retains the
  actual four panic contexts and raw evidence hashes with release readiness
  explicitly false. V2 lint independently matches all 60 existing diagnostics.
  The corrected 42-case focused selection and a complete v3 gate remain
  necessary. No release/version bump or installation has occurred.
- The final 42-case selection passes with strict built-in/schema/profile counts
  and full prompt equality retained. The missing valid-argument example for the
  new tool was added after the earlier focused attempt exposed it. Logs:
  `/tmp/ca-plan-workflow-feature-focused-v4.log` and
  `/tmp/ca-plan-workflow-format-v3.log`.
- Gate v3 is running on frozen source
  `df698747d64d61808ff01c3a846894985445f44d8b3ddebda4b0d69fa72b6bca`,
  with unchanged verification criteria. Focused/source evidence is pinned in
  `scratch/plan-workflow/focused-evidence-v3.json`; complete gate classification
  and final artifact/installed replays remain pending.
- V3's complete default matrix has 3,956 passed / 157 failed / 43 ignored
  across 29 targets; its complete all-feature matrix has 4,112 passed / 157
  failed / 44 ignored across 30 targets. Both have zero filtered tests and
  no new failed test identity. All 157 current failures match FLT-287; the
  all-feature native Git flake passes this run. All four narrow gate-discovered
  checks now pass in both full matrices. Strict Clippy retains exactly the
  same 60 diagnostic file/message/count identities. Release/runtime stages
  and the independent final assessment are still pending. The comparator's
  raw all-feature compile field includes Cargo's summary changing from five
  failed targets to four; the exact test identities establish the resolved
  native Git target failure rather than a new compiler diagnostic.
- A source-bound background release/install wrapper and exact baseline
  comparison are staged under `scratch/plan-workflow/`. Comparator selfchecks
  preserve the failed baseline and detect deliberate added failing-test and
  lint identities. No version bump or installation had run at that checkpoint.
- Gate v3 completed all eight stages with unchanged source and criteria. The
  workspace/all-feature release build passes in 267.782 seconds. Official
  runtime has seven passes, the identical existing independent-review deadline
  failure, and dependent resize not run; clean child exit and terminal
  restoration pass. Raw gate and evaluator stay failed.
- The final exact comparison at
  `scratch/plan-workflow/baseline-comparison-v3.json` adds no failed test,
  lint, runtime or evaluator identity. Its frozen mechanical parser retains
  exit 1 for the Cargo summary changing from five failed targets to four.
  `scratch/plan-workflow/release-assessment-v3.json` independently reviews that
  summary delta and binds 48 immutable evidence files. Standing release was
  ready for parent review, conditional on signed final-candidate and actual
  installed terminal replays; full verification is explicitly not passed.
- Parent reviewed the separate assessment and ran background job 44584, which
  completed with exit zero. Required version bump 0.1.22 → 0.1.23, full
  workspace/all-feature release, signed candidate acceptance, recoverable atomic
  install and installed readback pass. Build/install log:
  `scratch/plan-workflow/release-install.log`.
- Final signed candidate and actual `/Users/aji/.local/bin/ca` independently pass
  the unchanged full terminal fixture, each making 18 Chat Completions/Responses
  requests across A → B → A → C → D plus manual return to B. Each retained six
  visits, three immutable Markdown revisions, exact routes/acceptance/task
  evidence, wide/narrow graph/reader/picker navigation, unchanged fixture config,
  child exit zero and terminal restoration. Receipts:
  `scratch/plan-workflow/release-0.1.23/{candidate-pty,installed-pty}/receipt.json`.
- Candidate, staged and installed SHA-256 all equal
  `5581d53b75457a592a6ef7d92ca1acb53b4bfb31e491ba93c069c6ee2c2a76ca`.
  Installed version/help, uid 501 / gid 20 / mode 0755, valid code signature and
  prior recovery hash pass. Authoritative readback:
  `scratch/plan-workflow/release-0.1.23/installed-readback.json`.
  The release source digest is
  `f9ade3b2d3592cef0efb44623a797b8c56706494cb9dd6ff21e981f63c3d3bd6`;
  only Cargo.toml/Cargo.lock version metadata changed after the validated source.
  Production configuration/sessions and existing processes were preserved.
  Existing repository failures remain tracked separately under VAL-288.

## Recovery and Rollback

Preserve prior task-local evidence and source before-images; revert only this
task's changes. Workflow failures preserve the latest committed plan and stop
dependent work. Binary installation retains a timestamped SHA-256-labelled
recovery copy and restores it if post-install checks fail. Leave running sessions
alone.
