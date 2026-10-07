# MCP-064 — Deferred MCP Tool Discovery

Status: COMPLETE
Owner/session: current Codex task
Last updated: 2026-08-11

## Goal

Keep ordinary prepared requests from starting MCP servers while giving the model one bounded `search_mcp_tools` tool that explicitly discovers eligible servers, activates only exact matching schemas process-locally, and makes those schemas available on the next continuation with correct `/context` and `/request` accounting.

## Non-Goals

- Persisting activated schemas, indexing remote catalogs on disk, semantic embeddings, fuzzy network search, or automatic startup based on prompt text.
- Changing project trust, distinct network grants, OAuth, approval policy, tool-name namespaces, resource authority, or MCP transport behavior.
- Giving review, checkpoint, side-question, bare, or provider-probe runners MCP discovery.
- Claiming Cursor parity; this is an MCP-ecosystem-informed `ca` extension.

## Evidence and Current Behavior

- `src/mcp.rs::McpManager::catalog` starts every enabled definition before returning a combined registry.
- Interactive request preparation in `src/main.rs`, ordinary headless turns in `src/headless.rs`, and headless repair turns call `catalog()` before resolving tools. Therefore a normal request starts every eligible MCP server even when no MCP tool is needed.
- `ToolSelectionPolicy` already partitions built-in and MCP definitions after final filtering, and prepared context/request snapshots already count the exact resolved partitions.
- `RunningServer::stale_tools` receives MCP list-change notifications and `ensure_started` refreshes a stale running catalog, but no process-only activated subset exists.
- Subagents currently use built-ins only; `/context` preview deliberately does not start MCP.

## Scope

Allowed files: `src/mcp.rs`, `src/agent/registry.rs`, `src/agent/tools.rs` only if dispatch alignment requires it, `src/tool_selection.rs`, `src/main.rs`, `src/headless.rs`, `src/app.rs` only if visible state requires it, `src/tui/**`, focused tests/snapshots, this plan, and harness docs.

Forbidden files/areas: `oracles/**`, `reference/**`, provider protocol/config/session formats, real config/Keychain/sessions, installed binary, unrelated formatting.

Runtime state affected: process-local running MCP services and exact activated tool-name sets only. Tests and walkthroughs use fake stdio/loopback servers and temporary homes.

## Design and Invariants

- Add a read-only main-profile built-in `search_mcp_tools { query, server?, limit? }`. Query and optional exact server ID are bounded; result limit defaults to 5 and is capped at 10. The tool is absent whenever MCP capability is disabled, including bare.
- Ordinary request resolution calls a no-start active-catalog method. Before any search it contains built-ins plus the search tool and zero remote schemas; `/context` and `/request` therefore count zero MCP schemas.
- Explicit search considers only enabled, trusted definitions with satisfied network authority. An exact `server` searches only that server. Without it, server-ID matches are preferred; otherwise at most eight eligible servers are started in sorted order with concurrency capped at four. Skipped/ineligible/failing servers produce bounded typed summaries, never raw stderr, endpoints, headers, challenges, or bodies.
- Search matches case-insensitive query tokens against safe server ID, exposed tool name, and bounded description. It activates at most the requested exact exposed names. Activation is atomic: schema/case collisions or invalid schemas leave the prior active set unchanged.
- Activated names persist only for the manager lifetime. The next provider continuation advertises the exact active schemas through the central resolver; unavailable calls still fail before execution and dynamic tools retain existing approval/network policy.
- A tool-list change refreshes only the already-running affected server before the next active-catalog read, then removes activated names no longer advertised. Reconnect, disconnect, failed refresh, and server close invalidate only that server's activations. Shutdown clears all state.
- Explicit `/mcp` status/reconnect may still start or refresh servers because the user requested MCP management; normal interactive/headless request preparation must not call the full catalog.

## Milestones

### M1 — Characterize eager startup and active-catalog state

- Add failing MCP tests proving an ordinary active-catalog read makes zero fixture requests/process starts and contains no dynamic schema before search.
- Add exact activation, bounds, ineligible-server, collision, and catalog-change invalidation tests.
- Add tool schema/selection tests proving `search_mcp_tools` is main-only and disappears when MCP capability is disabled.

### M2 — Bounded search and activation

- Add process-only activated state, no-start active registry assembly, bounded eligible startup/search/ranking, atomic activation, safe result formatting, and affected-server invalidation.
- Preserve the existing explicit full-catalog path for `/mcp` management and compatibility tests.

### M3 — Runner integration and visible behavior

- Route interactive, ordinary headless, and repair request preparation through the active catalog.
- Dispatch search through the same validation, hooks, tool result, and headless event paths as other built-ins without granting remote execution authority.
- Prove the next continuation advertises activated schemas and exact context/request snapshots move tokens from zero to the active MCP partition.
- Add reviewed wide/narrow snapshots for successful activation and safe no-match/failure output.

### M4 — Completion and isolated walkthrough

