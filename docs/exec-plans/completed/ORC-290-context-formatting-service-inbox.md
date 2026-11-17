# ORC-290 — Context pruning, formatting and service inbox

Status: COMPLETED AND INSTALLED (2026-10-15)

## Goal and authority
Implement the user-approved three-milestone plan from this chat. MCP orchestration is excluded. Preserve oracle/reference content, runtime configuration, production sessions and unrelated source changes. Standing post-task release authorization applies after validation.

## Established baseline
Current source is the workspace root, without Git metadata or .agent/PLANS.md. Follow the existing PLN-289 plan format. Before-files and SHA-256 manifest are preserved under scratch/ORC-290/. Reuse PLN-289 final-v3 gate evidence: 157 existing failures in each matrix, 60 Clippy diagnostics, existing Reviewer timeout; the raw gate remains failed. New regressions block release.

## Design
1. Automatic, deterministic outbound pruning only for known successful text tool results. Protect two newest human turns, skill/private/media/error/unknown content and tool pairing. Protect 25% of usable capacity capped at 40k tokens; require 10% savings capped at 20k. Unknown capacity uses 40k/20k. Canonical history and evaluation/summary requests remain intact. Freeze retries and account for the actual projection before compaction, snapshots and caching.
2. Disabled-by-default installed rustfmt/Prettier/Ruff presets and custom argv commands. Format candidate bytes through bounded owned processes before one atomic edit, approval, recovery and diagnostics. Preserve unformatted edits with visible formatter warnings. No implicit installation, shell mutations or external-tool edits. Cancellation before commit writes nothing.
3. Add explicit service inbox/persistent-session operations while preserving legacy ephemeral/user_input behavior. Durable admission and exact retry IDs, steering at safe provider/tool boundaries, queued input only when idle. 64 pending / 512 KiB aggregate. Native session writer leases, explicit resume, no automatic replay of interrupted requests, inert forks/imports.

## Milestones
- [x] M1: shared pruning and configuration/context reporting.
- [x] M2: formatting pipeline and recovery/approval/process coverage.
- [x] M3: service inbox, native persistence and protocol coverage.
- [x] M4: focused/actual-binary tests and full frozen gate; classify baseline.
- [x] M5: documentation, one release bump, background recoverable signed install and readback.

## Verification
Use disposable profiles/workspaces and loopback providers. Record failure-first tests, focused tests, all three adapters, unchanged canonical bytes, exact formatter receipts, service stdio/WebSocket/restart boundaries and actual TUI inspection. Run scripts/verify.sh once source is stable; preserve report.json, evaluation.json and raw logs. Do not weaken criteria. At most three repairs per failure.

## Progress and evidence
- Initial inspection confirms existing provenance is retained in canonical ToolCall status but lowered ChatMessage does not retain status. Pruning must carry host provenance through lowering rather than infer success from output text.
- Existing service sessions are process-only and legacy duplicate operation IDs are rejected; add explicit inbox IDs without changing that contract.

- Focused v1: 9 pass / one fixture-terminal-marker timeout; v2: 13 pass / same fixture failure. Backtrace identified the exact wait; supplying the missing ProviderEvent::Done preserves assertions and deadlines. Focused v3: 14/14 pass; expanded v4: 19/19 pass.
- All existing/new service tests: 28/28 pass, including existing released-v1 shape, approvals, interactions, networking, cancellation and WebSocket cases.
- Actual signed debug proof: scratch/ORC-290/runtime-debug-01/receipt.json; stdio/WebSocket rustfmt, native persistence, exact retries, held reopen, resume and TUI context inspection pass. The debug SHA is 174cdbda9c127472934f09ca8c9b658a89866c383ffc8dafd5bb14dda39db63a.
- Final review changed formatter authorization to use live caller policy and sandbox ceilings instead of saved global policy. This preserves child command grants and avoids weakening a later stricter policy. Final focus/gate and actual final-artifact proof follow.

- Frozen gate v1 was intentionally interrupted after default root evidence revealed a zero-savings context row shifting an established wide/narrow snapshot. Repair keeps the existing zero-reduction layout and displays pruning statistics when results are actually omitted; established snapshots remain unchanged. The actual-binary fixture is strengthened to create real pruning and assert a positive result count.
- V1 also observed the unchanged voice cancellation test fail; the exact same compiled test passed three isolated reruns. Preserve v1 evidence and check the final complete matrices before classification. Interruption receipt: scratch/ORC-290/interrupted-v1.json; voice-isolated.json.

