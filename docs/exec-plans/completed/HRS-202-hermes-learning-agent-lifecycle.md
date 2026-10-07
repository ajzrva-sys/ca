# HRS-202 — Hermes-Informed Learning and Agent Lifecycle

Status: COMPLETE
Owner/session: Codex / current workspace
Last updated: 2026-08-28

## Goal

Add the pertinent Hermes learning and delegation behavior that CA does not
already have: explicit relationships between portable skills, a bounded
metadata-only learning graph, lossless child steering, bounded child waits,
and digest-verifiable terminal handoffs.

## Non-Goals

- No oracle modification or source copying, GUI, plugin lifecycle API, new
  provider route, broader child authority, automatic skill mutation/archive,
  transcript retention, credential access, runtime-profile mutation, binary
  installation, or live provider request.
- No replacement of CA's existing skill ledger, agent IDs, session summaries,
  approvals, immutable workspace snapshots, exact tool grants, or cancellation
  ownership.
- No claim of Cursor parity. Hermes is behavioral evidence for CA extensions.

## Evidence and Current Behavior

- `src/learning_journey.rs` already records digest-bound skill invocation,
  completion, failure, correction, mutation, and bundle events. It already
  derives refinement, staleness, and repeated-bundle suggestions, so a second
  usage sidecar or automatic curator would duplicate shipped behavior.
- `src/skills.rs::SkillInstanceMetadata` has no category or declared related
  skills, and `/learning` has no relationship view.
- `src/agent/subagent.rs::AgentManager::send_message` queues an untracked
  string. `await_child_approval` consumes and discards messages while approval
  is pending, and a message accepted during terminalization can disappear
  without a delivery or missed-steer receipt.
- `await_agent` waits without a caller bound. Terminal summaries have no stable
  digest that a parent can use as a visible handoff identity.
- Read-only Hermes evidence in `oracles/hermes-agent-main/agent/learning_graph.py`
  exposes declared skill relationships alongside usage, while
  `tools/delegate_tool.py` linearizes steering completion and names undelivered
  accepted steering. `agent/subagent_lifecycle.py` provides bounded waits and
  immutable result hashes. These are observations only; the implementation
  remains native to CA's Rust types and constraints.

## Scope

Allowed files: `src/skills.rs`, `src/learning_journey.rs`,
`src/agent/{registry,subagent}.rs`, `src/{main}.rs`,
`src/tui/commands.rs`, focused tests/snapshots if required, this plan, and the
repository architecture/continuity/testing/index/next-step documents.

Forbidden files/areas: `oracles/`, `reference/`, credentials, user profile or
session roots, installed binaries, provider state, unrelated dirty-tree hunks,
and public network services.

Runtime state affected: shipped interactive profiles may append the same
metadata-only skill events they already record. The graph is derived at read
time. Agent steering receipts and result digests are process-local. Validation
uses temporary fixtures only.

## Design and Invariants

- Portable frontmatter may declare bounded `category` and `related_skills`
  either at top level or under `metadata.hermes`. Values are descriptive only:
  they cannot activate a skill, grant a tool, install a dependency, or resolve
  ambiguity implicitly.
- `/learning graph` derives current-digest usage/outcomes, declared links, and
  repeated successful co-use links from the existing registry and ledger. It
  stores no new content, stays bounded, and names unresolved relationships.
- Every accepted child steer receives an opaque process-local ID and is tracked
  until the provider runner inserts it at the next request boundary. Messages
  arriving during approval are retained. Completion and send acceptance share
  the manager lock, so a steer is either rejected, marked delivered, or exposed
  as bounded sanitized `missed_steer`; it is never silently lost.
- Child wait accepts an optional bounded timeout and returns the current
  snapshot when the bound expires. Existing no-timeout behavior remains.
- A terminal result digest is SHA-256 over the final sanitized bounded summary.
  It is immutable after completion and is presentation metadata, not a secret,
  authority token, or replacement for the result.
- Exact grants, parent/depth/turn limits, cancellation, approval, workspace
  snapshots, output bounds, and session schema remain unchanged.

## Milestones

### M1 — Characterization and contracts

- Add failing tests for portable relationship parsing/validation and a bounded
  graph built from current-digest usage plus declared and observed links.
- Add failing tests proving steering during approval is delivered later, a
  terminal race cannot silently absorb accepted text, bounded waits return a
  live snapshot, and terminal hashes are stable.

### M2 — Skill relationships and learning graph

- Extend portable metadata with bounded category/relationship declarations.
- Add `/learning graph` rendering without adding persistence or mutation.

### M3 — Lossless steering and terminal handoff

- Track steering acceptance/delivery under the manager lifecycle and preserve
  approval-time messages until the next child request boundary.
- Add optional bounded await behavior and stable terminal result digests to the
  existing model-facing tools and snapshots.

### M4 — Completion

- Run focused skill, learning, registry, and subagent suites.
- Run the repository completion gates, reconcile durable documents, and move
  this plan to completed only when acceptance criteria pass.

## Acceptance Criteria

- A valid portable skill exposes normalized category and relationship metadata;
  oversized, duplicate, self-referential, or malformed declarations fail
  closed without changing authority.
- `/learning graph` deterministically shows bounded current skill usage and
  deduplicated declared/observed edges, ignores stale-digest attribution, and
  names unresolved declared targets without reading skill bodies.
- Steering accepted while a child awaits approval is supplied on its next
  provider request. Any accepted steering that misses the final boundary is
  retained in the terminal snapshot; steering a terminal child is rejected.
- `await_agent` without a timeout keeps its existing behavior; a valid timeout
  returns the latest nonterminal snapshot, and invalid bounds fail safely.
