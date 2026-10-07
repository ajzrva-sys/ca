# PRV-045 — Local Provider Discovery

Status: COMPLETE
Owner/session: current Codex session
Last updated: 2026-08-10

## Goal

Add an explicit `/provider discover` and `/providers discover` flow that searches a fixed set of loopback ports without credentials or persistence, displays compatible local OpenAI-style providers, and pre-fills the existing Add Provider form only after the user selects a result.

## Non-Goals

- Do not scan any host other than `127.0.0.1` or any port outside 8000, 8080–8179, 11434, and 1234.
- Do not use configured providers, proxies, redirects, credentials, cookies, environment authentication, mDNS, LAN discovery, or arbitrary URLs.
- Do not persist discovered endpoints or models until the user explicitly saves the existing Add Provider form.
- Do not restore deleted built-ins, auto-select a model, auto-start a local runtime, or claim Cursor parity.
- Do not modify oracle/reference trees, real user config, Keychain data, sessions, or the installed binary.

## Evidence and Current Behavior

- `src/tui/commands.rs` registers `/provider` and `/providers`; `src/main.rs` currently treats only the exact `setup` subcommand specially and otherwise opens the normal provider manager with the argument as a filter.
- `src/model_catalog.rs` fetches configured providers' `/models` catalogs, but it may use provider credentials and configured endpoints, so it is not an acceptable discovery boundary.
- `src/app.rs`, `src/tui/input.rs`, `src/tui/render.rs`, and `src/main.rs` already own provider-manager/editor state, navigation, rendering, blocking mutations, and post-save orchestration.
- The existing Add Provider form requires a provider ID, endpoint, credential source, and initial model; selecting a discovered provider can safely reuse that editor with provider/model drafts.
- Oracle review found no generic Cursor provider discovery fixture. This is an intentional, oracle-informed `ca` extension.
- Inferred: discovery must have its own process-only generation/state because catalog refresh and provider probes resolve configured credentials and have different safety semantics.

## Scope

Allowed files:

- `src/provider_discovery.rs`
- `src/{lib,app,main}.rs`
- `src/tui/{commands,input,render}.rs`
- focused tests/snapshots under existing source-owned test modules
- `docs/exec-plans/{active,completed}/`, `ARCHITECTURE.md`, `CONTINUITY.md`, `TESTING.md`, `NEXT_STEPS.md`, `PARITY.md`

Forbidden files/areas:

- `oracles/`, `reference/`, real runtime config/session/Keychain roots, installed binaries
- existing configured-provider catalog authentication behavior except minimal shared safe parsing if tests prove equivalence

Runtime state affected:

- Process-only discovery state and one bounded background scan.
- Persistent config only after an explicit save in the pre-existing Add Provider form.

## Design and Invariants

1. `provider_discovery` owns a fixed approved port list and derives every request URL internally as `http://127.0.0.1:<port>/v1/models`.
2. A dedicated `reqwest::Client` disables proxy use and redirects, has short connect/request deadlines, and never receives a credential, cookie, endpoint, or arbitrary header.
3. Requests run with a documented fixed concurrency bound. Each response is streamed under a decoded-byte cap; HTTP failures, redirects, unsafe content types, malformed JSON, empty catalogs, and invalid model IDs are ignored without retaining response bodies.
4. Candidate state contains only a deterministic draft ID, loopback port/base URL, and bounded model IDs. It has no resolved secrets and is never serialized.
5. `/provider discover` is exact subcommand routing. Similar strings such as `/provider discovery` remain ordinary provider filters.
6. Opening discovery performs the scan explicitly. Rescan is also explicit. Generation IDs discard stale results after close/rescan; actions remain bounded while scanning.
7. Enter on a candidate opens the existing Add Provider form with the derived endpoint and first discovered model. Cancel returns to discovery. Saving follows existing duplicate-ID and atomic provider-add behavior.
8. Paste is isolated from the composer. The overlay supports bounded navigation and remains usable at narrow terminal sizes.
9. No discovery operation reads or writes config, Keychain, sessions, oracle files, or user conversation state.

## Milestones

### M1 — Characterization and safe discovery boundary

- Add failing command/state tests for exact discover routing and normal-filter compatibility.
- Add the fixed-port scanner with loopback URL construction, no-proxy/no-redirect client, deadlines, concurrency/body/model caps, and secret-free candidate results.
- Add hermetic loopback tests for success, headers, redirects, malformed/oversized/empty responses, deadlines, and exact production port allowlist.

Expected result: only valid model catalogs reached through the internal loopback-port boundary become candidates.

Validation: focused `provider_discovery` and command tests.

### M2 — TUI state, input, and rendering

- Add process-only discovery overlay state and stale-generation handling.
- Route `/provider[s] discover`, start/cancel/rescan the background task, and handle results without blocking the TUI loop.
- Add navigation, paste isolation, Enter-to-prefill, editor-origin return, and safe status/error rendering.
- Add reviewed wide/narrow snapshots for scanning, populated, empty, and failure states.

Expected result: users can discover a local server and land in a pre-filled Add Provider form; nothing persists until Save.

Validation: focused input/orchestration/render tests and snapshot review.

### M3 — Completion gates and reconciliation

