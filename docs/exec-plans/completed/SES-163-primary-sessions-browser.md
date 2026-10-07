# SES-163 — Primary Sessions Browser

Status: COMPLETE
Owner/session: `/root`
Last updated: 2026-08-23

## Goal

Make `/sessions` the primary discoverable command for browsing and continuing
former saved conversations while preserving `/resume` compatibility and the
singular `/session` diagnostics report.

## Non-Goals

- No session storage, schema, ordering, profile, lazy-history, or startup-flag
  changes.
- No new keyboard shortcut and no change to session mutation authority.
- No oracle edits, copied implementation, or Cursor parity claim.

## Evidence and Current Behavior

- `src/tui/commands.rs` exposes `/resume [session-id]` for the saved-session
  picker and promotes `/session` current-session diagnostics in common command
  discovery. `/sessions` does not resolve.
- `src/main.rs` already opens the searchable Active/Archive picker or directly
  loads a supplied ID through `CommandId::Resume`.
- Picker rendering and input independently filter `state.session_list`, and
  currently include the current saved session with mutation guards.
- Read-only MiMoCode oracle evidence registers canonical `/sessions` with
  `resume` and `continue` aliases and opens its session-selection dialog. This
  informs vocabulary and interaction only.

## Scope

Allowed: command registry/routing, session-picker projection/input/rendering,
focused tests/snapshots, and current harness documentation.

Forbidden: `oracles/`, `reference/`, real session/config/credential files,
provider traffic, and installed binaries.

## Design and Invariants

- Canonical `/sessions [session-id]` reuses `CommandId::Resume`; `/resume` and
  `/continue` are aliases. Startup flags remain unchanged.
- `/session` remains an Always, read-only, argument-free current-session report
  but is removed from common-command priority.
- The Active picker projection excludes the current session UUID before count,
  query filtering, navigation, selection, and action mapping. Archive rows are
  unchanged. Existing current-session mutation rejection remains defense in
  depth.
- Only-current plus empty archive reports `No previous sessions`; an archive
  entry still opens the picker with Active count zero.
- Direct-ID loading remains accepted through all three command spellings.

## Milestones

### M1 — Characterize vocabulary and projection

- Add failing command tests for canonical `/sessions`, both aliases, singular
  separation, and common-command priority.
- Add failing orchestration/input/render tests for only-current, archive-only,
  current-row exclusion, counts, navigation, and action mapping.

### M2 — Implement the command and picker behavior

- Update the command registry and descriptions without adding a new command ID.
- Share one former-session projection rule across input and rendering.
- Update empty-state guidance and preserve direct-ID/bare/no-session behavior.

### M3 — Validate and reconcile

- Run focused tests and review wide/narrow command/session snapshots.
- Run the workspace test, format, strict Clippy, release, diff, snapshot, and
  help/version gates.
- Update authoritative harness documents, complete the task record, and move
  this plan to `completed/`.

## Acceptance Criteria

- `/sessions` is the visible canonical saved-session command; `/resume` and
  `/continue` execute the same action and `/session` remains separate.
- Common empty-query discovery promotes `/sessions`, not `/session`.
- The Active tab never renders, counts, navigates to, or mutates the current
  session; Archive remains reachable when Active has no former sessions.
- Direct saved-session IDs still load, persistence-disabled runs retain useful
  guidance, and all existing storage/security invariants pass.

## Validation

Focused: command registry, main command orchestration, session-picker input,
and session render tests/snapshots.

Completion: `cargo test --workspace --no-fail-fast -- --quiet`,
`cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets -- -D warnings`,
`cargo build --workspace --release`, `git diff --check`, snapshot hygiene, and
release help/version smokes.

## Progress

- [x] Traced current command routing, session storage/picker behavior, and
  read-only MiMoCode evidence.
- [x] Added failing characterization tests.
- [x] Implemented canonical `/sessions` and former-only projection.
- [x] Passed focused validation and reviewed snapshots.
- [x] Passed completion gates and reconciled documentation.

## Decisions

- Decision: retain `/session` but demote it.
  Evidence: user selected compatibility without common-command priority.
  Consequence: current-session diagnostics remain available without obscuring
  the frequently used history browser.
- Decision: exclude the current session from Active picker rows.
  Evidence: user explicitly selected former-only behavior.
  Consequence: visible counts and every selection operation must use the same
  projected list.
- Decision: use MiMoCode only for vocabulary and interaction evidence.
  Evidence: repository rules keep oracles read-only and prohibit copying.
  Consequence: `ca` retains its existing storage, archive, and safety model.

## Surprises and Discoveries

- The required browser already exists under `/resume`; the feature gap is
  command naming/discovery plus a consistent former-only projection.
- The working tree contains overlapping user-owned command/picker changes, so
  edits must stay surgical and avoid broad rewrites.

## Recovery and Rollback

Use targeted patches limited to SES-163-owned hunks. Tests use in-memory state
and temporary session stores; no real session data is touched.

## Completion Record

Changed files: command registry/routing, shared app projection, picker
input/render tests and snapshots, main orchestration tests, and authoritative
harness documents. Session storage/schema and startup flags are unchanged.

Commands and results: focused command/input/render/session tests passed; the
final `cargo test --workspace --no-fail-fast -- --quiet` passed 2,478 tests
with three ignored; formatting, strict workspace/all-target Clippy, release
workspace build, diff/snapshot hygiene, and release help/version smokes passed.

Remaining risks: no live TUI walkthrough used real saved sessions. Deterministic
render/input/orchestration tests cover the user-visible and selection behavior,
and no installed binary was changed.
