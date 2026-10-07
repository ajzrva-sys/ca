# USG-122 — Provider-Account Usage

Status: COMPLETE
Owner/session: Codex / current reliability continuation
Last updated: 2026-08-16

## Goal

Add an explicit typed quota-adapter boundary and a separate Provider account
section to `/usage`, querying only configured supported official provider
hosts when the user opens that report while preserving every existing local
ledger calculation and label.

## Non-Goals

- No startup/background polling, retry loop, saved quota state, second OpenAI
  credential, undocumented MiMo balance endpoint, inferred unsupported quota,
  provider trace capture, or canonical/session/export history.
- No oracle/reference access or Cursor parity claim.

## Scope

Allowed: typed OpenAI and DeepSeek adapters, MiMo local-plan estimation,
explicit concurrent `/usage` orchestration, a 60-second process cache,
bounded sanitized unavailable/error states, pager loading/results, focused
loopback fixtures, snapshots, manual validation, and harness documentation.

Forbidden: credentials sent to overridden/unrecognized hosts, model requests,
user config mutation, installed binaries, oracles/reference, branches,
worktrees, stashes, intermediate commits, or unrelated ledger changes.

## Design Invariants

- Query only on explicit `/usage`, concurrently, with a ten-second per-adapter
  timeout, no retries, bounded bodies, and a 60-second process-only cache.
- OpenAI uses the configured credential only for official organization Usage
  and Costs APIs for the current UTC month; missing admin access is visibly
  unavailable. DeepSeek uses only its official balance endpoint. MiMo exposes
  configured Token Plan allocation and locally observed usage as an estimate.
- Never send credentials to custom/overridden hosts. Keep remote scopes
  incompatible and visibly separate from local `ca` ledger totals.
- Provider-account requests are non-model traffic and cannot enter OBS traces,
  sessions, exports, canonical history, or provider continuations.

## Milestones

1. Add failing adapter, host-policy, cache, command, and report tests.
2. Implement bounded typed adapters and concurrent explicit orchestration.
3. Integrate loading/result `/usage` states without changing local accounting.
4. Run focused, local, disposable-manual, and physical-FreeBSD gates.
5. Reconcile docs, archive this plan, and activate CTX-123.

## Validation

Focused: PASS. The initial red step produced the expected cache/report
characterization failures. Ten adapter tests now cover exact official-host
classification, arbitrary configured provider names, concurrent OpenAI
Usage/Costs and DeepSeek balance requests, bearer placement, bounded and
malformed responses, admin denial, timeouts, MiMo local estimates, cache
identity/expiry, deterministic ordering, and secret-free diagnostics. The
`/usage` orchestration test and eight reviewed wide/narrow loading, empty,
unavailable, and populated snapshots also pass.

Completion: PASS. Default tests pass 1,420 main and 37 CLI tests with one
documented ignore. All-feature tests pass 1,449 main, 118 experiment, and 37
CLI tests with two documented ignores. Formatting, strict all-feature Clippy,
all-feature release, release help/version, diff check, and snapshot hygiene
pass.

Manual/visual: PASS. A disposable 100x30 no-session TUI with a synthetic
credential and a deliberately overridden loopback OpenAI route displayed
loading, then the official-host refusal without network traffic, then the
60-second cached report on a second `/usage`. It exited cleanly; only the
fixture config existed, and the fixture was removed.

Physical FreeBSD: PASS on physical FreeBSD 15.1-RELEASE-p2 amd64 as
unprivileged `aji`. Focused tests pass; default passes 1,407 main and 37 CLI
tests; all features pass 1,436 main, 116 experiment, and 37 CLI tests, with
the two documented ignores. Native formatting, strict Clippy, all-feature
release, help/version, snapshot hygiene, x86-64 ELF identity, and linkage
pass. The exact source tree was removed; the shared build cache remains for
the remaining roadmap gates.

## Progress

- [x] Plan activated after TUI-121 completion.
- [x] Failing characterization tests added.
- [x] Typed adapters, cache, and `/usage` integration implemented.
- [x] Focused/local/manual/FreeBSD gates complete.
- [x] Harness reconciled and plan archived.

## Recovery

USG-122 stores no durable quota state. Source rollback removes only its adapter
module, process cache/orchestration, report section, tests/snapshots, and docs
while retaining OBS-117 through TUI-121.
