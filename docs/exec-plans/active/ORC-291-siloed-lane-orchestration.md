Do not start this plan unless the task names it. The continuation is [AGT-271](AGT-271-task-first-economic-delegation.md).

# ORC-291 — Silo'd lane orchestration: declared handoffs, clarification channel, facts/merge lanes

Status: HELD — design reviewed; implementation in progress
Date: 2026-10-15 (started)

## User intent

Review `esxi-vmxnet3-case-runner-task-graph.md` and `ca-siloed-orchestration.md`
and add the orchestration capability they describe to ca. The ESXi case runner
is only an example of the *shape*: one coordinator owning a task DAG whose
tasks run as isolated subagent lanes that share nothing except narrow briefs
and host-verified declared artifacts. The capability, not the ESXi workload,
is the deliverable.

The silo'd-orchestration design discussion settles on this contract set:

1. **Silo'd lanes** — each lane gets a fresh conversation, a need-to-know brief,
   disjoint writes, and declared inputs only; never sibling transcripts or state.
2. **Contract-only handoffs** — a handoff is a named artifact with a contract
   (path, kind, digest/size). Lanes only receive inputs the graph declares;
   undeclared cross-lane consumption fails closed.
3. **Evidence over claims** — completion is host-verified (existing AGT-157
   receipts); only pass receipts release dependents.
4. **Bounded attempts and one finite graph budget** — existing AGT-157 budgets.
5. **Fail-stop + replayable ledger** — persist the graph and verified handoffs;
   restore pauses (existing) and reuses verified facts instead of re-running.
6. **Typed clarification channel** — a lane may ask its coordinator bounded
   typed questions instead of stalling on an under-specified brief.
7. **Facts and merge lanes** — coordinator-level dedupe of verified inputs
   keyed by contract hash; merge as its own isolated lane receiving only
   declared outputs, a base, and a declared conflict policy.

## Current capabilities and observed gaps

Source audit (2026-10-15) of `src/agent_graph.rs`, `src/work.rs`,
`src/main.rs` (`perform_agent_graph_tick`), and `src/agent/subagent.rs`:

| Requirement | Current behavior | Gap |
| --- | --- | --- |
| Coordinator-owned DAG, admission, scheduling | `configure_agent_graph` → `AgentTaskDefinition` per task with `depends_on`; `ready_tasks` + tick spawn/observe loop | none |
| Isolated lanes | `worktree_id`, path/command scopes, exact tool grants, `ensure_workspace_lanes` (same-lane writers serialized; cross-lane dependency requires `accept_cross_lane_handoff`) | none |
| Fresh lane conversations, no sibling transcript | child spawns use `AgentContextMode::Summary` with the plan render; no sibling transcripts | none |
| Contract-only handoffs (named artifact, path/kind/digest) | boolean `accept_cross_lane_handoff` only; no declared artifact identity; the consuming brief contains no artifact facts | **M1** |
| Host-verified completion | AGT-157 receipts, workspace snapshots, artifact/closed-command gates | none |
| Bounded attempts + graph budget | `max_attempts`, monotonic budgets | none |
| Fail-stop + replayable ledger | `pause_agent_graph_after_restore` (fail-stop, tamper rejection) | reuse of verified facts across restore/retry missing: **M3** |
| Typed clarification channel lane→coordinator | subagent `WaitingApproval` machinery exists, but graph lanes have no bounded typed question tool routed to the coordinator | **M2** |
| Facts lane / hash-keyed dedupe | none | **M3** |
| Merge lane with declared conflict policy | none | **M3** |

## Design

### M1 — Declared handoff artifacts

- New `HandoffArtifactV1 { name, path, kind, sha256?, min_bytes?, max_bytes? }`
  and `HandoffInputV1 { producer: TaskId, name, path }`; both serde-defaulted
  in `AgentTaskDefinition` (`produces`, `consumes`) and `AgentTaskInput`.
