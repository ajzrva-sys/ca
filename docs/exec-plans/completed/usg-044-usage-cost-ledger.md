# USG-044 — Usage and Cost Ledger

Status: COMPLETE
Owner/session: current Codex task
Last updated: 2026-08-10

## Goal

Add a process-only `/usage` ledger that truthfully records every TUI completion request, including main turns, tool continuations, context checkpoints, reviews, subagents, provider probes, and reserved check/`/btw` categories, with provider/model, token subsets, duration, outcome, and optional exact-decimal configured cost.

## Non-Goals

- Do not bundle, fetch, or infer provider prices.
- Do not persist usage into config, sessions, history, logs, or snapshots of runtime secrets.
- Do not add `/btw`, headless `--check`, provider fallback, or protocol adapters in this plan; only make their ledger categories ready.
- Do not change existing `ca.headless.v1` output for providers that omit detailed token subsets.
- Do not edit oracle/reference trees or claim Cursor parity.

## Evidence and Current Behavior

- `UsageInfo` currently contains prompt/completion/total counts only. The SSE parser ignores OpenAI cached-prompt and reasoning-token detail fields (`src/agent/stream.rs`).
- TUI streaming stores the last prompt/completion pair in `StreamingState`, adds it to a two-counter session total, and `/context` displays only that aggregate (`src/{app,main}.rs`). There is no per-request identity, duration, outcome, provider/model provenance, or cost.
- TUI completion traffic also originates in context checkpoints, fresh reviews, provider probes, and provider-backed subagents. Those runners currently discard assembled usage (`src/{context,review,provider_probe}.rs`, `src/agent/subagent.rs`).
- Headless already emits provider usage events directly. This plan leaves that protocol intact and builds the process ledger at the TUI orchestration boundary.
- Ante documents context/cost visibility, but no clean Cursor fixture establishes this ledger. This is an oracle-informed `ca` extension.

## Scope

Allowed: a new `src/usage.rs`; focused changes to `src/{app,config,context,main,provider_probe,review}.rs`, `src/agent/{stream,subagent}.rs`, TUI command/render/input tests and snapshots, Cargo metadata only if exact arithmetic cannot stay dependency-free, this plan, and the five harness documents.
Forbidden: oracle/reference edits, real config/session/Keychain state, external provider traffic, persistence schema changes, and unrelated runner refactors.
Runtime state affected: process memory only. Optional pricing is read from the selected profile config; tests use temporary files and loopback/fake providers.

## Design and Invariants

- `UsageLedger` is a shared process-only boundary. A record has a monotonic ID, typed request kind/outcome, provider/model IDs, start/duration, and optional `UsageInfo`; it never holds provider configs, endpoints, headers, prompts, generated text, or credentials.
- Every TUI request begins exactly one record before provider I/O and finishes it exactly once on completed, failed, or cancelled execution. Tool-loop continuations are distinct from the initiating main request.
- Background review, checkpoint, subagent, and probe runners receive only a cloned safe recorder plus provider/model identity. Stale UI generations cannot cause a second record.
- Token details parse optional cached-prompt and reasoning subsets. Counts are clamped to their parent prompt/completion count for cost and marked invalid in tests if the provider exceeds those parents; ordinary total remains provider-reported.
- Pricing is keyed by exact `provider/model` and uses quoted non-negative decimal USD-per-million strings for input/output plus optional cached-input/reasoning-output overrides. Arithmetic is integer/decimal only, never binary floating point. Missing pricing leaves cost visibly `unknown`; there is no default table.
- `/usage` is read-only and available while other work runs. It opens a bounded report showing totals and newest records, including running requests. The report reads a point-in-time clone; rendering never holds the ledger lock.
- Existing `token_count` remains as a compatibility projection during this slice, but is updated from the same full usage sample so totals cannot diverge.

## Milestones

### M1 — Usage and pricing characterization
- Add failing tests for detailed usage parsing, exact decimal validation/arithmetic, missing/partial pricing, monotonic record lifecycle, duplicate completion, cancellation, and redacted debug/state.
- Validation: focused `usage` and stream/config tests.

