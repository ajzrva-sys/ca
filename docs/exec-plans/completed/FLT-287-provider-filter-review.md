# FLT-287 Provider filter independent review and repair

## Purpose and acceptance

Review the user-requested FLT-274 filter using independent source review and credentialless local regressions. Correct demonstrated defects in outbound rewriting, streamed stop detection, mid-stream arming, image-provider enforcement, restoration and provider-pane scrolling. Preserve runtime settings/sessions and oracle/reference bytes. No Cursor parity claim: acceptance follows the existing CA filter contract and user request.

## Baseline and repository state

The current application checkout is `/Volumes/projects/ca` (root Cargo.toml version 0.1.21). There is no Git metadata or nested `ca/` directory. `.agent/PLANS.md` is absent; this plan follows the existing purpose/progress/evidence pattern. Before edits, source inventory and relevant source/docs are retained at `scratch/filter-review/20261015T014453Z/before/` with `baseline-manifest.json`. The verification helper supplies disposable HOME/XDG/TMPDIR and a private dependency cache; no production config or session is read.

## Behavioral hypotheses

- Rewriting prompt text must preserve valid provider JSON and protocol identities.
- Stop phrases must be detected in assembled provider text even across SSE events or JSON escapes. Enabling during a stream must govern subsequent bytes.
- The exact rewritten image request must be transmitted, and a trip must stop its active I/O.
- Restoration must not reprocess already restored text/arguments on each delta or at multiple execution layers.
- Every word-wrapped provider-pane row must be reachable by scrolling.
- Accepted URL spellings and stop/hide validation must match documented behavior.

## Progress

- [x] Read harness entrypoints and FLT-274 contract; preserve baseline.
- [x] Three independent reviews identify boundary defects and reproduction cases.
- [x] Existing focused baseline and new failure-first regressions: original shared binary 41 passed / 9 new regressions failed; client-specific before binary 1 passed / 6 failed, with retained raw logs.
- [x] Narrow repairs, focused regression checks and independent review: initial integrated freeze `1c9e23f072bb904a1350452e765019eb8256c0fdac046aed2c4420e0c98c6b4c` passed 165 focused tests; final direct-append correction is frozen as `eda5cee531a33f6caf747bd40cd3bc6e1889ea010b618bb4c9414da7157b7e80` for `focused-after-03` and full v2 validation.
- [x] Full unattended verification completed; raw gate/evaluator remain failed on independently audited existing failures.
- [x] Finalize continuity/queue/testing with completed install outcome and unresolved baseline boundaries.
- [x] Patch bump and recoverable signed background install completed; all installed artifact/filter checks pass, full gate failures remain explicit.

## Ownership

Root owns image generation integration, verification orchestration and records. `filter_matching_review` owns provider_filter.rs. `filter_review` owns agent/client.rs and client_tests.rs. `filter_ui_review` owns provider pane, streaming restoration, main/workspace and only the initial restoration block in tools.rs. No tests/snapshots/evaluator criteria are weakened.

## Evidence and decisions

Pending focused baseline under `scratch/filter-review/20261015T014453Z/`. Keep all failed attempts and at most three automatic repairs of each failure. If full verification exposes unrelated defects, record them distinctly; do not claim a full pass from focused evidence.

### First integrated check

`focused-after-01.log`: 86 passed, 1 failed. Every new client, image, restoration and pane regression passed in that compiled source. The failure was the preexisting URL chunk test whose terminal token now requires an explicit EOF flush; its same final stop assertion was migrated to the corrected API. Source review additionally found escaped tool argument JSON beyond the carry window; a bounded incremental per-string decoder is being added before the final freeze. Raw evidence and all initial test failures remain retained.

The invalid Pages TOML example is reproduced in `docs-before.json` and corrected to one `[provider_filter.pages]` table with `stand_in = "latin"`. No config/session file was read or changed. Image asset downloads now share the process trip guard; returned provider asset bytes are scanned before saving.

### Final source freeze and focused acceptance

`focused-after-02.json` binds 165 passing tests to the same digest as `scratch/verify/FLT-287-final/report.json`, with `source_unchanged=true`. The final client scanner additionally passes 19 direct held-out production-helper cases. The full gate has passed preflight, verification contracts and workspace formatting; strict lint/default/all-feature/release/runtime/evaluation are still in progress. Do not infer completion or installation from these partial receipts.

The terminal filter walkthrough is prepared under the evidence directory and will run against the actual signed candidate. Installed baseline was freshly resolved and verified as `/Users/aji/.local/bin/ca`, version 0.1.21, SHA-256 `da659100de3050034edfa83dc918764d155ba41599a0c583b76a1513cd622153`, owner aji, mode 0755, valid code signature. No replacement has occurred yet.

