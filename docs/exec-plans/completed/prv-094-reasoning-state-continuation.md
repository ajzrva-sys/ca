# PRV-094 — Reasoning-State Tool Continuation

Status: COMPLETE
Owner/session: current Codex task
Last updated: 2026-08-12

## Goal

Make the existing OpenAI Responses, DeepSeek Chat Completions, and Xiaomi MiMo Chat Completions routes preserve the provider-owned reasoning state required between an assistant tool call and its tool-result continuation, with deterministic two-request fixtures proving that the second request replays exactly the state returned by the first.

## Non-Goals

- `previous_response_id`, provider-hosted conversation state, or importing another client's authentication/session cache.
- Support for every provider-specific reasoning schema in the Hermes catalog.
- Exposing opaque encrypted reasoning content in the TUI, request snapshots, logs, portable exports, checkpoint prompts, or error messages.
- Live provider traffic, real credentials, user config/Keychain/session mutation, installed-binary replacement, or oracle/reference edits.
- Replacing the existing visible reasoning display or changing the user-selected reasoning effort.

## Initial Evidence and Behavior

- Before PRV-094, `src/agent/provider.rs` translated Responses reasoning deltas into display-only `ProviderEvent::Reasoning` and discarded completed output items. Its request type had no reasoning input item and did not request `reasoning.encrypted_content`.
- Before PRV-094, `src/agent/client.rs::append_provider_messages` discarded `app::Message::thinking`; the Chat Completions request message had no `reasoning_content` field. The streaming decoder read a reasoning field only when a model profile explicitly named one, while the built-in DeepSeek and MiMo configurations did not seed such profiles.
- `app::Message` is the canonical, session-serialized history and already retains visible `thinking`. A new optional opaque provider-state lane must be backward-compatible with existing version-4 sessions and redacted from `Debug` and diagnostic snapshots.
- Official OpenAI guidance says manually managed history must preserve and resend every response output item; stateless/ZDR operation replays returned encrypted reasoning items. OpenAI's published Codex loop describes reasoning and function-call output items as the exact prefix of the next request.
- Official DeepSeek and Xiaomi MiMo guidance requires the previous assistant `reasoning_content` to be returned with tool calls before tool results are continued.

## Scope

Allowed files:
- `src/{app,context,main,request,session,session_portability}.rs` only where canonical provider state must be retained, bounded, redacted, reconstructed, or tested.
- `src/agent/{client,client_tests,provider,stream,subagent}.rs`, plus headless/service orchestration only where their tool loops must carry the same state.
- `src/test.rs` and scripted harness tests where their independent multi-turn loop must carry the same state.
- `src/provider_templates.rs` and catalog snapshots only to change OpenAI, DeepSeek, and MiMo availability after their complete two-step fixtures pass.
- Harness documents and this plan.

Forbidden files/areas:
- `oracles/`, `reference/`, user runtime configuration, real sessions, Keychain, installed binaries, and unrelated provider/auth/catalog work.

Runtime state affected:
- Canonical assistant messages may persist bounded opaque OpenAI Responses output state or DeepSeek/MiMo Chat reasoning state in normal session storage. Old sessions decode with no state. Portable text/JSON exports intentionally omit opaque state.

## Design and Invariants

- OpenAI stays stateless from `ca`'s perspective: request `reasoning.encrypted_content`, retain the bounded completed Responses output sequence (reasoning, message, and function call), validate it against canonical visible text/tool calls, and replay it in exact `output_index` order before the tool output. Do not use `previous_response_id`.
- OpenAI output state has strict item-count/aggregate-byte/depth/node/ID bounds, a redacted `Debug`, and no renderer or request-snapshot representation. Malformed, oversized, duplicate, incomplete, or conflicting output items fail closed.
- DeepSeek and MiMo retain streamed `reasoning_content` both as existing visible `thinking` text and as a route-bound continuation item. Serialization requires exact source equality with the active target, so other Chat-compatible providers and later provider/protocol/endpoint switches cannot receive foreign reasoning state.
- Source identity is derived inside the actual runtime client after route/fallback selection and contains only provider ID, protocol, and a domain-separated SHA-256 of the normalized endpoint. It never persists or formats the raw endpoint. The model ID is intentionally not part of source identity because the cited provider contracts attach continuation state to conversation history rather than a model-specific token.
- Built-in DeepSeek and MiMo routes decode `reasoning_content` without requiring users to add model-profile boilerplate. An explicit configured reasoning stream field still wins for other providers.
- Provider state stays attached to the assistant message containing the matching tool calls. Cancellation/failure paths never retain incomplete opaque state as a valid continuation.
- Direct session save/resume preserves complete state; checkpointing may discard state only together with the complete compacted prefix. Checkpoint prompts and portable exports omit opaque state.
- All first/second requests use loopback fixtures with sentinel values and prove no credential, reasoning blob, or response body reaches logs, errors, snapshots, or user-visible text.

