# CFG-032 — Secure API-Key Entry

Status: COMPLETE
Owner/session: Codex 2026-08-10
Last updated: 2026-08-10

## Goal

Let a user paste a provider API key directly into the provider/model editor, keep it masked, store it in macOS Keychain, and use it immediately without writing the secret to `config.toml` or requiring shell setup.

## Non-Goals

- Displaying, copying, exporting, or validating an API key against a live provider.
- Removing environment-variable support.
- Deleting Keychain items or adding a general credential-management screen.
- Changing provider protocols, model discovery policy, or model fallbacks.
- Adding GUI, mouse, accessibility, or VoiceOver behavior.
- Mutating the user's real config, Keychain, sessions, or installed binary during automated/manual validation.

## Evidence and Current Behavior

- `ProviderEditorField` exposes only `ApiKeyEnv`; the editor explicitly tells users never to paste an API key.
- `ConfigStore::upsert_provider_model` can persist only `${ENV_VAR}` references and preserves other raw values.
- `ConfigStore::load_with_env` resolves environment placeholders into the runtime-only config copy.
- `security-framework` 3.7.0 is already present transitively through `reqwest`/`native-tls` on macOS, but is not a direct dependency.
- Apple documents generic-password Keychain items as encrypted secret storage identified by service and account. Apple also documents Keychain item calls as blocking operations that should run away from the UI thread.
- Inferred: the direct-entry path should be the first credential field, with the environment-variable name retained as an alternative.

## Scope

Allowed files:
- `Cargo.toml`, `Cargo.lock`
- `src/{app,config,main,provider_secrets}.rs`
- `src/tui/{input,input_tests,render,render_tests}.rs`
- affected provider-editor snapshots
- `ARCHITECTURE.md`, `CONTINUITY.md`, `TESTING.md`, `NEXT_STEPS.md`
- this ExecPlan

Forbidden files/areas:
- `oracles/`, `reference/`
- user runtime config/session/Keychain state
- installed binary

Runtime state affected: an explicit successful save with a pasted key writes one generic-password item to macOS Keychain and writes only a provider-scoped Keychain reference to raw config. Tests inject a fake secret store; the manual flow cancels before save.

## Design and Invariants

- Add a masked `API key` editor field before the existing environment-variable field. A fixed mask never exposes content or length; blank preserves the current credential.
- Typing/pasting a direct key clears the environment-variable field. Typing an environment-variable name clears the unsaved direct key. The two sources cannot be saved together.
- Secret input uses a redacted, zeroizing host-only buffer. It is never placed in `InputAction`, debug output, status text, snapshots, config, sessions, or provider/model drafts.
- Raw config stores `keychain://ca/provider/<provider>` only. Runtime config resolves that exact provider-scoped reference through an injected secret-store boundary.
- macOS production storage uses a generic-password item with a stable service and the provider ID as account. Other platforms fail with an actionable environment-variable message rather than treating the reference as a bearer token.
- Keychain and config work runs on a blocking worker, not the TUI event loop. The form remains open and visibly busy until completion.
- The config update is validated/rendered before any mutation. For a direct key, the safe config reference is written first; if Keychain storage fails, the original raw config is restored atomically. Errors never include key material.
- Existing `${ENV_VAR}` and legacy raw-value behavior remains compatible. Editing with both credential fields blank preserves the raw credential source.

## Milestones

### M1 — Failing characterization
- Add secret-buffer redaction/zeroization tests.
- Add config tests for exact Keychain-reference resolution, missing-secret failure, safe persistence, rollback, and environment compatibility.
- Add input/render tests for masked paste, source switching, busy state, and wide/narrow copy.

### M2 — Secure persistence and UI
- Add the injected provider-secret boundary and macOS Keychain adapter.
- Add the masked editor field and secret-safe editing behavior.
- Move direct-key persistence to a blocking worker and apply success/failure back on the TUI thread.

### M3 — Completion audit
- Run focused suites and all repository completion gates.
- Exercise the rebuilt TUI under a temporary home through masked paste and cancel, without writing a Keychain item.
- Reconcile harness documents and record remaining platform/runtime uncertainty.

## Acceptance Criteria

- Add Provider visibly offers `API key` with “Paste to save in macOS Keychain” and retains `API key env` as an alternative.
- Pasting into `API key` renders only a fixed mask, does not alter the composer, and does not place the secret in debug output or action values.
- Enter saves the key under the provider account, persists only `keychain://ca/provider/<provider>`, reloads the runtime config with the resolved value, and selects the saved model.
- A failed Keychain write restores the original raw config and keeps the form open with an actionable inline error and the masked input available to retry.
- Existing exact `${ENV_VAR}` entries remain visible as names and continue to resolve without Keychain access.
- Missing or unreadable referenced Keychain entries fail before provider I/O with no secret or reference used as a bearer token.
- Focused tests, full completion gates, reviewed snapshots, and isolated rebuilt-TUI masked-entry flow pass with no pending snapshots.

