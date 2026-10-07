# AGT-106 — macOS Inter-Provider Delegation

Status: COMPLETE
Owner/session: Codex implementation session
Last updated: 2026-08-15

## Goal

On macOS, make `ca` safely route explicitly complexity-labelled `task` and
`spawn_agent` work across configured OpenAI and Xiaomi MiMo routes, while
repairing MiMo browser authorization, supporting masked Token Plan key setup,
and refusing automatic delegation unless capability, credential, billing, and
fresh official pricing evidence are complete.

## Non-Goals

- Making a live OAuth, provider-probe, or paid model request during
  implementation or acceptance testing.
- Making PAYG MiMo, OpenAI Codex subscription/OAuth, non-USD Token Plans,
  unknown models, or legacy user-entered price rows delegation-eligible.
- Expanding Linux or FreeBSD provider setup/routing behavior in this slice.
- Scraping Xiaomi private account endpoints or treating models.dev as an
  authoritative billing source.
- Retrying on a different provider after a child request has started.

## Evidence and Current Behavior

- `src/mimo_auth.rs::receive_callback` accepts exactly one socket. A zero-byte
  Firefox speculative connection returns `request was too large`, drops the
  listener, and leaves the real browser callback with connection refused.
- `src/agent/registry.rs::{TaskArgs,SpawnAgentArgs}` have no complexity field;
  ordinary manager spawns set `AgentSpawnRequest.route` to `None`.
- `src/agent/subagent.rs::ProviderAgentRunner` supports an exact route client,
  but route presence also selects multi-run accounting, conflating route and
  purpose.
- `src/main.rs::run_headless` does not construct an agent manager, while the
  headless executor rejects interaction/subagent effects.
- `src/usage.rs::UsagePricing` is a fixed USD metered card. It has no source,
  verification time, long-context tier, capability, Token Plan credit mode, or
  request wall-clock interval.
- Official OpenAI model pages publish the requested Sol and Luna rates, a
  1.25x cache-write multiplier, and the >272K 2x-input/1.5x-output rule.
- Xiaomi's public Token Plan documentation publishes plan pools, monthly and
  annual USD list prices, model credit weights, and a UTC 16:00–24:00 0.8x
  window. No supported account-plan lookup API is published.
- Xiaomi MiMoCode reads models.dev for PAYG estimates; that is advisory and
  does not establish Token Plan billing eligibility.

## Scope

Allowed files: `src/` authentication, provider/config, pricing/usage, routing,
agent, TUI, headless, doctor, and CLI modules; their deterministic tests and
snapshots; user-facing setup/routing/pricing docs; the AGT-106 harness record.

Forbidden files/areas: `/Users/aji/Documents/ChatGPT/ca`, `oracles/`,
`reference/`, real credentials, user sessions/configuration, and unrelated
dirty-tree edits.

Runtime state affected: only the final explicitly requested atomic replacement
of `/Users/aji/.local/bin/ca`, after preserving the prior binary as a backup.
All tests use loopback/fake transports, temporary config/key stores, and frozen
official-page fixtures.

Stop conditions: any test path would contact a provider or OAuth endpoint; a
required credential would need to be read; an unrelated dirty edit overlaps in
a way that cannot be preserved; an install target cannot be identified and
backed up exactly.

## Design and Invariants

- The MiMo callback listener keeps accepting until one structurally valid
  callback or the existing five-minute deadline. Each connection has a
  two-second header deadline; no more than 16 invalid sockets are tolerated.
- Loopback peer, exact Host, GET, callback path, header termination, 128 KiB
  request cap, and 96 KiB result cap are validated before the one-use private
  key is consumed. Decryption precedes a success response. Queries, ciphertext,
  keys, and UID never enter logs or Debug output.
- Manual MiMo setup accepts only a masked `tp-` Token Plan key and an explicit
  CN, Singapore, or Europe regional endpoint. The key is persisted only through
  the provider credential reference; setup retains transactional probe and
  rollback semantics.
- Token Plan billing metadata records tier, term, exact USD paid, exact official
  list amount, confirmation when those differ, and no inferred account state.
- A typed official pricing catalog owns provider/model identity, capability,
  billing mode, exact source URL, and verification time. Cards expire after 30
  days and refresh publishes only an atomically validated complete catalog.
- OpenAI metered and MiMo credit/allocation arithmetic is exact. Missing or
  inconsistent token subsets and MiMo requests that straddle the off-peak
  boundary have unknown actual cost; no subset is silently assumed to be zero.
