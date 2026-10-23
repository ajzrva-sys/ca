# ca JSONL Service

`ca serve` and `ca serve --stdio` run the local `ca.serve.v1` protocol over stdin/stdout. `ca serve --listen 127.0.0.1:PORT` exposes the same application protocol as authenticated loopback-only WebSocket text frames, and `ca attach ws://127.0.0.1:PORT` bridges local JSONL stdin/stdout to it. Diagnostics use stderr. This protocol is a `ca` extension and is not an Ante or Cursor compatibility contract.

Sessions are ephemeral by default. Explicit persistent-session operations use the profile’s native session store; loading holds work until explicit resume.

## Transports and authentication

Stdio accepts one JSON object per bounded line and emits one JSON event per flushed line. WebSocket accepts and emits exactly one equivalent JSON object per text message; `ready.transport` is `stdio` or `websocket`. Binary, multiline, oversized, malformed, and browser-Origin frames are refused at the transport boundary.

WebSocket serving requires exactly one external token source: `--auth-file PATH` or `--auth-env NAME`. Attach requires its own matching source flag. Token values are never CLI arguments. Token files must be current-user-owned, non-symlink regular files with group/other access disabled; token content is bounded visible ASCII. Handshakes use an Authorization bearer header and fixed-size digest comparison. Token values, lengths, headers, and request URLs never enter service events or diagnostics.

Listener addresses and attach URLs must contain a literal loopback IP. Wildcard, hostname, LAN/public, URL-credential, query, fragment, `wss`, and non-loopback forms fail before network I/O. Only one authenticated WebSocket client is active at a time; a later client gets a safe busy refusal. A completed or disconnected client releases ownership so a new client gets a fresh connection lifecycle starting at sequence zero.

For remote use, create an SSH tunnel yourself and attach to its local loopback endpoint. `ca` does not open SSH, terminate TLS, or listen on non-loopback interfaces.

## Envelope

Every operation is strict JSON with an explicit schema and client-chosen operation ID:

```json
{"schema":"ca.serve.v1","id":"op_start","op":{"type":"start_session"}}
```

Every event has the same schema, one daemon UUID, a unique event UUID, a monotonically increasing sequence, an optional parent operation ID, and a tagged `event` object:

```json
{"schema":"ca.serve.v1","sequence":0,"service_id":"…","event_id":"…","event":{"type":"ready","transport":"stdio"}}
```

Operation IDs are unique safe ASCII strings of 1–128 bytes. A process accepts at most 4,096 IDs. Lines are capped at 64 KiB and a too-large line is fully drained before the next record. Unknown fields, unknown variants, invalid UTF-8/JSON, duplicate IDs, and state errors produce a bounded `error` event without desynchronizing the stream.

## Operations

- `start_session`: starts or replaces one idle process-only session. Optional fields are `provider`, `model`, `cwd`, `tools`, `include_tools`, `exclude_tools`, `show_thinking`, and `allow_network`. Provider/model must already be configured. Cwd cannot escape the startup workspace. Starting with `allow_network: true` also requires the daemon flag `--allow-network`.
- `user_input`: `{ "text": "…" }` starts one turn. Inputs are 1–48 KiB. A second input is rejected while busy and is never queued.
- `approval_response`: `{ "request_id": "apr_…", "decision": "allow_once|deny|abort" }` answers the exact live approval.
- `interaction_response`: `{ "request_id": "int_…", "selected": ["value"], "text": null }` answers the exact live structured interaction. Shape and values are validated against the request; invalid answers leave it pending.
- `cancel`: aborts the active provider/tool work and its owned jobs. Completed history plus the submitted user input remains available for the next turn.
- `status`: reports the optional active session and whether its turn is idle, running, approval-waiting, or interaction-waiting.
- `shutdown`: cancels active work, closes extensions and owned jobs, emits `session_ended` and `goodbye`, then exits successfully.

EOF performs the same graceful cleanup and emits `goodbye` when stdout remains connected.

## Events

