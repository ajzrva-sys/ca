# PRV-057 — Protocol-Neutral Provider Boundary

Status: COMPLETE
Owner/session: current Codex task
Last updated: 2026-08-11

## Goal

Preserve the existing OpenAI-compatible Chat Completions wire contract byte-for-byte while making provider protocol selection, request serialization, authentication, stream decoding, usage mapping, and safe failures adapter-owned behind the shared `PreparedModelRequest` and `ProviderEvent` boundaries.

## Non-Goals

- OpenAI Responses support; PRV-058 owns that adapter.
- Anthropic Messages support; PRV-059 owns that adapter.
- OAuth, provider fallback, MCP transport, UI protocol editing, or runtime protocol switching.
- Any Cursor parity claim. This is an oracle-informed `ca` architecture extension.

## Evidence and Current Behavior

- `src/agent/client.rs` owns the `/chat/completions` suffix, bearer authentication, the private `ChatRequest` JSON shape, retries, and direct construction of `SseDecoder`.
- `src/request.rs` provides the semantic `PreparedModelRequest` used by interactive, headless, scripted, review, checkpoint, subagent, side-question, and provider-probe runners. `RequestSnapshot` currently hard-codes `chat_completions`.
- `src/agent/stream.rs` already defines protocol-neutral terminal events, usage, finish reasons, safe typed errors, and complete-turn assembly, but its `SseDecoder` maps the current Chat Completions SSE dialect directly.
- `src/config.rs::ProviderConfig` has endpoint, API-key reference, and models only. Missing protocol configuration therefore implicitly means Chat Completions today.
- Existing client tests characterize JSON field presence and tool-result continuation semantically, but do not freeze exact serialized bytes, endpoint/auth behavior, or retry-body identity.
- Ante documents multiple provider families in `oracles/ante-main/docs-site/docs/usage/providers.mdx`; that is descriptive product evidence only. No Ante runtime behavior or clean Cursor provider-protocol fixture is claimed.

## Scope

Allowed files:
- `src/agent/{mod,client,stream,provider}.rs` and focused tests.
- `src/{config,request,main,provider_probe,review,headless,test}.rs` only where construction or safe snapshot plumbing requires it.
- Existing unit/integration fixtures outside oracle/reference trees.
- `ARCHITECTURE.md`, `CONTINUITY.md`, `TESTING.md`, `NEXT_STEPS.md`, `PARITY.md`, `docs/.INDEX.md`, and this plan.

Forbidden files/areas:
- `oracles/`, `reference/`, real user configuration, Keychain data, saved sessions, and the installed binary.
- PRV-058/059 protocol implementations.

Runtime state affected:
- None during tests. Manual validation uses only a temporary home, credentialless loopback provider, and no-session mode.

## Design and Invariants

- `PreparedModelRequest` remains the canonical semantic request. It contains provider/model IDs, messages, tool schemas, options, and estimates, but never endpoints, headers, or resolved credentials.
- `ProviderEvent`, `UsageInfo`, `FinishReason`, and `ProviderError` remain the canonical runner-facing stream contract.
- Add a defaulted, serialized `ProviderProtocol` to provider configuration. The only accepted value in this slice is `chat_completions`; an absent value preserves current behavior without rewriting TOML. Future variants are added only with their complete adapters.
- A protocol adapter owns endpoint suffix, exact request bytes, runtime authentication application, and the decoder that maps bytes into canonical events. Runtime credentials stay in `AgentClient` and are passed only at request construction.
- The Chat Completions adapter serializes the same private ordered Rust struct through `serde_json::to_vec`, preserving the existing `.json(&body)` bytes. Retries reuse the exact same byte buffer.
- The existing safe error boundary remains body-free and secret-free. Unsupported/invalid protocol configuration fails before provider traffic.
- `RequestStore` records the actual safe protocol label supplied by the runtime client; it never stores adapter authority.
- Existing `ChatBackend` fake compatibility stays available so runner tests do not depend on HTTP adapters.

## Milestones

### M1 — Exact characterization

- Add failing tests for exact compact request bytes, omitted optional fields, endpoint suffix, conditional bearer auth, and identical retry bodies.
- Add config tests proving an absent protocol defaults to Chat Completions without a write and invalid/unsupported values fail before I/O.
- Confirm canonical tool-continuation messages and decoded event/usage order are unchanged.

Validation:
- `cargo test agent::client::tests --no-fail-fast`
- `cargo test config::tests --no-fail-fast`

### M2 — Adapter boundary

- Add `ProviderProtocol` and the Chat Completions adapter module.
- Move private wire-body construction behind the adapter and dispatch request path/auth/decoder from `AgentClient` through it.
- Pass the actual protocol label into bounded request snapshots.
- Keep all runner-facing request and event APIs stable.

Validation:
- focused provider/client/stream/request/config suites.

### M3 — Isolation and compatibility

