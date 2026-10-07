# MCP-063 — MCP OAuth

Status: COMPLETE
Owner/session: current Codex task
Last updated: 2026-08-11

## Goal

Allow an explicitly configured Streamable HTTP MCP server to authorize through a standards-based browser or advertised device flow, refresh and rotate credentials, report only typed safe status, and disconnect, with tokens stored under the exact active profile and server.

## Non-Goals

- OAuth for stdio MCP servers, provider OAuth, or importing another client's tokens.
- Automatic browser launch, authorization, or device polling merely from catalog discovery.
- Treating device authorization as core MCP parity; it is an optional RFC 8628 extension.
- Literal tokens, secrets, endpoints, authorization URLs, response bodies, or callback parameters in application state, snapshots, diagnostics, or sessions.

## Evidence and Current Behavior

- `src/mcp.rs` has a transport-neutral manager and hardened Streamable HTTP client, but a 401 is reduced to a safe terminal transport error and there is no credential state.
- `src/main.rs` exposes `/mcp`, reconnect, project trust, and network grants through one cancellable background task.
- `src/runtime.rs::ActiveProfile` already provides the immutable profile namespace used by provider credentials and sessions.
- rmcp 3.1.2 provides OAuth protected-resource/authorization-server discovery, PKCE S256, authorization-code exchange, refresh rotation, issuer validation, and an injectable `CredentialStore`/HTTP client. Its raw errors are not safe for display and must be mapped at the ca boundary.
- Current MCP authorization specification requires protected-resource metadata, OAuth/OIDC discovery, PKCE, resource indicators, bearer authentication, and refresh handling. Device authorization is not a core MCP requirement.

## Scope

Allowed files: `Cargo.toml`, `Cargo.lock`, `src/config.rs`, `src/mcp.rs`, a new `src/mcp_oauth.rs`, `src/main.rs`, `src/lib.rs`, `src/app.rs`, `src/tui/**`, focused tests/snapshots, active/completed plan and harness docs.

Forbidden: `oracles/**`, `reference/**`, real config/Keychain/session state, installed binary, unrelated formatting.

Runtime state affected: only an explicitly selected MCP OAuth Keychain item and process-local authorization state in production. Tests and walkthroughs use fake stores, loopback servers, and temporary homes.

## Design and Invariants

- Add a defaulted OAuth block valid only for `transport = "streamable_http"`. Browser is the default. Device flow requires an advertised device endpoint and a configured public `client_id`; confidential device clients are rejected in this slice.
- Validate the resource URL and optional issuer pin before I/O. Remote OAuth endpoints require HTTPS. Literal-loopback HTTP is allowed only when the MCP resource itself is literal loopback. Cross-origin discovery must match the configured issuer pin.
- OAuth and a configured `Authorization` header are mutually exclusive. Client secrets are exact environment references and are resolved only inside the explicit authorization worker.
- Store one bounded serialized credential record in Keychain service `ca.mcp-oauth`, account `server/<id>` for default/bare and `profile/<profile>/server/<id>` for named profiles. Bare can connect process-locally but does not persist credentials. Store interfaces and Debug output never expose credential contents.
- Normal catalog startup never opens a browser. A missing token becomes `authorization required`; `/mcp connect <server>` starts one cancellable flow. `/mcp refresh <server>` is explicit, while authenticated requests may perform one bounded automatic refresh. `/mcp disconnect <server>` closes the service and idempotently clears only that exact item.
- Browser flow binds `127.0.0.1` before constructing the redirect, uses PKCE S256 and exact state validation, opens without a shell, accepts one bounded callback, and times out. Device flow shows only verification host and user code, never device code; polling honors interval/slow-down and a hard deadline.
- Raw OAuth/rmcp/HTTP errors are reduced to typed safe outcomes. Challenges may cross only the private transport/auth boundary and are discarded before manager state/output.
- Successful connect/refresh/disconnect reconnects or invalidates only the selected server. Cancellation or stale generations cannot install credentials.

## Milestones

### M1 — Configuration and profile-scoped credential store

- Add validation tests for transport/issuer/client identity/flow/header conflicts and secret-free Debug.
- Add fake and macOS Keychain-backed credential stores with exact account isolation, bounded encoding, atomic replace, idempotent delete, and bare in-memory behavior.
- Enable only the rmcp OAuth feature needed by this plan.

### M2 — Safe OAuth client and authenticated transport

- Add pinned-origin OAuth HTTP behavior, bounded metadata/token bodies, no proxy/redirect, safe errors, stored credential initialization, bearer attachment, refresh rotation, and auth-required status.
- Characterize unauthenticated transport behavior and prove no tokens, endpoints, challenges, response markers, or body text cross the manager boundary.

### M3 — Explicit connect, refresh, and disconnect

- Add browser callback and optional advertised RFC 8628 device state machines with generation cancellation and deadlines.
- Extend `/mcp` grammar/status and TUI progress/failure surfaces; reconnect only the target server after success.
- Add wide/narrow snapshots for disconnected, waiting, device code, success, refresh failure, cancellation, and disconnected states.

