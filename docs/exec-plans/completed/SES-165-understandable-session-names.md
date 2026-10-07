# SES-165 — Understandable Session Names

Status: COMPLETE
Owner/session: Codex implementation session
Last updated: 2026-08-23

## Goal

Make `/sessions` read like a conversation list instead of a collection of
prompt fragments and paths. New sessions receive a concise generated name after
their first response, while existing sessions can be repaired explicitly from
the browser.

## Non-Goals

- No session schema, identity, ordering, profile, or history-format migration.
- No provider request while merely opening or searching `/sessions`.
- No batch rewrite of saved sessions, installed-binary change, or real-session
  mutation during validation.
- No oracle source copying or Cursor-parity claim.

## Evidence and Current Behavior

- SES-164 made titles primary, grouped them by date, and added manual rename,
  but its deterministic first-request fallback remains visibly path-like and
  verbose in the user's rendered browser.
- The user-provided example presents short semantic names, readable dates, and
  plain-language controls.
- The read-only MiMoCode oracle uses a model-produced title for a conversation's
  first request. This is behavioral evidence only.
- `ca` already has isolated, tool-free provider request and usage-accounting
  patterns; session persistence already supports exact UUID-checked rename.

## Scope

Allowed files:

- `src/{session,app,main,request,usage,provider_trace}.rs`
- `src/tui/{commands,input,render,render_tests,input_tests}.rs` and affected
  session snapshots
- this plan and current repository harness documents

Forbidden files/areas: `oracles/`, `reference/`, user runtime state, installed
binaries, Git history, unrelated dirty work.

## Design and Invariants

- Keep the immediate bounded local title so the first save never depends on a
  second network request. Mark only that fallback as eligible for one generated
  replacement; explicit `--name`, `/name`, rename, and resumed titles win.
- Generate a 3–8 word, terminal-safe title with a separate tool-free request
  after the first completed provider response. Failure keeps the local fallback
  and never fails or delays the main turn.
- Apply an asynchronous result only when its session ID and process-local
  generation still match. Account for the physical request separately.
- Existing sessions change only after an explicit browser action. Load their
  full first request off the TUI thread, generate one title, then persist through
  exact active/archive UUID checks and refresh both projections.
- The picker labels former active sessions as `Recent`, uses `Archived`, exposes
  a visible search affordance, and describes controls in words. Filtering,
  navigation, and actions continue to share one projected row list.

## Milestones

### M1 — Characterize title quality and UI language

- Add focused tests for generated-title cleanup, poor-title detection, explicit
  generation input mapping, and readable wide/narrow browser presentation.

### M2 — Implement asynchronous naming

- Add current-session title provenance, isolated title requests, stale-result
  checks, exact stored-session repair, and failure behavior.

### M3 — Validate and reconcile

- Run focused session/input/render/request/usage checks, the full workspace
  suite, formatting, strict all-target Clippy, release build, snapshot/diff
  hygiene, and help/version smokes.
- Update architecture, parity-extension, continuity, testing, index, and next
  steps; move this plan to completed.

## Acceptance Criteria

- New unnamed sessions keep an immediate local fallback and replace it once
  with a concise generated title after the first response.
- Explicit or resumed names cannot be overwritten by a late title result.
- A highlighted active or archived former session can generate and persist a
  better name without changing any other session.
- Opening and searching `/sessions` never triggers provider traffic.
- The browser says `Recent`/`Archived`, makes search discoverable, and uses
  readable action/navigation labels in wide and narrow layouts.
- No oracle/reference, runtime session, config, or unrelated source is changed.

## Validation

Focused: PASS. Bounded title subject/cleanup/repair heuristics, process-local
stale-result application, tool-free request recording and usage, exact archived
session repair, projected Ctrl+G targeting, and all session picker input/render
tests pass.

Completion: PASS. The final workspace run passed 2,492 tests with three
documented ignores. Formatting, strict workspace/all-target Clippy, release
workspace build, release help/version smokes, diff hygiene, and snapshot hygiene
pass.

Manual/visual: reviewed deterministic 100×30 and 60×24 Recent, Archived, empty,
busy, confirmation, failure, and rename snapshots against the supplied examples.

Blocked checks: none currently.

## Progress

- [x] User screenshot and current SES-164 implementation compared.
- [x] Current title derivation, picker projection, exact rename, stream event,
  and isolated provider-request paths traced.
- [x] Failing focused characterizations added.
- [x] Naming and picker refinements implemented.
- [x] Completion gates and harness reconciliation complete.

## Decisions

- Decision: generated naming is asynchronous and best-effort.
  Evidence: the user needs semantic names, while the main turn and first save
  must remain reliable when the provider is unavailable.
  Consequence: the local fallback may appear briefly and remains on failure.
- Decision: legacy repair is explicit per selected session.
  Evidence: opening the browser should be read-only and should not fan out
  hidden provider requests or rewrite a user's archive.
  Consequence: poor legacy names display a repair hint and can be fixed with one
  shortcut; manual rename remains available.

## Surprises and Discoveries

- The SES-164 screenshot confirms that better layout cannot compensate for raw
  first-prompt titles; the naming source itself must change.

## Recovery and Rollback

Revert only SES-165 hunks and generated snapshot updates. Do not reset the dirty
worktree. Generated titles change only metadata and preserve session identity,
messages, storage keys, and archive location.

## Completion Record

Changed files: session/title helpers and exact archived loading in
`src/session.rs`; process-only title state in `src/app.rs`; isolated request,
usage, stream orchestration, persistence, and tests in `src/main.rs`;
`SessionTitle` accounting labels in `src/{request,usage,provider_trace}.rs`;
Ctrl+G and readable picker presentation in `src/tui/`; affected session/usage
snapshots; and the current repository harness documents.

Commands and results: focused title/input/render/session suites pass; the final
workspace run passed 2,492 tests with three documented ignores; formatting,
strict Clippy, release build, diff/snapshot hygiene, and both release binary
help/version smokes pass. The only diagnostic is the existing macOS debug-linker
compact-unwind warning.

No live provider, credentials, real runtime session/configuration, installed
binary, oracle, or reference content was accessed or changed.
