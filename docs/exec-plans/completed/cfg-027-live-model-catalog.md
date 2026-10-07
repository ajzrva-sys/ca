# CFG-027 — Live Provider Model Catalog

Status: COMPLETE
Owner/session: Codex 2026-08-10
Last updated: 2026-08-10

## Goal

Populate the model picker with the actual model IDs returned by each configured OpenAI-compatible provider, while retaining configured models when discovery is unavailable.

## Non-Goals

- Inferring model capabilities from names.
- Sending chat requests during discovery.
- Removing the manual provider/model editor.
- Requiring every provider to implement model listing.
- Mutating user runtime configuration during validation.

## Evidence and Current Behavior

- `src/tui/input.rs` opens a picker backed only by `AppState::model_list`.
- `src/tui/input.rs::build_model_list` derives that list solely from `ProviderConfig::models`.
- The OpenAI-compatible model-list contract is `GET <base_url>/models` with bearer authentication and a `{ data: [{ id }] }` response.
- Xiaomi's current official plan page advertises the full MiMo V2.5 lineup, while the local config contains only `mimo-v2.5`.
- `src/main.rs` already routes async TUI work through typed `StreamMsg` events without blocking input/rendering.

## Scope

Allowed files:
- `src/model_catalog.rs`, `src/main.rs`, `src/app.rs`, `src/config.rs`
- `src/tui/{input,input_tests,render,render_tests}.rs`
- relevant snapshots
- `ARCHITECTURE.md`, `CONTINUITY.md`, `TESTING.md`, `NEXT_STEPS.md`
- this ExecPlan

Forbidden files/areas:
- `oracles/`, `reference/`
- user runtime config/session files
- installed binary

Runtime state affected: opening the model picker may issue read-only authenticated `GET /models` requests. Selecting a discovered model atomically adds its ID to the provider's configured fallback list and makes it the default.

## Design and Invariants

- First picker open starts one asynchronous catalog refresh; `Shift+R` explicitly refreshes again.
- Each provider is queried independently with a bounded timeout/response size. One failure cannot hide other providers or configured fallback entries.
- Responses are accepted only from successful JSON objects with bounded, nonempty model IDs; IDs are sorted and deduplicated.
- Provider errors are summarized without response bodies, URLs containing credentials, or API keys.
- Live IDs merge with configured IDs. The current provider sorts first, followed by deterministic provider/model ordering.
- A live-only selection is atomically inserted into the raw provider model array together with the default update, preserving placeholders and unrelated TOML text.

## Milestones

### M1 — Bounded catalog client
- Add a dedicated async catalog boundary and loopback tests for success, auth, malformed/error/oversize responses, and partial provider failure.

### M2 — Responsive picker integration
- Trigger refresh on first open and `Shift+R`, show loading/result state, merge typed results, and ignore stale generations.
- Add input and wide/narrow render coverage.

### M3 — Durable selection and completion
- Persist live-only selections atomically, run completion gates, reconcile harness docs, and complete this plan.

## Acceptance Criteria

- A provider returning multiple IDs exposes all of them as separately selectable picker rows.
- The picker remains responsive and configured models remain selectable during/after refresh failure.
- Loading, success, partial-failure, and explicit-refresh states are visible and deterministic.
- Selecting a live-only model survives a config reload without exposing or resolving the API key.
- Automated validation uses loopback/temporary roots only and needs no real provider credentials.

## Validation

Focused:
- `cargo test model_catalog --no-fail-fast`
- `cargo test config_store_tests --no-fail-fast`
- `cargo test tui::input_tests --no-fail-fast`
- `cargo test tui::render_tests --no-fail-fast`

Completion:
- `cargo test --no-fail-fast`
- `cargo fmt --check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo build --release`

Manual/visual:
- Review changed wide/narrow picker snapshots.

Blocked checks:
- Live provider discovery is not required for completion because repository rules prohibit credential-dependent claims; record it as unrun unless a credential-safe app path is explicitly exercised.

## Progress

- [x] Current picker/config/orchestration paths and official model-list contract traced.
- [x] Bounded multi-provider catalog discovery implemented and tested.
- [x] Async picker refresh and visible state implemented and tested.
- [x] Live-only selection persistence implemented and tested.
- [x] Completion gates and harness reconciliation complete.

## Decisions

- Decision: merge live and configured models instead of replacing configured entries.
  Evidence: OpenAI-compatible providers may omit or reject model listing, and offline/local endpoints may not be running.
  Consequence: discovery improves selection without breaking the existing fallback path.
- Decision: do not infer chat/tool/image support from returned IDs.
  Evidence: the standard model-list object does not carry those capabilities.
  Consequence: capability profiles remain explicit config; discovery only establishes selectable provider IDs.

## Surprises and Discoveries

- The validation process did not inherit `XIAOMI_API_KEY`, so the credential-safe Xiaomi `/v1/models` check could not run. Loopback coverage proves the exact authenticated contract without claiming Xiaomi's live response.
- An oversized loopback response correctly caused the bounded client to close early; the fixture was updated to tolerate the expected broken pipe instead of treating client cancellation as server failure.

## Recovery and Rollback

Revert only the files named in Scope. All automated network/config fixtures are loopback or temporary.

## Completion Record

Changed files: added `src/model_catalog.rs`; updated model/config state, orchestration, persistence, input/render tests, four picker snapshots, and the named harness documents.
Commands and results: 3 catalog, 11 config, 35 input, 33 render, and focused orchestration tests PASS; full suite PASS (310 unit, 20 CLI); format, strict Clippy, release, help, and version PASS.
Remaining risks: Xiaomi's live endpoint was not confirmed in this process because its key was unavailable. Providers that do not implement the standard endpoint remain on configured fallbacks by design; returned IDs do not imply chat/tool capability.
Harness updates: `ARCHITECTURE.md`, `CONTINUITY.md`, `TESTING.md`, and `NEXT_STEPS.md` reconciled on 2026-08-10.
