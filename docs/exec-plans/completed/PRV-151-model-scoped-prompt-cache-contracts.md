# PRV-151 — Model-Scoped Prompt Cache Contracts

Status: COMPLETE
Owner/session: Codex / current implementation session
Last updated: 2026-08-22

## Goal

Make prompt/KV caching truthful for exact model routes: stable across one saved
conversation and its tool loop, explicitly bounded where the wire protocol
supports it, provider-managed elsewhere, and accurately normalized for usage
and billing.

## Non-Goals

- Do not add Anthropic outbound cache markers or local `llama-server` KV tuning.
- Do not infer or send undocumented controls to Codex OAuth, MiMo, DeepSeek, or
  unknown compatible endpoints.
- Do not run paid/live provider requests, inspect credentials, install a binary,
  or mutate runtime configuration/sessions during implementation.
- Do not modify or copy implementation source from `oracles/` or `reference/`.

## Evidence and Current Behavior

- `ProviderConfig::prompt_cache_mode` is provider-wide, while the existing
  first-party resolver already keys behavior by endpoint, protocol, and model.
- Interactive `start_llm_turn` constructs a new client for every physical call;
  the current cache key is random unless the trace-session helper is called.
- `SystemPromptParts::render` folds query memory, task skills, plan state, and
  hooks into the same system message selected as the explicit stable prefix.
- `context::messages_for_request` inserts a checkpoint `Role::System` message,
  but `AgentClient::append_provider_messages` drops canonical system messages.
- DeepSeek, MiMo/OpenAI-shaped, and Anthropic usage counters have different
  presence and billing semantics. Current pricing treats cache writes as a
  disjoint replacement bucket for every provider.
- Codex, MiMo-Code, and Hax oracles provide architectural lessons only. Cursor
  exposes normalized cache usage but no observable request policy; this is not
  a Cursor parity task.

## Scope

Allowed files: provider/config/request/context/session/pricing/usage/client and
TUI source, focused tests/snapshots, this plan, and current harness documents.

Forbidden files/areas: `oracles/`, `reference/`, credentials, user config and
sessions, installed binaries, Git history, and unrelated provider behavior.

Runtime state affected: none during implementation and automatic validation.

## Design and Invariants

- Add exact `[model_prompt_cache."provider/model"]` declarations with independent
  request format, usage format, and policy. Exact first-party contracts own
  formats; custom routes may opt into closed known dialects.
- Provider-managed routes send no cache-control fields and cannot promise `off`.
  Only explicit-capable Responses routes support automatic, stable-prefix, and
  off policies.
- Derive a private cache key from the saved/run scope, exact route/model,
  contract/layout version, stable prefix, and canonical tool digest. Branches
  rotate scope; resumes on an unchanged route reproduce it.
- Prompt construction distinguishes session-epoch, append-only, logical-turn,
  and request-only material. One explicit breakpoint follows only the
  session-epoch prefix; a logical turn is byte-stable through tool continuations.
- Checkpoint summaries are serialized exactly once and activate only between
  logical turns. Canonical history remains intact.
- Normalize cache read/write/miss counts with reported/derived/unknown
  provenance. Billing explicitly distinguishes replacement, surcharge, and
  included writes without changing existing headless/service JSON schemas.

## Milestones

### M1 — Characterization and contract configuration

- Add exact cache contract/configuration types, validation, precedence, legacy
  preservation, and deterministic model-cache UI coverage.
- Add failing characterizations for key churn, dropped checkpoints, unstable
  explicit prefixes, and nondeterministic MCP ordering.

### M2 — Prompt scope and cache epochs

- Make cache identity independent of tracing and stable across the interactive
  session/tool loop while rotating on route/model/layout/prefix/tool changes.
- Add typed prompt boundaries, checkpoint wire projection/staging, and stable
  tool serialization.

### M3 — Usage and billing

- Add canonical cache counter provenance and dialect-aware decoding.
- Add write-billing semantics and compatible catalog/config migration.

### M4 — Completion

- Finish the dedicated model-cache workflow, focused lifecycle/property/render
  coverage, full repository validation, and durable documentation.

## Acceptance Criteria

- Main turn, retry, and tool continuations on one exact route share one private
  key; resume reproduces it, while branch/endpoint/protocol/model/layout changes
  rotate it.
- Dynamic memory/hooks are outside the explicit stable boundary, and equivalent
  tool catalogs serialize identically.
- A valid active checkpoint appears once on the wire, covered history is absent,
  and activation never changes a running tool loop's prompt epoch.