Lifecycle events are `ready`, `session_started`, `session_ended`, `turn_started`, `turn_resumed`, `turn_completed`, `turn_failed`, `turn_cancelled`, `status`, `error`, and `goodbye`.

Runner activity is nested in a `runner` event using the existing `HeadlessEventV1` vocabulary: `run_start`, `turn_start`, `assistant_output`, optional `reasoning_output`, `tool_start`, `tool_finish`, `usage`, `completion`, and `failure`. A client should treat the outer `turn_completed`, `turn_failed`, or `turn_cancelled` event as the terminal boundary.

`approval_requested` supplies the exact request/call IDs, tool name, safe reason, and network flag. The preceding `tool_start` contains a bounded, terminal-safe, credential-field-redacted argument summary. `interaction_requested` contains the validated question, description, mode, choices, and `allow_other` flag; masked/secret interaction input is unsupported over stdio.

## Authority and persistence

Service provider calls use `PreparedModelRequest` and central tool selection. Unsupported TUI/work-state/subagent tools fail before provider traffic. Persistent Deny is automatic; an exact persistent Allow may execute non-network work. Other tool work and every permitted network request pauses until a matching client response. Approval never widens the selected workspace, configured tools, startup network upper bound, sandbox, or provider.

Ordinary service sessions are never written to the saved-session store. Explicit persistent sessions use native saved-session commits and writer ownership. Provider configuration, endpoints, headers, resolved credentials, OAuth values, and binary content never enter service envelopes or Debug output. Client disconnect, EOF, cancellation, replacement, and shutdown clean the exact service-owned provider/tool processes. `ca attach` is a transparent bounded bridge: JSON protocol data uses stdout, safe local connection diagnostics use stderr, stdin EOF closes the connection, and explicit `shutdown` is the reliable way to receive `goodbye` before close.

## Optional persistent sessions and input inbox

The released `start_session` and busy-rejecting `user_input` operations retain their existing behavior. The following operations are additive to `ca.serve.v1` on both stdio and authenticated WebSocket:

- `start_persistent_session`: accepts the same fields as `start_session`, creates a saved session, and returns `session_started` with its UUID.
- `load_persistent_session`: `{ "session_id": "UUID" }` loads a native saved session with current configuration and permissions. Loading performs no provider request and holds inbox work.
- `submit_input`: `{ "input_id": "client-id", "text": "...", "delivery": "steer" }` admits input independently of execution. Delivery defaults to `steer`; `queue` waits until execution would otherwise become idle.
- `inspect_inbox`: returns an `inbox` event with `held` and input receipts.
- `resume_inbox`: explicitly releases held pending inputs; interrupted inputs are not retried.

`input_admitted` includes the session UUID and an immutable admission receipt containing `input_id`, `delivery`, `status: pending` and `durable`. Persistent admission commits before acknowledgment. Ephemeral inbox receipts report `durable: false`. Exact input-ID retries return the same receipt without another save or execution; different text or delivery under that ID fails. Use `inspect_inbox` for current lifecycle status. Each protocol operation still needs a fresh envelope operation ID. Input IDs are scoped to one session.

Steering is delivered after an active provider response and any admitted tool batch finish, before the next provider request. It does not answer a tool approval or structured interaction. Pending steering keeps admission order; queued inputs are promoted one at a time at idle boundaries. Background inbox-driven turns can use `service-inbox` as their parent.

Input text retains the 48-KiB limit. Pending admission is bounded to 64 items and 512 KiB total, with 4,096 retained input IDs per session. Rejected admissions leave the inbox unchanged. Status is `pending`, `promoted`, `completed` or `interrupted`.

Disconnect, shutdown, failure and cancellation clean owned work and hold the remaining inbox. Reopen/resume does not automatically replay an uncertain provider request or tool effect. A caller can inspect an interrupted input and submit a new explicit continuation with a new ID. Admission/promotion identity does not promise exactly-once external effects. Native forks and portable imports do not acquire actionable inbox entries; private email results are scrubbed before saving.