- Final focused v3: 22/22 pass, adding exact savings-boundary, whole-result recent-budget and formatter cancellation/process cleanup coverage. Pruning capacity now uses the existing configured input ceiling (default 80% of known context window); no capacity keeps fixed limits.
- Actual debug v2 passes stdio, authenticated WebSocket and TUI with eight omitted results: scratch/ORC-290/runtime-debug-02/receipt.json. This precedes the final budget/immutable-admission-receipt refinement, so final candidate and installed checks must rerun the unchanged positive fixture.
- V1 source drift also includes concurrent TUI composer/input/render edits outside ORC-290; a temporary compile error in those edits was repaired externally. No original snapshot was changed by ORC-290. The final gate must report a stable source manifest.
- Admission receipts remain immutable across exact retries, which do not resave or reexecute work. Inspection separately reports lifecycle status.

- Final focused v4 passes all 22 tests with immutable retry receipts. Formatting passes on the frozen source copy. Final gate v2 runs from scratch/ORC-290/frozen-source using shared compiled artifacts and disposable runtime/dependency state; source snapshot receipt is scratch/ORC-290/frozen-source.json (8a35c304e01ee3e6e179104143c8107c587fe2cc363d414c41635b280affc314). Two formatting-only blocks in concurrent src/tui/input.rs were repaired locally and copied into the frozen source; the exact prior file is retained. Source otherwise remains frozen for this gate.

- Final call-site audit found direct subagent result appends lacked the host provenance carried by ordinary canonical lowering. Added the same known-success qualification and success status there; synthetic media remains protected. V2 was intentionally interrupted before expensive test matrices (scratch/ORC-290/interrupted-v2.json). V3 uses frozen source fb87589f9819a1c4acb91c40e9f7f9b0685626b8fb08c16b5938919d2465b435.

- V3 strict lint exactly matches all 60 baseline diagnostic identities (scratch/ORC-290/lint-comparison-v3.json). Default matrix observed one extra unchanged sandbox fixture failure: caller-supplied CARGO_TARGET_DIR pointed outside its workspace, correctly denied at .cargo-build-lock. Three exact same-artifact replays with only that variable removed pass; no policy/assertion/source changed. Raw gate failures remain failed and are separate from VAL-288 (scratch/ORC-290/cached-cargo-corrected-env.json).
- Supplemental native debug busy fixture passes stdio/authenticated WebSocket admission, steering, queue ordering and exact retries while a response is delayed. It also holds a write approval, verifies steering remains pending and no file exists, then verifies the admitted batch completes before steering enters the following provider request. Evidence: scratch/ORC-290/runtime-debug-busy-02/receipt.json. Final release/installed artifacts repeat this fixture independently of unchanged full-gate criteria.

- V3 default workspace completed but the driver classified the caller-induced sandbox denial as infrastructure failure and correctly stopped later stages. The three corrected-environment replays pass, but do not rewrite that failed gate. V4 removes CARGO_TARGET_DIR from the gate environment and reuses compiled artifacts through a target-directory link (outside source inventory); fixture Cargo builds now retain their own workspace-local target. No application/source/test/policy change. Additional full-run time was explained before proceeding.

- Concurrent composer task completed signed ca 0.1.24 installation (job 45272) while ORC-290 validates. Live/frozen inventory comparison differs only in Cargo.toml/Cargo.lock version metadata; all code and verification inputs match. Reuse that existing 0.1.23→0.1.24 advance under the standing no-double-bump rule. After the final gate, mirror version metadata into the frozen copy, assert exact live/frozen inventories, then run the authorized background rebuild/install from the real repository. Preserve the current signed 0.1.24 as the recovery copy. Existing concurrent receipt/log remains under scratch/composer-click/release/.

- Actual currently installed 0.1.24 (SHA 94679c2d9966301d755b909d4e6b3d0ff693448e39c07ddf2d71a0820067002a) passes both ORC-290 runtime fixtures: 30 main + 10 supplemental requests; seven pruned results in TUI, real rustfmt, persistent exact retries/reopen/resume, busy transport admission/ordering and outstanding approval hold. Receipts: scratch/ORC-290/concurrent-installed-runtime/receipt.json and concurrent-installed-busy/receipt.json. V4 gate and separate post-gate release/readback remain pending.

## Changed boundaries
New modules: src/context_pruning.rs, src/formatting.rs, src/service_inbox.rs.
Request/config/provenance integration: src/config.rs, src/image_content.rs, src/request.rs, src/agent/client.rs and its appended tests.
Edit/service/session integration: src/approval.rs, src/agent/tools.rs, src/agent/subagent.rs, src/headless.rs, src/main.rs, src/service.rs, src/session.rs, src/tui/render.rs.
Native runtime fixture: tools/orc290_runtime.py. Supplemental actual-binary busy/approval evidence stays under scratch/ORC-290/runtime-busy.py.
Documentation: architecture/parity/continuity/queue/testing, docs index/configuration/user/service protocol and this single ExecPlan. The only additional local prerequisite is two rustfmt-only blocks in concurrent src/tui/input.rs; functional composer changes belong to the separate composer task and remain preserved. No oracle/reference, runtime configuration or production session edits.