- Add secret/debug/snapshot regressions and loopback coverage for ordinary text, reasoning, tools, usage, malformed streams, cancellation, timeouts, and retries.
- Exercise interactive/headless/review/checkpoint/subagent/provider-probe callers through existing focused tests.
- Verify missing protocol configuration makes the same path, headers, and body as before.

Validation:
- focused runner tests and loopback integration tests.

### M4 — Completion gates and handoff

- Run repository-wide tests, formatting, strict Clippy, release build, help/version/profile smoke, and a disposable loopback walkthrough.
- Reconcile harness documents, archive this plan, and activate PRV-058 only after every gate passes.

## Acceptance Criteria

- A provider entry without `protocol` uses Chat Completions and does not cause a config write.
- `protocol = "chat_completions"` is accepted; unsupported values fail before network traffic or secret resolution.
- Exact request bytes, `/chat/completions` path, content type, bearer omission/presence, and retry-body identity match the characterized contract.
- Text, reasoning, tool-call fragments, finish reason, and usage map to the same canonical events in the same order.
- Tool continuations still emit one assistant call followed by exactly one result per completed call.
- Every existing runner continues to use `PreparedModelRequest` and consume `ProviderEvent` without protocol-specific branching.
- Debug output, `/request`, tests, and snapshots contain no endpoint, header, credential, credential length, or HTTP body.
- Full repository gates and isolated walkthrough pass without touching user or oracle state.

## Validation

Focused:
- PASS: provider/client/stream/config/request suites cover exact bytes, path/auth, retry identity, tool continuation, every canonical event/usage field, safe failures, protocol parsing/defaults, edit preservation, and runtime-authority redaction.

Completion:
- PASS: `cargo test --no-fail-fast` (676 unit, 28 CLI), `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo build --release`.

Manual/visual:
- No new TUI surface; zero `.snap.new` files and the reviewed snapshot set is unchanged.
- PASS: rebuilt help/version/profile smoke and one credentialless temporary-home loopback request with omitted protocol, exact Chat Completions path, no Authorization header, successful completion, and no session/config/profile mutation.

Blocked checks:
- None currently.

## Progress

- [x] Current request, config, stream, and runner seams traced.
- [x] Active ExecPlan created before source implementation.
- [x] Exact Chat Completions wire characterization added.
- [x] Protocol enum and adapter boundary implemented.
- [x] Safe snapshot/config compatibility proven.
- [x] Focused and repository-wide gates passed.
- [x] Temporary-home walkthrough passed and cleaned recoverably.
- [x] Harness reconciled and plan archived.

## Decisions

- Decision: accept only `chat_completions` in PRV-057 rather than exposing future protocol names early.
  Evidence: PRV-058 and PRV-059 separately require complete request, stream, tool, usage, timeout, and malformed-response fixtures.
  Consequence: configuration cannot select an incomplete adapter.
- Decision: keep `PreparedModelRequest` semantic and protocol-neutral; bind protocol in `AgentClient` and record its safe label at the adapter boundary.
  Evidence: every runner already constructs the same request before runtime I/O, while endpoint/auth are client-owned.
  Consequence: adding an adapter does not duplicate orchestration or leak runtime authority into request state.
- Decision: keep `ProviderEvent` as the canonical event type.
  Evidence: all current runners already consume it and the Chat Completions adapter maps wire chunks into it.
  Consequence: protocol adapters translate at one boundary and downstream loops stay protocol-agnostic.

## Surprises and Discoveries

- `ARCHITECTURE.md` already describes the intended adapter invariant even though current implementation still constructs the body, path, auth, and decoder directly in `AgentClient`.
- The semantic JSON-shape test compares `Value`, so it cannot detect object-field byte-order drift; PRV-057 needs a compact-byte fixture before moving serialization.
- Keeping Chat Completions JSON mapping in `src/agent/stream.rs` would have left the canonical event module protocol-specific; the decoder and usage mapping moved entirely into `src/agent/provider.rs`.

## Recovery and Rollback

Revert only PRV-057-owned source and documentation hunks with `apply_patch`. Do not use destructive Git commands. Because config parsing remains backward compatible and tests use temporary roots, no runtime migration or cleanup is expected.

## Completion Record

Changed files: `src/agent/{mod,client,client_tests,provider,stream}.rs`, `src/{config,request,doctor,main,model_catalog,provider_probe}.rs`, focused provider construction tests, and repository harness documents.

Commands and results: focused suites passed; full 676-unit/28-CLI suite, formatting, strict Clippy, release, smoke, and isolated walkthrough passed.

Remaining risks: only Chat Completions is accepted until PRV-058/059 complete their own adapters; provider manager does not yet edit protocol values.

Harness updates: architecture/runtime docs describe the adapter/config contract; continuity, parity, testing, index, and next steps record completion and PRV-058 handoff.
