# HRS-293 — Paper-informed harness improvements

Status: completed and installed as signed ca 0.1.28. User approved the complete plan on 2026-10-16.

## Goal and boundaries

Complete TUI finalization review using the existing shared barrier, make later
checkpoints incremental, and compare current eager/lazy discovery without changing
its default. Preserve canonical history, permissions, session formats and running
sessions. No token calibration, storage redesign or unrelated VAL-288 repair.

The supplied source tree has no root Git metadata or `.agent/PLANS.md`; use the
existing completed-plan structure. Bounded before-images and hashes are retained
under `scratch/HRS-293/before/`. Do not modify oracle/reference trees.

## Implementation

1. Bind the existing process-local tracker to parent turns, mutation receipts,
   command captures and child receipts. Asynchronously review before any successful
   finalization side effects. Bind results to pane/turn/generation; retain three
   candidate attempts, current permissions and exact-byte acceptance.
2. Supply the previous validated summary plus newly covered canonical exchanges to
   later checkpoint requests. Full-prefix fallback for first/invalid/backward/focused
   requests. Derive integrity and metadata from canonical history and publish only
   through the existing session and generation checks.
3. Compare authorized catalogs, schema estimates, selection and activation behavior
   with deterministic/local-provider fixtures. Preserve eager as the default.

## Validation and release

Add focused regressions, then run existing terminal fixtures and the full unchanged
`./scripts/verify.sh`. Read raw reports/evaluation/logs and distinguish existing
VAL-288 failures from regressions. Preserve failed attempts; at most three repairs
per failure. Update harness evidence after stable-source validation.

Advance the package patch once, build the workspace/all-feature release in a managed
background job, sign and install recoverably with matching candidate/staged/installed
hashes, mode/owner/signature/version/help readback. Leave configuration/sessions alone.

## Progress

- [x] Read current implementation and retain before-images.
- [x] TUI barrier integration and focused regressions.
- [x] Incremental checkpoint generation and focused regressions.
- [x] Eager/lazy evaluation and terminal proof.
- [x] Completion gate, evidence classification, documentation.
- [x] Versioned signed release, recoverable install and installed readback.

## Interim evidence

`cargo check --bin ca` passes. Focused V2: 86 passed, zero failed/ignored,
covering checkpoint, review, command capture, discovery and pane modules. A final
pause/resume regression was then added; the full gate includes it. Debug binary
`6657d4ef0f503c28167aff1237f0dac567ca59e7864137a3b7bae12cf823213f`
passes all nine unchanged official terminal runtime cases, including actual file
write, separate reviewer read and structured approval, host-evidence integration,
wide/narrow resize, clean exit and terminal restoration.

`debug-features-v3/receipt.json` passes three compactions, canonical history and
recent-tail preservation, focused full rebuild and failed-quality preservation.
The actual eager/lazy task completes with the same `No child agents` tool result:
initial schema bytes 34,203 versus 13,246, and two versus three provider requests.
The deterministic three-task comparison uses identical authorized catalogs and
records 37,897 versus 14,713 bytes (conservative token estimates 22,071 versus
8,501). These are schema/fixture observations, not billed-token or model-quality
measurements. The default remains eager.

Preserved failures: debug feature V1 used a noncanonical temporary root in its
fixture trust configuration, so a trust overlay covered a correctly returned
reply. V2 then exposed a fixture assumption about inline messages; native V4 uses
a digest-bound external JSONL history. V3 reads and verifies that history instead
of weakening preservation assertions. No product behavior was changed for these
fixture corrections. Gate V1 was deliberately interrupted during lint before
updating the fixture; its infrastructure-failed report remains retained. Final
gate V2 reached the default workspace suite with all twelve new cases passing
and the same 157 observed failure names, then was interrupted for the compatibility
correction below. Its incomplete raw receipt is retained; it is not a completed gate.

## Essential folder-access compatibility