## Practical limits
Loopback requests, native formatting, deterministic models and estimated-token fixtures establish local behavior and byte preservation. They do not establish paid-provider quality, general economic benefit, cross-platform behavior or a long soak. Rustfmt is installed and exercised; Prettier/Ruff are not installed on this host, so their selection is covered by deterministic fixtures without installing tools. Existing VAL-288 failures remain unmodified.

- User confirmed the concurrent composer edits are finished. The last mechanical live/frozen inventory comparison has only the existing version metadata difference; continue final gate and post-gate release without further coordination.

- Actual installed formatter/LSP proof passes: published sample.rs equals rustfmt output and the exact LSP didOpen text; the fake server's diagnostic reaches the model tool continuation. Reuses the existing repository fake LSP server source in disposable TUI state. Receipt: scratch/ORC-290/installed-lsp-01/receipt.json. Final candidate/installed artifacts repeat it (two extra loopback requests each). No source or gate criteria change.

- Corrected V4 matrices complete on unchanged source: default 3,985 passed / 157 failed / 43 ignored, 29 targets; all features 4,141 passed / 157 failed / 44 ignored, 30 targets. Zero outer filtered cases. All 157 failed identities in each matrix and all 60 strict-lint diagnostics exactly match PLN-289/VAL-288; no new or removed failed identities. Existing evaluator parser excludes the one nested child summary. Mechanical comparison exits zero: scratch/ORC-290/baseline-comparison-v4.json; authoritative counts: matrix-totals-v4.json. Release build/runtime/evaluation still running.

- V4 completes all eight stages with stable source and unchanged criteria. All-feature workspace release passes in 257.862s. Official runtime retains exactly the existing Reviewer deadline failure and dependent resize not-run; seven other cases pass, including child exit/terminal restoration/artifact identity. Independent evaluator adds no diagnostic identity beyond PLN-289. Raw gate/evaluator remain FAILED. Explicit separate assessment preserves full_verification_passed=false and accepts the standing release because no new source regression is observed: scratch/ORC-290/release-assessment-v4.json.
- Keep the frozen validation tree immutable. Compare live release manifests to an in-memory version-only projection, rather than altering the original snapshot. The background job requires every other source/verification byte to match and rechecks source after building.

## Final release and acceptance
Background job 63911 exits zero. Reused the concurrent combined-source 0.1.23→0.1.24 advance; no second package bump. The original validation snapshot remains unchanged. Release source differs only in those version fields and is rechecked after building.

Signed candidate, staged and installed SHA-256: 8340c51628cef3c68e3c6dff013380bf5022da02936b1284c870c38f77a5ec96. Mode 0755, uid 501 / gid 20, valid-on-disk macOS signature, installed version/help and exact recovery hash all pass. Each candidate/installed artifact passes 42 loopback requests (30 main, 10 busy/approval, 2 formatter/LSP): seven pruned results in the actual TUI; real rustfmt; canonical native persistence and held reopen/resume; busy stdio and authenticated WebSocket admission, immutable retries, queue order and approval holds; final bytes in LSP notifications and diagnostics in tool continuation.

Readback: scratch/ORC-290/release/installed-readback.json. Background log: scratch/ORC-290/release-install.log. Candidate/installed runtime, busy and LSP receipts are in scratch/ORC-290/release/. Recovery: /Users/aji/.local/bin/.ca-release-stage.GSEznO/ca.recovery-ORC258-20261015T201057Z-94679c2d9966301d755b909d4e6b3d0ff693448e39c07ddf2d71a0820067002a.

Validated source: fb87589f9819a1c4acb91c40e9f7f9b0685626b8fb08c16b5938919d2465b435. Release source: 8bb5d74eeccf3901da0e49fb538a7f5335322b350e27cc8d0172320afd8fbb9b. Runtime configuration, production sessions, oracle/reference trees and running processes were preserved. Start a fresh CA process.

Full repository verification is still FAILED on VAL-288: 157 existing failures in each matrix, 60 existing strict-lint diagnostics, and the existing independent Reviewer timeout/blocked resize. No new failed identity or evaluator diagnostic. Earlier V1/V2 interruptions, V3 caller-environment failure and corrected replays remain preserved. All requested implementation milestones and applicable feature/artifact checks are complete; remaining repository repair task is VAL-288.
