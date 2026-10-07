# PRV-058 — OpenAI Responses

Status: COMPLETE
Owner/session: current Codex task
Last updated: 2026-08-11

## Goal

Add a complete `openai_responses` provider adapter for text, reasoning, function calls/results, usage, cancellation, timeout, and malformed streams while leaving omitted and explicit `chat_completions` providers byte-for-byte unchanged.

## Non-Goals

- Anthropic Messages (PRV-059), OpenAI OAuth/ChatGPT subscription auth (AUTH-060), fallback (FAL-061), hosted OpenAI tools, background Responses, server-managed conversations, or WebSocket mode.
- Typed images/audio; MM-067 owns multimodal content. Non-text Chat Completions content fails locally for this adapter.
- Persisted/encrypted reasoning-item replay or `previous_response_id`. `ca` continues to manage canonical history locally and replays documented message/function items.
- Runtime protocol switching or provider-manager protocol editing. Protocol is selected in TOML at startup.

## Evidence and Current Behavior

- PRV-057 introduced `ProviderProtocol`, `PreparedModelRequest`, canonical `ProviderEvent`, and adapter ownership in `src/agent/provider.rs`; only `chat_completions` is accepted today.
- Every runner consumes the same canonical event/turn assembly and therefore should need no Responses-specific orchestration.
- Current official OpenAI documentation (retrieved 2026-08-11 from official domains) states:
  - Responses uses typed `input`/`output` Items and `/v1/responses`: `https://developers.openai.com/api/docs/guides/migrate-to-responses`.
  - Function tools are flat `{type,name,description,parameters}` definitions; function outputs use `{type:"function_call_output",call_id,output}` and should replay response output items: `https://developers.openai.com/api/docs/guides/function-calling`.
  - common streaming events include `response.created`, `response.output_text.delta`, `response.completed`, and `error`: `https://developers.openai.com/api/docs/guides/streaming-responses`.
  - the official streaming reference documents `response.output_item.added`, `response.function_call_arguments.delta/done`, reasoning deltas, and nested completed-response usage: `https://platform.openai.com/docs/api-reference/responses-streaming/response/refusal/delta`.
- The OpenAI developer-docs MCP was unavailable. Installing it would mutate user Codex configuration, so the binding no-user-state rule required the official-domain web fallback.

## Scope

Allowed files:
- `src/agent/{provider,client,stream}.rs` and focused tests.
- `src/{config,request,provider_probe,headless,main}.rs` only for defaulted protocol/error plumbing.
- Loopback/unit/CLI tests outside oracle/reference trees.
- Harness and this ExecPlan.

Forbidden files/areas:
- `oracles/`, `reference/`, real user configuration, Keychain data, sessions, installed binary, and OpenAI developer-docs installation.
- PRV-059 onward.

Runtime state affected:
- None during automated validation. Manual validation uses a temporary home and loopback server only.

## Design and Invariants

- Add `ProviderProtocol::OpenAiResponses` serialized exactly as `openai_responses`; existing absent/explicit Chat Completions behavior and bytes remain frozen.
- The Responses adapter owns `/responses`, bearer authentication, exact request serialization, content conversion, SSE event decoding, usage mapping, and safe terminal failures.
- Map string `system`, `developer`, `user`, and assistant messages to text input Items. Convert assistant tool calls to `function_call` Items and tool results to `function_call_output` Items preserving exact `call_id`, name, arguments, output, and order.
- Flatten existing tool definitions and explicitly use non-strict function schemas because current registry schemas are not normalized to OpenAI strict-mode requirements.
- Map configured reasoning effort to Responses `reasoning.effort`. Map reasoning-text and reasoning-summary deltas to canonical `Reasoning`; output-text deltas to `Text`.
- `response.output_item.added/done` plus function-argument deltas map to one canonical tool start and lossless argument fragments. Full done values may supply a missing suffix but never duplicate or conflict with emitted fragments.
- `response.completed` emits exactly one `Finish` (`tool_calls` if any function item occurred, otherwise `stop`) with mapped input/output/total, cached-input, and reasoning-output tokens, then `Done`.
- `error`, `response.failed`, `response.incomplete`, refusal, unknown output-item types, malformed JSON/SSE, duplicate completion, and inconsistent deltas fail with typed secret/body-free errors. Error messages from provider payloads are discarded.
- Responses has no `[DONE]` sentinel requirement; stream EOF before one successful `response.completed` remains `PrematureEof` through canonical assembly.
- Dropping the receiver cancels the decode body task through the existing send-failure path; headers/first-event/no-progress deadlines and pre-output retries remain shared.
- Request/snapshot/debug state contains the protocol label only, never endpoint, headers, credential, provider error body, or response ID.

## Milestones

### M1 — Request fixtures and config

- Add failing exact fixtures for text, tools, reasoning, assistant function-call replay, and function outputs.
- Add default/explicit config tests and frozen Chat Completions regression.
- Reject non-text content before HTTP I/O.

### M2 — Streaming decoder

