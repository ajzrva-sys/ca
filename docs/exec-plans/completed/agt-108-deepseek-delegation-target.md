# AGT-108 — DeepSeek Delegation Target

Status: COMPLETE
Owner/session: Codex desktop `/root`
Last updated: 2026-08-15

## Goal

Make official DeepSeek V4 Flash and Pro routes eligible for CA's fail-closed
automatic subagent delegation with exact official USD pricing and cache-aware
actual usage accounting.

## Non-Goals

- Do not modify `oracles/` or `reference/`.
- Do not make unofficial DeepSeek-compatible endpoints delegation-eligible.
- Do not change the user's saved provider/default selection, install a binary,
  or make a paid provider call without separate explicit authority.
- Do not add fallback retries after a delegated child starts.

## Evidence and Current Behavior

- Installed offline doctor sees a saved DeepSeek credential and official
  `https://api.deepseek.com/v1` endpoint, but reports pricing unknown,
  unsupported billing, unknown capability, and routing ineligible.
- Adding DeepSeek changed the saved default to
  `deepseek/deepseek-v4-pro`; implementation must not silently change it.
- `src/pricing.rs` currently validates exactly four OpenAI/MiMo cards and only
  permits metered OpenAI or confirmed MiMo Token Plan billing.
- DeepSeek's official pricing page currently lists V4 Flash at USD
  $0.0028 cache-hit / $0.14 cache-miss / $0.28 output and V4 Pro at
  $0.003625 / $0.435 / $0.87 per million tokens.
- DeepSeek's official Chat Completions schema defines
  `prompt_cache_hit_tokens`, `prompt_cache_miss_tokens`, and optional reasoning
  output detail. Prompt tokens must equal the two cache subsets.

## Scope

Allowed files: `src/pricing.rs`, the shared Chat Completions usage decoder,
doctor/router/headless tests, and delegation/pricing/harness documentation.

Forbidden files/areas: `oracles/`, `reference/`, unrelated dirty worktree
changes, user secrets, user config/defaults, and installed binaries.

Runtime state affected: none during implementation and offline validation.

## Design and Invariants

- Add first-party cards only for exact `deepseek-v4-flash` and
  `deepseek-v4-pro`, sourced from the allowlisted official pricing page.
- Capability ceilings are Flash=`simple` and Pro=`standard`.
- DeepSeek metered billing is eligible only at exact official API base URLs
  (`https://api.deepseek.com` or `/v1`) with a resolved credential and a fresh
  official card. Proxies and custom endpoints remain ineligible.
- Forecasts assume uncached input plus 4,096 output tokens, matching the shared
  router contract. No long-context surcharge is invented.
- Actual cost uses exact hit/miss subsets only when both fields exist and sum to
  total prompt tokens. DeepSeek has no separate cache-write fee. Missing or
  inconsistent subsets keep cost unknown.
- Reasoning and visible output share DeepSeek's single output price, so a
  missing reasoning split does not affect the exact total-output charge.
- Existing four-card V1 catalogs remain readable but do not create DeepSeek
  routes; a fresh seed/refresh produces the complete six-card catalog.
- Refresh remains all-or-nothing and fetches DeepSeek only from the exact
  allowlisted official HTTPS page with a bounded strict parser.

## Milestones

### M1 — Characterization

- Add failing card, billing, routing, parser, and accounting tests.
- Prove unofficial endpoints and incomplete cache subsets fail closed.

### M2 — Pricing and protocol implementation

- Add DeepSeek cards, official-source refresh validation, endpoint billing
  readiness, and exact cache usage decoding.
- Preserve legacy catalog loading and current MiMo/OpenAI behavior.

### M3 — Integration and documentation

- Extend mocked headless routing/accounting coverage and offline doctor output.
- Update delegation, testing, architecture, continuity, index, and task docs.

### M4 — Validation

- Run focused tests, `cargo fmt --check`, strict all-target/all-feature Clippy,
  the complete all-target/all-feature suite, snapshot hygiene, and the macOS
  release build without paid calls or runtime mutation.

## Acceptance Criteria

- A configured exact official DeepSeek V4 Flash route is eligible for `simple`;
  Pro is eligible through `standard`.
- Route forecasts use the exact official miss/output prices and deterministic
  existing tie-breaking.
- Exact DeepSeek usage subsets produce exact incremental USD; absent or
  inconsistent subsets report unknown.
- Offline doctor distinguishes ready official DeepSeek metered billing from an
  unsupported custom endpoint.
- Mocked Sol-parent headless delegation selects DeepSeek Flash, records
  `Subagent`, and performs one child attempt.
- Existing OpenAI/MiMo tests and full completion gates pass.

## Validation

Focused: `cargo test deepseek -- --nocapture` passes 12 tests, including exact
pricing/accounting, official endpoint diagnostics, protocol decoding, routing,
and mocked Sol-parent dispatch. The strict-source and legacy-catalog focused
tests also pass.

Completion:

- `cargo fmt --check`: PASS.
- `cargo clippy --all-targets --all-features -- -D warnings`: PASS.
- `cargo test --all-targets --all-features --no-fail-fast`: PASS with
  1,334 main tests passed / 1 documented manual-browser ignore, 115 experiment
  tests passed / 1 documented external-fixture ignore, and 36 CLI tests passed.