### M4 — Completion and isolated walkthrough

- Run focused OAuth/config/MCP/command/snapshot tests, then all completion gates.
- Exercise a released binary against a loopback OAuth/MCP fixture and fake credential store or isolated Keychain abstraction, never the user's Keychain.
- Reconcile harness docs and archive this plan before MCP-064.

## Acceptance Criteria

- Legacy stdio and unauthenticated HTTP configs deserialize and behave unchanged.
- Invalid OAuth config fails before DNS/network/secret lookup; cross-origin issuer drift, redirects, URL secrets, and literal Authorization headers fail closed.
- Browser connect performs discovery, PKCE, exact callback validation, token exchange, persistence, authenticated MCP initialization, and refresh rotation against loopback fixtures.
- Advertised device flow polls exactly as bounded, handles pending/slow-down/denied/expired/cancelled states, and never exposes the device code.
- Missing/rejected/refresh-failed credentials produce safe actionable states; disconnect removes only the exact profile/server item and is idempotent.
- Two profiles using one server ID never load, refresh, or delete each other's credentials.
- No credential, endpoint, response body marker, authorization URL, state, verifier, device code, or credential length appears in Debug, manager state, snapshots, session/config files, or user-visible failure text.

## Validation

Focused: PASS. OAuth/config/store, MCP integration, command/input, cancellation, and snapshot suites cover safe validation, pinned discovery, browser/device authorization, refresh rotation, profile isolation, authorization-required startup, exact disconnect, stale generations, and secret-free rendering.

Completion: PASS. `cargo test --no-fail-fast` passed 733 unit tests plus 28 CLI tests; `cargo fmt --check`, snapshot hygiene, `cargo clippy --all-targets -- -D warnings`, `cargo build --release`, and help/version/profile smoke all passed.

Manual/visual: PASS. Wide/narrow browser, device, and failure snapshots were reviewed. A released `--no-session` TUI under a disposable home reached the loopback server's authorization-required state after explicit network grant, displayed only safe actionable status, made no MCP POST/token request, and did not open a browser or Keychain.

Blocked checks: none.

## Progress

- [x] Source and rmcp OAuth seams inspected; protocol/security decisions recorded.
- [x] Configuration and credential-store characterization tests pass.
- [x] Authenticated transport and safe failure tests pass.
- [x] Explicit browser/device/connect/refresh/disconnect tests and snapshots pass.
- [x] Repository-wide gates and isolated walkthrough pass.
- [x] Harness reconciled and plan archived.

## Decisions

- Decision: device authorization is optional RFC 8628 behavior, not MCP-core parity, and requires a pre-registered public client in this slice.
  Evidence: the current MCP authorization specification describes authorization-code plus PKCE; rmcp's auth module does not implement a device state machine.
  Consequence: unsupported servers fail safely and retain browser flow.
- Decision: a discovered cross-origin authorization server must equal an explicit configured issuer pin.
  Evidence: discovery is network authority and OAuth credentials are issuer-bound.
  Consequence: operator intent is inspectable and issuer drift cannot silently redirect tokens.
- Decision: default-profile MCP OAuth accounts retain an unscoped legacy-compatible form while named profiles include the profile component.
  Evidence: `ActiveProfile::secret_namespace` and provider Keychain isolation already establish this compatibility rule.
  Consequence: named-profile deletion cannot affect another profile.

## Surprises and Discoveries

- rmcp already implements refresh rotation and reactive challenge handling, but its errors may contain unsafe remote details and therefore cannot be surfaced directly.

## Recovery and Rollback

Keep the new config fields defaulted and OAuth opt-in. On failure, remove only the new module/call sites and feature flag with `apply_patch`; do not use destructive Git operations. Fake-store tests must prove failed exchanges leave the previous credential intact.

## Completion Record

Changed files: `Cargo.toml`, `Cargo.lock`, `src/config.rs`, `src/mcp.rs`, `src/mcp_oauth.rs`, `src/main.rs`, `src/lib.rs`, `src/app.rs`, `src/tui/commands.rs`, `src/tui/input.rs`, `src/tui/render.rs`, MCP/OAuth snapshots, this plan, and harness documentation.

Commands and results: focused OAuth/MCP/config/command/input/snapshot tests PASS; `cargo test --no-fail-fast` PASS (733 unit + 28 CLI); format/snapshot hygiene PASS; strict Clippy PASS; release build PASS; help/version/profile smoke PASS; isolated released-TUI loopback walkthrough PASS.

Remaining risks: real third-party authorization-server interoperability and macOS Keychain prompts were intentionally not exercised. Device flow support is limited to advertised RFC 8628 servers with a preregistered public client, and dynamic client registration depends on server metadata/support.

Harness updates: `ARCHITECTURE.md`, `CONTINUITY.md`, `ENVIRONMENT.md`, `NEXT_STEPS.md`, `PARITY.md`, `TESTING.md`, and `docs/.INDEX.md` now own the OAuth boundary and MCP-064 handoff.