## Validation

Focused:
- `cargo test provider_secrets --no-fail-fast`
- `cargo test config_store_tests --no-fail-fast`
- `cargo test tui::input_tests --no-fail-fast`
- focused provider-editor orchestration tests
- `cargo test tui::render_tests --no-fail-fast`

Completion:
- `cargo test --no-fail-fast`
- `cargo fmt --check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo build --release`
- local `--help` and `--version`

Manual/visual:
- Review add-provider/add-model/edit-model snapshots at both supported sizes.
- Run the rebuilt binary with a temporary home; open Add Provider, paste a non-secret marker into the masked key field, verify no characters/length appear, then cancel.

Blocked checks:
- A real default-Keychain save/read is intentionally not performed because validation may not mutate user Keychain state.
- Credential-dependent provider discovery/authentication remains outside this task.

## Progress

- [x] Current editor, config, event-loop, and platform-secret seams traced.
- [x] Failing secret, config, input, orchestration, and render coverage added.
- [x] Secure direct-entry implementation complete.
- [x] Focused and completion validation complete.
- [x] Harness reconciled and completion audit recorded.

## Decisions

- Decision: use macOS Keychain rather than raw TOML or a shell profile.
  Evidence: repository policy keeps secrets external, while Keychain provides encrypted generic-password storage keyed by service/account.
  Consequence: direct paste is supported securely on macOS; portable environment-variable configuration remains available everywhere.
- Decision: keep the environment-variable field alongside direct entry.
  Evidence: existing configurations and non-macOS users depend on `${ENV_VAR}` references.
  Consequence: no migration is required, and blank fields continue to preserve current credential configuration.
- Decision: perform persistence on a blocking worker.
  Evidence: Keychain APIs are synchronous and documented as potentially blocking.
  Consequence: save completion becomes an explicit TUI state transition rather than a synchronous reducer side effect.

## Surprises and Discoveries

- `security-framework` is already locked transitively at 3.7.0, so adding the macOS adapter does not introduce a second Keychain implementation or version.
- The existing editor action is secret-safe because it clones only `ProviderModelDraft`; keeping the new key outside that draft preserves the boundary.
- The picker refresh completed against Ofox's public catalog during the isolated TUI run and produced 105 merged selectable models; the credential form remained independent of that asynchronous update.
- A lone Ctrl+C is intentionally not the picker close path; the advertised Escape path was used before quitting the isolated run.

## Recovery and Rollback

Revert only the files named in Scope. Fake stores and temporary config roots cover persistence tests. The manual flow cancels before saving, so it creates no credential item to clean up.

## Completion Record

Changed files: `Cargo.toml`, `Cargo.lock`, `src/{app,config,main,provider_secrets}.rs`, `src/tui/{input,input_tests,render,render_tests}.rs`, affected picker/provider-editor snapshots, and the four harness documents.

Commands and results:
- `cargo test provider_secrets --no-fail-fast`: PASS, 2 tests.
- `cargo test config_store_tests --no-fail-fast`: PASS, 17 tests.
- `cargo test tui::input_tests --no-fail-fast`: PASS, 42 tests.
- focused provider-editor environment/direct save tests: PASS, 2 tests.
- `cargo test tui::render_tests --no-fail-fast`: PASS, 39 tests and 74 accepted snapshots with zero pending.
- `cargo test --no-fail-fast`: PASS, 343 unit tests plus 20 CLI integration tests.
- `cargo fmt --check`, strict Clippy, release build, local help, and version: PASS.
- Rebuilt-binary isolated TUI: PASS. The real picker advertised Edit model/key, Add Provider exposed both credential sources, bracketed paste rendered only a fixed mask, and Escape canceled before persistence. Inspection found no saved marker/reference/provider; the temporary home was removed.

Remaining risks: the macOS adapter was compiled but a real default-Keychain save/read was intentionally not performed because validation may not mutate user Keychain state. Non-macOS direct entry returns an environment-variable instruction. Switching away from a saved Keychain credential does not delete the now-unused item because credential deletion is out of scope.

Harness updates: `ARCHITECTURE.md`, `CONTINUITY.md`, `TESTING.md`, and `NEXT_STEPS.md` record the masked secret boundary, Keychain reference format, background persistence/rollback contract, focused coverage, and isolated terminal evidence.
