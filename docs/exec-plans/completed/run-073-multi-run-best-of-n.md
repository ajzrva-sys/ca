# RUN-073 — Multi-Run and Best-of-N

Status: COMPLETE
Owner/session: Codex `/root`
Last updated: 2026-08-12

## Goal

Add an explicit `/multi-run` workflow that freezes one user request, runs two candidates by default and at most five across exact provider/model routes in separate profile-owned Git worktrees, then offers a read-only evidence/diff judgment and a separate fusion proposal. Candidate worktrees and branches remain user-owned; `ca` never silently attaches, merges, commits, deletes, cleans, or changes the current/default model.

## Non-Goals

- Automatic best-of-N for ordinary chat, implicit retries/fallbacks, model routing, background benchmark suites, arbitrary candidate counts, remote repositories, or non-Git workspaces.
- Merging, cherry-picking, rebasing, committing, pushing, branch deletion, worktree cleanup, automatic attachment, or applying a judge/fusion decision. WT-071 remains the sole lifecycle owner.
- Treating model prose as sufficient evidence. The judge receives exact bounded local diffs/status plus safe candidate summaries and successful-tool names; incomplete evidence remains visible.
- Session-format changes or persistence of frozen prompts, candidate output, judgments, or fusion prose. RUN-073 state is process-only; created worktrees remain durable in the existing profile manifest.
- Cursor parity. Multi-run ideas are oracle-informed `ca` product design without a clean Cursor runtime fixture.

## Evidence and Current Behavior

- `AgentManager` already owns child concurrency, per-parent limits, cancellation, approval waits, immutable workspace snapshots, tool grants, job cleanup, and safe child summaries (`src/agent/subagent.rs`). AGT-072 schedules through this boundary and proves exact owned-worktree roots; RUN-073 must not create a parallel authority path.
- `WorktreeStore` can create, inspect, diff, and attach profile-owned worktrees with verified Git identity while preserving branches and refusing automatic cleanup (`src/worktree.rs`). Multi-run may create worktrees only after explicit confirmation and must leave every resulting record to `/worktree`.
- `UsageLedger` is process-only and records exact safe provider/model IDs, token subsets, duration, outcome, and optional configured exact-decimal pricing (`src/usage.rs`). Candidate, judge, and fusion requests need distinct usage kinds and must keep unknown pricing visibly unknown.
- `PreparedModelRequest` and `RequestStore` already provide canonical request construction and bounded semantic snapshots without endpoints, headers, or credentials (`src/request.rs`). All new provider calls must use this same path.
- `ProviderAgentRunner` currently owns one startup `AgentClient` and one default usage recorder. Different candidate routes therefore require a safe request-local route selector inside the existing runner/manager boundary; manager/app state may carry only provider/model IDs.
- Session V4 persists plans, graphs, workflows, worktrees, and agents, but not general ephemeral side work. A process-only multi-run state avoids storing prompts/diffs/results and preserves current compatibility.

## Scope

Allowed files: a new bounded multi-run module; safe request-local child route/usage fields; runtime capabilities; app/command/input/render/orchestration state; worktree/usage/request integration; hermetic fake-runner/loopback/temp-Git tests and deterministic wide/narrow snapshots; required harness documents.

Forbidden: oracle/reference edits, real configuration/Keychain/sessions/repositories, external network, provider endpoints/credentials in app/actions/events/snapshots/debug/session state, automatic tool/network authority, automatic worktree integration/cleanup, installed-binary mutation, and any judge/fusion executable tool access.

Runtime state affected: process-only multi-run definitions, progress, safe candidate summaries, judgment, and fusion proposal; explicitly created profile-owned worktrees/branches; normal process-only usage/request ledgers. No canonical conversation, session file, current/default model, main workspace attachment, or user config changes.

## Design and Invariants

### Definition, preview, and authority

