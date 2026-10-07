# ORCH-LEARN-COMPILER-001 — Controlled Learning Compiler v1

## Purpose / Big Picture

Turn the existing advisory deterministic-recipe harvest into a human-approved,
machine-global, evidence-only Rust execution lane. One observed successful
typed command chain may become a proposal. A human approves its exact digest
once through `ca`; matching repositories may then activate it automatically,
but execution is permitted only for an already-admitted evidence-only task and
only inside required verification isolation.

## Progress

- [x] Restore the integration branch's all-target build and finish required verification isolation.
- [x] Add versioned proposal, approval, activation, execution, and catalog records.
- [x] Migrate legacy harvested recipes as observations without granting authority.
- [x] Add exact CLI preview/approval/rejection/reactivation/retirement commands.
- [x] Match and execute one unambiguous approved recipe before Worker dispatch.
- [x] Add local quarantine and two-repository global retirement.
- [x] Add MCP read/operate tools and receipt/status/doctor/cost visibility.
- [x] Add the model-free `ca` Plan-mode recipe review.
- [ ] Complete one clean supervised global-activation pilot on a host whose production isolation probe passes.

## Surprises & Discoveries

- Existing `candidate` / `trusted` / `auto-runnable` values are advisory string
  labels. They are not authority and must remain unapproved during migration.
- Current harvest records already use typed `VerificationCommand` values and
  stable command fingerprints, so v1 can reuse the command validator rather
  than parse or execute arbitrary shell text.
- Required verification isolation now uses one private source projection per
  plan, scrubbed process state, offline dependencies, bounded result metadata,
  and a deny-default macOS sandbox. Approval and execution fail closed when its
  production capability probe is unavailable; tests use an explicit debug-only
  isolation double and cannot promote the feature out of pilot status.

## Decision Log

- V1 is evidence-only: no product-file writes or learned source transformations.
- The approved catalog is machine-local under `~/.codex-team/learning/`.
- One successful observation creates a proposal; approval is still explicit.
- Approval is global to the machine and matching repositories activate without
  another human approval.
- Execution requires an admitted evidence-only task; there is no background,
  maintenance, watch-triggered, or spontaneous execution.
- One real command failure quarantines the recipe revision in that repository.
  Failures in two distinct repository fingerprints retire that revision globally.

## Context and Orientation

- `src/app.rs` currently harvests `DeterministicRecipe` files, constructs route
  scorecards, and owns the pre-Worker deterministic lanes.
- `src/verification/recipe.rs` owns typed command intents and
  `src/verification/runner.rs` owns command execution.
- New catalog and lifecycle logic belongs in `src/learning/`, with `app.rs`
  limited to orchestration calls.
- `src/cli.rs` owns the new `recipe` command group; `src/mcp/server.rs` owns the
  pinned-repository MCP surface; `src/app/receipts.rs` owns additive status and
  doctor fields.
- The target UI package is the workspace root at `/Users/aji/project/ca`;
  this orchestrator package is `/Users/aji/project/ca/crates/codex-team`.

## Plan of Work

First restore an all-target build and add required isolated verification using
a private source projection, scrubbed environment, no-network sandbox policy,
bounded output, and preflight checks before role traffic. Then introduce typed
learning records and a hardened global catalog with immutable revisions and
digest-bound mutations. Convert the current harvest output into proposals while
loading legacy recipe files as unapproved observations.

Add the CLI and MCP contracts, then route only admitted evidence-only tasks
through one exact, unambiguous applicable approved recipe. Persist activation
and execution receipts, re-evaluate compatibility before every run, quarantine
real failures locally, and retire after failures in two distinct repositories.
Finally add the host-rendered `ca` review screen and run cross-repository tests.

## Concrete Steps

1. Fix current compilation, add isolation capability/preflight, and require it
   from `step`, `run`, doctor, MCP operate, approval, and recipe execution.
2. Add strict schemas/types for proposal, approved revision, activation, and
   execution receipts; harden `~/.codex-team/learning/` publication.
3. Harvest first-success proposals, migrate v1 observations, and implement
   deterministic matching/drift fingerprints.
4. Add `codex-team recipe list|preview|approve|reject|reactivate|retire`.
5. Insert the approved evidence lane after task admission and before Worker.
6. Add MCP tools and additive learning summaries.
7. Add a provider-free `ca` Plan-mode review and action handling.
8. Complete focused/full/release/cross-process gates and one disposable pilot.

## Validation and Acceptance

- Existing configs and repositories with no approved recipe behave identically.
- A first successful allowlisted chain creates one stable proposal without
  granting authority.
- Approval is digest/revision bound, validates inside isolation, and publishes
  one immutable global revision without raw prompts/output/secrets/paths.
- A second compatible repository auto-activates the recipe only for an admitted
  evidence-only task and makes zero provider calls.
- Drift deactivates without a quality failure. A real failure quarantines
  locally; a second distinct-repository failure retires globally.
- Cancellation, timeout, environment failure, and isolation failure stop safely
  without AI fallback or erroneous retirement.
- Focused learning/isolation/CLI/MCP/UI tests pass, followed by both repositories'
  documented fmt, strict Clippy, full test, release, and compatibility gates.

## Idempotence and Recovery

Catalog revisions and approval/rejection/retirement receipts are immutable and
published with create-new/atomic no-follow writes. Exact replay returns the
existing receipt. Partial activation/execution writes fail closed and can be
reconstructed from the immutable execution receipt. No command resets source,
clears quarantine silently, or rewrites legacy observations in place.

## Artifacts and Notes

Expected source artifacts include strict JSON schemas, repository activation
receipts below `.orchestrator/recipes/`, machine-global approved revisions below
`~/.codex-team/learning/`, MCP structured results, and deterministic UI
snapshots. Artifacts contain identifiers, hashes, bounded summaries, and counts
only—never credentials, prompts, raw output, environment values, or canonical
repository paths.

## Interfaces and Dependencies

Public CLI: `codex-team recipe list|preview|approve|reject|reactivate|retire`.
MCP adds repository-filtered list/preview in observe mode and exact human-gated
mutations in operate mode. `ca` uses its existing `McpManager`, Plan-mode visual
language, clipboard toast, and host-side actions; approval must not call a model.

## Outcomes & Retrospective

The implementation and automated gates are complete. Typed learning records,
machine-private catalog storage, digest-bound approval, exact applicability,
automatic activation, drift handling, quarantine/retirement, CLI/MCP receipts,
and the model-free `ca` cockpit are present. The final orchestrator suite passes
508 library tests (one documented ignore), 1 import foundation test, 1 manual
commit CLI test, 5 MCP SDK tests, and 3 step-cycle tests. Formatting, strict
all-target Clippy, release build, whitespace, snapshots, and protected trees
pass. Both real-binary compatibility directions pass using disposable state.

The feature remains pilot-only because the clean supervised global-activation
pilot requires a host on which the production isolation capability probe—not
the test double—passes. No live provider, real profile, installed binary,
cross-machine catalog, or background execution was used.
