# HRS-203 — Hermes Delegation and Learning Completion

Status: COMPLETE
Owner/session: current Codex task
Last updated: 2026-08-28

## Goal

Close the ten remaining Hermes-informed delegation and learning gaps without
weakening CA's exact grants, approval ownership, session compatibility, or
CLI/TUI-only product boundary.

## Non-Goals

- No GUI, hosted control plane, or Cursor-hosted dependency.
- No copying implementation source or prompts from an oracle.
- No provider, credential, installed-binary, or user runtime configuration
  mutation as part of this task.
- No relaxation of child depth, concurrency, runtime, output, approval,
  cancellation, workspace, or tool authority limits.

## Evidence and Current Behavior

- `src/agent/subagent.rs` already owns bounded child spawning, exact inherited
  grants, immutable workspace snapshots, acceptance-tracked steering, bounded
  waits, terminal receipts, and result SHA-256 identities.
- `src/agent/registry.rs` exposes individual spawn/list/status/wait/steer/cancel
  tools but has no one-call batch, structured child-output contract, or global
  future-spawn pause.
- `src/worktree.rs` owns a recoverable, profile-owned Git worktree registry,
  while ordinary child agents currently share the parent workspace.
- `src/jobs.rs` records job ownership, but child finalization currently treats
  owned background work as child-scoped cleanup rather than a durable handoff.
- `src/skills.rs`, `src/learning.rs`, `src/codex_learning.rs`, and
  `src/learning_journey.rs` already provide digest-bound skill mutations,
  journals/undo, learning evidence, and a metadata graph; there is no curator
  lifecycle covering archive, restore, consolidate, pin, sync, and adopt, and
  graph projection does not join bounded native memory nodes.
- Read-only observations from the Hermes oracle show per-task JSON contracts
  with one repair attempt, parallel task batches, origin-bound detached
  completion, capability-bearing lifecycle handles, recoverable skill
  curation, memory/skill graph projection, spill artifacts, timeout diagnostics,
  persistent background-job attribution, worktree isolation, and a global
  spawn pause. These observations define behavior only; oracle source remains
  read-only and is not an implementation source.

## Scope

Allowed files:

- `src/agent/{subagent,registry}.rs` and narrowly scoped new agent lifecycle or
  artifact modules.
- `src/{main,app,jobs,worktree,plugin,skills,learning,learning_journey,memory,config}.rs`
  and narrowly scoped new curator/graph modules.
- Relevant TUI command/input/render modules and deterministic snapshots.
- `Cargo.toml`/`Cargo.lock` only if an audited dependency is necessary.
- This ExecPlan and `ARCHITECTURE.md`, `CONTINUITY.md`, `NEXT_STEPS.md`,
  `TESTING.md`, `PARITY.md`, `docs/.INDEX.md`, and `docs/QUALITY.md` when the
  implemented behavior creates a durable documentation obligation.

Forbidden files/areas:

- `oracles/`, `reference/`, nested repositories under them, credentials,
  provider profiles, saved sessions, installed binaries, and unrelated dirty
  tree changes.

Runtime state affected:

- New state must be private, bounded, versioned, atomic, and recoverable.
- Worktree and skill mutations require exact ownership/provenance receipts;
  automatic curator mutations are disabled unless explicitly enabled.
- Tests use temporary roots. Validation does not alter the user's live profile.

## Design and Invariants

- Structured child output uses a bounded local JSON-schema contract. The
  authoritative child result is validated once; failure permits exactly one
  model repair turn containing sanitized validation errors, then terminates
  truthfully if still invalid. Repair cannot widen tools or authority.
- A batch validates every task before admitting any child, has a stable batch
  identity, uses the existing concurrency governor, and returns one ordered
  consolidated terminal projection.
- Detached terminal completions are session/turn-bound events. They queue into
  the originating conversation and request at most one host-owned continuation
  when the foreground loop is safe; they never impersonate user input.
- Spawn pause blocks only future child admission. Existing children, steering,
  waits, cancellation, and result retrieval continue.
- Optional child worktrees are created only from a validated Git workspace,
  receive an owned branch/receipt, and are cleaned only under the existing
  clean-owned recovery rules. Shared workspace remains the compatibility
  default.
- The plugin-facing lifecycle facade is versioned and capability-protected.
  Handles bind child, origin, correlation ID, and expiry; reconnect/status/
  wait/cancel/result calls fail closed on invalid handles. Correlation IDs are
  idempotent within their retention window, and terminal records have bounded
  retention.