- Add `/multi-run [2-5] [request]` with `/best-of` as an alias. With no request it opens a composer-backed setup manager; omitted count defaults to two. Starting is idle-only, requires a Git workspace and mutable profile worktree capability, and is always Cancel-first.
- Freeze one bounded request and the completed conversation context at preview time. Candidate definitions contain stable local IDs, exact safe `provider/model` pairs, and planned worktree labels only. The preview shows each route, candidate count, isolation/branch preservation, central tool set, network policy, and a clearly estimated request cost. Unknown configured pricing remains `unknown`; estimates never claim to include unpredictable tool continuations.
- Default routes place the current pair first, then unique available pairs sorted by provider/model. If fewer distinct pairs exist, candidates may share a route but never a worktree. A candidate route can be changed through the existing model picker without selecting or persisting the global model.
- Confirmation creates exactly one WT-071-owned worktree per candidate from the same verified base. A partial creation failure stops before provider work, preserves and lists already-created worktrees, and offers no automatic rollback or branch deletion.
- Multi-run grants are exact centrally selected subagent coding tools, scoped to each immutable candidate worktree, with no implicit network and no nested agent/workflow/worktree lifecycle tools. Existing persistent policy, approvals, sandboxing, hooks, process limits, and job ownership remain authoritative.

### Candidate execution

- Extend `AgentSpawnRequest` with defaulted request-only safe route and usage-kind fields. `ProviderAgentRunner` resolves an exact configured route only inside the background runner, constructs the canonical client/request there, and has a custom secret-free debug boundary. Ordinary/manual/graph agents without a route retain byte-for-byte current behavior.
- Schedule candidates through the existing `AgentManager` under its configured concurrency and child limits using one stable synthetic parent turn. Each candidate receives the exact same frozen request/context and its own immutable worktree root. Invalid/deleted routes, worktree identity failure, spawn failure, denial, timeout, cancellation, and incomplete responses become typed safe candidate outcomes without affecting peers or defaults.
- The manager exposes existing approval/interaction states per candidate. The multi-run detail can approve/deny/answer only the selected waiting candidate using current safety controls; no decision is inferred or shared across candidates.
- Candidate state stores only safe route IDs, worktree UUID, timestamps/status, successful tool names, and bounded sanitized final summary/error. Tool arguments/results, raw diffs, full model output, credentials, endpoints, and prompts do not enter application state or snapshots.
- Cancel stops live child requests/jobs through `AgentManager` but leaves every filesystem mutation, worktree, and branch untouched and visible for inspection.

### Evidence, judge, and fusion

- When all candidates are terminal, refresh exact WT-071 status/diff evidence on a blocking worker. Evidence is bounded per candidate, includes omitted-byte/file markers, and is invalidated when the worktree changes. Candidate summaries are never treated as a substitute for the local snapshot.
- `Shift+J` opens a separate Cancel-first judge preview with route and cost estimate. The judge is a fresh-context read-only provider request with only one non-executable structured report schema. It receives the original request plus bounded candidate IDs/routes, summaries, successful-tool names, and exact local evidence hashes/diffs. It cannot call built-in/MCP/LSP/workflow/agent/worktree tools.
- The validated judgment selects one existing candidate, records bounded rationale and evidence references process-only, and marks itself stale if any candidate evidence hash changes. Invalid/malformed judge output remains a typed failure and never selects implicitly.
- `Shift+F` is separately explicit and available only from a fresh valid judgment. Fusion is another fresh-context read-only structured request that may reference multiple candidates, but returns only a proposed source worktree ID and bounded integration plan. It cannot modify/merge/copy files or attach the main workspace. Applying changes remains a deliberate `/worktree attach` plus ordinary user/model work.
- Candidate, judge, and fusion calls use distinct `UsageKind`s and canonical request snapshots. The ledger never stores response text or local diffs; the multi-run UI reports measured costs and unknown-price cases honestly.

### Lifecycle and compatibility

- Only one multi-run may be active. Closing/reopening the manager retains process-only state; application shutdown cancels active candidates and loses the transient UI state while preserving worktrees/branches.
- Provider/model edits or catalog refreshes do not rewrite a frozen run. A missing route fails that candidate safely before network traffic. Main chat, `/retry`, goals, workflows, graphs, subagents, provider setup, and defaults remain unchanged outside an explicitly started multi-run.
- Bare and read-only profiles may inspect no previous process state and cannot start multi-run because worktree creation is persistent. Headless/scripted modes gain no implicit execution surface in this slice.

## Milestones

### M1 — Safe definition and route-aware runner characterization

- Add failing tests for count/request bounds, deterministic route defaults, cost estimates, duplicate routes with unique worktrees, process-only serialization boundary, and bare/non-Git refusal.
- Add default-preserving request-local agent route/usage support with exact provider/model resolution and zero-provider-traffic invalid-route coverage.

### M2 — Isolated candidate orchestration

