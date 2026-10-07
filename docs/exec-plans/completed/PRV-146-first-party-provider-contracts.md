# PRV-146 — First-Party OpenAI, MiMo, and DeepSeek Support

Status: COMPLETE
Owner/session: Codex / current implementation session
Last updated: 2026-08-19

## Goal

Make official OpenAI API and ChatGPT/Codex OAuth, Xiaomi MiMo PAYG/Token Plan,
and DeepSeek routes truthful and reliable across model capabilities, reasoning,
tools, continuation, caching, context limits, usage, pricing, and bounded
opt-in contract verification.

## Non-Goals

- Do not change unrelated providers or generic OpenAI-compatible proxy behavior.
- Do not enable provider-hosted web, shell, MCP, or remote-agent tools.
- Do not add audio/video input or silently rewrite existing provider config.
- Do not run live OAuth, paid requests, install a binary, mutate runtime state,
  inspect credentials, or modify/copy `oracles/` or `reference/`.

## Evidence and Current Behavior

- `src/agent/provider.rs` owns Chat Completions, OpenAI Responses, and
  Anthropic Messages request/stream translation. It already replays bound
  Responses output items and Chat `reasoning_content` through tool continuations.
- Provider-specific Chat decoding currently branches on raw provider IDs for
  MiMo and DeepSeek. Official model capabilities are absent unless the user
  creates exact `model_profiles` entries, so current built-in models do not
  expose their context, modalities, or reasoning controls by default.
- `ReasoningEffort` has `minimal`, `low`, `medium`, `high`, and `max`; current
  OpenAI documents also require `none` and `xhigh`. MiMo and DeepSeek Chat use
  a `thinking.type` switch that CA does not currently serialize.
- New MiMo templates/defaults use Chat Completions even though the current
  official coding-agent integration documents Responses. DeepSeek's proven
  default remains Chat Completions, while its official Responses dialect is
  not certified by dedicated fixtures.
- The price catalog has OpenAI Sol/Luna, MiMo Token Plan, and DeepSeek cards.
  It lacks OpenAI Terra and endpoint/billing-scoped MiMo PAYG accounting.
- `ca --doctor --connect` makes one simple probe per configured provider but
  does not verify tool continuation or expose a family-filtered, hard-capped
  first-party contract report.
- Current official OpenAI documentation identifies GPT-5.6 Sol/Terra/Luna as
  Responses models with text/image input, 1.05M context, 128K maximum output,
  tools, and `none` through `max` reasoning. It recommends Responses for
  reasoning/tool/multi-turn work and documents automatic plus explicit prompt
  caching.
- Inferred: first-party behavior must be keyed by normalized official endpoint,
  protocol, and exact model so a custom proxy named `openai`, `mimo`, or
  `deepseek` does not receive incompatible fields.

## Scope

Allowed files: provider/configuration/request/context/pricing/doctor code,
provider-management TUI code and deterministic snapshots, focused tests, and
current project documentation.

Forbidden files/areas: `oracles/`, `reference/`, credentials, user config and
sessions, installed binaries, Git history, and unrelated provider behavior.

Runtime state affected: none during implementation and automatic validation.
The new live verifier is user-invoked only and will not be run here.

## Design and Invariants

- Resolve an internal first-party dialect only for an allowlisted official
  endpoint plus matching protocol and exact current model. User model profiles
  and legacy context entries override built-in fallbacks; custom endpoints stay
  generic even when their provider ID matches a first-party name.
- Direct OpenAI and subscription-backed Codex share wire capabilities where
  proven but never share price eligibility. MiMo PAYG and Token Plan share the
  model dialect but retain distinct endpoint-bound accounting.
- New MiMo definitions prefer Responses; existing explicit Chat definitions
  remain byte-compatible. DeepSeek remains Chat-first and supports an explicit
  certified Responses route.
- Provider dialects own reasoning serialization, continuation replay, usage
  validation, and unsupported-field exclusion. Secrets and cache keys never
  enter Debug, errors, snapshots, sessions, traces, or reports.
