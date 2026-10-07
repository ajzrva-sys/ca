# INT-194 — Dependency-Aware Verification and Automatic Failure Diagnosis

Status: COMPLETE
Owner/session: current Codex implementation session
Last updated: 2026-08-27

## Goal

Make automatic mechanistic verification dependency-aware, diagnose an
authoritative failure before CA spends a repair pass, and expose a bounded
evidence trail that can safely resume from saved sessions.

## Non-Goals

- No new shell, arbitrary command, edit, network, commit, installation, or
  orchestration authority.
- No MCP catalog/resource changes or delegation on excluded CA surfaces.
- No clean-tree gate, reset, stash, commit, protected-tree edit, live provider
  traffic, or runtime-profile mutation.

## Evidence and Current Behavior

- `codex-team-protocol` already exposes composite query actions and
  `verify_changed_files`, with optional decision metadata but no typed failure
  diagnosis or dependency-selection metadata.
- Codex-team currently selects focused verification from nearest manifests and
  filename affinity. `diagnose_failed_recipe` summarizes a terminal evidence
  job but does not classify cause or recommend a typed rerun.
- CA retains one `FileMutationReceipt` per successful editing tool, gates
  continuation through a durable verification job, permits two repair passes,
  and has only a process-local event deduplicator.
- Delegation feedback and negative-evidence cache invalidation already exist;
  INT-194 must reuse them.
- The checkout contains extensive pre-existing dirty and untracked work. It is
  informational; edits stay confined to INT-194 ownership and are reviewed by
  exact diff.

## Scope

Allowed files: shared mechanistic protocol/schema; codex-team mechanistic,
verification, job, and focused tests; CA mechanistic, mutation/undo, session,
TUI command/presentation, headless/main integration and focused tests; current
architecture, continuity, testing, task-index, and this plan.

Forbidden files/areas: `oracles/`, `reference/`, credentials, runtime profiles,
saved user sessions, installed binaries, Git index/history, and unrelated
dirty files.

Runtime state affected: focused tests may create temporary repositories and
ordinary build caches. Quality-history tests use temporary profile roots only.

## Design and Invariants

- A bounded deterministic dependency graph maps changed tracked paths through
  owning packages and transitive reverse dependencies to typed recipes. Graph
  failure escalates to policy-controlled full verification, never path guesses.
- Failure diagnosis reads the exact durable job, never reruns it, and consumes
  no repair pass. Revision drift remains authoritative and blocks continuation.
- Mutation batches contain exact bounded paths and are atomic undo/redo units.
- Evidence presentation may show bounded active receipt data, while persistent
  quality records contain only hashes/categories/timing/outcomes.
- Saved checkpoints resume only on explicit session opening after exact server,
  revision, digest, and budget validation. `--no-session` stays process-local.
- Events may update model-free state or cause one required continuation; they
  cannot recursively enqueue themselves or generate polling provider calls.

## Milestones

### M1 — Characterization and public contracts
- Add failing protocol, planner, diagnosis, mutation-batch, checkpoint, and
  evidence-ledger tests.
- Add backward-compatible receipt/session types and bounded validation.
- Validation: focused protocol and CA unit tests.

### M2 — Dependency graph and diagnosis executor
- Implement bounded ecosystem graphs, typed package/target recipe selection,
  process-memory graph caching, escalation, and structured diagnosis.
- Validation: focused codex-team mechanistic and MCP tests.

### M3 — CA workflow, mutation batches, and recovery
- Gate repair continuation on automatic diagnosis, standardize mutation
  batches, persist optional checkpoints, and advance them through typed events.
- Validation: focused main/headless/retry/session/exclusion tests.

### M4 — Evidence ledger and privacy-safe quality history
- Add `/evidence`, reuse existing feedback, and write bounded profile-scoped
  hash-only quality aggregates with fail-open corruption handling.
- Validation: focused command/render/privacy/feedback tests.

### M5 — Completion documentation and gates
- Reconcile architecture, continuity, testing, and task ownership.
- Run the workspace suite once, formatting, strict Clippy, all-feature release,
  diff/snapshot/protected checks, catalog compatibility, and production
  isolation probe.

## Acceptance Criteria

- A composer-cursor refactor selects owning/reverse-dependent CA targets rather
  than the workspace suite when the graph is bounded and unambiguous.
- A failed receipt is deterministically diagnosed before the first repair
  continuation without consuming a repair pass or rerunning the recipe.
- Exact multi-file receipts drive verification and undo/redo atomically.
- `/evidence` explains selection, cache, edits, diagnosis, and authorization;
  feedback and status refresh make zero provider calls.
- An explicitly reopened saved session reconnects once when current and blocks
  without restart when stale; `--no-session` persists nothing.
