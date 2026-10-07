# CFG-033 — `/provider` Management Hub

Status: COMPLETE
Owner/session: Codex 2026-08-10
Last updated: 2026-08-10

## Goal

Make `/provider` open a provider-first manager where users can edit provider settings, add providers, browse and manage each provider's models, and delete providers safely.

## Non-Goals

- Deleting individual models.
- Renaming provider IDs.
- Validating credentials against live providers.
- Repairing a missing Keychain reference that prevents startup.
- Modifying oracle/reference trees, user runtime state, or the installed binary during validation.

## Evidence and Current Behavior

- `src/tui/commands.rs` has `/model` but no `/provider` command.
- Provider actions are nested in the global model picker, and `EditModel` couples provider URL/key changes to one model operation.
- `ConfigStore::upsert_provider_model_with_secret` always carries a model and rewrites defaults, so it cannot safely implement provider-only edits.
- Editor Escape always returns to the global model picker; a provider hub needs explicit return targets.
- The clean Cursor backup exposes command-to-picker `/model` behavior but no generic provider manager. `/provider` is therefore an oracle-informed `ca` extension, not a parity claim.

## Scope

Allowed files:
- `src/{app,config,main,provider_secrets}.rs`
- `src/tui/{commands,input,input_tests,render,render_tests}.rs`
- relevant snapshot baselines
- `ARCHITECTURE.md`, `CONTINUITY.md`, `PARITY.md`, `TESTING.md`, `NEXT_STEPS.md`
- this ExecPlan

Forbidden files/areas:
- `oracles/`, `reference/`
- user config, sessions, or Keychain
- installed binary

Runtime state affected: only explicit manager saves/deletes. Tests use temporary roots and injected secret stores.

## Design and Invariants

- `/provider [filter]` and `/providers [filter]` open an idle-only searchable provider list with the current provider first.
- Provider manager state contains only safe summaries: provider ID, endpoint, configured model IDs, and credential-source classification. It never contains resolved keys.
- Provider detail shows configured plus discovered/documented models and reuses typed model selection/add/edit actions. No model-delete action exists.
- Existing provider IDs are fixed. Provider-only edits change endpoint/credential source without changing models/defaults.
- Add Provider requires an initial model and rejects duplicate provider IDs.
- Provider deletion is confirmed with Cancel selected by default. If current/default is removed, a deterministic remaining model is selected and persisted; deletion is blocked when none exists.
- Exact provider-scoped profile/context entries and review overrides are removed. Historical sessions are not rewritten.
- Exact Keychain-backed credentials are deleted after the safe config patch. Keychain failure restores the original config; a missing item is success.
- Save/delete work stays off the TUI event loop. Success reloads state and invalidates affected provider catalog data; failure keeps the relevant surface open.

## Milestones

### M1 — Characterization and UI state
- Add failing command/input/render tests for the provider list, detail, actions, return paths, and confirmation.
- Add the typed manager/editor-return state and render/input flow.

### M2 — Persistence and orchestration
- Add safe raw management snapshots, provider-only updates, provider deletion, and secret deletion.
- Wire background mutations, deterministic replacement, reload, and cache refresh.

### M3 — Completion audit
- Run focused and full validation, review snapshots, exercise an isolated temporary-home TUI flow, and reconcile the harness.

## Acceptance Criteria

- `/provider` opens the current provider in a searchable one-row-per-provider manager; `/providers` is an alias and an optional argument prefilters.
- Provider detail shows all available models while distinguishing configured/current rows.
- Add/edit provider and add/edit/select model flows return to the correct originating surface.
- Provider-only edits preserve all models, defaults, profiles, context, review settings, comments, and unrelated config.
- Provider deletion requires confirmation, removes scoped config/Keychain state, and atomically selects a deterministic replacement when needed.
- No provider deletion occurs when no replacement exists, confirmation is cancelled, or Keychain cleanup fails.
- Provider IDs remain fixed and no model deletion is exposed.
- Focused tests, reviewed wide/narrow snapshots, completion gates, and isolated TUI validation pass without touching user state.

## Validation

Focused:
- `cargo test tui::commands::tests --no-fail-fast`
- `cargo test tui::input_tests --no-fail-fast`
- `cargo test config_store_tests --no-fail-fast`
- `cargo test provider_secrets --no-fail-fast`
- `cargo test provider_manager --no-fail-fast`
- `cargo test tui::render_tests --no-fail-fast`

Completion:
- `cargo test --no-fail-fast`
- `cargo fmt --check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo build --release`
- local help/version smoke tests

Manual/visual:
- Review all new wide/narrow snapshots with zero pending files.
- Use a temporary home with non-Keychain providers to exercise list/detail/edit/add/delete/replacement behavior, then remove it.

Blocked checks:
- Real Keychain deletion and credential-dependent provider authentication are intentionally not run.

## Progress

- [x] Current command, picker/editor, config, orchestration, test, and oracle seams traced.
- [x] Provider manager command/state/input/render implemented.
- [x] Provider-only update and transactional deletion implemented.
- [x] Focused tests and snapshots reviewed.
- [x] Completion gates and isolated TUI validation complete.
- [x] Harness reconciled and plan archived.

## Decisions

- Decision: manage provider settings and models in one provider-first hub.
  Evidence: user selected the combined hub.
  Consequence: `/model` remains the quick global switcher while `/provider` is the durable management entry point.
- Decision: delete providers but never individual models.
  Evidence: user explicitly narrowed deletion after choosing all available models.
  Consequence: live/discovered models cannot appear to return after a delete.
- Decision: automatically choose a replacement and delete provider-scoped Keychain credentials.
  Evidence: user selected both behaviors.
  Consequence: deletion is transactional and blocked when no replacement exists.
- Decision: keep provider IDs fixed.
  Evidence: user selected fixed IDs.
  Consequence: no defaults/session/profile/Keychain rename migration is introduced.

## Surprises and Discoveries

- Provider-scoped refresh needed an explicit target carried through the async catalog result; otherwise an edit would clear or relabel unrelated provider cache status.
- Raw base URLs can theoretically embed user-info or query credentials. Management snapshots now strip both before UI state construction, and new saves reject credential-bearing/query/fragment endpoints.

## Recovery and Rollback

Revert only the allowed files. Temporary roots and fake secret stores contain all automated persistence. Provider deletion writes config before Keychain cleanup and restores the original raw config if cleanup fails.

## Completion Record

Changed files: provider manager state/command/input/render, safe config/provider-secret persistence, async orchestration, tests/snapshots, and the five project harness pages.
Commands and results: 361 unit plus 20 CLI tests pass; format, strict Clippy, release build, help/version, and isolated rebuilt-TUI validation pass; 92 snapshots accepted with zero pending.
Remaining risks: real macOS Keychain deletion and credential-dependent external provider authentication were intentionally not exercised. Historical sessions remain unchanged by design.
Harness updates: `ARCHITECTURE.md`, `CONTINUITY.md`, `PARITY.md`, `TESTING.md`, and `NEXT_STEPS.md` record the extension boundary, ownership, validation, and completion.
