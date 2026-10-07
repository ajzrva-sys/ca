# CFG-093 — Provider Template Catalog

Status: COMPLETE
Owner/session: current Codex task
Last updated: 2026-08-12

## Goal

Expose the complete 40-row provider catalog named by the current Hermes Agent provider guide in a searchable `ca` provider-template surface, make entries that fit `ca`'s complete existing HTTP protocol/auth adapters installable through the secure provider editor, preserve the accepted basic OpenAI Responses template with an explicit limitation, and label entries that require a new OAuth, SDK, custom-auth, dynamic-routing, or subprocess integration without claiming they work.

## Non-Goals

- Silently seeding, migrating, or overwriting user providers.
- Copying Hermes implementation code or treating Hermes behavior as Cursor parity.
- Reading/importing Hermes, Codex, Claude, GitHub, cloud, or browser credentials.
- Inventing third-party OAuth contracts, AWS SigV4, Vertex ADC, Azure API-version/auth behavior, Copilot model-dependent routing, or ACP subprocess support.
- Live provider traffic, real credentials, user runtime configuration, Keychain mutation, installed-binary replacement, or session writes during validation.

## Evidence and Current Behavior

- The official Hermes Agent provider page resolved on 2026-08-12 lists exactly 40 inference-provider rows. The rows mix ordinary API-key or keyless HTTP endpoints with OAuth, AWS/Google credential chains, Azure-specific auth, per-model protocol routing, and a local ACP subprocess.
- `ca` currently has three wire adapters: Chat Completions with optional Bearer auth, OpenAI Responses with optional Bearer auth, and Anthropic Messages with `x-api-key`. The Responses adapter does not yet retain reasoning output items across tool continuation. `ProviderConfig` cannot express arbitrary headers, OAuth refresh, SDK signing, or subprocess transports.
- `/provider` and `/provider setup` enumerate only configured providers. CFG-092 added a safe explicit OpenAI template action; opening it performs no write and explicit save atomically persists the provider and its protocol.
- Inferred: a static, audited catalog is the smallest truthful extension. It makes compatible endpoints immediately usable, retains the existing OpenAI shortcut/custom editor, and shows exactly why integration-backed rows cannot be installed rather than fabricating support.

## Scope

Allowed files:
- `src/provider_templates.rs` plus module wiring in `src/main.rs`.
- Provider state/orchestration/persistence and TUI files: `src/app.rs`, `src/config.rs`, `src/main.rs`, `src/tui/{input,render,input_tests,render_tests}.rs`, and affected snapshots.
- `docs/.INDEX.md`, `ARCHITECTURE.md`, `CONTINUITY.md`, `ENVIRONMENT.md`, `NEXT_STEPS.md`, `PARITY.md`, `TESTING.md`, and this plan.

Forbidden files/areas:
- `oracles/`, `reference/`, external evidence repositories, user config, Keychain, saved sessions, installed binaries, and unrelated source.

Runtime state affected:
- None during automated validation. Manual validation uses a disposable config/runtime root, no credential, and no live provider request.

## Design and Invariants

- Add a compiled static catalog with one stable row per linked Hermes entry, including search aliases, canonical `ca` provider ID where applicable, safe template fields, protocol, and typed availability.
- A ready template must be fully representable by one existing adapter/auth pair for the audited agent loop. A separately typed limited template may open the editor only when it names the exact missing capability; direct OpenAI is the sole accepted basic-Responses exception. Entries requiring OAuth/refresh, AWS/Google SDK credentials, Azure custom auth/versioning, model-dependent protocols, ACP subprocesses, or provider-state replay remain visible with a concise typed limitation and cannot open a misleading editor.
- Add `ProviderManagerView::Templates { return_to }`; the return target must preserve whether the catalog opened from the ordinary provider manager or setup. A setup-origin save must not switch defaults before the existing finish step.
- Keep `Shift+O` as the direct OpenAI shortcut and `Shift+A` as custom add. Add a visible `Shift+P` searchable catalog action in provider/setup lists.
- Generalize template opening through a stable catalog entry ID. The editor title uses the canonical provider ID and the protocol row uses the typed protocol; it never infers OpenAI identity from Responses.
- Opening/filtering/canceling the catalog or editor performs no config, Keychain, model-catalog, probe, or provider I/O. Explicit successful save remains the only mutation.
- Template adds write an explicit protocol; generic custom add continues to omit protocol and existing-provider edits preserve raw protocol.
- Duplicate canonical provider IDs refuse overwrite both before editor opening and at atomic persistence.
- A blank starter model is allowed only as an editable prefill for compatible dynamic local catalogs; save retains the existing nonempty-model validation.
- No secret value appears in catalog constants, snapshots, logs, tests, or documentation.