- Run all focused suites, full tests, formatting, strict Clippy, release build, and help/version/profile command smoke tests.
- Exercise the rebuilt TUI under a temporary home with a loopback service on one approved port, cancel once, then select and cancel the Add Provider form; prove no config/session is created by discovery.
- Reconcile architecture, continuity, testing, next-steps, and parity documents; record actual commands/results and move this plan to completed.

Expected result: PRV-045 is independently releasable and the next roadmap plan is ready but not active.

## Acceptance Criteria

- Both aliases open discovery only for the exact `discover` subcommand.
- The production scan targets exactly 103 unique approved loopback ports and no other address.
- Requests contain no Authorization, Proxy-Authorization, Cookie, or configured provider data; environment proxies and redirects cannot expand authority.
- Connect/request deadlines, concurrency, response bytes, model count, and model ID size are bounded.
- Valid nonempty `/v1/models` responses appear in deterministic port order with sorted/deduplicated model IDs.
- Empty, malformed, oversized, redirected, non-JSON, and unsuccessful responses do not expose bodies or become candidates.
- Closing/rescanning invalidates stale results; scanning never blocks input or starts a second concurrent scan generation.
- Enter pre-fills Add Provider with a deterministic editable ID, exact loopback endpoint, and initial model; Cancel returns to discovery and Save alone persists.
- Existing `/provider [filter]`, `/provider setup [filter]`, configured catalog refresh, provider editing, and bare-mode write restrictions remain unchanged.
- All required tests, snapshots, release/smoke gates, and isolated walkthrough pass without touching real state.

## Validation

Focused:

- `cargo test provider_discovery::tests -- --nocapture`: PASS, 8 scanner boundary tests.
- `cargo test provider_ -- --nocapture`: PASS, 73 unit plus 2 CLI tests.
- Command/input/orchestration coverage proves exact routing, normal-filter compatibility, paste/navigation, stale/closed generation rejection, prefill, no-write-before-save, and post-save targeted catalog refresh.

Completion:

- `cargo test --no-fail-fast`: PASS, 488 unit plus 25 CLI tests.
- `cargo fmt --check`: PASS.
- `cargo clippy --all-targets -- -D warnings`: PASS.
- `cargo build --release`: PASS.
- Rebuilt help/version/bare/named-profile smoke: PASS.

Manual/visual:

- Eight new wide/narrow scanning/populated/empty/failure snapshots and two provider-palette updates reviewed; zero `.snap.new` files.
- Temporary-home rebuilt TUI with a credentialless loopback catalog on approved port 1234: scan, rescan, populated result, pre-filled Add Provider, editor cancel, discovery close, and quit all observed. Config SHA-256 stayed `6aa5920ca42d337d712f28de85adc38e7ce8d401615ed61c5531bc9bf774409d`; no session file appeared. The server and temporary root were removed.

Blocked checks: none.

## Progress

- [x] Source seams and current provider/catalog behavior inspected.
- [x] Active ExecPlan created before source edits.
- [x] Safe scanner and hermetic tests complete.
- [x] Command/state/orchestration flow complete.
- [x] Wide/narrow snapshots reviewed.
- [x] Full gates and temporary-home walkthrough complete.
- [x] Harness reconciled and plan ready to archive.

## Decisions

- Decision: use an independent scanner rather than the configured model-catalog client.
  Evidence: configured catalog refresh accepts runtime `ProviderConfig`, including resolved credentials and arbitrary endpoints.
  Consequence: the new boundary can prove fixed authority and absence of credentials by construction.
- Decision: require at least one valid discovered model before showing a candidate.
  Evidence: the existing Add Provider flow requires an initial model.
  Consequence: Enter always opens a saveable pre-filled form while the model remains editable.
- Decision: derive a default provider ID as `local-<port>` and keep it editable.
  Evidence: ports are the only stable safe identifier exposed by the scan.
  Consequence: repeated scans are deterministic and duplicate IDs are still rejected by existing save logic.

## Surprises and Discoveries

- The stated numeric range contains 103 ports, not 183: 100 ports in 8080–8179 plus 8000, 11434, and 1234. The allowlist test fixes the exact intended set.
- Normal TUI startup creates a missing default config before workspace trust. The walkthrough therefore used a prepared temporary credentialless config and proved discovery/editor cancellation caused no write by comparing its SHA-256 before and after.

## Recovery and Rollback

- Remove the discovery module/state/command route and its tests with targeted patches; no schema migration or runtime data rollback is needed because discovery state is process-only.
- If a user explicitly saved the Add Provider form during a manual run, remove only that temporary-home config. Tests must never point at the real home.

## Completion Record

Changed files: `src/provider_discovery.rs`, `src/{app,main}.rs`, `src/tui/{commands,input,input_tests,render,render_tests}.rs`, ten discovery/palette snapshots, this ExecPlan, and the five harness documents.

Commands and results: all focused, full, format, strict-lint, release, smoke, snapshot, and isolated TUI gates above passed.

Remaining risks: compatible local servers that do not expose OpenAI-style `/v1/models` are intentionally invisible; occupied approved ports running unrelated valid-looking JSON services can appear only if they return a nonempty conforming model list, and still require explicit user Save.

Harness updates: architecture ownership/invariants, continuity inventory/verification/handoff, testing evidence, next-step status, and intentional-extension parity classification reconciled.
