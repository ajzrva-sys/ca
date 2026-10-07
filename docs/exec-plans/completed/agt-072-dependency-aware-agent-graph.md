# AGT-072 — Dependency-Aware Agent Graph

Status: COMPLETE (2026-08-12)
Owner/session: Codex `/root`
Last updated: 2026-08-12

## Goal

Extend approved typed plan tasks into a bounded, durable agent graph. Each graph task may declare dependency edges, exact child grants, one existing profile-owned worktree, an attempt limit, and local evidence gates. The scheduler starts only ready tasks within the existing `AgentManager` concurrency/children limits, records safe attempt state in Session V4, and never releases descendants of a failed task until the user explicitly confirms a retry.

## Non-Goals

- Automatic worktree creation, branch integration/deletion, merging, commits, pushes, or cleanup. WT-071 remains the sole worktree lifecycle owner.
- General workflow DAG replacement, arbitrary executable graph files, dynamic task generation after approval, nested scheduler ownership, best-of-N, fusion, or multi-run judging; WFL-070 and RUN-073 remain distinct.
- Automatic retry, continuing past failed dependencies, granting network implicitly, bypassing persistent permission denial/approval, or treating a child summary as proof without the configured local gate.
- Cursor parity. Oracle graph/worktree ideas informed the priority; no clean Cursor runtime fixture establishes this behavior.

## Evidence and Current Behavior

- `WorkState` already persists approved revisioned `PlanTask`s plus recorded evidence, but tasks have no dependency/grant/attempt/scheduler fields and `/tasks` is a static pager (`src/work.rs`, `src/main.rs`).
- `AgentManager` already enforces configured concurrency, per-parent-turn child bounds, tool grants, workspace authority, timeout/cancellation, approval waits, and immutable spawn-time workspaces (`src/agent/subagent.rs`). It should remain the execution owner; the graph must schedule through it rather than create another provider/tool runner.
- `WorktreeStore::attach_context` validates exact profile ownership and Git identity without changing the main workspace. This is the safe task-local worktree lookup seam; task records persist only the worktree UUID.
- WFL-070 has bounded DAG validation and deterministic ready-node logic, but workflow definitions are local untrusted files with request-only prompts. Agent graphs are user-approved plan state and must not import workflow authority or serialize arbitrary outputs.
- Session V4 already serializes `WorkState` and safe `AgentSummary` values. New fields must be defaulted so V1-V4 and existing exported/imported histories remain backward-readable; portability continues to strip runtime graph/agent state.

## Scope

Allowed files: a new graph module; defaulted work/task/session types; exact plan-tool schemas and validation; agent completion evidence; app/orchestration/input/render state; worktree lookup integration; focused fake-runner/temp-Git tests and deterministic snapshots; required harness documents.

Forbidden: oracle/reference edits, real configuration/Keychain/sessions/repositories, branch/worktree mutation, provider credentials/endpoints in graph state, automatic approval, external network, installed-binary mutation, and any scheduler path that executes a node whose dependencies are not locally proven successful.

Runtime state affected: approved graph metadata and bounded attempt summaries in the current profile-scoped session; child agents/jobs already owned by `AgentManager`; existing owned worktrees are read/modified only through the exact grants of the task assigned to them. No graph starts before explicit user approval/start.

## Design and Invariants

### Typed graph state

- Add a bounded `AgentTaskDefinition` to each `PlanTask`, defaulting absent. It contains normalized unique `depends_on: Vec<TaskId>`, an exact `AgentGrant`, optional `worktree_id: Uuid`, `max_attempts` in 1–3, and nonempty evidence gates. Titles remain the child prompt; no hidden prompt/output is persisted.
- Add defaulted durable scheduler state with `Idle`, `Running`, `Paused`, `Completed`, `Failed`, `Cancelled`, plus bounded per-task attempts. An attempt stores only number, child `AgentId`, typed terminal status, timestamps, successful tool names, and a bounded sanitized summary/error. It never stores tool arguments/results, provider requests, endpoints, credentials, approval content, or workspace paths.
- Graph configuration validates at most 64 tasks, exact plan task IDs, no duplicate/self/unknown edges, acyclicity, exact available tools, non-widening path/command/network grants, unique worktree ownership among graph tasks, attempt/evidence bounds, and an approved plan revision before start. Validation is all-or-nothing under the existing CAS reducer.
- Existing string-only `propose_plan` remains compatible. Add a plan-mode-only typed graph configuration mutation after task IDs exist; the user-visible approval preview includes dependencies, grants, worktree labels/short IDs, attempts, and evidence. Model changes cannot approve/start/retry a graph.

### Scheduling and authority

