# PRV-201 — DeepSeek Cache-Usage Compatibility

Status: COMPLETE
Owner/session: current Codex session
Last updated: 2026-08-28

## Goal

Make official DeepSeek Chat and Responses streams complete successfully while
retaining exact protocol-specific cache accounting.

## Non-Goals

- Do not change provider configuration, model availability, reasoning or tool
  continuation behavior, public output schemas, sessions, or TUI presentation.
- Do not weaken required DeepSeek hit/miss validation or apply first-party
  behavior to custom compatible endpoints.
- Do not inspect or print credentials, mutate runtime configuration/sessions,
  or make more than the explicitly authorized two-request live verification.
- Do not modify `oracles/` or `reference/`.

## Evidence and Current Behavior

- The user supplied a live TUI failure reading `unexpected OpenAI cache details
  for hit_miss contract` after selecting DeepSeek.
- `src/agent/provider.rs::cache_usage_from_chat_shape` rejects any
  `prompt_tokens_details` cache member before parsing the configured DeepSeek
  hit/miss counters.
- `src/provider_contract.rs::prompt_cache_metadata` assigns `HitMiss` to both
  DeepSeek protocols even though the Responses decoder accepts
  `OpenAiDetails` and rejects `HitMiss`.
- Official DeepSeek Chat documentation defines
  `prompt_tokens = prompt_cache_hit_tokens + prompt_cache_miss_tokens`.
  Official DeepSeek Responses documentation defines
  `input_tokens_details.cached_tokens`.
- Existing focused DeepSeek usage and provider-contract-verifier tests pass,
  demonstrating that the supplemental Chat shape and resolved Responses usage
  path are not covered.

## Scope

Allowed files: `src/agent/provider.rs`, `src/provider_contract.rs`, focused
tests in those modules, this plan, and root harness documents.

Forbidden files/areas: unrelated dirty changes, provider credentials, user
configuration/sessions, `oracles/`, and `reference/`.

Runtime state affected: only the explicitly authorized release installation
and one capped default-profile live verifier after all source gates pass.

## Design and Invariants

- DeepSeek Chat uses its complete, internally consistent hit/miss pair as the
  sole cache-accounting authority. Supplemental OpenAI-shaped cache details
  are ignored and cannot override or invalidate those counters.
- Missing, non-integer, oversized, or inconsistent hit/miss counters continue
  to fail the known DeepSeek contract.
- DeepSeek Responses uses `OpenAiDetails`; both protocols remain
  provider-managed and send no cache-control fields.
- Preserve the existing `deepseek_v1` contract ID and external usage shape.
- Custom `Auto`, generic unspecified, OpenAI, MiMo, and Anthropic behavior is
  unchanged.

## Milestones

### M1 — Characterization

- Add a byte-split DeepSeek Chat SSE fixture containing valid hit/miss counters
  plus null, numeric, malformed, and contradictory supplemental details.
- Add exact protocol-resolution and DeepSeek Responses usage coverage.

### M2 — Decoder and contract repair

- Make explicit `HitMiss` parsing authoritative and independent of
  `prompt_tokens_details`.
- Select the DeepSeek usage format by protocol without changing request
  behavior or contract identity.

### M3 — Completion and delivery

- Run focused and complete repository gates and reconcile durable documents.
- Back up and atomically install the validated `ca` release, verify artifact
  identity/signature/smokes, and run the live DeepSeek verifier once.

## Acceptance Criteria

- A DeepSeek Chat usage-only SSE chunk with complete hit/miss counters and any
  supplemental `prompt_tokens_details` reaches `Usage` and `Done` without a
  malformed-provider failure.
- Canonical DeepSeek Chat cache reads and misses come only from the reported
  hit/miss pair; writes remain unknown.
- Incomplete or inconsistent hit/miss counters still fail.
- Exact DeepSeek Responses routes decode `input_tokens_details.cached_tokens`
  under `OpenAiDetails` and send no explicit cache fields.
- Focused and completion gates pass; the installed hash matches the validated
  release; the single live verifier reports success or a sanitized no-retry
  failure.

