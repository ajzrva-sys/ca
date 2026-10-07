# BUG-242 — Astra reasoning capabilities

Status: COMPLETE
Last updated: 2026-09-05

## Goal
Selecting gpt-6-astra shows reasoning medium and permits supported effort selection.

## Evidence and Current Behavior
The screenshot is failure evidence only. `apply_model_reasoning_default` uses the effective profile; `provider_contract::resolve` recognizes only GPT-5.6. Official https://developers.openai.com/api/docs/models/gpt-6-astra documents low/medium/high/xhigh/max, image input, and 1,050,000 API context. Local Codex model metadata reports default medium and 272,000 context. Its ultra entry describes automatic delegation, not an ordinary API effort; this repair uses documented wire efforts.

## Scope
Edit provider_contract.rs, focused main.rs/provider.rs tests, config.rs serialization, and task documentation. Preserve other working-tree edits. No oracle edits or config/session mutation. Install under standing authorization.

## Design and Invariants
Add an exact Astra Responses contract, with no none/minimal setting. Keep unknown/custom routes and explicit profiles unchanged. Use existing request serialization, picker and rendering paths.

## Milestones
- [x] Reproduce with focused status/picker test.
- [x] Add exact profile and test route boundaries.
- [x] Run focused/full tests, formatting, lint, release and recoverable install.

## Acceptance Criteria
Model selection displays reasoning medium, picker contains five supported efforts, and max selection succeeds. Unknown/custom endpoints receive no inferred contract. Explicit profiles retain precedence.

## Validation
See detailed results below. Existing broad working tree and recorded oracle baseline discrepancy are unrelated.

## Recovery and Rollback
Revert only this task's edits; install preserves a hash-labelled previous executable and rolls back failed verification.

## Completion Record
Release/install job 44274 completed successfully. All-feature workspace release,
candidate/staged/installed SHA-256, strict signature, mode/ownership, version and
help checks passed. Installed `/Users/aji/.local/bin/ca`; SHA-256
`f6af39cd7c1eb0d1d6c7bafec884af414cd055e2f69207a0103c37f908283bb4`. Recovery:
`/Users/aji/.local/bin/.ca-recovery-20260905T222628Z-hd8mj6id/ca.previous-d9cbe4aac3947fb79eba4510ecafba0f53513f526045fba5c91ce41c6efa88e6`.
Log: `/tmp/ca-bug242-release.log`. Independent installed hash/signature/version
checks passed. Running sessions and user config/session data were left alone.

Changed source: src/provider_contract.rs, src/config.rs; regressions in src/main.rs and src/agent/provider.rs. Updated continuity, queue, architecture, parity, testing and index documentation. No further BUG-242 work remains.

Discovery: failure-first test reproduced the exact unavailable status. Request regression also found XHigh serializes as x_high; canonical output is now xhigh, accepting legacy x_high on read.

Validation: focused 3/3, deterministic wide/narrow status renders, format,
strict Clippy, and diff whitespace pass. Broad no-fail-fast run: 3,228 passed,
two failed, six ignored; both unrelated privilege/process timeout tests pass
unchanged in isolation. All logs use /tmp/ca-bug242-*; TESTING.md records exact
commands. No live inference or manual terminal walkthrough. Background release
and recoverable install job 44274 completed. Initial temporary runner exited
before any mutation because system Python lacks hashlib.file_digest; portable
SHA-256 reading corrected the runner.