- `/tasks` becomes a read-only-by-default graph manager. It exposes Start/Pause/Cancel and task detail; failed tasks offer Cancel-first Retry. Start is explicit after plan approval, so approving an ordinary plan retains current main-agent behavior and does not unexpectedly spend on children.
- One pure scheduler computes states from durable task/attempt data. A task is ready only when every dependency is completed with its configured gates satisfied, it has no live attempt, and it has not exhausted/failed awaiting user retry. Ready order is stable plan order.
- The TUI scheduler asks `AgentManager` to spawn ready tasks up to existing limits. The manager remains authoritative for concurrency and per-turn child counts; a failed spawn is a typed failed attempt and stops descendants. Scheduler polling is event/tick driven and never blocks the TUI.
- Each child receives its task title, exact task ID, approved-plan summary, configured grant, and either the current immutable workspace snapshot or the exact verified context of its unique owned worktree. Assigning a worktree never attaches the main session. Missing/mismatched worktree state fails that task before spawn and sends no provider traffic.
- Persistent policy, sandbox, tool selection, network, hooks, child depth/runtime/job ownership, and approval waits remain enforced by existing child boundaries. A graph grant narrows; it never authorizes by itself.

### Completion, failure, retry, restore

- The child runner emits typed successful-tool observations into `AgentSnapshot`. Evidence supports at least nonempty final assistant output and exact successful tool name. All configured gates must pass before the scheduler records a successful `WorkEvent`, links it to the task, and marks the task completed.
- Failed, timed-out, cancelled, interrupted, denied, missing-worktree, spawn, or evidence-gate outcomes mark the task failed/blocked and pause graph advancement. Pending descendants remain visibly dependency-blocked rather than running or being rewritten.
- Retry is user-only, Cancel-first, and creates a new immutable attempt if below `max_attempts`; it never overwrites the failed attempt. The user may cancel the whole graph, but cancellation does not roll back child changes, remove worktrees, delete branches, or automatically retry.
- Session save occurs on every graph transition. Restored `Running`/live attempts become `Interrupted` and the graph becomes `Paused`; nothing auto-resumes or auto-spawns at startup. Full forks receive fresh plan/task/graph IDs with internally remapped dependencies and empty runtime attempts; historical forks omit work state as today.
- Portable export/import strips graph runtime state with the rest of `WorkState`; existing sessions with no graph fields render and behave exactly as ordinary plans.

## Milestones

### M1 — Durable graph model and validation

- Add failing tests for defaulted compatibility, graph CAS, bounds, duplicate/self/unknown/cyclic edges, grants, unique worktree ownership, evidence/attempt policy, fork remapping, and restore interruption.
- Add the plan-mode graph configuration schema/handler without starting children.

### M2 — Scheduler and child evidence

- Add pure ready/blocked/terminal scheduling tests and typed child tool-success evidence.
- Integrate exact current/worktree workspace preparation, bounded spawning, terminal observation, evidence linking, failure stops, explicit retry, pause/cancel, and session persistence.

### M3 — TUI manager and orchestration

- Replace static `/tasks` with searchable/list-detail graph state, start/pause/cancel/retry actions, Cancel-first confirmations, busy locking, actionable dependency/failure text, and worktree/grant summaries without paths or secrets.
- Add wide/narrow empty/ordinary-plan/ready/running/waiting-approval/failed/dependency-blocked/retry/busy/completed/restored snapshots.

### M4 — Gates and isolated walkthrough

- Run focused/full/format/strict-Clippy/release/smoke gates.
- Exercise a released TUI with a disposable home/repository, fake runner or loopback provider, and two owned worktrees: prove parallel independent nodes stay within concurrency, a dependent node waits, one failure blocks descendants until explicit retry, successful retry releases it, exact worktree markers remain isolated, restore does not auto-resume, and no branch/tree is cleaned.

## Acceptance Criteria

- Invalid graphs fail atomically before child/provider activity; approved ordinary plans remain backward-compatible and inert unless explicitly started as graphs.
- Only dependency-ready nodes spawn, in stable order and within existing manager limits. Failed dependencies stop every descendant until explicit user retry succeeds.
- Every child receives only exact configured tools/resources/network plus the correct immutable current or owned-worktree workspace. No main-session attach or authority widening occurs.
- Durable attempts are append-only, bounded, secret-free, and restore as interrupted/paused without automatic provider traffic. Full forks remap edges and clear runtime ownership safely.
- Task completion requires local evidence gates, records a successful event, and keeps existing `CompletionWithoutEvidence` protection. Failure/cancel never claims completion.
- `/tasks` makes graph status, dependencies, worktree ownership, attempts, evidence, and required user actions visible at wide/narrow sizes.
- WT-071 branch/worktree preservation, WFL-070 workflow semantics, manual `spawn_agent`, sessions, approvals, sandbox, tools, and ordinary chat remain unchanged outside an active graph.

## Validation

Focused: PASS across `agent_graph`, `work`, graph orchestration, subagent evidence/capacity, input, and render suites. Coverage includes atomic graph validation, stable ready scheduling, missing-worktree zero-spawn preflight, exact owned-worktree child roots, dependency/evidence failure, append-only retry, cancellation, restore interruption, full-fork remapping, input locking, and ordinary-plan compatibility.