- Existing 11/15/28 catalogs, zero resources, two-repair maximum, and every
  recursion exclusion remain unchanged.

## Validation

Focused: protocol, dependency graph, diagnosis, MCP, mutation/undo, automatic
verification, session recovery, event coordination, evidence UI, feedback,
quality-history privacy, retry, and excluded-surface tests.

Completion:

```text
cargo test --workspace --no-fail-fast -- --test-threads=1
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo build --workspace --release --all-features
git diff --check
```

Manual/compatibility: release help/version, exact 11/15/28 zero-resource MCP
fixture, snapshot/protected-tree hygiene, and the production isolation probe.
Live provider traffic and runtime installation are excluded.

## Progress

- [x] Current INT-193 protocol, executor, diagnosis, mutation, session, event,
  feedback, and documentation boundaries characterized.
- [x] Focused characterization and regression tests added.
- [x] Dependency graph and structured diagnosis implemented.
- [x] Mutation batches, checkpoints, and event transitions implemented.
- [x] Evidence ledger and quality history implemented.
- [x] Focused validation passed.
- [x] Completion gates passed once.
- [x] Harness reconciled and plan moved to completed.

## Decisions

- Decision: deliver all eight INT-194 improvements in one milestone.
  Evidence: explicit user selection during planning.
  Consequence: protocol, host workflow, persistence, and TUI remain one coherent
  compatibility change.
- Decision: saved-session opening automatically continues current work.
  Evidence: explicit user selection during planning.
  Consequence: recovery validates exact checkpoint state before making at most
  one required provider continuation.
- Decision: quality history is a bounded hash-only rolling file.
  Evidence: explicit user selection during planning.
  Consequence: raw prompts, outputs, paths, and secrets remain absent.

## Surprises and Discoveries

- Focused verification had no explicit way to distinguish “full escalation is
  required and permitted” from “focused-only policy.” `VerifyChangedFiles`
  therefore gained a backward-compatible `allow_full_escalation = false`
  field; CA sets it from the active profile policy, and codex-team fails
  unverified when escalation is required but forbidden.
- Durable codex-team status records expose the exact job and revision while a
  job is running; the request digest becomes independently checkable from the
  terminal mechanistic receipt. Recovery consequently validates job/revision
  before polling and validates request/receipt digests before continuation.
- The CA binary's debug link still reports the known macOS compact-unwind size
  warning. Focused tests pass; strict Clippy and the release build remain the
  completion authority.

## Recovery and Rollback

Use exact diffs and pre-edit file digests to isolate INT-194 hunks. Revert only
those hunks with narrow patches if necessary; do not use destructive Git
commands or modify unrelated dirty state.

## Completion Record

Changed files:

- Shared contract and schemas:
  `crates/codex-team-protocol/src/lib.rs` and generated mechanistic schemas.
- Codex-team graph, execution, diagnosis, typed recipes, and tests:
  `crates/codex-team/src/mechanistic/{dependency_graph,executor,query}.rs`,
  the verification registry/recipe/runner modules, and MCP coverage.
- CA mutation, session, delegation, coordinator, schedule, TUI, and host-loop
  integration: `src/{undo,session,mechanistic_delegation,schedule_v2}.rs`,
  the app/main/headless/job paths, and `src/tui/` command/render coverage.
- Root and nested architecture, continuity, testing, task, and integration
  documentation.

Commands and results:

- Focused protocol: six passed; codex-team mechanistic: 17 passed; real stdio
  SDK: six passed; CA mechanistic/session/event/evidence suite: 24 passed;
  undo/mutation batch: six passed.
- The single serial workspace run reached every target. Its only failure was a
  stale command-count snapshot (90 versus 91) after adding `/evidence`; the
  corrected exact snapshot test passed. Codex-team passed 595 library tests
  with one external-binary ignore and all 13 integration tests.
- Formatting, strict all-target/all-feature Clippy, all-feature optimized
  workspace build, release help/version, diff/snapshot/protected-tree checks,
  real release 11/15/28 zero-resource CA compatibility, and production
  Seatbelt without a debug override: PASS.

Remaining risks:

- Graph generators intentionally fail to policy-controlled full verification
  on unsupported manifest shapes, ambiguity, excessive fan-out, or isolation
  loss. That is a bounded safety behavior, not a heuristic fallback.
- The root remains intentionally uninitialized for codex-team orchestration,
  so overall doctor is unhealthy even though the production isolation subcheck
  passes.

Harness updates:

- Root and nested continuity/testing/architecture documents describe INT-194;
  `NEXT_STEPS.md` marks it complete, and this plan is archived under
  `docs/exec-plans/completed/`.
