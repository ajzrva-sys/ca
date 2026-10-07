# WFL-222 — Audited Built-in Workflows and Structure Inspection

Status: COMPLETE
Owner/session: ORC-218 goal session
Last updated: 2026-08-29

## Goal

Add immutable CA-native `deep-research`, `fact-check`, and
`research-experiment` workflow templates over the existing declarative engine,
with strict preflight, no executable workflow language, and a safe structural
tree view for definitions and live runs.

## Non-Goals

- No JavaScript, arbitrary plugin runtime, automatic commit/push/merge, hosted
  research service, provider-route discovery, or ambient network authority.
- No user definition may replace a `builtin:` identity.
- No experiment may mutate outside a clean attached CA-owned worktree or
  retain a candidate without host-observed evaluation success.
- No provider or network traffic is used by deterministic validation.

## Evidence and Baseline

- `src/workflow.rs` already validates bounded TOML/JSON DAGs, routes, restrictive
  tool intersections, scoped grants, evidence gates, retries, outputs, and
  deterministic step scheduling.
- `/workflow` already provides a Cancel-first start/resume preview, session-safe
  run state, and explicit pause/resume/clear controls.
- AGT-157 owns host-created verification receipts and worktree lane authority;
  REV-221 owns dual-route acceptance. WFL-222 consumes those boundaries and
  does not redefine them.
- ORC-218 baseline: 868 status records, SHA-256
  `2dcaf84dc86d3ae27bfea593bc811ce16f46f9782382780901fdcf890ff39978`.

## Design

- Built-ins are constructed from private Rust definitions, validated through
  the same `WorkflowDefinition` path, serialized canonically for a stable
  digest, and exposed only as `builtin:<id>`.
- `templates` lists safe phase/authority metadata. `inspect` renders the
  immutable definition tree. `tree` renders either the live run or an explicit
  built-in with dependencies, attempts, routes, gates, budget ceilings, pause
  reason, and terminal state without prompts or outputs.
- `deep-research` uses six ordered phases and makes network tools available
  only in research phases; every actual network operation remains subject to
  ordinary explicit approval. Optional output stays within the workspace and
  uses an exact path-scoped grant.
- `fact-check` requires exactly three explicit pairwise-distinct configured
  provider/model juror routes. Each juror receives a fresh request projection
  containing only the common system boundary and its identical bounded
  evidence prompt; canonical conversation history, prior juror verdicts,
  attachments, memory recall, and pending diff context are excluded. The
  deterministic scheduler runs the ready jurors without cross-juror context,
  then a final report consumes all three bounded outputs.
- `research-experiment` requires an attached clean CA-owned worktree, 1–10
  iterations, bounded relative editable globs, and an exact digest-bound
  evaluation command. The workflow exposes only those mutation/evaluation
  scopes. Failed candidates are instructed to revert only owned edits; final
  transfer remains separate. No definition can run arbitrary script bytes.

## Validation

Add failure-first unit/command/render coverage for immutable identities,
definition digests, route/tool/network preflight, three-juror distinctness,
worktree cleanliness, editable globs, evaluation digest/iteration bounds,
tree state, pause/resume/cancel, and no arbitrary runtime. Then run focused
workflow suites, formatting, strict all-feature Clippy, all-feature release,
diff/snapshot hygiene, and direct oracle/reference inspection.

## Progress

- [x] Child plan and queue entry created after REV-221 completion.
- [x] Failure-first template/preflight/tree coverage added.
- [x] Immutable templates and safe instantiation implemented.
- [x] Command and TUI tree/preview surfaces implemented.
- [x] Focused suites and child completion gates pass.
- [x] Harness reconciled and plan completed.

## Completion Record

Changed paths: `src/workflow.rs`, `src/app.rs`, `src/main.rs`,
`src/tui/commands.rs`, four reviewed built-in workflow tree snapshots, and the
WFL-222/ORC-218 harness records. No oracle, reference, provider, profile,
session fixture outside disposable roots, protected subtree, or installed
binary was changed.

Focused validation passed: 40 workflow-path tests; 16 core workflow tests;
request-projection isolation; exact evaluation-command rejection before
approval; command parsing; and deterministic wide/narrow definition/live-tree
snapshots. Formatting, strict workspace all-target/all-feature Clippy, the
workspace all-feature release build, `git diff --check`, zero `.snap.new`, and
direct `oracles/`/`reference/` inspection pass. The existing macOS oversized
`__eh_frame` linker warning remains non-fatal. Complete workspace matrices and
installation remain owned by REL-229.

## Recovery

Reverse only WFL-222-labelled hunks and delete only WFL-created paths with
`apply_patch`. Do not reset, clean, mutate oracle/reference trees, contact a
provider, or install a binary.