- Validation in `parse_configuration`: unique artifact names per task; produced
  path must fall under the producer's path grant; each `consumes` producer must
  be a `depends_on` prerequisite that declares the named artifact; the consumer
  path must fall under the consumer's path grant; cross-lane consumption
  requires `accept_cross_lane_handoff`; bounded handoff counts.
- `VerificationContractBinding` gains `produces`/`consumes` (and the M3 fields),
  so the receipt contract sha256 binds the whole declaration set. Old receipts
  without the fields fail closed through the existing binding comparison.
- Tick: before spawn, host-verifies every declared input at the consumer's path
  (existence, kind, digest/size constraints) and injects a `Declared handoff
  inputs` section into the child brief with name/producer/path/digest/size.
  No other producer context is passed. Missing/mismatched input → preflight
  failure, task blocked, dependents never release.
- After terminal verification, capture each declared output's digest/size into
  the attempt (`handoff_outputs`) so the ledger has verified facts.
- Invariant: a lane can only be *told* about artifacts it declares in
  `consumes`; everything else about sibling lanes stays out of the brief.

### M2 — Typed clarification channel

- New subagent-only tool `request_graph_clarification` (bounded question +
  optional choices) granted automatically to TaskGraph-origin children.
- Calling it puts the attempt in `WaitingApproval` with the question stored on
  the snapshot; the tick surfaces the pending question on the task; the
  coordinator answers through the existing approval/answer machinery, resuming
  the lane with the answer as the tool result. Bounded per attempt (one
  pending question at a time; questions counted by the existing budget).

### M3 — Facts lanes, merge lanes, replayable ledger

- `task_kind: AgentTaskKind` (`agent` default, `facts`, `merge`) in definition
  and input; `merge_policy` (`RejectConflict` default, `LastWriteByTaskOrder`,
  `ThreeWayText`) valid only on merge tasks.
- Ledger: `AgentGraphState.handoff_ledger` persists verified facts
  (`name/task/path/digest/bytes/contract_sha256`). On restore/retry, a facts
  task whose verified outputs are already in the ledger with matching digests
  completes from the ledger (`reused_from_ledger` on the attempt) instead of
  spawning — replay without recompute, still bound to the same contract.
- Merge lanes receive a brief containing *only* declared inputs with digests,
  the base, and the declared conflict policy; their output is verified like
  any other task and becomes declared input for dependents.
- No weakening of AGT-157: outputs are facts only when host-verified; ledger
  entries are immutable per contract; tampered digests re-verify or fail.

## Invariants (do not break)

- Only a current host pass receipt releases dependents (AGT-157).
- Restore still pauses running graphs; ledger reuse never marks a never-run
  task completed without a valid host-verified fact.
- Additive serde defaults keep saved sessions readable; legacy graphs without
  handoff declarations keep working unchanged.
- No sibling transcript, raw output, or secret enters any lane brief.
- Approval classification and tool-schema alignment stay consistent; the new
  tool is subagent-only and non-mutating.

## Allowed files

`src/agent_graph.rs`, `src/work.rs`, `src/main.rs`, `src/agent/{registry,
subagent,tools}.rs`, compatible session/TUI files (`/tasks` surfacing only),
focused tests, `docs/exec-plans/active/ORC-291-*.md`, and the root harness.
No oracle/reference edits; no runtime config/session mutation.

## Milestones and acceptance

- **M1**: declared produces/consumes validate (unique names, path grants,
  dependency+declaration required, cross-lane handoff flag); briefs contain
  only declared input facts; missing/mismatched inputs block the task and
  dependents; receipts bind the declarations (contract hash changes).
- **M2**: a graph lane can raise one bounded typed question, the coordinator
  sees and answers it, the lane resumes with the answer, and the question
  count is bounded.
- **M3**: facts outputs are verified and persisted in the ledger; restore/
  retry reuses matching verified facts instead of re-running; merge tasks get
  declared-inputs-only briefs with the declared policy; policy-valid merge
  outputs pass normal host verification.

## Validation

