# AGT-144 — Automatic Safe Agent Launches

Status: COMPLETE
Owner/session: Codex / current implementation session
Last updated: 2026-08-19 (completed)

## Goal

Make ordinary read-only conversational `spawn_agent` calls automatic, inherit
the active provider/model without accidental cost routing, resolve runtime
Codex OAuth before child traffic, and collapse identical spawn failures in one
tool batch into one expandable transcript block.

## Non-Goals

- Do not change pricing cards or make ChatGPT subscription routes eligible for
  metered inter-provider delegation.
- Do not auto-approve `task`, writes, shell commands, network tools, external
  communication, or nested agents.
- Do not change persisted configuration/session schemas, install a binary, or
  mutate provider/runtime state.
- Do not inspect, copy, or modify oracle/reference implementations.

## Evidence and Current Behavior

- `src/agent/registry.rs` declares `spawn_agent` mutating, while
  `permission::auto_review_safe_invocation` admits only statically read-only
  tools, so a bounded read-only child still prompts.
- The `complexity` fields have no model-facing explanation. Supplying one
  invokes `DelegationRouter::select`; the current local metadata has eligible
  routes only through standard complexity, so complex calls fail before spawn.
- `refresh_agent_routing` builds route clients directly from loaded provider
  config. Ordinary Codex turns resolve runtime OAuth separately, leaving an
  inherited child client without that runtime credential.
- Four contiguous failed `spawn_agent` calls render as four ordinary tool
  blocks even when their terminal output is identical.
- Reproduction was metadata-only: `ca --doctor` reported Codex OAuth routing
  ineligible, DeepSeek/MiMo eligible through standard, and direct OpenAI
  complex routing ineligible. No credential contents or provider traffic were
  used.

## Scope

Allowed files: agent registry/client/subagent code, permission/orchestration
code, TUI render/tests/snapshots, and current project documentation.

Forbidden files/areas: `oracles/`, `reference/`, user configuration/session or
credential stores, installed binaries, provider price data, and Git history.

Runtime state affected: none during implementation and offline validation.

## Design and Invariants

- AutoReview may admit `spawn_agent` only when `tools` is nonempty, every tool
  is registered and statically read-only, no tool has Network,
  ExternalCommunication, or Subagent capability, and `commands` is absent or
  empty. Persistent Deny/Ask and strict Allowlist keep existing precedence.
- Omitted complexity inherits the parent route. Model-facing tool guidance says
  to omit it unless the user explicitly requests inter-provider cost routing.
  Explicit selection remains fail-closed without fallback.
- Agent route entries retain enough provider configuration to call the
  canonical runtime resolver asynchronously at child start, then construct a
  transient client. Runtime tokens stay out of requests, snapshots, sessions,
  errors, Debug, and route-selection metadata. Concurrent resolution is
  serialized.
- Only contiguous completed `spawn_agent` errors with identical output group.
  The first call ID owns the expandable block; every underlying call/result
  remains ordered and protocol-visible.

## Milestones

### M1 — Characterization and approval/routing repair

- Add focused failing approval and schema/prompt tests.
- Implement argument-sensitive safe-spawn AutoReview classification.
- Add model-facing inherited-versus-explicit complexity guidance.

### M2 — Runtime credential resolution

- Add an injectable async route/provider resolver.
- Resolve inherited and selected route configuration before constructing the
  child client; cover success, failure, bypass, serialization, and redaction
  with fake/loopback tests only.

### M3 — Duplicate failure presentation

- Add a contiguous identical-spawn-failure grouping model and wide/narrow
  snapshots.
- Prove active/success/distinct/noncontiguous calls remain separate and stored
  call results are unchanged.

### M4 — Completion

- Run focused suites and the repository completion gates.
- Reconcile `CONTINUITY.md`, `NEXT_STEPS.md`, `TESTING.md`, architecture and
  delegation documentation, then move this plan to completed.

## Acceptance Criteria