## Milestones

### M1 — Catalog and failing characterization

- Add the exact 40-row typed catalog and tests for uniqueness, search, safe fields, stable OpenAI values, and every availability class.
- Add failing input/orchestration tests for catalog opening, filtering, paging, selection, unsupported refusal, duplicate refusal, cancel return, and setup-origin preservation.
- Add wide/narrow render characterization for mixed ready/configured/integration-required rows, long scrolling, no matches, and safe template editor metadata.

### M2 — TUI and orchestration implementation

- Wire template state, request routing, selection, paste/filter behavior, and rendering through the existing provider manager.
- Reuse the secure editor and atomic persistence boundary for installable templates.
- Preserve the OpenAI direct shortcut and custom add behavior.

### M3 — Verification and delivery

- Run catalog/provider/config/input/render focused suites and inspect every changed snapshot at 100x30 and 60x24.
- Run the Rust completion gates from `TESTING.md`.
- Exercise the released TUI against a disposable existing config: open/filter/cancel without a write, verify an unavailable row remains inert, and save representative Chat Completions, Responses, and Anthropic templates without live traffic or default switching.
- Reconcile harness documents and archive this plan.

## Acceptance Criteria

- The template catalog contains and searches all 40 linked rows exactly once.
- Every catalog row reports one of: ready with an existing complete adapter, explicitly limited but installable, already configured, or a concrete integration requirement. Unsupported rows cannot be saved as if working.
- Compatible template fields are safe, editable, and use environment-variable references rather than secrets.
- `Shift+P` is visible and works from `/provider` and `/provider setup`; `Shift+O` and `Shift+A` retain their existing meanings.
- OpenAI remains exact, uses Responses, and displays its missing reasoning-item continuation; representative ready Chat Completions and Anthropic templates persist their explicit protocols correctly.
- Saving from setup preserves the existing default/current pair; saving from the ordinary manager retains its existing selection behavior.
- Opening, filtering, canceling, duplicate selection, and unavailable selection cause no write or network request.
- Wide and 60x24 narrow snapshots show complete statuses/help without clipped safety outcomes.

## Validation

Focused:
- `cargo test provider_templates --no-fail-fast`
- `cargo test provider_template --no-fail-fast`
- `cargo test provider_ --no-fail-fast`
- `cargo test tui::input_tests --no-fail-fast`
- `cargo test tui::render_tests --no-fail-fast`

Completion:
- `cargo test --no-fail-fast`
- `cargo fmt --check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo build --release`
- released `--help` and `--version` smoke checks
- zero pending `.snap.new` files

Manual/visual:
- Review all changed wide/narrow snapshots.
- Released disposable TUI walkthrough for ready, unavailable, cancel, and representative saved protocols with config hashes and no provider traffic.

Blocked checks:
- Live external-provider traffic and real authentication remain intentionally excluded.
- Rows whose documented authentication/transport cannot be expressed by current `ca` adapters remain intentionally non-installable in this task.

## Progress

