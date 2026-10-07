# Responses WebSocket Readiness

Record: `PRV-228`
Status: DEFERRED — no stable implementation authorized
Checked: 2026-08-29

## Current Official Evidence

The official stable Responses reference documents `POST /responses` under the
ordinary `responses` resource:

- [Create a model response](https://developers.openai.com/api/reference/cli/resources/responses/methods/create)

The official WebSocket client/server event types and input injection behavior
are currently published under the explicitly Beta `beta/subresources/responses`
reference, with `beta_*` schema identities:

- [Beta Responses reference](https://developers.openai.com/api/reference/cli/resources/beta/subresources/responses)

Inference from those official source boundaries: the documentation does not
currently establish a stable Responses WebSocket wire contract equivalent to
CA's stable `/v1/responses` HTTP/SSE adapter. The Beta page demonstrates that a
WebSocket event design exists, but it is not evidence that its endpoint,
authentication, ordering, reconnect, or compatibility behavior is stable.

CA therefore ships no WebSocket provider dialect, command-line option,
configuration key, Cargo feature, endpoint assumption, or Beta event decoder.
This is an explicit deferral, not an implementation or parity claim.

## Evidence Required to Reopen

A separately authorized plan may reopen this work only after official OpenAI
documentation establishes all of the following as stable:

1. Wire identity and versioning
   - A non-Beta API resource and stable schema names.
   - The exact WebSocket endpoint, URI construction, negotiated subprotocol,
     supported models, and compatibility/version policy.
2. Authentication and connection authority
   - Required headers or other authentication during upgrade.
   - Organization/project scoping, credential rotation, redirect behavior, TLS
     requirements, proxy behavior, and maximum connection lifetime.
3. Request correlation and ordering
   - Stable client and server event unions, required IDs, sequence semantics,
     legal interleaving across responses, and exactly one terminal event.
   - Definitive behavior for text, reasoning, refusals, output items, errors,
     and usage when frames split arbitrary UTF-8 boundaries.
4. Tool continuation
   - Stable request/response events for client-owned tool results, validation,
     atomic commit, rejection, and correlation to an awaiting tool call.
   - Whether continuation can extend an existing response or must create a new
     response, including all replay and idempotency rules.
5. Cancellation and partial output
   - The exact cancellation event and acknowledgement, terminal status after a
     cancellation race, and whether already delivered output remains usable.
   - Proof that CA can cancel transport and provider work without executing or
     replaying a tool result twice.
6. Disconnect, reconnect, and resume
   - Server retention and resume tokens/cursors, replay window and ordering,
     duplicate-frame handling, expiration, and behavior after an uncertain
     disconnect.
   - A documented fail-closed path when resumption safety cannot be proven.
7. Usage and billing
   - Stable terminal usage placement and exact input, cached-input, output,
     reasoning, and total-token relationships.
   - Whether reconnect/replay/injection creates additional billable requests
     and how partial or cancelled responses are accounted for.
8. Errors, bounds, and service guarantees
   - Stable error classes, close codes, rate limits, frame/message limits,
     heartbeat or idle-timeout behavior, overload signaling, and retry safety.
   - Clear data-retention and no-replay guarantees adequate for CA's provider,
     privacy, trace, and usage boundaries.

## Future Acceptance Contract

Any future implementation must remain a provider adapter behind CA's canonical
stream events. It must add failure-first byte-split loopback fixtures for every
stable event, tool continuation, cancellation race, disconnect/reconnect,
duplicate frame, usage, malformed input, timeout, and no-replay case before any
live provider check. Existing HTTP/SSE Responses behavior must remain the
default and byte-compatible.

Live validation, new public flags/configuration, and provider traffic require a
new explicit authorization. PRV-228 itself made no API request and changed no
provider, credential, session, CLI, configuration, or Cargo-feature surface.
