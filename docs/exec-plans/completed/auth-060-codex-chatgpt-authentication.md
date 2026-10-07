# AUTH-060 — Codex/ChatGPT Authentication

Status: BLOCKED — no supported third-party provider-authentication flow is documented
Owner/session: current Codex task
Last updated: 2026-08-12

## Goal

Implement `/connect`, status, refresh, and `/disconnect` for Codex/ChatGPT subscription authentication only if an official supported flow authorizes a third-party local client such as `ca` to obtain and use those credentials against exact official model hosts.

## Non-Goals

- Reverse-engineering Codex OAuth client IDs, endpoints, scopes, token exchange, refresh behavior, or undocumented hosts.
- Reading, copying, importing, or depending on `~/.codex/auth.json`, the Codex keyring, browser cookies, or another application's cached credentials.
- Treating a Codex access token, browser token, or ChatGPT session as a general OpenAI API bearer token.
- Delegating `ca` provider requests through Codex CLI/app-server under this provider-auth plan; that would be a separate architecture and product decision.
- Changing the existing API-key, environment, Keychain, or credentialless provider paths.

## Official Evidence Reviewed

The fresh OpenAI Codex manual was retrieved on 2026-08-11 through the repository-required official documentation workflow.

The completion audit refreshed the same official manual on 2026-08-12. It still documents browser/device login only for the ChatGPT desktop app, Codex CLI, and IDE extension, and still directs general OpenAI API traffic to Platform API keys. It now more explicitly documents Codex access tokens for Codex CLI or app-server clients and describes app-server as the supported interface for deep third-party product integration. Neither addition publishes a third-party OAuth client registration, direct token audience, refresh contract, or official provider-host contract for `ca`; app-server remains the separately scoped architecture already identified below.

- The Authentication page documents ChatGPT browser/device login only for the ChatGPT desktop app, Codex CLI, and IDE extension. It documents `codex login`, automatic Codex token refresh, and Codex-owned auth storage; it does not publish a third-party OAuth client-registration or token-use contract: `https://learn.chatgpt.com/docs/auth`.
- The same page directs programmatic workflows to API keys and states that API-key usage uses standard OpenAI Platform billing.
- The Access tokens page limits Codex access tokens to trusted local workflows through Codex CLI or an app-server client. It explicitly says Platform API keys are for general OpenAI API calls and calls using a Codex access token for general API traffic the wrong credential type: `https://learn.chatgpt.com/docs/enterprise/access-tokens`.
- The official supported app-server surface could be evaluated under a separate adapter/service ExecPlan, but it does not authorize `ca` to own the ChatGPT OAuth tokens, bind them to direct provider HTTP requests, or reproduce Codex's login protocol.

## Blocking Decision

No official source reviewed defines a supported third-party OAuth client, public client identifier/registration process, scopes, token audience, refresh contract, or direct model endpoint for `ca`'s intended provider boundary. Therefore AUTH-060 must not implement `/connect` or token storage.

This satisfies the program's explicit stop condition: if no supported flow is verifiable, mark the plan blocked without an unofficial workaround.

## Source Changes

None. Existing provider authentication remains API-key, environment reference, profile-scoped Keychain reference, or credentialless operation.

## Validation

- Fresh official Codex manual fetch: PASS.
- Targeted Authentication and Access tokens review: PASS.
- Source/config/tests: unchanged by AUTH-060.
- Cargo gates: not rerun because this blocked evidence-only plan changes no Rust, config schema, tests, or runtime behavior; PRV-059's immediately preceding full gates remain the current repository baseline.

## Recovery and Future Unblock

Reopen AUTH-060 only when OpenAI publishes an official third-party client contract that explicitly permits this use, or after the user approves a separately designed official Codex app-server adapter that leaves authentication owned by Codex. Do not infer support from Codex implementation details or cached credentials.

## Completion Record

Changed files: this evidence-only ExecPlan and harness documentation.

Remaining risk: subscription authentication remains unavailable in `ca`; users must continue using supported provider credentials. This is deliberate and safer than an undocumented flow.
