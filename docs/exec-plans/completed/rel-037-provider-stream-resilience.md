# REL-037 — Ordinary Provider Stream Resilience

Status: COMPLETE
Owner/session: current Codex task
Last updated: 2026-08-10

## Goal

Bound ordinary provider connect, first-event, and no-progress waits across interactive, headless, checkpoint, review, scripted, and subagent calls, with conservative pre-output retry and safe `Retry-After` handling.

## Non-Goals

- Never replay after text, reasoning, tool-call data, or another usable provider event.
- Do not change immutable `/retry` branching or provider-probe policy.
- Do not expose response bodies, URLs, headers, credentials, or retry header contents.
- Do not claim Ante/Cursor parity.

## Evidence and Current Behavior

- `AgentClient` uses an unconfigured `reqwest::Client`; request send and stream body waits are not bounded (`src/agent/client.rs`).
- `consume_stream` waits indefinitely for channel input (`src/agent/stream.rs`).
- The shared client/consumer reaches TUI, headless/script, checkpoint, review, and subagent paths.
- Ante changelog documentation describes no-output deadlines and bounded `Retry-After`; it is secondary evidence only.

## Scope

Allowed: provider client/stream policy and hermetic loopback/fake-backend tests, call-site adjustments required for typed partial-output behavior, harness docs.
Forbidden: oracle/reference edits, live providers/credentials, session format changes, retry after usable output.

## Design and Invariants

- Introduce one typed provider-call policy with connect/headers, first usable event, and no-progress deadlines plus bounded attempts/delay.
- The HTTP client owns connect timeout. Stream decoding owns event-progress deadlines so comment/empty-byte traffic cannot keep a request alive forever.
- Retry transport, 429, and retryable 5xx only before a stream receiver is returned and at most the policy attempt limit. Honor only parseable bounded `Retry-After` delta seconds; otherwise use bounded backoff.
- Once a receiver exists, no automatic replay occurs. Timeout/error preserves already-delivered partial output and existing explicit `/retry` semantics.
- Cancellation drops pending waits immediately. All displayed failures remain typed and secret-free.

## Milestones

- [x] Current shared client/consumer and call sites inspected.
- [x] Injected policy and failing connect/first-event/no-progress tests added.
- [x] Conservative pre-stream retry and bounded `Retry-After` implemented.
- [x] Partial-output/no-replay/cancellation coverage added across adapters.
- [x] Completion gates passed.

## Acceptance Criteria

- Hung connect/headers, silent stream, and stalled mid-stream terminate with typed timeout.
- Pre-stream reset/429/5xx retries are bounded; oversized/invalid `Retry-After` cannot delay beyond policy.
- Partial text or tool-call data is never replayed automatically and remains available to the existing failure path.
- TUI/headless/script/checkpoint/review/subagent behavior uses the shared policy without credentials or external network in tests.

## Validation

Focused loopback/client/stream and adapter tests, then full repository completion gates.

## Decisions

- Receiver creation is the automatic-retry boundary; later failures remain explicit `/retry` only.
- Provider probes retain their separate 20-second whole-check timeout.

## Surprises and Discoveries

- None yet.

## Recovery and Rollback

Revert only provider policy/client/stream changes; no persistence migration exists.

## Completion Record

Changed files:
Commands/results:
Remaining risks:
Harness updates:
