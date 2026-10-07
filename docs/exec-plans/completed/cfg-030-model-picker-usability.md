# CFG-030 — Model Picker Usability and Visibility

Status: COMPLETE
Owner/session: Codex 2026-08-10
Last updated: 2026-08-10

## Goal

Make the populated provider/model picker genuinely usable: the advertised shortcut opens it, the current model is obvious, long catalogs remain navigable and visible, provider catalog health is actionable, and editing a discovered model does not fail as though it were already configured.

## Non-Goals

- Changing provider chat protocols or catalog contents.
- Adding mouse interaction or a GUI.
- Deleting providers/models or changing approval boundaries.
- Editing accessibility or VoiceOver behavior.
- Mutating user runtime configuration during validation or replacing an installed binary.

## Evidence and Current Behavior

- `src/tui/render.rs` advertises `Ctrl+K` as “switch model,” but `src/tui/input.rs` uses `Ctrl+K` for kill-to-end and opens the picker with undocumented `Ctrl+L`.
- Opening the picker always selects row zero instead of the current model.
- Down navigation increments without a bound. Enter can close the picker without selecting anything after the index passes the filtered result count.
- The renderer emits every model into a fixed 20-row popup without scroll state or a visible window; a live Ofox catalog currently contributes about 100 compatible rows.
- Rows do not distinguish the current model from the navigation cursor, and the status shows only aggregate provider counts, not the selected provider's live/fallback/failure state.
- `Shift+E` treats every selected row as configured. Saving a discovered-only row then fails because `ConfigStore::upsert_provider_model` cannot find the original ID in the provider's configured array.

## Scope

Allowed files:
- `src/app.rs`, `src/main.rs`
- `src/model_catalog.rs` only for bounded display-safe error classification if needed
- `src/tui/{input,input_tests,render,render_tests}.rs`
- relevant snapshots
- `ARCHITECTURE.md`, `CONTINUITY.md`, `TESTING.md`, `NEXT_STEPS.md`
- this ExecPlan

Forbidden files/areas:
- `oracles/`, `reference/`
- user runtime config/session files
- installed binary

Runtime state affected: none during validation. Explicit model selection and provider/model saves retain their existing persistence behavior.

## Design and Invariants

- `Ctrl+K` and `/model` share one picker-opening helper that clears the filter, highlights the current model, and requests the first catalog refresh when needed. `Ctrl+L` remains an undocumented compatibility alias for this release.
- Filtered selection is always bounded. Up/Down, Home/End, and PageUp/PageDown navigate model rows only.
- Rendering computes a fixed-capacity visible model window around the selected filtered row, keeping instructions visible at every supported terminal height.
- A position counter communicates selection and match count; a distinct `current` marker communicates the active runtime model.
- The selected provider receives one concise status line: configured, live, fallback, or unavailable, followed by a safe actionable reason when available.
- Provider errors shown in the UI are classified strings and never include request URLs, response bodies, or secrets.
- `Shift+E` on a configured row edits it; on a discovered/documented-only row it opens a prefilled add-to-provider form instead of a guaranteed failing rename.
- Interface copy remains terse, specific, and placed next to the affected picker row or field.

## Milestones

### M1 — Failing interaction characterization
- Add tests for `Ctrl+K`, current-row opening, bounded/long-list navigation, empty filtered results, and discovered-only edit setup.
- Add wide/narrow snapshots for current/provider status and a scrolled long catalog.

### M2 — Picker behavior and visibility
- Centralize picker opening and filtered selection helpers.
- Add bounded paging/scrolling, current marker, match position, and selected-provider status.
- Make catalog failure reasons safe to display and discovered-only edit setup valid.

### M3 — Completion audit
- Run focused and repository completion checks.
- Exercise a long deterministic catalog through input and rendering.
- Reconcile harness documents and audit remaining goal requirements.

## Acceptance Criteria

