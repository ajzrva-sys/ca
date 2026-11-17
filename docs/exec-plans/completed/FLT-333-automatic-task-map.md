# FLT-333 — Automatic task breakdown and live task map

Status: COMPLETED and installed as CA 0.1.86 (2026-11-08)

Ordinary new interactive Agent submissions now resolve normal attachments and
explicit skills, then use the selected model for read-only task planning. The
existing structured planning result configures the executable graph before
review. Type revisions or choose Enter — Approve and start; approval binds the
exact saved revision. Tasks retain shared purpose, constraints, relevant inputs,
dependencies, grants, budgets and evidence checks. One-task maps are supported.
Unsupported images remain held with an explanation. Ask/headless and active
steering keep their existing entry paths. Direct Agent submission is available
through `agent.automatic_task_breakdown=false`.

The expanded review becomes a compact, collapsible live task map. Stable theme
colors and numbered labels connect tasks to progress/output; status text and
icons remain separate. `/tasks` provides live details, explicit retry, pause,
resume and cancellation. Ctrl+X then T collapses the map. Narrow terminals retain
stacked task/dependency labels. Host evidence controls completion; worker
self-report is shown as verifying until accepted.

A failed node blocks its descendants while independent branches continue within
existing concurrency limits. Shared-workspace writers are conservatively ordered
before review. Verified results survive recovery, restored work remains paused,
and execution revisions require renewed review. Worker reservations save before
dispatch; post-dispatch receipts save/read back exactly. Save uncertainty pauses
work without replay. Durable background outcomes release queued input only on
success and preserve any active foreground steering turn.

Filtering stays active in the background. The filter manager and provider-copy
pane open explicitly through `/filter`; `/filter guide` retains manual local
suggestions. Closing the window restores conversation space. Original request,
file and document region highlighting remains deferred, as requested. No cheaper
model selection, Cursor parity or provider-refusal benefit is claimed.

## Completed work

- [x] Ordinary submission, revision and exact-revision approval reuse the existing workflow/graph services.
- [x] Attachment/skill/queue preservation and before/after-commit save/readback coverage.
- [x] Independent overlap, writer ordering, failed-branch isolation, bounded explicit retry, cancel/resume and evidence contracts.
- [x] Deterministic wide/narrow color, ownership, live inspector, long-history and second-update rendering coverage.
- [x] Frozen source independently reviewed; focused checks and full workspace gate pass.
- [x] One release build, recoverable signed installation and exact installed-entry readback.

## Source and verification

Frozen source digest: `5c873a2c28e7ed8d63a8c50565f751f2cdfac3f4acb4d100a31372f0b5437c86` (1,220 inputs).
Independent source and delta reviews: `scratch/FLT-333/review.json`.
Final combined delivery review passes at
`scratch/FLT-333/independent-final-review.json`; final receipt:
`scratch/FLT-333/final.json`.
Full gate: `scratch/FLT-333/full-3/report.json` and `evaluation.json`; source
identity matches before/after with no diagnostics. Commands: `./scripts/verify.sh
--output scratch/FLT-333/full-3`, including workflow checks, formatting, strict
workspace/all-target/all-feature Clippy, default/all-feature workspace tests,
release build, signing, runtime evaluation and independent evaluator.

Default workspace: 4,863 passed, zero failed, 45 ignored, 29 targets.
All-feature workspace: 5,019 passed, zero failed, 46 ignored, 30 targets.
Ignored opt-in coverage remains explicitly unexecuted in the gate report.
Focused receipts include `scratch/check-change/runs/20261108T155116Z-5136/`
(82), `20261108T160355Z-34912/` (104), `20261108T161318Z-60204/` (102),
and final queue correction `20261108T164333Z-39003/` (20); these overlap and
are not summed. Each passes formatting and strict affected-target lint.