- Complexity omission preserves the parent route. A supplied complexity
  chooses among ready route clients whose capability is sufficient and whose
  official pricing and supported billing metadata are current.
- Ranking uses bounded estimated input plus 4,096 output tokens, uncached OpenAI
  input, peak MiMo credits, then lower capability and lexical provider/model.
  Route selection finishes before provider traffic or usage recording and is
  never retried across providers after child start.
- Route and run purpose are distinct. Routed ordinary children are `Subagent`,
  not multi-run candidates. Only explicit task text and a bounded context
  summary cross providers; opaque reasoning and credentials remain local.
- Headless advertises `task` and `spawn_agent` unless excluded or hard-denied,
  but child grants remain read-only: bounded file read/search/list/metadata
  only. Shell, writes, network, interactive tools, and mutating named agents are
  rejected before spawn regardless of yolo, auto-review, or allow-network.
- Every headless exit shuts down the manager. Active asynchronous children are
  cancelled and reaped, and otherwise-successful completion reports an
  incomplete-subagents failure.

## Milestones

### M1 — Characterize and repair MiMo setup

- Add socket-level tests for speculative empty, partial timeout, oversized,
  wrong peer/host/path, invalid ciphertext, success redirect, and overall
  timeout behavior.
- Implement the multi-connection receiver and decrypt-before-success boundary.
- Add browser/manual selection, masked Token Plan key, region, tier/term/USD
  metadata, mismatch confirmation, cancellation/retry, and secret-free TUI
  coverage.

### M2 — Typed official pricing and billing

- Add exact metered and Token Plan cards with seeded source evidence,
  capabilities, freshness, long-context, credit/off-peak, and allocation math.
- Add strict bounded official parsers, atomic cache refresh, advisory
  models.dev comparison, `ca pricing status|refresh` text/JSON, and doctor
  readiness output.

### M3 — Automatic route selection

- Add optional complexity to both subagent tools and a deterministic
  fail-closed router using all currently constructed configured route clients.
- Refresh route clients and metadata after provider mutation, separate run
  purpose from routing, expose selection/forecast, and record actual usage as
  `Subagent`.

### M4 — Restricted headless integration

- Construct and dispatch through a headless agent manager, force child grants
  to the closed read-only subset, retain tool include/exclude policy, and make
  shutdown/reaping exhaustive across every exit.
- Add mocked end-to-end Sol-to-Luna and Sol-to-MiMo runs with zero external
  traffic and explicit mutation/network denials.

### M5 — Documentation, completion gates, and install

- Update setup, pricing refresh, route hints, headless restrictions,
  architecture, continuity, testing evidence, and next-step status.
- Run focused tests, snapshot review, `cargo fmt --check`, strict all-target and
  all-feature Clippy, complete all-target/all-feature tests, and a macOS release
  build.
- Back up and atomically replace `/Users/aji/.local/bin/ca` only with the
  validated release artifact; record hashes and leave paid live verification
  explicitly deferred.

## Acceptance Criteria

- Firefox-style empty/partial loopback sockets cannot consume the callback or
  private key; a following valid callback completes and receives success only
  after decryption.
- Manual setup never renders a Token Plan key, accepts only `tp-...`, selects
  the documented region endpoint, persists only a credential reference, and
  records confirmed USD plan metadata.
- All four seeded cards produce exact forecast/actual arithmetic, including
  long-context and MiMo off-peak/allocation edge cases; stale/incomplete data is
  visibly unknown and cannot route.
- `pricing status`, `pricing refresh`, and offline doctor provide stable text
  and JSON facts without secrets; invalid/partial official pages leave the
  prior catalog intact.
- Complexity omission inherits. Supplied complexity deterministically picks
  the cheapest eligible route, includes same-provider candidates, refuses
  missing/stale/unsupported routes before traffic, and reports its
  provider/model forecast.
- TUI children retain existing approval behavior. Headless children can only
  use the closed read-only tool set, and every exit cancels/reaps/shuts down.
- Mocked headless tests demonstrate separate Sol-to-Luna and Sol-to-MiMo simple
  delegation and `Subagent` usage without credentials in any event/log.
- Required format, Clippy, complete tests, and release build pass on macOS; the
  installed binary has a recoverable prior-version backup. No paid request ran.

## Validation

Focused: PASS. MiMo socket (8), pricing/accounting/router, provider-auth input
and snapshots, transactional billing rollback, doctor, subagent manager, and
28+ headless tests pass with loopback/fake transports only.