- Completed and failed children expose a stable SHA-256 of their sanitized,
  retained terminal result, and repeated reads do not change it.
- Focused suites and every applicable completion gate pass with no oracle,
  runtime profile, credential, provider, or installed-binary mutation.

## Validation

Focused:

- `cargo test skills::tests --lib --no-fail-fast`
- `cargo test learning_journey::tests --lib --no-fail-fast`
- `cargo test agent::subagent --lib --no-fail-fast`
- `cargo test agent::registry --lib --no-fail-fast`

Completion:

- `cargo test --workspace --all-features --no-fail-fast -- --test-threads=1`
- `cargo test --workspace --no-default-features --no-fail-fast -- --test-threads=1`
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo build --workspace --release --all-features`
- release `--help` and `--version` smoke tests
- `git diff --check`, zero `.snap.new`, and protected-tree verification

Manual/visual: textual command output and deterministic tests only; no live TUI
or provider is required.

Blocked checks: report unavailable environmental checks rather than using real
credentials, profiles, sessions, or provider traffic.

## Progress

- [x] Inventory CA and read-only Hermes skill, learning, delegation, steering,
  and lifecycle behavior; remove features CA already has from scope.
- [x] Add failing relationship/graph and lifecycle tests.
- [x] Implement skill relationships and the metadata-only graph.
- [x] Implement lossless steering, bounded waits, and result digests.
- [x] Pass focused and completion gates and reconcile the harness.

## Decisions

- Decision: extend the existing lifecycle ledger instead of adding a Hermes-like
  usage sidecar or autonomous curator.
  Evidence: CA already attributes exact invocations/outcomes and derives the
  pertinent deterministic maintenance thresholds.
  Consequence: one metadata authority remains, and user-authored skills are
  never silently archived or rewritten.
- Decision: copy no Hermes prompts or implementation; reproduce only observed
  contracts in CA's existing manager and registry.
  Evidence: repository oracle rules and CA's stricter authority model.
  Consequence: names, types, limits, and persistence remain CA-native.
- Decision: expose missed steering rather than attempting a hidden extra turn.
  Evidence: a child that has crossed its final request boundary cannot consume
  new input truthfully.
  Consequence: the parent can explicitly respawn or continue with the retained
  instruction.

## Surprises and Discoveries

- CA's shipped learning journey already covers usage, correction, staleness,
  and repeated-skill-set maintenance, substantially narrowing the useful
  Hermes learning delta.
- Approval-time child messages are currently consumed by the approval loop and
  only rendered as activity, which is a concrete silent-loss path rather than
  merely a missing convenience.
- A bounded accepted-steer queue and terminalization must share the manager
  lock. Tracking only the channel send would still leave a completion race in
  which the runner exits before observing an accepted message.
- Existing structured child snapshots are already the correct public handoff
  boundary. Adding a parallel Hermes output-schema subsystem would duplicate
  CA's typed status/result/error surface and broaden this change unnecessarily.

## Recovery and Rollback

Reverse only HRS-202 hunks with `apply_patch`; do not reset the dirty worktree.
The new metadata fields use serde defaults and create no migration. Validation
artifacts stay under Cargo target or temporary directories. No runtime state or
binary installation needs recovery.

## Completion Record

Changed files:

- `src/skills.rs` adds bounded non-authorizing category and related-skill
  metadata to portable skills and catalogs.
- `src/learning_journey.rs` derives the bounded metadata-only relationship
  graph from current definitions and the existing ledger.
- `src/agent/subagent.rs` adds acceptance-tracked steering, approval retention,
  terminal missed-steer receipts, bounded waits, and stable result digests.
- `src/agent/registry.rs`, `src/main.rs`, and `src/tui/commands.rs` expose the
  optional wait and graph command through the existing surfaces.
- `ARCHITECTURE.md`, `CONTINUITY.md`, `TESTING.md`, `NEXT_STEPS.md`, and
  `docs/.INDEX.md` record the durable behavior and actual validation.

Commands and results:

- Focused skill, journey, subagent, registry, and TUI command suites passed 26,
  7, 20, 11, and 48 tests respectively.
- The all-feature workspace matrix passed CA 2,149/4 ignored, the reasoning
  experiment 118/1 ignored, and every CLI, watcher, codex-team, integration,
  protocol, and doc-test target.
- The no-default workspace matrix's first attempt exposed one timing-sensitive
  failure in an unchanged codex-team five-client stdio stress test. The exact
  case passed alone, then the complete matrix passed cleanly with CA 2,120/4
  ignored and every remaining target green; no code changed for the retry.
- Formatting, strict workspace all-target/all-feature Clippy, optimized
  all-feature release, release help/version, strict signature, diff and
  snapshot hygiene, direct protected-tree checks, and the Rust-owned
  protected-tree verifier all passed.

Remaining risks:

- Relationships are descriptive declarations and successful co-use evidence,
  not semantic dependency proof. The bounded graph intentionally requires
  human/model interpretation and grants no authority.
- Steering receipts, missed messages, wait snapshots, and terminal hashes are
  process-local. Persisting or replaying them would require a separate session
  schema and privacy review.
- Hermes' separate structured child-output schema was not added because CA's
  existing typed child snapshots already carry status, bounded result/error,
  lifecycle events, and authority identity; adding parallel machinery was not
  pertinent to this scoped delta.

Harness updates: the plan is completed, HRS-202 is complete in
`NEXT_STEPS.md`, and architecture, continuity, testing, and the knowledge index
describe the implemented boundary. `oracles/` and `reference/` are unchanged.