## Milestones

### M1 — Red characterizations

- Add exact decoder/serializer tests for a Responses reasoning item and Chat `reasoning_content`.
- Add two-request tool-loop fixtures for OpenAI, DeepSeek, and MiMo that initially fail because request two lacks the returned state.
- Add old-session decode and redaction/size-bound characterizations.

### M2 — OpenAI Responses continuation

- Carry a typed bounded reasoning item through stream assembly into canonical assistant history.
- Request encrypted reasoning content and replay the item before its function call and output.
- Cover tool continuation, following user turn, malformed/oversized state, session round-trip, and diagnostic omission.

### M3 — DeepSeek and MiMo continuation

- Decode built-in `reasoning_content`, retain it as canonical thinking, and replay it on the assistant tool-call message for the exact provider route.
- Prove both providers' second requests contain the original content byte-for-byte and unrelated Chat providers do not.
- Cover interactive, headless/service, and subagent loop construction where applicable.
- Cover the scripted `.ca` harness loop as an independent continuation consumer.

### M4 — Catalog and completion

- Change only these three catalog statuses once their complete fixtures pass; update reviewed wide/narrow snapshots and exact counts.
- Run focused and repository completion gates, then a credentialless disposable released-binary loopback walkthrough.
- Reconcile harness documents and archive this plan.

## Acceptance Criteria

- OpenAI request one asks for encrypted reasoning content; request two includes the completed reasoning item before the matching function call and `function_call_output`, without `previous_response_id`.
- DeepSeek and MiMo request two each include the exact first-response `reasoning_content` on the assistant tool-call message; a generic Chat provider does not receive it.
- Missing state on a provider response that does not emit reasoning remains valid; malformed/oversized emitted state fails locally and safely.
- Normal session save/resume retains OpenAI continuation state and old sessions remain readable. Opaque state is absent from portable exports, checkpoint prompts, request snapshots, `Debug`, and UI snapshots.
- All three providers have deterministic two-step tool fixtures before the catalog calls them ready.
- No live external request, secret access, user-runtime write, oracle edit, or installed-binary mutation occurs.

## Validation

Focused:
- exact OpenAI Responses decoder/request/tool-continuation tests
- exact DeepSeek and MiMo decoder/request/tool-continuation tests
- `cargo test provider_ --no-fail-fast`
- `cargo test agent::client::tests:: --no-fail-fast` (verify a nonzero test count)
- session/context/request safety tests
- `cargo test tui::render_tests --no-fail-fast` after catalog status changes

Completion:
- `cargo test --no-fail-fast`
- `cargo fmt --check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo build --release`
- released `--help` and `--version`
- zero pending `.snap.new` files

Manual/visual:
- Review all catalog snapshot changes at 100x30 and 60x24.
- Run a released binary against a disposable loopback server that requires the exact state on request two for each priority provider.

Blocked checks:
- Live OpenAI, DeepSeek, and MiMo authentication/traffic remain excluded unless the user separately authorizes credentials and provider usage.

## Progress

- [x] Current adapter/history/session seams and official provider requirements inspected.
- [x] Red focused characterizations added.
- [x] OpenAI completed output items retained and replayed.
- [x] DeepSeek and MiMo reasoning content retained and replayed.
- [x] Catalog statuses reconciled after fixtures passed.
- [x] Focused/completion/manual validation recorded.
- [x] Harness reconciled and plan archived.

## Decisions

- Decision: use locally replayed provider state, not server-owned response IDs.
  Evidence: `ca` already rebuilds canonical local history, and official OpenAI guidance supports replaying output items for stateless/ZDR-compatible loops.
  Consequence: retries, sessions, local context control, and privacy do not depend on remote conversation retention.