Completion: PASS — `cargo test --no-fail-fast` (869 unit tests plus 28 CLI integration tests), `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo build --release`, and released help/version/named-profile/bare-profile temporary-home smokes.

Manual/visual: PASS — fourteen reviewed wide/narrow empty/ordinary/ready/running/busy/retry/completed snapshots. A released TUI under a disposable home/repository and credentialless loopback provider configured a three-node graph over two owned worktrees, started both roots concurrently, kept the dependent blocked after the second root failed its required `read_file` gate, then recorded a second attempt after explicit Cancel-first retry. That attempt read only `SECOND_TREE_MARKER` from the assigned worktree and released the dependent; the durable graph completed with summaries `FIRST_WORKTREE_OK`, `SECOND_WORKTREE_OK`, and `DEPENDENCY_RELEASED_OK`. Temporary state was moved to Trash.

Blocked checks: none.

## Progress

- [x] Work/task/agent/worktree/workflow seams characterized and safety decisions recorded.
- [x] Durable graph model, validation, schemas, and fork/restore compatibility complete.
- [x] Scheduler, exact workspace/grant preparation, evidence, stop/retry, and persistence complete.
- [x] `/tasks` manager/orchestration/render surfaces complete.
- [x] Repository-wide gates and released disposable walkthrough pass.
- [x] Harness reconciled and plan archived.

## Decisions

- Decision: explicit graph Start after plan approval.
  Evidence: existing approval sends the main agent an execution instruction; silently converting approval into multiple paid child requests would change ordinary plan behavior and weaken cost control.
  Consequence: ordinary plans stay compatible, while graph plans become autonomous only through a visible user action.
- Decision: schedule through `AgentManager`, not a second runner.
  Evidence: it already owns concurrency, child limits, approval waits, timeout/cancellation, job cleanup, and restrictive tool selection.
  Consequence: the graph is a durable reducer/orchestrator and cannot bypass established child authority.
- Decision: assign only existing owned worktrees and never attach the main session.
  Evidence: WT-071 intentionally makes worktree creation/cleanup explicit and stores validated ownership in one profile registry.
  Consequence: graph tasks can isolate changes without gaining lifecycle authority or moving unrelated main work.
- Decision: explicit failed-task retry, no automatic retries.
  Evidence: child failures can follow partial filesystem mutations and the roadmap forbids downstream execution without a retry decision.
  Consequence: the graph pauses on every failed/evidence-incomplete node; attempts remain auditable and descendants stay blocked.

## Surprises and Discoveries

- `PlanTask` already owns durable task/evidence identity and `AgentSpawnRequest` already has an optional task ID, so AGT-072 can join existing boundaries without serializing provider output into the plan.
- `AgentManager` queues beyond its semaphore but limits children per parent turn. The graph must account for both before spawn rather than treating a queued child as free scheduler capacity.
- Worktree attachment currently swaps main process state, but `WorktreeStore::attach_context` returns an independently validated `WorkspaceContext`; task-local use can avoid all main-session attachment side effects.
- Current agent events are display strings. Evidence gates require one new typed successful-tool channel instead of parsing those strings.
- The real Plan-mode TUI finishes proposal generation in an explicit approval interaction. Esc correctly rejects it; the walkthrough used `/plan approve` after exercising that rejection path, then `/tasks` Start remained a second independent Cancel-first action.

## Recovery and Rollback

Defaulted graph fields can be ignored while retaining ordinary `PlanTask` data. Cancelling/rolling back the feature never removes child modifications, owned worktrees, or branches. Existing running children remain under `AgentManager` and are cancelled only through existing explicit/shutdown behavior.

## Completion Record

Changed files: `src/agent_graph.rs`, `src/work.rs`, `src/agent/{registry,tools,subagent}.rs`, `src/{runtime,app,main}.rs`, `src/tui/{commands,input,render}.rs`, graph and ordinary-plan snapshots, plus this plan and the required harness documents.

Commands and results: focused graph/work/subagent/input/render suites PASS; full tests PASS (869 + 28); formatting PASS; strict Clippy PASS; release build PASS; help/version/profile smokes PASS; released disposable TUI walkthrough PASS.

Remaining risks: local evidence proves exact successful tool names and nonempty assistant output, not semantic correctness of arbitrary child work. Graphs intentionally require explicit start and retry, use existing worktrees only, and never integrate branches; RUN-073 must preserve those boundaries when comparing candidates.

Harness updates: `ARCHITECTURE.md`, `CONTINUITY.md`, `TESTING.md`, `NEXT_STEPS.md`, `PARITY.md`, and `docs/.INDEX.md` now describe the graph boundary, validation evidence, extension status, and RUN-073 handoff.