### Final performance correction and validation restart

Review of the repaired stream path found duplicate full-buffer copies even with rewriting disabled or stop-only. The direct append path is restored; two regressions verify no raw duplicates and correct activation midway through existing text/arguments. The first full run was gracefully interrupted to validate this correction from one frozen source. Its unmodified report at `scratch/verify/FLT-287-final/` remains `infrastructure_failed` and incomplete; it is not completion evidence.

The second full run is `scratch/verify/FLT-287-final-v2/`, tool session 77449, frozen digest `eda5cee531a33f6caf747bd40cd3bc6e1889ea010b618bb4c9414da7157b7e80`. Independent reviews of the first run reproduced all 147 root test failures against the preserved original harness, all 10 integration failures against the prior signed installed binary, and attributed every strict lint diagnostic to preserved preexisting code. These receipts classify failures; they do not turn the aggregate gate into a pass. Compare v2 actual outcomes before assessing release readiness.

The disposable real terminal walkthrough `debug-filter-pty-02/result.json` passed all six checks: off/on report/banner, actual hidden outbound body and restored visible response, narrow pane, sticky stop/no subsequent socket, config unchanged, and terminal restoration. Its source was the earlier integrated freeze; repeat against the final release/installed artifact. An earlier harness timing failure remains retained at `debug-filter-pty/`.

`focused-after-03.json`: all 167 focused checks passed, no failures/ignores, source unchanged at the v2 freeze. Workspace verification contracts and formatting also passed on this freeze.

### Changed-file inventory

- `src/provider_filter.rs`: content-aware JSON rewriting/scanning, matching/EOF behavior, trip-file handling and core regressions.
- `src/agent/client.rs`, `src/agent/client_tests.rs`: live filter refresh, semantic streamed stop scans and bounded per-call escaped argument scanning with HTTP/SSE regressions.
- `src/agent/stream.rs`: once-only view/tool JSON restoration and assembler regressions.
- `src/agent/tools.rs`: restored execution arguments, actual filtered image-provider payload and guarded request/download I/O with HTTP regressions.
- `src/app.rs`, `src/main.rs`, `src/workspace.rs`: raw streamed state only when rewriting, once-only restoration before approval/execution and focused integration regressions.
- `src/jobs.rs`: genuine EOF stop scan before bounded output disposition and regression.
- `src/tui/provider_pane.rs`, `src/tui/render.rs`: exact rendered word-wrap scroll extent.
- `src/tui/input_tests.rs`, `src/tui/render_tests.rs`: state initialization and deterministic pane coverage; no snapshots changed.
- `docs/CONFIGURATION.md`: valid single Pages TOML table. `CONTINUITY.md`, `NEXT_STEPS.md`, `TESTING.md` and this plan retain review/verification facts. Cargo package version changes only if the standing release is installed.

### Remaining boundaries

The review uses loopback HTTP/SSE providers and isolated state; no real provider-account request or production session/configuration was exercised. The `/filter` report remains a snapshot and does not include trip reason/origin; the sticky trip banner still supplies that information. Minor Unicode summary-padding alignment was observed but is not an enforcement defect. These are observations, not newly imposed scope restrictions.

### Completed v2 default matrix comparison

`v2-default-comparison.json`: 29 expected targets completed, 3,929 passed / 157 failed / 43 ignored / 0 filtered. The failed case set and each assertion/snapshot signature exactly match the independently reproduced baseline, and v2 lint diagnostics match the independent baseline multiset. Full raw v2 gate remains failed; all-features, release/signing/runtime and evaluator are still pending.

### On/off/on regression found during final source read

The direct-append optimization incorrectly left an already allocated raw accumulator stale while rewriting was off. Re-enabling during the same response then discarded the off-period deltas from the rendered full copy. This was introduced by the review's optimization, not attributed to the original filter. A second independent reviewer confirmed the invariant: allocate raw only when rewriting first becomes active; once it exists, append each delta even while off; disabled visible updates remain direct appends without full copies.

V2 was gracefully interrupted for this correction; its raw report/evaluator are retained as `infrastructure_failed`, incomplete all-features. The completed default-matrix comparison remains valid evidence for that earlier freeze only. V3 will rerun the full workflow on the corrected freeze.

### Corrected final freeze

The exact same new on/off/on regression failed before and passed after, using an extracted unmodified handler and actual provider_filter test helper; both source/extraction hashes and logs are retained in `stream-toggle-{before,after}-receipt.json`. Only three off-branch appends and the new regression changed in `main.rs`. Formatting and an independent invariant review passed.

