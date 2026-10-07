# CFG-132 — Recent and Favorite Models

Status: COMPLETE
Owner/session: Codex / current user-requested implementation
Last updated: 2026-08-16

## Goal

Add bounded recent/favorite model shortcuts that remain profile-scoped,
deduplicated, compatible with changing model catalogs, and process-only in bare
mode where required.

## Non-Goals

- Do not change provider configuration, route eligibility, one-turn override,
  model discovery, or existing cycle-all behavior.
- Do not surface unavailable models, persist bare favorites, add a drawer, or
  begin SES-133/LRN-134.

## Evidence and Current Behavior

- `src/app.rs::ModelInfo` is the provider/model/display identity consumed by
  current route state and the picker.
- `src/tui/input.rs::handle_model_picker` filters one flat `model_list`, and
  `dispatch_key_action` cycles the complete list for `CycleModel`.
- `src/tui/render.rs::render_model_picker` renders that same flat list with
  current-model status and contextual provider editing.
- `src/main.rs::execute_input_action` is the persistent model-selection
  boundary; bare mode already selects without config writes.
- Profile data is resolved through `src/runtime.rs`; bounded owner-only atomic
  stores already exist in `src/attachments.rs` and `src/prompt_stash.rs`.

## Scope

Allowed files: a typed model-preference store, runtime/app/keymap/model-picker
state and orchestration, focused tests/snapshots, and harness documents.

Forbidden files/areas: `oracles/`, `reference/`, provider credentials/routes,
session schema, model catalog network behavior, installed binary, and user
runtime state outside disposable validation.

Runtime state affected: one bounded profile data file containing only
provider/model identity, favorite state, and recency metadata. Bare retains at
most eight recent identities in memory and persists no favorites.

## Design and Invariants

- Store a closed versioned document with unique `(provider, model)` keys,
  favorite boolean, and bounded recency ordering/time metadata only.
- Retain at most eight recent models. A selection moves one available identity
  to newest without duplicating it.
- Build a typed visible-row projection with unique Favorites and Recent
  sections before provider-group rows. An identity appears at most once in the
  complete picker and unavailable stored identities are omitted, not deleted.
- Favorite toggling is explicit and limited to the current-route picker; review
  and multi-run selection remain authority-neutral.
- Add configurable `cycle_recent_model` and `cycle_favorite_model` actions;
  preserve `cycle_model` over the complete configured list.
- Ordinary selection keeps existing reasoning-default, one-turn cancellation,
  config persistence, parent-route update, and error behavior.

## Milestones

### M1 — Preference storage and row projection

- Characterize every model-selection entry point.
- Add bounded owner/symlink-safe atomic storage, compatibility/corruption
  behavior, process-only bare state, recency/favorite reducer, and unique
  available-row projection.

### M2 — Picker and key actions

- Render Favorites, Recent, and provider sections with stable navigation.
- Add explicit favorite toggle plus configurable recent/favorite cycling while
  retaining all-model cycling.

### M3 — Validation and completion

- Cover persistence, deduplication, unavailable/reappearing models, bare mode,
  old keymap/config compatibility, selection entry points, and wide/narrow
  snapshots.
- Run focused/full gates and a disposable native PTY, reconcile the harness,
  archive this plan, and activate SES-133.

## Acceptance Criteria

- Favorites and Recent precede provider groups and contain no duplicate rows.
- Missing models remain stored but invisible and reappear when configured.
- Recent selection is capped at eight and deterministic; favorites toggle only
  by explicit action.
- Recent/favorite chords cycle their own available unique lists; cycle-all is
  unchanged and an empty list is a safe no-op.
- Bare mode has in-memory recency only and refuses persistent favorite changes.
- Existing config/keymap/profile data loads without rewriting.

## Validation

Focused: eight storage/projection tests, focused picker/keymap/input tests, and
the end-to-end model-selection persistence test pass. Reviewed deterministic
wide/narrow snapshots cover the preference sections and the updated keymap.

Completion: `cargo test` passes 1,520 main and 37 CLI tests with one documented
ignore. `cargo test --all-targets --all-features --no-fail-fast` passes 1,549
main, 118 experiment, and 37 CLI tests with two documented ignores. Formatting,
strict all-target/all-feature Clippy, all-feature release build, release
help/version, diff, and snapshot-hygiene checks pass.

Manual/visual: a released native TUI in a disposable persistent profile added
`fake/one` to Favorites, selected `fake/three`, showed unique Favorites/Recent/
provider groups, cycled each scoped list, and retained the state after restart.
A bare restart loaded no persisted preference sections, refused favorite
mutation, and allowed process-only selection. The preference file digest was
unchanged by the bare run. The fixture moved recoverably to
`/Users/aji/.Trash/ca-cfg132-pty-20260816`.

Blocked checks: physical FreeBSD was not requested or run.

## Progress

- [x] Selection/storage seams fully characterized.
- [x] Bounded preference store and unique row projection complete.
- [x] Picker/favorite/cycle interactions and snapshots complete.
- [x] Native walkthrough, full gates, and harness complete.

## Decisions

- Decision: keep preference data separate from provider configuration.
  Evidence: preference state is presentation/navigation metadata and removed
  models must remain dormant without polluting configured routes.
  Consequence: preference writes cannot grant a route or provider authority.

## Surprises and Discoveries

- Favorite mutation must save before changing the live reducer, otherwise an
  unavailable store could present a favorite that was never persisted.
- Toggling a favorite reorders the projected rows, so selection is anchored by
  provider/model identity rather than the previous numeric row.

## Recovery and Rollback

The preference file is additive and contains no credentials or session data.
Rollback can stop reading it without rewriting provider configuration. Never
use destructive Git reset/checkout for recovery.
