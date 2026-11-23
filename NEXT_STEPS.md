# Next steps

## Queue rules

Small local work uses the task template in this file. Risky, multi-file, protocol, event-loop, approval, or session-format work gets an ExecPlan under `docs/exec-plans/active/` that follows `.agent/PLANS.md`. Link the plan here. The plan holds the steps.

Install receipts that used to fill this file are in [docs/archive/NEXT_STEPS-install-notes-2026-11-13.md](docs/archive/NEXT_STEPS-install-notes-2026-11-13.md).

`python3 scripts/check-pyramid.py` is the comment ceiling, the `src/tui/` blocking-call ban, and the oracle include ban. It is not a new ExecPlan. Do not start another file in `docs/exec-plans/active/` unless the task names it. [verify-drive](docs/exec-plans/active/verify-drive.md) is the named plan for an opt-in proof file, an isolated in-process driver, and `ca verify`. M1 through M3 are in ca 0.1.187. M4 and M5 stay unstarted.

ca 0.1.205 writes a high-effort diff concern and one follow-up edit onto the plain message after an earlier edit. An empty answer records `ran: false` on that message and still finishes. A parallel child keeps the user task. `finish` clears that worktree path, and the merge runs only after every part of the job is idle. LoopForge slice 2 and verify-drive M4 and M5 stay unstarted. Focused check `scratch/check-change/runs/20261123T045605Z-88452/report.json` passed. It is not a full gate. A new process is required. The 0.1.204 check `scratch/check-change/runs/20261123T043951Z-87241/report.json` passed for the earlier wiring.

The provider filter and project-privacy rewriter are removed in ca 0.1.181. The plan is [docs/exec-plans/completed/remove-filter.md](docs/exec-plans/completed/remove-filter.md). `[provider_filter]` still parses and does nothing. The previous sources are in [docs/archive/filter-removed-2026-11-18/](docs/archive/filter-removed-2026-11-18/). Focused check `scratch/check-change/runs/20261118T122653Z-6694/report.json` passed. It is not a full gate. A new process is required.

The latest local fix is ca 0.1.167. Shift+Tab cycles Normal, Plan, Auto-review, and Always-approve. The installed file matches candidate `41c41bf9d7ec5218887df025b5df548114b26de8c78725f626aabc16408825eb`. Focused check `scratch/check-change/runs/20261117T032757Z-88374/report.json` passed. A batch that completes the goal and then waits for approval keeps that goal completed. Tool dispatch publishes the active `/goal` goal before `update_goal` and copies it back after. `scheduler_create` enables a `ScheduleStoreV2` record that `claim_due` can run and honors `fire_immediately`. The built-in spec count is 126. ca 0.1.162 added `search_replace`, `todo_write`, `update_goal`, `enter_plan_mode`, `exit_plan_mode`, `lsp`, `scheduler_create`, `scheduler_list`, `scheduler_delete`, and `monitor`. ca 0.1.163 added `image_gen`, `image_edit`, `image_to_video`, and `reference_to_video`. Focused check `scratch/check-change/runs/20261117T030121Z-87653/report.json` passed. Those receipts are not a full gate. A new process is required. `workflow` stays unspecified and is not added. verify-drive is the named plan; do not start a different file in `docs/exec-plans/active/`.

## AGT-271: Task-first economic delegation

Status: Done
Priority: High

Closed record. M0 through M5 are finished. A live cheaper-quality claim stays separately authorized and is not started. The open named plan is [verify-drive](docs/exec-plans/active/verify-drive.md) M4 and M5. Do not start another file in `docs/exec-plans/active/` unless the task names it. The hold list is in [docs/exec-plans/README.md](docs/exec-plans/README.md).

Context:
- Closed. M0 through M5 are checked in the plan. The live cheaper-quality claim is not started.
- The open named plan is verify-drive M4 and M5.
- ORC-291's progress boxes are all checked, including install as 0.1.26, so it is not the continuation.

Instructions:
1. Do not implement this plan. It is finished.
2. The open work is verify-drive M4 and M5.
3. A live cheaper-quality claim stays separately authorized. Do not add a second mechanical executor.

Acceptance criteria:
- M0 is recorded: the test exists, production code is unchanged, and the focused compile failed on the missing function.
- M5 is recorded: `economic_delegation_acceptance_fixture` passes without a provider credential. A live cheaper-quality claim stays separately authorized.
- This file still names the finished plan.

## MAIN-SPLIT: Move seams out of main.rs

Status: Stopped
Priority: Low

The tool-dispatch and turn functions call the private functions that fill `src/main.rs`. Moving one of them does not compile unless those functions move with it or gain a new interface. That is not a straight move. AGT-271 M5 is finished. This stays stopped. The open named plan is verify-drive.

## GRD-010: Check new ExecPlans for required sections

Status: Done
Priority: Medium

Allowed files:
- `scripts/check-exec-plan.py` (create)
- `docs/QUALITY.md` (status line for GRD-010 only)
- `TESTING.md` (one command row)

Forbidden files:
- Product source under `src/` and `crates/`
- Existing files in `docs/exec-plans/`

Context:
- `docs/QUALITY.md` GRD-010 is enforced by `python3 scripts/check-exec-plan.py <plan.md>`. Existing plans were not rewritten.

Reference parity:
- Reference: N/A
- Match: the section names in `.agent/PLANS.md`
- Do not match/copy: `crates/codex-team/.agent/PLANS.md` headings
- Verification method: run the new script on a fixture plan and on one known-good plan such as `docs/exec-plans/active/SEC-158-privilege-supervisor-and-service-v2.md`

Instructions:
1. Read `.agent/PLANS.md` and the GRD-010 entry.
2. Add a script that reads one plan path and exits non-zero with `active ExecPlan missing required section: <name>` when a required heading is absent.
3. Point `TESTING.md` and GRD-010 at that command. Leave existing plans unchanged.

Acceptance criteria:
- A plan missing `## Recovery and Rollback` fails with that message.
- `docs/exec-plans/active/SEC-158-privilege-supervisor-and-service-v2.md` passes.

Tests:
- `python3 scripts/check-exec-plan.py docs/exec-plans/active/SEC-158-privilege-supervisor-and-service-v2.md`
- One temporary fixture with a section removed, then removed after the run.

Discovery budget:
- `.agent/PLANS.md`, `docs/QUALITY.md` GRD-010, and the SEC-158 headings. Stop if SEC-158 no longer has those headings.

Stop conditions:
- Stop if the check would need to rewrite historical plans to pass.
- Stop if the script would import product crates.

## DEF-stack-and-personality: Recorded test-default failures

Status: Done
Priority: High

`editable_personality_reaches_actual_provider_prompt_with_profile_and_bare_isolation` passes. An unanswered personality template is not sent as user-authored steering. Seed-only documents still load. `sol_parent_routes_mocked_simple_tasks_to_luna_mimo_and_deepseek` passes on the default test stack and still checks the mocked simple routes to luna, mimo, and deepseek.

The local queue after this is Held plans in `docs/exec-plans/README.md` and stopped MAIN-SPLIT. Do not start those.

## Done

DOC-001 is done. Completed-plan narration from `docs/.INDEX.md` is in [docs/archive/INDEX-completed-plans.md](docs/archive/INDEX-completed-plans.md). Active-plan links stayed in the index.

xAI (Grok) Responses is installable from the `xai` template with `${XAI_API_KEY}`, and from the `xai-oauth` row with a device-code sign-in and a stored refresh token. The official route is `https://api.x.ai/v1`. The starter model is `grok-4.7`. Encrypted reasoning replay covers `grok-4.5`, `grok-4.6`, and `grok-4.7`.