- OpenAI implicit caching is the compatibility default. Stable-prefix and off
  modes apply only to official direct OpenAI Responses requests. The cache key
  is a bounded digest of non-secret process/session identity.
- The opt-in provider-contract verifier performs at most two physical POSTs per
  concrete route, never retries, exposes no real tool authority, and writes no
  session, usage ledger, config, trace, or provider body.

## Milestones

### M1 — Characterization and first-party capability resolution

- Add failing coverage for exact official-route detection, proxy isolation,
  built-in capabilities, profile/context precedence, and new reasoning values.
- Implement the internal dialect/capability resolver and route all capability
  consumers through it.

### M2 — Protocol and caching correctness

- Add exact request fixtures for OpenAI, MiMo, and DeepSeek reasoning modes and
  unsupported-field exclusion.
- Implement dialect-owned Chat/Responses request behavior, default new MiMo
  definitions to Responses, and add OpenAI cache modes/key handling.
- Add two-turn loopback tool-continuation coverage for primary and compatibility
  protocols with fake OAuth only.

### M3 — Catalog, pricing, and usage

- Add current offline model rows, OpenAI Terra, and MiMo PAYG accounting while
  retaining Token Plan and DeepSeek exact cache math.
- Version endpoint/billing-scoped price data compatibly and cover migration,
  stale/failure behavior, and delegation eligibility.

### M4 — Bounded provider-contract verifier

- Add repeatable `--provider-contract` family selection under
  `--doctor --connect`, a versioned sanitized report, and a two-request
  no-retry synthetic local-tool protocol.
- Prove ordinary doctor output and behavior are unchanged.

### M5 — Completion

- Add deterministic user-visible render coverage, run focused and repository
  gates, reconcile durable docs, and move this plan to completed.

## Acceptance Criteria

- Official current OpenAI, MiMo, and DeepSeek models expose truthful built-in
  context, tool, image, and reasoning controls without requiring user profiles;
  user overrides still win and custom proxies receive no first-party behavior.
- OpenAI, MiMo, and DeepSeek primary/compatibility protocols serialize only
  supported fields and complete a two-turn tool continuation hermetically.
- New MiMo definitions use Responses, existing Chat config remains unchanged,
  DeepSeek remains Chat-first, and direct OpenAI uses `store: false`.
- OpenAI caching modes are scoped to direct official Responses requests, cache
  keys are stable and private, and usage/cost accounting distinguishes reads,
  writes, and uncached input without underflow.
- OpenAI Terra, MiMo PAYG/Token Plan, and DeepSeek price/account routes are
  endpoint/billing bound; Codex subscription routes never receive API prices.
- The explicit provider-contract command is family-filtered, sanitized,
  no-retry, and physically capped at two POSTs per route; no automatic test
  performs provider traffic.
- No runtime configuration/session/credential store, installed binary,
  oracle/reference file, or unrelated provider is changed.

## Validation

Focused: config/capability, provider wire/continuation, client, pricing/usage,
doctor/CLI, orchestration, and provider-management render suites.

Completion: `cargo test --no-fail-fast`, `cargo test --all-targets
--all-features --no-fail-fast`, `cargo fmt --all --check`, `cargo clippy
--all-targets --all-features -- -D warnings`, `cargo build --release
--all-features`, help/version, snapshot hygiene, `git diff --check`, and
oracle/reference diff checks.

Manual/visual: review deterministic wide/narrow snapshots. Do not run the live
provider-contract command during implementation.

Blocked checks: native live provider execution requiring credentials or spend.

## Progress

- [x] Read the control harness and official OpenAI contract; baseline current source and dirty-tree ownership.
- [x] Add failing first-party capability and reasoning characterizations.
- [x] Implement exact official-route capability resolution.
- [x] Implement provider-aware wire behavior and OpenAI caching modes.
- [x] Implement catalog, pricing, and usage changes.
- [x] Implement the bounded opt-in provider-contract verifier.
- [x] Run focused and completion validation; reconcile documentation.

