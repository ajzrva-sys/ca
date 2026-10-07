# CFG-034 — Provider Setup and Credential Checks

Status: COMPLETE (2026-08-10)

## Purpose

Add an explicit `/provider setup [filter]` walkthrough over configured providers. Each provider can be checked with one isolated, bounded chat request, repaired through the existing secure editor, and selected as the current/default provider only after a successful check. Provider checks are process-local and never contain resolved credentials or response content.

This is an oracle-informed `ca` extension, not a Cursor parity claim. Oracle and reference trees remain read-only.

## Scope

- Reserve `setup` under `/provider` and `/providers` while preserving normal provider filtering.
- Add a checklist, provider setup step, and passing-provider finish chooser.
- Add one-at-a-time typed provider probes with deterministic representative-model selection, cancellation, timeout, bounded output, and stale-result rejection.
- Make provider HTTP and transport failures safe across TUI, headless, scripted, checkpoint, and subagent paths; raw response bodies, URLs, and credentials must not cross the provider boundary.
- Preserve immutable `/retry` branching and guide authentication failures through setup before retry.
- Keep missing-Keychain startup recovery, persisted health history, provider presets, and per-model compatibility testing out of scope.

## Current State

`/provider` already owns safe raw summaries, provider/model detail, secure editor return paths, scoped model discovery, and provider deletion. Catalog status is stringly typed and does not prove chat authentication. `AgentClient` currently prints HTTP response bodies through `ProviderError::Display`, and unresolved environment placeholders can reach a normal chat request. `/retry` already creates an immutable child branch and will use a newly persisted model selection.

## Implementation

1. Introduce secret-free provider setup/check state and typed outcomes. Store results in `AppState` only, keyed by provider ID, with the tested model and generation.
2. Add a provider-probe boundary that accepts an owned resolved provider only inside the background task, sends a fixed user-only/no-tools request, consumes a complete bounded stream within 20 seconds, and returns only provider/model IDs plus a typed outcome.
3. Select a representative model in this order: current for provider, persisted default for provider, first configured, first live-discovered, documented fallback. No model means a local `NeedsModel` result.
4. Add `/provider setup [filter]`, checklist/detail/finish views, explicit Test/Edit/Add/Finish actions, cancellation, successful advancement, failure-in-place, setup-specific editor return targets, and default selection through the existing atomic model-selection path.
5. Replace raw HTTP/transport display with typed safe summaries. Omit Authorization when no credential exists, reject unresolved environment placeholders before I/O, and make 401/403 failure copy name the provider and direct the user to `/provider setup <provider>` and then `/retry`.
6. Invalidate only affected process-local results after provider/model mutations, ignore stale probe results, and remove deleted-provider results.
7. Add focused tests, wide/narrow snapshots, and an isolated temporary-home/loopback TUI walkthrough. Reconcile the repository harness after all checks pass.

## Acceptance

- No provider network request occurs merely by opening setup.
- One explicit check makes at most one chat request and never includes conversation history, tools, system prompts, images, or reasoning options.
- Successful checks appear in the finish chooser and the chosen pair becomes current/default without disturbing unrelated raw TOML.
- Missing environment variables perform no I/O. Credentialless local/public providers send no Authorization header.
- 401, 403, timeout, connection, route, rate-limit, provider, malformed, premature, and oversized failures are distinct and contain no provider body, URL, key, or generated text.
- Repairing a provider, selecting it, and running `/retry` retries the original user request in a child session without mutating the source.

## Validation

- Focused command, input, config, probe, retry/headless, orchestration, and render suites.
- `cargo test --no-fail-fast`
- `cargo fmt --check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo build --release`
- `./target/release/ca --help` and `./target/release/ca --version`
- Isolated rebuilt-TUI walkthrough using a temporary home, fake/local credentials, and loopback providers only.

## Progress

- [x] Repository and oracle seams inspected; product decisions locked.
- [x] Typed safe probe and error boundary implemented.
- [x] Setup walkthrough and finish selection implemented.
- [x] Focused tests and snapshots reviewed.
- [x] Completion gates and isolated TUI walkthrough passed.
- [x] Harness reconciled and plan moved to completed.

## Decisions

- Provider checks are explicit and process-local.
- One representative model is used only to validate the provider/credential path; no model matrix is produced.
- A completed valid provider stream is required for `Working`; HTTP 200 alone is insufficient.
- Results never persist because stale green checks are misleading.
- Missing referenced Keychain recovery remains a separate task because it fails before normal TUI construction.

## Outcome

Implemented the setup checklist/detail/finish state machine and a dedicated background probe boundary. Probes are user-triggered, one-at-a-time, cancellable, generation-checked, limited to a fixed user-only completion request, and considered working only after a valid completed stream. Only provider/model IDs and typed outcomes return to application state.

The shared provider boundary now drops HTTP bodies and classifies transport failures before display. Unresolved environment references fail locally, credentialless providers omit Authorization, runtime client debug output is redacted, and interactive authentication failures direct users to setup before explicit immutable `/retry`.

Validation passed `cargo test --no-fail-fast` (376 unit + 20 CLI), `cargo fmt --check`, strict Clippy, release build, and local help/version. Focused provider tests passed 57 unit + 1 matching CLI; render passed 45 and input passed 47. A rebuilt TUI used a temporary home and loopback providers to verify authenticated success, credentialless success, missing-environment local failure, and 401 rejection without displaying the fake response marker. The fake home was inspected and moved to Trash; no real runtime state, Keychain, session, oracle/reference file, or installed binary changed.
