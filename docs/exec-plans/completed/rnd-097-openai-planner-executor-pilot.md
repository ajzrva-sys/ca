# RND-097 — Capped OpenAI Planner/Executor Pilot

Status: COMPLETE
Owner/session: Codex `/root`
Last updated: 2026-08-12

## Goal

Extend the isolated planner/executor experiment into a crash-safe, preregistered, verifier-backed OpenAI Responses pilot and run up to 72 paid trials without allowing local actual plus unresolved reservations to exceed USD 50. The result is an underpowered descriptive pilot, never an automatic routing policy.

## Dependencies and Scope

- WSP-096 is complete. Every trial/recovery uses one exact materialized workspace and retains only a verified patch/recovery artifact plus bounded receipts.
- Allowed implementation: the feature-gated experiment binary and modules, a hidden feature-gated `ca` worker/protocol seam, six dedicated fixtures/verifiers, provider request/usage fields required for exact Responses limits/accounting, Cargo registration, and current harness docs.
- Forbidden: oracle/reference edits, user config/session writes, key printing, request-body retention, opaque state transfer between models, ordinary CLI behavior changes, production routing, commits, installation, or main-checkout execution.

## Frozen Experiment

- Six deterministic stdlib-only Rust fixtures: `unicode_parser_boundary`, `permission_scope_matching`, `protocol_tool_interleave`, `atomic_journal_recovery`, `process_timeout_cleanup`, and `config_migration`.
- Six arms: S0 Sol direct; C0 Luna direct; SS Sol structured plan then fresh Sol; SC Sol structured plan then fresh Luna; SP distinct Sol structured plan deterministically rendered to size-matched prose then fresh Luna; SC-G Sol plan then frozen pre-execution routing with at most one Sol recovery after deterministic verifier failure.
- Models: canonical OpenAI Responses `gpt-5.6-sol` and `gpt-5.6-luna`, both high effort, default service tier, `store=false`, no `previous_response_id`, automatic prompt caching observed but never deliberately warmed.
- Exactly two repetitions: 6 fixtures × 6 arms × 2 = 72 target trials. The first fixture (`unicode_parser_boundary`), repetition one, across all six arms in preregistered balanced order is the included six-slot calibration; no extra paid calibration calls exist.
- SC-G routes to Sol iff the validated artifact has a nonempty unresolved decision; protocol/persistence/concurrency/migration risk; more than three distinct affected files; or inadequate required-gate coverage. Otherwise Luna. Only a deterministic verifier failure may trigger its single Sol recovery.
- SP uses its own Sol call, then a deterministic executor-only prose rendering within ±2% UTF-8 bytes of that arm's artifact. It may use semantically inert fixed padding, never new task information.

## Limits and Ledger

- Each physical request: at most 64,000 input and 8,192 output tokens, below the 272K higher-price tier.
- Planner: one semantic request. Executor: 12 semantic requests, 40 tool calls, ten minutes. Recovery: eight semantic requests and 24 tools.
- One retry is allowed only for a verified pre-output transport failure, and every physical POST has a separate reservation. Physical caps are planner 2, executor 24, recovery 16.
- An owner-only append/fsync hash-chained ledger reserves worst-case frozen-price cost before every physical POST. A send is forbidden when settled actual + unresolved/unknown reservations + new maximum exceeds USD 50. Missing, invalid, conflicting, or interrupted usage retains the full reservation and never becomes zero.
- Freeze a dated official OpenAI price card immediately before traffic. Expected standard per-million rates: Sol 5.00 regular input, 0.50 cached input, 6.25 cache write, 30.00 output; Luna 0.20, 0.02, 0.25, 1.20. Reasoning output is billed at output. Price/model/service drift stops before the next request.

## Architecture and Security Invariants