- Pressing the advertised `Ctrl+K` opens the picker without changing composer text and highlights the current model.
- A 100-model filtered list can reach its last row with End/PageDown, keeps that row visible, and Enter selects it; Down at the end remains on the last row.
- No-match filtering keeps the picker open and explains that no models match, with Add remaining discoverable.
- Rows visibly distinguish the current model from the navigation cursor.
- The selected provider line identifies live/fallback/unavailable state and gives a safe next step or reason.
- Editing a live/documented-only row opens a valid add-to-existing-provider draft and can save it.
- Wide/narrow snapshots, focused tests, and completion gates pass with no pending snapshots.

## Validation

Focused:
- `cargo test tui::input_tests --no-fail-fast`
- focused provider-editor orchestration tests
- `cargo test tui::render_tests --no-fail-fast`
- focused catalog error-safety tests

Completion:
- `cargo test --no-fail-fast`
- `cargo fmt --check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo build --release`
- local `--help` and `--version`

Manual/visual:
- Review current-row, provider-status, no-match, and long-list wide/narrow snapshots.

Blocked checks:
- None currently.

## Progress

- [x] Current shortcut, navigation, rendering, provider-status, and edit flows traced.
- [x] Failing interaction and render coverage added.
- [x] Picker behavior and visibility implementation complete.
- [x] Focused and completion validation complete.
- [x] Harness reconciled and remaining-goal audit recorded.

## Decisions

- Decision: keep one flat searchable result list for now, but add a visible selected-provider status and bounded window.
  Evidence: model IDs already include provider context and filtering works across both fields; the immediate failure is loss of selection/visibility in long lists.
  Consequence: navigation becomes reliable without introducing a second provider-selection mode.
- Decision: use concise inline state rather than a separate diagnostics modal.
  Evidence: provider availability matters at selection time and the popup has room for one contextual line.
  Consequence: users see what is live or unavailable without leaving the picker.

## Surprises and Discoveries

- The documented `Ctrl+K` path and implemented `Ctrl+L` path were reversed, making the primary advertised entry point ineffective.
- Filtered catalog refresh used the current model's index in the unfiltered list, so a visible highlighted row could disagree with what Enter attempted to select. Selection is now recomputed within the active filter.

## Recovery and Rollback

Revert only the files named in Scope. Tests use in-memory state, loopback services, and temporary roots.

## Completion Record

Changed files: `src/{app,main,model_catalog}.rs`, `src/tui/{input,input_tests,render,render_tests}.rs`, affected model-picker/startup/queue snapshot baselines, and the four repository harness documents.

Commands and results:
- `cargo test tui::input_tests --no-fail-fast`: PASS, 38 tests.
- Focused provider-editor, filtered refresh/selection, live selection/edit, and catalog error-safety tests: PASS.
- `cargo test tui::render_tests --no-fail-fast`: PASS, 37 tests and 70 accepted snapshots with zero pending.
- `cargo test --no-fail-fast`: PASS, 329 unit tests plus 20 CLI integration tests.
- `cargo fmt --check`, strict Clippy, release build, local help, and version: PASS.
- Rebuilt-binary isolated TUI: PASS. `Ctrl+K` displayed six choices including MiMo Pro and DeepSeek Pro; selecting `mimo-v2.5-pro` updated both active-model surfaces. The temporary home was removed.

Remaining risks: authenticated provider catalog refresh was not exercised because no credentials were read; CFG-027–029 retain loopback and public-Ofox coverage. No broad Cursor parity claim is made.

Harness updates: `ARCHITECTURE.md`, `CONTINUITY.md`, `TESTING.md`, and `NEXT_STEPS.md` record the completed interaction contract and validation evidence. The original provider/model goal is covered end to end: add/edit (`CFG-026`), live catalog discovery (`CFG-027`), MiMo Pro fallback (`CFG-028`), current DeepSeek/Ofox catalogs (`CFG-029`), and usable/visible selection (`CFG-030`).
