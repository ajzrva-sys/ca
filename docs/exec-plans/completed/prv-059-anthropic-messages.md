# PRV-059 — Anthropic Messages

Status: COMPLETE
Owner/session: current Codex task
Last updated: 2026-08-11

## Goal

Add an opt-in `anthropic_messages` provider adapter with complete text, reasoning, client-tool call/result, usage, authentication, cancellation, timeout, and malformed-stream behavior while preserving the default Chat Completions and opt-in OpenAI Responses contracts exactly.

## Non-Goals

- Bedrock, Vertex, Microsoft Foundry, Anthropic OAuth/subscription auth, prompt caching policy, batches, hosted/server tools, server fallback, files, citations, PDF/image input, or runtime protocol switching.
- Persisted thinking signatures or replay of thinking blocks. Canonical local sessions continue to replay text and client-tool history only; MM-067 owns typed multimodal content.
- New provider-manager protocol controls or a configurable output-token limit. This slice uses one documented bounded adapter constant and records the later configuration seam.

## Evidence and Current Behavior

- PRV-057 owns the protocol-neutral boundary; PRV-058 proves a second adapter can preserve every runner's canonical request/event contract.
- Official Anthropic documentation retrieved 2026-08-11 states:
  - Messages uses `POST /v1/messages`, `x-api-key`, `anthropic-version: 2023-06-01`, `max_tokens`, top-level `system`, user/assistant messages, and flat tools: `https://platform.claude.com/docs/en/api/messages`.
  - tool calls are assistant `tool_use` blocks and results are leading `tool_result` blocks in the immediately following user message: `https://platform.claude.com/docs/en/agents-and-tools/tool-use/implement-tool-use`.
  - streaming lifecycle is `message_start`, content-block start/delta/stop, cumulative `message_delta`, then `message_stop`; deltas include text, partial tool-input JSON, and thinking, while ping and future unknown event types must be tolerated: `https://platform.claude.com/docs/en/build-with-claude/streaming`.
  - current effort is `output_config.effort`; supported shared values are low, medium, high, and model-dependent max: `https://platform.claude.com/docs/en/build-with-claude/effort`.
- No live Anthropic traffic is authorized or required; loopback fixtures are the acceptance authority.

## Scope

Allowed files:
- `src/agent/{provider,client,stream}.rs` and focused tests.
- `src/{config,provider_probe,model_catalog}.rs` only for protocol/error/auth plumbing.
- Loopback/unit/CLI tests outside oracle/reference trees.
- Harness and this ExecPlan.

Forbidden files/areas:
- `oracles/`, `reference/`, real user configuration, Keychain data, sessions, installed binary, and PRV-060 onward.

Runtime state affected:
- None during automated validation. Manual validation uses a temporary home and credentialless loopback server only.

## Design and Invariants

- Add `ProviderProtocol::AnthropicMessages` serialized exactly as `anthropic_messages`; absent protocol and both existing explicit spellings retain frozen behavior.
- The adapter owns `/messages`, `anthropic-version: 2023-06-01`, conditional `x-api-key`, exact request bytes, and a fixed bounded `max_tokens` value. It never sends bearer auth for this protocol.
- Move leading system/developer text into ordered top-level system text blocks. Reject interleaved system messages, unknown roles, and non-text content before I/O.
- Map user/assistant text to Anthropic content blocks. Map assistant function calls to `tool_use` after parsing their arguments as JSON objects. Coalesce each consecutive canonical tool-result run into one following user message whose `tool_result` blocks preserve call order. No executable semantics or provider text is invented.
- Flatten current tool definitions to `{name,description,input_schema}`. Map supported reasoning effort to `output_config.effort`; map `minimal` conservatively to `low` because Anthropic has no minimal value.
- Decode text and thinking deltas to canonical events; decode `tool_use` plus partial input JSON to canonical tool fragments. Ignore signatures, ping, and unknown future top-level events as required by Anthropic's versioning policy, but fail closed on malformed client-tool identity/fragments.
- Accumulate input/cache usage from `message_start` and cumulative output usage from `message_delta`. Canonical prompt tokens include ordinary, cache-creation, and cache-read input; cached prompt tokens report cache-read input; total is recomputed safely.
- Translate `end_turn`/`stop_sequence` to stop, `tool_use` to tool calls, max-token/context stops to length, and refusals/errors to typed body-free failures. Emit `Done` only at one valid `message_stop`; clean EOF earlier remains premature.
- Shared HTTP retry/deadline/cancellation behavior stays in `AgentClient`. Request/debug/snapshot/error state never retains endpoint, headers, keys, message IDs, thinking signatures, provider payloads, or response bodies.

## Milestones

### M1 — Exact request and auth fixtures

- Add failing config and exact-byte fixtures for system/text, tools, assistant tool replay, coalesced tool results, effort, and invalid content/arguments.
- Add loopback path/header/credentialless tests while freezing both existing adapters.

### M2 — Stream translation

- Add byte-split lifecycle, text/thinking, multi-tool, usage/cache, stop-reason, ping/unknown-event, error/refusal, malformed/oversized/conflict, premature EOF, and duplicate-stop fixtures.

### M3 — Shared-runner compatibility

- Cover provider setup, tool continuation, usage, timeout, cancellation, safe HTTP/stream failures, and catalog auth where the existing catalog endpoint supports this protocol.

### M4 — Delivery gates

