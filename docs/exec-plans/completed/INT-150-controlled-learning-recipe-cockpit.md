# INT-150 — Controlled Learning Recipe Cockpit

Status: COMPLETE
Owner/session: Codex `/root`
Last updated: 2026-08-21

## Goal

Add a model-free, Plan-styled `ca` review surface for `codex-team` learned Rust
recipe proposals, with exact MCP approval/rejection calls and visible local
activation/quarantine state.

## Non-Goals

- No model request, background recipe execution, catalog persistence in `ca`,
  provider credential access, live provider traffic, or synchronization.
- No change to Plan-mode tool authority or generic MCP model-tool activation.
- No edits to `oracles/`, `reference/`, installed binaries, or real profiles.

## Evidence and Current Behavior

- `src/mcp.rs::McpManager` already owns bounded SDK calls but exposes them to
  model-tool dispatch by activated names; there is no exact host-only call API.
- `src/main.rs` already runs asynchronous MCP host work and renders result pages.
- `AgentMode::Plan`, host interactions, the Mimocode palette, Markdown pager,
  clipboard toast, and deterministic wide/narrow render tests already exist.
- The orchestrator exposes strict `recipe_list`, `recipe_preview`,
  `recipe_approve`, `recipe_reject`, `recipe_reactivate`, and `recipe_retire`
  MCP tools; mutations require `human_confirmed=true`.

## Scope

Allowed files:

- `src/{app,main,mcp}.rs`
- `src/tui/{commands,input,render,render_tests}.rs` and focused snapshots
- harness documentation and this ExecPlan

Forbidden files/areas:

- provider protocols, permission widening, generic model-visible MCP activation,
  sessions/config credentials, `oracles/`, and `reference/`

Runtime state affected: none beyond the explicit remote orchestrator MCP action
chosen by the human. `ca` stores no recipe catalog copy.

## Design and Invariants

- `/recipes [server] [recipe-id]`, `/plan recipes`, and `/mcp recipes` reach one
  host-rendered review path. The default server ID is `codex-team`.
- `McpManager` gets a bounded exact server/tool call that returns structured JSON
  without activating a model-visible tool or making a model request.
- Proposal details render as sanitized Markdown in the existing Plan-themed
  pager; a host single-choice interaction offers Approve globally, Reject, Later.
- Approve/reject arguments come only from the preview receipt and always bind the
  exact ID, digest, and repository revision. `human_confirmed=true` is added only
  after the corresponding local human action.
- Active, drifted, quarantined, and retired summaries are visible in the same
  orchestration page. Copy continues through the existing bounded toast.

## Milestones

### M1 — Exact host MCP boundary

- Add a server-pinned structured tool call and tests proving no activation.

### M2 — Plan-style review lifecycle

- Add typed process-only review/request state, async loading/action handling,
  command routing, and host confirmation.

### M3 — Rendering and validation

- Add wide/narrow snapshots, model-zero-call assertions, focused tests, and
  completion gates.

## Acceptance Criteria

- A proposal is readable without a provider request and shows commands,
  evidence, predicates, isolation, limits, risks, and global reach.
- Approve globally and Reject call the exact MCP tools only after a human choice;
  Later makes no MCP mutation call.
- Status includes active/quarantined entries and the review is reachable from
  Plan and MCP/orchestration commands.
- Narrow and wide Mimocode snapshots remain bounded and copy feedback is intact.

## Validation

Focused: PASS — four learned-recipe state/action tests and the exact host MCP
nonactivation test.

Completion: PASS — the default suite passes 1,715 unit tests with two
documented ignores, 37 CLI tests, and 17 role-run CLI tests. The final
all-target/all-feature suite passes 1,744 main tests with two ignores, 118
experiment tests with one ignore, 37 CLI tests, and 17 role-run CLI tests.
Strict Clippy, formatting, all-feature release, help/version, snapshots, and
whitespace checks pass.

Manual/visual: deterministic 100×30 and 60×24 snapshots reviewed.

Cross-process: PASS — the real `ca::McpManager` discovers and calls the final
orchestrator observe/operate recipe tools using disposable state. A supervised
production-isolation-backed global activation remains pending; no live provider
or real-profile mutation is authorized.

## Progress

- [x] Existing MCP, Plan, interaction, pager, theme, and toast boundaries inspected.
- [x] Exact host MCP structured call implemented and tested.
- [x] Review lifecycle and command routing implemented and tested.
- [x] Wide/narrow snapshots reviewed.
- [x] Completion checks and harness reconciliation recorded.

## Decisions

- Decision: reuse the existing Markdown pager plus host interaction instead of
  adding another modal framework.
  Evidence: both already provide the requested readability, Plan identity, and
  model-free choice handling.
  Consequence: the feature stays small and inherits tested copy/theme behavior.

## Surprises and Discoveries

- The requested Mimocode palette, copy toast, Plan identity, and structured
  Markdown readability were already completed by TUI-149.

## Recovery and Rollback

Remove the additive command/state/stream variant and exact host MCP method.
No local catalog or profile migration is created by this feature.

## Completion Record

Changed files: `src/{app,main,mcp}.rs`, `src/tui/{commands,render,render_tests}.rs`,
two deterministic snapshots, `NEXT_STEPS.md`, and this plan.

Commands and results: focused and completion matrices above pass, including the
ignored-by-default real orchestrator MCP compatibility test.

Remaining risks: the cockpit is pilot-only until the orchestrator completes a
clean supervised global-activation pilot with its production isolation backend.

Harness updates: `NEXT_STEPS.md`, `CONTINUITY.md`, `TESTING.md`, and
`ARCHITECTURE.md` record the final model-free boundary and pilot disposition.
