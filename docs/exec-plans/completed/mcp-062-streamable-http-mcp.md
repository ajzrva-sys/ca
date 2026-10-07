# MCP-062 — Streamable HTTP MCP

Status: COMPLETE
Owner/session: current Codex task
Last updated: 2026-08-11

## Goal

Extend the existing lazy MCP manager so an explicitly configured server may use MCP Streamable HTTP over HTTPS, or plain HTTP only to a literal loopback endpoint, without weakening project trust, distinct network grants, response/output bounds, cancellation, tool collision checks, or profile isolation.

## Non-Goals

- OAuth discovery/login/refresh/disconnect; `MCP-063` owns that only after this transport passes its security gates.
- Remote SSE compatibility, WebSocket MCP, arbitrary redirect following, proxy inheritance, URL query configuration, cookies, literal header secrets, runtime transport switching, or automatic server discovery.
- Deferred MCP tool discovery; `MCP-064` owns changing the current catalog-start behavior.
- A new MCP manager UI. Existing `/mcp` status/reconnect/trust/network-grant controls remain the surface.

## Evidence and Current Behavior

- `src/config.rs` currently models only `command`, `args`, `env`, `enabled`, `timeout_secs`, and `network`; omitted future fields would be rejected because `McpServerConfig` denies unknown fields.
- `src/mcp.rs` validates user/project definitions at manager construction, starts enabled servers lazily on catalog/resource/call demand, and currently creates only `TokioChildProcess` stdio transports.
- Project servers require exact workspace/hash trust. A server with `network = true` additionally requires trusted project network authority plus a process/server grant; user servers also require the process/server grant. The HTTP transport must reuse this exact gate.
- `RunningService<RoleClient, McpClientHandler>` is transport-neutral after `serve`, while cleanup fields (`stderr`, temporary directory, process group) are stdio-specific. This is the extraction seam.
- `rmcp = 3.1.2` already provides a client Streamable HTTP worker behind `transport-streamable-http-client-reqwest`; its default reqwest helper disables redirects but permits ambient proxies and may include response-body text in transport errors. `ca` therefore needs an owned bounded/safe HTTP client wrapper and must not forward raw transport error strings.
- The installed rmcp transport config supports a fixed URI, custom/bearer headers, per-SSE-event bounds, stateless operation, and one bounded expired-session reinitialization. These are source facts about a dependency API, not oracle parity evidence.
- Inferred: header values should be exact `${ENV_VAR}` references retained as references in config/manager definitions and resolved only inside the HTTP request boundary. This satisfies literal-secret rejection and avoids resolved values in management state.

## Scope

Allowed files: `Cargo.toml`, `Cargo.lock`, `src/config.rs`, `src/mcp.rs`, directly affected TUI status snapshots/tests, repository harness documents, and this ExecPlan.

Forbidden files/areas: `oracles/`, `reference/`, real config/Keychain/session state, provider protocol adapters, OAuth storage, installed binaries, and unrelated TUI/runner behavior.

Runtime state affected: only explicitly configured MCP connections. Automated validation uses loopback servers, temporary workspaces/homes, reference-only fake credentials, and no external service.

## Design and Invariants

- Add a defaulted `McpTransport` (`stdio` by omission, `streamable_http` opt-in) plus optional `url` and `headers` fields. Existing stdio TOML and serialized defaults remain unchanged; stdio rejects HTTP-only fields and HTTP rejects command/args/env process fields.
- Streamable HTTP requires `network = true`. It accepts `https://` endpoints with a host and `http://` only when the host is a literal loopback IP. URLs with user info, passwords, query strings, fragments, non-HTTP schemes, or missing hosts fail manager construction before any I/O.
- Header names must be valid and may not override Host, Accept, Content-Type/Length, Cookie, proxy, MCP session/protocol/event, forwarding, or connection-control headers. Every value must be exactly one `${UPPER_SNAKE_ENV}` reference; literal or interpolated values fail construction. A missing/invalid referenced value fails before the first request.
- The HTTP client uses no ambient proxy, no redirects, no cookies, bounded connect/operation deadlines, no transparent content decompression, a 1-MiB whole JSON response cap, and a 1-MiB raw SSE-event cap. HTTP failure bodies and authentication challenges are never retained or displayed.
- The client stores only endpoint plus header environment-variable names. It resolves header values for each request, applies `Authorization` as an exact configured header rather than inventing credentials, and drops resolved values with the request. Custom Debug output exposes only transport kind/server identity.
- `RunningServer` records only a typed transport kind. Stdio retains sandbox/process-group/stderr/temp cleanup; HTTP retains no child-process claims. Shared tool/resource discovery, schema/output bounds, name collision handling, refresh, timeout, call, cancellation, and `RunningService` shutdown remain one path.
- Remote protocol errors map to typed operation labels (`initialization`, `tool discovery`, `resource discovery`, `tool call`, `resource read`, `refresh`, `shutdown`) and never use rmcp/reqwest body, URL, header, or challenge strings. Server-returned tool/resource text still uses existing terminal, size, and secret-reference redaction boundaries.
- Existing `/mcp allow-network <server>` and `--allow-network` remain the only process network grants. Loopback does not bypass the grant. Reconnect closes the old session before establishing one replacement; shutdown/cancellation close HTTP workers promptly.
- Status output changes from the inaccurate `Local stdio MCP servers` heading to `MCP servers` and labels each row `stdio` or `streamable_http` without showing the URL or headers.

## Milestones

### M1 — Config and security characterization

- Add failing decode/validation tests proving old stdio config is unchanged and HTTP rejects unsafe schemes, non-loopback plaintext, URL credentials/query/fragment, missing `network = true`, process fields, literal/interpolated/missing header references, reserved header names, and unknown transport names before listener acceptance.
- Add safe Debug/status tests proving endpoint, header names/values, environment contents, challenges, and response markers cannot appear.

### M2 — Transport-neutral lifecycle

- Extract typed transport preparation/cleanup without changing the existing stdio fixtures.
- Enable rmcp Streamable HTTP support and add the owned no-proxy/no-redirect/bounded client.
- Reuse one post-serve discovery/call/resource/refresh path for stdio and HTTP.

### M3 — Hermetic HTTP behavior

- Add a raw loopback Streamable HTTP fixture covering initialize, initialized notification, tools/resources pagination, tool calls, resource reads, session headers, reconnect, and shutdown.
- Prove one explicit network grant, credentialless and referenced-header requests, literal/missing references with zero requests, redirect refusal, HTTP failure/body/challenge redaction, JSON/SSE/decompression/timeout bounds, malformed protocol handling, cancellation, stale catalog refresh, and collision behavior.
- Re-run all stdio MCP, headless MCP, tool-selection, approval, project-trust, and bare/profile tests.

### M4 — Delivery gates

- Review affected wide/narrow status snapshots; run focused/full tests, format, snapshot hygiene, strict Clippy, release, help/version/profile smoke, and a disposable TUI or headless walkthrough using a temporary home and loopback HTTP MCP server.
- Reconcile `ARCHITECTURE.md`, `CONTINUITY.md`, `ENVIRONMENT.md`, `NEXT_STEPS.md`, `PARITY.md`, `TESTING.md`, and `docs/.INDEX.md`; archive this plan before activating `MCP-063`.

## Acceptance Criteria

- Existing omitted-transport stdio configurations keep identical lazy startup, tool/resource behavior, sandbox/network policy, and cleanup.
- An explicitly granted safe loopback/HTTPS Streamable HTTP server exposes the same bounded dynamic tool/resource registry and can complete calls through the existing approval path.
- Every unsafe URL/header/network configuration fails before network I/O; redirects and proxies are never followed/used.
- HTTP bodies, URLs, header names/values, environment values, credentials, auth challenges, and decompressed oversized content never enter errors, status, Debug, snapshots, sessions, model-visible tool output, or logs.
- Cancellation, reconnect, shutdown, project trust, profile isolation, exact collisions, timeouts, and output bounds remain deterministic and hermetically tested.

## Validation

