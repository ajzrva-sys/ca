# AGT-157 — Host-Owned Agent Verification and Budgets

Status: COMPLETED
Owner/session: current Codex session
Last updated: 2026-08-23

## Goal

Prevent assistant self-assertion from completing dependency-graph work. Only a
host-created, revision-bound verification receipt with authoritative evidence
may release a dependent task, and every autonomous graph must consume one
monotonic finite resource budget.

## Non-Goals

- No model-authored receipt, hidden-reasoning inspection, automatic merge, or
  widening of child tools/workspaces/network authority.
- No silent migration of a legacy pending/running graph into verified status.
- No provider traffic, real worktree mutation, runtime profile/session change,
  oracle/reference mutation, or installed-binary change during tests.

## Evidence and Current Behavior

- `AgentTaskDefinition` defaults to `AssistantOutput` in
  `src/agent_graph.rs`, and `evidence_satisfied` accepts any nonblank summary.
- `WorkState::observe_agent_task` converts that result into successful review
  evidence, marks the task complete, and releases dependents.
- `perform_agent_graph_tick` supplies only a child summary and successful tool
  names; both originate from the child run rather than a deterministic artifact
  contract.
- Current child limits cover concurrency, depth, logical turns, elapsed time,
  and retained output. Physical requests/retries, total tokens/cost, per-call
  output, tool calls/failures, and graph-wide totals are not bounded together.

## Scope

Allowed files: `src/agent_graph.rs`, `src/work.rs`, `src/main.rs`,
`src/agent/{client,subagent}.rs`, compatible session/TUI files and focused
tests/snapshots, plus the root harness and this plan.

Forbidden files/areas: `oracles/`, `reference/`, credentials, user runtime
state, installed binaries, provider endpoints, and unrelated dirty changes.

Runtime state affected: deterministic in-memory/test session fixtures only.

## Design and Invariants

- `AssistantOutput` is an untrusted completion claim, never authoritative
  evidence by itself.
- `ca.agent-verification-receipt.v1` is created by Rust and binds plan/task,
  attempt, child, normalized definition/contract, terminal event sequence,
  source/workspace revision, checks, budget use, disposition, and receipt
  digest. It stores no raw output, tool arguments, or secrets.
- Dispositions are `pass`, `fail`, and `inconclusive`; only `pass` marks a task
  complete or releases dependents.
- New graph configurations require an authoritative contract: exact artifact
  predicates/checks or explicit human review. Legacy missing contracts pause as
  inconclusive and require reconfiguration plus approval.
- Writable tasks sharing one workspace lane are dependency-ordered and
  serialized; incomparable writers on one lane are rejected.
- Resource counters are append-only across retry, pause, restore, and replan.
  A changed hypothesis/evidence digest may clear a loop detector, never a
  resource counter.
- Provider requests reserve before dispatch. Hidden transport retries, nested
  children, reviewers, tool failures, and failed calls consume the same task
  and graph totals.

## Milestones

### M1 — Receipt boundary and dependency closure

- Add typed evidence contracts, checks, dispositions, and receipts with
  backward-compatible defaulted session fields.
- Make output-only and legacy completion inconclusive.
- Permit completion only with host-observed authoritative evidence and a valid
  receipt; surface the disposition in `/tasks` without exposing raw output.

### M2 — Deterministic artifact and workspace-lane checks

- Bind contracts to graph/source/workspace identity and expected contained
  artifact predicates or closed argv verification.
- Snapshot candidates immutably, reject symlink/path escape/drift, serialize
  same-lane writers, and require explicit cross-lane handoff tasks.

### M3 — Monotonic graph budgets

- Add graph/task limits and usage for 30 active minutes, 30 physical requests,
  100,000 tokens, 8,192 output tokens per request, 120 tool calls, eight tool
  failures, eight child spawns, 256 KiB retained output, and one attempt by
  default (maximum three).
- Require an explicit positive cost cap or an explicit unpriced-route decision
  with a physical-request cap. Use one physical provider attempt per
  reservation and fail closed on unknown usage as declared.

## Acceptance Criteria

- A nonempty assistant response alone never completes a task or releases a
  dependent.
- Missing artifacts, symlinks, path escape, check failure/timeout, source
  drift, receipt corruption, legacy missing contract, or inconclusive review
  cannot unblock downstream work.
- A host-observed authoritative check produces a deterministic valid receipt
  and may release a matching dependency exactly once.
- Restored legacy running/pending graphs pause; historical completed tasks stay
  readable and are labelled legacy-unverified.
- Budget equality stops before the next operation; retries and restores retain
  totals, and no downstream task runs after budget-limited/inconclusive status.

## Validation

