# AUTH-061 — Codex Auth Cache Integration

Status: COMPLETE
Owner/session: completed Codex task
Last updated: 2026-08-14

## Goal

Allow the `openai-codex` provider to use a ChatGPT subscription session from the
documented Codex `auth.json` cache on macOS and FreeBSD, without requiring a
separate Codex runtime on FreeBSD.

## Non-Goals

- General-purpose import of browser cookies, keychains, or arbitrary credentials.
- Printing, logging, committing, or embedding token values.
- Installing the Codex CLI or app-server on FreeBSD.
- Changing authentication for providers other than exact `openai-codex`.

## Evidence and Current Behavior

- Official OpenAI authentication documentation says Codex may cache credentials
  at `~/.codex/auth.json`, automatically refreshes ChatGPT sessions, and documents
  copying that file to the same location on a trusted headless machine.
- Local `/Users/aji/.codex/auth.json` exists, is owned by `aji`, and has mode
  `0600`; only its JSON field paths were inspected.
- `src/codex_auth.rs` currently loads only a private macOS Keychain entry and
  returns no credentials on FreeBSD.
- `src/main.rs::resolve_codex_oauth_provider` injects only an access token.
- `src/agent/client.rs` currently has no exact-provider path for the Codex account
  header associated with a ChatGPT subscription token.

## Scope

Allowed files: `src/codex_auth.rs`, `src/agent/client.rs`, focused tests,
`ENVIRONMENT.md`, `CONTINUITY.md`, `NEXT_STEPS.md`, `TESTING.md`, and this plan.

Forbidden areas: API-key storage, browser cookies, unrelated provider adapters,
oracles, and unrelated dirty-worktree changes.

Runtime state affected: with the user's explicit authorization, the existing
Codex cache is copied opaquely to `/home/aji/.codex/auth.json` with owner-only
permissions. Token refresh may atomically update that same cache.

## Design and Invariants

- Resolve `CODEX_HOME/auth.json`, falling back to `$HOME/.codex/auth.json`.
- Refuse symlinks, non-regular files, files not owned by the effective user, and
  group/world-accessible files.
- Deserialize only in process memory and never include token values in errors or
  debug output.
- Preserve unrelated top-level JSON fields while atomically persisting refreshed
  tokens with mode `0600`.
- Add the account header only for exact provider ID `openai-codex` and only when
  the account ID belongs to the currently injected access token.
- Retain the existing macOS Keychain entry as a compatibility fallback.

## Milestones

### M1 — Characterize and document

- Add focused cache parsing, permission, and provider-header tests.
- Revise the prior policy decision to permit the explicitly approved documented
  cache integration.

### M2 — Implement

- Add safe Codex cache resolution, load, refresh persistence, and deletion.
- Apply the exact Codex account header at the HTTP boundary.

### M3 — Validate locally and on FreeBSD

- Run focused tests and repository completion gates.
- Copy source to a disposable FreeBSD directory, build, and perform an authenticated
  minimal provider request without exposing credentials.

## Acceptance Criteria

- A secure standard Codex cache makes `openai-codex` resolve as authenticated.
- Unsafe cache permissions or ownership fail closed.
- Exact Codex requests include bearer auth and the matching account header;
  other providers do not.
- The cache remains owner-only after refresh persistence.
- Native FreeBSD authentication succeeds using `/home/aji/.codex/auth.json`.

## Validation

Focused: PASS — cache security/parsing/refresh, account header, Codex request
shape, metadata resolution, and request-ID tests.

Completion: PASS — 1,242 unit tests, one accepted manual ignore, 36 CLI tests,
formatting, strict Clippy, release, and released help/version.

Manual/network: PASS — native unprivileged permanent-home request returned
exactly `FINAL_AUTH_OK`.

## Progress

- [x] User explicitly approved revising the prior cache restriction.
- [x] Official documented headless cache-copy behavior confirmed.
- [x] Focused tests added.
- [x] Secure cache integration implemented.
- [x] Local completion gates passed.
- [x] FreeBSD authenticated check passed.

## Decisions

- Decision: use the documented Codex cache directly rather than require a
  separately built Codex app-server.
  Evidence: the user explicitly relaxed the repository restriction, and OpenAI
  documents the cache-copy flow for trusted headless machines.
  Consequence: `ca` shares Codex's login lifecycle and must protect the file as a
  password.

## Surprises and Discoveries

- The existing uncommitted OAuth implementation is macOS-Keychain-only and does
  not propagate the ChatGPT account ID to provider requests.

## Recovery and Rollback

Remove the cache-loading branch and exact-provider account header. The copied
FreeBSD cache can be removed independently without changing other `ca` state.

## Completion Record

Changed files: `src/codex_auth.rs`, `src/agent/client.rs`,
`src/agent/client_tests.rs`, `src/agent/provider.rs`, `src/main.rs`, harness
documentation, and this plan.

Commands and results: focused tests PASS; full local gates PASS; native FreeBSD
release PASS; owner-only cache/config checks PASS; authenticated native turn
PASS with exact output `FINAL_AUTH_OK`.

Remaining risks: the FreeBSD host uses explicit policy-only enforcement because
no supported process-isolation backend is present. The shared cache follows the
Codex login lifecycle and must continue to be treated as a password.

Harness updates: `ENVIRONMENT.md`, `CONTINUITY.md`, `NEXT_STEPS.md`, and
`TESTING.md` record the approved boundary and evidence.
