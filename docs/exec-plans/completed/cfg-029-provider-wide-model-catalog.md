# CFG-029 — Provider-Wide Model Catalog Completion

Status: COMPLETE
Owner/session: Codex 2026-08-10
Last updated: 2026-08-10

## Goal

Finish the original provider/model picker work across every built-in provider: show current DeepSeek models without credentials, use Ofox's public live catalog without requiring its chat API key, retain MiMo's documented chat fallback, and leave Ollama/custom providers driven by their real configured or live catalogs.

## Non-Goals

- Guessing which models are installed in a stopped local Ollama instance.
- Freezing Ofox's changing 100-plus-model catalog into source.
- Listing non-chat MiMo speech models as coding-agent choices.
- Mutating user runtime config or replacing an installed binary.

## Evidence and Current Behavior

- The user's picker showed all four providers unavailable, so every provider fell back to its static configured rows.
- After CFG-028, MiMo has a documented fallback, but `src/config.rs` still gives DeepSeek only the discontinued `deepseek-chat` ID.
- DeepSeek's current official [`/models`](https://api-docs.deepseek.com/api/list-models), [pricing](https://api-docs.deepseek.com/quick_start/pricing), and [changelog](https://api-docs.deepseek.com/updates/) pages list `deepseek-v4-flash` and `deepseek-v4-pro`; the legacy `deepseek-chat` and `deepseek-reasoner` aliases were discontinued on 2026-07-24.
- Ofox's official [Models API](https://ofox.ai/docs/api/openai/models) documentation says `GET https://api.ofox.ai/v1/models` is public and requires no API key, but `src/model_catalog.rs` rejects an unresolved key before sending that request.
- Ollama's available models are machine-local, so configured entries and a successful local `/models` response are the only truthful sources.

## Scope

Allowed files:
- `src/config.rs`, `src/model_catalog.rs`, `src/main.rs`
- focused catalog/config/orchestration/render tests and snapshots
- `CONTINUITY.md`, `TESTING.md`, `NEXT_STEPS.md`
- `ARCHITECTURE.md`
- this ExecPlan

Forbidden files/areas:
- `oracles/`, `reference/`
- user runtime config/session files
- installed binary

Runtime state affected: catalog refresh remains read-only. Ofox refresh may now reach its public model-list endpoint even when `OFOX_API_KEY` is unset; selecting a model retains the existing explicit persistence behavior.

## Design and Invariants

- Built-in documented fallbacks are provider-specific and restricted to models usable by this chat/tool client.
- DeepSeek receives `deepseek-v4-flash` and `deepseek-v4-pro`; new configs no longer seed the discontinued alias.
- DeepSeek's discontinued aliases are removed from the in-memory model list only for the official host; raw user TOML remains untouched.
- Only the exact official `api.ofox.ai` host bypasses model-catalog authentication. Chat requests continue to require the configured key.
- Ofox model IDs come from its live public endpoint, not a stale embedded list.
- Unknown providers and local Ollama receive no inferred model IDs.

## Milestones

### M1 — Failing provider-wide characterization
- Add DeepSeek current-fallback/default tests and Ofox public-catalog policy/request tests.
- Confirm failures against the current MiMo-only implementation.

### M2 — Shared catalog correction
- Generalize documented chat fallbacks to DeepSeek and MiMo.
- Permit anonymous list requests only for Ofox's official public catalog.
- Update new-config DeepSeek defaults and exact zero-live picker coverage.

### M3 — Completion
- Run focused and completion gates, review snapshots, and reconcile harness documents.

## Acceptance Criteria

- With zero live DeepSeek results, `deepseek-v4-flash` and `deepseek-v4-pro` appear as separate picker rows.
- A new config contains current DeepSeek IDs and not `deepseek-chat`.
- An unresolved `OFOX_API_KEY` does not block `api.ofox.ai/v1/models`, and no Authorization header is sent to that public catalog.
- MiMo retains its two chat-model fallback rows.
- Ollama/custom providers receive no invented models.
- Focused and completion checks pass without credentials or user-runtime mutation.

## Validation

Focused:
- `cargo test model_catalog --no-fail-fast`
- `cargo test config_store_tests --no-fail-fast`
- focused zero-live orchestration test
- `cargo test tui::render_tests --no-fail-fast`

Completion:
- `cargo test --no-fail-fast`
- `cargo fmt --check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo build --release`
- local `--help` and `--version`

Manual/visual:
- Review wide/narrow filtered DeepSeek fallback snapshots.

Blocked checks:
- Credential-dependent DeepSeek/MiMo chat and model-list requests remain unclaimed.

## Progress

- [x] Built-in provider sources and the incomplete MiMo-only recovery characterized.
- [x] Failing provider-wide tests added.
- [x] DeepSeek and public Ofox catalog corrections implemented.
- [x] Focused and completion validation complete.
- [x] Harness reconciled and plan archived.

## Decisions

- Decision: use static fallbacks only for small first-party catalogs with current official documentation.
  Evidence: DeepSeek and MiMo each expose two chat models, while Ofox exposes a public, rapidly changing catalog and Ollama is local.
  Consequence: offline behavior is useful without embedding a misleading Ofox/Ollama list.
- Decision: hide discontinued DeepSeek aliases at runtime without rewriting raw TOML.
  Evidence: the aliases are past their official discontinuation date and would leave a known-broken picker row; raw config preservation remains an existing invariant.
  Consequence: official DeepSeek configs immediately present only current choices, and selecting one persists through the existing explicit selection path.

## Surprises and Discoveries

- Ofox's model-list endpoint is explicitly public; treating every unresolved provider key as a pre-request failure hid a live catalog that needed no credential.
- Ofox's live public catalog returned 129 total models during validation; filtering its documented metadata to text-output models with tool support retained 100 coding-agent-compatible choices and excluded video/image-only or no-tool entries.

## Recovery and Rollback

Revert only the files named in Scope. Tests use loopback servers and temporary roots; no user state requires rollback.

## Completion Record

Changed files: `src/config.rs`, `src/model_catalog.rs`, `src/main.rs`, `src/tui/render_tests.rs`, two DeepSeek picker snapshots, and the named harness documents.
Commands and results: 7 catalog tests, 13 config-store tests, 3 focused orchestration paths, 35 render tests with 66 accepted snapshots, 320 unit tests, 20 CLI tests, format, strict Clippy, release, help, and version all PASS; the public Ofox endpoint returned 129 IDs and 100 agent-compatible choices.
Remaining risks: credential-dependent DeepSeek and MiMo live requests were not exercised; Ofox's public count is time-dependent and will be refreshed at runtime.
Harness updates: `ARCHITECTURE.md`, `CONTINUITY.md`, `TESTING.md`, and `NEXT_STEPS.md` reconciled on 2026-08-10.
