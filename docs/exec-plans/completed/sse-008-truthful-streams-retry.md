# SSE-008 — Truthful Provider Streams and Retry

Status: COMPLETE
Owner/session: Codex `/root`
Last updated: 2026-08-09

## Goal

Decode OpenAI-compatible SSE incrementally without data loss, classify provider/protocol failures truthfully across every runner, preserve partial assistant text on failure, and provide a non-destructive branch-based `/retry`.

## Non-Goals

- Automatic retries or backoff.
- Provider-specific reasoning fields (`MOD-019`).
- Headless output envelopes (`HED-017`).
- Cursor parity claims.

## Evidence and Current Behavior

- `src/agent/client.rs::chat_stream` converts each byte chunk with `from_utf8_lossy`, parses per line, silently ignores malformed JSON, and sends transport errors as `ChatDelta::Finish`.
- `src/main.rs::start_llm_turn` emits `StreamMsg::Finish` whenever the channel closes, regardless of a finish reason.
- TUI error handling discards `state.streaming`, losing partial assistant text; headless/test/subagent loops accept channel EOF as a completed turn.
- Existing session branching can save a source and create an immutable child, providing the correct `/retry` primitive.

## Scope

Allowed files: `src/agent/client.rs`, its focused tests, `src/agent/subagent.rs`, `src/test.rs`, `src/main.rs`, `src/app.rs`, `src/tui/commands.rs`, focused input/render/session tests if required, and harness docs.

Forbidden: oracle/reference trees, credentials, user sessions/config, installed binary, running TUI, automatic network tests.

Runtime state affected: automated tests use fake streams and temporary session roots only.

## Design and Invariants

- `SseDecoder` buffers bytes, finds LF delimiters, strips one CR, ignores comment/unknown fields, joins repeated `data:` lines with LF, and rejects events over 256 KiB.
- UTF-8 is decoded only after a complete event is framed; invalid event UTF-8 is a typed error.
- JSON decoding yields typed `ProviderEvent`; channel items are `Result<ProviderEvent, ProviderError>` so transport/protocol failures cannot masquerade as completion.
- `AssistantTurnAssembler` is used by TUI, headless/script tests, checkpoints, and subagents. It requires a valid finish reason plus `[DONE]`/clean EOF, validates tool identities/JSON arguments, and retains partial text separately from its result.
- Only `stop` and `tool_calls` are complete terminal reasons. `length`, `content_filter`, missing finish, malformed events, transport errors, and inconsistent tool state fail.
- `/retry` first saves the source, branches before the failed/last exchange, restores the original user request, and starts one new turn. Source files are never rewritten after the branch is created.
- Failed turns never drain queued input and never auto-retry.

## Milestones

### M1 — Decoder and assembler characterization
- Add byte-by-byte, CRLF, comment, multiline, usage-only, done, cap, malformed JSON/UTF-8, premature EOF, finish-reason, and tool-integrity tests.

### M2 — Shared stream contract
- Introduce typed events/errors/reasons and migrate AgentClient, TUI, headless/script runner, checkpoint generator, and subagents to the assembler.
- Preserve partial assistant text without unresolved tool metadata on failure.

### M3 — Branch-safe retry
- Register `/retry`, record the retryable user boundary, save/branch immutably, and resubmit exactly once.
- Add temporary-store tests proving source immutability and queue no-drain.

### M4 — Validation
- Run focused/full tests, format, strict Clippy, release, help/version, and relevant snapshots; reconcile harness docs.

## Acceptance Criteria

- All specified framing cases are deterministic under one-byte chunking.
- No error or EOF path becomes successful completion.
- All four stream consumers use `AssistantTurnAssembler`.
- TUI failures retain partial assistant content but no unresolved tool-call metadata and do not drain the queue.
- `/retry` creates a child session and leaves the source session byte-for-byte unchanged after branching.
- No automatic retry occurs.

## Validation

Focused: PASS — 6 decoder/assembler tests, 2 failure/retry orchestration tests, registry coverage, and 2 snapshots.

Completion: 175 unit + 3 CLI tests, format, release, help/version PASS; strict Clippy reports only the 3 pre-existing errors.

Manual/visual: reviewed wide/narrow failure snapshots; the narrow view keeps `/retry` visible before truncating provider detail. Live provider/TUI not touched.

Blocked checks: none.

## Progress

- [x] Inspected stream consumers and session-branch primitives; claimed SSE-008.
- [x] Added red decoder/assembler tests.
- [x] Migrated all stream consumers.
- [x] Implemented branch-safe `/retry`.
- [x] Completed validation and harness updates.

## Decisions

- Decision: channel errors are typed values, not synthetic finish reasons.
  Evidence: current transport failures are indistinguishable from successful channel completion.
  Consequence: every consumer must explicitly handle provider errors and validate EOF through the shared assembler.

## Surprises and Discoveries

- The existing status line truncates at narrow widths, so the retry action is placed before provider error detail to remain visible.

## Recovery and Rollback

Inverse-patch only listed source/docs files. Tests create no user runtime state.

## Completion Record

Changed files: `src/agent/stream.rs`, `src/agent/client.rs`, `src/agent/mod.rs`, `src/agent/subagent.rs`, `src/test.rs`, `src/test_tests.rs`, `src/main.rs`, `src/app.rs`, `src/tui/commands.rs`, `src/tui/render_tests.rs`, 2 snapshots, and harness docs.

Commands and results: focused/full tests, format, release, help/version, and snapshot-artifact check PASS; strict Clippy retains the known 3 unrelated findings.

Remaining risks: provider-specific nonstandard finish reasons fail closed and may need an explicit adapter; headless exit-code mapping remains `HED-017`.

Harness updates: `ARCHITECTURE.md`, `TESTING.md`, `CONTINUITY.md`, and `NEXT_STEPS.md` reconciled.
