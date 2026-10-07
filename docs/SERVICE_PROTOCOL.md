# ca JSONL Service

`ca serve` and `ca serve --stdio` run the local, process-only `ca.serve.v1` protocol over stdin/stdout. `ca serve --listen 127.0.0.1:PORT` exposes the same application protocol as authenticated loopback-only WebSocket text frames, and `ca attach ws://127.0.0.1:PORT` bridges local JSONL stdin/stdout to it. Diagnostics use stderr. This protocol is a `ca` extension and is not an Ante or Cursor compatibility contract.

## Transports and authentication

Stdio accepts one JSON object per bounded line and emits one JSON event per flushed line. WebSocket accepts and emits exactly one equivalent JSON object per text message; `ready.transport` is `stdio` or `websocket`. Binary, multiline, oversized, malformed, and browser-Origin frames are refused at the transport boundary.

WebSocket serving requires exactly one external token source: `--auth-file PATH` or `--auth-env NAME`. Attach requires its own matching source flag. Token values are never CLI arguments. Token files must be current-user-owned, non-symlink regular files with group/other access disabled; token content is bounded visible ASCII. Handshakes use an Authorization bearer header and fixed-size digest comparison. Token values, lengths, headers, and request URLs never enter service events or diagnostics.

Listener addresses and attach URLs must contain a literal loopback IP. Wildcard, hostname, LAN/public, URL-credential, query, fragment, `wss`, and non-loopback forms fail before network I/O. Only one authenticated WebSocket client is active at a time; a later client gets a safe busy refusal. A completed or disconnected client releases ownership so a new client gets a fresh process-only service lifecycle starting at sequence zero.

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

Service sessions are never written to the saved-session store. Provider configuration, endpoints, headers, resolved credentials, OAuth values, and binary content never enter service envelopes or Debug output. Client disconnect, EOF, cancellation, replacement, and shutdown clean the exact service-owned provider/tool processes. `ca attach` is a transparent bounded bridge: JSON protocol data uses stdout, safe local connection diagnostics use stderr, stdin EOF closes the connection, and explicit `shutdown` is the reliable way to receive `goodbye` before close.