- Full child output spills to a private bounded artifact only when inline
  limits are exceeded. Terminal and timeout receipts identify its digest and
  safe path plus current action, bounded recent output, event trail, and
  attributed background jobs without credentials or raw provider payloads.
- Background processes may outlive a child only when already admitted as
  background jobs. Ownership is transferred to an origin-bound attribution
  record; cancellation and notification remain possible from the host.
- The skill curator builds on existing exact-digest journals. It can pin,
  archive, restore, synchronize managed state, adopt an unmanaged local skill,
  and consolidate an explicitly approved compatible set. Automatic policy is
  bounded, deterministic, recoverable, and opt-in; it never edits foreign
  read-only skill sources.
- The learning graph remains a local TUI/text visualization. It joins bounded
  current-digest skill nodes with bounded native memory metadata/snippets and
  evidence-scored edges while respecting memory scope and disclosure limits.
- Existing public session schemas and default TUI behavior remain compatible;
  new controls are additive.

## Milestones

### M1 — Characterize contracts and persistence seams

- Add failing focused tests for every acceptance behavior.
- Resolve existing structured-output validation, event-loop re-entry, plugin,
  worktree, job-owner, memory, and skill-journal APIs before implementation.
- Validation: focused test targets compile and fail for the intended missing
  behavior.

### M2 — Structured outputs, batch delegation, detached completion, pause

- Add per-child schema contracts and one repair turn.
- Add atomic one-call batch admission and ordered consolidated wait/result.
- Add origin-bound terminal event re-entry and global future-spawn pause/resume.
- Validation: agent manager/registry and interactive/headless loop focused tests.

### M3 — Worktrees, lifecycle facade, spills, diagnostics, job attribution

- Add optional owned child worktree isolation and terminal receipts.
- Add capability-protected plugin lifecycle handles with correlation,
  reconnect, and bounded retention.
- Add bounded private spill artifacts, detailed timeout receipts, and durable
  origin attribution for surviving background jobs.
- Validation: lifecycle, worktree, job, artifact, registry, and recovery tests.

### M4 — Curator and memory-aware graph

- Add recoverable curator state/actions and opt-in automatic policy.
- Add memory nodes/edges and bounded visual graph rendering.
- Validation: curator journal/recovery/policy tests and graph scope/render tests.

### M5 — Reconcile and complete

- Run all focused suites and repository completion gates.
- Reconcile harness documents and move this plan to `completed/` only after all
  ten acceptance criteria and gates pass.

## Acceptance Criteria

1. A child with a declared JSON output schema cannot complete successfully with
   invalid output; one and only one repair turn can produce a validated result.
2. One tool call can submit multiple child tasks, reject an invalid batch before
   partial admission, and later return an ordered consolidated completion.
3. A detached child completion is automatically queued into its originating
   conversation and causes at most one safe host continuation.
4. A child can opt into a dedicated owned Git worktree/branch and receives a
   truthful terminal isolation receipt; default shared-workspace behavior is
   unchanged.
5. Plugin code can use a versioned capability handle for launch, status, wait,
   cancel, result, and reconnect; forged/expired/cross-origin handles fail and
   repeated correlation IDs are idempotent during bounded retention.
6. The curator can archive, restore, consolidate, pin, sync, and adopt through
   exact-digest recoverable transactions; opt-in automatic policy operates only
   on eligible native skills and is deterministic in tests.
7. `/learning graph` (or its additive detail view) renders bounded native memory
   nodes and memory-skill edges without crossing memory scope or exposing full
   private bodies.
8. Oversized child output is available in a bounded private spill artifact with
   digest verification, and timeouts include detailed sanitized diagnostic
   state rather than only a generic terminal label.
9. A background process admitted by a child can outlive it with durable child,
   origin, and job attribution and remains controllable/observable by the host.
10. Global pause prevents every future subagent spawn path, including batch and
    lifecycle launch, until resumed, without cancelling existing children.

## Validation

Focused:

- New agent lifecycle/registry/structured-output/batch/event/artifact tests.
- Existing `agent::subagent`, registry, worktree, jobs, plugin, skills,
  learning, memory, and TUI command/render suites.

Completion:

- Both workspace test matrices recorded in `TESTING.md`.
- `cargo fmt --all -- --check`.
- Strict all-target, all-feature Clippy.
- All-feature release build.
- Help/version smokes, diff/snapshot hygiene, and protected-tree verification.

