# INI-118 — Safe `/init`

Status: COMPLETE
Owner/session: Codex / current reliability continuation
Last updated: 2026-08-16

## Goal

Add an approval-safe `/init` workflow that drafts at most 32 KiB of root
`AGENTS.md` from bounded local evidence using one tool-free model request,
previews it, and creates it only after explicit confirmation with `/undo`
integration.

## Non-Goals

- No overwrite, merge, multi-file initialization, tool-enabled model request,
  model-written executable workflow, branch/worktree, push, or installation.
- No oracle/reference reads, writes, or parity claim.

## Evidence and Current Behavior

- The baseline lacked `/init` and a project-bootstrap request kind.
- Workspace root selection, provider requests, modal rendering, and undo
  receipts remained their existing authorities.
- The completed uncommitted OBS-117 change set was retained intact.

## Scope

Production/tests/docs required for INI-118 were changed. `oracles/`,
`reference/`, user runtime configuration, sessions, credentials, and installed
binaries were not changed.

## Design and Invariants

- `/init` is visible, idle-only, argument-free, and targets only the canonical
  primary workspace root.
- Evidence is bounded, ignore-aware, excludes build/oracle/reference content,
  and is explicitly delimited as untrusted input to one tool-free
  `ProjectInit` request.
- Draft validation is deterministic; the complete preview defaults to Cancel.
- Target absence, workspace identity, and evidence fingerprint are revalidated
  before descriptor-relative create-new/no-follow publication.
- A successful Missing-to-UTF-8 mutation is added to the existing undo stack.

## Milestones

### M1 — Characterization

Completed with command, evidence, validation, modal, symlink/existing target,
and undo tests.

### M2 — Implementation

Completed with the request kind, evidence service, orchestration state,
preview, safe create, and undo receipt.

### M3 — Completion

Completed with reviewed snapshots, disposable loopback walkthrough, local and
physical-FreeBSD gates, reconciled harness documents, and plan archival.

## Validation

Focused:

- `cargo test project_init --no-fail-fast`: PASS, 7 tests.
- `cargo test init_ --no-fail-fast`: PASS, 8 tests.

Completion:

- Local default: PASS, 1,387 main + 36 CLI; one ignored.
- Local all features: PASS, 1,416 main + 118 experiment + 36 CLI; two ignored.
- Local format, strict Clippy, release, help/version, diff, snapshot hygiene:
  PASS.
- Physical FreeBSD focused/default/all-feature/format/Clippy/release/smoke/
  ELF/linkage gates: PASS. Counts are recorded in `TESTING.md`.

Manual/visual:

- PASS in a disposable 80x24 bare-profile TUI with a loopback provider. Each
  invocation made one request without tools. Default Cancel preserved target
  absence; explicit Create wrote the preview; `/undo` removed it.
- Six reviewed wide/narrow snapshots cover preparing, preview, and error.

Blocked checks: none.

## Progress

- [x] Baseline inspected and task activated.
- [x] Failing characterization tests added.
- [x] Implementation and focused validation complete.
- [x] Local/manual/FreeBSD completion gates complete.
- [x] Harness reconciled and plan archived.

## Decisions

- Target the canonical primary root and refuse every existing target type.
- Bind confirmation to a fingerprint of fresh bounded evidence.
- Use descriptor-relative no-follow/create-new publication on Unix and the
  strict create-new fallback elsewhere.

## Surprises and Discoveries

- A minimal ignore-aware walker dependency was required to honor repository
  ignore policy without accidentally traversing oracle, reference, dependency,
  or build trees.
- The disposable walkthrough confirmed the typed request remains tool-free at
  the serialized provider boundary, not only in orchestration state.

## Recovery and Rollback

Before confirmation there is no workspace mutation. After success, `/undo`
removes the file while its bytes remain unchanged. Source rollback is a normal
review/revert of named INI-118 changes; no reset, stash, or worktree is needed.

## Completion Record

Changed files: `src/project_init.rs`; request/usage identity; app/TUI command,
input, render, and orchestration integration; tests/snapshots; `ignore`
dependency; architecture and harness documents.

Remaining risks: generated instructions remain model output and therefore
require human review; the workflow intentionally refuses merge/update of an
existing `AGENTS.md`.

Harness updates: architecture, continuity, testing, index, and roadmap are
reconciled. INI-118 is complete; LSP-119 is active.
