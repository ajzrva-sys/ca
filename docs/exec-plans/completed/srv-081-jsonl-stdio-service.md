# SRV-081 — JSONL Stdio Service

Status: COMPLETE
Owner/session: Codex `/root`
Last updated: 2026-08-12

## Goal

Add `ca serve --stdio` as a bounded long-lived local JSONL protocol that supports an explicit process-only session, multiple user turns, streamed canonical runner events, client-resolved tool approvals and structured interactions, cancellation, status, and graceful shutdown.

## Non-Goals

- WebSocket/network listeners, attach/resume, concurrent sessions or turns, saved service sessions, runtime config mutation, shell/slash commands, goals/workflows/subagents, remote authentication, protocol compatibility with Ante, or Cursor parity.
- Automatic approval, accepting unresolved approvals/interactions as success, exposing endpoint/header/credential values, or letting a JSON operation widen the startup workspace, profile, sandbox, provider, MCP, LSP, or network authority.

## Evidence and Current Behavior

- `src/main.rs::RawCliArgs` is a flat flag/positional parser and routes piped stdin into a one-shot `run_headless`; no `serve` command or long-lived protocol exists.
- `src/headless.rs::run_with_backend` already uses `PreparedModelRequest`, central tool selection, typed provider streams, canonical tool-result continuation, and versioned serializable runner events, but an approval or interaction emits a terminal failure instead of waiting for another input.
- `src/interaction.rs` provides validated structured user-input requests and model-result encoding. `src/approval.rs` provides the shared prepared-invocation/policy decision boundary.
- `src/agent/tools.rs` can execute a prepared invocation with an exact `JobOwner`; `JobManager` can enumerate and terminate only that owner’s processes.
- Ante documents a daemon-style JSONL stdin/stdout bridge, explicit session start/input/approval/shutdown, and streamed events in `oracles/ante-main/docs-site/docs/usage/serve.mdx` and `docs/reference/protocol-reference.mdx`. This is descriptive product evidence only; `ca.serve.v1` is an independently specified `ca` extension.

## Scope

Allowed files: a new `src/service.rs`; narrow CLI/runtime/request-kind additions in `src/{main,request}.rs`; focused tests; this plan and required harness documents.

Forbidden files/areas: oracle/reference trees; TUI/session/config/provider wire schemas; real user config/sessions/Keychain; installed binary; WebSocket/listener dependencies; accessibility.

Runtime state affected: process-only service/session/turn state and owned jobs. Production reads the selected profile and workspace, and provider traffic occurs only after an explicit valid `user_input`. Tests and walkthroughs use disposable homes, workspaces, fake backends, and loopback HTTP only.

## Design and Invariants

### CLI and transport

- Reserve the leading token `serve`; `ca serve` and `ca serve --stdio` select stdio, while the existing flat parser remains byte-for-byte responsible for every non-service invocation and `ca -p serve` remains a normal prompt.
- Stdio accepts one deny-unknown-fields `ca.serve.v1` operation envelope per bounded line and emits one flushed `ca.serve.v1` event envelope per line with one service ID, monotonic sequence, unique event ID, and optional parent operation ID. Stdout contains protocol JSON only.
- Empty, malformed, oversized, duplicate-ID, invalid-ID, invalid-state, and unsupported operations produce bounded typed error events without terminating the daemon. EOF is graceful shutdown; broken stdout is successful client disconnect.

### Session and turn ownership

- One active process-only session owns an exact provider/model, startup-bounded workspace, system prompt, skills/MCP/LSP capability snapshot, canonical provider messages, central tool policy, sandbox/network settings, and unique job owner. It never reads/writes `SessionStore`.
- `start_session` defaults to configured provider/model and may select only an existing configured provider/model. An optional cwd must resolve inside the startup workspace. Replacing an idle session shuts down only its extension clients/jobs and emits an explicit replacement boundary; replacing a busy session is rejected until cancel.
- Only one turn runs at a time. `user_input` appends one bounded text message, then a child task emits headless-compatible output/reasoning/tool/usage/completion/failure events and returns the updated canonical message list. A second input while busy is rejected and never queued.
- `cancel` aborts only the active turn, drops provider I/O, kills that session owner’s jobs, preserves completed canonical history plus the submitted user message, clears pending manual input, and emits an interrupted turn end. `shutdown` performs the same cleanup before session-end/goodbye.

### Approval and interaction

- The central resolver operates over a service-capable registry: ordinary built-ins, explicit `request_user_input`, activated MCP tools, LSP, and skills remain eligible; TUI/work-state/subagent-only orchestration tools are unknown before provider traffic.
- Persistent Deny remains automatic denial. Existing exact Allow may execute non-network work. Prompt/Ask decisions and every permitted network request emit one safe approval request and block that tool until the client responds allow-once, deny, or abort. A response must match the exact live request ID.
- `request_user_input` emits the validated question, description, mode, choices, and allow-other flag and waits for a matching structured response. The service validates selected values/text locally and sends only the normalized model result back into the tool loop. Secrets are unsupported and never cross the wire.
- No later tool/provider continuation starts while an approval or interaction is unresolved. Stale, duplicate, or wrong-kind responses are errors and cannot resume work.

### Security and compatibility