Completion: PASS. `cargo fmt --check`, strict all-target/all-feature Clippy,
the full all-target/all-feature suite, the all-feature release build, and the
default release build all passed on macOS arm64. The full suite recorded 1,321
main tests passed with one manual Chromium test ignored, 115 experiment tests
passed with one complete external-fixture test ignored, and all 36 CLI tests
passed.

Manual/visual: new wide/narrow method, region, masked-key, and price-confirmation
snapshots reviewed; no secret text appears. Offline CLI walkthrough remains for
the release artifact.

Blocked checks: live Sol-to-Luna/MiMo calls are intentionally forbidden by the
acceptance policy and will remain deferred.

## Progress

- [x] Verified the repository harness, dirty-tree boundaries, relevant source
  symbols, and official public pricing evidence.
- [x] Added callback and routing/pricing characterization tests.
- [x] Completed MiMo browser and Token Plan setup milestone.
- [x] Completed typed pricing and official refresh milestone.
- [x] Completed TUI/headless automatic routing milestone.
- [x] Completed docs, gates, release build, and recoverable install.

## Decisions

- Decision: implement automatic routing only when an explicit complexity is
  present.
  Evidence: compatibility requires omitted hints to preserve the existing
  inherited route.
  Consequence: existing callers and saved schemas continue behaving as before.
- Decision: browser MiMo credentials remain PAYG-compatible but ineligible for
  delegation; only confirmed USD Token Plans qualify.
  Evidence: Xiaomi publishes no account-plan lookup API and MiMoCode's
  models.dev estimates are PAYG data.
  Consequence: routing fails closed without explicit Token Plan setup metadata.
- Decision: use direct official model/doc pages as authority and models.dev
  only as an advisory discrepancy.
  Evidence: the requested billing modes and credit pools are published only by
  OpenAI/Xiaomi first-party pages.
  Consequence: advisory availability cannot make an otherwise unknown route
  eligible.

## Surprises and Discoveries

- The user's installed binary and current release artifact initially have the
  same SHA-256, so a byte-for-byte backup can be verified before replacement.
- The current working tree contains extensive prior in-progress tracked and
  untracked authentication/provider work; AGT-106 must layer narrowly without
  resetting or staging unrelated files.

## Recovery and Rollback

Reverse only AGT-106-scoped hunks with targeted patches; never reset the dirty
tree. Pricing cache publication uses same-directory temporary files plus atomic
rename. Provider setup retains the existing staged-config/credential rollback.
The final installed binary is recoverable from the explicit backup created
immediately before atomic replacement.

## Completion Record

Changed files: scoped changes span `src/mimo_auth.rs`, `src/pricing.rs`,
provider/config/usage/doctor integration, subagent and headless orchestration,
TUI state/input/render plus snapshots, CLI wiring, and user/harness documents.
The pre-existing dirty and untracked authentication/provider work was
preserved; `/Users/aji/Documents/ChatGPT/ca` was not touched.

Commands and results:

- Focused MiMo callback, Token Plan setup, pricing/accounting/router, doctor,
  subagent, headless, and snapshot tests: PASS using only loopback/fake inputs.
- `cargo fmt --check`: PASS.
- `cargo clippy --all-targets --all-features -- -D warnings`: PASS.
- `cargo test --all-targets --all-features --no-fail-fast`: PASS; 1,321 main
  passed/1 manual ignored, 115 experiment passed/1 external-fixture ignored,
  and 36 CLI passed.
- `cargo build --release --all-features`: PASS.
- `cargo build --release`: PASS; installed artifact is macOS arm64 Mach-O.
- Disposable-home `ca --version`, `ca pricing status`, and JSON pricing status:
  PASS with four fresh cards and no runtime files created.

Install record: `/Users/aji/.local/bin/ca` was atomically replaced from the
default release artifact. Both have SHA-256
`e3d79cef89cb692e37871a1b751e5fce8475be52cd2e90dd0616cf4bb56ce65c`.
The recoverable previous binary is
`/Users/aji/.local/bin/ca.pre-agt106-20260815`, SHA-256
`32a4f164f46109394e8e981bb9e1aba406692abd867f6d5553d97ffedecbac6c`.

Remaining risks: live Sol-to-Luna/MiMo verification is deliberately deferred
because the acceptance policy forbids paid calls. `ca pricing refresh` was not
executed during acceptance; its bounded official-source parsers and atomic
failure behavior are covered by offline tests. Cards fail closed after 30 days
until an operator refreshes them.

Harness updates: the completed plan, NEXT_STEPS, architecture, continuity,
testing guide, knowledge index, and operator guide all contain AGT-106 state
and recovery evidence.