Inspection found a redundant saved-root check in shared candidate disk validation:
`resolve(Create)` already validates live identity-bound folder approvals, but the
second check rejected exports outside persisted roots. Remove that redundancy;
revoked/unapproved folders still fail the original resolver. Keep automatic
repository verification scoped to primary-repository paths, while independent
read-only review covers the complete candidate, including approved external files.
Do not infer compiler/test verification of external files. Add a grant/revocation
and mixed-scope regression and exercise actual folder approval plus reviewer read.
Final gate V3 will cover the stable correction with unchanged verifier criteria.

The corrected source passes 88 focused tests, zero failed/ignored. Debug artifact
`dc65b7ec6abddc09ab49d51736775e4b7eecaa1fcb442108a39c95cb37e8f296`
passes the unchanged nine-case official terminal fixture and all four feature
scenarios in `debug-features-v4/receipt.json`. The external scenario executes six
provider requests: grant, write, initial parent reply, separate reviewer read,
structured approval, and parent integration. The configuration digest stays
unchanged. The earlier artifact's external scenario remains failed with no reviewer
roundtrip, preserving the compatibility regression evidence. An obsolete exact
focused selector was interrupted before being counted; final focused evidence is V3.

## Final validation and delivery

Implemented and installed as signed **ca 0.1.28** (2026-10-16). TUI successful
finalization now waits for the shared asynchronous review barrier; later checkpoint
requests merge a validated prior summary with newly covered canonical exchanges.
Eager loading remains the default. The same loopback task uses 34,203 initial
schema bytes eagerly versus 13,246 lazily (61.27% smaller), with two versus three
provider requests and the same tool result. These are fixture observations, not
billed-token or model-quality measurements.

The 88 focused checks pass. Stable-source gate V3 completes with unchanged criteria:
117 verifier contracts, formatting, workspace/all-feature release and all nine
official terminal cases pass. Default: 4,042 passed / 157 failed / 43 ignored;
all features: 4,198 / 157 / 44. All 13 new regressions pass in both matrices.
The failing identities exactly match BUG-292 V5; strict lint has 55 remaining
diagnostic groups versus 60, with no new underlying items. Raw gate and evaluator
remain **failed**; VAL-288 remains open. No assertions or snapshots were weakened.

Background job **11688** exited zero. Signed candidate, staged and installed
SHA-256: `bc263f63718e8c99b865b9483364fb27872dbca42db418b3bc55d80d4f0a7270`; mode 0755, uid 501/gid 20. Candidate and installed binaries each
pass both the nine-case terminal fixture and four feature scenarios, including
repeated compaction, eager/lazy selection and approved external-folder review.
Recovery: `/Users/aji/.local/bin/ca.recovery-HRS-293-20261016T205138.889489Z-80cfdddb371e2150fd2c7f689495cbd9b9932d521dee6a9f3977ba45b119bc17`.
Readback: `scratch/HRS-293/release/installed-readback.json`; job log:
`scratch/HRS-293/release-install.log`. Only the exact Cargo package version fields
changed after the full gate. Production configuration/session and oracle/reference
content were left unchanged; running sessions were left intact. Start a fresh CA
process to use the release. Evidence: `scratch/verify/HRS-293-final-v3/`,
`scratch/HRS-293/final-assessment.json`. Plan: `docs/exec-plans/completed/HRS-293-paper-informed-harness-improvements.md`.

## Changed files

- `src/main.rs`: turn/receipt ownership, asynchronous finalization and incremental publication wiring, focused regressions.
- `src/automatic_review.rs`: honor live folder authority and scope repository verification, regression.
- `src/context.rs`: incremental prompt/generation and canonical metadata regressions.
- `src/tool_discovery.rs`: deterministic same-catalog comparison, default unchanged.
- `tools/hrs293_runtime.py`: credentialless native feature scenarios.
- `Cargo.toml`, `Cargo.lock`: root package patch 0.1.27 → 0.1.28 only.
- `ARCHITECTURE.md`, `PARITY.md`, `TESTING.md`, `CONTINUITY.md`, `NEXT_STEPS.md`, `docs/.INDEX.md` and this plan: mechanics, limits, status and evidence.

No HRS-293 work remains. Next independent task: VAL-288.