- [x] Official 40-row provider list and current `ca` protocol/auth boundary inspected.
- [x] Typed catalog and red focused tests added.
- [x] Searchable catalog and generic secure template editor path implemented.
- [x] Focused and completion gates passed.
- [x] Released disposable TUI path verified.
- [x] Harness reconciled and plan archived.

## Decisions

- Decision: implement a static audited catalog, not a runtime Hermes dependency or default-config migration.
  Evidence: provider configuration is user-owned and only explicit atomic mutations currently preserve existing configs safely.
  Consequence: existing installations see the new catalog immediately without being marked configured or changed on load.
- Decision: include unavailable integration-backed rows with typed reasons.
  Evidence: the linked page calls distinct OAuth, SDK, cloud-auth, dynamic-routing, and subprocess paths “providers,” while `ca` cannot represent those paths in `ProviderConfig`.
  Consequence: the catalog is complete and honest; a row is never called supported merely because its name and URL can be displayed.
- Decision: preserve direct OpenAI and custom shortcuts while adding a searchable catalog.
  Evidence: those actions are already documented and tested user-visible behavior.
  Consequence: CFG-092 remains a fast path and the broader catalog scales without a shortcut per provider.

## Surprises and Discoveries

- `ca` appends adapter paths directly, so Anthropic-compatible template base URLs require the version path expected by the current adapter rather than blindly copying a foreign client's root URL.
- Template origin must survive the intermediate catalog view; otherwise setup-origin saves would silently select a new default.
- Editor labeling cannot infer provider identity from protocol because OpenAI and xAI both use Responses.
- A one-shot compatible endpoint is not sufficient for coding-agent support. Direct Gemini 3, DeepSeek V4, Kimi-family, GLM, MiMo, MiniMax, xAI, and several gateway starters require reasoning content, thought signatures, encrypted items, or request controls to survive a tool continuation. Those rows remain visible but inert until the missing state lane or provider fixture exists.

## Recovery and Rollback

Use `apply_patch` to revert only CFG-093-owned source, snapshots, and harness hunks. No destructive Git operation or user-runtime cleanup is needed.

## Completion Record

Changed files:

- Added `src/provider_templates.rs`; wired template state, routing, persistence orchestration, input, rendering, command discovery, and deterministic snapshots through `src/{app,main}.rs` and `src/tui/{commands,input,input_tests,render,render_tests}.rs`.
- Updated the repository knowledge index and provider/runtime/parity/testing/continuity handoff documents.

Commands and results:

- `cargo test provider_template --no-fail-fast`: PASS, 10 tests.
- `cargo test provider_ --no-fail-fast`: PASS, 121 unit plus 2 CLI tests.
- `cargo test tui::input_tests --no-fail-fast`: PASS, 65 tests.
- `cargo test tui::render_tests --no-fail-fast`: PASS, 114 tests with reviewed wide/narrow catalog, help, and editor snapshots.
- `cargo test --no-fail-fast`: PASS, 1106 unit, one ignored manual-Chromium test, and 36 CLI tests.
- `cargo fmt --check`, strict Clippy, release build, released help/version, and zero pending snapshots: PASS.
- Released disposable TUI: PASS for filtered ready editor/cancel, unavailable OAuth refusal, visible manager shortcut, no session, and no provider traffic.

Remaining risks:

- The catalog is a dated compiled snapshot and requires a deliberate evidence update when upstream providers change.
- OpenAI direct remains the accepted CFG-092 basic Responses path and is labeled limited; PRV-058 still excludes persisted/encrypted reasoning-item replay and `previous_response_id`.
- The 28 inert rows require separately scoped adapter/auth/transport work; no live external-provider certification was attempted.

Harness updates:

- `docs/.INDEX.md`, `ARCHITECTURE.md`, `CONTINUITY.md`, `ENVIRONMENT.md`, `NEXT_STEPS.md`, `PARITY.md`, and `TESTING.md` record the catalog ownership, exact 11-ready/1-limited/28-integration boundary, validation, and next protocol seam.