Focused: PASS. Twenty `mcp::tests` cover legacy stdio, safe HTTP config/Debug, exact project/network authority, missing-reference zero-I/O behavior, JSON/SSE bounds, SSE success, redirect/error/challenge redaction, timeout, catalogs, tool/resource calls, reconnect, sandbox, collision, cancellation, and cleanup. Existing config, headless MCP, restrictive selection, approval, and bare-mode regressions pass.

Completion: PASS. `cargo test --no-fail-fast` reports 717 unit and 28 CLI tests; `cargo fmt --check`, snapshot hygiene, `cargo clippy --all-targets -- -D warnings`, `cargo build --release`, help, version, and disposable bare-profile model listing all pass.

Manual/visual: PASS. Wide/narrow MCP status snapshots were reviewed with explicit transport labels. A released-headless disposable-home walkthrough initialized an environment-authenticated loopback HTTP MCP server, discovered and executed `mcp__remote__echo`, returned its result through the next provider request, and sent session deletion; no session was persisted.

Blocked checks: External HTTPS/MCP services are intentionally excluded; loopback fixtures own the contract.

## Progress

- [x] Active ExecPlan created after FAL-061 passed and was archived.
- [x] Config/security characterization complete.
- [x] Transport-neutral stdio/HTTP lifecycle complete.
- [x] Hermetic HTTP protocol and cancellation coverage complete.
- [x] Repository gates and isolated walkthrough pass.
- [x] Harness reconciled and plan archived.

## Decisions

- Decision: preserve `network = true` plus the existing process/server grant for every HTTP server, including loopback.
  Evidence: current `McpManager` already separates configuration intent, project trust, and process grant; bypassing one for a transport would widen authority.
  Consequence: merely adding a URL never initiates traffic.
- Decision: accept only exact environment references for headers and resolve them at the request boundary.
  Evidence: literal values would put secrets in TOML/manager Debug, while early config expansion would put resolved values in manager state.
  Consequence: MCP-062 supports common bearer/API-key headers without introducing OAuth/token persistence.
- Decision: own the reqwest wrapper rather than use rmcp's convenience client unchanged.
  Evidence: the dependency disables redirects but permits ambient proxies and can format response bodies/challenges in errors; `ca` requires no proxy, explicit body/event limits, and a secret-free error boundary.
  Consequence: dependency upgrade tests must characterize the wrapper/trait integration.

## Surprises and Discoveries

- The rmcp running service type is transport-neutral, so protocol/tool/resource logic need not be duplicated; only creation, secret resolution, safe error mapping, and cleanup differ.
- The current status heading claims every server is local stdio. It must become transport-neutral when the first remote transport ships.
- rmcp's convenience reqwest client already bounds individual SSE events but reads JSON bodies without a whole-body cap and may retain HTTP error bodies/challenges. The owned client therefore parses successful JSON through a 1-MiB streaming cap and maps every HTTP failure to a body-free local error.

## Recovery and Rollback

Use `apply_patch` to revert only MCP-062-owned config fields, HTTP client/transport branch, tests/snapshots, dependency features, and harness hunks. Because the new transport is opt-in and no migration or runtime write occurs, rollback needs no user-state cleanup.

## Completion Record

Changed files: `Cargo.toml`, `Cargo.lock`, `src/config.rs`, `src/mcp.rs`, `src/tui/commands.rs`, `src/tui/render_tests.rs`, two MCP status snapshots, this plan, and the reconciled harness documents.

Commands and results: focused MCP/config/headless/tool-selection/bare suites PASS; full 717+28 tests PASS; format/snapshot hygiene/strict Clippy/release/help/version/profile smoke PASS; released loopback HTTP MCP walkthrough PASS.

Remaining risks: External HTTPS interoperability is intentionally unclaimed; hermetic loopback fixtures own the contract. OAuth, authorization discovery, token refresh, deferred tool discovery, and remote compatibility negotiation remain MCP-063/MCP-064 work.

Harness updates: `ARCHITECTURE.md`, `CONTINUITY.md`, `ENVIRONMENT.md`, `NEXT_STEPS.md`, `PARITY.md`, `TESTING.md`, and `docs/.INDEX.md` now describe the default-preserving opt-in transport and its validation evidence.
