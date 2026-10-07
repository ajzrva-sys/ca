# ORCH-DELEGATION-FABRIC-003 — Mechanistic Delegation Fabric

Status: COMPLETED
Owner: codex-team
Created: 2026-08-26

## Purpose / Big Picture

Give AI agents one bounded, versioned contract for handing read-only repository
queries, deterministic evidence, and local process observation to Rust. The same
contract is used by the MCP server and the internal pre-Worker router, so a
mechanical action is validated, executed, cancelled, and reported identically
regardless of its caller. Learned artifacts may propose this contract through an
inert manifest, but stored scripts and executable members never gain authority.

The observable result is an `observe | delegate | operate` MCP hierarchy,
revision-bound repository queries, durable delegation jobs, typed task policy,
and manifest promotion rules that preserve the existing isolation, learning,
guard, and commit boundaries.

## Progress

- [x] Preserve the reviewed `codex-team` and `ca-watch` baseline in commit
      `c1513f5` after exact owner authorization.
- [x] Characterize the current recipe registry, MCP catalogs, job lifecycle,
      task/import contracts, watcher API, and learned-artifact lifecycle.
- [x] Add the shared versioned action/request/receipt/policy types and one
      validator/executor for repository queries, evidence, and local observation.
- [x] Consolidate deterministic recipe parsing and final execution allowlisting.
- [x] Add the MCP `delegate` tier, synchronous `repo_query`, and durable
      delegation start/status/cancel/feedback tools.
- [x] Add task/import/planner mechanistic actions and legacy/prefer/require/deny
      routing with compact query context and safe Worker skipping.
- [x] Add inert `DELEGATION.json` packages, shadow evidence, exact promotion,
      drift, quarantine, retirement, and privacy-bounded telemetry.
- [x] Update the supervisor skill and integration/source-of-truth documentation.
- [x] Run focused, workspace, formatting, strict lint, release, compatibility,
      snapshot, diff, and protected-tree gates; record the production isolation
      result and every fail-closed repository condition.

## Surprises & Discoveries

- The preservation inventory counted four nested control documents ignored by
  `crates/codex-team/.gitignore`; they were force-added because the owner
  authorized the complete reviewed tree.
- The prior inventory's aggregate tree digest algorithm was not recorded. The
  manifests matched exactly; a reproducible relative-path plus content-digest
  algorithm was established before the checkpoint for later audits.
- Existing active plans already own controlled recipe authority and the broader
  self-improvement vault. This plan extends their typed read/observe/verify
  ceiling and must not add source edits, builds, deployment, or background
  learning.
- Artifact files are namespaced before promotion, so delegation discovery must
  match the `DELEGATION.json` package filename rather than only a root path.
  Rust canonicalizes the manifest digest before namespacing, excludes it from
  prompt injection, and treats multiple manifests as ambiguous.
- Authority status and structural matching initially reused the registration
  helper. Those inspection paths were made read-only; only artifact promotion,
  shadow evidence, approval, feedback, quarantine, and correction mutate the
  authority record.
- The production Seatbelt self-test succeeds on this host. Evidence delegation
  therefore remains available without the debug-only isolation double.
- The ignored root `ca::McpManager` compatibility fixture already overlaps
  unrelated user edits and hard-codes the old MCP catalogs. It successfully
  discovers the new `repo_query` tool before its stale 10-tool assertion fails;
  the overlap stop condition forbids rewriting that fixture in this slice.
- The built-in protected-tree check fails closed because ignored `oracles/` and
  `reference/` roots have no trusted tracked-content baseline. The release
  audit independently confirms they are absent from tracked release source,
  and the exact Git diff from the preservation commit contains no protected
  path.

## Decision Log

- The checkpoint commit is a separate historical baseline. Feature edits begin
  only after it and will not absorb unrelated root-worktree changes.
- `MechanisticActionV1` is the sole execution vocabulary. MCP handlers, task
  routing, and learned manifests call the same library entry points and never
  assemble arbitrary commands.
- `repo_query` is synchronous and read-only; evidence and observation are
  durable jobs. Query results are returned after redaction but are not written
  into telemetry or learning state.
- Delegate readiness is capability-specific: lack of production verification
  isolation disables evidence actions but does not disable pure repository
  queries or contained local observation.