- Decision: keep visible reasoning text and opaque OpenAI state separate.
  Evidence: DeepSeek/MiMo require textual `reasoning_content`; OpenAI requires an encrypted reasoning item that must never be rendered as thought text.
  Consequence: no encoded blob can leak through existing thinking UI/hooks.
- Decision: activate Chat replay only for the canonical `deepseek` and `mimo` IDs.
  Evidence: generic Chat endpoints may reject an unknown `reasoning_content` field.
  Consequence: DeepSeek/MiMo work without widening every OpenAI-compatible request; custom aliases retain generic behavior.
- Decision: bind all opaque continuation state to the actual provider, protocol, and normalized endpoint digest selected by `AgentClient`.
  Evidence: a provider ID can be edited to point at a different endpoint, and fallback selects a different concrete client after request preparation.
  Consequence: state cannot cross provider, protocol, endpoint, or fallback boundaries; credentials and raw endpoint strings are not persisted.
- Decision: retain the complete supported OpenAI output sequence rather than reasoning alone.
  Evidence: official manual-history guidance requires every response output item, and output order relative to messages and function calls is observable.
  Consequence: exact sequence replay is deterministic and validated against canonical text/tool calls before use.

## Surprises and Discoveries

- The catalog audit initially treated wire-shape compatibility as complete agent compatibility. A second pass found provider-owned state requirements that only appear after a tool call; one-shot probes cannot certify this behavior.
- Existing `Message::thinking` provides the visible text for DeepSeek/MiMo, but it cannot be the protocol lane by itself because it lacks source-provider provenance across model switching; a provider-tagged continuation item is also required.

## Recovery and Rollback

Revert only PRV-094-owned optional state fields, adapter translation, tests, snapshots, and harness hunks with `apply_patch`. Optional/defaulted session fields keep old files readable throughout. No runtime cleanup is required because validation uses disposable roots only.

## Completion Record

Changed files:

- Provider state and wire behavior: `src/agent/{stream,provider,client,client_tests,subagent,tools}.rs`, `src/{app,main,headless,service,test,test_tests,review,goal,provider_probe,approval,change_walkthrough}.rs`, plus mechanical empty-state construction in TUI tests.
- Persistence/privacy: `src/{context,request,session,session_portability}.rs`.
- Catalog/UI: `src/provider_templates.rs`, `src/tui/{render,input_tests,render_tests}.rs`, and seven provider-template catalog snapshots.
- Harness: `ARCHITECTURE.md`, `CONTINUITY.md`, `ENVIRONMENT.md`, `PARITY.md`, `TESTING.md`, `NEXT_STEPS.md`, and this plan.

Commands and results:

- `cargo test agent::provider::tests:: --no-fail-fast`: PASS, 23 tests.
- `cargo test agent::client::tests:: --no-fail-fast`: PASS, 32 tests.
- `cargo test provider_state --no-fail-fast`: PASS, 7 tests.
- `cargo test provider_ --no-fail-fast`: PASS, 134 unit tests plus 2 CLI tests.
- `cargo test tui::render_tests --no-fail-fast`: PASS, 114 tests with reviewed 100×30 and 60×24 catalog snapshots.
- `cargo test --no-fail-fast`: PASS, 1130 unit tests, one existing ignored manual-Chromium test, and 36 CLI tests.
- `cargo fmt --check`, strict Clippy, release build, released help/version, and zero-pending-snapshot inspection: PASS.
- Released disposable loopback walkthrough: PASS for exact two-request OpenAI, DeepSeek, and MiMo tool continuations; opaque sentinels were absent from stdout/stderr.

Remaining risks:

- No credentialed live-provider certification was performed.
- Automatic continuation behavior is deliberately keyed to the canonical IDs `openai`, `deepseek`, and `mimo`; custom aliases remain generic.
- Source identity intentionally excludes model ID, so same-endpoint model-switch behavior is not separately provider-certified.
- The endpoint digest is a routing identity, not authentication against a user deliberately editing their local session file.
- Portable/legacy transcript imports remain readable but are not continuation-equivalent to direct sessions because opaque provider state is intentionally omitted.
- The other 26 catalog rows still require separate auth, routing, request-control, signature/state, transport, or gateway proof.

Harness updates:

- Current-state architecture, environment, continuity, parity, testing, and queue documentation now records the route-bound state lane, the 14-ready/26-integration catalog split, exact validation, and remaining boundaries without rewriting historical CFG-093 evidence.