Verifier change is limited to explicit direct mode in its disposable legacy
fixture; original request-count, tool-effect and independent-review criteria
remain intact. Its intentional-defect/exact-restoration self-test passes at
`scratch/FLT-333/verifier-self-test/result.json`. Automatic default behavior is
qualified separately by `tools/task_map_pty.py` on the exact release candidate
and installed binary.

Both `candidate-terminal/receipt.json` and `installed-terminal/receipt.json`
under `scratch/FLT-333` pass: 13 actual loopback requests, 195 canonical messages,
normal file/skill submission, read-only authoring, typed revision, approval,
overlapping independent workers, two visibly distinct pending updates at narrow
and wide sizes, independent native documentation write after another branch
fails, blocked descendant, explicit retry with unchanged verifier, and final
4/4 host-verified completion. Filter canary is absent from every physical
provider request while filter UI stays closed. Terminal state is restored.
Pending, partial and complete text/ANSI captures and full requests are retained.

Earlier failed attempts remain recorded. First full attempt found a test-only
Clippy initializer issue (`full-abort.json`); second found two queue integration
failures (`full-2-abort.json`). Both stopped before release and are not passes.
Narrow corrections preserve original direct-queue contracts and add automatic
admission/background-outcome regressions. Native iteration also repaired typed
revision/skill retention, bounded live progress and contradictory absolute-command
grant validation. Fixture setup failures and all failed receipts remain retained.

## Installation and preservation

Build job 57484 completed; install job 13485 completed. Logs:
`scratch/FLT-333/full-3.log`, `install-qualified.log`, `install.log`.
Installed `/Users/aji/.local/bin/ca` reports `ca 0.1.86`; signed candidate,
staged and installed SHA-256: `4d950d175576acd7bab4a50a53434d70e6151d60fb9ce77b5bdc6f0cb3c3f3e1`.
Mode/owner, `--version`, `--help`, and strict code signature readback pass.
Recovery copy: `/Users/aji/.local/bin/.ca-release-stage.jca8jU/ca.recovery-ORC258-20261108T172928Z-8042a7a81c31ce2987fbe1abb94f95fa3b7a57f26117a18d38e0adbae6244831`.
All 4,457 protected configuration/session metadata entries match baseline
(`protected-check.json`); running sessions were left alone. No settings or
production sessions were mutated. Oracle/reference sources remain untouched.

This supplied workspace has no `.git` or `.agent/PLANS.md`; exact source
inventories replace commit identity. Loopback/native evidence qualifies local
orchestration and display, not real-provider decomposition quality, refusal
reduction, cross-platform behavior or opt-in external/VM/soak tests.

## Changed files

- `Cargo.lock`
- `Cargo.toml`
- `src/agent/subagent.rs`
- `src/agent_graph.rs`
- `src/app.rs`
- `src/automatic_tasks.rs`
- `src/config.rs`
- `src/keymap.rs`
- `src/main.rs`
- `src/plan.rs`
- `src/plan_workflow.rs`
- `src/plan_workflow_runtime.rs`
- `src/tui/input.rs`
- `src/tui/provider_pane.rs`
- `src/tui/render.rs`
- `src/tui/render_tests.rs`
- `src/tui/snapshots/ca__tui__render_tests__keymap_bare.snap`
- `src/tui/snapshots/ca__tui__render_tests__keymap_capture.snap`
- `src/tui/snapshots/ca__tui__render_tests__keymap_error.snap`
- `src/tui/snapshots/ca__tui__render_tests__keymap_manager.snap`
- `src/work.rs`
- `tools/task_map_pty.py`
- `tools/verify_runtime.py`

Documentation: `CONTINUITY.md`, `NEXT_STEPS.md`, `ARCHITECTURE.md`, `TESTING.md`,
`PARITY.md`, `docs/.INDEX.md`, `docs/USER_GUIDE.md`, `docs/CONFIGURATION.md`, and
this completed plan. No further implementation task remains for FLT-333.