- Add temporary-Git/fake-runner tests for Cancel-first preview, all worktrees sharing one base but unique paths/branches, partial-create preservation, same frozen request, exact roots, shared concurrency, candidate approval/cancel/failure independence, and no canonical/session/default mutation.
- Implement process-only state/reducer, worktree creation, manager scheduling/observation, safe summaries, cancellation, and evidence refresh/staleness.

### M3 — Read-only judge, fusion proposal, and TUI

- Add schema/local-validation tests proving judge/fusion have zero executable tools, cannot name unknown candidates/worktrees, fail closed on malformed output, and become stale after evidence change.
- Add searchable list/detail/setup, route selection, Cancel-first Start/Judge/Fuse confirmations, waiting-approval controls, busy locking, cost/usage/evidence/stale/error displays, and wide/narrow empty/setup/running/waiting/failure/cancelled/ready-to-judge/judged/stale/fusion snapshots.

### M4 — Gates and isolated released walkthrough

- Run focused/full/format/strict-Clippy/release/smoke gates.
- Under a disposable home/repository and credentialless loopback providers, start three candidates on at least two routes, prove distinct exact roots and bounded concurrency, produce different local diffs, exercise one failure or approval wait without stopping peers, run a read-only judgment, create a fusion proposal, and verify the main tree/default/session remain unchanged while all candidate branches/worktrees survive. Move temporary state to Trash afterward.

## Acceptance Criteria

- Two candidates are the default and five is the hard maximum. Invalid definitions, profiles, routes, or workspaces fail before worktree/provider activity.
- Explicit confirmation creates unique verified profile-owned worktrees from one base. Every partial/success/cancel path preserves worktrees and branches and never attaches, merges, commits, deletes, or cleans automatically.
- Candidates receive the same frozen request/context, exact selected route, exact immutable worktree, central narrowed tools, and no implicit network; scheduling stays within existing manager limits.
- Multi-run state and UI are secret-free and process-only. Canonical messages, Session V4, current/default model, config, and main workspace remain byte-for-byte unaffected.
- Local diff/status evidence is bounded, hashed, omission-aware, and refreshed before judgment. The judge and fusion requests are fresh-context, read-only, structurally validated, tool-free except their non-executable report schemas, and stale on worktree changes.
- Judgment selects only an existing candidate. Fusion returns only a proposed existing source worktree plus an integration plan; neither operation applies changes.
- All visible setup/progress/approval/error/cancel/judge/fusion states are actionable and legible at wide and narrow terminal sizes, with honest measured/estimated/unknown cost labels.
- Ordinary chat, manual agents, AGT-072, workflows, goals, WT-071, provider setup, sessions, profiles, approvals, hooks, tools, and usage/request inspectors remain compatible when no multi-run is active.

## Validation

Focused:

- `cargo test multi_run --no-fail-fast`: PASS, 16 matching tests covering bounded definitions, deterministic routes/costs, process-only state, command/input safety, route-local provider execution, missing-route zero traffic, temporary-Git candidate isolation, structured judge/fusion requests, usage/request ownership, and rendering.
- `cargo test provider_runner_records_each_subagent_request --no-fail-fast`: PASS.
- `cargo test missing_request_local_route --no-fail-fast`: PASS.
- Reviewed 14 multi-run setup/confirmation/running/failure/cancel/ready/judged/fusion snapshots at 100x30 and 60x24. Four usage snapshots changed intentionally for the new candidate/judge/fusion usage kinds; zero `.snap.new` files remain.

Completion:

- `cargo test --no-fail-fast`: PASS, 886 unit tests plus 28 CLI integration tests.
- `cargo fmt --check`: PASS.
- `cargo clippy --all-targets -- -D warnings`: PASS.
- `cargo build --release`: PASS.
- Released `--version`, `--help`, named-profile list-models, and bare list-models smokes: PASS under a disposable home; missing configs remained unwritten.

Manual/visual:

- Released 100x30 TUI walkthrough: PASS under a disposable home and Git repository with two credentialless loopback routes. Three candidates used three distinct owned worktrees from one HEAD; candidate 1 wrote `ALPHA CANDIDATE`, candidate 2 wrote `BETA CANDIDATE`, and candidate 3 failed on malformed SSE without stopping its peers.
- The walkthrough reached ready-to-judge, selected candidate 2 through the sole `report_multi_run_judgment` schema, then produced a source-2 integration proposal through the sole `report_multi_run_fusion` schema. The main checkout and config hashes were unchanged, `--no-session` created no session, Authorization was absent, and all worktrees/branches survived.