## Validation

Focused: DeepSeek provider decoder, first-party cache contract, Responses
adapter/tool continuation, pricing/usage, and provider-contract verifier tests.

Completion: both workspace test matrices, `cargo fmt --all -- --check`, strict
all-target/all-feature Clippy, all-feature release build, release help/version,
`git diff --check`, snapshot hygiene, and protected-tree verification.

Manual/live: after installation, run
`ca --doctor --connect --provider-contract deepseek` against the default
profile exactly once. The verifier permits at most two no-retry requests.

Blocked checks: none currently; a sanitized live failure is recorded without
retrying or changing configuration.

## Progress

- [x] Current decoder, contract resolver, user evidence, and focused baseline characterized.
- [x] Active ExecPlan and queue record created.
- [x] Failing Chat and Responses regressions added.
- [x] Protocol-specific repair implemented and focused tests pass.
- [x] Completion gates and harness reconciliation complete.
- [x] Validated release installed and single live verifier recorded.

## Decisions

- Decision: treat the documented DeepSeek Chat hit/miss pair as authoritative
  and ignore supplemental OpenAI-shaped details.
  Evidence: the live response reached the explicit cross-dialect rejection;
  DeepSeek documents hit/miss as the Chat billing partition.
  Consequence: forward-compatible response metadata cannot discard a valid,
  already-paid completion or alter its accounting.
- Decision: split DeepSeek usage parsing by protocol while keeping
  `deepseek_v1`.
  Evidence: the official Responses schema uses cached input details, while the
  current Responses decoder rejects `HitMiss`.
  Consequence: the supported Responses route becomes internally consistent
  without changing public identity or outbound controls.

## Surprises and Discoveries

- The model-scoped cache work made the first-party usage dialect explicit but
  did not test a provider returning supplemental compatible fields.
- DeepSeek Responses request/continuation fixtures omit terminal usage, hiding
  the protocol-wide `HitMiss` mismatch.

## Recovery and Rollback

Use only narrow inverse patches; never reset or clean the shared checkout. Keep
the pre-install binary as a timestamped recovery copy. Restore it only if
installed artifact integrity/signature/help/version checks fail. A live
provider failure does not authorize retries or configuration mutation.

## Completion Record

Changed files: `src/agent/provider.rs`, `src/provider_contract.rs`,
`NEXT_STEPS.md`, `CONTINUITY.md`, `TESTING.md`, and this ExecPlan.

Commands and results: both new regressions failed against the old decoder and
resolver, then passed after the repair. The focused `deepseek` filter passed 16
tests, the provider-contract verifier passed four, and cache-usage coverage
passed three. Both complete workspace matrices passed with zero failures: the
default CA target passed 2,115 tests with four ignores, while all-feature CA
passed 2,144 with four ignores and its experiment target passed 118 with one
ignore; CLI, role-run, ca-watch, codex-team, integration, and protocol targets
all passed. Formatting, strict all-feature Clippy, the optimized all-feature
release build, release help/version/signature, diff/snapshot hygiene, and the
committed protected-tree verifier passed. Debug linking emitted the existing
macOS oversized `__eh_frame` warning.

The validated release was installed atomically at
`/Users/aji/.local/bin/ca`. Candidate and installed SHA-256 values match at
`82c72a53a0092c1a274a92b550d5790d530fef39515247bd525144ec1fd603ea`;
the prior verified binary remains at
`/Users/aji/.local/bin/ca.backup-prv201-20260828-005217-e68c2bfb`. Installed
signature, help, version, and path lookup passed. The authorized default-profile
contract verifier ran exactly once and passed `deepseek/deepseek-v4-pro` with
exactly two requests, then reported `ready`.

Remaining risks: the exact supplemental field values are provider-owned; the
regression therefore covers all value classes while keeping documented
hit/miss fields authoritative.

Harness updates: the completed plan, `CONTINUITY.md`, `NEXT_STEPS.md`, and
`TESTING.md` record the regression, protocol split, exact validation,
recoverable installation, and live result.
