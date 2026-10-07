# CFG-026 — Provider and Model Manager

Status: COMPLETE
Owner/session: Codex 2026-08-10
Last updated: 2026-08-10

## Goal

Let a user add a provider/model or edit the highlighted provider/model from the existing model picker, save it without exposing a resolved API key, and use the refreshed entry immediately.

## Non-Goals

- Deleting providers or models.
- Renaming provider identifiers after creation.
- Discovering models from provider networks.
- Changing model capability/reasoning profiles.
- Writing literal API secrets into the configuration.

## Evidence and Current Behavior

- `src/tui/render.rs::render_model_picker` exposes selection and filtering only.
- `src/tui/input.rs::handle_model_picker` handles navigation, filtering, selection, and close only.
- `src/config.rs::ConfigStore` already atomically patches defaults while preserving unresolved `${SECRET}` placeholders.
- `src/main.rs::execute_input_action` owns configuration side effects and refreshes in-memory selection state.
- Inferred: a focused modal reached from the picker is the shortest discoverable path that preserves the current CLI/TUI boundary.

## Scope

Allowed files:
- `src/app.rs`, `src/config.rs`, `src/main.rs`
- `src/tui/{commands,input,input_tests,render,render_tests}.rs`
- TUI snapshots
- `ARCHITECTURE.md`, `CONTINUITY.md`, `TESTING.md`, `NEXT_STEPS.md`
- this ExecPlan

Forbidden files/areas:
- `oracles/`, `reference/`
- user runtime config/session files
- installed release binary

Runtime state affected: an explicit save from the new modal atomically updates the configured provider/model list and defaults when the edited model was the default.

## Design and Invariants

- `A` from the model picker opens an add form; `E` opens the highlighted entry for editing.
- The editor owns provider ID, base URL, API-key environment variable name, and model ID fields. Provider ID is fixed while editing an existing entry.
- The environment-variable field never shows or accepts a resolved/literal secret. Blank preserves an existing key reference and means no authorization value for a new provider.
- Persistence reads the raw TOML and patches only the selected provider/default/profile keys, retaining unrelated text and unresolved placeholders.
- Saving refreshes the runtime config and model list; editing the selected/default model updates those references coherently.

## Milestones

### M1 — Characterize and specify input/render behavior
- Add input coverage for opening add/edit, field navigation, cancellation, and save actions.
- Add wide/narrow reviewed snapshots for the form.
- Validation: focused input and render tests.

### M2 — Implement secret-safe persistence and refresh
- Add a raw-document provider/model upsert with validation and focused temporary-root tests.
- Route the typed save action through orchestration and refresh in-memory configuration.
- Validation: focused config and orchestration tests.

### M3 — Completion gates and durable handoff
- Run the minimum Rust completion gates and update harness documents with actual results.
- Move this plan to `docs/exec-plans/completed/` when all acceptance criteria pass.

## Acceptance Criteria

- The model picker visibly advertises add/edit shortcuts.
- Add and edit forms work entirely by keyboard and explain their fields/secret behavior.
- Invalid provider/model/base URL/environment-variable input produces a visible error without modifying disk.
- A save atomically preserves unrelated comments and `${SECRET}` references.
- Adding to an existing provider preserves its key reference when the environment field is blank.
- Renaming a model updates matching default and exact profile/context keys.
- Focused input, render, config, and orchestration tests require no network, credentials, or user runtime state.

## Validation

Focused:
- `cargo test config_store_tests --no-fail-fast`
- `cargo test tui::input_tests --no-fail-fast`
- `cargo test tui::render_tests --no-fail-fast`

Completion:
- `cargo test --no-fail-fast`
- `cargo fmt --check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo build --release`

Manual/visual:
- Review all new/changed wide and narrow snapshots.

Blocked checks:
- No live provider request or user config mutation is required or planned.

## Progress

- [x] Current config, picker, input-action, and persistence paths traced.
- [x] Focused interaction tests and render fixtures added.
- [x] Secret-safe raw config mutation implemented and tested.
- [x] Runtime refresh and selection coherence implemented.
- [x] Completion gates and harness reconciliation complete.

## Decisions

- Decision: API credentials are entered only as environment-variable names.
  Evidence: repository rules keep secrets external and `ConfigStore` preserves unresolved placeholders.
  Consequence: the UI cannot accidentally echo a resolved key or persist a newly typed literal secret.
- Decision: provider identifiers are immutable in edit mode.
  Evidence: provider names key defaults, profiles, sessions, review settings, and provider-wide model lists.
  Consequence: provider URL/auth and model IDs are editable without an ambiguous provider-wide rename migration.

## Surprises and Discoveries

- Editing a provider by replacing its whole TOML table also removed a comment attached to that table; mutating the existing table in place preserves the comment and unrelated document structure.

## Recovery and Rollback

Revert only the files named in Scope. Tests use temporary config roots; no user config rollback is needed during implementation.

## Completion Record

Changed files: `src/app.rs`, `src/config.rs`, `src/main.rs`, `src/tui/{commands,input,input_tests,render,render_tests}.rs`, four model-picker/editor snapshots, and the named harness documents.
Commands and results: focused config/input/orchestration/render suites PASS; `cargo test --no-fail-fast` PASS (302 unit, 20 CLI); format, strict Clippy, release, help, and version PASS.
Remaining risks: no live provider call was made; provider identifiers intentionally remain immutable in edit mode, and provider/model deletion remains out of scope.
Harness updates: `ARCHITECTURE.md`, `CONTINUITY.md`, `TESTING.md`, and `NEXT_STEPS.md` reconciled on 2026-08-10.