Manual/visual:

- Deterministic narrow/wide TUI graph/agent-control snapshots where presentation
  changes.

Blocked checks:

- None currently. No network or credentials are required.

## Progress

- [x] Reopened the incomplete Hermes comparison as ten explicit acceptance
  criteria instead of the narrower HRS-202 subset.
- [x] Characterize existing seams and add failing tests.
- [x] Complete structured output, batch, re-entry, and spawn-pause behavior.
- [x] Complete worktree, lifecycle, spill, timeout, and job-attribution behavior.
- [x] Complete curator and memory-aware graph behavior.
- [x] Pass completion gates and reconcile the harness.

## Decisions

- Decision: HRS-203 is a follow-on rather than rewriting HRS-202 history.
  Evidence: HRS-202's recorded subset and validation are accurate, but not the
  full ten-capability user requirement.
  Consequence: both plans remain auditable and HRS-203 owns the unfinished work.
- Decision: compatibility defaults remain shared workspace, no output schema,
  no automatic curator mutation, and spawning unpaused.
  Evidence: these are additive capabilities; default changes would alter public
  behavior and runtime state beyond the request.
  Consequence: every new mutation-heavy behavior is explicit or opt-in.
- Decision: detached results re-enter as host-owned lifecycle system data, not
  a synthetic user message.
  Evidence: this preserves conversation provenance while allowing exactly one
  idle continuation for the originating turn.
  Consequence: detached completion cannot impersonate the user or cross into a
  different conversation.
- Decision: lifecycle handles are a trusted in-process plugin facade, not a
  declarative-manifest native-code surface.
  Evidence: CA's plugin security contract intentionally excludes native code.
  Consequence: trusted integrations gain capability-scoped lifecycle control
  without widening installed plugin authority.

## Surprises and Discoveries

- The repository already contains strong component primitives for worktrees,
  structured headless output, skill mutation journals, native memory, and job
  ownership. HRS-203 should compose them rather than introduce parallel stores.
- The spawn-pause and correlation checks had to share their admission locks
  with child insertion; checking a flag before an await would leave a race.
- Spill creation performs filesystem work outside Tokio's scheduler, while
  receipts retain only bounded text plus path, length, and SHA-256 identity.
- Curator recovery needs both rollback of prepared moves and roll-forward of
  completed moves; a write-only final journal state cannot cover process death
  between rename and state publication.
- Repository-wide process stress exposed two unrelated timing races during the
  full-feature matrices. The voice case and broker-cache case each passed in
  the complementary full run and on immediate exact rerun; no HRS-203-owned
  test failed.

## Recovery and Rollback

- Preserve all unrelated dirty-tree changes and revert only HRS-203-owned hunks
  if recovery is required.
- Curator and worktree changes use their native journals/registries and clean-
  owned recovery rules; tests operate only in temporary roots.
- No destructive Git recovery commands are authorized.

## Completion Record

Changed files: `src/agent/{lifecycle,mod,registry,subagent}.rs`,
`src/{app,config,headless_contract,learning_journey,main,plugin,skill_curator}.rs`,
plus `ARCHITECTURE.md`, `CONTINUITY.md`, `NEXT_STEPS.md`, `TESTING.md`,
`docs/.INDEX.md`, `docs/QUALITY.md`, and this plan.

Commands and results: focused manager (28), lifecycle (4), registry (11),
curator (5), and learning-graph (7) suites pass. The clean default workspace
matrix passes. Two complete all-target/all-feature runs covered all HRS paths;
their complementary unchanged voice/broker timing failures each passed in the
other run and on exact rerun. Formatting, strict Clippy, all-feature optimized
build, help/version/signature, diff/snapshot hygiene, and protected-tree
verification pass. Full command/count evidence is recorded in `TESTING.md`.

Remaining risks: the repository retains the pre-existing timing sensitivity in
the voice descendant assertion and five-client broker cache-pruning stress
case. Neither is in HRS-203 scope, both reproduce as passing in isolation, and
neither affected an HRS-203 target. Curator automation remains opt-in; retained
worktrees and spill artifacts follow their explicit cleanup/retention rules.

Harness updates: completion state is reconciled across the plan,
`ARCHITECTURE.md`, `CONTINUITY.md`, `NEXT_STEPS.md`, `TESTING.md`, the index,
and quality guardrails. No provider, profile, session, installation, user skill
store, or oracle was changed.
