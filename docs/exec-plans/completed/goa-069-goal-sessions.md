# GOA-069 — Goal Sessions

Status: COMPLETE
Owner/session: Codex `/root`
Last updated: 2026-08-11

## Goal

Add durable, finite `/goal` sessions that evaluate recorded work evidence in a fresh read-only context after each completed agent response, continue only while the goal remains active and within explicit limits, and pause or stop with a visible typed reason instead of running indefinitely.

## Non-Goals

- Infinite/autonomous background work, silent approval, automatic interaction answers, hidden spending, parallel goals, schedules, workflow DAGs, or remote evaluators.
- Treating the main assistant's claim of completion as proof, allowing an evaluator to mutate files/tools/session state, or auto-resuming after user input/approval.
- Cursor parity. Ante documents the product idea but no clean Cursor or Ante runtime fixture proves this behavior.

## Evidence and Current Behavior

- `SessionV4` is backward-readable through defaulted incremental fields and already persists plan/task/evidence state, but has no goal state (`src/session.rs`).
- `WorkState` records bounded command/test/review events and evidence-linked task status; this is the strongest existing durable source for evaluator grounding (`src/work.rs`).
- `UsageLedger` owns process-wide typed provider records and token subsets but has no goal-evaluation kind or durable per-goal accounting boundary (`src/usage.rs`).
- Plain successful turns converge through `finalize_completed_plain_response`, which marks the turn complete, saves, starts checkpointing, and drains the queue (`src/main.rs`). This is the exact decision seam for evaluate/continue/finish.
- Fresh-context review already demonstrates isolated prepared requests, a constrained tool set, typed sole-final reporting, usage recording, and bounded validation (`src/review.rs`). Goal evaluation is stricter: no executable tools and one non-executable reporting schema.
- Approval and interaction requests enter `ApprovalQueue`/`ManualResolutionQueue`; submitted user input can cancel/steer an active turn. Goal autonomy must pause before those paths can silently resume.
- Ante's documented `/goal` lifecycle is descriptive evidence only. This plan is an intentional finite `ca` extension.

## Scope

Allowed files: a new goal state/evaluator module; Session V4 defaulted state; config limits; usage/request kinds; command/app/input/render state; main-loop orchestration; focused loopback/temp-session tests and snapshots; required harness documents.

Forbidden: oracle/reference trees, real providers/credentials/Keychain/user sessions/config, automatic permission decisions, schedules/workflows, installed-binary changes, or mutation-capable evaluator tools.

Runtime state affected: the current session's typed goal state only after explicit `/goal`; process-only evaluator tasks/usage; ordinary session JSON under injected temporary stores during tests/walkthroughs.

## Design and Invariants

