# SRV-082 — Loopback WebSocket and Attach

Status: COMPLETE
Owner/session: Codex `/root`
Last updated: 2026-08-12

## Goal

Extend the released `ca.serve.v1` lifecycle with an authenticated literal-loopback WebSocket transport and a bounded `ca attach` JSONL bridge, while preserving the stdio contract, prepared-request/tool-policy boundaries, process-only sessions, and deterministic disconnect cleanup.

## Non-Goals

- Non-loopback listeners, TLS termination, public-network discovery, browser UI, session persistence/reconnection, multiplexing multiple users into one service session, command-line token values, automatic token creation/storage, SSH orchestration, or protocol changes to `ca.serve.v1`.
- Treating Ante documentation as runtime evidence or claiming Cursor parity.

## Evidence and Current Behavior

- `src/service.rs` owns one strict bounded `ca.serve.v1` reader/writer loop over generic async byte streams, but `run_production_stdio` wires only stdin/stdout and labels Ready as `stdio`.
- `src/main.rs::parse_launch_args` reserves only a leading `serve`; there is no `attach` command, listener/address parser, or authentication-source parser.
- `Cargo.toml` contains async HTTP clients but no WebSocket client/server dependency.
- Ante documents stdio and WebSocket transports plus clients over a long-lived service. This is descriptive evidence only; SRV-082 retains the independent `ca.serve.v1` contract and supports only literal loopback.

## Scope

Allowed files: `Cargo.toml`/`Cargo.lock`; narrow changes in `src/{main,service}.rs`; focused CLI/protocol tests; `docs/SERVICE_PROTOCOL.md`; this plan and required harness documents.

Forbidden files/areas: oracle/reference trees; provider/session/config wire schemas; real user configuration, sessions, Keychain, token files, or installed binary; non-loopback listener/TLS/SSH implementation; accessibility.

Runtime state affected: an explicitly requested loopback TCP listener, one bounded authenticated WebSocket client at a time, process-only service state/jobs/extensions, and caller-provided token file/environment reads. Tests and walkthroughs use temporary homes, owner-only token files, loopback providers, and ephemeral loopback ports.

## Design and Invariants

### CLI and token sources

- `ca serve`/`--stdio` remain byte-for-byte stdio. `ca serve --listen IP:PORT` selects WebSocket only when `IP` is a literal loopback address and exactly one `--auth-file PATH` or `--auth-env NAME` is supplied.
- `ca attach ws://IP:PORT[/path]` relays bounded JSONL stdin/stdout to/from one WebSocket using exactly one external auth source. The endpoint may contain no userinfo, query, or fragment and must resolve syntactically to a literal loopback address.
- Tokens never appear as CLI values. Files must be non-symlink regular files with no group/other permission bits, are read through a bounded no-follow handle, and contain one 32–1024-byte non-control token. Environment variable names are validated; values use the same bounds. Resolved tokens have only redacted Debug and are compared by fixed-size digest.

### WebSocket boundary

- Server bind validation occurs before I/O and rejects wildcard/private/public/hostname addresses. Handshake authentication uses `Authorization: Bearer …`, rejects browser Origin headers, never echoes token material, and starts no service/provider state on failure.
- One authenticated client is active at a time. Text frames map one-to-one to existing bounded JSONL operations/events; binary/fragment abuse, oversized frames, protocol errors, disconnects, and broken output close the connection and trigger exact service cleanup.
- Ping/pong/close are handled without becoming application operations. WebSocket Ready differs only by the safe transport label `websocket`; all subsequent service event shapes and authority rules remain unchanged.

### Attach bridge

- Attach performs authentication before relaying input, bounds/drains each local line using the existing 64-KiB rule, writes protocol JSON only to stdout, and sends diagnostics only to stderr.
- EOF, Ctrl+C/process termination, server close, stdout failure, and malformed frames close the socket; the server receives disconnect cleanup. Explicit `shutdown` remains the only way to receive the normal goodbye lifecycle reliably.
- Remote use is documented only as a user-created SSH tunnel terminating at loopback. `ca` neither opens SSH nor accepts non-loopback listeners.

## Milestones

### M1 — Characterization and auth primitives

- Claim the plan, add failing CLI/address/token/auth tests, and characterize unchanged stdio parsing/output.
- Add bounded redacted token sources and literal-loopback URL/listener validation.

### M2 — Server and attach bridge