- `cargo build --release --all-features`: PASS; arm64 Mach-O at
  `target/release/ca`, SHA-256
  `3c57a1ea4a4214996409a526fe676c77b72ae44e86b73d43141e1d3faf0ddced`.
- Snapshot hygiene and `git diff --check`: PASS; no `.snap.new` files.
- The live official DeepSeek pricing document contains every exact marker used
  by the bounded parser. This documentation fetch made no model/API request.
- Read-only `target/release/ca --doctor`: PASS with all six cards fresh, two
  eligible providers, and DeepSeek reporting `1/1 fresh`, `metered USD ready`,
  `standard`, and `routing eligible`. It also confirms the unchanged saved
  default is `deepseek/deepseek-v4-pro`.

Manual/visual: PASS. The installed TUI model picker selected the exact
`openai-codex/gpt-5.6-sol` row and persisted it through the normal atomic model
selection path. Installed offline doctor then reported all six cards fresh,
the Codex Sol default restored, and both DeepSeek and MiMo delegation-eligible.

Live acceptance: PASS. Authorized run
`745e6c33-25d9-4164-afe5-ce9f88707218` made exactly one `task` call with
`complexity="simple"`, selected `deepseek/deepseek-v4-flash`, forecast
approximately `$0.001156` incremental for 68 estimated input tokens plus the
fixed output allowance, returned `AGT108_DEEPSEEK_OK`, and completed with
`AGT108_PARENT_OK AGT108_DEEPSEEK_OK`. The JSONL trace contained exactly two
Codex parent turns, no fallback route, no retry, and no credential material.

Blocked checks: none.

## Progress

- [x] Verified configured DeepSeek identity and current doctor failure.
- [x] Verified official V4 prices and usage fields.
- [x] Add failing characterization tests.
- [x] Implement pricing, endpoint eligibility, refresh, and usage parsing.
- [x] Add integration tests and documentation.
- [x] Pass full offline validation and release build.
- [x] Back up and atomically install the release, restore the Codex Sol parent,
  and pass one authorized no-retry live delegation to DeepSeek Flash.

## Decisions

- Decision: Flash is `simple`; Pro is `standard`.
  Evidence: the product identities and existing CA capability scale.
  Consequence: DeepSeek is a cheap executor, not a replacement for Sol's
  `complex` ceiling.
- Decision: accept both official root and `/v1` API bases.
  Evidence: current official integration documentation publishes both forms.
  Consequence: existing first-class CA setup is eligible without rewrite.
- Decision: do not change the current saved DeepSeek default during source work.
  Evidence: repository runtime-mutation guardrail.
  Consequence: a later Codex-parent live acceptance requires explicit selection.
- Decision: treat one authorized acceptance as one complete tool loop.
  Evidence: explicit user confirmation.
  Consequence: the run permitted two Codex parent turns and exactly one
  DeepSeek child invocation, with no retry.

## Surprises and Discoveries

- Provider setup selected DeepSeek Pro as the saved default even though the user
  described it as an additional delegation target.
- CA's in-memory discontinued-model upgrade already exposes both current V4
  models for the official endpoint, while the secret-safe offline snapshot
  truthfully reports only the persisted model count.
- The exact strings required by the strict DeepSeek parser are present in the
  current raw official pricing page, including both model IDs, all six prices,
  and both cache labels.

## Recovery and Rollback

- Keep edits confined to the named source/docs and use existing atomic catalog
  persistence unchanged.
- Do not use destructive Git recovery in the dirty worktree.
- The prior installed binary is retained at
  `/Users/aji/.local/bin/ca.backup-agt108-20260815-175629`, SHA-256
  `ede243f9de9465bdfb8c95b2585bc30ace9e5fd7972b88d106e7c7fee3a8fdc9`.
  It can be restored with an exact same-directory atomic rename if a future
  local binary regression appears.

## Completion Record

Changed files: `src/pricing.rs`, `src/agent/provider.rs`, `src/doctor.rs`,
`src/headless.rs`, `docs/INTER_PROVIDER_DELEGATION.md`, `ARCHITECTURE.md`,
`TESTING.md`, `CONTINUITY.md`, `docs/.INDEX.md`, `NEXT_STEPS.md`, and this
ExecPlan.

Commands and results: focused DeepSeek, source-parser, legacy-catalog, and
mocked headless tests passed; all completion commands and exact totals are
recorded above. Installed `/Users/aji/.local/bin/ca` is the arm64 release with
SHA-256 `3c57a1ea4a4214996409a526fe676c77b72ae44e86b73d43141e1d3faf0ddced`.
Installed doctor passes with `openai-codex/gpt-5.6-sol` as default and DeepSeek
`metered USD ready`, `standard`, and eligible. Live run
`745e6c33-25d9-4164-afe5-ce9f88707218` passed the exact route, marker,
single-call, no-fallback, and secret-free acceptance checks.

Remaining risks: price cards still expire after 30 days and must be refreshed
from the official sources before routing can resume. Offline doctor
intentionally does not resolve the Codex OAuth cache, and headless JSONL does
not serialize the process-local child usage-ledger row; the shared runner's
`Subagent` classification remains directly covered by the integration tests.

Harness updates: exact DeepSeek price/capability/accounting tests, secret-free
doctor official/custom endpoint coverage, legacy four-card compatibility, and
mocked Sol-to-DeepSeek subagent dispatch are permanent regressions.