Blocked checks: none.

## Progress

- [x] Provider/agent/worktree/usage/request/session seams characterized and safety decisions recorded.
- [x] Safe definition, route/cost preview, and request-local route runner complete.
- [x] Candidate worktree creation/orchestration/evidence complete.
- [x] Read-only judge/fusion and TUI surfaces complete.
- [x] Repository-wide gates and released disposable walkthrough pass.
- [x] Harness reconciled and plan archived.

## Decisions

- Decision: process-only run state, durable user-owned worktrees.
  Evidence: prompts, model output, and diffs are unnecessary session authority, while WT-071 intentionally preserves filesystem results through a separate profile manifest.
  Consequence: reopening during one process works; restarting loses comparison prose but never loses candidate branches/worktrees.
- Decision: reuse `AgentManager` through a safe request-local route selector.
  Evidence: it is the proven owner of concurrency, approvals, cancellation, jobs, grants, immutable workspaces, and graph scheduling; parallel private runners would bypass global limits and interaction handling.
  Consequence: route resolution must occur only in the background runner, and all existing spawn callers keep the current default route.
- Decision: separate explicit Start, Judge, and Fuse actions.
  Evidence: each stage creates cost and the latter stages make subjective model decisions; autonomy is disabled by default throughout the roadmap.
  Consequence: candidate completion does not spend on a judge, and a judgment never triggers fusion or filesystem changes automatically.
- Decision: fusion is a plan over one existing proposed source worktree, not a merge.
  Evidence: safe automatic integration needs conflict handling and branch authority outside RUN-073, and the roadmap explicitly forbids silent merges.
  Consequence: the user inspects/attaches a worktree and applies any integration deliberately through existing boundaries.

## Surprises and Discoveries

- `ProviderAgentRunner` is route-fixed even though the manager already has every other required multi-candidate lifecycle boundary. One defaulted request-local route field is smaller and safer than duplicating the runner.
- Child approval/interaction states already live in `AgentManager`; multi-run must expose those selected-child controls rather than creating a second approval protocol.
- Exact-decimal pricing can estimate a bounded first request, but tool continuations are unknowable before execution. The preview must label this rather than presenting false totals.
- Ratatui's released alternate-screen output is incremental, so the first disposable Expect harness could not match one transient contiguous approval label even though the run reached its terminal state. The clean rerun asserted stable stage labels and verified actual requests/worktrees afterward; candidate 3's malformed-stream failure supplied the required peer-failure exercise.

## Recovery and Rollback

Remove the `/multi-run` surfaces and ignore process-only state. Defaulted route/usage fields retain existing agent behavior when absent. Any created worktrees/branches remain intentionally registered and user-owned for `/worktree` inspection/cleanup; rollback must never delete them automatically.

## Completion Record

Changed files:

- `src/multi_run.rs`, `src/main.rs`, `src/app.rs`, and `src/runtime.rs`: process-only definitions/state, orchestration, exact route construction, worktree creation, evidence, judge/fusion tasks, cancellation, capabilities, and shutdown.
- `src/agent/subagent.rs`: default-preserving safe request-local provider/model and usage-kind routing through the existing `AgentManager`.
- `src/usage.rs` and `src/request.rs`: distinct candidate/judge/fusion ownership.
- `src/tui/commands.rs`, `src/tui/input.rs`, `src/tui/render.rs`, `src/tui/render_tests.rs`, and snapshots: command, setup/route selection, confirmation/progress/action states, status surface, input locking, and deterministic wide/narrow coverage.
- Required harness documents and this ExecPlan.

Commands and results: all focused, repository-wide, format, strict-lint, release, smoke, snapshot, and released-walkthrough gates above passed.

Remaining risks:

- Multi-run comparison state is intentionally process-only; after restart users inspect retained candidate branches/worktrees through `/worktree`, but judgment/fusion prose is not restored.
- Cost preview covers configured deterministic request estimates; unbounded future tool continuations remain honestly unknown until measured.
- Evidence is bounded and omission-aware, so a user should inspect the chosen worktree before deliberate integration.

Harness updates: `ARCHITECTURE.md`, `CONTINUITY.md`, `TESTING.md`, `NEXT_STEPS.md`, and `PARITY.md` record the completed `ca` extension and leave SKL-074 as the next active capability.