Focused: new agent-graph/work/tick tests plus existing graph suites.
Completion (recorded honestly): `cargo test`, `cargo fmt --check`,
`cargo clippy --all-targets -- -D warnings` (baseline of 60 pre-existing
diagnostics unchanged), `cargo build --release --workspace --all-features`.
Full `./scripts/verify.sh` only if time/evidence permits; never claim green.

## Progress

- [x] Reviewed both source documents and audited current graph/subagent source.
- [x] ExecPlan created.
- [x] M1 implementation and focused tests (declared produces/consumes, host-verified facts, brief injection, fail-closed missing inputs, contract binding, ledger publication).
- [x] M2 implementation and focused tests (`request_graph_clarification` subagent-only tool, graph-lane auto-grant, parked answer via steering channel, bounds and decline paths).
- [x] M3 implementation and focused tests (facts/merge task kinds, declared conflict policies, graph handoff ledger with contract-bound reuse, kind-specific lane briefs).
- [x] Completion checks and release/install.

## Completion Record

Changed files: `src/agent_graph.rs`, `src/work.rs`, `src/main.rs`,
`src/agent/{registry,subagent,tools}.rs`, `src/tui/render_tests.rs` (fixture
fields only), `docs/exec-plans/active/ORC-291-siloed-lane-orchestration.md`,
`docs/.INDEX.md`, `CONTINUITY.md`, `NEXT_STEPS.md`, `TESTING.md`, and the root
`Cargo.toml`/`Cargo.lock` version advance (0.1.25 → 0.1.26). No oracle,
reference, runtime-config, or session mutation.

Commands and results (recorded honestly):

- Focused: 40+ tests pass across agent_graph (declarations, capture/verify,
  contract binding, kinds/policies, ledger helper), work (recording, session
  round-trip, ledger publication), registry (builtin count 99 + new validation
  sample), subagent (clarification channel bounds/answer/decline), and the
  graph tick (declared-fact injection, fail-closed missing input, existing
  worktree/dependency ticks).
- `cargo fmt --check` clean after `cargo fmt` (only new code hunks).
- `cargo clippy --all-targets -- -D warnings`: 60 diagnostics, identical to the
  documented baseline; zero in touched files. Two diagnostics introduced by
  this work were found and fixed during the run (never-loop clarification
  select, redundant ledger closure).
- `cargo test -p ca --no-fail-fast` (this sandbox, two hanging lsp fixture
  tests skipped): 2,913 passed / 367 failed / 42 ignored / 2 filtered. All
  failures are pre-existing environment families: sandbox PermissionDenied
  (loopback sockets, spawned binaries, MCP/CLI fixtures) and the known
  header-padding snapshot drift. Zero failures in touched modules; zero diffs
  mention the new tool. Two `src/lsp.rs` fixture tests hang in this sandbox and
  one `ca-automation` process-reaping test hangs/fails; all unrelated files.
- `cargo build --release --workspace --all-features` exits 0 (log:
  `scratch/ORC-291/release/build.log`).
- Release/install (standing authorization): version bumped to 0.1.26; signed
  candidate `target/release/ca-0.1.26-signed` (SHA-256
  `2506ada15e6a09104faa9559e134e3d713ad82c9c952bd65c9c62f380296673e`,
  ad-hoc signature valid); installed at `/Users/aji/.local/bin/ca` via
  privileged copy/rename, owner aji:staff mode 0755, signature valid on disk,
  `--version` reports `ca 0.1.26`, `--help` renders. Prior 0.1.25 preserved at
  `/Users/aji/.local/bin/ca.recovery-ORC291-20261015T1942Z-4881798dc2d0289ac`
  with its original hash `4881798dc2…`. `install_binary` refused with a
  persistent `Drift` in this environment, so the replacement used the closed
  privileged copy/rename path; the destination filesystem staging, hash,
  signature, ownership, and readback checks all pass.

Remaining uncertainty: full `./scripts/verify.sh` not run (environment and
time); the sandbox prevents the loopback/CLI fixture subset from running
anywhere, so the raw gate stays failed on the documented VAL-288 baseline plus
the environment-only families above. Start a fresh CA process to load 0.1.26.
