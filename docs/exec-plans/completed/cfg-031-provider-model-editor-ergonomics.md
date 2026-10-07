# CFG-031 — Provider/Model Editor Ergonomics

Status: COMPLETE
Owner/session: Codex 2026-08-10
Last updated: 2026-08-10

## Goal

Make adding and editing provider/model entries direct from the populated picker: adding a model reuses the selected provider, adding a provider is a distinct visible action, environment-variable references are safely visible, and every form field supports real cursor-based editing.

## Non-Goals

- Deleting providers or models.
- Renaming an existing provider ID or migrating all provider-scoped profile keys.
- Displaying, copying, or changing resolved API-key values.
- Changing provider protocols, catalog contents, or selection persistence.
- Adding mouse, GUI, accessibility, or VoiceOver behavior.
- Mutating user runtime configuration, installing a binary, or editing oracle/reference trees during validation.

## Evidence and Current Behavior

- `src/tui/input.rs::handle_model_picker` maps `Shift+A` to `OpenProviderEditor(None)`, so it discards the selected provider and opens a blank provider/base URL/key/model form.
- The picker has no separate add-provider action even though the form represents both adding a provider and adding a model.
- `handle_provider_editor` only appends characters and pops the last character. Left, Right, Home, End, and Delete do nothing, so an existing base URL or model ID cannot be edited in place.
- `ProviderEditorState` has no cursor and `render_provider_editor` highlights a whole field without showing an insertion point.
- `Event::Paste` bypasses picker/editor overlay precedence and writes into the hidden chat composer, so model IDs, URLs, and filter text cannot be pasted into the visible control.
- Existing API-key environment references are intentionally omitted from the editor. The raw config retains safe `${ENV_VAR}` placeholders, but the editor can only say “blank keeps current,” so the configured variable name is not visible.
- Inferred: the product voice is terse and matter-of-fact. Action-first labels and contextual inline notes fit the existing terminal UI.

## Scope

Allowed files:
- `src/app.rs`, `src/config.rs`, `src/main.rs`
- `src/tui/{commands,input,input_tests,render,render_tests}.rs`
- relevant snapshot baselines
- `ARCHITECTURE.md`, `CONTINUITY.md`, `TESTING.md`, `NEXT_STEPS.md`
- this ExecPlan

Forbidden files/areas:
- `oracles/`, `reference/`
- user runtime config/session files
- installed binary

Runtime state affected: explicit form saves retain their current atomic config/default/current-model behavior. Automated and manual validation use temporary roots only.

## Design and Invariants

- `Shift+A` means “add model” for the selected/current provider; it prefills the fixed provider, base URL, and exact safe API-key environment reference, leaves Model ID empty, and focuses Model ID.
- `Shift+P` means “add provider” and opens the full blank form. When no provider exists, `Shift+A` falls back to this form rather than dead-ending.
- `Shift+E` retains edit-selected-model behavior. Add-provider, add-model, and edit-model are explicit editor modes rather than inferred from whether `original_model` is present.
- Existing provider IDs remain fixed in add-model/edit-model modes. Base URL, API-key environment name, and Model ID remain editable.
- The raw config boundary may expose only an exact syntactically valid `${ENV_VAR}` reference. Literal/resolved key material is never returned or rendered.
- Each field owns a UTF-8 byte-boundary cursor. Left/Right, Home/End, Backspace/Delete, insertion, and Tab/BackTab preserve valid boundaries; changing fields moves the cursor to that field's end.
- Paste targets the visible provider field or picker filter, strips single-line control characters, and never mutates the hidden composer.
- The selected field renders an explicit insertion marker. Copy remains action-specific and concise: “Add provider,” “Add model to <provider>,” or “Edit model.”
- Save errors remain inline. Success identifies the saved provider/model without exposing key material.

## Milestones

### M1 — Failing characterization
- Add action tests for selected-provider Add Model and separate Add Provider.
- Add cursor/edit tests covering insertion and Unicode-safe deletion.
- Add config tests proving exact environment-reference extraction and literal-key hiding.
- Add wide/narrow snapshots for add-model and add-provider forms.

### M2 — Editor behavior and copy
- Add explicit editor intent/mode and safe raw environment-reference lookup.
- Prefill selected-provider forms and implement field cursor editing.
- Render action-specific titles, notes, insertion marker, and picker actions.

### M3 — Completion audit
- Run focused suites and all repository completion gates.
- Exercise add-model and in-place edit behavior through the rebuilt TUI with a temporary home.
- Reconcile harness documents and record remaining risks.