- Learned packages are data. `DELEGATION.json` may name typed actions, while
  script text, binaries, executable members, and shell strings are permanently
  non-executable.
- A repository revision is the local repository fingerprint for quarantine in
  this slice. It already binds HEAD, Git metadata, staged identities/modes, and
  dirty path bytes/types/modes; the authority record is separately scoped to
  one manifest digest.

## Context and Orientation

- `src/verification/{recipe,validate,runner,isolation}.rs` owns typed no-shell
  verification policy and execution.
- `src/mcp/{server,jobs}.rs` owns pinned-repository MCP catalogs and durable job
  records. Existing step jobs supervise executor-owned process groups.
- `src/app.rs::try_run_mechanistic_handoff_task` is the current pre-Worker route.
- `src/domain/task.rs`, `src/task_import.rs`, `schemas/task*.json`, and planner
  prompt/schema files define task contracts.
- `src/learning/mod.rs` owns approved recipe activation, drift, quarantine, and
  retirement. `src/self_improvement/` owns learned artifact packages and vault
  records.
- The workspace `ca-watch` crate supplies generic/copy PID observation. This
  slice permits only local observation with repository-contained marker/log
  paths; SSH remains outside shared delegation authority.
- `skills/codex-team-ca/SKILL.md` is the repository-contained supervisor skill
  that agents use to discover and prefer safe mechanical delegation.

## Plan of Work

First add characterization tests that expose the current MCP catalogs and the
recipe parser/final-allowlist drift. Introduce a `mechanistic` module with closed
serde types, explicit limits, validation errors, capability readiness, and a
single executor. Repository queries canonicalize a pinned Git root and revision,
enumerate only tracked regular files, reject links/hardlinks/private or
secret-shaped paths, redact output, and cap operations, entries, lines, and
aggregate bytes. Evidence actions reuse the verification registry and runner.
Observation actions adapt the local `ca-watch` API and never signal observed
PIDs.

Extend the durable job layer with a delegation job kind and executor-owned
cancellation. Add the `delegate` access level between observe and operate;
advertise `repo_query` in observe, four delegation tools in delegate, and retain
the existing mutation tools only in operate. Keep schemas closed, annotations
truthful, resources empty, repository selection impossible, and all starts
revision-bound.

Then extend task/import/planner contracts additively. Existing tasks deserialize
to `legacy`; new policies validate before routing. Query receipts become bounded
role context. Only admitted, machine-checkable `rust_only` evidence work can
complete without Worker. `prefer` may record a declined/unavailable fallback;
`require` never falls back; real evidence failure is preserved as evidence.

Finally add an inert delegation manifest to learned artifact packages. Validate
exact actions and limits, shadow against real typed receipts, propose after
three matching successes over two revisions, require exact digest plus current
revision approval, and compose existing activation/quarantine/retirement rules.
Telemetry stores only hashes, categories, timing, revision, and avoided-call
estimates. Update docs and the bundled skill, then complete the full matrix.

## Concrete Steps

1. Run and preserve focused characterization tests for MCP catalogs, recipes,
   task schemas, learning artifacts, and `ca-watch` behavior.
2. Add `src/mechanistic/{mod,contracts,query,executor,observation}.rs` and expose
   it from `src/lib.rs`.
3. Refactor recipe parsing/validation so every accepted recipe maps through one
   registry to a `ValidatedCommand`; add Cargo fmt/Clippy and Node/Python parity
   regressions.
4. Extend `src/mcp/jobs.rs`, `src/mcp/server.rs`, and CLI access-mode parsing;
   add SDK and unit coverage for all three catalogs and durable delegation.
5. Extend task DTOs, import schemas, planner schema/prompt, and `src/app.rs`
   routing with additive defaults and policy receipts.
6. Extend learned artifact contracts/vault/compiler with `DELEGATION.json` and
   shadow/promotion lifecycle tests using temporary repositories and homes.
7. Update architecture, continuity, testing, CA integration, README, schemas,
   and the supervisor skill.
8. Run the acceptance matrix from the repository root and record every result.

## Validation and Acceptance

Focused acceptance covers:

- Query batching, revision drift, traversal, symlink/hardlink/untracked/private
  denial, redaction, binary/oversized input, output caps, and no target mutation.
- Every supported evidence recipe, shell/operator/network rejection, isolation
  loss, cancellation, timeout, bounded output, and zero provider calls.