- `preregister`, `fixture-check`, and `report` are credentialless/offline. `run`/`resume` require the immutable preregistration, fixture receipts, binary/source/price hashes, explicit USD 50 authorization, and canonical route preflight.
- Harness-owned repo/task/authority/budgets/verifier fields live outside planner output. A fresh executor receives only its isolated workspace plus visible structured artifact/prose, never planner transcript or opaque provider state.
- A credential-holding broker performs only bounded OpenAI POSTs and cannot execute tools. A clean-environment worker performs local tools under the required macOS sandbox and cannot inspect broker or parent secret-bearing environment. Paid traffic is blocked until sentinel tests prove that boundary.
- Generated programs and verifiers have network denied. Hidden verifier code/commands/golden solutions never enter a model workspace or request. `CARGO_TARGET_DIR` is always outside materialized workspaces.
- Retained state: preregistration, sanitized request metadata/body digests, usage and verifier receipts, bounded failure classes, unified patches/recovery artifacts, binary digests, and report. Never retain request bodies, secrets, response bodies, opaque reasoning state, hidden verifier source/command, or workspaces after verified cleanup.

## Milestones

### M1 — Protocol, usage, and price accounting

- Add strict bounded pilot/worker/receipt/ledger types, cache-write usage, exact request options, returned-model/request metadata, and one-attempt experiment call policy.
- Add exact fake-provider/price/cap/hash-chain/crash tests without credential access.

### M2 — Fixtures, workspace, and verifier

- Add six baseline/golden/public/hidden deterministic fixtures and prove baseline intended failures plus golden passes.
- Wire WSP-096 materialization, external Cargo targets, patch/recovery receipts, sandboxed clean-env execution, and resume.

### M3 — Commands and isolated execution

- Implement `preregister`, `fixture-check`, `run`, `resume`, and `report` while preserving `schema`, `validate`, and `cost`.
- Prove distinct planner/executor state, SC-G routing/recovery, SP rendering, one reservation per physical POST, no automatic replay, and offline report aggregation.

### M4 — Calibration and paid pilot

- Build released controller/worker and run a credentialless literal-loopback end-to-end test.
- Re-fetch/freeze official prices; preflight exact configured canonical OpenAI models, credential source, required sandbox, fixture/verifier/binary hashes, and local/project USD 50 caps without printing the secret.
- Run the included six-slot calibration. Continue the remaining 66 only if model identity, usage, fixtures, verifiers, sandbox, and projected cost are valid. Stop cleanly at the cap.

## Implemented Result

- The feature-gated controller implements immutable preregistration, all six arms, distinct planner/executor contexts, SC-G routing and one recovery, deterministic SP rendering, isolated WSP workspaces, hidden verification, complete recovery artifacts, safe pre-execution resume, calibration gating, and body-free reports with paired bootstrap intervals.
- Separate framed processes own provider and tool authority. Every physical Responses POST has a price-card-bound reservation and one-attempt policy; ambiguous or invalid usage retains its full reservation. The tool worker has a scrubbed environment, exact grants, no provider credential, and required macOS sandbox denials.
- A released fresh experiment passed all offline gates and reached the credential-only canonical-route preflight. The configured model registry had no `openai` Sol/Luna route, so the broker stopped before its first POST. The durable report proves 72 pending slots, zero events, zero settled/outstanding/unknown USD, and no provider traffic.

## Acceptance and Validation

- Focused feature-bin, protocol, provider, usage, WSP, fixture, ledger, worker, resume, report, CLI, sandbox/env, and loopback tests pass.
- Full `cargo test --no-fail-fast`, formatting, default/all-feature strict Clippy, default and feature release builds, help/version, and snapshot hygiene pass.
- The final report includes paired verified success, false acceptance, exact dollars per verified success, escalation/recovery, latency, five disjoint token categories, and paired bootstrap intervals, labelled underpowered.
- Paid results are reported honestly; inability to pass any safety/credential/provider gate is recorded as blocked with zero subsequent traffic.

## Progress