- Run focused/full tests, format, strict Clippy, release, help/version/profile smoke, and a disposable loopback walkthrough.
- Reconcile harness, archive this plan, and activate AUTH-060 only after every gate passes.

## Acceptance Criteria

- `protocol = "anthropic_messages"` uses `/messages`, the version header, conditional `x-api-key`, and exact documented request shapes.
- Text/thinking/tool/usage fixtures assemble into the same canonical turns consumed by every runner.
- Tool results immediately follow and match their exact `tool_use` IDs; invalid or non-object input fails before traffic.
- Unknown future event types do not crash a valid stream, while malformed known client-tool events fail closed.
- Default Chat Completions and OpenAI Responses bytes/tests remain unchanged.
- Full gates and isolated walkthrough pass without real state.

## Validation

Focused: PASS. Anthropic-filtered adapter/client/probe/catalog/config tests cover exact request/auth bytes, text/thinking/tool/usage lifecycle, multiple tool blocks, malformed and premature streams, safe terminal failures, setup probes, model catalogs, timeout, and receiver-drop cancellation. The default Chat Completions and OpenAI Responses characterizations remain green.

Completion: PASS. `cargo test --no-fail-fast` passed 697 unit tests plus 28 CLI integration tests; `cargo fmt --check`, zero pending `.snap.new` files, `cargo clippy --all-targets -- -D warnings`, `cargo build --release`, and rebuilt help/version/lazy-profile smoke all passed.

Manual/visual: No new TUI state was added. A disposable credentialless loopback walkthrough used the rebuilt binary, exact `POST /v1/messages`, `anthropic-version: 2023-06-01`, no `x-api-key` or Authorization header, fixed `max_tokens = 16384`, reasoning plus `OK`, and successful usage completion. Captured body SHA-256: `cc0d9ce9de2d0b7569627f75d659a4f542daf7f9f239a425272971ebac6f40c3`. The root was moved recoverably to `/Users/aji/.Trash/ca-prv059-walk.GgUqhk`.

Blocked checks: Live Anthropic traffic is intentionally excluded; current official docs plus loopback fixtures define the hermetic contract.

## Progress

- [x] Current official Anthropic request/tool/stream/effort documentation reviewed.
- [x] Active ExecPlan created before source implementation.
- [x] Request/config/auth fixtures and adapter added.
- [x] Stream/usage/tool decoder complete.
- [x] Shared runner and provider setup/catalog behavior proven.
- [x] Repository gates and walkthrough passed.
- [x] Harness reconciled and plan archived.

## Decisions

- Decision: configuration spelling is `anthropic_messages` and direct API authentication is `x-api-key` plus the stable version header.
  Evidence: these name the official endpoint family and required direct-API headers without overloading bearer-compatible gateways.
  Consequence: gateway-specific auth belongs to a future explicit protocol/capability rather than silent dual-header leakage.
- Decision: use manual local-history replay and do not persist thinking signatures.
  Evidence: current canonical sessions contain text and function semantics, while Anthropic requires signatures for thinking-block replay.
  Consequence: continued requests remain valid text/tool conversations but do not claim thinking-cache continuity.
- Decision: coalesce consecutive tool results into one user message.
  Evidence: Anthropic requires tool results directly after the corresponding assistant tool-use blocks and before any following user text.
  Consequence: wrapper shape changes per protocol while exact IDs, result strings, and order remain stable.

## Surprises and Discoveries

- Anthropic's documented thinking example can omit `message_start.usage`; the decoder therefore accepts absent initial usage while still requiring a valid terminal lifecycle.
- `cache_creation_input_tokens` and `cache_read_input_tokens` are provider input subsets. Canonical prompt usage includes both, while cached prompt usage reports only cache reads.
- Anthropic's versioning policy requires ignoring unknown future top-level event types. Known content-delta families remain block-type checked, while signatures and citation deltas are deliberately discarded.
- A pre-existing config compatibility bug remains out of scope: a raw file with no `[agent]` table receives derived zero values rather than the field-level defaults. The walkthrough included the normal generated agent table and `CONTINUITY.md` records the separate follow-up.

## Recovery and Rollback

Use `apply_patch` to revert only PRV-059-owned hunks. Keep the completed Chat Completions and Responses adapters untouched. No migration or runtime cleanup is expected because the new protocol is opt-in and tests use temporary roots.

## Completion Record

Changed files: `src/agent/provider.rs`, `src/agent/client_tests.rs`, `src/provider_probe.rs`, `src/model_catalog.rs`, `src/config.rs`, this plan, and the repository harness documents.

Commands and results: Anthropic-focused unit/loopback suites PASS; `cargo test --no-fail-fast` PASS (697 + 28); format PASS; snapshot hygiene PASS; strict Clippy PASS; release PASS; help/version/lazy-profile smoke PASS; credentialless released-binary walkthrough PASS.

Remaining risks: live Anthropic traffic was intentionally excluded; fixed output tokens are not yet configurable; persisted thinking signatures/prompt-cache continuity, cloud-host variants, hosted tools, and multimodal content remain later explicit work.

Harness updates: `ARCHITECTURE.md`, `CONTINUITY.md`, `ENVIRONMENT.md`, `TESTING.md`, `NEXT_STEPS.md`, `PARITY.md`, and `docs/.INDEX.md` describe the new adapter, evidence, boundaries, and next handoff.