- Marker-first copy completion, PID identity drift, degradation recovery, and
  cancellation that never signals the observed PID.
- Exact observe/delegate/operate catalogs, closed schemas, annotations, durable
  jobs, stale requests, CA client discovery, and zero resources.
- Task-policy routing, compact query context, eligible Worker skip, edit-task
  non-bypass, and prefer/require semantics.
- Inert scripts, manifest validation, shadow thresholds, exact approval,
  ambiguity, drift, quarantine, retirement, human negative evidence, and raw
  data exclusion.

Completion commands, run from `/Users/aji/project/ca`, are:

    cargo test --workspace --no-fail-fast -- --test-threads=1
    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets --all-features -- -D warnings
    cargo build --workspace --release --all-features
    git diff --check
    target/release/codex-team verify snapshot-hygiene --repo /Users/aji/project/ca
    target/release/codex-team verify protected-trees --repo /Users/aji/project/ca

Run the documented CLI/MCP compatibility tests as well. Do not substitute the
debug-only isolation double for the production capability check.

## Idempotence and Recovery

All query calls are mutation-free and revision-bound. Job creation uses
create-new identifiers and atomic records; replay returns the existing receipt
or a stale error. Cancellation may terminate only a child spawned by the
delegation executor or stop a watcher future. Learned proposals, approvals, and
retirement evidence are immutable digest-addressed records. Partial writes fail
closed and can be retried from the last durable receipt. No recovery path uses
reset, clean, stash, checkout, or deletion of user data.

If implementation overlaps an unrelated dirty path, stop before editing that
path. The baseline can be inspected at `c1513f5`; recovery must preserve newer
user work rather than restoring it destructively.

## Artifacts and Notes

- Baseline commit: `c1513f5 Preserve codex-team and ca-watch baseline`.
- Authorized preservation set: root `Cargo.toml`, root `Cargo.lock`, all eight
  `ca-watch` files, and all 216 `codex-team` files.
- Existing pre-change validation: 556 library tests passed with one external
  binary test ignored; ten integration tests, formatting, and strict Clippy
  passed.
- Final validation: the serial workspace matrix passed; `codex-team` has 579
  passing library tests with one documented external-binary ignore and eleven
  passing integration tests. Workspace formatting, strict all-target/all-feature
  Clippy, all-feature release build, schema parsing, diff hygiene, release CLI,
  real release-`ca` role compatibility, snapshot hygiene, and the production
  isolation probe passed.
- The release binary digest is
  `ffe5090b3434dac593fa8b6aa88a50242be6140987c3c40b51806ea74b532cb2`.
- Expected fail-closed receipts remain for the dirty/uncommitted repository
  release audit, the unbaselineable ignored protected roots, and the overlapping
  stale root MCP catalog fixture. No bypass, cleanup, or unrelated edit was
  made.

## Interfaces and Dependencies

Public Rust contracts: `MechanisticActionV1`, `MechanisticRequestV1`,
`MechanisticReceiptV1`, `DelegationPolicy`, action-specific request/result
types, readiness, and durable job receipts.

Public MCP access levels: `observe`, `delegate`, `operate`. Public tools add
`repo_query`, `delegation_start`, `delegation_status`, `delegation_cancel`, and
`delegation_feedback`; all existing operate tools remain compatible.

Public task/import/planner fields add optional `mechanistic_actions` and
`delegation_policy`. Learned packages add inert `DELEGATION.json` governed by
the `read_observe_verify_only` safety class.

No new network, shell, deployment, installation, dependency-changing, or
product-edit capability is introduced.

## Outcomes & Retrospective

The shared Rust contract, MCP delegation tier, internal task routing, and inert
learned-manifest lifecycle are implemented and validated. Pure repository
queries can precompute bounded context, typed evidence can skip Worker only for
eligible Rust-only tasks, and contained observation cancellation never signals
the observed PID. Learned script text remains inert at every stage.

The production isolation probe passed. The one root actual-client fixture that
needs a catalog expectation refresh is intentionally untouched because it
overlaps unrelated pre-existing user work; its discovery output proves that the
new tool reached `ca` before the stale assertion. The dirty repository cannot
produce a clean release-readiness receipt until feature changes and unrelated
work are separately reviewed. Feature changes remain uncommitted; the only
authorized commit is the baseline `c1513f5`.