- `GoalState` is a defaulted Session V4 field with stable ID, bounded condition, typed status/stop reason, explicit limits, iteration/token/working-time counters, last bounded evaluation, and timestamps. Old sessions decode unchanged; historical branches omit goal state while full forks clone it with a fresh goal ID and source link.
- Default limits are 12 continuation iterations, 100,000 combined provider tokens, and 30 working minutes. Config may explicitly raise hard defaults within absolute safety caps; `/goal ... --iterations/--tokens/--minutes` may only lower the resolved defaults for that run.
- `/goal <condition>` starts one goal only while idle. `/goal status`, `/goal pause`, `/goal resume`, and `/goal clear` are explicit; replacing an unfinished goal requires clearing it first. Status is viewable while busy, but mutations never race an evaluator or active provider/tool task.
- A completed plain response never self-certifies the goal. The main loop snapshots only the bounded goal condition, last response, current typed plan/tasks, recorded work events, validation outcomes, iteration/usage/time counters, and a sanitized workspace label into a fresh evaluator request with no executable tools.
- The evaluator must return exactly one typed `report_goal_evaluation` call with `continue`, `complete`, `unreachable`, or `blocked`, a bounded rationale, and bounded continuation feedback/evidence gaps. It receives no chat history, skills, memory, MCP, hooks, images, endpoint, credentials, or mutation tools.
- A `continue` result increments the evaluated iteration, charges exact reported provider tokens to the goal, stores the bounded evaluation, and starts a request-only continuation instruction. The instruction never impersonates the user or mutates canonical messages; the durable goal state supplies restart context.
- Before evaluation or continuation, enforce limits in a fixed order: user/approval pause, iteration, combined provider-token usage, then elapsed working time. Limit terminal states are `usage_limited` or `budget_limited`; evaluator terminal states are `complete`, `unreachable`, or `blocked`.
- Opening an approval/interaction, receiving user steering/input, cancellation/interruption, or explicit pause moves an active goal to `paused` with a typed safe reason. It never auto-resumes after the blocking condition clears; `/goal resume` is required.
- Goal evaluation is one generation-bound background task. Cancellation, clear, new session load, branch/rewind, or stale generation discards its result. Shutdown aborts the task. Evaluation failures pause with a retryable safe message rather than claiming completion or consuming another continuation.
- A normal goal continuation remains inside all existing tool selection, approval, sandbox, hook, fallback, checkpoint, queue, and session boundaries. Goal mode grants no new tool or network authority.

## Milestones

### M1 — Durable state, limits, and commands

- Add failing tests for old-session defaults, full/historical fork behavior, config bounds/defaults, per-run lowering, condition/result bounds, pause/resume/clear transitions, and deterministic limit ordering.
- Add `/goal` parsing/discovery/status controls and a process-safe status surface.

### M2 — Fresh evaluator and accounting

- Add an isolated evaluator with only the non-executable reporting schema, plus loopback fixtures for typed outcomes, malformed/ordinary-text/multiple-report failures, no-history/no-executable-tool requests, usage charging, timeout/cancellation, and secret/binary exclusion.
- Add `GoalEvaluation` usage/request kinds without changing ordinary request bytes or `/usage` semantics.

### M3 — Main-loop continuation and safety pauses

- Route completed plain responses through generation-bound evaluation, inject continuation feedback request-only, and preserve canonical/session messages.
- Pause on approval, interaction, user input/steering, evaluator failure, interruption, and explicit cancellation; never drain restored/queued user input autonomously.

### M4 — Rendering, walkthrough, and gates

- Add wide/narrow status snapshots for active/evaluating/paused/complete/unreachable/blocked/usage-limited/budget-limited states.
- Run focused/full/format/snapshot/strict-Clippy/release/smoke gates and an isolated released-TUI loopback walkthrough that proves one continuation, evidence-grounded completion, persistence/resume, and Cancel-safe pause behavior.

## Acceptance Criteria

- Old V1–V4 sessions load with no active goal; a new goal survives save/resume with exact counters/status and no schema migration rewrite before a normal save.
- Per-run limits cannot exceed config-resolved defaults; config and absolute caps reject zero/overflow values before provider traffic.
- Evaluator requests are fresh, bounded, evidence-grounded, expose no executable tools, and contain no canonical history, memory, image bytes, credentials, endpoints, or raw unbounded outputs.
- Main assistant text alone cannot complete a goal; only a valid evaluator result can produce evaluator terminal states.
- Continue starts no more than 12 default continuations and stops before exceeding 100,000 combined reported provider tokens or 30 working minutes.
- Any approval, interaction, submitted user input, interrupt, or evaluator failure pauses without auto-resume; existing approval and `/retry` semantics remain intact.
- Clear/cancel/session load invalidates stale evaluator results and no evaluator task survives shutdown.
- Goal state is visible and durable but does not enter provider authority, Keychain, snapshots beyond safe bounded text, or unrelated sessions/profiles.

## Validation

Focused: `cargo test goal --no-fail-fast` passed 19 matching unit tests with no matching CLI tests.

