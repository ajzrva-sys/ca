# LRN-168 — Guided, Copyable Learning Review

Status: COMPLETE
Owner/session: Codex / current implementation session
Last updated: 2026-08-23

## Goal

Replace digest retyping with a responsive in-TUI evidence and proposal review
flow, while making pager text directly selectable and copyable.

## Non-Goals

- Do not activate the background learning service or auto-register evidence roots.
- Do not weaken exact evidence/proposal binding or expand publication beyond
  memory and inert skills.
- Do not change the persisted archive-learning schema or remove the existing CLI.
- Do not mutate runtime learning state, `oracles/`, or `reference/` during validation.

## Evidence and Current Behavior

- `src/codex_learning.rs::run_tui_command` performs sync/review work inline and
  returns only rendered text; the TUI exposes no typed review actions.
- Evidence and proposal renderers print long exact CLI commands, and approval
  re-reads revision-bound sources before provider processing.
- `src/tui/input.rs::handle_pager` only scrolls/closes; mouse selection is always
  routed to the conversation frame captured before the pager is rendered.
- TUI-145 already provides bounded Unicode-safe visible-cell selection and uses
  the in-process `arboard` dependency.
- The screenshots reproduce both failures: the pager command cannot be selected,
  and retyping it after a live archive append fails with source drift.

## Scope

Allowed files: learning/TUI orchestration and state, focused tests/snapshots,
this plan, and current architecture/continuity/testing/queue/index documents.

Forbidden files/areas: runtime profile data, source archives, installed binaries
or services, unrelated dirty work, `oracles/`, `reference/`, and Git history.

Runtime state affected: none during implementation. A later user-confirmed TUI
review may update the existing learning index, provider-backed inert proposal,
and memory/skill journals exactly as the CLI already does.

## Design and Invariants

- Carry exact evidence/proposal digests in typed process-local TUI state; every
  mutation revalidates that digest inside the archive-learning store.
- Execute scans, pricing/materialization, provider generation, and proposal
  decisions on a dedicated async task with generation-based stale-result rejection.
- A pending proposal is rendered without syncing; otherwise review load syncs
  registered roots and prepares the priced revision.
- Evidence/route drift triggers one automatic sync/re-price and returns a fresh
  unapproved view. A second drift stops with an explicit refresh action.
- Evidence and proposal choices are Later-first. Proposal scopes are all,
  memory-only, skills-only, and reject-all.
- The shared pager captures only its visible content body for drag selection;
  Copy All copies the complete semantic pager content. Conversation selection,
  scroll behavior, and CLI output remain compatible.
- Closing a busy pager does not abort approved work or steal focus when it
  finishes; completion remains discoverable through `/learn review`.

## Milestones

### M1 — Typed learning review boundary

- Add view/request/result/error types and store-backed TUI operations.
- Add source/route drift refresh behavior and proposal scope decisions.

### M2 — Async TUI workflow

- Add dedicated request/generation/busy state, task orchestration, action
  confirmations, and closed-pager completion handling.

### M3 — Copyable pager

- Extract/reuse the bounded visible-cell selector for the pager, add Copy All,
  and render contextual learning hints.

### M4 — Validation and reconciliation

- Add focused fixtures/input/render coverage, run completion gates, reconcile
  durable docs, and move this plan to completed.

## Acceptance Criteria

- `/learn` offers Enter-to-review and `/learn review` opens a fresh priced
  evidence review without blocking the TUI or requiring a copied digest.
- Approval produces an inert proposal; all/memory/skills/reject/later decisions
  are exact-bound and partial application displays the remaining digest.
- Drift never inherits approval, never contacts a provider for the stale
  revision, and retries automatically at most once.
- Pager drag copies exact visible Unicode text; Copy All includes off-screen
  content and excludes borders/hints/hidden conversation text.
- Existing CLI commands, storage, service-off state, and protected evidence
  trees remain unchanged.

## Validation

Focused: archive-learning typed workflow, input/selection, orchestration, and
wide/narrow render snapshots.

Completion: `cargo test --workspace --no-fail-fast`, `cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets --all-features -- -D warnings`,
`cargo build --workspace --release --all-features`, diff/snapshot hygiene, and
protected-tree status.

Manual/visual: deterministic TestBackend snapshots; no runtime profile or live
provider/service exercise.

Blocked checks: live provider processing is not required and will not run.

## Progress

- [x] Current review, pager, selection, drift, and async task boundaries traced.
- [x] Typed learning review boundary implemented.
- [x] Async guided workflow implemented.
- [x] Pager selection/copy implemented.
- [x] Focused and completion validation recorded.
- [x] Harness reconciled and plan archived.

## Decisions

- Decision: retain two explicit checkpoints while removing command/digest entry.
  Evidence: the user selected the guided two-step option.
  Consequence: evidence approval and proposal application remain separate.
- Decision: expose scope choices rather than per-item selection.
  Evidence: the user selected Apply all, Memory only, Skills only, Reject all,
  or Later.
  Consequence: item-level CLI selection remains available as a copyable fallback.
- Decision: refresh once on drift and require a new approval.
  Evidence: live append-only archives make the current retype loop impractical.
  Consequence: freshness improves without transferring authority.

## Surprises and Discoveries

- The existing pager overlays the captured conversation selection frame, so a
  drag over pager text can target hidden conversation cells instead.
- Review source verification scans registered roots synchronously from the TUI
  command path, contrary to the event-loop responsiveness invariant.

## Recovery and Rollback

Remove only LRN-168 typed state/actions, pager-selection composition, focused
tests/snapshots, and documentation hunks. Do not reset, clean, stash, or replace
the surrounding dirty/untracked integration.

## Completion Record

Changed files: `src/codex_learning.rs`, `src/app.rs`, `src/main.rs`,
`src/tui/{commands,input,render,render_tests}.rs`, six reviewed learning pager
snapshots, and the current harness/plan documents.

Commands and results: 32 focused learning tests, three learning-orchestration
tests, 170 render tests, and 175 input tests pass. The final workspace run
passes 2,517 tests with three manual-only ignores. Formatting, strict
workspace/all-target/all-feature Clippy, the all-feature release build,
diff/snapshot hygiene, and protected-tree checks pass.

Remaining risks: live provider processing and a real terminal clipboard were
not exercised because that would require credentials or runtime interaction;
the provider boundary uses the existing fake backend and the clipboard boundary
is injected in deterministic tests. The recurring macOS debug-linker
compact-unwind warning is unchanged.

Harness updates: `ARCHITECTURE.md`, `CONTINUITY.md`, `TESTING.md`,
`NEXT_STEPS.md`, and `docs/.INDEX.md` record the typed async workflow, exact
authority/drift rules, pager-copy contract, validation, and completed plan.
