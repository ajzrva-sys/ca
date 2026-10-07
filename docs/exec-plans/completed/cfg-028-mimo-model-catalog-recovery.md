# CFG-028 — MiMo Model Catalog Recovery

Status: COMPLETE
Owner/session: Codex 2026-08-10
Last updated: 2026-08-10

## Goal

Keep MiMo's supported chat models, including `mimo-v2.5-pro`, selectable when live model discovery cannot authenticate, while restoring live discovery for configurations that use Xiaomi's official environment-variable name.

## Non-Goals

- Listing MiMo ASR or TTS models that cannot serve this coding agent's chat requests.
- Mutating the user's runtime configuration or installing a replacement binary.
- Claiming live Xiaomi validation without a credential-safe request that actually runs.
- Changing generic provider chat behavior.

## Evidence and Current Behavior

- The user's 2026-08-10 screenshot shows `4 models · 0 live · unavailable: deepseek, mimo, ofox, ollama`; filtering for `mimo` leaves only the configured `mimo-v2.5` row.
- Before CFG-028, `src/config.rs::default_config` stored `${XIAOMI_API_KEY}`, while Xiaomi's official integration documentation names `MIMO_API_KEY`.
- `src/model_catalog.rs` retains configured entries on discovery failure but has no provider-documented fallback catalog.
- Xiaomi's official [model-list](https://mimo.mi.com/docs/en-US/api/model/list-models), [Token Plan](https://mimo.mi.com/docs/en-US/price/token-plan), and [Codex integration](https://mimo.mi.com/docs/en-US/tokenplan/integration/codex-configuration) documentation identifies `mimo-v2.5` and `mimo-v2.5-pro` as current language models and uses `MIMO_API_KEY`; the other listed V2.5 endpoints are speech models.
- Inferred: unresolved `${XIAOMI_API_KEY}` can prevent the MiMo `/models` request even when the documented `MIMO_API_KEY` is available.

## Scope

Allowed files:
- `src/model_catalog.rs`, `src/config.rs`, `src/main.rs`
- focused picker/config/catalog tests and snapshots if needed
- `CONTINUITY.md`, `TESTING.md`, `NEXT_STEPS.md`
- this ExecPlan

Forbidden files/areas:
- `oracles/`, `reference/`
- user runtime config/session files
- installed binary

Runtime state affected: none during implementation or validation. At product runtime, catalog refresh remains read-only; selecting a model keeps the existing explicit atomic persistence behavior.

## Design and Invariants

- New default configurations use `${MIMO_API_KEY}`.
- Existing configurations containing `${XIAOMI_API_KEY}` resolve from `MIMO_API_KEY` only when the legacy name is absent; raw TOML remains unchanged.
- MiMo providers receive a bounded documented fallback containing only chat-compatible `mimo-v2.5` and `mimo-v2.5-pro`.
- Live results still merge with configured and documented IDs and take no trust from response bodies or model-name capability inference.
- Status distinguishes live provider results from documented fallback results and does not expose secrets or response bodies.

## Milestones

### M1 — Failing characterization
- Add focused coverage for legacy environment-name compatibility and MiMo documented fallback behavior.
- Confirm the tests fail against current source.

### M2 — Recovery implementation
- Correct the default environment reference, add compatibility resolution, and merge the MiMo chat fallback into the picker.
- Add orchestration/render coverage for the exact zero-live MiMo scenario.

### M3 — Completion
- Run focused and repository completion checks, review snapshots, and reconcile harness documentation.

## Acceptance Criteria

- Filtering the picker for `mimo` shows separate `mimo-v2.5` and `mimo-v2.5-pro` rows even when MiMo live discovery fails.
- A legacy `${XIAOMI_API_KEY}` config can use an available `MIMO_API_KEY` at runtime without rewriting or exposing the resolved value.
- Live catalog results continue to merge and persist exactly as before.
- The picker status truthfully reports zero live providers and one documented fallback provider in the characterized failure case.
- Focused tests and the minimum Rust completion gates pass without external credentials or runtime-state mutation.

## Validation

Focused:
- `cargo test model_catalog --no-fail-fast`
- `cargo test config_store_tests --no-fail-fast`
- focused orchestration test for catalog refresh
- `cargo test tui::render_tests --no-fail-fast`

Completion:
- `cargo test --no-fail-fast`
- `cargo fmt --check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo build --release`

Manual/visual:
- Review wide/narrow picker snapshots for the zero-live MiMo fallback state.

Blocked checks:
- Live Xiaomi authentication remains unclaimed if `MIMO_API_KEY` is unavailable to this process.

## Progress

- [x] Screenshot behavior and the config/catalog data flow characterized.
- [x] Failing environment compatibility and documented fallback tests added.
- [x] Recovery implementation complete.
- [x] Focused and completion validation complete.
- [x] Harness documents reconciled and plan archived.

## Decisions

- Decision: expose only MiMo's language models as documented fallback entries.
  Evidence: `ca` sends chat-completion requests; MiMo's ASR/TTS IDs have different modalities and endpoints.
  Consequence: the fallback exposes `mimo-v2.5` and `mimo-v2.5-pro`, not speech-only models that would fail after selection.
- Decision: preserve legacy placeholders in raw config while adding runtime alias resolution.
  Evidence: config patching intentionally preserves unresolved secret references and unrelated TOML.
  Consequence: existing users recover without an automatic config rewrite or secret materialization.

## Surprises and Discoveries

- The added documented-source label made the prior provider-name failure summary too wide for the picker. The fallback status now reports the bounded unavailable-provider count, keeping the complete state visible at both tested widths.

## Recovery and Rollback

Revert only the files named in Scope. All tests use temporary roots or loopback servers; no user runtime data requires rollback.

## Completion Record

Changed files: `src/config.rs`, `src/model_catalog.rs`, `src/main.rs`, `src/tui/render_tests.rs`, two picker snapshots, and the named harness documents.
Commands and results: 5 catalog tests, 12 config-store tests, 2 focused orchestration tests, 34 render tests with 64 accepted snapshots, 315 unit tests, 20 CLI tests, format, strict Clippy, release, help, and version all PASS.
Remaining risks: Xiaomi live authentication was not exercised, so the live response remains unclaimed; the documented fallback is intentionally limited to chat-compatible models.
Harness updates: `CONTINUITY.md`, `TESTING.md`, and `NEXT_STEPS.md` reconciled on 2026-08-10.