V3 full workflow started as tool session 10207 at `scratch/verify/FLT-287-final-v3/`, frozen source `8f1b3e7cfe238a5e2ef3d861739e28aa20bd3821ae6ce57feb7929d6a7cc6cc2` (`final-source-manifest-04.json`). A separate focused runner will execute filters directly from the actual freshly compiled v3 root harness, retain its hash and recheck the source digest; this avoids a duplicate compilation. The v3 separate readiness assessor remains scratch-only and retains all raw failure evidence.

`focused-after-04.json`: final actual v3 test harness passed 168 checks / 0 failed / 0 ignored in 1.56 seconds. Source was unchanged at the corrected final freeze. Receipt includes original/copied harness SHA-256, compiled-by log and focused log hash. V3 strict lint is independently confirmed identical to the 60 displayed baseline diagnostics.

### Completed frozen verification and independent assessment

V3 complete raw gate/evaluator remain **failed**, source unchanged. Default: 3,930 passed / 157 failed / 43 ignored, 29 targets. All features: 4,085 passed / 158 failed / 44 ignored, 30 targets. The original 157 failures and all 60 displayed lint diagnostics match independently reproduced baseline evidence. The additional all-feature Git fixture failure expects two clean-filter invocations but observes one; all 183 files in its dependency subgraph are unchanged, three exact-artifact replays passed, and a controlled Git timestamp experiment demonstrates count one/two with the same approved committed bytes. See `git-clean-filter-audit/audit-summary.json`.

Release build and candidate signing passed. Official runtime passed seven cases, failed the TUI automatic-review timeout and did not run resize afterward. The prior installed 0.1.21 reproduces that exact outcome with the same unchanged official helper. Current source has headless review-barrier integration but lacks its TUI counterpart. A separate candidate probe executes the exact unchanged official resize AST, passing both sizes with tool reply intact and clean terminal restoration. See `runtime-baseline-audit/audit-summary.json`; the official failed/not-run results remain intact.

The final signed candidate also passes six filter terminal checks with before/after candidate/helper SHA bindings (`gate-release-filter-pty-bound-execution.json`). Assessor `assess_frozen_release_v5.py` independently pins/rechecks the full source, case identities/signatures, baseline graph/replays, runtime vector, focused168, exact resize and actual filter HTTP/PTY artifacts. Six negative evidence probes reject changed/new failures and identities; actual evidence is accepted. `release-readiness-v5.json` is a separate standing-release assessment, explicitly `full_verification_passed=false`, and does not modify or pass the raw gate.

### Standing background release

Noninteractive build/install launched as tool session **5761**, log `scratch/filter-review/20261015T014453Z/release-install.log`. It will run the required patch bump once, require only Cargo.toml/Cargo.lock metadata changes, build the full workspace with all features, sign and check the candidate, perform filter PTY checks, preserve a timestamped hash-labelled prior binary and atomically replace it, then check candidate/staged/installed hashes, owner/mode, signature, version/help and installed filter PTY. A failed post-install check restores the prior binary. Outcome is pending; no installed replacement is claimed yet.

### Final installed outcome (2026-10-15)

Background job 5761 completed with exit zero. Required root patch bump was run exactly once (0.1.21 to 0.1.22). Full workspace/all-feature release build, candidate signature/version, release filter PTY, recoverable atomic installation, owner/mode/signature/version/help readback and installed filter PTY all passed. Running sessions and production runtime settings were left alone.

Candidate/staged/installed SHA-256: `4f01a503e8a32be194bc6cf154b43c1446506231ada04e61d78bf4b356019cbf`. Installed `/Users/aji/.local/bin/ca`, uid 501, gid 20, mode 0755. Preserved prior SHA-256 `da659100de3050034edfa83dc918764d155ba41599a0c583b76a1513cd622153` at `/Users/aji/.local/bin/.ca-automation-stage-v5bdkcc7/ca.recovery-automation-20261015T130359.527470Z-da659100de3050034edfa83dc918764d155ba41599a0c583b76a1513cd622153`. Readback: `scratch/filter-review/20261015T014453Z/installed-readback.json`; transaction: `install-receipt.json`; log: `release-install.log`. Release source `1c148e34891a4c6a3de379d97f9ec4c6065a9057546823ca03e52dec633b85ff` differs from validated source only in Cargo.toml/Cargo.lock version metadata; independent final readback confirms no other source change and both six-check release/installed terminal receipts.

Requested filter review/repairs and standing installation are complete. Full repository verification remains failed on the independently documented baseline issues; follow-up VAL-288 owns those existing gaps. No real provider-account test was performed.

Final count correction: the experimental reasoning binary declared 122 tests, with 121 passed and one explicitly ignored external-fixture check. Full all-feature totals already include that ignored case correctly.