## Acceptance Criteria

- From a selected MiMo row, `Shift+A` opens “Add model to mimo,” fixes Provider to `mimo`, preserves the current base URL and safe environment-variable name, leaves Model ID empty, and focuses it.
- `Shift+P` opens “Add provider” with the Provider field focused and all values blank.
- Left/Right/Home/End/Backspace/Delete and typed insertion edit the active field at a visible UTF-8-safe cursor; Tab and BackTab move to the next/previous allowed field and place the cursor at its end.
- Pasting a model ID into the editor or picker updates that visible field/filter and leaves the composer unchanged.
- A raw exact `${MIMO_API_KEY}` displays only `MIMO_API_KEY`; a literal key or composite placeholder is never returned or rendered.
- Existing edit and discovered-only edit/save behavior remains valid.
- Picker and editor copy distinguish Add Model, Add Provider, Edit, Refresh, select, and navigation at wide/narrow sizes.
- Focused tests, full completion gates, reviewed snapshots, and the isolated rebuilt-TUI flow pass with no pending snapshots.

## Validation

Focused:
- `cargo test tui::input_tests --no-fail-fast`
- `cargo test config_store_tests --no-fail-fast`
- focused provider-editor orchestration tests
- `cargo test tui::render_tests --no-fail-fast`

Completion:
- `cargo test --no-fail-fast`
- `cargo fmt --check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo build --release`
- local `--help` and `--version`

Manual/visual:
- Review add-provider, add-model, and edit-model wide/narrow snapshots.
- Run the rebuilt binary with a temporary home; open the picker, enter Add Model, edit within a prefilled value, cancel, and confirm no real runtime state changed.

Blocked checks:
- Credential-dependent live provider discovery remains outside this editor task.

## Progress

- [x] Current picker/editor/config seams traced.
- [x] Failing interaction, secret-safety, and render coverage added.
- [x] Editor modes, prefilling, cursor editing, paste routing, and copy implemented.
- [x] Focused and completion validation complete.
- [x] Harness reconciled and completion audit recorded.

## Decisions

- Decision: split Add Model and Add Provider at the picker instead of adding a second management screen.
  Evidence: the selected row already supplies provider context, and the existing form/config boundary can safely reuse it.
  Consequence: common add-model work becomes one focused field while new-provider setup stays available and explicit.
- Decision: retain one editor for provider settings and model ID, but make its mode explicit.
  Evidence: base URL and API-key environment reference belong to the selected provider and already save atomically with the model.
  Consequence: no new persistence format or separate partial-save path is required.

## Surprises and Discoveries

- The editor's full-field highlight looked interactive in snapshots, but the state machine had no cursor and supported end-only mutation.
- The raw config already preserves the safe environment reference needed for visibility; the runtime config cannot be used because it may contain a resolved secret.
- Paste events had a separate global path from key events, so overlay precedence was correct for typing but wrong for pasted provider/model text.

## Recovery and Rollback

Revert only the files named in Scope. Tests use in-memory state and temporary roots; no runtime migration is introduced.

## Completion Record

Changed files: `src/{app,config,main}.rs`, `src/tui/{commands,input,input_tests,render,render_tests}.rs`, affected picker/editor snapshot baselines, and the four repository harness documents.

Commands and results:
- `cargo test config_store_tests --no-fail-fast`: PASS, 14 tests.
- `cargo test tui::input_tests --no-fail-fast`: PASS, 41 tests.
- Focused add/edit/discovered-only provider-editor orchestration tests: PASS.
- `cargo test tui::render_tests --no-fail-fast`: PASS, 39 tests and 74 accepted snapshots with zero pending.
- `cargo test --no-fail-fast`: PASS, 336 unit tests plus 20 CLI integration tests.
- `cargo fmt --check`, strict Clippy, release build, local help, and version: PASS.
- Rebuilt-binary isolated TUI: PASS. MiMo Add Model reused the provider endpoint, exposed only `MIMO_API_KEY`, focused Model ID, and supported visible insertion inside the existing URL. The flow was canceled, the temporary home was removed, and no user state was touched.

Remaining risks: provider IDs remain intentionally fixed after creation; deletion remains out of scope. Credential-dependent catalog refresh was not exercised because no credentials were read. No broad Cursor parity claim is made.

Harness updates: `ARCHITECTURE.md`, `CONTINUITY.md`, `TESTING.md`, and `NEXT_STEPS.md` record the contextual editor modes, raw-secret boundary, cursor/paste contract, and validation evidence.
