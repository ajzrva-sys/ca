# CFG-092 — OpenAI Provider Template

Status: COMPLETE
Owner/session: current Codex task
Last updated: 2026-08-12

## Goal

Make OpenAI a visible one-action template from the provider manager and `/provider setup`, prefilled with the official API endpoint, environment-variable name, current explicit flagship model, and the existing `openai_responses` adapter, while preserving the generic custom-provider flow.

## Non-Goals

- Reading, creating, testing, or migrating a real OpenAI API key; live OpenAI traffic is excluded.
- ChatGPT subscription or Codex OAuth authentication.
- Adding hosted OpenAI tools, server-managed conversation state, automatic provider installation, a general template marketplace, or editable protocol controls.
- Changing existing providers, current/default selection, or the four-provider seed config merely by opening the template.

## Evidence and Current Behavior

- `/provider setup` renders only `ConfigStore::provider_management_entries()`; the shipped missing-config seed contains DeepSeek, Ofox, Ollama, and MiMo, so OpenAI is absent.
- `Shift+A` opens a blank `AddProvider` editor. Saving a new provider currently omits `protocol`, which intentionally defaults to Chat Completions.
- The complete opt-in `openai_responses` adapter already exists and is selected only by `protocol = "openai_responses"`.
- Official OpenAI documentation resolved on 2026-08-12 names `gpt-5.6-sol` as the current explicit flagship model, uses `https://api.openai.com/v1`, documents `OPENAI_API_KEY`, and recommends Responses for reasoning and tool-calling workflows.
- Inferred: a visible template action is safer and clearer than adding OpenAI silently to existing user configuration; it preserves explicit persistence and lets the user review every non-secret field before saving.

## Scope

Allowed files:
- `src/app.rs`, `src/config.rs`, `src/main.rs`, `src/tui/{input,render,input_tests,render_tests}.rs`, and affected snapshots.
- This plan plus `ARCHITECTURE.md`, `CONTINUITY.md`, `ENVIRONMENT.md`, `NEXT_STEPS.md`, and `TESTING.md` for durable facts and validation evidence.

Forbidden files/areas:
- `oracles/`, `reference/`, user config, Keychain, saved sessions, installed binaries, and unrelated source.

Runtime state affected:
- None during automated validation. Manual validation must use a disposable profile/home and must cancel before any credentialed request; loopback-only protocol checks are allowed.

## Design and Invariants

- Add a distinct `AddOpenAiProvider` editor request rather than changing generic `AddProvider` semantics.
- Expose the template visibly in provider-list and setup-list help; keep custom add available and distinguish the two actions.
- Prefill exactly `openai`, `https://api.openai.com/v1`, `OPENAI_API_KEY`, and `gpt-5.6-sol`. Show that the template uses OpenAI Responses.
- Carry a process-only template marker through the editor. Only an explicit successful save may persist `protocol = "openai_responses"`; cancellation performs no write.
- Extend the atomic provider/model update with an optional protocol override. `None` preserves existing-provider protocols and keeps generic new providers byte-compatible with omitted-protocol Chat Completions.
- Refuse template creation when provider ID `openai` is already configured instead of overwriting it.
- Never place a raw key in config, state, snapshots, logs, or tests.

## Milestones

### M1 — Failing characterization

- Add input/orchestration/config tests for the visible template action, exact prefill, duplicate refusal, cancel no-write behavior, and exact Responses protocol persistence.
- Add wide/narrow render coverage for provider/setup help and the prefilled masked editor.

### M2 — Implementation

- Add the template request/marker, key handling, editor prefill/rendering, and optional atomic protocol persistence.
- Preserve every existing generic provider path and editor return target.

### M3 — Verification and delivery

- Run focused provider/config/input/render tests and review changed snapshots.
- Run the Rust completion gates from `TESTING.md`.
- Exercise the released TUI with a disposable runtime root, proving the OpenAI template opens prefilled without a write and explicit save persists only the intended table.
- Reconcile harness documents and move this plan to completed.

## Acceptance Criteria

- `/provider` and `/provider setup` visibly advertise separate OpenAI-template and custom-provider actions.
- The OpenAI action opens an editable form with the four exact safe fields and a visible Responses protocol label.
- Saving persists one new provider with `${OPENAI_API_KEY}`, `gpt-5.6-sol`, and `protocol = "openai_responses"` atomically; setup-origin save does not silently change the default before the tested finish step.
- Generic Add Provider still omits protocol and therefore remains Chat Completions.
- Existing provider/model edits preserve their protocol byte-for-byte.
- Duplicate OpenAI template requests fail before editor mutation or config writes.
- No live request, real credential, user configuration, Keychain item, session, oracle, or installed binary is touched.

