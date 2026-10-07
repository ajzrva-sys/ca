# SES-164 — Named Session Browser

Status: COMPLETE
Owner/session: Codex implementation session
Last updated: 2026-08-23

## Goal

Make `/sessions` present saved conversations as named, date-grouped rows with
useful timestamps and allow a highlighted saved session to be renamed without
exposing storage paths as the primary presentation.

## Non-Goals

- No session schema, storage-key, ordering, profile, or lazy-history migration.
- No oracle source copying or Cursor-parity claim.
- No removal of Active/Archive, pin, restore, archive, or delete behavior.
- No installed-binary or real saved-session mutation.

## Evidence and Current Behavior

- `src/session.rs::automatic_session_title` currently collapses the first user
  message verbatim, so attachment envelopes can become the saved title.
- `src/tui/render.rs::render_session_picker` forces every entry into a two-row
  title plus public-ID/workspace presentation inside a 60-column dialog.
- `src/tui/input.rs::handle_session_picker` supports resume, pin, archive,
  restore, and delete, but no rename operation.
- The read-only MiMoCode session list groups rows by date, uses the title as the
  primary label, shows the updated time at the right, and exposes rename. Its
  first-request title generation is behavioral evidence only; no source will be
  copied.
- Inferred: extracting explicit user-request envelopes before falling back to
  the existing bounded local title makes `ca`'s offline naming substantially
  more useful without adding provider cost or failure modes.

## Scope

Allowed files:

- `src/session.rs`, `src/app.rs`, `src/main.rs`
- `src/tui/{commands,input,render,render_tests}.rs` and affected snapshots
- this plan and current repository harness documents

Forbidden files/areas: `oracles/`, `reference/`, user runtime state, installed
binaries, Git history, unrelated dirty work.

Runtime state affected: only normal explicit rename writes to the selected
saved-session metadata; automated tests use temporary stores.

## Design and Invariants

- Prefer an explicit `My request:` envelope when deriving the initial bounded
  local title; attachment declarations and document-handling boilerplate are
  not session names. Explicit `--name` and `/name` remain authoritative.
- Render one selectable row per summary. Group rows by local calendar date,
  label the current day `Today`, and right-align local updated times when width
  permits. IDs and workspace paths remain searchable, not primary chrome.
- Keep one shared projected list for display, filtering, navigation, and all
  actions. The current active session remains excluded before projection.
- Rename is explicit, bounded, terminal-sanitized, UUID-checked, atomic, and
  works in Active and Archive. Empty names are rejected without closing the
  editor. Persistence stays off the TUI event loop.

## Milestones

### M1 — Characterize naming and browser presentation

- Add focused tests for request-envelope extraction, date groups, one-row
  titles/times, hidden metadata, and rename input mapping.
- Validation: focused session/render/input tests fail for the intended gaps.

### M2 — Implement naming, rendering, and rename

- Patch the title heuristic, browser renderer, rename state/input/action, and
  exact store mutation with temporary-store coverage.
- Validation: focused suites and reviewed wide/narrow snapshots pass.

### M3 — Complete validation and handoff

- Run full workspace tests, formatting, strict all-target Clippy, release build,
  diff/snapshot hygiene, and help/version smokes.
- Reconcile architecture, parity-extension, continuity, testing, index, and
  next-step documents; then move this plan to completed.

## Acceptance Criteria

- Attachment/path envelope text does not displace an explicit `My request:`
  title, and titles remain nonempty, Unicode-safe, and bounded.
- `/sessions` shows date headings, one descriptive title per row, and a
  right-aligned time; public IDs/workspaces do not occupy a second row.
- Filtering and all existing actions still target the highlighted projected
  summary across date boundaries.
- Rename opens with the saved title, supports editing/cancel/submit, rejects an
  empty value, and atomically updates only the UUID-checked active or archived
  session.
- No oracle/reference or unrelated source is changed.

## Validation

Focused: PASS. The automatic-title, exact active/archive store rename, picker
rename input, empty-name refusal, orchestration application, projected picker,
and all 107 matching session tests pass. All affected deterministic render
snapshots pass.

Completion: PASS. The final workspace run passed 2,484 tests with three
documented ignores. Formatting, strict workspace/all-target Clippy, release
workspace build, diff hygiene, snapshot hygiene, and release help/version
smokes pass.

Manual/visual: reviewed deterministic 100×30 and 60×24 Active, Archive, busy,
empty, confirmation, failure, and rename snapshots against the user-provided
example.

Blocked checks: none currently.

## Progress

- [x] Existing naming, persistence, picker projection, rendering, input, and
  read-only MiMoCode behavior traced.
- [x] Failing focused characterizations added.
- [x] Naming/browser/rename implementation complete.
- [x] Completion gates and harness reconciliation complete.

## Decisions

- Decision: preserve offline local auto-naming and improve envelope extraction
  instead of adding a second provider request.
  Evidence: SES-140 intentionally made naming immediate and deterministic;
  this request can be satisfied by presenting names correctly and adding manual
  correction where the local inference is imperfect.
  Consequence: no extra latency, cost, credential dependency, or provider
  failure can block the first turn.
- Decision: retain `ca`'s Active/Archive and mutation safety while adopting the
  example's name/date/time information hierarchy.
  Evidence: those controls are current product behavior and the request targets
  readability/naming rather than storage semantics.
  Consequence: the browser looks like the example without regressing archive,
  restore, or pin workflows.

## Surprises and Discoveries

- SES-140 recorded the local heuristic's reduced polish as an explicit risk;
  the current screenshot demonstrates that risk when envelope/path text wins.

## Recovery and Rollback

Revert only SES-164 hunks and its generated snapshot updates. Do not reset the
dirty worktree. Rename does not change identity or file format.

## Completion Record

Changed files: `src/session.rs`, `src/app.rs`, `src/main.rs`,
`src/tui/{commands,input,render,render_tests,test_support}.rs`, affected session
snapshots, this plan, and current index/architecture/parity/continuity/testing/
next-step documents.

Commands and results: focused naming/rename/session/render suites passed; the
final workspace run passed 2,484 tests with three ignored; formatting, strict
Clippy, release build, diff/snapshot hygiene, and both release help/version
smokes passed. One broad render run exposed an accidental unrelated fixture
timestamp edit made during this task; the exact byte was restored and its
focused plus final workspace reruns passed.

Remaining risks: deterministic local first-request naming is intentionally
less polished than a second provider-generated title, but it adds no cost,
latency, credential dependency, or failure mode and every saved row is now
explicitly renameable.

Harness updates: SES-164 is recorded complete in the repository index,
architecture, parity extensions, continuity, testing, and next-step documents.
No provider traffic, real saved-session/config mutation, installed-binary
change, or oracle/reference mutation occurred.