Focused: `agent_graph`, `work` dependency/receipt tests, session round trips,
agent runner budgets, and `/tasks` wide/narrow rendering.

Completion: `cargo test --workspace --no-fail-fast`, all-target/all-feature
workspace tests, formatting, strict Clippy, all-feature release build, binary
smokes, diff/snapshot hygiene, and protected-tree status.

Manual/visual: deterministic reviewed snapshots only.

Blocked checks: real provider, real worktree mutation, and paid cost accounting.

## Progress

- [x] Unsafe response-only completion and missing budget dimensions characterized.
- [x] Active ExecPlan and queue record created.
- [x] Host-owned receipt boundary implemented and focused tests pass.
- [x] Artifact/workspace-lane verification implemented.
- [x] Monotonic graph budgets implemented.
- [x] Completion checks and harness reconciliation complete.

## Decisions

- Decision: preserve old serialized fields but treat missing verification
  contracts as inconclusive.
  Evidence: Session V4 must remain readable without manufacturing authority.
  Consequence: unsafe automatic migration is impossible.
- Decision: successful tool names are telemetry unless the host contract marks
  an exact tool observation authoritative.
  Evidence: a tool name alone does not prove the requested artifact or result.
  Consequence: M1 can close assistant-output completion while M2 adds stronger
  artifact and deterministic command predicates.

## Surprises and Discoveries

- Existing logical-turn limits permit up to twice as many physical requests
  because the default provider client retries twice outside graph accounting.
- `DecimalUsd` previously derived ordering from its stored scale and mantissa,
  so numerically equal values with different scales could compare incorrectly.
  Graph cost ceilings exposed the defect; numeric ordering now aligns decimal
  scales before comparison and has a focused regression.
- The first complete matrix exposed two stale fixtures: the generic schema test
  omitted the newly required graph budget, and the exact Plan-mode prompt still
  expected the pre-AGT wording. Both fixtures now exercise the current contract.

## Recovery and Rollback

Use narrow inverse patches. New serialized fields are additive/defaulted, so
rollback requires no data rewrite. Never reset or clean the shared checkout.

## Completion Record

Changed files: `src/agent_graph.rs`, `src/work.rs`, `src/agent/client.rs`,
`src/agent/subagent.rs`, `src/main.rs`, `src/app.rs`, `src/tui/{commands,input,
render,render_tests}.rs`, and `src/usage.rs`, plus the reconciled harness.

Commands and results: agent-graph tests passed 7/7, work tests passed 12/12,
graph-tick tests passed 3/3, and task-graph TUI/input tests passed 4/4. A
separate regression proving an approval binding cannot change while an agent
is running passed 1/1. Formatting and focused diff checks pass; the only output
was the existing macOS oversized `__eh_frame` linker warning.

Second review found and closed three M1 fail-open cases: restored legacy
completion, valid non-pass receipts, and stale receipts rendered as Pass. The
current focused totals are agent-graph 7/7, work 15/15, graph-tick 3/3, and
task-graph TUI/input 5/5. The complete default workspace suite, formatting,
strict all-feature Clippy, and release build pass on the integrated checkout.
The final all-target/all-feature run passed all agent coverage but exposed one
unrelated flaky `codex-team` clean-filter exact-count assertion; its focused
rerun passed and REL-160 tracks it.

Final result: artifact predicates bind contained regular/directory/absent
state, source revision, before/after drift, optional digest/size/change rules,
and host terminal evidence. Closed verifier commands use exact argv, contained
cwd, existing sandbox/network authority, bounded time, and no shell. Writable
primary/worktree lanes are serialized and cross-lane dependencies require an
explicit handoff. Graph requests, provider usage/cost, tool calls/failures,
children, output, elapsed time, retries, and restored usage share one monotonic
finite budget. Assistant output never becomes authoritative; only a current
host pass receipt or an explicit HumanReview gate can release a dependency.

Final validation: focused agent-graph 14/14, work 16/16, graph-tick 3/3,
task-graph 5/5, human-review 1/1, provider one-attempt, and decimal-ordering
tests pass. Both exact serial workspace matrices pass, including CA 2,279/4
ignored in the default matrix and CA 2,304/4 ignored plus the 118/1 ignored
experiment target in the all-target/all-feature matrix. Formatting, strict
workspace all-target/all-feature Clippy, all-feature release build,
`git diff --check`, zero `.snap.new`, and direct oracle/reference inspection
pass. The only emitted diagnostic is the existing macOS oversized
`__eh_frame` linker warning.

Remaining risks: real provider accounting and real worktree mutation remain
deliberately unexecuted; their boundaries are covered by deterministic
loopback and disposable-workspace tests. No installation occurred for AGT-157.

Harness updates: active plan/task/index pointers created.
