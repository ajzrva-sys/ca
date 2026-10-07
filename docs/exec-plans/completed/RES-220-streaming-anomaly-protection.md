# RES-220 — Streaming Anomaly Protection

Status: COMPLETED
Owner/session: ORC-218 goal session
Last updated: 2026-08-29

## Goal

Add a provider-neutral, bounded stream guard after provider wire decoding and
before canonical turn assembly. Stop clear repetition loops and unfenced
pseudo-tool envelopes without executing prose, replaying usable output, or
misclassifying ordinary repetition, code examples, or valid provider tool
events.

## Scope and Invariants

- Guard decoded text and reasoning events for every concrete `AgentClient`
  route; provider decoders and public request/session schemas remain unchanged.
- Retain at most 64 KiB of detector state. Detection must work across arbitrary
  byte/chunk boundaries and remain Unicode safe.
- Stop three repeated bounded prose spans or four near-identical consecutive
  prose paragraphs after conservative normalization.
- At terminal response events, reject a response composed primarily of an
  unfenced JSON/XML tool envelope that names an advertised tool. A real decoded
  tool event disables pseudo-tool classification for that response.
- Fenced examples, ordinary repeated code, short natural repetition, and valid
  tool calls must pass unchanged.
- Forward all safe earlier events before a typed, secret-free anomaly error;
  returning from the transport task drops the response stream and cancels it.
- No anomaly is retryable or eligible for provider fallback. Interactive users
  retain the partial turn through existing state/error handling and may use the
  existing branch-based retry; headless/service callers retain the provider
  protocol failure class.
- Preserve all pre-ORC dirty bytes and never modify `oracles/`, `reference/`,
  runtime configuration, sessions, credentials, or the installed binary.

## Milestones

1. Add failure-first detector tests for Unicode/chunk splitting, repetition,
   pseudo-tool envelopes, false-positive controls, bounds, and typed errors.
2. Implement the bounded provider-neutral guard and integrate it between
   decoding and event publication for both chunk and decoder-finish paths.
3. Add transport fixtures proving cancellation, partial-output preservation,
   no replay, and headless provider-protocol classification.
4. Run focused suites, formatting, strict all-feature Clippy, all-feature
   release, and reconcile the ORC harness before completion.

## Validation

- Unit tests for three-span and four-paragraph detection, Unicode, fences,
  code, valid tool events, short repetition, pseudo JSON/XML, and state caps.
- Loopback streaming tests proving earlier text delivery, immediate transport
  drop, one physical request, and no post-anomaly events.
- Existing provider decoder, client, stream, fallback, service/headless, and
  tool-continuation tests.
- `cargo fmt --all -- --check`; strict workspace all-target/all-feature Clippy;
  all-feature release. The two complete workspace matrices remain the final
  ORC-218/REL-229 gates.

## Progress

- [x] Child plan and queue entry created after DIA-219 completion.
- [x] Failure-first coverage added.
- [x] Bounded guard and typed failure implemented.
- [x] Provider transport and caller behavior verified.
- [x] Focused tests and child gates pass.
- [x] Harness reconciled and plan completed.

## Recovery

Reverse only RES-220-labelled hunks with `apply_patch`. All HTTP fixtures are
loopback-only and disposable; no provider or user runtime path is used.

## Completion Record

Changed files: new `src/agent/stream_anomaly.rs`; focused integration in
`src/agent/{mod,client,client_tests,stream}.rs`; provider-probe and service
classification tests; and ORC queue/continuity/testing/index records.

Commands and results:

- Failure-first guard cases initially failed for repetition and pseudo-tool
  classification, then passed 10/10 after implementation. Coverage includes
  arbitrary Unicode fragmentation, exact three-span loops, four near-identical
  paragraphs, independent reasoning, fences, ordinary repeated code, short
  repetition, JSON/XML pseudo tools, malformed/other-tool controls, genuine
  tool events, rolling-state bounds, and oversized advertised-tool fail-close.
- The byte-at-a-time loopback proves two prior Unicode spans are delivered,
  the third is rejected with `StreamAnomalyKind::RepetitionLoop`, the response
  body is dropped, and a two-attempt policy makes no replay. A second loopback
  preserves pseudo-tool text but produces no tool event and fails with
  `SuspectedToolEnvelope`.
- Existing stream assembly passed 17/17, provider decoders 34/34, client and
  protocol transports 47/47, fallback 4/4, provider probes 8/8, and service
  behavior 25/25. The existing main-TUI partial-output preservation regression
  also passed for the shared failure path.
- `cargo fmt --all -- --check`, strict workspace all-target/all-feature
  Clippy, and `cargo build --workspace --release --all-features` passed.
- `git diff --check`, zero `.snap.new`, and direct `oracles/`/`reference/`
  inspection passed.

Remaining risks: the deliberately conservative prose heuristics may leave an
ambiguous repetition or pseudo-tool-shaped response untouched; they are not a
general content classifier. Clear detections fail closed and never replay or
execute text. Complete workspace matrices remain the integrated REL-229 gate.
The existing macOS oversized `__eh_frame` linker warning remains non-fatal.

Harness updates: the umbrella plan, queue, continuity, testing map, and index
record this completion. No public request/session/configuration schema,
provider contract, protected subtree, runtime state, or installed binary
changed.