- [x] Freeze exact 72-slot protocol, calibration, models, efforts, routing, limits, and spend cap.
- [x] Implement and focus-test provider request options, one-attempt policy, response metadata, cache-write usage, and pricing primitives.
- [x] Build six baseline/golden fixture trees and a standalone hidden verifier characterization.
- [x] Wire and compile the current protocol/fixture modules without changing ordinary experiment commands.
- [x] Finish the provider/tool one-call protocol types/framing/tests and return feature strict Clippy to green.
- [x] Apply the narrow sandbox correction and pass Cargo plus parent-process/network/host-HOME denial regressions.
- [x] Bind durable reservations to a persisted frozen price card and exact one-call protocol identity; provider/controller retry/resume integration remains.
- [x] Implement and literal-loopback-test the feature-gated one-POST broker with no tool surface and no durable request/response bodies.
- [x] Implement and sandbox-test the separate constrained tool worker with no config/provider surface.
- [x] Replace fixture-private-copy verification with WSP materialization and complete the 12-candidate isolated fixture sweep.
- [x] Implement and focus-test the WSP trial recovery-bundle reconstruction and exported-workspace reopen core.
- [x] Implement and focus-test offline immutable preregistration with exact 72-slot ordering and pending paid-run gates.
- [x] Bind a successful fixture-check receipt to the immutable preregistration after source/binary/protocol revalidation.
- [x] Build and focus-test the durable trial phase journal that rejects ambiguous automatic resume.
- [x] Wire preregistration/fixture-gated WSP materialization, recovery checkpoints/proof, and journal phase boundaries into an offline trial lifecycle.
- [x] Add read-only `report`/`resume` inspection and a provider-free `run` preflight that fails closed while official pricing is unfrozen.
- [x] Add controller-owned framed worker transport plus lifecycle-bound, body-free provider/tool receipt persistence.
- [x] Implement immutable 24-hour official-price-card receipt retrieval from fixed public documentation, with no provider/config access.
- [x] Add same-phase IPC-only Responses output retention and typed exact-grant file reads/atomic rewrites to the isolated tool worker.
- [x] Wire frozen arm construction and actual broker/tool dispatch into the `run` controller, including ledger-bound retry limits, verifier settlement, and cleanup-retry transitions.
- [x] Freeze a dated official price-card receipt and implement paid `run`/safe `resume` dispatch without weakening the preflight boundary.
- [x] Pass credentialless released end-to-end gates.
- [x] Attempt calibration and record the canonical-provider stop before any request or spend.
- [x] Reconcile docs and archive.

## Remaining Risks

- A local ledger is a hard harness-send cap, not an invoice guarantee; provider adjustments remain possible. A provider-side project budget is the billing backstop and is never mutated automatically.
- The pilot is small and heterogeneous. Even a complete run is descriptive and cannot establish non-inferiority or enable production routing.
- If a configured exact model ID or price differs from the frozen official source, the run stops rather than substituting.

## Completion Record

Changed files:

- `Cargo.toml`; RND experiment/controller modules under `src/bin/reasoning_artifact_experiment*`; `src/reasoning_experiment_{protocol,ledger,worker}.rs`; the bounded provider/usage/sandbox integration seams; six baseline/golden fixture crates and the hidden verifier; focused snapshots and harness documentation.

Commands and results:

- 2026-08-12: default `cargo test --no-fail-fast` passed 1,166 tests with one accepted manual ignore, plus 36 CLI tests. The feature experiment suite passed 112 tests with one intentional external-fixture-unit ignore; the feature reasoning slice passed 47 tests. Default/all-feature strict Clippy, release/default+feature builds, help/version, and snapshot hygiene passed. A released `fixture-check` completed all 12 isolated candidates.

Paid run:

- 2026-08-12: a fresh owner-only preregistration completed fixture verification and froze the official price card. `run --authorize-max-usd 50.00` passed all offline gates and wrote its immutable authorization receipt, then the credential-only broker stopped before its first POST because the canonical `openai` route preflight failed. Sanitized model listing showed only DeepSeek, MiMo, Ofox, and Ollama models; it did not include canonical OpenAI Sol/Luna. The report records 72 pending slots, zero events, and exact USD 0 settled/reserved/unknown. No credential value, provider traffic, or spend occurred.

Remaining risks:

- The pilot remains blocked until a canonical `openai` Responses provider with both frozen `gpt-5.6-sol` and `gpt-5.6-luna` model IDs is deliberately configured. Re-run only with a newly frozen price card (24-hour expiry) and the existing exact USD-50 authorization boundary.

Harness updates:

- `ARCHITECTURE.md`, `CONTINUITY.md`, `NEXT_STEPS.md`, `TESTING.md`, `docs/.INDEX.md`, and `docs/CAPABILITY_EXPANSION_HANDOFF.md` describe the completed implementation and the externally blocked paid sample.