Completion: `cargo test --no-fail-fast` passed 810 unit and 28 CLI integration tests; `cargo fmt --check`, strict Clippy, release build, snapshot hygiene, and help/version/profile smoke tests passed.

Manual/visual: 14 goal-state snapshots were reviewed at 100x30 and 60x24. A released-TUI credentialless loopback walkthrough completed one autonomous continuation, resumed the completed goal with exact counters, and separately proved explicit pause of an in-flight turn preserves its 11 reported tokens without starting evaluation.

Blocked checks: none.

## Progress

- [x] Completion, session, evidence, usage, approval, and fresh-review seams characterized.
- [x] Durable state/config/command tests and implementation complete.
- [x] Fresh evaluator and typed usage/request boundaries complete.
- [x] Main-loop continuation, pause, generation, and persistence paths complete.
- [x] Snapshots, repository-wide gates, and isolated walkthrough pass.
- [x] Harness reconciled and plan archived.

## Decisions

- Decision: evaluation is a separate prepared request with no executable tools and exactly one typed reporting call.
  Evidence: ordinary assistant prose is not evidence and `review.rs` already proves isolated typed reporting.
  Consequence: evaluator failure is distinguishable from goal failure and cannot mutate the workspace.
- Decision: continuation feedback is request-only, not a synthetic canonical user message.
  Evidence: canonical messages are user/assistant/tool history and must not misattribute autonomous control text to the user.
  Consequence: durable `GoalState` carries evaluator context across saves while provider requests receive one explicit continuation instruction.
- Decision: pause requires explicit resume after any human/approval boundary.
  Evidence: autonomous continuation must never race or override user intent.
  Consequence: clearing an approval or submitting input does not silently restart goal work.
- Decision: keep Session V4 and add a defaulted field.
  Evidence: the roadmap permits defaulted V4 fields incrementally and existing readers already tolerate them.
  Consequence: no eager migration or new storage backend is introduced.

## Surprises and Discoveries

- `finalize_completed_plain_response` currently drains queued input immediately after save/checkpoint scheduling; goal evaluation must take precedence, and any queued/restored user input must pause autonomy instead of being consumed.
- Existing `token_count` covers main streaming only, while `UsageLedger` covers every request category. Goal budgets therefore need an explicit per-goal counter updated from the exact main/evaluator spans rather than the display tuple.
- An explicit pause can happen while a provider request is still in flight. Usage ownership is fixed when that request starts, so its reported tokens still charge the paused goal; later ordinary user turns do not.
- A reporting tool schema is protocol framing, not evaluator authority. Tests assert that the evaluator receives only `report_goal_evaluation` and never receives an executable built-in, MCP, memory, image, hook, or approval surface.

## Recovery and Rollback

Remove goal command/state/evaluator orchestration and leave the defaulted Session V4 field ignored by older code. An active goal can always be cleared without deleting messages, work evidence, or sessions. Tests and walkthroughs use disposable session/config roots.

## Completion Record

Changed files: `src/goal.rs`, goal/session/config/request/usage state and orchestration in `src/{main,app,config,session,request,usage}.rs`, slash-command/input/render integration in `src/tui/`, focused tests and snapshots, and the required harness documents.

Commands and results: focused goal tests passed 19 matches; the repository completed 810 unit tests and 28 CLI tests; formatting, strict Clippy, release build, help/version/profile smoke, snapshot hygiene, and released-TUI isolated walkthroughs all passed.

Remaining risks: evaluator conclusions depend on bounded recorded evidence and can be conservative; a provider that omits usage cannot contribute exact token counts; elapsed working time is process wall time and restored active goals deliberately reopen paused.

Harness updates: `ARCHITECTURE.md`, `CONTINUITY.md`, `TESTING.md`, `NEXT_STEPS.md`, `PARITY.md`, `ENVIRONMENT.md`, and `docs/.INDEX.md` now describe the implemented boundary and route the next handoff to WFL-070.