## Decisions

- Decision: official endpoint, protocol, and exact model jointly select the
  first-party dialect.
  Evidence: raw provider-ID branching can misclassify compatible proxies.
  Consequence: custom endpoints remain generic and fail closed.
- Decision: MiMo defaults to Responses only for newly generated definitions;
  DeepSeek remains Chat-first.
  Evidence: current official MiMo coding integration recommends Responses,
  while CA's existing DeepSeek Chat continuation is proven and officially
  supported.
  Consequence: no saved user config is migrated.
- Decision: live contract verification is explicit and capped.
  Evidence: the user selected an opt-in smoke command and automatic spend is
  outside implementation authority.
  Consequence: hermetic loopback tests are the completion evidence.

## Surprises and Discoveries

- The working tree already contains hundreds of user-owned modifications,
  including `src/main.rs`, `src/config.rs`, `src/agent/client.rs`, and all
  durable harness documents. PRV-146 patches the current working versions
  surgically and will not reset or normalize unrelated changes.
- Persisted Token Plan cards used JSON `u128` fields that `serde_json` could
  serialize but the derived deserializer rejected. A bounded integer visitor
  now reads existing numeric cards, which makes catalog-v1 in-memory upgrade
  executable instead of merely nominal.
- Process usage records intentionally do not persist endpoints. Runtime cost
  presentation therefore resolves the current configured official endpoint
  and billing mode before selecting a v2 card; a named custom proxy receives
  no first-party price.

## Recovery and Rollback

Revert only PRV-146 hunks with targeted patches. Do not reset the dirty
worktree, restore files from `HEAD`, run broad snapshot acceptance, or discard
unrelated user changes. New files can be removed individually if abandoned.

## Completion Record

Changed files:

- `src/provider_contract.rs`, `src/config.rs`, `src/model_catalog.rs`, and
  provider templates/connect code now resolve exact first-party capabilities,
  defaults, and model profiles while leaving custom endpoints generic.
- `src/agent/provider.rs`, `src/agent/client.rs`, tests, and
  `src/provider_trace.rs` now own dialect-specific reasoning, continuation,
  cache, usage, unsupported-field, and redaction behavior.
- `src/pricing.rs` and `src/usage.rs` now use the compatible endpoint/billing-
  scoped catalog-v2 contract for OpenAI Terra, MiMo PAYG/Token Plan, and
  DeepSeek accounting.
- `src/provider_contract_verifier.rs` and `src/main.rs` add the explicit,
  family-filtered, sanitized two-request contract verifier. Provider-editor
  state/input/render code exposes direct-OpenAI prompt-cache policy.

Commands and results:

- Focused provider request/continuation, config, catalog, pricing, usage,
  trace-redaction, verifier, doctor/CLI, and provider-editor render/input
  suites: PASS.
- `cargo test --no-fail-fast`: PASS, 1,645 main tests and 37 CLI tests; one
  documented ignore.
- `cargo test --all-targets --all-features --no-fail-fast`: PASS, 1,674 main,
  118 experiment-target, and 37 CLI tests; two documented ignores.
- `cargo fmt --all --check`, strict all-target/all-feature Clippy,
  `cargo build --release --all-features`, local release help/version
  (`ca 0.1.0`), `git diff --check`, snapshot hygiene, and oracle/reference diff
  checks: PASS.

Remaining risks:

- The opt-in live OpenAI, MiMo, and DeepSeek contract commands were not run;
  they require user credentials and may incur provider charges. Hermetic fake-
  credential and loopback transport tests are the completion evidence.
- Official provider contracts and prices can change after 2026-08-19; refresh
  remains explicit and stale cards continue to fail closed.

Harness updates:

- `CONTINUITY.md`, `TESTING.md`, `NEXT_STEPS.md`, `ARCHITECTURE.md`,
  `docs/.INDEX.md`, and `docs/INTER_PROVIDER_DELEGATION.md` record the completed
  contracts, validation, operating guidance, and remaining live-verification
  boundary.