- Operations and events contain safe IDs, provider/model identifiers, cwd, tool metadata/results, usage, and typed failures only. Runtime provider config, endpoint, credential, headers, MCP auth, and request bodies remain inside existing runtime clients.
- Provider calls use `PreparedModelRequest` with a dedicated service kind. Tool execution retains ordinary registry validation, prepared workspace resources, permission policy, required sandbox, network authority, and owner-scoped job control.
- Ordinary headless v1/v2 output and all existing CLI positional parsing remain unchanged outside the reserved leading `serve` command.

## Milestones

### M1 — Protocol and CLI characterization

- Add failing tests for service routing, positional compatibility, strict bounded envelopes, monotonic output, invalid operations, status, EOF, and shutdown.
- Add the active ExecPlan and `NEXT_STEPS.md` claim before source behavior changes.

### M2 — Session runner and manual resolution

- Implement the production/fake session factory, prepared-request turn loop, central service registry, canonical continuations, explicit approval/interaction waits, cancellation, and owner cleanup.
- Cover multi-turn history, unknown-tool zero traffic, allow/deny/abort, interaction answers, provider failures, cancellation/stale responses, secret-free Debug/output, MCP/LSP shutdown, and backpressure.

### M3 — Gates and disposable walkthrough

- Run focused/full gates and release smokes. Drive the released service with a disposable home/workspace and loopback provider through start, status, two turns, one approval, one interaction, cancellation, and shutdown; inspect all JSONL and filesystem effects.

## Acceptance Criteria

- A client can start one session, submit multiple sequential inputs, consume parseable ordered streaming events, answer exact approval and interaction requests, inspect status, cancel, and shut down without TTY or persistence.
- Invalid input cannot panic, desynchronize framing, start provider traffic, widen authority, or leak runtime secrets. All bounds, IDs, state transitions, and stale-response rules are deterministic.
- Every provider request is `PreparedModelRequest::Service`; selected tools come through the central policy and ordinary execution/approval/sandbox boundaries; canonical tool calls receive exactly one result before continuation.
- Existing headless contracts and non-service CLI parsing/tests remain unchanged. No real runtime state or oracle file is modified.

## Validation

Focused: PASS. `cargo test service --no-fail-fast` ran 16 matching protocol/CLI/runner tests; the focused released-protocol CLI integration path also passed.

Completion: PASS. `cargo test --no-fail-fast` ran 993 unit tests plus 29 CLI integration tests; `cargo fmt --check`, strict Clippy, and `cargo build --release` passed. Release help, version, service-help, named-profile doctor, and bare-profile doctor smokes passed without creating missing profile files.

Manual/visual: PASS. The released daemon emitted 33 ordered JSON events over five credentialless loopback requests, blocked a file mutation until exact approval, validated and resumed one structured interaction, cancelled a hanging request back to idle, shut down with empty stderr, and created no session files. The inspected fixtures moved recoverably to Trash.

Blocked checks: none known.

## Progress

- [x] Existing CLI/headless/request/approval/interaction/job seams traced.
- [x] Failing protocol/CLI/runner characterization added.
- [x] Stdio service and production session factory implemented.
- [x] Focused/full gates and released loopback walkthrough pass.
- [x] Harness reconciled and plan archived.

## Decisions

- Decision: one active process-only session, not a persisted or concurrent session registry.
  Evidence: SRV-081 asks for explicit session lifecycle and multiple interactions; persistence, attach, and network concurrency have separate later boundaries.
  Consequence: replacement/cancel/shutdown semantics remain exact and auditable, and no service operation touches saved sessions.
- Decision: nest the existing typed headless runner events inside service turn events instead of inventing a second output/tool/usage vocabulary.
  Evidence: the existing events already cover streamed text/reasoning, tools, usage, completion, and safe failures.
  Consequence: clients get one versioned service lifecycle around reusable runner payloads without changing `ca.headless.v1`.
- Decision: all non-persistently-allowed tool and network work waits for a client response.
  Evidence: a daemon has no terminal approval surface, and implicit yolo would widen authority silently.
  Consequence: an unattended client must configure exact persistent rules or implement the approval protocol.

## Surprises and Discoveries

- Headless currently advertises several interaction/work-state tools it cannot execute and then exits on their use. The service must resolve against a capability-filtered registry before traffic so unsupported orchestration is never advertised.
- A runner failure event and its outer turn-terminal event must remain distinct: clients receive the safe detailed `runner.failure`, then use `turn_failed` as the state transition before submitting another operation.

## Recovery and Rollback

Remove the service module, dedicated request kind, and leading-token service parser with local patches. Existing flat CLI/headless/TUI code paths and persisted formats remain unchanged, so no migration or runtime cleanup is required.

## Completion Record

Changed files: `src/{main,request,service}.rs`, `tests/cli.rs`, `docs/SERVICE_PROTOCOL.md`, this ExecPlan, and the required harness documents.

Commands and results: focused service tests PASS; full test PASS (993 unit plus 29 CLI); format PASS; strict Clippy PASS; release build PASS; help/version/profile/service smokes PASS; released disposable walkthrough PASS.

Remaining risks: v1 intentionally supports one process-only session and one turn, has no network listener or attach/reconnect contract, and inherits the existing startup behavior when a configured Keychain reference cannot resolve. Those boundaries remain explicit follow-up work rather than silent degradation.

Harness updates: `docs/.INDEX.md`, `ARCHITECTURE.md`, `CONTINUITY.md`, `TESTING.md`, `NEXT_STEPS.md`, and `PARITY.md` now describe the released contract, validation evidence, and extension-only parity status.