## Validation

Focused:
- provider template config/orchestration tests
- `cargo test provider_ --no-fail-fast`
- `cargo test tui::input_tests --no-fail-fast`
- `cargo test tui::render_tests --no-fail-fast`

Completion:
- `cargo test --no-fail-fast`
- `cargo fmt --check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo build --release`

Manual/visual:
- Review all changed wide/narrow snapshots.
- Released disposable TUI walkthrough opening the template from setup and canceling without a write.

Blocked checks:
- Live OpenAI traffic remains intentionally excluded.

## Progress

- [x] Current provider/setup, secure editor, protocol adapter, and official OpenAI guidance inspected.
- [x] Failing focused tests added.
- [x] Template and atomic protocol persistence implemented.
- [x] Focused and completion gates passed.
- [x] Released disposable TUI path verified.
- [x] Harness reconciled and plan archived.

## Decisions

- Decision: use an explicit template action rather than silently seeding or mutating OpenAI in user configs.
  Evidence: provider setup is defined over configured providers and all provider mutations are explicit.
  Consequence: existing installations gain the option immediately without a config migration.
- Decision: persist the existing Responses adapter for the template.
  Evidence: current OpenAI guidance recommends Responses for reasoning/tool workflows and `ca` already implements the complete adapter.
  Consequence: the template does not accidentally route the coding-agent tool loop through omitted-protocol Chat Completions.
- Decision: pin the explicit `gpt-5.6-sol` slug in source.
  Evidence: the current official resolver identifies it as the explicit flagship; aliases may drift.
  Consequence: future model changes are deliberate source updates, not silent alias reroutes.

## Surprises and Discoveries

- The prior provider-setup task explicitly left provider presets out of scope, explaining why OpenAI was absent despite Responses protocol support.
- The setup footer already reserved enough vertical capacity for a third help row; the ordinary provider list needed its visible-row capacity reduced by one. Both remain unclipped at 60×24.
- A final narrow-layout audit found that the first duplicate-refusal sentence exceeded the 54-column list interior. The final copy fits completely at 60×24, and a dedicated snapshot now protects the visible `left it unchanged` outcome.
- Saving the template starts the existing catalog refresh. With `OPENAI_API_KEY` absent, that refresh fails locally with the safe missing-environment status and performs no provider traffic.

## Recovery and Rollback

Use `apply_patch` to revert only CFG-092-owned hunks and snapshots. The disposable runtime root was moved recoverably to Trash and contained no real credential.

## Completion Record

Changed files:
- Source: `src/app.rs`, `src/config.rs`, `src/main.rs`, `src/tui/input.rs`, `src/tui/input_tests.rs`, `src/tui/render.rs`, and `src/tui/render_tests.rs`.
- Snapshots: new wide/narrow OpenAI-template editor snapshots, a narrow duplicate-refusal snapshot, plus updated provider-manager and setup wide/narrow help snapshots.
- Harness: `ARCHITECTURE.md`, `CONTINUITY.md`, `ENVIRONMENT.md`, `NEXT_STEPS.md`, `PARITY.md`, `TESTING.md`, and this plan.

Commands and results:
- Initial focused characterization: RED on the absent `AddOpenAiProvider` action; the helper's required filter argument was corrected.
- `cargo test openai_template --no-fail-fast`: PASS, 4 tests.
- Isolated Responses probe and adapter tests: PASS, 1 each.
- `cargo test provider_ --no-fail-fast`: PASS, 106 unit plus 2 CLI tests.
- `cargo test tui::input_tests --no-fail-fast`: PASS, 63 tests.
- `cargo test tui::render_tests --no-fail-fast`: PASS, 112 tests; all affected 100×30 and 60×24 snapshots reviewed, including the complete narrow duplicate-refusal copy.
- `cargo test --no-fail-fast`: PASS, 1091 unit plus 36 CLI tests; one existing manual local-Chromium test ignored by the ordinary gate.
- `cargo fmt --check`, strict Clippy, release build, release help, release version, and zero-pending-snapshot inspection: PASS.
- Released disposable TUI: PASS. Opening preserved the exact config hash; saving wrote the exact Responses table, kept `deepseek/auto`, and missing-key discovery stopped locally without provider traffic.

Remaining risks:
- `gpt-5.6-sol` is deliberately pinned from the 2026-08-12 official resolver and requires a future source update when the recommended explicit model changes.
- No credentialed live OpenAI request was run or authorized; the complete Responses wire path remains covered by isolated loopback tests.

Harness updates:
- Documented the explicit-action/no-migration decision, template values, optional protocol persistence boundary, intentional `ca` extension, current validation counts, and completed handoff.