- Add authenticated WebSocket accept/connect, single-client lifecycle, framing bridge, exact cleanup, and released CLI integration tests.
- Cover wrong/missing token, Origin rejection, busy connection, oversized/binary input, sequential reconnect, approval/interaction relay, cancellation, and zero persistence/leakage.

### M3 — Gates and disposable walkthrough

- Run focused/full gates and release smokes. Drive the released listener and attach bridge through authentication, one approved tool loop, status, cancellation, shutdown, and reconnect using only disposable roots.

## Acceptance Criteria

- A protected-file or environment token can authenticate `ca attach` to an exact loopback listener and carry the unchanged ordered `ca.serve.v1` lifecycle bidirectionally.
- Missing/wrong/weak/unprotected tokens, non-loopback addresses, URL credentials/query/fragment, browser Origin, binary/oversized frames, and a second concurrent client fail before widening authority or leaking secrets.
- Disconnect/cancel/shutdown close exact service-owned jobs/extensions; no service or attach path reads/writes saved sessions.
- Existing stdio service and ordinary CLI/headless/TUI behavior remain unchanged.

## Validation

Focused: PASS. Twenty service-filtered tests cover the existing protocol plus address/URL closure, protected file/environment tokens, redacted Debug, binary/oversized frames, and exact cleanup. The focused CLI integration covers wrong token, Origin rejection, active-client refusal, approved tool continuation, sequential reconnect, secret-free output, and zero persistence.

Completion: PASS. `cargo test --no-fail-fast` ran 997 unit tests plus 30 CLI integration tests; `cargo fmt --check`, strict Clippy, and `cargo build --release` passed. Release help, version, service-help, attach-help, profile, and invalid-address smokes passed.

Manual/visual: PASS. The released listener and attach bridge used file authentication, rejected a wrong token, carried 33 ordered events across approval/interaction/cancellation, reconnected with an environment token at sequence zero, stopped gracefully, kept daemon stdout empty, leaked no token/provider authorization, and created no sessions.

Blocked checks: none known.

## Progress

- [x] Existing service/CLI/dependency seams traced and SRV-081 archived.
- [x] Failing address/token/handshake/relay characterization added.
- [x] Authenticated server and attach bridge implemented.
- [x] Focused/full gates and released walkthrough pass.
- [x] Harness reconciled and plan archived.

## Decisions

- Decision: preserve `ca.serve.v1` and adapt WebSocket frames at the transport edge.
  Evidence: SRV-081 already characterizes service operations/events, policy, lifecycle, and cleanup independently of stdio's concrete byte handles.
  Consequence: clients see one application protocol across transports; transport security cannot alter runner semantics.
- Decision: accept only one authenticated WebSocket client at a time in this slice.
  Evidence: SRV-081 intentionally owns one process-only session/turn, while durable multi-session service state and reconnect identity are outside the roadmap item.
  Consequence: connection ownership and cleanup remain exact, and additional handshakes receive a safe busy refusal.

## Surprises and Discoveries

- The existing generic async reader/writer loop still hard-codes the Ready transport label. That must become an explicit transport argument without changing any stdio event bytes beyond the already-characterized `stdio` value.
- WebSocket shutdown must drain the protocol output pipe after the runner task finishes; selecting task completion directly could race and discard the already-buffered final `goodbye` event. The adapter uses output EOF as the terminal boundary.

## Recovery and Rollback

Remove the WebSocket dependency, listener/attach CLI branches, token/address primitives, and frame adapter with local patches. The stdio parser and `run_protocol` contract remain the stable fallback; no persistent data migration or external listener cleanup is required.

## Completion Record

Changed files: `Cargo.{toml,lock}`, `src/{main,service}.rs`, `tests/cli.rs`, `docs/SERVICE_PROTOCOL.md`, this ExecPlan, and the required harness/runtime documents.

Commands and results: focused service/attach tests PASS; full test PASS (997 unit plus 30 CLI); format PASS; strict Clippy PASS; release build PASS; help/version/profile/address smokes PASS; released disposable listener/attach walkthrough PASS.

Remaining risks: this slice intentionally supports one active WebSocket client, no durable reconnection/session identity, no TLS or non-loopback listener, and no automated SSH transport. An authenticated client still has only the service's configured tool/approval/workspace/sandbox/network authority.

Harness updates: `docs/.INDEX.md`, `docs/SERVICE_PROTOCOL.md`, `ENVIRONMENT.md`, `ARCHITECTURE.md`, `CONTINUITY.md`, `TESTING.md`, `NEXT_STEPS.md`, and `PARITY.md` now describe the released transport, authentication, validation, and extension-only parity status.
