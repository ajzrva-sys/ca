# AGT-151 — App-Configured Delegation Routing

Status: COMPLETE
Owner/session: Codex `/root`
Last updated: 2026-08-21

## Goal

Let a person choose the default delegated-agent provider/model inside `ca`.
The backward-compatible default is “Same as current agent”; an explicit route
may select any configured provider/model and is used by `task` and
`spawn_agent` when they omit a complexity override.

## Non-Goals

- No automatic fallback, provider probing, credential migration, new provider
  protocol, background launch, or change to child tool/path/network authority.
- No per-task or per-agent-name routing matrix in v1.
- No modification of `codex-team`, user credentials, installed binaries,
  sessions, `oracles/`, or `reference/`.

## Evidence and Current Behavior

- `AgentManager` already stores the active parent route. An omitted child
  `complexity` inherits it; explicit complexity invokes the existing priced
  `DelegationRouter` and fails closed when no route qualifies.
- All configured providers already have child route clients. Cross-provider
  child execution and runtime credential resolution have focused tests.
- `/config` is the searchable in-app settings hub; the current model picker
  already presents configured provider/model routes.
- `ConfigStore` owns comment-preserving, atomic profile writes.

## Scope

Allowed files:

- `src/config.rs`
- `src/agent/{subagent,registry}.rs`
- `src/{app,main}.rs`
- `src/tui/{commands,input,render,render_tests}.rs` and focused snapshots
- `docs/INTER_PROVIDER_DELEGATION.md`, harness docs, and this ExecPlan

Forbidden files/areas: provider wire adapters, secret storage, approval/sandbox
policy, oracle/reference trees, installed binaries, and real runtime profiles.

Runtime state affected: only the explicitly selected profile's `config.toml`
after the user presses Enter. Bare mode remains read-only.

## Design and Invariants

- Add a serde-default delegation policy containing either no route (inherit) or
  one exact configured provider/model. Existing configs deserialize unchanged.
- Validate both-or-neither fields and require an exact configured model.
- `AgentManager` resolves an omitted child route as explicit default first,
  otherwise current parent. Explicit complexity remains higher precedence.
- Nested children continue inheriting their immediate parent's already-selected
  route.
- `/config` shows “Delegated agent”; Enter opens the existing model picker in a
  delegation mode with a first-class “Same as current agent” row.
- Saving is atomic and updates the live manager only after reload succeeds.
- Provider deletion resets a matching delegated route to inherit; model rename
  updates an exact matching delegated route.

## Milestones

### M1 — Typed persistence and routing

- Add failing config migration/validation/save tests.
- Add child-route precedence tests for inherit, explicit default, and explicit
  complexity.

### M2 — In-app settings

- Add the settings entry, delegation model picker mode, bare-mode behavior,
  async save, and live manager refresh.
- Add wide/narrow render and input tests.

### M3 — Completion

- Run focused tests, full default/all-feature suites, formatting, strict
  Clippy, release, snapshot hygiene, and protected-tree checks.
- Reconcile harness docs and create one clean review commit.

## Acceptance Criteria

- Existing profiles delegate to the current agent exactly as before.
- A saved route sends an omitted-complexity child to that exact provider/model.
- Explicit complexity still invokes automatic routing and does not silently
  fall back to the saved default or parent.
- The setting can be changed back to Same as current agent in the app.
- Invalid/stale routes fail during config load or save before provider traffic.
- Child grants, sandbox, network denial, cancellation, and result contracts are
  unchanged.

## Validation

Focused: PASS — config persistence/migration/drift, runtime precedence, picker
input, settings hub, and wide/narrow render coverage.

Completion: PASS — default and all-target/all-feature test matrices, strict
Clippy, formatting, all-feature release, help/version, diff, snapshot hygiene,
and protected-tree checks.

Manual/visual: reviewed deterministic 100x30 and 60x24 delegated-agent picker
snapshots, including the inherit row and cross-provider rows.

Blocked checks: no live provider traffic is required or authorized.

## Progress

- [x] Existing config, settings, route-client, and child routing boundaries inspected.
- [x] Typed delegation policy and atomic persistence implemented.
- [x] Runtime child routing wired and characterized.
- [x] Settings picker implemented and snapshots reviewed.
- [x] Completion gates and harness reconciliation recorded.

## Decisions

- Decision: saved default applies only when `complexity` is omitted.
  Evidence: explicit complexity already means a deliberate priced-route request.
  Consequence: user intent has one deterministic precedence order and no hidden
  fallback.
- Decision: reuse the configured-model picker rather than create a provider-only
  editor.
  Evidence: model identity is required by the child route and the picker already
  spans providers.
  Consequence: provider setup and delegation remain separate concerns.

## Surprises and Discoveries

- Cross-provider delegation already works; the missing product surface is a
  persistent default and app settings control.

## Recovery and Rollback

Remove the optional delegation block or select Same as current agent. This
restores exact legacy inheritance without migrating sessions or task state.

## Completion Record

Changed files: `src/config.rs`, `src/agent/{subagent,registry}.rs`, `src/{app,main}.rs`,
`src/tui/{commands,input,render,render_tests}.rs`, two new picker snapshots, two
updated settings-hub snapshots, and harness records.

Commands and results: `cargo test --no-fail-fast` (1,720 unit pass, 2 ignored;
37 CLI pass; 17 role-run pass); all-target/all-feature matrix (1,749 main pass,
2 ignored; 118 experiment pass, 1 ignored; 37 CLI pass; 17 role-run pass);
strict Clippy, formatting, all-feature release, help/version, diff, snapshot,
and protected-tree gates all pass.

Remaining risks: the default is one route for all top-level delegated work;
per-agent-name and per-task policies remain intentionally deferred. No live
cross-provider request was needed for this settings/routing change.

Harness updates: `CONTINUITY.md`, `ARCHITECTURE.md`, `TESTING.md`,
`NEXT_STEPS.md`, and `docs/.INDEX.md` reconciled.