- Run focused suites and every repository gate.
- Exercise a released binary with a temporary home plus fake MCP/provider: first request sees only built-ins/search, search starts one selected fixture, next request sees only its activated schema, and no real state is touched.
- Reconcile harness docs and archive this plan before LSP-065.

## Acceptance Criteria

- A normal request with configured MCP servers performs zero MCP start/discovery I/O until `search_mcp_tools` executes or the user invokes explicit `/mcp` management.
- Search never starts disabled, untrusted, network-ungranted, or out-of-bound servers; exact-server search cannot widen authority.
- At most ten exact matching schemas activate, remain process-only, retain namespaces/validation/approval, and appear only from the following provider request onward.
- Catalog changes remove stale activated schemas before another request can advertise or execute them; unrelated activations remain.
- Built-in definitions remain visible and stable; bare/review/checkpoint/side-question contexts cannot see or call search or dynamic MCP tools.
- `/context` and `/request` report the exact active MCP schema estimate, not all configured/discovered tools.
- Debug, status, tool output, snapshots, sessions, and failures contain no endpoints, headers, credentials, OAuth parameters, response bodies, raw stderr secrets, or credential lengths.

## Validation

Focused: PASS. MCP active/full catalog, bounded search, eligibility, exact activation, context partitioning, list-change invalidation, registry/selection, headless continuation, and TUI render suites pass.

Completion: PASS. `cargo test --no-fail-fast` passed 739 unit tests plus 28 CLI tests; `cargo fmt --check`, snapshot hygiene, strict Clippy, release build, and help/version/profile smoke all passed.

Manual/visual: PASS. Successful/failed search and active-count wide/narrow snapshots were reviewed. A released three-request headless run under a disposable home proved search-only first request, exact remote schema on the second, safe results on the third, MCP cleanup, unchanged config hash, and absent profile/session state.

Blocked checks: none.

## Progress

- [x] Eager catalog call sites, stale-catalog signal, runner tool resolution, approval dispatch, and context/request accounting traced.
- [x] Characterization tests fail for eager startup and missing search/activation state.
- [x] Bounded search, active catalog, and invalidation tests pass.
- [x] Interactive/headless continuation and visible snapshot tests pass.
- [x] Repository gates and isolated released walkthrough pass.
- [x] Harness reconciled and plan archived.

## Decisions

- Decision: discovery is a model-visible read-only built-in rather than prompt-triggered orchestration.
  Evidence: explicit tool calls preserve prepared-request provenance, validation, hooks, events, cancellation, and deterministic tests.
  Consequence: the model can decide when discovery cost is justified, while users can remove the tool through central selection.
- Decision: no-server-hint search starts at most eight eligible servers, preferring IDs matched by the query; exact `server` is the scalable path.
  Evidence: remote tool names/descriptions are unknowable before discovery, but unbounded all-server startup would recreate the current defect.
  Consequence: very large registries may require a second exact-server search and receive an honest omission hint.
- Decision: existing `/mcp` management keeps an explicit full-catalog path.
  Evidence: `/mcp` is a direct user request to inspect/manage servers, unlike ordinary provider request preparation.
  Consequence: status behavior remains useful without making chat eager.

## Surprises and Discoveries

- The dynamic MCP path is currently main-profile only and subagent execution is built-in-only, so this slice does not need to propagate a remote manager into child runners.
- The first characterization compile failed exactly because `McpManager` had neither `active_catalog` nor `search_tools`; the existing `catalog` suggestion would preserve eager startup and is therefore not an acceptable substitution.

## Recovery and Rollback

Keep the original explicit `catalog()` implementation while adding a separate active-catalog path, so rollback is localized to runner call sites and the added process-only set. Use `apply_patch`; do not reset the worktree or touch runtime/oracle data.

## Completion Record

Changed files: `src/mcp.rs`, `src/agent/registry.rs`, `src/agent/tools.rs`, `src/tool_selection.rs`, `src/main.rs`, `src/headless.rs`, `src/agent/subagent.rs`, `src/test.rs`, `tests/cli.rs`, `src/tui/render_tests.rs`, six snapshots, this plan, and harness documentation.

Commands and results: characterization failed at missing `active_catalog`/`search_tools`; focused MCP/registry/selection/headless/CLI/snapshot suites PASS; `cargo test --no-fail-fast` PASS (739 unit + 28 CLI); format/snapshot hygiene PASS; strict Clippy PASS; release PASS; help/version/profile smoke PASS; released isolated walkthrough PASS.

Remaining risks: lexical search cannot rank an undiscovered tool description without starting its server. Unhinted search therefore inspects at most eight eligible servers and asks the model to retry with an exact server ID when more exist. Third-party catalog-change behavior beyond the local protocol fixtures remains unverified.

Harness updates: `ARCHITECTURE.md`, `CONTINUITY.md`, `ENVIRONMENT.md`, `NEXT_STEPS.md`, `PARITY.md`, `TESTING.md`, and `docs/.INDEX.md` now own deferred discovery and the LSP-065 handoff.