- A read-only, child-network-free `spawn_agent` call in AutoReview does not
  create an approval prompt; unsafe grants and all strict-policy cases do.
- A normal spawn omits complexity and inherits the current route; an explicit
  unavailable complexity returns a deterministic failure without fallback.
- A Codex child obtains a valid runtime credential before its first provider
  request, while resolution failure produces no provider request or usage.
- Four contiguous identical spawn errors render as one collapsed block and
  expand to four ordered calls; protocol/session state retains four results.
- No credential, runtime config, installed binary, or oracle/reference file is
  read for content or changed.

## Validation

Focused: permission, registry/schema, subagent/router, fake OAuth/loopback,
orchestration, and TUI render/snapshot tests.

Completion: `cargo test --no-fail-fast`, `cargo test --all-targets
--all-features --no-fail-fast`, `cargo fmt --all --check`, `cargo clippy
--all-targets --all-features -- -D warnings`, `cargo build --release
--all-features`, help/version, `git diff --check`, snapshot hygiene, and
oracle/reference diff checks.

Manual/visual: review deterministic wide/narrow snapshots. No live OAuth or
paid provider call is authorized.

Blocked checks: native provider-backed TUI execution if it would require user
runtime state or credentials.

## Progress

- [x] Current source, local metadata-only routing status, and approval path characterized.
- [x] Add failing focused characterizations.
- [x] Implement safe spawn approval and routing guidance.
- [x] Implement runtime credential resolution.
- [x] Implement grouped failure presentation.
- [x] Run completion gates and reconcile documentation.

## Decisions

- Decision: explicit complexity remains fail-closed.
  Evidence: user selected the existing cost-authority boundary.
  Consequence: only omitted complexity inherits ChatGPT/Codex.
- Decision: automatic approval is limited to read-only `spawn_agent` under
  AutoReview.
  Evidence: user selected the narrow dynamic-grant boundary.
  Consequence: `task`, strict Allowlist, and unsafe grants still prompt.
- Decision: duplicate suppression is presentation-only.
  Evidence: provider protocols require one result for each call ID.
  Consequence: one card may represent several retained tool results.

## Surprises and Discoveries

- A concurrency fixture initially constructed its client with the test-only
  `unknown` provider identity, so the Codex-only resolver correctly bypassed
  it. The fixture now supplies the `openai-codex` identity explicitly.
- Storing prebuilt route clients would have repaired OAuth but not satisfied
  the start-time construction contract. Route tables now store provider
  specifications and request sinks; transient clients are constructed only
  after any required runtime resolution.

## Recovery and Rollback

Revert only AGT-144 hunks with a targeted patch. Do not reset the dirty
worktree or discard unrelated user changes. New snapshots and the active plan
can be removed individually if the slice is abandoned.

## Completion Record

Changed files: `src/permission.rs`, `src/agent/{client,registry,subagent}.rs`,
`src/main.rs`, `src/tui/{render,render_tests}.rs`, four new spawn-failure
snapshots, and the AGT-144 architecture/delegation/harness documents.

Commands and results: focused permission (11), subagent (16), routing/OAuth,
and render (157) suites pass. Default completion passes with 1,619 main and 37
CLI tests; all-target/all-feature completion passes with 1,648 main, 118
experiment, and 37 CLI tests. Formatting, strict Clippy, all-feature release,
local help/version, diff/snapshot hygiene, and oracle/reference checks pass.

Remaining risks: no live OAuth/provider request or native TUI session was run
because the accepted plan limits validation to fake/loopback providers and
forbids runtime-state mutation. The canonical resolver and deterministic
wide/narrow snapshots are covered hermetically.

Harness updates: `CONTINUITY.md`, `NEXT_STEPS.md`, `TESTING.md`,
`ARCHITECTURE.md`, `docs/.INDEX.md`, and
`docs/INTER_PROVIDER_DELEGATION.md` record the completed contracts and evidence.