- First-party and custom routes send only declared cache fields; provider-managed
  routes remain automatic-only.
- Cache read/write/miss accounting validates known subsets and prices replacement,
  surcharge, and included writes exactly without changing stable output schemas.
- Existing config/comments and legacy cache settings load without automatic
  rewrite; model rename/provider deletion maintain exact cache keys.

## Validation

Focused: config/contracts, provider/client serialization, context/session/MCP,
usage/pricing, orchestration, and model-cache input/render suites.

Completion: `cargo test --no-fail-fast`, all-target/all-feature tests,
`cargo fmt --all --check`, strict Clippy, release build, smoke help/version,
snapshot hygiene, `git diff --check`, and oracle/reference immutability checks.

Manual/visual: review deterministic wide/narrow cache-screen snapshots.

Blocked checks: native live provider verification requiring credentials/spend.

## Progress

- [x] Audit current cache contracts, orchestration, usage, pricing, and oracles.
- [x] Add failing characterizations and exact cache configuration/contracts.
- [x] Implement stable prompt scope, segmentation, and checkpoint epochs.
- [x] Implement canonical usage and write-billing semantics.
- [x] Complete UI, validation, and harness reconciliation.

## Decisions

- Decision: implement the generic exact-model framework now.
  Evidence: provider, model architecture, wire protocol, usage dialect, and
  billing mode vary independently.
  Consequence: custom routes can declare known closed dialects without receiving
  first-party assumptions.
- Decision: retain cache identity across saved-session resume on the same exact
  route/model and rotate it for branches.
  Evidence: this preserves reuse without sharing branch correlation.
  Consequence: no raw key needs to be persisted.
- Decision: use a dedicated model-cache screen and implement only provider-managed
  plus OpenAI Responses explicit outbound controls.
  Evidence: provider credentials and model cache capabilities have different
  scopes; MiMo/DeepSeek/Codex controls are not publicly documented.
  Consequence: Anthropic outbound markers remain separate work.

## Surprises and Discoveries

- The working tree already contains extensive user-owned changes, including the
  untracked first-party contract implementation. All PRV-151 edits must be
  surgical and preserve unrelated content.
- The existing checkpoint projection removes covered history but drops the
  replacement summary before provider serialization.
- Tool continuations must retain the original logical-turn images even after
  assistant/tool messages become the request tail; provider-generated image
  follow-ups are instead typed as request-only segments.
- Cache writes are a subset of misses, not a third prompt partition. A fixture
  with 20 prompt and 5 read tokens therefore reports 15 uncached tokens and 3
  writes, rather than subtracting the writes twice.

## Recovery and Rollback

Revert only PRV-151 hunks with targeted patches. Do not reset the dirty tree,
accept unrelated snapshots, modify oracle/reference files, or discard user work.

## Completion Record

Changed files: exact cache configuration/resolution and targeted TOML editing in
`src/config.rs` and `src/provider_contract.rs`; scoped prompt assembly and wire
handling in `src/request.rs`, `src/agent/{client,provider,stream}.rs`, and the
interactive/headless/service orchestration; checkpoint/session and MCP ordering
in `src/{context,session,mcp}.rs`; canonical usage/pricing in
`src/{usage,pricing,provider_contract_verifier,multi_run}.rs`; and the dedicated
model-cache command/editor plus focused wide/narrow snapshots under `src/tui/`.

Commands and results:

- Focused cache, checkpoint, MCP, usage, pricing, headless, service, lifecycle,
  configuration, and render suites: PASS.
- `cargo test --no-fail-fast`: PASS, 1,728 main tests and 38 CLI tests; one
  ignored.
- `cargo test --all-targets --all-features --no-fail-fast --quiet`: PASS,
  1,757 main tests, 118 experiment tests, and 38 CLI tests; two ignored.
- `cargo fmt --all --check`, strict all-target/all-feature Clippy,
  `cargo build --release --all-features`, release help/version (`ca 0.1.0`),
  `git diff --check`, snapshot hygiene, and oracle/reference immutability: PASS.

Remaining risks: live provider behavior was not exercised because it requires
credentials and may incur cost. MiMo, DeepSeek, and Codex remain deliberately
provider-managed; Anthropic outbound markers and local GGUF KV tuning remain
separate work.

Harness updates: `CONTINUITY.md`, `ARCHITECTURE.md`, `TESTING.md`,
`NEXT_STEPS.md`, and `docs/.INDEX.md` now record the completed contract and
validation evidence.
