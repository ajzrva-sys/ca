# PRV-228 — Responses WebSocket Readiness Record

Status: COMPLETE (DEFERRED)
Owner/session: ORC-218 goal session
Last updated: 2026-08-29

## Goal

Record the exact evidence and acceptance conditions required before CA could
separately implement a stable Responses WebSocket provider dialect.

## Non-Goals

- No provider code, request, CLI/config/Cargo-feature surface, experimental
  endpoint, Beta protocol, authentication change, session change, or binary
  installation.
- Do not infer stable behavior from a Beta multi-agent event surface.

## Evidence and Baseline

- ORC-218 deliberately defers this capability because the cited official
  Responses WebSocket event reference is Beta rather than CA's stable
  `/v1/responses` contract.
- ORC-218 baseline: 868 records, SHA-256
  `2dcaf84dc86d3ae27bfea593bc811ce16f46f9782382780901fdcf890ff39978`.

## Readiness Questions

The record must require future official stable evidence for endpoint and
authentication, connection lifetime and event ordering, request correlation,
tool continuation, cancellation, reconnect/resume, usage/accounting, error
taxonomy, partial-output preservation, and replay/idempotency semantics.

## Validation

Confirm through official OpenAI documentation only that no stable contract is
currently available for CA to implement. Record dated source links and make no
network provider request. Verify no new code/config/CLI/feature surface exists.

## Progress

- [x] Child plan created after IMP-227 completion.
- [x] Official stable/Beta boundary rechecked and cited.
- [x] Deferred readiness record completed and reconciled.

## Completion Evidence

- Official documentation fetched on 2026-08-29 places ordinary creation under
  stable `responses` as `POST /responses`, while WebSocket client/server event
  types and input injection appear under `beta/subresources/responses` with
  `beta_*` schema identities. This supports deferral; it does not establish a
  stable WebSocket contract.
- `docs/RESPONSES_WEBSOCKET_READINESS.md` records the exact future stable
  endpoint/authentication, ordering, continuation, cancellation,
  reconnect/replay, usage/billing, error/bounds, and acceptance evidence needed
  to reopen the work.
- No provider source, dependency, feature, CLI/config schema, session format,
  credential path, request, runtime state, protected tree, oracle/reference
  file, or binary installation changed for PRV-228.

## Recovery

This child is documentation-only. Reverse only PRV-228 documentation hunks with
`apply_patch`; never modify providers, contact an API, or install a binary.