### M2 — Central TUI instrumentation
- Add the ledger and instrument main requests/continuations, context checkpoints, reviews, provider probes, and provider subagents. Cover success, provider error, cancellation, and multi-turn tool/review/agent loops.
- Validation: fake/loopback runner and orchestration tests with exact one-record assertions.

### M3 — `/usage` report and terminal states
- Add the command, safe report construction, active-profile pricing lookup, empty/running/populated/unknown-price/error-outcome states, and reviewed 100×30/60×24 snapshots.
- Validation: command/input/report/render tests and snapshot review.

### M4 — Completion gates and reconciliation
- Run full tests, formatting, strict Clippy, release, smoke, and an isolated temporary-home fake-provider walkthrough; reconcile docs and archive before PRV-045.

## Acceptance Criteria

- Each existing TUI completion request produces one provider/model-attributed record, with main and continuation requests distinct and background runner categories correct.
- Completed records retain optional prompt/completion/cached/reasoning counts; failures without usage remain explicit rather than showing zero; cancellation closes a running record once.
- `/usage` is safe during an active request and shows process-only totals, durations, outcomes, exact configured cost, and `unknown` whenever applicable.
- Decimal price parsing rejects negative, exponent, floating TOML, excessive precision, traversal-like keys, and overflow before provider traffic; existing config loads unchanged.
- No request content, response content, endpoint, header, credential, or secret length enters ledger state, debug output, snapshots, sessions, or config writes.

## Validation

Focused: PASS — 13 usage/parser/config/command/orchestration/render tests plus loopback review, provider-probe, and provider-subagent record assertions.
Completion: PASS — 474 unit tests and 25 CLI tests; formatting, strict Clippy, release, and local help/version/profile smoke passed.
Manual/visual: PASS — four wide/narrow usage snapshots were reviewed. A rebuilt TUI under a temporary home showed both a malformed-stream failed record and a successful credentialless loopback record with detailed subsets and exact configured cost; all temporary state was removed.
Blocked checks: none.

## Progress

- [x] Inspected current usage parsing, TUI aggregation, and all existing TUI completion-request origins.
- [x] Add failing usage/pricing/lifecycle tests.
- [x] Instrument all existing TUI request origins.
- [x] Add and review `/usage` states.
- [x] Run completion gates, reconcile docs, and archive before PRV-045.

## Decisions

- Decision: keep the ledger at the TUI application boundary rather than changing the stable headless event protocol.
  Evidence: headless already emits usage per request, while the user-facing gap is a process `/usage` history spanning TUI background runners.
  Consequence: shared public ledger types are reusable by later service/headless work without altering `ca.headless.v1` here.
- Decision: require quoted exact decimal prices keyed by exact provider/model.
  Evidence: no bundled table is allowed and floating TOML would lose the exactness promised by the plan.
  Consequence: invalid prices fail config loading with a local actionable error; unspecified prices remain unknown.

## Surprises and Discoveries

- Current `StreamingState` collapses usage to a prompt/completion tuple, so cached/reasoning details and provider/model identity are lost before the turn finishes.
- Review and subagent loops may issue several provider requests inside one user-visible operation; the ledger must record each request, not one aggregate operation.

## Recovery and Rollback

Revert only USG-044 patches. Tests construct ledgers in memory and configs/servers under temporary roots. Never use destructive Git operations or real runtime state.

## Completion Record

Changed files: `src/{app,config,doctor,main,provider_probe,review,usage}.rs`, `src/agent/{stream,subagent}.rs`, `src/tui/{commands,input_tests,render_tests}.rs`, four usage snapshots, this plan, and the five harness documents.
Commands and results: focused usage and loopback runner suites PASS; full tests PASS (474 unit + 25 CLI); format PASS; strict Clippy PASS; release PASS; smoke and isolated live TUI PASS.
Remaining risks: provider usage fields are optional and provider-reported; check and `/btw` categories stay reserved until HED-056 and BTW-046 add those request paths.
Harness updates: architecture, continuity, testing, next steps, and parity record USG-044 as an oracle-informed `ca` extension and route the next handoff to PRV-045.