- Add byte-split fixtures for text, reasoning, tool fragments, usage, completed/failed/incomplete/error/refusal, malformed JSON, oversized events, inconsistent done values, premature EOF, and multiple output indexes.
- Translate only documented event/item types.

### M3 — Loopback and runner compatibility

- Cover path/auth/body, tool continuation round trip, usage, response-body/error redaction, timeout, cancellation, and retry boundaries.
- Run every existing runner characterization with both the unchanged fake boundary and Responses loopback where orchestration is shared.

### M4 — Delivery gates

- Run focused and full tests, formatting, strict Clippy, release, help/version/profile smoke, and disposable temporary-home walkthrough.
- Reconcile harness, archive this plan, and activate PRV-059 only after all gates pass.

## Acceptance Criteria

- `protocol = "openai_responses"` selects `/responses`; missing protocol remains exact Chat Completions.
- Text/reasoning/tool/usage fixtures map losslessly to canonical events and complete turns.
- Assistant function calls and exact tool outputs replay as documented typed Items with stable `call_id`.
- Provider payload messages, endpoints, headers, credentials, response IDs, and bodies never enter state/debug/snapshots/output.
- HTTP/status/transport deadlines/retries/cancellation retain PRV-057 semantics.
- Unsupported content/protocol events fail before unsafe or invented translation.
- Full gates and isolated walkthrough pass without real state.

## Validation

Focused: PASS. Eight adapter/decoder units, four Responses client loopback tests, the provider-probe Responses path, protocol config/load/edit preservation, and the unchanged 25-test client suite passed. One pre-existing wall-clock retry assertion failed only while four Cargo processes contended for the build lock and passed immediately in isolation; the serial full suite passed it again.

Completion: PASS. `cargo test --no-fail-fast` passed 687 unit and 28 CLI tests; `cargo fmt --check`, strict Clippy, release build, help/version/profile smoke, and zero pending snapshots all passed.

Manual/visual: PASS. A disposable-home, credentialless loopback run used `POST /v1/responses`, omitted Authorization, streamed reasoning plus `OK`, mapped usage, and created no session/profile file. No new TUI state was introduced.

Blocked checks: Live OpenAI traffic is intentionally excluded; official docs plus loopback fixtures define the hermetic contract.

## Progress

- [x] Official current request/tool/stream documentation reviewed through allowed official-domain fallback.
- [x] Active ExecPlan created before source implementation.
- [x] Request/config fixtures and adapter added.
- [x] Streaming/usage/tool decoder complete.
- [x] Loopback and runner compatibility proven.
- [x] Repository gates and walkthrough passed.
- [x] Harness reconciled and plan archived.

## Decisions

- Decision: configuration spelling is `openai_responses`.
  Evidence: it is unambiguous beside `chat_completions` and avoids exposing an incomplete generic `responses` family.
  Consequence: unsupported names continue to fail at config decode before secrets or I/O.
- Decision: manually replay typed message/function items without server-managed response IDs.
  Evidence: `ca` sessions own canonical history and every runner reconstructs full local context.
  Consequence: response IDs and server conversation authority never enter state; encrypted reasoning replay remains explicitly out of scope.
- Decision: reject non-string message content.
  Evidence: existing array content uses Chat Completions-specific image blocks, while MM-067 separately owns typed multimodal translation.
  Consequence: no accidental cross-protocol image schema is invented.

## Surprises and Discoveries

- Official tool definitions are flat in Responses rather than nested under `function`, but tool outputs remain linked by the model-issued `call_id`.
- Responses terminates through typed lifecycle events, not Chat Completions' `[DONE]` sentinel.
- A raw partial TOML fixture that omits the entire `[agent]` table receives `AgentConfig::default()` from a derived zero-valued implementation, so its headless `max_turns` is zero. Production-created configs include the table; this pre-existing nested-default defect is recorded in continuity rather than expanded into PRV-058.

## Recovery and Rollback

Use `apply_patch` to revert only PRV-058-owned hunks. Keep the completed PRV-057 adapter untouched. No config migration or runtime cleanup is expected because tests use temporary roots and the new protocol is opt-in.

## Completion Record

Changed files: `src/agent/provider.rs`, `src/agent/stream.rs`, `src/agent/client_tests.rs`, `src/provider_probe.rs`, `src/config.rs`, the harness, and this plan.

Commands and results: focused adapter/client/probe/config suites passed; `cargo test --no-fail-fast` passed 687 + 28; formatting, strict Clippy, release, help/version/profile smoke, and isolated walkthrough passed. Walkthrough request SHA-256 was `085889359bfab7fc91047ee7046edeeb280ac6bdbd92b6784148773ab8589cc2`; its recoverable root is `/Users/aji/.Trash/ca-prv058-walk.OoQ8wS`.

Remaining risks: Live OpenAI traffic remains intentionally untested. The adapter implements the official documented manual-history streaming surface; hosted tools, server conversation IDs, encrypted reasoning replay, and multimodal content remain excluded for their later plans.

Harness updates: architecture, environment, continuity, parity, testing, next-steps, and knowledge-index descriptions now identify the opt-in Responses adapter and unchanged Chat Completions default.
